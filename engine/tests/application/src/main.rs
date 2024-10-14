use std::{
    any::TypeId,
    num::NonZeroUsize,
    sync::{Arc, OnceLock},
    thread,
};

use async_ffi::FutureExt;
use bubble_core::os::{thread::Context, SysThreadId};
use bubble_tasks::{dispatcher::Dispatcher, runtime::Runtime};
use dlopen2::wrapper::{Container, WrapperApi};
use shared as sd;
use shared::TaskSystemApi;

#[derive(WrapperApi)]
pub struct PluginApi {
    load_plugin: fn(context: &Context, task_system_api: &shared::TaskSystemApi),
    test_dynamic_tls: fn(),
    plugin_task: fn(s: String) -> async_ffi::FfiFuture<String>,
    test_typeid: fn(ids: (TypeId, TypeId, TypeId)),
}

pub fn test_dynamic_tls(container: &'static Container<PluginApi>) {
    let handle = thread::spawn(|| {
        // use bubble_core TEST_VAR as a example
        let x = {
            shared::TEST_VAR.with(|x| {
                let mut guard = x.lock().unwrap();
                println!("host:{}", *guard);
                assert_eq!(*guard, 0);
                *guard = 42;
                println!("host:{}", *guard);
                assert_eq!(*guard, 42);
                drop(guard);
            });
            0
        };
        // test_func
        (container.test_dynamic_tls)();
        let x = {
            shared::TEST_VAR.with(|x| {
                let mut guard = x.lock().unwrap();
                println!("host:{}", *guard);
                assert_eq!(*guard, 43);
                *guard = 42;
                println!("host:{}", *guard);
                assert_eq!(*guard, 42);
                drop(guard);
            });
            0
        };
    });

    handle.join().unwrap();
}

pub fn test_bubble_tasks(container: &Arc<Container<PluginApi>>) {
    println!("(StdId)main_thread: {:?}", std::thread::current().id());
    println!("(SysId)main_thread: {:?}", SysThreadId::current());
    println!("(StdName)main_thread: {:?}", std::thread::current().name());
    let container_clone = container.clone();
    let r = TASK_DISPATCHER
        .get()
        .unwrap()
        .dispatch(|| async move {
            println!(
                "(StdId)main_thread(dispatch): {:?}",
                std::thread::current().id()
            );
            println!("(SysId)main_thread(dispatch): {:?}", SysThreadId::current());
            println!(
                "(StdName)main_thread(dispatch): {:?}",
                std::thread::current().name()
            );
            println!("(RuntimeName)main_thread(dispatch): {:?}", Runtime::name());
            let x = container_clone.plugin_task("Cargo.toml".to_string()).await;
            println!("{:?}", x);
        })
        .unwrap();

    let runtime = Runtime::new().unwrap();
    let _ = runtime.block_on(r);
}

pub fn test_typeid(container: &Container<PluginApi>) {
    let ids = (
        TypeId::of::<String>(),
        TypeId::of::<shared::StringAlias0>(),
        TypeId::of::<sd::StringAlias1>(),
    );
    (container.test_typeid)(ids);
}

static CONTAINER: OnceLock<Arc<Container<PluginApi>>> = OnceLock::new();
static TASK_DISPATCHER: OnceLock<Dispatcher> = OnceLock::new();

fn main() {
    // get current working directory
    let cwd = std::env::current_dir().unwrap();
    println!("Current working directory: {:?}", cwd);
    let dll_name = [cwd.join("./target/debug/plugin.dll")];

    let container =
        CONTAINER.get_or_init(|| Arc::new(unsafe { Container::load(&dll_name[0]) }.unwrap()));

    // get context, and prepare apis
    let context = Context::get();
    const THREAD_NUM: usize = 8;
    let _dispatcher = Dispatcher::builder()
        .worker_threads(NonZeroUsize::new(THREAD_NUM).unwrap())
        .thread_names(|index| format!("compio-worker-{index}"))
        .build()
        .unwrap();
    let _ = TASK_DISPATCHER.set(_dispatcher);
    let task_api = TaskSystemApi {
        dispatcher: TASK_DISPATCHER.get().unwrap(),
        spawn: bubble_tasks::runtime::spawn,
        dispatch: |f| {
            async move {
                bubble_tasks::runtime::Runtime::with_current(|r| {
                    r.spawn(async move {
                        // make a non-send future into a send future
                        // so that it can be sent to another thread
                        TASK_DISPATCHER
                            .get()
                            .unwrap()
                            .dispatch(move || async move { f().await })
                            .unwrap()
                            .await
                            .unwrap()
                    })
                })
                .await
                .unwrap()
            }
            .into_ffi()
        },
    };

    (container.load_plugin)(&context, &task_api);

    // TEST1
    println!("--TEST1->");
    test_dynamic_tls(&container);
    println!("<-TEST1--");

    // TEST2
    println!("--TEST2->");
    test_bubble_tasks(&container);
    println!("<-TEST2--");

    // TEST3
    println!("--TEST3->");
    test_typeid(&container);
    println!("<-TEST3--");
}

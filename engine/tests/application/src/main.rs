use std::{num::NonZeroUsize, sync::OnceLock, thread};

use async_ffi::FutureExt;
use bubble_core::os::thread::Context;
use bubble_tasks::{dispatcher::Dispatcher, runtime::Runtime};
use dlopen2::wrapper::{Container, WrapperApi};
use shared::TaskSystemApi;

#[derive(WrapperApi)]
pub struct PluginApi {
    load_plugin: fn(context: &Context, task_system_api: &shared::TaskSystemApi),
    test_func: fn(),
    plugin_task: fn(s: String) -> async_ffi::FfiFuture<String>,
}

fn main() {
    // get current working directory
    let cwd = std::env::current_dir().unwrap();
    println!("Current working directory: {:?}", cwd);
    let dll_name = [cwd.join("./target/debug/plugin.dll")];
    let container: Container<PluginApi> = unsafe { Container::load(&dll_name[0]) }.unwrap();

    // get context
    let context = Context::get();

    // print the two function inside Context

    // TEST1
    //
    // (container.load_plugin)(&context);
    // let handle = thread::spawn(move || {
    //     // use bubble_core TEST_VAR as a example
    //     let x = {
    //         shared::TEST_VAR.with(|x| {
    //             let mut guard = x.lock().unwrap();
    //             println!("host:{}", *guard);
    //             *guard = 42;
    //             println!("host:{}", *guard);
    //             drop(guard);

    //             // println!("{x}");
    //         });
    //         0
    //     };
    //     // test_func
    //     (container.test_func)();
    //     let x = {
    //         shared::TEST_VAR.with(|x| {
    //             let mut guard = x.lock().unwrap();
    //             println!("host:{}", *guard);
    //             *guard = 42;
    //             println!("host:{}", *guard);
    //             drop(guard);

    //             // println!("{x}");
    //         });
    //         0
    //     };
    // });

    // handle.join().unwrap();

    // TEST2
    //
    // bubble-tasks test
    const THREAD_NUM: usize = 8;
    static TASK_DISPATCHER: OnceLock<Dispatcher> = OnceLock::new();
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

    println!("main_thread: {:?}", std::thread::current().id());

    let r = TASK_DISPATCHER
        .get()
        .unwrap()
        .dispatch(|| async move {
            println!("main_thread(dispatch): {:?}", std::thread::current().id());
            let x = container.plugin_task("Cargo.toml".to_string()).await;
            println!("{:?}", x);
        })
        .unwrap();

    let runtime = Runtime::new().unwrap();
    let _ = runtime.block_on(r);
}
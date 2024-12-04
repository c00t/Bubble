#![feature(async_closure)]

use std::{
    any::TypeId,
    ops::Deref,
    sync::OnceLock,
    time::{Duration, Instant},
};

use bubble_core::{
    api::prelude::*,
    os::{thread::dyntls::Context, SysThreadId},
    sync::circ,
    tracing::{self, info, instrument, warn},
};
use bubble_tasks::futures_util::{self, stream::FuturesUnordered, StreamExt};
use bubble_tasks::{
    async_ffi,
    async_ffi::{async_ffi, FutureExt},
    types::TaskSystemApi,
};
use bubble_tasks::{
    io::{AsyncReadAt, AsyncReadAtExt},
    runtime::Runtime,
};

// A singleton plugin, or use Mutex to protect the global plugin.
//
// The sync primitive you use should be compatible with your expected use case.
static PLUGIN: OnceLock<circ::AtomicRc<Plugin>> = OnceLock::new();

fn task_api(guard: &circ::Guard) -> LocalApiHandle<'_, dyn TaskSystemApi> {
    PLUGIN
        .get()
        .unwrap()
        .load(std::sync::atomic::Ordering::Relaxed, &guard)
        .as_ref()
        .unwrap()
        .task_api
        .get(&guard)
        .unwrap()
}

struct Plugin {
    task_api: ApiHandle<dyn TaskSystemApi>,
}

unsafe impl circ::RcObject for Plugin {
    fn pop_edges(&mut self, out: &mut Vec<circ::Rc<Self>>) {
        // no edges
    }
}

#[no_mangle]
pub fn test_dynamic_tls() {
    shared::TEST_VAR.with(|x| {
        let mut guard = x.lock().unwrap();
        println!("plugin: {}", *guard);
        *guard = 43;
        println!("plugin: {}", *guard);
    });
}

#[no_mangle]
pub fn test_typeid(ids: (TypeId, TypeId, TypeId)) {
    println!("--StringAlias--");
    println!(
        "plugin's std::String         [{:?}]",
        TypeId::of::<String>()
    );
    println!("main's   std::String         [{:?}]", ids.0);
    println!(
        "plugin's shared::StringAlias0[{:?}]",
        TypeId::of::<shared::StringAlias0>()
    );
    println!("main's   shared::StringAlias0[{:?}]", ids.1);
    println!(
        "plugin's shared::StringAlias1[{:?}]",
        TypeId::of::<shared::StringAlias1>()
    );
    println!("main's   shared::StringAlias1[{:?}]", ids.2);
    println!("--StringAlias--");

    println!("--ModulePath--");
    println!("{}", std::module_path!());
}

#[no_mangle]
pub fn load_plugin(context: &Context, api_registry_api: ApiHandle<dyn ApiRegistryApi>) {
    unsafe {
        context.initialize();
    }
    let guard = circ::cs();
    let api_guard = api_registry_api.get(&guard).unwrap();

    let task_system_api = api_guard.local_find::<dyn TaskSystemApi>(None);

    // let apis: ApiHandle<Box<dyn Api>> = task_system.into();

    // let task_guard = apis.get().unwrap();
    // println!("{:p}", task_guard.deref());

    PLUGIN.get_or_init(|| {
        circ::AtomicRc::new(Plugin {
            task_api: task_system_api,
        })
    });
}

#[no_mangle]
pub fn unload_plugin() {
    // clear plugin data
    // PLUGIN.get().unwrap().store::<Arc<_>>(None);
}

#[async_ffi(?Send)]
#[instrument]
#[no_mangle]
async fn plugin_task(s: String) -> String {
    let guard = circ::cs();
    let task_system_api = task_api(&guard);
    // get static value
    // let x = api::TEST_INT.get().unwrap();
    // println!("plugin_task: {:?}", x);
    // print thread name

    println!("{:p}", task_system_api.deref());
    println!(
        "(StdId)plugin_thread(plugin task): {:?}",
        std::thread::current().id()
    );
    println!(
        "(SysId)plugin_thread(plugin task1): {:?}",
        SysThreadId::current()
    );

    // let task_system_api: &dyn TaskSystemApi = task_guard.as_ref().downcast_ref().unwrap();

    let x = async {
        println!(
            "(StdId)plugin_thread(detached): {:?}",
            std::thread::current().id()
        );
        println!(
            "(SysId)plugin_thread(detached): {:?}",
            SysThreadId::current()
        );
        println!(
            "(StdName)plugin_thread(detached): {:?}",
            std::thread::current().name()
        );
        println!(
            "(RuntimeName)plugin_thread(detached): {:?}",
            Runtime::name()
        );
        let file = bubble_tasks::fs::File::open("Cargo.toml").await.unwrap();
        let buf = Vec::with_capacity(1024);
        let (read, buffer) = file.read_to_end_at(buf, 0).await.unwrap();
        assert_eq!(read, buffer.len());
        let buffer = String::from_utf8(buffer).unwrap();
        println!("{buffer}");
        let instant = Instant::now();
        bubble_tasks::runtime::time::sleep(Duration::from_secs(1)).await;
        println!("plugin_thread(loop time): {:?}", instant.elapsed());
        ()
    }
    .into_local_ffi();

    let x = task_system_api.spawn_detached(x);

    let task_system_global_api: ApiHandle<_> = (&task_system_api).into();
    let x = task_system_api.spawn(
        async move {
            let mut handles = FuturesUnordered::new();
            let global_api = task_system_global_api;
            let guard = circ::cs();
            let task_system_api = global_api.get(&guard).unwrap();
            for i in 0..2 {
                handles.push(
                    task_system_api
                        .dispatch(
                            None,
                            Box::new(move || {
                                async move {
                                    // info!("plugin_thread(loop{})", i);
                                    // let filename = format!("test0.txt");
                                    // let file = bubble_tasks::fs::File::open(filename).await.unwrap();
                                    // let (read, buffer) = file
                                    //     .read_to_end_at(Vec::with_capacity(1024), 0)
                                    //     .await
                                    //     .unwrap();
                                    // assert_eq!(read, buffer.len());
                                    // print thread id
                                    println!(
                                        "(StdId)plugin_thread(loop{}): {:?}",
                                        i,
                                        std::thread::current().id()
                                    );
                                    bubble_tasks::runtime::time::sleep(Duration::from_secs(2))
                                        .await;
                                    println!(
                                        "(SysId)plugin_thread(loop{}): {:?}",
                                        i,
                                        SysThreadId::current()
                                    );
                                    println!(
                                        "(StdName)plugin_thread(loop{}): {:?}",
                                        i,
                                        std::thread::current().name()
                                    );
                                    println!(
                                        "(RuntimeName)plugin_thread(loop{}): {:?}",
                                        i,
                                        Runtime::name()
                                    );
                                }
                                .into_local_ffi()
                            }),
                        )
                        .unwrap(),
                );
            }
            let instant = Instant::now();
            while handles.next().await.is_some() {}
            println!("plugin_thread(loop time): {:?}", instant.elapsed());
            "sss".to_string();
        }
        .into_local_ffi(),
    );
    // drop(task_system_global_api);
    let y = task_system_api
        .dispatch_blocking(
            None,
            Box::new(|| {
                println!("before dispatch blocking sleep");
                std::thread::sleep(Duration::from_secs(10));
                println!("after dispatch blocking sleep");
                1;
            }),
        )
        .unwrap();
    println!("{:?}", futures_util::join!(x, y));

    "xxx".to_string()

    // let file = File::open(s).await.unwrap();
    // let (read, buffer) = file.read_to_end_at(Vec::with_capacity(1024), 0).await.unwrap();
    // assert_eq!(read, buffer.len());
    // let buffer = String::from_utf8(buffer).unwrap();
    // buffer
}

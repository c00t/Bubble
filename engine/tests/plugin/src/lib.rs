#![feature(async_closure)]

use std::{
    any::TypeId,
    ops::Deref,
    sync::OnceLock,
    time::{Duration, Instant},
};

use async_ffi::{async_ffi, FutureExt};
use bubble_core::{
    api::prelude::*,
    os::{thread::dyntls::Context, SysThreadId},
    sync::{Arc, AtomicArc, RefCount},
    tracing::{self, info, instrument, warn},
};
use bubble_tasks::{
    io::{AsyncReadAt, AsyncReadAtExt},
    runtime::Runtime,
};
use futures_util::{stream::FuturesUnordered, StreamExt};
use shared::TaskSystemApi;

// a singleton plugin
static PLUGIN: OnceLock<AtomicArc<Plugin>> = OnceLock::new();

struct Plugin {
    task_api: ApiHandle<dyn TaskSystemApi>,
}

#[no_mangle]
pub unsafe extern "C" fn test_dynamic_tls() {
    shared::TEST_VAR.with(|x| {
        let mut guard = x.lock().unwrap();
        println!("plugin: {}", *guard);
        *guard = 43;
        println!("plugin: {}", *guard);
    });
}

#[no_mangle]
pub unsafe extern "C" fn test_typeid(ids: (TypeId, TypeId, TypeId)) {
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
pub unsafe extern "C" fn load_plugin(
    context: &Context,
    api_registry_api: ApiHandle<dyn ApiRegistryApi>,
) {
    context.initialize();
    let api_guard = api_registry_api.get().unwrap();

    let task_system_api = api_guard.local_find::<dyn TaskSystemApi>();

    // let apis: ApiHandle<Box<dyn Api>> = task_system.into();

    // let task_guard = apis.get().unwrap();
    // println!("{:p}", task_guard.deref());

    println!(
        "task_system_api.stront_count({})(at load_plugin)",
        task_system_api.strong_count()
    );

    PLUGIN.get_or_init(|| {
        AtomicArc::new(Plugin {
            task_api: task_system_api,
        })
    });
}

#[no_mangle]
pub unsafe extern "C" fn unload_plugin() {
    // clear plugin data
    PLUGIN.get().unwrap().store::<Arc<_>>(None);
}

#[async_ffi(?Send)]
#[instrument]
#[no_mangle]
async fn plugin_task(s: String) -> String {
    let plugin = PLUGIN.get().unwrap().load().unwrap();
    let task_system_api = plugin.task_api.get().unwrap();
    // it's 1 or 2 depends on the order of `set` or `get` of api_registry_api
    println!(
        "task_guard.strong_count()(at plugin_task begin), should be 1 or 2: {}",
        task_system_api.strong_count()
    );
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
    println!(
        "task_system_api_arc.strong_count()(before dispatch): {}",
        task_system_global_api.strong_count()
    );
    let x = task_system_api.spawn(
        async move {
            let mut handles = FuturesUnordered::new();
            let global_api = task_system_global_api;
            println!(
                "global_api.strong_count()(at dispatch begin): {}",
                global_api.strong_count()
            );
            let task_system_api = global_api.get().unwrap();
            for i in 0..2 {
                handles.push(
                    task_system_api
                        .dispatch(
                            None,
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
                                bubble_tasks::runtime::time::sleep(Duration::from_secs(2)).await;
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
                            .into_ffi(),
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

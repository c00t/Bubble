#![feature(async_closure)]

use std::{
    any::TypeId,
    ops::Deref,
    sync::OnceLock,
    time::{Duration, Instant},
};

use async_ffi::{async_ffi, FutureExt};
use bubble_core::{
    api::{
        api_registry_api::{ApiHandle, ApiRegistry, ApiRegistryApi, ApiRegistryRef},
        Api,
    },
    os::{thread::Context, SysThreadId},
    sync::{Arc, RefCount},
};
use bubble_tasks::{
    io::{AsyncReadAt, AsyncReadAtExt},
    runtime::Runtime,
};
use futures_util::{stream::FuturesUnordered, StreamExt};
use shared::{task_system_api, TaskSystem, TaskSystemApi};

use bubble_core::api::{
    TraitcastTarget, TraitcastableAny, TraitcastableAnyInfra, TraitcastableAnyInfraExt,
    TraitcastableTo,
};

// a singleton plugin
static PLUGIN: OnceLock<Plugin> = OnceLock::new();

struct Plugin {
    task_api: ApiHandle<Box<dyn TraitcastableAny>>,
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
pub unsafe extern "C" fn load_plugin(context: &Context, api_registry: &Box<dyn ApiRegistryApi>) {
    context.initialize();

    let task_system = api_registry.get(task_system_api::NAME, task_system_api::VERSION);

    // let apis: ApiHandle<Box<dyn Api>> = task_system.into();

    // let task_guard = apis.get().unwrap();
    // println!("{:p}", task_guard.deref());

    PLUGIN.get_or_init(|| Plugin {
        task_api: task_system,
    });
}

#[async_ffi(?Send)]
#[no_mangle]
async fn plugin_task(s: String) -> String {
    let plugin = PLUGIN.get().unwrap();
    let task_guard = plugin.task_api.get().unwrap();
    println!(
        "task_guard.strong_count()(at plugin_task begin): {}",
        task_guard.strong_count()
    );
    // get static value
    // let x = api::TEST_INT.get().unwrap();
    // println!("plugin_task: {:?}", x);
    // print thread name

    println!("{:p}", task_guard.deref());
    println!(
        "(StdId)plugin_thread(plugin task): {:?}",
        std::thread::current().id()
    );
    println!(
        "(SysId)plugin_thread(plugin task1): {:?}",
        SysThreadId::current()
    );

    println!(
        "any id:{:?}",
        TraitcastableAny::type_id(task_guard.as_ref())
    );
    let task_system_api: &dyn TaskSystemApi = task_guard.as_ref().downcast_ref().unwrap();

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

    let x = task_system_api.spawn(x).detach();

    let task_system_api_arc: Arc<_> = (&task_guard).into();
    println!(
        "task_system_api_arc.strong_count()(before dispatch): {}",
        task_system_api_arc.strong_count()
    );
    let x = task_system_api.dispatch(
        async move {
            // let file = bubble_tasks::fs::File::open("Cargo.toml").await.unwrap();
            // let (read, buffer) = file
            //     .read_to_end_at(Vec::with_capacity(1024), 0)
            //     .await
            //     .unwrap();
            // assert_eq!(read, buffer.len());
            // let buffer = String::from_utf8(buffer).unwrap();
            let mut handles = FuturesUnordered::new();
            let task_system_api: &dyn TaskSystemApi =
                task_system_api_arc.as_ref().downcast_ref().unwrap();
            println!(
                "task_system_api_arc.strong_count()(in dispatch future): {}",
                task_system_api_arc.strong_count()
            );
            for _ in 0..16 {
                handles.push(
                    task_system_api.dispatch(
                        async move {
                            // let filename = format!("test0.txt");
                            // let file = bubble_tasks::fs::File::open(filename).await.unwrap();
                            // let (read, buffer) = file
                            //     .read_to_end_at(Vec::with_capacity(1024), 0)
                            //     .await
                            //     .unwrap();
                            // assert_eq!(read, buffer.len());
                            // print thread id
                            println!(
                                "(StdId)plugin_thread(loop): {:?}",
                                std::thread::current().id()
                            );
                            println!("(SysId)plugin_thread(loop): {:?}", SysThreadId::current());
                            println!(
                                "(StdName)plugin_thread(loop): {:?}",
                                std::thread::current().name()
                            );
                            println!("(RuntimeName)plugin_thread(loop): {:?}", Runtime::name());
                            bubble_tasks::runtime::time::sleep(Duration::from_secs(1)).await;
                        }
                        .into_ffi(),
                    ),
                );
            }
            let instant = Instant::now();
            while handles.next().await.is_some() {}
            println!("plugin_thread(loop time): {:?}", instant.elapsed());
            "sss".to_string();
        }
        .into_ffi(),
    );
    x.await;

    let y = task_system_api.dispatch_blocking(Box::new(|| {
        std::thread::sleep(Duration::from_secs(3));
        1
    }));
    println!("{}", y.await);

    "xxx".to_string()

    // let file = File::open(s).await.unwrap();
    // let (read, buffer) = file.read_to_end_at(Vec::with_capacity(1024), 0).await.unwrap();
    // assert_eq!(read, buffer.len());
    // let buffer = String::from_utf8(buffer).unwrap();
    // buffer
}

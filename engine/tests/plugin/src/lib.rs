use std::{
    any::TypeId,
    ops::Deref,
    sync::OnceLock,
    time::{Duration, Instant},
};

use async_ffi::{async_ffi, FutureExt};
use bubble_core::{
    api::{ApiHandle, ApiRegistry, ApiRegistryApi},
    os::{thread::Context, SysThreadId},
    sync::Arc,
};
use bubble_tasks::runtime::Runtime;
use futures_util::{stream::FuturesUnordered, StreamExt};
use shared::{TaskSystem, TaskSystemApi};

// a singleton plugin
static PLUGIN: OnceLock<Plugin> = OnceLock::new();

struct Plugin {
    task_api: ApiHandle<TaskSystem>,
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
pub unsafe extern "C" fn load_plugin(context: &Context, api_registry: &'static ApiRegistry) {
    context.initialize();

    let task_system_api = api_registry.get::<TaskSystem>();

    let task_guard = task_system_api.get().unwrap();
    // println!("{:p}", task_guard.deref());

    PLUGIN.get_or_init(|| Plugin {
        task_api: task_system_api,
    });
}

#[async_ffi(?Send)]
#[no_mangle]
async fn plugin_task(s: String) -> String {
    let plugin = PLUGIN.get().unwrap();
    let task_guard = plugin.task_api.get().unwrap();
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
        "(SysId)plugin_thread(plugin task): {:?}",
        SysThreadId::current()
    );

    task_guard
        .spawn(
            async {
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
            }
            .into_ffi(),
        )
        .detach();

    let x = task_guard
        .dispatcher
        .dispatch(|| {
            async move {
                // let file = File::open("Cargo.toml").await.unwrap();
                // let (read, buffer) = file
                //     .read_to_end_at(Vec::with_capacity(1024), 0)
                //     .await
                //     .unwrap();
                // assert_eq!(read, buffer.len());
                // let buffer = String::from_utf8(buffer).unwrap();
                let mut handles = FuturesUnordered::new();
                for _ in 0..16 {
                    handles.push(async move {
                        // let filename = format!("test0.txt");
                        // let file = File::open(filename).await.unwrap();
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
                    });
                }
                let instant = Instant::now();
                while handles.next().await.is_some() {}
                println!("plugin_thread(loop time): {:?}", instant.elapsed());
                "sss".to_string()
            }
            .into_local_ffi()
        })
        .unwrap();
    x.await.unwrap()

    // let file = File::open(s).await.unwrap();
    // let (read, buffer) = file.read_to_end_at(Vec::with_capacity(1024), 0).await.unwrap();
    // assert_eq!(read, buffer.len());
    // let buffer = String::from_utf8(buffer).unwrap();
    // buffer
}

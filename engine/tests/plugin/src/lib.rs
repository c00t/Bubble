use std::{
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

use async_ffi::{async_ffi, FutureExt};
use bubble_core::os::{thread::Context, SysThreadId};
use futures_util::{stream::FuturesUnordered, StreamExt};

// a singleton plugin
static PLUGIN: OnceLock<Plugin> = OnceLock::new();

struct Plugin {
    task_api: shared::TaskSystemApi,
}

#[no_mangle]
pub unsafe extern "C" fn test_func() {
    shared::TEST_VAR.with(|x| {
        let mut guard = x.lock().unwrap();
        println!("plugin: {}", *guard);
        *guard = 43;
        println!("plugin: {}", *guard);
    });
}

#[no_mangle]
pub unsafe extern "C" fn load_plugin(context: &Context, task_system_api: &shared::TaskSystemApi) {
    context.initialize();

    let task_api = task_system_api.clone();
    PLUGIN.get_or_init(|| Plugin { task_api });
}

#[async_ffi(?Send)]
#[no_mangle]
async fn plugin_task(s: String) -> String {
    // get static value
    // let x = api::TEST_INT.get().unwrap();
    // println!("plugin_task: {:?}", x);
    // print thread name

    println!(
        "plugin_thread(plugin task): {:?}",
        std::thread::current().id()
    );
    println!("plugin_thread(plugin task): {:?}", SysThreadId::current());
    let plugin = PLUGIN.get().unwrap();
    (plugin.task_api.spawn)(
        async {
            println!("plugin_thread(detached): {:?}", std::thread::current().id());
            println!("plugin_thread(detached): {:?}", SysThreadId::current());
            println!(
                "plugin_thread(detached): {:?}",
                std::thread::current().name()
            );
        }
        .into_ffi(),
    )
    .detach();
    let x = (plugin.task_api.dispatch)(|| {
        async move {
            // let file = File::open("Cargo.toml").await.unwrap();
            // let (read, buffer) = file
            //     .read_to_end_at(Vec::with_capacity(1024), 0)
            //     .await
            //     .unwrap();
            // assert_eq!(read, buffer.len());
            // let buffer = String::from_utf8(buffer).unwrap();
            let mut handles = FuturesUnordered::new();
            for i in 0..16 {
                handles.push(async move {
                    // let filename = format!("test0.txt");
                    // let file = File::open(filename).await.unwrap();
                    // let (read, buffer) = file
                    //     .read_to_end_at(Vec::with_capacity(1024), 0)
                    //     .await
                    //     .unwrap();
                    // assert_eq!(read, buffer.len());
                    // print thread id
                    println!("plugin_thread(loop): {:?}", std::thread::current().id());
                    println!("plugin_thread(loop): {:?}", SysThreadId::current());
                    bubble_tasks::runtime::time::sleep(Duration::from_secs(1)).await;
                });
            }
            let instant = Instant::now();
            while handles.next().await.is_some() {}
            println!("plugin_thread(loop time): {:?}", instant.elapsed());
            "sss".to_string()
        }
        .into_local_ffi()
    });
    x.await

    // let file = File::open(s).await.unwrap();
    // let (read, buffer) = file.read_to_end_at(Vec::with_capacity(1024), 0).await.unwrap();
    // assert_eq!(read, buffer.len());
    // let buffer = String::from_utf8(buffer).unwrap();
    // buffer
}

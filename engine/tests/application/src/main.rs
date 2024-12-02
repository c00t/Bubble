use std::cell::OnceCell;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;
use std::{any::TypeId, sync::OnceLock, thread};

use bubble_core::api::prelude::*;
use bubble_core::sync::circ;
use bubble_core::tracing;
use bubble_core::{
    api::api_registry_api::get_api_registry_api,
    os::{thread::dyntls::Context, SysThreadId},
    tracing::info,
};
use bubble_tasks::runtime::Runtime;
use bubble_tasks::{
    async_ffi,
    async_ffi::{async_ffi, FutureExt},
    futures_channel, futures_util,
    types::TaskSystemApi,
};
use dlopen2::wrapper::{Container, WrapperApi};
use rand::rngs::ThreadRng;
use rand::Rng;
use rand_distr::Distribution;
use shared::{self as sd, TraitCastableDropSub, TraitCastableDropSuper};

#[derive(WrapperApi)]
pub struct PluginApi {
    load_plugin: fn(context: &Context, api_registry: ApiHandle<dyn ApiRegistryApi>),
    test_dynamic_tls: fn(),
    plugin_task: fn(s: String) -> async_ffi::FfiFuture<String>,
    test_typeid: fn(ids: (TypeId, TypeId, TypeId)),
    unload_plugin: fn(),
}

pub fn test_dynamic_tls_std_thread(container: &'static Container<PluginApi>) {
    let handle = thread::spawn(|| {
        let _x = {
            shared::TEST_VAR.with(|x| {
                let mut guard = x.lock().unwrap();
                println!("new thread host:{}", *guard);
                assert_eq!(*guard, 0);
                *guard = 42;
                println!("new thread host:{}", *guard);
                assert_eq!(*guard, 42);
                drop(guard);
            });
            0
        };
        // test_func
        (container.test_dynamic_tls)();
        let _x = {
            shared::TEST_VAR.with(|x| {
                let mut guard = x.lock().unwrap();
                println!("new thread host:{}", *guard);
                assert_eq!(*guard, 43);
                *guard = 42;
                println!("new thread host:{}", *guard);
                assert_eq!(*guard, 42);
                drop(guard);
            });
            0
        };
    });
    // test in current thread
    let _x = {
        shared::TEST_VAR.with(|x| {
            let mut guard = x.lock().unwrap();
            println!("current thread host:{}", *guard);
            assert_eq!(*guard, 0);
            *guard = 41;
            println!("current thread host:{}", *guard);
            assert_eq!(*guard, 41);
            drop(guard);
        });
        0
    };
    // test_func
    (container.test_dynamic_tls)();
    let _x = {
        shared::TEST_VAR.with(|x| {
            let mut guard = x.lock().unwrap();
            println!("current thread host:{}", *guard);
            assert_eq!(*guard, 43);
            *guard = 41;
            println!("current thread host:{}", *guard);
            assert_eq!(*guard, 41);
            drop(guard);
        });
        0
    };

    handle.join().unwrap();
}

pub fn test_bubble_tasks(
    container: &std::sync::Arc<Container<PluginApi>>,
    api_registry_api: ApiHandle<dyn ApiRegistryApi>,
) {
    println!("(StdId)main_thread: {:?}", std::thread::current().id());
    println!("(SysId)main_thread: {:?}", SysThreadId::current());
    println!("(StdName)main_thread: {:?}", std::thread::current().name());
    let container_clone = container.clone();
    let guard = circ::cs();
    let task_system_api = api_registry_api
        .get(&guard)
        .unwrap()
        .local_find::<dyn TaskSystemApi>(None);
    // meature time
    let start = std::time::Instant::now();
    let r = task_system_api.get(&guard).unwrap().dispatch(
        None,
        async move {
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

            let x =
                bubble_tasks::runtime::spawn(container_clone.plugin_task("Cargo.toml".to_string()));
            let (x, _) = futures_util::join!(
                x,
                bubble_tasks::runtime::time::sleep(std::time::Duration::from_secs(10))
            );
            println!("{:?}", x);
        }
        .into_ffi(),
    );

    let runtime = Runtime::new().unwrap();
    let _ = runtime.block_on(r.unwrap());
    println!(
        "test_bubble_tasks time: {:?}, it's should be 10s",
        start.elapsed()
    );
}

pub fn test_typeid(container: &Container<PluginApi>) {
    let ids = (
        TypeId::of::<String>(),
        TypeId::of::<shared::StringAlias0>(),
        TypeId::of::<sd::StringAlias1>(),
    );
    (container.test_typeid)(ids);
}

pub fn test_trait_castable_any(d: Box<dyn TraitcastableAny>) {
    println!("any");
    drop(d);
}

pub fn test_trait_castable_any_2(d: Box<dyn shared::TraitCastableDropSub>) {
    println!("sub");
    drop(d);
}

pub fn test_trait_castable_any_3(d: Box<dyn shared::TraitCastableDropSuper>) {
    println!("super");
    drop(d);
}

static CONTAINER1: OnceLock<std::sync::Arc<Container<PluginApi>>> = OnceLock::new();
static CONTAINER2: OnceLock<std::sync::Arc<Container<PluginApi>>> = OnceLock::new();

static COUNTER: AtomicI32 = AtomicI32::new(0);

async fn long_running_async_task(idx: i32) -> String {
    let mut rng = rand::thread_rng();
    let normal = rand_distr::Normal::new(16.0, 10.0).unwrap();
    let x = normal.sample(&mut rng);
    let x = (x as u64).clamp(8, 100);
    println!("in tick {idx}, wait {x} ms");
    bubble_tasks::runtime::time::sleep(Duration::from_millis(x)).await;
    "Long Running Async Task".to_string()
}

pub async fn tick() -> bool {
    // let task_system_api = task_system.get().unwrap();
    // let counter = COUNTER.load(Ordering::SeqCst);
    // random choose tasks
    // /// A long running background task which should be run in the background.
    // fn long_running_blocking_task(index: i32, count: i32) -> String {
    //     // suppose that we're reading a large shader file with index.
    //     std::thread::sleep(Duration::from_secs(10));
    //     format!("Long Running Blocking Task {}:{}", index, count).to_string()
    // }

    // // create a channel to send the result of the task
    // let (tx, mut rx) = futures_channel::oneshot::channel();
    // let _ = task_system_api.dispatch_blocking(Box::new(move || {
    //     let q = long_running_blocking_task(counter,0);
    //     tx.send(q).unwrap();
    // }));
    // while let Ok(Some(shader_module)) = rx.try_recv() {
    //     println!("store a shader module to {}", shader_module);
    // }
    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
    long_running_async_task(counter).await;

    if counter == 2000 {
        println!("in tick: {}, return false", counter);
        false
    } else {
        println!("in tick: {}, return true", counter);
        true
    }
}

fn main() {
    // get current working directory
    let cwd = std::env::current_dir().unwrap();
    println!("Current working directory: {:?}", cwd);
    let dll_name = [cwd.join("./target/debug/plugin.dll")];
    let dll_name2 = [cwd.join("./target/debug/plugin2.dll")];

    let container = CONTAINER1
        .get_or_init(|| std::sync::Arc::new(unsafe { Container::load(&dll_name[0]) }.unwrap()));
    let container2 = CONTAINER2
        .get_or_init(|| std::sync::Arc::new(unsafe { Container::load(&dll_name2[0]) }.unwrap()));

    // get context, and prepare apis
    let context = bubble_core::os::thread::dyntls_context::get();
    unsafe {
        context.initialize();
    }

    bubble_log::LogSubscriberBuilder::default().set_global();

    let api_registry_api = get_api_registry_api(); // api: strong count 1,2
    let guard = circ::cs();
    // TEST0 set after load
    println!("--TEST0-->");
    // for each plugin, run load_plugin, pass clone() of ApiHandle<dyn ApiRegistryApi> to it.
    // 1. load plugin
    (container.load_plugin)(&context, api_registry_api.clone()); // task: strong count 1,2, api: clone then dropped
    (container2.load_plugin)(&context, api_registry_api.clone()); // task: strong count 3, api: clone then dropped
    let x = api_registry_api.get(&guard).unwrap();
    // 2. set api
    println!("...");
    let task_system_api = {
        let guard = circ::cs();
        let api = bubble_tasks::task_system_api::get_task_system_api_default();
        api_registry_api.get(&guard).expect("Failed to get API registry api").local_set(api, None)
    };
    // let task_system_api =
    //     bubble_tasks::task_system_api::register_task_system_api(&api_registry_api, None); // task: strong count 4
    // let plugin_api = bubble_core::api::plugin_api::register_plugin_api(&api_registry_api, None);
    let plugin_api = {
        let guard = circ::cs();
        let api = bubble_core::api::plugin_api::get_default_plugin_api();
        api_registry_api.get(&guard).expect("Failed to get API registry api").local_set(api, None)
    };
    let hot_reload_test_api = api_registry_api
        .get(&guard)
        .unwrap()
        .local_find::<hot_reload_plugin_types::DynHotReloadTestApi>(None);
    println!("{:?}", task_system_api);
    println!(
        "num_threads: {}",
        task_system_api.get(&guard).unwrap().num_threads()
    );

    // info!(">>>>info log");
    let q = plugin_api
        .get(&guard)
        .unwrap()
        .load("./target/debug/hot_reload_plugin.dll", true);
    println!("hot reload plugin is none? {:?}", q.is_none());
    // plugin loaded, so it's safe to call get_test_string
    info!(test_string = hot_reload_test_api.get(&guard).unwrap().get_test_string());
    drop(guard);

    // TEST1
    println!("--TEST1->");
    test_dynamic_tls_std_thread(&container);
    // test_dynamic_tls_tasks_thread(&container2);
    println!("<-TEST1--");

    // TEST2
    println!("--TEST2->");
    test_bubble_tasks(&container, api_registry_api.clone()); // task: strong count 6
    test_bubble_tasks(&container2, api_registry_api.clone()); // task: strong count 7
    println!("<-TEST2--");

    // TEST3
    println!("--TEST3->");
    test_typeid(&container);
    test_typeid(&container2);
    println!("<-TEST3--");

    // thread::sleep(std::time::Duration::from_secs(10));

    // remove task system api
    // let q = api_registry_api
    //     .get()
    //     .unwrap()
    //     .local_remove::<dyn TaskSystemApi>();
    // // .remove(shared::task_system_api_constants::NAME, shared::task_system_api_constants::VERSION);
    // drop(q); // task: strong count 6
    container.unload_plugin(); // task: strong count 5
    container2.unload_plugin(); // task: strong count 4

    // Unload dll library
    // TODO: Arc? it will unload when it drop, but if you're using Arc in static to ref it

    println!("--TEST5->");
    let b = Box::new(shared::TraitCastableDrop { value: 1 });
    let b_any: Box<dyn TraitcastableAny> = b;
    test_trait_castable_any(b_any);
    let b = Box::new(shared::TraitCastableDrop { value: 1 });
    let b_any: Box<dyn TraitcastableAny> = b;
    let b_sub: Box<dyn TraitCastableDropSub> = b_any.downcast().unwrap();
    test_trait_castable_any_2(b_sub);
    let b = Box::new(shared::TraitCastableDrop { value: 1 });
    let b_any: Box<dyn TraitcastableAny> = b;
    let b_super: Box<dyn TraitCastableDropSuper> = b_any.downcast().unwrap();
    test_trait_castable_any_3(b_super);
    println!("<-TEST5--");

    use winit::event_loop::EventLoop;
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::default();
    let task_system_api_clone = task_system_api.clone();
    let tick_thread = thread::Builder::new()
        .name("Tick Thread".to_string())
        .spawn(move || {
            let task_system = task_system_api_clone;
            let guard = circ::cs();
            let task_api = task_system.get(&guard).unwrap();
            while unsafe {
                let plugin_api = plugin_api.clone();
                let hot_reload_test_api = hot_reload_test_api.clone();
                task_api.tick(
                    async move {
                        let guard = circ::cs();
                        let plugin_api = plugin_api.get(&guard).unwrap();
                        let reload = plugin_api.check_hot_reload_tick();
                        if reload {
                            let guard = circ::cs();
                            info!(
                                test_string =
                                    hot_reload_test_api.get(&guard).unwrap().get_test_string()
                            );
                        }
                        let b = tick().await;
                        tracing::event!(
                            tracing::Level::INFO,
                            message = "end tick",
                            tracy.frame_mark = true
                        );
                        b
                    }
                    .into_local_ffi(),
                )
            } {
                println!("...");
            }
        })
        .unwrap();
    let _ = event_loop.run_app(&mut app);

    let _ = tick_thread.join();
    let guard = circ::cs();
    task_system_api.get(&guard).map(|local| {
        local.shutdown();
    });
    api_registry_api.get(&guard).map(|local| {
        local.local_shutdown();
    });
}

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Default)]
struct App {
    window: Option<Window>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.window = Some(
            event_loop
                .create_window(Window::default_attributes())
                .unwrap(),
        );
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                println!("The close button was pressed; stopping");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // Redraw the application.
                //
                // It's preferable for applications that do not render continuously to render in
                // this event rather than in AboutToWait, since rendering in here allows
                // the program to gracefully handle redraws requested by the OS.

                // Draw.

                // Queue a RedrawRequested event.
                //
                // You only need to call this if you've determined that you need to redraw in
                // applications which do not always need to. Applications that redraw continuously
                // can render here instead.
                self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }
}

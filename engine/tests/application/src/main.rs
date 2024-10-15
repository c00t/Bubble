use std::{any::TypeId, num::NonZeroUsize, sync::OnceLock, thread};

use async_ffi::FutureExt;
use bubble_core::{
    api::{ApiRegistry, ApiRegistryApi},
    os::{thread::Context, SysThreadId},
    sync::AtomicArc,
};
use bubble_tasks::{dispatcher::Dispatcher, runtime::Runtime};
use dlopen2::wrapper::{Container, WrapperApi};
use shared as sd;
use shared::TaskSystem;

#[derive(WrapperApi)]
pub struct PluginApi {
    load_plugin: fn(context: &Context, api_registry: &'static ApiRegistry),
    test_dynamic_tls: fn(),
    plugin_task: fn(s: String) -> async_ffi::FfiFuture<String>,
    test_typeid: fn(ids: (TypeId, TypeId, TypeId)),
}

pub fn test_dynamic_tls(container: &'static Container<PluginApi>) {
    let handle = thread::spawn(|| {
        // use bubble_core TEST_VAR as a example
        let _x = {
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
        let _x = {
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

pub fn test_bubble_tasks(container: &std::sync::Arc<Container<PluginApi>>) {
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

/// Demonstrates various approaches to handle API reloading in a plugin system,
/// using different combinations of Arc and AtomicArc. It illustrates the memory
/// layout and potential issues with each approach, ultimately suggesting a
/// solution using Arc<AtomicArc<T>> for better control over API lifecycle.
pub fn doc_reload_plugin_ptr() {
    // 1. Create a simple api strct
    #[derive(Clone, Copy)]
    struct ApiData {
        data: i32,
    }

    let api_data = ApiData { data: 42 };

    // 2. When we set an API, we store AtomicArc inside ApiRegistry's internal hashmap
    //
    // Memory view:
    // API registry map:
    // ┌─────────────┐ ┌─────────────┐
    // │ AtomicArc<T>│ │AtomicArc<T> │
    // └─────┬───────┘ └─────────────┘
    //       │
    //       ▼
    // ┌─────────────────────────────┐
    // │ strong_count AtomicUsize (1)│
    // │ weak_count   AtomicUsize (1)│
    // │ birth_era    u64          - │
    // │ data         size::of<T>  * │
    // └─────────────────────────────┘
    let atomic_ptr_in_registry = AtomicArc::new(api_data);

    // 3.1 When a plugin gets an API, if we just clone the AtomicArc
    //
    // Memory view:
    // API registry map:
    // ┌─────────────┐ ┌─────────────┐
    // │ AtomicArc<T>│ │AtomicArc<T> │
    // └─────┬───────┘ └─────────────┘
    //       │
    //       ▼
    // ┌───────────────────────────────────┐
    // │ strong_count AtomicUsize      (4) │ ◄── plugin1_api_handle
    // │ weak_count   AtomicUsize      (1) │ ◄── plugin2_api_handle
    // │ birth_era    u64               -  │ ◄── plugin3_api_handle
    // │ data         size::of<ApiData> *  │ ◄── ...
    // └───────────────────────────────────┘
    let plugin1_api_handle = atomic_ptr_in_registry.clone();
    let plugin2_api_handle = atomic_ptr_in_registry.clone();
    let plugin3_api_handle = atomic_ptr_in_registry.clone();
    // using above method, if we want to reload the api, we only use the ApiRegistry methods.
    // so we atomic changed the ptr of AtomicArc in registry map, like below:
    //
    // Memory View
    //
    // API Registry Map:
    // ┌─────────────────┐ ┌─────────────────┐
    // │  AtomicArc<T>   │ │  AtomicArc<T>   │
    // └────────┬────────┘ └─────────────────┘
    //          │
    //          ▼
    // ┌─────────────────────────────────────┐ ┌─────────────────────────────────────┐
    // │       New Plugin Arc                │ │       Old Plugin Arc                │
    // ├─────────────────────────────────────┤ ├─────────────────────────────────────┤
    // │ strong_count: AtomicUsize        (1)│ │ strong_count: AtomicUsize        (3)│ ◄── plugin1_api_handle
    // │ weak_count:   AtomicUsize        (1)│ │ weak_count:   AtomicUsize        (1)│ ◄── plugin2_api_handle
    // │ birth_era:    u64                 - │ │ birth_era:    u64                 - │ ◄── plugin3_api_handle
    // │ data:         size::of::<ApiData> * │ │ data:         size::of::<ApiData> * │ ◄── ...
    // └─────────────────────────────────────┘ └─────────────────────────────────────┘
    //
    // other plugins still using the old api, we can't unload dll
    // nooooooooo!

    // 3.2 when a plugin get a api, if we create a new Arc from the AtomicArc.clone(),
    //
    // Memory View:
    //
    // API Registry Map:
    // ┌───────────────────────┐ ┌───────────────────────┐
    // │   AtomicArc<T> <m1>   │ │   AtomicArc<T> <m2>   │
    // │ ┌───────────────────┐ │ │                       │
    // │ │    atomic_ptr     │ │ │                       │
    // │ └─────────┬─────────┘ │ │                       │
    // └───────────┼───────────┘ └───────────────────────┘
    //             │
    //             ▼
    // ┌─────────────────────────────────────────────────┐
    // │ strong_count: AtomicUsize                    (1)│
    // │ weak_count:   AtomicUsize                    (1)│
    // │ birth_era:    u64                             - │
    // │ data:         size::of::<ApiData>             * │
    // └─────────────────────────────────────────────────┘
    let plugin1_api_handle = bubble_core::sync::Arc::new(atomic_ptr_in_registry.clone());
    let plugin2_api_handle = bubble_core::sync::Arc::new(atomic_ptr_in_registry.clone());
    let plugin3_api_handle = bubble_core::sync::Arc::new(atomic_ptr_in_registry.clone());
    // ┌──────────────────────────────┐
    // │   plugin1 Arc<AtomicArc<T>>  │
    // │ ┌────────────────────────┐   │
    // │ │       plainptr         │   │
    // │ └───────────┬────────────┘   │
    // └─────────────┼────────────────┘
    //               │
    //               ▼
    // ┌─────────────────────────────────────────────────┐
    // │ strong_count: AtomicUsize                    (1)│
    // │ weak_count:   AtomicUsize                    (1)│
    // │ birth_era:    u64                             - │
    // │ data:         size::of::<AtomicArc<T>>        * │
    // │ ┌────────────────────────┐                      │
    // │ │      atomic_ptr        │                      │
    // │ └───────────┬────────────┘                      │
    // └─────────────┼───────────────────────────────────┘
    //               │
    //               ▼
    // ┌─────────────────────────────────────────────┐
    // │ strong_count: AtomicUsize                (4)│
    // │ weak_count:   AtomicUsize                (1)│ ...
    // │ birth_era:    u64                         - │
    // │ data:         size::of::<ApiData>         * │
    // └─────────────────────────────────────────────┘
    //               ▲
    //               │
    //               └─── atomic_ptr ◄─── plugin2&3_api_handle plainptr(Arc<AtomicPtr<T>>)
    //
    // if we hot realod the api, we only use the ApiRegistry methods.
    // still result in the same problem.
    //
    // Memory View
    //
    // API Registry Map:
    // ┌─────────────────┐ ┌─────────────────┐
    // │  AtomicArc<T>   │ │  AtomicArc<T>   │
    // └────────┬────────┘ └─────────────────┘
    //          │
    //          ▼
    // ┌─────────────────────────────────────┐  ┌─────────────────────────────────────┐
    // │       New Plugin Arc                │  │       Old Plugin Arc                │
    // ├─────────────────────────────────────┤  ├─────────────────────────────────────┤
    // │ strong_count: AtomicUsize        (1)│  │ strong_count: AtomicUsize        (3)│ ◄── atomic_ptr ── plugin1_api_handle
    // │ weak_count:   AtomicUsize        (1)│  │ weak_count:   AtomicUsize        (1)│ ◄── atomic_ptr ── plugin2_api_handle
    // │ birth_era:    u64                 - │  │ birth_era:    u64                 - │ ◄── atomic_ptr ── plugin3_api_handle
    // │ data:         size::of::<ApiData> * │  │ data:         size::of::<ApiData> * │ ◄── ...
    // └─────────────────────────────────────┘  └─────────────────────────────────────┘

    // 3.3 when a plugin get a api, if we create a new Arc from the Arc::from(&AtomicArc.load().unwrap()),
    //
    // Memory View:
    //
    // API Registry Map:
    // ┌───────────────────────┐ ┌───────────────────────┐
    // │   AtomicArc<T> <m1>   │ │   AtomicArc<T> <m2>   │
    // │ ┌───────────────────┐ │ │                       │
    // │ │    atomic_ptr     │ │ │                       │
    // │ └─────────┬─────────┘ │ │                       │
    // └───────────┼───────────┘ └───────────────────────┘
    //             │
    //             ▼
    // ┌─────────────────────────────────────────────────┐
    // │ strong_count: AtomicUsize                    (1)│
    // │ weak_count:   AtomicUsize                    (1)│
    // │ birth_era:    u64                             - │
    // │ data:         size::of::<ApiData>             * │
    // └─────────────────────────────────────────────────┘
    let plugin1_api_handle = bubble_core::sync::Arc::from(&atomic_ptr_in_registry.load().unwrap());
    let plugin2_api_handle = bubble_core::sync::Arc::from(&atomic_ptr_in_registry.load().unwrap());
    let plugin3_api_handle = bubble_core::sync::Arc::from(&atomic_ptr_in_registry.load().unwrap());
    // same as 3.1, but plugin1&2&3 ref to old plugin by plain ptr

    // 3.4 when a plugin get a api, if we create a new Arc from the a existing Arc build along with AtomicPtr,
    //
    // Memory View:
    //
    // API Registry Map:
    // ┌───────────────────────┐ ┌───────────────────────┐
    // │  Arc<AtomicArc<T>>    │ │  Arc<AtomicArc<T>>    │
    // │ ┌───────────────────┐ │ │                       │
    // │ │     plainptr      │ │ │                       │
    // │ └─────────┬─────────┘ │ │                       │
    // └───────────┼───────────┘ └───────────────────────┘
    //             │
    //             ▼
    // ┌─────────────────────────────────────────────────┐
    // │ strong_count: AtomicUsize                    (1)│
    // │ weak_count:   AtomicUsize                    (1)│
    // │ birth_era:    u64                             - │
    // │ data:         size::of::<AtomicArc>           * │
    // │ ┌───────────────────┐                           │
    // │ │atomic_ptr(detail) │                           │
    // │ └─────────┬─────────┘                           │
    // └───────────┼─────────────────────────────────────┘
    //             │
    //     ┌───────┘
    //     │
    //     ▼
    // ┌───────────────────────┐ ┌───────────────────────┐
    // │   AtomicArc<T> <m1>   │ │   AtomicArc<T> <m2>   │
    // │ ┌───────────────────┐ │ │                       │
    // │ │    atomic_ptr     │ │ │                       │
    // │ └─────────┬─────────┘ │ │                       │
    // └───────────┼───────────┘ └───────────────────────┘
    //             │
    //             ▼
    // ┌─────────────────────────────────────────────────┐
    // │ strong_count: AtomicUsize                    (1)│
    // │ weak_count:   AtomicUsize                    (1)│
    // │ birth_era:    u64                             - │
    // │ data:         size::of::<ApiData>             * │
    // └─────────────────────────────────────────────────┘
    // that's we store Arc<AtomicArc<T>> in the registry, instead of AtomicArc<T>
    let arc_atomic_ptr_in_registry =
        bubble_core::sync::Arc::new(bubble_core::sync::AtomicArc::new(api_data));
    let plugin1_api_handle = arc_atomic_ptr_in_registry.clone();
    let plugin2_api_handle = arc_atomic_ptr_in_registry.clone();
    let plugin3_api_handle = arc_atomic_ptr_in_registry.clone();
    // Memory View:
    //
    // API Registry Map:
    // ┌───────────────────────┐ ┌───────────────────────┐
    // │  Arc<AtomicArc<T>>    │ │  Arc<AtomicArc<T>>    │
    // │ ┌───────────────────┐ │ │                       │
    // │ │     plainptr      │ │ │                       │
    // │ └─────────┬─────────┘ │ │                       │
    // └───────────┼───────────┘ └───────────────────────┘
    //             │
    //             ▼
    // ┌─────────────────────────────────────────────────┐
    // │ strong_count: AtomicUsize                    (4)│ ◄── plugin1_api_handle
    // │ weak_count:   AtomicUsize                    (1)│
    // │ birth_era:    u64                             - │
    // │ data:         size::of::<AtomicArc>           * │
    // │ ┌───────────────────┐                           │
    // │ │atomic_ptr(detail) │                           │ ◄── plugin2_api_handle ...
    // │ └─────────┬─────────┘                           │
    // └───────────┼─────────────────────────────────────┘
    //             │
    //     ┌───────┘
    //     │
    //     ▼
    // ┌───────────────────────┐
    // │   AtomicArc<T> <m1>   │
    // │ ┌───────────────────┐ │
    // │ │    atomic_ptr     │ │
    // │ └─────────┬─────────┘ │
    // └───────────┼───────────┘
    //             │
    //             ▼
    // ┌─────────────────────────────────────────────────┐
    // │ strong_count: AtomicUsize                    (1)│ +? in plugin procs, drop delayed by guard in plugin
    // │ weak_count:   AtomicUsize                    (1)│
    // │ birth_era:    u64                             - │
    // │ data:         size::of::<ApiData>             * │
    // └─────────────────────────────────────────────────┘

    // Notes:
    // 1. When code in plugin1 wants to use the API, it will call:
    //    Arc::from(&plugin1_api_handle.load().unwrap())
    // 2. Or plugin1_api_handle.load().unwrap() -> Guard, which will update
    //    the underlying AtomicArc<T>'s strong_count
    // 3. Be careful with Arc<internal mut> between await points

    // if we reload the plugin, we need to update the atomic_arc
    // Memory View:
    //
    // API Registry Map:
    // ┌───────────────────────┐ ┌───────────────────────┐
    // │  Arc<AtomicArc<T>>    │ │  Arc<AtomicArc<T>>    │
    // │ ┌───────────────────┐ │ │                       │
    // │ │     plainptr      │ │ │                       │
    // │ └─────────┬─────────┘ │ │                       │
    // └───────────┼───────────┘ └───────────────────────┘
    //             │
    //             ▼
    // ┌─────────────────────────────────────────────────┐
    // │ strong_count: AtomicUsize                    (4)│ ◄── plugin1_api_handle
    // │ weak_count:   AtomicUsize                    (1)│ ◄── plugin2_api_handle
    // │ birth_era:    u64                             - │ ◄── plugin3_api_handle
    // │ data:         size::of::<AtomicArc>           * │
    // │ ┌───────────────────┐                           │
    // │ │atomic_ptr(detail) │                           │
    // │ └─────────┬─────────┘                           │
    // └───────────┼─────────────────────────────────────┘
    //             │
    //     ┌───────┘
    //     │
    //     ▼
    // ┌───────────────────────┐                       ┌───────────────────────┐
    // │   newAtomicArc<T>     │                       │   oldAtomicArc<T>     │
    // │ ┌───────────────────┐ │                       │ ┌───────────────────┐ │
    // │ │ AtomicArc<T> <m1> │ │                       │ │ AtomicArc<T> <m1> │ │
    // │ │    atomic_ptr     │ │                       │ │    atomic_ptr     │ │
    // │ └─────────┬─────────┘ │                       │ └─────────┬─────────┘ │
    // └───────────┼───────────┘                       └───────────┼───────────┘
    //             │                                               │
    //             ▼                                               ▼
    // ┌─────────────────────────────────────────┐   ┌─────────────────────────────────────────┐
    // │ strong_count: AtomicUsize            (1)│   │ strong_count: AtomicUsize            (1)│
    // │ weak_count:   AtomicUsize            (1)│   │ weak_count:   AtomicUsize            (1)│
    // │ birth_era:    u64                     - │   │ birth_era:    u64                     - │
    // │ data:         size::of::<ApiData>     * │   │ data:         size::of::<ApiData>     * │
    // └─────────────────────────────────────────┘   └─────────────────────────────────────────┘
    //   +? in plugin procs                            +? in plugin procs (mainly async),
    //                                                 drop delayed by guard in plugin
}

static CONTAINER: OnceLock<std::sync::Arc<Container<PluginApi>>> = OnceLock::new();
static TASK_DISPATCHER: OnceLock<Dispatcher> = OnceLock::new();
static API_REGISTRY: OnceLock<ApiRegistry> = OnceLock::new();

fn main() {
    // get current working directory
    let cwd = std::env::current_dir().unwrap();
    println!("Current working directory: {:?}", cwd);
    let dll_name = [cwd.join("./target/debug/plugin.dll")];

    let container = CONTAINER
        .get_or_init(|| std::sync::Arc::new(unsafe { Container::load(&dll_name[0]) }.unwrap()));

    // get context, and prepare apis
    let context = Context::get();
    const THREAD_NUM: usize = 8;
    let _dispatcher = Dispatcher::builder()
        .worker_threads(NonZeroUsize::new(THREAD_NUM).unwrap())
        .thread_names(|index| format!("compio-worker-{index}"))
        .build()
        .unwrap();
    let _ = TASK_DISPATCHER.set(_dispatcher);
    let task_api = TaskSystem {
        dispatcher: TASK_DISPATCHER.get().unwrap(),
    };
    let api_registry = API_REGISTRY.get_or_init(|| ApiRegistry::new());

    api_registry.set(task_api);

    (container.load_plugin)(&context, &api_registry);

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

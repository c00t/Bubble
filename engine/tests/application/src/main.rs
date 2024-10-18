use std::{any::TypeId, sync::OnceLock, thread};

use async_ffi::FutureExt;
use bubble_core::api::prelude::*;
use bubble_core::sync::RefCount;
use bubble_core::{
    api::api_registry_api::get_api_registry_api,
    os::{thread::Context, SysThreadId},
    sync::AtomicArc,
};
use bubble_tasks::runtime::Runtime;
use dlopen2::wrapper::{Container, WrapperApi};
use shared::{self as sd, TaskSystemApi, TraitCastableDropSub, TraitCastableDropSuper};

#[derive(WrapperApi)]
pub struct PluginApi {
    load_plugin: fn(context: &Context, api_registry: ApiHandle<dyn ApiRegistryApi>),
    test_dynamic_tls: fn(),
    plugin_task: fn(s: String) -> async_ffi::FfiFuture<String>,
    test_typeid: fn(ids: (TypeId, TypeId, TypeId)),
    unload_plugin: fn(),
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
    let main_struct = MAIN.get().unwrap().load().unwrap();
    // meature time
    let start = std::time::Instant::now();
    let r = main_struct.task_system_api.get().unwrap().dispatch(
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
    let _ = runtime.block_on(r);
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

pub fn test_check_strong_count(
    task_system: ApiHandle<dyn TaskSystemApi>,
    api_registry: ApiHandle<dyn ApiRegistryApi>,
) {
    // print task_system & api_registry strong count
    println!(
        "ApiHandle<task_system> strong count: {}",
        task_system.strong_count()
    );
    println!(
        "ApiHandle<api_registry> strong count: {}",
        api_registry.strong_count()
    );

    // print local_hanler strong count, that's the atomicarc strong count
    println!(
        "LocalHandle<task_system> strong count: {}",
        task_system.get().unwrap().strong_count()
    );
    println!(
        "LocalHandle<api_registry> strong count: {}",
        api_registry.get().unwrap().strong_count()
    );

    // use it carefully, it will increment the era inside a shared context
    // it can be called at the end of each frame.
    bubble_core::sync::increment_era();
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

static MAIN: OnceLock<AtomicArc<Main>> = OnceLock::new();

pub struct Main {
    api_registry_api: ApiHandle<dyn ApiRegistryApi>,
    task_system_api: ApiHandle<dyn TaskSystemApi>,
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
    let context = Context::get();

    let api_registry_api = get_api_registry_api(); // api: strong count 1,2

    // TEST0 set after load
    println!("--TEST0-->");
    // for each plugin, run load_plugin, pass clone() of ApiHandle<dyn ApiRegistryApi> to it.
    // 1. load plugin
    (container.load_plugin)(&context, api_registry_api.clone()); // task: strong count 1,2, api: clone then dropped
    (container2.load_plugin)(&context, api_registry_api.clone()); // task: strong count 3, api: clone then dropped

    // 2. set api
    println!("...");
    let task_system_api = shared::register_task_system_api(&api_registry_api); // task: strong count 4
    println!("...");
    let _ = MAIN.get_or_init(|| {
        AtomicArc::new(Main {
            api_registry_api: api_registry_api.clone(), // api: strong count 3
            task_system_api: task_system_api.clone(),   // task: strong count:5
        })
    });
    println!("<--TEST0--");

    for i in 0..30 {
        // 7, 5
        println!("<<<AFTER TEST0");
        // if your don't clone it, the arc will never be dropped.
        test_check_strong_count(task_system_api.clone(), api_registry_api.clone());
        println!("<<<");
    }

    // TEST1
    println!("--TEST1->");
    test_dynamic_tls(&container);
    test_dynamic_tls(&container2);
    println!("<-TEST1--");

    // TEST2
    println!("--TEST2->");
    test_bubble_tasks(&container); // task: strong count 6
    test_bubble_tasks(&container2); // task: strong count 7
    println!("<-TEST2--");

    for i in 0..30 {
        println!("<<<AFTER TEST2");
        // 9,5
        // if your don't clone it, the arc will never be dropped.
        test_check_strong_count(task_system_api.clone(), api_registry_api.clone());
        println!("<<<");
    }

    // TEST3
    println!("--TEST3->");
    test_typeid(&container);
    test_typeid(&container2);
    println!("<-TEST3--");

    // thread::sleep(std::time::Duration::from_secs(10));

    // remove task system api
    let q = api_registry_api
        .get()
        .unwrap()
        .remove(shared::constants::NAME, shared::constants::VERSION);
    drop(q); // task: strong count 6
    container.unload_plugin(); // task: strong count 5
    container2.unload_plugin(); // task: strong count 4

    for i in 0..30 {
        // 6,4
        println!("<<<AFTER UNLOAD");
        // if your don't clone it, the arc will never be dropped.
        test_check_strong_count(task_system_api.clone(), api_registry_api.clone());
        println!("<<<");
    }

    MAIN.get()
        .unwrap()
        .load()
        .unwrap()
        .task_system_api
        .get()
        .unwrap()
        .shutdown();
    MAIN.get()
        .unwrap()
        .load()
        .unwrap()
        .api_registry_api
        .get()
        .unwrap()
        .shutdown();
    MAIN.get().unwrap().store::<bubble_core::sync::Arc<_>>(None); // api: strong count 2, task: strong count 3

    // TEST4
    println!("--TEST4->");
    println!("--You should see strong count increase then drop--");
    for i in 0..30 {
        // 5,4
        println!("<<<");
        // if your don't clone it, the arc will never be dropped.
        test_check_strong_count(task_system_api.clone(), api_registry_api.clone());
        println!("<<<");
    }
    println!("<-TEST4--");

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

    thread::sleep(std::time::Duration::from_secs(10));
}

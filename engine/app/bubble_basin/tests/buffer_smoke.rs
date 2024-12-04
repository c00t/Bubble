use std::{collections::HashMap, hash::BuildHasher, sync::Arc};

use bubble_basin::buffer::{
    BasinBufferApi, Buffer, BufferAsyncResult, BufferEvictionLifeCycle, BufferId, BufferOptions,
    BufferStorage, LayeredLifecycle,
};
use bubble_core::{
    api::{api_registry_api, prelude::*},
    sync::circ::Rc,
    tracing,
    utils::hash::RapidInlineBuildHasher,
};
use bubble_tasks::{
    async_ffi::FutureExt,
    futures_util::{stream::futures_unordered, StreamExt},
    types::TaskSystemApi,
};
use bubble_tests::{utils::mimic_game_tick, TestSkeleton};
use quick_cache::{
    sync::Cache, DefaultHashBuilder, Options, OptionsBuilder, UnitWeighter, Weighter,
};
use rand::{seq::SliceRandom, Rng};
use sharded_slab::Slab;

#[derive(Clone)]
struct TestWeighter;

impl Weighter<BufferId, Rc<Buffer>> for TestWeighter {
    fn weight(&self, _: &BufferId, buffer: &Rc<Buffer>) -> u64 {
        unsafe { buffer.deref() }.len() as u64
    }
}

fixed_type_id_without_version_hash! {
    bubble_basin::tests::TestWeighter
}

#[test]
pub fn smoke() {
    let files = create_test_files();
    let mut hashmap = HashMap::new();

    let test = TestSkeleton::new();
    let data_slab = Arc::new(Slab::new());
    let life_cycle = BufferEvictionLifeCycle::new(data_slab.clone());
    let disk_cache = Arc::new(Cache::with_options(
        OptionsBuilder::new()
            // Make sure that the capacity matches the weight.
            .estimated_items_capacity(10)
            // Make the weight is the size of the buffer in bytes, we premit max 20MB is test
            // Note that the this weight will affect the memory usage of the disk cache under
            // extreme thread race. You should set it according to your machine's memory size and
            // test scenario.
            .weight_capacity(100 * 1024 * 1024)
            .build()
            .unwrap(),
        TestWeighter {},
        // for
        RapidInlineBuildHasher::default(),
        life_cycle,
    ));

    let buffer_storage = BufferStorage {
        task_system: test.task_system_api.clone(),
        data: data_slab,
        disk_cache: disk_cache.clone(),
    };

    // get buffer api
    // TODO: it may should be an interface instead of a api
    let buffer_api: ApiHandle<dyn BasinBufferApi> = {
        let anyapi: AnyApiHandle = Box::new(buffer_storage).into();
        anyapi.downcast()
    };
    let cs = circ::cs();
    let api_registry_api = test
        .api_registry_api
        .get(&cs)
        .expect("failed to get api registry api");
    api_registry_api.local_set::<dyn BasinBufferApi>(buffer_api.clone(), None);
    let local_buffer_api = buffer_api.get(&cs).expect("failed to get buffer api");

    let id = local_buffer_api.add(BufferOptions::LocalStorage {
        hash: None,
        filename: "Cargo.toml".to_string(),
        offset: None,
        size: None,
    });
    let x = local_buffer_api.get(id).unwrap();
    let len = x.as_ref().unwrap().len();

    tracing::info!("{}", len);

    // add files to buffer
    for (filename, size) in files.iter() {
        let filename = filename.path().to_string_lossy().to_string();
        let id = local_buffer_api.add(BufferOptions::LocalStorage {
            hash: None,
            filename: filename.clone(),
            offset: None,
            size: Some(*size),
        });
        hashmap.insert(id, filename);
    }

    drop(cs);

    let cs = circ::cs();

    let local_task_system_api = test.task_system_api.get(&cs).unwrap();

    // drop(cs);
    // let mut handles = vec![];
    // // Spawn 16 threads, about 1GB.
    // for _ in 0..16 {
    //     let buffer_api_clone = buffer_api.clone();
    //     let hashmap_clone = hashmap.clone();

    //     let handle = std::thread::spawn(move || {
    //         // Do more iterations per thread
    //         for _ in 0..1 {
    //             let mut rng = rand::thread_rng();
    //             let mut guard = circ::cs();
    //             let mut vec: Vec<(BufferId, String)> = hashmap_clone.iter()
    //                 .map(|(id, filename)| (*id, filename.clone()))
    //                 .collect();
    //             vec.shuffle(&mut rng);

    //             for (id, _) in vec {
    //                 let local_buffer_api = buffer_api_clone.get(&guard).unwrap();
    //                 let _x = local_buffer_api.get(id).unwrap();
    //                 drop(_x);
    //                 guard.reactivate();
    //             }
    //             guard.flush();
    //             drop(guard);
    //         }
    //     });

    //     handles.push(handle);
    // }

    // // Wait for all threads to complete
    // for handle in handles {
    //     handle.join().unwrap();
    // }

    // tracing::info!("End of std::thread::spawn");

    // // Test `local_buffer_api.get(id)`
    // for _ in 0..100 {
    //     let buffer_api_clone = buffer_api.clone();
    //     let hashmap_clone = hashmap.clone();
    //     let Ok(_) = local_task_system_api.dispatch_blocking(
    //         None,
    //         Box::new(move || {
    //             let mut rng = rand::thread_rng();
    //             let mut guard = circ::cs();
    //             let mut vec: Vec<(BufferId, String)> = hashmap_clone
    //                 .iter()
    //                 .map(|(id, filename)| (*id, filename.clone()))
    //                 .collect();
    //             vec.shuffle(&mut rng);
    //             for (id, filename) in vec {
    //                 let local_buffer_api = buffer_api_clone.get(&guard).unwrap();
    //                 let _x = local_buffer_api.get(id).unwrap();
    //                 drop(_x);
    //                 guard.reactivate();
    //             }
    //             // !!!!!!!!
    //             // Make sure that you flush the guard when use it in thread pool.
    //             guard.flush();
    //             drop(guard);
    //         }),
    //     ) else {
    //         panic!()
    //     };
    // }
    // drop(cs);
    // // end test

    // // Test `local_buffer_api.get_snapshot(id)`
    // // The memory reclaim time should be shorter compared to `local_buffer_api.get(id)`.
    // for _ in 0..100 {
    //     let buffer_api_clone = buffer_api.clone();
    //     let hashmap_clone = hashmap.clone();
    //     let Ok(_) = local_task_system_api.dispatch_blocking(
    //         None,
    //         Box::new(move || {
    //             let mut rng = rand::thread_rng();
    //             let mut guard = circ::cs();
    //             let mut vec: Vec<(BufferId, String)> = hashmap_clone
    //                 .iter()
    //                 .map(|(id, filename)| (*id, filename.clone()))
    //                 .collect();
    //             vec.shuffle(&mut rng);
    //             for (id, filename) in vec {
    //                 let local_buffer_api = buffer_api_clone.get(&guard).unwrap();
    //                 let _x = local_buffer_api.get_snapshot(id, &guard).unwrap();
    //                 // !!!!!!!!
    //                 // reactivate frequently will shorten the memory reclaim time, **by eagerly advancing the epoch**.
    //                 // If disabled, the memory will be reclaimed epoch by epoch.
    //                 guard.reactivate();
    //             }
    //             // !!!!!!!!
    //             // Make sure that you flush the guard when use it in thread pool.
    //             // Or the memory can't be reclaimed.
    //             guard.flush();
    //             drop(guard);
    //         }),
    //     ) else {
    //         panic!()
    //     };
    // }
    // drop(cs);
    // // end test

    // Test `local_buffer_api.get_async(id)`
    for _ in 0..100 {
        let buffer_api_clone = buffer_api.clone();
        let hashmap_clone = hashmap.clone();
        let Ok(_) = local_task_system_api.dispatch(
            None,
            Box::new(move || {
                async move {
                    let mut rng = rand::thread_rng();
                    let mut guard = circ::cs();
                    let mut vec: Vec<(BufferId, String)> = hashmap_clone
                        .iter()
                        .map(|(id, filename)| (*id, filename.clone()))
                        .collect();
                    vec.shuffle(&mut rng);
                    for (id, filename) in vec {
                        let local_buffer_api = buffer_api_clone.get(&guard).unwrap();
                        // let _x = local_buffer_api.get_snapshot(id, &guard).unwrap();
                        match local_buffer_api.get_async(id).unwrap() {
                            BufferAsyncResult::Loaded(x) => {
                                drop(x);
                            }
                            BufferAsyncResult::Loading(ch) => match ch.await.unwrap() {
                                Some(x) => {
                                    drop(x);
                                }
                                None => {
                                    tracing::warn!(
                                        "buffer meta has been removed from buffer storage"
                                    );
                                }
                            },
                        }
                        // !!!!!!!!
                        // reactivate frequently will shorten the memory reclaim time, **by eagerly advancing the epoch**.
                        // If disabled, the memory will be reclaimed epoch by epoch.
                        guard.reactivate();
                    }
                    // !!!!!!!!
                    // Make sure that you flush the guard when use it in thread pool.
                    // Or the memory can't be reclaimed.
                    guard.flush();
                    drop(guard);
                }
                .into_local_ffi()
            }),
        ) else {
            panic!()
        };
    }
    drop(cs);
    // end test

    // set tick end to 1000
    bubble_tests::utils::set_counter(1000);

    // let tick_thread = test.create_tick_thread(async_tick,disk_cache);

    // tick_thread.join().unwrap();

    let runtime = bubble_tasks::runtime::Runtime::new().unwrap();
    let task_system_api_clone = test.task_system_api.clone();
    let buffer_api_clone = buffer_api.clone();
    let hashmap_clone = hashmap.clone();
    runtime.block_on(async move {
        while async_tick(
            task_system_api_clone.clone(),
            buffer_api_clone.clone(),
            hashmap_clone.clone(),
            disk_cache.clone(),
        )
        .await
        {}
    });

    test.end_test();
}

fn create_test_files() -> Vec<(tempfile::NamedTempFile, usize)> {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut files = Vec::new();

    for _ in 0..20 {
        let size = rand::thread_rng().gen_range(0..(10 * 1024 * 1024));

        let mut temp_file = NamedTempFile::new().unwrap();
        let data: Vec<u8> = (0..size).map(|_| rand::random::<u8>()).collect();
        temp_file.write_all(&data).unwrap();
        temp_file.flush().unwrap();

        files.push((temp_file, size));
    }

    files
}

async fn async_tick(
    task_system_api: ApiHandle<dyn TaskSystemApi>,
    buffer_api: ApiHandle<dyn BasinBufferApi>,
    hashmap: HashMap<BufferId, String>,
    disk_cache: Arc<
        Cache<BufferId, Rc<Buffer>, TestWeighter, RapidInlineBuildHasher, BufferEvictionLifeCycle>,
    >,
) -> bool {
    let b = mimic_game_tick(()).await;
    // disk_cache.clear();
    // tracing::info!("weight: {}", disk_cache.weight());

    b
}

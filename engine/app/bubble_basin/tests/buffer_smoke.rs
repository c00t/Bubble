use std::{
    collections::HashMap,
    hash::BuildHasher,
    path::PathBuf,
    sync::{atomic::AtomicUsize, Arc},
};

use bubble_basin::buffer::{
    BasinBufferApi, Buffer, BufferAsyncResult, BufferEvictionLifeCycle, BufferId, BufferObject,
    BufferOptions, BufferStorage, LayeredLifecycle,
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
    sync::Cache, DefaultHashBuilder, Lifecycle, Options, OptionsBuilder, UnitWeighter, Weighter,
};
use rand::{seq::SliceRandom, Rng};
use rkyv::util::AlignedVec;
use sharded_slab::Slab;
use tempfile::{NamedTempFile, TempPath};

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
    let test_skeleton = TestSkeleton::new();
    let data_slab = Arc::new(Slab::new());
    let (buffer_api, disk_cache, data_slab) = create_test_buffer_api(
        data_slab.clone(),
        10,
        80 * 1024 * 1024,
        test_skeleton.task_system_api.clone(),
        TestWeighter {},
        RapidInlineBuildHasher::default(),
        BufferEvictionLifeCycle::new(data_slab),
    );
    test_skeleton.add_api_to_registry(buffer_api.clone());

    // prepare data
    let (memory_buffers, local_storage_buffers) =
        prepare_raw_data(20, 10 * 1024 * 1024, 20, 10 * 1024 * 1024);
    let spilt_point = 20;
    let buffer_options = prepare_buffer_options(memory_buffers, &local_storage_buffers);
    let buffer_ids = add_buffers_to_storage(buffer_options, buffer_api.clone());

    tracing::info!(
        "End of prepare data, total buffer count: {}",
        buffer_ids.len()
    );

    // Functions Unit Test
    //
    // test_buffer_get_with_threads(buffer_api.clone(), buffer_ids);
    // test_buffer_get(test_skeleton.task_system_api.clone(), buffer_api.clone(), buffer_ids);
    // test_buffer_get_snapshot(test_skeleton.task_system_api.clone(),buffer_api.clone(),buffer_ids);
    test_buffer_get_async(
        test_skeleton.task_system_api.clone(),
        buffer_api.clone(),
        buffer_ids,
    );

    // // simple test `local_buffer_api.to_local_storage(id, filename)`
    // for (id, _) in memory_buffer_hashmap.clone() {
    //     let buffer_api_clone = buffer_api.clone();
    //     let temp_path = NamedTempFile::new().unwrap().into_temp_path();
    //     let _ = local_task_system_api.dispatch(
    //         None,
    //         Box::new(move || {
    //             async move {
    //                 let mut guard = circ::cs();
    //                 let local_buffer_api = buffer_api_clone.get(&guard).unwrap();
    //                 let ch = local_buffer_api
    //                     .to_local_storage(id, temp_path.to_path_buf())
    //                     .unwrap();
    //                 guard.reactivate();
    //                 guard.flush();
    //                 drop(guard);
    //             }
    //             .into_local_ffi()
    //         }),
    //     );
    // }
    // drop(cs);
    // // end test

    let guard = circ::cs();
    guard.flush();
    drop(guard);

    // set tick end to 1000
    bubble_tests::utils::set_counter(1000);
    let regular_log_info = RegularLogInfo::new(disk_cache.clone(), data_slab.clone());
    run_async_runtime(
        test_skeleton.task_system_api.clone(),
        buffer_api.clone(),
        regular_log_info,
    );
    drop(local_storage_buffers);
    test_skeleton.end_test();
}

/// Prepare data for buffer test
///
/// create `memory_count` memory buffers and `local_storage_count` local storage buffers
fn prepare_raw_data(
    memory_count: usize,
    max_memory_size: usize,
    local_storage_count: usize,
    max_local_storage_size: usize,
) -> (Vec<AlignedVec>, Vec<(TempPath, usize)>) {
    use std::{io::Write, sync::mpsc, thread};
    use tempfile::NamedTempFile;

    // Number of threads to use (adjust based on system)
    let thread_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    // Channels for collecting results
    let (mem_tx, mem_rx) = mpsc::channel();
    let (storage_tx, storage_rx) = mpsc::channel();

    // Calculate chunks for each thread
    let mem_chunk_size = (memory_count + thread_count - 1) / thread_count;
    let storage_chunk_size = (local_storage_count + thread_count - 1) / thread_count;

    let mut handles = Vec::new();

    // Spawn threads for memory buffers
    for thread_idx in 0..thread_count {
        let tx = mem_tx.clone();
        let start = thread_idx * mem_chunk_size;
        let end = (start + mem_chunk_size).min(memory_count);

        if start >= end {
            continue;
        }

        handles.push(thread::spawn(move || {
            let mut chunk_results = Vec::with_capacity(end - start);
            for _ in start..end {
                let size = rand::thread_rng().gen_range(0..max_memory_size);
                let mut align_vec = AlignedVec::with_capacity(size);
                let content: Vec<u8> = (0..size).map(|_| rand::random::<u8>()).collect();
                align_vec.extend_from_slice(&content);
                chunk_results.push(align_vec);
            }
            tx.send(chunk_results).unwrap();
        }));
    }

    // Spawn threads for storage buffers
    for thread_idx in 0..thread_count {
        let tx = storage_tx.clone();
        let start = thread_idx * storage_chunk_size;
        let end = (start + storage_chunk_size).min(local_storage_count);

        if start >= end {
            continue;
        }

        handles.push(thread::spawn(move || {
            let mut chunk_results = Vec::with_capacity(end - start);
            for _ in start..end {
                let size = rand::thread_rng().gen_range(0..max_local_storage_size);
                let mut temp_file = NamedTempFile::new().unwrap();
                let data: Vec<u8> = (0..size).map(|_| rand::random::<u8>()).collect();
                temp_file.write_all(&data).unwrap();
                temp_file.flush().unwrap();
                chunk_results.push((temp_file.into_temp_path(), size));
            }
            tx.send(chunk_results).unwrap();
        }));
    }

    // Drop extra sender handles
    drop(mem_tx);
    drop(storage_tx);

    // Collect results
    let mut memory_buffers = Vec::with_capacity(memory_count);
    let mut local_storage_buffers = Vec::with_capacity(local_storage_count);

    // Collect memory buffer results
    while let Ok(chunk) = mem_rx.recv() {
        memory_buffers.extend(chunk);
    }

    // Collect storage buffer results
    while let Ok(chunk) = storage_rx.recv() {
        local_storage_buffers.extend(chunk);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    (memory_buffers, local_storage_buffers)
}

/// Make the data returned by `prepare_raw_data` into buffer add options
fn prepare_buffer_options(
    memory_buffers: Vec<AlignedVec>,
    local_storage_buffers: &Vec<(TempPath, usize)>,
) -> Vec<BufferOptions> {
    let mut buffer_options = Vec::new();

    for memory_buffer in memory_buffers {
        buffer_options.push(BufferOptions::Memory {
            hash: None,
            data: memory_buffer,
        });
    }

    for (temp_path, size) in local_storage_buffers {
        buffer_options.push(BufferOptions::LocalStorage {
            hash: None,
            filepath: temp_path.to_path_buf(),
            offset: None,
            size: Some(*size),
        });
    }

    buffer_options
}

/// Add all buffers into buffer storage
///
/// return the mix of buffer ids and buffer options
///
/// Note that the sequence of buffer ids is the same as the sequence of buffer options,
/// Vec<memory buffers first, then local storage buffers>
fn add_buffers_to_storage(
    buffer_options: Vec<BufferOptions>,
    buffer_api: ApiHandle<dyn BasinBufferApi>,
) -> Vec<BufferId> {
    let cs = circ::cs();
    let local_buffer_api = buffer_api.get(&cs).unwrap();
    let mut buffer_ids = Vec::new();
    for buffer_option in buffer_options {
        buffer_ids.push(local_buffer_api.add(buffer_option));
    }
    buffer_ids
}

// TODO: it may should be an interface instead of a api
fn create_test_buffer_api<W, H, L>(
    data_slab: Arc<Slab<BufferObject>>,
    disk_cache_capacity: usize,
    disk_cache_weight_capacity: u64,
    task_system_api: ApiHandle<dyn TaskSystemApi>,
    weighter: W,
    hasher: H,
    life_cycle: L,
) -> (
    ApiHandle<dyn BasinBufferApi>,
    Arc<Cache<BufferId, Rc<Buffer>, W, H, L>>,
    Arc<Slab<BufferObject>>,
)
where
    W: Weighter<BufferId, Rc<Buffer>> + FixedTypeId + Clone + Send + Sync + 'static,
    H: BuildHasher + Clone + FixedTypeId + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>
        + FixedTypeId
        + Clone
        + Send
        + Sync
        + 'static,
{
    let disk_cache = Arc::new(Cache::with_options(
        OptionsBuilder::new()
            .estimated_items_capacity(disk_cache_capacity)
            .weight_capacity(disk_cache_weight_capacity)
            .build()
            .unwrap(),
        weighter,
        hasher,
        life_cycle,
    ));
    let buffer_api: AnyApiHandle = Box::new(BufferStorage {
        task_system: task_system_api,
        data: data_slab.clone(),
        disk_cache: disk_cache.clone(),
    })
    .into();
    (buffer_api.downcast(), disk_cache, data_slab)
}

/// Test buffer get() using std::thread
fn test_buffer_get_with_threads(
    buffer_api: ApiHandle<dyn BasinBufferApi>,
    buffer_ids: Vec<BufferId>,
) {
    let mut handles = vec![];
    // Spawn 16 threads, about 1GB.
    for _ in 0..16 {
        let buffer_api_clone = buffer_api.clone();
        let buffer_ids_clone = buffer_ids.clone();

        let handle = std::thread::spawn(move || {
            for _ in 0..1 {
                let mut rng = rand::thread_rng();
                let mut guard = circ::cs();
                let mut shuffle_buffer_ids: Vec<BufferId> =
                    buffer_ids_clone.iter().map(|id| (*id)).collect();
                shuffle_buffer_ids.shuffle(&mut rng);

                for id in shuffle_buffer_ids {
                    let local_buffer_api = buffer_api_clone.get(&guard).unwrap();
                    let x = local_buffer_api.get(id).unwrap();
                    drop(x);
                    guard.reactivate();
                }
                guard.flush();
                drop(guard);
            }
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    tracing::info!("End of std::thread::spawn");
}

/// Test buffer get() using task system
fn test_buffer_get(
    task_system_api: ApiHandle<dyn TaskSystemApi>,
    buffer_api: ApiHandle<dyn BasinBufferApi>,
    buffer_ids: Vec<BufferId>,
) {
    let guard = circ::cs();
    let local_task_system_api = task_system_api.get(&guard).unwrap();
    for _ in 0..100 {
        let buffer_api_clone = buffer_api.clone();
        let buffer_ids_clone = buffer_ids.clone();
        let Ok(_) = local_task_system_api.dispatch_blocking(
            None,
            Box::new(move || {
                let mut rng = rand::thread_rng();
                let mut guard = circ::cs();
                let mut shuffle_buffer_ids: Vec<BufferId> = buffer_ids_clone;
                shuffle_buffer_ids.shuffle(&mut rng);
                for id in shuffle_buffer_ids {
                    let local_buffer_api = buffer_api_clone.get(&guard).unwrap();
                    let x = local_buffer_api.get(id).unwrap();
                    drop(x);
                    guard.reactivate();
                }
                // Make sure that you flush the guard when use it in thread pool.
                guard.flush();
                drop(guard);
            }),
        ) else {
            panic!()
        };
    }
}

/// Test buffer get_snapshot() using task system
fn test_buffer_get_snapshot(
    task_system_api: ApiHandle<dyn TaskSystemApi>,
    buffer_api: ApiHandle<dyn BasinBufferApi>,
    buffer_ids: Vec<BufferId>,
) {
    let guard = circ::cs();
    let local_task_system_api = task_system_api.get(&guard).unwrap();
    // The memory reclaim time should be shorter compared to `local_buffer_api.get(id)`.
    for _ in 0..100 {
        let buffer_api_clone = buffer_api.clone();
        let buffer_ids = buffer_ids.clone();
        let Ok(_) = local_task_system_api.dispatch_blocking(
            None,
            Box::new(move || {
                let mut rng = rand::thread_rng();
                let mut guard = circ::cs();
                let mut shuffle_buffer_ids: Vec<BufferId> = buffer_ids;
                shuffle_buffer_ids.shuffle(&mut rng);
                for id in shuffle_buffer_ids {
                    let local_buffer_api = buffer_api_clone.get(&guard).unwrap();
                    let _ = local_buffer_api.get_snapshot(id, &guard).unwrap();
                    // reactivate frequently will shorten the memory reclaim time,
                    // by eagerly advancing the epoch.
                    // If disabled, the memory will be reclaimed epoch by epoch.
                    guard.reactivate();
                }
                // Make sure that you flush the guard when use it in thread pool.
                // Or the memory can't be reclaimed.
                guard.flush();
                drop(guard);
            }),
        ) else {
            panic!()
        };
    }
    drop(guard);
}

/// Test buffer get_async() using task system
fn test_buffer_get_async(
    task_system_api: ApiHandle<dyn TaskSystemApi>,
    buffer_api: ApiHandle<dyn BasinBufferApi>,
    buffer_ids: Vec<BufferId>,
) {
    let guard = circ::cs();
    let local_task_system_api = task_system_api.get(&guard).unwrap();
    for _ in 0..100 {
        let buffer_api_clone = buffer_api.clone();
        let buffer_ids = buffer_ids.clone();
        let Ok(_) = local_task_system_api.dispatch(
            None,
            Box::new(move || {
                async move {
                    let mut rng = rand::thread_rng();
                    let mut guard = circ::cs();
                    let mut shuffle_buffer_ids: Vec<BufferId> = buffer_ids.clone();
                    shuffle_buffer_ids.shuffle(&mut rng);
                    for id in shuffle_buffer_ids {
                        let local_buffer_api = buffer_api_clone.get(&guard).unwrap();
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
                        // reactivate frequently will shorten the memory reclaim time,
                        // by eagerly advancing the epoch.
                        // If disabled, the memory will be reclaimed epoch by epoch.
                        guard.reactivate();
                    }
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
}

/// Regular Log Info
#[derive(Clone)]
struct RegularLogInfo<W, H, L>
where
    W: Weighter<BufferId, Rc<Buffer>> + Clone + Send + Sync + 'static,
    H: BuildHasher + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()> + Clone + Send + Sync + 'static,
{
    disk_cache: Arc<Cache<BufferId, Rc<Buffer>, W, H, L>>,
    data_slab: Arc<Slab<BufferObject>>,
}

impl<W, H, L> RegularLogInfo<W, H, L>
where
    W: Weighter<BufferId, Rc<Buffer>> + Clone + Send + Sync + 'static,
    H: BuildHasher + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()> + Clone + Send + Sync + 'static,
{
    fn new(
        disk_cache: Arc<Cache<BufferId, Rc<Buffer>, W, H, L>>,
        data_slab: Arc<Slab<BufferObject>>,
    ) -> Self {
        Self {
            disk_cache,
            data_slab,
        }
    }

    fn log(&self) {
        tracing::info!("disk_cache weight: {}", self.disk_cache.weight());
    }
}

fn run_async_runtime<W, H, L>(
    task_system_api: ApiHandle<dyn TaskSystemApi>,
    buffer_api: ApiHandle<dyn BasinBufferApi>,
    regular_log_info: RegularLogInfo<W, H, L>,
) where
    W: Weighter<BufferId, Rc<Buffer>> + Clone + Send + Sync + 'static,
    H: BuildHasher + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()> + Clone + Send + Sync + 'static,
{
    let runtime = bubble_tasks::runtime::Runtime::new().unwrap();
    runtime.block_on(async move {
        let regular_log_info = regular_log_info.clone();
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        while async {
            if COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 50 == 0 {
                regular_log_info.clone().log();
            }
            let guard = circ::cs();
            let break_out = mimic_game_tick(()).await;
            guard.flush();
            break_out
        }
        .await
        {}
    });
}

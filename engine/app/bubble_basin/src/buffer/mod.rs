//! A buffer api, will be used to store blob data across the program.
//!
//! Different from BubbleBasin, which mainly store human readable metadata of objects,
//! buffer api is used to store binary data that need to be shared across the program, like mesh, image, audio, etc.
//! There is no need to make them human readable, metadata, like filename, width, height, etc, will stored beside them.
//! So we can make it read&write more efficiently with zero-copy.
//!

mod trait_cast_impl;

use std::borrow::Borrow;
use std::hash::{BuildHasher, Hash, Hasher};
use std::io::Read;
use std::num::NonZeroU64;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bubble_core::api::prelude::*;
use bubble_core::sync::circ::{AtomicRc, RcObject};
use bubble_core::tracing;
use bubble_core::utils::hash::rapidhash::RapidInlineHasher;
use bubble_tasks::async_ffi::FutureExt;
use bubble_tasks::buf::{IoBuf, IoBufMut, SetBufInit};
use bubble_tasks::futures_channel::oneshot;
use bubble_tasks::io::AsyncReadAtExt;
use bubble_tasks::types::DynTaskSystemApi;
use circ::{Rc, Snapshot};
use circ_ds::concurrent_map::OutputHolder;
use circ_ds::natarajan_mittal_tree::NMTreeMap;
use quick_cache::sync::{Cache, GuardResult};
use quick_cache::{DefaultHashBuilder, Lifecycle, Weighter};
pub use rkyv::util::AlignedVec;
use sharded_slab::Slab;
use url::Url;

/// The storage of the buffer.
///
/// Implement [`BasinBufferInterface`], which is the public interface of the buffer storage.
///
/// The eviction of the buffer backed by external storage(like local storage or url)
/// is handled by [`quick_cache`], when the quick_cache decides to evict a buffer, it will
/// remove the buffer from the internal storage.
///
/// The weight, hashbuilder and lifecycle type will be implemented by user.
#[define_interface(bubble_basin::buffer::BasinBufferInterface, skip_castable = true)]
pub struct BufferStorage<We, B, L>
where
    We: Weighter<BufferId, Rc<Buffer>> + FixedTypeId + Clone + Send + Sync + 'static,
    B: BuildHasher + FixedTypeId + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>
        + FixedTypeId
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub task_system: ApiHandle<DynTaskSystemApi>,
    /// The slab of the buffer, which will store the buffer data.
    ///
    /// Note that, buffer can only be removed from slab when eviction happens.
    ///
    /// Buffers in this slab may not be loaded into memory. And already loaded buffer will be evicted
    /// by [`BufferStorage::disk_cache`] through swap Rc<Buffer> with null.
    ///
    /// It may be used by weight and lifecycle, so add [`Arc`] here.
    pub data: Arc<Slab<BufferObject>>,
    /// The cache of the buffer, which will cached the already loaded buffer.
    ///
    /// It's a 1-to-1 mapping between BufferId and already loaded Rc<Buffer>.
    ///
    /// The `Rc<Buffer>` is stored for weight calculation. But it will delay memory reclamation.
    ///
    /// When eviction happens, BufferId will be removed from data slab.
    pub disk_cache: Arc<Cache<BufferId, Rc<Buffer>, We, B, L>>,
    // /// The lookup table from hash to runtime id.
    // ///
    // /// TODO: Do we really need to use buffer hash to
    // hash_to_runtime: NMTreeMap<u64, BufferId>,
}

#[derive(Clone)]
pub struct BufferEvictionLifeCycle {
    data: Arc<Slab<BufferObject>>,
}

fixed_type_id! {
    BufferEvictionLifeCycle
}

impl BufferEvictionLifeCycle {
    pub fn new(data: Arc<Slab<BufferObject>>) -> Self {
        Self { data }
    }
}

impl Lifecycle<BufferId, Rc<Buffer>> for BufferEvictionLifeCycle {
    type RequestState = ();

    fn begin_request(&self) -> Self::RequestState {
        ()
    }

    fn on_evict(&self, state: &mut Self::RequestState, key: BufferId, val: Rc<Buffer>) {
        drop(val);
        // the key might be removed from data slab before eviction by `BasinBufferApi::remove`
        let buffer_object = self.data.get(key.runtime_key()).unwrap();
        {
            let guard = circ::cs();
            let mut snapshot = buffer_object.data.load(Ordering::Acquire, &guard);
            // compare exchange until success
            while !snapshot.is_null() {
                if buffer_object
                    .data
                    .compare_exchange(
                        snapshot,
                        Rc::null(),
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                        &guard,
                    )
                    .is_ok()
                {
                    break;
                }
                tracing::info!("busy evicting buffer {key:?}");
                snapshot = buffer_object.data.load(Ordering::Acquire, &guard);
            }
            tracing::info!("evicted buffer {key:?}");
        };
    }
}

/// A lifecycle that layers multiple lifecycles.
///
/// TODO: delete it, because [`quick_cache`] needs [`Clone`] lifecycle when create new [`Cache`] instance.
pub struct LayeredLifecycle {
    layers: Vec<Box<dyn Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>>>,
}

impl Lifecycle<BufferId, Rc<Buffer>> for LayeredLifecycle {
    type RequestState = ();

    fn begin_request(&self) -> Self::RequestState {
        // Call begin_request on all layers
        for layer in &self.layers {
            layer.begin_request();
        }
        ()
    }

    fn is_pinned(&self, key: &BufferId, val: &Rc<Buffer>) -> bool {
        // Return true if any layer considers it pinned
        self.layers.iter().any(|layer| layer.is_pinned(key, val))
    }

    fn on_evict(&self, state: &mut Self::RequestState, key: BufferId, val: Rc<Buffer>) {
        // Call on_evict on all layers in reverse order
        for layer in self.layers.iter().rev() {
            layer.on_evict(state, key, val.clone());
        }
    }

    fn before_evict(&self, state: &mut Self::RequestState, key: &BufferId, val: &mut Rc<Buffer>) {
        // Call before_evict on all layers in reverse order
        for layer in self.layers.iter().rev() {
            layer.before_evict(state, key, val);
        }
    }

    fn end_request(&self, state: Self::RequestState) {
        // Call end_request on all layers
        for layer in &self.layers {
            layer.end_request(state);
        }
    }
}

impl LayeredLifecycle {
    /// Creates a new empty LayeredLifecycle with no layers
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Adds a new lifecycle layer to the end of the layers list
    pub fn add_layer(
        &mut self,
        layer: Box<dyn Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>>,
    ) {
        self.layers.push(layer);
    }

    /// Returns the number of layers
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Returns true if there are no layers
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

/// The id of a buffer in the cache, it can only be created from [`BufferId::Runtime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BufferCacheId(BufferId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BufferId {
    /// This type of id is the internal id in the buffer storage slab.
    Runtime(usize),
    /// This type of id is the hash of the buffer content, used for persistence.
    Hash(u64),
}

impl BufferId {
    /// Create a runtime id.
    pub fn runtime(id: usize) -> Self {
        Self::Runtime(id)
    }
    /// Create a hash id.
    pub fn hash(hash: u64) -> Self {
        Self::Hash(hash)
    }
    /// Create a cache id from a runtime id.
    fn to_cache_id(&self) -> Option<BufferCacheId> {
        match self {
            Self::Runtime(id) => Some(BufferCacheId(*self)),
            _ => None,
        }
    }
    #[inline]
    fn runtime_key(&self) -> usize {
        match self {
            Self::Runtime(id) => *id,
            _ => unreachable!(),
        }
    }
    #[inline]
    fn hash_key(&self) -> u64 {
        match self {
            Self::Hash(hash) => *hash,
            _ => unreachable!(),
        }
    }
}

/// The object of the buffer in the slab.
///
/// ## Note
///
/// The buffer object can only be added to [`BufferStorage`] through [`BasinBufferApi::add`],
/// and removed by [`BasinBufferApi::remove`].
///
/// The internal [`Buffer`] is atomically updated by disk cache and get operations.
///
/// The update of [`BufferObject::hash`] and [`BufferObject::meta`] is also atomically, but
/// separate from each other and the update of internal [`Buffer`].
///
/// The [`BufferObject::meta`] may be transitioned from memory to disk-backed but **not** from disk-backed to memory.
/// Becuase it's guaranteed that when:
///
/// 1. From [`BufferMeta::Memory`] to [`BufferMeta::LocalStorage`]: typically a save operation, the file will
///    be saved to disk before the [`BufferObject::meta`] is updated. And the internal [`Buffer`] will be swapped to null.
///    So you won't read a file which is being written.
///    
///    Request save -1-> save done -2-> atomic update meta -3-> atomic swap buffer to null -4-> end.
///    It's safe to get the buffer at any time, because it's read-only buffer.
///
/// 2. From [`BufferMeta::LocalStorage`] to [`BufferMeta::Memory`]: it's a infomation loss operation, the memory can't
///    be reclaimed, and disk cache will decide when to evict buffer. When the disk cache evicts the buffer, it will
///    swap the internal [`Buffer`] to null, it's not possible to look up the [`BufferObject::meta`] because it's a
///    separate atomic operation. I think it won't happen frequently, so it's acceptable.
///
pub struct BufferObject {
    /// The data of the buffer.
    ///
    /// If it's null, the buffer is not loaded into memory.
    ///
    /// TODO: It's the default alignment(16) sufficient for most cases?
    ///     should we also store the alignment inside BufferInner?
    data: AtomicRc<Buffer>,
    /// The hash of underlying data
    ///
    /// TODO: Do we really need to calculate this if the data is huge?
    pub hash: AtomicU64,
    meta: AtomicRc<BufferMeta>,
}

impl BufferObject {
    pub fn meta<'g>(&self, guard: &'g circ::Guard) -> Snapshot<'g, BufferMeta> {
        self.meta.load(Ordering::Acquire, guard)
    }
}

#[derive(Debug, Clone)]
pub enum BufferMeta {
    Memory,
    LocalStorage {
        filepath: PathBuf,
        offset: usize,
        size: Option<usize>,
    },
    Url {
        url: Url,
        offset: usize,
        size: Option<usize>,
    },
}

unsafe impl RcObject for BufferMeta {
    fn pop_edges(&mut self, out: &mut Vec<Rc<Self>>) {}
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Buffer(AlignedVec);

#[derive(Debug)]
pub struct RcBuffer(Rc<Buffer>);

unsafe impl IoBuf for RcBuffer {
    fn as_buf_ptr(&self) -> *const u8 {
        self.0.as_ref().unwrap().0.as_ptr()
    }

    fn buf_len(&self) -> usize {
        self.0.as_ref().unwrap().0.len()
    }

    fn buf_capacity(&self) -> usize {
        self.0.as_ref().unwrap().0.capacity()
    }
}

impl Deref for Buffer {
    type Target = AlignedVec;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Buffer {
    pub fn new(data: AlignedVec) -> Self {
        Self(data)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// [`BufferObject`] is a plain old data struct.
unsafe impl RcObject for Buffer {
    fn pop_edges(&mut self, out: &mut Vec<bubble_core::sync::circ::Rc<Self>>) {}
}

unsafe impl IoBuf for Buffer {
    fn as_buf_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }

    fn buf_len(&self) -> usize {
        self.0.len()
    }

    fn buf_capacity(&self) -> usize {
        self.0.capacity()
    }
}

unsafe impl IoBufMut for Buffer {
    fn as_buf_mut_ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr()
    }
}

impl SetBufInit for Buffer {
    unsafe fn set_buf_init(&mut self, len: usize) {
        self.0.set_len(len);
    }
}

#[derive(Debug)]
pub enum BufferOptions {
    /// The buffer will be loaded into memory.
    Memory { hash: Option<u64>, data: AlignedVec },
    /// The buffer will be loaded from local storage.
    LocalStorage {
        hash: Option<u64>,
        filepath: PathBuf,
        offset: Option<usize>,
        size: Option<usize>,
    },
    /// The buffer will be loaded from url.
    Url {
        hash: Option<u64>,
        url: Url,
        offset: Option<usize>,
        size: Option<usize>,
    },
}

pub enum BufferAsyncResult {
    /// The buffer is already loaded into memory.
    Loaded(Rc<Buffer>),
    /// The buffer is not loaded into memory, and the file is being loaded asynchronously.
    Loading(oneshot::Receiver<Option<Rc<Buffer>>>),
}

pub enum BufferAsyncSnapshotResult<'g> {
    /// The buffer is already loaded into memory.
    Loaded(Snapshot<'g, Buffer>),
    /// The buffer is not loaded into memory, and the file is being loaded asynchronously.
    Loading(oneshot::Receiver<Option<Snapshot<'g, Buffer>>>),
}

#[declare_interface((0,1,0), bubble_basin::buffer::BasinBufferInterface)]
pub trait BasinBufferInterface: Interface {
    /// Add a buffer to the storage, and return the id of the buffer.
    ///
    /// It will return a runtime id. Currently, this api supports 3 kinds of buffer options:
    ///
    /// - Memory: The buffer is a memory buffer, provided by user, with optional hash.
    ///
    ///   It will automatically calculate the hash if not provided. Because the data is already in memory, it's a fast operation.
    ///
    /// - LocalStorage: The buffer will be loaded from local storage.
    ///
    ///   This kinds of buffer is backed by file on disk, so it's a relatively slow operation. This operation will just add a metadata
    ///   to the buffer storage without loading the file into memory. If you already know the hash of this file, you can provide it as an option.
    ///   If there is a buffer with the same hash already in the storage, it will return the existing buffer's id.
    ///   **But your metadata provided will be ignored, the existing buffer's metadata will be used.**
    ///
    /// - Url: The buffer will be loaded from url.
    ///
    ///   This kinds of buffer is backed by file on internet, so it's also a relatively slow operation. This operation will just add a metadata
    ///   to the buffer storage without downloading the file into memory. If you already know the hash of this file, you can provide it as an option.
    ///   If there is a buffer with the same hash already in the storage, it will return the existing buffer's id.
    ///   **But your metadata provided will be ignored, the existing buffer's metadata will be used.**
    fn add(&self, options: BufferOptions) -> BufferId;

    /// Get the metadata of the buffer.
    fn meta<'g>(&self, id: BufferId, guard: &'g circ::Guard) -> Option<Snapshot<'g, BufferMeta>>;

    /// Remove a buffer from the storage.
    ///
    /// ## Note
    ///
    /// It's not recommended to use this api to remove buffer, the buffer may be reclaimed until
    /// the eviction callback is called.
    fn remove(&self, id: BufferId);

    /// Get the buffer [`circ::Rc`] from the storage, this function ensure that the buffer is loaded into memory.
    fn get(&self, id: BufferId) -> Option<Rc<Buffer>>;

    /// Get the buffer [`circ::Snapshot`] from the storage, this function ensure that the buffer is loaded into memory.
    ///
    /// This method may be more efficient than [`BasinBufferApi::get`] if you don't need to hold the buffer alive longer than the scope of the guard.
    fn get_snapshot<'g>(
        &self,
        id: BufferId,
        guard: &'g circ::Guard,
    ) -> Option<Snapshot<'g, Buffer>>;

    /// Convert the buffer to local storage buffer.
    fn to_local_storage(&self, id: BufferId, filepath: PathBuf) -> Result<(), std::io::Error>;

    /// Convert the buffer to local storage buffer asynchronously.
    fn to_local_storage_async(
        &self,
        id: BufferId,
        filename: PathBuf,
    ) -> Result<oneshot::Receiver<Result<(), std::io::Error>>, std::io::Error>;

    /// Get the buffer from the storage, this function return a channel that will receive the handler to the buffer.
    fn get_async(&self, id: BufferId) -> Result<BufferAsyncResult, ()>;

    // /// Get the buffer snapshot from the storage, this function return a channel that will receive the handler to the buffer snapshot.
    // ///
    // /// Note: because that the task system only accept `'static` future, we must have 'g: 'static,
    // ///     it's useless to use snapshot.
    // fn get_async_snapshot<'g>(&self, id: BufferId, guard: &'g circ::Guard) -> Result<BufferAsyncSnapshotResult<'g>, ()>;
    /// Get size of the buffer.
    ///
    /// It's the runtime size of underlying data.
    fn buffer_size(&self, id: BufferId) -> Option<usize>;

    /// Get the number of cached buffers.
    fn cached_len(&self) -> usize;

    /// Whether the buffer is loaded into memory?
    fn is_loaded(&self, id: BufferId) -> bool;

    /// Whether this buffer have a backing storage?
    fn is_persistent(&self, id: BufferId) -> bool;

    /// Get the hash of the buffer.
    fn hash(&self, id: BufferId) -> Option<u64>;

    /// Load the buffer into buffer storage using task system, this function should be non-blocking.
    fn load(&self, id: BufferId);
}

impl<We, B, L> BasinBufferInterface for BufferStorage<We, B, L>
where
    We: Weighter<BufferId, Rc<Buffer>> + FixedTypeId + Clone + Send + Sync + 'static,
    B: BuildHasher + FixedTypeId + Clone + Send + Sync + 'static,
    L: Lifecycle<BufferId, Rc<Buffer>, RequestState = ()>
        + FixedTypeId
        + Clone
        + Send
        + Sync
        + 'static,
{
    fn add(&self, options: BufferOptions) -> BufferId {
        match options {
            BufferOptions::Memory { hash, data } => {
                tracing::debug!("Adding memory buffer");
                // 1. Calculate the hash if not provided.
                let hash = hash.unwrap_or_else(|| {
                    let mut rapidhasher = RapidInlineHasher::default();
                    data.hash(&mut rapidhasher);
                    rapidhasher.finish()
                });
                // 2. Add the data into the slab.
                let runtime_id = self
                    .data
                    .insert(BufferObject {
                        data: AtomicRc::new(Buffer(data)),
                        hash: AtomicU64::new(hash),
                        meta: AtomicRc::new(BufferMeta::Memory),
                    })
                    .expect("Failed to insert buffer into slab");
                tracing::info!("Memory buffer added with runtime ID: {:?}", runtime_id);
                // 3. Return the runtime id.
                BufferId::Runtime(runtime_id)
            }
            BufferOptions::LocalStorage {
                hash,
                filepath,
                offset,
                size,
            } => {
                tracing::debug!("Adding local storage buffer");
                // 1. Add the metadata to the disk cache.
                let runtime_id = self
                    .data
                    .insert(BufferObject {
                        data: AtomicRc::null(),
                        hash: AtomicU64::new(hash.unwrap_or_default()),
                        meta: AtomicRc::new(BufferMeta::LocalStorage {
                            filepath,
                            offset: offset.unwrap_or(0),
                            size,
                        }),
                    })
                    .expect("Failed to insert buffer into slab");
                tracing::info!(
                    "Local storage buffer added with runtime ID: {:?}",
                    runtime_id
                );
                // 2. Return the runtime id.
                BufferId::Runtime(runtime_id)
            }
            BufferOptions::Url {
                hash,
                url,
                offset,
                size,
            } => {
                tracing::warn!("URL buffer option is not supported yet");
                // Don't support for now.
                todo!()
            }
        }
    }

    fn meta<'g>(&self, id: BufferId, guard: &'g circ::Guard) -> Option<Snapshot<'g, BufferMeta>> {
        Some(
            self.data
                .get(id.runtime_key())?
                .meta
                .load(Ordering::Acquire, guard),
        )
    }

    fn remove(&self, id: BufferId) {
        tracing::info!("Removing buffer with ID: {:?}", id);
        // Note: the id has generation field, so it's safe to remove them step by step.
        //
        // only remove from slab, the disk cache will be removed when the corresponding disk-backed buffer evicted.
        self.data.remove(id.runtime_key());
        // the remove operation won't call on eviction callback.
        // let _ = self.disk_cache.remove(&id);
    }

    fn get(&self, id: BufferId) -> Option<Rc<Buffer>> {
        tracing::info!("Getting buffer with ID: {:?}", id);
        // 1. get the type of this buffer.
        let buffer_object = self.data.get(id.runtime_key())?;
        let meta = {
            let guard = circ::cs();
            let snapshot = buffer_object.meta(&guard);
            snapshot.as_ref().unwrap().clone()
        };
        match &meta {
            BufferMeta::Memory => {
                tracing::debug!("Buffer {:?} is a memory buffer", id);
                // 2. If it's memory buffer, it should be already loaded into memory.
                let guard = circ::cs();
                let snapshot = buffer_object.data.load(Ordering::Acquire, &guard);
                assert!(!snapshot.is_null());
                tracing::info!("Memory buffer {:?} retrieved successfully", id);
                Some(snapshot.counted())
            }
            BufferMeta::LocalStorage {
                filepath,
                offset,
                size,
            } => {
                tracing::debug!("Buffer {:?} is a local storage buffer", id);
                // 2. If it's local storage buffer
                // 2.1 We need to check if it's already loaded

                // First check disk cache
                // Note: It's a must operation, because the eviction rely on the cache get operation.
                // if let Some(buffer) = self.disk_cache.get(&id) {
                //     tracing::info!("Local storage buffer {:?} found in disk cache", id);
                //     return Some(buffer);
                // }
                // If we can't get the Buffer from cache, and block here, it means that the buffer is already being loading by other thread.
                // So we can't be faster than that thread.
                let cache_guard = match self.disk_cache.get_value_or_guard(&id, None) {
                    GuardResult::Value(buffer) => {
                        tracing::info!("Local storage buffer {:?} found in disk cache", id);
                        return Some(buffer);
                    }
                    GuardResult::Guard(guard) => guard,
                    _ => unreachable!(),
                };

                // // Then check if loaded in memory between the disk cache check and the memory check.
                // {
                //     let guard = circ::cs();
                //     let snapshot = buffer_object.data.load(Ordering::Acquire, &guard);
                //     if !snapshot.is_null() {
                //         tracing::info!("Local storage buffer {:?} already loaded in memory", id);
                //         return Some(snapshot.counted());
                //     }
                // }
                // 2.2 If not, we need to load it from local storage.
                tracing::info!("Loading buffer {:?} from local storage: {:?}", id, filepath);
                let mut file = std::fs::File::open(filepath).unwrap();

                // If size is not provided, and the offset is 0, we can just use the whole file to create aligned buffer.
                // TODO: the size of the file may be stored into meta if the size is unknown before.
                //       but currently it's readonly.
                let (buffer, size) = match (size, offset) {
                    (None, 0) => {
                        let size = file.metadata().unwrap().len() as usize;
                        let mut file_buffer = AlignedVec::with_capacity(size);
                        file_buffer.resize(size, 0);
                        file.read_exact(&mut file_buffer).unwrap();
                        (file_buffer, size)
                    }
                    _ => {
                        let file_size = file.metadata().unwrap().len() as usize;
                        let mut file_buffer = Vec::with_capacity(file_size);
                        file_buffer.resize(file_size, 0);
                        // Note: standard library don't provide read_exact_at, so we can't use offset here.
                        file.read_exact(&mut file_buffer).unwrap();
                        // Note: Try to spilit this file_buffer into multiple buffers, respect to the offset and size.
                        let start = *offset;
                        let len = size.unwrap_or(file_size - start);
                        let mut buffer = AlignedVec::with_capacity(len);
                        buffer.extend_from_slice(&file_buffer[start..start + len]);
                        (buffer, len)
                    }
                };
                tracing::info!(
                    "Buffer {:?} loaded with size: {},{}",
                    id,
                    buffer.len(),
                    size
                );

                // 2.3 Get the hash of the buffer.
                let hash = {
                    let mut rapidhasher = RapidInlineHasher::default();
                    buffer.hash(&mut rapidhasher);
                    rapidhasher.finish()
                };
                // 2.4 Set the hash into the atomic.
                buffer_object.hash.store(hash, Ordering::Release);

                let buffer = Rc::new(Buffer(buffer));
                let guard = circ::cs();
                // 2.5 If the buffer is already loaded via other thread, no need to set it again.
                match buffer_object.data.compare_exchange(
                    Snapshot::null(),
                    buffer.clone(),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                    &guard,
                ) {
                    Ok(_) => {
                        tracing::info!("Local storage buffer {:?} loaded by current thread", id);
                        // 2.6 Store into disk cache.
                        cache_guard.insert(buffer.clone()).unwrap();
                        tracing::info!("Local storage buffer {:?} loaded and cached", id);
                        // 2.7 Return the buffer.
                        Some(buffer)
                    }
                    Err(e) => {
                        tracing::warn!("Local storage buffer {:?} already loaded by other thread, but it shouldn't happended", id);
                        drop(e);
                        drop(buffer);
                        // 2.8 If the buffer is already loaded by other thread, no need to set it again.
                        let snapshot = buffer_object.data.load(Ordering::Acquire, &guard);
                        Some(snapshot.counted())
                    }
                }
            }
            BufferMeta::Url { url, offset, size } => {
                tracing::warn!("URL buffer option is not supported yet");
                todo!()
            }
        }
    }

    fn get_snapshot<'g>(
        &self,
        id: BufferId,
        guard: &'g circ::Guard,
    ) -> Option<Snapshot<'g, Buffer>> {
        tracing::info!("Getting snapshot for buffer with ID: {:?}", id);
        // 1. get the type of this buffer.
        let buffer_object = self.data.get(id.runtime_key())?;
        let meta = {
            let cs = circ::cs();
            let snapshot = buffer_object.meta(&cs);
            snapshot.as_ref().unwrap().clone()
        };
        match &meta {
            BufferMeta::Memory => {
                tracing::debug!("Buffer {:?} is a memory buffer", id);
                // 2. If it's memory buffer, it should be already loaded into memory.
                let snapshot = buffer_object.data.load(Ordering::Acquire, guard);
                assert!(!snapshot.is_null());
                tracing::info!(
                    "Memory buffer {:?} snapshot retrieved successfully, size: {}",
                    id,
                    snapshot.as_ref().unwrap().len()
                );
                Some(snapshot)
            }
            BufferMeta::LocalStorage {
                filepath,
                offset,
                size,
            } => {
                tracing::debug!("Buffer {:?} is a local storage buffer", id);
                // 2. If it's local storage buffer
                // 2.1 First check disk cache
                // if let Some(buffer) = self.disk_cache.get(&id) {
                //     tracing::info!("Local storage buffer {:?} found in disk cache", id);
                //     return Some(buffer.snapshot(guard));
                // }
                let cache_guard = match self.disk_cache.get_value_or_guard(&id, None) {
                    GuardResult::Value(buffer) => {
                        return Some(buffer.snapshot(guard));
                    }
                    GuardResult::Guard(guard) => guard,
                    _ => unreachable!(),
                };
                // 2.2 Then check if it's already loaded
                // let snapshot = buffer_object.data.load(Ordering::Acquire, guard);
                // if !snapshot.is_null() {
                //     tracing::info!(
                //         "Local storage buffer {:?} already loaded, size: {}",
                //         id,
                //         snapshot.as_ref().unwrap().len()
                //     );
                //     return Some(snapshot);
                // }
                // 2.2 If not, we need to load it from local storage.
                tracing::info!("Loading buffer {:?} from local storage: {:?}", id, filepath);
                let mut file = std::fs::File::open(filepath).unwrap();

                let (buffer, size) = match (size, offset) {
                    (None, 0) => {
                        let size = file.metadata().unwrap().len() as usize;
                        let mut file_buffer = AlignedVec::with_capacity(size);
                        file_buffer.resize(size, 0);
                        file.read_exact(&mut file_buffer).unwrap();
                        (file_buffer, size)
                    }
                    _ => {
                        let file_size = file.metadata().unwrap().len() as usize;
                        let mut file_buffer = Vec::with_capacity(file_size);
                        file_buffer.resize(file_size, 0);
                        file.read_exact(&mut file_buffer).unwrap();
                        let start = *offset;
                        let len = size.unwrap_or(file_size - start);
                        let mut buffer = AlignedVec::with_capacity(len);
                        buffer.extend_from_slice(&file_buffer[start..start + len]);
                        (buffer, len)
                    }
                };
                tracing::info!(
                    "Buffer {:?} loaded from local storage with size: ({}, {})",
                    id,
                    buffer.len(),
                    size
                );

                let buffer = Rc::new(Buffer(buffer));
                // 2.3 Atomically set the buffer into the slab.
                // Note: we don't need to check if the buffer is already loaded by other thread,
                match buffer_object.data.compare_exchange(
                    Snapshot::null(),
                    buffer.clone(),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                    guard,
                ) {
                    Ok(_) => {
                        tracing::info!("Local storage buffer {:?} loaded by current thread", id);
                        // 2.4 Store into disk cache.
                        cache_guard.insert(buffer.clone()).unwrap();
                        tracing::info!("Local storage buffer {:?} loaded and cached", id);
                        // 2.5 Return the buffer.
                        Some(buffer.snapshot(guard))
                    }
                    Err(e) => {
                        tracing::warn!("Local storage buffer {:?} already loaded by other thread, but it shouldn't happended", id);
                        drop(e);
                        drop(buffer);
                        let snapshot = buffer_object.data.load(Ordering::Acquire, guard);
                        Some(snapshot)
                    }
                }
            }
            BufferMeta::Url { url, offset, size } => {
                tracing::warn!("URL buffer option is not supported yet");
                todo!()
            }
        }
    }

    fn get_async(&self, id: BufferId) -> Result<BufferAsyncResult, ()> {
        tracing::info!("Getting buffer asynchronously with ID: {:?}", id);
        let buffer_object = self.data.get(id.runtime_key()).ok_or(())?;
        let meta = {
            let guard = circ::cs();
            let snapshot = buffer_object.meta(&guard);
            snapshot.as_ref().unwrap().clone()
        };
        match meta {
            BufferMeta::Memory => {
                tracing::debug!("Buffer {:?} is a memory buffer", id);
                let guard = circ::cs();
                let buffer = buffer_object.data.load(Ordering::Acquire, &guard);
                tracing::info!("Memory buffer {:?} retrieved successfully", id);
                Ok(BufferAsyncResult::Loaded(buffer.counted()))
            }
            BufferMeta::LocalStorage {
                filepath,
                offset,
                size,
            } => {
                tracing::debug!("Buffer {:?} is a local storage buffer", id);
                // 1. If the buffer is already loaded return it.
                if let Some(buffer) = self.disk_cache.get(&id) {
                    tracing::info!("Local storage buffer {:?} already loaded", id);
                    return Ok(BufferAsyncResult::Loaded(buffer));
                }

                // {
                //     let guard = circ::cs();
                //     let snapshot = buffer_object.data.load(Ordering::Acquire, &guard);
                //     if !snapshot.is_null() {
                //         tracing::info!("Local storage buffer {:?} already loaded", id);
                //         return Ok(BufferAsyncResult::Loaded(snapshot.counted()));
                //     }
                // }

                // 2. If not, we need to load it from local storage.
                tracing::info!(
                    "Loading buffer {:?} from local storage asynchronously: {:?}",
                    id,
                    filepath
                );
                let (sender, receiver) = oneshot::channel();

                let guard = circ::cs();
                let task_system_api = self.task_system.get(&guard).ok_or(())?;
                let filepath = filepath.clone();
                let data_slab = Arc::clone(&self.data);
                let disk_cache = self.disk_cache.clone();
                task_system_api.spawn_detached(
                    async move {
                        let cache_guard = match disk_cache.get_value_or_guard_async(&id).await {
                            Ok(buffer) => {
                                sender.send(Some(buffer)).unwrap();
                                return;
                            }
                            Err(guard) => {
                                guard
                            }
                        };
                        let file = bubble_tasks::fs::File::open(filepath).await.unwrap();
                        let size = size.unwrap_or(file.metadata().await.unwrap().len() as usize);
                        let file_buffer = Buffer(AlignedVec::with_capacity(size));
                        let (_, buffer) = file
                            .read_exact_at(file_buffer, offset as u64)
                            .await
                            .unwrap();
                        let buffer = Rc::new(buffer);
                        tracing::info!("Buffer {:?} loaded from local storage: {}", id, size);

                        if let Some(buffer_object) = data_slab.get(id.runtime_key()) {
                            let guard = circ::cs();
                            match buffer_object.data.compare_exchange(
                                Snapshot::null(),
                                buffer.clone(),
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                                &guard,
                            ) {
                                Ok(_) => {
                                    sender.send(Some(buffer.clone())).unwrap();
                                    // Store into disk cache.
                                    cache_guard.insert(buffer.clone()).unwrap();
                                }
                                Err(e) => {
                                    tracing::warn!("Local storage buffer {:?} already loaded by other thread, but it shouldn't happended", id);
                                    // already loaded
                                    drop(e);
                                    drop(buffer);
                                    let snapshot =
                                        buffer_object.data.load(Ordering::Acquire, &guard);
                                    sender.send(Some(snapshot.counted())).unwrap();
                                }
                            }
                        } else {
                            sender.send(None).unwrap();
                        }
                    }
                    .into_local_ffi(),
                );
                return Ok(BufferAsyncResult::Loading(receiver));
            }
            BufferMeta::Url { url, offset, size } => {
                tracing::warn!("URL buffer option is not supported yet");
                todo!()
            }
        }
    }

    fn to_local_storage_async(
        &self,
        id: BufferId,
        filepath: PathBuf,
    ) -> Result<oneshot::Receiver<Result<(), std::io::Error>>, std::io::Error> {
        // Request save -1-> save done -2-> atomic update meta -3-> atomic swap buffer to null -4-> end.
        // 1. get meta, and object data
        // let buffer_object = self.data.get(id.runtime_key()).unwrap();
        let meta = {
            let guard = circ::cs();
            let snapshot = self.data.get(id.runtime_key()).unwrap().meta(&guard);
            snapshot.as_ref().unwrap().clone()
        };
        match meta {
            BufferMeta::Memory => {
                tracing::info!(
                    "Persisting memory buffer {:?} to local storage: {:?}",
                    id,
                    filepath
                );
                // 2. create a temp file, get a temp path
                let temp_file_path = tempfile::NamedTempFile::new()?.into_temp_path();
                // 3. write the buffer to the file
                let guard = circ::cs();
                let buffer = self.get(id).unwrap();
                let local_task_system_api =
                    self.task_system.get(&guard).expect("Task system not found");
                let data_slab = Arc::clone(&self.data);
                let disk_cache = self.disk_cache.clone();
                let (sender, receiver) = oneshot::channel();
                let taks_system_api = self.task_system.clone();
                local_task_system_api.spawn_detached(
                    async move {
                        let buffer = buffer;
                        let temp_file_path = temp_file_path;
                        // 5. make the persistent process async atomic, it's save because this change can only happen once by 1 `to_local_storage_async` call
                        let Err(cache_guard) = disk_cache.get_value_or_guard_async(&id).await
                        else {
                            sender.send(Ok(())).unwrap();
                            return;
                        };
                        // 4. write the buffer to the file
                        bubble_tasks::fs::write(&temp_file_path, RcBuffer(buffer.clone()))
                            .await
                            .unwrap();
                        let (persist_tx, persist_rx) = oneshot::channel();
                        let guard = circ::cs();
                        let local_task_system_api =
                            taks_system_api.get(&guard).expect("Task system not found");
                        let filepath_clone = filepath.clone();
                        let _ = local_task_system_api.dispatch_blocking(
                            None,
                            Box::new(move || {
                                let result = temp_file_path.persist(&filepath_clone);
                                persist_tx.send(result).unwrap();
                            }),
                        );

                        // 6. persist the file to the target path, in fact the persist operation is a blocking operation
                        //    TODO: use bubble_tasks::fs to replace this, or dispatch this function to thread pool use a sender
                        //    to avoid blocking the async runtime.
                        //
                        //    It seems that `dispatch` has better performance than `dispatch_blocking`? why?
                        match persist_rx.await.unwrap() {
                            Ok(_) => {
                                // 6. successfully atomic replace the file,
                                //    update the meta
                                tracing::info!(
                                    "Successfully atomic replace the file: {:?}",
                                    filepath
                                );
                                let guard = circ::cs();
                                let buffer_object = data_slab.get(id.runtime_key()).unwrap();
                                let mut new_meta = Rc::new(BufferMeta::LocalStorage {
                                    filepath,
                                    offset: 0,
                                    size: Some(buffer.as_ref().unwrap().len()),
                                });
                                let mut meta_snapshot =
                                    buffer_object.meta.load(Ordering::Acquire, &guard);
                                while let Err(e) = buffer_object.meta.compare_exchange(
                                    meta_snapshot,
                                    new_meta,
                                    Ordering::SeqCst,
                                    Ordering::SeqCst,
                                    &guard,
                                ) {
                                    new_meta = e.desired;
                                    meta_snapshot =
                                        buffer_object.meta.load(Ordering::Acquire, &guard);
                                }
                                tracing::info!("Successfully updated meta for buffer {:?}", id);
                                // 7. swap the buffer to null
                                // buffer_object
                                //     .data
                                //     .store(Rc::null(), Ordering::Release, &guard);
                                // 8. add to disk cache
                                cache_guard.insert(buffer.clone()).unwrap();
                                tracing::info!("Successfully added buffer {:?} to disk cache", id);
                                // 9. send complete signal
                                sender.send(Ok(())).unwrap();
                            }
                            e => {
                                tracing::error!("Failed to persist file: {:?}", filepath);
                                sender
                                    .send(e.map_err(|e| {
                                        std::io::Error::new(
                                            std::io::ErrorKind::Other,
                                            format!("Failed to persist file: {:?}", e),
                                        )
                                    }))
                                    .unwrap();
                            }
                        }
                    }
                    .into_local_ffi(),
                );

                return Ok(receiver);
            }
            BufferMeta::LocalStorage { .. } => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Already in local storage",
                ));
            }
            BufferMeta::Url { .. } => {
                tracing::warn!("URL buffer is not supported yet");
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "URL buffer is not supported yet",
                ));
            }
        }
    }

    fn to_local_storage(&self, id: BufferId, filepath: PathBuf) -> Result<(), std::io::Error> {
        // Request save -1-> save done -2-> atomic update meta -3-> atomic swap buffer to null -4-> end.
        // 1. get meta, and object data
        let meta = {
            let guard = circ::cs();
            let snapshot = self.data.get(id.runtime_key()).unwrap().meta(&guard);
            snapshot.as_ref().unwrap().clone()
        };
        match meta {
            BufferMeta::Memory => {
                tracing::info!(
                    "Persisting memory buffer {:?} to local storage: {:?}",
                    id,
                    filepath
                );

                // 5. make the persistent process atomic
                let cache_guard = match self.disk_cache.get_value_or_guard(&id, None) {
                    GuardResult::Value(_) => {
                        return Ok(());
                    }
                    GuardResult::Guard(guard) => guard,
                    _ => unreachable!(),
                };
                // 2. create a temp file, get a temp path
                let temp_file_path = tempfile::NamedTempFile::new()?.into_temp_path();
                // 3. write the buffer to the file
                let buffer = self.get(id).unwrap();

                // 4. write the buffer to the file synchronously
                std::fs::write(&temp_file_path, &buffer.as_ref().unwrap().0)?;

                // 6. persist the file to the target path
                match temp_file_path.persist(&filepath) {
                    Ok(_) => {
                        // 6. successfully atomic replace the file,
                        //    update the meta
                        tracing::info!("Successfully atomic replace the file: {:?}", filepath);
                        let guard = circ::cs();
                        let buffer_object = self.data.get(id.runtime_key()).unwrap();
                        let mut new_meta = Rc::new(BufferMeta::LocalStorage {
                            filepath: filepath.clone(),
                            offset: 0,
                            size: Some(buffer.as_ref().unwrap().len()),
                        });
                        let mut meta_snapshot = buffer_object.meta.load(Ordering::Acquire, &guard);
                        while let Err(e) = buffer_object.meta.compare_exchange(
                            meta_snapshot,
                            new_meta,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                            &guard,
                        ) {
                            new_meta = e.desired;
                            meta_snapshot = buffer_object.meta.load(Ordering::Acquire, &guard);
                        }
                        tracing::info!("Successfully updated meta for buffer {:?}", id);

                        // 7. add to disk cache
                        cache_guard.insert(buffer.clone()).unwrap();
                        tracing::info!("Successfully added buffer {:?} to disk cache", id);

                        Ok(())
                    }
                    Err(e) => {
                        tracing::error!("Failed to persist file: {:?}", filepath);
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to persist file: {:?}", e),
                        ))
                    }
                }
            }
            BufferMeta::LocalStorage { .. } => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Already in local storage",
            )),
            BufferMeta::Url { .. } => {
                tracing::warn!("URL buffer is not supported yet");
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "URL buffer is not supported yet",
                ))
            }
        }
    }

    fn buffer_size(&self, id: BufferId) -> Option<usize> {
        tracing::info!("Getting buffer size for ID: {:?}", id);
        // 1. Get the buffer object from the data slab using the runtime key.
        let buffer_object = self.data.get(id.runtime_key())?;

        // 2. Get the size from the underlying buffer's len
        let guard = circ::cs();
        let buffer = buffer_object
            .data
            .load(Ordering::Acquire, &guard)
            .as_ref()
            .expect("Buffer is null");
        let size = buffer.len();
        tracing::info!("Buffer size: {}", size);
        Some(size)
    }

    fn cached_len(&self) -> usize {
        let len = self.disk_cache.len();
        tracing::info!("Number of cached buffers: {}", len);
        len
    }

    fn hash(&self, id: BufferId) -> Option<u64> {
        tracing::info!("Getting hash for buffer ID: {:?}", id);
        // Get the buffer object from the data slab
        let buffer_object = self.data.get(id.runtime_key())?;
        // Return the hash stored in the atomic
        let hash = buffer_object.hash.load(Ordering::Acquire);
        tracing::info!("Buffer hash: {}", hash);
        Some(hash)
    }

    fn load(&self, id: BufferId) {
        tracing::info!("Loading buffer with ID: {:?}", id);
        // Get the buffer object
        if let Some(buffer_object) = self.data.get(id.runtime_key()) {
            // Only proceed if buffer isn't already loaded
            let guard = circ::cs();
            if buffer_object.data.load(Ordering::Acquire, &guard).is_null() {
                let meta = {
                    let guard = circ::cs();
                    let snapshot = buffer_object.meta(&guard);
                    snapshot.as_ref().unwrap().clone()
                };
                match meta {
                    BufferMeta::Memory => {
                        tracing::debug!("Buffer is a memory buffer, already loaded");
                    }
                    BufferMeta::LocalStorage {
                        filepath,
                        offset,
                        size,
                    } => {
                        tracing::info!(
                            "Loading buffer from local storage asynchronously: {:?}",
                            filepath
                        );
                        // Spawn a task to load the file asynchronously
                        let filepath = filepath.clone();
                        let guard = circ::cs();
                        let task_system =
                            self.task_system.get(&guard).expect("Task system not found");
                        let runtime_id = id.runtime_key();
                        let data_slab = Arc::clone(&self.data);
                        let disk_cache = self.disk_cache.clone();

                        task_system.spawn_detached(
                            async move {
                                let buffer = {
                                    // Load file in task
                                    let file =
                                        bubble_tasks::fs::File::open(filepath).await.unwrap();
                                    let file_buffer =
                                        Buffer(AlignedVec::with_capacity(
                                            size.unwrap_or(
                                                file.metadata().await.unwrap().len() as usize
                                            ) as usize,
                                        ));
                                    let (_, buffer) = file
                                        .read_exact_at(file_buffer, offset as u64)
                                        .await
                                        .unwrap();
                                    Rc::new(buffer)
                                };

                                // Update the buffer object atomically if it still exists
                                if let Some(buffer_object) = data_slab.get(runtime_id) {
                                    let guard = circ::cs();
                                    let mut snapshot =
                                        buffer_object.data.load(Ordering::Acquire, &guard);
                                    while snapshot.is_null() {
                                        if buffer_object
                                            .data
                                            .compare_exchange(
                                                snapshot,
                                                buffer.clone(),
                                                Ordering::SeqCst,
                                                Ordering::SeqCst,
                                                &guard,
                                            )
                                            .is_ok()
                                        {
                                            // Store in disk cache
                                            disk_cache
                                                .insert(BufferId::Runtime(runtime_id), buffer);
                                            tracing::info!(
                                                "Buffer loaded and cached asynchronously"
                                            );
                                            break;
                                        }
                                        snapshot =
                                            buffer_object.data.load(Ordering::Acquire, &guard);
                                    }
                                }
                            }
                            .into_local_ffi(),
                        );
                    }
                    BufferMeta::Url { .. } => {
                        tracing::warn!("URL buffer option is not supported yet");
                        todo!()
                    }
                }
            }
        }
    }

    fn is_loaded(&self, id: BufferId) -> bool {
        tracing::info!("Checking if buffer is loaded for ID: {:?}", id);
        // Check if the buffer data is present in memory
        let loaded = self
            .data
            .get(id.runtime_key())
            .map(|buffer_object| {
                let guard = circ::cs();
                !buffer_object.data.load(Ordering::Acquire, &guard).is_null()
            })
            .unwrap_or(false);
        tracing::info!("Buffer loaded: {}", loaded);
        loaded
    }

    fn is_persistent(&self, id: BufferId) -> bool {
        tracing::info!("Checking if buffer is persistent for ID: {:?}", id);
        // Check if the buffer has backing storage (local storage or URL)
        let persistent = self
            .data
            .get(id.runtime_key())
            .map(|buffer_object| {
                let guard = circ::cs();
                let meta = buffer_object.meta(&guard);
                matches!(
                    meta.as_ref().unwrap(),
                    BufferMeta::LocalStorage { .. } | BufferMeta::Url { .. }
                )
            })
            .unwrap_or(false);
        tracing::info!("Buffer persistent: {}", persistent);
        persistent
    }
}

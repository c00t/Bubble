use std::sync::atomic::AtomicBool;

use crate::futures_channel::oneshot;
use bubble_core::api::prelude::*;

/// The hint for the task system to run the task on the specific thread pool which bind to some specific cores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffinityHint {
    /// The task should be run on the performance core
    Performance,
    /// The task should be run on the efficiency core
    Efficiency,
}

#[derive(Debug, Clone)]
pub struct CancellableTicket(std::sync::Arc<(AtomicBool, AtomicBool)>);

impl CancellableTicket {
    pub fn new() -> Self {
        Self(std::sync::Arc::new((
            AtomicBool::new(false),
            AtomicBool::new(false),
        )))
    }

    pub fn set_allow_cancel(&self, allow: bool) {
        self.0 .0.store(allow, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn allow_cancel(&self) -> bool {
        self.0 .0.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.0 .1.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_canceled(&self) -> bool {
        self.0 .1.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// A unique id for a task.
///
/// Currently it's only generated for cancelable task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CalcellationId(pub(crate) u64);

/// Task system api
///
/// Run async task on the task system. Because the task system will be used by ui,
/// so the api provide a cancelable friendly spawn function. The closure passed to the function
/// take a `TaskId` as parameter, which can be used to cancel the task cooperatively, you will need to
/// explictly allow cancellation in your async function if you want systems outside your code can cancel it.
#[declare_api((0,1,0), bubble_tasks::types::TaskSystemApi)]
pub trait TaskSystemApi: Api {
    /// Spawn a task on the current thread, and return a handle to it.
    ///
    /// The task will be executed on the current thread, and the handle will be returned immediately.
    /// `bubble_tasks` is a per-core runtime library, so the task will be executed on the current core.
    ///
    /// ## TODO
    ///
    /// Add opaque type to the return type of the future, which may be a struct or an enum.
    ///
    /// ## NOTE
    ///
    /// You can drop the future returned by this function, the task will be cancelled.
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> async_ffi::LocalFfiFuture<()>;

    fn spawn_cancelable(
        &self,
        make_fut: Box<dyn FnOnce(CalcellationId) -> async_ffi::LocalFfiFuture<()> + Send + 'static>,
    ) -> (CalcellationId, async_ffi::LocalFfiFuture<()>);

    /// Spawn a blocking task, and return a future to wait for the task to finish.
    ///
    /// ## NOTE
    ///
    /// If you drop the future returned by this function, the task will be cancelled.
    /// But typically the func you passed to this function is a cpu-bound task, so the task will be executed to the end once it started.
    fn spawn_blocking(
        &self,
        func: Box<dyn FnOnce() -> () + Send + Sync + 'static>,
    ) -> async_ffi::FfiFuture<()>;

    /// Spawn a task in the background, you don't need to wait for the task to finish,
    /// it will run in the background.
    fn spawn_detached(&self, fut: async_ffi::LocalFfiFuture<()>);

    fn spawn_detached_cancelable(
        &self,
        make_fut: Box<dyn FnOnce(CalcellationId) -> async_ffi::LocalFfiFuture<()> + Send + 'static>,
    ) -> CalcellationId;

    /// Dispatch a task to the task system, and return a receiver to it.
    ///
    /// ## Note
    ///
    /// whether the task is finished or not, just ignore the [`oneshot::Receiver`]. It doesn't matter whether the [`oneshot::Receiver`] is dropped or not,
    /// the task will be executed to the end anyway.
    ///
    /// The task system should just ignore the error returned from [`oneshot::Sender::send`].
    ///
    /// ## Affinity hint
    ///
    /// If the underlying implementation supports affinity hint, the task system will
    /// try to run the task on the thread pool which bind to cores with the same affinity hint.
    fn dispatch(
        &self,
        affinity_hint: Option<AffinityHint>,
        fut_func: Box<dyn FnOnce() -> async_ffi::LocalFfiFuture<()> + Send + 'static>,
    ) -> Result<oneshot::Receiver<()>, ()>;

    fn dispatch_cancelable(
        &self,
        affinity_hint: Option<AffinityHint>,
        make_fut: Box<dyn FnOnce(CalcellationId) -> async_ffi::FfiFuture<()> + Send + 'static>,
    ) -> Result<(CalcellationId, oneshot::Receiver<()>), ()>;

    fn dispatch_blocking(
        &self,
        affinity_hint: Option<AffinityHint>,
        func: Box<dyn FnOnce() -> () + Send + 'static>,
    ) -> Result<oneshot::Receiver<()>, ()>;

    /// Explicitly set a shared internal state to allow the task to be cancelled.
    fn allow_cancel(&self, task_id: CalcellationId) -> bool;

    fn cancel(&self, task_id: CalcellationId);

    /// Shutdown the task system.
    ///
    /// Use a temp unsafe implementation to check atomic counter
    fn shutdown(&self);
    /// Run the tick task as the current future
    ///
    /// ## Note
    ///
    /// You should not do to much io task in tick task, it should handled by dispatcher,
    /// and then use a channel to notify the tick task.
    ///
    /// 2 or more tick task may be executed parallelly on different threads, so you should be careful about the data race.
    unsafe fn tick(&self, tick_task: async_ffi::LocalFfiFuture<bool>) -> bool;
    /// Get the number of worker threads in the task system dispatcher.
    fn num_threads(&self) -> usize;
}

#![feature(ptr_metadata)]
#![feature(once_cell_get_mut)]
use std::num::NonZeroUsize;
use std::sync::OnceLock;

use async_ffi::FutureExt;
use bubble_core::{
    api::prelude::*,
    sync::{AsPtr, AtomicArc},
    thread_local,
};
use bubble_tasks::dispatcher::Dispatcher;

pub trait TraitCastableDropSuper {
    fn super_func(&self);
}

pub trait TraitCastableDropSub {
    fn sub_func(&self);
}

#[make_trait_castable(TraitCastableDropSuper, TraitCastableDropSub)]
pub struct TraitCastableDrop {
    pub value: i32,
}

unique_id! {
    #[UniqueTypeIdVersion((0,1,0))]
    dyn TraitCastableDropSuper;
    dyn TraitCastableDropSub;
}

impl TraitCastableDropSuper for TraitCastableDrop {
    fn super_func(&self) {
        println!("super_func: {}", self.value);
    }
}

impl TraitCastableDropSub for TraitCastableDrop {
    fn sub_func(&self) {
        println!("sub_func: {}", self.value);
    }
}

impl Drop for TraitCastableDrop {
    fn drop(&mut self) {
        println!("TraitCastableDrop: {}", self.value);
    }
}

crate::thread_local! {
    pub static TEST_VAR: std::sync::Mutex<i32> =  std::sync::Mutex::new(0);
}

pub type StringAlias0 = std::string::String;
pub type StringAlias1 = String;

#[define_api((0,1,0), shared::TaskSystemApi)]
struct TaskSystem {
    pub dispatcher: AtomicArc<std::sync::RwLock<Option<Dispatcher>>>,
}

impl TaskSystem {
    pub fn new() -> ApiHandle<dyn TaskSystemApi> {
        let dispatcher = Dispatcher::builder()
            .worker_threads(NonZeroUsize::new(8).unwrap())
            .thread_names(|index| format!("compio-worker-{index}"))
            .build()
            .unwrap();
        let api_registry_api: AnyApiHandle = Box::new(TaskSystem {
            dispatcher: AtomicArc::new(std::sync::RwLock::new(Some(dispatcher))),
        })
        .into();
        api_registry_api.downcast()
    }
}

pub trait TaskSystemApi: Api {
    /// Spawn a task on the current thread, and return a handle to it.
    ///
    /// The task will be executed on the current thread, and the handle will be returned immediately.
    /// `bubble_tasks` is a per-core runtime library, so the task will be executed on the current core.
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> bubble_tasks::runtime::JoinHandle<()>;
    /// Dispatch a task to the task system, and return a future to it.
    ///
    ///
    fn dispatch(
        &self,
        fut: async_ffi::FfiFuture<()>,
    ) -> async_ffi::FfiFuture<Result<(), futures_channel::oneshot::Canceled>>;
    fn dispatch_blocking(
        &self,
        func: Box<dyn FnOnce() -> () + Send + 'static>,
    ) -> async_ffi::FfiFuture<Result<(), futures_channel::oneshot::Canceled>>;
    /// Shutdown the task system.
    ///
    /// Use a temp unsafe implementation to check atomic counter
    fn shutdown(&self);
}

impl TaskSystemApi for TaskSystem {
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> bubble_tasks::runtime::JoinHandle<()> {
        bubble_tasks::runtime::spawn(fut)
    }

    fn dispatch(
        &self,
        fut: async_ffi::FfiFuture<()>,
    ) -> async_ffi::FfiFuture<Result<(), futures_channel::oneshot::Canceled>> {
        let result = self
            .dispatcher
            .load()
            .unwrap()
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .dispatch(|| fut)
            .unwrap();
        result.into_ffi()
    }

    fn dispatch_blocking(
        &self,
        func: Box<dyn FnOnce() -> () + Send + 'static>,
    ) -> async_ffi::FfiFuture<Result<(), futures_channel::oneshot::Canceled>> {
        let result = self
            .dispatcher
            .load()
            .unwrap()
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .dispatch_blocking(func)
            .unwrap();
        result.into_ffi()
    }

    fn shutdown(&self) {
        let mut dispatcher = self.dispatcher.load();
        let null_arc = bubble_core::sync::Arc::new(std::sync::RwLock::new(None));

        loop {
            let dispatcher_ptr = dispatcher.as_ref().map_or(std::ptr::null(), AsPtr::as_ptr);
            match self
                .dispatcher
                .compare_exchange(dispatcher_ptr, Some(&null_arc))
            {
                Ok(()) => break,
                Err(before) => dispatcher = before,
            }
        }

        if let Some(dispatcher) = dispatcher {
            println!("in shutdown");
            let x = dispatcher.write().unwrap().take().unwrap();
            let runtime = bubble_tasks::runtime::Runtime::new().unwrap();
            let _ = runtime.block_on(async move { x.join().await });
        }
    }
}

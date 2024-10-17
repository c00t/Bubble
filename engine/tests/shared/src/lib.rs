#![feature(ptr_metadata)]
use async_ffi::FutureExt;
use bubble_core::api::{unique_id, UniqueId, UniqueTypeId};
use bubble_core::{
    api::{Api, Version},
    thread_local,
};

use bubble_core::api::{
    make_trait_castable, make_trait_castable_decl, TraitcastableAny, TraitcastableAnyInfra,
    TraitcastableAnyInfraExt,
};

crate::thread_local! {
    pub static TEST_VAR: std::sync::Mutex<i32> =  std::sync::Mutex::new(0);
}

pub type StringAlias0 = std::string::String;
pub type StringAlias1 = String;

#[repr(C)]
#[make_trait_castable(Api, TaskSystemApi)]
pub struct TaskSystem {
    pub dispatcher: &'static bubble_tasks::dispatcher::Dispatcher,
}

unique_id! {
    #[UniqueTypeIdVersion((0,0,1))]
    dyn TaskSystemApi
}

pub mod task_system_api {
    use bubble_core::api::Version;

    pub const NAME: &'static str = "TaskSystemApi";

    pub const VERSION: Version = Version::new(0, 0, 1);
}

pub trait TaskSystemApi {
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> bubble_tasks::runtime::JoinHandle<()>;
    fn dispatch(&self, fut: async_ffi::FfiFuture<()>) -> async_ffi::FfiFuture<()>;
    fn dispatch_blocking(
        &self,
        func: Box<dyn FnOnce() -> i32 + Send + 'static>,
    ) -> async_ffi::FfiFuture<i32>;
    fn shutdown(&self);
}

impl TaskSystemApi for TaskSystem {
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> bubble_tasks::runtime::JoinHandle<()> {
        bubble_tasks::runtime::spawn(fut)
    }

    fn dispatch(&self, fut: async_ffi::FfiFuture<()>) -> async_ffi::FfiFuture<()> {
        async {
            let result = self.dispatcher.dispatch(|| fut);
            result.unwrap().await.unwrap()
        }
        .into_ffi()
    }

    fn dispatch_blocking(
        &self,
        func: Box<dyn FnOnce() -> i32 + Send + 'static>,
    ) -> async_ffi::FfiFuture<i32> {
        async {
            let result = self.dispatcher.dispatch_blocking(func);
            result.unwrap().await.unwrap()
        }
        .into_ffi()
    }

    fn shutdown(&self) {
        todo!()
    }
}

impl Api for TaskSystem {
    fn name(&self) -> &'static str {
        task_system_api::NAME
    }

    fn version(&self) -> Version {
        task_system_api::VERSION
    }
}

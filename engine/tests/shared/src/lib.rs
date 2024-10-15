use async_ffi::FutureExt;
use bubble_core::{
    api::{Api, Version},
    thread_local,
};

crate::thread_local! {
    pub static TEST_VAR: std::sync::Mutex<i32> =  std::sync::Mutex::new(0);
}

pub type StringAlias0 = std::string::String;
pub type StringAlias1 = String;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskSystem {
    pub dispatcher: &'static bubble_tasks::dispatcher::Dispatcher,
}

pub trait TaskSystemApi: Api {
    fn spawn(&self, fut: async_ffi::FfiFuture<()>) -> bubble_tasks::runtime::JoinHandle<()>;
}

impl Api for TaskSystem {
    const NAME: &'static str = "TaskSystemApi";

    const VERSION: Version = Version::new(0, 0, 1);
}

impl TaskSystemApi for TaskSystem {
    fn spawn(&self, fut: async_ffi::FfiFuture<()>) -> bubble_tasks::runtime::JoinHandle<()> {
        bubble_tasks::runtime::spawn(fut)
    }
}

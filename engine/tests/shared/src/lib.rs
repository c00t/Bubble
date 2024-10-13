use bubble_core::thread_local;

crate::thread_local! {
    pub static TEST_VAR: std::sync::Mutex<i32> =  std::sync::Mutex::new(0);
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskSystemApi {
    pub dispatcher: &'static bubble_tasks::dispatcher::Dispatcher,
    pub spawn: fn(async_ffi::FfiFuture<()>) -> bubble_tasks::runtime::JoinHandle<()>,
    pub dispatch: fn(
        fn() -> async_ffi::LocalBorrowingFfiFuture<'static, String>,
    ) -> async_ffi::FfiFuture<String>,
}

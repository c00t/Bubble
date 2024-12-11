//! Customed async runtime for dynamic shared library
#![feature(ptr_metadata)]
#![feature(downcast_unchecked)]
#![feature(min_specialization)]

pub use compio::*;
pub mod task_system_api;
pub mod types;
pub use async_ffi;
pub use backon;
pub use futures_channel;
pub use futures_util;

/// A special future
pub struct LocalFuture<T>(async_ffi::LocalFfiFuture<T>);

pub struct BubbleSleeper;

impl BubbleSleeper {
    pub fn new() -> Self {
        Self
    }
}

impl backon::Sleeper for BubbleSleeper {
    type Sleep = runtime::time::Sleep;

    fn sleep(&self, dur: std::time::Duration) -> Self::Sleep {
        time::sleep_future(dur)
    }
}

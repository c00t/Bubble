//! Customed async runtime for dynamic shared library
#![feature(ptr_metadata)]
#![feature(downcast_unchecked)]
#![feature(min_specialization)]

pub use compio::*;
pub mod task_system_api;
pub mod types;
pub use async_ffi;
pub use futures_channel;
pub use futures_util;

/// A special future
pub struct LocalFuture<T>(async_ffi::LocalFfiFuture<T>);

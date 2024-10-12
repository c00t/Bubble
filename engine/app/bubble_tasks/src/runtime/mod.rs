//! A runtime for asynchronous tasks, with a global thread-per-core registry
//! which can be set outside.

mod attacher;
mod core;
pub mod event;
#[cfg(feature = "time")]
pub mod time;

pub use async_task::Task;
pub use attacher::*;
use compio_buf::BufResult;
pub use core::{
    registry, set_global_registry, spawn, spawn_blocking, submit, submit_with_flags, JoinHandle,
    Runtime, RuntimeBuilder,
};

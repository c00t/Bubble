#![feature(ptr_metadata)]
#![feature(downcast_unchecked)]
// #![feature(trait_upcasting)]
// #![allow(incomplete_features)]

pub mod alloc;
pub mod api;
pub mod os;
pub mod sync;
pub use bon;
pub use tracing;

pub use os::thread::dyntls::{lazy_static, scoped_thread_local, thread_local};

pub mod prelude {
    pub use super::tracing::{debug, error, event, info, instrument, span, trace, warn};
}

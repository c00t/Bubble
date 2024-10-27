use std::cell::OnceCell;

pub use dyntls::{lazy_static, scoped_thread_local, thread_local, Context, LocalKey, ScopedKey};

/// A thread id represent system-wide thread id
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub struct SysThreadId(u64);

crate::thread_local! {
    /// A thread local [`ThreadId`] that can be used to identify the current thread
    static SYSTHREAD_ID: OnceCell<SysThreadId> = OnceCell::new();
}

impl SysThreadId {
    /// Get the current thread id
    pub fn current() -> Self {
        SYSTHREAD_ID.with(|id| *id.get_or_init(|| SysThreadId(gettid::gettid())))
    }

    /// Get the uncached thread id
    pub fn current_uncached() -> Self {
        SysThreadId(gettid::gettid())
    }
}

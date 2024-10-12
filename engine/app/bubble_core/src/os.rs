use std::cell::OnceCell;

/// A thread id represent system-wide thread id
#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub struct SysThreadId(u64);

thread_local! {
    /// A threaq local [`ThreadId`] that can be used to identify the current thread
    static THREAD_ID: OnceCell<SysThreadId> = OnceCell::new();
}

impl SysThreadId {
    /// Get the current thread id
    pub fn current() -> Self {
        THREAD_ID.with(|id| *id.get_or_init(|| SysThreadId(gettid::gettid())))
    }
}

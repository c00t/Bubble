//! Customed async runtime for dynamic shared library

pub use compio::*;

/// The hint for the task system to run the task on the specific thread pool which bind to some specific cores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffinityHint {
    /// The task should be run on the performance core
    Performance,
    /// The task should be run on the efficiency core
    Efficiency,
}

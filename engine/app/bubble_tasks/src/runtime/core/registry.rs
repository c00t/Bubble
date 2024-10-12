//! A registry which can be dynamicly set and get.

use crate::runtime::Runtime;
use bubble_core::os::SysThreadId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// An readonly registry which can be shared across threads&dynamic libraries,
/// and can be used between async/await points.
pub struct SharedRuntimeRegistry {
    registry: HashMap<SysThreadId, ThreadRuntimeRef>,
}

/// A wrapper around `*const Runtime`.
///
/// It's [`Send`] and [`Sync`], through we collect them across threads, we only use them as [`Runtime`] inside the thread create it.
/// The plain data under the reference is always thread-safe, what's not thread-safe is the semantic of [`Runtime`] itself.
///
/// # Safety
///
/// You should ensure that the [`Runtime`] is always accessible from the thread which creates it, and only the thread which creates it can access it.
pub struct ThreadRuntimeRef(pub *const Runtime);

impl SharedRuntimeRegistry {
    /// Get current runtime from the registry.
    pub fn get_current(self: Arc<Self>) -> Option<&'static Runtime> {
        self.registry
            .get(&SysThreadId::current())
            .map(|runtime| unsafe { &*runtime.0 })
    }
}

unsafe impl Sync for ThreadRuntimeRef {}

unsafe impl Send for ThreadRuntimeRef {}

/// A builder which can be used to create a [`SharedRegistry`].
/// It's mutable, and can be used to add more runtimes to the registry.
#[derive(Clone)]
pub struct SharedRuntimeRegistryBuilder {
    registry: Arc<Mutex<HashMap<SysThreadId, ThreadRuntimeRef>>>,
}

impl SharedRuntimeRegistryBuilder {
    /// Creates a new [`SharedRegistryBuilder`].
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    /// register a runtime into the registry.
    pub fn register(&mut self, runtime: ThreadRuntimeRef) {
        self.registry
            .lock()
            .unwrap()
            .insert(SysThreadId::current(), runtime);
    }
    /// Build the [`SharedRegistry`].
    pub fn build(self) -> Option<SharedRuntimeRegistry> {
        // ensure that only 1 strong count exists, so that we can unwrap the lock.
        debug_assert_eq!(Arc::strong_count(&self.registry), 1);
        let registry = Arc::try_unwrap(self.registry).ok()?.into_inner().ok()?;
        Some(SharedRuntimeRegistry { registry })
    }
}

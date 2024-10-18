//! API registry that store all APIs that are registered by plugins.
//!
//! It expose an generic API through [`ApiRegistryApi`], and provide an implementation of it through [`ApiRegistry`].
//! Because [`ApiRegistryApi`] will be accessed by both static and dynamic plugins, it's desigend to be generic-free.
//! So it'll use an opaque trait object handler in its api, which can be downcasted to the actual API trait object.
//!
//! ## [`ApiHandle`] vs. [`LocalApiHandle`]
//!
//! [`ApiHandle`] is a plugin(or future)-wide global handle to an API, it will be stored by a static Plugin instance in plugin dll,
//! while [`LocalApiHandle`] is usually a function local handle to an API, it will be used by a function(or future) to access the API.
//! [`LocalApiHandle`] reduces the overhead of ref count increment and decrement in functions, while avoid freeing memory during the function execution.
//!

use std::{
    any::{type_name, type_name_of_val},
    marker,
    ops::Deref,
    sync::{OnceLock, RwLock},
};

use crate::api::{
    make_trait_castable, make_trait_castable_decl, unique_id, TraitcastableAny,
    TraitcastableAnyInfra, TraitcastableAnyInfraExt, UniqueId, UniqueTypeId,
};
use aarc::RefCount;
use bubble_macros::define_api;
use rustc_hash::FxHashMap;
use semver::Version;

use super::Api;
use crate::sync::{Arc, AtomicArc, Guard};

/// A plugin(or future) global type-aware handle to an API.
///
/// Usually get from [`AnyApiHandle`] by downcasting.
pub struct ApiHandle<T: 'static + ?Sized> {
    inner: AnyApiHandle,
    _phantom_type: marker::PhantomData<*const T>,
}

impl<T: 'static + ?Sized> ApiHandle<T> {
    pub fn get<'local>(&'local self) -> Option<LocalApiHandle<'local, T>> {
        self.inner.0.load().map(|guard| LocalApiHandle {
            _guard: guard,
            _phantom_type: marker::PhantomData,
            _api_handle_ref: self,
        })
    }
}

impl<T: 'static + ?Sized> Clone for ApiHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom_type: self._phantom_type.clone(),
        }
    }
}

impl<T: 'static + ?Sized> RefCount for ApiHandle<T> {
    /// Get the strong count of the underlying [`Arc`].
    fn strong_count(&self) -> usize {
        self.inner.0.strong_count()
    }
    /// Get the weak count of the underlying [`Arc`].
    fn weak_count(&self) -> usize {
        self.inner.0.weak_count()
    }
}

unsafe impl<T: 'static + ?Sized> Send for ApiHandle<T> {}
unsafe impl<T: 'static + ?Sized> Sync for ApiHandle<T> {}

/// A function local type-aware handle to an API.
///
/// Usually get from [`ApiHandle`] by [`ApiHandle::get`]. It avoid overhead of ref count
/// increment and decrement compare to [`ApiHandle`] when used inside a function or future.
/// And avoid underlying memory be freed. Usually you'll get a [`LocalApiHandle`] from [`ApiHandle`]
/// by [`ApiHandle::get`] at the start of functions, and if you want to access the API in a async function,
/// you should use [`LocalApiHandle::from`] to create a new [`ApiHandle`].
///
/// You don't need to remember their differences, [`ApiHandle`] implement [`Send`] and [`Sync`], while [`LocalApiHandle`] doesn't.
pub struct LocalApiHandle<'local, T: 'static + ?Sized> {
    // a guard that pervent the api from being dropped
    _guard: Guard<Box<(dyn TraitcastableAny + 'static)>>,
    _api_handle_ref: &'local ApiHandle<T>,
    _phantom_type: marker::PhantomData<*const T>,
}

impl<T: 'static + ?Sized> RefCount for LocalApiHandle<'_, T> {
    /// Return a strong count of the [`AtomicArc`] object
    ///
    /// ## Note
    ///
    /// Typically, strong count of [`LocalApiHandle`] won't equal to the strong count of the
    /// [`ApiHandle`] that be generated from.
    fn strong_count(&self) -> usize {
        self._guard.strong_count()
    }

    /// Return a weak count of the [`AtomicArc`] object
    ///
    /// ## Note
    ///
    /// Typically, weak count of [`LocalApiHandle`] won't equal to the weak count of the
    /// [`ApiHandle`] that be generated from.
    fn weak_count(&self) -> usize {
        self._guard.weak_count()
    }
}

impl<T: 'static + ?Sized> From<LocalApiHandle<'_, T>> for ApiHandle<T>
where
    dyn TraitcastableAny: TraitcastableAnyInfra<T>,
{
    fn from(value: LocalApiHandle<T>) -> Self {
        value._api_handle_ref.clone()
    }
}

impl<T: 'static + ?Sized> From<&LocalApiHandle<'_, T>> for ApiHandle<T>
where
    dyn TraitcastableAny: TraitcastableAnyInfra<T>,
{
    fn from(value: &LocalApiHandle<T>) -> Self {
        value._api_handle_ref.clone()
    }
}

impl<'local, T: 'static + ?Sized> From<&'local ApiHandle<T>> for LocalApiHandle<'local, T>
where
    dyn TraitcastableAny: TraitcastableAnyInfra<T>,
{
    fn from(value: &'local ApiHandle<T>) -> Self {
        // it shouldn't fail (with same version of compiler toolchain), the type infomation was encoded in the generic
        value.get().unwrap()
    }
}

impl<T: 'static + ?Sized> Deref for LocalApiHandle<'_, T>
where
    dyn TraitcastableAny: TraitcastableAnyInfra<T>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self._guard.as_ref().downcast_ref().unwrap()
    }
}

/// Opaque api handle returned by [`ApiRegistryApi::find`].
///
/// It can be downcasted to a specific api using [`AnyApiHandle::downcast`].
///
/// ## Example
///
/// ```no_run
/// use bubble_core::api::api_registy_api::{ApiRegistryApi, AnyApiHandle, ApiHandle};
///
/// let task_system_api: ApiHandle<dyn TaskSystemApi> = api_registry_api.find(task_system_api::NAME, task_system_api::VERSION).downcast();
///
/// ```
pub struct AnyApiHandle(Arc<AtomicArc<Box<dyn TraitcastableAny>>>);

impl AnyApiHandle {
    /// Downcast the opaque api handle to a specific api handle.
    pub fn downcast<T: 'static + ?Sized>(self) -> ApiHandle<T> {
        ApiHandle {
            inner: self,
            _phantom_type: marker::PhantomData,
        }
    }
}

impl Clone for AnyApiHandle {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: 'static + TraitcastableAny> From<Box<T>> for AnyApiHandle
where
    dyn TraitcastableAny: TraitcastableAnyInfra<T>,
{
    fn from(value: Box<T>) -> Self {
        let boxed_any: Box<dyn TraitcastableAny> = value;
        AnyApiHandle(Arc::new(AtomicArc::new(boxed_any)))
    }
}

unsafe impl Send for AnyApiHandle {}
unsafe impl Sync for AnyApiHandle {}

/// Api registry api.
///
/// This api is used to register and find apis.
pub trait ApiRegistryApi: Api {
    /// Set an api with specific name and version.
    ///
    /// Typically, if you want to register an api, you create a `Box<dyn YourApiTraitName>` or `Box<YourApiStructName>`,
    /// and then use [`AnyApiHandle::from<Box<T>>`] where `T:'static + TraitcastableAny` convert it to `AnyApiHandle`.
    /// Or if you using #[define_api] to define your api, you can use the public functions exposed as `pub fn get_your_api_trait_name() -> ApiHandle<dyn YourApiTrait>`.
    fn set(&self, name: &'static str, version: Version, api: AnyApiHandle);
    /// Remove an api with specific name and version.
    fn remove(&self, name: &'static str, version: Version);
    /// Find an api with specific name and version.
    fn find(&self, name: &'static str, version: Version) -> AnyApiHandle;
    /// Clear and shut down the api registry. Currently not implemented.
    ///
    /// ## Note
    ///
    /// Do we really need it? All memory will be freed when the program exits.
    fn shutdown(&self);
    fn ref_counts(&self);
}

struct ApiEntry {
    /// Arc<AtomicArc<T>> erased to ArcAtomicArcErased
    /// It's `Arc<AtomicArc<dyn TraitcastableAny>>`.
    inner: AnyApiHandle,
}

unsafe impl Send for ApiEntry {}
unsafe impl Sync for ApiEntry {}

/// An api registry that can be used to register api implementations.
struct ApiRegistry {
    // A hash map that maps api types to their implementations
    apis: RwLock<FxHashMap<(&'static str, Version), ApiEntry>>,
}

// crate::impl_api!(ApiRegistryRef, ApiRegistryApi, (0, 1, 0));

// #[make_trait_castable(Api, ApiRegistryApi)]
#[define_api((0,1,0), bubble_core::api::api_registry_api::ApiRegistryApi)]
struct ApiRegistryRef {
    pub inner: &'static ApiRegistry,
}

/// It will be managed by the main executable, so no need to use dyntls.
static API_REGISTRY: OnceLock<ApiRegistry> = OnceLock::new();

impl ApiRegistryRef {
    fn new() -> ApiHandle<dyn ApiRegistryApi> {
        let api_registry_api: AnyApiHandle = Box::new(ApiRegistryRef {
            inner: API_REGISTRY.get_or_init(|| ApiRegistry::new()),
        })
        .into();
        api_registry_api.downcast()
    }
}

impl ApiRegistry {
    pub fn new() -> Self {
        Self {
            apis: RwLock::new(FxHashMap::default()),
        }
    }
    pub fn set(&self, name: &'static str, version: Version, api: AnyApiHandle) {
        let id = (name, version);
        let entry = ApiEntry { inner: api };
        self.apis.write().unwrap().insert(id, entry);
    }

    pub fn remove(&self, name: &'static str, version: Version) {
        todo!()
    }

    pub fn get(&self, name: &'static str, version: Version) -> AnyApiHandle {
        // get the raw entry
        let r_lock = self.apis.read().unwrap();
        let id = (name, version.clone());
        if let Some(raw_entry) = r_lock.get(&id) {
            raw_entry.inner.clone()
        } else {
            drop(r_lock);
            let mut w_lock = self.apis.write().unwrap();
            // if don't have the api, create a new one inside registry
            let raw_entry = w_lock.entry((name, version)).or_insert({
                let new_arc_to_null = Arc::new(AtomicArc::new(None));
                ApiEntry {
                    inner: AnyApiHandle(new_arc_to_null),
                }
            });
            raw_entry.inner.clone()
        }
    }

    pub fn ref_counts(&self) {
        // loop through the hash map and print the ref counts
    }

    pub fn shutdown(self) {}
}

impl ApiRegistryApi for ApiRegistryRef
where
    Self: 'static,
{
    fn set(&self, name: &'static str, version: Version, api: AnyApiHandle) {
        self.inner.set(name, version, api);
    }

    fn remove(&self, name: &'static str, version: Version) {
        todo!()
    }

    fn find(&self, name: &'static str, version: Version) -> AnyApiHandle {
        self.inner.get(name, version)
    }

    fn ref_counts(&self) {}

    fn shutdown(&self) {
        todo!()
    }
}

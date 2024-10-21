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
    fmt::Display,
    marker,
    ops::Deref,
    sync::{OnceLock, RwLock},
};

use super::prelude::*;

use bon::bon;
use bubble_macros::{define_api, define_api_with_id};
use rustc_hash::FxHashMap;
use thiserror::Error;

use super::Api;
use crate::sync::{Arc, AsPtr, AtomicArc, Guard, RefCount};

type HandleInternal<T> = Arc<AtomicArc<Box<T>>>;

/// A plugin(or future) global type-aware handle to an API.
///
/// Usually get from [`AnyApiHandle`] by downcasting.
pub struct ApiHandle<T: 'static + ?Sized + Api> {
    inner: AnyApiHandle,
    _phantom_type: marker::PhantomData<HandleInternal<T>>,
}

impl<T: 'static + ?Sized + Api> ApiHandle<T> {
    /// Get a [`LocalApiHandle`] of api type `dyn ApiTraitName`, return `None` if underlying api has not been registered
    ///
    /// ## Note
    ///
    /// When the api is reloaded by the plugin register it, [`ApiHandle`] will automatically reference the updated api. But
    /// you can still use the [`LocalApiHandle`] created by old [`ApiHandle`] to access the old api. Use it carefully when you're
    /// inside async context. When you access [`ApiHandle`] across await points, you may access a new api. When you access [`LocalApiHandle`]
    /// across await points, you still access the old api.
    ///
    /// ## TODO
    ///
    /// add a era counter into [`AnyApiHandle`] when debugging, to indicate whether the api is updated.
    pub fn get<'local>(&'local self) -> Option<LocalApiHandle<'local, T>> {
        self.inner.0.load().map(|guard| LocalApiHandle {
            _guard: guard,
            _phantom_type: marker::PhantomData,
            _api_handle_ref: self,
        })
    }
}

impl<T: 'static + ?Sized + Api> std::fmt::Debug for ApiHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiHandle").field(">", &self.inner).finish()
    }
}

impl<T: 'static + ?Sized + Api> Clone for ApiHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom_type: self._phantom_type.clone(),
        }
    }
}

impl<T: 'static + ?Sized + Api> RefCount for ApiHandle<T> {
    /// Get the strong count of the underlying [`Arc`].
    fn strong_count(&self) -> usize {
        self.inner.0.strong_count()
    }
    /// Get the weak count of the underlying [`Arc`].
    fn weak_count(&self) -> usize {
        self.inner.0.weak_count()
    }
}

pub struct InterfaceHandle<T: 'static + ?Sized + Interface> {
    inner: AnyInterfaceHandle,
    _phantom_type: marker::PhantomData<HandleInternal<T>>,
}

impl<T: 'static + ?Sized + Interface> InterfaceHandle<T> {
    pub fn get<'local>(&'local self) -> Option<LocalInterfaceHandle<'local, T>> {
        self.inner.0.load().map(|guard| LocalInterfaceHandle {
            _guard: guard,
            _phantom_type: marker::PhantomData,
            _api_handle_ref: self,
        })
    }
}

impl<T: 'static + ?Sized + Interface> std::fmt::Debug for InterfaceHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InterfaceHandle")
            .field(">", &self.inner)
            .finish()
    }
}

impl<T: 'static + ?Sized + Interface> Clone for InterfaceHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom_type: self._phantom_type.clone(),
        }
    }
}

impl<T: 'static + ?Sized + Interface> RefCount for InterfaceHandle<T> {
    /// Get the strong count of the underlying [`Arc`].
    fn strong_count(&self) -> usize {
        self.inner.0.strong_count()
    }
    /// Get the weak count of the underlying [`Arc`].
    fn weak_count(&self) -> usize {
        self.inner.0.weak_count()
    }
}

/// A function local type-aware handle to an API.
///
/// Usually get from [`ApiHandle`] by [`ApiHandle::get`]. It avoid overhead of ref count
/// increment and decrement compare to [`ApiHandle`] when used inside a function or future.
/// And avoid underlying memory be freed. Usually you'll get a [`LocalApiHandle`] from [`ApiHandle`]
/// by [`ApiHandle::get`] at the start of functions, and if you want to access the API in a async function,
/// you should use [`LocalApiHandle::from`] to create a new [`ApiHandle`].
///
/// You don't need to remember their differences, [`ApiHandle`] implement [`Send`] and [`Sync`], while [`LocalApiHandle`] doesn't.
pub struct LocalApiHandle<'local, T: 'static + ?Sized + Api> {
    // a guard that pervent the api from being dropped
    _guard: Guard<Box<dyn TraitcastableAny + 'static + Sync + Send>>,
    _api_handle_ref: &'local ApiHandle<T>,
    _phantom_type: marker::PhantomData<&'local T>,
}

impl<'local, T: 'static + ?Sized + Api> std::fmt::Debug for LocalApiHandle<'local, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Some((name, version)) = self
            ._guard
            .as_ref()
            .downcast_ref()
            .and_then(|box_api: &dyn Api| Some((box_api.name(), box_api.version())))
        else {
            return f.debug_struct("Invalid LocalApiHandle").finish();
        };
        f.debug_struct("LocalApiHandle")
            .field("name", &name)
            .field("version", &version)
            .field("strong_count", &self._guard.strong_count())
            .field("weak_count", &self._guard.weak_count())
            .finish()
    }
}

impl<T: 'static + ?Sized + Api> RefCount for LocalApiHandle<'_, T> {
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

impl<T: 'static + ?Sized + Api> From<LocalApiHandle<'_, T>> for ApiHandle<T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: LocalApiHandle<T>) -> Self {
        value._api_handle_ref.clone()
    }
}

impl<T: 'static + ?Sized + Api> From<&LocalApiHandle<'_, T>> for ApiHandle<T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: &LocalApiHandle<T>) -> Self {
        value._api_handle_ref.clone()
    }
}

impl<'local, T: 'static + ?Sized + Api> From<&'local ApiHandle<T>> for LocalApiHandle<'local, T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: &'local ApiHandle<T>) -> Self {
        // it shouldn't fail (with same version of compiler toolchain), the type infomation was encoded in the generic
        value.get().unwrap()
    }
}

impl<T: 'static + ?Sized + Api> Deref for LocalApiHandle<'_, T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self._guard.as_ref().downcast_ref().unwrap()
    }
}

pub struct LocalInterfaceHandle<'local, T: 'static + ?Sized + Interface> {
    // a guard that pervent the api from being dropped
    _guard: Guard<Box<dyn TraitcastableAny + 'static + Sync + Send>>,
    _api_handle_ref: &'local InterfaceHandle<T>,
    _phantom_type: marker::PhantomData<&'local T>,
}

impl<'local, T: 'static + ?Sized + Interface> std::fmt::Debug for LocalInterfaceHandle<'local, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Some((name, version)) = self
            ._guard
            .as_ref()
            .downcast_ref()
            .and_then(|box_api: &dyn Api| Some((box_api.name(), box_api.version())))
        else {
            return f.debug_struct("Invalid LocalInterfaceHandle").finish();
        };
        f.debug_struct("LocalInterfaceHandle")
            .field("name", &name)
            .field("version", &version)
            .field("strong_count", &self._guard.strong_count())
            .field("weak_count", &self._guard.weak_count())
            .finish()
    }
}

impl<T: 'static + ?Sized + Interface> RefCount for LocalInterfaceHandle<'_, T> {
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

impl<T: 'static + ?Sized + Interface> From<LocalInterfaceHandle<'_, T>> for InterfaceHandle<T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: LocalInterfaceHandle<T>) -> Self {
        value._api_handle_ref.clone()
    }
}

impl<T: 'static + ?Sized + Interface> From<&LocalInterfaceHandle<'_, T>> for InterfaceHandle<T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: &LocalInterfaceHandle<T>) -> Self {
        value._api_handle_ref.clone()
    }
}

impl<'local, T: 'static + ?Sized + Interface> From<&'local InterfaceHandle<T>>
    for LocalInterfaceHandle<'local, T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: &'local InterfaceHandle<T>) -> Self {
        // it shouldn't fail (with same version of compiler toolchain), the type infomation was encoded in the generic
        value.get().unwrap()
    }
}

impl<T: 'static + ?Sized + Interface> Deref for LocalInterfaceHandle<'_, T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
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
pub struct AnyApiHandle(HandleInternal<dyn TraitcastableAny + Sync + Send>);

impl AnyApiHandle {
    /// Downcast the opaque api handle to a specific api handle.
    pub fn downcast<T: 'static + ?Sized + Api>(self) -> ApiHandle<T> {
        ApiHandle {
            inner: self,
            _phantom_type: marker::PhantomData,
        }
    }
}

impl std::fmt::Debug for AnyApiHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Some((name, version)) = self.0.load().and_then(|guard| {
            let trait_cast_any = guard.as_ref();
            let box_api: &dyn Api = trait_cast_any.downcast_ref()?;
            Some((box_api.name(), box_api.version()))
        }) else {
            return f.debug_struct("Invalid AnyApiHandle").finish();
        };
        f.debug_struct("AnyApiHandle")
            .field("name", &name)
            .field("version", &version)
            .field("strong_count", &self.0.strong_count())
            .field("weak_count", &self.0.weak_count())
            .finish()
    }
}

impl Clone for AnyApiHandle {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: 'static + TraitcastableAny + Sync + Send> From<Box<T>> for AnyApiHandle
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: Box<T>) -> Self {
        let boxed_any: Box<dyn TraitcastableAny + Sync + Send> = value;
        AnyApiHandle(Arc::new(AtomicArc::new(boxed_any)))
    }
}

impl<T: 'static + ?Sized + Api> From<ApiHandle<T>> for AnyApiHandle {
    fn from(value: ApiHandle<T>) -> Self {
        value.inner
    }
}

pub struct AnyInterfaceHandle(HandleInternal<dyn TraitcastableAny + Sync + Send>);

impl AnyInterfaceHandle {
    /// Downcast the opaque api handle to a specific api handle.
    pub fn downcast<T: 'static + ?Sized + Interface>(self) -> InterfaceHandle<T> {
        InterfaceHandle {
            inner: self,
            _phantom_type: marker::PhantomData,
        }
    }
}

impl std::fmt::Debug for AnyInterfaceHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Some((name, version)) = self.0.load().and_then(|guard| {
            let trait_cast_any = guard.as_ref();
            let box_api: &dyn Api = trait_cast_any.downcast_ref()?;
            Some((box_api.name(), box_api.version()))
        }) else {
            return f.debug_struct("Invalid AnyInterfaceHandle").finish();
        };
        f.debug_struct("AnyInterfaceHandle")
            .field("name", &name)
            .field("version", &version)
            .field("strong_count", &self.0.strong_count())
            .field("weak_count", &self.0.weak_count())
            .finish()
    }
}

impl Clone for AnyInterfaceHandle {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: 'static + TraitcastableAny + Sync + Send> From<Box<T>> for AnyInterfaceHandle
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: Box<T>) -> Self {
        let boxed_any: Box<dyn TraitcastableAny + Sync + Send> = value;
        AnyInterfaceHandle(Arc::new(AtomicArc::new(boxed_any)))
    }
}

impl<T: 'static + ?Sized + Interface> From<InterfaceHandle<T>> for AnyInterfaceHandle {
    fn from(value: InterfaceHandle<T>) -> Self {
        value.inner
    }
}

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Null entry inside api registry, which usually means you're accessing this api after")]
    NullEntry(String),
}

/// Api registry api.
///
/// This api is used to register and find apis.
pub trait ApiRegistryApi: Api {
    /// Set an api with specific name and version.
    ///
    /// Typically, if you want to register an api, you create a `Box<dyn YourApiTraitName>` or `Box<YourApiStructName>`,
    /// and then use [`AnyApiHandle::from<Box<T>>`] where `T:'static + TraitcastableAny` convert it to `AnyApiHandle`.
    /// Or if you using #[define_api] to define your api, you can use the public functions exposed as `pub fn get_your_api_trait_name() -> ApiHandle<dyn YourApiTrait>`.
    fn set(&self, name: &'static str, version: Version, api: AnyApiHandle) -> AnyApiHandle;
    /// Remove an api with specific name and version.
    ///
    /// Though removed, plugins can still access the api if they have a reference to it.
    /// Return None if the api not found.
    fn remove(&self, name: &'static str, version: Version) -> Option<AnyApiHandle>;
    /// Find an api with specific name and version.
    ///
    /// It will return a [`Arc`] which points to a null pointer if the api not found,
    /// Be careful, you shouldn't access the api handle when you're inside `load_plugin`,
    /// and you shouldn't access the api handle when the plugin is unloaded.
    fn find(&self, name: &'static str, version: Version) -> AnyApiHandle;
    /// Clear and shut down the api registry. Currently not implemented.
    ///
    /// ## Note
    ///
    /// Do we really need it? All memory will be freed when the program exits.
    ///
    /// It will swap all internal ptr to null
    fn shutdown(&self);
    fn ref_counts(&self);

    fn add_interface(
        &self,
        name: &'static str,
        version: Version,
        interface: AnyInterfaceHandle,
    ) -> AnyInterfaceHandle;

    fn interface_count(&self, name: &'static str, version: Version) -> Option<usize>;
    fn get_interfaces(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<Vec<AnyInterfaceHandle>>;
    fn first_interface(&self, name: &'static str, version: Version) -> Option<AnyInterfaceHandle>;
    fn available_api_versions(&self, name: &'static str) -> Vec<Version>;
}

struct ApiEntry {
    /// Arc<AtomicArc<T>> erased to ArcAtomicArcErased
    /// It's `Arc<AtomicArc<dyn TraitcastableAny>>`.
    inner: AnyApiHandle,
}

struct InterfaceEntry {
    /// Arc<AtomicArc<T>> erased to ArcAtomicArcErased
    /// It's `Arc<AtomicArc<dyn TraitcastableAny>>`.
    inner: AnyInterfaceHandle,
}

// crate::impl_api!(ApiRegistryRef, ApiRegistryApi, (0, 1, 0));

/// An [`ApiRegistryApi`] implementation.
///
/// It should be efficient to use [`RwLock`] and [`HashMap`] here, it will be wrote rarely.
#[define_api_with_id((0,1,0), bubble_core::api::api_registry_api::ApiRegistryApi)]
struct ApiRegistry {
    /// Store api which is registered by plugins.
    ///
    /// 1 implementation per api
    pub inner_apis: RwLock<FxHashMap<(&'static str, Version), ApiEntry>>,
    /// Store implementationss which is registered by plugins
    ///
    /// multiple implementations per interface
    pub inner_interfaces: RwLock<FxHashMap<(&'static str, Version), Vec<InterfaceEntry>>>,
}

pub fn get_api_registry_api() -> ApiHandle<dyn ApiRegistryApi> {
    ApiRegistry::builder().build()
}

/// It will be managed by the main executable, so no need to use dyntls.
static API_REGISTRY: OnceLock<ApiHandle<dyn ApiRegistryApi>> = OnceLock::new();

#[bon]
impl ApiRegistry {
    #[builder]
    fn new() -> ApiHandle<dyn ApiRegistryApi> {
        let handle = API_REGISTRY
            .get_or_init(|| {
                let registry = ApiRegistry::empty();
                let any_handler: AnyApiHandle = Box::new(registry).into();
                // TODO: remove downcast here to avoid drop delay.
                any_handler.downcast()
            })
            .clone();
        handle
    }

    pub fn empty() -> Self {
        Self {
            inner_apis: RwLock::new(FxHashMap::default()),
            inner_interfaces: RwLock::new(FxHashMap::default()),
        }
    }

    pub fn set(&self, name: &'static str, version: Version, api: AnyApiHandle) -> AnyApiHandle {
        let id = (name, version);
        let mut w_lock = self.inner_apis.write().unwrap();
        // if don't have the api, create a new one inside registry,
        // if specific entry is already exist, update the api using atomic store
        let raw_entry = w_lock
            .entry(id.clone())
            .and_modify(|entry| {
                // TODO: if we don't use RWLock in ApiRegistry, we should use compare_exchange
                println!("found {:?} when set", id.clone());
                let arc = &entry.inner.0;
                arc.store(api.0.load().as_ref());
            })
            .or_insert({
                println!("not found {:?} when set", id.clone());
                ApiEntry { inner: api }
            });
        raw_entry.inner.clone()
    }

    pub fn add_interface(
        &self,
        name: &'static str,
        version: Version,
        interface: AnyInterfaceHandle,
    ) -> AnyInterfaceHandle {
        let id = (name, version);
        let mut w_lock = self.inner_interfaces.write().unwrap();
        let raw_entry = w_lock
            .entry(id)
            .and_modify(|entry| {
                entry.push(InterfaceEntry {
                    inner: interface.clone(),
                });
            })
            .or_insert(Vec::new());
        interface
    }

    pub fn remove(&self, name: &'static str, version: Version) -> Option<AnyApiHandle> {
        let mut w_lock = self.inner_apis.write().unwrap();
        let id = (name, version);
        let entry = w_lock.remove(&id).map(|entry| entry.inner);
        entry
    }

    pub fn remove_interface(
        &self,
        name: &'static str,
        version: Version,
        instance_id: UniqueId,
    ) -> Option<AnyInterfaceHandle> {
        let mut w_lock = self.inner_interfaces.write().unwrap();
        let id = (name, version);
        let mut result = None;
        w_lock.entry(id).and_modify(|entries| {
            let pos = entries.iter_mut().position(|entry| {
                entry
                    .inner
                    .0
                    .load()
                    .and_then(|guard| {
                        let trait_cast_any = guard.as_ref();
                        let box_interface: &dyn Interface = trait_cast_any.downcast_ref()?;
                        Some(box_interface.id() == instance_id)
                    })
                    .unwrap_or(false)
            });
            if let Some(pos) = pos {
                result = Some(entries.remove(pos).inner);
            }
        });
        result
    }

    pub fn get(&self, name: &'static str, version: Version) -> AnyApiHandle {
        let r_lock = self.inner_apis.read().unwrap();

        // Find the best matching version
        let best_match = r_lock
            .keys()
            .filter(|&(key_name, key_version)| *key_name == name)
            .max_by(|&(_, a), &(_, b)| a.cmp(b));

        if let Some((_, best_version)) = best_match {
            let id = (name, best_version.clone());
            if let Some(raw_entry) = r_lock.get(&id) {
                println!("found {:?} when get", id);
                return raw_entry.inner.clone();
            }
        }

        // If no match found, create a new entry
        drop(r_lock);
        println!("not found {:?} when get", (name, version.clone()));
        let mut w_lock = self.inner_apis.write().unwrap();
        let id = (name, version);
        let raw_entry = w_lock.entry(id).or_insert_with(|| {
            let new_arc_to_null = Arc::new(AtomicArc::new(None));
            ApiEntry {
                inner: AnyApiHandle(new_arc_to_null),
            }
        });
        raw_entry.inner.clone()
    }

    pub fn ref_counts(&self) {
        // loop through the hash map and print the ref counts
    }

    pub fn interface_count(&self, name: &'static str, version: Version) -> Option<usize> {
        let id = (name, version);
        let mut r_lock = self.inner_interfaces.read().unwrap();

        r_lock.get(&id).map(|v| v.len())
    }

    pub fn get_interfaces(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<Vec<AnyInterfaceHandle>> {
        let id = (name, version);
        let r_lock = self.inner_interfaces.read().unwrap();
        r_lock
            .get(&id)
            .map(|v| v.iter().map(|v| v.inner.clone()).collect())
    }

    pub fn first_interface(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<AnyInterfaceHandle> {
        let id = (name, version);
        let r_lock = self.inner_interfaces.read().unwrap();
        r_lock
            .get(&id)
            .and_then(|v| v.first().map(|v| v.inner.clone()))
    }

    pub fn available_api_versions(&self, name: &'static str) -> Vec<Version> {
        // return the specific api loaded versions
        let mut versions = Vec::new();
        let r_lock = self.inner_apis.read().unwrap();
        for (k, _) in r_lock.iter() {
            if k.0 == name {
                versions.push(k.1.clone());
            }
        }
        versions
    }

    fn shutdown(&self) {
        // for all entry in the hash map, drop the api
        self.inner_apis.write().unwrap().clear();
    }
}

impl ApiRegistryApi for ApiRegistry {
    fn set(&self, name: &'static str, version: Version, api: AnyApiHandle) -> AnyApiHandle {
        self.set(name, version, api)
    }

    fn remove(&self, name: &'static str, version: Version) -> Option<AnyApiHandle> {
        self.remove(name, version)
    }

    fn find(&self, name: &'static str, version: Version) -> AnyApiHandle {
        self.get(name, version)
    }

    fn ref_counts(&self) {}

    fn shutdown(&self) {
        self.shutdown();
    }
    fn add_interface(
        &self,
        name: &'static str,
        version: Version,
        interface: AnyInterfaceHandle,
    ) -> AnyInterfaceHandle {
        self.add_interface(name, version, interface)
    }

    fn interface_count(&self, name: &'static str, version: Version) -> Option<usize> {
        self.interface_count(name, version)
    }

    fn get_interfaces(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<Vec<AnyInterfaceHandle>> {
        self.get_interfaces(name, version)
    }

    fn first_interface(&self, name: &'static str, version: Version) -> Option<AnyInterfaceHandle> {
        self.first_interface(name, version)
    }

    fn available_api_versions(&self, name: &'static str) -> Vec<Version> {
        self.available_api_versions(name)
    }
}

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

use core::fmt;
use std::{
    marker,
    ops::Deref,
    sync::{OnceLock, RwLock},
};

use super::prelude::*;
#[allow(unused_imports)]
use crate::tracing::{debug, error, info, trace, warn};
use bon::bon;
use bubble_macros::{declare_api, define_api};
use rustc_hash::FxHashMap;
use thiserror::Error;

use super::Api;
use crate::sync::{Arc, AsPtr, AtomicArc, Guard, RefCount, Weak};

type HandleInternal<T> = Arc<AtomicArc<Box<T>>>;
type WeakHandleInternal<T> = Weak<AtomicArc<Box<T>>>;

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
            guard,
            phantom_type: marker::PhantomData,
            api_handle_ref: self,
        })
    }
}

impl<T: 'static + ?Sized + Api> fmt::Debug for ApiHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiHandle").field("", &self.inner).finish()
    }
}

impl<T: 'static + ?Sized + Api> fmt::Display for ApiHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
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
            guard,
            phantom_type: marker::PhantomData,
            api_handle_ref: self,
        })
    }
}

impl<T: 'static + ?Sized + Interface> fmt::Debug for InterfaceHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InterfaceHandle")
            .field("", &self.inner)
            .finish()
    }
}

impl<T: 'static + ?Sized + Interface> fmt::Display for InterfaceHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
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
    /// A guard that pervent the api from being dropped.
    guard: Guard<Box<dyn TraitcastableAny + 'static + Sync + Send>>,
    /// Used to cast back to [`ApiHandle`], when use [`circ`], this field can be ommited.
    api_handle_ref: &'local ApiHandle<T>,
    phantom_type: marker::PhantomData<&'local T>,
}

impl<'local, T: 'static + ?Sized + Api> fmt::Debug for LocalApiHandle<'local, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((name, version)) = self
            .guard
            .as_ref()
            .downcast_ref()
            .and_then(|box_api: &dyn Api| Some((box_api.name(), box_api.version())))
        else {
            return f.debug_struct("Invalid LocalApiHandle").finish();
        };
        f.debug_struct("LocalApiHandle")
            .field("name", &name)
            .field("version", &version)
            .field("strong_count", &self.guard.strong_count())
            .field("weak_count", &self.guard.weak_count())
            .finish()
    }
}

impl<T: 'static + ?Sized + Api> fmt::Display for LocalApiHandle<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((name, version)) = self
            .guard
            .as_ref()
            .downcast_ref()
            .and_then(|box_api: &dyn Api| Some((box_api.name(), box_api.version())))
        else {
            return write!(f, "Invalid LocalApiHandle");
        };
        write!(f, "{}::{}", name, version)
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
        self.guard.strong_count()
    }

    /// Return a weak count of the [`AtomicArc`] object
    ///
    /// ## Note
    ///
    /// Typically, weak count of [`LocalApiHandle`] won't equal to the weak count of the
    /// [`ApiHandle`] that be generated from.
    fn weak_count(&self) -> usize {
        self.guard.weak_count()
    }
}

impl<T: 'static + ?Sized + Api> From<LocalApiHandle<'_, T>> for ApiHandle<T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: LocalApiHandle<T>) -> Self {
        value.api_handle_ref.clone()
    }
}

impl<T: 'static + ?Sized + Api> From<&LocalApiHandle<'_, T>> for ApiHandle<T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: &LocalApiHandle<T>) -> Self {
        value.api_handle_ref.clone()
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
        self.guard
            .as_ref()
            .downcast_ref()
            .expect("Invalid LocalApiHandle")
    }
}

pub struct LocalInterfaceHandle<'local, T: 'static + ?Sized + Interface> {
    // a guard that pervent the api from being dropped
    guard: Guard<Box<dyn TraitcastableAny + 'static + Sync + Send>>,
    api_handle_ref: &'local InterfaceHandle<T>,
    phantom_type: marker::PhantomData<&'local T>,
}

impl<'local, T: 'static + ?Sized + Interface> std::fmt::Debug for LocalInterfaceHandle<'local, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Some((name, version)) = self
            .guard
            .as_ref()
            .downcast_ref()
            .and_then(|box_api: &dyn Api| Some((box_api.name(), box_api.version())))
        else {
            return f.debug_struct("Invalid LocalInterfaceHandle").finish();
        };
        f.debug_struct("LocalInterfaceHandle")
            .field("name", &name)
            .field("version", &version)
            .field("strong_count", &self.guard.strong_count())
            .field("weak_count", &self.guard.weak_count())
            .finish()
    }
}

impl<T: 'static + ?Sized + Interface> fmt::Display for LocalInterfaceHandle<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((name, version)) = self
            .guard
            .as_ref()
            .downcast_ref()
            .and_then(|box_api: &dyn Interface| Some((box_api.name(), box_api.version())))
        else {
            return write!(f, "Invalid LocalInterfaceHandle");
        };
        write!(f, "{}::{}", name, version)
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
        self.guard.strong_count()
    }

    /// Return a weak count of the [`AtomicArc`] object
    ///
    /// ## Note
    ///
    /// Typically, weak count of [`LocalApiHandle`] won't equal to the weak count of the
    /// [`ApiHandle`] that be generated from.
    fn weak_count(&self) -> usize {
        self.guard.weak_count()
    }
}

impl<T: 'static + ?Sized + Interface> From<LocalInterfaceHandle<'_, T>> for InterfaceHandle<T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: LocalInterfaceHandle<T>) -> Self {
        value.api_handle_ref.clone()
    }
}

impl<T: 'static + ?Sized + Interface> From<&LocalInterfaceHandle<'_, T>> for InterfaceHandle<T>
where
    dyn trait_cast_rs::TraitcastableAny + Sync + Send: trait_cast_rs::TraitcastableAnyInfra<T>,
{
    fn from(value: &LocalInterfaceHandle<T>) -> Self {
        value.api_handle_ref.clone()
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
        self.guard.as_ref().downcast_ref().unwrap()
    }
}

/// Opaque api handle returned by [`ApiRegistryApi::find`].
///
/// It can be downcasted to a specific api using [`AnyApiHandle::downcast`].
///
/// ## Example
///
/// ```no_compile
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

impl Api for AnyApiHandle {
    fn name(&self) -> &'static str {
        self.0
            .load()
            .and_then(|guard| {
                let trait_cast_any = guard.as_ref();
                let box_api: &dyn Api = trait_cast_any.downcast_ref()?;
                Some(box_api.name())
            })
            .expect("Invalid AnyApiHandle")
    }

    fn version(&self) -> Version {
        self.0
            .load()
            .and_then(|guard| {
                let trait_cast_any = guard.as_ref();
                let box_api: &dyn Api = trait_cast_any.downcast_ref()?;
                Some(box_api.version())
            })
            .expect("Invalid AnyApiHandle")
    }
}

impl std::fmt::Debug for AnyApiHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyApiHandle")
            .field("name", &self.name())
            .field("version", &self.version())
            .field("strong_count", &self.0.strong_count())
            .field("weak_count", &self.0.weak_count())
            .finish()
    }
}

impl fmt::Display for AnyApiHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.name(), self.version())
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

impl Interface for AnyInterfaceHandle {
    fn name(&self) -> &'static str {
        self.0
            .load()
            .and_then(|guard| {
                let trait_cast_any = guard.as_ref();
                let box_api: &dyn Interface = trait_cast_any.downcast_ref()?;
                Some(box_api.name())
            })
            .expect("Invalid AnyInterfaceHandle")
    }

    fn version(&self) -> Version {
        self.0
            .load()
            .and_then(|guard| {
                let trait_cast_any = guard.as_ref();
                let box_api: &dyn Interface = trait_cast_any.downcast_ref()?;
                Some(box_api.version())
            })
            .expect("Invalid AnyInterfaceHandle")
    }

    fn id(&self) -> UniqueId {
        self.0
            .load()
            .and_then(|guard| {
                let trait_cast_any = guard.as_ref();
                let box_api: &dyn Interface = trait_cast_any.downcast_ref()?;
                Some(box_api.id())
            })
            .expect("Invalid AnyInterfaceHandle")
    }
}

impl std::fmt::Debug for AnyInterfaceHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyInterfaceHandle")
            .field("name", &self.name())
            .field("version", &self.version())
            .field("instance_id", &self.id())
            .field("strong_count", &self.0.strong_count())
            .field("weak_count", &self.0.weak_count())
            .finish()
    }
}

impl fmt::Display for AnyInterfaceHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}::{}", self.name(), self.version(), self.id())
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
#[declare_api((0,1,0), bubble_core::api::api_registry_api::ApiRegistryApi)]
pub trait ApiRegistryApi: Api {
    /// Set an api with specific name and version.
    ///
    /// It may store multiple implementations of the same api, but with different versions.
    /// When you set a new version of an api, it will replace all compatible <name,version> entries with the new version api.
    /// eg. if you set a new version of an api, and the old version is 1.2.3, and the new version is 1.2.4,
    /// then the compatible entries is `<name,1.2.3> -> [1.2.3]`, after set, the compatible entries will be `<name,1.2.3> -> [1.2.4]`.
    /// Now the registry contains two associated entries: `<name,1.2.3> -> [1.2.4]` and `<name,1.2.4> -> [1.2.4]`.
    ///
    /// Typically, if you want to register an api, you create a `Box<dyn YourApiTraitName>` or `Box<YourApiStructName>`,
    /// and then use [`AnyApiHandle::from<Box<T>>`] where `T:'static + TraitcastableAny` convert it to `AnyApiHandle`.
    /// Or if you using #[define_api] to define your api, you can use the public functions exposed as `pub fn get_your_api_trait_name() -> ApiHandle<dyn YourApiTrait>`.
    ///
    /// ## TODO
    ///
    /// We can remove the name and version params in the future, and use the name and version inside [`AnyApiHandle`] instead.
    /// I don't know it's needed or not, because we already have the helper function [`LocalApiHandle<dyn ApiResitryApi>::local_set`].
    /// And the only way you can access an api is getting a [`LocalApiHandle`] first.
    ///
    /// Currently, it will force overwrite the api if the api with the same name and version already exists.
    /// Whether we need an overwrite enable flag?
    fn set(&self, name: &'static str, version: Version, api: AnyApiHandle) -> AnyApiHandle;
    /// Remove an api with specific name and version.
    ///
    /// It will remove implementation of exact <api,version> pair. No semver checking.
    /// So if you set a new version of one api, and then remove the it, with the new version,
    /// the old `<name,version>` entry will still hold the new version api.
    ///
    /// Though removed, plugins can still access the api if they have a reference to it.
    /// Return None if the api not found.
    fn remove(&self, name: &'static str, version: Version) -> Option<AnyApiHandle>;
    /// Find an api with specific name and version.
    ///
    /// It uses "compatible" updates checking([`semver::Op::Caret`]) in [`semver::Op`], so:
    /// - `^I.J.K (for I>0)` — equivalent to `>=I.J.K, <(I+1).0.0`
    /// - `^0.J.K (for J>0)` — equivalent to `>=0.J.K, <0.(J+1).0`
    /// - `^0.0.K` — equivalent to `=0.0.K`
    ///
    /// It will return a [`Arc`] which points to a null pointer if the api not found,
    /// Be careful, you shouldn't access the api handle when you're inside `load_plugin`,
    /// and you shouldn't access the api handle when the plugin is unloaded.
    fn find(&self, name: &'static str, version: Version) -> AnyApiHandle;
    /// Find an api with specific name and version, return None if not found.
    fn get_optional(&self, name: &'static str, version: Version) -> Option<AnyApiHandle>;
    /// Clear and shut down the api registry. Currently not implemented.
    ///
    /// ## Note
    ///
    /// Do we really need it? All memory will be freed when the program exits.
    ///
    /// It will swap all internal ptr to null
    fn shutdown(&self);
    /// Add an interface with specific name and version.
    ///
    /// It may store multiple implementations of the same interface, but with different versions.
    /// When you set a new version of an interface, it will push the new version interface to all compatible <name,version> entries.
    /// eg. if you set a new version of an interface, and the old version is 1.2.3, and the new version is 1.2.4,
    /// then the compatible entries is `<name,1.2.3> -> Vec<[1.2.3 instance1, 1.2.3 instance2]>`, after set, the compatible entries will be
    /// `<name,1.2.3> -> Vec<[1.2.3 instance1, 1.2.3 instance2, 1.2.4 instance1]>`.
    /// Now the registry contains two associated entries:
    /// `<name,1.2.3> -> Vec<[1.2.3 instance1, 1.2.3 instance2, 1.2.4 instance1]>` and `<name,1.2.4> -> Vec<[1.2.4 instance1]>`.
    ///
    /// ## Example
    fn add_interface(
        &self,
        name: &'static str,
        version: Version,
        interface: AnyInterfaceHandle,
    ) -> AnyInterfaceHandle;
    /// Remove all interfaces with specific instance id from all compatible <name,version> entries.
    ///
    /// Different from [`ApiRegistryApi::remove`], it will remove the specific instance of interface from all
    /// compatible <name,version> entries.
    ///
    /// eg1. if you remove an `instance1` of interface with version 1.2.3, and the registry contains
    /// `<name,1.2.3> -> Vec<[1.2.3 instance1, 1.2.3 instance2, 1.2.4 instance1]>` and `<name,1.2.4> -> Vec<[1.2.4 instance1]>`,
    /// after remove, the registry will be `<name,1.2.3> -> Vec<[1.2.3 instance2]>` and `<name,1.2.4> -> Vec<[1.2.4 instance1]>`.
    ///
    /// eg2. if you remove an instance of `interface1` with version 1.2.4, and the registry contains
    /// `<name,1.2.3> -> Vec<[1.2.3 instance1, 1.2.3 instance2, 1.2.4 instance1]>` and `<name,1.2.4> -> Vec<[1.2.4 instance1]>`,
    /// after remove, the registry will be `<name,1.2.3> -> Vec<[1.2.3 instance1, 1.2.3 instance2, 1.2.4 instance1]>` and `<name,1.2.4> -> Vec<[]>`.
    ///
    /// ## Note
    ///
    /// Normally, your will remove interface when the plugin is unloaded(either reloading or shutting down). It's different from [`ApiRegistryApi::remove`],
    /// which is used to remove the specific version of an api.
    ///
    fn remove_interface(&self, name: &'static str, from_version: Version, instance_id: UniqueId);
    /// Get the number of interfaces with specific name and compatible version.
    ///
    /// ## Performance Note
    ///
    /// Currently, the implementation is count all compatible entries and return the total number of instances.
    /// So it's same as [`ApiRegistryApi::get_interfaces`].
    fn interface_count(&self, name: &'static str, version: Version) -> Option<usize>;
    /// Get all interfaces with specific name and compatible version.
    fn get_interfaces(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<Vec<AnyInterfaceHandle>>;
    /// Get the first interface with specific name and compatible version.
    fn first_interface(&self, name: &'static str, version: Version) -> Option<AnyInterfaceHandle>;
    /// Get interface with specific name and compatible version, check it's the only one.
    fn single_interface(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<(UniqueId, AnyInterfaceHandle)>;
    /// Get all available versions of an api.
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
#[define_api(ApiRegistryApi)]
struct ApiRegistry {
    /// Store api which is registered by plugins.
    ///
    /// 1 implementation per <api,version> pair
    ///
    /// ## TODO
    ///
    /// Change to lock free hash map, but i don't know whether the performance impact is huge or not.
    /// Because the size of this hashmap is about x00. Maybe a fixed size lock free hashmap is sufficient.
    pub inner_apis: RwLock<FxHashMap<VersionedName, ApiEntry>>,
    /// Store implementationss which is registered by plugins
    ///
    /// multiple implementations per <interface,version> pair
    ///
    /// ## TODO
    ///
    /// Same as [`inner_apis`], change to lock free hash map. But the size of this hashmap is expected to be larger than [`inner_apis`].
    pub inner_interfaces: RwLock<FxHashMap<VersionedName, Vec<InterfaceEntry>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VersionedName {
    pub name: &'static str,
    pub version: Version,
}

impl fmt::Display for VersionedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.name, self.version)
    }
}

impl VersionedName {
    pub fn new(name: &'static str, version: Version) -> Self {
        Self { name, version }
    }
}

pub fn get_api_registry_api() -> ApiHandle<dyn ApiRegistryApi> {
    ApiRegistry::builder().build()
}

/// The global api registry.
///
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

    pub fn set(
        &self,
        actual_name: &'static str,
        actual_version: Version,
        api: AnyApiHandle,
    ) -> AnyApiHandle {
        // check that name and version is match, if use fixed size lock free hashmap, may need another size check
        debug_assert!(actual_name == api.name(), "name mismatch");
        debug_assert!(actual_version == api.version(), "version mismatch");
        let id = VersionedName::new(actual_name, actual_version.clone());
        info!("Setting Api: actual {}", id);

        let mut w_lock = self.inner_apis.write().unwrap();

        // check that current hashmap has the api with compatible version?
        // the api in hashmap is compatible with the actual api version you passed in
        // eg1. actual_version is 1.2.3, and hashmap has 1.2.3, 1.2.4, 2.0.0
        // then the matches is [1.2.3].
        // eg2. actual_version is 1.2.5, and hashmap has 1.2.3, 1.2.4, 2.0.0
        // then the matches is [1.2.3, 1.2.4].
        // eg3. actual_version is 1.3.0, and hashmap has 1.2.3, 1.2.4
        // then the matches is [1.2.3,1.2.4].
        //
        // the rule is test in [`tests::test_set_version_matching`]
        let best_match: Vec<_> = w_lock
            .keys()
            .filter(|VersionedName { name, version }| {
                let version_cmper = semver::Comparator {
                    op: semver::Op::Caret,
                    major: version.major,
                    minor: Some(version.minor),
                    patch: Some(version.patch),
                    pre: version.pre.clone(),
                };
                version_cmper.matches(&actual_version) && *name == actual_name
            })
            .map(|x| x.clone())
            .collect();

        for match_id in best_match {
            if match_id == id {
                continue;
            }
            info!("Replace {} with actual {}", match_id, id);
            w_lock.entry(match_id.clone()).and_modify(|entry| {
                info!("Overwriting: {}", match_id);
                let arc = &entry.inner.0;
                arc.store(api.0.load().as_ref());
            });
        }

        // if don't have the api, create a new one inside registry,
        // if specific entry is already exist, update the api using atomic store
        let raw_entry = w_lock
            .entry(id.clone())
            .and_modify(|entry| {
                // TODO: if we don't use RWLock in ApiRegistry, we should use compare_exchange
                info!("Overwriting: {}", id);
                let arc = &entry.inner.0;
                arc.store(api.0.load().as_ref());
            })
            .or_insert(ApiEntry { inner: api });
        raw_entry.inner.clone()
    }

    pub fn add_interface(
        &self,
        actual_name: &'static str,
        actual_version: Version,
        interface: AnyInterfaceHandle,
    ) -> AnyInterfaceHandle {
        // check that name and version is match
        debug_assert!(actual_name == interface.name(), "name mismatch");
        debug_assert!(actual_version == interface.version(), "version mismatch");
        let id = VersionedName::new(actual_name, actual_version.clone());
        info!("Adding Interface: actual {}", id);

        let mut w_lock = self.inner_interfaces.write().unwrap();

        // Find compatible version entries
        let best_matches: Vec<_> = w_lock
            .keys()
            .filter(|VersionedName { name, version }| {
                let version_cmper = semver::Comparator {
                    op: semver::Op::Caret,
                    major: version.major,
                    minor: Some(version.minor),
                    patch: Some(version.patch),
                    pre: version.pre.clone(),
                };
                version_cmper.matches(&actual_version) && *name == actual_name
            })
            .map(|x| x.clone())
            .collect();

        // Add interface to all compatible version entries
        for match_id in best_matches {
            if match_id == id {
                continue;
            }
            info!(
                "Adding to compatible version entry: {} while adding actual {}",
                match_id, id
            );
            w_lock.entry(match_id).and_modify(|entries| {
                entries.push(InterfaceEntry {
                    inner: interface.clone(),
                });
            });
        }

        // Add to the actual version entry
        w_lock
            .entry(id)
            .and_modify(|entries| {
                entries.push(InterfaceEntry {
                    inner: interface.clone(),
                });
            })
            .or_insert_with(|| {
                vec![InterfaceEntry {
                    inner: interface.clone(),
                }]
            });

        interface
    }

    pub fn remove(&self, name: &'static str, version: Version) -> Option<AnyApiHandle> {
        // only remove the exact <name,version> pair
        let mut w_lock = self.inner_apis.write().unwrap();
        let id = VersionedName::new(name, version);
        info!("Removing Api: {}", id);
        let entry = w_lock.remove(&id).map(|entry| entry.inner);
        entry
    }

    pub fn remove_interface(
        &self,
        name: &'static str,
        from_version: Version,
        instance_id: UniqueId,
    ) {
        let mut w_lock = self.inner_interfaces.write().unwrap();
        let id = VersionedName::new(name, from_version.clone());
        info!("Removing Interface: {}", id);

        // Find all compatible version entries
        let compatible_versions: Vec<_> = w_lock
            .keys()
            .filter(
                |VersionedName {
                     name: entry_name,
                     version: entry_version,
                 }| {
                    let version_cmper = semver::Comparator {
                        op: semver::Op::Caret,
                        major: from_version.major,
                        minor: Some(from_version.minor),
                        patch: Some(from_version.patch),
                        pre: from_version.pre.clone(),
                    };
                    version_cmper.matches(entry_version) && *entry_name == name
                },
            )
            .map(|x| x.clone())
            .collect();

        // Remove the interface from all compatible version entries
        for compatible_id in compatible_versions {
            info!("Removing from compatible version entry: {}", compatible_id);
            w_lock.entry(compatible_id).and_modify(|entries| {
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
                    entries.remove(pos);
                }
            });
        }
    }

    pub fn get(&self, expected_name: &'static str, expected_version: Version) -> AnyApiHandle {
        let id = VersionedName::new(expected_name, expected_version.clone());
        info!("Getting Api: expecting {}", id);
        let r_lock = self.inner_apis.read().unwrap();

        // Find the best matching version
        let best_match = r_lock
            .keys()
            .filter(
                |VersionedName {
                     name,
                     version: actual_version,
                     ..
                 }| {
                    let version_cmper = semver::Comparator {
                        op: semver::Op::Caret,
                        major: expected_version.major,
                        minor: Some(expected_version.minor),
                        patch: Some(expected_version.patch),
                        pre: expected_version.pre.clone(),
                    };
                    version_cmper.matches(&actual_version) && *name == expected_name
                },
            )
            .max_by(|a, b| a.version.cmp(&b.version));

        if let Some(best_match) = best_match {
            if let Some(raw_entry) = r_lock.get(&best_match) {
                info!("Found {} while expecting {}", best_match, id);
                return raw_entry.inner.clone();
            }
        }

        // If no match found, create a new entry
        drop(r_lock);
        info!("Not found expecting {}, creating new entry for it", id);
        let mut w_lock = self.inner_apis.write().unwrap();

        let raw_entry = w_lock.entry(id).or_insert_with(|| {
            let new_arc_to_null = Arc::new(AtomicArc::new(None));
            ApiEntry {
                inner: AnyApiHandle(new_arc_to_null),
            }
        });
        raw_entry.inner.clone()
    }

    fn get_optional(&self, name: &'static str, version: Version) -> Option<AnyApiHandle> {
        let id = VersionedName::new(name, version.clone());
        info!("Getting Optional Api: expecting {}", id);
        let r_lock = self.inner_apis.read().unwrap();

        // Find the best matching version
        let best_match = r_lock
            .keys()
            .filter(
                |VersionedName {
                     name: entry_name,
                     version: actual_version,
                 }| {
                    let version_cmper = semver::Comparator {
                        op: semver::Op::Caret,
                        major: version.major,
                        minor: Some(version.minor),
                        patch: Some(version.patch),
                        pre: version.pre.clone(),
                    };
                    version_cmper.matches(&actual_version) && *entry_name == name
                },
            )
            .max_by(|a, b| a.version.cmp(&b.version));

        // Return the entry if found, otherwise None
        if let Some(best_match) = best_match {
            if let Some(raw_entry) = r_lock.get(&best_match) {
                info!("Found {} while expecting {}", best_match, id);
                return Some(raw_entry.inner.clone());
            }
        }

        info!("Not found expecting {}", id);
        None
    }

    pub fn interface_count(&self, name: &'static str, version: Version) -> Option<usize> {
        self.get_interfaces(name, version).map(|v| v.len())
    }

    pub fn get_interfaces(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<Vec<AnyInterfaceHandle>> {
        let id = VersionedName::new(name, version.clone());
        info!("Getting Interfaces: expecting {}", id);
        let r_lock = self.inner_interfaces.read().unwrap();

        // Find all compatible version entries
        let compatible_entries: Vec<_> = r_lock
            .iter()
            .filter(
                |(
                    VersionedName {
                        name: entry_name,
                        version: entry_version,
                    },
                    _,
                )| {
                    let version_cmper = semver::Comparator {
                        op: semver::Op::Caret,
                        major: version.major,
                        minor: Some(version.minor),
                        patch: Some(version.patch),
                        pre: version.pre.clone(),
                    };
                    version_cmper.matches(entry_version) && *entry_name == name
                },
            )
            .flat_map(|(_, entries)| entries.iter().map(|e| e.inner.clone()))
            .collect();

        if compatible_entries.is_empty() {
            info!("Not found expecting interfaces {}", id);
            return None;
        }

        // Remove duplicates by instance ID
        let mut unique_entries = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for handle in compatible_entries {
            if let Some(local_handle) = handle.clone().downcast::<dyn Interface>().get() {
                let instance_id = local_handle.id();
                if seen_ids.insert(instance_id) {
                    unique_entries.push(handle);
                }
            }
        }

        info!("Found {} {} interfaces", unique_entries.len(), id);
        Some(unique_entries)
    }

    pub fn first_interface(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<AnyInterfaceHandle> {
        let id = VersionedName::new(name, version.clone());
        info!("Getting First Interface: expecting {}", id);
        let r_lock = self.inner_interfaces.read().unwrap();

        // Find all compatible version entries and sort by version (highest first)
        r_lock
            .iter()
            .filter(
                |(
                    VersionedName {
                        name: entry_name,
                        version: entry_version,
                    },
                    _,
                )| {
                    let version_cmper = semver::Comparator {
                        op: semver::Op::Caret,
                        major: version.major,
                        minor: Some(version.minor),
                        patch: Some(version.patch),
                        pre: version.pre.clone(),
                    };
                    version_cmper.matches(entry_version) && *entry_name == name
                },
            )
            .max_by(|a, b| a.0.version.cmp(&b.0.version)) // Get entry with highest compatible version
            .and_then(|(_, entries)| entries.first()) // Get first interface from that version
            .map(|entry| entry.inner.clone())
    }

    pub fn single_interface(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<(UniqueId, AnyInterfaceHandle)> {
        let id = VersionedName::new(name, version.clone());
        info!("Getting Single Interface: expecting {}", id);
        let r_lock = self.inner_interfaces.read().unwrap();

        // Find all compatible version entries
        let compatible_entries: Vec<_> = r_lock
            .iter()
            .filter(
                |(
                    VersionedName {
                        name: entry_name,
                        version: entry_version,
                    },
                    _,
                )| {
                    let version_cmper = semver::Comparator {
                        op: semver::Op::Caret,
                        major: version.major,
                        minor: Some(version.minor),
                        patch: Some(version.patch),
                        pre: version.pre.clone(),
                    };
                    version_cmper.matches(entry_version) && *entry_name == name
                },
            )
            .flat_map(|(_, entries)| entries.iter().map(|e| e.inner.clone()))
            .collect();

        if compatible_entries.is_empty() {
            info!("Not found expecting interfaces {}", id);
            return None;
        }

        // Track unique instances by ID
        let mut unique_entry = None;
        let mut seen_ids = std::collections::HashSet::new();

        for handle in compatible_entries {
            if let Some(local_handle) = handle.clone().downcast::<dyn Interface>().get() {
                let instance_id = local_handle.id();
                if !seen_ids.insert(instance_id) {
                    // Found duplicate ID
                    continue;
                }

                if unique_entry.is_some() {
                    info!("Found more than one unique implementation {}", id);
                    // Found more than one unique implementation
                    return None;
                }

                unique_entry = Some((instance_id, handle));
            }
        }

        unique_entry
    }

    pub fn available_api_versions(&self, name: &'static str) -> Vec<Version> {
        // return the specific api loaded versions
        let mut versions = Vec::new();
        let r_lock = self.inner_apis.read().unwrap();
        for (k, _) in r_lock.iter() {
            if k.name == name.to_string() {
                versions.push(k.version.clone());
            }
        }
        versions
    }

    fn shutdown(&self) {
        // for all entry in the hash map, drop the api
        self.inner_apis.write().unwrap().clear();
        self.inner_interfaces.write().unwrap().clear();
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

    fn get_optional(&self, name: &'static str, version: Version) -> Option<AnyApiHandle> {
        self.get_optional(name, version)
    }

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

    fn single_interface(
        &self,
        name: &'static str,
        version: Version,
    ) -> Option<(UniqueId, AnyInterfaceHandle)> {
        self.single_interface(name, version)
    }

    fn available_api_versions(&self, name: &'static str) -> Vec<Version> {
        self.available_api_versions(name)
    }

    fn remove_interface(&self, name: &'static str, from_version: Version, instance_id: UniqueId) {
        self.remove_interface(name, from_version, instance_id)
    }
}

/// A typed-decorated api for LocalApiHandle
impl<'local> LocalApiHandle<'local, dyn ApiRegistryApi> {
    pub fn local_set<T: ApiConstant + Api + ?Sized>(&self, api: ApiHandle<T>) -> ApiHandle<T> {
        self.deref().set(T::NAME, T::VERSION, api.inner).downcast()
    }

    pub fn local_remove<T: ApiConstant + Api + ?Sized>(&self) -> Option<ApiHandle<T>> {
        self.deref()
            .remove(T::NAME, T::VERSION)
            .map(|api| api.downcast())
    }

    pub fn local_find<T: ApiConstant + Api + ?Sized>(&self) -> ApiHandle<T> {
        self.deref().find(T::NAME, T::VERSION).downcast()
    }

    pub fn local_shutdown(&self) {
        self.deref().shutdown()
    }

    pub fn local_add_interface<T: InterfaceConstant + Interface + ?Sized>(
        &self,
        interface: InterfaceHandle<T>,
    ) -> InterfaceHandle<T> {
        self.deref()
            .add_interface(T::NAME, T::VERSION, interface.inner)
            .downcast()
    }

    pub fn local_remove_interface<T: InterfaceConstant + Interface + ?Sized>(
        &self,
        from_version: Version,
        instance_id: UniqueId,
    ) {
        self.deref()
            .remove_interface(T::NAME, from_version, instance_id)
    }

    pub fn local_interface_count<T: InterfaceConstant + Interface + ?Sized>(
        &self,
    ) -> Option<usize> {
        self.deref().interface_count(T::NAME, T::VERSION)
    }

    pub fn local_get_interfaces<T: InterfaceConstant + Interface + ?Sized>(
        &self,
    ) -> Option<Vec<InterfaceHandle<T>>> {
        self.deref()
            .get_interfaces(T::NAME, T::VERSION)
            .map(|handles| handles.into_iter().map(|h| h.downcast()).collect())
    }

    pub fn local_first_interface<T: InterfaceConstant + Interface + ?Sized>(
        &self,
    ) -> Option<InterfaceHandle<T>> {
        self.deref()
            .first_interface(T::NAME, T::VERSION)
            .map(|h| h.downcast())
    }

    pub fn local_available_api_versions<T: InterfaceConstant + Interface + ?Sized>(
        &self,
    ) -> Vec<Version> {
        self.deref().available_api_versions(T::NAME)
    }
}

/// A weak plugin(or future) global type-aware handle to an API.
///
/// It's used to be stored inside api struct, avoid circular reference.
pub struct WeakApiHandle<T: 'static + ?Sized + Api> {
    inner: WeakAnyApiHandle,
    _phantom_type: marker::PhantomData<WeakHandleInternal<T>>,
}

impl<T: 'static + ?Sized + Api> WeakApiHandle<T> {
    /// Attempts to upgrade the weak handle to an ApiHandle.
    /// Returns None if the original ApiHandle has been dropped.
    pub fn upgrade(&self) -> Option<ApiHandle<T>> {
        self.inner.upgrade().map(|handle| ApiHandle {
            inner: handle,
            _phantom_type: marker::PhantomData,
        })
    }
}

impl<T: 'static + ?Sized + Api> Clone for WeakApiHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom_type: marker::PhantomData,
        }
    }
}

/// Opaque weak api handle
pub struct WeakAnyApiHandle(WeakHandleInternal<dyn TraitcastableAny + Sync + Send>);

impl WeakAnyApiHandle {
    /// Attempts to upgrade to an AnyApiHandle
    pub fn upgrade(&self) -> Option<AnyApiHandle> {
        self.0.upgrade().map(AnyApiHandle)
    }
}

impl Clone for WeakAnyApiHandle {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: 'static + ?Sized + Api> ApiHandle<T> {
    /// Creates a new weak handle to this API
    pub fn downgrade(&self) -> WeakApiHandle<T> {
        WeakApiHandle {
            inner: self.inner.downgrade(),
            _phantom_type: marker::PhantomData,
        }
    }
}

impl AnyApiHandle {
    /// Creates a new weak handle to this API
    pub fn downgrade(&self) -> WeakAnyApiHandle {
        WeakAnyApiHandle(Arc::downgrade(&self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_matching_versions(
        actual_version: Version,
        available_versions: Vec<Version>,
    ) -> Vec<Version> {
        let actual_name = "test_api";
        let versioned_names: FxHashMap<VersionedName, ()> = available_versions
            .into_iter()
            .map(|v| (VersionedName::new(actual_name, v), ()))
            .collect();

        versioned_names
            .keys()
            .filter(|VersionedName { name, version }| {
                let version_cmper = semver::Comparator {
                    op: semver::Op::Caret,
                    major: version.major,
                    minor: Some(version.minor),
                    patch: Some(version.patch),
                    pre: version.pre.clone(),
                };
                version_cmper.matches(&actual_version) && *name == actual_name
            })
            .map(|v| v.version.clone())
            .collect()
    }

    /// Test examples from [`ApiRegistry::set`]
    #[test]
    fn test_set_version_matching() {
        // Example 1: actual_version is 1.2.3, and hashmap has 1.2.3, 1.2.4, 2.0.0
        // then the matches is [1.2.3]
        assert_eq!(
            find_matching_versions(
                Version::new(1, 2, 3),
                vec![
                    Version::new(1, 2, 3),
                    Version::new(1, 2, 4),
                    Version::new(2, 0, 0),
                ]
            ),
            vec![Version::new(1, 2, 3)]
        );

        // Example 2: actual_version is 1.2.5, and hashmap has 1.2.3, 1.2.4, 2.0.0
        // then the matches is [1.2.3, 1.2.4]
        assert_eq!(
            find_matching_versions(
                Version::new(1, 2, 5),
                vec![
                    Version::new(1, 2, 3),
                    Version::new(1, 2, 4),
                    Version::new(2, 0, 0),
                ]
            )
            .sort(),
            vec![Version::new(1, 2, 3), Version::new(1, 2, 4)].sort()
        );

        // Example 3: actual_version is 1.3.0, and hashmap has 1.2.3, 1.2.4
        // then the matches is [1.2.3, 1.2.4]
        assert_eq!(
            find_matching_versions(
                Version::new(1, 3, 0),
                vec![Version::new(1, 2, 3), Version::new(1, 2, 4),]
            )
            .sort(),
            vec![Version::new(1, 2, 3), Version::new(1, 2, 4)].sort()
        );

        // Additional test cases
        // Major version mismatch
        assert_eq!(
            find_matching_versions(
                Version::new(2, 0, 0),
                vec![Version::new(1, 2, 3), Version::new(1, 2, 4),]
            ),
            Vec::<Version>::new()
        );

        // Pre-release versions
        assert_eq!(
            find_matching_versions(
                Version::new(1, 2, 0),
                vec![
                    Version::parse("1.2.0-alpha").unwrap(),
                    Version::parse("1.2.0-beta").unwrap(),
                    Version::new(1, 2, 0),
                ]
            )
            .sort(),
            vec![
                Version::parse("1.2.0-alpha").unwrap(),
                Version::parse("1.2.0-beta").unwrap(),
                Version::new(1, 2, 0),
            ]
            .sort()
        );
    }
}

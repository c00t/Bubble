pub use bubble_macros::{declare_api, declare_interface, define_api, define_interface};
pub use semver::Version;
use std::any::Any;
pub use trait_cast_rs::{
    make_trait_castable, make_trait_castable_decl, unique_id, TraitcastTarget, TraitcastableAny,
    TraitcastableAnyInfra, TraitcastableAnyInfraExt, TraitcastableTo, UniqueId, UniqueTypeId,
};

mod interfaces;

/// Api is used to exposed a collections of functions.
///
/// Because that an api or the api struct implementing a specific api trait will be accessed by
/// multiple plugins, which can use them in a concurrent way, the api struct must be thread-safe.
/// That's why the api struct must be `Sync`. Normally, you should use them as if it's a `&T`
/// and implement your api struct using internal mutability if needed.
///
/// When you define an api in your plugin, and want other plugins to use it, you should never export
/// the struct that implements the api directly. Instead, you should export the specific api trait.
/// Typically, you can define your api trait in a separate `some_plugin_api_types` crate,
/// export the trait and define you api struct in another crate. Thus, other plugins can depends
/// on your `some_plugin_api_types` crate and use the api trait but use the api implementation
/// which has been registered into api registry, instead of generating a new instance of the api
/// struct.
///
/// Also, if your plugin's api is intended to be used by other plugins in concurrent context,
/// you should consider your usage scenario and use proper synchronization primitives. Through in
/// Bubble, your api can register statics or thread local data from plugin using [`crate::os::thread::dyntls::thread_local`]
/// or [`crate::os::thread::dyntls::lazy_static`], you should avoid use them in your api, it will decrease your performance if you use them heavily.
/// Especially, if you use a library which use statics or thread local data to sync between plugins, you need to vendor them,
/// and use [`dyntls::thread_local`] or [`dyntls::lazy_static`] instead.
///
/// When your api need to clear some resources when the application shutdown, because the api will
/// hold a static reference to your api, you should implement a `shutdown` function
/// instead of [`Drop`], it will be called when the application shutdown. It won't be included in
/// the [`Api`] trait.
pub trait Api: Any + Sync + Send {
    /// The identifier name of the api.
    ///
    /// Typically, it's the full name of the api including the module hierarchy.
    fn name(&self) -> &'static str;
    /// The version of the api.
    ///
    /// ## Note
    ///
    /// When you declare an api using [`declare_api`], it will generate a version constant for you in a corresponding module.
    /// It defines the version others will use in their plugin. But the [`Api::version`] function is used to get the api version of
    /// the trait object you get from the api registry.
    fn version(&self) -> Version;
}

impl<T: Api + ?Sized> Api for Box<T> {
    fn name(&self) -> &'static str {
        T::name(&self)
    }

    fn version(&self) -> Version {
        T::version(&self)
    }
}

// We currently don't add version to [`Api`], it's the super trait for all api traits.
unique_id! {
    dyn bubble_core::api::api_registry_api::Api;
}

/// Implementation is used to define a function which can be implemented by a plugin.
///
/// The difference between [`Api`] and [`Interface`] is that [`Api`] is defined only once,
/// but [`Interface`] can be implemented by multiple plugins.
pub trait Interface: Any + Sync + Send {
    /// The identifier name of the interface.
    ///
    /// Typically, it's the full name of the interface including the module hierarchy.
    fn name(&self) -> &'static str;
    /// The version of the interface.
    /// 
    /// ## Note
    ///
    /// When you declare an interface using [`declare_interface`], it will generate a version constant for you in a corresponding module.
    /// It defines the version others will use in their plugin. But the [`Interface::version`] function is used to get the interface version of
    /// the trait object you get from the api registry.
    fn version(&self) -> Version;
    /// The unique id of the interface trait object.
    ///
    /// It's used to identify the instances of the interface trait object. Because one interface trait can be implemented by multiple plugins,
    /// we need to identify them by a unique id.
    fn id(&self) -> UniqueId;
}

impl<T: Interface + ?Sized> Interface for Box<T> {
    fn name(&self) -> &'static str {
        T::name(&self)
    }

    fn version(&self) -> Version {
        T::version(&self)
    }

    fn id(&self) -> UniqueId {
        T::id(&self)
    }
}

unique_id! {
    dyn bubble_core::api::api_registry_api::Interface;
}

/// A constant trait for api.
///
/// It's used to declare the name and version of the api in a corresponding module.
/// Because the api trait should be object safe, we can't declare it as super or sub trait of [`Api`].
pub trait ApiConstant {
    const NAME: &'static str;
    const VERSION: Version;
}

/// A constant trait for interface.
///
/// It's used to declare the name and version of the interface in a corresponding module.
/// Because the interface trait should be object safe, we can't declare it as super or sub trait of [`Interface`].
pub trait InterfaceConstant {
    const NAME: &'static str;
    const VERSION: Version;
}

pub mod api_registry_api;
pub mod bump_allocator_api;
pub mod plugin_api;

/// A prelude for users to define or declare their own api and interface.
pub mod prelude {
    pub use super::api_registry_api::{
        AnyApiHandle, AnyInterfaceHandle, ApiHandle, ApiRegistryApi, InterfaceHandle,
        LocalApiHandle, LocalInterfaceHandle,
    };
    pub use super::{
        declare_api, declare_interface, define_api, define_interface, make_trait_castable,
        make_trait_castable_decl, unique_id, Api, ApiConstant, Interface, InterfaceConstant,
        TraitcastTarget, TraitcastableAny, TraitcastableAnyInfra, TraitcastableAnyInfraExt,
        TraitcastableTo, UniqueId, UniqueTypeId, Version,
    };
    pub use crate::bon::{bon, builder};
}

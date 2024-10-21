pub use bubble_macros::{
    define_api, define_api_with_id, define_interface, define_interface_with_id,
};
pub use semver::Version;
use std::any::Any;
pub use trait_cast_rs::{
    make_trait_castable, make_trait_castable_decl, unique_id, TraitcastTarget, TraitcastableAny,
    TraitcastableAnyInfra, TraitcastableAnyInfraExt, TraitcastableTo, UniqueId, UniqueTypeId,
};

/// Api is used to exposed a collections of functions.
///
/// Because that an api or the api struct implementing a specific api trait will be accessed by
/// multiple plugins, which can use them in a concurrent way, the api struct must be thread-safe.
/// That's why the api struct must be `Sync`. Normally, you should use them as if it's a `&T`
/// and implement your api struct using internal mutability.
///
/// When you define an api in your plugin, and want other plugins to use it, you should never export
/// the struct that implements the api directly. Instead, you should export the specific api trait. Thus,
/// other plugins can depends on your crate and use the api trait but use the api implementation which has been
/// registered into api registry, instead of generating a new instance of the api struct.
///
/// Also, if your plugin's api is intended to be used by other plugins in concurrent context,
/// you should consider your usage scenario and use proper synchronization primitives. Through in
/// Bubble, your api can register statics or thread local data from plugin, you should avoid use them
/// in your api, it will decrease your performance if you use them heavily. Especially,
/// if you use a library which use statics or thread local data, you need to vendor them,
/// and use [`dyntls::thread_local`] or [`dyntls::lazy_static`] instead.
///
/// When your api need to clear some resources when the application shutdown, because the api will
/// hold a static reference to your api, you should implement the [`Api::shutdown`] function
/// instead of [`Drop`], it will be called when the application shutdown.
pub trait Api: Any + Sync + Send {
    // const NAME: &'static str;
    // const VERSION: Version;
    fn name(&self) -> &'static str;
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

unique_id! {
    dyn bubble_core::api::api_registry_api::Api;
}

/// Implementation is used to define a function which can be implemented by a plugin.
///
/// The difference between [`Api`] and [`Interface`] is that [`Api`] is defined only once,
/// but [`Interface`] can be implemented by multiple plugins.
pub trait Interface: Any + Sync + Send {
    fn name(&self) -> &'static str;
    fn version(&self) -> Version;
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

pub trait ApiConstant {
    const NAME: &'static str;
    const VERSION: Version;
}

pub trait InterfaceConstant {
    const NAME: &'static str;
    const VERSION: Version;
    const ID: UniqueId;
}

/// Implement [`Api`] trait for a api data bundle struct.
///
/// ## Example
///
/// ```
/// use bubble_core::impl_api;
///
/// pub struct MyApiData {}
///
/// pub trait MyApi {}
///
/// impl_api!(MyApiData, MyApi, (1, 0, 0));
/// ```
#[macro_export]
macro_rules! impl_api {
    ($struct_name:ident, $api_name:ident, ($major:expr, $minor:expr,$patch:expr)) => {
        pub mod constants {
            use super::Version;
            pub const NAME: &'static str = ::std::stringify!($api_name);
            pub const VERSION: self::Version = self::Version::new($major, $minor, $patch);
        }

        impl self::ApiConstant for $struct_name {
            const NAME: &'static str = self::constants::NAME;
            const VERSION: self::Version = self::constants::VERSION;
        }

        impl self::Api for $struct_name {
            fn name(&self) -> &'static str {
                <Self as self::ApiConstant>::NAME
            }
            fn version(&self) -> self::Version {
                <Self as self::ApiConstant>::VERSION
            }
        }
    };
}

#[macro_export]
macro_rules! impl_interface {
    ($struct_name:ident, $api_name:ident, ($major:expr, $minor:expr,$patch:expr)) => {
        pub mod constants {
            use super::$struct_name;
            use super::UniqueId;
            use super::UniqueTypeId;
            use super::Version;
            pub const NAME: &'static str = ::std::stringify!($api_name);
            pub const VERSION: self::Version = self::Version::new($major, $minor, $patch);
            pub const INTERFACE_ID: UniqueId = $struct_name::TYPE_ID;
        }

        impl self::InterfaceConstant for $struct_name {
            const NAME: &'static str = self::constants::NAME;
            const VERSION: self::Version = self::constants::VERSION;
            const ID: UniqueId = self::constants::INTERFACE_ID;
        }

        impl self::Interface for $struct_name {
            fn name(&self) -> &'static str {
                <Self as self::InterfaceConstant>::NAME
            }
            fn version(&self) -> self::Version {
                <Self as self::InterfaceConstant>::VERSION
            }
            fn id(&self) -> UniqueId {
                <Self as self::InterfaceConstant>::ID
            }
        }
    };
}

pub mod api_registry_api;
pub mod bump_allocator_api;

pub mod prelude {
    pub use super::api_registry_api::{
        AnyApiHandle, AnyInterfaceHandle, ApiHandle, ApiRegistryApi, InterfaceHandle,
        LocalApiHandle, LocalInterfaceHandle,
    };
    pub use super::{
        define_api, define_api_with_id, define_interface, define_interface_with_id,
        make_trait_castable, make_trait_castable_decl, unique_id, Api, ApiConstant, Interface,
        InterfaceConstant, TraitcastTarget, TraitcastableAny, TraitcastableAnyInfra,
        TraitcastableAnyInfraExt, TraitcastableTo, UniqueId, UniqueTypeId, Version,
    };
    pub use crate::impl_api;
    pub use crate::impl_interface;
}

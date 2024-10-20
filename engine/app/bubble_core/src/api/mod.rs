pub use bubble_macros::define_api;
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
pub trait Implementation {
    fn name(&self) -> &'static str;
    fn version(&self) -> Version;
}

pub trait ApiConstant {
    const NAME: &'static str;
    const VERSION: Version;
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

pub mod api_registry_api;

pub mod prelude {
    pub use super::api_registry_api::{AnyApiHandle, ApiHandle, ApiRegistryApi, LocalApiHandle};
    pub use super::{
        define_api, make_trait_castable, make_trait_castable_decl, unique_id, Api, ApiConstant,
        Implementation, TraitcastTarget, TraitcastableAny, TraitcastableAnyInfra,
        TraitcastableAnyInfraExt, TraitcastableTo, UniqueId, UniqueTypeId, Version,
    };
    pub use crate::impl_api;
}

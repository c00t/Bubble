pub use semver::Version;
use std::any::Any;
pub use trait_cast_rs::{
    make_trait_castable, make_trait_castable_decl, unique_id, TraitcastTarget, TraitcastableAny,
    TraitcastableAnyInfra, TraitcastableAnyInfraExt, TraitcastableTo, UniqueId, UniqueTypeId,
};

/// Api is used to exposed a collections of functions.
pub trait Api: Any + Sync {
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
            pub const NAME: &'static str = ::std::stringify!($api_name);
            pub const VERSION: $crate::api::Version =
                $crate::api::Version::new($major, $minor, $patch);
        }

        impl $crate::api::ApiConstant for $struct_name {
            const NAME: &'static str = self::constants::NAME;
            const VERSION: $crate::api::Version = self::constants::VERSION;
        }

        impl $crate::api::Api for $struct_name {
            fn name(&self) -> &'static str {
                <Self as crate::api::ApiConstant>::NAME
            }
            fn version(&self) -> $crate::api::Version {
                <Self as crate::api::ApiConstant>::VERSION
            }
        }
    };
}

pub mod api_registry_api;

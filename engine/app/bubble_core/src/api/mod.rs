use crate::sync::{Arc, ArcAtomicArcErased, AtomicArc, Guard};
use rustc_hash::FxHashMap;
pub use semver::Version;
use std::{
    any::{type_name, type_name_of_val, Any, TypeId},
    ops::Deref,
    sync::RwLock,
};
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

pub trait ApiConstant {
    const NAME: &'static str;
    const VERSION: Version;
}

impl<T: Api + ?Sized> Api for Box<T> {
    fn name(&self) -> &'static str {
        T::name(&self)
    }

    fn version(&self) -> Version {
        T::version(&self)
    }
}

// impl_downcast!(Api);

/// Implementation is used to define a function which can be implemented by a plugin.
pub trait Implementation {
    fn name(&self) -> &'static str;
    fn version(&self) -> Version;
}

pub mod api_registry_api {
    use std::{
        any::{type_name, type_name_of_val},
        ops::Deref,
        sync::RwLock,
    };

    use crate::api::{
        make_trait_castable, make_trait_castable_decl, unique_id, TraitcastableAny,
        TraitcastableAnyInfra, TraitcastableAnyInfraExt, UniqueId, UniqueTypeId,
    };
    use rustc_hash::FxHashMap;
    use semver::Version;

    use super::Api;
    use crate::sync::{Arc, ArcAtomicArcErased, AtomicArc, Guard};
    pub struct ApiHandle<T: 'static>(Arc<AtomicArc<T>>);

    unsafe impl<T: 'static> Send for ApiHandle<T> {}
    unsafe impl<T: 'static> Sync for ApiHandle<T> {}

    impl<T> ApiHandle<T> {
        pub fn get(&self) -> Option<Guard<T>> {
            self.0.load()
        }
    }

    pub trait ApiRegistryApi: Api {
        fn set(&self, api: Box<dyn TraitcastableAny>);
        fn remove(&self, name: &'static str, version: Version);
        fn get(&self, name: &'static str, version: Version)
            -> ApiHandle<Box<dyn TraitcastableAny>>;
        fn ref_counts(&self);
    }

    struct ApiEntry {
        /// Arc<AtomicArc<T>> erased to ArcAtomicArcErased
        /// It's `Arc<AtomicArc<dyn TraitcastableAny>>`.
        inner: Arc<AtomicArc<Box<dyn TraitcastableAny>>>,
    }

    unsafe impl Send for ApiEntry {}
    unsafe impl Sync for ApiEntry {}

    /// An api registry that can be used to register api implementations.
    pub struct ApiRegistry {
        // a hash map that maps api types to their implementations
        apis: RwLock<FxHashMap<(&'static str, Version), ApiEntry>>,
    }

    crate::impl_api!(ApiRegistryRef, ApiRegistryApi, (0, 1, 0));

    #[make_trait_castable(Api, ApiRegistryApi)]
    pub struct ApiRegistryRef {
        pub inner: &'static ApiRegistry,
    }

    unique_id! {
        dyn Api;
        dyn ApiRegistryApi
    }

    impl ApiRegistry {
        pub fn new() -> Self {
            Self {
                apis: RwLock::new(FxHashMap::default()),
            }
        }
        pub fn set(&self, api: Box<dyn TraitcastableAny>) {
            let as_api: &dyn Api = api.downcast_ref().unwrap();
            let id = (as_api.name(), as_api.version());
            println!("{id:?}");
            println!("{:?}", type_name_of_val(&api));
            println!("any id:{:?}", TraitcastableAny::type_id(api.as_ref()));
            // get a ApiEntry first to transform into erased ptr
            let atomic_arc = Arc::new(AtomicArc::new(api));
            let entry = ApiEntry { inner: atomic_arc };
            self.apis.write().unwrap().insert(id, entry);
        }

        pub fn remove(&self, name: &str, version: Version) {
            todo!()
        }

        pub fn get(
            &self,
            name: &'static str,
            version: Version,
        ) -> ApiHandle<Box<dyn TraitcastableAny>> {
            // get the raw entry
            let r_lock = self.apis.read().unwrap();
            let id = (name, version.clone());
            println!("{id:?}");
            if let Some(raw_entry) = r_lock.get(&id) {
                println!(
                    "any id:{:?}",
                    TraitcastableAny::type_id(raw_entry.inner.load().unwrap().as_ref())
                );
                ApiHandle(raw_entry.inner.clone())
            } else {
                drop(r_lock);
                let mut w_lock = self.apis.write().unwrap();
                // if don't have the api, create a new one inside registry
                let raw_entry = w_lock.entry((name, version)).or_insert({
                    let x = Arc::new(AtomicArc::new(None));
                    ApiEntry { inner: x }
                });
                ApiHandle(raw_entry.inner.clone())
            }
        }

        pub fn ref_counts(&self) {
            // loop through the hash map and print the ref counts
        }
    }

    pub trait ApiConstant {
        const NAME: &'static str;
        const VERSION: Version;
    }

    #[macro_export]
    macro_rules! impl_api {
        ($struct_name:ident, $api_name:ident, ($major:expr, $minor:expr,$patch:expr)) => {
            impl $crate::api::api_registry_api::ApiConstant for $struct_name {
                const NAME: &'static str = ::std::stringify!($api_name);
                const VERSION: $crate::api::Version =
                    $crate::api::Version::new($major, $minor, $patch);
            }

            impl $crate::api::Api for $struct_name {
                fn name(&self) -> &'static str {
                    Self::NAME
                }
                fn version(&self) -> $crate::api::Version {
                    Self::VERSION
                }
            }
        };
    }

    impl ApiRegistryApi for ApiRegistryRef
    where
        Self: 'static,
    {
        fn set(&self, api: Box<dyn TraitcastableAny>) {
            self.inner.set(api);
        }

        fn remove(&self, name: &'static str, version: Version) {
            todo!()
        }

        fn get(
            &self,
            name: &'static str,
            version: Version,
        ) -> ApiHandle<Box<dyn TraitcastableAny>> {
            self.inner.get(name, version)
        }

        fn ref_counts(&self) {}
    }
}

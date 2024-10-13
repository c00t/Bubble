use downcast_rs::{impl_downcast, Downcast};
use rustc_hash::FxHashMap;
use std::{
    any::Any,
    sync::{Arc, RwLock},
};

/// Api is used to exposed a collections of functions.
pub trait Api {
    fn name(&self) -> &'static str;
    fn version(&self) -> crate::SemVer;
}
// impl_downcast!(Api);

/// Implementation is used to define a function which can be implemented by a plugin.
pub trait Implementation {
    fn name(&self) -> &'static str;
    fn version(&self) -> crate::SemVer;
}

pub trait ApiRegistryApi: Api {
    fn add<T: Send + Api + 'static>(&self, api_impl: T);
}

/// An api registry that can be used to register api implementations.
pub struct ApiRegistry {
    // a hash map that maps api types to their implementations
    apis: RwLock<FxHashMap<&'static str, Arc<dyn Api + Send>>>,
}

impl Api for ApiRegistry {
    fn name(&self) -> &'static str {
        "ApiRegistry"
    }
    fn version(&self) -> crate::SemVer {
        crate::SemVer {
            major: 0,
            minor: 1,
            patch: 0,
        }
    }
}

impl ApiRegistryApi for ApiRegistry {
    fn add<T: Send + Api + 'static>(&self, api_impl: T) {
        let mut apis = self.apis.write().unwrap();
        apis.insert(T::name(&api_impl), Arc::new(api_impl));
    }
}

impl ApiRegistry {
    pub fn new() -> Self {
        Self {
            apis: RwLock::new(FxHashMap::default()),
        }
    }

    // The api implementation should be cloneable.
    // pub fn get<T: Any + Copy>(&self) -> Option<T> {
    //     // 1. get the type id of the api type
    //     // 2. lock the hash map
    //     // 3. get the api implementation from the hash map
    //     // 4. downcast the api implementation to the api type
    //     // 5. return the api implementation
    //     let type_name = type_name::<T>();
    //     let apis = self.apis.read().unwrap();
    //     let any = apis.get(type_name)?;
    //     // use unsafe method to convert to T
    //     let api = unsafe { *(any.as_ref() as *const _ as *const T) };
    //     Some(api)
    // }
}

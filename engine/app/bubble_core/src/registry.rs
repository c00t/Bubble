use rustc_hash::FxHashMap;
use std::{
    any::{type_name, Any},
    sync::RwLock,
};

/// An api registry that can be used to register api implementations.
pub struct ApiRegistry {
    // a hash map that maps api types to their implementations
    apis: RwLock<FxHashMap<&'static str, Box<dyn Any + Send>>>,
}

impl ApiRegistry {
    pub fn new() -> Self {
        Self {
            apis: RwLock::new(FxHashMap::default()),
        }
    }

    pub fn add<T: Any + Send>(&self, api_impl: T) {
        // 1. get the type id of the api type
        // 2. lock the hash map
        // 3. insert the api implementation into the hash map
        let type_name = type_name::<T>();
        println!("adding api: {}", type_name);
        let mut apis = self.apis.write().unwrap();
        apis.insert(type_name, Box::new(api_impl));
    }

    /// The api implementation should be cloneable.
    pub fn get<T: Any + Copy>(&self) -> Option<T> {
        // 1. get the type id of the api type
        // 2. lock the hash map
        // 3. get the api implementation from the hash map
        // 4. downcast the api implementation to the api type
        // 5. return the api implementation
        let type_name = type_name::<T>();
        let apis = self.apis.read().unwrap();
        let any = apis.get(type_name)?;
        // use unsafe method to convert to T
        let api = unsafe { *(any.as_ref() as *const _ as *const T) };
        Some(api)
    }
}

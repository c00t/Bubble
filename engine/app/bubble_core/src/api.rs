use crate::sync::{Arc, ArcAtomicArcErased, AtomicArc, Guard};
use rustc_hash::FxHashMap;
pub use semver::Version;
use std::{
    any::{type_name, type_name_of_val, TypeId},
    ops::Deref,
    sync::RwLock,
};

/// Api is used to exposed a collections of functions.
pub trait Api {
    const NAME: &'static str;
    const VERSION: Version;
}
// impl_downcast!(Api);

/// Implementation is used to define a function which can be implemented by a plugin.
pub trait Implementation {
    fn name(&self) -> &'static str;
    fn version(&self) -> Version;
}

struct ApiEntry {
    /// Arc<AtomicArc<T>> erased to ArcAtomicArcErased
    inner: ArcAtomicArcErased,
}

unsafe impl Send for ApiEntry {}
unsafe impl Sync for ApiEntry {}

/// An api registry that can be used to register api implementations.
pub struct ApiRegistry {
    // a hash map that maps api types to their implementations
    apis: RwLock<FxHashMap<(&'static str, Version), ApiEntry>>,
}

impl ApiRegistry {
    pub fn new() -> Self {
        Self {
            apis: RwLock::new(FxHashMap::default()),
        }
    }
}

// it's should be handled by proc macro or mbe
impl Api for ApiRegistry {
    const NAME: &'static str = "ApiRegistryApi";
    const VERSION: Version = Version::new(0, 1, 0);
}

impl ApiRegistryApi for ApiRegistry
where
    Self: 'static,
{
    fn set<T: 'static + Sync + Api>(&self, api: T) {
        let id = (T::NAME, T::VERSION);
        println!("{id:?}");
        println!("{:?}", type_name::<T>());
        println!("{:?}", type_name_of_val(&api));
        // get a ApiEntry first to transform into erased ptr
        let atomic_arc = Arc::new(AtomicArc::new(api));
        let erased = unsafe { ArcAtomicArcErased::from_arc(atomic_arc) };
        let entry = ApiEntry { inner: erased };
        self.apis.write().unwrap().insert(id, entry);
    }

    fn remove<T: 'static + Sync + Api>(&self) {
        todo!()
    }

    fn get<T: 'static + Sync + Api>(&'static self) -> ApiHandle<T> {
        // get the raw entry
        let r_lock = self.apis.read().unwrap();
        let id = (T::NAME, T::VERSION);
        println!("{:?}", type_name::<T>());
        println!("{id:?}");
        if let Some(raw_entry) = r_lock.get(&id) {
            ApiHandle(unsafe { ArcAtomicArcErased::ref_into_arc(&raw_entry.inner) })
        } else {
            drop(r_lock);
            let mut w_lock = self.apis.write().unwrap();
            // if don't have the api, create a new one inside registry
            let raw_entry = w_lock.entry((T::NAME, T::VERSION)).or_insert({
                let x =
                    unsafe { ArcAtomicArcErased::from_arc::<()>(Arc::new(AtomicArc::new(None))) };
                ApiEntry { inner: x }
            });

            // convert to api entry
            ApiHandle(unsafe { ArcAtomicArcErased::ref_into_arc(&raw_entry.inner) })
        }
    }
}

pub struct ApiHandle<T: 'static>(Arc<AtomicArc<T>>);

unsafe impl<T: 'static> Send for ApiHandle<T> {}
unsafe impl<T: 'static> Sync for ApiHandle<T> {}

impl<T> ApiHandle<T> {
    pub fn get(&self) -> Option<Guard<T>> {
        self.0.load()
    }
}

pub trait ApiRegistryApi: Api + 'static {
    fn set<T: 'static + Sync + Api>(&self, api: T);
    fn remove<T: 'static + Sync + Api>(&self);
    fn get<T: 'static + Sync + Api>(&'static self) -> ApiHandle<T>;
}

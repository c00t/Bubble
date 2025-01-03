#![feature(ptr_metadata)]
#![feature(downcast_unchecked)]
use bon::{bon, builder};
use bubble_core::api::{plugin_api::prelude::*, prelude::*};
use bubble_core::sync::circ;
use hot_reload_plugin_types::{DynHotReloadTestApi, HotReloadTestApi};
use std::sync::atomic::{AtomicUsize, Ordering};

/// API implementation for testing hot reload functionality
#[define_api(HotReloadTestApi)]
pub struct HotReloadTestApiImpl {
    counter: AtomicUsize,
    value: AtomicUsize,
}

impl HotReloadTestApi for HotReloadTestApiImpl {
    fn get_test_string(&self) -> String {
        const RANDOM: u32 = const_random::const_random!(u32);
        format!("Hello from HotReloadTestApi! (#{})", RANDOM)
    }

    fn increment_counter(&self) -> usize {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }

    fn get_counter(&self) -> usize {
        self.counter.load(Ordering::SeqCst)
    }

    fn set_value(&self, val: usize) {
        self.value.store(val, Ordering::SeqCst);
    }

    fn get_value(&self) -> usize {
        self.value.load(Ordering::SeqCst)
    }

    fn reset(&self) {
        self.counter.store(0, Ordering::SeqCst);
        self.value.store(0, Ordering::SeqCst);
    }
}

#[bon]
impl HotReloadTestApiImpl {
    #[builder]
    pub fn new() -> ApiHandle<dyn HotReloadTestApi> {
        let handler: AnyApiHandle = Box::new(Self {
            counter: AtomicUsize::new(0),
            value: AtomicUsize::new(0),
        })
        .into();
        handler.downcast()
    }
}

use bubble_core::tracing::{self, info, instrument};

#[plugin_export]
pub struct HotReloadPlugin;

impl PluginInstance for HotReloadPlugin {
    #[inline]
    fn load_plugin(
        context: &PluginContext,
        api_registry: ApiHandle<dyn ApiRegistryApi>,
        is_reload: bool,
    ) -> bool {
        let Ok(_) = context.load(&api_registry) else {
            return false;
        };
        let guard = circ::cs();
        let api_registry_api_local = api_registry.get(&guard).unwrap();
        test_instrument(1);
        // add hot reload test api to it
        api_registry_api_local.local_set(HotReloadTestApiImpl::builder().build(), context.dep_id);
        // info!("Loaded hot_reload_plugin({})", is_reload);
        true
    }
    #[inline]
    fn unload_plugin(
        context: &PluginContext,
        api_registry: ApiHandle<dyn ApiRegistryApi>,
        is_reload: bool,
    ) -> bool {
        let guard = circ::cs();
        let api_registry_api_local = api_registry.get(&guard).unwrap();
        // if is_reload is true, we don't need to remove the old api before loading the new one
        // because the new one will overwrite the old one, while remove will cause the old api struct to be dropped
        if !is_reload {
            api_registry_api_local.local_remove::<DynHotReloadTestApi>(context.dep_id);
        }
        // info!("Unloaded hot_reload_plugin({})", is_reload);
        true
    }
    #[inline]
    fn plugin_info() -> PluginInfo {
        let info = PluginInfo::new("hot_reload_plugin", Version::new(0, 1, 0));
        info
    }
}

#[instrument]
pub fn test_instrument(x: i32) {
    // Note: why tracing can get correct thread name here?
    info!("test instrument: {}", x);
}

use bubble_core::api::{
    plugin_api::{PluginContext, PluginInfo},
    prelude::*,
};

use bubble_core::tracing::{self, info, instrument};

#[no_mangle]
pub fn load_plugin(context: &PluginContext, api_registry_api: ApiHandle<dyn ApiRegistryApi>) {
    println!("load plugin in hot_reload_plugin");
    context.load();
    test_instrument(1);

    info!("hot_reload_plugin info log");
}

#[instrument]
pub fn test_instrument(x: i32) {
    // Note: why tracing can get correct thread name here?
    info!("test instrument: {}", x);
}

#[no_mangle]
pub fn unload_plugin(context: &PluginContext, api_registry_api: ApiHandle<dyn ApiRegistryApi>) {
    println!("unload plugin in hot_reload_plugin");
}

#[no_mangle]
pub fn plugin_info() -> PluginInfo {
    let info = PluginInfo::new("hot_reload_plugin", Version::new(0, 1, 0));
    info
}

use bubble_core::api::{
    plugin_api::{PluginContext, PluginInfo},
    prelude::*,
};

#[no_mangle]
pub fn load_plugin(context: &PluginContext, api_registry_api: ApiHandle<dyn ApiRegistryApi>) {
    println!("load plugin in hot_reload_plugin");
    context.load();
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

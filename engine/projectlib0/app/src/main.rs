use foundation::Plugin;
extern crate static_plugin;
pub fn main() {
    for plugin in inventory::iter::<Plugin> {
        println!("plugin: {}, version: {}", plugin.name, plugin.version);
        let x = (plugin.load_func)(42);
        println!("load_fucn(42) = {}", x);
    }
}

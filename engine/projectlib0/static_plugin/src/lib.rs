use foundation::{Plugin,PLUGINS};
use linkme::distributed_slice;

inventory::submit! {
    Plugin::new("static_plugin","1.0", inside_static_plugin)
}

fn inside_static_plugin(x: i32) -> i32 {
    x + 2
}

pub fn dummy() {}

#[distributed_slice(PLUGINS)]
static STATIC_PLUGIN: Plugin = Plugin::new("static_plugin", "1.0", inside_static_plugin);

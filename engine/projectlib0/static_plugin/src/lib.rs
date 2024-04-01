use foundation::Plugin;

inventory::submit! {
    Plugin::new("static_plugin","1.0", inside_static_plugin)
}

fn inside_static_plugin(x: i32) -> i32 {
    x + 2
}

pub fn dummy() {}

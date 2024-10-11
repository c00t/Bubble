use linkme::distributed_slice;

pub struct Plugin {
    pub name: &'static str,
    pub version: &'static str,
    pub load_func: fn(i32) -> i32,
}

impl Plugin {
    pub const fn new(name: &'static str, version: &'static str, load_func: fn(i32) -> i32) -> Self {
        Self {
            name,
            version,
            load_func,
        }
    }
}

inventory::collect!(Plugin);

inventory::submit! {
    Plugin::new("inside-foundation", "1.0", inside_lib_fn)
}

fn inside_lib_fn(x: i32) -> i32 {
    x + 1
}

#[distributed_slice]
pub static PLUGINS: [Plugin];

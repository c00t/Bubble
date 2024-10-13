/// Singleton utility for declare a plugin
///
/// # Warning
/// Avoid singletons as much as you can, this utility exists only for declare a plugin
pub unsafe trait Singleton: Sized + Send + Sync {
    /// # Safety
    /// This should only be called once.
    unsafe fn create(value: Self);

    /// # Safety
    /// This should only be called once.
    unsafe fn destroy();

    /// # Safety
    /// The data behind this pointer will only be valid for as long as the singleton is alive.
    unsafe fn ptr() -> *mut Self;
}

#[macro_export]
macro_rules! plugin {
    ($ty:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn tm_load_plugin(
            registry: *const machinery_api::foundation::ApiRegistryApi,
            load: bool,
        ) {
            machinery::load_plugin::<$ty>(registry, load);
        }
    };
}

// /// # Safety
// /// This should only be called once for load and once for unload.
// pub unsafe fn load_plugin<P: Plugin>(registry: *const ApiRegistryApi, load: bool) {
//     if load {
//         // Load the plugin
//         let plugin = P::load(registry);
//         P::create(plugin);
//     } else {
//         // Unload the plugin
//         P::destroy();
//     }
// }

pub trait Plugin: Singleton {
    fn load() -> Self;
}

/// Get a package version.
///
/// It's normally used in `build.rs` to get a static package version.
#[macro_export]
macro_rules! crate_version {
    () => {
        let pkg_version = env!("CARGO_PKG_VERSION");
        Version::parse(pkg_version).unwrap()
    };
}

/// Get a package name.
///
/// Returns a `&str`, which is the package name.
#[macro_export]
macro_rules! crate_name {
    () => {
        env!("CARGO_PKG_NAME")
    };
}

use core::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::{atomic::AtomicU64, Mutex, OnceLock};

use crate::bon::bon;
use dlopen2::wrapper::{Container, WrapperApi};
use semver::BuildMetadata;
use sharded_slab::Slab;

use crate::os;

use super::prelude::*;
#[allow(unused_imports)]
use crate::tracing::{debug, error, info, warn};

pub mod prelude {
    pub use super::{
        plugin_export, PluginApi, PluginContext, PluginHandle, PluginInfo, PluginInstance,
    };
}

pub use bubble_macros::plugin_export;

pub mod constants {
    pub const TARGET_TRIPLE: &str = env!("VERGEN_CARGO_TARGET_TRIPLE");
    pub const HOST_TRIPLE: &str = env!("VERGEN_RUSTC_HOST_TRIPLE");
    pub const RUSTC_SEMVER: &str = env!("VERGEN_RUSTC_SEMVER");
    pub const RUSTC_HASH: &str = env!("VERGEN_RUSTC_COMMIT_HASH");
    pub const BUILD_DEBUG: &str = env!("VERGEN_CARGO_DEBUG");
    pub const BUILD_OPT_LEVEL: &str = env!("VERGEN_CARGO_OPT_LEVEL");
    pub const BUILD_TIMESTAMP: &str = env!("VERGEN_BUILD_TIMESTAMP");
    pub fn build_meta() -> String {
        if BUILD_DEBUG == "true" {
            format!(
                "{}-{}-debug.{}-{}",
                RUSTC_SEMVER,
                &RUSTC_HASH[..9],
                BUILD_OPT_LEVEL,
                TARGET_TRIPLE.replace('_', "-")
            )
        } else {
            format!(
                "{}-{}-release.{}-{}",
                RUSTC_SEMVER,
                &RUSTC_HASH[..9],
                BUILD_OPT_LEVEL,
                TARGET_TRIPLE.replace('_', "-")
            )
        }
    }
}

/// Information about the plugin internally, include its identifier, and its version
///
/// Build details are included in the version's build metadata.
/// It's format is `{RUSTC_SEMVER}-{RUSTC_HASH[..9]}-{debug|release}.{BUILD_OPT_LEVEL}.{TARGET_TRIPLE.replace('_', "-")}`
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// The identifier of the plugin, provided by the plugin author.
    pub identifier: String,
    /// The version of the plugin, including the build metadata.
    pub version: Version,
}

impl fmt::Display for PluginInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::[{}]", self.identifier, self.version)
    }
}

impl PluginInfo {
    /// Create a new plugin info with the given name and version.
    ///
    /// The build metadata will be included by overwriting the version's build metadata you provided.
    pub fn new(name: &str, version: Version) -> Self {
        let build_meta = BuildMetadata::new(&constants::build_meta()).unwrap();
        let mut version = version;
        version.build = build_meta;
        Self {
            identifier: name.to_string(),
            version,
        }
    }
}

// /// The functions that should be exported by a plugin dll
// #[derive(WrapperApi)]
// pub struct PluginExportFns {
//     /// The load plugin function
//     load_plugin: fn(
//         context: &PluginContext,
//         api_registry: ApiHandle<dyn ApiRegistryApi>,
//         is_reload: bool,
//     ) -> bool,
//     /// The unload plugin function
//     unload_plugin: fn(
//         context: &PluginContext,
//         api_registry: ApiHandle<dyn ApiRegistryApi>,
//         is_reload: bool,
//     ) -> bool,
//     /// The plugin info function
//     plugin_info: fn() -> PluginInfo,
// }

/// Macro to generate the PluginInstance trait from PluginExportFns
macro_rules! generate_plugin_instance_trait {
    (
        $(#[$struct_meta:meta])*
        $struct_vis:vis struct $struct_name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_name:ident: fn($($arg_name:ident: $arg_ty:ty),* $(,)?) -> $ret_ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$struct_meta])*
        $struct_vis struct $struct_name {
            $(
                $(#[$field_meta])*
                $field_name: fn($($arg_name: $arg_ty),*) -> $ret_ty
            ),*
        }

        /// This trait is generated according to the [`PluginExportFns`] struct
        ///
        /// It will be implemented by the plugin author and generate **pub**,**export**,**no mangle** functions,
        /// which's names are the same as the fields of the [`PluginExportFns`] struct.
        ///
        /// It's recommended to use the [`#[inline]`] attribute for the functions.
        pub trait PluginInstance {
            $(
                $(#[$field_meta])*
                fn $field_name($($arg_name: $arg_ty),*) -> $ret_ty;
            )*
        }
    }
}

// Generate the [`PluginInstance`] trait
generate_plugin_instance_trait! {
    /// The functions that should be exported by a plugin dll, it will generate the [`PluginInstance`] trait
    #[derive(WrapperApi)]
    pub struct PluginExportFns {
        /// The load plugin function
        load_plugin: fn(
            context: &PluginContext,
            api_registry: ApiHandle<dyn ApiRegistryApi>,
            is_reload: bool,
        ) -> bool,
        /// The unload plugin function
        unload_plugin: fn(
            context: &PluginContext,
            api_registry: ApiHandle<dyn ApiRegistryApi>,
            is_reload: bool,
        ) -> bool,
        /// The plugin info function
        plugin_info: fn() -> PluginInfo,
    }
}

/// A plugin initialization context
///
/// ## Note
///
/// [`PluginContext::new`] should be called in main executable, and pass the context to the plugin's
/// [`PluginExportFns::load_plugin`] function. It's automatically done when you use [`PluginApi`].
///
/// [`PluginContext::load`] should be called first in the plugin's
/// [`PluginExportFns::load_plugin`] function before using any function inside core lib.
#[repr(C)]
pub struct PluginContext {
    /// The dyntls context
    dyntls_context: dyntls::Context,
    /// DepId
    pub dep_id: Option<DepId>,
}

impl fmt::Debug for PluginContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PluginContext")
    }
}

impl PluginContext {
    /// Create a new plugin context
    ///
    /// It should be called in main executable.
    pub fn new() -> Self {
        Self {
            dyntls_context: os::thread::dyntls_context::get(),
            dep_id: None,
        }
    }

    /// Give the plugin a dep id
    pub fn give_dep_id(&self, dep_id: DepId) -> Self {
        Self {
            dyntls_context: self.dyntls_context,
            dep_id: Some(dep_id),
        }
    }

    /// Load the plugin context
    ///
    /// It should be called first in the plugin's [`PluginExportFns::load_plugin`] function.
    ///
    /// ## Note
    ///
    /// This function currently:
    /// - initialize the dyntls context.
    /// - check the api registry api version compatibility.
    ///
    /// It will return an error if the api registry api version is incompatible.
    ///
    #[must_use]
    pub fn load(&self, api_handle: &ApiHandle<dyn ApiRegistryApi>) -> Result<(), ()> {
        unsafe {
            self.dyntls_context.initialize();
        }
        // check the api registry api version compatibility
        let api_handle = api_handle.get().unwrap();
        let actual_version = api_handle.version();
        // check against the version declared in the crate itself, use semver methods
        let request_version = crate::api::api_registry_api::api_registry_api_constants::VERSION;
        if actual_version < request_version {
            error!(
                "Plugin load failed, api registry api version mismatch, actual: {}, request: {}",
                actual_version, request_version
            );
            return Err(());
        }
        Ok(())
    }
}

/// A plugin
pub struct Plugin {
    /// The plugin path
    pub path: String,
    /// Load path
    pub load_path: String,
    /// The plugin name
    pub name: String,
    /// The container of the plugin's dll
    pub container: Container<PluginExportFns>,
    /// The dep id of the plugin
    pub dep_id: DepId,
    /// The plugin is hot reloadable?
    pub hot_reloadable: bool,
    /// The last mofified time of the plugin dll
    pub last_modified: u64,
}

impl fmt::Display for Plugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // name, path, hot_reloadable, last_modified
        f.debug_struct("Plugin")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("load_path", &self.load_path)
            .field("hot_reloadable", &self.hot_reloadable)
            .field("last_modified", &self.last_modified)
            .finish()
    }
}

impl fmt::Debug for Plugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plugin")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("load_path", &self.load_path)
            .field("hot_reloadable", &self.hot_reloadable)
            .field("last_modified", &self.last_modified)
            .finish()
    }
}

/// The metadata of the plugin inside the plugin registry
///
/// Compare to [`PluginInfo`], it contains more external information about the plugin,
/// such as the identity path(may different from the physical path([`PluginMeta::load_path`]) when hot reloading),
/// the plugin name, hot reloadable, last modified time, etc.
///
#[derive(Debug)]
pub struct PluginMeta {
    /// The identity path of the plugin
    pub path: String,
    /// The physical path of the plugin
    pub load_path: String,
    /// The name of the plugin
    pub name: String,
    /// The plugin info
    pub plugin_info: PluginInfo,
    /// The dep id of the plugin
    pub dep_id: DepId,
    /// The plugin is hot reloadable?
    pub hot_reloadable: bool,
    /// The last modified time of the plugin dll
    pub last_modified: u64,
}

impl fmt::Display for PluginMeta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = f
            .debug_struct("PluginBasic")
            .field("path", &self.path)
            .field("load_path", &self.load_path)
            .field("name", &self.name)
            .field("hot_reloadable", &self.hot_reloadable)
            .field("last_modified", &self.last_modified)
            .finish();
        f.write_fmt(format_args!(", PluginInfo: {}", self.plugin_info))
    }
}

fn get_plugin_name_from_path(path: &str) -> String {
    let path_buf = std::path::PathBuf::from(path);
    path_buf
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

impl Plugin {
    /// Creates a new Plugin instance by loading a dynamic library from the given path.
    ///
    /// ## Arguments
    ///
    /// * `path` - The path to the plugin dynamic library file
    /// * `hot_reloadable` - Whether the plugin supports hot reloading. If true, the plugin will be copied to a temporary location.
    /// * `is_reloading` - Whether this is a reload of an existing plugin (only relevant if hot_reloadable is true)
    ///
    /// ## Returns
    ///
    /// Returns a Result containing either:
    /// * Ok(Plugin) - Successfully loaded plugin instance
    /// * Err(std::io::Error) - Error loading the plugin, such as file not found or invalid library format
    ///
    /// ## Notes
    ///
    /// If hot reloading is enabled, the plugin file will be copied to a temporary location before loading.
    /// This allows the original file to be modified while the plugin is loaded.
    ///
    /// If reloading, the plugin file will be copied to another location, with a timestamp suffix.
    fn load(
        path: &str,
        hot_reloadable: bool,
        is_reloading: bool,
        api_registry: &ApiHandle<dyn ApiRegistryApi>,
    ) -> Result<Self, std::io::Error> {
        // Assert that is_reloading can only be true if hot_reloadable is true
        debug_assert!(!is_reloading || hot_reloadable, "Invalid parameter combination: is_reloading can only be true if hot_reloadable is true");
        // Copy the DLL if hot reloading is enabled
        let load_path = if hot_reloadable {
            copy_dynamic_library(path, is_reloading)?
        } else {
            path.to_string()
        };

        // // Wait for file to be fully accessible (helpful when rebuilds)
        // let start = std::time::Instant::now();
        // let max_wait = std::time::Duration::from_secs(2);
        // let sleep_duration = std::time::Duration::from_millis(10);

        // while start.elapsed() < max_wait {
        //     // Try to read the entire file
        //     match std::fs::read(&load_path) {
        //         Ok(_) => break,
        //         Err(_) => {
        //             if start.elapsed() + sleep_duration > max_wait {
        //                 break;
        //             }
        //             std::thread::sleep(sleep_duration);
        //             continue;
        //         }
        //     }
        // }

        // Load the DLL
        let container = unsafe {
            match Container::<PluginExportFns>::load(&load_path) {
                Ok(container) => container,
                Err(e) => {
                    // Clean up the copied file if it exists
                    if hot_reloadable {
                        let _ = std::fs::remove_file(&load_path);
                    }
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                }
            }
        };

        // Get the file name for the plugin name
        let name = get_plugin_name_from_path(path);

        // Get last modified time
        let last_modified = std::fs::metadata(path)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let dep_id = api_registry
            .get()
            .unwrap()
            .get_dep_context(name.clone(), None);
        let plugin = Plugin {
            path: path.to_string(),
            load_path,
            name,
            container,
            dep_id,
            hot_reloadable,
            last_modified,
        };
        let meta = plugin.metadata();
        info!("[Plugin] {}", meta);

        Ok(plugin)
    }

    /// Call the [`PluginExportFns::load_plugin`] function of the plugin
    fn load_plugin(
        &self,
        context: &PluginContext,
        api_registry: ApiHandle<dyn ApiRegistryApi>,
        is_reload: bool,
    ) -> bool {
        info!("Loading plugin: {}, is_reload: {}", self.name, is_reload);
        (self.container.load_plugin)(context, api_registry, is_reload)
    }

    /// Call the [`PluginExportFns::unload_plugin`] function of the plugin
    fn unload_plugin(
        &self,
        context: &PluginContext,
        api_registry: ApiHandle<dyn ApiRegistryApi>,
        is_reload: bool,
    ) -> bool {
        info!("Unloading plugin: {}, is_reload: {}", self.name, is_reload);
        (self.container.unload_plugin)(context, api_registry, is_reload)
    }

    /// Get the metadata of the plugin
    fn metadata(&self) -> PluginMeta {
        PluginMeta {
            path: self.path.clone(),
            load_path: self.load_path.clone(),
            name: self.name.clone(),
            plugin_info: self.container.plugin_info(),
            dep_id: self.dep_id,
            hot_reloadable: self.hot_reloadable,
            last_modified: self.last_modified,
        }
    }
}

/// A plugin handle returned or accepted by the plugin api
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle(usize);

/// The plugin api
///
/// Load the specified plugin dll and store it inside the plugin registry.
#[declare_api((0,1,0), bubble_core::api::plugin_api::PluginApi)]
pub trait PluginApi: Api {
    /// Load the plugin dll at the specified path, and return the plugin handle that can be used to unload or reload the plugin.
    ///
    /// It's the initial load function, so it will call your [`load_plugin`] function of the plugin with `is_reload` set to false.
    fn load(&self, path: &str, hot_reloadable: bool) -> Option<PluginHandle>;
    /// Get the version tick of the plugin registry,
    /// which will be increased when the plugin registry is modified.
    /// It's used to check if the plugin registry is modified since last [`PluginApi::version_tick`] call in other plugins.
    ///
    /// ## Note
    ///
    /// false positive may happen if the plugin load function failed.
    fn version_tick(&self) -> u64;
    /// Unload the plugin which loaded by the [`PluginApi::load`] function.
    ///
    /// It's the initial unload function, so it will call your [`unload_plugin`] function of the plugin with `is_reload` set to false.
    fn unload(&self, handle: PluginHandle);
    /// Handles hot reloading of plugins.
    ///
    /// It's the initial reload function, so it will call your [`unload_plugin`] and [`load_plugin`] functions of the plugin with `is_reload` set to true.
    fn reload(&self, handle: PluginHandle);

    /// Set the path of specific plugin after loaded, when reload, it will be loaded from the new path
    ///
    /// It's mainly used by plugin assets, which
    fn set_path(&self, handle: PluginHandle, new_path: String);

    /// Checks loaded plugins for any changes on disk, reload plugins that changed.
    fn check_hot_reload_tick(&self) -> bool;

    /// Get the metadata of the plugin
    fn info(&self, handle: PluginHandle) -> Option<PluginMeta>;
}

#[define_api(PluginApi)]
struct PluginsRegistry {
    /// The plugins' storage
    plugins_storage: Slab<Mutex<Plugin>>,
    /// The plugins' handle storage
    plugins_handles: Mutex<Vec<PluginHandle>>,
    /// Unloaded plugins' storage
    ///
    /// We reserve the unloaded plugins to avoid the plugin dll being unloaded while the program is using it(typically the static memory).
    unloaded_plugins: Mutex<Vec<Plugin>>,
    /// The version tick of the plugin registry,
    /// it will be increased when the plugin registry is modified.
    version_tick: AtomicU64,
    /// The plugin context
    context: PluginContext,
    /// The api registry api handler
    api_registry: ApiHandle<dyn ApiRegistryApi>,
}

static PLUGINS_REGISTRY: OnceLock<ApiHandle<DynPluginApi>> = OnceLock::new();

#[bon]
impl PluginsRegistry {
    #[builder]
    pub fn new() -> ApiHandle<DynPluginApi> {
        PLUGINS_REGISTRY
            .get_or_init(|| {
                let registry = PluginsRegistry::empty();
                let any_handler: AnyApiHandle = Box::new(registry).into();
                any_handler.downcast()
            })
            .clone()
    }

    pub fn empty() -> Self {
        Self {
            plugins_storage: Slab::new(),
            plugins_handles: Mutex::new(Vec::new()),
            unloaded_plugins: Mutex::new(Vec::new()),
            version_tick: AtomicU64::new(0),
            context: PluginContext::new(),
            api_registry: super::api_registry_api::get_api_registry_api(),
        }
    }

    pub fn version_tick(&self) -> u64 {
        self.version_tick.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn increase_version_tick(&self) {
        self.version_tick
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Clean up the hot reload copies of the plugin
fn clean_up_hot_reload_copies(path: &str) {
    // Get the directory and filename from the path
    let path = std::path::Path::new(path);
    let dir = path.parent().unwrap_or(std::path::Path::new(""));
    let file_name = path.file_stem().unwrap_or_default().to_str().unwrap_or("");

    // Create the hot reload prefix pattern
    let pattern = format!("{}.hot_reload.", file_name);

    // Read directory entries
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let entry_name = entry.file_name();
            let entry_str = entry_name.to_str().unwrap_or("");
            // Check if this is a hot reload copy (matches pattern and ends with a number)
            // template:
            //   test_plugin.hot_reload.{timestamp}.dll
            // examples:
            //   test_plugin.hot_reload.12345.dll
            //   test_plugin.hot_reload.67890.dll
            if entry_str.starts_with(&pattern) {
                let suffix = &entry_str[pattern.len()..];
                let suffix_parts: Vec<_> = suffix.split('.').collect();
                if suffix_parts.len() == 2 && suffix_parts[0].chars().all(|c| c.is_ascii_digit()) {
                    // Remove the hot reload copy
                    std::fs::remove_file(entry.path()).unwrap();
                }
            }
        }
    }
}

/// Copy the dynamic library to a temporary file, and return the path of the temporary file.
///
/// `is_reload` is true if the plugin is reloading.
fn copy_dynamic_library(path: &str, is_reloading: bool) -> Result<String, std::io::Error> {
    let path = std::path::Path::new(path);
    let extension = path.extension().unwrap_or_default().to_str().unwrap_or("");
    let file_stem = path.file_stem().unwrap_or_default().to_str().unwrap_or("");
    let directory = path
        .parent()
        .unwrap_or(std::path::Path::new(""))
        .to_str()
        .unwrap_or("");

    // Generate the temporary filename
    let tmp_name = if is_reloading {
        format!(
            "{}.hot_reload.{}",
            file_stem,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        )
    } else {
        format!("{}.hot_reload", file_stem)
    };

    // Create the full temporary path
    let tmp_path = format!("{}/{}.{}", directory, tmp_name, extension);

    // Check if files are identical
    if path.to_str().unwrap_or_default() == tmp_path {
        return Ok(tmp_path);
    }

    // Copy the file
    std::fs::copy(path, &tmp_path)?;

    // Handle debug symbols on macOS
    if cfg!(target_os = "macos") {
        let symbol_path = format!("{}.dSYM", path.to_str().unwrap_or_default());
        let tmp_symbol_path = format!("{}.dSYM", tmp_path);

        if let Err(e) = std::fs::copy(&symbol_path, &tmp_symbol_path) {
            // If debug symbols copy fails, clean up the temporary dll and return error
            std::fs::remove_file(&tmp_path)?;
            return Err(e);
        }
    }

    Ok(tmp_path)
}

use std::fs::OpenOptions;
use std::io;

/// Check if a plugin can be loaded by attempting to open it with write permissions.
///
/// This function checks two things:
/// - If the plugin file exists at the given path
/// - If the file can be opened with write permissions (i.e., not locked by another process)
///
/// # Arguments
///
/// * `path` - The path to the plugin file to check
///
/// # Returns
///
/// * `Ok(true)` - The plugin exists and can be loaded
/// * `Ok(false)` - The plugin either doesn't exist or is locked/inaccessible
/// * `Err(e)` - An unexpected IO error occurred while checking
fn can_load_plugin(path: &str) -> io::Result<bool> {
    // Check if file exists
    if !std::path::Path::new(path).exists() {
        return Ok(false);
    }

    // Try to open file with write permissions to check if it's not locked
    match OpenOptions::new().append(true).open(path) {
        Ok(_) => Ok(true),
        Err(e) => match e.kind() {
            io::ErrorKind::PermissionDenied => Ok(false),
            io::ErrorKind::WouldBlock => Ok(false), // File is locked
            _ => Err(e),                            // Propagate unexpected errors
        },
    }
}

impl PluginApi for PluginsRegistry {
    fn load(&self, path: &str, hot_reloadable: bool) -> Option<PluginHandle> {
        // increase the version tick
        self.increase_version_tick();

        // clean up the hot reload copies if it's a hot reloadable plugin
        if hot_reloadable {
            clean_up_hot_reload_copies(path);
        }

        let plugin = Plugin::load(path, hot_reloadable, false, &self.api_registry).ok()?;

        let plugin_context = self.context.give_dep_id(plugin.dep_id);
        let loaded = plugin.load_plugin(&plugin_context, self.api_registry.clone(), false);
        if !loaded {
            error!("Failed to load plugin: {}", plugin);
            self.api_registry
                .get()
                .unwrap()
                .remove_dep_context(plugin.dep_id);
            return None;
        }

        // add the plugin to the plugins registry
        let handle = self
            .plugins_storage
            .insert(Mutex::new(plugin))
            .and_then(|key| {
                // increase the version tick
                self.increase_version_tick();
                // add the handle to the plugins handles
                let mut handles_lock = self.plugins_handles.lock().unwrap();
                handles_lock.push(PluginHandle(key));
                Some(PluginHandle(key))
            });
        handle
    }

    fn version_tick(&self) -> u64 {
        self.version_tick()
    }

    fn unload(&self, handle: PluginHandle) {
        let slab = &self.plugins_storage;
        // we remove it from slab, but we don't drop it, because program will ref to static memory of the plugin dll
        let plugin = slab.take(handle.0 as usize);
        if plugin.is_none() {
            return;
        }
        let plugin = plugin.unwrap().into_inner().unwrap();
        let plugin_unload_context = self.context.give_dep_id(plugin.dep_id);
        let unloaded =
            plugin.unload_plugin(&plugin_unload_context, self.api_registry.clone(), false);
        if !unloaded {
            error!("Failed to unload plugin: {}", plugin);
        }
        self.api_registry
            .get()
            .unwrap()
            .remove_dep_context(plugin.dep_id);
        // remove the handle from the plugins handles
        let mut handles_lock = self.plugins_handles.lock().unwrap();
        handles_lock.retain(|&h| h != handle);
        // add the plugin to the unloaded plugins
        let mut unload_lock = self.unloaded_plugins.lock().unwrap();
        unload_lock.push(plugin);
        // increase the version tick
        self.increase_version_tick();
    }

    fn reload(&self, handle: PluginHandle) {
        let plugin = self.plugins_storage.get(handle.0 as usize);
        if plugin.is_none() {
            return;
        }
        let plugin = plugin.unwrap();
        let mut plugin = plugin.lock().unwrap();
        match can_load_plugin(&plugin.path) {
            Ok(true) => {
                info!("Reloading plugin: {:?}", plugin.path);
                let new_plugin = Plugin::load(
                    &plugin.path,
                    plugin.hot_reloadable,
                    true,
                    &self.api_registry,
                )
                .unwrap();
                // unload the old plugin, in fact, currently we don't need to record the remove operations.
                let plugin_unload_context = self.context.give_dep_id(plugin.dep_id);
                let unloaded =
                    plugin.unload_plugin(&plugin_unload_context, self.api_registry.clone(), true);
                if !unloaded {
                    error!("Failed to unload plugin when reload: {}", plugin);
                }
                self.api_registry
                    .get()
                    .unwrap()
                    .remove_dep_context(plugin.dep_id);
                // load new plugin
                let new_plugin_context = self.context.give_dep_id(new_plugin.dep_id);
                let loaded =
                    new_plugin.load_plugin(&new_plugin_context, self.api_registry.clone(), true);
                if !loaded {
                    error!("Failed to load plugin when reload: {}", new_plugin);
                    self.api_registry
                        .get()
                        .unwrap()
                        .remove_dep_context(new_plugin.dep_id);
                }
                // replace the old plugin with the new one
                let old_plugin = std::mem::replace(&mut *plugin, new_plugin);
                // add the old plugin to the unloaded plugins
                let mut unload_lock = self.unloaded_plugins.lock().unwrap();
                unload_lock.push(old_plugin);
            }
            Ok(false) => {
                error!("Failed to reload plugin: {:?}", plugin.path);
            }
            Err(e) => {
                error!("Failed to check if plugin is loaded: {:?}", e);
            }
        }
        self.increase_version_tick();
    }

    fn set_path(&self, handle: PluginHandle, new_path: String) {
        let plugin = self.plugins_storage.get(handle.0 as usize);
        if let Some(plugin_mutex) = plugin {
            let mut plugin = plugin_mutex.lock().expect("Failed to lock plugin");
            // Extract the new name from the path
            let new_name = get_plugin_name_from_path(&new_path);
            // Update both path and name
            plugin.path = new_path;
            plugin.name = new_name;
            // Increase version tick to notify of changes
            self.increase_version_tick();
        };
    }

    fn check_hot_reload_tick(&self) -> bool {
        // Get a read lock on the plugins storage
        let plugins = &self.plugins_storage;
        let handles = self.plugins_handles.lock().expect("Failed to lock handles");

        // Return early if no plugins
        if handles.is_empty() {
            return false;
        }

        // Use a static atomic to track which plugin to check next
        static CURRENT_INDEX: AtomicU64 = AtomicU64::new(0);

        // Get and increment the index atomically, wrapping around to 0
        let current = CURRENT_INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let index = (current as usize) % handles.len();

        // Check if the current plugin is modified
        let current_handle = handles[index];
        let is_modified = plugins
            .get(current_handle.0)
            .and_then(|plugin_lock| {
                let plugin = plugin_lock.lock().expect("Failed to lock plugin");
                if !plugin.hot_reloadable {
                    return None;
                }

                let modified_time = std::fs::metadata(&plugin.path)
                    .ok()?
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_secs();

                Some(modified_time != plugin.last_modified)
            })
            .unwrap_or(false);
        // debug!(
        //     "check hot reload tick: {}, is_modified: {}",
        //     index, is_modified
        // );
        static WHOLE_RELOAD_TICK: AtomicBool = AtomicBool::new(false);
        // If any plugin was modified, check and reload all modified plugins
        if is_modified && !WHOLE_RELOAD_TICK.load(std::sync::atomic::Ordering::Acquire) {
            // set the whole reload tick to true
            WHOLE_RELOAD_TICK.store(true, std::sync::atomic::Ordering::Release);
            info!("Reloading all modified plugins");
            for handle in handles.iter() {
                if let Some(plugin_lock) = plugins.get(handle.0) {
                    let plugin = plugin_lock.lock().expect("Failed to lock plugin");
                    if !plugin.hot_reloadable {
                        continue;
                    }

                    // Check if this plugin was modified
                    if let Ok(metadata) = std::fs::metadata(&plugin.path) {
                        if let Ok(modified) = metadata.modified() {
                            if let Ok(modified_secs) = modified
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                            {
                                if modified_secs != plugin.last_modified {
                                    // Drop the lock before calling reload
                                    drop(plugin);
                                    drop(plugin_lock);
                                    self.reload(*handle);
                                }
                            }
                        }
                    }
                }
            }
            // set the whole reload tick to false
            WHOLE_RELOAD_TICK.store(false, std::sync::atomic::Ordering::Release);
        }

        is_modified
    }

    fn info(&self, handle: PluginHandle) -> Option<PluginMeta> {
        let plugin = self.plugins_storage.get(handle.0 as usize);
        if let Some(plugin_mutex) = plugin {
            let plugin = plugin_mutex.lock().expect("Failed to lock plugin");
            Some(plugin.metadata())
        } else {
            None
        }
    }
}

/// Get the plugins path in the specified directory
pub fn get_plugins_in_directory(directory: &str) -> Vec<String> {
    let mut plugins = Vec::new();

    if let Ok(entries) = std::fs::read_dir(directory) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                // Only process files
                if !file_type.is_file() {
                    continue;
                }

                if let Some(file_name) = entry.file_name().to_str() {
                    // Skip hot reload copies
                    if file_name.contains(".hot_reload.") {
                        continue;
                    }

                    if file_name.ends_with(std::env::consts::DLL_EXTENSION) {
                        if let Some(path) = entry.path().to_str() {
                            plugins.push(path.to_string());
                        }
                    }
                }
            }
        }
    }

    plugins
}

/// Get the plugin named `plugin_name` under the subdirectory `sub_dir` besides the file `file`
pub fn get_plugin_besides_file(file: &str, plugin_name: &str, sub_dir: &str) -> Option<String> {
    let ext = std::env::consts::DLL_EXTENSION;
    let file_path = std::path::Path::new(file);

    // Get parent directory of the file
    let parent_dir = file_path.parent()?;

    // Construct path to subdirectory
    let plugin_dir = parent_dir.join(sub_dir);

    // Construct expected plugin path
    let plugin_path = plugin_dir.join(format!("{}.{}", plugin_name, ext));

    // Convert to string and return if exists
    if plugin_path.exists() {
        plugin_path.to_str().map(String::from)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    #[test]
    fn test_get_plugin_besides_file() {
        // Create a temporary directory structure
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create main file
        let main_file_path = temp_path.join("main.exe");
        File::create(&main_file_path).unwrap();

        // Create plugins directory
        let plugins_dir = temp_path.join("plugins");
        fs::create_dir(&plugins_dir).unwrap();

        // Create plugin file
        let ext = std::env::consts::DLL_EXTENSION;
        let plugin_path = plugins_dir.join(format!("test_plugin.{}", ext));
        File::create(&plugin_path).unwrap();

        // Test successful case
        let result =
            get_plugin_besides_file(main_file_path.to_str().unwrap(), "test_plugin", "plugins");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), plugin_path.to_str().unwrap().to_string());

        // Test non-existent plugin
        let result = get_plugin_besides_file(
            main_file_path.to_str().unwrap(),
            "non_existent_plugin",
            "plugins",
        );
        assert!(result.is_none());

        // Test non-existent directory
        let result = get_plugin_besides_file(
            main_file_path.to_str().unwrap(),
            "test_plugin",
            "non_existent_dir",
        );
        assert!(result.is_none());

        // Test invalid main file path
        let result = get_plugin_besides_file("non/existent/path", "test_plugin", "plugins");
        assert!(result.is_none());
    }

    #[test]
    fn test_get_plugins_in_directory() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create test files with platform-specific extension
        let ext = std::env::consts::DLL_EXTENSION;
        let files = [
            format!("plugin1.{ext}"),
            format!("plugin2.{ext}"),
            format!("plugin1.hot_reload.123.{ext}"), // Should be ignored
            format!("plugin2.hot_reload.{ext}"),     // Should be ignored
            "not_a_plugin.txt".to_string(),          // Should be ignored
            format!("another.hot_reload.456.{ext}"), // Should be ignored
        ];

        // Create all test files
        for file_name in &files {
            let file_path = temp_path.join(file_name);
            File::create(&file_path).unwrap();
        }

        // Create a subdirectory (should be ignored)
        std::fs::create_dir(temp_path.join("subdir")).unwrap();

        // Get plugins
        let plugins = get_plugins_in_directory(temp_path.to_str().unwrap());

        // Verify results
        assert_eq!(plugins.len(), 2);
        assert!(plugins
            .iter()
            .any(|p| p.ends_with(&format!("plugin1.{ext}"))));
        assert!(plugins
            .iter()
            .any(|p| p.ends_with(&format!("plugin2.{ext}"))));
        assert!(plugins.iter().all(|p| !p.contains(".hot_reload.")));
    }

    #[test]
    fn test_clean_up_hot_reload_copies() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create original plugin file
        let plugin_path = temp_path.join("test_plugin.dll");
        File::create(&plugin_path).unwrap();

        // Create some hot reload copies
        let copy1 = temp_path.join("test_plugin.hot_reload.1.dll");
        let copy2 = temp_path.join("test_plugin.hot_reload.42.dll");
        let non_copy = temp_path.join("test_plugin.other.1.dll"); // Should not be deleted

        File::create(&copy1).unwrap();
        File::create(&copy2).unwrap();
        File::create(&non_copy).unwrap();

        // Run the cleanup function
        clean_up_hot_reload_copies(plugin_path.to_str().unwrap());

        // Verify results
        assert!(plugin_path.exists(), "Original plugin should still exist");
        assert!(!copy1.exists(), "Hot reload copy 1 should be deleted");
        assert!(!copy2.exists(), "Hot reload copy 2 should be deleted");
        assert!(non_copy.exists(), "Non-hot reload file should still exist");
    }

    #[test]
    fn test_copy_dynamic_library() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create original plugin file
        let plugin_path = temp_path.join("test_plugin.dll");
        File::create(&plugin_path).unwrap();

        // Test non-hot-reloading copy
        let result = copy_dynamic_library(plugin_path.to_str().unwrap(), false).unwrap();
        let copied_path = std::path::Path::new(&result);
        assert!(copied_path.exists(), "Copied file should exist");
        // check file name test_plugin.hot_reload.dll
        assert_eq!(
            copied_path.file_name().unwrap().to_str().unwrap(),
            "test_plugin.hot_reload.dll"
        );

        // Test hot-reloading copy
        let result = copy_dynamic_library(plugin_path.to_str().unwrap(), true).unwrap();
        let copied_path = std::path::Path::new(&result);
        assert!(copied_path.exists(), "Copied file should exist");
        // check file name test_plugin.hot_reload.{timestamp}.dll
        let file_name = copied_path.file_name().unwrap().to_str().unwrap();
        let parts: Vec<_> = file_name.split('.').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "test_plugin");
        assert_eq!(parts[1], "hot_reload");
        eprintln!("parts[2]: {}", parts[2]);
        assert!(parts[2].chars().all(|c| c.is_ascii_digit()));

        // Test macOS debug symbols if on macOS
        if cfg!(target_os = "macos") {
            // Create a fake dSYM file
            let dsym_path = format!("{}.dSYM", plugin_path.to_str().unwrap());
            File::create(&dsym_path).unwrap();

            let result = copy_dynamic_library(plugin_path.to_str().unwrap(), false).unwrap();
            let copied_dsym = format!("{}.dSYM", result);
            assert!(
                std::path::Path::new(&copied_dsym).exists(),
                "Copied dSYM should exist"
            );
        }
    }
}

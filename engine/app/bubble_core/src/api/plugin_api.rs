use core::fmt;
use std::sync::{atomic::AtomicU64, Mutex, OnceLock};

use bon::{bon, builder};
use dlopen2::wrapper::{Container, WrapperApi};
use semver::BuildMetadata;

use crate::os;

use super::prelude::*;

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

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: Version,
}

impl PluginInfo {
    pub fn new(name: &str, version: Version) -> Self {
        let build_meta = BuildMetadata::new(&constants::build_meta()).unwrap();
        let mut version = version;
        version.build = build_meta;
        Self {
            name: name.to_string(),
            version,
        }
    }
}

/// The plugin export functions
#[derive(WrapperApi)]
pub struct PluginExportFns {
    load_plugin: fn(context: &PluginContext, api_registry: ApiHandle<dyn ApiRegistryApi>),
    unload_plugin: fn(context: &PluginContext, api_registry: ApiHandle<dyn ApiRegistryApi>),
    plugin_info: fn() -> PluginInfo,
}

/// A plugin initialization context
#[repr(C)]
pub struct PluginContext {
    /// The dyntls context
    dyntls_context: dyntls::Context,
}

impl fmt::Debug for PluginContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PluginContext")
    }
}

impl PluginContext {
    pub fn new() -> Self {
        Self {
            dyntls_context: os::thread::dyntls_context::get(),
        }
    }

    pub fn load(&self) {
        unsafe {
            self.dyntls_context.initialize();
        }
    }
}

/// A plugin
pub struct Plugin {
    /// The plugin path
    pub path: String,
    /// The plugin name
    pub name: String,
    /// The container of the plugin's dll
    pub container: Container<PluginExportFns>,
    /// The plugin is hot reloadable?
    pub hot_reloadable: bool,
    /// The last mofified time of the plugin dll
    pub last_modified: u64,
}

impl Plugin {
    fn load(path: &str, hot_reloadable: bool, is_reloading: bool) -> Result<Self, std::io::Error> {
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
        let path_buf = std::path::PathBuf::from(path);
        let name = path_buf
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        // Get last modified time
        let last_modified = std::fs::metadata(path)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Plugin {
            path: path.to_string(),
            name,
            container,
            hot_reloadable,
            last_modified,
        })
    }

    fn load_plugin(&self, context: &PluginContext, api_registry: ApiHandle<dyn ApiRegistryApi>) {
        (self.container.load_plugin)(context, api_registry);
    }

    fn unload_plugin(&self, context: &PluginContext, api_registry: ApiHandle<dyn ApiRegistryApi>) {
        (self.container.unload_plugin)(context, api_registry);
    }
}

/// A plugin handle returned or accepted by the plugin api
pub struct PluginHandle(u64);

/// The plugin api
///
/// Load the specified plugin dll and store it inside the plugin registry.
#[declare_api((0,1,0), bubble_core::api::plugin_api::PluginApi)]
pub trait PluginApi: Api {
    /// Load the plugin dll at the specified path, and return the plugin handle that can be used to unload or reload the plugin.
    fn load(&self, path: &str, hot_reloadable: bool) -> Option<PluginHandle>;
    /// Get the version tick of the plugin registry,
    /// which will be increased when the plugin registry is modified.
    /// It's used to check if the plugin registry is modified since last [`PluginApi::version_tick`] call in other plugins.
    ///
    /// ## Note
    ///
    /// false positive may happen if the plugin load function failed.
    fn version_tick(&self) -> u64;
}

#[define_api(PluginApi)]
struct PluginsRegistry {
    /// The plugins' storage
    plugins: Mutex<Vec<Plugin>>,
    /// The version tick of the plugin registry,
    /// it will be increased when the plugin registry is modified.
    version_tick: AtomicU64,
    context: PluginContext,
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
            plugins: Mutex::new(Vec::new()),
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

impl PluginApi for PluginsRegistry {
    fn load(&self, path: &str, hot_reloadable: bool) -> Option<PluginHandle> {
        // increase the version tick
        self.increase_version_tick();

        // clean up the hot reload copies if it's a hot reloadable plugin
        if hot_reloadable {
            clean_up_hot_reload_copies(path);
        }

        let plugin = Plugin::load(path, hot_reloadable, false).ok()?;

        plugin.load_plugin(&self.context, self.api_registry.clone());

        let info = plugin.container.plugin_info();
        println!("{:?}", info);

        // add the plugin to the plugins registry
        self.plugins.lock().unwrap().push(plugin);
        Some(PluginHandle(self.plugins.lock().unwrap().len() as u64 - 1))
    }

    fn version_tick(&self) -> u64 {
        self.version_tick()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

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

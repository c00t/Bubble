pub mod utils;
use std::{
    future::Future,
    path::PathBuf,
    thread::{self, JoinHandle},
};

use bubble_core::{
    api::{
        api_registry_api::get_api_registry_api,
        plugin_api::PluginApi,
        prelude::{ApiHandle, ApiRegistryApi},
        Api, ApiConstant,
    },
    sync::circ,
    tracing,
};
use bubble_tasks::{async_ffi::FutureExt, types::TaskSystemApi};

pub struct TestSkeleton {
    pub cwd: PathBuf,
    pub api_registry_api: ApiHandle<dyn ApiRegistryApi>,
    pub task_system_api: ApiHandle<dyn TaskSystemApi>,
    pub plugin_api: ApiHandle<dyn PluginApi>,
}

impl TestSkeleton {
    pub fn new() -> Self {
        let cwd = std::env::current_dir().unwrap();
        let context = bubble_core::os::thread::dyntls_context::get();
        unsafe {
            context.initialize();
        }

        bubble_log::LogSubscriberBuilder::default().set_global();

        let api_registry_api = get_api_registry_api();

        // task system api
        let task_system_api = {
            let guard = circ::cs();
            let api = bubble_tasks::task_system_api::get_task_system_api_default();
            api_registry_api
                .get(&guard)
                .expect("Failed to get API registry api")
                .local_set(api, None)
        };
        // plugin api
        let plugin_api = {
            let guard = circ::cs();
            let api = bubble_core::api::plugin_api::get_default_plugin_api();
            api_registry_api
                .get(&guard)
                .expect("Failed to get API registry api")
                .local_set(api, None)
        };

        Self {
            cwd,
            api_registry_api,
            task_system_api,
            plugin_api,
        }
    }

    pub fn create_tick_thread<T, I>(&self, tick_fut: fn(I) -> T, tick_fut_arg: I) -> JoinHandle<()>
    where
        T: Future<Output = bool> + 'static,
        I: Clone + Send + 'static,
    {
        let task_system_api_clone = self.task_system_api.clone();
        let plugin_api_clone = self.plugin_api.clone();
        let guard_create = || circ::cs();
        thread::Builder::new()
            .name("Tick Thread".to_string())
            .spawn(move || {
                let task_system = task_system_api_clone;
                let plugin = plugin_api_clone;
                let guard = guard_create();
                let task_api = task_system.get(&guard).unwrap();
                while unsafe {
                    let plugin_api = plugin.clone();
                    // let tick_fut_clone = tick_fut.clone();
                    let tick_fut_arg_clone = tick_fut_arg.clone();
                    task_api.tick(
                        async move {
                            let guard = circ::cs();
                            let plugin_api = plugin_api.get(&guard).unwrap();
                            let reload = plugin_api.check_hot_reload_tick();
                            let b = tick_fut(tick_fut_arg_clone).await;
                            #[cfg(feature = "tracy")]
                            tracing::event!(
                                tracing::Level::INFO,
                                message = "end tick",
                                tracy.frame_mark = true
                            );
                            b
                        }
                        .into_local_ffi(),
                    )
                } {
                    // println!("...");
                }
            })
            .unwrap()
    }

    pub fn add_api_to_registry<T: ApiConstant + Api + ?Sized>(&self, api: ApiHandle<T>) {
        let guard = circ::cs();
        let local_api_registry_api = self.api_registry_api.get(&guard).unwrap();
        local_api_registry_api.local_set(api, None);
    }

    pub fn end_test(&self) {
        let guard = circ::cs();
        self.task_system_api.get(&guard).map(|local| {
            local.shutdown();
        });
        self.api_registry_api.get(&guard).map(|local| {
            local.local_shutdown();
        });
    }
}

// Fork from: https://github.com/bevyengine/bevy/blob/main/crates/bevy_log/src/lib.rs
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use std::error::Error;

use bubble_core::tracing::Level;
use tracing_subscriber::{
    filter::{FromEnvError, ParseError},
    layer::SubscriberExt,
    EnvFilter, Layer, Registry,
};

#[cfg(target_os = "android")]
mod android_tracing;

pub struct LogSubscriberBuilder {
    pub filter: String,
    pub level: Level,
    pub custom_layer: fn() -> Option<Box<dyn Layer<Registry> + Send + Sync + 'static>>,
}

impl Default for LogSubscriberBuilder {
    fn default() -> Self {
        Self {
            filter: "".to_string(),
            level: Level::INFO,
            custom_layer: || None,
        }
    }
}

impl LogSubscriberBuilder {
    pub fn set_global(&self) {
        #[cfg(feature = "panic-hook")]
        {
            // it can only catch panic in the main executable
            let old_handler = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |infos| {
                eprintln!("{}", tracing_error::SpanTrace::capture());
                old_handler(infos);
            }));
        }
        let finished_subscriber;
        let subscriber = Registry::default();
        let subscriber = subscriber.with((self.custom_layer)());
        let default_filter = { format!("{},{}", self.level, self.filter) };
        let filter_layer = EnvFilter::try_from_default_env()
            .or_else(|from_env_error| {
                _ = from_env_error
                    .source()
                    .and_then(|source| source.downcast_ref::<ParseError>())
                    .map(|parse_err| {
                        // we cannot use the `error!` macro here because the logger is not ready yet.
                        eprintln!("LogPlugin failed to parse filter from env: {}", parse_err);
                    });

                Ok::<EnvFilter, FromEnvError>(EnvFilter::builder().parse_lossy(&default_filter))
            })
            .unwrap();
        let subscriber = subscriber.with(filter_layer);

        #[cfg(feature = "panic-hook")]
        let subscriber = subscriber.with(tracing_error::ErrorLayer::default());

        #[cfg(all(
            not(target_arch = "wasm32"),
            not(target_os = "android"),
            not(target_os = "ios")
        ))]
        {
            #[cfg(feature = "tracing-tracy")]
            let tracy_layer = tracing_tracy::TracyLayer::default();

            let fmt_layer = tracing_subscriber::fmt::Layer::default()
                .with_level(true)
                .with_target(true)
                // .with_thread_ids(true)
                .with_thread_names(true)
                .with_line_number(true)
                .with_writer(std::io::stderr);

            #[cfg(feature = "tracy")]
            let fmt_layer =
                fmt_layer.with_filter(tracing_subscriber::filter::FilterFn::new(|meta| {
                    meta.fields().field("tracy.frame_mark").is_none()
                }));

            let subscriber = subscriber.with(fmt_layer);

            #[cfg(feature = "tracy")]
            let subscriber = subscriber.with(tracy_layer);
            finished_subscriber = subscriber;
        }

        #[cfg(target_os = "android")]
        {
            finished_subscriber = subscriber.with(android_tracing::AndroidLayer::default());
        }

        #[cfg(target_os = "ios")]
        {
            finished_subscriber = subscriber.with(tracing_oslog::OsLogger::default());
        }

        bubble_core::tracing::subscriber::set_global_default(finished_subscriber).unwrap();
    }
}

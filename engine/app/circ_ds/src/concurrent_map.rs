use circ::Guard;

pub trait OutputHolder<V> {
    fn output(&self) -> &V;
}

pub trait ConcurrentMap<K, V> {
    type Output<'a>: OutputHolder<V>
    where
        Self: 'a;

    fn new() -> Self;
    fn get<'l>(&self, key: &K, cs: &'l Guard) -> Option<Self::Output<'l>>;
    fn insert<'l>(&self, key: K, value: V, cs: &'l Guard) -> bool;
    fn remove<'l>(&self, key: &K, cs: &'l Guard) -> Option<Self::Output<'l>>;
}

#[cfg(test)]
pub mod tests {
    extern crate rand;
    use std::error::Error;

    use super::{ConcurrentMap, OutputHolder};
    use circ::{cs, Guard};
    use crossbeam_utils::thread;
    use rand::prelude::*;
    use tracing_subscriber::{
        filter::{FromEnvError, ParseError},
        EnvFilter,
    };
    use tracy_client::ProfiledAllocator;

    #[global_allocator]
    static GLOBAL: ProfiledAllocator<std::alloc::System> =
        ProfiledAllocator::new(std::alloc::System, 10);

    const THREADS: i32 = 16;
    const ELEMENTS_PER_THREADS: i32 = 1000;

    pub fn smoke<M: ConcurrentMap<i32, String> + Send + Sync>() {
        let context = dyntls_host::get();
        let default_filter = { format!("{},{}", tracing::Level::INFO, "") };
        use tracing_subscriber::layer::SubscriberExt;
        let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
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
        // we use tracy to see memory usage cross-platform,
        // it's easier but isn't realtime
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry()
                .with(filter_layer)
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_thread_ids(true)
                        .with_thread_names(true)
                        .with_file(true)
                        .with_line_number(true),
                )
                .with(tracing_tracy::TracyLayer::default()),
        )
        .expect("setup tracy layer");

        let map_l = M::new();
        let map1 = &map_l;
        thread::scope(|s| {
            let handles: Vec<_> = (0..THREADS)
                .map(|t| {
                    s.spawn(move |_| {
                        unsafe {
                            context.initialize();
                        }
                        let mut rng = rand::thread_rng();
                        let mut keys: Vec<i32> =
                            (0..ELEMENTS_PER_THREADS).map(|k| k * THREADS + t).collect();
                        keys.shuffle(&mut rng);
                        for i in keys {
                            let cs = cs();
                            assert!(map1.insert(i, i.to_string(), &cs));
                        }
                    })
                })
                .collect();

            handles.into_iter().for_each(|h| h.join().unwrap());
        })
        .unwrap();
        tracing::event!(tracing::Level::INFO, "insert 1");
        thread::scope(|s| {
            let handles: Vec<_> = (0..(THREADS / 2))
                .map(|t| {
                    s.spawn(move |_| {
                        unsafe {
                            context.initialize();
                        }
                        let mut rng = rand::thread_rng();
                        let mut keys: Vec<i32> =
                            (0..ELEMENTS_PER_THREADS).map(|k| k * THREADS + t).collect();
                        keys.shuffle(&mut rng);
                        let cs = &mut cs();
                        for i in keys {
                            assert_eq!(i.to_string(), *map1.remove(&i, cs).unwrap().output());
                            cs.reactivate();
                        }
                    })
                })
                .collect();

            handles.into_iter().for_each(|h| h.join().unwrap());
        })
        .unwrap();
        tracing::event!(tracing::Level::INFO, "remove 1");
        thread::scope(|s| {
            let handles: Vec<_> = ((THREADS / 2)..THREADS)
                .map(|t| {
                    s.spawn(move |_| {
                        unsafe {
                            context.initialize();
                        }
                        let mut rng = rand::thread_rng();
                        let mut keys: Vec<i32> =
                            (0..ELEMENTS_PER_THREADS).map(|k| k * THREADS + t).collect();
                        keys.shuffle(&mut rng);
                        let mut cs = cs();
                        for i in keys {
                            let result = map1.get(&i, &cs);
                            if (0..THREADS / 2).contains(&i) {
                                assert!(result.is_none());
                                drop(result);
                            } else {
                                assert_eq!(i.to_string(), *result.unwrap().output());
                            }
                            cs.reactivate();
                        }
                    })
                })
                .collect();

            handles.into_iter().for_each(|h| h.join().unwrap());
        })
        .unwrap();
        tracing::event!(tracing::Level::INFO, "contains 1");

        let map_o = M::new();
        let map2 = &map_o;
        thread::scope(|s| {
            let handles: Vec<_> = (0..THREADS)
                .map(|t| {
                    s.spawn(move |_| {
                        unsafe {
                            context.initialize();
                        }
                        let mut rng = rand::thread_rng();
                        let mut keys: Vec<i32> =
                            (0..ELEMENTS_PER_THREADS).map(|k| k * THREADS + t).collect();
                        keys.shuffle(&mut rng);
                        for i in keys {
                            let cs = cs();
                            assert!(map2.insert(i, i.to_string(), &cs));
                        }
                    })
                })
                .collect();

            handles.into_iter().for_each(|h| h.join().unwrap());
        })
        .unwrap();
        tracing::event!(tracing::Level::INFO, "insert 2");
        thread::scope(|s| {
            let handles: Vec<_> = (0..(THREADS / 2))
                .map(|t| {
                    s.spawn(move |_| {
                        unsafe {
                            context.initialize();
                        }
                        let mut rng = rand::thread_rng();
                        let mut keys: Vec<i32> =
                            (0..ELEMENTS_PER_THREADS).map(|k| k * THREADS + t).collect();
                        keys.shuffle(&mut rng);
                        let cs = &mut cs();
                        for i in keys {
                            assert_eq!(i.to_string(), *map2.remove(&i, cs).unwrap().output());
                            cs.reactivate();
                        }
                    })
                })
                .collect();

            handles.into_iter().for_each(|h| h.join().unwrap());
        })
        .unwrap();
        tracing::event!(tracing::Level::INFO, "remove 2");
        thread::scope(|s| {
            let handles: Vec<_> = ((THREADS / 2)..THREADS)
                .map(|t| {
                    s.spawn(move |_| {
                        unsafe {
                            context.initialize();
                        }
                        let mut rng = rand::thread_rng();
                        let mut keys: Vec<i32> =
                            (0..ELEMENTS_PER_THREADS).map(|k| k * THREADS + t).collect();
                        keys.shuffle(&mut rng);
                        let mut cs = cs();
                        for i in keys {
                            let result = map2.get(&i, &cs);
                            if (0..THREADS / 2).contains(&i) {
                                assert!(result.is_none());
                                drop(result);
                            } else {
                                assert_eq!(i.to_string(), *result.unwrap().output());
                            }
                            cs.reactivate();
                        }
                    })
                })
                .collect();

            handles.into_iter().for_each(|h| h.join().unwrap());
        })
        .unwrap();
        tracing::event!(tracing::Level::INFO, "contains 2");

        // because circ use delay drop, keep the program running to see memory usage in tracy
        // memory log data is huge, so tracy is lagging behind
        // only if you can see the `first done`... events in tracy, data is collected fully.
        loop {}
    }
}

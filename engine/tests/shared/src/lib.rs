#![feature(ptr_metadata)]
#![feature(once_cell_get_mut)]
use std::sync::OnceLock;
use std::{num::NonZeroUsize, time::Duration};

use async_ffi::FutureExt;
use bon::{bon, builder};
use bubble_core::{
    api::prelude::*,
    sync::{AsPtr, AtomicArc},
    thread_local,
};
use bubble_tasks::dispatcher::Dispatcher;

pub trait TraitCastableDropSuper {
    fn super_func(&self);
}

pub trait TraitCastableDropSub {
    fn sub_func(&self);
}

#[make_trait_castable(TraitCastableDropSuper, TraitCastableDropSub)]
pub struct TraitCastableDrop {
    pub value: i32,
}

unique_id! {
    #[UniqueTypeIdVersion((0,1,0))]
    dyn TraitCastableDropSuper;
    dyn TraitCastableDropSub;
}

impl TraitCastableDropSuper for TraitCastableDrop {
    fn super_func(&self) {
        println!("super_func: {}", self.value);
    }
}

impl TraitCastableDropSub for TraitCastableDrop {
    fn sub_func(&self) {
        println!("sub_func: {}", self.value);
    }
}

impl Drop for TraitCastableDrop {
    fn drop(&mut self) {
        println!("TraitCastableDrop: {}", self.value);
    }
}

crate::thread_local! {
    pub static TEST_VAR: std::sync::Mutex<i32> =  std::sync::Mutex::new(0);
}

pub type StringAlias0 = std::string::String;
pub type StringAlias1 = String;

struct MainRuntime(bubble_tasks::runtime::Runtime);

unsafe impl Send for MainRuntime {}
unsafe impl Sync for MainRuntime {}

#[define_api_with_id((0,1,0), shared::TaskSystemApi)]
struct TaskSystem {
    pub dispatcher: AtomicArc<std::sync::RwLock<Option<Dispatcher>>>,
    pub tick_sender: thingbuf::mpsc::blocking::Sender<bool>,
    pub tick_reader: thingbuf::mpsc::blocking::Receiver<bool>,
    pub main_runtime: MainRuntime,
}

#[builder]
pub fn get_task_system_api(
    thread_count: usize,
    thread_names: impl Fn(usize) -> String + 'static,
    frame_delay: usize,
) -> ApiHandle<dyn TaskSystemApi> {
    let dispatcher = Dispatcher::builder()
        .worker_threads(NonZeroUsize::new(thread_count).unwrap())
        .thread_names(thread_names)
        .build()
        .unwrap();
    let (sender, reader) = thingbuf::mpsc::blocking::channel(frame_delay);
    // queue a few frames to start with
    sender.send(true).unwrap();
    let main_runtime = MainRuntime(bubble_tasks::runtime::Runtime::new().unwrap());
    let task_system_api: AnyApiHandle = Box::new(TaskSystem {
        dispatcher: AtomicArc::new(std::sync::RwLock::new(Some(dispatcher))),
        tick_sender: sender,
        tick_reader: reader,
        main_runtime,
    })
    .into();
    task_system_api.downcast()
}

#[bon]
impl TaskSystem {
    #[builder]
    pub fn new() -> ApiHandle<dyn TaskSystemApi> {
        let dispatcher = Dispatcher::builder()
            .thread_names(|index| format!("compio-worker-{index}"))
            .build()
            .unwrap();
        let (sender, reader) = thingbuf::mpsc::blocking::channel(3);
        for _ in 0..3 {
            sender.send(true).unwrap();
        }
        let main_runtime = MainRuntime(bubble_tasks::runtime::Runtime::new().unwrap());
        let task_system_api: AnyApiHandle = Box::new(TaskSystem {
            dispatcher: AtomicArc::new(std::sync::RwLock::new(Some(dispatcher))),
            tick_sender: sender,
            tick_reader: reader,
            main_runtime,
        })
        .into();
        task_system_api.downcast()
    }
}

pub trait TaskSystemApi: Api {
    /// Spawn a task on the current thread, and return a handle to it.
    ///
    /// The task will be executed on the current thread, and the handle will be returned immediately.
    /// `bubble_tasks` is a per-core runtime library, so the task will be executed on the current core.
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> bubble_tasks::runtime::JoinHandle<()>;
    /// Dispatch a task to the task system, and return a future to it.
    fn dispatch(
        &self,
        fut: async_ffi::FfiFuture<()>,
    ) -> async_ffi::FfiFuture<Result<(), futures_channel::oneshot::Canceled>>;
    fn dispatch_blocking(
        &self,
        func: Box<dyn FnOnce() -> () + Send + 'static>,
    ) -> async_ffi::FfiFuture<Result<(), futures_channel::oneshot::Canceled>>;
    /// Shutdown the task system.
    ///
    /// Use a temp unsafe implementation to check atomic counter
    fn shutdown(&self);
    /// Run the tick task as the current future
    ///
    /// ## Note
    ///
    /// You should not do to much io task in tick task, it should handled by dispatcher,
    /// and then use a channel to notify the tick task.
    ///
    /// 2 or more tick task may be executed parallelly on different threads, so you should be careful about the data race.
    unsafe fn tick(&self, tick_task: async_ffi::LocalFfiFuture<bool>) -> bool;
}

impl TaskSystemApi for TaskSystem {
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> bubble_tasks::runtime::JoinHandle<()> {
        bubble_tasks::runtime::spawn(fut)
    }

    fn dispatch(
        &self,
        fut: async_ffi::FfiFuture<()>,
    ) -> async_ffi::FfiFuture<Result<(), futures_channel::oneshot::Canceled>> {
        let result = self
            .dispatcher
            .load()
            .unwrap()
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .dispatch(|| fut)
            .unwrap();
        result.into_ffi()
    }

    fn dispatch_blocking(
        &self,
        func: Box<dyn FnOnce() -> () + Send + 'static>,
    ) -> async_ffi::FfiFuture<Result<(), futures_channel::oneshot::Canceled>> {
        let result = self
            .dispatcher
            .load()
            .unwrap()
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .dispatch_blocking(func)
            .unwrap();
        result.into_ffi()
    }

    fn shutdown(&self) {
        let mut dispatcher = self.dispatcher.load();
        let null_arc = bubble_core::sync::Arc::new(std::sync::RwLock::new(None));

        loop {
            let dispatcher_ptr = dispatcher.as_ref().map_or(std::ptr::null(), AsPtr::as_ptr);
            match self
                .dispatcher
                .compare_exchange(dispatcher_ptr, Some(&null_arc))
            {
                Ok(()) => break,
                Err(before) => dispatcher = before,
            }
        }

        if let Some(dispatcher) = dispatcher {
            println!("in shutdown");
            let x = dispatcher.write().unwrap().take().unwrap();
            let runtime = bubble_tasks::runtime::Runtime::new().unwrap();
            let _ = runtime.block_on(async move { x.join().await });
        }
    }

    unsafe fn tick(&self, tick_task: async_ffi::LocalFfiFuture<bool>) -> bool {
        let r = self.main_runtime.0.enter(|| {
            // it's ok to enter next frame? we allow max x frame to be queued
            // or we'll block the thread
            if let Ok(result) = self.tick_reader.try_recv() {
                println!("receive a tick result:{}", result);
                // we are allowed to queue next tick, and the result return by tick_task is true(continue loop)
                if result {
                    println!("queue a tick");
                    // deatch queue next tick
                    unsafe {
                        self.main_runtime
                            .0
                            .spawn_unchecked(async {
                                self.tick_sender.send(tick_task.await).unwrap();
                            })
                            .detach();
                    }
                    // loop till current has no more tasks
                    loop {
                        self.main_runtime.0.poll_with(Some(Duration::ZERO));
                        let remaining_tasks = self.main_runtime.0.run();
                        if !remaining_tasks {
                            break;
                        }
                    }
                    return true;
                } else {
                    println!("tick task return a end loop");
                    // loop the runtime untill all tasks are done
                    loop {
                        // self.main_runtime.0.poll_with(Some(Duration::ZERO));
                        let remaining_tasks = self.main_runtime.0.run();
                        if let Err(x) = self.tick_reader.try_recv() {
                            println!("{:?}", x);
                            break;
                        }
                        let mut timeout = if remaining_tasks {
                            Some(Duration::ZERO)
                        } else {
                            self.main_runtime.0.current_timeout()
                        };
                        if let None = timeout {
                            timeout = Some(Duration::ZERO)
                        }
                        self.main_runtime.0.poll_with(timeout);
                    }
                    debug_assert_eq!(self.tick_reader.len(), 0);
                    return false;
                }
            } else {
                // we can't queue next tick, so we just loop the runtime
                println!("tick queue blocked");
                loop {
                    // self.main_runtime.0.poll_with(Some(Duration::ZERO));
                    // some tick task finish
                    print!("{}", self.tick_reader.len());
                    if self.tick_reader.len() != 0 {
                        break;
                    }
                    let remaining_tasks = self.main_runtime.0.run();
                    print!("{}", remaining_tasks);
                    let mut timeout = if remaining_tasks {
                        Some(Duration::ZERO)
                    } else {
                        self.main_runtime.0.current_timeout()
                    };
                    if let None = timeout {
                        timeout = Some(Duration::ZERO)
                    }
                    println!("timeout({:?})", timeout);
                    self.main_runtime.0.poll_with(timeout);
                }
                println!("end blocked");
                return true;
            }
        });
        r
    }
}

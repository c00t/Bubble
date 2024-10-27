#![feature(ptr_metadata)]
#![feature(once_cell_get_mut)]
use std::num::NonZero;
use std::sync::atomic::AtomicBool;
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
use bubble_tasks::AffinityHint;
use futures_channel::oneshot;
use hwlocality::cpu::binding::CpuBindingFlags;
use hwlocality::cpu::cpuset::CpuSet;
use hwlocality::cpu::kind::{CpuEfficiency, CpuKind};
use hwlocality::object::depth::NormalDepth;
use hwlocality::topology::support::{CpuBindingSupport, DiscoverySupport};
use hwlocality::Topology;

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

#[derive(Debug, Clone)]
pub struct CancellableTicket(std::sync::Arc<(AtomicBool, AtomicBool)>);

impl CancellableTicket {
    pub fn new() -> Self {
        Self(std::sync::Arc::new((
            AtomicBool::new(false),
            AtomicBool::new(false),
        )))
    }

    pub fn set_allow_cancel(&self, allow: bool) {
        self.0 .0.store(allow, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn allow_cancel(&self) -> bool {
        self.0 .0.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.0 .1.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_canceled(&self) -> bool {
        self.0 .1.load(std::sync::atomic::Ordering::Relaxed)
    }
}
#[define_api(TaskSystemApi)]
struct TaskSystem {
    pub performance_dispatcher: AtomicArc<std::sync::RwLock<Option<Dispatcher>>>,
    pub efficiency_dispatcher: Option<AtomicArc<std::sync::RwLock<Option<Dispatcher>>>>,
    pub tick_sender: thingbuf::mpsc::blocking::Sender<bool>,
    pub tick_reader: thingbuf::mpsc::blocking::Receiver<bool>,
    /// a timeout value which wait before tick to next frame.
    pub tick_timeout: std::time::Duration,
    pub main_runtime: MainRuntime,
    pub num_threads: usize,
    next_calcellation_id: std::sync::atomic::AtomicU64,
    cancellable_tickets:
        std::sync::RwLock<std::collections::HashMap<CalcellationId, CancellableTicket>>,
}

#[derive(Debug)]
pub(crate) struct HwCpuBindingLocality {
    pub binding_support: CpuBindingSupport,
    pub discovery_support: DiscoverySupport,
    pub cpu_kinds: Vec<(CpuSet, Option<CpuEfficiency>)>,
}

impl HwCpuBindingLocality {
    pub fn new(topology: &Option<Topology>) -> Option<Self> {
        let topology = topology.as_ref()?;
        let feature_support = topology.feature_support();
        let binding_support = feature_support.cpu_binding()?.clone();
        // check is thread binding supported
        if !binding_support.get_current_thread() {
            return None;
        }
        let discovery_support = feature_support.discovery()?.clone();
        let cpu_kinds = topology
            .cpu_kinds()
            .ok()?
            .map(|kind| {
                let kind = kind.clone();
                (kind.cpuset, kind.efficiency)
            })
            .collect();
        Some(Self {
            binding_support,
            discovery_support,
            cpu_kinds,
        })
    }
    /// Split the cpu kinds into two groups, the first group is the most power-efficient kind,
    /// the second group is the rest kinds. If there is only one kind, the second group will be `None`.
    pub fn split_to_two_kinds(&self) -> (CpuSet, Option<CpuSet>) {
        let mut iter = self.cpu_kinds.iter();
        let (first_set, _) = iter.next().unwrap();
        if self.cpu_kinds.len() == 1 {
            return (first_set.clone(), None);
        }
        let second_set = iter.fold(CpuSet::new(), |acc, (set, _)| acc | set);
        (first_set.clone(), Some(second_set))
    }
}

pub fn get_task_system_api(
    thread_count: (usize, Option<usize>),
    tick_timeout: Duration,
    frame_delay: usize,
) -> ApiHandle<dyn TaskSystemApi> {
    let topology = Topology::new().ok();
    let cpu_locality = HwCpuBindingLocality::new(&topology).and_then(|locality| {
        let (first_set, second_set) = locality.split_to_two_kinds();
        Some((first_set, second_set))
    });

    let (performance_dispatcher, efficiency_dispatcher) = cpu_locality.map_or_else(
        || {
            let p_dispatcher = Dispatcher::builder()
                .thread_names(|index| format!("performance-{index}"))
                .worker_threads(NonZero::new(thread_count.0).unwrap())
                .build()
                .unwrap();
            (p_dispatcher, None)
        },
        |(first_set, second_set)| {
            let topology = topology.unwrap();
            match (first_set, second_set) {
                (first_set, None) => {
                    let topology_clone = topology.clone();
                    let p_dispatcher = Dispatcher::builder()
                        .thread_names(|index| format!("performance-{index}"))
                        .worker_threads(NonZero::new(thread_count.0).unwrap())
                        .build()
                        .unwrap();
                    (p_dispatcher, None)
                }
                (first_set, Some(second_set)) => {
                    let topology_clone = topology.clone();
                    let p_dispatcher = Dispatcher::builder()
                        .thread_names(|index| format!("performance-{index}"))
                        .on_thread_start(move || {
                            topology_clone
                                .bind_cpu(&second_set, CpuBindingFlags::THREAD)
                                .ok();
                        })
                        .worker_threads(NonZero::new(thread_count.0).unwrap())
                        .build()
                        .unwrap();
                    let topology_clone = topology.clone();
                    let e_dispatcher = Dispatcher::builder()
                        .thread_names(|index| format!("efficiency-{index}"))
                        .on_thread_start(move || {
                            topology_clone
                                .bind_cpu(&first_set, CpuBindingFlags::THREAD)
                                .ok();
                        })
                        .worker_threads(
                            NonZero::new(thread_count.1.unwrap_or(thread_count.0)).unwrap(),
                        )
                        .build()
                        .unwrap();
                    (p_dispatcher, Some(e_dispatcher))
                }
            }
        },
    );

    let num_threads = performance_dispatcher.num_threads();
    let (sender, reader) = thingbuf::mpsc::blocking::channel(frame_delay);
    for _ in 0..frame_delay {
        sender.send(true).unwrap();
    }
    // TODO: may be we should use the same proactor builder of the performance dispatchers?
    let main_runtime = MainRuntime(
        bubble_tasks::runtime::RuntimeBuilder::new()
            .name(Some("tick-main".into()))
            .build()
            .unwrap(),
    );
    let task_system_api: AnyApiHandle = Box::new(TaskSystem {
        performance_dispatcher: AtomicArc::new(std::sync::RwLock::new(Some(
            performance_dispatcher,
        ))),
        efficiency_dispatcher: if let Some(e_dispatcher) = efficiency_dispatcher {
            Some(AtomicArc::new(std::sync::RwLock::new(Some(e_dispatcher))))
        } else {
            None
        },
        tick_sender: sender,
        tick_reader: reader,
        tick_timeout,
        main_runtime,
        num_threads,
        next_calcellation_id: std::sync::atomic::AtomicU64::new(0),
        cancellable_tickets: std::sync::RwLock::new(std::collections::HashMap::new()),
    })
    .into();
    task_system_api.downcast()
}

#[bon]
impl TaskSystem {
    #[builder]
    pub fn new() -> ApiHandle<dyn TaskSystemApi> {
        let topology = Topology::new().ok();
        let cpu_locality = HwCpuBindingLocality::new(&topology).and_then(|locality| {
            let (first_set, second_set) = locality.split_to_two_kinds();
            Some((first_set, second_set))
        });

        let (performance_dispatcher, efficiency_dispatcher) = cpu_locality.map_or_else(
            || {
                let p_dispatcher = Dispatcher::builder()
                    .thread_names(|index| format!("performance-{index}"))
                    .build()
                    .unwrap();
                (p_dispatcher, None)
            },
            |(first_set, second_set)| {
                let topology = topology.unwrap();
                match (first_set, second_set) {
                    (first_set, None) => {
                        let topology_clone = topology.clone();
                        let p_dispatcher = Dispatcher::builder()
                            .thread_names(|index| format!("performance-{index}"))
                            .build()
                            .unwrap();
                        (p_dispatcher, None)
                    }
                    (first_set, Some(second_set)) => {
                        let topology_clone = topology.clone();
                        let p_dispatcher = Dispatcher::builder()
                            .thread_names(|index| format!("performance-{index}"))
                            .on_thread_start(move || {
                                topology_clone
                                    .bind_cpu(&second_set, CpuBindingFlags::THREAD)
                                    .ok();
                            })
                            .build()
                            .unwrap();
                        let topology_clone = topology.clone();
                        let e_dispatcher = Dispatcher::builder()
                            .thread_names(|index| format!("efficiency-{index}"))
                            .on_thread_start(move || {
                                topology_clone
                                    .bind_cpu(&first_set, CpuBindingFlags::THREAD)
                                    .ok();
                            })
                            .build()
                            .unwrap();
                        (p_dispatcher, Some(e_dispatcher))
                    }
                }
            },
        );

        let num_threads = performance_dispatcher.num_threads();
        let (sender, reader) = thingbuf::mpsc::blocking::channel(3);
        for _ in 0..3 {
            sender.send(true).unwrap();
        }
        let main_runtime = MainRuntime(
            bubble_tasks::runtime::RuntimeBuilder::new()
                .name(Some("tick-main".into()))
                .build()
                .unwrap(),
        );
        let task_system_api: AnyApiHandle = Box::new(TaskSystem {
            performance_dispatcher: AtomicArc::new(std::sync::RwLock::new(Some(
                performance_dispatcher,
            ))),
            efficiency_dispatcher: if let Some(e_dispatcher) = efficiency_dispatcher {
                Some(AtomicArc::new(std::sync::RwLock::new(Some(e_dispatcher))))
            } else {
                None
            },
            tick_sender: sender,
            tick_reader: reader,
            tick_timeout: Duration::from_millis(1000 / 60), // 60 fps
            main_runtime,
            num_threads,
            next_calcellation_id: std::sync::atomic::AtomicU64::new(0),
            cancellable_tickets: std::sync::RwLock::new(std::collections::HashMap::new()),
        })
        .into();
        task_system_api.downcast()
    }

    fn get_next_calcellation_id(&self) -> CalcellationId {
        let id = self
            .next_calcellation_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        CalcellationId(id)
    }

    fn create_cancellable_task<F, Fut>(
        &self,
        make_fut: F,
    ) -> (CalcellationId, CancellableTicket, Fut)
    where
        F: FnOnce(CalcellationId) -> Fut,
    {
        let calcellation_id = self.get_next_calcellation_id();
        let ticket = CancellableTicket::new();
        // Store the cancel flag
        self.cancellable_tickets
            .write()
            .unwrap()
            .insert(calcellation_id, ticket.clone());

        let future = make_fut(calcellation_id);
        (calcellation_id, ticket, future)
    }
}

/// A unique id for a task.
///
/// Currently it's only generated for cancelable task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CalcellationId(u64);

/// Task system api
///
/// Run async task on the task system. Because the task system will be used by ui,
/// so the api provide a cancelable friendly spawn function. The closure passed to the function
/// take a `TaskId` as parameter, which can be used to cancel the task cooperatively, you will need to
/// explictly allow cancellation in your async function if you want systems outside your code can cancel it.
#[declare_api((0,1,0), shared::TaskSystemApi)]
pub trait TaskSystemApi: Api {
    /// Spawn a task on the current thread, and return a handle to it.
    ///
    /// The task will be executed on the current thread, and the handle will be returned immediately.
    /// `bubble_tasks` is a per-core runtime library, so the task will be executed on the current core.
    ///
    /// ## TODO
    ///
    /// Add opaque type to the return type of the future, which may be a struct or an enum.
    ///
    /// ## NOTE
    ///
    /// You can drop the future returned by this function, the task will be cancelled.
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> async_ffi::LocalFfiFuture<()>;

    fn spawn_cancelable(
        &self,
        make_fut: Box<dyn FnOnce(CalcellationId) -> async_ffi::LocalFfiFuture<()> + Send + 'static>,
    ) -> (CalcellationId, async_ffi::LocalFfiFuture<()>);

    /// Spawn a blocking task, and return a future to wait for the task to finish.
    ///
    /// ## NOTE
    ///
    /// If you drop the future returned by this function, the task will be cancelled.
    /// But typically the func you passed to this function is a cpu-bound task, so the task will be executed to the end once it started.
    fn spawn_blocking(
        &self,
        func: Box<dyn FnOnce() -> () + Send + Sync + 'static>,
    ) -> async_ffi::FfiFuture<()>;

    /// Spawn a task in the background, you don't need to wait for the task to finish,
    /// it will run in the background.
    fn spawn_detached(&self, fut: async_ffi::LocalFfiFuture<()>);

    fn spawn_detached_cancelable(
        &self,
        make_fut: Box<dyn FnOnce(CalcellationId) -> async_ffi::LocalFfiFuture<()> + Send + 'static>,
    ) -> CalcellationId;

    /// Dispatch a task to the task system, and return a receiver to it.
    ///
    /// ## Note
    ///
    /// whether the task is finished or not, just ignore the [`oneshot::Receiver`]. It doesn't matter whether the [`oneshot::Receiver`] is dropped or not,
    /// the task will be executed to the end anyway.
    ///
    /// The task system should just ignore the error returned from [`oneshot::Sender::send`].
    ///
    /// ## Affinity hint
    ///
    /// If the underlying implementation supports affinity hint, the task system will
    /// try to run the task on the thread pool which bind to cores with the same affinity hint.
    fn dispatch(
        &self,
        affinity_hint: Option<bubble_tasks::AffinityHint>,
        fut: async_ffi::FfiFuture<()>,
    ) -> Result<oneshot::Receiver<()>, ()>;

    fn dispatch_cancelable(
        &self,
        affinity_hint: Option<bubble_tasks::AffinityHint>,
        make_fut: Box<dyn FnOnce(CalcellationId) -> async_ffi::FfiFuture<()> + Send + 'static>,
    ) -> Result<(CalcellationId, oneshot::Receiver<()>), ()>;

    fn dispatch_blocking(
        &self,
        affinity_hint: Option<bubble_tasks::AffinityHint>,
        func: Box<dyn FnOnce() -> () + Send + 'static>,
    ) -> Result<oneshot::Receiver<()>, ()>;

    /// Explicitly set a shared internal state to allow the task to be cancelled.
    fn allow_cancel(&self, task_id: CalcellationId) -> bool;

    fn cancel(&self, task_id: CalcellationId);

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
    /// Get the number of worker threads in the task system dispatcher.
    fn num_threads(&self) -> usize;
}

impl TaskSystemApi for TaskSystem {
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> async_ffi::LocalFfiFuture<()> {
        futures_util::FutureExt::map(bubble_tasks::runtime::spawn(fut), |r| r.unwrap())
            .into_local_ffi()
    }

    fn spawn_blocking(
        &self,
        func: Box<dyn FnOnce() -> () + Send + Sync + 'static>,
    ) -> async_ffi::FfiFuture<()> {
        futures_util::FutureExt::map(bubble_tasks::runtime::spawn_blocking(func), |r| r.unwrap())
            .into_ffi()
    }

    fn spawn_detached(&self, fut: async_ffi::LocalFfiFuture<()>) {
        bubble_tasks::runtime::spawn(fut).detach();
    }

    fn dispatch(
        &self,
        affinity_hint: Option<bubble_tasks::AffinityHint>,
        fut: async_ffi::FfiFuture<()>,
    ) -> Result<oneshot::Receiver<()>, ()> {
        if affinity_hint.is_none()
            || self.efficiency_dispatcher.is_none()
            || affinity_hint == Some(AffinityHint::Performance)
        {
            let result = self
                .performance_dispatcher
                .load()
                .unwrap()
                .read()
                .expect("Can't get the read lock on performance dispatcher")
                .as_ref()
                .expect("Performance dispatcher has been shutdown or not initialized")
                .dispatch(|| fut)
                .unwrap();
            Ok(result)
        } else {
            let result = self
                .efficiency_dispatcher
                .as_ref()
                .unwrap()
                .load()
                .unwrap()
                .read()
                .expect("Can't get the read lock on efficiency dispatcher")
                .as_ref()
                .expect("Efficiency dispatcher has been shutdown or not initialized")
                .dispatch(|| fut)
                .unwrap();
            Ok(result)
        }
    }

    fn dispatch_blocking(
        &self,
        affinity_hint: Option<bubble_tasks::AffinityHint>,
        func: Box<dyn FnOnce() -> () + Send + 'static>,
    ) -> Result<oneshot::Receiver<()>, ()> {
        if affinity_hint.is_none()
            || self.efficiency_dispatcher.is_none()
            || affinity_hint == Some(AffinityHint::Performance)
        {
            let result = self
                .performance_dispatcher
                .load()
                .unwrap()
                .read()
                .unwrap()
                .as_ref()
                .expect("Performance dispatcher has been shutdown or not initialized")
                .dispatch_blocking(func)
                .unwrap();
            Ok(result)
        } else {
            let result = self
                .efficiency_dispatcher
                .as_ref()
                .unwrap()
                .load()
                .unwrap()
                .read()
                .unwrap()
                .as_ref()
                .expect("Efficiency dispatcher has been shutdown or not initialized")
                .dispatch_blocking(func)
                .unwrap();
            Ok(result)
        }
    }

    fn shutdown(&self) {
        // Shutdown performance dispatcher
        let mut p_dispatcher = self.performance_dispatcher.load();
        let null_arc = bubble_core::sync::Arc::new(std::sync::RwLock::new(None));

        // Exchange performance dispatcher with None
        loop {
            let dispatcher_ptr = p_dispatcher
                .as_ref()
                .map_or(std::ptr::null(), AsPtr::as_ptr);
            match self
                .performance_dispatcher
                .compare_exchange(dispatcher_ptr, Some(&null_arc))
            {
                Ok(()) => break,
                Err(before) => p_dispatcher = before,
            }
        }

        // Shutdown efficiency dispatcher if it exists
        let mut e_dispatcher = self.efficiency_dispatcher.as_ref().map(|d| d.load());
        if let Some(ref mut e_dispatcher) = e_dispatcher {
            loop {
                let dispatcher_ptr = e_dispatcher
                    .as_ref()
                    .map_or(std::ptr::null(), AsPtr::as_ptr);
                match self
                    .efficiency_dispatcher
                    .as_ref()
                    .unwrap()
                    .compare_exchange(dispatcher_ptr, Some(&null_arc))
                {
                    Ok(()) => break,
                    Err(before) => *e_dispatcher = before,
                }
            }
        }

        // Create runtime for cleanup
        let runtime = bubble_tasks::runtime::Runtime::new().unwrap();

        // Join performance dispatcher
        if let Some(dispatcher) = p_dispatcher {
            println!("shutting down performance dispatcher");
            let x = dispatcher.write().unwrap().take().unwrap();
            let _ = runtime.block_on(async move { x.join().await });
        }

        // Join efficiency dispatcher
        if let Some(dispatcher) = e_dispatcher.flatten() {
            println!("shutting down efficiency dispatcher");
            let x = dispatcher.write().unwrap().take().unwrap();
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

                    // get a timer, which shouldn't panic, i don't care it's value.
                    let (tx, mut rx) = futures_channel::oneshot::channel();

                    unsafe {
                        self.main_runtime
                            .0
                            .spawn_unchecked(async {
                                let timer_task = async {
                                    bubble_tasks::runtime::time::sleep(self.tick_timeout).await;
                                    tx.send(()).expect("frame timeout receiver dropped");
                                };
                                let (_, b) = futures_util::join!(timer_task, tick_task);
                                self.tick_sender.send(b).unwrap();
                            })
                            .detach();
                    }
                    // loop till current has no more tasks
                    loop {
                        let remaining_tasks = self.main_runtime.0.run();
                        let timeout = rx.try_recv();
                        if let Ok(Some(())) = timeout {
                            // if timeout, exit loop
                            println!("tick timeout, quit tick");
                            break;
                        } else if let Err(e) = timeout {
                            println!("tick timeout error");
                            if !remaining_tasks {
                                println!("timeout error, quit tick");
                                break;
                            }
                        }
                        if remaining_tasks {
                            self.main_runtime.0.poll_with(Some(Duration::ZERO));
                        } else {
                            self.main_runtime.0.poll();
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
                    println!("tick queue len: {}", self.tick_reader.len());
                    if self.tick_reader.len() != 0 {
                        break;
                    }
                    let remaining_tasks = self.main_runtime.0.run();
                    println!("{}", remaining_tasks);
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

    /// Get the number of worker threads in the task system dispatcher.
    fn num_threads(&self) -> usize {
        self.num_threads
    }

    fn spawn_cancelable(
        &self,
        make_fut: Box<dyn FnOnce(CalcellationId) -> async_ffi::LocalFfiFuture<()> + Send + 'static>,
    ) -> (CalcellationId, async_ffi::LocalFfiFuture<()>) {
        let (task_id, ticket, mut fut) = self.create_cancellable_task(make_fut);

        let wrapped_fut = async move {
            futures_util::future::poll_fn(|cx| {
                if ticket.allow_cancel() && ticket.is_canceled() {
                    return std::task::Poll::Ready(());
                }
                futures_util::FutureExt::poll_unpin(&mut fut, cx)
            })
            .await
        };

        (task_id, wrapped_fut.into_local_ffi())
    }

    fn spawn_detached_cancelable(
        &self,
        make_fut: Box<dyn FnOnce(CalcellationId) -> async_ffi::LocalFfiFuture<()> + Send + 'static>,
    ) -> CalcellationId {
        let (ticket, fut) = self.spawn_cancelable(make_fut);
        self.spawn_detached(fut);
        ticket
    }

    fn dispatch_cancelable(
        &self,
        affinity_hint: Option<bubble_tasks::AffinityHint>,
        make_fut: Box<dyn FnOnce(CalcellationId) -> async_ffi::FfiFuture<()> + Send + 'static>,
    ) -> Result<(CalcellationId, oneshot::Receiver<()>), ()> {
        let (calcellation_id, ticket, mut future) = self.create_cancellable_task(make_fut);
        let wrapped_fut = async move {
            futures_util::future::poll_fn(|cx| {
                if ticket.allow_cancel() && ticket.is_canceled() {
                    return std::task::Poll::Ready(());
                }
                futures_util::FutureExt::poll_unpin(&mut future, cx)
            })
            .await
        };

        let result = self
            .dispatch(affinity_hint, wrapped_fut.into_ffi())
            .unwrap();
        Ok((calcellation_id, result))
    }

    #[doc = " Explicitly set a shared internal state to allow the task to be cancelled."]
    fn allow_cancel(&self, task_id: CalcellationId) -> bool {
        let ticket = self.cancellable_tickets.read().unwrap();
        let ticket = ticket.get(&task_id).unwrap();
        ticket.allow_cancel()
    }

    fn cancel(&self, task_id: CalcellationId) {
        let ticket = self.cancellable_tickets.read().unwrap();
        let ticket = ticket.get(&task_id).unwrap();
        ticket.cancel();
    }
}

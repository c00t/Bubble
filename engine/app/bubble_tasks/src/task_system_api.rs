use std::num::NonZero;
use std::ops::Deref;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};
use std::time::Duration;

use crate::dispatcher::Dispatcher;
use crate::futures_channel::oneshot;
use crate::types::{
    AffinityHint, CalcellationId, CancellableTicket, DynTaskSystemApi, TaskSystemApi,
};
use async_ffi::FutureExt;
use bubble_core::tracing::info;
use bubble_core::{api::prelude::*, sync::circ};
use hwlocality::cpu::binding::CpuBindingFlags;
use hwlocality::cpu::cpuset::CpuSet;
use hwlocality::cpu::kind::CpuEfficiency;
use hwlocality::topology::support::{CpuBindingSupport, DiscoverySupport};
use hwlocality::Topology;

struct MainRuntime(crate::runtime::Runtime);

unsafe impl Send for MainRuntime {}
unsafe impl Sync for MainRuntime {}

struct RwLockDiapatcher(std::sync::RwLock<Option<Dispatcher>>);

impl RwLockDiapatcher {
    pub fn new(dispatcher: Dispatcher) -> Self {
        Self(std::sync::RwLock::new(Some(dispatcher)))
    }
}

unsafe impl circ::RcObject for RwLockDiapatcher {
    fn pop_edges(&mut self, out: &mut Vec<circ::Rc<Self>>) {
        // no need to track edges
    }
}

impl Deref for RwLockDiapatcher {
    type Target = std::sync::RwLock<Option<Dispatcher>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[define_api(TaskSystemApi)]
struct TaskSystem {
    pub performance_dispatcher: circ::AtomicRc<RwLockDiapatcher>,
    pub efficiency_dispatcher: Option<circ::AtomicRc<RwLockDiapatcher>>,
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
        crate::runtime::RuntimeBuilder::new()
            .name(Some("tick-main".into()))
            .build()
            .unwrap(),
    );
    let task_system_api: AnyApiHandle = Box::new(TaskSystem {
        performance_dispatcher: circ::AtomicRc::new(RwLockDiapatcher::new(performance_dispatcher)),
        efficiency_dispatcher: if let Some(e_dispatcher) = efficiency_dispatcher {
            Some(circ::AtomicRc::new(RwLockDiapatcher::new(e_dispatcher)))
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

pub fn get_task_system_api_default() -> ApiHandle<dyn TaskSystemApi> {
    TaskSystem::new()
}

impl TaskSystem {
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
            crate::runtime::RuntimeBuilder::new()
                .name(Some("tick-main".into()))
                .build()
                .unwrap(),
        );
        let task_system_api: AnyApiHandle = Box::new(TaskSystem {
            performance_dispatcher: circ::AtomicRc::new(RwLockDiapatcher::new(
                performance_dispatcher,
            )),
            efficiency_dispatcher: if let Some(e_dispatcher) = efficiency_dispatcher {
                Some(circ::AtomicRc::new(RwLockDiapatcher::new(e_dispatcher)))
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
        let id = self.next_calcellation_id.fetch_add(1, Relaxed);
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

impl TaskSystemApi for TaskSystem {
    fn spawn(&self, fut: async_ffi::LocalFfiFuture<()>) -> async_ffi::LocalFfiFuture<()> {
        futures_util::FutureExt::map(crate::runtime::spawn(fut), |r| r.unwrap()).into_local_ffi()
    }

    fn spawn_blocking(
        &self,
        func: Box<dyn FnOnce() -> () + Send + Sync + 'static>,
    ) -> async_ffi::FfiFuture<()> {
        futures_util::FutureExt::map(crate::runtime::spawn_blocking(func), |r| r.unwrap())
            .into_ffi()
    }

    fn spawn_detached(&self, fut: async_ffi::LocalFfiFuture<()>) {
        crate::runtime::spawn(fut).detach();
    }

    fn dispatch(
        &self,
        affinity_hint: Option<AffinityHint>,
        fut: async_ffi::FfiFuture<()>,
    ) -> Result<oneshot::Receiver<()>, ()> {
        let guard = circ::cs();
        if affinity_hint.is_none()
            || self.efficiency_dispatcher.is_none()
            || affinity_hint == Some(AffinityHint::Performance)
        {
            let result = self
                .performance_dispatcher
                .load(Relaxed, &guard)
                .as_ref()
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
                .load(Relaxed, &guard)
                .as_ref()
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
        affinity_hint: Option<AffinityHint>,
        func: Box<dyn FnOnce() -> () + Send + 'static>,
    ) -> Result<oneshot::Receiver<()>, ()> {
        let guard = circ::cs();
        if affinity_hint.is_none()
            || self.efficiency_dispatcher.is_none()
            || affinity_hint == Some(AffinityHint::Performance)
        {
            let result = self
                .performance_dispatcher
                .load(Relaxed, &guard)
                .as_ref()
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
                .load(Relaxed, &guard)
                .as_ref()
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
        let guard = circ::cs();

        let mut p_dispatcher_snapshot = self.performance_dispatcher.load(Acquire, &guard);
        loop {
            match self.performance_dispatcher.compare_exchange(
                p_dispatcher_snapshot,
                circ::Rc::null(),
                AcqRel,
                Acquire,
                &guard,
            ) {
                Ok(_) => break,
                Err(err) => p_dispatcher_snapshot = err.current,
            }
        }

        // Shutdown efficiency dispatcher if it exists
        let mut e_dispatcher_snapshot = self
            .efficiency_dispatcher
            .as_ref()
            .map(|d| d.load(Acquire, &guard));
        if let Some(ref mut e_dispatcher) = e_dispatcher_snapshot {
            loop {
                match self
                    .efficiency_dispatcher
                    .as_ref()
                    .unwrap()
                    .compare_exchange(*e_dispatcher, circ::Rc::null(), AcqRel, AcqRel, &guard)
                {
                    Ok(_) => break,
                    Err(err) => *e_dispatcher = err.current,
                }
            }
        }

        // Create runtime for cleanup
        let runtime = crate::runtime::Runtime::new().unwrap();

        // Join performance dispatcher
        p_dispatcher_snapshot.as_ref().map(|dispatcher| {
            info!("shutting down performance dispatcher");
            let x = dispatcher.write().unwrap().take().unwrap();
            let _ = runtime.block_on(async move { x.join().await });
        });

        // Join efficiency dispatcher
        e_dispatcher_snapshot
            .as_ref()
            .and_then(|dispatcher| dispatcher.as_ref())
            .map(|dispatcher| {
                info!("shutting down efficiency dispatcher");
                let x = dispatcher.write().unwrap().take().unwrap();
                let _ = runtime.block_on(async move { x.join().await });
            });
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
                                    crate::runtime::time::sleep(self.tick_timeout).await;
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
        affinity_hint: Option<AffinityHint>,
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

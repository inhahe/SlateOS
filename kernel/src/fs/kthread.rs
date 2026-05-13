//! Kernel Thread — kernel thread lifecycle monitoring.
//!
//! Tracks kernel thread creation, destruction, CPU affinity,
//! and activity state. Essential for understanding kernel
//! background work and workqueue threads.
//!
//! ## Architecture
//!
//! ```text
//! Kernel thread monitoring
//!   → kthread::register(name, cpu) → track new kernel thread
//!   → kthread::unregister(id) → track thread exit
//!   → kthread::set_state(id, state) → update thread state
//!   → kthread::list() → list kernel threads
//!
//! Integration:
//!   → wqstat (workqueue stats)
//!   → cpustat (CPU utilization)
//!   → schedclass (scheduler class)
//!   → softirq (soft interrupts)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Kernel thread state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KthreadState {
    Running,
    Sleeping,
    Idle,
    Parked,
    Exiting,
}

impl KthreadState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Sleeping => "sleeping",
            Self::Idle => "idle",
            Self::Parked => "parked",
            Self::Exiting => "exiting",
        }
    }
}

/// Kernel thread info.
#[derive(Debug, Clone)]
pub struct KernelThread {
    pub id: u32,
    pub name: String,
    pub cpu: u32,
    pub state: KthreadState,
    pub cpu_time_ns: u64,
    pub wakeups: u64,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_KTHREADS: usize = 256;

struct State {
    threads: Vec<KernelThread>,
    next_id: u32,
    total_created: u64,
    total_exited: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        threads: alloc::vec![
            KernelThread { id: 1, name: String::from("kswapd0"), cpu: 0, state: KthreadState::Sleeping, cpu_time_ns: 500_000_000, wakeups: 100_000, created_ns: now },
            KernelThread { id: 2, name: String::from("ksoftirqd/0"), cpu: 0, state: KthreadState::Sleeping, cpu_time_ns: 200_000_000, wakeups: 500_000, created_ns: now },
            KernelThread { id: 3, name: String::from("kworker/0:0"), cpu: 0, state: KthreadState::Idle, cpu_time_ns: 1_000_000_000, wakeups: 1_000_000, created_ns: now },
            KernelThread { id: 4, name: String::from("kworker/1:0"), cpu: 1, state: KthreadState::Idle, cpu_time_ns: 900_000_000, wakeups: 900_000, created_ns: now },
            KernelThread { id: 5, name: String::from("writeback"), cpu: 0, state: KthreadState::Sleeping, cpu_time_ns: 100_000_000, wakeups: 50_000, created_ns: now },
        ],
        next_id: 6,
        total_created: 100,
        total_exited: 95,
        ops: 0,
    });
}

/// Register a kernel thread.
pub fn register(name: &str, cpu: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.threads.len() >= MAX_KTHREADS { return Err(KernelError::ResourceExhausted); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.total_created += 1;
        state.threads.push(KernelThread {
            id, name: String::from(name), cpu, state: KthreadState::Running,
            cpu_time_ns: 0, wakeups: 0, created_ns: now,
        });
        Ok(id)
    })
}

/// Unregister (exit) a kernel thread.
pub fn unregister(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.threads.iter().position(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        state.threads.remove(idx);
        state.total_exited += 1;
        Ok(())
    })
}

/// Set thread state.
pub fn set_state(id: u32, new_state: KthreadState) -> KernelResult<()> {
    with_state(|state| {
        let t = state.threads.iter_mut().find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        t.state = new_state;
        if new_state == KthreadState::Running { t.wakeups += 1; }
        Ok(())
    })
}

/// Record CPU time.
pub fn record_cpu_time(id: u32, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let t = state.threads.iter_mut().find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        t.cpu_time_ns += ns;
        Ok(())
    })
}

/// List all kernel threads.
pub fn list() -> Vec<KernelThread> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.threads.clone())
}

/// Threads on a specific CPU.
pub fn on_cpu(cpu: u32) -> Vec<KernelThread> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.threads.iter().filter(|t| t.cpu == cpu).cloned().collect()
    })
}

/// Statistics: (thread_count, total_created, total_exited, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.threads.len(), s.total_created, s.total_exited, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("kthread::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 5);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    let id = register("test_kthread", 2).expect("register");
    assert!(id >= 6);
    assert_eq!(list().len(), 6);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Set state.
    set_state(id, KthreadState::Sleeping).expect("state");
    let t = list().iter().find(|t| t.id == id).cloned().unwrap();
    assert_eq!(t.state, KthreadState::Sleeping);
    crate::serial_println!("  [3/8] state: OK");

    // 4: Wakeup counting.
    set_state(id, KthreadState::Running).expect("wake");
    let t = list().iter().find(|t| t.id == id).cloned().unwrap();
    assert_eq!(t.wakeups, 1);
    crate::serial_println!("  [4/8] wakeups: OK");

    // 5: CPU time.
    record_cpu_time(id, 50_000).expect("cpu_time");
    let t = list().iter().find(|t| t.id == id).cloned().unwrap();
    assert_eq!(t.cpu_time_ns, 50_000);
    crate::serial_println!("  [5/8] cpu time: OK");

    // 6: On CPU.
    let cpu2 = on_cpu(2);
    assert!(cpu2.len() >= 1);
    crate::serial_println!("  [6/8] on cpu: OK");

    // 7: Unregister.
    unregister(id).expect("unregister");
    assert_eq!(list().len(), 5);
    assert!(unregister(id).is_err());
    crate::serial_println!("  [7/8] unregister: OK");

    // 8: Stats.
    let (threads, created, exited, ops) = stats();
    assert_eq!(threads, 5);
    assert!(created > 100);
    assert!(exited > 95);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("kthread::self_test() — all 8 tests passed");
}

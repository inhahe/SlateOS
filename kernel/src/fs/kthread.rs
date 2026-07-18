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
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise the kernel-thread tracking state.
///
/// Starts with no tracked threads and zero created/exited totals. The
/// `/proc/kthread` generator and the `kthread` kshell command surface this
/// list (and `on_cpu`) as if it reflects the real set of running kernel
/// threads, so seeding it with phantom threads would be fabricated procfs
/// data — it would claim kernel threads exist that nothing actually spawned.
/// Kernel threads are registered through [`register`] when they are created
/// and removed through [`unregister`] on exit; counters and per-thread
/// activity advance only through real [`register`] / [`unregister`] /
/// [`set_state`] / [`record_cpu_time`] calls.
///
/// (Previously this seeded five fictional kernel threads — "kswapd0",
/// "ksoftirqd/0", "kworker/0:0", "kworker/1:0", and "writeback" — with
/// invented CPU times and wakeup counts, plus totals of 100 created / 95
/// exited.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        threads: Vec::new(),
        next_id: 1,
        total_created: 0,
        total_exited: 0,
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
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live kernel-thread list afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom threads, zero totals.
    assert_eq!(list().len(), 0);
    let (threads0, created0, exited0, _) = stats();
    assert_eq!((threads0, created0, exited0), (0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register — id starts at 1, thread begins Running with zeroed activity.
    let id = register("test_kthread", 2).expect("register");
    assert_eq!(id, 1);
    assert_eq!(list().len(), 1);
    let t = list().into_iter().find(|t| t.id == id).expect("find");
    assert_eq!(t.state, KthreadState::Running);
    assert_eq!((t.cpu_time_ns, t.wakeups), (0, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Set state — a non-Running transition does not count a wakeup.
    set_state(id, KthreadState::Sleeping).expect("state");
    let t = list().into_iter().find(|t| t.id == id).expect("f3");
    assert_eq!(t.state, KthreadState::Sleeping);
    assert_eq!(t.wakeups, 0);
    crate::serial_println!("  [3/8] state: OK");

    // 4: Wakeup counting — a transition to Running increments wakeups.
    set_state(id, KthreadState::Running).expect("wake");
    assert_eq!(list().into_iter().find(|t| t.id == id).expect("f4").wakeups, 1);
    crate::serial_println!("  [4/8] wakeups: OK");

    // 5: CPU time accumulates exactly.
    record_cpu_time(id, 50_000).expect("cpu_time");
    assert_eq!(list().into_iter().find(|t| t.id == id).expect("f5").cpu_time_ns, 50_000);
    crate::serial_println!("  [5/8] cpu time: OK");

    // 6: on_cpu filters by CPU. Register a second thread on a different CPU.
    let id2 = register("other_kthread", 5).expect("register2");
    assert_eq!(id2, 2);
    assert_eq!(on_cpu(2).len(), 1);
    assert_eq!(on_cpu(5).len(), 1);
    assert_eq!(on_cpu(99).len(), 0);
    crate::serial_println!("  [6/8] on cpu: OK");

    // 7: Unregister removes the thread; double/unknown unregister is NotFound.
    unregister(id).expect("unregister");
    assert_eq!(list().len(), 1); // id2 remains
    assert!(unregister(id).is_err());
    assert!(unregister(9999).is_err());
    crate::serial_println!("  [7/8] unregister: OK");

    // 8: Final stats reflect only the real activity above.
    let (threads, created, exited, ops) = stats();
    assert_eq!(threads, 1);   // id2 still tracked
    assert_eq!(created, 2);   // two registers
    assert_eq!(exited, 1);    // one unregister
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("kthread::self_test() — all 8 tests passed");
}

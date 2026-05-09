//! Workqueue Statistics — kernel workqueue monitoring.
//!
//! Tracks work items queued, executed, and pending across
//! kernel workqueues. Monitors queue depth, latency, and
//! worker thread utilization.
//!
//! ## Architecture
//!
//! ```text
//! Workqueue stats
//!   → wqstat::list() → list workqueues
//!   → wqstat::enqueue(wq, work) → record enqueue
//!   → wqstat::complete(wq, work) → record completion
//!   → wqstat::summary() → system summary
//!
//! Integration:
//!   → perfmon (performance monitor)
//!   → taskmon (task monitor)
//!   → tracemon (trace monitor)
//!   → schedtune (scheduler tuning)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Workqueue type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WqType {
    Bound,       // Per-CPU bound.
    Unbound,     // System-wide.
    Highpri,     // High priority.
    Ordered,     // Serialized execution.
}

impl WqType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bound => "bound",
            Self::Unbound => "unbound",
            Self::Highpri => "highpri",
            Self::Ordered => "ordered",
        }
    }
}

/// A workqueue entry.
#[derive(Debug, Clone)]
pub struct Workqueue {
    pub id: u32,
    pub name: String,
    pub wq_type: WqType,
    pub pending: u64,
    pub active: u64,
    pub completed: u64,
    pub cancelled: u64,
    pub max_pending: u64,
    pub avg_latency_us: u64,
    pub max_latency_us: u64,
    pub workers: u32,
    pub cpu_affinity: Option<u32>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_WORKQUEUES: usize = 128;

struct State {
    queues: Vec<Workqueue>,
    next_id: u32,
    total_enqueued: u64,
    total_completed: u64,
    total_cancelled: u64,
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
    *guard = Some(State {
        queues: alloc::vec![
            Workqueue { id: 1, name: String::from("events"), wq_type: WqType::Bound, pending: 3, active: 1, completed: 15000, cancelled: 5, max_pending: 32, avg_latency_us: 50, max_latency_us: 5000, workers: 4, cpu_affinity: None },
            Workqueue { id: 2, name: String::from("events_highpri"), wq_type: WqType::Highpri, pending: 0, active: 0, completed: 2000, cancelled: 0, max_pending: 8, avg_latency_us: 10, max_latency_us: 500, workers: 4, cpu_affinity: None },
            Workqueue { id: 3, name: String::from("kblockd"), wq_type: WqType::Bound, pending: 5, active: 2, completed: 50000, cancelled: 10, max_pending: 64, avg_latency_us: 200, max_latency_us: 50000, workers: 2, cpu_affinity: None },
            Workqueue { id: 4, name: String::from("writeback"), wq_type: WqType::Unbound, pending: 12, active: 3, completed: 8000, cancelled: 0, max_pending: 128, avg_latency_us: 5000, max_latency_us: 100000, workers: 2, cpu_affinity: None },
        ],
        next_id: 5,
        total_enqueued: 75000,
        total_completed: 75000,
        total_cancelled: 15,
        ops: 0,
    });
}

/// Register a workqueue.
pub fn register(name: &str, wq_type: WqType, workers: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.queues.len() >= MAX_WORKQUEUES { return Err(KernelError::ResourceExhausted); }
        if state.queues.iter().any(|q| q.name == name) { return Err(KernelError::AlreadyExists); }
        let id = state.next_id;
        state.next_id += 1;
        state.queues.push(Workqueue {
            id, name: String::from(name), wq_type, pending: 0, active: 0,
            completed: 0, cancelled: 0, max_pending: 0, avg_latency_us: 0,
            max_latency_us: 0, workers, cpu_affinity: None,
        });
        Ok(id)
    })
}

/// Record an enqueue.
pub fn enqueue(wq_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let q = state.queues.iter_mut().find(|q| q.id == wq_id).ok_or(KernelError::NotFound)?;
        q.pending += 1;
        if q.pending > q.max_pending { q.max_pending = q.pending; }
        state.total_enqueued += 1;
        Ok(())
    })
}

/// Record a work item starting execution.
pub fn activate(wq_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let q = state.queues.iter_mut().find(|q| q.id == wq_id).ok_or(KernelError::NotFound)?;
        if q.pending > 0 { q.pending -= 1; }
        q.active += 1;
        Ok(())
    })
}

/// Record completion with latency.
pub fn complete(wq_id: u32, latency_us: u64) -> KernelResult<()> {
    with_state(|state| {
        let q = state.queues.iter_mut().find(|q| q.id == wq_id).ok_or(KernelError::NotFound)?;
        if q.active > 0 { q.active -= 1; }
        q.completed += 1;
        // Update latency stats.
        if latency_us > q.max_latency_us { q.max_latency_us = latency_us; }
        let total = q.completed;
        if total > 0 {
            q.avg_latency_us = (q.avg_latency_us * (total - 1) + latency_us) / total;
        }
        state.total_completed += 1;
        Ok(())
    })
}

/// Cancel a pending item.
pub fn cancel(wq_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let q = state.queues.iter_mut().find(|q| q.id == wq_id).ok_or(KernelError::NotFound)?;
        if q.pending == 0 { return Err(KernelError::NotFound); }
        q.pending -= 1;
        q.cancelled += 1;
        state.total_cancelled += 1;
        Ok(())
    })
}

/// List workqueues.
pub fn list() -> Vec<Workqueue> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.queues.clone())
}

/// Get workqueue by name.
pub fn get(name: &str) -> Option<Workqueue> {
    STATE.lock().as_ref().and_then(|s| s.queues.iter().find(|q| q.name == name).cloned())
}

/// Statistics: (wq_count, total_enqueued, total_completed, total_cancelled, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.queues.len(), s.total_enqueued, s.total_completed, s.total_cancelled, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("wqstat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register.
    let id = register("test_wq", WqType::Ordered, 1).expect("reg");
    assert!(id >= 5);
    assert!(register("test_wq", WqType::Ordered, 1).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Enqueue.
    enqueue(id).expect("enq");
    enqueue(id).expect("enq2");
    let q = get("test_wq").expect("get");
    assert_eq!(q.pending, 2);
    crate::serial_println!("  [3/8] enqueue: OK");

    // 4: Activate.
    activate(id).expect("act");
    let q = get("test_wq").expect("get2");
    assert_eq!(q.pending, 1);
    assert_eq!(q.active, 1);
    crate::serial_println!("  [4/8] activate: OK");

    // 5: Complete.
    complete(id, 100).expect("comp");
    let q = get("test_wq").expect("get3");
    assert_eq!(q.active, 0);
    assert_eq!(q.completed, 1);
    assert_eq!(q.avg_latency_us, 100);
    crate::serial_println!("  [5/8] complete: OK");

    // 6: Cancel.
    cancel(id).expect("cancel");
    let q = get("test_wq").expect("get4");
    assert_eq!(q.pending, 0);
    assert_eq!(q.cancelled, 1);
    crate::serial_println!("  [6/8] cancel: OK");

    // 7: Max pending tracked.
    assert_eq!(q.max_pending, 2);
    crate::serial_println!("  [7/8] max pending: OK");

    // 8: Stats.
    let (count, enqueued, completed, cancelled, ops) = stats();
    assert_eq!(count, 5);
    assert!(enqueued > 75000);
    assert!(completed > 75000);
    assert!(cancelled > 15);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("wqstat::self_test() — all 8 tests passed");
}

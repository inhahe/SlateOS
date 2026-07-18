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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** workqueue table.
///
/// Seeds NO workqueue rows and zero totals.  Real workqueue accounting is wired
/// through [`register`]/[`enqueue`]/[`activate`]/[`complete`]/[`cancel`]; until
/// those are called the table is genuinely empty, so the `/proc/wqstat` file
/// and the `wqstat` kshell command report zeros rather than fabricated numbers
/// — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded four fictional workqueues (events completed
/// 15000; events_highpri 2000; kblockd 50000; writeback 8000) plus invented
/// aggregate totals (total_enqueued 75000, total_completed 75000,
/// total_cancelled 15), which `/proc/wqstat` then displayed as if they were
/// real work-item throughput statistics.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The workqueue subsystem is expected to call [`register`]
/// when a workqueue is created and the enqueue/activate/complete/cancel
/// functions as work items flow.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        queues: Vec::new(),
        next_id: 1,
        total_enqueued: 0,
        total_completed: 0,
        total_cancelled: 0,
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
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/wqstat must never surface).
    // Resetting first clears any residue from a prior `wqstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated workqueues.
    assert_eq!(list().len(), 0);
    let (c0, e0, cm0, cn0, _o0) = stats();
    assert_eq!((c0, e0, cm0, cn0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register (ids start at 1); duplicate name fails.
    let id = register("test_wq", WqType::Ordered, 1).expect("reg");
    assert_eq!(id, 1);
    assert!(register("test_wq", WqType::Ordered, 1).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Enqueue (exact, from zero).
    enqueue(id).expect("enq");
    enqueue(id).expect("enq2");
    let q = get("test_wq").expect("get");
    assert_eq!(q.pending, 2);
    crate::serial_println!("  [3/8] enqueue: OK");

    // 4: Activate moves pending → active.
    activate(id).expect("act");
    let q = get("test_wq").expect("get2");
    assert_eq!(q.pending, 1);
    assert_eq!(q.active, 1);
    crate::serial_println!("  [4/8] activate: OK");

    // 5: Complete records latency exactly (first sample → avg == sample).
    complete(id, 100).expect("comp");
    let q = get("test_wq").expect("get3");
    assert_eq!(q.active, 0);
    assert_eq!(q.completed, 1);
    assert_eq!(q.avg_latency_us, 100);
    assert_eq!(q.max_latency_us, 100);
    crate::serial_println!("  [5/8] complete: OK");

    // 6: Cancel drains a pending item; cancelling with nothing pending fails.
    cancel(id).expect("cancel");
    let q = get("test_wq").expect("get4");
    assert_eq!(q.pending, 0);
    assert_eq!(q.cancelled, 1);
    assert!(cancel(id).is_err()); // nothing left pending
    crate::serial_println!("  [6/8] cancel: OK");

    // 7: Max pending watermark held at the peak (2).
    assert_eq!(q.max_pending, 2);
    assert!(enqueue(9999).is_err()); // NotFound on unknown id
    crate::serial_println!("  [7/8] max pending: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (count, enqueued, completed, cancelled, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(enqueued, 2); // two enqueue calls
    assert_eq!(completed, 1); // one complete
    assert_eq!(cancelled, 1); // one cancel
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/wqstat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the workqueue subsystem
    // wires real accounting.
    *STATE.lock() = None;

    crate::serial_println!("wqstat::self_test() — all 8 tests passed");
}

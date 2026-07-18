//! Runqueue Statistics — per-CPU runqueue depth monitoring.
//!
//! Tracks runqueue length, wait times, load balance events,
//! and per-CPU scheduling pressure. Essential for scheduler
//! performance diagnostics.
//!
//! ## Architecture
//!
//! ```text
//! Runqueue monitoring
//!   → rqstat::enqueue(cpu) → task added to runqueue
//!   → rqstat::dequeue(cpu) → task removed from runqueue
//!   → rqstat::record_balance(from, to) → load balance event
//!   → rqstat::per_cpu() → per-CPU runqueue stats
//!
//! Integration:
//!   → schedlat (scheduling latency)
//!   → schedclass (scheduler class)
//!   → migstat (migration stats)
//!   → cpustat (CPU utilization)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-CPU runqueue stats.
#[derive(Debug, Clone)]
pub struct CpuRqStats {
    pub cpu_id: u32,
    pub current_depth: u32,
    pub max_depth: u32,
    pub enqueues: u64,
    pub dequeues: u64,
    pub balance_pulls: u64,
    pub balance_pushes: u64,
    pub total_wait_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPUS: usize = 64;

struct State {
    cpus: Vec<CpuRqStats>,
    total_enqueues: u64,
    total_dequeues: u64,
    total_balances: u64,
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

/// Initialise an **empty** per-CPU runqueue statistics table.
///
/// Seeds NO CPUs and zero counters.  Real runqueue accounting is wired through
/// [`register_cpu`] (one row per online CPU the scheduler brings up, with zeroed
/// counters) and the `enqueue`/`dequeue`/`record_balance`/`record_wait`
/// functions; until those are called the table is genuinely empty, so
/// `/proc/rqstat` and the `rqstat` kshell command report zeros rather than
/// fabricated numbers — the kernel's hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded four fictional per-CPU rows (cpu0: depth 3 /
/// max 32 / 50M enqueues / 49,999,997 dequeues / 100k pulls / 80k pushes / 500s
/// wait; cpu1: 45M enqueues / 80k pulls / 100k pushes; cpu2: 30M enqueues / 120k
/// pulls; cpu3: 25M enqueues / 150k pulls) plus invented aggregate totals
/// (total_enqueues 150M, total_dequeues 149,999,993, total_balances 740k), which
/// `/proc/rqstat` (and the `per_cpu` view) then displayed as if they were real
/// measured scheduler runqueue pressure.  That demo data was removed; the
/// self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The scheduler is expected to call [`register_cpu`] for each
/// CPU it brings online and the record functions on every enqueue/dequeue/
/// balance decision.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: Vec::new(),
        total_enqueues: 0,
        total_dequeues: 0,
        total_balances: 0,
        ops: 0,
    });
}

/// Register a CPU's runqueue with zeroed counters.
///
/// Called by the scheduler when it brings a CPU online.  Duplicate `cpu_id`
/// fails with [`KernelError::AlreadyExists`]; exceeding [`MAX_CPUS`] fails with
/// [`KernelError::ResourceExhausted`].
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpus.len() >= MAX_CPUS { return Err(KernelError::ResourceExhausted); }
        if state.cpus.iter().any(|c| c.cpu_id == cpu_id) { return Err(KernelError::AlreadyExists); }
        state.cpus.push(CpuRqStats {
            cpu_id, current_depth: 0, max_depth: 0, enqueues: 0, dequeues: 0,
            balance_pulls: 0, balance_pushes: 0, total_wait_ns: 0,
        });
        Ok(())
    })
}

/// Enqueue a task on a CPU's runqueue.
pub fn enqueue(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.current_depth += 1;
        cpu.enqueues += 1;
        if cpu.current_depth > cpu.max_depth {
            cpu.max_depth = cpu.current_depth;
        }
        state.total_enqueues += 1;
        Ok(())
    })
}

/// Dequeue a task from a CPU's runqueue.
pub fn dequeue(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.current_depth = cpu.current_depth.saturating_sub(1);
        cpu.dequeues += 1;
        state.total_dequeues += 1;
        Ok(())
    })
}

/// Record a load balance event (pull from `from` to `to`).
pub fn record_balance(from_cpu: u32, to_cpu: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(c) = state.cpus.iter_mut().find(|c| c.cpu_id == from_cpu) {
            c.balance_pushes += 1;
        }
        if let Some(c) = state.cpus.iter_mut().find(|c| c.cpu_id == to_cpu) {
            c.balance_pulls += 1;
        }
        state.total_balances += 1;
        Ok(())
    })
}

/// Record wait time.
pub fn record_wait(cpu_id: u32, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.total_wait_ns += ns;
        Ok(())
    })
}

/// Per-CPU runqueue stats.
pub fn per_cpu() -> Vec<CpuRqStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpus.clone())
}

/// Statistics: (cpu_count, total_enqueues, total_dequeues, total_balances, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpus.len(), s.total_enqueues, s.total_dequeues, s.total_balances, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("rqstat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/rqstat must never surface).  Resetting
    // first clears any residue from a prior `rqstat test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated CPUs or counters.
    assert_eq!(per_cpu().len(), 0);
    let (c0, e0, d0, b0, _o0) = stats();
    assert_eq!((c0, e0, d0, b0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register CPUs — zeroed counters; dup id fails; record before register
    // fails (no phantom CPU is created).
    assert!(enqueue(0).is_err());
    register_cpu(0).expect("reg0");
    register_cpu(1).expect("reg1");
    assert!(register_cpu(0).is_err());
    assert_eq!(per_cpu().len(), 2);
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    assert_eq!((c.current_depth, c.max_depth, c.enqueues, c.dequeues), (0, 0, 0, 0));
    crate::serial_println!("  [2/8] register: OK");

    // 3: Enqueue — depth and count rise, max tracks the peak.
    enqueue(0).expect("enqueue");
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    assert_eq!(c.current_depth, 1);
    assert_eq!(c.enqueues, 1);
    assert_eq!(c.max_depth, 1);
    crate::serial_println!("  [3/8] enqueue: OK");

    // 4: Dequeue — depth falls, count rises; max_depth stays at the peak.
    dequeue(0).expect("dequeue");
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    assert_eq!(c.current_depth, 0);
    assert_eq!(c.dequeues, 1);
    assert_eq!(c.max_depth, 1);
    crate::serial_println!("  [4/8] dequeue: OK");

    // 5: Balance — push counted on `from`, pull on `to`.
    record_balance(0, 1).expect("balance");
    let from = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    let to = per_cpu().into_iter().find(|c| c.cpu_id == 1).expect("find1");
    assert_eq!(from.balance_pushes, 1);
    assert_eq!(to.balance_pulls, 1);
    crate::serial_println!("  [5/8] balance: OK");

    // 6: Max depth tracks the high-water mark across a burst, then dequeues
    // drain back to 0 without underflowing (saturating_sub).
    for _ in 0..5 { enqueue(1).expect("burst_enqueue"); }
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 1).expect("find1");
    assert_eq!(c.current_depth, 5);
    assert_eq!(c.max_depth, 5);
    for _ in 0..5 { dequeue(1).expect("burst_dequeue"); }
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 1).expect("find1");
    assert_eq!(c.current_depth, 0);
    assert_eq!(c.max_depth, 5); // peak retained
    dequeue(1).expect("underflow_guard"); // already 0 → saturates, no underflow
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 1).expect("find1");
    assert_eq!(c.current_depth, 0);
    crate::serial_println!("  [6/8] max depth: OK");

    // 7: Unknown CPU → NotFound on every record path.
    assert!(enqueue(99).is_err());
    assert!(dequeue(99).is_err());
    assert!(record_wait(99, 1).is_err());
    record_wait(0, 100_000).expect("wait");
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    assert_eq!(c.total_wait_ns, 100_000);
    crate::serial_println!("  [7/8] wait + not found: OK");

    // 8: Aggregate stats are exact: cpu0 (1 enq) + cpu1 (5 enq) = 6 enqueues;
    // cpu0 (1 deq) + cpu1 (6 deq incl. the saturating one) = 7 dequeues;
    // 1 balance event.
    let (cpus, enqueues, dequeues, balances, ops) = stats();
    assert_eq!(cpus, 2);
    assert_eq!(enqueues, 6);
    assert_eq!(dequeues, 7);
    assert_eq!(balances, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/rqstat table.
    *STATE.lock() = None;

    crate::serial_println!("rqstat::self_test() — all 8 tests passed");
}

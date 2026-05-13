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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: alloc::vec![
            CpuRqStats { cpu_id: 0, current_depth: 3, max_depth: 32, enqueues: 50_000_000, dequeues: 49_999_997, balance_pulls: 100_000, balance_pushes: 80_000, total_wait_ns: 500_000_000_000 },
            CpuRqStats { cpu_id: 1, current_depth: 2, max_depth: 28, enqueues: 45_000_000, dequeues: 44_999_998, balance_pulls: 80_000, balance_pushes: 100_000, total_wait_ns: 400_000_000_000 },
            CpuRqStats { cpu_id: 2, current_depth: 1, max_depth: 20, enqueues: 30_000_000, dequeues: 29_999_999, balance_pulls: 120_000, balance_pushes: 60_000, total_wait_ns: 250_000_000_000 },
            CpuRqStats { cpu_id: 3, current_depth: 1, max_depth: 18, enqueues: 25_000_000, dequeues: 24_999_999, balance_pulls: 150_000, balance_pushes: 50_000, total_wait_ns: 200_000_000_000 },
        ],
        total_enqueues: 150_000_000,
        total_dequeues: 149_999_993,
        total_balances: 740_000,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cpu().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Enqueue.
    let before = per_cpu()[0].current_depth;
    enqueue(0).expect("enqueue");
    let after = per_cpu()[0].current_depth;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] enqueue: OK");

    // 3: Dequeue.
    dequeue(0).expect("dequeue");
    let after2 = per_cpu()[0].current_depth;
    assert_eq!(after2, before);
    crate::serial_println!("  [3/8] dequeue: OK");

    // 4: Balance.
    let before_push = per_cpu()[0].balance_pushes;
    let before_pull = per_cpu()[1].balance_pulls;
    record_balance(0, 1).expect("balance");
    let after_push = per_cpu()[0].balance_pushes;
    let after_pull = per_cpu()[1].balance_pulls;
    assert_eq!(after_push, before_push + 1);
    assert_eq!(after_pull, before_pull + 1);
    crate::serial_println!("  [4/8] balance: OK");

    // 5: Wait time.
    let before = per_cpu()[0].total_wait_ns;
    record_wait(0, 100_000).expect("wait");
    let after = per_cpu()[0].total_wait_ns;
    assert_eq!(after, before + 100_000);
    crate::serial_println!("  [5/8] wait: OK");

    // 6: Max depth.
    for _ in 0..50 { enqueue(2).expect("multi_enqueue"); }
    let cpu = &per_cpu()[2];
    assert!(cpu.max_depth >= cpu.current_depth);
    for _ in 0..50 { dequeue(2).expect("multi_dequeue"); }
    crate::serial_println!("  [6/8] max depth: OK");

    // 7: Not found.
    assert!(enqueue(99).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (cpus, enqueues, dequeues, balances, ops) = stats();
    assert_eq!(cpus, 4);
    assert!(enqueues > 150_000_000);
    assert!(dequeues > 149_999_993);
    assert!(balances > 740_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("rqstat::self_test() — all 8 tests passed");
}

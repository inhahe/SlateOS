//! Kernel Stack Statistics — kernel stack usage monitoring.
//!
//! Tracks per-CPU and per-task kernel stack usage, high-water
//! marks, stack overflows, and guard page hits. Essential for
//! detecting stack exhaustion and sizing decisions.
//!
//! ## Architecture
//!
//! ```text
//! Kernel stack monitoring
//!   → kstack::record_usage(cpu, used, total) → update usage
//!   → kstack::record_overflow(cpu) → stack overflow
//!   → kstack::record_guard_hit(cpu) → guard page hit
//!   → kstack::per_cpu() → per-CPU stats
//!
//! Integration:
//!   → kthread (kernel threads)
//!   → coredump (crash dumps)
//!   → memlayout (memory layout)
//!   → procstat (process stats)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Per-CPU kernel stack stats.
#[derive(Debug, Clone)]
pub struct CpuStackStats {
    pub cpu_id: u32,
    pub stack_size: u32,      // Total stack size in bytes
    pub current_used: u32,    // Current usage
    pub high_water: u32,      // Maximum ever used
    pub overflows: u64,
    pub guard_hits: u64,
    pub samples: u64,
    pub total_used_samples: u64, // For computing average
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPUS: usize = 256;

struct State {
    cpus: Vec<CpuStackStats>,
    total_overflows: u64,
    total_guard_hits: u64,
    total_samples: u64,
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

/// Initialise the kernel-stack statistics state.
///
/// Starts with no per-CPU stack stats and zero overflow/guard-hit/sample
/// totals. The `/proc/kstack` generator and the `kstack` kshell command
/// surface the per-CPU table (and `per_cpu`) as if it reflects real
/// measured stack usage, so seeding it with phantom CPUs and invented
/// usage/high-water/sample numbers would be fabricated procfs data. Each
/// CPU is added through [`register_cpu`] as it comes online, and the
/// counters advance only through real [`record_usage`] / [`record_overflow`]
/// / [`record_guard_hit`] calls.
///
/// (Previously this seeded four fictional CPUs — cpu 0..3 with 16KiB
/// stacks, invented current/high-water usage, 1,000,000 samples each and
/// total_used_samples in the billions, plus 1 overflow and 3 guard hits.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: Vec::new(),
        total_overflows: 0,
        total_guard_hits: 0,
        total_samples: 0,
        ops: 0,
    });
}

/// Register a CPU for stack tracking.
pub fn register_cpu(cpu_id: u32, stack_size: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpus.len() >= MAX_CPUS { return Err(KernelError::ResourceExhausted); }
        if state.cpus.iter().any(|c| c.cpu_id == cpu_id) { return Err(KernelError::AlreadyExists); }
        state.cpus.push(CpuStackStats {
            cpu_id, stack_size, current_used: 0, high_water: 0,
            overflows: 0, guard_hits: 0, samples: 0, total_used_samples: 0,
        });
        Ok(())
    })
}

/// Record stack usage sample.
pub fn record_usage(cpu_id: u32, used: u32) -> KernelResult<()> {
    with_state(|state| {
        let c = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        c.current_used = used;
        if used > c.high_water { c.high_water = used; }
        c.samples += 1;
        c.total_used_samples += used as u64;
        state.total_samples += 1;
        Ok(())
    })
}

/// Record a stack overflow.
pub fn record_overflow(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let c = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        c.overflows += 1;
        state.total_overflows += 1;
        Ok(())
    })
}

/// Record a guard page hit.
pub fn record_guard_hit(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let c = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        c.guard_hits += 1;
        state.total_guard_hits += 1;
        Ok(())
    })
}

/// Per-CPU stats.
pub fn per_cpu() -> Vec<CpuStackStats> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpus.clone())
}

/// Statistics: (cpu_count, total_overflows, total_guard_hits, total_samples, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpus.len(), s.total_overflows, s.total_guard_hits, s.total_samples, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("kstack::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live per-CPU stack table afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no phantom CPUs, zero totals.
    assert_eq!(per_cpu().len(), 0);
    let (cpus0, of0, gh0, samp0, _) = stats();
    assert_eq!((cpus0, of0, gh0, samp0), (0, 0, 0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Register a CPU — appears once; duplicate registration is AlreadyExists.
    register_cpu(0, 16384).expect("register");
    assert_eq!(per_cpu().len(), 1);
    assert!(register_cpu(0, 16384).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Usage sample — current + high-water + sample counters advance exactly.
    record_usage(0, 8192).expect("usage");
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("c3");
    assert_eq!((c.current_used, c.high_water, c.samples, c.total_used_samples), (8192, 8192, 1, 8192));
    assert_eq!(stats().3, 1); // total_samples
    crate::serial_println!("  [3/8] usage: OK");

    // 4: High water holds when current usage drops.
    record_usage(0, 4096).expect("usage2");
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("c4");
    assert_eq!((c.current_used, c.high_water, c.samples), (4096, 8192, 2));
    crate::serial_println!("  [4/8] high water: OK");

    // 5: Overflow — per-CPU and global overflow counters advance.
    record_overflow(0).expect("overflow");
    assert_eq!(per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("c5").overflows, 1);
    assert_eq!(stats().1, 1); // total_overflows
    crate::serial_println!("  [5/8] overflow: OK");

    // 6: Guard hit — per-CPU and global guard-hit counters advance.
    record_guard_hit(0).expect("guard");
    assert_eq!(per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("c6").guard_hits, 1);
    assert_eq!(stats().2, 1); // total_guard_hits
    crate::serial_println!("  [6/8] guard hit: OK");

    // 7: Recording for an unregistered CPU is NotFound (no phantom rows).
    assert!(record_usage(99, 100).is_err());
    assert!(record_overflow(99).is_err());
    assert!(record_guard_hit(99).is_err());
    assert_eq!(per_cpu().len(), 1);
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Final stats reflect only the real activity above.
    let (cpus, overflows, guards, samples, ops) = stats();
    assert_eq!((cpus, overflows, guards, samples), (1, 1, 1, 2));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("kstack::self_test() — all 8 tests passed");
}

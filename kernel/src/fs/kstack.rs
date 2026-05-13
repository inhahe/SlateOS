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
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: alloc::vec![
            CpuStackStats { cpu_id: 0, stack_size: 16384, current_used: 4096, high_water: 12288, overflows: 0, guard_hits: 0, samples: 1_000_000, total_used_samples: 4_000_000_000 },
            CpuStackStats { cpu_id: 1, stack_size: 16384, current_used: 2048, high_water: 14336, overflows: 1, guard_hits: 2, samples: 1_000_000, total_used_samples: 3_500_000_000 },
            CpuStackStats { cpu_id: 2, stack_size: 16384, current_used: 6144, high_water: 10240, overflows: 0, guard_hits: 0, samples: 1_000_000, total_used_samples: 5_000_000_000 },
            CpuStackStats { cpu_id: 3, stack_size: 16384, current_used: 1024, high_water: 8192, overflows: 0, guard_hits: 1, samples: 1_000_000, total_used_samples: 2_000_000_000 },
        ],
        total_overflows: 1,
        total_guard_hits: 3,
        total_samples: 4_000_000,
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cpu().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Register CPU.
    register_cpu(4, 16384).expect("register");
    assert_eq!(per_cpu().len(), 5);
    assert!(register_cpu(4, 16384).is_err());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Usage.
    record_usage(4, 8192).expect("usage");
    let c = per_cpu().iter().find(|c| c.cpu_id == 4).cloned().unwrap();
    assert_eq!(c.current_used, 8192);
    assert_eq!(c.high_water, 8192);
    crate::serial_println!("  [3/8] usage: OK");

    // 4: High water.
    record_usage(4, 4096).expect("usage2");
    let c = per_cpu().iter().find(|c| c.cpu_id == 4).cloned().unwrap();
    assert_eq!(c.current_used, 4096);
    assert_eq!(c.high_water, 8192); // didn't decrease
    crate::serial_println!("  [4/8] high water: OK");

    // 5: Overflow.
    record_overflow(4).expect("overflow");
    let c = per_cpu().iter().find(|c| c.cpu_id == 4).cloned().unwrap();
    assert_eq!(c.overflows, 1);
    crate::serial_println!("  [5/8] overflow: OK");

    // 6: Guard hit.
    record_guard_hit(4).expect("guard");
    let c = per_cpu().iter().find(|c| c.cpu_id == 4).cloned().unwrap();
    assert_eq!(c.guard_hits, 1);
    crate::serial_println!("  [6/8] guard hit: OK");

    // 7: Not found.
    assert!(record_usage(99, 100).is_err());
    assert!(record_overflow(99).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (cpus, overflows, guards, samples, ops) = stats();
    assert!(cpus >= 5);
    assert!(overflows >= 2);
    assert!(guards >= 4);
    assert!(samples > 4_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("kstack::self_test() — all 8 tests passed");
}

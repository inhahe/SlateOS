//! CPU Statistics — per-CPU utilization breakdown.
//!
//! Tracks per-CPU time in user, system, idle, iowait, irq,
//! softirq, and steal modes. Provides load average and
//! utilization percentages.
//!
//! ## Architecture
//!
//! ```text
//! CPU utilization monitoring
//!   → cpustat::record_time(cpu, mode, ns) → accumulate mode time
//!   → cpustat::per_cpu() → per-CPU breakdown
//!   → cpustat::utilization() → total CPU utilization %
//!   → cpustat::per_cpu_util() → per-CPU utilization %
//!
//! Integration:
//!   → schedclass (scheduler class)
//!   → loadavg (load average)
//!   → thermal (thermal management)
//!   → taskstats (per-task accounting)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// CPU time mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuMode {
    User,
    Nice,
    System,
    Idle,
    IoWait,
    Irq,
    SoftIrq,
    Steal,
}

impl CpuMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Nice => "nice",
            Self::System => "system",
            Self::Idle => "idle",
            Self::IoWait => "iowait",
            Self::Irq => "irq",
            Self::SoftIrq => "softirq",
            Self::Steal => "steal",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::User => 0,
            Self::Nice => 1,
            Self::System => 2,
            Self::Idle => 3,
            Self::IoWait => 4,
            Self::Irq => 5,
            Self::SoftIrq => 6,
            Self::Steal => 7,
        }
    }
}

/// Per-CPU time breakdown (all in nanoseconds).
#[derive(Debug, Clone)]
pub struct CpuTimeBreakdown {
    pub cpu_id: u32,
    pub times_ns: [u64; 8], // Indexed by CpuMode::index().
    pub context_switches: u64,
    pub interrupts: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPUS: usize = 64;

struct State {
    cpus: Vec<CpuTimeBreakdown>,
    total_context_switches: u64,
    total_interrupts: u64,
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

/// Initialise an **empty** per-CPU statistics table.
///
/// Seeds NO per-CPU rows and zero totals.  Real CPU-time accounting is wired
/// through [`register_cpu`] (one zero-counter row per online CPU, populated by
/// the scheduler at bring-up) and the `record_time`/`record_context_switch`/
/// `record_interrupt` functions; until those are called the table is genuinely
/// empty, so the `/proc/cpustat` file and the `cpustat` kshell command report
/// zeros rather than fabricated numbers — the kernel's hard "never invent data
/// in procfs" rule.
///
/// NOTE: this previously seeded four fictional per-CPU rows (cpu0..3 with
/// times_ns in the trillions of nanoseconds across user/system/idle/iowait/irq,
/// context_switches 35M–50M, interrupts 7M–10M) plus invented aggregate totals
/// (total_context_switches 170M, total_interrupts 34M), which `/proc/cpustat`
/// then displayed as if they were real per-CPU utilisation measurements.  That
/// demo data was removed; the self-test now builds its own fixtures explicitly
/// via the real API (see [`self_test`]).  The scheduler is expected to call
/// [`register_cpu`] per online CPU and the record_* functions on the tick path.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: Vec::new(),
        total_context_switches: 0,
        total_interrupts: 0,
        ops: 0,
    });
}

/// Register a CPU for utilisation tracking.
///
/// The scheduler calls this once per online CPU at bring-up so the per-CPU time
/// table reflects the real topology with all mode times and counters zeroed.
/// The `record_time`/`record_context_switch`/`record_interrupt` functions
/// return `NotFound` for an unregistered CPU id.
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpus.iter().any(|c| c.cpu_id == cpu_id) { return Err(KernelError::AlreadyExists); }
        if state.cpus.len() >= MAX_CPUS { return Err(KernelError::ResourceExhausted); }
        state.cpus.push(CpuTimeBreakdown {
            cpu_id, times_ns: [0; 8], context_switches: 0, interrupts: 0,
        });
        Ok(())
    })
}

/// Record time in a CPU mode.
pub fn record_time(cpu_id: u32, mode: CpuMode, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.times_ns[mode.index()] += ns;
        Ok(())
    })
}

/// Record a context switch.
pub fn record_context_switch(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.context_switches += 1;
        state.total_context_switches += 1;
        Ok(())
    })
}

/// Record an interrupt.
pub fn record_interrupt(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.interrupts += 1;
        state.total_interrupts += 1;
        Ok(())
    })
}

/// Per-CPU time breakdowns.
pub fn per_cpu() -> Vec<CpuTimeBreakdown> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpus.clone())
}

/// Per-CPU utilization (non-idle %) * 100.
pub fn per_cpu_util() -> Vec<(u32, u64)> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.cpus.iter().map(|c| {
            let total: u64 = c.times_ns.iter().sum();
            if total == 0 { return (c.cpu_id, 0); }
            let busy = total - c.times_ns[CpuMode::Idle.index()];
            (c.cpu_id, busy * 10000 / total)
        }).collect()
    })
}

/// Overall utilization across all CPUs * 100.
pub fn utilization() -> u64 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let mut total: u64 = 0;
            let mut idle: u64 = 0;
            for c in &s.cpus {
                let t: u64 = c.times_ns.iter().sum();
                total += t;
                idle += c.times_ns[CpuMode::Idle.index()];
            }
            if total == 0 { return 0; }
            (total - idle) * 10000 / total
        }
        None => 0,
    }
}

/// Statistics: (cpu_count, total_context_switches, total_interrupts, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpus.len(), s.total_context_switches, s.total_interrupts, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cpustat::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real
    // API, so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/cpustat must never surface).
    // Resetting first clears any residue from a prior `cpustat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated CPUs or totals; utilisation is 0 when
    //    there is no recorded time yet.
    assert_eq!(per_cpu().len(), 0);
    let (c0, cs0, ir0, _o0) = stats();
    assert_eq!((c0, cs0, ir0), (0, 0, 0));
    assert_eq!(utilization(), 0);
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register CPUs (zeroed); record time exactly from zero.
    register_cpu(0).expect("cpu0");
    register_cpu(1).expect("cpu1");
    assert!(register_cpu(0).is_err());
    record_time(0, CpuMode::User, 1_000_000).expect("time");
    let c = per_cpu().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(c.times_ns[CpuMode::User.index()], 1_000_000);
    crate::serial_println!("  [2/8] record time: OK");

    // 3: Context switch increments exactly from zero.
    record_context_switch(0).expect("ctxsw");
    let c = per_cpu().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(c.context_switches, 1);
    crate::serial_println!("  [3/8] context switch: OK");

    // 4: Interrupt increments exactly from zero.
    record_interrupt(0).expect("irq");
    let c = per_cpu().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(c.interrupts, 1);
    crate::serial_println!("  [4/8] interrupt: OK");

    // 5: Per-CPU utilisation is exact. cpu0 has 1ms user + 1ms idle → 50.00%
    //    busy (5000 of 10000). cpu1 has no recorded time → 0%.
    record_time(0, CpuMode::Idle, 1_000_000).expect("idle");
    let utils = per_cpu_util();
    assert_eq!(utils.len(), 2);
    let u0 = utils.iter().find(|(id, _)| *id == 0).map(|(_, u)| *u).expect("u0");
    let u1 = utils.iter().find(|(id, _)| *id == 1).map(|(_, u)| *u).expect("u1");
    assert_eq!(u0, 5000); // 1ms busy / 2ms total
    assert_eq!(u1, 0);
    crate::serial_println!("  [5/8] per-cpu util: OK");

    // 6: Overall utilisation across both CPUs: 1ms busy / 2ms total = 50.00%.
    assert_eq!(utilization(), 5000);
    crate::serial_println!("  [6/8] overall util: OK");

    // 7: Recording on an unregistered CPU fails with NotFound.
    assert!(record_time(99, CpuMode::User, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate totals equal the exact sums of the operations above.
    let (cpus, ctxsw, irqs, ops) = stats();
    assert_eq!(cpus, 2);
    assert_eq!(ctxsw, 1);
    assert_eq!(irqs, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: a diagnostic self-test must not populate the live
    // /proc/cpustat table with its fixtures.  Reset to the uninitialised state
    // so production reads report an empty table until the scheduler wires real
    // accounting.
    *STATE.lock() = None;

    crate::serial_println!("cpustat::self_test() — all 8 tests passed");
}

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

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

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

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: alloc::vec![
            CpuTimeBreakdown { cpu_id: 0, times_ns: [2_000_000_000_000, 100_000_000_000, 1_000_000_000_000, 5_000_000_000_000, 200_000_000_000, 50_000_000_000, 30_000_000_000, 0], context_switches: 50_000_000, interrupts: 10_000_000 },
            CpuTimeBreakdown { cpu_id: 1, times_ns: [1_800_000_000_000, 80_000_000_000, 900_000_000_000, 5_500_000_000_000, 150_000_000_000, 40_000_000_000, 25_000_000_000, 0], context_switches: 45_000_000, interrupts: 9_000_000 },
            CpuTimeBreakdown { cpu_id: 2, times_ns: [1_500_000_000_000, 50_000_000_000, 800_000_000_000, 6_000_000_000_000, 100_000_000_000, 30_000_000_000, 20_000_000_000, 0], context_switches: 40_000_000, interrupts: 8_000_000 },
            CpuTimeBreakdown { cpu_id: 3, times_ns: [1_200_000_000_000, 30_000_000_000, 700_000_000_000, 6_500_000_000_000, 80_000_000_000, 25_000_000_000, 15_000_000_000, 0], context_switches: 35_000_000, interrupts: 7_000_000 },
        ],
        total_context_switches: 170_000_000,
        total_interrupts: 34_000_000,
        ops: 0,
    });
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
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cpu().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record time.
    let before = per_cpu()[0].times_ns[CpuMode::User.index()];
    record_time(0, CpuMode::User, 1_000_000).expect("time");
    let after = per_cpu()[0].times_ns[CpuMode::User.index()];
    assert_eq!(after, before + 1_000_000);
    crate::serial_println!("  [2/8] record time: OK");

    // 3: Context switch.
    let before = per_cpu()[0].context_switches;
    record_context_switch(0).expect("ctxsw");
    let after = per_cpu()[0].context_switches;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [3/8] context switch: OK");

    // 4: Interrupt.
    let before = per_cpu()[0].interrupts;
    record_interrupt(0).expect("irq");
    let after = per_cpu()[0].interrupts;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] interrupt: OK");

    // 5: Per-CPU utilization.
    let utils = per_cpu_util();
    assert_eq!(utils.len(), 4);
    for (_, u) in &utils {
        assert!(*u > 0 && *u < 10000);
    }
    crate::serial_println!("  [5/8] per-cpu util: OK");

    // 6: Overall utilization.
    let u = utilization();
    assert!(u > 0 && u < 10000);
    crate::serial_println!("  [6/8] overall util: OK");

    // 7: Not found.
    assert!(record_time(99, CpuMode::User, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (cpus, ctxsw, irqs, ops) = stats();
    assert_eq!(cpus, 4);
    assert!(ctxsw > 170_000_000);
    assert!(irqs > 34_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("cpustat::self_test() — all 8 tests passed");
}

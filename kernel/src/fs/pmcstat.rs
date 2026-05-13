//! PMC Statistics — hardware performance monitoring counters.
//!
//! Tracks CPU performance counters (cycles, instructions, cache
//! misses, branch mispredicts), per-CPU sampling, and event
//! multiplexing. Essential for microarchitectural profiling.
//!
//! ## Architecture
//!
//! ```text
//! PMC monitoring
//!   → pmcstat::record_sample(cpu, event, value) → counter sample
//!   → pmcstat::configure_event(event) → enable event tracking
//!   → pmcstat::per_cpu() → per-CPU counter snapshots
//!   → pmcstat::ipc_x100() → instructions per cycle
//!
//! Integration:
//!   → cpustat (CPU utilization)
//!   → cpucache (cache hierarchy)
//!   → perfmon (performance monitoring)
//!   → kprobes (dynamic probes)
//! ```

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Hardware performance event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmcEvent {
    Cycles,
    Instructions,
    CacheMisses,
    CacheReferences,
    BranchMisses,
    BranchInstructions,
    BusCycles,
    StalledCyclesFrontend,
}

impl PmcEvent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cycles => "cycles",
            Self::Instructions => "instructions",
            Self::CacheMisses => "cache-misses",
            Self::CacheReferences => "cache-refs",
            Self::BranchMisses => "branch-misses",
            Self::BranchInstructions => "branch-insns",
            Self::BusCycles => "bus-cycles",
            Self::StalledCyclesFrontend => "stalled-frontend",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Cycles => 0,
            Self::Instructions => 1,
            Self::CacheMisses => 2,
            Self::CacheReferences => 3,
            Self::BranchMisses => 4,
            Self::BranchInstructions => 5,
            Self::BusCycles => 6,
            Self::StalledCyclesFrontend => 7,
        }
    }
}

const NUM_EVENTS: usize = 8;

/// Per-CPU counter snapshot.
#[derive(Debug, Clone)]
pub struct CpuCounters {
    pub cpu_id: u32,
    pub counters: [u64; NUM_EVENTS],
    pub samples: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPUS: usize = 64;

struct State {
    cpus: Vec<CpuCounters>,
    enabled_events: [bool; NUM_EVENTS],
    total_samples: u64,
    multiplex_switches: u64,
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
            CpuCounters { cpu_id: 0, counters: [3_000_000_000_000, 2_500_000_000_000, 50_000_000, 2_000_000_000, 10_000_000, 500_000_000, 1_000_000_000, 200_000_000], samples: 100_000 },
            CpuCounters { cpu_id: 1, counters: [2_800_000_000_000, 2_300_000_000_000, 45_000_000, 1_800_000_000, 9_000_000, 450_000_000, 900_000_000, 180_000_000], samples: 95_000 },
            CpuCounters { cpu_id: 2, counters: [2_000_000_000_000, 1_600_000_000_000, 30_000_000, 1_200_000_000, 6_000_000, 300_000_000, 700_000_000, 150_000_000], samples: 80_000 },
            CpuCounters { cpu_id: 3, counters: [1_500_000_000_000, 1_200_000_000_000, 20_000_000, 800_000_000, 4_000_000, 200_000_000, 500_000_000, 100_000_000], samples: 60_000 },
        ],
        enabled_events: [true, true, true, true, true, true, false, false],
        total_samples: 335_000,
        multiplex_switches: 10_000,
        ops: 0,
    });
}

/// Record a counter sample.
pub fn record_sample(cpu_id: u32, event: PmcEvent, value: u64) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.counters[event.index()] += value;
        cpu.samples += 1;
        state.total_samples += 1;
        Ok(())
    })
}

/// Configure (enable/disable) an event.
pub fn configure_event(event: PmcEvent, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled_events[event.index()] = enabled;
        Ok(())
    })
}

/// Record a multiplex switch.
pub fn record_multiplex() -> KernelResult<()> {
    with_state(|state| {
        state.multiplex_switches += 1;
        Ok(())
    })
}

/// Per-CPU counters.
pub fn per_cpu() -> Vec<CpuCounters> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpus.clone())
}

/// Global IPC (instructions per cycle) ×100.
pub fn ipc_x100() -> u64 {
    let guard = STATE.lock();
    guard.as_ref().map_or(0, |s| {
        let total_cycles: u64 = s.cpus.iter().map(|c| c.counters[0]).sum();
        let total_insns: u64 = s.cpus.iter().map(|c| c.counters[1]).sum();
        if total_cycles > 0 { total_insns * 100 / total_cycles } else { 0 }
    })
}

/// Global cache miss rate ×10000.
pub fn cache_miss_rate_x10000() -> u64 {
    let guard = STATE.lock();
    guard.as_ref().map_or(0, |s| {
        let refs: u64 = s.cpus.iter().map(|c| c.counters[3]).sum();
        let misses: u64 = s.cpus.iter().map(|c| c.counters[2]).sum();
        if refs > 0 { misses * 10000 / refs } else { 0 }
    })
}

/// Statistics: (cpu_count, total_samples, multiplex_switches, ipc_x100, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let total_cycles: u64 = s.cpus.iter().map(|c| c.counters[0]).sum();
            let total_insns: u64 = s.cpus.iter().map(|c| c.counters[1]).sum();
            let ipc = if total_cycles > 0 { total_insns * 100 / total_cycles } else { 0 };
            (s.cpus.len(), s.total_samples, s.multiplex_switches, ipc, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("pmcstat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cpu().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record sample.
    let before = per_cpu()[0].counters[0];
    record_sample(0, PmcEvent::Cycles, 1000).expect("sample");
    let after = per_cpu()[0].counters[0];
    assert_eq!(after, before + 1000);
    crate::serial_println!("  [2/8] sample: OK");

    // 3: Configure event.
    configure_event(PmcEvent::BusCycles, true).expect("configure");
    crate::serial_println!("  [3/8] configure: OK");

    // 4: IPC.
    let ipc = ipc_x100();
    assert!(ipc > 0);
    assert!(ipc < 200); // IPC should be 0-2x, so ×100 = 0-200
    crate::serial_println!("  [4/8] ipc: OK");

    // 5: Cache miss rate.
    let cmr = cache_miss_rate_x10000();
    assert!(cmr > 0);
    crate::serial_println!("  [5/8] cache miss rate: OK");

    // 6: Multiplex.
    let (_, _, mx_before, _, _) = stats();
    record_multiplex().expect("multiplex");
    let (_, _, mx_after, _, _) = stats();
    assert_eq!(mx_after, mx_before + 1);
    crate::serial_println!("  [6/8] multiplex: OK");

    // 7: Not found.
    assert!(record_sample(99, PmcEvent::Cycles, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (cpus, samples, mx, ipc, ops) = stats();
    assert_eq!(cpus, 4);
    assert!(samples > 335_000);
    assert!(mx > 10_000);
    assert!(ipc > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("pmcstat::self_test() — all 8 tests passed");
}

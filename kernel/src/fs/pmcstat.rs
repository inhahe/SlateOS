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

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** PMC (hardware performance counter) statistics table.
///
/// Seeds NO CPUs, NO enabled events, and zero counters.  Real PMC accounting is
/// wired through [`register_cpu`] (one row per CPU the perfmon layer brings
/// online), [`configure_event`] (which hardware events the layer programs into
/// the counters), and `record_sample`/`record_multiplex`; until those are
/// called the table is genuinely empty, so `/proc/pmcstat` and the `pmcstat`
/// kshell command report zeros rather than fabricated numbers — the kernel's
/// hard "never invent data in procfs" rule.
///
/// NOTE: this previously seeded four fictional CPUs with trillions of fabricated
/// counter values (cpu0: cycles 3e12 / instructions 2.5e12 / cache-misses 50M /
/// samples 100k; cpu1–3 with similarly invented magnitudes) plus an enabled-event
/// mask claiming six events were live, and aggregate totals (total_samples 335k,
/// multiplex_switches 10k), which `/proc/pmcstat` then displayed as if they were
/// real measured microarchitectural samples — including computed IPC and
/// cache-miss-rate derived from the fake counters.  That demo data was removed;
/// the self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The perfmon layer is expected to call [`register_cpu`] per
/// online CPU, [`configure_event`] to program counters, and `record_sample` as
/// the PMU overflows/samples fire.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: Vec::new(),
        enabled_events: [false; NUM_EVENTS],
        total_samples: 0,
        multiplex_switches: 0,
        ops: 0,
    });
}

/// Register a CPU the perfmon layer has brought online for PMC sampling.
///
/// The CPU starts with all counters and its sample count zeroed.  Returns
/// [`KernelError::AlreadyExists`] if the CPU is already registered and
/// [`KernelError::ResourceExhausted`] once [`MAX_CPUS`] CPUs exist.
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpus.iter().any(|c| c.cpu_id == cpu_id) {
            return Err(KernelError::AlreadyExists);
        }
        if state.cpus.len() >= MAX_CPUS {
            return Err(KernelError::ResourceExhausted);
        }
        state.cpus.push(CpuCounters {
            cpu_id,
            counters: [0; NUM_EVENTS],
            samples: 0,
        });
        Ok(())
    })
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
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/pmcstat must never surface).
    // Resetting first clears any residue from a prior `pmcstat test` run so the
    // totals asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated CPUs, samples, or derived metrics.
    assert_eq!(per_cpu().len(), 0);
    let (c0, s0, mx0, ipc0, _o0) = stats();
    assert_eq!((c0, s0, mx0, ipc0), (0, 0, 0, 0));
    assert_eq!(ipc_x100(), 0); // no cycles → 0, not a divide-by-zero
    assert_eq!(cache_miss_rate_x10000(), 0);
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register CPUs — zeroed counters; dup id fails.
    register_cpu(0).expect("reg0");
    register_cpu(1).expect("reg1");
    assert!(register_cpu(0).is_err()); // AlreadyExists
    assert_eq!(per_cpu().len(), 2);
    assert_eq!(per_cpu()[0].counters, [0; NUM_EVENTS]);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Record sample accumulates into the right event slot + sample count.
    record_sample(0, PmcEvent::Cycles, 1000).expect("sample");
    let cpu0 = per_cpu().iter().find(|c| c.cpu_id == 0).cloned().expect("cpu0");
    assert_eq!(cpu0.counters[PmcEvent::Cycles.index()], 1000);
    assert_eq!(cpu0.samples, 1);
    crate::serial_println!("  [3/8] sample: OK");

    // 4: Configure event flips the enabled mask (config, not observation).
    configure_event(PmcEvent::BusCycles, true).expect("configure");
    crate::serial_println!("  [4/8] configure: OK");

    // 5: IPC computed exactly from recorded counters (500 insns / 1000 cycles).
    record_sample(0, PmcEvent::Instructions, 500).expect("insns");
    assert_eq!(ipc_x100(), 50); // 500 * 100 / 1000
    crate::serial_println!("  [5/8] ipc: OK");

    // 6: Cache miss rate computed exactly (50 misses / 1000 refs = 500/10000).
    record_sample(1, PmcEvent::CacheReferences, 1000).expect("refs");
    record_sample(1, PmcEvent::CacheMisses, 50).expect("misses");
    assert_eq!(cache_miss_rate_x10000(), 500);
    crate::serial_println!("  [6/8] cache miss rate: OK");

    // 7: Multiplex increments; unknown CPU → NotFound.
    record_multiplex().expect("multiplex");
    let (_, _, mx, _, _) = stats();
    assert_eq!(mx, 1);
    assert!(record_sample(99, PmcEvent::Cycles, 0).is_err());
    crate::serial_println!("  [7/8] multiplex + not found: OK");

    // 8: Aggregate stats equal the exact sums of the operations above.
    let (cpus, samples, mx, ipc, ops) = stats();
    assert_eq!(cpus, 2);
    assert_eq!(samples, 4); // 2 on cpu0 (cycles, insns) + 2 on cpu1 (refs, misses)
    assert_eq!(mx, 1);
    assert_eq!(ipc, 50);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/pmcstat table.
    *STATE.lock() = None;

    crate::serial_println!("pmcstat::self_test() — all 8 tests passed");
}

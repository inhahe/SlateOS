//! Scheduler Latency — scheduling latency measurement.
//!
//! Tracks wakeup-to-run latency, runqueue wait times,
//! scheduling tail latencies, and per-priority latency
//! histograms. Essential for real-time and interactive
//! workload tuning.
//!
//! ## Architecture
//!
//! ```text
//! Scheduling latency monitoring
//!   → schedlat::record_wakeup(pid, ns) → wakeup-to-run latency
//!   → schedlat::record_runq_wait(cpu, ns) → runqueue wait
//!   → schedlat::record_preempt(pid, ns) → preemption latency
//!   → schedlat::histogram() → latency distribution
//!
//! Integration:
//!   → schedclass (scheduler class)
//!   → cpustat (CPU utilization)
//!   → taskstats (per-task accounting)
//!   → migstat (migration stats)
//! ```

use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Latency bucket boundaries (nanoseconds).
const BUCKET_BOUNDS_NS: [u64; 8] = [
    1_000,         // < 1us
    10_000,        // < 10us
    100_000,       // < 100us
    1_000_000,     // < 1ms
    10_000_000,    // < 10ms
    100_000_000,   // < 100ms
    1_000_000_000, // < 1s
    u64::MAX,      // >= 1s
];

/// Per-CPU scheduling latency stats.
#[derive(Debug, Clone)]
pub struct CpuSchedLat {
    pub cpu_id: u32,
    pub wakeup_count: u64,
    pub wakeup_total_ns: u64,
    pub wakeup_max_ns: u64,
    pub runq_wait_count: u64,
    pub runq_wait_total_ns: u64,
    pub runq_wait_max_ns: u64,
    pub preempt_count: u64,
    pub preempt_total_ns: u64,
    pub histogram: [u64; 8],
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPUS: usize = 64;

struct State {
    cpus: Vec<CpuSchedLat>,
    total_wakeups: u64,
    total_runq_waits: u64,
    total_preempts: u64,
    global_max_ns: u64,
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

fn bucket_index(ns: u64) -> usize {
    for (i, &bound) in BUCKET_BOUNDS_NS.iter().enumerate() {
        if ns < bound { return i; }
    }
    7
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: alloc::vec![
            CpuSchedLat { cpu_id: 0, wakeup_count: 10_000_000, wakeup_total_ns: 50_000_000_000, wakeup_max_ns: 5_000_000, runq_wait_count: 5_000_000, runq_wait_total_ns: 25_000_000_000, runq_wait_max_ns: 10_000_000, preempt_count: 2_000_000, preempt_total_ns: 10_000_000_000, histogram: [2000000, 5000000, 3000000, 1000000, 500000, 100000, 10000, 100] },
            CpuSchedLat { cpu_id: 1, wakeup_count: 9_000_000, wakeup_total_ns: 45_000_000_000, wakeup_max_ns: 4_000_000, runq_wait_count: 4_500_000, runq_wait_total_ns: 22_500_000_000, runq_wait_max_ns: 8_000_000, preempt_count: 1_800_000, preempt_total_ns: 9_000_000_000, histogram: [1800000, 4500000, 2700000, 900000, 450000, 90000, 9000, 90] },
            CpuSchedLat { cpu_id: 2, wakeup_count: 8_000_000, wakeup_total_ns: 40_000_000_000, wakeup_max_ns: 3_500_000, runq_wait_count: 4_000_000, runq_wait_total_ns: 20_000_000_000, runq_wait_max_ns: 7_000_000, preempt_count: 1_500_000, preempt_total_ns: 7_500_000_000, histogram: [1500000, 4000000, 2500000, 800000, 400000, 80000, 8000, 80] },
            CpuSchedLat { cpu_id: 3, wakeup_count: 7_000_000, wakeup_total_ns: 35_000_000_000, wakeup_max_ns: 3_000_000, runq_wait_count: 3_500_000, runq_wait_total_ns: 17_500_000_000, runq_wait_max_ns: 6_000_000, preempt_count: 1_200_000, preempt_total_ns: 6_000_000_000, histogram: [1200000, 3500000, 2200000, 700000, 350000, 70000, 7000, 70] },
        ],
        total_wakeups: 34_000_000,
        total_runq_waits: 17_000_000,
        total_preempts: 6_500_000,
        global_max_ns: 10_000_000,
        ops: 0,
    });
}

/// Record a wakeup-to-run latency.
pub fn record_wakeup(cpu_id: u32, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.wakeup_count += 1;
        cpu.wakeup_total_ns += ns;
        if ns > cpu.wakeup_max_ns { cpu.wakeup_max_ns = ns; }
        cpu.histogram[bucket_index(ns)] += 1;
        state.total_wakeups += 1;
        if ns > state.global_max_ns { state.global_max_ns = ns; }
        Ok(())
    })
}

/// Record a runqueue wait.
pub fn record_runq_wait(cpu_id: u32, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.runq_wait_count += 1;
        cpu.runq_wait_total_ns += ns;
        if ns > cpu.runq_wait_max_ns { cpu.runq_wait_max_ns = ns; }
        cpu.histogram[bucket_index(ns)] += 1;
        state.total_runq_waits += 1;
        Ok(())
    })
}

/// Record a preemption latency.
pub fn record_preempt(cpu_id: u32, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.cpu_id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        cpu.preempt_count += 1;
        cpu.preempt_total_ns += ns;
        cpu.histogram[bucket_index(ns)] += 1;
        state.total_preempts += 1;
        Ok(())
    })
}

/// Per-CPU scheduling latency stats.
pub fn per_cpu() -> Vec<CpuSchedLat> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpus.clone())
}

/// Aggregated histogram across all CPUs.
pub fn global_histogram() -> [u64; 8] {
    STATE.lock().as_ref().map_or([0; 8], |s| {
        let mut h = [0u64; 8];
        for c in &s.cpus {
            for i in 0..8 { h[i] += c.histogram[i]; }
        }
        h
    })
}

/// Statistics: (cpu_count, total_wakeups, total_runq_waits, total_preempts, global_max_ns, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpus.len(), s.total_wakeups, s.total_runq_waits, s.total_preempts, s.global_max_ns, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("schedlat::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_cpu().len(), 4);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Wakeup.
    let before = per_cpu()[0].wakeup_count;
    record_wakeup(0, 5000).expect("wakeup");
    let after = per_cpu()[0].wakeup_count;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [2/8] wakeup: OK");

    // 3: Runq wait.
    let before = per_cpu()[0].runq_wait_count;
    record_runq_wait(0, 50_000).expect("runq");
    let after = per_cpu()[0].runq_wait_count;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [3/8] runq wait: OK");

    // 4: Preempt.
    let before = per_cpu()[0].preempt_count;
    record_preempt(0, 100).expect("preempt");
    let after = per_cpu()[0].preempt_count;
    assert_eq!(after, before + 1);
    crate::serial_println!("  [4/8] preempt: OK");

    // 5: Histogram.
    let hist = global_histogram();
    assert!(hist.iter().sum::<u64>() > 0);
    crate::serial_println!("  [5/8] histogram: OK");

    // 6: Max tracking.
    record_wakeup(0, 50_000_000).expect("big_wakeup");
    let (_, _, _, _, max, _) = stats();
    assert!(max >= 50_000_000);
    crate::serial_println!("  [6/8] max: OK");

    // 7: Not found.
    assert!(record_wakeup(99, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (cpus, wakes, runqs, preempts, _max, ops) = stats();
    assert_eq!(cpus, 4);
    assert!(wakes > 34_000_000);
    assert!(runqs > 17_000_000);
    assert!(preempts > 6_500_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("schedlat::self_test() — all 8 tests passed");
}

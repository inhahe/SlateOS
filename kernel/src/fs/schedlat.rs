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

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** per-CPU scheduling-latency table.
///
/// Seeds NO CPUs and zero counters.  Real latency accounting is wired through
/// [`register_cpu`] (one row per online CPU the scheduler brings up, with zeroed
/// counters and an empty histogram) and the `record_wakeup`/`record_runq_wait`/
/// `record_preempt` functions; until those are called the table is genuinely
/// empty, so `/proc/schedlat` and the `schedlat` kshell command report zeros
/// rather than fabricated numbers — the kernel's hard "never invent data in
/// procfs" rule.
///
/// NOTE: this previously seeded four fictional per-CPU rows (cpu0: 10M wakeups /
/// 50s total / 5M runq waits / 2M preempts / a fully-populated latency histogram;
/// cpu1: 9M wakeups; cpu2: 8M; cpu3: 7M) plus invented aggregate totals
/// (total_wakeups 34M, total_runq_waits 17M, total_preempts 6.5M, global_max_ns
/// 10ms), which `/proc/schedlat` (and the `per_cpu`/`global_histogram` views)
/// then displayed as if they were real measured scheduling latencies.  That demo
/// data was removed; the self-test now builds its own fixtures explicitly via the
/// real API (see [`self_test`]).  The scheduler is expected to call
/// [`register_cpu`] for each CPU it brings online and the record functions on
/// every wakeup/runqueue-wait/preemption event.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        cpus: Vec::new(),
        total_wakeups: 0,
        total_runq_waits: 0,
        total_preempts: 0,
        global_max_ns: 0,
        ops: 0,
    });
}

/// Register a CPU's scheduling-latency row with zeroed counters.
///
/// Called by the scheduler when it brings a CPU online.  Duplicate `cpu_id`
/// fails with [`KernelError::AlreadyExists`]; exceeding [`MAX_CPUS`] fails with
/// [`KernelError::ResourceExhausted`].
pub fn register_cpu(cpu_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.cpus.len() >= MAX_CPUS { return Err(KernelError::ResourceExhausted); }
        if state.cpus.iter().any(|c| c.cpu_id == cpu_id) { return Err(KernelError::AlreadyExists); }
        state.cpus.push(CpuSchedLat {
            cpu_id, wakeup_count: 0, wakeup_total_ns: 0, wakeup_max_ns: 0,
            runq_wait_count: 0, runq_wait_total_ns: 0, runq_wait_max_ns: 0,
            preempt_count: 0, preempt_total_ns: 0, histogram: [0; 8],
        });
        Ok(())
    })
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
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/schedlat must never surface).  Resetting
    // first clears any residue from a prior `schedlat test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated CPUs or counters.
    assert_eq!(per_cpu().len(), 0);
    let (c0, w0, r0, p0, m0, _o0) = stats();
    assert_eq!((c0, w0, r0, p0, m0), (0, 0, 0, 0, 0));
    assert_eq!(global_histogram().iter().sum::<u64>(), 0);
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Register CPUs — zeroed counters; dup id fails; record before register
    // fails (no phantom CPU is created).
    assert!(record_wakeup(0, 5000).is_err());
    register_cpu(0).expect("reg0");
    register_cpu(1).expect("reg1");
    assert!(register_cpu(0).is_err());
    assert_eq!(per_cpu().len(), 2);
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    assert_eq!((c.wakeup_count, c.runq_wait_count, c.preempt_count), (0, 0, 0));
    assert_eq!(c.histogram.iter().sum::<u64>(), 0);
    crate::serial_println!("  [2/8] register: OK");

    // 3: Wakeup — count/total/max rise; 5us lands in bucket 1 (<10us); the
    // global max tracks it.
    record_wakeup(0, 5_000).expect("wakeup");
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    assert_eq!(c.wakeup_count, 1);
    assert_eq!(c.wakeup_total_ns, 5_000);
    assert_eq!(c.wakeup_max_ns, 5_000);
    assert_eq!(c.histogram[1], 1);
    let (_, _, _, _, gmax, _) = stats();
    assert_eq!(gmax, 5_000);
    crate::serial_println!("  [3/8] wakeup: OK");

    // 4: Runq wait — 50us lands in bucket 2 (<100us).
    record_runq_wait(0, 50_000).expect("runq");
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    assert_eq!(c.runq_wait_count, 1);
    assert_eq!(c.runq_wait_total_ns, 50_000);
    assert_eq!(c.histogram[2], 1);
    crate::serial_println!("  [4/8] runq wait: OK");

    // 5: Preempt — 100ns lands in bucket 0 (<1us); preempt does NOT move the
    // global max (only wakeups do, per record_wakeup).
    record_preempt(0, 100).expect("preempt");
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    assert_eq!(c.preempt_count, 1);
    assert_eq!(c.histogram[0], 1);
    let (_, _, _, _, gmax, _) = stats();
    assert_eq!(gmax, 5_000); // unchanged by the 100ns preempt
    // global histogram so far = one event in buckets 0, 1, 2 = sum 3.
    let g = global_histogram();
    assert_eq!((g[0], g[1], g[2]), (1, 1, 1));
    assert_eq!(g.iter().sum::<u64>(), 3);
    crate::serial_println!("  [5/8] preempt + histogram: OK");

    // 6: Max tracking — a 50ms wakeup lands in bucket 5 (<100ms) and raises the
    // global max and the per-CPU wakeup max.
    record_wakeup(0, 50_000_000).expect("big_wakeup");
    let c = per_cpu().into_iter().find(|c| c.cpu_id == 0).expect("find0");
    assert_eq!(c.wakeup_max_ns, 50_000_000);
    assert_eq!(c.histogram[5], 1);
    let (_, _, _, _, gmax, _) = stats();
    assert_eq!(gmax, 50_000_000);
    crate::serial_println!("  [6/8] max: OK");

    // 7: Unknown CPU → NotFound on every record path.
    assert!(record_wakeup(99, 0).is_err());
    assert!(record_runq_wait(99, 0).is_err());
    assert!(record_preempt(99, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Aggregate stats are exact: 2 wakeups + 1 runq wait + 1 preempt on cpu0;
    // cpu1 untouched.
    let (cpus, wakes, runqs, preempts, max, ops) = stats();
    assert_eq!(cpus, 2);
    assert_eq!(wakes, 2);
    assert_eq!(runqs, 1);
    assert_eq!(preempts, 1);
    assert_eq!(max, 50_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/schedlat table.
    *STATE.lock() = None;

    crate::serial_println!("schedlat::self_test() — all 8 tests passed");
}

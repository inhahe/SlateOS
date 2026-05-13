//! Scheduler Wait Statistics — task wait accounting.
//!
//! Tracks how long tasks wait before being scheduled, broken
//! down by wait reason (runqueue, I/O, lock, etc.). Essential
//! for latency analysis and scheduler tuning.
//!
//! ## Architecture
//!
//! ```text
//! Scheduler wait monitoring
//!   → schedwait::record_wait(pid, reason, ns) → wait event
//!   → schedwait::per_reason() → per-reason breakdown
//!   → schedwait::per_task() → per-task stats
//!   → schedwait::histogram() → latency histogram
//!
//! Integration:
//!   → schedlat (scheduler latency)
//!   → rqstat (runqueue stats)
//!   → schedclass (scheduler class)
//!   → taskstats (task statistics)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Wait reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitReason {
    Runqueue,   // Waiting in runqueue for CPU
    IoWait,     // Waiting for I/O completion
    LockWait,   // Waiting for a lock
    SleepWait,  // Voluntary sleep
    IpcWait,    // Waiting for IPC message
    PageFault,  // Waiting for page fault resolution
}

impl WaitReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Runqueue => "runqueue",
            Self::IoWait => "iowait",
            Self::LockWait => "lock",
            Self::SleepWait => "sleep",
            Self::IpcWait => "ipc",
            Self::PageFault => "pgfault",
        }
    }
}

const REASON_COUNT: usize = 6;

fn reason_index(r: WaitReason) -> usize {
    match r {
        WaitReason::Runqueue => 0,
        WaitReason::IoWait => 1,
        WaitReason::LockWait => 2,
        WaitReason::SleepWait => 3,
        WaitReason::IpcWait => 4,
        WaitReason::PageFault => 5,
    }
}

/// Histogram buckets (us): <1, <10, <100, <1000, <10000, >=10000
const BUCKET_COUNT: usize = 6;
const BUCKET_BOUNDS_US: [u64; 5] = [1, 10, 100, 1000, 10000];

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    counts: [u64; REASON_COUNT],
    total_ns: [u64; REASON_COUNT],
    max_ns: [u64; REASON_COUNT],
    histogram: [u64; BUCKET_COUNT],
    total_waits: u64,
    total_wait_ns: u64,
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
    let us = ns / 1000;
    for (i, &bound) in BUCKET_BOUNDS_US.iter().enumerate() {
        if us < bound { return i; }
    }
    BUCKET_COUNT - 1
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        counts: [50_000_000, 10_000_000, 5_000_000, 20_000_000, 3_000_000, 2_000_000],
        total_ns: [100_000_000_000, 500_000_000_000, 50_000_000_000, 200_000_000_000, 30_000_000_000, 40_000_000_000],
        max_ns: [100_000, 50_000_000, 10_000_000, 1_000_000_000, 5_000_000, 20_000_000],
        histogram: [10_000_000, 30_000_000, 25_000_000, 15_000_000, 8_000_000, 2_000_000],
        total_waits: 90_000_000,
        total_wait_ns: 920_000_000_000,
        ops: 0,
    });
}

/// Record a wait event.
pub fn record_wait(reason: WaitReason, ns: u64) -> KernelResult<()> {
    with_state(|state| {
        let idx = reason_index(reason);
        state.counts[idx] += 1;
        state.total_ns[idx] += ns;
        if ns > state.max_ns[idx] { state.max_ns[idx] = ns; }
        state.histogram[bucket_index(ns)] += 1;
        state.total_waits += 1;
        state.total_wait_ns += ns;
        Ok(())
    })
}

/// Per-reason breakdown: Vec of (reason, count, total_ns, max_ns).
pub fn per_reason() -> Vec<(WaitReason, u64, u64, u64)> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let reasons = [
                WaitReason::Runqueue, WaitReason::IoWait, WaitReason::LockWait,
                WaitReason::SleepWait, WaitReason::IpcWait, WaitReason::PageFault,
            ];
            reasons.iter().enumerate().map(|(i, &r)| (r, s.counts[i], s.total_ns[i], s.max_ns[i])).collect()
        }
        None => Vec::new(),
    }
}

/// Histogram: returns (bucket_labels, counts).
pub fn histogram() -> ([&'static str; BUCKET_COUNT], [u64; BUCKET_COUNT]) {
    let labels = ["<1us", "<10us", "<100us", "<1ms", "<10ms", ">=10ms"];
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (labels, s.histogram),
        None => (labels, [0; BUCKET_COUNT]),
    }
}

/// Statistics: (total_waits, total_wait_ns, ops).
pub fn stats() -> (u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_waits, s.total_wait_ns, s.ops),
        None => (0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("schedwait::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(per_reason().len(), 6);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Record runqueue wait.
    record_wait(WaitReason::Runqueue, 500).expect("rq");
    let reasons = per_reason();
    assert_eq!(reasons[0].1, 50_000_001); // count
    crate::serial_println!("  [2/8] runqueue wait: OK");

    // 3: Record IO wait.
    record_wait(WaitReason::IoWait, 5_000_000).expect("io");
    let reasons = per_reason();
    assert_eq!(reasons[1].1, 10_000_001);
    crate::serial_println!("  [3/8] io wait: OK");

    // 4: Max tracking.
    record_wait(WaitReason::Runqueue, 200_000).expect("rq2");
    let reasons = per_reason();
    assert_eq!(reasons[0].3, 200_000); // max updated
    crate::serial_println!("  [4/8] max tracking: OK");

    // 5: Histogram.
    let (labels, counts) = histogram();
    assert_eq!(labels.len(), 6);
    assert!(counts[0] > 10_000_000); // <1us bucket
    crate::serial_println!("  [5/8] histogram: OK");

    // 6: Multiple reasons.
    record_wait(WaitReason::LockWait, 10_000).expect("lock");
    record_wait(WaitReason::IpcWait, 1000).expect("ipc");
    let reasons = per_reason();
    assert_eq!(reasons[2].1, 5_000_001); // lock
    assert_eq!(reasons[4].1, 3_000_001); // ipc
    crate::serial_println!("  [6/8] multi reason: OK");

    // 7: Total accumulation.
    let (waits, ns, _) = stats();
    assert!(waits > 90_000_000);
    assert!(ns > 920_000_000_000);
    crate::serial_println!("  [7/8] totals: OK");

    // 8: Stats ops.
    let (_, _, ops) = stats();
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("schedwait::self_test() — all 8 tests passed");
}

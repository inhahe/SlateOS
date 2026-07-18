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
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise the scheduler-wait statistics state.
///
/// Starts with all per-reason counts, totals, maxima, histogram buckets,
/// and global totals at zero. The six wait-reason slots (runqueue, iowait,
/// lock, sleep, ipc, pgfault) and the six latency-histogram buckets are a
/// fixed structure, so they are always present — but zeroed. The
/// `/proc/schedwait` generator and the `schedwait` kshell command surface
/// this breakdown as if it reflects real observed task waits, so seeding it
/// with invented counts/latencies would be fabricated procfs data. The
/// counters advance only through real [`record_wait`] calls.
///
/// (Previously this seeded fabricated activity — per-reason counts of
/// 50M/10M/5M/20M/3M/2M waits, total wait time in the hundreds of billions
/// of nanoseconds per reason, a populated latency histogram, and global
/// totals of 90M waits over 920s of wait time.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        counts: [0; REASON_COUNT],
        total_ns: [0; REASON_COUNT],
        max_ns: [0; REASON_COUNT],
        histogram: [0; BUCKET_COUNT],
        total_waits: 0,
        total_wait_ns: 0,
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
    // Start from a clean, empty state so the assertions below are exact and
    // no fixtures leak into the live wait statistics afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — six zeroed reason rows, zeroed histogram, zero totals.
    let reasons = per_reason();
    assert_eq!(reasons.len(), 6);
    for (_, count, total, max) in &reasons {
        assert_eq!((*count, *total, *max), (0, 0, 0));
    }
    let (_, hist) = histogram();
    assert_eq!(hist, [0; BUCKET_COUNT]);
    let (waits0, ns0, _) = stats();
    assert_eq!((waits0, ns0), (0, 0));
    crate::serial_println!("  [1/8] empty defaults: OK");

    // 2: Record a runqueue wait (500ns → <1us bucket).
    record_wait(WaitReason::Runqueue, 500).expect("rq");
    let r = per_reason();
    assert_eq!((r[0].1, r[0].2, r[0].3), (1, 500, 500)); // count, total_ns, max_ns
    assert_eq!(histogram().1[0], 1); // <1us
    crate::serial_println!("  [2/8] runqueue wait: OK");

    // 3: Record an I/O wait (5ms → <10ms bucket).
    record_wait(WaitReason::IoWait, 5_000_000).expect("io");
    let r = per_reason();
    assert_eq!((r[1].1, r[1].2, r[1].3), (1, 5_000_000, 5_000_000));
    assert_eq!(histogram().1[4], 1); // <10ms
    crate::serial_println!("  [3/8] io wait: OK");

    // 4: Max tracking — a larger runqueue wait raises the max (200us → <1ms).
    record_wait(WaitReason::Runqueue, 200_000).expect("rq2");
    let r = per_reason();
    assert_eq!(r[0].1, 2);             // two runqueue waits
    assert_eq!(r[0].2, 200_500);       // 500 + 200_000
    assert_eq!(r[0].3, 200_000);       // max raised
    assert_eq!(histogram().1[3], 1);   // <1ms
    crate::serial_println!("  [4/8] max tracking: OK");

    // 5: Histogram structure — six buckets, exact placements so far.
    let (labels, counts) = histogram();
    assert_eq!(labels.len(), 6);
    assert_eq!(counts, [1, 0, 0, 1, 1, 0]);
    crate::serial_println!("  [5/8] histogram: OK");

    // 6: Multiple reasons (lock 10us → <100us, ipc 1us → <10us).
    record_wait(WaitReason::LockWait, 10_000).expect("lock");
    record_wait(WaitReason::IpcWait, 1000).expect("ipc");
    let r = per_reason();
    assert_eq!(r[2].1, 1); // lock
    assert_eq!(r[4].1, 1); // ipc
    assert_eq!(histogram().1, [1, 1, 1, 1, 1, 0]);
    crate::serial_println!("  [6/8] multi reason: OK");

    // 7: Global totals reflect exactly the five recorded waits.
    let (waits, ns, _) = stats();
    assert_eq!(waits, 5);
    assert_eq!(ns, 500 + 5_000_000 + 200_000 + 10_000 + 1000);
    crate::serial_println!("  [7/8] totals: OK");

    // 8: Stats ops advanced.
    assert!(stats().2 > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("schedwait::self_test() — all 8 tests passed");
}

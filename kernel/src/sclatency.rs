//! Syscall latency histogram.
//!
//! Tracks the distribution of syscall execution times across logarithmic
//! buckets.  Provides a histogram view showing where syscalls spend their
//! time, useful for identifying slow paths and regression detection.
//!
//! ## Design
//!
//! - Uses TSC timestamps at syscall entry/exit for nanosecond precision.
//! - Latencies are bucketed into 12 logarithmic ranges from <1μs to >100ms.
//! - Per-syscall-number tracking for the most common syscalls.
//! - All counters are atomic (lock-free, safe from any context).
//!
//! ## Overhead
//!
//! Two `rdtsc` reads per syscall (~20 cycles total) plus one atomic
//! increment for the histogram bucket (~5 cycles).  Well under 1% of
//! even the fastest syscalls.
//!
//! ## Usage
//!
//! ```ignore
//! // At syscall entry:
//! let start = sclatency::enter();
//!
//! // ... handle syscall ...
//!
//! // At syscall exit:
//! sclatency::exit(start, syscall_nr);
//! ```
//!
//! ## Kshell Command
//!
//! `sclatency` shows the histogram.  `sclatency reset` clears it.
//!
//! ## References
//!
//! - Linux `perf trace --summary`
//! - BPF syscall latency histograms (`biolatency`, `syscount`)
//! - Brendan Gregg, "Systems Performance" (2020), Chapter 5

// Diagnostic/profiling subsystem — all public API for tooling and kshell
// commands; many helpers may not have call sites in production paths yet.
#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of histogram buckets (logarithmic ranges).
const NUM_BUCKETS: usize = 12;

/// Bucket boundaries in nanoseconds.
/// [0] <1μs, [1] 1-2μs, [2] 2-4μs, [3] 4-8μs, [4] 8-16μs,
/// [5] 16-32μs, [6] 32-64μs, [7] 64-128μs, [8] 128-256μs,
/// [9] 256μs-1ms, [10] 1-10ms, [11] >10ms
const BUCKET_THRESHOLDS_NS: [u64; NUM_BUCKETS] = [
    1_000,        // <1μs
    2_000,        // 1-2μs
    4_000,        // 2-4μs
    8_000,        // 4-8μs
    16_000,       // 8-16μs
    32_000,       // 16-32μs
    64_000,       // 32-64μs
    128_000,      // 64-128μs
    256_000,      // 128-256μs
    1_000_000,    // 256μs-1ms
    10_000_000,   // 1-10ms
    100_000_000,  // 10-100ms (bucket 11 is >100ms)
];

/// Bucket labels for display.
const BUCKET_LABELS: [&str; NUM_BUCKETS] = [
    "<1us",
    "1-2us",
    "2-4us",
    "4-8us",
    "8-16us",
    "16-32us",
    "32-64us",
    "64-128us",
    "128-256us",
    "256us-1ms",
    "1-10ms",
    ">10ms",
];

/// Number of individual syscall numbers to track (0..MAX_TRACKED).
const MAX_TRACKED_SYSCALLS: usize = 16;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Global histogram buckets.
static BUCKETS: [AtomicU64; NUM_BUCKETS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

/// Total syscalls measured.
static TOTAL_CALLS: AtomicU64 = AtomicU64::new(0);

/// Cumulative latency in cycles (for mean calculation).
static TOTAL_CYCLES: AtomicU64 = AtomicU64::new(0);

/// Minimum latency observed (cycles).
static MIN_CYCLES: AtomicU64 = AtomicU64::new(u64::MAX);

/// Maximum latency observed (cycles).
static MAX_CYCLES: AtomicU64 = AtomicU64::new(0);

/// Per-syscall call count (for top-N display).
static PER_SYSCALL_COUNT: [AtomicU64; MAX_TRACKED_SYSCALLS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

/// Per-syscall cumulative cycles (for per-syscall mean).
static PER_SYSCALL_CYCLES: [AtomicU64; MAX_TRACKED_SYSCALLS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

/// Whether tracking is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Mark syscall entry.  Returns the TSC timestamp.
///
/// Called at the top of the syscall dispatch path.
#[inline]
#[must_use]
pub fn enter() -> u64 {
    if !ENABLED.load(Ordering::Relaxed) {
        return 0;
    }
    crate::bench::rdtsc()
}

/// Mark syscall exit and record the latency.
///
/// Called at the end of the syscall dispatch path.
/// `start` is the value returned by [`enter`].
/// `syscall_nr` is the syscall number (for per-syscall breakdown).
#[inline]
pub fn exit(start: u64, syscall_nr: u64) {
    if start == 0 {
        return;
    }
    let end = crate::bench::rdtsc();
    let elapsed_cycles = end.saturating_sub(start);

    // Convert to nanoseconds for bucketing.
    let elapsed_ns = crate::bench::cycles_to_ns(elapsed_cycles);

    // Find the right bucket.
    let bucket = find_bucket(elapsed_ns);
    BUCKETS[bucket].fetch_add(1, Ordering::Relaxed);

    // Update global stats.
    TOTAL_CALLS.fetch_add(1, Ordering::Relaxed);
    TOTAL_CYCLES.fetch_add(elapsed_cycles, Ordering::Relaxed);

    // Update min (CAS loop).
    loop {
        let current_min = MIN_CYCLES.load(Ordering::Relaxed);
        if elapsed_cycles >= current_min {
            break;
        }
        if MIN_CYCLES.compare_exchange_weak(
            current_min, elapsed_cycles, Ordering::Relaxed, Ordering::Relaxed
        ).is_ok() {
            break;
        }
    }

    // Update max (CAS loop).
    loop {
        let current_max = MAX_CYCLES.load(Ordering::Relaxed);
        if elapsed_cycles <= current_max {
            break;
        }
        if MAX_CYCLES.compare_exchange_weak(
            current_max, elapsed_cycles, Ordering::Relaxed, Ordering::Relaxed
        ).is_ok() {
            break;
        }
    }

    // Per-syscall tracking.
    let nr = syscall_nr as usize;
    if nr < MAX_TRACKED_SYSCALLS {
        PER_SYSCALL_COUNT[nr].fetch_add(1, Ordering::Relaxed);
        PER_SYSCALL_CYCLES[nr].fetch_add(elapsed_cycles, Ordering::Relaxed);
    }
}

/// Find the histogram bucket for a given latency in nanoseconds.
#[inline]
fn find_bucket(ns: u64) -> usize {
    for (i, &threshold) in BUCKET_THRESHOLDS_NS.iter().enumerate() {
        if ns < threshold {
            return i;
        }
    }
    NUM_BUCKETS - 1
}

// ---------------------------------------------------------------------------
// Statistics readout
// ---------------------------------------------------------------------------

/// Histogram snapshot.
pub struct LatencyStats {
    /// Counts per bucket.
    pub buckets: [u64; NUM_BUCKETS],
    /// Total syscalls measured.
    pub total_calls: u64,
    /// Minimum latency in nanoseconds.
    pub min_ns: u64,
    /// Maximum latency in nanoseconds.
    pub max_ns: u64,
    /// Mean latency in nanoseconds.
    pub mean_ns: u64,
}

/// Read the current histogram.
#[must_use]
pub fn stats() -> LatencyStats {
    let total = TOTAL_CALLS.load(Ordering::Relaxed);
    let total_cyc = TOTAL_CYCLES.load(Ordering::Relaxed);
    let min_cyc = MIN_CYCLES.load(Ordering::Relaxed);
    let max_cyc = MAX_CYCLES.load(Ordering::Relaxed);

    let min_ns = if min_cyc == u64::MAX { 0 } else { crate::bench::cycles_to_ns(min_cyc) };
    let max_ns = crate::bench::cycles_to_ns(max_cyc);
    let mean_ns = if total > 0 {
        crate::bench::cycles_to_ns(total_cyc / total)
    } else {
        0
    };

    let mut buckets = [0u64; NUM_BUCKETS];
    for (i, b) in BUCKETS.iter().enumerate() {
        buckets[i] = b.load(Ordering::Relaxed);
    }

    LatencyStats {
        buckets,
        total_calls: total,
        min_ns,
        max_ns,
        mean_ns,
    }
}

/// Get per-syscall statistics.
///
/// Returns (syscall_nr, call_count, mean_cycles) tuples for active syscalls.
#[must_use]
pub fn per_syscall_stats() -> alloc::vec::Vec<(usize, u64, u64)> {
    let mut result = alloc::vec::Vec::new();
    for i in 0..MAX_TRACKED_SYSCALLS {
        let count = PER_SYSCALL_COUNT[i].load(Ordering::Relaxed);
        if count > 0 {
            let cycles = PER_SYSCALL_CYCLES[i].load(Ordering::Relaxed);
            let mean = cycles / count.max(1);
            result.push((i, count, mean));
        }
    }
    // Sort by call count descending.
    result.sort_unstable_by_key(|e| core::cmp::Reverse(e.1));
    result
}

/// Get bucket labels for display.
#[must_use]
pub fn bucket_labels() -> &'static [&'static str; NUM_BUCKETS] {
    &BUCKET_LABELS
}

/// Reset all counters.
pub fn reset() {
    for b in &BUCKETS {
        b.store(0, Ordering::Relaxed);
    }
    TOTAL_CALLS.store(0, Ordering::Relaxed);
    TOTAL_CYCLES.store(0, Ordering::Relaxed);
    MIN_CYCLES.store(u64::MAX, Ordering::Relaxed);
    MAX_CYCLES.store(0, Ordering::Relaxed);
    for c in &PER_SYSCALL_COUNT {
        c.store(0, Ordering::Relaxed);
    }
    for c in &PER_SYSCALL_CYCLES {
        c.store(0, Ordering::Relaxed);
    }
}

/// Enable or disable tracking.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if tracking is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

extern crate alloc;

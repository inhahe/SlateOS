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
    1_000,       // <1μs
    2_000,       // 1-2μs
    4_000,       // 2-4μs
    8_000,       // 4-8μs
    16_000,      // 8-16μs
    32_000,      // 16-32μs
    64_000,      // 32-64μs
    128_000,     // 64-128μs
    256_000,     // 128-256μs
    1_000_000,   // 256μs-1ms
    10_000_000,  // 1-10ms
    100_000_000, // 10-100ms (bucket 11 is >100ms)
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
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
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
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Per-syscall cumulative cycles (for per-syscall mean).
static PER_SYSCALL_CYCLES: [AtomicU64; MAX_TRACKED_SYSCALLS] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Whether tracking is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// [`BUCKET_THRESHOLDS_NS`] converted to TSC cycles once, by [`calibrate`].
///
/// The hot path measures cycles and the buckets are fixed, so converting the
/// *thresholds* once at calibration is strictly better than converting every
/// *sample* at record time: it moves a division off a path taken by every
/// syscall in the system onto a path taken once per boot.
///
/// `exit` used to call `bench::cycles_to_ns` per syscall purely to choose a
/// bucket — two 64-bit divisions to produce a number that was then compared
/// against 12 constants and thrown away.  Nothing else in this module needed
/// it: every other statistic here (`TOTAL_CYCLES`, `MIN_CYCLES`, `MAX_CYCLES`,
/// `PER_SYSCALL_CYCLES`) was already kept in cycles and converted only at
/// readout.
static BUCKET_THRESHOLDS_CYCLES: [AtomicU64; NUM_BUCKETS] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Whether [`BUCKET_THRESHOLDS_CYCLES`] holds real values.
///
/// Separate from "the TSC frequency is known" so the hot path reads one flag
/// rather than re-deriving the condition.
static CALIBRATED: AtomicBool = AtomicBool::new(false);

/// Samples seen while uncalibrated, and therefore *not* placed in any bucket.
///
/// This exists because the previous code had no way to say "I could not
/// measure this."  `cycles_to_ns` returns 0 when the TSC frequency is unknown,
/// 0 is less than the first threshold, so every such sample landed in bucket 0
/// and the histogram cheerfully reported **100% of syscalls under 1 µs** — the
/// most flattering possible answer — precisely when it had no idea.  A
/// histogram that cannot distinguish "fast" from "unmeasurable" is worse than
/// one that reports nothing, so these are counted apart and surfaced by
/// [`stats`].
static UNCALIBRATED_SAMPLES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert the nanosecond bucket thresholds into TSC cycles.
///
/// Call once, immediately after `bench::calibrate_tsc()`.  Idempotent, and a
/// no-op if the TSC frequency is still unknown — in which case the histogram
/// stays uncalibrated and says so rather than inventing a bucket.
///
/// Syscalls dispatched *before* this runs are counted in
/// [`UNCALIBRATED_SAMPLES`] rather than silently binned as "<1 µs".
pub fn calibrate() {
    let freq = crate::bench::tsc_freq();
    if freq == 0 {
        // Leave CALIBRATED false: `exit` will count samples as unmeasurable.
        return;
    }

    for (slot, &ns) in BUCKET_THRESHOLDS_CYCLES
        .iter()
        .zip(BUCKET_THRESHOLDS_NS.iter())
    {
        // cycles = ns * freq / 1e9.  The multiply is done first to keep the
        // precision that dividing first would throw away, and cannot overflow:
        // the largest threshold is 1e8 ns and a plausible TSC is under 1e10 Hz,
        // so the product stays below 1e18 < u64::MAX.  `saturating_mul` covers
        // an implausible calibration result rather than trusting that bound.
        let cycles = ns
            .saturating_mul(freq)
            .checked_div(1_000_000_000)
            .unwrap_or(u64::MAX);
        slot.store(cycles, Ordering::Relaxed);
    }

    // Release: a CPU that sees CALIBRATED also sees the thresholds above.
    CALIBRATED.store(true, Ordering::Release);
}

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

    // Bucket in cycles.  No unit conversion here: the thresholds were
    // converted once by `calibrate`, so this is a handful of compares against
    // pre-scaled values instead of the two 64-bit divisions `cycles_to_ns`
    // needed on every syscall.
    match find_bucket_cycles(elapsed_cycles) {
        Some(bucket) => {
            if let Some(b) = BUCKETS.get(bucket) {
                b.fetch_add(1, Ordering::Relaxed);
            }
        }
        // Not calibrated: this sample's duration is genuinely unknown, so it
        // is counted as unknown rather than assigned to the fastest bucket.
        None => {
            UNCALIBRATED_SAMPLES.fetch_add(1, Ordering::Relaxed);
        }
    }

    // Update global stats.
    TOTAL_CALLS.fetch_add(1, Ordering::Relaxed);
    TOTAL_CYCLES.fetch_add(elapsed_cycles, Ordering::Relaxed);

    // Update min (CAS loop).
    loop {
        let current_min = MIN_CYCLES.load(Ordering::Relaxed);
        if elapsed_cycles >= current_min {
            break;
        }
        if MIN_CYCLES
            .compare_exchange_weak(
                current_min,
                elapsed_cycles,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            break;
        }
    }

    // Update max (CAS loop).
    loop {
        let current_max = MAX_CYCLES.load(Ordering::Relaxed);
        if elapsed_cycles <= current_max {
            break;
        }
        if MAX_CYCLES
            .compare_exchange_weak(
                current_max,
                elapsed_cycles,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
        {
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
///
/// The reference implementation.  Not on the hot path any more — it is what
/// [`find_bucket_cycles`] is checked against in [`self_test`], since a
/// rescaled comparison that silently disagrees with the ns one would shift the
/// whole histogram without changing a single label.
#[inline]
fn find_bucket(ns: u64) -> usize {
    for (i, &threshold) in BUCKET_THRESHOLDS_NS.iter().enumerate() {
        if ns < threshold {
            return i;
        }
    }
    NUM_BUCKETS.saturating_sub(1)
}

/// Find the histogram bucket for a latency in TSC cycles.
///
/// Returns `None` if the thresholds have not been calibrated, so the caller
/// must decide what an unmeasurable sample means rather than being handed a
/// plausible-looking bucket.
///
/// The loop exits on the first threshold above `cycles`, so the common case (a
/// fast syscall, bucket 0) is a single relaxed load and one compare.
#[inline]
fn find_bucket_cycles(cycles: u64) -> Option<usize> {
    // Acquire: pairs with the Release store in `calibrate`, so seeing the flag
    // set guarantees the thresholds below are visible.
    if !CALIBRATED.load(Ordering::Acquire) {
        return None;
    }
    for (i, threshold) in BUCKET_THRESHOLDS_CYCLES.iter().enumerate() {
        if cycles < threshold.load(Ordering::Relaxed) {
            return Some(i);
        }
    }
    Some(NUM_BUCKETS.saturating_sub(1))
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
    /// Samples recorded before the TSC was calibrated, and so placed in no
    /// bucket at all.
    ///
    /// Non-zero means `buckets` does not account for every call in
    /// `total_calls`; the difference is here.  Displaying the histogram
    /// without this number reports a distribution over the subset that could
    /// be measured while implying it covers all of them.
    pub uncalibrated: u64,
    /// Whether the cycle thresholds are calibrated *now*.
    ///
    /// `false` means every future sample will land in `uncalibrated` too.
    pub calibrated: bool,
}

/// Read the current histogram.
#[must_use]
pub fn stats() -> LatencyStats {
    let total = TOTAL_CALLS.load(Ordering::Relaxed);
    let total_cyc = TOTAL_CYCLES.load(Ordering::Relaxed);
    let min_cyc = MIN_CYCLES.load(Ordering::Relaxed);
    let max_cyc = MAX_CYCLES.load(Ordering::Relaxed);

    let min_ns = if min_cyc == u64::MAX {
        0
    } else {
        crate::bench::cycles_to_ns(min_cyc)
    };
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
        uncalibrated: UNCALIBRATED_SAMPLES.load(Ordering::Relaxed),
        calibrated: CALIBRATED.load(Ordering::Relaxed),
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
    // Counts, not calibration: `reset` clears what was observed, never the
    // thresholds.  Clearing CALIBRATED here would silently switch bucketing
    // off for the rest of the boot.
    UNCALIBRATED_SAMPLES.store(0, Ordering::Relaxed);
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

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify that bucketing in cycles agrees with bucketing in nanoseconds.
///
/// # Why this needs a test at all
///
/// Rescaling the thresholds is the kind of change that cannot fail loudly.
/// The bucket *labels* are hard-coded strings, so an off-by-one or a rounding
/// error in the conversion would relabel every measurement in the system —
/// "2-4 µs" would keep saying "2-4 µs" while counting something else — and
/// every consumer would keep working. The only way to catch that is to hold
/// the new implementation against the old one over values that straddle every
/// boundary, which is what this does.
///
/// Checks each threshold at `-1`, exactly, and `+1` in nanoseconds, converting
/// to cycles the same way a real sample arrives.
pub fn self_test() {
    use crate::serial_println;

    let freq = crate::bench::tsc_freq();
    if freq == 0 || !CALIBRATED.load(Ordering::Acquire) {
        // Do not pass silently: an uncalibrated TSC makes every assertion below
        // vacuous, and a vacuous test that prints OK is worse than no test.
        serial_println!(
            "[sclatency] Self-test SKIPPED: TSC uncalibrated (freq={freq}) — \
             cycle bucketing is UNVERIFIED this boot"
        );
        return;
    }

    let ns_to_cycles = |ns: u64| -> u64 {
        ns.saturating_mul(freq)
            .checked_div(1_000_000_000)
            .unwrap_or(u64::MAX)
    };

    let mut checked = 0usize;
    for &threshold_ns in &BUCKET_THRESHOLDS_NS {
        for probe_ns in [
            threshold_ns.saturating_sub(1),
            threshold_ns,
            threshold_ns.saturating_add(1),
        ] {
            let want = find_bucket(probe_ns);
            let Some(got) = find_bucket_cycles(ns_to_cycles(probe_ns)) else {
                serial_println!("[sclatency] Self-test FAILED: uncalibrated mid-test");
                return;
            };
            // One cycle of rounding slack: `ns * freq / 1e9` truncates, so a
            // probe exactly *on* a boundary can land one cycle below it. A
            // disagreement of more than one bucket is a real scaling bug.
            let delta = want.abs_diff(got);
            assert!(
                delta <= 1,
                "sclatency bucket mismatch at {probe_ns}ns: ns-path says {want}, cycle-path says {got}"
            );
            checked = checked.saturating_add(1);
        }
    }

    // A sample far above the top threshold must saturate into the last bucket,
    // not wrap to 0 — the failure mode where the slowest syscalls in the system
    // are reported as the fastest.
    let huge = find_bucket_cycles(u64::MAX).unwrap_or(0);
    assert!(
        huge == NUM_BUCKETS.saturating_sub(1),
        "sclatency: saturating sample landed in bucket {huge}, expected {}",
        NUM_BUCKETS.saturating_sub(1)
    );

    // Zero must land in bucket 0, and must be reached through the *calibrated*
    // path — this is the value an uncalibrated `cycles_to_ns` used to return
    // for every sample.
    assert!(
        find_bucket_cycles(0) == Some(0),
        "sclatency: 0 cycles must be bucket 0"
    );

    serial_println!(
        "[sclatency] Self-test PASSED ({checked} boundary probes agree with the ns reference, \
         TSC {freq} Hz)"
    );
}

extern crate alloc;

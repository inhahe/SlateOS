//! Allocation latency histogram — measures and profiles alloc/free timing.
//!
//! Tracks the wall-clock latency of every frame allocation and free
//! operation, bucketing results into a logarithmic histogram.  This
//! enables answering performance questions:
//! - What percentage of allocations complete in < 1µs?
//! - Are there occasional multi-millisecond stalls?
//! - Is the per-CPU cache effective (most allocs in the fast bucket)?
//!
//! ## Design
//!
//! Uses TSC-based timing wrapped around each allocation.  Latencies
//! are bucketed into power-of-2 cycle ranges, then reported in
//! human-readable microseconds using the calibrated TSC frequency.
//!
//! Bucket boundaries (in TSC cycles, assuming ~3 GHz TSC):
//!   0: < 64 cycles       (~20ns)  — per-CPU cache hit
//!   1: 64-127 cycles     (~40ns)  — fast path
//!   2: 128-255 cycles    (~85ns)  — buddy allocator hit
//!   3: 256-511 cycles    (~170ns) — lock contention
//!   4: 512-1023 cycles   (~340ns) — moderate contention
//!   5: 1024-2047 cycles  (~680ns) — significant contention
//!   6: 2048-4095 cycles  (~1.3µs) — slow path
//!   7: 4096-8191 cycles  (~2.7µs) — very slow
//!   8: 8192-16383 cycles (~5.5µs) — extremely slow
//!   9: 16384+ cycles     (>5µs)   — stall (reclaim, compaction, etc.)
//!
//! ## Overhead
//!
//! Two `rdtsc` calls per operation (~20 cycles each) + one atomic
//! increment.  Total overhead: ~50-60 cycles per alloc/free (~20ns).
//! Acceptable for development/profiling; can be disabled in production.
//!
//! ## References
//!
//! - Linux `mm/page_alloc.c` — `__alloc_pages` latency tracing
//! - BPF histograms (bcc `funclatency`) — power-of-2 bucket approach
//! - Brendan Gregg, "Systems Performance" — latency distribution analysis

// Diagnostic/profiling subsystem — all public API for tooling and kshell
// commands; many helpers may not have call sites in production paths yet.
#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of histogram buckets (power-of-2 ranges).
const NUM_BUCKETS: usize = 10;

/// Base shift: bucket 0 covers cycles 0..(1 << BASE_SHIFT).
const BASE_SHIFT: u32 = 6; // 2^6 = 64 cycles

/// TSC frequency in MHz (approximate, for display only).
/// Calibrated at boot from the APIC timer or HPET.
/// Default 3000 MHz (3 GHz) as a reasonable starting point.
static TSC_MHZ: AtomicU64 = AtomicU64::new(3000);

// ---------------------------------------------------------------------------
// Histograms
// ---------------------------------------------------------------------------

/// Allocation latency histogram buckets.
static ALLOC_HIST: [AtomicU64; NUM_BUCKETS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Free latency histogram buckets.
static FREE_HIST: [AtomicU64; NUM_BUCKETS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Total allocation time (sum of all alloc latencies, in cycles).
static ALLOC_TOTAL_CYCLES: AtomicU64 = AtomicU64::new(0);

/// Total free time.
static FREE_TOTAL_CYCLES: AtomicU64 = AtomicU64::new(0);

/// Total allocations measured.
static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total frees measured.
static FREE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Maximum alloc latency observed (cycles).
static ALLOC_MAX: AtomicU64 = AtomicU64::new(0);

/// Maximum free latency observed (cycles).
static FREE_MAX: AtomicU64 = AtomicU64::new(0);

/// Whether latency measurement is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

// ---------------------------------------------------------------------------
// Public API — measurement
// ---------------------------------------------------------------------------

/// Begin a latency measurement.  Returns the start TSC value.
///
/// Call this before an allocation/free, then pass the result to
/// `end_alloc()` or `end_free()` after the operation completes.
#[inline]
pub fn begin() -> u64 {
    if !ENABLED.load(Ordering::Relaxed) {
        return 0;
    }
    rdtsc()
}

/// End an allocation latency measurement.
///
/// `start` is the value returned by `begin()`.
#[inline]
pub fn end_alloc(start: u64) {
    if start == 0 {
        return;
    }
    let end = rdtsc();
    let elapsed = end.saturating_sub(start);
    record_latency(&ALLOC_HIST, &ALLOC_TOTAL_CYCLES, &ALLOC_COUNT, &ALLOC_MAX, elapsed);
}

/// End a free latency measurement.
#[inline]
pub fn end_free(start: u64) {
    if start == 0 {
        return;
    }
    let end = rdtsc();
    let elapsed = end.saturating_sub(start);
    record_latency(&FREE_HIST, &FREE_TOTAL_CYCLES, &FREE_COUNT, &FREE_MAX, elapsed);
}

/// Record a raw latency value into the histogram.
#[inline]
fn record_latency(
    hist: &[AtomicU64; NUM_BUCKETS],
    total: &AtomicU64,
    count: &AtomicU64,
    max: &AtomicU64,
    cycles: u64,
) {
    let bucket = cycles_to_bucket(cycles);
    hist[bucket].fetch_add(1, Ordering::Relaxed);
    total.fetch_add(cycles, Ordering::Relaxed);
    count.fetch_add(1, Ordering::Relaxed);

    // Update max (relaxed CAS loop — benign races are acceptable).
    let mut current_max = max.load(Ordering::Relaxed);
    while cycles > current_max {
        match max.compare_exchange_weak(
            current_max, cycles, Ordering::Relaxed, Ordering::Relaxed
        ) {
            Ok(_) => break,
            Err(actual) => current_max = actual,
        }
    }
}

/// Map a cycle count to a bucket index.
///
/// Bucket 0 covers [0, 2^BASE_SHIFT) = [0, 64).
/// Bucket 1 covers [2^BASE_SHIFT, 2^(BASE_SHIFT+1)) = [64, 128).
/// Bucket 2 covers [128, 256).
/// ...
/// Last bucket is a catch-all for anything above.
#[inline]
fn cycles_to_bucket(cycles: u64) -> usize {
    if cycles == 0 {
        return 0;
    }
    // Find the highest set bit position (floor(log2(cycles))).
    let bits = 63 - cycles.leading_zeros();
    if bits < BASE_SHIFT {
        // Below 2^BASE_SHIFT → bucket 0.
        0
    } else {
        // Bucket = floor(log2(cycles)) - BASE_SHIFT + 1.
        let bucket = (bits - BASE_SHIFT + 1) as usize;
        bucket.min(NUM_BUCKETS - 1)
    }
}

// ---------------------------------------------------------------------------
// Public API — control
// ---------------------------------------------------------------------------

/// Enable latency measurement.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable latency measurement.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether measurement is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Set the TSC frequency (for converting cycles to microseconds).
///
/// Should be called once during boot after TSC calibration.
pub fn set_tsc_mhz(mhz: u64) {
    TSC_MHZ.store(mhz, Ordering::Release);
}

/// Reset all histograms and counters.
pub fn reset() {
    for b in &ALLOC_HIST {
        b.store(0, Ordering::Relaxed);
    }
    for b in &FREE_HIST {
        b.store(0, Ordering::Relaxed);
    }
    ALLOC_TOTAL_CYCLES.store(0, Ordering::Relaxed);
    FREE_TOTAL_CYCLES.store(0, Ordering::Relaxed);
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    FREE_COUNT.store(0, Ordering::Relaxed);
    ALLOC_MAX.store(0, Ordering::Relaxed);
    FREE_MAX.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API — reporting
// ---------------------------------------------------------------------------

/// Latency histogram snapshot for one operation type.
#[derive(Debug, Clone, Copy)]
pub struct LatencyHist {
    /// Bucket counts.
    pub buckets: [u64; NUM_BUCKETS],
    /// Total operations measured.
    pub count: u64,
    /// Sum of all latencies (cycles).
    pub total_cycles: u64,
    /// Maximum latency observed (cycles).
    pub max_cycles: u64,
    /// Average latency (cycles), or 0 if no samples.
    pub avg_cycles: u64,
    /// TSC frequency used for conversion (MHz).
    pub tsc_mhz: u64,
}

impl LatencyHist {
    /// Convert cycles to nanoseconds.
    pub fn cycles_to_ns(&self, cycles: u64) -> u64 {
        if self.tsc_mhz == 0 {
            return 0;
        }
        // cycles / (tsc_mhz * 1000) * 1_000_000_000
        // = cycles * 1000 / tsc_mhz
        cycles.saturating_mul(1000).checked_div(self.tsc_mhz).unwrap_or(0)
    }

    /// Get the bucket lower bound in cycles.
    pub fn bucket_lower_cycles(bucket: usize) -> u64 {
        if bucket == 0 {
            0
        } else {
            1u64 << (BASE_SHIFT as u64 + bucket as u64 - 1)
        }
    }

    /// Get the bucket upper bound in cycles (exclusive).
    pub fn bucket_upper_cycles(bucket: usize) -> u64 {
        if bucket >= NUM_BUCKETS - 1 {
            u64::MAX // Catch-all bucket.
        } else {
            1u64 << (BASE_SHIFT as u64 + bucket as u64)
        }
    }

    /// Calculate the Nth percentile (0-100) in cycles.
    ///
    /// Finds the bucket containing the Nth percentile and returns
    /// the upper bound of that bucket as the estimate.
    pub fn percentile(&self, pct: u8) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let target = (self.count as u128)
            .saturating_mul(pct as u128)
            .checked_div(100)
            .unwrap_or(0) as u64;
        let mut cumulative: u64 = 0;
        for (i, &count) in self.buckets.iter().enumerate() {
            cumulative = cumulative.saturating_add(count);
            if cumulative >= target {
                return Self::bucket_upper_cycles(i);
            }
        }
        self.max_cycles
    }
}

/// Get the allocation latency histogram.
#[must_use]
pub fn alloc_histogram() -> LatencyHist {
    let mut buckets = [0u64; NUM_BUCKETS];
    for (i, b) in ALLOC_HIST.iter().enumerate() {
        buckets[i] = b.load(Ordering::Relaxed);
    }
    let count = ALLOC_COUNT.load(Ordering::Relaxed);
    let total = ALLOC_TOTAL_CYCLES.load(Ordering::Relaxed);
    let max = ALLOC_MAX.load(Ordering::Relaxed);
    let avg = if count > 0 { total / count } else { 0 };
    let tsc_mhz = TSC_MHZ.load(Ordering::Relaxed);

    LatencyHist { buckets, count, total_cycles: total, max_cycles: max, avg_cycles: avg, tsc_mhz }
}

/// Get the free latency histogram.
#[must_use]
pub fn free_histogram() -> LatencyHist {
    let mut buckets = [0u64; NUM_BUCKETS];
    for (i, b) in FREE_HIST.iter().enumerate() {
        buckets[i] = b.load(Ordering::Relaxed);
    }
    let count = FREE_COUNT.load(Ordering::Relaxed);
    let total = FREE_TOTAL_CYCLES.load(Ordering::Relaxed);
    let max = FREE_MAX.load(Ordering::Relaxed);
    let avg = if count > 0 { total / count } else { 0 };
    let tsc_mhz = TSC_MHZ.load(Ordering::Relaxed);

    LatencyHist { buckets, count, total_cycles: total, max_cycles: max, avg_cycles: avg, tsc_mhz }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read TSC.
#[inline]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: rdtsc is always available on x86_64 and has no side effects.
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for allocation latency histogramming.
pub fn self_test() {
    serial_println!("[alloc_lat] Running self-test...");

    // Test 1: Reset clears everything.
    reset();
    let h = alloc_histogram();
    assert_eq!(h.count, 0);
    assert_eq!(h.max_cycles, 0);
    for &b in &h.buckets {
        assert_eq!(b, 0);
    }
    serial_println!("[alloc_lat]   Reset: OK");

    // Test 2: begin/end records a measurement.
    let start = begin();
    // Simulate some work (just a few iterations).
    let mut dummy = 0u64;
    for i in 0..100u64 {
        dummy = dummy.wrapping_add(i);
    }
    core::hint::black_box(dummy);
    end_alloc(start);

    let h = alloc_histogram();
    assert_eq!(h.count, 1, "should have 1 alloc measurement");
    assert!(h.max_cycles > 0, "max should be non-zero");
    assert!(h.total_cycles > 0, "total should be non-zero");
    // Check that exactly one bucket was incremented.
    let total_bucketed: u64 = h.buckets.iter().sum();
    assert_eq!(total_bucketed, 1);
    serial_println!("[alloc_lat]   Single measurement: OK ({}ns)",
        h.cycles_to_ns(h.avg_cycles));

    // Test 3: Multiple measurements distribute across buckets.
    reset();
    for _ in 0..50 {
        let s = begin();
        core::hint::black_box(0u64);
        end_alloc(s);
    }
    let h = alloc_histogram();
    assert_eq!(h.count, 50);
    serial_println!("[alloc_lat]   50 fast allocs: avg={}ns, max={}ns",
        h.cycles_to_ns(h.avg_cycles), h.cycles_to_ns(h.max_cycles));

    // Test 4: Free histogram is separate.
    let start = begin();
    core::hint::black_box(0u64);
    end_free(start);
    let fh = free_histogram();
    assert_eq!(fh.count, 1);
    let ah = alloc_histogram();
    assert_eq!(ah.count, 50); // Unchanged.
    serial_println!("[alloc_lat]   Separate alloc/free histograms: OK");

    // Test 5: Bucket classification.
    assert_eq!(cycles_to_bucket(0), 0);
    assert_eq!(cycles_to_bucket(32), 0);   // < 64 → bucket 0
    assert_eq!(cycles_to_bucket(63), 0);   // < 64 → bucket 0
    assert_eq!(cycles_to_bucket(64), 1);   // 64-127 → bucket 1
    assert_eq!(cycles_to_bucket(128), 2);  // 128-255 → bucket 2
    assert_eq!(cycles_to_bucket(1000), 4); // 512-1023 → bucket 4
    assert_eq!(cycles_to_bucket(100_000), 9); // overflow → last bucket
    serial_println!("[alloc_lat]   Bucket classification: OK");

    // Test 6: Percentile calculation.
    reset();
    // Fill bucket 0 with 90 entries, bucket 5 with 10 entries.
    for _ in 0..90 {
        ALLOC_HIST[0].fetch_add(1, Ordering::Relaxed);
    }
    for _ in 0..10 {
        ALLOC_HIST[5].fetch_add(1, Ordering::Relaxed);
    }
    ALLOC_COUNT.store(100, Ordering::Relaxed);
    ALLOC_TOTAL_CYCLES.store(5000, Ordering::Relaxed);
    ALLOC_MAX.store(2000, Ordering::Relaxed);

    let h = alloc_histogram();
    let p50 = h.percentile(50);
    let p99 = h.percentile(99);
    // p50 should be in bucket 0 (< 64 cycles).
    assert!(p50 <= 64, "p50 should be in bucket 0: got {}", p50);
    // p99 should be in bucket 5 (1024-2047 cycles).
    assert!(p99 > 64, "p99 should be above bucket 0: got {}", p99);
    serial_println!("[alloc_lat]   Percentiles: p50={}cyc, p99={}cyc: OK", p50, p99);

    // Test 7: Disable suppresses measurement.
    reset();
    disable();
    let s = begin();
    assert_eq!(s, 0, "begin should return 0 when disabled");
    end_alloc(s); // Should be no-op.
    let h = alloc_histogram();
    assert_eq!(h.count, 0);
    enable();
    serial_println!("[alloc_lat]   Disable/enable: OK");

    // Cleanup.
    reset();

    serial_println!("[alloc_lat] Self-test PASSED");
}

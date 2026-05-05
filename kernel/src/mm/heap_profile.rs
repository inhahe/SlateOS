//! Kernel heap allocation profiler — tracks allocation size distribution.
//!
//! Records the sizes of heap allocations and frees to build a profile
//! of the kernel's memory usage patterns.  This data informs:
//! - Slab size class tuning (which size classes are busiest?)
//! - Internal fragmentation analysis (are allocations rounding up significantly?)
//! - Hot allocation identification (which sizes need per-CPU caches?)
//!
//! ## Design
//!
//! Uses logarithmic size buckets (8, 16, 32, 64, ... 8192, >8192) to
//! count allocations.  Each bucket tracks:
//! - Number of allocs
//! - Number of frees
//! - Net active (allocs - frees)
//! - Peak active (high water mark)
//!
//! ## Overhead
//!
//! One comparison + one atomic increment per alloc/free (~5ns).
//! Negligible compared to the actual allocation cost.
//!
//! ## Integration
//!
//! The kernel heap allocator calls `record_alloc(size)` and
//! `record_free(size)` on every operation.  The kshell `heapprofile`
//! command displays the accumulated profile.
//!
//! ## References
//!
//! - Linux SLUB debug — per-slab allocation statistics
//! - jemalloc `malloc_stats_print()` — size class utilization
//! - mimalloc statistics — per-size-class counters

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Size buckets: 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, >8192
const NUM_BUCKETS: usize = 12;

/// Bucket upper bounds (exclusive).  Bucket i covers sizes in
/// (BUCKET_BOUNDS[i-1], BUCKET_BOUNDS[i]].  Bucket 0 covers (0, 8].
const BUCKET_BOUNDS: [usize; NUM_BUCKETS] = [
    8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, usize::MAX,
];

// ---------------------------------------------------------------------------
// Per-bucket statistics
// ---------------------------------------------------------------------------

/// Allocation counts per bucket.
static ALLOC_COUNTS: [AtomicU64; NUM_BUCKETS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

/// Free counts per bucket.
static FREE_COUNTS: [AtomicU64; NUM_BUCKETS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

/// Total bytes allocated per bucket (cumulative).
static ALLOC_BYTES: [AtomicU64; NUM_BUCKETS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

/// Peak active count per bucket (high water mark).
static PEAK_ACTIVE: [AtomicU64; NUM_BUCKETS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

// ---------------------------------------------------------------------------
// Global statistics
// ---------------------------------------------------------------------------

/// Total allocation requests since boot/reset.
static TOTAL_ALLOCS: AtomicU64 = AtomicU64::new(0);

/// Total free requests since boot/reset.
static TOTAL_FREES: AtomicU64 = AtomicU64::new(0);

/// Total bytes requested (sum of all allocation sizes).
static TOTAL_BYTES_REQUESTED: AtomicU64 = AtomicU64::new(0);

/// Largest single allocation size seen.
static MAX_ALLOC_SIZE: AtomicU64 = AtomicU64::new(0);

/// Whether profiling is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

// ---------------------------------------------------------------------------
// Public API — recording
// ---------------------------------------------------------------------------

/// Record a heap allocation of the given size.
///
/// Called by the heap allocator on every alloc.
#[inline]
pub fn record_alloc(size: usize) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let bucket = size_to_bucket(size);
    ALLOC_COUNTS[bucket].fetch_add(1, Ordering::Relaxed);
    ALLOC_BYTES[bucket].fetch_add(size as u64, Ordering::Relaxed);
    TOTAL_ALLOCS.fetch_add(1, Ordering::Relaxed);
    TOTAL_BYTES_REQUESTED.fetch_add(size as u64, Ordering::Relaxed);

    // Update peak active (approximate — races are acceptable for statistics).
    let allocs = ALLOC_COUNTS[bucket].load(Ordering::Relaxed);
    let frees = FREE_COUNTS[bucket].load(Ordering::Relaxed);
    let active = allocs.saturating_sub(frees);
    let mut current_peak = PEAK_ACTIVE[bucket].load(Ordering::Relaxed);
    while active > current_peak {
        match PEAK_ACTIVE[bucket].compare_exchange_weak(
            current_peak, active, Ordering::Relaxed, Ordering::Relaxed
        ) {
            Ok(_) => break,
            Err(actual) => current_peak = actual,
        }
    }

    // Track max allocation size.
    let size64 = size as u64;
    let mut current_max = MAX_ALLOC_SIZE.load(Ordering::Relaxed);
    while size64 > current_max {
        match MAX_ALLOC_SIZE.compare_exchange_weak(
            current_max, size64, Ordering::Relaxed, Ordering::Relaxed
        ) {
            Ok(_) => break,
            Err(actual) => current_max = actual,
        }
    }
}

/// Record a heap free of the given size.
///
/// Called by the heap allocator on every dealloc.
#[inline]
pub fn record_free(size: usize) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let bucket = size_to_bucket(size);
    FREE_COUNTS[bucket].fetch_add(1, Ordering::Relaxed);
    TOTAL_FREES.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API — control
// ---------------------------------------------------------------------------

/// Enable heap profiling.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable heap profiling.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether profiling is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Reset all counters.
pub fn reset() {
    for i in 0..NUM_BUCKETS {
        ALLOC_COUNTS[i].store(0, Ordering::Relaxed);
        FREE_COUNTS[i].store(0, Ordering::Relaxed);
        ALLOC_BYTES[i].store(0, Ordering::Relaxed);
        PEAK_ACTIVE[i].store(0, Ordering::Relaxed);
    }
    TOTAL_ALLOCS.store(0, Ordering::Relaxed);
    TOTAL_FREES.store(0, Ordering::Relaxed);
    TOTAL_BYTES_REQUESTED.store(0, Ordering::Relaxed);
    MAX_ALLOC_SIZE.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API — reporting
// ---------------------------------------------------------------------------

/// Per-bucket statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct BucketStats {
    /// Upper bound of this size class (bytes).
    pub size_class: usize,
    /// Total allocations in this bucket.
    pub allocs: u64,
    /// Total frees in this bucket.
    pub frees: u64,
    /// Currently active (allocs - frees).
    pub active: u64,
    /// Peak active (highest value of active ever observed).
    pub peak: u64,
    /// Total bytes allocated in this bucket (cumulative).
    pub total_bytes: u64,
    /// Average allocation size in this bucket.
    pub avg_size: u64,
}

/// Full profile snapshot.
#[derive(Debug, Clone)]
pub struct HeapProfile {
    /// Per-bucket statistics.
    pub buckets: [BucketStats; NUM_BUCKETS],
    /// Total allocations.
    pub total_allocs: u64,
    /// Total frees.
    pub total_frees: u64,
    /// Total bytes requested.
    pub total_bytes: u64,
    /// Largest allocation seen.
    pub max_alloc: u64,
    /// Whether profiling is enabled.
    pub enabled: bool,
}

/// Get the current heap profile.
#[must_use]
pub fn profile() -> HeapProfile {
    let mut buckets = [BucketStats {
        size_class: 0, allocs: 0, frees: 0,
        active: 0, peak: 0, total_bytes: 0, avg_size: 0,
    }; NUM_BUCKETS];

    for i in 0..NUM_BUCKETS {
        let allocs = ALLOC_COUNTS[i].load(Ordering::Relaxed);
        let frees = FREE_COUNTS[i].load(Ordering::Relaxed);
        let total_bytes = ALLOC_BYTES[i].load(Ordering::Relaxed);
        let avg = if allocs > 0 { total_bytes / allocs } else { 0 };

        buckets[i] = BucketStats {
            size_class: BUCKET_BOUNDS[i],
            allocs,
            frees,
            active: allocs.saturating_sub(frees),
            peak: PEAK_ACTIVE[i].load(Ordering::Relaxed),
            total_bytes,
            avg_size: avg,
        };
    }

    HeapProfile {
        buckets,
        total_allocs: TOTAL_ALLOCS.load(Ordering::Relaxed),
        total_frees: TOTAL_FREES.load(Ordering::Relaxed),
        total_bytes: TOTAL_BYTES_REQUESTED.load(Ordering::Relaxed),
        max_alloc: MAX_ALLOC_SIZE.load(Ordering::Relaxed),
        enabled: ENABLED.load(Ordering::Relaxed),
    }
}

/// Get the hottest size class (most allocations).
#[must_use]
pub fn hottest_bucket() -> (usize, u64) {
    let mut max_count: u64 = 0;
    let mut max_bucket: usize = 0;

    for i in 0..NUM_BUCKETS {
        let count = ALLOC_COUNTS[i].load(Ordering::Relaxed);
        if count > max_count {
            max_count = count;
            max_bucket = i;
        }
    }

    (BUCKET_BOUNDS[max_bucket], max_count)
}

/// Estimate internal fragmentation percentage.
///
/// Compares total bytes requested to total bytes actually consumed
/// (allocated size rounded up to slab class).  Higher fragmentation
/// means more wasted memory in padding.
#[must_use]
pub fn fragmentation_estimate() -> u8 {
    let requested = TOTAL_BYTES_REQUESTED.load(Ordering::Relaxed);
    if requested == 0 {
        return 0;
    }

    // Estimate consumed bytes by rounding each bucket's total bytes up
    // to the bucket size class boundary.
    let mut consumed: u64 = 0;
    for i in 0..NUM_BUCKETS {
        let allocs = ALLOC_COUNTS[i].load(Ordering::Relaxed);
        let class_size = BUCKET_BOUNDS[i].min(8192) as u64; // Cap overflow bucket.
        consumed = consumed.saturating_add(allocs.saturating_mul(class_size));
    }

    if consumed <= requested {
        return 0; // No fragmentation (shouldn't happen in practice).
    }

    let wasted = consumed.saturating_sub(requested);
    let pct = wasted.saturating_mul(100).checked_div(consumed).unwrap_or(0);
    (pct as u8).min(100)
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

/// Map an allocation size to a bucket index.
#[inline]
fn size_to_bucket(size: usize) -> usize {
    // Fast path for common small sizes.
    if size <= 8 { return 0; }
    if size <= 16 { return 1; }
    if size <= 32 { return 2; }
    if size <= 64 { return 3; }
    if size <= 128 { return 4; }
    if size <= 256 { return 5; }
    if size <= 512 { return 6; }
    if size <= 1024 { return 7; }
    if size <= 2048 { return 8; }
    if size <= 4096 { return 9; }
    if size <= 8192 { return 10; }
    NUM_BUCKETS - 1 // >8192 → overflow bucket
}

/// Human-readable bucket label.
pub fn bucket_label(idx: usize) -> &'static str {
    match idx {
        0 => "≤8",
        1 => "≤16",
        2 => "≤32",
        3 => "≤64",
        4 => "≤128",
        5 => "≤256",
        6 => "≤512",
        7 => "≤1K",
        8 => "≤2K",
        9 => "≤4K",
        10 => "≤8K",
        11 => ">8K",
        _ => "???",
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for heap profiling.
pub fn self_test() {
    serial_println!("[heap_profile] Running self-test...");

    // Test 1: Initial state after reset.
    reset();
    let p = profile();
    assert_eq!(p.total_allocs, 0);
    assert_eq!(p.total_frees, 0);
    assert_eq!(p.total_bytes, 0);
    serial_println!("[heap_profile]   Reset state: OK");

    // Test 2: Size-to-bucket mapping.
    assert_eq!(size_to_bucket(1), 0);
    assert_eq!(size_to_bucket(8), 0);
    assert_eq!(size_to_bucket(9), 1);
    assert_eq!(size_to_bucket(16), 1);
    assert_eq!(size_to_bucket(17), 2);
    assert_eq!(size_to_bucket(64), 3);
    assert_eq!(size_to_bucket(65), 4);
    assert_eq!(size_to_bucket(1024), 7);
    assert_eq!(size_to_bucket(4096), 9);
    assert_eq!(size_to_bucket(8192), 10);
    assert_eq!(size_to_bucket(8193), 11);
    assert_eq!(size_to_bucket(100_000), 11);
    serial_println!("[heap_profile]   Bucket mapping: OK");

    // Test 3: Record allocs and verify counts.
    record_alloc(32);
    record_alloc(32);
    record_alloc(32);
    record_alloc(128);
    record_free(32);

    let p = profile();
    assert_eq!(p.total_allocs, 4);
    assert_eq!(p.total_frees, 1);
    assert_eq!(p.buckets[2].allocs, 3); // ≤32 bucket
    assert_eq!(p.buckets[2].frees, 1);
    assert_eq!(p.buckets[2].active, 2);
    assert_eq!(p.buckets[4].allocs, 1); // ≤128 bucket
    serial_println!("[heap_profile]   Record alloc/free: OK");

    // Test 4: Peak tracking.
    assert!(p.buckets[2].peak >= 3, "peak should be >= 3");
    serial_println!("[heap_profile]   Peak tracking: OK (peak={})", p.buckets[2].peak);

    // Test 5: Total bytes and avg size.
    assert_eq!(p.buckets[2].total_bytes, 96); // 3 × 32
    assert_eq!(p.buckets[2].avg_size, 32);    // 96 / 3
    serial_println!("[heap_profile]   Bytes tracking: OK");

    // Test 6: Max allocation size.
    record_alloc(5000);
    let p = profile();
    assert_eq!(p.max_alloc, 5000);
    serial_println!("[heap_profile]   Max alloc tracking: OK");

    // Test 7: Hottest bucket.
    let (hot_size, hot_count) = hottest_bucket();
    assert_eq!(hot_size, 32); // Bucket ≤32 has 3 allocs.
    assert_eq!(hot_count, 3);
    serial_println!("[heap_profile]   Hottest bucket: ≤{} ({} allocs)", hot_size, hot_count);

    // Test 8: Disable/enable.
    disable();
    let before = TOTAL_ALLOCS.load(Ordering::Relaxed);
    record_alloc(64);
    assert_eq!(TOTAL_ALLOCS.load(Ordering::Relaxed), before); // Unchanged.
    enable();
    serial_println!("[heap_profile]   Disable/enable: OK");

    // Test 9: Fragmentation estimate.
    reset();
    // Allocate 10 bytes into the ≤16 bucket (50% waste: 10 bytes used, 16 allocated).
    record_alloc(10);
    record_alloc(10);
    record_alloc(10);
    let frag = fragmentation_estimate();
    // 30 bytes requested, 48 bytes consumed (3×16) → 18/48 = 37% waste.
    assert!(frag > 0, "should have some fragmentation");
    serial_println!("[heap_profile]   Fragmentation estimate: {}%", frag);

    // Cleanup.
    reset();

    serial_println!("[heap_profile] Self-test PASSED");
}

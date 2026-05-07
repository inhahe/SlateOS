//! Kernel memory type accounting.
//!
//! Tracks how physical memory is distributed across usage categories:
//! page tables, kernel stacks, slab heaps, DMA buffers, user pages, etc.
//! Provides the data for a `/proc/meminfo`-style breakdown showing where
//! all physical memory went.
//!
//! ## Design
//!
//! Each category has a global atomic counter (in frames).  Allocation
//! sites call [`charge`] when acquiring frames and [`uncharge`] when
//! releasing them.  The counters are advisory — racing updates may
//! produce slightly stale reads, but the totals are consistent over
//! time (every charge has a matching uncharge on the free path).
//!
//! ## Categories
//!
//! | Category | What it tracks |
//! |----------|----------------|
//! | PageTable | Frames used for PML4/PDPT/PD/PT page tables |
//! | KernelStack | Task kernel-mode stacks |
//! | SlabHeap | Slab allocator backing frames |
//! | LargeHeap | Large (buddy-backed) heap allocations |
//! | DmaBuf | DMA-coherent buffers |
//! | UserAnon | Anonymous user pages (demand-paged, CoW) |
//! | UserMapped | User file-backed or shared memory pages |
//! | ZeroPool | Pre-zeroed frame pool |
//! | Metadata | Allocator metadata (bitmap, refcount arrays) |
//! | Swap | Pages backing the zram compressed store |
//! | Other | Uncategorized allocations |
//!
//! ## Usage
//!
//! ```ignore
//! use crate::mm::memtype::{MemType, charge, uncharge};
//!
//! let frame = alloc_frame()?;
//! charge(MemType::PageTable, 1);
//! // ... use frame for a page table ...
//! free_frame(frame);
//! uncharge(MemType::PageTable, 1);
//! ```

use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Memory type categories
// ---------------------------------------------------------------------------

/// Categories of physical memory usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MemType {
    /// PML4, PDPT, PD, and PT page table frames.
    PageTable = 0,
    /// Kernel-mode task stacks (allocated via kstack module).
    KernelStack = 1,
    /// Slab allocator backing frames (small allocations).
    SlabHeap = 2,
    /// Large heap allocations (multi-frame, buddy-backed).
    LargeHeap = 3,
    /// DMA-coherent buffers.
    DmaBuf = 4,
    /// Anonymous user pages (demand paging, CoW, stack growth).
    UserAnon = 5,
    /// User file-backed or shared memory pages.
    UserMapped = 6,
    /// Pre-zeroed frame pool.
    ZeroPool = 7,
    /// Allocator metadata (frame bitmap, refcount arrays).
    Metadata = 8,
    /// Swap/zram compressed backing store.
    Swap = 9,
    /// Uncategorized.
    Other = 10,
}

/// Total number of memory type categories.
const NUM_TYPES: usize = 11;

/// Human-readable names for each category (indexed by enum discriminant).
const TYPE_NAMES: [&str; NUM_TYPES] = [
    "PageTable",
    "KernelStack",
    "SlabHeap",
    "LargeHeap",
    "DmaBuf",
    "UserAnon",
    "UserMapped",
    "ZeroPool",
    "Metadata",
    "Swap",
    "Other",
];

// ---------------------------------------------------------------------------
// Counters
// ---------------------------------------------------------------------------

/// Per-type frame counts.  Index = MemType discriminant.
static COUNTERS: [AtomicU64; NUM_TYPES] = [
    AtomicU64::new(0), // PageTable
    AtomicU64::new(0), // KernelStack
    AtomicU64::new(0), // SlabHeap
    AtomicU64::new(0), // LargeHeap
    AtomicU64::new(0), // DmaBuf
    AtomicU64::new(0), // UserAnon
    AtomicU64::new(0), // UserMapped
    AtomicU64::new(0), // ZeroPool
    AtomicU64::new(0), // Metadata
    AtomicU64::new(0), // Swap
    AtomicU64::new(0), // Other
];

/// Peak (high-water mark) per-type frame counts.
static PEAKS: [AtomicU64; NUM_TYPES] = [
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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Charge `count` frames to the given memory type.
///
/// Call when frames are allocated for a specific purpose.
#[inline]
pub fn charge(typ: MemType, count: u64) {
    let idx = typ as usize;
    if idx >= NUM_TYPES {
        return;
    }
    let new_val = COUNTERS[idx].fetch_add(count, Ordering::Relaxed)
        .saturating_add(count);
    // Update peak via CAS loop.
    loop {
        let peak = PEAKS[idx].load(Ordering::Relaxed);
        if new_val <= peak {
            break;
        }
        if PEAKS[idx].compare_exchange_weak(
            peak, new_val, Ordering::Relaxed, Ordering::Relaxed
        ).is_ok() {
            break;
        }
    }
}

/// Uncharge `count` frames from the given memory type.
///
/// Call when frames are freed/returned.
#[inline]
pub fn uncharge(typ: MemType, count: u64) {
    let idx = typ as usize;
    if idx >= NUM_TYPES {
        return;
    }
    // Saturating subtract to prevent underflow from mismatched charge/uncharge.
    let old = COUNTERS[idx].fetch_sub(count, Ordering::Relaxed);
    if old < count {
        // Underflow — fix up by setting to 0.
        COUNTERS[idx].store(0, Ordering::Relaxed);
    }
}

/// A snapshot of all memory type counters.
#[derive(Debug, Clone, Copy)]
pub struct MemTypeStats {
    /// Current frame count per type.
    pub current: [u64; NUM_TYPES],
    /// Peak frame count per type (high-water mark).
    pub peak: [u64; NUM_TYPES],
}

/// Read a snapshot of all memory type counters.
#[must_use]
pub fn stats() -> MemTypeStats {
    let mut s = MemTypeStats {
        current: [0; NUM_TYPES],
        peak: [0; NUM_TYPES],
    };
    for i in 0..NUM_TYPES {
        s.current[i] = COUNTERS[i].load(Ordering::Relaxed);
        s.peak[i] = PEAKS[i].load(Ordering::Relaxed);
    }
    s
}

/// Get the current frame count for a specific type.
#[must_use]
#[inline]
pub fn current(typ: MemType) -> u64 {
    let idx = typ as usize;
    if idx >= NUM_TYPES { return 0; }
    COUNTERS[idx].load(Ordering::Relaxed)
}

/// Get the human-readable name of a memory type.
#[must_use]
pub fn type_name(typ: MemType) -> &'static str {
    let idx = typ as usize;
    if idx >= NUM_TYPES { return "Unknown"; }
    TYPE_NAMES[idx]
}

/// Get all type names (for iteration).
#[must_use]
pub fn all_type_names() -> &'static [&'static str; NUM_TYPES] {
    &TYPE_NAMES
}

/// Total accounted frames across all types.
#[must_use]
pub fn total_accounted() -> u64 {
    let mut total = 0u64;
    for counter in &COUNTERS {
        total = total.saturating_add(counter.load(Ordering::Relaxed));
    }
    total
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for memory type accounting.
pub fn self_test() {
    use crate::serial_println;
    serial_println!("[memtype] Running self-test...");

    // Test 1: Charge increases counter and updates peak.
    let before = current(MemType::Other);
    charge(MemType::Other, 10);
    let after = current(MemType::Other);
    assert_eq!(after, before + 10, "charge should increase counter");
    serial_println!("[memtype]   Charge: OK");

    // Test 2: Uncharge decreases counter.
    uncharge(MemType::Other, 5);
    let now = current(MemType::Other);
    assert_eq!(now, before + 5, "uncharge should decrease counter");
    serial_println!("[memtype]   Uncharge: OK");

    // Test 3: Peak is at least as high as current after charge.
    let st = stats();
    let idx = MemType::Other as usize;
    assert!(st.peak[idx] >= st.current[idx],
        "peak should be >= current");
    serial_println!("[memtype]   Peak tracking: OK");

    // Test 4: Uncharge below zero saturates to 0 (no underflow).
    uncharge(MemType::Other, 5); // Back to `before`.
    let extra_uncharge = before + 100;
    uncharge(MemType::Other, extra_uncharge);
    let underflow = current(MemType::Other);
    assert_eq!(underflow, 0, "underflow should saturate to 0");
    // Restore approximate original value.
    charge(MemType::Other, before);
    serial_println!("[memtype]   Underflow saturation: OK");

    // Test 5: type_name returns valid names.
    assert_eq!(type_name(MemType::PageTable), "PageTable");
    assert_eq!(type_name(MemType::Swap), "Swap");
    serial_println!("[memtype]   Type names: OK");

    // Test 6: total_accounted is consistent.
    let total = total_accounted();
    let st = stats();
    let sum: u64 = st.current.iter().sum();
    assert_eq!(total, sum, "total should equal sum of all types");
    serial_println!("[memtype]   Total consistency: OK");

    // Test 7: all_type_names has correct count.
    assert_eq!(all_type_names().len(), 11);
    serial_println!("[memtype]   all_type_names: OK");

    serial_println!("[memtype] Self-test PASSED");
}

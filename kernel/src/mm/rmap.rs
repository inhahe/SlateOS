//! Reverse mapping (rmap) — find all PTEs mapping a given physical frame.
//!
//! The reverse map tracks which virtual addresses (across which address
//! spaces) are mapped to each physical frame.  This enables:
//!
//! - **Memory compaction**: migrate a page by updating all PTEs that point
//!   to it, then copying data to a new frame.
//! - **Transparent huge pages**: check if all 128 consecutive frames are
//!   mapped consecutively in the same address space.
//! - **KSM (Kernel Same-page Merging)**: when merging identical pages,
//!   remap all PTEs to a single shared frame.
//! - **Efficient eviction**: when swapping out a frame, invalidate all
//!   PTEs pointing to it without scanning every page table.
//!
//! ## Design
//!
//! Each physical frame that has user-mode mappings gets an entry in the
//! rmap.  The entry is a small list of (address_space, virtual_address)
//! pairs — the "mappers" of that frame.
//!
//! For most pages, there is exactly one mapper (private pages).  CoW
//! (copy-on-write) pages may have multiple mappers temporarily (after
//! fork, before the first write triggers a copy).
//!
//! ## Data Structure
//!
//! We use a global hash map keyed by physical frame address.  Each entry
//! stores up to `MAX_MAPPERS` inline (pml4_phys, virt_addr) pairs before
//! overflowing.  This avoids heap allocation for the common single-mapper
//! case.
//!
//! ## Thread Safety
//!
//! The rmap is protected by a global spinlock.  Rmap operations are not
//! on the hottest path (they happen on map/unmap, not on every memory
//! access), so a global lock is acceptable initially.  If it becomes a
//! bottleneck, we can shard by frame address.
//!
//! ## References
//!
//! - Linux `mm/rmap.c` — reverse mapping implementation
//! - Linux `include/linux/rmap.h` — `page_vma_mapped_walk()`
//! - Rik van Riel, "Object-based Reverse Mapping" (2004 OLS)

use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of rmap entries (physical frames tracked).
/// Each frame with at least one user mapping gets an entry.
/// 16384 entries covers 256 MiB of mapped user memory at 16 KiB/frame.
const RMAP_TABLE_SIZE: usize = 16384;

/// Maximum number of mappers per frame stored inline.
/// Most pages have exactly 1 mapper (private).  CoW pages briefly have 2+.
/// If a page exceeds this, additional mappers are silently dropped from
/// tracking (the rmap becomes incomplete for that frame, but no crash).
const MAX_MAPPERS: usize = 4;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single mapping: identifies which address space maps this frame and where.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Mapping {
    /// Physical address of the PML4 table (identifies the address space).
    pml4_phys: u64,
    /// Virtual address where this frame is mapped (16 KiB aligned).
    virt_addr: u64,
}

impl Mapping {
    const EMPTY: Self = Self { pml4_phys: 0, virt_addr: 0 };

    fn is_empty(self) -> bool {
        self.pml4_phys == 0 && self.virt_addr == 0
    }
}

/// Rmap entry for a single physical frame.
#[derive(Clone, Copy)]
struct RmapEntry {
    /// Physical address of the frame being tracked (0 = unused slot).
    frame_phys: u64,
    /// Inline array of mappers.
    mappers: [Mapping; MAX_MAPPERS],
    /// Number of active mappers (may exceed MAX_MAPPERS if overflow occurred).
    mapper_count: u16,
    /// Whether we've lost track of some mappers due to overflow.
    overflow: bool,
}

impl RmapEntry {
    const fn empty() -> Self {
        Self {
            frame_phys: 0,
            mappers: [Mapping::EMPTY; MAX_MAPPERS],
            mapper_count: 0,
            overflow: false,
        }
    }

    fn is_free(&self) -> bool {
        self.frame_phys == 0
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// The rmap table: hash-indexed by frame physical address.
struct RmapTable {
    entries: [RmapEntry; RMAP_TABLE_SIZE],
}

impl RmapTable {
    const fn new() -> Self {
        Self {
            entries: [RmapEntry::empty(); RMAP_TABLE_SIZE],
        }
    }

    /// Hash a physical frame address to a table index.
    /// Simple multiplicative hash — frame addresses are 16 KiB aligned so
    /// we shift right by 14 first, then multiply by a prime.
    #[allow(clippy::arithmetic_side_effects)]
    fn hash(frame_phys: u64) -> usize {
        let shifted = frame_phys >> 14; // Remove alignment bits.
        let h = shifted.wrapping_mul(0x517c_c1b7_2722_0a95); // Fibonacci hash.
        (h as usize) % RMAP_TABLE_SIZE
    }

    /// Find the entry for a frame, or a free slot to insert into.
    /// Uses linear probing with a maximum probe distance.
    fn find_or_free(&self, frame_phys: u64) -> Option<usize> {
        let start = Self::hash(frame_phys);
        // Probe up to 16 slots (linear probing with bounded distance).
        for i in 0..16 {
            let idx = (start + i) % RMAP_TABLE_SIZE;
            let entry = &self.entries[idx];
            if entry.frame_phys == frame_phys {
                return Some(idx); // Found existing entry.
            }
            if entry.is_free() {
                return Some(idx); // Found free slot.
            }
        }
        None // Table is too full in this region.
    }

    /// Find an existing entry for a frame (returns None if not tracked).
    fn find_existing(&self, frame_phys: u64) -> Option<usize> {
        let start = Self::hash(frame_phys);
        for i in 0..16 {
            let idx = (start + i) % RMAP_TABLE_SIZE;
            let entry = &self.entries[idx];
            if entry.frame_phys == frame_phys {
                return Some(idx);
            }
            if entry.is_free() {
                return None; // Empty slot means frame is not tracked.
            }
        }
        None
    }
}

static TABLE: Mutex<RmapTable> = Mutex::new(RmapTable::new());

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static ADD_COUNT: AtomicU64 = AtomicU64::new(0);
static REMOVE_COUNT: AtomicU64 = AtomicU64::new(0);
static LOOKUP_COUNT: AtomicU64 = AtomicU64::new(0);
static OVERFLOW_COUNT: AtomicU64 = AtomicU64::new(0);
static TABLE_FULL_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record that `frame_phys` is mapped at `virt_addr` in address space `pml4_phys`.
///
/// Call this after successfully mapping a frame into a user address space.
/// Kernel-only mappings (HHDM, kstack, vmalloc) do not need rmap tracking
/// because they are never migrated or evicted.
#[allow(clippy::arithmetic_side_effects)]
pub fn add(frame_phys: u64, pml4_phys: u64, virt_addr: u64) {
    let mapping = Mapping { pml4_phys, virt_addr };
    let mut table = TABLE.lock();

    if let Some(idx) = table.find_or_free(frame_phys) {
        let entry = &mut table.entries[idx];

        if entry.is_free() {
            // New entry for this frame.
            entry.frame_phys = frame_phys;
            entry.mappers[0] = mapping;
            entry.mapper_count = 1;
            entry.overflow = false;
        } else {
            // Existing entry — add a mapper.
            // Check for duplicate first.
            for i in 0..MAX_MAPPERS {
                if entry.mappers[i] == mapping {
                    // Already tracked, nothing to do.
                    ADD_COUNT.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
            // Find a free slot.
            if let Some(slot) = entry.mappers.iter().position(|m| m.is_empty()) {
                entry.mappers[slot] = mapping;
            } else {
                // All slots full — overflow.
                entry.overflow = true;
                OVERFLOW_COUNT.fetch_add(1, Ordering::Relaxed);
            }
            entry.mapper_count = entry.mapper_count.saturating_add(1);
        }
    } else {
        // Hash table region is full — can't track this frame.
        TABLE_FULL_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    ADD_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Remove the mapping record for `frame_phys` at `virt_addr` in `pml4_phys`.
///
/// Call this when unmapping a frame from a user address space.
/// If the frame has no remaining mappers, the entry is freed.
#[allow(clippy::arithmetic_side_effects)]
pub fn remove(frame_phys: u64, pml4_phys: u64, virt_addr: u64) {
    let mapping = Mapping { pml4_phys, virt_addr };
    let mut table = TABLE.lock();

    if let Some(idx) = table.find_existing(frame_phys) {
        let entry = &mut table.entries[idx];

        // Remove the mapping from the inline array.
        for i in 0..MAX_MAPPERS {
            if entry.mappers[i] == mapping {
                entry.mappers[i] = Mapping::EMPTY;
                break;
            }
        }

        entry.mapper_count = entry.mapper_count.saturating_sub(1);

        // If no mappers remain, free the entry.
        if entry.mapper_count == 0 {
            *entry = RmapEntry::empty();
        }
    }

    REMOVE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Look up all known mappers of a physical frame.
///
/// Returns the number of mappers found and fills `out` with up to
/// `out.len()` (pml4_phys, virt_addr) pairs.
///
/// If the frame has overflowed its inline storage, the returned list
/// may be incomplete (caller should check `is_complete`).
#[allow(clippy::arithmetic_side_effects)]
pub fn lookup(frame_phys: u64, out: &mut [(u64, u64)]) -> RmapLookup {
    let table = TABLE.lock();
    LOOKUP_COUNT.fetch_add(1, Ordering::Relaxed);

    if let Some(idx) = table.find_existing(frame_phys) {
        let entry = &table.entries[idx];
        let mut found = 0;

        for i in 0..MAX_MAPPERS {
            if !entry.mappers[i].is_empty() && found < out.len() {
                out[found] = (entry.mappers[i].pml4_phys, entry.mappers[i].virt_addr);
                found += 1;
            }
        }

        RmapLookup {
            count: entry.mapper_count as usize,
            filled: found,
            is_complete: !entry.overflow,
        }
    } else {
        RmapLookup {
            count: 0,
            filled: 0,
            is_complete: true,
        }
    }
}

/// Result of an rmap lookup.
#[derive(Debug, Clone, Copy)]
pub struct RmapLookup {
    /// Total number of known mappers (may exceed `filled` if out buffer was too small).
    pub count: usize,
    /// Number of mappers written to the output buffer.
    pub filled: usize,
    /// Whether the mapper list is known to be complete.
    /// `false` means overflow occurred and some mappers were lost.
    pub is_complete: bool,
}

/// Check how many address spaces map a given frame.
///
/// Returns 0 if the frame is not tracked in the rmap.
/// Faster than `lookup()` when you only need the count.
pub fn mapper_count(frame_phys: u64) -> usize {
    let table = TABLE.lock();
    if let Some(idx) = table.find_existing(frame_phys) {
        table.entries[idx].mapper_count as usize
    } else {
        0
    }
}

/// Check if a frame is mapped by exactly one address space (private page).
///
/// Private pages can be migrated freely during compaction (only one PTE
/// to update).  Shared pages (mapper_count > 1) require CoW-aware handling.
pub fn is_private(frame_phys: u64) -> bool {
    mapper_count(frame_phys) == 1
}

/// Collect a batch of privately-mapped frame addresses for compaction.
///
/// Scans the rmap table and returns up to `max` frame addresses that have
/// exactly one mapper (private pages).  These are candidates for migration
/// during memory compaction — they only need one PTE update.
///
/// The scan starts from index `start_idx` (modulo table size) and wraps
/// around.  Returns the next start index for continuation.
///
/// Shared pages (mapper_count > 1) and overflowed entries are skipped.
pub fn collect_private_frames(out: &mut [u64], start_idx: usize) -> (usize, usize) {
    let table = TABLE.lock();
    let mut found = 0;
    let start = start_idx % RMAP_TABLE_SIZE;

    for i in 0..RMAP_TABLE_SIZE {
        if found >= out.len() {
            // Return next index for continuation.
            return (found, (start + i) % RMAP_TABLE_SIZE);
        }

        let idx = (start + i) % RMAP_TABLE_SIZE;
        let entry = &table.entries[idx];

        // Only collect privately-mapped, non-overflow frames.
        if !entry.is_free() && entry.mapper_count == 1 && !entry.overflow {
            out[found] = entry.frame_phys;
            found += 1;
        }
    }

    // Full scan completed — wrap to 0.
    (found, 0)
}

/// Statistics for the rmap subsystem.
#[derive(Debug, Clone, Copy)]
pub struct RmapStats {
    /// Total `add()` calls since boot.
    pub add_count: u64,
    /// Total `remove()` calls since boot.
    pub remove_count: u64,
    /// Total `lookup()` calls since boot.
    pub lookup_count: u64,
    /// Number of frames that exceeded inline mapper capacity.
    pub overflow_count: u64,
    /// Number of add() calls that failed due to table region being full.
    pub table_full_count: u64,
    /// Number of entries currently in use.
    pub entries_used: usize,
    /// Total table capacity.
    pub table_capacity: usize,
}

/// Get rmap statistics.
#[must_use]
pub fn stats() -> RmapStats {
    let table = TABLE.lock();
    let used = table.entries.iter().filter(|e| !e.is_free()).count();

    RmapStats {
        add_count: ADD_COUNT.load(Ordering::Relaxed),
        remove_count: REMOVE_COUNT.load(Ordering::Relaxed),
        lookup_count: LOOKUP_COUNT.load(Ordering::Relaxed),
        overflow_count: OVERFLOW_COUNT.load(Ordering::Relaxed),
        table_full_count: TABLE_FULL_COUNT.load(Ordering::Relaxed),
        entries_used: used,
        table_capacity: RMAP_TABLE_SIZE,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the rmap subsystem.
pub fn self_test() {
    serial_println!("[rmap] Running self-test...");

    // Use fake frame addresses for testing (not real frames/PML4s).
    //
    // CRITICAL: the rmap is a GLOBAL table keyed by physical frame address,
    // and this self-test runs late in boot — *after* heavy CoW/fork activity
    // (the Path-Z ring-3 toolchain tests) has populated the table with many
    // real frames.  If the test reused a low address like 1 MiB / 2 MiB, a
    // *real* user frame at that exact physical address could already have a
    // mapper registered, so `add(frame, ...)` would append a *second* mapper
    // and `is_private(frame)` would be false → spurious assertion panic
    // (observed: `assertion failed: is_private(frame2)` at this line).  To stay
    // collision-proof regardless of allocation timing, the test frames must sit
    // far above any installed physical RAM (machines here have at most a few
    // GiB), so the global table can never hold a pre-existing entry for them.
    // 15 TiB is comfortably beyond any real frame while remaining a valid u64
    // hash key (the rmap does not validate the physical-address width).
    let frame1: u64 = 0x0000_0F00_0000_0000; // ~15 TiB, impossible as real RAM
    let frame2: u64 = 0x0000_0F00_0000_4000; // frame1 + 16 KiB
    let pml4_a: u64 = 0x00A0_0000;
    let pml4_b: u64 = 0x00B0_0000;
    let virt1: u64 = 0x0000_4000_0000_0000; // User space
    let virt2: u64 = 0x0000_4000_0000_4000; // Next page

    // Test 1: Add and lookup.
    add(frame1, pml4_a, virt1);
    let mut buf = [(0u64, 0u64); 4];
    let result = lookup(frame1, &mut buf);
    assert_eq!(result.count, 1);
    assert_eq!(result.filled, 1);
    assert!(result.is_complete);
    assert_eq!(buf[0], (pml4_a, virt1));
    serial_println!("[rmap]   Add + lookup single mapper: OK");

    // Test 2: Multiple mappers (CoW scenario).
    add(frame1, pml4_b, virt2);
    let result = lookup(frame1, &mut buf);
    assert_eq!(result.count, 2);
    assert_eq!(result.filled, 2);
    serial_println!("[rmap]   Multiple mappers (CoW): OK (count={})", result.count);

    // Test 3: is_private / mapper_count.
    assert!(!is_private(frame1)); // Two mappers.
    add(frame2, pml4_a, virt2);
    assert!(is_private(frame2)); // One mapper.
    serial_println!("[rmap]   is_private check: OK");

    // Test 4: Remove.
    remove(frame1, pml4_b, virt2);
    let result = lookup(frame1, &mut buf);
    assert_eq!(result.count, 1);
    assert!(is_private(frame1));
    serial_println!("[rmap]   Remove mapper: OK");

    // Test 5: Remove last mapper (entry freed).
    remove(frame1, pml4_a, virt1);
    let result = lookup(frame1, &mut buf);
    assert_eq!(result.count, 0);
    assert_eq!(result.filled, 0);
    serial_println!("[rmap]   Remove last mapper (entry freed): OK");

    // Test 6: Frame not tracked.  Use another impossible-as-real address (see
    // the frame1/frame2 note above) so a real frame can never make this lookup
    // return a nonzero count.
    let result = lookup(0x0000_0F00_0001_0000, &mut buf);
    assert_eq!(result.count, 0);
    assert!(result.is_complete);
    serial_println!("[rmap]   Lookup untracked frame: OK");

    // Cleanup test frame2.
    remove(frame2, pml4_a, virt2);

    // Test 7: Duplicate add is idempotent.
    add(frame1, pml4_a, virt1);
    add(frame1, pml4_a, virt1); // Same mapping again.
    let result = lookup(frame1, &mut buf);
    assert_eq!(result.count, 1); // Still just one.
    remove(frame1, pml4_a, virt1);
    serial_println!("[rmap]   Duplicate add idempotent: OK");

    // Test 8: Stats.
    let st = stats();
    assert!(st.add_count > 0);
    assert!(st.remove_count > 0);
    assert!(st.lookup_count > 0);
    serial_println!("[rmap]   Stats: OK (adds={}, removes={}, lookups={})",
        st.add_count, st.remove_count, st.lookup_count);

    serial_println!("[rmap] Self-test PASSED");
}

//! Page migration type classification — anti-fragmentation for compaction.
//!
//! Physical frames are classified by how they can be reclaimed or moved:
//!
//! - **Unmovable**: Kernel slab objects, page table pages, DMA buffers.
//!   Cannot be moved or reclaimed. If these scatter through the address
//!   space, they fragment the movable pool permanently.
//! - **Movable**: User pages, file-backed pages.  Can be relocated via
//!   the rmap + page migration infrastructure.
//! - **Reclaimable**: File cache, dentries, inode cache.  Can be evicted
//!   (freed) to recover memory without moving them.
//! - **HighAtomic**: Small reserve of emergency pages for atomic (IRQ)
//!   allocations that cannot sleep.
//!
//! ## Why This Matters
//!
//! Without migration types, kernel allocations (unmovable) scatter through
//! physical memory.  Even with lots of free memory, high-order buddy
//! allocations fail because unmovable pages prevent coalescing.
//!
//! By grouping allocations by migration type, movable pages cluster together
//! in "pageblocks" that can be fully evacuated during compaction, allowing
//! the freed region to be coalesced into high-order blocks.
//!
//! ## Design
//!
//! We store a 2-bit migration type per frame in a compact bitmap (4 frames
//! per byte).  For 64K frames (1 GiB @ 16 KiB pages), this costs only
//! 16 KiB of memory.
//!
//! The migration type is set at allocation time by the caller (who knows
//! whether the allocation is for kernel metadata vs user page vs cache).
//! The compaction system queries it to find movable candidates.
//!
//! ## Pageblocks
//!
//! Like Linux, we group frames into "pageblocks" — contiguous regions
//! whose migration type is set as a unit.  Our pageblock size is 64 frames
//! (1 MiB at 16 KiB pages).  When a pageblock's type is Movable, the
//! compactor knows that all allocations in it should be relocatable.
//!
//! ## References
//!
//! - Linux `include/linux/mmzone.h` — MIGRATE_TYPES enum
//! - Linux `mm/page_alloc.c` — per-pageblock migration type tracking
//! - Mel Gorman, "Understanding the Linux Virtual Memory Manager" §6

use core::sync::atomic::{AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Migration type enum
// ---------------------------------------------------------------------------

/// Classification of a physical frame's movability.
///
/// Stored as 2 bits per frame, so limited to 4 variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MigrateType {
    /// Cannot be moved or reclaimed.  Kernel slab, page tables, DMA.
    Unmovable = 0,
    /// Can be relocated via page migration (rmap + copy + PTE update).
    Movable = 1,
    /// Can be evicted/freed (file cache, dentry cache).
    Reclaimable = 2,
    /// Emergency reserve for atomic/IRQ allocations.
    HighAtomic = 3,
}

impl MigrateType {
    /// Convert from raw 2-bit value.
    #[inline]
    #[must_use]
    const fn from_raw(val: u8) -> Self {
        match val & 0x03 {
            0 => Self::Unmovable,
            1 => Self::Movable,
            2 => Self::Reclaimable,
            _ => Self::HighAtomic,
        }
    }

    /// Human-readable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unmovable => "unmovable",
            Self::Movable => "movable",
            Self::Reclaimable => "reclaimable",
            Self::HighAtomic => "highatomic",
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of frames we track (1 GiB / 16 KiB = 65536 frames).
/// Can be increased for systems with more memory.
const MAX_FRAMES: usize = 65536;

/// Bytes needed: 2 bits per frame, 4 frames per byte.
const BITMAP_BYTES: usize = MAX_FRAMES / 4;

/// Number of frames in a pageblock (migration type unit).
/// 64 frames = 1 MiB at 16 KiB pages.
pub const PAGEBLOCK_FRAMES: usize = 64;

/// Number of pageblocks we can track.
const MAX_PAGEBLOCKS: usize = MAX_FRAMES / PAGEBLOCK_FRAMES;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Per-frame migration type bitmap (2 bits per frame, packed 4 per byte).
///
/// Index = frame_number / 4, shift = (frame_number % 4) * 2.
static mut FRAME_TYPES: [u8; BITMAP_BYTES] = [0; BITMAP_BYTES];

/// Per-pageblock default migration type.
///
/// When a pageblock is first allocated from, its type is set based on
/// the allocation's migration type.  Compaction uses this to identify
/// regions worth evacuating.
static mut PAGEBLOCK_TYPES: [u8; MAX_PAGEBLOCKS] = [0; MAX_PAGEBLOCKS];

/// Whether the migration type system has been initialized.
static mut INITIALIZED: bool = false;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Frames allocated per migration type.
static ALLOC_UNMOVABLE: AtomicU64 = AtomicU64::new(0);
static ALLOC_MOVABLE: AtomicU64 = AtomicU64::new(0);
static ALLOC_RECLAIMABLE: AtomicU64 = AtomicU64::new(0);
static ALLOC_HIGHATOMIC: AtomicU64 = AtomicU64::new(0);

/// Frames freed per migration type.
static FREE_UNMOVABLE: AtomicU64 = AtomicU64::new(0);
static FREE_MOVABLE: AtomicU64 = AtomicU64::new(0);
static FREE_RECLAIMABLE: AtomicU64 = AtomicU64::new(0);
static FREE_HIGHATOMIC: AtomicU64 = AtomicU64::new(0);

/// Pageblock type changes (steal events).
static PAGEBLOCK_STEALS: AtomicU64 = AtomicU64::new(0);

/// Statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct MigrateStats {
    /// Frames currently allocated per type (alloc - free).
    pub current: [u64; 4],
    /// Total frames allocated per type since boot.
    pub alloc_total: [u64; 4],
    /// Total frames freed per type since boot.
    pub free_total: [u64; 4],
    /// Number of pageblock type steal events.
    pub pageblock_steals: u64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the migration type tracking system.
///
/// Call once during memory subsystem init, after the frame allocator is
/// ready.  All frames start as Unmovable (conservative default).
pub fn init() {
    // SAFETY: Called once during single-threaded boot.
    unsafe {
        // Default: all frames unmovable (0 = Unmovable).
        // Already zero-initialized by static, so nothing to do.
        INITIALIZED = true;
    }
    serial_println!("[migrate_type] Initialized: {} frames, {} pageblocks",
        MAX_FRAMES, MAX_PAGEBLOCKS);
}

/// Set the migration type for a specific frame.
///
/// Called by the allocator when handing out a frame, or by the caller
/// to classify their allocation.
///
/// `frame_idx` is the frame number (physical_address / FRAME_SIZE).
#[inline]
pub fn set_frame_type(frame_idx: usize, mtype: MigrateType) {
    if frame_idx >= MAX_FRAMES {
        return;
    }

    let byte_idx = frame_idx / 4;
    let bit_shift = (frame_idx % 4) * 2;
    let mask = !(0x03u8 << bit_shift);
    let value = (mtype as u8) << bit_shift;

    // SAFETY: Single-writer assumption for per-frame metadata during
    // allocation (frame is owned by caller, not concurrently accessed).
    unsafe {
        let byte = &mut FRAME_TYPES[byte_idx];
        *byte = (*byte & mask) | value;
    }

    // Update allocation counter.
    match mtype {
        MigrateType::Unmovable => ALLOC_UNMOVABLE.fetch_add(1, Ordering::Relaxed),
        MigrateType::Movable => ALLOC_MOVABLE.fetch_add(1, Ordering::Relaxed),
        MigrateType::Reclaimable => ALLOC_RECLAIMABLE.fetch_add(1, Ordering::Relaxed),
        MigrateType::HighAtomic => ALLOC_HIGHATOMIC.fetch_add(1, Ordering::Relaxed),
    };
}

/// Get the migration type of a specific frame.
#[inline]
#[must_use]
pub fn get_frame_type(frame_idx: usize) -> MigrateType {
    if frame_idx >= MAX_FRAMES {
        return MigrateType::Unmovable;
    }

    let byte_idx = frame_idx / 4;
    let bit_shift = (frame_idx % 4) * 2;

    // SAFETY: Read-only access to static array, frame_idx < MAX_FRAMES.
    let raw = unsafe { FRAME_TYPES[byte_idx] };
    MigrateType::from_raw((raw >> bit_shift) & 0x03)
}

/// Record that a frame has been freed (for statistics).
///
/// Does NOT clear the migration type — that's done on next allocation.
#[inline]
pub fn record_free(frame_idx: usize) {
    let mtype = get_frame_type(frame_idx);
    match mtype {
        MigrateType::Unmovable => FREE_UNMOVABLE.fetch_add(1, Ordering::Relaxed),
        MigrateType::Movable => FREE_MOVABLE.fetch_add(1, Ordering::Relaxed),
        MigrateType::Reclaimable => FREE_RECLAIMABLE.fetch_add(1, Ordering::Relaxed),
        MigrateType::HighAtomic => FREE_HIGHATOMIC.fetch_add(1, Ordering::Relaxed),
    };
}

/// Set the migration type for an entire pageblock.
///
/// Called when a pageblock's dominant allocation type changes (e.g.,
/// when the first allocation in a free pageblock determines its type).
pub fn set_pageblock_type(pageblock_idx: usize, mtype: MigrateType) {
    if pageblock_idx >= MAX_PAGEBLOCKS {
        return;
    }

    // SAFETY: pageblock_idx < MAX_PAGEBLOCKS.
    let old = unsafe { PAGEBLOCK_TYPES[pageblock_idx] };
    if old != mtype as u8 {
        PAGEBLOCK_STEALS.fetch_add(1, Ordering::Relaxed);
    }
    unsafe {
        PAGEBLOCK_TYPES[pageblock_idx] = mtype as u8;
    }
}

/// Get the migration type of a pageblock.
#[must_use]
pub fn get_pageblock_type(pageblock_idx: usize) -> MigrateType {
    if pageblock_idx >= MAX_PAGEBLOCKS {
        return MigrateType::Unmovable;
    }
    // SAFETY: pageblock_idx < MAX_PAGEBLOCKS.
    let raw = unsafe { PAGEBLOCK_TYPES[pageblock_idx] };
    MigrateType::from_raw(raw)
}

/// Get the pageblock index for a frame index.
#[inline]
#[must_use]
pub const fn frame_to_pageblock(frame_idx: usize) -> usize {
    frame_idx / PAGEBLOCK_FRAMES
}

/// Check if a frame is movable (eligible for compaction/migration).
#[inline]
#[must_use]
pub fn is_movable(frame_idx: usize) -> bool {
    get_frame_type(frame_idx) == MigrateType::Movable
}

/// Check if a frame is reclaimable (eligible for eviction).
#[inline]
#[must_use]
pub fn is_reclaimable(frame_idx: usize) -> bool {
    get_frame_type(frame_idx) == MigrateType::Reclaimable
}

/// Count frames of a given migration type in a pageblock.
///
/// Useful for compaction decisions: a pageblock with many movable frames
/// is a good evacuation candidate.
#[must_use]
pub fn count_in_pageblock(pageblock_idx: usize, mtype: MigrateType) -> usize {
    if pageblock_idx >= MAX_PAGEBLOCKS {
        return 0;
    }

    let start_frame = pageblock_idx * PAGEBLOCK_FRAMES;
    let end_frame = (start_frame + PAGEBLOCK_FRAMES).min(MAX_FRAMES);
    let mut count = 0;

    for idx in start_frame..end_frame {
        if get_frame_type(idx) == mtype {
            count += 1;
        }
    }
    count
}

/// Find the next pageblock of a given type starting from `start_pb`.
///
/// Returns `None` if no matching pageblock is found.
#[must_use]
pub fn find_pageblock(start_pb: usize, mtype: MigrateType) -> Option<usize> {
    for pb in start_pb..MAX_PAGEBLOCKS {
        if get_pageblock_type(pb) == mtype {
            return Some(pb);
        }
    }
    None
}

/// Get current migration type statistics.
#[must_use]
pub fn stats() -> MigrateStats {
    let alloc = [
        ALLOC_UNMOVABLE.load(Ordering::Relaxed),
        ALLOC_MOVABLE.load(Ordering::Relaxed),
        ALLOC_RECLAIMABLE.load(Ordering::Relaxed),
        ALLOC_HIGHATOMIC.load(Ordering::Relaxed),
    ];
    let free = [
        FREE_UNMOVABLE.load(Ordering::Relaxed),
        FREE_MOVABLE.load(Ordering::Relaxed),
        FREE_RECLAIMABLE.load(Ordering::Relaxed),
        FREE_HIGHATOMIC.load(Ordering::Relaxed),
    ];
    let current = [
        alloc[0].saturating_sub(free[0]),
        alloc[1].saturating_sub(free[1]),
        alloc[2].saturating_sub(free[2]),
        alloc[3].saturating_sub(free[3]),
    ];

    MigrateStats {
        current,
        alloc_total: alloc,
        free_total: free,
        pageblock_steals: PAGEBLOCK_STEALS.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the migration type system.
pub fn self_test() {
    serial_println!("[migrate_type] Running self-test...");

    // Test 1: Default type is Unmovable.
    assert_eq!(get_frame_type(0), MigrateType::Unmovable);
    assert_eq!(get_frame_type(100), MigrateType::Unmovable);
    serial_println!("[migrate_type]   Default type=Unmovable: OK");

    // Test 2: Set and get individual frame types.
    set_frame_type(10, MigrateType::Movable);
    assert_eq!(get_frame_type(10), MigrateType::Movable);

    set_frame_type(11, MigrateType::Reclaimable);
    assert_eq!(get_frame_type(11), MigrateType::Reclaimable);

    set_frame_type(12, MigrateType::HighAtomic);
    assert_eq!(get_frame_type(12), MigrateType::HighAtomic);

    // Verify adjacent frames aren't affected (2-bit packing).
    assert_eq!(get_frame_type(9), MigrateType::Unmovable);
    assert_eq!(get_frame_type(13), MigrateType::Unmovable);
    serial_println!("[migrate_type]   Set/get frame type: OK");

    // Test 3: Overwrite a frame type.
    set_frame_type(10, MigrateType::Unmovable);
    assert_eq!(get_frame_type(10), MigrateType::Unmovable);
    serial_println!("[migrate_type]   Overwrite frame type: OK");

    // Test 4: Pageblock type.
    set_pageblock_type(0, MigrateType::Movable);
    assert_eq!(get_pageblock_type(0), MigrateType::Movable);
    set_pageblock_type(1, MigrateType::Reclaimable);
    assert_eq!(get_pageblock_type(1), MigrateType::Reclaimable);
    serial_println!("[migrate_type]   Pageblock type: OK");

    // Test 5: Frame-to-pageblock mapping.
    assert_eq!(frame_to_pageblock(0), 0);
    assert_eq!(frame_to_pageblock(63), 0);
    assert_eq!(frame_to_pageblock(64), 1);
    assert_eq!(frame_to_pageblock(127), 1);
    serial_println!("[migrate_type]   Frame→pageblock mapping: OK");

    // Test 6: is_movable / is_reclaimable helpers.
    // Use pageblock 4 (frame 256+) to avoid interfering with Test 7's count.
    set_frame_type(260, MigrateType::Movable);
    assert!(is_movable(260));
    assert!(!is_reclaimable(260));

    set_frame_type(261, MigrateType::Reclaimable);
    assert!(is_reclaimable(261));
    assert!(!is_movable(261));
    serial_println!("[migrate_type]   is_movable/is_reclaimable: OK");

    // Test 7: count_in_pageblock.
    // Pageblock 3 starts at frame 192.
    set_frame_type(192, MigrateType::Movable);
    set_frame_type(193, MigrateType::Movable);
    set_frame_type(194, MigrateType::Movable);
    set_frame_type(195, MigrateType::Reclaimable);
    let movable_count = count_in_pageblock(3, MigrateType::Movable);
    assert_eq!(movable_count, 3);
    serial_println!("[migrate_type]   count_in_pageblock: OK ({})", movable_count);

    // Test 8: Out-of-range access is safe.
    assert_eq!(get_frame_type(MAX_FRAMES + 100), MigrateType::Unmovable);
    set_frame_type(MAX_FRAMES + 100, MigrateType::Movable); // No-op, no panic.
    serial_println!("[migrate_type]   Out-of-range safety: OK");

    // Test 9: Statistics.
    let s = stats();
    assert!(s.alloc_total[1] > 0, "should have movable allocations");
    serial_println!("[migrate_type]   Stats: unmov={} mov={} recl={} hatm={}",
        s.current[0], s.current[1], s.current[2], s.current[3]);

    // Cleanup: reset test frames back to Unmovable.
    for i in [10, 11, 12, 192, 193, 194, 195, 260, 261] {
        set_frame_type(i, MigrateType::Unmovable);
    }

    serial_println!("[migrate_type] Self-test PASSED");
}

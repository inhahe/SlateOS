//! Physical frame ownership tracker — "who allocated this frame?"
//!
//! Records the subsystem or call site that allocated each physical frame.
//! When memory usage is high and you need to understand *where* it went,
//! this module can answer: which subsystem owns the most frames?
//!
//! ## Design
//!
//! Each frame index (0..MAX_FRAMES) gets a compact 8-bit owner tag stored
//! in a flat array.  The tag identifies the subsystem that allocated the
//! frame (page tables, kernel stacks, DMA buffers, user pages, etc.).
//!
//! Tags are set at allocation time and cleared on free.  The overhead is
//! exactly 1 byte per frame (64 KiB for 65536 frames = 1 GiB physical RAM).
//!
//! ## Owner Tags
//!
//! Tags are defined in [`Owner`] and cover all major allocation sources.
//! Unknown or untracked allocations get `Owner::Unknown`.
//!
//! ## Querying
//!
//! - `get(frame_idx)` → which subsystem owns this frame
//! - `summary()` → per-tag frame counts
//! - `find_by_owner(tag)` → iterator over frame indices with that tag
//!
//! ## Integration
//!
//! The frame allocator calls `set()` after allocation and `clear()` on free.
//! Subsystems pass their owner tag to allocation functions, or the allocator
//! infers it from the call context.
//!
//! ## References
//!
//! - Linux `mm/page_owner.c` — per-page allocation tracking
//! - Linux `include/linux/page_owner.h` — page_owner API

use core::sync::atomic::{AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum tracked frames (matches frame allocator's MAX_FRAMES).
const MAX_FRAMES: usize = 65536;

// ---------------------------------------------------------------------------
// Owner tag
// ---------------------------------------------------------------------------

/// Identifies which subsystem allocated a frame.
///
/// Each variant corresponds to a major allocation source in the kernel.
/// 8-bit representation keeps the per-frame overhead to 1 byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Owner {
    /// Frame is free (not allocated).
    Free = 0,
    /// Unknown or untagged allocation.
    Unknown = 1,
    /// Kernel page table structures (PML4, PDPT, PD, PT pages).
    PageTable = 2,
    /// Kernel heap slab allocator backing frames.
    HeapSlab = 3,
    /// Kernel stack frames (per-task stacks with guard pages).
    KernelStack = 4,
    /// DMA buffer allocation (physically contiguous, device-accessible).
    Dma = 5,
    /// User-space anonymous pages (demand paging, mmap).
    UserAnon = 6,
    /// User-space file-backed pages (page cache).
    UserFile = 7,
    /// Copy-on-Write source/destination frames.
    Cow = 8,
    /// Shared memory regions (IPC).
    SharedMem = 9,
    /// VMA metadata or internal bookkeeping.
    VmaMeta = 10,
    /// vmalloc backing frames (virtually-contiguous kernel allocations).
    Vmalloc = 11,
    /// Memory pool (mempool) reserved frames.
    Mempool = 12,
    /// Swap cache (frames awaiting or completing swap I/O).
    SwapCache = 13,
    /// Zero-page pool (pre-zeroed frames for fast demand paging).
    ZeroPool = 14,
    /// Huge page allocation (2 MiB / 128 frames).
    HugePage = 15,
    /// Filesystem buffer cache backing.
    FsCache = 16,
    /// Network buffer frames (packet data).
    NetBuffer = 17,
    /// Crypto / RNG scratch buffers.
    Crypto = 18,
    /// Boot-time allocations (before subsystems are initialized).
    Boot = 19,
    /// Self-test / benchmark temporary allocations.
    SelfTest = 20,
    /// Frame used for ACPI tables or firmware data.
    Firmware = 21,
    /// Framebuffer / display memory.
    Framebuffer = 22,
    /// Compressed page backing (zswap/zram).
    Compressed = 23,
}

impl Owner {
    /// Convert from raw u8 tag.
    #[inline]
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Free,
            1 => Self::Unknown,
            2 => Self::PageTable,
            3 => Self::HeapSlab,
            4 => Self::KernelStack,
            5 => Self::Dma,
            6 => Self::UserAnon,
            7 => Self::UserFile,
            8 => Self::Cow,
            9 => Self::SharedMem,
            10 => Self::VmaMeta,
            11 => Self::Vmalloc,
            12 => Self::Mempool,
            13 => Self::SwapCache,
            14 => Self::ZeroPool,
            15 => Self::HugePage,
            16 => Self::FsCache,
            17 => Self::NetBuffer,
            18 => Self::Crypto,
            19 => Self::Boot,
            20 => Self::SelfTest,
            21 => Self::Firmware,
            22 => Self::Framebuffer,
            23 => Self::Compressed,
            _ => Self::Unknown,
        }
    }

    /// Human-readable name for display.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Unknown => "unknown",
            Self::PageTable => "page_table",
            Self::HeapSlab => "heap_slab",
            Self::KernelStack => "kstack",
            Self::Dma => "dma",
            Self::UserAnon => "user_anon",
            Self::UserFile => "user_file",
            Self::Cow => "cow",
            Self::SharedMem => "shm",
            Self::VmaMeta => "vma_meta",
            Self::Vmalloc => "vmalloc",
            Self::Mempool => "mempool",
            Self::SwapCache => "swap_cache",
            Self::ZeroPool => "zero_pool",
            Self::HugePage => "hugepage",
            Self::FsCache => "fs_cache",
            Self::NetBuffer => "net_buf",
            Self::Crypto => "crypto",
            Self::Boot => "boot",
            Self::SelfTest => "selftest",
            Self::Firmware => "firmware",
            Self::Framebuffer => "framebuf",
            Self::Compressed => "compressed",
        }
    }

    /// Total number of defined owner tags.
    pub const COUNT: usize = 24;
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Per-frame owner tags.  Index = frame index, value = Owner as u8.
///
/// Uses a flat array for O(1) lookup.  No heap allocation needed.
/// Wrapped in UnsafeCell for interior mutability since each frame slot
/// is only written by the CPU that just allocated/freed it (no races).
struct OwnerArray(core::cell::UnsafeCell<[u8; MAX_FRAMES]>);

// SAFETY: Each frame slot is only mutated by the CPU that just allocated
// or freed it, so there are no data races.  Reads from diagnostics are
// inherently racy but that's acceptable for statistics.
unsafe impl Sync for OwnerArray {}

static OWNERS: OwnerArray = OwnerArray(core::cell::UnsafeCell::new([0u8; MAX_FRAMES]));

/// Whether frame ownership tracking is enabled.
///
/// Can be disabled at runtime to eliminate the overhead of set/clear
/// operations on every alloc/free.
static ENABLED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(true);

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total set() calls since boot.
static TOTAL_SETS: AtomicU64 = AtomicU64::new(0);

/// Total clear() calls since boot.
static TOTAL_CLEARS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record ownership of a frame.
///
/// Called by the frame allocator after a successful allocation.
/// If tracking is disabled, this is a no-op.
///
/// # Safety
///
/// Caller must ensure `frame_idx < MAX_FRAMES` and that this CPU
/// has exclusive access to the frame (just allocated it).
#[inline]
pub fn set(frame_idx: usize, owner: Owner) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if frame_idx >= MAX_FRAMES {
        return;
    }
    // SAFETY: Frame was just allocated by this CPU — no concurrent access.
    // The array is large enough (checked above).  We use raw pointer
    // arithmetic to avoid creating a reference to the mutable static.
    unsafe {
        let ptr = OWNERS.0.get() as *mut u8;
        ptr.add(frame_idx).write(owner as u8);
    }
    TOTAL_SETS.fetch_add(1, Ordering::Relaxed);
}

/// Clear ownership of a frame (mark as free).
///
/// Called by the frame allocator when a frame is freed.
///
/// # Safety
///
/// Caller must ensure `frame_idx < MAX_FRAMES` and has exclusive
/// ownership (is freeing the frame).
#[inline]
pub fn clear(frame_idx: usize) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    if frame_idx >= MAX_FRAMES {
        return;
    }
    // SAFETY: Frame is being freed by this CPU — exclusive access.
    unsafe {
        let ptr = OWNERS.0.get() as *mut u8;
        ptr.add(frame_idx).write(Owner::Free as u8);
    }
    TOTAL_CLEARS.fetch_add(1, Ordering::Relaxed);
}

/// Query the owner of a specific frame.
#[inline]
#[must_use]
pub fn get(frame_idx: usize) -> Owner {
    if frame_idx >= MAX_FRAMES {
        return Owner::Unknown;
    }
    // SAFETY: Read-only access, and we checked bounds.
    let raw = unsafe {
        let ptr = OWNERS.0.get() as *const u8;
        ptr.add(frame_idx).read()
    };
    Owner::from_u8(raw)
}

/// Enable ownership tracking.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable ownership tracking (for performance-critical periods).
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether tracking is currently enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Summary / reporting
// ---------------------------------------------------------------------------

/// Per-owner frame count summary.
#[derive(Debug, Clone)]
pub struct OwnerSummary {
    /// Frame count per owner tag.
    pub counts: [u32; Owner::COUNT],
    /// Total allocated (non-free) frames.
    pub total_allocated: u32,
    /// Total free frames (according to ownership tracking).
    pub total_free: u32,
    /// Total set() calls since boot.
    pub total_sets: u64,
    /// Total clear() calls since boot.
    pub total_clears: u64,
}

/// Compute a summary of frame ownership across all physical memory.
///
/// Scans the entire ownership array.  O(MAX_FRAMES) — not for hot paths,
/// but fine for diagnostics (takes < 1ms for 65536 frames).
#[must_use]
pub fn summary() -> OwnerSummary {
    let mut counts = [0u32; Owner::COUNT];
    let mut total_free: u32 = 0;
    let mut total_allocated: u32 = 0;

    for i in 0..MAX_FRAMES {
        // SAFETY: i is in bounds (0..MAX_FRAMES).
        let raw = unsafe {
            let ptr = OWNERS.0.get() as *const u8;
            ptr.add(i).read()
        };
        let idx = raw as usize;
        if idx < Owner::COUNT {
            counts[idx] = counts[idx].saturating_add(1);
        }
        if raw == 0 {
            total_free = total_free.saturating_add(1);
        } else {
            total_allocated = total_allocated.saturating_add(1);
        }
    }

    OwnerSummary {
        counts,
        total_allocated,
        total_free,
        total_sets: TOTAL_SETS.load(Ordering::Relaxed),
        total_clears: TOTAL_CLEARS.load(Ordering::Relaxed),
    }
}

/// Find up to `limit` frame indices owned by the given tag.
///
/// Returns the actual count found (may be less than limit).
/// Useful for targeted investigation of a specific subsystem's usage.
pub fn find_by_owner(owner: Owner, buf: &mut [usize]) -> usize {
    let target = owner as u8;
    let mut found = 0;

    for i in 0..MAX_FRAMES {
        if found >= buf.len() {
            break;
        }
        // SAFETY: i is in bounds (0..MAX_FRAMES).
        let raw = unsafe {
            let ptr = OWNERS.0.get() as *const u8;
            ptr.add(i).read()
        };
        if raw == target {
            buf[found] = i;
            found += 1;
        }
    }
    found
}

/// Get the top N owners by frame count (sorted descending).
///
/// Returns an array of (Owner, count) pairs.  Useful for the kshell
/// command to show "who is using the most memory?"
#[must_use]
pub fn top_owners() -> [(Owner, u32); Owner::COUNT] {
    let s = summary();
    let mut result = [(Owner::Free, 0u32); Owner::COUNT];

    for (i, &count) in s.counts.iter().enumerate() {
        result[i] = (Owner::from_u8(i as u8), count);
    }

    // Sort by count descending (simple insertion sort, N=24 is tiny).
    for i in 1..Owner::COUNT {
        let mut j = i;
        while j > 0 && result[j].1 > result[j - 1].1 {
            result.swap(j, j - 1);
            j -= 1;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for frame ownership tracking.
pub fn self_test() {
    serial_println!("[frame_owner] Running self-test...");

    // Test 1: Default state — all frames are Free.
    let owner = get(0);
    assert_eq!(owner, Owner::Free, "frame 0 should start as Free");
    serial_println!("[frame_owner]   Default Free state: OK");

    // Test 2: Set and get.
    set(100, Owner::HeapSlab);
    assert_eq!(get(100), Owner::HeapSlab);
    set(101, Owner::KernelStack);
    assert_eq!(get(101), Owner::KernelStack);
    set(102, Owner::PageTable);
    assert_eq!(get(102), Owner::PageTable);
    serial_println!("[frame_owner]   Set/get round-trip: OK");

    // Test 3: Clear resets to Free.
    clear(100);
    assert_eq!(get(100), Owner::Free);
    clear(101);
    clear(102);
    serial_println!("[frame_owner]   Clear resets to Free: OK");

    // Test 4: Out-of-bounds returns Unknown without panicking.
    assert_eq!(get(MAX_FRAMES + 1), Owner::Unknown);
    set(MAX_FRAMES + 1, Owner::Dma); // Should be no-op.
    serial_println!("[frame_owner]   Out-of-bounds safety: OK");

    // Test 5: Summary reports correct counts.
    set(200, Owner::UserAnon);
    set(201, Owner::UserAnon);
    set(202, Owner::UserAnon);
    set(203, Owner::Dma);
    let s = summary();
    assert!(s.counts[Owner::UserAnon as usize] >= 3);
    assert!(s.counts[Owner::Dma as usize] >= 1);
    // Cleanup.
    clear(200);
    clear(201);
    clear(202);
    clear(203);
    serial_println!("[frame_owner]   Summary counting: OK");

    // Test 6: find_by_owner locates tagged frames.
    set(300, Owner::Crypto);
    set(301, Owner::Crypto);
    set(302, Owner::Crypto);
    let mut buf = [0usize; 8];
    let found = find_by_owner(Owner::Crypto, &mut buf);
    assert!(found >= 3, "should find at least 3 Crypto frames");
    assert!(buf[..found].contains(&300));
    assert!(buf[..found].contains(&301));
    assert!(buf[..found].contains(&302));
    // Cleanup.
    clear(300);
    clear(301);
    clear(302);
    serial_println!("[frame_owner]   find_by_owner: OK");

    // Test 7: top_owners sorting.
    set(400, Owner::Vmalloc);
    set(401, Owner::Vmalloc);
    set(402, Owner::Vmalloc);
    set(403, Owner::Vmalloc);
    set(410, Owner::NetBuffer);
    set(411, Owner::NetBuffer);
    let top = top_owners();
    // Free should be at top (most frames are unallocated).
    assert_eq!(top[0].0, Owner::Free, "Free should have most frames");
    // Cleanup.
    clear(400);
    clear(401);
    clear(402);
    clear(403);
    clear(410);
    clear(411);
    serial_println!("[frame_owner]   top_owners sorted: OK");

    // Test 8: Disable/enable.
    disable();
    set(500, Owner::Boot);
    assert_eq!(get(500), Owner::Free, "set should be no-op when disabled");
    enable();
    set(500, Owner::Boot);
    assert_eq!(get(500), Owner::Boot, "set should work when re-enabled");
    clear(500);
    serial_println!("[frame_owner]   Enable/disable toggle: OK");

    // Test 9: Statistics tracking.
    let s2 = summary();
    assert!(s2.total_sets > 0);
    assert!(s2.total_clears > 0);
    serial_println!("[frame_owner]   Stats: sets={}, clears={}",
        s2.total_sets, s2.total_clears);

    serial_println!("[frame_owner] Self-test PASSED");
}

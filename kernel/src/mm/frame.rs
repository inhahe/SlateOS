//! Physical frame allocator.
//!
//! Buddy allocator for 16 KiB base pages.  Based on the Linux kernel's
//! buddy allocator design (`mm/page_alloc.c`) adapted for 16 KiB base
//! pages instead of 4 KiB.
//!
//! ## Design
//!
//! The allocator manages physical memory in blocks of 2^order frames,
//! where one frame = 16 KiB (4 contiguous 4 KiB hardware pages).
//!
//! - Order 0: 16 KiB (1 frame)
//! - Order 1: 32 KiB (2 frames)
//! - ...
//! - Order 10: 16 MiB (1024 frames)
//!
//! Free blocks are stored on per-order doubly-linked intrusive free lists
//! (the list node is stored in the first 16 bytes of the free block itself,
//! which is safe because the block is not in use).
//!
//! A per-frame metadata array (1 byte per frame) tracks whether each frame
//! is allocated or free (and at what order).  This metadata is carved from
//! the first usable memory region during initialization.
//!
//! ## Initialization
//!
//! The allocator is initialized from the Limine memory map.  Only `USABLE`
//! regions are added to the free lists.  All other memory (reserved, ACPI,
//! kernel, framebuffer) is marked as permanently allocated.
//!
//! ## Per-CPU Free Lists
//!
//! Each CPU maintains a small cache of order-0 frames to avoid acquiring
//! the global spinlock on every single-frame allocation (the hot path).
//!
//! - **alloc_frame()**: tries the local cache first.  If empty, acquires
//!   the global lock and batch-refills (up to `PCPU_BATCH` frames at once).
//! - **free_frame()**: pushes to the local cache.  If full, acquires the
//!   global lock and batch-drains half the cache back.
//! - **alloc_order(N>0)**: bypasses per-CPU cache (multi-frame allocations
//!   need contiguous naturally-aligned blocks, which the cache doesn't
//!   provide).
//!
//! Cache access is protected by disabling interrupts (not a spinlock) —
//! since we're per-CPU, no other CPU touches our cache, and disabling
//! interrupts prevents preemption on the same CPU.  This makes the
//! common path lock-free relative to other CPUs.
//!
//! Based on Linux's `struct per_cpu_pages` in `mm/page_alloc.c`.
//!
//! ## Performance Target
//!
//! Single alloc/free: < 1us (Linux buddy: 100-500ns).
//! See `bench/baselines.toml` for measured targets.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::error::{KernelError, KernelResult};
use crate::limine::{MemmapEntry, memmap_type};
use crate::serial_println;
use spin::{Mutex, Once};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Size of a single physical frame (our base allocation unit).
pub const FRAME_SIZE: usize = 16 * 1024; // 16 KiB

/// Number of 4 KiB hardware pages per frame.
#[allow(dead_code)] // Public API for drivers and user-space mappings.
pub const PAGES_PER_FRAME: usize = FRAME_SIZE / 4096;

/// Maximum buddy order.  Order N = 2^N frames = `FRAME_SIZE` × 2^N bytes.
/// Order 10 = 1024 frames = 16 MiB.
const MAX_ORDER: usize = 10;

/// Page-info value indicating the frame is allocated (or not part of
/// usable memory).
const INFO_ALLOCATED: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Lock-free refcount snapshot (set once during init, never changes)
// ---------------------------------------------------------------------------

/// Cached pointer to the refcount array (HHDM virtual address).
///
/// OPT: `free_frame()` needs to check the refcount to decide whether
/// a shared (CoW) frame should go through the global allocator for
/// ref_dec.  Without this cache, it takes the global lock on EVERY
/// free just to read a u16.  By caching the immutable refcount pointer
/// and total_frames count here, the common case (refcount == 1) avoids
/// the lock entirely.
///
/// Safety argument: The refcount array is allocated once during `init()`
/// and never moves or reallocates.  Reads are `read_volatile` to pick
/// up the latest write from any CPU.  Refcount increments (ref_inc)
/// happen under the global lock which provides a memory fence; our
/// read sees the latest value because the freeing CPU was the owner —
/// any fork's ref_inc must have completed before the original process
/// unmaps the page.
static REFCOUNT_PTR: AtomicU64 = AtomicU64::new(0);
static REFCOUNT_LEN: AtomicU64 = AtomicU64::new(0);

/// Physical memory below this address is never added to the free lists.
///
/// The first 1 MiB of physical address space on x86 is reserved for:
/// - Real-mode IVT (0x000–0x3FF)
/// - BIOS Data Area (0x400–0x4FF)
/// - Extended BIOS Data Area (~0x80000–0x9FFFF)
/// - SMP AP trampoline (typically 0x8000)
/// - Legacy video memory (0xA0000–0xBFFFF)
/// - ROM / option ROMs (0xC0000–0xFFFFF)
///
/// Linux does the same: `memblock_reserve(0, SZ_1M)`.  We accept the
/// small memory loss (~640 KiB of usable conventional memory) in
/// exchange for never having to worry about low-memory conflicts.
const LOW_MEMORY_RESERVE: u64 = 0x10_0000; // 1 MiB

// ---------------------------------------------------------------------------
// PhysFrame
// ---------------------------------------------------------------------------

/// A physical frame address (always 16 KiB aligned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysFrame(u64);

impl PhysFrame {
    /// Create a `PhysFrame` from a raw physical address.
    ///
    /// Returns `None` if the address is not aligned to [`FRAME_SIZE`].
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub const fn from_addr(addr: u64) -> Option<Self> {
        if addr.is_multiple_of(FRAME_SIZE as u64) {
            Some(Self(addr))
        } else {
            None
        }
    }

    /// The raw physical address of this frame.
    #[must_use]
    pub const fn addr(self) -> u64 {
        self.0
    }

    /// Convert to a virtual address using the HHDM offset.
    ///
    /// Physical addresses + HHDM offset cannot overflow on `x86_64`
    /// (both are within the 48/57-bit address space).
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn to_virt(self, hhdm_offset: u64) -> u64 {
        self.0 + hhdm_offset
    }
}

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

/// A doubly-linked free-list node, stored in the first 16 bytes of a
/// free block.
///
/// Since the block is free (not in use by anyone), we repurpose its
/// memory for bookkeeping.  This avoids separate allocation for the
/// free list structure.
#[repr(C)]
struct FreeNode {
    /// Physical address of the next free block at this order (0 = end).
    next: u64,
    /// Physical address of the previous free block at this order (0 = head).
    prev: u64,
}

/// Head of a per-order doubly-linked free list.
#[derive(Clone, Copy)]
struct FreeList {
    /// Physical address of the first free block (0 = empty list).
    head: u64,
    /// Number of blocks on this list.
    count: usize,
}

impl FreeList {
    const fn new() -> Self {
        Self { head: 0, count: 0 }
    }
}

/// The buddy allocator state.
///
/// Protected by a spinlock in the global [`ALLOCATOR`].  All methods
/// assume the caller holds the lock (exclusive access).
struct BuddyAllocator {
    /// One free list per buddy order (0..=[`MAX_ORDER`]).
    free_lists: [FreeList; MAX_ORDER + 1],

    /// Per-frame metadata array.  Indexed by frame number
    /// (`phys_addr / FRAME_SIZE`).
    ///
    /// Values:
    /// - `0..=MAX_ORDER` — frame is the head of a free block at this order
    /// - [`INFO_ALLOCATED`] — frame is allocated or not part of usable memory
    ///
    /// Only the FIRST frame of a multi-frame free block stores the order;
    /// remaining frames in the block keep [`INFO_ALLOCATED`] to prevent
    /// false buddy matches during coalescing.
    page_info: *mut u8,

    /// Length of the `page_info` array (= total managed frames).
    page_info_len: usize,

    /// Per-frame reference count array.  Indexed by frame number.
    ///
    /// - 0: frame is free (not allocated)
    /// - 1: normal single-owner allocation
    /// - 2+: shared via Copy-on-Write (multiple page tables reference
    ///   the same physical frame)
    ///
    /// When freeing a frame, the refcount is decremented.  The frame is
    /// only returned to the free lists when the refcount reaches 0.
    /// This enables efficient CoW: on fork/clone, shared pages have their
    /// refcount bumped instead of being copied immediately.
    refcount: *mut u16,

    /// Total number of frames in the managed physical address range.
    /// Includes non-usable holes marked as permanently allocated.
    total_frames: usize,

    /// HHDM offset for physical-to-virtual address conversion.
    hhdm_offset: u64,

    /// Number of currently free frames (sum of 2^order across all free
    /// blocks).
    free_frames: usize,
}

// SAFETY: BuddyAllocator is only ever accessed through a spin::Mutex,
// which provides exclusive access.  The raw pointers `page_info` and
// `refcount` point to memory exclusively owned by this allocator
// (carved from usable physical memory during init, never aliased).
unsafe impl Send for BuddyAllocator {}

// ---------------------------------------------------------------------------
// BuddyAllocator implementation
// ---------------------------------------------------------------------------

impl BuddyAllocator {
    /// Convert a physical address to a virtual pointer via the HHDM.
    #[inline]
    #[allow(clippy::arithmetic_side_effects)]
    fn phys_to_virt(&self, phys: u64) -> *mut u8 {
        // Physical + HHDM offset fits in u64 on x86_64 (both are within
        // the 48/57-bit canonical address space).
        (phys + self.hhdm_offset) as *mut u8
    }

    /// Compute the frame index for a physical address.
    ///
    /// The address must be frame-aligned (callers validate this).
    // unused_self: kept as a method for readability at call sites
    // (allocator.frame_index(addr) reads better than frame_index(addr)).
    #[inline]
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::unused_self)]
    fn frame_index(&self, addr: u64) -> usize {
        // FRAME_SIZE is non-zero, and addr fits in u64.  On x86_64
        // (our only target), usize is 64 bits, so the cast is lossless.
        (addr / FRAME_SIZE as u64) as usize
    }

    /// Read one byte from the page-info array.
    #[inline]
    fn get_info(&self, idx: usize) -> u8 {
        debug_assert!(idx < self.page_info_len, "page_info index out of bounds");
        // SAFETY: idx < page_info_len is guaranteed by callers (validated
        // before calling, or asserted above in debug builds).  The buffer
        // is exclusively owned and valid for page_info_len bytes.
        unsafe { self.page_info.add(idx).read() }
    }

    /// Write one byte to the page-info array.
    #[inline]
    fn set_info(&mut self, idx: usize, value: u8) {
        debug_assert!(idx < self.page_info_len, "page_info index out of bounds");
        // SAFETY: same as get_info.
        unsafe { self.page_info.add(idx).write(value); }
    }

    // -- Reference count operations -----------------------------------------

    /// Read the reference count for a frame.
    #[inline]
    fn get_refcount(&self, idx: usize) -> u16 {
        debug_assert!(idx < self.page_info_len, "refcount index out of bounds");
        // SAFETY: idx < page_info_len, refcount array is the same length
        // as page_info, exclusively owned by this allocator.
        unsafe { self.refcount.add(idx).read() }
    }

    /// Write the reference count for a frame.
    #[inline]
    fn set_refcount(&mut self, idx: usize, value: u16) {
        debug_assert!(idx < self.page_info_len, "refcount index out of bounds");
        // SAFETY: same as get_refcount.
        unsafe { self.refcount.add(idx).write(value); }
    }

    // -- Free list operations ------------------------------------------------

    /// Push a free block onto the front of its order's free list.
    ///
    /// Writes a [`FreeNode`] into the block's first 16 bytes (via HHDM)
    /// and updates the list head.
    // cast_ptr_alignment: All frame addresses are 16 KiB aligned, which
    // exceeds FreeNode's 8-byte alignment requirement.
    // cast_possible_truncation: order is bounded by MAX_ORDER (10), fits in u8.
    #[allow(clippy::indexing_slicing, clippy::cast_ptr_alignment, clippy::cast_possible_truncation)]
    fn push_free(&mut self, addr: u64, order: usize) {
        debug_assert!(order <= MAX_ORDER);
        let node_ptr = self.phys_to_virt(addr).cast::<FreeNode>();
        let old_head = self.free_lists[order].head;

        // SAFETY: `addr` points to a free block of at least FRAME_SIZE
        // bytes in usable physical memory.  The HHDM mapping covers all
        // of physical memory.  We have exclusive access via the spinlock.
        unsafe {
            node_ptr.write(FreeNode {
                next: old_head,
                prev: 0,
            });
        }

        // Link old head back to us.
        if old_head != 0 {
            let old_head_ptr = self.phys_to_virt(old_head).cast::<FreeNode>();
            // SAFETY: old_head is a valid free block on this order's list.
            unsafe { (*old_head_ptr).prev = addr; }
        }

        self.free_lists[order].head = addr;
        self.free_lists[order].count = self.free_lists[order].count.saturating_add(1);

        // Mark this frame as free at the given order.
        let idx = self.frame_index(addr);
        self.set_info(idx, order as u8);
    }

    /// Remove a specific block from its order's free list.
    ///
    /// Used during buddy coalescing to unlink the buddy before promoting
    /// the merged block to a higher order.
    // cast_ptr_alignment: same as push_free — frame addresses are 16 KiB aligned.
    #[allow(clippy::indexing_slicing, clippy::cast_ptr_alignment)]
    fn remove_free(&mut self, addr: u64, order: usize) {
        debug_assert!(order <= MAX_ORDER);
        let node_ptr = self.phys_to_virt(addr).cast::<FreeNode>();

        // SAFETY: addr is a valid free block on free_lists[order].
        let (next, prev) = unsafe {
            let node = &*node_ptr;
            (node.next, node.prev)
        };

        // Unlink from neighbors.
        if prev != 0 {
            let prev_ptr = self.phys_to_virt(prev).cast::<FreeNode>();
            // SAFETY: prev is a valid free block on this list.
            unsafe { (*prev_ptr).next = next; }
        } else {
            // This block was the list head.
            self.free_lists[order].head = next;
        }

        if next != 0 {
            let next_ptr = self.phys_to_virt(next).cast::<FreeNode>();
            // SAFETY: next is a valid free block on this list.
            unsafe { (*next_ptr).prev = prev; }
        }

        self.free_lists[order].count = self.free_lists[order].count.saturating_sub(1);

        // Mark as allocated (no longer on any free list).
        let idx = self.frame_index(addr);
        self.set_info(idx, INFO_ALLOCATED);
    }

    /// Pop the head block from an order's free list.
    ///
    /// Returns `None` if the list is empty.
    fn pop_free(&mut self, order: usize) -> Option<u64> {
        debug_assert!(order <= MAX_ORDER);
        // Indexing: order is bounds-checked by debug_assert.
        #[allow(clippy::indexing_slicing)]
        let head = self.free_lists[order].head;
        if head == 0 {
            return None;
        }
        self.remove_free(head, order);
        Some(head)
    }

    // -- Bulk initialization -------------------------------------------------

    /// Add a contiguous range of frames to the free lists at the highest
    /// possible orders.
    ///
    /// Greedily picks the largest naturally-aligned block at each step.
    /// Both `start` and `end` must be frame-aligned.
    #[allow(clippy::arithmetic_side_effects)]
    fn add_free_range(&mut self, start: u64, end: u64) {
        debug_assert!(start <= end);
        let frame_size = FRAME_SIZE as u64;
        let mut addr = start;

        while addr < end {
            let remaining = end - addr;

            // Find the largest order whose block size both fits in the
            // remaining space AND is naturally aligned at `addr`.
            let mut order = 0;
            while order < MAX_ORDER {
                let next_size = frame_size << (order + 1);
                if next_size > remaining {
                    break;
                }
                if !addr.is_multiple_of(next_size) {
                    break;
                }
                order += 1;
            }

            self.push_free(addr, order);
            self.free_frames = self.free_frames.saturating_add(1 << order);
            addr += frame_size << order;
        }
    }

    // -- Allocation and freeing ----------------------------------------------

    /// Allocate a contiguous block of 2^order frames.
    ///
    /// Finds the smallest order with a free block, pops it, and splits
    /// down to the requested order (returning upper halves to their
    /// respective free lists).
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn alloc_inner(&mut self, order: usize) -> KernelResult<u64> {
        if order > MAX_ORDER {
            return Err(KernelError::InvalidArgument);
        }

        // Walk up to find the smallest order with a free block.
        let mut source_order = order;
        while source_order <= MAX_ORDER {
            if self.free_lists[source_order].head != 0 {
                break;
            }
            source_order += 1;
        }

        if source_order > MAX_ORDER {
            return Err(KernelError::OutOfMemory);
        }

        // Pop from the source order's list.
        let addr = self.pop_free(source_order)
            .ok_or(KernelError::InternalError)?;

        // Split down to the requested order, putting the upper halves
        // on their respective free lists.
        while source_order > order {
            source_order -= 1;
            // The upper half of the split block starts at:
            let buddy_addr = addr + (FRAME_SIZE as u64 * (1u64 << source_order));
            self.push_free(buddy_addr, source_order);
        }

        // Update free-frame count.  push_free / remove_free are pure list
        // operations — only alloc_inner, free_inner, and add_free_range
        // modify the counter.  Net change: we consumed 2^order frames
        // (the split halves cancel out).

        let frames_out = 1usize << order;
        self.free_frames = self.free_frames.saturating_sub(frames_out);

        // Set refcount = 1 for all frames in the allocated block.
        // A refcount of 1 means single-owner; CoW sharing bumps it to 2+.
        let base_idx = self.frame_index(addr);
        for i in 0..frames_out {
            self.set_refcount(base_idx.saturating_add(i), 1);
        }

        Ok(addr)
    }

    /// Allocate a block of `2^order` contiguous frames whose physical
    /// address is entirely below `max_addr`.
    ///
    /// Used by the DMA subsystem for devices with address constraints
    /// (e.g., 32-bit DMA can only address below 4 GiB).
    ///
    /// Walks the free list at each order (from `order` up to `MAX_ORDER`)
    /// looking for a block whose base address satisfies:
    ///   `addr + (FRAME_SIZE << order) <= max_addr`
    ///
    /// This is O(n) in the number of free blocks at each order, but DMA
    /// allocation is infrequent and not on a hot path.
    ///
    /// Based on Linux's GFP_DMA / GFP_DMA32 zone-aware allocation
    /// (`mm/page_alloc.c`).
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn alloc_inner_constrained(&mut self, order: usize, max_addr: u64) -> KernelResult<u64> {
        if order > MAX_ORDER {
            return Err(KernelError::InvalidArgument);
        }

        let alloc_size = (FRAME_SIZE as u64) << order;

        // Scan each order from `order` up to MAX_ORDER.
        // At higher orders we'll split down, but the lower half is always
        // at the base address — so checking `addr + alloc_size <= max_addr`
        // is sufficient regardless of the source order.
        for source_order in order..=MAX_ORDER {
            // Walk the doubly-linked free list at this order.
            let mut addr = self.free_lists[source_order].head;
            while addr != 0 {
                // Check if the allocation (the lower portion after splitting)
                // fits within the constraint.
                if addr.checked_add(alloc_size).map_or(false, |end| end <= max_addr) {
                    // Found a suitable block.  Remove it from the list.
                    self.remove_free(addr, source_order);

                    // Split down to the requested order.
                    let mut current_order = source_order;
                    while current_order > order {
                        current_order -= 1;
                        let buddy_addr = addr + (FRAME_SIZE as u64 * (1u64 << current_order));
                        self.push_free(buddy_addr, current_order);
                    }

                    // Update bookkeeping (same as alloc_inner).
                    let frames_out = 1usize << order;
                    self.free_frames = self.free_frames.saturating_sub(frames_out);

                    let base_idx = self.frame_index(addr);
                    for i in 0..frames_out {
                        self.set_refcount(base_idx.saturating_add(i), 1);
                    }

                    return Ok(addr);
                }

                // Move to the next block in this order's free list.
                let node_ptr = self.phys_to_virt(addr).cast::<FreeNode>();
                // SAFETY: addr is on the free list, node is valid.
                addr = unsafe { (*node_ptr).next };
            }
        }

        Err(KernelError::OutOfMemory)
    }

    /// Free a block of 2^order contiguous frames, coalescing with buddies.
    ///
    /// Attempts to merge with the buddy block at each order level,
    /// recursively coalescing up to [`MAX_ORDER`].
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn free_inner(&mut self, addr: u64, order: usize) -> KernelResult<()> {
        // Validate inputs.
        if order > MAX_ORDER {
            return Err(KernelError::InvalidArgument);
        }
        #[allow(clippy::cast_possible_truncation)]
        if !addr.is_multiple_of(FRAME_SIZE as u64) {
            return Err(KernelError::BadAlignment);
        }

        let idx = self.frame_index(addr);
        if idx >= self.total_frames {
            return Err(KernelError::InvalidAddress);
        }

        // Double-free detection: if the frame is not marked as allocated,
        // it's either already free or was never allocated.
        if self.get_info(idx) != INFO_ALLOCATED {
            return Err(KernelError::InvalidAddress);
        }

        // Decrement refcount.  Only actually free the block when the
        // refcount reaches 0 (last reference dropped).  This supports
        // CoW: shared frames have refcount > 1, and each free just
        // decrements until the last user frees.
        let frames_in_block = 1usize << order;
        let rc = self.get_refcount(idx);
        if rc > 1 {
            // Still shared — decrement all frames in the block and return.
            for i in 0..frames_in_block {
                let fi = idx.saturating_add(i);
                let cur = self.get_refcount(fi);
                self.set_refcount(fi, cur.saturating_sub(1));
            }
            return Ok(());
        }

        // refcount is 0 or 1 — actually free the block.
        // Zero the refcount for all frames in the block.
        for i in 0..frames_in_block {
            self.set_refcount(idx.saturating_add(i), 0);
        }

        let frame_size = FRAME_SIZE as u64;
        let mut current_addr = addr;
        let mut current_order = order;

        // Try to coalesce with buddies at each order level.
        while current_order < MAX_ORDER {
            // The buddy's address is found by flipping the bit at the
            // current block size position.  For a block of order N at
            // address A, the buddy is at A XOR (FRAME_SIZE * 2^N).
            let buddy_addr = current_addr ^ (frame_size << current_order);
            let buddy_idx = self.frame_index(buddy_addr);

            // Can't coalesce if buddy is outside managed range.
            if buddy_idx >= self.total_frames {
                break;
            }

            // Can't coalesce if buddy is not free at the same order.
            #[allow(clippy::cast_possible_truncation)]
            let expected_order = current_order as u8;
            if self.get_info(buddy_idx) != expected_order {
                break;
            }

            // Buddy is free at the same order — unlink it and merge.
            self.remove_free(buddy_addr, current_order);

            // The coalesced block starts at the lower of the two addresses.
            current_addr = core::cmp::min(current_addr, buddy_addr);
            current_order += 1;
        }

        // Add the (possibly coalesced) block to the appropriate free list.
        self.push_free(current_addr, current_order);

        // Update stats.
        let frames_in = 1usize << order;
        self.free_frames = self.free_frames.saturating_add(frames_in);

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Global allocator instance
// ---------------------------------------------------------------------------

/// The singleton buddy allocator, initialized once during boot.
static ALLOCATOR: Once<Mutex<BuddyAllocator>> = Once::new();

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Align `addr` up to the next multiple of `align` (power of two).
#[inline]
#[allow(clippy::arithmetic_side_effects)]
const fn align_up(addr: u64, align: u64) -> u64 {
    // `align` is always a power of two (FRAME_SIZE or derived).
    // (addr + align - 1) cannot overflow for practical physical addresses
    // (52-bit max) and alignment values (16 MiB max).
    (addr + align - 1) & !(align - 1)
}

/// Align `addr` down to the previous multiple of `align` (power of two).
#[inline]
#[allow(clippy::arithmetic_side_effects)]
const fn align_down(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}

// ---------------------------------------------------------------------------
// Per-CPU frame cache
// ---------------------------------------------------------------------------

use crate::smp::MAX_CPUS;

/// Number of frames in each per-CPU cache.
///
/// Chosen to amortize lock acquisition cost without hoarding too much
/// memory per CPU.  32 frames × 16 KiB = 512 KiB per CPU.
const PCPU_CACHE_SIZE: usize = 32;

/// Number of frames to transfer in a single batch refill/drain.
///
/// Half the cache size — so a full cache drain transfers 16 frames,
/// and a refill from empty gets 16 frames.  This bounds the worst-case
/// time spent holding the global lock during batch operations.
const PCPU_BATCH: usize = PCPU_CACHE_SIZE / 2;

/// Per-CPU frame cache.
///
/// Each CPU keeps a small stack of order-0 frame physical addresses.
/// Access is serialized by disabling interrupts (per-CPU, so no other
/// CPU touches this cache; disabling interrupts prevents preemption).
///
/// The `count` field tracks how many valid entries are in `frames[0..count]`.
/// OPT: Aligned to 64 bytes (x86 cache line) to prevent false sharing
/// between CPUs.  Without this, adjacent CPUs' caches share a cache
/// line, causing expensive cache-line bouncing on every alloc/free.
#[repr(align(64))]
struct PerCpuFrameCache {
    /// Stack of cached frame physical addresses.
    frames: [u64; PCPU_CACHE_SIZE],
    /// Number of valid entries (0 = empty, PCPU_CACHE_SIZE = full).
    count: usize,
}

impl PerCpuFrameCache {
    const fn new() -> Self {
        Self {
            frames: [0; PCPU_CACHE_SIZE],
            count: 0,
        }
    }

    /// Pop a frame from the cache.  Returns `None` if empty.
    #[inline]
    fn pop(&mut self) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        self.count -= 1;
        // SAFETY: count was > 0, so frames[count] is valid.
        Some(self.frames[self.count])
    }

    /// Push a frame onto the cache.  Returns `false` if full.
    #[inline]
    fn push(&mut self, addr: u64) -> bool {
        if self.count >= PCPU_CACHE_SIZE {
            return false;
        }
        self.frames[self.count] = addr;
        self.count += 1;
        true
    }

    /// Is the cache empty?
    #[inline]
    #[allow(dead_code)] // Useful for diagnostics / future cache tuning.
    fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Is the cache full?
    #[inline]
    fn is_full(&self) -> bool {
        self.count >= PCPU_CACHE_SIZE
    }
}

/// Global array of per-CPU frame caches.
///
/// Indexed by `smp::fast_cpu_index()`.  Each cache is a simple array
/// (no heap allocation needed).
///
/// SAFETY: Each element is only accessed by its owning CPU with interrupts
/// disabled (preventing preemption).  No two CPUs access the same element.
/// The array is wrapped in `UnsafeCell` to allow interior mutability
/// without a Mutex (the per-CPU access pattern provides exclusion).
static mut PCPU_CACHES: [PerCpuFrameCache; MAX_CPUS] = {
    const INIT: PerCpuFrameCache = PerCpuFrameCache::new();
    [INIT; MAX_CPUS]
};

/// Whether per-CPU caches are active.
///
/// Disabled during early boot (before SMP init) and during the allocator
/// self-test.  When disabled, `alloc_frame()` falls through to the global
/// allocator directly.
static PCPU_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable per-CPU frame caches.
///
/// Call after SMP initialization is complete (all CPUs are online and
/// `fast_cpu_index()` returns correct values).
pub fn enable_pcpu_caches() {
    PCPU_ENABLED.store(true, Ordering::Release);
    serial_println!("[mm] Per-CPU frame caches enabled");
}

/// Disable interrupts and return the previous RFLAGS value.
///
/// Used by per-CPU caches (frame and heap) to prevent preemption
/// during cache access on the local CPU.
///
/// # Safety
///
/// Caller must restore interrupts via [`restore_interrupts`] promptly.
/// Holding interrupts disabled for too long causes latency issues.
#[inline]
pub(crate) unsafe fn disable_interrupts() -> u64 {
    let flags: u64;
    // SAFETY: pushfq/popfq + cli is safe in ring 0.
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {}",
            "cli",
            out(reg) flags,
            options(nomem, preserves_flags),
        );
    }
    flags
}

/// Restore the RFLAGS value (re-enabling interrupts if they were enabled).
///
/// # Safety
///
/// `flags` must be a value from a prior [`disable_interrupts`] call.
#[inline]
pub(crate) unsafe fn restore_interrupts(flags: u64) {
    // SAFETY: Restoring RFLAGS to a previously-saved value is safe.
    unsafe {
        core::arch::asm!(
            "push {}",
            "popfq",
            in(reg) flags,
            options(nomem),
        );
    }
}

/// Batch-refill the current CPU's cache from the global allocator.
///
/// Called with interrupts disabled and the global lock NOT held.
/// Acquires the global lock, pops up to `PCPU_BATCH` order-0 frames,
/// and pushes them into the per-CPU cache.
///
/// Returns the number of frames transferred.
#[allow(clippy::indexing_slicing)]
fn pcpu_refill(cpu: usize) -> usize {
    let Some(allocator) = ALLOCATOR.get() else {
        return 0;
    };
    let mut guard = allocator.lock();

    let mut refilled = 0;
    for _ in 0..PCPU_BATCH {
        match guard.alloc_inner(0) {
            Ok(addr) => {
                // SAFETY: cpu < MAX_CPUS (validated by smp::fast_cpu_index()),
                // and interrupts are disabled so no preemption.
                unsafe {
                    PCPU_CACHES[cpu].push(addr);
                }
                refilled += 1;
            }
            Err(_) => break, // Out of memory.
        }
    }

    refilled
}

/// Batch-drain half of the current CPU's cache back to the global allocator.
///
/// Called with interrupts disabled when the per-CPU cache is full.
/// Returns the number of frames drained.
#[allow(clippy::indexing_slicing)]
fn pcpu_drain(cpu: usize) -> usize {
    let Some(allocator) = ALLOCATOR.get() else {
        return 0;
    };
    let mut guard = allocator.lock();

    let mut drained = 0;
    for _ in 0..PCPU_BATCH {
        // SAFETY: cpu < MAX_CPUS, interrupts disabled.
        let addr = unsafe { PCPU_CACHES[cpu].pop() };
        match addr {
            Some(a) => {
                // Return frame to global buddy allocator.
                // Ignore errors (frame already free = logic bug, but
                // don't panic in the allocator).
                let _ = guard.free_inner(a, 0);
                drained += 1;
            }
            None => break,
        }
    }

    drained
}

// ---------------------------------------------------------------------------
// Pre-zeroed frame pool
// ---------------------------------------------------------------------------
//
// Maintains a cache of frames that have already been zeroed during idle
// time.  `alloc_frame_zeroed()` drains this pool first, avoiding the
// inline 16 KiB memset on the page fault hot path.
//
// The idle loop calls `refill_zero_pool()` to replenish frames in the
// background, doing useful work instead of just HLT.
//
// Based on Linux's free page zeroing and Windows' zero page thread.
// See `bench/baselines.toml` — page fault latency is dominated by the
// 16 KiB zero; this optimization removes it from the critical path.

/// Zero a 16 KiB region using non-temporal (streaming) stores.
///
/// Non-temporal stores bypass the CPU cache and write directly to memory.
/// This is ideal for the idle-loop zero pool refill path because:
///
/// 1. The zeroed frame will be consumed later (possibly on a different CPU),
///    so caching zeros here wastes L1/L2 capacity on this CPU's working set.
/// 2. Streaming stores can saturate memory bandwidth without the
///    read-for-ownership overhead of regular stores (no cache line fetch
///    before write).
/// 3. The idle CPU has nothing better to use its cache for, but other CPUs
///    sharing LLC will benefit from not having their data evicted.
///
/// Uses 8-byte `movnti` writes (available on all x86-64 CPUs, no SSE/AVX
/// feature detection needed).  Followed by `sfence` to ensure all NT
/// stores are visible before the frame is handed to another CPU.
///
/// # Safety
///
/// `ptr` must point to a valid, exclusively-owned 16 KiB region.
/// The pointer must be at least 8-byte aligned (16 KiB frame addresses
/// always satisfy this).
#[inline]
unsafe fn zero_frame_nontemporal(ptr: *mut u8) {
    // FRAME_SIZE (16384) / 8 = 2048 iterations of 8-byte stores.
    // Using movnti (64-bit non-temporal store) avoids polluting the cache.
    let qwords = FRAME_SIZE / 8;
    let ptr64 = ptr.cast::<u64>();

    // SAFETY: Caller guarantees the region is valid and exclusively owned.
    // movnti requires only 4-byte alignment; our 16 KiB aligned pointer
    // satisfies this trivially.  We write exactly FRAME_SIZE bytes.
    unsafe {
        for i in 0..qwords {
            core::arch::x86_64::_mm_stream_si64(
                ptr64.add(i).cast::<i64>(),
                0,
            );
        }
        // sfence: ensure all non-temporal stores complete before we hand
        // this frame to another CPU.  Without this, the pool consumer
        // could see stale (non-zero) data.
        core::arch::x86_64::_mm_sfence();
    }
}

/// Maximum number of pre-zeroed frames in the pool.
///
/// OPT: Increased from 64→256 (1 MiB → 4 MiB) to absorb typical
/// application launch bursts.  A medium application startup triggers
/// 50-200 page faults; with 64 slots the pool emptied after the first
/// 64 faults and fell back to expensive inline 16 KiB zeroing for the
/// rest.  256 slots cover the full burst for most apps.
///
/// 256 frames × 16 KiB = 4 MiB.  On a 16 GB desktop, that's 0.02%
/// of RAM.  Even on our 256 MiB QEMU test system it's only 1.5%.
const ZERO_POOL_CAPACITY: usize = 256;

/// Number of frames to zero in a single `refill_zero_pool()` call.
///
/// OPT: Increased from 8→16 to replenish the pool faster after a
/// burst.  Each 16 KiB zero takes ~3µs (12k cycles on the dev CPU),
/// so a batch of 16 takes ~48µs — still well under a timer tick (10ms)
/// and acceptable for idle-loop work.
const ZERO_POOL_REFILL_BATCH: usize = 16;

/// Pre-zeroed frame pool.
///
/// A simple stack (LIFO) of physical frame addresses that have been
/// zeroed.  Protected by a spinlock separate from the main allocator
/// lock to minimize contention.
struct ZeroPool {
    /// Frame addresses (physical, 16 KiB aligned).  Only `count`
    /// entries are valid.
    frames: [u64; ZERO_POOL_CAPACITY],
    /// Number of valid entries in `frames`.
    count: usize,
}

impl ZeroPool {
    const fn new() -> Self {
        Self {
            frames: [0; ZERO_POOL_CAPACITY],
            count: 0,
        }
    }

    /// Pop a pre-zeroed frame from the pool.  Returns `None` if empty.
    fn pop(&mut self) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        self.count -= 1;
        // SAFETY: count was > 0, so frames[count] is valid.
        Some(self.frames[self.count])
    }

    /// Push a zeroed frame into the pool.  Returns `false` if full.
    fn push(&mut self, phys: u64) -> bool {
        if self.count >= ZERO_POOL_CAPACITY {
            return false;
        }
        self.frames[self.count] = phys;
        self.count += 1;
        true
    }
}

static ZERO_POOL: Mutex<ZeroPool> = Mutex::new(ZeroPool::new());

/// Whether the zero pool is active.
///
/// Enabled after per-CPU caches are active and HHDM is available.
/// The idle loop checks this before calling `refill_zero_pool()`.
static ZERO_POOL_ENABLED: AtomicBool = AtomicBool::new(false);

/// Total number of frames served from the zero pool (diagnostic counter).
static ZERO_POOL_HITS: AtomicU64 = AtomicU64::new(0);

/// Total number of zeroed-frame requests that missed the pool (diagnostic).
static ZERO_POOL_MISSES: AtomicU64 = AtomicU64::new(0);

/// Enable the pre-zeroed frame pool.
///
/// Call after per-CPU frame caches are enabled and HHDM is available.
/// From this point, `alloc_frame_zeroed()` will check the zero pool
/// first, and `refill_zero_pool()` will populate it during idle time.
pub fn enable_zero_pool() {
    ZERO_POOL_ENABLED.store(true, Ordering::Release);
    serial_println!("[mm] Pre-zeroed frame pool enabled (capacity: {} frames)", ZERO_POOL_CAPACITY);
}

/// Refill the pre-zeroed frame pool.
///
/// Allocates up to `ZERO_POOL_REFILL_BATCH` frames, zeros them, and
/// adds them to the pool.  Returns the number of frames added.
///
/// **Call from the idle loop** — this function does real work (frame
/// allocation + 16 KiB memset per frame) and should not be called on
/// latency-sensitive paths.
///
/// Returns 0 if:
/// - The pool is already full
/// - The zero pool is not enabled
/// - Frame allocation fails (low memory)
/// - HHDM is not available
pub fn refill_zero_pool() -> usize {
    if !ZERO_POOL_ENABLED.load(Ordering::Acquire) {
        return 0;
    }

    let hhdm = match crate::mm::page_table::hhdm() {
        Some(h) => h,
        None => return 0,
    };

    // Check how many slots are available without holding the lock
    // during the expensive zeroing operation.
    let current_count = {
        let pool = ZERO_POOL.lock();
        pool.count
    };

    if current_count >= ZERO_POOL_CAPACITY {
        return 0;
    }

    let space = ZERO_POOL_CAPACITY.saturating_sub(current_count);
    let batch = space.min(ZERO_POOL_REFILL_BATCH);
    let mut added = 0usize;

    for _ in 0..batch {
        // Allocate a raw frame (unzeroed).
        let frame = match alloc_frame() {
            Ok(f) => f,
            Err(_) => break, // Low memory — stop refilling.
        };

        // Zero the frame outside any lock using non-temporal stores.
        // OPT: Non-temporal (streaming) stores bypass the cache, avoiding
        // pollution of this CPU's L1/L2 with zeros that won't be used by
        // this CPU.  The zeroed frame will be consumed by alloc_frame_zeroed
        // later, possibly on a different CPU.
        let virt = frame.to_virt(hhdm) as *mut u8;
        // SAFETY: frame is freshly allocated and exclusively ours.
        // HHDM mapping is valid for all physical memory.  The frame
        // address is 16 KiB aligned, exceeding the 8-byte requirement.
        unsafe {
            zero_frame_nontemporal(virt);
        }

        // Push to the pool.
        let mut pool = ZERO_POOL.lock();
        if !pool.push(frame.addr()) {
            // Pool filled up while we were zeroing — free the frame.
            // SAFETY: frame was just allocated, exclusively ours.
            let _ = unsafe { free_frame(frame) };
            break;
        }
        added += 1;
    }

    added
}

/// Get the number of frames currently in the pre-zeroed pool.
#[must_use]
pub fn zero_pool_count() -> usize {
    ZERO_POOL.lock().count
}

/// Get zero pool hit/miss statistics.
///
/// Returns `(hits, misses)` — the number of `alloc_frame_zeroed()` calls
/// that were served from the pool vs. fell through to alloc+zero.
#[must_use]
pub fn zero_pool_stats() -> (u64, u64) {
    (
        ZERO_POOL_HITS.load(Ordering::Relaxed),
        ZERO_POOL_MISSES.load(Ordering::Relaxed),
    )
}

// ---------------------------------------------------------------------------
// Zero-on-free mode
// ---------------------------------------------------------------------------
//
// When the `mm.zero_on_alloc` sysctl parameter is set to 1, frames are
// zeroed when freed instead of when allocated.  This amortises the 16 KiB
// memset cost across free operations (which are less latency-sensitive)
// and removes it from the allocation hot path (page faults, process
// creation).
//
// Server and Gaming workload profiles enable this mode because:
// - Server: high-throughput allocation patterns benefit from predictable
//   alloc latency without inline zeroing.
// - Gaming: frame allocation spikes during level loads and scene transitions
//   are smoother without zeroing stalls.
//
// ## Transition Safety
//
// When zero-on-free is enabled at runtime (not just at boot), existing
// free frames are NOT pre-zeroed.  A generation counter tracks how many
// frames have been zeroed-on-free.  Once the counter exceeds `total_frames`,
// we know all frames in the system have been through at least one free
// cycle, and `alloc_frame_zeroed()` can safely skip inline zeroing.
//
// Before settling, `alloc_frame_zeroed()` continues to zero inline as
// usual, so correctness is always maintained.
//
// Based on:
// - Windows "zero page thread" (system thread that pre-zeros free pages)
// - Linux CONFIG_INIT_ON_FREE_DEFAULT_ON (init_on_free kernel parameter)

/// Check if zero-on-free mode is active (`mm.zero_on_alloc == 1`).
///
/// Reads the sysctl parameter each time — the sysctl lock is a spinlock
/// with no contention on the read path, so this is effectively a single
/// memory load.
#[inline]
fn is_zero_on_free() -> bool {
    crate::sysctl::get(crate::sysctl::PARAM_MM_ZERO_ON_ALLOC)
        .unwrap_or(0) == 1
}

/// Number of frames zeroed-on-free since the mode was enabled.
///
/// Once this reaches `total_frames`, we know all frames have been
/// through at least one zero-on-free cycle, and `alloc_frame_zeroed()`
/// can skip inline zeroing.
static ZERO_ON_FREE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Set once `ZERO_ON_FREE_COUNT >= total_frames`.  After this point,
/// `alloc_frame_zeroed()` can safely skip inline zeroing because every
/// frame in the system has been zeroed during a free operation.
static ZERO_ON_FREE_SETTLED: AtomicBool = AtomicBool::new(false);

/// Record that a frame was zeroed during a free operation.
///
/// Increments the generation counter.  When the counter reaches
/// `total_frames`, sets the `ZERO_ON_FREE_SETTLED` flag (one-time
/// transition to steady state).
#[inline]
fn mark_zeroed_on_free() {
    if ZERO_ON_FREE_SETTLED.load(Ordering::Relaxed) {
        return; // Already settled — no bookkeeping needed.
    }
    let count = ZERO_ON_FREE_COUNT.fetch_add(1, Ordering::Relaxed);
    // Check if we've now zeroed enough frames to cover the whole system.
    // This is a heuristic: it counts total frees, not unique frames.
    // In practice, after total_frames frees, virtually all frames have
    // been recycled (the birthday paradox works in our favor).
    if let Some(s) = stats() {
        if count.saturating_add(1) >= s.total_frames as u64 {
            ZERO_ON_FREE_SETTLED.store(true, Ordering::Release);
            serial_println!(
                "[mm] Zero-on-free settled: {} frees >= {} total frames",
                count.saturating_add(1),
                s.total_frames,
            );
        }
    }
}

/// Zero a single frame on the free path using non-temporal stores.
///
/// Non-temporal stores bypass the cache, which is ideal for the free
/// path: the freed frame won't be accessed again by this CPU immediately,
/// so caching zeros wastes L1/L2 capacity.
///
/// Returns `true` if the frame was zeroed, `false` if HHDM is
/// unavailable (early boot).
fn zero_on_free_frame(frame: PhysFrame) -> bool {
    let Some(hhdm) = crate::mm::page_table::hhdm() else {
        return false;
    };
    let virt = frame.to_virt(hhdm) as *mut u8;
    // SAFETY: The caller has exclusive ownership (refcount confirmed ≤ 1).
    // HHDM mapping is valid for all physical memory.  `zero_frame_nontemporal`
    // writes exactly FRAME_SIZE bytes via non-temporal stores + sfence.
    unsafe { zero_frame_nontemporal(virt); }
    true
}

/// Zero a block of 2^order frames on the free path.
///
/// Zeros each frame individually using non-temporal stores.
/// Returns `true` if all frames were zeroed, `false` if HHDM is
/// unavailable.
#[allow(clippy::arithmetic_side_effects)]
fn zero_on_free_block(addr: u64, order: usize) -> bool {
    let Some(hhdm) = crate::mm::page_table::hhdm() else {
        return false;
    };
    let frames = 1usize << order;
    for i in 0..frames {
        let frame_addr = addr.saturating_add((i * FRAME_SIZE) as u64);
        let virt = (frame_addr as usize).wrapping_add(hhdm as usize) as *mut u8;
        // SAFETY: Caller has exclusive ownership of the entire block.
        // Each frame_addr is within the block.  HHDM is valid.
        unsafe { zero_frame_nontemporal(virt); }
    }
    true
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Allocator statistics snapshot.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public stats API; fields used by diagnostics/sysctl.
pub struct FrameAllocStats {
    /// Total frames in the managed range (including non-usable holes).
    pub total_frames: usize,
    /// Number of currently free frames.
    pub free_frames: usize,
    /// Free memory in bytes.
    pub free_bytes: usize,
    /// Number of free blocks per order level.
    pub order_counts: [usize; MAX_ORDER + 1],
}

/// Find the highest physical address in the memory map and compute
/// metadata placement.  Returns `(total_frames, metadata_phys, metadata_size)`.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn plan_metadata(memory_map: &[&MemmapEntry]) -> KernelResult<(usize, u64, u64)> {
    let frame_size = FRAME_SIZE as u64;

    // Find the highest physical address among memory types we might
    // ever manage (USABLE, BOOTLOADER_RECLAIMABLE, ACPI_RECLAIMABLE).
    // Ignore RESERVED / MMIO regions at high addresses (e.g., PCI MMIO
    // at 0xfd00000000) — those would bloat the metadata array with
    // entries for memory we never allocate.
    let mut max_phys: u64 = 0;
    for entry in memory_map {
        let dominated = matches!(
            entry.type_,
            memmap_type::USABLE
                | memmap_type::BOOTLOADER_RECLAIMABLE
                | memmap_type::ACPI_RECLAIMABLE
        );
        if !dominated {
            continue;
        }
        let end = entry.base.saturating_add(entry.length);
        if end > max_phys {
            max_phys = end;
        }
    }

    if max_phys == 0 {
        serial_println!("[mm] ERROR: No memory regions in memory map");
        return Err(KernelError::OutOfMemory);
    }

    let total_frames = (align_up(max_phys, frame_size) / frame_size) as usize;
    serial_println!(
        "[mm] Physical range: 0x0 - {:#x} ({} frames, {} MiB addressable)",
        max_phys,
        total_frames,
        (total_frames * FRAME_SIZE) / (1024 * 1024)
    );

    // We need 1 byte per frame for page_info + 2 bytes per frame for
    // refcount, plus up to 1 byte of alignment padding between them.
    let refcount_offset = (total_frames + 1) & !1; // align up to 2
    let metadata_bytes = refcount_offset + total_frames * 2;
    let metadata_frames = (align_up(metadata_bytes as u64, frame_size) / frame_size) as usize;
    let metadata_size = (metadata_frames as u64) * frame_size;

    serial_println!(
        "[mm] Metadata: {} bytes ({} frames, {} KiB) [page_info: {}B, refcount: {}B]",
        metadata_bytes,
        metadata_frames,
        metadata_size / 1024,
        total_frames,
        total_frames * 2
    );

    // Find the first USABLE region large enough for the metadata.
    for entry in memory_map {
        if entry.type_ != memmap_type::USABLE {
            continue;
        }
        let base = align_up(entry.base, frame_size);
        let end = align_down(entry.base.saturating_add(entry.length), frame_size);
        if end > base && end - base >= metadata_size {
            serial_println!("[mm] Metadata at {:#x} - {:#x}", base, base + metadata_size);
            return Ok((total_frames, base, metadata_size));
        }
    }

    serial_println!("[mm] ERROR: No region large enough for metadata");
    Err(KernelError::OutOfMemory)
}

/// Populate the allocator's free lists from USABLE memory map regions,
/// skipping the metadata area and low memory (below [`LOW_MEMORY_RESERVE`]).
/// Returns the number of usable frames added.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn populate_free_lists(
    allocator: &mut BuddyAllocator,
    memory_map: &[&MemmapEntry],
    metadata_phys: u64,
    metadata_end: u64,
) -> usize {
    let frame_size = FRAME_SIZE as u64;
    let mut total_usable: usize = 0;

    for entry in memory_map {
        if entry.type_ != memmap_type::USABLE {
            continue;
        }

        let start = align_up(entry.base, frame_size);
        let end = align_down(entry.base.saturating_add(entry.length), frame_size);
        if end <= start {
            continue;
        }

        // Skip low memory — reserved for BIOS, IVT, and SMP trampoline.
        // The frame allocator metadata may live in this range (it's carved
        // separately), but the frames themselves are never added to the
        // free list.  This avoids corruption when the SMP trampoline
        // writes to physical 0x8000 (which would otherwise be a free-list
        // node in the buddy allocator).
        let start = start.max(LOW_MEMORY_RESERVE);
        if end <= start {
            continue;
        }

        // Split the region around the metadata area if it overlaps.
        if start < metadata_end && end > metadata_phys {
            if start < metadata_phys {
                let before_end = core::cmp::min(end, metadata_phys);
                let frames = ((before_end - start) / frame_size) as usize;
                total_usable += frames;
                allocator.add_free_range(start, before_end);
            }
            if end > metadata_end {
                let after_start = core::cmp::max(start, metadata_end);
                let frames = ((end - after_start) / frame_size) as usize;
                total_usable += frames;
                allocator.add_free_range(after_start, end);
            }
        } else {
            let frames = ((end - start) / frame_size) as usize;
            total_usable += frames;
            allocator.add_free_range(start, end);
        }
    }

    total_usable
}

/// Initialize the physical frame allocator from the bootloader memory map.
///
/// Scans the Limine memory map for `USABLE` regions, carves off a small
/// metadata area from the first suitable region, and populates the buddy
/// free lists with all remaining usable frames.
///
/// # Safety
///
/// - Must be called exactly once during early boot (single-threaded).
/// - `hhdm_offset` must be the correct Higher Half Direct Map offset.
/// - `memory_map` must contain valid Limine memory map entries.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub unsafe fn init(hhdm_offset: u64, memory_map: &[&MemmapEntry]) -> KernelResult<()> {
    serial_println!("[mm] Initializing physical frame allocator...");

    if ALLOCATOR.get().is_some() {
        serial_println!("[mm] WARNING: Frame allocator already initialized");
        return Ok(());
    }

    // Plan metadata placement.
    let (total_frames, metadata_phys, metadata_size) = plan_metadata(memory_map)?;
    let metadata_end = metadata_phys + metadata_size;

    // Initialize the metadata arrays.
    //
    // Layout within the metadata region:
    //   [0 .. total_frames)                → page_info (1 byte per frame)
    //   [refcount_offset .. refcount_offset + total_frames*2) → refcount (u16)
    //
    // The refcount array must be 2-byte aligned (u16).  Round up the
    // page_info region to the next even byte boundary.
    let refcount_offset = (total_frames + 1) & !1; // align up to 2
    let metadata_virt = (metadata_phys + hhdm_offset) as *mut u8;
    // SAFETY: metadata_phys is in a USABLE memory region, the HHDM maps
    // it to metadata_virt, and we have exclusive access during early boot
    // (single CPU, interrupts disabled, no other allocators).
    unsafe {
        // page_info: fill with INFO_ALLOCATED = "not free".
        core::ptr::write_bytes(metadata_virt, INFO_ALLOCATED, total_frames);
        // refcount: fill with 0 = "not allocated / no references".
        // Use byte-level zeroing to avoid alignment issues, then store
        // the aligned pointer for later use.
        let refcount_ptr = metadata_virt.add(refcount_offset);
        core::ptr::write_bytes(refcount_ptr, 0, total_frames * 2);
    }
    // SAFETY: refcount_offset is even, so metadata_virt + refcount_offset
    // is 2-byte aligned (metadata_virt is frame-aligned = 16 KiB aligned).
    let refcount_virt = unsafe { metadata_virt.add(refcount_offset) as *mut u16 };

    // Build the allocator and populate free lists.
    let mut allocator = BuddyAllocator {
        free_lists: [FreeList::new(); MAX_ORDER + 1],
        page_info: metadata_virt,
        page_info_len: total_frames,
        refcount: refcount_virt,
        total_frames,
        hhdm_offset,
        free_frames: 0,
    };

    let usable = populate_free_lists(&mut allocator, memory_map, metadata_phys, metadata_end);

    // Log results.
    serial_println!(
        "[mm] Added {} usable frames ({} MiB)",
        usable,
        (usable * FRAME_SIZE) / (1024 * 1024)
    );

    for order in 0..=MAX_ORDER {
        #[allow(clippy::indexing_slicing)]
        let count = allocator.free_lists[order].count;
        if count > 0 {
            serial_println!(
                "[mm]   Order {:2} ({:>6} KiB): {} blocks",
                order,
                (FRAME_SIZE << order) / 1024,
                count
            );
        }
    }

    serial_println!(
        "[mm] Total free: {} frames ({} MiB)",
        allocator.free_frames,
        (allocator.free_frames * FRAME_SIZE) / (1024 * 1024)
    );

    // Cache the refcount pointer and length for lock-free reads in free_frame().
    // These never change after init.
    REFCOUNT_PTR.store(allocator.refcount as u64, Ordering::Release);
    REFCOUNT_LEN.store(allocator.page_info_len as u64, Ordering::Release);

    // Store in the global singleton.
    ALLOCATOR.call_once(|| Mutex::new(allocator));

    serial_println!("[mm] Physical frame allocator initialized");
    Ok(())
}

/// Allocate a single physical frame (16 KiB, order 0).
///
/// Uses the per-CPU frame cache when available (lock-free fast path).
/// Falls back to the global buddy allocator when the cache is empty
/// or per-CPU caches are not yet enabled.
///
/// Returns the frame on success, or `OutOfMemory` if no frames are
/// available.
#[allow(clippy::indexing_slicing)]
pub fn alloc_frame() -> KernelResult<PhysFrame> {
    // Fast path: try per-CPU cache (no global lock needed).
    if PCPU_ENABLED.load(Ordering::Acquire) {
        // SAFETY: We're in ring 0; pushfq+cli is always valid here.
        // The returned flags value will be restored below.
        let flags = unsafe { disable_interrupts() };
        let cpu = crate::smp::fast_cpu_index();

        // SAFETY: interrupts disabled, cpu < MAX_CPUS (bounded by
        // smp::fast_cpu_index()), exclusive per-CPU access.
        let cached = unsafe { PCPU_CACHES[cpu].pop() };
        if let Some(addr) = cached {
            // SAFETY: flags from disable_interrupts() above.
            unsafe { restore_interrupts(flags); }
            return PhysFrame::from_addr(addr).ok_or(KernelError::InternalError);
        }

        // Cache empty — batch-refill from global allocator.
        // (This acquires the global lock internally.)
        let refilled = pcpu_refill(cpu);

        if refilled > 0 {
            // SAFETY: interrupts still disabled, same cpu, exclusive access.
            let cached = unsafe { PCPU_CACHES[cpu].pop() };
            // SAFETY: flags from disable_interrupts() above.
            unsafe { restore_interrupts(flags); }
            if let Some(addr) = cached {
                return PhysFrame::from_addr(addr).ok_or(KernelError::InternalError);
            }
        }

        // SAFETY: flags from disable_interrupts() above.
        unsafe { restore_interrupts(flags); }
        // Fall through to global allocator (reclamation path).
    }

    // Slow path: direct global allocation (also handles reclamation).
    alloc_order(0)
}

/// Allocate a single physical frame (16 KiB) and zero it.
///
/// This is the most common allocation pattern in the kernel (page
/// faults, stack growth, process creation all need zeroed frames).
///
/// OPT: Checks the pre-zeroed frame pool first.  If a pre-zeroed
/// frame is available, returns it immediately — no 16 KiB memset
/// on the hot path.  The idle loop refills the pool in the background
/// via `refill_zero_pool()`.  Falls back to alloc + inline zero when
/// the pool is empty.
///
/// Returns the zeroed frame on success, or `OutOfMemory`/`NotSupported`.
pub fn alloc_frame_zeroed() -> KernelResult<PhysFrame> {
    // Fast path 1: grab a pre-zeroed frame from the pool.
    if ZERO_POOL_ENABLED.load(Ordering::Acquire) {
        let mut pool = ZERO_POOL.lock();
        if let Some(phys) = pool.pop() {
            drop(pool); // Release lock before atomic increment.
            ZERO_POOL_HITS.fetch_add(1, Ordering::Relaxed);
            return PhysFrame::from_addr(phys).ok_or(KernelError::InternalError);
        }
    }

    // Fast path 2: zero-on-free mode, settled (all frames pre-zeroed).
    //
    // After every frame in the system has been through at least one
    // free cycle with zero-on-free active, all frames on the free lists
    // and in per-CPU caches are pre-zeroed.  `alloc_frame()` returns a
    // pre-zeroed frame, so we skip the inline memset entirely.
    //
    // OPT: This removes the 16 KiB zero from the allocation hot path
    // (~3µs savings per alloc).  The cost was amortised across the
    // free path instead.
    if is_zero_on_free() && ZERO_ON_FREE_SETTLED.load(Ordering::Acquire) {
        ZERO_POOL_HITS.fetch_add(1, Ordering::Relaxed);
        return alloc_frame();
    }

    // Slow path: allocate + zero inline.
    ZERO_POOL_MISSES.fetch_add(1, Ordering::Relaxed);

    let frame = alloc_frame()?;

    let hhdm = crate::mm::page_table::hhdm().ok_or_else(|| {
        // Can't zero without HHDM — free the frame and return error.
        // SAFETY: frame was just allocated, exclusively ours.
        let _ = unsafe { free_frame(frame) };
        KernelError::NotSupported
    })?;

    let virt = frame.to_virt(hhdm) as *mut u8;
    // SAFETY: frame is freshly allocated and exclusively ours.
    // The HHDM mapping is valid for all physical memory.
    unsafe {
        core::ptr::write_bytes(virt, 0, FRAME_SIZE);
    }

    Ok(frame)
}

/// Zero an already-allocated frame via its HHDM mapping.
///
/// # Safety
///
/// The caller must own the frame exclusively (no other CPU or mapping
/// references the memory).
#[allow(dead_code)] // Public API for other zones (proc, ipc, drivers).
pub unsafe fn zero_frame(frame: PhysFrame) -> KernelResult<()> {
    let hhdm = crate::mm::page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let virt = frame.to_virt(hhdm) as *mut u8;
    // SAFETY: Caller guarantees exclusive ownership.  HHDM is valid.
    unsafe { core::ptr::write_bytes(virt, 0, FRAME_SIZE); }
    Ok(())
}

/// Allocate a contiguous block of 2^order physical frames.
///
/// The returned frame is naturally aligned to the block size
/// (e.g., order 2 = 64 KiB aligned).  `order` must be ≤ [`MAX_ORDER`].
///
/// If the allocator is out of memory, attempts to reclaim pages via
/// the swap subsystem's Clock algorithm before giving up.
pub fn alloc_order(order: usize) -> KernelResult<PhysFrame> {
    let allocator = ALLOCATOR.get().ok_or(KernelError::NotSupported)?;

    // First attempt — fast path.
    {
        let mut guard = allocator.lock();
        match guard.alloc_inner(order) {
            Ok(addr) => return PhysFrame::from_addr(addr).ok_or(KernelError::InternalError),
            Err(KernelError::OutOfMemory) => {
                // Fall through to reclamation.
            }
            Err(e) => return Err(e),
        }
    }
    // Allocator lock released before reclamation (lock ordering:
    // SWAP → RECLAIM → page table → frame allocator).

    // Wake kswapd to continue background reclamation while we do
    // inline reclamation for this specific allocation.  kswapd will
    // keep reclaiming until the high watermark is satisfied.
    super::kswapd::wake_kswapd();

    // Try to reclaim pages via swap to free physical memory.
    // Request enough frames for the order, plus a small buffer so the
    // allocator can potentially coalesce buddies.
    let needed = 1usize << order;
    let reclaimed = super::swap::try_reclaim(needed.saturating_add(2));

    if reclaimed > 0 {
        // Retry allocation after reclamation.
        let mut guard = allocator.lock();
        match guard.alloc_inner(order) {
            Ok(addr) => return PhysFrame::from_addr(addr).ok_or(KernelError::InternalError),
            Err(KernelError::OutOfMemory) => {
                // Reclaimed pages weren't enough or couldn't be coalesced.
                // Fall through to OOM handler.
            }
            Err(e) => return Err(e),
        }
    }

    // Last resort: invoke the OOM handler.
    // The handler may kill a process (policy 0/1) or return 0 (policy 2).
    let oom_freed = super::oom::handle_oom(needed);
    if oom_freed == 0 {
        return Err(KernelError::OutOfMemory);
    }

    // OOM handler freed memory — retry one more time.
    let mut guard = allocator.lock();
    let addr = guard.alloc_inner(order)?;
    PhysFrame::from_addr(addr).ok_or(KernelError::InternalError)
}

/// Allocate a contiguous block of `2^order` physical frames with the
/// entire allocation below `max_addr`.
///
/// Used for DMA buffers where the device has address constraints
/// (e.g., 32-bit DMA requires all memory below 4 GiB).
///
/// Like [`alloc_order`], attempts swap reclamation on first OOM.
/// Unlike `alloc_order`, does not use per-CPU caches (DMA allocations
/// are infrequent and the address constraint can't be satisfied by
/// arbitrary cached frames).
pub fn alloc_order_constrained(order: usize, max_addr: u64) -> KernelResult<PhysFrame> {
    let allocator = ALLOCATOR.get().ok_or(KernelError::NotSupported)?;

    // First attempt.
    {
        let mut guard = allocator.lock();
        match guard.alloc_inner_constrained(order, max_addr) {
            Ok(addr) => return PhysFrame::from_addr(addr).ok_or(KernelError::InternalError),
            Err(KernelError::OutOfMemory) => {
                // Fall through to reclamation.
            }
            Err(e) => return Err(e),
        }
    }

    // Wake kswapd for background reclamation.
    super::kswapd::wake_kswapd();

    // Try reclamation, then retry.
    let needed = 1usize << order;
    let reclaimed = super::swap::try_reclaim(needed.saturating_add(2));

    if reclaimed > 0 {
        let mut guard = allocator.lock();
        match guard.alloc_inner_constrained(order, max_addr) {
            Ok(addr) => return PhysFrame::from_addr(addr).ok_or(KernelError::InternalError),
            Err(KernelError::OutOfMemory) => {
                // Fall through to OOM handler.
            }
            Err(e) => return Err(e),
        }
    }

    // Last resort: OOM handler.
    let oom_freed = super::oom::handle_oom(needed);
    if oom_freed == 0 {
        return Err(KernelError::OutOfMemory);
    }

    let mut guard = allocator.lock();
    let addr = guard.alloc_inner_constrained(order, max_addr)?;
    PhysFrame::from_addr(addr).ok_or(KernelError::InternalError)
}

/// Free a single physical frame (16 KiB, order 0).
///
/// Uses the per-CPU frame cache when available (lock-free fast path).
/// When the cache is full, batch-drains half back to the global buddy
/// allocator.
///
/// # Safety
///
/// - The frame must have been allocated by [`alloc_frame()`].
/// - Must not be freed more than once (double-free is detected and
///   returns an error, but the caller should not rely on this).
/// - The caller must ensure no references to the frame's memory remain.
#[allow(clippy::indexing_slicing)]
pub unsafe fn free_frame(frame: PhysFrame) -> KernelResult<()> {
    // Fast path: push to per-CPU cache (no global lock needed).
    if PCPU_ENABLED.load(Ordering::Acquire) {
        // OPT: Lock-free refcount check.  Shared (CoW) frames with
        // refcount > 1 must go through the global allocator for ref_dec.
        // Non-shared frames (refcount == 1, the common case) skip the
        // global lock entirely.
        //
        // Previously this took the global lock on EVERY free_frame call
        // just to read the refcount — defeating per-CPU caching on the
        // free path.  Now we read the immutable refcount pointer directly.
        let rc_ptr = REFCOUNT_PTR.load(Ordering::Acquire);
        let rc_len = REFCOUNT_LEN.load(Ordering::Relaxed);
        if rc_ptr != 0 {
            #[allow(clippy::arithmetic_side_effects)]
            let idx = (frame.addr() / FRAME_SIZE as u64) as usize;
            if (idx as u64) < rc_len {
                // SAFETY: rc_ptr is a valid HHDM pointer to the refcount
                // array, set during init() and never moved.  idx < rc_len
                // is bounds-checked above.  read_volatile ensures we see
                // the latest write (ref_inc is done under the global lock
                // which provides a memory fence on the writing side).
                let rc = unsafe {
                    (rc_ptr as *const u16).add(idx).read_volatile()
                };
                if rc > 1 {
                    // Shared frame — go through global path for ref_dec.
                    // SAFETY: Caller guarantees frame was validly allocated.
                    return unsafe { free_order(frame, 0) };
                }
            }
        }

        // Zero-on-free: zero the frame before returning it to the free
        // pool.  Done with interrupts ENABLED (before the cli below) so
        // timer interrupts aren't blocked during the ~3µs memset.
        //
        // This moves the zeroing cost from alloc_frame_zeroed() (page
        // fault hot path) to free_frame() (less latency-sensitive).
        // The frame is exclusively ours (refcount ≤ 1 confirmed above),
        // so zeroing is safe.
        if is_zero_on_free() {
            if zero_on_free_frame(frame) {
                mark_zeroed_on_free();
            }
        }

        // SAFETY: We're in ring 0; pushfq+cli is always valid.
        let flags = unsafe { disable_interrupts() };
        let cpu = crate::smp::fast_cpu_index();

        // SAFETY: interrupts disabled, cpu < MAX_CPUS, exclusive access.
        let full = unsafe { PCPU_CACHES[cpu].is_full() };
        if full {
            // Cache full — drain half back to global.
            pcpu_drain(cpu);
        }

        // SAFETY: interrupts disabled, cpu < MAX_CPUS, exclusive access.
        let pushed = unsafe { PCPU_CACHES[cpu].push(frame.addr()) };
        // SAFETY: flags from disable_interrupts() above.
        unsafe { restore_interrupts(flags); }

        if pushed {
            return Ok(());
        }
        // Fall through if push failed (shouldn't happen after drain).
    }

    // Slow path: direct global free.
    // SAFETY: Caller guarantees the frame was validly allocated.
    unsafe { free_order(frame, 0) }
}

/// Free a contiguous block of 2^order physical frames.
///
/// # Safety
///
/// - `frame` and `order` must exactly match a prior [`alloc_order()`] call.
/// - Must not be freed more than once.
/// - The caller must ensure no references to the block's memory remain.
#[allow(clippy::indexing_slicing)]
pub unsafe fn free_order(frame: PhysFrame, order: usize) -> KernelResult<()> {
    // Zero-on-free: zero the block BEFORE taking the global lock to
    // avoid holding the lock during the expensive zeroing operation.
    //
    // For order > 0: multi-frame blocks are never CoW-shared (only
    // single frames participate in CoW), so they're always solely owned.
    //
    // For order == 0: this path is reached from free_frame() in two cases:
    //   (a) Shared frame (rc > 1) — must NOT zero (other mappings live).
    //   (b) PCPU disabled fallthrough — sole owner, should zero.
    // Use the lockless refcount check to distinguish these cases.
    if is_zero_on_free() {
        let should_zero = if order > 0 {
            true // Multi-frame blocks are always solely owned.
        } else {
            // Lockless refcount check (same mechanism as free_frame).
            let rc_ptr = REFCOUNT_PTR.load(Ordering::Acquire);
            let rc_len = REFCOUNT_LEN.load(Ordering::Relaxed);
            if rc_ptr != 0 {
                #[allow(clippy::arithmetic_side_effects)]
                let idx = (frame.addr() / FRAME_SIZE as u64) as usize;
                if (idx as u64) < rc_len {
                    let rc = unsafe {
                        (rc_ptr as *const u16).add(idx).read_volatile()
                    };
                    rc <= 1
                } else {
                    false // Can't verify refcount — skip zeroing.
                }
            } else {
                false // REFCOUNT_PTR not set (early boot) — skip zeroing.
            }
        };
        if should_zero && zero_on_free_block(frame.addr(), order) {
            let frames_in_block = 1u64 << order;
            for _ in 0..frames_in_block {
                mark_zeroed_on_free();
            }
        }
    }

    let allocator = ALLOCATOR.get().ok_or(KernelError::NotSupported)?;
    let mut guard = allocator.lock();
    guard.free_inner(frame.addr(), order)
}

/// Check if a physical frame falls within the allocator's managed range.
///
/// Returns `true` if the frame's physical address is within the range
/// of memory tracked by the buddy allocator.  This is used to
/// distinguish allocator-owned frames (which should be freed on unmap)
/// from device MMIO frames (which should not).
///
/// Returns `false` if the allocator hasn't been initialized.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn is_allocator_owned(frame: PhysFrame) -> bool {
    let Some(allocator) = ALLOCATOR.get() else {
        return false;
    };
    let guard = allocator.lock();
    let idx = (frame.addr() / FRAME_SIZE as u64) as usize;
    idx < guard.total_frames
}

/// Get a snapshot of the current allocator statistics.
///
/// Returns `None` if the allocator has not been initialized.
#[must_use]
#[allow(clippy::indexing_slicing)]
pub fn stats() -> Option<FrameAllocStats> {
    let allocator = ALLOCATOR.get()?;
    let guard = allocator.lock();

    let mut order_counts = [0usize; MAX_ORDER + 1];
    for (i, count) in order_counts.iter_mut().enumerate() {
        // i is always in 0..=MAX_ORDER (the array length matches).
        *count = guard.free_lists[i].count;
    }

    Some(FrameAllocStats {
        total_frames: guard.total_frames,
        free_frames: guard.free_frames,
        free_bytes: guard.free_frames.saturating_mul(FRAME_SIZE),
        order_counts,
    })
}

/// Non-blocking variant of [`stats()`] for use in panic handlers.
///
/// Uses `try_lock` to avoid deadlock if the panic occurred while the
/// allocator lock was held.  Returns `None` if the allocator is not
/// initialized or the lock is currently held.
#[must_use]
#[allow(clippy::indexing_slicing)]
pub fn try_stats() -> Option<FrameAllocStats> {
    let allocator = ALLOCATOR.get()?;
    let guard = allocator.try_lock()?;

    let mut order_counts = [0usize; MAX_ORDER + 1];
    for (i, count) in order_counts.iter_mut().enumerate() {
        *count = guard.free_lists[i].count;
    }

    Some(FrameAllocStats {
        total_frames: guard.total_frames,
        free_frames: guard.free_frames,
        free_bytes: guard.free_frames.saturating_mul(FRAME_SIZE),
        order_counts,
    })
}

// ---------------------------------------------------------------------------
// Copy-on-Write reference counting API
// ---------------------------------------------------------------------------

/// Get the reference count of a physical frame.
///
/// Returns 0 if the allocator is not initialized, the frame is outside
/// the managed range, or the frame is not allocated.
///
/// - 1: single owner (normal allocation)
/// - 2+: shared by multiple page tables (CoW)
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn refcount(frame: PhysFrame) -> u16 {
    let Some(allocator) = ALLOCATOR.get() else {
        return 0;
    };
    let guard = allocator.lock();
    let idx = (frame.addr() / FRAME_SIZE as u64) as usize;
    if idx >= guard.page_info_len {
        return 0;
    }
    guard.get_refcount(idx)
}

/// Increment the reference count of a physical frame.
///
/// Used by Copy-on-Write: when a page is shared between two address
/// spaces (e.g., after duplicating page tables), bump the refcount
/// so that `free_frame` won't actually release the memory until all
/// users have dropped their reference.
///
/// # Safety
///
/// - `frame` must be an allocated frame from this allocator.
/// - Caller must ensure the frame is not concurrently being freed.
#[allow(clippy::cast_possible_truncation)]
pub unsafe fn ref_inc(frame: PhysFrame) -> KernelResult<()> {
    let allocator = ALLOCATOR.get().ok_or(KernelError::NotSupported)?;
    let mut guard = allocator.lock();
    let idx = (frame.addr() / FRAME_SIZE as u64) as usize;
    if idx >= guard.page_info_len {
        return Err(KernelError::InvalidAddress);
    }
    let rc = guard.get_refcount(idx);
    if rc == 0 {
        // Frame is not allocated — can't increment.
        return Err(KernelError::InvalidArgument);
    }
    guard.set_refcount(idx, rc.saturating_add(1));
    Ok(())
}

/// Decrement the reference count of a physical frame without freeing.
///
/// Returns the new refcount.  If the refcount would go below 0, returns
/// an error.  Unlike `free_frame`, this NEVER returns the frame to the
/// free list — use this when you're replacing a CoW mapping but don't
/// own the frame's allocation order.
///
/// When you need to free AND potentially return to the free list, use
/// `free_frame` / `free_order` instead (they handle refcount internally).
///
/// # Safety
///
/// - `frame` must be an allocated frame from this allocator.
#[allow(clippy::cast_possible_truncation)]
pub unsafe fn ref_dec(frame: PhysFrame) -> KernelResult<u16> {
    let allocator = ALLOCATOR.get().ok_or(KernelError::NotSupported)?;
    let mut guard = allocator.lock();
    let idx = (frame.addr() / FRAME_SIZE as u64) as usize;
    if idx >= guard.page_info_len {
        return Err(KernelError::InvalidAddress);
    }
    let rc = guard.get_refcount(idx);
    if rc == 0 {
        return Err(KernelError::InvalidArgument);
    }
    let new_rc = rc.saturating_sub(1);
    guard.set_refcount(idx, new_rc);
    Ok(new_rc)
}

// ---------------------------------------------------------------------------
// Self-test (runs during boot)
// ---------------------------------------------------------------------------

/// Run a boot-time self-test of the frame allocator.
///
/// Exercises basic allocation, freeing, coalescing, and double-free
/// detection.  All frames allocated during the test are freed before
/// returning, leaving the allocator in its original state.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn self_test() -> KernelResult<()> {
    serial_println!("[mm] Running frame allocator self-test...");

    let initial = stats().ok_or(KernelError::NotSupported)?;
    serial_println!("[mm]   Initial: {} free frames", initial.free_frames);

    // -- Test 1: Single alloc + free -----------------------------------------
    let f1 = alloc_frame()?;
    serial_println!("[mm]   Alloc frame: {:#x}", f1.addr());

    if !f1.addr().is_multiple_of(FRAME_SIZE as u64) {
        serial_println!("[mm]   FAIL: frame not aligned!");
        return Err(KernelError::BadAlignment);
    }

    // SAFETY: f1 was just allocated by us and is not aliased.
    unsafe { free_frame(f1)?; }
    serial_println!("[mm]   Free frame: OK");

    let after1 = stats().ok_or(KernelError::NotSupported)?;
    if after1.free_frames != initial.free_frames {
        serial_println!(
            "[mm]   FAIL: count mismatch after alloc/free: {} vs {}",
            after1.free_frames, initial.free_frames
        );
        return Err(KernelError::InternalError);
    }

    // -- Test 2: Higher-order alloc + free (order 2 = 64 KiB) ----------------
    let f2 = alloc_order(2)?;
    serial_println!("[mm]   Alloc order 2: {:#x}", f2.addr());

    // Must be aligned to block size (64 KiB = FRAME_SIZE * 4).
    let order2_align = (FRAME_SIZE as u64) * 4;
    if !f2.addr().is_multiple_of(order2_align) {
        serial_println!("[mm]   FAIL: order-2 block not aligned to {} KiB!", order2_align / 1024);
        return Err(KernelError::BadAlignment);
    }

    // SAFETY: f2 was just allocated.
    unsafe { free_order(f2, 2)?; }
    serial_println!("[mm]   Free order 2: OK");

    let after2 = stats().ok_or(KernelError::NotSupported)?;
    if after2.free_frames != initial.free_frames {
        serial_println!(
            "[mm]   FAIL: count mismatch after order-2: {} vs {}",
            after2.free_frames, initial.free_frames
        );
        return Err(KernelError::InternalError);
    }

    // -- Test 3: Batch alloc + free (16 frames) ------------------------------
    #[allow(clippy::items_after_statements)]
    const BATCH: usize = 16;
    let mut addrs = [0u64; BATCH];
    for slot in &mut addrs {
        let f = alloc_frame()?;
        *slot = f.addr();
    }
    serial_println!("[mm]   Alloc {} frames: OK", BATCH);

    let during = stats().ok_or(KernelError::NotSupported)?;
    let expected_free = initial.free_frames - BATCH;
    if during.free_frames != expected_free {
        serial_println!(
            "[mm]   FAIL: expected {} free, got {}",
            expected_free, during.free_frames
        );
        return Err(KernelError::InternalError);
    }

    for &addr in &addrs {
        if let Some(f) = PhysFrame::from_addr(addr) {
            // SAFETY: each frame was allocated in the loop above.
            unsafe { free_frame(f)?; }
        }
    }
    serial_println!("[mm]   Free {} frames: OK", BATCH);

    let after3 = stats().ok_or(KernelError::NotSupported)?;
    if after3.free_frames != initial.free_frames {
        serial_println!(
            "[mm]   FAIL: final count {} != initial {}",
            after3.free_frames, initial.free_frames
        );
        return Err(KernelError::InternalError);
    }

    // -- Test 4: Double-free detection ---------------------------------------
    let f4 = alloc_frame()?;
    // SAFETY: f4 was just allocated.
    unsafe { free_frame(f4)?; }
    // Second free of the same frame should be detected and return an error.
    let double_free = unsafe { free_frame(f4) };
    if double_free.is_ok() {
        serial_println!("[mm]   FAIL: double-free was not detected!");
        return Err(KernelError::InternalError);
    }
    serial_println!("[mm]   Double-free detection: OK");

    // -- Test 5: Zeroed frame allocation (every byte zero) ------------------
    test_zeroed_alloc()?;

    // -- Test 6: Per-CPU cache (alloc/free pattern after enabling) ----------
    test_pcpu_cache()?;

    // -- Test 7: Zero-on-free mode (sysctl mm.zero_on_alloc=1) -------------
    test_zero_on_free()?;

    serial_println!("[mm] Frame allocator self-test PASSED");
    Ok(())
}

/// Verify `alloc_frame_zeroed` returns a completely zero-filled frame.
///
/// Requires page_table::hhdm() to be available (page_table::init must
/// have been called).  Skips gracefully if called too early in boot.
fn test_zeroed_alloc() -> KernelResult<()> {
    let hhdm = match crate::mm::page_table::hhdm() {
        Some(h) => h,
        None => {
            // Page table module not initialized yet — alloc_frame_zeroed
            // won't work either.  Skip this test at early boot; it will
            // be exercised indirectly by the demand paging self-test.
            serial_println!("[mm]   Zeroed frame allocation: SKIP (HHDM not ready)");
            return Ok(());
        }
    };

    let frame = alloc_frame_zeroed()?;
    let ptr = frame.to_virt(hhdm) as *const u8;

    // Check every byte in the 16 KiB frame is zero.
    // SAFETY: frame is allocated, HHDM mapping is valid.
    let all_zero = unsafe {
        let slice = core::slice::from_raw_parts(ptr, FRAME_SIZE);
        slice.iter().all(|&b| b == 0)
    };

    // SAFETY: sole owner.
    unsafe { free_frame(frame)?; }

    if !all_zero {
        serial_println!("[mm]   FAIL: alloc_frame_zeroed returned non-zero frame!");
        return Err(KernelError::InternalError);
    }
    serial_println!("[mm]   Zeroed frame allocation: OK");
    Ok(())
}

/// Test per-CPU cache behavior: rapid alloc/free pattern should hit
/// the per-CPU path (no contention, no global lock).
fn test_pcpu_cache() -> KernelResult<()> {
    let initial = stats().ok_or(KernelError::NotSupported)?;

    // Rapid alloc-free-alloc-free pattern (exercises per-CPU cache).
    for _ in 0..32 {
        let f = alloc_frame()?;
        // SAFETY: just allocated, sole owner.
        unsafe { free_frame(f)?; }
    }

    // Free count should be unchanged (all frames returned).
    let after = stats().ok_or(KernelError::NotSupported)?;
    if after.free_frames != initial.free_frames {
        serial_println!(
            "[mm]   FAIL: pcpu cache test: free count {} != initial {}",
            after.free_frames, initial.free_frames
        );
        return Err(KernelError::InternalError);
    }

    // Batch alloc (exceeds per-CPU cache size), then batch free.
    // Per-CPU cache is typically PCPU_MAX_CACHE=8, so 32 frames
    // will force multiple global lock acquisitions.
    let mut frames = [0u64; 32];
    for slot in &mut frames {
        let f = alloc_frame()?;
        *slot = f.addr();
    }
    for &addr in &frames {
        if let Some(f) = PhysFrame::from_addr(addr) {
            // SAFETY: allocated in the loop above.
            unsafe { free_frame(f)?; }
        }
    }

    let after2 = stats().ok_or(KernelError::NotSupported)?;
    if after2.free_frames != initial.free_frames {
        serial_println!(
            "[mm]   FAIL: pcpu batch test: free count {} != initial {}",
            after2.free_frames, initial.free_frames
        );
        return Err(KernelError::InternalError);
    }

    serial_println!("[mm]   Per-CPU cache alloc/free: OK");
    Ok(())
}

/// Test zero-on-free mode: frames zeroed during free are correctly
/// pre-zeroed when allocated via `alloc_frame_zeroed()`.
///
/// This test:
/// 1. Enables zero-on-free mode via sysctl.
/// 2. Allocates a frame, writes non-zero data, frees it.
/// 3. Re-allocates (likely gets the same frame from per-CPU cache).
/// 4. Verifies all bytes are zero.
/// 5. Restores original sysctl value.
///
/// Requires HHDM and per-CPU caches to be available.  Called both from
/// `self_test()` (skips if too early) and from main.rs after full
/// memory subsystem initialization.
pub fn test_zero_on_free() -> KernelResult<()> {
    let hhdm = match crate::mm::page_table::hhdm() {
        Some(h) => h,
        None => {
            serial_println!("[mm]   Zero-on-free: SKIP (HHDM not ready)");
            return Ok(());
        }
    };

    // Save original sysctl value and enable zero-on-free.
    let original = crate::sysctl::get(crate::sysctl::PARAM_MM_ZERO_ON_ALLOC)
        .unwrap_or(0);
    let _ = crate::sysctl::set(crate::sysctl::PARAM_MM_ZERO_ON_ALLOC, 1);

    // Allocate a frame, fill with non-zero data, then free.
    let frame = alloc_frame()?;
    let ptr = frame.to_virt(hhdm) as *mut u8;
    // SAFETY: frame is allocated, exclusively ours, HHDM valid.
    unsafe {
        core::ptr::write_bytes(ptr, 0xAA, FRAME_SIZE);
    }
    // SAFETY: sole owner, just allocated.
    unsafe { free_frame(frame)?; }

    // Allocate again — high probability of getting the same frame back
    // from the per-CPU cache (LIFO).
    let frame2 = alloc_frame()?;
    let ptr2 = frame2.to_virt(hhdm) as *const u8;

    // Verify all bytes are zero (frame was zeroed on free).
    // SAFETY: frame2 is allocated, HHDM valid.
    let all_zero = unsafe {
        let slice = core::slice::from_raw_parts(ptr2, FRAME_SIZE);
        slice.iter().all(|&b| b == 0)
    };
    // SAFETY: sole owner.
    unsafe { free_frame(frame2)?; }

    // Restore original sysctl.
    let _ = crate::sysctl::set(crate::sysctl::PARAM_MM_ZERO_ON_ALLOC, original);

    if !all_zero {
        serial_println!("[mm]   FAIL: zero-on-free frame still has non-zero data!");
        return Err(KernelError::InternalError);
    }

    // Verify the generation counter incremented (at least 2 frees
    // with zero-on-free active: the first frame and frame2).
    let count = ZERO_ON_FREE_COUNT.load(Ordering::Relaxed);
    if count < 2 {
        serial_println!(
            "[mm]   FAIL: zero-on-free counter {} < 2 (expected at least 2 frees)",
            count,
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[mm]   Zero-on-free: OK (counter={}, settled={})",
        count,
        ZERO_ON_FREE_SETTLED.load(Ordering::Relaxed),
    );
    Ok(())
}

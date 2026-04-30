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
//! ## Per-CPU Free Lists (NOT YET IMPLEMENTED)
//!
//! To avoid cross-CPU atomic contention on the hot path, each CPU will
//! maintain a small local free list.  Allocations pull from the local list
//! first; when it's empty, a batch is refilled from the global allocator.
//! Currently all allocations go through the global spinlock.
//!
//! ## Performance Target
//!
//! Single alloc/free: < 1us (Linux buddy: 100-500ns).
//! See `bench/baselines.toml` for measured targets.

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
pub const PAGES_PER_FRAME: usize = FRAME_SIZE / 4096;

/// Maximum buddy order.  Order N = 2^N frames = `FRAME_SIZE` × 2^N bytes.
/// Order 10 = 1024 frames = 16 MiB.
const MAX_ORDER: usize = 10;

/// Page-info value indicating the frame is allocated (or not part of
/// usable memory).
const INFO_ALLOCATED: u8 = 0xFF;

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
// which provides exclusive access.  The raw pointer `page_info` points
// to memory exclusively owned by this allocator (carved from usable
// physical memory during init, never aliased).
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

        // Update free-frame count.  We consumed 2^order frames from the
        // free pool (the split halves that went back don't count — push_free
        // already accounted for them, and pop_free removed the parent).
        //
        // pop_free removed 2^source_order frames (via remove_free, which
        // doesn't update free_frames — that's our job).  The splits put
        // back everything except the 2^order block we're returning.
        //
        // Net change: free_frames -= 2^order (the splits cancel out against
        // the pop, leaving only the returned block unaccounted for).
        //
        // Wait — push_free in add_free_range updates free_frames, but
        // push_free during splitting does NOT update free_frames (it's
        // an internal redistribution).  Let me fix this: push_free always
        // updates free_frames, so I need to compensate.
        //
        // Actually, let me reconsider.  push_free adds to free_frames in
        // add_free_range (init path).  But during splitting, the frames
        // being split are already counted.  pop_free → remove_free doesn't
        // change free_frames.  push_free during splitting would double-count.
        //
        // The cleanest fix: DON'T update free_frames in push_free/remove_free.
        // Only update it in alloc_inner, free_inner, and add_free_range.
        //
        // ... but I already have push_free updating free_frames in
        // add_free_range.  Let me restructure.

        // Actually, the simplest correct approach:
        // - add_free_range tracks free_frames itself (already does)
        // - push_free / remove_free do NOT touch free_frames
        // - alloc_inner subtracts 2^order
        // - free_inner adds 2^order
        //
        // But wait, I have push_free updating free_frames via saturating_add
        // in add_free_range... no, look at the code: push_free doesn't
        // update free_frames.  add_free_range calls push_free and then
        // updates free_frames itself.  Let me verify...

        // OK, looking at my code above: push_free does NOT update
        // self.free_frames.  add_free_range does it explicitly after
        // push_free.  Good — so push_free / remove_free are pure list
        // operations.  alloc_inner and free_inner manage the counter.

        let frames_out = 1usize << order;
        self.free_frames = self.free_frames.saturating_sub(frames_out);

        Ok(addr)
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
// Public API
// ---------------------------------------------------------------------------

/// Allocator statistics snapshot.
#[derive(Debug, Clone)]
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

    // We need 1 byte per frame for the page_info array.
    let metadata_bytes = total_frames;
    let metadata_frames = (align_up(metadata_bytes as u64, frame_size) / frame_size) as usize;
    let metadata_size = (metadata_frames as u64) * frame_size;

    serial_println!(
        "[mm] Metadata: {} bytes ({} frames, {} KiB)",
        metadata_bytes,
        metadata_frames,
        metadata_size / 1024
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
/// skipping the metadata area.  Returns the number of usable frames added.
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

    // Initialize the metadata array (fill with INFO_ALLOCATED = "not free").
    let metadata_virt = (metadata_phys + hhdm_offset) as *mut u8;
    // SAFETY: metadata_phys is in a USABLE memory region, the HHDM maps
    // it to metadata_virt, and we have exclusive access during early boot
    // (single CPU, interrupts disabled, no other allocators).
    unsafe {
        core::ptr::write_bytes(metadata_virt, INFO_ALLOCATED, total_frames);
    }

    // Build the allocator and populate free lists.
    let mut allocator = BuddyAllocator {
        free_lists: [FreeList::new(); MAX_ORDER + 1],
        page_info: metadata_virt,
        page_info_len: total_frames,
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

    // Store in the global singleton.
    ALLOCATOR.call_once(|| Mutex::new(allocator));

    serial_println!("[mm] Physical frame allocator initialized");
    Ok(())
}

/// Allocate a single physical frame (16 KiB, order 0).
///
/// Returns the frame on success, or `OutOfMemory` if no frames are
/// available.
pub fn alloc_frame() -> KernelResult<PhysFrame> {
    alloc_order(0)
}

/// Allocate a contiguous block of 2^order physical frames.
///
/// The returned frame is naturally aligned to the block size
/// (e.g., order 2 = 64 KiB aligned).  `order` must be ≤ [`MAX_ORDER`].
pub fn alloc_order(order: usize) -> KernelResult<PhysFrame> {
    let allocator = ALLOCATOR.get().ok_or(KernelError::NotSupported)?;
    let mut guard = allocator.lock();
    let addr = guard.alloc_inner(order)?;

    // The buddy allocator always returns frame-aligned addresses.
    PhysFrame::from_addr(addr).ok_or(KernelError::InternalError)
}

/// Free a single physical frame (16 KiB, order 0).
///
/// # Safety
///
/// - The frame must have been allocated by [`alloc_frame()`].
/// - Must not be freed more than once (double-free is detected and
///   returns an error, but the caller should not rely on this).
/// - The caller must ensure no references to the frame's memory remain.
pub unsafe fn free_frame(frame: PhysFrame) -> KernelResult<()> {
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
pub unsafe fn free_order(frame: PhysFrame, order: usize) -> KernelResult<()> {
    let allocator = ALLOCATOR.get().ok_or(KernelError::NotSupported)?;
    let mut guard = allocator.lock();
    guard.free_inner(frame.addr(), order)
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

    serial_println!("[mm] Frame allocator self-test PASSED");
    Ok(())
}

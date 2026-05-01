//! Kernel heap allocator.
//!
//! Slab allocator with geometric (power-of-2) size classes, providing
//! `#[global_allocator]` for kernel code that needs dynamic memory
//! (`alloc` crate: `Box`, `Vec`, `String`, etc.).
//!
//! ## Size Classes
//!
//! Allocations are bucketed into power-of-2 size classes from 8 bytes
//! to 8192 bytes (11 classes).  Each class maintains a free list of
//! available slots.  When a class runs empty, a new 16 KiB frame is
//! allocated from the buddy allocator and divided into slots.
//!
//! Allocations larger than 8192 bytes go directly to the buddy
//! allocator (whole frames, naturally aligned).
//!
//! ## Thread Safety
//!
//! Two-tier locking:
//!
//! 1. **Per-CPU slab caches** (fast path): each CPU has a small free
//!    list per size class, accessed with interrupts disabled (no lock).
//!    Hits the common case without cross-CPU contention.
//!
//! 2. **Global slab allocator** (slow path): protected by a spinlock.
//!    Used for batch refill/drain of per-CPU caches and for large
//!    allocations.
//!
//! ## Performance Target
//!
//! Common-size allocation: < 200ns (jemalloc: 20-50ns).
//! Per-CPU cache hit (uncontended): ~30-50ns (interrupt disable + pop).
//! See `bench/baselines.toml` for measured targets.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::serial_println;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Allocation statistics (lock-free, atomic counters)
// ---------------------------------------------------------------------------

/// Total number of slab allocations since boot.
static SLAB_ALLOCS: AtomicU64 = AtomicU64::new(0);
/// Total number of slab deallocations since boot.
static SLAB_FREES: AtomicU64 = AtomicU64::new(0);
/// Total number of large allocations (> MAX_SLAB_SIZE) since boot.
static LARGE_ALLOCS: AtomicU64 = AtomicU64::new(0);
/// Total number of large deallocations since boot.
static LARGE_FREES: AtomicU64 = AtomicU64::new(0);
/// Total number of slab refills (new frame carved into slots).
static SLAB_REFILLS: AtomicU64 = AtomicU64::new(0);
/// Total number of failed allocations (OOM).
static ALLOC_FAILURES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Power-of-2 size classes served by the slab allocator.
const SIZE_CLASSES: [usize; 11] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192];

/// Number of size classes.
const NUM_CLASSES: usize = SIZE_CLASSES.len();

/// Maximum allocation size served by the slab path.  Larger requests
/// go directly to the buddy allocator.
const MAX_SLAB_SIZE: usize = 8192;

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

/// A free slot in a slab.  Stored in the slot's own memory when the
/// slot is not in use (same intrusive-list technique as the buddy
/// allocator's `FreeNode`).
struct FreeSlot {
    next: *mut FreeSlot,
}

/// Inner allocator state, protected by a spinlock.
struct HeapInner {
    /// Head of the free list for each size class.  Pointers are HHDM
    /// virtual addresses.
    free_lists: [*mut FreeSlot; NUM_CLASSES],

    /// HHDM offset for physical → virtual conversion.
    hhdm_offset: u64,

    /// Set to `true` after `init()` is called.
    initialized: bool,
}

// SAFETY: HeapInner is only accessed through a spin::Mutex, which
// provides exclusive access.  The raw pointers in `free_lists` point
// to frame-backed HHDM memory exclusively owned by this allocator.
unsafe impl Send for HeapInner {}

// ---------------------------------------------------------------------------
// Global allocator
// ---------------------------------------------------------------------------

/// The kernel heap allocator instance.
///
/// Registered as `#[global_allocator]` so the Rust `alloc` crate
/// routes all heap operations through it.
pub struct KernelHeap {
    inner: Mutex<HeapInner>,
}

// SAFETY: All access goes through the inner Mutex.
unsafe impl Sync for KernelHeap {}

#[global_allocator]
static HEAP: KernelHeap = KernelHeap {
    inner: Mutex::new(HeapInner {
        free_lists: [ptr::null_mut(); NUM_CLASSES],
        hhdm_offset: 0,
        initialized: false,
    }),
};

// ---------------------------------------------------------------------------
// HeapInner implementation
// ---------------------------------------------------------------------------

impl HeapInner {
    /// Find the size-class index for a given layout.
    ///
    /// Returns `None` if the allocation is too large for the slab path
    /// (should go to the buddy allocator directly).
    ///
    /// OPT: Uses bit manipulation instead of linear scan.  SIZE_CLASSES
    /// are powers of 2 from 2^3 (8) to 2^13 (8192), so the index is
    /// simply `ilog2(next_power_of_two(needed)) - 3`.  This replaces an
    /// up-to-11-iteration loop with a single branch + bit op on every
    /// alloc and dealloc call.
    #[inline]
    fn size_class_index(layout: &Layout) -> Option<usize> {
        // The class must be large enough for both the requested size
        // and alignment.  Since classes are powers of 2 and slots are
        // spaced at class_size intervals from a frame-aligned base,
        // using a class >= alignment guarantees proper alignment.
        let needed = layout.size().max(layout.align());
        if needed > MAX_SLAB_SIZE {
            return None;
        }
        // Smallest class is 8 = 2^3.  Anything <= 8 goes to index 0.
        if needed <= 8 {
            return Some(0);
        }
        // Round up to the next power of two, then extract the exponent.
        // needed is 9..=8192 here, so next_power_of_two() won't overflow.
        // trailing_zeros() gives log2 for a power-of-two value.
        // Subtract 3 because class 0 = 2^3.
        #[allow(clippy::arithmetic_side_effects)]
        Some(needed.next_power_of_two().trailing_zeros() as usize - 3)
    }

    /// Allocate a new frame, divide it into slots for the given class,
    /// and prepend all slots to the class's free list.
    ///
    /// Returns `true` on success, `false` if the frame allocator is
    /// out of memory.
    // cast_ptr_alignment: slot addresses are aligned to class_size (power of 2
    // >= 8 bytes), which meets FreeSlot's 8-byte alignment requirement.
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::cast_ptr_alignment)]
    fn refill(&mut self, class_idx: usize) -> bool {
        let class_size = SIZE_CLASSES[class_idx];

        // Allocate a physical frame.
        let Ok(frame) = frame::alloc_frame() else {
            return false;
        };

        let virt_base = frame.to_virt(self.hhdm_offset) as *mut u8;
        let slots = FRAME_SIZE / class_size;

        // Build a free list through the slab in reverse order so that
        // the first slot (lowest address) ends up at the head.
        for i in (0..slots).rev() {
            // SAFETY: virt_base points to a valid 16 KiB frame via HHDM.
            // i * class_size is always < FRAME_SIZE.  class_size >= 8,
            // so the slot is large enough for a FreeSlot (1 pointer = 8 bytes).
            let slot_ptr = unsafe { virt_base.add(i * class_size) }.cast::<FreeSlot>();
            // SAFETY: slot_ptr is valid, aligned (class_size is a power of 2
            // >= 8, and virt_base is 16 KiB aligned), and we have exclusive
            // access via the spinlock.
            unsafe {
                (*slot_ptr).next = self.free_lists[class_idx];
            }
            self.free_lists[class_idx] = slot_ptr;
        }

        SLAB_REFILLS.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Allocate from a slab size class.
    #[allow(clippy::indexing_slicing)]
    fn slab_alloc(&mut self, class_idx: usize) -> *mut u8 {
        // Refill if the free list is empty.
        if self.free_lists[class_idx].is_null() && !self.refill(class_idx) {
            return ptr::null_mut();
        }

        // Pop the head slot.
        let slot = self.free_lists[class_idx];
        // SAFETY: slot is non-null (we just refilled or it was already non-null).
        // It points to a valid FreeSlot in HHDM-mapped frame memory.
        self.free_lists[class_idx] = unsafe { (*slot).next };
        slot.cast::<u8>()
    }

    /// Return a slot to its size class's free list.
    // cast_ptr_alignment: same as refill — slot pointers are aligned to
    // the class size (>= 8 bytes).
    #[allow(clippy::indexing_slicing, clippy::cast_ptr_alignment)]
    fn slab_dealloc(&mut self, ptr: *mut u8, class_idx: usize) {
        let slot = ptr.cast::<FreeSlot>();
        // SAFETY: ptr was returned by slab_alloc, so it points to a valid
        // slot in HHDM-mapped memory.  The slot is large enough for
        // FreeSlot (>= 8 bytes).
        unsafe {
            (*slot).next = self.free_lists[class_idx];
        }
        self.free_lists[class_idx] = slot;
    }

    /// Compute the buddy allocator order needed for a large allocation.
    ///
    /// Accounts for both the requested size and alignment.
    #[allow(clippy::arithmetic_side_effects)]
    fn large_order(layout: &Layout) -> usize {
        let needed = layout.size().max(layout.align());
        let frames = needed.div_ceil(FRAME_SIZE);
        if frames <= 1 {
            return 0;
        }
        // Round up to the next power of 2 for buddy-allocator orders.
        // frames >= 2 here, so next_power_of_two() won't overflow for
        // any practical allocation size on 64-bit.
        frames.next_power_of_two().trailing_zeros() as usize
    }

    /// Allocate directly from the buddy allocator (for large requests).
    fn large_alloc(&self, layout: &Layout) -> *mut u8 {
        let order = Self::large_order(layout);
        match frame::alloc_order(order) {
            Ok(f) => f.to_virt(self.hhdm_offset) as *mut u8,
            Err(_) => ptr::null_mut(),
        }
    }

    /// Free a large allocation back to the buddy allocator.
    ///
    /// # Safety
    ///
    /// `ptr` must have been returned by `large_alloc` with the same layout.
    #[allow(clippy::arithmetic_side_effects)]
    unsafe fn large_dealloc(&self, ptr: *mut u8, layout: &Layout) {
        let order = Self::large_order(layout);

        // Convert HHDM virtual address back to physical.
        let virt = ptr as u64;
        let phys = virt - self.hhdm_offset;

        if let Some(frame) = PhysFrame::from_addr(phys) {
            // SAFETY: The caller guarantees ptr was allocated by large_alloc
            // with the same layout, which used alloc_order(order).
            // Ignoring the Result: if free_order fails, we leak memory
            // (which is better than corrupting the allocator).  In practice,
            // this cannot fail if the caller upholds the safety contract.
            let _ = unsafe { frame::free_order(frame, order) };
        }
    }
}

// ---------------------------------------------------------------------------
// Per-CPU slab caches
// ---------------------------------------------------------------------------

/// Maximum free slots per size class per CPU.
///
/// 16 is enough to absorb short alloc/free bursts without contending
/// on the global lock.  Batch refill/drain transfers half at a time.
const PCPU_SLAB_MAX: usize = 16;

/// Batch size for refilling/draining per-CPU caches (half of max).
const PCPU_SLAB_BATCH: usize = PCPU_SLAB_MAX / 2;

/// Maximum CPUs (mirrors smp::MAX_CPUS).
const HEAP_MAX_CPUS: usize = 16;

/// Per-CPU slab cache: one singly-linked free list per size class.
///
/// Accessed only with interrupts disabled on the owning CPU.
/// No lock needed — interrupt-disable serializes access on a single CPU.
///
/// We store the free list heads as `usize` (cast from `*mut FreeSlot`)
/// to avoid raw-pointer Sync issues in a static.  Zero means empty.
/// OPT: Aligned to 64 bytes (x86 cache line) to prevent false sharing
/// between CPUs.  Each CPU's slab cache occupies its own cache line(s).
#[repr(align(64))]
struct PerCpuSlabCache {
    /// Free list heads for each size class (HHDM virtual addresses).
    heads: [usize; NUM_CLASSES],
    /// Number of free slots in each class's list.
    counts: [u16; NUM_CLASSES],
}

impl PerCpuSlabCache {
    const fn new() -> Self {
        Self {
            heads: [0; NUM_CLASSES],
            counts: [0; NUM_CLASSES],
        }
    }
}

// SAFETY: Each PerCpuSlabCache is only accessed by one CPU at a time
// (interrupt-disabled serialization).  The usize values are interpreted
// as *mut FreeSlot only on the owning CPU.
unsafe impl Send for PerCpuSlabCache {}
unsafe impl Sync for PerCpuSlabCache {}

/// Static array of per-CPU slab caches.
static mut PCPU_SLAB_CACHES: [PerCpuSlabCache; HEAP_MAX_CPUS] = {
    const INIT: PerCpuSlabCache = PerCpuSlabCache::new();
    [INIT; HEAP_MAX_CPUS]
};

/// Whether per-CPU slab caches are enabled.
///
/// Starts `false` — enabled after SMP bootstrap via [`enable_pcpu_slab_caches`].
static PCPU_SLAB_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable per-CPU slab caches.
///
/// Call after SMP bootstrap when `current_cpu_index()` works correctly.
pub fn enable_pcpu_slab_caches() {
    PCPU_SLAB_ENABLED.store(true, Ordering::Release);
    serial_println!("[heap] Per-CPU slab caches enabled");
}

/// Try to allocate from the per-CPU slab cache.
///
/// Returns a pointer to the allocated slot, or null if the local cache
/// is empty and batch refill also failed.
///
/// # Safety
///
/// Must be called with a valid `class_idx` (0..NUM_CLASSES).
/// The global heap must be initialized.
#[allow(clippy::cast_ptr_alignment)]
unsafe fn pcpu_slab_alloc(class_idx: usize) -> *mut u8 {
    // SAFETY: We need to disable interrupts for exclusive per-CPU access.
    let flags = unsafe { frame::disable_interrupts() };
    let cpu = crate::smp::current_cpu_index();

    // SAFETY: Interrupts are disabled, so no concurrent access from
    // this CPU.  `cpu` is a valid index (< HEAP_MAX_CPUS).
    let cache = unsafe { &mut PCPU_SLAB_CACHES[cpu] };

    if cache.counts[class_idx] > 0 {
        // Fast path: pop from local cache.
        let slot_ptr = cache.heads[class_idx] as *mut FreeSlot;
        // SAFETY: slot_ptr is non-null (count > 0 means head is valid).
        // It points to HHDM-mapped frame memory owned by this allocator.
        cache.heads[class_idx] = unsafe { (*slot_ptr).next } as usize;
        cache.counts[class_idx] -= 1;
        // SAFETY: Restoring interrupt state to what it was before.
        unsafe { frame::restore_interrupts(flags); }
        return slot_ptr.cast::<u8>();
    }

    // Slow path: batch refill from global allocator.
    // Take the global lock while still interrupt-disabled (leaf lock).
    let mut inner = HEAP.inner.lock();
    let mut transferred = 0u16;

    for _ in 0..PCPU_SLAB_BATCH {
        let head = inner.free_lists[class_idx];
        if head.is_null() {
            // Global free list empty — try to refill it.
            if !inner.refill(class_idx) {
                break; // OOM — stop refilling.
            }
            continue; // Retry after refill added new slots.
        }
        // Pop from global, push to local.
        // SAFETY: head is non-null, points to valid HHDM memory.
        inner.free_lists[class_idx] = unsafe { (*head).next };
        unsafe { (*head).next = cache.heads[class_idx] as *mut FreeSlot; }
        cache.heads[class_idx] = head as usize;
        transferred += 1;
    }
    // Release global lock.
    drop(inner);

    cache.counts[class_idx] += transferred;

    if cache.counts[class_idx] > 0 {
        // Got at least one slot — pop it.
        let slot_ptr = cache.heads[class_idx] as *mut FreeSlot;
        // SAFETY: same as fast path above.
        cache.heads[class_idx] = unsafe { (*slot_ptr).next } as usize;
        cache.counts[class_idx] -= 1;
        // SAFETY: Restoring interrupt state.
        unsafe { frame::restore_interrupts(flags); }
        slot_ptr.cast::<u8>()
    } else {
        // Couldn't get any slots (OOM).
        // SAFETY: Restoring interrupt state.
        unsafe { frame::restore_interrupts(flags); }
        ptr::null_mut()
    }
}

/// Try to free to the per-CPU slab cache.
///
/// Returns `true` if the slot was cached locally, `false` if the
/// per-CPU path couldn't handle it (shouldn't happen in practice).
///
/// # Safety
///
/// `ptr` must have been allocated from the slab for `class_idx`.
#[allow(clippy::cast_ptr_alignment)]
unsafe fn pcpu_slab_dealloc(ptr: *mut u8, class_idx: usize) -> bool {
    // SAFETY: We need to disable interrupts for exclusive per-CPU access.
    let flags = unsafe { frame::disable_interrupts() };
    let cpu = crate::smp::current_cpu_index();

    // SAFETY: Interrupts disabled, exclusive access to this CPU's cache.
    let cache = unsafe { &mut PCPU_SLAB_CACHES[cpu] };

    if cache.counts[class_idx] < PCPU_SLAB_MAX as u16 {
        // Fast path: push to local cache.
        let slot = ptr.cast::<FreeSlot>();
        // SAFETY: ptr is a valid slab slot (caller guarantee).
        unsafe { (*slot).next = cache.heads[class_idx] as *mut FreeSlot; }
        cache.heads[class_idx] = slot as usize;
        cache.counts[class_idx] += 1;
        // SAFETY: Restoring interrupt state.
        unsafe { frame::restore_interrupts(flags); }
        return true;
    }

    // Local cache full — drain half to global.
    let mut inner = HEAP.inner.lock();
    for _ in 0..PCPU_SLAB_BATCH {
        if cache.counts[class_idx] == 0 {
            break;
        }
        let slot_ptr = cache.heads[class_idx] as *mut FreeSlot;
        // SAFETY: slot_ptr is valid (count > 0).
        cache.heads[class_idx] = unsafe { (*slot_ptr).next } as usize;
        cache.counts[class_idx] -= 1;
        // Push to global free list.
        unsafe { (*slot_ptr).next = inner.free_lists[class_idx]; }
        inner.free_lists[class_idx] = slot_ptr;
    }
    drop(inner);

    // Now push the new slot to the (no longer full) local cache.
    let slot = ptr.cast::<FreeSlot>();
    // SAFETY: ptr is a valid slab slot.
    unsafe { (*slot).next = cache.heads[class_idx] as *mut FreeSlot; }
    cache.heads[class_idx] = slot as usize;
    cache.counts[class_idx] += 1;

    // SAFETY: Restoring interrupt state.
    unsafe { frame::restore_interrupts(flags); }
    true
}

// ---------------------------------------------------------------------------
// GlobalAlloc implementation
// ---------------------------------------------------------------------------

unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let is_slab = HeapInner::size_class_index(&layout).is_some();

        // Per-CPU slab cache fast path (lock-free).
        if PCPU_SLAB_ENABLED.load(Ordering::Relaxed) {
            if let Some(class_idx) = HeapInner::size_class_index(&layout) {
                // SAFETY: class_idx is valid (checked by size_class_index),
                // heap is initialized (PCPU_SLAB_ENABLED is set after init).
                let ptr = unsafe { pcpu_slab_alloc(class_idx) };
                if !ptr.is_null() {
                    SLAB_ALLOCS.fetch_add(1, Ordering::Relaxed);
                    return ptr;
                }
                // Per-CPU path failed (OOM) — fall through to global.
            }
        }

        // Global locked path.
        let mut inner = self.inner.lock();
        if !inner.initialized {
            return ptr::null_mut();
        }

        let ptr = match HeapInner::size_class_index(&layout) {
            Some(class_idx) => inner.slab_alloc(class_idx),
            None => inner.large_alloc(&layout),
        };

        if ptr.is_null() {
            ALLOC_FAILURES.fetch_add(1, Ordering::Relaxed);
        } else if is_slab {
            SLAB_ALLOCS.fetch_add(1, Ordering::Relaxed);
        } else {
            LARGE_ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }

        // Per-CPU slab cache fast path.
        if PCPU_SLAB_ENABLED.load(Ordering::Relaxed) {
            if let Some(class_idx) = HeapInner::size_class_index(&layout) {
                // SAFETY: ptr was allocated from slab for this class.
                if unsafe { pcpu_slab_dealloc(ptr, class_idx) } {
                    SLAB_FREES.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
        }

        // Global locked path.
        let mut inner = self.inner.lock();
        if !inner.initialized {
            return;
        }

        match HeapInner::size_class_index(&layout) {
            Some(class_idx) => {
                inner.slab_dealloc(ptr, class_idx);
                SLAB_FREES.fetch_add(1, Ordering::Relaxed);
            }
            // SAFETY: Caller guarantees ptr was allocated with this layout.
            None => {
                unsafe { inner.large_dealloc(ptr, &layout) };
                LARGE_FREES.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Statistics API
// ---------------------------------------------------------------------------

/// Snapshot of heap allocator statistics.
///
/// All counters are cumulative since boot.  They use relaxed atomic
/// loads, so a snapshot may not be perfectly self-consistent under
/// heavy concurrent allocation, but individual counters are accurate.
#[derive(Debug, Clone, Copy)]
pub struct HeapStats {
    /// Total slab-path allocations since boot.
    pub slab_allocs: u64,
    /// Total slab-path deallocations since boot.
    pub slab_frees: u64,
    /// Total large (buddy-path) allocations since boot.
    pub large_allocs: u64,
    /// Total large (buddy-path) deallocations since boot.
    pub large_frees: u64,
    /// Number of slab refills (new frame carved into slots).
    pub slab_refills: u64,
    /// Number of failed allocations (OOM).
    pub alloc_failures: u64,
}

/// Read heap allocator statistics (lock-free).
///
/// Returns a snapshot of the atomic counters.  No lock is needed since
/// all counters use relaxed atomic operations.
pub fn stats() -> HeapStats {
    HeapStats {
        slab_allocs: SLAB_ALLOCS.load(Ordering::Relaxed),
        slab_frees: SLAB_FREES.load(Ordering::Relaxed),
        large_allocs: LARGE_ALLOCS.load(Ordering::Relaxed),
        large_frees: LARGE_FREES.load(Ordering::Relaxed),
        slab_refills: SLAB_REFILLS.load(Ordering::Relaxed),
        alloc_failures: ALLOC_FAILURES.load(Ordering::Relaxed),
    }
}

/// Read heap statistics without blocking (alias for [`stats`]).
///
/// Since the stats are atomic counters (no lock needed), this always
/// succeeds.  Provided for API consistency with [`frame::try_stats`].
pub fn try_stats() -> Option<HeapStats> {
    Some(stats())
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the kernel heap allocator.
///
/// Must be called after the frame allocator is initialized and before
/// any heap allocations are made.
pub fn init(hhdm_offset: u64) {
    let mut inner = HEAP.inner.lock();
    inner.hhdm_offset = hhdm_offset;
    inner.initialized = true;
    serial_println!("[mm] Kernel heap allocator initialized");
}

/// Run a boot-time self-test of the heap allocator.
///
/// Exercises slab allocations at various size classes, large allocations,
/// and verifies that allocated memory is usable.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    serial_println!("[heap] Running heap allocator self-test...");

    // -- Test 1: Small slab allocations across size classes -------------------
    let test_sizes: [usize; 6] = [8, 32, 64, 256, 1024, 8192];
    for &size in &test_sizes {
        let layout = Layout::from_size_align(size, 8)
            .map_err(|_| KernelError::InvalidArgument)?;

        // SAFETY: layout is valid (non-zero size, power-of-2 alignment).
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        if ptr.is_null() {
            serial_println!("[heap]   FAIL: alloc({}) returned null", size);
            return Err(KernelError::OutOfMemory);
        }

        // Write to the allocation to verify it's usable.
        // SAFETY: ptr is valid and points to at least `size` bytes.
        unsafe { ptr.write_bytes(0xAA, size); }

        // Verify the write.
        // SAFETY: ptr is valid and initialized.
        let first = unsafe { ptr.read() };
        if first != 0xAA {
            serial_println!("[heap]   FAIL: memory at {:p} not writable", ptr);
            return Err(KernelError::InternalError);
        }

        // SAFETY: ptr was just allocated with this layout.
        unsafe { alloc::alloc::dealloc(ptr, layout); }
    }
    serial_println!("[heap]   Slab allocations (6 sizes): OK");

    // -- Test 2: Large allocation (32 KiB = 2 frames) ------------------------
    let large_layout = Layout::from_size_align(32 * 1024, 16)
        .map_err(|_| KernelError::InvalidArgument)?;

    // SAFETY: layout is valid.
    let large_ptr = unsafe { alloc::alloc::alloc_zeroed(large_layout) };
    if large_ptr.is_null() {
        serial_println!("[heap]   FAIL: large alloc returned null");
        return Err(KernelError::OutOfMemory);
    }

    // Verify zeroed.
    // SAFETY: large_ptr is valid and points to 32 KiB of zeroed memory.
    let first_byte = unsafe { large_ptr.read() };
    if first_byte != 0 {
        serial_println!("[heap]   FAIL: alloc_zeroed not zeroed");
        return Err(KernelError::InternalError);
    }

    // SAFETY: large_ptr was allocated with large_layout.
    unsafe { alloc::alloc::dealloc(large_ptr, large_layout); }
    serial_println!("[heap]   Large allocation (32 KiB): OK");

    // -- Test 3: Multiple allocations (slab refill) --------------------------
    let small_layout = Layout::from_size_align(64, 8)
        .map_err(|_| KernelError::InvalidArgument)?;
    let count = 32;
    let mut ptrs = [ptr::null_mut::<u8>(); 32];
    for slot in &mut ptrs {
        // SAFETY: layout is valid.
        let p = unsafe { alloc::alloc::alloc(small_layout) };
        if p.is_null() {
            serial_println!("[heap]   FAIL: batch alloc returned null");
            return Err(KernelError::OutOfMemory);
        }
        *slot = p;
    }
    // Free all.
    for &p in &ptrs {
        // SAFETY: each pointer was allocated with small_layout.
        unsafe { alloc::alloc::dealloc(p, small_layout); }
    }
    serial_println!("[heap]   Batch alloc/free ({} x 64B): OK", count);

    serial_println!("[heap] Heap allocator self-test PASSED");
    Ok(())
}

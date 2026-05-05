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
//! 1. **Per-CPU slab caches** (fast path): each CPU has a free list per
//!    size class, protected by a reentrancy flag (no lock, no CLI/STI).
//!    If an ISR interrupts mid-operation, it falls through to the global
//!    path.  Avoids VM-exit overhead from CLI/STI under hypervisors.
//!
//! 2. **Global slab allocator** (slow path): protected by a spinlock.
//!    Used for batch refill/drain of per-CPU caches and for large
//!    allocations.
//!
//! ## Performance Target
//!
//! Common-size allocation: < 200ns (jemalloc: 20-50ns).
//! Per-CPU cache hit (uncontended): ~15-30ns (load + pop + store).
//! See `bench/baselines.toml` for measured targets.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::serial_println;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Slab poisoning (use-after-free / double-free detection)
// ---------------------------------------------------------------------------

/// Poison byte written to freed slab memory (0xFE = "Free Entry").
///
/// On dealloc, bytes 8..class_size are filled with this pattern.
/// On alloc, those bytes are checked — if they've been modified, a
/// use-after-free write was made to the freed memory.
const FREE_POISON: u8 = 0xFE;

/// Poison byte written to freshly allocated memory before handing to caller.
/// Helps catch reads of uninitialized memory (will produce obviously-wrong
/// values rather than seemingly-valid zeros or stale data).
const ALLOC_POISON: u8 = 0xCD;

/// Whether slab poisoning is active.  Adds ~5-20ns per alloc/dealloc
/// (memset + memcmp over the slot body).  Enable during development
/// and testing; disable for production or benchmarks.
static POISON_ENABLED: AtomicBool = AtomicBool::new(false);

/// Count of detected use-after-free corruptions.
static POISON_VIOLATIONS: AtomicU32 = AtomicU32::new(0);

/// Enable slab poisoning (use-after-free detection).
pub fn enable_poison() {
    POISON_ENABLED.store(true, Ordering::Release);
    serial_println!("[heap] Slab poisoning enabled (UAF detection active)");
}

/// Disable slab poisoning.
#[allow(dead_code)]
pub fn disable_poison() {
    POISON_ENABLED.store(false, Ordering::Release);
}

/// Return the number of poison violations detected.
#[allow(dead_code)]
pub fn poison_violations() -> u32 {
    POISON_VIOLATIONS.load(Ordering::Relaxed)
}

/// 4-byte magic signature written at bytes 8..12 of poisoned slots.
///
/// `check_poison` looks for this first — if absent, the slot was never
/// freed through the poison path (e.g., carved from a fresh frame) and
/// the check is skipped.  This eliminates false positives from the
/// "cold start" period after poisoning is enabled.
const POISON_MAGIC: [u8; 4] = [0xFE, 0xED, 0xFA, 0xCE];

/// Fill bytes 8..class_size with FREE_POISON, prefixed by POISON_MAGIC.
///
/// Skips the first 8 bytes (used by the FreeSlot::next pointer).
/// For slots < 16 bytes, there's not enough room for the magic + poison.
///
/// Both `#[inline(never)]` and `write_volatile` are required here.
/// Under thin LTO + O3, the compiler can constant-propagate through the
/// dealloc→alloc boundary and dead-store-eliminate regular writes to
/// freed memory.  Volatile stores create a compiler memory barrier that
/// prevents this optimization, and `#[inline(never)]` prevents the
/// function body from being visible to the caller's optimization context.
///
/// # Safety
///
/// `ptr` must point to a valid slab slot of `class_size` bytes.
#[inline(never)]
unsafe fn poison_free(ptr: *mut u8, class_size: usize) {
    if class_size < 16 {
        return; // Need at least 8 (next ptr) + 4 (magic) + some poison.
    }
    // Write the magic signature at bytes 8..12 using volatile stores.
    // Volatile prevents the optimizer from dead-store-eliminating these
    // writes even with full LTO visibility.
    // SAFETY: ptr is valid for class_size bytes (>= 16).
    unsafe {
        core::ptr::write_volatile(ptr.add(8), POISON_MAGIC[0]);
        core::ptr::write_volatile(ptr.add(9), POISON_MAGIC[1]);
        core::ptr::write_volatile(ptr.add(10), POISON_MAGIC[2]);
        core::ptr::write_volatile(ptr.add(11), POISON_MAGIC[3]);
    }
    // Fill bytes 12..class_size with FREE_POISON.
    for i in 12..class_size {
        unsafe {
            core::ptr::write_volatile(ptr.add(i), FREE_POISON);
        }
    }
}

/// Fill bytes 0..class_size with ALLOC_POISON.
///
/// # Safety
///
/// `ptr` must point to a valid slab slot of `class_size` bytes.
#[inline]
unsafe fn poison_alloc(ptr: *mut u8, class_size: usize) {
    // SAFETY: ptr is valid for class_size bytes.
    unsafe {
        ptr.write_bytes(ALLOC_POISON, class_size);
    }
}

/// Check poison integrity on a slot being allocated.
///
/// First verifies the POISON_MAGIC signature at bytes 8..12.  If not
/// present, this slot was never freed through the poison path (e.g.,
/// carved from a fresh frame during refill) — skip the check silently.
///
/// If the magic IS present, checks bytes 12..class_size for FREE_POISON.
/// Any modification indicates a use-after-free write.
///
/// # Safety
///
/// `ptr` must point to a valid slab slot of `class_size` bytes.
///
/// NOTE: `#[inline(never)]` is required here.  With `#[inline]` + thin LTO,
/// the compiler inlines check_poison into pcpu_slab_alloc and then
/// constant-propagates through the dealloc→alloc boundary, "knowing"
/// what bytes poison_free wrote and skipping the actual memory reads.
/// Preventing inlining forces an actual function call with a real
/// memory read that can't be elided.
#[inline(never)]
unsafe fn check_poison(ptr: *mut u8, class_size: usize) {
    if class_size < 16 {
        return;
    }
    // Check the magic signature using volatile reads.  This prevents
    // the optimizer from constant-propagating through dealloc→alloc
    // boundaries (it can't assume it knows what's at ptr+8 even with
    // full LTO visibility into poison_free).
    let m0 = unsafe { core::ptr::read_volatile(ptr.add(8)) };
    let m1 = unsafe { core::ptr::read_volatile(ptr.add(9)) };
    let m2 = unsafe { core::ptr::read_volatile(ptr.add(10)) };
    let m3 = unsafe { core::ptr::read_volatile(ptr.add(11)) };
    if m0 != POISON_MAGIC[0] || m1 != POISON_MAGIC[1]
        || m2 != POISON_MAGIC[2] || m3 != POISON_MAGIC[3]
    {
        return; // Virgin slot — never been through poison_free.
    }

    // Magic is intact.  Now check the poison zone (bytes 12..class_size).
    if class_size <= 12 {
        return;
    }
    for i in 12..class_size {
        let byte = unsafe { core::ptr::read_volatile(ptr.add(i)) };
        if byte != FREE_POISON {
            POISON_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
            serial_println!(
                "[heap] USE-AFTER-FREE detected! slot={:#x}, offset={}, expected=0x{:02X}, found=0x{:02X}, class={}",
                ptr as usize, i, FREE_POISON, byte, class_size
            );
            // Only report the first corrupted byte per slot.
            return;
        }
    }
}

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

/// Per-size-class allocation counters (for leak detection / profiling).
///
/// `CLASS_ALLOCS[i]` counts total allocations from size class `i` since boot.
/// `CLASS_FREES[i]` counts total frees back to size class `i` since boot.
/// Active objects = allocs - frees.  A monotonically growing difference
/// suggests a leak in that size class.
static CLASS_ALLOCS: [AtomicU64; 11] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; 11]
};
static CLASS_FREES: [AtomicU64; 11] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; 11]
};

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
        let ptr = slot.cast::<u8>();

        // Slab poisoning: check free-poison integrity, then alloc-poison.
        if POISON_ENABLED.load(Ordering::Relaxed) {
            let class_size = SIZE_CLASSES[class_idx];
            // SAFETY: ptr is a valid slab slot of class_size bytes.
            unsafe { check_poison(ptr, class_size); }
            unsafe { poison_alloc(ptr, class_size); }
        }
        ptr
    }

    /// Return a slot to its size class's free list.
    // cast_ptr_alignment: same as refill — slot pointers are aligned to
    // the class size (>= 8 bytes).
    #[allow(clippy::indexing_slicing, clippy::cast_ptr_alignment)]
    fn slab_dealloc(&mut self, ptr: *mut u8, class_idx: usize) {
        // Slab poisoning: fill freed memory with FREE_POISON pattern.
        if POISON_ENABLED.load(Ordering::Relaxed) {
            let class_size = SIZE_CLASSES[class_idx];
            // SAFETY: ptr is a valid slab slot of class_size bytes.
            unsafe { poison_free(ptr, class_size); }
        }

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
/// OPT: 32 slots per class absorbs allocation bursts without hitting
/// the global lock.  At 32 slots, steady alloc/free sequences hit the
/// slow-path refill every ~32 iterations.  Memory cost: 64 × 11 classes
/// × 8 bytes × 16 CPUs = 88 KiB total — negligible.
///
/// OPT: Increased from 32→64 to reduce global lock contention under
/// burst allocation patterns.  The steady-state cost is nil (each slot
/// is just an 8-byte pointer in the free list).
const PCPU_SLAB_MAX: usize = 64;

/// Batch size for refilling/draining per-CPU caches (half of max).
const PCPU_SLAB_BATCH: usize = PCPU_SLAB_MAX / 2;

/// Maximum CPUs (mirrors smp::MAX_CPUS).
const HEAP_MAX_CPUS: usize = 16;

/// Per-CPU slab cache: one singly-linked free list per size class.
///
/// Accessed only with interrupts disabled on the owning CPU.
/// No lock needed — a reentrancy flag serializes access on a single CPU.
///
/// We store the free list heads as `usize` (cast from `*mut FreeSlot`)
/// to avoid raw-pointer Sync issues in a static.  Zero means empty.
/// OPT: Aligned to 64 bytes (x86 cache line) to prevent false sharing
/// between CPUs.  Each CPU's slab cache occupies its own cache line(s).
///
/// ## Why no CLI/STI?
///
/// Previous versions used `cli`/`sti` (interrupt disable/enable) to
/// serialize per-CPU access.  Under hypervisors (WHPX, KVM), CLI/STI
/// cause VM exits costing ~200-500 cycles EACH.  With 2× per alloc +
/// 2× per dealloc, that's 800-2000 cycles of pure overhead per
/// alloc+free cycle.
///
/// Instead we use a simple boolean `active` flag.  Since only THIS CPU
/// accesses its cache, and only the timer ISR can interrupt us, the
/// flag prevents re-entry: if the ISR fires mid-alloc and itself needs
/// to allocate, it sees `active == true` and falls through to the
/// global locked path.  This is safe because:
/// 1. Only code on this CPU reads/writes this cache (per-CPU data).
/// 2. Interrupt handlers are the only source of re-entry on a CPU.
/// 3. The global path is always available as a fallback.
#[repr(align(64))]
struct PerCpuSlabCache {
    /// Reentrancy guard: true while this CPU is mid-operation on the cache.
    /// If an ISR needs to alloc/dealloc while this is set, it falls
    /// through to the global locked path.  Plain bool (not atomic) is
    /// safe because only this CPU accesses it.
    active: bool,
    /// Free list heads for each size class (HHDM virtual addresses).
    heads: [usize; NUM_CLASSES],
    /// Number of free slots in each class's list.
    counts: [u16; NUM_CLASSES],
    /// Per-CPU slab allocation count.
    ///
    /// OPT: Counted with a plain store (no `lock` prefix) instead of
    /// a global `lock xadd`.  Aggregated by `stats()` on demand.
    /// Saves ~30-50 cycles per alloc/dealloc on the per-CPU fast path
    /// by avoiding cross-CPU cache line bouncing.
    slab_allocs: u64,
    /// Per-CPU slab deallocation count.
    slab_frees: u64,
}

impl PerCpuSlabCache {
    const fn new() -> Self {
        Self {
            active: false,
            heads: [0; NUM_CLASSES],
            counts: [0; NUM_CLASSES],
            slab_allocs: 0,
            slab_frees: 0,
        }
    }
}

// SAFETY: Each PerCpuSlabCache is only accessed by one CPU at a time
// (per-CPU reentrancy guard serialization).  The usize values are
// interpreted as *mut FreeSlot only on the owning CPU.
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
#[inline(always)]
#[allow(clippy::cast_ptr_alignment)]
unsafe fn pcpu_slab_alloc(class_idx: usize) -> *mut u8 {
    let cpu = crate::smp::fast_cpu_index();

    // SAFETY: `cpu` is a valid index (< HEAP_MAX_CPUS).  We access the
    // cache via a raw pointer to avoid borrow issues with the reentrancy
    // guard.  Only this CPU accesses this cache element.
    let cache = unsafe { &mut PCPU_SLAB_CACHES[cpu] };

    // Reentrancy guard: if we're already inside an alloc/dealloc on
    // this CPU (ISR interrupted us mid-operation), fall through to the
    // global locked path.  No CLI/STI needed — only this CPU touches
    // this flag, and the flag prevents ISR re-entry corruption.
    if cache.active {
        return ptr::null_mut();
    }
    cache.active = true;

    if cache.counts[class_idx] > 0 {
        // Fast path: pop from local cache.
        let slot_ptr = cache.heads[class_idx] as *mut FreeSlot;
        // SAFETY: slot_ptr is non-null (count > 0 means head is valid).
        // It points to HHDM-mapped frame memory owned by this allocator.
        cache.heads[class_idx] = unsafe { (*slot_ptr).next } as usize;
        cache.counts[class_idx] -= 1;
        // OPT: Per-CPU counter — plain increment, no `lock` prefix.
        cache.slab_allocs += 1;
        cache.active = false;
        let ptr = slot_ptr.cast::<u8>();
        // Slab poisoning: verify free-poison integrity (UAF detection),
        // then fill with alloc-poison (uninitialized-read detection).
        if POISON_ENABLED.load(Ordering::Relaxed) {
            let class_size = SIZE_CLASSES[class_idx];
            unsafe { check_poison(ptr, class_size); }
            unsafe { poison_alloc(ptr, class_size); }
        }
        return ptr;
    }

    // Slow path: batch refill from global allocator.
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
        // OPT: Per-CPU counter (slow path but still per-CPU).
        cache.slab_allocs += 1;
        cache.active = false;
        let ptr = slot_ptr.cast::<u8>();
        if POISON_ENABLED.load(Ordering::Relaxed) {
            let class_size = SIZE_CLASSES[class_idx];
            unsafe { check_poison(ptr, class_size); }
            unsafe { poison_alloc(ptr, class_size); }
        }
        ptr
    } else {
        // Couldn't get any slots (OOM).
        cache.active = false;
        ptr::null_mut()
    }
}

/// Try to free to the per-CPU slab cache.
///
/// Returns `true` if the slot was cached locally, `false` if the
/// per-CPU path couldn't handle it (ISR re-entry; caller should fall
/// through to the global locked path).
///
/// # Safety
///
/// `ptr` must have been allocated from the slab for `class_idx`.
#[inline(always)]
#[allow(clippy::cast_ptr_alignment)]
unsafe fn pcpu_slab_dealloc(ptr: *mut u8, class_idx: usize) -> bool {
    let cpu = crate::smp::fast_cpu_index();

    // SAFETY: `cpu` is valid.  Only this CPU accesses its cache.
    let cache = unsafe { &mut PCPU_SLAB_CACHES[cpu] };

    // Reentrancy guard: if ISR interrupted us mid-operation, fall
    // through to global locked path.
    if cache.active {
        return false;
    }
    cache.active = true;

    // Slab poisoning: fill freed slot with FREE_POISON before linking
    // into the free list.  Must happen before writing the next pointer.
    if POISON_ENABLED.load(Ordering::Relaxed) {
        let class_size = SIZE_CLASSES[class_idx];
        unsafe { poison_free(ptr, class_size); }
    }

    if cache.counts[class_idx] < PCPU_SLAB_MAX as u16 {
        // Fast path: push to local cache.
        let slot = ptr.cast::<FreeSlot>();
        // SAFETY: ptr is a valid slab slot (caller guarantee).
        unsafe { (*slot).next = cache.heads[class_idx] as *mut FreeSlot; }
        cache.heads[class_idx] = slot as usize;
        cache.counts[class_idx] += 1;
        // OPT: Per-CPU counter — plain increment, no `lock` prefix.
        cache.slab_frees += 1;
        cache.active = false;
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
    // OPT: Per-CPU counter (slow path but still per-CPU).
    cache.slab_frees += 1;
    cache.active = false;
    true
}

// ---------------------------------------------------------------------------
// GlobalAlloc implementation
// ---------------------------------------------------------------------------

unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // OPT: Compute the size class once (O(1) via bit ops) and reuse
        // the result for both the per-CPU fast path and the global slow
        // path.  Previously this was called up to 3 times per allocation.
        let class_idx = HeapInner::size_class_index(&layout);

        // Per-CPU slab cache fast path (lock-free).
        // Stats are counted per-CPU inside pcpu_slab_alloc (plain add,
        // no `lock` prefix), aggregated by stats() on demand.
        if PCPU_SLAB_ENABLED.load(Ordering::Relaxed) {
            if let Some(idx) = class_idx {
                // SAFETY: idx is valid (checked by size_class_index),
                // heap is initialized (PCPU_SLAB_ENABLED is set after init).
                let ptr = unsafe { pcpu_slab_alloc(idx) };
                if !ptr.is_null() {
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

        let ptr = match class_idx {
            Some(idx) => inner.slab_alloc(idx),
            None => inner.large_alloc(&layout),
        };

        if ptr.is_null() {
            ALLOC_FAILURES.fetch_add(1, Ordering::Relaxed);
        } else if let Some(idx) = class_idx {
            SLAB_ALLOCS.fetch_add(1, Ordering::Relaxed);
            if let Some(counter) = CLASS_ALLOCS.get(idx) {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            LARGE_ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }

        // OPT: Compute size class once and reuse for both per-CPU and
        // global paths.  Previously this was computed twice when falling
        // through from the per-CPU fast path to the global slow path.
        let class_idx = HeapInner::size_class_index(&layout);

        // Per-CPU slab cache fast path.
        // OPT: No atomic counter on this path — the per-CPU
        // cache.slab_frees counter (plain increment, no lock prefix)
        // tracks frees without cross-CPU cache line bouncing.
        // CLASS_FREES is only incremented on the global slow path.
        if PCPU_SLAB_ENABLED.load(Ordering::Relaxed) {
            if let Some(idx) = class_idx {
                // SAFETY: ptr was allocated from slab for this class.
                if unsafe { pcpu_slab_dealloc(ptr, idx) } {
                    return;
                }
            }
        }

        // Global locked path.
        let mut inner = self.inner.lock();
        if !inner.initialized {
            return;
        }

        match class_idx {
            Some(idx) => {
                inner.slab_dealloc(ptr, idx);
                SLAB_FREES.fetch_add(1, Ordering::Relaxed);
                if let Some(counter) = CLASS_FREES.get(idx) {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
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

/// Read heap allocator statistics.
///
/// Aggregates per-CPU slab counters (lock-free, plain reads) with the
/// global atomic counters for large allocs, refills, and failures.
/// The per-CPU counters may be slightly stale (no cross-CPU fence)
/// but are accurate for diagnostic/reporting purposes.
#[allow(clippy::arithmetic_side_effects)]
pub fn stats() -> HeapStats {
    // Aggregate per-CPU slab counters.
    // SAFETY: We read each PerCpuSlabCache's counters.  These are
    // plain u64 values, but races are benign — we're just reporting
    // approximate stats.  On x86, aligned u64 reads are atomic.
    let mut pcpu_allocs = 0u64;
    let mut pcpu_frees = 0u64;
    let online = crate::smp::cpu_count().max(1);
    for cpu in 0..online {
        // SAFETY: cpu < HEAP_MAX_CPUS (cpu_count is bounded by SMP init).
        let cache = unsafe { &PCPU_SLAB_CACHES[cpu] };
        pcpu_allocs = pcpu_allocs.saturating_add(cache.slab_allocs);
        pcpu_frees = pcpu_frees.saturating_add(cache.slab_frees);
    }

    HeapStats {
        slab_allocs: pcpu_allocs + SLAB_ALLOCS.load(Ordering::Relaxed),
        slab_frees: pcpu_frees + SLAB_FREES.load(Ordering::Relaxed),
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

/// Per-size-class statistics for leak detection and profiling.
#[derive(Debug, Clone, Copy)]
pub struct SlabClassStats {
    /// Size of objects in this class (bytes).
    pub class_size: usize,
    /// Total allocations from this class since boot.
    pub allocs: u64,
    /// Total frees to this class since boot.
    pub frees: u64,
    /// Currently active (in-use) objects: allocs - frees.
    pub active: u64,
}

/// Read per-size-class slab statistics.
///
/// Returns an array of 11 entries (one per size class, from 8B to 8192B).
/// The `active` field (allocs - frees) indicates how many objects of
/// that size are currently alive.  A steadily growing `active` count
/// suggests a memory leak in that size class.
#[must_use]
#[allow(dead_code)] // Public diagnostic API.
pub fn class_stats() -> [SlabClassStats; NUM_CLASSES] {
    let mut result = [SlabClassStats {
        class_size: 0,
        allocs: 0,
        frees: 0,
        active: 0,
    }; NUM_CLASSES];

    for (i, entry) in result.iter_mut().enumerate() {
        let allocs = CLASS_ALLOCS.get(i)
            .map_or(0, |c| c.load(Ordering::Relaxed));
        let frees = CLASS_FREES.get(i)
            .map_or(0, |c| c.load(Ordering::Relaxed));
        entry.class_size = SIZE_CLASSES.get(i).copied().unwrap_or(0);
        entry.allocs = allocs;
        entry.frees = frees;
        entry.active = allocs.saturating_sub(frees);
    }

    result
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

/// Self-test for slab poisoning (use-after-free detection).
///
/// Tests:
/// 1. Normal alloc/free cycle with poison enabled — no violations.
/// 2. Simulated use-after-free (write to freed memory) — detected.
/// 3. Verify violation counter increments.
///
/// Helper: corrupt a freed slot and re-allocate to trigger detection.
///
/// Isolated in a separate function to prevent thin LTO from optimizing
/// across the full dealloc→corrupt→alloc sequence.  When the entire
/// sequence is visible to the optimizer, it can prove that check_poison's
/// read_volatile calls will return the values written by poison_free
/// (ignoring the corruption write through a provenance-less pointer).
#[inline(never)]
fn corrupt_and_realloc(slot_addr: usize, layout: Layout) -> *mut u8 {
    // SAFETY: slot_addr points to a 64-byte slab slot that was just freed
    // with interrupts disabled (no reuse possible).  The inline asm write
    // is the only reliable way to corrupt memory under full LTO — LLVM
    // cannot see through it or reason about its effects.
    unsafe {
        core::arch::asm!(
            "mov byte ptr [{ptr} + 16], 0xBA",
            ptr = in(reg) slot_addr,
        );
    }
    // Allocate — LIFO guarantees we get the same slot back.
    unsafe { alloc::alloc::alloc(layout) }
}

pub fn poison_self_test() {
    serial_println!("[heap] Running slab poison self-test...");

    // Enable poisoning for the test.
    let was_enabled = POISON_ENABLED.load(Ordering::Relaxed);
    POISON_ENABLED.store(true, Ordering::Relaxed);
    let violations_before = POISON_VIOLATIONS.load(Ordering::Relaxed);

    // Both tests run with interrupts disabled to ensure LIFO slot reuse.
    // The per-CPU cache returns the most-recently-freed slot on the next
    // alloc of the same size class — but only if no ISR steals it first.
    let layout = Layout::from_size_align(64, 8).unwrap();
    unsafe { crate::cpu::cli(); }

    // --- Test 1: Normal cycle (no false positives) ---
    //
    // "Warmup" cycle: prime a slot with the poison magic.  The first
    // alloc from the per-CPU cache may grab a virgin slot (from batch
    // refill) that was never freed through the poison path.
    let warmup = unsafe { alloc::alloc::alloc(layout) };
    assert!(!warmup.is_null(), "poison test: warmup alloc failed");
    unsafe { alloc::alloc::dealloc(warmup, layout); }
    let violations_after_warmup = POISON_VIOLATIONS.load(Ordering::Relaxed);

    // Now alloc → write → dealloc → realloc.  The realloc should NOT
    // trigger a violation (poison was written on free and not disturbed).
    let p1 = unsafe { alloc::alloc::alloc(layout) };
    assert!(!p1.is_null(), "poison test: alloc failed");
    unsafe { p1.write_bytes(0x42, 64); }
    unsafe { alloc::alloc::dealloc(p1, layout); }
    let p2 = unsafe { alloc::alloc::alloc(layout) };
    assert!(!p2.is_null(), "poison test: realloc failed");
    let violations_after_normal = POISON_VIOLATIONS.load(Ordering::Relaxed);
    assert_eq!(
        violations_after_normal, violations_after_warmup,
        "poison test: unexpected violation on clean alloc/free cycle"
    );
    serial_println!("[heap]   Clean alloc/free/realloc: OK (no false positives)");

    // --- Test 2: UAF detection ---
    //
    // Free p2 (primes it with poison magic), then corrupt it, then
    // realloc — the corruption should be detected.  p2 is guaranteed
    // to have been through poison_free (just happened above in Test 1).
    //
    // Save the address as usize BEFORE freeing — after dealloc, the
    // pointer's provenance is invalid and LLVM may optimize writes
    // through it even with write_volatile.  The usize round-trip
    // breaks provenance tracking so the store is guaranteed.
    let p2_addr = p2 as usize;
    unsafe { alloc::alloc::dealloc(p2, layout); }
    let violations_pre_uaf = POISON_VIOLATIONS.load(Ordering::Relaxed);
    // BAD: simulate use-after-free by writing to the freed slot.
    // Offset 16 is inside the poison zone (bytes 12..class_size).
    // The usize→ptr cast + write_volatile ensures the compiler cannot
    // eliminate this store regardless of optimization level.
    // Delegate corruption + re-alloc to a separate non-inlined function.
    // This isolates the UAF simulation from the dealloc above, preventing
    // thin LTO from performing whole-sequence optimization across the
    // dealloc→corrupt→alloc boundary.
    let p4 = corrupt_and_realloc(p2_addr, layout);
    assert!(!p4.is_null(), "poison test: realloc for UAF test failed");
    let violations_after_uaf = POISON_VIOLATIONS.load(Ordering::Relaxed);
    assert_eq!(
        violations_after_uaf,
        violations_pre_uaf + 1,
        "poison test: UAF not detected (violations didn't increment)"
    );
    unsafe { alloc::alloc::dealloc(p4, layout); }
    serial_println!("[heap]   Use-after-free detection: OK (violation caught)");

    unsafe { crate::cpu::sti(); }

    // Restore previous state.
    POISON_ENABLED.store(was_enabled, Ordering::Relaxed);

    serial_println!("[heap] Slab poison self-test PASSED");
}

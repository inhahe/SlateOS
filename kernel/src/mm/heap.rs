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
//! Protected by a global spinlock.  Per-CPU caches will be added when
//! SMP support is implemented.
//!
//! ## Performance Target
//!
//! Common-size allocation: < 200ns (jemalloc: 20-50ns).
//! See `bench/baselines.toml` for measured targets.

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::serial_println;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use spin::Mutex;

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
    #[allow(clippy::indexing_slicing)]
    fn size_class_index(layout: &Layout) -> Option<usize> {
        // The class must be large enough for both the requested size
        // and alignment.  Since classes are powers of 2 and slots are
        // spaced at class_size intervals from a frame-aligned base,
        // using a class >= alignment guarantees proper alignment.
        let needed = layout.size().max(layout.align());
        if needed > MAX_SLAB_SIZE {
            return None;
        }
        for (i, &class_size) in SIZE_CLASSES.iter().enumerate() {
            if class_size >= needed {
                return Some(i);
            }
        }
        None
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
// GlobalAlloc implementation
// ---------------------------------------------------------------------------

unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut inner = self.inner.lock();
        if !inner.initialized {
            return ptr::null_mut();
        }

        match HeapInner::size_class_index(&layout) {
            Some(class_idx) => inner.slab_alloc(class_idx),
            None => inner.large_alloc(&layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }

        let mut inner = self.inner.lock();
        if !inner.initialized {
            return;
        }

        match HeapInner::size_class_index(&layout) {
            Some(class_idx) => inner.slab_dealloc(ptr, class_idx),
            // SAFETY: Caller guarantees ptr was allocated with this layout.
            None => unsafe { inner.large_dealloc(ptr, &layout) },
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
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

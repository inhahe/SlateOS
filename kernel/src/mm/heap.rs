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
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::{Mutex, MutexGuard};

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

/// Count of detected double-free attempts.
static DOUBLE_FREE_VIOLATIONS: AtomicU32 = AtomicU32::new(0);

/// Count of detected buffer overflows (red zone corruption).
static REDZONE_VIOLATIONS: AtomicU32 = AtomicU32::new(0);

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
///
/// Returns `true` if a double-free was detected.  When this returns
/// `true`, the caller must NOT push the slot back onto the free list
/// — re-adding an already-freed slot would corrupt the free list by
/// creating a cycle.  The slot is "leaked" intentionally: one leaked
/// slot is better than cascading corruption.
#[inline(never)]
unsafe fn poison_free(ptr: *mut u8, class_size: usize) -> bool {
    if class_size < 16 {
        return false; // Need at least 8 (next ptr) + 4 (magic) + some poison.
    }
    // Double-free detection: if the magic signature is already present,
    // this slot was freed before without being re-allocated in between.
    // Check using volatile reads (same reason as check_poison).
    // SAFETY: ptr is valid for class_size bytes (>= 16).
    let m0 = unsafe { core::ptr::read_volatile(ptr.add(8)) };
    let m1 = unsafe { core::ptr::read_volatile(ptr.add(9)) };
    let m2 = unsafe { core::ptr::read_volatile(ptr.add(10)) };
    let m3 = unsafe { core::ptr::read_volatile(ptr.add(11)) };
    if m0 == POISON_MAGIC[0] && m1 == POISON_MAGIC[1]
        && m2 == POISON_MAGIC[2] && m3 == POISON_MAGIC[3]
    {
        DOUBLE_FREE_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
        serial_println!(
            "[heap] DOUBLE-FREE detected! slot={:#x}, class={}",
            ptr as usize, class_size
        );
        return true;
    }

    // Write the magic signature at bytes 8..12 using volatile stores.
    // Volatile prevents the optimizer from dead-store-eliminating these
    // writes even with full LTO visibility.
    // SAFETY: ptr is valid for class_size bytes (>= 16), so offsets
    // 8..12 are in-bounds.  We have exclusive access to this slot
    // (it's being freed — the caller no longer uses it).
    unsafe {
        core::ptr::write_volatile(ptr.add(8), POISON_MAGIC[0]);
        core::ptr::write_volatile(ptr.add(9), POISON_MAGIC[1]);
        core::ptr::write_volatile(ptr.add(10), POISON_MAGIC[2]);
        core::ptr::write_volatile(ptr.add(11), POISON_MAGIC[3]);
    }
    // Fill bytes 12..class_size with FREE_POISON.
    for i in 12..class_size {
        // SAFETY: ptr is valid for class_size bytes and i < class_size,
        // so ptr.add(i) is in-bounds.  Volatile prevents DSE.
        unsafe {
            core::ptr::write_volatile(ptr.add(i), FREE_POISON);
        }
    }
    false
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

/// Check red zone integrity on dealloc.
///
/// The "red zone" is the gap between the user's requested size
/// (`alloc_size`) and the slab class size (`class_size`).  On alloc,
/// `poison_alloc` fills the entire slot with ALLOC_POISON (0xCD).
/// The user writes their data into bytes 0..alloc_size.  If any byte
/// in alloc_size..class_size is NOT 0xCD, the caller wrote past the
/// end of their allocation — a buffer overflow.
///
/// # Safety
///
/// `ptr` must point to a valid slab slot of `class_size` bytes.
#[inline(never)]
unsafe fn check_redzone(ptr: *mut u8, alloc_size: usize, class_size: usize) {
    // No red zone if the allocation fills the entire class.
    if alloc_size >= class_size {
        return;
    }
    // Minimum red zone: need at least 4 bytes to be meaningful
    // (avoids false positives from alignment padding the compiler adds).
    if class_size.saturating_sub(alloc_size) < 4 {
        return;
    }

    // Scan bytes from alloc_size to class_size for corruption.
    // Use volatile reads to prevent optimizer from constant-propagating.
    for i in alloc_size..class_size {
        // SAFETY: ptr is valid for class_size bytes (precondition) and
        // i < class_size, so ptr.add(i) is in-bounds.
        let byte = unsafe { core::ptr::read_volatile(ptr.add(i)) };
        if byte != ALLOC_POISON {
            REDZONE_VIOLATIONS.fetch_add(1, Ordering::Relaxed);
            serial_println!(
                "[heap] BUFFER OVERFLOW detected! slot={:#x}, alloc={}, class={}, offset={}",
                ptr as usize, alloc_size, class_size, i
            );
            // Only report once per dealloc — no need to scan the rest.
            return;
        }
    }
}

/// Validate a slab free-list `next` link (poison-debug builds only).
///
/// A freed slot stores its intrusive `next` pointer in bytes 0..8, but the
/// poison magic/fill only covers bytes 8..class_size — so an 8-byte
/// use-after-free write to a freed slot's first word corrupts `next` *without*
/// tripping [`check_poison`].  That silently splices a bad link into the free
/// list; the allocator then either follows a wild pointer or, if the write
/// happens to point at another live/free slot, hands out **aliased** memory,
/// which downstream corrupts a `BTreeMap`/`Vec` node and wedges the kernel in a
/// non-terminating traversal (see known-issues: the tmpwatch-self_test livelock
/// caught 2026-07-15).  Catching the bad link here converts that silent,
/// location-moving corruption into a loud, precise fault at the point of damage.
///
/// A valid link is either null or a pointer that is (a) in the higher-half
/// HHDM window, (b) aligned to `class_size` (every real slot is, since frames
/// are 16 KiB-aligned and `class_size` is a power of two ≤ 16 KiB), and (c) not
/// a self-cycle.  Returns `true` if the link is structurally valid.
///
/// This is O(1) (a few comparisons) and gated on `POISON_ENABLED`, so it costs
/// nothing in release builds.  It does not catch a perfect cycle between two
/// *valid* same-class slots, but the overwhelmingly common corruption — a stray
/// data value written over `next` — is caught immediately.
#[inline]
fn free_link_valid(link: *mut FreeSlot, slot: *mut FreeSlot, class_size: usize) -> bool {
    if link.is_null() {
        return true;
    }
    let addr = link as usize;
    // Higher-half kernel/HHDM addresses only; a stray heap-data value written
    // over `next` is almost always a low or unaligned value.
    if addr < 0xffff_8000_0000_0000 {
        return false;
    }
    if !addr.is_multiple_of(class_size) {
        return false;
    }
    // Immediate self-cycle: the list head points back at the node we just
    // popped (the simplest cycle, and the one a naive double-free re-add makes).
    if link == slot {
        return false;
    }
    true
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
    // SAFETY: ptr is valid for class_size bytes (>= 16, checked above),
    // so offsets 8..12 are in-bounds.  Read-only access, no aliasing.
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
        // SAFETY: ptr is valid for class_size bytes (precondition) and
        // i < class_size, so ptr.add(i) is in-bounds.
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

/// Current bytes in use (allocated but not yet freed).
///
/// Incremented by `layout.size()` on each successful allocation,
/// decremented on each deallocation.  Provides a live view of heap
/// memory pressure.
static BYTES_IN_USE: AtomicU64 = AtomicU64::new(0);

/// Peak (high-water mark) of `BYTES_IN_USE` since boot.
///
/// Updated via CAS loop on each allocation.  Represents the maximum
/// simultaneous heap consumption observed — useful for capacity planning
/// and detecting memory pressure spikes.
static PEAK_BYTES_IN_USE: AtomicU64 = AtomicU64::new(0);

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

/// Per-size-class cumulative bytes requested by callers.
///
/// `CLASS_BYTES_REQUESTED[i]` accumulates the actual `layout.size()`
/// values for allocations served by class `i`.  Combined with
/// `CLASS_ALLOCS[i] * SIZE_CLASSES[i]` (bytes consumed), this measures
/// internal fragmentation — memory wasted by rounding up to the next
/// power-of-2 class.
static CLASS_BYTES_REQUESTED: [AtomicU64; 11] = {
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
// Heap-lock owner instrumentation (deadlock diagnosis)
// ---------------------------------------------------------------------------
//
// The global heap lock (`HEAP.inner`) is a `spin::Mutex`, so a task that
// wedges while holding it hangs every other CPU the instant it next tries to
// allocate — the frozen-RIP capture from the NMI watchdog only ever names the
// *victim* spinning in `spin_loop_hint`, never the *holder*. These two atomics
// record, for the duration of each critical section, WHO holds the lock (the
// owning task-id) and WHERE it was acquired (a `&'static Location` pointer,
// stored as usize). They are updated with plain relaxed stores immediately
// after the lock is taken and cleared when the guard drops, so the liveness /
// NMI hang path can name the holder + acquire site directly.
//
// `u64::MAX` in OWNER means "unlocked". SITE is only meaningful while OWNER is
// not `u64::MAX`.

/// Task-id currently holding `HEAP.inner`, or `u64::MAX` when unlocked.
static HEAP_LOCK_OWNER: AtomicU64 = AtomicU64::new(u64::MAX);

/// `&'static Location` (as usize) of the acquire site of the current holder.
static HEAP_LOCK_SITE: AtomicUsize = AtomicUsize::new(0);

/// RAII guard wrapping the real `spin::MutexGuard` that records the heap-lock
/// owner + acquire site on acquisition and clears them on drop.
///
/// Derefs transparently to `HeapInner`, so call sites are unchanged apart from
/// swapping `.lock()` for `.lock_tracked()`.
struct TrackedGuard<'a> {
    guard: MutexGuard<'a, HeapInner>,
}

impl Deref for TrackedGuard<'_> {
    type Target = HeapInner;
    #[inline]
    fn deref(&self) -> &HeapInner {
        &self.guard
    }
}

impl DerefMut for TrackedGuard<'_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut HeapInner {
        &mut self.guard
    }
}

impl Drop for TrackedGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        // Clear the owner *before* the wrapped MutexGuard field drops (fields
        // drop after the Drop body), so the "unlocked" marker is published just
        // ahead of the physical lock release. A brief window where the lock is
        // held but marked unlocked is harmless for a diagnostic.
        HEAP_LOCK_OWNER.store(u64::MAX, Ordering::Relaxed);
        HEAP_LOCK_SITE.store(0, Ordering::Relaxed);
    }
}

impl KernelHeap {
    /// Acquire `inner`, recording the owning task-id and acquire site for the
    /// hang-diagnosis path. `#[track_caller]` captures the *call site* at
    /// compile time (zero runtime cost), so the recorded `Location` names the
    /// line that took the lock, not this helper.
    #[inline]
    #[track_caller]
    fn lock_tracked(&self) -> TrackedGuard<'_> {
        let guard = self.inner.lock();
        // Reading current_task_id() is lock-free (a per-CPU atomic load), so it
        // is safe to call from inside the allocator without re-entrancy.
        let tid = crate::sched::current_task_id();
        HEAP_LOCK_OWNER.store(tid, Ordering::Relaxed);
        HEAP_LOCK_SITE.store(
            core::panic::Location::caller() as *const _ as usize,
            Ordering::Relaxed,
        );
        TrackedGuard { guard }
    }
}

/// Dump the current holder of the global heap lock to the serial log.
///
/// Called from the liveness / NMI hang path. Lock-free (plain atomic loads),
/// so it is safe to call from a partially-wedged system or IRQ context. When
/// the lock is held, names the owning task-id and the `file:line` where it was
/// acquired — turning a "victim spinning in spin_loop_hint" capture into a
/// direct pointer at the deadlocking critical section.
pub fn dump_lock_owner() {
    let owner = HEAP_LOCK_OWNER.load(Ordering::Relaxed);
    if owner == u64::MAX {
        serial_println!("[liveness]   heap-lock: unlocked (no current holder)");
        return;
    }
    let site_ptr = HEAP_LOCK_SITE.load(Ordering::Relaxed);
    if site_ptr == 0 {
        serial_println!("[liveness]   heap-lock: HELD by tid={} (acquire site unknown)", owner);
        return;
    }
    // SAFETY: HEAP_LOCK_SITE, when non-zero, holds a `&'static Location`
    // pointer written by lock_tracked() from `Location::caller()`, which has
    // static lifetime. It is only cleared to 0 (checked above) on guard drop,
    // so a non-zero value is always a live &'static Location.
    let loc: &'static core::panic::Location<'static> =
        unsafe { &*(site_ptr as *const core::panic::Location<'static>) };
    serial_println!(
        "[liveness]   heap-lock: HELD by tid={} acquired at {}:{}:{}  <-- likely deadlock holder",
        owner,
        loc.file(),
        loc.line(),
        loc.column(),
    );
}

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
        super::memtype::charge(super::memtype::MemType::SlabHeap, 1);

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
        let next = unsafe { (*slot).next };
        // Free-list integrity: validate the link we're about to install as the
        // new head *before* trusting it.  A use-after-free that overwrote this
        // slot's `next` word escapes check_poison (which only covers bytes
        // 8..); catching it here stops the allocator from following a wild/
        // aliasing link and turns a silent, location-moving wedge into a precise
        // fault.  Debug-only (guarded by POISON_ENABLED).
        if POISON_ENABLED.load(Ordering::Relaxed)
            && !free_link_valid(next, slot, SIZE_CLASSES[class_idx])
        {
            serial_println!(
                "[heap] FREE-LIST CORRUPTION! class={} slot={:#x} bad next={:#x} \
                 (use-after-free overwrote the intrusive link)",
                SIZE_CLASSES[class_idx], slot as usize, next as usize,
            );
            // Sever the list rather than propagate the bad link: hand out this
            // slot but drop the corrupted tail (leak) so we neither loop nor
            // alias.  One leaked run of slots beats cascading corruption.
            self.free_lists[class_idx] = ptr::null_mut();
        } else {
            self.free_lists[class_idx] = next;
        }
        let ptr = slot.cast::<u8>();

        // Slab poisoning: check free-poison integrity, then alloc-poison.
        if POISON_ENABLED.load(Ordering::Relaxed) {
            let class_size = SIZE_CLASSES[class_idx];
            // SAFETY: ptr is a valid slab slot of class_size bytes
            // (just popped from the free list which holds HHDM-mapped slots).
            unsafe { check_poison(ptr, class_size); }
            // SAFETY: same ptr, same class_size — still valid.
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
        // If a double-free is detected, do NOT re-add the slot — it's
        // already on the free list and re-adding would create a cycle.
        if POISON_ENABLED.load(Ordering::Relaxed) {
            let class_size = SIZE_CLASSES[class_idx];
            // SAFETY: ptr is a valid slab slot of class_size bytes.
            let is_double_free = unsafe { poison_free(ptr, class_size) };
            if is_double_free {
                return; // Leak the slot intentionally to prevent corruption.
            }
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
            Ok(f) => {
                super::memtype::charge(
                    super::memtype::MemType::LargeHeap,
                    1u64 << order,
                );
                f.to_virt(self.hhdm_offset) as *mut u8
            }
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
            super::memtype::uncharge(
                super::memtype::MemType::LargeHeap,
                1u64 << order,
            );
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
// `class_idx` is a caller-guaranteed invariant (0..NUM_CLASSES, see # Safety),
// so every `cache.heads[class_idx]` / `cache.counts[class_idx]` / `SIZE_CLASSES`
// index is in bounds — same rationale as `slab_alloc`'s existing allow.
#[allow(clippy::indexing_slicing)]
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
        let class_size = SIZE_CLASSES[class_idx];
        // Fast path: pop from local cache.
        let slot_ptr = cache.heads[class_idx] as *mut FreeSlot;
        // SAFETY: slot_ptr is non-null (count > 0 means head is valid).
        // It points to HHDM-mapped frame memory owned by this allocator.
        let next = unsafe { (*slot_ptr).next };
        // Free-list integrity (see slab_alloc): validate the intrusive link a
        // use-after-free may have overwritten before installing it as the new
        // head.  The per-CPU count bounds the list length (so no infinite pop),
        // but a corrupted link still aliases live memory — catch it here.
        if POISON_ENABLED.load(Ordering::Relaxed)
            && !free_link_valid(next, slot_ptr, class_size)
        {
            serial_println!(
                "[heap] FREE-LIST CORRUPTION (pcpu)! class={} slot={:#x} bad next={:#x} \
                 (use-after-free overwrote the intrusive link)",
                class_size, slot_ptr as usize, next as usize,
            );
            // Sever: drop the rest of the per-CPU run (leak) rather than alias.
            cache.heads[class_idx] = 0;
            cache.counts[class_idx] = 1; // decremented to 0 below
        } else {
            cache.heads[class_idx] = next as usize;
        }
        cache.counts[class_idx] -= 1;
        // OPT: Per-CPU counter — plain increment, no `lock` prefix.
        cache.slab_allocs += 1;
        cache.active = false;
        let ptr = slot_ptr.cast::<u8>();
        // Slab poisoning: verify free-poison integrity (UAF detection),
        // then fill with alloc-poison (uninitialized-read detection).
        if POISON_ENABLED.load(Ordering::Relaxed) {
            // SAFETY: ptr (cast from slot_ptr) is a valid slab slot of
            // class_size bytes — it was on the per-CPU free list, which
            // only contains HHDM-mapped allocator-owned memory.
            unsafe { check_poison(ptr, class_size); }
            // SAFETY: same ptr, same class_size — still valid.
            unsafe { poison_alloc(ptr, class_size); }
        }
        return ptr;
    }

    // Slow path: batch refill from global allocator.
    let mut inner = HEAP.lock_tracked();
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
        // SAFETY: head is a valid FreeSlot (just read from global list).
        // Writing .next to re-link it into the per-CPU list is safe.
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
            // SAFETY: ptr (cast from slot_ptr) is a valid slab slot —
            // just transferred from the global free list to the per-CPU
            // cache.  All free-list entries are HHDM-mapped and owned
            // by the allocator.
            unsafe { check_poison(ptr, class_size); }
            // SAFETY: same ptr, same class_size — still valid.
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
    // If double-free is detected, skip re-adding — the slot is already
    // on a free list and re-adding would create a cycle.
    if POISON_ENABLED.load(Ordering::Relaxed) {
        let class_size = SIZE_CLASSES[class_idx];
        // SAFETY: ptr was allocated from the slab for class_idx (caller
        // guarantee), so it points to a valid slot of class_size bytes.
        let is_double_free = unsafe { poison_free(ptr, class_size) };
        if is_double_free {
            cache.active = false;
            return true; // "Handled" — leaked intentionally.
        }
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
    let mut inner = HEAP.lock_tracked();
    for _ in 0..PCPU_SLAB_BATCH {
        if cache.counts[class_idx] == 0 {
            break;
        }
        let slot_ptr = cache.heads[class_idx] as *mut FreeSlot;
        // SAFETY: slot_ptr is valid (count > 0).
        cache.heads[class_idx] = unsafe { (*slot_ptr).next } as usize;
        cache.counts[class_idx] -= 1;
        // Push to global free list.
        // SAFETY: slot_ptr is valid (popped from per-CPU cache above).
        // Writing .next to re-link it into the global free list is safe.
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
                    // Track bytes requested for fragmentation analysis.
                    if let Some(counter) = CLASS_BYTES_REQUESTED.get(idx) {
                        counter.fetch_add(layout.size() as u64, Ordering::Relaxed);
                    }
                    // Track bytes-in-use watermark.
                    let current = BYTES_IN_USE.fetch_add(layout.size() as u64, Ordering::Relaxed)
                        .saturating_add(layout.size() as u64);
                    let _ = PEAK_BYTES_IN_USE.fetch_update(
                        Ordering::Relaxed, Ordering::Relaxed,
                        |peak| if current > peak { Some(current) } else { None },
                    );
                    return ptr;
                }
                // Per-CPU path failed (OOM) — fall through to global.
            }
        }

        // Global locked path.
        let mut inner = self.lock_tracked();
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
            // Track bytes requested for fragmentation analysis.
            if let Some(counter) = CLASS_BYTES_REQUESTED.get(idx) {
                counter.fetch_add(layout.size() as u64, Ordering::Relaxed);
            }
            // Track bytes-in-use watermark.
            let current = BYTES_IN_USE.fetch_add(layout.size() as u64, Ordering::Relaxed)
                .saturating_add(layout.size() as u64);
            let _ = PEAK_BYTES_IN_USE.fetch_update(
                Ordering::Relaxed, Ordering::Relaxed,
                |peak| if current > peak { Some(current) } else { None },
            );
        } else {
            LARGE_ALLOCS.fetch_add(1, Ordering::Relaxed);
            // Track bytes-in-use watermark for large allocs.
            let current = BYTES_IN_USE.fetch_add(layout.size() as u64, Ordering::Relaxed)
                .saturating_add(layout.size() as u64);
            let _ = PEAK_BYTES_IN_USE.fetch_update(
                Ordering::Relaxed, Ordering::Relaxed,
                |peak| if current > peak { Some(current) } else { None },
            );
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

        // Red zone check: if poisoning is enabled, verify that bytes
        // between the user's allocation size and the class size still
        // contain ALLOC_POISON.  If not, the user overflowed their buffer.
        if POISON_ENABLED.load(Ordering::Relaxed) {
            if let Some(idx) = class_idx {
                let class_size = SIZE_CLASSES[idx];
                // SAFETY: ptr points to a valid slab slot of class_size bytes.
                unsafe { check_redzone(ptr, layout.size(), class_size); }
            }
        }

        // Per-CPU slab cache fast path.
        // OPT: No atomic counter on this path — the per-CPU
        // cache.slab_frees counter (plain increment, no lock prefix)
        // tracks frees without cross-CPU cache line bouncing.
        // CLASS_FREES is only incremented on the global slow path.
        if PCPU_SLAB_ENABLED.load(Ordering::Relaxed) {
            if let Some(idx) = class_idx {
                // SAFETY: ptr was allocated from slab for this class.
                if unsafe { pcpu_slab_dealloc(ptr, idx) } {
                    BYTES_IN_USE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
                    return;
                }
            }
        }

        // Global locked path.
        let mut inner = self.lock_tracked();
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
                BYTES_IN_USE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
            }
            // SAFETY: Caller guarantees ptr was allocated with this layout.
            None => {
                unsafe { inner.large_dealloc(ptr, &layout) };
                LARGE_FREES.fetch_add(1, Ordering::Relaxed);
                BYTES_IN_USE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
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
    /// Whether slab poisoning is currently enabled.
    pub poison_enabled: bool,
    /// Number of use-after-free violations detected.
    pub poison_violations: u32,
    /// Number of double-free violations detected.
    pub double_free_violations: u32,
    /// Number of buffer overflow (red zone) violations detected.
    pub redzone_violations: u32,
    /// Current bytes in use (allocated - freed).
    pub bytes_in_use: u64,
    /// Peak bytes in use since boot (high-water mark).
    pub peak_bytes_in_use: u64,
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
        poison_enabled: POISON_ENABLED.load(Ordering::Relaxed),
        poison_violations: POISON_VIOLATIONS.load(Ordering::Relaxed),
        double_free_violations: DOUBLE_FREE_VIOLATIONS.load(Ordering::Relaxed),
        redzone_violations: REDZONE_VIOLATIONS.load(Ordering::Relaxed),
        bytes_in_use: BYTES_IN_USE.load(Ordering::Relaxed),
        peak_bytes_in_use: PEAK_BYTES_IN_USE.load(Ordering::Relaxed),
    }
}

/// Read heap statistics without blocking (alias for [`stats`]).
///
/// Since the stats are atomic counters (no lock needed), this always
/// succeeds.  Provided for API consistency with [`frame::try_stats`].
#[allow(dead_code)] // API for diagnostics consumers; used once vmstat goes full-featured.
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

/// Per-size-class internal fragmentation statistics.
///
/// Internal fragmentation = bytes consumed by the allocator (class_size * count)
/// minus bytes actually requested by callers.  This is "wasted" memory due to
/// rounding up to the next power-of-2 class.  E.g., a 33-byte allocation uses
/// a 64-byte slot, wasting 31 bytes (48% fragmentation for that allocation).
#[derive(Debug, Clone, Copy)]
pub struct ClassFragStats {
    /// Size of objects in this class (bytes).
    pub class_size: usize,
    /// Total bytes requested by callers served by this class.
    pub bytes_requested: u64,
    /// Total bytes consumed (allocs * class_size).
    pub bytes_consumed: u64,
    /// Bytes wasted = consumed - requested.
    pub bytes_wasted: u64,
    /// Fragmentation percentage (0-100).  0 = no waste, 50 = half wasted.
    pub frag_pct: u8,
}

/// Read per-size-class internal fragmentation statistics.
///
/// Shows how much memory is wasted by rounding allocations up to the
/// nearest power-of-2 size class.  High fragmentation in a class
/// suggests many allocations just above the previous class boundary
/// (e.g., lots of 33-byte allocations landing in the 64-byte class).
///
/// Returns an array of 11 entries (one per size class, from 8B to 8192B).
#[must_use]
#[allow(dead_code)]
#[allow(clippy::arithmetic_side_effects)]
pub fn fragmentation_stats() -> [ClassFragStats; NUM_CLASSES] {
    let mut result = [ClassFragStats {
        class_size: 0,
        bytes_requested: 0,
        bytes_consumed: 0,
        bytes_wasted: 0,
        frag_pct: 0,
    }; NUM_CLASSES];

    for (i, entry) in result.iter_mut().enumerate() {
        let allocs = CLASS_ALLOCS.get(i)
            .map_or(0u64, |c| c.load(Ordering::Relaxed));
        let requested = CLASS_BYTES_REQUESTED.get(i)
            .map_or(0u64, |c| c.load(Ordering::Relaxed));
        let class_size = SIZE_CLASSES.get(i).copied().unwrap_or(0);
        let consumed = allocs.saturating_mul(class_size as u64);
        let wasted = consumed.saturating_sub(requested);

        let pct = if consumed > 0 {
            ((wasted * 100) / consumed).min(100) as u8
        } else {
            0
        };

        entry.class_size = class_size;
        entry.bytes_requested = requested;
        entry.bytes_consumed = consumed;
        entry.bytes_wasted = wasted;
        entry.frag_pct = pct;
    }

    result
}

// ---------------------------------------------------------------------------
// Leak detection
// ---------------------------------------------------------------------------

/// Per-class active-object counts from the previous leak check snapshot.
///
/// Updated each time `check_leaks()` is called.  If a class's active
/// count grows monotonically across N consecutive checks, it's flagged
/// as a potential leak.
static PREV_ACTIVE: [AtomicU64; 11] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; 11]
};

/// Per-class consecutive growth counter.
///
/// Incremented when a class's active count is higher than the previous
/// snapshot; reset to zero when it stays the same or decreases.
static GROWTH_STREAK: [AtomicU32; 11] = {
    const ZERO: AtomicU32 = AtomicU32::new(0);
    [ZERO; 11]
};

/// Result of a leak check.
#[derive(Debug, Clone, Copy)]
pub struct LeakCheckResult {
    /// Number of classes that show monotonic growth.
    pub suspect_classes: u8,
    /// Per-class details (class_size, active_objects, growth_streak).
    pub classes: [LeakClassInfo; NUM_CLASSES],
}

/// Leak status for a single size class.
#[derive(Debug, Clone, Copy)]
pub struct LeakClassInfo {
    /// Size class in bytes.
    pub class_size: usize,
    /// Current active objects (allocs - frees).
    pub active: u64,
    /// Net change since last check (positive = growth).
    pub delta: i64,
    /// Consecutive checks where active count has grown.
    pub growth_streak: u32,
}

/// Check for potential memory leaks.
///
/// Compares the current active-object count per size class against the
/// previous snapshot.  Classes where active counts grow monotonically
/// across multiple checks are flagged as potential leaks.
///
/// This is a heuristic, not definitive — transient growth (e.g., during
/// boot or workload ramp-up) will trigger false positives.  A class is
/// only "suspect" after growing for many consecutive checks (typically
/// called once per second from kswapd or a periodic timer).
///
/// Returns a summary with per-class growth streaks.
#[allow(dead_code)]
#[allow(clippy::arithmetic_side_effects)]
pub fn check_leaks() -> LeakCheckResult {
    let mut result = LeakCheckResult {
        suspect_classes: 0,
        classes: [LeakClassInfo {
            class_size: 0,
            active: 0,
            delta: 0,
            growth_streak: 0,
        }; NUM_CLASSES],
    };

    for i in 0..NUM_CLASSES {
        let allocs = CLASS_ALLOCS.get(i).map_or(0, |c| c.load(Ordering::Relaxed));
        let frees = CLASS_FREES.get(i).map_or(0, |c| c.load(Ordering::Relaxed));
        let active = allocs.saturating_sub(frees);
        let prev = PREV_ACTIVE.get(i).map_or(0, |c| c.load(Ordering::Relaxed));

        // Compute signed delta.
        let delta = if active >= prev {
            (active - prev) as i64
        } else {
            -((prev - active) as i64)
        };

        // Update growth streak.
        let streak = if active > prev && prev > 0 {
            // Growing — increment streak.
            GROWTH_STREAK.get(i).map_or(0, |c| c.fetch_add(1, Ordering::Relaxed) + 1)
        } else {
            // Stable or shrinking — reset streak.
            if let Some(c) = GROWTH_STREAK.get(i) {
                c.store(0, Ordering::Relaxed);
            }
            0
        };

        // Save current as the new "previous" for next check.
        if let Some(c) = PREV_ACTIVE.get(i) {
            c.store(active, Ordering::Relaxed);
        }

        let class_size = SIZE_CLASSES.get(i).copied().unwrap_or(0);

        result.classes[i] = LeakClassInfo {
            class_size,
            active,
            delta,
            growth_streak: streak,
        };

        // Flag as suspect if growth streak exceeds threshold.
        // 10 consecutive checks = ~10 seconds of monotonic growth.
        if streak >= 10 {
            result.suspect_classes += 1;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Heap integrity audit
// ---------------------------------------------------------------------------

/// Result of a heap integrity audit.
#[derive(Debug, Clone, Copy)]
pub struct HeapAuditResult {
    /// Total free slots counted across all size classes.
    pub total_free_slots: usize,
    /// Number of free slots with corrupted poison magic.
    pub corrupted_slots: usize,
    /// Number of classes where a free-list cycle was detected.
    pub cycles_detected: usize,
    /// Number of classes where a bad pointer was found.
    pub bad_pointers: usize,
    /// True if the audit passed with no issues.
    pub ok: bool,
}

/// Audit the heap's free lists for integrity.
///
/// Walks every size class's global free list (under the heap lock) and
/// checks:
/// 1. Each pointer is in a valid HHDM virtual range.
/// 2. No cycles exist (Floyd's tortoise/hare algorithm).
/// 3. If poisoning is enabled, the magic signature is intact.
///
/// This is a diagnostic tool for use from the kshell, not the hot path.
/// It takes the global heap lock for the duration — do not call from
/// latency-sensitive contexts.
#[allow(clippy::cast_ptr_alignment)]
pub fn audit_free_lists() -> HeapAuditResult {
    let inner = HEAP.lock_tracked();
    let poison_on = POISON_ENABLED.load(Ordering::Relaxed);

    let mut total_free: usize = 0;
    let mut corrupted: usize = 0;
    let mut cycles: usize = 0;
    let mut bad_ptrs: usize = 0;

    // HHDM valid range: hhdm_offset to (hhdm_offset + some reasonable max).
    // A pointer should be at minimum above hhdm_offset and not null.
    let hhdm_base = inner.hhdm_offset;

    for (class_idx, &head) in inner.free_lists.iter().enumerate() {
        if head.is_null() {
            continue;
        }

        let class_size = SIZE_CLASSES.get(class_idx).copied().unwrap_or(0);
        let mut slow = head;
        let mut fast = head;
        let mut count: usize = 0;
        let mut cycle_found = false;

        loop {
            // Validate slow pointer.
            let slow_addr = slow as usize;
            if slow_addr < hhdm_base as usize || slow.is_null() {
                bad_ptrs += 1;
                break;
            }

            count += 1;

            // Check poison integrity on this free slot.
            if poison_on && class_size >= 16 {
                let ptr = slow.cast::<u8>();
                // SAFETY: slot is in the free list, still owned by allocator,
                // and HHDM-mapped.  Reading bytes 8..12 is safe.
                let m0 = unsafe { core::ptr::read_volatile(ptr.add(8)) };
                let m1 = unsafe { core::ptr::read_volatile(ptr.add(9)) };
                let m2 = unsafe { core::ptr::read_volatile(ptr.add(10)) };
                let m3 = unsafe { core::ptr::read_volatile(ptr.add(11)) };
                if m0 != POISON_MAGIC[0] || m1 != POISON_MAGIC[1]
                    || m2 != POISON_MAGIC[2] || m3 != POISON_MAGIC[3]
                {
                    corrupted += 1;
                }
            }

            // Advance slow by 1.
            // SAFETY: slow is a valid HHDM pointer (checked above).
            slow = unsafe { (*slow).next };
            if slow.is_null() {
                break;
            }

            // Advance fast by 2 (for cycle detection).
            let fast_addr = fast as usize;
            if fast_addr < hhdm_base as usize || fast.is_null() {
                break;
            }
            // SAFETY: fast is non-null and above hhdm_base (checked above),
            // so it points to a valid HHDM-mapped FreeSlot.
            fast = unsafe { (*fast).next };
            if fast.is_null() {
                break;
            }
            let fast_addr2 = fast as usize;
            if fast_addr2 < hhdm_base as usize {
                bad_ptrs += 1;
                break;
            }
            // SAFETY: fast is non-null and above hhdm_base (checked above).
            fast = unsafe { (*fast).next };
            if fast.is_null() {
                break;
            }

            // Cycle check: if slow == fast, we have a loop.
            if core::ptr::eq(slow, fast) {
                cycle_found = true;
                cycles += 1;
                break;
            }

            // Safety limit: don't walk more than 1M entries.
            if count > 1_000_000 {
                break;
            }
        }

        if !cycle_found {
            total_free += count;
        }
    }

    let ok = corrupted == 0 && cycles == 0 && bad_ptrs == 0;
    HeapAuditResult { total_free_slots: total_free, corrupted_slots: corrupted, cycles_detected: cycles, bad_pointers: bad_ptrs, ok }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the kernel heap allocator.
///
/// Must be called after the frame allocator is initialized and before
/// any heap allocations are made.
pub fn init(hhdm_offset: u64) {
    let mut inner = HEAP.lock_tracked();
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
/// Helper: perform a double-free on a slot (already freed by caller).
///
/// Isolated to prevent LTO from optimizing across the pair of dealloc
/// calls.  Without this, LLVM constant-propagates through both calls
/// and sees that poison_free's volatile reads will match the magic
/// it itself wrote — eliminating the double-free detection branch.
#[inline(never)]
fn double_free_slot(ptr: *mut u8, layout: Layout) {
    // SAFETY: ptr was previously allocated with this layout.  This is
    // an intentional double-free to test detection — UB in normal code,
    // but the slab poison system is designed to catch and handle it.
    unsafe { alloc::alloc::dealloc(ptr, layout); }
}

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
    // SAFETY: layout is valid (constructed with from_size_align).
    // The global allocator is initialized.
    unsafe { alloc::alloc::alloc(layout) }
}

pub fn poison_self_test() {
    serial_println!("[heap] Running slab poison self-test...");

    // Enable poisoning for the test.
    let was_enabled = POISON_ENABLED.load(Ordering::Relaxed);
    POISON_ENABLED.store(true, Ordering::Relaxed);

    // Both tests run with interrupts disabled to ensure LIFO slot reuse.
    // The per-CPU cache returns the most-recently-freed slot on the next
    // alloc of the same size class — but only if no ISR steals it first.
    let layout = Layout::from_size_align(64, 8).unwrap();
    // SAFETY: disabling interrupts is required to ensure LIFO slot reuse
    // in the per-CPU slab cache (no ISR can steal the slot between free
    // and re-alloc).  Restored at the end of the test.
    unsafe { crate::cpu::cli(); }

    // --- Test 1: Normal cycle (no false positives) ---
    //
    // "Warmup" cycle: prime a slot with the poison magic.  The first
    // alloc from the per-CPU cache may grab a virgin slot (from batch
    // refill) that was never freed through the poison path.
    // SAFETY: layout is valid (64 bytes, align 8).  Allocator is initialized.
    let warmup = unsafe { alloc::alloc::alloc(layout) };
    assert!(!warmup.is_null(), "poison test: warmup alloc failed");
    // SAFETY: warmup was just allocated with this layout and is non-null.
    unsafe { alloc::alloc::dealloc(warmup, layout); }
    let violations_after_warmup = POISON_VIOLATIONS.load(Ordering::Relaxed);

    // Now alloc → write → dealloc → realloc.  The realloc should NOT
    // trigger a violation (poison was written on free and not disturbed).
    // SAFETY: layout is valid, allocator initialized.
    let p1 = unsafe { alloc::alloc::alloc(layout) };
    assert!(!p1.is_null(), "poison test: alloc failed");
    // SAFETY: p1 is non-null and points to 64 allocated bytes.
    unsafe { p1.write_bytes(0x42, 64); }
    // SAFETY: p1 was allocated with this layout.
    unsafe { alloc::alloc::dealloc(p1, layout); }
    // SAFETY: layout is valid, allocator initialized.
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
    // SAFETY: p2 was allocated with this layout and is non-null.
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
    // SAFETY: p4 was allocated by corrupt_and_realloc with this layout.
    unsafe { alloc::alloc::dealloc(p4, layout); }
    serial_println!("[heap]   Use-after-free detection: OK (violation caught)");

    // --- Test 3: Double-free detection ---
    //
    // Allocate, free, then free again.  The second free should detect
    // that the slot already has the poison magic (from the first free).
    // SAFETY: layout is valid, allocator initialized.
    let p5 = unsafe { alloc::alloc::alloc(layout) };
    assert!(!p5.is_null(), "poison test: alloc for double-free test failed");
    // SAFETY: p5 was allocated with this layout and is non-null.
    unsafe { alloc::alloc::dealloc(p5, layout); }
    let df_pre = DOUBLE_FREE_VIOLATIONS.load(Ordering::Relaxed);
    // Second free via isolated #[inline(never)] helper — prevents LTO
    // from optimizing across both dealloc calls.
    double_free_slot(p5, layout);
    let df_post = DOUBLE_FREE_VIOLATIONS.load(Ordering::Relaxed);
    assert_eq!(
        df_post,
        df_pre + 1,
        "poison test: double-free not detected"
    );
    // No cleanup needed: the double-free'd slot was NOT re-added to the
    // free list (poison_free returns true → dealloc skips the push).
    // The slot from the first free is still validly on the list.
    serial_println!("[heap]   Double-free detection: OK (violation caught)");

    // --- Test 4: Red zone (buffer overflow) detection ---
    //
    // Allocate 40 bytes (gets 64-byte class), write past byte 40 into
    // the red zone (bytes 40..64), then free.  The free should detect
    // that the red zone was corrupted.
    let layout40 = Layout::from_size_align(40, 8).unwrap();
    // SAFETY: layout40 is valid, allocator initialized.
    let p6 = unsafe { alloc::alloc::alloc(layout40) };
    assert!(!p6.is_null(), "poison test: alloc for redzone test failed");
    let rz_pre = REDZONE_VIOLATIONS.load(Ordering::Relaxed);
    // Simulate buffer overflow: write past the 40-byte allocation
    // into the red zone (byte 44 is in the gap between 40 and 64).
    // SAFETY: p6 points to a 64-byte slab slot (class size for 40-byte
    // alloc).  Offset 44 is within the slot but past the allocation —
    // intentional corruption for testing the red zone detector.
    unsafe { core::ptr::write_volatile(p6.add(44), 0x42); }
    // Free triggers red zone check — should detect corruption at byte 44.
    // SAFETY: p6 was allocated with layout40.
    unsafe { alloc::alloc::dealloc(p6, layout40); }
    let rz_post = REDZONE_VIOLATIONS.load(Ordering::Relaxed);
    assert_eq!(
        rz_post,
        rz_pre + 1,
        "poison test: red zone overflow not detected"
    );
    serial_println!("[heap]   Buffer overflow (red zone) detection: OK");

    // SAFETY: restoring the interrupt state disabled at the start of
    // this test.  All allocations have been freed.
    unsafe { crate::cpu::sti(); }

    // Restore previous state.
    POISON_ENABLED.store(was_enabled, Ordering::Relaxed);

    serial_println!("[heap] Slab poison self-test PASSED");
}

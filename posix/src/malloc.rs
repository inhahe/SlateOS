//! C dynamic memory allocation.
//!
//! Implements `malloc`, `free`, `calloc`, `realloc`, `reallocarray`,
//! `posix_memalign`, `aligned_alloc`, `memalign`, `valloc`, `pvalloc`,
//! `malloc_usable_size`, glibc's `__libc_*` aliases, and glibc's heap
//! statistics and trimming: `mallinfo`, `mallinfo2`, `malloc_stats`,
//! `malloc_trim`.
//!
//! ## The allocator is Doug Lea's
//!
//! The heap is dlmalloc — the allocator glibc's own descends from — in
//! Alex Crichton's Rust port, `dlmalloc-rs` 0.2.14, which is what Rust's
//! standard library uses on wasm. It is vendored in `malloc/dlmalloc.rs`; the
//! local changes are listed in `malloc/VENDORED.md`, and the two that matter
//! are that large requests get a mapping of their own (C dlmalloc's
//! `mmap_alloc`, which the Rust port does not carry) and that
//! address-contiguous regions are never merged.
//!
//! ## What it replaced, and why that mattered to every program
//!
//! Until 2026-09-25 every allocation was its own `mmap` region: one system
//! call for each `malloc` and each `free`, and at least one 16 KiB page per
//! allocation however small — a 10-byte string committed 16 KiB, and the
//! kernel printed a serial line for each map and unmap. Rust's `std` allocator
//! on this target *is* these functions (`System` calls `malloc`/`free` on
//! linux-musl), so that was the price of every `Box`, `Vec` and `String` in
//! the userland, not only of C programs. dlmalloc takes memory in 64 KiB
//! regions and carves them, so a small allocation costs its size plus 8 or 16
//! bytes and no system call at all.
//!
//! ## How memory reaches the kernel and comes back
//!
//! [`SlateSystem`] is dlmalloc's view of the system: anonymous mappings of
//! whole 16 KiB pages. Three properties of this kernel shape it:
//!
//! * **`munmap` releases a VMA record only when given its base.** So a region
//!   is only ever returned whole — the core never merges neighbouring regions
//!   into one segment, and [`SlateSystem::free_part`] declines to trim. A
//!   region is released when the core finds it wholly free; a request of
//!   256 KiB or more is its own mapping and goes back the moment it is freed.
//! * **`mremap` is not implemented** (`mman::mremap` answers `ENOSYS`), so
//!   growing a large block copies it; [`SlateSystem::remap`] says so.
//! * **Anonymous mappings arrive zeroed**, which lets `calloc` skip clearing a
//!   block that came fresh from the kernel.
//!
//! ## Size zero
//!
//! `malloc(0)`, `calloc(0, n)`, `posix_memalign(&p, a, 0)` and the other
//! aligned forms return a unique pointer that must be freed, as glibc and musl
//! both do. They returned NULL until 2026-09-25, which POSIX permits and
//! ported software does not expect: `p = malloc(n); if (!p) die("out of
//! memory")` dies on a legitimate `n == 0`. `realloc(p, 0)` frees `p` and
//! returns NULL, as glibc does. See `design-decisions.md` §1101.
//!
//! ## Threads and `fork`
//!
//! One heap, one lock ([`HeapGuard`]): a spin lock that yields after a short
//! spin, because the holder may have been preempted on a single CPU. `fork`
//! takes it before the system call and releases it in both processes after
//! ([`lock_for_fork`] and friends), so a child never starts with the lock held
//! by a thread that does not exist in it — the deadlock glibc's own
//! `__malloc_fork_lock_parent` prevents.
//!
//! `malloc` is not async-signal-safe, here as everywhere: a signal handler
//! that allocates while its thread holds the lock deadlocks.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

mod dlmalloc;

/// The page size every region is anchored on.
///
/// An alias of [`crate::unistd::PAGE_SIZE`], not an independent value — see the
/// doc comment there for why the number is written down exactly once.
const REGION_ALIGN: usize = crate::unistd::PAGE_SIZE;

// ---------------------------------------------------------------------------
// dlmalloc's system interface
// ---------------------------------------------------------------------------

/// How dlmalloc obtains and returns memory — `dlmalloc-rs`'s `Allocator` trait,
/// vendored with the core it serves (`malloc/VENDORED.md`). The documentation
/// of each method is upstream's.
///
/// # Safety
///
/// Implementations must return memory that is valid, writable and not in use
/// elsewhere for the whole of the size they report.
pub(crate) unsafe trait Allocator: Send {
    /// Allocates system memory region of at least `size` bytes
    /// Returns a triple of `(base, size, flags)` where `base` is a pointer to the beginning of the
    /// allocated memory region. `size` is the actual size of the region while `flags` specifies
    /// properties of the allocated region. If `EXTERN_BIT` (bit 0) set in flags, then we did not
    /// allocate this segment and so should not try to deallocate or merge with others.
    /// This function can return a `std::ptr::null_mut()` when allocation fails (other values of
    /// the triple will be ignored).
    fn alloc(&self, size: usize) -> (*mut u8, usize, u32);

    /// Remaps system memory region at `ptr` with size `oldsize` to a potential new location with
    /// size `newsize`. `can_move` indicates if the location is allowed to move to a completely new
    /// location, or that it is only allowed to change in size. Returns a pointer to the new
    /// location in memory.
    /// This function can return a `std::ptr::null_mut()` to signal an error.
    fn remap(&self, ptr: *mut u8, oldsize: usize, newsize: usize, can_move: bool) -> *mut u8;

    /// Frees a part of a memory chunk. The original memory chunk starts at `ptr` with size `oldsize`
    /// and is turned into a memory region starting at the same address but with `newsize` bytes.
    /// Returns `true` iff the access memory region could be freed.
    fn free_part(&self, ptr: *mut u8, oldsize: usize, newsize: usize) -> bool;

    /// Frees an entire memory region. Returns `true` iff the operation succeeded. When `false` is
    /// returned, the `dlmalloc` may re-use the location on future allocation requests
    fn free(&self, ptr: *mut u8, size: usize) -> bool;

    /// Indicates if the system can release a part of memory. For the `flags` argument, see
    /// `Allocator::alloc`
    fn can_release_part(&self, flags: u32) -> bool;

    /// Indicates whether newly allocated regions contain zeros.
    fn allocates_zeros(&self) -> bool;

    /// Returns the page size. Must be a power of two
    fn page_size(&self) -> usize;
}

/// dlmalloc's source of memory: whole-page anonymous mappings.
struct SlateSystem;

// SAFETY: `alloc` returns either NULL or a fresh mapping of exactly the size it
// reports, which nothing else uses; `free` releases exactly such a mapping.
unsafe impl Allocator for SlateSystem {
    fn alloc(&self, size: usize) -> (*mut u8, usize, u32) {
        let base = map_region(size);
        if base.is_null() {
            (core::ptr::null_mut(), 0, 0)
        } else {
            (base, size, 0)
        }
    }

    /// `mremap` is not implemented on this system (`mman::mremap` returns
    /// `ENOSYS`), so a large block that grows is copied instead.
    fn remap(&self, _ptr: *mut u8, _oldsize: usize, _newsize: usize, _can_move: bool) -> *mut u8 {
        core::ptr::null_mut()
    }

    /// Declined, always. Unmapping the tail of a region would free its frames
    /// but leave the kernel's VMA record claiming the whole original range,
    /// because native `munmap` drops a record only when handed its base
    /// address. dlmalloc treats a refusal as "nothing released" and stops
    /// retrying once it has been refused.
    fn free_part(&self, _ptr: *mut u8, _oldsize: usize, _newsize: usize) -> bool {
        false
    }

    fn free(&self, ptr: *mut u8, size: usize) -> bool {
        // SAFETY: dlmalloc hands back exactly the `(base, size)` pairs `alloc`
        // produced — the vendored core never merges regions (VENDORED.md).
        unsafe { unmap_region(ptr, size) };
        true
    }

    /// True, so that dlmalloc's pass that releases wholly free regions runs;
    /// the partial releases the same flag would permit are refused by
    /// `free_part`.
    fn can_release_part(&self, _flags: u32) -> bool {
        true
    }

    fn allocates_zeros(&self) -> bool {
        true
    }

    fn page_size(&self) -> usize {
        REGION_ALIGN
    }
}

// ---------------------------------------------------------------------------
// Backing store
// ---------------------------------------------------------------------------

/// Map `total` bytes of zeroed, writable anonymous memory; NULL on failure.
#[cfg(target_os = "none")]
fn map_region(total: usize) -> *mut u8 {
    let ptr = crate::mman::mmap(
        core::ptr::null_mut(),
        total,
        crate::mman::PROT_READ | crate::mman::PROT_WRITE,
        crate::mman::MAP_PRIVATE | crate::mman::MAP_ANONYMOUS,
        -1,
        0,
    );
    if ptr == crate::mman::MAP_FAILED {
        return core::ptr::null_mut();
    }
    ptr.cast::<u8>()
}

/// Release a region obtained from `map_region`.
///
/// # Safety
/// `base`/`total` must be exactly the pair a `map_region` call returned, and
/// the region must not have been released already.
#[cfg(target_os = "none")]
unsafe fn unmap_region(base: *mut u8, total: usize) {
    // A failed unmap of our own private anonymous mapping has no remedy here;
    // the region is reclaimed with the process in any case.
    let _ = crate::mman::munmap(base.cast::<core::ffi::c_void>(), total);
}

/// Host-build backing, so `cargo test` exercises this allocator instead of
/// silently skipping it.
///
/// `syscallN()` returns `-ENOSYS` on host builds — deliberately, see
/// syscall.rs's "Host-build safety gate" — so `mman::mmap` fails there, and
/// **every `malloc` on the host used to return NULL**, un-testing every libc
/// function that allocates without saying so. Backing the regions with the
/// Rust global allocator — page-aligned and zeroed, like a real mapping — is
/// what lets the whole suite run dlmalloc for real.
#[cfg(not(target_os = "none"))]
fn map_region(total: usize) -> *mut u8 {
    extern crate std;
    let Ok(layout) = std::alloc::Layout::from_size_align(total, REGION_ALIGN) else {
        return core::ptr::null_mut();
    };
    if layout.size() == 0 {
        return core::ptr::null_mut();
    }
    // SAFETY: non-zero size, checked just above — `alloc_zeroed`'s one
    // requirement. Zeroed, because `allocates_zeros` promises it.
    unsafe { std::alloc::alloc_zeroed(layout) }
}

/// Release a region obtained from `map_region`.
///
/// # Safety
/// `base`/`total` must be exactly the pair a `map_region` call returned, and
/// the region must not have been released already.
#[cfg(not(target_os = "none"))]
unsafe fn unmap_region(base: *mut u8, total: usize) {
    extern crate std;
    let Ok(layout) = std::alloc::Layout::from_size_align(total, REGION_ALIGN) else {
        return;
    };
    // SAFETY: the caller guarantees `base`/`total` name a live region from
    // `map_region`, and `Layout::from_size_align` is deterministic, so this is
    // the layout it was allocated with.
    unsafe { std::alloc::dealloc(base, layout) };
}

// ---------------------------------------------------------------------------
// The heap and its lock
// ---------------------------------------------------------------------------

/// The process's heap. One instance, shared by every thread under
/// [`HEAP_LOCK`]; on the host it is shared by every test thread, which is the
/// point — it is the allocator the suite runs on.
static HEAP: Heap = Heap(UnsafeCell::new(dlmalloc::Dlmalloc::new(SlateSystem)));

/// [`HEAP`]'s type: the heap in a cell that only a [`HeapGuard`] opens.
struct Heap(UnsafeCell<dlmalloc::Dlmalloc<SlateSystem>>);

// SAFETY: the cell's contents are reached only through `HeapGuard::heap`, and
// a `HeapGuard` exists only while it holds `HEAP_LOCK`, so no two threads ever
// hold a reference to the heap at once.
unsafe impl Sync for Heap {}

/// Held while [`HEAP`] is in use.
static HEAP_LOCK: AtomicBool = AtomicBool::new(false);

/// Ownership of [`HEAP`], released on drop.
struct HeapGuard;

impl HeapGuard {
    /// Take the heap. Spins briefly, then yields: on a single CPU the holder
    /// may have been preempted, and spinning then only delays its return.
    fn lock() -> Self {
        let mut spins: u32 = 0;
        while HEAP_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            if spins < 64 {
                spins = spins.saturating_add(1);
                core::hint::spin_loop();
            } else {
                yield_cpu();
            }
        }
        Self
    }

    /// The heap, for as long as this guard lives.
    #[allow(clippy::unused_self)] // the borrow of `self` is the point
    fn heap(&mut self) -> &mut dlmalloc::Dlmalloc<SlateSystem> {
        // SAFETY: a `HeapGuard` exists only while `HEAP_LOCK` is held by it, so
        // this is the only reference to `HEAP` anywhere, and it cannot outlive
        // the guard it is borrowed from.
        unsafe { &mut *HEAP.0.get() }
    }
}

impl Drop for HeapGuard {
    fn drop(&mut self) {
        HEAP_LOCK.store(false, Ordering::Release);
    }
}

fn yield_cpu() {
    #[cfg(target_os = "none")]
    {
        let _ = crate::pthread::sched_yield();
    }
    #[cfg(not(target_os = "none"))]
    {
        extern crate std;
        std::thread::yield_now();
    }
}

/// `fork`, before the system call: take the heap, so that no other thread of
/// this process can be halfway through changing it at the instant the address
/// space is copied.
///
/// Must be paired with [`unlock_after_fork_parent`] in the parent and
/// [`unlock_after_fork_child`] in the child.
pub(crate) fn lock_for_fork() {
    core::mem::forget(HeapGuard::lock());
}

/// `fork`, in the parent afterwards (and after a failed fork): release the heap.
pub(crate) fn unlock_after_fork_parent() {
    HEAP_LOCK.store(false, Ordering::Release);
}

/// `fork`, in the child afterwards: release the heap. The child has one
/// thread — the one that called `fork` and took the lock — so the heap is
/// consistent and the lock is simply given up.
pub(crate) fn unlock_after_fork_child() {
    HEAP_LOCK.store(false, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Test accounting
// ---------------------------------------------------------------------------

/// Per-thread count of blocks this thread allocated and has not yet freed.
///
/// Exists so a test can assert **"nothing leaked"** rather than assume it: a
/// function that reports an error after allocating — `regcomp` rejecting a bad
/// pattern part-way through the compile, say — returns exactly the same code
/// whether or not it freed what it had. Only a count distinguishes them.
///
/// It counted *regions* until 2026-09-25, when a region was an allocation.
/// dlmalloc keeps regions and carves many blocks from each, so the region
/// count stopped saying anything about a leak; blocks are what a leak is made
/// of. Thread-local and signed for the reasons it always was: libtest runs
/// tests in parallel in one process, and a block allocated on one thread and
/// freed on another legitimately drives the freeing thread's count negative.
#[cfg(all(test, not(target_os = "none")))]
pub(crate) mod live_allocations {
    extern crate std;
    use core::cell::Cell;

    std::thread_local! {
        static COUNT: Cell<i64> = const { Cell::new(0) };
    }

    /// Add `delta` to this thread's live-block count.
    pub(super) fn bump(delta: i64) {
        // `try_with`: a `free` from a thread-local destructor during thread
        // teardown must not panic after this cell has gone.
        let _ = COUNT.try_with(|c| c.set(c.get().wrapping_add(delta)));
    }

    /// This thread's live-block count. Only differences are meaningful.
    pub(crate) fn count() -> i64 {
        COUNT.with(Cell::get)
    }
}

/// Record a block handed out (tests only).
#[inline]
fn note_alloc(ptr: *mut u8) -> *mut u8 {
    #[cfg(all(test, not(target_os = "none")))]
    if !ptr.is_null() {
        live_allocations::bump(1);
    }
    ptr
}

/// Record a block returned (tests only).
#[inline]
fn note_free() {
    #[cfg(all(test, not(target_os = "none")))]
    live_allocations::bump(-1);
}

/// Test builds only: the byte a new `malloc` block is filled with.
///
/// The allocator this one replaced gave every block a fresh, zeroed mapping
/// and unmapped it again on `free`. So code in this crate that read a block
/// before writing it saw zeroes, and code that read a block after freeing it
/// faulted at once — and neither mistake could be caught, because neither
/// could fail. A heap that reuses memory changes both, silently. Filling
/// blocks with non-zero garbage at both ends of their life (glibc's
/// `MALLOC_PERTURB_`) turns both into test failures instead of heisenbugs on
/// the target.
#[cfg(all(test, not(target_os = "none")))]
const PERTURB_ON_ALLOC: u8 = 0xa5;

/// Test builds only: the byte a block is overwritten with as it is freed.
#[cfg(all(test, not(target_os = "none")))]
const PERTURB_ON_FREE: u8 = 0x5a;

/// Fill `len` bytes at `ptr` with `byte` (test builds; NULL is ignored).
///
/// # Safety
///
/// `ptr` must be NULL or valid for `len` bytes of writes.
#[cfg(all(test, not(target_os = "none")))]
unsafe fn perturb(ptr: *mut u8, len: usize, byte: u8) {
    if !ptr.is_null() {
        // SAFETY: the caller's contract.
        unsafe { core::ptr::write_bytes(ptr, byte, len) };
    }
}

// ---------------------------------------------------------------------------
// The C interface
// ---------------------------------------------------------------------------

/// The alignment every block has without being asked: 16 on x86_64, which is
/// what `max_align_t` requires.
const MALLOC_ALIGN: usize = 2 * core::mem::size_of::<usize>();

/// Allocate `size` bytes of uninitialised memory, 16-byte aligned.
///
/// `malloc(0)` returns a unique pointer, as glibc and musl do (see the module
/// docs). Returns NULL with `errno` set to `ENOMEM` when the memory cannot be
/// had, including for a size too large to represent with its bookkeeping.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn malloc(size: usize) -> *mut u8 {
    let mut guard = HeapGuard::lock();
    // SAFETY: the guard gives exclusive use of the heap.
    let ptr = unsafe { guard.heap().malloc(size) };
    drop(guard);
    if ptr.is_null() {
        crate::errno::set_errno(crate::errno::ENOMEM);
    }
    // SAFETY: a non-null `ptr` is a new block of at least `size` bytes.
    #[cfg(all(test, not(target_os = "none")))]
    unsafe {
        perturb(ptr, size, PERTURB_ON_ALLOC);
    }
    note_alloc(ptr)
}

/// Allocate zeroed memory for `nmemb` elements of `size` bytes each.
///
/// NULL with `ENOMEM` if the product overflows or the memory cannot be had.
/// A block that came straight from the kernel is already zero and is not
/// cleared again.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn calloc(nmemb: usize, size: usize) -> *mut u8 {
    let Some(total) = nmemb.checked_mul(size) else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return core::ptr::null_mut();
    };
    let mut guard = HeapGuard::lock();
    let heap = guard.heap();
    // SAFETY: the guard gives exclusive use of the heap, and a non-null result
    // is a block of at least `total` bytes that this call owns.
    let ptr = unsafe {
        let ptr = heap.malloc(total);
        if !ptr.is_null() && heap.calloc_must_clear(ptr) {
            core::ptr::write_bytes(ptr, 0, total);
        }
        ptr
    };
    drop(guard);
    if ptr.is_null() {
        crate::errno::set_errno(crate::errno::ENOMEM);
    }
    note_alloc(ptr)
}

/// Change the size of an allocated block, moving it if it must.
///
/// `realloc(NULL, n)` is `malloc(n)`; `realloc(p, 0)` frees `p` and returns
/// NULL, as glibc does. On failure the old block is untouched and still owned
/// by the caller, and `errno` is `ENOMEM`.
///
/// The result is 16-byte aligned; a block that was over-aligned by
/// `posix_memalign` keeps its bytes but not its alignment, as in C.
///
/// # Safety
///
/// `ptr` must be NULL or a live block from this allocator.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    if ptr.is_null() {
        return malloc(size);
    }
    if size == 0 {
        // SAFETY: the caller's contract.
        unsafe { free(ptr) };
        return core::ptr::null_mut();
    }
    let mut guard = HeapGuard::lock();
    // SAFETY (both blocks): the guard gives exclusive use of the heap; `ptr`
    // is a live block of it (the caller's contract).
    #[cfg(all(test, not(target_os = "none")))]
    let old_usable = unsafe { guard.heap().usable_size(ptr) };
    let moved = unsafe { guard.heap().realloc(ptr, size) };
    drop(guard);
    if moved.is_null() {
        crate::errno::set_errno(crate::errno::ENOMEM);
    }
    // The bytes a grown block gains are as uninitialised as a new block's.
    // SAFETY: a non-null `moved` is a block of at least `size` bytes, of which
    // the first `old_usable` (when fewer) are the old contents.
    #[cfg(all(test, not(target_os = "none")))]
    if !moved.is_null() && size > old_usable {
        unsafe { perturb(moved.add(old_usable), size - old_usable, PERTURB_ON_ALLOC) };
    }
    // One block in, one block out: the count only changes if the old block
    // survives a failure, and then it was already counted.
    moved
}

/// Free a block. `free(NULL)` does nothing.
///
/// # Safety
///
/// `ptr` must be NULL or a live block from this allocator.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    // `free` must not change `errno`, and releasing a region calls `munmap`.
    let saved = crate::errno::get_errno();
    let mut guard = HeapGuard::lock();
    // SAFETY (both blocks): the guard gives exclusive use of the heap; `ptr`
    // is a live block of it (the caller's contract), so its usable bytes are
    // still the caller's to overwrite until the `free` below.
    #[cfg(all(test, not(target_os = "none")))]
    unsafe {
        let usable = guard.heap().usable_size(ptr);
        perturb(ptr, usable, PERTURB_ON_FREE);
    }
    unsafe { guard.heap().free(ptr) };
    drop(guard);
    crate::errno::set_errno(saved);
    note_free();
}

/// The number of bytes the block at `ptr` can hold (GNU extension) — at least
/// what was asked for, and possibly more. 0 for NULL.
///
/// # Safety
///
/// `ptr` must be NULL or a live block from this allocator.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn malloc_usable_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }
    let mut guard = HeapGuard::lock();
    // SAFETY: the guard gives exclusive use of the heap; `ptr` is a live block
    // of it (the caller's contract).
    unsafe { guard.heap().usable_size(ptr) }
}

// ---------------------------------------------------------------------------
// Heap statistics and trimming (glibc's malloc.h extensions)
// ---------------------------------------------------------------------------

/// glibc's `struct mallinfo`: ten `int`s, 40 bytes.
///
/// The fields are C `int`s, so a heap past 2 GiB cannot be described in them;
/// glibc lets the values wrap, and this saturates at `i32::MAX` instead, so a
/// large heap reads as large rather than as some unrelated smaller number.
/// [`Mallinfo2`] has the same fields at full width.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mallinfo {
    /// Bytes in the heap's segments, the top chunk included.
    pub arena: i32,
    /// Free chunks.
    pub ordblks: i32,
    /// Free fastbin blocks: always 0, dlmalloc has no fastbins.
    pub smblks: i32,
    /// Separately mapped blocks: 0, as C dlmalloc reports (it does not count them).
    pub hblks: i32,
    /// Bytes in separately mapped blocks.
    pub hblkhd: i32,
    /// The largest the heap has been, in bytes.
    pub usmblks: i32,
    /// Bytes in free fastbin blocks: always 0.
    pub fsmblks: i32,
    /// Bytes in use.
    pub uordblks: i32,
    /// Bytes free.
    pub fordblks: i32,
    /// Bytes `malloc_trim` could release from the top of the heap.
    pub keepcost: i32,
}

/// glibc 2.33's `struct mallinfo2`: [`Mallinfo`]'s fields as `size_t`, 80 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mallinfo2 {
    /// Bytes in the heap's segments, the top chunk included.
    pub arena: usize,
    /// Free chunks.
    pub ordblks: usize,
    /// Free fastbin blocks: always 0.
    pub smblks: usize,
    /// Separately mapped blocks: 0, as C dlmalloc reports.
    pub hblks: usize,
    /// Bytes in separately mapped blocks.
    pub hblkhd: usize,
    /// The largest the heap has been, in bytes.
    pub usmblks: usize,
    /// Bytes in free fastbin blocks: always 0.
    pub fsmblks: usize,
    /// Bytes in use.
    pub uordblks: usize,
    /// Bytes free.
    pub fordblks: usize,
    /// Bytes `malloc_trim` could release from the top of the heap.
    pub keepcost: usize,
}

const _: () = {
    assert!(core::mem::size_of::<Mallinfo>() == 40);
    assert!(core::mem::size_of::<Mallinfo2>() == 80);
};

impl From<dlmalloc::HeapStats> for Mallinfo2 {
    fn from(st: dlmalloc::HeapStats) -> Self {
        Self {
            arena: st.arena,
            ordblks: st.ordblks,
            smblks: 0,
            hblks: 0,
            hblkhd: st.hblkhd,
            usmblks: st.usmblks,
            fsmblks: 0,
            uordblks: st.uordblks,
            fordblks: st.fordblks,
            keepcost: st.keepcost,
        }
    }
}

impl From<Mallinfo2> for Mallinfo {
    fn from(m: Mallinfo2) -> Self {
        let narrow = |v: usize| i32::try_from(v).unwrap_or(i32::MAX);
        Self {
            arena: narrow(m.arena),
            ordblks: narrow(m.ordblks),
            smblks: narrow(m.smblks),
            hblks: narrow(m.hblks),
            hblkhd: narrow(m.hblkhd),
            usmblks: narrow(m.usmblks),
            fsmblks: narrow(m.fsmblks),
            uordblks: narrow(m.uordblks),
            fordblks: narrow(m.fordblks),
            keepcost: narrow(m.keepcost),
        }
    }
}

/// A snapshot of the heap, taken under its lock so the fields agree.
fn heap_stats() -> dlmalloc::HeapStats {
    let mut guard = HeapGuard::lock();
    // SAFETY: the guard gives exclusive use of the heap; `stats` only reads it.
    unsafe { guard.heap().stats() }
}

/// `mallinfo2()` — the heap's statistics at full width (glibc 2.33+).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mallinfo2() -> Mallinfo2 {
    Mallinfo2::from(heap_stats())
}

/// `mallinfo()` — [`mallinfo2`] in C `int`s, saturating rather than wrapping.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mallinfo() -> Mallinfo {
    Mallinfo::from(mallinfo2())
}

/// `malloc_trim(pad)` — give free memory back to the system, keeping `pad`
/// bytes at the top of the heap. Returns 1 if anything was released, 0 if not.
///
/// What can go back is what `SlateSystem` can unmap: a whole segment none of
/// whose chunks is in use. The top of a segment still in use cannot be cut
/// off (`free_part` refuses, since native `munmap` releases whole mappings),
/// so glibc's partial release is not available here, and neither is its
/// `madvise` of free pages in the middle of the heap.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn malloc_trim(pad: usize) -> i32 {
    let mut guard = HeapGuard::lock();
    // SAFETY: the guard gives exclusive use of the heap.
    i32::from(unsafe { guard.heap().trim(pad) })
}

/// Format [`malloc_stats`]'s report into `out`, returning its length.
///
/// glibc's layout, without the two lines it ends with ("max mmap regions" and
/// "max mmap bytes"): dlmalloc does not keep those numbers, and a report that
/// printed zero for them would be wrong rather than incomplete.
fn format_malloc_stats(m: &Mallinfo2, out: &mut [u8; 256]) -> usize {
    struct Cursor<'a> {
        buf: &'a mut [u8; 256],
        len: usize,
    }
    impl core::fmt::Write for Cursor<'_> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let end = self.len.checked_add(s.len()).ok_or(core::fmt::Error)?;
            let dst = self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?;
            dst.copy_from_slice(s.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut c = Cursor { buf: out, len: 0 };
    let in_segments = m.arena.saturating_sub(m.fordblks);
    // A report that does not fit is cut short, never allowed to overrun; five
    // lines of at most 30 bytes fit 256 with room to spare.
    let _ = core::fmt::write(
        &mut c,
        format_args!(
            concat!(
                "Arena 0:\n",
                "system bytes     = {:>10}\n",
                "in use bytes     = {:>10}\n",
                "Total (incl. mmap):\n",
                "system bytes     = {:>10}\n",
                "in use bytes     = {:>10}\n",
            ),
            m.arena,
            in_segments,
            m.arena.saturating_add(m.hblkhd),
            m.uordblks,
        ),
    );
    c.len
}

/// `malloc_stats()` — print the heap's use to standard error, in glibc's
/// layout (see [`format_malloc_stats`] for the two lines it omits).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn malloc_stats() {
    let mut buf = [0u8; 256];
    let n = format_malloc_stats(&mallinfo2(), &mut buf);
    // Diagnostic output with nowhere to report its own failure, as in glibc.
    let _ = crate::file::write(2, buf.as_ptr(), n);
}

// ---------------------------------------------------------------------------
// Aligned allocation
// ---------------------------------------------------------------------------

/// A block of `size` bytes aligned to `alignment`, a power of two; NULL with
/// `ENOMEM` on failure.
fn aligned_block(alignment: usize, size: usize) -> *mut u8 {
    let mut guard = HeapGuard::lock();
    let heap = guard.heap();
    // SAFETY: the guard gives exclusive use of the heap. `memalign` requires a
    // power of two above the natural alignment, which the branch establishes
    // (callers have already checked `alignment` is a power of two).
    let ptr = unsafe {
        if alignment <= MALLOC_ALIGN {
            heap.malloc(size)
        } else {
            heap.memalign(alignment, size)
        }
    };
    drop(guard);
    if ptr.is_null() {
        crate::errno::set_errno(crate::errno::ENOMEM);
    }
    // SAFETY: a non-null `ptr` is a new block of at least `size` bytes.
    #[cfg(all(test, not(target_os = "none")))]
    unsafe {
        perturb(ptr, size, PERTURB_ON_ALLOC);
    }
    note_alloc(ptr)
}

/// Allocate `size` bytes aligned to `alignment` (POSIX `posix_memalign`).
///
/// `alignment` must be a power of two and a multiple of `sizeof(void *)`.
/// Returns 0 and stores the block in `*memptr`, or returns `EINVAL`/`ENOMEM`
/// without touching `*memptr` — the error is the return value, and `errno` is
/// left alone, as POSIX specifies.
///
/// `size == 0` stores a unique pointer, as glibc does.
///
/// # Safety
///
/// `memptr` must be NULL or valid for one pointer-sized write.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn posix_memalign(memptr: *mut *mut u8, alignment: usize, size: usize) -> i32 {
    // glibc's order (malloc/malloc.c, `__posix_memalign`): the alignment,
    // then the allocation, and only then the write through `memptr`, which
    // it does not check.  A NULL `memptr` is `EFAULT` here, this libc's
    // substitute for that write's fault (design-decisions.md §303), in its
    // place: after an `EINVAL` or `ENOMEM` that would have come first.
    // Until 2026-09-26 the NULL test came first.
    if alignment < core::mem::size_of::<usize>() || !alignment.is_power_of_two() {
        return crate::errno::EINVAL;
    }
    let saved = crate::errno::get_errno();
    let ptr = aligned_block(alignment, size);
    crate::errno::set_errno(saved);
    if ptr.is_null() {
        return crate::errno::ENOMEM;
    }
    if memptr.is_null() {
        // SAFETY: `ptr` is the block `aligned_block` just returned, which
        // nothing else has seen.
        unsafe { free(ptr) };
        return crate::errno::EFAULT;
    }
    // SAFETY: `memptr` was checked non-null and is writable (the caller's
    // contract).
    unsafe { *memptr = ptr };
    0
}

/// Allocate `size` bytes aligned to `alignment` (C11 `aligned_alloc`).
///
/// `alignment` must be a power of two; anything else is NULL with `EINVAL`.
/// C17 no longer requires `size` to be a multiple of `alignment`, and neither
/// does this. `size == 0` returns a unique pointer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aligned_alloc(alignment: usize, size: usize) -> *mut u8 {
    if !alignment.is_power_of_two() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }
    aligned_block(alignment, size)
}

/// Allocate aligned memory (obsolete, still used). As [`aligned_alloc`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn memalign(alignment: usize, size: usize) -> *mut u8 {
    aligned_alloc(alignment, size)
}

/// Allocate page-aligned memory (obsolete, still used).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn valloc(size: usize) -> *mut u8 {
    aligned_block(REGION_ALIGN, size)
}

/// Allocate page-aligned memory rounded up to a whole number of pages
/// (GNU/BSD extension). NULL with `ENOMEM` if the rounding overflows.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pvalloc(size: usize) -> *mut u8 {
    // `REGION_ALIGN` is the page size (an alias of `unistd::PAGE_SIZE`), so this
    // cannot drift from what `getpagesize()` reports.
    let Some(rounded) = size
        .checked_add(REGION_ALIGN.wrapping_sub(1))
        .map(|v| v & !REGION_ALIGN.wrapping_sub(1))
    else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return core::ptr::null_mut();
    };
    aligned_block(REGION_ALIGN, rounded)
}

/// Overflow-safe array reallocation: `realloc(ptr, nmemb * size)`, or NULL
/// with `ENOMEM` (and `ptr` untouched) if the product overflows.
///
/// # Safety
///
/// As [`realloc`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn reallocarray(ptr: *mut u8, nmemb: usize, size: usize) -> *mut u8 {
    let Some(total) = nmemb.checked_mul(size) else {
        crate::errno::set_errno(crate::errno::ENOMEM);
        return core::ptr::null_mut();
    };
    // SAFETY: forwarded; the product was checked above.
    unsafe { realloc(ptr, total) }
}

// ---------------------------------------------------------------------------
// glibc internal aliases
// ---------------------------------------------------------------------------
//
// Some programs call glibc's internal __libc_* symbols directly (e.g. when
// interposing malloc). These just delegate to our implementations.

/// glibc internal: `__libc_malloc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_malloc(size: usize) -> *mut u8 {
    malloc(size)
}

/// glibc internal: `__libc_free`.
///
/// # Safety
///
/// Same requirements as `free`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __libc_free(ptr: *mut u8) {
    unsafe {
        free(ptr);
    }
}

/// glibc internal: `__libc_realloc`.
///
/// # Safety
///
/// Same requirements as `realloc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __libc_realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    unsafe { realloc(ptr, size) }
}

/// glibc internal: `__libc_calloc`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_calloc(nmemb: usize, size: usize) -> *mut u8 {
    calloc(nmemb, size)
}

/// glibc internal: `__libc_memalign`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_memalign(alignment: usize, size: usize) -> *mut u8 {
    memalign(alignment, size)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    /// `valloc`/`pvalloc` promise "page-aligned", so their alignment must be
    /// the page size `unistd` reports.
    #[test]
    fn region_align_is_the_page_size() {
        assert_eq!(REGION_ALIGN, crate::unistd::PAGE_SIZE);
        assert_eq!(SlateSystem.page_size(), REGION_ALIGN);
    }

    /// `max_align_t` is 16 on x86_64, and every block must meet it unasked.
    #[test]
    fn plain_blocks_are_sixteen_byte_aligned() {
        assert_eq!(MALLOC_ALIGN, 16);
        let mut held = Vec::new();
        for size in [1usize, 7, 8, 15, 16, 17, 100, 1000, 4096, 70_000] {
            let p = malloc(size);
            assert!(!p.is_null(), "malloc({size})");
            assert_eq!(p as usize % MALLOC_ALIGN, 0, "malloc({size}) = {p:p}");
            held.push(p);
        }
        for p in held {
            unsafe { free(p) };
        }
    }

    // -----------------------------------------------------------------------
    // Size zero: a unique pointer, as glibc and musl
    // -----------------------------------------------------------------------

    /// Every allocating entry point answers size 0 with a distinct, freeable
    /// block. They all answered NULL until 2026-09-25, and these tests used to
    /// pin that.
    #[test]
    fn size_zero_is_a_unique_freeable_pointer_everywhere() {
        let mut got = Vec::new();
        got.push(malloc(0));
        got.push(calloc(0, 100));
        got.push(calloc(100, 0));
        got.push(calloc(0, 0));
        got.push(aligned_alloc(64, 0));
        got.push(memalign(16, 0));
        got.push(valloc(0));
        got.push(pvalloc(0));
        got.push(unsafe { realloc(core::ptr::null_mut(), 0) });
        got.push(reallocarray(core::ptr::null_mut(), 0, 100));
        got.push(__libc_malloc(0));
        got.push(__libc_calloc(0, 0));
        got.push(__libc_memalign(16, 0));
        let mut p = core::ptr::null_mut();
        assert_eq!(posix_memalign(&raw mut p, 32, 0), 0);
        got.push(p);
        for (i, a) in got.iter().enumerate() {
            assert!(!a.is_null(), "entry {i} returned NULL for size 0");
            for b in &got[..i] {
                assert_ne!(a, b, "size-0 blocks must be distinct");
            }
        }
        for p in got {
            unsafe { free(p) };
        }
    }

    /// Size 0 at every valid alignment: a unique, aligned pointer each time.
    /// (These asserted NULL for alignments up to 1 MiB until 2026-09-25.)
    #[test]
    fn size_zero_at_every_alignment() {
        for shift in 3..=20u32 {
            let align = 1usize << shift;
            let mut p: *mut u8 = 0x1234 as *mut u8;
            assert_eq!(
                posix_memalign(&raw mut p, align, 0),
                0,
                "posix_memalign({align}, 0)"
            );
            assert!(!p.is_null());
            assert_eq!(p as usize % align, 0, "posix_memalign({align}, 0) = {p:p}");
            unsafe { free(p) };
        }
        for shift in 0..=16u32 {
            let align = 1usize << shift;
            let p = aligned_alloc(align, 0);
            assert!(!p.is_null(), "aligned_alloc({align}, 0)");
            assert_eq!(p as usize % align, 0, "aligned_alloc({align}, 0) = {p:p}");
            unsafe { free(p) };
        }
    }

    /// The test-build fill is on: a new block is not zero, and neither are the
    /// bytes a grown block gains. If this fails, the fill was switched off and
    /// the suite has stopped catching code that reads memory before writing it.
    /// (The fill on `free` cannot be observed without reading freed memory.)
    #[test]
    fn test_builds_fill_new_blocks() {
        let p = malloc(64);
        assert!(!p.is_null());
        let bytes = unsafe { core::slice::from_raw_parts(p, 64) };
        assert!(bytes.iter().all(|&b| b == PERTURB_ON_ALLOC), "{bytes:?}");
        unsafe { core::ptr::write_bytes(p, 0, 64) };
        let old = unsafe { malloc_usable_size(p) };
        let q = unsafe { realloc(p, old + 4096) };
        assert!(!q.is_null());
        let head = unsafe { core::slice::from_raw_parts(q, 64) };
        assert!(head.iter().all(|&b| b == 0), "realloc kept the contents");
        let tail = unsafe { core::slice::from_raw_parts(q.add(old), 4096) };
        assert!(tail.iter().all(|&b| b == PERTURB_ON_ALLOC));
        unsafe { free(q) };
    }

    /// `realloc(p, 0)` frees and returns NULL, as glibc — not a new block.
    #[test]
    fn realloc_to_zero_frees() {
        let before = live_allocations::count();
        let p = malloc(40);
        let q = unsafe { realloc(p, 0) };
        assert!(q.is_null());
        assert_eq!(live_allocations::count(), before, "the block was freed");
    }

    // -----------------------------------------------------------------------
    // Refusals
    // -----------------------------------------------------------------------

    #[test]
    fn oversized_requests_fail_with_enomem() {
        for size in [usize::MAX, usize::MAX - 8, usize::MAX / 2] {
            crate::errno::set_errno(0);
            assert!(malloc(size).is_null(), "malloc({size:#x})");
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOMEM);
        }
        assert!(unsafe { realloc(core::ptr::null_mut(), usize::MAX) }.is_null());
        assert!(valloc(usize::MAX).is_null());
        assert!(pvalloc(usize::MAX).is_null());
        assert!(pvalloc(usize::MAX - 100).is_null());
    }

    #[test]
    fn calloc_overflow_is_enomem() {
        crate::errno::set_errno(0);
        assert!(calloc(usize::MAX, 2).is_null());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOMEM);
        assert!(calloc(usize::MAX / 2 + 1, 3).is_null());
        assert!(reallocarray(core::ptr::null_mut(), usize::MAX, 2).is_null());
    }

    #[test]
    fn free_and_usable_size_of_null() {
        unsafe { free(core::ptr::null_mut()) };
        unsafe { __libc_free(core::ptr::null_mut()) };
        assert_eq!(unsafe { malloc_usable_size(core::ptr::null_mut()) }, 0);
    }

    /// POSIX: `posix_memalign` reports through its return value and leaves
    /// `errno` alone, and a refusal leaves `*memptr` alone.
    #[test]
    fn posix_memalign_refusals() {
        assert_eq!(
            posix_memalign(core::ptr::null_mut(), 16, 100),
            crate::errno::EFAULT
        );
        // glibc tests the alignment before it ever writes through memptr.
        assert_eq!(
            posix_memalign(core::ptr::null_mut(), 3, 100),
            crate::errno::EINVAL
        );
        for bad in [0usize, 1, 2, 3, 4, 6, 12, 24] {
            let mut p: *mut u8 = 0x1234 as *mut u8;
            crate::errno::set_errno(0);
            assert_eq!(
                posix_memalign(&raw mut p, bad, 100),
                crate::errno::EINVAL,
                "{bad}"
            );
            assert_eq!(p, 0x1234 as *mut u8, "untouched on refusal");
            assert_eq!(crate::errno::get_errno(), 0, "errno untouched");
        }
    }

    #[test]
    fn aligned_alloc_refuses_a_non_power_of_two() {
        for bad in [0usize, 3, 6, 100] {
            crate::errno::set_errno(0);
            assert!(aligned_alloc(bad, 100).is_null(), "{bad}");
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
            assert!(memalign(bad, 100).is_null());
        }
    }

    // -----------------------------------------------------------------------
    // Alignment
    // -----------------------------------------------------------------------

    #[test]
    fn requested_alignments_are_honoured() {
        for shift in 3..=16u32 {
            let align = 1usize << shift;
            let mut p: *mut u8 = core::ptr::null_mut();
            assert_eq!(posix_memalign(&raw mut p, align, 1000), 0);
            assert_eq!(p as usize % align, 0, "posix_memalign({align})");
            // Writable for the whole request.
            unsafe { core::ptr::write_bytes(p, 0x5a, 1000) };
            assert!(unsafe { malloc_usable_size(p) } >= 1000);
            unsafe { free(p) };

            let q = aligned_alloc(align, align * 3);
            assert_eq!(q as usize % align, 0, "aligned_alloc({align})");
            unsafe { free(q) };
        }
        let v = valloc(100);
        assert_eq!(v as usize % REGION_ALIGN, 0);
        unsafe { free(v) };
        let pv = pvalloc(REGION_ALIGN + 1);
        assert_eq!(pv as usize % REGION_ALIGN, 0);
        assert!(unsafe { malloc_usable_size(pv) } >= 2 * REGION_ALIGN);
        unsafe { free(pv) };
    }

    // -----------------------------------------------------------------------
    // Contents
    // -----------------------------------------------------------------------

    /// A reused block is not zero; `calloc` must clear it. The old allocator
    /// never reused anything, so it could skip the clear — this one cannot.
    #[test]
    fn calloc_clears_a_reused_block() {
        let p = malloc(512);
        unsafe { core::ptr::write_bytes(p, 0xff, 512) };
        unsafe { free(p) };
        let q = calloc(64, 8);
        assert!(!q.is_null());
        let bytes = unsafe { core::slice::from_raw_parts(q, 512) };
        assert!(bytes.iter().all(|&b| b == 0), "calloc must return zeroes");
        unsafe { free(q) };
    }

    /// `realloc` keeps the bytes that fit, across the small/large boundary
    /// (256 KiB, where a block becomes a mapping of its own) in both
    /// directions.
    #[test]
    fn realloc_keeps_contents_across_the_mapping_threshold() {
        let fill = |p: *mut u8, n: usize| {
            for i in 0..n {
                unsafe { p.add(i).write((i % 251) as u8) };
            }
        };
        let check = |p: *const u8, n: usize| {
            for i in 0..n {
                assert_eq!(unsafe { p.add(i).read() }, (i % 251) as u8, "byte {i}");
            }
        };
        let p = malloc(1000);
        fill(p, 1000);
        let p = unsafe { realloc(p, 300 * 1024) };
        assert!(!p.is_null());
        check(p, 1000);
        fill(p, 300 * 1024);
        let p = unsafe { realloc(p, 600 * 1024) };
        check(p, 300 * 1024);
        let p = unsafe { realloc(p, 2000) };
        check(p, 2000);
        unsafe { free(p) };
    }

    #[test]
    fn a_large_block_round_trips() {
        let n = 3 * 1024 * 1024;
        let p = malloc(n);
        assert!(!p.is_null());
        assert!(unsafe { malloc_usable_size(p) } >= n);
        unsafe {
            p.write(1);
            p.add(n - 1).write(2);
            assert_eq!(p.read(), 1);
            assert_eq!(p.add(n - 1).read(), 2);
            free(p);
        }
    }

    #[test]
    fn reallocarray_sizes_by_the_product() {
        let ptr = reallocarray(core::ptr::null_mut(), 1, 1);
        assert!(!ptr.is_null(), "reallocarray(NULL, 1, 1) is malloc(1)");
        assert!(unsafe { malloc_usable_size(ptr) } >= 1);
        unsafe { free(ptr) };
        let ptr = reallocarray(core::ptr::null_mut(), 7, 9);
        assert!(!ptr.is_null());
        assert!(unsafe { malloc_usable_size(ptr) } >= 63);
        unsafe { free(ptr) };
    }

    /// `free` must not change `errno` — it may `munmap`, and a caller reading
    /// `errno` after cleaning up must still see its own failure.
    #[test]
    fn free_preserves_errno() {
        let big = malloc(1024 * 1024);
        crate::errno::set_errno(crate::errno::EIO);
        unsafe { free(big) };
        assert_eq!(crate::errno::get_errno(), crate::errno::EIO);
    }

    // -----------------------------------------------------------------------
    // The accounting the rest of the suite relies on
    // -----------------------------------------------------------------------

    #[test]
    fn live_allocations_counts_blocks() {
        let before = live_allocations::count();
        let a = malloc(10);
        let b = calloc(2, 2);
        let mut c = core::ptr::null_mut();
        assert_eq!(posix_memalign(&raw mut c, 64, 8), 0);
        assert_eq!(live_allocations::count(), before + 3);
        let a = unsafe { realloc(a, 5000) }; // one in, one out
        assert_eq!(live_allocations::count(), before + 3);
        unsafe {
            free(a);
            free(b);
            free(c);
        }
        assert_eq!(live_allocations::count(), before);
    }

    // -----------------------------------------------------------------------
    // Under load
    // -----------------------------------------------------------------------

    /// A deterministic mixed workload: many small blocks, some medium, a few
    /// large enough to be mappings of their own, freed and resized in a
    /// shuffled order, every byte checked against a pattern derived from the
    /// block's identity. A corrupted chunk header, a double hand-out or a bad
    /// realloc copy shows up as a pattern mismatch.
    #[test]
    fn a_mixed_workload_keeps_every_block_intact() {
        struct Block {
            ptr: *mut u8,
            len: usize,
            tag: u8,
        }
        fn fill(b: &Block) {
            for i in 0..b.len {
                unsafe { b.ptr.add(i).write(b.tag.wrapping_add(i as u8)) };
            }
        }
        fn check(b: &Block) {
            for i in 0..b.len {
                let got = unsafe { b.ptr.add(i).read() };
                assert_eq!(
                    got,
                    b.tag.wrapping_add(i as u8),
                    "block {:p} byte {i}",
                    b.ptr
                );
            }
        }
        let mut rng: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        let before = live_allocations::count();
        let mut live: Vec<Block> = Vec::new();
        for round in 0..6000u32 {
            let r = next();
            let choice = r % 10;
            if choice < 5 || live.is_empty() {
                let len = match r % 100 {
                    0..=79 => (r >> 8) as usize % 512 + 1,
                    80..=97 => (r >> 8) as usize % (64 * 1024) + 1,
                    _ => (r >> 8) as usize % (512 * 1024) + 256 * 1024,
                };
                let ptr = malloc(len);
                assert!(!ptr.is_null(), "round {round}: malloc({len})");
                assert_eq!(ptr as usize % MALLOC_ALIGN, 0);
                let b = Block {
                    ptr,
                    len,
                    tag: (round % 251) as u8,
                };
                fill(&b);
                live.push(b);
            } else if choice < 8 {
                let i = (r >> 16) as usize % live.len();
                let b = live.swap_remove(i);
                check(&b);
                unsafe { free(b.ptr) };
            } else {
                let i = (r >> 16) as usize % live.len();
                let new_len = (r >> 24) as usize % (128 * 1024) + 1;
                let b = &mut live[i];
                check(b);
                let moved = unsafe { realloc(b.ptr, new_len) };
                assert!(!moved.is_null());
                b.ptr = moved;
                // The surviving prefix must be intact; refill for the new size.
                let keep = b.len.min(new_len);
                for j in 0..keep {
                    let got = unsafe { moved.add(j).read() };
                    assert_eq!(got, b.tag.wrapping_add(j as u8), "realloc lost byte {j}");
                }
                b.len = new_len;
                fill(b);
            }
        }
        for b in live.drain(..) {
            check(&b);
            unsafe { free(b.ptr) };
        }
        assert_eq!(
            live_allocations::count(),
            before,
            "every block accounted for"
        );
    }

    /// Four threads allocating and freeing at once. The heap is one instance
    /// behind one lock; a missing or wrong lock corrupts it quickly under this.
    #[test]
    fn concurrent_threads_share_the_heap_safely() {
        let handles: Vec<_> = (0..4u8)
            .map(|t| {
                std::thread::spawn(move || {
                    let mut held: Vec<(usize, usize)> = Vec::new();
                    for i in 0..3000usize {
                        let len = (i * 37 + usize::from(t) * 11) % 3000 + 1;
                        let p = malloc(len);
                        assert!(!p.is_null());
                        unsafe { core::ptr::write_bytes(p, t, len) };
                        held.push((p as usize, len));
                        if held.len() > 64 {
                            let (q, n) = held.remove(i % held.len());
                            let q = q as *mut u8;
                            let bytes = unsafe { core::slice::from_raw_parts(q, n) };
                            assert!(
                                bytes.iter().all(|&b| b == t),
                                "thread {t} saw another's bytes"
                            );
                            unsafe { free(q) };
                        }
                    }
                    for (q, _) in held {
                        unsafe { free(q as *mut u8) };
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("an allocating thread panicked");
        }
    }

    /// The `fork` pair leaves the heap usable in the parent, and a lock taken
    /// for a fork does not stay taken.
    #[test]
    fn the_fork_lock_is_released() {
        lock_for_fork();
        // Read, release, *then* assert: a failed assertion with the heap still
        // locked would hang every other test in the process.
        let held = HEAP_LOCK.load(Ordering::Acquire);
        unlock_after_fork_parent();
        assert!(held, "held across the fork");
        let p = malloc(8);
        assert!(!p.is_null(), "the heap must be usable again");
        unsafe { free(p) };

        lock_for_fork();
        unlock_after_fork_child();
        let p = malloc(8);
        assert!(!p.is_null());
        unsafe { free(p) };
    }

    // -- mallinfo, mallinfo2, malloc_trim, malloc_stats --
    //
    // The heap is shared by the whole test suite, so these assert what holds of
    // one snapshot whatever other tests are doing: identities between the
    // fields, and lower bounds set by a block this test is holding.

    /// Every byte obtained from the system is counted once either way: by
    /// where it lives (segments + separate mappings) and by whether it is in
    /// use (used + free). Both sums are the footprint.
    #[test]
    fn mallinfo2_fields_account_for_the_footprint_both_ways() {
        let p = malloc(100);
        assert!(!p.is_null());
        let m = mallinfo2();
        unsafe { free(p) };
        assert_eq!(m.arena + m.hblkhd, m.uordblks + m.fordblks);
        assert!(m.ordblks >= 1, "the top chunk is always free");
        assert!(
            m.keepcost <= m.fordblks,
            "the top chunk is part of the free bytes"
        );
        assert!(
            m.usmblks >= m.arena + m.hblkhd,
            "the peak is at least the present"
        );
        assert_eq!(
            (m.smblks, m.hblks, m.fsmblks),
            (0, 0, 0),
            "dlmalloc keeps none"
        );
    }

    /// A block past the mapping threshold lives in a mapping of its own, and
    /// is counted there and as in use while it is held.
    #[test]
    fn a_large_block_is_counted_as_mapped_and_in_use() {
        const BIG: usize = 1 << 20;
        // A block gets a mapping of its own only once the heap exists (C
        // dlmalloc's rule): the very first allocation, however large, founds
        // the heap's first segment instead. So found it first, whatever order
        // the tests run in.
        let small = malloc(16);
        let p = malloc(BIG);
        assert!(!small.is_null() && !p.is_null());
        let m = mallinfo2();
        unsafe {
            free(p);
            free(small);
        }
        assert!(m.hblkhd >= BIG, "hblkhd {} < {BIG}", m.hblkhd);
        assert!(m.uordblks >= BIG, "uordblks {} < {BIG}", m.uordblks);
    }

    /// `mallinfo` is `mallinfo2` in `int`s: equal while the values fit,
    /// `i32::MAX` -- not a wrapped value -- once they do not.
    #[test]
    fn mallinfo_narrows_by_saturating() {
        let wide = Mallinfo2 {
            arena: 5,
            ordblks: 1,
            hblkhd: usize::MAX,
            uordblks: (1 << 31) + 7,
            fordblks: 3,
            keepcost: 2,
            ..Mallinfo2::default()
        };
        let narrow = Mallinfo::from(wide);
        assert_eq!(
            (
                narrow.arena,
                narrow.ordblks,
                narrow.fordblks,
                narrow.keepcost
            ),
            (5, 1, 3, 2)
        );
        assert_eq!(narrow.hblkhd, i32::MAX);
        assert_eq!(narrow.uordblks, i32::MAX);
        // And the real call reports a heap once there is one: before the
        // first allocation of the process there is none, and every field is
        // 0 -- which a test that happens to run first would see.
        let p = malloc(16);
        let m = mallinfo();
        unsafe { free(p) };
        assert!(m.ordblks >= 1 && m.arena > 0, "{m:?}");
    }

    /// Before the first allocation there is no heap to describe, and the
    /// report says so with zeros rather than inventing a top chunk.
    #[test]
    fn an_empty_heap_reports_nothing() {
        assert_eq!(
            Mallinfo2::from(dlmalloc::HeapStats::default()),
            Mallinfo2::default()
        );
    }

    /// `malloc_trim` answers 0 or 1 and leaves a working heap behind.
    #[test]
    fn malloc_trim_leaves_a_working_heap() {
        let blocks: std::vec::Vec<*mut u8> = (0..64).map(|i| malloc(1 + i * 97)).collect();
        for &b in &blocks {
            unsafe { free(b) };
        }
        let r = malloc_trim(0);
        assert!(r == 0 || r == 1, "{r}");
        let p = malloc(4096);
        assert!(!p.is_null());
        unsafe { free(p) };
        assert!(matches!(malloc_trim(1 << 20), 0 | 1));
    }

    #[test]
    fn malloc_stats_reports_in_glibcs_layout() {
        let m = Mallinfo2 {
            arena: 135_168,
            fordblks: 133_104,
            hblkhd: 1_052_672,
            uordblks: 1_054_736,
            ..Mallinfo2::default()
        };
        let mut buf = [0u8; 256];
        let n = format_malloc_stats(&m, &mut buf);
        assert_eq!(
            core::str::from_utf8(&buf[..n]).expect("ASCII"),
            concat!(
                "Arena 0:\n",
                "system bytes     =     135168\n",
                "in use bytes     =       2064\n",
                "Total (incl. mmap):\n",
                "system bytes     =    1187840\n",
                "in use bytes     =    1054736\n",
            )
        );
    }
}

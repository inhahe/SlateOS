//! Kernel virtual memory allocator (vmalloc).
//!
//! Allocates virtually-contiguous kernel memory backed by
//! physically-discontiguous frames.  Used for large allocations
//! that don't require physical contiguity (unlike DMA buffers).
//!
//! ## When to Use vmalloc vs. Other Allocators
//!
//! | Allocator | Physical contiguity | Size range | Use case |
//! |-----------|--------------------:|:-----------|----------|
//! | Slab heap | Yes (per-frame)     | 8 B – 8 KiB | Small kernel objects |
//! | Frame alloc | Yes (buddy)       | 16 KiB – 16 MiB | DMA, page tables |
//! | **vmalloc** | No               | 16 KiB – 1 GiB | Heap allocations above 16 MiB, large buffers |
//! | Huge page | Yes (2 MiB-aligned) | 2 MiB | Performance-critical large mappings |
//!
//! The kernel heap (`mm::heap`) sends every allocation whose buddy order
//! would exceed the frame allocator's maximum block (16 MiB) here, so an
//! ordinary `Vec` can be as large as free memory allows — which is what lets
//! a program bigger than 16 MiB be read in and started.
//!
//! ## Design
//!
//! The vmalloc region occupies a dedicated portion of kernel virtual
//! address space ([`kvspace::VMALLOC`](super::kvspace::VMALLOC):
//! 0xFFFF_C300_0000_0000, 1 GiB).  A bitmap tracks which virtual pages
//! within this region are allocated.
//!
//! Each vmalloc allocation:
//! 1. Finds N contiguous free virtual pages in the bitmap.
//! 2. Allocates N physical frames (individually, no contiguity needed).
//! 3. Maps each virtual page to its physical frame.
//! 4. Returns a pointer to the start of the contiguous virtual region.
//!
//! Freeing reverses this: unmap each page, flush the stale translations
//! from every CPU's TLB, free each frame, clear the bitmap.
//!
//! ## Visible in every address space
//!
//! Mappings are made through the kernel's own PML4
//! ([`page_table::kernel_pml4_phys`]), never the loaded one, and the
//! region's top-level page-table entry is created at boot by
//! `page_table::init` (it is listed in `kvspace::PAGE_TABLE_MAPPED`). Both
//! matter:
//!
//! - Every process PML4 copies the kernel's top-level entries by value when
//!   it is created. Had the first `vmalloc` created the region's entry, it
//!   would have existed only in whichever PML4 was loaded — every address
//!   space created before that moment would fault on vmalloc memory.
//! - A mapping made through a process PML4 is charged to that process's
//!   resident-set size by `mm::accounting`, so a kernel buffer allocated
//!   during a syscall would count against the caller (and be subtracted
//!   from whichever process happened to be running when it was freed).
//!
//! ## TLB flushing on free
//!
//! A freed page's translation may still sit in a TLB — the hardware caches
//! present entries whether or not software touched them. If the frame is
//! handed to someone else and the virtual range to the next `vmalloc`, a
//! CPU using the stale entry would read and write the *previous* frame. So
//! [`vfree`] (and a failed `vmalloc`'s rollback) shoot down the whole range
//! on every CPU before a single frame goes back to the allocator. Like any
//! TLB shootdown, that must not be done with interrupts disabled while
//! other CPUs are online: the shootdown waits for every CPU to acknowledge
//! an IPI.
//!
//! ## Guard Pages
//!
//! Each allocation is surrounded by unmapped guard pages to catch
//! out-of-bounds accesses.  This adds 2 pages of overhead per allocation
//! but provides immediate detection of buffer overflows/underflows.
//!
//! ## Thread Safety
//!
//! The bitmap and allocation metadata are protected by a spinlock that
//! disables preemption but not interrupts, like the kernel heap's: neither
//! may be called from interrupt context. Page-table work happens outside
//! the lock; each allocation owns its virtual range exclusively, and
//! `page_table` creates shared intermediate tables with a compare-and-swap,
//! so two allocations whose ranges share a page table cannot lose each
//! other's mappings.
//!
//! ## References
//!
//! - Linux `mm/vmalloc.c` — virtual kernel memory allocator
//! - Linux `include/linux/vmalloc.h` — vmalloc/vfree/vmap
//! - FreeBSD `kern/kern_malloc.c` — kernel_map allocations

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::mm::frame_owner::{Owner, OwnerScope};
use crate::mm::page_table::{self, HW_PAGES_PER_FRAME, PageFlags, VirtAddr};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Base virtual address of the vmalloc region
/// ([`kvspace::VMALLOC`](super::kvspace::VMALLOC)).
const VMALLOC_BASE: u64 = super::kvspace::VMALLOC.start;

/// Size of the vmalloc region in bytes (1 GiB = 65536 × 16 KiB frames).
#[allow(clippy::cast_possible_truncation)] // 1 GiB fits any 64-bit usize.
const VMALLOC_SIZE: usize = super::kvspace::VMALLOC.size as usize;

/// Maximum number of virtual pages (frames) in the vmalloc region.
const VMALLOC_MAX_PAGES: usize = VMALLOC_SIZE / FRAME_SIZE;

/// Maximum number of concurrent vmalloc allocations we track.
const MAX_VMALLOC_ENTRIES: usize = 256;

// ---------------------------------------------------------------------------
// Allocation metadata
// ---------------------------------------------------------------------------

/// Metadata for a single vmalloc allocation.
#[derive(Clone, Copy)]
struct VmallocEntry {
    /// Virtual address of the allocation (0 = unused slot).
    vaddr: u64,
    /// Number of pages allocated (excluding guard pages).
    page_count: u32,
    /// Whether this entry is active.
    active: bool,
}

impl VmallocEntry {
    const fn empty() -> Self {
        Self {
            vaddr: 0,
            page_count: 0,
            active: false,
        }
    }
}

/// Bitmap tracking allocated virtual pages: one bit per 16 KiB page of the
/// region, 65536 pages in 1024 words (8 KiB).
const BITMAP_WORDS: usize = VMALLOC_MAX_PAGES.div_ceil(64);

// `find_free_run` steps over whole words, which is only exact when the
// region ends on a word boundary: otherwise the unused high bits of the last
// word would read as free pages beyond the region.
const _: () = assert!(VMALLOC_MAX_PAGES.is_multiple_of(64));

/// Global vmalloc state, protected by a spinlock.
struct VmallocState {
    /// Bitmap: 1 = allocated (or reserved while being freed), 0 = free.
    bitmap: [u64; BITMAP_WORDS],
    /// Allocation metadata.
    entries: [VmallocEntry; MAX_VMALLOC_ENTRIES],
    /// Number of active allocations.
    active_count: usize,
}

impl VmallocState {
    const fn new() -> Self {
        Self {
            bitmap: [0; BITMAP_WORDS],
            entries: [VmallocEntry::empty(); MAX_VMALLOC_ENTRIES],
            active_count: 0,
        }
    }

    /// Mark pages as allocated in the bitmap.
    fn mark_allocated(&mut self, start: usize, count: usize) {
        set_bits(&mut self.bitmap, start, count, true);
    }

    /// Mark pages as free in the bitmap.
    fn mark_free(&mut self, start: usize, count: usize) {
        set_bits(&mut self.bitmap, start, count, false);
    }

    /// Deactivate the active entry for `vaddr`, returning its page count.
    /// Leaves the bitmap alone: the caller decides when the range is free.
    fn take_entry(&mut self, vaddr: u64) -> Option<usize> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.active && e.vaddr == vaddr)?;
        let pages = entry.page_count as usize;
        *entry = VmallocEntry::empty();
        self.active_count = self.active_count.saturating_sub(1);
        Some(pages)
    }
}

/// First run of `page_count` free pages with a free guard page either side,
/// searching `bitmap` (bit set = allocated) first-fit from the bottom.
/// Returns the index of the leading guard page.
///
/// Whole words are consumed 64 pages at a time when they are completely
/// full or completely free, so the 1024-word bitmap of the full region
/// costs about a thousand word tests in the worst case rather than 65536
/// bit tests. `bitmap.len() * 64` pages are searched.
#[allow(clippy::arithmetic_side_effects)] // All indices are bounded by `max_pages`.
fn find_free_run(bitmap: &[u64], page_count: usize) -> Option<usize> {
    let max_pages = bitmap.len().checked_mul(64)?;
    // A guard page before and after.
    let total_needed = page_count.checked_add(2)?;
    if page_count == 0 || total_needed > max_pages {
        return None;
    }

    let mut run_start = 0;
    let mut run_len = 0;
    let mut i = 0;
    while i < max_pages {
        let word = bitmap.get(i / 64).copied().unwrap_or(u64::MAX);
        if i % 64 == 0 && word == u64::MAX {
            // 64 allocated pages: whatever run was growing ends here.
            i += 64;
            run_start = i;
            run_len = 0;
        } else if i % 64 == 0 && word == 0 {
            run_len += 64;
            i += 64;
            if run_len >= total_needed {
                return Some(run_start);
            }
        } else if (word >> (i % 64)) & 1 != 0 {
            i += 1;
            run_start = i;
            run_len = 0;
        } else {
            run_len += 1;
            i += 1;
            if run_len >= total_needed {
                return Some(run_start);
            }
        }
    }
    None
}

/// Set (`allocated`) or clear `count` bits of `bitmap` starting at bit
/// `start`. Bits beyond the bitmap are ignored.
#[allow(clippy::arithmetic_side_effects)] // `i / 64` and `i % 64` cannot overflow.
fn set_bits(bitmap: &mut [u64], start: usize, count: usize, allocated: bool) {
    for i in start..start.saturating_add(count) {
        if let Some(word) = bitmap.get_mut(i / 64) {
            let bit = 1u64 << (i % 64);
            if allocated {
                *word |= bit;
            } else {
                *word &= !bit;
            }
        }
    }
}

static STATE: Mutex<VmallocState> = Mutex::new(VmallocState::new());

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static FREE_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_FAILURES: AtomicU64 = AtomicU64::new(0);
static BYTES_ALLOCATED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Whether `addr` lies inside the vmalloc region.
///
/// Says nothing about whether it is part of a live allocation; the kernel
/// heap uses it to route a freed pointer back to [`vfree`] rather than to
/// the buddy allocator, which never hands out addresses in this range.
#[must_use]
pub fn contains(addr: u64) -> bool {
    super::kvspace::VMALLOC.contains(addr)
}

/// Allocate virtually-contiguous kernel memory.
///
/// Returns a pointer to `size` bytes of kernel virtual memory, backed
/// by individually-allocated physical frames.  The memory is writable
/// and non-executable, 16 KiB aligned, not zeroed, and visible in every
/// address space (see the module docs).
///
/// `size` is rounded up to the nearest multiple of `FRAME_SIZE` (16 KiB).
///
/// The frames are tagged [`Owner::Vmalloc`] in the frame-owner census.
/// Nothing is charged to a `memtype` category here; a caller that accounts
/// its memory by purpose (the kernel heap charges `LargeHeap`) does so
/// itself.
///
/// Must not be called from interrupt context (see "Thread Safety").
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `size` is 0.
/// - [`KernelError::OutOfMemory`] if no contiguous virtual range is
///   available, the allocation table is full, or a physical frame or page
///   table cannot be allocated. Nothing is left mapped or reserved.
/// - [`KernelError::NotSupported`] before `page_table::init`.
#[allow(clippy::arithmetic_side_effects)] // Offsets are bounded by the region size.
pub fn vmalloc(size: usize) -> KernelResult<*mut u8> {
    if size == 0 {
        return Err(KernelError::InvalidArgument);
    }
    let pml4 = page_table::kernel_pml4_phys()?;

    // Round up to frame size. Anything larger than the region cannot fit,
    // and refusing it here also keeps `page_count` within `u32` below.
    let page_count = size.div_ceil(FRAME_SIZE);
    if page_count > VMALLOC_MAX_PAGES {
        ALLOC_FAILURES.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::OutOfMemory);
    }

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;

    // Reserve the virtual range (and a metadata slot) under the lock.
    let (vaddr, run_start) = {
        let mut state = STATE.lock();
        let reservation = state
            .entries
            .iter()
            .position(|e| !e.active)
            .zip(find_free_run(&state.bitmap, page_count));
        let Some((entry_idx, run_start)) = reservation else {
            drop(state);
            ALLOC_FAILURES.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::OutOfMemory);
        };

        // The allocation starts after the leading guard page; the full
        // range, guards included, is marked so no neighbour abuts it.
        let vaddr = VMALLOC_BASE + ((run_start + 1) as u64) * (FRAME_SIZE as u64);
        state.mark_allocated(run_start, page_count + 2);
        if let Some(entry) = state.entries.get_mut(entry_idx) {
            #[allow(clippy::cast_possible_truncation)] // page_count <= VMALLOC_MAX_PAGES.
            let pages = page_count as u32;
            *entry = VmallocEntry {
                vaddr,
                page_count: pages,
                active: true,
            };
        }
        state.active_count += 1;
        (vaddr, run_start)
    };

    // Allocate frames and create mappings (outside the lock: the range is
    // ours alone, and `page_table` publishes shared tables atomically).
    for i in 0..page_count {
        let page_virt = VirtAddr::new(vaddr + (i as u64) * (FRAME_SIZE as u64));
        let frame = {
            let _own = OwnerScope::new(Owner::Vmalloc);
            frame::alloc_frame()
        };
        let mapped = frame.and_then(|f| {
            // SAFETY: pml4 is the kernel's PML4; page_virt lies in the range
            // reserved above, which nothing else maps; f is a frame we just
            // allocated and own.
            unsafe { page_table::map_frame(pml4, page_virt, f, flags) }.inspect_err(|_| {
                // SAFETY: f was never mapped, so nothing can reach it.
                // Ignoring the result: the frame came from the allocator a
                // moment ago, so returning it cannot meet a state it rejects.
                let _ = unsafe { frame::free_frame(f) };
            })
        });
        if let Err(e) = mapped {
            // SAFETY: pages 0..i of this allocation were mapped by this loop
            // and nothing else has seen the pointer yet.
            unsafe { unmap_and_free(pml4, vaddr, i) };
            {
                let mut state = STATE.lock();
                state.take_entry(vaddr);
                state.mark_free(run_start, page_count + 2);
            }
            ALLOC_FAILURES.fetch_add(1, Ordering::Relaxed);
            return Err(e);
        }
    }

    ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
    BYTES_ALLOCATED.fetch_add((page_count * FRAME_SIZE) as u64, Ordering::Relaxed);

    Ok(vaddr as *mut u8)
}

/// Free a vmalloc allocation.
///
/// The virtual range stays reserved until every page is unmapped and its
/// translation shot down on every CPU, and only then returns to the pool:
/// released earlier, a concurrent [`vmalloc`] could map the range again
/// and this call would then unmap *its* pages.
///
/// Must not be called from interrupt context, nor with interrupts disabled
/// while other CPUs are online (the TLB shootdown waits for them).
///
/// # Errors
///
/// - [`KernelError::InvalidAddress`] if `ptr` is outside the vmalloc region.
/// - [`KernelError::NotFound`] if it is not the start of a live allocation
///   (including a second free of the same pointer).
/// - [`KernelError::NotSupported`] before `page_table::init`.
///
/// # Safety
///
/// `ptr` must have been returned by a prior successful `vmalloc()` call,
/// and nothing may access the allocation after this call begins.
#[allow(clippy::arithmetic_side_effects)] // Page arithmetic bounded by the region.
pub unsafe fn vfree(ptr: *mut u8) -> KernelResult<()> {
    let vaddr = ptr as u64;
    if !contains(vaddr) {
        return Err(KernelError::InvalidAddress);
    }
    let pml4 = page_table::kernel_pml4_phys()?;

    // Claim the entry first, so a second vfree of the same pointer finds
    // nothing, but keep the range reserved in the bitmap.
    let page_count = STATE
        .lock()
        .take_entry(vaddr)
        .ok_or(KernelError::NotFound)?;

    // SAFETY: the entry described exactly these page_count pages at vaddr,
    // all mapped by vmalloc, and the caller guarantees nothing uses them.
    unsafe { unmap_and_free(pml4, vaddr, page_count) };

    // Guard + data + guard, now that no CPU can reach the old frames.
    let first_page = ((vaddr - VMALLOC_BASE) / FRAME_SIZE as u64) as usize;
    STATE
        .lock()
        .mark_free(first_page.saturating_sub(1), page_count + 2);

    FREE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Frames unmapped per TLB shootdown in [`unmap_and_free`]. A frame cannot
/// be freed until its translation is gone from every TLB, so the frames of
/// one batch are held on the stack across the shootdown; 32 keeps that to
/// 256 bytes while making a 20 MiB free about 40 shootdowns rather than
/// 1280.
const UNMAP_BATCH: usize = 32;

/// Unmap the first `count` pages of the allocation at `vaddr`, shoot their
/// translations down on every CPU, and only then free the frames.
///
/// Works in batches of [`UNMAP_BATCH`] so it needs no heap memory — it runs
/// on the kernel heap's free path.
///
/// # Safety
///
/// Pages `0..count` at `vaddr` must be mapped by [`vmalloc`] through
/// `pml4`, and nothing may access them again.
#[allow(clippy::arithmetic_side_effects)] // Bounded by `count`, itself bounded by the region.
unsafe fn unmap_and_free(pml4: u64, vaddr: u64, count: usize) {
    let mut done = 0;
    while done < count {
        let batch = (count - done).min(UNMAP_BATCH);
        let batch_start = vaddr + (done as u64) * (FRAME_SIZE as u64);
        let mut frames: [Option<PhysFrame>; UNMAP_BATCH] = [None; UNMAP_BATCH];
        for (i, slot) in frames.iter_mut().take(batch).enumerate() {
            let page_virt = VirtAddr::new(batch_start + (i as u64) * (FRAME_SIZE as u64));
            // SAFETY: the caller guarantees this page was mapped through
            // pml4 by vmalloc. A page that is somehow not mapped reports
            // an error and has no frame to free, so `.ok()` loses nothing.
            *slot = unsafe { page_table::unmap_frame(pml4, page_virt) }.ok();
        }
        // Every CPU must drop the old translations before any frame can be
        // reused, or a stale entry would keep reading and writing it.
        #[allow(clippy::cast_possible_truncation)] // batch * 4 <= 128.
        crate::tlb::flush_range(batch_start, (batch * HW_PAGES_PER_FRAME) as u32);
        for frame in frames.iter().take(batch).flatten() {
            // SAFETY: the frame was allocated by vmalloc, is now unmapped,
            // and its translation has been flushed from every TLB.
            // Ignoring the result: a refusal would mean the frame was
            // already free, and freeing nothing is the only safe response.
            let _ = unsafe { frame::free_frame(*frame) };
        }
        done += batch;
    }
}

/// Get the size (in bytes) of a vmalloc allocation.
///
/// Returns `None` if `ptr` is not a known vmalloc allocation.
pub fn vmalloc_size(ptr: *const u8) -> Option<usize> {
    let vaddr = ptr as u64;
    let state = STATE.lock();
    state
        .entries
        .iter()
        .find(|e| e.active && e.vaddr == vaddr)
        .map(|e| e.page_count as usize * FRAME_SIZE)
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// vmalloc statistics.
#[derive(Debug, Clone, Copy)]
pub struct VmallocStats {
    /// Total successful allocations.
    pub alloc_count: u64,
    /// Total frees.
    pub free_count: u64,
    /// Failed allocation attempts.
    pub alloc_failures: u64,
    /// Current active allocations.
    pub active: usize,
    /// Total bytes currently allocated via vmalloc.
    pub bytes_allocated: u64,
    /// Total vmalloc region size.
    pub region_size: usize,
}

/// Get vmalloc statistics.
#[must_use]
pub fn stats() -> VmallocStats {
    let state = STATE.lock();
    let active_bytes: u64 = state
        .entries
        .iter()
        .filter(|e| e.active)
        .map(|e| (e.page_count as u64) * (FRAME_SIZE as u64))
        .sum();

    VmallocStats {
        alloc_count: ALLOC_COUNT.load(Ordering::Relaxed),
        free_count: FREE_COUNT.load(Ordering::Relaxed),
        alloc_failures: ALLOC_FAILURES.load(Ordering::Relaxed),
        active: state.active_count,
        bytes_allocated: active_bytes,
        region_size: VMALLOC_SIZE,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the vmalloc subsystem.
pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[vmalloc] Running self-test...");

    // Test 1: Basic allocation.
    let ptr = vmalloc(FRAME_SIZE).expect("vmalloc should succeed");
    assert!(!ptr.is_null());
    let addr = ptr as u64;
    assert!(addr >= VMALLOC_BASE);
    assert!(addr < VMALLOC_BASE + VMALLOC_SIZE as u64);
    serial_println!("[vmalloc]   Alloc 16 KiB: OK (addr={:#x})", addr);

    // Test 2: Write and read.
    // SAFETY: ptr was returned by vmalloc (valid, mapped, FRAME_SIZE bytes);
    // ptr.add(FRAME_SIZE - 1) is the last byte of the allocation.
    unsafe {
        core::ptr::write_volatile(ptr, 0xAB);
        let val = core::ptr::read_volatile(ptr);
        assert_eq!(val, 0xAB);

        // Write at end of allocation.
        let end = ptr.add(FRAME_SIZE - 1);
        core::ptr::write_volatile(end, 0xCD);
        let val2 = core::ptr::read_volatile(end);
        assert_eq!(val2, 0xCD);
    }
    serial_println!("[vmalloc]   Read/write: OK");

    // Test 3: Size query.
    let sz = vmalloc_size(ptr).expect("should find allocation");
    assert_eq!(sz, FRAME_SIZE);
    serial_println!("[vmalloc]   Size query: OK ({} bytes)", sz);

    // Test 4: Multi-page allocation.
    let big_size = FRAME_SIZE * 4; // 64 KiB
    let ptr2 = vmalloc(big_size).expect("multi-page vmalloc should succeed");
    assert!(!ptr2.is_null());
    // Write to each page to verify mappings.
    // SAFETY: ptr2 was returned by vmalloc(4 * FRAME_SIZE); offsets
    // i * FRAME_SIZE for i in 0..4 are within the allocation bounds.
    unsafe {
        for i in 0..4usize {
            let p = ptr2.add(i * FRAME_SIZE);
            core::ptr::write_volatile(p, (i as u8) + 1);
        }
        for i in 0..4usize {
            let p = ptr2.add(i * FRAME_SIZE);
            let val = core::ptr::read_volatile(p);
            assert_eq!(val, (i as u8) + 1);
        }
    }
    serial_println!(
        "[vmalloc]   Multi-page (4 × 16 KiB): OK (addr={:#x})",
        ptr2 as u64
    );

    // Test 5: Free.
    // SAFETY: ptr and ptr2 were returned by successful vmalloc calls
    // and have not been freed yet.
    unsafe {
        vfree(ptr).expect("vfree should succeed");
        vfree(ptr2).expect("vfree should succeed");
    }
    serial_println!("[vmalloc]   Free: OK");

    // Test 6: Re-allocation after free.
    let ptr3 = vmalloc(FRAME_SIZE).expect("re-alloc should succeed");
    assert!(!ptr3.is_null());
    // SAFETY: ptr3 was returned by vmalloc and has not been freed.
    unsafe {
        vfree(ptr3).expect("vfree should succeed");
    }
    serial_println!("[vmalloc]   Re-alloc after free: OK");

    // Test 7: Zero-size rejection.
    let zero_result = vmalloc(0);
    assert!(zero_result.is_err());
    serial_println!("[vmalloc]   Zero-size rejected: OK");

    // Tests 8-12: the properties the kernel heap relies on now that it sends
    // every allocation above 16 MiB here.
    test_free_run_search()?;
    serial_println!("[vmalloc]   Free-run search (word skipping, boundaries): OK");
    test_visible_in_existing_address_space()?;
    serial_println!("[vmalloc]   Visible in an address space created before the allocation: OK");
    test_not_charged_to_loaded_process()?;
    serial_println!("[vmalloc]   Not charged to the process whose page tables are loaded: OK");
    test_stale_translation_flushed()?;
    serial_println!("[vmalloc]   Freed range reused with no stale translation: OK");
    test_larger_than_buddy_block()?;
    serial_println!("[vmalloc]   20 MiB allocation (above the 16 MiB buddy maximum): OK");

    // Test 13: Stats.
    let st = stats();
    assert!(st.alloc_count >= 3);
    assert!(st.free_count >= 3);
    assert_eq!(st.active, 0);
    serial_println!(
        "[vmalloc]   Stats: OK (allocs={}, frees={}, active={})",
        st.alloc_count,
        st.free_count,
        st.active
    );

    serial_println!("[vmalloc] Self-test PASSED");
    Ok(())
}

/// Report a self-test failure and turn it into an error.
fn fail(what: core::fmt::Arguments<'_>) -> KernelError {
    serial_println!("[vmalloc]   FAIL: {}", what);
    KernelError::InternalError
}

/// The first-fit search, on bitmaps small enough to reason about by hand.
///
/// The search skips whole words that are completely full or completely
/// free; every case here puts a run edge where that skipping could
/// miscount — at a word boundary, just inside one, across one.
fn test_free_run_search() -> KernelResult<()> {
    let check = |what: &str, got: Option<usize>, want: Option<usize>| {
        if got == want {
            Ok(())
        } else {
            Err(fail(format_args!(
                "free-run search, {}: got {:?}, want {:?}",
                what, got, want
            )))
        }
    };
    // 4 words = 256 pages.
    let mut bitmap = [0u64; 4];
    check("empty, 1 page", find_free_run(&bitmap, 1), Some(0))?;
    check(
        "empty, all but the guards",
        find_free_run(&bitmap, 254),
        Some(0),
    )?;
    check(
        "empty, one page too many",
        find_free_run(&bitmap, 255),
        None,
    )?;
    check("zero pages", find_free_run(&bitmap, 0), None)?;

    // Pages 0..3 used, word 1 (pages 64..128) full: free runs 3..64 (61
    // pages) and 128..256 (128 pages).
    set_bits(&mut bitmap, 0, 3, true);
    set_bits(&mut bitmap, 64, 64, true);
    check("after a used prefix", find_free_run(&bitmap, 1), Some(3))?;
    check(
        "exactly fills the gap below a full word",
        find_free_run(&bitmap, 59),
        Some(3),
    )?;
    check(
        "one page too big for that gap",
        find_free_run(&bitmap, 60),
        Some(128),
    )?;

    // Page 200 used: free runs 3..64 (61), 128..200 (72), 201..256 (55).
    set_bits(&mut bitmap, 200, 1, true);
    check(
        "run spanning a word boundary",
        find_free_run(&bitmap, 70),
        Some(128),
    )?;
    check("too big for every gap", find_free_run(&bitmap, 71), None)?;

    set_bits(&mut bitmap, 0, 256, false);
    check("cleared again", find_free_run(&bitmap, 254), Some(0))?;
    if bitmap != [0; 4] {
        return Err(fail(format_args!(
            "set_bits left bits behind: {:x?}",
            bitmap
        )));
    }
    Ok(())
}

/// An allocation must be mapped in an address space that existed before it
/// was made — the failure the boot-time top-level entry exists to prevent.
#[allow(clippy::arithmetic_side_effects)] // Offsets within a 4-frame allocation.
fn test_visible_in_existing_address_space() -> KernelResult<()> {
    const PAGES: usize = 4;
    let kernel = page_table::kernel_pml4_phys()?;
    let existing = page_table::alloc_pml4()?;
    let result = (|| -> KernelResult<()> {
        let ptr = vmalloc(PAGES * FRAME_SIZE)?;
        let page = |i: usize| VirtAddr::new(ptr as u64 + (i * FRAME_SIZE) as u64);
        let mapped = (0..PAGES).try_for_each(|i| {
            let (k, e) = (
                page_table::translate(kernel, page(i)),
                page_table::translate(existing, page(i)),
            );
            if k.is_some() && k == e {
                Ok(())
            } else {
                Err(fail(format_args!(
                    "page {} of {:#x}: kernel sees {:?}, an older address space sees {:?}",
                    i, ptr as u64, k, e
                )))
            }
        });
        // SAFETY: ptr came from vmalloc above and nothing else holds it.
        unsafe { vfree(ptr)? };
        mapped?;
        // And gone from both once freed.
        (0..PAGES).try_for_each(|i| {
            let (k, e) = (
                page_table::translate(kernel, page(i)),
                page_table::translate(existing, page(i)),
            );
            if k.is_none() && e.is_none() {
                Ok(())
            } else {
                Err(fail(format_args!(
                    "page {} still mapped after vfree: kernel {:?}, older address space {:?}",
                    i, k, e
                )))
            }
        })
    })();
    // SAFETY: `existing` came from alloc_pml4, was never loaded into CR3,
    // and has no user mappings.
    unsafe { page_table::free_pml4(existing) };
    result
}

/// A `vmalloc` made while a process's page tables are loaded must not be
/// charged to that process: it is kernel memory the process neither owns
/// nor can free. (It was, when mappings went through the loaded PML4.)
fn test_not_charged_to_loaded_process() -> KernelResult<()> {
    let kernel = page_table::kernel_pml4_phys()?;
    let process = page_table::alloc_pml4()?;
    let mapped_ever = || super::accounting::query(process).map(|s| s.total_mapped_ever);
    let before = mapped_ever();
    // With interrupts off nothing can switch tasks while the process's
    // tables are loaded, and the kernel half they share keeps this code,
    // its stack and every kernel structure mapped.
    let allocated = crate::cpu::without_interrupts(|| {
        let saved = page_table::read_cr3();
        // SAFETY: `process`'s kernel half is a copy of the kernel's, so it
        // maps everything the kernel is running on; CR3 is restored below
        // before interrupts come back.
        unsafe { page_table::write_cr3(process) };
        let r = vmalloc(FRAME_SIZE);
        // SAFETY: restoring the value read above.
        unsafe { page_table::write_cr3(saved) };
        r
    });
    let after = mapped_ever();
    let result = match allocated {
        Err(e) => Err(fail(format_args!("vmalloc with a process loaded: {:?}", e))),
        Ok(ptr) => {
            let visible = page_table::translate(kernel, VirtAddr::new(ptr as u64)).is_some();
            // SAFETY: ptr came from vmalloc above and nothing else holds it.
            let freed = unsafe { vfree(ptr) };
            if before.is_none() || after != before {
                Err(fail(format_args!(
                    "the loaded process was charged: frames mapped ever {:?} -> {:?}",
                    before, after
                )))
            } else if !visible {
                Err(fail(format_args!(
                    "allocation made with a process loaded is not in the kernel's tables"
                )))
            } else {
                freed
            }
        }
    };
    // SAFETY: `process` came from alloc_pml4, is no longer in CR3 (restored
    // above), and has no user mappings.
    unsafe { page_table::free_pml4(process) };
    result
}

/// A freed range handed out again must not be reached through the
/// translation of its previous frame. `vfree` once left the old entries in
/// the TLB, so a write through the new allocation could land in the frame
/// the previous one had given back.
#[allow(clippy::arithmetic_side_effects)] // Physical address + HHDM offset.
fn test_stale_translation_flushed() -> KernelResult<()> {
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let kernel = page_table::kernel_pml4_phys()?;
    // No task switch from the first touch to the check: a switch to a task
    // on other page tables reloads CR3 and would flush the stale entry this
    // test exists to catch. Preemption rather than interrupts is disabled,
    // because `vfree` performs a TLB shootdown, and a CPU waiting for one
    // with interrupts off can deadlock against another CPU doing the same.
    crate::sched::preempt_disable();
    let result = (|| -> KernelResult<()> {
        let first = vmalloc(FRAME_SIZE)?;
        // SAFETY: first is a live one-frame vmalloc allocation. The read
        // makes sure its translation is cached.
        unsafe {
            core::ptr::write_volatile(first, 0x11);
            let _ = core::ptr::read_volatile(first);
        }
        let old_phys = page_table::translate(kernel, VirtAddr::new(first as u64))
            .ok_or_else(|| fail(format_args!("a fresh allocation has no translation")))?;
        // SAFETY: first came from vmalloc and is not used again.
        unsafe { vfree(first)? };

        // Hold the old frame so the next allocation cannot be backed by it,
        // and mark it through the direct map.
        let mut held: [Option<PhysFrame>; 8] = [None; 8];
        let mut holding_old = false;
        for slot in &mut held {
            let Ok(f) = frame::alloc_frame() else { break };
            *slot = Some(f);
            if f.addr() == old_phys {
                holding_old = true;
                // SAFETY: we own the frame again, and the direct map covers it.
                unsafe { core::ptr::write_volatile((old_phys + hhdm) as *mut u8, 0x33) };
                break;
            }
        }

        let result = (|| -> KernelResult<()> {
            let second = vmalloc(FRAME_SIZE)?;
            let checked = (|| -> KernelResult<()> {
                if second != first {
                    return Err(fail(format_args!(
                        "the freed range was not reused ({:#x} then {:#x}); first fit should return it",
                        first as u64, second as u64
                    )));
                }
                let new_phys = page_table::translate(kernel, VirtAddr::new(second as u64))
                    .ok_or_else(|| fail(format_args!("the reused range has no translation")))?;
                // SAFETY: second is a live one-frame vmalloc allocation; both
                // physical addresses are frames the direct map covers.
                let (landed, old) = unsafe {
                    core::ptr::write_volatile(second, 0x22);
                    (
                        core::ptr::read_volatile((new_phys + hhdm) as *const u8),
                        core::ptr::read_volatile((old_phys + hhdm) as *const u8),
                    )
                };
                if landed != 0x22 {
                    return Err(fail(format_args!(
                        "a write through the reused range missed its own frame ({:#x} reads {:#x}): \
                         a stale translation still named the freed frame",
                        new_phys, landed
                    )));
                }
                if holding_old && old != 0x33 {
                    return Err(fail(format_args!(
                        "the freed frame {:#x} was written through a stale translation (reads {:#x})",
                        old_phys, old
                    )));
                }
                Ok(())
            })();
            // SAFETY: second came from vmalloc and is not used again.
            unsafe { vfree(second)? };
            checked
        })();
        for f in held.iter().flatten() {
            // SAFETY: allocated above and never mapped anywhere.
            // Ignoring the result: see unmap_and_free.
            let _ = unsafe { frame::free_frame(*f) };
        }
        result
    })();
    crate::sched::preempt_enable();
    result
}

/// The case the heap needs vmalloc for: one allocation larger than the
/// buddy allocator's biggest block (2^10 frames = 16 MiB), every page
/// individually writable and distinct.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn test_larger_than_buddy_block() -> KernelResult<()> {
    const SIZE: usize = 20 * 1024 * 1024;
    const PAGES: usize = SIZE / FRAME_SIZE;
    let ptr = vmalloc(SIZE)?;
    // SAFETY: every offset is below SIZE, inside the live allocation.
    let bad = unsafe {
        for i in 0..PAGES {
            core::ptr::write_volatile(ptr.add(i * FRAME_SIZE), i as u8);
            core::ptr::write_volatile(ptr.add(i * FRAME_SIZE + FRAME_SIZE - 1), !(i as u8));
        }
        (0..PAGES).find(|&i| {
            core::ptr::read_volatile(ptr.add(i * FRAME_SIZE)) != i as u8
                || core::ptr::read_volatile(ptr.add(i * FRAME_SIZE + FRAME_SIZE - 1)) != !(i as u8)
        })
    };
    // SAFETY: ptr came from vmalloc above and is not used again.
    unsafe { vfree(ptr)? };
    match bad {
        None => Ok(()),
        Some(i) => Err(fail(format_args!(
            "page {} of a {} MiB allocation did not keep its own contents",
            i,
            SIZE >> 20
        ))),
    }
}

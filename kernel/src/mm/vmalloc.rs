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
//! | **vmalloc** | No               | 16 KiB – 32 MiB | Large buffers, tables, module code |
//! | Huge page | Yes (2 MiB-aligned) | 2 MiB | Performance-critical large mappings |
//!
//! ## Design
//!
//! The vmalloc region occupies a dedicated portion of kernel virtual
//! address space (0xFFFF_C300_0000_0000, 128 MiB).  A bitmap tracks
//! which virtual pages within this region are allocated.
//!
//! Each vmalloc allocation:
//! 1. Finds N contiguous free virtual pages in the bitmap.
//! 2. Allocates N physical frames (individually, no contiguity needed).
//! 3. Maps each virtual page to its physical frame.
//! 4. Returns a pointer to the start of the contiguous virtual region.
//!
//! Freeing reverses this: unmap each page, free each frame, clear bitmap.
//!
//! ## Guard Pages
//!
//! Each allocation is surrounded by unmapped guard pages to catch
//! out-of-bounds accesses.  This adds 2 pages of overhead per allocation
//! but provides immediate detection of buffer overflows/underflows.
//!
//! ## Thread Safety
//!
//! The bitmap and allocation metadata are protected by a spinlock.
//! The actual page table operations are lock-free (they only touch
//! the kernel's page tables which are shared across all CPUs).
//!
//! ## References
//!
//! - Linux `mm/vmalloc.c` — virtual kernel memory allocator
//! - Linux `include/linux/vmalloc.h` — vmalloc/vfree/vmap
//! - FreeBSD `kern/kern_malloc.c` — kernel_map allocations

#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;
use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Base virtual address of the vmalloc region.
/// Located in kernel space, well away from HHDM, kstack, and huge page regions.
const VMALLOC_BASE: u64 = 0xFFFF_C300_0000_0000;

/// Size of the vmalloc region (128 MiB = 8192 × 16 KiB frames).
const VMALLOC_SIZE: usize = 128 * 1024 * 1024;

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
        Self { vaddr: 0, page_count: 0, active: false }
    }
}

/// Bitmap tracking allocated virtual pages.
/// Each bit represents one 16 KiB page in the vmalloc region.
/// VMALLOC_MAX_PAGES = 8192, so we need 8192 / 64 = 128 u64 words.
const BITMAP_WORDS: usize = VMALLOC_MAX_PAGES.div_ceil(64);

/// Global vmalloc state, protected by a spinlock.
struct VmallocState {
    /// Bitmap: 1 = allocated, 0 = free.
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

    /// Find N contiguous free bits in the bitmap (with 1 guard page before and after).
    fn find_free_run(&self, page_count: usize) -> Option<usize> {
        // We need page_count + 2 contiguous free pages (guard before + after).
        let total_needed = page_count + 2;
        if total_needed > VMALLOC_MAX_PAGES {
            return None;
        }

        let mut run_start = 0;
        let mut run_len = 0;

        for i in 0..VMALLOC_MAX_PAGES {
            let word = i / 64;
            let bit = i % 64;
            let allocated = (self.bitmap[word] >> bit) & 1 != 0;

            if allocated {
                run_start = i + 1;
                run_len = 0;
            } else {
                run_len += 1;
                if run_len >= total_needed {
                    return Some(run_start);
                }
            }
        }
        None
    }

    /// Mark pages as allocated in the bitmap.
    fn mark_allocated(&mut self, start: usize, count: usize) {
        for i in start..start.saturating_add(count) {
            if i < VMALLOC_MAX_PAGES {
                let word = i / 64;
                let bit = i % 64;
                self.bitmap[word] |= 1u64 << bit;
            }
        }
    }

    /// Mark pages as free in the bitmap.
    fn mark_free(&mut self, start: usize, count: usize) {
        for i in start..start.saturating_add(count) {
            if i < VMALLOC_MAX_PAGES {
                let word = i / 64;
                let bit = i % 64;
                self.bitmap[word] &= !(1u64 << bit);
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

/// Allocate virtually-contiguous kernel memory.
///
/// Returns a pointer to `size` bytes of kernel virtual memory, backed
/// by individually-allocated physical frames.  The memory is writable
/// and non-executable.
///
/// `size` is rounded up to the nearest multiple of `FRAME_SIZE` (16 KiB).
///
/// Returns `Err` if:
/// - No contiguous virtual address range is available.
/// - Physical frame allocation fails.
/// - The allocation table is full.
#[allow(clippy::arithmetic_side_effects)]
pub fn vmalloc(size: usize) -> KernelResult<*mut u8> {
    if size == 0 {
        return Err(KernelError::InvalidArgument);
    }

    // Round up to frame size.
    let page_count = size.div_ceil(FRAME_SIZE);

    let pml4 = page_table::active_pml4_phys();
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;

    // Find free virtual pages and allocate the entry under lock.
    let (vaddr, run_start);
    {
        let mut state = STATE.lock();

        // Find a metadata slot.
        let entry_idx = state.entries.iter().position(|e| !e.active)
            .ok_or(KernelError::OutOfMemory)?;

        // Find contiguous virtual pages.
        run_start = state.find_free_run(page_count)
            .ok_or(KernelError::OutOfMemory)?;

        // The actual allocation starts after the leading guard page.
        let alloc_start = run_start + 1;
        vaddr = VMALLOC_BASE + (alloc_start as u64) * (FRAME_SIZE as u64);

        // Mark the full range (including guard pages) as allocated.
        state.mark_allocated(run_start, page_count + 2);

        // Record metadata.
        state.entries[entry_idx] = VmallocEntry {
            vaddr,
            page_count: page_count as u32,
            active: true,
        };
        state.active_count += 1;
    }

    // Allocate frames and create mappings (outside the lock).
    for i in 0..page_count {
        let frame = match frame::alloc_frame() {
            Ok(f) => f,
            Err(e) => {
                // Rollback: unmap and free any frames we already allocated.
                // SAFETY: pml4 is valid (from active_pml4_phys); each page_virt
                // was successfully mapped in a prior iteration of this loop;
                // each frame f returned by unmap_frame is valid for freeing.
                for j in 0..i {
                    let page_virt = VirtAddr::new(vaddr + (j as u64) * (FRAME_SIZE as u64));
                    if let Ok(f) = unsafe { page_table::unmap_frame(pml4, page_virt) } {
                        let _ = unsafe { frame::free_frame(f) };
                    }
                }
                // Free the bitmap reservation.
                let mut state = STATE.lock();
                state.mark_free(run_start, page_count + 2);
                // Find and deactivate the entry.
                for entry in state.entries.iter_mut() {
                    if entry.active && entry.vaddr == vaddr {
                        *entry = VmallocEntry::empty();
                        state.active_count -= 1;
                        break;
                    }
                }
                ALLOC_FAILURES.fetch_add(1, Ordering::Relaxed);
                return Err(e);
            }
        };

        let page_virt = VirtAddr::new(vaddr + (i as u64) * (FRAME_SIZE as u64));

        // SAFETY: pml4 is valid, page_virt is in our reserved vmalloc region.
        if let Err(e) = unsafe { page_table::map_frame(pml4, page_virt, frame, flags) } {
            // Mapping failed — free this frame and rollback prior mappings.
            // SAFETY: frame was just allocated by alloc_frame and is valid
            // for freeing.  Prior pages were successfully mapped, so
            // unmap_frame and free_frame are valid for those addresses.
            let _ = unsafe { frame::free_frame(frame) };
            for j in 0..i {
                let prior_virt = VirtAddr::new(vaddr + (j as u64) * (FRAME_SIZE as u64));
                if let Ok(f) = unsafe { page_table::unmap_frame(pml4, prior_virt) } {
                    let _ = unsafe { frame::free_frame(f) };
                }
            }
            let mut state = STATE.lock();
            state.mark_free(run_start, page_count + 2);
            for entry in state.entries.iter_mut() {
                if entry.active && entry.vaddr == vaddr {
                    *entry = VmallocEntry::empty();
                    state.active_count -= 1;
                    break;
                }
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
/// # Safety
///
/// `ptr` must have been returned by a prior successful `vmalloc()` call
/// and must not have been freed already.
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn vfree(ptr: *mut u8) -> KernelResult<()> {
    let vaddr = ptr as u64;

    if vaddr < VMALLOC_BASE || vaddr >= VMALLOC_BASE + VMALLOC_SIZE as u64 {
        return Err(KernelError::InvalidAddress);
    }

    // Find the entry.
    let page_count;
    let alloc_page_start;
    {
        let mut state = STATE.lock();
        let entry = state.entries.iter_mut()
            .find(|e| e.active && e.vaddr == vaddr)
            .ok_or(KernelError::NotFound)?;

        page_count = entry.page_count as usize;
        // The bitmap includes guard pages: 1 before + page_count + 1 after.
        let offset_pages = ((vaddr - VMALLOC_BASE) / FRAME_SIZE as u64) as usize;
        alloc_page_start = offset_pages.saturating_sub(1); // Include leading guard.

        entry.active = false;
        entry.vaddr = 0;
        entry.page_count = 0;
        state.active_count -= 1;

        // Free bitmap (guard + data + guard).
        state.mark_free(alloc_page_start, page_count + 2);
    }

    // Unmap and free each frame (outside the lock).
    // SAFETY: pml4 is valid (from active_pml4_phys); each page_virt is in
    // our vmalloc region and was mapped during allocation; each frame f
    // returned by unmap_frame was allocated by alloc_frame.
    let pml4 = page_table::active_pml4_phys();
    for i in 0..page_count {
        let page_virt = VirtAddr::new(vaddr + (i as u64) * (FRAME_SIZE as u64));
        if let Ok(f) = unsafe { page_table::unmap_frame(pml4, page_virt) } {
            let _ = unsafe { frame::free_frame(f) };
        }
    }

    FREE_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get the size (in bytes) of a vmalloc allocation.
///
/// Returns `None` if `ptr` is not a known vmalloc allocation.
pub fn vmalloc_size(ptr: *const u8) -> Option<usize> {
    let vaddr = ptr as u64;
    let state = STATE.lock();
    state.entries.iter()
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
    let active_bytes: u64 = state.entries.iter()
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
pub fn self_test() {
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
    serial_println!("[vmalloc]   Multi-page (4 × 16 KiB): OK (addr={:#x})", ptr2 as u64);

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
    unsafe { vfree(ptr3).expect("vfree should succeed"); }
    serial_println!("[vmalloc]   Re-alloc after free: OK");

    // Test 7: Zero-size rejection.
    let zero_result = vmalloc(0);
    assert!(zero_result.is_err());
    serial_println!("[vmalloc]   Zero-size rejected: OK");

    // Test 8: Stats.
    let st = stats();
    assert!(st.alloc_count >= 3);
    assert!(st.free_count >= 3);
    assert_eq!(st.active, 0);
    serial_println!("[vmalloc]   Stats: OK (allocs={}, frees={}, active={})",
        st.alloc_count, st.free_count, st.active);

    serial_println!("[vmalloc] Self-test PASSED");
}

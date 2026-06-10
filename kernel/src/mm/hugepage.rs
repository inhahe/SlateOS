//! Huge page support (2 MiB pages).
//!
//! x86_64 supports two sizes of huge pages:
//! - **2 MiB** — PD-level (Page Directory) huge pages.
//! - **1 GiB** — PDPT-level huge pages (not yet supported here).
//!
//! Huge pages reduce TLB pressure by covering more virtual address space
//! per TLB entry.  A single 2 MiB TLB entry replaces 512 standard 4 KiB
//! entries, dramatically improving performance for large, contiguous
//! memory regions (heaps, framebuffers, DMA buffers, large file mappings).
//!
//! ## TLB Coverage Comparison
//!
//! | Page Size | TLB Entries Needed | Coverage (512 entries) |
//! |-----------|-------------------|------------------------|
//! | 4 KiB     | 512               | 2 MiB                  |
//! | 2 MiB     | 1                 | 1 GiB (512 × 2 MiB)   |
//! | 1 GiB     | 1                 | 512 GiB                |
//!
//! ## Design
//!
//! This module provides explicit huge page allocation and mapping.  It
//! does NOT implement transparent huge pages (THP) — coalescence of
//! contiguous standard pages into huge pages is left for future work.
//!
//! The physical allocation uses the buddy allocator at order 7
//! (2^7 × 16 KiB = 2 MiB), which guarantees natural 2 MiB alignment.
//!
//! ## Mapping
//!
//! A 2 MiB huge page is created by setting the PS (Page Size, bit 7) flag
//! in a Page Directory entry.  Instead of pointing to a Page Table, the
//! PDE directly contains the physical address of the 2 MiB region.
//!
//! ## References
//!
//! - Intel SDM Vol. 3A §4.5: "4-Level Paging" — PAT/PS bit encoding
//! - Linux `mm/huge_memory.c`, `mm/hugetlb.c`
//! - Linux Documentation/admin-guide/mm/hugetlbpage.rst

#![allow(dead_code)]

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame};
use crate::mm::page_table::{self, PageFlags, PageTableEntry, VirtAddr};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Size of a 2 MiB huge page in bytes.
pub const HUGE_PAGE_SIZE_2M: usize = 2 * 1024 * 1024;

/// Buddy allocator order for a 2 MiB region (2^7 × 16 KiB = 2 MiB).
const ORDER_2M: usize = 7;

/// Number of standard 16 KiB frames in a 2 MiB huge page.
const FRAMES_PER_HUGE_2M: usize = 1 << ORDER_2M; // 128

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Number of 2 MiB huge pages currently mapped.
static HUGE_PAGES_MAPPED: AtomicU32 = AtomicU32::new(0);

/// Total 2 MiB huge pages ever allocated.
static HUGE_PAGES_ALLOCATED: AtomicU64 = AtomicU64::new(0);

/// Total 2 MiB huge pages ever freed.
static HUGE_PAGES_FREED: AtomicU64 = AtomicU64::new(0);

/// Allocation failures (out of contiguous memory).
static HUGE_ALLOC_FAILURES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Allocation
// ---------------------------------------------------------------------------

/// Allocate a 2 MiB contiguous, naturally-aligned physical region.
///
/// Returns the physical frame representing the base of the 2 MiB region.
/// The buddy allocator guarantees 2 MiB alignment for order-7 allocations.
///
/// Returns `Err` if insufficient contiguous physical memory is available.
pub fn alloc_huge_2m() -> KernelResult<PhysFrame> {
    match frame::alloc_order(ORDER_2M) {
        Ok(frame) => {
            HUGE_PAGES_ALLOCATED.fetch_add(1, Ordering::Relaxed);
            HUGE_PAGES_MAPPED.fetch_add(1, Ordering::Relaxed);
            Ok(frame)
        }
        Err(e) => {
            HUGE_ALLOC_FAILURES.fetch_add(1, Ordering::Relaxed);
            Err(e)
        }
    }
}

/// Free a previously allocated 2 MiB huge page.
///
/// # Safety
///
/// The caller must ensure:
/// - `frame` was returned by `alloc_huge_2m()`.
/// - The region is not currently mapped anywhere.
/// - No references to any part of the region exist.
pub unsafe fn free_huge_2m(frame: PhysFrame) {
    // SAFETY: Caller guarantees the frame was allocated as order-7.
    let _ = unsafe { frame::free_order(frame, ORDER_2M) };
    HUGE_PAGES_FREED.fetch_add(1, Ordering::Relaxed);
    HUGE_PAGES_MAPPED.fetch_sub(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Mapping
// ---------------------------------------------------------------------------

/// Map a 2 MiB huge page at `virt` pointing to `phys`.
///
/// Creates a Page Directory entry with the PS (Page Size) bit set.
/// Both `virt` and `phys` must be 2 MiB-aligned.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The caller must ensure no existing mappings conflict.
/// - The virtual address range must not already have 4 KiB mappings
///   within the 2 MiB region (a PT must not exist for this PDE slot).
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn map_huge_2m(
    pml4_phys: u64,
    virt: VirtAddr,
    phys: PhysFrame,
    flags: PageFlags,
) -> KernelResult<()> {
    let vaddr = virt.as_u64();
    let paddr = phys.addr();

    // Alignment check: both must be 2 MiB-aligned.
    if !vaddr.is_multiple_of(HUGE_PAGE_SIZE_2M as u64) {
        return Err(KernelError::InvalidAddress);
    }
    if !paddr.is_multiple_of(HUGE_PAGE_SIZE_2M as u64) {
        return Err(KernelError::InvalidAddress);
    }

    // Canonicality check.
    if !virt.is_canonical() {
        return Err(KernelError::InvalidAddress);
    }

    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
    let user = virt.is_user();

    // Walk to the PD level (PML4 → PDPT → PD), creating intermediate
    // tables as needed.
    // SAFETY: pml4_phys is valid (caller guarantee).  walk_or_create
    // and read_entry target valid page table levels at valid indices.
    let pml4_idx = virt.pml4_index();
    let pdpt_phys = unsafe {
        page_table::walk_or_create(pml4_phys, pml4_idx, true, user, hhdm)?
    };

    let pdpt_idx = virt.pdpt_index();
    let pd_phys = unsafe {
        page_table::walk_or_create(pdpt_phys, pdpt_idx, true, user, hhdm)?
    };

    // Check that the PD slot is not already occupied.
    let pd_idx = virt.pd_index();
    let existing = unsafe { page_table::read_entry(pd_phys, pd_idx, hhdm) };
    if existing.is_present() {
        return Err(KernelError::AlreadyExists);
    }

    // Create the huge page PDE: physical address + flags + HUGE_PAGE bit.
    let pde_flags = flags | PageFlags::PRESENT | PageFlags::HUGE_PAGE;
    let entry = PageTableEntry::new(paddr, pde_flags);

    // SAFETY: pd_phys is valid (from walk_or_create), pd_idx < 512.
    unsafe { page_table::write_entry(pd_phys, pd_idx, entry, hhdm); }

    Ok(())
}

/// Unmap a 2 MiB huge page at `virt`.
///
/// Returns the physical address of the unmapped region.
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - `virt` must be 2 MiB-aligned and must currently have a huge page
///   mapping.
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn unmap_huge_2m(
    pml4_phys: u64,
    virt: VirtAddr,
) -> KernelResult<PhysFrame> {
    let vaddr = virt.as_u64();
    if !vaddr.is_multiple_of(HUGE_PAGE_SIZE_2M as u64) {
        return Err(KernelError::InvalidAddress);
    }

    let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;

    // Walk to the PD level.
    // SAFETY: pml4_phys is valid (caller guarantee).  walk_or_create
    // and read_entry target valid page table levels.
    let pml4_idx = virt.pml4_index();
    let pdpt_phys = unsafe {
        page_table::walk_or_create(pml4_phys, pml4_idx, false, false, hhdm)?
    };

    let pdpt_idx = virt.pdpt_index();
    let pd_phys = unsafe {
        page_table::walk_or_create(pdpt_phys, pdpt_idx, false, false, hhdm)?
    };

    // Read and verify the PDE is a huge page.
    let pd_idx = virt.pd_index();
    let entry = unsafe { page_table::read_entry(pd_phys, pd_idx, hhdm) };

    if !entry.is_present() {
        return Err(KernelError::NotFound);
    }
    if !entry.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let phys_addr = entry.phys_addr();

    // Clear the PDE.
    let empty = PageTableEntry::new(0, PageFlags::empty());
    // SAFETY: pd_phys is valid from walk_or_create; pd_idx < 512.
    unsafe { page_table::write_entry(pd_phys, pd_idx, empty, hhdm); }

    // Flush the TLB for this 2 MiB region.
    // SAFETY: We're invalidating our own mapping which we just removed.
    unsafe { flush_tlb_range(vaddr, HUGE_PAGE_SIZE_2M); }

    PhysFrame::from_addr(phys_addr).ok_or(KernelError::InvalidAddress)
}

/// Flush TLB entries for a virtual address range.
///
/// Uses INVLPG for each page in the range.  For huge pages, a single
/// INVLPG on any address within the huge page flushes the entire entry.
#[allow(clippy::arithmetic_side_effects)]
unsafe fn flush_tlb_range(vaddr: u64, size: usize) {
    // For a 2 MiB huge page, one INVLPG suffices.
    // For safety, issue INVLPG at the base address.
    // SAFETY: INVLPG invalidates the TLB entry for the given address.
    // The caller guarantees vaddr was a valid mapping that was just removed.
    unsafe {
        core::arch::asm!(
            "invlpg [{}]",
            in(reg) vaddr,
            options(nostack, preserves_flags)
        );
    }
    // If the range is larger than 2 MiB, flush additional addresses.
    let mut offset = HUGE_PAGE_SIZE_2M;
    while offset < size {
        let addr = vaddr.wrapping_add(offset as u64);
        // SAFETY: Continuation of TLB invalidation for the same range.
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) addr,
                options(nostack, preserves_flags)
            );
        }
        offset = offset.wrapping_add(HUGE_PAGE_SIZE_2M);
    }
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Check if a virtual address is mapped as a 2 MiB huge page.
///
/// Returns `Some(phys_base)` if the address falls within a huge page mapping.
#[allow(clippy::arithmetic_side_effects)]
pub fn is_huge_mapped(pml4_phys: u64, virt: VirtAddr) -> Option<u64> {
    let hhdm = page_table::hhdm()?;

    let pml4_idx = virt.pml4_index();
    // SAFETY: pml4_phys and indices are valid.
    let pml4e = unsafe { page_table::read_entry(pml4_phys, pml4_idx, hhdm) };
    if !pml4e.is_present() { return None; }

    let pdpt_phys = pml4e.phys_addr();
    let pdpt_idx = virt.pdpt_index();
    let pdpte = unsafe { page_table::read_entry(pdpt_phys, pdpt_idx, hhdm) };
    if !pdpte.is_present() { return None; }
    if pdpte.is_huge() {
        // 1 GiB huge page (not created by us, but respect it).
        return Some(pdpte.phys_addr());
    }

    let pd_phys = pdpte.phys_addr();
    let pd_idx = virt.pd_index();
    let pde = unsafe { page_table::read_entry(pd_phys, pd_idx, hhdm) };
    if !pde.is_present() { return None; }
    if pde.is_huge() {
        return Some(pde.phys_addr());
    }

    None // Standard 4 KiB mapping (not huge).
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Huge page statistics.
#[derive(Debug, Clone, Copy)]
pub struct HugePageStats {
    /// Currently mapped 2 MiB huge pages.
    pub mapped: u32,
    /// Total ever allocated.
    pub allocated: u64,
    /// Total ever freed.
    pub freed: u64,
    /// Allocation failures.
    pub failures: u64,
}

/// Get huge page statistics.
#[must_use]
pub fn stats() -> HugePageStats {
    HugePageStats {
        mapped: HUGE_PAGES_MAPPED.load(Ordering::Relaxed),
        allocated: HUGE_PAGES_ALLOCATED.load(Ordering::Relaxed),
        freed: HUGE_PAGES_FREED.load(Ordering::Relaxed),
        failures: HUGE_ALLOC_FAILURES.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the huge page subsystem.
pub fn self_test() {
    serial_println!("[hugepage] Running self-test...");

    let pml4 = page_table::active_pml4_phys();

    // Test 1: Allocate a 2 MiB huge page.
    let frame = match alloc_huge_2m() {
        Ok(f) => f,
        Err(e) => {
            serial_println!("[hugepage]   Allocation failed: {:?} (insufficient memory?)", e);
            serial_println!("[hugepage] Self-test SKIPPED (not enough contiguous memory)");
            return;
        }
    };
    let phys_addr = frame.addr();
    assert_eq!(phys_addr % HUGE_PAGE_SIZE_2M as u64, 0, "must be 2 MiB-aligned");
    serial_println!("[hugepage]   Alloc 2 MiB: OK (phys={:#x})", phys_addr);

    // Test 2: Map the huge page at a kernel virtual address.
    // Use an address in the kernel's reserved huge-page region.
    // 0xFFFF_C200_0000_0000 is well within kernel space and unlikely to conflict.
    let test_virt = VirtAddr::new(0xFFFF_C200_0000_0000);
    let map_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;

    // SAFETY: pml4 is valid, test_virt is unused kernel space.
    let map_result = unsafe { map_huge_2m(pml4, test_virt, frame, map_flags) };
    assert!(map_result.is_ok(), "huge page map should succeed");
    serial_println!("[hugepage]   Map 2 MiB: OK (virt={:#x})", test_virt.as_u64());

    // Test 3: Verify the mapping is detected as a huge page.
    let detected = is_huge_mapped(pml4, test_virt);
    assert_eq!(detected, Some(phys_addr), "should detect huge page mapping");
    serial_println!("[hugepage]   Detect huge: OK");

    // Test 4: Write and read through the huge page mapping.
    let ptr = test_virt.as_u64() as *mut u64;
    // SAFETY: test_virt was just mapped to a 2 MiB page; reads/writes within it are valid.
    unsafe {
        // Write at the start of the huge page.
        core::ptr::write_volatile(ptr, 0xDEAD_BEEF_CAFE_F00D);
        let val = core::ptr::read_volatile(ptr);
        assert_eq!(val, 0xDEAD_BEEF_CAFE_F00D);

        // Write near the end of the huge page (offset 2 MiB - 8).
        let end_ptr = (test_virt.as_u64() + HUGE_PAGE_SIZE_2M as u64 - 8) as *mut u64;
        core::ptr::write_volatile(end_ptr, 0x1234_5678_9ABC_DEF0);
        let val2 = core::ptr::read_volatile(end_ptr);
        assert_eq!(val2, 0x1234_5678_9ABC_DEF0);
    }
    serial_println!("[hugepage]   Read/write: OK (start + end of 2 MiB region)");

    // Test 5: Double-map prevention.
    // SAFETY: pml4 is valid; testing that double-map is rejected.
    let double_result = unsafe { map_huge_2m(pml4, test_virt, frame, map_flags) };
    assert!(double_result.is_err(), "double-map should fail");
    serial_println!("[hugepage]   Double-map rejected: OK");

    // Test 6: Alignment rejection.
    let unaligned = VirtAddr::new(test_virt.as_u64() + 4096);
    // SAFETY: pml4 is valid; testing that unaligned virt is rejected.
    let align_result = unsafe { map_huge_2m(pml4, unaligned, frame, map_flags) };
    assert!(align_result.is_err(), "unaligned virt should fail");
    serial_println!("[hugepage]   Alignment check: OK");

    // Test 7: Unmap the huge page.
    // SAFETY: pml4 is valid; test_virt has a huge mapping from test 2.
    let unmap_result = unsafe { unmap_huge_2m(pml4, test_virt) };
    assert!(unmap_result.is_ok());
    let returned_phys = unmap_result.unwrap();
    assert_eq!(returned_phys.addr(), phys_addr);
    serial_println!("[hugepage]   Unmap 2 MiB: OK");

    // Test 8: Verify no longer detected as huge.
    let after_unmap = is_huge_mapped(pml4, test_virt);
    assert_eq!(after_unmap, None, "should not detect after unmap");
    serial_println!("[hugepage]   Post-unmap check: OK");

    // Test 9: Free the physical memory.
    // SAFETY: returned_phys is the frame we allocated and just unmapped.
    unsafe { free_huge_2m(returned_phys); }
    serial_println!("[hugepage]   Free 2 MiB: OK");

    // Test 10: Stats.
    let st = stats();
    assert!(st.allocated >= 1);
    assert!(st.freed >= 1);
    serial_println!("[hugepage]   Stats: OK (alloc={}, freed={}, failures={})",
        st.allocated, st.freed, st.failures);

    serial_println!("[hugepage] Self-test PASSED");
}

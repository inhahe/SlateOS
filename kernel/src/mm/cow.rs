//! Copy-on-Write (CoW) page fault resolution.
//!
//! When multiple address spaces share a physical page (e.g., after
//! `duplicate_user_pages` for process creation, or shared library text
//! pages), the shared pages are mapped read-only with the COW bit set
//! (bit 9 in the PTE).  A write to a COW page triggers a page fault
//! (present + write violation), which the CoW handler resolves by:
//!
//! 1. **Checking the refcount** of the physical frame.
//! 2. **If refcount > 1**: allocate a new frame, copy the old contents,
//!    decrement the old frame's refcount, update the PTE to point to
//!    the new frame with WRITABLE set and COW cleared.
//! 3. **If refcount == 1**: we're the last reference — just set WRITABLE
//!    and clear COW in the existing PTE (no copy needed).
//!
//! This defers page copying until the first write, saving memory and
//! time when pages are read-only or never written after sharing.
//!
//! ## Usage
//!
//! The CoW handler is called from the page fault path (both kernel and
//! user-space).  It operates on individual 4 KiB hardware pages because
//! our 16 KiB frames are mapped as 4 consecutive 4 KiB PTEs, and CoW
//! is tracked per-PTE.
//!
//! ## References
//!
//! - Linux `mm/memory.c` `do_wp_page()` — CoW fault handler
//! - Linux `mm/memory.c` `copy_page_range()` — PTE duplication for fork

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
use crate::mm::page_table::{self, PageFlags, PageTableEntry, VirtAddr};
use crate::serial_println;

/// Size of a single 4 KiB hardware page.
const HW_PAGE_SIZE: usize = 4096;

/// Number of 4 KiB hardware pages per 16 KiB frame.
const HW_PAGES_PER_FRAME: usize = FRAME_SIZE / HW_PAGE_SIZE;

// ---------------------------------------------------------------------------
// CoW fault resolution
// ---------------------------------------------------------------------------

/// Resolve a Copy-on-Write page fault.
///
/// Called when a write fault occurs on a present page with the COW bit
/// set.  Determines whether to copy the page (shared) or just mark it
/// writable (last reference).
///
/// ## Arguments
///
/// - `pml4_phys`: the PML4 physical address of the faulting address space.
/// - `fault_addr`: the faulting virtual address (not necessarily page-aligned).
///
/// ## Returns
///
/// `Ok(())` if the fault was resolved (the CPU should retry the write).
///
/// ## Errors
///
/// - [`KernelError::PageFault`] — the page is not a CoW page.
/// - [`KernelError::OutOfMemory`] — no physical frame available for the copy.
/// - [`KernelError::NotSupported`] — subsystem not initialized.
#[allow(clippy::arithmetic_side_effects)]
pub fn resolve_cow_fault(pml4_phys: u64, fault_addr: u64) -> KernelResult<()> {
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

    // Align down to the 4 KiB hardware page boundary.
    let hw_page_base = fault_addr & !(HW_PAGE_SIZE as u64 - 1);
    let virt = VirtAddr::new(hw_page_base);

    // Read the current PTE.
    // SAFETY: pml4_phys is a valid PML4 (caller guarantee).
    let pte = unsafe { read_pte(pml4_phys, virt, hhdm)? };

    // Verify this is actually a CoW page.
    if !pte.is_present() || !pte.is_cow() {
        return Err(KernelError::PageFault);
    }

    let old_phys = pte.phys_addr();

    // Determine the 16 KiB frame that contains this 4 KiB page.
    // CoW refcounting is per-frame (the buddy allocator operates on
    // 16 KiB frames), so we check the frame's refcount.
    let frame_base = old_phys & !(FRAME_SIZE as u64 - 1);
    let frame = PhysFrame::from_addr(frame_base)
        .ok_or(KernelError::InternalError)?;
    let rc = frame::refcount(frame);

    if rc <= 1 {
        // We're the sole owner — just make pages writable (no copy).
        //
        // OPT: Eagerly resolve all 4 sibling 4 KiB pages within the
        // same 16 KiB frame, not just the faulting page.  This prevents
        // up to 3 additional CoW faults for pages that share the same
        // frame.  Each frame is mapped as 4 consecutive PTEs, so the
        // sibling pages are at predictable virtual addresses.
        //
        // Based on Linux mm/memory.c do_wp_page() which also batches
        // nearby pages to amortize TLB flushes and fault overhead.
        let page_index = ((old_phys - frame_base) as usize) / HW_PAGE_SIZE;
        let group_virt_base = hw_page_base - (page_index as u64 * HW_PAGE_SIZE as u64);

        for i in 0..HW_PAGES_PER_FRAME {
            let sibling_virt = VirtAddr::new(group_virt_base + (i as u64 * HW_PAGE_SIZE as u64));

            // SAFETY: pml4_phys is valid (same address space).
            if let Ok(sibling_pte) = unsafe { read_pte(pml4_phys, sibling_virt, hhdm) } {
                if sibling_pte.is_present() && sibling_pte.is_cow() {
                    // Verify it's part of the same physical frame.
                    let sib_frame_base = sibling_pte.phys_addr() & !(FRAME_SIZE as u64 - 1);
                    if sib_frame_base == frame_base {
                        let mut new_flags = sibling_pte.flags() | PageFlags::WRITABLE;
                        new_flags = PageFlags::from_bits(new_flags.bits() & !PageFlags::COW.bits());
                        let new_pte = PageTableEntry::new(sibling_pte.phys_addr(), new_flags);
                        // SAFETY: pml4_phys is valid, sibling_virt is in the same frame group.
                        unsafe { write_pte(pml4_phys, sibling_virt, new_pte, hhdm).ok(); }
                    }
                }
            }
        }

        // Flush TLB for the entire frame group (4 pages).
        crate::tlb::flush_range(group_virt_base, HW_PAGES_PER_FRAME as u32);

        return Ok(());
    }

    // Shared page (refcount > 1) — need to copy.
    //
    // OPT: Batch all 4 pages of the 16 KiB frame together.  Instead of
    // copying just the faulting 4 KiB page (wasting 12 KiB of the new
    // frame), we scan all 4 sibling PTEs in the frame group.  Any that
    // are present + CoW + point to the same old frame are copied into
    // the corresponding offset of a single new frame.  This:
    //   1. Eliminates up to 3 additional CoW faults
    //   2. Uses the full 16 KiB of the allocated frame (no waste)
    //   3. Amortizes the TLB flush across all 4 pages
    //
    // Refcounting: each resolved PTE had its own ref_inc during fork, so
    // we decrement the old frame's refcount once per resolved PTE.
    //
    // Based on Linux mm/memory.c do_wp_page() + copy_page_range() which
    // track refcounts per-PTE and batch nearby resolutions.

    // Compute the frame group's virtual base address.
    let page_index = ((old_phys - frame_base) as usize) / HW_PAGE_SIZE;
    let group_virt_base = hw_page_base - (page_index as u64 * HW_PAGE_SIZE as u64);

    // Allocate one new 16 KiB frame for all resolved siblings.
    let new_frame = frame::alloc_frame()?;
    let new_phys = new_frame.addr();

    let mut pages_resolved = 0u32;

    for i in 0..HW_PAGES_PER_FRAME {
        let sibling_virt = VirtAddr::new(group_virt_base + (i as u64 * HW_PAGE_SIZE as u64));

        // SAFETY: pml4_phys is valid (same address space).
        let sibling_pte = match unsafe { read_pte(pml4_phys, sibling_virt, hhdm) } {
            Ok(pte) => pte,
            Err(_) => continue, // Unmapped intermediate — skip.
        };

        if !sibling_pte.is_present() || !sibling_pte.is_cow() {
            continue; // Not a CoW page — leave it alone.
        }

        // Verify this sibling references the same physical frame.
        let sib_phys = sibling_pte.phys_addr();
        let sib_frame_base = sib_phys & !(FRAME_SIZE as u64 - 1);
        if sib_frame_base != frame_base {
            continue; // Different frame — not our business.
        }

        // Compute offsets within old and new frames.
        let sib_page_offset = (sib_phys - frame_base) as usize;
        let new_4k_phys = new_phys + sib_page_offset as u64;

        // Copy 4 KiB from old frame to new frame via HHDM.
        let src = (sib_phys + hhdm) as *const u8;
        let dst = (new_4k_phys + hhdm) as *mut u8;
        // SAFETY: Both addresses are valid (old is allocated + HHDM,
        // new is freshly allocated + HHDM).  No overlap (different frames).
        unsafe {
            core::ptr::copy_nonoverlapping(src, dst, HW_PAGE_SIZE);
        }

        // Update PTE: point to new frame, set WRITABLE, clear COW.
        let mut new_flags = sibling_pte.flags() | PageFlags::WRITABLE;
        new_flags = PageFlags::from_bits(new_flags.bits() & !PageFlags::COW.bits());
        let new_pte = PageTableEntry::new(new_4k_phys, new_flags);

        // SAFETY: pml4_phys is valid, sibling_virt is in the same group.
        unsafe { write_pte(pml4_phys, sibling_virt, new_pte, hhdm).ok(); }
        pages_resolved += 1;
    }

    // Decrement the old frame's refcount once per resolved PTE.
    // Each PTE had its own ref_inc during fork/duplication.
    // SAFETY: old frame is a valid allocated frame.
    for _ in 0..pages_resolved {
        let _ = unsafe { frame::ref_dec(frame) };
    }

    // Flush TLB for the entire frame group.
    crate::tlb::flush_range(group_virt_base, HW_PAGES_PER_FRAME as u32);

    Ok(())
}

// ---------------------------------------------------------------------------
// PTE read/write helpers (walk page table to leaf PTE)
// ---------------------------------------------------------------------------

/// Read the leaf PTE for a 4 KiB virtual address.
///
/// Walks the 4-level page table to find the PT-level entry.
///
/// # Safety
///
/// `pml4_phys` must be a valid PML4 table.
unsafe fn read_pte(pml4_phys: u64, virt: VirtAddr, hhdm: u64) -> KernelResult<PageTableEntry> {
    // Walk PML4 → PDPT → PD → PT.
    // SAFETY: pml4_phys is valid (caller guarantee).
    let pml4e = unsafe { page_table::read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    let pdpte = unsafe { page_table::read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() || pdpte.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pde = unsafe { page_table::read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() || pde.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pt = pde.phys_addr();
    Ok(unsafe { page_table::read_entry(pt, virt.pt_index(), hhdm) })
}

/// Write a PTE at the leaf level for a 4 KiB virtual address.
///
/// Walks the page table to find the PT, then writes the entry.
/// The intermediate levels must already exist (no creation).
///
/// # Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - The caller must flush the TLB after calling this.
unsafe fn write_pte(
    pml4_phys: u64,
    virt: VirtAddr,
    entry: PageTableEntry,
    hhdm: u64,
) -> KernelResult<()> {
    // Walk PML4 → PDPT → PD → PT.
    let pml4e = unsafe { page_table::read_entry(pml4_phys, virt.pml4_index(), hhdm) };
    if !pml4e.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    let pdpte = unsafe { page_table::read_entry(pml4e.phys_addr(), virt.pdpt_index(), hhdm) };
    if !pdpte.is_present() || pdpte.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pde = unsafe { page_table::read_entry(pdpte.phys_addr(), virt.pd_index(), hhdm) };
    if !pde.is_present() || pde.is_huge() {
        return Err(KernelError::InvalidAddress);
    }

    let pt = pde.phys_addr();
    // SAFETY: pt is a valid page table, virt.pt_index() < 512.
    unsafe { page_table::write_entry(pt, virt.pt_index(), entry, hhdm); }

    Ok(())
}

// ---------------------------------------------------------------------------
// Mark a page as CoW (for use by address space duplication)
// ---------------------------------------------------------------------------

/// Mark a mapped 4 KiB page as Copy-on-Write.
///
/// Clears the WRITABLE flag and sets the COW bit.  The next write to
/// this page will trigger a page fault that [`resolve_cow_fault`] handles.
///
/// ## Safety
///
/// - `pml4_phys` must be a valid PML4 table.
/// - `virt` must be a 4 KiB-aligned virtual address that is currently
///   mapped and present.
/// - The caller must flush the TLB for this address after calling.
/// - The physical frame's refcount must be incremented to reflect the
///   additional reference (the caller is responsible for this).
pub unsafe fn mark_cow(pml4_phys: u64, virt: VirtAddr) -> KernelResult<()> {
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

    let pte = unsafe { read_pte(pml4_phys, virt, hhdm)? };
    if !pte.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    // Already CoW — nothing to do.
    if pte.is_cow() {
        return Ok(());
    }

    // Build new flags: remove WRITABLE, add COW.
    let mut flags = pte.flags();
    flags = PageFlags::from_bits(flags.bits() & !PageFlags::WRITABLE.bits());
    flags = flags | PageFlags::COW;

    let new_pte = PageTableEntry::new(pte.phys_addr(), flags);
    // SAFETY: pml4_phys valid, virt is an existing present mapping.
    unsafe { write_pte(pml4_phys, virt, new_pte, hhdm)?; }

    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for CoW infrastructure.
///
/// Tests the refcount API, COW flag manipulation, and (where possible)
/// the CoW fault resolution logic.  Full end-to-end testing (actual
/// page faults) requires a user-space test process.
pub fn self_test() {
    serial_println!("[cow] Running self-test...");

    // Test 1: Refcount API.
    test_refcount();

    // Test 2: COW flag in PTE.
    test_cow_flag();

    serial_println!("[cow] Self-test PASSED");
}

/// Test frame refcount increment / decrement.
fn test_refcount() {
    // Allocate a frame — refcount should start at 1.
    let frame = frame::alloc_frame().expect("alloc for refcount test");
    let rc = frame::refcount(frame);
    assert!(rc == 1, "initial refcount should be 1, got {}", rc);

    // Increment refcount (simulating CoW sharing).
    // SAFETY: frame is allocated, we hold the only reference.
    unsafe { frame::ref_inc(frame).expect("ref_inc") };
    let rc = frame::refcount(frame);
    assert!(rc == 2, "refcount after inc should be 2, got {}", rc);

    // Decrement back to 1.
    // SAFETY: frame is allocated.
    let new_rc = unsafe { frame::ref_dec(frame).expect("ref_dec") };
    assert!(new_rc == 1, "refcount after dec should be 1, got {}", new_rc);

    // Free the frame (refcount goes 1 → 0, actually freed).
    // SAFETY: we're the sole owner.
    unsafe { frame::free_frame(frame).expect("free") };

    serial_println!("[cow]   Refcount API: OK");
}

/// Test COW PTE flag.
fn test_cow_flag() {
    use crate::mm::page_table::{PageFlags, PageTableEntry};

    // Create a PTE with COW set.
    let flags = PageFlags::PRESENT | PageFlags::USER_ACCESSIBLE | PageFlags::COW;
    let pte = PageTableEntry::new(0x1000, flags);
    assert!(pte.is_present(), "COW PTE should be present");
    assert!(pte.is_cow(), "COW PTE should have COW bit");
    assert!(
        !pte.flags().contains(PageFlags::WRITABLE),
        "COW PTE should not be writable"
    );

    // A normal writable PTE should not be COW.
    let flags2 = PageFlags::PRESENT | PageFlags::WRITABLE;
    let pte2 = PageTableEntry::new(0x2000, flags2);
    assert!(!pte2.is_cow(), "normal PTE should not be COW");

    serial_println!("[cow]   COW PTE flag: OK");
}

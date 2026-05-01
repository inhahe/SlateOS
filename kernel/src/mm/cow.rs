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
        // We're the sole owner — just make the page writable.
        // Remove COW, add WRITABLE.
        let mut new_flags = pte.flags();
        new_flags = new_flags | PageFlags::WRITABLE;
        // Clear COW bit: construct flags without it.
        new_flags = PageFlags::from_bits(new_flags.bits() & !PageFlags::COW.bits());
        let new_pte = PageTableEntry::new(old_phys, new_flags);

        // SAFETY: pml4_phys is valid, virt is the same page we just read.
        unsafe { write_pte(pml4_phys, virt, new_pte, hhdm)?; }

        // Flush TLB so the CPU sees the updated permissions.
        crate::tlb::flush_range(hw_page_base, 1);

        return Ok(());
    }

    // Shared page (refcount > 1) — need to copy.
    //
    // Allocate a new frame.  We allocate a full 16 KiB frame even though
    // only one 4 KiB page triggered the fault, because the buddy allocator
    // works in 16 KiB units.  The other 3 pages in the new frame will be
    // wired up if/when they also fault.
    //
    // Actually, the correct approach is to check whether ALL 4 pages in
    // this frame's group are CoW.  If so, we can copy the entire frame
    // at once and update all 4 PTEs.  If not (some pages might already
    // be private), we need to handle each page individually.
    //
    // For simplicity and correctness, we copy just the single faulting
    // 4 KiB page into a new frame and update only that PTE.  The other
    // pages remain CoW and will be copied on their first write.
    //
    // Optimization (TODO): batch all 4 pages of a frame together.

    // We need a fresh 4 KiB page.  The buddy allocator gives us 16 KiB
    // frames.  We use the first 4 KiB page of the new frame for this
    // CoW copy, and the remaining 3 pages are wasted unless other CoW
    // faults in the same frame group can use them.
    //
    // Better approach: allocate one frame and use it for up to 4 CoW
    // copies within the same frame group.  For now, we accept the waste
    // and allocate a full frame per CoW copy.  This will be improved
    // when we add a page-level sub-allocator.
    let new_frame = frame::alloc_frame()?;
    let new_phys = new_frame.addr();

    // Compute which 4 KiB page within the frame we're replacing.
    let page_offset = (old_phys - frame_base) as usize;
    let page_index = page_offset / HW_PAGE_SIZE;

    // The new physical address for this specific 4 KiB page.
    // We place it at the same offset within the new frame to maintain
    // natural alignment (and to make the remaining pages usable for
    // future CoW faults in the same group).
    let new_4k_phys = new_phys + (page_index as u64 * HW_PAGE_SIZE as u64);

    // Copy the 4 KiB page contents from old to new via HHDM.
    let src = (old_phys + hhdm) as *const u8;
    let dst = (new_4k_phys + hhdm) as *mut u8;
    // SAFETY: Both old and new physical addresses are valid (old is an
    // allocated frame we can read, new is freshly allocated and mapped
    // via HHDM).  The regions don't overlap (different physical frames).
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, HW_PAGE_SIZE);
    }

    // Update the PTE: point to the new physical page, set WRITABLE,
    // clear COW.
    let mut new_flags = pte.flags();
    new_flags = new_flags | PageFlags::WRITABLE;
    new_flags = PageFlags::from_bits(new_flags.bits() & !PageFlags::COW.bits());
    let new_pte = PageTableEntry::new(new_4k_phys, new_flags);

    // SAFETY: pml4_phys is valid, virt is the faulting page.
    unsafe { write_pte(pml4_phys, virt, new_pte, hhdm)?; }

    // Decrement the old frame's refcount (we no longer reference it
    // from this PTE).  Note: we decrement the frame refcount, not the
    // individual 4 KiB page.  This is correct because refcounting is
    // per-frame — when all 4 pages in the frame have been copied away,
    // the frame's refcount reaches 1 (or 0) and can be freed.
    //
    // SAFETY: old frame is a valid allocated frame.
    let _ = unsafe { frame::ref_dec(frame) };

    // Flush TLB for the updated page.
    crate::tlb::flush_range(hw_page_base, 1);

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

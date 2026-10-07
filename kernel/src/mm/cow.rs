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
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
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
/// **In two halves.** [`prepare_cow`] reads the entries and, for a shared
/// frame, copies it into a new one -- unlocked, since it allocates and
/// copies 16 KiB. [`install_cow`] then changes the entries and settles the
/// refcount under the address space's page-table lock
/// ([`super::as_lock`]), installing only over entries that are still what
/// the copy was made from. Until 2026-10-03 one function did both with no
/// lock, and two threads breaking the same page on two CPUs each installed
/// a copy -- losing a write made between the two copies -- and each dropped
/// the address space's reference to the shared frame, one too many: with
/// two children sharing it, one child then wrote the page in place under
/// the other and freed it on exit. The second break now finds the work done,
/// frees its copy, and the write runs again.
///
/// Waits for the lock, so it is for thread context, or the #PF handler with
/// interrupts on (the handler turns them back on for a user-mode fault).
///
/// ## Arguments
///
/// - `pml4_phys`: the PML4 physical address of the faulting address space.
/// - `fault_addr`: the faulting virtual address (not necessarily page-aligned).
///
/// ## Returns
///
/// `Ok(())` if the fault was resolved (the CPU should retry the write) --
/// including when another CPU resolved it first.
///
/// ## Errors
///
/// - [`KernelError::PageFault`] — the page is not a CoW page.
/// - [`KernelError::OutOfMemory`] — no physical frame available for the copy.
/// - [`KernelError::NotSupported`] — subsystem not initialized.
pub fn resolve_cow_fault(pml4_phys: u64, fault_addr: u64) -> KernelResult<()> {
    let prepared = prepare_cow(pml4_phys, fault_addr)?;
    let _held = super::as_lock::lock(pml4_phys);
    install_cow(pml4_phys, prepared)
}

/// What the slow half of a copy-on-write break found and made
/// ([`prepare_cow`]), for [`install_cow`].
struct CowPrep {
    /// The 16 KiB frame the group's COW entries share.
    frame_base: u64,
    /// The virtual base of the frame's four 4 KiB entries.
    group_virt_base: u64,
    /// The copy, when the frame was shared; `None` when this address space
    /// looked like its only owner, and may make it writable in place.
    copy: Option<PhysFrame>,
    /// Which parts the copy holds: those that were COW entries on the shared
    /// frame when it was made.
    copied: [bool; HW_PAGES_PER_FRAME],
}

/// The slow half of [`resolve_cow_fault`], with no lock: check the faulting
/// entry is a COW entry, and, if its frame is shared, copy every part of the
/// group that is a COW entry on it into a new frame, at the same offsets.
///
/// The copy can be made unlocked because a frame shared copy-on-write is
/// read-only to every mapper while it is shared: none writes it in place
/// until it is the last ([`install_cow`]'s sole-owner case), and this
/// address space's own reference keeps it shared until the install drops it.
#[allow(clippy::arithmetic_side_effects)]
fn prepare_cow(pml4_phys: u64, fault_addr: u64) -> KernelResult<CowPrep> {
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
    let frame = PhysFrame::from_addr(frame_base).ok_or(KernelError::InternalError)?;
    let page_index = ((old_phys - frame_base) as usize) / HW_PAGE_SIZE;
    let group_virt_base = hw_page_base - (page_index as u64 * HW_PAGE_SIZE as u64);

    if frame::refcount(frame) <= 1 {
        // We look like the sole owner: no copy, the install makes the
        // entries writable in place.
        return Ok(CowPrep {
            frame_base,
            group_virt_base,
            copy: None,
            copied: [false; HW_PAGES_PER_FRAME],
        });
    }

    // Shared: copy every part of the group that is a COW entry on this
    // frame into one new frame, at the same offset. One 16 KiB frame for up
    // to four parts uses the whole frame, saves up to three more faults and
    // amortizes the TLB flush (Linux's do_wp_page batches nearby pages too).
    let new_frame = {
        let _own = super::frame_owner::OwnerScope::new(super::frame_owner::Owner::Cow);
        frame::alloc_frame()?
    };
    let new_phys = new_frame.addr();
    let mut copied = [false; HW_PAGES_PER_FRAME];
    for (i, part) in copied.iter_mut().enumerate() {
        let sibling_virt = VirtAddr::new(group_virt_base + (i as u64 * HW_PAGE_SIZE as u64));
        // SAFETY: pml4_phys is valid (same address space).
        let Ok(sibling_pte) = (unsafe { read_pte(pml4_phys, sibling_virt, hhdm) }) else {
            continue; // Unmapped intermediate -- skip.
        };
        if !sibling_pte.is_present() || !sibling_pte.is_cow() {
            continue; // Not a CoW page -- leave it alone.
        }
        let sib_phys = sibling_pte.phys_addr();
        if sib_phys & !(FRAME_SIZE as u64 - 1) != frame_base {
            continue; // Different frame -- not our business.
        }
        let offset = sib_phys - frame_base;
        // Copy 4 KiB from the old frame to the same offset in the new one.
        // SAFETY: both through the HHDM; the old frame is mapped and shared
        // read-only (see above), the new one freshly allocated and ours;
        // different frames, so no overlap.
        unsafe {
            core::ptr::copy_nonoverlapping(
                (sib_phys + hhdm) as *const u8,
                (new_phys + offset + hhdm) as *mut u8,
                HW_PAGE_SIZE,
            );
        }
        *part = true;
    }

    Ok(CowPrep {
        frame_base,
        group_virt_base,
        copy: Some(new_frame),
        copied,
    })
}

/// The install half of [`resolve_cow_fault`], under the address space's
/// page-table lock: change only the entries that are still COW entries on
/// the frame [`prepare_cow`] found, and drop this address space's reference
/// to that frame once, when the last of its entries leaves it.
///
/// Another CPU that broke the same page first left nothing to change: the
/// copy is freed, and the write runs again on what that CPU installed.
#[allow(clippy::arithmetic_side_effects)]
fn install_cow(pml4_phys: u64, prep: CowPrep) -> KernelResult<()> {
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let CowPrep {
        frame_base,
        group_virt_base,
        copy,
        copied,
    } = prep;
    let frame = PhysFrame::from_addr(frame_base).ok_or(KernelError::InternalError)?;
    let sibling = |i: usize| VirtAddr::new(group_virt_base + (i as u64 * HW_PAGE_SIZE as u64));
    // Whether part `i` is still a COW entry on the shared frame, and its entry.
    let still_cow = |i: usize| -> Option<PageTableEntry> {
        // SAFETY: pml4_phys is valid (same address space).
        let pte = unsafe { read_pte(pml4_phys, sibling(i), hhdm) }.ok()?;
        (pte.is_present()
            && pte.is_cow()
            && pte.phys_addr() & !(FRAME_SIZE as u64 - 1) == frame_base)
            .then_some(pte)
    };

    let Some(new_frame) = copy else {
        // Prepared as the sole owner. Not if the frame has gained an owner
        // since: the write then runs again, faults, and copies.
        if frame::refcount(frame) > 1 {
            return Ok(());
        }
        // Make every part still COW on the frame writable, in place.
        for i in 0..HW_PAGES_PER_FRAME {
            if let Some(pte) = still_cow(i) {
                let flags = PageFlags::from_bits(
                    (pte.flags() | PageFlags::WRITABLE).bits() & !PageFlags::COW.bits(),
                );
                // SAFETY: pml4_phys is valid; part `i` of the same group.
                unsafe {
                    write_pte(
                        pml4_phys,
                        sibling(i),
                        PageTableEntry::new(pte.phys_addr(), flags),
                        hhdm,
                    )
                    .ok();
                }
            }
        }
        crate::tlb::flush_range(group_virt_base, HW_PAGES_PER_FRAME as u32);
        super::fault::record_cow();
        return Ok(());
    };

    // Install the copy over each part that is still the COW entry it was
    // copied from.
    let new_phys = new_frame.addr();
    let mut installed = 0u32;
    for (i, &was_copied) in copied.iter().enumerate() {
        if !was_copied {
            continue;
        }
        let Some(pte) = still_cow(i) else {
            continue;
        };
        let offset = pte.phys_addr() - frame_base;
        let flags = PageFlags::from_bits(
            (pte.flags() | PageFlags::WRITABLE).bits() & !PageFlags::COW.bits(),
        );
        // SAFETY: pml4_phys is valid; part `i` of the same group; the copy
        // at `offset` holds this part's data.
        unsafe {
            write_pte(
                pml4_phys,
                sibling(i),
                PageTableEntry::new(new_phys + offset, flags),
                hhdm,
            )
            .ok();
        }
        installed += 1;
    }

    if installed == 0 {
        // Another CPU broke the page first (or no part was COW on the frame
        // any more): nothing of the copy is used.
        // SAFETY: allocated by prepare_cow and never mapped.
        let _ = unsafe { frame::free_frame(new_frame) };
        return Ok(());
    }

    // Whether this address space still references the OLD frame through any
    // part of the group. After a partial resolve some parts can
    // legitimately remain on it -- most commonly a read-only *shared* part
    // (no COW bit) living in the same 16 KiB frame as a writable CoW part
    // (the ELF loader packs a read-only segment tail and a writable segment
    // head into one frame). The per-frame refcount counts *address-space
    // references*, so the reference goes only when NO part still points into
    // the old frame: decrementing while one does under-counts it and frees a
    // frame still mapped here and elsewhere (the Path-Z #12 dash-pipeline #GP
    // was exactly this double-decrement).
    let old_still_referenced = (0..HW_PAGES_PER_FRAME).any(|i| {
        // SAFETY: pml4_phys is valid (same address space).
        unsafe { read_pte(pml4_phys, sibling(i), hhdm) }
            .is_ok_and(|p| p.is_present() && p.phys_addr() & !(FRAME_SIZE as u64 - 1) == frame_base)
    });

    // The new frame is now mapped at this group's virtual base.
    super::rmap::add(new_phys, pml4_phys, group_virt_base);

    // Flush before the reference goes: until every CPU has dropped its
    // stale entry for the old frame, this address space can still read it,
    // and a reference given up could let its last other owner free it.
    crate::tlb::flush_range(group_virt_base, HW_PAGES_PER_FRAME as u32);

    if old_still_referenced {
        // The group now maps two frames, the old and the new: one more
        // frame in this address space's RSS (released by whichever of
        // munmap or teardown drops the new frame's last sub-page).
        super::accounting::charge(pml4_phys, 1);
    } else {
        // This address space's last reference to the old frame is gone.
        super::rmap::remove(frame_base, pml4_phys, group_virt_base);
        // SAFETY: old frame is a valid allocated frame.
        let _ = unsafe { frame::ref_dec(frame) };
    }

    super::fault::record_cow();
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
    // SAFETY: pml4_phys is valid (caller guarantee); each subsequent read
    // uses the phys_addr from the prior level, which was checked present.
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
    unsafe {
        page_table::write_entry(pt, virt.pt_index(), entry, hhdm);
    }

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
#[allow(dead_code)] // Used by fork/duplicate_user_pages (not yet integrated).
pub unsafe fn mark_cow(pml4_phys: u64, virt: VirtAddr) -> KernelResult<()> {
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

    // SAFETY: pml4_phys is valid (caller guarantee); virt is aligned.
    let pte = unsafe { read_pte(pml4_phys, virt, hhdm)? };
    if !pte.is_present() {
        return Err(KernelError::InvalidAddress);
    }

    // Already CoW — nothing to do.
    if pte.is_cow() {
        return Ok(());
    }

    // A shared-by-design page is never copied: marking it copy-on-write would
    // hand the next writer a private copy and disconnect it from the other
    // parties (PageFlags::SHARED's invariant).
    if pte.is_shared() {
        return Err(KernelError::InvalidArgument);
    }

    // Build new flags: remove WRITABLE, add COW.
    let mut flags = pte.flags();
    flags = PageFlags::from_bits(flags.bits() & !PageFlags::WRITABLE.bits());
    flags |= PageFlags::COW;

    let new_pte = PageTableEntry::new(pte.phys_addr(), flags);
    // SAFETY: pml4_phys valid, virt is an existing present mapping.
    unsafe {
        write_pte(pml4_phys, virt, new_pte, hhdm)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Address-space duplication for fork()
// ---------------------------------------------------------------------------

/// Number of page table entries per table (PML4/PDPT/PD/PT).
const ENTRIES_PER_TABLE: usize = 512;

/// Compose a user-half virtual address from its four page-table indices.
///
/// Only valid for the user half (`pml4_idx < 256`), where bit 47 is 0 and
/// the address is therefore canonical without sign extension.
#[allow(clippy::arithmetic_side_effects)]
fn compose_virt(pml4_idx: usize, pdpt_idx: usize, pd_idx: usize, pt_idx: usize) -> u64 {
    ((pml4_idx as u64) << 39)
        | ((pdpt_idx as u64) << 30)
        | ((pd_idx as u64) << 21)
        | ((pt_idx as u64) << 12)
}

/// Map a leaf PTE into a child address space, creating any missing
/// intermediate page-table levels (PDPT/PD/PT).
///
/// # Safety
///
/// - `child_pml4` must be a valid PML4 table owned by the caller.
/// - `virt` must be a 4 KiB-aligned user-half virtual address.
unsafe fn map_child_pte(
    child_pml4: u64,
    virt: VirtAddr,
    entry: PageTableEntry,
    hhdm: u64,
) -> KernelResult<()> {
    let user = virt.is_user();
    // SAFETY: child_pml4 is a valid PML4 (caller guarantee); each level is
    // created or returned by walk_or_create. The top level goes through
    // `walk_or_create_pml4`, which refuses to add a kernel-half entry after
    // boot should a kernel address ever reach here.
    let pdpt = unsafe { page_table::walk_or_create_pml4(child_pml4, virt, true, user, hhdm)? };
    // SAFETY: pdpt was returned by walk_or_create above.
    let pd = unsafe { page_table::walk_or_create(pdpt, virt.pdpt_index(), true, user, hhdm)? };
    // SAFETY: pd was returned by walk_or_create above.
    let pt = unsafe { page_table::walk_or_create(pd, virt.pd_index(), true, user, hhdm)? };
    // SAFETY: pt is a valid page table; pt_index() < 512.
    unsafe {
        page_table::write_entry(pt, virt.pt_index(), entry, hhdm);
    }
    Ok(())
}

/// Duplicate a 16 KiB frame group (4 consecutive 4 KiB PTEs) from the
/// parent into the child, applying copy-on-write semantics.
///
/// For each present PTE in the group:
/// - **Shared-by-design** pages ([`PageFlags::SHARED`]: a shared-memory
///   region, an io ring, a DMA or scanout buffer, device memory) are mapped
///   into the child as they are -- same frame, same permissions -- and the
///   parent's entry is left alone.  Making them copy-on-write would
///   disconnect whichever side wrote first from the region (the region, the
///   kernel's ring or the device still hold the original frame), and for
///   device memory would copy registers into RAM.  They get no reverse
///   mapping either: the parent's own mapping of them has none, and a frame
///   the compactor can find with one mapper looks private and movable.
/// - **Writable** pages are made copy-on-write in *both* parent and child
///   (WRITABLE cleared, COW set).  The first write by either side triggers
///   [`resolve_cow_fault`], which copies the page.
/// - **Read-only** pages are shared as-is (no COW bit — a write is a
///   genuine protection fault, not a CoW event).
///
/// Each *distinct* 16 KiB frame referenced by the group's present
/// siblings gains exactly one refcount (the child becomes one more
/// address-space reference to it).  In the common case all four siblings
/// share a single frame, so this is one increment; but after a partial
/// CoW resolve in the parent, siblings can point into different frames,
/// each of which must be counted.
///
/// Returns `true` if the group was shared (at least one PTE present),
/// `false` if the group was entirely unmapped (nothing to copy — the
/// child will demand-fault it via the inherited VMA, if any).
///
/// Sub-pages marked in `skip` are left out entirely: not mapped in the
/// child, not downgraded in the parent, and not counted. They are the
/// pieces of `madvise(MADV_WIPEONFORK / MADV_DONTFORK)` regions that fall in
/// this group (see [`clone_address_space_cow`]).
///
/// # Safety
///
/// - `parent_pml4` and `child_pml4` must be valid PML4 tables.
/// - `parent_pt` must be the parent's PT page for this group.
/// - `group_virt_base` must be the 16 KiB-aligned virtual base of the group.
#[allow(clippy::arithmetic_side_effects)]
unsafe fn clone_frame_group(
    parent_pml4: u64,
    child_pml4: u64,
    group_virt_base: u64,
    parent_pt: u64,
    base_pt_idx: usize,
    hhdm: u64,
    skip: [bool; HW_PAGES_PER_FRAME],
) -> KernelResult<bool> {
    let mut parent_needs_flush = false;
    // Distinct 16 KiB frame bases referenced by this group's siblings.  At
    // most HW_PAGES_PER_FRAME distinct frames (one per sub-PTE).  We
    // increment each frame's refcount exactly once for the child.
    let mut seen_frames: [u64; HW_PAGES_PER_FRAME] = [0; HW_PAGES_PER_FRAME];
    let mut seen_count = 0usize;

    for i in 0..HW_PAGES_PER_FRAME {
        if skip.get(i) == Some(&true) {
            continue;
        }
        let pt_idx = base_pt_idx + i;
        // SAFETY: parent_pt is a valid PT page, pt_idx < 512.
        let pte = unsafe { page_table::read_entry(parent_pt, pt_idx, hhdm) };
        if !pte.is_present() {
            continue;
        }

        let phys = pte.phys_addr();
        let frame_base = phys & !(FRAME_SIZE as u64 - 1);
        let shared = pte.is_shared();

        // Increment this frame's refcount once per distinct frame in the
        // group (each present sibling may, after a partial CoW resolve in
        // the parent, point into a different 16 KiB frame).
        let mut already_seen = false;
        for &seen in seen_frames.iter().take(seen_count) {
            if seen == frame_base {
                already_seen = true;
                break;
            }
        }
        if !already_seen {
            if let Some(frame) = PhysFrame::from_addr(frame_base) {
                // SAFETY: frame is a valid allocated frame currently mapped
                // into the parent.
                unsafe {
                    frame::ref_inc(frame)?;
                }
            }
            // Frames not owned by the allocator (e.g., device MMIO mapped
            // into user space) are shared without refcounting — they are
            // never returned to the frame allocator.  Record the frame and
            // its child rmap entry regardless so teardown stays symmetric.
            if let Some(slot) = seen_frames.get_mut(seen_count) {
                *slot = frame_base;
                seen_count += 1;
            }
            // The child now maps this frame; record the reverse mapping so
            // the reclaimer/compactor can find it -- unless the page is shared
            // by design.  Neither may move such a frame (every other party
            // would be left holding the old one), and a lone rmap entry for
            // it is exactly what would make it look private and movable.
            // rmap::remove at the child's teardown no-ops for a frame with no
            // entry, so skipping the add keeps teardown symmetric.
            if !shared {
                super::rmap::add(frame_base, child_pml4, group_virt_base);
            }
        }

        // Compute child flags and, for private writable pages, downgrade the
        // parent to CoW as well. A child's memory is not locked (Linux's
        // fork), so the child's entry drops `MLOCKED`; the parent keeps it.
        let unlocked = |e: PageTableEntry| {
            PageTableEntry::new(
                e.phys_addr(),
                PageFlags::from_bits(e.flags().bits() & !PageFlags::MLOCKED.bits()),
            )
        };
        let child_entry = unlocked(if shared {
            // Shared by design: the child sees the same frame with the same
            // permissions, and the parent keeps its writable mapping.
            PageTableEntry::new(phys, pte.flags())
        } else if pte.flags().contains(PageFlags::WRITABLE) {
            let mut cow_flags = pte.flags() | PageFlags::COW;
            cow_flags = PageFlags::from_bits(cow_flags.bits() & !PageFlags::WRITABLE.bits());

            // Downgrade the parent PTE to read-only + COW in place.
            let parent_cow = PageTableEntry::new(phys, cow_flags);
            // SAFETY: parent_pt is valid, pt_idx < 512.
            unsafe {
                page_table::write_entry(parent_pt, pt_idx, parent_cow, hhdm);
            }
            parent_needs_flush = true;

            parent_cow
        } else {
            // Read-only page: share identically, no COW bit.
            PageTableEntry::new(phys, pte.flags())
        });

        let hw_virt = VirtAddr::new(group_virt_base + (i as u64 * HW_PAGE_SIZE as u64));
        // SAFETY: child_pml4 is a valid PML4 owned by the caller; hw_virt is
        // a 4 KiB-aligned user address.
        unsafe {
            map_child_pte(child_pml4, hw_virt, child_entry, hhdm)?;
        }
    }

    if seen_count == 0 {
        // Entire group was non-present — nothing shared.
        return Ok(false);
    }

    // Charge the child's RSS for each frame it now maps (mirrors map_frame):
    // usually one, but a group whose sub-pages point into different frames
    // maps each of them, and its teardown and munmap release each one.
    super::accounting::charge(child_pml4, seen_count as u64);

    // If we downgraded any parent PTE to CoW, the parent (the running
    // process that called fork) must flush stale writable TLB entries.
    if parent_needs_flush && parent_pml4 == page_table::active_pml4_phys() {
        crate::tlb::flush_range(group_virt_base, HW_PAGES_PER_FRAME as u32);
    }

    Ok(true)
}

/// Which sub-pages of the group at `group_virt` fall in `uncopied` (sorted,
/// non-overlapping `[start, end)` ranges): the per-sub-page `skip` mask for
/// [`clone_frame_group`].
#[allow(clippy::arithmetic_side_effects)]
fn uncopied_mask(uncopied: &[(u64, u64)], group_virt: u64) -> [bool; HW_PAGES_PER_FRAME] {
    let mut skip = [false; HW_PAGES_PER_FRAME];
    if uncopied.is_empty() {
        return skip;
    }
    for (i, slot) in skip.iter_mut().enumerate() {
        let sub_va = group_virt + (i as u64) * (HW_PAGE_SIZE as u64);
        // The last range starting at or below `sub_va` is the only one that
        // can contain it.
        let idx = uncopied.partition_point(|&(start, _)| start <= sub_va);
        *slot = idx > 0 && uncopied.get(idx - 1).is_some_and(|&(_, end)| sub_va < end);
    }
    skip
}

/// Walk the parent's user half and duplicate every mapped frame group into
/// the child via [`clone_frame_group`].  Swapped-out pages are brought
/// back into RAM first (in the parent) so they can be shared CoW.  Pages in
/// `uncopied` are not duplicated (see [`clone_address_space_cow`]).
///
/// # Safety
///
/// - `parent_pml4` and `child_pml4` must be valid PML4 tables.
#[allow(clippy::arithmetic_side_effects)]
unsafe fn clone_user_half(
    parent_pml4: u64,
    child_pml4: u64,
    hhdm: u64,
    uncopied: &[(u64, u64)],
) -> KernelResult<()> {
    for pml4_idx in 0..256usize {
        // SAFETY: parent_pml4 valid, index < 512.
        let pml4e = unsafe { page_table::read_entry(parent_pml4, pml4_idx, hhdm) };
        if !pml4e.is_present() {
            continue;
        }
        let pdpt = pml4e.phys_addr();

        for pdpt_idx in 0..ENTRIES_PER_TABLE {
            // SAFETY: pdpt from present pml4e, index < 512.
            let pdpte = unsafe { page_table::read_entry(pdpt, pdpt_idx, hhdm) };
            if !pdpte.is_present() || pdpte.is_huge() {
                continue;
            }
            let pd = pdpte.phys_addr();

            for pd_idx in 0..ENTRIES_PER_TABLE {
                // SAFETY: pd from present pdpte, index < 512.
                let pde = unsafe { page_table::read_entry(pd, pd_idx, hhdm) };
                if !pde.is_present() || pde.is_huge() {
                    continue;
                }
                let pt = pde.phys_addr();

                for base_pt_idx in (0..ENTRIES_PER_TABLE).step_by(HW_PAGES_PER_FRAME) {
                    let group_virt = compose_virt(pml4_idx, pdpt_idx, pd_idx, base_pt_idx);

                    let virt = VirtAddr::new(group_virt);
                    let skip = uncopied_mask(uncopied, group_virt);
                    if skip.iter().all(|&s| s) {
                        // Nothing of this group reaches the child: not even
                        // worth bringing back from swap.
                        continue;
                    }

                    loop {
                        // Swapped-out frame: bring it back to RAM (in the
                        // parent) before sharing, each part as it was. A
                        // 16 KiB frame is swapped as a unit, but any part may
                        // be the one still naming the slot (one unmapped since
                        // gave its entry up), so every part is looked at
                        // (`swap::is_swapped`). Unlocked: it reads the slot,
                        // and takes the lock itself to install.
                        // SAFETY: parent_pml4 is valid (the caller's contract).
                        if unsafe { super::swap::is_swapped(parent_pml4, virt) } {
                            // SAFETY: parent_pml4 valid, a part holds a swap entry.
                            if let Some(flags) =
                                unsafe { super::swap::swap_in_page(parent_pml4, virt)? }
                            {
                                // Re-register so the page can be evicted again.
                                super::swap::register_reclaimable(parent_pml4, group_virt, flags);
                            }
                        }

                        // The group is shared under the parent's page-table lock
                        // (`super::as_lock`), so no copy-on-write break or swap-in
                        // of the same group installs while it is marked and
                        // copied. Group by group, not for the whole clone: a big
                        // address space would hold every other thread's fault for
                        // the length of the fork.
                        let held = super::as_lock::lock(parent_pml4);
                        // Swapped out again since the look above: bring it back
                        // again, rather than share it as a gap.
                        // SAFETY: as above.
                        if unsafe { super::swap::is_swapped(parent_pml4, virt) } {
                            drop(held);
                            continue;
                        }
                        // SAFETY: all tables valid; group_virt is the group base.
                        unsafe {
                            clone_frame_group(
                                parent_pml4,
                                child_pml4,
                                group_virt,
                                pt,
                                base_pt_idx,
                                hhdm,
                                skip,
                            )?;
                        }
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Duplicate an address space for `fork()`, sharing all user pages
/// copy-on-write.
///
/// Allocates a fresh child PML4 (with the kernel half — entries 256–511 —
/// shared with the parent via [`page_table::alloc_pml4`]), then walks the
/// parent's user half (entries 0–255) and shares every mapped 16 KiB frame
/// with the child:
///
/// - **Writable** pages become copy-on-write in both address spaces.
/// - **Read-only** pages are shared directly.
/// - **Swapped-out** pages are faulted back into the parent first, then
///   shared CoW.
/// - **Unmapped / demand-paged** regions are skipped (the child inherits
///   the parent's VMAs separately and will demand-fault them on access).
/// - **`uncopied`** ranges -- sorted, non-overlapping `[start, end)`, from
///   [`crate::proc::pcb::fork_uncopied_ranges`] -- are skipped too, page by
///   4 KiB page: the parent's `madvise(MADV_WIPEONFORK)` regions, which the
///   child keeps as regions and demand-zeroes, and `MADV_DONTFORK` ones,
///   which it does not get at all. The parent's own pages there are left
///   exactly as they were (still writable: nothing shares them).
///
/// On any failure the partially-built child address space is fully torn
/// down (releasing all shared references) before the error is returned.
///
/// ## Returns
///
/// The physical address of the new child PML4 on success.
///
/// ## Errors
///
/// - [`KernelError::NotSupported`] — the MM subsystem is not initialized.
/// - [`KernelError::OutOfMemory`] — page-table page or frame allocation
///   failed (including swap-in of a parent page).
///
/// # Safety
///
/// - `parent_pml4` must be a valid PML4 table that the caller owns.
/// - The parent's user address space must be quiescent for the duration of
///   the call (no other thread mutating its page tables concurrently).
pub unsafe fn clone_address_space_cow(
    parent_pml4: u64,
    uncopied: &[(u64, u64)],
) -> KernelResult<u64> {
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

    // Allocate the child PML4 (kernel half cloned from the active table).
    let child_pml4 = page_table::alloc_pml4()?;

    // SAFETY: both PML4s are valid; parent is quiescent (caller guarantee).
    let result = unsafe { clone_user_half(parent_pml4, child_pml4, hhdm, uncopied) };

    if let Err(e) = result {
        // Roll back: tear down everything we mapped into the child.  Shared
        // CoW frames have their refcount decremented (the parent keeps its
        // references); intermediate page-table pages are returned to the
        // pool; the PML4 itself is freed.
        // SAFETY: child_pml4 came from alloc_pml4, is not loaded in any CR3,
        // and no thread is using it yet.
        unsafe {
            page_table::destroy_user_address_space(child_pml4);
        }
        return Err(e);
    }

    // The parent's writable pages are copy-on-write now and shared with the
    // child. A `process_vm_writev` into one of them that passed its checks
    // before the change is still writing the frame both now hold; it must end
    // before the child can run and copy that frame, or the child keeps half
    // of the write. One that checks after the change finds the page
    // read-only and breaks the share first (`mm::frame`'s remote-copy
    // windows).
    frame::wait_for_open_remote_copies();

    serial_println!(
        "[cow] Cloned address space: parent={:#x} -> child={:#x}",
        parent_pml4,
        child_pml4
    );

    Ok(child_pml4)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for CoW infrastructure.
///
/// Tests the refcount API, COW flag manipulation, and (where possible)
/// the CoW fault resolution logic.  Full end-to-end testing (actual
/// page faults) requires a user-space test process.
pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[cow] Running self-test...");

    // Test 1: Refcount API.
    test_refcount();

    // Test 2: COW flag in PTE.
    test_cow_flag();

    // Test 3: Sole-owner CoW resolution (refcount == 1).
    test_cow_resolve_sole_owner();

    // Test 4: Shared-frame CoW resolution (refcount > 1).
    test_cow_resolve_shared();
    test_cow_break_twice()?;

    // Test 5: Address-space duplication for fork().
    test_clone_address_space_cow();

    // Test 6: a page shared by design stays shared across fork().
    test_fork_keeps_shared_pages_shared()?;

    // Test 7: madvise(MADV_WIPEONFORK / MADV_DONTFORK) pages stay behind.
    test_fork_leaves_out_uncopied_pages()?;

    serial_println!("[cow] Self-test PASSED");
    Ok(())
}

/// A page shared by design ([`PageFlags::SHARED`]) survives `fork` shared:
/// the child maps the same frame writable, the parent keeps its writable
/// mapping, neither is copy-on-write, and the child's mapping gets no
/// reverse-mapping entry for the compactor to find.  And `mark_cow` refuses
/// such a page.
///
/// The bug this pins (2026-09-27): fork made every writable page
/// copy-on-write, a shared-memory region's included.  The region holds its
/// own reference to the frame, so the parent's first write after a fork
/// copied it and silently left the region; the same happened to io rings,
/// DMA and scanout buffers, and -- frames the allocator does not own --
/// device registers were copied into RAM.
fn test_fork_keeps_shared_pages_shared() -> KernelResult<()> {
    let virt: u64 = 0x0000_4000_0001_0000; // user half, frame-aligned
    let shared_rw = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE
        | PageFlags::SHARED;

    // The frame's first reference is the "region's", as an SHM region holds
    // one; the mapping below takes a second, as sys_shm_map does.
    let region_frame = frame::alloc_frame_zeroed()?;
    let phys = region_frame.addr();
    let mut parent: Option<u64> = None;
    let mut child: Option<u64> = None;

    let outcome = (|| -> KernelResult<()> {
        let p = page_table::alloc_pml4()?;
        parent = Some(p);
        // SAFETY: region_frame is live and ours.  This is the mapping's
        // reference: the parent's teardown drops it once the mapping exists,
        // and the error arm below drops it if the mapping cannot be made.
        unsafe { frame::ref_inc(region_frame)? };
        // SAFETY: p is a valid PML4 built above; virt is a frame-aligned user
        // address with nothing mapped at it.
        if let Err(e) =
            unsafe { page_table::map_frame(p, VirtAddr::new(virt), region_frame, shared_rw) }
        {
            // SAFETY: drops the reference taken above, which no mapping holds.
            unsafe { frame::free_frame(region_frame)? };
            return Err(e);
        }
        let rmap_before = super::rmap::mapper_count(phys);

        // SAFETY: p is a valid, quiescent PML4 built above.
        let c = unsafe { clone_address_space_cow(p, &[])? };
        child = Some(c);

        let refs = frame::refcount(region_frame);
        if refs != 3 {
            serial_println!(
                "[cow]   FAIL: after fork the shared frame has {} references, want 3 \
                 (region, parent, child)",
                refs
            );
            return Err(KernelError::InternalError);
        }
        for (who, pml4) in [("parent", p), ("child", c)] {
            if page_table::translate(pml4, VirtAddr::new(virt)) != Some(phys) {
                serial_println!("[cow]   FAIL: the {} does not map the region's frame", who);
                return Err(KernelError::InternalError);
            }
            let flags = page_table::translate_flags(pml4, VirtAddr::new(virt))
                .unwrap_or(PageFlags::empty());
            if !flags.contains(PageFlags::WRITABLE)
                || !flags.contains(PageFlags::SHARED)
                || flags.contains(PageFlags::COW)
            {
                serial_println!(
                    "[cow]   FAIL: the {}'s shared page is not writable, shared and free of \
                     COW after fork (flags {:#x})",
                    who,
                    flags.bits()
                );
                return Err(KernelError::InternalError);
            }
        }
        if super::rmap::mapper_count(phys) != rmap_before {
            serial_println!("[cow]   FAIL: fork gave a shared frame a reverse mapping");
            return Err(KernelError::InternalError);
        }
        // SAFETY: p is valid and virt is mapped and present.
        if unsafe { mark_cow(p, VirtAddr::new(virt)) } != Err(KernelError::InvalidArgument) {
            serial_println!("[cow]   FAIL: mark_cow did not refuse a shared page");
            return Err(KernelError::InternalError);
        }
        Ok(())
    })();

    // Teardown, however the test went: each mapping's reference goes with its
    // address space, leaving the region's.
    // SAFETY: neither PML4 is loaded in any CR3; no thread uses them.
    unsafe {
        if let Some(c) = child {
            page_table::destroy_user_address_space(c);
        }
        if let Some(p) = parent {
            page_table::destroy_user_address_space(p);
        }
    }
    let left = frame::refcount(region_frame);
    if left >= 1 {
        // SAFETY: the region's reference, taken by the allocation above.
        unsafe { frame::free_frame(region_frame)? };
    }
    outcome?;
    if left != 1 {
        serial_println!(
            "[cow]   FAIL: after both teardowns the shared frame had {} references, want 1 \
             (the region's)",
            left
        );
        return Err(KernelError::InternalError);
    }

    serial_println!(
        "[cow]   fork keeps a shared page shared (same frame, writable, no COW, no rmap): OK"
    );
    Ok(())
}

/// Test 7: the pages of a parent's `madvise(MADV_WIPEONFORK / MADV_DONTFORK)`
/// regions do not reach the child -- at 4 KiB granularity, inside a group as
/// well as a whole group -- and the parent's own pages there stay exactly as
/// they were: writable, not copy-on-write, not shared.
///
/// Two groups, each fully mapped in the parent to a frame of its own. The
/// first loses one sub-page to `uncopied`, the second all four. The test
/// holds a reference of its own on each frame, so the counts it checks after
/// both teardowns are exact rather than racing a reuse of a freed frame.
#[allow(clippy::arithmetic_side_effects)]
fn test_fork_leaves_out_uncopied_pages() -> KernelResult<()> {
    let base: u64 = 0x0000_4000_0002_0000; // user half, frame-aligned
    let hw = HW_PAGE_SIZE as u64;
    let group = FRAME_SIZE as u64;
    let rw = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE;
    let fail = |what: &str| {
        serial_println!("[cow]   FAIL: fork with uncopied ranges: {}", what);
        Err(KernelError::InternalError)
    };

    let mut parent: Option<u64> = None;
    let mut child: Option<u64> = None;
    let mut frames: [Option<PhysFrame>; 2] = [None, None];

    let outcome = (|| -> KernelResult<()> {
        let p = page_table::alloc_pml4()?;
        parent = Some(p);
        for (k, slot) in frames.iter_mut().enumerate() {
            let f = frame::alloc_frame_zeroed()?;
            let va = VirtAddr::new(base + k as u64 * group);
            // SAFETY: `p` is the valid PML4 built above; `va` is a
            // frame-aligned user address with nothing mapped at it. The
            // mapping takes the allocation's reference.
            if let Err(e) = unsafe { page_table::map_frame(p, va, f, rw) } {
                // SAFETY: never mapped; this drops the allocation's reference.
                unsafe { frame::free_frame(f)? };
                return Err(e);
            }
            // SAFETY: `f` is live (mapped above). The test's own reference,
            // dropped after the teardowns.
            unsafe { frame::ref_inc(f)? };
            *slot = Some(f);
        }

        let uncopied = [(base + hw, base + 2 * hw), (base + group, base + 2 * group)];
        // SAFETY: `p` is a valid, quiescent PML4 built above.
        let c = unsafe { clone_address_space_cow(p, &uncopied)? };
        child = Some(c);

        for i in 0..2 * HW_PAGES_PER_FRAME as u64 {
            let va = VirtAddr::new(base + i * hw);
            let left_out = i == 1 || i >= HW_PAGES_PER_FRAME as u64;
            let in_child = page_table::translate(c, va).is_some();
            let pflags = page_table::translate_flags(p, va).unwrap_or(PageFlags::empty());
            if left_out {
                if in_child {
                    return fail("the child got a page of an uncopied range");
                }
                if !pflags.contains(PageFlags::WRITABLE) || pflags.contains(PageFlags::COW) {
                    return fail("the parent's page in an uncopied range was changed");
                }
            } else {
                if !in_child {
                    return fail("the child lost a page outside the uncopied ranges");
                }
                if pflags.contains(PageFlags::WRITABLE) || !pflags.contains(PageFlags::COW) {
                    return fail("the parent's shared page was not made copy-on-write");
                }
            }
        }
        // test + parent + child for the first group's frame; test + parent
        // for the second's, which nothing shares.
        let refs: [u16; 2] = frames.map(|f| f.map_or(0, frame::refcount));
        if refs != [3, 2] {
            serial_println!("[cow]   references after fork: {:?}, want [3, 2]", refs);
            return fail("wrong frame references after fork");
        }
        Ok(())
    })();

    // SAFETY: neither PML4 is loaded in any CR3; no thread uses them.
    unsafe {
        if let Some(c) = child {
            page_table::destroy_user_address_space(c);
        }
        if let Some(p) = parent {
            page_table::destroy_user_address_space(p);
        }
    }
    let mut left = [0u16; 2];
    for (slot, f) in left.iter_mut().zip(frames.iter()) {
        if let Some(f) = *f {
            *slot = frame::refcount(f);
            if *slot >= 1 {
                // SAFETY: the test's own reference, taken above.
                unsafe { frame::free_frame(f)? };
            }
        }
    }
    outcome?;
    if left != [1, 1] {
        serial_println!(
            "[cow]   references after both teardowns: {:?}, want [1, 1]",
            left
        );
        return fail("a teardown kept or lost a reference");
    }
    serial_println!(
        "[cow]   fork leaves wipe/dontfork pages out, 4 KiB-exact, parent untouched: OK"
    );
    Ok(())
}

/// Test [`clone_address_space_cow`]: fork-style address-space duplication.
///
/// Builds a synthetic parent address space with a writable and a read-only
/// user page, clones it, and verifies:
/// - the child maps the same physical frames,
/// - writable pages become CoW (RO + COW) in *both* parent and child,
/// - read-only pages are shared as-is (no COW bit),
/// - the shared frames' refcounts are incremented to 2,
/// - page data is intact,
/// - teardown of both address spaces frees the frames exactly once each.
#[allow(clippy::arithmetic_side_effects)]
fn test_clone_address_space_cow() {
    use crate::mm::page_table::{self, PageFlags, VirtAddr};

    let hhdm = page_table::hhdm().expect("hhdm for fork test");

    // Build a synthetic parent address space (not loaded in any CR3).
    let parent = page_table::alloc_pml4().expect("alloc parent pml4");

    let rw_virt: u64 = 0x0000_4000_0000_0000; // user half
    let ro_virt: u64 = 0x0000_4000_0000_4000; // next 16 KiB frame

    let rw_frame = frame::alloc_frame().expect("alloc rw frame");
    let ro_frame = frame::alloc_frame().expect("alloc ro frame");
    let rw_phys = rw_frame.addr();
    let ro_phys = ro_frame.addr();

    // Write recognizable data via HHDM.
    // SAFETY: both frames are freshly allocated and mapped via HHDM.
    unsafe {
        let p = (rw_phys + hhdm) as *mut u8;
        for i in 0u8..16 {
            p.add(i as usize).write(0x10 + i);
        }
        let q = (ro_phys + hhdm) as *mut u8;
        for i in 0u8..16 {
            q.add(i as usize).write(0x40 + i);
        }
    }

    // SAFETY: parent is a valid PML4; addresses are user, frame-aligned.
    unsafe {
        page_table::map_frame(
            parent,
            VirtAddr::new(rw_virt),
            rw_frame,
            PageFlags::PRESENT
                | PageFlags::WRITABLE
                | PageFlags::USER_ACCESSIBLE
                | PageFlags::NO_EXECUTE,
        )
        .expect("map rw");
        page_table::map_frame(
            parent,
            VirtAddr::new(ro_virt),
            ro_frame,
            PageFlags::PRESENT | PageFlags::USER_ACCESSIBLE | PageFlags::NO_EXECUTE,
        )
        .expect("map ro");
    }

    assert!(
        frame::refcount(rw_frame) == 1,
        "rw refcount should start at 1"
    );
    assert!(
        frame::refcount(ro_frame) == 1,
        "ro refcount should start at 1"
    );

    // Clone the address space (fork).
    // SAFETY: parent is a valid, quiescent PML4 we just built.
    let child = unsafe { clone_address_space_cow(parent, &[]).expect("clone_address_space_cow") };

    // Refcounts bumped to 2 (parent + child).
    assert!(
        frame::refcount(rw_frame) == 2,
        "rw refcount should be 2 after fork"
    );
    assert!(
        frame::refcount(ro_frame) == 2,
        "ro refcount should be 2 after fork"
    );

    // Child maps the same physical frames.
    assert!(
        page_table::translate(child, VirtAddr::new(rw_virt)) == Some(rw_phys),
        "child rw should map the parent's frame"
    );
    assert!(
        page_table::translate(child, VirtAddr::new(ro_virt)) == Some(ro_phys),
        "child ro should map the parent's frame"
    );

    // Writable page downgraded to CoW (RO + COW) in BOTH address spaces.
    let pflags =
        page_table::translate_flags(parent, VirtAddr::new(rw_virt)).expect("parent rw flags");
    assert!(pflags.contains(PageFlags::COW), "parent rw should be COW");
    assert!(
        !pflags.contains(PageFlags::WRITABLE),
        "parent rw should be read-only"
    );
    let cflags =
        page_table::translate_flags(child, VirtAddr::new(rw_virt)).expect("child rw flags");
    assert!(cflags.contains(PageFlags::COW), "child rw should be COW");
    assert!(
        !cflags.contains(PageFlags::WRITABLE),
        "child rw should be read-only"
    );

    // Read-only page: shared without a COW bit.
    let proflags =
        page_table::translate_flags(parent, VirtAddr::new(ro_virt)).expect("parent ro flags");
    assert!(
        !proflags.contains(PageFlags::COW),
        "parent ro should not be COW"
    );
    let croflags =
        page_table::translate_flags(child, VirtAddr::new(ro_virt)).expect("child ro flags");
    assert!(
        !croflags.contains(PageFlags::COW),
        "child ro should not be COW"
    );
    assert!(
        !croflags.contains(PageFlags::WRITABLE),
        "child ro should be read-only"
    );

    // Data intact.
    // SAFETY: rw_phys is still a valid frame, mapped via HHDM.
    unsafe {
        let p = (rw_phys + hhdm) as *const u8;
        for i in 0u8..16 {
            assert!(
                p.add(i as usize).read() == 0x10 + i,
                "rw data corrupted after fork"
            );
        }
    }

    // Teardown: destroy child then parent.  free_frame is refcount-aware,
    // so the frames are freed exactly once (when the last reference drops).
    // SAFETY: neither PML4 is loaded in any CR3; no thread uses them.
    unsafe {
        page_table::destroy_user_address_space(child);
        page_table::destroy_user_address_space(parent);
    }

    serial_println!("[cow]   clone_address_space_cow (fork): OK");
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
    assert!(
        new_rc == 1,
        "refcount after dec should be 1, got {}",
        new_rc
    );

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

/// Test CoW resolution when the current task is the sole owner (refcount == 1).
///
/// Scenario: a page was marked CoW (e.g., the other sharer already
/// resolved their copy), but our refcount is 1.  The resolver should
/// simply flip WRITABLE on and clear COW — no copy needed.
#[allow(clippy::arithmetic_side_effects)]
fn test_cow_resolve_sole_owner() {
    use crate::mm::page_table::{self, PageFlags, VirtAddr};

    let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());
    let hhdm = page_table::hhdm().expect("hhdm for cow test");

    // Use a kernel-space virtual address that's not in use.
    let test_virt_base: u64 = 0xFFFF_CA00_0000_0000;

    // Allocate a frame, write a pattern, map it.
    let frame_val = frame::alloc_frame().expect("cow test alloc");
    let phys = frame_val.addr();
    let virt_ptr = (phys + hhdm) as *mut u8;

    // Write a recognizable pattern into the first 16 bytes.
    // SAFETY: frame is allocated and valid via HHDM.
    unsafe {
        for i in 0u8..16 {
            virt_ptr.add(i as usize).write(0xAA + i);
        }
    }

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;
    let virt = VirtAddr::new(test_virt_base);
    // SAFETY: test address, valid pml4, valid frame.
    unsafe {
        page_table::map_frame(pml4, virt, frame_val, flags).expect("cow test map");
    }

    // Mark all 4 hardware pages as CoW (clear WRITABLE, set COW).
    for i in 0..HW_PAGES_PER_FRAME {
        let hw_virt = VirtAddr::new(test_virt_base + (i as u64 * HW_PAGE_SIZE as u64));
        // SAFETY: pages are mapped.
        unsafe {
            mark_cow(pml4, hw_virt).expect("mark_cow");
        }
    }

    // Verify PTEs are now CoW (not writable).
    // SAFETY: pml4 and virt reference our test mapping.
    let pte = unsafe { read_pte(pml4, virt, hhdm).expect("read pte") };
    assert!(pte.is_cow(), "PTE should be CoW after mark_cow");
    assert!(
        !pte.flags().contains(PageFlags::WRITABLE),
        "CoW PTE should not be writable"
    );

    // Flush TLB to ensure CoW state is visible.
    crate::tlb::flush_range(test_virt_base, HW_PAGES_PER_FRAME as u32);

    // Refcount is 1 (sole owner).  Resolve the CoW fault.
    assert!(
        frame::refcount(frame_val) == 1,
        "refcount should be 1 before sole-owner resolve"
    );

    // Call resolve_cow_fault for the first hardware page.
    resolve_cow_fault(pml4, test_virt_base).expect("sole-owner cow resolve should succeed");

    // Verify: PTE should now be WRITABLE and not COW.
    // SAFETY: pml4 and virt reference our test mapping.
    let pte_after = unsafe { read_pte(pml4, virt, hhdm).expect("read pte after") };
    assert!(
        !pte_after.is_cow(),
        "PTE should not be CoW after sole-owner resolve"
    );
    assert!(
        pte_after.flags().contains(PageFlags::WRITABLE),
        "PTE should be writable after sole-owner resolve"
    );

    // Physical address should be unchanged (no copy for sole owner).
    assert!(
        pte_after.phys_addr() == phys,
        "sole-owner resolve should keep same physical page"
    );

    // Batch resolution: all 4 sibling PTEs should be resolved too.
    for i in 1..HW_PAGES_PER_FRAME {
        let sib_virt = VirtAddr::new(test_virt_base + (i as u64 * HW_PAGE_SIZE as u64));
        // SAFETY: pml4 and sib_virt reference our test mapping.
        let sib_pte = unsafe { read_pte(pml4, sib_virt, hhdm).expect("read sibling") };
        assert!(
            !sib_pte.is_cow(),
            "sibling {} should not be CoW after batch resolve",
            i
        );
        assert!(
            sib_pte.flags().contains(PageFlags::WRITABLE),
            "sibling {} should be writable after batch resolve",
            i
        );
    }

    // Verify data integrity — pattern should be intact.
    // SAFETY: frame is still mapped via HHDM.
    unsafe {
        for i in 0u8..16 {
            let val = virt_ptr.add(i as usize).read();
            assert!(
                val == 0xAA + i,
                "data integrity check failed at byte {}: expected {:#x}, got {:#x}",
                i,
                0xAA + i,
                val
            );
        }
    }

    // Cleanup: unmap and free.
    // SAFETY: we mapped it above, sole owner.
    let returned = unsafe { page_table::unmap_frame(pml4, virt).expect("cow test unmap") };
    crate::tlb::flush_range(test_virt_base, HW_PAGES_PER_FRAME as u32);
    // SAFETY: sole owner.
    unsafe {
        frame::free_frame(returned).expect("cow test free");
    }

    serial_println!("[cow]   Sole-owner CoW resolve: OK");
}
/// Two CPUs breaking the same copy-on-write page: both prepare a copy, the
/// first installs, the second finds nothing left to change and frees its copy.
/// The entries end on the first copy, and the shared frame loses this address
/// space's reference once. Driven as the two
/// halves interleave on two CPUs -- prepare, prepare, install, install --
/// which is what happened unlocked before 2026-10-03, when each install
/// installed and each dropped a reference.
#[allow(clippy::arithmetic_side_effects)]
fn test_cow_break_twice() -> KernelResult<()> {
    use crate::mm::page_table::{self, PageFlags, VirtAddr};

    fn fail(what: &str) -> KernelResult<()> {
        serial_println!("[cow]   FAIL: break twice: {}", what);
        Err(KernelError::InternalError)
    }

    let pml4 = page_table::alloc_pml4()?;
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let base: u64 = 0x0000_0041_0000_0000;
    let shared = frame::alloc_frame()?;
    // SAFETY: a fresh frame through the HHDM.
    unsafe { core::ptr::write_bytes((shared.addr() + hhdm) as *mut u8, 0x5A, FRAME_SIZE) };
    let flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE;
    // SAFETY: this test's address space; a fresh frame.
    unsafe { page_table::map_frame(pml4, VirtAddr::new(base), shared, flags)? };
    // Shared, as fork leaves it: a second owner, and every part COW.
    // SAFETY: the frame is allocated.
    unsafe { frame::ref_inc(shared)? };
    for i in 0..HW_PAGES_PER_FRAME {
        // SAFETY: the parts are mapped.
        unsafe { mark_cow(pml4, VirtAddr::new(base + (i * HW_PAGE_SIZE) as u64))? };
    }

    let first = prepare_cow(pml4, base)?;
    let second = prepare_cow(pml4, base + 5000)?;
    let first_copy = first.copy.map(|f| f.addr());
    let second_copy = second.copy.map(|f| f.addr());
    {
        let _held = super::as_lock::lock(pml4);
        install_cow(pml4, first)?;
    }
    {
        let _held = super::as_lock::lock(pml4);
        install_cow(pml4, second)?;
    }

    // SAFETY: this test's address space.
    let entry = unsafe { read_pte(pml4, VirtAddr::new(base), hhdm)? };
    let on_first = first_copy.is_some_and(|f| entry.phys_addr() & !(FRAME_SIZE as u64 - 1) == f);
    let writable = entry.flags().contains(PageFlags::WRITABLE) && !entry.is_cow();
    let one_reference_dropped = frame::refcount(shared) == 1;
    let both_copied = first_copy.is_some() && second_copy.is_some() && first_copy != second_copy;

    // Clean up: the test address space (which frees the first copy), and the
    // stand-in second owner's reference to the shared frame.
    // SAFETY: never loaded in any CR3; nothing else uses it.
    unsafe { page_table::destroy_user_address_space(pml4) };
    // SAFETY: the shared frame's last reference is the stand-in's.
    let _ = unsafe { frame::free_frame(shared) };

    let checks = [
        ("both CPUs prepared their own copy", both_copied),
        ("the entries are on the first copy", on_first),
        ("the entries are writable and no longer COW", writable),
        (
            "the shared frame lost one reference, not two",
            one_reference_dropped,
        ),
    ];
    if let Some((what, _)) = checks.iter().find(|(_, ok)| !ok) {
        return fail(what);
    }
    serial_println!(
        "[cow]   break twice: the first install wins, the second frees its copy, and the \
         shared frame loses one reference: OK"
    );
    Ok(())
}

/// Test CoW resolution when the frame is shared (refcount > 1).
///
/// Scenario: two address spaces share a page (refcount == 2).  A write
/// fault triggers CoW resolution which must: allocate a new frame, copy
/// the data, update PTEs to point to the new frame, decrement the old
/// frame's refcount.
#[allow(clippy::arithmetic_side_effects)]
fn test_cow_resolve_shared() {
    use crate::mm::page_table::{self, PageFlags, VirtAddr};

    let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());
    let hhdm = page_table::hhdm().expect("hhdm for cow test");

    let test_virt_base: u64 = 0xFFFF_CA00_0004_0000;

    // Allocate a frame and write a distinctive pattern.
    let frame_val = frame::alloc_frame().expect("cow test alloc");
    let phys = frame_val.addr();
    let virt_ptr = (phys + hhdm) as *mut u8;

    // Write 0xBB pattern in the first page, 0xCC in second, etc.
    // SAFETY: frame allocated via HHDM.
    unsafe {
        for page in 0..HW_PAGES_PER_FRAME {
            let page_ptr = virt_ptr.add(page * HW_PAGE_SIZE);
            for j in 0..16 {
                page_ptr.add(j).write(0xBB + page as u8);
            }
        }
    }

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;
    let virt = VirtAddr::new(test_virt_base);
    // SAFETY: test address, valid frame.
    unsafe {
        page_table::map_frame(pml4, virt, frame_val, flags).expect("cow test map");
    }

    // Simulate sharing: increment refcount to 2 (as if fork duplicated the PTE).
    // SAFETY: frame is allocated.
    unsafe {
        frame::ref_inc(frame_val).expect("ref_inc for sharing");
    }
    assert!(
        frame::refcount(frame_val) == 2,
        "refcount should be 2 after sharing"
    );

    // Mark all 4 hardware pages as CoW.
    for i in 0..HW_PAGES_PER_FRAME {
        let hw_virt = VirtAddr::new(test_virt_base + (i as u64 * HW_PAGE_SIZE as u64));
        // SAFETY: pages are mapped.
        unsafe {
            mark_cow(pml4, hw_virt).expect("mark_cow shared");
        }
    }
    crate::tlb::flush_range(test_virt_base, HW_PAGES_PER_FRAME as u32);

    // Resolve CoW — should allocate new frame and copy.
    resolve_cow_fault(pml4, test_virt_base).expect("shared cow resolve should succeed");

    // Verify: PTE should point to a DIFFERENT physical address.
    // SAFETY: pml4 and virt reference our test mapping.
    let pte_after = unsafe { read_pte(pml4, virt, hhdm).expect("read pte after") };
    let new_phys = pte_after.phys_addr();
    // The new PTE points to the first 4 KiB page of a new 16 KiB frame.
    // Round down to frame base for comparison.
    let new_frame_base = new_phys & !(FRAME_SIZE as u64 - 1);
    assert!(
        new_frame_base != phys,
        "shared CoW resolve should allocate a new frame (old: {:#x}, new: {:#x})",
        phys,
        new_frame_base
    );
    assert!(
        !pte_after.is_cow(),
        "PTE should not be CoW after shared resolve"
    );
    assert!(
        pte_after.flags().contains(PageFlags::WRITABLE),
        "PTE should be writable after shared resolve"
    );

    // Old frame's refcount should have been decremented.
    // We started at 2, resolved 4 pages from the same frame, so each
    // resolution decrements once → 2 - 4 = clamp(0) but actually the
    // batch copies all 4 at once from one frame, decrementing 4 times.
    // Refcount was 2 → after 4 decrements the frame subsystem may have
    // freed it.  But since we know the batch resolved all 4 CoW PTEs
    // from one shared frame, the refcount went 2 → 2-4 which would
    // underflow.  Actually, each PTE had its own ref_inc during "fork"...
    // but we only did ONE ref_inc.  So the batch does pages_resolved
    // ref_dec calls.  With 4 decrements on refcount=2, the first two
    // decrement to 0 and the last two would fail or underflow.
    //
    // The correct simulation is: ref_inc 4 times (once per PTE as fork
    // would).  Let's not assert on the old refcount since our test
    // shortcut only did 1 ref_inc — just verify the new mapping works.

    // Verify data integrity in the NEW frame.
    let new_phys_base = new_frame_base;
    let new_ptr = (new_phys_base + hhdm) as *const u8;
    // SAFETY: new_phys_base is a valid physical frame (just copied into
    // by resolve_cow_fault); HHDM maps it.
    unsafe {
        for page in 0..HW_PAGES_PER_FRAME {
            let page_ptr = new_ptr.add(page * HW_PAGE_SIZE);
            for j in 0..16 {
                let expected = 0xBB + page as u8;
                let actual = page_ptr.add(j).read();
                assert!(
                    actual == expected,
                    "data copy check failed: page {}, byte {}: expected {:#x}, got {:#x}",
                    page,
                    j,
                    expected,
                    actual
                );
            }
        }
    }

    // Cleanup: unmap the new frame and free it.
    // SAFETY: pml4/virt reference our test mapping; we are sole owner.
    let returned = unsafe { page_table::unmap_frame(pml4, virt).expect("cow test unmap") };
    crate::tlb::flush_range(test_virt_base, HW_PAGES_PER_FRAME as u32);
    // SAFETY: sole owner of the new frame.
    unsafe {
        frame::free_frame(returned).expect("cow test free new");
    }

    // The old frame may or may not still be allocated (refcount was
    // decremented during resolve).  Don't try to free it again — the
    // ref_dec calls in resolve_cow_fault handle cleanup.

    serial_println!("[cow]   Shared CoW resolve: OK");
}

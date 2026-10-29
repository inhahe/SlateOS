//! DMA (Direct Memory Access) buffer management.
//!
//! Provides allocation of physically contiguous, cache-coherent memory
//! buffers for device DMA.  In a microkernel, userspace drivers need to
//! allocate DMA buffers and know their physical addresses so they can
//! program device descriptor rings.
//!
//! ## Design
//!
//! - **Contiguous physical allocation** via the buddy allocator's
//!   `alloc_order()` — guaranteed naturally aligned to the block size.
//! - **Kernel-accessible** via the HHDM (Higher Half Direct Map).
//! - **Userspace-accessible** via mapping into the driver process's
//!   address space with appropriate page flags (writable, no-cache
//!   for MMIO, or write-combining for framebuffers).
//! - **Physical address returned** alongside the virtual address so the
//!   driver can program device DMA descriptors.
//!
//! ## Addressing Constraints
//!
//! Some legacy devices can only DMA to the first 4 GiB (32-bit DMA).
//! The `DmaConstraint` enum lets callers specify the maximum physical
//! address.  The allocator attempts to satisfy the constraint; if it
//! can't, it returns `OutOfMemory`.
//!
//! ## IOMMU
//!
//! When an IOMMU is present, DMA buffers should also be mapped in the
//! device's I/O page table so the device can translate bus addresses
//! to physical addresses.  IOMMU support is a separate module
//! (not yet implemented).
//!
//! ## References
//!
//! - Linux `kernel/dma/` — DMA allocation and mapping framework
//! - Fuchsia `zircon/kernel/dev/iommu/` — IOMMU-aware DMA

// Most DMA functions are public API awaiting userspace driver syscall integration.
#![allow(dead_code)]

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::serial_println;

// ---------------------------------------------------------------------------
// DMA addressing constraints
// ---------------------------------------------------------------------------

/// Constraint on the physical address range for DMA buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaConstraint {
    /// No constraint — any physical address is acceptable (64-bit DMA).
    None,
    /// Physical address must be below 4 GiB (32-bit DMA devices).
    Below4G,
    /// Physical address must be below 16 MiB (ISA DMA — very legacy).
    Below16M,
}

// ---------------------------------------------------------------------------
// DMA buffer descriptor
// ---------------------------------------------------------------------------

/// A physically contiguous DMA buffer.
///
/// Holds the allocation metadata needed to free the buffer later.
/// The buffer is accessible via both its physical address (for device
/// programming) and its kernel virtual address (via HHDM).
#[derive(Debug)]
pub struct DmaBuffer {
    /// The underlying physical frame from the buddy allocator.
    frame: PhysFrame,
    /// Buddy order of the allocation (size = FRAME_SIZE × 2^order).
    order: usize,
    /// Size in bytes (may be less than the allocated block due to
    /// rounding up to the next power-of-two frame count).
    size: usize,
}

impl DmaBuffer {
    /// Physical address of the buffer (for device DMA descriptors).
    #[must_use]
    pub fn phys_addr(&self) -> u64 {
        self.frame.addr()
    }

    /// Kernel virtual address of the buffer (via HHDM).
    ///
    /// Returns `None` if the HHDM offset is not available (shouldn't
    /// happen after boot).
    #[must_use]
    pub fn virt_addr(&self) -> Option<*mut u8> {
        let hhdm = crate::mm::page_table::hhdm()?;
        Some((self.frame.addr() + hhdm) as *mut u8)
    }

    /// Size of the usable buffer in bytes.
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Actual allocated size (may be larger than requested due to
    /// power-of-two rounding).
    #[must_use]
    pub fn allocated_size(&self) -> usize {
        FRAME_SIZE << self.order
    }

    /// The buddy order of the allocation.
    #[must_use]
    pub fn order(&self) -> usize {
        self.order
    }

    /// The underlying physical frame.
    #[must_use]
    pub fn frame(&self) -> PhysFrame {
        self.frame
    }
}

// ---------------------------------------------------------------------------
// Allocation
// ---------------------------------------------------------------------------

/// Minimum buddy order needed to hold `size` bytes.
///
/// Returns the smallest `order` such that `FRAME_SIZE × 2^order >= size`.
fn size_to_order(size: usize) -> usize {
    if size == 0 {
        return 0;
    }
    // Number of frames needed (round up).
    let frames = size.div_ceil(FRAME_SIZE);
    // Smallest power of 2 >= frames.
    if frames <= 1 {
        0
    } else {
        // next_power_of_two().trailing_zeros() gives the order.
        (frames.next_power_of_two().trailing_zeros()) as usize
    }
}

/// Allocate a physically contiguous DMA buffer.
///
/// The buffer is at least `size` bytes, naturally aligned to the
/// allocated block size (e.g., a 64 KiB allocation is 64 KiB aligned).
///
/// The buffer is zeroed before return (prevents information leaks
/// to devices and simplifies driver initialization).
///
/// ## Arguments
///
/// - `size`: minimum buffer size in bytes.
/// - `constraint`: physical address constraint for the device.
///
/// ## Returns
///
/// A `DmaBuffer` on success, or `OutOfMemory` / `InvalidArgument`
/// on failure.
pub fn alloc(size: usize, constraint: DmaConstraint) -> KernelResult<DmaBuffer> {
    if size == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let order = size_to_order(size);

    // Allocate via the buddy allocator, respecting address constraints.
    // The constrained allocator walks the free list to find a block
    // entirely below the device's DMA address limit (like Linux's
    // GFP_DMA / GFP_DMA32 zone-aware allocation).
    let phys_frame = match constraint {
        DmaConstraint::None => frame::alloc_order(order)?,
        DmaConstraint::Below4G => frame::alloc_order_constrained(order, 0x1_0000_0000)?,
        DmaConstraint::Below16M => frame::alloc_order_constrained(order, 0x100_0000)?,
    };

    // Zero the buffer.
    let hhdm = crate::mm::page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let virt = (phys_frame.addr() + hhdm) as *mut u8;
    let alloc_size = FRAME_SIZE << order;
    // SAFETY: virt points to a valid HHDM mapping of the allocated
    // physical memory, alloc_size is the exact allocation size.
    unsafe {
        core::ptr::write_bytes(virt, 0, alloc_size);
    }

    super::memtype::charge(super::memtype::MemType::DmaBuf, 1u64 << order);

    Ok(DmaBuffer {
        frame: phys_frame,
        order,
        size,
    })
}

/// Free a DMA buffer.
///
/// # Safety
///
/// - The buffer must have been allocated by [`alloc()`].
/// - No device must be actively DMA-ing to/from this buffer.
/// - No references to the buffer's memory may exist.
pub unsafe fn free(buf: DmaBuffer) -> KernelResult<()> {
    // SAFETY: Caller guarantees the buffer was validly allocated
    // and is no longer in use by any device or CPU.
    let result = unsafe { frame::free_order(buf.frame, buf.order) };
    if result.is_ok() {
        super::memtype::uncharge(super::memtype::MemType::DmaBuf, 1u64 << buf.order);
    }
    result
}

// ---------------------------------------------------------------------------
// Syscall-facing API
// ---------------------------------------------------------------------------

/// Allocate a DMA buffer and map it into a process's address space.
///
/// Returns `(user_virt, phys_addr, actual_size)` on success.
///
/// The buffer is mapped as [`place`] maps it: write-back, which x86's
/// cache-coherent PCI DMA needs nothing more than, and never executable.
///
/// ## Arguments
///
/// - `pml4_phys`: the process's PML4 physical address.
/// - `size`: minimum buffer size in bytes.
/// - `constraint`: physical address constraint.
pub fn alloc_for_user(
    pml4_phys: u64,
    size: usize,
    constraint: DmaConstraint,
) -> KernelResult<(u64, u64, usize)> {
    let buf = alloc(size, constraint)?;
    // Find a free region in the process's user address space.
    // Use the upper portion of user space (below 0x7FFF_FFFF_F000).
    // We search downward from a high address to avoid conflicts with
    // the normal heap/stack regions.
    let user_virt = match find_user_vaddr(pml4_phys, buf.allocated_size()) {
        Ok(at) => at,
        Err(e) => {
            // SAFETY: just allocated, mapped nowhere and given to no device.
            unsafe { free(buf) }?;
            return Err(e);
        }
    };
    place(pml4_phys, user_virt, buf, None)
}

/// [`alloc_for_user`], placed where the process `pid` says: exactly `size`
/// bytes -- a power of two of frames (16 KiB) or more -- at `user_virt`, on
/// a frame boundary, inside memory `pid` reserved for itself and has not
/// touched there. The range becomes a device region of its own
/// ([`crate::proc::pcb::claim_reserved_region`]) for as long as the buffer
/// lives, so that no anonymous-memory path -- a fault, a fork, reclaim --
/// meets the device's frames, and is given back to the reservation when
/// the buffer is freed. Returns `(user_virt, phys_addr, size)`.
///
/// Everything about the request is checked before anything is allocated.
///
/// # Errors
///
/// `InvalidArgument` for a size or an address not as above, or a range
/// outside such a reservation; `AlreadyExists` for a page already in the
/// range (touched, or swapped out since); whatever `alloc` answers.
pub fn alloc_for_user_at(
    pml4_phys: u64,
    pid: crate::proc::pcb::ProcessId,
    user_virt: u64,
    size: usize,
    constraint: DmaConstraint,
) -> KernelResult<(u64, u64, usize)> {
    use crate::proc::pcb;

    let end = u64::try_from(size)
        .ok()
        .and_then(|bytes| user_virt.checked_add(bytes));
    let (true, Some(end)) = (size >= FRAME_SIZE && size.is_power_of_two(), end) else {
        return Err(KernelError::InvalidArgument);
    };
    // Refuses a range off a frame boundary as well as one outside a
    // reservation.
    let reservation = pcb::claim_reserved_region(pid, user_virt, end, arena_flags())?;
    let placed = alloc(size, constraint)
        .and_then(|buf| place(pml4_phys, user_virt, buf, Some((pid, reservation))));
    if placed.is_err() {
        // Nothing of the buffer is left in the range: the region is the
        // reservation's again, with whatever the process had put there.
        pcb::return_reserved_region(pid, pml4_phys, user_virt, end, reservation);
    }
    placed
}

/// How a user DMA buffer is mapped: present, writable, the process's.
/// Write-back, as the kernel's own view of the frames is: x86's PCI DMA is
/// cache-coherent, so nothing weaker is needed, and two views of one frame
/// with different memory types are what the SDM forbids (11.12.4). Never
/// executable: memory a device writes is data. FOREIGN: the frames are the
/// DMA tracker's, freed through it and only it -- not by an unmap or the
/// address space's teardown, which clear the entries and leave the frames
/// -- and a fork does not give them to the child, which holds no tracker
/// entry and no device (design-decisions 1579).
fn arena_flags() -> crate::mm::page_table::PageFlags {
    use crate::mm::page_table::PageFlags;
    PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::NO_EXECUTE
        | PageFlags::FOREIGN
}

/// The buffer `buf` mapped into the process at `user_virt` ([`arena_flags`])
/// and tracked: `(user_virt, phys_addr, size)`. `reservation`: the process
/// and the reservation the range was claimed from, given back when the
/// buffer is freed; `None` for a buffer the kernel placed.
///
/// The range must be vacant -- no page present, none swapped out -- which is
/// checked and the buffer mapped under the address space's page-table lock
/// (`mm::as_lock`), so that a fault resolving there at the same time either
/// installs first, and the placement is refused, or finds the buffer there.
/// Refused, the buffer is freed (`AlreadyExists`).
fn place(
    pml4_phys: u64,
    user_virt: u64,
    buf: DmaBuffer,
    reservation: Option<(crate::proc::pcb::ProcessId, crate::proc::pcb::Reservation)>,
) -> KernelResult<(u64, u64, usize)> {
    use crate::mm::page_table::{self, VirtAddr};

    let phys = buf.phys_addr();
    let alloc_size = buf.allocated_size();
    let flags = arena_flags();
    let hw_pages = alloc_size / 4096;
    let page = |i: usize| VirtAddr::new(user_virt.saturating_add((i as u64).saturating_mul(4096)));

    // How many pages were mapped, and why the rest were not.
    let (mapped, refused) = {
        let _held = crate::mm::as_lock::lock(pml4_phys);
        if (0..hw_pages).all(|i| page_table::leaf_vacant(pml4_phys, page(i))) {
            let mut mapped = 0usize;
            let mut refused = None;
            for i in 0..hw_pages {
                let offset = (i as u64).saturating_mul(4096);
                // SAFETY: the frames are the buffer's, allocated for this
                // alone; the table is the process's; the page is vacant, and
                // stays so while the lock is held.
                let done = unsafe {
                    page_table::map_4k_if_absent(
                        pml4_phys,
                        page(i),
                        phys.saturating_add(offset),
                        flags,
                    )
                };
                if !matches!(done, Ok(true)) {
                    refused = Some(done.err().unwrap_or(KernelError::AlreadyExists));
                    break;
                }
                mapped = mapped.saturating_add(1);
            }
            (mapped, refused)
        } else {
            (0, Some(KernelError::AlreadyExists))
        }
    };
    if let Some(e) = refused {
        let placed = DmaMappingInfo {
            pml4_phys,
            user_virt,
            phys,
            order: buf.order,
            size: mapped.saturating_mul(4096),
            device: None,
            reservation: None,
        };
        unmap_user_buffer(&placed);
        crate::tlb::flush_range(user_virt, mapped as u32);
        // SAFETY: nothing maps the buffer any more and no device was given
        // it.
        unsafe { free(buf) }?;
        return Err(e);
    }

    // The physical memory is now owned by the process mapping. It is freed
    // when the process frees the region (SYS_DMA_FREE), or the device it was
    // given to is released. `DmaBuffer` has no `Drop` impl (the buddy frame
    // is not auto-freed), so letting `buf` fall out of scope here does not
    // release the physical memory: it stays allocated, and tracked here
    // with its order, until then.
    DMA_MAPPINGS.lock().push(DmaMappingInfo {
        pml4_phys,
        user_virt,
        phys,
        order: buf.order,
        size: alloc_size,
        device: None,
        reservation,
    });

    Ok((user_virt, phys, alloc_size))
}

/// After a user DMA buffer has left its range: a range claimed from the
/// process's reservation is the reservation's again.
fn return_range(mapping: &DmaMappingInfo) {
    if let Some((pid, reservation)) = mapping.reservation {
        crate::proc::pcb::return_reserved_region(
            pid,
            mapping.pml4_phys,
            mapping.user_virt,
            mapping.user_virt.saturating_add(mapping.size as u64),
            reservation,
        );
    }
}

/// Free a DMA buffer previously allocated with `alloc_for_user`.
///
/// Unmaps the buffer from the process's address space and frees the
/// underlying physical frames.
///
/// ## Arguments
///
/// - `pml4_phys`: the process's PML4 physical address.
/// - `user_virt`: the virtual address returned by `alloc_for_user`.
pub fn free_for_user(pml4_phys: u64, user_virt: u64) -> KernelResult<()> {
    // Look up the DMA mapping.
    let mapping = untrack_dma_mapping(pml4_phys, user_virt).ok_or(KernelError::InvalidArgument)?;
    revoke_device(&mapping);

    let hw_pages = unmap_user_buffer(&mapping);
    // Flush TLB for the unmapped range.
    crate::tlb::flush_range(user_virt, hw_pages as u32);
    return_range(&mapping);
    free_block(&mapping)
}

/// Take a user DMA buffer out of its process without freeing it: its entry
/// is untracked and its pages unmapped, and its frames stay allocated for
/// good. For a buffer a device may still reach -- its IOMMU domain could not
/// be taken from it -- where handing the frames back would let the device
/// write into whatever is given them next. A leak, said in the log, is the
/// lesser harm.
pub fn abandon_for_user(pml4_phys: u64, user_virt: u64) {
    let Some(mapping) = untrack_dma_mapping(pml4_phys, user_virt) else {
        return;
    };
    let hw_pages = unmap_user_buffer(&mapping);
    crate::tlb::flush_range(user_virt, hw_pages as u32);
    // The range is the process's again; the frames it gets there are new.
    return_range(&mapping);
    serial_println!(
        "[dma] user buffer {:#x} ({} bytes) abandoned, not freed: a device may still reach it",
        mapping.phys,
        mapping.size
    );
}

// ---------------------------------------------------------------------------
// DMA mapping tracker
// ---------------------------------------------------------------------------

/// Metadata for a DMA mapping in a process's address space.
#[derive(Debug, Clone)]
struct DmaMappingInfo {
    /// Process PML4 physical address.
    pml4_phys: u64,
    /// User virtual address of the mapping.
    user_virt: u64,
    /// Physical address of the DMA buffer.
    phys: u64,
    /// Buddy allocator order.
    order: usize,
    /// Mapped size in bytes.
    size: usize,
    /// The device handle whose device may reach the buffer through its IOMMU
    /// domain (`devhandle`), told before the buffer is freed so that it
    /// stops reaching it first.
    device: Option<u64>,
    /// For a buffer placed where its process said ([`alloc_for_user_at`]):
    /// the process, and the reservation its range was claimed from, which
    /// gets the range back when the buffer leaves.
    reservation: Option<(crate::proc::pcb::ProcessId, crate::proc::pcb::Reservation)>,
}

/// Global table of active DMA mappings.
///
/// Protected by a spinlock.  The table is small (DMA allocations are
/// infrequent) so a simple Vec is fine.  A BTreeMap keyed by
/// (pml4_phys, user_virt) would be better for large systems.
static DMA_MAPPINGS: crate::sync::Mutex<alloc::vec::Vec<DmaMappingInfo>> =
    crate::sync::Mutex::named(alloc::vec::Vec::new(), b"DMA_MAP");

/// Record that the device behind device handle `device` may reach the user
/// DMA buffer at `user_virt` (design-decisions 1579), so that whatever frees
/// the buffer revokes the device's access first.
/// Returns whether the buffer was still tracked: one freed in the meantime
/// must not be given to the device.
#[must_use]
pub fn mark_device(pml4_phys: u64, user_virt: u64, device: u64) -> bool {
    DMA_MAPPINGS
        .lock()
        .iter_mut()
        .find(|m| m.pml4_phys == pml4_phys && m.user_virt == user_virt)
        .map(|m| m.device = Some(device))
        .is_some()
}

/// Before a user DMA buffer is freed: a device that may reach it stops.
fn revoke_device(mapping: &DmaMappingInfo) {
    if let Some(device) = mapping.device {
        crate::devhandle::revoke_arena(device, mapping.phys, mapping.size);
    }
}

fn untrack_dma_mapping(pml4_phys: u64, user_virt: u64) -> Option<DmaMappingInfo> {
    let mut mappings = DMA_MAPPINGS.lock();
    let pos = mappings
        .iter()
        .position(|m| m.pml4_phys == pml4_phys && m.user_virt == user_virt)?;
    Some(mappings.swap_remove(pos))
}

/// Whether the address space `pml4_phys` has a user DMA buffer at
/// `user_virt` still tracked.
#[must_use]
pub fn is_tracked(pml4_phys: u64, user_virt: u64) -> bool {
    DMA_MAPPINGS
        .lock()
        .iter()
        .any(|m| m.pml4_phys == pml4_phys && m.user_virt == user_virt)
}

/// Free every user DMA buffer of the address space `pml4_phys`: called as
/// the space is cleared, by exit or exec (`page_table::clear_user_address_space`).
pub fn free_all_for_process(pml4_phys: u64) {
    release_overlapping(pml4_phys, 0, u64::MAX);
}

/// Free, whole, every user DMA buffer of the address space `pml4_phys` that
/// overlaps `[start, end)`. Called before anything else unmaps or frees in
/// that range -- `munmap`, an exec or exit clearing the space -- so that a
/// buffer leaves only through here: unmapped, untracked and freed together.
/// Left to the generic paths, its frames would be freed page by page while
/// its tracking entry stayed, and freed again when the entry was.
pub fn release_overlapping(pml4_phys: u64, start: u64, end: u64) {
    let doomed: alloc::vec::Vec<DmaMappingInfo> = {
        let mut mappings = DMA_MAPPINGS.lock();
        let overlaps = |m: &DmaMappingInfo| {
            m.pml4_phys == pml4_phys
                && m.user_virt < end
                && m.user_virt.saturating_add(m.size as u64) > start
        };
        let doomed = mappings.iter().filter(|m| overlaps(m)).cloned().collect();
        mappings.retain(|m| !overlaps(m));
        doomed
    };
    for mapping in doomed {
        revoke_device(&mapping);
        let hw_pages = unmap_user_buffer(&mapping);
        crate::tlb::flush_range(mapping.user_virt, hw_pages as u32);
        // Whatever unmaps the range next (`munmap`) takes the part it names;
        // the rest is the reservation's again. A space being cleared resets
        // its regions after this.
        return_range(&mapping);
        // Nothing can be done about a block the allocator refuses -- it
        // would mean the block was already free -- but it is not silent.
        if let Err(e) = free_block(&mapping) {
            serial_println!(
                "[dma] user buffer {:#x} ({} bytes) could not be freed: {:?}",
                mapping.phys,
                mapping.size,
                e
            );
        }
    }
}

/// Free a user DMA buffer's block, which nothing maps any more, and uncharge
/// it as `alloc` charged it.
fn free_block(mapping: &DmaMappingInfo) -> KernelResult<()> {
    let frame = PhysFrame::from_addr(mapping.phys).ok_or(KernelError::InternalError)?;
    // SAFETY: the caller has unmapped every page of the buffer, and the
    // device it was for has been stopped or never had it.
    unsafe { frame::free_order(frame, mapping.order) }?;
    super::memtype::uncharge(super::memtype::MemType::DmaBuf, 1u64 << mapping.order);
    Ok(())
}

/// Clear the page-table entries of a user DMA buffer, leaving its frames
/// allocated. An entry no longer pointing into the buffer is someone else's
/// and is left alone. Returns how many 4 KiB pages the buffer covers.
fn unmap_user_buffer(mapping: &DmaMappingInfo) -> usize {
    use crate::mm::page_table::{self, VirtAddr};

    let hw_pages = mapping.size / 4096;
    for i in 0..hw_pages {
        let offset = (i as u64).saturating_mul(4096);
        let vaddr = VirtAddr::new(mapping.user_virt.saturating_add(offset));
        if page_table::translate(mapping.pml4_phys, vaddr)
            != Some(mapping.phys.saturating_add(offset))
        {
            continue;
        }
        // SAFETY: the entry maps this buffer's own page, mapped by
        // `alloc_for_user` into this table, and the buffer's tracking entry
        // is gone, so nothing else unmaps it. Clearing it frees nothing.
        let _ = unsafe { page_table::unmap_4k(mapping.pml4_phys, vaddr) };
    }
    hw_pages
}

// ---------------------------------------------------------------------------
// User vaddr finder
// ---------------------------------------------------------------------------

/// Find a free virtual address region in the upper user address space.
///
/// Searches downward from 0x7FFF_0000_0000 in steps of `size` to find
/// an unmapped region.  This avoids conflicts with the normal user heap
/// (which grows upward from low addresses) and the user stack (at the
/// top of user space).
fn find_user_vaddr(pml4_phys: u64, size: usize) -> KernelResult<u64> {
    use crate::mm::page_table::{self, VirtAddr};

    // Start scanning from a high user address.
    let mut candidate = 0x7FFF_0000_0000_u64;
    let align = size as u64; // naturally aligned to allocation size
    let min_addr = 0x1_0000_0000_u64; // Don't go below 4 GiB

    while candidate >= min_addr {
        // Align down.
        candidate &= !(align - 1);

        // Check if the first page of the candidate range is free.
        let vaddr = VirtAddr::new(candidate);
        match page_table::translate(pml4_phys, vaddr) {
            Some(_) => {
                // Already mapped — try the next region down.
                candidate = candidate.saturating_sub(align);
                continue;
            }
            None => {
                // Not mapped — check the last page too.
                let last_page = VirtAddr::new(candidate + (size as u64) - 4096);
                if page_table::translate(pml4_phys, last_page).is_none() {
                    return Ok(candidate);
                }
                candidate = candidate.saturating_sub(align);
            }
        }
    }

    Err(KernelError::OutOfMemory)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test DMA buffer allocation.
pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[dma] Running self-test...");

    // Test 1: size_to_order correctness.
    assert!(size_to_order(1) == 0, "1 byte → order 0");
    assert!(size_to_order(FRAME_SIZE) == 0, "1 frame → order 0");
    assert!(size_to_order(FRAME_SIZE + 1) == 1, "1 frame + 1 → order 1");
    assert!(size_to_order(FRAME_SIZE * 2) == 1, "2 frames → order 1");
    assert!(
        size_to_order(FRAME_SIZE * 3) == 2,
        "3 frames → order 2 (round up)"
    );
    assert!(size_to_order(FRAME_SIZE * 4) == 2, "4 frames → order 2");
    serial_println!("[dma]   size_to_order: OK");

    // Test 2: Allocate and free a small DMA buffer.
    let buf = alloc(4096, DmaConstraint::None).expect("DMA alloc 4K");
    assert!(buf.phys_addr() != 0, "phys addr nonzero");
    assert!(buf.size() == 4096, "size matches request");
    assert!(buf.allocated_size() >= 4096, "alloc size >= request");
    let virt = buf.virt_addr().expect("virt addr via HHDM");
    // Verify zeroed.
    // SAFETY: We just allocated this buffer and it's mapped via HHDM.
    let first_byte = unsafe { core::ptr::read_volatile(virt) };
    assert!(first_byte == 0, "buffer is zeroed");
    // SAFETY: We're the only user.
    unsafe { free(buf).expect("DMA free") };
    serial_println!("[dma]   alloc/free 4K: OK");

    // Test 3: Allocate a larger buffer (64 KiB = 4 frames = order 2).
    let buf = alloc(64 * 1024, DmaConstraint::None).expect("DMA alloc 64K");
    assert!(buf.order() == 2, "64K → order 2");
    assert!(buf.allocated_size() == FRAME_SIZE * 4, "alloc 4 frames");
    // Verify alignment (order 2 = 64 KiB aligned).
    assert!(
        buf.phys_addr().is_multiple_of(FRAME_SIZE as u64 * 4),
        "aligned"
    );
    // SAFETY: We're the only user.
    unsafe { free(buf).expect("DMA free 64K") };
    serial_println!("[dma]   alloc/free 64K: OK");

    // Test 4: Constrained allocation — Below4G.
    // Most of the boot test's RAM is below 4 GiB, so this succeeds while
    // anything is free there.  Verify the result is actually below 4 GiB.
    let buf = alloc(FRAME_SIZE, DmaConstraint::Below4G).expect("DMA alloc Below4G");
    let end = buf.phys_addr() + buf.allocated_size() as u64;
    assert!(
        end <= 0x1_0000_0000,
        "Below4G: end {:#x} exceeds 4 GiB",
        end
    );
    // SAFETY: We're the only user.
    unsafe { free(buf).expect("DMA free Below4G") };
    serial_println!("[dma]   constrained Below4G: OK");

    // Test 5: Constrained allocation — Below16M.
    // ISA DMA zone.  The frame allocator keeps it for requests like this
    // one (`frame::DMA_ZONE_END`), so it has free frames however much the
    // boot has allocated -- until 2026-10-02 it did not, and late in boot
    // this failed with gigabytes free (rq43).  Verify the result is below
    // 16 MiB.
    let buf = alloc(FRAME_SIZE, DmaConstraint::Below16M).expect("DMA alloc Below16M");
    let end = buf.phys_addr() + buf.allocated_size() as u64;
    assert!(end <= 0x100_0000, "Below16M: end {:#x} exceeds 16 MiB", end);
    // SAFETY: We're the only user.
    unsafe { free(buf).expect("DMA free Below16M") };
    serial_println!("[dma]   constrained Below16M: OK");

    // Test 6: Constrained allocation — larger buffer below 16M.
    // 64 KiB (order 2) should still fit in the first 16 MiB.
    let buf = alloc(64 * 1024, DmaConstraint::Below16M).expect("DMA alloc 64K Below16M");
    let end = buf.phys_addr() + buf.allocated_size() as u64;
    assert!(
        end <= 0x100_0000,
        "Below16M 64K: end {:#x} exceeds 16 MiB",
        end
    );
    assert!(buf.order() == 2, "64K → order 2");
    // SAFETY: We're the only user.
    unsafe { free(buf).expect("DMA free 64K Below16M") };
    serial_println!("[dma]   constrained 64K Below16M: OK");

    serial_println!("[dma] Self-test PASSED");
    Ok(())
}

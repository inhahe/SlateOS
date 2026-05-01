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
use crate::mm::frame::{self, PhysFrame, FRAME_SIZE};
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
    let hhdm = crate::mm::page_table::hhdm()
        .ok_or(KernelError::NotSupported)?;
    let virt = (phys_frame.addr() + hhdm) as *mut u8;
    let alloc_size = FRAME_SIZE << order;
    // SAFETY: virt points to a valid HHDM mapping of the allocated
    // physical memory, alloc_size is the exact allocation size.
    unsafe {
        core::ptr::write_bytes(virt, 0, alloc_size);
    }

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
    unsafe { frame::free_order(buf.frame, buf.order) }
}

// ---------------------------------------------------------------------------
// Syscall-facing API
// ---------------------------------------------------------------------------

/// Allocate a DMA buffer and map it into a process's address space.
///
/// Returns `(user_virt, phys_addr, actual_size)` on success.
///
/// The buffer is mapped with PRESENT + WRITABLE + USER + WRITE-THROUGH
/// flags (suitable for device DMA).  Write-through ensures the device
/// sees writes immediately without needing explicit cache flushes on
/// x86 (which has cache-coherent DMA for PCI devices).
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
    use crate::mm::page_table::{self, PageFlags, VirtAddr};

    let buf = alloc(size, constraint)?;
    let phys = buf.phys_addr();
    let alloc_size = buf.allocated_size();

    // Find a free region in the process's user address space.
    // Use the upper portion of user space (below 0x7FFF_FFFF_F000).
    // We search downward from a high address to avoid conflicts with
    // the normal heap/stack regions.
    let user_virt = find_user_vaddr(pml4_phys, alloc_size)?;

    // Map the DMA buffer into the process's address space.
    // Use write-through caching — x86 PCI DMA is cache-coherent, but
    // write-through avoids subtle ordering issues with device reads.
    let flags = PageFlags::PRESENT
        | PageFlags::WRITABLE
        | PageFlags::USER_ACCESSIBLE
        | PageFlags::WRITE_THROUGH;

    let hw_pages = alloc_size / 4096;
    for i in 0..hw_pages {
        let vaddr = VirtAddr::new(user_virt + (i as u64) * 4096);
        let paddr = phys + (i as u64) * 4096;
        // SAFETY: We own the physical memory and the process's page tables.
        unsafe {
            page_table::map_4k_if_absent(pml4_phys, vaddr, paddr, flags)?;
        }
    }

    // Leak the DmaBuffer — the physical memory is now owned by the
    // process mapping.  It will be freed when the process unmaps
    // the DMA region (via SYS_DMA_FREE).
    //
    // We need to store the order somewhere so we can free the correct
    // buddy block later.  We store it in a tracking table.
    track_dma_mapping(pml4_phys, user_virt, phys, buf.order, alloc_size);
    core::mem::forget(buf);

    Ok((user_virt, phys, alloc_size))
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
    use crate::mm::page_table::{self, VirtAddr};

    // Look up the DMA mapping.
    let mapping = untrack_dma_mapping(pml4_phys, user_virt)
        .ok_or(KernelError::InvalidArgument)?;

    // Unmap from the process's address space.
    let hw_pages = mapping.size / 4096;
    for i in 0..hw_pages {
        let vaddr = VirtAddr::new(user_virt + (i as u64) * 4096);
        // SAFETY: We own this mapping.
        let _ = unsafe { page_table::unmap_4k(pml4_phys, vaddr) };
    }

    // Flush TLB for the unmapped range.
    crate::tlb::flush_range(user_virt, hw_pages as u32);

    // Free the physical frames.
    let frame = PhysFrame::from_addr(mapping.phys)
        .ok_or(KernelError::InternalError)?;
    // SAFETY: We just unmapped all references, and the device should
    // have been stopped before calling free.
    unsafe { frame::free_order(frame, mapping.order) }
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
}

/// Global table of active DMA mappings.
///
/// Protected by a spinlock.  The table is small (DMA allocations are
/// infrequent) so a simple Vec is fine.  A BTreeMap keyed by
/// (pml4_phys, user_virt) would be better for large systems.
static DMA_MAPPINGS: spin::Mutex<alloc::vec::Vec<DmaMappingInfo>> =
    spin::Mutex::new(alloc::vec::Vec::new());

fn track_dma_mapping(pml4_phys: u64, user_virt: u64, phys: u64, order: usize, size: usize) {
    let mut mappings = DMA_MAPPINGS.lock();
    mappings.push(DmaMappingInfo { pml4_phys, user_virt, phys, order, size });
}

fn untrack_dma_mapping(pml4_phys: u64, user_virt: u64) -> Option<DmaMappingInfo> {
    let mut mappings = DMA_MAPPINGS.lock();
    let pos = mappings.iter().position(|m| m.pml4_phys == pml4_phys && m.user_virt == user_virt)?;
    Some(mappings.swap_remove(pos))
}

/// Free all DMA mappings for a process (called on process exit).
pub fn free_all_for_process(pml4_phys: u64) {
    let mut mappings = DMA_MAPPINGS.lock();
    let to_free: alloc::vec::Vec<_> = mappings
        .iter()
        .filter(|m| m.pml4_phys == pml4_phys)
        .cloned()
        .collect();
    mappings.retain(|m| m.pml4_phys != pml4_phys);
    drop(mappings); // Release lock before freeing.

    for mapping in to_free {
        let frame = match PhysFrame::from_addr(mapping.phys) {
            Some(f) => f,
            // SAFETY: If from_addr fails, the mapping was corrupt — skip.
            None => continue,
        };
        // SAFETY: Process is exiting; no device should be using the buffer.
        // We don't unmap (page tables are being torn down anyway).
        let _ = unsafe { frame::free_order(frame, mapping.order) };
    }
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
pub fn self_test() {
    serial_println!("[dma] Running self-test...");

    // Test 1: size_to_order correctness.
    assert!(size_to_order(1) == 0, "1 byte → order 0");
    assert!(size_to_order(FRAME_SIZE) == 0, "1 frame → order 0");
    assert!(size_to_order(FRAME_SIZE + 1) == 1, "1 frame + 1 → order 1");
    assert!(size_to_order(FRAME_SIZE * 2) == 1, "2 frames → order 1");
    assert!(size_to_order(FRAME_SIZE * 3) == 2, "3 frames → order 2 (round up)");
    assert!(size_to_order(FRAME_SIZE * 4) == 2, "4 frames → order 2");
    serial_println!("[dma]   size_to_order: OK");

    // Test 2: Allocate and free a small DMA buffer.
    let buf = alloc(4096, DmaConstraint::None)
        .expect("DMA alloc 4K");
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
    let buf = alloc(64 * 1024, DmaConstraint::None)
        .expect("DMA alloc 64K");
    assert!(buf.order() == 2, "64K → order 2");
    assert!(buf.allocated_size() == FRAME_SIZE * 4, "alloc 4 frames");
    // Verify alignment (order 2 = 64 KiB aligned).
    assert!(buf.phys_addr() % (FRAME_SIZE as u64 * 4) == 0, "aligned");
    // SAFETY: We're the only user.
    unsafe { free(buf).expect("DMA free 64K") };
    serial_println!("[dma]   alloc/free 64K: OK");

    // Test 4: Constrained allocation — Below4G.
    // Our QEMU VM has 256 MiB of RAM (all below 4 GiB), so this should
    // always succeed.  Verify the result is actually below 4 GiB.
    let buf = alloc(FRAME_SIZE, DmaConstraint::Below4G)
        .expect("DMA alloc Below4G");
    let end = buf.phys_addr() + buf.allocated_size() as u64;
    assert!(
        end <= 0x1_0000_0000,
        "Below4G: end {:#x} exceeds 4 GiB", end
    );
    // SAFETY: We're the only user.
    unsafe { free(buf).expect("DMA free Below4G") };
    serial_println!("[dma]   constrained Below4G: OK");

    // Test 5: Constrained allocation — Below16M.
    // ISA DMA zone.  With 256 MiB of RAM, the first 16 MiB should have
    // free frames.  Verify the result is below 16 MiB.
    let buf = alloc(FRAME_SIZE, DmaConstraint::Below16M)
        .expect("DMA alloc Below16M");
    let end = buf.phys_addr() + buf.allocated_size() as u64;
    assert!(
        end <= 0x100_0000,
        "Below16M: end {:#x} exceeds 16 MiB", end
    );
    // SAFETY: We're the only user.
    unsafe { free(buf).expect("DMA free Below16M") };
    serial_println!("[dma]   constrained Below16M: OK");

    // Test 6: Constrained allocation — larger buffer below 16M.
    // 64 KiB (order 2) should still fit in the first 16 MiB.
    let buf = alloc(64 * 1024, DmaConstraint::Below16M)
        .expect("DMA alloc 64K Below16M");
    let end = buf.phys_addr() + buf.allocated_size() as u64;
    assert!(
        end <= 0x100_0000,
        "Below16M 64K: end {:#x} exceeds 16 MiB", end
    );
    assert!(buf.order() == 2, "64K → order 2");
    // SAFETY: We're the only user.
    unsafe { free(buf).expect("DMA free 64K Below16M") };
    serial_println!("[dma]   constrained 64K Below16M: OK");

    serial_println!("[dma] Self-test PASSED");
}

//! Virtual Memory Area (VMA) tracking and address space management.
//!
//! A VMA describes a contiguous range of virtual addresses with uniform
//! properties (permissions, backing type).  The page fault handler uses
//! VMAs to decide how to resolve faults; the process manager uses them
//! to track what's mapped in each address space.
//!
//! ## VMA Kinds
//!
//! - **Anonymous**: Demand-paged memory.  Physical frames are allocated
//!   and zero-filled on first access (page fault).
//! - **Stack**: Grows downward on demand.  The VMA spans the maximum
//!   allowed stack range.  Pages are committed on fault within the
//!   growth limit.  A guard VMA below catches overflow.
//! - **Guard**: Non-accessible sentinel.  Any access is fatal (stack
//!   overflow or buffer overrun).  Never backed by physical memory.
//! - **Fixed**: Already fully backed by physical frames at creation
//!   time.  Page faults here indicate a bug (the PTE should be
//!   present).
//!
//! ## Address Space
//!
//! An [`AddressSpace`] pairs a PML4 physical address with a sorted
//! list of VMAs.  The kernel has one global address space; each user
//! process will have its own.  Kernel-half PML4 entries (256--511)
//! are shared across all address spaces.

use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::serial_println;
use core::ptr;

// ---------------------------------------------------------------------------
// VMA types
// ---------------------------------------------------------------------------

/// The kind of memory a VMA represents.
///
/// Determines how the page fault handler resolves faults within the
/// VMA's address range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmaKind {
    /// Demand-paged anonymous memory.  Frames are allocated and
    /// zero-filled on first access.
    Anonymous,

    /// Stack memory.  Behaves like anonymous memory but represents
    /// a thread stack that grows downward.  The VMA covers the full
    /// maximum range; pages are committed on demand.
    Stack,

    /// Guard page.  Any access is fatal — used to detect stack
    /// overflow cleanly instead of silently corrupting adjacent
    /// memory.
    Guard,

    /// Fixed mapping.  Already fully backed by physical frames.
    /// A page fault here means something went wrong (e.g., a PTE
    /// was corrupted or prematurely cleared).
    Fixed,
}

/// A Virtual Memory Area: a contiguous range of virtual addresses
/// with uniform properties.
///
/// Invariants:
/// - `start` is 16 KiB frame-aligned.
/// - `end > start`.
/// - `end` is 16 KiB frame-aligned.
/// - VMAs within an address space do not overlap.
pub struct Vma {
    /// Start virtual address (inclusive, frame-aligned).
    pub start: u64,
    /// End virtual address (exclusive, frame-aligned).
    pub end: u64,
    /// What kind of memory this is.
    pub kind: VmaKind,
    /// Page table flags applied to new mappings in this VMA.
    /// For guard VMAs this is unused (pages are never mapped).
    pub flags: PageFlags,
}

impl Vma {
    /// Check whether `addr` falls within this VMA's range.
    #[must_use]
    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end
    }

    /// Size of this VMA in bytes.
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub const fn len(&self) -> u64 {
        self.end - self.start
    }
}

// ---------------------------------------------------------------------------
// Address space
// ---------------------------------------------------------------------------

/// A virtual address space: PML4 table + VMA list.
///
/// The kernel has one global `AddressSpace`.  Each user process will
/// get its own.  The kernel-half entries (PML4 256--511) are shared
/// across all address spaces, so kernel mappings are consistent
/// everywhere.
pub struct AddressSpace {
    /// Physical address of the PML4 table.
    pml4_phys: u64,
    /// VMAs sorted by start address.  Invariant: no overlaps.
    vmas: Vec<Vma>,
}

impl AddressSpace {
    /// Create an address space wrapping an existing PML4 table.
    #[must_use]
    pub fn new(pml4_phys: u64) -> Self {
        Self {
            pml4_phys,
            vmas: Vec::new(),
        }
    }

    /// The PML4 physical address for this address space.
    #[must_use]
    pub const fn pml4_phys(&self) -> u64 {
        self.pml4_phys
    }

    /// Add a VMA to this address space.
    ///
    /// The VMA must not overlap any existing VMA.  Both `start` and
    /// `end` must be 16 KiB frame-aligned.
    ///
    /// # Errors
    ///
    /// - [`KernelError::BadAlignment`] if start/end are not aligned.
    /// - [`KernelError::InvalidArgument`] if `end <= start`.
    /// - [`KernelError::AlreadyExists`] if the range overlaps an
    ///   existing VMA.
    pub fn add_vma(&mut self, vma: Vma) -> KernelResult<()> {
        let start_aligned = VirtAddr::new(vma.start).is_frame_aligned();
        let end_aligned = VirtAddr::new(vma.end).is_frame_aligned();
        if !start_aligned || !end_aligned {
            return Err(KernelError::BadAlignment);
        }
        if vma.end <= vma.start {
            return Err(KernelError::InvalidArgument);
        }

        // Check for overlaps with existing VMAs.
        for existing in &self.vmas {
            if vma.start < existing.end && vma.end > existing.start {
                return Err(KernelError::AlreadyExists);
            }
        }

        // Insert sorted by start address (binary search for position).
        let pos = self.vmas
            .binary_search_by_key(&vma.start, |v| v.start)
            .unwrap_or_else(|p| p);
        self.vmas.insert(pos, vma);

        Ok(())
    }

    /// Remove the VMA that starts at `start`.
    ///
    /// Returns the removed VMA, or `None` if no VMA starts at that
    /// address.
    pub fn remove_vma(&mut self, start: u64) -> Option<Vma> {
        if let Ok(idx) = self.vmas.binary_search_by_key(&start, |v| v.start) {
            Some(self.vmas.remove(idx))
        } else {
            None
        }
    }

    /// Find the VMA containing `addr`.
    ///
    /// Returns `None` if no VMA covers this address.
    #[must_use]
    pub fn find_vma(&self, addr: u64) -> Option<&Vma> {
        // Binary search: find the last VMA whose start <= addr.
        // Since VMAs are sorted by start and don't overlap, the
        // containing VMA (if any) is the one with the largest start
        // that is <= addr.
        let idx = match self.vmas.binary_search_by_key(&addr, |v| v.start) {
            Ok(i) => i,
            Err(0) => return None,
            #[allow(clippy::arithmetic_side_effects)]
            Err(i) => i - 1,
        };
        let vma = self.vmas.get(idx)?;
        if vma.contains(addr) {
            Some(vma)
        } else {
            None
        }
    }

    /// Resolve a page fault within this address space.
    ///
    /// Looks up the faulting address in the VMA list, checks
    /// permissions against the error code, and allocates + maps a
    /// frame for demand-paged or stack regions.
    ///
    /// Returns `Ok(())` if the fault was resolved (the CPU should
    /// retry the faulting instruction).
    ///
    /// # Errors
    ///
    /// - [`KernelError::PageFault`] if the fault is not resolvable
    ///   (no VMA, guard page, permission violation, etc.).
    /// - [`KernelError::OutOfMemory`] if no frame is available.
    pub fn resolve_fault(
        &self,
        fault_addr: u64,
        is_present: bool,
        is_write: bool,
        is_instruction_fetch: bool,
    ) -> KernelResult<()> {
        // Look up the VMA.  Copy what we need to avoid borrow conflicts.
        let (kind, flags) = {
            let vma = self.find_vma(fault_addr).ok_or(KernelError::PageFault)?;
            (vma.kind, vma.flags)
        };

        // If the page IS present, this is a protection violation (e.g.,
        // write to read-only page).  We don't handle CoW yet, so this
        // is always fatal.
        if is_present {
            return Err(KernelError::PageFault);
        }

        // Permission checks against the VMA flags.
        if is_write && !flags.contains(PageFlags::WRITABLE) {
            return Err(KernelError::PageFault);
        }
        if is_instruction_fetch && flags.contains(PageFlags::NO_EXECUTE) {
            return Err(KernelError::PageFault);
        }

        match kind {
            VmaKind::Anonymous | VmaKind::Stack => {
                // Demand-page: allocate a frame, zero it, map it.
                self.demand_page(fault_addr, flags)
            }
            VmaKind::Guard => {
                // Guard page hit — always fatal (stack overflow).
                serial_println!(
                    "[fault] Guard page hit at {:#x} — stack overflow or invalid access",
                    fault_addr
                );
                Err(KernelError::PageFault)
            }
            VmaKind::Fixed => {
                // Fixed mapping should already be present.  A non-present
                // fault here means a PTE was corrupted.
                serial_println!(
                    "[fault] Fixed VMA fault at {:#x} — PTE missing for fixed mapping",
                    fault_addr
                );
                Err(KernelError::PageFault)
            }
        }
    }

    /// Allocate a physical frame, zero it, and map it at the frame-
    /// aligned address containing `fault_addr`.
    ///
    /// This is the core demand-paging operation.
    #[allow(clippy::arithmetic_side_effects)]
    fn demand_page(&self, fault_addr: u64, flags: PageFlags) -> KernelResult<()> {
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;

        // Round fault address down to the 16 KiB frame boundary.
        let frame_base = fault_addr & !(FRAME_SIZE as u64 - 1);
        let virt = VirtAddr::new(frame_base);

        // Allocate a physical frame.
        let phys_frame = frame::alloc_frame()?;

        // Zero the frame via HHDM before making it accessible at the
        // faulting address.  This prevents information leaks — the
        // frame may contain data from a previous user.
        //
        // SAFETY: phys_frame.to_virt(hhdm) is a valid HHDM virtual
        // address pointing to the frame's 16 KiB of physical memory.
        // We have exclusive ownership of this freshly-allocated frame.
        unsafe {
            let hhdm_ptr = phys_frame.to_virt(hhdm) as *mut u8;
            ptr::write_bytes(hhdm_ptr, 0, FRAME_SIZE);
        }

        // Map the frame at the faulting address.
        //
        // SAFETY: pml4_phys is our address space's PML4 (valid).
        // phys_frame is a valid, freshly-allocated frame.  virt is
        // frame-aligned and within a VMA that allows this mapping.
        let map_result = unsafe {
            page_table::map_frame(self.pml4_phys, virt, phys_frame, flags)
        };

        if let Err(e) = map_result {
            // Map failed — free the frame to avoid leaking it.
            //
            // SAFETY: phys_frame was just allocated and never exposed
            // to anything else.
            let _ = unsafe { frame::free_frame(phys_frame) };
            return Err(e);
        }

        // Flush the TLB so the CPU sees the new mapping when it
        // retries the faulting instruction.
        //
        // SAFETY: invlpg is always safe in ring 0.
        unsafe {
            page_table::flush_frame(virt);
        }

        Ok(())
    }
}

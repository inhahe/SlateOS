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

// ---------------------------------------------------------------------------
// VMA types
// ---------------------------------------------------------------------------

/// The kind of memory a VMA represents.
///
/// Determines how the page fault handler resolves faults within the
/// VMA's address range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // All variants needed by process address space management.
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

    /// Demand-paged, file-backed private mapping (`MAP_PRIVATE` over a
    /// regular file).  On the first access to a page, the fault handler
    /// allocates a zeroed frame, reads the corresponding bytes from the
    /// backing file (tail-zero-filled past EOF, matching Linux's page
    /// zero-fill), and maps it.  Because the mapping is private, later
    /// writes mutate that per-process frame and are never written back to
    /// the file — so once populated the frame behaves like anonymous
    /// memory (reclaimable to swap, CoW-shareable across fork).
    ///
    /// `handle` is a `crate::fs::handle` open-file id that the VMA owns an
    /// independent reference to (bumped via `dup_shared` at mmap time and
    /// on fork, released via `close` when the VMA is removed or the
    /// process exits).  Keeping our own reference decouples the mapping's
    /// lifetime from the file descriptor: `munmap`-after-`close` still
    /// reads the right bytes.
    FileBacked {
        /// Open-file handle id backing this mapping (owned reference).
        handle: u64,
        /// Byte offset into the backing file that `start` maps to.
        file_offset: u64,
    },
}

/// A Virtual Memory Area: a contiguous range of virtual addresses
/// with uniform properties.
///
/// Invariants:
/// - `start` is 16 KiB frame-aligned.
/// - `end > start`.
/// - `end` is 16 KiB frame-aligned.
/// - VMAs within an address space do not overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    #[allow(dead_code)] // Public API for VMA size queries.
    pub const fn len(&self) -> u64 {
        self.end - self.start
    }
}

// ---------------------------------------------------------------------------
// Unmapped-area search
// ---------------------------------------------------------------------------

/// Find the lowest frame-aligned gap of at least `size` bytes within the
/// half-open virtual-address window `[region_start, region_end)` that does
/// not overlap any VMA in `vmas`.
///
/// `vmas` must be sorted by `start` (the per-process VMA-list invariant
/// maintained by [`AddressSpace::add_vma`] / `pcb::add_vma`).  This is a
/// bottom-up first-fit search: it returns the lowest address whose
/// `[addr, addr + size)` range is entirely free, mirroring the default
/// bottom-up behaviour of Linux's `vm_unmapped_area` (`mm/mmap.c`).
///
/// Unlike a monotonic bump allocator, this reuses gaps freed by `munmap`,
/// so a process that maps and unmaps repeatedly does not exhaust the
/// window — and because it consults the live VMA list it can never hand
/// out an address that overlaps an existing mapping (e.g. a `MAP_FIXED`
/// overlay or a file-backed map).
///
/// Returns `None` if no free gap large enough exists (the caller maps that
/// to `OutOfMemory`/`ENOMEM`).
///
/// VMAs outside the window are handled naturally: those ending at or below
/// `region_start` are skipped; once a VMA starts at or beyond
/// `region_end`, the remainder of the (sorted) list is irrelevant.
#[must_use]
pub fn find_gap(vmas: &[Vma], size: u64, region_start: u64, region_end: u64) -> Option<u64> {
    if size == 0 || region_end <= region_start {
        return None;
    }

    // `cursor` is the lowest address in the window not yet ruled out by a
    // VMA we've already passed.
    let mut cursor = region_start;
    for vma in vmas {
        // VMAs are sorted by `start`; one ending at/below the cursor is
        // entirely behind us and cannot bound the current gap.
        if vma.end <= cursor {
            continue;
        }
        // The first VMA starting at/after the window end (and, by sort
        // order, every VMA after it) cannot reduce any remaining gap.
        if vma.start >= region_end {
            break;
        }
        // The candidate gap is `[cursor, vma.start)`.  `checked_sub`
        // yields `None` when `vma.start <= cursor` (the VMA straddles the
        // cursor from below), which correctly reports "no gap here".
        if let Some(gap) = vma.start.checked_sub(cursor) {
            if gap >= size {
                return Some(cursor);
            }
        }
        // Advance past this VMA (it may extend the cursor forward).
        if vma.end > cursor {
            cursor = vma.end;
        }
        if cursor >= region_end {
            return None;
        }
    }

    // Trailing gap `[cursor, region_end)` after the last in-window VMA.
    match region_end.checked_sub(cursor) {
        Some(tail) if tail >= size => Some(cursor),
        _ => None,
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
    #[allow(dead_code)] // Needed by fork (clone page tables) and process management.
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

        // If the page IS present, this is a protection violation.
        // Check for Copy-on-Write: a write to a present COW page can
        // be resolved by copying (or just marking writable if sole owner).
        if is_present {
            if is_write {
                // Try CoW resolution.  The CoW handler checks the PTE
                // for the COW bit and handles refcount/copy.
                return super::cow::resolve_cow_fault(self.pml4_phys, fault_addr);
            }
            // Present + read fault = genuine protection violation.
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
            VmaKind::FileBacked { .. } => {
                // File-backed mappings only ever live in a *process*
                // address space and are resolved by `pcb::try_resolve_fault`
                // (which has access to the file-handle layer).  The kernel's
                // global address space never registers one, so reaching here
                // is a bug.
                serial_println!(
                    "[fault] FileBacked VMA fault at {:#x} in kernel address space — unexpected",
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
        // Round fault address down to the 16 KiB frame boundary.
        #[allow(clippy::arithmetic_side_effects)]
        let frame_base = fault_addr & !(FRAME_SIZE as u64 - 1);
        let virt = VirtAddr::new(frame_base);

        // Allocate a zeroed physical frame.  Zeroing prevents information
        // leaks — the frame may contain data from a previous user.
        let phys_frame = frame::alloc_frame_zeroed()?;

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

        // Flush the local TLB so this CPU sees the new mapping when it
        // retries the faulting instruction.
        //
        // OPT: Use local-only flush (no IPI broadcast) because this is
        // a demand fault — the page was never mapped before, so no other
        // CPU can have a stale TLB entry for it.  The previous unmap (if
        // any) would have done a full TLB shootdown already.
        //
        // This avoids an IPI round-trip on every page fault in SMP mode
        // (~1-5µs saved per fault on real hardware).
        //
        // SAFETY: invlpg is always safe in ring 0.  Local-only is
        // correct because the mapping didn't exist before this handler.
        unsafe {
            page_table::flush_frame_local(virt);
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for VMA management (no hardware interaction — pure data structure).
pub fn self_test() {
    serial_println!("[vma] Running self-test...");

    // Use a fake PML4 address (we only test data structure operations,
    // not actual page table manipulation).
    let mut addr_space = AddressSpace::new(0xAAAA_0000);
    assert_eq!(addr_space.pml4_phys(), 0xAAAA_0000);

    let frame_size = FRAME_SIZE as u64;

    // Test 1: Add a VMA.
    let vma1 = Vma {
        start: 0x0000_4000_0000_0000,
        end: 0x0000_4000_0000_0000 + 4 * frame_size,
        kind: VmaKind::Anonymous,
        flags: PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER_ACCESSIBLE,
    };
    addr_space.add_vma(vma1).expect("add_vma should succeed");
    serial_println!("[vma]   Add VMA: OK");

    // Test 2: Find VMA containing an address.
    let found = addr_space.find_vma(0x0000_4000_0000_0000 + frame_size);
    assert!(found.is_some(), "should find VMA");
    assert_eq!(found.unwrap().kind, VmaKind::Anonymous);
    serial_println!("[vma]   Find VMA: OK");

    // Test 3: Address outside VMA returns None.
    let outside = addr_space.find_vma(0x0000_5000_0000_0000);
    assert!(outside.is_none(), "address outside VMA should be None");
    serial_println!("[vma]   Outside VMA: OK");

    // Test 4: Overlapping VMA is rejected.
    let overlap = Vma {
        start: 0x0000_4000_0000_0000 + 2 * frame_size,
        end: 0x0000_4000_0000_0000 + 6 * frame_size,
        kind: VmaKind::Anonymous,
        flags: PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER_ACCESSIBLE,
    };
    let result = addr_space.add_vma(overlap);
    assert!(result.is_err(), "overlapping VMA should be rejected");
    serial_println!("[vma]   Overlap rejection: OK");

    // Test 5: Non-overlapping VMA succeeds.
    let vma2 = Vma {
        start: 0x0000_4000_0001_0000,
        end: 0x0000_4000_0001_0000 + 2 * frame_size,
        kind: VmaKind::Stack,
        flags: PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER_ACCESSIBLE,
    };
    addr_space.add_vma(vma2).expect("non-overlapping VMA should succeed");
    serial_println!("[vma]   Non-overlapping add: OK");

    // Test 6: Remove VMA by start address.
    let removed = addr_space.remove_vma(0x0000_4000_0001_0000);
    assert!(removed.is_some(), "should remove existing VMA");
    assert_eq!(removed.unwrap().kind, VmaKind::Stack);
    serial_println!("[vma]   Remove VMA: OK");

    // Test 7: Remove non-existent VMA returns None.
    let no_remove = addr_space.remove_vma(0xDEAD_0000);
    assert!(no_remove.is_none(), "non-existent VMA removal should be None");
    serial_println!("[vma]   Remove non-existent: OK");

    // Test 8: Misaligned VMA is rejected.
    let misaligned = Vma {
        start: 0x0000_4000_0000_0001, // Not frame-aligned
        end: 0x0000_4000_0000_4000,
        kind: VmaKind::Anonymous,
        flags: PageFlags::PRESENT,
    };
    let result = addr_space.add_vma(misaligned);
    assert!(result.is_err(), "misaligned VMA should be rejected");
    serial_println!("[vma]   Alignment check: OK");

    // Test 9: Vma contains() and len().
    let vma3 = Vma {
        start: 0x1000_0000,
        end: 0x1000_0000 + 3 * frame_size,
        kind: VmaKind::Guard,
        flags: PageFlags::empty(),
    };
    assert!(vma3.contains(0x1000_0000));
    assert!(vma3.contains(0x1000_0000 + frame_size));
    assert!(!vma3.contains(0x1000_0000 + 3 * frame_size)); // End is exclusive.
    assert_eq!(vma3.len(), 3 * frame_size);
    serial_println!("[vma]   contains/len: OK");

    // Clean up: remove remaining VMA.
    addr_space.remove_vma(0x0000_4000_0000_0000);

    // Test 10: find_gap over an empty VMA list returns the region start.
    let base = 0x0000_0060_0000_0000_u64;
    let end = 0x0000_0070_0000_0000_u64;
    assert_eq!(
        find_gap(&[], 4 * frame_size, base, end),
        Some(base),
        "empty list: lowest address"
    );

    // A gap-finder VMA helper (kind/flags are irrelevant to find_gap).
    let mk = |s: u64, e: u64| Vma {
        start: s,
        end: e,
        kind: VmaKind::Anonymous,
        flags: PageFlags::PRESENT,
    };

    // Test 11: a single VMA at the base pushes the allocation past it.
    let v_a = [mk(base, base + 4 * frame_size)];
    assert_eq!(
        find_gap(&v_a, 2 * frame_size, base, end),
        Some(base + 4 * frame_size),
        "single VMA at base: allocate after it"
    );

    // Test 12: a freed hole between two VMAs is reused (first-fit).
    //   [base, base+2f) used | [base+2f, base+6f) FREE | [base+6f, base+8f) used
    let v_b = [
        mk(base, base + 2 * frame_size),
        mk(base + 6 * frame_size, base + 8 * frame_size),
    ];
    assert_eq!(
        find_gap(&v_b, 4 * frame_size, base, end),
        Some(base + 2 * frame_size),
        "reuse the exact-size hole between mappings"
    );
    // A request one frame larger than the hole skips it and lands after
    // the last VMA.
    assert_eq!(
        find_gap(&v_b, 5 * frame_size, base, end),
        Some(base + 8 * frame_size),
        "too big for the hole: allocate after the last VMA"
    );

    // Test 13: a request that does not fit anywhere in the window → None.
    let v_full = [mk(base, end - frame_size)];
    assert_eq!(
        find_gap(&v_full, 2 * frame_size, base, end),
        None,
        "no gap large enough: None"
    );

    // Test 14: VMAs entirely below/above the window are ignored.
    let v_outside = [
        mk(base - 8 * frame_size, base - 2 * frame_size), // wholly below
        mk(end + 2 * frame_size, end + 4 * frame_size),   // wholly above
    ];
    assert_eq!(
        find_gap(&v_outside, 4 * frame_size, base, end),
        Some(base),
        "out-of-window VMAs do not consume the window"
    );

    // Test 15: a VMA straddling the window start consumes the low part.
    let v_straddle = [mk(base - 2 * frame_size, base + 3 * frame_size)];
    assert_eq!(
        find_gap(&v_straddle, frame_size, base, end),
        Some(base + 3 * frame_size),
        "straddling VMA: allocate after its in-window tail"
    );

    // Test 16: degenerate inputs.
    assert_eq!(find_gap(&[], 0, base, end), None, "zero size: None");
    assert_eq!(find_gap(&[], 4 * frame_size, end, base), None, "inverted window: None");
    serial_println!("[vma]   find_gap: OK");

    serial_println!("[vma] Self-test PASSED");
}

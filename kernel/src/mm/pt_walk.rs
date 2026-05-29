//! Page table walker — generic iteration over mapped entries.
//!
//! Provides a structured way to walk all (or a range of) page table
//! entries in an address space, calling a visitor closure for each leaf
//! entry.  This is the foundation for:
//!
//! - **Fork**: copy all PTEs to a new address space (with CoW marking).
//! - **Process exit**: unmap all pages and free frames.
//! - **RSS calculation**: count physical pages currently mapped.
//! - **Page aging**: scan all PTEs for Accessed/Dirty bits.
//! - **Debugging**: dump the page table structure for diagnostics.
//!
//! ## Design
//!
//! The walker uses a non-recursive approach with explicit iteration over
//! the 4-level x86_64 page table hierarchy (PML4 → PDPT → PD → PT).
//! For each present leaf entry, it calls the provided visitor with:
//! - The virtual address of the mapping
//! - The physical address of the backing frame
//! - The effective flags (AND of all parent flags + leaf flags)
//! - The mapping size (4 KiB, 2 MiB, or 1 GiB)
//!
//! ## Range Walking
//!
//! `walk_range()` efficiently skips non-relevant page table levels by
//! computing the starting indices for the requested virtual address range.
//! This avoids iterating 512×512×512×512 entries when only a small
//! range needs scanning.
//!
//! ## References
//!
//! - Linux `mm/pagewalk.c` — `walk_page_range()`, `struct mm_walk_ops`
//! - Linux `arch/x86/mm/init_64.c` — `kernel_physical_mapping_init()`

// Diagnostic/profiling subsystem — all public API for tooling and kshell
// commands; many helpers may not have call sites in production paths yet.
#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use crate::serial_println;
use crate::mm::page_table::{self, PageFlags};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of entries in each page table level.
const ENTRIES_PER_TABLE: usize = 512;

// ---------------------------------------------------------------------------
// Walk callback types
// ---------------------------------------------------------------------------

/// Information about a single mapped page discovered during a walk.
#[derive(Debug, Clone, Copy)]
pub struct WalkEntry {
    /// Virtual address of the mapping.
    pub virt_addr: u64,
    /// Physical address of the backing frame.
    pub phys_addr: u64,
    /// Effective page flags (combined from all levels).
    pub flags: PageFlags,
    /// Size of this mapping in bytes (4096, 2MiB, or 1GiB).
    pub size: u64,
    /// Whether this is a leaf PTE (4 KiB page).
    pub is_4k: bool,
    /// Whether this is a 2 MiB huge page (PD level).
    pub is_2m: bool,
    /// Whether this is a 1 GiB huge page (PDPT level).
    pub is_1g: bool,
}

/// Action returned by the walk visitor to control iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkAction {
    /// Continue walking.
    Continue,
    /// Stop walking immediately.
    Stop,
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total walk operations performed.
static WALK_OPS: AtomicU64 = AtomicU64::new(0);

/// Total leaf entries visited across all walks.
static ENTRIES_VISITED: AtomicU64 = AtomicU64::new(0);

/// Walk statistics.
#[derive(Debug, Clone, Copy)]
pub struct WalkStats {
    /// Total walk operations since boot.
    pub walk_ops: u64,
    /// Total leaf entries visited.
    pub entries_visited: u64,
}

/// Get walk statistics.
#[must_use]
pub fn stats() -> WalkStats {
    WalkStats {
        walk_ops: WALK_OPS.load(Ordering::Relaxed),
        entries_visited: ENTRIES_VISITED.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Walk results
// ---------------------------------------------------------------------------

/// Summary of a page table walk.
#[derive(Debug, Clone, Copy)]
pub struct WalkSummary {
    /// Number of 4 KiB pages found.
    pub pages_4k: u64,
    /// Number of 2 MiB huge pages found.
    pub pages_2m: u64,
    /// Number of 1 GiB huge pages found.
    pub pages_1g: u64,
    /// Total mapped bytes.
    pub total_mapped_bytes: u64,
    /// Whether the walk was stopped early by the visitor.
    pub stopped_early: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Walk all mapped entries in a page table hierarchy.
///
/// Iterates every present leaf entry (4 KiB, 2 MiB, or 1 GiB page)
/// in the given PML4 and calls `visitor` for each.
///
/// The visitor can return `WalkAction::Stop` to terminate early.
///
/// # Safety
///
/// `pml4_phys` must be the physical address of a valid PML4 table.
/// The page table must not be concurrently modified during the walk
/// (or the caller must accept that some entries may be stale/missing).
#[allow(clippy::arithmetic_side_effects)]
pub unsafe fn walk_all(
    pml4_phys: u64,
    mut visitor: impl FnMut(WalkEntry) -> WalkAction,
) -> WalkSummary {
    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return WalkSummary {
            pages_4k: 0, pages_2m: 0, pages_1g: 0,
            total_mapped_bytes: 0, stopped_early: false,
        },
    };

    WALK_OPS.fetch_add(1, Ordering::Relaxed);

    let mut summary = WalkSummary {
        pages_4k: 0,
        pages_2m: 0,
        pages_1g: 0,
        total_mapped_bytes: 0,
        stopped_early: false,
    };

    let mut visited: u64 = 0;

    // SAFETY: pml4_phys is valid (caller guarantee).  Each subsequent
    // read_entry uses the phys_addr from the prior level, which was checked
    // present.  All indices are in [0, 512).
    'pml4: for pml4_idx in 0..ENTRIES_PER_TABLE {
        let pml4e = unsafe { page_table::read_entry(pml4_phys, pml4_idx, hhdm) };
        if !pml4e.is_present() {
            continue;
        }

        let pdpt_phys = pml4e.phys_addr();

        for pdpt_idx in 0..ENTRIES_PER_TABLE {
            let pdpte = unsafe { page_table::read_entry(pdpt_phys, pdpt_idx, hhdm) };
            if !pdpte.is_present() {
                continue;
            }

            // 1 GiB huge page?
            if pdpte.is_huge() {
                let virt = compose_vaddr(pml4_idx, pdpt_idx, 0, 0);
                let entry = WalkEntry {
                    virt_addr: virt,
                    phys_addr: pdpte.phys_addr(),
                    flags: pdpte.flags(),
                    size: 1024 * 1024 * 1024, // 1 GiB
                    is_4k: false,
                    is_2m: false,
                    is_1g: true,
                };
                summary.pages_1g += 1;
                summary.total_mapped_bytes += entry.size;
                visited += 1;
                if visitor(entry) == WalkAction::Stop {
                    summary.stopped_early = true;
                    break 'pml4;
                }
                continue;
            }

            let pd_phys = pdpte.phys_addr();

            for pd_idx in 0..ENTRIES_PER_TABLE {
                let pde = unsafe { page_table::read_entry(pd_phys, pd_idx, hhdm) };
                if !pde.is_present() {
                    continue;
                }

                // 2 MiB huge page?
                if pde.is_huge() {
                    let virt = compose_vaddr(pml4_idx, pdpt_idx, pd_idx, 0);
                    let entry = WalkEntry {
                        virt_addr: virt,
                        phys_addr: pde.phys_addr(),
                        flags: pde.flags(),
                        size: 2 * 1024 * 1024, // 2 MiB
                        is_4k: false,
                        is_2m: true,
                        is_1g: false,
                    };
                    summary.pages_2m += 1;
                    summary.total_mapped_bytes += entry.size;
                    visited += 1;
                    if visitor(entry) == WalkAction::Stop {
                        summary.stopped_early = true;
                        break 'pml4;
                    }
                    continue;
                }

                let pt_phys = pde.phys_addr();

                for pt_idx in 0..ENTRIES_PER_TABLE {
                    let pte = unsafe { page_table::read_entry(pt_phys, pt_idx, hhdm) };
                    if !pte.is_present() {
                        continue;
                    }

                    let virt = compose_vaddr(pml4_idx, pdpt_idx, pd_idx, pt_idx);
                    let entry = WalkEntry {
                        virt_addr: virt,
                        phys_addr: pte.phys_addr(),
                        flags: pte.flags(),
                        size: 4096, // 4 KiB hardware page
                        is_4k: true,
                        is_2m: false,
                        is_1g: false,
                    };
                    summary.pages_4k += 1;
                    summary.total_mapped_bytes += 4096;
                    visited += 1;
                    if visitor(entry) == WalkAction::Stop {
                        summary.stopped_early = true;
                        break 'pml4;
                    }
                }
            }
        }
    }

    ENTRIES_VISITED.fetch_add(visited, Ordering::Relaxed);
    summary
}

/// Walk mapped entries in a specific virtual address range.
///
/// Only visits entries whose virtual address falls within
/// `[start_vaddr, end_vaddr)`.  Efficiently skips irrelevant page table
/// levels by computing starting indices.
///
/// # Safety
///
/// Same requirements as [`walk_all`].
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
pub unsafe fn walk_range(
    pml4_phys: u64,
    start_vaddr: u64,
    end_vaddr: u64,
    mut visitor: impl FnMut(WalkEntry) -> WalkAction,
) -> WalkSummary {
    let hhdm = match page_table::hhdm() {
        Some(h) => h,
        None => return WalkSummary {
            pages_4k: 0, pages_2m: 0, pages_1g: 0,
            total_mapped_bytes: 0, stopped_early: false,
        },
    };

    if start_vaddr >= end_vaddr {
        return WalkSummary {
            pages_4k: 0, pages_2m: 0, pages_1g: 0,
            total_mapped_bytes: 0, stopped_early: false,
        };
    }

    WALK_OPS.fetch_add(1, Ordering::Relaxed);

    let mut summary = WalkSummary {
        pages_4k: 0,
        pages_2m: 0,
        pages_1g: 0,
        total_mapped_bytes: 0,
        stopped_early: false,
    };
    let mut visited: u64 = 0;

    // Compute index ranges for the requested virtual address range.
    let start_pml4 = ((start_vaddr >> 39) & 0x1FF) as usize;
    let end_pml4 = ((end_vaddr.saturating_sub(1) >> 39) & 0x1FF) as usize;

    // SAFETY: pml4_phys is valid (caller guarantee).  Each subsequent
    // read_entry uses phys_addr from a present parent entry.
    'pml4: for pml4_idx in start_pml4..=end_pml4.min(511) {
        let pml4e = unsafe { page_table::read_entry(pml4_phys, pml4_idx, hhdm) };
        if !pml4e.is_present() {
            continue;
        }

        let pdpt_phys = pml4e.phys_addr();

        for pdpt_idx in 0..ENTRIES_PER_TABLE {
            let pdpte = unsafe { page_table::read_entry(pdpt_phys, pdpt_idx, hhdm) };
            if !pdpte.is_present() {
                continue;
            }

            if pdpte.is_huge() {
                let virt = compose_vaddr(pml4_idx, pdpt_idx, 0, 0);
                if virt >= end_vaddr || virt + (1 << 30) <= start_vaddr {
                    continue; // Out of range.
                }
                let entry = WalkEntry {
                    virt_addr: virt,
                    phys_addr: pdpte.phys_addr(),
                    flags: pdpte.flags(),
                    size: 1024 * 1024 * 1024,
                    is_4k: false,
                    is_2m: false,
                    is_1g: true,
                };
                summary.pages_1g += 1;
                summary.total_mapped_bytes += entry.size;
                visited += 1;
                if visitor(entry) == WalkAction::Stop {
                    summary.stopped_early = true;
                    break 'pml4;
                }
                continue;
            }

            let pd_phys = pdpte.phys_addr();

            for pd_idx in 0..ENTRIES_PER_TABLE {
                let pde = unsafe { page_table::read_entry(pd_phys, pd_idx, hhdm) };
                if !pde.is_present() {
                    continue;
                }

                if pde.is_huge() {
                    let virt = compose_vaddr(pml4_idx, pdpt_idx, pd_idx, 0);
                    if virt >= end_vaddr || virt + (1 << 21) <= start_vaddr {
                        continue; // Out of range.
                    }
                    let entry = WalkEntry {
                        virt_addr: virt,
                        phys_addr: pde.phys_addr(),
                        flags: pde.flags(),
                        size: 2 * 1024 * 1024,
                        is_4k: false,
                        is_2m: true,
                        is_1g: false,
                    };
                    summary.pages_2m += 1;
                    summary.total_mapped_bytes += entry.size;
                    visited += 1;
                    if visitor(entry) == WalkAction::Stop {
                        summary.stopped_early = true;
                        break 'pml4;
                    }
                    continue;
                }

                let pt_phys = pde.phys_addr();

                for pt_idx in 0..ENTRIES_PER_TABLE {
                    let pte = unsafe { page_table::read_entry(pt_phys, pt_idx, hhdm) };
                    if !pte.is_present() {
                        continue;
                    }

                    let virt = compose_vaddr(pml4_idx, pdpt_idx, pd_idx, pt_idx);
                    if virt >= end_vaddr {
                        break 'pml4; // Past the end of range.
                    }
                    if virt < start_vaddr {
                        continue; // Before start of range.
                    }

                    let entry = WalkEntry {
                        virt_addr: virt,
                        phys_addr: pte.phys_addr(),
                        flags: pte.flags(),
                        size: 4096,
                        is_4k: true,
                        is_2m: false,
                        is_1g: false,
                    };
                    summary.pages_4k += 1;
                    summary.total_mapped_bytes += 4096;
                    visited += 1;
                    if visitor(entry) == WalkAction::Stop {
                        summary.stopped_early = true;
                        break 'pml4;
                    }
                }
            }
        }
    }

    ENTRIES_VISITED.fetch_add(visited, Ordering::Relaxed);
    summary
}

/// Count the resident set size (total mapped physical pages) of an
/// address space without allocating memory.
///
/// Returns (pages_4k, pages_2m, pages_1g, total_bytes).
///
/// # Safety
///
/// `pml4_phys` must be a valid PML4 physical address.
#[must_use]
pub unsafe fn count_mapped(pml4_phys: u64) -> (u64, u64, u64, u64) {
    // SAFETY: Caller guarantees pml4_phys is valid; forwarded to walk_all.
    let summary = unsafe {
        walk_all(pml4_phys, |_| WalkAction::Continue)
    };
    (summary.pages_4k, summary.pages_2m, summary.pages_1g, summary.total_mapped_bytes)
}

/// Count mapped pages in a specific virtual address range.
///
/// # Safety
///
/// `pml4_phys` must be a valid PML4 physical address.
#[must_use]
pub unsafe fn count_mapped_range(
    pml4_phys: u64,
    start: u64,
    end: u64,
) -> (u64, u64, u64, u64) {
    // SAFETY: Caller guarantees pml4_phys is valid; forwarded to walk_range.
    let summary = unsafe {
        walk_range(pml4_phys, start, end, |_| WalkAction::Continue)
    };
    (summary.pages_4k, summary.pages_2m, summary.pages_1g, summary.total_mapped_bytes)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Compose a virtual address from page table indices.
///
/// x86_64 virtual address layout (48-bit):
///   [47:39] PML4 index (9 bits)
///   [38:30] PDPT index (9 bits)
///   [29:21] PD index (9 bits)
///   [20:12] PT index (9 bits)
///   [11:0]  Offset (12 bits, zero for page-aligned)
///
/// If bit 47 is set, sign-extend to bits 63:48 (canonical form).
#[inline]
#[allow(clippy::arithmetic_side_effects)]
const fn compose_vaddr(pml4_idx: usize, pdpt_idx: usize, pd_idx: usize, pt_idx: usize) -> u64 {
    let raw = ((pml4_idx as u64) << 39)
        | ((pdpt_idx as u64) << 30)
        | ((pd_idx as u64) << 21)
        | ((pt_idx as u64) << 12);

    // Sign-extend if bit 47 is set (canonical address).
    if raw & (1 << 47) != 0 {
        raw | 0xFFFF_0000_0000_0000
    } else {
        raw
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the page table walker.
pub fn self_test() {
    serial_println!("[pt_walk] Running self-test...");

    // Test 1: compose_vaddr produces correct addresses.
    // PML4=0, PDPT=0, PD=0, PT=0 → 0x0
    assert_eq!(compose_vaddr(0, 0, 0, 0), 0);

    // PML4=0, PDPT=0, PD=0, PT=1 → 0x1000
    assert_eq!(compose_vaddr(0, 0, 0, 1), 0x1000);

    // PML4=0, PDPT=0, PD=1, PT=0 → 0x200000 (2 MiB)
    assert_eq!(compose_vaddr(0, 0, 1, 0), 0x20_0000);

    // PML4=0, PDPT=1, PD=0, PT=0 → 0x40000000 (1 GiB)
    assert_eq!(compose_vaddr(0, 1, 0, 0), 0x4000_0000);

    // PML4=256, PDPT=0, PD=0, PT=0 → kernel half (sign-extended).
    let kernel_addr = compose_vaddr(256, 0, 0, 0);
    assert_eq!(kernel_addr, 0xFFFF_8000_0000_0000);
    serial_println!("[pt_walk]   compose_vaddr: OK");

    // Test 2: Walk the current kernel page tables.
    // Read CR3 to get the current PML4.
    let cr3: u64;
    // SAFETY: Reading CR3 to obtain the current PML4 physical address.
    unsafe {
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nomem, nostack, preserves_flags),
        );
    }
    let pml4_phys = cr3 & 0x000F_FFFF_FFFF_F000;

    // Walk the kernel half (PML4 entries 256-511).
    // Use walk_all with an early stop after finding a few entries.
    // We know the kernel has mappings — we're executing from them.
    let mut found_entries = 0u32;
    // SAFETY: pml4_phys came from CR3, which is always a valid PML4.
    let summary = unsafe {
        walk_all(pml4_phys, |_entry| {
            found_entries += 1;
            if found_entries >= 10 {
                WalkAction::Stop
            } else {
                WalkAction::Continue
            }
        })
    };
    serial_println!("[pt_walk]   Kernel walk (first 10): 4k={} 2m={} 1g={} ({} bytes)",
        summary.pages_4k, summary.pages_2m, summary.pages_1g, summary.total_mapped_bytes);
    assert!(found_entries > 0, "kernel page tables should have mapped entries");
    serial_println!("[pt_walk]   Found mapped entries: OK ({})", found_entries);

    // Test 3: Walk with Stop action (stop after 3 entries).
    let mut count = 0u32;
    // SAFETY: pml4_phys from CR3 is valid.
    let summary = unsafe {
        walk_all(pml4_phys, |_entry| {
            count += 1;
            if count >= 3 {
                WalkAction::Stop
            } else {
                WalkAction::Continue
            }
        })
    };
    assert!(summary.stopped_early);
    assert!(count <= 3);
    serial_println!("[pt_walk]   Walk with Stop: OK (stopped after {} entries)", count);

    // Test 4: Walk empty range returns nothing.
    // SAFETY: pml4_phys from CR3 is valid.
    let summary = unsafe {
        walk_range(pml4_phys, 0x1000_0000, 0x1000_0000, |_| WalkAction::Continue)
    };
    assert_eq!(summary.total_mapped_bytes, 0);
    serial_println!("[pt_walk]   Empty range: OK");

    // Test 5: Walk user space (should be empty in kernel-only context).
    // SAFETY: pml4_phys from CR3 is valid.
    let summary = unsafe {
        walk_range(pml4_phys, 0x0000_0000_0040_0000, 0x0000_0000_0100_0000, |_| {
            WalkAction::Continue
        })
    };
    serial_println!("[pt_walk]   User range: 4k={} 2m={} (expected ~0 in kernel context)",
        summary.pages_4k, summary.pages_2m);

    // Test 6: Statistics updated.
    let s = stats();
    assert!(s.walk_ops >= 3);
    assert!(s.entries_visited > 0);
    serial_println!("[pt_walk]   Stats: ops={}, entries_visited={}",
        s.walk_ops, s.entries_visited);

    serial_println!("[pt_walk] Self-test PASSED");
}

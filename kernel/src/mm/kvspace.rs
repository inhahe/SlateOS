//! Kernel virtual address space layout — centralized region registry.
//!
//! This module defines and tracks all kernel virtual address regions.
//! It prevents accidental overlap when adding new subsystems and provides
//! a single source of truth for the kernel's memory map.
//!
//! USER_* constants and `all_regions()` are part of the VA layout API;
//! they're exported for tooling/debugging even when not directly used
//! by the kernel itself, so we allow dead_code at module scope.

#![allow(dead_code)]
//!
//! ## Layout (x86_64, higher-half kernel)
//!
//! ```text
//! 0x0000_0000_0000_0000 .. 0x0000_7FFF_FFFF_FFFF  User space (128 TiB)
//!     0x0000_0000_0040_0000  ELF load base
//!     0x0000_0060_0000_0000  Mmap region (MAP_LAZY / MAP_MMIO)
//!     0x0000_7FFF_FFFF_0000  User stack top (grows down)
//!
//! 0x0000_8000_0000_0000 .. 0xFFFF_7FFF_FFFF_FFFF  Non-canonical hole
//!
//! 0xFFFF_8000_0000_0000 .. 0xFFFF_FFFF_FFFF_FFFF  Kernel space (128 TiB)
//!     HHDM (from bootloader)   Physical memory direct-map
//!     0xFFFF_C100_0000_0000    Kernel stacks (per-task, with guard pages)
//!     0xFFFF_C200_0000_0000    Huge pages (2 MiB mappings)
//!     0xFFFF_C300_0000_0000    vmalloc (128 MiB, discontiguous allocations)
//!     0xFFFF_C900_0000_0000    Page table self-test area
//!     0xFFFF_CA00_0000_0000    Demand paging test area
//!     0xFFFF_FF00_0000_0000    Kernel text/data (Limine loads here)
//! ```
//!
//! ## Design
//!
//! All regions are defined as constants here.  Other modules import their
//! base addresses from this module rather than hardcoding magic numbers.
//! The `validate()` function checks for overlaps at boot.

use crate::serial_println;

// ---------------------------------------------------------------------------
// Region definitions
// ---------------------------------------------------------------------------

/// A named kernel virtual address region.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    /// Human-readable name.
    pub name: &'static str,
    /// Start address (inclusive).
    pub start: u64,
    /// Size in bytes.
    pub size: u64,
}

impl Region {
    /// End address (exclusive).
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.start + self.size
    }

    /// Check if an address falls within this region.
    #[must_use]
    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.start + self.size
    }
}

// ---------------------------------------------------------------------------
// Kernel-space regions
// ---------------------------------------------------------------------------

/// Kernel stack region.
/// Each task gets a 32 KiB stack + 16 KiB guard page = 48 KiB per task.
/// 256 MiB supports ~5400 tasks.
pub const KSTACK: Region = Region {
    name: "kstack",
    start: 0xFFFF_C100_0000_0000,
    size: 256 * 1024 * 1024, // 256 MiB
};

/// Huge page (2 MiB) mapping region.
pub const HUGEPAGE: Region = Region {
    name: "hugepage",
    start: 0xFFFF_C200_0000_0000,
    size: 1024 * 1024 * 1024, // 1 GiB
};

/// vmalloc region (virtually-contiguous, physically-discontiguous).
pub const VMALLOC: Region = Region {
    name: "vmalloc",
    start: 0xFFFF_C300_0000_0000,
    size: 128 * 1024 * 1024, // 128 MiB
};

/// Page table self-test area (temporary mappings during tests).
pub const PT_SELFTEST: Region = Region {
    name: "pt_selftest",
    start: 0xFFFF_C900_0000_0000,
    size: 16 * 1024 * 1024, // 16 MiB
};

/// Demand paging test area.
pub const FAULT_TEST: Region = Region {
    name: "fault_test",
    start: 0xFFFF_CA00_0000_0000,
    size: 16 * 1024 * 1024, // 16 MiB
};

/// User-space range.
pub const USER_SPACE: Region = Region {
    name: "user",
    start: 0x0000_0000_0000_0000,
    size: 0x0000_8000_0000_0000, // 128 TiB
};

/// ELF load base (within user space).
pub const USER_ELF_BASE: u64 = 0x0000_0000_0040_0000;

/// User mmap region base.
pub const USER_MMAP_BASE: u64 = 0x0000_0060_0000_0000;

/// User stack top (grows downward).
pub const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_0000;

// ---------------------------------------------------------------------------
// All kernel regions (for overlap checking)
// ---------------------------------------------------------------------------

/// All kernel virtual address regions (excluding HHDM which is dynamic).
const ALL_REGIONS: &[Region] = &[
    KSTACK,
    HUGEPAGE,
    VMALLOC,
    PT_SELFTEST,
    FAULT_TEST,
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Validate that no kernel regions overlap.
///
/// Call once at boot to catch configuration errors.
/// Panics if overlaps are detected.
pub fn validate() {
    for i in 0..ALL_REGIONS.len() {
        for j in (i + 1)..ALL_REGIONS.len() {
            let a = &ALL_REGIONS[i];
            let b = &ALL_REGIONS[j];
            // Two regions overlap if: a.start < b.end && b.start < a.end
            if a.start < b.end() && b.start < a.end() {
                serial_println!(
                    "FATAL: kernel VA regions overlap: {} [{:#x}..{:#x}] vs {} [{:#x}..{:#x}]",
                    a.name, a.start, a.end(),
                    b.name, b.start, b.end()
                );
                panic!("kernel VA region overlap detected");
            }
        }
    }
}

/// Look up which kernel region (if any) contains the given address.
///
/// Returns `None` for addresses in user space, the canonical hole, or
/// unmapped kernel space.
#[must_use]
pub fn identify(addr: u64) -> Option<&'static Region> {
    ALL_REGIONS.iter().find(|&region| region.contains(addr)).map(|v| v as _)
}

/// Check if an address is in kernel space (above the canonical hole).
#[inline]
#[must_use]
pub const fn is_kernel(addr: u64) -> bool {
    addr >= 0xFFFF_8000_0000_0000
}

/// Check if an address is in user space (below the canonical hole).
#[inline]
#[must_use]
pub const fn is_user(addr: u64) -> bool {
    addr < 0x0000_8000_0000_0000
}

/// Get all defined regions (for kshell display).
#[must_use]
pub fn all_regions() -> &'static [Region] {
    ALL_REGIONS
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the kernel virtual address space layout.
pub fn self_test() {
    serial_println!("[kvspace] Running self-test...");

    // Test 1: No overlaps.
    validate();
    serial_println!("[kvspace]   No region overlaps: OK");

    // Test 2: Identify known addresses.
    let id = identify(KSTACK.start);
    assert!(id.is_some());
    assert_eq!(id.unwrap().name, "kstack");

    let id = identify(VMALLOC.start + 1024);
    assert!(id.is_some());
    assert_eq!(id.unwrap().name, "vmalloc");
    serial_println!("[kvspace]   identify(): OK");

    // Test 3: Unknown address.
    let id = identify(0xFFFF_D000_0000_0000);
    assert!(id.is_none());
    serial_println!("[kvspace]   Unknown addr → None: OK");

    // Test 4: is_kernel / is_user.
    assert!(is_kernel(0xFFFF_8000_0000_0000));
    assert!(is_kernel(0xFFFF_FFFF_FFFF_FFFF));
    assert!(!is_kernel(0x0000_0000_0040_0000));
    assert!(is_user(0x0000_0000_0040_0000));
    assert!(!is_user(0xFFFF_8000_0000_0000));
    serial_println!("[kvspace]   is_kernel/is_user: OK");

    // Test 5: Region contains.
    assert!(VMALLOC.contains(VMALLOC.start));
    assert!(VMALLOC.contains(VMALLOC.start + VMALLOC.size - 1));
    assert!(!VMALLOC.contains(VMALLOC.start + VMALLOC.size)); // Exclusive end.
    serial_println!("[kvspace]   Region::contains: OK");

    serial_println!("[kvspace] Self-test PASSED");
}

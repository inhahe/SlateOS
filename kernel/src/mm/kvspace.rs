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
//!     0xFFFF_D000_0000_0000    KASAN shadow (16 TiB, 1:8 over all kernel VA)
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

/// Compile-time constant added to `addr >> 3` to obtain a KASAN shadow address.
///
/// This is the *whole* KASAN address mapping:
///
/// ```text
/// shadow(addr) = (addr >> 3) + KASAN_SHADOW_OFFSET      (wrapping, 64-bit)
/// ```
///
/// It is derived so that the lowest kernel address maps to the base of
/// [`KASAN_SHADOW`]:
///
/// ```text
/// (0xFFFF_8000_0000_0000 >> 3) = 0x1FFF_F000_0000_0000
/// 0xFFFF_D000_0000_0000 - 0x1FFF_F000_0000_0000 = 0xDFFF_E000_0000_0000
/// ```
///
/// **This value is not ours alone to choose.** It is passed verbatim to LLVM as
/// `-Cllvm-args=-asan-mapping-offset=0xDFFFE00000000000` in the compiler-KASAN
/// build profile, and the instrumentation LLVM emits inline at every load/store
/// is literally `shr $3; add $offset; movzbl (…)`. The manual poison written by
/// the heap hooks in `mm::kasan` and the checks the compiler emits therefore
/// address the *same* shadow bytes — that unification is the entire point of
/// using this mapping rather than an HHDM-relative one.
///
/// Note that LLVM's default mapping is `(addr >> 3) | 0x100000000000` — an
/// **OR**, which is only correct while `addr < 2^47` and so cannot express a
/// higher-half kernel address at all. Supplying an explicit offset switches the
/// codegen to an **ADD** (verified by disassembling an instrumented object).
/// Linux does exactly the same thing with `0xDFFFFC0000000000`.
pub const KASAN_SHADOW_OFFSET: u64 = 0xDFFF_E000_0000_0000;

/// KASAN shadow region — 1:8 scale over the *entire* 128 TiB kernel half.
///
/// 16 TiB of VA is reserved because the mapping above is a pure function of the
/// address, so every kernel address — HHDM heap, kernel stacks, vmalloc, kernel
/// text — has a shadow byte at a fixed place. The reservation is virtual-only:
/// `mm::kasan` backs shadow pages on first touch, and only for the low window it
/// actually tracks (the HHDM heap), so the cost of the extra VA is zero.
///
/// The region deliberately ends exactly where 0xFFFF_E000_0000_0000 begins and
/// sits above `fault_test`, so it overlaps nothing. Kernel text (0xFFFF_FF00_…)
/// is ~30 TiB above the end.
pub const KASAN_SHADOW: Region = Region {
    name: "kasan_shadow",
    start: 0xFFFF_D000_0000_0000,
    size: 0x0000_1000_0000_0000, // 16 TiB
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
    KASAN_SHADOW,
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

    // Test 3: Unknown address. (0xFFFF_C000_… sits below `kstack` and above the
    // HHDM, in a gap no region claims.)
    let id = identify(0xFFFF_C000_0000_0000);
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

    // Test 6: the KASAN shadow mapping must land inside the reserved region for
    // every kernel address, and must not wrap. This constant is compiled into
    // LLVM's instrumentation (`-asan-mapping-offset`), so a silent drift between
    // it and the region would put the compiler's inline checks on unmapped
    // memory — a triple fault at the first instrumented access, with no clue as
    // to why. Pin both ends of the kernel half.
    let shadow_of = |addr: u64| (addr >> 3).wrapping_add(KASAN_SHADOW_OFFSET);
    assert_eq!(shadow_of(0xFFFF_8000_0000_0000), KASAN_SHADOW.start);
    assert_eq!(shadow_of(0xFFFF_FFFF_FFFF_FFFF), KASAN_SHADOW.end().wrapping_sub(1));
    assert!(KASAN_SHADOW.contains(shadow_of(KASAN_SHADOW.start)));
    serial_println!("[kvspace]   KASAN shadow mapping covers kernel VA: OK");

    serial_println!("[kvspace] Self-test PASSED");
}

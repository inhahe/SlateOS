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

/// First PML4 slot covering [`KASAN_SHADOW`].
pub const KASAN_SHADOW_PML4_FIRST: usize = ((KASAN_SHADOW.start >> 39) & 0x1FF) as usize;
/// One past the last PML4 slot covering [`KASAN_SHADOW`].
pub const KASAN_SHADOW_PML4_END: usize =
    KASAN_SHADOW_PML4_FIRST + (KASAN_SHADOW.size / (512 * 1024 * 1024 * 1024)) as usize;

/// Whether `pml4_slot` belongs to the KASAN shadow reservation.
///
/// **Any walker that enumerates the whole kernel half of an address space must
/// consult this and skip.** The shadow is the one region that deliberately
/// *aliases* a handful of page tables across an enormous virtual range: a
/// single read-only zero page is mapped by one page table, which one page
/// directory repeats 512 times, which the PDPTs repeat again — so the 32 PML4
/// slots below expand to roughly 4×10⁹ "mapped" entries that all name the same
/// physical page. Descending into them yields no information (every entry is
/// the same zero page) and, at a few hundred nanoseconds per entry, does not
/// finish in any practical time — it presents as a kernel hang.
///
/// See `mm::kasan::early_init` for how the aliasing is built and why.
#[must_use]
pub const fn is_kasan_shadow_pml4_slot(pml4_slot: usize) -> bool {
    pml4_slot >= KASAN_SHADOW_PML4_FIRST && pml4_slot < KASAN_SHADOW_PML4_END
}

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
// Overlap check — enforced at build time
// ---------------------------------------------------------------------------

/// Do two regions share any address?
///
/// Both are half-open — `[start, end)` — so they overlap iff each starts
/// strictly before the other ends. The strictness is the whole predicate:
/// with `<=` in place of `<`, two *adjacent* regions would report as
/// overlapping and the build below would never succeed; with the comparison
/// inverted, nothing would ever report and the build gate would certify
/// exactly the layouts it exists to reject. `self_test` pins both edges.
///
/// The size guards are not redundant. A zero-size region contains no address,
/// so it can share none, but `start < end` is false for it in *both*
/// directions and the two-comparison form reports it as overlapping whatever
/// it sits inside. This was not a hypothetical: the assertion in `self_test`
/// saying so failed on its first boot, which is the entire reason that test
/// covers the degenerate case rather than only the interesting ones. A
/// zero-size region is still a bug, but it is a different bug and gets its own
/// message below — "overlap" would send the reader looking for a second region
/// that is not the problem.
const fn overlaps(a: &Region, b: &Region) -> bool {
    a.size > 0 && b.size > 0 && a.start < b.end() && b.start < a.end()
}

/// The first zero-size region in [`ALL_REGIONS`], if any.
///
/// Its own check because a region with no addresses in it is always a mistake —
/// a typo'd size constant, or a subsystem whose region was added before its
/// extent was known — and because [`overlaps`] deliberately reports such a
/// region as clashing with nothing, so the overlap gate cannot catch it.
const fn first_empty() -> Option<usize> {
    let n = ALL_REGIONS.len();
    let mut i = 0;
    while i < n {
        #[allow(clippy::indexing_slicing)]
        let empty = ALL_REGIONS[i].size == 0;
        if empty {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The first pair of overlapping regions in [`ALL_REGIONS`], as indices into it.
///
/// Every region above is a compile-time constant, so this whole search is a
/// compile-time question — and the `const` item below asks it at compile time.
/// This is a `const fn` rather than a `const` so the pair of indices survives
/// for a caller that wants to report *which* regions clashed: a
/// const-evaluated `panic!` takes only a literal, so the assertion itself
/// cannot name them.
const fn first_overlap() -> Option<(usize, usize)> {
    let n = ALL_REGIONS.len();
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n {
            // `while` rather than `for`, and indexing rather than iteration,
            // because neither `for` nor `Iterator` is available in a const fn.
            // Both indices are bounded by `n` on the enclosing line, so they
            // cannot be out of range — and const evaluation would reject the
            // build outright rather than panic at runtime if they ever were.
            #[allow(clippy::indexing_slicing)]
            let (a, b) = (&ALL_REGIONS[i], &ALL_REGIONS[j]);
            if overlaps(a, b) {
                return Some((i, j));
            }
            j += 1;
        }
        i += 1;
    }
    None
}

// Two kernel VA regions overlap. Fix the constants in this file.
//
// This fires at **build** time. It replaces a `validate()` function whose doc
// comment said "call once at boot to catch configuration errors" — but no boot
// path ever called it. Its only caller was this module's own `self_test`, so
// the check ran when the self-test ran and never otherwise, and the module's
// blanket `allow(dead_code)` meant nothing pointed out the discrepancy.
//
// A guard over compile-time constants that must be *invoked* to work is a
// guard that can be silently dropped — by reordering boot, by a self-test that
// stops being run, by a caller that never existed. As an anonymous `const`
// item it is evaluated because it exists. There is no way to build a kernel
// whose regions overlap, and no boot is needed to find that out.
//
// The error points here rather than at the offending pair, because a
// const-evaluated `panic!` accepts only a string literal — no formatting, so
// no region names. `first_overlap` returns the indices into `ALL_REGIONS` for
// whoever wants them; in practice the pair is whichever region was just added
// or resized.
const _: () = assert!(
    first_overlap().is_none(),
    "kernel VA regions overlap - see ALL_REGIONS in kernel/src/mm/kvspace.rs"
);

// A kernel VA region has size 0. Checked separately from the overlap above
// because `overlaps` reports an empty region as clashing with nothing, so it
// would otherwise pass silently and hand out an address range of no addresses.
const _: () = assert!(
    first_empty().is_none(),
    "a kernel VA region has size 0 - see ALL_REGIONS in kernel/src/mm/kvspace.rs"
);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Look up which kernel region (if any) contains the given address.
///
/// Returns `None` for addresses in user space, the canonical hole, or
/// unmapped kernel space.
#[must_use]
pub fn identify(addr: u64) -> Option<&'static Region> {
    ALL_REGIONS
        .iter()
        .find(|&region| region.contains(addr))
        .map(|v| v as _)
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

    // Test 1: the overlap predicate the build gate rests on.
    //
    // That no *real* region overlaps is settled at build time by the `const`
    // assertion above, so re-asserting `first_overlap().is_none()` here would
    // be a tautology: it cannot fail in a kernel that exists to run it. What
    // can still be wrong is `overlaps` itself, and a gate with a broken
    // comparison passes everything — it would certify the very layouts it was
    // written to reject, silently and forever. So test the predicate against
    // cases with known answers, including both boundaries.
    const A: Region = Region { name: "a", start: 0x1000, size: 0x1000 };
    let case = |start: u64, size: u64| overlaps(&A, &Region { name: "x", start, size });
    assert!(!case(0x2000, 0x1000), "regions that merely abut must not overlap");
    assert!(!case(0x0000, 0x1000), "regions that merely abut from below must not overlap");
    assert!(case(0x1FFF, 0x1000), "a one-byte overlap at the top must be detected");
    assert!(case(0x0001, 0x1000), "a one-byte overlap at the bottom must be detected");
    assert!(case(0x1000, 0x1000), "an identical region must overlap");
    assert!(case(0x1400, 0x0100), "a fully contained region must overlap");
    // The degenerate case, which is here because it failed. `overlaps` was
    // originally the textbook two-comparison form, and that form calls an
    // empty region an overlap of whatever contains it — `start < end` is false
    // in both directions, so both comparisons pass. The first boot after this
    // assertion was written panicked on it.
    assert!(!case(0x1400, 0x0000), "an empty region overlaps nothing");
    assert!(!case(0x1000, 0x0000), "an empty region at a shared start overlaps nothing");
    {
        const EMPTY: Region = Region { name: "e", start: 0x1400, size: 0 };
        assert!(!overlaps(&EMPTY, &A), "emptiness must be checked on both sides");
    }
    // Zero-size regions are caught by their own build assertion, since the
    // line above means the overlap gate will never see them.
    assert!(first_empty().is_none());
    assert!(first_overlap().is_none());
    serial_println!("[kvspace]   No region overlaps or empties (build-enforced), predicate: OK");

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
    assert_eq!(
        shadow_of(0xFFFF_FFFF_FFFF_FFFF),
        KASAN_SHADOW.end().wrapping_sub(1)
    );
    assert!(KASAN_SHADOW.contains(shadow_of(KASAN_SHADOW.start)));
    serial_println!("[kvspace]   KASAN shadow mapping covers kernel VA: OK");

    serial_println!("[kvspace] Self-test PASSED");
}

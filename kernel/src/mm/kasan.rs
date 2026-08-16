//! Kernel Address Sanitizer (KASAN) — shadow-memory heap-corruption detector.
//!
//! This is a KASAN-style *shadow memory* system for the kernel heap, built to
//! finally root-cause the long-standing **B-KNULLJUMP** corruption (see
//! `known-issues.md`): an intermittent wild write into a *live* heap object
//! (symbolized to a scheduler `BTreeMap` node) at a Path-Z process
//! spawn/teardown boundary. The existing slab poison/redzone (`mm/poison.rs`,
//! `mm/heap.rs`) can't catch that class — it is neither a use-after-free of a
//! poisoned slot nor an adjacent in-slot redzone overflow. Shadow memory marks
//! every heap byte as addressable or poisoned, so a KASAN-checked access to a
//! freed/redzone region is caught **at the access**, not later at a victim's
//! crash.
//!
//! ## Shadow encoding (generic-KASAN compatible)
//!
//! One shadow byte covers `KASAN_GRANULE` (8) heap bytes:
//! - `0x00`      — all 8 bytes addressable.
//! - `0x01..=07` — only the first N bytes of the granule are addressable
//!   (an object that ends mid-granule; the tail is poisoned).
//! - `0xFA`      — freed heap (use-after-free).
//! - `0xFB`      — heap redzone (slab slot padding past the requested size).
//! - `0xFF`      — the shadow default for never-tracked heap (treated as
//!   *addressable* so we never false-positive on memory KASAN didn't stamp;
//!   only regions we explicitly poison are flagged).
//!
//! Note: a freshly *mapped* shadow page is zeroed (`0x00` = addressable), so
//! untracked heap reads clean. KASAN only reports accesses into regions it has
//! explicitly marked `FREE`/`REDZONE`/partial. This is the conservative choice
//! (no false positives) and is exactly what we need to catch a checked access
//! to a freed scheduler node.
//!
//! ## Address mapping
//!
//! The shadow address is a pure, HHDM-independent function of the address:
//!
//! ```text
//! shadow(addr) = (addr >> 3) + kvspace::KASAN_SHADOW_OFFSET    (wrapping)
//! ```
//!
//! which maps the whole 128 TiB kernel half onto the 16 TiB
//! `kvspace::KASAN_SHADOW` reservation, with the lowest kernel address
//! (`0xFFFF_8000_0000_0000`) landing exactly on the region base.
//!
//! **Why this shape and not an HHDM-relative one.** It is the mapping LLVM's
//! address sanitizer emits inline when given
//! `-Cllvm-args=-asan-mapping-offset=0xDFFFE00000000000` (design-decisions.md
//! §107, the compiler-instrumented KASAN build). Because the compiler cannot be
//! taught a runtime-discovered HHDM base, the *shadow* must be the thing that is
//! address-derived — and then the poison this module writes on every heap
//! alloc/free is read by the compiler's checks on every load and store, with no
//! translation layer between them. Using two different mappings would give two
//! disjoint shadows and defeat the purpose.
//!
//! Since the heap only ever lives in the HHDM, this module *backs* only the
//! first [`KASAN_SHADOW_SIZE`] of that reservation — enough shadow for
//! [`KASAN_COVER_BYTES`] of kernel VA starting at `0xFFFF_8000_0000_0000`.
//! Addresses beyond it have no shadow byte here and fail open. Pages within the
//! window are **lazily mapped** on first touch (the heap only ever uses a
//! fraction of RAM, so eagerly backing `phys_ram / 8` would waste hundreds of
//! MiB).
//!
//! ## Performance / gating
//!
//! KASAN is **runtime-gated** by [`ENABLED`] (default `false`) so a normal
//! build pays only one relaxed atomic load per alloc/free — matching the
//! existing `POISON_ENABLED` fast-path pattern and protecting the <200 ns heap
//! target. It is turned on around the boot self-test and can be enabled for a
//! focused corruption hunt.
//!
//! ## References
//!
//! - Linux `mm/kasan/` — generic KASAN shadow memory + `KASAN_SHADOW_SCALE_SHIFT`.
//! - Documentation/dev-tools/kasan.rst — shadow encoding (`0xFA`/`0xFB`/…).

// KASAN debug profile: this module is exempt from compiler instrumentation.
// This *is* the shadow machinery: every check LLVM emits calls into it, so
// instrumenting it would recurse without bound.
// (`sanitize` is nightly-only, so it is gated on the `kasan_instrumented` cfg
// that `scripts/kasan-build.sh` sets; the ordinary build never sees it.)
#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]
// Diagnostic subsystem: much of the public API is tooling/shim surface that
// may not have production call sites yet (the scheduler-path checked-store shim
// lands in a follow-up). The shadow-index arithmetic is checked by the
// KASAN_COVER bound; per-operation checked math would obscure the hot path.
// The boot self-test intentionally uses assert/expect/unwrap on known-good
// values to fail loudly on a broken shadow map (same pattern as poison.rs).
#![allow(
    dead_code,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic
)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::kvspace;
use crate::mm::page_table::{self, HW_PAGES_PER_FRAME};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Bytes of heap covered by one shadow byte (`1 << KASAN_GRANULE_SHIFT`).
const KASAN_GRANULE_SHIFT: u64 = 3;
/// Heap-granule size in bytes (8).
const KASAN_GRANULE: u64 = 1 << KASAN_GRANULE_SHIFT;
/// Low-order bits of an in-granule offset.
const KASAN_GRANULE_MASK: u64 = KASAN_GRANULE - 1;

/// Base of the shadow region (see `kvspace::KASAN_SHADOW`).
const KASAN_SHADOW_BASE: u64 = kvspace::KASAN_SHADOW.start;
/// Constant added to `addr >> 3` to obtain the shadow address.
///
/// Shared with the compiler-KASAN build profile, which passes the identical
/// value to LLVM as `-asan-mapping-offset` — see `kvspace::KASAN_SHADOW_OFFSET`.
const KASAN_SHADOW_OFFSET: u64 = kvspace::KASAN_SHADOW_OFFSET;
/// Size of the shadow window this module actually backs (8 GiB → covers the
/// first 64 GiB of kernel VA, i.e. the whole HHDM on any realistic machine).
///
/// Must comfortably exceed the machine's physical RAM: the HHDM heap can back
/// a slab slot with any physical frame, and an object whose HHDM offset lands
/// past `KASAN_COVER_BYTES` has no shadow byte (`shadow_of` → `None`), so its
/// poison is silently dropped and every access to it fails *open* (unchecked).
/// A 4 GiB cover previously left a blind spot on the 5 GiB test machine: once
/// heap churn pushed allocations above 4 GiB, redzones stopped being poisoned —
/// which both broke the self-test intermittently and made the corruption hunt
/// blind to any stomp in high memory. 64 GiB covers every realistic dev/QEMU
/// config; the shadow is lazily mapped so the reservation is virtual-only until
/// heap is actually touched at a given offset.
///
/// This is a *backing* limit, not an addressing limit: the mapping itself
/// (`kvspace::KASAN_SHADOW_OFFSET`) spans the entire kernel half, and the
/// 16 TiB `kvspace::KASAN_SHADOW` reservation has room for all of it when the
/// compiler-KASAN profile needs whole-VA coverage.
const KASAN_SHADOW_SIZE: u64 = 8 * 1024 * 1024 * 1024;
/// Kernel-VA span the backed shadow can describe
/// (`KASAN_SHADOW_SIZE * KASAN_GRANULE`), starting at `0xFFFF_8000_0000_0000`.
const KASAN_COVER_BYTES: u64 = KASAN_SHADOW_SIZE << KASAN_GRANULE_SHIFT;

/// Number of 16 KiB shadow frames in the reserved range.
const SHADOW_FRAMES: usize = (KASAN_SHADOW_SIZE / FRAME_SIZE as u64) as usize;
/// `u64` words needed for the mapped-frame bitmap (`SHADOW_FRAMES / 64`).
const MAP_BITMAP_WORDS: usize = SHADOW_FRAMES.div_ceil(64);

// Shadow encoding values.
/// All 8 bytes of the granule are addressable.
const KASAN_ADDRESSABLE: u8 = 0x00;
/// Freed heap (use-after-free).
const KASAN_FREE: u8 = 0xFA;
/// Heap redzone (slab padding past the requested size).
const KASAN_REDZONE: u8 = 0xFB;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether KASAN checking + shadow maintenance is active.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Whether [`init`] has run (shadow base/hhdm are valid).
static INITED: AtomicBool = AtomicBool::new(false);
/// HHDM offset captured at init (base of the covered heap range).
static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Bitmap of which shadow frames are physically mapped. A `1` bit means the
/// corresponding 16 KiB shadow frame is backed and zeroed.
static SHADOW_MAPPED: [AtomicU64; MAP_BITMAP_WORDS] =
    [const { AtomicU64::new(0) }; MAP_BITMAP_WORDS];

/// Serializes lazy shadow-frame mapping. Held with interrupts disabled so an
/// IRQ-context allocation cannot re-enter and self-deadlock on the same CPU.
static MAP_LOCK: spin::Mutex<()> = spin::Mutex::new(());

/// Run `f` holding [`MAP_LOCK`] with interrupts disabled, without ever *waiting*
/// for the lock with interrupts disabled. Returns `None` if the lock was busy
/// and this caller could not safely wait for it.
///
/// The distinction matters because the mapping path performs a cross-CPU TLB
/// shootdown while holding the lock, and the shootdown blocks until every other
/// CPU acknowledges an IPI. A CPU spinning on `MAP_LOCK.lock()` with interrupts
/// disabled can never take that IPI, so the holder would wait for an
/// acknowledgement that can never come — a two-CPU deadlock.
///
/// So the lock is only ever acquired with `try_lock`:
///
/// * If the caller arrived with interrupts enabled, a failed attempt re-enables
///   them before retrying, which lets the shootdown IPI through and guarantees
///   forward progress.
/// * If the caller arrived with interrupts already disabled (an IRQ-context
///   allocation), re-enabling them is not ours to do, so a failed attempt gives
///   up. The caller then simply leaves that shadow byte unwritten, which loses
///   detection coverage for one allocation but is never a correctness problem —
///   an unpoisoned shadow byte reads as "addressable" and fails open.
fn with_map_lock<R>(f: impl FnOnce() -> R) -> Option<R> {
    let irqs_were_on = crate::cpu::interrupts_enabled();
    loop {
        // SAFETY: masking interrupts is always permitted in ring 0; we restore
        // the caller's flag state on every exit path below.
        unsafe { crate::cpu::cli() };
        if let Some(guard) = MAP_LOCK.try_lock() {
            let out = f();
            drop(guard);
            if irqs_were_on {
                // SAFETY: restoring the interrupt flag the caller arrived with.
                unsafe { crate::cpu::sti() };
            }
            return Some(out);
        }
        if !irqs_were_on {
            MAP_LOCK_GIVEUPS.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        // SAFETY: as above — the caller had interrupts enabled, so re-enabling
        // them between attempts merely restores their state, and it is what
        // lets an incoming TLB-shootdown IPI be serviced while we wait.
        unsafe { crate::cpu::sti() };
        core::hint::spin_loop();
    }
}

/// Physical address of the all-zero shadow page, or 0 before [`early_init`].
static ZERO_PAGE_PHYS: AtomicU64 = AtomicU64::new(0);
/// Physical address of the shared, read-only zero-shadow page table.
static ZERO_PT_PHYS: AtomicU64 = AtomicU64::new(0);
/// Physical address of the shared page directory (every entry → `ZERO_PT`).
static ZERO_PD_PHYS: AtomicU64 = AtomicU64::new(0);
/// Physical address of `SHADOW_PDPT0`, the private PDPT covering the backed
/// window. Written last by [`early_init`], so a non-zero value here is the
/// "the zero shadow is installed and usable" flag for the whole module.
static PDPT0_PHYS: AtomicU64 = AtomicU64::new(0);

/// Set to 1 by [`early_init`] once the zero shadow is live.
///
/// Duplicates what a non-zero `PDPT0_PHYS` already means, and exists only
/// because `early_init`'s own idempotency check runs *before* any shadow is
/// mapped: `AtomicU64::load` is a generic `core` function, so it is compiled
/// into this crate *with* instrumentation and would probe unmapped shadow.
/// A plain `static mut` can be read with a raw `mov` that stays inside this
/// exempt module. Only ever touched on the BSP with no other CPU running, so
/// the absence of atomicity is not a race.
static mut EARLY_SHADOW_DONE: u64 = 0;

// Statistics.
static VIOLATIONS: AtomicU64 = AtomicU64::new(0);
static SHADOW_FRAMES_MAPPED: AtomicU64 = AtomicU64::new(0);
/// Times [`with_map_lock`] gave up because an IRQ-context caller found the lock
/// busy, each one an allocation whose shadow was never written.
///
/// This gap is otherwise completely invisible: it fails open, so nothing
/// downstream can tell "not poisoned" from "poison never recorded". Counting it
/// is what makes the trade in
/// `known-issues.md` → `TD-KASAN-IRQ-CONTEXT-ALLOCATIONS-LOSE-SHADOW-COVERAGE`
/// decidable — whether it happens once a boot or never decides whether the
/// (risky) real fix is worth attempting at all — and it is the only way to tell
/// afterwards that a fix worked.
static MAP_LOCK_GIVEUPS: AtomicU64 = AtomicU64::new(0);
static BYTES_POISONED: AtomicU64 = AtomicU64::new(0);
static BYTES_UNPOISONED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Early zero shadow
// ---------------------------------------------------------------------------
//
// The compiler-instrumented build checks a shadow byte before *every* load and
// store, from the first instruction of instrumented code onwards. That creates
// a bootstrap problem with no gentle failure mode: an access whose shadow page
// is not mapped faults, and a fault before the IDT exists is a triple fault and
// a silent reboot.
//
// The classic answer (Linux's `kasan_early_init`) is to make the *entire*
// shadow readable from the very start by pointing all of it at one shared,
// read-only, all-zero page — zero meaning "addressable", so every check passes
// until something poisons a byte for real. Because every level of the paging
// hierarchy is shared as well, the whole 16 TiB costs five 4 KiB pages:
//
//   PML4[416]        ─→ SHADOW_PDPT0   (512 entries, all →) ZERO_PD ─┐
//   PML4[417..448]   ─→ ZERO_PDPT      (512 entries, all →) ZERO_PD ─┤
//                                                                     │
//                                ZERO_PD (512 entries, all →) ZERO_PT ┘
//                                ZERO_PT (512 entries, all →) ZERO_SHADOW_PAGE (RO)
//
// They are `static`s in `.bss` rather than allocations because this must be
// able to run before the frame allocator exists — the only inputs are CR3 and
// the HHDM offset.
//
// Installed unconditionally, not just in the instrumented profile. Twenty KiB
// and 32 PML4 entries is a negligible price for having the path exercised by
// every boot test, instead of first running in the build where something is
// already going wrong.
//
// ## Why the first PDPT is private
//
// Real (writable) shadow pages get spliced into this hierarchy lazily, one
// 16 KiB frame at a time, by replacing shared tables with private copies on the
// path down — see [`install_shadow_frame`]. Those edits must be visible in
// *every* address space, and that constrains which level they may start at:
// `page_table::alloc_pml4` copies the kernel's PML4 entries **by value** into
// each new process PML4, so a PML4 entry rewritten after boot would be seen
// only by whichever address space happened to be active. Everything *below* the
// PML4 is reached by pointer and therefore shared by construction.
//
// So the PML4 entries are written exactly once, in [`early_init`], and never
// again. To make that possible the slot that covers the backed window gets its
// own private PDPT (`SHADOW_PDPT0`) instead of the shared one, so that writing
// a PDPT entry affects only that slot. `SHADOW_PML4_ONLY_SLOT` below asserts
// that the backed window really does fit in one 512 GiB slot.

/// One 4 KiB hardware page holding 512 page-table entries.
///
/// (Note the kernel's `FRAME_SIZE` is 16 KiB, but paging structures are
/// hardware-defined 4 KiB objects regardless.)
#[repr(C, align(4096))]
struct PtPage([u64; 512]);

/// The single all-zero page that every unpoisoned shadow address resolves to.
/// Mapped read-only 2^31 times over; nothing may ever write through it.
static mut ZERO_SHADOW_PAGE: PtPage = PtPage([0; 512]);
/// Shared page table: all 512 entries map [`ZERO_SHADOW_PAGE`] read-only.
static mut ZERO_SHADOW_PT: PtPage = PtPage([0; 512]);
/// Shared page directory: all 512 entries point at [`ZERO_SHADOW_PT`].
static mut ZERO_SHADOW_PD: PtPage = PtPage([0; 512]);
/// Shared PDPT for the shadow PML4 slots that are never backed by real pages
/// (everything above the first 512 GiB of shadow). All entries → `ZERO_SHADOW_PD`.
static mut ZERO_SHADOW_PDPT: PtPage = PtPage([0; 512]);
/// Private PDPT for the *first* shadow PML4 slot — the 512 GiB that contains
/// the whole backed window. Entries start out pointing at `ZERO_SHADOW_PD` and
/// are swapped for private page directories as shadow is really backed.
static mut SHADOW_PDPT0: PtPage = PtPage([0; 512]);

/// Present bit in a page-table entry.
const PTE_PRESENT: u64 = 1 << 0;
/// Writable bit.
const PTE_WRITABLE: u64 = 1 << 1;
/// Page-size bit (a huge page at PDPT/PD level).
const PTE_HUGE: u64 = 1 << 7;
/// No-execute bit.
const PTE_NX: u64 = 1 << 63;
/// Physical-address field of a page-table entry.
const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

// ---------------------------------------------------------------------------
// Raw memory access for the pre-shadow window
// ---------------------------------------------------------------------------
//
// Everything `early_init` executes runs *before* any shadow exists, so it must
// not perform a single instrumented memory access: with no shadow mapped and no
// IDT installed yet, one would be a triple fault and a silent reboot rather
// than a diagnosable report.
//
// Marking this module `sanitize(address = "off")` is not sufficient by itself.
// The exemption is a per-function LLVM attribute, and it only covers functions
// that are *ours*. Generic `core` functions — `ptr::read_volatile`,
// `AtomicU64::load`, and friends — monomorphise into **this** crate's codegen
// units, are emitted as real out-of-line calls at `-O0`, and carry the default
// (instrumented) attribute. They dereference the pointer we hand them, so the
// check happens in `core`'s frame where our exemption has no reach. That is
// exactly how the first instrumented boot died: `serial::init` took a spinlock,
// which called `core::sync::atomic::atomic_compare_exchange_weak::<u8>`, which
// probed the not-yet-mapped shadow of `serial::SERIAL`.
//
// So the pre-shadow window issues its loads and stores itself, via `asm!`.
// Being inline assembly it is also opaque to LLVM and cannot be elided or
// reordered, which is the property `read_volatile` was being used for.
//
// The same primitives serve a second caller with the same requirement for a
// different reason: the *shadow lookup itself* (`get_shadow` and everything
// under `shadow_allows`). The build uses outlined instrumentation, so every
// checked access is a call to `mm::kasan_rt::__asan_load*`, which calls
// `shadow_allows`. If the lookup performed an instrumented access of its own it
// would call `__asan_load*` again, and so on without bound — a stack overflow
// with no explanation, in the one build where you are already debugging
// something else. `scripts/kasan-check-preshadow.py --runtime` enforces this.

/// Load a `u64` from `addr` without letting the compiler instrument the access.
///
/// Exposed crate-wide so the handful of other call sites that also run in the
/// pre-shadow window (see `limine::LimineRequest::<HhdmResponse>::offset_raw`)
/// can share one audited implementation instead of open-coding `asm!`.
///
/// # Safety
///
/// `addr` must be mapped, readable and 8-byte aligned.
#[inline]
pub(crate) unsafe fn raw_load_u64(addr: u64) -> u64 {
    let value: u64;
    // SAFETY: the caller guarantees `addr` is a mapped, aligned, readable u64.
    unsafe {
        core::arch::asm!(
            "mov {value}, qword ptr [{addr}]",
            addr = in(reg) addr,
            value = out(reg) value,
            options(nostack, preserves_flags, readonly),
        );
    }
    value
}

/// Load a `u8` from `addr` without letting the compiler instrument the access.
///
/// The shadow-byte read in [`get_shadow`] uses this: that read is on the path
/// LLVM's outlined check calls into, so instrumenting it would make every
/// checked access recurse.
///
/// # Safety
///
/// `addr` must be mapped and readable.
#[inline]
unsafe fn raw_load_u8(addr: u64) -> u8 {
    let value: u8;
    // SAFETY: the caller guarantees `addr` is a mapped, readable byte.
    unsafe {
        core::arch::asm!(
            "mov {value}, byte ptr [{addr}]",
            addr = in(reg) addr,
            value = out(reg_byte) value,
            options(nostack, preserves_flags, readonly),
        );
    }
    value
}

/// Store a `u64` to `addr` without letting the compiler instrument the access.
///
/// # Safety
///
/// `addr` must be mapped, writable, 8-byte aligned, and exclusively owned by
/// the caller.
#[inline]
unsafe fn raw_store_u64(addr: u64, value: u64) {
    // SAFETY: the caller guarantees `addr` is a mapped, aligned, writable u64
    // that nothing else is concurrently touching.
    unsafe {
        core::arch::asm!(
            "mov qword ptr [{addr}], {value}",
            addr = in(reg) addr,
            value = in(reg) value,
            options(nostack, preserves_flags),
        );
    }
}

/// `value >> shift`, emitted directly, with no `core` helper in between.
///
/// A plain `>>` by a non-constant amount is not usable in the pre-shadow
/// window. In a debug build the checked shift branches to
/// `core::panicking::panic_const_shr_overflow`, and `wrapping_shr` is no better
/// — it forwards to `core::num::<u64>::unchecked_shr`, whose `ub_checks`
/// precondition check is itself a `core` generic that monomorphises into this
/// crate *with* instrumentation. Either one puts a shadow probe in a frame our
/// `sanitize(address = "off")` cannot reach, which before the shadow exists is
/// a triple fault with no serial output.
///
/// Doing the shift in `asm!` keeps it inside this exempt module. x86 masks the
/// count in `cl` to 6 bits, so this has `wrapping_shr` semantics; every caller
/// here passes a fixed page-table level shift (39/30/21/12), well under 64.
#[inline]
fn raw_shr_u64(value: u64, shift: u32) -> u64 {
    let out: u64;
    // SAFETY: `shr r64, cl` touches no memory and is defined for any count.
    // `preserves_flags` is deliberately not claimed: `shr` writes the flags.
    unsafe {
        core::arch::asm!(
            "shr {out}, cl",
            out = inout(reg) value => out,
            in("cl") shift as u8,
            options(nomem, nostack),
        );
    }
    out
}

/// `value << shift`, emitted directly. The left-shift counterpart of
/// [`raw_shr_u64`], with the same rationale and the same masking semantics.
#[inline]
fn raw_shl_u64(value: u64, shift: u32) -> u64 {
    let out: u64;
    // SAFETY: `shl r64, cl` touches no memory and is defined for any count.
    // `preserves_flags` is deliberately not claimed: `shl` writes the flags.
    unsafe {
        core::arch::asm!(
            "shl {out}, cl",
            out = inout(reg) value => out,
            in("cl") shift as u8,
            options(nomem, nostack),
        );
    }
    out
}

/// Byte address of entry `index` in the 4 KiB paging structure at `table_phys`.
#[inline]
fn pt_entry_addr(table_phys: u64, index: usize, hhdm: u64) -> u64 {
    table_phys
        .wrapping_add(hhdm)
        .wrapping_add((index as u64).wrapping_mul(8))
}

/// Read entry `index` of the 4 KiB paging structure at `table_phys`.
///
/// # Safety
///
/// `table_phys` must be a live paging structure, `index < 512`, and `hhdm` the
/// correct direct-map offset.
#[inline]
unsafe fn pt_read(table_phys: u64, index: usize, hhdm: u64) -> u64 {
    // SAFETY: caller guarantees the table is live and mapped through the HHDM;
    // entries are 8 bytes and the structure is 4 KiB aligned, so the read is
    // aligned and in bounds for index < 512.
    unsafe { raw_load_u64(pt_entry_addr(table_phys, index, hhdm)) }
}

/// Write entry `index` of the 4 KiB paging structure at `table_phys`.
///
/// # Safety
///
/// As [`pt_read`], plus the caller must have exclusive access to the entry.
#[inline]
unsafe fn pt_write(table_phys: u64, index: usize, value: u64, hhdm: u64) {
    // SAFETY: as `pt_read`; the caller guarantees exclusivity.
    unsafe { raw_store_u64(pt_entry_addr(table_phys, index, hhdm), value) }
}

/// Translate a kernel virtual address using the live page tables.
///
/// A standalone walk rather than `page_table::translate` because this runs
/// *before* `page_table::init`, so the module's HHDM cell is not yet set.
///
/// # Safety
///
/// `pml4_phys` must be the live PML4 and `hhdm` the correct direct-map offset.
unsafe fn early_translate(pml4_phys: u64, hhdm: u64, va: u64) -> Option<u64> {
    // `raw_shr_u64` rather than `>>` — see its doc comment: both the checked
    // shift and `wrapping_shr` route through instrumented `core` generics that
    // would triple-fault here.
    let index = |shift: u32| (raw_shr_u64(va, shift) & 0x1FF) as usize;

    // SAFETY: each level is checked present before being used as the next
    // table's physical address, which is what makes the following read valid.
    unsafe {
        let e = pt_read(pml4_phys, index(39), hhdm);
        if e & PTE_PRESENT == 0 {
            return None;
        }
        let e = pt_read(e & PTE_ADDR_MASK, index(30), hhdm);
        if e & PTE_PRESENT == 0 {
            return None;
        }
        if e & PTE_HUGE != 0 {
            return Some((e & PTE_ADDR_MASK) | (va & 0x3FFF_FFFF));
        }
        let e = pt_read(e & PTE_ADDR_MASK, index(21), hhdm);
        if e & PTE_PRESENT == 0 {
            return None;
        }
        if e & PTE_HUGE != 0 {
            return Some((e & PTE_ADDR_MASK) | (va & 0x1F_FFFF));
        }
        let e = pt_read(e & PTE_ADDR_MASK, index(12), hhdm);
        if e & PTE_PRESENT == 0 {
            return None;
        }
        Some((e & PTE_ADDR_MASK) | (va & 0xFFF))
    }
}

/// Reload CR3 to flush the TLB after editing top-level paging structures.
///
/// `invlpg` cannot be used here: the mappings being added cover 16 TiB, and
/// there is nothing to invalidate per-page anyway — the entries were absent.
///
/// # Safety
///
/// Writing CR3 with the value just read from it is always safe in ring 0; it
/// changes no mapping, only flushes non-global TLB entries.
#[inline]
unsafe fn flush_tlb_all() {
    // SAFETY: read-then-write of CR3 with an unchanged value.
    unsafe {
        core::arch::asm!(
            "mov {tmp}, cr3",
            "mov cr3, {tmp}",
            tmp = out(reg) _,
            options(nostack, preserves_flags),
        );
    }
}

/// Physical addresses of the paging structures backing the zero shadow.
///
/// Produced by [`install_zero_shadow`] and handed to [`publish_shadow_roots`],
/// which is what keeps the two boot phases separable — see [`early_init`].
#[derive(Clone, Copy)]
struct ShadowRoots {
    /// The single all-zero page every shadow PTE initially points at.
    zero_page: u64,
    /// The shared, read-only zero-shadow page table.
    pt: u64,
    /// The shared page directory (every entry → `pt`).
    pd: u64,
    /// The private PDPT covering the lazily-backed window.
    pdpt0: u64,
}

/// Map the whole KASAN shadow reservation onto a shared read-only zero page.
///
/// This is **phase one**: the half of [`early_init`] that runs before the
/// shadow exists. Everything it calls, transitively, must be exempt from
/// instrumentation — `scripts/kasan-check-preshadow.py` walks the call graph
/// from here and fails the build otherwise. It is a separate function precisely
/// so that boundary is a thing the tooling can name, rather than a comment in
/// the middle of a function whose first half is pre-shadow and second half is
/// not.
///
/// Returns `None` if the kernel's own statics could not be translated.
///
/// # Safety
///
/// Must be called on the BSP, with interrupts disabled and no other CPU
/// running, since it edits the live PML4. `hhdm` must be the bootloader's
/// direct-map offset.
unsafe fn install_zero_shadow(hhdm: u64) -> Option<ShadowRoots> {
    // CR3's low 12 bits are flags (PCD/PWT/PCID), not part of the address.
    let cr3: u64;
    // SAFETY: reading CR3 is a plain register read, always valid in ring 0.
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    }
    let pml4_phys = cr3 & PTE_ADDR_MASK;

    // Physical addresses of our four .bss pages. `addr_of_mut!` rather than a
    // reference: taking `&mut` to a `static mut` that the paging hardware is
    // about to alias is exactly the kind of aliasing Rust forbids.
    let zero_page_va = core::ptr::addr_of_mut!(ZERO_SHADOW_PAGE) as u64;
    let pt_va = core::ptr::addr_of_mut!(ZERO_SHADOW_PT) as u64;
    let pd_va = core::ptr::addr_of_mut!(ZERO_SHADOW_PD) as u64;
    let pdpt_va = core::ptr::addr_of_mut!(ZERO_SHADOW_PDPT) as u64;
    let pdpt0_va = core::ptr::addr_of_mut!(SHADOW_PDPT0) as u64;

    // SAFETY: `pml4_phys` came from CR3 and `hhdm` is the bootloader's offset,
    // so the walk reads live, mapped paging structures.
    let (Some(zero_page_phys), Some(pt_phys), Some(pd_phys), Some(pdpt_phys), Some(pdpt0_phys)) =
        (unsafe {
            (
                early_translate(pml4_phys, hhdm, zero_page_va),
                early_translate(pml4_phys, hhdm, pt_va),
                early_translate(pml4_phys, hhdm, pd_va),
                early_translate(pml4_phys, hhdm, pdpt_va),
                early_translate(pml4_phys, hhdm, pdpt0_va),
            )
        })
    else {
        return None;
    };

    // Leaf entries: read-only and non-executable. Read-only is load-bearing —
    // one writable alias to this page would let a stray shadow *store* (e.g.
    // stack-redzone poisoning, were it ever enabled without real backing)
    // silently mark all of memory poisoned or all of it clean.
    let leaf = zero_page_phys | PTE_PRESENT | PTE_NX;
    let pde = pt_phys | PTE_PRESENT | PTE_WRITABLE;
    let pdpte = pd_phys | PTE_PRESENT | PTE_WRITABLE;

    // SAFETY: the five tables are our own 4 KiB-aligned statics, reached
    // through their HHDM aliases; nothing else refers to them yet, and no
    // other CPU is running. Intermediate levels are writable so that
    // `install_shadow_frame` can later split a branch off for real shadow
    // pages; the read-only leaf still makes the *pages* read-only, because
    // x86 ANDs the writable bit across all levels.
    //
    // `while` rather than `for`: a `for` over a `Range` hands `&mut Range` to
    // `core::iter::range::RangeIteratorImpl::spec_next`, and *there* the
    // counter is reached through a pointer parameter rather than the callee's
    // own `alloca`, so `-asan-stack=0` does not suppress the check and the
    // instrumented probe hits this frame's not-yet-mapped stack shadow. (That
    // was the second triple fault this bootstrap produced.) A plain counter
    // never lets its address escape this exempt function.
    unsafe {
        let mut i = 0usize;
        while i < 512 {
            pt_write(pt_phys, i, leaf, hhdm);
            pt_write(pd_phys, i, pde, hhdm);
            pt_write(pdpt_phys, i, pdpte, hhdm);
            pt_write(pdpt0_phys, i, pdpte, hhdm);
            i = i.saturating_add(1);
        }
        pt_write(
            pml4_phys,
            SHADOW_PML4_FIRST,
            pdpt0_phys | PTE_PRESENT | PTE_WRITABLE | PTE_NX,
            hhdm,
        );
        let mut slot = SHADOW_PML4_FIRST.saturating_add(1);
        while slot < SHADOW_PML4_END {
            pt_write(
                pml4_phys,
                slot,
                pdpt_phys | PTE_PRESENT | PTE_WRITABLE | PTE_NX,
                hhdm,
            );
            slot = slot.saturating_add(1);
        }
        flush_tlb_all();
        // The shadow is live from here on: every kernel address now has a
        // readable shadow byte, so ordinary instrumented code is safe again.
        raw_store_u64(core::ptr::addr_of!(EARLY_SHADOW_DONE) as u64, 1);
    }

    Some(ShadowRoots {
        zero_page: zero_page_phys,
        pt: pt_phys,
        pd: pd_phys,
        pdpt0: pdpt0_phys,
    })
}

/// Record the zero-shadow roots for the rest of the module to use.
///
/// **Phase two**: runs only after [`install_zero_shadow`] has succeeded, so
/// ordinary instrumented code — including `AtomicU64::store`, whose generic
/// `core` implementation *is* instrumented — is safe here. Kept separate from
/// phase one so `scripts/kasan-check-preshadow.py` can exclude it by name
/// instead of reporting these stores as pre-shadow violations.
fn publish_shadow_roots(roots: ShadowRoots) {
    ZERO_PAGE_PHYS.store(roots.zero_page, Ordering::Relaxed);
    ZERO_PT_PHYS.store(roots.pt, Ordering::Relaxed);
    ZERO_PD_PHYS.store(roots.pd, Ordering::Relaxed);
    // Written last, with Release: it is the flag the rest of the module tests.
    PDPT0_PHYS.store(roots.pdpt0, Ordering::Release);
}

/// Install the KASAN zero shadow over the whole kernel half.
///
/// Must be called as early in boot as the HHDM offset is known and *before*
/// any instrumented code runs. Requires no allocator. Idempotent.
///
/// Returns `false` if the kernel's own statics could not be translated, which
/// would mean the HHDM offset is wrong — in the instrumented build that is
/// fatal and the caller should say so loudly rather than continue into a
/// triple fault.
///
/// The body is deliberately just the two phases in order: everything reachable
/// from [`install_zero_shadow`] must be uninstrumented, and nothing reachable
/// from [`publish_shadow_roots`] needs to be.
///
/// # Safety
///
/// Must be called exactly once, on the BSP, with interrupts disabled and no
/// other CPU running, since it edits the live PML4.
pub unsafe fn early_init(hhdm: u64) -> bool {
    // Idempotency guard. A raw load rather than `PDPT0_PHYS.load()` because
    // `AtomicU64::load` is a generic `core` function and therefore instrumented
    // — see the "Raw memory access for the pre-shadow window" section above.
    // SAFETY: `EARLY_SHADOW_DONE` is our own aligned `static mut` in `.bss`, and
    // this runs on the BSP with no other CPU up.
    if unsafe { raw_load_u64(core::ptr::addr_of!(EARLY_SHADOW_DONE) as u64) } != 0 {
        return true; // already installed
    }

    // SAFETY: forwarding this function's own contract.
    let Some(roots) = (unsafe { install_zero_shadow(hhdm) }) else {
        return false;
    };
    publish_shadow_roots(roots);
    true
}

/// First PML4 slot covering the shadow reservation.
const SHADOW_PML4_FIRST: usize = kvspace::KASAN_SHADOW_PML4_FIRST;
/// One past the last PML4 slot covering the shadow reservation.
const SHADOW_PML4_END: usize = kvspace::KASAN_SHADOW_PML4_END;

// The shadow reservation must start on a PML4 (512 GiB) boundary, or
// `SHADOW_PML4_FIRST` would not describe it.
const _: () = assert!(
    KASAN_SHADOW_BASE & ((1u64 << 39) - 1) == 0,
    "KASAN shadow base must be 512 GiB aligned"
);
// The *backed* window must fit entirely in the first shadow PML4 slot. This is
// what lets `install_shadow_frame` start its walk at `SHADOW_PDPT0` and never
// touch a PML4 entry after boot — see the section comment above for why a
// post-boot PML4 edit would not be visible in every address space.
const _: () = assert!(
    KASAN_SHADOW_SIZE <= 1u64 << 39,
    "backed KASAN shadow window must fit in one PML4 slot"
);

/// Splice a real, writable 16 KiB shadow frame into the zero-shadow hierarchy.
///
/// Cannot go through `page_table::map_frame`: that refuses to overwrite a
/// present PTE (`AlreadyExists`), and in the shadow region *every* PTE is
/// already present — pointing at the shared read-only zero page. Unmapping
/// first is not an option either, since it would leave a window in which an
/// instrumented access faults instead of reading a benign zero. So the four
/// hardware PTEs are replaced in place, after copy-on-writing whichever shared
/// tables lie on the path.
///
/// Each shared table found on the way down is copied first, with the copy
/// pre-filled with the same shared child, so coverage is preserved exactly and
/// only the branch being written diverges. Because the walk starts at
/// `SHADOW_PDPT0` — a fixed static that every address space reaches through the
/// same by-value PML4 entry — every table it edits is shared by pointer, and
/// the new mapping is therefore visible in all address spaces at once.
///
/// TLB coherence: `invlpg` on a linear address invalidates both its TLB entry
/// and the paging-structure-cache entries used to translate it (Intel SDM Vol.
/// 3A §4.10.4.1), and those are exactly the PDPTE/PDE entries this function may
/// have rewritten — so a cross-CPU `invlpg` of the four pages is sufficient; no
/// full CR3 shootdown is needed.
///
/// Returns `false` if a table page could not be allocated (the shadow simply
/// stays unbacked there, which fails open) or if [`early_init`] never ran.
///
/// # Safety
///
/// `shadow_va` must be 16 KiB-aligned and inside the backed window, `phys` a
/// frame owned by the caller, and `hhdm` the live direct-map offset. The caller
/// must serialize against other mappers (`MAP_LOCK`).
unsafe fn install_shadow_frame(shadow_va: u64, phys: u64, hhdm: u64) -> bool {
    let pdpt0 = PDPT0_PHYS.load(Ordering::Acquire);
    if pdpt0 == 0 {
        return false; // early_init never ran; nothing to splice into
    }
    let shared_pd = ZERO_PD_PHYS.load(Ordering::Relaxed);
    let shared_pt = ZERO_PT_PHYS.load(Ordering::Relaxed);
    let zero_page = ZERO_PAGE_PHYS.load(Ordering::Relaxed);

    let idx = |shift: u32| ((shadow_va >> shift) & 0x1FF) as usize;

    // SAFETY: `pdpt0` is our own static, and every table below it was either
    // installed by `early_init` or freshly allocated here, so each read/write
    // targets a live 4 KiB paging structure reached through the HHDM.
    unsafe {
        // PDPT → PD.
        let mut pd = pt_read(pdpt0, idx(30), hhdm) & PTE_ADDR_MASK;
        if pd == shared_pd {
            let Ok(fresh) = page_table::alloc_pt_page() else {
                return false;
            };
            let fill = shared_pt | PTE_PRESENT | PTE_WRITABLE;
            for e in 0..512 {
                pt_write(fresh, e, fill, hhdm);
            }
            pt_write(pdpt0, idx(30), fresh | PTE_PRESENT | PTE_WRITABLE, hhdm);
            pd = fresh;
        }

        // PD → PT.
        let mut pt = pt_read(pd, idx(21), hhdm) & PTE_ADDR_MASK;
        if pt == shared_pt {
            let Ok(fresh) = page_table::alloc_pt_page() else {
                // The private PD allocated just above (if any) is retained: it
                // maps exactly what the shared one did, so leaving it in place
                // is correct, and the next call will reuse it.
                return false;
            };
            let fill = zero_page | PTE_PRESENT | PTE_NX;
            for e in 0..512 {
                pt_write(fresh, e, fill, hhdm);
            }
            pt_write(pd, idx(21), fresh | PTE_PRESENT | PTE_WRITABLE, hhdm);
            pt = fresh;
        }

        // Leaves: the four 4 KiB hardware pages of one 16 KiB kernel frame.
        // `shadow_va` is frame-aligned, so `idx(12)` is a multiple of 4 and
        // all four entries live in this one page table.
        let base = idx(12);
        for k in 0..HW_PAGES_PER_FRAME {
            let entry = phys.wrapping_add((k as u64) << 12) | PTE_PRESENT | PTE_WRITABLE | PTE_NX;
            pt_write(pt, base.wrapping_add(k), entry, hhdm);
        }
    }

    // Cross-CPU invalidation of the four pages, which also drops any stale
    // paging-structure-cache entries for them (see the doc comment).
    crate::tlb::flush_range(shadow_va, HW_PAGES_PER_FRAME as u32);
    true
}

// ---------------------------------------------------------------------------
// Init / enable
// ---------------------------------------------------------------------------

/// Initialize KASAN. Records the HHDM offset — needed only to reach a freshly
/// allocated shadow frame through its direct-map alias in order to zero it, not
/// for shadow addressing, which is HHDM-independent. Does **not** map any shadow
/// pages (they are lazily backed) and does **not** enable checking (call
/// [`enable`]).
pub fn init(hhdm_offset: u64) {
    HHDM_OFFSET.store(hhdm_offset, Ordering::Relaxed);
    INITED.store(true, Ordering::Release);
    const COVER_LOW: u64 = 0xFFFF_8000_0000_0000;
    serial_println!(
        "[kasan] shadow ready: base={:#x} backs kernel VA [{:#x}..{:#x}) \
         (1:8 scale, offset={:#x}, lazily mapped)",
        KASAN_SHADOW_BASE,
        COVER_LOW,
        COVER_LOW.wrapping_add(KASAN_COVER_BYTES),
        KASAN_SHADOW_OFFSET
    );
}

/// Enable KASAN checking + shadow maintenance.
pub fn enable() {
    if INITED.load(Ordering::Acquire) {
        ENABLED.store(true, Ordering::Release);
    }
}

/// Disable KASAN checking + shadow maintenance.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether KASAN is active (hot-path gate).
#[inline(always)]
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Shadow addressing + lazy mapping
// ---------------------------------------------------------------------------

/// Return the shadow byte address for `addr`, or `None` if `addr` lies outside
/// the shadow window this module backs.
///
/// The mapping is unconditional (`(addr >> 3) + OFFSET`, matching the compiler
/// instrumentation exactly); the `None` case is purely about *backing*: only the
/// first [`KASAN_SHADOW_SIZE`] of the shadow reservation is ever mapped here, so
/// anything landing above it — or below the base, i.e. a user address — has no
/// shadow byte and must fail open rather than dereference unmapped memory.
///
/// `wrapping_*` throughout: for a user address the add legitimately produces a
/// value below the shadow base, and the subtraction then wraps to a huge number
/// that the bound check rejects. That is the intended arithmetic, not overflow.
#[inline]
fn shadow_of(addr: u64) -> Option<u64> {
    let sv = (addr >> KASAN_GRANULE_SHIFT).wrapping_add(KASAN_SHADOW_OFFSET);
    if sv.wrapping_sub(KASAN_SHADOW_BASE) >= KASAN_SHADOW_SIZE {
        return None;
    }
    Some(sv)
}

/// Ensure the 16 KiB shadow frame containing `shadow_va` is mapped and zeroed.
///
/// Cheap fast path: a single bitmap load. On the (rare) first touch of a
/// shadow frame — one map per 128 KiB of heap ever touched — allocates a
/// frame, maps it NX+writable, and zeroes it.
fn ensure_shadow_mapped(shadow_va: u64) -> bool {
    let frame_idx = ((shadow_va - KASAN_SHADOW_BASE) / FRAME_SIZE as u64) as usize;
    if frame_idx >= SHADOW_FRAMES {
        return false;
    }
    let word = frame_idx / 64;
    let bit = 1u64 << (frame_idx % 64);

    // Fast path: already mapped.
    if SHADOW_MAPPED[word].load(Ordering::Acquire) & bit != 0 {
        return true;
    }

    // Slow path: map under the lock with interrupts disabled (re-entrancy safe).
    with_map_lock(|| {
        // Double-check after acquiring the lock.
        if SHADOW_MAPPED[word].load(Ordering::Acquire) & bit != 0 {
            return true;
        }

        let Ok(phys) = frame::alloc_frame() else {
            return false;
        };

        // Zero the shadow frame via its HHDM alias before mapping so the
        // default state is "addressable" (0x00).
        let hhdm = HHDM_OFFSET.load(Ordering::Relaxed);
        let frame_virt = phys.to_virt(hhdm) as *mut u8;
        // SAFETY: to_virt yields the HHDM alias of a freshly-allocated frame,
        // valid and writable for FRAME_SIZE bytes.
        unsafe {
            core::ptr::write_bytes(frame_virt, 0, FRAME_SIZE);
        }

        let frame_base = KASAN_SHADOW_BASE + (frame_idx as u64) * FRAME_SIZE as u64;
        // SAFETY: `frame_base` is frame-aligned and inside the backed window
        // (checked above), `phys` is a frame we just allocated and still own,
        // and we hold MAP_LOCK.
        if unsafe { install_shadow_frame(frame_base, phys.addr(), hhdm) } {
            SHADOW_MAPPED[word].fetch_or(bit, Ordering::Release);
            SHADOW_FRAMES_MAPPED.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            // Roll back the frame we couldn't map.
            // SAFETY: `phys` was just allocated and never handed out.
            let _ = unsafe { frame::free_frame(phys) };
            false
        }
    })
    .unwrap_or(false)
}

/// Write `val` to the shadow byte for `addr` (mapping the shadow page if
/// needed). No-op if `addr` is outside the covered range or mapping fails.
#[inline]
fn set_shadow(addr: u64, val: u8) {
    let Some(sv) = shadow_of(addr) else { return };
    if !ensure_shadow_mapped(sv) {
        return;
    }
    // SAFETY: ensure_shadow_mapped guarantees the shadow byte is mapped
    // writable; `sv` is within the reserved shadow range.
    unsafe {
        core::ptr::write_volatile(sv as *mut u8, val);
    }
}

/// `log2(FRAME_SIZE)`. The shadow lookup divides by the frame size on the
/// hottest path in the instrumented build, and must do it with a shift rather
/// than `/`: a debug-profile division emits a divide-by-zero check whose panic
/// path is `core` code monomorphised into this crate *with* instrumentation.
/// A compile-time assertion keeps the two in step.
const FRAME_SIZE_SHIFT: u32 = 14;
const _: () = assert!(1usize << FRAME_SIZE_SHIFT == FRAME_SIZE);

/// Read the shadow byte for `addr`. Returns `KASAN_ADDRESSABLE` for
/// out-of-range or unmapped shadow (fail-open: never a false positive).
///
/// **Every memory access here is raw** — see the "Raw memory access" section
/// above. With outlined instrumentation this function sits underneath
/// `mm::kasan_rt::__asan_load*`, so an instrumented access of its own would
/// call back into `__asan_load*` and recurse without bound. The bitmap word and
/// the shadow byte are therefore loaded with `asm!`, the index arithmetic uses
/// shifts and masks instead of `/` and `%`, and the shift amounts go through
/// [`raw_shl_u64`]. None of this is an optimization; all of it is the
/// termination argument.
///
/// Dropping the `Acquire` on the bitmap load costs nothing: x86 gives every
/// `mov` acquire semantics, and the `asm!` block is an optimization barrier, so
/// the load cannot be hoisted above the range checks that guard it.
#[inline]
fn get_shadow(addr: u64) -> u8 {
    let Some(sv) = shadow_of(addr) else {
        return KASAN_ADDRESSABLE;
    };
    let frame_idx = raw_shr_u64(sv.wrapping_sub(KASAN_SHADOW_BASE), FRAME_SIZE_SHIFT) as usize;
    if frame_idx >= SHADOW_FRAMES {
        return KASAN_ADDRESSABLE;
    }
    let word = frame_idx >> 6;
    let bit = raw_shl_u64(1, (frame_idx & 63) as u32);
    // The bitmap is a `static`, so its base address is a link-time constant and
    // `word` was just bounds-checked against `SHADOW_FRAMES`.
    let word_addr = (SHADOW_MAPPED.as_ptr() as u64).wrapping_add((word as u64) << 3);
    // SAFETY: `word_addr` is `&SHADOW_MAPPED[word]` computed without indexing,
    // with `word < MAP_BITMAP_WORDS` established by the check above.
    if unsafe { raw_load_u64(word_addr) } & bit == 0 {
        return KASAN_ADDRESSABLE;
    }
    // SAFETY: the shadow frame is mapped (bitmap bit set); `sv` is in range.
    unsafe { raw_load_u8(sv) }
}

// ---------------------------------------------------------------------------
// Poison / unpoison
// ---------------------------------------------------------------------------

/// Mark every granule fully inside `[addr, addr+size)` with `val`.
fn poison_granules(addr: u64, size: u64, val: u8) {
    if size == 0 {
        return;
    }
    let mut g = addr & !KASAN_GRANULE_MASK;
    let end = addr + size;
    while g + KASAN_GRANULE <= end {
        set_shadow(g, val);
        g += KASAN_GRANULE;
    }
}

/// Mark `[addr, addr+size)` as addressable, encoding a trailing partial
/// granule (an object that ends mid-granule) as its addressable-byte count.
fn unpoison_range(addr: u64, size: u64) {
    if size == 0 {
        return;
    }
    let full = size & !KASAN_GRANULE_MASK;
    poison_granules(addr, full, KASAN_ADDRESSABLE);
    let rem = size & KASAN_GRANULE_MASK;
    if rem != 0 {
        // Partial trailing granule: first `rem` bytes addressable.
        set_shadow(addr + full, rem as u8);
    }
    BYTES_UNPOISONED.fetch_add(size, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Alloc / free hooks (called from the heap allocator)
// ---------------------------------------------------------------------------

/// Record a heap allocation: `req` bytes usable within a `slot`-byte slot.
/// Marks the object addressable and the slot's trailing padding as a redzone.
#[inline]
pub fn on_alloc(ptr: *mut u8, req: usize, slot: usize) {
    if !is_enabled() || ptr.is_null() || req == 0 {
        return;
    }
    let addr = ptr as u64;
    unpoison_range(addr, req as u64);
    // Redzone = the slot bytes past the object (rounded up to a granule).
    let obj_end = (addr + req as u64 + KASAN_GRANULE_MASK) & !KASAN_GRANULE_MASK;
    let slot_end = addr + slot as u64;
    if slot_end > obj_end {
        poison_granules(obj_end, slot_end - obj_end, KASAN_REDZONE);
        BYTES_POISONED.fetch_add(slot_end - obj_end, Ordering::Relaxed);
    }
}

/// Record a heap free: poison the whole `slot`-byte slot as freed so any
/// later KASAN-checked access to it is flagged as a use-after-free.
#[inline]
pub fn on_free(ptr: *mut u8, slot: usize) {
    if !is_enabled() || ptr.is_null() || slot == 0 {
        return;
    }
    let addr = ptr as u64;
    poison_granules(addr, slot as u64, KASAN_FREE);
    BYTES_POISONED.fetch_add(slot as u64, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Access checking (the checked-store/load shim)
// ---------------------------------------------------------------------------

/// A detected KASAN violation.
#[derive(Debug, Clone, Copy)]
pub struct Violation {
    /// The offending access address.
    pub addr: u64,
    /// Access size in bytes.
    pub size: usize,
    /// `true` for a write, `false` for a read.
    pub is_write: bool,
    /// The shadow byte that flagged the access.
    pub shadow: u8,
}

impl Violation {
    /// Human-readable name for the shadow state.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        describe_shadow(self.shadow)
    }
}

/// Human-readable name for a shadow byte value.
///
/// Shared with `mm::kasan_rt` so a violation found by the compiler's inline
/// check and one found by this module's manual `check` are described in the
/// same words — when both appear in one log, a wording difference reads as a
/// difference in *kind*, which it is not.
#[must_use]
pub fn describe_shadow(shadow: u8) -> &'static str {
    match shadow {
        KASAN_FREE => "use-after-free (freed heap)",
        KASAN_REDZONE => "out-of-bounds (heap redzone)",
        v if v <= 7 => "out-of-bounds (partial granule)",
        _ => "poisoned heap",
    }
}

/// The shadow byte covering `addr` (`KASAN_ADDRESSABLE` if unshadowed).
#[must_use]
pub fn shadow_byte(addr: u64) -> u8 {
    get_shadow(addr)
}

/// Whether the shadow permits `[addr, addr+size)`, consulting only the shadow.
///
/// Unlike [`check`] this is **not** gated on [`is_enabled`]: it backs the
/// compiler-emitted checks in `mm::kasan_rt`, which LLVM runs unconditionally.
/// The gate is unnecessary there anyway — when KASAN is disabled nothing writes
/// poison, so every shadow byte reads `0x00` and this returns `true`.
#[must_use]
pub fn shadow_allows(addr: u64, size: usize) -> bool {
    if size == 0 {
        return true;
    }
    // `wrapping_*` throughout: `size` is non-zero, so none of these can actually
    // overflow, but a *checked* operator emits a branch into `core`'s panic
    // machinery — which is monomorphised into this crate with instrumentation,
    // and this function runs underneath the instrumentation. See `get_shadow`.
    let last = addr.wrapping_add((size as u64).wrapping_sub(1));
    let mut a = addr;
    loop {
        if byte_bad(a) != KASAN_ADDRESSABLE {
            return false;
        }
        let next = (a & !KASAN_GRANULE_MASK).wrapping_add(KASAN_GRANULE);
        if next > last {
            break;
        }
        a = next;
    }
    byte_bad(last) == KASAN_ADDRESSABLE
}

/// Allocate, free, and return the address of a heap object whose shadow is
/// therefore poisoned `KASAN_FREE` — a known-bad address for testing a checker.
///
/// Returns `None` if the object landed outside the backed shadow window, in
/// which case there is nothing to test against and the caller must skip rather
/// than fail.
///
/// Leaves [`ENABLED`] as it found it: this is called from a boot self-test, and
/// silently turning KASAN on for the rest of the boot would change the
/// performance profile of everything measured afterwards.
#[must_use]
pub fn self_test_freed_address() -> Option<u64> {
    use alloc::alloc::{Layout, alloc, dealloc};

    if !INITED.load(Ordering::Acquire) {
        return None;
    }
    let was_enabled = is_enabled();
    enable();

    let layout = Layout::from_size_align(64, 8).expect("valid layout");
    // SAFETY: valid non-zero layout; null-checked immediately.
    let p = unsafe { alloc(layout) };
    let result = if p.is_null() {
        None
    } else {
        let a = p as u64;
        // SAFETY: `p` came from `alloc(layout)` and is not used afterwards
        // except as an integer address for shadow lookups.
        unsafe {
            dealloc(p, layout);
        }
        // Only usable if the free actually poisoned a shadow byte we can read.
        if get_shadow(a) == KASAN_FREE {
            Some(a)
        } else {
            None
        }
    };

    if !was_enabled {
        disable();
    }
    result
}

/// The shadow byte that makes the single byte at `a` inaccessible, or
/// [`KASAN_ADDRESSABLE`] (0) if it is accessible.
///
/// Returning a sentinel rather than `Option<u8>` is not a style choice. This
/// function is on the path `mm::kasan_rt`'s outlined checks call into, and
/// `Option::<u8>::is_some` is a generic `core` function that monomorphises into
/// this crate *with* instrumentation — so testing the result would call the
/// check again, without bound. A zero shadow byte already means "addressable"
/// and is never a violation code (`byte_bad` never returned `Some(0)`), so the
/// sentinel loses no information.
#[inline]
fn byte_bad(a: u64) -> u8 {
    let sb = get_shadow(a);
    if sb == KASAN_ADDRESSABLE {
        KASAN_ADDRESSABLE
    } else if sb <= 7 {
        // Partial granule: bytes [0, sb) accessible.
        if (a & KASAN_GRANULE_MASK) as u8 >= sb {
            sb
        } else {
            KASAN_ADDRESSABLE
        }
    } else {
        // 0xFA / 0xFB / any other poison.
        sb
    }
}

/// Check whether `[addr, addr+size)` is fully accessible. Returns the first
/// violation found, or `Ok(())`. Fail-open when KASAN is disabled/uninit.
pub fn check(addr: u64, size: usize, is_write: bool) -> Result<(), Violation> {
    if !is_enabled() || size == 0 {
        return Ok(());
    }
    let last = addr + (size as u64 - 1);
    // Check the first byte, every intermediate granule boundary, and the last.
    let mut a = addr;
    loop {
        let sb = byte_bad(a);
        if sb != KASAN_ADDRESSABLE {
            VIOLATIONS.fetch_add(1, Ordering::Relaxed);
            return Err(Violation {
                addr: a,
                size,
                is_write,
                shadow: sb,
            });
        }
        // Advance to the next granule boundary (or stop at the last byte).
        let next = (a & !KASAN_GRANULE_MASK) + KASAN_GRANULE;
        if next > last {
            break;
        }
        a = next;
    }
    // Ensure the final byte is checked (it may share the first byte's granule).
    let sb = byte_bad(last);
    if sb != KASAN_ADDRESSABLE {
        VIOLATIONS.fetch_add(1, Ordering::Relaxed);
        return Err(Violation {
            addr: last,
            size,
            is_write,
            shadow: sb,
        });
    }
    Ok(())
}

/// Checked-access shim: report (but do not panic on) a violation. Intended for
/// the suspect scheduler/teardown paths. Returns `true` if the access is safe.
#[inline]
pub fn check_access(addr: u64, size: usize, is_write: bool) -> bool {
    match check(addr, size, is_write) {
        Ok(()) => true,
        Err(v) => {
            report(&v);
            false
        }
    }
}

/// Emit a one-line CRITICAL report for a violation (no panic — the caller
/// decides how to proceed; this converts a silent wild access into evidence).
pub fn report(v: &Violation) {
    serial_println!(
        "[kasan] CRITICAL: {} on {} of {} bytes @ {:#x} (shadow={:#04x})",
        v.kind(),
        if v.is_write { "write" } else { "read" },
        v.size,
        v.addr,
        v.shadow
    );
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// KASAN subsystem statistics.
#[derive(Debug, Clone, Copy)]
pub struct KasanStats {
    /// Whether KASAN is currently enabled.
    pub enabled: bool,
    /// Violations detected since boot.
    pub violations: u64,
    /// Shadow frames currently mapped.
    pub shadow_frames_mapped: u64,
    /// Total bytes poisoned (free + redzone).
    pub bytes_poisoned: u64,
    /// Total bytes unpoisoned (allocated).
    pub bytes_unpoisoned: u64,
    /// Allocations whose shadow was never written because an IRQ-context caller
    /// found `MAP_LOCK` busy and could not safely wait for it.
    ///
    /// Nonzero means KASAN has blind spots this boot: each one is an allocation
    /// whose redzones and freed-state poison were silently dropped, so an
    /// overflow or use-after-free on it cannot be detected. Never a false
    /// positive — the shadow fails open. See
    /// `known-issues.md` → `TD-KASAN-IRQ-CONTEXT-ALLOCATIONS-LOSE-SHADOW-COVERAGE`.
    pub map_lock_giveups: u64,
}

/// Read KASAN statistics.
#[must_use]
pub fn stats() -> KasanStats {
    KasanStats {
        enabled: is_enabled(),
        violations: VIOLATIONS.load(Ordering::Relaxed),
        shadow_frames_mapped: SHADOW_FRAMES_MAPPED.load(Ordering::Relaxed),
        bytes_poisoned: BYTES_POISONED.load(Ordering::Relaxed),
        bytes_unpoisoned: BYTES_UNPOISONED.load(Ordering::Relaxed),
        map_lock_giveups: MAP_LOCK_GIVEUPS.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot self-test: exercises the shadow map with **real heap allocations** so
/// the whole lazy-mapping + poison + check pipeline is validated end to end.
///
/// Verifies (with values distinct from all defaults, so nothing can
/// false-pass): a live object reads clean, its redzone and a partial granule
/// are flagged out-of-bounds, and a freed object is flagged use-after-free.
pub fn self_test() {
    use alloc::alloc::{Layout, alloc, dealloc};

    if !INITED.load(Ordering::Acquire) {
        serial_println!("[kasan] self-test SKIPPED (not initialized)");
        return;
    }
    serial_println!("[kasan] Running self-test...");

    let was_enabled = is_enabled();
    enable();

    // -- Test 1: 40-byte object in a 64-byte slot (clean body + redzone) -----
    // 40 is granule-aligned → 5 clean shadow bytes; slot padding [40,64) is a
    // 24-byte (3-granule) redzone.
    let layout40 = Layout::from_size_align(40, 8).expect("valid layout");
    // SAFETY: valid non-zero layout; pointer checked for null below.
    let p = unsafe { alloc(layout40) };
    assert!(!p.is_null(), "kasan self-test: alloc(40) failed");
    let a = p as u64;

    // Defensive: KASAN only covers `[hhdm, hhdm + KASAN_COVER_BYTES)`. If the
    // heap handed back a slot whose HHDM offset falls outside that window
    // (physical RAM larger than the cover), its shadow byte does not exist —
    // `shadow_of` returns `None`, poison is silently dropped, and every check
    // below fails *open*, so the redzone/UAF asserts would spuriously panic and
    // halt the boot. With the 64 GiB cover this cannot happen on any realistic
    // dev/QEMU config, but rather than reintroduce a boot-halting flake on a
    // future RAM-larger-than-cover box, skip the self-test with a loud warning.
    if shadow_of(a).is_none() || shadow_of(a + 63).is_none() {
        serial_println!(
            "[kasan] self-test SKIPPED: test object {:#x} is outside the covered \
             window — raise KASAN_SHADOW_SIZE above physical RAM to self-verify",
            a
        );
        // SAFETY: `p` was allocated with `layout40` and is otherwise unused.
        unsafe {
            dealloc(p, layout40);
        }
        if !was_enabled {
            disable();
        }
        return;
    }

    // Body is fully accessible.
    assert!(check(a, 40, false).is_ok(), "kasan: body flagged");
    assert!(check(a, 40, true).is_ok(), "kasan: body write flagged");
    // First redzone byte and last slot byte must be flagged.
    assert!(
        check(a + 40, 1, false).is_err(),
        "kasan: redzone not caught"
    );
    assert!(
        check(a + 63, 1, true).is_err(),
        "kasan: slot end not caught"
    );
    // A read straddling the object end into the redzone must be caught.
    assert!(
        check(a + 39, 2, false).is_err(),
        "kasan: straddle not caught"
    );
    serial_println!("[kasan]   redzone (40-in-64): OK");

    // -- Test 2: free → whole slot poisoned as use-after-free ----------------
    // SAFETY: `p` was allocated with `layout40`; not used after free except
    // via KASAN shadow checks (which do not dereference it).
    unsafe {
        dealloc(p, layout40);
    }
    assert!(check(a, 8, false).is_err(), "kasan: UAF read not caught");
    assert!(
        check(a + 32, 4, true).is_err(),
        "kasan: UAF write not caught"
    );
    let v = check(a, 8, false).unwrap_err();
    assert_eq!(v.shadow, KASAN_FREE, "kasan: freed shadow mismatch");
    serial_println!("[kasan]   use-after-free (freed 64B slot): OK");

    // -- Test 3: partial-granule object (12 bytes in a 16-byte slot) ---------
    let layout12 = Layout::from_size_align(12, 8).expect("valid layout");
    // SAFETY: valid non-zero layout; null-checked below.
    let p2 = unsafe { alloc(layout12) };
    assert!(!p2.is_null(), "kasan self-test: alloc(12) failed");
    let a2 = p2 as u64;
    // Bytes 0..12 accessible; byte 12 (in the second granule, only 4 allowed)
    // must be flagged.
    assert!(check(a2, 12, false).is_ok(), "kasan: 12B body flagged");
    assert!(
        check(a2 + 12, 1, false).is_err(),
        "kasan: partial granule not caught"
    );
    assert!(
        check(a2 + 11, 1, false).is_ok(),
        "kasan: byte 11 wrongly flagged"
    );
    // SAFETY: `p2` was allocated with `layout12`.
    unsafe {
        dealloc(p2, layout12);
    }
    serial_println!("[kasan]   partial granule (12-in-16): OK");

    // -- Test 4: an address outside the covered range fails open -------------
    // A user address maps *below* the shadow base (the add wraps under it), so
    // this also pins the "negative" side of the window check.
    assert!(
        check(0x1000, 8, false).is_ok(),
        "kasan: out-of-range not fail-open"
    );
    serial_println!("[kasan]   out-of-range fail-open: OK");

    // -- Test 5: the shadow mapping is exactly the one LLVM is told to emit ---
    // The compiler-KASAN profile hands `KASAN_SHADOW_OFFSET` to LLVM, which then
    // emits `shr $3; add $offset` inline at every load/store. If this module's
    // `shadow_of` ever drifted from that formula, the poison written below would
    // land somewhere the compiler's checks never read — KASAN would go silently
    // blind rather than fail. Assert the two agree, on a real heap address.
    let layout8 = Layout::from_size_align(8, 8).expect("valid layout");
    // SAFETY: valid non-zero layout; null-checked below.
    let p3 = unsafe { alloc(layout8) };
    assert!(!p3.is_null(), "kasan self-test: alloc(8) failed");
    let a3 = p3 as u64;
    let expected = (a3 >> KASAN_GRANULE_SHIFT).wrapping_add(KASAN_SHADOW_OFFSET);
    assert_eq!(
        shadow_of(a3),
        Some(expected),
        "kasan: shadow_of != LLVM mapping"
    );
    assert!(
        expected >= KASAN_SHADOW_BASE && expected < KASAN_SHADOW_BASE + KASAN_SHADOW_SIZE,
        "kasan: heap shadow outside the backed window"
    );
    // SAFETY: `p3` was allocated with `layout8`.
    unsafe {
        dealloc(p3, layout8);
    }
    serial_println!("[kasan]   mapping matches -asan-mapping-offset: OK");

    let st = stats();
    serial_println!(
        "[kasan]   stats: violations={}, shadow_frames={}, poisoned={}B, unpoisoned={}B, \
         map_lock_giveups={}",
        st.violations,
        st.shadow_frames_mapped,
        st.bytes_poisoned,
        st.bytes_unpoisoned,
        st.map_lock_giveups
    );
    // Printed as its own line rather than left in the stats tuple: a nonzero
    // value means the shadow has holes this boot, which changes how any KASAN
    // result from this boot should be read, and it must not be something you
    // only notice by comparing two numbers in a long line.
    if st.map_lock_giveups != 0 {
        serial_println!(
            "[kasan]   WARNING: {} IRQ-context allocation(s) went unpoisoned \
             (MAP_LOCK busy) — shadow coverage has holes this boot",
            st.map_lock_giveups
        );
    }

    if !was_enabled {
        disable();
    }
    serial_println!("[kasan] Self-test PASSED");
}

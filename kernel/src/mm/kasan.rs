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
use crate::mm::page_table::{self, PageFlags, VirtAddr};
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

// Statistics.
static VIOLATIONS: AtomicU64 = AtomicU64::new(0);
static SHADOW_FRAMES_MAPPED: AtomicU64 = AtomicU64::new(0);
static BYTES_POISONED: AtomicU64 = AtomicU64::new(0);
static BYTES_UNPOISONED: AtomicU64 = AtomicU64::new(0);

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
    crate::cpu::without_interrupts(|| {
        let _guard = MAP_LOCK.lock();
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
        unsafe { core::ptr::write_bytes(frame_virt, 0, FRAME_SIZE); }

        let frame_base = KASAN_SHADOW_BASE + (frame_idx as u64) * FRAME_SIZE as u64;
        let pml4 = page_table::active_pml4_phys();
        let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;
        // SAFETY: pml4 is the active table; `phys` is a valid frame we just
        // allocated; `frame_base` is a reserved-but-unmapped shadow VA. We
        // flush the new mapping below.
        match unsafe { page_table::map_frame(pml4, VirtAddr::new(frame_base), phys, flags) } {
            Ok(()) => {
                // SAFETY: invlpg on the just-mapped frame is always safe in ring 0.
                unsafe { page_table::flush_frame(VirtAddr::new(frame_base)); }
                SHADOW_MAPPED[word].fetch_or(bit, Ordering::Release);
                SHADOW_FRAMES_MAPPED.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(_) => {
                // Roll back the frame we couldn't map.
                // SAFETY: `phys` was just allocated and never handed out.
                let _ = unsafe { frame::free_frame(phys) };
                false
            }
        }
    })
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
    unsafe { core::ptr::write_volatile(sv as *mut u8, val); }
}

/// Read the shadow byte for `addr`. Returns `KASAN_ADDRESSABLE` for
/// out-of-range or unmapped shadow (fail-open: never a false positive).
#[inline]
fn get_shadow(addr: u64) -> u8 {
    let Some(sv) = shadow_of(addr) else { return KASAN_ADDRESSABLE };
    let frame_idx = ((sv - KASAN_SHADOW_BASE) / FRAME_SIZE as u64) as usize;
    if frame_idx >= SHADOW_FRAMES {
        return KASAN_ADDRESSABLE;
    }
    let word = frame_idx / 64;
    let bit = 1u64 << (frame_idx % 64);
    if SHADOW_MAPPED[word].load(Ordering::Acquire) & bit == 0 {
        return KASAN_ADDRESSABLE;
    }
    // SAFETY: the shadow frame is mapped (bitmap bit set); `sv` is in range.
    unsafe { core::ptr::read_volatile(sv as *const u8) }
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
    let last = addr.wrapping_add(size as u64 - 1);
    let mut a = addr;
    loop {
        if byte_bad(a).is_some() {
            return false;
        }
        let next = (a & !KASAN_GRANULE_MASK) + KASAN_GRANULE;
        if next > last {
            break;
        }
        a = next;
    }
    byte_bad(last).is_none()
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
    use alloc::alloc::{alloc, dealloc, Layout};

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
        unsafe { dealloc(p, layout); }
        // Only usable if the free actually poisoned a shadow byte we can read.
        if get_shadow(a) == KASAN_FREE { Some(a) } else { None }
    };

    if !was_enabled {
        disable();
    }
    result
}

/// Is a single byte at `a` inaccessible per its shadow?
#[inline]
fn byte_bad(a: u64) -> Option<u8> {
    let sb = get_shadow(a);
    if sb == KASAN_ADDRESSABLE {
        None
    } else if sb <= 7 {
        // Partial granule: bytes [0, sb) accessible.
        if (a & KASAN_GRANULE_MASK) as u8 >= sb {
            Some(sb)
        } else {
            None
        }
    } else {
        // 0xFA / 0xFB / any other poison.
        Some(sb)
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
        if let Some(sb) = byte_bad(a) {
            VIOLATIONS.fetch_add(1, Ordering::Relaxed);
            return Err(Violation { addr: a, size, is_write, shadow: sb });
        }
        // Advance to the next granule boundary (or stop at the last byte).
        let next = (a & !KASAN_GRANULE_MASK) + KASAN_GRANULE;
        if next > last {
            break;
        }
        a = next;
    }
    // Ensure the final byte is checked (it may share the first byte's granule).
    if let Some(sb) = byte_bad(last) {
        VIOLATIONS.fetch_add(1, Ordering::Relaxed);
        return Err(Violation { addr: last, size, is_write, shadow: sb });
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
    use alloc::alloc::{alloc, dealloc, Layout};

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
        unsafe { dealloc(p, layout40); }
        if !was_enabled {
            disable();
        }
        return;
    }

    // Body is fully accessible.
    assert!(check(a, 40, false).is_ok(), "kasan: body flagged");
    assert!(check(a, 40, true).is_ok(), "kasan: body write flagged");
    // First redzone byte and last slot byte must be flagged.
    assert!(check(a + 40, 1, false).is_err(), "kasan: redzone not caught");
    assert!(check(a + 63, 1, true).is_err(), "kasan: slot end not caught");
    // A read straddling the object end into the redzone must be caught.
    assert!(check(a + 39, 2, false).is_err(), "kasan: straddle not caught");
    serial_println!("[kasan]   redzone (40-in-64): OK");

    // -- Test 2: free → whole slot poisoned as use-after-free ----------------
    // SAFETY: `p` was allocated with `layout40`; not used after free except
    // via KASAN shadow checks (which do not dereference it).
    unsafe { dealloc(p, layout40); }
    assert!(check(a, 8, false).is_err(), "kasan: UAF read not caught");
    assert!(check(a + 32, 4, true).is_err(), "kasan: UAF write not caught");
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
    assert!(check(a2 + 12, 1, false).is_err(), "kasan: partial granule not caught");
    assert!(check(a2 + 11, 1, false).is_ok(), "kasan: byte 11 wrongly flagged");
    // SAFETY: `p2` was allocated with `layout12`.
    unsafe { dealloc(p2, layout12); }
    serial_println!("[kasan]   partial granule (12-in-16): OK");

    // -- Test 4: an address outside the covered range fails open -------------
    // A user address maps *below* the shadow base (the add wraps under it), so
    // this also pins the "negative" side of the window check.
    assert!(check(0x1000, 8, false).is_ok(), "kasan: out-of-range not fail-open");
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
    assert_eq!(shadow_of(a3), Some(expected), "kasan: shadow_of != LLVM mapping");
    assert!(
        expected >= KASAN_SHADOW_BASE && expected < KASAN_SHADOW_BASE + KASAN_SHADOW_SIZE,
        "kasan: heap shadow outside the backed window"
    );
    // SAFETY: `p3` was allocated with `layout8`.
    unsafe { dealloc(p3, layout8); }
    serial_println!("[kasan]   mapping matches -asan-mapping-offset: OK");

    let st = stats();
    serial_println!(
        "[kasan]   stats: violations={}, shadow_frames={}, poisoned={}B, unpoisoned={}B",
        st.violations, st.shadow_frames_mapped, st.bytes_poisoned, st.bytes_unpoisoned
    );

    if !was_enabled {
        disable();
    }
    serial_println!("[kasan] Self-test PASSED");
}

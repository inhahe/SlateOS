//! Uninstrumented raw byte access for code that deliberately touches poisoned
//! memory.
//!
//! # Why this module exists
//!
//! Several memory-debugging subsystems — `mm::heap`'s free-magic and redzone
//! checks, `mm::poison`'s fills, `mm::quarantine`'s parked slots — exist
//! precisely to read and write bytes that `mm::kasan` has marked inaccessible.
//! For them the access *is* the detector, not a bug, so a KASAN report on it is
//! pure noise.
//!
//! Those modules all carry the module-level
//! `#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]` opt-out, and
//! for a long time that looked sufficient. It is not, and the reason is the
//! same one recorded in `design-decisions.md` §118 and §119:
//!
//! > A module-level `sanitize` attribute cannot exempt a generic `core`
//! > function. `sanitize` is a per-function LLVM attribute, and a generic
//! > `core` function monomorphises into *this* crate carrying the default
//! > (instrumented) attribute — the exempt module's attribute never applies to
//! > it.
//!
//! Every one of those modules did its actual byte touching through
//! `core::ptr::{read_volatile, write_volatile, write_bytes}`, which are exactly
//! such generics. The opt-out was therefore cosmetic on the only operations
//! that mattered. The first instrumented boot to reach the self-tests died of
//! it, with a backtrace reading
//! `report <- __asan_load1_noabort <- core::ptr::read_volatile::<u8> <-
//! mm::heap::check_redzone` (see `known-issues.md`
//! → `B-KASAN-INSTRUMENTED-BUILD-PANICS-ON-ITS-OWN-REDZONE-CHECKS`).
//!
//! # Why inline `asm!` rather than another attribute
//!
//! The obvious repair — write hand-rolled loops in the exempt modules so no
//! `core` generic is called — would work today and silently break the first
//! time someone reaches for a `core::ptr` helper inside an "exempt" module,
//! which is a natural thing to do and gives no warning. Inline assembly is a
//! stronger guarantee: **LLVM's AddressSanitizer pass does not instrument
//! memory operands of inline asm**, so the access is uninstrumented no matter
//! whose function body it ends up in, whether or not this helper is inlined,
//! and whether or not the caller is exempt. The property is enforced by the
//! mechanism instead of by everyone remembering a rule.
//!
//! # Volatility
//!
//! The call sites these replace all used *volatile* accesses on purpose: the
//! optimizer must not dead-store-eliminate a poison fill it can prove is never
//! read, nor constant-propagate a verify read. That property is preserved here
//! by omitting the `nomem`/`readonly` options — an `asm!` block without them is
//! assumed to read and write arbitrary memory, which is at least as strong a
//! barrier as `read_volatile`/`write_volatile`.

// Diagnostic support code for the memory debuggers; see the module docs above.
// Exempt for the same reason its callers are — though note that the exemption
// is belt-and-braces here, since the accesses are in `asm!` regardless.
#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]

use core::arch::asm;

/// Read one byte from `p` without KASAN instrumentation.
///
/// Equivalent to `core::ptr::read_volatile(p)` except that the load is emitted
/// as inline assembly and so is never instrumented.
///
/// # Safety
///
/// `p` must be valid for a 1-byte read. It may point at memory KASAN has
/// poisoned — that is the entire point — but it must still be mapped.
#[inline]
#[must_use]
pub unsafe fn read_u8(p: *const u8) -> u8 {
    let v: u8;
    // SAFETY: the caller guarantees `p` is valid for a 1-byte read. The block
    // performs exactly that load and touches nothing else; `nostack` holds
    // because no push/pop is emitted, and `preserves_flags` because `mov` does
    // not write flags. `nomem`/`readonly` are deliberately NOT set, to keep the
    // volatile semantics the call sites depend on.
    unsafe {
        asm!(
            "mov {v}, byte ptr [{p}]",
            p = in(reg) p,
            v = out(reg_byte) v,
            options(nostack, preserves_flags),
        );
    }
    v
}

/// Write one byte to `p` without KASAN instrumentation.
///
/// Equivalent to `core::ptr::write_volatile(p, v)` except that the store is
/// emitted as inline assembly and so is never instrumented.
///
/// # Safety
///
/// `p` must be valid for a 1-byte write, and the caller must have exclusive
/// access to it. It may point at memory KASAN has poisoned.
#[inline]
pub unsafe fn write_u8(p: *mut u8, v: u8) {
    // SAFETY: the caller guarantees `p` is valid for a 1-byte write and is
    // exclusively owned. The block performs exactly that store. `nostack` and
    // `preserves_flags` hold as in `read_u8`.
    unsafe {
        asm!(
            "mov byte ptr [{p}], {v}",
            p = in(reg) p,
            v = in(reg_byte) v,
            options(nostack, preserves_flags),
        );
    }
}

/// Fill `len` bytes at `p` with `v` without KASAN instrumentation.
///
/// Equivalent to `core::ptr::write_bytes(p, v, len)` except that the stores are
/// emitted as inline assembly and so are never instrumented. This is the bulk
/// form of [`write_u8`]; prefer it for fills, since the per-byte helper would
/// otherwise emit one `asm!` block per byte in a debug build.
///
/// # Safety
///
/// `p` must be valid for `len` bytes of writing and exclusively owned by the
/// caller. It may point at memory KASAN has poisoned.
#[inline]
pub unsafe fn fill_u8(p: *mut u8, v: u8, len: usize) {
    // `rep stosb` with RCX = 0 is a no-op, so no length guard is needed.
    //
    // SAFETY: the caller guarantees `p` is valid for `len` bytes of exclusive
    // writing. `rep stosb` writes exactly `[p, p + len)` and clobbers only RDI
    // and RCX, both declared. It requires DF = 0, which the SysV ABI guarantees
    // at every function boundary and which nothing here changes. `stos` does
    // not write flags, so `preserves_flags` holds; `nostack` holds because no
    // push/pop is emitted. `nomem` is deliberately not set (see module docs).
    unsafe {
        asm!(
            "rep stosb",
            inout("rdi") p => _,
            inout("rcx") len => _,
            in("eax") u32::from(v),
            options(nostack, preserves_flags),
        );
    }
}

/// Boot self-test for the raw accessors.
///
/// This is a boot self-test rather than a `#[cfg(test)]` module because the
/// kernel is a `no_std` binary that defines its own `panic_impl`, so it cannot
/// be built for the host test harness (`cargo test -p kernel` fails with a
/// duplicate `panic_impl` lang item). The rest of `mm` tests itself the same
/// way — see `mm::poison::self_test`.
///
/// These helpers are the foundation the poison, redzone and quarantine checks
/// all stand on, so they run *before* those subsystems' own self-tests: if a
/// hand-written `asm!` store had the wrong operand size or direction, every
/// downstream "OK" would be meaningless.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[rawmem] Running self-test...");

    // Round-trip through the byte accessors, at both ends of a buffer so an
    // off-by-one in the addressing shows up.
    let mut buf = [0u8; 8];
    let p = buf.as_mut_ptr();
    // SAFETY: `p` points at an 8-byte stack array owned exclusively here, so
    // offsets 0 and 7 are in-bounds for both reading and writing.
    unsafe {
        write_u8(p, 0xAB);
        write_u8(p.add(7), 0xCD);
    }
    assert!(
        buf[0] == 0xAB && buf[7] == 0xCD,
        "rawmem: write_u8 did not land where expected"
    );
    // SAFETY: as above — reads of two in-bounds offsets of a live array.
    let (first, last) = unsafe { (read_u8(p), read_u8(p.add(7))) };
    assert!(
        first == 0xAB && last == 0xCD,
        "rawmem: read_u8 disagreed with the value written"
    );
    serial_println!("[rawmem]   read_u8/write_u8 round-trip: OK");

    // A fill must cover exactly the requested range: `rep stosb` with the
    // wrong direction flag or an off-by-one count would spill either side.
    let mut fbuf = [0u8; 16];
    let fp = fbuf.as_mut_ptr();
    // SAFETY: filling bytes 4..12 of a 16-byte array owned exclusively here.
    unsafe { fill_u8(fp.add(4), 0xDE, 8) };
    assert!(fbuf[0..4] == [0; 4], "rawmem: fill_u8 wrote before the start");
    assert!(fbuf[4..12] == [0xDE; 8], "rawmem: fill_u8 did not fill the range");
    assert!(fbuf[12..16] == [0; 4], "rawmem: fill_u8 wrote past the end");
    serial_println!("[rawmem]   fill_u8 covers exactly the range: OK");

    // `rep stosb` with RCX = 0 must be a no-op; the fill helpers are called
    // with zero lengths on empty redzones.
    let mut zbuf = [0x11u8; 4];
    // SAFETY: a zero-length fill dereferences nothing.
    unsafe { fill_u8(zbuf.as_mut_ptr(), 0xFF, 0) };
    assert!(zbuf == [0x11; 4], "rawmem: zero-length fill wrote something");
    serial_println!("[rawmem]   zero-length fill is a no-op: OK");

    serial_println!("[rawmem] Self-test PASSED");
}

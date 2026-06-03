//! `<sys/io.h>` — I/O port access for x86.
//!
//! Provides `ioperm()` and `iopl()` for controlling access to
//! x86 I/O ports, plus inline port I/O functions.
//!
//! # Status of these calls
//!
//! `iopl(2)` was deprecated in Linux 5.5 and now returns `ENOSYS` on
//! many kernels. `ioperm(2)` remains supported but is rarely used —
//! modern userspace drivers prefer character devices, `/dev/uio*`, VFIO,
//! or memory-mapped I/O. In our microkernel architecture (per
//! `design.txt`), drivers live in userspace and access hardware via
//! the capability system, not via a kernel-managed I/O bitmap. There
//! is therefore no plan to implement either syscall; we return ENOSYS
//! for valid calls and meaningful EINVAL/EPERM for malformed inputs so
//! that code probing for port-I/O access (X servers, DOS emulators,
//! legacy serial-port tools) falls back to its capability/UIO path or
//! disables direct port access gracefully.

use crate::errno;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Total number of x86 I/O ports (0x0000–0xFFFF).
///
/// Linux's `arch/x86/include/asm/desc.h` defines `IO_BITMAP_BITS = 65536`.
/// Any `ioperm(from, num, ...)` whose range exceeds this is rejected
/// with `EINVAL`.
pub const IO_BITMAP_BITS: u64 = 65536;

/// Maximum valid IOPL level (rings 0–3 on x86).
///
/// `iopl(level)` with `level > 3` returns `EINVAL`. Negative levels also
/// fail because the syscall takes `unsigned int` on Linux and our `i32`
/// signature rejects sign-bit values via the `< 0` check.
pub const IOPL_LEVEL_MAX: i32 = 3;

// ---------------------------------------------------------------------------
// Port access permission functions
// ---------------------------------------------------------------------------

/// Set I/O port permissions.
///
/// Enables or disables access to the I/O port range `[from, from + num)`.
///
/// Linux semantics (`arch/x86/kernel/ioport.c::ksys_ioperm`):
///
/// ```c
/// if ((from + num <= from) || (from + num > IO_BITMAP_BITS))
///     return -EINVAL;
/// if (turn_on && !capable(CAP_SYS_RAWIO))
///     return -EPERM;
/// ```
///
/// Note the **`<=`** in the first clause: it rejects both
///   * `num == 0` (because `from + 0 == from`, so `from <= from` is
///     true — Linux treats a zero-length range as a malformed argument,
///     *not* a no-op success), and
///   * `from + num` overflows below `from` (wrap-around).
///
/// Errors (Linux-matching priority order):
/// - `EINVAL` — `num == 0`, or `from + num` overflows, or `from + num
///   > IO_BITMAP_BITS` (the 64 KiB port space limit).
/// - `EPERM` — `turn_on != 0` and the caller lacks `CAP_SYS_RAWIO`
///   (Phase 178).  Pre-Phase-178 this was *unconditional* on
///   `turn_on != 0` — a divergence: a privileged caller (Linux:
///   CAP_SYS_RAWIO held) should reach the IO-bitmap install path,
///   not be refused.
/// - `ENOSYS` — `turn_on != 0`, caller has `CAP_SYS_RAWIO`, but we
///   have no IO-bitmap install path (per `design.txt`, drivers
///   live in userspace and access hardware via the capability
///   system, not via a kernel-managed IO bitmap).
///
/// `turn_on == 0` on a well-formed range is a successful no-op (we
/// "clear" access we never granted); the cap probe is not run.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ioperm(from: u64, num: u64, turn_on: i32) -> i32 {
    // Range check matches Linux's `(from + num <= from) || (from + num
    // > IO_BITMAP_BITS)` exactly.  We split the overflow detection out
    // from the upper-bound check for readability, but the observable
    // behaviour is identical:
    //
    //   * num == 0 → from + 0 == from → first clause true → EINVAL.
    //   * from + num overflows u64 → first clause true → EINVAL.
    //   * from + num > IO_BITMAP_BITS → second clause true → EINVAL.
    let end = if let Some(e) = from.checked_add(num) { e } else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if end <= from || end > IO_BITMAP_BITS {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // turn_on == 0 on a valid range: clearing access we never granted.
    // Linux treats this as a successful no-op; we do the same.  The
    // cap probe runs only on the "grant" path (turn_on != 0), matching
    // Linux's `ksys_ioperm` which only invokes capable(CAP_SYS_RAWIO)
    // under `if (turn_on)`.
    if turn_on == 0 {
        return 0;
    }

    // Phase 178: gate the grant path on CAP_SYS_RAWIO.  Linux:
    //   if (turn_on && !capable(CAP_SYS_RAWIO))
    //       return -EPERM;
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_RAWIO) {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    // Privileged grant request — no IO-bitmap install path exists
    // in our microkernel (drivers live in userspace per design.txt).
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Set I/O privilege level.
///
/// Sets the I/O privilege level (IOPL) of the calling process. Level
/// must be 0–3; only level 3 grants unrestricted port access on x86.
///
/// ## Linux semantics (`arch/x86/kernel/ioport.c::sys_iopl`)
///
/// ```c
/// SYSCALL_DEFINE1(iopl, unsigned int, level)
/// {
///     if (level > 3) return -EINVAL;
///     if (level > old) {
///         if (!capable(CAP_SYS_RAWIO))
///             return -EPERM;
///     }
///     ...
/// }
/// ```
///
/// Linux does NOT unconditionally return `ENOSYS` — that was a
/// misreading in pre-Phase-180 comments.  iopl remains functional on
/// x86 (though deprecated in favour of ioperm).  The cap check fires
/// only on a *raise*: lowering to level 0, or re-asserting the
/// current level, never needs `CAP_SYS_RAWIO`.
///
/// ## Our model (Phase 180)
///
/// Our microkernel never grants any IOPL level to userspace (drivers
/// live in their own process per design.txt — they don't run with
/// CPL 3 + IOPL>0).  The notional "current level" is therefore
/// always 0.  Under that model:
///
/// 1. `level < 0 || level > 3`        → `EINVAL`  (range)
/// 2. `level == 0`                    → `0`       (no-op release;
///                                                 no cap required)
/// 3. `level > 0 && !CAP_SYS_RAWIO`   → `EPERM`   (Linux's `level >
///                                                 old` branch)
/// 4. `level > 0` with cap            → `ENOSYS`  (we have no
///                                                 IO-bitmap install
///                                                 path; callers fall
///                                                 back to ioperm /
///                                                 character devices)
///
/// Pre-Phase-180 we returned `ENOSYS` unconditionally for any valid
/// `level`, which conflated the "no backend" case with the
/// "unprivileged caller" case and let unprivileged programs probe
/// for privileged behaviour without the EPERM signal Linux gives.
///
/// We treat negative levels (which would have the sign bit set if cast
/// to `unsigned`) as `EINVAL` to be defensive — Linux's `unsigned int`
/// parameter would wrap them to huge positives that fail the `> 3`
/// check anyway.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iopl(level: i32) -> i32 {
    if !(0..=IOPL_LEVEL_MAX).contains(&level) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Phase 180: level == 0 is a "release any IOPL grant" call.  Our
    // notional current level is 0, so this is a no-op — and Linux's
    // `level > old` cap check never fires.  Succeed without probing
    // CAP_SYS_RAWIO so unprivileged code can defensively call
    // `iopl(0)` (e.g. on shutdown paths) without spurious EPERM.
    if level == 0 {
        return 0;
    }
    // Phase 180: level > 0 is a *raise* relative to our current 0.
    // Linux's `if (level > old) capable(CAP_SYS_RAWIO)` gates this;
    // mirror it so unprivileged callers get EPERM (matching what
    // dosemu2 / X servers / DOS emulators see on real Linux when
    // they're missing the cap).
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_RAWIO) {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    // Cap held but no IO-bitmap install path exists in our
    // microkernel (drivers run in userspace per design.txt) — surface
    // ENOSYS so callers fall back.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Inline port I/O (x86-specific)
//
// On a real x86 kernel these would use `in`/`out` instructions.
// These stubs are provided so code that references them compiles.
// ---------------------------------------------------------------------------

/// Read a byte from an I/O port.
///
/// Stub — returns `0xFF`.
#[inline]
pub fn inb(_port: u16) -> u8 {
    0xFF
}

/// Read a word (16-bit) from an I/O port.
///
/// Stub — returns `0xFFFF`.
#[inline]
pub fn inw(_port: u16) -> u16 {
    0xFFFF
}

/// Read a dword (32-bit) from an I/O port.
///
/// Stub — returns `0xFFFF_FFFF`.
#[inline]
pub fn inl(_port: u16) -> u32 {
    0xFFFF_FFFF
}

/// Write a byte to an I/O port.
///
/// Stub — no-op.
#[inline]
pub fn outb(_value: u8, _port: u16) {}

/// Write a word (16-bit) to an I/O port.
///
/// Stub — no-op.
#[inline]
pub fn outw(_value: u16, _port: u16) {}

/// Write a dword (32-bit) to an I/O port.
///
/// Stub — no-op.
#[inline]
pub fn outl(_value: u32, _port: u16) {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ioperm validation ----

    #[test]
    fn test_ioperm_zero_num_einval() {
        // Linux's `(from + num <= from)` check rejects num == 0 with
        // EINVAL — the `<=` (not `<`) is the operative bit.  A zero-
        // length range is treated as a malformed argument, *not* a
        // no-op success.
        errno::set_errno(0);
        let r = ioperm(0x3F8, 0, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_zero_num_zero_from_einval() {
        // from == 0 + num == 0: still `from + num == from`, still EINVAL.
        errno::set_errno(0);
        let r = ioperm(0, 0, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_range_overflow_einval() {
        // from + num overflows u64.
        errno::set_errno(errno::EBADF);
        let r = ioperm(u64::MAX - 4, 16, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_range_beyond_64k_einval() {
        // from + num <= u64::MAX but > 65536.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0xFFF0, 0x20, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_exactly_64k_ok() {
        // from + num == 65536 is the boundary case and must be accepted.
        // turn_on=1 with CAP_SYS_RAWIO held (Phase 178, host default)
        // → ENOSYS (no IO-bitmap backend), not EINVAL.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0, IO_BITMAP_BITS, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_ioperm_serial_port_turn_on_enosys() {
        // Classic case: `ioperm(0x3F8, 8, 1)` for COM1. Range is fine,
        // CAP_SYS_RAWIO held (Phase 178) → ENOSYS (no IO-bitmap
        // backend).  Dropping the cap restores EPERM (see
        // ioperm_cap_phase178).
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x3F8, 8, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_ioperm_turn_off_valid_range_success() {
        // Clearing access on a well-formed range is harmless (we never
        // granted any), so it succeeds.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x3F8, 8, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn test_ioperm_turn_off_with_arbitrary_truthy_value() {
        // Any non-zero turn_on is treated as "grant access" → reaches
        // the cap-gated path.  With CAP_SYS_RAWIO held (Phase 178,
        // host default) → ENOSYS.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x3F8, 8, 42);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ---- iopl validation ----

    #[test]
    fn test_iopl_level_too_high_einval() {
        errno::set_errno(errno::EBADF);
        let r = iopl(4);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_iopl_negative_einval() {
        errno::set_errno(errno::EBADF);
        let r = iopl(-1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_iopl_int_min_einval() {
        errno::set_errno(errno::EBADF);
        let r = iopl(i32::MIN);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_iopl_level_zero_is_noop_success() {
        // Phase 180: level==0 is a release; succeeds without cap and
        // without setting errno (POSIX successful-call rule).
        errno::set_errno(errno::EBADF);
        let r = iopl(0);
        assert_eq!(r, 0);
        // Errno must be untouched on success.
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_iopl_each_raise_level_reaches_enosys_with_cap() {
        // Phase 180: levels 1..=3 are raises; with CAP_SYS_RAWIO held
        // (the host-build default) they pass the cap probe and fall
        // through to ENOSYS (no IO-bitmap install path).
        for level in 1..=3 {
            errno::set_errno(errno::EBADF);
            let r = iopl(level);
            assert_eq!(r, -1, "level={level}");
            assert_eq!(errno::get_errno(), errno::ENOSYS, "level={level}");
        }
    }

    // ---- Real-world workflow tests ----

    #[test]
    fn test_xorg_serial_port_init_workflow() {
        // Xorg's old `xf86-input-mouse` driver and various keyboard
        // drivers call `ioperm(0x60, 16, 1)` to claim PS/2 controller
        // ports at startup.  With CAP_SYS_RAWIO held (Phase 178) we
        // return ENOSYS — Xorg's fallback path treats ENOSYS the same
        // as EPERM (both surface as "no port-IO available", driving
        // the /dev/input/event* path).  The EPERM-on-no-cap variant
        // is covered in ioperm_cap_phase178.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x60, 16, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_dosemu2_iopl_probe_workflow() {
        // dosemu2 calls `iopl(3)` at startup to see if it can run DOS
        // programs with direct port access. On ENOSYS, it falls back
        // to the v86 emulation path.
        errno::set_errno(errno::EBADF);
        let r = iopl(3);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_setpci_disable_then_reenable_workflow() {
        // Some hot-plug PCI tools call `ioperm(port, n, 0)` to drop
        // their port window after use. That's a valid range with
        // turn_on=0 and must succeed.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0xCF8, 8, 0); // PCI config-space ports
        assert_eq!(r, 0);
    }

    #[test]
    fn test_legacy_parallel_port_lp0_workflow() {
        // Old `parport` driver in userspace mode calls
        // `ioperm(0x378, 3, 1)` for LPT1.  Phase 178: with
        // CAP_SYS_RAWIO held (host default) we return ENOSYS;
        // parport's failure path treats ENOSYS and EPERM the same
        // (falls back to /dev/parport0 character device or refuses
        // to drive the printer).
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x378, 3, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // ---- errno-preserved-on-success regression ----

    #[test]
    fn test_ioperm_success_does_not_clobber_errno() {
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x3F8, 8, 0);
        assert_eq!(r, 0);
        // POSIX: successful syscall must not touch errno.
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // ---- Inline port I/O preserved ----

    #[test]
    fn test_inb_stub() {
        assert_eq!(inb(0x3F8), 0xFF);
    }

    #[test]
    fn test_inw_stub() {
        assert_eq!(inw(0x3F8), 0xFFFF);
    }

    #[test]
    fn test_inl_stub() {
        assert_eq!(inl(0x3F8), 0xFFFF_FFFF);
    }

    #[test]
    fn test_outb_stub() {
        outb(0x42, 0x3F8); // Should not panic.
    }

    #[test]
    fn test_outw_stub() {
        outw(0x4242, 0x3F8); // Should not panic.
    }

    #[test]
    fn test_outl_stub() {
        outl(0x42424242, 0x3F8); // Should not panic.
    }

    #[test]
    fn test_io_bitmap_bits_value() {
        assert_eq!(IO_BITMAP_BITS, 65536);
    }

    #[test]
    fn test_iopl_level_max_is_three() {
        assert_eq!(IOPL_LEVEL_MAX, 3);
    }

    // ------------------------------------------------------------------
    // Phase 127: ioperm zero-num parity with Linux's `<=` clause
    // ------------------------------------------------------------------
    //
    // Linux's arch/x86/kernel/ioport.c::ksys_ioperm uses:
    //     if ((from + num <= from) || (from + num > IO_BITMAP_BITS))
    //         return -EINVAL;
    // The `<=` (not `<`) folds the num==0 case in with overflow
    // detection, so a zero-length range is malformed.  Earlier phases
    // had a `num == 0 → return 0` short-circuit that diverged from
    // this; these tests pin down the corrected behaviour.

    #[test]
    fn test_ioperm_phase127_zero_num_turn_on_zero_einval() {
        // num=0 with turn_on=0 — pre-fix this was a "harmless no-op
        // success", but Linux rejects it.
        errno::set_errno(0);
        let r = ioperm(0x3F8, 0, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_phase127_zero_num_turn_on_one_einval() {
        // num=0 with turn_on=1 — EINVAL fires first (range check
        // precedes capability check in Linux's order).
        errno::set_errno(0);
        let r = ioperm(0x3F8, 0, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_phase127_zero_num_high_from_einval() {
        // num=0 with from at the upper edge of the port space.  Still
        // EINVAL — the range-degenerate clause fires before the
        // upper-bound clause.
        errno::set_errno(0);
        let r = ioperm(IO_BITMAP_BITS - 1, 0, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_phase127_zero_num_from_above_64k_einval() {
        // num=0 with from > IO_BITMAP_BITS — *both* clauses of Linux's
        // OR would be true; we only ever surface one EINVAL.
        errno::set_errno(0);
        let r = ioperm(IO_BITMAP_BITS + 100, 0, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_phase127_zero_num_from_u64_max_einval() {
        // num=0 with from = u64::MAX — `from + 0` doesn't overflow but
        // the `<= from` clause still fires.
        errno::set_errno(0);
        let r = ioperm(u64::MAX, 0, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_phase127_num_one_still_succeeds_with_turn_off() {
        // num=1 (minimal valid length) with turn_on=0 still succeeds —
        // confirms we only rejected the degenerate num==0 case.
        errno::set_errno(0);
        let r = ioperm(0x3F8, 1, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn test_ioperm_phase127_einval_beats_eperm_for_zero_num() {
        // Validation-order parity: range check (EINVAL) fires before
        // capability check (EPERM) — same as Linux.
        errno::set_errno(0);
        let r = ioperm(0x3F8, 0, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // (Not EPERM, even though turn_on=1 would also fail later.)
    }

    #[test]
    fn test_ioperm_phase127_overflow_still_einval() {
        // Pre-existing overflow-detection branch must still work — the
        // refactor folded both cases into a single `end <= from` check.
        errno::set_errno(0);
        let r = ioperm(u64::MAX - 4, 16, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_phase127_workflow_buggy_caller_uninit_num() {
        // C code: `unsigned long n; ioperm(port, n, 1);` where n is
        // uninitialised and happens to be zero.  Pre-fix: silently
        // succeeded, masking the bug.  Post-fix: EINVAL exposes the
        // bug clearly.
        errno::set_errno(0);
        let r = ioperm(0x3F8, 0, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_phase127_workflow_loop_no_iterations() {
        // A common pattern: `for (i = 0; i < count; ++i) ioperm(base
        // + i*step, step, 1);`.  If count is computed as 0 from an
        // empty config, no iterations run — but if someone writes
        // `ioperm(base, count*step, 1)` instead, count==0 silently
        // succeeded pre-fix.  Now EINVAL surfaces the empty-config bug.
        errno::set_errno(0);
        let r = ioperm(0x3F8, 0u64, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_phase127_recovery_after_zero_num_einval() {
        // Per-call errno: an EINVAL from zero-num doesn't poison the
        // next well-formed call.
        errno::set_errno(0);
        assert_eq!(ioperm(0x3F8, 0, 1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        let r = ioperm(0x3F8, 8, 0);
        assert_eq!(r, 0);
        // errno not cleared by successful call (POSIX behaviour) — we
        // just confirm it isn't EINVAL anymore.
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn test_ioperm_phase127_boundary_num_one_from_max_einval() {
        // from = IO_BITMAP_BITS, num = 1 → end = IO_BITMAP_BITS + 1
        // → exceeds upper bound → EINVAL.  Boundary check still works.
        errno::set_errno(0);
        let r = ioperm(IO_BITMAP_BITS, 1, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ioperm_phase127_boundary_exactly_64k_still_ok() {
        // from = 0, num = IO_BITMAP_BITS → end = IO_BITMAP_BITS → not
        // greater than IO_BITMAP_BITS, and end > from → both clauses
        // false → not EINVAL.  turn_on=0 → success.  Confirms the
        // refactored `end <= from || end > IO_BITMAP_BITS` is correct
        // at the upper boundary.
        errno::set_errno(0);
        let r = ioperm(0, IO_BITMAP_BITS, 0);
        assert_eq!(r, 0);
    }

    // ======================================================================
    // Phase 178 — ioperm() CAP_SYS_RAWIO gate.
    //
    // Linux's `ksys_ioperm` (arch/x86/kernel/ioport.c) gates only the
    // grant path (turn_on != 0) on CAP_SYS_RAWIO:
    //
    //     if (turn_on && !capable(CAP_SYS_RAWIO))
    //         return -EPERM;
    //
    // Pre-Phase-178 we returned EPERM *unconditionally* on the grant
    // path — wrong, because a privileged caller (Linux: CAP_SYS_RAWIO
    // held) should reach the IO-bitmap install code.  Phase 178
    // differentiates: missing cap → EPERM; held cap → ENOSYS (no
    // IO-bitmap backend exists in our microkernel).
    //
    // EINVAL on the range check fires before the cap probe, matching
    // Linux's prologue ordering.  turn_on == 0 short-circuits to
    // success without consulting capabilities.
    //
    // CapGuard snapshot/restore pattern; --test-threads=1.
    // ======================================================================

    mod ioperm_cap_phase178 {
        use super::*;

        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) = crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_rawio() {
            use crate::sys_capability::CAP_SYS_RAWIO;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_RAWIO < 32 {
                (lo & !(1u32 << CAP_SYS_RAWIO), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_RAWIO - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed when dropping CAP_SYS_RAWIO");
            assert!(!crate::sys_capability::has_capability(CAP_SYS_RAWIO));
        }

        // -- grant path EPERM without cap ----------------------------------

        #[test]
        fn test_ioperm_phase178_grant_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            let r = ioperm(0x3F8, 8, 1);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_ioperm_phase178_grant_arbitrary_truthy_no_cap_eperm() {
            // Any non-zero turn_on enters the gated path.
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            let r = ioperm(0x60, 16, 42);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_ioperm_phase178_grant_full_range_no_cap_eperm() {
            // Full IO bitmap (the maximum legal range).
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            let r = ioperm(0, IO_BITMAP_BITS, 1);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- ordering: EINVAL beats EPERM ----------------------------------

        #[test]
        fn test_ioperm_phase178_range_overflow_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            let r = ioperm(u64::MAX - 4, 16, 1);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        #[test]
        fn test_ioperm_phase178_zero_num_beats_eperm() {
            // num == 0 → EINVAL fires before cap probe.
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            let r = ioperm(0x3F8, 0, 1);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        #[test]
        fn test_ioperm_phase178_beyond_64k_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            let r = ioperm(0xFFF0, 0x20, 1);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- turn_on == 0 bypasses the gate --------------------------------

        /// turn_on == 0 on a valid range succeeds even without
        /// CAP_SYS_RAWIO — Linux's `if (turn_on)` guard around the
        /// `capable()` call.
        #[test]
        fn test_ioperm_phase178_turn_off_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            let r = ioperm(0x3F8, 8, 0);
            assert_eq!(r, 0);
        }

        /// turn_on == 0 with EINVAL-shape range still EINVALs (range
        /// check runs before turn_on short-circuit).
        #[test]
        fn test_ioperm_phase178_turn_off_bad_range_einval() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            let r = ioperm(0, 0, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- cap-held sentinel ---------------------------------------------

        /// With CAP_SYS_RAWIO held, the grant path reaches ENOSYS
        /// (no IO-bitmap backend).  Proves the gate is the only thing
        /// blocking the path.
        #[test]
        fn test_ioperm_phase178_grant_with_cap_reaches_enosys() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_RAWIO,
            ));
            errno::set_errno(0);
            let r = ioperm(0x3F8, 8, 1);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- workflow: drop → EPERM, restore → ENOSYS ----------------------

        #[test]
        fn test_ioperm_phase178_drop_then_restore_workflow() {
            {
                let _g = CapGuard::snapshot();
                drop_cap_sys_rawio();
                errno::set_errno(0);
                let r = ioperm(0x3F8, 8, 1);
                assert_eq!(r, -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_RAWIO,
            ));
            errno::set_errno(0);
            let r = ioperm(0x3F8, 8, 1);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- no-side-effect loop -------------------------------------------

        #[test]
        fn test_ioperm_phase178_repeated_grant_no_cap_stable() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            for _ in 0..16 {
                errno::set_errno(0);
                let r = ioperm(0x3F8, 8, 1);
                assert_eq!(r, -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
        }

        // -- recovery: turn_off then turn_on with no cap -------------------

        /// turn_off succeeds without cap; subsequent turn_on still
        /// EPERMs — clearing access doesn't grant any latent privilege.
        #[test]
        fn test_ioperm_phase178_turn_off_does_not_unlock_grant_path() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(ioperm(0x3F8, 8, 0), 0);
            errno::set_errno(0);
            let r = ioperm(0x3F8, 8, 1);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- buggy-caller --------------------------------------------------

        /// A caller passes turn_on=1 with an overflow range and no
        /// cap: must see EINVAL (range bug), not EPERM (cap bug) —
        /// matches Linux's prologue order.
        #[test]
        fn test_ioperm_phase178_buggy_caller_overflow_diagnosed_first() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            let r = ioperm(u64::MAX, 1, 1);
            assert_eq!(r, -1);
            assert_eq!(
                errno::get_errno(),
                errno::EINVAL,
                "range bug must be diagnosed before cap-lack"
            );
        }

        // -- errno-preserved-on-success ------------------------------------

        /// Successful turn_off with no cap must not clobber errno.
        #[test]
        fn test_ioperm_phase178_turn_off_no_cap_preserves_errno() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(errno::EBADF);
            let r = ioperm(0x3F8, 8, 0);
            assert_eq!(r, 0);
            assert_eq!(errno::get_errno(), errno::EBADF);
        }
    }

    // ----------------------------------------------------------------------
    // Phase 180: iopl — CAP_SYS_RAWIO gate on level-raise +
    // unconditional ENOSYS for level==0 success.
    //
    // Pre-Phase-180 behaviour: iopl returned ENOSYS for every valid
    // level (0..=3) regardless of capability state.  That conflated
    // two distinct error modes:
    //   - "level 0 release" should be a no-op SUCCESS;
    //   - "level > 0 raise without CAP_SYS_RAWIO" is EPERM.
    // Returning ENOSYS in both cases lets unprivileged code probe
    // privileged behaviour and misleads userspace fallback paths
    // (dosemu2, X11 legacy port-IO clients) into thinking the syscall
    // is wholly absent rather than just unavailable to them.
    //
    // Linux semantics (arch/x86/kernel/ioport.c::sys_iopl):
    //     if (level > 3)            return -EINVAL;
    //     if (level > old) {
    //         if (!capable(CAP_SYS_RAWIO))
    //             return -EPERM;
    //     }
    //     /* install IO-bitmap ... */
    //     return 0;
    // Our notional current-level is always 0 (we never grant IOPL),
    // so `level > old` collapses to `level > 0`.
    //
    // Implementation:
    //   1. range check (EINVAL) — already in place;
    //   2. level == 0 → return 0 (no cap probe);
    //   3. level > 0 without CAP_SYS_RAWIO → EPERM;
    //   4. level > 0 with cap → ENOSYS (no IO-bitmap backend).
    // ----------------------------------------------------------------------

    mod iopl_cap_phase180 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase 178.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) = crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap_sys_rawio() {
            use crate::sys_capability::CAP_SYS_RAWIO;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_RAWIO < 32 {
                (lo & !(1u32 << CAP_SYS_RAWIO), hi)
            } else {
                (lo, hi & !(1u32 << (CAP_SYS_RAWIO - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed when dropping CAP_SYS_RAWIO");
            assert!(!crate::sys_capability::has_capability(CAP_SYS_RAWIO));
        }

        // -- Per-error-class ----------------------------------------------

        /// `iopl(1)` without CAP_SYS_RAWIO returns -1/EPERM.  The
        /// smallest raise hits Linux's `level > old` branch.
        #[test]
        fn test_iopl_phase180_level_one_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// `iopl(3)` (the max raise) without cap is also EPERM.
        #[test]
        fn test_iopl_phase180_level_three_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(3), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// `iopl(2)` (intermediate) without cap is EPERM too —
        /// confirms the gate fires on every raise, not just the
        /// minimum or maximum.
        #[test]
        fn test_iopl_phase180_level_two_no_cap_returns_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(2), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Errno is specifically EPERM, not EACCES (that's the
        /// setpriority gate) and not ENOSYS (that's the backend
        /// fall-through).
        #[test]
        fn test_iopl_phase180_errno_is_eperm_not_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(1), -1);
            assert_ne!(errno::get_errno(), errno::ENOSYS);
            assert_ne!(errno::get_errno(), errno::EACCES);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Ordering matrix ----------------------------------------------

        /// EINVAL on out-of-range level beats the cap probe.  A
        /// no-cap caller passing level=4 sees EINVAL, not EPERM.
        #[test]
        fn test_iopl_phase180_einval_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(4), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// EINVAL on negative level beats EPERM too.
        #[test]
        fn test_iopl_phase180_einval_negative_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(-1), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// `iopl(0)` — release/no-op — succeeds without cap.  Linux's
        /// `level > old` branch is false (0 > 0 is false) so the cap
        /// check never fires.
        #[test]
        fn test_iopl_phase180_level_zero_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(errno::EBADF);
            assert_eq!(iopl(0), 0);
            // Errno must be untouched on success.
            assert_eq!(errno::get_errno(), errno::EBADF);
        }

        // -- Workflow -----------------------------------------------------

        /// Daemon workflow: a process raises iopl with cap held
        /// (reaches ENOSYS — the backend signal), then drops the cap
        /// and re-tries (EPERM), then releases with iopl(0) (succeeds
        /// even without cap).  Models a privileged tool dropping
        /// caps mid-flight.
        #[test]
        fn test_iopl_phase180_workflow_raise_drop_cap_raise_release() {
            let _g = CapGuard::snapshot();
            // With cap: raise reaches ENOSYS.
            errno::set_errno(0);
            assert_eq!(iopl(3), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
            // Drop cap and raise again: EPERM.
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(2), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Release: succeeds even without cap.
            errno::set_errno(errno::EBADF);
            assert_eq!(iopl(0), 0);
            assert_eq!(errno::get_errno(), errno::EBADF);
        }

        /// dosemu2's startup probe: `iopl(3)`.  With cap held it
        /// reaches ENOSYS (dosemu2 falls back to v86 emulation); with
        /// cap dropped it gets EPERM (dosemu2 also falls back).
        /// Same end-user outcome via different errnos — both must be
        /// distinguishable for strace-style tooling.
        #[test]
        fn test_iopl_phase180_workflow_dosemu_probe_distinguishes_errno() {
            let _g = CapGuard::snapshot();
            // With cap: ENOSYS.
            errno::set_errno(0);
            assert_eq!(iopl(3), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
            // Without cap: EPERM.
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(3), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Buggy caller -------------------------------------------------

        /// A no-cap caller passing i32::MAX (a wildly invalid level)
        /// must hit EINVAL — the range check fires before the cap
        /// probe, so we don't misdiagnose an obvious bug as a
        /// permission problem.
        #[test]
        fn test_iopl_phase180_buggy_caller_int_max_no_cap_einval() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(i32::MAX), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// A no-cap caller passing i32::MIN likewise hits EINVAL via
        /// the lower bound, not EPERM.
        #[test]
        fn test_iopl_phase180_buggy_caller_int_min_no_cap_einval() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(i32::MIN), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- Recovery -----------------------------------------------------

        /// After an EPERM rejection, restoring CAP_SYS_RAWIO lets the
        /// same iopl(N>0) call reach ENOSYS.  Confirms dynamic cap
        /// evaluation per call.
        #[test]
        fn test_iopl_phase180_recovery_restore_cap_reaches_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            assert_eq!(iopl(3), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // Restore caps.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(crate::sys_capability::capset(&mut hdr, data.as_ptr()), 0,);
            errno::set_errno(0);
            assert_eq!(iopl(3), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- No-side-effect ----------------------------------------------

        /// `iopl(0)` success preserves errno — POSIX rule.
        #[test]
        fn test_iopl_phase180_level_zero_success_preserves_errno() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(errno::EBADF);
            assert_eq!(iopl(0), 0);
            assert_eq!(errno::get_errno(), errno::EBADF);
        }

        // -- Sentinel -----------------------------------------------------

        /// With CAP_SYS_RAWIO held (default), `iopl(3)` reaches
        /// ENOSYS — confirms the privileged path is unbroken.
        #[test]
        fn test_iopl_phase180_sentinel_with_cap_reaches_enosys() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_RAWIO,
            ));
            errno::set_errno(0);
            assert_eq!(iopl(3), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- Cross-checks -------------------------------------------------

        /// Dropping CAP_SYS_RAWIO must not affect other caps —
        /// CAP_SYS_ADMIN remains held, so ioperm's other check paths
        /// behave normally.  Defends against a stray bit-clear
        /// regression.
        #[test]
        fn test_iopl_phase180_drop_sys_rawio_leaves_other_caps() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_RAWIO,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_BOOT,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_NICE,
            ));
        }

        /// Cross-check vs ioperm (Phase 178): both gate on
        /// CAP_SYS_RAWIO, both return EPERM when the cap is missing
        /// and the operation is a real grant.  Pin that they remain
        /// consistent — if one gate regresses, the other still
        /// catches the violation.
        #[test]
        fn test_iopl_phase180_consistent_with_ioperm_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            errno::set_errno(0);
            // iopl raise without cap → EPERM.
            assert_eq!(iopl(1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // ioperm turn_on without cap → EPERM.
            errno::set_errno(0);
            assert_eq!(ioperm(0x3F8, 8, 1), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Cross-check the "no-op release" semantics: just as
        /// `ioperm(_, _, 0)` short-circuits to success without a cap
        /// probe (Phase 178), `iopl(0)` short-circuits the same way.
        /// Symmetry guard.
        #[test]
        fn test_iopl_phase180_consistent_with_ioperm_turn_off_release() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_rawio();
            // iopl(0) — release.
            errno::set_errno(0);
            assert_eq!(iopl(0), 0);
            // ioperm(_, _, 0) — release.
            errno::set_errno(0);
            assert_eq!(ioperm(0x3F8, 8, 0), 0);
        }
    }
}

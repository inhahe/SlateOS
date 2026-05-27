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
/// Linux semantics (`arch/x86/kernel/ioport.c::sys_ioperm`):
/// - `num == 0` → success (no-op). We return 0 to match — this is the
///   one well-formed call that succeeds without needing kernel support
///   because there's nothing to actually do.
/// - `from + num` overflows or `from + num > IO_BITMAP_BITS` → EINVAL.
/// - `turn_on != 0` requires `CAP_SYS_RAWIO` on Linux → EPERM if
///   missing. In our design (no ambient root, capabilities everywhere),
///   we treat any caller as lacking the capability and return EPERM
///   for the "turn on" case. The "turn off" case (`turn_on == 0`) on a
///   well-formed range is harmless (you're clearing bits in a bitmap
///   that doesn't exist) — we return 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ioperm(from: u64, num: u64, turn_on: i32) -> i32 {
    // num == 0 is a no-op success on Linux (the loop body never runs).
    if num == 0 {
        return 0;
    }

    // Range overflow check: from + num must not wrap and must fit in the
    // 64 KiB I/O port space.
    let end = match from.checked_add(num) {
        Some(e) => e,
        None => {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    };
    if end > IO_BITMAP_BITS {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Granting access requires capability; we always lack it.
    if turn_on != 0 {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    // turn_on == 0 on a valid range: clearing access we never granted.
    // Linux treats this as a successful no-op; we do the same.
    0
}

/// Set I/O privilege level.
///
/// Sets the I/O privilege level (IOPL) of the calling process. Level
/// must be 0–3; only level 3 grants unrestricted port access on x86.
///
/// Linux semantics (`arch/x86/kernel/ioport.c::sys_iopl`):
/// - `level > 3` → EINVAL.
/// - Raising IOPL above current requires `CAP_SYS_RAWIO` → EPERM.
/// - On modern kernels (5.5+) the syscall returns ENOSYS regardless,
///   per the deprecation. We follow that: any well-formed call →
///   ENOSYS so callers (X servers, dosemu2, legacy serial tools) fall
///   back to their alternative paths.
///
/// We treat negative levels (which would have the sign bit set if cast
/// to `unsigned`) as EINVAL to be defensive.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iopl(level: i32) -> i32 {
    if !(0..=IOPL_LEVEL_MAX).contains(&level) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
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
    fn test_ioperm_zero_num_is_noop_success() {
        // num == 0 is a well-defined no-op on Linux.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x3F8, 0, 1);
        assert_eq!(r, 0);
        // Linux doesn't touch errno on success, but we shouldn't change
        // it either — the prior value (EBADF) must remain.
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_ioperm_zero_num_zero_from_success() {
        errno::set_errno(errno::EBADF);
        let r = ioperm(0, 0, 0);
        assert_eq!(r, 0);
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
        // turn_on=1 still gives EPERM (capability missing), not EINVAL.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0, IO_BITMAP_BITS, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_ioperm_serial_port_turn_on_eperm() {
        // Classic case: `ioperm(0x3F8, 8, 1)` for COM1. Range is fine,
        // but the "grant access" half requires CAP_SYS_RAWIO which we
        // never have.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x3F8, 8, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
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
        // Any non-zero turn_on is treated as "grant access" → EPERM.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x3F8, 8, 42);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
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
    fn test_iopl_each_valid_level_reaches_enosys() {
        for level in 0..=3 {
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
        // ports at startup, then fall back to /dev/input/event* on
        // EPERM. We give them EPERM.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x60, 16, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
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
        // `ioperm(0x378, 3, 1)` for LPT1. EPERM → falls back to
        // /dev/parport0 character device or refuses to drive the
        // printer.
        errno::set_errno(errno::EBADF);
        let r = ioperm(0x378, 3, 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
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
}

//! `<sys/io.h>` — I/O port access for x86.
//!
//! Provides `ioperm()` and `iopl()` for controlling access to
//! x86 I/O ports, plus inline port I/O functions.

use crate::errno;

// ---------------------------------------------------------------------------
// Port access permission functions
// ---------------------------------------------------------------------------

/// Set I/O port permissions.
///
/// Enables or disables access to the I/O port range
/// `[from, from + num)`.
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ioperm(from: u64, num: u64, turn_on: i32) -> i32 {
    let _ = (from, num, turn_on);
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Set I/O privilege level.
///
/// Sets the I/O privilege level (IOPL) of the calling process.
/// Level must be 0-3 (only 3 grants unrestricted port access).
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iopl(level: i32) -> i32 {
    let _ = level;
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

    #[test]
    fn test_ioperm_stub() {
        assert_eq!(ioperm(0x3F8, 8, 1), -1);
    }

    #[test]
    fn test_iopl_stub() {
        assert_eq!(iopl(3), -1);
    }

    #[test]
    fn test_iopl_levels() {
        // All levels should fail (stub).
        for level in 0..=3 {
            assert_eq!(iopl(level), -1);
        }
    }

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
}

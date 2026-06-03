//! `<linux/pidfd.h>` — `pidfd_open(2)` / `pidfd_send_signal(2)` flags.
//!
//! pidfds are FD-based handles on processes that don't race with PID
//! reuse. The flags below are accepted by `pidfd_open()`,
//! `pidfd_send_signal()`, and `pidfd_getfd()`. They are consumed by
//! systemd, dbus-broker, the Rust `process` crate's pidfd backend,
//! and Go's `pidfd` package.

// ---------------------------------------------------------------------------
// pidfd_open() flag bits
// ---------------------------------------------------------------------------

/// Open the pidfd for a thread (not just a process leader).
/// Available from Linux 6.5; older kernels reject it.
pub const PIDFD_THREAD: u32 = 1 << 0;
/// Non-blocking pidfd — `poll()` will report `POLLIN` immediately if
/// the target has already exited.
pub const PIDFD_NONBLOCK: u32 = 0x0000_0800;

// ---------------------------------------------------------------------------
// pidfd_send_signal() flags (all reserved as of 6.x; included as a
// future-proof zero default)
// ---------------------------------------------------------------------------

/// Flags must currently be 0 for `pidfd_send_signal()`.
pub const PIDFD_SIGNAL_FLAGS_RESERVED: u32 = 0;

// ---------------------------------------------------------------------------
// pidfd_getfd() flag bits
// ---------------------------------------------------------------------------

/// `PIDFD_GETFD_FLAGS_RESERVED` — flags must be 0 in kernel 5.6..6.x.
pub const PIDFD_GETFD_FLAGS_RESERVED: u32 = 0;

// ---------------------------------------------------------------------------
// File-descriptor close-on-exec masking helper
// ---------------------------------------------------------------------------

/// Bit-mask of every flag pidfd_open() understands. Userspace can
/// `flags & PIDFD_OPEN_VALID_FLAGS` to detect unknown bits before
/// invoking the syscall.
pub const PIDFD_OPEN_VALID_FLAGS: u32 = PIDFD_THREAD | PIDFD_NONBLOCK;

// ---------------------------------------------------------------------------
// poll() events on a pidfd
// ---------------------------------------------------------------------------

/// A pidfd is `POLLIN`-ready when the process has exited.
pub const PIDFD_POLLIN_ON_EXIT: u32 = 0x0001;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_flag_bits_distinct_pow2() {
        assert!(PIDFD_THREAD.is_power_of_two());
        assert!(PIDFD_NONBLOCK.is_power_of_two());
        assert_ne!(PIDFD_THREAD, PIDFD_NONBLOCK);
    }

    #[test]
    fn test_nonblock_matches_o_nonblock_value() {
        // PIDFD_NONBLOCK is defined to equal O_NONBLOCK on every arch
        // (0o4000 == 0x800).
        assert_eq!(PIDFD_NONBLOCK, 0x800);
    }

    #[test]
    fn test_reserved_flags_are_zero() {
        assert_eq!(PIDFD_SIGNAL_FLAGS_RESERVED, 0);
        assert_eq!(PIDFD_GETFD_FLAGS_RESERVED, 0);
    }

    #[test]
    fn test_valid_mask_covers_all_open_flags() {
        // VALID_FLAGS must include every accepted flag and nothing else.
        assert_eq!(
            PIDFD_OPEN_VALID_FLAGS,
            PIDFD_THREAD | PIDFD_NONBLOCK
        );
        // Any bit not in VALID_FLAGS must be rejected at the syscall;
        // verify by checking a random unrelated bit.
        assert_eq!(PIDFD_OPEN_VALID_FLAGS & 0x4000_0000, 0);
    }

    #[test]
    fn test_pollin_event_bit() {
        assert!(PIDFD_POLLIN_ON_EXIT.is_power_of_two());
    }
}

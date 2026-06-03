//! `<linux/seccomp.h>` — Additional seccomp constants (batch 3).
//!
//! Supplementary seccomp constants covering notification flags,
//! addfd flags, and seccomp user notification fields.

// ---------------------------------------------------------------------------
// Seccomp notification flags (SECCOMP_USER_NOTIF_FLAG_*)
// ---------------------------------------------------------------------------

/// Notification: continue the syscall.
pub const SECCOMP_USER_NOTIF_FLAG_CONTINUE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Seccomp addfd flags (SECCOMP_ADDFD_FLAG_*)
// ---------------------------------------------------------------------------

/// Replace the file descriptor in the target.
pub const SECCOMP_ADDFD_FLAG_SETFD: u32 = 1 << 0;
/// Send signal after adding fd.
pub const SECCOMP_ADDFD_FLAG_SEND: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Seccomp filter flags (SECCOMP_FILTER_FLAG_*)
// ---------------------------------------------------------------------------

/// Log all filtered syscalls.
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 1 << 1;
/// Disable speculative store bypass.
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 1 << 2;
/// Synchronize new filter with all threads.
pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1 << 0;
/// Create new listener fd.
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 1 << 3;
/// Error on TSYNC failure.
pub const SECCOMP_FILTER_FLAG_TSYNC_ESRCH: u32 = 1 << 4;
/// Wait for killable.
pub const SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Seccomp return action data mask
// ---------------------------------------------------------------------------

/// Data portion of return value (lower 16 bits).
pub const SECCOMP_RET_DATA_MASK: u32 = 0x0000FFFF;
/// Action portion of return value (upper 16 bits).
pub const SECCOMP_RET_ACTION_MASK: u32 = 0x7FFF0000;
/// Full action + data mask.
pub const SECCOMP_RET_ACTION_FULL: u32 = 0x7FFFFFFF;

// ---------------------------------------------------------------------------
// Seccomp notification sizes
// ---------------------------------------------------------------------------

/// Size of seccomp_notif structure.
pub const SECCOMP_NOTIF_SIZE: u32 = 80;
/// Size of seccomp_notif_resp structure.
pub const SECCOMP_NOTIF_RESP_SIZE: u32 = 24;
/// Size of seccomp_notif_addfd structure.
pub const SECCOMP_NOTIF_ADDFD_SIZE: u32 = 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notif_flag() {
        assert_eq!(SECCOMP_USER_NOTIF_FLAG_CONTINUE, 1);
    }

    #[test]
    fn test_addfd_flags_power_of_two() {
        assert!(SECCOMP_ADDFD_FLAG_SETFD.is_power_of_two());
        assert!(SECCOMP_ADDFD_FLAG_SEND.is_power_of_two());
    }

    #[test]
    fn test_addfd_flags_no_overlap() {
        assert_eq!(SECCOMP_ADDFD_FLAG_SETFD & SECCOMP_ADDFD_FLAG_SEND, 0);
    }

    #[test]
    fn test_filter_flags_power_of_two() {
        let flags = [
            SECCOMP_FILTER_FLAG_TSYNC,
            SECCOMP_FILTER_FLAG_LOG,
            SECCOMP_FILTER_FLAG_SPEC_ALLOW,
            SECCOMP_FILTER_FLAG_NEW_LISTENER,
            SECCOMP_FILTER_FLAG_TSYNC_ESRCH,
            SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_filter_flags_no_overlap() {
        let flags = [
            SECCOMP_FILTER_FLAG_TSYNC,
            SECCOMP_FILTER_FLAG_LOG,
            SECCOMP_FILTER_FLAG_SPEC_ALLOW,
            SECCOMP_FILTER_FLAG_NEW_LISTENER,
            SECCOMP_FILTER_FLAG_TSYNC_ESRCH,
            SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ret_masks() {
        assert_eq!(SECCOMP_RET_DATA_MASK & SECCOMP_RET_ACTION_MASK, 0);
    }

    #[test]
    fn test_notif_sizes() {
        assert!(SECCOMP_NOTIF_SIZE > 0);
        assert!(SECCOMP_NOTIF_RESP_SIZE > 0);
        assert!(SECCOMP_NOTIF_ADDFD_SIZE > 0);
    }
}

//! `<linux/seccomp.h>` — Seccomp BPF filter flag constants.
//!
//! Seccomp filters use BPF programs to restrict which syscalls a
//! process can execute. These constants control filter installation
//! behavior and the notification mechanism for userspace handling.

// ---------------------------------------------------------------------------
// seccomp() operation codes
// ---------------------------------------------------------------------------

/// Set seccomp mode to strict.
pub const SECCOMP_SET_MODE_STRICT: u32 = 0;
/// Install a BPF filter.
pub const SECCOMP_SET_MODE_FILTER: u32 = 1;
/// Get notification fd for user-space handling.
pub const SECCOMP_GET_ACTION_AVAIL: u32 = 2;
/// Get listener fd for notifications.
pub const SECCOMP_GET_NOTIF_SIZES: u32 = 3;

// ---------------------------------------------------------------------------
// seccomp filter flags (SECCOMP_FILTER_FLAG_*)
// ---------------------------------------------------------------------------

/// Synchronize all threads in thread group.
pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1 << 0;
/// Log all non-ALLOW actions.
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 1 << 1;
/// Disable speculative store bypass.
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 1 << 2;
/// Deliver SIGSYS on action instead of killing.
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 1 << 3;
/// TSYNC must succeed for all threads or fail.
pub const SECCOMP_FILTER_FLAG_TSYNC_ESRCH: u32 = 1 << 4;
/// Wait for notification response before continuing.
pub const SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Seccomp notification flags
// ---------------------------------------------------------------------------

/// Notification response: continue syscall.
pub const SECCOMP_USER_NOTIF_FLAG_CONTINUE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Seccomp notification ioctl commands
// ---------------------------------------------------------------------------

/// Receive notification.
pub const SECCOMP_IOCTL_NOTIF_RECV: u32 = 0xC0502100;
/// Send notification response.
pub const SECCOMP_IOCTL_NOTIF_SEND: u32 = 0xC0182101;
/// Check if notification ID is still valid.
pub const SECCOMP_IOCTL_NOTIF_ID_VALID: u32 = 0x40082102;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operations_distinct() {
        let ops = [
            SECCOMP_SET_MODE_STRICT,
            SECCOMP_SET_MODE_FILTER,
            SECCOMP_GET_ACTION_AVAIL,
            SECCOMP_GET_NOTIF_SIZES,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
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
            assert!(f.is_power_of_two());
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
    fn test_strict_is_zero() {
        assert_eq!(SECCOMP_SET_MODE_STRICT, 0);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            SECCOMP_IOCTL_NOTIF_RECV,
            SECCOMP_IOCTL_NOTIF_SEND,
            SECCOMP_IOCTL_NOTIF_ID_VALID,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}

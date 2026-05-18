//! `<linux/seccomp.h>` — Seccomp filter action and flag constants.
//!
//! Seccomp (secure computing mode) restricts which syscalls a
//! process can invoke.  In filter mode (BPF), these constants
//! define the actions the kernel takes on a syscall match and
//! the flags controlling filter installation.

// ---------------------------------------------------------------------------
// Seccomp operations (for prctl/seccomp syscall)
// ---------------------------------------------------------------------------

/// Set seccomp mode (strict or filter).
pub const SECCOMP_SET_MODE_STRICT: u32 = 0;
/// Install a seccomp BPF filter.
pub const SECCOMP_SET_MODE_FILTER: u32 = 1;
/// Get current notification FD.
pub const SECCOMP_GET_ACTION_AVAIL: u32 = 2;
/// Get notification ID.
pub const SECCOMP_GET_NOTIF_SIZES: u32 = 3;

// ---------------------------------------------------------------------------
// Seccomp filter flags (for SECCOMP_SET_MODE_FILTER)
// ---------------------------------------------------------------------------

/// Synchronize all threads to the new filter.
pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1 << 0;
/// Log all filtered actions.
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 1 << 1;
/// Disable speculative store bypass (Spectre mitigation).
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 1 << 2;
/// Create a new user notification listener.
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 1 << 3;
/// Synchronize all threads, fail if any cannot.
pub const SECCOMP_FILTER_FLAG_TSYNC_ESRCH: u32 = 1 << 4;
/// Wait for notification handling to complete.
pub const SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Seccomp return actions (in BPF return value)
// ---------------------------------------------------------------------------

/// Kill the offending thread.
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x00000000;
/// Kill the offending process (all threads).
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x80000000;
/// Send a SIGSYS to the thread and allow the syscall to fail.
pub const SECCOMP_RET_TRAP: u32 = 0x00030000;
/// Return an errno value to the caller.
pub const SECCOMP_RET_ERRNO: u32 = 0x00050000;
/// Forward to a userspace notification.
pub const SECCOMP_RET_USER_NOTIF: u32 = 0x7FC00000;
/// Pass to a ptrace tracer for decision.
pub const SECCOMP_RET_TRACE: u32 = 0x7FF00000;
/// Log the syscall and allow it.
pub const SECCOMP_RET_LOG: u32 = 0x7FFC0000;
/// Allow the syscall.
pub const SECCOMP_RET_ALLOW: u32 = 0x7FFF0000;

/// Mask for the action field (high 16 bits).
pub const SECCOMP_RET_ACTION_FULL: u32 = 0xFFFF0000;
/// Mask for the data field (low 16 bits, errno value for RET_ERRNO).
pub const SECCOMP_RET_DATA: u32 = 0x0000FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operations_distinct() {
        let ops = [
            SECCOMP_SET_MODE_STRICT, SECCOMP_SET_MODE_FILTER,
            SECCOMP_GET_ACTION_AVAIL, SECCOMP_GET_NOTIF_SIZES,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_strict_is_zero() {
        assert_eq!(SECCOMP_SET_MODE_STRICT, 0);
    }

    #[test]
    fn test_filter_flags_powers_of_two() {
        let flags = [
            SECCOMP_FILTER_FLAG_TSYNC, SECCOMP_FILTER_FLAG_LOG,
            SECCOMP_FILTER_FLAG_SPEC_ALLOW,
            SECCOMP_FILTER_FLAG_NEW_LISTENER,
            SECCOMP_FILTER_FLAG_TSYNC_ESRCH,
            SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_filter_flags_no_overlap() {
        let flags = [
            SECCOMP_FILTER_FLAG_TSYNC, SECCOMP_FILTER_FLAG_LOG,
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
    fn test_ret_actions_distinct() {
        let actions = [
            SECCOMP_RET_KILL_THREAD, SECCOMP_RET_KILL_PROCESS,
            SECCOMP_RET_TRAP, SECCOMP_RET_ERRNO,
            SECCOMP_RET_USER_NOTIF, SECCOMP_RET_TRACE,
            SECCOMP_RET_LOG, SECCOMP_RET_ALLOW,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_kill_thread_is_zero() {
        assert_eq!(SECCOMP_RET_KILL_THREAD, 0);
    }

    #[test]
    fn test_action_data_masks() {
        assert_eq!(SECCOMP_RET_ACTION_FULL | SECCOMP_RET_DATA, 0xFFFFFFFF);
        assert_eq!(SECCOMP_RET_ACTION_FULL & SECCOMP_RET_DATA, 0);
    }
}

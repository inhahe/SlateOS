//! `<linux/seccomp.h>` — seccomp BPF filter constants.
//!
//! seccomp (secure computing) restricts the syscalls a process can
//! make. In strict mode only read/write/exit/sigreturn are allowed.
//! In filter mode, a BPF program inspects each syscall and its
//! arguments, returning an action (allow, kill, trap, errno, trace,
//! log, or notify). Used by containers, sandboxes (Chrome, Firefox),
//! and systemd service hardening.

// ---------------------------------------------------------------------------
// seccomp modes (PR_SET_SECCOMP argument)
// ---------------------------------------------------------------------------

/// Disabled (no filter).
pub const SECCOMP_MODE_DISABLED: u32 = 0;
/// Strict mode: only read/write/exit/sigreturn.
pub const SECCOMP_MODE_STRICT: u32 = 1;
/// Filter mode: BPF program decides.
pub const SECCOMP_MODE_FILTER: u32 = 2;

// ---------------------------------------------------------------------------
// seccomp operations (seccomp() syscall first arg)
// ---------------------------------------------------------------------------

/// Set strict mode.
pub const SECCOMP_SET_MODE_STRICT: u32 = 0;
/// Set filter mode with BPF program.
pub const SECCOMP_SET_MODE_FILTER: u32 = 1;
/// Get notification fd for user-space supervisor.
pub const SECCOMP_GET_ACTION_AVAIL: u32 = 2;
/// Get the listener notification fd.
pub const SECCOMP_GET_NOTIF_SIZES: u32 = 3;

// ---------------------------------------------------------------------------
// seccomp filter flags
// ---------------------------------------------------------------------------

/// Synchronize all threads to same filter.
pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1 << 0;
/// Log all non-ALLOW actions.
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 1 << 1;
/// Disable speculative execution when filter matches.
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 1 << 2;
/// Create a notification fd for user notification.
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 1 << 3;
/// TSYNC requires esrch on thread conflict.
pub const SECCOMP_FILTER_FLAG_TSYNC_ESRCH: u32 = 1 << 4;
/// Wait for notification killable.
pub const SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// seccomp return actions (BPF return value high 16 bits)
// ---------------------------------------------------------------------------

/// Kill the thread immediately.
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x0000_0000;
/// Kill the process (all threads).
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
/// Send SIGSYS to the thread.
pub const SECCOMP_RET_TRAP: u32 = 0x0003_0000;
/// Return errno (low 16 bits = errno value).
pub const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
/// Notify attached ptrace tracer.
pub const SECCOMP_RET_TRACE: u32 = 0x7FF0_0000;
/// Forward to user-space supervisor.
pub const SECCOMP_RET_USER_NOTIF: u32 = 0x7FC0_0000;
/// Allow but log the syscall.
pub const SECCOMP_RET_LOG: u32 = 0x7FFC_0000;
/// Allow the syscall.
pub const SECCOMP_RET_ALLOW: u32 = 0x7FFF_0000;

/// Mask for action value (high 16 bits).
pub const SECCOMP_RET_ACTION_FULL: u32 = 0xFFFF_0000;
/// Mask for data value (low 16 bits).
pub const SECCOMP_RET_DATA: u32 = 0x0000_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [SECCOMP_MODE_DISABLED, SECCOMP_MODE_STRICT, SECCOMP_MODE_FILTER];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

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
    fn test_filter_flags_no_overlap() {
        let flags = [
            SECCOMP_FILTER_FLAG_TSYNC, SECCOMP_FILTER_FLAG_LOG,
            SECCOMP_FILTER_FLAG_SPEC_ALLOW, SECCOMP_FILTER_FLAG_NEW_LISTENER,
            SECCOMP_FILTER_FLAG_TSYNC_ESRCH, SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ret_actions_distinct() {
        let actions = [
            SECCOMP_RET_KILL_THREAD, SECCOMP_RET_KILL_PROCESS,
            SECCOMP_RET_TRAP, SECCOMP_RET_ERRNO, SECCOMP_RET_TRACE,
            SECCOMP_RET_USER_NOTIF, SECCOMP_RET_LOG, SECCOMP_RET_ALLOW,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_ret_masks_complement() {
        assert_eq!(SECCOMP_RET_ACTION_FULL | SECCOMP_RET_DATA, 0xFFFF_FFFF);
        assert_eq!(SECCOMP_RET_ACTION_FULL & SECCOMP_RET_DATA, 0);
    }

    #[test]
    fn test_kill_thread_is_zero() {
        assert_eq!(SECCOMP_RET_KILL_THREAD, 0);
    }
}

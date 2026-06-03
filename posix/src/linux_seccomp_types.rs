//! `<linux/seccomp.h>` — Secure computing mode constants.
//!
//! Seccomp restricts the system calls a process can make. Mode 1
//! (strict) allows only read/write/exit/sigreturn. Mode 2 (filter)
//! uses BPF programs to allow/deny/log individual syscalls with
//! argument inspection, enabling fine-grained sandboxing.

// ---------------------------------------------------------------------------
// Seccomp modes
// ---------------------------------------------------------------------------

/// Seccomp disabled.
pub const SECCOMP_MODE_DISABLED: u32 = 0;
/// Strict mode (only read/write/_exit/sigreturn).
pub const SECCOMP_MODE_STRICT: u32 = 1;
/// Filter mode (BPF-based policy).
pub const SECCOMP_MODE_FILTER: u32 = 2;

// ---------------------------------------------------------------------------
// Seccomp filter return actions (high 16 bits of return value)
// ---------------------------------------------------------------------------

/// Kill the thread.
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x0000_0000;
/// Kill the process.
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
/// Send a SIGSYS signal.
pub const SECCOMP_RET_TRAP: u32 = 0x0003_0000;
/// Return errno to caller.
pub const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
/// Notify userspace (seccomp_notif).
pub const SECCOMP_RET_USER_NOTIF: u32 = 0x7FC0_0000;
/// Log and allow.
pub const SECCOMP_RET_LOG: u32 = 0x7FFC_0000;
/// Allow the syscall.
pub const SECCOMP_RET_ALLOW: u32 = 0x7FFF_0000;

// ---------------------------------------------------------------------------
// Seccomp operations (prctl/seccomp syscall)
// ---------------------------------------------------------------------------

/// Set seccomp mode (strict).
pub const SECCOMP_SET_MODE_STRICT: u32 = 0;
/// Set seccomp filter.
pub const SECCOMP_SET_MODE_FILTER: u32 = 1;
/// Get notification FD.
pub const SECCOMP_GET_NOTIF_SIZES: u32 = 3;
/// Get action availability.
pub const SECCOMP_GET_ACTION_AVAIL: u32 = 2;

// ---------------------------------------------------------------------------
// Seccomp filter flags
// ---------------------------------------------------------------------------

/// Allow filter to be shared with children via TSYNC.
pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1 << 0;
/// Log all filtered actions.
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 1 << 1;
/// Disable speculative store bypass.
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 1 << 2;
/// Install filter as new root.
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 1 << 3;
/// Thread sync exclusive.
pub const SECCOMP_FILTER_FLAG_TSYNC_ESRCH: u32 = 1 << 4;
/// Wait for all threads.
pub const SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            SECCOMP_MODE_DISABLED,
            SECCOMP_MODE_STRICT,
            SECCOMP_MODE_FILTER,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_ret_actions_distinct() {
        let actions = [
            SECCOMP_RET_KILL_THREAD,
            SECCOMP_RET_KILL_PROCESS,
            SECCOMP_RET_TRAP,
            SECCOMP_RET_ERRNO,
            SECCOMP_RET_USER_NOTIF,
            SECCOMP_RET_LOG,
            SECCOMP_RET_ALLOW,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

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
}

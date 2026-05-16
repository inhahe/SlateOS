//! `<linux/seccomp.h>` — seccomp (secure computing) constants.
//!
//! Provides seccomp operation constants, return values, and the
//! `seccomp()` syscall wrapper.

use crate::errno;

// ---------------------------------------------------------------------------
// Re-exports from sys_prctl where seccomp modes are already defined
// ---------------------------------------------------------------------------

pub use crate::sys_prctl::SECCOMP_MODE_DISABLED;
pub use crate::sys_prctl::SECCOMP_MODE_STRICT;
pub use crate::sys_prctl::SECCOMP_MODE_FILTER;

// ---------------------------------------------------------------------------
// seccomp() operations
// ---------------------------------------------------------------------------

/// Set the seccomp mode for the calling thread.
pub const SECCOMP_SET_MODE_STRICT: u32 = 0;
/// Install a seccomp BPF filter.
pub const SECCOMP_SET_MODE_FILTER: u32 = 1;
/// Fetch the notifier FD.
pub const SECCOMP_GET_ACTION_AVAIL: u32 = 2;
/// Get notification ID.
pub const SECCOMP_GET_NOTIF_SIZES: u32 = 3;

// ---------------------------------------------------------------------------
// seccomp filter flags
// ---------------------------------------------------------------------------

/// Log all actions except SECCOMP_RET_ALLOW.
pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1;
/// Log filtered syscalls.
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 2;
/// Disable speculative store bypass.
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 4;
/// Create a new listener (SECCOMP_RET_USER_NOTIF).
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 8;
/// Wait for filter to be installed.
pub const SECCOMP_FILTER_FLAG_TSYNC_ESRCH: u32 = 16;

// ---------------------------------------------------------------------------
// seccomp return values (lower 16 bits of BPF return)
// ---------------------------------------------------------------------------

/// Kill the task immediately.
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
/// Kill the thread immediately.
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x0000_0000;
/// Alias for `RET_KILL_THREAD`.
pub const SECCOMP_RET_KILL: u32 = SECCOMP_RET_KILL_THREAD;
/// Send SIGSYS with information.
pub const SECCOMP_RET_TRAP: u32 = 0x0003_0000;
/// Return an error to the caller.
pub const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
/// Forward to userspace notifier.
pub const SECCOMP_RET_USER_NOTIF: u32 = 0x7FC0_0000;
/// Pass to a ptrace tracer.
pub const SECCOMP_RET_TRACE: u32 = 0x7FF0_0000;
/// Log and allow the syscall.
pub const SECCOMP_RET_LOG: u32 = 0x7FFC_0000;
/// Allow the syscall.
pub const SECCOMP_RET_ALLOW: u32 = 0x7FFF_0000;

/// Mask for the return action.
pub const SECCOMP_RET_ACTION_FULL: u32 = 0xFFFF_0000;
/// Mask for the return data.
pub const SECCOMP_RET_DATA: u32 = 0x0000_FFFF;
/// Mask for the action only (no data).
pub const SECCOMP_RET_ACTION: u32 = 0x7FFF_0000;

// ---------------------------------------------------------------------------
// SeccompData — the data available to BPF programs
// ---------------------------------------------------------------------------

/// Data passed to seccomp BPF programs.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SeccompData {
    /// Syscall number (arch-dependent).
    pub nr: i32,
    /// AUDIT_ARCH_* value.
    pub arch: u32,
    /// Instruction pointer at time of syscall.
    pub instruction_pointer: u64,
    /// Syscall arguments (up to 6).
    pub args: [u64; 6],
}

// ---------------------------------------------------------------------------
// seccomp() syscall
// ---------------------------------------------------------------------------

/// Install or query seccomp filters.
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn seccomp(_operation: u32, _flags: u32, _args: *mut u8) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seccomp_modes() {
        assert_eq!(SECCOMP_MODE_DISABLED, 0);
        assert_eq!(SECCOMP_MODE_STRICT, 1);
        assert_eq!(SECCOMP_MODE_FILTER, 2);
    }

    #[test]
    fn test_operations() {
        assert_eq!(SECCOMP_SET_MODE_STRICT, 0);
        assert_eq!(SECCOMP_SET_MODE_FILTER, 1);
        assert_eq!(SECCOMP_GET_ACTION_AVAIL, 2);
    }

    #[test]
    fn test_filter_flags_distinct() {
        let flags = [
            SECCOMP_FILTER_FLAG_TSYNC,
            SECCOMP_FILTER_FLAG_LOG,
            SECCOMP_FILTER_FLAG_SPEC_ALLOW,
            SECCOMP_FILTER_FLAG_NEW_LISTENER,
            SECCOMP_FILTER_FLAG_TSYNC_ESRCH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_ret_values_ordered() {
        // KILL < TRAP < ERRNO < USER_NOTIF < TRACE < LOG < ALLOW
        assert!(SECCOMP_RET_KILL < SECCOMP_RET_TRAP);
        assert!(SECCOMP_RET_TRAP < SECCOMP_RET_ERRNO);
        assert!(SECCOMP_RET_ERRNO < SECCOMP_RET_USER_NOTIF);
        assert!(SECCOMP_RET_USER_NOTIF < SECCOMP_RET_TRACE);
        assert!(SECCOMP_RET_TRACE < SECCOMP_RET_LOG);
        assert!(SECCOMP_RET_LOG < SECCOMP_RET_ALLOW);
    }

    #[test]
    fn test_seccomp_data_size() {
        // 4 + 4 + 8 + 6*8 = 64 bytes
        assert_eq!(core::mem::size_of::<SeccompData>(), 64);
    }

    #[test]
    fn test_seccomp_stub() {
        assert_eq!(seccomp(0, 0, core::ptr::null_mut()), -1);
    }

    #[test]
    fn test_ret_masks() {
        // Action mask should extract action from return value.
        let ret = SECCOMP_RET_ERRNO | 0x0001;
        assert_eq!(ret & SECCOMP_RET_ACTION_FULL, SECCOMP_RET_ERRNO);
        assert_eq!(ret & SECCOMP_RET_DATA, 0x0001);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(SECCOMP_MODE_STRICT, crate::sys_prctl::SECCOMP_MODE_STRICT);
        assert_eq!(SECCOMP_MODE_FILTER, crate::sys_prctl::SECCOMP_MODE_FILTER);
    }
}

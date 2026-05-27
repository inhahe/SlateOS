//! `<linux/seccomp.h>` — seccomp (secure computing) syscall.
//!
//! seccomp restricts the syscalls a thread can make. The `seccomp(2)`
//! syscall is the modern (Linux 3.17+) interface to set strict mode,
//! install BPF filters, query supported actions, and read notification
//! struct sizes.
//!
//! # Status
//!
//! `seccomp()` now performs real input validation matching Linux's
//! contract, and one operation — `SECCOMP_GET_ACTION_AVAIL` — is
//! fully implemented (it's a pure constant lookup that asks "does
//! this kernel support this seccomp return action?", with no
//! side effects). The remaining operations validate inputs and then
//! return `-1 / ENOSYS` because the underlying mechanisms (per-thread
//! BPF interpreter, notification fd table, syscall filter table) are
//! not yet implemented in our kernel.
//!
//! Programs that probe `seccomp(SECCOMP_GET_ACTION_AVAIL, 0, &action)`
//! at startup (Chrome's sandbox, systemd's `SeccompFilter=`, libseccomp's
//! `seccomp_arch_native_check()`) now see real "supported" answers for
//! the actions we recognize and `EOPNOTSUPP` for unknown ones, which
//! lets them safely decide which `SECCOMP_RET_*` value to use without
//! gambling on what kernel version they're running on.

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

/// Set the seccomp mode for the calling thread to strict.
pub const SECCOMP_SET_MODE_STRICT: u32 = 0;
/// Install a seccomp BPF filter.
pub const SECCOMP_SET_MODE_FILTER: u32 = 1;
/// Probe whether the kernel supports a given seccomp return action.
pub const SECCOMP_GET_ACTION_AVAIL: u32 = 2;
/// Read the sizes of the notification structs.
pub const SECCOMP_GET_NOTIF_SIZES: u32 = 3;

// ---------------------------------------------------------------------------
// seccomp filter flags
// ---------------------------------------------------------------------------

/// Synchronize the filter across all threads.
pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1 << 0;
/// Log every non-ALLOW action.
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 1 << 1;
/// Disable Spectre v4 mitigation (speculative store bypass) when filter matches.
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 1 << 2;
/// Returns a notification fd instead of 0 on success.
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 1 << 3;
/// TSYNC reports ESRCH (not EBUSY) on thread conflict.
pub const SECCOMP_FILTER_FLAG_TSYNC_ESRCH: u32 = 1 << 4;
/// Notification recv() is killable (Linux 5.18+).
pub const SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV: u32 = 1 << 5;

/// OR of every flag bit `SECCOMP_SET_MODE_FILTER` accepts.
const SECCOMP_FILTER_VALID_FLAGS: u32 = SECCOMP_FILTER_FLAG_TSYNC
    | SECCOMP_FILTER_FLAG_LOG
    | SECCOMP_FILTER_FLAG_SPEC_ALLOW
    | SECCOMP_FILTER_FLAG_NEW_LISTENER
    | SECCOMP_FILTER_FLAG_TSYNC_ESRCH
    | SECCOMP_FILTER_FLAG_WAIT_KILLABLE_RECV;

// ---------------------------------------------------------------------------
// seccomp return values (high 16 bits of BPF return)
// ---------------------------------------------------------------------------

/// Kill the thread immediately.
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x0000_0000;
/// Alias for `RET_KILL_THREAD`.
pub const SECCOMP_RET_KILL: u32 = SECCOMP_RET_KILL_THREAD;
/// Kill the entire process.
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
/// Send SIGSYS to the thread.
pub const SECCOMP_RET_TRAP: u32 = 0x0003_0000;
/// Return an errno to userspace.
pub const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
/// Forward the syscall to a user-space notifier.
pub const SECCOMP_RET_USER_NOTIF: u32 = 0x7FC0_0000;
/// Notify a ptrace tracer.
pub const SECCOMP_RET_TRACE: u32 = 0x7FF0_0000;
/// Log the syscall and allow it.
pub const SECCOMP_RET_LOG: u32 = 0x7FFC_0000;
/// Allow the syscall.
pub const SECCOMP_RET_ALLOW: u32 = 0x7FFF_0000;

/// Mask for the action portion (high 16 bits).
pub const SECCOMP_RET_ACTION_FULL: u32 = 0xFFFF_0000;
/// Mask for the data portion (low 16 bits).
pub const SECCOMP_RET_DATA: u32 = 0x0000_FFFF;
/// Mask for the action only (no data, ignores `KILL_PROCESS` high bit).
pub const SECCOMP_RET_ACTION: u32 = 0x7FFF_0000;

// ---------------------------------------------------------------------------
// SeccompData — what BPF programs inspect
// ---------------------------------------------------------------------------

/// Data passed to seccomp BPF programs (`struct seccomp_data` in Linux).
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
// Helpers
// ---------------------------------------------------------------------------

/// Is `action` a `SECCOMP_RET_*` action value the kernel recognizes?
///
/// Used by `SECCOMP_GET_ACTION_AVAIL` — the BPF return value's action
/// portion is the high 16 bits, but the API takes the full u32 (the
/// caller masks `& SECCOMP_RET_ACTION_FULL` themselves).
fn is_known_action(action: u32) -> bool {
    matches!(
        action,
        SECCOMP_RET_KILL_PROCESS
            | SECCOMP_RET_KILL_THREAD
            | SECCOMP_RET_TRAP
            | SECCOMP_RET_ERRNO
            | SECCOMP_RET_USER_NOTIF
            | SECCOMP_RET_TRACE
            | SECCOMP_RET_LOG
            | SECCOMP_RET_ALLOW
    )
}

// ---------------------------------------------------------------------------
// seccomp() syscall
// ---------------------------------------------------------------------------

/// Install or query seccomp filters.
///
/// `operation` selects one of `SECCOMP_SET_MODE_STRICT` /
/// `SECCOMP_SET_MODE_FILTER` / `SECCOMP_GET_ACTION_AVAIL` /
/// `SECCOMP_GET_NOTIF_SIZES`. `flags` and `args` are op-specific.
///
/// # Returns
///
/// * `SECCOMP_GET_ACTION_AVAIL`: `0` if the kernel recognizes the
///   given action, `-1` + `EOPNOTSUPP` if unknown. This is fully
///   implemented — it's a constant lookup with no side effects.
/// * Other ops: `-1` with errno set per the validation table:
///   * `EFAULT` — `args` is NULL when the op requires a buffer.
///   * `EINVAL` — unknown operation, unknown flag bit, or `flags`
///     non-zero for an op that requires `flags == 0`.
///   * `ENOSYS` — every input was valid but the underlying mechanism
///     (BPF interpreter / notification table / strict-mode enforcement)
///     isn't implemented yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn seccomp(operation: u32, flags: u32, args: *mut u8) -> i32 {
    match operation {
        SECCOMP_SET_MODE_STRICT => {
            // Linux: flags must be 0, args must be NULL. The thread
            // is restricted to read/write/exit/sigreturn after the
            // call. We can't enforce that without a kernel-side
            // syscall gate, so we ENOSYS after validating shape.
            if flags != 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            if !args.is_null() {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            -1
        }
        SECCOMP_SET_MODE_FILTER => {
            // flags is a bitmask of SECCOMP_FILTER_FLAG_* values.
            // args points to a `struct sock_fprog` — must be non-NULL.
            if (flags & !SECCOMP_FILTER_VALID_FLAGS) != 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            if args.is_null() {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            // TSYNC and NEW_LISTENER are mutually exclusive in Linux:
            // a thread-synchronized filter has no single owning fd to
            // hand back to userspace.
            if (flags & SECCOMP_FILTER_FLAG_TSYNC) != 0
                && (flags & SECCOMP_FILTER_FLAG_NEW_LISTENER) != 0
            {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // TSYNC_ESRCH is meaningless without TSYNC.
            if (flags & SECCOMP_FILTER_FLAG_TSYNC_ESRCH) != 0
                && (flags & SECCOMP_FILTER_FLAG_TSYNC) == 0
            {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            -1
        }
        SECCOMP_GET_ACTION_AVAIL => {
            // Probe a single action. Linux requires flags == 0 and
            // args to point to a u32 containing the action to query.
            if flags != 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            if args.is_null() {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            // SAFETY: caller promises `args` points to a readable u32.
            // We use `read_unaligned` to defend against callers that
            // hand us a misaligned struct field.
            let action = unsafe { core::ptr::read_unaligned(args.cast::<u32>()) };
            if is_known_action(action) {
                0
            } else {
                errno::set_errno(errno::EOPNOTSUPP);
                -1
            }
        }
        SECCOMP_GET_NOTIF_SIZES => {
            // Linux populates a `struct seccomp_notif_sizes` with the
            // sizes of `seccomp_notif`, `seccomp_notif_resp`, and
            // `seccomp_data`. We don't have a notification path yet,
            // so saying "here are the sizes" would lie about supported
            // surface — userspace would then issue SET_MODE_FILTER
            // with FLAG_NEW_LISTENER and get a confusing ENOSYS only
            // at filter-install time. Cleaner contract: report ENOSYS
            // here so callers skip notification-mode entirely.
            if flags != 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            if args.is_null() {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            errno::set_errno(errno::ENOSYS);
            -1
        }
        _ => {
            // Unknown operation — Linux returns EINVAL.
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // Constant invariants
    // -----------------------------------------------------------------

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
        assert_eq!(SECCOMP_GET_NOTIF_SIZES, 3);
    }

    #[test]
    fn test_filter_flags_distinct() {
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
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_ret_values_ordered() {
        // KILL_THREAD < TRAP < ERRNO < USER_NOTIF < TRACE < LOG < ALLOW
        // (KILL_PROCESS is the high-bit-set special case).
        assert!(SECCOMP_RET_KILL_THREAD < SECCOMP_RET_TRAP);
        assert!(SECCOMP_RET_TRAP < SECCOMP_RET_ERRNO);
        assert!(SECCOMP_RET_ERRNO < SECCOMP_RET_USER_NOTIF);
        assert!(SECCOMP_RET_USER_NOTIF < SECCOMP_RET_TRACE);
        assert!(SECCOMP_RET_TRACE < SECCOMP_RET_LOG);
        assert!(SECCOMP_RET_LOG < SECCOMP_RET_ALLOW);
    }

    #[test]
    fn test_seccomp_data_size() {
        // 4 + 4 + 8 + 6*8 = 64 bytes.
        assert_eq!(core::mem::size_of::<SeccompData>(), 64);
    }

    #[test]
    fn test_ret_masks() {
        let ret = SECCOMP_RET_ERRNO | 0x0001;
        assert_eq!(ret & SECCOMP_RET_ACTION_FULL, SECCOMP_RET_ERRNO);
        assert_eq!(ret & SECCOMP_RET_DATA, 0x0001);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(SECCOMP_MODE_STRICT, crate::sys_prctl::SECCOMP_MODE_STRICT);
        assert_eq!(SECCOMP_MODE_FILTER, crate::sys_prctl::SECCOMP_MODE_FILTER);
    }

    // -----------------------------------------------------------------
    // SECCOMP_SET_MODE_STRICT
    // -----------------------------------------------------------------

    #[test]
    fn test_strict_valid_inputs_enosys() {
        errno::set_errno(0);
        let ret = seccomp(SECCOMP_SET_MODE_STRICT, 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_strict_nonzero_flags_einval() {
        errno::set_errno(0);
        let ret = seccomp(SECCOMP_SET_MODE_STRICT, 1, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_strict_nonnull_args_einval() {
        errno::set_errno(0);
        let mut sentinel: u32 = 0;
        let ret = seccomp(
            SECCOMP_SET_MODE_STRICT,
            0,
            (&mut sentinel as *mut u32).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // SECCOMP_SET_MODE_FILTER
    // -----------------------------------------------------------------

    #[test]
    fn test_filter_valid_inputs_enosys() {
        errno::set_errno(0);
        // Pretend `args` points to a sock_fprog — we never read it
        // because validation succeeds and then ENOSYS short-circuits
        // before touching it. Use a stack scratch buffer.
        let mut scratch: [u8; 16] = [0; 16];
        let ret = seccomp(SECCOMP_SET_MODE_FILTER, 0, scratch.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_filter_unknown_flag_einval() {
        errno::set_errno(0);
        let mut scratch: [u8; 16] = [0; 16];
        let ret = seccomp(SECCOMP_SET_MODE_FILTER, 1 << 16, scratch.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_filter_null_args_efault() {
        errno::set_errno(0);
        let ret = seccomp(SECCOMP_SET_MODE_FILTER, 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_filter_tsync_and_new_listener_einval() {
        errno::set_errno(0);
        let mut scratch: [u8; 16] = [0; 16];
        let ret = seccomp(
            SECCOMP_SET_MODE_FILTER,
            SECCOMP_FILTER_FLAG_TSYNC | SECCOMP_FILTER_FLAG_NEW_LISTENER,
            scratch.as_mut_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_filter_tsync_esrch_without_tsync_einval() {
        errno::set_errno(0);
        let mut scratch: [u8; 16] = [0; 16];
        let ret = seccomp(
            SECCOMP_SET_MODE_FILTER,
            SECCOMP_FILTER_FLAG_TSYNC_ESRCH,
            scratch.as_mut_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_filter_tsync_plus_tsync_esrch_ok() {
        errno::set_errno(0);
        let mut scratch: [u8; 16] = [0; 16];
        let ret = seccomp(
            SECCOMP_SET_MODE_FILTER,
            SECCOMP_FILTER_FLAG_TSYNC | SECCOMP_FILTER_FLAG_TSYNC_ESRCH,
            scratch.as_mut_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // SECCOMP_GET_ACTION_AVAIL — fully implemented
    // -----------------------------------------------------------------

    #[test]
    fn test_get_action_avail_allow_supported() {
        errno::set_errno(errno::EINVAL); // sentinel to confirm preserved
        let mut action: u32 = SECCOMP_RET_ALLOW;
        let ret = seccomp(
            SECCOMP_GET_ACTION_AVAIL,
            0,
            (&mut action as *mut u32).cast::<u8>(),
        );
        assert_eq!(ret, 0);
        // POSIX: errno must not be cleared by a successful call.
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_get_action_avail_every_known_action_supported() {
        let known = [
            SECCOMP_RET_KILL_PROCESS,
            SECCOMP_RET_KILL_THREAD,
            SECCOMP_RET_TRAP,
            SECCOMP_RET_ERRNO,
            SECCOMP_RET_USER_NOTIF,
            SECCOMP_RET_TRACE,
            SECCOMP_RET_LOG,
            SECCOMP_RET_ALLOW,
        ];
        for &a in &known {
            let mut action: u32 = a;
            let ret = seccomp(
                SECCOMP_GET_ACTION_AVAIL,
                0,
                (&mut action as *mut u32).cast::<u8>(),
            );
            assert_eq!(ret, 0, "action {a:#x} should be supported");
        }
    }

    #[test]
    fn test_get_action_avail_unknown_eopnotsupp() {
        errno::set_errno(0);
        // 0x1234_5678 is not any known action.
        let mut action: u32 = 0x1234_5678;
        let ret = seccomp(
            SECCOMP_GET_ACTION_AVAIL,
            0,
            (&mut action as *mut u32).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_get_action_avail_action_with_data_low_bits_unknown() {
        // The caller is supposed to mask off the data low bits before
        // querying. SECCOMP_RET_ERRNO | 0x0001 is not a bare action.
        errno::set_errno(0);
        let mut action: u32 = SECCOMP_RET_ERRNO | 0x0001;
        let ret = seccomp(
            SECCOMP_GET_ACTION_AVAIL,
            0,
            (&mut action as *mut u32).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_get_action_avail_nonzero_flags_einval() {
        errno::set_errno(0);
        let mut action: u32 = SECCOMP_RET_ALLOW;
        let ret = seccomp(
            SECCOMP_GET_ACTION_AVAIL,
            1,
            (&mut action as *mut u32).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_get_action_avail_null_args_efault() {
        errno::set_errno(0);
        let ret = seccomp(SECCOMP_GET_ACTION_AVAIL, 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_get_action_avail_misaligned_pointer_ok() {
        // Build a byte-aligned buffer that happens to land on an odd
        // address — read_unaligned must handle it without UB.
        let mut buf: [u8; 8] = [0; 8];
        // SECCOMP_RET_ALLOW = 0x7FFF_0000 in little-endian bytes.
        buf[1] = 0x00;
        buf[2] = 0x00;
        buf[3] = 0xFF;
        buf[4] = 0x7F;
        // Read starting at offset 1 (guaranteed misaligned for u32).
        let ret = seccomp(
            SECCOMP_GET_ACTION_AVAIL,
            0,
            unsafe { buf.as_mut_ptr().add(1) },
        );
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------
    // SECCOMP_GET_NOTIF_SIZES
    // -----------------------------------------------------------------

    #[test]
    fn test_get_notif_sizes_valid_inputs_enosys() {
        errno::set_errno(0);
        let mut buf: [u8; 8] = [0; 8];
        let ret = seccomp(SECCOMP_GET_NOTIF_SIZES, 0, buf.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_get_notif_sizes_nonzero_flags_einval() {
        errno::set_errno(0);
        let mut buf: [u8; 8] = [0; 8];
        let ret = seccomp(SECCOMP_GET_NOTIF_SIZES, 1, buf.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_get_notif_sizes_null_args_efault() {
        errno::set_errno(0);
        let ret = seccomp(SECCOMP_GET_NOTIF_SIZES, 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -----------------------------------------------------------------
    // Unknown operation
    // -----------------------------------------------------------------

    #[test]
    fn test_unknown_op_einval() {
        errno::set_errno(0);
        let ret = seccomp(0xDEAD_BEEF, 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_op_four_einval() {
        // Op 4 doesn't exist (yet) — Linux returns EINVAL.
        errno::set_errno(0);
        let ret = seccomp(4, 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // Workflow: realistic libseccomp probe
    // -----------------------------------------------------------------

    #[test]
    fn test_typical_libseccomp_probe_workflow() {
        // libseccomp's `seccomp_arch_native_check()` queries which
        // actions the kernel supports before deciding how to phrase
        // its rules. We answer truthfully for every known action.
        for &(action, expected_ok) in &[
            (SECCOMP_RET_ALLOW, true),
            (SECCOMP_RET_KILL_PROCESS, true),
            (SECCOMP_RET_LOG, true),
            (SECCOMP_RET_USER_NOTIF, true),
            // Made-up action — the kernel must say "I don't know that"
            // so libseccomp can pick a fallback.
            (0xDEAD_0000, false),
        ] {
            let mut a: u32 = action;
            let ret = seccomp(
                SECCOMP_GET_ACTION_AVAIL,
                0,
                (&mut a as *mut u32).cast::<u8>(),
            );
            if expected_ok {
                assert_eq!(ret, 0, "expected support for {action:#x}");
            } else {
                assert_eq!(ret, -1, "expected no support for {action:#x}");
                assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
            }
        }
        // After probing, the caller would call SET_MODE_FILTER to
        // install rules — that's still ENOSYS in our world, so they
        // fall back to "no sandbox" or to seccomp_arch_remove() and
        // try a different policy strategy. The shape is what matters.
        let mut scratch: [u8; 16] = [0; 16];
        let install = seccomp(SECCOMP_SET_MODE_FILTER, 0, scratch.as_mut_ptr());
        assert_eq!(install, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }
}

//! `<linux/sched.h>` — clone3() struct clone_args layout and sizes.
//!
//! `clone3()` extends `clone(2)` with an open-ended struct so new
//! parameters can be added without breaking ABI. The kernel selects
//! which version to use based on the `size` argument passed in.

// ---------------------------------------------------------------------------
// struct clone_args field offsets
// ---------------------------------------------------------------------------

/// `__u64 flags` — CLONE_* flags.
pub const CLONE_ARGS_OFF_FLAGS: usize = 0;
/// `__u64 pidfd` — destination pidfd (output if CLONE_PIDFD set).
pub const CLONE_ARGS_OFF_PIDFD: usize = 8;
/// `__u64 child_tid` — userspace child TID address.
pub const CLONE_ARGS_OFF_CHILD_TID: usize = 16;
/// `__u64 parent_tid` — userspace parent TID address.
pub const CLONE_ARGS_OFF_PARENT_TID: usize = 24;
/// `__u64 exit_signal` — signal to send to parent on exit.
pub const CLONE_ARGS_OFF_EXIT_SIGNAL: usize = 32;
/// `__u64 stack` — child stack base.
pub const CLONE_ARGS_OFF_STACK: usize = 40;
/// `__u64 stack_size` — child stack size.
pub const CLONE_ARGS_OFF_STACK_SIZE: usize = 48;
/// `__u64 tls` — TLS pointer.
pub const CLONE_ARGS_OFF_TLS: usize = 56;
/// `__u64 set_tid` — vector of TIDs to set.
pub const CLONE_ARGS_OFF_SET_TID: usize = 64;
/// `__u64 set_tid_size` — number of entries in set_tid.
pub const CLONE_ARGS_OFF_SET_TID_SIZE: usize = 72;
/// `__u64 cgroup` — fd of target cgroup (CLONE_INTO_CGROUP).
pub const CLONE_ARGS_OFF_CGROUP: usize = 80;

// ---------------------------------------------------------------------------
// Version sizes (CLONE_ARGS_SIZE_VERx)
// ---------------------------------------------------------------------------

/// V0: original 5.3 layout — through tls (size 64).
pub const CLONE_ARGS_SIZE_VER0: usize = 64;
/// V1: added set_tid + set_tid_size (5.5).
pub const CLONE_ARGS_SIZE_VER1: usize = 80;
/// V2: added cgroup field (5.7).
pub const CLONE_ARGS_SIZE_VER2: usize = 88;

// ---------------------------------------------------------------------------
// Syscall number on x86_64
// ---------------------------------------------------------------------------

pub const NR_CLONE3_X86_64: u32 = 435;
pub const NR_CLONE3_AARCH64: u32 = 435;

// ---------------------------------------------------------------------------
// Maximum entries in set_tid vector
// ---------------------------------------------------------------------------

/// Kernel-enforced cap on `set_tid` vector length (one per pid ns).
pub const CLONE_ARGS_SET_TID_MAX: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_consecutive_u64s() {
        let o = [
            CLONE_ARGS_OFF_FLAGS,
            CLONE_ARGS_OFF_PIDFD,
            CLONE_ARGS_OFF_CHILD_TID,
            CLONE_ARGS_OFF_PARENT_TID,
            CLONE_ARGS_OFF_EXIT_SIGNAL,
            CLONE_ARGS_OFF_STACK,
            CLONE_ARGS_OFF_STACK_SIZE,
            CLONE_ARGS_OFF_TLS,
            CLONE_ARGS_OFF_SET_TID,
            CLONE_ARGS_OFF_SET_TID_SIZE,
            CLONE_ARGS_OFF_CGROUP,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, i * 8);
        }
    }

    #[test]
    fn test_versions_in_increasing_size_order() {
        assert!(CLONE_ARGS_SIZE_VER0 < CLONE_ARGS_SIZE_VER1);
        assert!(CLONE_ARGS_SIZE_VER1 < CLONE_ARGS_SIZE_VER2);
        // V0 covers flags..tls (8 u64s).
        assert_eq!(CLONE_ARGS_SIZE_VER0, 8 * 8);
        // V1 adds 2 more (set_tid + set_tid_size).
        assert_eq!(CLONE_ARGS_SIZE_VER1, 10 * 8);
        // V2 adds 1 more (cgroup).
        assert_eq!(CLONE_ARGS_SIZE_VER2, 11 * 8);
    }

    #[test]
    fn test_v0_ends_at_tls() {
        // The next offset after tls is exactly V0's size.
        assert_eq!(CLONE_ARGS_OFF_SET_TID, CLONE_ARGS_SIZE_VER0);
    }

    #[test]
    fn test_v1_ends_at_set_tid_size() {
        // The next offset after set_tid_size is V1's size.
        assert_eq!(CLONE_ARGS_OFF_CGROUP, CLONE_ARGS_SIZE_VER1);
    }

    #[test]
    fn test_syscall_number_435() {
        // clone3() got syscall 435 on every arch — kernel chose to keep
        // numbering aligned for newer syscalls.
        assert_eq!(NR_CLONE3_X86_64, 435);
        assert_eq!(NR_CLONE3_AARCH64, 435);
    }

    #[test]
    fn test_set_tid_cap_is_32() {
        // 32 entries = up to 32 nested pid namespaces.
        assert_eq!(CLONE_ARGS_SET_TID_MAX, 32);
    }
}

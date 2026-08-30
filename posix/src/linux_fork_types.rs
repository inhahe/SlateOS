//! `<unistd.h>` / `<sched.h>` — fork/vfork/clone related constants.
//!
//! `fork()` and `vfork()` create new processes.  The lower-level
//! `clone()` provides more control.  These constants define
//! return values, clone flags subsets, and waitpid-related values.

// ---------------------------------------------------------------------------
// fork() return values
// ---------------------------------------------------------------------------

/// Return value in the child process.
pub const FORK_CHILD: u32 = 0;

// ---------------------------------------------------------------------------
// Process ID limits
// ---------------------------------------------------------------------------

/// Maximum PID value (Linux default).
pub const PID_MAX_DEFAULT: u32 = 32768;
/// Maximum PID value (Linux max, set via /proc/sys/kernel/pid_max).
pub const PID_MAX_LIMIT: u32 = 4194304; // 4M on 64-bit
/// Init process PID.
pub const PID_INIT: u32 = 1;

// ---------------------------------------------------------------------------
// clone3() structure sizes
// ---------------------------------------------------------------------------

/// Size of struct clone_args version 0 (bytes).
pub const CLONE_ARGS_SIZE_V0: u32 = 64;
/// Size of struct clone_args version 1 (bytes, with set_tid).
pub const CLONE_ARGS_SIZE_V1: u32 = 80;
/// Size of struct clone_args version 2 (bytes, with cgroup).
pub const CLONE_ARGS_SIZE_V2: u32 = 88;

// ---------------------------------------------------------------------------
// clone3() struct clone_args field offsets
// ---------------------------------------------------------------------------

/// Offset of flags in struct clone_args.
pub const CLONE_ARGS_OFF_FLAGS: u32 = 0;
/// Offset of pidfd in struct clone_args.
pub const CLONE_ARGS_OFF_PIDFD: u32 = 8;
/// Offset of child_tid in struct clone_args.
pub const CLONE_ARGS_OFF_CHILD_TID: u32 = 16;
/// Offset of parent_tid in struct clone_args.
pub const CLONE_ARGS_OFF_PARENT_TID: u32 = 24;
/// Offset of exit_signal in struct clone_args.
pub const CLONE_ARGS_OFF_EXIT_SIGNAL: u32 = 32;
/// Offset of stack in struct clone_args.
pub const CLONE_ARGS_OFF_STACK: u32 = 40;
/// Offset of stack_size in struct clone_args.
pub const CLONE_ARGS_OFF_STACK_SIZE: u32 = 48;
/// Offset of tls in struct clone_args.
pub const CLONE_ARGS_OFF_TLS: u32 = 56;

// ---------------------------------------------------------------------------
// Default stack sizes for child processes
// ---------------------------------------------------------------------------

/// Default thread stack size (bytes).
pub const DEFAULT_CHILD_STACK: u32 = 8388608; // 8 MiB
/// Minimum child stack size (bytes).
pub const MIN_CHILD_STACK: u32 = 16384; // 16 KiB

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fork_child_is_zero() {
        assert_eq!(FORK_CHILD, 0);
    }

    #[test]
    fn test_pid_max_default() {
        assert_eq!(PID_MAX_DEFAULT, 32768);
    }

    #[test]
    fn test_pid_max_limit() {
        assert!(PID_MAX_LIMIT > PID_MAX_DEFAULT);
    }

    #[test]
    fn test_pid_init() {
        assert_eq!(PID_INIT, 1);
    }

    #[test]
    fn test_clone_args_sizes_ascending() {
        assert!(CLONE_ARGS_SIZE_V1 > CLONE_ARGS_SIZE_V0);
        assert!(CLONE_ARGS_SIZE_V2 > CLONE_ARGS_SIZE_V1);
    }

    #[test]
    fn test_clone_args_offsets_ascending() {
        let offsets = [
            CLONE_ARGS_OFF_FLAGS,
            CLONE_ARGS_OFF_PIDFD,
            CLONE_ARGS_OFF_CHILD_TID,
            CLONE_ARGS_OFF_PARENT_TID,
            CLONE_ARGS_OFF_EXIT_SIGNAL,
            CLONE_ARGS_OFF_STACK,
            CLONE_ARGS_OFF_STACK_SIZE,
            CLONE_ARGS_OFF_TLS,
        ];
        for i in 1..offsets.len() {
            assert!(offsets[i] > offsets[i - 1]);
        }
    }

    #[test]
    fn test_clone_args_offsets_within_v0() {
        assert!(CLONE_ARGS_OFF_TLS < CLONE_ARGS_SIZE_V0);
    }

    #[test]
    fn test_stack_sizes() {
        assert!(DEFAULT_CHILD_STACK > MIN_CHILD_STACK);
    }
}

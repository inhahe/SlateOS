//! `<linux/sched.h>` — clone3() system call flag constants.
//!
//! clone3() is the modern process/thread creation syscall, replacing
//! clone() with a struct-based interface that's extensible. It
//! supports all clone() flags plus new features like cgroup
//! placement, PID selection, and set_tid.

// ---------------------------------------------------------------------------
// clone3 flags (superset of clone() flags)
// ---------------------------------------------------------------------------

/// Share virtual memory (threads).
pub const CLONE3_VM: u64 = 0x0000_0100;
/// Share filesystem info (root, cwd, umask).
pub const CLONE3_FS: u64 = 0x0000_0200;
/// Share file descriptor table.
pub const CLONE3_FILES: u64 = 0x0000_0400;
/// Share signal handlers.
pub const CLONE3_SIGHAND: u64 = 0x0000_0800;
/// Create in a new PID namespace.
pub const CLONE3_NEWPID: u64 = 0x2000_0000;
/// Parent sets TID in child memory (for futex).
pub const CLONE3_PARENT_SETTID: u64 = 0x0010_0000;
/// Child clears TID on exit (for futex-based join).
pub const CLONE3_CHILD_CLEARTID: u64 = 0x0020_0000;
/// Set child TID in child memory.
pub const CLONE3_CHILD_SETTID: u64 = 0x0100_0000;
/// Share thread group (TGID).
pub const CLONE3_THREAD: u64 = 0x0001_0000;
/// New network namespace.
pub const CLONE3_NEWNET: u64 = 0x4000_0000;
/// Signal parent on exit (usually SIGCHLD).
pub const CLONE3_SIGNAL_MASK: u64 = 0x0000_00FF;
/// Clear the child signal (vfork behavior).
pub const CLONE3_VFORK: u64 = 0x0000_4000;
/// Attach to specific cgroup (cgroup fd in struct).
pub const CLONE3_INTO_CGROUP: u64 = 0x0000_0000_2000_0000;

// ---------------------------------------------------------------------------
// clone3 struct fields (sizes/offsets for kernel ABI)
// ---------------------------------------------------------------------------

/// Minimum clone_args struct size (v5.3).
pub const CLONE_ARGS_SIZE_VER0: u32 = 64;
/// Extended size with set_tid (v5.5).
pub const CLONE_ARGS_SIZE_VER1: u32 = 80;
/// Extended size with cgroup fd (v5.7).
pub const CLONE_ARGS_SIZE_VER2: u32 = 88;

// ---------------------------------------------------------------------------
// clone3 exit signal values
// ---------------------------------------------------------------------------

/// No signal on child exit.
pub const CLONE3_EXIT_SIGNAL_NONE: u64 = 0;
/// SIGCHLD on child exit (default for fork-like).
pub const CLONE3_EXIT_SIGNAL_SIGCHLD: u64 = 17;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone3_flags_distinct() {
        let flags = [
            CLONE3_VM, CLONE3_FS, CLONE3_FILES, CLONE3_SIGHAND,
            CLONE3_PARENT_SETTID, CLONE3_CHILD_CLEARTID,
            CLONE3_CHILD_SETTID, CLONE3_THREAD, CLONE3_VFORK,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_struct_sizes_growing() {
        assert!(CLONE_ARGS_SIZE_VER0 < CLONE_ARGS_SIZE_VER1);
        assert!(CLONE_ARGS_SIZE_VER1 < CLONE_ARGS_SIZE_VER2);
    }

    #[test]
    fn test_exit_signals() {
        assert_ne!(CLONE3_EXIT_SIGNAL_NONE, CLONE3_EXIT_SIGNAL_SIGCHLD);
    }
}

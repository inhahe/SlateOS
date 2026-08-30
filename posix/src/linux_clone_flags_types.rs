//! `<linux/sched.h>` — Clone flag constants for process/thread creation.
//!
//! These flags control which resources are shared between the parent
//! and child when creating a new process or thread via `clone()`,
//! `clone3()`, or `unshare()`.

// ---------------------------------------------------------------------------
// Classic clone flags (bitmask in the clone flags argument)
// ---------------------------------------------------------------------------

/// Share virtual memory (threads).
pub const CLONE_VM: u64 = 0x0000_0100;
/// Share filesystem info (root, cwd, umask).
pub const CLONE_FS: u64 = 0x0000_0200;
/// Share open file descriptor table.
pub const CLONE_FILES: u64 = 0x0000_0400;
/// Share signal handlers.
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
/// Place in traced child's ptrace list.
pub const CLONE_PTRACE: u64 = 0x0000_2000;
/// Set if parent wants child to wake it on mm_release.
pub const CLONE_VFORK: u64 = 0x0000_4000;
/// Set if we want to have the same parent as the cloner.
pub const CLONE_PARENT: u64 = 0x0000_8000;
/// Same thread group (POSIX threads).
pub const CLONE_THREAD: u64 = 0x0001_0000;
/// Create a new mount namespace.
pub const CLONE_NEWNS: u64 = 0x0002_0000;
/// Share System V semaphore undo lists.
pub const CLONE_SYSVSEM: u64 = 0x0004_0000;
/// Set TLS for the child.
pub const CLONE_SETTLS: u64 = 0x0008_0000;
/// Set the parent TID in the parent.
pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
/// Clear the child TID in the child on exit.
pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
/// Unused (was CLONE_DETACHED).
pub const CLONE_DETACHED: u64 = 0x0040_0000;
/// Set if tracing cannot force CLONE_PTRACE.
pub const CLONE_UNTRACED: u64 = 0x0080_0000;
/// Set the child TID in the child.
pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;

// ---------------------------------------------------------------------------
// Namespace clone flags
// ---------------------------------------------------------------------------

/// New cgroup namespace.
pub const CLONE_NEWCGROUP: u64 = 0x0200_0000;
/// New UTS namespace (hostname).
pub const CLONE_NEWUTS: u64 = 0x0400_0000;
/// New IPC namespace.
pub const CLONE_NEWIPC: u64 = 0x0800_0000;
/// New user namespace.
pub const CLONE_NEWUSER: u64 = 0x1000_0000;
/// New PID namespace.
pub const CLONE_NEWPID: u64 = 0x2000_0000;
/// New network namespace.
pub const CLONE_NEWNET: u64 = 0x4000_0000;
/// Share I/O context.
pub const CLONE_IO: u64 = 0x8000_0000;

// ---------------------------------------------------------------------------
// clone3-specific flags (upper 32 bits)
// ---------------------------------------------------------------------------

/// Clear any signal handler that is not SIG_DFL or SIG_IGN.
pub const CLONE_CLEAR_SIGHAND: u64 = 0x1_0000_0000;
/// Create child in a new time namespace.
pub const CLONE_NEWTIME: u64 = 0x0000_0080;
/// Set the child into an existing cgroup.
pub const CLONE_INTO_CGROUP: u64 = 0x2_0000_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classic_flags_no_overlap() {
        let flags = [
            CLONE_VM,
            CLONE_FS,
            CLONE_FILES,
            CLONE_SIGHAND,
            CLONE_PTRACE,
            CLONE_VFORK,
            CLONE_PARENT,
            CLONE_THREAD,
            CLONE_NEWNS,
            CLONE_SYSVSEM,
            CLONE_SETTLS,
            CLONE_PARENT_SETTID,
            CLONE_CHILD_CLEARTID,
            CLONE_DETACHED,
            CLONE_UNTRACED,
            CLONE_CHILD_SETTID,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ns_flags_no_overlap() {
        let flags = [
            CLONE_NEWCGROUP,
            CLONE_NEWUTS,
            CLONE_NEWIPC,
            CLONE_NEWUSER,
            CLONE_NEWPID,
            CLONE_NEWNET,
            CLONE_IO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_clone_vm() {
        assert_eq!(CLONE_VM, 0x100);
    }

    #[test]
    fn test_clone_thread() {
        assert_eq!(CLONE_THREAD, 0x0001_0000);
    }

    #[test]
    fn test_clone3_flags() {
        assert_eq!(CLONE_CLEAR_SIGHAND, 0x1_0000_0000);
        assert_eq!(CLONE_INTO_CGROUP, 0x2_0000_0000);
    }

    #[test]
    fn test_classic_flags_power_of_two() {
        let flags = [
            CLONE_VM,
            CLONE_FS,
            CLONE_FILES,
            CLONE_SIGHAND,
            CLONE_PTRACE,
            CLONE_VFORK,
            CLONE_PARENT,
            CLONE_THREAD,
            CLONE_NEWNS,
            CLONE_SYSVSEM,
            CLONE_SETTLS,
            CLONE_PARENT_SETTID,
            CLONE_CHILD_CLEARTID,
            CLONE_DETACHED,
            CLONE_UNTRACED,
            CLONE_CHILD_SETTID,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }
}

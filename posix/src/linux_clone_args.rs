//! `<linux/sched.h>` — clone3() arguments and flags.
//!
//! clone3() is the modern process/thread creation syscall that takes
//! a versioned struct rather than a long argument list. It supports
//! all clone flags plus new features like cgroup placement, set_tid
//! for checkpoint/restore, and pidfd creation in one atomic step.

// ---------------------------------------------------------------------------
// Clone flags (shared between clone, clone3, and unshare)
// ---------------------------------------------------------------------------

/// Share virtual memory (threads).
pub const CLONE_VM: u64 = 0x0000_0100;
/// Share filesystem info (cwd, root, umask).
pub const CLONE_FS: u64 = 0x0000_0200;
/// Share open files.
pub const CLONE_FILES: u64 = 0x0000_0400;
/// Share signal handlers.
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
/// Create pidfd for the child.
pub const CLONE_PIDFD: u64 = 0x0000_1000;
/// Trace the child if the parent is being traced (clone-only).
pub const CLONE_PTRACE: u64 = 0x0000_2000;
/// Parent blocks until child calls exec or _exit (clone-only).
pub const CLONE_VFORK: u64 = 0x0000_4000;
/// Same parent as the calling task (sibling rather than child).
pub const CLONE_PARENT: u64 = 0x0000_8000;
/// Same thread group (CLONE_THREAD implies CLONE_SIGHAND+CLONE_VM).
pub const CLONE_THREAD: u64 = 0x0001_0000;
/// New mount namespace.
pub const CLONE_NEWNS: u64 = 0x0002_0000;
/// Share System V semaphore adjustments.
pub const CLONE_SYSVSEM: u64 = 0x0004_0000;
/// Set TLS (thread-local storage).
pub const CLONE_SETTLS: u64 = 0x0008_0000;
/// Store child TID at parent-provided address.
pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
/// Clear child TID and wake futex on exit.
pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
/// Historical "detached" flag — ignored by Linux since 2.5.32 but
/// still accepted in the flag mask for ABI compatibility.
pub const CLONE_DETACHED: u64 = 0x0040_0000;
/// Unused (was CLONE_DETACHED).
pub const CLONE_UNTRACED: u64 = 0x0080_0000;
/// Store child TID at child-provided address.
pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;
/// New cgroup namespace.
pub const CLONE_NEWCGROUP: u64 = 0x0200_0000;
/// New UTS namespace.
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
/// New time namespace.
pub const CLONE_NEWTIME: u64 = 0x0000_0080;
/// Place child in specific cgroup (clone3 only).
///
/// **Bit 33** in Linux's `<linux/sched.h>` — `0x2_0000_0000`.  Earlier
/// revisions of this file mistakenly used `0x2000_0000` which is a
/// 32-bit value colliding with `CLONE_NEWPID`; that has been corrected.
pub const CLONE_INTO_CGROUP: u64 = 0x0000_0002_0000_0000_u64;
/// Clear signal mask in child.
pub const CLONE_CLEAR_SIGHAND: u64 = 0x0000_0001_0000_0000_u64;

/// Low byte of `clone(2)` flags — the exit signal delivered to the
/// parent when the child terminates.  Any value 0..=SIGRTMAX is
/// accepted; the kernel masks `flags & CSIGNAL` to extract it.
pub const CSIGNAL: u64 = 0x0000_00ff;

// ---------------------------------------------------------------------------
// clone3 struct size
// ---------------------------------------------------------------------------

/// Minimum clone_args struct size (v0).
pub const CLONE_ARGS_SIZE_VER0: u32 = 64;
/// Size with set_tid (v1).
pub const CLONE_ARGS_SIZE_VER1: u32 = 80;
/// Size with cgroup (v2).
pub const CLONE_ARGS_SIZE_VER2: u32 = 88;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_namespace_flags_no_overlap() {
        let ns_flags = [
            CLONE_NEWNS,
            CLONE_NEWCGROUP,
            CLONE_NEWUTS,
            CLONE_NEWIPC,
            CLONE_NEWUSER,
            CLONE_NEWPID,
            CLONE_NEWNET,
            CLONE_NEWTIME,
        ];
        for i in 0..ns_flags.len() {
            for j in (i + 1)..ns_flags.len() {
                assert_eq!(ns_flags[i] & ns_flags[j], 0);
            }
        }
    }

    #[test]
    fn test_thread_flags_no_overlap() {
        let flags = [
            CLONE_VM,
            CLONE_FS,
            CLONE_FILES,
            CLONE_SIGHAND,
            CLONE_PIDFD,
            CLONE_THREAD,
            CLONE_SYSVSEM,
            CLONE_SETTLS,
            CLONE_PARENT_SETTID,
            CLONE_CHILD_CLEARTID,
            CLONE_CHILD_SETTID,
            CLONE_IO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_clone_args_sizes_ascending() {
        assert!(CLONE_ARGS_SIZE_VER0 < CLONE_ARGS_SIZE_VER1);
        assert!(CLONE_ARGS_SIZE_VER1 < CLONE_ARGS_SIZE_VER2);
    }
}

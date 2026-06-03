//! `<linux/sched.h>` (thread subset) — Thread creation and management constants.
//!
//! Linux threads are lightweight processes created with clone() sharing
//! various resources (address space, file descriptors, signal handlers)
//! with their parent. The CLONE_* flags control exactly which resources
//! are shared vs. copied. Thread groups (TGID) represent what POSIX
//! calls a "process" — all threads in a group share a PID from the
//! outside perspective (getpid returns the TGID).

// ---------------------------------------------------------------------------
// Clone flags (subset for thread creation)
// ---------------------------------------------------------------------------

/// Share virtual memory (address space).
pub const CLONE_VM: u64 = 0x0000_0100;
/// Share filesystem info (cwd, root, umask).
pub const CLONE_FS: u64 = 0x0000_0200;
/// Share file descriptor table.
pub const CLONE_FILES: u64 = 0x0000_0400;
/// Share signal handlers.
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
/// Create in new PID namespace.
pub const CLONE_NEWPID: u64 = 0x2000_0000;
/// Set the parent TID in the child's memory.
pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
/// Clear the child TID in the child's memory on exit.
pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
/// Set the child TID in the child's memory.
pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;
/// Share the thread group (same TGID, i.e., POSIX thread).
pub const CLONE_THREAD: u64 = 0x0001_0000;
/// New mount namespace.
pub const CLONE_NEWNS: u64 = 0x0002_0000;
/// Share System V semaphore undo values.
pub const CLONE_SYSVSEM: u64 = 0x0004_0000;
/// New TLS for the child.
pub const CLONE_SETTLS: u64 = 0x0008_0000;
/// Create in new user namespace.
pub const CLONE_NEWUSER: u64 = 0x1000_0000;
/// Create in new network namespace.
pub const CLONE_NEWNET: u64 = 0x4000_0000;
/// Create in new IPC namespace.
pub const CLONE_NEWIPC: u64 = 0x0800_0000;
/// Create in new UTS namespace.
pub const CLONE_NEWUTS: u64 = 0x0400_0000;
/// Vfork semantics (parent suspended until child execs/exits).
pub const CLONE_VFORK: u64 = 0x0000_4000;
/// Create in new cgroup namespace.
pub const CLONE_NEWCGROUP: u64 = 0x0200_0000;

// ---------------------------------------------------------------------------
// Thread group limits
// ---------------------------------------------------------------------------

/// Default maximum threads per process.
pub const DEFAULT_MAX_THREADS: u32 = 32768;
/// Minimum thread stack size (bytes).
pub const MIN_THREAD_STACK: u32 = 16384;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_flags_power_of_two() {
        let flags: [u64; 18] = [
            CLONE_VM,
            CLONE_FS,
            CLONE_FILES,
            CLONE_SIGHAND,
            CLONE_NEWPID,
            CLONE_PARENT_SETTID,
            CLONE_CHILD_CLEARTID,
            CLONE_CHILD_SETTID,
            CLONE_THREAD,
            CLONE_NEWNS,
            CLONE_SYSVSEM,
            CLONE_SETTLS,
            CLONE_NEWUSER,
            CLONE_NEWNET,
            CLONE_NEWIPC,
            CLONE_NEWUTS,
            CLONE_VFORK,
            CLONE_NEWCGROUP,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {:#x} is not power of two", f);
        }
    }

    #[test]
    fn test_clone_flags_no_overlap() {
        let flags: [u64; 18] = [
            CLONE_VM,
            CLONE_FS,
            CLONE_FILES,
            CLONE_SIGHAND,
            CLONE_NEWPID,
            CLONE_PARENT_SETTID,
            CLONE_CHILD_CLEARTID,
            CLONE_CHILD_SETTID,
            CLONE_THREAD,
            CLONE_NEWNS,
            CLONE_SYSVSEM,
            CLONE_SETTLS,
            CLONE_NEWUSER,
            CLONE_NEWNET,
            CLONE_NEWIPC,
            CLONE_NEWUTS,
            CLONE_VFORK,
            CLONE_NEWCGROUP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(
                    flags[i] & flags[j],
                    0,
                    "flags {:#x} and {:#x} overlap",
                    flags[i],
                    flags[j]
                );
            }
        }
    }

    #[test]
    fn test_limits() {
        assert!(DEFAULT_MAX_THREADS > 0);
        assert!(MIN_THREAD_STACK > 0);
    }
}

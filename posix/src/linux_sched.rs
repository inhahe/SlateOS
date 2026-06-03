//! `<linux/sched.h>` — Linux-specific scheduling constants.
//!
//! Provides clone flags and scheduling policy extensions beyond
//! POSIX `<sched.h>`.

// Re-export base scheduling from sched.rs
pub use crate::sched::SCHED_FIFO;
pub use crate::sched::SCHED_OTHER;
pub use crate::sched::SCHED_RR;

// ---------------------------------------------------------------------------
// Scheduling policies (Linux extensions)
// ---------------------------------------------------------------------------

/// Batch scheduling (for CPU-intensive non-interactive tasks).
pub const SCHED_BATCH: i32 = 3;

/// Idle scheduling (only runs when nothing else wants the CPU).
pub const SCHED_IDLE: i32 = 5;

/// Deadline scheduling (earliest deadline first).
pub const SCHED_DEADLINE: i32 = 6;

/// Reset-on-fork flag (combine with policy).
pub const SCHED_RESET_ON_FORK: i32 = 0x40000000;

// ---------------------------------------------------------------------------
// Clone flags
// ---------------------------------------------------------------------------

/// Share virtual memory.
pub const CLONE_VM: u64 = 0x00000100;

/// Share filesystem info (root, cwd, umask).
pub const CLONE_FS: u64 = 0x00000200;

/// Share file descriptor table.
pub const CLONE_FILES: u64 = 0x00000400;

/// Share signal handlers.
pub const CLONE_SIGHAND: u64 = 0x00000800;

/// Place in same PID namespace.
pub const CLONE_NEWPID: u64 = 0x20000000;

/// Place in new mount namespace.
pub const CLONE_NEWNS: u64 = 0x00020000;

/// Place in new network namespace.
pub const CLONE_NEWNET: u64 = 0x40000000;

/// Place in new UTS namespace.
pub const CLONE_NEWUTS: u64 = 0x04000000;

/// Place in new IPC namespace.
pub const CLONE_NEWIPC: u64 = 0x08000000;

/// Place in new user namespace.
pub const CLONE_NEWUSER: u64 = 0x10000000;

/// Place in new cgroup namespace.
pub const CLONE_NEWCGROUP: u64 = 0x02000000;

/// Share thread group.
pub const CLONE_THREAD: u64 = 0x00010000;

/// Set the TLS (thread-local storage).
pub const CLONE_SETTLS: u64 = 0x00080000;

/// Set parent TID in parent's memory.
pub const CLONE_PARENT_SETTID: u64 = 0x00100000;

/// Clear child TID in child's memory on exit.
pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000;

/// Set child TID in child's memory.
pub const CLONE_CHILD_SETTID: u64 = 0x01000000;

/// Create clone detached.
pub const CLONE_DETACHED: u64 = 0x00400000;

/// Unused / untraced.
pub const CLONE_UNTRACED: u64 = 0x00800000;

/// Use vfork semantics (parent blocks until child exits/execs).
pub const CLONE_VFORK: u64 = 0x00004000;

/// Share I/O context.
pub const CLONE_IO: u64 = 0x80000000;

/// Set close-on-exec on the pidfd.
pub const CLONE_PIDFD: u64 = 0x00001000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sched_policies_distinct() {
        let policies = [
            SCHED_OTHER,
            SCHED_FIFO,
            SCHED_RR,
            SCHED_BATCH,
            SCHED_IDLE,
            SCHED_DEADLINE,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_sched_values() {
        assert_eq!(SCHED_OTHER, 0);
        assert_eq!(SCHED_FIFO, 1);
        assert_eq!(SCHED_RR, 2);
        assert_eq!(SCHED_BATCH, 3);
        assert_eq!(SCHED_IDLE, 5);
        assert_eq!(SCHED_DEADLINE, 6);
    }

    #[test]
    fn test_clone_flags_no_overlap() {
        let flags = [
            CLONE_VM,
            CLONE_FS,
            CLONE_FILES,
            CLONE_SIGHAND,
            CLONE_THREAD,
            CLONE_NEWNS,
            CLONE_SETTLS,
            CLONE_PARENT_SETTID,
            CLONE_CHILD_CLEARTID,
            CLONE_DETACHED,
            CLONE_UNTRACED,
            CLONE_VFORK,
            CLONE_PIDFD,
            CLONE_CHILD_SETTID,
        ];
        // Each flag should be a single bit (power of two).
        for &f in &flags {
            assert!(
                f.count_ones() == 1,
                "CLONE flag 0x{f:X} should be power of 2"
            );
        }
    }

    #[test]
    fn test_namespace_flags_distinct() {
        let ns = [
            CLONE_NEWPID,
            CLONE_NEWNS,
            CLONE_NEWNET,
            CLONE_NEWUTS,
            CLONE_NEWIPC,
            CLONE_NEWUSER,
            CLONE_NEWCGROUP,
        ];
        for i in 0..ns.len() {
            for j in (i + 1)..ns.len() {
                assert_ne!(ns[i], ns[j], "namespace flags must be distinct");
            }
        }
    }

    #[test]
    fn test_sched_reset_on_fork() {
        assert_ne!(SCHED_RESET_ON_FORK, 0);
        // Should be combinable with any policy.
        let combined = SCHED_FIFO | SCHED_RESET_ON_FORK;
        assert_ne!(combined, SCHED_FIFO);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(SCHED_OTHER, crate::sched::SCHED_OTHER);
        assert_eq!(SCHED_FIFO, crate::sched::SCHED_FIFO);
    }
}

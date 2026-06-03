//! `<linux/sched.h>` — unshare() flag constants.
//!
//! The unshare() system call disassociates parts of the calling
//! process's execution context that are currently shared with
//! other processes. Unlike clone() which creates sharing, unshare()
//! breaks sharing for the calling thread/process.

// ---------------------------------------------------------------------------
// unshare() flags (which contexts to unshare)
// ---------------------------------------------------------------------------

/// Unshare filesystem attributes (root, cwd, umask).
pub const UNSHARE_FS: u64 = 0x0000_0200;
/// Unshare file descriptor table (get private copy).
pub const UNSHARE_FILES: u64 = 0x0000_0400;
/// Create new mount namespace.
pub const UNSHARE_NEWNS: u64 = 0x0002_0000;
/// Create new UTS namespace.
pub const UNSHARE_NEWUTS: u64 = 0x0400_0000;
/// Create new IPC namespace.
pub const UNSHARE_NEWIPC: u64 = 0x0800_0000;
/// Create new user namespace.
pub const UNSHARE_NEWUSER: u64 = 0x1000_0000;
/// Create new PID namespace (for children).
pub const UNSHARE_NEWPID: u64 = 0x2000_0000;
/// Create new network namespace.
pub const UNSHARE_NEWNET: u64 = 0x4000_0000;
/// Create new cgroup namespace.
pub const UNSHARE_NEWCGROUP: u64 = 0x0200_0000;
/// Create new time namespace.
pub const UNSHARE_NEWTIME: u64 = 0x0000_0080;
/// Unshare System V semaphore undo values.
pub const UNSHARE_SYSVSEM: u64 = 0x0004_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unshare_flags_no_overlap() {
        let flags = [
            UNSHARE_FS,
            UNSHARE_FILES,
            UNSHARE_NEWNS,
            UNSHARE_NEWUTS,
            UNSHARE_NEWIPC,
            UNSHARE_NEWUSER,
            UNSHARE_NEWPID,
            UNSHARE_NEWNET,
            UNSHARE_NEWCGROUP,
            UNSHARE_NEWTIME,
            UNSHARE_SYSVSEM,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_unshare_newns_value() {
        assert_eq!(UNSHARE_NEWNS, 0x0002_0000);
    }
}

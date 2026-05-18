//! `<linux/sched.h>` — Linux namespace type flag constants.
//!
//! Namespaces provide process isolation by virtualizing global
//! resources (PIDs, network, mount points, etc.). Each namespace
//! type is identified by a flag used with clone(), unshare(), and
//! setns() to create or enter namespace instances.

// ---------------------------------------------------------------------------
// Namespace type flags (CLONE_NEW*)
// ---------------------------------------------------------------------------

/// Mount namespace (isolated mount points).
pub const CLONE_NEWNS: u64 = 0x0002_0000;
/// UTS namespace (hostname, domainname).
pub const CLONE_NEWUTS: u64 = 0x0400_0000;
/// IPC namespace (System V IPC, POSIX MQ).
pub const CLONE_NEWIPC: u64 = 0x0800_0000;
/// User namespace (UID/GID mapping).
pub const CLONE_NEWUSER: u64 = 0x1000_0000;
/// PID namespace (process ID space).
pub const CLONE_NEWPID: u64 = 0x2000_0000;
/// Network namespace (network stack).
pub const CLONE_NEWNET: u64 = 0x4000_0000;
/// Cgroup namespace (cgroup root visibility).
pub const CLONE_NEWCGROUP: u64 = 0x0200_0000;
/// Time namespace (CLOCK_MONOTONIC, CLOCK_BOOTTIME offsets).
pub const CLONE_NEWTIME: u64 = 0x0000_0080;

// ---------------------------------------------------------------------------
// Namespace file paths (/proc/[pid]/ns/*)
// ---------------------------------------------------------------------------

/// Inode type for namespace procfs entries.
pub const NSTYPE_MNT: u32 = 0;
/// UTS namespace.
pub const NSTYPE_UTS: u32 = 1;
/// IPC namespace.
pub const NSTYPE_IPC: u32 = 2;
/// Network namespace.
pub const NSTYPE_NET: u32 = 3;
/// PID namespace.
pub const NSTYPE_PID: u32 = 4;
/// User namespace.
pub const NSTYPE_USER: u32 = 5;
/// Cgroup namespace.
pub const NSTYPE_CGROUP: u32 = 6;
/// Time namespace.
pub const NSTYPE_TIME: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_new_flags_no_overlap() {
        let flags = [
            CLONE_NEWNS, CLONE_NEWUTS, CLONE_NEWIPC,
            CLONE_NEWUSER, CLONE_NEWPID, CLONE_NEWNET,
            CLONE_NEWCGROUP, CLONE_NEWTIME,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ns_types_distinct() {
        let types = [
            NSTYPE_MNT, NSTYPE_UTS, NSTYPE_IPC, NSTYPE_NET,
            NSTYPE_PID, NSTYPE_USER, NSTYPE_CGROUP, NSTYPE_TIME,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_clone_newns_value() {
        assert_eq!(CLONE_NEWNS, 0x0002_0000);
    }
}

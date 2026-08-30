//! `<linux/sched.h>` — Namespace type constants.
//!
//! Linux namespaces provide isolation of global system resources.
//! Each namespace type wraps a particular global resource (PIDs,
//! network stack, mount table, etc.) so that processes in different
//! namespaces have independent views. Namespaces are the foundation
//! of container technologies like Docker and LXC.

// ---------------------------------------------------------------------------
// Namespace type flags (for clone/unshare/setns)
// ---------------------------------------------------------------------------

/// Mount namespace (isolated mount table).
pub const CLONE_NEWNS: u32 = 0x0002_0000;
/// UTS namespace (hostname, domain name).
pub const CLONE_NEWUTS: u32 = 0x0400_0000;
/// IPC namespace (SysV IPC, POSIX MQ).
pub const CLONE_NEWIPC: u32 = 0x0800_0000;
/// User namespace (UID/GID mapping).
pub const CLONE_NEWUSER: u32 = 0x1000_0000;
/// PID namespace (process IDs).
pub const CLONE_NEWPID: u32 = 0x2000_0000;
/// Network namespace (network stack).
pub const CLONE_NEWNET: u32 = 0x4000_0000;
/// Cgroup namespace.
pub const CLONE_NEWCGROUP: u32 = 0x0200_0000;
/// Time namespace (CLOCK_MONOTONIC/BOOTTIME offsets).
pub const CLONE_NEWTIME: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// /proc/[pid]/ns type identifiers (NSTYPE for setns)
// ---------------------------------------------------------------------------

/// Mount namespace identifier.
pub const NSTYPE_MNT: u32 = 0x0002_0000;
/// UTS namespace identifier.
pub const NSTYPE_UTS: u32 = 0x0400_0000;
/// IPC namespace identifier.
pub const NSTYPE_IPC: u32 = 0x0800_0000;
/// User namespace identifier.
pub const NSTYPE_USER: u32 = 0x1000_0000;
/// PID namespace identifier.
pub const NSTYPE_PID: u32 = 0x2000_0000;
/// Network namespace identifier.
pub const NSTYPE_NET: u32 = 0x4000_0000;
/// Cgroup namespace identifier.
pub const NSTYPE_CGROUP: u32 = 0x0200_0000;
/// Time namespace identifier.
pub const NSTYPE_TIME: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// Namespace limits
// ---------------------------------------------------------------------------

/// Maximum nesting depth of user namespaces.
pub const MAX_USER_NS_NESTING: u32 = 32;
/// Maximum nesting depth of PID namespaces.
pub const MAX_PID_NS_NESTING: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ns_flags_no_overlap() {
        let flags = [
            CLONE_NEWNS,
            CLONE_NEWUTS,
            CLONE_NEWIPC,
            CLONE_NEWUSER,
            CLONE_NEWPID,
            CLONE_NEWNET,
            CLONE_NEWCGROUP,
            CLONE_NEWTIME,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_nstype_matches_clone_flag() {
        assert_eq!(NSTYPE_MNT, CLONE_NEWNS);
        assert_eq!(NSTYPE_UTS, CLONE_NEWUTS);
        assert_eq!(NSTYPE_IPC, CLONE_NEWIPC);
        assert_eq!(NSTYPE_USER, CLONE_NEWUSER);
        assert_eq!(NSTYPE_PID, CLONE_NEWPID);
        assert_eq!(NSTYPE_NET, CLONE_NEWNET);
        assert_eq!(NSTYPE_CGROUP, CLONE_NEWCGROUP);
        assert_eq!(NSTYPE_TIME, CLONE_NEWTIME);
    }

    #[test]
    fn test_nesting_limits_positive() {
        assert!(MAX_USER_NS_NESTING > 0);
        assert!(MAX_PID_NS_NESTING > 0);
    }
}

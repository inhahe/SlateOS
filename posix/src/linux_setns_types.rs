//! `<linux/sched.h>` — setns() flag and namespace fd constants.
//!
//! The setns() system call allows a process to join an existing
//! namespace instance specified by a file descriptor (from
//! /proc/[pid]/ns/* or from an open namespace fd). The nstype
//! parameter validates the fd matches the expected namespace type.

// ---------------------------------------------------------------------------
// setns() nstype values (0 = auto-detect from fd)
// ---------------------------------------------------------------------------

/// Auto-detect namespace type from fd.
pub const SETNS_TYPE_AUTO: u32 = 0;
/// Expect mount namespace fd.
pub const SETNS_TYPE_MNT: u32 = 0x0002_0000;
/// Expect UTS namespace fd.
pub const SETNS_TYPE_UTS: u32 = 0x0400_0000;
/// Expect IPC namespace fd.
pub const SETNS_TYPE_IPC: u32 = 0x0800_0000;
/// Expect user namespace fd.
pub const SETNS_TYPE_USER: u32 = 0x1000_0000;
/// Expect PID namespace fd.
pub const SETNS_TYPE_PID: u32 = 0x2000_0000;
/// Expect network namespace fd.
pub const SETNS_TYPE_NET: u32 = 0x4000_0000;
/// Expect cgroup namespace fd.
pub const SETNS_TYPE_CGROUP: u32 = 0x0200_0000;
/// Expect time namespace fd.
pub const SETNS_TYPE_TIME: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// Namespace procfs paths (for obtaining fd via open())
// ---------------------------------------------------------------------------

/// Maximum namespace path length ("/proc/<pid>/ns/<type>").
pub const NS_PATH_MAX: u32 = 64;

// ---------------------------------------------------------------------------
// ioctl commands for namespace fds
// ---------------------------------------------------------------------------

/// Get the namespace type from an nsfd.
pub const NS_GET_NSTYPE: u32 = 0xB701;
/// Get the owning user namespace.
pub const NS_GET_USERNS: u32 = 0xB702;
/// Get the parent namespace (for hierarchical ns like PID, user).
pub const NS_GET_PARENT: u32 = 0xB703;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setns_types_no_overlap() {
        let types = [
            SETNS_TYPE_MNT,
            SETNS_TYPE_UTS,
            SETNS_TYPE_IPC,
            SETNS_TYPE_USER,
            SETNS_TYPE_PID,
            SETNS_TYPE_NET,
            SETNS_TYPE_CGROUP,
            SETNS_TYPE_TIME,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_auto_is_zero() {
        assert_eq!(SETNS_TYPE_AUTO, 0);
    }

    #[test]
    fn test_ns_ioctls_distinct() {
        assert_ne!(NS_GET_NSTYPE, NS_GET_USERNS);
        assert_ne!(NS_GET_USERNS, NS_GET_PARENT);
        assert_ne!(NS_GET_NSTYPE, NS_GET_PARENT);
    }
}

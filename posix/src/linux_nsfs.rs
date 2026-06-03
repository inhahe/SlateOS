//! `<linux/nsfs.h>` — namespace filesystem ioctls.
//!
//! Namespace file descriptors (obtained from `/proc/[pid]/ns/*` or
//! `clone(2)` with `CLONE_NEW*`) support these ioctls for querying
//! namespace relationships and ownership.

// ---------------------------------------------------------------------------
// Namespace ioctl commands
// ---------------------------------------------------------------------------

/// Get the namespace type (CLONE_NEW* constant).
pub const NS_GET_NSTYPE: u64 = 0xB701;
/// Get the owning user namespace.
pub const NS_GET_USERNS: u64 = 0xB701 + 1;
/// Get the parent namespace.
pub const NS_GET_PARENT: u64 = 0xB701 + 2;
/// Get the owner UID.
pub const NS_GET_OWNER_UID: u64 = 0xB701 + 4;

// ---------------------------------------------------------------------------
// CLONE_NEW* flags (namespace types)
// ---------------------------------------------------------------------------

/// New mount namespace.
pub const CLONE_NEWNS: u64 = 0x00020000;
/// New cgroup namespace.
pub const CLONE_NEWCGROUP: u64 = 0x02000000;
/// New UTS namespace.
pub const CLONE_NEWUTS: u64 = 0x04000000;
/// New IPC namespace.
pub const CLONE_NEWIPC: u64 = 0x08000000;
/// New user namespace.
pub const CLONE_NEWUSER: u64 = 0x10000000;
/// New PID namespace.
pub const CLONE_NEWPID: u64 = 0x20000000;
/// New network namespace.
pub const CLONE_NEWNET: u64 = 0x40000000;
/// New time namespace.
pub const CLONE_NEWTIME: u64 = 0x00000080;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ns_ioctls_distinct() {
        let cmds = [
            NS_GET_NSTYPE,
            NS_GET_USERNS,
            NS_GET_PARENT,
            NS_GET_OWNER_UID,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_clone_new_flags_distinct() {
        let flags = [
            CLONE_NEWNS,
            CLONE_NEWCGROUP,
            CLONE_NEWUTS,
            CLONE_NEWIPC,
            CLONE_NEWUSER,
            CLONE_NEWPID,
            CLONE_NEWNET,
            CLONE_NEWTIME,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_clone_flags_no_overlap() {
        // Each CLONE_NEW* should be a distinct bit.
        let combined = CLONE_NEWNS
            | CLONE_NEWCGROUP
            | CLONE_NEWUTS
            | CLONE_NEWIPC
            | CLONE_NEWUSER
            | CLONE_NEWPID
            | CLONE_NEWNET
            | CLONE_NEWTIME;
        // Count set bits — should equal number of flags.
        assert_eq!(combined.count_ones(), 8);
    }

    #[test]
    fn test_ns_get_nstype_value() {
        assert_eq!(NS_GET_NSTYPE, 0xB701);
    }
}

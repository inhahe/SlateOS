//! `<linux/proc_ns.h>` — /proc namespace interface constants.
//!
//! Each process's namespaces are exposed as symbolic links under
//! /proc/[pid]/ns/. These can be opened and passed to setns() to
//! enter an existing namespace, or bind-mounted to keep a namespace
//! alive after all processes have exited. The inode number of each
//! ns link uniquely identifies that namespace instance.

// ---------------------------------------------------------------------------
// /proc/[pid]/ns/ entry names
// ---------------------------------------------------------------------------

/// Mount namespace entry.
pub const PROC_NS_MNT: &str = "mnt";
/// UTS namespace entry.
pub const PROC_NS_UTS: &str = "uts";
/// IPC namespace entry.
pub const PROC_NS_IPC: &str = "ipc";
/// Network namespace entry.
pub const PROC_NS_NET: &str = "net";
/// PID namespace entry.
pub const PROC_NS_PID: &str = "pid";
/// PID namespace for children.
pub const PROC_NS_PID_FOR_CHILDREN: &str = "pid_for_children";
/// User namespace entry.
pub const PROC_NS_USER: &str = "user";
/// Cgroup namespace entry.
pub const PROC_NS_CGROUP: &str = "cgroup";
/// Time namespace entry.
pub const PROC_NS_TIME: &str = "time";
/// Time namespace for children.
pub const PROC_NS_TIME_FOR_CHILDREN: &str = "time_for_children";

// ---------------------------------------------------------------------------
// ioctl commands on ns file descriptors
// ---------------------------------------------------------------------------

/// Get the owning user namespace fd.
pub const NS_GET_USERNS: u32 = 0xB701;
/// Get the parent namespace fd.
pub const NS_GET_PARENT: u32 = 0xB702;
/// Get the namespace type (CLONE_NEW* flag).
pub const NS_GET_NSTYPE: u32 = 0xB703;
/// Get the owner UID of the namespace.
pub const NS_GET_OWNER_UID: u32 = 0xB704;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ns_names_distinct() {
        let names = [
            PROC_NS_MNT, PROC_NS_UTS, PROC_NS_IPC, PROC_NS_NET,
            PROC_NS_PID, PROC_NS_PID_FOR_CHILDREN, PROC_NS_USER,
            PROC_NS_CGROUP, PROC_NS_TIME, PROC_NS_TIME_FOR_CHILDREN,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [NS_GET_USERNS, NS_GET_PARENT, NS_GET_NSTYPE, NS_GET_OWNER_UID];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_ns_names_nonempty() {
        let names = [
            PROC_NS_MNT, PROC_NS_UTS, PROC_NS_IPC, PROC_NS_NET,
            PROC_NS_PID, PROC_NS_USER, PROC_NS_CGROUP, PROC_NS_TIME,
        ];
        for name in &names {
            assert!(!name.is_empty());
        }
    }

    #[test]
    fn test_ioctl_magic() {
        // All NS ioctls use 0xB7 magic
        assert_eq!(NS_GET_USERNS >> 8, 0xB7);
        assert_eq!(NS_GET_PARENT >> 8, 0xB7);
        assert_eq!(NS_GET_NSTYPE >> 8, 0xB7);
        assert_eq!(NS_GET_OWNER_UID >> 8, 0xB7);
    }
}

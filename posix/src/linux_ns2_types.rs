//! `<linux/sched.h>` — Additional namespace constants.
//!
//! Supplementary namespace constants covering clone flags,
//! namespace types, and setns flags.

// ---------------------------------------------------------------------------
// Namespace clone flags (CLONE_NEW*)
// ---------------------------------------------------------------------------

/// New mount namespace.
pub const CLONE_NEWNS: u64 = 0x00020000;
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
/// New cgroup namespace.
pub const CLONE_NEWCGROUP: u64 = 0x02000000;
/// New time namespace.
pub const CLONE_NEWTIME: u64 = 0x00000080;

// ---------------------------------------------------------------------------
// Namespace types (/proc/pid/ns/)
// ---------------------------------------------------------------------------

/// Mount namespace inode magic.
pub const NSTYPE_MNT: u32 = 0;
/// UTS namespace.
pub const NSTYPE_UTS: u32 = 1;
/// IPC namespace.
pub const NSTYPE_IPC: u32 = 2;
/// User namespace.
pub const NSTYPE_USER: u32 = 3;
/// PID namespace.
pub const NSTYPE_PID: u32 = 4;
/// Network namespace.
pub const NSTYPE_NET: u32 = 5;
/// Cgroup namespace.
pub const NSTYPE_CGROUP: u32 = 6;
/// Time namespace.
pub const NSTYPE_TIME: u32 = 7;

// ---------------------------------------------------------------------------
// Namespace ioctl commands
// ---------------------------------------------------------------------------

/// Get namespace type.
pub const NS_GET_NSTYPE: u32 = 0xB701;
/// Get owner UID.
pub const NS_GET_OWNER_UID: u32 = 0xB704;
/// Get parent namespace.
pub const NS_GET_PARENT: u32 = 0xB702;
/// Get user namespace.
pub const NS_GET_USERNS: u32 = 0xB703;

// ---------------------------------------------------------------------------
// Unshare flags
// ---------------------------------------------------------------------------

/// Unshare files.
pub const CLONE_FILES: u64 = 0x00000400;
/// Unshare filesystem info.
pub const CLONE_FS: u64 = 0x00000200;
/// Unshare signal handlers.
pub const CLONE_SIGHAND: u64 = 0x00000800;
/// Unshare VM.
pub const CLONE_VM: u64 = 0x00000100;
/// Unshare thread.
pub const CLONE_THREAD: u64 = 0x00010000;
/// Unshare sysvsem.
pub const CLONE_SYSVSEM: u64 = 0x00040000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_new_distinct() {
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
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_clone_new_no_overlap() {
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
                assert_eq!(
                    flags[i] & flags[j],
                    0,
                    "0x{:08x} & 0x{:08x}",
                    flags[i],
                    flags[j]
                );
            }
        }
    }

    #[test]
    fn test_nstype_distinct() {
        let types = [
            NSTYPE_MNT,
            NSTYPE_UTS,
            NSTYPE_IPC,
            NSTYPE_USER,
            NSTYPE_PID,
            NSTYPE_NET,
            NSTYPE_CGROUP,
            NSTYPE_TIME,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ns_ioctls_distinct() {
        let cmds = [
            NS_GET_NSTYPE,
            NS_GET_OWNER_UID,
            NS_GET_PARENT,
            NS_GET_USERNS,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_unshare_flags_no_overlap() {
        let flags = [
            CLONE_FILES,
            CLONE_FS,
            CLONE_SIGHAND,
            CLONE_VM,
            CLONE_THREAD,
            CLONE_SYSVSEM,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

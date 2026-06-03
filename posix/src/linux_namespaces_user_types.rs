//! `<sched.h>` and `<linux/nsfs.h>` — namespace creation and inspection ABI.
//!
//! Linux namespaces are how containers (Docker, podman, systemd-nspawn,
//! lxc) and sandboxes (bubblewrap, Firejail, browser content processes)
//! isolate processes. `clone(2)` / `unshare(2)` / `setns(2)` flip these
//! flags and `/proc/[pid]/ns/*` exposes per-process namespace handles.

// ---------------------------------------------------------------------------
// `clone(2)` / `unshare(2)` namespace flags
// ---------------------------------------------------------------------------

pub const CLONE_NEWNS: u32 = 0x0002_0000;
pub const CLONE_NEWCGROUP: u32 = 0x0200_0000;
pub const CLONE_NEWUTS: u32 = 0x0400_0000;
pub const CLONE_NEWIPC: u32 = 0x0800_0000;
pub const CLONE_NEWUSER: u32 = 0x1000_0000;
pub const CLONE_NEWPID: u32 = 0x2000_0000;
pub const CLONE_NEWNET: u32 = 0x4000_0000;
pub const CLONE_NEWTIME: u32 = 0x0000_0080;

/// Mask covering every namespace flag.
pub const CLONE_NEW_ALL: u32 = CLONE_NEWNS
    | CLONE_NEWCGROUP
    | CLONE_NEWUTS
    | CLONE_NEWIPC
    | CLONE_NEWUSER
    | CLONE_NEWPID
    | CLONE_NEWNET
    | CLONE_NEWTIME;

// ---------------------------------------------------------------------------
// `/proc/[pid]/ns/*` symlink names
// ---------------------------------------------------------------------------

pub const NS_MOUNT: &str = "mnt";
pub const NS_CGROUP: &str = "cgroup";
pub const NS_UTS: &str = "uts";
pub const NS_IPC: &str = "ipc";
pub const NS_USER: &str = "user";
pub const NS_PID: &str = "pid";
pub const NS_NET: &str = "net";
pub const NS_TIME: &str = "time";
pub const NS_PID_FOR_CHILDREN: &str = "pid_for_children";
pub const NS_TIME_FOR_CHILDREN: &str = "time_for_children";

// ---------------------------------------------------------------------------
// `nsfs` ioctls (magic '!' = 0xb7 in modern kernels but historically `0xb7`)
// ---------------------------------------------------------------------------

/// `NS_GET_USERNS` — get the user-namespace owning this namespace.
pub const NS_GET_USERNS: u32 = 0xB701;
/// `NS_GET_PARENT` — get the parent namespace (PID/USER hierarchies).
pub const NS_GET_PARENT: u32 = 0xB702;
/// `NS_GET_NSTYPE` — return the CLONE_NEW* flag for this fd.
pub const NS_GET_NSTYPE: u32 = 0xB703;
/// `NS_GET_OWNER_UID` — owner UID of the user namespace.
pub const NS_GET_OWNER_UID: u32 = 0xB704;
/// `NS_GET_MNTNS_ID` — 64-bit unique id of the mount namespace.
pub const NS_GET_MNTNS_ID: u32 = 0xB705;
/// `NS_GET_PID_FROM_PIDNS` — translate pid into the target pid namespace.
pub const NS_GET_PID_FROM_PIDNS: u32 = 0xB706;
/// `NS_GET_TGID_FROM_PIDNS` — translate tgid into the target pid namespace.
pub const NS_GET_TGID_FROM_PIDNS: u32 = 0xB707;
/// `NS_GET_PID_IN_PIDNS` — translate pid from the target pid namespace.
pub const NS_GET_PID_IN_PIDNS: u32 = 0xB708;
/// `NS_GET_TGID_IN_PIDNS` — translate tgid from the target pid namespace.
pub const NS_GET_TGID_IN_PIDNS: u32 = 0xB709;

// ---------------------------------------------------------------------------
// Syscall numbers (x86_64)
// ---------------------------------------------------------------------------

pub const NR_UNSHARE: u32 = 272;
pub const NR_SETNS: u32 = 308;
pub const NR_CLONE3: u32 = 435;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_new_flags_single_bit_and_distinct() {
        let f = [
            CLONE_NEWNS,
            CLONE_NEWCGROUP,
            CLONE_NEWUTS,
            CLONE_NEWIPC,
            CLONE_NEWUSER,
            CLONE_NEWPID,
            CLONE_NEWNET,
            CLONE_NEWTIME,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
        // ALL is the bitwise OR.
        assert_eq!(CLONE_NEW_ALL, f.iter().fold(0u32, |a, b| a | b));
    }

    #[test]
    fn test_nsfs_ioctl_block() {
        // All NS_GET_* share the 0xB7 type byte.
        let i = [
            NS_GET_USERNS,
            NS_GET_PARENT,
            NS_GET_NSTYPE,
            NS_GET_OWNER_UID,
            NS_GET_MNTNS_ID,
            NS_GET_PID_FROM_PIDNS,
            NS_GET_TGID_FROM_PIDNS,
            NS_GET_PID_IN_PIDNS,
            NS_GET_TGID_IN_PIDNS,
        ];
        for v in i {
            assert_eq!(v >> 8, 0xB7);
        }
        // Low byte values are dense 1..9.
        for (idx, &v) in i.iter().enumerate() {
            assert_eq!(v & 0xFF, idx as u32 + 1);
        }
    }

    #[test]
    fn test_ns_names_distinct() {
        let n = [
            NS_MOUNT,
            NS_CGROUP,
            NS_UTS,
            NS_IPC,
            NS_USER,
            NS_PID,
            NS_NET,
            NS_TIME,
            NS_PID_FOR_CHILDREN,
            NS_TIME_FOR_CHILDREN,
        ];
        for i in 0..n.len() {
            for j in (i + 1)..n.len() {
                assert_ne!(n[i], n[j]);
            }
        }
        // Historical mnt name (not "mount").
        assert_eq!(NS_MOUNT, "mnt");
    }

    #[test]
    fn test_syscall_numbers() {
        assert_eq!(NR_UNSHARE, 272);
        assert_eq!(NR_SETNS, 308);
        assert_eq!(NR_CLONE3, 435);
        // clone3 is much newer than unshare/setns.
        assert!(NR_CLONE3 > NR_SETNS);
        assert!(NR_SETNS > NR_UNSHARE);
    }
}

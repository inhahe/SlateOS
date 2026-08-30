//! `<selinux/selinux.h>` — SELinux userspace ABI.
//!
//! SELinux exposes its state through the `selinuxfs` pseudo-FS
//! (mounted at `/sys/fs/selinux`) and reads policy decisions via
//! the AVC. `getenforce`, `setenforce`, `restorecon`, `runcon`,
//! and every `libselinux` client read/write the files and labels
//! below.

// ---------------------------------------------------------------------------
// Mount point and core selinuxfs files
// ---------------------------------------------------------------------------

pub const SELINUX_MOUNT: &str = "/sys/fs/selinux";
pub const SELINUXFS_FSTYPE: &str = "selinuxfs";

pub const SELINUXFS_ENFORCE: &str = "/sys/fs/selinux/enforce";
pub const SELINUXFS_POLICYVERS: &str = "/sys/fs/selinux/policyvers";
pub const SELINUXFS_MLS: &str = "/sys/fs/selinux/mls";
pub const SELINUXFS_DENY_UNKNOWN: &str = "/sys/fs/selinux/deny_unknown";
pub const SELINUXFS_REJECT_UNKNOWN: &str = "/sys/fs/selinux/reject_unknown";
pub const SELINUXFS_ACCESS: &str = "/sys/fs/selinux/access";
pub const SELINUXFS_LOAD: &str = "/sys/fs/selinux/load";
pub const SELINUXFS_CONTEXT: &str = "/sys/fs/selinux/context";
pub const SELINUXFS_CHECKREQPROT: &str = "/sys/fs/selinux/checkreqprot";

// ---------------------------------------------------------------------------
// Process-context files in /proc
// ---------------------------------------------------------------------------

pub const PROC_SELF_ATTR_CURRENT: &str = "/proc/self/attr/current";
pub const PROC_SELF_ATTR_PREV: &str = "/proc/self/attr/prev";
pub const PROC_SELF_ATTR_EXEC: &str = "/proc/self/attr/exec";
pub const PROC_SELF_ATTR_FSCREATE: &str = "/proc/self/attr/fscreate";
pub const PROC_SELF_ATTR_KEYCREATE: &str = "/proc/self/attr/keycreate";
pub const PROC_SELF_ATTR_SOCKCREATE: &str = "/proc/self/attr/sockcreate";

// ---------------------------------------------------------------------------
// xattr that carries a file's security context
// ---------------------------------------------------------------------------

pub const XATTR_NAME_SELINUX: &str = "security.selinux";

// ---------------------------------------------------------------------------
// Enforce-mode states (`/sys/fs/selinux/enforce`)
// ---------------------------------------------------------------------------

pub const SELINUX_PERMISSIVE: u32 = 0;
pub const SELINUX_ENFORCING: u32 = 1;
pub const SELINUX_DISABLED: i32 = -1;

// ---------------------------------------------------------------------------
// AVC decision flags (from `<linux/selinux_netlink.h>`)
// ---------------------------------------------------------------------------

pub const AVC_AUDIT_ALLOW: u32 = 1 << 0;
pub const AVC_AUDIT_AUDITALLOW: u32 = 1 << 1;
pub const AVC_AUDIT_DONTAUDIT: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Policy version range understood by current kernels (subset)
// ---------------------------------------------------------------------------

pub const POLICYDB_VERSION_BASE: u32 = 15;
pub const POLICYDB_VERSION_XPERMS_IOCTL: u32 = 30;
pub const POLICYDB_VERSION_MAX: u32 = 33;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selinuxfs_paths_under_mountpoint() {
        let p = [
            SELINUXFS_ENFORCE,
            SELINUXFS_POLICYVERS,
            SELINUXFS_MLS,
            SELINUXFS_DENY_UNKNOWN,
            SELINUXFS_REJECT_UNKNOWN,
            SELINUXFS_ACCESS,
            SELINUXFS_LOAD,
            SELINUXFS_CONTEXT,
            SELINUXFS_CHECKREQPROT,
        ];
        for path in p {
            assert!(path.starts_with(SELINUX_MOUNT));
        }
    }

    #[test]
    fn test_proc_attr_paths_under_attr_dir() {
        let p = [
            PROC_SELF_ATTR_CURRENT,
            PROC_SELF_ATTR_PREV,
            PROC_SELF_ATTR_EXEC,
            PROC_SELF_ATTR_FSCREATE,
            PROC_SELF_ATTR_KEYCREATE,
            PROC_SELF_ATTR_SOCKCREATE,
        ];
        for path in p {
            assert!(path.starts_with("/proc/self/attr/"));
        }
    }

    #[test]
    fn test_xattr_name_security_namespace() {
        assert!(XATTR_NAME_SELINUX.starts_with("security."));
        assert_eq!(XATTR_NAME_SELINUX, "security.selinux");
    }

    #[test]
    fn test_enforce_states() {
        assert_eq!(SELINUX_PERMISSIVE, 0);
        assert_eq!(SELINUX_ENFORCING, 1);
        // DISABLED is the only state that is signed and negative —
        // matches the `selinux_status_open()` API.
        assert_eq!(SELINUX_DISABLED, -1);
    }

    #[test]
    fn test_avc_audit_flags_single_bit() {
        let a = [AVC_AUDIT_ALLOW, AVC_AUDIT_AUDITALLOW, AVC_AUDIT_DONTAUDIT];
        let mut or = 0u32;
        for v in a {
            assert!(v.is_power_of_two());
            or |= v;
        }
        assert_eq!(or, 0x7);
    }

    #[test]
    fn test_policy_version_bounds_monotonic() {
        assert!(POLICYDB_VERSION_BASE <= POLICYDB_VERSION_XPERMS_IOCTL);
        assert!(POLICYDB_VERSION_XPERMS_IOCTL <= POLICYDB_VERSION_MAX);
        // xperms ioctl was added at policy v30.
        assert_eq!(POLICYDB_VERSION_XPERMS_IOCTL, 30);
    }
}

//! `<linux/capability.h>` — Linux capability constants.
//!
//! Re-exports all capability constants and structures from
//! `sys_capability`. This is the kernel-header path (`<linux/capability.h>`)
//! vs. the userspace path (`<sys/capability.h>`).

// ---------------------------------------------------------------------------
// Re-exports from sys_capability
// ---------------------------------------------------------------------------

pub use crate::sys_capability::_LINUX_CAPABILITY_U32S_3;
pub use crate::sys_capability::_LINUX_CAPABILITY_VERSION_3;
pub use crate::sys_capability::CapUserData;
pub use crate::sys_capability::CapUserHeader;

pub use crate::sys_capability::CAP_AUDIT_CONTROL;
pub use crate::sys_capability::CAP_AUDIT_READ;
pub use crate::sys_capability::CAP_AUDIT_WRITE;
pub use crate::sys_capability::CAP_BLOCK_SUSPEND;
pub use crate::sys_capability::CAP_BPF;
pub use crate::sys_capability::CAP_CHECKPOINT_RESTORE;
pub use crate::sys_capability::CAP_CHOWN;
pub use crate::sys_capability::CAP_DAC_OVERRIDE;
pub use crate::sys_capability::CAP_DAC_READ_SEARCH;
pub use crate::sys_capability::CAP_FOWNER;
pub use crate::sys_capability::CAP_FSETID;
pub use crate::sys_capability::CAP_IPC_LOCK;
pub use crate::sys_capability::CAP_IPC_OWNER;
pub use crate::sys_capability::CAP_KILL;
pub use crate::sys_capability::CAP_LAST_CAP;
pub use crate::sys_capability::CAP_MAC_ADMIN;
pub use crate::sys_capability::CAP_MAC_OVERRIDE;
pub use crate::sys_capability::CAP_MKNOD;
pub use crate::sys_capability::CAP_NET_ADMIN;
pub use crate::sys_capability::CAP_NET_BIND_SERVICE;
pub use crate::sys_capability::CAP_NET_RAW;
pub use crate::sys_capability::CAP_PERFMON;
pub use crate::sys_capability::CAP_SETFCAP;
pub use crate::sys_capability::CAP_SETGID;
pub use crate::sys_capability::CAP_SETPCAP;
pub use crate::sys_capability::CAP_SETUID;
pub use crate::sys_capability::CAP_SYS_ADMIN;
pub use crate::sys_capability::CAP_SYS_BOOT;
pub use crate::sys_capability::CAP_SYS_CHROOT;
pub use crate::sys_capability::CAP_SYS_MODULE;
pub use crate::sys_capability::CAP_SYS_NICE;
pub use crate::sys_capability::CAP_SYS_PACCT;
pub use crate::sys_capability::CAP_SYS_PTRACE;
pub use crate::sys_capability::CAP_SYS_RAWIO;
pub use crate::sys_capability::CAP_SYS_RESOURCE;
pub use crate::sys_capability::CAP_SYS_TIME;
pub use crate::sys_capability::CAP_SYS_TTY_CONFIG;
pub use crate::sys_capability::CAP_SYSLOG;
pub use crate::sys_capability::CAP_WAKE_ALARM;

// ---------------------------------------------------------------------------
// Capability bit conversion helpers
// ---------------------------------------------------------------------------

/// Convert a capability number to a bitmask index (for CapUserData).
///
/// Capabilities 0..31 are in data[0], 32..63 are in data[1].
pub const fn cap_to_index(cap: u32) -> usize {
    (cap >> 5) as usize
}

/// Convert a capability number to a bitmask within its u32 word.
pub const fn cap_to_mask(cap: u32) -> u32 {
    1 << (cap & 31)
}

// ---------------------------------------------------------------------------
// VFS capability version
// ---------------------------------------------------------------------------

/// v2 capability structure version.
pub const VFS_CAP_REVISION_2: u32 = 0x02000000;
/// v3 capability structure version (namespace-aware).
pub const VFS_CAP_REVISION_3: u32 = 0x02000080;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_values() {
        assert_eq!(CAP_CHOWN, 0);
        assert_eq!(CAP_SYS_ADMIN, 21);
        assert_eq!(CAP_LAST_CAP, 40);
    }

    #[test]
    fn test_cap_to_index() {
        assert_eq!(cap_to_index(0), 0);
        assert_eq!(cap_to_index(31), 0);
        assert_eq!(cap_to_index(32), 1);
        assert_eq!(cap_to_index(40), 1);
    }

    #[test]
    fn test_cap_to_mask() {
        assert_eq!(cap_to_mask(0), 1);
        assert_eq!(cap_to_mask(1), 2);
        assert_eq!(cap_to_mask(31), 1 << 31);
        assert_eq!(cap_to_mask(32), 1); // wraps to data[1]
    }

    #[test]
    fn test_vfs_cap_versions() {
        assert_ne!(VFS_CAP_REVISION_2, VFS_CAP_REVISION_3);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(CAP_CHOWN, crate::sys_capability::CAP_CHOWN);
        assert_eq!(CAP_SYS_ADMIN, crate::sys_capability::CAP_SYS_ADMIN);
        assert_eq!(CAP_LAST_CAP, crate::sys_capability::CAP_LAST_CAP);
    }
}

//! `<linux/landlock.h>` — unprivileged access-control (sandboxing).
//!
//! Landlock is a stackable Linux security module that allows
//! unprivileged processes to restrict their own access rights
//! (filesystem, network) without needing root or a security policy.
//! Available since Linux 5.13.
//!
//! ## There is no Landlock here
//!
//! The kernel does not enforce Landlock, so this module is the header only:
//! the rule types, access rights, flags and attribute structures, for
//! programs that build the calls. The calls themselves are Linux's three
//! system calls, which glibc does not wrap -- a program makes them with
//! `syscall(SYS_landlock_create_ruleset, …)` -- and `syscall` answers them
//! `ENOSYS`, as a kernel built without Landlock does. That is the answer a
//! program's first call, the ABI-version probe, is written to test for
//! ("Landlock is not supported by the current kernel"), after which a
//! best-effort sandbox carries on without it.
//!
//! Until 2026-09-26 this module exported the three calls under their own
//! names, which glibc does not, and the version probe answered ABI 1 -- after
//! which every ruleset was refused with `ENOSYS`, so a program told Landlock
//! was there failed setting it up (`B-D-LANDLOCK-SAID-YES-THEN-NO`). As for
//! kernel AIO (design-decisions.md §1114), the names are gone.

// ---------------------------------------------------------------------------
// Landlock rule types
// ---------------------------------------------------------------------------

/// Path-beneath rule: restrict access under a directory hierarchy.
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
/// Network port rule: restrict binding/connecting to ports.
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

// ---------------------------------------------------------------------------
// Filesystem access rights (for LANDLOCK_RULE_PATH_BENEATH)
// ---------------------------------------------------------------------------

/// Execute a file.
pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
/// Open a file with write access.
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
/// Open a file with read access.
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
/// Open a directory or list its content.
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
/// Remove an empty directory or rename one.
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
/// Unlink (remove) a file.
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
/// Create a character device.
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
/// Create a directory.
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
/// Create a regular file.
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
/// Create a socket.
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
/// Create a named pipe (FIFO).
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
/// Create a block device.
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
/// Create a symbolic link.
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
/// Link or rename a file to a directory.
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
/// Truncate a file.
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;
/// Issue an IOCTL on a device file.
pub const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;

/// Bitmask of every defined filesystem access right.
pub const LANDLOCK_ACCESS_FS_ALL: u64 = LANDLOCK_ACCESS_FS_EXECUTE
    | LANDLOCK_ACCESS_FS_WRITE_FILE
    | LANDLOCK_ACCESS_FS_READ_FILE
    | LANDLOCK_ACCESS_FS_READ_DIR
    | LANDLOCK_ACCESS_FS_REMOVE_DIR
    | LANDLOCK_ACCESS_FS_REMOVE_FILE
    | LANDLOCK_ACCESS_FS_MAKE_CHAR
    | LANDLOCK_ACCESS_FS_MAKE_DIR
    | LANDLOCK_ACCESS_FS_MAKE_REG
    | LANDLOCK_ACCESS_FS_MAKE_SOCK
    | LANDLOCK_ACCESS_FS_MAKE_FIFO
    | LANDLOCK_ACCESS_FS_MAKE_BLOCK
    | LANDLOCK_ACCESS_FS_MAKE_SYM
    | LANDLOCK_ACCESS_FS_REFER
    | LANDLOCK_ACCESS_FS_TRUNCATE
    | LANDLOCK_ACCESS_FS_IOCTL_DEV;

// ---------------------------------------------------------------------------
// Network access rights (for LANDLOCK_RULE_NET_PORT)
// ---------------------------------------------------------------------------

/// Bind to a TCP port.
pub const LANDLOCK_ACCESS_NET_BIND_TCP: u64 = 1 << 0;
/// Connect to a TCP port.
pub const LANDLOCK_ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;

/// Bitmask of every defined network access right.
pub const LANDLOCK_ACCESS_NET_ALL: u64 =
    LANDLOCK_ACCESS_NET_BIND_TCP | LANDLOCK_ACCESS_NET_CONNECT_TCP;

// ---------------------------------------------------------------------------
// Landlock create flags
// ---------------------------------------------------------------------------

/// `landlock_create_ruleset`'s probe: return the ABI version instead of
/// creating a ruleset.
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

/// `landlock_create_ruleset`'s probe for the errata bitfield (Linux 6.10).
pub const LANDLOCK_CREATE_RULESET_ERRATA: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Ruleset attributes for `landlock_create_ruleset()`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LandlockRulesetAttr {
    /// Filesystem access rights to handle.
    pub handled_access_fs: u64,
    /// Network access rights to handle.
    pub handled_access_net: u64,
}

impl LandlockRulesetAttr {
    /// Create a zeroed ruleset attribute.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Path-beneath rule attribute.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LandlockPathBeneathAttr {
    /// Allowed access rights.
    pub allowed_access: u64,
    /// File descriptor of the directory.
    pub parent_fd: i32,
    /// Padding.
    _pad: i32,
}

impl LandlockPathBeneathAttr {
    /// Create a zeroed path-beneath attribute.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Network port rule attribute.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LandlockNetPortAttr {
    /// Allowed access rights.
    pub allowed_access: u64,
    /// Port number.
    pub port: u64,
}

impl LandlockNetPortAttr {
    /// Create a zeroed network port attribute.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errno;

    #[test]
    fn test_fs_access_rights_are_powers_of_two() {
        let rights = [
            LANDLOCK_ACCESS_FS_EXECUTE,
            LANDLOCK_ACCESS_FS_WRITE_FILE,
            LANDLOCK_ACCESS_FS_READ_FILE,
            LANDLOCK_ACCESS_FS_READ_DIR,
            LANDLOCK_ACCESS_FS_REMOVE_DIR,
            LANDLOCK_ACCESS_FS_REMOVE_FILE,
            LANDLOCK_ACCESS_FS_MAKE_CHAR,
            LANDLOCK_ACCESS_FS_MAKE_DIR,
            LANDLOCK_ACCESS_FS_MAKE_REG,
            LANDLOCK_ACCESS_FS_MAKE_SOCK,
            LANDLOCK_ACCESS_FS_MAKE_FIFO,
            LANDLOCK_ACCESS_FS_MAKE_BLOCK,
            LANDLOCK_ACCESS_FS_MAKE_SYM,
            LANDLOCK_ACCESS_FS_REFER,
            LANDLOCK_ACCESS_FS_TRUNCATE,
            LANDLOCK_ACCESS_FS_IOCTL_DEV,
        ];
        for r in &rights {
            assert!(r.is_power_of_two(), "right {r:#x} not power of 2");
        }
    }

    #[test]
    fn test_net_access_rights() {
        assert_eq!(LANDLOCK_ACCESS_NET_BIND_TCP, 1);
        assert_eq!(LANDLOCK_ACCESS_NET_CONNECT_TCP, 2);
        assert_ne!(
            LANDLOCK_ACCESS_NET_BIND_TCP,
            LANDLOCK_ACCESS_NET_CONNECT_TCP
        );
    }

    #[test]
    fn test_rule_types() {
        assert_eq!(LANDLOCK_RULE_PATH_BENEATH, 1);
        assert_eq!(LANDLOCK_RULE_NET_PORT, 2);
    }

    #[test]
    fn test_ruleset_attr_size() {
        assert_eq!(core::mem::size_of::<LandlockRulesetAttr>(), 16);
    }

    #[test]
    fn test_path_beneath_attr_size() {
        assert_eq!(core::mem::size_of::<LandlockPathBeneathAttr>(), 16);
    }

    #[test]
    fn test_net_port_attr_size() {
        assert_eq!(core::mem::size_of::<LandlockNetPortAttr>(), 16);
    }

    #[test]
    fn test_fs_all_includes_every_bit() {
        // Every defined FS access bit is in LANDLOCK_ACCESS_FS_ALL.
        assert!(LANDLOCK_ACCESS_FS_ALL & LANDLOCK_ACCESS_FS_TRUNCATE != 0);
        assert!(LANDLOCK_ACCESS_FS_ALL & LANDLOCK_ACCESS_FS_IOCTL_DEV != 0);
    }

    // -- ABI probe form ------------------------------------------------------

    #[test]
    fn syscall_answers_as_a_kernel_without_landlock() {
        // THE REGRESSION PIN: the version probe answered 1, and then every
        // ruleset was refused.  A kernel without Landlock answers ENOSYS to
        // all three calls, the probe first among them.
        const SYS_LANDLOCK_CREATE_RULESET: isize = 444;
        const SYS_LANDLOCK_ADD_RULE: isize = 445;
        const SYS_LANDLOCK_RESTRICT_SELF: isize = 446;
        for (nr, a1, a2, a3) in [
            (
                SYS_LANDLOCK_CREATE_RULESET,
                0,
                0,
                LANDLOCK_CREATE_RULESET_VERSION as isize,
            ),
            (
                SYS_LANDLOCK_ADD_RULE,
                3,
                LANDLOCK_RULE_PATH_BENEATH as isize,
                0,
            ),
            (SYS_LANDLOCK_RESTRICT_SELF, 3, 0, 0),
        ] {
            errno::set_errno(0);
            let r = crate::sys_syscall::syscall(nr, a1, a2, a3, 0, 0, 0);
            assert_eq!((r, errno::get_errno()), (-1, errno::ENOSYS), "syscall {nr}");
        }
    }
}

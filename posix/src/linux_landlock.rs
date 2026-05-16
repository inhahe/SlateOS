//! `<linux/landlock.h>` — unprivileged access-control (sandboxing).
//!
//! Landlock is a stackable Linux security module that allows
//! unprivileged processes to restrict their own access rights
//! (filesystem, network) without needing root or a security policy.
//! Available since Linux 5.13.

use crate::errno;

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
/// Create a regular file.
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
/// Set IOCTL on a file.
pub const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;

// ---------------------------------------------------------------------------
// Network access rights (for LANDLOCK_RULE_NET_PORT)
// ---------------------------------------------------------------------------

/// Bind to a TCP port.
pub const LANDLOCK_ACCESS_NET_BIND_TCP: u64 = 1 << 0;
/// Connect to a TCP port.
pub const LANDLOCK_ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Landlock create flags
// ---------------------------------------------------------------------------

/// Latest supported ABI version.
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

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
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Stubs
// ---------------------------------------------------------------------------

/// Create a Landlock ruleset.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn landlock_create_ruleset(
    _attr: *const LandlockRulesetAttr,
    _size: usize,
    _flags: u32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Add a rule to a Landlock ruleset.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn landlock_add_rule(
    _ruleset_fd: i32,
    _rule_type: u32,
    _rule_attr: *const u8,
    _flags: u32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Restrict the calling thread with a Landlock ruleset.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn landlock_restrict_self(_ruleset_fd: i32, _flags: u32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_ne!(LANDLOCK_ACCESS_NET_BIND_TCP, LANDLOCK_ACCESS_NET_CONNECT_TCP);
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
    fn test_create_ruleset_stub() {
        let ret = landlock_create_ruleset(core::ptr::null(), 0, LANDLOCK_CREATE_RULESET_VERSION);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_restrict_self_stub() {
        let ret = landlock_restrict_self(-1, 0);
        assert_eq!(ret, -1);
    }
}

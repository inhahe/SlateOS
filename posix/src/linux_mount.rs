//! `<linux/mount.h>` — new mount API (Linux 5.2+).
//!
//! The new mount API (`fsopen`, `fsconfig`, `fsmount`, `move_mount`,
//! `open_tree`) provides a more flexible alternative to the legacy
//! `mount(2)` syscall. Each step is explicit and composable.
//!
//! Re-exports legacy MS_* flags from `sys_mount`.

// ---------------------------------------------------------------------------
// Re-exports: legacy mount flags
// ---------------------------------------------------------------------------

pub use crate::sys_mount::MS_BIND;
pub use crate::sys_mount::MS_DIRSYNC;
pub use crate::sys_mount::MS_MANDLOCK;
pub use crate::sys_mount::MS_NOATIME;
pub use crate::sys_mount::MS_NODEV;
pub use crate::sys_mount::MS_NODIRATIME;
pub use crate::sys_mount::MS_NOEXEC;
pub use crate::sys_mount::MS_NOSUID;
pub use crate::sys_mount::MS_PRIVATE;
pub use crate::sys_mount::MS_RDONLY;
pub use crate::sys_mount::MS_REC;
pub use crate::sys_mount::MS_REMOUNT;
pub use crate::sys_mount::MS_SHARED;
pub use crate::sys_mount::MS_SILENT;
pub use crate::sys_mount::MS_SLAVE;
pub use crate::sys_mount::MS_SYNCHRONOUS;
pub use crate::sys_mount::MS_UNBINDABLE;

// ---------------------------------------------------------------------------
// MOUNT_ATTR_* flags (for mount_setattr)
// ---------------------------------------------------------------------------

/// Mount read-only.
pub const MOUNT_ATTR_RDONLY: u64 = 0x00000001;
/// Ignore suid/sgid bits.
pub const MOUNT_ATTR_NOSUID: u64 = 0x00000002;
/// Disallow device access.
pub const MOUNT_ATTR_NODEV: u64 = 0x00000004;
/// Disallow program execution.
pub const MOUNT_ATTR_NOEXEC: u64 = 0x00000008;
/// Setting atime requires CAP_SYS_ADMIN or owner.
pub const MOUNT_ATTR__ATIME: u64 = 0x00000070;
/// Update atime relative to mtime/ctime.
pub const MOUNT_ATTR_RELATIME: u64 = 0x00000000;
/// Do not update atime.
pub const MOUNT_ATTR_NOATIME: u64 = 0x00000010;
/// Only update atime on the directory.
pub const MOUNT_ATTR_STRICTATIME: u64 = 0x00000020;
/// Do not follow symlinks.
pub const MOUNT_ATTR_NOSYMFOLLOW: u64 = 0x00200000;
/// Use idmapped mount.
pub const MOUNT_ATTR_IDMAP: u64 = 0x00100000;

// ---------------------------------------------------------------------------
// fsopen() flags
// ---------------------------------------------------------------------------

/// Create filesystem context with cloexec.
pub const FSOPEN_CLOEXEC: u32 = 0x00000001;

// ---------------------------------------------------------------------------
// fsmount() flags
// ---------------------------------------------------------------------------

/// Close-on-exec for the resulting fd.
pub const FSMOUNT_CLOEXEC: u32 = 0x00000001;

// ---------------------------------------------------------------------------
// fsconfig() commands
// ---------------------------------------------------------------------------

/// Set a string parameter.
pub const FSCONFIG_SET_FLAG: u32 = 0;
/// Set a string-valued parameter.
pub const FSCONFIG_SET_STRING: u32 = 1;
/// Set a binary-blob parameter.
pub const FSCONFIG_SET_BINARY: u32 = 2;
/// Set a path parameter.
pub const FSCONFIG_SET_PATH: u32 = 3;
/// Set a path parameter relative to an empty path.
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
/// Set an fd parameter.
pub const FSCONFIG_SET_FD: u32 = 5;
/// Create/reconfigure the superblock.
pub const FSCONFIG_CMD_CREATE: u32 = 6;
/// Reconfigure the superblock.
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;

// ---------------------------------------------------------------------------
// move_mount() flags
// ---------------------------------------------------------------------------

/// Follow symlinks on "from" path.
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x00000001;
/// Auto-mount at "from" path.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x00000002;
/// Empty "from" path (use fd).
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x00000004;
/// Follow symlinks on "to" path.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x00000010;
/// Auto-mount at "to" path.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x00000020;
/// Empty "to" path (use fd).
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x00000040;
/// Set the expiry mark on a mount.
pub const MOVE_MOUNT_SET_GROUP: u32 = 0x00000100;
/// Beneath the "to" path.
pub const MOVE_MOUNT_BENEATH: u32 = 0x00000200;

// ---------------------------------------------------------------------------
// open_tree() flags
// ---------------------------------------------------------------------------

/// Open tree: clone the mount subtree.
pub const OPEN_TREE_CLONE: u32 = 1;
/// Open tree: close-on-exec.
pub const OPEN_TREE_CLOEXEC: u32 = 0x00080000;

// ---------------------------------------------------------------------------
// mount_setattr struct
// ---------------------------------------------------------------------------

/// Arguments for `mount_setattr(2)`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MountAttr {
    /// Mount attributes to set.
    pub attr_set: u64,
    /// Mount attributes to clear.
    pub attr_clr: u64,
    /// Mount propagation type.
    pub propagation: u64,
    /// User namespace fd (for ID-mapped mounts).
    pub userns_fd: u64,
}

impl MountAttr {
    /// Create a zeroed `MountAttr`.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// AT_RECURSIVE for mount_setattr
// ---------------------------------------------------------------------------

/// Apply recursively to the entire subtree.
pub const AT_RECURSIVE: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_attr_flags() {
        assert_eq!(MOUNT_ATTR_RDONLY, 0x01);
        assert_eq!(MOUNT_ATTR_NOSUID, 0x02);
        assert_eq!(MOUNT_ATTR_NODEV, 0x04);
        assert_eq!(MOUNT_ATTR_NOEXEC, 0x08);
    }

    #[test]
    fn test_mount_attr_distinct() {
        let attrs = [
            MOUNT_ATTR_RDONLY,
            MOUNT_ATTR_NOSUID,
            MOUNT_ATTR_NODEV,
            MOUNT_ATTR_NOEXEC,
            MOUNT_ATTR_NOATIME,
            MOUNT_ATTR_STRICTATIME,
            MOUNT_ATTR_NOSYMFOLLOW,
            MOUNT_ATTR_IDMAP,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_fsconfig_commands_sequential() {
        assert_eq!(FSCONFIG_SET_FLAG, 0);
        assert_eq!(FSCONFIG_SET_STRING, 1);
        assert_eq!(FSCONFIG_SET_BINARY, 2);
        assert_eq!(FSCONFIG_SET_PATH, 3);
        assert_eq!(FSCONFIG_SET_PATH_EMPTY, 4);
        assert_eq!(FSCONFIG_SET_FD, 5);
        assert_eq!(FSCONFIG_CMD_CREATE, 6);
        assert_eq!(FSCONFIG_CMD_RECONFIGURE, 7);
    }

    #[test]
    fn test_move_mount_flags() {
        // "from" and "to" flags should not overlap.
        assert_eq!(MOVE_MOUNT_F_SYMLINKS & MOVE_MOUNT_T_SYMLINKS, 0);
        assert_eq!(MOVE_MOUNT_F_EMPTY_PATH & MOVE_MOUNT_T_EMPTY_PATH, 0);
    }

    #[test]
    fn test_mount_attr_struct_size() {
        assert_eq!(core::mem::size_of::<MountAttr>(), 32);
    }

    #[test]
    fn test_mount_attr_zeroed() {
        let attr = MountAttr::zeroed();
        assert_eq!(attr.attr_set, 0);
        assert_eq!(attr.attr_clr, 0);
        assert_eq!(attr.propagation, 0);
        assert_eq!(attr.userns_fd, 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(MS_RDONLY, crate::sys_mount::MS_RDONLY);
        assert_eq!(MS_NOSUID, crate::sys_mount::MS_NOSUID);
        assert_eq!(MS_BIND, crate::sys_mount::MS_BIND);
    }
}

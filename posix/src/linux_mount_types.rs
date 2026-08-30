//! `<linux/mount.h>` — New mount API (fsopen/fsmount/move_mount) constants.
//!
//! Linux 5.2+ introduced a new filesystem mounting API that splits
//! the mount operation into discrete steps: fsopen() creates an FS
//! context, fsconfig() configures it, fsmount() creates a mount from
//! the context, and move_mount() attaches it to the namespace tree.
//! This replaces the monolithic mount() syscall for new use cases.

// ---------------------------------------------------------------------------
// move_mount flags
// ---------------------------------------------------------------------------

/// Move mount from fd (source is a mount fd).
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x0000_0001;
/// Automount at source.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x0000_0002;
/// Empty path at source.
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x0000_0004;
/// Follow symlinks at target.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x0000_0010;
/// Automount at target.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x0000_0020;
/// Empty path at target.
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x0000_0040;
/// Set connectable (for NFS export).
pub const MOVE_MOUNT_SET_GROUP: u32 = 0x0000_0100;
/// Beneath target (mount under existing mount).
pub const MOVE_MOUNT_BENEATH: u32 = 0x0000_0200;

// ---------------------------------------------------------------------------
// open_tree flags
// ---------------------------------------------------------------------------

/// Open a detached clone of the mount tree.
pub const OPEN_TREE_CLONE: u32 = 1;
/// Clone the tree detached (combinable with AT_RECURSIVE).
pub const OPEN_TREE_CLOEXEC: u32 = 0x0008_0000;

// ---------------------------------------------------------------------------
// fsmount flags
// ---------------------------------------------------------------------------

/// Set close-on-exec for the mount fd.
pub const FSMOUNT_CLOEXEC: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// fsmount attr_flags (mount attributes)
// ---------------------------------------------------------------------------

/// Read-only mount.
pub const MOUNT_ATTR_RDONLY: u32 = 0x0000_0001;
/// No setuid/setgid bits.
pub const MOUNT_ATTR_NOSUID: u32 = 0x0000_0002;
/// No device nodes.
pub const MOUNT_ATTR_NODEV: u32 = 0x0000_0004;
/// No executables.
pub const MOUNT_ATTR_NOEXEC: u32 = 0x0000_0008;
/// No access time updates.
pub const MOUNT_ATTR_NOATIME: u32 = 0x0000_0010;
/// Strict access time updates.
pub const MOUNT_ATTR_STRICTATIME: u32 = 0x0000_0020;
/// Directory-level no-dev.
pub const MOUNT_ATTR_NODIRATIME: u32 = 0x0000_0080;
/// ID-mapped mount.
pub const MOUNT_ATTR_IDMAP: u32 = 0x0010_0000;
/// Disable symlink traversal.
pub const MOUNT_ATTR_NOSYMFOLLOW: u32 = 0x0020_0000;

// ---------------------------------------------------------------------------
// fsconfig commands
// ---------------------------------------------------------------------------

/// Set a flag (boolean) parameter.
pub const FSCONFIG_SET_FLAG: u32 = 0;
/// Set a string parameter.
pub const FSCONFIG_SET_STRING: u32 = 1;
/// Set a binary blob parameter.
pub const FSCONFIG_SET_BINARY: u32 = 2;
/// Set a path parameter.
pub const FSCONFIG_SET_PATH: u32 = 3;
/// Set a path (empty path = fd).
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
/// Set an fd parameter.
pub const FSCONFIG_SET_FD: u32 = 5;
/// Create the superblock.
pub const FSCONFIG_CMD_CREATE: u32 = 6;
/// Reconfigure the superblock.
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;
/// Create exclusive (error if FS already active).
pub const FSCONFIG_CMD_CREATE_EXCL: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_mount_f_flags_no_overlap() {
        let flags = [
            MOVE_MOUNT_F_SYMLINKS,
            MOVE_MOUNT_F_AUTOMOUNTS,
            MOVE_MOUNT_F_EMPTY_PATH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_move_mount_t_flags_no_overlap() {
        let flags = [
            MOVE_MOUNT_T_SYMLINKS,
            MOVE_MOUNT_T_AUTOMOUNTS,
            MOVE_MOUNT_T_EMPTY_PATH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mount_attr_no_overlap() {
        let attrs = [
            MOUNT_ATTR_RDONLY,
            MOUNT_ATTR_NOSUID,
            MOUNT_ATTR_NODEV,
            MOUNT_ATTR_NOEXEC,
            MOUNT_ATTR_NOATIME,
            MOUNT_ATTR_STRICTATIME,
            MOUNT_ATTR_NODIRATIME,
            MOUNT_ATTR_IDMAP,
            MOUNT_ATTR_NOSYMFOLLOW,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_fsconfig_cmds_distinct() {
        let cmds = [
            FSCONFIG_SET_FLAG,
            FSCONFIG_SET_STRING,
            FSCONFIG_SET_BINARY,
            FSCONFIG_SET_PATH,
            FSCONFIG_SET_PATH_EMPTY,
            FSCONFIG_SET_FD,
            FSCONFIG_CMD_CREATE,
            FSCONFIG_CMD_RECONFIGURE,
            FSCONFIG_CMD_CREATE_EXCL,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_open_tree_flags() {
        assert_eq!(OPEN_TREE_CLONE, 1);
        assert_ne!(OPEN_TREE_CLONE, OPEN_TREE_CLOEXEC);
    }
}

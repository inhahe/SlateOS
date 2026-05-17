//! `<linux/mount.h>` — New mount API (fsopen/fsmount/move_mount) constants.
//!
//! Linux 5.2+ introduced a new mount API replacing the monolithic mount()
//! syscall. It decomposes mounting into steps: fsopen (create FS context),
//! fsconfig (configure), fsmount (create mount), move_mount (attach).
//! This gives finer control over error handling and security.

// ---------------------------------------------------------------------------
// fsopen flags
// ---------------------------------------------------------------------------

/// Create filesystem context for fsopen().
pub const FSOPEN_CLOEXEC: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// fspick flags
// ---------------------------------------------------------------------------

/// Pick by path (default).
pub const FSPICK_CLOEXEC: u32 = 0x0000_0001;
/// Pick symlink itself, not target.
pub const FSPICK_SYMLINK_NOFOLLOW: u32 = 0x0000_0002;
/// Don't cross mount points.
pub const FSPICK_NO_AUTOMOUNT: u32 = 0x0000_0004;
/// Open empty path (AT_EMPTY_PATH equivalent).
pub const FSPICK_EMPTY_PATH: u32 = 0x0000_0008;

// ---------------------------------------------------------------------------
// fsconfig commands
// ---------------------------------------------------------------------------

/// Set parameter (key=value string).
pub const FSCONFIG_SET_STRING: u32 = 1;
/// Set binary parameter.
pub const FSCONFIG_SET_BINARY: u32 = 2;
/// Set path parameter.
pub const FSCONFIG_SET_PATH: u32 = 3;
/// Set path-beneath parameter.
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
/// Set fd parameter.
pub const FSCONFIG_SET_FD: u32 = 5;
/// Set flag (boolean, key only).
pub const FSCONFIG_SET_FLAG: u32 = 0;
/// Create superblock.
pub const FSCONFIG_CMD_CREATE: u32 = 6;
/// Reconfigure superblock.
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;

// ---------------------------------------------------------------------------
// fsmount flags
// ---------------------------------------------------------------------------

/// Close-on-exec for the mount fd.
pub const FSMOUNT_CLOEXEC: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// fsmount attr_flags (mount attributes)
// ---------------------------------------------------------------------------

/// Mount read-only.
pub const MOUNT_ATTR_RDONLY: u64 = 0x0000_0001;
/// Disallow setuid.
pub const MOUNT_ATTR_NOSUID: u64 = 0x0000_0002;
/// No device access.
pub const MOUNT_ATTR_NODEV: u64 = 0x0000_0004;
/// No execution.
pub const MOUNT_ATTR_NOEXEC: u64 = 0x0000_0008;
/// No atime updates.
pub const MOUNT_ATTR_NOATIME: u64 = 0x0000_0010;
/// Strict atime.
pub const MOUNT_ATTR_STRICTATIME: u64 = 0x0000_0020;
/// Directory modifications are synchronous.
pub const MOUNT_ATTR_NODIRATIME: u64 = 0x0000_0080;
/// ID-mapped mount.
pub const MOUNT_ATTR_IDMAP: u64 = 0x0010_0000;
/// Disallow user-namespace mounts beneath.
pub const MOUNT_ATTR_NOSYMFOLLOW: u64 = 0x0020_0000;

// ---------------------------------------------------------------------------
// move_mount flags
// ---------------------------------------------------------------------------

/// Move from an open fd (AT_FDCWD-relative source).
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x0000_0001;
/// Follow symlinks in from-path.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x0000_0002;
/// Empty from-path (use fd directly).
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x0000_0004;
/// Follow symlinks in to-path.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x0000_0010;
/// Trigger automounts in to-path.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x0000_0020;
/// Empty to-path (use fd directly).
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x0000_0040;
/// Set move as beneath target.
pub const MOVE_MOUNT_SET_GROUP: u32 = 0x0000_0100;
/// Beneath existing mount at target.
pub const MOVE_MOUNT_BENEATH: u32 = 0x0000_0200;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fspick_flags_no_overlap() {
        let flags = [
            FSPICK_CLOEXEC, FSPICK_SYMLINK_NOFOLLOW,
            FSPICK_NO_AUTOMOUNT, FSPICK_EMPTY_PATH,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fsconfig_commands_distinct() {
        let cmds = [
            FSCONFIG_SET_FLAG, FSCONFIG_SET_STRING, FSCONFIG_SET_BINARY,
            FSCONFIG_SET_PATH, FSCONFIG_SET_PATH_EMPTY, FSCONFIG_SET_FD,
            FSCONFIG_CMD_CREATE, FSCONFIG_CMD_RECONFIGURE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_mount_attr_no_overlap() {
        let attrs = [
            MOUNT_ATTR_RDONLY, MOUNT_ATTR_NOSUID, MOUNT_ATTR_NODEV,
            MOUNT_ATTR_NOEXEC, MOUNT_ATTR_NOATIME, MOUNT_ATTR_STRICTATIME,
            MOUNT_ATTR_NODIRATIME, MOUNT_ATTR_IDMAP, MOUNT_ATTR_NOSYMFOLLOW,
        ];
        for i in 0..attrs.len() {
            assert!(attrs[i].is_power_of_two());
            for j in (i + 1)..attrs.len() {
                assert_eq!(attrs[i] & attrs[j], 0);
            }
        }
    }

    #[test]
    fn test_move_mount_flags_distinct() {
        let flags = [
            MOVE_MOUNT_F_SYMLINKS, MOVE_MOUNT_F_AUTOMOUNTS,
            MOVE_MOUNT_F_EMPTY_PATH, MOVE_MOUNT_T_SYMLINKS,
            MOVE_MOUNT_T_AUTOMOUNTS, MOVE_MOUNT_T_EMPTY_PATH,
            MOVE_MOUNT_SET_GROUP, MOVE_MOUNT_BENEATH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}

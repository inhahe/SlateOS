//! `<linux/mount.h>` — Additional mount constants (part 3).
//!
//! Supplementary mount constants covering mount_setattr flags,
//! open_tree flags, and move_mount flags.

// ---------------------------------------------------------------------------
// mount_setattr flags (MOUNT_ATTR_*)
// ---------------------------------------------------------------------------

/// Read-only mount.
pub const MOUNT_ATTR_RDONLY: u64 = 0x00000001;
/// No suid.
pub const MOUNT_ATTR_NOSUID: u64 = 0x00000002;
/// No device access.
pub const MOUNT_ATTR_NODEV: u64 = 0x00000004;
/// No exec.
pub const MOUNT_ATTR_NOEXEC: u64 = 0x00000008;
/// Relatime.
pub const MOUNT_ATTR__ATIME: u64 = 0x00000070;
/// Relatime.
pub const MOUNT_ATTR_RELATIME: u64 = 0x00000000;
/// No atime.
pub const MOUNT_ATTR_NOATIME: u64 = 0x00000010;
/// Strict atime.
pub const MOUNT_ATTR_STRICTATIME: u64 = 0x00000020;
/// No diratime.
pub const MOUNT_ATTR_NODIRATIME: u64 = 0x00000080;
/// ID mapped mount.
pub const MOUNT_ATTR_IDMAP: u64 = 0x00100000;
/// No symlinks.
pub const MOUNT_ATTR_NOSYMFOLLOW: u64 = 0x00200000;

// ---------------------------------------------------------------------------
// open_tree flags
// ---------------------------------------------------------------------------

/// Open tree flag: clone.
pub const OPEN_TREE_CLONE: u32 = 1;
/// Open tree flag: cloexec.
pub const OPEN_TREE_CLOEXEC: u32 = 0o02000000;

// ---------------------------------------------------------------------------
// move_mount flags
// ---------------------------------------------------------------------------

/// From fd (source is fd).
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x00000001;
/// From auto-mount.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x00000002;
/// To fd empty path.
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x00000004;
/// To empty path.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x00000010;
/// To automounts.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x00000020;
/// To empty path.
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x00000040;
/// Set group.
pub const MOVE_MOUNT_SET_GROUP: u32 = 0x00000100;
/// Beneath.
pub const MOVE_MOUNT_BENEATH: u32 = 0x00000200;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_attrs_distinct() {
        let attrs = [
            MOUNT_ATTR_RDONLY, MOUNT_ATTR_NOSUID,
            MOUNT_ATTR_NODEV, MOUNT_ATTR_NOEXEC,
            MOUNT_ATTR_NOATIME, MOUNT_ATTR_STRICTATIME,
            MOUNT_ATTR_NODIRATIME, MOUNT_ATTR_IDMAP,
            MOUNT_ATTR_NOSYMFOLLOW,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_move_mount_from_flags_no_overlap() {
        let flags = [
            MOVE_MOUNT_F_SYMLINKS, MOVE_MOUNT_F_AUTOMOUNTS,
            MOVE_MOUNT_F_EMPTY_PATH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_move_mount_to_flags_no_overlap() {
        let flags = [
            MOVE_MOUNT_T_SYMLINKS, MOVE_MOUNT_T_AUTOMOUNTS,
            MOVE_MOUNT_T_EMPTY_PATH,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_move_mount_set_group() {
        assert!(MOVE_MOUNT_SET_GROUP.is_power_of_two());
    }
}

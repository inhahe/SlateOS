//! `<linux/mount.h>` — new mount API (`fsopen`/`fsconfig`/`fsmount`).
//!
//! Modern container runtimes (systemd-nspawn, runc) and mountpoint
//! managers prefer the new mount API over the legacy `mount(2)` —
//! it splits configuration into fsopen → fsconfig keys → fsmount and
//! moves namespaces with move_mount(2). The flag namespaces below
//! cover the user-visible part.

// ---------------------------------------------------------------------------
// `fsopen` flags
// ---------------------------------------------------------------------------

/// New mount fd should be CLOEXEC.
pub const FSOPEN_CLOEXEC: u32 = 0x00000001;

// ---------------------------------------------------------------------------
// `fsmount` flags and attributes
// ---------------------------------------------------------------------------

/// `O_CLOEXEC` on returned mount fd.
pub const FSMOUNT_CLOEXEC: u32 = 0x00000001;

/// Mount attribute: read-only.
pub const MOUNT_ATTR_RDONLY: u32 = 0x0000_0001;
/// Mount attribute: nosuid.
pub const MOUNT_ATTR_NOSUID: u32 = 0x0000_0002;
/// Mount attribute: nodev.
pub const MOUNT_ATTR_NODEV: u32 = 0x0000_0004;
/// Mount attribute: noexec.
pub const MOUNT_ATTR_NOEXEC: u32 = 0x0000_0008;
/// Mount attribute mask: atime semantics.
pub const MOUNT_ATTR__ATIME: u32 = 0x0000_0070;
/// relatime semantics.
pub const MOUNT_ATTR_RELATIME: u32 = 0x0000_0000;
/// noatime.
pub const MOUNT_ATTR_NOATIME: u32 = 0x0000_0010;
/// strictatime.
pub const MOUNT_ATTR_STRICTATIME: u32 = 0x0000_0020;
/// nodiratime.
pub const MOUNT_ATTR_NODIRATIME: u32 = 0x0000_0080;
/// Mount attribute: idmapped mount.
pub const MOUNT_ATTR_IDMAP: u32 = 0x0010_0000;
/// Mount attribute: nosymfollow.
pub const MOUNT_ATTR_NOSYMFOLLOW: u32 = 0x0020_0000;

// ---------------------------------------------------------------------------
// `move_mount` flags
// ---------------------------------------------------------------------------

/// Source path is interpreted relative to fd.
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x0000_0001;
/// Source: AT_EMPTY_PATH.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x0000_0002;
/// Empty source path.
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x0000_0004;
/// Dest path is symlink-following.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x0000_0010;
/// Dest auto-mounts followed.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x0000_0020;
/// Dest empty path.
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x0000_0040;
/// Mask of all valid move_mount flags.
pub const MOVE_MOUNT__MASK: u32 = 0x0000_0077;

// ---------------------------------------------------------------------------
// `fsconfig` commands
// ---------------------------------------------------------------------------

/// Set a string parameter.
pub const FSCONFIG_SET_FLAG: u32 = 0;
/// Set a key=value string.
pub const FSCONFIG_SET_STRING: u32 = 1;
/// Set a binary blob.
pub const FSCONFIG_SET_BINARY: u32 = 2;
/// Set a path string.
pub const FSCONFIG_SET_PATH: u32 = 3;
/// Set an empty path.
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
/// Set a file descriptor.
pub const FSCONFIG_SET_FD: u32 = 5;
/// Create the superblock.
pub const FSCONFIG_CMD_CREATE: u32 = 6;
/// Reconfigure (remount) an existing superblock.
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;

// ---------------------------------------------------------------------------
// `open_tree` flags
// ---------------------------------------------------------------------------

/// Clone the mount tree.
pub const OPEN_TREE_CLONE: u32 = 1;
/// Make the new mount unmountable until accepted.
pub const OPEN_TREE_CLOEXEC: u32 = 0o2000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fsopen_fsmount_flags() {
        assert_eq!(FSOPEN_CLOEXEC, 1);
        assert_eq!(FSMOUNT_CLOEXEC, 1);
    }

    #[test]
    fn test_mount_attr_bits_distinct() {
        let a = [
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
        for &b in &a {
            assert!(b.is_power_of_two());
        }
        for i in 0..a.len() {
            for j in (i + 1)..a.len() {
                assert_ne!(a[i], a[j]);
            }
        }
        // Atime mask must include NOATIME and STRICTATIME (0x10|0x20|0x40).
        assert_eq!(MOUNT_ATTR__ATIME, 0x70);
        // RELATIME is the default (zero) within the atime mask.
        assert_eq!(MOUNT_ATTR_RELATIME, 0);
    }

    #[test]
    fn test_move_mount_flags_distinct() {
        let m = [
            MOVE_MOUNT_F_SYMLINKS,
            MOVE_MOUNT_F_AUTOMOUNTS,
            MOVE_MOUNT_F_EMPTY_PATH,
            MOVE_MOUNT_T_SYMLINKS,
            MOVE_MOUNT_T_AUTOMOUNTS,
            MOVE_MOUNT_T_EMPTY_PATH,
        ];
        for &b in &m {
            assert!(b.is_power_of_two());
        }
        let or = m.iter().fold(0u32, |x, &y| x | y);
        // Mask must include all defined bits.
        assert_eq!(MOVE_MOUNT__MASK & or, or);
    }

    #[test]
    fn test_fsconfig_commands_dense() {
        let c = [
            FSCONFIG_SET_FLAG,
            FSCONFIG_SET_STRING,
            FSCONFIG_SET_BINARY,
            FSCONFIG_SET_PATH,
            FSCONFIG_SET_PATH_EMPTY,
            FSCONFIG_SET_FD,
            FSCONFIG_CMD_CREATE,
            FSCONFIG_CMD_RECONFIGURE,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_open_tree_flags() {
        assert_eq!(OPEN_TREE_CLONE, 1);
        // OPEN_TREE_CLOEXEC overlays O_CLOEXEC's value (0o2000000).
        assert_eq!(OPEN_TREE_CLOEXEC, 0o2000000);
        assert_ne!(OPEN_TREE_CLONE, OPEN_TREE_CLOEXEC);
    }
}

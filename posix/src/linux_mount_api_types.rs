//! `<linux/mount.h>` (new API subset) — New mount API constants.
//!
//! The new mount API (Linux 5.2+) replaces the traditional mount()
//! syscall with a multi-step process: fsopen() creates a context,
//! fsconfig() sets options, fsmount() creates the mount, and
//! move_mount() attaches it to the namespace. This provides better
//! error handling, atomicity, and the ability to configure mounts
//! before they become visible. Works with mount_setattr() for
//! changing mount properties after creation.

// ---------------------------------------------------------------------------
// fsmount() flags
// ---------------------------------------------------------------------------

/// Create mount with close-on-exec on the mount fd.
pub const FSMOUNT_CLOEXEC: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// move_mount() flags
// ---------------------------------------------------------------------------

/// Source is a mount fd from fsmount (not a path).
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x0000_0001;
/// Follow symlinks in source path.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x0000_0002;
/// Don't trigger automounts in source path.
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x0000_0004;
/// Follow symlinks in destination path.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x0000_0010;
/// Allow automounts in destination path.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x0000_0020;
/// Use empty path for destination (use fd directly).
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x0000_0040;
/// Set attr on the mount (combined with mount_setattr).
pub const MOVE_MOUNT_SET_GROUP: u32 = 0x0000_0100;
/// Beneath: mount beneath an existing mount.
pub const MOVE_MOUNT_BENEATH: u32 = 0x0000_0200;

// ---------------------------------------------------------------------------
// mount_setattr() attribute flags
// ---------------------------------------------------------------------------

/// Make mount read-only.
pub const MOUNT_ATTR_RDONLY: u32 = 0x0000_0001;
/// Disallow setuid/setgid bits.
pub const MOUNT_ATTR_NOSUID: u32 = 0x0000_0002;
/// Disallow device special files.
pub const MOUNT_ATTR_NODEV: u32 = 0x0000_0004;
/// Disallow program execution.
pub const MOUNT_ATTR_NOEXEC: u32 = 0x0000_0008;
/// Update atime relative to mtime/ctime.
pub const MOUNT_ATTR_RELATIME: u32 = 0x0000_0000;
/// Never update atime.
pub const MOUNT_ATTR_NOATIME: u32 = 0x0000_0010;
/// Always update atime (strict POSIX).
pub const MOUNT_ATTR_STRICTATIME: u32 = 0x0000_0020;
/// Don't update directory atime.
pub const MOUNT_ATTR_NODIRATIME: u32 = 0x0000_0080;
/// Mount is idmapped (user namespace mapping).
pub const MOUNT_ATTR_IDMAP: u32 = 0x0010_0000;
/// Don't allow suid/sgid through idmapping.
pub const MOUNT_ATTR_NOSYMFOLLOW: u32 = 0x0020_0000;

// ---------------------------------------------------------------------------
// mount_setattr() propagation flags
// ---------------------------------------------------------------------------

/// No propagation.
pub const MS_PRIVATE: u32 = 1 << 18;
/// Shared propagation.
pub const MS_SHARED: u32 = 1 << 20;
/// Slave propagation.
pub const MS_SLAVE: u32 = 1 << 19;
/// Unbindable mount.
pub const MS_UNBINDABLE: u32 = 1 << 17;

// ---------------------------------------------------------------------------
// open_tree() flags
// ---------------------------------------------------------------------------

/// Clone the mount tree.
pub const OPEN_TREE_CLONE: u32 = 1;
/// Close-on-exec for the returned fd.
pub const OPEN_TREE_CLOEXEC: u32 = 0x0008_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_mount_f_flags_no_overlap() {
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
    fn test_move_mount_t_flags_no_overlap() {
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
    fn test_mount_attr_flags_distinct() {
        // Excluding RELATIME which is 0
        let flags = [
            MOUNT_ATTR_RDONLY, MOUNT_ATTR_NOSUID, MOUNT_ATTR_NODEV,
            MOUNT_ATTR_NOEXEC, MOUNT_ATTR_NOATIME,
            MOUNT_ATTR_STRICTATIME, MOUNT_ATTR_NODIRATIME,
            MOUNT_ATTR_IDMAP, MOUNT_ATTR_NOSYMFOLLOW,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_propagation_flags_no_overlap() {
        let flags = [MS_PRIVATE, MS_SHARED, MS_SLAVE, MS_UNBINDABLE];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_relatime_is_default() {
        assert_eq!(MOUNT_ATTR_RELATIME, 0);
    }
}

//! `<linux/mount.h>` — Mount attribute and propagation constants.
//!
//! The mount_setattr()/fsconfig() API uses attribute flags to control
//! mount behavior (read-only, noexec, nosuid, etc.) and propagation
//! type (shared, private, slave). These replace the older MS_* flags
//! with a cleaner, extensible interface.

// ---------------------------------------------------------------------------
// Mount attribute flags (mount_setattr / mount_attr.attr_set/attr_clr)
// ---------------------------------------------------------------------------

/// Mount read-only.
pub const MOUNT_ATTR_RDONLY: u64 = 0x0000_0001;
/// Ignore setuid/setgid bits.
pub const MOUNT_ATTR_NOSUID: u64 = 0x0000_0002;
/// Disallow device access.
pub const MOUNT_ATTR_NODEV: u64 = 0x0000_0004;
/// Disallow program execution.
pub const MOUNT_ATTR_NOEXEC: u64 = 0x0000_0008;
/// Update atime relative to mtime/ctime.
pub const MOUNT_ATTR_RELATIME: u64 = 0x0000_0000;
/// Do not update access times.
pub const MOUNT_ATTR_NOATIME: u64 = 0x0000_0010;
/// Always update atime (strict POSIX).
pub const MOUNT_ATTR_STRICTATIME: u64 = 0x0000_0020;
/// Directory atime only updated when contents read.
pub const MOUNT_ATTR_NODIRATIME: u64 = 0x0000_0080;
/// Map user/group IDs (idmapped mount).
pub const MOUNT_ATTR_IDMAP: u64 = 0x0010_0000;
/// Forbid symlink traversal.
pub const MOUNT_ATTR_NOSYMFOLLOW: u64 = 0x0020_0000;

// ---------------------------------------------------------------------------
// Mount propagation types
// ---------------------------------------------------------------------------

/// Shared mount (events propagate bidirectionally to peers).
pub const MS_SHARED: u32 = 1 << 20;
/// Private mount (no propagation).
pub const MS_PRIVATE: u32 = 1 << 18;
/// Slave mount (receives from master, doesn't propagate).
pub const MS_SLAVE: u32 = 1 << 19;
/// Unbindable mount (cannot be bind-mounted).
pub const MS_UNBINDABLE: u32 = 1 << 17;

// ---------------------------------------------------------------------------
// mount_setattr() flags (passed in `flags` argument)
// ---------------------------------------------------------------------------

/// Recursively apply to all mounts in the tree.
pub const AT_RECURSIVE: u32 = 0x8000;

// ---------------------------------------------------------------------------
// open_tree() / move_mount() flags
// ---------------------------------------------------------------------------

/// Create a clone of the mount for open_tree().
pub const OPEN_TREE_CLONE: u32 = 1;
/// Close-on-exec for open_tree file descriptor.
pub const OPEN_TREE_CLOEXEC: u32 = 0x0008_0000;

/// Move mount to target.
pub const MOVE_MOUNT_F_SYMLINKS: u32 = 0x0000_0001;
/// Automounts are permitted.
pub const MOVE_MOUNT_F_AUTOMOUNTS: u32 = 0x0000_0002;
/// Empty path (use fd).
pub const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x0000_0004;
/// Target is beneath connector.
pub const MOVE_MOUNT_T_SYMLINKS: u32 = 0x0000_0010;
/// Target automounts permitted.
pub const MOVE_MOUNT_T_AUTOMOUNTS: u32 = 0x0000_0020;
/// Target empty path.
pub const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x0000_0040;
/// Set group.
pub const MOVE_MOUNT_SET_GROUP: u32 = 0x0000_0100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_attrs_distinct() {
        let attrs = [
            MOUNT_ATTR_RDONLY, MOUNT_ATTR_NOSUID, MOUNT_ATTR_NODEV,
            MOUNT_ATTR_NOEXEC, MOUNT_ATTR_NOATIME, MOUNT_ATTR_STRICTATIME,
            MOUNT_ATTR_NODIRATIME, MOUNT_ATTR_IDMAP, MOUNT_ATTR_NOSYMFOLLOW,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_propagation_types_no_overlap() {
        let props = [MS_SHARED, MS_PRIVATE, MS_SLAVE, MS_UNBINDABLE];
        for i in 0..props.len() {
            assert!(props[i].is_power_of_two());
            for j in (i + 1)..props.len() {
                assert_eq!(props[i] & props[j], 0);
            }
        }
    }

    #[test]
    fn test_move_mount_f_t_distinct() {
        assert_ne!(MOVE_MOUNT_F_SYMLINKS, MOVE_MOUNT_T_SYMLINKS);
        assert_ne!(MOVE_MOUNT_F_AUTOMOUNTS, MOVE_MOUNT_T_AUTOMOUNTS);
        assert_ne!(MOVE_MOUNT_F_EMPTY_PATH, MOVE_MOUNT_T_EMPTY_PATH);
    }
}

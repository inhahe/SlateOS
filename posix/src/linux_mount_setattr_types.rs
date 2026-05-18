//! `<linux/mount.h>` — mount_setattr() syscall constants.
//!
//! mount_setattr() changes mount attributes on an existing
//! mount.  These constants define attribute flags, atime
//! propagation types, and ID mapping parameters.

// ---------------------------------------------------------------------------
// mount_setattr() flags (AT_*)
// ---------------------------------------------------------------------------

/// Recursive (apply to subtree).
pub const AT_RECURSIVE: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Mount attributes (MOUNT_ATTR_*)
// ---------------------------------------------------------------------------

/// Read only.
pub const MOUNT_SETATTR_RDONLY: u64 = 0x00000001;
/// No setuid.
pub const MOUNT_SETATTR_NOSUID: u64 = 0x00000002;
/// No device access.
pub const MOUNT_SETATTR_NODEV: u64 = 0x00000004;
/// No exec.
pub const MOUNT_SETATTR_NOEXEC: u64 = 0x00000008;
/// No atime updates.
pub const MOUNT_SETATTR_NOATIME: u64 = 0x00000010;
/// Strict atime.
pub const MOUNT_SETATTR_STRICTATIME: u64 = 0x00000020;
/// No directory atime.
pub const MOUNT_SETATTR_NODIRATIME: u64 = 0x00000080;
/// ID-mapped mount.
pub const MOUNT_SETATTR_IDMAP: u64 = 0x00100000;
/// No symlink follow.
pub const MOUNT_SETATTR_NOSYMFOLLOW: u64 = 0x00200000;

// ---------------------------------------------------------------------------
// Atime mode mask
// ---------------------------------------------------------------------------

/// Atime setting mask (bits that affect atime behavior).
pub const MOUNT_SETATTR__ATIME: u64 = 0x00000070;

// ---------------------------------------------------------------------------
// mount_setattr struct sizes
// ---------------------------------------------------------------------------

/// Size of mount_attr v0.
pub const MOUNT_ATTR_SIZE_VER0: u32 = 32;

// ---------------------------------------------------------------------------
// userns_fd special values
// ---------------------------------------------------------------------------

/// No ID mapping.
pub const MOUNT_SETATTR_NO_IDMAP: u64 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            MOUNT_SETATTR_RDONLY, MOUNT_SETATTR_NOSUID,
            MOUNT_SETATTR_NODEV, MOUNT_SETATTR_NOEXEC,
            MOUNT_SETATTR_NOATIME, MOUNT_SETATTR_STRICTATIME,
            MOUNT_SETATTR_NODIRATIME, MOUNT_SETATTR_IDMAP,
            MOUNT_SETATTR_NOSYMFOLLOW,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_recursive_flag() {
        assert_eq!(AT_RECURSIVE, 0x8000);
    }

    #[test]
    fn test_atime_mask() {
        assert_eq!(MOUNT_SETATTR__ATIME, 0x70);
    }

    #[test]
    fn test_no_idmap_is_zero() {
        assert_eq!(MOUNT_SETATTR_NO_IDMAP, 0);
    }

    #[test]
    fn test_attr_size() {
        assert_eq!(MOUNT_ATTR_SIZE_VER0, 32);
    }

    #[test]
    fn test_noatime_in_mask() {
        assert_ne!(MOUNT_SETATTR_NOATIME & MOUNT_SETATTR__ATIME, 0);
    }
}

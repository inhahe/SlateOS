//! `<linux/mount.h>` — statmount/listmount syscall constants.
//!
//! statmount() and listmount() are modern mount query
//! interfaces.  These constants define request mask bits,
//! propagation types, and mount attribute flags.

// ---------------------------------------------------------------------------
// statmount() request mask (STATMOUNT_*)
// ---------------------------------------------------------------------------

/// Mount ID.
pub const STATMOUNT_SB_BASIC: u64 = 1 << 0;
/// Mount point path.
pub const STATMOUNT_MNT_BASIC: u64 = 1 << 1;
/// Propagation info.
pub const STATMOUNT_PROPAGATE_FROM: u64 = 1 << 2;
/// Mount point string.
pub const STATMOUNT_MNT_POINT: u64 = 1 << 4;
/// FS root string.
pub const STATMOUNT_MNT_ROOT: u64 = 1 << 3;
/// Filesystem type string.
pub const STATMOUNT_FS_TYPE: u64 = 1 << 5;
/// Mount source string.
pub const STATMOUNT_MNT_NS_ID: u64 = 1 << 6;
/// Mount options string.
pub const STATMOUNT_MNT_OPTS: u64 = 1 << 7;
/// Superblock source string.
pub const STATMOUNT_FS_SUBTYPE: u64 = 1 << 8;
/// Superblock source.
pub const STATMOUNT_SB_SOURCE: u64 = 1 << 9;

// ---------------------------------------------------------------------------
// Mount propagation types
// ---------------------------------------------------------------------------

/// Shared propagation.
pub const MS_SHARED: u32 = 1 << 20;
/// Slave propagation.
pub const MS_SLAVE: u32 = 1 << 19;
/// Private propagation (no propagation).
pub const MS_PRIVATE: u32 = 1 << 18;
/// Unbindable.
pub const MS_UNBINDABLE: u32 = 1 << 17;

// ---------------------------------------------------------------------------
// Mount attribute flags (MOUNT_ATTR_*)
// ---------------------------------------------------------------------------

/// Read-only mount.
pub const MOUNT_ATTR_RDONLY: u64 = 0x00000001;
/// No setuid.
pub const MOUNT_ATTR_NOSUID: u64 = 0x00000002;
/// No device access.
pub const MOUNT_ATTR_NODEV: u64 = 0x00000004;
/// No exec.
pub const MOUNT_ATTR_NOEXEC: u64 = 0x00000008;
/// No access time updates.
pub const MOUNT_ATTR_NOATIME: u64 = 0x00000010;
/// Strict access time.
pub const MOUNT_ATTR_STRICTATIME: u64 = 0x00000020;
/// No directory access time.
pub const MOUNT_ATTR_NODIRATIME: u64 = 0x00000080;
/// ID-mapped mount.
pub const MOUNT_ATTR_IDMAP: u64 = 0x00100000;
/// No symlinks.
pub const MOUNT_ATTR_NOSYMFOLLOW: u64 = 0x00200000;

// ---------------------------------------------------------------------------
// listmount() flags
// ---------------------------------------------------------------------------

/// Reverse order.
pub const LISTMOUNT_REVERSE: u64 = 1 << 0;

// ---------------------------------------------------------------------------
// Special mount IDs
// ---------------------------------------------------------------------------

/// Current mount namespace root.
pub const LSMT_ROOT: u64 = 0xFFFFFFFFFFFFFFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statmount_masks_powers_of_two() {
        let masks = [
            STATMOUNT_SB_BASIC, STATMOUNT_MNT_BASIC,
            STATMOUNT_PROPAGATE_FROM, STATMOUNT_MNT_POINT,
            STATMOUNT_MNT_ROOT, STATMOUNT_FS_TYPE,
            STATMOUNT_MNT_NS_ID, STATMOUNT_MNT_OPTS,
            STATMOUNT_FS_SUBTYPE, STATMOUNT_SB_SOURCE,
        ];
        for m in &masks {
            assert!(m.is_power_of_two(), "mask {m:#x} not power of two");
        }
    }

    #[test]
    fn test_statmount_masks_no_overlap() {
        let masks = [
            STATMOUNT_SB_BASIC, STATMOUNT_MNT_BASIC,
            STATMOUNT_PROPAGATE_FROM, STATMOUNT_MNT_POINT,
            STATMOUNT_MNT_ROOT, STATMOUNT_FS_TYPE,
            STATMOUNT_MNT_NS_ID, STATMOUNT_MNT_OPTS,
            STATMOUNT_FS_SUBTYPE, STATMOUNT_SB_SOURCE,
        ];
        for i in 0..masks.len() {
            for j in (i + 1)..masks.len() {
                assert_eq!(masks[i] & masks[j], 0);
            }
        }
    }

    #[test]
    fn test_propagation_types_no_overlap() {
        let props = [MS_SHARED, MS_SLAVE, MS_PRIVATE, MS_UNBINDABLE];
        for i in 0..props.len() {
            for j in (i + 1)..props.len() {
                assert_eq!(props[i] & props[j], 0);
            }
        }
    }

    #[test]
    fn test_mount_attrs_distinct() {
        let attrs = [
            MOUNT_ATTR_RDONLY, MOUNT_ATTR_NOSUID, MOUNT_ATTR_NODEV,
            MOUNT_ATTR_NOEXEC, MOUNT_ATTR_NOATIME,
            MOUNT_ATTR_STRICTATIME, MOUNT_ATTR_NODIRATIME,
            MOUNT_ATTR_IDMAP, MOUNT_ATTR_NOSYMFOLLOW,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_lsmt_root() {
        assert_eq!(LSMT_ROOT, u64::MAX);
    }
}

//! `<linux/btrfs.h>` — Btrfs filesystem user-facing ioctls and limits.
//!
//! Btrfs exposes a rich ioctl surface for snapshots, subvolumes,
//! balance, scrub, and device management. This module covers the
//! base ioctl numbers (`BTRFS_IOC_*`), the magic/version constants,
//! and the fundamental on-disk limits.

// ---------------------------------------------------------------------------
// Magic and version
// ---------------------------------------------------------------------------

/// Btrfs filesystem magic — `_BHRfS_M` (little-endian).
pub const BTRFS_SUPER_MAGIC: u64 = 0x4D5F_5366_5248_425F;

/// Filesystem label maximum length (bytes, including NUL).
pub const BTRFS_LABEL_SIZE: usize = 256;

/// Maximum on-disk filename length.
pub const BTRFS_NAME_LEN: usize = 255;

/// Subvolume name max length (chars, excluding NUL).
pub const BTRFS_SUBVOL_NAME_MAX: usize = 4039;

/// Path-spec max for the `BTRFS_IOC_INO_LOOKUP` family.
pub const BTRFS_PATH_NAME_MAX: usize = 4087;

/// UUID-size constant.
pub const BTRFS_UUID_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Base ioctls (type 0x94 = '\x94')
// ---------------------------------------------------------------------------

/// `_IOW(0x94, 1, struct btrfs_ioctl_vol_args)` — `SNAP_CREATE`.
pub const BTRFS_IOC_SNAP_CREATE: u32 = 0x5000_9401;

/// `_IOW(0x94, 13, struct btrfs_ioctl_vol_args)` — `DEFRAG`.
pub const BTRFS_IOC_DEFRAG: u32 = 0x5000_940D;

/// `_IOW(0x94, 14, struct btrfs_ioctl_vol_args)` — `RESIZE`.
pub const BTRFS_IOC_RESIZE: u32 = 0x5000_940E;

/// `_IOW(0x94, 10, struct btrfs_ioctl_vol_args)` — `SUBVOL_CREATE`.
pub const BTRFS_IOC_SUBVOL_CREATE: u32 = 0x5000_940A;

/// `_IO(0x94, 5)` — `TRANS_START` (legacy).
pub const BTRFS_IOC_TRANS_START: u32 = 0x0000_9406;

/// `_IO(0x94, 6)` — `TRANS_END` (legacy).
pub const BTRFS_IOC_TRANS_END: u32 = 0x0000_9407;

// ---------------------------------------------------------------------------
// Subvolume flags
// ---------------------------------------------------------------------------

/// Subvolume is read-only.
pub const BTRFS_SUBVOL_RDONLY: u64 = 1 << 1;

/// Pass through the qgroup inheritance.
pub const BTRFS_SUBVOL_QGROUP_INHERIT: u64 = 1 << 2;

// ---------------------------------------------------------------------------
// Tree object IDs
// ---------------------------------------------------------------------------

/// Root tree objectid.
pub const BTRFS_ROOT_TREE_OBJECTID: u64 = 1;

/// FS tree objectid (the top-of-tree directory).
pub const BTRFS_FS_TREE_OBJECTID: u64 = 5;

/// First free objectid for user subvolumes.
pub const BTRFS_FIRST_FREE_OBJECTID: u64 = 256;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_decodes_to_bhrfs_m() {
        // Magic spells "_BHRfS_M" reading bytes low-to-high.
        let bytes = BTRFS_SUPER_MAGIC.to_le_bytes();
        assert_eq!(&bytes, b"_BHRfS_M");
    }

    #[test]
    fn test_name_size_limits() {
        assert_eq!(BTRFS_LABEL_SIZE, 256);
        assert_eq!(BTRFS_NAME_LEN, 255);
        // LABEL_SIZE = NAME_LEN + 1 (room for trailing NUL).
        assert_eq!(BTRFS_LABEL_SIZE, BTRFS_NAME_LEN + 1);
        assert_eq!(BTRFS_UUID_SIZE, 16);
    }

    #[test]
    fn test_path_and_subvol_limits_sum_to_4096_window() {
        // The 4039 + NUL + structure overhead sits inside a 4 KiB ioctl arg.
        assert!(BTRFS_SUBVOL_NAME_MAX < 4096);
        assert!(BTRFS_PATH_NAME_MAX < 4096);
        assert!(BTRFS_PATH_NAME_MAX > BTRFS_SUBVOL_NAME_MAX);
    }

    #[test]
    fn test_ioctls_in_btrfs_type_byte() {
        // All BTRFS ioctls live in the 0x94 type byte.
        for v in [
            BTRFS_IOC_SNAP_CREATE,
            BTRFS_IOC_DEFRAG,
            BTRFS_IOC_RESIZE,
            BTRFS_IOC_SUBVOL_CREATE,
            BTRFS_IOC_TRANS_START,
            BTRFS_IOC_TRANS_END,
        ] {
            assert_eq!((v >> 8) & 0xFF, 0x94);
        }
        // TRANS_END follows TRANS_START in the table.
        assert_eq!(BTRFS_IOC_TRANS_END - BTRFS_IOC_TRANS_START, 1);
    }

    #[test]
    fn test_iow_direction_bit_set_on_vol_args() {
        // _IOW direction bits are 0x4 in the top nibble of the high u32 half;
        // the encoded form starts with 0x5000.
        for v in [
            BTRFS_IOC_SNAP_CREATE,
            BTRFS_IOC_DEFRAG,
            BTRFS_IOC_RESIZE,
            BTRFS_IOC_SUBVOL_CREATE,
        ] {
            assert_eq!(v >> 28, 0x5);
        }
    }

    #[test]
    fn test_subvol_flags_distinct_bits() {
        assert!(BTRFS_SUBVOL_RDONLY.is_power_of_two());
        assert!(BTRFS_SUBVOL_QGROUP_INHERIT.is_power_of_two());
        assert_eq!(BTRFS_SUBVOL_RDONLY & BTRFS_SUBVOL_QGROUP_INHERIT, 0);
    }

    #[test]
    fn test_well_known_objectids() {
        assert_eq!(BTRFS_ROOT_TREE_OBJECTID, 1);
        assert_eq!(BTRFS_FS_TREE_OBJECTID, 5);
        assert_eq!(BTRFS_FIRST_FREE_OBJECTID, 256);
        // User subvolumes start above the reserved range.
        assert!(BTRFS_FIRST_FREE_OBJECTID > BTRFS_FS_TREE_OBJECTID);
    }
}

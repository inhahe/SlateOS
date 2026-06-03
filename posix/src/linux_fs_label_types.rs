//! `<linux/fs.h>` — Filesystem label and identification constants.
//!
//! These constants define filesystem label limits, UUID format
//! sizes, and superblock identification fields used by tools
//! like `blkid`, `lsblk`, and mount helpers.

// ---------------------------------------------------------------------------
// Filesystem label limits
// ---------------------------------------------------------------------------

/// Maximum filesystem label length for ext2/ext3/ext4.
pub const EXT_LABEL_MAX: u32 = 16;
/// Maximum filesystem label length for XFS.
pub const XFS_LABEL_MAX: u32 = 12;
/// Maximum filesystem label length for Btrfs.
pub const BTRFS_LABEL_MAX: u32 = 256;
/// Maximum filesystem label length for FAT/VFAT.
pub const FAT_LABEL_MAX: u32 = 11;
/// Maximum filesystem label length for NTFS.
pub const NTFS_LABEL_MAX: u32 = 128;
/// Maximum filesystem label length for exFAT.
pub const EXFAT_LABEL_MAX: u32 = 15;

// ---------------------------------------------------------------------------
// UUID format sizes
// ---------------------------------------------------------------------------

/// UUID string length (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx).
pub const UUID_STRING_LEN: u32 = 36;
/// UUID binary size in bytes.
pub const UUID_BINARY_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// Superblock feature flags (ext4 compatible features)
// ---------------------------------------------------------------------------

/// Has journal (ext3/ext4).
pub const EXT4_FEATURE_COMPAT_HAS_JOURNAL: u32 = 0x0004;
/// Extended attributes supported.
pub const EXT4_FEATURE_COMPAT_EXT_ATTR: u32 = 0x0008;
/// Resize inode present.
pub const EXT4_FEATURE_COMPAT_RESIZE_INODE: u32 = 0x0010;
/// Directory indexing (htree).
pub const EXT4_FEATURE_COMPAT_DIR_INDEX: u32 = 0x0020;

// ---------------------------------------------------------------------------
// Superblock state flags
// ---------------------------------------------------------------------------

/// Filesystem is cleanly unmounted.
pub const EXT4_VALID_FS: u16 = 0x0001;
/// Filesystem has errors.
pub const EXT4_ERROR_FS: u16 = 0x0002;
/// Orphan inodes being recovered.
pub const EXT4_ORPHAN_FS: u16 = 0x0004;

// ---------------------------------------------------------------------------
// blkid type identifiers
// ---------------------------------------------------------------------------

/// blkid partition type for Linux filesystem.
pub const BLKID_PART_LINUX: u8 = 0x83;
/// blkid partition type for Linux swap.
pub const BLKID_PART_SWAP: u8 = 0x82;
/// blkid partition type for Linux LVM.
pub const BLKID_PART_LVM: u8 = 0x8E;
/// blkid partition type for Linux RAID.
pub const BLKID_PART_RAID: u8 = 0xFD;
/// blkid partition type for EFI System.
pub const BLKID_PART_EFI: u8 = 0xEF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_limits_distinct() {
        let limits = [
            EXT_LABEL_MAX,
            XFS_LABEL_MAX,
            BTRFS_LABEL_MAX,
            FAT_LABEL_MAX,
            NTFS_LABEL_MAX,
            EXFAT_LABEL_MAX,
        ];
        for i in 0..limits.len() {
            for j in (i + 1)..limits.len() {
                assert_ne!(limits[i], limits[j]);
            }
        }
    }

    #[test]
    fn test_uuid_sizes() {
        assert_eq!(UUID_STRING_LEN, 36);
        assert_eq!(UUID_BINARY_SIZE, 16);
    }

    #[test]
    fn test_ext4_compat_features_no_overlap() {
        let feats = [
            EXT4_FEATURE_COMPAT_HAS_JOURNAL,
            EXT4_FEATURE_COMPAT_EXT_ATTR,
            EXT4_FEATURE_COMPAT_RESIZE_INODE,
            EXT4_FEATURE_COMPAT_DIR_INDEX,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_ext4_state_flags_no_overlap() {
        let states = [EXT4_VALID_FS, EXT4_ERROR_FS, EXT4_ORPHAN_FS];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_eq!(states[i] & states[j], 0);
            }
        }
    }

    #[test]
    fn test_blkid_parts_distinct() {
        let parts = [
            BLKID_PART_LINUX,
            BLKID_PART_SWAP,
            BLKID_PART_LVM,
            BLKID_PART_RAID,
            BLKID_PART_EFI,
        ];
        for i in 0..parts.len() {
            for j in (i + 1)..parts.len() {
                assert_ne!(parts[i], parts[j]);
            }
        }
    }

    #[test]
    fn test_ext_label_max() {
        assert_eq!(EXT_LABEL_MAX, 16);
    }

    #[test]
    fn test_btrfs_label_max() {
        assert_eq!(BTRFS_LABEL_MAX, 256);
    }
}

//! `<linux/fs.h>` — Filesystem ioctl flag and command constants.
//!
//! The FS_IOC_* ioctls control per-file and per-filesystem attributes
//! such as immutable, append-only, compression, and encryption flags.
//! These are the inode flags exposed via FS_IOC_GETFLAGS/SETFLAGS
//! and the newer fsxattr interface.

// ---------------------------------------------------------------------------
// FS_IOC inode flags (FS_IOC_GETFLAGS/FS_IOC_SETFLAGS)
// ---------------------------------------------------------------------------

/// Secure deletion (unused on most filesystems).
pub const FS_SECRM_FL: u32 = 0x0000_0001;
/// Undelete (unused on most filesystems).
pub const FS_UNRM_FL: u32 = 0x0000_0002;
/// Compress file.
pub const FS_COMPR_FL: u32 = 0x0000_0004;
/// Synchronous updates.
pub const FS_SYNC_FL: u32 = 0x0000_0008;
/// Immutable file.
pub const FS_IMMUTABLE_FL: u32 = 0x0000_0010;
/// Writes to file may only append.
pub const FS_APPEND_FL: u32 = 0x0000_0020;
/// Do not dump file.
pub const FS_NODUMP_FL: u32 = 0x0000_0040;
/// Do not update access time.
pub const FS_NOATIME_FL: u32 = 0x0000_0080;
/// Compressed file (internal).
pub const FS_DIRTY_FL: u32 = 0x0000_0100;
/// One or more compressed clusters.
pub const FS_COMPRBLK_FL: u32 = 0x0000_0200;
/// Don't compress (internal).
pub const FS_NOCOMP_FL: u32 = 0x0000_0400;
/// Encrypted file.
pub const FS_ENCRYPT_FL: u32 = 0x0000_0800;
/// Hash-indexed directory.
pub const FS_INDEX_FL: u32 = 0x0000_1000;
/// AFS directory.
pub const FS_IMAGIC_FL: u32 = 0x0000_2000;
/// Journal data mode (ext3/4).
pub const FS_JOURNAL_DATA_FL: u32 = 0x0000_4000;
/// Don't tail-merge.
pub const FS_NOTAIL_FL: u32 = 0x0000_8000;
/// Synchronous directory modifications.
pub const FS_DIRSYNC_FL: u32 = 0x0001_0000;
/// Top of directory hierarchy.
pub const FS_TOPDIR_FL: u32 = 0x0002_0000;
/// Huge file (ext4: use large extents).
pub const FS_HUGE_FILE_FL: u32 = 0x0004_0000;
/// Extents (ext4).
pub const FS_EXTENT_FL: u32 = 0x0008_0000;
/// fs-verity protected file.
pub const FS_VERITY_FL: u32 = 0x0010_0000;
/// File is DAX (direct access).
pub const FS_DAX_FL: u32 = 0x0200_0000;
/// Inode is using inline data.
pub const FS_INLINE_DATA_FL: u32 = 0x1000_0000;
/// Project ID assigned.
pub const FS_PROJINHERIT_FL: u32 = 0x2000_0000;
/// Case-insensitive directory.
pub const FS_CASEFOLD_FL: u32 = 0x4000_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inode_flags_no_overlap() {
        let flags = [
            FS_SECRM_FL,
            FS_UNRM_FL,
            FS_COMPR_FL,
            FS_SYNC_FL,
            FS_IMMUTABLE_FL,
            FS_APPEND_FL,
            FS_NODUMP_FL,
            FS_NOATIME_FL,
            FS_DIRTY_FL,
            FS_COMPRBLK_FL,
            FS_NOCOMP_FL,
            FS_ENCRYPT_FL,
            FS_INDEX_FL,
            FS_IMAGIC_FL,
            FS_JOURNAL_DATA_FL,
            FS_NOTAIL_FL,
            FS_DIRSYNC_FL,
            FS_TOPDIR_FL,
            FS_HUGE_FILE_FL,
            FS_EXTENT_FL,
            FS_VERITY_FL,
            FS_DAX_FL,
            FS_INLINE_DATA_FL,
            FS_PROJINHERIT_FL,
            FS_CASEFOLD_FL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_immutable_value() {
        assert_eq!(FS_IMMUTABLE_FL, 0x10);
    }

    #[test]
    fn test_encrypt_value() {
        assert_eq!(FS_ENCRYPT_FL, 0x800);
    }
}

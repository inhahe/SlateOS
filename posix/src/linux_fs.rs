//! `<linux/fs.h>` — filesystem ioctl and flag definitions.
//!
//! Provides filesystem-level ioctl numbers and file attribute flags
//! used by Linux filesystem operations.

// ---------------------------------------------------------------------------
// Filesystem ioctls
// ---------------------------------------------------------------------------

/// Get filesystem flags (ioctl).
pub const FS_IOC_GETFLAGS: u64 = 0x80086601;

/// Set filesystem flags (ioctl).
pub const FS_IOC_SETFLAGS: u64 = 0x40086602;

/// Get filesystem version (ioctl).
pub const FS_IOC_GETVERSION: u64 = 0x80087601;

/// Set filesystem version (ioctl).
pub const FS_IOC_SETVERSION: u64 = 0x40087602;

/// FIEMAP ioctl for extent mapping.
pub const FS_IOC_FIEMAP: u64 = 0xC020660B;

// ---------------------------------------------------------------------------
// File attribute flags (chattr / lsattr)
// ---------------------------------------------------------------------------

/// Secure deletion (not implemented by most fs).
pub const FS_SECRM_FL: u32 = 0x00000001;

/// Undelete (not implemented by most fs).
pub const FS_UNRM_FL: u32 = 0x00000002;

/// Compress file.
pub const FS_COMPR_FL: u32 = 0x00000004;

/// Synchronous updates.
pub const FS_SYNC_FL: u32 = 0x00000008;

/// Immutable file.
pub const FS_IMMUTABLE_FL: u32 = 0x00000010;

/// Append only.
pub const FS_APPEND_FL: u32 = 0x00000020;

/// Do not dump file.
pub const FS_NODUMP_FL: u32 = 0x00000040;

/// Do not update atime.
pub const FS_NOATIME_FL: u32 = 0x00000080;

/// Directory is encrypted.
pub const FS_ENCRYPT_FL: u32 = 0x00000800;

/// btree format dir.
pub const FS_BTREE_FL: u32 = 0x00001000;

/// Hash-indexed directory.
pub const FS_INDEX_FL: u32 = 0x00001000;

/// Reserved for ext2/3/4.
pub const FS_JOURNAL_DATA_FL: u32 = 0x00004000;

/// Do not tail-merge.
pub const FS_NOTAIL_FL: u32 = 0x00008000;

/// Dirsync: changes to this dir are synchronous.
pub const FS_DIRSYNC_FL: u32 = 0x00010000;

/// Top of directory hierarchy.
pub const FS_TOPDIR_FL: u32 = 0x00020000;

/// Use huge pages.
pub const FS_HUGE_FILE_FL: u32 = 0x00040000;

/// File uses extents.
pub const FS_EXTENT_FL: u32 = 0x00080000;

/// Verity protected inode.
pub const FS_VERITY_FL: u32 = 0x00100000;

/// File is DAX (direct access, no page cache).
pub const FS_DAX_FL: u32 = 0x02000000;

/// Inode uses inline data.
pub const FS_INLINE_DATA_FL: u32 = 0x10000000;

/// Project hierarchy.
pub const FS_PROJINHERIT_FL: u32 = 0x20000000;

/// Case-insensitive directory.
pub const FS_CASEFOLD_FL: u32 = 0x40000000;

// ---------------------------------------------------------------------------
// Block size
// ---------------------------------------------------------------------------

/// Minimum block size.
pub const BLKROSET: u64 = 0x125D;

/// Get read-only state.
pub const BLKROGET: u64 = 0x125E;

/// Re-read partition table.
pub const BLKRRPART: u64 = 0x125F;

/// Get block device size in sectors.
pub const BLKGETSIZE: u64 = 0x1260;

/// Flush buffer cache.
pub const BLKFLSBUF: u64 = 0x1261;

/// Set read-ahead.
pub const BLKRASET: u64 = 0x1262;

/// Get read-ahead.
pub const BLKRAGET: u64 = 0x1263;

/// Get block device size in bytes (u64).
pub const BLKGETSIZE64: u64 = 0x80081272;

/// Get sector size.
pub const BLKSSZGET: u64 = 0x1268;

/// Get block size.
pub const BLKBSZGET: u64 = 0x80081270;

/// Set block size.
pub const BLKBSZSET: u64 = 0x40081271;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_ioc_distinct() {
        let iocs = [
            FS_IOC_GETFLAGS, FS_IOC_SETFLAGS,
            FS_IOC_GETVERSION, FS_IOC_SETVERSION,
            FS_IOC_FIEMAP,
        ];
        for i in 0..iocs.len() {
            for j in (i + 1)..iocs.len() {
                assert_ne!(iocs[i], iocs[j]);
            }
        }
    }

    #[test]
    fn test_fs_flags_powers_of_two() {
        let flags = [
            FS_SECRM_FL, FS_UNRM_FL, FS_COMPR_FL, FS_SYNC_FL,
            FS_IMMUTABLE_FL, FS_APPEND_FL, FS_NODUMP_FL, FS_NOATIME_FL,
            FS_DIRSYNC_FL, FS_TOPDIR_FL, FS_EXTENT_FL,
        ];
        for &f in &flags {
            assert!(
                f.count_ones() == 1,
                "FS flag 0x{f:X} should be power of 2"
            );
        }
    }

    #[test]
    fn test_immutable_append_distinct() {
        assert_ne!(FS_IMMUTABLE_FL, FS_APPEND_FL);
    }

    #[test]
    fn test_blk_ioctls_distinct() {
        let iocs = [
            BLKROSET, BLKROGET, BLKRRPART, BLKGETSIZE,
            BLKFLSBUF, BLKRASET, BLKRAGET, BLKGETSIZE64,
            BLKSSZGET, BLKBSZGET, BLKBSZSET,
        ];
        for i in 0..iocs.len() {
            for j in (i + 1)..iocs.len() {
                assert_ne!(iocs[i], iocs[j], "BLK ioctls must be distinct");
            }
        }
    }
}

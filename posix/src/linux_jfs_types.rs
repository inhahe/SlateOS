//! `<linux/jfs_fs.h>` — JFS (Journaled File System) constants.
//!
//! JFS is IBM's journaling filesystem ported to Linux.
//! These constants define superblock flags, inode flags,
//! and journal parameters.

// ---------------------------------------------------------------------------
// Superblock magic
// ---------------------------------------------------------------------------

/// JFS superblock magic.
pub const JFS_SUPER_MAGIC: u32 = 0x3153464A;

// ---------------------------------------------------------------------------
// Superblock flags (JFS_SBI_*)
// ---------------------------------------------------------------------------

/// Case insensitive names.
pub const JFS_SBI_CASE_INSENSITIVE: u32 = 0x00000001;
/// ACLs enabled.
pub const JFS_SBI_ACL: u32 = 0x00000002;
/// Unicode names.
pub const JFS_SBI_UNICODE: u32 = 0x00000004;
/// OS/2 EA format.
pub const JFS_SBI_OS2_EA: u32 = 0x00000008;
/// POSIX EA format.
pub const JFS_SBI_POSIX_EA: u32 = 0x00000010;
/// Large file support.
pub const JFS_SBI_LARGEFILE: u32 = 0x00000020;
/// Inline data.
pub const JFS_SBI_INLINELOG: u32 = 0x00000800;

// ---------------------------------------------------------------------------
// Inode flags (JFS_*)
// ---------------------------------------------------------------------------

/// Inode is directory.
pub const JFS_DIR_FL: u32 = 0x00000001;
/// Inode is regular file.
pub const JFS_FILE_FL: u32 = 0x00000002;
/// Inode is symlink.
pub const JFS_SYMLINK_FL: u32 = 0x00000004;
/// Secure deletion.
pub const JFS_SECRM_FL: u32 = 0x00000010;
/// Immutable.
pub const JFS_IMMUTABLE_FL: u32 = 0x00000020;
/// Append only.
pub const JFS_APPEND_FL: u32 = 0x00000040;
/// No dump.
pub const JFS_NODUMP_FL: u32 = 0x00000080;
/// No atime.
pub const JFS_NOATIME_FL: u32 = 0x00000100;
/// Synchronous writes.
pub const JFS_SYNC_FL: u32 = 0x00000200;
/// Data sync.
pub const JFS_DIRSYNC_FL: u32 = 0x00000400;

// ---------------------------------------------------------------------------
// Extent allocation
// ---------------------------------------------------------------------------

/// B+ tree leaf page size.
pub const JFS_LPAGE_SIZE: u32 = 4096;
/// Minimum aggregate block size.
pub const JFS_MIN_BLOCK_SIZE: u32 = 512;
/// Maximum aggregate block size.
pub const JFS_MAX_BLOCK_SIZE: u32 = 4096;
/// Maximum inline EA size.
pub const JFS_MAX_INLINE_EA_SIZE: u32 = 128;
/// Maximum xattr size.
pub const JFS_MAX_XATTR_SIZE: u32 = 65536;

// ---------------------------------------------------------------------------
// Log/journal constants
// ---------------------------------------------------------------------------

/// Log magic number.
pub const JFS_LOG_MAGIC: u32 = 0x87654321;
/// Minimum log size (in pages).
pub const JFS_MIN_LOG_SIZE: u32 = 2048;
/// Log record commit type.
pub const JFS_LOG_COMMIT: u32 = 0x0004;
/// Log record mount type.
pub const JFS_LOG_MOUNT: u32 = 0x0008;
/// Log record syncpt type.
pub const JFS_LOG_SYNCPT: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Special inode numbers
// ---------------------------------------------------------------------------

/// Root inode.
pub const JFS_ROOT_I: u32 = 2;
/// Reserved aggregate inode (block map).
pub const JFS_AGGREGATE_I: u32 = 0;
/// Fileset inode.
pub const JFS_FILESYSTEM_I: u32 = 16;
/// Bad block inode.
pub const JFS_BADBLOCK_I: u32 = 1;

// ---------------------------------------------------------------------------
// Superblock state
// ---------------------------------------------------------------------------

/// Cleanly unmounted.
pub const JFS_MOUNT_CLEAN: u32 = 0;
/// Dirty (needs fsck).
pub const JFS_MOUNT_DIRTY: u32 = 1;
/// Log redo needed.
pub const JFS_MOUNT_LOGREDO: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(JFS_SUPER_MAGIC, 0x3153464A);
    }

    #[test]
    fn test_sbi_flags_distinct() {
        let flags = [
            JFS_SBI_CASE_INSENSITIVE, JFS_SBI_ACL, JFS_SBI_UNICODE,
            JFS_SBI_OS2_EA, JFS_SBI_POSIX_EA, JFS_SBI_LARGEFILE,
            JFS_SBI_INLINELOG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_inode_flags_distinct() {
        let flags = [
            JFS_DIR_FL, JFS_FILE_FL, JFS_SYMLINK_FL,
            JFS_SECRM_FL, JFS_IMMUTABLE_FL, JFS_APPEND_FL,
            JFS_NODUMP_FL, JFS_NOATIME_FL, JFS_SYNC_FL,
            JFS_DIRSYNC_FL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_block_size_ordering() {
        assert!(JFS_MIN_BLOCK_SIZE < JFS_MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_lpage_size() {
        assert_eq!(JFS_LPAGE_SIZE, 4096);
        assert!(JFS_LPAGE_SIZE.is_power_of_two());
    }

    #[test]
    fn test_log_magic() {
        assert_eq!(JFS_LOG_MAGIC, 0x87654321);
    }

    #[test]
    fn test_log_types_distinct() {
        let types = [JFS_LOG_COMMIT, JFS_LOG_MOUNT, JFS_LOG_SYNCPT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_mount_states_sequential() {
        assert_eq!(JFS_MOUNT_CLEAN, 0);
        assert_eq!(JFS_MOUNT_DIRTY, 1);
        assert_eq!(JFS_MOUNT_LOGREDO, 2);
    }

    #[test]
    fn test_root_inode() {
        assert_eq!(JFS_ROOT_I, 2);
    }

    #[test]
    fn test_special_inodes_distinct() {
        let inodes = [JFS_AGGREGATE_I, JFS_BADBLOCK_I, JFS_ROOT_I, JFS_FILESYSTEM_I];
        for i in 0..inodes.len() {
            for j in (i + 1)..inodes.len() {
                assert_ne!(inodes[i], inodes[j]);
            }
        }
    }
}

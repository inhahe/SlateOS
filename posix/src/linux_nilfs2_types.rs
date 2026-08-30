//! `<linux/nilfs2_ondisk.h>` — NILFS2 (New Implementation of a Log-structured File System) constants.
//!
//! NILFS2 is a log-structured filesystem with continuous snapshotting.
//! These constants define superblock fields, inode flags,
//! checkpoint types, and segment usage.

// ---------------------------------------------------------------------------
// Superblock magic
// ---------------------------------------------------------------------------

/// NILFS2 superblock magic.
pub const NILFS_SUPER_MAGIC: u32 = 0x3434;

// ---------------------------------------------------------------------------
// Superblock revision
// ---------------------------------------------------------------------------

/// Major revision 2.
pub const NILFS_SUPER_REV_MAJOR: u32 = 2;
/// Minor revision 0.
pub const NILFS_SUPER_REV_MINOR: u32 = 0;

// ---------------------------------------------------------------------------
// Special inode numbers
// ---------------------------------------------------------------------------

/// Root directory inode.
pub const NILFS_ROOT_INO: u64 = 2;
/// DAT (disk address translation) inode.
pub const NILFS_DAT_INO: u64 = 3;
/// CPFILE inode.
pub const NILFS_CPFILE_INO: u64 = 4;
/// SUFILE inode.
pub const NILFS_SUFILE_INO: u64 = 5;
/// Inode file inode.
pub const NILFS_IFILE_INO: u64 = 6;
/// .sketch file inode.
pub const NILFS_ATIME_INO: u64 = 7;
/// .nilfs file inode.
pub const NILFS_XATTR_INO: u64 = 8;
/// Sketch inode.
pub const NILFS_SKETCH_INO: u64 = 10;
/// User inode start.
pub const NILFS_USER_INO: u64 = 11;

// ---------------------------------------------------------------------------
// Inode flags (NILFS_INODE_*)
// ---------------------------------------------------------------------------

/// Secure deletion.
pub const NILFS_INODE_SECRM_FL: u32 = 0x00000001;
/// Undelete.
pub const NILFS_INODE_UNRM_FL: u32 = 0x00000002;
/// Compressed.
pub const NILFS_INODE_COMPR_FL: u32 = 0x00000004;
/// Synchronous updates.
pub const NILFS_INODE_SYNC_FL: u32 = 0x00000008;
/// Immutable.
pub const NILFS_INODE_IMMUTABLE_FL: u32 = 0x00000010;
/// Append only.
pub const NILFS_INODE_APPEND_FL: u32 = 0x00000020;
/// No dump.
pub const NILFS_INODE_NODUMP_FL: u32 = 0x00000040;
/// No atime updates.
pub const NILFS_INODE_NOATIME_FL: u32 = 0x00000080;
/// Btree format directory.
pub const NILFS_INODE_BTREE_FL: u32 = 0x00001000;
/// Index directory.
pub const NILFS_INODE_INDEX_FL: u32 = 0x00001000;

// ---------------------------------------------------------------------------
// Checkpoint flags
// ---------------------------------------------------------------------------

/// Checkpoint is a snapshot.
pub const NILFS_CHECKPOINT_SNAPSHOT: u32 = 1;
/// Checkpoint is invalid.
pub const NILFS_CHECKPOINT_INVALID: u32 = 2;
/// Checkpoint is a sketch.
pub const NILFS_CHECKPOINT_SKETCH: u32 = 4;
/// Minor checkpoint.
pub const NILFS_CHECKPOINT_MINOR: u32 = 8;

// ---------------------------------------------------------------------------
// Segment usage flags
// ---------------------------------------------------------------------------

/// Segment is active.
pub const NILFS_SEGMENT_USAGE_ACTIVE: u32 = 1;
/// Segment is dirty.
pub const NILFS_SEGMENT_USAGE_DIRTY: u32 = 2;
/// Segment has error.
pub const NILFS_SEGMENT_USAGE_ERROR: u32 = 4;

// ---------------------------------------------------------------------------
// GC/cleaner IOCTL numbers
// ---------------------------------------------------------------------------

/// Get suinfo IOCTL.
pub const NILFS_IOCTL_GET_SUINFO: u32 = 0x80;
/// Set suinfo IOCTL.
pub const NILFS_IOCTL_SET_SUINFO: u32 = 0x81;
/// Get cpinfo IOCTL.
pub const NILFS_IOCTL_GET_CPINFO: u32 = 0x82;
/// Get cpstat IOCTL.
pub const NILFS_IOCTL_GET_CPSTAT: u32 = 0x83;
/// Change cpmode IOCTL.
pub const NILFS_IOCTL_CHANGE_CPMODE: u32 = 0x84;
/// Delete checkpoint IOCTL.
pub const NILFS_IOCTL_DELETE_CHECKPOINT: u32 = 0x85;
/// Get sustat IOCTL.
pub const NILFS_IOCTL_GET_SUSTAT: u32 = 0x86;
/// Clean segments IOCTL.
pub const NILFS_IOCTL_CLEAN_SEGMENTS: u32 = 0x88;
/// Resize IOCTL.
pub const NILFS_IOCTL_RESIZE: u32 = 0x8B;
/// Set alloc range IOCTL.
pub const NILFS_IOCTL_SET_ALLOC_RANGE: u32 = 0x8C;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// Minimum block size.
pub const NILFS_MIN_BLOCK_SIZE: u32 = 1024;
/// Maximum block size.
pub const NILFS_MAX_BLOCK_SIZE: u32 = 65536;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(NILFS_SUPER_MAGIC, 0x3434);
    }

    #[test]
    fn test_special_inodes_distinct() {
        let inodes = [
            NILFS_ROOT_INO,
            NILFS_DAT_INO,
            NILFS_CPFILE_INO,
            NILFS_SUFILE_INO,
            NILFS_IFILE_INO,
            NILFS_ATIME_INO,
            NILFS_XATTR_INO,
            NILFS_SKETCH_INO,
            NILFS_USER_INO,
        ];
        for i in 0..inodes.len() {
            for j in (i + 1)..inodes.len() {
                assert_ne!(inodes[i], inodes[j]);
            }
        }
    }

    #[test]
    fn test_root_is_two() {
        assert_eq!(NILFS_ROOT_INO, 2);
    }

    #[test]
    fn test_inode_flags_power_of_two() {
        let flags = [
            NILFS_INODE_SECRM_FL,
            NILFS_INODE_UNRM_FL,
            NILFS_INODE_COMPR_FL,
            NILFS_INODE_SYNC_FL,
            NILFS_INODE_IMMUTABLE_FL,
            NILFS_INODE_APPEND_FL,
            NILFS_INODE_NODUMP_FL,
            NILFS_INODE_NOATIME_FL,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_checkpoint_flags_power_of_two() {
        let flags = [
            NILFS_CHECKPOINT_SNAPSHOT,
            NILFS_CHECKPOINT_INVALID,
            NILFS_CHECKPOINT_SKETCH,
            NILFS_CHECKPOINT_MINOR,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "{} not power of two", f);
        }
    }

    #[test]
    fn test_segment_flags_power_of_two() {
        let flags = [
            NILFS_SEGMENT_USAGE_ACTIVE,
            NILFS_SEGMENT_USAGE_DIRTY,
            NILFS_SEGMENT_USAGE_ERROR,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "{} not power of two", f);
        }
    }

    #[test]
    fn test_ioctl_numbers_distinct() {
        let ioctls = [
            NILFS_IOCTL_GET_SUINFO,
            NILFS_IOCTL_SET_SUINFO,
            NILFS_IOCTL_GET_CPINFO,
            NILFS_IOCTL_GET_CPSTAT,
            NILFS_IOCTL_CHANGE_CPMODE,
            NILFS_IOCTL_DELETE_CHECKPOINT,
            NILFS_IOCTL_GET_SUSTAT,
            NILFS_IOCTL_CLEAN_SEGMENTS,
            NILFS_IOCTL_RESIZE,
            NILFS_IOCTL_SET_ALLOC_RANGE,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_block_size_ordering() {
        assert!(NILFS_MIN_BLOCK_SIZE < NILFS_MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_block_sizes_power_of_two() {
        assert!(NILFS_MIN_BLOCK_SIZE.is_power_of_two());
        assert!(NILFS_MAX_BLOCK_SIZE.is_power_of_two());
    }
}

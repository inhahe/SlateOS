//! `<linux/qnx6_fs.h>` — QNX6 filesystem constants.
//!
//! QNX6 is the power-safe filesystem of QNX Neutrino RTOS.
//! These constants define magic numbers, superblock fields,
//! and inode parameters.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// QNX6 superblock magic.
pub const QNX6_SUPER_MAGIC: u32 = 0x68191122;

// ---------------------------------------------------------------------------
// Superblock parameters
// ---------------------------------------------------------------------------

/// Superblock offset (bytes).
pub const QNX6_SUPER_OFFSET: u32 = 0x2000;
/// Bootblock size (bytes).
pub const QNX6_BOOTBLOCK_SIZE: u32 = 0x2000;
/// Root inode number.
pub const QNX6_ROOT_INO: u32 = 1;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// Minimum block size (512 bytes).
pub const QNX6_MIN_BLOCK_SIZE: u32 = 512;
/// Maximum block size (4 KiB).
pub const QNX6_MAX_BLOCK_SIZE: u32 = 4096;
/// Default block size.
pub const QNX6_DEFAULT_BLOCK_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Inode parameters
// ---------------------------------------------------------------------------

/// Short symlink max length.
pub const QNX6_SHORT_SYMLINK_LEN: u32 = 128;
/// Maximum file name length.
pub const QNX6_LONG_NAME_MAX: u32 = 510;
/// Short name max.
pub const QNX6_SHORT_NAME_MAX: u32 = 27;
/// Number of direct block pointers.
pub const QNX6_NUM_DIRECT_PTRS: u32 = 16;
/// Maximum number of tree levels.
pub const QNX6_MAX_TREE_LEVELS: u32 = 4;

// ---------------------------------------------------------------------------
// Inode flags
// ---------------------------------------------------------------------------

/// Inode is directory.
pub const QNX6_INODE_DIR: u16 = 0x01;
/// Inode is deleted.
pub const QNX6_INODE_DELETED: u16 = 0x02;
/// Inode is in use.
pub const QNX6_INODE_IN_USE: u16 = 0x04;

// ---------------------------------------------------------------------------
// Filesystem features
// ---------------------------------------------------------------------------

/// Long filenames supported.
pub const QNX6_FEATURE_LONGFILENAMES: u32 = 0x01;
/// Sparse files supported.
pub const QNX6_FEATURE_SPARSE: u32 = 0x02;

// ---------------------------------------------------------------------------
// Directory entry sizes
// ---------------------------------------------------------------------------

/// Short dir entry size.
pub const QNX6_SHORT_DIR_ENTRY_SIZE: u32 = 32;
/// Long dir entry header size.
pub const QNX6_LONG_DIR_ENTRY_HEADER: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_super_magic() {
        assert_eq!(QNX6_SUPER_MAGIC, 0x68191122);
    }

    #[test]
    fn test_root_inode() {
        assert_eq!(QNX6_ROOT_INO, 1);
    }

    #[test]
    fn test_block_size_ordering() {
        assert!(QNX6_MIN_BLOCK_SIZE < QNX6_MAX_BLOCK_SIZE);
        assert_eq!(QNX6_DEFAULT_BLOCK_SIZE, QNX6_MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_block_sizes_power_of_two() {
        assert!(QNX6_MIN_BLOCK_SIZE.is_power_of_two());
        assert!(QNX6_MAX_BLOCK_SIZE.is_power_of_two());
    }

    #[test]
    fn test_name_lengths() {
        assert!(QNX6_SHORT_NAME_MAX < QNX6_LONG_NAME_MAX);
    }

    #[test]
    fn test_inode_flags_distinct() {
        let flags: [u16; 3] = [QNX6_INODE_DIR, QNX6_INODE_DELETED, QNX6_INODE_IN_USE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_inode_flags_power_of_two() {
        let flags: [u16; 3] = [QNX6_INODE_DIR, QNX6_INODE_DELETED, QNX6_INODE_IN_USE];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

    #[test]
    fn test_features_power_of_two() {
        assert!(QNX6_FEATURE_LONGFILENAMES.is_power_of_two());
        assert!(QNX6_FEATURE_SPARSE.is_power_of_two());
    }

    #[test]
    fn test_super_offset() {
        assert_eq!(QNX6_SUPER_OFFSET, 0x2000);
    }

    #[test]
    fn test_direct_ptrs() {
        assert_eq!(QNX6_NUM_DIRECT_PTRS, 16);
    }
}

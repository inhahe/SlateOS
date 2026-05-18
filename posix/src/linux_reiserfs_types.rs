//! `<linux/reiserfs_fs.h>` — ReiserFS filesystem constants.
//!
//! ReiserFS is a B*-tree based journaling filesystem.
//! These constants define superblock magic, hash types,
//! item types, and journal parameters.

// ---------------------------------------------------------------------------
// Superblock magic
// ---------------------------------------------------------------------------

/// ReiserFS v3.5 magic.
pub const REISERFS_SUPER_MAGIC_V35: u32 = 0x52654973;
/// ReiserFS v3.6 magic.
pub const REISERFS_SUPER_MAGIC_V36: u32 = 0x52654973;
/// ReiserFS JR magic string offset.
pub const REISERFS_SUPER_MAGIC_OFFSET: u32 = 52;
/// Disk format version 1.
pub const REISERFS_FORMAT_3_5: u32 = 0;
/// Disk format version 2.
pub const REISERFS_FORMAT_3_6: u32 = 2;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// Default block size (4 KiB).
pub const REISERFS_DEFAULT_BLOCK_SIZE: u32 = 4096;
/// Minimum block size.
pub const REISERFS_MIN_BLOCK_SIZE: u32 = 512;
/// Maximum block size.
pub const REISERFS_MAX_BLOCK_SIZE: u32 = 8192;

// ---------------------------------------------------------------------------
// Hash types
// ---------------------------------------------------------------------------

/// Unset hash.
pub const REISERFS_HASH_UNSET: u32 = 0;
/// TEA hash.
pub const REISERFS_HASH_TEA: u32 = 1;
/// YURA hash.
pub const REISERFS_HASH_YURA: u32 = 2;
/// R5 hash.
pub const REISERFS_HASH_R5: u32 = 3;

// ---------------------------------------------------------------------------
// Item types (key types)
// ---------------------------------------------------------------------------

/// Stat data.
pub const REISERFS_TYPE_STAT_DATA: u32 = 0;
/// Indirect item.
pub const REISERFS_TYPE_INDIRECT: u32 = 1;
/// Direct item.
pub const REISERFS_TYPE_DIRECT: u32 = 2;
/// Directory item.
pub const REISERFS_TYPE_DIRENTRY: u32 = 3;
/// Any item (for search).
pub const REISERFS_TYPE_ANY: u32 = 15;

// ---------------------------------------------------------------------------
// Journal parameters
// ---------------------------------------------------------------------------

/// Default journal size (in blocks).
pub const REISERFS_DEFAULT_JOURNAL_SIZE: u32 = 8192;
/// Maximum journal transactions.
pub const REISERFS_DEFAULT_MAX_TRANS_SIZE: u32 = 1024;
/// Journal magic.
pub const REISERFS_JOURNAL_MAGIC: u32 = 0x1234;
/// Maximum journal age (seconds).
pub const REISERFS_DEFAULT_MAX_TRANS_AGE: u32 = 30;

// ---------------------------------------------------------------------------
// Mount options flags
// ---------------------------------------------------------------------------

/// No tail packing.
pub const REISERFS_NO_TAIL: u32 = 0x00000001;
/// Ordered data mode.
pub const REISERFS_DATA_ORDERED: u32 = 0x00000002;
/// Writeback data mode.
pub const REISERFS_DATA_WRITEBACK: u32 = 0x00000004;
/// Journal data mode.
pub const REISERFS_DATA_JOURNAL: u32 = 0x00000008;
/// No border packing.
pub const REISERFS_NO_BORDER: u32 = 0x00000010;
/// No unhashed relocation.
pub const REISERFS_NO_UNHASHED_RELOCATION: u32 = 0x00000020;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formats_distinct() {
        assert_ne!(REISERFS_FORMAT_3_5, REISERFS_FORMAT_3_6);
    }

    #[test]
    fn test_block_size_ordering() {
        assert!(REISERFS_MIN_BLOCK_SIZE < REISERFS_DEFAULT_BLOCK_SIZE);
        assert!(REISERFS_DEFAULT_BLOCK_SIZE < REISERFS_MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_hash_types_sequential() {
        assert_eq!(REISERFS_HASH_UNSET, 0);
        assert_eq!(REISERFS_HASH_TEA, 1);
        assert_eq!(REISERFS_HASH_YURA, 2);
        assert_eq!(REISERFS_HASH_R5, 3);
    }

    #[test]
    fn test_item_types_distinct() {
        let types = [
            REISERFS_TYPE_STAT_DATA, REISERFS_TYPE_INDIRECT,
            REISERFS_TYPE_DIRECT, REISERFS_TYPE_DIRENTRY,
            REISERFS_TYPE_ANY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_journal_defaults() {
        assert_eq!(REISERFS_DEFAULT_JOURNAL_SIZE, 8192);
        assert_eq!(REISERFS_DEFAULT_MAX_TRANS_SIZE, 1024);
    }

    #[test]
    fn test_mount_flags_distinct() {
        let flags = [
            REISERFS_NO_TAIL, REISERFS_DATA_ORDERED,
            REISERFS_DATA_WRITEBACK, REISERFS_DATA_JOURNAL,
            REISERFS_NO_BORDER, REISERFS_NO_UNHASHED_RELOCATION,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_mount_flags_power_of_two() {
        let flags = [
            REISERFS_NO_TAIL, REISERFS_DATA_ORDERED,
            REISERFS_DATA_WRITEBACK, REISERFS_DATA_JOURNAL,
            REISERFS_NO_BORDER, REISERFS_NO_UNHASHED_RELOCATION,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_journal_magic() {
        assert_eq!(REISERFS_JOURNAL_MAGIC, 0x1234);
    }
}

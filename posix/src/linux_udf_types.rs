//! `<linux/udf_fs.h>` — UDF (Universal Disk Format) constants.
//!
//! UDF is the filesystem standard for DVDs, Blu-ray discs, and large
//! removable media. It supports packet writing (incremental writes to
//! optical media), long filenames, large files, and advanced metadata.
//! Linux uses it for DVD±RW, BD-RE, and large USB drives.

// ---------------------------------------------------------------------------
// UDF identification
// ---------------------------------------------------------------------------

/// UDF standard identifier for BEA01.
pub const UDF_BEA_ID: &str = "BEA01";
/// UDF standard identifier for NSR02 (UDF 1.50).
pub const UDF_NSR02_ID: &str = "NSR02";
/// UDF standard identifier for NSR03 (UDF 2.x).
pub const UDF_NSR03_ID: &str = "NSR03";
/// UDF standard identifier for TEA01.
pub const UDF_TEA_ID: &str = "TEA01";

// ---------------------------------------------------------------------------
// Tag identifiers
// ---------------------------------------------------------------------------

/// Primary volume descriptor.
pub const UDF_TAG_PRIMARY_VOL_DESC: u16 = 1;
/// Anchor volume descriptor pointer.
pub const UDF_TAG_ANCHOR_VOL_DESC_PTR: u16 = 2;
/// Volume descriptor pointer.
pub const UDF_TAG_VOL_DESC_PTR: u16 = 3;
/// Partition descriptor.
pub const UDF_TAG_PARTITION_DESC: u16 = 5;
/// Logical volume descriptor.
pub const UDF_TAG_LOGICAL_VOL_DESC: u16 = 6;
/// Unallocated space descriptor.
pub const UDF_TAG_UNALLOC_SPACE_DESC: u16 = 7;
/// Terminating descriptor.
pub const UDF_TAG_TERMINATING_DESC: u16 = 8;
/// File set descriptor.
pub const UDF_TAG_FILE_SET_DESC: u16 = 256;
/// File identifier descriptor.
pub const UDF_TAG_FILE_IDENT_DESC: u16 = 257;
/// File entry.
pub const UDF_TAG_FILE_ENTRY: u16 = 261;
/// Extended file entry.
pub const UDF_TAG_EXT_FILE_ENTRY: u16 = 266;

// ---------------------------------------------------------------------------
// File type (icbtag)
// ---------------------------------------------------------------------------

/// Unallocated.
pub const UDF_ICBTAG_FILE_TYPE_UNALLOC: u8 = 0;
/// Directory.
pub const UDF_ICBTAG_FILE_TYPE_DIR: u8 = 4;
/// Regular file.
pub const UDF_ICBTAG_FILE_TYPE_REGULAR: u8 = 5;
/// Block device.
pub const UDF_ICBTAG_FILE_TYPE_BLOCK: u8 = 6;
/// Character device.
pub const UDF_ICBTAG_FILE_TYPE_CHAR: u8 = 7;
/// FIFO.
pub const UDF_ICBTAG_FILE_TYPE_FIFO: u8 = 9;
/// Socket.
pub const UDF_ICBTAG_FILE_TYPE_SOCKET: u8 = 10;
/// Symbolic link.
pub const UDF_ICBTAG_FILE_TYPE_SYMLINK: u8 = 12;
/// Stream directory.
pub const UDF_ICBTAG_FILE_TYPE_STREAMDIR: u8 = 13;

// ---------------------------------------------------------------------------
// Sector size
// ---------------------------------------------------------------------------

/// UDF logical sector size.
pub const UDF_SECTOR_SIZE: u16 = 2048;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ids_distinct() {
        let ids = [UDF_BEA_ID, UDF_NSR02_ID, UDF_NSR03_ID, UDF_TEA_ID];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_tag_identifiers_distinct() {
        let tags = [
            UDF_TAG_PRIMARY_VOL_DESC, UDF_TAG_ANCHOR_VOL_DESC_PTR,
            UDF_TAG_VOL_DESC_PTR, UDF_TAG_PARTITION_DESC,
            UDF_TAG_LOGICAL_VOL_DESC, UDF_TAG_UNALLOC_SPACE_DESC,
            UDF_TAG_TERMINATING_DESC, UDF_TAG_FILE_SET_DESC,
            UDF_TAG_FILE_IDENT_DESC, UDF_TAG_FILE_ENTRY,
            UDF_TAG_EXT_FILE_ENTRY,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }

    #[test]
    fn test_file_types_distinct() {
        let types = [
            UDF_ICBTAG_FILE_TYPE_UNALLOC, UDF_ICBTAG_FILE_TYPE_DIR,
            UDF_ICBTAG_FILE_TYPE_REGULAR, UDF_ICBTAG_FILE_TYPE_BLOCK,
            UDF_ICBTAG_FILE_TYPE_CHAR, UDF_ICBTAG_FILE_TYPE_FIFO,
            UDF_ICBTAG_FILE_TYPE_SOCKET, UDF_ICBTAG_FILE_TYPE_SYMLINK,
            UDF_ICBTAG_FILE_TYPE_STREAMDIR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sector_size() {
        assert_eq!(UDF_SECTOR_SIZE, 2048);
    }
}

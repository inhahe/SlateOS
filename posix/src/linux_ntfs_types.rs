//! `<linux/ntfs.h>` — NTFS filesystem constants.
//!
//! Constants for NTFS attribute types, file attribute flags,
//! and MFT record values used by the Linux NTFS3 driver.

// ---------------------------------------------------------------------------
// NTFS attribute types (ATTR_TYPE_*)
// ---------------------------------------------------------------------------

/// Standard information attribute.
pub const ATTR_TYPE_STANDARD_INFORMATION: u32 = 0x10;
/// Attribute list.
pub const ATTR_TYPE_ATTRIBUTE_LIST: u32 = 0x20;
/// File name attribute.
pub const ATTR_TYPE_FILE_NAME: u32 = 0x30;
/// Object ID attribute.
pub const ATTR_TYPE_OBJECT_ID: u32 = 0x40;
/// Security descriptor.
pub const ATTR_TYPE_SECURITY_DESCRIPTOR: u32 = 0x50;
/// Volume name.
pub const ATTR_TYPE_VOLUME_NAME: u32 = 0x60;
/// Volume information.
pub const ATTR_TYPE_VOLUME_INFORMATION: u32 = 0x70;
/// Data attribute.
pub const ATTR_TYPE_DATA: u32 = 0x80;
/// Index root.
pub const ATTR_TYPE_INDEX_ROOT: u32 = 0x90;
/// Index allocation.
pub const ATTR_TYPE_INDEX_ALLOCATION: u32 = 0xA0;
/// Bitmap attribute.
pub const ATTR_TYPE_BITMAP: u32 = 0xB0;
/// Reparse point.
pub const ATTR_TYPE_REPARSE_POINT: u32 = 0xC0;
/// EA information.
pub const ATTR_TYPE_EA_INFORMATION: u32 = 0xD0;
/// Extended attributes.
pub const ATTR_TYPE_EA: u32 = 0xE0;
/// Logged utility stream.
pub const ATTR_TYPE_LOGGED_UTILITY_STREAM: u32 = 0x100;
/// End marker.
pub const ATTR_TYPE_END: u32 = 0xFFFFFFFF;

// ---------------------------------------------------------------------------
// NTFS file attribute flags (FILE_ATTR_*)
// ---------------------------------------------------------------------------

/// Read-only file.
pub const FILE_ATTR_READONLY: u32 = 0x0001;
/// Hidden file.
pub const FILE_ATTR_HIDDEN: u32 = 0x0002;
/// System file.
pub const FILE_ATTR_SYSTEM: u32 = 0x0004;
/// Directory.
pub const FILE_ATTR_DIRECTORY: u32 = 0x0010;
/// Archive flag.
pub const FILE_ATTR_ARCHIVE: u32 = 0x0020;
/// Device.
pub const FILE_ATTR_DEVICE: u32 = 0x0040;
/// Normal file.
pub const FILE_ATTR_NORMAL: u32 = 0x0080;
/// Temporary file.
pub const FILE_ATTR_TEMPORARY: u32 = 0x0100;
/// Sparse file.
pub const FILE_ATTR_SPARSE_FILE: u32 = 0x0200;
/// Reparse point.
pub const FILE_ATTR_REPARSE_POINT: u32 = 0x0400;
/// Compressed.
pub const FILE_ATTR_COMPRESSED: u32 = 0x0800;
/// Offline.
pub const FILE_ATTR_OFFLINE: u32 = 0x1000;
/// Not content indexed.
pub const FILE_ATTR_NOT_CONTENT_INDEXED: u32 = 0x2000;
/// Encrypted.
pub const FILE_ATTR_ENCRYPTED: u32 = 0x4000;

// ---------------------------------------------------------------------------
// MFT system file numbers
// ---------------------------------------------------------------------------

/// $MFT file number.
pub const FILE_MFT: u64 = 0;
/// $MFTMirr file number.
pub const FILE_MFTMIRR: u64 = 1;
/// $LogFile file number.
pub const FILE_LOGFILE: u64 = 2;
/// $Volume file number.
pub const FILE_VOLUME: u64 = 3;
/// $AttrDef file number.
pub const FILE_ATTRDEF: u64 = 4;
/// Root directory (.) file number.
pub const FILE_ROOT: u64 = 5;
/// $Bitmap file number.
pub const FILE_BITMAP: u64 = 6;
/// $Boot file number.
pub const FILE_BOOT: u64 = 7;
/// $BadClus file number.
pub const FILE_BADCLUS: u64 = 8;
/// $Secure file number.
pub const FILE_SECURE: u64 = 9;
/// $UpCase file number.
pub const FILE_UPCASE: u64 = 10;
/// $Extend file number.
pub const FILE_EXTEND: u64 = 11;
/// First user file number.
pub const FILE_FIRST_USER: u64 = 24;

// ---------------------------------------------------------------------------
// NTFS record header magic numbers
// ---------------------------------------------------------------------------

/// FILE record signature ("FILE").
pub const NTFS_RECORD_MAGIC_FILE: u32 = 0x454C4946;
/// INDX record signature ("INDX").
pub const NTFS_RECORD_MAGIC_INDX: u32 = 0x58444E49;
/// RSTR record signature ("RSTR").
pub const NTFS_RECORD_MAGIC_RSTR: u32 = 0x52545352;
/// RCRD record signature ("RCRD").
pub const NTFS_RECORD_MAGIC_RCRD: u32 = 0x44524352;

// ---------------------------------------------------------------------------
// NTFS cluster sizes
// ---------------------------------------------------------------------------

/// Default cluster size (4 KiB).
pub const NTFS_DEFAULT_CLUSTER_SIZE: u32 = 4096;
/// Maximum cluster size (2 MiB).
pub const NTFS_MAX_CLUSTER_SIZE: u32 = 2 * 1024 * 1024;
/// Minimum cluster size (512 bytes).
pub const NTFS_MIN_CLUSTER_SIZE: u32 = 512;

// ---------------------------------------------------------------------------
// NTFS version numbers
// ---------------------------------------------------------------------------

/// NTFS major version 3 (Windows 2000+).
pub const NTFS_VERSION_MAJOR_3: u8 = 3;
/// NTFS minor version 1 (Windows XP/2003+).
pub const NTFS_VERSION_MINOR_1: u8 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attr_types_distinct() {
        let types = [
            ATTR_TYPE_STANDARD_INFORMATION,
            ATTR_TYPE_ATTRIBUTE_LIST,
            ATTR_TYPE_FILE_NAME,
            ATTR_TYPE_OBJECT_ID,
            ATTR_TYPE_SECURITY_DESCRIPTOR,
            ATTR_TYPE_VOLUME_NAME,
            ATTR_TYPE_VOLUME_INFORMATION,
            ATTR_TYPE_DATA,
            ATTR_TYPE_INDEX_ROOT,
            ATTR_TYPE_INDEX_ALLOCATION,
            ATTR_TYPE_BITMAP,
            ATTR_TYPE_REPARSE_POINT,
            ATTR_TYPE_EA_INFORMATION,
            ATTR_TYPE_EA,
            ATTR_TYPE_LOGGED_UTILITY_STREAM,
            ATTR_TYPE_END,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_attr_type_data_is_0x80() {
        assert_eq!(ATTR_TYPE_DATA, 0x80);
    }

    #[test]
    fn test_attr_type_end() {
        assert_eq!(ATTR_TYPE_END, 0xFFFFFFFF);
    }

    #[test]
    fn test_file_attrs_distinct() {
        let attrs = [
            FILE_ATTR_READONLY,
            FILE_ATTR_HIDDEN,
            FILE_ATTR_SYSTEM,
            FILE_ATTR_DIRECTORY,
            FILE_ATTR_ARCHIVE,
            FILE_ATTR_DEVICE,
            FILE_ATTR_NORMAL,
            FILE_ATTR_TEMPORARY,
            FILE_ATTR_SPARSE_FILE,
            FILE_ATTR_REPARSE_POINT,
            FILE_ATTR_COMPRESSED,
            FILE_ATTR_OFFLINE,
            FILE_ATTR_NOT_CONTENT_INDEXED,
            FILE_ATTR_ENCRYPTED,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_file_attrs_power_of_two() {
        let attrs = [
            FILE_ATTR_READONLY,
            FILE_ATTR_HIDDEN,
            FILE_ATTR_SYSTEM,
            FILE_ATTR_DIRECTORY,
            FILE_ATTR_ARCHIVE,
            FILE_ATTR_DEVICE,
            FILE_ATTR_NORMAL,
            FILE_ATTR_TEMPORARY,
            FILE_ATTR_SPARSE_FILE,
            FILE_ATTR_REPARSE_POINT,
            FILE_ATTR_COMPRESSED,
            FILE_ATTR_OFFLINE,
            FILE_ATTR_NOT_CONTENT_INDEXED,
            FILE_ATTR_ENCRYPTED,
        ];
        for a in &attrs {
            assert!(a.is_power_of_two(), "0x{:04x} not power of two", a);
        }
    }

    #[test]
    fn test_mft_system_files_sequential() {
        assert_eq!(FILE_MFT, 0);
        assert_eq!(FILE_MFTMIRR, 1);
        assert_eq!(FILE_LOGFILE, 2);
        assert_eq!(FILE_VOLUME, 3);
        assert_eq!(FILE_ATTRDEF, 4);
        assert_eq!(FILE_ROOT, 5);
        assert_eq!(FILE_BITMAP, 6);
        assert_eq!(FILE_BOOT, 7);
        assert_eq!(FILE_BADCLUS, 8);
        assert_eq!(FILE_SECURE, 9);
        assert_eq!(FILE_UPCASE, 10);
        assert_eq!(FILE_EXTEND, 11);
    }

    #[test]
    fn test_first_user_file() {
        assert_eq!(FILE_FIRST_USER, 24);
        assert!(FILE_FIRST_USER > FILE_EXTEND);
    }

    #[test]
    fn test_record_magic_values() {
        let magics = [
            NTFS_RECORD_MAGIC_FILE,
            NTFS_RECORD_MAGIC_INDX,
            NTFS_RECORD_MAGIC_RSTR,
            NTFS_RECORD_MAGIC_RCRD,
        ];
        for i in 0..magics.len() {
            for j in (i + 1)..magics.len() {
                assert_ne!(magics[i], magics[j]);
            }
        }
    }

    #[test]
    fn test_cluster_size_ordering() {
        assert!(NTFS_MIN_CLUSTER_SIZE < NTFS_DEFAULT_CLUSTER_SIZE);
        assert!(NTFS_DEFAULT_CLUSTER_SIZE < NTFS_MAX_CLUSTER_SIZE);
    }

    #[test]
    fn test_cluster_sizes_power_of_two() {
        assert!(NTFS_MIN_CLUSTER_SIZE.is_power_of_two());
        assert!(NTFS_DEFAULT_CLUSTER_SIZE.is_power_of_two());
        assert!(NTFS_MAX_CLUSTER_SIZE.is_power_of_two());
    }
}

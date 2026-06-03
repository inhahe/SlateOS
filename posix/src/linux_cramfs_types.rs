//! `<linux/cramfs_fs.h>` — Cramfs (Compressed ROM filesystem) constants.
//!
//! Cramfs is a simple compressed read-only filesystem for
//! embedded use. These constants define magic numbers,
//! flags, and limits.

// ---------------------------------------------------------------------------
// Magic numbers
// ---------------------------------------------------------------------------

/// Cramfs magic number.
pub const CRAMFS_MAGIC: u32 = 0x28CD3D45;
/// Cramfs magic (byte-swapped).
pub const CRAMFS_MAGIC_WEND: u32 = 0x453DCD28;

// ---------------------------------------------------------------------------
// Superblock flags
// ---------------------------------------------------------------------------

/// Contains a CRC.
pub const CRAMFS_FLAG_FSID_VERSION_2: u32 = 0x00000001;
/// Sorted directories.
pub const CRAMFS_FLAG_SORTED_DIRS: u32 = 0x00000002;
/// Has holes.
pub const CRAMFS_FLAG_HOLES: u32 = 0x00000100;
/// Wrong signature.
pub const CRAMFS_FLAG_WRONG_SIGNATURE: u32 = 0x00000200;
/// Shifted root offset.
pub const CRAMFS_FLAG_SHIFTED_ROOT_OFFSET: u32 = 0x00000400;
/// Extended block pointers.
pub const CRAMFS_FLAG_EXT_BLOCK_POINTERS: u32 = 0x00000800;

// ---------------------------------------------------------------------------
// Supported flags mask
// ---------------------------------------------------------------------------

/// All supported flags.
pub const CRAMFS_SUPPORTED_FLAGS: u32 = CRAMFS_FLAG_FSID_VERSION_2
    | CRAMFS_FLAG_SORTED_DIRS
    | CRAMFS_FLAG_HOLES
    | CRAMFS_FLAG_WRONG_SIGNATURE
    | CRAMFS_FLAG_SHIFTED_ROOT_OFFSET
    | CRAMFS_FLAG_EXT_BLOCK_POINTERS;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum file size.
pub const CRAMFS_MAXSIZE: u32 = 16777216;
/// Path name length limit.
pub const CRAMFS_PATH_MAX: u32 = 256;
/// Maximum file name length.
pub const CRAMFS_MAXNAMELEN: u32 = 255;
/// Block size.
pub const CRAMFS_BLK_SIZE: u32 = 4096;
/// Block size shift.
pub const CRAMFS_BLK_SHIFT: u32 = 12;

// ---------------------------------------------------------------------------
// Inode constants
// ---------------------------------------------------------------------------

/// Offset width in inode (26 bits).
pub const CRAMFS_OFFSET_WIDTH: u32 = 26;
/// Offset mask.
pub const CRAMFS_OFFSET_MASK: u32 = (1 << CRAMFS_OFFSET_WIDTH) - 1;
/// Size width in inode (24 bits).
pub const CRAMFS_SIZE_WIDTH: u32 = 24;
/// Size mask.
pub const CRAMFS_SIZE_MASK: u32 = (1 << CRAMFS_SIZE_WIDTH) - 1;
/// NameLen width (6 bits).
pub const CRAMFS_NAMELEN_WIDTH: u32 = 6;

// ---------------------------------------------------------------------------
// CRC
// ---------------------------------------------------------------------------

/// CRC poly.
pub const CRAMFS_CRC_POLY: u32 = 0xEDB88320;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic() {
        assert_eq!(CRAMFS_MAGIC, 0x28CD3D45);
    }

    #[test]
    fn test_magic_wend() {
        assert_eq!(CRAMFS_MAGIC_WEND, 0x453DCD28);
        // Byte-swapped version
        let swapped = CRAMFS_MAGIC.swap_bytes();
        assert_eq!(swapped, CRAMFS_MAGIC_WEND);
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            CRAMFS_FLAG_FSID_VERSION_2,
            CRAMFS_FLAG_SORTED_DIRS,
            CRAMFS_FLAG_HOLES,
            CRAMFS_FLAG_WRONG_SIGNATURE,
            CRAMFS_FLAG_SHIFTED_ROOT_OFFSET,
            CRAMFS_FLAG_EXT_BLOCK_POINTERS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_supported_flags() {
        assert_ne!(CRAMFS_SUPPORTED_FLAGS & CRAMFS_FLAG_FSID_VERSION_2, 0);
        assert_ne!(CRAMFS_SUPPORTED_FLAGS & CRAMFS_FLAG_EXT_BLOCK_POINTERS, 0);
    }

    #[test]
    fn test_block_size() {
        assert_eq!(CRAMFS_BLK_SIZE, 4096);
        assert!(CRAMFS_BLK_SIZE.is_power_of_two());
        assert_eq!(1u32 << CRAMFS_BLK_SHIFT, CRAMFS_BLK_SIZE);
    }

    #[test]
    fn test_maxsize() {
        assert_eq!(CRAMFS_MAXSIZE, 16 * 1024 * 1024);
    }

    #[test]
    fn test_offset_mask() {
        assert_eq!(CRAMFS_OFFSET_MASK, (1 << 26) - 1);
    }

    #[test]
    fn test_size_mask() {
        assert_eq!(CRAMFS_SIZE_MASK, (1 << 24) - 1);
    }

    #[test]
    fn test_crc_poly() {
        assert_eq!(CRAMFS_CRC_POLY, 0xEDB88320);
    }
}

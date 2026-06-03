//! `<linux/fiemap.h>` — file extent mapping.
//!
//! Provides structures and constants for the FIEMAP ioctl, which
//! maps logical file offsets to physical block locations.

pub use crate::linux_fs::FS_IOC_FIEMAP;

// ---------------------------------------------------------------------------
// FIEMAP flags
// ---------------------------------------------------------------------------

/// Sync file data before mapping.
pub const FIEMAP_FLAG_SYNC: u32 = 0x0001;
/// Request extent data (vs. just mapping).
pub const FIEMAP_FLAG_XATTR: u32 = 0x0002;
/// Cache results.
pub const FIEMAP_FLAG_CACHE: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Extent flags
// ---------------------------------------------------------------------------

/// This is the last extent in the file.
pub const FIEMAP_EXTENT_LAST: u32 = 0x0001;
/// Extent is unknown (could be a hole).
pub const FIEMAP_EXTENT_UNKNOWN: u32 = 0x0002;
/// Extent is encoded (encrypted/compressed).
pub const FIEMAP_EXTENT_DELALLOC: u32 = 0x0004;
/// Extent data is encrypted.
pub const FIEMAP_EXTENT_ENCODED: u32 = 0x0008;
/// Extent data is encrypted.
pub const FIEMAP_EXTENT_DATA_ENCRYPTED: u32 = 0x0080;
/// Extent not yet allocated.
pub const FIEMAP_EXTENT_NOT_ALIGNED: u32 = 0x0100;
/// Extent is inline data.
pub const FIEMAP_EXTENT_DATA_INLINE: u32 = 0x0200;
/// Extent data is stored in block tail.
pub const FIEMAP_EXTENT_DATA_TAIL: u32 = 0x0400;
/// Extent is unwritten (allocated but not initialized).
pub const FIEMAP_EXTENT_UNWRITTEN: u32 = 0x0800;
/// Extent is merged from multiple file blocks.
pub const FIEMAP_EXTENT_MERGED: u32 = 0x1000;
/// Extent is shared with other files.
pub const FIEMAP_EXTENT_SHARED: u32 = 0x2000;

// ---------------------------------------------------------------------------
// Fiemap structs
// ---------------------------------------------------------------------------

/// Single extent mapping.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FiemapExtent {
    /// Logical offset in file (bytes).
    pub fe_logical: u64,
    /// Physical offset on device (bytes).
    pub fe_physical: u64,
    /// Length of extent (bytes).
    pub fe_length: u64,
    /// Reserved.
    pub fe_reserved64: [u64; 2],
    /// Flags (FIEMAP_EXTENT_*).
    pub fe_flags: u32,
    /// Reserved.
    pub fe_reserved: [u32; 3],
}

/// Fiemap request header.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Fiemap {
    /// Logical offset to start mapping (bytes).
    pub fm_start: u64,
    /// Logical length to map (bytes).
    pub fm_length: u64,
    /// Request flags (FIEMAP_FLAG_*).
    pub fm_flags: u32,
    /// Number of mapped extents (output).
    pub fm_mapped_extents: u32,
    /// Size of the fm_extents array.
    pub fm_extent_count: u32,
    /// Reserved.
    pub fm_reserved: u32,
    // Followed by `fm_extent_count` FiemapExtent entries.
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fiemap_extent_size() {
        // 8*5 + 4*4 = 56 bytes.
        assert_eq!(core::mem::size_of::<FiemapExtent>(), 56);
    }

    #[test]
    fn test_fiemap_header_size() {
        // 8 + 8 + 4 + 4 + 4 + 4 = 32 bytes.
        assert_eq!(core::mem::size_of::<Fiemap>(), 32);
    }

    #[test]
    fn test_flag_values() {
        assert_eq!(FIEMAP_FLAG_SYNC, 1);
        assert_eq!(FIEMAP_FLAG_XATTR, 2);
    }

    #[test]
    fn test_extent_flags_last() {
        assert_eq!(FIEMAP_EXTENT_LAST, 1);
    }

    #[test]
    fn test_extent_flags_are_bits() {
        let flags = [
            FIEMAP_EXTENT_LAST,
            FIEMAP_EXTENT_UNKNOWN,
            FIEMAP_EXTENT_DELALLOC,
            FIEMAP_EXTENT_ENCODED,
            FIEMAP_EXTENT_DATA_ENCRYPTED,
            FIEMAP_EXTENT_NOT_ALIGNED,
            FIEMAP_EXTENT_DATA_INLINE,
            FIEMAP_EXTENT_DATA_TAIL,
            FIEMAP_EXTENT_UNWRITTEN,
            FIEMAP_EXTENT_MERGED,
            FIEMAP_EXTENT_SHARED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "Extent flags must not overlap");
            }
        }
    }
}

//! `<linux/fiemap.h>` — File extent mapping (FIEMAP) constants.
//!
//! The FIEMAP ioctl maps logical file offsets to physical disk
//! locations. It returns a list of extents describing where each
//! portion of a file is stored on disk. Used by defragmentation
//! tools, backup programs, and filesystem analysis utilities.
//! Each extent has flags indicating its state (written, unwritten,
//! shared, encrypted, inline, etc.).

// ---------------------------------------------------------------------------
// FIEMAP ioctl number
// ---------------------------------------------------------------------------

/// The FIEMAP ioctl command.
pub const FS_IOC_FIEMAP: u32 = 0xC020660B;

// ---------------------------------------------------------------------------
// FIEMAP flags (input to ioctl)
// ---------------------------------------------------------------------------

/// Sync file data before mapping (ensure extents are up-to-date).
pub const FIEMAP_FLAG_SYNC: u32 = 0x0000_0001;
/// Request extent info for xattr data (not file data).
pub const FIEMAP_FLAG_XATTR: u32 = 0x0000_0002;
/// Return extents in cached/memory state (not necessarily on-disk).
pub const FIEMAP_FLAG_CACHE: u32 = 0x0000_0004;

// ---------------------------------------------------------------------------
// FIEMAP extent flags (per-extent attributes)
// ---------------------------------------------------------------------------

/// This is the last extent in the file.
pub const FIEMAP_EXTENT_LAST: u32 = 0x0000_0001;
/// Extent spans an unknown physical location (e.g., pre-allocation).
pub const FIEMAP_EXTENT_UNKNOWN: u32 = 0x0000_0002;
/// Extent is delayed allocation (not yet written to disk).
pub const FIEMAP_EXTENT_DELALLOC: u32 = 0x0000_0004;
/// Extent data is encoded/encrypted on disk.
pub const FIEMAP_EXTENT_ENCODED: u32 = 0x0000_0008;
/// Extent data is encrypted.
pub const FIEMAP_EXTENT_DATA_ENCRYPTED: u32 = 0x0000_0080;
/// Extent is not backed by any blocks (hole/sparse).
pub const FIEMAP_EXTENT_NOT_ALIGNED: u32 = 0x0000_0100;
/// Extent is stored inline in the inode.
pub const FIEMAP_EXTENT_DATA_INLINE: u32 = 0x0000_0200;
/// Extent is in tail-packing region.
pub const FIEMAP_EXTENT_DATA_TAIL: u32 = 0x0000_0400;
/// Extent is unwritten (allocated but no data written yet).
pub const FIEMAP_EXTENT_UNWRITTEN: u32 = 0x0000_0800;
/// Extent is merged (adjacent extents combined for reporting).
pub const FIEMAP_EXTENT_MERGED: u32 = 0x0000_1000;
/// Extent is shared with other files (reflink/dedup).
pub const FIEMAP_EXTENT_SHARED: u32 = 0x0000_2000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fiemap_flags_no_overlap() {
        let flags = [FIEMAP_FLAG_SYNC, FIEMAP_FLAG_XATTR, FIEMAP_FLAG_CACHE];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_extent_flags_distinct() {
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
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_extent_flags_are_bitmask() {
        // All extent flags should be non-overlapping bits
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
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

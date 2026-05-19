//! `<linux/fiemap.h>` — Additional FIEMAP constants.
//!
//! Supplementary FIEMAP constants covering extent flags,
//! request flags, and special ranges.

// ---------------------------------------------------------------------------
// FIEMAP extent flags
// ---------------------------------------------------------------------------

/// Last extent in file.
pub const FIEMAP_EXTENT_LAST: u32 = 0x00000001;
/// Data unknown (e.g., encrypted).
pub const FIEMAP_EXTENT_UNKNOWN: u32 = 0x00000002;
/// Delayed allocation.
pub const FIEMAP_EXTENT_DELALLOC: u32 = 0x00000004;
/// Data encrypted.
pub const FIEMAP_EXTENT_ENCODED: u32 = 0x00000008;
/// Data encrypted.
pub const FIEMAP_EXTENT_DATA_ENCRYPTED: u32 = 0x00000080;
/// Not aligned to block.
pub const FIEMAP_EXTENT_NOT_ALIGNED: u32 = 0x00000100;
/// Data inline.
pub const FIEMAP_EXTENT_DATA_INLINE: u32 = 0x00000200;
/// Data in tail packing.
pub const FIEMAP_EXTENT_DATA_TAIL: u32 = 0x00000400;
/// Unwritten extent.
pub const FIEMAP_EXTENT_UNWRITTEN: u32 = 0x00000800;
/// Merged for efficiency.
pub const FIEMAP_EXTENT_MERGED: u32 = 0x00001000;
/// Shared extent.
pub const FIEMAP_EXTENT_SHARED: u32 = 0x00002000;

// ---------------------------------------------------------------------------
// FIEMAP request flags
// ---------------------------------------------------------------------------

/// Sync before mapping.
pub const FIEMAP_FLAG_SYNC: u32 = 0x00000001;
/// Request extent data too.
pub const FIEMAP_FLAG_XATTR: u32 = 0x00000002;
/// Cache control.
pub const FIEMAP_FLAG_CACHE: u32 = 0x00000004;

// ---------------------------------------------------------------------------
// FIEMAP ioctl
// ---------------------------------------------------------------------------

/// FIEMAP ioctl command.
pub const FS_IOC_FIEMAP: u32 = 0xC020660B;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extent_flags_power_of_two() {
        let flags = [
            FIEMAP_EXTENT_LAST, FIEMAP_EXTENT_UNKNOWN,
            FIEMAP_EXTENT_DELALLOC, FIEMAP_EXTENT_ENCODED,
            FIEMAP_EXTENT_DATA_ENCRYPTED, FIEMAP_EXTENT_NOT_ALIGNED,
            FIEMAP_EXTENT_DATA_INLINE, FIEMAP_EXTENT_DATA_TAIL,
            FIEMAP_EXTENT_UNWRITTEN, FIEMAP_EXTENT_MERGED,
            FIEMAP_EXTENT_SHARED,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_extent_flags_no_overlap() {
        let flags = [
            FIEMAP_EXTENT_LAST, FIEMAP_EXTENT_UNKNOWN,
            FIEMAP_EXTENT_DELALLOC, FIEMAP_EXTENT_ENCODED,
            FIEMAP_EXTENT_DATA_ENCRYPTED, FIEMAP_EXTENT_NOT_ALIGNED,
            FIEMAP_EXTENT_DATA_INLINE, FIEMAP_EXTENT_DATA_TAIL,
            FIEMAP_EXTENT_UNWRITTEN, FIEMAP_EXTENT_MERGED,
            FIEMAP_EXTENT_SHARED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_request_flags_no_overlap() {
        let flags = [FIEMAP_FLAG_SYNC, FIEMAP_FLAG_XATTR, FIEMAP_FLAG_CACHE];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

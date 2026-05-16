//! `<linux/falloc.h>` — File allocation constants.
//!
//! fallocate(2) preallocates or manipulates disk space for a file.
//! It can allocate blocks without writing data, punch holes to
//! release space, collapse ranges to remove data, or insert
//! zero-filled space.

// ---------------------------------------------------------------------------
// fallocate mode flags
// ---------------------------------------------------------------------------

/// Default allocation (preallocate, extend file if needed).
pub const FALLOC_FL_DEFAULT: u32 = 0;

/// Keep file size unchanged after allocation.
pub const FALLOC_FL_KEEP_SIZE: u32 = 0x01;

/// Punch a hole (deallocate blocks within the range).
pub const FALLOC_FL_PUNCH_HOLE: u32 = 0x02;

/// Don't hide stale data (expose unwritten extents — dangerous).
/// Requires CAP_SYS_RAWIO.
pub const FALLOC_FL_NO_HIDE_STALE: u32 = 0x04;

/// Collapse range (remove data, shift subsequent data down).
pub const FALLOC_FL_COLLAPSE_RANGE: u32 = 0x08;

/// Zero range (convert data blocks to unwritten extents).
pub const FALLOC_FL_ZERO_RANGE: u32 = 0x10;

/// Insert range (insert hole, shift subsequent data up).
pub const FALLOC_FL_INSERT_RANGE: u32 = 0x20;

/// Unshare range (ensure data is not shared/reflinked).
pub const FALLOC_FL_UNSHARE_RANGE: u32 = 0x40;

// ---------------------------------------------------------------------------
// Combined flag validation
// ---------------------------------------------------------------------------

/// Mask of all valid fallocate flags.
pub const FALLOC_FL_SUPPORTED_MASK: u32 =
    FALLOC_FL_KEEP_SIZE | FALLOC_FL_PUNCH_HOLE
    | FALLOC_FL_NO_HIDE_STALE | FALLOC_FL_COLLAPSE_RANGE
    | FALLOC_FL_ZERO_RANGE | FALLOC_FL_INSERT_RANGE
    | FALLOC_FL_UNSHARE_RANGE;

// ---------------------------------------------------------------------------
// Mutually exclusive operations
// ---------------------------------------------------------------------------

/// PUNCH_HOLE requires KEEP_SIZE (can't shrink file by punching).
pub const FALLOC_FL_PUNCH_REQUIRED: u32 = FALLOC_FL_KEEP_SIZE;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE,
            FALLOC_FL_NO_HIDE_STALE, FALLOC_FL_COLLAPSE_RANGE,
            FALLOC_FL_ZERO_RANGE, FALLOC_FL_INSERT_RANGE,
            FALLOC_FL_UNSHARE_RANGE,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE,
            FALLOC_FL_NO_HIDE_STALE, FALLOC_FL_COLLAPSE_RANGE,
            FALLOC_FL_ZERO_RANGE, FALLOC_FL_INSERT_RANGE,
            FALLOC_FL_UNSHARE_RANGE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_default_is_zero() {
        assert_eq!(FALLOC_FL_DEFAULT, 0);
    }

    #[test]
    fn test_supported_mask() {
        assert_eq!(FALLOC_FL_SUPPORTED_MASK, 0x7F);
    }

    #[test]
    fn test_punch_requires_keep_size() {
        assert_eq!(FALLOC_FL_PUNCH_REQUIRED, FALLOC_FL_KEEP_SIZE);
    }
}

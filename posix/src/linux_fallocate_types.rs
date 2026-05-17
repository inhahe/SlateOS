//! `<linux/falloc.h>` — fallocate() mode flag constants.
//!
//! fallocate() manipulates the allocated disk space for a file
//! without changing its visible size. It can pre-allocate space
//! (preventing ENOSPC on later writes), punch holes (deallocate
//! ranges), zero-fill ranges, collapse/insert ranges, and unshare
//! shared extents. Essential for databases, VMs, and any application
//! that needs guaranteed disk space.

// ---------------------------------------------------------------------------
// fallocate mode flags
// ---------------------------------------------------------------------------

/// Default: allocate space (no mode flag needed, but use 0).
pub const FALLOC_FL_ALLOCATE: u32 = 0;
/// Keep file size unchanged (allocate beyond EOF without extending).
pub const FALLOC_FL_KEEP_SIZE: u32 = 0x01;
/// Deallocate range (punch hole). Requires KEEP_SIZE.
pub const FALLOC_FL_PUNCH_HOLE: u32 = 0x02;
/// Remove a range without leaving a hole (collapse range, shift data).
pub const FALLOC_FL_COLLAPSE_RANGE: u32 = 0x08;
/// Zero-fill range (allocated but reads as zeros).
pub const FALLOC_FL_ZERO_RANGE: u32 = 0x10;
/// Insert space at offset (shift data right).
pub const FALLOC_FL_INSERT_RANGE: u32 = 0x20;
/// Unshare shared extents (break CoW).
pub const FALLOC_FL_UNSHARE_RANGE: u32 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_flags_distinct() {
        let flags = [
            FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE,
            FALLOC_FL_COLLAPSE_RANGE, FALLOC_FL_ZERO_RANGE,
            FALLOC_FL_INSERT_RANGE, FALLOC_FL_UNSHARE_RANGE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_nonzero_flags_no_overlap() {
        let flags = [
            FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE,
            FALLOC_FL_COLLAPSE_RANGE, FALLOC_FL_ZERO_RANGE,
            FALLOC_FL_INSERT_RANGE, FALLOC_FL_UNSHARE_RANGE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_allocate_is_zero() {
        assert_eq!(FALLOC_FL_ALLOCATE, 0);
    }
}

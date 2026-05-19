//! `<linux/falloc.h>` — Additional fallocate constants.
//!
//! Supplementary fallocate constants covering allocation modes
//! and range operation flags.

// ---------------------------------------------------------------------------
// fallocate mode flags
// ---------------------------------------------------------------------------

/// Allocate disk space without zeroing.
pub const FALLOC_FL_KEEP_SIZE: u32 = 0x01;
/// Punch a hole (deallocate).
pub const FALLOC_FL_PUNCH_HOLE: u32 = 0x02;
/// Indicate no-hide-stale.
pub const FALLOC_FL_NO_HIDE_STALE: u32 = 0x04;
/// Collapse range (remove space).
pub const FALLOC_FL_COLLAPSE_RANGE: u32 = 0x08;
/// Zero range (write zeros).
pub const FALLOC_FL_ZERO_RANGE: u32 = 0x10;
/// Insert range (shift data).
pub const FALLOC_FL_INSERT_RANGE: u32 = 0x20;
/// Unshare range (break COW).
pub const FALLOC_FL_UNSHARE_RANGE: u32 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE,
            FALLOC_FL_NO_HIDE_STALE, FALLOC_FL_COLLAPSE_RANGE,
            FALLOC_FL_ZERO_RANGE, FALLOC_FL_INSERT_RANGE,
            FALLOC_FL_UNSHARE_RANGE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
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
}

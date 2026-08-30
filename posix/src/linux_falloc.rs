//! `<linux/falloc.h>` — fallocate flags.
//!
//! Re-exports FALLOC_FL_* constants and `fallocate()` from the
//! `file` module.

pub use crate::file::FALLOC_FL_COLLAPSE_RANGE;
pub use crate::file::FALLOC_FL_INSERT_RANGE;
pub use crate::file::FALLOC_FL_KEEP_SIZE;
pub use crate::file::FALLOC_FL_PUNCH_HOLE;
pub use crate::file::FALLOC_FL_UNSHARE_RANGE;
pub use crate::file::FALLOC_FL_ZERO_RANGE;
pub use crate::file::fallocate;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_falloc_flags_are_bits() {
        let flags = [
            FALLOC_FL_KEEP_SIZE,
            FALLOC_FL_PUNCH_HOLE,
            FALLOC_FL_COLLAPSE_RANGE,
            FALLOC_FL_ZERO_RANGE,
            FALLOC_FL_INSERT_RANGE,
            FALLOC_FL_UNSHARE_RANGE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "FALLOC_FL_ flags must not overlap");
            }
        }
    }

    #[test]
    fn test_falloc_values() {
        assert_eq!(FALLOC_FL_KEEP_SIZE, 0x01);
        assert_eq!(FALLOC_FL_PUNCH_HOLE, 0x02);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(FALLOC_FL_KEEP_SIZE, crate::file::FALLOC_FL_KEEP_SIZE);
    }
}

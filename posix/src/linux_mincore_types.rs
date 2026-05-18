//! `<sys/mman.h>` — mincore() result flag constants.
//!
//! The `mincore()` syscall reports which pages in a virtual memory
//! range are currently resident in physical memory. The kernel writes
//! a vector of bytes, one per page, with flags indicating residency
//! status.

// ---------------------------------------------------------------------------
// mincore() result flags (per-page byte)
// ---------------------------------------------------------------------------

/// Page is currently in RAM.
pub const MINCORE_INCORE: u8 = 0x01;
/// Page is referenced (accessed recently).
pub const MINCORE_REFERENCED: u8 = 0x02;
/// Page has been modified (dirty).
pub const MINCORE_MODIFIED: u8 = 0x04;
/// Page is referenced by another mapping.
pub const MINCORE_REFERENCED_OTHER: u8 = 0x08;
/// Page is modified by another mapping.
pub const MINCORE_MODIFIED_OTHER: u8 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mincore_flags_no_overlap() {
        let flags = [
            MINCORE_INCORE, MINCORE_REFERENCED, MINCORE_MODIFIED,
            MINCORE_REFERENCED_OTHER, MINCORE_MODIFIED_OTHER,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_mincore_power_of_two() {
        assert!(MINCORE_INCORE.is_power_of_two());
        assert!(MINCORE_REFERENCED.is_power_of_two());
        assert!(MINCORE_MODIFIED.is_power_of_two());
        assert!(MINCORE_REFERENCED_OTHER.is_power_of_two());
        assert!(MINCORE_MODIFIED_OTHER.is_power_of_two());
    }

    #[test]
    fn test_mincore_incore_is_bit0() {
        assert_eq!(MINCORE_INCORE, 1);
    }

    #[test]
    fn test_mincore_values() {
        assert_eq!(MINCORE_INCORE, 0x01);
        assert_eq!(MINCORE_REFERENCED, 0x02);
        assert_eq!(MINCORE_MODIFIED, 0x04);
        assert_eq!(MINCORE_REFERENCED_OTHER, 0x08);
        assert_eq!(MINCORE_MODIFIED_OTHER, 0x10);
    }
}

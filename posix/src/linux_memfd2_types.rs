//! `<linux/memfd.h>` — Additional memfd constants.
//!
//! Supplementary memfd constants covering create flags,
//! sealing operations, and hugetlb size encoding.

// ---------------------------------------------------------------------------
// memfd_create flags (MFD_*)
// ---------------------------------------------------------------------------

/// Set close-on-exec.
pub const MFD_CLOEXEC: u32 = 0x0001;
/// Allow sealing.
pub const MFD_ALLOW_SEALING: u32 = 0x0002;
/// Use huge pages.
pub const MFD_HUGETLB: u32 = 0x0004;
/// Don't allow exec (W^X).
pub const MFD_NOEXEC_SEAL: u32 = 0x0008;
/// Allow exec.
pub const MFD_EXEC: u32 = 0x0010;

// ---------------------------------------------------------------------------
// File seals (F_SEAL_*)
// ---------------------------------------------------------------------------

/// Prevent further sealing.
pub const F_SEAL_SEAL: u32 = 0x0001;
/// Prevent shrinking.
pub const F_SEAL_SHRINK: u32 = 0x0002;
/// Prevent growing.
pub const F_SEAL_GROW: u32 = 0x0004;
/// Prevent writes.
pub const F_SEAL_WRITE: u32 = 0x0008;
/// Prevent future writes (shared mappings still work).
pub const F_SEAL_FUTURE_WRITE: u32 = 0x0010;
/// Prevent exec seal changes.
pub const F_SEAL_EXEC: u32 = 0x0020;

// ---------------------------------------------------------------------------
// Hugetlb size encoding shift
// ---------------------------------------------------------------------------

/// Shift for encoding huge page size in memfd flags.
pub const MFD_HUGE_SHIFT: u32 = 26;
/// Mask for huge page size encoding.
pub const MFD_HUGE_MASK: u32 = 0x3F;

/// 2 MiB huge pages.
pub const MFD_HUGE_2MB: u32 = 21 << 26;
/// 1 GiB huge pages.
pub const MFD_HUGE_1GB: u32 = 30 << 26;
/// 512 MiB huge pages (arm64).
pub const MFD_HUGE_512MB: u32 = 29 << 26;
/// 16 MiB huge pages (arm64).
pub const MFD_HUGE_16MB: u32 = 24 << 26;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_flags_distinct() {
        let flags = [
            MFD_CLOEXEC, MFD_ALLOW_SEALING, MFD_HUGETLB,
            MFD_NOEXEC_SEAL, MFD_EXEC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_create_flags_no_overlap() {
        let flags = [
            MFD_CLOEXEC, MFD_ALLOW_SEALING, MFD_HUGETLB,
            MFD_NOEXEC_SEAL, MFD_EXEC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_seal_flags_power_of_two() {
        let seals = [
            F_SEAL_SEAL, F_SEAL_SHRINK, F_SEAL_GROW,
            F_SEAL_WRITE, F_SEAL_FUTURE_WRITE, F_SEAL_EXEC,
        ];
        for s in &seals {
            assert!(s.is_power_of_two(), "0x{:04x} not power of two", s);
        }
    }

    #[test]
    fn test_seal_flags_no_overlap() {
        let seals = [
            F_SEAL_SEAL, F_SEAL_SHRINK, F_SEAL_GROW,
            F_SEAL_WRITE, F_SEAL_FUTURE_WRITE, F_SEAL_EXEC,
        ];
        for i in 0..seals.len() {
            for j in (i + 1)..seals.len() {
                assert_eq!(seals[i] & seals[j], 0);
            }
        }
    }

    #[test]
    fn test_huge_sizes_distinct() {
        let sizes = [MFD_HUGE_2MB, MFD_HUGE_1GB, MFD_HUGE_512MB, MFD_HUGE_16MB];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_huge_encoding() {
        // 2MB = 2^21, so shift value is 21
        assert_eq!(MFD_HUGE_2MB >> MFD_HUGE_SHIFT, 21);
        // 1GB = 2^30
        assert_eq!(MFD_HUGE_1GB >> MFD_HUGE_SHIFT, 30);
    }
}

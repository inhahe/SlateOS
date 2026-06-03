//! `<linux/fadvise.h>` — Additional fadvise constants.
//!
//! Supplementary fadvise constants covering file access patterns,
//! readahead configuration, and related fcntl hints.

// ---------------------------------------------------------------------------
// Posix fadvise advice values (POSIX_FADV_*)
// ---------------------------------------------------------------------------

/// Normal access pattern (default).
pub const POSIX_FADV_NORMAL: u32 = 0;
/// Random access pattern.
pub const POSIX_FADV_RANDOM: u32 = 1;
/// Sequential access pattern.
pub const POSIX_FADV_SEQUENTIAL: u32 = 2;
/// Data will be accessed soon.
pub const POSIX_FADV_WILLNEED: u32 = 3;
/// Data will not be accessed soon.
pub const POSIX_FADV_DONTNEED: u32 = 4;
/// Data will be accessed only once.
pub const POSIX_FADV_NOREUSE: u32 = 5;

// ---------------------------------------------------------------------------
// Readahead configuration
// ---------------------------------------------------------------------------

/// Default readahead size (128 KiB, in pages with 4K page size = 32 pages).
pub const READAHEAD_DEFAULT_PAGES: u32 = 32;
/// Maximum readahead size (pages).
pub const READAHEAD_MAX_PAGES: u32 = 256;
/// Minimum readahead size (pages).
pub const READAHEAD_MIN_PAGES: u32 = 4;

// ---------------------------------------------------------------------------
// File access pattern hints (RWF_* for preadv2/pwritev2)
// ---------------------------------------------------------------------------

/// High priority I/O.
pub const RWF_HIPRI: u32 = 0x00000001;
/// Data sync I/O.
pub const RWF_DSYNC: u32 = 0x00000002;
/// File sync I/O.
pub const RWF_SYNC: u32 = 0x00000004;
/// Non-blocking I/O.
pub const RWF_NOWAIT: u32 = 0x00000008;
/// Append mode.
pub const RWF_APPEND: u32 = 0x00000010;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fadvise_values_distinct() {
        let vals = [
            POSIX_FADV_NORMAL,
            POSIX_FADV_RANDOM,
            POSIX_FADV_SEQUENTIAL,
            POSIX_FADV_WILLNEED,
            POSIX_FADV_DONTNEED,
            POSIX_FADV_NOREUSE,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_fadvise_normal_is_zero() {
        assert_eq!(POSIX_FADV_NORMAL, 0);
    }

    #[test]
    fn test_readahead_ordering() {
        assert!(READAHEAD_MIN_PAGES < READAHEAD_DEFAULT_PAGES);
        assert!(READAHEAD_DEFAULT_PAGES < READAHEAD_MAX_PAGES);
    }

    #[test]
    fn test_rwf_flags_power_of_two() {
        let flags = [RWF_HIPRI, RWF_DSYNC, RWF_SYNC, RWF_NOWAIT, RWF_APPEND];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_rwf_flags_no_overlap() {
        let flags = [RWF_HIPRI, RWF_DSYNC, RWF_SYNC, RWF_NOWAIT, RWF_APPEND];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

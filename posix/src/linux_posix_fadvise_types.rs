//! `<fcntl.h>` — posix_fadvise() and posix_fallocate() constants.
//!
//! `posix_fadvise()` advises the kernel about expected file access
//! patterns so it can optimize readahead and caching.
//! `posix_fallocate()` preallocates file space.  These constants
//! define the advice values and allocation modes.

// ---------------------------------------------------------------------------
// posix_fadvise() advice values
// ---------------------------------------------------------------------------

/// No specific advice (default behaviour).
pub const POSIX_FADV_NORMAL: u32 = 0;
/// Expect random access; disable readahead.
pub const POSIX_FADV_RANDOM: u32 = 1;
/// Expect sequential access; increase readahead.
pub const POSIX_FADV_SEQUENTIAL: u32 = 2;
/// Data will be accessed soon; initiate readahead.
pub const POSIX_FADV_WILLNEED: u32 = 3;
/// Data will not be accessed soon; free page cache.
pub const POSIX_FADV_DONTNEED: u32 = 4;
/// Data will be accessed once; don't keep in cache.
pub const POSIX_FADV_NOREUSE: u32 = 5;

// ---------------------------------------------------------------------------
// posix_fallocate() / fallocate() mode flags
// ---------------------------------------------------------------------------

/// Default allocation mode (allocate space, zero-fill).
pub const FALLOC_FL_DEFAULT: u32 = 0;
/// Keep file size unchanged (allocate beyond EOF).
pub const FALLOC_FL_KEEP_SIZE: u32 = 0x01;
/// Punch a hole (deallocate space).
pub const FALLOC_FL_PUNCH_HOLE: u32 = 0x02;
/// Remove a range without leaving a hole.
pub const FALLOC_FL_COLLAPSE_RANGE: u32 = 0x08;
/// Convert range to zeros without deallocating.
pub const FALLOC_FL_ZERO_RANGE: u32 = 0x10;
/// Insert space at the given offset, shifting existing data.
pub const FALLOC_FL_INSERT_RANGE: u32 = 0x20;
/// Unshare shared extents (break copy-on-write).
pub const FALLOC_FL_UNSHARE_RANGE: u32 = 0x40;

// ---------------------------------------------------------------------------
// posix_madvise() advice values (complement to fadvise)
// ---------------------------------------------------------------------------

/// No specific advice.
pub const POSIX_MADV_NORMAL: u32 = 0;
/// Expect random access.
pub const POSIX_MADV_RANDOM: u32 = 1;
/// Expect sequential access.
pub const POSIX_MADV_SEQUENTIAL: u32 = 2;
/// Data will be accessed soon.
pub const POSIX_MADV_WILLNEED: u32 = 3;
/// Data will not be accessed soon.
pub const POSIX_MADV_DONTNEED: u32 = 4;

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
    fn test_normal_is_zero() {
        assert_eq!(POSIX_FADV_NORMAL, 0);
    }

    #[test]
    fn test_sequential_is_two() {
        assert_eq!(POSIX_FADV_SEQUENTIAL, 2);
    }

    #[test]
    fn test_falloc_default_is_zero() {
        assert_eq!(FALLOC_FL_DEFAULT, 0);
    }

    #[test]
    fn test_falloc_flags_distinct() {
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
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_falloc_flags_no_overlap() {
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
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_madvise_values_distinct() {
        let vals = [
            POSIX_MADV_NORMAL,
            POSIX_MADV_RANDOM,
            POSIX_MADV_SEQUENTIAL,
            POSIX_MADV_WILLNEED,
            POSIX_MADV_DONTNEED,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_madvise_matches_fadvise() {
        // POSIX specifies these share the same numbering
        assert_eq!(POSIX_MADV_NORMAL, POSIX_FADV_NORMAL);
        assert_eq!(POSIX_MADV_RANDOM, POSIX_FADV_RANDOM);
        assert_eq!(POSIX_MADV_SEQUENTIAL, POSIX_FADV_SEQUENTIAL);
        assert_eq!(POSIX_MADV_WILLNEED, POSIX_FADV_WILLNEED);
        assert_eq!(POSIX_MADV_DONTNEED, POSIX_FADV_DONTNEED);
    }
}

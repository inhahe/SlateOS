//! `readahead(2)` — File readahead constants.
//!
//! readahead(2) initiates readahead on a file, populating the
//! page cache with data that will be needed soon. This avoids
//! blocking on I/O when the data is eventually read. The kernel
//! also performs automatic readahead based on access patterns.

// ---------------------------------------------------------------------------
// readahead limits
// ---------------------------------------------------------------------------

/// Maximum readahead request size (kernel caps at this).
/// Matches VM_READAHEAD_PAGES * PAGE_SIZE (default 256 KiB on 4K pages).
pub const READAHEAD_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Default readahead window (128 KiB, typical kernel default).
pub const READAHEAD_DEFAULT_BYTES: u64 = 128 * 1024;

/// Minimum useful readahead (one page).
pub const READAHEAD_MIN_BYTES: u64 = 4096;

// ---------------------------------------------------------------------------
// Sysctl paths
// ---------------------------------------------------------------------------

/// Per-device readahead sysctl (in /sys/block/<dev>/queue/).
pub const SYSFS_READ_AHEAD_KB: &str = "read_ahead_kb";

/// Default VM readahead pages sysctl.
pub const SYSCTL_VM_READAHEAD: &str = "vm.page-cluster";

// ---------------------------------------------------------------------------
// POSIX advice values (fadvise / madvise related)
// ---------------------------------------------------------------------------

/// Normal access pattern (default readahead).
pub const POSIX_FADV_NORMAL: u32 = 0;
/// Sequential access (increase readahead).
pub const POSIX_FADV_SEQUENTIAL: u32 = 2;
/// Random access (disable readahead).
pub const POSIX_FADV_RANDOM: u32 = 1;
/// Data will be accessed once (don't cache aggressively).
pub const POSIX_FADV_NOREUSE: u32 = 5;
/// Data will be accessed soon (initiate readahead).
pub const POSIX_FADV_WILLNEED: u32 = 3;
/// Data won't be needed (can drop from cache).
pub const POSIX_FADV_DONTNEED: u32 = 4;

// ---------------------------------------------------------------------------
// madvise readahead hints
// ---------------------------------------------------------------------------

/// Normal access.
pub const MADV_NORMAL: u32 = 0;
/// Sequential access (readahead aggressively).
pub const MADV_SEQUENTIAL: u32 = 2;
/// Random access (disable readahead).
pub const MADV_RANDOM: u32 = 1;
/// Will need pages soon.
pub const MADV_WILLNEED: u32 = 3;
/// Don't need pages.
pub const MADV_DONTNEED: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readahead_sizes() {
        assert!(READAHEAD_MIN_BYTES > 0);
        assert!(READAHEAD_DEFAULT_BYTES > READAHEAD_MIN_BYTES);
        assert!(READAHEAD_MAX_BYTES >= READAHEAD_DEFAULT_BYTES);
    }

    #[test]
    fn test_sysfs_paths_distinct() {
        assert_ne!(SYSFS_READ_AHEAD_KB, SYSCTL_VM_READAHEAD);
    }

    #[test]
    fn test_fadvise_values_distinct() {
        let vals = [
            POSIX_FADV_NORMAL,
            POSIX_FADV_SEQUENTIAL,
            POSIX_FADV_RANDOM,
            POSIX_FADV_NOREUSE,
            POSIX_FADV_WILLNEED,
            POSIX_FADV_DONTNEED,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_madvise_values_distinct() {
        let vals = [
            MADV_NORMAL,
            MADV_SEQUENTIAL,
            MADV_RANDOM,
            MADV_WILLNEED,
            MADV_DONTNEED,
        ];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_fadvise_matches_madvise() {
        // These should use the same numeric values
        assert_eq!(POSIX_FADV_NORMAL, MADV_NORMAL);
        assert_eq!(POSIX_FADV_SEQUENTIAL, MADV_SEQUENTIAL);
        assert_eq!(POSIX_FADV_RANDOM, MADV_RANDOM);
        assert_eq!(POSIX_FADV_WILLNEED, MADV_WILLNEED);
        assert_eq!(POSIX_FADV_DONTNEED, MADV_DONTNEED);
    }
}

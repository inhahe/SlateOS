//! `<linux/fadvise.h>` — File access pattern advice constants.
//!
//! `posix_fadvise()` allows applications to inform the kernel about
//! their expected file access patterns so the kernel can optimize
//! read-ahead, page cache retention, and I/O scheduling. The advice
//! is non-binding — it's a hint, not a guarantee. Common use: setting
//! POSIX_FADV_SEQUENTIAL before reading a large file linearly, or
//! POSIX_FADV_DONTNEED after processing data that won't be reused.

// ---------------------------------------------------------------------------
// posix_fadvise() advice values
// ---------------------------------------------------------------------------

/// No special advice (default behavior).
pub const POSIX_FADV_NORMAL: u32 = 0;
/// Expect random access pattern (disable read-ahead).
pub const POSIX_FADV_RANDOM: u32 = 1;
/// Expect sequential access (aggressive read-ahead).
pub const POSIX_FADV_SEQUENTIAL: u32 = 2;
/// Data will be accessed soon (bring into page cache).
pub const POSIX_FADV_WILLNEED: u32 = 3;
/// Data will not be needed soon (can evict from cache).
pub const POSIX_FADV_DONTNEED: u32 = 4;
/// Data will be accessed only once (don't keep in cache).
pub const POSIX_FADV_NOREUSE: u32 = 5;

// ---------------------------------------------------------------------------
// Linux-specific read-ahead sizes (used internally by kernel)
// ---------------------------------------------------------------------------

/// Default read-ahead window size in pages (128 KiB at 4K pages).
pub const VM_READAHEAD_PAGES_DEFAULT: u32 = 32;
/// Maximum read-ahead window size in pages (2 MiB at 4K pages).
pub const VM_READAHEAD_PAGES_MAX: u32 = 512;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advice_values_distinct() {
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
    fn test_advice_values_sequential() {
        assert_eq!(POSIX_FADV_NORMAL, 0);
        assert_eq!(POSIX_FADV_RANDOM, 1);
        assert_eq!(POSIX_FADV_SEQUENTIAL, 2);
        assert_eq!(POSIX_FADV_WILLNEED, 3);
        assert_eq!(POSIX_FADV_DONTNEED, 4);
        assert_eq!(POSIX_FADV_NOREUSE, 5);
    }

    #[test]
    fn test_readahead_defaults() {
        assert!(VM_READAHEAD_PAGES_DEFAULT > 0);
        assert!(VM_READAHEAD_PAGES_MAX > VM_READAHEAD_PAGES_DEFAULT);
        assert!(VM_READAHEAD_PAGES_MAX.is_power_of_two());
    }
}

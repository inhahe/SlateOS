//! `<sys/param.h>` — miscellaneous system parameters.
//!
//! BSD-heritage header providing system limits and utility macros.
//! Many programs (autoconf-generated, BSD-derived) include this
//! header for constants like `MAXPATHLEN`, `NOFILE`, `HZ`, etc.

// ---------------------------------------------------------------------------
// Path limits
// ---------------------------------------------------------------------------

/// Maximum path length in bytes (including null terminator).
///
/// This matches `PATH_MAX` from `<limits.h>`.
pub const MAXPATHLEN: usize = 4096;

/// Maximum length of a hostname.
pub const MAXHOSTNAMELEN: usize = 256;

/// Maximum length of a symbolic link.
pub const MAXSYMLINKS: usize = 20;

/// Maximum length of a login name.
pub const MAXLOGNAME: usize = 256;

/// Maximum length of a domain name.
pub const MAXDOMNAMELEN: usize = 256;

// ---------------------------------------------------------------------------
// Resource limits
// ---------------------------------------------------------------------------

/// Default number of open files per process.
pub const NOFILE: usize = 256;

/// Maximum number of supplementary group IDs.
pub const NGROUPS: usize = 65536;

// ---------------------------------------------------------------------------
// System constants
// ---------------------------------------------------------------------------

/// Timer ticks per second (scheduling quantum).
///
/// Matches `sysconf(_SC_CLK_TCK)` = 100.
pub const HZ: u32 = 100;

/// Pages per kilobyte.
///
/// With our 16 KiB page size, there's less than one page per KB.
/// This constant is 1 for compatibility; page-based calculations
/// should use `getpagesize()` or `sysconf(_SC_PAGESIZE)`.
pub const NBPG: usize = 16384;

/// Page size (same as `getpagesize()`).
pub const PAGE_SIZE: usize = 16384;

/// Shift count for page size (log2(16384) = 14).
pub const PAGE_SHIFT: u32 = 14;

/// Page mask for rounding (PAGE_SIZE - 1).
pub const PAGE_MASK: usize = PAGE_SIZE - 1;

// ---------------------------------------------------------------------------
// Block sizes
// ---------------------------------------------------------------------------

/// Default block size for disk I/O.
pub const DEV_BSIZE: usize = 512;

/// Maximum raw I/O size.
pub const MAXBSIZE: usize = 65536;

// ---------------------------------------------------------------------------
// Alignment / rounding macros as const functions
// ---------------------------------------------------------------------------

/// Round `x` up to the next multiple of `align`.
///
/// `align` must be a power of two.
#[inline]
pub const fn roundup(x: usize, align: usize) -> usize {
    (x + align - 1) & !(align - 1)
}

/// Round `x` down to the previous multiple of `align`.
///
/// `align` must be a power of two.
#[inline]
pub const fn rounddown(x: usize, align: usize) -> usize {
    x & !(align - 1)
}

/// Round `x` up to the next page boundary.
#[inline]
pub const fn round_page(x: usize) -> usize {
    roundup(x, PAGE_SIZE)
}

/// Round `x` down to the previous page boundary.
#[inline]
pub const fn trunc_page(x: usize) -> usize {
    rounddown(x, PAGE_SIZE)
}

/// Return the larger of two values.
#[inline]
pub const fn max(a: usize, b: usize) -> usize {
    if a > b { a } else { b }
}

/// Return the smaller of two values.
#[inline]
pub const fn min(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

/// Clamp `x` between `lo` and `hi`.
#[inline]
pub const fn clamp(x: usize, lo: usize, hi: usize) -> usize {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

/// Check if `x` is a power of two.
#[inline]
pub const fn powerof2(x: usize) -> bool {
    x != 0 && x.is_power_of_two()
}

/// Number of bits in a type, given its size in bytes.
#[inline]
pub const fn nbits(size_bytes: usize) -> usize {
    size_bytes * 8
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants — basic values
    // -----------------------------------------------------------------------

    #[test]
    fn test_maxpathlen() {
        assert_eq!(MAXPATHLEN, 4096);
        assert_eq!(MAXPATHLEN, crate::limits::PATH_MAX as usize);
    }

    #[test]
    fn test_maxhostnamelen() {
        assert_eq!(MAXHOSTNAMELEN, 256);
    }

    #[test]
    fn test_maxsymlinks() {
        assert!(MAXSYMLINKS >= 8, "should allow reasonable symlink depth");
    }

    #[test]
    fn test_nofile() {
        assert!(NOFILE >= 64);
    }

    #[test]
    fn test_hz() {
        assert_eq!(HZ, 100);
    }

    #[test]
    fn test_page_size() {
        assert_eq!(PAGE_SIZE, 16384);
        assert_eq!(NBPG, 16384);
    }

    #[test]
    fn test_page_shift() {
        assert_eq!(1usize << PAGE_SHIFT, PAGE_SIZE);
    }

    #[test]
    fn test_page_mask() {
        assert_eq!(PAGE_MASK, PAGE_SIZE - 1);
        assert_eq!(PAGE_MASK, 0x3FFF);
    }

    #[test]
    fn test_dev_bsize() {
        assert_eq!(DEV_BSIZE, 512);
    }

    #[test]
    fn test_maxbsize() {
        assert!(MAXBSIZE >= DEV_BSIZE);
        assert!(MAXBSIZE >= PAGE_SIZE);
    }

    // -----------------------------------------------------------------------
    // roundup / rounddown
    // -----------------------------------------------------------------------

    #[test]
    fn test_roundup_already_aligned() {
        assert_eq!(roundup(4096, 4096), 4096);
        assert_eq!(roundup(0, 4096), 0);
    }

    #[test]
    fn test_roundup_needs_rounding() {
        assert_eq!(roundup(1, 4096), 4096);
        assert_eq!(roundup(4095, 4096), 4096);
        assert_eq!(roundup(4097, 4096), 8192);
    }

    #[test]
    fn test_roundup_small_alignment() {
        assert_eq!(roundup(5, 4), 8);
        assert_eq!(roundup(7, 8), 8);
        assert_eq!(roundup(9, 8), 16);
    }

    #[test]
    fn test_rounddown_already_aligned() {
        assert_eq!(rounddown(4096, 4096), 4096);
        assert_eq!(rounddown(0, 4096), 0);
    }

    #[test]
    fn test_rounddown_needs_rounding() {
        assert_eq!(rounddown(4095, 4096), 0);
        assert_eq!(rounddown(8191, 4096), 4096);
        assert_eq!(rounddown(1, 4096), 0);
    }

    // -----------------------------------------------------------------------
    // round_page / trunc_page
    // -----------------------------------------------------------------------

    #[test]
    fn test_round_page() {
        assert_eq!(round_page(0), 0);
        assert_eq!(round_page(1), PAGE_SIZE);
        assert_eq!(round_page(PAGE_SIZE), PAGE_SIZE);
        assert_eq!(round_page(PAGE_SIZE + 1), 2 * PAGE_SIZE);
    }

    #[test]
    fn test_trunc_page() {
        assert_eq!(trunc_page(0), 0);
        assert_eq!(trunc_page(1), 0);
        assert_eq!(trunc_page(PAGE_SIZE), PAGE_SIZE);
        assert_eq!(trunc_page(PAGE_SIZE + 1), PAGE_SIZE);
        assert_eq!(trunc_page(2 * PAGE_SIZE - 1), PAGE_SIZE);
    }

    // -----------------------------------------------------------------------
    // min / max / clamp
    // -----------------------------------------------------------------------

    #[test]
    fn test_max() {
        assert_eq!(max(1, 2), 2);
        assert_eq!(max(5, 3), 5);
        assert_eq!(max(0, 0), 0);
    }

    #[test]
    fn test_min() {
        assert_eq!(min(1, 2), 1);
        assert_eq!(min(5, 3), 3);
        assert_eq!(min(0, 0), 0);
    }

    #[test]
    fn test_clamp() {
        assert_eq!(clamp(5, 0, 10), 5);
        assert_eq!(clamp(0, 1, 10), 1);
        assert_eq!(clamp(20, 1, 10), 10);
        assert_eq!(clamp(1, 1, 1), 1);
    }

    // -----------------------------------------------------------------------
    // powerof2
    // -----------------------------------------------------------------------

    #[test]
    fn test_powerof2_true() {
        assert!(powerof2(1));
        assert!(powerof2(2));
        assert!(powerof2(4));
        assert!(powerof2(8));
        assert!(powerof2(1024));
        assert!(powerof2(PAGE_SIZE));
    }

    #[test]
    fn test_powerof2_false() {
        assert!(!powerof2(0));
        assert!(!powerof2(3));
        assert!(!powerof2(5));
        assert!(!powerof2(6));
        assert!(!powerof2(7));
        assert!(!powerof2(1023));
    }

    // -----------------------------------------------------------------------
    // nbits
    // -----------------------------------------------------------------------

    #[test]
    fn test_nbits() {
        assert_eq!(nbits(1), 8);
        assert_eq!(nbits(2), 16);
        assert_eq!(nbits(4), 32);
        assert_eq!(nbits(8), 64);
    }

    // -----------------------------------------------------------------------
    // Cross-module consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_page_size_matches_mman() {
        // PAGE_SIZE should agree with the value from sysconf.
        assert_eq!(
            PAGE_SIZE as i64,
            crate::unistd::sysconf(crate::unistd::_SC_PAGESIZE)
        );
    }

    #[test]
    fn test_hz_matches_clk_tck() {
        assert_eq!(
            HZ as i64,
            crate::unistd::sysconf(crate::unistd::_SC_CLK_TCK)
        );
    }
}

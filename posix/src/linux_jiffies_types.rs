//! `<linux/jiffies.h>` — jiffy time unit constants and conversions.
//!
//! A jiffy is the kernel's fundamental time unit: one tick of the
//! system timer (1/HZ seconds). Jiffies are a 64-bit counter that
//! starts at a deliberately large value to catch wraparound bugs.
//! Many kernel timeouts, delays, and scheduling decisions are
//! expressed in jiffies.

// ---------------------------------------------------------------------------
// Initial jiffy value
// ---------------------------------------------------------------------------

/// Initial jiffies value (intentionally near u32 wraparound).
/// Set to `-(5 * 60 * HZ)` at boot to catch unsigned comparison bugs
/// early.  For HZ=1000 this is about 300000 ticks before wraparound.
pub const INITIAL_JIFFIES_OFFSET_SECONDS: u32 = 300;

// ---------------------------------------------------------------------------
// Jiffy conversion constants (for HZ=1000)
// ---------------------------------------------------------------------------

/// Microseconds per jiffy at HZ=1000.
pub const USEC_PER_JIFFY_HZ1000: u32 = 1000;
/// Nanoseconds per jiffy at HZ=1000.
pub const NSEC_PER_JIFFY_HZ1000: u32 = 1_000_000;
/// Milliseconds per jiffy at HZ=1000.
pub const MSEC_PER_JIFFY_HZ1000: u32 = 1;

/// Microseconds per jiffy at HZ=250.
pub const USEC_PER_JIFFY_HZ250: u32 = 4000;
/// Nanoseconds per jiffy at HZ=250.
pub const NSEC_PER_JIFFY_HZ250: u32 = 4_000_000;
/// Milliseconds per jiffy at HZ=250.
pub const MSEC_PER_JIFFY_HZ250: u32 = 4;

/// Microseconds per jiffy at HZ=100.
pub const USEC_PER_JIFFY_HZ100: u32 = 10_000;
/// Nanoseconds per jiffy at HZ=100.
pub const NSEC_PER_JIFFY_HZ100: u32 = 10_000_000;
/// Milliseconds per jiffy at HZ=100.
pub const MSEC_PER_JIFFY_HZ100: u32 = 10;

// ---------------------------------------------------------------------------
// Time unit constants
// ---------------------------------------------------------------------------

/// Nanoseconds per microsecond.
pub const NSEC_PER_USEC: u32 = 1_000;
/// Nanoseconds per millisecond.
pub const NSEC_PER_MSEC: u32 = 1_000_000;
/// Nanoseconds per second.
pub const NSEC_PER_SEC: u32 = 1_000_000_000;
/// Microseconds per second.
pub const USEC_PER_SEC: u32 = 1_000_000;
/// Milliseconds per second.
pub const MSEC_PER_SEC: u32 = 1_000;

// ---------------------------------------------------------------------------
// Special timeout values
// ---------------------------------------------------------------------------

/// Infinite timeout (maximum jiffies value).
pub const MAX_JIFFY_OFFSET: u64 = u64::MAX / 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hz1000_conversions() {
        // At HZ=1000, one jiffy = 1ms = 1000us = 1_000_000ns
        assert_eq!(MSEC_PER_JIFFY_HZ1000, 1);
        assert_eq!(USEC_PER_JIFFY_HZ1000, 1000);
        assert_eq!(NSEC_PER_JIFFY_HZ1000, 1_000_000);
    }

    #[test]
    fn test_hz250_conversions() {
        // At HZ=250, one jiffy = 4ms
        assert_eq!(MSEC_PER_JIFFY_HZ250, 4);
        assert_eq!(USEC_PER_JIFFY_HZ250, 4000);
        assert_eq!(NSEC_PER_JIFFY_HZ250, 4_000_000);
    }

    #[test]
    fn test_hz100_conversions() {
        // At HZ=100, one jiffy = 10ms
        assert_eq!(MSEC_PER_JIFFY_HZ100, 10);
        assert_eq!(USEC_PER_JIFFY_HZ100, 10_000);
        assert_eq!(NSEC_PER_JIFFY_HZ100, 10_000_000);
    }

    #[test]
    fn test_time_unit_conversions() {
        assert_eq!(NSEC_PER_SEC, NSEC_PER_MSEC * MSEC_PER_SEC);
        assert_eq!(NSEC_PER_MSEC, NSEC_PER_USEC * 1000);
        assert_eq!(USEC_PER_SEC, MSEC_PER_SEC * 1000);
    }

    #[test]
    fn test_jiffy_consistency() {
        // ns_per_jiffy = us_per_jiffy * 1000
        assert_eq!(NSEC_PER_JIFFY_HZ1000, USEC_PER_JIFFY_HZ1000 * NSEC_PER_USEC);
        assert_eq!(NSEC_PER_JIFFY_HZ250, USEC_PER_JIFFY_HZ250 * NSEC_PER_USEC);
        assert_eq!(NSEC_PER_JIFFY_HZ100, USEC_PER_JIFFY_HZ100 * NSEC_PER_USEC);
    }

    #[test]
    fn test_max_jiffy_offset() {
        assert!(MAX_JIFFY_OFFSET > 0);
    }
}

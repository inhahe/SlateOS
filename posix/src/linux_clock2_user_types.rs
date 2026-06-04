//! `<time.h>` — itimerspec layout and POSIX timer flags.
//!
//! `timer_settime`, `timerfd_settime`, and `clock_nanosleep` all take
//! a `struct itimerspec` describing an initial expiration and an
//! interval. This module covers field offsets and absolute/relative
//! mode flags.

// ---------------------------------------------------------------------------
// struct timespec field offsets (64-bit, tv_sec=time_t, tv_nsec=long)
// ---------------------------------------------------------------------------

/// `time_t tv_sec` (seconds).
pub const TIMESPEC_OFF_SEC: usize = 0;
/// `long tv_nsec` (nanoseconds).
pub const TIMESPEC_OFF_NSEC: usize = 8;
/// Total struct timespec size on 64-bit.
pub const TIMESPEC_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// struct itimerspec field offsets
// ---------------------------------------------------------------------------

/// `struct timespec it_interval` — period between expirations.
pub const ITIMERSPEC_OFF_INTERVAL: usize = 0;
/// `struct timespec it_value` — initial expiration.
pub const ITIMERSPEC_OFF_VALUE: usize = 16;
/// Total struct itimerspec size.
pub const ITIMERSPEC_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// timer_settime / timerfd_settime flags
// ---------------------------------------------------------------------------

/// Relative time (default).
pub const TFD_TIMER_RELTIME: u32 = 0;
/// Absolute time (wake at specified clock time).
pub const TFD_TIMER_ABSTIME: u32 = 1 << 0;
/// Cancel on clock change (REALTIME only).
pub const TFD_TIMER_CANCEL_ON_SET: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// timerfd_create flags
// ---------------------------------------------------------------------------

/// Close on exec.
pub const TFD_CLOEXEC: u32 = 0o2_000_000;
/// Non-blocking.
pub const TFD_NONBLOCK: u32 = 0o0_004_000;

// ---------------------------------------------------------------------------
// Limits on tv_nsec
// ---------------------------------------------------------------------------

/// Maximum valid tv_nsec value (NSEC_PER_SEC - 1).
pub const TIMESPEC_NSEC_MAX: i64 = 999_999_999;

/// Minimum valid tv_nsec value.
pub const TIMESPEC_NSEC_MIN: i64 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timespec_layout_two_8byte_fields() {
        assert_eq!(TIMESPEC_OFF_SEC, 0);
        assert_eq!(TIMESPEC_OFF_NSEC, 8);
        assert_eq!(TIMESPEC_SIZE, 16);
    }

    #[test]
    fn test_itimerspec_is_two_timespecs() {
        assert_eq!(ITIMERSPEC_OFF_INTERVAL, 0);
        assert_eq!(ITIMERSPEC_OFF_VALUE, TIMESPEC_SIZE);
        assert_eq!(ITIMERSPEC_SIZE, 2 * TIMESPEC_SIZE);
    }

    #[test]
    fn test_settime_flags_distinct_bits() {
        assert_eq!(TFD_TIMER_RELTIME, 0);
        assert!(TFD_TIMER_ABSTIME.is_power_of_two());
        assert!(TFD_TIMER_CANCEL_ON_SET.is_power_of_two());
        assert_eq!(TFD_TIMER_ABSTIME & TFD_TIMER_CANCEL_ON_SET, 0);
    }

    #[test]
    fn test_create_flags_match_open_flags() {
        // TFD_CLOEXEC == O_CLOEXEC == 0o2000000.
        assert_eq!(TFD_CLOEXEC, 0o2_000_000);
        // TFD_NONBLOCK == O_NONBLOCK == 0o4000.
        assert_eq!(TFD_NONBLOCK, 0o4_000);
    }

    #[test]
    fn test_nsec_range() {
        assert_eq!(TIMESPEC_NSEC_MIN, 0);
        assert_eq!(TIMESPEC_NSEC_MAX, 999_999_999);
        // 0 .. 1_000_000_000.
        assert_eq!(TIMESPEC_NSEC_MAX + 1, 1_000_000_000);
    }
}

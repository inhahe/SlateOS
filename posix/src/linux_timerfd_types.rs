//! `<linux/timerfd.h>` — timerfd_create/settime/gettime constants.
//!
//! timerfd creates a file descriptor that delivers timer expirations
//! via read(). The fd is pollable (epoll/io_uring compatible), making
//! it ideal for event loops that need timeouts alongside I/O events.
//! Supports both CLOCK_REALTIME and CLOCK_MONOTONIC with absolute
//! or relative expiration and optional periodic repetition.

// ---------------------------------------------------------------------------
// timerfd_create flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the new fd.
pub const TFD_CLOEXEC: u32 = 0o200_0000;
/// Set non-blocking on the new fd.
pub const TFD_NONBLOCK: u32 = 0o000_4000;

// ---------------------------------------------------------------------------
// timerfd_settime flags
// ---------------------------------------------------------------------------

/// Use absolute time (not relative to now).
pub const TFD_TIMER_ABSTIME: u32 = 1 << 0;
/// Cancel timer when realtime clock is set (requires ABSTIME).
pub const TFD_TIMER_CANCEL_ON_SET: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Clock IDs used with timerfd_create
// ---------------------------------------------------------------------------

/// System-wide real-time clock.
pub const CLOCK_REALTIME: u32 = 0;
/// Monotonic clock (unaffected by clock_settime).
pub const CLOCK_MONOTONIC: u32 = 1;
/// Per-process CPU-time clock.
pub const CLOCK_PROCESS_CPUTIME_ID: u32 = 2;
/// Per-thread CPU-time clock.
pub const CLOCK_THREAD_CPUTIME_ID: u32 = 3;
/// Like MONOTONIC but includes time in suspend.
pub const CLOCK_BOOTTIME: u32 = 7;
/// Like REALTIME but with alarm wakeup capability.
pub const CLOCK_REALTIME_ALARM: u32 = 8;
/// Like BOOTTIME but with alarm wakeup capability.
pub const CLOCK_BOOTTIME_ALARM: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_flags_distinct() {
        assert_ne!(TFD_CLOEXEC, TFD_NONBLOCK);
    }

    #[test]
    fn test_settime_flags_no_overlap() {
        assert_eq!(TFD_TIMER_ABSTIME & TFD_TIMER_CANCEL_ON_SET, 0);
        assert!(TFD_TIMER_ABSTIME.is_power_of_two());
        assert!(TFD_TIMER_CANCEL_ON_SET.is_power_of_two());
    }

    #[test]
    fn test_clock_ids_distinct() {
        let clocks = [
            CLOCK_REALTIME,
            CLOCK_MONOTONIC,
            CLOCK_PROCESS_CPUTIME_ID,
            CLOCK_THREAD_CPUTIME_ID,
            CLOCK_BOOTTIME,
            CLOCK_REALTIME_ALARM,
            CLOCK_BOOTTIME_ALARM,
        ];
        for i in 0..clocks.len() {
            for j in (i + 1)..clocks.len() {
                assert_ne!(clocks[i], clocks[j]);
            }
        }
    }

    #[test]
    fn test_monotonic_nonzero() {
        assert_ne!(CLOCK_MONOTONIC, 0);
    }
}

//! `<sys/timerfd.h>` — Timerfd flag and clock constants.
//!
//! Timerfd provides timer expiration notifications via a file
//! descriptor. Each read returns the number of expirations since
//! the last read as a uint64 value.

// ---------------------------------------------------------------------------
// timerfd_create flags
// ---------------------------------------------------------------------------

/// Set close-on-exec flag.
pub const TFD_CLOEXEC: u32 = 0o2000000;
/// Set non-blocking I/O.
pub const TFD_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// timerfd_settime flags
// ---------------------------------------------------------------------------

/// Use absolute time (not relative).
pub const TFD_TIMER_ABSTIME: u32 = 1 << 0;
/// Cancel timer on CANCEL_ON_SET (for CLOCK_REALTIME).
pub const TFD_TIMER_CANCEL_ON_SET: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Clock IDs (for timerfd_create clockid argument)
// ---------------------------------------------------------------------------

/// Monotonic clock (cannot be set, unaffected by NTP).
pub const CLOCK_MONOTONIC: u32 = 1;
/// Real-time clock (wall clock, can be set).
pub const CLOCK_REALTIME: u32 = 0;
/// Boot-time clock (includes time spent in suspend).
pub const CLOCK_BOOTTIME: u32 = 7;
/// Alarm-capable real-time clock.
pub const CLOCK_REALTIME_ALARM: u32 = 8;
/// Alarm-capable boot-time clock.
pub const CLOCK_BOOTTIME_ALARM: u32 = 9;

// ---------------------------------------------------------------------------
// timerfd read size
// ---------------------------------------------------------------------------

/// Size of timerfd expiration counter (always 8 bytes / u64).
pub const TFD_COUNTER_SIZE: u32 = 8;

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
    }

    #[test]
    fn test_settime_flags_power_of_two() {
        assert!(TFD_TIMER_ABSTIME.is_power_of_two());
        assert!(TFD_TIMER_CANCEL_ON_SET.is_power_of_two());
    }

    #[test]
    fn test_clock_ids_distinct() {
        let clocks = [
            CLOCK_MONOTONIC, CLOCK_REALTIME, CLOCK_BOOTTIME,
            CLOCK_REALTIME_ALARM, CLOCK_BOOTTIME_ALARM,
        ];
        for i in 0..clocks.len() {
            for j in (i + 1)..clocks.len() {
                assert_ne!(clocks[i], clocks[j]);
            }
        }
    }

    #[test]
    fn test_realtime_is_zero() {
        assert_eq!(CLOCK_REALTIME, 0);
    }

    #[test]
    fn test_counter_size() {
        assert_eq!(TFD_COUNTER_SIZE, 8);
    }

    #[test]
    fn test_cloexec() {
        assert_eq!(TFD_CLOEXEC, 0o2000000);
    }
}

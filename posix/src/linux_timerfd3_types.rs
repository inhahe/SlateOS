//! `<linux/timerfd.h>` — Additional timerfd constants (batch 3).
//!
//! Supplementary timerfd constants covering timer flags,
//! clock source IDs, and timer interval limits.

// ---------------------------------------------------------------------------
// Timerfd create flags
// ---------------------------------------------------------------------------

/// Set close-on-exec.
pub const TFD_CLOEXEC: u32 = 0o2000000;
/// Set non-blocking.
pub const TFD_NONBLOCK: u32 = 0o4000;

// ---------------------------------------------------------------------------
// Timerfd settime flags
// ---------------------------------------------------------------------------

/// Absolute timer.
pub const TFD_TIMER_ABSTIME: u32 = 1 << 0;
/// Cancel on set (clock_settime).
pub const TFD_TIMER_CANCEL_ON_SET: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Clock source IDs for timerfd
// ---------------------------------------------------------------------------

/// Realtime clock.
pub const TFD_CLOCK_REALTIME: u32 = 0;
/// Monotonic clock.
pub const TFD_CLOCK_MONOTONIC: u32 = 1;
/// Boot time clock.
pub const TFD_CLOCK_BOOTTIME: u32 = 7;
/// Realtime alarm clock.
pub const TFD_CLOCK_REALTIME_ALARM: u32 = 8;
/// Boot time alarm clock.
pub const TFD_CLOCK_BOOTTIME_ALARM: u32 = 9;

// ---------------------------------------------------------------------------
// Timerfd IOCTLs
// ---------------------------------------------------------------------------

/// IOCTL to set ticks.
pub const TFD_IOC_SET_TICKS: u32 = 0x40085400;

// ---------------------------------------------------------------------------
// Timerfd read size
// ---------------------------------------------------------------------------

/// Size of data returned by read (u64 = 8 bytes).
pub const TFD_READ_SIZE: u32 = 8;

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
    fn test_settime_flags_power_of_two() {
        assert!(TFD_TIMER_ABSTIME.is_power_of_two());
        assert!(TFD_TIMER_CANCEL_ON_SET.is_power_of_two());
    }

    #[test]
    fn test_settime_flags_no_overlap() {
        assert_eq!(TFD_TIMER_ABSTIME & TFD_TIMER_CANCEL_ON_SET, 0);
    }

    #[test]
    fn test_clock_sources_distinct() {
        let clocks = [
            TFD_CLOCK_REALTIME,
            TFD_CLOCK_MONOTONIC,
            TFD_CLOCK_BOOTTIME,
            TFD_CLOCK_REALTIME_ALARM,
            TFD_CLOCK_BOOTTIME_ALARM,
        ];
        for i in 0..clocks.len() {
            for j in (i + 1)..clocks.len() {
                assert_ne!(clocks[i], clocks[j]);
            }
        }
    }

    #[test]
    fn test_clock_realtime_is_zero() {
        assert_eq!(TFD_CLOCK_REALTIME, 0);
    }

    #[test]
    fn test_read_size() {
        assert_eq!(TFD_READ_SIZE, 8);
    }
}

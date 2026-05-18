//! `<linux/time.h>` — Additional POSIX timer constants.
//!
//! Supplementary timer constants covering timer types,
//! clock IDs, timerfd flags, and timer flags.

// ---------------------------------------------------------------------------
// Timer types (ITIMER_*)
// ---------------------------------------------------------------------------

/// Real-time timer.
pub const ITIMER_REAL: u32 = 0;
/// Virtual timer (user CPU time).
pub const ITIMER_VIRTUAL: u32 = 1;
/// Profiling timer (user + sys CPU time).
pub const ITIMER_PROF: u32 = 2;

// ---------------------------------------------------------------------------
// Extended clock IDs
// ---------------------------------------------------------------------------

/// TAI (International Atomic Time).
pub const CLOCK_TAI: u32 = 11;
/// Boottime alarm.
pub const CLOCK_BOOTTIME_ALARM: u32 = 9;
/// Realtime alarm.
pub const CLOCK_REALTIME_ALARM: u32 = 8;
/// Realtime coarse.
pub const CLOCK_REALTIME_COARSE: u32 = 5;
/// Monotonic coarse.
pub const CLOCK_MONOTONIC_COARSE: u32 = 6;
/// Monotonic raw.
pub const CLOCK_MONOTONIC_RAW: u32 = 4;
/// Boottime.
pub const CLOCK_BOOTTIME: u32 = 7;
/// Process CPU time.
pub const CLOCK_PROCESS_CPUTIME_ID: u32 = 2;
/// Thread CPU time.
pub const CLOCK_THREAD_CPUTIME_ID: u32 = 3;

// ---------------------------------------------------------------------------
// Timer flags (timer_settime, timerfd)
// ---------------------------------------------------------------------------

/// Absolute time.
pub const TIMER_ABSTIME: u32 = 0x01;
/// Cancel on set (timerfd).
pub const TFD_TIMER_CANCEL_ON_SET: u32 = 0x02;

// ---------------------------------------------------------------------------
// timerfd flags
// ---------------------------------------------------------------------------

/// Close on exec.
pub const TFD_CLOEXEC: u32 = 0x80000;
/// Non-blocking.
pub const TFD_NONBLOCK: u32 = 0x800;

// ---------------------------------------------------------------------------
// clock_nanosleep flags
// ---------------------------------------------------------------------------

/// Relative timer.
pub const TIMER_RELATIVE: u32 = 0;

// ---------------------------------------------------------------------------
// Time constants
// ---------------------------------------------------------------------------

/// Nanoseconds per second.
pub const NSEC_PER_SEC: u64 = 1_000_000_000;
/// Microseconds per second.
pub const USEC_PER_SEC: u64 = 1_000_000;
/// Nanoseconds per microsecond.
pub const NSEC_PER_USEC: u64 = 1_000;
/// Nanoseconds per millisecond.
pub const NSEC_PER_MSEC: u64 = 1_000_000;
/// Milliseconds per second.
pub const MSEC_PER_SEC: u64 = 1_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_itimer_types_distinct() {
        let types = [ITIMER_REAL, ITIMER_VIRTUAL, ITIMER_PROF];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_clock_ids_distinct() {
        let ids = [
            CLOCK_TAI, CLOCK_BOOTTIME_ALARM, CLOCK_REALTIME_ALARM,
            CLOCK_REALTIME_COARSE, CLOCK_MONOTONIC_COARSE,
            CLOCK_MONOTONIC_RAW, CLOCK_BOOTTIME,
            CLOCK_PROCESS_CPUTIME_ID, CLOCK_THREAD_CPUTIME_ID,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_timer_flags() {
        assert_eq!(TIMER_ABSTIME, 0x01);
        assert_eq!(TFD_TIMER_CANCEL_ON_SET, 0x02);
        assert_eq!(TIMER_ABSTIME & TFD_TIMER_CANCEL_ON_SET, 0);
    }

    #[test]
    fn test_timerfd_flags() {
        assert_ne!(TFD_CLOEXEC, TFD_NONBLOCK);
    }

    #[test]
    fn test_time_constants() {
        assert_eq!(NSEC_PER_SEC, USEC_PER_SEC * NSEC_PER_USEC);
        assert_eq!(NSEC_PER_SEC, MSEC_PER_SEC * NSEC_PER_MSEC);
    }
}

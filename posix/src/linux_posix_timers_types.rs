//! `<linux/posix-timers.h>` — POSIX interval timer constants.
//!
//! POSIX timers (timer_create/timer_settime/timer_gettime) provide
//! per-process interval timers that can be set to expire at a
//! specified time or interval. Each timer is associated with a clock
//! (CLOCK_REALTIME, CLOCK_MONOTONIC, etc.) and delivers notifications
//! via signals, thread creation, or no notification. Unlike setitimer(),
//! POSIX timers support multiple timers per process and nanosecond
//! resolution.

// ---------------------------------------------------------------------------
// Timer notification types (sigevent.sigev_notify)
// ---------------------------------------------------------------------------

/// No notification on timer expiry.
pub const SIGEV_NONE: u32 = 0;
/// Deliver a signal on timer expiry.
pub const SIGEV_SIGNAL: u32 = 1;
/// Create a new thread on timer expiry.
pub const SIGEV_THREAD: u32 = 2;
/// Deliver signal to specific thread (Linux extension).
pub const SIGEV_THREAD_ID: u32 = 4;

// ---------------------------------------------------------------------------
// Timer flags (for timer_settime)
// ---------------------------------------------------------------------------

/// Relative time (fire after specified duration).
pub const TIMER_ABSTIME: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// Clock IDs (used with timer_create)
// ---------------------------------------------------------------------------

/// Real-time clock (affected by NTP/adjtime).
pub const CLOCK_REALTIME: u32 = 0;
/// Monotonic clock (unaffected by NTP).
pub const CLOCK_MONOTONIC: u32 = 1;
/// Per-process CPU time.
pub const CLOCK_PROCESS_CPUTIME_ID: u32 = 2;
/// Per-thread CPU time.
pub const CLOCK_THREAD_CPUTIME_ID: u32 = 3;
/// Monotonic raw (no NTP adjustments, no slewing).
pub const CLOCK_MONOTONIC_RAW: u32 = 4;
/// Coarse realtime (faster but lower resolution).
pub const CLOCK_REALTIME_COARSE: u32 = 5;
/// Coarse monotonic (faster but lower resolution).
pub const CLOCK_MONOTONIC_COARSE: u32 = 6;
/// Boottime (like monotonic but includes suspend time).
pub const CLOCK_BOOTTIME: u32 = 7;
/// Realtime alarm (wakes system from suspend).
pub const CLOCK_REALTIME_ALARM: u32 = 8;
/// Boottime alarm (wakes system from suspend).
pub const CLOCK_BOOTTIME_ALARM: u32 = 9;
/// TAI (International Atomic Time, no leap seconds).
pub const CLOCK_TAI: u32 = 11;

// ---------------------------------------------------------------------------
// Timer overrun limits
// ---------------------------------------------------------------------------

/// Maximum overrun count that can be reported.
pub const DELAYTIMER_MAX: u32 = 0x7FFF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sigev_types_distinct() {
        let types = [SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_clock_ids_distinct() {
        let clocks = [
            CLOCK_REALTIME, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID,
            CLOCK_THREAD_CPUTIME_ID, CLOCK_MONOTONIC_RAW,
            CLOCK_REALTIME_COARSE, CLOCK_MONOTONIC_COARSE,
            CLOCK_BOOTTIME, CLOCK_REALTIME_ALARM,
            CLOCK_BOOTTIME_ALARM, CLOCK_TAI,
        ];
        for i in 0..clocks.len() {
            for j in (i + 1)..clocks.len() {
                assert_ne!(clocks[i], clocks[j]);
            }
        }
    }

    #[test]
    fn test_timer_abstime_flag() {
        assert_eq!(TIMER_ABSTIME, 1);
    }

    #[test]
    fn test_delaytimer_max() {
        assert_eq!(DELAYTIMER_MAX, i32::MAX as u32);
    }
}

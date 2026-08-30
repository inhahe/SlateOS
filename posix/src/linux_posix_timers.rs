//! POSIX timer constants (`timer_create`, `timer_settime`, etc.).
//!
//! POSIX per-process timers deliver signals or thread notifications
//! when they expire. They use various clock sources and support
//! one-shot or periodic operation.

// ---------------------------------------------------------------------------
// Clock IDs
// ---------------------------------------------------------------------------

/// Wall-clock time (affected by NTP/settimeofday).
pub const CLOCK_REALTIME: u32 = 0;
/// Monotonic clock (unaffected by time adjustments).
pub const CLOCK_MONOTONIC: u32 = 1;
/// Per-process CPU time.
pub const CLOCK_PROCESS_CPUTIME_ID: u32 = 2;
/// Per-thread CPU time.
pub const CLOCK_THREAD_CPUTIME_ID: u32 = 3;
/// Monotonic raw (no NTP adjustments, no smoothing).
pub const CLOCK_MONOTONIC_RAW: u32 = 4;
/// Coarse realtime (faster, less precise).
pub const CLOCK_REALTIME_COARSE: u32 = 5;
/// Coarse monotonic.
pub const CLOCK_MONOTONIC_COARSE: u32 = 6;
/// Boot time (includes suspend time).
pub const CLOCK_BOOTTIME: u32 = 7;
/// Realtime alarm (wakes from suspend).
pub const CLOCK_REALTIME_ALARM: u32 = 8;
/// Boot time alarm.
pub const CLOCK_BOOTTIME_ALARM: u32 = 9;
/// TAI (International Atomic Time).
pub const CLOCK_TAI: u32 = 11;

// ---------------------------------------------------------------------------
// Timer flags
// ---------------------------------------------------------------------------

/// Relative time (default).
pub const TIMER_ABSTIME: u32 = 0x01;

// ---------------------------------------------------------------------------
// Signal notification (sigevent)
// ---------------------------------------------------------------------------

/// No notification.
pub const SIGEV_NONE: u32 = 1;
/// Deliver signal.
pub const SIGEV_SIGNAL: u32 = 0;
/// Deliver via thread.
pub const SIGEV_THREAD: u32 = 2;
/// Thread ID targeted signal.
pub const SIGEV_THREAD_ID: u32 = 4;

// ---------------------------------------------------------------------------
// Timer limits
// ---------------------------------------------------------------------------

/// Maximum POSIX timers per process (default).
pub const TIMER_MAX_DEFAULT: u32 = 512;

// ---------------------------------------------------------------------------
// Overrun
// ---------------------------------------------------------------------------

/// Maximum overrun count returned by timer_getoverrun.
pub const DELAYTIMER_MAX: u32 = 0x7FFFFFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_ids_distinct() {
        let clocks = [
            CLOCK_REALTIME,
            CLOCK_MONOTONIC,
            CLOCK_PROCESS_CPUTIME_ID,
            CLOCK_THREAD_CPUTIME_ID,
            CLOCK_MONOTONIC_RAW,
            CLOCK_REALTIME_COARSE,
            CLOCK_MONOTONIC_COARSE,
            CLOCK_BOOTTIME,
            CLOCK_REALTIME_ALARM,
            CLOCK_BOOTTIME_ALARM,
            CLOCK_TAI,
        ];
        for i in 0..clocks.len() {
            for j in (i + 1)..clocks.len() {
                assert_ne!(clocks[i], clocks[j]);
            }
        }
    }

    #[test]
    fn test_timer_abstime() {
        assert_eq!(TIMER_ABSTIME, 1);
    }

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
    fn test_timer_max() {
        assert!(TIMER_MAX_DEFAULT > 0);
    }

    #[test]
    fn test_delaytimer_max() {
        assert_eq!(DELAYTIMER_MAX, 0x7FFFFFFF);
    }
}

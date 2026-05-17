//! `<linux/time.h>` — Clock and time constants.
//!
//! Linux provides multiple clocks for different purposes: REALTIME
//! (wall clock, can be set), MONOTONIC (elapsed time since boot,
//! never decremented), BOOTTIME (like MONOTONIC but includes suspend),
//! and per-CPU/thread clocks. clock_gettime/clock_settime access these.

// ---------------------------------------------------------------------------
// Clock IDs (clockid_t for clock_gettime/clock_settime)
// ---------------------------------------------------------------------------

/// Wall-clock time (settable, affected by NTP/adjtime).
pub const CLOCK_REALTIME: u32 = 0;
/// Monotonic (never goes backward, not settable).
pub const CLOCK_MONOTONIC: u32 = 1;
/// Per-process CPU time (user + system).
pub const CLOCK_PROCESS_CPUTIME_ID: u32 = 2;
/// Per-thread CPU time.
pub const CLOCK_THREAD_CPUTIME_ID: u32 = 3;
/// Like MONOTONIC but includes suspend time.
pub const CLOCK_BOOTTIME: u32 = 7;
/// Like REALTIME but can wake from suspend.
pub const CLOCK_REALTIME_ALARM: u32 = 8;
/// Like BOOTTIME but can wake from suspend.
pub const CLOCK_BOOTTIME_ALARM: u32 = 9;
/// TAI (International Atomic Time, no leap seconds).
pub const CLOCK_TAI: u32 = 11;
/// Monotonic raw (no NTP adjustment).
pub const CLOCK_MONOTONIC_RAW: u32 = 4;
/// Coarse realtime (faster but less precise).
pub const CLOCK_REALTIME_COARSE: u32 = 5;
/// Coarse monotonic (faster but less precise).
pub const CLOCK_MONOTONIC_COARSE: u32 = 6;

// ---------------------------------------------------------------------------
// clock_nanosleep flags
// ---------------------------------------------------------------------------

/// Relative sleep (duration from now).
pub const TIMER_RELTIME: u32 = 0;
/// Absolute sleep (wake at specified time).
pub const TIMER_ABSTIME: u32 = 1;

// ---------------------------------------------------------------------------
// Time constants
// ---------------------------------------------------------------------------

/// Nanoseconds per second.
pub const NSEC_PER_SEC: u64 = 1_000_000_000;
/// Microseconds per second.
pub const USEC_PER_SEC: u64 = 1_000_000;
/// Nanoseconds per millisecond.
pub const NSEC_PER_MSEC: u64 = 1_000_000;
/// Nanoseconds per microsecond.
pub const NSEC_PER_USEC: u64 = 1_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_timer_flags_distinct() {
        assert_ne!(TIMER_RELTIME, TIMER_ABSTIME);
    }

    #[test]
    fn test_time_constants() {
        assert_eq!(NSEC_PER_SEC, 1_000_000_000);
        assert_eq!(USEC_PER_SEC, 1_000_000);
        assert_eq!(NSEC_PER_MSEC, 1_000_000);
        assert_eq!(NSEC_PER_USEC, 1_000);
    }
}

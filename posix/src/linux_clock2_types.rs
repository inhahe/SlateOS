//! `<time.h>` — POSIX clock type constants.
//!
//! `clock_gettime()`, `clock_settime()`, and `clock_getres()`
//! use clock IDs to identify which clock to query.  These
//! constants define the standard and Linux-specific clock IDs.

// ---------------------------------------------------------------------------
// POSIX clock IDs (clockid_t)
// ---------------------------------------------------------------------------

/// System-wide real-time clock (wall clock, settable).
pub const CLOCK_REALTIME: u32 = 0;
/// Monotonic clock (not settable, not affected by NTP).
pub const CLOCK_MONOTONIC: u32 = 1;
/// Per-process CPU-time clock.
pub const CLOCK_PROCESS_CPUTIME_ID: u32 = 2;
/// Per-thread CPU-time clock.
pub const CLOCK_THREAD_CPUTIME_ID: u32 = 3;

// ---------------------------------------------------------------------------
// Linux-specific clock IDs
// ---------------------------------------------------------------------------

/// Monotonic raw (not adjusted by NTP).
pub const CLOCK_MONOTONIC_RAW: u32 = 4;
/// Fast coarse-grained real-time clock.
pub const CLOCK_REALTIME_COARSE: u32 = 5;
/// Fast coarse-grained monotonic clock.
pub const CLOCK_MONOTONIC_COARSE: u32 = 6;
/// Boot-time clock (includes time in suspend).
pub const CLOCK_BOOTTIME: u32 = 7;
/// Real-time alarm clock (wakes from suspend).
pub const CLOCK_REALTIME_ALARM: u32 = 8;
/// Boot-time alarm clock (wakes from suspend).
pub const CLOCK_BOOTTIME_ALARM: u32 = 9;
/// TAI (International Atomic Time) clock.
pub const CLOCK_TAI: u32 = 11;

// ---------------------------------------------------------------------------
// clock_nanosleep flags
// ---------------------------------------------------------------------------

/// Relative timeout (default).
pub const TIMER_RELTIME: u32 = 0;
/// Absolute timeout.
pub const TIMER_ABSTIME: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clocks_distinct() {
        let clocks = [
            CLOCK_REALTIME, CLOCK_MONOTONIC,
            CLOCK_PROCESS_CPUTIME_ID, CLOCK_THREAD_CPUTIME_ID,
            CLOCK_MONOTONIC_RAW, CLOCK_REALTIME_COARSE,
            CLOCK_MONOTONIC_COARSE, CLOCK_BOOTTIME,
            CLOCK_REALTIME_ALARM, CLOCK_BOOTTIME_ALARM,
            CLOCK_TAI,
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
    fn test_monotonic_is_one() {
        assert_eq!(CLOCK_MONOTONIC, 1);
    }

    #[test]
    fn test_boottime_is_seven() {
        assert_eq!(CLOCK_BOOTTIME, 7);
    }

    #[test]
    fn test_timer_flags_distinct() {
        assert_ne!(TIMER_RELTIME, TIMER_ABSTIME);
    }

    #[test]
    fn test_reltime_is_zero() {
        assert_eq!(TIMER_RELTIME, 0);
    }
}

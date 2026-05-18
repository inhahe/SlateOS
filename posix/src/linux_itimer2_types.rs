//! `<sys/time.h>` — Interval timer (itimer) constants.
//!
//! `setitimer()` and `getitimer()` manage recurring timers
//! that deliver signals at fixed intervals.  These constants
//! define the timer types, struct layout, and limits.

// ---------------------------------------------------------------------------
// Interval timer types (which parameter)
// ---------------------------------------------------------------------------

/// Real-time timer (decrements in real time, delivers SIGALRM).
pub const ITIMER_REAL: u32 = 0;
/// Virtual timer (decrements in user CPU time, delivers SIGVTALRM).
pub const ITIMER_VIRTUAL: u32 = 1;
/// Profiling timer (decrements in user+system CPU time, delivers SIGPROF).
pub const ITIMER_PROF: u32 = 2;

// ---------------------------------------------------------------------------
// struct itimerval field offsets (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of it_interval (repeat interval) in struct itimerval.
pub const ITIMERVAL_OFF_INTERVAL: u32 = 0;
/// Offset of it_value (time until next expiration) in struct itimerval.
pub const ITIMERVAL_OFF_VALUE: u32 = 16;
/// Size of struct itimerval (bytes).
pub const ITIMERVAL_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// struct timeval field offsets (used inside itimerval)
// ---------------------------------------------------------------------------

/// Offset of tv_sec in struct timeval.
pub const TIMEVAL_OFF_SEC: u32 = 0;
/// Offset of tv_usec in struct timeval.
pub const TIMEVAL_OFF_USEC: u32 = 8;
/// Size of struct timeval on x86_64 (bytes).
pub const TIMEVAL_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// Timer disable
// ---------------------------------------------------------------------------

/// Zero itimerval disables the timer.
pub const ITIMER_DISABLE_SEC: u64 = 0;
/// Zero itimerval disables the timer (microseconds).
pub const ITIMER_DISABLE_USEC: u64 = 0;

// ---------------------------------------------------------------------------
// Number of timer types
// ---------------------------------------------------------------------------

/// Total number of interval timer types.
pub const ITIMER_COUNT: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_types_distinct() {
        let types = [ITIMER_REAL, ITIMER_VIRTUAL, ITIMER_PROF];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_real_is_zero() {
        assert_eq!(ITIMER_REAL, 0);
    }

    #[test]
    fn test_virtual_is_one() {
        assert_eq!(ITIMER_VIRTUAL, 1);
    }

    #[test]
    fn test_prof_is_two() {
        assert_eq!(ITIMER_PROF, 2);
    }

    #[test]
    fn test_itimerval_layout() {
        assert_eq!(ITIMERVAL_OFF_INTERVAL, 0);
        assert_eq!(ITIMERVAL_OFF_VALUE, 16);
        assert_eq!(ITIMERVAL_SIZE, 32);
    }

    #[test]
    fn test_timeval_layout() {
        assert_eq!(TIMEVAL_OFF_SEC, 0);
        assert_eq!(TIMEVAL_OFF_USEC, 8);
        assert_eq!(TIMEVAL_SIZE, 16);
    }

    #[test]
    fn test_timer_count() {
        assert_eq!(ITIMER_COUNT, 3);
    }

    #[test]
    fn test_itimerval_value_within_struct() {
        assert!(ITIMERVAL_OFF_VALUE < ITIMERVAL_SIZE);
    }
}

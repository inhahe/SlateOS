//! `<unistd.h>` — alarm() and related timer constants.
//!
//! `alarm()` sets a one-shot timer that delivers SIGALRM.
//! `ualarm()` provides microsecond precision.  These constants
//! define the signal numbers, limits, and related values.

// ---------------------------------------------------------------------------
// alarm() signal
// ---------------------------------------------------------------------------

/// Signal delivered by alarm() (SIGALRM).
pub const SIGALRM: u32 = 14;
/// Signal delivered by virtual timer (SIGVTALRM).
pub const SIGVTALRM: u32 = 26;
/// Signal delivered by profiling timer (SIGPROF).
pub const SIGPROF: u32 = 27;

// ---------------------------------------------------------------------------
// alarm() return values
// ---------------------------------------------------------------------------

/// No previous alarm was pending (return value 0).
pub const ALARM_NONE_PENDING: u32 = 0;

// ---------------------------------------------------------------------------
// ualarm() limits
// ---------------------------------------------------------------------------

/// Maximum useconds_t value (fits in 32-bit unsigned).
pub const USECONDS_MAX: u32 = 0xFFFFFFFF;
/// Microseconds per second.
pub const USEC_PER_SEC: u32 = 1000000;

// ---------------------------------------------------------------------------
// sleep() / usleep() / nanosleep()
// ---------------------------------------------------------------------------

/// Maximum sleep duration for sleep() (seconds, fits in unsigned int).
pub const SLEEP_MAX_SECONDS: u32 = 0xFFFFFFFF;
/// Maximum usleep duration (microseconds, POSIX limits to < 1000000).
pub const USLEEP_MAX: u32 = 999999;

// ---------------------------------------------------------------------------
// Nanosecond constants
// ---------------------------------------------------------------------------

/// Nanoseconds per second.
pub const NSEC_PER_SEC: u64 = 1000000000;
/// Nanoseconds per millisecond.
pub const NSEC_PER_MSEC: u64 = 1000000;
/// Nanoseconds per microsecond.
pub const NSEC_PER_USEC: u64 = 1000;
/// Maximum tv_nsec value (one less than 1 billion).
pub const NSEC_MAX: u64 = 999999999;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signals_distinct() {
        let sigs = [SIGALRM, SIGVTALRM, SIGPROF];
        for i in 0..sigs.len() {
            for j in (i + 1)..sigs.len() {
                assert_ne!(sigs[i], sigs[j]);
            }
        }
    }

    #[test]
    fn test_sigalrm_is_fourteen() {
        assert_eq!(SIGALRM, 14);
    }

    #[test]
    fn test_none_pending_is_zero() {
        assert_eq!(ALARM_NONE_PENDING, 0);
    }

    #[test]
    fn test_usec_per_sec() {
        assert_eq!(USEC_PER_SEC, 1000000);
    }

    #[test]
    fn test_nsec_per_sec() {
        assert_eq!(NSEC_PER_SEC, 1000000000);
    }

    #[test]
    fn test_nsec_relationships() {
        assert_eq!(NSEC_PER_SEC, NSEC_PER_MSEC * 1000);
        assert_eq!(NSEC_PER_MSEC, NSEC_PER_USEC * 1000);
    }

    #[test]
    fn test_nsec_max() {
        assert_eq!(NSEC_MAX, NSEC_PER_SEC - 1);
    }

    #[test]
    fn test_usleep_max() {
        assert_eq!(USLEEP_MAX, 999999);
        assert!(USLEEP_MAX < USEC_PER_SEC);
    }
}

//! `<time.h>` — POSIX timer constants.
//!
//! `timer_create()`, `timer_settime()`, `timer_gettime()`, and
//! `timer_delete()` manage per-process interval timers.  These
//! constants define notification methods and timer flags.

// ---------------------------------------------------------------------------
// Timer notification (sigevent sigev_notify)
// ---------------------------------------------------------------------------

/// No notification.
pub const SIGEV_NONE: u32 = 1;
/// Notify via signal.
pub const SIGEV_SIGNAL: u32 = 0;
/// Notify via thread creation.
pub const SIGEV_THREAD: u32 = 2;
/// Notify via thread ID (Linux extension).
pub const SIGEV_THREAD_ID: u32 = 4;

// ---------------------------------------------------------------------------
// timer_settime flags
// ---------------------------------------------------------------------------

/// Relative timer (default).
pub const TIMER_FLAG_RELATIVE: u32 = 0;
/// Absolute timer (TIMER_ABSTIME).
pub const TIMER_FLAG_ABSOLUTE: u32 = 1;

// ---------------------------------------------------------------------------
// Timer limits
// ---------------------------------------------------------------------------

/// Maximum number of timers per process (Linux default).
pub const TIMER_MAX_DEFAULT: u32 = 32768;
/// Maximum overrun count value.
pub const DELAYTIMER_MAX: u32 = 0x7FFFFFFF;

// ---------------------------------------------------------------------------
// struct sigevent layout (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of sigev_value in struct sigevent.
pub const SIGEVENT_OFF_VALUE: u32 = 0;
/// Offset of sigev_signo in struct sigevent.
pub const SIGEVENT_OFF_SIGNO: u32 = 8;
/// Offset of sigev_notify in struct sigevent.
pub const SIGEVENT_OFF_NOTIFY: u32 = 12;
/// Size of struct sigevent (bytes).
pub const SIGEVENT_SIZE: u32 = 64;

// ---------------------------------------------------------------------------
// struct itimerspec layout (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of it_interval in struct itimerspec.
pub const ITIMERSPEC_OFF_INTERVAL: u32 = 0;
/// Offset of it_value in struct itimerspec.
pub const ITIMERSPEC_OFF_VALUE: u32 = 16;
/// Size of struct itimerspec (bytes).
pub const ITIMERSPEC_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_types_distinct() {
        let types = [SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_signal_is_zero() {
        assert_eq!(SIGEV_SIGNAL, 0);
    }

    #[test]
    fn test_timer_flags_distinct() {
        assert_ne!(TIMER_FLAG_RELATIVE, TIMER_FLAG_ABSOLUTE);
    }

    #[test]
    fn test_timer_max() {
        assert_eq!(TIMER_MAX_DEFAULT, 32768);
    }

    #[test]
    fn test_delaytimer_max() {
        assert_eq!(DELAYTIMER_MAX, 0x7FFFFFFF);
    }

    #[test]
    fn test_sigevent_offsets_ascending() {
        assert!(SIGEVENT_OFF_SIGNO > SIGEVENT_OFF_VALUE);
        assert!(SIGEVENT_OFF_NOTIFY > SIGEVENT_OFF_SIGNO);
    }

    #[test]
    fn test_sigevent_within_struct() {
        assert!(SIGEVENT_OFF_NOTIFY < SIGEVENT_SIZE);
    }

    #[test]
    fn test_itimerspec_layout() {
        assert_eq!(ITIMERSPEC_OFF_INTERVAL, 0);
        assert_eq!(ITIMERSPEC_OFF_VALUE, 16);
        assert_eq!(ITIMERSPEC_SIZE, 32);
    }
}

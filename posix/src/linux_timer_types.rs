//! `<linux/time.h>` — POSIX interval timer constants.
//!
//! POSIX timers (timer_create/timer_settime) provide per-process
//! timers that can notify via signals, thread-directed signals, or
//! sigev_thread. They offer more control than setitimer() including
//! choice of clock source and overrun counting.

// ---------------------------------------------------------------------------
// Timer notification methods (sigevent.sigev_notify)
// ---------------------------------------------------------------------------

/// No notification.
pub const SIGEV_NONE: u32 = 1;
/// Send a signal.
pub const SIGEV_SIGNAL: u32 = 0;
/// Deliver via callback thread.
pub const SIGEV_THREAD: u32 = 2;
/// Send signal to specific thread.
pub const SIGEV_THREAD_ID: u32 = 4;

// ---------------------------------------------------------------------------
// setitimer/getitimer which values
// ---------------------------------------------------------------------------

/// Real-time timer (wall clock, delivers SIGALRM).
pub const ITIMER_REAL: u32 = 0;
/// Virtual timer (user CPU time, delivers SIGVTALRM).
pub const ITIMER_VIRTUAL: u32 = 1;
/// Profiling timer (user + system time, delivers SIGPROF).
pub const ITIMER_PROF: u32 = 2;

// ---------------------------------------------------------------------------
// Timer flags (timer_settime)
// ---------------------------------------------------------------------------

/// Relative time (default).
pub const TFD_TIMER_REL: u32 = 0;
/// Absolute time.
pub const TFD_TIMER_ABS: u32 = 1;

// ---------------------------------------------------------------------------
// Signal numbers used by timers
// ---------------------------------------------------------------------------

/// Default signal for POSIX timers.
pub const SIGALRM: u32 = 14;
/// Virtual timer signal.
pub const SIGVTALRM: u32 = 26;
/// Profiling timer signal.
pub const SIGPROF: u32 = 27;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_methods_distinct() {
        let methods = [SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID];
        for i in 0..methods.len() {
            for j in (i + 1)..methods.len() {
                assert_ne!(methods[i], methods[j]);
            }
        }
    }

    #[test]
    fn test_itimer_which_distinct() {
        let which = [ITIMER_REAL, ITIMER_VIRTUAL, ITIMER_PROF];
        for i in 0..which.len() {
            for j in (i + 1)..which.len() {
                assert_ne!(which[i], which[j]);
            }
        }
    }

    #[test]
    fn test_timer_signals_distinct() {
        assert_ne!(SIGALRM, SIGVTALRM);
        assert_ne!(SIGALRM, SIGPROF);
        assert_ne!(SIGVTALRM, SIGPROF);
    }
}

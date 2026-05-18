//! `<linux/posix-timers.h>` / `<time.h>` — POSIX timer and interval constants.
//!
//! POSIX timers (`timer_create`, `timer_settime`) provide per-process
//! or per-thread timers with configurable clock sources and expiry
//! notification methods (signal, thread creation). They are more
//! flexible than the older `setitimer`/`alarm` interfaces.

// ---------------------------------------------------------------------------
// Timer notification methods (sigevent.sigev_notify)
// ---------------------------------------------------------------------------

/// No notification on timer expiry.
pub const SIGEV_NONE: u32 = 1;
/// Deliver a signal on timer expiry.
pub const SIGEV_SIGNAL: u32 = 0;
/// Create a thread on timer expiry.
pub const SIGEV_THREAD: u32 = 2;
/// Deliver signal to a specific thread.
pub const SIGEV_THREAD_ID: u32 = 4;

// ---------------------------------------------------------------------------
// Timer flags (timer_settime flags argument)
// ---------------------------------------------------------------------------

/// Relative time (default).
pub const TIMER_ABSTIME: u32 = 0x01;

// ---------------------------------------------------------------------------
// Interval timer types (setitimer/getitimer)
// ---------------------------------------------------------------------------

/// Real (wall clock) timer — SIGALRM.
pub const ITIMER_REAL: u32 = 0;
/// Virtual (user CPU time) timer — SIGVTALRM.
pub const ITIMER_VIRTUAL: u32 = 1;
/// Profiling (user + system CPU time) timer — SIGPROF.
pub const ITIMER_PROF: u32 = 2;

// ---------------------------------------------------------------------------
// Timer limits
// ---------------------------------------------------------------------------

/// Maximum number of POSIX timers per process (typical default).
pub const TIMER_MAX_PER_PROCESS: u32 = 256;
/// Maximum overrun count.
pub const DELAYTIMER_MAX: u32 = 0x7FFF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sigev_methods_distinct() {
        let methods = [SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID];
        for i in 0..methods.len() {
            for j in (i + 1)..methods.len() {
                assert_ne!(methods[i], methods[j]);
            }
        }
    }

    #[test]
    fn test_itimer_types_distinct() {
        assert_ne!(ITIMER_REAL, ITIMER_VIRTUAL);
        assert_ne!(ITIMER_VIRTUAL, ITIMER_PROF);
        assert_ne!(ITIMER_REAL, ITIMER_PROF);
    }

    #[test]
    fn test_itimer_sequential() {
        assert_eq!(ITIMER_REAL, 0);
        assert_eq!(ITIMER_VIRTUAL, 1);
        assert_eq!(ITIMER_PROF, 2);
    }

    #[test]
    fn test_timer_abstime() {
        assert!(TIMER_ABSTIME.is_power_of_two());
    }

    #[test]
    fn test_delaytimer_max() {
        assert!(DELAYTIMER_MAX > 0);
        assert_eq!(DELAYTIMER_MAX, i32::MAX as u32);
    }
}

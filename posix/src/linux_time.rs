//! `<linux/time.h>` — Linux time types and constants (kernel view).
//!
//! Re-exports clock IDs, time structures, and timer types from the
//! POSIX `time` module and adds Linux-specific clock identifiers.

// ---------------------------------------------------------------------------
// Re-exports from time module
// ---------------------------------------------------------------------------

pub use crate::time::CLOCK_REALTIME;
pub use crate::time::CLOCK_MONOTONIC;
pub use crate::time::CLOCK_PROCESS_CPUTIME_ID;
pub use crate::time::CLOCK_THREAD_CPUTIME_ID;
pub use crate::time::CLOCK_MONOTONIC_RAW;
pub use crate::time::CLOCK_REALTIME_COARSE;
pub use crate::time::CLOCK_MONOTONIC_COARSE;
pub use crate::time::CLOCK_BOOTTIME;

pub use crate::time::Timeval;
pub use crate::time::Itimerspec;

pub use crate::stat::Timespec;
pub use crate::types::ClockidT;

// ---------------------------------------------------------------------------
// Linux-specific clock IDs
// ---------------------------------------------------------------------------

/// Realtime alarm (wakes suspended system).
pub const CLOCK_REALTIME_ALARM: ClockidT = 8;
/// Boot-time alarm.
pub const CLOCK_BOOTTIME_ALARM: ClockidT = 9;
/// TAI (International Atomic Time).
pub const CLOCK_TAI: ClockidT = 11;

// ---------------------------------------------------------------------------
// Timer flags
// ---------------------------------------------------------------------------

/// Absolute time flag for timer_settime / clock_nanosleep.
pub const TIMER_ABSTIME: i32 = 1;

// ---------------------------------------------------------------------------
// Itimerval (for setitimer/getitimer)
// ---------------------------------------------------------------------------

/// Interval timer value (32 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Itimerval {
    /// Timer interval.
    pub it_interval: Timeval,
    /// Current value.
    pub it_value: Timeval,
}

impl Itimerval {
    /// Create a zeroed interval timer value.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// ITIMER_* constants
// ---------------------------------------------------------------------------

/// Real (wall clock) timer.
pub const ITIMER_REAL: i32 = 0;
/// Virtual (user CPU time) timer.
pub const ITIMER_VIRTUAL: i32 = 1;
/// Profiling (user + system CPU time) timer.
pub const ITIMER_PROF: i32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_ids_distinct() {
        let clocks: [ClockidT; 11] = [
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
    fn test_linux_clock_ids() {
        assert_eq!(CLOCK_REALTIME_ALARM, 8);
        assert_eq!(CLOCK_BOOTTIME_ALARM, 9);
        assert_eq!(CLOCK_TAI, 11);
    }

    #[test]
    fn test_timer_abstime() {
        assert_eq!(TIMER_ABSTIME, 1);
    }

    #[test]
    fn test_itimerval_size() {
        assert_eq!(core::mem::size_of::<Itimerval>(), 32);
    }

    #[test]
    fn test_itimer_constants() {
        assert_eq!(ITIMER_REAL, 0);
        assert_eq!(ITIMER_VIRTUAL, 1);
        assert_eq!(ITIMER_PROF, 2);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(CLOCK_REALTIME, crate::time::CLOCK_REALTIME);
        assert_eq!(CLOCK_MONOTONIC, crate::time::CLOCK_MONOTONIC);
        assert_eq!(CLOCK_BOOTTIME, crate::time::CLOCK_BOOTTIME);
    }
}

//! `<sys/time.h>` — time types and operations.
//!
//! Re-exports `Timeval`, `gettimeofday`, `settimeofday`, timer
//! functions, and related constants from the `time` module.

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

pub use crate::time::Itimerval;
pub use crate::time::Timeval;

// ---------------------------------------------------------------------------
// Timer types
// ---------------------------------------------------------------------------

pub use crate::time::ITIMER_PROF;
pub use crate::time::ITIMER_REAL;
pub use crate::time::ITIMER_VIRTUAL;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

pub use crate::time::gettimeofday;
pub use crate::time::settimeofday;

// ---------------------------------------------------------------------------
// Convenience macros as inline functions
// ---------------------------------------------------------------------------

/// Set a timeval to zero.
#[inline]
pub fn timerclear(tvp: &mut Timeval) {
    tvp.tv_sec = 0;
    tvp.tv_usec = 0;
}

/// Test whether a timeval is non-zero.
#[inline]
pub fn timerisset(tvp: &Timeval) -> bool {
    tvp.tv_sec != 0 || tvp.tv_usec != 0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeval_struct_size() {
        assert!(core::mem::size_of::<Timeval>() > 0);
    }

    #[test]
    fn test_itimerval_struct_size() {
        assert!(core::mem::size_of::<Itimerval>() > 0);
    }

    #[test]
    fn test_itimer_constants_distinct() {
        assert_ne!(ITIMER_REAL, ITIMER_VIRTUAL);
        assert_ne!(ITIMER_REAL, ITIMER_PROF);
        assert_ne!(ITIMER_VIRTUAL, ITIMER_PROF);
    }

    #[test]
    fn test_gettimeofday() {
        let mut tv = Timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        let ret = gettimeofday(&mut tv, core::ptr::null_mut());
        // On the bare-metal / test stub, returns -1; on a real system, 0.
        if ret == 0 {
            assert!(tv.tv_sec > 0);
        } else {
            assert_eq!(ret, -1);
        }
    }

    #[test]
    fn test_timerclear() {
        let mut tv = Timeval {
            tv_sec: 42,
            tv_usec: 100,
        };
        timerclear(&mut tv);
        assert_eq!(tv.tv_sec, 0);
        assert_eq!(tv.tv_usec, 0);
    }

    #[test]
    fn test_timerisset() {
        let zero = Timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        assert!(!timerisset(&zero));

        let nonzero = Timeval {
            tv_sec: 1,
            tv_usec: 0,
        };
        assert!(timerisset(&nonzero));

        let usec_only = Timeval {
            tv_sec: 0,
            tv_usec: 500,
        };
        assert!(timerisset(&usec_only));
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(ITIMER_REAL, crate::time::ITIMER_REAL);
        assert_eq!(
            core::mem::size_of::<Timeval>(),
            core::mem::size_of::<crate::time::Timeval>()
        );
    }
}

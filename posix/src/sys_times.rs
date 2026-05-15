//! POSIX `<sys/times.h>` — process times.
//!
//! Implements `times()` for querying process and children CPU times.
//!
//! ## Limitations
//!
//! Since we have no kernel scheduler to query, all CPU time fields
//! are zero and the return value increments based on `clock()`.

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct tms` — process times.
///
/// Times are in clock ticks.  Use `sysconf(_SC_CLK_TCK)` (100 on our
/// OS) to convert to seconds.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Tms {
    /// User CPU time of the process.
    pub tms_utime: i64,
    /// System CPU time of the process.
    pub tms_stime: i64,
    /// User CPU time of terminated children.
    pub tms_cutime: i64,
    /// System CPU time of terminated children.
    pub tms_cstime: i64,
}

/// Clock ticks per second (matches `_SC_CLK_TCK`).
const CLK_TCK: i64 = 100;

// ---------------------------------------------------------------------------
// Monotonic tick counter
// ---------------------------------------------------------------------------

/// Simple monotonic tick counter.
///
/// Each call to `times()` increments this, providing a strictly
/// monotonic elapsed-time value even without a real scheduler.
static mut TICK_COUNTER: i64 = 0;

// ---------------------------------------------------------------------------
// times
// ---------------------------------------------------------------------------

/// `times` — get process times.
///
/// Fills the `tms` structure pointed to by `buffer` with CPU time
/// information.  Returns the elapsed real time in clock ticks
/// since an arbitrary epoch, or `(clock_t)(-1)` on error.
///
/// ## Stub behavior
///
/// All CPU time fields are set to zero (we have no kernel scheduler
/// to query).  The return value is a monotonically increasing tick
/// count.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn times(buffer: *mut Tms) -> i64 {
    if buffer.is_null() {
        crate::errno::set_errno(crate::errno::EFAULT);
        return -1;
    }

    // SAFETY: caller guarantees buffer is valid for one Tms.
    unsafe {
        (*buffer).tms_utime = 0;
        (*buffer).tms_stime = 0;
        (*buffer).tms_cutime = 0;
        (*buffer).tms_cstime = 0;
    }

    // Return a monotonically increasing tick value.
    // SAFETY: single-threaded access.
    unsafe {
        TICK_COUNTER = TICK_COUNTER.wrapping_add(1);
        TICK_COUNTER
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Struct layout
    // -----------------------------------------------------------------------

    #[test]
    fn test_tms_size() {
        // 4 × i64 = 32 bytes.
        assert_eq!(core::mem::size_of::<Tms>(), 4 * 8);
    }

    #[test]
    fn test_tms_fields_default() {
        let tms = Tms {
            tms_utime: 0,
            tms_stime: 0,
            tms_cutime: 0,
            tms_cstime: 0,
        };
        assert_eq!(tms.tms_utime, 0);
        assert_eq!(tms.tms_stime, 0);
        assert_eq!(tms.tms_cutime, 0);
        assert_eq!(tms.tms_cstime, 0);
    }

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_clk_tck() {
        assert_eq!(CLK_TCK, 100);
    }

    // -----------------------------------------------------------------------
    // times — success
    // -----------------------------------------------------------------------

    #[test]
    fn test_times_returns_positive() {
        let mut tms = Tms {
            tms_utime: 99,
            tms_stime: 99,
            tms_cutime: 99,
            tms_cstime: 99,
        };
        let ret = times(&mut tms);
        assert!(ret > 0, "times should return a positive tick count");
    }

    #[test]
    fn test_times_zeroes_fields() {
        let mut tms = Tms {
            tms_utime: 42,
            tms_stime: 42,
            tms_cutime: 42,
            tms_cstime: 42,
        };
        times(&mut tms);
        assert_eq!(tms.tms_utime, 0);
        assert_eq!(tms.tms_stime, 0);
        assert_eq!(tms.tms_cutime, 0);
        assert_eq!(tms.tms_cstime, 0);
    }

    #[test]
    fn test_times_monotonic() {
        let mut tms = Tms {
            tms_utime: 0, tms_stime: 0,
            tms_cutime: 0, tms_cstime: 0,
        };
        let t1 = times(&mut tms);
        let t2 = times(&mut tms);
        let t3 = times(&mut tms);
        assert!(t2 > t1, "tick count should increase");
        assert!(t3 > t2, "tick count should increase");
    }

    // -----------------------------------------------------------------------
    // times — null buffer
    // -----------------------------------------------------------------------

    #[test]
    fn test_times_null_buffer() {
        crate::errno::set_errno(0);
        let ret = times(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -----------------------------------------------------------------------
    // times — multiple calls accumulate
    // -----------------------------------------------------------------------

    #[test]
    fn test_times_increments_each_call() {
        let mut tms = Tms {
            tms_utime: 0, tms_stime: 0,
            tms_cutime: 0, tms_cstime: 0,
        };
        let base = times(&mut tms);
        for i in 1..=5 {
            let t = times(&mut tms);
            assert_eq!(t, base + i, "each call should increment by 1");
        }
    }

    // -----------------------------------------------------------------------
    // times — all CPU fields remain zero (stub)
    // -----------------------------------------------------------------------

    #[test]
    fn test_times_all_fields_zero_on_repeated_calls() {
        for _ in 0..10 {
            let mut tms = Tms {
                tms_utime: 0xFF, tms_stime: 0xFF,
                tms_cutime: 0xFF, tms_cstime: 0xFF,
            };
            times(&mut tms);
            assert_eq!(tms.tms_utime, 0);
            assert_eq!(tms.tms_stime, 0);
            assert_eq!(tms.tms_cutime, 0);
            assert_eq!(tms.tms_cstime, 0);
        }
    }
}

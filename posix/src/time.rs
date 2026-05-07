//! POSIX time functions.
//!
//! Implements `sleep`, `nanosleep`, `clock_gettime`, `gettimeofday`.

use crate::errno;
use crate::stat::Timespec;
use crate::syscall::*;
use crate::types::*;

// ---------------------------------------------------------------------------
// Clock IDs
// ---------------------------------------------------------------------------

/// Monotonic clock (does not set wall time, cannot go backward).
pub const CLOCK_MONOTONIC: ClockidT = 1;
/// Realtime clock (wall clock, can be set).
pub const CLOCK_REALTIME: ClockidT = 0;

// ---------------------------------------------------------------------------
// timeval
// ---------------------------------------------------------------------------

/// Time value with microsecond precision (for gettimeofday).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Timeval {
    /// Seconds since epoch.
    pub tv_sec: TimeT,
    /// Microseconds.
    pub tv_usec: SusecondsT,
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Sleep for a specified number of seconds.
///
/// Returns 0 on success, or the remaining seconds if interrupted.
#[unsafe(no_mangle)]
pub extern "C" fn sleep(seconds: u32) -> u32 {
    // Convert seconds to nanoseconds for our native SYS_SLEEP.
    let ns: u64 = u64::from(seconds).saturating_mul(1_000_000_000);
    let ret = syscall1(SYS_SLEEP, ns);

    if ret < 0 {
        // Sleep was interrupted — return remaining seconds.
        // Our kernel doesn't report remaining time, so return 0.
        0
    } else {
        0
    }
}

/// High-resolution sleep.
///
/// Sleeps for the time specified in `req`.  If interrupted, the
/// remaining time is stored in `rem` (if non-null).
///
/// Returns 0 on success, -1 if interrupted (errno = EINTR).
#[unsafe(no_mangle)]
pub extern "C" fn nanosleep(req: *const Timespec, rem: *mut Timespec) -> i32 {
    if req.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: Caller guarantees req is valid.
    let ts = unsafe { *req };

    // Convert to nanoseconds.
    let ns: u64 = (ts.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(ts.tv_nsec as u64);

    let ret = syscall1(SYS_SLEEP, ns);

    if ret < 0 {
        // Interrupted.  Our kernel doesn't report remaining time,
        // so set rem to zero.
        if !rem.is_null() {
            unsafe {
                (*rem).tv_sec = 0;
                (*rem).tv_nsec = 0;
            }
        }
        errno::set_errno(errno::EINTR);
        return -1;
    }

    0
}

/// Get time from a specific clock.
///
/// Currently only `CLOCK_MONOTONIC` is supported (maps to our
/// native `SYS_CLOCK_MONOTONIC`).
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn clock_gettime(clk_id: ClockidT, tp: *mut Timespec) -> i32 {
    if tp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    match clk_id {
        CLOCK_MONOTONIC | CLOCK_REALTIME => {
            let ns = syscall0(SYS_CLOCK_MONOTONIC);
            if ns < 0 {
                return errno::translate(ns) as i32;
            }

            let ns = ns as u64;
            unsafe {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    (*tp).tv_sec = (ns / 1_000_000_000) as TimeT;
                    (*tp).tv_nsec = (ns % 1_000_000_000) as i64;
                }
            }
            0
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Get time of day (legacy interface).
///
/// Uses `CLOCK_MONOTONIC` since we don't have a wall clock yet.
/// The `tz` parameter is ignored (deprecated in POSIX).
#[unsafe(no_mangle)]
pub extern "C" fn gettimeofday(tv: *mut Timeval, _tz: *mut core::ffi::c_void) -> i32 {
    if tv.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let ns = syscall0(SYS_CLOCK_MONOTONIC);
    if ns < 0 {
        return errno::translate(ns) as i32;
    }

    let ns = ns as u64;
    unsafe {
        #[allow(clippy::arithmetic_side_effects)]
        {
            (*tv).tv_sec = (ns / 1_000_000_000) as TimeT;
            (*tv).tv_usec = ((ns % 1_000_000_000) / 1_000) as SusecondsT;
        }
    }
    0
}

/// Return approximate time in seconds since epoch.
///
/// Uses monotonic clock (not true wall clock).
#[unsafe(no_mangle)]
pub extern "C" fn time(tloc: *mut TimeT) -> TimeT {
    let ns = syscall0(SYS_CLOCK_MONOTONIC);
    if ns < 0 {
        errno::set_errno(errno::EIO);
        return -1;
    }

    #[allow(clippy::arithmetic_side_effects)]
    let secs = (ns as u64 / 1_000_000_000) as TimeT;

    if !tloc.is_null() {
        // SAFETY: Caller guarantees tloc is valid or null (checked above).
        unsafe { *tloc = secs; }
    }

    secs
}

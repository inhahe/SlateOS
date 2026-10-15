//! The one clock `std` does not offer: the time since boot, sleep included.
//!
//! `std::time::Instant` is `CLOCK_MONOTONIC`, which on Linux stops while the
//! machine is suspended. gnulib's last way of finding when the machine booted
//! -- `who -b` on a system that writes no boot entry, SlateOS among them -- is
//! the clock less `CLOCK_BOOTTIME`, which does not stop; this is that call.

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// `CLOCK_BOOTTIME`: 7 in glibc, musl and SlateOS's library alike.
#[cfg(any(unix, test))]
const CLOCK_BOOTTIME: i32 = 7;

#[cfg(unix)]
mod sys {
    /// C's `struct timespec` on x86-64: two 64-bit fields.
    #[repr(C)]
    pub struct Timespec {
        pub tv_sec: i64,
        pub tv_nsec: i64,
    }

    /// C's `struct tm`, as glibc and SlateOS's library lay it out: nine
    /// `int`s, then `tm_gmtoff` and `tm_zone`.
    #[repr(C)]
    pub struct Tm {
        pub tm_sec: i32,
        pub tm_min: i32,
        pub tm_hour: i32,
        pub tm_mday: i32,
        pub tm_mon: i32,
        pub tm_year: i32,
        pub tm_wday: i32,
        pub tm_yday: i32,
        pub tm_isdst: i32,
        pub tm_gmtoff: i64,
        pub tm_zone: *const u8,
    }

    unsafe extern "C" {
        pub fn clock_gettime(clock: i32, tp: *mut Timespec) -> i32;
        pub fn nanosleep(req: *const Timespec, rem: *mut Timespec) -> i32;
        pub fn time(t: *mut i64) -> i64;
        pub fn localtime_r(t: *const i64, tm: *mut Tm) -> *mut Tm;
    }
}

/// A time broken down in the local zone: `struct tm`'s fields, as C has
/// them -- the month from 0, the year since 1900, Sunday day 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Local {
    /// `tm_sec`.
    pub sec: i32,
    /// `tm_min`.
    pub min: i32,
    /// `tm_hour`.
    pub hour: i32,
    /// `tm_mday`.
    pub mday: i32,
    /// `tm_mon`, January 0.
    pub mon: i32,
    /// `tm_year`, years since 1900.
    pub year: i32,
    /// `tm_wday`, Sunday 0.
    pub wday: i32,
}

/// `time (NULL)` and `localtime_r`: now, in the local zone (`TZ`,
/// `/etc/localtime`) as the C library reads it.
///
/// # Errors
///
/// `EOVERFLOW` from `localtime_r`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn local_now() -> Result<Local, i32> {
    #[cfg(unix)]
    {
        // SAFETY: a null argument asks `time` only for its result.
        let now = unsafe { sys::time(core::ptr::null_mut()) };
        let mut tm = sys::Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            tm_gmtoff: 0,
            tm_zone: core::ptr::null(),
        };
        // SAFETY: both pointers are to locals of the C library's layout,
        // alive across the call, which writes `tm` and keeps neither.
        if unsafe { sys::localtime_r(&raw const now, &raw mut tm) }.is_null() {
            return Err(last_errno());
        }
        Ok(Local {
            sec: tm.tm_sec,
            min: tm.tm_min,
            hour: tm.tm_hour,
            mday: tm.tm_mday,
            mon: tm.tm_mon,
            year: tm.tm_year,
            wday: tm.tm_wday,
        })
    }
    #[cfg(not(unix))]
    {
        Err(ENOSYS)
    }
}

/// Sleep for `seconds` and `nanos`, once: `nanosleep`. A signal that a
/// handler catches ends it early with `EINTR`, as it ends C's `usleep` and
/// `sleep` early -- which a program that redraws at once on `SIGWINCH`
/// relies on -- where `std::thread::sleep` would sleep the rest.
///
/// # Errors
///
/// `EINTR` for a sleep a signal ended, `EINVAL` for `nanos` of a second or
/// more; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn nanosleep(seconds: u64, nanos: u32) -> Result<(), i32> {
    nanosleep_one(seconds, nanos)
}

#[cfg(unix)]
fn nanosleep_one(seconds: u64, nanos: u32) -> Result<(), i32> {
    let req = sys::Timespec {
        tv_sec: i64::try_from(seconds).unwrap_or(i64::MAX),
        tv_nsec: i64::from(nanos),
    };
    // SAFETY: `req` is a live `struct timespec` the call only reads; a null
    // `rem` asks for nothing back.
    let rc = unsafe { sys::nanosleep(&raw const req, core::ptr::null_mut()) };
    if rc == 0 { Ok(()) } else { Err(last_errno()) }
}

#[cfg(not(unix))]
fn nanosleep_one(_seconds: u64, _nanos: u32) -> Result<(), i32> {
    Err(ENOSYS)
}

/// How long the machine has been up, suspended time included:
/// `clock_gettime (CLOCK_BOOTTIME)`, as seconds and nanoseconds.
///
/// # Errors
///
/// `EINVAL` from a library without the clock, and
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn since_boot() -> Result<(i64, i64), i32> {
    since_boot_one()
}

#[cfg(unix)]
fn since_boot_one() -> Result<(i64, i64), i32> {
    let mut ts = sys::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a live `struct timespec` that the call writes and
    // nothing else; the clock id is a number.
    let rc = unsafe { sys::clock_gettime(CLOCK_BOOTTIME, &raw mut ts) };
    if rc == 0 {
        Ok((ts.tv_sec, ts.tv_nsec))
    } else {
        Err(last_errno())
    }
}

#[cfg(not(unix))]
fn since_boot_one() -> Result<(i64, i64), i32> {
    Err(ENOSYS)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_clock_id_is_the_librarys() {
        assert_eq!(CLOCK_BOOTTIME, posix::time::CLOCK_BOOTTIME);
    }

    /// The real library: the machine has been up for some time, and a
    /// nanosecond count is less than a second.
    #[cfg(unix)]
    #[test]
    fn the_machine_has_been_up_a_while() {
        let (sec, nsec) = since_boot().unwrap();
        assert!(sec >= 0);
        assert!((0..1_000_000_000).contains(&nsec), "{nsec}");
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_it_declines() {
        assert_eq!(since_boot(), Err(ENOSYS));
    }
}

//! POSIX time functions.
//!
//! Implements `sleep`, `nanosleep`, `usleep`, `clock_gettime`,
//! `clock_getres`, `clock`, `gettimeofday`, `time`, `difftime`,
//! `localtime`, `gmtime`, `mktime`, `asctime`, `ctime`, `strftime`,
//! `strptime`, `timer_create`, `timer_settime`, `timer_gettime`,
//! `timer_delete`, `timer_getoverrun`.
//!
//! ## Timezone
//!
//! Our OS doesn't have timezone support.  All conversions assume UTC.
//! `localtime` and `gmtime` produce identical results.
//!
//! ## POSIX Timers
//!
//! Timer functions (`timer_create`, etc.) are stubs because our OS
//! does not deliver Unix signals.  Programs that create timers will
//! not get callbacks, but the API succeeds so programs that probe
//! for timer support don't fail at startup.

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

/// Sleep for a specified number of microseconds.
///
/// This is obsolete in POSIX.1-2008 (use `nanosleep` instead) but
/// many programs still use it.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn usleep(usec: u32) -> i32 {
    let ns: u64 = u64::from(usec).saturating_mul(1_000);
    let ret = syscall1(SYS_SLEEP, ns);
    if ret < 0 { -1 } else { 0 }
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

/// Get the resolution of a clock.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn clock_getres(clk_id: ClockidT, res: *mut Timespec) -> i32 {
    match clk_id {
        CLOCK_MONOTONIC | CLOCK_REALTIME => {
            if !res.is_null() {
                // Our kernel timer resolution is 1 nanosecond (TSC-based).
                unsafe {
                    (*res).tv_sec = 0;
                    (*res).tv_nsec = 1;
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

/// Set the clock.
///
/// Stub: returns -1 with `EPERM`.  The kernel clock cannot be
/// adjusted from userspace yet.
#[unsafe(no_mangle)]
pub extern "C" fn clock_settime(_clk_id: ClockidT, _tp: *const Timespec) -> i32 {
    errno::set_errno(errno::EPERM);
    -1
}

/// Timer flag: time value is absolute (not relative).
pub const TIMER_ABSTIME: i32 = 1;

/// High-resolution sleep with clock selection.
///
/// If `flags` includes `TIMER_ABSTIME`, `request` is treated as an
/// absolute time point.  Otherwise, it is relative (same as
/// `nanosleep`).
///
/// Returns 0 on success, or an error code (not via errno — POSIX
/// specifies direct return for this function).
#[unsafe(no_mangle)]
pub extern "C" fn clock_nanosleep(
    clk_id: ClockidT,
    flags: i32,
    request: *const Timespec,
    remain: *mut Timespec,
) -> i32 {
    if request.is_null() {
        return errno::EINVAL;
    }

    if clk_id != CLOCK_MONOTONIC && clk_id != CLOCK_REALTIME {
        return errno::EINVAL;
    }

    if flags & TIMER_ABSTIME != 0 {
        // Absolute time: compute the relative duration.
        let mut now = Timespec { tv_sec: 0, tv_nsec: 0 };
        if clock_gettime(clk_id, &raw mut now) < 0 {
            return errno::EINVAL;
        }

        // SAFETY: request is non-null.
        let req = unsafe { &*request };
        #[allow(clippy::arithmetic_side_effects)]
        let target_ns = req.tv_sec * 1_000_000_000 + req.tv_nsec;
        #[allow(clippy::arithmetic_side_effects)]
        let now_ns = now.tv_sec * 1_000_000_000 + now.tv_nsec;

        if target_ns <= now_ns {
            return 0; // Already past.
        }

        #[allow(clippy::arithmetic_side_effects)]
        let sleep_ns = (target_ns - now_ns) as u64;
        let _ = syscall1(SYS_SLEEP, sleep_ns);
    } else {
        // Relative time: same as nanosleep.
        nanosleep(request, remain);
    }

    0
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

/// Set the system clock.
///
/// Stub: returns -1 with `EPERM`.  The kernel clock cannot be adjusted
/// from userspace yet.
#[unsafe(no_mangle)]
pub extern "C" fn settimeofday(
    _tv: *const Timeval,
    _tz: *const core::ffi::c_void,
) -> i32 {
    errno::set_errno(errno::EPERM);
    -1
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

/// Compute the difference between two time_t values.
#[unsafe(no_mangle)]
#[allow(clippy::cast_precision_loss)]
// Precision loss is acceptable for difftime — POSIX defines it as
// returning double, and time differences rarely need 52-bit precision.
pub extern "C" fn difftime(time1: TimeT, time0: TimeT) -> f64 {
    (time1.wrapping_sub(time0)) as f64
}

// ---------------------------------------------------------------------------
// Broken-down time
// ---------------------------------------------------------------------------

/// Broken-down time (struct tm).
#[repr(C)]
pub struct Tm {
    /// Seconds [0, 60] (60 for leap second).
    pub tm_sec: i32,
    /// Minutes [0, 59].
    pub tm_min: i32,
    /// Hours [0, 23].
    pub tm_hour: i32,
    /// Day of month [1, 31].
    pub tm_mday: i32,
    /// Month [0, 11] (January = 0).
    pub tm_mon: i32,
    /// Years since 1900.
    pub tm_year: i32,
    /// Day of week [0, 6] (Sunday = 0).
    pub tm_wday: i32,
    /// Day of year [0, 365].
    pub tm_yday: i32,
    /// Daylight saving flag (0 = not in effect).
    pub tm_isdst: i32,
}

/// Static storage for gmtime/localtime (not thread-safe per POSIX).
static mut TM_RESULT: Tm = Tm {
    tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0,
    tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0,
    tm_isdst: 0,
};

/// Convert time_t to broken-down UTC time.
///
/// Returns a pointer to a static Tm (not thread-safe).
#[unsafe(no_mangle)]
pub extern "C" fn gmtime(timep: *const TimeT) -> *mut Tm {
    if timep.is_null() {
        return core::ptr::null_mut();
    }
    let secs = unsafe { *timep };
    // SAFETY: Single-threaded access to static storage.
    let tm = unsafe { &mut *core::ptr::addr_of_mut!(TM_RESULT) };
    secs_to_tm(secs, tm);
    core::ptr::addr_of_mut!(*tm)
}

/// Convert time_t to broken-down local time.
///
/// We don't have timezone support, so this returns UTC.
#[unsafe(no_mangle)]
pub extern "C" fn localtime(timep: *const TimeT) -> *mut Tm {
    // No timezone — UTC is local time.
    gmtime(timep)
}

/// Convert broken-down time to time_t.
///
/// Normalizes the Tm fields and returns seconds since epoch.
#[unsafe(no_mangle)]
pub extern "C" fn mktime(tm: *mut Tm) -> TimeT {
    if tm.is_null() {
        return -1;
    }
    let t = unsafe { &mut *tm };
    tm_to_secs(t)
}

/// Convert broken-down UTC time to seconds since epoch.
///
/// Like `mktime` but always interprets the Tm as UTC (no timezone
/// adjustment).  Since our OS is always UTC, this is identical to
/// `mktime`.
#[unsafe(no_mangle)]
pub extern "C" fn timegm(tm: *mut Tm) -> TimeT {
    mktime(tm)
}

/// Convert broken-down time to string.
///
/// Returns a pointer to a static string in the format
/// "Wed Jun 30 21:49:08 1993\n\0".
#[unsafe(no_mangle)]
pub extern "C" fn asctime(tm: *const Tm) -> *const u8 {
    if tm.is_null() {
        return c"??? ??? ?? ??:??:?? ????\n".as_ptr().cast::<u8>();
    }

    let t = unsafe { &*tm };

    // SAFETY: Single-threaded access to static buffer.
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(ASCTIME_BUF) };
    let len = format_asctime(t, buf);
    let _ = len; // We always null-terminate.
    buf.as_ptr()
}

/// Static buffer for asctime.
static mut ASCTIME_BUF: [u8; 32] = [0u8; 32];

/// Convert time_t to string.
///
/// Equivalent to `asctime(localtime(timep))`.
#[unsafe(no_mangle)]
pub extern "C" fn ctime(timep: *const TimeT) -> *const u8 {
    asctime(localtime(timep))
}

/// Format time according to a format string.
///
/// Supports a subset of strftime conversions:
/// `%Y` (year), `%m` (month), `%d` (day), `%H` (hour), `%M` (minute),
/// `%S` (second), `%A`/`%a` (weekday), `%B`/`%b` (month name),
/// `%c` (date+time), `%p` (AM/PM), `%j` (day of year), `%n` (newline),
/// `%t` (tab), `%%` (percent).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_lines)]
pub unsafe extern "C" fn strftime(
    buf: *mut u8,
    maxsize: usize,
    fmt: *const u8,
    tm: *const Tm,
) -> usize {
    if buf.is_null() || fmt.is_null() || tm.is_null() || maxsize == 0 {
        return 0;
    }

    let t = unsafe { &*tm };
    let mut pos: usize = 0;
    let mut fpos: usize = 0;
    let limit = maxsize.wrapping_sub(1); // Reserve space for null terminator.

    loop {
        let ch = unsafe { *fmt.add(fpos) };
        if ch == 0 {
            break;
        }

        if ch != b'%' {
            if pos < limit {
                unsafe { *buf.add(pos) = ch; }
            }
            pos = pos.wrapping_add(1);
            fpos = fpos.wrapping_add(1);
            continue;
        }

        fpos = fpos.wrapping_add(1);
        let spec = unsafe { *fmt.add(fpos) };
        if spec == 0 {
            break;
        }
        fpos = fpos.wrapping_add(1);

        match spec {
            b'Y' => pos = write_dec4(buf, limit, pos, t.tm_year.wrapping_add(1900)),
            b'm' => pos = write_dec2(buf, limit, pos, t.tm_mon.wrapping_add(1)),
            b'd' => pos = write_dec2(buf, limit, pos, t.tm_mday),
            b'H' => pos = write_dec2(buf, limit, pos, t.tm_hour),
            b'M' => pos = write_dec2(buf, limit, pos, t.tm_min),
            b'S' => pos = write_dec2(buf, limit, pos, t.tm_sec),
            b'j' => pos = write_dec3(buf, limit, pos, t.tm_yday.wrapping_add(1)),
            b'A' => pos = write_str(buf, limit, pos, wday_full(t.tm_wday)),
            b'a' => pos = write_str(buf, limit, pos, wday_abbr(t.tm_wday)),
            b'B' => pos = write_str(buf, limit, pos, mon_full(t.tm_mon)),
            b'b' | b'h' => pos = write_str(buf, limit, pos, mon_abbr(t.tm_mon)),
            b'p' => {
                let label = if t.tm_hour < 12 { b"AM" } else { b"PM" };
                pos = write_str(buf, limit, pos, label);
            }
            b'c' => {
                // "Thu Jan  1 00:00:00 1970" format.
                pos = write_str(buf, limit, pos, wday_abbr(t.tm_wday));
                pos = write_char(buf, limit, pos, b' ');
                pos = write_str(buf, limit, pos, mon_abbr(t.tm_mon));
                pos = write_char(buf, limit, pos, b' ');
                pos = write_dec2(buf, limit, pos, t.tm_mday);
                pos = write_char(buf, limit, pos, b' ');
                pos = write_dec2(buf, limit, pos, t.tm_hour);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_min);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_sec);
                pos = write_char(buf, limit, pos, b' ');
                pos = write_dec4(buf, limit, pos, t.tm_year.wrapping_add(1900));
            }
            b'n' => {
                pos = write_char(buf, limit, pos, b'\n');
            }
            b't' => {
                pos = write_char(buf, limit, pos, b'\t');
            }
            b'%' => {
                pos = write_char(buf, limit, pos, b'%');
            }
            _ => {
                // Unknown — pass through.
                pos = write_char(buf, limit, pos, b'%');
                pos = write_char(buf, limit, pos, spec);
            }
        }
    }

    // Null-terminate.
    let term = if pos < maxsize { pos } else { limit };
    unsafe { *buf.add(term) = 0; }

    if pos > limit { 0 } else { pos }
}

// ---------------------------------------------------------------------------
// Time conversion helpers
// ---------------------------------------------------------------------------

/// Days in each month (non-leap year).
const DAYS_IN_MONTH: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Check if a year is a leap year.
#[inline]
fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in a given month (1-indexed month, with leap year check).
#[inline]
fn days_in_month(mon: i32, year: i32) -> i32 {
    if mon == 1 && is_leap(year) {
        29
    } else {
        DAYS_IN_MONTH.get(mon as usize).copied().unwrap_or(30)
    }
}

/// Convert seconds since epoch (1970-01-01 00:00:00 UTC) to broken-down time.
#[allow(clippy::arithmetic_side_effects)]
fn secs_to_tm(secs: TimeT, tm: &mut Tm) {
    let mut rem = secs;

    // Seconds, minutes, hours.
    tm.tm_sec = (rem % 60) as i32;
    rem /= 60;
    tm.tm_min = (rem % 60) as i32;
    rem /= 60;
    tm.tm_hour = (rem % 24) as i32;
    rem /= 24;

    // rem is now days since epoch.
    // 1970-01-01 was a Thursday (wday=4).
    tm.tm_wday = ((rem + 4) % 7) as i32;
    if tm.tm_wday < 0 {
        tm.tm_wday += 7;
    }

    // Compute year and day-of-year.
    let mut year: i32 = 1970;
    loop {
        let days_this_year = if is_leap(year) { 366 } else { 365 };
        if rem < i64::from(days_this_year) {
            break;
        }
        rem -= i64::from(days_this_year);
        year += 1;
    }

    tm.tm_year = year - 1900;
    tm.tm_yday = rem as i32;

    // Compute month and day.
    let mut mon: i32 = 0;
    let mut remaining_days = rem as i32;
    while mon < 11 {
        let dim = days_in_month(mon, year);
        if remaining_days < dim {
            break;
        }
        remaining_days -= dim;
        mon += 1;
    }

    tm.tm_mon = mon;
    tm.tm_mday = remaining_days + 1;
    tm.tm_isdst = 0;
}

/// Convert broken-down time to seconds since epoch.
#[allow(clippy::arithmetic_side_effects)]
fn tm_to_secs(tm: &mut Tm) -> TimeT {
    let year = tm.tm_year + 1900;

    // Count days from 1970 to the start of `year`.
    let mut days: i64 = 0;
    if year > 1970 {
        let mut y = 1970;
        while y < year {
            days += if is_leap(y) { 366 } else { 365 };
            y += 1;
        }
    }

    // Add days for months.
    let mut mon = 0;
    while mon < tm.tm_mon {
        days += i64::from(days_in_month(mon, year));
        mon += 1;
    }

    // Day of month (1-based).
    days += i64::from(tm.tm_mday - 1);

    // Update tm_yday and tm_wday.
    tm.tm_yday = 0;
    let mut m2 = 0;
    while m2 < tm.tm_mon {
        tm.tm_yday += days_in_month(m2, year);
        m2 += 1;
    }
    tm.tm_yday += tm.tm_mday - 1;

    tm.tm_wday = ((days + 4) % 7) as i32;
    if tm.tm_wday < 0 {
        tm.tm_wday += 7;
    }

    days * 86400 + i64::from(tm.tm_hour) * 3600 + i64::from(tm.tm_min) * 60 + i64::from(tm.tm_sec)
}

/// Format asctime output into buffer.
fn format_asctime(tm: &Tm, buf: &mut [u8; 32]) -> usize {
    let mut pos: usize = 0;
    let limit = buf.len().wrapping_sub(1);

    pos = write_str(buf.as_mut_ptr(), limit, pos, wday_abbr(tm.tm_wday));
    pos = write_char(buf.as_mut_ptr(), limit, pos, b' ');
    pos = write_str(buf.as_mut_ptr(), limit, pos, mon_abbr(tm.tm_mon));
    pos = write_char(buf.as_mut_ptr(), limit, pos, b' ');
    pos = write_dec2(buf.as_mut_ptr(), limit, pos, tm.tm_mday);
    pos = write_char(buf.as_mut_ptr(), limit, pos, b' ');
    pos = write_dec2(buf.as_mut_ptr(), limit, pos, tm.tm_hour);
    pos = write_char(buf.as_mut_ptr(), limit, pos, b':');
    pos = write_dec2(buf.as_mut_ptr(), limit, pos, tm.tm_min);
    pos = write_char(buf.as_mut_ptr(), limit, pos, b':');
    pos = write_dec2(buf.as_mut_ptr(), limit, pos, tm.tm_sec);
    pos = write_char(buf.as_mut_ptr(), limit, pos, b' ');
    pos = write_dec4(buf.as_mut_ptr(), limit, pos, tm.tm_year.wrapping_add(1900));
    pos = write_char(buf.as_mut_ptr(), limit, pos, b'\n');

    if pos < buf.len() {
        if let Some(slot) = buf.get_mut(pos) {
            *slot = 0;
        }
    } else if let Some(slot) = buf.last_mut() {
        *slot = 0;
    }

    pos
}

// ---------------------------------------------------------------------------
// String tables
// ---------------------------------------------------------------------------

fn wday_abbr(wday: i32) -> &'static [u8] {
    match wday {
        0 => b"Sun", 1 => b"Mon", 2 => b"Tue", 3 => b"Wed",
        4 => b"Thu", 5 => b"Fri", 6 => b"Sat", _ => b"???",
    }
}

fn wday_full(wday: i32) -> &'static [u8] {
    match wday {
        0 => b"Sunday", 1 => b"Monday", 2 => b"Tuesday",
        3 => b"Wednesday", 4 => b"Thursday", 5 => b"Friday",
        6 => b"Saturday", _ => b"???",
    }
}

fn mon_abbr(mon: i32) -> &'static [u8] {
    match mon {
        0 => b"Jan", 1 => b"Feb", 2 => b"Mar", 3 => b"Apr",
        4 => b"May", 5 => b"Jun", 6 => b"Jul", 7 => b"Aug",
        8 => b"Sep", 9 => b"Oct", 10 => b"Nov", 11 => b"Dec",
        _ => b"???",
    }
}

fn mon_full(mon: i32) -> &'static [u8] {
    match mon {
        0 => b"January", 1 => b"February", 2 => b"March",
        3 => b"April", 4 => b"May", 5 => b"June",
        6 => b"July", 7 => b"August", 8 => b"September",
        9 => b"October", 10 => b"November", 11 => b"December",
        _ => b"???",
    }
}

// ---------------------------------------------------------------------------
// strftime helpers
// ---------------------------------------------------------------------------

/// Write a single character to a buffer.
fn write_char(buf: *mut u8, limit: usize, pos: usize, ch: u8) -> usize {
    if pos < limit {
        unsafe { *buf.add(pos) = ch; }
    }
    pos.wrapping_add(1)
}

/// Write a byte slice to a buffer.
fn write_str(buf: *mut u8, limit: usize, mut pos: usize, data: &[u8]) -> usize {
    for &byte in data {
        if pos < limit {
            unsafe { *buf.add(pos) = byte; }
        }
        pos = pos.wrapping_add(1);
    }
    pos
}

/// Write a 2-digit zero-padded decimal.
fn write_dec2(buf: *mut u8, limit: usize, pos: usize, val: i32) -> usize {
    let v = if val < 0 { 0 } else { val as u32 };
    let d1 = b'0'.wrapping_add((v.wrapping_div(10) % 10) as u8);
    let d0 = b'0'.wrapping_add((v % 10) as u8);
    let p1 = write_char(buf, limit, pos, d1);
    write_char(buf, limit, p1, d0)
}

/// Write a 3-digit zero-padded decimal.
fn write_dec3(buf: *mut u8, limit: usize, pos: usize, val: i32) -> usize {
    let v = if val < 0 { 0 } else { val as u32 };
    let d2 = b'0'.wrapping_add((v.wrapping_div(100) % 10) as u8);
    let d1 = b'0'.wrapping_add((v.wrapping_div(10) % 10) as u8);
    let d0 = b'0'.wrapping_add((v % 10) as u8);
    let p2 = write_char(buf, limit, pos, d2);
    let p1 = write_char(buf, limit, p2, d1);
    write_char(buf, limit, p1, d0)
}

/// Write a 4-digit zero-padded year.
fn write_dec4(buf: *mut u8, limit: usize, pos: usize, val: i32) -> usize {
    let v = if val < 0 { 0 } else { val as u32 };
    let d3 = b'0'.wrapping_add((v.wrapping_div(1000) % 10) as u8);
    let d2 = b'0'.wrapping_add((v.wrapping_div(100) % 10) as u8);
    let d1 = b'0'.wrapping_add((v.wrapping_div(10) % 10) as u8);
    let d0 = b'0'.wrapping_add((v % 10) as u8);
    let p3 = write_char(buf, limit, pos, d3);
    let p2 = write_char(buf, limit, p3, d2);
    let p1 = write_char(buf, limit, p2, d1);
    write_char(buf, limit, p1, d0)
}

// ---------------------------------------------------------------------------
// clock — CPU time
// ---------------------------------------------------------------------------

/// `CLOCKS_PER_SEC` for the `clock()` function.
///
/// POSIX requires this to be 1,000,000.
#[unsafe(no_mangle)]
pub static CLOCKS_PER_SEC: i64 = 1_000_000;

/// Return an approximation of CPU time used by the process.
///
/// Returns microseconds elapsed since an arbitrary point (we use
/// `CLOCK_MONOTONIC` as a proxy since we don't track per-process
/// CPU time yet).  Returns -1 on failure.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn clock() -> i64 {
    let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
    if clock_gettime(CLOCK_MONOTONIC, &raw mut ts) != 0 {
        return -1;
    }
    // Convert to microseconds (CLOCKS_PER_SEC = 1_000_000).
    ts.tv_sec * 1_000_000 + ts.tv_nsec / 1_000
}

// ---------------------------------------------------------------------------
// strptime — parse time strings
// ---------------------------------------------------------------------------

/// Parse a time string according to a format.
///
/// Inverse of `strftime`.  Reads from `buf` according to `format`,
/// filling fields in `tm`.  Returns a pointer to the first character
/// not consumed, or NULL if the input doesn't match.
///
/// Supports: `%Y`, `%m`, `%d`, `%H`, `%M`, `%S`, `%j`, `%n`, `%t`, `%%`.
///
/// # Safety
///
/// `buf` and `format` must be valid null-terminated strings.
/// `tm` must point to a valid `Tm`.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn strptime(
    buf: *const u8,
    format: *const u8,
    tm: *mut Tm,
) -> *const u8 {
    if buf.is_null() || format.is_null() || tm.is_null() {
        return core::ptr::null();
    }

    let mut bi: usize = 0; // Index into buf.
    let mut fi: usize = 0; // Index into format.

    loop {
        let fc = unsafe { *format.add(fi) };
        if fc == 0 {
            // End of format — success. Return pointer to remaining input.
            return unsafe { buf.add(bi) };
        }

        if fc == b'%' {
            fi = fi.wrapping_add(1);
            let spec = unsafe { *format.add(fi) };
            if spec == 0 {
                return core::ptr::null();
            }
            fi = fi.wrapping_add(1);

            match spec {
                b'Y' => {
                    // 4-digit year.
                    let (val, consumed) = parse_int(buf, bi, 4);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_year = val - 1900; }
                    bi = bi.wrapping_add(consumed);
                }
                b'm' => {
                    // Month 01-12.
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_mon = val - 1; }
                    bi = bi.wrapping_add(consumed);
                }
                b'd' => {
                    // Day 01-31.
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_mday = val; }
                    bi = bi.wrapping_add(consumed);
                }
                b'H' => {
                    // Hour 00-23.
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_hour = val; }
                    bi = bi.wrapping_add(consumed);
                }
                b'M' => {
                    // Minute 00-59.
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_min = val; }
                    bi = bi.wrapping_add(consumed);
                }
                b'S' => {
                    // Second 00-60.
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_sec = val; }
                    bi = bi.wrapping_add(consumed);
                }
                b'j' => {
                    // Day of year 001-366.
                    let (val, consumed) = parse_int(buf, bi, 3);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_yday = val - 1; }
                    bi = bi.wrapping_add(consumed);
                }
                b'n' | b't' => {
                    // Skip any whitespace.
                    while (unsafe { *buf.add(bi) }) == b' '
                        || (unsafe { *buf.add(bi) }) == b'\t'
                    {
                        bi = bi.wrapping_add(1);
                    }
                }
                b'%' => {
                    // Literal %.
                    if unsafe { *buf.add(bi) } != b'%' {
                        return core::ptr::null();
                    }
                    bi = bi.wrapping_add(1);
                }
                _ => {
                    // Unknown specifier — fail.
                    return core::ptr::null();
                }
            }
        } else if fc == b' ' || fc == b'\t' {
            // Whitespace in format matches any amount of whitespace in buf.
            while (unsafe { *buf.add(bi) }) == b' '
                || (unsafe { *buf.add(bi) }) == b'\t'
            {
                bi = bi.wrapping_add(1);
            }
            fi = fi.wrapping_add(1);
        } else {
            // Literal character — must match.
            if unsafe { *buf.add(bi) } != fc {
                return core::ptr::null();
            }
            bi = bi.wrapping_add(1);
            fi = fi.wrapping_add(1);
        }
    }
}

/// Parse up to `max_digits` decimal digits from `buf` starting at offset `off`.
///
/// Returns (value, number_of_digits_consumed).
#[allow(clippy::arithmetic_side_effects)]
fn parse_int(buf: *const u8, off: usize, max_digits: usize) -> (i32, usize) {
    let mut val: i32 = 0;
    let mut count: usize = 0;
    while count < max_digits {
        let c = unsafe { *buf.add(off.wrapping_add(count)) };
        if !c.is_ascii_digit() {
            break;
        }
        val = val * 10 + i32::from(c.wrapping_sub(b'0'));
        count = count.wrapping_add(1);
    }
    (val, count)
}

// ---------------------------------------------------------------------------
// POSIX per-process timers (stubs)
// ---------------------------------------------------------------------------
//
// Our OS does not deliver Unix signals, so timer expiration callbacks
// never fire.  These stubs allow programs that create timers at
// startup (e.g., for profiling or heartbeat) to link and run.

/// Timer ID type.
pub type TimerT = i32;

/// Timer specification (interval + initial expiration).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Itimerspec {
    /// Interval for periodic timer (0 = one-shot).
    pub it_interval: Timespec,
    /// Initial expiration time.
    pub it_value: Timespec,
}

/// Signal event specification for `timer_create`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Sigevent {
    /// Notification value.
    pub sigev_value: usize,
    /// Signal number to deliver.
    pub sigev_signo: i32,
    /// Notification method (SIGEV_NONE, SIGEV_SIGNAL, etc.).
    pub sigev_notify: i32,
    /// Padding for ABI compatibility.
    _pad: [u8; 48],
}

/// No notification on timer expiration.
pub const SIGEV_NONE: i32 = 1;
/// Deliver a signal on timer expiration.
pub const SIGEV_SIGNAL: i32 = 0;
/// Start a thread on timer expiration.
pub const SIGEV_THREAD: i32 = 2;

/// Maximum number of timers per process.
const MAX_TIMERS: usize = 32;

/// Timer state table.
///
/// Each slot holds the timer's itimerspec (or zeros if unused).
/// Timer IDs are indices into this table.
static mut TIMER_TABLE: [Option<Itimerspec>; MAX_TIMERS] = [None; MAX_TIMERS];

/// Create a per-process timer.
///
/// Allocates a timer ID and stores it in `*timerid`.  The timer
/// never actually fires (no signal delivery), but the API succeeds.
#[unsafe(no_mangle)]
pub extern "C" fn timer_create(
    _clockid: ClockidT,
    _sevp: *const Sigevent,
    timerid: *mut TimerT,
) -> i32 {
    if timerid.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Find a free slot.
    // SAFETY: single-threaded access by convention.
    let table = unsafe { core::ptr::addr_of_mut!(TIMER_TABLE).as_mut() };
    let Some(table) = table else {
        errno::set_errno(errno::ENOMEM);
        return -1;
    };

    for (idx, slot) in table.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(Itimerspec {
                it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
                it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
            });
            // SAFETY: timerid verified non-null above; idx fits in i32.
            unsafe { *timerid = idx as TimerT; }
            return 0;
        }
    }

    errno::set_errno(errno::EAGAIN);
    -1
}

/// Arm or disarm a per-process timer.
///
/// Stores the new value and returns the old value (if `old_value` is
/// non-null).  The timer never actually fires.
#[unsafe(no_mangle)]
pub extern "C" fn timer_settime(
    timerid: TimerT,
    _flags: i32,
    new_value: *const Itimerspec,
    old_value: *mut Itimerspec,
) -> i32 {
    if new_value.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let table = unsafe { core::ptr::addr_of_mut!(TIMER_TABLE).as_mut() };
    let Some(table) = table else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let Some(slot) = table.get_mut(timerid as usize) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let Some(ref current) = *slot else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    // Return old value if requested.
    if !old_value.is_null() {
        // SAFETY: old_value verified non-null.
        unsafe { *old_value = *current; }
    }

    // Store new value.
    // SAFETY: new_value verified non-null.
    *slot = Some(unsafe { *new_value });
    0
}

/// Get the remaining time on a timer.
///
/// Always returns zeros (timers don't actually run).
#[unsafe(no_mangle)]
pub extern "C" fn timer_gettime(timerid: TimerT, curr_value: *mut Itimerspec) -> i32 {
    if curr_value.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let table = unsafe { core::ptr::addr_of_mut!(TIMER_TABLE).as_mut() };
    let Some(table) = table else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let Some(slot) = table.get(timerid as usize) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    if let Some(ref its) = *slot {
        // SAFETY: curr_value verified non-null.
        unsafe { *curr_value = *its; }
    } else {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Delete a per-process timer.
#[unsafe(no_mangle)]
pub extern "C" fn timer_delete(timerid: TimerT) -> i32 {
    let table = unsafe { core::ptr::addr_of_mut!(TIMER_TABLE).as_mut() };
    let Some(table) = table else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let Some(slot) = table.get_mut(timerid as usize) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    if slot.is_none() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    *slot = None;
    0
}

/// Get the overrun count for a timer.
///
/// Always returns 0 (timers don't actually fire).
#[unsafe(no_mangle)]
pub extern "C" fn timer_getoverrun(_timerid: TimerT) -> i32 {
    0
}

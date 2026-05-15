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

/// Realtime clock (wall clock, can be set).
pub const CLOCK_REALTIME: ClockidT = 0;
/// Monotonic clock (does not set wall time, cannot go backward).
pub const CLOCK_MONOTONIC: ClockidT = 1;
/// Process-wide CPU-time clock.
///
/// Programs use this to measure their own CPU usage.  We map it to
/// CLOCK_MONOTONIC since we don't have per-process CPU accounting yet.
pub const CLOCK_PROCESS_CPUTIME_ID: ClockidT = 2;
/// Thread-specific CPU-time clock.
///
/// We map it to CLOCK_MONOTONIC (single-threaded, no per-thread
/// accounting yet).
pub const CLOCK_THREAD_CPUTIME_ID: ClockidT = 3;
/// Like CLOCK_MONOTONIC but provides raw hardware time without NTP
/// adjustments.  We use the same monotonic source for all clocks.
pub const CLOCK_MONOTONIC_RAW: ClockidT = 4;
/// Coarse (fast but lower resolution) realtime clock.  We return
/// the same precision as CLOCK_REALTIME.
pub const CLOCK_REALTIME_COARSE: ClockidT = 5;
/// Coarse (fast but lower resolution) monotonic clock.
pub const CLOCK_MONOTONIC_COARSE: ClockidT = 6;
/// Time since boot (includes time spent suspended).  Maps to our
/// monotonic clock (we don't track suspend time separately).
pub const CLOCK_BOOTTIME: ClockidT = 7;

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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nanosleep(req: *const Timespec, rem: *mut Timespec) -> i32 {
    if req.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: Caller guarantees req is valid.
    let ts = unsafe { *req };

    // POSIX: EINVAL if tv_nsec not in [0, 999_999_999] or tv_sec < 0.
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec > 999_999_999 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Convert to nanoseconds (both values are now non-negative, cast is safe).
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn usleep(usec: u32) -> i32 {
    let ns: u64 = u64::from(usec).saturating_mul(1_000);
    let ret = syscall1(SYS_SLEEP, ns);
    if ret < 0 { -1 } else { 0 }
}

/// Get time from a specific clock.
///
/// All supported clock IDs (`CLOCK_REALTIME`, `CLOCK_MONOTONIC`,
/// `CLOCK_PROCESS_CPUTIME_ID`, `CLOCK_THREAD_CPUTIME_ID`,
/// `CLOCK_MONOTONIC_RAW`, `CLOCK_*_COARSE`, `CLOCK_BOOTTIME`)
/// currently map to the same underlying monotonic clock.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_gettime(clk_id: ClockidT, tp: *mut Timespec) -> i32 {
    if tp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    if !is_valid_clock(clk_id) {
        errno::set_errno(errno::EINVAL);
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
            (*tp).tv_sec = (ns / 1_000_000_000) as TimeT;
            (*tp).tv_nsec = (ns % 1_000_000_000) as i64;
        }
    }
    0
}

/// Check whether a clock ID is one we recognize.
fn is_valid_clock(clk_id: ClockidT) -> bool {
    matches!(
        clk_id,
        CLOCK_REALTIME
            | CLOCK_MONOTONIC
            | CLOCK_PROCESS_CPUTIME_ID
            | CLOCK_THREAD_CPUTIME_ID
            | CLOCK_MONOTONIC_RAW
            | CLOCK_REALTIME_COARSE
            | CLOCK_MONOTONIC_COARSE
            | CLOCK_BOOTTIME
    )
}

/// Get the resolution of a clock.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_getres(clk_id: ClockidT, res: *mut Timespec) -> i32 {
    if !is_valid_clock(clk_id) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    if !res.is_null() {
        // Our kernel timer resolution is 1 nanosecond (TSC-based).
        unsafe {
            (*res).tv_sec = 0;
            (*res).tv_nsec = 1;
        }
    }
    0
}

/// Set the clock.
///
/// Stub: returns -1 with `EPERM`.  The kernel clock cannot be
/// adjusted from userspace yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_nanosleep(
    clk_id: ClockidT,
    flags: i32,
    request: *const Timespec,
    remain: *mut Timespec,
) -> i32 {
    if request.is_null() {
        return errno::EINVAL;
    }

    if !is_valid_clock(clk_id) {
        return errno::EINVAL;
    }

    // POSIX: EINVAL if tv_nsec not in [0, 999_999_999].
    // SAFETY: request is non-null (checked above).
    let req = unsafe { &*request };
    if req.tv_nsec < 0 || req.tv_nsec > 999_999_999 {
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
        // Propagate EINTR if interrupted (clock_nanosleep returns error
        // codes directly, not via errno).
        if nanosleep(request, remain) != 0 {
            return errno::EINTR;
        }
    }

    0
}

/// Get time of day (legacy interface).
///
/// Uses `CLOCK_MONOTONIC` since we don't have a wall clock yet.
/// The `tz` parameter is ignored (deprecated in POSIX).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_precision_loss)]
// Precision loss is acceptable for difftime — POSIX defines it as
// returning double, and time differences rarely need 52-bit precision.
pub extern "C" fn difftime(time1: TimeT, time0: TimeT) -> f64 {
    (time1.wrapping_sub(time0)) as f64
}

// ---------------------------------------------------------------------------
// Timezone globals (POSIX)
// ---------------------------------------------------------------------------

/// Sync wrapper for `*const u8` in static arrays.
///
/// Our pointers are to static string literals — safe to share.
#[repr(transparent)]
pub struct TzPtr(*const u8);

// SAFETY: Points to static c-string literals with program lifetime.
unsafe impl Sync for TzPtr {}

/// Timezone name strings: [standard, daylight].
///
/// POSIX requires `tzname` to be a `char *[2]`.  Since we have no
/// timezone support, both are "UTC".  `repr(transparent)` on `TzPtr`
/// ensures the layout matches `[*const u8; 2]` for C interop.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static tzname: [TzPtr; 2] = [
    TzPtr(c"UTC".as_ptr().cast::<u8>()),
    TzPtr(c"UTC".as_ptr().cast::<u8>()),
];

/// Seconds west of UTC.
///
/// POSIX/BSD variable.  Always 0 since we are always UTC.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static timezone: i64 = 0;

/// Whether daylight saving is ever in effect.
///
/// Always 0 — our OS has no DST support.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static daylight: i32 = 0;

/// Initialize timezone information from the TZ environment variable.
///
/// Since our OS doesn't support timezones, this is a no-op.  Programs
/// call this early in main() per POSIX convention.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tzset() {
    // No-op: we are always UTC.
}

// ---------------------------------------------------------------------------
// Broken-down time
// ---------------------------------------------------------------------------

/// Broken-down time (struct tm).
#[repr(C)]
#[derive(Clone, Copy)]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn localtime(timep: *const TimeT) -> *mut Tm {
    // No timezone — UTC is local time.
    gmtime(timep)
}

/// Convert broken-down time to time_t.
///
/// Normalizes the Tm fields and returns seconds since epoch.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timegm(tm: *mut Tm) -> TimeT {
    mktime(tm)
}

/// Convert broken-down local time to seconds since epoch.
///
/// BSD/GNU extension.  Equivalent to `mktime` — our OS is always UTC.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timelocal(tm: *mut Tm) -> TimeT {
    mktime(tm)
}

/// Convert broken-down time to string.
///
/// Returns a pointer to a static string in the format
/// "Wed Jun 30 21:49:08 1993\n\0".
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ctime(timep: *const TimeT) -> *const u8 {
    asctime(localtime(timep))
}

// ---------------------------------------------------------------------------
// Reentrant variants (_r suffix)
// ---------------------------------------------------------------------------

/// Convert time_t to broken-down UTC time (reentrant).
///
/// Writes the result into the caller-supplied `result` buffer instead
/// of using a shared static.  Returns `result` on success, null on error.
///
/// # Safety
///
/// Both pointers must be valid and non-null.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gmtime_r(timep: *const TimeT, result: *mut Tm) -> *mut Tm {
    if timep.is_null() || result.is_null() {
        return core::ptr::null_mut();
    }
    let secs = unsafe { *timep };
    let tm = unsafe { &mut *result };
    secs_to_tm(secs, tm);
    result
}

/// Convert time_t to broken-down local time (reentrant).
///
/// Since we have no timezone support, this is identical to `gmtime_r`.
///
/// # Safety
///
/// Both pointers must be valid and non-null.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn localtime_r(timep: *const TimeT, result: *mut Tm) -> *mut Tm {
    unsafe { gmtime_r(timep, result) }
}

/// Convert broken-down time to string (reentrant).
///
/// Writes the result into the caller-supplied `buf` (must be at least
/// 26 bytes).  Returns `buf` on success, null on error.
///
/// # Safety
///
/// `tm` must point to a valid `Tm`.  `buf` must be at least 26 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn asctime_r(tm: *const Tm, buf: *mut u8) -> *mut u8 {
    if tm.is_null() || buf.is_null() {
        return core::ptr::null_mut();
    }
    let t = unsafe { &*tm };
    // Write into a local buffer then copy to user buf.
    // format_asctime produces exactly 25 chars ("Thu Jan  1 00:00:00 1970\n")
    // and null-terminates, so we copy at most 25 content bytes + 1 null = 26 bytes.
    let mut tmp = [0u8; 32];
    let len = format_asctime(t, &mut tmp);
    // Cap content to 25 bytes so that content + null fits in the
    // 26-byte minimum buffer guaranteed by POSIX.
    let copy_len = if len > 25 { 25 } else { len };
    let mut i: usize = 0;
    while i < copy_len {
        // SAFETY: i < copy_len <= 25 < 32 = tmp.len(), so this is in-bounds.
        let byte = *tmp.get(i).unwrap_or(&0);
        unsafe { *buf.add(i) = byte; }
        i = i.wrapping_add(1);
    }
    // Null-terminate (at most at index 25 = 26th byte).
    unsafe { *buf.add(i) = 0; }
    buf
}

/// Convert time_t to string (reentrant).
///
/// Equivalent to `asctime_r(localtime_r(timep, &tm), buf)`.
///
/// # Safety
///
/// `timep` must be valid.  `buf` must be at least 26 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ctime_r(timep: *const TimeT, buf: *mut u8) -> *mut u8 {
    if timep.is_null() || buf.is_null() {
        return core::ptr::null_mut();
    }
    let mut result = Tm {
        tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0,
        tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0,
        tm_isdst: 0,
    };
    if unsafe { gmtime_r(timep, &raw mut result) }.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { asctime_r(&raw const result, buf) }
}

/// Format time according to a format string.
///
/// Supports these POSIX and GNU extension conversions:
///
/// **Date components**: `%Y` (4-digit year), `%C` (century), `%y` (2-digit year),
/// `%m` (month 01-12), `%d` (day 01-31), `%e` (day, space-padded),
/// `%j` (day of year 001-366), `%w` (weekday 0-6, Sun=0),
/// `%u` (weekday 1-7, Mon=1, ISO 8601),
/// `%U` (week of year, Sunday start), `%W` (week of year, Monday start).
///
/// **Time components**: `%H` (hour 00-23), `%I` (hour 01-12),
/// `%k` (hour 0-23, space-padded), `%l` (hour 1-12, space-padded),
/// `%M` (minute), `%S` (second), `%p` (AM/PM), `%P` (am/pm, GNU).
///
/// **Names**: `%A`/`%a` (weekday), `%B`/`%b`/`%h` (month).
///
/// **Composites**: `%c` (date+time), `%D` (%m/%d/%y), `%F` (%Y-%m-%d),
/// `%T` (%H:%M:%S), `%R` (%H:%M), `%r` (%I:%M:%S %p),
/// `%x` (locale date), `%X` (locale time).
///
/// **Timezone**: `%z` (+0000, always UTC), `%Z` (UTC).
///
/// **GNU extensions**: `%s` (epoch seconds), `%P` (lowercase am/pm).
///
/// **Literal**: `%n` (newline), `%t` (tab), `%%` (percent).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
            // --- Date components ---
            b'Y' => pos = write_dec4(buf, limit, pos, t.tm_year.wrapping_add(1900)),
            b'C' => pos = write_dec2(buf, limit, pos,
                t.tm_year.wrapping_add(1900).wrapping_div(100)),
            b'y' => pos = write_dec2(buf, limit, pos,
                t.tm_year.wrapping_add(1900).wrapping_rem(100)),
            b'm' => pos = write_dec2(buf, limit, pos, t.tm_mon.wrapping_add(1)),
            b'd' => pos = write_dec2(buf, limit, pos, t.tm_mday),
            b'e' => pos = write_space_dec2(buf, limit, pos, t.tm_mday),
            b'j' => pos = write_dec3(buf, limit, pos, t.tm_yday.wrapping_add(1)),
            b'w' => pos = write_char(buf, limit, pos,
                b'0'.wrapping_add((t.tm_wday % 7) as u8)),
            b'u' => {
                // ISO 8601: Monday=1 .. Sunday=7.
                let iso = if t.tm_wday == 0 { 7 } else { t.tm_wday };
                pos = write_char(buf, limit, pos, b'0'.wrapping_add(iso as u8));
            }
            b'U' => {
                // Week number, Sunday as first day (00-53).
                #[allow(clippy::arithmetic_side_effects)]
                let wn = (t.tm_yday.wrapping_add(7).wrapping_sub(t.tm_wday)) / 7;
                pos = write_dec2(buf, limit, pos, wn);
            }
            b'W' => {
                // Week number, Monday as first day (00-53).
                let mon_wday = if t.tm_wday == 0 { 6 } else { t.tm_wday.wrapping_sub(1) };
                #[allow(clippy::arithmetic_side_effects)]
                let wn = (t.tm_yday.wrapping_add(7).wrapping_sub(mon_wday)) / 7;
                pos = write_dec2(buf, limit, pos, wn);
            }

            // --- Time components ---
            b'H' => pos = write_dec2(buf, limit, pos, t.tm_hour),
            b'I' => {
                let h12 = hour_12(t.tm_hour);
                pos = write_dec2(buf, limit, pos, h12);
            }
            b'k' => pos = write_space_dec2(buf, limit, pos, t.tm_hour),
            b'l' => pos = write_space_dec2(buf, limit, pos, hour_12(t.tm_hour)),
            b'M' => pos = write_dec2(buf, limit, pos, t.tm_min),
            b'S' => pos = write_dec2(buf, limit, pos, t.tm_sec),
            b'p' => {
                let label = if t.tm_hour < 12 { b"AM" } else { b"PM" };
                pos = write_str(buf, limit, pos, label);
            }
            b'P' => {
                // GNU extension: lowercase am/pm.
                let label = if t.tm_hour < 12 { b"am" } else { b"pm" };
                pos = write_str(buf, limit, pos, label);
            }

            // --- Name components ---
            b'A' => pos = write_str(buf, limit, pos, wday_full(t.tm_wday)),
            b'a' => pos = write_str(buf, limit, pos, wday_abbr(t.tm_wday)),
            b'B' => pos = write_str(buf, limit, pos, mon_full(t.tm_mon)),
            b'b' | b'h' => pos = write_str(buf, limit, pos, mon_abbr(t.tm_mon)),

            // --- Composite specifiers ---
            b'c' => {
                // "Thu Jan  1 00:00:00 1970" (asctime format).
                pos = write_str(buf, limit, pos, wday_abbr(t.tm_wday));
                pos = write_char(buf, limit, pos, b' ');
                pos = write_str(buf, limit, pos, mon_abbr(t.tm_mon));
                pos = write_char(buf, limit, pos, b' ');
                pos = write_space_dec2(buf, limit, pos, t.tm_mday);
                pos = write_char(buf, limit, pos, b' ');
                pos = write_dec2(buf, limit, pos, t.tm_hour);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_min);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_sec);
                pos = write_char(buf, limit, pos, b' ');
                pos = write_dec4(buf, limit, pos, t.tm_year.wrapping_add(1900));
            }
            b'D' => {
                // %m/%d/%y
                pos = write_dec2(buf, limit, pos, t.tm_mon.wrapping_add(1));
                pos = write_char(buf, limit, pos, b'/');
                pos = write_dec2(buf, limit, pos, t.tm_mday);
                pos = write_char(buf, limit, pos, b'/');
                pos = write_dec2(buf, limit, pos,
                    t.tm_year.wrapping_add(1900).wrapping_rem(100));
            }
            b'F' => {
                // %Y-%m-%d (ISO 8601 date).
                pos = write_dec4(buf, limit, pos, t.tm_year.wrapping_add(1900));
                pos = write_char(buf, limit, pos, b'-');
                pos = write_dec2(buf, limit, pos, t.tm_mon.wrapping_add(1));
                pos = write_char(buf, limit, pos, b'-');
                pos = write_dec2(buf, limit, pos, t.tm_mday);
            }
            b'T' => {
                // %H:%M:%S
                pos = write_dec2(buf, limit, pos, t.tm_hour);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_min);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_sec);
            }
            b'R' => {
                // %H:%M
                pos = write_dec2(buf, limit, pos, t.tm_hour);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_min);
            }
            b'r' => {
                // %I:%M:%S %p (12-hour time with AM/PM).
                pos = write_dec2(buf, limit, pos, hour_12(t.tm_hour));
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_min);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_sec);
                pos = write_char(buf, limit, pos, b' ');
                let label = if t.tm_hour < 12 { b"AM" } else { b"PM" };
                pos = write_str(buf, limit, pos, label);
            }
            b'x' => {
                // Locale date (C locale: %m/%d/%y).
                pos = write_dec2(buf, limit, pos, t.tm_mon.wrapping_add(1));
                pos = write_char(buf, limit, pos, b'/');
                pos = write_dec2(buf, limit, pos, t.tm_mday);
                pos = write_char(buf, limit, pos, b'/');
                pos = write_dec2(buf, limit, pos,
                    t.tm_year.wrapping_add(1900).wrapping_rem(100));
            }
            b'X' => {
                // Locale time (C locale: %H:%M:%S).
                pos = write_dec2(buf, limit, pos, t.tm_hour);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_min);
                pos = write_char(buf, limit, pos, b':');
                pos = write_dec2(buf, limit, pos, t.tm_sec);
            }

            // --- Timezone ---
            b'z' => {
                // UTC offset: always +0000 (we have no timezone support).
                pos = write_str(buf, limit, pos, b"+0000");
            }
            b'Z' => {
                // Timezone name: always UTC.
                pos = write_str(buf, limit, pos, b"UTC");
            }

            // --- ISO 8601 week date (%G, %g, %V) ---
            b'V' => {
                // ISO 8601 week number (01-53).
                let (_, week) = iso_week_date(t);
                pos = write_dec2(buf, limit, pos, week);
            }
            b'G' => {
                // ISO 8601 week-based year (4 digits).
                let (year, _) = iso_week_date(t);
                pos = write_dec4(buf, limit, pos, year);
            }
            b'g' => {
                // ISO 8601 week-based year, last 2 digits.
                let (year, _) = iso_week_date(t);
                pos = write_dec2(buf, limit, pos, year.wrapping_rem(100));
            }

            // --- GNU extension ---
            b's' => {
                // Seconds since epoch (GNU extension).
                let mut tmp = unsafe { *tm };
                let epoch = tm_to_secs(&mut tmp);
                pos = write_i64(buf, limit, pos, epoch);
            }

            // --- Literal ---
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

/// Compute ISO 8601 week-based year and week number.
///
/// ISO 8601 defines:
/// - Weeks start on Monday.
/// - Week 01 is the week containing the first Thursday of the year
///   (equivalently, the week containing January 4th).
/// - The year associated with a week can differ from the calendar year
///   for days near the boundary (e.g., Dec 31 can be in week 01 of
///   the next year, and Jan 1-3 can be in week 52/53 of the previous year).
///
/// Returns `(iso_year, iso_week)` where `iso_week` is in 1..=53.
#[allow(clippy::arithmetic_side_effects)]
fn iso_week_date(tm: &Tm) -> (i32, i32) {
    let year = tm.tm_year + 1900;

    // ISO day of week: Monday=1..Sunday=7.
    let iso_dow = if tm.tm_wday == 0 { 7 } else { tm.tm_wday };

    // Day of year (0-based).
    let yday = tm.tm_yday;

    // The ordinal of the Monday of the ISO week containing this day.
    // yday - iso_dow + 1 gives Monday of this week (since iso_dow is
    // 1 for Monday).  Then we need the week number relative to the
    // first Thursday.
    //
    // ISO week number formula: the week number is computed by finding
    // how many Thursdays have occurred so far in the year.  A simpler
    // way: compute the ordinal of the Thursday in the same ISO week,
    // then W = (ordinal_of_thursday / 7) + 1.
    let thursday_yday = yday + (4 - iso_dow); // Thursday of this week.

    if thursday_yday < 0 {
        // Thursday is in the previous year — this day belongs to the
        // last week of the previous year.
        let prev_year = year - 1;
        let prev_dec31_days = if is_leap(prev_year) { 365 } else { 364 };
        // Compute week number for Dec 31 of previous year.
        // Use the number of days in that year.
        let prev_year_days = if is_leap(prev_year) { 366 } else { 365 };
        // The Thursday for the adjusted day in the previous year.
        let adj_thursday = prev_dec31_days + thursday_yday + 1;
        let week = (adj_thursday / 7) + 1;
        // Clamp: ISO week is at most 53.
        let week = if week > 53 { 53 } else { week };
        let _ = prev_year_days; // Suppress unused warning.
        return (prev_year, week);
    }

    let year_days = if is_leap(year) { 366 } else { 365 };

    if thursday_yday >= year_days {
        // Thursday is in the next year — this day belongs to week 01
        // of the next year.
        return (year + 1, 1);
    }

    // Normal case: week number in the current year.
    let week = (thursday_yday / 7) + 1;
    (year, week)
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
///
/// Handles both positive timestamps (post-1970) and negative ones
/// (pre-1970) correctly.  Uses Euclidean division (always positive
/// remainders) for time-of-day decomposition.
#[allow(clippy::arithmetic_side_effects)]
fn secs_to_tm(secs: TimeT, tm: &mut Tm) {
    // Use Euclidean division to always get non-negative remainders.
    // Rust's % truncates toward zero: -1 % 60 = -1, but we need 59.
    let mut rem = secs;

    // Seconds.
    tm.tm_sec = (rem.rem_euclid(60)) as i32;
    rem = rem.div_euclid(60);
    // Minutes.
    tm.tm_min = (rem.rem_euclid(60)) as i32;
    rem = rem.div_euclid(60);
    // Hours.
    tm.tm_hour = (rem.rem_euclid(24)) as i32;
    rem = rem.div_euclid(24);

    // rem is now days since epoch (can be negative for pre-1970).
    // 1970-01-01 was a Thursday (wday=4).
    tm.tm_wday = ((rem + 4).rem_euclid(7)) as i32;

    // Compute year and day-of-year.
    let mut year: i32 = 1970;
    if rem >= 0 {
        // Post-epoch: count forward.
        loop {
            let days_this_year: i64 = if is_leap(year) { 366 } else { 365 };
            if rem < days_this_year {
                break;
            }
            rem -= days_this_year;
            year += 1;
        }
    } else {
        // Pre-epoch: count backward.
        loop {
            year -= 1;
            let days_this_year: i64 = if is_leap(year) { 366 } else { 365 };
            rem += days_this_year;
            if rem >= 0 {
                break;
            }
        }
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
///
/// Per POSIX, `mktime` normalizes all fields of the `Tm` structure:
/// - Seconds overflow into minutes, minutes into hours, etc.
/// - Negative values borrow from the next-higher unit.
/// - Month values > 11 or < 0 adjust the year.
/// - After normalization, `tm_wday` and `tm_yday` are set.
#[allow(clippy::arithmetic_side_effects)]
fn tm_to_secs(tm: &mut Tm) -> TimeT {
    // --- Normalize time-of-day fields (bottom up) ---

    // Seconds → minutes.
    let total_sec = i64::from(tm.tm_sec);
    tm.tm_sec = total_sec.rem_euclid(60) as i32;
    let carry_min = total_sec.div_euclid(60);

    // Minutes → hours.
    let total_min = i64::from(tm.tm_min) + carry_min;
    tm.tm_min = total_min.rem_euclid(60) as i32;
    let carry_hour = total_min.div_euclid(60);

    // Hours → days.
    let total_hour = i64::from(tm.tm_hour) + carry_hour;
    tm.tm_hour = total_hour.rem_euclid(24) as i32;
    let carry_day = total_hour.div_euclid(24);

    // Adjust mday with carry from hours.
    let mut mday = i64::from(tm.tm_mday) + carry_day;

    // --- Normalize month → year ---
    let mon_raw = i64::from(tm.tm_mon);
    let norm_mon = mon_raw.rem_euclid(12) as i32;
    let carry_year = mon_raw.div_euclid(12) as i32;
    tm.tm_mon = norm_mon;
    tm.tm_year += carry_year;

    let mut year = tm.tm_year + 1900;

    // --- Normalize day-of-month into month/year ---
    // Handle overflow (mday > days-in-month) and underflow (mday < 1).
    // Loop because adjusting the month may change the days-in-month
    // (e.g., stepping from March into February changes the limit).
    loop {
        let dim = i64::from(days_in_month(tm.tm_mon, year));
        if mday > dim {
            mday -= dim;
            tm.tm_mon += 1;
            if tm.tm_mon > 11 {
                tm.tm_mon = 0;
                tm.tm_year += 1;
                year += 1;
            }
        } else if mday < 1 {
            tm.tm_mon -= 1;
            if tm.tm_mon < 0 {
                tm.tm_mon = 11;
                tm.tm_year -= 1;
                year -= 1;
            }
            mday += i64::from(days_in_month(tm.tm_mon, year));
        } else {
            break;
        }
    }

    tm.tm_mday = mday as i32;

    // --- Compute total days from epoch ---
    let mut days: i64 = 0;
    if year > 1970 {
        let mut y = 1970;
        while y < year {
            days += if is_leap(y) { 366 } else { 365 };
            y += 1;
        }
    } else if year < 1970 {
        let mut y = 1969;
        while y >= year {
            days -= if is_leap(y) { 366 } else { 365 };
            y -= 1;
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

    // Update tm_yday.
    tm.tm_yday = 0;
    let mut m2 = 0;
    while m2 < tm.tm_mon {
        tm.tm_yday += days_in_month(m2, year);
        m2 += 1;
    }
    tm.tm_yday += tm.tm_mday - 1;

    // Update tm_wday: 1970-01-01 was Thursday (wday=4).
    tm.tm_wday = ((days + 4).rem_euclid(7)) as i32;

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
    // POSIX: asctime uses space-padded day (" 1" not "01").
    pos = write_space_dec2(buf.as_mut_ptr(), limit, pos, tm.tm_mday);
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

/// Write a 2-digit space-padded decimal (e.g., " 5" for 5).
fn write_space_dec2(buf: *mut u8, limit: usize, pos: usize, val: i32) -> usize {
    let v = if val < 0 { 0 } else { val as u32 };
    let tens = v.wrapping_div(10) % 10;
    let ones = v % 10;
    let d1 = if tens == 0 { b' ' } else { b'0'.wrapping_add(tens as u8) };
    let d0 = b'0'.wrapping_add(ones as u8);
    let p1 = write_char(buf, limit, pos, d1);
    write_char(buf, limit, p1, d0)
}

/// Convert 24-hour clock to 12-hour clock (1-12).
fn hour_12(h24: i32) -> i32 {
    let h = h24 % 12;
    if h == 0 { 12 } else { h }
}

/// Write an `i64` value as decimal digits (no padding, handles negatives).
fn write_i64(buf: *mut u8, limit: usize, mut pos: usize, val: i64) -> usize {
    if val < 0 {
        pos = write_char(buf, limit, pos, b'-');
        // Avoid overflow on i64::MIN by using wrapping.
        return write_u64(buf, limit, pos, (val.wrapping_neg()) as u64);
    }
    write_u64(buf, limit, pos, val as u64)
}

/// Write a `u64` value as decimal digits (no padding).
fn write_u64(buf: *mut u8, limit: usize, pos: usize, val: u64) -> usize {
    // Stack buffer for up to 20 digits (u64::MAX = ~1.8e19).
    let mut digits = [0u8; 20];
    let mut n = val;
    let mut count: usize = 0;

    if n == 0 {
        return write_char(buf, limit, pos, b'0');
    }

    while n > 0 {
        if let Some(slot) = digits.get_mut(count) {
            *slot = b'0'.wrapping_add((n % 10) as u8);
        }
        count = count.wrapping_add(1);
        n = n.wrapping_div(10);
    }

    // Write digits in reverse (most significant first).
    let mut p = pos;
    let mut i = count;
    while i > 0 {
        i = i.wrapping_sub(1);
        let d = digits.get(i).copied().unwrap_or(b'0');
        p = write_char(buf, limit, p, d);
    }
    p
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static CLOCKS_PER_SEC: i64 = 1_000_000;

/// Return an approximation of CPU time used by the process.
///
/// Returns microseconds elapsed since an arbitrary point (we use
/// `CLOCK_MONOTONIC` as a proxy since we don't track per-process
/// CPU time yet).  Returns -1 on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
/// Supports: `%Y`, `%C`, `%y`, `%m`, `%d`, `%e`, `%H`, `%I`, `%M`,
/// `%S`, `%j`, `%w`, `%u`, `%p`, `%n`, `%t`, `%%`.
///
/// # Safety
///
/// `buf` and `format` must be valid null-terminated strings.
/// `tm` must point to a valid `Tm`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
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
                b'C' => {
                    // Century (2 digits).  Sets year = century*100 + (year%100).
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe {
                        let cur_y2 = ((*tm).tm_year.wrapping_add(1900)) % 100;
                        (*tm).tm_year = val.wrapping_mul(100).wrapping_add(cur_y2).wrapping_sub(1900);
                    }
                    bi = bi.wrapping_add(consumed);
                }
                b'y' => {
                    // 2-digit year. 69-99 → 1969-1999, 00-68 → 2000-2068.
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    let full_year = if val >= 69 { val.wrapping_add(1900) }
                                    else { val.wrapping_add(2000) };
                    unsafe { (*tm).tm_year = full_year.wrapping_sub(1900); }
                    bi = bi.wrapping_add(consumed);
                }
                b'm' => {
                    // Month 01-12.
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_mon = val - 1; }
                    bi = bi.wrapping_add(consumed);
                }
                b'd' | b'e' => {
                    // Day 01-31 (or space-padded for %e).
                    // Skip leading space for %e.
                    if spec == b'e' {
                        while (unsafe { *buf.add(bi) }) == b' ' {
                            bi = bi.wrapping_add(1);
                        }
                    }
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_mday = val; }
                    bi = bi.wrapping_add(consumed);
                }
                b'H' | b'k' => {
                    // Hour 00-23 (%k allows space-padded).
                    if spec == b'k' {
                        while (unsafe { *buf.add(bi) }) == b' ' {
                            bi = bi.wrapping_add(1);
                        }
                    }
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_hour = val; }
                    bi = bi.wrapping_add(consumed);
                }
                b'I' | b'l' => {
                    // Hour 01-12 (12-hour clock).
                    if spec == b'l' {
                        while (unsafe { *buf.add(bi) }) == b' ' {
                            bi = bi.wrapping_add(1);
                        }
                    }
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    // Store as-is; %p adjusts for AM/PM later.
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
                b'w' => {
                    // Weekday 0-6 (Sunday=0).
                    let (val, consumed) = parse_int(buf, bi, 1);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_wday = val; }
                    bi = bi.wrapping_add(consumed);
                }
                b'u' => {
                    // ISO weekday 1-7 (Monday=1).
                    let (val, consumed) = parse_int(buf, bi, 1);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_wday = if val == 7 { 0 } else { val }; }
                    bi = bi.wrapping_add(consumed);
                }
                b'p' | b'P' => {
                    // AM/PM (or am/pm). Adjusts tm_hour for 12-hour input.
                    let c1 = unsafe { *buf.add(bi) };
                    let c2 = unsafe { *buf.add(bi.wrapping_add(1)) };
                    let afternoon = (c1 == b'P' || c1 == b'p')
                        && (c2 == b'M' || c2 == b'm');
                    let morning = (c1 == b'A' || c1 == b'a')
                        && (c2 == b'M' || c2 == b'm');
                    if !afternoon && !morning {
                        return core::ptr::null();
                    }
                    unsafe {
                        if afternoon && (*tm).tm_hour < 12 {
                            (*tm).tm_hour = (*tm).tm_hour.wrapping_add(12);
                        } else if morning && (*tm).tm_hour == 12 {
                            (*tm).tm_hour = 0;
                        }
                    }
                    bi = bi.wrapping_add(2);
                }
                b'b' | b'B' | b'h' => {
                    // Month name (abbreviated or full).
                    if let Some((mon, consumed)) = match_month_name(buf, bi) {
                        unsafe { (*tm).tm_mon = mon; }
                        bi = bi.wrapping_add(consumed);
                    } else {
                        return core::ptr::null();
                    }
                }
                b'a' | b'A' => {
                    // Weekday name (abbreviated or full).
                    if let Some((wday, consumed)) = match_wday_name(buf, bi) {
                        unsafe { (*tm).tm_wday = wday; }
                        bi = bi.wrapping_add(consumed);
                    } else {
                        return core::ptr::null();
                    }
                }
                b'V' => {
                    // ISO 8601 week number (01-53) — informational only,
                    // we parse the digits but don't derive date fields from
                    // the week number alone (would need %G too).
                    let (_, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    bi = bi.wrapping_add(consumed);
                }
                b'G' => {
                    // ISO 8601 week-based year — treat as regular year.
                    let (val, consumed) = parse_int(buf, bi, 4);
                    if consumed == 0 { return core::ptr::null(); }
                    unsafe { (*tm).tm_year = val - 1900; }
                    bi = bi.wrapping_add(consumed);
                }
                b'g' => {
                    // ISO 8601 week-based year (2-digit).
                    let (val, consumed) = parse_int(buf, bi, 2);
                    if consumed == 0 { return core::ptr::null(); }
                    let full_year = if val >= 69 { val.wrapping_add(1900) }
                                    else { val.wrapping_add(2000) };
                    unsafe { (*tm).tm_year = full_year.wrapping_sub(1900); }
                    bi = bi.wrapping_add(consumed);
                }
                b'z' => {
                    // Timezone offset (+HHMM or -HHMM).  Parse but ignore
                    // (we always use UTC).
                    let sign = unsafe { *buf.add(bi) };
                    if sign != b'+' && sign != b'-' {
                        return core::ptr::null();
                    }
                    bi = bi.wrapping_add(1);
                    let (_, consumed) = parse_int(buf, bi, 4);
                    if consumed < 2 { return core::ptr::null(); }
                    bi = bi.wrapping_add(consumed);
                }
                b'Z' => {
                    // Timezone abbreviation — skip alphabetic chars.
                    while (unsafe { *buf.add(bi) }).is_ascii_alphabetic() {
                        bi = bi.wrapping_add(1);
                    }
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

/// Match a month name (abbreviated or full) at position `off` in `buf`.
///
/// Returns `(month_0_indexed, chars_consumed)` or `None` if no match.
///
/// # Safety
///
/// `buf` must be valid for at least `off + 9` bytes (longest month name).
fn match_month_name(buf: *const u8, off: usize) -> Option<(i32, usize)> {
    // Try full names first (longer match wins), then abbreviated.
    static MONTHS: [(&[u8], &[u8]); 12] = [
        (b"January", b"Jan"),   (b"February", b"Feb"),
        (b"March", b"Mar"),     (b"April", b"Apr"),
        (b"May", b"May"),       (b"June", b"Jun"),
        (b"July", b"Jul"),      (b"August", b"Aug"),
        (b"September", b"Sep"), (b"October", b"Oct"),
        (b"November", b"Nov"),  (b"December", b"Dec"),
    ];

    for (i, (full, abbr)) in MONTHS.iter().enumerate() {
        // Try full name first.
        if ci_match(buf, off, full) {
            return Some((i as i32, full.len()));
        }
        // Then abbreviated.
        if ci_match(buf, off, abbr) {
            return Some((i as i32, abbr.len()));
        }
    }
    None
}

/// Match a weekday name (abbreviated or full) at position `off` in `buf`.
///
/// Returns `(wday_sunday_0, chars_consumed)` or `None` if no match.
///
/// # Safety
///
/// `buf` must be valid for at least `off + 9` bytes (longest weekday name).
fn match_wday_name(buf: *const u8, off: usize) -> Option<(i32, usize)> {
    static WDAYS: [(&[u8], &[u8]); 7] = [
        (b"Sunday", b"Sun"),    (b"Monday", b"Mon"),
        (b"Tuesday", b"Tue"),   (b"Wednesday", b"Wed"),
        (b"Thursday", b"Thu"),  (b"Friday", b"Fri"),
        (b"Saturday", b"Sat"),
    ];

    for (i, (full, abbr)) in WDAYS.iter().enumerate() {
        if ci_match(buf, off, full) {
            return Some((i as i32, full.len()));
        }
        if ci_match(buf, off, abbr) {
            return Some((i as i32, abbr.len()));
        }
    }
    None
}

/// Case-insensitive match of `pattern` against `buf[off..]`.
///
/// # Safety
///
/// `buf` must be valid for at least `off + pattern.len()` bytes.
fn ci_match(buf: *const u8, off: usize, pattern: &[u8]) -> bool {
    for (j, &p) in pattern.iter().enumerate() {
        // SAFETY: Caller guarantees buf is valid for off + pattern.len() bytes.
        let c = unsafe { *buf.add(off.wrapping_add(j)) };
        if !c.eq_ignore_ascii_case(&p) {
            return false;
        }
    }
    true
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_getoverrun(_timerid: TimerT) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// setitimer / getitimer — interval timers (BSD/POSIX)
// ---------------------------------------------------------------------------

/// Interval timer value (for `setitimer`/`getitimer`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Itimerval {
    /// Time until next expiration.
    pub it_interval: Timeval,
    /// Current value (time remaining).
    pub it_value: Timeval,
}

/// Which timer to set/get.
pub const ITIMER_REAL: i32 = 0;
/// Virtual timer (user-mode CPU time).
pub const ITIMER_VIRTUAL: i32 = 1;
/// Profiling timer (user + system CPU time).
pub const ITIMER_PROF: i32 = 2;

/// Number of interval timer types (ITIMER_REAL, ITIMER_VIRTUAL, ITIMER_PROF).
const ITIMER_COUNT: usize = 3;

/// Per-timer-type storage for `setitimer`/`getitimer`.
///
/// Indexed by the `which` parameter (0 = REAL, 1 = VIRTUAL, 2 = PROF).
/// The timers never actually fire (no signal delivery), but we store the
/// values so `getitimer` returns what `setitimer` set.  This makes
/// programs that read back their own timer settings work correctly.
static mut ITIMER_STATE: [Itimerval; ITIMER_COUNT] = [Itimerval {
    it_interval: Timeval { tv_sec: 0, tv_usec: 0 },
    it_value: Timeval { tv_sec: 0, tv_usec: 0 },
}; ITIMER_COUNT];

/// Set an interval timer.
///
/// Stores the timer value so `getitimer` can retrieve it.  The timer
/// never actually fires because we don't have signal delivery.
/// Programs that use `setitimer` for periodic alarms won't get
/// SIGALRM/SIGVTALRM/SIGPROF, but they will see their own settings
/// reflected back via `getitimer`.
///
/// # Safety
///
/// `new_value` must be a valid pointer.  `old_value` may be null.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setitimer(
    which: i32,
    new_value: *const Itimerval,
    old_value: *mut Itimerval,
) -> i32 {
    if which != ITIMER_REAL && which != ITIMER_VIRTUAL && which != ITIMER_PROF {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if new_value.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    #[allow(clippy::cast_sign_loss)]
    let idx = which as usize;

    // Return old value if requested.
    if !old_value.is_null() {
        // SAFETY: old_value verified non-null; idx < ITIMER_COUNT.
        unsafe {
            let state = core::ptr::addr_of_mut!(ITIMER_STATE);
            if let Some(entry) = (*state).get(idx) {
                *old_value = *entry;
            }
        }
    }

    // Store the new value.
    // SAFETY: single-threaded; idx < ITIMER_COUNT.
    unsafe {
        let state = core::ptr::addr_of_mut!(ITIMER_STATE);
        if let Some(entry) = (*state).get_mut(idx) {
            *entry = *new_value;
        }
    }

    0
}

/// Get the current value of an interval timer.
///
/// Returns the value last set by `setitimer`, or zeros if never set.
/// The timer never actually counts down (no kernel timer integration).
///
/// # Safety
///
/// `curr_value` must be a valid, writable pointer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getitimer(which: i32, curr_value: *mut Itimerval) -> i32 {
    if which != ITIMER_REAL && which != ITIMER_VIRTUAL && which != ITIMER_PROF {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if curr_value.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    #[allow(clippy::cast_sign_loss)]
    let idx = which as usize;

    // SAFETY: curr_value verified non-null; idx < ITIMER_COUNT.
    unsafe {
        let state = core::ptr::addr_of_mut!(ITIMER_STATE);
        if let Some(entry) = (*state).get(idx) {
            *curr_value = *entry;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a zeroed Tm.
    fn zero_tm() -> Tm {
        Tm {
            tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0,
            tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0,
            tm_isdst: 0,
        }
    }

    // -- gmtime / secs_to_tm tests --

    #[test]
    fn test_gmtime_epoch() {
        // 1970-01-01 00:00:00 UTC
        let t: TimeT = 0;
        let tm = gmtime(&t);
        assert!(!tm.is_null());
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 70);   // 1970 - 1900
        assert_eq!(tm.tm_mon, 0);     // January
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_min, 0);
        assert_eq!(tm.tm_sec, 0);
        assert_eq!(tm.tm_wday, 4);    // Thursday
        assert_eq!(tm.tm_yday, 0);
    }

    #[test]
    fn test_gmtime_known_date() {
        // 2000-01-01 00:00:00 UTC = 946684800
        let t: TimeT = 946_684_800;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 100);  // 2000 - 1900
        assert_eq!(tm.tm_mon, 0);     // January
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_wday, 6);    // Saturday
    }

    #[test]
    fn test_gmtime_leap_day() {
        // 2000-02-29 12:00:00 UTC = 951825600
        let t: TimeT = 951_825_600;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 100);
        assert_eq!(tm.tm_mon, 1);     // February (0-indexed)
        assert_eq!(tm.tm_mday, 29);
        assert_eq!(tm.tm_hour, 12);
    }

    #[test]
    fn test_gmtime_end_of_year() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        let t: TimeT = 1_704_067_199;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 123);  // 2023 - 1900
        assert_eq!(tm.tm_mon, 11);    // December
        assert_eq!(tm.tm_mday, 31);
        assert_eq!(tm.tm_hour, 23);
        assert_eq!(tm.tm_min, 59);
        assert_eq!(tm.tm_sec, 59);
        assert_eq!(tm.tm_yday, 364);
    }

    #[test]
    fn test_gmtime_pre_epoch() {
        // 1969-12-31 23:59:59 UTC = -1
        let t: TimeT = -1;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 69);   // 1969 - 1900
        assert_eq!(tm.tm_mon, 11);    // December
        assert_eq!(tm.tm_mday, 31);
        assert_eq!(tm.tm_hour, 23);
        assert_eq!(tm.tm_min, 59);
        assert_eq!(tm.tm_sec, 59);
    }

    #[test]
    fn test_gmtime_null() {
        let tm = gmtime(core::ptr::null());
        assert!(tm.is_null());
    }

    // -- mktime / tm_to_secs tests --

    #[test]
    fn test_mktime_epoch() {
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        let t = mktime(&mut tm);
        assert_eq!(t, 0);
    }

    #[test]
    fn test_mktime_known_date() {
        let mut tm = zero_tm();
        tm.tm_year = 100; // 2000
        tm.tm_mon = 0;    // January
        tm.tm_mday = 1;
        let t = mktime(&mut tm);
        assert_eq!(t, 946_684_800);
    }

    #[test]
    fn test_mktime_normalizes() {
        // 2000-01-01 00:00:90 should normalize to 00:01:30
        let mut tm = zero_tm();
        tm.tm_year = 100;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_sec = 90;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_sec, 30);
        assert_eq!(tm.tm_min, 1);
    }

    #[test]
    fn test_mktime_month_overflow() {
        // Month 12 (January of next year) should normalize.
        let mut tm = zero_tm();
        tm.tm_year = 100; // 2000
        tm.tm_mon = 12;   // 13th month → January 2001
        tm.tm_mday = 1;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_year, 101); // 2001
        assert_eq!(tm.tm_mon, 0);    // January
    }

    #[test]
    fn test_mktime_sets_wday() {
        // 2024-03-15 should be a Friday (wday=5).
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = 2;    // March
        tm.tm_mday = 15;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_wday, 5); // Friday
    }

    // -- gmtime / mktime roundtrip --

    #[test]
    fn test_gmtime_mktime_roundtrip() {
        let timestamps: &[TimeT] = &[
            0, 1, 86400, 946_684_800, 1_704_067_199, -1, -86400,
        ];
        for &t in timestamps {
            let tm = gmtime(&t);
            let tm = unsafe { &mut *tm };
            let t2 = mktime(tm);
            assert_eq!(t, t2, "roundtrip failed for timestamp {t}");
        }
    }

    // -- difftime tests --

    #[test]
    fn test_difftime_basic() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(difftime(100, 50), 50.0);
            assert_eq!(difftime(50, 100), -50.0);
            assert_eq!(difftime(0, 0), 0.0);
        }
    }

    // -- asctime tests --

    #[test]
    fn test_asctime_format() {
        let mut tm = zero_tm();
        tm.tm_year = 93;  // 1993
        tm.tm_mon = 5;    // June (0-indexed)
        tm.tm_mday = 30;
        tm.tm_hour = 21;
        tm.tm_min = 49;
        tm.tm_sec = 8;
        tm.tm_wday = 3;   // Wednesday

        let s = asctime(&tm);
        assert!(!s.is_null());
        // Expected: "Wed Jun 30 21:49:08 1993\n\0"
        let len = unsafe { crate::string::strlen(s) };
        assert!(len > 20);
        // Check that it starts with "Wed Jun"
        assert_eq!(unsafe { *s }, b'W');
        assert_eq!(unsafe { *s.add(1) }, b'e');
        assert_eq!(unsafe { *s.add(2) }, b'd');
    }

    // -- strftime tests --

    #[test]
    fn test_strftime_year_month_day() {
        let mut tm = zero_tm();
        tm.tm_year = 124;  // 2024
        tm.tm_mon = 2;     // March
        tm.tm_mday = 15;
        tm.tm_wday = 5;    // Friday

        let mut buf = [0u8; 64];
        let fmt = b"%Y-%m-%d\0";
        let n = unsafe {
            strftime(buf.as_mut_ptr(), 64, fmt.as_ptr(), &tm)
        };
        assert!(n > 0);
        assert_eq!(&buf[..10], b"2024-03-15");
    }

    #[test]
    fn test_strftime_time() {
        let mut tm = zero_tm();
        tm.tm_hour = 14;
        tm.tm_min = 30;
        tm.tm_sec = 45;

        let mut buf = [0u8; 64];
        let fmt = b"%H:%M:%S\0";
        let n = unsafe {
            strftime(buf.as_mut_ptr(), 64, fmt.as_ptr(), &tm)
        };
        assert!(n > 0);
        assert_eq!(&buf[..8], b"14:30:45");
    }

    #[test]
    fn test_strftime_percent_literal() {
        let tm = zero_tm();
        let mut buf = [0u8; 16];
        let fmt = b"100%%\0";
        let n = unsafe {
            strftime(buf.as_mut_ptr(), 16, fmt.as_ptr(), &tm)
        };
        assert!(n > 0);
        assert_eq!(&buf[..4], b"100%");
    }

    #[test]
    fn test_strftime_buffer_too_small() {
        let tm = zero_tm();
        let mut buf = [0u8; 4];
        let fmt = b"%Y-%m-%d\0";
        let n = unsafe {
            strftime(buf.as_mut_ptr(), 4, fmt.as_ptr(), &tm)
        };
        // Not enough space for "1900-01-00" (10 chars + null).
        // strftime returns 0 when buffer is insufficient.
        assert_eq!(n, 0);
    }

    // -- strptime tests --

    #[test]
    fn test_strptime_date() {
        let mut tm = zero_tm();
        let input = b"2024-03-15\0";
        let fmt = b"%Y-%m-%d\0";
        let result = unsafe {
            strptime(input.as_ptr(), fmt.as_ptr(), &mut tm)
        };
        assert!(!result.is_null());
        assert_eq!(tm.tm_year, 124);  // 2024 - 1900
        assert_eq!(tm.tm_mon, 2);     // March (0-indexed)
        assert_eq!(tm.tm_mday, 15);
    }

    #[test]
    fn test_strptime_time() {
        let mut tm = zero_tm();
        let input = b"14:30:45\0";
        let fmt = b"%H:%M:%S\0";
        let result = unsafe {
            strptime(input.as_ptr(), fmt.as_ptr(), &mut tm)
        };
        assert!(!result.is_null());
        assert_eq!(tm.tm_hour, 14);
        assert_eq!(tm.tm_min, 30);
        assert_eq!(tm.tm_sec, 45);
    }

    // -- is_leap / days_in_month tests --

    #[test]
    fn test_leap_years() {
        assert!(is_leap(2000)); // Divisible by 400.
        assert!(!is_leap(1900)); // Divisible by 100 but not 400.
        assert!(is_leap(2024)); // Divisible by 4 but not 100.
        assert!(!is_leap(2023)); // Not divisible by 4.
    }

    #[test]
    fn test_days_in_month_values() {
        assert_eq!(days_in_month(0, 2023), 31);  // January
        assert_eq!(days_in_month(1, 2023), 28);  // February (non-leap)
        assert_eq!(days_in_month(1, 2024), 29);  // February (leap)
        assert_eq!(days_in_month(3, 2023), 30);  // April
        assert_eq!(days_in_month(11, 2023), 31); // December
    }

    // -- mktime null --

    #[test]
    fn test_mktime_null() {
        assert_eq!(mktime(core::ptr::null_mut()), -1);
    }

    // -- timegm / timelocal identity --

    #[test]
    fn test_timegm_equals_mktime() {
        let mut tm = zero_tm();
        tm.tm_year = 100;
        tm.tm_mon = 5;
        tm.tm_mday = 15;
        tm.tm_hour = 12;
        let mut tm2 = tm;
        assert_eq!(mktime(&mut tm), timegm(&mut tm2));
    }

    #[test]
    fn test_timelocal_equals_mktime() {
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        let mut tm2 = tm;
        assert_eq!(mktime(&mut tm), timelocal(&mut tm2));
    }

    // -- strftime additional specifiers --

    /// Helper: run strftime on a Tm and return the result as a byte vector.
    fn run_strftime(fmt: &[u8], tm: &Tm) -> Vec<u8> {
        let mut buf = [0u8; 128];
        let n = unsafe {
            strftime(
                buf.as_mut_ptr(),
                buf.len(),
                fmt.as_ptr(),
                &raw const *tm,
            )
        };
        buf[..n].to_vec()
    }

    #[test]
    fn test_strftime_12hour_clock() {
        let mut tm = zero_tm();
        tm.tm_year = 123; // 2023
        tm.tm_mon = 0;
        tm.tm_mday = 15;

        // Midnight (0:00) → 12 in 12-hour format
        tm.tm_hour = 0;
        assert_eq!(run_strftime(b"%I\0", &tm), b"12");

        // 1 AM
        tm.tm_hour = 1;
        assert_eq!(run_strftime(b"%I\0", &tm), b"01");

        // Noon
        tm.tm_hour = 12;
        assert_eq!(run_strftime(b"%I\0", &tm), b"12");

        // 1 PM
        tm.tm_hour = 13;
        assert_eq!(run_strftime(b"%I\0", &tm), b"01");

        // 11 PM
        tm.tm_hour = 23;
        assert_eq!(run_strftime(b"%I\0", &tm), b"11");
    }

    #[test]
    fn test_strftime_ampm() {
        let mut tm = zero_tm();
        tm.tm_year = 123;
        tm.tm_mon = 0;
        tm.tm_mday = 15;

        tm.tm_hour = 0;
        assert_eq!(run_strftime(b"%p\0", &tm), b"AM");
        assert_eq!(run_strftime(b"%P\0", &tm), b"am");

        tm.tm_hour = 11;
        assert_eq!(run_strftime(b"%p\0", &tm), b"AM");

        tm.tm_hour = 12;
        assert_eq!(run_strftime(b"%p\0", &tm), b"PM");
        assert_eq!(run_strftime(b"%P\0", &tm), b"pm");

        tm.tm_hour = 23;
        assert_eq!(run_strftime(b"%p\0", &tm), b"PM");
    }

    #[test]
    fn test_strftime_iso_date() {
        let mut tm = zero_tm();
        tm.tm_year = 123; // 2023
        tm.tm_mon = 5;    // June
        tm.tm_mday = 15;
        assert_eq!(run_strftime(b"%F\0", &tm), b"2023-06-15");
    }

    #[test]
    fn test_strftime_time_formats() {
        let mut tm = zero_tm();
        tm.tm_hour = 14;
        tm.tm_min = 30;
        tm.tm_sec = 5;

        assert_eq!(run_strftime(b"%T\0", &tm), b"14:30:05");
        assert_eq!(run_strftime(b"%R\0", &tm), b"14:30");
    }

    #[test]
    fn test_strftime_12hour_time_with_ampm() {
        let mut tm = zero_tm();
        tm.tm_hour = 14;
        tm.tm_min = 30;
        tm.tm_sec = 5;

        let result = run_strftime(b"%r\0", &tm);
        assert_eq!(result, b"02:30:05 PM");
    }

    #[test]
    fn test_strftime_day_of_year() {
        let mut tm = zero_tm();
        tm.tm_year = 123;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_yday = 0;  // Jan 1 = day 1
        assert_eq!(run_strftime(b"%j\0", &tm), b"001");

        tm.tm_yday = 364; // Day 365 of a non-leap year (Dec 31)
        assert_eq!(run_strftime(b"%j\0", &tm), b"365");
    }

    #[test]
    fn test_strftime_weekday_names() {
        let mut tm = zero_tm();
        tm.tm_year = 123;

        tm.tm_wday = 0;
        assert_eq!(run_strftime(b"%A\0", &tm), b"Sunday");
        assert_eq!(run_strftime(b"%a\0", &tm), b"Sun");

        tm.tm_wday = 1;
        assert_eq!(run_strftime(b"%A\0", &tm), b"Monday");
        assert_eq!(run_strftime(b"%a\0", &tm), b"Mon");

        tm.tm_wday = 6;
        assert_eq!(run_strftime(b"%A\0", &tm), b"Saturday");
        assert_eq!(run_strftime(b"%a\0", &tm), b"Sat");
    }

    #[test]
    fn test_strftime_month_names() {
        let mut tm = zero_tm();
        tm.tm_year = 123;
        tm.tm_mday = 1;

        tm.tm_mon = 0;
        assert_eq!(run_strftime(b"%B\0", &tm), b"January");
        assert_eq!(run_strftime(b"%b\0", &tm), b"Jan");

        tm.tm_mon = 11;
        assert_eq!(run_strftime(b"%B\0", &tm), b"December");
        assert_eq!(run_strftime(b"%b\0", &tm), b"Dec");
    }

    #[test]
    fn test_strftime_timezone() {
        let tm = zero_tm();
        assert_eq!(run_strftime(b"%z\0", &tm), b"+0000");
        assert_eq!(run_strftime(b"%Z\0", &tm), b"UTC");
    }

    #[test]
    fn test_strftime_century() {
        let mut tm = zero_tm();
        tm.tm_year = 123; // 2023
        assert_eq!(run_strftime(b"%C\0", &tm), b"20");

        tm.tm_year = 0; // 1900
        assert_eq!(run_strftime(b"%C\0", &tm), b"19");
    }

    #[test]
    fn test_strftime_iso_weekday() {
        let mut tm = zero_tm();

        // Monday=1
        tm.tm_wday = 1;
        assert_eq!(run_strftime(b"%u\0", &tm), b"1");

        // Sunday=7
        tm.tm_wday = 0;
        assert_eq!(run_strftime(b"%u\0", &tm), b"7");
    }

    #[test]
    fn test_strftime_space_padded_day() {
        let mut tm = zero_tm();
        tm.tm_mday = 5;
        assert_eq!(run_strftime(b"%e\0", &tm), b" 5");

        tm.tm_mday = 15;
        assert_eq!(run_strftime(b"%e\0", &tm), b"15");
    }

    #[test]
    fn test_strftime_literal_escapes() {
        let tm = zero_tm();
        assert_eq!(run_strftime(b"%%\0", &tm), b"%");
        assert_eq!(run_strftime(b"%n\0", &tm), b"\n");
        assert_eq!(run_strftime(b"%t\0", &tm), b"\t");
    }

    #[test]
    fn test_strftime_date_composite() {
        let mut tm = zero_tm();
        tm.tm_year = 123; // 2023
        tm.tm_mon = 5;    // June
        tm.tm_mday = 15;
        // %D = %m/%d/%y
        assert_eq!(run_strftime(b"%D\0", &tm), b"06/15/23");
    }

    // -- strptime additional tests --

    #[test]
    fn test_strptime_ampm() {
        let mut tm = zero_tm();
        let rem = unsafe {
            strptime(
                b"02:30 PM\0".as_ptr(),
                b"%I:%M %p\0".as_ptr(),
                &raw mut tm,
            )
        };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_hour, 14); // 2 PM = 14
        assert_eq!(tm.tm_min, 30);
    }

    #[test]
    fn test_strptime_noon_pm() {
        let mut tm = zero_tm();
        unsafe {
            strptime(
                b"12:00 PM\0".as_ptr(),
                b"%I:%M %p\0".as_ptr(),
                &raw mut tm,
            );
        }
        assert_eq!(tm.tm_hour, 12); // 12 PM = noon = 12
    }

    #[test]
    fn test_strptime_midnight_am() {
        let mut tm = zero_tm();
        unsafe {
            strptime(
                b"12:00 AM\0".as_ptr(),
                b"%I:%M %p\0".as_ptr(),
                &raw mut tm,
            );
        }
        assert_eq!(tm.tm_hour, 0); // 12 AM = midnight = 0
    }

    #[test]
    fn test_strptime_month_name() {
        let mut tm = zero_tm();
        let rem = unsafe {
            strptime(
                b"January\0".as_ptr(),
                b"%B\0".as_ptr(),
                &raw mut tm,
            )
        };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_mon, 0); // January = 0
    }

    #[test]
    fn test_strptime_month_abbr() {
        let mut tm = zero_tm();
        let rem = unsafe {
            strptime(
                b"Dec\0".as_ptr(),
                b"%b\0".as_ptr(),
                &raw mut tm,
            )
        };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_mon, 11); // December = 11
    }

    #[test]
    fn test_strptime_weekday_name() {
        let mut tm = zero_tm();
        let rem = unsafe {
            strptime(
                b"Wednesday\0".as_ptr(),
                b"%A\0".as_ptr(),
                &raw mut tm,
            )
        };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_wday, 3); // Wednesday = 3
    }

    #[test]
    fn test_strptime_iso_date() {
        let mut tm = zero_tm();
        let input = b"2023-06-15\0";
        let fmt = b"%Y-%m-%d\0";
        let rem = unsafe {
            strptime(input.as_ptr(), fmt.as_ptr(), &raw mut tm)
        };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_year, 123); // 2023-1900
        assert_eq!(tm.tm_mon, 5);    // June = 5
        assert_eq!(tm.tm_mday, 15);
    }

    #[test]
    fn test_strptime_2digit_year() {
        let mut tm = zero_tm();
        unsafe {
            strptime(b"99\0".as_ptr(), b"%y\0".as_ptr(), &raw mut tm);
        }
        assert_eq!(tm.tm_year, 99); // 1999 - 1900

        let mut tm2 = zero_tm();
        unsafe {
            strptime(b"05\0".as_ptr(), b"%y\0".as_ptr(), &raw mut tm2);
        }
        assert_eq!(tm2.tm_year, 105); // 2005 - 1900
    }

    #[test]
    fn test_strptime_timezone_offset() {
        let mut tm = zero_tm();
        let rem = unsafe {
            strptime(
                b"+0530\0".as_ptr(),
                b"%z\0".as_ptr(),
                &raw mut tm,
            )
        };
        // Should succeed (parse but ignore timezone)
        assert!(!rem.is_null());
    }

    #[test]
    fn test_strptime_literal_percent() {
        let mut tm = zero_tm();
        let rem = unsafe {
            strptime(
                b"100%\0".as_ptr(),
                b"100%%\0".as_ptr(),
                &raw mut tm,
            )
        };
        assert!(!rem.is_null());
    }

    #[test]
    fn test_strptime_returns_remaining() {
        let mut tm = zero_tm();
        let input = b"2023 extra stuff\0";
        let rem = unsafe {
            strptime(input.as_ptr(), b"%Y\0".as_ptr(), &raw mut tm)
        };
        assert!(!rem.is_null());
        // rem should point to " extra stuff"
        assert_eq!(unsafe { *rem }, b' ');
    }

    #[test]
    fn test_strptime_bad_input() {
        let mut tm = zero_tm();
        let rem = unsafe {
            strptime(b"abc\0".as_ptr(), b"%Y\0".as_ptr(), &raw mut tm)
        };
        assert!(rem.is_null()); // No digits for %Y
    }

    // -- ISO 8601 week date tests --

    #[test]
    fn test_iso_week_2015_jan1() {
        // 2015-01-01 is Thursday → ISO week 01 of 2015
        let mut tm = zero_tm();
        tm.tm_year = 115; // 2015
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_wday = 4;   // Thursday
        tm.tm_yday = 0;
        let (year, week) = iso_week_date(&tm);
        assert_eq!(year, 2015);
        assert_eq!(week, 1);
    }

    #[test]
    fn test_iso_week_2014_dec29() {
        // 2014-12-29 is Monday → ISO week 01 of 2015
        let mut tm = zero_tm();
        tm.tm_year = 114; // 2014
        tm.tm_mon = 11;   // December
        tm.tm_mday = 29;
        tm.tm_wday = 1;   // Monday
        tm.tm_yday = 362; // 0-indexed
        let (year, week) = iso_week_date(&tm);
        assert_eq!(year, 2015);
        assert_eq!(week, 1);
    }

    #[test]
    fn test_iso_week_2016_jan1() {
        // 2016-01-01 is Friday → ISO week 53 of 2015
        let mut tm = zero_tm();
        tm.tm_year = 116; // 2016
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_wday = 5;   // Friday
        tm.tm_yday = 0;
        let (year, week) = iso_week_date(&tm);
        assert_eq!(year, 2015);
        assert_eq!(week, 53);
    }

    // -- mktime edge cases --

    #[test]
    fn test_mktime_negative_month() {
        // tm_mon = -1 should borrow from year (December of previous year)
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = -1;   // Should normalize to December 2023
        tm.tm_mday = 15;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_mon, 11);      // December
        assert_eq!(tm.tm_year, 123);     // 2023
        assert_eq!(tm.tm_mday, 15);
    }

    #[test]
    fn test_mktime_overflow_day() {
        // Jan 32 should become Feb 1
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = 0;    // January
        tm.tm_mday = 32;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_mon, 1);   // February
        assert_eq!(tm.tm_mday, 1);
    }

    #[test]
    fn test_mktime_overflow_seconds() {
        // 70 seconds should overflow to 1 min 10 sec
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_sec = 70;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_sec, 10);
        assert_eq!(tm.tm_min, 1);
    }

    // -- clock constants --

    #[test]
    fn test_clock_id_values() {
        assert_eq!(CLOCK_REALTIME, 0);
        assert_eq!(CLOCK_MONOTONIC, 1);
        assert_eq!(CLOCK_PROCESS_CPUTIME_ID, 2);
        assert_eq!(CLOCK_THREAD_CPUTIME_ID, 3);
        assert_eq!(CLOCK_MONOTONIC_RAW, 4);
        assert_eq!(CLOCK_REALTIME_COARSE, 5);
        assert_eq!(CLOCK_MONOTONIC_COARSE, 6);
        assert_eq!(CLOCK_BOOTTIME, 7);
    }

    #[test]
    fn test_is_valid_clock() {
        // All defined clock IDs should be valid.
        assert!(is_valid_clock(CLOCK_REALTIME));
        assert!(is_valid_clock(CLOCK_MONOTONIC));
        assert!(is_valid_clock(CLOCK_PROCESS_CPUTIME_ID));
        assert!(is_valid_clock(CLOCK_THREAD_CPUTIME_ID));
        assert!(is_valid_clock(CLOCK_MONOTONIC_RAW));
        assert!(is_valid_clock(CLOCK_REALTIME_COARSE));
        assert!(is_valid_clock(CLOCK_MONOTONIC_COARSE));
        assert!(is_valid_clock(CLOCK_BOOTTIME));
        // Invalid clock IDs should be rejected.
        assert!(!is_valid_clock(-1));
        assert!(!is_valid_clock(99));
        assert!(!is_valid_clock(8));
    }

    #[test]
    fn test_timer_abstime_value() {
        assert_eq!(TIMER_ABSTIME, 1);
    }

    #[test]
    fn test_sigev_values_match_glibc() {
        // glibc: SIGEV_SIGNAL=0, SIGEV_NONE=1, SIGEV_THREAD=2.
        assert_eq!(SIGEV_SIGNAL, 0);
        assert_eq!(SIGEV_NONE, 1);
        assert_eq!(SIGEV_THREAD, 2);
    }

    #[test]
    fn test_itimer_values_match_glibc() {
        assert_eq!(ITIMER_REAL, 0);
        assert_eq!(ITIMER_VIRTUAL, 1);
        assert_eq!(ITIMER_PROF, 2);
    }

    // -- Struct layout tests --

    #[test]
    fn test_tm_struct_layout() {
        // Tm has 9 i32 fields, repr(C) → 36 bytes, align 4.
        assert_eq!(core::mem::size_of::<Tm>(), 36);
        assert_eq!(core::mem::align_of::<Tm>(), 4);
    }

    #[test]
    fn test_timeval_struct_layout() {
        // Timeval = { tv_sec: i64, tv_usec: i64 } = 16 bytes.
        assert_eq!(core::mem::size_of::<Timeval>(), 16);
        assert_eq!(core::mem::align_of::<Timeval>(), 8);
    }

    #[test]
    fn test_itimerspec_struct_layout() {
        // Itimerspec = 2 × Timespec = 2 × 16 = 32 bytes.
        assert_eq!(core::mem::size_of::<Itimerspec>(), 32);
    }

    #[test]
    fn test_itimerval_struct_layout() {
        // Itimerval = 2 × Timeval = 2 × 16 = 32 bytes.
        assert_eq!(core::mem::size_of::<Itimerval>(), 32);
    }

    #[test]
    fn test_sigevent_struct_layout() {
        // Sigevent must be 64 bytes to match glibc x86_64.
        assert_eq!(core::mem::size_of::<Sigevent>(), 64);
    }

    // -- Additional conversion edge cases --

    #[test]
    fn test_mktime_feb29_nonleap_normalizes() {
        // Feb 29 in a non-leap year should normalize to March 1.
        let mut tm = zero_tm();
        tm.tm_year = 123; // 2023 (not a leap year)
        tm.tm_mon = 1;    // February
        tm.tm_mday = 29;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_mon, 2);   // March
        assert_eq!(tm.tm_mday, 1);
    }

    #[test]
    fn test_mktime_mday_zero_borrows() {
        // mday=0 should be last day of previous month.
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024 (leap year)
        tm.tm_mon = 2;    // March
        tm.tm_mday = 0;   // → Feb 29 (leap year)
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_mon, 1);   // February
        assert_eq!(tm.tm_mday, 29); // Leap day
    }

    #[test]
    fn test_mktime_negative_seconds() {
        // -1 seconds should borrow: sec=59, min decremented.
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_hour = 1;
        tm.tm_min = 0;
        tm.tm_sec = -1;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_sec, 59);
        assert_eq!(tm.tm_min, 59);
        assert_eq!(tm.tm_hour, 0);
    }

    #[test]
    fn test_gmtime_1900_jan1() {
        // 1900-01-01 00:00:00 UTC = -2208988800
        let t: TimeT = -2_208_988_800;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 0);    // 1900 - 1900
        assert_eq!(tm.tm_mon, 0);     // January
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_wday, 1);    // Monday
        assert_eq!(tm.tm_yday, 0);
    }

    #[test]
    fn test_gmtime_2038_boundary() {
        // 2038-01-19 03:14:07 UTC = 2^31 - 1 (max 32-bit time_t).
        let t: TimeT = 2_147_483_647;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 138);  // 2038 - 1900
        assert_eq!(tm.tm_mon, 0);     // January
        assert_eq!(tm.tm_mday, 19);
        assert_eq!(tm.tm_hour, 3);
        assert_eq!(tm.tm_min, 14);
        assert_eq!(tm.tm_sec, 7);
    }

    #[test]
    fn test_gmtime_mktime_roundtrip_leap_years() {
        // Test roundtrip for several leap-year Feb 29 timestamps.
        let leap_feb29_timestamps: &[TimeT] = &[
            68169600,    // 1972-02-29 00:00:00 UTC
            951782400,   // 2000-02-29 00:00:00 UTC (century leap)
            1709164800,  // 2024-02-29 00:00:00 UTC
        ];
        for &t in leap_feb29_timestamps {
            let tm = gmtime(&t);
            let tm = unsafe { &mut *tm };
            assert_eq!(tm.tm_mon, 1, "timestamp {t}: expected February");
            assert_eq!(tm.tm_mday, 29, "timestamp {t}: expected 29th");
            let t2 = mktime(tm);
            assert_eq!(t, t2, "roundtrip failed for leap Feb 29 timestamp {t}");
        }
    }

    // -- strftime week number tests --

    #[test]
    fn test_strftime_week_number_sunday() {
        // 2023-01-01 is Sunday (wday=0, yday=0).
        let mut tm = zero_tm();
        tm.tm_year = 123;
        tm.tm_wday = 0;
        tm.tm_yday = 0;
        // %U: Sunday starts the first week. Jan 1 Sunday → week 01.
        assert_eq!(run_strftime(b"%U\0", &tm), b"01");

        // 2024-01-01 is Monday (wday=1, yday=0).
        tm.tm_year = 124;
        tm.tm_wday = 1;
        tm.tm_yday = 0;
        // %U: before first Sunday → week 00.
        assert_eq!(run_strftime(b"%U\0", &tm), b"00");
    }

    #[test]
    fn test_strftime_week_number_monday() {
        // 2024-01-01 is Monday (wday=1, yday=0).
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_wday = 1;
        tm.tm_yday = 0;
        // %W: Monday starts the first week. Jan 1 Monday → week 01.
        assert_eq!(run_strftime(b"%W\0", &tm), b"01");

        // 2023-01-01 is Sunday (wday=0, yday=0).
        tm.tm_year = 123;
        tm.tm_wday = 0;
        tm.tm_yday = 0;
        // %W: before first Monday → week 00.
        assert_eq!(run_strftime(b"%W\0", &tm), b"00");
    }

    #[test]
    fn test_strftime_composite_c() {
        // %c = asctime format: "Thu Jan  1 00:00:00 1970"
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_wday = 4;
        let result = run_strftime(b"%c\0", &tm);
        assert_eq!(result, b"Thu Jan  1 00:00:00 1970");
    }

    #[test]
    fn test_strftime_epoch_seconds() {
        // %s = seconds since epoch (GNU extension).
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 2;   // 86400 seconds from epoch
        let result = run_strftime(b"%s\0", &tm);
        assert_eq!(result, b"86400");
    }

    #[test]
    fn test_hour_12_all_values() {
        assert_eq!(hour_12(0), 12);   // midnight
        assert_eq!(hour_12(1), 1);
        assert_eq!(hour_12(11), 11);
        assert_eq!(hour_12(12), 12);  // noon
        assert_eq!(hour_12(13), 1);
        assert_eq!(hour_12(23), 11);
    }

    // -- CLOCKS_PER_SEC --

    #[test]
    fn test_clocks_per_sec() {
        // POSIX requires CLOCKS_PER_SEC = 1_000_000.
        assert_eq!(CLOCKS_PER_SEC, 1_000_000);
    }

    // -- is_leap edge cases --

    #[test]
    fn test_is_leap_century_boundary() {
        // 1600 is a leap year (divisible by 400).
        assert!(is_leap(1600));
        // 1700, 1800 are NOT leap years (divisible by 100, not 400).
        assert!(!is_leap(1700));
        assert!(!is_leap(1800));
        // 2100 is NOT a leap year.
        assert!(!is_leap(2100));
        // 2400 IS a leap year.
        assert!(is_leap(2400));
    }

    #[test]
    fn test_is_leap_common_years() {
        assert!(!is_leap(2001));
        assert!(!is_leap(2002));
        assert!(!is_leap(2003));
        assert!(is_leap(2004));
        assert!(!is_leap(2005));
    }

    // -- secs_to_tm edge cases --

    #[test]
    fn test_secs_to_tm_pre_epoch_1960() {
        // 1960-01-01 00:00:00 UTC = -315619200
        let t: TimeT = -315_619_200;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 60);    // 1960
        assert_eq!(tm.tm_mon, 0);      // January
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_min, 0);
        assert_eq!(tm.tm_sec, 0);
    }

    #[test]
    fn test_secs_to_tm_y2k38_plus_one() {
        // 2038-01-19 03:14:08 — first second past 32-bit overflow.
        let t: TimeT = 2_147_483_648;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 138);
        assert_eq!(tm.tm_mon, 0);
        assert_eq!(tm.tm_mday, 19);
        assert_eq!(tm.tm_hour, 3);
        assert_eq!(tm.tm_min, 14);
        assert_eq!(tm.tm_sec, 8);
    }

    #[test]
    fn test_secs_to_tm_non_leap_century() {
        // 2100-03-01 00:00:00 UTC — tests non-leap century year 2100.
        let t: TimeT = 4_107_542_400;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 200);   // 2100
        assert_eq!(tm.tm_mon, 2);      // March (2100 is NOT a leap year)
        assert_eq!(tm.tm_mday, 1);
    }

    #[test]
    fn test_secs_to_tm_1970_jan_02() {
        // 86400 seconds = 1970-01-02 00:00:00.
        let t: TimeT = 86400;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 70);
        assert_eq!(tm.tm_mon, 0);
        assert_eq!(tm.tm_mday, 2);
        assert_eq!(tm.tm_yday, 1);
        assert_eq!(tm.tm_wday, 5); // Friday.
    }

    // -- mktime normalization edge cases --

    #[test]
    fn test_mktime_negative_hour_borrows() {
        // -1 hour from midnight Jan 1 → 23:00 Dec 31 previous year.
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_hour = -1;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_hour, 23);
        assert_eq!(tm.tm_mday, 31);
        assert_eq!(tm.tm_mon, 11);  // December
        assert_eq!(tm.tm_year, 123); // 2023
    }

    #[test]
    fn test_mktime_large_seconds_cascade() {
        // 3661 seconds = 1 hour, 1 minute, 1 second.
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_sec = 3661;
        let t = mktime(&mut tm);
        assert_eq!(t, 3661);
        assert_eq!(tm.tm_hour, 1);
        assert_eq!(tm.tm_min, 1);
        assert_eq!(tm.tm_sec, 1);
    }

    #[test]
    fn test_mktime_month_negative_deep() {
        // Month -13 should go back a full year + 1 month.
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = -13;  // Should normalize to November 2022.
        tm.tm_mday = 1;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_year, 122); // 2022
        assert_eq!(tm.tm_mon, 11);   // December
    }

    #[test]
    fn test_mktime_month_large_positive() {
        // Month 24 = 2 years forward.
        let mut tm = zero_tm();
        tm.tm_year = 70; // 1970
        tm.tm_mon = 24;  // 2 years = 1972 January
        tm.tm_mday = 1;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_year, 72);
        assert_eq!(tm.tm_mon, 0);
    }

    // -- iso_week_date edge cases --

    #[test]
    fn test_iso_week_jan1_2024() {
        // 2024-01-01 is Monday (wday=1, yday=0).
        // ISO week 01 of 2024 (first Thursday = Jan 4).
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_wday = 1; // Monday
        tm.tm_yday = 0;
        let (iso_year, iso_week) = iso_week_date(&tm);
        assert_eq!(iso_year, 2024);
        assert_eq!(iso_week, 1);
    }

    #[test]
    fn test_iso_week_dec31_2024() {
        // 2024-12-31 is Tuesday (wday=2, yday=365 in leap year).
        // ISO: The Thursday of this week is Jan 2, 2025 → week 01 of 2025.
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_wday = 2; // Tuesday
        tm.tm_yday = 365;
        let (iso_year, iso_week) = iso_week_date(&tm);
        assert_eq!(iso_year, 2025);
        assert_eq!(iso_week, 1);
    }

    #[test]
    fn test_iso_week_dec29_2014() {
        // 2014-12-29 is Monday (wday=1, yday=362).
        // Thursday of this ISO week = Jan 1, 2015 → week 01 of 2015.
        let mut tm = zero_tm();
        tm.tm_year = 114;
        tm.tm_wday = 1; // Monday
        tm.tm_yday = 362;
        let (iso_year, iso_week) = iso_week_date(&tm);
        assert_eq!(iso_year, 2015);
        assert_eq!(iso_week, 1);
    }

    #[test]
    fn test_iso_week_jan1_2016_in_prev_year() {
        // 2016-01-01 is Friday (wday=5, yday=0).
        // Thursday of this week is Dec 31, 2015 → still in 2015's weeks.
        // 2015 has 53 weeks (2015-01-01 is Thursday).
        let mut tm = zero_tm();
        tm.tm_year = 116;
        tm.tm_wday = 5; // Friday
        tm.tm_yday = 0;
        let (iso_year, iso_week) = iso_week_date(&tm);
        assert_eq!(iso_year, 2015);
        assert_eq!(iso_week, 53);
    }

    #[test]
    fn test_iso_week_mid_year_2024() {
        // 2024-06-15 is Saturday (wday=6, yday=166 in leap year).
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_wday = 6; // Saturday
        tm.tm_yday = 166;
        let (iso_year, iso_week) = iso_week_date(&tm);
        assert_eq!(iso_year, 2024);
        // Thursday of this ISO week: 166 + 4 - 6 = 164; 164/7 + 1 = 24.
        assert_eq!(iso_week, 24);
    }

    // -- gmtime/mktime roundtrip for extended range --

    #[test]
    fn test_roundtrip_post_2038_timestamps() {
        let timestamps: &[TimeT] = &[
            2_147_483_648,  // 2038-01-19 03:14:08
            4_107_542_400,  // 2100-03-01
        ];
        for &t in timestamps {
            let tm = gmtime(&t);
            let tm = unsafe { &mut *tm };
            let t2 = mktime(tm);
            assert_eq!(t, t2, "roundtrip failed for post-2038 timestamp {t}");
        }
    }

    // -- wday cycle via gmtime --

    #[test]
    fn test_gmtime_weekday_cycle() {
        // 1970-01-01 (Thu=4) through 1970-01-07 (Wed=3).
        let expected_wdays = [4, 5, 6, 0, 1, 2, 3]; // Thu..Wed
        for (i, &expected) in expected_wdays.iter().enumerate() {
            let t: TimeT = (i as i64) * 86400;
            let tm = gmtime(&t);
            let tm = unsafe { &*tm };
            assert_eq!(
                tm.tm_wday, expected,
                "day {} from epoch should be wday {}, got {}",
                i, expected, tm.tm_wday
            );
        }
    }

    #[test]
    fn test_gmtime_end_of_1970() {
        // Last second of 1970: 1970-12-31 23:59:59.
        let t: TimeT = 365 * 86400 - 1;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 70);
        assert_eq!(tm.tm_mon, 11);  // December
        assert_eq!(tm.tm_mday, 31);
        assert_eq!(tm.tm_hour, 23);
        assert_eq!(tm.tm_min, 59);
        assert_eq!(tm.tm_sec, 59);
        assert_eq!(tm.tm_yday, 364);
    }

    // -- strftime ISO week (%V, %G, %g) --

    #[test]
    fn test_strftime_iso_week_v() {
        // 2024-01-01 is Monday (wday=1, yday=0) → ISO week 01.
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_wday = 1;
        tm.tm_yday = 0;
        assert_eq!(run_strftime(b"%V\0", &tm), b"01");
    }

    #[test]
    fn test_strftime_iso_week_v_53() {
        // 2016-01-01 is Friday (wday=5, yday=0) → ISO W53 of 2015.
        let mut tm = zero_tm();
        tm.tm_year = 116;
        tm.tm_wday = 5;
        tm.tm_yday = 0;
        assert_eq!(run_strftime(b"%V\0", &tm), b"53");
    }

    #[test]
    fn test_strftime_iso_year_g() {
        // 2024-01-01 Monday → ISO year 2024.
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_wday = 1;
        tm.tm_yday = 0;
        assert_eq!(run_strftime(b"%G\0", &tm), b"2024");
        assert_eq!(run_strftime(b"%g\0", &tm), b"24");
    }

    #[test]
    fn test_strftime_iso_year_cross_boundary() {
        // 2016-01-01 is Friday → ISO year is 2015 (W53 of 2015).
        let mut tm = zero_tm();
        tm.tm_year = 116; // Calendar year 2016.
        tm.tm_wday = 5;
        tm.tm_yday = 0;
        assert_eq!(run_strftime(b"%G\0", &tm), b"2015"); // ISO year differs!
        assert_eq!(run_strftime(b"%g\0", &tm), b"15");
    }

    // -- strptime edge cases --

    #[test]
    fn test_strptime_am_pm() {
        let mut tm = zero_tm();
        let input = b"03:30 PM\0";
        let fmt = b"%I:%M %p\0";
        let result = unsafe { strptime(input.as_ptr(), fmt.as_ptr(), &mut tm) };
        assert!(!result.is_null());
        assert_eq!(tm.tm_hour, 15); // 3 PM = 15.
        assert_eq!(tm.tm_min, 30);
    }

    #[test]
    fn test_strptime_am() {
        let mut tm = zero_tm();
        let input = b"11:00 AM\0";
        let fmt = b"%I:%M %p\0";
        let result = unsafe { strptime(input.as_ptr(), fmt.as_ptr(), &mut tm) };
        assert!(!result.is_null());
        assert_eq!(tm.tm_hour, 11);
    }

    #[test]
    fn test_strptime_day_abbrev_month_year() {
        let mut tm = zero_tm();
        let input = b"15 Mar 2024\0";
        let fmt = b"%d %b %Y\0";
        let result = unsafe { strptime(input.as_ptr(), fmt.as_ptr(), &mut tm) };
        assert!(!result.is_null());
        assert_eq!(tm.tm_mday, 15);
        assert_eq!(tm.tm_mon, 2);     // March
        assert_eq!(tm.tm_year, 124);  // 2024 - 1900
    }

    #[test]
    fn test_strptime_full_month() {
        let mut tm = zero_tm();
        let input = b"December\0";
        let fmt = b"%B\0";
        let result = unsafe { strptime(input.as_ptr(), fmt.as_ptr(), &mut tm) };
        assert!(!result.is_null());
        assert_eq!(tm.tm_mon, 11);
    }

    #[test]
    fn test_strptime_weekday_abbrev() {
        let mut tm = zero_tm();
        let input = b"Fri\0";
        let fmt = b"%a\0";
        let result = unsafe { strptime(input.as_ptr(), fmt.as_ptr(), &mut tm) };
        assert!(!result.is_null());
        assert_eq!(tm.tm_wday, 5); // Friday
    }

    // -------------------------------------------------------------------
    // Additional edge cases — time conversion functions
    // -------------------------------------------------------------------

    #[test]
    fn test_secs_to_tm_negative_one_second() {
        // -1 = 1969-12-31 23:59:59
        let mut tm = zero_tm();
        secs_to_tm(-1, &mut tm);
        assert_eq!(tm.tm_year, 69);   // 1969
        assert_eq!(tm.tm_mon, 11);    // December
        assert_eq!(tm.tm_mday, 31);
        assert_eq!(tm.tm_hour, 23);
        assert_eq!(tm.tm_min, 59);
        assert_eq!(tm.tm_sec, 59);
        assert_eq!(tm.tm_wday, 3);    // Wednesday
    }

    #[test]
    fn test_secs_to_tm_end_of_day() {
        // 86399 = 1970-01-01 23:59:59
        let mut tm = zero_tm();
        secs_to_tm(86399, &mut tm);
        assert_eq!(tm.tm_hour, 23);
        assert_eq!(tm.tm_min, 59);
        assert_eq!(tm.tm_sec, 59);
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_mon, 0);
        assert_eq!(tm.tm_year, 70);
    }

    #[test]
    fn test_secs_to_tm_start_of_day_2() {
        // 86400 = 1970-01-02 00:00:00
        let mut tm = zero_tm();
        secs_to_tm(86400, &mut tm);
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_min, 0);
        assert_eq!(tm.tm_sec, 0);
        assert_eq!(tm.tm_mday, 2);
        assert_eq!(tm.tm_mon, 0);
        assert_eq!(tm.tm_year, 70);
        assert_eq!(tm.tm_wday, 5);    // Friday
    }

    #[test]
    fn test_mktime_dec31_to_jan1() {
        // Dec 32 should normalize to Jan 1 of next year.
        let mut tm = zero_tm();
        tm.tm_year = 70;  // 1970
        tm.tm_mon = 11;   // December
        tm.tm_mday = 32;  // Dec 32 = Jan 1 next year
        let secs = tm_to_secs(&mut tm);

        assert_eq!(tm.tm_year, 71);   // Normalized to 1971
        assert_eq!(tm.tm_mon, 0);     // January
        assert_eq!(tm.tm_mday, 1);

        // Verify via round-trip
        let mut tm2 = zero_tm();
        secs_to_tm(secs, &mut tm2);
        assert_eq!(tm2.tm_year, 71);
        assert_eq!(tm2.tm_mon, 0);
        assert_eq!(tm2.tm_mday, 1);
    }

    #[test]
    fn test_mktime_feb30_normalizes() {
        // Feb 30 in a non-leap year should normalize to March 2.
        let mut tm = zero_tm();
        tm.tm_year = 123;  // 2023 (non-leap)
        tm.tm_mon = 1;     // February
        tm.tm_mday = 30;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_mon, 2);     // March
        assert_eq!(tm.tm_mday, 2);
    }

    #[test]
    fn test_mktime_feb30_leap_normalizes() {
        // Feb 30 in a leap year should normalize to March 1.
        let mut tm = zero_tm();
        tm.tm_year = 124;  // 2024 (leap year)
        tm.tm_mon = 1;     // February
        tm.tm_mday = 30;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_mon, 2);     // March
        assert_eq!(tm.tm_mday, 1);
    }

    #[test]
    fn test_mktime_negative_mday() {
        // mday = 0 should borrow from previous month (Dec 31).
        let mut tm = zero_tm();
        tm.tm_year = 71;  // 1971
        tm.tm_mon = 0;    // January
        tm.tm_mday = 0;   // Jan 0 = Dec 31 of prev year
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_year, 70);   // 1970
        assert_eq!(tm.tm_mon, 11);    // December
        assert_eq!(tm.tm_mday, 31);
    }

    #[test]
    fn test_mktime_deeply_negative_mday() {
        // mday = -30 in January: Jan 0 = Dec 31, Jan -30 = Dec 1.
        let mut tm = zero_tm();
        tm.tm_year = 71;  // 1971
        tm.tm_mon = 0;    // January
        tm.tm_mday = -30;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_year, 70);   // 1970
        assert_eq!(tm.tm_mon, 11);    // December
        assert_eq!(tm.tm_mday, 1);
    }

    #[test]
    fn test_mktime_hour_overflow_crosses_day() {
        // 25 hours should become 1 hour next day.
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_hour = 25;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_hour, 1);
        assert_eq!(tm.tm_mday, 2);
    }

    #[test]
    fn test_mktime_minute_overflow_crosses_hour() {
        // 90 minutes should become 1 hour 30 minutes.
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_min = 90;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_min, 30);
        assert_eq!(tm.tm_hour, 1);
    }

    #[test]
    fn test_mktime_negative_minutes_borrows() {
        // -1 minute should borrow from hour.
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_hour = 2;
        tm.tm_min = -1;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_min, 59);
        assert_eq!(tm.tm_hour, 1);
    }

    #[test]
    fn test_secs_to_tm_leap_second_boundary() {
        // Last second of 2000-02-29 (leap day):
        // 2000-02-29 23:59:59 UTC
        // From epoch: 30 years of days...
        // Let's compute: 2000-03-01 = days 11017
        // 2000-02-29 23:59:59 = (11017 - 1) * 86400 + 86399
        //   = 11016 * 86400 + 86399
        //   = 951782400 + 86399 = 951868799
        // But let's just verify the roundtrip.
        let mut tm = zero_tm();
        tm.tm_year = 100;  // 2000
        tm.tm_mon = 1;     // February
        tm.tm_mday = 29;   // Feb 29 (leap day)
        tm.tm_hour = 23;
        tm.tm_min = 59;
        tm.tm_sec = 59;
        let secs = tm_to_secs(&mut tm);

        let mut tm2 = zero_tm();
        secs_to_tm(secs, &mut tm2);
        assert_eq!(tm2.tm_year, 100);
        assert_eq!(tm2.tm_mon, 1);
        assert_eq!(tm2.tm_mday, 29);
        assert_eq!(tm2.tm_hour, 23);
        assert_eq!(tm2.tm_min, 59);
        assert_eq!(tm2.tm_sec, 59);
    }

    #[test]
    fn test_secs_to_tm_first_second_march_2000() {
        // First second after leap day: 2000-03-01 00:00:00
        let mut tm = zero_tm();
        tm.tm_year = 100;
        tm.tm_mon = 2;     // March
        tm.tm_mday = 1;
        let secs = tm_to_secs(&mut tm);

        let mut tm2 = zero_tm();
        secs_to_tm(secs, &mut tm2);
        assert_eq!(tm2.tm_year, 100);
        assert_eq!(tm2.tm_mon, 2);
        assert_eq!(tm2.tm_mday, 1);
        assert_eq!(tm2.tm_hour, 0);
    }

    #[test]
    fn test_mktime_yday_computation() {
        // Jan 1 should have yday=0.
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_yday, 0);

        // Feb 1 should have yday=31.
        let mut tm2 = zero_tm();
        tm2.tm_year = 70;
        tm2.tm_mon = 1;
        tm2.tm_mday = 1;
        tm_to_secs(&mut tm2);
        assert_eq!(tm2.tm_yday, 31);

        // Dec 31 in non-leap year should have yday=364.
        let mut tm3 = zero_tm();
        tm3.tm_year = 70;
        tm3.tm_mon = 11;
        tm3.tm_mday = 31;
        tm_to_secs(&mut tm3);
        assert_eq!(tm3.tm_yday, 364);
    }

    #[test]
    fn test_mktime_yday_leap_year() {
        // Dec 31 in leap year should have yday=365.
        let mut tm = zero_tm();
        tm.tm_year = 100;  // 2000 (leap)
        tm.tm_mon = 11;
        tm.tm_mday = 31;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_yday, 365);
    }

    #[test]
    fn test_secs_to_tm_wday_sequence() {
        // Epoch (Thursday) through the next week.
        let days = [4, 5, 6, 0, 1, 2, 3]; // Thu Fri Sat Sun Mon Tue Wed
        for (i, &expected_wday) in days.iter().enumerate() {
            let mut tm = zero_tm();
            secs_to_tm((i as i64) * 86400, &mut tm);
            assert_eq!(tm.tm_wday, expected_wday,
                "day {i} should be wday {expected_wday}, got {}",
                tm.tm_wday);
        }
    }

    #[test]
    fn test_difftime_negative() {
        // difftime(0, 100) should be -100.
        let d = difftime(0, 100);
        assert!((d - (-100.0)).abs() < 0.001);
    }

    #[test]
    fn test_difftime_same() {
        let d = difftime(1000, 1000);
        assert!((d - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_mktime_wday_sunday() {
        // 1970-01-04 was Sunday (wday=0).
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 4;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_wday, 0, "1970-01-04 should be Sunday");
    }

    #[test]
    fn test_asctime_epoch() {
        let secs: TimeT = 0;
        let tm_ptr = gmtime(&raw const secs);
        assert!(!tm_ptr.is_null());
        let result = asctime(tm_ptr);
        assert!(!result.is_null());
        let len = unsafe { crate::string::strlen(result) };
        let s = unsafe { core::slice::from_raw_parts(result, len) };
        // "Thu Jan  1 00:00:00 1970\n"
        assert_eq!(s, b"Thu Jan  1 00:00:00 1970\n");
    }

    #[test]
    fn test_gmtime_r_basic() {
        let secs: TimeT = 0;
        let mut tm = zero_tm();
        let ret = unsafe { gmtime_r(&raw const secs, &raw mut tm) };
        assert!(!ret.is_null());
        assert_eq!(tm.tm_year, 70);
        assert_eq!(tm.tm_mon, 0);
        assert_eq!(tm.tm_mday, 1);
    }

    #[test]
    fn test_gmtime_r_null_params() {
        let mut tm = zero_tm();
        let ret = unsafe { gmtime_r(core::ptr::null(), &raw mut tm) };
        assert!(ret.is_null());

        let secs: TimeT = 0;
        let ret2 = unsafe { gmtime_r(&raw const secs, core::ptr::null_mut()) };
        assert!(ret2.is_null());
    }

    #[test]
    fn test_ctime_r_basic() {
        let secs: TimeT = 0;
        let mut buf = [0u8; 32];
        let ret = unsafe { ctime_r(&raw const secs, buf.as_mut_ptr()) };
        assert!(!ret.is_null());
        let len = unsafe { crate::string::strlen(buf.as_ptr()) };
        let s = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
        assert_eq!(s, b"Thu Jan  1 00:00:00 1970\n");
    }

    #[test]
    fn test_ctime_r_null_params() {
        let mut buf = [0u8; 32];
        let ret = unsafe { ctime_r(core::ptr::null(), buf.as_mut_ptr()) };
        assert!(ret.is_null());

        let secs: TimeT = 0;
        let ret2 = unsafe { ctime_r(&raw const secs, core::ptr::null_mut()) };
        assert!(ret2.is_null());
    }

    #[test]
    fn test_asctime_r_basic() {
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_wday = 4;
        let mut buf = [0u8; 32];
        let ret = unsafe { asctime_r(&raw const tm, buf.as_mut_ptr()) };
        assert!(!ret.is_null());
        let len = unsafe { crate::string::strlen(buf.as_ptr()) };
        let s = unsafe { core::slice::from_raw_parts(buf.as_ptr(), len) };
        assert_eq!(s, b"Thu Jan  1 00:00:00 1970\n");
    }

    #[test]
    fn test_asctime_r_null_params() {
        let tm = zero_tm();
        let ret = unsafe { asctime_r(&raw const tm, core::ptr::null_mut()) };
        assert!(ret.is_null());

        let mut buf = [0u8; 32];
        let ret2 = unsafe { asctime_r(core::ptr::null(), buf.as_mut_ptr()) };
        assert!(ret2.is_null());
    }

    #[test]
    fn test_clock_settime_returns_eperm() {
        let ts = Timespec { tv_sec: 0, tv_nsec: 0 };
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_settimeofday_returns_eperm() {
        let tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_tzset_is_noop() {
        // Just verify it doesn't crash.
        tzset();
    }

    #[test]
    fn test_timezone_globals() {
        assert_eq!(timezone, 0);
        assert_eq!(daylight, 0);
    }

    // -------------------------------------------------------------------
    // clock_getres — pure logic (no syscalls)
    // -------------------------------------------------------------------

    #[test]
    fn test_clock_getres_monotonic() {
        let mut ts = Timespec { tv_sec: 99, tv_nsec: 99 };
        let ret = clock_getres(CLOCK_MONOTONIC, &raw mut ts);
        assert_eq!(ret, 0);
        assert_eq!(ts.tv_sec, 0);
        assert_eq!(ts.tv_nsec, 1); // 1ns resolution
    }

    #[test]
    fn test_clock_getres_realtime() {
        let mut ts = Timespec { tv_sec: 99, tv_nsec: 99 };
        let ret = clock_getres(CLOCK_REALTIME, &raw mut ts);
        assert_eq!(ret, 0);
        assert_eq!(ts.tv_sec, 0);
        assert_eq!(ts.tv_nsec, 1);
    }

    #[test]
    fn test_clock_getres_null_res_ok() {
        // Passing null for res is valid — just checks the clock_id.
        let ret = clock_getres(CLOCK_MONOTONIC, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_clock_getres_invalid_clock() {
        let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
        crate::errno::set_errno(0);
        let ret = clock_getres(999, &raw mut ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_clock_gettime_null_tp() {
        crate::errno::set_errno(0);
        let ret = clock_gettime(CLOCK_MONOTONIC, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_clock_gettime_invalid_clock() {
        let mut ts = Timespec { tv_sec: 0, tv_nsec: 0 };
        crate::errno::set_errno(0);
        let ret = clock_gettime(999, &raw mut ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- timer_create / timer_settime / timer_gettime / timer_delete --

    /// Helper: reset TIMER_TABLE and ITIMER_STATE for isolation.
    fn reset_timers() {
        // SAFETY: single-threaded test, no concurrent access.
        unsafe {
            let table = core::ptr::addr_of_mut!(TIMER_TABLE).as_mut().unwrap();
            for slot in table.iter_mut() {
                *slot = None;
            }
            let state = core::ptr::addr_of_mut!(ITIMER_STATE).as_mut().unwrap();
            for entry in state.iter_mut() {
                *entry = Itimerval {
                    it_interval: Timeval { tv_sec: 0, tv_usec: 0 },
                    it_value: Timeval { tv_sec: 0, tv_usec: 0 },
                };
            }
        }
    }

    #[test]
    fn test_timer_create_basic() {
        reset_timers();
        let mut id: TimerT = 999;
        let ret = timer_create(CLOCK_MONOTONIC, core::ptr::null(), &raw mut id);
        assert_eq!(ret, 0);
        assert_eq!(id, 0); // First slot.
        // Clean up.
        timer_delete(id);
    }

    #[test]
    fn test_timer_create_multiple() {
        reset_timers();
        let mut id1: TimerT = 0;
        let mut id2: TimerT = 0;
        assert_eq!(timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id1), 0);
        assert_eq!(timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id2), 0);
        assert_ne!(id1, id2, "two timers should get distinct IDs");
        timer_delete(id1);
        timer_delete(id2);
    }

    #[test]
    fn test_timer_create_null_timerid() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_create(CLOCK_REALTIME, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_timer_create_reuse_deleted_slot() {
        reset_timers();
        let mut id: TimerT = 0;
        assert_eq!(timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id), 0);
        let first_id = id;
        assert_eq!(timer_delete(id), 0);

        // Create again — should reuse slot 0.
        assert_eq!(timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id), 0);
        assert_eq!(id, first_id, "deleted slot should be reused");
        timer_delete(id);
    }

    #[test]
    fn test_timer_delete_invalid() {
        reset_timers();
        crate::errno::set_errno(0);
        // Delete a timer that was never created → EINVAL.
        let ret = timer_delete(0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_timer_delete_double_delete() {
        reset_timers();
        let mut id: TimerT = 0;
        assert_eq!(timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id), 0);
        assert_eq!(timer_delete(id), 0);
        // Second delete should fail.
        crate::errno::set_errno(0);
        assert_eq!(timer_delete(id), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_timer_settime_basic() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        let new_val = Itimerspec {
            it_interval: Timespec { tv_sec: 1, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 5, tv_nsec: 0 },
        };
        let ret = timer_settime(id, 0, &raw const new_val, core::ptr::null_mut());
        assert_eq!(ret, 0);
        timer_delete(id);
    }

    #[test]
    fn test_timer_settime_returns_old_value() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        // Set initial value.
        let val1 = Itimerspec {
            it_interval: Timespec { tv_sec: 2, tv_nsec: 100 },
            it_value: Timespec { tv_sec: 10, tv_nsec: 200 },
        };
        timer_settime(id, 0, &raw const val1, core::ptr::null_mut());

        // Set new value and retrieve old.
        let val2 = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        let mut old = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        let ret = timer_settime(id, 0, &raw const val2, &raw mut old);
        assert_eq!(ret, 0);
        assert_eq!(old.it_interval.tv_sec, 2);
        assert_eq!(old.it_interval.tv_nsec, 100);
        assert_eq!(old.it_value.tv_sec, 10);
        assert_eq!(old.it_value.tv_nsec, 200);
        timer_delete(id);
    }

    #[test]
    fn test_timer_settime_null_new_value() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        timer_delete(id);
    }

    #[test]
    fn test_timer_settime_invalid_timer() {
        reset_timers();
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(0, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_timer_gettime_retrieves_set_value() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 3, tv_nsec: 500 },
            it_value: Timespec { tv_sec: 7, tv_nsec: 999 },
        };
        timer_settime(id, 0, &raw const val, core::ptr::null_mut());

        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        let ret = timer_gettime(id, &raw mut out);
        assert_eq!(ret, 0);
        assert_eq!(out.it_interval.tv_sec, 3);
        assert_eq!(out.it_interval.tv_nsec, 500);
        assert_eq!(out.it_value.tv_sec, 7);
        assert_eq!(out.it_value.tv_nsec, 999);
        timer_delete(id);
    }

    #[test]
    fn test_timer_gettime_null_curr_value() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        crate::errno::set_errno(0);
        let ret = timer_gettime(id, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        timer_delete(id);
    }

    #[test]
    fn test_timer_gettime_invalid_timer() {
        reset_timers();
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_gettime(0, &raw mut out);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_timer_getoverrun_returns_zero() {
        assert_eq!(timer_getoverrun(0), 0);
        assert_eq!(timer_getoverrun(99), 0);
    }

    // -- setitimer / getitimer --

    #[test]
    fn test_setitimer_valid_which() {
        reset_timers();
        let val = Itimerval {
            it_interval: Timeval { tv_sec: 0, tv_usec: 0 },
            it_value: Timeval { tv_sec: 1, tv_usec: 0 },
        };
        assert_eq!(setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut()), 0);
        assert_eq!(setitimer(ITIMER_VIRTUAL, &raw const val, core::ptr::null_mut()), 0);
        assert_eq!(setitimer(ITIMER_PROF, &raw const val, core::ptr::null_mut()), 0);
    }

    #[test]
    fn test_setitimer_invalid_which() {
        reset_timers();
        let val = Itimerval {
            it_interval: Timeval { tv_sec: 0, tv_usec: 0 },
            it_value: Timeval { tv_sec: 0, tv_usec: 0 },
        };
        crate::errno::set_errno(0);
        assert_eq!(setitimer(99, &raw const val, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_setitimer_null_new_value() {
        crate::errno::set_errno(0);
        assert_eq!(setitimer(ITIMER_REAL, core::ptr::null(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_setitimer_returns_old_value() {
        reset_timers();
        // First set: old should be zeros (fresh state).
        let val1 = Itimerval {
            it_interval: Timeval { tv_sec: 1, tv_usec: 100 },
            it_value: Timeval { tv_sec: 5, tv_usec: 200 },
        };
        let mut old = Itimerval {
            it_interval: Timeval { tv_sec: 99, tv_usec: 99 },
            it_value: Timeval { tv_sec: 99, tv_usec: 99 },
        };
        assert_eq!(setitimer(ITIMER_REAL, &raw const val1, &raw mut old), 0);
        assert_eq!(old.it_interval.tv_sec, 0);
        assert_eq!(old.it_interval.tv_usec, 0);
        assert_eq!(old.it_value.tv_sec, 0);
        assert_eq!(old.it_value.tv_usec, 0);

        // Second set: old should be val1.
        let val2 = Itimerval {
            it_interval: Timeval { tv_sec: 0, tv_usec: 0 },
            it_value: Timeval { tv_sec: 0, tv_usec: 0 },
        };
        assert_eq!(setitimer(ITIMER_REAL, &raw const val2, &raw mut old), 0);
        assert_eq!(old.it_interval.tv_sec, 1);
        assert_eq!(old.it_interval.tv_usec, 100);
        assert_eq!(old.it_value.tv_sec, 5);
        assert_eq!(old.it_value.tv_usec, 200);
    }

    #[test]
    fn test_getitimer_returns_set_value() {
        reset_timers();
        let val = Itimerval {
            it_interval: Timeval { tv_sec: 3, tv_usec: 500 },
            it_value: Timeval { tv_sec: 7, tv_usec: 999 },
        };
        setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());

        let mut out = Itimerval {
            it_interval: Timeval { tv_sec: 0, tv_usec: 0 },
            it_value: Timeval { tv_sec: 0, tv_usec: 0 },
        };
        assert_eq!(getitimer(ITIMER_REAL, &raw mut out), 0);
        assert_eq!(out.it_interval.tv_sec, 3);
        assert_eq!(out.it_interval.tv_usec, 500);
        assert_eq!(out.it_value.tv_sec, 7);
        assert_eq!(out.it_value.tv_usec, 999);
    }

    #[test]
    fn test_getitimer_fresh_returns_zeros() {
        reset_timers();
        let mut val = Itimerval {
            it_interval: Timeval { tv_sec: 99, tv_usec: 99 },
            it_value: Timeval { tv_sec: 99, tv_usec: 99 },
        };
        assert_eq!(getitimer(ITIMER_REAL, &raw mut val), 0);
        assert_eq!(val.it_interval.tv_sec, 0);
        assert_eq!(val.it_value.tv_sec, 0);
    }

    #[test]
    fn test_getitimer_per_timer_type_isolation() {
        reset_timers();
        // Set ITIMER_REAL, verify ITIMER_VIRTUAL is still zeros.
        let val = Itimerval {
            it_interval: Timeval { tv_sec: 10, tv_usec: 0 },
            it_value: Timeval { tv_sec: 20, tv_usec: 0 },
        };
        setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());

        let mut out = Itimerval {
            it_interval: Timeval { tv_sec: 0, tv_usec: 0 },
            it_value: Timeval { tv_sec: 0, tv_usec: 0 },
        };
        assert_eq!(getitimer(ITIMER_VIRTUAL, &raw mut out), 0);
        assert_eq!(out.it_interval.tv_sec, 0);
        assert_eq!(out.it_value.tv_sec, 0);
    }

    #[test]
    fn test_getitimer_invalid_which() {
        let mut val = Itimerval {
            it_interval: Timeval { tv_sec: 0, tv_usec: 0 },
            it_value: Timeval { tv_sec: 0, tv_usec: 0 },
        };
        crate::errno::set_errno(0);
        assert_eq!(getitimer(-1, &raw mut val), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_getitimer_null_curr_value() {
        crate::errno::set_errno(0);
        assert_eq!(getitimer(ITIMER_REAL, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- Itimerval / Itimerspec constants --

    #[test]
    fn test_itimer_constants() {
        assert_eq!(ITIMER_REAL, 0);
        assert_eq!(ITIMER_VIRTUAL, 1);
        assert_eq!(ITIMER_PROF, 2);
    }
}

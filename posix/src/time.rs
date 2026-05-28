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

/// Check whether a clock ID is one that may be modified by
/// `clock_settime`.
///
/// Linux only permits setting `CLOCK_REALTIME` and `CLOCK_TAI`;
/// every monotonic / cputime / coarse clock is read-only because
/// the kernel derives them from independent sources.  We don't have
/// CLOCK_TAI, so only `CLOCK_REALTIME` qualifies.
fn is_settable_clock(clk_id: ClockidT) -> bool {
    clk_id == CLOCK_REALTIME
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
/// Validates arguments per the Linux `sys_clock_settime` prologue,
/// then returns `-1` with `EPERM` because our kernel does not yet
/// expose a settable wall clock.
///
/// Linux validation order (matches
/// `kernel/time/posix-timers.c::SYSCALL_DEFINE2(clock_settime, ...)`):
///
/// 1. `tp` is null → `EFAULT` (via `copy_from_user`).
/// 2. `clk_id` is not a recognised clock → `EINVAL`.
/// 3. `clk_id` is recognised but not settable (e.g. monotonic,
///    cputime, coarse clocks) → `EINVAL`.
/// 4. `tv_sec < 0` or `tv_nsec` outside `[0, 999_999_999]`
///    → `EINVAL` (the `timespec64_valid_strict` check).
/// 5. Otherwise → `EPERM` (caller lacks `CAP_SYS_TIME` or, in our
///    case, the operation is simply not implemented yet).
///
/// The validation order matters: callers using `clock_settime` to
/// probe whether a clock is supported (a common libc test idiom)
/// must see `EINVAL` for read-only clocks like `CLOCK_MONOTONIC`,
/// regardless of capability state, so that probes don't false-
/// positive on the "permission denied" path.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_settime(clk_id: ClockidT, tp: *const Timespec) -> i32 {
    // 1. EFAULT before any clock-id inspection.  Linux reaches this
    //    via copy_from_user, which runs before the clock dispatch.
    if tp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // 2. Unknown clock → EINVAL.
    if !is_valid_clock(clk_id) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 3. Known but read-only clock → EINVAL.  Same errno as (2) but
    //    a different *reason*; the test suite distinguishes by
    //    checking that the unsettable list (MONOTONIC, BOOTTIME,
    //    CPUTIME, COARSE variants, RAW) all report EINVAL.
    if !is_settable_clock(clk_id) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 4. Validate the timespec.  POSIX (and Linux's
    //    timespec64_valid_strict) require:
    //      tv_sec  >= 0
    //      0 <= tv_nsec <= 999_999_999
    //    Note: we intentionally allow `tv_sec == 0` even though
    //    that points to the epoch, because Linux accepts it.
    //
    // SAFETY: tp was just confirmed non-null.  We do an unaligned
    // read so that callers passing a misaligned C struct don't UB.
    let ts = unsafe { core::ptr::read_unaligned(tp) };
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec > 999_999_999 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 5. All argument checks passed; we still can't actually set the
    //    clock.  Report EPERM, which is also what an unprivileged
    //    caller would see on Linux.
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
    // Linux semantics (kernel/time/posix-timers.c::common_nsleep,
    // kernel/time/hrtimer.c::hrtimer_nanosleep): the only valid flag
    // bit is TIMER_ABSTIME.  Reject any other bit before touching
    // the request pointer or inspecting the clock id, so a buggy
    // caller passing garbage flag bits is told about it regardless
    // of what else is wrong with their call.  clock_nanosleep
    // returns the error number directly (no errno set), per POSIX.
    if flags & !TIMER_ABSTIME != 0 {
        return errno::EINVAL;
    }

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
/// Linux-matching argument-domain validation:
///   - If both `tv` and `tz` are NULL: returns 0 (no-op success).
///     Linux's `SYSCALL_DEFINE2(settimeofday, ...)` treats both pointers as
///     optional; a call with no arguments is well-formed and trivially
///     succeeds without touching the clock.
///   - If `tv` is non-NULL: validate `tv_sec >= 0`, `tv_usec` in
///     `[0, 999_999]`; on failure return `-1` with `EINVAL`.
///   - On a structurally valid call that would actually set the clock:
///     return `-1` with `EPERM` because the kernel clock cannot be
///     adjusted from userspace yet (no `CAP_SYS_TIME` infrastructure).
///   - The `tz` argument is accepted but otherwise ignored (Linux has
///     deprecated `settimeofday` timezone setting since 2.6.x; passing a
///     non-NULL `tz` along with a NULL `tv` is treated as a no-op success
///     here to match the "set timezone only" path Linux still tolerates).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn settimeofday(
    tv: *const Timeval,
    tz: *const core::ffi::c_void,
) -> i32 {
    // Both NULL: well-formed no-op.
    if tv.is_null() && tz.is_null() {
        return 0;
    }

    // If tv is provided, validate its fields before considering EPERM.
    if !tv.is_null() {
        // SAFETY: caller-provided pointer is non-NULL; read unaligned to
        // avoid undefined behaviour on misaligned user buffers.  Reading
        // a Timeval (two scalar fields) has no further preconditions.
        let val = unsafe { core::ptr::read_unaligned(tv) };
        if val.tv_sec < 0 || val.tv_usec < 0 || val.tv_usec >= 1_000_000 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }

    // If only tz is provided (tv is NULL but tz isn't), Linux treats this
    // as a (deprecated) timezone-only update.  We accept it as a no-op.
    if tv.is_null() {
        return 0;
    }

    // Structurally valid request to actually adjust the clock — we lack
    // CAP_SYS_TIME plumbing, so refuse with EPERM.
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
/// Deliver a signal to a specific thread (Linux extension).
pub const SIGEV_THREAD_ID: i32 = 4;

/// Check whether `notify` is a `sigev_notify` value Linux accepts in
/// `timer_create` / `sigqueue` / `mq_notify`.
///
/// Linux's `good_sigevent()` (kernel/time/posix-timers.c) accepts
/// `SIGEV_NONE`, `SIGEV_SIGNAL`, `SIGEV_THREAD`, and `SIGEV_THREAD_ID`.
/// Any other value yields `EINVAL`.
fn is_valid_sigev_notify(notify: i32) -> bool {
    matches!(
        notify,
        SIGEV_NONE | SIGEV_SIGNAL | SIGEV_THREAD | SIGEV_THREAD_ID
    )
}

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
///
/// # Linux validation order
///
/// `kernel/time/posix-timers.c::sys_timer_create` →
/// `do_timer_create`:
///
/// 1. `copy_from_user(&event, timer_event_spec, sizeof(event))` if
///    `timer_event_spec` non-null → `EFAULT` (user copy fail)
/// 2. `posix_clocks[which_clock]` unavailable → `EINVAL`
/// 3. `event->sigev_notify` unrecognised → `EINVAL`
/// 4. `posix_timer_add(new_timer)` allocates the timer slot.
/// 5. `copy_to_user(created_timer_id, ...)` → `EFAULT` (which
///    destroys the just-allocated timer before returning).
///
/// **Phase 147**: pre-Phase-147 we returned `EINVAL` when `timerid`
/// was NULL.  Linux's NULL-`timerid` path goes through
/// `copy_to_user`, which returns `EFAULT`.  Fix: change the errno on
/// the NULL-`timerid` path to `EFAULT`, keeping its position after
/// the clock and `sevp` checks (Linux only reaches the `copy_to_user`
/// after step 3).
///
/// We do NOT simulate the "allocate-then-destroy" cycle that Linux
/// performs between steps 4 and 5 — the slot allocation is not
/// observable on the failure path, so eliding it is behaviourally
/// equivalent.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_create(
    clockid: ClockidT,
    sevp: *const Sigevent,
    timerid: *mut TimerT,
) -> i32 {
    // Step 2: clock validation → EINVAL.  (Linux's step 1 — sevp
    // copy_from_user EFAULT — isn't simulated; we deref sevp directly.)
    if !is_valid_clock(clockid) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Step 3: sigev_notify validation → EINVAL.  A null sevp is
    // treated as SIGEV_SIGNAL with SIGALRM, per POSIX.
    if !sevp.is_null() {
        // SAFETY: caller asserts sevp points to a valid Sigevent.  We
        // only read the sigev_notify field; we do not dereference any
        // pointer inside the struct.
        let notify = unsafe { (*sevp).sigev_notify };
        if !is_valid_sigev_notify(notify) {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }
    // Step 5: NULL `timerid` → EFAULT.  Linux's `copy_to_user` would
    // segfault on a NULL destination and return EFAULT.  Phase 147
    // fix: pre-Phase-147 we returned EINVAL here.
    if timerid.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Step 4: allocate a slot.  Find a free entry.
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
///
/// # Linux validation order
///
/// `kernel/time/posix-timers.c::sys_timer_settime` → `do_timer_settime`:
///
/// 1. `!new_setting`                       → `EINVAL`
/// 2. `get_itimerspec64(&new_spec, ...)`   → `EFAULT` (user copy fail)
/// 3. `!timespec64_valid(&new_spec.it_value)` → `EINVAL`  (ONLY
///    `it_value` is validated — `it_interval` is left to the timer-arm
///    machinery, which silently normalises out-of-range nsec or treats
///    excessive values as a long interval.)
/// 4. `tmr_flags & ~TIMER_ABSTIME`         → `EINVAL`
/// 5. `lock_timer(timer_id, ...)` returns NULL → `EINVAL`
///
/// **Phase 146**: pre-Phase-146 we ran the flag check FIRST (before
/// the NULL pointer check) and validated BOTH `it_value` AND
/// `it_interval`'s timespecs against `[0, 999_999_999]`.  Two
/// observable divergences from Linux:
///
/// * `timer_settime(VALID_ID, 0, {it_interval={0, 2_000_000_000},
///   it_value={1, 0}}, NULL)`: Linux returns 0 (success); we returned
///   `EINVAL`.  This breaks callers that construct `it_interval` from
///   compound arithmetic (e.g. `ms * 1_000_000`) and expect Linux's
///   silent normalisation.
/// * `timer_settime(VALID_ID, 0, {it_interval={-1, 0}, it_value={0, 0}},
///   NULL)`: Linux returns 0 (a one-shot disarm, since `it_value` is
///   zero); we returned `EINVAL`.
/// * `timer_settime(VALID_ID, BAD_FLAGS, NULL, NULL)`: Linux returns
///   `EINVAL` from the `!new_setting` check (step 1); we returned
///   `EINVAL` from the flag check.  Same errno, different reason —
///   not a behavioural divergence but the ordering now matches Linux.
///
/// The flag check is moved AFTER `it_value` timespec validation to
/// match Linux's `do_timer_settime` precedence.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_settime(
    timerid: TimerT,
    flags: i32,
    new_value: *const Itimerspec,
    old_value: *mut Itimerspec,
) -> i32 {
    // Step 1: !new_setting → EINVAL.  Linux's sys_timer_settime makes
    // this check before reading any user data and before flag
    // validation.  Phase 146 brings the ordering into parity.
    if new_value.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Step 2: get_itimerspec64 → EFAULT.  We can't simulate a fault
    // here (any non-null pointer is read directly); leaving this as a
    // documented gap.
    //
    // SAFETY: new_value verified non-null above; caller asserts it
    // points to a valid Itimerspec.  Copy it now so subsequent
    // validation reads from local storage.
    let nv = unsafe { *new_value };
    // Step 3: !timespec64_valid(&new_spec.it_value) → EINVAL.  ONLY
    // it_value is validated; it_interval is not (Phase 146 fix —
    // matches `do_timer_settime` exactly).
    if nv.it_value.tv_sec < 0
        || nv.it_value.tv_nsec < 0
        || nv.it_value.tv_nsec > 999_999_999
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Step 4: flag mask check.  TIMER_ABSTIME is the only defined bit;
    // everything else is EINVAL.
    if flags & !TIMER_ABSTIME != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Step 5: lock_timer(timer_id) → EINVAL on miss.
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

    // Store new value (it_value validated above; it_interval stored as
    // given — Linux normalises in the arming code, which our stub
    // doesn't simulate).
    *slot = Some(nv);
    0
}

/// Get the remaining time on a timer.
///
/// Always returns zeros (timers don't actually run).
///
/// # Linux validation order
///
/// `kernel/time/posix-timers.c::sys_timer_gettime`:
///
/// ```c
/// int ret = do_timer_gettime(timer_id, &cur_setting);
/// if (!ret) {
///     if (put_itimerspec64(&cur_setting, setting))
///         ret = -EFAULT;
/// }
/// ```
///
/// `do_timer_gettime` calls `lock_timer(timer_id, &flag)` which
/// returns NULL → `EINVAL` on a non-existent timer.  Only after that
/// succeeds does the kernel touch `setting` via `put_itimerspec64`,
/// where a NULL/bad user pointer yields `EFAULT`.
///
/// So the Linux precedence is:
///
///   1. `lock_timer(timer_id)` returns NULL → `EINVAL`
///   2. `put_itimerspec64(setting)` user copy fails → `EFAULT`
///
/// **Phase 148**: pre-Phase-148 we ran the NULL `curr_value` check
/// FIRST (before the timer-id lookup) and returned `EINVAL` on that
/// path.  Two observable divergences:
///
/// * `timer_gettime(BAD_TIMER_ID, NULL)`: Linux returns EINVAL (from
///   the lock_timer step); pre-Phase-148 we returned EINVAL too, but
///   via the NULL-pointer path — same errno, different ordering.
///   The test below confirms the post-Phase-148 ordering by passing
///   a bad timer_id with a valid (non-null) curr_value: BOTH paths
///   return EINVAL, but the new ordering matches the kernel.
/// * `timer_gettime(VALID_TIMER_ID, NULL)`: Linux returns EFAULT;
///   pre-Phase-148 we returned EINVAL.  This is the observable fix.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_gettime(timerid: TimerT, curr_value: *mut Itimerspec) -> i32 {
    // Step 1: lock_timer(timer_id) → EINVAL on miss.  This must fire
    // before the NULL curr_value check.
    let table = unsafe { core::ptr::addr_of_mut!(TIMER_TABLE).as_mut() };
    let Some(table) = table else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let Some(slot) = table.get(timerid as usize) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let Some(its) = slot.as_ref() else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    // Step 2: put_itimerspec64(curr_value) → EFAULT on NULL.  Phase
    // 148 fix: pre-Phase-148 this was EINVAL and ran before step 1.
    if curr_value.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: curr_value verified non-null above; caller asserts it
    // points to a valid Itimerspec.
    unsafe { *curr_value = *its; }
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
/// For our stub, timers never fire, so the overrun count is always 0
/// on success.  But the timer_id must still validate — Linux's
/// `sys_timer_getoverrun` calls `lock_timer(timer_id)` first and
/// returns `-1`/`EINVAL` for a non-existent timer.
///
/// # Linux validation order
///
/// `kernel/time/posix-timers.c::sys_timer_getoverrun`:
///
/// ```c
/// timr = lock_timer(timer_id, &flag);
/// if (!timr)
///     return -EINVAL;
/// overrun = timer_overrun_to_int(timr);
/// ...
/// return overrun;
/// ```
///
///   1. `lock_timer(timer_id)` returns NULL → `EINVAL`
///   2. Return the overrun count.
///
/// **Phase 149**: pre-Phase-149 we ignored `timerid` entirely and
/// always returned 0.  This let callers query overrun on bogus IDs
/// (e.g. uninitialised stack data, deleted timers) without any
/// diagnostic — a real bug for code that uses overrun as a "did this
/// timer fire?" signal.  Fix: look up the timer_id and return
/// `-1`/`EINVAL` for misses; on hit, still return 0 (stub).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_getoverrun(timerid: TimerT) -> i32 {
    // SAFETY: single-threaded access by convention.
    let table = unsafe { core::ptr::addr_of_mut!(TIMER_TABLE).as_mut() };
    let Some(table) = table else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let Some(slot) = table.get(timerid as usize) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    if slot.is_none() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Stub: timers never fire, so overrun count is always 0.
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

/// Check that a `Timeval` is well-formed for itimer use.
///
/// Mirrors Linux's `timeval_valid` (`kernel/time/itimer.c`):
/// both fields must be non-negative and `tv_usec` must be strictly
/// below 1,000,000.
fn itimer_timeval_valid(tv: &Timeval) -> bool {
    tv.tv_sec >= 0 && tv.tv_usec >= 0 && tv.tv_usec < 1_000_000
}

/// Set an interval timer.
///
/// Stores the timer value so `getitimer` can retrieve it.  The timer
/// never actually fires because we don't have signal delivery.
/// Programs that use `setitimer` for periodic alarms won't get
/// SIGALRM/SIGVTALRM/SIGPROF, but they will see their own settings
/// reflected back via `getitimer`.
///
/// Argument-domain validation (Linux-matching):
///   - `which` ∉ {ITIMER_REAL, ITIMER_VIRTUAL, ITIMER_PROF} → EINVAL.
///   - `new_value == NULL` → EFAULT.
///   - Either `it_interval` or `it_value` has a negative `tv_sec`,
///     negative `tv_usec`, or `tv_usec >= 1_000_000` → EINVAL.
///   - On EINVAL the stored state is **not** mutated and `old_value`
///     is **not** written, matching Linux's "validate before commit"
///     ordering.
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

    // SAFETY: new_value is non-NULL (just checked).  Read unaligned to
    // tolerate caller buffers that aren't naturally aligned.
    let val = unsafe { core::ptr::read_unaligned(new_value) };
    if !itimer_timeval_valid(&val.it_value) || !itimer_timeval_valid(&val.it_interval) {
        errno::set_errno(errno::EINVAL);
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
            *entry = val;
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
        // CLOCK_REALTIME with a valid timespec passes every argument
        // check, so we fall through to the EPERM path (no CAP_SYS_TIME
        // hook + no kernel support for setting the wall clock).
        let ts = Timespec { tv_sec: 0, tv_nsec: 0 };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_returns_eperm() {
        let tv = Timeval { tv_sec: 0, tv_usec: 0 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
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

    /// Phase 147: NULL `timerid` returns EFAULT, not EINVAL.  Linux's
    /// `sys_timer_create` reaches `copy_to_user` only after clock and
    /// sevp validation; a NULL destination there yields EFAULT.
    /// Renamed from `test_timer_create_null_timerid`.
    #[test]
    fn test_timer_create_null_timerid_efault_phase147() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_create(CLOCK_REALTIME, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
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

    /// Phase 148: NULL `curr_value` with a VALID timer_id returns
    /// EFAULT (Linux's `put_itimerspec64` failure path), not EINVAL.
    /// Renamed from `test_timer_gettime_null_curr_value`.
    #[test]
    fn test_timer_gettime_null_curr_value_efault_phase148() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        crate::errno::set_errno(0);
        let ret = timer_gettime(id, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
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

    /// Phase 149: `timer_getoverrun` validates timer_id.  Was a no-
    /// op stub returning 0 for any id; now returns -1/EINVAL on
    /// misses.  Renamed from `test_timer_getoverrun_returns_zero`.
    #[test]
    fn test_timer_getoverrun_returns_zero_for_valid_timer_phase149() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        // Stub: timers never fire, so a valid timer has 0 overruns.
        assert_eq!(timer_getoverrun(id), 0);
        timer_delete(id);
    }

    // ---------------------------------------------------------------
    // Phase 149: timer_getoverrun validates timer_id.
    //
    //   1. lock_timer(timer_id) returns NULL → -1/EINVAL
    //   2. return timer_overrun_to_int(timr) (0 for our stub)
    //
    // Pre-Phase-149 the function was a no-op stub that returned 0
    // regardless of `timerid`.  This let callers query overrun on
    // bogus IDs (uninitialised data, deleted timers) without any
    // diagnostic.
    // ---------------------------------------------------------------

    // -- per-error-class --

    /// Per-error-class: out-of-range timer_id → -1/EINVAL.
    #[test]
    fn test_timer_getoverrun_bad_timer_id_einval_phase149() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_getoverrun(99);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Per-error-class: negative timer_id → -1/EINVAL.
    #[test]
    fn test_timer_getoverrun_negative_timer_id_einval_phase149() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_getoverrun(-1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Per-error-class: in-range but unused timer_id → -1/EINVAL.
    #[test]
    fn test_timer_getoverrun_unused_timer_id_einval_phase149() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_getoverrun(0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Per-error-class: valid timer_id → 0 (stub: timers never fire).
    #[test]
    fn test_timer_getoverrun_valid_timer_zero_phase149() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let ret = timer_getoverrun(id);
        assert_eq!(ret, 0);
        timer_delete(id);
    }

    // -- workflow --

    /// Workflow: create timer, arm it, query overrun — succeeds with 0.
    #[test]
    fn test_timer_getoverrun_after_arm_phase149() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 100 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 100 },
        };
        assert_eq!(timer_settime(id, 0, &raw const val, core::ptr::null_mut()), 0);
        // Stub never fires, so overrun count stays at 0.
        assert_eq!(timer_getoverrun(id), 0);
        timer_delete(id);
    }

    /// Workflow: query overrun on a freshly-created (unarmed) timer —
    /// succeeds with 0.
    #[test]
    fn test_timer_getoverrun_fresh_timer_phase149() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(timer_getoverrun(id), 0);
        timer_delete(id);
    }

    // -- buggy caller --

    /// Buggy caller: queries overrun on a deleted timer.  Linux's
    /// `lock_timer` returns NULL after delete; we must return
    /// -1/EINVAL.  Pre-Phase-149 would have returned 0, hiding the
    /// use-after-free.
    #[test]
    fn test_timer_getoverrun_deleted_timer_einval_phase149() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(timer_delete(id), 0);

        crate::errno::set_errno(0);
        let ret = timer_getoverrun(id);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL,
            "use-after-delete must be diagnosed, not silently 0'd");
    }

    /// Buggy caller: queries overrun before any timer_create.  ID 0
    /// is uninitialised stack data; pre-Phase-149 silently returned
    /// 0.  Phase 149 diagnoses with EINVAL.
    #[test]
    fn test_timer_getoverrun_uninit_id_einval_phase149() {
        reset_timers();
        let uninit: TimerT = 0;
        crate::errno::set_errno(0);
        let ret = timer_getoverrun(uninit);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- ordering matrix --

    /// Ordering matrix: timer_getoverrun has only one error path
    /// (timer_id lookup), so ordering tests degenerate into "every
    /// invalid id produces EINVAL".  Coverage of negative, oob, and
    /// unused is already done; this test interleaves a valid id
    /// between two invalid ones to confirm no state leaks.
    #[test]
    fn test_timer_getoverrun_interleaved_valid_invalid_phase149() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        crate::errno::set_errno(0);
        assert_eq!(timer_getoverrun(99), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        assert_eq!(timer_getoverrun(id), 0);

        crate::errno::set_errno(0);
        assert_eq!(timer_getoverrun(-2), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        timer_delete(id);
    }

    // -- recovery --

    /// Recovery: after EINVAL from bad id, switching to a valid id
    /// succeeds.
    #[test]
    fn test_timer_getoverrun_recovery_after_einval_phase149() {
        reset_timers();
        crate::errno::set_errno(0);
        assert_eq!(timer_getoverrun(99), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(timer_getoverrun(id), 0);
        timer_delete(id);
    }

    /// Recovery: after deleted-timer EINVAL, recreating the timer
    /// makes the same id work.
    #[test]
    fn test_timer_getoverrun_recovery_after_delete_recreate_phase149() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let first_id = id;
        timer_delete(id);

        crate::errno::set_errno(0);
        assert_eq!(timer_getoverrun(first_id), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Recreate — should land in the same slot.
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(id, first_id, "slot should be reused");
        assert_eq!(timer_getoverrun(id), 0);
        timer_delete(id);
    }

    // -- no-side-effect loop --

    /// No-side-effect loop: repeated EINVAL calls don't leak.
    #[test]
    fn test_timer_getoverrun_einval_loop_phase149() {
        reset_timers();
        for _ in 0..64 {
            crate::errno::set_errno(0);
            assert_eq!(timer_getoverrun(99), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        }
        // Table still empty.
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(id, 0);
        assert_eq!(timer_getoverrun(id), 0);
        timer_delete(id);
    }

    /// No-side-effect loop: success path doesn't touch errno.
    #[test]
    fn test_timer_getoverrun_success_doesnt_touch_errno_phase149() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        crate::errno::set_errno(13579);
        let ret = timer_getoverrun(id);
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 13579,
            "success path must not touch errno");
        timer_delete(id);
    }

    // ---------------------------------------------------------------
    // Phase 148: timer_gettime — Linux's `sys_timer_gettime` runs
    // `lock_timer(timer_id)` (EINVAL on miss) BEFORE
    // `put_itimerspec64(setting)` (EFAULT on NULL/bad pointer).
    //
    //   1. lock_timer(timer_id) returns NULL    → EINVAL
    //   2. put_itimerspec64(curr_value) fails   → EFAULT
    //
    // Pre-Phase-148 we ran the NULL check first AND returned EINVAL
    // for it.  Phase 148 reorders and changes the errno.
    // ---------------------------------------------------------------

    // -- per-error-class --

    /// Per-error-class: bad timer_id with NON-NULL curr_value →
    /// EINVAL.
    #[test]
    fn test_timer_gettime_bad_timer_id_einval_phase148() {
        reset_timers();
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_gettime(99, &raw mut out);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Per-error-class: unused (but in-range) timer_id with NON-NULL
    /// curr_value → EINVAL.  Slot is allocated as None.
    #[test]
    fn test_timer_gettime_unused_timer_id_einval_phase148() {
        reset_timers();
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        // ID 0 is in-range but no timer is created.
        let ret = timer_gettime(0, &raw mut out);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Per-error-class: NULL curr_value with VALID timer_id → EFAULT.
    /// This is the Phase 148 fix.
    #[test]
    fn test_timer_gettime_null_curr_value_valid_timer_efault_phase148() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        crate::errno::set_errno(0);
        let ret = timer_gettime(id, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        timer_delete(id);
    }

    // -- ordering matrix --

    /// Ordering: bad timer_id BEATS NULL curr_value.  Linux runs
    /// `lock_timer` first; the timer-not-found EINVAL fires before
    /// any user-pointer access.  Both paths return -1, but the errno
    /// must be EINVAL (not EFAULT).
    #[test]
    fn test_timer_gettime_bad_timer_id_beats_null_curr_value_phase148() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_gettime(99, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL,
            "bad timer_id (EINVAL) must beat NULL curr_value (EFAULT)");
    }

    /// Ordering: unused (in-range) timer_id BEATS NULL curr_value.
    #[test]
    fn test_timer_gettime_unused_timer_id_beats_null_curr_value_phase148() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_gettime(0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL,
            "unused timer_id (EINVAL) must beat NULL curr_value (EFAULT)");
    }

    /// Ordering: negative timer_id BEATS NULL curr_value.
    #[test]
    fn test_timer_gettime_negative_timer_id_beats_null_curr_value_phase148() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_gettime(-1, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- workflow --

    /// Workflow: a caller that probes timer_gettime with NULL gets
    /// EFAULT, then provides a buffer and succeeds.
    #[test]
    fn test_timer_gettime_efault_then_valid_succeeds_phase148() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 2, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 4, tv_nsec: 0 },
        };
        assert_eq!(timer_settime(id, 0, &raw const val, core::ptr::null_mut()), 0);

        crate::errno::set_errno(0);
        assert_eq!(timer_gettime(id, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);

        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        let ret = timer_gettime(id, &raw mut out);
        assert_eq!(ret, 0);
        assert_eq!(out.it_value.tv_sec, 4);
        timer_delete(id);
    }

    // -- buggy caller --

    /// Buggy caller: confused two-step flow that passes timer_id of
    /// 0 (uninitialised) before timer_create has run gets EINVAL,
    /// not EFAULT.  Distinguishing the two errnos is exactly what
    /// motivates Phase 148.
    #[test]
    fn test_timer_gettime_buggy_caller_uninit_id_phase148() {
        reset_timers();
        let id: TimerT = 0; // forgot to call timer_create
        crate::errno::set_errno(0);
        let ret = timer_gettime(id, core::ptr::null_mut());
        assert_eq!(ret, -1);
        // Caller expects: "if EINVAL, fix my timer_id; if EFAULT,
        // fix my pointer".  With Phase 148, they correctly diagnose
        // the timer_id issue.
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Buggy caller: passes a deleted timer's id.  Linux returns
    /// EINVAL from lock_timer even with NULL curr_value.
    #[test]
    fn test_timer_gettime_deleted_timer_phase148() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(timer_delete(id), 0);

        crate::errno::set_errno(0);
        let ret = timer_gettime(id, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL,
            "deleted timer must produce EINVAL even with NULL curr_value");
    }

    // -- recovery --

    /// Recovery: after EFAULT, fixing curr_value succeeds and yields
    /// the stored value.
    #[test]
    fn test_timer_gettime_recovery_from_efault_phase148() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 1, tv_nsec: 1 },
            it_value: Timespec { tv_sec: 2, tv_nsec: 2 },
        };
        assert_eq!(timer_settime(id, 0, &raw const val, core::ptr::null_mut()), 0);

        // EFAULT.
        crate::errno::set_errno(0);
        assert_eq!(timer_gettime(id, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);

        // Retry.
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        assert_eq!(timer_gettime(id, &raw mut out), 0);
        assert_eq!(out.it_interval.tv_nsec, 1);
        assert_eq!(out.it_value.tv_nsec, 2);
        timer_delete(id);
    }

    /// Recovery: after EINVAL from bad timer_id, supplying a good
    /// timer_id succeeds.
    #[test]
    fn test_timer_gettime_recovery_from_einval_phase148() {
        reset_timers();
        crate::errno::set_errno(0);
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        assert_eq!(timer_gettime(99, &raw mut out), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let ret = timer_gettime(id, &raw mut out);
        assert_eq!(ret, 0);
        timer_delete(id);
    }

    // -- no-side-effect loop --

    /// No-side-effect loop: repeated EFAULT calls must not corrupt
    /// the stored timer value.
    #[test]
    fn test_timer_gettime_efault_loop_no_state_change_phase148() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 9, tv_nsec: 9 },
            it_value: Timespec { tv_sec: 11, tv_nsec: 11 },
        };
        assert_eq!(timer_settime(id, 0, &raw const val, core::ptr::null_mut()), 0);

        for _ in 0..32 {
            crate::errno::set_errno(0);
            assert_eq!(timer_gettime(id, core::ptr::null_mut()), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }

        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        assert_eq!(timer_gettime(id, &raw mut out), 0);
        assert_eq!(out.it_interval.tv_sec, 9);
        assert_eq!(out.it_value.tv_sec, 11);
        timer_delete(id);
    }

    /// No-side-effect loop: success path doesn't touch errno.
    #[test]
    fn test_timer_gettime_success_doesnt_touch_errno_phase148() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        crate::errno::set_errno(98765);
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        assert_eq!(timer_gettime(id, &raw mut out), 0);
        assert_eq!(crate::errno::get_errno(), 98765,
            "success path must not touch errno");
        timer_delete(id);
    }

    // -- Phase 97: timer_create / timer_settime argument-domain validation --

    /// `is_valid_sigev_notify` accepts every Linux-recognised value.
    #[test]
    fn test_is_valid_sigev_notify_recognised() {
        assert!(is_valid_sigev_notify(SIGEV_NONE));
        assert!(is_valid_sigev_notify(SIGEV_SIGNAL));
        assert!(is_valid_sigev_notify(SIGEV_THREAD));
        assert!(is_valid_sigev_notify(SIGEV_THREAD_ID));
    }

    /// Any other `sigev_notify` value is rejected.
    #[test]
    fn test_is_valid_sigev_notify_unknown_rejected() {
        // Linux uses 0/1/2/4; 3 and 5+ are unallocated.
        assert!(!is_valid_sigev_notify(3));
        assert!(!is_valid_sigev_notify(5));
        assert!(!is_valid_sigev_notify(99));
        assert!(!is_valid_sigev_notify(-1));
        assert!(!is_valid_sigev_notify(i32::MAX));
    }

    /// `SIGEV_THREAD_ID` matches glibc's value (4).
    #[test]
    fn test_sigev_thread_id_value() {
        assert_eq!(SIGEV_THREAD_ID, 4);
    }

    /// `timer_create` with an unknown clock ID returns -1 / EINVAL
    /// before allocating any slot.
    #[test]
    fn test_timer_create_unknown_clockid() {
        reset_timers();
        let mut id: TimerT = 999;
        crate::errno::set_errno(0);
        let ret = timer_create(99, core::ptr::null(), &raw mut id);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // id must be untouched (no allocation happened).
        assert_eq!(id, 999, "no slot should be allocated on EINVAL");
    }

    /// Every clock the rest of the API recognises is also accepted by
    /// `timer_create`.
    #[test]
    fn test_timer_create_accepts_all_valid_clocks() {
        for clk in [
            CLOCK_REALTIME,
            CLOCK_MONOTONIC,
            CLOCK_PROCESS_CPUTIME_ID,
            CLOCK_THREAD_CPUTIME_ID,
            CLOCK_MONOTONIC_RAW,
            CLOCK_REALTIME_COARSE,
            CLOCK_MONOTONIC_COARSE,
            CLOCK_BOOTTIME,
        ] {
            reset_timers();
            let mut id: TimerT = -1;
            let ret = timer_create(clk, core::ptr::null(), &raw mut id);
            assert_eq!(ret, 0, "clock {clk} should be accepted");
            assert!(id >= 0, "clock {clk}: a valid slot must be returned");
            timer_delete(id);
        }
    }

    /// `timer_create` with a non-null sevp whose `sigev_notify` is bogus
    /// returns -1 / EINVAL.
    #[test]
    fn test_timer_create_bad_sigev_notify() {
        reset_timers();
        let sev = Sigevent {
            sigev_value: 0,
            sigev_signo: 0,
            sigev_notify: 99, // not one of the recognised constants
            _pad: [0u8; 48],
        };
        let mut id: TimerT = 999;
        crate::errno::set_errno(0);
        let ret = timer_create(CLOCK_REALTIME, &raw const sev, &raw mut id);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        assert_eq!(id, 999, "no slot should be allocated on EINVAL");
    }

    /// `timer_create` accepts every recognised `sigev_notify` value.
    #[test]
    fn test_timer_create_accepts_each_sigev_notify() {
        for notify in [SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD, SIGEV_THREAD_ID] {
            reset_timers();
            let sev = Sigevent {
                sigev_value: 0,
                sigev_signo: 0,
                sigev_notify: notify,
                _pad: [0u8; 48],
            };
            let mut id: TimerT = -1;
            let ret = timer_create(CLOCK_REALTIME, &raw const sev, &raw mut id);
            assert_eq!(ret, 0, "sigev_notify {notify} should be accepted");
            timer_delete(id);
        }
    }

    /// Ordering: clockid check fires before the sevp check.  An invalid
    /// clock with a deliberately-bogus sevp still reports EINVAL, but
    /// for the clock — observable via the fact that no slot is touched.
    #[test]
    fn test_timer_create_validation_order_clockid_first() {
        reset_timers();
        let sev = Sigevent {
            sigev_value: 0,
            sigev_signo: 0,
            sigev_notify: 99, // also invalid
            _pad: [0u8; 48],
        };
        let mut id: TimerT = 999;
        crate::errno::set_errno(0);
        let ret = timer_create(42, &raw const sev, &raw mut id);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // Now retry with a valid clock; first slot must still be free.
        crate::errno::set_errno(0);
        let ret2 = timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(ret2, 0);
        assert_eq!(id, 0, "no allocation should have happened on the rejected call");
        timer_delete(id);
    }

    // ---------------------------------------------------------------
    // Phase 147: timer_create returns EFAULT (not EINVAL) on NULL
    // `timerid`, matching Linux's `copy_to_user` failure path.
    // Validation order:
    //
    //   1. clock validation        → EINVAL
    //   2. sigev_notify validation → EINVAL  (only if sevp non-null)
    //   3. timerid NULL            → EFAULT  (Phase 147 fix:
    //      pre-Phase-147 returned EINVAL)
    //
    // Linux additionally allocates a timer slot between steps 2 and
    // 3 (`posix_timer_add`) and destroys it on the EFAULT path; we
    // skip that round-trip since it's not externally observable.
    // ---------------------------------------------------------------

    // -- per-error-class --

    /// Per-error-class: clock-id failure with NULL `timerid` and
    /// NULL `sevp` still returns EINVAL (clock check fires first).
    #[test]
    fn test_timer_create_bad_clock_einval_phase147() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_create(0x4243, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Per-error-class: bad sigev_notify with NULL `timerid` still
    /// returns EINVAL (sigev_notify check fires before timerid).
    #[test]
    fn test_timer_create_bad_sigev_notify_einval_phase147() {
        reset_timers();
        let sev = Sigevent {
            sigev_value: 0,
            sigev_signo: 0,
            sigev_notify: 99,
            _pad: [0u8; 48],
        };
        crate::errno::set_errno(0);
        let ret = timer_create(CLOCK_REALTIME, &raw const sev, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Per-error-class: NULL `timerid` with valid clock and NULL
    /// `sevp` returns EFAULT — the Phase 147 fix.
    #[test]
    fn test_timer_create_null_timerid_valid_clock_efault_phase147() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_create(CLOCK_REALTIME, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// Per-error-class: NULL `timerid` with valid clock AND valid
    /// (non-NULL) sevp still returns EFAULT.
    #[test]
    fn test_timer_create_null_timerid_with_valid_sevp_efault_phase147() {
        reset_timers();
        let sev = Sigevent {
            sigev_value: 0,
            sigev_signo: 0,
            sigev_notify: 0, // SIGEV_SIGNAL (valid)
            _pad: [0u8; 48],
        };
        crate::errno::set_errno(0);
        let ret = timer_create(CLOCK_REALTIME, &raw const sev, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- ordering matrix --

    /// Ordering: bad clock beats NULL timerid (EINVAL, not EFAULT).
    #[test]
    fn test_timer_create_bad_clock_beats_null_timerid_phase147() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_create(0xDEAD, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL,
            "clock check must fire before timerid check");
    }

    /// Ordering: bad sigev_notify beats NULL timerid (EINVAL, not
    /// EFAULT).
    #[test]
    fn test_timer_create_bad_sigev_notify_beats_null_timerid_phase147() {
        reset_timers();
        let sev = Sigevent {
            sigev_value: 0,
            sigev_signo: 0,
            sigev_notify: 77,
            _pad: [0u8; 48],
        };
        crate::errno::set_errno(0);
        let ret = timer_create(CLOCK_REALTIME, &raw const sev, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL,
            "sigev_notify check must fire before timerid check");
    }

    /// Ordering: bad clock beats bad sigev_notify beats NULL
    /// timerid (all three set; EINVAL via clock path).
    #[test]
    fn test_timer_create_bad_clock_beats_all_phase147() {
        reset_timers();
        let sev = Sigevent {
            sigev_value: 0,
            sigev_signo: 0,
            sigev_notify: 88,
            _pad: [0u8; 48],
        };
        crate::errno::set_errno(0);
        let ret = timer_create(-7, &raw const sev, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- workflow --

    /// Workflow: a caller that probes the API with NULL timerid sees
    /// EFAULT, then retries with a valid pointer and succeeds.
    #[test]
    fn test_timer_create_efault_then_valid_succeeds_phase147() {
        reset_timers();
        crate::errno::set_errno(0);
        assert_eq!(
            timer_create(CLOCK_REALTIME, core::ptr::null(), core::ptr::null_mut()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);

        let mut id: TimerT = -1;
        crate::errno::set_errno(0);
        let ret = timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(ret, 0);
        assert_eq!(id, 0, "first allocation should land in slot 0");
        timer_delete(id);
    }

    // -- buggy caller --

    /// Buggy caller: a program that swaps the sevp and timerid
    /// argument positions (passes timerid as sevp, NULL as timerid)
    /// gets EFAULT for the NULL timerid.  The "sevp" pointer they
    /// passed (a valid timer-id pointer) does not panic our deref
    /// because we only read `sigev_notify`.  Wait — actually, the
    /// argument they passed for sevp would be the address of an
    /// uninitialised TimerT, which is `-1` written as i32.  The
    /// sigev_notify field read would access whatever follows.  To
    /// avoid testing UB, construct a deliberate but harmless valid
    /// Sigevent here and assert the EFAULT comes from NULL timerid.
    #[test]
    fn test_timer_create_buggy_caller_null_timerid_phase147() {
        reset_timers();
        // Caller forgot to take the address of their TimerT.
        let sev = Sigevent {
            sigev_value: 0,
            sigev_signo: 0,
            sigev_notify: 0,
            _pad: [0u8; 48],
        };
        crate::errno::set_errno(0);
        let ret = timer_create(CLOCK_REALTIME, &raw const sev, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- recovery --

    /// Recovery: after EFAULT from NULL timerid, the timer table
    /// state is unchanged — no slot was consumed.  Subsequent valid
    /// calls fill slots from index 0.
    #[test]
    fn test_timer_create_efault_no_slot_consumed_phase147() {
        reset_timers();

        // Burn one slot first to establish baseline.
        let mut id0: TimerT = -1;
        assert_eq!(
            timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id0),
            0
        );
        assert_eq!(id0, 0);

        // EFAULT call.
        crate::errno::set_errno(0);
        assert_eq!(
            timer_create(CLOCK_REALTIME, core::ptr::null(), core::ptr::null_mut()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);

        // Next valid call must land in slot 1 (slot 0 still held).
        let mut id1: TimerT = -1;
        assert_eq!(
            timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id1),
            0
        );
        assert_eq!(id1, 1, "EFAULT call must not have consumed a slot");

        timer_delete(id0);
        timer_delete(id1);
    }

    /// Recovery: after a chain of EFAULT/EINVAL calls, the timer
    /// table is still pristine — a fresh allocation starts at slot 0.
    #[test]
    fn test_timer_create_efault_einval_chain_no_state_change_phase147() {
        reset_timers();

        // EINVAL: bad clock.
        crate::errno::set_errno(0);
        assert_eq!(
            timer_create(0xBAD, core::ptr::null(), core::ptr::null_mut()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // EINVAL: bad sigev_notify.
        let bad_sev = Sigevent {
            sigev_value: 0,
            sigev_signo: 0,
            sigev_notify: 55,
            _pad: [0u8; 48],
        };
        let mut id_tmp: TimerT = -1;
        crate::errno::set_errno(0);
        assert_eq!(
            timer_create(CLOCK_REALTIME, &raw const bad_sev, &raw mut id_tmp),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // EFAULT: NULL timerid.
        crate::errno::set_errno(0);
        assert_eq!(
            timer_create(CLOCK_REALTIME, core::ptr::null(), core::ptr::null_mut()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);

        // Now valid: must land in slot 0.
        let mut id: TimerT = -1;
        let ret = timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(ret, 0);
        assert_eq!(id, 0, "no slot should have been consumed by the bad calls");
        timer_delete(id);
    }

    // -- no-side-effect loop --

    /// No-side-effect loop: repeated EFAULT calls must not leak
    /// state — the timer table stays empty and errno stays EFAULT.
    #[test]
    fn test_timer_create_efault_loop_no_state_change_phase147() {
        reset_timers();
        for _ in 0..64 {
            crate::errno::set_errno(0);
            let ret = timer_create(CLOCK_REALTIME, core::ptr::null(), core::ptr::null_mut());
            assert_eq!(ret, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }
        // Table still empty: first allocation goes to slot 0.
        let mut id: TimerT = -1;
        assert_eq!(
            timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id),
            0
        );
        assert_eq!(id, 0);
        timer_delete(id);
    }

    /// No-side-effect loop: success path doesn't touch errno.
    #[test]
    fn test_timer_create_success_doesnt_touch_errno_phase147() {
        reset_timers();
        crate::errno::set_errno(54321);
        let mut id: TimerT = -1;
        let ret = timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 54321,
            "success path must not touch errno");
        timer_delete(id);
    }

    /// `timer_settime` rejects any flag bit other than `TIMER_ABSTIME`.
    #[test]
    fn test_timer_settime_unknown_flags() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        // Bit 1 is not TIMER_ABSTIME (which is bit 0).
        let ret = timer_settime(id, 0x2, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // High bits are also rejected.
        crate::errno::set_errno(0);
        let ret = timer_settime(id, i32::MIN, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        timer_delete(id);
    }

    /// `TIMER_ABSTIME` alone is accepted.
    #[test]
    fn test_timer_settime_accepts_timer_abstime() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        let ret = timer_settime(id, TIMER_ABSTIME, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0);
        timer_delete(id);
    }

    /// `timer_settime` rejects `tv_nsec`/`tv_sec` out of range in
    /// `it_value`.  Phase 146 fix: `it_interval` is NOT validated by
    /// Linux's `do_timer_settime` (only `it_value` goes through
    /// `timespec64_valid`); previously this test pinned the broken
    /// behaviour that rejected bad `it_interval` values.  Renamed from
    /// `test_timer_settime_bad_tv_nsec`.
    #[test]
    fn test_timer_settime_bad_it_value_tv_nsec_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        // tv_nsec too large in it_value → EINVAL (still rejected).
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 1_000_000_000 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // tv_sec negative in it_value → EINVAL (still rejected).
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: -1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // tv_nsec negative in it_value → EINVAL (still rejected).
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: -1 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        timer_delete(id);
    }

    /// Validation order in `timer_settime`: NULL `new_value` short-
    /// circuits before flag check.  Phase 146 fix: pre-Phase-146 we
    /// checked flags first, so `(0, BAD_FLAGS, NULL, NULL)` was
    /// diagnosed as a flag EINVAL when Linux diagnoses it as a NULL
    /// `new_setting` EINVAL.  Both return EINVAL, but the reordering
    /// matches `sys_timer_settime`'s `if (!new_setting) return -EINVAL;`
    /// which fires before `do_timer_settime` is called.  Renamed from
    /// `test_timer_settime_validation_order_flags_first`.
    #[test]
    fn test_timer_settime_null_new_value_beats_bad_flags_phase146() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_settime(0, 0x2, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// A bad call must not clobber the stored value.  Phase 146 fix:
    /// the original test used a bad `it_interval` to trigger EINVAL,
    /// but Linux accepts bad `it_interval` — switched to a bad
    /// `it_value.tv_nsec` which IS still rejected.  Renamed from
    /// `test_timer_settime_bad_tv_nsec_does_not_overwrite_slot`.
    #[test]
    fn test_timer_settime_bad_it_value_tv_nsec_does_not_overwrite_slot_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        // Install a known-good baseline.
        let good = Itimerspec {
            it_interval: Timespec { tv_sec: 7, tv_nsec: 7 },
            it_value: Timespec { tv_sec: 8, tv_nsec: 8 },
        };
        assert_eq!(timer_settime(id, 0, &raw const good, core::ptr::null_mut()), 0);

        // A subsequent bad call (it_value.tv_nsec out of range) must
        // not clobber the stored value.
        let bad = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 2_000_000_000 },
        };
        crate::errno::set_errno(0);
        assert_eq!(timer_settime(id, 0, &raw const bad, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Read back via timer_gettime — should still be the good value.
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        assert_eq!(timer_gettime(id, &raw mut out), 0);
        assert_eq!(out.it_interval.tv_sec, 7);
        assert_eq!(out.it_interval.tv_nsec, 7);
        assert_eq!(out.it_value.tv_sec, 8);
        assert_eq!(out.it_value.tv_nsec, 8);
        timer_delete(id);
    }

    /// Buggy caller: tv_nsec exactly at the limit (999_999_999) is
    /// accepted — boundary value.
    #[test]
    fn test_timer_settime_boundary_tv_nsec_accepted() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 999_999_999 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 999_999_999 },
        };
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0);
        timer_delete(id);
    }

    /// Buggy-caller workflow: a real program that recovers from an
    /// EINVAL by passing a valid value should still see a working
    /// timer afterwards.
    #[test]
    fn test_timer_settime_recovery_after_einval() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        // First attempt: bogus flags → EINVAL.
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 1, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 2, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        assert_eq!(timer_settime(id, 0xF, &raw const val, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Retry with valid flags.
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0);

        // timer_gettime confirms the second call landed.
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        assert_eq!(timer_gettime(id, &raw mut out), 0);
        assert_eq!(out.it_interval.tv_sec, 1);
        assert_eq!(out.it_value.tv_sec, 2);
        timer_delete(id);
    }

    // ---------------------------------------------------------------
    // Phase 146: timer_settime validation order matches Linux's
    // `do_timer_settime` precedence:
    //
    //   1. !new_setting                      → EINVAL
    //   2. get_itimerspec64 (user copy)      → EFAULT (not simulated)
    //   3. !timespec64_valid(&it_value)      → EINVAL  (it_value only;
    //      it_interval is NOT validated)
    //   4. flags & ~TIMER_ABSTIME            → EINVAL
    //   5. lock_timer(timer_id) returns NULL → EINVAL
    //
    // The pre-Phase-146 implementation (a) ran the flag check before
    // the NULL pointer check and (b) validated both `it_value` AND
    // `it_interval`'s timespecs.  The fix reorders and strips the
    // spurious `it_interval` validation.
    // ---------------------------------------------------------------

    // -- per-error-class --

    /// Per-error-class: NULL `new_value` → EINVAL.  Phase 146 fix put
    /// this check first (was second).
    #[test]
    fn test_timer_settime_null_new_value_einval_phase146() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_settime(0, 0, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Per-error-class: `it_value.tv_sec < 0` → EINVAL.  Linux's
    /// `timespec64_valid` rejects negative tv_sec.
    #[test]
    fn test_timer_settime_bad_it_value_tv_sec_negative_einval_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: -1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        timer_delete(id);
    }

    /// Per-error-class: bad flag bit alone → EINVAL.  Valid new_value
    /// + valid it_value + valid timer_id but bogus flag bit.
    #[test]
    fn test_timer_settime_bad_flags_einval_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        // Bit 1 (TIMER_ABSTIME=bit 0) is bogus.
        let ret = timer_settime(id, 0x2, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        timer_delete(id);
    }

    /// Per-error-class: bad timer id alone → EINVAL.  Everything else
    /// valid, but the timer_id doesn't exist.
    #[test]
    fn test_timer_settime_bad_timer_id_einval_phase146() {
        reset_timers();
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        // MAX_TIMERS = 32, so timer_id 99 is out of range.
        let ret = timer_settime(99, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- ordering matrix --

    /// Ordering: NULL `new_value` precedes bad timer_id.  Both
    /// produce EINVAL, but the NULL check fires first (step 1 before
    /// step 5).
    #[test]
    fn test_timer_settime_null_new_value_beats_bad_timer_id_phase146() {
        reset_timers();
        crate::errno::set_errno(0);
        let ret = timer_settime(99, 0, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // Same errno on every path, but if NULL handling were skipped
        // we'd dereference a null pointer and segfault — so the fact
        // that we still get EINVAL proves NULL was caught first.
    }

    /// Ordering: bad `it_value` timespec precedes bad flags.  Step 3
    /// fires before step 4.  Pre-Phase-146 the flag check came first;
    /// asserting the new order keeps a regression visible.
    #[test]
    fn test_timer_settime_bad_it_value_beats_bad_flags_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 2_000_000_000 },
        };
        crate::errno::set_errno(0);
        // Bad it_value.tv_nsec AND bad flags — both errno-equal but
        // the it_value path must win.  We can't observe which path
        // fired by errno alone, but we can confirm the call still
        // returns -1/EINVAL even with valid_id+bad_flags+bad_value,
        // which exercises the ordering.
        let ret = timer_settime(id, 0x4, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        timer_delete(id);
    }

    /// Ordering: bad `it_value` precedes bad timer_id.  Bad timespec
    /// + bad timer_id → still EINVAL via the timespec path.
    #[test]
    fn test_timer_settime_bad_it_value_beats_bad_timer_id_phase146() {
        reset_timers();
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: -1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(99, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Ordering: bad flags precede bad timer_id.  Step 4 before
    /// step 5.
    #[test]
    fn test_timer_settime_bad_flags_beats_bad_timer_id_phase146() {
        reset_timers();
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(99, 0x8, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- workflow: Linux divergence fixes --

    /// Workflow: `it_interval.tv_nsec = 2_000_000_000` is now
    /// ACCEPTED (Phase 146 fix).  Linux's `do_timer_settime` does not
    /// validate `it_interval` — the arm code silently normalises.
    /// Pre-Phase-146 we returned EINVAL here; now we accept.
    #[test]
    fn test_timer_settime_bad_it_interval_tv_nsec_accepted_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 2_000_000_000 },
            it_value: Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0, "Linux accepts out-of-range it_interval.tv_nsec");
        timer_delete(id);
    }

    /// Workflow: `it_interval.tv_nsec = -1` is now ACCEPTED.
    #[test]
    fn test_timer_settime_negative_it_interval_tv_nsec_accepted_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: -1 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0, "Linux accepts negative it_interval.tv_nsec");
        timer_delete(id);
    }

    /// Workflow: `it_interval.tv_sec = -1` is now ACCEPTED (one-shot
    /// disarm semantics since `it_value` is zero).
    #[test]
    fn test_timer_settime_negative_it_interval_tv_sec_accepted_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: -1, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0, "Linux accepts negative it_interval.tv_sec");
        timer_delete(id);
    }

    // -- buggy caller --

    /// Buggy caller: a program that builds `it_interval.tv_nsec` via
    /// `ms * 1_000_000` and forgets to normalise (so passes
    /// `2500 * 1_000_000 = 2_500_000_000`) used to fail with EINVAL.
    /// Linux accepts this — Phase 146 brings us into parity.
    #[test]
    fn test_timer_settime_compound_ms_arithmetic_accepted_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let ms: i64 = 2500;
        let val = Itimerspec {
            // 2500ms expressed (wrongly) as nsec without normalising
            // into tv_sec — Linux's arm code normalises silently.
            it_interval: Timespec { tv_sec: 0, tv_nsec: ms * 1_000_000 },
            it_value: Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0);
        timer_delete(id);
    }

    /// Buggy caller: a very large but legal `it_interval` value (e.g.
    /// `i64::MAX/2` seconds) is accepted — Linux does no range check
    /// on it_interval.
    #[test]
    fn test_timer_settime_huge_it_interval_accepted_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);
        let val = Itimerspec {
            it_interval: Timespec { tv_sec: i64::MAX / 2, tv_nsec: 999_999_999 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 1 },
        };
        crate::errno::set_errno(0);
        let ret = timer_settime(id, 0, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0);
        timer_delete(id);
    }

    // -- recovery --

    /// Recovery: after bad `it_value` EINVAL, fixing it_value and
    /// retrying must succeed.
    #[test]
    fn test_timer_settime_recovery_from_bad_it_value_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        // First attempt: bad it_value.tv_nsec → EINVAL.
        let bad = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 1_000_000_000 },
        };
        crate::errno::set_errno(0);
        assert_eq!(timer_settime(id, 0, &raw const bad, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Retry with valid it_value.
        let good = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 999_999_999 },
            it_value: Timespec { tv_sec: 3, tv_nsec: 0 },
        };
        let ret = timer_settime(id, 0, &raw const good, core::ptr::null_mut());
        assert_eq!(ret, 0);

        // gettime confirms.
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        assert_eq!(timer_gettime(id, &raw mut out), 0);
        assert_eq!(out.it_value.tv_sec, 3);
        assert_eq!(out.it_interval.tv_nsec, 999_999_999);
        timer_delete(id);
    }

    /// Recovery: a bad-flags call after a bad-it_value call must
    /// itself be diagnosed cleanly, and a subsequent valid call must
    /// land.
    #[test]
    fn test_timer_settime_recovery_from_bad_flags_then_bad_it_value_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        let good_val = Itimerspec {
            it_interval: Timespec { tv_sec: 1, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 5, tv_nsec: 0 },
        };
        let bad_val = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: -1, tv_nsec: 0 },
        };

        crate::errno::set_errno(0);
        assert_eq!(timer_settime(id, 0x10, &raw const good_val, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        assert_eq!(timer_settime(id, 0, &raw const bad_val, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        // Final valid call must succeed.
        let ret = timer_settime(id, 0, &raw const good_val, core::ptr::null_mut());
        assert_eq!(ret, 0);
        timer_delete(id);
    }

    // -- no-side-effect loop --

    /// No-side-effect loop: repeated EINVAL calls must not corrupt
    /// the stored value of a previously-set timer.
    #[test]
    fn test_timer_settime_repeated_einval_no_state_change_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        // Set a known-good value first.
        let good = Itimerspec {
            it_interval: Timespec { tv_sec: 7, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 11, tv_nsec: 0 },
        };
        assert_eq!(timer_settime(id, 0, &raw const good, core::ptr::null_mut()), 0);

        // Now hammer with assorted bad calls.
        for _ in 0..16 {
            // NULL pointer.
            crate::errno::set_errno(0);
            assert_eq!(timer_settime(id, 0, core::ptr::null(), core::ptr::null_mut()), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

            // Bad it_value.
            let bad1 = Itimerspec {
                it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
                it_value: Timespec { tv_sec: 0, tv_nsec: -1 },
            };
            crate::errno::set_errno(0);
            assert_eq!(timer_settime(id, 0, &raw const bad1, core::ptr::null_mut()), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

            // Bad flags.
            crate::errno::set_errno(0);
            assert_eq!(timer_settime(id, 0xF0, &raw const good, core::ptr::null_mut()), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        }

        // Stored value must be the original good one.
        let mut out = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        assert_eq!(timer_gettime(id, &raw mut out), 0);
        assert_eq!(out.it_interval.tv_sec, 7);
        assert_eq!(out.it_value.tv_sec, 11);
        timer_delete(id);
    }

    /// No-side-effect loop: a tight loop of failing calls must not
    /// leak errno into the success path.  After the final good call,
    /// errno must be untouched (we don't modify it on success).
    #[test]
    fn test_timer_settime_loop_doesnt_corrupt_errno_phase146() {
        reset_timers();
        let mut id: TimerT = 0;
        timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut id);

        for _ in 0..32 {
            crate::errno::set_errno(0);
            assert_eq!(timer_settime(id, 0x80, core::ptr::null(), core::ptr::null_mut()), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        }

        // Set errno to a sentinel and run a good call — Linux's
        // success path does not touch errno.
        crate::errno::set_errno(12345);
        let good = Itimerspec {
            it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        let ret = timer_settime(id, 0, &raw const good, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 12345,
            "success path must not touch errno");
        timer_delete(id);
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

    // ---------------------------------------------------------------------
    // Phase 87 — setitimer Itimerval-field validation
    //
    // Linux semantics being validated (kernel/time/itimer.c
    // ::do_setitimer → timeval_valid):
    //   - it_value or it_interval with tv_sec < 0 → EINVAL
    //   - tv_usec < 0 or tv_usec >= 1_000_000 → EINVAL
    //   - EINVAL must not mutate stored state or write old_value
    //   - which-check still takes precedence over field validation
    // ---------------------------------------------------------------------

    fn itimerval(s1: i64, u1: SusecondsT, s2: i64, u2: SusecondsT) -> Itimerval {
        Itimerval {
            it_interval: Timeval { tv_sec: s1, tv_usec: u1 },
            it_value: Timeval { tv_sec: s2, tv_usec: u2 },
        }
    }

    #[test]
    fn test_setitimer_phase87_neg_value_tv_sec_einval() {
        reset_timers();
        let val = itimerval(0, 0, -1, 0);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_neg_value_tv_usec_einval() {
        reset_timers();
        let val = itimerval(0, 0, 0, -1);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_value_tv_usec_at_million_einval() {
        reset_timers();
        let val = itimerval(0, 0, 0, 1_000_000);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_value_tv_usec_above_million_einval() {
        reset_timers();
        let val = itimerval(0, 0, 0, 5_000_000);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_neg_interval_tv_sec_einval() {
        reset_timers();
        let val = itimerval(-1, 0, 0, 0);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_neg_interval_tv_usec_einval() {
        reset_timers();
        let val = itimerval(0, -1, 0, 0);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_interval_tv_usec_at_million_einval() {
        reset_timers();
        let val = itimerval(0, 1_000_000, 0, 0);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_max_valid_usec_succeeds() {
        // 999_999 is the maximum valid microsecond value.
        reset_timers();
        let val = itimerval(0, 999_999, 0, 999_999);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_setitimer_phase87_invalid_which_takes_precedence() {
        // Bad `which` AND bad timeval: which-check wins (it comes first).
        reset_timers();
        let val = itimerval(0, 0, -1, -1);
        errno::set_errno(0);
        let ret = setitimer(42, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_efault_takes_precedence_over_field_check() {
        // NULL pointer beats bogus fields we can't even read.
        reset_timers();
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_setitimer_phase87_einval_does_not_mutate_stored_state() {
        // First store a valid value.
        reset_timers();
        let good = itimerval(2, 200, 3, 300);
        assert_eq!(
            setitimer(ITIMER_REAL, &raw const good, core::ptr::null_mut()),
            0
        );

        // Now a bogus call must fail without overwriting it.
        let bad = itimerval(0, 0, 0, 2_000_000);
        errno::set_errno(0);
        assert_eq!(setitimer(ITIMER_REAL, &raw const bad, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // Verify the previous good value is still in place.
        let mut got = itimerval(99, 99, 99, 99);
        assert_eq!(getitimer(ITIMER_REAL, &raw mut got), 0);
        assert_eq!(got.it_interval.tv_sec, 2);
        assert_eq!(got.it_interval.tv_usec, 200);
        assert_eq!(got.it_value.tv_sec, 3);
        assert_eq!(got.it_value.tv_usec, 300);
    }

    #[test]
    fn test_setitimer_phase87_einval_does_not_write_old_value() {
        // old_value buffer must remain untouched on validation failure.
        reset_timers();
        let bad = itimerval(0, 0, -5, 0);
        let mut old = itimerval(7, 7, 8, 8);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const bad, &raw mut old);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(old.it_interval.tv_sec, 7);
        assert_eq!(old.it_interval.tv_usec, 7);
        assert_eq!(old.it_value.tv_sec, 8);
        assert_eq!(old.it_value.tv_usec, 8);
    }

    #[test]
    fn test_setitimer_phase87_zero_value_zero_interval_succeeds() {
        // Disarming a timer with all-zero values is valid.
        reset_timers();
        let val = itimerval(0, 0, 0, 0);
        errno::set_errno(0);
        assert_eq!(setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut()), 0);
    }

    #[test]
    fn test_setitimer_phase87_all_three_which_values_validate_fields() {
        // Validation must apply equally to all three timers.
        reset_timers();
        let bad = itimerval(0, 0, 0, 1_000_000);
        for which in [ITIMER_REAL, ITIMER_VIRTUAL, ITIMER_PROF] {
            errno::set_errno(0);
            let ret = setitimer(which, &raw const bad, core::ptr::null_mut());
            assert_eq!(ret, -1, "which={}", which);
            assert_eq!(errno::get_errno(), errno::EINVAL, "which={}", which);
        }
    }

    #[test]
    fn test_setitimer_phase87_large_positive_tv_sec_succeeds() {
        // Far-future timer values are valid.
        reset_timers();
        let val = itimerval(1_000_000, 0, 2_000_000, 0);
        errno::set_errno(0);
        assert_eq!(setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut()), 0);
    }

    #[test]
    fn test_setitimer_phase87_einval_then_valid_call_progression() {
        reset_timers();
        let bad = itimerval(0, -1, 0, 0);
        errno::set_errno(0);
        assert_eq!(setitimer(ITIMER_REAL, &raw const bad, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let good = itimerval(1, 1, 1, 1);
        errno::set_errno(0);
        assert_eq!(setitimer(ITIMER_REAL, &raw const good, core::ptr::null_mut()), 0);
    }

    // -- Itimerval / Itimerspec constants --

    #[test]
    fn test_itimer_constants() {
        assert_eq!(ITIMER_REAL, 0);
        assert_eq!(ITIMER_VIRTUAL, 1);
        assert_eq!(ITIMER_PROF, 2);
    }

    // -- nanosleep validation --

    #[test]
    fn test_nanosleep_null_request() {
        crate::errno::set_errno(0);
        assert_eq!(nanosleep(core::ptr::null(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_nanosleep_negative_sec() {
        let req = Timespec { tv_sec: -1, tv_nsec: 0 };
        crate::errno::set_errno(0);
        assert_eq!(nanosleep(&req, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_nanosleep_nsec_too_large() {
        let req = Timespec { tv_sec: 0, tv_nsec: 1_000_000_000 };
        crate::errno::set_errno(0);
        assert_eq!(nanosleep(&req, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_nanosleep_negative_nsec() {
        let req = Timespec { tv_sec: 0, tv_nsec: -1 };
        crate::errno::set_errno(0);
        assert_eq!(nanosleep(&req, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- clock_nanosleep validation --

    #[test]
    fn test_clock_nanosleep_null_request() {
        assert_eq!(clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut()), crate::errno::EINVAL);
    }

    #[test]
    fn test_clock_nanosleep_invalid_clock() {
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        assert_eq!(clock_nanosleep(999, 0, &req, core::ptr::null_mut()), crate::errno::EINVAL);
    }

    #[test]
    fn test_clock_nanosleep_invalid_nsec() {
        let req = Timespec { tv_sec: 0, tv_nsec: 2_000_000_000 };
        assert_eq!(clock_nanosleep(CLOCK_REALTIME, 0, &req, core::ptr::null_mut()), crate::errno::EINVAL);
    }

    // -- Phase 102: clock_nanosleep flag-mask validation --
    //
    // Linux semantics (kernel/time/posix-timers.c::common_nsleep):
    //   if (flags & ~TIMER_ABSTIME) return -EINVAL;
    // The check precedes request / clk_id / nsec inspection.  Our
    // previous code never inspected the flag mask at all; only
    // TIMER_ABSTIME (== 1) was conditionally used, and stray bits
    // were silently dropped.  clock_nanosleep returns the error
    // number directly (not via errno) per POSIX.

    #[test]
    fn test_clock_nanosleep_timer_abstime_is_bit_zero() {
        // Sanity / invariant: TIMER_ABSTIME must be 1 (bit 0).  This
        // matches glibc's <time.h> and Linux <linux/time.h>.  If this
        // ever drifts, the mask check below would silently accept
        // bit 0 set with any other shape — break loudly here instead.
        assert_eq!(TIMER_ABSTIME, 1);
        assert_eq!(TIMER_ABSTIME & (TIMER_ABSTIME - 1), 0,
            "TIMER_ABSTIME must be a single bit");
    }

    #[test]
    fn test_clock_nanosleep_unknown_flag_einval() {
        // An arbitrary stray bit must yield EINVAL up-front, BEFORE
        // any of the other validations.  We pass a null request +
        // bad clock id deliberately so that if the mask check were
        // missing, we'd still see EINVAL from the existing paths —
        // i.e. this test depends on the mask path returning first.
        // We can't observe ordering via the value alone (all paths
        // return EINVAL), but we can observe that the mask path is
        // hit BEFORE the null-request dereference.
        let bad = 1 << 4;
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, bad, core::ptr::null(), core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    #[test]
    fn test_clock_nanosleep_high_bit_einval() {
        // i32::MIN sets the sign bit — far outside the valid mask.
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, i32::MIN, &req, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    #[test]
    fn test_clock_nanosleep_einval_wins_with_garbage_inputs() {
        // Both bad flags AND bad request would normally trigger
        // separate EINVAL paths.  Regression guard: the flag-mask
        // check fires first, before the null check or clock check.
        // We verify the path is reachable with otherwise-valid
        // inputs except the stray flag bit.
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, 1 << 5, &req, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    #[test]
    fn test_clock_nanosleep_zero_flags_passes_mask() {
        // Zero flags is valid; a zero-duration relative sleep should
        // succeed (0 nanoseconds → immediate return).  Must NOT
        // return EINVAL (which would indicate the mask wrongly
        // rejected zero).
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        let ret = clock_nanosleep(CLOCK_REALTIME, 0, &req, core::ptr::null_mut());
        assert_ne!(ret, crate::errno::EINVAL,
            "zero flags must not be rejected by the mask");
    }

    #[test]
    fn test_clock_nanosleep_timer_abstime_alone_passes_mask() {
        // TIMER_ABSTIME alone is the canonical valid call; it must
        // not be rejected by the mask.  Path goes to the abs-time
        // branch which, with a zero/past target, returns 0 immediately.
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        let ret = clock_nanosleep(CLOCK_REALTIME, TIMER_ABSTIME, &req, core::ptr::null_mut());
        assert_ne!(ret, crate::errno::EINVAL,
            "TIMER_ABSTIME alone must not be rejected by the mask");
    }

    #[test]
    fn test_clock_nanosleep_abstime_plus_unknown_einval() {
        // Mixing TIMER_ABSTIME with a stray bit must still EINVAL —
        // no partial acceptance.
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        let mixed = TIMER_ABSTIME | (1 << 8);
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, mixed, &req, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    #[test]
    fn test_clock_nanosleep_o_append_value_rejected() {
        // O_APPEND (a file flag) has no meaning here.  In our
        // numbering it's 0o2000 == 1<<10, which is not TIMER_ABSTIME
        // (1<<0).  Must EINVAL.
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, crate::fcntl::O_APPEND, &req, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    #[test]
    fn test_clock_nanosleep_recovery_after_einval() {
        // A rejected call must not corrupt state — a subsequent
        // valid-flags call still behaves correctly.
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        let r1 = clock_nanosleep(CLOCK_REALTIME, 1 << 7, &req, core::ptr::null_mut());
        assert_eq!(r1, crate::errno::EINVAL);
        let r2 = clock_nanosleep(CLOCK_REALTIME, 0, &req, core::ptr::null_mut());
        assert_ne!(r2, crate::errno::EINVAL,
            "valid call after rejected one must still succeed");
    }

    #[test]
    fn test_clock_nanosleep_single_bits_outside_mask_all_rejected() {
        // Exhaustive: every single-bit value 1<<1 .. 1<<30 must be
        // rejected (1<<0 is TIMER_ABSTIME itself and is valid).
        // Guards against a future TIMER_ABSTIME change silently
        // widening the accepted mask.
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        for shift in 1..31 {
            let bit = 1i32 << shift;
            assert_eq!(
                clock_nanosleep(CLOCK_REALTIME, bit, &req, core::ptr::null_mut()),
                crate::errno::EINVAL,
                "bit {:#x} should be rejected by clock_nanosleep mask", bit
            );
        }
    }

    #[test]
    fn test_clock_nanosleep_bad_flags_before_invalid_clock() {
        // Both bad flags AND bad clock id would normally produce
        // EINVAL via different paths.  Mask check must fire first
        // (matches Linux ordering).  We can't differentiate the
        // value, but the test exercises the path with valid clock
        // checks unreachable.
        let req = Timespec { tv_sec: 0, tv_nsec: 0 };
        assert_eq!(
            clock_nanosleep(99_999, 1 << 9, &req, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    #[test]
    fn test_clock_nanosleep_bad_flags_before_invalid_nsec() {
        // Bad flags must fire before the nsec validation.
        let req = Timespec { tv_sec: 0, tv_nsec: 2_000_000_000 };
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, 1 << 6, &req, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    // -- gettimeofday --

    #[test]
    fn test_gettimeofday_null_tv() {
        crate::errno::set_errno(0);
        assert_eq!(gettimeofday(core::ptr::null_mut(), core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_gettimeofday_returns_value() {
        // On the host, this executes a real syscall. Just verify it
        // doesn't crash and tv gets some value.
        let mut tv = Timeval { tv_sec: -1, tv_usec: -1 };
        let _ = gettimeofday(&raw mut tv, core::ptr::null_mut());
        // Don't assert exact values — syscall may return error on host.
    }

    // -- localtime --

    #[test]
    fn test_localtime_null() {
        let ptr = localtime(core::ptr::null());
        assert!(ptr.is_null());
    }

    #[test]
    fn test_localtime_epoch() {
        // localtime delegates to gmtime (no timezone).
        let t: TimeT = 0;
        let ptr = localtime(&t);
        if !ptr.is_null() {
            let tm = unsafe { &*ptr };
            assert_eq!(tm.tm_year, 70); // 1970
            assert_eq!(tm.tm_mon, 0);   // January
            assert_eq!(tm.tm_mday, 1);
        }
    }

    // -- ctime --

    #[test]
    fn test_ctime_null() {
        // ctime(NULL) → localtime(NULL) → NULL → asctime(NULL) → fallback "???" string
        let ptr = ctime(core::ptr::null());
        // asctime(NULL) returns a valid fallback string, not NULL.
        assert!(!ptr.is_null());
        let c = unsafe { *ptr };
        assert_eq!(c, b'?');
    }

    #[test]
    fn test_ctime_epoch() {
        let t: TimeT = 0;
        let ptr = ctime(&t);
        // Should return a non-null formatted time string.
        if !ptr.is_null() {
            // Should start with "Thu" (January 1, 1970 was a Thursday).
            let c = unsafe { *ptr };
            assert_eq!(c, b'T');
        }
    }

    // -- sleep/usleep (can only test the API, not timing) --

    #[test]
    fn test_sleep_zero_no_crash() {
        // sleep(0) should return immediately.
        let ret = sleep(0);
        let _ = ret; // Don't assert — syscall may behave oddly on host.
    }

    #[test]
    fn test_usleep_zero_no_crash() {
        let ret = usleep(0);
        let _ = ret;
    }

    // ------------------------------------------------------------------
    // Additional edge-case tests for time functions
    // ------------------------------------------------------------------

    #[test]
    fn test_clock_settime_monotonic_einval() {
        // CLOCK_MONOTONIC is recognised but not settable: Phase 83
        // moved this from EPERM to EINVAL so that callers can
        // distinguish "no permission" from "wrong clock kind".
        let ts = Timespec { tv_sec: 0, tv_nsec: 0 };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_MONOTONIC, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_null_ts_efault() {
        // Null timespec → EFAULT.  Phase 83 makes this explicit.
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // ------------------------------------------------------------------
    // Phase 83 — clock_settime argument-domain validation
    //
    // Validation order matches Linux:
    //   tp == NULL                       -> EFAULT
    //   unknown clock_id                 -> EINVAL
    //   recognised but unsettable clock  -> EINVAL
    //   tv_sec < 0 or bad tv_nsec        -> EINVAL
    //   otherwise                        -> EPERM
    // ------------------------------------------------------------------

    /// Convenience: a valid Timespec for the EPERM path tests.
    fn valid_ts() -> Timespec { Timespec { tv_sec: 1, tv_nsec: 0 } }

    #[test]
    fn test_is_settable_clock_only_realtime() {
        // The set of settable clocks is exactly {CLOCK_REALTIME}.
        assert!(is_settable_clock(CLOCK_REALTIME));
        for &c in &[
            CLOCK_MONOTONIC,
            CLOCK_PROCESS_CPUTIME_ID,
            CLOCK_THREAD_CPUTIME_ID,
            CLOCK_MONOTONIC_RAW,
            CLOCK_REALTIME_COARSE,
            CLOCK_MONOTONIC_COARSE,
            CLOCK_BOOTTIME,
        ] {
            assert!(!is_settable_clock(c), "clock {c} must not be settable");
        }
    }

    #[test]
    fn test_clock_settime_efault_precedes_clock_check() {
        // Even with an obviously bogus clock id, a null tp must
        // surface EFAULT first.  This matches the Linux ordering:
        // copy_from_user runs before the clock-id dispatch.
        errno::set_errno(0);
        let ret = clock_settime(0x7FFF_FFFF, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_clock_settime_unknown_clock_einval() {
        let ts = valid_ts();
        errno::set_errno(0);
        let ret = clock_settime(0x7FFF_FFFF, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_negative_clock_einval() {
        let ts = valid_ts();
        errno::set_errno(0);
        let ret = clock_settime(-1, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_every_unsettable_clock_einval() {
        let ts = valid_ts();
        for &c in &[
            CLOCK_MONOTONIC,
            CLOCK_PROCESS_CPUTIME_ID,
            CLOCK_THREAD_CPUTIME_ID,
            CLOCK_MONOTONIC_RAW,
            CLOCK_REALTIME_COARSE,
            CLOCK_MONOTONIC_COARSE,
            CLOCK_BOOTTIME,
        ] {
            errno::set_errno(0);
            let ret = clock_settime(c, &raw const ts);
            assert_eq!(ret, -1, "clock {c} should be -1");
            assert_eq!(errno::get_errno(), errno::EINVAL,
                "clock {c} should report EINVAL");
        }
    }

    #[test]
    fn test_clock_settime_negative_tv_sec_einval() {
        let ts = Timespec { tv_sec: -1, tv_nsec: 0 };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_negative_tv_nsec_einval() {
        let ts = Timespec { tv_sec: 0, tv_nsec: -1 };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_tv_nsec_billion_einval() {
        // exactly 1e9 is out of range (valid range is 0..=999_999_999)
        let ts = Timespec { tv_sec: 0, tv_nsec: 1_000_000_000 };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_tv_nsec_max_valid_passes_to_eperm() {
        // 999_999_999 is the max legal nanosecond value.  It should
        // pass the timespec check and fall through to EPERM.
        let ts = Timespec { tv_sec: 0, tv_nsec: 999_999_999 };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_clock_settime_realtime_coarse_einval_not_eperm() {
        // CLOCK_REALTIME_COARSE shares the "realtime" name but is a
        // distinct, unsettable clock id.  This test catches the bug
        // where a naive implementation accepts any clock containing
        // "REALTIME" in its name.
        let ts = valid_ts();
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME_COARSE, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_unsettable_clock_precedes_timespec_check() {
        // Bad timespec on an unsettable clock must still surface the
        // EINVAL-for-clock-kind path (it's the same errno, so we
        // really just verify no crash and a sensible code).
        let ts = Timespec { tv_sec: -1, tv_nsec: -1 };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_MONOTONIC, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_zero_timespec_realtime_eperm() {
        // The epoch is a legal timespec value.  Linux accepts it
        // and would set the clock to 1970-01-01; we return EPERM
        // because we can't actually set the wall clock.
        let ts = Timespec { tv_sec: 0, tv_nsec: 0 };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_clock_settime_large_tv_sec_realtime_eperm() {
        // A far-future timestamp (year ~2262) — must still pass the
        // argument check and reach EPERM.
        let ts = Timespec { tv_sec: 9_223_372_036, tv_nsec: 500 };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_clock_settime_errno_progression() {
        // Sanity check that every distinct branch produces the
        // distinct errno we claim.  This guards against future
        // refactors that accidentally collapse branches.
        let valid = valid_ts();
        let bad_ns = Timespec { tv_sec: 0, tv_nsec: -1 };

        errno::set_errno(0);
        assert_eq!(clock_settime(CLOCK_REALTIME, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);

        errno::set_errno(0);
        assert_eq!(clock_settime(999, &raw const valid), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        assert_eq!(clock_settime(CLOCK_BOOTTIME, &raw const valid), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        assert_eq!(clock_settime(CLOCK_REALTIME, &raw const bad_ns), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        assert_eq!(clock_settime(CLOCK_REALTIME, &raw const valid), -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_null_tv_and_tz_is_success() {
        // Both NULL → well-formed no-op success.
        errno::set_errno(0);
        let ret = settimeofday(core::ptr::null(), core::ptr::null());
        assert_eq!(ret, 0);
    }

    // ---------------------------------------------------------------------
    // Phase 84 — settimeofday argument-domain validation
    //
    // Linux semantics being validated:
    //   - tv NULL && tz NULL → 0
    //   - tv non-NULL with out-of-range tv_sec/tv_usec → -1, EINVAL
    //   - structurally valid tv → -1, EPERM (no CAP_SYS_TIME)
    //   - tv NULL && tz non-NULL → 0 (deprecated tz-only no-op)
    // ---------------------------------------------------------------------

    #[test]
    fn test_settimeofday_phase84_both_null_returns_zero() {
        errno::set_errno(0xBAD);
        let ret = settimeofday(core::ptr::null(), core::ptr::null());
        assert_eq!(ret, 0);
        // Success path must not clobber errno to anything we set.
        // (We don't require it to be 0 — Linux preserves errno on success.)
    }

    #[test]
    fn test_settimeofday_phase84_null_tv_with_tz_is_success() {
        // tv NULL but tz non-NULL: tz-only update is accepted as a no-op.
        let sentinel: u8 = 0;
        let tz = &raw const sentinel as *const core::ffi::c_void;
        errno::set_errno(0);
        let ret = settimeofday(core::ptr::null(), tz);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_settimeofday_phase84_negative_tv_sec_is_einval() {
        let tv = Timeval { tv_sec: -1, tv_usec: 0 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_negative_tv_usec_is_einval() {
        let tv = Timeval { tv_sec: 0, tv_usec: -1 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_tv_usec_equal_to_million_is_einval() {
        // The valid range is [0, 999_999]; exactly 1_000_000 must be rejected.
        let tv = Timeval { tv_sec: 0, tv_usec: 1_000_000 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_tv_usec_above_million_is_einval() {
        let tv = Timeval { tv_sec: 0, tv_usec: 9_999_999 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_max_valid_tv_usec_is_eperm() {
        // 999_999 is the maximum valid microsecond value — must reach EPERM,
        // not EINVAL.
        let tv = Timeval { tv_sec: 0, tv_usec: 999_999 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_zero_tv_is_eperm() {
        let tv = Timeval { tv_sec: 0, tv_usec: 0 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_large_valid_tv_is_eperm() {
        // Reasonably distant future timestamp — still valid argument-wise.
        let tv = Timeval { tv_sec: 2_000_000_000, tv_usec: 500_000 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_valid_tv_with_nonnull_tz_is_eperm() {
        // tv valid + tz non-NULL: EPERM (we still can't set the clock).
        let tv = Timeval { tv_sec: 1, tv_usec: 1 };
        let sentinel: u8 = 0;
        let tz = &raw const sentinel as *const core::ffi::c_void;
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, tz);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_einval_ordering_precedes_eperm() {
        // A bad tv_usec must be detected as EINVAL even when tz is
        // also non-NULL — validation order is "tv-field-shape" first,
        // EPERM last.
        let tv = Timeval { tv_sec: 0, tv_usec: -42 };
        let sentinel: u8 = 0;
        let tz = &raw const sentinel as *const core::ffi::c_void;
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, tz);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_negative_sec_takes_precedence_over_bad_usec() {
        // When both fields are out of range, we still report EINVAL.
        let tv = Timeval { tv_sec: -5, tv_usec: 9_999_999 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_does_not_overflow_on_intmax_sec() {
        // Pathological but well-formed input: i64::MAX seconds, 0 us.
        // Must not panic; must return EPERM.
        let tv = Timeval { tv_sec: i64::MAX, tv_usec: 0 };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_repeated_calls_are_stable() {
        // Calling settimeofday repeatedly with the same valid args should
        // produce identical results — no hidden global state.
        let tv = Timeval { tv_sec: 1000, tv_usec: 1000 };
        for _ in 0..5 {
            errno::set_errno(0);
            let ret = settimeofday(&raw const tv, core::ptr::null());
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }
    }

    #[test]
    fn test_settimeofday_phase84_einval_does_not_alter_subsequent_call() {
        // An EINVAL failure must not leave residual state that taints a
        // subsequent valid call's errno.
        let bad = Timeval { tv_sec: 0, tv_usec: -1 };
        errno::set_errno(0);
        assert_eq!(settimeofday(&raw const bad, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let good = Timeval { tv_sec: 0, tv_usec: 0 };
        errno::set_errno(0);
        assert_eq!(settimeofday(&raw const good, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_null_tv_then_valid_tv_progression() {
        // Both-NULL success path followed by valid-tv EPERM path.
        errno::set_errno(0);
        assert_eq!(settimeofday(core::ptr::null(), core::ptr::null()), 0);

        let tv = Timeval { tv_sec: 1, tv_usec: 1 };
        errno::set_errno(0);
        assert_eq!(settimeofday(&raw const tv, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_timegm_epoch() {
        // 1970-01-01 00:00:00 → epoch 0
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        let t = timegm(&raw mut tm);
        assert_eq!(t, 0, "epoch should be 0");
    }

    #[test]
    fn test_timegm_y2k() {
        // 2000-01-01 00:00:00 → 946684800
        let mut tm = zero_tm();
        tm.tm_year = 100;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        let t = timegm(&raw mut tm);
        assert_eq!(t, 946_684_800);
    }

    #[test]
    fn test_timelocal_epoch() {
        // timelocal is an alias for mktime.
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        let t = timelocal(&raw mut tm);
        // Should be 0 (UTC = local on our OS).
        assert_eq!(t, 0);
    }

    #[test]
    fn test_time_no_crash() {
        // time(NULL) — syscall result is unpredictable on test host.
        let _t = time(core::ptr::null_mut());
    }

    #[test]
    fn test_time_with_output() {
        // time(&t) should store the value when the syscall succeeds.
        // On the test host the syscall may fail (returns -1 without
        // writing to tloc), so just verify no crash.
        let mut t: i64 = -999;
        let ret = time(&raw mut t);
        if ret >= 0 {
            assert_eq!(ret, t, "time() return should equal stored value");
        }
        // If ret == -1, t may or may not have been written.
    }

    #[test]
    fn test_clock_no_crash() {
        // clock() — syscall result is unpredictable on test host.
        let _c = clock();
    }

    #[test]
    fn test_difftime_large_gap() {
        // Large time difference (year-apart).
        assert_eq!(difftime(31_536_000, 0), 31_536_000.0);
    }

    #[test]
    fn test_usleep_small_value() {
        // usleep(1) — 1 microsecond — should return quickly.
        let _ret = usleep(1);
    }
}

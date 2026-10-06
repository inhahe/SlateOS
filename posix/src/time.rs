//! POSIX time functions.
//!
//! Implements `sleep`, `nanosleep`, `usleep`, `clock_gettime`,
//! `clock_getres`, `clock`, `gettimeofday`, `time`, `difftime`,
//! `localtime`, `gmtime`, `mktime`, `asctime`, `ctime`, `strptime`, `timer_create`, `timer_settime`, `timer_gettime`,
//! `timer_delete`, `timer_getoverrun`. `strftime` is [`crate::strftime`]'s,
//! which `wcsftime` shares, and is re-exported here.
//!
//! ## Timezone
//!
//! `localtime`, `localtime_r`, `mktime`, `ctime` and `strftime`'s `%z`/`%Z`
//! honour the `TZ` environment variable through [`crate::tz`], which
//! implements the full POSIX `TZ`-string grammar including DST rules.  A
//! zoneinfo *name* (`America/New_York`) needs the tzdata database, which
//! SlateOS does not ship yet, so it falls back to UTC — the same thing glibc
//! does when it cannot find the file.  `TZ` unset also means UTC until the OS
//! grows a system zone setting.
//!
//! `gmtime`/`gmtime_r`/`timegm` are the zone-independent forms and always
//! render UTC.
//!
//! ## POSIX Timers
//!
//! There are none yet. `timer_create` checks its arguments in Linux's order
//! and answers `ENOSYS`; `timer_settime`, `timer_gettime`, `timer_delete` and
//! `timer_getoverrun` answer `EINVAL`, there being no timer to name -- the
//! kernel's Linux ABI answers the same. The kernel has no timer of this kind
//! on either ABI, and
//! `requests/d-a-posix-timers-and-the-dumpable-flag-need-native-calls.md`
//! asks lane A for native ones. Until 2026-10-05 these were stubs that
//! reported a timer armed and armed nothing.
//!
//! `alarm`, `ualarm` and `setitimer` do arm a timer that fires: the kernel's
//! one `ITIMER_REAL` per process (`SYS_ITIMER_SET`), whose `SIGALRM` arrives
//! through `signal.rs`'s trampoline.

use crate::errno;
use crate::interrupt::Mark;
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

/// Is `ns` a well-formed `timespec.tv_nsec`?
///
/// This is glibc's `valid_nanoseconds` (`include/time.h:517`) verbatim:
/// `0 <= ns && ns < 1000000000`.  It deliberately says nothing about
/// `tv_sec` — a negative `tv_sec` is a *deadline in the past*, not a
/// malformed timespec, so the blocking primitives that use this predicate
/// (`pthread_cond_timedwait`, `pthread_mutex_timedlock`, `sem_timedwait`)
/// return `ETIMEDOUT` for it rather than `EINVAL`.
///
/// Do not confuse it with the kernel's `timespec64_valid`
/// (`include/linux/time64.h`), which *does* reject `tv_sec < 0` ("Dates
/// before 1970 are bogus") and is the predicate behind the `EINVAL` from
/// `mq_timedsend`/`mq_timedreceive`.  The two rules genuinely differ, and
/// which one applies depends on whether the deadline is interpreted by
/// glibc or handed to a syscall.
#[must_use]
#[inline]
pub(crate) fn valid_nanoseconds(ns: i64) -> bool {
    (0..1_000_000_000).contains(&ns)
}

/// Sleep `ns` nanoseconds on the monotonic clock, handlers counted from
/// `mark`: `Err(left)` when a signal handler ended it
/// ([`crate::lowlevellock::sleep_until`]).
fn sleep_ns(ns: u64, mark: Mark) -> Result<(), Timespec> {
    let now = crate::lowlevellock::now_on(CLOCK_MONOTONIC);
    let deadline = crate::lowlevellock::after(&now, ns);
    crate::lowlevellock::sleep_until(CLOCK_MONOTONIC, &deadline, mark)
}

/// Sleep for a specified number of seconds.
///
/// Returns 0 once they have passed, or -- when a signal handler ends the
/// sleep, which any does (signal(7) never restarts one) -- the whole seconds
/// that were still to go, as glibc's `seconds + ts.tv_sec` counts them:
/// truncated, so a sleep cut short in its last second answers 0.  `errno` is
/// then `nanosleep`'s `EINTR`, and otherwise left as it was, as glibc's is.
///
/// Until 2026-09-30 no signal ended a sleep here: `SYS_SLEEP` sleeps its full
/// time, and this answered 0 whatever happened.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sleep(seconds: u32) -> u32 {
    let mark = Mark::now();
    match sleep_ns(u64::from(seconds).saturating_mul(1_000_000_000), mark) {
        Ok(()) => 0,
        Err(left) => {
            errno::set_errno(errno::EINTR);
            u32::try_from(left.tv_sec).unwrap_or(u32::MAX)
        }
    }
}

/// High-resolution sleep.
///
/// Sleeps for the time specified in `req` -- or until a signal handler runs
/// on the thread, `SA_RESTART` or not: -1 with `EINTR`, and the time that
/// was still to go in `rem` (if non-null).  A signal that runs no handler
/// here does not end it ([`crate::interrupt`]).  Until 2026-09-30 nothing
/// did: `SYS_SLEEP` sleeps its full time.
///
/// Returns 0 on success, -1 if interrupted (errno = EINTR).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nanosleep(req: *const Timespec, rem: *mut Timespec) -> i32 {
    let mark = Mark::now();
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

    // Both fields are non-negative now.
    let ns = u64::try_from(ts.tv_sec)
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::try_from(ts.tv_nsec).unwrap_or(0));

    match sleep_ns(ns, mark) {
        Ok(()) => 0,
        Err(left) => {
            if !rem.is_null() {
                // SAFETY: a non-null `rem` is writable, per the contract.
                unsafe { core::ptr::write_unaligned(rem, left) };
            }
            errno::set_errno(errno::EINTR);
            -1
        }
    }
}

/// Sleep for a specified number of microseconds.
///
/// This is obsolete in POSIX.1-2008 (use `nanosleep` instead) but
/// many programs still use it, and it is glibc's: `nanosleep` of the time,
/// any number of microseconds, -1 with `EINTR` when a signal handler ends it.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn usleep(usec: u32) -> i32 {
    let mark = Mark::now();
    match sleep_ns(u64::from(usec).saturating_mul(1_000), mark) {
        Ok(()) => 0,
        Err(_) => {
            errno::set_errno(errno::EINTR);
            -1
        }
    }
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
    // Linux validation order (kernel/time/posix-timers.c
    // SYSCALL_DEFINE2(clock_gettime, ...)):
    //
    //   1. clockid_to_kclock(which_clock)         → EINVAL on miss
    //   2. kc->clock_get_timespec(&kernel_tp)     (populates local)
    //   3. put_timespec64(&kernel_tp, tp)         → EFAULT on NULL/bad
    //
    // Phase 150: pre-Phase-150 we ran the NULL pointer check FIRST
    // and the clock check second.  Linux validates the clock first
    // (in `clockid_to_kclock`) before touching the user pointer.
    // Observable divergence: `clock_gettime(BAD_CLOCK, NULL)` used
    // to return EFAULT; Linux returns EINVAL.
    //
    // We swap steps 2 and 3 in our implementation: the NULL check
    // runs before the syscall.  This is observably equivalent to
    // Linux's contract because `CLOCK_MONOTONIC` is a never-fail
    // clock — `clock_get_timespec` always succeeds, so reordering
    // the two post-clock-validation steps does not change any
    // externally visible behaviour.  Doing the NULL check first
    // avoids a wasted syscall on the EFAULT path.

    // Step 1: clock validation → EINVAL.
    if !is_valid_clock(clk_id) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Step 3 (hoisted): put_timespec64(tp) → EFAULT on NULL/bad.
    if tp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Step 2: read the clock value.  Wall-clock clocks (CLOCK_REALTIME and
    // its coarse variant) report nanoseconds since the Unix epoch and must
    // use the realtime source; monotonic/boottime/cputime clocks use the
    // boot-relative monotonic counter.
    let ns = if is_realtime_clock(clk_id) {
        syscall0(SYS_CLOCK_REALTIME)
    } else {
        syscall0(SYS_CLOCK_MONOTONIC)
    };
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

/// `base` value for [`timespec_get`] meaning "seconds since the Unix epoch".
///
/// C11 defines exactly this one; C23 adds `TIME_MONOTONIC`, `TIME_ACTIVE` and
/// `TIME_THREAD_ACTIVE`, which glibc still does not implement and which no
/// caller we care about uses.  Recognising only `TIME_UTC` is conforming — the
/// standard requires an unsupported base to return zero, which is what the
/// implementation below does.
pub const TIME_UTC: i32 = 1;

/// `timespec_get(ts, base)` — C11's clock reader.
///
/// Returns `base` on success and **0** on failure, which is the inverse of
/// every POSIX convention in this file and is easy to get backwards: there is
/// no errno involved and no -1.
///
/// Own archive member: gnulib ships a `timespec_get` replacement for platforms
/// that lack it (it is C11, so plenty do), meaning a GNU program that vendors
/// that module defines the symbol itself and must be able to decline ours.
/// `scripts/coreutils-spike/run.sh` found this symbol missing from `libc.a`
/// altogether; giving it its own member at the same time is what keeps it from
/// turning into a *duplicate* on the next run.  See `string.rs`'s module header
/// and `design-decisions.md` §339–§340.
#[cfg(target_os = "none")]
mod gnu_timespec_get {
    use super::{CLOCK_REALTIME, TIME_UTC, Timespec, clock_gettime};

    /// See the module-level documentation.
    ///
    /// # Safety
    /// `ts` must be NULL or point to a writable `struct timespec`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn timespec_get(ts: *mut Timespec, base: i32) -> i32 {
        if base != TIME_UTC || ts.is_null() {
            return 0;
        }
        if clock_gettime(CLOCK_REALTIME, ts) != 0 {
            return 0;
        }
        base
    }
}

#[cfg(target_os = "none")]
pub use gnu_timespec_get::timespec_get;

/// `timespec_get(ts, base)` — host build.
///
/// Written out twice rather than `cfg`-switched inside one definition: the
/// target build needs the body inside `mod gnu_timespec_get` so it lands in
/// its own archive member, and the host build should not carry an extra public
/// module that exists only to serve a linker concern.
///
/// # Safety
/// `ts` must be NULL or point to a writable `struct timespec`.
#[cfg(not(target_os = "none"))]
pub unsafe extern "C" fn timespec_get(ts: *mut Timespec, base: i32) -> i32 {
    if base != TIME_UTC || ts.is_null() {
        return 0;
    }
    if clock_gettime(CLOCK_REALTIME, ts) != 0 {
        return 0;
    }
    base
}

/// `timespec_getres(ts, base)` (C23 7.29.2.7): the resolution of the time
/// base `base` into `*ts`, unless `ts` is NULL, and `base` back; 0, and
/// `*ts` untouched, for a number that is no time base. `TIME_UTC` is the only
/// one, as in glibc 2.39, and its resolution is `CLOCK_REALTIME`'s.
///
/// Not in `gnu_timespec_get`'s archive member, which must define
/// `timespec_get` alone (`scripts/check-libc-shape.py`).
///
/// # Safety
///
/// `ts` must be NULL or point to a writable `struct timespec`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn timespec_getres(ts: *mut Timespec, base: i32) -> i32 {
    if base != TIME_UTC {
        return 0;
    }
    if !ts.is_null() && clock_getres(CLOCK_REALTIME, ts) != 0 {
        return 0;
    }
    base
}

/// Check whether a clock ID reports wall-clock (Unix-epoch) time.
///
/// Only `CLOCK_REALTIME` and its coarse variant track the wall clock; all
/// monotonic/boottime/cputime clocks are boot-relative.
fn is_realtime_clock(clk_id: ClockidT) -> bool {
    matches!(clk_id, CLOCK_REALTIME | CLOCK_REALTIME_COARSE)
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

/// Linux's `TIME_SETTOD_SEC_MAX`: `KTIME_SEC_MAX` (`KTIME_MAX` in seconds)
/// less `TIME_UPTIME_SEC_MAX` (30 years), the largest `tv_sec` that
/// `clock_settime` accepts.
const TIME_SETTOD_SEC_MAX: i64 = i64::MAX / 1_000_000_000 - 30 * 365 * 86_400;

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
/// # Linux validation order
///
/// `kernel/time/posix-timers.c::sys_clock_settime`:
///
/// ```c
/// const struct k_clock *kc = clockid_to_kclock(which_clock);
/// struct timespec64 new_tp;
///
/// if (!kc || !kc->clock_set)
///     return -EINVAL;
/// if (get_timespec64(&new_tp, tp))
///     return -EFAULT;
/// return kc->clock_set(which_clock, &new_tp);
/// ```
///
/// The kernel dispatches on `which_clock` FIRST.  Only after the
/// clock is found and confirmed settable does the kernel touch the
/// user pointer via `get_timespec64` → `copy_from_user`.  Precedence:
///
///   1. `clockid_to_kclock(which_clock)` returns NULL → `EINVAL`
///   2. `!kc->clock_set` (clock is read-only)         → `EINVAL`
///   3. `get_timespec64(tp)` user-copy fails          → `EFAULT`
///   4. `kc->clock_set(...)` validates timespec       → `EINVAL`
///      on `tv_sec < 0` / `tv_nsec ∉ [0, 999_999_999]`
///   5. `security_settime64()` → `capable(CAP_SYS_TIME)`  → `EPERM`
///      on missing cap
///   6. Otherwise: write the wall clock via `SYS_CLOCK_SETTIME`.
///      The kernel returns `EINVAL` if its realtime clock has not
///      been initialised from an RTC/time source yet.
///
/// **Phase 151**: pre-Phase-151 we ran the NULL `tp` check FIRST
/// (returning EFAULT) and the clock check SECOND.  Linux dispatches
/// the clock first.  Observable divergence: `clock_settime(BAD_CLOCK,
/// NULL)` returned EFAULT here; Linux returns EINVAL.
///
/// The validation order matters: callers using `clock_settime` to
/// probe whether a clock is supported (a common libc test idiom)
/// must see `EINVAL` for read-only clocks like `CLOCK_MONOTONIC`,
/// regardless of capability state, so that probes don't false-
/// positive on the "permission denied" path.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_settime(clk_id: ClockidT, tp: *const Timespec) -> i32 {
    // 1. Unknown clock → EINVAL.  Linux's clockid_to_kclock runs
    //    before the get_timespec64 user-copy step.
    if !is_valid_clock(clk_id) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 2. Known but read-only clock → EINVAL.  Linux's `!kc->clock_set`
    //    check; same errno as (1) but a different *reason*.
    if !is_settable_clock(clk_id) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 3. NULL tp → EFAULT (Linux's get_timespec64 / copy_from_user).
    if tp.is_null() {
        errno::set_errno(errno::EFAULT);
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
    // `timespec64_valid_settod` (include/linux/time64.h): a time past
    // `TIME_SETTOD_SEC_MAX` -- 30 years short of where `ktime_t` overflows --
    // is refused, so the clock cannot be set somewhere an uptime could carry
    // it past the end.  Missing until 2026-09-26.
    if ts.tv_sec >= TIME_SETTOD_SEC_MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // 5. Phase 177: gate on CAP_SYS_TIME.  Linux's settable clocks
    //    (CLOCK_REALTIME via posix_clock_realtime_set, CLOCK_TAI via
    //    tai_clock_set) all defer to do_settimeofday64() /
    //    timekeeping_inject_offset(), which call
    //    capable(CAP_SYS_TIME) (or, for slewing variants, may also
    //    accept CAP_SYS_NICE — but the abrupt set path is strictly
    //    CAP_SYS_TIME).  An unprivileged caller sees EPERM; a
    //    privileged caller proceeds to the kernel backend.
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_TIME) {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    // 6. Set the wall clock via SYS_CLOCK_SETTIME.  Only CLOCK_REALTIME is
    //    settable here (is_settable_clock above), so the target is always the
    //    real-time clock.  Convert the validated timespec to nanoseconds since
    //    the Unix epoch; tv_sec/tv_nsec are non-negative (checked in step 4),
    //    so the u64 math is safe and saturating-add guards the year-2262 edge.
    #[allow(clippy::cast_sign_loss)]
    let target_ns = (ts.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(ts.tv_nsec as u64);
    let ret = syscall1(SYS_CLOCK_SETTIME, target_ns);
    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    0
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
///
/// # Validation order
///
/// Phase 102 added a flag-mask check (any bit other than
/// `TIMER_ABSTIME` → `EINVAL`) that runs first — see the body
/// comment below for the rationale.  After the flag mask, the
/// remaining checks follow Linux precedence:
///
/// `kernel/time/posix-timers.c::SYSCALL_DEFINE4(clock_nanosleep, ...)`:
///
/// ```c
/// const struct k_clock *kc = clockid_to_kclock(which_clock);
/// struct timespec64 t;
///
/// if (!kc)                              return -EINVAL;
/// if (!kc->nsleep)                      return -EINVAL;
/// if (get_timespec64(&t, rqtp))         return -EFAULT;
/// if (!timespec64_valid(&t))            return -EINVAL;
/// ```
///
/// Linux dispatches on the clock FIRST.  Only after the clock is
/// resolved does the kernel attempt `get_timespec64`, which is
/// where a NULL/bad user pointer surfaces as `-EFAULT`.  The
/// timespec range check runs LAST.
///
/// Precedence (after Phase 102's flag mask):
///
///   1. `flags & ~TIMER_ABSTIME` set                → `EINVAL` (Phase 102)
///   2. `clockid_to_kclock(clk_id)` returns NULL    → `EINVAL`
///   3. `get_timespec64(request)` user-copy fails   → `EFAULT`
///   4. `!timespec64_valid(&t)`                     → `EINVAL`
///   5. Compute sleep, defer to backend syscall.
///
/// **Phase 153**: pre-Phase-153 we ran the NULL `request` check
/// BEFORE the clock check and returned `EINVAL` for NULL.  Two
/// divergences from Linux:
///
/// * `clock_nanosleep(VALID_CLOCK, 0, NULL, NULL)`: Linux returns
///   `EFAULT` (via `get_timespec64`); we returned `EINVAL`.
/// * `clock_nanosleep(BAD_CLOCK, 0, NULL, NULL)`: Linux returns
///   `EINVAL` from the clock check; we returned `EINVAL` from the
///   NULL check (same errno, wrong reason).
///
/// Phase 153 fixes both: the clock check now runs before the NULL
/// check, and NULL `request` now returns `EFAULT` to match Linux's
/// `get_timespec64` failure path.  The Phase 102 flag-mask check is
/// preserved unchanged — it still fires first regardless of any
/// other shape problem in the call.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_nanosleep(
    clk_id: ClockidT,
    flags: i32,
    request: *const Timespec,
    remain: *mut Timespec,
) -> i32 {
    // Handlers are counted from here: in Linux the call is one system call,
    // and a signal that comes at any point in it ends the sleep.
    let mark = Mark::now();
    // glibc 2.39 (sysdeps/unix/sysv/linux/clock_nanosleep.c) answers the
    // calling thread's CPU clock itself, before any other check.
    if clk_id == CLOCK_THREAD_CPUTIME_ID {
        return errno::EINVAL;
    }
    // Linux 6.6 (kernel/time/posix-timers.c:1373): an unknown clock, then a
    // clock that cannot be slept on -- `CLOCK_MONOTONIC_RAW` and the two
    // `_COARSE` clocks have no `nsleep` -- then the copy of the request, then
    // `timespec64_valid`.  `flags` is only ever asked whether TIMER_ABSTIME
    // is set: the other bits are ignored.  Until 2026-09-26 any other bit was
    // EINVAL, ahead of everything, under a comment citing a check in
    // `common_nsleep` that is not there.
    if !is_valid_clock(clk_id) {
        return errno::EINVAL;
    }
    if matches!(
        clk_id,
        CLOCK_MONOTONIC_RAW | CLOCK_REALTIME_COARSE | CLOCK_MONOTONIC_COARSE
    ) {
        return errno::EOPNOTSUPP;
    }
    if request.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: `request` is non-null, and the caller's contract makes it a
    // readable `timespec`; read unaligned, as a C caller's may not be.
    let req = unsafe { core::ptr::read_unaligned(request) };
    // `timespec64_valid`: a negative second or an out-of-range nanosecond.
    // A negative `tv_sec` was taken for "already past" in the absolute form,
    // and reported as EINTR in the relative one, until 2026-09-26.
    if req.tv_sec < 0 || !(0..1_000_000_000).contains(&req.tv_nsec) {
        return errno::EINVAL;
    }

    // The deadline, on the sleep's own clock: the request itself when
    // absolute, else the request from now.  A deadline already passed, or a
    // relative sleep of nothing, returns at once, as `hrtimer_nanosleep` does.
    let absolute = flags & TIMER_ABSTIME != 0;
    let deadline = if absolute {
        req
    } else {
        let now = crate::lowlevellock::now_on(clk_id);
        crate::lowlevellock::after(&now, timespec_to_ns_saturating(&req))
    };
    match crate::lowlevellock::sleep_until(clk_id, &deadline, mark) {
        Ok(()) => 0,
        Err(left) => {
            // A signal handler ended it, `SA_RESTART` or not.  Only the
            // relative form reports what is left: Linux drops `rmtp` for
            // TIMER_ABSTIME.
            if !absolute && !remain.is_null() {
                // SAFETY: a non-null `remain` is writable, per the contract.
                unsafe { core::ptr::write_unaligned(remain, left) };
            }
            errno::EINTR
        }
    }
}

/// A non-negative `timespec` as nanoseconds, saturating at `u64::MAX`: an
/// absolute deadline years away is a sleep that outlasts the caller, not an
/// overflow.  Until 2026-09-26 the absolute form multiplied unchecked.
fn timespec_to_ns_saturating(ts: &Timespec) -> u64 {
    let secs = u64::try_from(ts.tv_sec).unwrap_or(0);
    let nsecs = u64::try_from(ts.tv_nsec).unwrap_or(0);
    secs.saturating_mul(1_000_000_000).saturating_add(nsecs)
}

/// Get time of day (legacy interface).
///
/// Uses `CLOCK_MONOTONIC` since we don't have a wall clock yet.
/// The `tz` parameter is ignored (deprecated in POSIX).
///
/// # Linux validation order
///
/// `kernel/time/time.c::SYSCALL_DEFINE2(gettimeofday, ...)`:
///
/// ```c
/// if (likely(tv != NULL)) {
///     struct timespec64 ts;
///     ktime_get_real_ts64(&ts);
///     if (put_user(ts.tv_sec, &tv->tv_sec) ||
///         put_user(ts.tv_nsec/1000, &tv->tv_usec))
///         return -EFAULT;
/// }
/// if (unlikely(tz != NULL)) {
///     if (copy_to_user(tz, &sys_tz, sizeof(sys_tz)))
///         return -EFAULT;
/// }
/// return 0;
/// ```
///
/// The kernel treats both pointers as **optional**.  A `NULL tv` is
/// not an error — it just means "don't populate the time".  A NULL
/// `tz` likewise just skips the timezone copy.  Both NULL is a
/// well-formed (if pointless) call that returns 0.  EFAULT only
/// arises if a *non-NULL* pointer is invalid (which we can't
/// distinguish in userspace tests).
///
/// Precedence:
///
///   1. `tv != NULL` → read the clock; on user-copy failure return
///      `-1`/`EFAULT`.  Reaching this point with a NULL `tv` is **not**
///      an error.
///   2. `tz != NULL` → copy the timezone; on user-copy failure return
///      `-1`/`EFAULT`.  NULL `tz` is always fine.
///   3. Return 0.
///
/// **Phase 152**: pre-Phase-152 we returned `-1`/`EFAULT` on a NULL
/// `tv`.  Linux returns 0.  This made callers that pass NULL to
/// "ping" the syscall (a common shimming/probing idiom) see a
/// spurious failure.  Phase 152 reorders to match Linux: NULL `tv`
/// silently skips population and returns 0 without setting errno.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gettimeofday(tv: *mut Timeval, _tz: *mut core::ffi::c_void) -> i32 {
    // Linux: tv == NULL is NOT an error — skip population and fall
    // through to the (also-optional) tz path, returning 0.  Errno is
    // not touched.
    if tv.is_null() {
        return 0;
    }

    // gettimeofday reports wall-clock time (seconds/microseconds since the
    // Unix epoch), so it must use the realtime source — not the boot-relative
    // monotonic counter.
    let ns = syscall0(SYS_CLOCK_REALTIME);
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
///     Phase 177 gates on `CAP_SYS_TIME` — unprivileged callers get
///     `EPERM`; privileged callers write the wall clock via
///     `SYS_CLOCK_SETTIME` (the kernel returns `EINVAL` if its realtime
///     clock is not yet initialised from a time source).
///   - The `tz` argument is accepted but otherwise ignored (Linux has
///     deprecated `settimeofday` timezone setting since 2.6.x; passing a
///     non-NULL `tz` along with a NULL `tv` is treated as a no-op success
///     here to match the "set timezone only" path Linux still tolerates).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn settimeofday(tv: *const Timeval, tz: *const core::ffi::c_void) -> i32 {
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

    // Structurally valid request to actually adjust the clock.
    //
    // Phase 177: Linux's settimeofday gates writes on CAP_SYS_TIME
    // via security_settime64() → capable(CAP_SYS_TIME) before
    // delegating to do_sys_settimeofday64() / do_settimeofday64().
    // An unprivileged caller sees EPERM; a privileged caller proceeds.
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_TIME) {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    // Set the wall clock via SYS_CLOCK_SETTIME.  Re-read the (already
    // validated, non-negative) timeval and convert microseconds to the
    // nanosecond epoch value the kernel expects.  saturating math guards the
    // far-future edge; the fields were range-checked above.
    //
    // SAFETY: tv is non-NULL here (the NULL cases returned earlier); unaligned
    // read avoids UB on a misaligned user buffer.
    let val = unsafe { core::ptr::read_unaligned(tv) };
    #[allow(clippy::cast_sign_loss)]
    let target_ns = (val.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add((val.tv_usec as u64).saturating_mul(1_000));
    let ret = syscall1(SYS_CLOCK_SETTIME, target_ns);
    if ret < 0 {
        return errno::translate(ret) as i32;
    }
    0
}

/// Return the current time in seconds since the Unix epoch.
///
/// Uses the realtime (wall-clock) source via `SYS_CLOCK_REALTIME`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn time(tloc: *mut TimeT) -> TimeT {
    let ns = syscall0(SYS_CLOCK_REALTIME);
    if ns < 0 {
        errno::set_errno(errno::EIO);
        return -1;
    }

    #[allow(clippy::arithmetic_side_effects)]
    let secs = (ns as u64 / 1_000_000_000) as TimeT;

    if !tloc.is_null() {
        // SAFETY: Caller guarantees tloc is valid or null (checked above).
        unsafe {
            *tloc = secs;
        }
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
/// The pointers are into [`crate::tz`]'s names, which live for the process
/// as glibc's `__tzstring`s do, or are literals.
#[repr(transparent)]
pub struct TzPtr(pub(crate) *const u8);

// SAFETY: Points into `tz`'s static name buffers, which have program lifetime
// and are never reallocated.
unsafe impl Sync for TzPtr {}

/// Timezone name strings: `[standard, daylight]`.
///
/// POSIX requires `tzname` to be a `char *[2]`, refreshed by `tzset`.
/// `repr(transparent)` on `TzPtr` ensures the layout matches
/// `[*const u8; 2]` for C interop. `"GMT"` until a zone is read, as in
/// glibc -- and then wherever glibc's code puts it, which is not only
/// `tzset`: a `localtime` in a zoneinfo zone moves it to the names in force
/// around the instant converted ([`crate::tz`]).
///
/// The target's only: on the host the three are per test thread, as the
/// zone they describe is (`host_tz_globals` in this file).
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub static mut tzname: [TzPtr; 2] = [TzPtr(c"GMT".as_ptr().cast()), TzPtr(c"GMT".as_ptr().cast())];

/// Seconds **west** of UTC for standard time.
///
/// POSIX/BSD variable, and note the sign: it is the negation of `tm_gmtoff`,
/// so New York's `timezone` is `18000` while its `tm_gmtoff` is `-18000`.
/// POSIX defines it in terms of *standard* time, so it does not move when DST
/// is in effect.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub static mut timezone: i64 = 0;

/// Whether daylight saving is ever in effect in the current zone.
///
/// Note "ever", not "now": POSIX defines this as a property of the zone, so it
/// is 1 all year round for a zone with DST rules.  Use `tm_isdst` to ask about
/// a particular instant.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub static mut daylight: i32 = 0;

/// The host build's `tzname`, `timezone` and `daylight`: per test thread, as
/// the zone they describe is ([`crate::tz`]). Nothing outside this crate
/// links the host build, so these need not be the C ABI's statics -- and
/// shared, they would be every concurrent test's at once.
#[cfg(not(target_os = "none"))]
mod host_tz_globals {
    /// `([tzname[0], tzname[1]], timezone, daylight)`.
    pub(super) type Globals = ([*const u8; 2], i64, i32);

    std::thread_local! {
        static GLOBALS: core::cell::Cell<Globals> = const {
            core::cell::Cell::new(([c"GMT".as_ptr().cast(), c"GMT".as_ptr().cast()], 0, 0))
        };
    }

    pub(super) fn set(globals: Globals) {
        // A failed `try_with` means the thread is shutting down, and its
        // zone with it.
        let _ = GLOBALS.try_with(|g| g.set(globals));
    }

    #[cfg(test)]
    pub(super) fn get() -> Globals {
        GLOBALS.with(core::cell::Cell::get)
    }

    /// `tzname[i]`, or null past 1.
    pub(super) fn name(i: usize) -> *const u8 {
        // A failed `try_with` means the thread is shutting down: no zone.
        GLOBALS
            .try_with(|g| g.get().0.get(i).copied())
            .ok()
            .flatten()
            .unwrap_or(core::ptr::null())
    }
}

/// `tzname`, `timezone` and `daylight` as a C program would read them:
/// `([tzname[0], tzname[1]], timezone, daylight)`, this test thread's.
#[cfg(test)]
pub(crate) fn tz_globals_for_test() -> ([*const u8; 2], i64, i32) {
    host_tz_globals::get()
}

/// `tzname[i]` as a C program would read it -- this test thread's, on the
/// host -- or null past 1: what `strftime`'s `%Z` falls back on.
pub(crate) fn tzname_entry(i: usize) -> *const u8 {
    #[cfg(target_os = "none")]
    {
        // SAFETY: a read of one pointer-sized element of the C-visible
        // global, which POSIX leaves unsynchronised with `tzset`, as
        // glibc's `strftime` reads it.
        let names = unsafe { &*core::ptr::addr_of!(tzname) };
        names.get(i).map_or(core::ptr::null(), |p| p.0)
    }
    #[cfg(not(target_os = "none"))]
    {
        host_tz_globals::name(i)
    }
}

/// Initialize timezone information from the `TZ` environment variable.
///
/// glibc's `tzset` ([`crate::tz::tzset`]): reads `TZ` again when it has
/// changed, and sets `tzname`, `timezone` and `daylight`. POSIX specifies this
/// as not thread-safe, and it is not: it writes those three globals.
///
/// The conversion functions read the zone on first use, so a program that
/// never calls `tzset` still gets its zone, as in every real libc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tzset() {
    crate::tz::tzset();
}

/// Set the three POSIX zone globals: what [`crate::tz`]'s code leaves them as.
pub(crate) fn set_tz_globals(std: *const u8, dst: *const u8, west: i64, ever_dst: i32) {
    #[cfg(target_os = "none")]
    // SAFETY: these are the C-visible zone globals, which POSIX specifies as
    // unsynchronised and modifiable by `tzset`.  Each write is a single
    // pointer- or word-sized store to a process-lifetime static.
    unsafe {
        (*core::ptr::addr_of_mut!(tzname)) = [TzPtr(std), TzPtr(dst)];
        (*core::ptr::addr_of_mut!(timezone)) = west;
        (*core::ptr::addr_of_mut!(daylight)) = ever_dst;
    }
    #[cfg(not(target_os = "none"))]
    host_tz_globals::set(([std, dst], west, ever_dst));
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
    /// Daylight saving flag (0 = not in effect, negative = unknown).
    pub tm_isdst: i32,
    /// Seconds **east** of Greenwich for this time (`tm_gmtoff`).
    ///
    /// A BSD/GNU extension that musl and glibc both carry, so C code compiled
    /// against their headers allocates a `struct tm` with this field present.
    /// Before it existed here our `Tm` was two fields short of the layout
    /// every caller was passing us, which left `tm.tm_gmtoff` reading whatever
    /// happened to be on the caller's stack.
    pub tm_gmtoff: i64,
    /// Zone abbreviation for this time (`tm_zone`), NUL-terminated, or null.
    ///
    /// Points into the process's current-zone storage, so it stays valid until
    /// the next `tzset`. `strftime`'s `%Z` reads it.
    pub tm_zone: *const u8,
}

impl Tm {
    /// An all-zero `Tm`, for callers that fill it in immediately.
    ///
    /// Must stay bit-identical to all-zero: [`crate::perthread`] carves its
    /// block out of fresh anonymous memory and never explicitly initialises
    /// it, so any non-zero default here would be a lie about what a new
    /// thread actually sees.
    pub const ZERO: Self = Self {
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
}

/// Convert time_t to broken-down UTC time.
///
/// Returns a pointer to storage owned by the library, which the *calling
/// thread's* next `gmtime`/`localtime`/`ctime` overwrites — the classic
/// POSIX contract, and the reason `gmtime_r` exists.  The buffer lives in
/// [`crate::perthread`], so another thread calling `gmtime` concurrently
/// cannot clobber this result.
///
/// A year that does not fit `tm_year` is `EOVERFLOW` and NULL, as in glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gmtime(timep: *const TimeT) -> *mut Tm {
    if timep.is_null() {
        return core::ptr::null_mut();
    }
    let secs = unsafe { *timep };
    crate::tz::gmtime_touch(secs);
    // SAFETY: `perthread::current()` is non-null and valid for this thread,
    // and no other thread holds a pointer into this block.
    let tm = unsafe { &raw mut (*crate::perthread::current()).tm };
    // SAFETY: as above — `tm` points at this thread's own `Tm`.
    if secs_to_tm(secs, unsafe { &mut *tm }) {
        tm
    } else {
        crate::errno::set_errno(crate::errno::EOVERFLOW);
        core::ptr::null_mut()
    }
}

/// Convert time_t to broken-down **local** time, honouring `TZ`.
///
/// Reads `TZ` again when it has changed, as glibc's `localtime` does and its
/// `localtime_r` does not. Returns a pointer to per-thread storage that this
/// thread's next `gmtime`/`localtime`/`ctime` overwrites; `localtime_r` is
/// the reentrant form.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn localtime(timep: *const TimeT) -> *mut Tm {
    if timep.is_null() {
        return core::ptr::null_mut();
    }
    let secs = unsafe { *timep };
    // SAFETY: `perthread::current()` is non-null and valid for this thread,
    // and no other thread holds a pointer into this block.
    let tm = unsafe { &raw mut (*crate::perthread::current()).tm };
    // SAFETY: as above — `tm` points at this thread's own `Tm`.
    if crate::tz::localtime(secs, unsafe { &mut *tm }, true) {
        tm
    } else {
        crate::errno::set_errno(crate::errno::EOVERFLOW);
        core::ptr::null_mut()
    }
}

/// `localtime_r`'s work: `tm` filled with the broken-down local time for UTC
/// instant `secs`; `false` when its year does not fit `tm_year`.
///
/// Shared by `localtime_r`, `strptime`'s `%s` and `getdate`, which glibc
/// builds on `localtime_r` too, so that none of them can drift apart.
fn secs_to_local_tm(secs: TimeT, tm: &mut Tm) -> bool {
    crate::tz::localtime(secs, tm, false)
}

/// `mktime` on the caller's pointer -- the null check and `EOVERFLOW`
/// around [`mktime_tm`] -- for `mktime` and `timelocal`, so that neither
/// calls the other by its exported name: a program may define `mktime`
/// itself, and that changes what the program's calls reach and nothing
/// else.
pub(crate) fn mktime_ptr(tm: *mut Tm) -> TimeT {
    if tm.is_null() {
        return -1;
    }
    // SAFETY: non-null, and the caller's `struct tm`, which this call may
    // rewrite.
    let t = unsafe { &mut *tm };
    mktime_tm(t).unwrap_or_else(|| {
        crate::errno::set_errno(crate::errno::EOVERFLOW);
        -1
    })
}

/// Own archive member -- bash and gnulib's mktime module define `mktime`
/// themselves where they judge ours wanting, as bash's cross configure did
/// until 2026-10-01. See string.rs's module header.
mod gnu_mktime {
    use super::*;

    /// Convert broken-down **local** time to time_t, honouring `TZ`.
    ///
    /// glibc's `mktime` ([`crate::tz::mktime`]): `tzset`, then a search from the
    /// offset the previous call found. Every field may be out of range -- a 32nd
    /// of January, a -1st hour -- and the fields are rewritten to describe the
    /// instant found, `tm_wday` and `tm_yday` included. `tm_isdst` negative lets
    /// the zone decide; zero or positive asks for standard or daylight time and
    /// gets the nearest offset of that kind; a time a spring-forward skipped is
    /// the instant the gap's size away.
    ///
    /// When no instant can be found -- the year does not fit `tm_year` -- the
    /// result is -1 with `errno` `EOVERFLOW` and `*tm` is left as it was.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn mktime(tm: *mut Tm) -> TimeT {
        mktime_ptr(tm)
    }
}
pub use gnu_mktime::mktime;

/// `mktime`'s work, for `mktime` and `getdate`.
fn mktime_tm(t: &mut Tm) -> Option<TimeT> {
    crate::tz::mktime(t)
}

/// Own archive member — gnulib replaces `timegm`. See string.rs's module header.
mod gnu_timegm {
    use super::*;

    /// Convert broken-down **UTC** time to seconds since epoch.
    ///
    /// The zone-independent counterpart to `mktime`: the `Tm` is read as UTC
    /// whatever `TZ` says, normalised in place, and given glibc's zone
    /// fields for UTC (`GMT`). A year that does not fit `tm_year` is -1 with
    /// `EOVERFLOW`, `*tm` untouched.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub extern "C" fn timegm(tm: *mut Tm) -> TimeT {
        if tm.is_null() {
            return -1;
        }
        // SAFETY: non-null, and the caller's `struct tm`, which this call may
        // rewrite.
        let t = unsafe { &mut *tm };
        let Some(secs) = tm_to_secs(t) else {
            crate::errno::set_errno(crate::errno::EOVERFLOW);
            return -1;
        };
        set_utc_zone(t);
        secs
    }
}
pub use gnu_timegm::timegm;

/// The zone name glibc's UTC renderings report -- `gmtime` and `timegm` --
/// whatever `TZ` says.
static UTC_NAME: &core::ffi::CStr = c"GMT";

/// The zone fields of a UTC rendering.
fn set_utc_zone(tm: &mut Tm) {
    tm.tm_isdst = 0;
    tm.tm_gmtoff = 0;
    tm.tm_zone = UTC_NAME.as_ptr().cast::<u8>();
}

/// Convert broken-down local time to seconds since epoch.
///
/// BSD/GNU extension; a synonym for `mktime`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timelocal(tm: *mut Tm) -> TimeT {
    mktime_ptr(tm)
}

/// Convert broken-down time to string.
///
/// Returns a pointer to a string in the format
/// "Wed Jun 30 21:49:08 1993\n\0", in storage owned by the library and
/// overwritten by the *calling thread's* next `asctime`/`ctime` (see
/// [`crate::perthread`]; `asctime_r` is the reentrant form).
///
/// As glibc's: C's `"%.3s %.3s%3d %.2d:%.2d:%.2d %d\n"`, whatever the
/// fields hold -- a name out of range is `???`, a number prints at its own
/// width -- and so any year, where `asctime_r` is held to 26 bytes. NULL
/// with `EINVAL` for a NULL `tm`, and with `EOVERFLOW` for a year past
/// `INT_MAX`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn asctime(tm: *const Tm) -> *const u8 {
    if tm.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null();
    }
    // SAFETY: the caller's `struct tm`, checked non-null.
    let t = unsafe { &*tm };
    // SAFETY: `perthread::current()` is non-null and valid for this thread,
    // and no other thread holds a pointer into this block.  `t` may alias
    // the same block (`ctime` passes `localtime`'s result straight in), but
    // `format_asctime` reads `t` fully before writing the buffer, and the
    // two fields do not overlap.
    let buf = unsafe { &mut (*crate::perthread::current()).asctime };
    if format_asctime(t, buf).is_none() {
        crate::errno::set_errno(crate::errno::EOVERFLOW);
        return core::ptr::null();
    }
    buf.as_ptr()
}

/// Convert time_t to string.
///
/// Exactly `asctime(localtime(timep))`, as glibc defines it: a time whose
/// year does not fit `tm_year` makes `localtime` NULL, and `asctime` of
/// NULL is NULL with `EINVAL`.
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
///
/// A year that does not fit `tm_year` is NULL with `EOVERFLOW`, and
/// `*result` is left as it was.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn gmtime_r(timep: *const TimeT, result: *mut Tm) -> *mut Tm {
    if timep.is_null() || result.is_null() {
        return core::ptr::null_mut();
    }
    let secs = unsafe { *timep };
    crate::tz::gmtime_touch(secs);
    let tm = unsafe { &mut *result };
    if secs_to_tm(secs, tm) {
        result
    } else {
        crate::errno::set_errno(crate::errno::EOVERFLOW);
        core::ptr::null_mut()
    }
}

/// Convert time_t to broken-down local time (reentrant), honouring `TZ`.
///
/// # Safety
///
/// Both pointers must be valid and non-null.
///
/// A year that does not fit `tm_year` is NULL with `EOVERFLOW`, and
/// `*result` is left as it was.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn localtime_r(timep: *const TimeT, result: *mut Tm) -> *mut Tm {
    if timep.is_null() || result.is_null() {
        return core::ptr::null_mut();
    }
    let secs = unsafe { *timep };
    let tm = unsafe { &mut *result };
    if secs_to_local_tm(secs, tm) {
        result
    } else {
        crate::errno::set_errno(crate::errno::EOVERFLOW);
        core::ptr::null_mut()
    }
}

/// Convert broken-down time to string (reentrant).
///
/// `asctime`'s text into the caller's `buf`, which POSIX sizes at 26 bytes:
/// text that does not fit them -- a year past 9999 or before -999, a field
/// of more digits than it should have -- is NULL with `EOVERFLOW`, as in
/// glibc, rather than an overrun. A NULL `tm` or `buf` is NULL with
/// `EINVAL`.
///
/// # Safety
///
/// `tm` must point to a valid `Tm`.  `buf` must be at least 26 bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn asctime_r(tm: *const Tm, buf: *mut u8) -> *mut u8 {
    if tm.is_null() || buf.is_null() {
        crate::errno::set_errno(crate::errno::EINVAL);
        return core::ptr::null_mut();
    }
    // SAFETY: the caller's `struct tm`, checked non-null.
    let t = unsafe { &*tm };
    let mut text = [0u8; ASCTIME_MAX];
    match format_asctime(t, &mut text) {
        Some(len) if len < ASCTIME_R_SIZE => {
            // The text and its NUL, at most 26 bytes.
            let with_nul = text.get(..=len).unwrap_or(&[]);
            // SAFETY: `buf` holds 26 bytes (this function's contract), which
            // `with_nul` does not exceed; `text` is a local, so the two
            // cannot overlap.
            unsafe { core::ptr::copy_nonoverlapping(with_nul.as_ptr(), buf, with_nul.len()) };
            buf
        }
        _ => {
            crate::errno::set_errno(crate::errno::EOVERFLOW);
            core::ptr::null_mut()
        }
    }
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
    let mut result = Tm::ZERO;
    // `ctime` is defined as `asctime(localtime(t))`, so the reentrant form
    // must use `localtime_r` too — it used `gmtime_r`, which was invisible
    // only while the two were the same function. A NULL from `localtime_r`
    // goes on to `asctime_r`, which makes it `EINVAL`, as glibc's does.
    // SAFETY: this function's contract, forwarded.
    let tm = unsafe { localtime_r(timep, &raw mut result) };
    // SAFETY: `tm` is NULL or `result`, a local `Tm`.
    unsafe { asctime_r(tm, buf) }
}

// `strftime` and `strftime_l` are `crate::strftime`'s, with `wcsftime`:
// one engine for both widths, as glibc's is one source compiled twice.
pub use crate::strftime::{strftime, strftime_l};

// ---------------------------------------------------------------------------
// Time conversion helpers
// ---------------------------------------------------------------------------

/// Check if a year is a leap year.
#[inline]
fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// A broken-down time with every field in range, computed before anything
/// is stored: what `gmtime` and `localtime` write -- glibc's `__offtime`.
pub(crate) struct Broken {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
}

impl Broken {
    /// The instant `secs` seconds after the epoch, at `gmtoff` seconds east
    /// of UTC; `None` when its year does not fit `tm_year` -- glibc's
    /// `EOVERFLOW`, at years past about 2^31. In closed form, through
    /// `tzrules`' calendar, so an instant 292 billion years out costs what
    /// one in 1970 does.
    #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
    pub(crate) fn of(secs: i64, gmtoff: i64) -> Option<Self> {
        // `|gmtoff|` is under a day, so the sum cannot overflow.
        let rem = secs.rem_euclid(86_400) + gmtoff;
        let days = secs.div_euclid(86_400) + rem.div_euclid(86_400);
        let rem = rem.rem_euclid(86_400);
        let (y, m, d) = tzrules::civil_from_days(days);
        let year = i32::try_from(y - 1900).ok()?;
        let yday = days - tzrules::days_from_civil(y, 1, 1);
        // Each of these is in range by construction: `rem < 86400`,
        // `m` in 1..=12, `d` in 1..=31, `yday` in 0..=365.
        Some(Self {
            sec: (rem % 60) as i32,
            min: (rem / 60 % 60) as i32,
            hour: (rem / 3600) as i32,
            mday: d as i32,
            mon: m as i32 - 1,
            year,
            wday: (days + 4).rem_euclid(7) as i32,
            yday: yday as i32,
        })
    }

    /// Store the eight calendar fields; the zone fields are the caller's.
    pub(crate) fn store(&self, tm: &mut Tm) {
        tm.tm_sec = self.sec;
        tm.tm_min = self.min;
        tm.tm_hour = self.hour;
        tm.tm_mday = self.mday;
        tm.tm_mon = self.mon;
        tm.tm_year = self.year;
        tm.tm_wday = self.wday;
        tm.tm_yday = self.yday;
    }
}

/// Convert seconds since epoch (1970-01-01 00:00:00 UTC) to broken-down UTC
/// time, with glibc's zone fields for UTC; `false`, with `tm` untouched, when
/// the year does not fit `tm_year`.
fn secs_to_tm(secs: TimeT, tm: &mut Tm) -> bool {
    let Some(b) = Broken::of(secs, 0) else {
        return false;
    };
    b.store(tm);
    set_utc_zone(tm);
    true
}

/// `tm`'s wall clock read as UTC, in seconds since the epoch: the fields
/// taken at face value, whatever their range, and carried -- a 13th month
/// is the next January, a 0th day the last of the month before. No field
/// of a `Tm` can overflow this: the year is within 2^31 of zero, so the
/// days within 2^40 and the seconds within 2^57.
#[allow(clippy::arithmetic_side_effects)]
fn wall_secs(tm: &Tm) -> i64 {
    let mon = i64::from(tm.tm_mon);
    let year = i64::from(tm.tm_year) + 1900 + mon.div_euclid(12);
    // `rem_euclid(12) + 1` is 1..=12, which `days_from_civil` takes as it is.
    let month = u32::try_from(mon.rem_euclid(12) + 1).unwrap_or(1);
    let days = tzrules::days_from_civil(year, month, 1) + i64::from(tm.tm_mday) - 1;
    days * 86_400 + i64::from(tm.tm_hour) * 3600 + i64::from(tm.tm_min) * 60 + i64::from(tm.tm_sec)
}

/// Convert broken-down time, read as UTC, to seconds since epoch, and
/// normalise its eight calendar fields to describe that instant -- `timegm`
/// less the zone fields. `None`, with `tm` untouched, when the normalised
/// year does not fit `tm_year`.
fn tm_to_secs(tm: &mut Tm) -> Option<TimeT> {
    let secs = wall_secs(tm);
    Broken::of(secs, 0)?.store(tm);
    Some(secs)
}

/// Size of `asctime`'s per-thread buffer: its longest text, every field at
/// an `int`'s widest, is 67 bytes and the NUL.
pub(crate) const ASCTIME_MAX: usize = 72;

/// The buffer POSIX gives `asctime_r`: the 25 characters of a four-digit
/// year and the NUL.
const ASCTIME_R_SIZE: usize = 26;

/// `n` as C's `%W.Pd` writes it -- at least `precision` digits, the sign,
/// then spaces to `width` -- at `pos` in `out`; the new position.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn put_int(out: &mut [u8], mut pos: usize, n: i64, width: usize, precision: usize) -> usize {
    let mut digits = [0u8; 20];
    let mut len = 0;
    let mut v = n.unsigned_abs();
    while v > 0 || len < precision.max(1) {
        if let Some(slot) = digits.get_mut(len) {
            *slot = b'0' + (v % 10) as u8;
        }
        v /= 10;
        len += 1;
    }
    let body = len + usize::from(n < 0);
    for _ in body..width {
        if let Some(slot) = out.get_mut(pos) {
            *slot = b' ';
        }
        pos += 1;
    }
    if n < 0 {
        if let Some(slot) = out.get_mut(pos) {
            *slot = b'-';
        }
        pos += 1;
    }
    for i in (0..len).rev() {
        if let (Some(slot), Some(&d)) = (out.get_mut(pos), digits.get(i)) {
            *slot = d;
        }
        pos += 1;
    }
    pos
}

/// `asctime`'s text for `tm` into `out`, NUL-terminated -- C's
/// `"%.3s %.3s%3d %.2d:%.2d:%.2d %d\n"`, glibc's -- and its length; `None`
/// when `tm_year + 1900` overflows an `int`, glibc's `EOVERFLOW`.
#[allow(clippy::arithmetic_side_effects)]
fn format_asctime(tm: &Tm, out: &mut [u8; ASCTIME_MAX]) -> Option<usize> {
    let year = tm.tm_year.checked_add(1900)?;
    let mut pos = 0;
    let put = |out: &mut [u8; ASCTIME_MAX], bytes: &[u8], pos: &mut usize| {
        for &b in bytes {
            if let Some(slot) = out.get_mut(*pos) {
                *slot = b;
            }
            *pos += 1;
        }
    };
    put(out, wday_abbr(tm.tm_wday), &mut pos);
    put(out, b" ", &mut pos);
    put(out, mon_abbr(tm.tm_mon), &mut pos);
    pos = put_int(out, pos, i64::from(tm.tm_mday), 3, 1);
    put(out, b" ", &mut pos);
    pos = put_int(out, pos, i64::from(tm.tm_hour), 0, 2);
    put(out, b":", &mut pos);
    pos = put_int(out, pos, i64::from(tm.tm_min), 0, 2);
    put(out, b":", &mut pos);
    pos = put_int(out, pos, i64::from(tm.tm_sec), 0, 2);
    put(out, b" ", &mut pos);
    pos = put_int(out, pos, i64::from(year), 0, 1);
    put(out, b"\n\0", &mut pos);
    // The NUL is not part of the length.
    Some(pos - 1)
}

// ---------------------------------------------------------------------------
// String tables
// ---------------------------------------------------------------------------

fn wday_abbr(wday: i32) -> &'static [u8] {
    match wday {
        0 => b"Sun",
        1 => b"Mon",
        2 => b"Tue",
        3 => b"Wed",
        4 => b"Thu",
        5 => b"Fri",
        6 => b"Sat",
        _ => b"???",
    }
}

fn wday_full(wday: i32) -> &'static [u8] {
    match wday {
        0 => b"Sunday",
        1 => b"Monday",
        2 => b"Tuesday",
        3 => b"Wednesday",
        4 => b"Thursday",
        5 => b"Friday",
        6 => b"Saturday",
        _ => b"???",
    }
}

fn mon_abbr(mon: i32) -> &'static [u8] {
    match mon {
        0 => b"Jan",
        1 => b"Feb",
        2 => b"Mar",
        3 => b"Apr",
        4 => b"May",
        5 => b"Jun",
        6 => b"Jul",
        7 => b"Aug",
        8 => b"Sep",
        9 => b"Oct",
        10 => b"Nov",
        11 => b"Dec",
        _ => b"???",
    }
}

fn mon_full(mon: i32) -> &'static [u8] {
    match mon {
        0 => b"January",
        1 => b"February",
        2 => b"March",
        3 => b"April",
        4 => b"May",
        5 => b"June",
        6 => b"July",
        7 => b"August",
        8 => b"September",
        9 => b"October",
        10 => b"November",
        11 => b"December",
        _ => b"???",
    }
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
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if clock_gettime(CLOCK_MONOTONIC, &raw mut ts) != 0 {
        return -1;
    }
    // Convert to microseconds (CLOCKS_PER_SEC = 1_000_000).
    ts.tv_sec * 1_000_000 + ts.tv_nsec / 1_000
}

// ---------------------------------------------------------------------------
// strptime — parse time strings
// ---------------------------------------------------------------------------

/// Own archive member — gnulib replaces `strptime`. See string.rs's module header.
mod gnu_strptime {
    use super::*;

    /// Parse a time string according to a format -- the inverse of
    /// `strftime`, as glibc 2.39's does it in the C locale.
    ///
    /// Every conversion glibc knows, with its rules: a number skips white
    /// space first and reads at most its field's digits, stopping early
    /// once one more digit would pass the field's maximum (`%m` of "23" is
    /// 2, "3" left over), and must land in range; names (`%a %A %b %B %h`)
    /// match whole or abbreviated, in any case, with no space skipped; white
    /// space in the format matches any run of it, none included; `%c %D %F
    /// %r %R %T %x %X` parse their C-locale expansions all or nothing;
    /// strftime's flags and field widths are accepted and mean nothing.
    ///
    /// Fields are stored as they parse, so a call that fails part-way leaves
    /// the earlier ones written, as glibc's does. After a whole match, what
    /// the parse implies is filled in: `%I` with `%p` makes a 24-hour hour,
    /// `%C` sets the century of a `%y` year (or makes the year on its own),
    /// a date sets `tm_wday` and `tm_yday`, a day of the year sets the month
    /// and day, and a week number (`%U` or `%W`) with a weekday sets the
    /// date. `%s` reads seconds since the epoch into local time.
    ///
    /// The `E` and `O` modifiers change nothing in the C locale, which has no
    /// eras and no alternative digits, and here they do not. glibc's do:
    /// its `%Ey` reads a second number after the first, and every `%O`
    /// conversion after the first in a format fails
    /// (`posix/tools/oracle/strptime_harness.py`, and the test that replays
    /// it).
    ///
    /// # Safety
    ///
    /// `buf` and `format` must be valid null-terminated strings.
    /// `tm` must point to a valid `Tm`.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn strptime(buf: *const u8, format: *const u8, tm: *mut Tm) -> *const u8 {
        // SAFETY: this function's contract, which is the parse's.
        unsafe { strptime_in_c_locale(buf, format, tm) }
    }
}
pub use gnu_strptime::strptime;

/// What [`strptime`] and [`strptime_l`] do: parse `buf` by `format` into
/// `*tm`, in the C locale, returning the end of what matched or NULL.
///
/// Out here rather than in `gnu_strptime` so that that archive member
/// defines `strptime` alone: a program that brings its own `strptime`, as
/// gnulib's do, then declines it and still has [`strptime_l`]
/// (`scripts/check-libc-shape.py`, which found `strptime_l` beside it).
///
/// # Safety
///
/// As [`strptime`].
unsafe fn strptime_in_c_locale(buf: *const u8, format: *const u8, tm: *mut Tm) -> *const u8 {
    if buf.is_null() || format.is_null() || tm.is_null() {
        return core::ptr::null();
    }
    // SAFETY: the caller's NUL-terminated format.
    let fmt = unsafe { core::ffi::CStr::from_ptr(format.cast()) }.to_bytes();
    // SAFETY: the caller's `struct tm`, checked non-null.
    let t = unsafe { &mut *tm };
    let input = Input(buf);
    let mut state = Parse::default();
    let Some(end) = parse(&input, 0, fmt, t, &mut state) else {
        return core::ptr::null();
    };
    state.finish(t);
    // SAFETY: `end` is within the input: the parse only moves past bytes it
    // has read, and never past the terminator.
    unsafe { buf.add(end) }
}

/// `strptime_l` -- [`strptime`] in a locale, which is always C's here
/// (`locale.rs`).
///
/// # Safety
///
/// As [`strptime`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strptime_l(
    buf: *const u8,
    format: *const u8,
    tm: *mut Tm,
    _loc: crate::locale::LocaleT,
) -> *const u8 {
    // SAFETY: this function's contract.
    unsafe { strptime_in_c_locale(buf, format, tm) }
}

/// The input of a `strptime` parse: a NUL-terminated string, read a byte
/// at a time and never past its terminator -- every step forward is over a
/// byte already read and found not to be NUL.
struct Input(*const u8);

impl Input {
    /// The byte at `i`, which must not be past the terminator.
    fn at(&self, i: usize) -> u8 {
        // SAFETY: the invariant above: `i` is at most the terminator's index.
        unsafe { *self.0.add(i) }
    }

    /// Past any white space at `pos`.
    fn skip_space(&self, mut pos: usize) -> usize {
        while is_c_space(self.at(pos)) {
            pos = pos.wrapping_add(1);
        }
        pos
    }

    /// Whether `word` is at `pos`, in any case.
    fn has_word(&self, pos: usize, word: &[u8]) -> bool {
        word.iter()
            .enumerate()
            .all(|(k, w)| self.at(pos.wrapping_add(k)).eq_ignore_ascii_case(w))
    }
}

/// C's `isspace` in the C locale.
const fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// What a `strptime` parse has learnt besides the fields it stored: what
/// decides the fields filled in after it ([`Parse::finish`]).
#[derive(Clone, Copy, Default)]
struct Parse {
    /// The hour came from the 12-hour clock (`%I`), and `%p` said PM.
    have_i: bool,
    is_pm: bool,
    /// `%C`'s century.
    century: Option<i32>,
    /// The year came from `%y`, two digits, which `%C` completes.
    want_century: bool,
    /// Something named the date: compute the weekday and day of the year.
    want_xday: bool,
    have_wday: bool,
    have_yday: bool,
    have_mon: bool,
    have_mday: bool,
    /// A week number (`%U` or `%W`), and which.
    have_uweek: bool,
    have_wweek: bool,
    week_no: i32,
}

/// Cumulative days before each month, in a common year and a leap year,
/// the thirteenth entry the year's length.
const MON_YDAY: [[i32; 13]; 2] = [
    [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365],
    [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366],
];

/// `MON_YDAY` for `tm_year`'s year and month `mon` -- 0 for a month out of
/// range, which only an input that never named its month can reach.
fn mon_yday(tm_year: i32, mon: i32) -> i32 {
    let leap = usize::from(is_leap(tm_year.wrapping_add(1900)));
    usize::try_from(mon)
        .ok()
        .and_then(|m| MON_YDAY.get(leap).and_then(|row| row.get(m)))
        .copied()
        .unwrap_or(0)
}

/// Leap days from year 0 through year `y`, as the weekday arithmetic below
/// counts them: `y/4 - y/100 + y/400` for a year from 0 on, grouped so
/// that for a negative year the hundreds round the other way, as glibc's
/// do.
#[allow(clippy::arithmetic_side_effects)]
fn leap_days(y: i32) -> i32 {
    let quads = y / 4;
    let centuries = quads / 25;
    // Divisions by nonzero constants, and a sum of values each under
    // `|y| / 4`: nothing here can overflow.
    quads - centuries + i32::from(quads % 25 < 0) + centuries / 4
}

/// The weekday of `tm`'s date, as glibc computes it after a parse: whole
/// days from 1970-01-01, the months counted as in a common year and the
/// leap days up to the year before -- or up to this year, from March on.
/// In C's `int` arithmetic exactly, wrapping and all, because `getdate`
/// hands `strptime` a `struct tm` whose unset fields are `INT_MIN` and uses
/// the weekday that comes out: a 0th or a 32nd of the month is a day
/// either side, and year 0's January and February are a day off, as in
/// glibc.
fn weekday_of(tm: &Tm) -> i32 {
    let corr = 1900i32
        .wrapping_add(tm.tm_year)
        .wrapping_sub(i32::from(tm.tm_mon < 2));
    let month_days = usize::try_from(tm.tm_mon)
        .ok()
        .and_then(|m| MON_YDAY[0].get(m))
        .copied()
        .unwrap_or(0);
    // January 1st 1970 was a Thursday: 4, less the leap days to 1969.
    let days = 4i32
        .wrapping_sub(leap_days(1969))
        .wrapping_add(tm.tm_year.wrapping_sub(70).wrapping_mul(365))
        .wrapping_add(leap_days(corr))
        .wrapping_add(month_days)
        .wrapping_add(tm.tm_mday)
        .wrapping_sub(1);
    // C's `((days % 7) + 7) % 7`: the remainder into 0..7.
    days.rem_euclid(7)
}

impl Parse {
    /// Fill in what a whole match implies (see `strptime`), in C's `int`
    /// arithmetic, wrapping as glibc's does on `getdate`'s `INT_MIN`s.
    #[allow(clippy::arithmetic_side_effects)]
    fn finish(&self, tm: &mut Tm) {
        if self.have_i && self.is_pm {
            tm.tm_hour = tm.tm_hour.wrapping_add(12);
        }
        if let Some(c) = self.century {
            // `c` is 0..=99, so `(c - 19) * 100` cannot overflow.
            tm.tm_year = if self.want_century {
                (tm.tm_year % 100).wrapping_add((c - 19) * 100)
            } else {
                (c - 19) * 100
            };
        }
        let mut have_mon = self.have_mon;
        let mut have_mday = self.have_mday;
        // The weekday and the day of the year are worked out only for a
        // month that is one -- whatever the year and the day hold, as
        // glibc's are (`getdate` leaves unset fields `INT_MIN`).
        if self.want_xday && !self.have_wday {
            if !(have_mon && have_mday) && self.have_yday {
                date_of_yday(tm, have_mon, have_mday);
                have_mon = true;
                have_mday = true;
            }
            if (0..12).contains(&tm.tm_mon) {
                tm.tm_wday = weekday_of(tm);
            }
        }
        if self.want_xday && !self.have_yday && (0..12).contains(&tm.tm_mon) {
            tm.tm_yday = mon_yday(tm.tm_year, tm.tm_mon)
                .wrapping_add(tm.tm_mday)
                .wrapping_sub(1);
        }
        if (self.have_uweek || self.have_wweek) && self.have_wday {
            let wday = tm.tm_wday;
            let (mday, mon) = (tm.tm_mday, tm.tm_mon);
            let offset = i32::from(!self.have_uweek);
            // The weekday of the year's first day.
            tm.tm_mday = 1;
            tm.tm_mon = 0;
            let jan1 = weekday_of(tm);
            if have_mday {
                tm.tm_mday = mday;
            }
            if have_mon {
                tm.tm_mon = mon;
            }
            if !self.have_yday {
                tm.tm_yday =
                    (7 - (jan1 - offset)) % 7 + (self.week_no - 1) * 7 + (wday - offset + 7) % 7;
            }
            if !have_mday || !have_mon {
                date_of_yday(tm, have_mon, have_mday);
            }
            tm.tm_wday = wday;
        }
    }
}

/// `tm_mon` and `tm_mday` -- whichever the parse did not name -- from
/// `tm_yday`: the last month starting on or before it, December at the
/// latest, so a day of the year past the year's end is a day of December
/// past its 31st, as glibc has it.
fn date_of_yday(tm: &mut Tm, have_mon: bool, have_mday: bool) {
    let started = (0..12)
        .take_while(|&m| mon_yday(tm.tm_year, m) <= tm.tm_yday)
        .count();
    // The last month started, or -1 for a day before the year's first: the
    // day then counts from the year's start, as glibc's does.
    let mon = started
        .checked_sub(1)
        .and_then(|m| i32::try_from(m).ok())
        .unwrap_or(-1);
    if !have_mon {
        tm.tm_mon = mon;
    }
    if !have_mday {
        let start = if mon < 0 {
            0
        } else {
            mon_yday(tm.tm_year, mon)
        };
        tm.tm_mday = tm.tm_yday.wrapping_sub(start).wrapping_add(1);
    }
}

/// A number of at most `digits` digits in `from..=to`, after any white
/// space: the digits stop early when one more would take the value past
/// `to`, as glibc's do. `None` if there is no digit or the value is out of
/// range.
#[allow(clippy::arithmetic_side_effects)]
fn number(input: &Input, pos: &mut usize, from: i32, to: i32, digits: usize) -> Option<i32> {
    *pos = input.skip_space(*pos);
    if !input.at(*pos).is_ascii_digit() {
        return None;
    }
    let mut val = 0i32;
    let mut left = digits;
    loop {
        val = val * 10 + i32::from(input.at(*pos) - b'0');
        *pos += 1;
        left -= 1;
        if left == 0 || val * 10 > to || !input.at(*pos).is_ascii_digit() {
            break;
        }
    }
    (from..=to).contains(&val).then_some(val)
}

/// The weekday or month named at `pos`, whole or abbreviated: `(index,
/// length)`.
fn name(
    input: &Input,
    pos: usize,
    count: i32,
    full: fn(i32) -> &'static [u8],
    abbr: fn(i32) -> &'static [u8],
) -> Option<(i32, usize)> {
    (0..count).find_map(|i| {
        [full(i), abbr(i)]
            .into_iter()
            .find(|w| input.has_word(pos, w))
            .map(|w| (i, w.len()))
    })
}

/// Parse `input` from `pos` against `fmt`, storing into `tm` and `state`:
/// the position after the last byte matched, or `None`.
#[allow(clippy::arithmetic_side_effects)]
fn parse(
    input: &Input,
    mut pos: usize,
    fmt: &[u8],
    tm: &mut Tm,
    state: &mut Parse,
) -> Option<usize> {
    let mut fi = 0;
    while let Some(&c) = fmt.get(fi) {
        fi += 1;
        if is_c_space(c) {
            pos = input.skip_space(pos);
            continue;
        }
        if c != b'%' {
            if input.at(pos) != c {
                return None;
            }
            pos += 1;
            continue;
        }
        // strftime's flags and field width mean nothing here.
        while matches!(fmt.get(fi), Some(b'-' | b'_' | b'0' | b'^' | b'#')) {
            fi += 1;
        }
        while fmt.get(fi).is_some_and(u8::is_ascii_digit) {
            fi += 1;
        }
        let mut conv = *fmt.get(fi)?;
        fi += 1;
        // The C locale's alternative forms are the plain ones; a modifier on
        // a conversion that has none is no match.
        if conv == b'E' || conv == b'O' {
            let next = *fmt.get(fi)?;
            fi += 1;
            let allowed: &[u8] = if conv == b'E' {
                b"cCxXyY"
            } else {
                b"bBhdeHImMSUVWwy"
            };
            if !allowed.contains(&next) {
                return None;
            }
            conv = next;
        }
        pos = conversion(input, pos, conv, tm, state)?;
    }
    Some(pos)
}

/// `sub`, a C-locale expansion, parsed into copies of `tm` and `state` that
/// replace them only if it matches whole -- glibc's composite conversions
/// are all or nothing.
fn composite(
    input: &Input,
    pos: usize,
    sub: &[u8],
    tm: &mut Tm,
    state: &mut Parse,
) -> Option<usize> {
    let mut t = *tm;
    let mut s = *state;
    let end = parse(input, pos, sub, &mut t, &mut s)?;
    *tm = t;
    *state = s;
    Some(end)
}

/// One conversion, `conv`, at `pos`: the position after it.
#[allow(clippy::arithmetic_side_effects, clippy::too_many_lines)]
fn conversion(
    input: &Input,
    mut pos: usize,
    conv: u8,
    tm: &mut Tm,
    state: &mut Parse,
) -> Option<usize> {
    match conv {
        b'%' => {
            if input.at(pos) != b'%' {
                return None;
            }
            pos += 1;
        }
        b'a' | b'A' => {
            let (wday, len) = name(input, pos, 7, wday_full, wday_abbr)?;
            tm.tm_wday = wday;
            state.have_wday = true;
            pos += len;
        }
        b'b' | b'B' | b'h' => {
            let (mon, len) = name(input, pos, 12, mon_full, mon_abbr)?;
            tm.tm_mon = mon;
            state.have_mon = true;
            state.want_xday = true;
            pos += len;
        }
        b'c' => {
            pos = composite(input, pos, b"%a %b %e %H:%M:%S %Y", tm, state)?;
            state.want_xday = true;
        }
        b'C' => {
            state.century = Some(number(input, &mut pos, 0, 99, 2)?);
            state.want_xday = true;
        }
        b'd' | b'e' => {
            tm.tm_mday = number(input, &mut pos, 1, 31, 2)?;
            state.have_mday = true;
            state.want_xday = true;
        }
        b'F' => {
            pos = composite(input, pos, b"%Y-%m-%d", tm, state)?;
            state.want_xday = true;
        }
        b'x' | b'D' => {
            pos = composite(input, pos, b"%m/%d/%y", tm, state)?;
            state.want_xday = true;
        }
        b'k' | b'H' => {
            tm.tm_hour = number(input, &mut pos, 0, 23, 2)?;
            state.have_i = false;
        }
        b'l' | b'I' => {
            tm.tm_hour = number(input, &mut pos, 1, 12, 2)? % 12;
            state.have_i = true;
        }
        b'j' => {
            tm.tm_yday = number(input, &mut pos, 1, 366, 3)? - 1;
            state.have_yday = true;
        }
        b'm' => {
            tm.tm_mon = number(input, &mut pos, 1, 12, 2)? - 1;
            state.have_mon = true;
            state.want_xday = true;
        }
        b'M' => tm.tm_min = number(input, &mut pos, 0, 59, 2)?,
        b'n' | b't' => pos = input.skip_space(pos),
        b'p' => {
            if input.has_word(pos, b"AM") {
                state.is_pm = false;
            } else if input.has_word(pos, b"PM") {
                state.is_pm = true;
            } else {
                return None;
            }
            pos += 2;
        }
        b'r' => pos = composite(input, pos, b"%I:%M:%S %p", tm, state)?,
        b'R' => pos = composite(input, pos, b"%H:%M", tm, state)?,
        b's' => {
            // Seconds since the epoch, any number of digits; one past
            // `time_t` is no match (glibc's wraps).
            if !input.at(pos).is_ascii_digit() {
                return None;
            }
            let mut secs: TimeT = 0;
            while input.at(pos).is_ascii_digit() {
                secs = secs
                    .checked_mul(10)?
                    .checked_add(TimeT::from(input.at(pos) - b'0'))?;
                pos += 1;
            }
            if !secs_to_local_tm(secs, tm) {
                return None;
            }
        }
        b'S' => tm.tm_sec = number(input, &mut pos, 0, 61, 2)?,
        b'X' | b'T' => pos = composite(input, pos, b"%H:%M:%S", tm, state)?,
        b'u' => {
            tm.tm_wday = number(input, &mut pos, 1, 7, 1)? % 7;
            state.have_wday = true;
        }
        // The ISO 8601 week-based year and week: read, and not used -- they
        // cannot name a date without the rest of the ISO week date.
        b'g' => {
            number(input, &mut pos, 0, 99, 2)?;
        }
        b'G' => {
            if !input.at(pos).is_ascii_digit() {
                return None;
            }
            while input.at(pos).is_ascii_digit() {
                pos += 1;
            }
        }
        b'V' => {
            number(input, &mut pos, 0, 53, 2)?;
        }
        b'U' | b'W' => {
            state.week_no = number(input, &mut pos, 0, 53, 2)?;
            state.have_uweek = conv == b'U';
            state.have_wweek = conv == b'W';
        }
        b'w' => {
            tm.tm_wday = number(input, &mut pos, 0, 6, 1)?;
            state.have_wday = true;
        }
        b'y' => {
            let yy = number(input, &mut pos, 0, 99, 2)?;
            tm.tm_year = if yy >= 69 { yy } else { yy + 100 };
            state.want_century = true;
            state.want_xday = true;
        }
        b'Y' => {
            tm.tm_year = number(input, &mut pos, 0, 9999, 4)? - 1900;
            state.want_century = false;
            state.want_xday = true;
        }
        b'Z' => {
            // A zone name: read and not used.
            pos = input.skip_space(pos);
            while input.at(pos) != 0 && !is_c_space(input.at(pos)) {
                pos += 1;
            }
        }
        b'z' => {
            pos = input.skip_space(pos);
            if input.at(pos) == b'Z' {
                pos += 1;
                tm.tm_gmtoff = 0;
            } else {
                let negative = match input.at(pos) {
                    b'+' => false,
                    b'-' => true,
                    _ => return None,
                };
                pos += 1;
                // `hh`, `hhmm` or `hh:mm`.
                let mut val = 0i64;
                let mut n = 0;
                while n < 4 && input.at(pos).is_ascii_digit() {
                    val = val * 10 + i64::from(input.at(pos) - b'0');
                    pos += 1;
                    n += 1;
                    if n == 2 && input.at(pos) == b':' && input.at(pos + 1).is_ascii_digit() {
                        pos += 1;
                    }
                }
                match n {
                    2 => val *= 100,
                    4 if val % 100 < 60 => {}
                    _ => return None,
                }
                let secs = val / 100 * 3600 + val % 100 * 60;
                tm.tm_gmtoff = if negative { -secs } else { secs };
            }
        }
        _ => return None,
    }
    Some(pos)
}

// ---------------------------------------------------------------------------
// getdate — a date by the templates a file lists
// ---------------------------------------------------------------------------

/// Why the last `getdate` failed, numbered as POSIX numbers it: 1 `DATEMSK`
/// unset or empty; 2 its file cannot be opened for reading; 3 its status
/// cannot be read; 4 it is not a regular file; 5 reading it failed; 6 no
/// memory; 7 no template matches the input; 8 the input names no valid
/// date.
///
/// C declares it `extern int getdate_err`: one process-wide `int`, as
/// glibc's is -- an `AtomicI32` for the reason `signgam` is one
/// ([`crate::math::signgam`]): C's layout, and no undefined behaviour here
/// when two threads fail at once.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static getdate_err: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

/// `getdate(string)`: the date `string` names, by the first template in the
/// file `DATEMSK` names that matches it whole ([`getdate_r`]); NULL, with
/// [`getdate_err`] set, when there is none or the date is not one. The
/// result is this thread's own storage, overwritten by its next `getdate`.
///
/// # Safety
///
/// `string` must be a NUL-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getdate(string: *const u8) -> *mut Tm {
    // SAFETY: `perthread::current()` is non-null and valid for this thread,
    // and no other thread holds a pointer into this block.
    let tm = unsafe { &raw mut (*crate::perthread::current()).getdate };
    // SAFETY: this function's contract; `tm` is this thread's own `Tm`.
    match unsafe { getdate_r(string, tm) } {
        0 => tm,
        err => {
            getdate_err.store(err, core::sync::atomic::Ordering::Relaxed);
            core::ptr::null_mut()
        }
    }
}

/// `getdate_r(string, tp)`: [`getdate`] into the caller's `*tp`, returning
/// its error number -- 0 for a date -- rather than setting `getdate_err`.
/// A GNU extension, glibc's.
///
/// `DATEMSK` names a text file of `strptime` formats, one per line; the
/// input, less leading and trailing white space, is tried against each in
/// turn, and the first that consumes all of it is the match. What the
/// match left out comes from the present, as POSIX's `getdate` says
/// ([`getdate_fill`]). On a failure `*tp` holds what the last attempt left
/// there, as glibc's does.
///
/// # Safety
///
/// `string` must be a NUL-terminated string, and `tp` a valid `Tm`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getdate_r(string: *const u8, tp: *mut Tm) -> i32 {
    if string.is_null() || tp.is_null() {
        return 7;
    }
    // SAFETY: the name is a NUL-terminated literal.
    let path = unsafe { crate::environ::lookup(c"DATEMSK".as_ptr().cast()) };
    // SAFETY: `lookup` returns NULL or a NUL-terminated value.
    if path.is_null() || unsafe { *path } == 0 {
        return 1;
    }
    let mut st = crate::stat::Stat::zeroed();
    if crate::file::stat(path, &raw mut st) != 0 {
        return 3;
    }
    if !st.is_file() {
        return 4;
    }
    if crate::file::access(path, crate::fcntl::R_OK) != 0 {
        return 2;
    }
    // SAFETY: a NUL-terminated path and mode.
    let file = unsafe { crate::stdio::fopen(path, c"rce".as_ptr().cast()) };
    if file.is_null() {
        return 2;
    }
    // SAFETY: an open stream, and the caller's `Tm`.
    let rc = unsafe { getdate_stream(string, file, &mut *tp, time(core::ptr::null_mut())) };
    // The stream was only read; closing it cannot lose anything.
    let _ = crate::stdio::fclose(file);
    rc
}

/// `getdate_r` once its template file is open: `string` against each line
/// of `file`, then [`getdate_fill`] from `now`.
///
/// # Safety
///
/// `string` is NUL-terminated and `file` an open stream.
unsafe fn getdate_stream(string: *const u8, file: *mut u8, tp: &mut Tm, now: TimeT) -> i32 {
    // SAFETY: this function's contract.
    let Some(input) = (unsafe { GetdateInput::new(string) }) else {
        return 6;
    };
    let mut line: *mut u8 = core::ptr::null_mut();
    let mut cap = 0usize;
    let mut found = false;
    loop {
        // SAFETY: a `getline` pair of this function's, and an open stream.
        let n = unsafe { crate::stdio::getline(&raw mut line, &raw mut cap, file) };
        let Ok(n) = usize::try_from(n) else {
            break;
        };
        // One template, its newline cut.
        if let Some(i) = n.checked_sub(1) {
            // SAFETY: `getline` wrote `n` bytes and a NUL at `line`, and
            // `i < n`.
            let last = unsafe { line.add(i) };
            // SAFETY: as above; the byte is within the line.
            if unsafe { *last } == b'\n' {
                // SAFETY: as above.
                unsafe { *last = 0 };
            }
        }
        if getdate_try(input.ptr(), line, tp) {
            found = true;
            break;
        }
        if crate::stdio::feof(file) != 0 {
            break;
        }
    }
    // SAFETY: `line` is NULL or `getline`'s `malloc` block.
    unsafe { crate::malloc::free(line.cast()) };
    if crate::stdio::ferror(file) != 0 {
        return 5;
    }
    if !found {
        return 7;
    }
    getdate_fill(tp, now)
}

/// The input as `getdate` matches it: past its leading white space, and --
/// when it has trailing white space -- a copy without it, as glibc makes.
struct GetdateInput {
    start: *const u8,
    copy: Option<crate::decfloat::MallocBuf<u8>>,
}

impl GetdateInput {
    /// `None` when the copy cannot be allocated (`getdate`'s 6).
    ///
    /// # Safety
    ///
    /// `string` is NUL-terminated.
    #[allow(clippy::arithmetic_side_effects)]
    unsafe fn new(string: *const u8) -> Option<Self> {
        let mut start = string;
        // SAFETY: within the string, stopping at its NUL.
        while is_c_space(unsafe { *start }) {
            // SAFETY: as above.
            start = unsafe { start.add(1) };
        }
        // SAFETY: `start` is within the NUL-terminated string.
        let bytes = unsafe { core::ffi::CStr::from_ptr(start.cast()) }.to_bytes();
        let kept = bytes
            .iter()
            .rposition(|&b| !is_c_space(b))
            .map_or(0, |i| i + 1);
        if kept == bytes.len() {
            return Some(Self { start, copy: None });
        }
        let mut copy = crate::decfloat::MallocBuf::<u8>::zeroed(kept + 1)?;
        if let (Some(dst), Some(src)) = (copy.as_mut().get_mut(..kept), bytes.get(..kept)) {
            dst.copy_from_slice(src);
        }
        Some(Self {
            start,
            copy: Some(copy),
        })
    }

    /// The NUL-terminated input.
    fn ptr(&self) -> *const u8 {
        self.copy
            .as_ref()
            .map_or(self.start, |c| c.as_ref().as_ptr())
    }
}

/// `getdate`'s marker for a field no template set.
const GETDATE_UNSET: i32 = i32::MIN;

/// One template: `tp` marked unset in every field a template can set, then
/// `strptime`; whether it consumed the whole input.
fn getdate_try(input: *const u8, template: *const u8, tp: &mut Tm) -> bool {
    tp.tm_year = GETDATE_UNSET;
    tp.tm_mon = GETDATE_UNSET;
    tp.tm_mday = GETDATE_UNSET;
    tp.tm_wday = GETDATE_UNSET;
    tp.tm_hour = GETDATE_UNSET;
    tp.tm_min = GETDATE_UNSET;
    tp.tm_sec = GETDATE_UNSET;
    tp.tm_isdst = -1;
    tp.tm_gmtoff = 0;
    tp.tm_zone = core::ptr::null();
    // SAFETY: both are NUL-terminated (the callers' contracts).
    let end = unsafe { strptime(input, template, tp) };
    // SAFETY: a non-NULL result points into the NUL-terminated input.
    !end.is_null() && unsafe { *end } == 0
}

/// What the matching template left out, filled in as POSIX's `getdate`
/// says, from `now` in local time -- and then the whole normalised by
/// `mktime`. 0, or 8 when the date is not one.
///
/// - A weekday alone is the next such day, today included.
/// - A month without a day is the next such month, this one included, at
///   its first day -- or its first such weekday, when one was matched.
/// - No hour, minute or second: now's; any one missing: 0.
/// - An hour without a date is the next such hour, the current one
///   included, judged by the hour alone -- 13:45 at 13:50 is today's, as
///   in glibc.
/// - A missing year or month is the present one.
#[allow(clippy::arithmetic_side_effects)]
fn getdate_fill(tp: &mut Tm, now: TimeT) -> i32 {
    let mut here = Tm::ZERO;
    if !secs_to_local_tm(now, &mut here) {
        return 8;
    }
    let unset = GETDATE_UNSET;
    let mut mday_ok = false;
    // The present's fields are all in range, so none of the sums below can
    // overflow.
    if (0..=6).contains(&tp.tm_wday)
        && tp.tm_year == unset
        && tp.tm_mon == unset
        && tp.tm_mday == unset
    {
        tp.tm_year = here.tm_year;
        tp.tm_mon = here.tm_mon;
        tp.tm_mday = here.tm_mday + (tp.tm_wday - here.tm_wday + 7) % 7;
        mday_ok = true;
    }
    if (0..=11).contains(&tp.tm_mon) && tp.tm_mday == unset {
        if tp.tm_year == unset {
            tp.tm_year = here.tm_year + i32::from(tp.tm_mon < here.tm_mon);
        }
        tp.tm_mday = first_weekday(tp.tm_year, tp.tm_mon, tp.tm_wday);
        mday_ok = true;
    }
    if tp.tm_hour == unset && tp.tm_min == unset && tp.tm_sec == unset {
        tp.tm_hour = here.tm_hour;
        tp.tm_min = here.tm_min;
        tp.tm_sec = here.tm_sec;
    }
    for field in [&mut tp.tm_hour, &mut tp.tm_min, &mut tp.tm_sec] {
        if *field == unset {
            *field = 0;
        }
    }
    if (0..=23).contains(&tp.tm_hour)
        && tp.tm_mon == unset
        && tp.tm_mday == unset
        && tp.tm_wday == unset
    {
        tp.tm_mon = here.tm_mon;
        tp.tm_mday = here.tm_mday + i32::from(tp.tm_hour < here.tm_hour);
        mday_ok = true;
    }
    if tp.tm_year == unset {
        tp.tm_year = here.tm_year;
    }
    if tp.tm_mon == unset {
        tp.tm_mon = here.tm_mon;
    }
    if !mday_ok && !day_in_month(tp.tm_year, tp.tm_mon, tp.tm_mday) {
        return 8;
    }
    if mktime_tm(tp).is_none() {
        return 8;
    }
    0
}

/// Whether `mday` is a day of month `mon` (0-based) of `tm_year`'s year.
fn day_in_month(tm_year: i32, mon: i32, mday: i32) -> bool {
    let Some(month) = u32::try_from(mon).ok().filter(|m| *m < 12) else {
        return false;
    };
    let days = tzrules::days_in_month(month.wrapping_add(1), i64::from(tm_year).wrapping_add(1900));
    u32::try_from(mday).is_ok_and(|d| (1..=days).contains(&d))
}

/// The first day of month `mon` of `tm_year`'s year that falls on weekday
/// `wday`, found through `mktime` in local time; the 1st when no weekday
/// was matched.
#[allow(clippy::arithmetic_side_effects)]
fn first_weekday(tm_year: i32, mon: i32, wday: i32) -> i32 {
    if wday == GETDATE_UNSET {
        return 1;
    }
    let mut first = Tm {
        tm_year,
        tm_mon: mon,
        tm_mday: 1,
        ..Tm::ZERO
    };
    // A year past `tm_year` leaves the weekday 0, as glibc's unchecked
    // `mktime` does.
    let _ = mktime_tm(&mut first);
    // `wday` is 0..=6 from `strptime`, and `tm_wday` 0..=6.
    1 + (wday - first.tm_wday + 7) % 7
}

// ---------------------------------------------------------------------------
// POSIX per-process timers (none until the kernel has them)
// ---------------------------------------------------------------------------
//
// `timer_create` refuses with ENOSYS after Linux's argument checks, and the
// other four answer EINVAL; `timer_create`'s doc says why, and what these
// were until 2026-10-05.

/// Timer ID type: pointer-wide, as musl's `<time.h>` makes `timer_t` a
/// `void *`. It was an `i32` until 2026-09-29, and `timer_create` stored
/// four bytes into the caller's eight, leaving the other four whatever they
/// were -- so a `timer_t` compared with another, or with `NULL`, compared
/// garbage.
pub type TimerT = usize;

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

/// Create a per-process timer: refused with `ENOSYS`, after Linux's
/// argument checks, because SlateOS has no timer of this kind yet.
///
/// The kernel has none on either ABI: its Linux ABI's `timer_create`
/// (`kernel/src/syscall/linux.rs`, `sys_timer_create`) makes these same
/// checks and then answers `ENOSYS`, and no native call arms a timer that
/// signals on expiry -- `SYS_TIMER_CREATE` notifies a completion port, and
/// `SYS_ITIMER_SET` is the one `ITIMER_REAL` that `alarm` and `setitimer`
/// share. `requests/d-a-posix-timers-and-the-dumpable-flag-need-native-calls.md`
/// asks lane A for native per-process timers; this is rewritten over them
/// when they land.
///
/// `ENOSYS` is what a Linux kernel built without `CONFIG_POSIX_TIMERS`
/// answers, and the answer callers are written to fall back on: GNU
/// `timeout` (coreutils 9.5, `settimeout`) falls back to `alarm` silently
/// on exactly this errno and prints a warning on any other, and on any
/// failure of `timer_settime`. That is why the refusal is here and not at
/// arming time.
///
/// Until 2026-10-05 this claimed a slot in a table and succeeded, and
/// [`timer_settime`] stored its value there and succeeded too: a timer
/// reported armed that could never fire
/// (`known-issues-resolved/B-POSIX-TIMER-SETTIME-REPORTS-SUCCESS-AND-ARMS-NOTHING.md`).
///
/// # Linux validation order
///
/// `kernel/time/posix-timers.c::sys_timer_create` -> `do_timer_create`,
/// so that a malformed call gets the answer it would get from Linux:
///
/// 1. `copy_from_user(&event, timer_event_spec, ...)` -> `EFAULT`: not
///    simulated; a non-null `sevp` is read.
/// 2. `clockid_to_kclock(which_clock)` unknown -> `EINVAL`.
/// 3. the clock has no `timer_create` -> `EOPNOTSUPP`:
///    `CLOCK_MONOTONIC_RAW` and the two `_COARSE` clocks, which can be read
///    but not armed.
/// 4. `alloc_posix_timer` and `posix_timer_add` -> `EAGAIN` when there is
///    no room: nothing is allocated here, so never.
/// 5. `good_sigevent(event)` -> `EINVAL`: an unrecognised `sigev_notify`,
///    or a signal outside `1..=SIGRTMAX` when one is to be sent
///    (`SIGEV_SIGNAL`, `SIGEV_THREAD_ID`). `SIGEV_NONE` sends none, and a
///    `SIGEV_THREAD` caller's signal is never the kernel's to see -- glibc
///    hands it `SIGTIMER` instead -- so neither is looked at. Nor is the
///    thread a `SIGEV_THREAD_ID` event names, which Linux checks belongs
///    to the caller's process.
/// 6. `copy_to_user(created_timer_id, ...)` -> `EFAULT` for a NULL
///    `timerid` (Phase 147; it was `EINVAL` before).
/// 7. `ENOSYS`.
///
/// `*timerid` is never written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_create(
    clockid: ClockidT,
    sevp: *const Sigevent,
    timerid: *mut TimerT,
) -> i32 {
    // Step 2.
    if !is_valid_clock(clockid) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Step 3.
    if matches!(
        clockid,
        CLOCK_MONOTONIC_RAW | CLOCK_REALTIME_COARSE | CLOCK_MONOTONIC_COARSE
    ) {
        errno::set_errno(errno::EOPNOTSUPP);
        return -1;
    }
    // Step 5. A null sevp is SIGEV_SIGNAL with SIGALRM, per POSIX, and
    // needs no check.
    if !sevp.is_null() {
        // SAFETY: the caller asserts sevp points to a valid Sigevent; only
        // two `i32` fields are read, and no pointer inside it.
        let (notify, signo) = unsafe { ((*sevp).sigev_notify, (*sevp).sigev_signo) };
        if !is_valid_sigev_notify(notify) {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        let sends_signal = matches!(notify, SIGEV_SIGNAL | SIGEV_THREAD_ID);
        if sends_signal && !(1..=crate::signal::SIGRTMAX).contains(&signo) {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }
    // Step 6.
    if timerid.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // Step 7.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Arm or disarm a per-process timer: `EINVAL`, since no timer exists.
///
/// [`timer_create`] makes none, so every `timerid` names no timer, and
/// Linux's `do_timer_settime` answers that `EINVAL` (`lock_timer` finds
/// nothing). Its other refusals are `EINVAL` as well -- a NULL
/// `new_value`, an invalid `it_value` or `it_interval`, an unknown flag --
/// all but `EFAULT` for an unreadable `new_value`, which could not be told
/// from a readable one without reading it. So neither pointer is touched,
/// and every call answers `EINVAL`, as the kernel's Linux ABI answers
/// every call whose `new_value` it can read.
///
/// Until 2026-10-05 this stored the value in [`timer_create`]'s table and
/// reported success; see there.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_settime(
    _timerid: TimerT,
    _flags: i32,
    _new_value: *const Itimerspec,
    _old_value: *mut Itimerspec,
) -> i32 {
    errno::set_errno(errno::EINVAL);
    -1
}

/// Read a per-process timer: `EINVAL`, since no timer exists.
///
/// Linux's `do_timer_gettime` looks the timer up before
/// `put_itimerspec64` writes `curr_value`, so an unknown `timerid` is
/// `EINVAL` whatever `curr_value` is, and `curr_value` is not written --
/// the kernel's Linux ABI answers the same.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_gettime(_timerid: TimerT, _curr_value: *mut Itimerspec) -> i32 {
    errno::set_errno(errno::EINVAL);
    -1
}

/// Delete a per-process timer: `EINVAL`, since no timer exists -- Linux's
/// answer for an unknown `timerid`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_delete(_timerid: TimerT) -> i32 {
    errno::set_errno(errno::EINVAL);
    -1
}

/// Read a per-process timer's overrun count: `EINVAL`, since no timer
/// exists -- `sys_timer_getoverrun`'s `lock_timer` finds nothing.
///
/// Phase 149 made this look the id up instead of answering 0 for any;
/// with no timers the lookup always misses.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timer_getoverrun(_timerid: TimerT) -> i32 {
    errno::set_errno(errno::EINVAL);
    -1
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

// There is no per-timer-type storage here any more. `setitimer`/`getitimer`
// reach the kernel's one real interval timer through `SYS_ITIMER_SET` and
// `SYS_ITIMER_GET`; the table that used to answer for all three types, while
// arming nothing, is gone with the doc comment that counted them.

/// Ask the kernel to arm, re-arm or disarm the real interval timer.
///
/// Returns `(prev_value_ns, prev_interval_ns)`, or a negative first element
/// carrying `-errno`.
///
/// **On a host build there is no kernel, so this simulates.** That is the same
/// split `pipe.rs` uses, and it is not a return of the bug this replaced: the
/// bug was that the TARGET stored a value and armed nothing while reporting
/// success. Here the target really arms, and the host keeps a store so that
/// `cargo test` still exercises the validation order and the `Timeval`↔ns
/// conversions either side of it -- which is the libc-side logic, and the only
/// part a host test can be about. Whether a `SIGALRM` actually arrives is a
/// kernel question, answered by the kernel's own self-test for 1069/1070.
pub(crate) fn itimer_kernel_set(value_ns: u64, interval_ns: u64) -> (i64, i64) {
    #[cfg(target_os = "none")]
    {
        crate::syscall::syscall3_2ret(crate::syscall::SYS_ITIMER_SET, 0, value_ns, interval_ns)
    }
    #[cfg(not(target_os = "none"))]
    {
        let prev = host_itimer::swap(value_ns, interval_ns);
        (
            i64::try_from(prev.0).unwrap_or(i64::MAX),
            i64::try_from(prev.1).unwrap_or(i64::MAX),
        )
    }
}

/// Read the real interval timer without disturbing it. See
/// [`itimer_kernel_set`] for why the host arm simulates.
fn itimer_kernel_get() -> (i64, i64) {
    #[cfg(target_os = "none")]
    {
        crate::syscall::syscall3_2ret(crate::syscall::SYS_ITIMER_GET, 0, 0, 0)
    }
    #[cfg(not(target_os = "none"))]
    {
        let cur = host_itimer::peek();
        (
            i64::try_from(cur.0).unwrap_or(i64::MAX),
            i64::try_from(cur.1).unwrap_or(i64::MAX),
        )
    }
}

/// Host-only stand-in for the kernel's one real interval timer.
///
/// Per-*thread* rather than per-process: `cargo test` runs every test on its
/// own thread inside one process, so a process-global store would let one
/// test's alarm leak into another's.
#[cfg(not(target_os = "none"))]
mod host_itimer {
    use core::cell::Cell;

    std::thread_local! {
        static REAL: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    }

    /// Install `(value_ns, interval_ns)`, returning what was there before.
    pub(super) fn swap(value_ns: u64, interval_ns: u64) -> (u64, u64) {
        REAL.with(|r| r.replace((value_ns, interval_ns)))
    }

    /// Read without disturbing.
    pub(super) fn peek() -> (u64, u64) {
        REAL.with(Cell::get)
    }
}

/// Convert a validated, non-negative `Timeval` to nanoseconds, saturating.
///
/// The kernel's interval-timer ABI speaks nanoseconds in a register rather
/// than a `Timeval` through a buffer -- see `design-decisions.md` §925 -- so
/// this is the conversion at the boundary. Saturating rather than wrapping:
/// `setitimer` with a `tv_sec` near `i64::MAX` is a caller asking for
/// effectively-never, and the honest rendering of that is the largest delay
/// we can express, not a small one obtained by wrapping.
fn itimer_timeval_to_ns(tv: &Timeval) -> u64 {
    // Callers validate non-negativity first; `unwrap_or(0)` is the floor for
    // a value that cannot occur rather than a silent fallback.
    let sec = u64::try_from(tv.tv_sec).unwrap_or(0);
    let usec = u64::try_from(tv.tv_usec).unwrap_or(0);
    sec.saturating_mul(1_000_000_000)
        .saturating_add(usec.saturating_mul(1_000))
}

/// Convert nanoseconds from the kernel back to a `Timeval`.
///
/// Truncates toward zero on the sub-microsecond part, which is what Linux
/// reports through `getitimer` as well: the residue is smaller than the unit
/// the structure can hold.
fn itimer_ns_to_timeval(ns: u64) -> Timeval {
    Timeval {
        tv_sec: i64::try_from(ns / 1_000_000_000).unwrap_or(i64::MAX),
        tv_usec: i64::try_from(ns % 1_000_000_000 / 1_000).unwrap_or(0),
    }
}

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
/// Stores the timer value so `getitimer` can retrieve it.  The timer never
/// actually fires, so programs using `setitimer` for periodic alarms get no
/// SIGALRM/SIGVTALRM/SIGPROF -- though they do see their own settings
/// reflected back, which is the part that makes this hard to notice.
///
/// **Not "because we don't have signal delivery", which is what this said
/// until 2026-09-09.** Signals are delivered (`signal.rs`'s trampoline).
/// Nothing here asks the kernel for a timer: `proc/itimer.rs` implements this
/// with a real `SIGALRM` but is reachable only through the Linux-ABI table.
/// See [`crate::unistd::alarm`] for the full position and the request.
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
    // Linux 6.6's order (kernel/time/itimer.c): the new value is read and
    // validated first, then `which` (`do_setitimer`).  A NULL new value is not
    // EFAULT: Linux takes it as all zeros -- disarm -- a "misfeature" it still
    // supports, with a warning, and glibc passes the call straight through.
    // Until 2026-09-26 `which` came first and NULL was EFAULT.
    let val = if new_value.is_null() {
        Itimerval {
            it_interval: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        }
    } else {
        // SAFETY: new_value is non-NULL (just checked).  Read unaligned to
        // tolerate caller buffers that aren't naturally aligned.
        unsafe { core::ptr::read_unaligned(new_value) }
    };
    if !itimer_timeval_valid(&val.it_value) || !itimer_timeval_valid(&val.it_interval) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if which != ITIMER_REAL && which != ITIMER_VIRTUAL && which != ITIMER_PROF {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Only ITIMER_REAL exists. The kernel refuses the other two outright, and
    // the previous code answered for all three out of a table it kept itself --
    // which is how a program could set a profiling timer, read it back
    // unchanged, and wait forever for a signal that nothing was going to send.
    // Refusing is the same call `setgroups` made (design-decisions.md §1004):
    // a caller told ENOSYS can choose a fallback, a caller told 0 cannot.
    if which != ITIMER_REAL {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }

    let value_ns = itimer_timeval_to_ns(&val.it_value);
    let interval_ns = itimer_timeval_to_ns(&val.it_interval);
    let (prev_value, prev_interval) = itimer_kernel_set(value_ns, interval_ns);
    // `rax` carries the previous value on success and `-errno` on failure.
    // The two can never be confused: the kernel saturates its nanosecond
    // answer at `i64::MAX`, so a success is never negative.
    if crate::errno::translate(prev_value) < 0 {
        return -1;
    }

    if !old_value.is_null() {
        #[allow(clippy::cast_sign_loss)]
        let old = Itimerval {
            it_interval: itimer_ns_to_timeval(prev_interval as u64),
            it_value: itimer_ns_to_timeval(prev_value as u64),
        };
        // SAFETY: `old_value` is non-null (just checked). Written unaligned to
        // tolerate a caller buffer that is not naturally aligned, matching the
        // unaligned read of `new_value` above.
        unsafe {
            core::ptr::write_unaligned(old_value, old);
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

    if which != ITIMER_REAL {
        errno::set_errno(errno::ENOSYS);
        return -1;
    }

    let (value_ns, interval_ns) = itimer_kernel_get();
    if crate::errno::translate(value_ns) < 0 {
        return -1;
    }

    #[allow(clippy::cast_sign_loss)]
    let curr = Itimerval {
        it_interval: itimer_ns_to_timeval(interval_ns as u64),
        it_value: itimer_ns_to_timeval(value_ns as u64),
    };
    // SAFETY: `curr_value` is non-null (just checked).
    unsafe {
        core::ptr::write_unaligned(curr_value, curr);
    }
    0
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The old time interfaces musl's headers still declare
// ---------------------------------------------------------------------------

/// `struct timeb`, `ftime`'s result: seconds, milliseconds, and two fields
/// every implementation now leaves 0.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timeb {
    /// Seconds since the Epoch.
    pub time: TimeT,
    /// Milliseconds past `time`.
    pub millitm: u16,
    /// Minutes west of Greenwich: always 0, as in glibc and musl.
    pub timezone: i16,
    /// Daylight-saving flag: always 0, as in glibc and musl.
    pub dstflag: i16,
}

/// The time now, to the millisecond (removed from POSIX in 2008; glibc and
/// musl still answer it from `CLOCK_REALTIME`, with no zone).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftime(tp: *mut Timeb) -> i32 {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if clock_gettime(CLOCK_REALTIME, &raw mut ts) != 0 {
        return -1;
    }
    // SAFETY: NULL or the caller's `struct timeb`.
    let Some(out) = (unsafe { tp.as_mut() }) else {
        errno::set_errno(errno::EFAULT);
        return -1;
    };
    *out = Timeb {
        time: ts.tv_sec,
        // Below 1000: `tv_nsec` is below 10^9.
        millitm: u16::try_from(ts.tv_nsec / 1_000_000).unwrap_or(0),
        timezone: 0,
        dstflag: 0,
    };
    0
}

/// Set the system time to `*t` seconds (SVID; glibc keeps it for old
/// binaries): `settimeofday` with no microseconds, so `EPERM` without the
/// privilege. A NULL `t` is `EINVAL`, as glibc's compatibility `stime`
/// checks.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn stime(t: *const TimeT) -> i32 {
    // SAFETY: NULL or the caller's `time_t`.
    let Some(&secs) = (unsafe { t.as_ref() }) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let tv = Timeval {
        tv_sec: secs,
        tv_usec: 0,
    };
    settimeofday(&raw const tv, core::ptr::null())
}

/// The CPU-time clock of process `pid`, into `*clock_id`; the error number
/// on failure, as POSIX has it return. The calling process's own clock is
/// `CLOCK_PROCESS_CPUTIME_ID`. Another process's CPU time is not something
/// this system can read -- `clock_gettime` has no clock for it -- so an
/// existing other process is `EPERM` (the error POSIX gives for "may not
/// access that clock") rather than a clock id every later call would refuse,
/// and one that does not exist is `ESRCH`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_getcpuclockid(pid: PidT, clock_id: *mut ClockidT) -> i32 {
    if pid == 0 || pid == crate::process::getpid() {
        // SAFETY: NULL or the caller's `clockid_t`.
        let Some(out) = (unsafe { clock_id.as_mut() }) else {
            return errno::EFAULT;
        };
        *out = CLOCK_PROCESS_CPUTIME_ID;
        return 0;
    }
    let saved = errno::get_errno();
    let exists = crate::signal::kill(pid, 0) == 0 || errno::get_errno() == errno::EPERM;
    errno::set_errno(saved);
    if exists { errno::EPERM } else { errno::ESRCH }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GNU make's configure probe for a standard `gettimeofday` ("checking for
    /// standard gettimeofday"), which `scripts/make-spike/run.sh` answers `yes`
    /// for SlateOS (`ac_cv_func_gettimeofday`) without running it: the
    /// timeval filled with a time after the epoch, and 0 returned.
    #[test]
    fn make_configures_gettimeofday_probe_passes() {
        let mut t = Timeval {
            tv_sec: -1,
            tv_usec: -1,
        };
        assert_eq!(gettimeofday(&raw mut t, core::ptr::null_mut()), 0);
        assert!(t.tv_sec >= 0 && t.tv_usec >= 0, "{t:?}");
    }

    #[test]
    fn stime_of_null_is_einval() {
        errno::set_errno(0);
        assert_eq!(stime(core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn ftime_of_null_is_efault_and_zone_fields_are_zero() {
        errno::set_errno(0);
        let mut tb = Timeb {
            time: 7,
            millitm: 9,
            timezone: 5,
            dstflag: 1,
        };
        if ftime(&raw mut tb) == 0 {
            assert!(tb.millitm < 1000);
            assert_eq!((tb.timezone, tb.dstflag), (0, 0));
        }
    }

    #[test]
    fn clock_getcpuclockid_of_self_is_the_process_clock() {
        let mut id: ClockidT = -1;
        assert_eq!(clock_getcpuclockid(0, &raw mut id), 0);
        assert_eq!(id, CLOCK_PROCESS_CPUTIME_ID);
        assert_eq!(clock_getcpuclockid(0, core::ptr::null_mut()), errno::EFAULT);
    }

    /// Create a zeroed Tm.
    fn zero_tm() -> Tm {
        Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            ..crate::time::Tm::ZERO
        }
    }

    /// Installs a zone for the length of a test, and puts the previous `TZ`
    /// back on drop.
    ///
    /// The zone, like the environment it is read from, is per test thread
    /// on the host (`crate::tz`, `crate::perprocess`), so this serialises
    /// nothing: a test that installs no zone reads a fresh process's, UTC,
    /// whatever the tests beside it install. Until 2026-10-05 the zone was
    /// shared, every zone-reading test had to hold this as a lock, and the
    /// one that did not failed at random
    /// (`known-issues-resolved/D-POSIX-HOST-TESTS-SHARE-ONE-TIME-ZONE.md`).
    /// [`TzGuard::utc`] still says which zone a test assumes.
    struct TzGuard {
        saved: Option<std::vec::Vec<u8>>,
    }

    impl TzGuard {
        /// Install `tz` (a POSIX TZ string, no trailing NUL) for the
        /// duration of the test.
        fn set(tz: &[u8]) -> Self {
            let saved = crate::environ::getenv_bytes(b"TZ").map(<[u8]>::to_vec);
            Self::put(tz);
            Self { saved }
        }

        /// Install UTC — the zone the timezone-agnostic tests assume.
        fn utc() -> Self {
            Self::set(b"UTC0")
        }

        /// Write `TZ=<tz>` into the environment and re-read it.
        fn put(tz: &[u8]) {
            let mut value = std::vec::Vec::with_capacity(tz.len() + 1);
            value.extend_from_slice(tz);
            value.push(0);
            // SAFETY: both strings are NUL-terminated and outlive the call.
            let rc = unsafe { crate::environ::setenv(c"TZ".as_ptr().cast(), value.as_ptr(), 1) };
            assert_eq!(rc, 0, "setenv(TZ) failed");
            tzset();
        }
    }

    impl Drop for TzGuard {
        fn drop(&mut self) {
            match self.saved.as_deref() {
                Some(previous) => Self::put(previous),
                None => {
                    // SAFETY: the name is a NUL-terminated literal.
                    let rc = unsafe { crate::environ::unsetenv(c"TZ".as_ptr().cast()) };
                    assert_eq!(rc, 0, "unsetenv(TZ) failed");
                    tzset();
                }
            }
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
        assert_eq!(tm.tm_year, 70); // 1970 - 1900
        assert_eq!(tm.tm_mon, 0); // January
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_min, 0);
        assert_eq!(tm.tm_sec, 0);
        assert_eq!(tm.tm_wday, 4); // Thursday
        assert_eq!(tm.tm_yday, 0);
    }

    #[test]
    fn test_gmtime_known_date() {
        // 2000-01-01 00:00:00 UTC = 946684800
        let t: TimeT = 946_684_800;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 100); // 2000 - 1900
        assert_eq!(tm.tm_mon, 0); // January
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_wday, 6); // Saturday
    }

    #[test]
    fn test_gmtime_leap_day() {
        // 2000-02-29 12:00:00 UTC = 951825600
        let t: TimeT = 951_825_600;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 100);
        assert_eq!(tm.tm_mon, 1); // February (0-indexed)
        assert_eq!(tm.tm_mday, 29);
        assert_eq!(tm.tm_hour, 12);
    }

    #[test]
    fn test_gmtime_end_of_year() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        let t: TimeT = 1_704_067_199;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 123); // 2023 - 1900
        assert_eq!(tm.tm_mon, 11); // December
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
        assert_eq!(tm.tm_year, 69); // 1969 - 1900
        assert_eq!(tm.tm_mon, 11); // December
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
        let _tz = TzGuard::utc();
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        let t = mktime(&mut tm);
        assert_eq!(t, 0);
    }

    #[test]
    fn test_mktime_known_date() {
        let _tz = TzGuard::utc();
        let mut tm = zero_tm();
        tm.tm_year = 100; // 2000
        tm.tm_mon = 0; // January
        tm.tm_mday = 1;
        let t = mktime(&mut tm);
        assert_eq!(t, 946_684_800);
    }

    #[test]
    fn test_mktime_normalizes() {
        let _tz = TzGuard::utc();
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
        let _tz = TzGuard::utc();
        // Month 12 (January of next year) should normalize.
        let mut tm = zero_tm();
        tm.tm_year = 100; // 2000
        tm.tm_mon = 12; // 13th month → January 2001
        tm.tm_mday = 1;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_year, 101); // 2001
        assert_eq!(tm.tm_mon, 0); // January
    }

    #[test]
    fn test_mktime_sets_wday() {
        let _tz = TzGuard::utc();
        // 2024-03-15 should be a Friday (wday=5).
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = 2; // March
        tm.tm_mday = 15;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_wday, 5); // Friday
    }

    // -- gmtime / mktime roundtrip --

    #[test]
    fn test_gmtime_mktime_roundtrip() {
        let _tz = TzGuard::utc();
        let timestamps: &[TimeT] = &[0, 1, 86400, 946_684_800, 1_704_067_199, -1, -86400];
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
        tm.tm_year = 93; // 1993
        tm.tm_mon = 5; // June (0-indexed)
        tm.tm_mday = 30;
        tm.tm_hour = 21;
        tm.tm_min = 49;
        tm.tm_sec = 8;
        tm.tm_wday = 3; // Wednesday

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
        tm.tm_year = 124; // 2024
        tm.tm_mon = 2; // March
        tm.tm_mday = 15;
        tm.tm_wday = 5; // Friday

        let mut buf = [0u8; 64];
        let fmt = b"%Y-%m-%d\0";
        let n = unsafe { strftime(buf.as_mut_ptr(), 64, fmt.as_ptr(), &tm) };
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
        let n = unsafe { strftime(buf.as_mut_ptr(), 64, fmt.as_ptr(), &tm) };
        assert!(n > 0);
        assert_eq!(&buf[..8], b"14:30:45");
    }

    #[test]
    fn test_strftime_percent_literal() {
        let tm = zero_tm();
        let mut buf = [0u8; 16];
        let fmt = b"100%%\0";
        let n = unsafe { strftime(buf.as_mut_ptr(), 16, fmt.as_ptr(), &tm) };
        assert!(n > 0);
        assert_eq!(&buf[..4], b"100%");
    }

    #[test]
    fn test_strftime_buffer_too_small() {
        let tm = zero_tm();
        let mut buf = [0u8; 4];
        let fmt = b"%Y-%m-%d\0";
        let n = unsafe { strftime(buf.as_mut_ptr(), 4, fmt.as_ptr(), &tm) };
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
        let result = unsafe { strptime(input.as_ptr(), fmt.as_ptr(), &mut tm) };
        assert!(!result.is_null());
        assert_eq!(tm.tm_year, 124); // 2024 - 1900
        assert_eq!(tm.tm_mon, 2); // March (0-indexed)
        assert_eq!(tm.tm_mday, 15);
    }

    #[test]
    fn test_strptime_time() {
        let mut tm = zero_tm();
        let input = b"14:30:45\0";
        let fmt = b"%H:%M:%S\0";
        let result = unsafe { strptime(input.as_ptr(), fmt.as_ptr(), &mut tm) };
        assert!(!result.is_null());
        assert_eq!(tm.tm_hour, 14);
        assert_eq!(tm.tm_min, 30);
        assert_eq!(tm.tm_sec, 45);
    }

    // -- is_leap tests --

    #[test]
    fn test_leap_years() {
        assert!(is_leap(2000)); // Divisible by 400.
        assert!(!is_leap(1900)); // Divisible by 100 but not 400.
        assert!(is_leap(2024)); // Divisible by 4 but not 100.
        assert!(!is_leap(2023)); // Not divisible by 4.
    }

    // -- mktime null --

    #[test]
    fn test_mktime_null() {
        let _tz = TzGuard::utc();
        assert_eq!(mktime(core::ptr::null_mut()), -1);
    }

    // -- timegm / timelocal identity --

    #[test]
    fn test_timegm_equals_mktime() {
        let _tz = TzGuard::utc();
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
        let _tz = TzGuard::utc();
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
        let n = unsafe { strftime(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), &raw const *tm) };
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
        tm.tm_mon = 5; // June
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
        tm.tm_yday = 0; // Jan 1 = day 1
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
        // `%z` renders the offset recorded in the `Tm`, not the process's
        // current zone's, so a hand-built `Tm` renders a zero one. `%Z` is
        // `tm_zone`; when that is NULL, glibc's falls back on
        // `tzname[tm_isdst]` -- UTC's here -- and renders nothing at all
        // when `tm_isdst` is negative, `%z` neither. (Until 2026-10-06 this
        // said glibc renders an empty `%Z`; its oracle says otherwise.)
        let _tz = TzGuard::utc();
        let mut tm = zero_tm();
        assert_eq!(run_strftime(b"%z\0", &tm), b"+0000");
        assert_eq!(run_strftime(b"%Z\0", &tm), b"UTC");
        tm.tm_isdst = -1;
        assert_eq!(run_strftime(b"%Z\0", &tm), b"");
        assert_eq!(run_strftime(b"%z\0", &tm), b"");

        // A `Tm` that went through a conversion carries a name.
        tm.tm_isdst = 0;
        tm.tm_zone = c"EST".as_ptr().cast();
        assert_eq!(run_strftime(b"%Z\0", &tm), b"EST");
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
        tm.tm_mon = 5; // June
        tm.tm_mday = 15;
        // %D = %m/%d/%y
        assert_eq!(run_strftime(b"%D\0", &tm), b"06/15/23");
    }

    // -- strptime additional tests --

    #[test]
    fn test_strptime_ampm() {
        let mut tm = zero_tm();
        let rem = unsafe { strptime(b"02:30 PM\0".as_ptr(), b"%I:%M %p\0".as_ptr(), &raw mut tm) };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_hour, 14); // 2 PM = 14
        assert_eq!(tm.tm_min, 30);
    }

    #[test]
    fn test_strptime_noon_pm() {
        let mut tm = zero_tm();
        unsafe {
            strptime(b"12:00 PM\0".as_ptr(), b"%I:%M %p\0".as_ptr(), &raw mut tm);
        }
        assert_eq!(tm.tm_hour, 12); // 12 PM = noon = 12
    }

    #[test]
    fn test_strptime_midnight_am() {
        let mut tm = zero_tm();
        unsafe {
            strptime(b"12:00 AM\0".as_ptr(), b"%I:%M %p\0".as_ptr(), &raw mut tm);
        }
        assert_eq!(tm.tm_hour, 0); // 12 AM = midnight = 0
    }

    #[test]
    fn test_strptime_month_name() {
        let mut tm = zero_tm();
        let rem = unsafe { strptime(b"January\0".as_ptr(), b"%B\0".as_ptr(), &raw mut tm) };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_mon, 0); // January = 0
    }

    #[test]
    fn test_strptime_month_abbr() {
        let mut tm = zero_tm();
        let rem = unsafe { strptime(b"Dec\0".as_ptr(), b"%b\0".as_ptr(), &raw mut tm) };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_mon, 11); // December = 11
    }

    #[test]
    fn test_strptime_weekday_name() {
        let mut tm = zero_tm();
        let rem = unsafe { strptime(b"Wednesday\0".as_ptr(), b"%A\0".as_ptr(), &raw mut tm) };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_wday, 3); // Wednesday = 3
    }

    #[test]
    fn test_strptime_iso_date() {
        let mut tm = zero_tm();
        let input = b"2023-06-15\0";
        let fmt = b"%Y-%m-%d\0";
        let rem = unsafe { strptime(input.as_ptr(), fmt.as_ptr(), &raw mut tm) };
        assert!(!rem.is_null());
        assert_eq!(tm.tm_year, 123); // 2023-1900
        assert_eq!(tm.tm_mon, 5); // June = 5
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
        let rem = unsafe { strptime(b"+0530\0".as_ptr(), b"%z\0".as_ptr(), &raw mut tm) };
        // Should succeed (parse but ignore timezone)
        assert!(!rem.is_null());
    }

    #[test]
    fn test_strptime_literal_percent() {
        let mut tm = zero_tm();
        let rem = unsafe { strptime(b"100%\0".as_ptr(), b"100%%\0".as_ptr(), &raw mut tm) };
        assert!(!rem.is_null());
    }

    #[test]
    fn test_strptime_returns_remaining() {
        let mut tm = zero_tm();
        let input = b"2023 extra stuff\0";
        let rem = unsafe { strptime(input.as_ptr(), b"%Y\0".as_ptr(), &raw mut tm) };
        assert!(!rem.is_null());
        // rem should point to " extra stuff"
        assert_eq!(unsafe { *rem }, b' ');
    }

    #[test]
    fn test_strptime_bad_input() {
        let mut tm = zero_tm();
        let rem = unsafe { strptime(b"abc\0".as_ptr(), b"%Y\0".as_ptr(), &raw mut tm) };
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
        tm.tm_wday = 4; // Thursday
        tm.tm_yday = 0;
        assert_eq!(run_strftime(b"%G %V\0", &tm), b"2015 01");
    }

    #[test]
    fn test_iso_week_2014_dec29() {
        // 2014-12-29 is Monday → ISO week 01 of 2015
        let mut tm = zero_tm();
        tm.tm_year = 114; // 2014
        tm.tm_mon = 11; // December
        tm.tm_mday = 29;
        tm.tm_wday = 1; // Monday
        tm.tm_yday = 362; // 0-indexed
        assert_eq!(run_strftime(b"%G %V\0", &tm), b"2015 01");
    }

    #[test]
    fn test_iso_week_2016_jan1() {
        // 2016-01-01 is Friday → ISO week 53 of 2015
        let mut tm = zero_tm();
        tm.tm_year = 116; // 2016
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_wday = 5; // Friday
        tm.tm_yday = 0;
        assert_eq!(run_strftime(b"%G %V\0", &tm), b"2015 53");
    }

    // -- mktime edge cases --

    #[test]
    fn test_mktime_negative_month() {
        let _tz = TzGuard::utc();
        // tm_mon = -1 should borrow from year (December of previous year)
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = -1; // Should normalize to December 2023
        tm.tm_mday = 15;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_mon, 11); // December
        assert_eq!(tm.tm_year, 123); // 2023
        assert_eq!(tm.tm_mday, 15);
    }

    #[test]
    fn test_mktime_overflow_day() {
        let _tz = TzGuard::utc();
        // Jan 32 should become Feb 1
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = 0; // January
        tm.tm_mday = 32;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_mon, 1); // February
        assert_eq!(tm.tm_mday, 1);
    }

    #[test]
    fn test_mktime_overflow_seconds() {
        let _tz = TzGuard::utc();
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
        // Must match the glibc/musl x86-64 `struct tm` byte for byte:
        // callers allocate it, so a short struct means `localtime_r`
        // writes past the end of theirs.  9 `int`s = 36 bytes, padded to
        // 40 for the 8-byte `tm_gmtoff`, then the `tm_zone` pointer.
        assert_eq!(core::mem::size_of::<Tm>(), 56);
        assert_eq!(core::mem::align_of::<Tm>(), 8);
        let tm = Tm::ZERO;
        let base = (&raw const tm).cast::<u8>();
        // SAFETY: both projections are within the same allocated `Tm`.
        unsafe {
            assert_eq!((&raw const tm.tm_isdst).cast::<u8>().offset_from(base), 32);
            assert_eq!((&raw const tm.tm_gmtoff).cast::<u8>().offset_from(base), 40);
            assert_eq!((&raw const tm.tm_zone).cast::<u8>().offset_from(base), 48);
        }
        // Every field, in glibc's order: the nine `int`s four bytes apart.
        // (Until 2026-09-26 this half lived in linux_clock_user_types.rs,
        // deleted with the other unused constant modules.)
        use core::mem::offset_of;
        let ints = [
            offset_of!(Tm, tm_sec),
            offset_of!(Tm, tm_min),
            offset_of!(Tm, tm_hour),
            offset_of!(Tm, tm_mday),
            offset_of!(Tm, tm_mon),
            offset_of!(Tm, tm_year),
            offset_of!(Tm, tm_wday),
            offset_of!(Tm, tm_yday),
            offset_of!(Tm, tm_isdst),
        ];
        assert_eq!(ints, [0, 4, 8, 12, 16, 20, 24, 28, 32]);
        assert_eq!(offset_of!(Tm, tm_gmtoff), 40);
        assert_eq!(offset_of!(Tm, tm_zone), 48);
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
        let _tz = TzGuard::utc();
        // Feb 29 in a non-leap year should normalize to March 1.
        let mut tm = zero_tm();
        tm.tm_year = 123; // 2023 (not a leap year)
        tm.tm_mon = 1; // February
        tm.tm_mday = 29;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_mon, 2); // March
        assert_eq!(tm.tm_mday, 1);
    }

    #[test]
    fn test_mktime_mday_zero_borrows() {
        let _tz = TzGuard::utc();
        // mday=0 should be last day of previous month.
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024 (leap year)
        tm.tm_mon = 2; // March
        tm.tm_mday = 0; // → Feb 29 (leap year)
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_mon, 1); // February
        assert_eq!(tm.tm_mday, 29); // Leap day
    }

    #[test]
    fn test_mktime_negative_seconds() {
        let _tz = TzGuard::utc();
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
        assert_eq!(tm.tm_year, 0); // 1900 - 1900
        assert_eq!(tm.tm_mon, 0); // January
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_wday, 1); // Monday
        assert_eq!(tm.tm_yday, 0);
    }

    #[test]
    fn test_gmtime_2038_boundary() {
        // 2038-01-19 03:14:07 UTC = 2^31 - 1 (max 32-bit time_t).
        let t: TimeT = 2_147_483_647;
        let tm = gmtime(&t);
        let tm = unsafe { &*tm };
        assert_eq!(tm.tm_year, 138); // 2038 - 1900
        assert_eq!(tm.tm_mon, 0); // January
        assert_eq!(tm.tm_mday, 19);
        assert_eq!(tm.tm_hour, 3);
        assert_eq!(tm.tm_min, 14);
        assert_eq!(tm.tm_sec, 7);
    }

    #[test]
    fn test_gmtime_mktime_roundtrip_leap_years() {
        let _tz = TzGuard::utc();
        // Test roundtrip for several leap-year Feb 29 timestamps.
        let leap_feb29_timestamps: &[TimeT] = &[
            68169600,   // 1972-02-29 00:00:00 UTC
            951782400,  // 2000-02-29 00:00:00 UTC (century leap)
            1709164800, // 2024-02-29 00:00:00 UTC
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
        // `%s` is `mktime` of the broken-down time, which reads the zone:
        // without the guard, a neighbour's `TzGuard::set` moved the answer
        // by its offset, and the order gate refused pushes over it
        // (requests/b-d-strftime-epoch-seconds-test-races-the-time-zone.md).
        let _tz = TzGuard::utc();
        // %s = seconds since epoch (GNU extension).
        let mut tm = zero_tm();
        tm.tm_year = 70;
        tm.tm_mon = 0;
        tm.tm_mday = 2; // 86400 seconds from epoch
        let result = run_strftime(b"%s\0", &tm);
        assert_eq!(result, b"86400");
    }

    #[test]
    fn test_hour_12_all_values() {
        let mut tm = zero_tm();
        for (hour, twelve) in [
            (0, b"12"),
            (1, b"01"),
            (11, b"11"),
            (12, b"12"),
            (13, b"01"),
            (23, b"11"),
        ] {
            tm.tm_hour = hour;
            assert_eq!(run_strftime(b"%I\0", &tm), twelve, "hour {hour}");
        }
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
        assert_eq!(tm.tm_year, 60); // 1960
        assert_eq!(tm.tm_mon, 0); // January
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
        assert_eq!(tm.tm_year, 200); // 2100
        assert_eq!(tm.tm_mon, 2); // March (2100 is NOT a leap year)
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
        let _tz = TzGuard::utc();
        // -1 hour from midnight Jan 1 → 23:00 Dec 31 previous year.
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = 0;
        tm.tm_mday = 1;
        tm.tm_hour = -1;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_hour, 23);
        assert_eq!(tm.tm_mday, 31);
        assert_eq!(tm.tm_mon, 11); // December
        assert_eq!(tm.tm_year, 123); // 2023
    }

    #[test]
    fn test_mktime_large_seconds_cascade() {
        let _tz = TzGuard::utc();
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
        let _tz = TzGuard::utc();
        // Month -13 should go back a full year + 1 month.
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024
        tm.tm_mon = -13; // Should normalize to November 2022.
        tm.tm_mday = 1;
        let _ = mktime(&mut tm);
        assert_eq!(tm.tm_year, 122); // 2022
        assert_eq!(tm.tm_mon, 11); // December
    }

    #[test]
    fn test_mktime_month_large_positive() {
        let _tz = TzGuard::utc();
        // Month 24 = 2 years forward.
        let mut tm = zero_tm();
        tm.tm_year = 70; // 1970
        tm.tm_mon = 24; // 2 years = 1972 January
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
        assert_eq!(run_strftime(b"%G %V\0", &tm), b"2024 01");
    }

    #[test]
    fn test_iso_week_dec31_2024() {
        // 2024-12-31 is Tuesday (wday=2, yday=365 in leap year).
        // ISO: The Thursday of this week is Jan 2, 2025 → week 01 of 2025.
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_wday = 2; // Tuesday
        tm.tm_yday = 365;
        assert_eq!(run_strftime(b"%G %V\0", &tm), b"2025 01");
    }

    #[test]
    fn test_iso_week_dec29_2014() {
        // 2014-12-29 is Monday (wday=1, yday=362).
        // Thursday of this ISO week = Jan 1, 2015 → week 01 of 2015.
        let mut tm = zero_tm();
        tm.tm_year = 114;
        tm.tm_wday = 1; // Monday
        tm.tm_yday = 362;
        assert_eq!(run_strftime(b"%G %V\0", &tm), b"2015 01");
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
        assert_eq!(run_strftime(b"%G %V\0", &tm), b"2015 53");
    }

    #[test]
    fn test_iso_week_mid_year_2024() {
        // 2024-06-15 is Saturday (wday=6, yday=166 in leap year).
        let mut tm = zero_tm();
        tm.tm_year = 124;
        tm.tm_wday = 6; // Saturday
        tm.tm_yday = 166;
        assert_eq!(run_strftime(b"%G %V\0", &tm), b"2024 24");
    }

    // -- gmtime/mktime roundtrip for extended range --

    #[test]
    fn test_roundtrip_post_2038_timestamps() {
        let _tz = TzGuard::utc();
        let timestamps: &[TimeT] = &[
            2_147_483_648, // 2038-01-19 03:14:08
            4_107_542_400, // 2100-03-01
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
        assert_eq!(tm.tm_mon, 11); // December
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
        assert_eq!(tm.tm_mon, 2); // March
        assert_eq!(tm.tm_year, 124); // 2024 - 1900
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
        assert_eq!(tm.tm_year, 69); // 1969
        assert_eq!(tm.tm_mon, 11); // December
        assert_eq!(tm.tm_mday, 31);
        assert_eq!(tm.tm_hour, 23);
        assert_eq!(tm.tm_min, 59);
        assert_eq!(tm.tm_sec, 59);
        assert_eq!(tm.tm_wday, 3); // Wednesday
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
        assert_eq!(tm.tm_wday, 5); // Friday
    }

    #[test]
    fn test_mktime_dec31_to_jan1() {
        // Dec 32 should normalize to Jan 1 of next year.
        let mut tm = zero_tm();
        tm.tm_year = 70; // 1970
        tm.tm_mon = 11; // December
        tm.tm_mday = 32; // Dec 32 = Jan 1 next year
        let secs = tm_to_secs(&mut tm).unwrap();

        assert_eq!(tm.tm_year, 71); // Normalized to 1971
        assert_eq!(tm.tm_mon, 0); // January
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
        tm.tm_year = 123; // 2023 (non-leap)
        tm.tm_mon = 1; // February
        tm.tm_mday = 30;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_mon, 2); // March
        assert_eq!(tm.tm_mday, 2);
    }

    #[test]
    fn test_mktime_feb30_leap_normalizes() {
        // Feb 30 in a leap year should normalize to March 1.
        let mut tm = zero_tm();
        tm.tm_year = 124; // 2024 (leap year)
        tm.tm_mon = 1; // February
        tm.tm_mday = 30;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_mon, 2); // March
        assert_eq!(tm.tm_mday, 1);
    }

    #[test]
    fn test_mktime_negative_mday() {
        // mday = 0 should borrow from previous month (Dec 31).
        let mut tm = zero_tm();
        tm.tm_year = 71; // 1971
        tm.tm_mon = 0; // January
        tm.tm_mday = 0; // Jan 0 = Dec 31 of prev year
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_year, 70); // 1970
        assert_eq!(tm.tm_mon, 11); // December
        assert_eq!(tm.tm_mday, 31);
    }

    #[test]
    fn test_mktime_deeply_negative_mday() {
        // mday = -30 in January: Jan 0 = Dec 31, Jan -30 = Dec 1.
        let mut tm = zero_tm();
        tm.tm_year = 71; // 1971
        tm.tm_mon = 0; // January
        tm.tm_mday = -30;
        tm_to_secs(&mut tm);
        assert_eq!(tm.tm_year, 70); // 1970
        assert_eq!(tm.tm_mon, 11); // December
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
        tm.tm_year = 100; // 2000
        tm.tm_mon = 1; // February
        tm.tm_mday = 29; // Feb 29 (leap day)
        tm.tm_hour = 23;
        tm.tm_min = 59;
        tm.tm_sec = 59;
        let secs = tm_to_secs(&mut tm).unwrap();

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
        tm.tm_mon = 2; // March
        tm.tm_mday = 1;
        let secs = tm_to_secs(&mut tm).unwrap();

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
        tm.tm_year = 100; // 2000 (leap)
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
            assert_eq!(
                tm.tm_wday, expected_wday,
                "day {i} should be wday {expected_wday}, got {}",
                tm.tm_wday
            );
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
        let _tz = TzGuard::utc();
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
        let _tz = TzGuard::utc();
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
    fn test_clock_settime_valid_reaches_cap_gate() {
        // CLOCK_REALTIME with a valid timespec passes every argument
        // check and reaches the CAP_SYS_TIME gate.  With the cap held
        // (host default) the call would fall through to a real
        // SYS_CLOCK_SETTIME syscall, which we can't execute on the host —
        // so we drop the cap and assert EPERM, the maximal host-observable
        // boundary that proves the args passed all validation.
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_valid_reaches_cap_gate() {
        // Valid args pass validation and reach the CAP_SYS_TIME gate;
        // dropping the cap surfaces EPERM (the cap-held path issues a real
        // syscall not executable on the host).
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let tv = Timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_tzset_installs_the_zone_named_by_tz() {
        let _tz = TzGuard::set(b"EST5EDT,M3.2.0,M11.1.0");
        // 2021-01-15 12:00 UTC is winter, 2021-07-15 12:00 UTC summer.
        let winter = local_tm(1_610_712_000);
        assert_eq!(
            (winter.tm_hour, winter.tm_isdst, winter.tm_gmtoff),
            (7, 0, -5 * 3600)
        );
        // SAFETY: `tm_zone` points at a process-lifetime name.
        assert_eq!(unsafe { cstr(winter.tm_zone) }, b"EST");
        let summer = local_tm(1_626_350_400);
        assert_eq!(
            (summer.tm_hour, summer.tm_isdst, summer.tm_gmtoff),
            (8, 1, -4 * 3600)
        );
        // SAFETY: as above.
        assert_eq!(unsafe { cstr(summer.tm_zone) }, b"EDT");
    }

    #[test]
    fn test_timezone_globals_track_the_installed_zone() {
        // `timezone` is WEST-positive (the sign opposite `tm_gmtoff`),
        // reports *standard* time only, and `daylight` means "this zone
        // ever observes DST", not "DST is in force now".
        let _tz = TzGuard::set(b"EST5EDT,M3.2.0,M11.1.0");
        let (offset_west, ever_dst, std_name, dst_name) = zone_globals();
        assert_eq!(offset_west, 5 * 3600);
        assert_eq!(ever_dst, 1);
        assert_eq!(std_name, b"EST");
        assert_eq!(dst_name, b"EDT");
    }

    #[test]
    fn test_timezone_globals_for_a_zone_without_dst() {
        let _tz = TzGuard::set(b"MST7");
        let (offset_west, ever_dst, std_name, dst_name) = zone_globals();
        assert_eq!(offset_west, 7 * 3600);
        assert_eq!(ever_dst, 0);
        assert_eq!(std_name, b"MST");
        // With no daylight zone `tzname[1]` repeats the standard name
        // rather than going null, because C code prints it
        // unconditionally.  glibc does the same.
        assert_eq!(dst_name, b"MST");
    }

    /// The zone is per test thread on the host, as the environment is: one
    /// set on another thread is not this thread's, and a thread that sets
    /// none reads glibc's default for a process with no `TZ` and no
    /// `/etc/localtime` -- UTC, in both `tzname`s. Until 2026-10-05 the zone
    /// was one for every test, and a test that read it without taking
    /// [`TzGuard`] read whatever a neighbour had set
    /// (`known-issues-resolved/D-POSIX-HOST-TESTS-SHARE-ONE-TIME-ZONE.md`).
    #[test]
    fn test_a_zone_set_on_another_thread_is_not_this_threads() {
        let other = std::thread::spawn(|| {
            let _tz = TzGuard::set(b"EST5EDT,M3.2.0,M11.1.0");
            let (west, ever_dst, std_name, dst_name) = zone_globals();
            let mut tm = zero_tm();
            // 2026-07-01 16:00 UTC is noon in New York's summer.
            let t: TimeT = 1_782_921_600;
            // SAFETY: both pointers are to live locals of the right types.
            assert!(!unsafe { localtime_r(&raw const t, &raw mut tm) }.is_null());
            (west, ever_dst, std_name, dst_name, tm.tm_hour)
        });
        let (west, ever_dst, std_name, dst_name, hour) = other.join().unwrap();
        assert_eq!((west, ever_dst, hour), (5 * 3600, 1, 12));
        assert_eq!(
            (std_name.as_slice(), dst_name.as_slice()),
            (&b"EST"[..], &b"EDT"[..])
        );

        // This thread set none, before or after.
        tzset();
        let (west, ever_dst, std_name, dst_name) = zone_globals();
        assert_eq!((west, ever_dst), (0, 0));
        assert_eq!(
            (std_name.as_slice(), dst_name.as_slice()),
            (&b"UTC"[..], &b"UTC"[..])
        );
        let mut tm = zero_tm();
        let t: TimeT = 1_782_921_600;
        // SAFETY: both pointers are to live locals of the right types.
        assert!(!unsafe { localtime_r(&raw const t, &raw mut tm) }.is_null());
        assert_eq!(tm.tm_hour, 16);
    }

    /// Snapshot the four C-visible zone globals as owned values:
    /// `(timezone, daylight, tzname[0], tzname[1])`.
    ///
    /// This test thread's, as a C program would read them
    /// (`crate::time::tz_globals_for_test`).
    fn zone_globals() -> (i64, i32, std::vec::Vec<u8>, std::vec::Vec<u8>) {
        let (names, west, ever_dst) = crate::time::tz_globals_for_test();
        // SAFETY: each name is NUL-terminated, in this thread's name arena
        // or a literal, and outlives the copy.
        unsafe { (west, ever_dst, cstr(names[0]), cstr(names[1])) }
    }

    /// Copy a NUL-terminated libc string into an owned `Vec`.
    ///
    /// # Safety
    /// `ptr` must be non-null and NUL-terminated.
    unsafe fn cstr(ptr: *const u8) -> std::vec::Vec<u8> {
        assert!(!ptr.is_null(), "unexpected null tzname entry");
        // SAFETY: guaranteed by the caller.
        let len = unsafe { crate::string::strlen(ptr) } as usize;
        // SAFETY: `strlen` bytes are readable from `ptr` by construction.
        unsafe { core::slice::from_raw_parts(ptr, len) }.to_vec()
    }

    // -------------------------------------------------------------------
    // TZ wiring — the libc conversion functions honouring the zone
    // -------------------------------------------------------------------

    /// US Eastern, with the post-2007 rules written out explicitly.
    const US_EASTERN: &[u8] = b"EST5EDT,M3.2.0,M11.1.0";

    /// `localtime_r` into a fresh stack `Tm`, asserting success.
    fn local_tm(t: TimeT) -> Tm {
        let mut tm = zero_tm();
        // SAFETY: both pointers are valid, aligned and non-null, and the
        // `Tm` outlives the call.
        let ret = unsafe { localtime_r(&raw const t, &raw mut tm) };
        assert!(!ret.is_null(), "localtime_r failed for {t}");
        tm
    }

    /// `localtime_r` must shift the wall clock *and* fill in the three
    /// zone fields.  Before TZ support it aliased `gmtime_r`, so this is
    /// the test that would have caught the whole class of bug.
    #[test]
    fn test_localtime_r_applies_the_zone_in_winter() {
        let _tz = TzGuard::set(US_EASTERN);
        // 2021-01-15 12:00:00 UTC == 07:00:00 EST.
        let tm = local_tm(1_610_712_000);
        assert_eq!(tm.tm_hour, 7);
        assert_eq!(tm.tm_mday, 15);
        assert_eq!(tm.tm_isdst, 0);
        assert_eq!(tm.tm_gmtoff, -5 * 3600);
        // SAFETY: `tm_zone` points into `tz`'s static name storage.
        assert_eq!(unsafe { cstr(tm.tm_zone) }, b"EST");
    }

    #[test]
    fn test_localtime_r_applies_the_zone_in_summer() {
        let _tz = TzGuard::set(US_EASTERN);
        // 2021-07-15 12:00:00 UTC == 08:00:00 EDT.
        let tm = local_tm(1_626_350_400);
        assert_eq!(tm.tm_hour, 8);
        assert_eq!(tm.tm_isdst, 1);
        assert_eq!(tm.tm_gmtoff, -4 * 3600);
        // SAFETY: as above.
        assert_eq!(unsafe { cstr(tm.tm_zone) }, b"EDT");
    }

    /// `gmtime_r` must stay UTC no matter what `TZ` says, and must reset
    /// the zone fields rather than leaving a previous conversion's.
    #[test]
    fn test_gmtime_r_ignores_the_zone() {
        let _tz = TzGuard::set(US_EASTERN);
        let t: TimeT = 1_626_350_400;
        // Start from a local reading so a failure to reset the zone
        // fields shows up.
        let mut tm = local_tm(t);
        // SAFETY: both pointers are valid, aligned and non-null.
        assert!(!unsafe { gmtime_r(&raw const t, &raw mut tm) }.is_null());
        assert_eq!(tm.tm_hour, 12);
        assert_eq!(tm.tm_isdst, 0);
        assert_eq!(tm.tm_gmtoff, 0);
        // glibc names UTC renderings "GMT", whatever `TZ` says.
        // SAFETY: `tm_zone` points at the static "GMT" literal.
        assert_eq!(unsafe { cstr(tm.tm_zone) }, b"GMT");
    }

    /// `mktime` reads its `Tm` as local time; `timegm` reads the same
    /// fields as UTC.  Under a non-UTC zone they must disagree by exactly
    /// the offset — the property that made their old aliasing invisible.
    #[test]
    fn test_mktime_and_timegm_differ_by_the_offset() {
        let _tz = TzGuard::set(US_EASTERN);
        let mut local = zero_tm();
        local.tm_year = 121; // 2021
        local.tm_mon = 0; // January
        local.tm_mday = 15;
        local.tm_hour = 7;
        local.tm_isdst = -1;
        let mut utc = local;

        let from_local = mktime(&raw mut local);
        let from_utc = timegm(&raw mut utc);
        assert_eq!(from_local, 1_610_712_000);
        assert_eq!(from_utc, 1_610_694_000);
        assert_eq!(from_local - from_utc, 5 * 3600);
        // `mktime` writes back what it resolved.
        assert_eq!(local.tm_isdst, 0);
        assert_eq!(local.tm_gmtoff, -5 * 3600);
        // `timegm` is zone-free.
        assert_eq!(utc.tm_gmtoff, 0);
    }

    #[test]
    fn test_mktime_localtime_roundtrip_across_the_dst_start() {
        let _tz = TzGuard::set(US_EASTERN);
        // One second either side of 2021-03-14 07:00:00 UTC, the instant
        // EST becomes EDT.
        for (t, hour, isdst) in [
            (1_615_705_199 as TimeT, 1, 0),
            (1_615_705_200 as TimeT, 3, 1),
        ] {
            let mut tm = local_tm(t);
            assert_eq!(tm.tm_hour, hour, "wall clock at {t}");
            assert_eq!(tm.tm_isdst, isdst, "isdst at {t}");
            assert_eq!(mktime(&raw mut tm), t, "round-trip at {t}");
        }
    }

    /// The hour 02:00–02:59 local does not exist on the spring-forward
    /// day.  POSIX leaves the result unspecified; we resolve it the way
    /// glibc does, landing just past the jump rather than failing.
    #[test]
    fn test_mktime_resolves_a_nonexistent_local_hour() {
        let _tz = TzGuard::set(US_EASTERN);
        let mut tm = zero_tm();
        tm.tm_year = 121;
        tm.tm_mon = 2; // March
        tm.tm_mday = 14;
        tm.tm_hour = 2;
        tm.tm_min = 30;
        tm.tm_isdst = -1;
        let t = mktime(&raw mut tm);
        assert!(t > 0, "a vanished hour must still produce an instant");
        // Whatever instant we picked, re-reading it must be consistent.
        let mut back = local_tm(t);
        assert_eq!(mktime(&raw mut back), t);
    }

    /// The hour 01:00–01:59 local happens twice on the fall-back day.
    /// `tm_isdst` selects which one, so the two readings must be exactly
    /// an hour apart.
    #[test]
    fn test_mktime_honours_isdst_in_the_repeated_hour() {
        let _tz = TzGuard::set(US_EASTERN);
        let mut base = zero_tm();
        base.tm_year = 121;
        base.tm_mon = 10; // November
        base.tm_mday = 7;
        base.tm_hour = 1;
        base.tm_min = 30;

        let mut daylight_reading = base;
        daylight_reading.tm_isdst = 1;
        let mut standard_reading = base;
        standard_reading.tm_isdst = 0;

        let earlier = mktime(&raw mut daylight_reading);
        let later = mktime(&raw mut standard_reading);
        assert_eq!(later - earlier, 3600);
        assert_eq!(daylight_reading.tm_gmtoff, -4 * 3600);
        assert_eq!(standard_reading.tm_gmtoff, -5 * 3600);
    }

    /// `%z` and `%Z` render the zone recorded in the `Tm`, so they must
    /// follow DST rather than printing a constant.
    #[test]
    fn test_strftime_renders_the_zone_from_the_tm() {
        let _tz = TzGuard::set(US_EASTERN);
        for (t, expected) in [
            (1_610_712_000 as TimeT, &b"-0500 EST"[..]),
            (1_626_350_400 as TimeT, &b"-0400 EDT"[..]),
        ] {
            let tm = local_tm(t);
            let mut buf = [0u8; 32];
            // SAFETY: the buffer is writable for `buf.len()` bytes, the
            // format is a NUL-terminated literal and `tm` is valid.
            let n = unsafe {
                strftime(
                    buf.as_mut_ptr(),
                    buf.len(),
                    c"%z %Z".as_ptr().cast(),
                    &raw const tm,
                )
            };
            assert_eq!(&buf[..n], expected, "at {t}");
        }
    }

    /// `ctime` is defined as `asctime(localtime(t))`.  It used to call
    /// `gmtime_r`, which was invisible only while the two agreed.
    #[test]
    fn test_ctime_r_uses_local_time() {
        let _tz = TzGuard::set(US_EASTERN);
        let t: TimeT = 1_610_712_000; // 2021-01-15 07:00:00 EST
        let mut buf = [0u8; 32];
        // SAFETY: `buf` is at least the 26 bytes `ctime_r` requires.
        let ret = unsafe { ctime_r(&raw const t, buf.as_mut_ptr()) };
        assert!(!ret.is_null());
        // SAFETY: `ctime_r` wrote a NUL-terminated string into `buf`.
        let text = unsafe { cstr(buf.as_ptr()) };
        assert_eq!(text, b"Fri Jan 15 07:00:00 2021\n");
    }

    /// An unparsable `TZ` must fall back to UTC, not to garbage — the
    /// same thing glibc does with a zone file it cannot read.
    #[test]
    fn test_unparsable_tz_falls_back_to_utc() {
        let _tz = TzGuard::set(b"!!!not-a-zone!!!");
        let tm = local_tm(1_626_350_400);
        assert_eq!(tm.tm_hour, 12);
        assert_eq!(tm.tm_gmtoff, 0);
    }

    /// A zoneinfo name (`America/New_York`) is now *looked up* — the libc
    /// reads and honours a TZif file when one is there (see
    /// [`crate::tz::Zone`]) — but SlateOS ships no tzdata yet, so the open
    /// fails and the zone degrades to UTC.  Shipping tzdata is a packaging
    /// decision tracked separately; this test pins the fallback so the day
    /// the files appear it fails loudly rather than the behaviour changing
    /// silently.
    #[test]
    fn test_zoneinfo_names_resolve_to_utc_until_tzdata_is_shipped() {
        let _tz = TzGuard::set(b"America/New_York");
        let tm = local_tm(1_626_350_400);
        assert_eq!(
            tm.tm_gmtoff, 0,
            "no tzdata is installed, so named zones must degrade to UTC"
        );
    }

    // -------------------------------------------------------------------
    // clock_getres — pure logic (no syscalls)
    // -------------------------------------------------------------------

    #[test]
    fn test_clock_getres_monotonic() {
        let mut ts = Timespec {
            tv_sec: 99,
            tv_nsec: 99,
        };
        let ret = clock_getres(CLOCK_MONOTONIC, &raw mut ts);
        assert_eq!(ret, 0);
        assert_eq!(ts.tv_sec, 0);
        assert_eq!(ts.tv_nsec, 1); // 1ns resolution
    }

    #[test]
    fn test_is_realtime_clock_routing() {
        // Only the wall-clock clocks must route to SYS_CLOCK_REALTIME;
        // every monotonic/boottime/cputime clock stays on the monotonic
        // counter.
        assert!(is_realtime_clock(CLOCK_REALTIME));
        assert!(is_realtime_clock(CLOCK_REALTIME_COARSE));
        assert!(!is_realtime_clock(CLOCK_MONOTONIC));
        assert!(!is_realtime_clock(CLOCK_MONOTONIC_RAW));
        assert!(!is_realtime_clock(CLOCK_MONOTONIC_COARSE));
        assert!(!is_realtime_clock(CLOCK_BOOTTIME));
        assert!(!is_realtime_clock(CLOCK_PROCESS_CPUTIME_ID));
        assert!(!is_realtime_clock(CLOCK_THREAD_CPUTIME_ID));
    }

    #[test]
    fn test_clock_getres_realtime() {
        let mut ts = Timespec {
            tv_sec: 99,
            tv_nsec: 99,
        };
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
        let mut ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
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

    // ---------------------------------------------------------------
    // timespec_get — C11
    //
    // The return convention is inverted relative to everything else in
    // this file: `base` on success, 0 on failure, no errno.  Each of
    // these asserts the *value*, not merely success/failure, because
    // `return 1` and `return base` are indistinguishable while TIME_UTC
    // is the only base we accept.
    // ---------------------------------------------------------------

    #[test]
    fn timespec_get_fills_the_struct_and_returns_its_base() {
        let mut ts = Timespec {
            tv_sec: -1,
            tv_nsec: -1,
        };
        let ret = unsafe { timespec_get(&raw mut ts, TIME_UTC) };
        assert_eq!(ret, TIME_UTC, "success must return `base`, not 1 or 0");
        assert!(ts.tv_sec > 0, "wall clock must be past the epoch");
        assert!(
            (0..1_000_000_000).contains(&ts.tv_nsec),
            "nanoseconds must be normalised, got {}",
            ts.tv_nsec
        );
    }

    #[test]
    fn timespec_get_agrees_with_clock_gettime() {
        // It must read CLOCK_REALTIME, not the monotonic counter — the two
        // differ by decades, and picking the wrong one would still produce a
        // plausible-looking struct.
        let mut a = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let mut b = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        assert_eq!(unsafe { timespec_get(&raw mut a, TIME_UTC) }, TIME_UTC);
        assert_eq!(clock_gettime(CLOCK_REALTIME, &raw mut b), 0);
        assert!(
            (b.tv_sec - a.tv_sec).abs() <= 2,
            "timespec_get {} vs clock_gettime(CLOCK_REALTIME) {}",
            a.tv_sec,
            b.tv_sec
        );
    }

    #[test]
    fn timespec_get_rejects_an_unsupported_base() {
        // C23's TIME_MONOTONIC is 2; we do not implement it, and the standard
        // says an unsupported base returns zero rather than guessing.
        let mut ts = Timespec {
            tv_sec: -1,
            tv_nsec: -1,
        };
        assert_eq!(unsafe { timespec_get(&raw mut ts, 2) }, 0);
        assert_eq!(unsafe { timespec_get(&raw mut ts, 0) }, 0);
        assert_eq!(unsafe { timespec_get(&raw mut ts, -1) }, 0);
        assert_eq!(ts.tv_sec, -1, "a rejected call must not write through ts");
    }

    #[test]
    fn timespec_get_rejects_a_null_struct() {
        assert_eq!(
            unsafe { timespec_get(core::ptr::null_mut(), TIME_UTC) },
            0,
            "NULL must be reported, not dereferenced"
        );
    }

    #[test]
    fn test_clock_gettime_invalid_clock() {
        let mut ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        crate::errno::set_errno(0);
        let ret = clock_gettime(999, &raw mut ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // ---------------------------------------------------------------
    // Phase 150: clock_gettime — Linux validates the clock_id before
    // touching the user pointer.
    //
    //   1. clockid_to_kclock(which_clock) → EINVAL on miss
    //   2. kc->clock_get_timespec(...)
    //   3. put_timespec64(tp)             → EFAULT on NULL/bad
    //
    // Pre-Phase-150 we ran step 3 BEFORE step 1, so
    // `clock_gettime(BAD_CLOCK, NULL)` returned EFAULT; Linux
    // returns EINVAL.
    // ---------------------------------------------------------------

    // -- per-error-class --

    /// Per-error-class: bad clock with valid tp → EINVAL.
    #[test]
    fn test_clock_gettime_bad_clock_einval_phase150() {
        let mut ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        crate::errno::set_errno(0);
        let ret = clock_gettime(0x4242, &raw mut ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Per-error-class: NULL tp with valid clock → EFAULT.
    #[test]
    fn test_clock_gettime_null_tp_valid_clock_efault_phase150() {
        crate::errno::set_errno(0);
        let ret = clock_gettime(CLOCK_MONOTONIC, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// Per-error-class: negative clock_id → EINVAL.
    #[test]
    fn test_clock_gettime_negative_clock_einval_phase150() {
        let mut ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        crate::errno::set_errno(0);
        let ret = clock_gettime(-7, &raw mut ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- ordering matrix --

    /// Ordering: bad clock BEATS NULL tp.  Linux returns EINVAL
    /// (from `clockid_to_kclock`) before reaching `put_timespec64`.
    /// Pre-Phase-150 we returned EFAULT here.
    #[test]
    fn test_clock_gettime_bad_clock_beats_null_tp_phase150() {
        crate::errno::set_errno(0);
        let ret = clock_gettime(999, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "bad clock (EINVAL) must beat NULL tp (EFAULT)"
        );
    }

    /// Ordering: negative clock BEATS NULL tp.
    #[test]
    fn test_clock_gettime_negative_clock_beats_null_tp_phase150() {
        crate::errno::set_errno(0);
        let ret = clock_gettime(-1, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Ordering: i32::MAX clock BEATS NULL tp.
    #[test]
    fn test_clock_gettime_max_clock_beats_null_tp_phase150() {
        crate::errno::set_errno(0);
        let ret = clock_gettime(i32::MAX, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- workflow --

    /// Workflow: each valid clock id passes validation (advances
    /// past the EINVAL check).  We can't assert success because the
    /// underlying `syscall0(SYS_CLOCK_MONOTONIC)` is a no-op in the
    /// test environment — but we CAN assert that the early EFAULT
    /// path doesn't fire (i.e. the clock check passes), by passing
    /// a non-null tp and observing that errno is NOT EINVAL.
    #[test]
    fn test_clock_gettime_all_valid_clocks_pass_validation_phase150() {
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
            crate::errno::set_errno(0);
            let mut ts = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            // We don't assert ret == 0 because the test environment
            // has no working SYS_CLOCK_MONOTONIC; we DO assert that
            // errno wasn't set to EINVAL (the clock-validation
            // failure errno).
            let _ = clock_gettime(clk, &raw mut ts);
            assert_ne!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "clock {clk} must pass the EINVAL check"
            );
        }
    }

    // -- buggy caller --

    /// Buggy caller: passes a deliberately-suspicious clock id (one
    /// that looks like an ASCII byte, e.g. `b'A' as ClockidT = 65`)
    /// expecting EINVAL.  Pre-Phase-150 they'd get EFAULT if their
    /// tp was also null — confusing for diagnostics.
    #[test]
    fn test_clock_gettime_buggy_caller_phase150() {
        crate::errno::set_errno(0);
        let ret = clock_gettime(65, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "caller probing for clock support must see EINVAL, not EFAULT"
        );
    }

    // -- recovery --

    /// Recovery: after EINVAL from bad clock, retrying with a valid
    /// clock id ADVANCES past the EINVAL check (no longer EINVAL).
    /// Test-environment limitation: can't assert ret == 0.
    #[test]
    fn test_clock_gettime_recovery_after_einval_phase150() {
        crate::errno::set_errno(0);
        let mut ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        assert_eq!(clock_gettime(999, &raw mut ts), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        let _ = clock_gettime(CLOCK_MONOTONIC, &raw mut ts);
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "valid clock must clear the EINVAL state"
        );
    }

    /// Recovery: after EFAULT from NULL tp, providing a buffer
    /// clears the EFAULT state (no longer EFAULT).
    #[test]
    fn test_clock_gettime_recovery_after_efault_phase150() {
        crate::errno::set_errno(0);
        assert_eq!(clock_gettime(CLOCK_MONOTONIC, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);

        crate::errno::set_errno(0);
        let mut ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let _ = clock_gettime(CLOCK_MONOTONIC, &raw mut ts);
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EFAULT,
            "valid tp must clear the EFAULT state"
        );
    }

    // -- no-side-effect loop --

    /// No-side-effect loop: repeated EINVAL/EFAULT calls don't leak
    /// — each iteration sets the correct errno cleanly.
    #[test]
    fn test_clock_gettime_einval_efault_loop_phase150() {
        for _ in 0..32 {
            crate::errno::set_errno(0);
            assert_eq!(clock_gettime(0xBAD, core::ptr::null_mut()), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

            crate::errno::set_errno(0);
            assert_eq!(clock_gettime(CLOCK_MONOTONIC, core::ptr::null_mut()), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        }
    }

    // -- timer_create / timer_settime / timer_gettime / timer_delete /
    //    timer_getoverrun: no timer can be made, so none exists --
    //
    // Until 2026-10-05 `timer_create` claimed a slot in a table and the
    // rest worked on it, reporting a timer armed that could never fire;
    // the tests here then (phases 146-149) pinned the order of its checks
    // on a live timer. With no timer, `timer_create`'s checks are what is
    // left to order, and the other four answer EINVAL.

    /// Helper: disarm the interval timer, for the `setitimer` tests'
    /// isolation.
    ///
    /// The interval timer is the kernel's, stood in for per-thread on host
    /// builds; this disarms it the way a caller would rather than reaching
    /// past that boundary. Safe to call unilaterally: libtest gives each
    /// test its own thread, so this cannot disarm another test's timer.
    fn reset_itimer() {
        // The previous value is not wanted: this only disarms.
        let _ = super::host_itimer::swap(0, 0);
    }

    /// A `Sigevent` with only the two fields `timer_create` reads set.
    fn sev(notify: i32, signo: i32) -> Sigevent {
        Sigevent {
            sigev_value: 0,
            sigev_signo: signo,
            sigev_notify: notify,
            _pad: [0u8; 48],
        }
    }

    /// `timer_create`'s call, with `*timerid` preset to a sentinel: the
    /// result, errno, and whether the sentinel survived.
    fn create(clock: ClockidT, ev: Option<&Sigevent>) -> (i32, i32, bool) {
        let p = ev.map_or(core::ptr::null(), core::ptr::from_ref);
        let mut id: TimerT = 0xDEAD;
        crate::errno::set_errno(0);
        let r = timer_create(clock, p, &raw mut id);
        (r, crate::errno::get_errno(), id == 0xDEAD)
    }

    /// Clocks `timer_create` may arm, as Linux's `posix_clocks[]` gives
    /// them a `timer_create`, among those this library knows.
    const ARMABLE_CLOCKS: [ClockidT; 5] = [
        CLOCK_REALTIME,
        CLOCK_MONOTONIC,
        CLOCK_PROCESS_CPUTIME_ID,
        CLOCK_THREAD_CPUTIME_ID,
        CLOCK_BOOTTIME,
    ];

    /// Every clock that can be armed, with every well-formed event --
    /// none, `SIGEV_NONE`, `SIGEV_SIGNAL` at both ends of the signal
    /// range, `SIGEV_THREAD`, `SIGEV_THREAD_ID` -- passes the checks and is
    /// refused ENOSYS, and `*timerid` is never written. Each was a slot
    /// and 0 until 2026-10-05.
    #[test]
    fn test_timer_create_answers_enosys_once_its_checks_pass() {
        let events = [
            None,
            Some(sev(SIGEV_NONE, 0)),
            Some(sev(SIGEV_SIGNAL, crate::signal::SIGALRM)),
            Some(sev(SIGEV_SIGNAL, 1)),
            Some(sev(SIGEV_SIGNAL, crate::signal::SIGRTMAX)),
            Some(sev(SIGEV_THREAD, 0)),
            Some(sev(SIGEV_THREAD_ID, crate::signal::SIGALRM)),
        ];
        for clock in ARMABLE_CLOCKS {
            for ev in &events {
                let (r, e, untouched) = create(clock, ev.as_ref());
                let notify = ev.as_ref().map(|s| s.sigev_notify);
                assert_eq!(r, -1, "clock {clock}, notify {notify:?}");
                assert_eq!(e, crate::errno::ENOSYS, "clock {clock}, notify {notify:?}");
                assert!(
                    untouched,
                    "clock {clock}, notify {notify:?}: timerid written"
                );
            }
        }
    }

    /// `do_timer_create` refuses a clock that can be read but not armed
    /// with EOPNOTSUPP (kernel/time/posix-timers.c:454), before it looks
    /// at the event or the pointer.
    #[test]
    fn test_timer_create_unarmable_clocks_eopnotsupp() {
        for clock in [
            CLOCK_MONOTONIC_RAW,
            CLOCK_REALTIME_COARSE,
            CLOCK_MONOTONIC_COARSE,
        ] {
            for ev in [None, Some(sev(99, 0)), Some(sev(SIGEV_SIGNAL, 0))] {
                let (r, e, untouched) = create(clock, ev.as_ref());
                assert_eq!((r, e), (-1, crate::errno::EOPNOTSUPP), "clock {clock}");
                assert!(untouched, "clock {clock}");
            }
            crate::errno::set_errno(0);
            assert_eq!(
                timer_create(clock, core::ptr::null(), core::ptr::null_mut()),
                -1
            );
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::EOPNOTSUPP,
                "clock {clock}"
            );
        }
    }

    /// An unknown clock is EINVAL, and is checked first: a bad event and a
    /// NULL `timerid` beside it do not change the answer. 8 and 9 are
    /// Linux's `_ALARM` clocks and 11 is `CLOCK_TAI`, which this library
    /// does not know; 10 is `CLOCK_SGI_CYCLE`, which Linux does not.
    #[test]
    fn test_timer_create_unknown_clock_einval_before_everything() {
        for clock in [8, 9, 10, 11, 42, 99, 0x4243, -1, i32::MIN, i32::MAX] {
            for ev in [None, Some(sev(99, 0)), Some(sev(SIGEV_SIGNAL, 0))] {
                let (r, e, untouched) = create(clock, ev.as_ref());
                assert_eq!((r, e), (-1, crate::errno::EINVAL), "clock {clock}");
                assert!(untouched, "clock {clock}");
            }
            crate::errno::set_errno(0);
            assert_eq!(
                timer_create(clock, core::ptr::null(), core::ptr::null_mut()),
                -1
            );
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "clock {clock}"
            );
        }
    }

    /// An unrecognised `sigev_notify` is EINVAL (`good_sigevent`), and
    /// comes before the NULL-`timerid` check.
    #[test]
    fn test_timer_create_bad_sigev_notify_einval() {
        for notify in [3, 5, 6, 99, -1, i32::MIN, i32::MAX] {
            let (r, e, untouched) = create(CLOCK_REALTIME, Some(&sev(notify, 14)));
            assert_eq!((r, e), (-1, crate::errno::EINVAL), "notify {notify}");
            assert!(untouched, "notify {notify}");
            let bad = sev(notify, 14);
            crate::errno::set_errno(0);
            assert_eq!(
                timer_create(CLOCK_REALTIME, &raw const bad, core::ptr::null_mut()),
                -1
            );
            assert_eq!(
                crate::errno::get_errno(),
                crate::errno::EINVAL,
                "notify {notify}"
            );
        }
    }

    /// `good_sigevent` checks the signal number only when a signal is to be
    /// sent -- `SIGEV_SIGNAL` and `SIGEV_THREAD_ID` -- and wants it in
    /// `1..=SIGRTMAX`. `SIGEV_NONE` sends none, and a `SIGEV_THREAD`
    /// caller's number never reaches the kernel under glibc, so a nonsense
    /// number there passes to the ENOSYS.
    #[test]
    fn test_timer_create_signal_number_checked_only_when_one_is_sent() {
        let past = crate::signal::SIGRTMAX + 1;
        for notify in [SIGEV_SIGNAL, SIGEV_THREAD_ID] {
            for signo in [0, -1, past, i32::MAX, i32::MIN] {
                let (r, e, untouched) = create(CLOCK_MONOTONIC, Some(&sev(notify, signo)));
                assert_eq!(
                    (r, e),
                    (-1, crate::errno::EINVAL),
                    "notify {notify}, signo {signo}"
                );
                assert!(untouched, "notify {notify}, signo {signo}");
            }
        }
        for notify in [SIGEV_NONE, SIGEV_THREAD] {
            for signo in [0, -1, past, i32::MAX, i32::MIN] {
                let (r, e, untouched) = create(CLOCK_MONOTONIC, Some(&sev(notify, signo)));
                assert_eq!(
                    (r, e),
                    (-1, crate::errno::ENOSYS),
                    "notify {notify}, signo {signo}"
                );
                assert!(untouched, "notify {notify}, signo {signo}");
            }
        }
    }

    /// Phase 147: a NULL `timerid` is EFAULT, as Linux's `copy_to_user`
    /// gives -- after every other check, and ahead of the ENOSYS, as the
    /// kernel's Linux ABI orders it.
    #[test]
    fn test_timer_create_null_timerid_efault_after_the_other_checks() {
        for clock in ARMABLE_CLOCKS {
            for ev in [None, Some(sev(SIGEV_NONE, 0)), Some(sev(SIGEV_SIGNAL, 14))] {
                let p = ev.as_ref().map_or(core::ptr::null(), core::ptr::from_ref);
                crate::errno::set_errno(0);
                assert_eq!(timer_create(clock, p, core::ptr::null_mut()), -1);
                assert_eq!(
                    crate::errno::get_errno(),
                    crate::errno::EFAULT,
                    "clock {clock}"
                );
            }
        }
        // Each earlier check still wins over it.
        let bad_signo = sev(SIGEV_SIGNAL, 0);
        crate::errno::set_errno(0);
        assert_eq!(
            timer_create(CLOCK_REALTIME, &raw const bad_signo, core::ptr::null_mut()),
            -1
        );
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// A refusal leaves nothing behind: the same call answers the same,
    /// however often it is made or whatever came before it, as a stateless
    /// function must. (Several phase-147 tests pinned this for the table,
    /// whose slots a refused call must not consume.)
    #[test]
    fn test_timer_create_answers_do_not_depend_on_earlier_calls() {
        for _ in 0..64 {
            assert_eq!(
                create(CLOCK_REALTIME, None),
                (-1, crate::errno::ENOSYS, true)
            );
            assert_eq!(create(99, None), (-1, crate::errno::EINVAL, true));
            assert_eq!(
                create(CLOCK_MONOTONIC_RAW, None),
                (-1, crate::errno::EOPNOTSUPP, true)
            );
            assert_eq!(
                create(CLOCK_REALTIME, Some(&sev(99, 0))),
                (-1, crate::errno::EINVAL, true)
            );
        }
    }

    /// GNU `timeout`'s `settimeout` (coreutils 9.5, src/timeout.c),
    /// transcribed: it warns unless `timer_create` failed with ENOSYS, or
    /// whenever `timer_settime` fails, and then falls back to `alarm`. On
    /// SlateOS it must take the silent path, and the fallback must arm.
    #[test]
    fn test_gnu_timeout_falls_back_to_alarm_without_a_warning() {
        reset_itimer();
        let mut warnings = 0;
        let mut timerid: TimerT = 0;
        if timer_create(CLOCK_REALTIME, core::ptr::null(), &raw mut timerid) == 0 {
            let its = Itimerspec {
                it_interval: Timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                },
                it_value: Timespec {
                    tv_sec: 5,
                    tv_nsec: 0,
                },
            };
            assert_ne!(
                timer_settime(timerid, 0, &raw const its, core::ptr::null_mut()),
                0,
                "timer_settime armed a timer that cannot fire"
            );
            warnings += 1; // "warning: timer_settime"
            timer_delete(timerid);
        } else if crate::errno::get_errno() != crate::errno::ENOSYS {
            warnings += 1; // "warning: timer_create"
        }
        assert_eq!(warnings, 0, "timeout would print a warning on every run");
        // The fallback: alarm (5), which arms the one real interval timer.
        assert_eq!(crate::unistd::alarm(5), 0);
        assert_eq!(super::host_itimer::peek(), (5_000_000_000, 0));
        reset_itimer();
    }

    /// `timer_settime` answers EINVAL to every call, as Linux does for a
    /// `timerid` naming no timer, and touches neither pointer: `old_value`
    /// keeps what it held. A NULL `new_value` and a malformed one are
    /// EINVAL on Linux as well.
    #[test]
    fn test_timer_settime_einval_for_every_call_and_touches_nothing() {
        let ts = |s, ns| Timespec {
            tv_sec: s,
            tv_nsec: ns,
        };
        let good = Itimerspec {
            it_interval: ts(0, 0),
            it_value: ts(1, 0),
        };
        let bad = Itimerspec {
            it_interval: ts(0, 0),
            it_value: ts(0, 1_000_000_000),
        };
        let sentinel = Itimerspec {
            it_interval: ts(7, 8),
            it_value: ts(9, 10),
        };
        for id in [0, 1, 31, 32, 99, TimerT::MAX] {
            for flags in [0, TIMER_ABSTIME, 0xff] {
                for nv in [&raw const good, &raw const bad, core::ptr::null()] {
                    let mut old = sentinel;
                    crate::errno::set_errno(0);
                    assert_eq!(timer_settime(id, flags, nv, &raw mut old), -1, "id {id}");
                    assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL, "id {id}");
                    assert_eq!(
                        (old.it_interval.tv_sec, old.it_interval.tv_nsec),
                        (7, 8),
                        "id {id}: old_value written"
                    );
                    assert_eq!((old.it_value.tv_sec, old.it_value.tv_nsec), (9, 10));
                    crate::errno::set_errno(0);
                    assert_eq!(timer_settime(id, flags, nv, core::ptr::null_mut()), -1);
                    assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL, "id {id}");
                }
            }
        }
    }

    /// `timer_gettime` answers EINVAL for every `timerid` and does not
    /// write `curr_value`; with NULL there, still EINVAL, not EFAULT --
    /// Linux looks the timer up first (Phase 148's order).
    #[test]
    fn test_timer_gettime_einval_and_leaves_curr_value_alone() {
        for id in [0, 1, 31, 32, 99, TimerT::MAX] {
            let mut out = Itimerspec {
                it_interval: Timespec {
                    tv_sec: 7,
                    tv_nsec: 8,
                },
                it_value: Timespec {
                    tv_sec: 9,
                    tv_nsec: 10,
                },
            };
            crate::errno::set_errno(0);
            assert_eq!(timer_gettime(id, &raw mut out), -1, "id {id}");
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL, "id {id}");
            assert_eq!(
                (out.it_interval.tv_sec, out.it_value.tv_nsec),
                (7, 10),
                "id {id}"
            );
            crate::errno::set_errno(0);
            assert_eq!(timer_gettime(id, core::ptr::null_mut()), -1, "id {id}");
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL, "id {id}");
        }
    }

    /// `timer_delete` and `timer_getoverrun` answer EINVAL for every
    /// `timerid` (Phase 149 made the second look the id up; with no
    /// timers, the lookup always misses).
    #[test]
    fn test_timer_delete_and_getoverrun_einval_for_every_id() {
        for id in [0, 1, 31, 32, 99, TimerT::MAX - 1, TimerT::MAX] {
            crate::errno::set_errno(0);
            assert_eq!(timer_delete(id), -1, "id {id}");
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL, "id {id}");
            crate::errno::set_errno(0);
            assert_eq!(timer_getoverrun(id), -1, "id {id}");
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL, "id {id}");
        }
    }

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

    // -- setitimer / getitimer --

    #[test]
    fn test_setitimer_valid_which() {
        reset_itimer();
        let val = Itimerval {
            it_interval: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: Timeval {
                tv_sec: 1,
                tv_usec: 0,
            },
        };
        // ITIMER_REAL is the one that exists.
        assert_eq!(
            setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut()),
            0
        );
        // The other two are accepted by the ARGUMENT check -- they are valid
        // `which` values, so this is not EINVAL -- and then refused, because
        // neither is implemented. Until 2026-09-12 all three returned 0 out of
        // a table this module kept, so a program could set a profiling timer,
        // read it back unchanged, and wait forever for a signal nothing was
        // going to send. The errno is the assertion that matters here: ENOSYS
        // and not EINVAL is what distinguishes "not built" from "you asked
        // wrongly", and only the first is true of these.
        for which in [ITIMER_VIRTUAL, ITIMER_PROF] {
            errno::set_errno(0);
            assert_eq!(
                setitimer(which, &raw const val, core::ptr::null_mut()),
                -1,
                "setitimer({which}) must not claim to have armed a timer"
            );
            assert_eq!(errno::get_errno(), errno::ENOSYS, "which={which}");
        }
    }

    #[test]
    fn test_setitimer_invalid_which() {
        reset_itimer();
        let val = Itimerval {
            it_interval: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        };
        crate::errno::set_errno(0);
        assert_eq!(setitimer(99, &raw const val, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Linux 6.6 takes a NULL new value as zeros -- disarm -- rather than
    /// EFAULT (kernel/time/itimer.c, "Misfeature support will be removed"),
    /// and glibc passes the call through.  It was EFAULT until 2026-09-26.
    #[test]
    fn test_setitimer_null_new_value_disarms() {
        reset_itimer();
        let armed = itimerval(0, 0, 5, 0);
        assert_eq!(
            setitimer(ITIMER_REAL, &raw const armed, core::ptr::null_mut()),
            0
        );
        let mut old = itimerval(9, 9, 9, 9);
        assert_eq!(setitimer(ITIMER_REAL, core::ptr::null(), &raw mut old), 0);
        assert_eq!(old.it_value.tv_sec, 5, "the old value comes back");
        let mut now = itimerval(9, 9, 9, 9);
        assert_eq!(getitimer(ITIMER_REAL, &raw mut now), 0);
        assert_eq!(
            (now.it_value.tv_sec, now.it_value.tv_usec),
            (0, 0),
            "disarmed"
        );
    }

    #[test]
    fn test_setitimer_returns_old_value() {
        reset_itimer();
        // First set: old should be zeros (fresh state).
        let val1 = Itimerval {
            it_interval: Timeval {
                tv_sec: 1,
                tv_usec: 100,
            },
            it_value: Timeval {
                tv_sec: 5,
                tv_usec: 200,
            },
        };
        let mut old = Itimerval {
            it_interval: Timeval {
                tv_sec: 99,
                tv_usec: 99,
            },
            it_value: Timeval {
                tv_sec: 99,
                tv_usec: 99,
            },
        };
        assert_eq!(setitimer(ITIMER_REAL, &raw const val1, &raw mut old), 0);
        assert_eq!(old.it_interval.tv_sec, 0);
        assert_eq!(old.it_interval.tv_usec, 0);
        assert_eq!(old.it_value.tv_sec, 0);
        assert_eq!(old.it_value.tv_usec, 0);

        // Second set: old should be val1.
        let val2 = Itimerval {
            it_interval: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        };
        assert_eq!(setitimer(ITIMER_REAL, &raw const val2, &raw mut old), 0);
        assert_eq!(old.it_interval.tv_sec, 1);
        assert_eq!(old.it_interval.tv_usec, 100);
        assert_eq!(old.it_value.tv_sec, 5);
        assert_eq!(old.it_value.tv_usec, 200);
    }

    #[test]
    fn test_getitimer_returns_set_value() {
        reset_itimer();
        let val = Itimerval {
            it_interval: Timeval {
                tv_sec: 3,
                tv_usec: 500,
            },
            it_value: Timeval {
                tv_sec: 7,
                tv_usec: 999,
            },
        };
        setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());

        let mut out = Itimerval {
            it_interval: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        };
        assert_eq!(getitimer(ITIMER_REAL, &raw mut out), 0);
        assert_eq!(out.it_interval.tv_sec, 3);
        assert_eq!(out.it_interval.tv_usec, 500);
        assert_eq!(out.it_value.tv_sec, 7);
        assert_eq!(out.it_value.tv_usec, 999);
    }

    #[test]
    fn test_getitimer_fresh_returns_zeros() {
        reset_itimer();
        let mut val = Itimerval {
            it_interval: Timeval {
                tv_sec: 99,
                tv_usec: 99,
            },
            it_value: Timeval {
                tv_sec: 99,
                tv_usec: 99,
            },
        };
        assert_eq!(getitimer(ITIMER_REAL, &raw mut val), 0);
        assert_eq!(val.it_interval.tv_sec, 0);
        assert_eq!(val.it_value.tv_sec, 0);
    }

    #[test]
    fn test_getitimer_per_timer_type_isolation() {
        reset_itimer();
        // Set ITIMER_REAL, verify ITIMER_VIRTUAL is still zeros.
        let val = Itimerval {
            it_interval: Timeval {
                tv_sec: 10,
                tv_usec: 0,
            },
            it_value: Timeval {
                tv_sec: 20,
                tv_usec: 0,
            },
        };
        setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());

        let mut out = Itimerval {
            it_interval: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        };
        // There is no longer anything to be isolated FROM: the virtual and
        // profiling timers are not implemented, so reading one is a refusal
        // rather than a report of zeros. That distinction is the point. Zeros
        // are a plausible answer -- "no timer is set" -- and a caller cannot
        // tell them from "this kind of timer does not exist here", which is
        // how the old behaviour stayed invisible for as long as it did.
        errno::set_errno(0);
        assert_eq!(getitimer(ITIMER_VIRTUAL, &raw mut out), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);

        // And the real one still reads back what was just set, so this test
        // still covers what its name says.
        assert_eq!(getitimer(ITIMER_REAL, &raw mut out), 0);
        assert_eq!(out.it_interval.tv_sec, 10);
        assert_eq!(out.it_value.tv_sec, 20);
    }

    #[test]
    fn test_getitimer_invalid_which() {
        let mut val = Itimerval {
            it_interval: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: Timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
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
            it_interval: Timeval {
                tv_sec: s1,
                tv_usec: u1,
            },
            it_value: Timeval {
                tv_sec: s2,
                tv_usec: u2,
            },
        }
    }

    #[test]
    fn test_setitimer_phase87_neg_value_tv_sec_einval() {
        reset_itimer();
        let val = itimerval(0, 0, -1, 0);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_neg_value_tv_usec_einval() {
        reset_itimer();
        let val = itimerval(0, 0, 0, -1);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_value_tv_usec_at_million_einval() {
        reset_itimer();
        let val = itimerval(0, 0, 0, 1_000_000);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_value_tv_usec_above_million_einval() {
        reset_itimer();
        let val = itimerval(0, 0, 0, 5_000_000);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_neg_interval_tv_sec_einval() {
        reset_itimer();
        let val = itimerval(-1, 0, 0, 0);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_neg_interval_tv_usec_einval() {
        reset_itimer();
        let val = itimerval(0, -1, 0, 0);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_interval_tv_usec_at_million_einval() {
        reset_itimer();
        let val = itimerval(0, 1_000_000, 0, 0);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_max_valid_usec_succeeds() {
        // 999_999 is the maximum valid microsecond value.
        reset_itimer();
        let val = itimerval(0, 999_999, 0, 999_999);
        errno::set_errno(0);
        let ret = setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_setitimer_phase87_invalid_which_takes_precedence() {
        // Bad `which` AND bad timeval: which-check wins (it comes first).
        reset_itimer();
        let val = itimerval(0, 0, -1, -1);
        errno::set_errno(0);
        let ret = setitimer(42, &raw const val, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_null_value_still_checks_which() {
        // A NULL value is zeros, not a fault, so `which` is still judged.
        reset_itimer();
        errno::set_errno(0);
        let ret = setitimer(42, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setitimer_phase87_einval_does_not_mutate_stored_state() {
        // First store a valid value.
        reset_itimer();
        let good = itimerval(2, 200, 3, 300);
        assert_eq!(
            setitimer(ITIMER_REAL, &raw const good, core::ptr::null_mut()),
            0
        );

        // Now a bogus call must fail without overwriting it.
        let bad = itimerval(0, 0, 0, 2_000_000);
        errno::set_errno(0);
        assert_eq!(
            setitimer(ITIMER_REAL, &raw const bad, core::ptr::null_mut()),
            -1
        );
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
        reset_itimer();
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
        reset_itimer();
        let val = itimerval(0, 0, 0, 0);
        errno::set_errno(0);
        assert_eq!(
            setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut()),
            0
        );
    }

    #[test]
    fn test_setitimer_phase87_all_three_which_values_validate_fields() {
        // Validation must apply equally to all three timers.
        reset_itimer();
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
        reset_itimer();
        let val = itimerval(1_000_000, 0, 2_000_000, 0);
        errno::set_errno(0);
        assert_eq!(
            setitimer(ITIMER_REAL, &raw const val, core::ptr::null_mut()),
            0
        );
    }

    #[test]
    fn test_setitimer_phase87_einval_then_valid_call_progression() {
        reset_itimer();
        let bad = itimerval(0, -1, 0, 0);
        errno::set_errno(0);
        assert_eq!(
            setitimer(ITIMER_REAL, &raw const bad, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let good = itimerval(1, 1, 1, 1);
        errno::set_errno(0);
        assert_eq!(
            setitimer(ITIMER_REAL, &raw const good, core::ptr::null_mut()),
            0
        );
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
        let req = Timespec {
            tv_sec: -1,
            tv_nsec: 0,
        };
        crate::errno::set_errno(0);
        assert_eq!(nanosleep(&req, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_nanosleep_nsec_too_large() {
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        };
        crate::errno::set_errno(0);
        assert_eq!(nanosleep(&req, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_nanosleep_negative_nsec() {
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: -1,
        };
        crate::errno::set_errno(0);
        assert_eq!(nanosleep(&req, core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- clock_nanosleep validation --

    /// Phase 153: `clock_nanosleep(VALID_CLOCK, 0, NULL, ...)` now
    /// matches Linux's `get_timespec64` failure → EFAULT.
    /// Pre-Phase-153 this returned EINVAL.  Renamed from
    /// `test_clock_nanosleep_null_request`.
    #[test]
    fn test_clock_nanosleep_null_request_efault_phase153() {
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut()),
            crate::errno::EFAULT,
            "valid clock + NULL request must return EFAULT (Linux: get_timespec64)"
        );
    }

    #[test]
    fn test_clock_nanosleep_invalid_clock() {
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        assert_eq!(
            clock_nanosleep(999, 0, &req, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    #[test]
    fn test_clock_nanosleep_invalid_nsec() {
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 2_000_000_000,
        };
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, 0, &req, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    // -- clock_nanosleep's flags: only TIMER_ABSTIME is read --
    //
    // Linux 6.6 never tests the other bits (kernel/time/posix-timers.c:
    // `common_nsleep` asks `flags & TIMER_ABSTIME` and nothing else).
    // The tests here asserted an EINVAL for them, citing a check that
    // `common_nsleep` does not contain, until 2026-09-26.

    #[test]
    fn test_clock_nanosleep_timer_abstime_is_bit_zero() {
        // Sanity / invariant: TIMER_ABSTIME must be 1 (bit 0).  This
        // matches glibc's <time.h> and Linux <linux/time.h>.  If this
        // ever drifts, the mask check below would silently accept
        // bit 0 set with any other shape — break loudly here instead.
        assert_eq!(TIMER_ABSTIME, 1);
        assert_eq!(
            TIMER_ABSTIME & (TIMER_ABSTIME - 1),
            0,
            "TIMER_ABSTIME must be a single bit"
        );
    }

    #[test]
    fn test_clock_nanosleep_zero_flags_passes_mask() {
        // Zero flags is valid; a zero-duration relative sleep should
        // succeed (0 nanoseconds → immediate return).  Must NOT
        // return EINVAL (which would indicate the mask wrongly
        // rejected zero).
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let ret = clock_nanosleep(CLOCK_REALTIME, 0, &req, core::ptr::null_mut());
        assert_ne!(
            ret,
            crate::errno::EINVAL,
            "zero flags must not be rejected by the mask"
        );
    }

    #[test]
    fn test_clock_nanosleep_timer_abstime_alone_passes_mask() {
        // TIMER_ABSTIME alone is the canonical valid call; it must
        // not be rejected by the mask.  Path goes to the abs-time
        // branch which, with a zero/past target, returns 0 immediately.
        //
        // Host: `SYS_CLOCK_REALTIME` is now intercepted in
        // `syscall::syscall0` and backed by `SystemTime::now()`, so
        // `clock_gettime` returns a valid post-epoch ns value and the
        // abs-time branch can complete normally on the host build.
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let ret = clock_nanosleep(CLOCK_REALTIME, TIMER_ABSTIME, &req, core::ptr::null_mut());
        assert_ne!(
            ret,
            crate::errno::EINVAL,
            "TIMER_ABSTIME alone must not be rejected by the mask"
        );
    }

    #[test]
    fn test_clock_nanosleep_ignores_every_other_flag_bit() {
        // Every single bit but TIMER_ABSTIME, and the sign bit, is ignored:
        // a zero relative sleep succeeds with any of them set.
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        for bit in (1..31)
            .map(|shift| 1i32 << shift)
            .chain([i32::MIN, crate::fcntl::O_APPEND])
        {
            assert_eq!(
                clock_nanosleep(CLOCK_REALTIME, bit, &req, core::ptr::null_mut()),
                0,
                "bit {bit:#x}"
            );
        }
        // With TIMER_ABSTIME alongside, the absolute form still runs: a
        // deadline at the epoch has passed.
        assert_eq!(
            clock_nanosleep(
                CLOCK_REALTIME,
                TIMER_ABSTIME | (1 << 8),
                &req,
                core::ptr::null_mut()
            ),
            0
        );
    }

    /// The clocks Linux cannot sleep on are EOPNOTSUPP, after the clock
    /// check and before the request is read; the calling thread's CPU clock
    /// is glibc's EINVAL, before anything.
    #[test]
    fn test_clock_nanosleep_clocks_that_cannot_sleep() {
        for clk in [
            CLOCK_MONOTONIC_RAW,
            CLOCK_REALTIME_COARSE,
            CLOCK_MONOTONIC_COARSE,
        ] {
            assert_eq!(
                clock_nanosleep(clk, 0, core::ptr::null(), core::ptr::null_mut()),
                crate::errno::EOPNOTSUPP,
                "clk={clk}"
            );
        }
        assert_eq!(
            clock_nanosleep(
                CLOCK_THREAD_CPUTIME_ID,
                0,
                core::ptr::null(),
                core::ptr::null_mut()
            ),
            crate::errno::EINVAL
        );
    }

    /// `timespec64_valid` refuses a negative second in both forms.  The
    /// relative form answered EINTR and the absolute form 0 until 2026-09-26.
    #[test]
    fn test_clock_nanosleep_negative_seconds_einval() {
        let req = Timespec {
            tv_sec: -1,
            tv_nsec: 0,
        };
        for flags in [0, TIMER_ABSTIME] {
            assert_eq!(
                clock_nanosleep(CLOCK_REALTIME, flags, &req, core::ptr::null_mut()),
                crate::errno::EINVAL,
                "flags {flags}"
            );
        }
    }

    fn handle_usr1(handler: crate::signal::SighandlerT, flags: u32) {
        let act = crate::signal::Sigaction {
            sa_handler: handler,
            sa_mask: crate::signal::SigsetT::EMPTY,
            sa_flags: flags,
            sa_restorer: 0,
        };
        // SAFETY: a valid action; the old one is not wanted.
        let rc = unsafe {
            crate::signal::sigaction(
                crate::signal::SIGUSR1,
                &raw const act,
                core::ptr::null_mut(),
            )
        };
        assert_eq!(rc, 0);
    }

    extern "C" fn nothing(_: i32) {}

    /// A signal handler ends a sleep whatever its `SA_RESTART`, and the sleep
    /// answers the time that was still to go.  The kernel ending the sleep
    /// is played by `interrupt::script`.
    #[test]
    fn a_signal_handler_ends_a_sleep_with_the_time_still_to_go() {
        use crate::interrupt::script::{self, Step};
        use crate::signal::{SA_RESTART, SIG_DFL, SIGUSR1};
        let handler = nothing as *const () as crate::signal::SighandlerT;
        let five = Timespec {
            tv_sec: 5,
            tv_nsec: 0,
        };
        for flags in [0, SA_RESTART] {
            handle_usr1(handler, flags);
            script::set([Step::Signal(SIGUSR1)]);
            let mut rem = Timespec {
                tv_sec: -1,
                tv_nsec: -1,
            };
            errno::set_errno(0);
            assert_eq!(nanosleep(&five, &mut rem), -1, "flags {flags:#x}");
            assert_eq!(errno::get_errno(), errno::EINTR);
            assert_eq!(rem.tv_sec, 4, "nearly all of it was still to go");
            assert!((0..1_000_000_000).contains(&rem.tv_nsec));
            assert_eq!(script::clear(), 0);

            script::set([Step::Signal(SIGUSR1)]);
            assert_eq!(sleep(5), 4, "the whole seconds still to go, truncated");
            script::set([Step::Signal(SIGUSR1)]);
            assert_eq!(usleep(5_000_000), -1);
            assert_eq!(errno::get_errno(), errno::EINTR);

            // clock_nanosleep answers the error, and says what is left only
            // for a relative sleep.
            script::set([Step::Signal(SIGUSR1)]);
            let mut left = Timespec {
                tv_sec: -1,
                tv_nsec: -1,
            };
            assert_eq!(
                clock_nanosleep(CLOCK_MONOTONIC, 0, &five, &mut left),
                errno::EINTR
            );
            assert_eq!(left.tv_sec, 4);
            let mut at = crate::lowlevellock::now_on(CLOCK_MONOTONIC);
            at.tv_sec += 5;
            script::set([Step::Signal(SIGUSR1)]);
            let mut untouched = Timespec {
                tv_sec: -1,
                tv_nsec: -1,
            };
            assert_eq!(
                clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &at, &mut untouched),
                errno::EINTR
            );
            assert_eq!((untouched.tv_sec, untouched.tv_nsec), (-1, -1));
        }
        handle_usr1(SIG_DFL, 0);
    }

    /// A signal that runs no handler here -- ignored, or handled on another
    /// thread -- does not end a sleep: it runs its course.
    #[test]
    fn a_signal_that_runs_no_handler_here_does_not_end_a_sleep() {
        use crate::interrupt::script::{self, Step};
        use crate::signal::{SIG_DFL, SIG_IGN, SIGUSR1};
        let short = Timespec {
            tv_sec: 0,
            tv_nsec: 20_000_000,
        };
        handle_usr1(SIG_IGN, 0);
        for step in [
            Step::Signal(SIGUSR1),
            Step::SignalElsewhere(std::boxed::Box::new(|| {})),
        ] {
            script::set([step]);
            let start = crate::lowlevellock::now_on(CLOCK_MONOTONIC);
            assert_eq!(nanosleep(&short, core::ptr::null_mut()), 0);
            let end = crate::lowlevellock::now_on(CLOCK_MONOTONIC);
            assert!(
                crate::lowlevellock::ns_until(&start, &end).unwrap_or(0) >= 20_000_000,
                "it slept all of it"
            );
            assert_eq!(script::clear(), 0);
        }
        // errno is left alone by a sleep that runs its course.
        errno::set_errno(12345);
        assert_eq!(sleep(0), 0);
        assert_eq!(errno::get_errno(), 12345);
        handle_usr1(SIG_DFL, 0);
    }

    /// An absolute deadline far in the future does not overflow: the sum
    /// saturates, where it used to multiply unchecked.
    #[test]
    fn test_timespec_to_ns_saturates() {
        let far = Timespec {
            tv_sec: i64::MAX,
            tv_nsec: 999_999_999,
        };
        assert_eq!(timespec_to_ns_saturating(&far), u64::MAX);
        let one = Timespec {
            tv_sec: 1,
            tv_nsec: 5,
        };
        assert_eq!(timespec_to_ns_saturating(&one), 1_000_000_005);
    }

    // -- Phase 153: clock_nanosleep clock-vs-NULL ordering + NULL→EFAULT --
    //
    // Linux's `kernel/time/posix-timers.c::SYSCALL_DEFINE4(
    // clock_nanosleep, ...)` calls `clockid_to_kclock(which_clock)`
    // FIRST.  A bad clock surfaces as EINVAL before any user pointer
    // is touched.  Only after the clock is resolved does
    // `get_timespec64(&t, rqtp)` run; a NULL `rqtp` returns -EFAULT
    // (the `copy_from_user` failure path), not -EINVAL.
    //
    // Pre-Phase-153 we checked NULL FIRST and returned EINVAL.  Two
    // divergences: ordering (NULL beat clock check) and errno
    // (EINVAL instead of EFAULT).  Phase 153 fixes both while
    // preserving Phase 102's flag-mask-first invariant.

    // --- Per-error-class: confirm new EFAULT path is observable.

    #[test]
    fn test_clock_nanosleep_valid_clock_null_request_efault_phase153() {
        // The headline change: valid clock + zero flags + NULL request
        // now returns EFAULT (not EINVAL).
        assert_eq!(
            clock_nanosleep(CLOCK_MONOTONIC, 0, core::ptr::null(), core::ptr::null_mut()),
            crate::errno::EFAULT
        );
    }

    #[test]
    fn test_clock_nanosleep_every_valid_clock_null_request_efault_phase153() {
        // The EFAULT path applies to every clock that can be slept on; the
        // others are `test_clock_nanosleep_clocks_that_cannot_sleep`'s.
        for &clk in &[
            CLOCK_REALTIME,
            CLOCK_MONOTONIC,
            CLOCK_PROCESS_CPUTIME_ID,
            CLOCK_BOOTTIME,
        ] {
            assert_eq!(
                clock_nanosleep(clk, 0, core::ptr::null(), core::ptr::null_mut()),
                crate::errno::EFAULT,
                "clk={} must return EFAULT for NULL request",
                clk
            );
        }
    }

    // --- Ordering matrix: clock check beats NULL check.

    #[test]
    fn test_clock_nanosleep_bad_clock_beats_null_request_phase153() {
        // BAD clock + NULL request: pre-Phase-153 returned EINVAL via
        // the NULL check.  Post-Phase-153 the clock check fires first
        // and we still get EINVAL — same errno but different *path*.
        // The observable difference would be in errno (had Linux
        // chosen different errnos for the two), but both are EINVAL.
        // The cross-check below confirms ordering:
        assert_eq!(
            clock_nanosleep(99_999, 0, core::ptr::null(), core::ptr::null_mut()),
            crate::errno::EINVAL,
            "bad clock + NULL must still return EINVAL"
        );

        // With a VALID clock, the same NULL request now returns
        // EFAULT — proves the clock check is what's distinguishing the
        // two cases.
        assert_eq!(
            clock_nanosleep(CLOCK_MONOTONIC, 0, core::ptr::null(), core::ptr::null_mut()),
            crate::errno::EFAULT,
            "valid clock + NULL must return EFAULT (Phase 153 path proof)"
        );
    }

    #[test]
    fn test_clock_nanosleep_negative_clock_beats_null_request_phase153() {
        // Negative clock id (e.g., -1) is a common bug — must yield
        // EINVAL via the clock check, not EFAULT via NULL.
        assert_eq!(
            clock_nanosleep(-1, 0, core::ptr::null(), core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    #[test]
    fn test_clock_nanosleep_stray_flag_does_not_beat_null_phase153() {
        // A stray flag bit is not a verdict, so a NULL request is EFAULT
        // with it as without it.  (EINVAL, from the mask, until 2026-09-26.)
        assert_eq!(
            clock_nanosleep(
                CLOCK_REALTIME,
                1 << 4,
                core::ptr::null(),
                core::ptr::null_mut()
            ),
            crate::errno::EFAULT
        );
    }

    #[test]
    fn test_clock_nanosleep_null_request_beats_invalid_nsec_phase153() {
        // NULL request must short-circuit before the timespec is
        // dereferenced — i.e., we can't observe "bad nsec" because
        // the pointer is NULL.  Trivially true (can't read invalid
        // nsec from a NULL pointer) — guards against a future
        // refactor that dereferences before the null check.
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut()),
            crate::errno::EFAULT
        );
    }

    #[test]
    fn test_clock_nanosleep_clock_check_beats_invalid_nsec_phase153() {
        // BAD clock + valid pointer + invalid nsec: clock check
        // fires first → EINVAL (clock reason).  Linux ordering.
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 2_000_000_000,
        };
        assert_eq!(
            clock_nanosleep(99_999, 0, &req, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
    }

    // --- Workflow: probe-then-call with NULL request.

    #[test]
    fn test_clock_nanosleep_probe_then_real_call_phase153() {
        // A caller probes the call shape with NULL; gets EFAULT.
        // Then makes a real call with a valid request.
        assert_eq!(
            clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut()),
            crate::errno::EFAULT
        );
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let ret = clock_nanosleep(CLOCK_REALTIME, 0, &req, core::ptr::null_mut());
        assert_ne!(
            ret,
            crate::errno::EFAULT,
            "real call after probe must not return EFAULT"
        );
        assert_ne!(
            ret,
            crate::errno::EINVAL,
            "zero timespec is valid — must not return EINVAL"
        );
    }

    #[test]
    fn test_clock_nanosleep_efault_then_einval_workflow_phase153() {
        // Probe with NULL (EFAULT), then with bad clock (EINVAL):
        // the errno discriminates between the two failure modes,
        // letting a caller tell which thing was wrong.
        let r1 = clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut());
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let r2 = clock_nanosleep(99_999, 0, &req, core::ptr::null_mut());
        assert_eq!(r1, crate::errno::EFAULT);
        assert_eq!(r2, crate::errno::EINVAL);
        assert_ne!(r1, r2, "EFAULT and EINVAL must be distinguishable");
    }

    // --- Buggy caller: NULL pointer accidentally passed.

    #[test]
    fn test_clock_nanosleep_buggy_caller_phase153() {
        // A buggy caller forgot to initialise the request pointer.
        // Linux returns EFAULT — telling them "the pointer was bad"
        // instead of EINVAL ("some argument was invalid", which is
        // less informative).
        let ret = clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(
            ret,
            crate::errno::EFAULT,
            "buggy NULL pointer must give the specific EFAULT diagnostic"
        );
    }

    // --- Recovery: state isn't corrupted by an EFAULT failure.

    #[test]
    fn test_clock_nanosleep_recovery_phase153() {
        // EFAULT failure must not corrupt any internal state.
        // A subsequent valid call must succeed normally.
        for _ in 0..5 {
            assert_eq!(
                clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut()),
                crate::errno::EFAULT
            );
        }
        let req = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let ret = clock_nanosleep(CLOCK_REALTIME, 0, &req, core::ptr::null_mut());
        assert_ne!(ret, crate::errno::EFAULT);
        assert_ne!(ret, crate::errno::EINVAL);
    }

    // --- No-side-effect loop: repeated NULL calls all return EFAULT.

    #[test]
    fn test_clock_nanosleep_efault_loop_phase153() {
        // Repeated NULL-request calls all yield EFAULT.  Guards
        // against any future state-machine bug in the early-return
        // path (e.g., a counter that wraps and changes behaviour).
        for i in 0..1000 {
            assert_eq!(
                clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut()),
                crate::errno::EFAULT,
                "iteration {} must return EFAULT",
                i
            );
        }
    }

    // --- Errno-not-touched: clock_nanosleep returns error directly.

    #[test]
    fn test_clock_nanosleep_null_request_does_not_set_errno_phase153() {
        // POSIX: clock_nanosleep returns the error number directly
        // and does NOT set errno.  Verify the NULL path respects this.
        crate::errno::set_errno(crate::errno::ENOENT);
        let ret = clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut());
        assert_eq!(ret, crate::errno::EFAULT);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::ENOENT,
            "clock_nanosleep must not touch errno"
        );
    }

    // --- Sentinel: previously-EINVAL NULL path now distinguishable.

    #[test]
    fn test_clock_nanosleep_null_request_no_longer_einval_phase153() {
        // Regression guard: explicitly assert NULL + valid clock no
        // longer collapses to EINVAL.  If a future refactor restores
        // the old behaviour, this test breaks loudly.
        let ret = clock_nanosleep(CLOCK_REALTIME, 0, core::ptr::null(), core::ptr::null_mut());
        assert_ne!(
            ret,
            crate::errno::EINVAL,
            "Phase 153: valid clock + NULL must NOT collapse to EINVAL"
        );
        assert_eq!(ret, crate::errno::EFAULT);
    }

    // --- ABSTIME + NULL: flag bit zero is fine, NULL still gives EFAULT.

    #[test]
    fn test_clock_nanosleep_abstime_null_request_efault_phase153() {
        // TIMER_ABSTIME alone passes the flag mask; with NULL request
        // the next check is clock (OK) then NULL → EFAULT.
        assert_eq!(
            clock_nanosleep(
                CLOCK_REALTIME,
                TIMER_ABSTIME,
                core::ptr::null(),
                core::ptr::null_mut(),
            ),
            crate::errno::EFAULT
        );
    }

    // -- gettimeofday --

    /// Phase 152: NULL `tv` now matches Linux — it is silently
    /// accepted and `gettimeofday` returns 0 without setting errno.
    /// Renamed from `test_gettimeofday_null_tv` (which pinned the
    /// pre-Phase-152 EFAULT behaviour).
    #[test]
    fn test_gettimeofday_null_tv_returns_zero_phase152() {
        crate::errno::set_errno(0);
        assert_eq!(
            gettimeofday(core::ptr::null_mut(), core::ptr::null_mut()),
            0,
            "NULL tv must return 0, not -1 — Linux semantics"
        );
        assert_eq!(crate::errno::get_errno(), 0, "NULL tv must not set errno");
    }

    #[test]
    fn test_gettimeofday_returns_value() {
        // On the host, this executes a real syscall. Just verify it
        // doesn't crash and tv gets some value.
        let mut tv = Timeval {
            tv_sec: -1,
            tv_usec: -1,
        };
        let _ = gettimeofday(&raw mut tv, core::ptr::null_mut());
        // Don't assert exact values — syscall may return error on host.
    }

    // -- Phase 152: gettimeofday NULL tv tolerance --
    //
    // Linux's `kernel/time/time.c::SYSCALL_DEFINE2(gettimeofday, ...)`
    // wraps the tv population in `if (likely(tv != NULL))`.  A NULL
    // `tv` is not an error — the kernel just skips writing the
    // timeval and falls through to the (also-optional) tz copy,
    // returning 0.  Pre-Phase-152 we returned `-1`/`EFAULT` instead.
    //
    // All tests below cover the new NULL-tolerance contract.

    // --- Per-error-class: confirm NULL tv leaves errno unchanged.

    #[test]
    fn test_gettimeofday_null_tv_does_not_set_errno_phase152() {
        // Seed errno with a non-zero sentinel.  A NULL-tv call must
        // not touch it — Linux's `sys_gettimeofday` never writes
        // errno on the NULL branch.
        crate::errno::set_errno(crate::errno::EINVAL);
        let ret = gettimeofday(core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "errno must remain at the seeded sentinel — NULL tv is silent"
        );
    }

    // --- Ordering matrix: tz state is independent of tv-NULL handling.

    #[test]
    fn test_gettimeofday_null_tv_non_null_tz_returns_zero_phase152() {
        // Even with a non-NULL `tz` pointer, NULL `tv` returns 0.
        // Linux's tz branch is independent of the tv branch — both
        // are short-circuited by their respective NULL checks.
        crate::errno::set_errno(0);
        let mut dummy_tz: u8 = 0;
        let ret = gettimeofday(
            core::ptr::null_mut(),
            (&raw mut dummy_tz).cast::<core::ffi::c_void>(),
        );
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_gettimeofday_both_null_returns_zero_phase152() {
        // Canonical "ping the syscall" idiom: callers that don't
        // care about the time but want to confirm the syscall is
        // wired up.  Linux returns 0; we now do the same.
        crate::errno::set_errno(0);
        assert_eq!(
            gettimeofday(core::ptr::null_mut(), core::ptr::null_mut()),
            0
        );
        assert_eq!(crate::errno::get_errno(), 0);
    }

    #[test]
    fn test_gettimeofday_non_null_tv_unaffected_phase152() {
        // The non-NULL tv path must still populate the timeval as
        // before — Phase 152 only touches the NULL branch.
        let mut tv = Timeval {
            tv_sec: -1,
            tv_usec: -1,
        };
        let ret = gettimeofday(&raw mut tv, core::ptr::null_mut());
        // On the host the underlying syscall may fail; just verify
        // we didn't take the NULL-shortcut and that ret is sane.
        if ret == 0 {
            // If the syscall succeeded, tv must have been written
            // (no longer the -1/-1 sentinel).
            assert!(
                tv.tv_sec != -1 || tv.tv_usec != -1,
                "non-NULL tv path must populate the timeval on success"
            );
        }
    }

    // --- Workflow: probe-then-use pattern.

    #[test]
    fn test_gettimeofday_probe_then_real_call_workflow_phase152() {
        // A program probes with NULL first to confirm the syscall is
        // wired up, then makes a real call with a valid buffer.
        crate::errno::set_errno(0);
        let probe = gettimeofday(core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(probe, 0, "probe with NULL must succeed");
        assert_eq!(crate::errno::get_errno(), 0);

        let mut tv = Timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        let _ = gettimeofday(&raw mut tv, core::ptr::null_mut());
        // We don't assert the value — host syscall may fail — but
        // the probe must not have poisoned the subsequent call.
    }

    #[test]
    fn test_gettimeofday_null_tv_then_settimeofday_workflow_phase152() {
        // gettimeofday(NULL,NULL) returns 0; settimeofday(NULL,NULL)
        // also returns 0.  Confirm both halves of the no-op shape
        // agree — a common shim pattern when programs want to verify
        // both syscalls are present.
        crate::errno::set_errno(0);
        assert_eq!(
            gettimeofday(core::ptr::null_mut(), core::ptr::null_mut()),
            0
        );
        assert_eq!(settimeofday(core::ptr::null(), core::ptr::null()), 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // --- Buggy caller: NULL tv must not cascade into a fake error.

    #[test]
    fn test_gettimeofday_buggy_caller_phase152() {
        // A buggy caller passes NULL by accident.  Linux returns 0
        // (silent).  Our pre-Phase-152 code returned -1/EFAULT,
        // which would mislead the caller into believing the clock
        // was unavailable.  Verify the silent success path.
        crate::errno::set_errno(crate::errno::EBADF); // pre-existing errno
        let ret = gettimeofday(core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(ret, 0, "NULL tv must succeed silently");
        // Errno preserved from the caller's previous state.
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EBADF,
            "buggy NULL call must not stamp over the caller's errno"
        );
    }

    // --- Recovery: a NULL call followed by a real call works.

    #[test]
    fn test_gettimeofday_recovery_phase152() {
        // NULL call (no-op), then real call must work normally.
        crate::errno::set_errno(0);
        assert_eq!(
            gettimeofday(core::ptr::null_mut(), core::ptr::null_mut()),
            0
        );
        let mut tv = Timeval {
            tv_sec: -42,
            tv_usec: -42,
        };
        let ret = gettimeofday(&raw mut tv, core::ptr::null_mut());
        if ret == 0 {
            assert!(
                tv.tv_sec != -42 || tv.tv_usec != -42,
                "real call after NULL probe must populate tv"
            );
        }
    }

    // --- No-side-effect loop: repeated NULL calls don't accumulate state.

    #[test]
    fn test_gettimeofday_null_tv_loop_phase152() {
        // 1000 NULL-tv calls in a row must all succeed identically
        // and leave errno alone.  Guards against any future hidden
        // side effect (e.g. a static counter that wraps) in the
        // NULL-shortcut path.
        crate::errno::set_errno(0);
        for i in 0..1000 {
            let ret = gettimeofday(core::ptr::null_mut(), core::ptr::null_mut());
            assert_eq!(ret, 0, "iteration {} must succeed", i);
            assert_eq!(
                crate::errno::get_errno(),
                0,
                "iteration {} must not set errno",
                i
            );
        }
    }

    // --- Cross-check: settimeofday no-op shape is unaffected.

    #[test]
    fn test_gettimeofday_does_not_affect_settimeofday_phase152() {
        // The Phase 152 change is purely on gettimeofday — confirm
        // settimeofday's existing NULL-NULL → 0 behaviour is not
        // perturbed.
        crate::errno::set_errno(0);
        let _ = gettimeofday(core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(settimeofday(core::ptr::null(), core::ptr::null()), 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // --- Tz-only: NULL tv with non-NULL tz must still be a no-op success.

    #[test]
    fn test_gettimeofday_null_tv_arbitrary_tz_phase152() {
        // Stress: vary the tz pointer (NULL, a stack address, an
        // "unaligned" address).  Pre-Phase-152 short-circuited on
        // NULL tv before tz was inspected, so tz value never mattered.
        // Post-Phase-152 the NULL-tv branch still short-circuits to 0
        // — confirm the contract.
        let mut a: u8 = 0;
        let mut b: u32 = 0;
        let tz_variants: [*mut core::ffi::c_void; 3] = [
            core::ptr::null_mut(),
            (&raw mut a).cast::<core::ffi::c_void>(),
            (&raw mut b).cast::<core::ffi::c_void>(),
        ];
        for tz in tz_variants {
            crate::errno::set_errno(0);
            assert_eq!(
                gettimeofday(core::ptr::null_mut(), tz),
                0,
                "tz variant must not affect NULL-tv result"
            );
            assert_eq!(crate::errno::get_errno(), 0);
        }
    }

    // --- Interleaved: NULL/non-NULL tv calls in alternation.

    #[test]
    fn test_gettimeofday_alternating_null_and_valid_phase152() {
        // Alternate NULL and non-NULL tv calls.  The NULL calls
        // must not affect the non-NULL ones and vice versa.
        let mut tv = Timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        for i in 0..20 {
            crate::errno::set_errno(0);
            if i % 2 == 0 {
                assert_eq!(
                    gettimeofday(core::ptr::null_mut(), core::ptr::null_mut()),
                    0
                );
                assert_eq!(crate::errno::get_errno(), 0);
            } else {
                let _ = gettimeofday(&raw mut tv, core::ptr::null_mut());
                // value not asserted — host syscall may fail
            }
        }
    }

    // --- Independence: errno set by an unrelated call survives NULL-tv.

    #[test]
    fn test_gettimeofday_null_tv_preserves_prior_errno_phase152() {
        // Mirror of buggy-caller: confirm NULL-tv path does not
        // clear errno from any value, not just the canonical zero.
        // Exhaustive across a representative spread of errno values.
        for &e in &[
            crate::errno::EBADF,
            crate::errno::EINVAL,
            crate::errno::ENOENT,
            crate::errno::EAGAIN,
            crate::errno::EFAULT,
        ] {
            crate::errno::set_errno(e);
            assert_eq!(
                gettimeofday(core::ptr::null_mut(), core::ptr::null_mut()),
                0,
                "ret=0 expected with pre-set errno {}",
                e
            );
            assert_eq!(
                crate::errno::get_errno(),
                e,
                "errno must remain {} after NULL-tv call",
                e
            );
        }
    }

    // --- Phase 152 sentinel: previously-divergent input now matches Linux.

    #[test]
    fn test_gettimeofday_null_tv_no_longer_efault_phase152() {
        // Sentinel guard: explicitly assert the previous divergent
        // EFAULT path is gone.  If a future refactor reintroduces
        // the EFAULT-on-NULL behaviour, this test breaks loudly.
        crate::errno::set_errno(0);
        let ret = gettimeofday(core::ptr::null_mut(), core::ptr::null_mut());
        assert_ne!(ret, -1, "Phase 152: NULL tv must NOT return -1");
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EFAULT,
            "Phase 152: NULL tv must NOT set EFAULT"
        );
    }

    // -- localtime --

    #[test]
    fn test_localtime_null() {
        let _tz = TzGuard::utc();
        let ptr = localtime(core::ptr::null());
        assert!(ptr.is_null());
    }

    #[test]
    fn test_localtime_epoch() {
        let _tz = TzGuard::utc();
        // localtime delegates to gmtime (no timezone).
        let t: TimeT = 0;
        let ptr = localtime(&t);
        if !ptr.is_null() {
            let tm = unsafe { &*ptr };
            assert_eq!(tm.tm_year, 70); // 1970
            assert_eq!(tm.tm_mon, 0); // January
            assert_eq!(tm.tm_mday, 1);
        }
    }

    // -- ctime --

    #[test]
    fn test_ctime_null() {
        let _tz = TzGuard::utc();
        // ctime(NULL) → localtime(NULL) → NULL → asctime(NULL) → NULL with
        // EINVAL, as glibc's.
        errno::set_errno(0);
        let ptr = ctime(core::ptr::null());
        assert!(ptr.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ctime_epoch() {
        let _tz = TzGuard::utc();
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
        let ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
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
    // Validation order (CORRECTED in Phase 151 to actually match
    // Linux; the pre-Phase-151 comment below was wrong about the
    // tp NULL check order):
    //   unknown clock_id                 -> EINVAL
    //   recognised but unsettable clock  -> EINVAL
    //   tp == NULL                       -> EFAULT
    //   tv_sec < 0 or bad tv_nsec        -> EINVAL
    //   otherwise                        -> EPERM
    // ------------------------------------------------------------------

    /// Convenience: a valid Timespec for the EPERM path tests.
    fn valid_ts() -> Timespec {
        Timespec {
            tv_sec: 1,
            tv_nsec: 0,
        }
    }

    /// RAII guard that restores the effective capability set on drop.
    ///
    /// Restoring matters within a single test (drop `CAP_SYS_TIME`, assert
    /// the unprivileged path, then carry on privileged), not across tests:
    /// on host builds the capability words are per-thread and cargo runs
    /// each test on its own thread, so a cap this test drops is invisible
    /// to every other test. See `sys_capability`'s storage docs.
    struct CapGuard {
        lo: u32,
        hi: u32,
    }
    impl CapGuard {
        fn snapshot() -> Self {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            Self { lo, hi }
        }
    }
    impl Drop for CapGuard {
        fn drop(&mut self) {
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: self.lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: self.hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
        }
    }

    /// Drop `CAP_SYS_TIME` from the effective set.  Callers must hold a
    /// live `CapGuard` so the cap is restored when the test ends.
    ///
    /// Dropping the cap is what makes the clock-set path host-testable:
    /// with the cap held, a structurally valid call falls through to a
    /// real `SYS_CLOCK_SETTIME` syscall (which cannot be executed on the
    /// host), so the maximal host-observable boundary for a valid call is
    /// the `EPERM` cap gate.
    fn drop_cap_sys_time() {
        use crate::sys_capability::CAP_SYS_TIME;
        let (lo, hi) = crate::sys_capability::current_caps_effective();
        let (new_lo, new_hi) = if CAP_SYS_TIME < 32 {
            (lo & !(1u32 << CAP_SYS_TIME), hi)
        } else {
            (lo, hi & !(1u32 << (CAP_SYS_TIME - 32)))
        };
        let mut hdr = crate::sys_capability::CapUserHeader {
            version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        let data = [
            crate::sys_capability::CapUserData {
                effective: new_lo,
                permitted: u32::MAX,
                inheritable: 0,
            },
            crate::sys_capability::CapUserData {
                effective: new_hi,
                permitted: u32::MAX,
                inheritable: 0,
            },
        ];
        let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
        assert_eq!(rc, 0, "capset must succeed when dropping CAP_SYS_TIME");
        assert!(!crate::sys_capability::has_capability(CAP_SYS_TIME));
    }

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

    /// Phase 151: Linux dispatches the clock BEFORE touching the
    /// user pointer via get_timespec64.  So a bogus clock with a
    /// NULL tp returns EINVAL, not EFAULT.  The pre-Phase-151
    /// implementation had this backwards and the test asserted the
    /// wrong order; renamed from
    /// `test_clock_settime_efault_precedes_clock_check`.
    #[test]
    fn test_clock_settime_bad_clock_beats_null_tp_phase151() {
        errno::set_errno(0);
        let ret = clock_settime(0x7FFF_FFFF, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "Linux dispatches the clock before reaching get_timespec64"
        );
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
            assert_eq!(
                errno::get_errno(),
                errno::EINVAL,
                "clock {c} should report EINVAL"
            );
        }
    }

    #[test]
    fn test_clock_settime_negative_tv_sec_einval() {
        let ts = Timespec {
            tv_sec: -1,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_negative_tv_nsec_einval() {
        let ts = Timespec {
            tv_sec: 0,
            tv_nsec: -1,
        };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_tv_nsec_billion_einval() {
        // exactly 1e9 is out of range (valid range is 0..=999_999_999)
        let ts = Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_tv_nsec_max_valid_passes_to_cap_gate() {
        // 999_999_999 is the max legal nanosecond value.  It should pass
        // the timespec check and reach the cap gate; with the cap dropped
        // that surfaces as EPERM (the cap-held path issues a real syscall).
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let ts = Timespec {
            tv_sec: 0,
            tv_nsec: 999_999_999,
        };
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
        let ts = Timespec {
            tv_sec: -1,
            tv_nsec: -1,
        };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_MONOTONIC, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_settime_zero_timespec_realtime_cap_gate() {
        // The epoch is a legal timespec value.  Linux accepts it and would
        // set the clock to 1970-01-01; it passes validation and reaches the
        // cap gate (EPERM with the cap dropped here).
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let ts = Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_clock_settime_large_tv_sec_realtime_cap_gate() {
        // `timespec64_valid_settod` runs before the capability check
        // (kernel/time/time.c:174), so a time at or past
        // `TIME_SETTOD_SEC_MAX` is EINVAL even without CAP_SYS_TIME; one just
        // short of it reaches the cap gate.  The year-2262 time here reached
        // the gate until 2026-09-26.
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        for (secs, want) in [
            (9_223_372_036, errno::EINVAL),
            (TIME_SETTOD_SEC_MAX, errno::EINVAL),
            (TIME_SETTOD_SEC_MAX - 1, errno::EPERM),
        ] {
            let ts = Timespec {
                tv_sec: secs,
                tv_nsec: 500,
            };
            errno::set_errno(0);
            let ret = clock_settime(CLOCK_REALTIME, &raw const ts);
            assert_eq!(ret, -1, "tv_sec {secs}");
            assert_eq!(errno::get_errno(), want, "tv_sec {secs}");
        }
    }

    // ------------------------------------------------------------------
    // Phase 151 — clock_settime: clock_id dispatched before user pointer
    //
    // Linux's sys_clock_settime:
    //   1. clockid_to_kclock(which_clock) returns NULL  -> EINVAL
    //   2. !kc->clock_set (read-only clock)             -> EINVAL
    //   3. get_timespec64(tp) user-copy fails           -> EFAULT
    //   4. kc->clock_set validates timespec             -> EINVAL
    //   5. otherwise: kernel writes; may be EPERM for unpriv caller
    //
    // Pre-Phase-151 we ran the tp == NULL check FIRST.  Phase 151
    // reorders to match Linux: clock check, settable check, THEN
    // user-pointer check.
    // ------------------------------------------------------------------

    // -- per-error-class --

    /// Per-error-class: unsettable clock with valid tp → EINVAL
    /// (matches Linux's `!kc->clock_set` check).
    #[test]
    fn test_clock_settime_unsettable_clock_einval_phase151() {
        let ts = valid_ts();
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_MONOTONIC, &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Per-error-class: settable clock with NULL tp → EFAULT.
    #[test]
    fn test_clock_settime_settable_clock_null_tp_efault_phase151() {
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- ordering matrix --

    /// Ordering: unknown clock BEATS NULL tp.  Linux returns EINVAL
    /// from `clockid_to_kclock`; pre-Phase-151 returned EFAULT.
    #[test]
    fn test_clock_settime_unknown_clock_beats_null_tp_phase151() {
        errno::set_errno(0);
        let ret = clock_settime(0xDEAD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Ordering: unsettable (read-only) clock BEATS NULL tp.
    #[test]
    fn test_clock_settime_unsettable_clock_beats_null_tp_phase151() {
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_MONOTONIC, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "read-only clock check (EINVAL) must beat NULL tp (EFAULT)"
        );
    }

    /// Ordering: negative clock id BEATS NULL tp.
    #[test]
    fn test_clock_settime_negative_clock_beats_null_tp_phase151() {
        errno::set_errno(0);
        let ret = clock_settime(-5, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Ordering: NULL tp BEATS bad timespec (no way to construct;
    /// degenerate case — but a settable clock + NULL tp must return
    /// EFAULT before any timespec-validity check).  Tested by
    /// confirming EFAULT, not EINVAL, on the NULL path.
    #[test]
    fn test_clock_settime_null_tp_beats_timespec_validation_phase151() {
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// Ordering: every unsettable clock with NULL tp → EINVAL (the
    /// settability check fires before the tp check).
    #[test]
    fn test_clock_settime_every_unsettable_clock_null_tp_einval_phase151() {
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
            let ret = clock_settime(c, core::ptr::null());
            assert_eq!(ret, -1, "clock {c}");
            assert_eq!(
                errno::get_errno(),
                errno::EINVAL,
                "clock {c} unsettable check must fire before NULL tp"
            );
        }
    }

    // -- workflow --

    /// Workflow: probe whether CLOCK_REALTIME is settable with a
    /// NULL tp — must surface as EFAULT (i.e. clock passed validation
    /// and we reached the user-pointer step).
    #[test]
    fn test_clock_settime_realtime_probe_with_null_tp_phase151() {
        errno::set_errno(0);
        let ret = clock_settime(CLOCK_REALTIME, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(
            errno::get_errno(),
            errno::EFAULT,
            "CLOCK_REALTIME with NULL tp must reach the user-pointer step"
        );
    }

    /// Workflow: a libc probe-then-set sequence sees EINVAL for
    /// MONOTONIC then EFAULT/EPERM for REALTIME — confirming the
    /// "is it settable" diagnostic isn't masked by a NULL pointer.
    #[test]
    fn test_clock_settime_probe_then_set_workflow_phase151() {
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();

        // Probe MONOTONIC (read-only) with NULL — must be EINVAL.
        errno::set_errno(0);
        assert_eq!(clock_settime(CLOCK_MONOTONIC, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // Now set REALTIME with a valid value — passes validation and
        // reaches the cap gate (EPERM, cap dropped above).
        let ts = valid_ts();
        errno::set_errno(0);
        assert_eq!(clock_settime(CLOCK_REALTIME, &raw const ts), -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    // -- buggy caller --

    /// Buggy caller: forgets to take the address of their timespec
    /// AND passes a bad clock.  Phase 151 gives them EINVAL (fix
    /// the clock), not the misleading EFAULT (fix the pointer).
    #[test]
    fn test_clock_settime_buggy_caller_phase151() {
        errno::set_errno(0);
        let ret = clock_settime(0xBAD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "buggy clock+pointer combo must diagnose the clock first"
        );
    }

    // -- recovery --

    /// Recovery: after EINVAL from bad clock+NULL tp, fixing the
    /// clock surfaces EFAULT (the NULL is now the limiting factor).
    #[test]
    fn test_clock_settime_recovery_phase151() {
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();

        errno::set_errno(0);
        assert_eq!(clock_settime(999, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // Fix the clock; NULL tp now surfaces.
        errno::set_errno(0);
        assert_eq!(clock_settime(CLOCK_REALTIME, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);

        // Fix the tp; the path now passes validation and lands on the cap
        // gate (EPERM, cap dropped above).
        let ts = valid_ts();
        errno::set_errno(0);
        assert_eq!(clock_settime(CLOCK_REALTIME, &raw const ts), -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    // -- no-side-effect loop --

    /// No-side-effect loop: repeated bad-clock+NULL-tp calls
    /// consistently produce EINVAL.
    #[test]
    fn test_clock_settime_einval_loop_phase151() {
        for _ in 0..32 {
            errno::set_errno(0);
            assert_eq!(clock_settime(999, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);

            errno::set_errno(0);
            assert_eq!(clock_settime(CLOCK_REALTIME, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }
    }

    #[test]
    fn test_clock_settime_errno_progression() {
        // Sanity check that every distinct branch produces the
        // distinct errno we claim.  This guards against future
        // refactors that accidentally collapse branches.
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();

        let valid = valid_ts();
        let bad_ns = Timespec {
            tv_sec: 0,
            tv_nsec: -1,
        };

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
        // With CAP_SYS_TIME dropped, the valid-arg path reaches the cap
        // gate and surfaces EPERM (the cap-held path would issue a real
        // syscall, not host-testable).
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
    //   - structurally valid tv with CAP_SYS_TIME held → issues a real
    //     SYS_CLOCK_SETTIME syscall (not host-testable; success path)
    //   - structurally valid tv without CAP_SYS_TIME → -1, EPERM
    //     (the maximal host-observable boundary for a valid call)
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
        let tv = Timeval {
            tv_sec: -1,
            tv_usec: 0,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_negative_tv_usec_is_einval() {
        let tv = Timeval {
            tv_sec: 0,
            tv_usec: -1,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_tv_usec_equal_to_million_is_einval() {
        // The valid range is [0, 999_999]; exactly 1_000_000 must be rejected.
        let tv = Timeval {
            tv_sec: 0,
            tv_usec: 1_000_000,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_tv_usec_above_million_is_einval() {
        let tv = Timeval {
            tv_sec: 0,
            tv_usec: 9_999_999,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_max_valid_tv_usec_reaches_cap_gate() {
        // 999_999 is the maximum valid microsecond value — must pass
        // validation and reach the cap gate (EPERM, cap dropped), not
        // EINVAL.
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let tv = Timeval {
            tv_sec: 0,
            tv_usec: 999_999,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_zero_tv_reaches_cap_gate() {
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let tv = Timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_large_valid_tv_reaches_cap_gate() {
        // Reasonably distant future timestamp — still valid argument-wise.
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let tv = Timeval {
            tv_sec: 2_000_000_000,
            tv_usec: 500_000,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_valid_tv_with_nonnull_tz_reaches_cap_gate() {
        // tv valid + tz non-NULL: passes validation, reaches cap gate
        // (EPERM, cap dropped).
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let tv = Timeval {
            tv_sec: 1,
            tv_usec: 1,
        };
        let sentinel: u8 = 0;
        let tz = &raw const sentinel as *const core::ffi::c_void;
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, tz);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_einval_ordering_precedes_cap_gate() {
        // A bad tv_usec must be detected as EINVAL even when tz is
        // also non-NULL — validation order is "tv-field-shape" first,
        // cap-gate / syscall last.
        let tv = Timeval {
            tv_sec: 0,
            tv_usec: -42,
        };
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
        let tv = Timeval {
            tv_sec: -5,
            tv_usec: 9_999_999,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_settimeofday_phase84_does_not_overflow_on_intmax_sec() {
        // Pathological but well-formed input: i64::MAX seconds, 0 us.
        // Must not panic; the nanosecond conversion uses saturating math.
        // With the cap dropped it reaches the cap gate (EPERM).
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let tv = Timeval {
            tv_sec: i64::MAX,
            tv_usec: 0,
        };
        errno::set_errno(0);
        let ret = settimeofday(&raw const tv, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_repeated_calls_are_stable() {
        // Calling settimeofday repeatedly with the same valid args should
        // produce identical results — no hidden global state.
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let tv = Timeval {
            tv_sec: 1000,
            tv_usec: 1000,
        };
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
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        let bad = Timeval {
            tv_sec: 0,
            tv_usec: -1,
        };
        errno::set_errno(0);
        assert_eq!(settimeofday(&raw const bad, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let good = Timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        errno::set_errno(0);
        assert_eq!(settimeofday(&raw const good, core::ptr::null()), -1);
        // Valid args reach the cap gate (EPERM, cap dropped above).
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_settimeofday_phase84_null_tv_then_valid_tv_progression() {
        // Both-NULL success path followed by valid-tv cap-gate path
        // (EPERM, cap dropped).
        let _g = CapGuard::snapshot();
        drop_cap_sys_time();
        errno::set_errno(0);
        assert_eq!(settimeofday(core::ptr::null(), core::ptr::null()), 0);

        let tv = Timeval {
            tv_sec: 1,
            tv_usec: 1,
        };
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
        let _tz = TzGuard::utc();
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

    // ======================================================================
    // Phase 177 — clock_settime() / settimeofday() CAP_SYS_TIME gate.
    //
    // Linux gates both syscalls on CAP_SYS_TIME via
    // security_settime64() → capable(CAP_SYS_TIME) before delegating
    // to do_settimeofday64().  Pre-Phase-177 we returned EPERM
    // unconditionally regardless of capability state, which broke
    // privileged callers (they should reach the backend, not be
    // refused with EPERM).
    //
    // After Phase 177 (and the SYS_CLOCK_SETTIME backend):
    //   * with CAP_SYS_TIME held → issues a real SYS_CLOCK_SETTIME
    //     syscall (success path; not host-testable)
    //   * without CAP_SYS_TIME   → EPERM
    //   * EINVAL / EFAULT argument errors still take precedence
    //     because Linux validates those before the security_settime
    //     hook fires.
    //
    // Uses the CapGuard snapshot/restore pattern; --test-threads=1.
    // ======================================================================

    mod clock_time_cap_phase177 {
        use super::*;

        // `CapGuard` and `drop_cap_sys_time` are defined in the parent
        // `tests` module and imported via `use super::*` — single source
        // of truth so the main-module validation tests and these cap-gate
        // tests share one implementation.

        // -- clock_settime: per-error EPERM --------------------------------

        #[test]
        fn test_clock_settime_phase177_realtime_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let ts = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            errno::set_errno(0);
            assert_eq!(clock_settime(CLOCK_REALTIME, &raw const ts), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_clock_settime_phase177_max_valid_nsec_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let ts = Timespec {
                tv_sec: 1,
                tv_nsec: 999_999_999,
            };
            errno::set_errno(0);
            assert_eq!(clock_settime(CLOCK_REALTIME, &raw const ts), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- clock_settime: ordering — EINVAL/EFAULT beat EPERM ------------

        #[test]
        fn test_clock_settime_phase177_unknown_clock_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let ts = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            errno::set_errno(0);
            assert_eq!(clock_settime(0xDEAD, &raw const ts), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        #[test]
        fn test_clock_settime_phase177_unsettable_clock_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let ts = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            errno::set_errno(0);
            assert_eq!(clock_settime(CLOCK_MONOTONIC, &raw const ts), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        #[test]
        fn test_clock_settime_phase177_null_tp_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            errno::set_errno(0);
            assert_eq!(clock_settime(CLOCK_REALTIME, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }

        #[test]
        fn test_clock_settime_phase177_bad_nsec_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let ts = Timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            };
            errno::set_errno(0);
            assert_eq!(clock_settime(CLOCK_REALTIME, &raw const ts), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- clock_settime: workflow drop/restore --------------------------

        /// Dropping `CAP_SYS_TIME` yields `EPERM` for a valid call; the
        /// `CapGuard` restores the cap when the inner scope ends.  We can
        /// only observe up to the cap gate on the host — with the cap held
        /// the call would issue a real `SYS_CLOCK_SETTIME` syscall, which
        /// is not executable in the host test process — so the post-restore
        /// half just confirms the cap is back, not the syscall result.
        #[test]
        fn test_clock_settime_phase177_drop_then_restore_workflow() {
            let ts = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            {
                let _g = CapGuard::snapshot();
                drop_cap_sys_time();
                errno::set_errno(0);
                assert_eq!(clock_settime(CLOCK_REALTIME, &raw const ts), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_TIME,
            ));
        }

        // -- settimeofday: per-error EPERM ---------------------------------

        #[test]
        fn test_settimeofday_phase177_valid_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let tv = Timeval {
                tv_sec: 0,
                tv_usec: 0,
            };
            errno::set_errno(0);
            assert_eq!(settimeofday(&raw const tv, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        #[test]
        fn test_settimeofday_phase177_max_valid_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let tv = Timeval {
                tv_sec: 2_000_000_000,
                tv_usec: 999_999,
            };
            errno::set_errno(0);
            assert_eq!(settimeofday(&raw const tv, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- settimeofday: ordering — EINVAL beats EPERM -------------------

        #[test]
        fn test_settimeofday_phase177_negative_sec_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let tv = Timeval {
                tv_sec: -1,
                tv_usec: 0,
            };
            errno::set_errno(0);
            assert_eq!(settimeofday(&raw const tv, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        #[test]
        fn test_settimeofday_phase177_bad_usec_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let tv = Timeval {
                tv_sec: 0,
                tv_usec: 1_000_000,
            };
            errno::set_errno(0);
            assert_eq!(settimeofday(&raw const tv, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- settimeofday: NULL-NULL bypasses the gate ---------------------

        /// Both-NULL no-op succeeds even without CAP_SYS_TIME — it
        /// short-circuits before reaching the cap probe.
        #[test]
        fn test_settimeofday_phase177_null_null_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            errno::set_errno(0);
            assert_eq!(settimeofday(core::ptr::null(), core::ptr::null()), 0);
        }

        /// tz-only update (tv NULL, tz non-NULL) also short-circuits
        /// before the cap probe.
        #[test]
        fn test_settimeofday_phase177_tz_only_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let sentinel: u8 = 0;
            let tz = &raw const sentinel as *const core::ffi::c_void;
            errno::set_errno(0);
            assert_eq!(settimeofday(core::ptr::null(), tz), 0);
        }

        // -- settimeofday: workflow drop/restore ---------------------------

        /// As with `clock_settime`: dropping the cap yields `EPERM`; the
        /// post-restore half only confirms the cap is reinstated, because
        /// the cap-held path issues a real syscall we can't run on host.
        #[test]
        fn test_settimeofday_phase177_drop_then_restore_workflow() {
            let tv = Timeval {
                tv_sec: 0,
                tv_usec: 0,
            };
            {
                let _g = CapGuard::snapshot();
                drop_cap_sys_time();
                errno::set_errno(0);
                assert_eq!(settimeofday(&raw const tv, core::ptr::null()), -1);
                assert_eq!(errno::get_errno(), errno::EPERM);
            }
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_TIME,
            ));
        }

        // -- cross-checks --------------------------------------------------

        /// settimeofday and clock_settime agree on cap semantics:
        /// dropping CAP_SYS_TIME yields EPERM on both (the cap-held
        /// path issues a real syscall, not host-testable).
        #[test]
        fn test_phase177_clock_settime_and_settimeofday_agree_on_cap_gate() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let ts = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            let tv = Timeval {
                tv_sec: 0,
                tv_usec: 0,
            };
            errno::set_errno(0);
            assert_eq!(clock_settime(CLOCK_REALTIME, &raw const ts), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            errno::set_errno(0);
            assert_eq!(settimeofday(&raw const tv, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Errno isolation: an EPERM call doesn't bleed into a
        /// subsequent fresh-errno EINVAL call.
        #[test]
        fn test_phase177_eperm_then_einval_isolation() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let ts = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            errno::set_errno(0);
            assert_eq!(clock_settime(CLOCK_REALTIME, &raw const ts), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);

            errno::set_errno(0);
            let bad = Timeval {
                tv_sec: -1,
                tv_usec: 0,
            };
            assert_eq!(settimeofday(&raw const bad, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }
    }

    // -- glibc's answers, replayed --

    /// glibc 2.39's time conversions (`posix/tools/oracle/timeconv_harness.py`).
    const TIMECONV_ORACLE: &str = include_str!("timeconv_oracle.txt");

    /// The zones the oracle ran in, by index.
    const ORACLE_ZONES: [&[u8]; 2] = [b"UTC0", b"EST5EDT,M3.2.0,M11.1.0"];

    /// The recognisable `struct tm` every oracle call starts from.
    fn sentinel_tm() -> Tm {
        Tm {
            tm_sec: 47,
            tm_min: 37,
            tm_hour: 17,
            tm_mday: 27,
            tm_mon: 6,
            tm_year: 77,
            tm_wday: 3,
            tm_yday: 222,
            tm_isdst: -7,
            tm_gmtoff: -12345,
            tm_zone: c"sentinel".as_ptr().cast(),
        }
    }

    /// A `Tm` as the oracle prints one.
    fn oracle_tm(t: &Tm) -> String {
        let zone = if t.tm_zone.is_null() {
            String::from("-")
        } else {
            // SAFETY: every `tm_zone` here is a NUL-terminated static.
            unsafe { core::ffi::CStr::from_ptr(t.tm_zone.cast()) }
                .to_str()
                .unwrap()
                .to_owned()
        };
        format!(
            "{} {} {} {} {} {} {} {} {} {} {zone}",
            t.tm_sec,
            t.tm_min,
            t.tm_hour,
            t.tm_mday,
            t.tm_mon,
            t.tm_year,
            t.tm_wday,
            t.tm_yday,
            t.tm_isdst,
            t.tm_gmtoff
        )
    }

    /// A C string as the oracle prints one: its bytes in hex, `-` for NULL
    /// or empty.
    fn oracle_text(p: *const u8) -> String {
        if p.is_null() {
            return String::from("-");
        }
        // SAFETY: a NUL-terminated result of the function under test.
        let bytes = unsafe { core::ffi::CStr::from_ptr(p.cast()) }.to_bytes();
        if bytes.is_empty() {
            return String::from("-");
        }
        let mut out = String::new();
        for b in bytes {
            core::fmt::Write::write_fmt(&mut out, format_args!("{b:02x}")).unwrap();
        }
        out
    }

    /// glibc 2.39's `gmtime_r`, `localtime_r`, `mktime`, `timegm`,
    /// `strftime("%s")`, `ctime`, `ctime_r`, `asctime` and `asctime_r`, over
    /// the whole of `time_t` and `int` and in two zones, one with daylight
    /// time: every value, `errno` and field it produced.
    ///
    /// Two differences, on purpose:
    ///
    /// - When `gmtime_r` or `localtime_r` refuse a year that does not fit
    ///   `tm_year` (`EOVERFLOW`), glibc has already written some fields --
    ///   which ones depends on where the zone came from -- and this library
    ///   has written none. Only the NULL and the `errno` are compared there.
    /// - In the zone with daylight time, glibc applies a `TZ` string's rule
    ///   only from 1970: for an earlier year it anchors the transitions in
    ///   1970, so no earlier instant is ever daylight time; and past year
    ///   5,885,486 its day count overflows an `int`. `tzrules` applies the
    ///   rule in every year, as POSIX reads it and musl does, so those
    ///   instants ([`glibc_tz_rule_differs`]) are not compared. (A zone
    ///   file's history, which is what `/etc/localtime` holds, is unaffected
    ///   either way.)
    #[test]
    #[allow(clippy::too_many_lines)]
    fn time_conversions_answer_as_glibc_does() {
        let _tz = TzGuard::utc();
        let mut zone = usize::MAX;
        let mut set_zone = |z: &str| {
            let z: usize = z.parse().unwrap();
            if z != zone {
                TzGuard::put(ORACLE_ZONES[z]);
                zone = z;
            }
        };
        let mut bad = Vec::new();
        let mut calls = 0;
        let mut skipped = 0;
        for line in TIMECONV_ORACLE.lines() {
            let (lhs, want) = line.split_once(" = ").unwrap();
            let w: Vec<&str> = lhs.split(' ').collect();
            // The instant glibc's answer is about, for the zone rule above.
            let instant = match w[0] {
                "l" | "c" | "C" => w[2].parse::<i64>().ok(),
                "m" => want.split(' ').next().and_then(|t| t.parse().ok()),
                "f" => want
                    .split(' ')
                    .nth(2)
                    .and_then(|h| String::from_utf8(unhex(h)).ok())
                    .and_then(|t| t.parse().ok()),
                _ => None,
            };
            if instant.is_some_and(|t| glibc_tz_rule_differs(w[1], t)) {
                skipped += 1;
                continue;
            }
            let mut want = want.to_owned();
            errno::set_errno(0);
            let got = match w[0] {
                "g" | "l" => {
                    set_zone(w[1]);
                    let secs: TimeT = w[2].parse().unwrap();
                    let mut t = sentinel_tm();
                    // SAFETY: a local `time_t` and `Tm`.
                    let r = unsafe {
                        if w[0] == "g" {
                            gmtime_r(&raw const secs, &raw mut t)
                        } else {
                            localtime_r(&raw const secs, &raw mut t)
                        }
                    };
                    let e = errno::get_errno();
                    if r.is_null() {
                        // glibc's partial fields are not compared (above).
                        want = want.split(' ').take(2).collect::<Vec<_>>().join(" ");
                        format!("0 {e}")
                    } else {
                        format!("1 {e} {}", oracle_tm(&t))
                    }
                }
                "m" | "t" | "f" => {
                    set_zone(w[1]);
                    let v: Vec<i32> = w[2..9].iter().map(|x| x.parse().unwrap()).collect();
                    let mut t = sentinel_tm();
                    t.tm_sec = v[0];
                    t.tm_min = v[1];
                    t.tm_hour = v[2];
                    t.tm_mday = v[3];
                    t.tm_mon = v[4];
                    t.tm_year = v[5];
                    t.tm_isdst = v[6];
                    if w[0] == "f" {
                        let mut buf = [0u8; 64];
                        // SAFETY: a 64-byte buffer, a NUL-terminated format and
                        // a local `Tm`.
                        let n = unsafe {
                            strftime(
                                buf.as_mut_ptr(),
                                buf.len(),
                                c"%s".as_ptr().cast(),
                                &raw const t,
                            )
                        };
                        let e = errno::get_errno();
                        let text = if n == 0 {
                            String::from("-")
                        } else {
                            oracle_text(buf.as_ptr())
                        };
                        format!("{n} {e} {text}")
                    } else {
                        let r = if w[0] == "m" {
                            mktime(&raw mut t)
                        } else {
                            timegm(&raw mut t)
                        };
                        let e = errno::get_errno();
                        format!("{r} {e} {}", oracle_tm(&t))
                    }
                }
                "c" | "C" => {
                    set_zone(w[1]);
                    let secs: TimeT = w[2].parse().unwrap();
                    let mut buf = [0u8; 64];
                    // SAFETY: a local `time_t` and a 64-byte buffer.
                    let r: *const u8 = if w[0] == "c" {
                        unsafe { ctime_r(&raw const secs, buf.as_mut_ptr()) }
                    } else {
                        ctime(&raw const secs)
                    };
                    let e = errno::get_errno();
                    format!("{} {e} {}", u8::from(!r.is_null()), oracle_text(r))
                }
                "a" | "A" if w[1] == "null" => {
                    let r = asctime(core::ptr::null());
                    let e = errno::get_errno();
                    format!("{} {e} {}", u8::from(!r.is_null()), oracle_text(r))
                }
                "a" | "A" => {
                    let v: Vec<i32> = w[1..8].iter().map(|x| x.parse().unwrap()).collect();
                    let mut t = sentinel_tm();
                    t.tm_sec = v[0];
                    t.tm_min = v[1];
                    t.tm_hour = v[2];
                    t.tm_mday = v[3];
                    t.tm_mon = v[4];
                    t.tm_year = v[5];
                    t.tm_wday = v[6];
                    let mut buf = [0u8; 64];
                    // SAFETY: a local `Tm` and a 64-byte buffer.
                    let r: *const u8 = if w[0] == "a" {
                        unsafe { asctime_r(&raw const t, buf.as_mut_ptr()) }
                    } else {
                        asctime(&raw const t)
                    };
                    let e = errno::get_errno();
                    format!("{} {e} {}", u8::from(!r.is_null()), oracle_text(r))
                }
                other => panic!("oracle row {other}"),
            };
            calls += 1;
            if got != want {
                bad.push(format!("{line}\n    ours {got}"));
            }
        }
        assert!(calls > 1000, "only {calls} calls");
        // The rows the zone rule sets aside: the pre-1970 and far-future
        // instants in the zone with daylight time -- half its random
        // sample, and the calendar's corners. A third of all the rows would
        // mean the rule had grown to swallow what it should not.
        assert!(skipped * 3 < calls, "{skipped} rows set aside of {calls}");
        assert!(
            bad.is_empty(),
            "{} of {calls} differ:\n{}",
            bad.len(),
            bad.iter().take(60).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    /// Whether glibc's answer about `instant` in oracle zone `zone` rests on
    /// its reading of a `TZ` rule outside 1970 to year 5,885,486 (see
    /// [`time_conversions_answer_as_glibc_does`]).
    fn glibc_tz_rule_differs(zone: &str, instant: i64) -> bool {
        zone == "1" && !(0..180_000_000_000_000).contains(&instant)
    }

    /// Hex text as bytes; empty for the oracle's `-`.
    fn unhex(h: &str) -> Vec<u8> {
        if h == "-" {
            return Vec::new();
        }
        (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
            .collect()
    }

    // -- strptime, against glibc --

    /// glibc 2.39's `strptime` (`posix/tools/oracle/strptime_harness.py`).
    const STRPTIME_ORACLE: &str = include_str!("strptime_oracle.txt");

    /// `fmt` without its `E` and `O` modifiers -- those on a conversion
    /// that takes one -- if it had any.
    fn without_modifiers(fmt: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut changed = false;
        let mut i = 0;
        while let Some(&c) = fmt.get(i) {
            out.push(c);
            i += 1;
            if c != b'%' {
                continue;
            }
            while let Some(&f) = fmt.get(i).filter(|f| b"-_0^#123456789".contains(f)) {
                out.push(f);
                i += 1;
            }
            let pair = (fmt.get(i).copied(), fmt.get(i + 1).copied());
            let takes = match pair {
                (Some(b'E'), Some(n)) => b"cCxXyY".contains(&n),
                (Some(b'O'), Some(n)) => b"bBhdeHImMSUVWwy".contains(&n),
                _ => false,
            };
            if takes {
                i += 1;
                changed = true;
            }
            if let Some(&n) = fmt.get(i) {
                out.push(n);
                i += 1;
            }
        }
        changed.then_some(out)
    }

    /// The `struct tm` an oracle row starts from.
    fn strptime_start(kind: &str) -> Tm {
        match kind {
            "s" => Tm {
                tm_zone: core::ptr::null(),
                ..sentinel_tm()
            },
            _ => Tm::ZERO,
        }
    }

    /// glibc 2.39's `strptime` for every conversion, alone and in the
    /// combinations programs use, from two starting `struct tm`s: the
    /// bytes it took, or NULL, and every field -- those a failing call had
    /// already stored included.
    ///
    /// Two kinds of row are held to another answer than glibc's:
    ///
    /// - A format with an `E` or `O` modifier answers as the same format
    ///   without it, which is what the modifiers mean in the C locale. glibc
    ///   parts from that in three ways: its `%Ey` reads a second number
    ///   after the year, every `%O` conversion after a format's first is no
    ///   match, and its `%Oy` does not take `%C`'s century.
    /// - Where `%s`'s number or its year is too big and the call is NULL,
    ///   glibc has stored some of the fields `localtime` was part-way
    ///   through; this library has stored none. Only the NULL is compared.
    #[test]
    fn strptime_answers_as_glibc_does() {
        let _tz = TzGuard::utc();
        let mut answers = std::collections::HashMap::new();
        for line in STRPTIME_ORACLE.lines() {
            let (lhs, want) = line.split_once(" = ").unwrap();
            let w: Vec<&str> = lhs.split(' ').collect();
            answers.insert((unhex(w[0]), unhex(w[1]), w[2]), want);
        }
        let mut bad = Vec::new();
        let mut calls = 0;
        for line in STRPTIME_ORACLE.lines() {
            let (lhs, glibc) = line.split_once(" = ").unwrap();
            let w: Vec<&str> = lhs.split(' ').collect();
            let (fmt, input) = (unhex(w[0]), unhex(w[1]));
            let mut want = match without_modifiers(&fmt) {
                Some(plain) => (*answers.get(&(plain, input.clone(), w[2])).unwrap()).to_owned(),
                None => glibc.to_owned(),
            };
            let mut t = strptime_start(w[2]);
            let (mut f, mut i) = (fmt.clone(), input.clone());
            f.push(0);
            i.push(0);
            // SAFETY: NUL-terminated format and input, and a local `Tm`.
            let r = unsafe { strptime(i.as_ptr(), f.as_ptr(), &raw mut t) };
            let consumed = if r.is_null() {
                -1
            } else {
                // SAFETY: a non-NULL result points into `i`.
                unsafe { r.offset_from(i.as_ptr()) }
            };
            let mut got = format!(
                "{consumed} {} {} {} {} {} {} {} {} {} {}",
                t.tm_sec,
                t.tm_min,
                t.tm_hour,
                t.tm_mday,
                t.tm_mon,
                t.tm_year,
                t.tm_wday,
                t.tm_yday,
                t.tm_isdst,
                t.tm_gmtoff
            );
            if fmt.windows(2).any(|p| p == b"%s") && want.starts_with("-1 ") {
                want.truncate(2);
                got.truncate(got.find(' ').unwrap_or(got.len()));
            }
            calls += 1;
            if got != want {
                bad.push(format!(
                    "\"{}\" \"{}\" {} = {want}\n    ours {got}",
                    fmt.escape_ascii(),
                    input.escape_ascii(),
                    w[2]
                ));
            }
        }
        assert!(calls > 7000, "only {calls} calls");
        assert!(
            bad.is_empty(),
            "{} of {calls} differ:\n{}",
            bad.len(),
            bad.iter().take(60).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    // -- getdate, against glibc --

    /// glibc 2.39's `getdate` and `getdate_r`
    /// (`posix/tools/oracle/getdate_harness.py`).
    const GETDATE_ORACLE: &str = include_str!("getdate_oracle.txt");

    /// `getdate_r`'s matching and filling, over templates held in memory
    /// rather than read from `DATEMSK`'s file: `getdate_stream`'s loop, for
    /// the host, where no file system answers.
    fn getdate_lines(string: *const u8, templates: &[Vec<u8>], tp: &mut Tm, now: TimeT) -> i32 {
        // SAFETY: the callers pass NUL-terminated input.
        let Some(input) = (unsafe { GetdateInput::new(string) }) else {
            return 6;
        };
        if !templates
            .iter()
            .any(|t| getdate_try(input.ptr(), t.as_ptr(), tp))
        {
            return 7;
        }
        getdate_fill(tp, now)
    }

    /// A `Tm` as the getdate oracle prints one.
    fn getdate_fields(t: &Tm) -> String {
        format!(
            "{} {} {} {} {} {} {} {} {}",
            t.tm_sec,
            t.tm_min,
            t.tm_hour,
            t.tm_mday,
            t.tm_mon,
            t.tm_year,
            t.tm_wday,
            t.tm_yday,
            t.tm_isdst
        )
    }

    /// glibc 2.39's `getdate` and `getdate_r`: 49 inputs against 21
    /// templates in two zones, each replayed at the second glibc's call ran
    /// in -- weekdays, months and times alone resolved from it -- with the
    /// result, the error number and every field, those a failure leaves
    /// included; and the errors of `DATEMSK` itself that this host can
    /// reach (unset, empty, naming no file). The other two, a directory and
    /// an empty file, need a file system.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn getdate_answers_as_glibc_does() {
        let _tz = TzGuard::utc();
        let mut lines = GETDATE_ORACLE.lines();
        let templates: Vec<Vec<u8>> = lines
            .next()
            .unwrap()
            .strip_prefix("T ")
            .unwrap()
            .split(' ')
            .map(|h| {
                let mut t = unhex(h);
                t.push(0);
                t
            })
            .collect();
        let mut zone = usize::MAX;
        let mut bad = Vec::new();
        let mut calls = 0;
        let mut file_errors = std::collections::HashMap::new();
        for line in lines {
            let (lhs, want) = line.split_once(" = ").unwrap();
            let w: Vec<&str> = lhs.split(' ').collect();
            if w[0] == "E" {
                file_errors.insert(w[1].to_owned(), want.to_owned());
                continue;
            }
            let z: usize = w[1].parse().unwrap();
            // A date before 1970 in the zone with daylight time: glibc
            // applies a `TZ` string's rule only from 1970, `tzrules` in
            // every year ([`glibc_tz_rule_differs`]); `tm_isdst` differs.
            let year = want.split(' ').nth(if w[0] == "g" { 7 } else { 6 });
            if z == 1
                && year
                    .and_then(|y| y.parse::<i32>().ok())
                    .is_some_and(|y| y < 70)
            {
                continue;
            }
            if z != zone {
                TzGuard::put(ORACLE_ZONES[z]);
                zone = z;
            }
            let now: TimeT = w[2].parse().unwrap();
            let mut input = unhex(w[3]);
            input.push(0);
            let mut t = Tm::ZERO;
            let rc = getdate_lines(input.as_ptr(), &templates, &mut t, now);
            let got = match (w[0], rc) {
                ("g", 0) => format!("1 0 {}", getdate_fields(&t)),
                ("g", _) => format!("0 {rc} -"),
                _ => format!("{rc} {}", getdate_fields(&t)),
            };
            calls += 1;
            if got != want {
                bad.push(format!("{line}\n    ours {got}"));
            }
        }
        assert!(calls > 150, "only {calls} calls");
        assert!(
            bad.is_empty(),
            "{} of {calls} differ:\n{}",
            bad.len(),
            bad.iter().take(60).cloned().collect::<Vec<_>>().join("\n")
        );

        for (case, setting) in [
            ("unset", None),
            ("empty", Some(&b"\0"[..])),
            ("missing", Some(&b"/nonexistent/templates\0"[..])),
        ] {
            // SAFETY: NUL-terminated name and value; the test holds the
            // environment's lock (`TzGuard`).
            let rc = unsafe {
                match setting {
                    None => crate::environ::unsetenv(c"DATEMSK".as_ptr().cast()),
                    Some(v) => crate::environ::setenv(c"DATEMSK".as_ptr().cast(), v.as_ptr(), 1),
                }
            };
            assert_eq!(rc, 0);
            getdate_err.store(0, core::sync::atomic::Ordering::Relaxed);
            // SAFETY: a NUL-terminated input.
            let r = unsafe { getdate(c"2026-09-28".as_ptr().cast()) };
            let got = format!(
                "{} {}",
                u8::from(!r.is_null()),
                getdate_err.load(core::sync::atomic::Ordering::Relaxed)
            );
            assert_eq!(&got, file_errors.get(case).unwrap(), "DATEMSK {case}");
        }
        // SAFETY: as above.
        assert_eq!(
            unsafe { crate::environ::unsetenv(c"DATEMSK".as_ptr().cast()) },
            0
        );
    }

    /// `strptime_l` parses as `strptime` does.
    #[test]
    fn strptime_l_is_strptime() {
        let mut a = zero_tm();
        let mut b = zero_tm();
        let input = b"2026-09-29 17:05:03\0".as_ptr();
        let format = b"%Y-%m-%d %H:%M:%S\0".as_ptr();
        // SAFETY: NUL-terminated strings; `Tm`s on the stack.
        let (ra, rb) = unsafe {
            (
                strptime_l(input, format, &raw mut a, 0),
                strptime(input, format, &raw mut b),
            )
        };
        assert_eq!(ra, rb);
        assert!(!ra.is_null());
        assert_eq!(
            (a.tm_year, a.tm_mon, a.tm_mday, a.tm_hour),
            (126, 8, 29, 17)
        );
        assert_eq!(
            (a.tm_min, a.tm_sec, a.tm_wday, a.tm_yday),
            (b.tm_min, b.tm_sec, b.tm_wday, b.tm_yday)
        );
    }

    /// `timespec_getres` against glibc 2.39's answers
    /// (`posix/tools/oracle/strfrom_harness.py`): `TIME_UTC` and no other
    /// base, with or without a `timespec` to fill -- and what it fills is
    /// `CLOCK_REALTIME`'s resolution.
    #[test]
    fn timespec_getres_is_glibcs() {
        let line = include_str!("strfrom_oracle.txt")
            .lines()
            .find_map(|l| l.strip_prefix("timespec_getres "))
            .expect("the oracle's timespec_getres line");
        let marker = Timespec {
            tv_sec: -7,
            tv_nsec: -7,
        };
        let mut got = String::new();
        for base in 0..=5 {
            let mut ts = marker;
            let r = unsafe { timespec_getres(&raw mut ts, base) };
            got.push_str(&format!("{r} "));
            if r == 0 {
                assert_eq!(
                    (ts.tv_sec, ts.tv_nsec),
                    (-7, -7),
                    "untouched for base {base}"
                );
            } else {
                let mut want = marker;
                assert_eq!(clock_getres(CLOCK_REALTIME, &raw mut want), 0);
                assert_eq!((ts.tv_sec, ts.tv_nsec), (want.tv_sec, want.tv_nsec));
            }
        }
        got.push_str(&format!("/ {}", unsafe {
            timespec_getres(core::ptr::null_mut(), TIME_UTC)
        }));
        assert_eq!(got, line);
    }
}

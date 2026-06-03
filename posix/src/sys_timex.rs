//! `<sys/timex.h>` — clock adjustment interface.
//!
//! Provides the `Timex` struct and `adjtimex()`/`ntp_adjtime()`
//! for kernel clock discipline (NTP, PTP, etc.).

use crate::errno;
// Only the real OS build (target_os = "none") issues native syscalls; the host
// test build links no kernel, so the syscall path is compiled out there (see
// the ADJ_SETOFFSET application in `adjtimex`).
#[cfg(target_os = "none")]
use crate::syscall::{SYS_CLOCK_ADJTIME, syscall1};

// ---------------------------------------------------------------------------
// Mode bits for Timex.modes
// ---------------------------------------------------------------------------

/// Adjust offset.
pub const ADJ_OFFSET: u32 = 0x0001;
/// Adjust frequency.
pub const ADJ_FREQUENCY: u32 = 0x0002;
/// Adjust maximum time error.
pub const ADJ_MAXERROR: u32 = 0x0004;
/// Adjust estimated time error.
pub const ADJ_ESTERROR: u32 = 0x0008;
/// Set clock status bits.
pub const ADJ_STATUS: u32 = 0x0010;
/// Adjust PLL time constant.
pub const ADJ_TIMECONST: u32 = 0x0020;
/// Set TAI offset.
pub const ADJ_TAI: u32 = 0x0080;
/// Select microsecond resolution.
pub const ADJ_MICRO: u32 = 0x1000;
/// Select nanosecond resolution.
pub const ADJ_NANO: u32 = 0x2000;
/// Set time (absolute).
pub const ADJ_SETOFFSET: u32 = 0x0100;
/// Adjust tick value.
pub const ADJ_TICK: u32 = 0x4000;
/// Don't actually adjust — just return status.
pub const MOD_OFFSET: u32 = ADJ_OFFSET;
/// Alias for `ADJ_FREQUENCY`.
pub const MOD_FREQUENCY: u32 = ADJ_FREQUENCY;
/// Alias for `ADJ_MAXERROR`.
pub const MOD_MAXERROR: u32 = ADJ_MAXERROR;
/// Alias for `ADJ_ESTERROR`.
pub const MOD_ESTERROR: u32 = ADJ_ESTERROR;
/// Alias for `ADJ_STATUS`.
pub const MOD_STATUS: u32 = ADJ_STATUS;
/// Alias for `ADJ_TIMECONST`.
pub const MOD_TIMECONST: u32 = ADJ_TIMECONST;

// ---------------------------------------------------------------------------
// Status bits in Timex.status
// ---------------------------------------------------------------------------

/// Phase-locked loop updates enabled.
pub const STA_PLL: i32 = 0x0001;
/// Insert leap second.
pub const STA_INS: i32 = 0x0010;
/// Delete leap second.
pub const STA_DEL: i32 = 0x0020;
/// Clock unsynchronized.
pub const STA_UNSYNC: i32 = 0x0040;
/// Frequency hold mode.
pub const STA_FREQHOLD: i32 = 0x0080;
/// PPS (pulse per second) signal present.
pub const STA_PPSSIGNAL: i32 = 0x0100;
/// PPS signal jitter exceeded.
pub const STA_PPSJITTER: i32 = 0x0200;
/// PPS signal wander exceeded.
pub const STA_PPSWANDER: i32 = 0x0400;
/// PPS signal calibration error.
pub const STA_PPSERROR: i32 = 0x0800;
/// Clock hardware fault.
pub const STA_CLOCKERR: i32 = 0x1000;
/// Nanosecond mode active.
pub const STA_NANO: i32 = 0x2000;

// ---------------------------------------------------------------------------
// Return codes from adjtimex
// ---------------------------------------------------------------------------

/// Clock synchronized.
pub const TIME_OK: i32 = 0;
/// Insert leap second.
pub const TIME_INS: i32 = 1;
/// Delete leap second.
pub const TIME_DEL: i32 = 2;
/// Leap second in progress.
pub const TIME_OOP: i32 = 3;
/// Leap second has occurred.
pub const TIME_WAIT: i32 = 4;
/// Clock not synchronized.
pub const TIME_ERROR: i32 = 5;

// ---------------------------------------------------------------------------
// ADJ_TICK validation bounds
// ---------------------------------------------------------------------------
//
// Linux's `kernel/time/ntp.c::ntp_validate_timex` rejects any `ADJ_TICK`
// update whose `tick` value is more than ±10% off the default jiffy
// length:
//
//     if (txc->modes & ADJ_TICK &&
//         (txc->tick <  900000/USER_HZ ||
//          txc->tick > 1100000/USER_HZ))
//         return -EINVAL;
//
// USER_HZ is 100 on x86_64 glibc, so the inclusive range is [9000,
// 11000] microseconds.  Values like 0, 1, or `i64::MAX` would derail
// NTP discipline if applied silently — chrony, ntpd, and Java's clock
// probes all rely on this check fronting the kernel.

/// Linux's `USER_HZ` — the userspace-visible clock-tick rate, fixed at
/// 100 on x86_64 glibc regardless of the kernel's internal `HZ`.
pub const USER_HZ: i64 = 100;

/// Minimum accepted `ADJ_TICK` value (microseconds per jiffy, –10%).
pub const MIN_TICK: i64 = 900_000 / USER_HZ;

/// Maximum accepted `ADJ_TICK` value (microseconds per jiffy, +10%).
pub const MAX_TICK: i64 = 1_100_000 / USER_HZ;

// ---------------------------------------------------------------------------
// ADJ_SETOFFSET sub-second bounds
// ---------------------------------------------------------------------------
//
// Linux's `kernel/time/ntp.c::ntp_validate_timex` (via
// `timeval_inject_offset_valid` / `timespec_inject_offset_valid`)
// rejects an `ADJ_SETOFFSET` whose sub-second field is negative or
// `>= one second`:
//
//     if (tv->tv_usec >= USEC_PER_SEC || tv->tv_usec < 0)
//         return false;     // (nsec version uses NSEC_PER_SEC)
//
// The sub-second field is interpreted as nanoseconds when `ADJ_NANO`
// is also requested, otherwise as microseconds.  Pre-fix we silently
// accepted any value here; chrony's `clock_step()` would happily pass
// 1_999_999 usec and we would mis-apply the offset.

/// Linux's `USEC_PER_SEC`.
pub const USEC_PER_SEC: i64 = 1_000_000;

/// Linux's `NSEC_PER_SEC`.
pub const NSEC_PER_SEC: i64 = 1_000_000_000;

// ---------------------------------------------------------------------------
// Timex struct
// ---------------------------------------------------------------------------

/// Kernel clock discipline parameters.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Timex {
    /// Mode selector (ADJ_* bits).
    pub modes: u32,
    /// Time offset (usec or nsec).
    pub offset: i64,
    /// Frequency offset (scaled ppm).
    pub freq: i64,
    /// Maximum time error (usec).
    pub maxerror: i64,
    /// Estimated time error (usec).
    pub esterror: i64,
    /// Clock command/status.
    pub status: i32,
    /// PLL time constant.
    pub constant: i64,
    /// Clock precision (usec).
    pub precision: i64,
    /// Clock frequency tolerance (scaled ppm).
    pub tolerance: i64,
    /// Current time (seconds).
    pub time_tv_sec: i64,
    /// Current time (usec or nsec).
    pub time_tv_usec: i64,
    /// PPS jitter (usec).
    pub tick: i64,
    /// PPS calibration interval (sec).
    pub ppsfreq: i64,
    /// PPS jitter (usec).
    pub jitter: i64,
    /// PPS stability.
    pub shift: i32,
    /// PPS stability.
    pub stabil: i64,
    /// PPS jitter limit exceeded count.
    pub jitcnt: i64,
    /// PPS calibration intervals.
    pub calcnt: i64,
    /// PPS calibration errors.
    pub errcnt: i64,
    /// PPS stability limit exceeded count.
    pub stbcnt: i64,
    /// TAI offset (sec).
    pub tai: i32,
    /// Padding.
    _pad: [u8; 44],
}

impl Timex {
    /// Create a zeroed `Timex`.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Process-local NTP discipline state
// ---------------------------------------------------------------------------
//
// We don't have a real-time NTP-disciplined clock (no RTC infrastructure
// yet — the kernel exposes only a monotonic counter).  Rather than fail
// every NTP-client call, we remember the parameters that callers write
// and reflect them back on subsequent reads.  This makes ntpd / chronyd
// / Java's `LD_PRELOAD`-style clock probes happy without lying about
// what the kernel actually disciplines.
//
// The state is protected by a spinlock because adjtimex can be called
// from multiple threads.

use core::sync::atomic::{AtomicBool, Ordering};

/// Process-local NTP discipline parameters.  Mirrors the subset of
/// `Timex` fields that adjtimex actually carries between calls.
struct TimexState {
    offset: i64,
    freq: i64,
    maxerror: i64,
    esterror: i64,
    status: i32,
    constant: i64,
    tick: i64,
    tai: i32,
}

static TIMEX_LOCK: AtomicBool = AtomicBool::new(false);
static mut TIMEX_STATE: TimexState = TimexState {
    offset: 0,
    // Default NTP frequency tolerance: 32_768_000 scaled ppm (Linux's
    // `MAXFREQ * (1 << 16)`).
    freq: 0,
    maxerror: 16_000_000,
    esterror: 16_000_000,
    // Clock is unsynchronized until NTP discipline is engaged.
    status: STA_UNSYNC,
    constant: 2,
    // Linux default jiffy tick (USEC_PER_SEC / HZ at HZ=100).
    tick: 10_000,
    tai: 0,
};

/// RAII guard for the TIMEX spinlock.
struct TimexLockGuard;
impl Drop for TimexLockGuard {
    fn drop(&mut self) {
        TIMEX_LOCK.store(false, Ordering::Release);
    }
}

/// Acquire the TIMEX spinlock.  Spins (with `core::hint::spin_loop`)
/// until acquired; safe to call from anywhere.
fn lock_timex() -> TimexLockGuard {
    while TIMEX_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    TimexLockGuard
}

/// Convert the saved state's `status` into the adjtimex return code.
///
/// Linux returns one of `TIME_OK`, `TIME_INS`, `TIME_DEL`, `TIME_OOP`,
/// `TIME_WAIT`, or `TIME_ERROR`.  `TIME_ERROR` is used whenever the
/// `STA_UNSYNC` bit is set.
fn status_to_return(status: i32) -> i32 {
    if (status & STA_UNSYNC) != 0 {
        return TIME_ERROR;
    }
    if (status & STA_INS) != 0 {
        return TIME_INS;
    }
    if (status & STA_DEL) != 0 {
        return TIME_DEL;
    }
    TIME_OK
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Adjust the kernel clock (Linux `adjtimex(2)`).
///
/// Reads `tx->modes` to decide which subset of fields to apply, then
/// fills the entire struct with the current discipline state and
/// returns the appropriate `TIME_*` code.  Setting `modes = 0` is a
/// read-only query.
///
/// # Errors (Linux-matching priority order)
///
/// 1. `EFAULT` — `tx` is NULL.
/// 2. `EINVAL` — `modes` contains an unrecognised bit, or
///    `ADJ_TICK`/`ADJ_SETOFFSET` carries an out-of-range value
///    (matches Linux's `ntp_validate_timex`).
/// 3. **Phase 173:** `EPERM` — `modes != 0` (i.e. the caller is
///    requesting a write) and the caller lacks `CAP_SYS_TIME`.
///    Read-only queries (`modes == 0`) require no capability.
///
/// The cap check sits after argument-domain `EINVAL` because Linux's
/// `do_adjtimex` calls `ntp_validate_timex` *before* the
/// `capable(CAP_SYS_TIME)` probe — so a bad mode bit or a wild tick
/// value still returns `EINVAL` to an unprivileged caller, never
/// `EPERM`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn adjtimex(tx: *mut Timex) -> i32 {
    if tx.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: caller contract — `tx` points to a writable Timex.
    let modes = unsafe { (*tx).modes };

    // Reject mode bits we don't recognise.
    const KNOWN_MODES: u32 = ADJ_OFFSET
        | ADJ_FREQUENCY
        | ADJ_MAXERROR
        | ADJ_ESTERROR
        | ADJ_STATUS
        | ADJ_TIMECONST
        | ADJ_TAI
        | ADJ_MICRO
        | ADJ_NANO
        | ADJ_SETOFFSET
        | ADJ_TICK;
    if (modes & !KNOWN_MODES) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Phase 161: Linux's `ntp_validate_timex` rejects ADJ_TICK with a
    // tick value outside [MIN_TICK, MAX_TICK] (±10% of the default
    // jiffy length).  Pre-fix we silently applied any tick value the
    // caller passed, including 0 and `i64::MAX` — a divergence that
    // would let buggy or malicious callers derail the NTP discipline
    // state.  We validate before taking the lock so the state stays
    // untouched on rejection.
    if (modes & ADJ_TICK) != 0 {
        // SAFETY: caller contract — `tx` points to a readable Timex.
        let tick = unsafe { (*tx).tick };
        if !(MIN_TICK..=MAX_TICK).contains(&tick) {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }

    // Phase 162: Linux's `ntp_validate_timex` rejects an
    // `ADJ_SETOFFSET` whose sub-second field is out of range.  The
    // field is interpreted as nanoseconds if `ADJ_NANO` is set in the
    // same call, otherwise as microseconds.  Pre-fix we silently
    // accepted any value (negative, ≥ 1s) and dropped it on the floor
    // because we don't yet have an RTC to apply the step to —
    // surfacing the EINVAL still matters because chrony's
    // `clock_step()` falls back to settimeofday on a SETOFFSET
    // failure, and silently "succeeding" hides the bug.
    if (modes & ADJ_SETOFFSET) != 0 {
        // SAFETY: caller contract — `tx` points to a readable Timex.
        let sub_sec = unsafe { (*tx).time_tv_usec };
        let limit = if (modes & ADJ_NANO) != 0 {
            NSEC_PER_SEC
        } else {
            USEC_PER_SEC
        };
        if !(0..limit).contains(&sub_sec) {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }

    // Phase 173: Linux's `do_adjtimex` gates any modify-mode call on
    // CAP_SYS_TIME.  A pure read-only query (modes == 0) is allowed for
    // unprivileged callers — only writes need the cap.  The probe runs
    // after `ntp_validate_timex` (our EINVAL guards above), matching the
    // kernel's `validate then capable` ordering.
    if modes != 0 && !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_TIME) {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    // ADJ_SETOFFSET requests an immediate step of the wall clock by the
    // supplied (possibly negative) offset.  Capture the delta now — before the
    // read-back below zeroes the tx time fields — and apply it to the kernel
    // clock after the discipline state is updated.  The sub-second field is
    // nanoseconds when ADJ_NANO is set, otherwise microseconds (already range-
    // checked above); the seconds field is signed, so the total may be
    // negative (stepping the clock backwards).  saturating_* guards the
    // astronomically large offsets that would only arise from a malformed tx.
    let setoffset_delta_ns: Option<i64> = if (modes & ADJ_SETOFFSET) != 0 {
        // SAFETY: caller contract — `tx` points to a readable Timex.
        let (secs, sub) = unsafe { ((*tx).time_tv_sec, (*tx).time_tv_usec) };
        let sub_ns = if (modes & ADJ_NANO) != 0 {
            sub
        } else {
            sub.saturating_mul(1_000)
        };
        Some(secs.saturating_mul(1_000_000_000).saturating_add(sub_ns))
    } else {
        None
    };

    let _guard = lock_timex();

    // SAFETY: serialized by TIMEX_LOCK.
    let state = unsafe { &mut *core::ptr::addr_of_mut!(TIMEX_STATE) };

    // SAFETY: caller-supplied writable struct.
    unsafe {
        // Apply each requested update.
        if (modes & ADJ_OFFSET) != 0 {
            state.offset = (*tx).offset;
        }
        if (modes & ADJ_FREQUENCY) != 0 {
            state.freq = (*tx).freq;
        }
        if (modes & ADJ_MAXERROR) != 0 {
            state.maxerror = (*tx).maxerror;
        }
        if (modes & ADJ_ESTERROR) != 0 {
            state.esterror = (*tx).esterror;
        }
        if (modes & ADJ_STATUS) != 0 {
            state.status = (*tx).status;
        }
        if (modes & ADJ_TIMECONST) != 0 {
            state.constant = (*tx).constant;
        }
        if (modes & ADJ_TAI) != 0 {
            state.tai = (*tx).tai;
        }
        if (modes & ADJ_TICK) != 0 {
            state.tick = (*tx).tick;
        }
        // ADJ_NANO / ADJ_MICRO toggle the STA_NANO bit but otherwise
        // don't carry a value.
        if (modes & ADJ_NANO) != 0 {
            state.status |= STA_NANO;
        }
        if (modes & ADJ_MICRO) != 0 {
            state.status &= !STA_NANO;
        }

        // Now read everything back into the caller's struct.
        (*tx).offset = state.offset;
        (*tx).freq = state.freq;
        (*tx).maxerror = state.maxerror;
        (*tx).esterror = state.esterror;
        (*tx).status = state.status;
        (*tx).constant = state.constant;
        (*tx).precision = 1; // 1 unit (nano if STA_NANO else micro).
        // 32_768_000 scaled ppm = NTP's MAXFREQ default.
        (*tx).tolerance = 32_768_000;
        (*tx).tick = state.tick;
        (*tx).tai = state.tai;
        // Wall clock fields we don't track stay at whatever the caller
        // wrote — set them to 0 so reads after a fresh adjtimex are
        // deterministic.
        (*tx).time_tv_sec = 0;
        (*tx).time_tv_usec = 0;
        (*tx).ppsfreq = 0;
        (*tx).jitter = 0;
        (*tx).shift = 0;
        (*tx).stabil = 0;
        (*tx).jitcnt = 0;
        (*tx).calcnt = 0;
        (*tx).errcnt = 0;
        (*tx).stbcnt = 0;
    }

    // Apply the ADJ_SETOFFSET clock step to the kernel wall clock.  This is
    // the abrupt correction chrony/ntpd issue via clock_step(); before this
    // was wired, the step was validated and "succeeded" but never moved the
    // clock, so the daemon believed it had stepped and would not fall back to
    // settimeofday.  We discard the kernel return: SYS_CLOCK_ADJTIME only
    // fails (EINVAL) when the realtime base is uninitialized (no usable RTC),
    // in which case the clock is 0-based anyway and there is nothing to step;
    // adjtimex's return value reflects the NTP status word, not the step
    // result, matching Linux's do_adjtimex semantics.
    #[cfg(target_os = "none")]
    #[allow(clippy::cast_sign_loss)]
    if let Some(delta_ns) = setoffset_delta_ns {
        let _ = syscall1(SYS_CLOCK_ADJTIME, delta_ns as u64);
    }
    // On the host test build there is no kernel to step; the discipline-state
    // update and return value above are identical on both builds (which is
    // what the host tests assert), so we only need to keep the captured delta
    // from tripping the unused-variable lint.
    #[cfg(not(target_os = "none"))]
    let _ = &setoffset_delta_ns;

    status_to_return(state.status)
}

/// NTP-compatible clock adjustment (identical to `adjtimex`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ntp_adjtime(tx: *mut Timex) -> i32 {
    adjtimex(tx)
}

// ---------------------------------------------------------------------------
// clock_adjtime clock-id dispatch table
// ---------------------------------------------------------------------------
//
// Linux's `clock_adjtime(2)` (`kernel/time/posix-timers.c`) dispatches
// through the per-clock `k_clock` table:
//
//     int do_clock_adjtime(const clockid_t id, struct __kernel_timex *t)
//     {
//         const struct k_clock *kc = clockid_to_kclock(id);
//
//         if (!kc)              return -EINVAL;       // unknown clock
//         if (!kc->clock_adj)   return -EOPNOTSUPP;   // known, no adj
//         return kc->clock_adj(id, t);
//     }
//
// Only `CLOCK_REALTIME` and `CLOCK_TAI` have a `clock_adj` callback.
// Every other standard clock (MONOTONIC, BOOTTIME, the CPU-time
// clocks, the COARSE variants, the ALARM variants) is recognised but
// returns `EOPNOTSUPP`.  Pre-Phase-163 we lumped MONOTONIC in with
// REALTIME and rejected everything else with `EINVAL` — both
// divergent.
//
// We also follow Linux's `SYSCALL_DEFINE2(clock_adjtime)` ordering:
// `copy_from_user` runs *before* `do_clock_adjtime`, so a null `tx`
// returns `EFAULT` even when `clk_id` is also bogus.  Pre-fix our
// EINVAL fired first on `clock_adjtime(99, NULL)` — fixed by
// hoisting the null-tx check above the clock-id dispatch.

/// Standard POSIX clock ids recognised by `clock_adjtime`.  These
/// mirror `linux_clock2_types::CLOCK_*` but kept as `i32` because the
/// syscall takes a signed `clockid_t`.
const CLOCK_REALTIME_ID: i32 = 0;
const CLOCK_MONOTONIC_ID: i32 = 1;
const CLOCK_PROCESS_CPUTIME_ID_ID: i32 = 2;
const CLOCK_THREAD_CPUTIME_ID_ID: i32 = 3;
const CLOCK_MONOTONIC_RAW_ID: i32 = 4;
const CLOCK_REALTIME_COARSE_ID: i32 = 5;
const CLOCK_MONOTONIC_COARSE_ID: i32 = 6;
const CLOCK_BOOTTIME_ID: i32 = 7;
const CLOCK_REALTIME_ALARM_ID: i32 = 8;
const CLOCK_BOOTTIME_ALARM_ID: i32 = 9;
const CLOCK_TAI_ID: i32 = 11;

/// Per-clock adjtimex (`clock_adjtime(2)`).
///
/// Linux dispatches by clock id: `CLOCK_REALTIME` and `CLOCK_TAI`
/// forward to `do_adjtimex`; every other *known* standard clock
/// returns `EOPNOTSUPP`; unknown clock ids return `EINVAL`.  A null
/// `tx` always returns `EFAULT`, even when the clock id is also
/// invalid — matching `SYSCALL_DEFINE2`'s `copy_from_user`-first
/// ordering.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_adjtime(clk_id: i32, tx: *mut Timex) -> i32 {
    // Linux's `copy_from_user` runs before the clock-id dispatch, so
    // EFAULT beats EINVAL/EOPNOTSUPP.
    if tx.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    match clk_id {
        // Adjustable clocks — forward to the shared NTP state.  TAI
        // shares the same discipline; the +/-leap offset would be
        // applied to reads, but we don't yet expose a wall clock.
        CLOCK_REALTIME_ID | CLOCK_TAI_ID => adjtimex(tx),

        // Known clocks that don't support `clock_adj` in Linux.
        CLOCK_MONOTONIC_ID
        | CLOCK_PROCESS_CPUTIME_ID_ID
        | CLOCK_THREAD_CPUTIME_ID_ID
        | CLOCK_MONOTONIC_RAW_ID
        | CLOCK_REALTIME_COARSE_ID
        | CLOCK_MONOTONIC_COARSE_ID
        | CLOCK_BOOTTIME_ID
        | CLOCK_REALTIME_ALARM_ID
        | CLOCK_BOOTTIME_ALARM_ID => {
            errno::set_errno(errno::EOPNOTSUPP);
            -1
        }

        // Unknown clock id.
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timex_struct_size() {
        // Timex is a large struct — at least 200 bytes.
        assert!(core::mem::size_of::<Timex>() > 100);
    }

    #[test]
    fn test_timex_zeroed() {
        let tx = Timex::zeroed();
        assert_eq!(tx.modes, 0);
        assert_eq!(tx.offset, 0);
        assert_eq!(tx.status, 0);
        assert_eq!(tx.tai, 0);
    }

    #[test]
    fn test_adj_mode_bits() {
        assert_eq!(ADJ_OFFSET, 0x0001);
        assert_eq!(ADJ_FREQUENCY, 0x0002);
        assert_eq!(ADJ_NANO, 0x2000);
        // MOD_ aliases match ADJ_.
        assert_eq!(MOD_OFFSET, ADJ_OFFSET);
        assert_eq!(MOD_FREQUENCY, ADJ_FREQUENCY);
    }

    #[test]
    fn test_adj_bits_distinct() {
        let bits = [
            ADJ_OFFSET,
            ADJ_FREQUENCY,
            ADJ_MAXERROR,
            ADJ_ESTERROR,
            ADJ_STATUS,
            ADJ_TIMECONST,
            ADJ_TAI,
            ADJ_MICRO,
            ADJ_NANO,
            ADJ_SETOFFSET,
            ADJ_TICK,
        ];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j], "ADJ_ bits must be distinct");
            }
        }
    }

    #[test]
    fn test_status_bits() {
        assert_eq!(STA_PLL, 0x0001);
        assert_eq!(STA_UNSYNC, 0x0040);
        assert_eq!(STA_NANO, 0x2000);
    }

    #[test]
    fn test_time_return_codes() {
        assert_eq!(TIME_OK, 0);
        assert_eq!(TIME_ERROR, 5);
        assert_ne!(TIME_INS, TIME_DEL);
    }

    /// Serializes tests that touch the global TIMEX_STATE.
    static TIMEX_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Restore TIMEX_STATE to its boot defaults so each test starts
    /// from a known baseline.
    fn reset_timex_state() {
        let _guard = lock_timex();
        // SAFETY: serialized by TIMEX_LOCK.
        unsafe {
            let s = &mut *core::ptr::addr_of_mut!(TIMEX_STATE);
            s.offset = 0;
            s.freq = 0;
            s.maxerror = 16_000_000;
            s.esterror = 16_000_000;
            s.status = STA_UNSYNC;
            s.constant = 2;
            s.tick = 10_000;
            s.tai = 0;
        }
    }

    #[test]
    fn test_adjtimex_null_efault() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let ret = adjtimex(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_adjtimex_read_only_returns_time_error_when_unsync() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        // modes=0 = read-only query.
        let ret = adjtimex(&mut tx);
        // Boot default has STA_UNSYNC set → TIME_ERROR.
        assert_eq!(ret, TIME_ERROR);
        // Status field is populated from state.
        assert_ne!(tx.status & STA_UNSYNC, 0);
        // Default tick reflected.
        assert_eq!(tx.tick, 10_000);
    }

    #[test]
    fn test_adjtimex_set_offset_persists() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_OFFSET;
        tx.offset = 12_345;
        let _ = adjtimex(&mut tx);
        // Read back.
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(tx2.offset, 12_345);
    }

    #[test]
    fn test_adjtimex_set_status_clears_unsync_returns_time_ok() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_STATUS;
        tx.status = STA_PLL; // No STA_UNSYNC, no leap.
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, TIME_OK);
    }

    #[test]
    fn test_adjtimex_set_status_with_leap_ins_returns_time_ins() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_STATUS;
        tx.status = STA_INS; // Synchronized, leap-second insert pending.
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, TIME_INS);
    }

    #[test]
    fn test_adjtimex_set_status_with_leap_del_returns_time_del() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_STATUS;
        tx.status = STA_DEL;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, TIME_DEL);
    }

    #[test]
    fn test_adjtimex_adj_nano_sets_sta_nano() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_NANO;
        let _ = adjtimex(&mut tx);
        // STA_NANO should now be set in the state.
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_ne!(tx2.status & STA_NANO, 0);
    }

    #[test]
    fn test_adjtimex_adj_micro_clears_sta_nano() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        // First set nano.
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_NANO;
        let _ = adjtimex(&mut tx);
        // Now clear it.
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_MICRO;
        let _ = adjtimex(&mut tx);
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(tx2.status & STA_NANO, 0);
    }

    #[test]
    fn test_adjtimex_unknown_modes_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = 0x8000_0000; // Not in KNOWN_MODES.
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_fills_tolerance_and_precision() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        let _ = adjtimex(&mut tx);
        // tolerance is the NTP MAXFREQ default.
        assert_eq!(tx.tolerance, 32_768_000);
        // precision = 1 unit.
        assert_eq!(tx.precision, 1);
    }

    #[test]
    fn test_ntp_adjtime_matches_adjtimex() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        let ret = ntp_adjtime(&mut tx);
        assert_eq!(ret, TIME_ERROR);
    }

    #[test]
    fn test_clock_adjtime_realtime_works() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        let ret = clock_adjtime(0, &mut tx); // CLOCK_REALTIME
        assert_eq!(ret, TIME_ERROR);
    }

    /// Phase 163: `clock_adjtime(CLOCK_MONOTONIC, ...)` resolves to
    /// `EOPNOTSUPP` after the clock-id dispatch fix — Linux never
    /// allowed adjtimex on MONOTONIC.  Renamed from
    /// `test_clock_adjtime_monotonic_works` to reflect the new
    /// expected outcome; the post-fix behaviour is asserted directly
    /// here so the original name doesn't suggest the old (wrong)
    /// semantics.
    #[test]
    fn test_clock_adjtime_monotonic_returns_eopnotsupp_phase163() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        let ret = clock_adjtime(1, &mut tx); // CLOCK_MONOTONIC
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_clock_adjtime_unknown_clock_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        let ret = clock_adjtime(99, &mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_adjtime_null_tx_efault() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let ret = clock_adjtime(0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // ------------------------------------------------------------------
    // Phase 161 — ADJ_TICK out-of-range rejection (Linux ABI parity)
    // ------------------------------------------------------------------
    //
    // Linux's `kernel/time/ntp.c::ntp_validate_timex` rejects an
    // ADJ_TICK update whose `tick` value lies outside ±10% of the
    // default jiffy length:
    //
    //     if (txc->modes & ADJ_TICK &&
    //         (txc->tick <  900000/USER_HZ ||
    //          txc->tick > 1100000/USER_HZ))
    //         return -EINVAL;
    //
    // With USER_HZ=100 the inclusive range is [9000, 11000].  Pre-fix
    // we silently applied any tick value the caller passed (0, 1,
    // i64::MAX, negatives), which would derail the NTP discipline
    // state.  Post-fix we reject the bad values with EINVAL while
    // still accepting the two endpoints and any in-range value.

    // ---- Per-error-class ----

    #[test]
    fn test_adjtimex_phase161_tick_zero_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = 0;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase161_tick_negative_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = -1;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase161_tick_i64_min_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = i64::MIN;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase161_tick_i64_max_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = i64::MAX;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase161_tick_just_below_min_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = MIN_TICK - 1; // 8999
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase161_tick_just_above_max_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = MAX_TICK + 1; // 11001
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Boundary acceptance ----

    #[test]
    fn test_adjtimex_phase161_tick_at_min_accepted() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = MIN_TICK; // 9000
        let ret = adjtimex(&mut tx);
        // Boot default has STA_UNSYNC → TIME_ERROR, but no errno.
        assert_eq!(ret, TIME_ERROR);
        // Read back: tick was applied.
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(tx2.tick, MIN_TICK);
    }

    #[test]
    fn test_adjtimex_phase161_tick_at_max_accepted() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = MAX_TICK; // 11000
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, TIME_ERROR);
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(tx2.tick, MAX_TICK);
    }

    #[test]
    fn test_adjtimex_phase161_tick_default_accepted() {
        // Default ntp tick = USEC_PER_SEC / USER_HZ = 10_000.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = 10_000;
        let _ = adjtimex(&mut tx);
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(tx2.tick, 10_000);
    }

    // ---- Ordering matrix ----

    #[test]
    fn test_adjtimex_phase161_null_ptr_beats_tick_einval() {
        // EFAULT (null ptr) fires before mode/tick validation —
        // the EFAULT check happens before the deref that reads tick.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let ret = adjtimex(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_adjtimex_phase161_unknown_mode_beats_tick_einval() {
        // Unknown mode-bit check fires before tick range check.
        // Both would yield EINVAL, but the unknown-mode branch returns
        // first; we confirm by setting a clearly-bad mode + bad tick
        // and asserting EINVAL (and that the state is untouched).
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK | 0x8000_0000; // unknown bit + tick
        tx.tick = 0; // would also be invalid
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // State must not have been mutated.
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(tx2.tick, 10_000);
    }

    // ---- No-side-effect (state untouched on rejection) ----

    #[test]
    fn test_adjtimex_phase161_bad_tick_leaves_state_untouched() {
        // Even when ADJ_TICK is combined with other in-band updates
        // (ADJ_OFFSET), a bad tick value must abort the whole call
        // before any state mutation — matching Linux's
        // validate-then-apply ordering.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_OFFSET | ADJ_TICK;
        tx.offset = 999_999;
        tx.tick = 0; // bad
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // Read back: neither offset nor tick was applied.
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(
            tx2.offset, 0,
            "offset must NOT be applied when tick is invalid"
        );
        assert_eq!(tx2.tick, 10_000, "tick must remain at default");
    }

    // ---- Real-world workflow ----

    #[test]
    fn test_adjtimex_phase161_ntpd_tick_adjustment_workflow() {
        // ntpd periodically adjusts tick to compensate for crystal
        // drift, staying within a few hundred microseconds of 10000.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        // Drift up by 0.1%.
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = 10_010;
        let _ = adjtimex(&mut tx);
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(tx2.tick, 10_010);
        // Drift down.
        let mut tx3 = Timex::zeroed();
        tx3.modes = ADJ_TICK;
        tx3.tick = 9_990;
        let _ = adjtimex(&mut tx3);
        let mut tx4 = Timex::zeroed();
        let _ = adjtimex(&mut tx4);
        assert_eq!(tx4.tick, 9_990);
    }

    #[test]
    fn test_adjtimex_phase161_buggy_caller_uninit_tick() {
        // C code: `struct timex tx = { .modes = ADJ_TICK };` where
        // `tx.tick` is left at its zero-initialised value.  Pre-fix:
        // silently set tick to 0 and broke the NTP state.  Post-fix:
        // EINVAL surfaces the bug.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK; // tx.tick stays at the zeroed default (0)
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Recovery ----

    #[test]
    fn test_adjtimex_phase161_recovery_after_bad_tick() {
        // An EINVAL from a bad tick must not poison a follow-up call
        // with a valid tick.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = 0;
        assert_eq!(adjtimex(&mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        let mut tx2 = Timex::zeroed();
        tx2.modes = ADJ_TICK;
        tx2.tick = 10_000;
        let ret = adjtimex(&mut tx2);
        assert_ne!(ret, -1);
        // errno preserved on success (POSIX).
        assert_eq!(errno::get_errno(), 0);
    }

    // ---- Sentinel ----

    #[test]
    fn test_adjtimex_tick_out_of_range_no_longer_silently_accepted_phase161() {
        // Sentinel: pre-Phase-161, `adjtimex` with `ADJ_TICK` and
        // `tick = 0` returned a TIME_* code (success) and silently
        // mutated the discipline state.  Post-fix it must return -1
        // with EINVAL.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = 0;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1, "post-Phase-161: bad tick must fail");
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "post-Phase-161: bad tick yields EINVAL"
        );
    }

    // ---- Cross-checks ----

    #[test]
    fn test_clock_adjtime_phase161_bad_tick_forwards_einval() {
        // clock_adjtime is a thin wrapper around adjtimex; the tick
        // validation must apply equally.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = 1; // way out of range
        let ret = clock_adjtime(0, &mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ntp_adjtime_phase161_bad_tick_forwards_einval() {
        // ntp_adjtime is an alias for adjtimex — same check applies.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = MAX_TICK + 1;
        let ret = ntp_adjtime(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase161_constants_match_linux() {
        // USER_HZ=100 on x86_64 glibc; bounds are derived.
        assert_eq!(USER_HZ, 100);
        assert_eq!(MIN_TICK, 9_000);
        assert_eq!(MAX_TICK, 11_000);
    }

    // ------------------------------------------------------------------
    // Phase 162 — ADJ_SETOFFSET sub-second range (Linux ABI parity)
    // ------------------------------------------------------------------
    //
    // Linux's `ntp_validate_timex` rejects an `ADJ_SETOFFSET` whose
    // sub-second field (`time.tv_usec`, reinterpreted as nanoseconds
    // when `ADJ_NANO` is co-set) is negative or `>=` one second:
    //
    //     if (tv->tv_usec >= USEC_PER_SEC || tv->tv_usec < 0)
    //         return false;
    //
    // For the ADJ_NANO variant the bound is NSEC_PER_SEC = 1e9.
    // Pre-fix we silently accepted out-of-range values; even though
    // we don't actually apply the offset (no RTC yet), the EINVAL is
    // still observable behaviour that NTP clients depend on.

    // ---- Per-error-class: usec mode ----

    #[test]
    fn test_adjtimex_phase162_setoffset_usec_negative_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = -1;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase162_setoffset_usec_equal_one_second_einval() {
        // The check is `>=`, so exactly USEC_PER_SEC must fail.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = USEC_PER_SEC;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase162_setoffset_usec_way_above_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = i64::MAX;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase162_setoffset_usec_i64_min_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = i64::MIN;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Per-error-class: nsec mode (ADJ_SETOFFSET | ADJ_NANO) ----

    #[test]
    fn test_adjtimex_phase162_setoffset_nano_at_usec_limit_accepted() {
        // 999_999 is a perfectly valid nsec value (well below 1e9) —
        // confirms the nano variant uses NSEC_PER_SEC, not
        // USEC_PER_SEC.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET | ADJ_NANO;
        tx.time_tv_usec = 999_999; // nsec — valid
        let ret = adjtimex(&mut tx);
        assert_ne!(ret, -1, "999_999 nsec must be accepted under ADJ_NANO");
    }

    #[test]
    fn test_adjtimex_phase162_setoffset_nano_equal_one_second_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET | ADJ_NANO;
        tx.time_tv_usec = NSEC_PER_SEC; // exactly 1e9 ns → EINVAL
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase162_setoffset_nano_negative_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET | ADJ_NANO;
        tx.time_tv_usec = -1;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Boundary acceptance ----

    #[test]
    fn test_adjtimex_phase162_setoffset_usec_zero_accepted() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = 0;
        let ret = adjtimex(&mut tx);
        assert_ne!(ret, -1);
    }

    #[test]
    fn test_adjtimex_phase162_setoffset_usec_max_accepted() {
        // USEC_PER_SEC - 1 = 999_999 — the largest valid microsecond.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = USEC_PER_SEC - 1;
        let ret = adjtimex(&mut tx);
        assert_ne!(ret, -1);
    }

    #[test]
    fn test_adjtimex_phase162_setoffset_nano_max_accepted() {
        // NSEC_PER_SEC - 1 = 999_999_999 — the largest valid nanosecond.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET | ADJ_NANO;
        tx.time_tv_usec = NSEC_PER_SEC - 1;
        let ret = adjtimex(&mut tx);
        assert_ne!(ret, -1);
    }

    // ---- Ordering matrix ----

    #[test]
    fn test_adjtimex_phase162_null_ptr_beats_setoffset_einval() {
        // EFAULT (null ptr) fires before SETOFFSET validation.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let ret = adjtimex(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_adjtimex_phase162_unknown_mode_beats_setoffset_einval() {
        // Unknown mode bit short-circuits before SETOFFSET check.
        // Both yield EINVAL but the precedence matters because the
        // unknown-mode branch doesn't read `time_tv_usec` at all.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET | 0x8000_0000;
        tx.time_tv_usec = i64::MAX;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase162_bad_tick_beats_bad_setoffset_einval() {
        // ADJ_TICK check fires before ADJ_SETOFFSET check (our code
        // order matches Linux's: tick first, then setoffset).  Both
        // are EINVAL; this pins the ordering for future readers.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK | ADJ_SETOFFSET;
        tx.tick = 0;
        tx.time_tv_usec = -1;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // State untouched on rejection.
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(tx2.tick, 10_000);
    }

    // ---- No-side-effect ----

    #[test]
    fn test_adjtimex_phase162_bad_setoffset_leaves_state_untouched() {
        // Combined ADJ_OFFSET | ADJ_SETOFFSET with a bad sub-second
        // must abort before the ADJ_OFFSET part is applied.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_OFFSET | ADJ_SETOFFSET;
        tx.offset = 777_777;
        tx.time_tv_usec = USEC_PER_SEC; // invalid
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(
            tx2.offset, 0,
            "offset must NOT be applied on SETOFFSET failure"
        );
    }

    // ---- Workflow ----

    #[test]
    fn test_adjtimex_phase162_chrony_clock_step_workflow() {
        // chrony's clock_step() injects a small offset, typically a
        // few milliseconds.  500_000 usec = 0.5s — well within the
        // valid range.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_sec = 0;
        tx.time_tv_usec = 500_000;
        let ret = adjtimex(&mut tx);
        assert_ne!(ret, -1, "valid chrony clock_step must succeed");
    }

    #[test]
    fn test_adjtimex_phase162_buggy_caller_unnormalised_usec() {
        // C code: `tx.time_tv_sec = 0; tx.time_tv_usec = 1_500_000;`
        // — a caller that "forgot" to normalise 1.5s into 1s + 0.5s.
        // Pre-fix: silently accepted (and we'd have dropped it on
        // the floor).  Post-fix: EINVAL exposes the bug.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = 1_500_000;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Recovery ----

    #[test]
    fn test_adjtimex_phase162_recovery_after_bad_setoffset() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = USEC_PER_SEC;
        assert_eq!(adjtimex(&mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        let mut tx2 = Timex::zeroed();
        tx2.modes = ADJ_SETOFFSET;
        tx2.time_tv_usec = 100_000;
        let ret = adjtimex(&mut tx2);
        assert_ne!(ret, -1);
        assert_eq!(errno::get_errno(), 0);
    }

    // ---- Sentinel ----

    #[test]
    fn test_adjtimex_setoffset_out_of_range_no_longer_silently_accepted_phase162() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = USEC_PER_SEC;
        let ret = adjtimex(&mut tx);
        assert_eq!(ret, -1, "post-Phase-162: out-of-range sub-second must fail");
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "post-Phase-162: yields EINVAL"
        );
    }

    // ---- Cross-checks ----

    #[test]
    fn test_clock_adjtime_phase162_bad_setoffset_forwards_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET;
        tx.time_tv_usec = -42;
        let ret = clock_adjtime(0, &mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_ntp_adjtime_phase162_bad_setoffset_nano_forwards_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_SETOFFSET | ADJ_NANO;
        tx.time_tv_usec = NSEC_PER_SEC + 1;
        let ret = ntp_adjtime(&mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_adjtimex_phase162_sub_second_constants_match_linux() {
        assert_eq!(USEC_PER_SEC, 1_000_000);
        assert_eq!(NSEC_PER_SEC, 1_000_000_000);
    }

    // ------------------------------------------------------------------
    // Phase 163 — clock_adjtime per-clock dispatch (Linux ABI parity)
    // ------------------------------------------------------------------
    //
    // Linux's `do_clock_adjtime` walks a per-clock `k_clock` table:
    // unknown id → EINVAL, known id without `clock_adj` → EOPNOTSUPP,
    // known id with `clock_adj` → forward.  Only CLOCK_REALTIME and
    // CLOCK_TAI implement `clock_adj`.  Additionally, the syscall
    // entry runs `copy_from_user` *before* `do_clock_adjtime`, so a
    // null `tx` returns EFAULT even when the clock id is also bogus.
    //
    // Pre-Phase-163:
    //   * `clk_id == 1` (CLOCK_MONOTONIC) forwarded to adjtimex —
    //     wrong; Linux returns EOPNOTSUPP.
    //   * Any other id returned EINVAL — wrong for the
    //     known-but-unsupported clocks (BOOTTIME, CPU-time, COARSE,
    //     ALARM variants), which Linux gives EOPNOTSUPP.
    //   * `clk_id == 11` (CLOCK_TAI) returned EINVAL — wrong; Linux
    //     supports adjtime on TAI.
    //   * Ordering: `clock_adjtime(99, NULL)` returned EINVAL — wrong;
    //     Linux returns EFAULT because copy_from_user fires first.

    // ---- Per-error-class: EOPNOTSUPP on known-but-unsupported ----

    #[test]
    fn test_clock_adjtime_phase163_monotonic_eopnotsupp() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(1, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_clock_adjtime_phase163_process_cputime_eopnotsupp() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(2, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_clock_adjtime_phase163_thread_cputime_eopnotsupp() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(3, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_clock_adjtime_phase163_monotonic_raw_eopnotsupp() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(4, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_clock_adjtime_phase163_realtime_coarse_eopnotsupp() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(5, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_clock_adjtime_phase163_monotonic_coarse_eopnotsupp() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(6, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_clock_adjtime_phase163_boottime_eopnotsupp() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(7, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_clock_adjtime_phase163_realtime_alarm_eopnotsupp() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(8, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    #[test]
    fn test_clock_adjtime_phase163_boottime_alarm_eopnotsupp() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(9, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    // ---- CLOCK_TAI accepted ----

    #[test]
    fn test_clock_adjtime_phase163_tai_forwards_to_adjtimex() {
        // CLOCK_TAI (id 11) is one of only two clocks whose
        // `clock_adj` callback Linux populates.  Pre-fix we returned
        // EINVAL for id=11; post-fix it must succeed.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        let ret = clock_adjtime(11, &mut tx);
        // Boot default has STA_UNSYNC → TIME_ERROR (not -1).
        assert_eq!(ret, TIME_ERROR);
    }

    // ---- Unknown clock id → EINVAL ----

    #[test]
    fn test_clock_adjtime_phase163_unknown_id_10_einval() {
        // id 10 (was CLOCK_SGI_CYCLE, removed) is no longer
        // recognised — Linux's k_clock table has no entry → EINVAL.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(10, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_adjtime_phase163_unknown_id_99_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(99, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_adjtime_phase163_negative_id_einval() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(-1, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ---- Ordering matrix: EFAULT beats clock-id check ----

    #[test]
    fn test_clock_adjtime_phase163_null_tx_unknown_id_efault() {
        // Bad clock + null tx → EFAULT (copy_from_user runs first
        // in Linux's SYSCALL_DEFINE2).  Pre-fix this would have been
        // EINVAL because we checked clock id before tx.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let ret = clock_adjtime(99, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_clock_adjtime_phase163_null_tx_unsupported_id_efault() {
        // Known-but-unsupported clock + null tx → still EFAULT.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let ret = clock_adjtime(7, core::ptr::null_mut()); // BOOTTIME
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_clock_adjtime_phase163_null_tx_realtime_efault() {
        // Sanity: even on the fully-supported CLOCK_REALTIME the null
        // tx returns EFAULT (this was already passing via adjtimex'
        // own check, but the new structure must not have regressed
        // it).
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let ret = clock_adjtime(0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // ---- Workflow ----

    #[test]
    fn test_clock_adjtime_phase163_chrony_capability_probe_workflow() {
        // chrony probes for adjustable clocks at startup: it tries
        // CLOCK_REALTIME, CLOCK_TAI, then falls back if either
        // returns EOPNOTSUPP.  Pre-fix we returned EINVAL on TAI,
        // confusing the probe.  Post-fix both succeed.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        // REALTIME — supported.
        let mut tx = Timex::zeroed();
        assert_ne!(clock_adjtime(0, &mut tx), -1, "REALTIME must be adjustable");
        // TAI — supported.
        let mut tx2 = Timex::zeroed();
        assert_ne!(clock_adjtime(11, &mut tx2), -1, "TAI must be adjustable");
    }

    #[test]
    fn test_clock_adjtime_phase163_systemd_timesyncd_workflow() {
        // systemd-timesyncd checks clock_adjtime(CLOCK_BOOTTIME) at
        // init to decide if it should skip the suspend-resume
        // correction step.  Pre-fix: EINVAL (confusing).  Post-fix:
        // EOPNOTSUPP (the documented "this clock isn't adjustable"
        // signal).
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        let ret = clock_adjtime(7, &mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
    }

    // ---- Recovery ----

    #[test]
    fn test_clock_adjtime_phase163_recovery_after_eopnotsupp() {
        // EOPNOTSUPP from one clock doesn't poison a follow-up call
        // on a supported clock.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(1, &mut tx), -1);
        assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);

        errno::set_errno(0);
        let mut tx2 = Timex::zeroed();
        let ret = clock_adjtime(0, &mut tx2);
        assert_ne!(ret, -1);
        assert_eq!(errno::get_errno(), 0);
    }

    // ---- No-side-effect: rejection must not touch state ----

    #[test]
    fn test_clock_adjtime_phase163_eopnotsupp_leaves_state_untouched() {
        // Even a rejected MONOTONIC call must not mutate the shared
        // discipline state — important if a caller probes the clock
        // before setting up real ntpd discipline.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_OFFSET;
        tx.offset = 555_555;
        let _ = clock_adjtime(1, &mut tx); // EOPNOTSUPP
        // State must still be at boot defaults.
        let mut tx2 = Timex::zeroed();
        let _ = adjtimex(&mut tx2);
        assert_eq!(
            tx2.offset, 0,
            "EOPNOTSUPP rejection must not apply ADJ_OFFSET"
        );
    }

    // ---- Sentinel ----

    #[test]
    fn test_clock_adjtime_monotonic_no_longer_silently_forwards_phase163() {
        // Sentinel: pre-Phase-163, clock_adjtime(CLOCK_MONOTONIC,
        // ...) silently forwarded to adjtimex and returned a TIME_*
        // code — a divergence from Linux that hid the "this clock
        // isn't adjustable" condition.  Post-fix it must return -1
        // with EOPNOTSUPP.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        let ret = clock_adjtime(1, &mut tx);
        assert_eq!(ret, -1, "post-Phase-163: MONOTONIC must fail");
        assert_eq!(
            errno::get_errno(),
            errno::EOPNOTSUPP,
            "post-Phase-163: EOPNOTSUPP, not TIME_ERROR or EINVAL"
        );
    }

    #[test]
    fn test_clock_adjtime_tai_no_longer_einval_phase163() {
        // Sentinel: pre-Phase-163, CLOCK_TAI was wrongly rejected
        // with EINVAL.  Post-fix it forwards to adjtimex.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        let ret = clock_adjtime(11, &mut tx);
        // Returns a TIME_* code (TIME_ERROR by default), NOT -1.
        assert_ne!(ret, -1, "post-Phase-163: TAI must succeed");
        // errno must be preserved (POSIX success contract).
        assert_eq!(errno::get_errno(), 0);
    }

    // ---- Cross-checks ----

    #[test]
    fn test_clock_adjtime_phase163_tai_propagates_tick_validation() {
        // The Phase-161 tick validation applies via TAI too — proves
        // TAI really does forward to the same code path.
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        errno::set_errno(0);
        let mut tx = Timex::zeroed();
        tx.modes = ADJ_TICK;
        tx.tick = 0;
        let ret = clock_adjtime(11, &mut tx);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_clock_adjtime_phase163_eopnotsupp_value_matches_enotsup() {
        // Linux uses ENOTSUP and EOPNOTSUPP interchangeably (both =
        // 95).  Our errno table mirrors this.
        assert_eq!(errno::EOPNOTSUPP, errno::ENOTSUP);
    }

    // ======================================================================
    // Phase 173 — adjtimex CAP_SYS_TIME gate
    //
    // Linux `kernel/time/timekeeping.c::do_adjtimex` returns -EPERM when
    // the caller requests *any* modification (`txc->modes != 0`) without
    // CAP_SYS_TIME.  A read-only query (`modes == 0`) is unprivileged.
    // The check sits after `ntp_validate_timex` so EINVAL beats EPERM
    // for bad mode bits / out-of-range tick / out-of-range setoffset.
    //
    // ntp_adjtime forwards to adjtimex and inherits the gate.
    // clock_adjtime forwards to adjtimex only for CLOCK_REALTIME and
    // CLOCK_TAI; other clock ids return EOPNOTSUPP/EINVAL before any
    // cap check (matches Linux's `do_clock_adjtime`).
    // ======================================================================

    mod adjtimex_cap_phase173 {
        use super::*;

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

        // -- Per-error-class --------------------------------------------

        /// ADJ_OFFSET write without CAP_SYS_TIME → EPERM.
        #[test]
        fn test_adjtimex_phase173_offset_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_OFFSET;
            tx.offset = 1234;
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// ADJ_FREQUENCY write without CAP_SYS_TIME → EPERM.
        #[test]
        fn test_adjtimex_phase173_frequency_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_FREQUENCY;
            tx.freq = 100;
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// ADJ_STATUS write without CAP_SYS_TIME → EPERM.
        #[test]
        fn test_adjtimex_phase173_status_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_STATUS;
            tx.status = STA_PLL;
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// ADJ_TICK write (with valid tick) without CAP_SYS_TIME → EPERM.
        #[test]
        fn test_adjtimex_phase173_tick_valid_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_TICK;
            // Valid mid-range tick — passes the EINVAL guard so the
            // cap probe is the next thing.
            tx.tick = (MIN_TICK + MAX_TICK) / 2;
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// Combined-modes write without CAP_SYS_TIME → EPERM.
        #[test]
        fn test_adjtimex_phase173_combined_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_OFFSET | ADJ_FREQUENCY | ADJ_STATUS;
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- Read-only query is unprivileged ----------------------------

        /// modes == 0 is a read-only query — no cap required, returns
        /// a TIME_* status code.
        #[test]
        fn test_adjtimex_phase173_readonly_no_cap_succeeds() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = 0;
            errno::set_errno(0);
            let rc = adjtimex(&mut tx);
            // Read-only queries return a TIME_* code (non-negative);
            // errno must not be touched (success path).
            assert!(
                rc >= 0,
                "modes==0 should succeed without CAP_SYS_TIME, got rc={rc}"
            );
        }

        // -- Ordering matrix (EFAULT/EINVAL beat EPERM) ------------------

        /// NULL tx → EFAULT even without cap (pointer check first).
        #[test]
        fn test_adjtimex_phase173_null_tx_efault_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            errno::set_errno(0);
            assert_eq!(adjtimex(core::ptr::null_mut()), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }

        /// Unknown mode bits → EINVAL even without cap (ntp_validate_timex
        /// runs before the cap probe).
        #[test]
        fn test_adjtimex_phase173_bad_modes_einval_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            // 0x8000_0000 is not a known mode bit.
            tx.modes = 0x8000_0000;
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// ADJ_TICK with out-of-range tick → EINVAL even without cap.
        #[test]
        fn test_adjtimex_phase173_bad_tick_einval_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_TICK;
            tx.tick = MAX_TICK + 1;
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// ADJ_SETOFFSET with out-of-range sub-second → EINVAL even
        /// without cap.
        #[test]
        fn test_adjtimex_phase173_bad_setoffset_einval_beats_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_SETOFFSET;
            // Default unit is microseconds; USEC_PER_SEC is out-of-range.
            tx.time_tv_usec = USEC_PER_SEC;
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- Workflow / recovery ----------------------------------------

        /// chrony-like probe: drop cap, attempt write → EPERM; flip to
        /// read-only → succeeds; restore cap → write succeeds.
        #[test]
        fn test_adjtimex_phase173_workflow_drop_probe_restore() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            // 1. Write attempt fails.
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_OFFSET;
            tx.offset = 1;
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
            // 2. Read-only succeeds.
            let mut probe = Timex::zeroed();
            probe.modes = 0;
            errno::set_errno(0);
            assert!(adjtimex(&mut probe) >= 0);
            // 3. Restore CAP_SYS_TIME and write succeeds.
            use crate::sys_capability::CAP_SYS_TIME;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_TIME < 32 {
                (lo | (1u32 << CAP_SYS_TIME), hi)
            } else {
                (lo, hi | (1u32 << (CAP_SYS_TIME - 32)))
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
            assert_eq!(rc, 0);
            assert!(crate::sys_capability::has_capability(CAP_SYS_TIME));
            tx.modes = ADJ_OFFSET;
            tx.offset = 42;
            errno::set_errno(0);
            assert!(adjtimex(&mut tx) >= 0);
        }

        // -- No-side-effect on EPERM ------------------------------------

        /// EPERM must leave the NTP discipline state untouched — a
        /// subsequent read-only query under restored caps must not
        /// observe the rejected write.
        #[test]
        fn test_adjtimex_phase173_eperm_no_side_effect_on_state() {
            let _g = CapGuard::snapshot();
            // First read the current offset under default caps.
            let mut probe = Timex::zeroed();
            probe.modes = 0;
            assert!(adjtimex(&mut probe) >= 0);
            let baseline_offset = probe.offset;

            // Drop cap and try to write a known-distinctive offset.
            drop_cap_sys_time();
            let mut bad = Timex::zeroed();
            bad.modes = ADJ_OFFSET;
            bad.offset = baseline_offset.wrapping_add(0x5BAD_C0DE);
            errno::set_errno(0);
            assert_eq!(adjtimex(&mut bad), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);

            // Read back under restored caps via the outer guard's drop
            // path — but we want to assert *before* the guard drops, so
            // manually restore CAP_SYS_TIME and read again.
            use crate::sys_capability::CAP_SYS_TIME;
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if CAP_SYS_TIME < 32 {
                (lo | (1u32 << CAP_SYS_TIME), hi)
            } else {
                (lo, hi | (1u32 << (CAP_SYS_TIME - 32)))
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
            assert_eq!(crate::sys_capability::capset(&mut hdr, data.as_ptr()), 0,);
            let mut after = Timex::zeroed();
            after.modes = 0;
            assert!(adjtimex(&mut after) >= 0);
            assert_eq!(
                after.offset, baseline_offset,
                "EPERM must not change state.offset"
            );
        }

        // -- ntp_adjtime forwards the gate ------------------------------

        /// ntp_adjtime is an alias for adjtimex — inherits the gate.
        #[test]
        fn test_ntp_adjtime_phase173_write_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_OFFSET;
            errno::set_errno(0);
            assert_eq!(ntp_adjtime(&mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        // -- clock_adjtime forwarding (REALTIME/TAI inherit the gate) ----

        /// clock_adjtime(CLOCK_REALTIME, write) without cap → EPERM via
        /// the adjtimex forward.
        #[test]
        fn test_clock_adjtime_phase173_realtime_write_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_OFFSET;
            errno::set_errno(0);
            assert_eq!(clock_adjtime(0 /* CLOCK_REALTIME */, &mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// clock_adjtime(CLOCK_TAI, write) without cap → EPERM.
        #[test]
        fn test_clock_adjtime_phase173_tai_write_no_cap_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_OFFSET;
            errno::set_errno(0);
            assert_eq!(clock_adjtime(11 /* CLOCK_TAI */, &mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }

        /// clock_adjtime on a non-adjustable clock returns EOPNOTSUPP
        /// without needing any cap — the dispatch table rejects before
        /// the cap probe.
        #[test]
        fn test_clock_adjtime_phase173_monotonic_no_cap_eopnotsupp() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_OFFSET;
            errno::set_errno(0);
            assert_eq!(clock_adjtime(1 /* CLOCK_MONOTONIC */, &mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EOPNOTSUPP);
        }

        /// clock_adjtime with unknown clock id → EINVAL even without cap.
        #[test]
        fn test_clock_adjtime_phase173_bad_clock_no_cap_einval() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_OFFSET;
            errno::set_errno(0);
            assert_eq!(clock_adjtime(99, &mut tx), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- Sentinel: cap-held privileged path still works -------------

        /// With CAP_SYS_TIME held (default), every write reaches the
        /// state-application path and returns a TIME_* code.
        #[test]
        fn test_adjtimex_phase173_sentinel_cap_held_write_succeeds() {
            let _g = CapGuard::snapshot();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_TIME,
            ));
            let mut tx = Timex::zeroed();
            tx.modes = ADJ_OFFSET | ADJ_FREQUENCY;
            tx.offset = 100;
            tx.freq = 200;
            errno::set_errno(0);
            assert!(adjtimex(&mut tx) >= 0, "cap-held write should succeed");
        }

        // -- Cross-check: dropping CAP_SYS_TIME isolates other caps ----

        /// Dropping CAP_SYS_TIME must not disturb other caps.
        #[test]
        fn test_adjtimex_phase173_drop_isolates_other_caps() {
            let _g = CapGuard::snapshot();
            drop_cap_sys_time();
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_NICE,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_IPC_LOCK,
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYSLOG,
            ));
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_TIME,
            ));
        }
    }
}

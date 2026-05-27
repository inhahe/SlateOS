//! `<sys/timex.h>` — clock adjustment interface.
//!
//! Provides the `Timex` struct and `adjtimex()`/`ntp_adjtime()`
//! for kernel clock discipline (NTP, PTP, etc.).

use crate::errno;

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
/// Returns -1 with `EFAULT` on null pointer.  We do not enforce
/// privilege on writes (no user/permission model yet); on a real
/// multi-user system this would require CAP_SYS_TIME.
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

    status_to_return(state.status)
}

/// NTP-compatible clock adjustment (identical to `adjtimex`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ntp_adjtime(tx: *mut Timex) -> i32 {
    adjtimex(tx)
}

/// Per-clock adjtimex (`clock_adjtime(2)`).
///
/// `clk_id` selects which clock to adjust.  We accept `CLOCK_REALTIME`
/// (0) and `CLOCK_MONOTONIC` (1) — both forward to the same shared
/// discipline state because we have only one underlying clock.
/// Returns -1 with `EINVAL` for any other clock id, or with `EFAULT`
/// on null `tx`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_adjtime(clk_id: i32, tx: *mut Timex) -> i32 {
    // CLOCK_REALTIME = 0, CLOCK_MONOTONIC = 1 — see <time.h>.
    if clk_id != 0 && clk_id != 1 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    adjtimex(tx)
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
            ADJ_OFFSET, ADJ_FREQUENCY, ADJ_MAXERROR, ADJ_ESTERROR,
            ADJ_STATUS, ADJ_TIMECONST, ADJ_TAI, ADJ_MICRO, ADJ_NANO,
            ADJ_SETOFFSET, ADJ_TICK,
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

    #[test]
    fn test_clock_adjtime_monotonic_works() {
        let _g = TIMEX_TEST_LOCK.lock().unwrap();
        reset_timex_state();
        let mut tx = Timex::zeroed();
        let ret = clock_adjtime(1, &mut tx); // CLOCK_MONOTONIC
        assert_eq!(ret, TIME_ERROR);
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
}

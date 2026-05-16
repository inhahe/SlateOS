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
// Functions
// ---------------------------------------------------------------------------

/// Adjust the kernel clock.
///
/// Stub — returns `TIME_ERROR` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn adjtimex(_tx: *mut Timex) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// NTP-compatible clock adjustment (identical to `adjtimex`).
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ntp_adjtime(tx: *mut Timex) -> i32 {
    adjtimex(tx)
}

/// Adjust kernel clock (alias for `adjtimex`).
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clock_adjtime(_clk_id: i32, _tx: *mut Timex) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
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

    #[test]
    fn test_adjtimex_stub() {
        let mut tx = Timex::zeroed();
        assert_eq!(adjtimex(&mut tx), -1);
    }

    #[test]
    fn test_ntp_adjtime_stub() {
        let mut tx = Timex::zeroed();
        assert_eq!(ntp_adjtime(&mut tx), -1);
    }

    #[test]
    fn test_clock_adjtime_stub() {
        let mut tx = Timex::zeroed();
        assert_eq!(clock_adjtime(0, &mut tx), -1);
    }
}

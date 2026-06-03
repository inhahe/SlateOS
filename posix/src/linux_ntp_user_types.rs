//! `<linux/timex.h>` — NTP / `adjtimex(2)` ABI.
//!
//! `adjtimex` is the only documented way for `ntpd`, `chronyd`, and
//! `systemd-timesyncd` to slew the kernel clock. Userspace sets a
//! mode-mask of which `timex` fields are being supplied, the kernel
//! returns the resulting clock state. The constants below define
//! that mode-mask, the `STA_*` status word, and the discipline-state
//! return values.

// ---------------------------------------------------------------------------
// `adjtimex` mode bits (which `timex` fields are being set)
// ---------------------------------------------------------------------------

pub const ADJ_OFFSET: u32 = 0x0001;
pub const ADJ_FREQUENCY: u32 = 0x0002;
pub const ADJ_MAXERROR: u32 = 0x0004;
pub const ADJ_ESTERROR: u32 = 0x0008;
pub const ADJ_STATUS: u32 = 0x0010;
pub const ADJ_TIMECONST: u32 = 0x0020;
pub const ADJ_TAI: u32 = 0x0080;
pub const ADJ_SETOFFSET: u32 = 0x0100;
pub const ADJ_MICRO: u32 = 0x1000;
pub const ADJ_NANO: u32 = 0x2000;
pub const ADJ_TICK: u32 = 0x4000;
/// Old-style `adjtime(3)` calling convention.
pub const ADJ_OFFSET_SINGLESHOT: u32 = 0x8001;
pub const ADJ_OFFSET_SS_READ: u32 = 0xa001;

// ---------------------------------------------------------------------------
// `timex.status` flags (`STA_*`)
// ---------------------------------------------------------------------------

pub const STA_PLL: u32 = 0x0001;
pub const STA_PPSFREQ: u32 = 0x0002;
pub const STA_PPSTIME: u32 = 0x0004;
pub const STA_FLL: u32 = 0x0008;
pub const STA_INS: u32 = 0x0010;
pub const STA_DEL: u32 = 0x0020;
pub const STA_UNSYNC: u32 = 0x0040;
pub const STA_FREQHOLD: u32 = 0x0080;
pub const STA_PPSSIGNAL: u32 = 0x0100;
pub const STA_PPSJITTER: u32 = 0x0200;
pub const STA_PPSWANDER: u32 = 0x0400;
pub const STA_PPSERROR: u32 = 0x0800;
pub const STA_CLOCKERR: u32 = 0x1000;
pub const STA_NANO: u32 = 0x2000;
pub const STA_MODE: u32 = 0x4000;
pub const STA_CLK: u32 = 0x8000;

/// Read-only bits inside `STA_*` (the kernel maintains these, userspace
/// is not allowed to write them through `ADJ_STATUS`).
pub const STA_RONLY: u32 = STA_PPSSIGNAL
    | STA_PPSJITTER
    | STA_PPSWANDER
    | STA_PPSERROR
    | STA_CLOCKERR
    | STA_NANO
    | STA_MODE
    | STA_CLK;

// ---------------------------------------------------------------------------
// `adjtimex` return value — clock discipline state (`TIME_*`)
// ---------------------------------------------------------------------------

pub const TIME_OK: i32 = 0;
pub const TIME_INS: i32 = 1;
pub const TIME_DEL: i32 = 2;
pub const TIME_OOP: i32 = 3;
pub const TIME_WAIT: i32 = 4;
pub const TIME_ERROR: i32 = 5;
pub const TIME_BAD: i32 = TIME_ERROR;

// ---------------------------------------------------------------------------
// Tick-rate constants
// ---------------------------------------------------------------------------

/// `MAXPHASE` — maximum phase error in microseconds (500 ms).
pub const MAXPHASE: i64 = 500_000_000;
/// `MAXFREQ` — maximum frequency error in scaled ppm (500 ppm).
pub const MAXFREQ: i64 = 500_000;
/// Default time-constant for the kernel PLL (in log2 seconds).
pub const MINTC: i32 = 0;
pub const MAXTC: i32 = 10;

// ---------------------------------------------------------------------------
// Syscalls
// ---------------------------------------------------------------------------

pub const NR_ADJTIMEX: u32 = 159;
pub const NR_CLOCK_ADJTIME: u32 = 305;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adj_mode_low_bits_single_bit() {
        let m = [
            ADJ_OFFSET,
            ADJ_FREQUENCY,
            ADJ_MAXERROR,
            ADJ_ESTERROR,
            ADJ_STATUS,
            ADJ_TIMECONST,
            ADJ_TAI,
            ADJ_SETOFFSET,
            ADJ_MICRO,
            ADJ_NANO,
            ADJ_TICK,
        ];
        for v in m {
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_adj_singleshot_legacy_form() {
        // The two SINGLESHOT modes embed ADJ_OFFSET (0x0001) in the low bit.
        assert_eq!(ADJ_OFFSET_SINGLESHOT & ADJ_OFFSET, ADJ_OFFSET);
        assert_eq!(ADJ_OFFSET_SS_READ & ADJ_OFFSET, ADJ_OFFSET);
        assert_eq!(ADJ_OFFSET_SINGLESHOT, 0x8001);
        assert_eq!(ADJ_OFFSET_SS_READ, 0xa001);
    }

    #[test]
    fn test_sta_bits_dense_0_to_15() {
        // STA_PLL..STA_CLK occupy bits 0..15.
        let s = [
            STA_PLL,
            STA_PPSFREQ,
            STA_PPSTIME,
            STA_FLL,
            STA_INS,
            STA_DEL,
            STA_UNSYNC,
            STA_FREQHOLD,
            STA_PPSSIGNAL,
            STA_PPSJITTER,
            STA_PPSWANDER,
            STA_PPSERROR,
            STA_CLOCKERR,
            STA_NANO,
            STA_MODE,
            STA_CLK,
        ];
        let mut or = 0u32;
        for (i, &v) in s.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1 << i);
            or |= v;
        }
        assert_eq!(or, 0xFFFF);
    }

    #[test]
    fn test_sta_rdonly_is_top_byte_plus_pps() {
        // Read-only bits are the PPS-monitor and high-state bits the kernel
        // sets on its own.
        let expected =
            STA_PPSSIGNAL | STA_PPSJITTER | STA_PPSWANDER | STA_PPSERROR |
            STA_CLOCKERR | STA_NANO | STA_MODE | STA_CLK;
        assert_eq!(STA_RONLY, expected);
    }

    #[test]
    fn test_time_states_dense_0_to_5() {
        let t = [TIME_OK, TIME_INS, TIME_DEL, TIME_OOP, TIME_WAIT, TIME_ERROR];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // BAD is an alias for ERROR.
        assert_eq!(TIME_BAD, TIME_ERROR);
    }

    #[test]
    fn test_phase_freq_caps() {
        // 0.5 seconds expressed in nanoseconds.
        assert_eq!(MAXPHASE, 500_000_000);
        // 500 ppm in the kernel's scaled-ppm format.
        assert_eq!(MAXFREQ, 500_000);
        assert!(MINTC < MAXTC);
    }

    #[test]
    fn test_syscall_numbers() {
        assert_eq!(NR_ADJTIMEX, 159);
        assert_eq!(NR_CLOCK_ADJTIME, 305);
    }
}

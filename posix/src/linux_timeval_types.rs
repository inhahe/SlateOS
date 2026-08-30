//! `<sys/time.h>` — struct timeval and time-of-day constants.
//!
//! `gettimeofday()` and `settimeofday()` use `struct timeval`
//! for microsecond-precision wall-clock time.  These constants
//! define the structure layout, timezone handling, and
//! adjtime-related values.

// ---------------------------------------------------------------------------
// struct timeval field offsets (same as in itimer2 but standalone)
// ---------------------------------------------------------------------------

/// Offset of tv_sec in struct timeval.
pub const TV_OFF_SEC: u32 = 0;
/// Offset of tv_usec in struct timeval.
pub const TV_OFF_USEC: u32 = 8;
/// Size of struct timeval on x86_64 (bytes).
pub const TV_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// struct timezone field offsets
// ---------------------------------------------------------------------------

/// Offset of tz_minuteswest in struct timezone.
pub const TZ_OFF_MINUTESWEST: u32 = 0;
/// Offset of tz_dsttime in struct timezone.
pub const TZ_OFF_DSTTIME: u32 = 4;
/// Size of struct timezone (bytes).
pub const TZ_SIZE: u32 = 8;

// ---------------------------------------------------------------------------
// adjtime / adjtimex constants
// ---------------------------------------------------------------------------

/// ADJ_OFFSET: adjust time offset.
pub const ADJ_OFFSET: u32 = 0x0001;
/// ADJ_FREQUENCY: adjust frequency.
pub const ADJ_FREQUENCY: u32 = 0x0002;
/// ADJ_MAXERROR: adjust maximum error.
pub const ADJ_MAXERROR: u32 = 0x0004;
/// ADJ_ESTERROR: adjust estimated error.
pub const ADJ_ESTERROR: u32 = 0x0008;
/// ADJ_STATUS: adjust clock status.
pub const ADJ_STATUS: u32 = 0x0010;
/// ADJ_TIMECONST: adjust PLL time constant.
pub const ADJ_TIMECONST: u32 = 0x0020;
/// ADJ_TAI: adjust TAI offset.
pub const ADJ_TAI: u32 = 0x0080;
/// ADJ_SETOFFSET: set time offset (add to current).
pub const ADJ_SETOFFSET: u32 = 0x0100;
/// ADJ_MICRO: select microsecond resolution.
pub const ADJ_MICRO: u32 = 0x1000;
/// ADJ_NANO: select nanosecond resolution.
pub const ADJ_NANO: u32 = 0x2000;
/// ADJ_TICK: adjust tick length.
pub const ADJ_TICK: u32 = 0x4000;

// ---------------------------------------------------------------------------
// adjtimex status bits
// ---------------------------------------------------------------------------

/// Clock is synchronized.
pub const STA_PLL: u32 = 0x0001;
/// Insert leap second.
pub const STA_INS: u32 = 0x0010;
/// Delete leap second.
pub const STA_DEL: u32 = 0x0020;
/// Clock is unsynchronized.
pub const STA_UNSYNC: u32 = 0x0040;
/// Nano-second mode.
pub const STA_NANO: u32 = 0x2000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeval_layout() {
        assert_eq!(TV_OFF_SEC, 0);
        assert_eq!(TV_OFF_USEC, 8);
        assert_eq!(TV_SIZE, 16);
    }

    #[test]
    fn test_timezone_layout() {
        assert_eq!(TZ_OFF_MINUTESWEST, 0);
        assert_eq!(TZ_OFF_DSTTIME, 4);
        assert_eq!(TZ_SIZE, 8);
    }

    #[test]
    fn test_adj_flags_no_overlap() {
        let flags = [
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
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_adj_offset_is_one() {
        assert_eq!(ADJ_OFFSET, 1);
    }

    #[test]
    fn test_sta_bits_distinct() {
        let bits = [STA_PLL, STA_INS, STA_DEL, STA_UNSYNC, STA_NANO];
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_sta_pll_is_one() {
        assert_eq!(STA_PLL, 1);
    }
}

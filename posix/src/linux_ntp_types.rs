//! `<linux/timex.h>` — NTP (Network Time Protocol) synchronization constants.
//!
//! The kernel NTP subsystem disciplines the system clock using
//! adjtimex()/clock_adjtime(). NTP daemons (ntpd, chrony, systemd-
//! timesyncd) measure time offsets from reference servers and feed
//! corrections to the kernel, which slews or steps the clock. The
//! kernel maintains phase-locked loop (PLL) or frequency-locked loop
//! (FLL) state to keep the clock accurate between updates.

// ---------------------------------------------------------------------------
// adjtimex() mode flags (which fields to set)
// ---------------------------------------------------------------------------

/// Set time offset.
pub const ADJ_OFFSET: u32 = 0x0001;
/// Set frequency offset.
pub const ADJ_FREQUENCY: u32 = 0x0002;
/// Set maximum error.
pub const ADJ_MAXERROR: u32 = 0x0004;
/// Set estimated error.
pub const ADJ_ESTERROR: u32 = 0x0008;
/// Set clock status bits.
pub const ADJ_STATUS: u32 = 0x0010;
/// Set PLL time constant.
pub const ADJ_TIMECONST: u32 = 0x0020;
/// Set TAI offset.
pub const ADJ_TAI: u32 = 0x0080;
/// Set tick value.
pub const ADJ_TICK: u32 = 0x4000;
/// Set offset via nanoseconds (not microseconds).
pub const ADJ_NANO: u32 = 0x2000;
/// Select microsecond resolution (default).
pub const ADJ_MICRO: u32 = 0x1000;

// ---------------------------------------------------------------------------
// adjtimex() status flags
// ---------------------------------------------------------------------------

/// PLL mode (phase-locked loop, continuous correction).
pub const STA_PLL: u32 = 0x0001;
/// PPS (pulse-per-second) signal detected.
pub const STA_PPSFREQ: u32 = 0x0002;
/// PPS time discipline active.
pub const STA_PPSTIME: u32 = 0x0004;
/// FLL mode (frequency-locked loop, large corrections).
pub const STA_FLL: u32 = 0x0008;
/// Insert leap second at end of day.
pub const STA_INS: u32 = 0x0010;
/// Delete leap second at end of day.
pub const STA_DEL: u32 = 0x0020;
/// Clock is unsynchronized.
pub const STA_UNSYNC: u32 = 0x0040;
/// Hold frequency (don't adjust).
pub const STA_FREQHOLD: u32 = 0x0080;
/// PPS signal jitter exceeded.
pub const STA_PPSJITTER: u32 = 0x0200;
/// PPS signal stability exceeded.
pub const STA_PPSSTAB: u32 = 0x0400;
/// PPS signal error (calibration error).
pub const STA_PPSERROR: u32 = 0x0800;
/// Clock error (hardware problem).
pub const STA_CLOCKERR: u32 = 0x1000;
/// Use nanosecond resolution.
pub const STA_NANO: u32 = 0x2000;

// ---------------------------------------------------------------------------
// adjtimex() return values (clock state)
// ---------------------------------------------------------------------------

/// Clock is synchronized.
pub const TIME_OK: u32 = 0;
/// Leap second insert pending.
pub const TIME_INS: u32 = 1;
/// Leap second delete pending.
pub const TIME_DEL: u32 = 2;
/// Leap second in progress.
pub const TIME_OOP: u32 = 3;
/// Leap second occurred (informational).
pub const TIME_WAIT: u32 = 4;
/// Clock not synchronized.
pub const TIME_ERROR: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adj_mode_flags_no_overlap() {
        let flags = [
            ADJ_OFFSET, ADJ_FREQUENCY, ADJ_MAXERROR, ADJ_ESTERROR,
            ADJ_STATUS, ADJ_TIMECONST, ADJ_TAI, ADJ_TICK,
            ADJ_NANO, ADJ_MICRO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_status_flags_selected_no_overlap() {
        let flags = [STA_PLL, STA_PPSFREQ, STA_PPSTIME, STA_FLL, STA_INS, STA_DEL];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_time_states_distinct() {
        let states = [TIME_OK, TIME_INS, TIME_DEL, TIME_OOP, TIME_WAIT, TIME_ERROR];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}

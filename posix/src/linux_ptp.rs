//! `<linux/ptp_clock.h>` — Precision Time Protocol clock constants.
//!
//! PTP (IEEE 1588) provides sub-microsecond time synchronization
//! over Ethernet. The Linux PTP subsystem exposes PTP hardware
//! clocks via /dev/ptpN with ioctls for time read/set and alarms.

// ---------------------------------------------------------------------------
// PTP clock capabilities
// ---------------------------------------------------------------------------

/// Maximum number of alarms.
pub const PTP_MAX_ALARMS: usize = 4;
/// Maximum number of external timestamp channels.
pub const PTP_MAX_EXT_TS: usize = 8;
/// Maximum number of periodic output signals.
pub const PTP_MAX_PER_OUT: usize = 4;

// ---------------------------------------------------------------------------
// PTP ioctl commands
// ---------------------------------------------------------------------------

/// Get clock capabilities.
pub const PTP_CLOCK_GETCAPS: u32 = 0x8090_3D01;
/// Request external timestamp.
pub const PTP_EXTTS_REQUEST: u32 = 0x4010_3D02;
/// Configure periodic output.
pub const PTP_PEROUT_REQUEST: u32 = 0x4038_3D03;
/// Enable PPS (pulse per second).
pub const PTP_ENABLE_PPS: u32 = 0x4004_3D04;
/// Get system offset (PTP-SYS).
pub const PTP_SYS_OFFSET: u32 = 0x8168_3D05;
/// Pin function get/set.
pub const PTP_PIN_GETFUNC: u32 = 0xC060_3D06;
/// Pin function set.
pub const PTP_PIN_SETFUNC: u32 = 0x4060_3D07;
/// Precise system offset.
pub const PTP_SYS_OFFSET_PRECISE: u32 = 0x8030_3D08;
/// Extended system offset.
pub const PTP_SYS_OFFSET_EXTENDED: u32 = 0xC4C0_3D09;

// ---------------------------------------------------------------------------
// PTP external timestamp flags
// ---------------------------------------------------------------------------

/// Enable external timestamp.
pub const PTP_ENABLE_FEATURE: u32 = 1 << 0;
/// Rising edge trigger.
pub const PTP_RISING_EDGE: u32 = 1 << 1;
/// Falling edge trigger.
pub const PTP_FALLING_EDGE: u32 = 1 << 2;
/// Strict mode (fail if cannot achieve exact period).
pub const PTP_STRICT_FLAGS: u32 = 1 << 3;
/// Extended timestamp (nanosecond resolution).
pub const PTP_EXT_OFFSET: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// PTP pin function types
// ---------------------------------------------------------------------------

/// Pin unused.
pub const PTP_PF_NONE: u32 = 0;
/// External timestamp input.
pub const PTP_PF_EXTTS: u32 = 1;
/// Periodic output.
pub const PTP_PF_PEROUT: u32 = 2;
/// Physical layer signaling.
pub const PTP_PF_PHYSYNC: u32 = 3;

// ---------------------------------------------------------------------------
// PTP clock index constants
// ---------------------------------------------------------------------------

/// Invalid clock index.
pub const PTP_CLOCK_NONE: i32 = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            PTP_CLOCK_GETCAPS,
            PTP_EXTTS_REQUEST,
            PTP_PEROUT_REQUEST,
            PTP_ENABLE_PPS,
            PTP_SYS_OFFSET,
            PTP_PIN_GETFUNC,
            PTP_PIN_SETFUNC,
            PTP_SYS_OFFSET_PRECISE,
            PTP_SYS_OFFSET_EXTENDED,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_are_powers_of_two() {
        let flags = [
            PTP_ENABLE_FEATURE,
            PTP_RISING_EDGE,
            PTP_FALLING_EDGE,
            PTP_STRICT_FLAGS,
            PTP_EXT_OFFSET,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_pin_funcs_distinct() {
        let funcs = [PTP_PF_NONE, PTP_PF_EXTTS, PTP_PF_PEROUT, PTP_PF_PHYSYNC];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_max_values() {
        assert_eq!(PTP_MAX_ALARMS, 4);
        assert_eq!(PTP_MAX_EXT_TS, 8);
        assert_eq!(PTP_MAX_PER_OUT, 4);
    }

    #[test]
    fn test_clock_none() {
        assert_eq!(PTP_CLOCK_NONE, -1);
    }
}

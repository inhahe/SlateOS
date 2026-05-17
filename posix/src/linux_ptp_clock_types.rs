//! `<linux/ptp_clock.h>` — PTP (Precision Time Protocol) hardware clock constants.
//!
//! PTP hardware clocks (PHCs) are NICs with IEEE 1588 timestamping
//! hardware. The PTP subsystem exposes each PHC as `/dev/ptpN` with
//! IOCTLs for reading/setting time, configuring alarms, enabling
//! external timestamp pins, and adjusting frequency. Used by linuxptp
//! (ptp4l/phc2sys), Chrony, and telecom timing applications for
//! sub-microsecond clock synchronization.

// ---------------------------------------------------------------------------
// PTP clock IOCTLs
// ---------------------------------------------------------------------------

/// Get PTP clock capabilities.
pub const PTP_CLOCK_GETCAPS: u32 = 0x01;
/// Enable external timestamp events.
pub const PTP_EXTTS_REQUEST: u32 = 0x02;
/// Configure periodic output signal.
pub const PTP_PEROUT_REQUEST: u32 = 0x03;
/// Enable PPS (pulse-per-second) output.
pub const PTP_ENABLE_PPS: u32 = 0x04;
/// Get system-to-PHC time offset (SYS_OFFSET).
pub const PTP_SYS_OFFSET: u32 = 0x05;
/// Configure pin function.
pub const PTP_PIN_SETFUNC: u32 = 0x06;
/// Get pin function.
pub const PTP_PIN_GETFUNC: u32 = 0x07;
/// Precise system-to-PHC offset (using cross-timestamp HW).
pub const PTP_SYS_OFFSET_PRECISE: u32 = 0x08;
/// Extended system-to-PHC offset (multiple samples).
pub const PTP_SYS_OFFSET_EXTENDED: u32 = 0x09;

// ---------------------------------------------------------------------------
// PTP clock capabilities flags
// ---------------------------------------------------------------------------

/// Supports external timestamp input.
pub const PTP_CLOCK_CAP_EXTTS: u32 = 1 << 0;
/// Supports periodic output.
pub const PTP_CLOCK_CAP_PEROUT: u32 = 1 << 1;
/// Supports PPS output.
pub const PTP_CLOCK_CAP_PPS: u32 = 1 << 2;
/// Supports cross-timestamp (precise offset).
pub const PTP_CLOCK_CAP_CROSS_TS: u32 = 1 << 3;
/// Supports adjust phase.
pub const PTP_CLOCK_CAP_ADJUST_PHASE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// PTP pin function types
// ---------------------------------------------------------------------------

/// Pin not in use.
pub const PTP_PF_NONE: u32 = 0;
/// Pin configured for external timestamp input.
pub const PTP_PF_EXTTS: u32 = 1;
/// Pin configured for periodic output.
pub const PTP_PF_PEROUT: u32 = 2;
/// Pin configured for physical hardware clock (PHC).
pub const PTP_PF_PHYSYNC: u32 = 3;

// ---------------------------------------------------------------------------
// External timestamp flags
// ---------------------------------------------------------------------------

/// Enable rising edge detection.
pub const PTP_RISING_EDGE: u32 = 1 << 0;
/// Enable falling edge detection.
pub const PTP_FALLING_EDGE: u32 = 1 << 1;
/// Strictly enforce period (no phase correction).
pub const PTP_STRICT_FLAGS: u32 = 1 << 2;
/// Enable external timestamp events via poll().
pub const PTP_EXTTS_EDGES: u32 = (1 << 0) | (1 << 1);

// ---------------------------------------------------------------------------
// Periodic output flags
// ---------------------------------------------------------------------------

/// Duty cycle control available.
pub const PTP_PEROUT_DUTY_CYCLE: u32 = 1 << 0;
/// Phase control available.
pub const PTP_PEROUT_PHASE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// PTP system offset max samples
// ---------------------------------------------------------------------------

/// Maximum number of samples for SYS_OFFSET IOCTL.
pub const PTP_MAX_SAMPLES: u32 = 25;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            PTP_CLOCK_GETCAPS, PTP_EXTTS_REQUEST,
            PTP_PEROUT_REQUEST, PTP_ENABLE_PPS,
            PTP_SYS_OFFSET, PTP_PIN_SETFUNC,
            PTP_PIN_GETFUNC, PTP_SYS_OFFSET_PRECISE,
            PTP_SYS_OFFSET_EXTENDED,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_caps_no_overlap() {
        let caps = [
            PTP_CLOCK_CAP_EXTTS, PTP_CLOCK_CAP_PEROUT,
            PTP_CLOCK_CAP_PPS, PTP_CLOCK_CAP_CROSS_TS,
            PTP_CLOCK_CAP_ADJUST_PHASE,
        ];
        for i in 0..caps.len() {
            assert!(caps[i].is_power_of_two());
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_pin_functions_distinct() {
        let funcs = [PTP_PF_NONE, PTP_PF_EXTTS, PTP_PF_PEROUT, PTP_PF_PHYSYNC];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_edge_flags_no_overlap() {
        assert_eq!(PTP_RISING_EDGE & PTP_FALLING_EDGE, 0);
        assert_eq!(PTP_EXTTS_EDGES, PTP_RISING_EDGE | PTP_FALLING_EDGE);
    }

    #[test]
    fn test_perout_flags_no_overlap() {
        assert_eq!(PTP_PEROUT_DUTY_CYCLE & PTP_PEROUT_PHASE, 0);
    }

    #[test]
    fn test_max_samples() {
        assert_eq!(PTP_MAX_SAMPLES, 25);
        assert!(PTP_MAX_SAMPLES > 0);
    }
}

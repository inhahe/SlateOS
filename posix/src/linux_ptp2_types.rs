//! `<linux/ptp_clock.h>` — Additional PTP (Precision Time Protocol) constants.
//!
//! Supplementary PTP constants covering clock capabilities,
//! external timestamp modes, and pin functions.

// ---------------------------------------------------------------------------
// PTP clock capabilities (PTP_CLK_REQ_*)
// ---------------------------------------------------------------------------

/// External timestamp.
pub const PTP_CLK_REQ_EXTTS: u32 = 0;
/// Periodic output.
pub const PTP_CLK_REQ_PEROUT: u32 = 1;
/// PPS enable.
pub const PTP_CLK_REQ_PPS: u32 = 2;

// ---------------------------------------------------------------------------
// External timestamp flags (PTP_EXTTS_FLAG_*)
// ---------------------------------------------------------------------------

/// Rising edge.
pub const PTP_RISING_EDGE: u32 = 1 << 1;
/// Falling edge.
pub const PTP_FALLING_EDGE: u32 = 1 << 2;
/// Strict flags.
pub const PTP_STRICT_FLAGS: u32 = 1 << 3;
/// External timestamp valid.
pub const PTP_EXTTS_VALID_FLAGS: u32 = PTP_RISING_EDGE | PTP_FALLING_EDGE | PTP_STRICT_FLAGS;
/// Enable feature.
pub const PTP_ENABLE_FEATURE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Periodic output flags
// ---------------------------------------------------------------------------

/// Duty cycle.
pub const PTP_PEROUT_DUTY_CYCLE: u32 = 1 << 1;
/// Phase.
pub const PTP_PEROUT_PHASE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Pin functions (PTP_PF_*)
// ---------------------------------------------------------------------------

/// No function.
pub const PTP_PF_NONE: u32 = 0;
/// External timestamp.
pub const PTP_PF_EXTTS: u32 = 1;
/// Periodic output.
pub const PTP_PF_PEROUT: u32 = 2;
/// Physical hardware clock.
pub const PTP_PF_PHYSYNC: u32 = 3;

// ---------------------------------------------------------------------------
// Max values
// ---------------------------------------------------------------------------

/// Max clock name length.
pub const PTP_CLOCK_NAME_LEN: u32 = 32;
/// Max number of pins.
pub const PTP_MAX_SAMPLES: u32 = 25;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_req_types_sequential() {
        assert_eq!(PTP_CLK_REQ_EXTTS, 0);
        assert_eq!(PTP_CLK_REQ_PEROUT, 1);
        assert_eq!(PTP_CLK_REQ_PPS, 2);
    }

    #[test]
    fn test_extts_flags_power_of_two() {
        assert!(PTP_ENABLE_FEATURE.is_power_of_two());
        assert!(PTP_RISING_EDGE.is_power_of_two());
        assert!(PTP_FALLING_EDGE.is_power_of_two());
        assert!(PTP_STRICT_FLAGS.is_power_of_two());
    }

    #[test]
    fn test_valid_flags_mask() {
        assert_eq!(PTP_EXTTS_VALID_FLAGS, PTP_RISING_EDGE | PTP_FALLING_EDGE | PTP_STRICT_FLAGS);
    }

    #[test]
    fn test_pin_functions_sequential() {
        assert_eq!(PTP_PF_NONE, 0);
        assert_eq!(PTP_PF_EXTTS, 1);
        assert_eq!(PTP_PF_PEROUT, 2);
        assert_eq!(PTP_PF_PHYSYNC, 3);
    }

    #[test]
    fn test_name_len() {
        assert_eq!(PTP_CLOCK_NAME_LEN, 32);
    }
}

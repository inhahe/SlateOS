//! `<linux/pwm.h>` — Pulse Width Modulation constants.
//!
//! PWM (Pulse Width Modulation) is used for controlling LEDs,
//! motor speed, fan speed, display backlights, and servo motors.
//! The Linux PWM subsystem provides a unified framework for PWM
//! controllers and their consumers.

// ---------------------------------------------------------------------------
// PWM polarity
// ---------------------------------------------------------------------------

/// Normal polarity (high during duty cycle).
pub const PWM_POLARITY_NORMAL: u8 = 0;
/// Inversed polarity (low during duty cycle).
pub const PWM_POLARITY_INVERSED: u8 = 1;

// ---------------------------------------------------------------------------
// PWM output state
// ---------------------------------------------------------------------------

/// PWM output enabled.
pub const PWM_STATE_ENABLED: u8 = 1;
/// PWM output disabled.
pub const PWM_STATE_DISABLED: u8 = 0;

// ---------------------------------------------------------------------------
// PWM capture flags
// ---------------------------------------------------------------------------

/// Capture rising edge.
pub const PWM_CAPTURE_RISING: u32 = 1 << 0;
/// Capture falling edge.
pub const PWM_CAPTURE_FALLING: u32 = 1 << 1;
/// Capture both edges.
pub const PWM_CAPTURE_BOTH: u32 = (1 << 0) | (1 << 1);

// ---------------------------------------------------------------------------
// Common PWM frequencies (periods in nanoseconds)
// ---------------------------------------------------------------------------

/// 1 kHz (period = 1 ms).
pub const PWM_PERIOD_1KHZ_NS: u32 = 1_000_000;
/// 10 kHz (period = 100 us).
pub const PWM_PERIOD_10KHZ_NS: u32 = 100_000;
/// 25 kHz (typical PC fan PWM).
pub const PWM_PERIOD_25KHZ_NS: u32 = 40_000;
/// 50 Hz (typical servo PWM).
pub const PWM_PERIOD_50HZ_NS: u32 = 20_000_000;
/// 100 kHz.
pub const PWM_PERIOD_100KHZ_NS: u32 = 10_000;
/// 1 MHz.
pub const PWM_PERIOD_1MHZ_NS: u32 = 1_000;

// ---------------------------------------------------------------------------
// PWM chip capabilities
// ---------------------------------------------------------------------------

/// Supports polarity inversion.
pub const PWM_CAP_POLARITY: u32 = 1 << 0;
/// Supports duty cycle of 0%.
pub const PWM_CAP_DUTY_ZERO: u32 = 1 << 1;
/// Supports duty cycle of 100%.
pub const PWM_CAP_DUTY_FULL: u32 = 1 << 2;
/// Supports capture mode.
pub const PWM_CAP_CAPTURE: u32 = 1 << 3;
/// Supports atomic update.
pub const PWM_CAP_ATOMIC: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polarity_distinct() {
        assert_ne!(PWM_POLARITY_NORMAL, PWM_POLARITY_INVERSED);
    }

    #[test]
    fn test_state_distinct() {
        assert_ne!(PWM_STATE_ENABLED, PWM_STATE_DISABLED);
    }

    #[test]
    fn test_capture_flags_bits() {
        assert_eq!(PWM_CAPTURE_BOTH, PWM_CAPTURE_RISING | PWM_CAPTURE_FALLING);
        assert_ne!(PWM_CAPTURE_RISING, PWM_CAPTURE_FALLING);
    }

    #[test]
    fn test_periods_decreasing_with_frequency() {
        // Higher frequency → shorter period
        assert!(PWM_PERIOD_50HZ_NS > PWM_PERIOD_1KHZ_NS);
        assert!(PWM_PERIOD_1KHZ_NS > PWM_PERIOD_10KHZ_NS);
        assert!(PWM_PERIOD_10KHZ_NS > PWM_PERIOD_25KHZ_NS);
        assert!(PWM_PERIOD_25KHZ_NS > PWM_PERIOD_100KHZ_NS);
        assert!(PWM_PERIOD_100KHZ_NS > PWM_PERIOD_1MHZ_NS);
    }

    #[test]
    fn test_capabilities_no_overlap() {
        let caps = [
            PWM_CAP_POLARITY, PWM_CAP_DUTY_ZERO,
            PWM_CAP_DUTY_FULL, PWM_CAP_CAPTURE, PWM_CAP_ATOMIC,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_eq!(caps[i] & caps[j], 0);
            }
        }
    }

    #[test]
    fn test_capabilities_power_of_two() {
        let caps = [
            PWM_CAP_POLARITY, PWM_CAP_DUTY_ZERO,
            PWM_CAP_DUTY_FULL, PWM_CAP_CAPTURE, PWM_CAP_ATOMIC,
        ];
        for c in &caps {
            assert!(c.is_power_of_two());
        }
    }
}

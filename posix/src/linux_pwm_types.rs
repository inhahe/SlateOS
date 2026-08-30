//! `<linux/pwm.h>` — Pulse Width Modulation (PWM) constants.
//!
//! The PWM subsystem drives hardware PWM outputs for controlling
//! LED brightness, motor speed, fan speed, display backlights,
//! and other analog-like signals from digital GPIOs. Each PWM
//! channel has a period, duty cycle, and polarity.

// ---------------------------------------------------------------------------
// PWM polarity
// ---------------------------------------------------------------------------

/// Normal polarity (high during duty, low during remainder).
pub const PWM_POLARITY_NORMAL: u32 = 0;
/// Inverted polarity (low during duty, high during remainder).
pub const PWM_POLARITY_INVERSED: u32 = 1;

// ---------------------------------------------------------------------------
// PWM flags
// ---------------------------------------------------------------------------

/// PWM is currently enabled.
pub const PWM_FLAG_ENABLED: u32 = 1 << 0;
/// PWM polarity is inverted.
pub const PWM_FLAG_POLARITY_INVERTED: u32 = 1 << 1;
/// PWM is exported to sysfs.
pub const PWM_FLAG_EXPORTED: u32 = 1 << 2;
/// PWM requested (in use).
pub const PWM_FLAG_REQUESTED: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// PWM capture states
// ---------------------------------------------------------------------------

/// Capture idle (waiting for edge).
pub const PWM_CAPTURE_IDLE: u32 = 0;
/// Capture running (measuring period).
pub const PWM_CAPTURE_RUNNING: u32 = 1;
/// Capture complete (data available).
pub const PWM_CAPTURE_DONE: u32 = 2;

// ---------------------------------------------------------------------------
// Common PWM frequencies (Hz) for reference
// ---------------------------------------------------------------------------

/// Typical LED PWM frequency (1 kHz).
pub const PWM_FREQ_LED_DEFAULT: u32 = 1000;
/// Typical fan PWM frequency (25 kHz).
pub const PWM_FREQ_FAN_DEFAULT: u32 = 25000;
/// Typical servo PWM frequency (50 Hz).
pub const PWM_FREQ_SERVO: u32 = 50;
/// Typical LCD backlight frequency (200 Hz).
pub const PWM_FREQ_BACKLIGHT: u32 = 200;

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
    fn test_flags_no_overlap() {
        let flags = [
            PWM_FLAG_ENABLED,
            PWM_FLAG_POLARITY_INVERTED,
            PWM_FLAG_EXPORTED,
            PWM_FLAG_REQUESTED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_capture_states_distinct() {
        assert_ne!(PWM_CAPTURE_IDLE, PWM_CAPTURE_RUNNING);
        assert_ne!(PWM_CAPTURE_RUNNING, PWM_CAPTURE_DONE);
    }

    #[test]
    fn test_frequencies() {
        assert!(PWM_FREQ_SERVO < PWM_FREQ_BACKLIGHT);
        assert!(PWM_FREQ_BACKLIGHT < PWM_FREQ_LED_DEFAULT);
        assert!(PWM_FREQ_LED_DEFAULT < PWM_FREQ_FAN_DEFAULT);
    }
}

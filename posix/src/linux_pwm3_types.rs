//! `<linux/pwm.h>` — Additional PWM constants (part 3).
//!
//! Supplementary PWM constants covering polarity values,
//! capture flags, and channel state.

// ---------------------------------------------------------------------------
// PWM polarity
// ---------------------------------------------------------------------------

/// Normal polarity (active high).
pub const PWM_POLARITY_NORMAL: u32 = 0;
/// Inversed polarity (active low).
pub const PWM_POLARITY_INVERSED: u32 = 1;

// ---------------------------------------------------------------------------
// PWM state flags
// ---------------------------------------------------------------------------

/// PWM enabled.
pub const PWM_STATE_ENABLED: u32 = 1 << 0;
/// Usage power (PM hint).
pub const PWM_STATE_USAGE_POWER: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// PWM sysfs constants
// ---------------------------------------------------------------------------

/// Export a PWM channel.
pub const PWM_SYSFS_EXPORT: u32 = 0;
/// Unexport a PWM channel.
pub const PWM_SYSFS_UNEXPORT: u32 = 1;

// ---------------------------------------------------------------------------
// PWM capture flags
// ---------------------------------------------------------------------------

/// Capture duty cycle.
pub const PWM_CAPTURE_DUTY_CYCLE: u32 = 0;
/// Capture period.
pub const PWM_CAPTURE_PERIOD: u32 = 1;

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
    fn test_state_flags_no_overlap() {
        assert_eq!(PWM_STATE_ENABLED & PWM_STATE_USAGE_POWER, 0);
    }

    #[test]
    fn test_sysfs_distinct() {
        assert_ne!(PWM_SYSFS_EXPORT, PWM_SYSFS_UNEXPORT);
    }

    #[test]
    fn test_capture_distinct() {
        assert_ne!(PWM_CAPTURE_DUTY_CYCLE, PWM_CAPTURE_PERIOD);
    }
}

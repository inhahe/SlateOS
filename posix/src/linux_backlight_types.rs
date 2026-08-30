//! `<linux/backlight.h>` — Backlight subsystem constants.
//!
//! The backlight subsystem provides a unified interface for display
//! brightness control across different hardware (ACPI, platform,
//! firmware, raw PWM). Exposed via /sys/class/backlight/ and used
//! by desktop environments, power managers, and ambient light
//! daemons to adjust screen brightness.

// ---------------------------------------------------------------------------
// Backlight types (driver categories)
// ---------------------------------------------------------------------------

/// Raw hardware register control (direct PWM/register).
pub const BACKLIGHT_RAW: u32 = 0;
/// Platform-specific control (vendor ACPI/WMI).
pub const BACKLIGHT_PLATFORM: u32 = 1;
/// ACPI/firmware-based control.
pub const BACKLIGHT_FIRMWARE: u32 = 2;
/// Number of backlight types.
pub const BACKLIGHT_TYPE_MAX: u32 = 3;

// ---------------------------------------------------------------------------
// Backlight update reasons
// ---------------------------------------------------------------------------

/// User explicitly requested brightness change.
pub const BACKLIGHT_UPDATE_HOTKEY: u32 = 0;
/// Sysfs write triggered the change.
pub const BACKLIGHT_UPDATE_SYSFS: u32 = 1;

// ---------------------------------------------------------------------------
// Common brightness values
// ---------------------------------------------------------------------------

/// Minimum brightness (display off, if supported).
pub const BACKLIGHT_MIN_BRIGHTNESS: u32 = 0;
/// Typical maximum brightness for integer-scaled backends.
pub const BACKLIGHT_SCALE_LINEAR_MAX: u32 = 100;

// ---------------------------------------------------------------------------
// Backlight scale types (Linux 5.10+)
// ---------------------------------------------------------------------------

/// Unknown scale.
pub const BACKLIGHT_SCALE_UNKNOWN: u32 = 0;
/// Linear scale (perceived brightness is linear with value).
pub const BACKLIGHT_SCALE_LINEAR: u32 = 1;
/// Non-linear scale (value maps to hardware directly).
pub const BACKLIGHT_SCALE_NON_LINEAR: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        let types = [BACKLIGHT_RAW, BACKLIGHT_PLATFORM, BACKLIGHT_FIRMWARE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_type_max() {
        assert_eq!(BACKLIGHT_TYPE_MAX, 3);
        assert!(BACKLIGHT_FIRMWARE < BACKLIGHT_TYPE_MAX);
    }

    #[test]
    fn test_update_reasons_distinct() {
        assert_ne!(BACKLIGHT_UPDATE_HOTKEY, BACKLIGHT_UPDATE_SYSFS);
    }

    #[test]
    fn test_scale_types_distinct() {
        let scales = [
            BACKLIGHT_SCALE_UNKNOWN,
            BACKLIGHT_SCALE_LINEAR,
            BACKLIGHT_SCALE_NON_LINEAR,
        ];
        for i in 0..scales.len() {
            for j in (i + 1)..scales.len() {
                assert_ne!(scales[i], scales[j]);
            }
        }
    }

    #[test]
    fn test_brightness_range() {
        assert!(BACKLIGHT_MIN_BRIGHTNESS < BACKLIGHT_SCALE_LINEAR_MAX);
    }
}

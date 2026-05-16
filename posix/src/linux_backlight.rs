//! `<linux/backlight.h>` — Backlight device constants.
//!
//! The backlight subsystem controls display panel backlights
//! on laptops, tablets, and monitors. Brightness is exposed
//! via sysfs (/sys/class/backlight/). Drivers register with
//! a type indicating how brightness is controlled.

// ---------------------------------------------------------------------------
// Backlight types
// ---------------------------------------------------------------------------

/// Raw backlight (direct hardware register).
pub const BACKLIGHT_RAW: u32 = 0;
/// Platform backlight (ACPI, WMI, vendor-specific).
pub const BACKLIGHT_PLATFORM: u32 = 1;
/// Firmware backlight (EFI, BIOS).
pub const BACKLIGHT_FIRMWARE: u32 = 2;

// ---------------------------------------------------------------------------
// Backlight update reasons
// ---------------------------------------------------------------------------

/// Sysfs update (user wrote to brightness file).
pub const BACKLIGHT_UPDATE_SYSFS: u32 = 0;
/// Hotkey update (hardware brightness key pressed).
pub const BACKLIGHT_UPDATE_HOTKEY: u32 = 1;

// ---------------------------------------------------------------------------
// Backlight power states (fb_blank values reused)
// ---------------------------------------------------------------------------

/// Backlight on.
pub const BACKLIGHT_POWER_ON: u32 = 0;
/// Backlight reduced (standby).
pub const BACKLIGHT_POWER_REDUCED: u32 = 1;
/// Backlight off.
pub const BACKLIGHT_POWER_OFF: u32 = 4;

// ---------------------------------------------------------------------------
// Backlight scale
// ---------------------------------------------------------------------------

/// Unknown brightness scale.
pub const BACKLIGHT_SCALE_UNKNOWN: u32 = 0;
/// Linear brightness scale.
pub const BACKLIGHT_SCALE_LINEAR: u32 = 1;
/// Non-linear (perceptual) brightness scale.
pub const BACKLIGHT_SCALE_NON_LINEAR: u32 = 2;

// ---------------------------------------------------------------------------
// Sysfs attribute names
// ---------------------------------------------------------------------------

/// Current brightness.
pub const BL_ATTR_BRIGHTNESS: &str = "brightness";
/// Maximum brightness.
pub const BL_ATTR_MAX_BRIGHTNESS: &str = "max_brightness";
/// Actual brightness (hardware-reported).
pub const BL_ATTR_ACTUAL_BRIGHTNESS: &str = "actual_brightness";
/// Power state.
pub const BL_ATTR_BL_POWER: &str = "bl_power";
/// Backlight type.
pub const BL_ATTR_TYPE: &str = "type";
/// Scale type.
pub const BL_ATTR_SCALE: &str = "scale";

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
    fn test_update_reasons_distinct() {
        assert_ne!(BACKLIGHT_UPDATE_SYSFS, BACKLIGHT_UPDATE_HOTKEY);
    }

    #[test]
    fn test_power_states_distinct() {
        let states = [
            BACKLIGHT_POWER_ON, BACKLIGHT_POWER_REDUCED,
            BACKLIGHT_POWER_OFF,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_scales_distinct() {
        let scales = [
            BACKLIGHT_SCALE_UNKNOWN, BACKLIGHT_SCALE_LINEAR,
            BACKLIGHT_SCALE_NON_LINEAR,
        ];
        for i in 0..scales.len() {
            for j in (i + 1)..scales.len() {
                assert_ne!(scales[i], scales[j]);
            }
        }
    }

    #[test]
    fn test_attr_names_distinct() {
        let attrs = [
            BL_ATTR_BRIGHTNESS, BL_ATTR_MAX_BRIGHTNESS,
            BL_ATTR_ACTUAL_BRIGHTNESS, BL_ATTR_BL_POWER,
            BL_ATTR_TYPE, BL_ATTR_SCALE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

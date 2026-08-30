//! `<linux/input.h>` (INPUT_PROP subset) — input device property flags.
//!
//! Input properties describe physical characteristics of input devices
//! that don't change during the device's lifetime. They help userspace
//! (libinput, X11, Wayland compositors) configure correct handling —
//! e.g. whether a touchpad is a clickpad, whether a touchscreen is
//! direct-touch, or whether a device has an accelerometer.

// ---------------------------------------------------------------------------
// Device properties
// ---------------------------------------------------------------------------

/// Device has no special properties.
pub const INPUT_PROP_POINTER: u16 = 0x00;
/// Device is a direct input device (e.g. touchscreen).
pub const INPUT_PROP_DIRECT: u16 = 0x01;
/// Device is a clickpad (whole surface is one button).
pub const INPUT_PROP_BUTTONPAD: u16 = 0x02;
/// Device is a semi-multitouch device (reports bounding box only).
pub const INPUT_PROP_SEMI_MT: u16 = 0x03;
/// Device is a top-button area pad (Lenovo *40 series).
pub const INPUT_PROP_TOPBUTTONPAD: u16 = 0x04;
/// Device is a pointing stick (TrackPoint-like).
pub const INPUT_PROP_POINTING_STICK: u16 = 0x05;
/// Device has an accelerometer.
pub const INPUT_PROP_ACCELEROMETER: u16 = 0x06;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum property code.
pub const INPUT_PROP_MAX: u16 = 0x1F;
/// Number of property codes (INPUT_PROP_MAX + 1).
pub const INPUT_PROP_CNT: u16 = 0x20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_props_distinct() {
        let props = [
            INPUT_PROP_POINTER,
            INPUT_PROP_DIRECT,
            INPUT_PROP_BUTTONPAD,
            INPUT_PROP_SEMI_MT,
            INPUT_PROP_TOPBUTTONPAD,
            INPUT_PROP_POINTING_STICK,
            INPUT_PROP_ACCELEROMETER,
        ];
        for i in 0..props.len() {
            for j in (i + 1)..props.len() {
                assert_ne!(props[i], props[j], "props {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn test_props_sequential() {
        assert_eq!(INPUT_PROP_POINTER, 0);
        assert_eq!(INPUT_PROP_DIRECT, 1);
        assert_eq!(INPUT_PROP_BUTTONPAD, 2);
        assert_eq!(INPUT_PROP_SEMI_MT, 3);
        assert_eq!(INPUT_PROP_TOPBUTTONPAD, 4);
        assert_eq!(INPUT_PROP_POINTING_STICK, 5);
        assert_eq!(INPUT_PROP_ACCELEROMETER, 6);
    }

    #[test]
    fn test_all_within_max() {
        let props = [
            INPUT_PROP_POINTER,
            INPUT_PROP_DIRECT,
            INPUT_PROP_BUTTONPAD,
            INPUT_PROP_SEMI_MT,
            INPUT_PROP_TOPBUTTONPAD,
            INPUT_PROP_POINTING_STICK,
            INPUT_PROP_ACCELEROMETER,
        ];
        for &p in &props {
            assert!(
                p <= INPUT_PROP_MAX,
                "prop 0x{:02X} exceeds INPUT_PROP_MAX",
                p
            );
        }
    }

    #[test]
    fn test_prop_cnt() {
        assert_eq!(INPUT_PROP_CNT, INPUT_PROP_MAX + 1);
    }
}

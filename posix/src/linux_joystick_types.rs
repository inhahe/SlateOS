//! `<linux/joystick.h>` — Joystick/gamepad API constants.
//!
//! The Linux joystick API provides access to game controllers via
//! /dev/input/jsN devices. Applications read `js_event` structures
//! containing axis positions and button states. Largely superseded
//! by the evdev API for modern applications but still widely
//! supported for backward compatibility.

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// Button press/release event.
pub const JS_EVENT_BUTTON: u32 = 0x01;
/// Axis movement event.
pub const JS_EVENT_AXIS: u32 = 0x02;
/// Initial state event (sent on open).
pub const JS_EVENT_INIT: u32 = 0x80;

// ---------------------------------------------------------------------------
// Joystick ioctl commands
// ---------------------------------------------------------------------------

/// Get joystick driver version.
pub const JSIOCGVERSION: u32 = 0x8004_6A01;
/// Get number of axes.
pub const JSIOCGAXES: u32 = 0x8001_6A11;
/// Get number of buttons.
pub const JSIOCGBUTTONS: u32 = 0x8001_6A12;
/// Get device name.
pub const JSIOCGNAME: u32 = 0x8100_6A13;
/// Set axis correction (calibration).
pub const JSIOCSCORR: u32 = 0x4024_6A21;
/// Get axis correction.
pub const JSIOCGCORR: u32 = 0x8024_6A22;
/// Set axis mapping.
pub const JSIOCSAXMAP: u32 = 0x4040_6A31;
/// Get axis mapping.
pub const JSIOCGAXMAP: u32 = 0x8040_6A32;
/// Set button mapping.
pub const JSIOCSBTNMAP: u32 = 0x4400_6A33;
/// Get button mapping.
pub const JSIOCGBTNMAP: u32 = 0x8400_6A34;

// ---------------------------------------------------------------------------
// Axis ranges
// ---------------------------------------------------------------------------

/// Minimum axis value.
pub const JS_AXIS_MIN: i16 = -32767;
/// Maximum axis value.
pub const JS_AXIS_MAX: i16 = 32767;

// ---------------------------------------------------------------------------
// Correction types
// ---------------------------------------------------------------------------

/// No correction.
pub const JS_CORR_NONE: u32 = 0;
/// Broken line correction.
pub const JS_CORR_BROKEN: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_no_overlap() {
        assert_eq!(JS_EVENT_BUTTON & JS_EVENT_AXIS, 0);
        assert_eq!(JS_EVENT_BUTTON & JS_EVENT_INIT, 0);
        assert_eq!(JS_EVENT_AXIS & JS_EVENT_INIT, 0);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            JSIOCGVERSION, JSIOCGAXES, JSIOCGBUTTONS, JSIOCGNAME,
            JSIOCSCORR, JSIOCGCORR, JSIOCSAXMAP, JSIOCGAXMAP,
            JSIOCSBTNMAP, JSIOCGBTNMAP,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_axis_range() {
        assert!(JS_AXIS_MIN < 0);
        assert!(JS_AXIS_MAX > 0);
        assert_eq!(JS_AXIS_MIN, -JS_AXIS_MAX);
    }

    #[test]
    fn test_correction_types_distinct() {
        assert_ne!(JS_CORR_NONE, JS_CORR_BROKEN);
    }

    #[test]
    fn test_init_is_flag() {
        // INIT can be ORed with BUTTON or AXIS.
        let init_button = JS_EVENT_INIT | JS_EVENT_BUTTON;
        assert_ne!(init_button, JS_EVENT_INIT);
        assert_ne!(init_button, JS_EVENT_BUTTON);
    }
}

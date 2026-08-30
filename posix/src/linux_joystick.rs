//! `<linux/joystick.h>` — joystick input interface.
//!
//! The Linux joystick API provides a character device interface
//! (`/dev/input/jsN`) for reading joystick/gamepad events. Modern
//! programs often use the evdev interface instead, but the joystick
//! API remains widely supported.

// ---------------------------------------------------------------------------
// Joystick event types
// ---------------------------------------------------------------------------

/// Button was pressed or released.
pub const JS_EVENT_BUTTON: u8 = 0x01;
/// Joystick axis moved.
pub const JS_EVENT_AXIS: u8 = 0x02;
/// Initial state of the device.
pub const JS_EVENT_INIT: u8 = 0x80;

// ---------------------------------------------------------------------------
// Joystick event struct
// ---------------------------------------------------------------------------

/// Joystick event (8 bytes).
///
/// Read from `/dev/input/jsN`. The `type_` field is a combination
/// of `JS_EVENT_*` flags.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JsEvent {
    /// Event timestamp in milliseconds.
    pub time: u32,
    /// Value (axis: -32767..32767, button: 0 or 1).
    pub value: i16,
    /// Event type (`JS_EVENT_BUTTON`, `JS_EVENT_AXIS`, optionally ORed with `JS_EVENT_INIT`).
    pub type_: u8,
    /// Axis/button number.
    pub number: u8,
}

impl JsEvent {
    /// Create a zeroed joystick event.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Joystick ioctl commands
// ---------------------------------------------------------------------------

/// Get driver version.
pub const JSIOCGVERSION: u64 = 0x80046A01;
/// Get number of axes.
pub const JSIOCGAXES: u64 = 0x80016A11;
/// Get number of buttons.
pub const JSIOCGBUTTONS: u64 = 0x80016A12;
/// Get axis mapping (array of u8).
pub const JSIOCGAXMAP: u64 = 0x80406A32;
/// Set axis mapping.
pub const JSIOCSAXMAP: u64 = 0x40406A31;
/// Get button mapping (array of u16).
pub const JSIOCGBTNMAP: u64 = 0x84006A34;
/// Set button mapping.
pub const JSIOCSBTNMAP: u64 = 0x44006A33;
/// Get device name (string).
pub const JSIOCGNAME_BASE: u64 = 0x80006A13;

// ---------------------------------------------------------------------------
// Joystick axis/button limits
// ---------------------------------------------------------------------------

/// Maximum axis value.
pub const JS_MAX_AXIS: i16 = 32767;
/// Minimum axis value.
pub const JS_MIN_AXIS: i16 = -32767;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_event_size() {
        assert_eq!(core::mem::size_of::<JsEvent>(), 8);
    }

    #[test]
    fn test_js_event_zeroed() {
        let ev = JsEvent::zeroed();
        assert_eq!(ev.time, 0);
        assert_eq!(ev.value, 0);
        assert_eq!(ev.type_, 0);
        assert_eq!(ev.number, 0);
    }

    #[test]
    fn test_event_types_distinct() {
        assert_ne!(JS_EVENT_BUTTON, JS_EVENT_AXIS);
        assert_ne!(JS_EVENT_BUTTON, JS_EVENT_INIT);
        assert_ne!(JS_EVENT_AXIS, JS_EVENT_INIT);
    }

    #[test]
    fn test_event_init_combinable() {
        // JS_EVENT_INIT can be ORed with BUTTON or AXIS.
        let init_button = JS_EVENT_INIT | JS_EVENT_BUTTON;
        let init_axis = JS_EVENT_INIT | JS_EVENT_AXIS;
        assert_ne!(init_button, init_axis);
        assert!(init_button & JS_EVENT_INIT != 0);
        assert!(init_axis & JS_EVENT_INIT != 0);
    }

    #[test]
    fn test_axis_range() {
        assert_eq!(JS_MAX_AXIS, 32767);
        assert_eq!(JS_MIN_AXIS, -32767);
        assert!(JS_MAX_AXIS > 0);
        assert!(JS_MIN_AXIS < 0);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            JSIOCGVERSION,
            JSIOCGAXES,
            JSIOCGBUTTONS,
            JSIOCGAXMAP,
            JSIOCSAXMAP,
            JSIOCGBTNMAP,
            JSIOCSBTNMAP,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}

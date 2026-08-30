//! `<linux/joystick.h>` — `/dev/input/js*` legacy joystick API.
//!
//! The legacy `js` API predates `evdev` but is still the most
//! widely-supported interface for games that use SDL2's joystick
//! backend, RetroArch, and most emulators. The driver exposes
//! per-event packets of 8 bytes containing a timestamp, value, type,
//! and number — encoded as defined here.

// ---------------------------------------------------------------------------
// Event type bits (`js_event.type`)
// ---------------------------------------------------------------------------

/// Button pressed/released.
pub const JS_EVENT_BUTTON: u8 = 0x01;
/// Axis moved.
pub const JS_EVENT_AXIS: u8 = 0x02;
/// Initial state of all axes/buttons on first read (or'd in).
pub const JS_EVENT_INIT: u8 = 0x80;

// ---------------------------------------------------------------------------
// Driver version
// ---------------------------------------------------------------------------

/// Joystick driver version (major.minor.patch) — 0x020100 = 2.1.0.
pub const JS_VERSION: u32 = 0x0002_0100;

// ---------------------------------------------------------------------------
// ioctl numbers (raw values — `_IO('j', n)`)
// ---------------------------------------------------------------------------

/// Get driver version.
pub const JSIOCGVERSION: u32 = 0x8004_6A01;
/// Get number of axes (1 byte).
pub const JSIOCGAXES: u32 = 0x8001_6A11;
/// Get number of buttons (1 byte).
pub const JSIOCGBUTTONS: u32 = 0x8001_6A12;
/// Get device name (variable length, JSIOCGNAME(len) macro in C).
pub const JSIOCGNAME_BASE: u32 = 0x8000_6A13;
/// Set axis correction.
pub const JSIOCSCORR: u32 = 0x4018_6A21;
/// Get axis correction.
pub const JSIOCGCORR: u32 = 0x8018_6A22;
/// Set axis mapping.
pub const JSIOCSAXMAP: u32 = 0x4040_6A31;
/// Get axis mapping.
pub const JSIOCGAXMAP: u32 = 0x8040_6A32;
/// Set button mapping.
pub const JSIOCSBTNMAP: u32 = 0x4200_6A33;
/// Get button mapping.
pub const JSIOCGBTNMAP: u32 = 0x8200_6A34;

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// `struct js_event` size on Linux: u32 time + s16 value + u8 type + u8 number.
pub const JS_EVENT_SIZE: usize = 8;
/// Per the kernel, `KEY_MAX - BTN_MISC` buttons fit on one device.
pub const JS_MAX_BUTTONS: u32 = 0x200;
/// `ABS_CNT` on Linux — maximum number of axes.
pub const JS_MAX_AXES: u32 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn test_event_type_bits() {
        assert_eq!(JS_EVENT_BUTTON, 0x01);
        assert_eq!(JS_EVENT_AXIS, 0x02);
        // INIT is the high bit, OR'd with the other two.
        assert_eq!(JS_EVENT_INIT, 0x80);
        // BUTTON and AXIS are distinct single bits and don't overlap INIT.
        assert!(JS_EVENT_BUTTON.is_power_of_two());
        assert!(JS_EVENT_AXIS.is_power_of_two());
        assert_eq!(JS_EVENT_BUTTON & JS_EVENT_AXIS, 0);
        assert_eq!((JS_EVENT_BUTTON | JS_EVENT_AXIS) & JS_EVENT_INIT, 0);
    }

    #[test]
    fn test_driver_version_layout() {
        // Major in bits 16..23, minor 8..15, patch 0..7.
        let major = (JS_VERSION >> 16) & 0xFF;
        let minor = (JS_VERSION >> 8) & 0xFF;
        let patch = JS_VERSION & 0xFF;
        assert_eq!(major, 2);
        assert_eq!(minor, 1);
        assert_eq!(patch, 0);
    }

    #[test]
    fn test_ioctl_type_byte_is_j() {
        // 'j' magic = 0x6A in bits 8..15 of every joystick ioctl.
        for cmd in [
            JSIOCGVERSION,
            JSIOCGAXES,
            JSIOCGBUTTONS,
            JSIOCGNAME_BASE,
            JSIOCSCORR,
            JSIOCGCORR,
            JSIOCSAXMAP,
            JSIOCGAXMAP,
            JSIOCSBTNMAP,
            JSIOCGBTNMAP,
        ] {
            assert_eq!((cmd >> 8) & 0xFF, u32::from(b'j'));
        }
    }

    #[test]
    fn test_event_size_matches_struct() {
        // 4 + 2 + 1 + 1 = 8 octets, no padding on x86_64.
        let computed = size_of::<u32>() + size_of::<i16>() + size_of::<u8>() + size_of::<u8>();
        assert_eq!(JS_EVENT_SIZE, 8);
        assert_eq!(JS_EVENT_SIZE, computed);
    }

    #[test]
    fn test_max_caps_sane() {
        // Linux exposes up to 512 buttons and 64 axes per joystick device.
        assert_eq!(JS_MAX_BUTTONS, 512);
        assert_eq!(JS_MAX_AXES, 64);
        assert!(JS_MAX_BUTTONS > JS_MAX_AXES);
    }
}

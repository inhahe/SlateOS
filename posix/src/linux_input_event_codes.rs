//! `<linux/input-event-codes.h>` — Input event type and code constants.
//!
//! Supplements linux_input.rs with the most commonly used key codes,
//! button codes, and relative/absolute axis codes from the input
//! event subsystem.

// ---------------------------------------------------------------------------
// Event types (EV_*)
// ---------------------------------------------------------------------------

/// Synchronization event.
pub const EV_SYN: u16 = 0x00;
/// Key/button event.
pub const EV_KEY: u16 = 0x01;
/// Relative axis event (mouse).
pub const EV_REL: u16 = 0x02;
/// Absolute axis event (touchscreen, joystick).
pub const EV_ABS: u16 = 0x03;
/// Miscellaneous event.
pub const EV_MSC: u16 = 0x04;
/// Switch event.
pub const EV_SW: u16 = 0x05;
/// LED event.
pub const EV_LED: u16 = 0x11;
/// Sound event.
pub const EV_SND: u16 = 0x12;
/// Repeat event.
pub const EV_REP: u16 = 0x14;
/// Force feedback event.
pub const EV_FF: u16 = 0x15;
/// Power event.
pub const EV_PWR: u16 = 0x16;
/// Force feedback status.
pub const EV_FF_STATUS: u16 = 0x17;
/// Max event type.
pub const EV_MAX: u16 = 0x1F;

// ---------------------------------------------------------------------------
// Common key codes (KEY_*)
// ---------------------------------------------------------------------------

/// Escape key.
pub const KEY_ESC: u16 = 1;
/// Enter / Return key.
pub const KEY_ENTER: u16 = 28;
/// Space key.
pub const KEY_SPACE: u16 = 57;
/// Backspace key.
pub const KEY_BACKSPACE: u16 = 14;
/// Tab key.
pub const KEY_TAB: u16 = 15;
/// Left Ctrl.
pub const KEY_LEFTCTRL: u16 = 29;
/// Left Shift.
pub const KEY_LEFTSHIFT: u16 = 42;
/// Left Alt.
pub const KEY_LEFTALT: u16 = 56;
/// Caps Lock.
pub const KEY_CAPSLOCK: u16 = 58;
/// Up arrow.
pub const KEY_UP: u16 = 103;
/// Down arrow.
pub const KEY_DOWN: u16 = 108;
/// Left arrow.
pub const KEY_LEFT: u16 = 105;
/// Right arrow.
pub const KEY_RIGHT: u16 = 106;
/// Delete key.
pub const KEY_DELETE: u16 = 111;

// ---------------------------------------------------------------------------
// Button codes (BTN_*)
// ---------------------------------------------------------------------------

/// Left mouse button.
pub const BTN_LEFT: u16 = 0x110;
/// Right mouse button.
pub const BTN_RIGHT: u16 = 0x111;
/// Middle mouse button.
pub const BTN_MIDDLE: u16 = 0x112;
/// Touch (touchscreen).
pub const BTN_TOUCH: u16 = 0x14A;
/// Stylus pen button.
pub const BTN_STYLUS: u16 = 0x14B;

// ---------------------------------------------------------------------------
// Relative axis codes (REL_*)
// ---------------------------------------------------------------------------

/// Relative X movement.
pub const REL_X: u16 = 0x00;
/// Relative Y movement.
pub const REL_Y: u16 = 0x01;
/// Relative Z (tilt).
pub const REL_Z: u16 = 0x02;
/// Wheel (vertical scroll).
pub const REL_WHEEL: u16 = 0x08;
/// Horizontal wheel (horizontal scroll).
pub const REL_HWHEEL: u16 = 0x06;
/// High-resolution wheel.
pub const REL_WHEEL_HI_RES: u16 = 0x0B;
/// High-resolution horizontal wheel.
pub const REL_HWHEEL_HI_RES: u16 = 0x0C;

// ---------------------------------------------------------------------------
// Absolute axis codes (ABS_*)
// ---------------------------------------------------------------------------

/// Absolute X position.
pub const ABS_X: u16 = 0x00;
/// Absolute Y position.
pub const ABS_Y: u16 = 0x01;
/// Absolute Z position.
pub const ABS_Z: u16 = 0x02;
/// Pressure.
pub const ABS_PRESSURE: u16 = 0x18;
/// Multitouch slot.
pub const ABS_MT_SLOT: u16 = 0x2F;
/// Multitouch X.
pub const ABS_MT_POSITION_X: u16 = 0x35;
/// Multitouch Y.
pub const ABS_MT_POSITION_Y: u16 = 0x36;
/// Multitouch tracking ID.
pub const ABS_MT_TRACKING_ID: u16 = 0x39;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let types = [
            EV_SYN,
            EV_KEY,
            EV_REL,
            EV_ABS,
            EV_MSC,
            EV_SW,
            EV_LED,
            EV_SND,
            EV_REP,
            EV_FF,
            EV_PWR,
            EV_FF_STATUS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_key_codes_distinct() {
        let keys = [
            KEY_ESC,
            KEY_ENTER,
            KEY_SPACE,
            KEY_BACKSPACE,
            KEY_TAB,
            KEY_LEFTCTRL,
            KEY_LEFTSHIFT,
            KEY_LEFTALT,
            KEY_CAPSLOCK,
            KEY_UP,
            KEY_DOWN,
            KEY_LEFT,
            KEY_RIGHT,
            KEY_DELETE,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_button_codes_distinct() {
        let btns = [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_TOUCH, BTN_STYLUS];
        for i in 0..btns.len() {
            for j in (i + 1)..btns.len() {
                assert_ne!(btns[i], btns[j]);
            }
        }
    }

    #[test]
    fn test_rel_axes_distinct() {
        let axes = [
            REL_X,
            REL_Y,
            REL_Z,
            REL_WHEEL,
            REL_HWHEEL,
            REL_WHEEL_HI_RES,
            REL_HWHEEL_HI_RES,
        ];
        for i in 0..axes.len() {
            for j in (i + 1)..axes.len() {
                assert_ne!(axes[i], axes[j]);
            }
        }
    }

    #[test]
    fn test_abs_axes_distinct() {
        let axes = [
            ABS_X,
            ABS_Y,
            ABS_Z,
            ABS_PRESSURE,
            ABS_MT_SLOT,
            ABS_MT_POSITION_X,
            ABS_MT_POSITION_Y,
            ABS_MT_TRACKING_ID,
        ];
        for i in 0..axes.len() {
            for j in (i + 1)..axes.len() {
                assert_ne!(axes[i], axes[j]);
            }
        }
    }
}

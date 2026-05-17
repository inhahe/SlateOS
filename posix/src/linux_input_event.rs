//! `<linux/input-event-codes.h>` — Input event type and code constants.
//!
//! The Linux input subsystem (evdev) provides a unified interface for
//! all input devices: keyboards, mice, touchscreens, gamepads, etc.
//! Each event has a type (what kind of event), code (which specific
//! event), and value (the event data). Applications read events from
//! /dev/input/eventN devices.

// ---------------------------------------------------------------------------
// Event types (input_event.type)
// ---------------------------------------------------------------------------

/// Synchronization event (marks end of event group).
pub const EV_SYN: u16 = 0x00;
/// Key/button event.
pub const EV_KEY: u16 = 0x01;
/// Relative axis movement (mouse).
pub const EV_REL: u16 = 0x02;
/// Absolute axis position (touchscreen, joystick).
pub const EV_ABS: u16 = 0x03;
/// Miscellaneous event.
pub const EV_MSC: u16 = 0x04;
/// Switch event (lid, headphone jack).
pub const EV_SW: u16 = 0x05;
/// LED event.
pub const EV_LED: u16 = 0x11;
/// Sound event (beep).
pub const EV_SND: u16 = 0x12;
/// Repeat event (autorepeat settings).
pub const EV_REP: u16 = 0x14;
/// Force feedback event.
pub const EV_FF: u16 = 0x15;

// ---------------------------------------------------------------------------
// Synchronization codes
// ---------------------------------------------------------------------------

/// End of event group.
pub const SYN_REPORT: u16 = 0;
/// Configuration change.
pub const SYN_CONFIG: u16 = 1;
/// Multi-touch slot boundary.
pub const SYN_MT_REPORT: u16 = 2;
/// Event buffer overrun.
pub const SYN_DROPPED: u16 = 3;

// ---------------------------------------------------------------------------
// Key codes (subset of common keys)
// ---------------------------------------------------------------------------

/// Escape key.
pub const KEY_ESC: u16 = 1;
/// Enter key.
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
/// F1.
pub const KEY_F1: u16 = 59;

// ---------------------------------------------------------------------------
// Relative axis codes (mouse)
// ---------------------------------------------------------------------------

/// X axis movement.
pub const REL_X: u16 = 0x00;
/// Y axis movement.
pub const REL_Y: u16 = 0x01;
/// Scroll wheel (vertical).
pub const REL_WHEEL: u16 = 0x08;
/// Scroll wheel (horizontal).
pub const REL_HWHEEL: u16 = 0x06;

// ---------------------------------------------------------------------------
// Absolute axis codes (touchscreen/joystick)
// ---------------------------------------------------------------------------

/// X position.
pub const ABS_X: u16 = 0x00;
/// Y position.
pub const ABS_Y: u16 = 0x01;
/// Pressure.
pub const ABS_PRESSURE: u16 = 0x18;
/// Multi-touch slot.
pub const ABS_MT_SLOT: u16 = 0x2F;
/// Multi-touch X position.
pub const ABS_MT_POSITION_X: u16 = 0x35;
/// Multi-touch Y position.
pub const ABS_MT_POSITION_Y: u16 = 0x36;
/// Multi-touch tracking ID.
pub const ABS_MT_TRACKING_ID: u16 = 0x39;

// ---------------------------------------------------------------------------
// Key/button value meanings
// ---------------------------------------------------------------------------

/// Key released.
pub const KEY_STATE_RELEASED: i32 = 0;
/// Key pressed.
pub const KEY_STATE_PRESSED: i32 = 1;
/// Key repeat (autorepeat).
pub const KEY_STATE_REPEAT: i32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct() {
        let types = [
            EV_SYN, EV_KEY, EV_REL, EV_ABS, EV_MSC,
            EV_SW, EV_LED, EV_SND, EV_REP, EV_FF,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_syn_codes_distinct() {
        let codes = [SYN_REPORT, SYN_CONFIG, SYN_MT_REPORT, SYN_DROPPED];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_key_codes_distinct() {
        let keys = [
            KEY_ESC, KEY_BACKSPACE, KEY_TAB, KEY_ENTER, KEY_LEFTCTRL,
            KEY_LEFTSHIFT, KEY_LEFTALT, KEY_SPACE, KEY_CAPSLOCK, KEY_F1,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }

    #[test]
    fn test_key_states_distinct() {
        assert_ne!(KEY_STATE_RELEASED, KEY_STATE_PRESSED);
        assert_ne!(KEY_STATE_PRESSED, KEY_STATE_REPEAT);
        assert_ne!(KEY_STATE_RELEASED, KEY_STATE_REPEAT);
    }

    #[test]
    fn test_ev_syn_is_zero() {
        assert_eq!(EV_SYN, 0);
    }
}

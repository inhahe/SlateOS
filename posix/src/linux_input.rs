//! `<linux/input.h>` — input event definitions.
//!
//! Provides the `InputEvent` structure and event type/code constants
//! for processing raw input events from keyboard, mouse, and other
//! input devices.

// ---------------------------------------------------------------------------
// Input event structure
// ---------------------------------------------------------------------------

/// Raw input event from an event device (`/dev/input/event*`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InputEvent {
    /// Seconds since epoch.
    pub tv_sec: i64,
    /// Microseconds.
    pub tv_usec: i64,
    /// Event type (`EV_*`).
    pub r#type: u16,
    /// Event code (key code, axis, etc.).
    pub code: u16,
    /// Event value (0=release, 1=press, 2=repeat for keys).
    pub value: i32,
}

// ---------------------------------------------------------------------------
// Event types (EV_*)
// ---------------------------------------------------------------------------

/// Synchronization event.
pub const EV_SYN: u16 = 0x00;

/// Key press/release.
pub const EV_KEY: u16 = 0x01;

/// Relative movement (mouse).
pub const EV_REL: u16 = 0x02;

/// Absolute movement (touchscreen, tablet).
pub const EV_ABS: u16 = 0x03;

/// Miscellaneous events.
pub const EV_MSC: u16 = 0x04;

/// Switch event.
pub const EV_SW: u16 = 0x05;

/// LED event.
pub const EV_LED: u16 = 0x11;

/// Sound event.
pub const EV_SND: u16 = 0x12;

/// Auto-repeat.
pub const EV_REP: u16 = 0x14;

/// Force feedback.
pub const EV_FF: u16 = 0x15;

/// Power management.
pub const EV_PWR: u16 = 0x16;

/// Force feedback status.
pub const EV_FF_STATUS: u16 = 0x17;

// ---------------------------------------------------------------------------
// Synchronization codes (SYN_*)
// ---------------------------------------------------------------------------

/// Report end of a single event frame.
pub const SYN_REPORT: u16 = 0;

/// Separate events in the same frame (MT protocol).
pub const SYN_MT_REPORT: u16 = 2;

/// Buffer overrun (events were dropped).
pub const SYN_DROPPED: u16 = 3;

// ---------------------------------------------------------------------------
// Common key codes (KEY_*)
// ---------------------------------------------------------------------------

/// Escape key.
pub const KEY_ESC: u16 = 1;

/// Enter/Return key.
pub const KEY_ENTER: u16 = 28;

/// Backspace key.
pub const KEY_BACKSPACE: u16 = 14;

/// Tab key.
pub const KEY_TAB: u16 = 15;

/// Space bar.
pub const KEY_SPACE: u16 = 57;

/// Left Ctrl.
pub const KEY_LEFTCTRL: u16 = 29;

/// Left Shift.
pub const KEY_LEFTSHIFT: u16 = 42;

/// Right Shift.
pub const KEY_RIGHTSHIFT: u16 = 54;

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

/// Home key.
pub const KEY_HOME: u16 = 102;

/// End key.
pub const KEY_END: u16 = 107;

/// Page Up.
pub const KEY_PAGEUP: u16 = 104;

/// Page Down.
pub const KEY_PAGEDOWN: u16 = 109;

// ---------------------------------------------------------------------------
// Relative axis codes (REL_*)
// ---------------------------------------------------------------------------

/// Relative X axis.
pub const REL_X: u16 = 0x00;

/// Relative Y axis.
pub const REL_Y: u16 = 0x01;

/// Scroll wheel (vertical).
pub const REL_WHEEL: u16 = 0x08;

/// Scroll wheel (horizontal).
pub const REL_HWHEEL: u16 = 0x06;

// ---------------------------------------------------------------------------
// Absolute axis codes (ABS_*)
// ---------------------------------------------------------------------------

/// Absolute X axis.
pub const ABS_X: u16 = 0x00;

/// Absolute Y axis.
pub const ABS_Y: u16 = 0x01;

/// Absolute Z axis.
pub const ABS_Z: u16 = 0x02;

/// Multitouch slot.
pub const ABS_MT_SLOT: u16 = 0x2F;

/// Multitouch touch major axis.
pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;

/// Multitouch position X.
pub const ABS_MT_POSITION_X: u16 = 0x35;

/// Multitouch position Y.
pub const ABS_MT_POSITION_Y: u16 = 0x36;

/// Multitouch tracking ID.
pub const ABS_MT_TRACKING_ID: u16 = 0x39;

// ---------------------------------------------------------------------------
// Mouse button codes (BTN_*)
// ---------------------------------------------------------------------------

/// Left mouse button.
pub const BTN_LEFT: u16 = 0x110;

/// Right mouse button.
pub const BTN_RIGHT: u16 = 0x111;

/// Middle mouse button.
pub const BTN_MIDDLE: u16 = 0x112;

/// Touch contact.
pub const BTN_TOUCH: u16 = 0x14A;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_event_size() {
        // 2 × i64 (16) + u16 + u16 + i32 (8) = 24
        assert_eq!(core::mem::size_of::<InputEvent>(), 24);
    }

    #[test]
    fn test_ev_types_distinct() {
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
                assert_ne!(types[i], types[j], "EV_* types must be distinct");
            }
        }
    }

    #[test]
    fn test_key_codes_distinct() {
        let keys = [
            KEY_ESC,
            KEY_ENTER,
            KEY_BACKSPACE,
            KEY_TAB,
            KEY_SPACE,
            KEY_LEFTCTRL,
            KEY_LEFTSHIFT,
            KEY_RIGHTSHIFT,
            KEY_LEFTALT,
            KEY_CAPSLOCK,
            KEY_UP,
            KEY_DOWN,
            KEY_LEFT,
            KEY_RIGHT,
            KEY_DELETE,
            KEY_HOME,
            KEY_END,
            KEY_PAGEUP,
            KEY_PAGEDOWN,
        ];
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j], "KEY_* codes must be distinct");
            }
        }
    }

    #[test]
    fn test_btn_codes_distinct() {
        let btns = [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_TOUCH];
        for i in 0..btns.len() {
            for j in (i + 1)..btns.len() {
                assert_ne!(btns[i], btns[j]);
            }
        }
    }

    #[test]
    fn test_input_event_init() {
        let ev = InputEvent {
            tv_sec: 1234,
            tv_usec: 5678,
            r#type: EV_KEY,
            code: KEY_ENTER,
            value: 1, // press
        };
        assert_eq!(ev.r#type, EV_KEY);
        assert_eq!(ev.code, KEY_ENTER);
        assert_eq!(ev.value, 1);
    }

    #[test]
    fn test_ev_syn_zero() {
        assert_eq!(EV_SYN, 0);
    }

    #[test]
    fn test_syn_report_zero() {
        assert_eq!(SYN_REPORT, 0);
    }

    #[test]
    fn test_abs_mt_codes_distinct() {
        let codes = [
            ABS_MT_SLOT,
            ABS_MT_TOUCH_MAJOR,
            ABS_MT_POSITION_X,
            ABS_MT_POSITION_Y,
            ABS_MT_TRACKING_ID,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }
}

//! `<linux/input-event-codes.h>` (key/button subset) — keyboard and button codes.
//!
//! Linux input event codes identify individual keys, buttons, and
//! switches. The kernel's input subsystem translates raw scancodes
//! from keyboards, mice, gamepads, and touchscreens into these
//! standardised codes. Userspace (libinput, X11, Wayland) maps them
//! to keysyms and application-level actions.

// ---------------------------------------------------------------------------
// Keyboard keys — common subset
// ---------------------------------------------------------------------------

/// Reserved / unknown key.
pub const KEY_RESERVED: u16 = 0;
/// Escape.
pub const KEY_ESC: u16 = 1;
/// 1 / !
pub const KEY_1: u16 = 2;
/// 2 / @
pub const KEY_2: u16 = 3;
/// 3 / #
pub const KEY_3: u16 = 4;
/// 4 / $
pub const KEY_4: u16 = 5;
/// 5 / %
pub const KEY_5: u16 = 6;
/// 6 / ^
pub const KEY_6: u16 = 7;
/// 7 / &
pub const KEY_7: u16 = 8;
/// 8 / *
pub const KEY_8: u16 = 9;
/// 9 / (
pub const KEY_9: u16 = 10;
/// 0 / )
pub const KEY_0: u16 = 11;
/// - / _
pub const KEY_MINUS: u16 = 12;
/// = / +
pub const KEY_EQUAL: u16 = 13;
/// Backspace.
pub const KEY_BACKSPACE: u16 = 14;
/// Tab.
pub const KEY_TAB: u16 = 15;
/// Q.
pub const KEY_Q: u16 = 16;
/// W.
pub const KEY_W: u16 = 17;
/// E.
pub const KEY_E: u16 = 18;
/// R.
pub const KEY_R: u16 = 19;
/// T.
pub const KEY_T: u16 = 20;
/// Y.
pub const KEY_Y: u16 = 21;
/// U.
pub const KEY_U: u16 = 22;
/// I.
pub const KEY_I: u16 = 23;
/// O.
pub const KEY_O: u16 = 24;
/// P.
pub const KEY_P: u16 = 25;
/// Enter / Return.
pub const KEY_ENTER: u16 = 28;
/// Left Ctrl.
pub const KEY_LEFTCTRL: u16 = 29;
/// A.
pub const KEY_A: u16 = 30;
/// S.
pub const KEY_S: u16 = 31;
/// D.
pub const KEY_D: u16 = 32;
/// F.
pub const KEY_F: u16 = 33;
/// G.
pub const KEY_G: u16 = 34;
/// H.
pub const KEY_H: u16 = 35;
/// J.
pub const KEY_J: u16 = 36;
/// K.
pub const KEY_K: u16 = 37;
/// L.
pub const KEY_L: u16 = 38;
/// Left Shift.
pub const KEY_LEFTSHIFT: u16 = 42;
/// Z.
pub const KEY_Z: u16 = 44;
/// X.
pub const KEY_X: u16 = 45;
/// C.
pub const KEY_C: u16 = 46;
/// V.
pub const KEY_V: u16 = 47;
/// B.
pub const KEY_B: u16 = 48;
/// N.
pub const KEY_N: u16 = 49;
/// M.
pub const KEY_M: u16 = 50;
/// Right Shift.
pub const KEY_RIGHTSHIFT: u16 = 54;
/// Left Alt.
pub const KEY_LEFTALT: u16 = 56;
/// Space bar.
pub const KEY_SPACE: u16 = 57;
/// Caps Lock.
pub const KEY_CAPSLOCK: u16 = 58;

// ---------------------------------------------------------------------------
// Function keys
// ---------------------------------------------------------------------------

/// F1.
pub const KEY_F1: u16 = 59;
/// F2.
pub const KEY_F2: u16 = 60;
/// F3.
pub const KEY_F3: u16 = 61;
/// F4.
pub const KEY_F4: u16 = 62;
/// F5.
pub const KEY_F5: u16 = 63;
/// F6.
pub const KEY_F6: u16 = 64;
/// F7.
pub const KEY_F7: u16 = 65;
/// F8.
pub const KEY_F8: u16 = 66;
/// F9.
pub const KEY_F9: u16 = 67;
/// F10.
pub const KEY_F10: u16 = 68;
/// F11.
pub const KEY_F11: u16 = 87;
/// F12.
pub const KEY_F12: u16 = 88;

// ---------------------------------------------------------------------------
// Navigation keys
// ---------------------------------------------------------------------------

/// Home.
pub const KEY_HOME: u16 = 102;
/// Up arrow.
pub const KEY_UP: u16 = 103;
/// Page Up.
pub const KEY_PAGEUP: u16 = 104;
/// Left arrow.
pub const KEY_LEFT: u16 = 105;
/// Right arrow.
pub const KEY_RIGHT: u16 = 106;
/// End.
pub const KEY_END: u16 = 107;
/// Down arrow.
pub const KEY_DOWN: u16 = 108;
/// Page Down.
pub const KEY_PAGEDOWN: u16 = 109;
/// Insert.
pub const KEY_INSERT: u16 = 110;
/// Delete.
pub const KEY_DELETE: u16 = 111;

// ---------------------------------------------------------------------------
// Modifier keys (additional)
// ---------------------------------------------------------------------------

/// Right Ctrl.
pub const KEY_RIGHTCTRL: u16 = 97;
/// Right Alt (AltGr on international keyboards).
pub const KEY_RIGHTALT: u16 = 100;
/// Left Meta / Super / Windows key.
pub const KEY_LEFTMETA: u16 = 125;
/// Right Meta / Super / Windows key.
pub const KEY_RIGHTMETA: u16 = 126;

// ---------------------------------------------------------------------------
// Mouse buttons (BTN_MOUSE range: 0x110–0x11f)
// ---------------------------------------------------------------------------

/// Left mouse button.
pub const BTN_LEFT: u16 = 0x110;
/// Right mouse button.
pub const BTN_RIGHT: u16 = 0x111;
/// Middle mouse button.
pub const BTN_MIDDLE: u16 = 0x112;
/// Side button (thumb).
pub const BTN_SIDE: u16 = 0x113;
/// Extra button.
pub const BTN_EXTRA: u16 = 0x114;

// ---------------------------------------------------------------------------
// Miscellaneous buttons
// ---------------------------------------------------------------------------

/// Touch contact (touchscreen / touchpad).
pub const BTN_TOUCH: u16 = 0x14A;
/// Stylus (pen) button.
pub const BTN_STYLUS: u16 = 0x14B;
/// Stylus secondary button.
pub const BTN_STYLUS2: u16 = 0x14C;

// ---------------------------------------------------------------------------
// Key ranges
// ---------------------------------------------------------------------------

/// Maximum key code value.
pub const KEY_MAX: u16 = 0x2FF;
/// Number of key codes (KEY_MAX + 1).
pub const KEY_CNT: u16 = 0x300;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_letter_keys_sequential() {
        // Q-P row: Q=16..P=25
        assert_eq!(KEY_Q, 16);
        assert_eq!(KEY_P, 25);
        // A-L row: A=30..L=38
        assert_eq!(KEY_A, 30);
        assert_eq!(KEY_L, 38);
        // Z-M row: Z=44..M=50
        assert_eq!(KEY_Z, 44);
        assert_eq!(KEY_M, 50);
    }

    #[test]
    fn test_number_keys_sequential() {
        assert_eq!(KEY_1, 2);
        assert_eq!(KEY_9, 10);
        assert_eq!(KEY_0, 11);
    }

    #[test]
    fn test_function_keys() {
        // F1-F10 are sequential
        assert_eq!(KEY_F10 - KEY_F1, 9);
        // F11, F12 are at 87, 88
        assert_eq!(KEY_F11, 87);
        assert_eq!(KEY_F12, KEY_F11 + 1);
    }

    #[test]
    fn test_nav_keys_ordered() {
        assert!(KEY_HOME < KEY_UP);
        assert!(KEY_UP < KEY_PAGEUP);
        assert!(KEY_LEFT < KEY_RIGHT);
        assert!(KEY_DOWN < KEY_PAGEDOWN);
        assert!(KEY_INSERT < KEY_DELETE);
    }

    #[test]
    fn test_mouse_buttons_sequential() {
        assert_eq!(BTN_LEFT, 0x110);
        assert_eq!(BTN_RIGHT, BTN_LEFT + 1);
        assert_eq!(BTN_MIDDLE, BTN_LEFT + 2);
        assert_eq!(BTN_SIDE, BTN_LEFT + 3);
        assert_eq!(BTN_EXTRA, BTN_LEFT + 4);
    }

    #[test]
    fn test_modifier_pairs() {
        assert_ne!(KEY_LEFTCTRL, KEY_RIGHTCTRL);
        assert_ne!(KEY_LEFTALT, KEY_RIGHTALT);
        assert_ne!(KEY_LEFTSHIFT, KEY_RIGHTSHIFT);
        assert_ne!(KEY_LEFTMETA, KEY_RIGHTMETA);
    }

    #[test]
    fn test_key_cnt() {
        assert_eq!(KEY_CNT, KEY_MAX + 1);
    }
}

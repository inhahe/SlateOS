//! `<linux/kd.h>` — Console (keyboard/display) ioctl constants.
//!
//! These ioctls control the Linux console driver: keyboard mode,
//! LED state, font loading, and text/graphics mode switching.

// ---------------------------------------------------------------------------
// Console mode ioctls
// ---------------------------------------------------------------------------

/// Get keyboard mode.
pub const KDGKBMODE: u32 = 0x4B44;
/// Set keyboard mode.
pub const KDSKBMODE: u32 = 0x4B45;
/// Get keyboard type.
pub const KDGKBTYPE: u32 = 0x4B33;
/// Get LED state.
pub const KDGETLED: u32 = 0x4B31;
/// Set LED state.
pub const KDSETLED: u32 = 0x4B32;
/// Set display mode (text/graphics).
pub const KDSETMODE: u32 = 0x4B3A;
/// Get display mode.
pub const KDGETMODE: u32 = 0x4B3B;
/// Signal on console switch.
pub const KDSIGACCEPT: u32 = 0x4B4E;

// ---------------------------------------------------------------------------
// Keyboard modes (for KDSKBMODE)
// ---------------------------------------------------------------------------

/// RAW mode (scancodes).
pub const K_RAW: u32 = 0;
/// XLATE mode (ASCII/keymap translation).
pub const K_XLATE: u32 = 1;
/// MEDIUMRAW mode (keycodes).
pub const K_MEDIUMRAW: u32 = 2;
/// UNICODE mode (UTF-8).
pub const K_UNICODE: u32 = 3;
/// OFF mode (keyboard disabled).
pub const K_OFF: u32 = 4;

// ---------------------------------------------------------------------------
// Display modes (for KDSETMODE)
// ---------------------------------------------------------------------------

/// Text mode.
pub const KD_TEXT: u32 = 0;
/// Graphics mode.
pub const KD_GRAPHICS: u32 = 1;
/// Text mode with Unicode.
pub const KD_TEXT0: u32 = 2;
/// Text mode alternate.
pub const KD_TEXT1: u32 = 3;

// ---------------------------------------------------------------------------
// LED flags
// ---------------------------------------------------------------------------

/// Scroll Lock LED.
pub const LED_SCR: u8 = 0x01;
/// Num Lock LED.
pub const LED_NUM: u8 = 0x02;
/// Caps Lock LED.
pub const LED_CAP: u8 = 0x04;

// ---------------------------------------------------------------------------
// Keyboard types
// ---------------------------------------------------------------------------

/// XT keyboard.
pub const KB_84: u8 = 0x01;
/// AT keyboard (101/102 key).
pub const KB_101: u8 = 0x02;
/// Other keyboard type.
pub const KB_OTHER: u8 = 0x03;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            KDGKBMODE,
            KDSKBMODE,
            KDGKBTYPE,
            KDGETLED,
            KDSETLED,
            KDSETMODE,
            KDGETMODE,
            KDSIGACCEPT,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_kb_modes_distinct() {
        let modes = [K_RAW, K_XLATE, K_MEDIUMRAW, K_UNICODE, K_OFF];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_display_modes_distinct() {
        let modes = [KD_TEXT, KD_GRAPHICS, KD_TEXT0, KD_TEXT1];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_led_flags_no_overlap() {
        let flags = [LED_SCR, LED_NUM, LED_CAP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_kb_types_distinct() {
        let types = [KB_84, KB_101, KB_OTHER];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_raw_is_zero() {
        assert_eq!(K_RAW, 0);
    }
}

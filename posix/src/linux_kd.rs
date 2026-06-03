//! `<linux/kd.h>` — console keyboard/display control.
//!
//! Provides ioctl constants for controlling the Linux virtual console
//! (keyboard mode, LED state, display mode, etc.).

// ---------------------------------------------------------------------------
// Keyboard mode (KDGKBMODE / KDSKBMODE)
// ---------------------------------------------------------------------------

/// Scancode mode.
pub const K_RAW: i32 = 0x00;
/// Keycode + translate through keymap.
pub const K_XLATE: i32 = 0x01;
/// Medium-raw (scancode + keycode).
pub const K_MEDIUMRAW: i32 = 0x02;
/// UTF-8 mode.
pub const K_UNICODE: i32 = 0x03;
/// Turn keyboard off.
pub const K_OFF: i32 = 0x04;

// ---------------------------------------------------------------------------
// Display mode (KDGETMODE / KDSETMODE)
// ---------------------------------------------------------------------------

/// Text mode.
pub const KD_TEXT: i32 = 0x00;
/// Graphics mode.
pub const KD_GRAPHICS: i32 = 0x01;
/// Text + graphics mode (rare).
pub const KD_TEXT0: i32 = 0x02;
/// Text + graphics mode (rare).
pub const KD_TEXT1: i32 = 0x03;

// ---------------------------------------------------------------------------
// LED flags (KDGETLED / KDSETLED)
// ---------------------------------------------------------------------------

/// Scroll Lock LED.
pub const LED_SCR: u8 = 0x01;
/// Num Lock LED.
pub const LED_NUM: u8 = 0x02;
/// Caps Lock LED.
pub const LED_CAP: u8 = 0x04;

// ---------------------------------------------------------------------------
// Ioctl commands
// ---------------------------------------------------------------------------

/// Get keyboard mode.
pub const KDGKBMODE: u64 = 0x4B44;
/// Set keyboard mode.
pub const KDSKBMODE: u64 = 0x4B45;
/// Get display mode.
pub const KDGETMODE: u64 = 0x4B3B;
/// Set display mode.
pub const KDSETMODE: u64 = 0x4B3A;
/// Get LED state.
pub const KDGETLED: u64 = 0x4B31;
/// Set LED state.
pub const KDSETLED: u64 = 0x4B32;
/// Beep.
pub const KIOCSOUND: u64 = 0x4B2F;
/// Set tone.
pub const KDMKTONE: u64 = 0x4B30;
/// Get keyboard type.
pub const KDGKBTYPE: u64 = 0x4B33;
/// Get key entry.
pub const KDGKBENT: u64 = 0x4B46;
/// Set key entry.
pub const KDSKBENT: u64 = 0x4B47;
/// Get string entry.
pub const KDGKBSENT: u64 = 0x4B48;
/// Set string entry.
pub const KDSKBSENT: u64 = 0x4B49;
/// Get keyboard meta mode.
pub const KDGKBMETA: u64 = 0x4B62;
/// Set keyboard meta mode.
pub const KDSKBMETA: u64 = 0x4B63;

// ---------------------------------------------------------------------------
// Keyboard types (returned by KDGKBTYPE)
// ---------------------------------------------------------------------------

/// XT keyboard.
pub const KB_84: u8 = 0x01;
/// AT keyboard.
pub const KB_101: u8 = 0x02;
/// Other/generic keyboard.
pub const KB_OTHER: u8 = 0x03;

// ---------------------------------------------------------------------------
// Meta key modes (KDGKBMETA / KDSKBMETA)
// ---------------------------------------------------------------------------

/// Meta key sets high bit.
pub const K_METABIT: i32 = 0x03;
/// Meta key sends ESC prefix.
pub const K_ESCPREFIX: i32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyboard_modes() {
        assert_eq!(K_RAW, 0);
        assert_eq!(K_XLATE, 1);
        assert_eq!(K_MEDIUMRAW, 2);
        assert_eq!(K_UNICODE, 3);
        assert_eq!(K_OFF, 4);
    }

    #[test]
    fn test_display_modes() {
        assert_eq!(KD_TEXT, 0);
        assert_eq!(KD_GRAPHICS, 1);
        assert_ne!(KD_TEXT, KD_GRAPHICS);
    }

    #[test]
    fn test_led_flags_are_bits() {
        assert_eq!(LED_SCR & LED_NUM, 0);
        assert_eq!(LED_NUM & LED_CAP, 0);
        assert_eq!(LED_SCR & LED_CAP, 0);
        let all = LED_SCR | LED_NUM | LED_CAP;
        assert_eq!(all, 7);
    }

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            KDGKBMODE, KDSKBMODE, KDGETMODE, KDSETMODE, KDGETLED, KDSETLED, KIOCSOUND, KDMKTONE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_keyboard_types() {
        assert_ne!(KB_84, KB_101);
        assert_ne!(KB_101, KB_OTHER);
    }

    #[test]
    fn test_meta_modes() {
        assert_ne!(K_METABIT, K_ESCPREFIX);
    }
}

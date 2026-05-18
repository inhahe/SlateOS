//! `<linux/kd.h>` — Additional console/keyboard constants.
//!
//! Supplementary KD constants covering keyboard modes,
//! LED states, console modes, and font operations.

// ---------------------------------------------------------------------------
// KD keyboard modes
// ---------------------------------------------------------------------------

/// Raw scancode mode.
pub const K_RAW: u32 = 0x00;
/// Xlate (cooked) mode.
pub const K_XLATE: u32 = 0x01;
/// Medium-raw mode.
pub const K_MEDIUMRAW: u32 = 0x02;
/// Unicode mode.
pub const K_UNICODE: u32 = 0x03;
/// Off mode.
pub const K_OFF: u32 = 0x04;

// ---------------------------------------------------------------------------
// KD LED flags
// ---------------------------------------------------------------------------

/// Scroll Lock LED.
pub const LED_SCR: u8 = 0x01;
/// Num Lock LED.
pub const LED_NUM: u8 = 0x02;
/// Caps Lock LED.
pub const LED_CAP: u8 = 0x04;

// ---------------------------------------------------------------------------
// KD console modes
// ---------------------------------------------------------------------------

/// Text mode.
pub const KD_TEXT: u32 = 0x00;
/// Graphics mode.
pub const KD_GRAPHICS: u32 = 0x01;
/// Text0 mode.
pub const KD_TEXT0: u32 = 0x02;
/// Text1 mode.
pub const KD_TEXT1: u32 = 0x03;

// ---------------------------------------------------------------------------
// KD ioctl commands
// ---------------------------------------------------------------------------

/// Set keyboard mode.
pub const KDSKBMODE: u32 = 0x4B45;
/// Get keyboard mode.
pub const KDGKBMODE: u32 = 0x4B44;
/// Set LED state.
pub const KDSETLED: u32 = 0x4B32;
/// Get LED state.
pub const KDGETLED: u32 = 0x4B31;
/// Set console mode.
pub const KDSETMODE: u32 = 0x4B3A;
/// Get console mode.
pub const KDGETMODE: u32 = 0x4B3B;
/// Set keyboard map entry.
pub const KDSKBENT: u32 = 0x4B47;
/// Get keyboard map entry.
pub const KDGKBENT: u32 = 0x4B46;
/// Set string entry.
pub const KDSKBSENT: u32 = 0x4B49;
/// Get string entry.
pub const KDGKBSENT: u32 = 0x4B48;
/// Set keyboard meta.
pub const KDSKBMETA: u32 = 0x4B63;
/// Get keyboard meta.
pub const KDGKBMETA: u32 = 0x4B62;
/// Set keyboard type.
pub const KDSKBDIACR: u32 = 0x4B4A;
/// Get keyboard type.
pub const KDGKBDIACR: u32 = 0x4B4B;
/// Make sound.
pub const KDMKTONE: u32 = 0x4B30;
/// Set beep.
pub const KIOCSOUND: u32 = 0x4B2F;

// ---------------------------------------------------------------------------
// KD meta modes
// ---------------------------------------------------------------------------

/// Meta generates escape.
pub const K_METABIT: u32 = 0x03;
/// Meta sets high bit.
pub const K_ESCPREFIX: u32 = 0x04;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_led_flags_power_of_two() {
        let leds: [u8; 3] = [LED_SCR, LED_NUM, LED_CAP];
        for l in &leds {
            assert!(l.is_power_of_two(), "0x{:02x} not power of two", l);
        }
    }

    #[test]
    fn test_led_flags_no_overlap() {
        let leds: [u8; 3] = [LED_SCR, LED_NUM, LED_CAP];
        for i in 0..leds.len() {
            for j in (i + 1)..leds.len() {
                assert_eq!(leds[i] & leds[j], 0);
            }
        }
    }

    #[test]
    fn test_console_modes_distinct() {
        let modes = [KD_TEXT, KD_GRAPHICS, KD_TEXT0, KD_TEXT1];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_ioctl_distinct() {
        let cmds = [
            KDSKBMODE, KDGKBMODE, KDSETLED, KDGETLED,
            KDSETMODE, KDGETMODE, KDSKBENT, KDGKBENT,
            KDSKBSENT, KDGKBSENT, KDSKBMETA, KDGKBMETA,
            KDSKBDIACR, KDGKBDIACR, KDMKTONE, KIOCSOUND,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_meta_modes() {
        assert_ne!(K_METABIT, K_ESCPREFIX);
    }
}

//! `<linux/input-event-codes.h>` (LED subset) — LED indicator codes.
//!
//! LED events control indicator lights on keyboards, mice, and other
//! input devices. The kernel's input layer maps the logical LED state
//! (Num Lock, Caps Lock, etc.) to the physical LEDs on each attached
//! device. Userspace can also drive LEDs directly via `EV_LED`
//! events.

// ---------------------------------------------------------------------------
// LED codes
// ---------------------------------------------------------------------------

/// Num Lock indicator.
pub const LED_NUML: u16 = 0x00;
/// Caps Lock indicator.
pub const LED_CAPSL: u16 = 0x01;
/// Scroll Lock indicator.
pub const LED_SCROLLL: u16 = 0x02;
/// Compose mode indicator.
pub const LED_COMPOSE: u16 = 0x03;
/// Kana mode indicator.
pub const LED_KANA: u16 = 0x04;
/// Suspend (sleep) indicator.
pub const LED_SLEEP: u16 = 0x05;
/// Suspend / standby indicator.
pub const LED_SUSPEND: u16 = 0x06;
/// Mute indicator.
pub const LED_MUTE: u16 = 0x07;
/// Miscellaneous LED.
pub const LED_MISC: u16 = 0x08;
/// Mail notification LED.
pub const LED_MAIL: u16 = 0x09;
/// Charging indicator LED.
pub const LED_CHARGING: u16 = 0x0A;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum LED code.
pub const LED_MAX: u16 = 0x0F;
/// Number of LED codes (LED_MAX + 1).
pub const LED_CNT: u16 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_led_codes_distinct() {
        let leds = [
            LED_NUML,
            LED_CAPSL,
            LED_SCROLLL,
            LED_COMPOSE,
            LED_KANA,
            LED_SLEEP,
            LED_SUSPEND,
            LED_MUTE,
            LED_MISC,
            LED_MAIL,
            LED_CHARGING,
        ];
        for i in 0..leds.len() {
            for j in (i + 1)..leds.len() {
                assert_ne!(leds[i], leds[j], "LED codes {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn test_keyboard_leds_first() {
        // Num/Caps/Scroll Lock are the classic trio, codes 0-2
        assert_eq!(LED_NUML, 0);
        assert_eq!(LED_CAPSL, 1);
        assert_eq!(LED_SCROLLL, 2);
    }

    #[test]
    fn test_led_codes_sequential() {
        assert_eq!(LED_COMPOSE, LED_SCROLLL + 1);
        assert_eq!(LED_KANA, LED_COMPOSE + 1);
        assert_eq!(LED_SLEEP, LED_KANA + 1);
        assert_eq!(LED_SUSPEND, LED_SLEEP + 1);
        assert_eq!(LED_MUTE, LED_SUSPEND + 1);
    }

    #[test]
    fn test_all_within_max() {
        let leds = [
            LED_NUML,
            LED_CAPSL,
            LED_SCROLLL,
            LED_COMPOSE,
            LED_KANA,
            LED_SLEEP,
            LED_SUSPEND,
            LED_MUTE,
            LED_MISC,
            LED_MAIL,
            LED_CHARGING,
        ];
        for &l in &leds {
            assert!(l <= LED_MAX, "LED 0x{:02X} exceeds LED_MAX", l);
        }
    }

    #[test]
    fn test_led_cnt() {
        assert_eq!(LED_CNT, LED_MAX + 1);
    }
}

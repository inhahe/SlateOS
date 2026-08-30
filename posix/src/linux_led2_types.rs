//! `<linux/leds.h>` — Additional LED subsystem constants.
//!
//! Supplementary LED constants covering brightness values,
//! trigger types, and flash modes.

// ---------------------------------------------------------------------------
// LED brightness values
// ---------------------------------------------------------------------------

/// LED off.
pub const LED_OFF: u32 = 0;
/// Half brightness.
pub const LED_HALF: u32 = 127;
/// Full brightness.
pub const LED_FULL: u32 = 255;

// ---------------------------------------------------------------------------
// LED flash modes
// ---------------------------------------------------------------------------

/// No flash.
pub const LED_FLASH_NONE: u32 = 0;
/// Torch mode (continuous).
pub const LED_FLASH_TORCH: u32 = 1;
/// Flash mode (single shot).
pub const LED_FLASH_FLASH: u32 = 2;

// ---------------------------------------------------------------------------
// LED flags
// ---------------------------------------------------------------------------

/// LED supports hardware blink.
pub const LED_HW_PLUGGABLE: u32 = 1 << 0;
/// LED is unregistering.
pub const LED_UNREGISTERING: u32 = 1 << 1;
/// LED brightness set blocking.
pub const LED_BRIGHTNESS_FAST: u32 = 1 << 2;
/// LED blink disable.
pub const LED_BLINK_DISABLE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// LED function indices (standard function names)
// ---------------------------------------------------------------------------

/// Activity indicator.
pub const LED_FUNCTION_ACTIVITY: u32 = 0;
/// Backlight.
pub const LED_FUNCTION_BACKLIGHT: u32 = 1;
/// Bluetooth.
pub const LED_FUNCTION_BLUETOOTH: u32 = 2;
/// Boot indicator.
pub const LED_FUNCTION_BOOT: u32 = 3;
/// Caps lock.
pub const LED_FUNCTION_CAPSLOCK: u32 = 4;
/// Disk activity.
pub const LED_FUNCTION_DISK: u32 = 5;
/// Flash.
pub const LED_FUNCTION_FLASH: u32 = 6;
/// Heartbeat.
pub const LED_FUNCTION_HEARTBEAT: u32 = 7;
/// Keyboard backlight.
pub const LED_FUNCTION_KBD_BACKLIGHT: u32 = 8;
/// LAN indicator.
pub const LED_FUNCTION_LAN: u32 = 9;
/// Mute indicator.
pub const LED_FUNCTION_MUTE: u32 = 10;
/// Num lock.
pub const LED_FUNCTION_NUMLOCK: u32 = 11;
/// Power indicator.
pub const LED_FUNCTION_POWER: u32 = 12;
/// Scroll lock.
pub const LED_FUNCTION_SCROLLLOCK: u32 = 13;
/// Status indicator.
pub const LED_FUNCTION_STATUS: u32 = 14;
/// Torch.
pub const LED_FUNCTION_TORCH: u32 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brightness_ordered() {
        assert!(LED_OFF < LED_HALF);
        assert!(LED_HALF < LED_FULL);
    }

    #[test]
    fn test_brightness_values() {
        assert_eq!(LED_OFF, 0);
        assert_eq!(LED_FULL, 255);
    }

    #[test]
    fn test_flash_modes_distinct() {
        let modes = [LED_FLASH_NONE, LED_FLASH_TORCH, LED_FLASH_FLASH];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            LED_HW_PLUGGABLE,
            LED_UNREGISTERING,
            LED_BRIGHTNESS_FAST,
            LED_BLINK_DISABLE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_functions_distinct() {
        let funcs = [
            LED_FUNCTION_ACTIVITY,
            LED_FUNCTION_BACKLIGHT,
            LED_FUNCTION_BLUETOOTH,
            LED_FUNCTION_BOOT,
            LED_FUNCTION_CAPSLOCK,
            LED_FUNCTION_DISK,
            LED_FUNCTION_FLASH,
            LED_FUNCTION_HEARTBEAT,
            LED_FUNCTION_KBD_BACKLIGHT,
            LED_FUNCTION_LAN,
            LED_FUNCTION_MUTE,
            LED_FUNCTION_NUMLOCK,
            LED_FUNCTION_POWER,
            LED_FUNCTION_SCROLLLOCK,
            LED_FUNCTION_STATUS,
            LED_FUNCTION_TORCH,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }
}

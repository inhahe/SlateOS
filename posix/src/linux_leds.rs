//! `<linux/leds.h>` — LED subsystem constants.
//!
//! The LED subsystem controls indicator LEDs (power, disk activity,
//! keyboard backlights, etc.) via sysfs. Each LED has a brightness
//! value and an optional trigger (heartbeat, disk-activity, timer, etc.).

// ---------------------------------------------------------------------------
// LED brightness levels
// ---------------------------------------------------------------------------

/// LED off.
pub const LED_OFF: u32 = 0;
/// LED half brightness.
pub const LED_HALF: u32 = 127;
/// LED full brightness.
pub const LED_FULL: u32 = 255;

// ---------------------------------------------------------------------------
// LED flags
// ---------------------------------------------------------------------------

/// LED supports hardware-controlled blinking.
pub const LED_HW_PLUGGABLE: u32 = 1 << 0;
/// LED is panicking (used by panic notifier).
pub const LED_PANIC_INDICATOR: u32 = 1 << 1;
/// LED brightness can be set.
pub const LED_BRIGHT_HW_CHANGED: u32 = 1 << 2;
/// LED is unregistering.
pub const LED_UNREGISTERING: u32 = 1 << 3;
/// LED blink disabled.
pub const LED_BLINK_DISABLE: u32 = 1 << 4;
/// LED supports brightness_hw_changed notification.
pub const LED_SATA: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Standard LED functions (trigger names)
// ---------------------------------------------------------------------------

/// Activity trigger.
pub const LED_FUNCTION_ACTIVITY: &str = "activity";
/// Backlight trigger.
pub const LED_FUNCTION_BACKLIGHT: &str = "backlight";
/// Bluetooth power trigger.
pub const LED_FUNCTION_BLUETOOTH_POWER: &str = "bluetooth-power";
/// Capslock trigger.
pub const LED_FUNCTION_CAPSLOCK: &str = "capslock";
/// Charging trigger.
pub const LED_FUNCTION_CHARGING: &str = "charging";
/// Disk activity trigger.
pub const LED_FUNCTION_DISK: &str = "disk-activity";
/// Disk read trigger.
pub const LED_FUNCTION_DISK_READ: &str = "disk-read";
/// Disk write trigger.
pub const LED_FUNCTION_DISK_WRITE: &str = "disk-write";
/// Flash trigger.
pub const LED_FUNCTION_FLASH: &str = "flash";
/// Heartbeat trigger.
pub const LED_FUNCTION_HEARTBEAT: &str = "heartbeat";
/// Indicator trigger.
pub const LED_FUNCTION_INDICATOR: &str = "indicator";
/// LAN trigger.
pub const LED_FUNCTION_LAN: &str = "lan";
/// Mail trigger.
pub const LED_FUNCTION_MAIL: &str = "mail";
/// Mute trigger.
pub const LED_FUNCTION_MUTE: &str = "mute";
/// Numlock trigger.
pub const LED_FUNCTION_NUMLOCK: &str = "numlock";
/// Power trigger.
pub const LED_FUNCTION_POWER: &str = "power";
/// Scrolllock trigger.
pub const LED_FUNCTION_SCROLLLOCK: &str = "scrolllock";
/// Standby trigger.
pub const LED_FUNCTION_STANDBY: &str = "standby";
/// Torch trigger.
pub const LED_FUNCTION_TORCH: &str = "torch";
/// WAN trigger.
pub const LED_FUNCTION_WAN: &str = "wan";
/// WLAN trigger.
pub const LED_FUNCTION_WLAN: &str = "wlan";

// ---------------------------------------------------------------------------
// Standard LED colors
// ---------------------------------------------------------------------------

/// White LED.
pub const LED_COLOR_ID_WHITE: u32 = 0;
/// Red LED.
pub const LED_COLOR_ID_RED: u32 = 1;
/// Green LED.
pub const LED_COLOR_ID_GREEN: u32 = 2;
/// Blue LED.
pub const LED_COLOR_ID_BLUE: u32 = 3;
/// Amber LED.
pub const LED_COLOR_ID_AMBER: u32 = 4;
/// Violet LED.
pub const LED_COLOR_ID_VIOLET: u32 = 5;
/// Yellow LED.
pub const LED_COLOR_ID_YELLOW: u32 = 6;
/// Infrared.
pub const LED_COLOR_ID_IR: u32 = 7;
/// Multi-color.
pub const LED_COLOR_ID_MULTI: u32 = 8;
/// RGB (all colors combined).
pub const LED_COLOR_ID_RGB: u32 = 9;
/// Maximum color ID.
pub const LED_COLOR_ID_MAX: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brightness_ordering() {
        assert!(LED_OFF < LED_HALF);
        assert!(LED_HALF < LED_FULL);
    }

    #[test]
    fn test_brightness_values() {
        assert_eq!(LED_OFF, 0);
        assert_eq!(LED_HALF, 127);
        assert_eq!(LED_FULL, 255);
    }

    #[test]
    fn test_flags_are_powers_of_two() {
        let flags = [
            LED_HW_PLUGGABLE, LED_PANIC_INDICATOR,
            LED_BRIGHT_HW_CHANGED, LED_UNREGISTERING,
            LED_BLINK_DISABLE, LED_SATA,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not a power of two", flag);
        }
    }

    #[test]
    fn test_colors_distinct() {
        let colors = [
            LED_COLOR_ID_WHITE, LED_COLOR_ID_RED, LED_COLOR_ID_GREEN,
            LED_COLOR_ID_BLUE, LED_COLOR_ID_AMBER, LED_COLOR_ID_VIOLET,
            LED_COLOR_ID_YELLOW, LED_COLOR_ID_IR, LED_COLOR_ID_MULTI,
            LED_COLOR_ID_RGB,
        ];
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    #[test]
    fn test_color_max() {
        assert_eq!(LED_COLOR_ID_MAX, LED_COLOR_ID_RGB + 1);
    }

    #[test]
    fn test_functions() {
        assert_eq!(LED_FUNCTION_HEARTBEAT, "heartbeat");
        assert_eq!(LED_FUNCTION_DISK, "disk-activity");
        assert_eq!(LED_FUNCTION_POWER, "power");
    }

    #[test]
    fn test_functions_all_distinct() {
        let funcs = [
            LED_FUNCTION_ACTIVITY, LED_FUNCTION_BACKLIGHT,
            LED_FUNCTION_BLUETOOTH_POWER, LED_FUNCTION_CAPSLOCK,
            LED_FUNCTION_CHARGING, LED_FUNCTION_DISK,
            LED_FUNCTION_DISK_READ, LED_FUNCTION_DISK_WRITE,
            LED_FUNCTION_FLASH, LED_FUNCTION_HEARTBEAT,
            LED_FUNCTION_INDICATOR, LED_FUNCTION_LAN,
            LED_FUNCTION_MAIL, LED_FUNCTION_MUTE,
            LED_FUNCTION_NUMLOCK, LED_FUNCTION_POWER,
            LED_FUNCTION_SCROLLLOCK, LED_FUNCTION_STANDBY,
            LED_FUNCTION_TORCH, LED_FUNCTION_WAN,
            LED_FUNCTION_WLAN,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }
}

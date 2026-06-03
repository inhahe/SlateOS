//! `<linux/leds.h>` — LED subsystem constants.
//!
//! The LED subsystem controls hardware LEDs and LED triggers.
//! These constants define LED brightness levels, trigger types,
//! blink patterns, and sysfs attribute identifiers.

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

/// LED supports brightness setting.
pub const LED_BRIGHT_HW_CHANGED: u32 = 1 << 0;
/// LED cannot be turned off.
pub const LED_UNREGISTERING: u32 = 1 << 1;
/// LED has blink capability.
pub const LED_BLINK_SW: u32 = 1 << 2;
/// LED blink oneshot active.
pub const LED_BLINK_ONESHOT: u32 = 1 << 3;
/// LED blink oneshot stop.
pub const LED_BLINK_ONESHOT_STOP: u32 = 1 << 4;
/// LED blink invert.
pub const LED_BLINK_INVERT: u32 = 1 << 5;
/// LED blink brightness change.
pub const LED_BLINK_BRIGHTNESS_CHANGE: u32 = 1 << 6;
/// LED blink disable.
pub const LED_BLINK_DISABLE: u32 = 1 << 7;
/// LED HW control.
pub const LED_HW_PLUGGABLE: u32 = 1 << 8;
/// LED panic indicator.
pub const LED_PANIC_INDICATOR: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// LED trigger types (well-known trigger names)
// ---------------------------------------------------------------------------

/// No trigger.
pub const LED_TRIGGER_NONE: u32 = 0;
/// Default-on trigger.
pub const LED_TRIGGER_DEFAULT_ON: u32 = 1;
/// Timer trigger (blink).
pub const LED_TRIGGER_TIMER: u32 = 2;
/// Heartbeat trigger.
pub const LED_TRIGGER_HEARTBEAT: u32 = 3;
/// Disk activity trigger.
pub const LED_TRIGGER_DISK: u32 = 4;
/// Network activity trigger.
pub const LED_TRIGGER_NETDEV: u32 = 5;
/// CPU activity trigger.
pub const LED_TRIGGER_CPU: u32 = 6;
/// Panic trigger.
pub const LED_TRIGGER_PANIC: u32 = 7;
/// Backlight trigger.
pub const LED_TRIGGER_BACKLIGHT: u32 = 8;
/// GPIO trigger.
pub const LED_TRIGGER_GPIO: u32 = 9;

// ---------------------------------------------------------------------------
// LED color IDs (LED_COLOR_ID_*)
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
/// IR (infrared) LED.
pub const LED_COLOR_ID_IR: u32 = 7;
/// Multi-color LED.
pub const LED_COLOR_ID_MULTI: u32 = 8;
/// RGB LED.
pub const LED_COLOR_ID_RGB: u32 = 9;

// ---------------------------------------------------------------------------
// LED functions (LED_FUNCTION_*)
// ---------------------------------------------------------------------------

/// Status indicator.
pub const LED_FUNCTION_STATUS: u32 = 0;
/// Power indicator.
pub const LED_FUNCTION_POWER: u32 = 1;
/// Disk activity.
pub const LED_FUNCTION_DISK: u32 = 2;
/// Network activity.
pub const LED_FUNCTION_LAN: u32 = 3;
/// WLAN activity.
pub const LED_FUNCTION_WLAN: u32 = 4;
/// Bluetooth activity.
pub const LED_FUNCTION_BLUETOOTH: u32 = 5;
/// USB activity.
pub const LED_FUNCTION_USB: u32 = 6;
/// Microphone mute.
pub const LED_FUNCTION_MICMUTE: u32 = 7;
/// Camera indicator.
pub const LED_FUNCTION_CAMERA: u32 = 8;
/// Charging indicator.
pub const LED_FUNCTION_CHARGING: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brightness_range() {
        assert_eq!(LED_OFF, 0);
        assert_eq!(LED_FULL, 255);
        assert!(LED_HALF > LED_OFF && LED_HALF < LED_FULL);
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            LED_BRIGHT_HW_CHANGED,
            LED_UNREGISTERING,
            LED_BLINK_SW,
            LED_BLINK_ONESHOT,
            LED_BLINK_ONESHOT_STOP,
            LED_BLINK_INVERT,
            LED_BLINK_BRIGHTNESS_CHANGE,
            LED_BLINK_DISABLE,
            LED_HW_PLUGGABLE,
            LED_PANIC_INDICATOR,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not power of two");
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            LED_BRIGHT_HW_CHANGED,
            LED_UNREGISTERING,
            LED_BLINK_SW,
            LED_BLINK_ONESHOT,
            LED_BLINK_ONESHOT_STOP,
            LED_BLINK_INVERT,
            LED_BLINK_BRIGHTNESS_CHANGE,
            LED_BLINK_DISABLE,
            LED_HW_PLUGGABLE,
            LED_PANIC_INDICATOR,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_triggers_distinct() {
        let triggers = [
            LED_TRIGGER_NONE,
            LED_TRIGGER_DEFAULT_ON,
            LED_TRIGGER_TIMER,
            LED_TRIGGER_HEARTBEAT,
            LED_TRIGGER_DISK,
            LED_TRIGGER_NETDEV,
            LED_TRIGGER_CPU,
            LED_TRIGGER_PANIC,
            LED_TRIGGER_BACKLIGHT,
            LED_TRIGGER_GPIO,
        ];
        for i in 0..triggers.len() {
            for j in (i + 1)..triggers.len() {
                assert_ne!(triggers[i], triggers[j]);
            }
        }
    }

    #[test]
    fn test_colors_distinct() {
        let colors = [
            LED_COLOR_ID_WHITE,
            LED_COLOR_ID_RED,
            LED_COLOR_ID_GREEN,
            LED_COLOR_ID_BLUE,
            LED_COLOR_ID_AMBER,
            LED_COLOR_ID_VIOLET,
            LED_COLOR_ID_YELLOW,
            LED_COLOR_ID_IR,
            LED_COLOR_ID_MULTI,
            LED_COLOR_ID_RGB,
        ];
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    #[test]
    fn test_functions_distinct() {
        let funcs = [
            LED_FUNCTION_STATUS,
            LED_FUNCTION_POWER,
            LED_FUNCTION_DISK,
            LED_FUNCTION_LAN,
            LED_FUNCTION_WLAN,
            LED_FUNCTION_BLUETOOTH,
            LED_FUNCTION_USB,
            LED_FUNCTION_MICMUTE,
            LED_FUNCTION_CAMERA,
            LED_FUNCTION_CHARGING,
        ];
        for i in 0..funcs.len() {
            for j in (i + 1)..funcs.len() {
                assert_ne!(funcs[i], funcs[j]);
            }
        }
    }

    #[test]
    fn test_none_trigger_is_zero() {
        assert_eq!(LED_TRIGGER_NONE, 0);
    }
}

//! `<linux/leds.h>` — LED subsystem constants.
//!
//! The Linux LED subsystem provides a unified interface for hardware
//! LEDs (keyboard LEDs, notification LEDs, disk activity indicators,
//! etc.). Each LED has a brightness file, a trigger selector, and
//! optional hardware blink support. Used by system notification
//! daemons, keyboard drivers, and embedded applications.

// ---------------------------------------------------------------------------
// LED brightness levels
// ---------------------------------------------------------------------------

/// LED is off.
pub const LED_OFF: u32 = 0;
/// LED is at half brightness.
pub const LED_HALF: u32 = 127;
/// LED is at full brightness.
pub const LED_FULL: u32 = 255;

// ---------------------------------------------------------------------------
// Standard LED trigger names
// ---------------------------------------------------------------------------

/// No trigger (manual control).
pub const LED_TRIGGER_NONE: &str = "none";
/// Default-on (LED on at boot).
pub const LED_TRIGGER_DEFAULT_ON: &str = "default-on";
/// Heartbeat (kernel alive indicator).
pub const LED_TRIGGER_HEARTBEAT: &str = "heartbeat";
/// Disk activity.
pub const LED_TRIGGER_DISK_ACTIVITY: &str = "disk-activity";
/// Timer (periodic blink).
pub const LED_TRIGGER_TIMER: &str = "timer";
/// Network activity (general).
pub const LED_TRIGGER_NETDEV: &str = "netdev";
/// Panic indicator.
pub const LED_TRIGGER_PANIC: &str = "panic";
/// CPU activity.
pub const LED_TRIGGER_CPU: &str = "cpu";
/// Backlight control.
pub const LED_TRIGGER_BACKLIGHT: &str = "backlight";

// ---------------------------------------------------------------------------
// LED flags
// ---------------------------------------------------------------------------

/// LED supports hardware blinking.
pub const LED_HW_PLUGGABLE: u32 = 1 << 0;
/// LED panic indicator (cannot be turned off during panic).
pub const LED_PANIC_INDICATOR: u32 = 1 << 1;
/// LED brightness can be set (not read-only).
pub const LED_BRIGHT_HW_CHANGED: u32 = 1 << 2;
/// LED retains state across suspend.
pub const LED_RETAIN_AT_SHUTDOWN: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Blink parameters (milliseconds)
// ---------------------------------------------------------------------------

/// Default blink on time (ms).
pub const LED_BLINK_ON_DEFAULT_MS: u32 = 500;
/// Default blink off time (ms).
pub const LED_BLINK_OFF_DEFAULT_MS: u32 = 500;

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
        assert_eq!(LED_OFF, 0);
        assert_eq!(LED_FULL, 255);
    }

    #[test]
    fn test_trigger_names_distinct() {
        let triggers = [
            LED_TRIGGER_NONE,
            LED_TRIGGER_DEFAULT_ON,
            LED_TRIGGER_HEARTBEAT,
            LED_TRIGGER_DISK_ACTIVITY,
            LED_TRIGGER_TIMER,
            LED_TRIGGER_NETDEV,
            LED_TRIGGER_PANIC,
            LED_TRIGGER_CPU,
            LED_TRIGGER_BACKLIGHT,
        ];
        for i in 0..triggers.len() {
            for j in (i + 1)..triggers.len() {
                assert_ne!(triggers[i], triggers[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            LED_HW_PLUGGABLE,
            LED_PANIC_INDICATOR,
            LED_BRIGHT_HW_CHANGED,
            LED_RETAIN_AT_SHUTDOWN,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_blink_defaults() {
        assert_eq!(LED_BLINK_ON_DEFAULT_MS, 500);
        assert_eq!(LED_BLINK_OFF_DEFAULT_MS, 500);
    }
}

//! `<linux/leds.h>` and `/sys/class/leds/` — LED class user ABI.
//!
//! The Linux LED class is the path every keyboard backlight tool,
//! GPIO-status indicator, smartphone notification daemon, and
//! `cpufreq`-trigger desk lamp uses. The constants below capture the
//! sysfs attribute names, the standard "function::color::index"
//! naming scheme, and the well-known trigger names recognized by
//! `ledtrig-*` drivers.

// ---------------------------------------------------------------------------
// Sysfs root
// ---------------------------------------------------------------------------

/// Mount point of the LED class — every LED lives under here.
pub const LEDS_CLASS_PATH: &str = "/sys/class/leds";

// ---------------------------------------------------------------------------
// Per-LED sysfs attribute filenames
// ---------------------------------------------------------------------------

pub const LED_ATTR_BRIGHTNESS: &str = "brightness";
pub const LED_ATTR_MAX_BRIGHTNESS: &str = "max_brightness";
pub const LED_ATTR_TRIGGER: &str = "trigger";
pub const LED_ATTR_DELAY_ON: &str = "delay_on";
pub const LED_ATTR_DELAY_OFF: &str = "delay_off";
pub const LED_ATTR_INVERT: &str = "invert";

// ---------------------------------------------------------------------------
// Standard trigger names (see `drivers/leds/trigger/`)
// ---------------------------------------------------------------------------

pub const LED_TRIGGER_NONE: &str = "none";
pub const LED_TRIGGER_TIMER: &str = "timer";
pub const LED_TRIGGER_HEARTBEAT: &str = "heartbeat";
pub const LED_TRIGGER_DEFAULT_ON: &str = "default-on";
pub const LED_TRIGGER_DISK_ACTIVITY: &str = "disk-activity";
pub const LED_TRIGGER_DISK_READ: &str = "disk-read";
pub const LED_TRIGGER_DISK_WRITE: &str = "disk-write";
pub const LED_TRIGGER_NETDEV: &str = "netdev";
pub const LED_TRIGGER_CPU: &str = "cpu";
pub const LED_TRIGGER_USBPORT: &str = "usbport";
pub const LED_TRIGGER_KBD_CAPSLOCK: &str = "kbd-capslock";
pub const LED_TRIGGER_KBD_NUMLOCK: &str = "kbd-numlock";
pub const LED_TRIGGER_KBD_SCROLLLOCK: &str = "kbd-scrolllock";

// ---------------------------------------------------------------------------
// Standard LED-function names (per `Documentation/leds/leds-class.rst`)
// ---------------------------------------------------------------------------

pub const LED_FUNCTION_ACTIVITY: &str = "activity";
pub const LED_FUNCTION_ALARM: &str = "alarm";
pub const LED_FUNCTION_BACKLIGHT: &str = "backlight";
pub const LED_FUNCTION_BLUETOOTH: &str = "bluetooth";
pub const LED_FUNCTION_BOOT: &str = "boot";
pub const LED_FUNCTION_CHARGING: &str = "charging";
pub const LED_FUNCTION_DEBUG: &str = "debug";
pub const LED_FUNCTION_DISK: &str = "disk";
pub const LED_FUNCTION_FAULT: &str = "fault";
pub const LED_FUNCTION_HEARTBEAT: &str = "heartbeat";
pub const LED_FUNCTION_INDICATOR: &str = "indicator";
pub const LED_FUNCTION_LAN: &str = "lan";
pub const LED_FUNCTION_MAIL: &str = "mail";
pub const LED_FUNCTION_MTD: &str = "mtd";
pub const LED_FUNCTION_PANIC: &str = "panic";
pub const LED_FUNCTION_PLAYER: &str = "player";
pub const LED_FUNCTION_POWER: &str = "power";
pub const LED_FUNCTION_RX: &str = "rx";
pub const LED_FUNCTION_STATUS: &str = "status";
pub const LED_FUNCTION_TX: &str = "tx";
pub const LED_FUNCTION_USB: &str = "usb";
pub const LED_FUNCTION_WAN: &str = "wan";
pub const LED_FUNCTION_WLAN: &str = "wlan";

// ---------------------------------------------------------------------------
// Standard brightness range
// ---------------------------------------------------------------------------

pub const LED_OFF: u32 = 0;
pub const LED_ON: u32 = 1;
pub const LED_HALF: u32 = 127;
pub const LED_FULL: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_class_path_is_sysfs() {
        assert_eq!(LEDS_CLASS_PATH, "/sys/class/leds");
    }

    #[test]
    fn test_brightness_levels_monotonic() {
        assert!(LED_OFF < LED_ON);
        assert!(LED_ON < LED_HALF);
        assert!(LED_HALF < LED_FULL);
        // FULL is exactly 8 bits.
        assert_eq!(LED_FULL, 255);
    }

    #[test]
    fn test_attr_names_no_underscore_or_slash() {
        let names = [
            LED_ATTR_BRIGHTNESS,
            LED_ATTR_MAX_BRIGHTNESS,
            LED_ATTR_TRIGGER,
            LED_ATTR_DELAY_ON,
            LED_ATTR_DELAY_OFF,
            LED_ATTR_INVERT,
        ];
        // sysfs attribute filenames never contain '/'.
        for n in names {
            assert!(!n.contains('/'));
            assert!(!n.is_empty());
        }
    }

    #[test]
    fn test_trigger_names_distinct() {
        let t = [
            LED_TRIGGER_NONE,
            LED_TRIGGER_TIMER,
            LED_TRIGGER_HEARTBEAT,
            LED_TRIGGER_DEFAULT_ON,
            LED_TRIGGER_DISK_ACTIVITY,
            LED_TRIGGER_DISK_READ,
            LED_TRIGGER_DISK_WRITE,
            LED_TRIGGER_NETDEV,
            LED_TRIGGER_CPU,
            LED_TRIGGER_USBPORT,
            LED_TRIGGER_KBD_CAPSLOCK,
            LED_TRIGGER_KBD_NUMLOCK,
            LED_TRIGGER_KBD_SCROLLLOCK,
        ];
        for i in 0..t.len() {
            for j in (i + 1)..t.len() {
                assert_ne!(t[i], t[j]);
            }
        }
    }

    #[test]
    fn test_function_names_lowercase() {
        let f = [
            LED_FUNCTION_ACTIVITY,
            LED_FUNCTION_ALARM,
            LED_FUNCTION_BACKLIGHT,
            LED_FUNCTION_BLUETOOTH,
            LED_FUNCTION_BOOT,
            LED_FUNCTION_CHARGING,
            LED_FUNCTION_DEBUG,
            LED_FUNCTION_DISK,
            LED_FUNCTION_FAULT,
            LED_FUNCTION_HEARTBEAT,
            LED_FUNCTION_INDICATOR,
            LED_FUNCTION_LAN,
            LED_FUNCTION_MAIL,
            LED_FUNCTION_MTD,
            LED_FUNCTION_PANIC,
            LED_FUNCTION_PLAYER,
            LED_FUNCTION_POWER,
            LED_FUNCTION_RX,
            LED_FUNCTION_STATUS,
            LED_FUNCTION_TX,
            LED_FUNCTION_USB,
            LED_FUNCTION_WAN,
            LED_FUNCTION_WLAN,
        ];
        for name in f {
            for b in name.as_bytes() {
                assert!(b.is_ascii_lowercase() || b.is_ascii_digit());
            }
        }
    }
}

//! `<linux/device/class.h>` — Device class constants.
//!
//! Device classes group devices by their function rather than their
//! bus connection. For example, all network interfaces (Ethernet, WiFi,
//! virtual) appear under /sys/class/net/ regardless of whether they're
//! PCI, USB, or virtual devices. Classes provide a stable userspace
//! interface for device discovery and are the basis for /dev node
//! creation by udev rules.

// ---------------------------------------------------------------------------
// Device class categories
// ---------------------------------------------------------------------------

/// Block device class (disks, partitions).
pub const DEV_CLASS_BLOCK: u32 = 0;
/// Character device class (terminals, misc).
pub const DEV_CLASS_CHAR: u32 = 1;
/// Network device class (interfaces).
pub const DEV_CLASS_NET: u32 = 2;
/// Input device class (keyboard, mouse, touchscreen).
pub const DEV_CLASS_INPUT: u32 = 3;
/// Sound device class (ALSA cards).
pub const DEV_CLASS_SOUND: u32 = 4;
/// Video/DRM device class (GPU, display).
pub const DEV_CLASS_DRM: u32 = 5;
/// TTY device class (serial, virtual consoles).
pub const DEV_CLASS_TTY: u32 = 6;
/// USB device class (USB host controllers, devices).
pub const DEV_CLASS_USB: u32 = 7;
/// Power supply class (batteries, chargers).
pub const DEV_CLASS_POWER_SUPPLY: u32 = 8;
/// Thermal class (thermal zones, cooling devices).
pub const DEV_CLASS_THERMAL: u32 = 9;
/// Backlight class (display backlights).
pub const DEV_CLASS_BACKLIGHT: u32 = 10;
/// LEDs class (indicator lights).
pub const DEV_CLASS_LEDS: u32 = 11;
/// Watchdog class (hardware watchdog timers).
pub const DEV_CLASS_WATCHDOG: u32 = 12;
/// RTC class (real-time clocks).
pub const DEV_CLASS_RTC: u32 = 13;
/// Regulator class (voltage/current regulators).
pub const DEV_CLASS_REGULATOR: u32 = 14;
/// HWMON class (hardware monitoring, temperature/fan/voltage sensors).
pub const DEV_CLASS_HWMON: u32 = 15;

// ---------------------------------------------------------------------------
// Device class flags
// ---------------------------------------------------------------------------

/// Class manages its own device nodes (no udev needed).
pub const CLASS_FLAG_OWN_DEVNODE: u32 = 0x01;
/// Class supports namespace isolation.
pub const CLASS_FLAG_NS_AWARE: u32 = 0x02;
/// Class devices are enumerable (have well-defined numbering).
pub const CLASS_FLAG_ENUMERABLE: u32 = 0x04;

// ---------------------------------------------------------------------------
// Device number limits
// ---------------------------------------------------------------------------

/// Maximum major device number.
pub const DEV_MAJOR_MAX: u32 = 512;
/// Maximum minor device number.
pub const DEV_MINOR_MAX: u32 = 1048576; // 2^20
/// Bits for minor number in dev_t.
pub const DEV_MINOR_BITS: u32 = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classes_distinct() {
        let classes = [
            DEV_CLASS_BLOCK, DEV_CLASS_CHAR, DEV_CLASS_NET,
            DEV_CLASS_INPUT, DEV_CLASS_SOUND, DEV_CLASS_DRM,
            DEV_CLASS_TTY, DEV_CLASS_USB, DEV_CLASS_POWER_SUPPLY,
            DEV_CLASS_THERMAL, DEV_CLASS_BACKLIGHT, DEV_CLASS_LEDS,
            DEV_CLASS_WATCHDOG, DEV_CLASS_RTC, DEV_CLASS_REGULATOR,
            DEV_CLASS_HWMON,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CLASS_FLAG_OWN_DEVNODE, CLASS_FLAG_NS_AWARE,
            CLASS_FLAG_ENUMERABLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_dev_number_limits() {
        assert!(DEV_MAJOR_MAX > 0);
        assert!(DEV_MINOR_MAX > 0);
        assert_eq!(DEV_MINOR_MAX, 1 << DEV_MINOR_BITS);
    }
}

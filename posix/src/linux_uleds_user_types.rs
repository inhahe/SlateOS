//! `<linux/uleds.h>` — userspace LED-class transport.
//!
//! `/dev/uleds` lets a userspace daemon present a virtual LED into
//! `/sys/class/leds/`. Status-tray apps, build-light wrappers, and
//! desktop-notifier services use it to expose blinkable indicators
//! without writing a kernel driver.

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

/// Maximum LED name length (matches LED_MAX_NAME_SIZE).
pub const LED_MAX_NAME_SIZE: usize = 64;

// ---------------------------------------------------------------------------
// Protocol
// ---------------------------------------------------------------------------

/// Size of `struct uleds_user_dev` (name[64] + max_brightness u32).
pub const ULEDS_USER_DEV_SIZE: usize = LED_MAX_NAME_SIZE + 4;
/// Wire format for brightness: read returns a 4-byte little-endian u32.
pub const ULEDS_BRIGHTNESS_BYTES: usize = 4;

// ---------------------------------------------------------------------------
// Typical max_brightness values
// ---------------------------------------------------------------------------

/// Default `max_brightness` (single-bit on/off LED).
pub const ULEDS_BRIGHTNESS_ON_OFF: u32 = 1;
/// 8-bit PWM dimmable (common GPIO+driver pairing).
pub const ULEDS_BRIGHTNESS_8BIT: u32 = 255;
/// 12-bit dimmable (kernel max for many PCA-style chips).
pub const ULEDS_BRIGHTNESS_12BIT: u32 = 4095;

// ---------------------------------------------------------------------------
// Sysfs root
// ---------------------------------------------------------------------------

/// `/sys/class/leds` — root for the LED class.
pub const LED_SYSFS_ROOT: &str = "/sys/class/leds";
/// uleds character device.
pub const ULEDS_DEVICE: &str = "/dev/uleds";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_size() {
        // 64 matches `LED_MAX_NAME_SIZE` in include/linux/leds.h.
        assert_eq!(LED_MAX_NAME_SIZE, 64);
        assert!(LED_MAX_NAME_SIZE.is_power_of_two());
    }

    #[test]
    fn test_user_dev_layout() {
        // 64 (name) + 4 (max_brightness u32) = 68 bytes on the wire.
        assert_eq!(ULEDS_USER_DEV_SIZE, 68);
        assert_eq!(ULEDS_BRIGHTNESS_BYTES, 4);
    }

    #[test]
    fn test_brightness_levels_ordered() {
        assert!(ULEDS_BRIGHTNESS_ON_OFF < ULEDS_BRIGHTNESS_8BIT);
        assert!(ULEDS_BRIGHTNESS_8BIT < ULEDS_BRIGHTNESS_12BIT);
        // 8-bit max really is 255 (and 12-bit really is 4095).
        assert_eq!(ULEDS_BRIGHTNESS_8BIT, (1u32 << 8) - 1);
        assert_eq!(ULEDS_BRIGHTNESS_12BIT, (1u32 << 12) - 1);
    }

    #[test]
    fn test_paths() {
        assert_eq!(LED_SYSFS_ROOT, "/sys/class/leds");
        assert_eq!(ULEDS_DEVICE, "/dev/uleds");
        // Both live under canonical kernel-supplied trees.
        assert!(LED_SYSFS_ROOT.starts_with("/sys/class/"));
        assert!(ULEDS_DEVICE.starts_with("/dev/"));
    }
}

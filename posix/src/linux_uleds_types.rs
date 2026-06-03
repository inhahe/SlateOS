//! `<linux/uleds.h>` — userspace-LED character-device interface.
//!
//! `/dev/uleds` lets a process create a virtual LED that shows up in
//! sysfs (`/sys/class/leds`). The device is configured by writing a
//! `uleds_user_dev` struct (name + max_brightness) and then read for
//! brightness updates as `i32` values. Useful for software-defined
//! status LEDs and tests.

// ---------------------------------------------------------------------------
// uleds_user_dev field sizes
// ---------------------------------------------------------------------------

/// Maximum LED name length, matching `LED_MAX_NAME_SIZE` in the kernel
/// header. Names longer than 63 bytes (plus NUL) are rejected.
pub const LED_MAX_NAME_SIZE: u32 = 64;

/// Total `struct uleds_user_dev` size on the wire: name buffer +
/// `max_brightness` (i32, 4 bytes). Userspace must write exactly this
/// many bytes to register the device.
pub const ULEDS_USER_DEV_SIZE: u32 = LED_MAX_NAME_SIZE + 4;

// ---------------------------------------------------------------------------
// Brightness read size
// ---------------------------------------------------------------------------

/// Brightness value size (i32). One `read(2)` returns exactly this many
/// bytes per brightness update.
pub const ULEDS_BRIGHTNESS_SIZE: u32 = 4;

// ---------------------------------------------------------------------------
// LED_OFF / LED_ON / LED_FULL constants (from <linux/leds.h>, but
// shared with uleds clients)
// ---------------------------------------------------------------------------

/// LED off (brightness = 0).
pub const LED_OFF: u32 = 0;
/// LED on at lowest non-zero brightness (LED is "lit" but not bright).
pub const LED_ON: u32 = 1;
/// Default half brightness.
pub const LED_HALF: u32 = 127;
/// Maximum brightness used by most simple LEDs.
pub const LED_FULL: u32 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_size_is_64() {
        // LED_MAX_NAME_SIZE has been stable at 64 since uleds was added
        // in 4.13 — a regression here means we shadow a stale value.
        assert_eq!(LED_MAX_NAME_SIZE, 64);
    }

    #[test]
    fn test_user_dev_size_is_name_plus_i32() {
        assert_eq!(ULEDS_USER_DEV_SIZE, LED_MAX_NAME_SIZE + 4);
        assert_eq!(ULEDS_BRIGHTNESS_SIZE, 4);
    }

    #[test]
    fn test_brightness_levels_ordered() {
        assert!(LED_OFF < LED_ON);
        assert!(LED_ON < LED_HALF);
        assert!(LED_HALF < LED_FULL);
        assert_eq!(LED_OFF, 0);
        assert_eq!(LED_FULL, 255);
    }
}

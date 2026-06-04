//! `<linux/backlight.h>` — display backlight control sysfs surface.
//!
//! Backlight devices live under `/sys/class/backlight/<name>/` with a
//! tiny stable file API: `brightness`, `max_brightness`, `bl_power`,
//! `actual_brightness`, `type`. The kernel does not assign per-device
//! ioctl codes — userspace writes ASCII integers to the sysfs files.

// ---------------------------------------------------------------------------
// sysfs layout
// ---------------------------------------------------------------------------

pub const SYS_CLASS_BACKLIGHT: &str = "/sys/class/backlight";
pub const SYSFS_BRIGHTNESS: &str = "brightness";
pub const SYSFS_ACTUAL_BRIGHTNESS: &str = "actual_brightness";
pub const SYSFS_MAX_BRIGHTNESS: &str = "max_brightness";
pub const SYSFS_BL_POWER: &str = "bl_power";
pub const SYSFS_TYPE: &str = "type";
pub const SYSFS_SCALE: &str = "scale";

// ---------------------------------------------------------------------------
// Backlight type strings (string contents of the "type" file)
// ---------------------------------------------------------------------------

pub const BACKLIGHT_TYPE_RAW: &str = "raw";
pub const BACKLIGHT_TYPE_PLATFORM: &str = "platform";
pub const BACKLIGHT_TYPE_FIRMWARE: &str = "firmware";

// ---------------------------------------------------------------------------
// Numeric type enum (matches `enum backlight_type` ABI ordering)
// ---------------------------------------------------------------------------

pub const BACKLIGHT_RAW: u32 = 1;
pub const BACKLIGHT_PLATFORM: u32 = 2;
pub const BACKLIGHT_FIRMWARE: u32 = 3;
pub const BACKLIGHT_TYPE_MAX: u32 = 4;

// ---------------------------------------------------------------------------
// Power states (FB_BLANK_* convention used by drivers)
// ---------------------------------------------------------------------------

pub const FB_BLANK_UNBLANK: u32 = 0;
pub const FB_BLANK_NORMAL: u32 = 1;
pub const FB_BLANK_VSYNC_SUSPEND: u32 = 2;
pub const FB_BLANK_HSYNC_SUSPEND: u32 = 3;
pub const FB_BLANK_POWERDOWN: u32 = 4;

// ---------------------------------------------------------------------------
// Scale strings ("unknown", "linear", "non-linear")
// ---------------------------------------------------------------------------

pub const BACKLIGHT_SCALE_UNKNOWN: &str = "unknown";
pub const BACKLIGHT_SCALE_LINEAR: &str = "linear";
pub const BACKLIGHT_SCALE_NON_LINEAR: &str = "non-linear";

// ---------------------------------------------------------------------------
// Numeric scale enum
// ---------------------------------------------------------------------------

pub const BACKLIGHT_SCALE_UNKNOWN_N: u32 = 0;
pub const BACKLIGHT_SCALE_LINEAR_N: u32 = 1;
pub const BACKLIGHT_SCALE_NON_LINEAR_N: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysfs_root_path() {
        assert_eq!(SYS_CLASS_BACKLIGHT, "/sys/class/backlight");
        assert!(SYS_CLASS_BACKLIGHT.starts_with("/sys/class/"));
    }

    #[test]
    fn test_sysfs_attribute_names_distinct() {
        let names = [
            SYSFS_BRIGHTNESS,
            SYSFS_ACTUAL_BRIGHTNESS,
            SYSFS_MAX_BRIGHTNESS,
            SYSFS_BL_POWER,
            SYSFS_TYPE,
            SYSFS_SCALE,
        ];
        for (i, &a) in names.iter().enumerate() {
            for &b in &names[i + 1..] {
                assert_ne!(a, b);
            }
            // Sysfs filenames contain no leading slash.
            assert!(!a.starts_with('/'));
        }
        // brightness/actual_brightness/max_brightness all share the
        // "brightness" suffix.
        assert!(SYSFS_BRIGHTNESS.ends_with("brightness"));
        assert!(SYSFS_ACTUAL_BRIGHTNESS.ends_with("brightness"));
        assert!(SYSFS_MAX_BRIGHTNESS.ends_with("brightness"));
    }

    #[test]
    fn test_type_strings_dense_lowercase() {
        let t = [
            BACKLIGHT_TYPE_RAW,
            BACKLIGHT_TYPE_PLATFORM,
            BACKLIGHT_TYPE_FIRMWARE,
        ];
        for &s in &t {
            assert!(!s.is_empty());
            assert!(s.bytes().all(|b| b.is_ascii_lowercase()));
        }
    }

    #[test]
    fn test_type_numeric_dense_1_to_3() {
        assert_eq!(BACKLIGHT_RAW, 1);
        assert_eq!(BACKLIGHT_PLATFORM, 2);
        assert_eq!(BACKLIGHT_FIRMWARE, 3);
        assert_eq!(BACKLIGHT_TYPE_MAX, 4);
        // _MAX is exclusive upper bound.
        assert!(BACKLIGHT_FIRMWARE < BACKLIGHT_TYPE_MAX);
    }

    #[test]
    fn test_fb_blank_states_dense_0_to_4() {
        let b = [
            FB_BLANK_UNBLANK,
            FB_BLANK_NORMAL,
            FB_BLANK_VSYNC_SUSPEND,
            FB_BLANK_HSYNC_SUSPEND,
            FB_BLANK_POWERDOWN,
        ];
        for (i, &v) in b.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // UNBLANK = 0 means "fully on" — the only state that lights the
        // panel without entering any power saving.
        assert_eq!(FB_BLANK_UNBLANK, 0);
        assert!(FB_BLANK_POWERDOWN > FB_BLANK_UNBLANK);
    }

    #[test]
    fn test_scale_strings_match_numeric_enum() {
        assert_eq!(BACKLIGHT_SCALE_UNKNOWN, "unknown");
        assert_eq!(BACKLIGHT_SCALE_LINEAR, "linear");
        assert_eq!(BACKLIGHT_SCALE_NON_LINEAR, "non-linear");
        // Numeric and string variants agree on ordering.
        assert_eq!(BACKLIGHT_SCALE_UNKNOWN_N, 0);
        assert_eq!(BACKLIGHT_SCALE_LINEAR_N, 1);
        assert_eq!(BACKLIGHT_SCALE_NON_LINEAR_N, 2);
    }
}

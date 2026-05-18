//! `<linux/gpio.h>` (chip info subset) — GPIO chip information constants.
//!
//! Each GPIO controller is represented as a chip with a label, a
//! base number, and a count of lines. The chardev interface exposes
//! chip information via `GPIO_GET_CHIPINFO_IOCTL`. Chip-level
//! properties help userspace tools (gpiodetect, gpioinfo) discover
//! available GPIO controllers.

// ---------------------------------------------------------------------------
// Chip name/label size limits
// ---------------------------------------------------------------------------

/// Maximum chip name length.
pub const GPIO_MAX_NAME_SIZE: u32 = 32;
/// Maximum line name length.
pub const GPIOLINE_MAX_NAME_SIZE: u32 = 32;
/// Maximum consumer name length.
pub const GPIOLINE_MAX_CONSUMER_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// Line count limits
// ---------------------------------------------------------------------------

/// Maximum lines per handle request (v1 API).
pub const GPIOHANDLES_MAX: u32 = 64;
/// Maximum lines per request (v2 API).
pub const GPIO_V2_LINES_MAX: u32 = 64;
/// Maximum line attributes per request (v2 API).
pub const GPIO_V2_LINE_NUM_ATTRS_MAX: u32 = 10;

// ---------------------------------------------------------------------------
// GPIO numbering
// ---------------------------------------------------------------------------

/// Dynamic base allocation (let kernel pick the base number).
pub const GPIO_DYNAMIC_BASE: i32 = -1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_sizes() {
        assert!(GPIO_MAX_NAME_SIZE > 0);
        assert_eq!(GPIOLINE_MAX_NAME_SIZE, 32);
        assert_eq!(GPIOLINE_MAX_CONSUMER_SIZE, 32);
    }

    #[test]
    fn test_handle_limits() {
        assert_eq!(GPIOHANDLES_MAX, 64);
        assert_eq!(GPIO_V2_LINES_MAX, 64);
    }

    #[test]
    fn test_dynamic_base() {
        assert_eq!(GPIO_DYNAMIC_BASE, -1);
    }

    #[test]
    fn test_attr_limit() {
        assert!(GPIO_V2_LINE_NUM_ATTRS_MAX > 0);
        assert!(GPIO_V2_LINE_NUM_ATTRS_MAX <= 64);
    }
}

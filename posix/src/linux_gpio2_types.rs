//! `<linux/gpio.h>` — Additional GPIO constants.
//!
//! Supplementary GPIO constants covering line flags,
//! event types, and configuration attributes for
//! the GPIO character device interface (v2).

// ---------------------------------------------------------------------------
// GPIO line flags (GPIO_V2_LINE_FLAG_*)
// ---------------------------------------------------------------------------

/// Line in use.
pub const GPIO_V2_LINE_FLAG_USED: u64 = 1 << 0;
/// Active low.
pub const GPIO_V2_LINE_FLAG_ACTIVE_LOW: u64 = 1 << 1;
/// Input.
pub const GPIO_V2_LINE_FLAG_INPUT: u64 = 1 << 2;
/// Output.
pub const GPIO_V2_LINE_FLAG_OUTPUT: u64 = 1 << 3;
/// Edge rising.
pub const GPIO_V2_LINE_FLAG_EDGE_RISING: u64 = 1 << 4;
/// Edge falling.
pub const GPIO_V2_LINE_FLAG_EDGE_FALLING: u64 = 1 << 5;
/// Open drain.
pub const GPIO_V2_LINE_FLAG_OPEN_DRAIN: u64 = 1 << 6;
/// Open source.
pub const GPIO_V2_LINE_FLAG_OPEN_SOURCE: u64 = 1 << 7;
/// Bias pull up.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_UP: u64 = 1 << 8;
/// Bias pull down.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_DOWN: u64 = 1 << 9;
/// Bias disabled.
pub const GPIO_V2_LINE_FLAG_BIAS_DISABLED: u64 = 1 << 10;
/// Event clock realtime.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_REALTIME: u64 = 1 << 11;
/// Event clock HTE.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_HTE: u64 = 1 << 12;

// ---------------------------------------------------------------------------
// GPIO line attribute IDs
// ---------------------------------------------------------------------------

/// Flags attribute.
pub const GPIO_V2_LINE_ATTR_ID_FLAGS: u32 = 1;
/// Output values attribute.
pub const GPIO_V2_LINE_ATTR_ID_OUTPUT_VALUES: u32 = 2;
/// Debounce attribute.
pub const GPIO_V2_LINE_ATTR_ID_DEBOUNCE: u32 = 3;

// ---------------------------------------------------------------------------
// GPIO event types (GPIO_V2_LINE_EVENT_*)
// ---------------------------------------------------------------------------

/// Rising edge event.
pub const GPIO_V2_LINE_EVENT_RISING_EDGE: u32 = 1;
/// Falling edge event.
pub const GPIO_V2_LINE_EVENT_FALLING_EDGE: u32 = 2;

// ---------------------------------------------------------------------------
// GPIO chip info flags
// ---------------------------------------------------------------------------

/// Max lines per chip.
pub const GPIO_MAX_NAME_SIZE: u32 = 32;
/// Max lines per request.
pub const GPIO_V2_LINES_MAX: u32 = 64;
/// Max config attributes per request.
pub const GPIO_V2_LINE_NUM_ATTRS_MAX: u32 = 10;

// ---------------------------------------------------------------------------
// Legacy GPIO flags (v1 ABI)
// ---------------------------------------------------------------------------

/// Kernel line.
pub const GPIOLINE_FLAG_KERNEL: u32 = 1 << 0;
/// Line is output.
pub const GPIOLINE_FLAG_IS_OUT: u32 = 1 << 1;
/// Active low.
pub const GPIOLINE_FLAG_ACTIVE_LOW: u32 = 1 << 2;
/// Open drain.
pub const GPIOLINE_FLAG_OPEN_DRAIN: u32 = 1 << 3;
/// Open source.
pub const GPIOLINE_FLAG_OPEN_SOURCE: u32 = 1 << 4;
/// Bias pull up.
pub const GPIOLINE_FLAG_BIAS_PULL_UP: u32 = 1 << 5;
/// Bias pull down.
pub const GPIOLINE_FLAG_BIAS_PULL_DOWN: u32 = 1 << 6;
/// Bias disabled.
pub const GPIOLINE_FLAG_BIAS_DISABLE: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v2_flags_power_of_two() {
        let flags: [u64; 13] = [
            GPIO_V2_LINE_FLAG_USED,
            GPIO_V2_LINE_FLAG_ACTIVE_LOW,
            GPIO_V2_LINE_FLAG_INPUT,
            GPIO_V2_LINE_FLAG_OUTPUT,
            GPIO_V2_LINE_FLAG_EDGE_RISING,
            GPIO_V2_LINE_FLAG_EDGE_FALLING,
            GPIO_V2_LINE_FLAG_OPEN_DRAIN,
            GPIO_V2_LINE_FLAG_OPEN_SOURCE,
            GPIO_V2_LINE_FLAG_BIAS_PULL_UP,
            GPIO_V2_LINE_FLAG_BIAS_PULL_DOWN,
            GPIO_V2_LINE_FLAG_BIAS_DISABLED,
            GPIO_V2_LINE_FLAG_EVENT_CLOCK_REALTIME,
            GPIO_V2_LINE_FLAG_EVENT_CLOCK_HTE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:016x} not power of two", f);
        }
    }

    #[test]
    fn test_attr_ids_sequential() {
        assert_eq!(GPIO_V2_LINE_ATTR_ID_FLAGS, 1);
        assert_eq!(GPIO_V2_LINE_ATTR_ID_OUTPUT_VALUES, 2);
        assert_eq!(GPIO_V2_LINE_ATTR_ID_DEBOUNCE, 3);
    }

    #[test]
    fn test_event_types() {
        assert_eq!(GPIO_V2_LINE_EVENT_RISING_EDGE, 1);
        assert_eq!(GPIO_V2_LINE_EVENT_FALLING_EDGE, 2);
    }

    #[test]
    fn test_limits() {
        assert_eq!(GPIO_MAX_NAME_SIZE, 32);
        assert_eq!(GPIO_V2_LINES_MAX, 64);
    }

    #[test]
    fn test_legacy_flags_power_of_two() {
        let flags = [
            GPIOLINE_FLAG_KERNEL,
            GPIOLINE_FLAG_IS_OUT,
            GPIOLINE_FLAG_ACTIVE_LOW,
            GPIOLINE_FLAG_OPEN_DRAIN,
            GPIOLINE_FLAG_OPEN_SOURCE,
            GPIOLINE_FLAG_BIAS_PULL_UP,
            GPIOLINE_FLAG_BIAS_PULL_DOWN,
            GPIOLINE_FLAG_BIAS_DISABLE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }
}

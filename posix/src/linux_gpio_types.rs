//! `<linux/gpio.h>` — GPIO (General-Purpose I/O) subsystem constants.
//!
//! The Linux GPIO subsystem provides a unified interface for accessing
//! GPIO pins from userspace (via /dev/gpiochipN character devices) and
//! kernel space. GPIOs are used for LEDs, buttons, chip selects, resets,
//! interrupts, and low-speed communication on embedded systems.

// ---------------------------------------------------------------------------
// GPIO line flags
// ---------------------------------------------------------------------------

/// Line is used/busy.
pub const GPIO_V2_LINE_FLAG_USED: u64 = 1 << 0;
/// Line is active-low.
pub const GPIO_V2_LINE_FLAG_ACTIVE_LOW: u64 = 1 << 1;
/// Line direction is input.
pub const GPIO_V2_LINE_FLAG_INPUT: u64 = 1 << 2;
/// Line direction is output.
pub const GPIO_V2_LINE_FLAG_OUTPUT: u64 = 1 << 3;
/// Edge detection: rising.
pub const GPIO_V2_LINE_FLAG_EDGE_RISING: u64 = 1 << 4;
/// Edge detection: falling.
pub const GPIO_V2_LINE_FLAG_EDGE_FALLING: u64 = 1 << 5;
/// Open drain output.
pub const GPIO_V2_LINE_FLAG_OPEN_DRAIN: u64 = 1 << 6;
/// Open source output.
pub const GPIO_V2_LINE_FLAG_OPEN_SOURCE: u64 = 1 << 7;
/// Internal pull-up enabled.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_UP: u64 = 1 << 8;
/// Internal pull-down enabled.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_DOWN: u64 = 1 << 9;
/// No bias (high-impedance).
pub const GPIO_V2_LINE_FLAG_BIAS_DISABLED: u64 = 1 << 10;
/// Event clock: realtime.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_REALTIME: u64 = 1 << 11;
/// Event clock: hardware timestamp.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_HTE: u64 = 1 << 12;

// ---------------------------------------------------------------------------
// GPIO event types
// ---------------------------------------------------------------------------

/// Rising edge event.
pub const GPIO_V2_LINE_EVENT_RISING_EDGE: u32 = 1;
/// Falling edge event.
pub const GPIO_V2_LINE_EVENT_FALLING_EDGE: u32 = 2;

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
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of lines per request.
pub const GPIO_V2_LINES_MAX: u32 = 64;
/// Maximum GPIO line name length.
pub const GPIO_MAX_NAME_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_flags_no_overlap() {
        let flags = [
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
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_event_types_distinct() {
        assert_ne!(
            GPIO_V2_LINE_EVENT_RISING_EDGE,
            GPIO_V2_LINE_EVENT_FALLING_EDGE
        );
    }

    #[test]
    fn test_attr_ids_distinct() {
        let ids = [
            GPIO_V2_LINE_ATTR_ID_FLAGS,
            GPIO_V2_LINE_ATTR_ID_OUTPUT_VALUES,
            GPIO_V2_LINE_ATTR_ID_DEBOUNCE,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert_eq!(GPIO_V2_LINES_MAX, 64);
        assert_eq!(GPIO_MAX_NAME_SIZE, 32);
    }
}

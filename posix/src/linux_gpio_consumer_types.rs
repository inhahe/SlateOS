//! `<linux/gpio/consumer.h>` — GPIO consumer API constants.
//!
//! The GPIO consumer API is how drivers request and use GPIO pins.
//! GPIOs are identified by function name (not number) and looked up
//! from device tree, ACPI, or board file descriptions. The consumer
//! API provides logical operations (active-high/low abstraction),
//! interrupt support, debouncing, and multi-GPIO atomic operations.
//! Supersedes the legacy numbered-GPIO API (gpio_request/gpio_set_value).

// ---------------------------------------------------------------------------
// GPIO direction
// ---------------------------------------------------------------------------

/// GPIO is an input.
pub const GPIO_DIR_INPUT: u32 = 0;
/// GPIO is an output.
pub const GPIO_DIR_OUTPUT: u32 = 1;

// ---------------------------------------------------------------------------
// GPIO active level (logical vs physical)
// ---------------------------------------------------------------------------

/// Active high (logical 1 = physical high).
pub const GPIO_ACTIVE_HIGH: u32 = 0;
/// Active low (logical 1 = physical low).
pub const GPIO_ACTIVE_LOW: u32 = 1;

// ---------------------------------------------------------------------------
// GPIO flags (for gpio_desc / device tree bindings)
// ---------------------------------------------------------------------------

/// GPIO is open drain (can only drive low or float).
pub const GPIO_FLAG_OPEN_DRAIN: u32 = 1 << 0;
/// GPIO is open source (can only drive high or float).
pub const GPIO_FLAG_OPEN_SOURCE: u32 = 1 << 1;
/// GPIO has internal pull-up enabled.
pub const GPIO_FLAG_PULL_UP: u32 = 1 << 2;
/// GPIO has internal pull-down enabled.
pub const GPIO_FLAG_PULL_DOWN: u32 = 1 << 3;
/// GPIO is used for interrupt (not general I/O).
pub const GPIO_FLAG_IRQ: u32 = 1 << 4;
/// GPIO output is initially low.
pub const GPIO_FLAG_INIT_LOW: u32 = 1 << 5;
/// GPIO output is initially high.
pub const GPIO_FLAG_INIT_HIGH: u32 = 1 << 6;
/// GPIO is transitory (value doesn't persist in suspend).
pub const GPIO_FLAG_TRANSITORY: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// GPIO lookup flags (for gpiod_get)
// ---------------------------------------------------------------------------

/// Default lookup (use DT/ACPI/board polarity).
pub const GPIOD_FLAGS_DEFAULT: u32 = 0;
/// Request as input.
pub const GPIOD_FLAGS_IN: u32 = 1;
/// Request as output, initially low.
pub const GPIOD_FLAGS_OUT_LOW: u32 = 2;
/// Request as output, initially high.
pub const GPIOD_FLAGS_OUT_HIGH: u32 = 3;
/// Request as output, initial value from DT.
pub const GPIOD_FLAGS_OUT_LOW_OPEN_DRAIN: u32 = 4;

// ---------------------------------------------------------------------------
// GPIO interrupt trigger types
// ---------------------------------------------------------------------------

/// Trigger on rising edge.
pub const GPIO_IRQ_RISING: u32 = 1 << 0;
/// Trigger on falling edge.
pub const GPIO_IRQ_FALLING: u32 = 1 << 1;
/// Trigger on both edges.
pub const GPIO_IRQ_BOTH: u32 = (1 << 0) | (1 << 1);
/// Trigger on high level.
pub const GPIO_IRQ_HIGH: u32 = 1 << 2;
/// Trigger on low level.
pub const GPIO_IRQ_LOW: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direction_distinct() {
        assert_ne!(GPIO_DIR_INPUT, GPIO_DIR_OUTPUT);
    }

    #[test]
    fn test_active_level_distinct() {
        assert_ne!(GPIO_ACTIVE_HIGH, GPIO_ACTIVE_LOW);
    }

    #[test]
    fn test_gpio_flags_selective_no_overlap() {
        let flags = [
            GPIO_FLAG_OPEN_DRAIN, GPIO_FLAG_OPEN_SOURCE,
            GPIO_FLAG_PULL_UP, GPIO_FLAG_PULL_DOWN,
            GPIO_FLAG_IRQ, GPIO_FLAG_INIT_LOW,
            GPIO_FLAG_INIT_HIGH, GPIO_FLAG_TRANSITORY,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_lookup_flags_distinct() {
        let flags = [
            GPIOD_FLAGS_DEFAULT, GPIOD_FLAGS_IN,
            GPIOD_FLAGS_OUT_LOW, GPIOD_FLAGS_OUT_HIGH,
            GPIOD_FLAGS_OUT_LOW_OPEN_DRAIN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_irq_triggers() {
        assert_eq!(GPIO_IRQ_BOTH, GPIO_IRQ_RISING | GPIO_IRQ_FALLING);
        assert_eq!(GPIO_IRQ_RISING & GPIO_IRQ_FALLING, 0);
        assert_eq!(GPIO_IRQ_HIGH & GPIO_IRQ_LOW, 0);
    }
}

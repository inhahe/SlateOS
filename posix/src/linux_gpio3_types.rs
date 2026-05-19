//! `<linux/gpio.h>` — Additional GPIO constants (part 3).
//!
//! Supplementary GPIO constants covering line config flags,
//! event types, and chip info flags.

// ---------------------------------------------------------------------------
// GPIO v2 line flags
// ---------------------------------------------------------------------------

/// Line used.
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
/// Bias pull-up.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_UP: u64 = 1 << 8;
/// Bias pull-down.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_DOWN: u64 = 1 << 9;
/// Bias disabled.
pub const GPIO_V2_LINE_FLAG_BIAS_DISABLED: u64 = 1 << 10;
/// Event clock realtime.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_REALTIME: u64 = 1 << 11;
/// Event clock HTE.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_HTE: u64 = 1 << 12;

// ---------------------------------------------------------------------------
// GPIO v2 line attribute IDs
// ---------------------------------------------------------------------------

/// Flags attribute.
pub const GPIO_V2_LINE_ATTR_ID_FLAGS: u32 = 1;
/// Output values attribute.
pub const GPIO_V2_LINE_ATTR_ID_OUTPUT_VALUES: u32 = 2;
/// Debounce period attribute.
pub const GPIO_V2_LINE_ATTR_ID_DEBOUNCE: u32 = 3;

// ---------------------------------------------------------------------------
// GPIO v2 event types
// ---------------------------------------------------------------------------

/// Rising edge.
pub const GPIO_V2_LINE_EVENT_RISING_EDGE: u32 = 1;
/// Falling edge.
pub const GPIO_V2_LINE_EVENT_FALLING_EDGE: u32 = 2;

// ---------------------------------------------------------------------------
// GPIO v2 line change types
// ---------------------------------------------------------------------------

/// Line requested.
pub const GPIO_V2_LINE_CHANGED_REQUESTED: u32 = 1;
/// Line released.
pub const GPIO_V2_LINE_CHANGED_RELEASED: u32 = 2;
/// Line config changed.
pub const GPIO_V2_LINE_CHANGED_CONFIG: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_flags_power_of_two() {
        let flags = [
            GPIO_V2_LINE_FLAG_USED, GPIO_V2_LINE_FLAG_ACTIVE_LOW,
            GPIO_V2_LINE_FLAG_INPUT, GPIO_V2_LINE_FLAG_OUTPUT,
            GPIO_V2_LINE_FLAG_EDGE_RISING, GPIO_V2_LINE_FLAG_EDGE_FALLING,
            GPIO_V2_LINE_FLAG_OPEN_DRAIN, GPIO_V2_LINE_FLAG_OPEN_SOURCE,
            GPIO_V2_LINE_FLAG_BIAS_PULL_UP, GPIO_V2_LINE_FLAG_BIAS_PULL_DOWN,
            GPIO_V2_LINE_FLAG_BIAS_DISABLED,
            GPIO_V2_LINE_FLAG_EVENT_CLOCK_REALTIME,
            GPIO_V2_LINE_FLAG_EVENT_CLOCK_HTE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_line_flags_no_overlap() {
        let flags = [
            GPIO_V2_LINE_FLAG_USED, GPIO_V2_LINE_FLAG_ACTIVE_LOW,
            GPIO_V2_LINE_FLAG_INPUT, GPIO_V2_LINE_FLAG_OUTPUT,
            GPIO_V2_LINE_FLAG_EDGE_RISING, GPIO_V2_LINE_FLAG_EDGE_FALLING,
            GPIO_V2_LINE_FLAG_OPEN_DRAIN, GPIO_V2_LINE_FLAG_OPEN_SOURCE,
            GPIO_V2_LINE_FLAG_BIAS_PULL_UP, GPIO_V2_LINE_FLAG_BIAS_PULL_DOWN,
            GPIO_V2_LINE_FLAG_BIAS_DISABLED,
            GPIO_V2_LINE_FLAG_EVENT_CLOCK_REALTIME,
            GPIO_V2_LINE_FLAG_EVENT_CLOCK_HTE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
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
    fn test_event_types_distinct() {
        assert_ne!(GPIO_V2_LINE_EVENT_RISING_EDGE, GPIO_V2_LINE_EVENT_FALLING_EDGE);
    }

    #[test]
    fn test_change_types_distinct() {
        let types = [
            GPIO_V2_LINE_CHANGED_REQUESTED,
            GPIO_V2_LINE_CHANGED_RELEASED,
            GPIO_V2_LINE_CHANGED_CONFIG,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}

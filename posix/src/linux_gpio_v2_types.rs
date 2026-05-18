//! `<linux/gpio.h>` (v2 line flags) — GPIO v2 API line attribute constants.
//!
//! The GPIO v2 API (introduced in Linux 5.10) replaces the v1 handle
//! and event interfaces with a unified line request API. It supports
//! multi-line requests, per-line configuration, debounce, and edge
//! detection in a single request.

// ---------------------------------------------------------------------------
// V2 line flags (GPIO_V2_LINE_FLAG_*)
// ---------------------------------------------------------------------------

/// Line is used (in use by kernel or userspace).
pub const GPIO_V2_LINE_FLAG_USED: u64 = 1 << 0;
/// Line is active-low.
pub const GPIO_V2_LINE_FLAG_ACTIVE_LOW: u64 = 1 << 1;
/// Line is an input.
pub const GPIO_V2_LINE_FLAG_INPUT: u64 = 1 << 2;
/// Line is an output.
pub const GPIO_V2_LINE_FLAG_OUTPUT: u64 = 1 << 3;
/// Line reports edge events.
pub const GPIO_V2_LINE_FLAG_EDGE_RISING: u64 = 1 << 4;
/// Line reports falling-edge events.
pub const GPIO_V2_LINE_FLAG_EDGE_FALLING: u64 = 1 << 5;
/// Line uses open-drain drive.
pub const GPIO_V2_LINE_FLAG_OPEN_DRAIN: u64 = 1 << 6;
/// Line uses open-source drive.
pub const GPIO_V2_LINE_FLAG_OPEN_SOURCE: u64 = 1 << 7;
/// Line has bias pull-up.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_UP: u64 = 1 << 8;
/// Line has bias pull-down.
pub const GPIO_V2_LINE_FLAG_BIAS_PULL_DOWN: u64 = 1 << 9;
/// Line has bias disabled.
pub const GPIO_V2_LINE_FLAG_BIAS_DISABLED: u64 = 1 << 10;
/// Timestamp events with realtime clock.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_REALTIME: u64 = 1 << 11;
/// Timestamp events with HTE clock.
pub const GPIO_V2_LINE_FLAG_EVENT_CLOCK_HTE: u64 = 1 << 12;

// ---------------------------------------------------------------------------
// V2 line attribute IDs (GPIO_V2_LINE_ATTR_ID_*)
// ---------------------------------------------------------------------------

/// Attribute: line flags.
pub const GPIO_V2_LINE_ATTR_ID_FLAGS: u32 = 1;
/// Attribute: output values.
pub const GPIO_V2_LINE_ATTR_ID_OUTPUT_VALUES: u32 = 2;
/// Attribute: debounce period (microseconds).
pub const GPIO_V2_LINE_ATTR_ID_DEBOUNCE: u32 = 3;

// ---------------------------------------------------------------------------
// V2 line change types (GPIO_V2_LINE_CHANGED_*)
// ---------------------------------------------------------------------------

/// Line was requested.
pub const GPIO_V2_LINE_CHANGED_REQUESTED: u32 = 1;
/// Line was released.
pub const GPIO_V2_LINE_CHANGED_RELEASED: u32 = 2;
/// Line configuration changed.
pub const GPIO_V2_LINE_CHANGED_CONFIG: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v2_flags_no_overlap() {
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
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_attr_ids_distinct() {
        let attrs = [
            GPIO_V2_LINE_ATTR_ID_FLAGS,
            GPIO_V2_LINE_ATTR_ID_OUTPUT_VALUES,
            GPIO_V2_LINE_ATTR_ID_DEBOUNCE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_change_types_distinct() {
        let changes = [
            GPIO_V2_LINE_CHANGED_REQUESTED,
            GPIO_V2_LINE_CHANGED_RELEASED,
            GPIO_V2_LINE_CHANGED_CONFIG,
        ];
        for i in 0..changes.len() {
            for j in (i + 1)..changes.len() {
                assert_ne!(changes[i], changes[j]);
            }
        }
    }
}

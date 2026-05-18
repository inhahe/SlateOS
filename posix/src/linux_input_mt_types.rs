//! `<linux/input.h>` (multi-touch subset) — MT tool types and protocol constants.
//!
//! Multi-touch (MT) input uses two protocols: type A (stateless,
//! SYN_MT_REPORT between contacts) and type B (stateful, ABS_MT_SLOT
//! selects active contact, ABS_MT_TRACKING_ID for lifecycle). Type B
//! is preferred for modern touchscreens. These constants define the
//! tool types reported by MT contacts and protocol parameters.

// ---------------------------------------------------------------------------
// MT tool types (ABS_MT_TOOL_TYPE values)
// ---------------------------------------------------------------------------

/// Contact is a finger.
pub const MT_TOOL_FINGER: u16 = 0x00;
/// Contact is a pen / stylus.
pub const MT_TOOL_PEN: u16 = 0x01;
/// Contact is a palm (to be rejected by userspace).
pub const MT_TOOL_PALM: u16 = 0x02;
/// Contact is a dial / rotary tool.
pub const MT_TOOL_DIAL: u16 = 0x0A;
/// Maximum tool type.
pub const MT_TOOL_MAX: u16 = 0x0F;

// ---------------------------------------------------------------------------
// MT tracking ID special values
// ---------------------------------------------------------------------------

/// Tracking ID value meaning "contact lifted" (type-B protocol).
pub const MT_TRACKING_ID_NONE: i32 = -1;

// ---------------------------------------------------------------------------
// MT protocol type constants (for driver authors)
// ---------------------------------------------------------------------------

/// Type-A protocol: stateless, one SYN_MT_REPORT per contact.
pub const INPUT_MT_POINTER: u32 = 0x0001;
/// Type-B protocol: stateful, uses slots.
pub const INPUT_MT_DIRECT: u32 = 0x0002;
/// Device may produce spurious contacts (noise).
pub const INPUT_MT_DROP_UNUSED: u32 = 0x0004;
/// Device tracks contacts internally (hardware tracking).
pub const INPUT_MT_TRACK: u32 = 0x0008;
/// Semi-MT: only bounding box of two contacts.
pub const INPUT_MT_SEMI_MT: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_types_distinct() {
        let tools = [MT_TOOL_FINGER, MT_TOOL_PEN, MT_TOOL_PALM, MT_TOOL_DIAL];
        for i in 0..tools.len() {
            for j in (i + 1)..tools.len() {
                assert_ne!(tools[i], tools[j],
                    "MT tool types {} and {} collide", i, j);
            }
        }
    }

    #[test]
    fn test_finger_is_default() {
        assert_eq!(MT_TOOL_FINGER, 0);
    }

    #[test]
    fn test_tools_within_max() {
        assert!(MT_TOOL_FINGER <= MT_TOOL_MAX);
        assert!(MT_TOOL_PEN <= MT_TOOL_MAX);
        assert!(MT_TOOL_PALM <= MT_TOOL_MAX);
        assert!(MT_TOOL_DIAL <= MT_TOOL_MAX);
    }

    #[test]
    fn test_tracking_id_none() {
        assert_eq!(MT_TRACKING_ID_NONE, -1);
    }

    #[test]
    fn test_protocol_flags_no_overlap() {
        let flags = [
            INPUT_MT_POINTER, INPUT_MT_DIRECT,
            INPUT_MT_DROP_UNUSED, INPUT_MT_TRACK,
            INPUT_MT_SEMI_MT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two(),
                "flag 0x{:04X} is not power of two", flags[i]);
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0,
                    "flags 0x{:04X} and 0x{:04X} overlap", flags[i], flags[j]);
            }
        }
    }
}

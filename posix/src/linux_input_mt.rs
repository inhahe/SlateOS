//! `<linux/input/mt.h>` — Multi-touch input protocol constants.
//!
//! The multi-touch (MT) protocol defines how touchscreens and
//! trackpads report multiple simultaneous contacts. Protocol A
//! (anonymous) reports all contacts per frame; Protocol B (slotted)
//! tracks individual contacts via tracking IDs.

// ---------------------------------------------------------------------------
// MT tool types
// ---------------------------------------------------------------------------

/// Finger touch.
pub const MT_TOOL_FINGER: u16 = 0;
/// Pen / stylus.
pub const MT_TOOL_PEN: u16 = 1;
/// Palm (for rejection).
pub const MT_TOOL_PALM: u16 = 2;
/// Maximum tool type value.
pub const MT_TOOL_MAX: u16 = 2;

// ---------------------------------------------------------------------------
// MT input flags (input_mt_init_slots flags)
// ---------------------------------------------------------------------------

/// Report pointer emulation events.
pub const INPUT_MT_POINTER: u32 = 0x0001;
/// Device is direct-touch (touchscreen, not touchpad).
pub const INPUT_MT_DIRECT: u32 = 0x0002;
/// Drop unused slots.
pub const INPUT_MT_DROP_UNUSED: u32 = 0x0004;
/// Track contacts (assign tracking IDs).
pub const INPUT_MT_TRACK: u32 = 0x0008;
/// Semi-MT device (two contacts, but no individual position).
pub const INPUT_MT_SEMI_MT: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Slot state
// ---------------------------------------------------------------------------

/// Slot is inactive (no contact).
pub const MT_SLOT_INACTIVE: i32 = -1;

// ---------------------------------------------------------------------------
// Protocol B event codes (ABS_MT_*)
// ---------------------------------------------------------------------------

/// Multi-touch slot index.
pub const ABS_MT_SLOT: u16 = 0x2F;
/// Touch major axis.
pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;
/// Touch minor axis.
pub const ABS_MT_TOUCH_MINOR: u16 = 0x31;
/// Width major.
pub const ABS_MT_WIDTH_MAJOR: u16 = 0x32;
/// Width minor.
pub const ABS_MT_WIDTH_MINOR: u16 = 0x33;
/// Orientation.
pub const ABS_MT_ORIENTATION: u16 = 0x34;
/// X position.
pub const ABS_MT_POSITION_X: u16 = 0x35;
/// Y position.
pub const ABS_MT_POSITION_Y: u16 = 0x36;
/// Tool type (finger/pen/palm).
pub const ABS_MT_TOOL_TYPE: u16 = 0x37;
/// Blob ID (group contacts).
pub const ABS_MT_BLOB_ID: u16 = 0x38;
/// Tracking ID (-1 = lift off).
pub const ABS_MT_TRACKING_ID: u16 = 0x39;
/// Pressure.
pub const ABS_MT_PRESSURE: u16 = 0x3A;
/// Distance from surface.
pub const ABS_MT_DISTANCE: u16 = 0x3B;
/// Tool X (center of tool).
pub const ABS_MT_TOOL_X: u16 = 0x3C;
/// Tool Y.
pub const ABS_MT_TOOL_Y: u16 = 0x3D;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_types_distinct() {
        let tools = [MT_TOOL_FINGER, MT_TOOL_PEN, MT_TOOL_PALM];
        for i in 0..tools.len() {
            for j in (i + 1)..tools.len() {
                assert_ne!(tools[i], tools[j]);
            }
        }
    }

    #[test]
    fn test_tool_types_in_range() {
        assert!(MT_TOOL_FINGER <= MT_TOOL_MAX);
        assert!(MT_TOOL_PEN <= MT_TOOL_MAX);
        assert!(MT_TOOL_PALM <= MT_TOOL_MAX);
    }

    #[test]
    fn test_input_mt_flags_no_overlap() {
        let flags = [
            INPUT_MT_POINTER, INPUT_MT_DIRECT,
            INPUT_MT_DROP_UNUSED, INPUT_MT_TRACK, INPUT_MT_SEMI_MT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_abs_mt_codes_distinct() {
        let codes = [
            ABS_MT_SLOT, ABS_MT_TOUCH_MAJOR, ABS_MT_TOUCH_MINOR,
            ABS_MT_WIDTH_MAJOR, ABS_MT_WIDTH_MINOR, ABS_MT_ORIENTATION,
            ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_TOOL_TYPE,
            ABS_MT_BLOB_ID, ABS_MT_TRACKING_ID, ABS_MT_PRESSURE,
            ABS_MT_DISTANCE, ABS_MT_TOOL_X, ABS_MT_TOOL_Y,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_slot_inactive() {
        assert_eq!(MT_SLOT_INACTIVE, -1);
    }
}

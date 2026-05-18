//! `<linux/input-event-codes.h>` (ABS subset) — absolute axis event codes.
//!
//! Absolute axis events report the current position rather than
//! displacement. Touchscreens, digitiser tablets, joysticks, and
//! game controllers are the primary producers. Each axis has a
//! fixed range (min/max) reported via `EVIOCGABS` ioctl; the event
//! value is always within that range.

// ---------------------------------------------------------------------------
// Primary axes (pointer / touch)
// ---------------------------------------------------------------------------

/// Absolute X position.
pub const ABS_X: u16 = 0x00;
/// Absolute Y position.
pub const ABS_Y: u16 = 0x01;
/// Absolute Z position (pressure or altitude).
pub const ABS_Z: u16 = 0x02;
/// Rotation around X axis.
pub const ABS_RX: u16 = 0x03;
/// Rotation around Y axis.
pub const ABS_RY: u16 = 0x04;
/// Rotation around Z axis.
pub const ABS_RZ: u16 = 0x05;
/// Throttle.
pub const ABS_THROTTLE: u16 = 0x06;
/// Rudder.
pub const ABS_RUDDER: u16 = 0x07;
/// Wheel (steering).
pub const ABS_WHEEL: u16 = 0x08;
/// Gas pedal.
pub const ABS_GAS: u16 = 0x09;
/// Brake pedal.
pub const ABS_BRAKE: u16 = 0x0A;

// ---------------------------------------------------------------------------
// Hat (D-pad) axes
// ---------------------------------------------------------------------------

/// Hat switch 0 X axis (left/right).
pub const ABS_HAT0X: u16 = 0x10;
/// Hat switch 0 Y axis (up/down).
pub const ABS_HAT0Y: u16 = 0x11;
/// Hat switch 1 X axis.
pub const ABS_HAT1X: u16 = 0x12;
/// Hat switch 1 Y axis.
pub const ABS_HAT1Y: u16 = 0x13;
/// Hat switch 2 X axis.
pub const ABS_HAT2X: u16 = 0x14;
/// Hat switch 2 Y axis.
pub const ABS_HAT2Y: u16 = 0x15;
/// Hat switch 3 X axis.
pub const ABS_HAT3X: u16 = 0x16;
/// Hat switch 3 Y axis.
pub const ABS_HAT3Y: u16 = 0x17;

// ---------------------------------------------------------------------------
// Touch / pen axes
// ---------------------------------------------------------------------------

/// Pressure (touch or pen).
pub const ABS_PRESSURE: u16 = 0x18;
/// Contact distance (hover height).
pub const ABS_DISTANCE: u16 = 0x19;
/// Tilt along X axis.
pub const ABS_TILT_X: u16 = 0x1A;
/// Tilt along Y axis.
pub const ABS_TILT_Y: u16 = 0x1B;
/// Tool width (contact area).
pub const ABS_TOOL_WIDTH: u16 = 0x1C;

// ---------------------------------------------------------------------------
// Volume
// ---------------------------------------------------------------------------

/// Volume (e.g. audio knob).
pub const ABS_VOLUME: u16 = 0x20;

// ---------------------------------------------------------------------------
// Multi-touch (MT) axes
// ---------------------------------------------------------------------------

/// MT: slot number (for type-B multi-touch protocol).
pub const ABS_MT_SLOT: u16 = 0x2F;
/// MT: major axis of bounding ellipse.
pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;
/// MT: minor axis of bounding ellipse.
pub const ABS_MT_TOUCH_MINOR: u16 = 0x31;
/// MT: major axis of approaching tool.
pub const ABS_MT_WIDTH_MAJOR: u16 = 0x32;
/// MT: minor axis of approaching tool.
pub const ABS_MT_WIDTH_MINOR: u16 = 0x33;
/// MT: ellipse orientation.
pub const ABS_MT_ORIENTATION: u16 = 0x34;
/// MT: centre X position.
pub const ABS_MT_POSITION_X: u16 = 0x35;
/// MT: centre Y position.
pub const ABS_MT_POSITION_Y: u16 = 0x36;
/// MT: type of touching tool (finger, pen, etc.).
pub const ABS_MT_TOOL_TYPE: u16 = 0x37;
/// MT: maximum area of contact blob.
pub const ABS_MT_BLOB_ID: u16 = 0x38;
/// MT: unique tracking ID per contact.
pub const ABS_MT_TRACKING_ID: u16 = 0x39;
/// MT: pressure on contact area.
pub const ABS_MT_PRESSURE: u16 = 0x3A;
/// MT: distance from surface.
pub const ABS_MT_DISTANCE: u16 = 0x3B;
/// MT: centre X tool position.
pub const ABS_MT_TOOL_X: u16 = 0x3C;
/// MT: centre Y tool position.
pub const ABS_MT_TOOL_Y: u16 = 0x3D;

// ---------------------------------------------------------------------------
// Miscellaneous
// ---------------------------------------------------------------------------

/// Miscellaneous absolute axis.
pub const ABS_MISC: u16 = 0x28;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum absolute axis code.
pub const ABS_MAX: u16 = 0x3F;
/// Number of absolute axis codes (ABS_MAX + 1).
pub const ABS_CNT: u16 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primary_axes_sequential() {
        assert_eq!(ABS_X, 0);
        assert_eq!(ABS_Y, 1);
        assert_eq!(ABS_Z, 2);
        assert_eq!(ABS_RX, 3);
        assert_eq!(ABS_RY, 4);
        assert_eq!(ABS_RZ, 5);
    }

    #[test]
    fn test_hat_pairs() {
        assert_eq!(ABS_HAT0Y, ABS_HAT0X + 1);
        assert_eq!(ABS_HAT1Y, ABS_HAT1X + 1);
        assert_eq!(ABS_HAT2Y, ABS_HAT2X + 1);
        assert_eq!(ABS_HAT3Y, ABS_HAT3X + 1);
    }

    #[test]
    fn test_mt_axes_sequential() {
        assert_eq!(ABS_MT_TOUCH_MAJOR, 0x30);
        assert_eq!(ABS_MT_TOUCH_MINOR, 0x31);
        assert_eq!(ABS_MT_WIDTH_MAJOR, 0x32);
        assert_eq!(ABS_MT_WIDTH_MINOR, 0x33);
        assert_eq!(ABS_MT_ORIENTATION, 0x34);
        assert_eq!(ABS_MT_POSITION_X, 0x35);
        assert_eq!(ABS_MT_POSITION_Y, 0x36);
    }

    #[test]
    fn test_mt_slot_before_mt_axes() {
        assert!(ABS_MT_SLOT < ABS_MT_TOUCH_MAJOR);
    }

    #[test]
    fn test_all_within_max() {
        let axes = [
            ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ,
            ABS_THROTTLE, ABS_RUDDER, ABS_WHEEL, ABS_GAS, ABS_BRAKE,
            ABS_HAT0X, ABS_HAT0Y, ABS_HAT1X, ABS_HAT1Y,
            ABS_HAT2X, ABS_HAT2Y, ABS_HAT3X, ABS_HAT3Y,
            ABS_PRESSURE, ABS_DISTANCE, ABS_TILT_X, ABS_TILT_Y,
            ABS_TOOL_WIDTH, ABS_VOLUME, ABS_MISC,
            ABS_MT_SLOT, ABS_MT_TOUCH_MAJOR, ABS_MT_TOUCH_MINOR,
            ABS_MT_WIDTH_MAJOR, ABS_MT_WIDTH_MINOR,
            ABS_MT_ORIENTATION, ABS_MT_POSITION_X, ABS_MT_POSITION_Y,
            ABS_MT_TOOL_TYPE, ABS_MT_BLOB_ID, ABS_MT_TRACKING_ID,
            ABS_MT_PRESSURE, ABS_MT_DISTANCE,
            ABS_MT_TOOL_X, ABS_MT_TOOL_Y,
        ];
        for &a in &axes {
            assert!(a <= ABS_MAX, "axis 0x{:02X} exceeds ABS_MAX", a);
        }
    }

    #[test]
    fn test_abs_cnt() {
        assert_eq!(ABS_CNT, ABS_MAX + 1);
    }
}

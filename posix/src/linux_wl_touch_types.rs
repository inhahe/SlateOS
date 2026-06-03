//! Wayland `wl_touch` — touch event type constants.
//!
//! The `wl_touch` interface handles multi-touch input on touchscreens
//! and touchpads. Each contact has an ID that persists from down to
//! up. Events within a frame are delivered atomically between
//! `wl_touch.frame` boundaries.

// ---------------------------------------------------------------------------
// Touch event types (used internally, not in wire protocol)
// ---------------------------------------------------------------------------

/// Touch point went down (new contact).
pub const WL_TOUCH_DOWN: u32 = 0;
/// Touch point went up (contact lifted).
pub const WL_TOUCH_UP: u32 = 1;
/// Touch point moved.
pub const WL_TOUCH_MOTION: u32 = 2;
/// End of a set of simultaneous events.
pub const WL_TOUCH_FRAME: u32 = 3;
/// Touch sequence cancelled by compositor.
pub const WL_TOUCH_CANCEL: u32 = 4;
/// Shape of touch contact (ellipse axes).
pub const WL_TOUCH_SHAPE: u32 = 5;
/// Orientation of touch contact ellipse.
pub const WL_TOUCH_ORIENTATION: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_touch_events_distinct() {
        let events = [
            WL_TOUCH_DOWN,
            WL_TOUCH_UP,
            WL_TOUCH_MOTION,
            WL_TOUCH_FRAME,
            WL_TOUCH_CANCEL,
            WL_TOUCH_SHAPE,
            WL_TOUCH_ORIENTATION,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_touch_events_sequential() {
        assert_eq!(WL_TOUCH_DOWN, 0);
        assert_eq!(WL_TOUCH_UP, 1);
        assert_eq!(WL_TOUCH_MOTION, 2);
        assert_eq!(WL_TOUCH_FRAME, 3);
        assert_eq!(WL_TOUCH_CANCEL, 4);
        assert_eq!(WL_TOUCH_SHAPE, 5);
        assert_eq!(WL_TOUCH_ORIENTATION, 6);
    }

    #[test]
    fn test_lifecycle_order() {
        assert!(WL_TOUCH_DOWN < WL_TOUCH_UP);
        assert!(WL_TOUCH_DOWN < WL_TOUCH_MOTION);
    }
}

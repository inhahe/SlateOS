//! `<linux/videodev2.h>` — V4L2 event-subscription constants.
//!
//! V4L2 lets userspace subscribe to per-device events
//! (`VIDIOC_SUBSCRIBE_EVENT`) so capture pipelines can react to
//! resolution changes, control-value updates, source changes, frame
//! sync, and motion-detection triggers without polling. Constants
//! below cover the event-type IDs, subscription flags, and the
//! source-change cause bitmap.

// ---------------------------------------------------------------------------
// Event types (struct v4l2_event_subscription.type)
// ---------------------------------------------------------------------------

/// "All" pseudo-type used by `VIDIOC_UNSUBSCRIBE_EVENT` to drop every
/// subscription at once.
pub const V4L2_EVENT_ALL: u32 = 0;
/// V-sync / new-frame event.
pub const V4L2_EVENT_VSYNC: u32 = 1;
/// End-of-stream event.
pub const V4L2_EVENT_EOS: u32 = 2;
/// Control-value change.
pub const V4L2_EVENT_CTRL: u32 = 3;
/// Frame-sync (used by ISP/sensor pipelines).
pub const V4L2_EVENT_FRAME_SYNC: u32 = 4;
/// Source change (resolution, framerate, bus parameters).
pub const V4L2_EVENT_SOURCE_CHANGE: u32 = 5;
/// Motion-detection trigger.
pub const V4L2_EVENT_MOTION_DET: u32 = 6;
/// Lowest free event ID for driver-private events.
pub const V4L2_EVENT_PRIVATE_START: u32 = 0x0800_0000;

// ---------------------------------------------------------------------------
// Subscription flags (struct v4l2_event_subscription.flags)
// ---------------------------------------------------------------------------

/// Deliver the current value immediately after subscription.
pub const V4L2_EVENT_SUB_FL_SEND_INITIAL: u32 = 1 << 0;
/// Allow this subscription to also receive change events caused by
/// the subscribing fd itself.
pub const V4L2_EVENT_SUB_FL_ALLOW_FEEDBACK: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Control-event change-flags (struct v4l2_event_ctrl.changes)
// ---------------------------------------------------------------------------

/// Control value changed.
pub const V4L2_EVENT_CTRL_CH_VALUE: u32 = 1 << 0;
/// Control flags (e.g., disabled, inactive) changed.
pub const V4L2_EVENT_CTRL_CH_FLAGS: u32 = 1 << 1;
/// Control range changed.
pub const V4L2_EVENT_CTRL_CH_RANGE: u32 = 1 << 2;
/// Control dimensions changed.
pub const V4L2_EVENT_CTRL_CH_DIMENSIONS: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Source-change "changes" bitmap (struct v4l2_event_src_change.changes)
// ---------------------------------------------------------------------------

/// Resolution / colorspace / framerate changed.
pub const V4L2_EVENT_SRC_CH_RESOLUTION: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Queue depth for buffered events (driver-private start floor)
// ---------------------------------------------------------------------------

/// Maximum events the kernel queues per subscription before dropping.
pub const V4L2_EVENT_QUEUE_MAX: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_distinct_and_private_above_well_known() {
        let e = [
            V4L2_EVENT_VSYNC,
            V4L2_EVENT_EOS,
            V4L2_EVENT_CTRL,
            V4L2_EVENT_FRAME_SYNC,
            V4L2_EVENT_SOURCE_CHANGE,
            V4L2_EVENT_MOTION_DET,
        ];
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
            // All well-known event IDs must be below the private range.
            assert!(e[i] < V4L2_EVENT_PRIVATE_START);
        }
        // ALL is the unsubscribe wildcard.
        assert_eq!(V4L2_EVENT_ALL, 0);
    }

    #[test]
    fn test_subscription_flags_distinct_pow2() {
        let f = [
            V4L2_EVENT_SUB_FL_SEND_INITIAL,
            V4L2_EVENT_SUB_FL_ALLOW_FEEDBACK,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        assert_ne!(f[0], f[1]);
    }

    #[test]
    fn test_ctrl_change_bits_distinct_pow2() {
        let c = [
            V4L2_EVENT_CTRL_CH_VALUE,
            V4L2_EVENT_CTRL_CH_FLAGS,
            V4L2_EVENT_CTRL_CH_RANGE,
            V4L2_EVENT_CTRL_CH_DIMENSIONS,
        ];
        for &b in &c {
            assert!(b.is_power_of_two());
        }
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
        }
    }

    #[test]
    fn test_src_change_and_queue_max() {
        assert!(V4L2_EVENT_SRC_CH_RESOLUTION.is_power_of_two());
        // QUEUE_MAX must be > 0 and a power of two so the ring index
        // can use a bit-mask.
        assert!(V4L2_EVENT_QUEUE_MAX.is_power_of_two());
    }
}

//! `<linux/vbox_*.h>` — VirtualBox guest-additions ioctl constants.
//!
//! VirtualBox exposes guest-additions interfaces over `/dev/vboxguest`
//! and `/dev/vboxuser`. Userspace guest tools (VBoxClient, shared
//! folder mounter) consume these op-codes and event masks.

// ---------------------------------------------------------------------------
// VMM device request types (vbg_req_hdr.requestType)
// ---------------------------------------------------------------------------

/// Get mouse status.
pub const VBG_REQ_MOUSE_STATUS: u32 = 1;
/// Set mouse status.
pub const VBG_REQ_SET_MOUSE_STATUS: u32 = 2;
/// Set mouse pointer image.
pub const VBG_REQ_SET_POINTER_SHAPE: u32 = 3;
/// Get host display change request.
pub const VBG_REQ_DISPLAY_CHANGE: u32 = 4;
/// Get host time.
pub const VBG_REQ_HOST_TIME: u32 = 10;
/// Acknowledge event.
pub const VBG_REQ_ACK_EVENTS: u32 = 41;
/// Send event filter mask.
pub const VBG_REQ_CTL_FILTER_MASK: u32 = 42;

// ---------------------------------------------------------------------------
// Host-event flag bits (VMM device events, ack mask)
// ---------------------------------------------------------------------------

/// Mouse position changed.
pub const VBG_EVENT_MOUSE_POSITION_CHANGED: u32 = 1 << 0;
/// Host wants to change resolution.
pub const VBG_EVENT_DISPLAY_CHANGE: u32 = 1 << 1;
/// Shared-folder mount points changed.
pub const VBG_EVENT_HGCM: u32 = 1 << 2;
/// VRDP state changed.
pub const VBG_EVENT_VRDP: u32 = 1 << 3;
/// Judge credentials request.
pub const VBG_EVENT_JUDGE_CREDENTIALS: u32 = 1 << 4;
/// Restored from a saved state.
pub const VBG_EVENT_RESTORED: u32 = 1 << 5;
/// Seamless mode toggled.
pub const VBG_EVENT_SEAMLESS_MODE_CHANGED: u32 = 1 << 6;
/// Balloon-driver size changed.
pub const VBG_EVENT_BALLOON_CHANGE: u32 = 1 << 7;
/// Statistics report due.
pub const VBG_EVENT_STATISTICS_INTERVAL_CHANGE: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Pointer-shape flags (struct vmmdev_pointer_shape.flags)
// ---------------------------------------------------------------------------

/// Pointer is visible.
pub const VBG_POINTER_VISIBLE: u32 = 0x0001;
/// Pointer shape contains alpha.
pub const VBG_POINTER_ALPHA: u32 = 0x0002;
/// Pointer shape is in this request.
pub const VBG_POINTER_SHAPE: u32 = 0x0004;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_req_types_distinct() {
        let reqs = [
            VBG_REQ_MOUSE_STATUS,
            VBG_REQ_SET_MOUSE_STATUS,
            VBG_REQ_SET_POINTER_SHAPE,
            VBG_REQ_DISPLAY_CHANGE,
            VBG_REQ_HOST_TIME,
            VBG_REQ_ACK_EVENTS,
            VBG_REQ_CTL_FILTER_MASK,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_event_bits_distinct_powers_of_two() {
        let events = [
            VBG_EVENT_MOUSE_POSITION_CHANGED,
            VBG_EVENT_DISPLAY_CHANGE,
            VBG_EVENT_HGCM,
            VBG_EVENT_VRDP,
            VBG_EVENT_JUDGE_CREDENTIALS,
            VBG_EVENT_RESTORED,
            VBG_EVENT_SEAMLESS_MODE_CHANGED,
            VBG_EVENT_BALLOON_CHANGE,
            VBG_EVENT_STATISTICS_INTERVAL_CHANGE,
        ];
        for &e in &events {
            assert!(e.is_power_of_two());
        }
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_pointer_flags_distinct_powers_of_two() {
        let flags = [VBG_POINTER_VISIBLE, VBG_POINTER_ALPHA, VBG_POINTER_SHAPE];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}

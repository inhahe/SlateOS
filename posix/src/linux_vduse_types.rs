//! `<linux/vduse.h>` — vDPA Device in Userspace (VDUSE) constants.
//!
//! VDUSE lets userspace implement a virtio device backend that
//! plugs into the in-kernel vDPA bus. Userspace processes (DPDK,
//! SPDK) consume these ioctls and events.

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of virtqueues per VDUSE device.
pub const VDUSE_MAX_VQS: u32 = 0x10000;
/// Length cap on the VDUSE device-name string (NUL-terminated).
pub const VDUSE_NAME_MAX: u32 = 256;
/// Maximum message size on the control channel (bytes).
pub const VDUSE_MSG_MAX_SIZE: u32 = 4096;

// ---------------------------------------------------------------------------
// Device-status bits (mirrors virtio status)
// ---------------------------------------------------------------------------

/// Device acknowledged.
pub const VDUSE_STATUS_ACKNOWLEDGE: u32 = 1;
/// Driver attached.
pub const VDUSE_STATUS_DRIVER: u32 = 2;
/// Driver fully usable.
pub const VDUSE_STATUS_DRIVER_OK: u32 = 4;
/// Feature negotiation complete.
pub const VDUSE_STATUS_FEATURES_OK: u32 = 8;
/// Device needs reset.
pub const VDUSE_STATUS_NEEDS_RESET: u32 = 0x40;
/// Device failed.
pub const VDUSE_STATUS_FAILED: u32 = 0x80;

// ---------------------------------------------------------------------------
// VDUSE message types (struct vduse_dev_request.type)
// ---------------------------------------------------------------------------

/// Get device features.
pub const VDUSE_GET_VQ_STATE: u32 = 0x01;
/// Set device-status byte.
pub const VDUSE_SET_STATUS: u32 = 0x02;
/// Update memory-table mapping.
pub const VDUSE_UPDATE_IOTLB: u32 = 0x03;

// ---------------------------------------------------------------------------
// VQ state types (struct vduse_vq_state.type)
// ---------------------------------------------------------------------------

/// Split virtqueue state.
pub const VDUSE_VQ_STATE_TYPE_SPLIT: u32 = 0x00;
/// Packed virtqueue state.
pub const VDUSE_VQ_STATE_TYPE_PACKED: u32 = 0x01;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limits_sane() {
        assert!(VDUSE_MAX_VQS >= 1);
        assert!(VDUSE_NAME_MAX >= 16);
        assert!(VDUSE_MSG_MAX_SIZE >= 64);
    }

    #[test]
    fn test_status_bits_distinct_powers_of_two() {
        let bits = [
            VDUSE_STATUS_ACKNOWLEDGE,
            VDUSE_STATUS_DRIVER,
            VDUSE_STATUS_DRIVER_OK,
            VDUSE_STATUS_FEATURES_OK,
            VDUSE_STATUS_NEEDS_RESET,
            VDUSE_STATUS_FAILED,
        ];
        for &b in &bits {
            assert!(b.is_power_of_two());
        }
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [VDUSE_GET_VQ_STATE, VDUSE_SET_STATUS, VDUSE_UPDATE_IOTLB];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_vq_state_types_distinct() {
        assert_ne!(VDUSE_VQ_STATE_TYPE_SPLIT, VDUSE_VQ_STATE_TYPE_PACKED);
    }
}

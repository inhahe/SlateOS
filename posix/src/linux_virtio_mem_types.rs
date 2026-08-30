//! `<linux/virtio_mem.h>` — VirtIO memory device constants.
//!
//! virtio-mem provides dynamic memory hotplug for VMs. Unlike
//! the balloon (which reclaims existing pages), virtio-mem adds
//! or removes memory blocks from the guest's physical address
//! space. Blocks can be plugged (made available) or unplugged
//! (returned to host) at runtime.

// ---------------------------------------------------------------------------
// Request types (virtio_mem_req.type)
// ---------------------------------------------------------------------------

/// Plug memory blocks (make available to guest).
pub const VIRTIO_MEM_REQ_PLUG: u16 = 0;
/// Unplug memory blocks (return to host).
pub const VIRTIO_MEM_REQ_UNPLUG: u16 = 1;
/// Unplug all memory blocks.
pub const VIRTIO_MEM_REQ_UNPLUG_ALL: u16 = 2;
/// Query block state.
pub const VIRTIO_MEM_REQ_STATE: u16 = 3;

// ---------------------------------------------------------------------------
// Response types (virtio_mem_resp.type)
// ---------------------------------------------------------------------------

/// Request succeeded.
pub const VIRTIO_MEM_RESP_ACK: u16 = 0;
/// Request rejected (busy or not possible).
pub const VIRTIO_MEM_RESP_NACK: u16 = 1;
/// Device is busy (try again later).
pub const VIRTIO_MEM_RESP_BUSY: u16 = 2;
/// Error: generic failure.
pub const VIRTIO_MEM_RESP_ERROR: u16 = 3;

// ---------------------------------------------------------------------------
// Block states (virtio_mem_resp_state.state)
// ---------------------------------------------------------------------------

/// Block is plugged (available to guest).
pub const VIRTIO_MEM_STATE_PLUGGED: u16 = 0;
/// Block is unplugged (not available).
pub const VIRTIO_MEM_STATE_UNPLUGGED: u16 = 1;
/// Block state is mixed (partially plugged).
pub const VIRTIO_MEM_STATE_MIXED: u16 = 2;

// ---------------------------------------------------------------------------
// Feature bits
// ---------------------------------------------------------------------------

/// Device supports unplugged inaccessible memory.
pub const VIRTIO_MEM_F_ACPI_PXM: u64 = 1 << 0;
/// Device supports persistent suspend.
pub const VIRTIO_MEM_F_UNPLUGGED_INACCESSIBLE: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_req_types_distinct() {
        let reqs = [
            VIRTIO_MEM_REQ_PLUG,
            VIRTIO_MEM_REQ_UNPLUG,
            VIRTIO_MEM_REQ_UNPLUG_ALL,
            VIRTIO_MEM_REQ_STATE,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_resp_types_distinct() {
        let resps = [
            VIRTIO_MEM_RESP_ACK,
            VIRTIO_MEM_RESP_NACK,
            VIRTIO_MEM_RESP_BUSY,
            VIRTIO_MEM_RESP_ERROR,
        ];
        for i in 0..resps.len() {
            for j in (i + 1)..resps.len() {
                assert_ne!(resps[i], resps[j]);
            }
        }
    }

    #[test]
    fn test_block_states_distinct() {
        assert_ne!(VIRTIO_MEM_STATE_PLUGGED, VIRTIO_MEM_STATE_UNPLUGGED);
        assert_ne!(VIRTIO_MEM_STATE_UNPLUGGED, VIRTIO_MEM_STATE_MIXED);
        assert_ne!(VIRTIO_MEM_STATE_PLUGGED, VIRTIO_MEM_STATE_MIXED);
    }

    #[test]
    fn test_feature_bits_no_overlap() {
        assert!(VIRTIO_MEM_F_ACPI_PXM.is_power_of_two());
        assert!(VIRTIO_MEM_F_UNPLUGGED_INACCESSIBLE.is_power_of_two());
        assert_eq!(
            VIRTIO_MEM_F_ACPI_PXM & VIRTIO_MEM_F_UNPLUGGED_INACCESSIBLE,
            0
        );
    }
}

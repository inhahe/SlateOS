//! `<linux/rpmsg.h>` — Remote Processor Messaging (rpmsg) constants.
//!
//! rpmsg provides message-passing IPC between the main CPU and
//! remote processors (managed by remoteproc). It uses VirtIO
//! transport over shared memory with a name-based service discovery
//! mechanism. Drivers register rpmsg endpoints with a service name;
//! when the remote creates a matching channel, the endpoints are
//! connected for bidirectional messaging.

// ---------------------------------------------------------------------------
// rpmsg endpoint states
// ---------------------------------------------------------------------------

/// Endpoint is registered but not connected.
pub const RPMSG_STATE_REGISTERED: u32 = 0;
/// Endpoint is connected (channel established).
pub const RPMSG_STATE_CONNECTED: u32 = 1;
/// Endpoint is disconnected (channel closed).
pub const RPMSG_STATE_DISCONNECTED: u32 = 2;

// ---------------------------------------------------------------------------
// rpmsg message header values
// ---------------------------------------------------------------------------

/// Maximum message payload size (default, 512 bytes).
pub const RPMSG_BUF_SIZE: u32 = 512;
/// Name service announcement message type.
pub const RPMSG_NS_ANNOUNCEMENT: u32 = 0x35;
/// Reserved source address (broadcast).
pub const RPMSG_ADDR_ANY: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// rpmsg name service flags
// ---------------------------------------------------------------------------

/// Service is being created (new channel).
pub const RPMSG_NS_CREATE: u32 = 0;
/// Service is being destroyed (channel removed).
pub const RPMSG_NS_DESTROY: u32 = 1;

// ---------------------------------------------------------------------------
// rpmsg channel flags
// ---------------------------------------------------------------------------

/// Channel supports zero-copy (shared buffer mapping).
pub const RPMSG_FLAG_ZEROCOPY: u32 = 1 << 0;
/// Channel is flow-controlled (backpressure support).
pub const RPMSG_FLAG_FLOW_CONTROL: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// rpmsg transport types
// ---------------------------------------------------------------------------

/// VirtIO transport (default, over shared memory).
pub const RPMSG_TRANSPORT_VIRTIO: u32 = 0;
/// GLink transport (Qualcomm proprietary).
pub const RPMSG_TRANSPORT_GLINK: u32 = 1;
/// SMD transport (Qualcomm legacy).
pub const RPMSG_TRANSPORT_SMD: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            RPMSG_STATE_REGISTERED,
            RPMSG_STATE_CONNECTED,
            RPMSG_STATE_DISCONNECTED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_ns_flags_distinct() {
        assert_ne!(RPMSG_NS_CREATE, RPMSG_NS_DESTROY);
    }

    #[test]
    fn test_channel_flags_no_overlap() {
        let flags = [RPMSG_FLAG_ZEROCOPY, RPMSG_FLAG_FLOW_CONTROL];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_transport_types_distinct() {
        let types = [
            RPMSG_TRANSPORT_VIRTIO,
            RPMSG_TRANSPORT_GLINK,
            RPMSG_TRANSPORT_SMD,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_buf_size() {
        assert_eq!(RPMSG_BUF_SIZE, 512);
        assert!(RPMSG_BUF_SIZE.is_power_of_two());
    }
}

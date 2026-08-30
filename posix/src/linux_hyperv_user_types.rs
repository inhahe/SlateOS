//! `<linux/hyperv.h>` — Microsoft Hyper-V guest-side VMBus ABI.
//!
//! When Linux runs as a Hyper-V VM (Azure, on-prem Hyper-V), the
//! `hv_vmbus` driver multiplexes per-channel devices: synthetic NIC
//! (`netvsc`), SCSI (`storvsc`), framebuffer (`hyperv_fb`), KVP, VSS,
//! file-copy daemon, and PCI passthrough. Userspace daemons in the
//! `hyperv-daemons` package consume the message types and version
//! constants below.

// ---------------------------------------------------------------------------
// Negotiated framework version (Hv_* in `vmbus_request_offers` reply)
// ---------------------------------------------------------------------------

/// VMBus version Windows Server 2008 (Vista).
pub const VMBUS_VERSION_WS2008: u32 = (0 << 16) | 13;
/// VMBus version Windows 7.
pub const VMBUS_VERSION_WIN7: u32 = (1 << 16) | 1;
/// VMBus version Windows 8 / Windows Server 2012.
pub const VMBUS_VERSION_WIN8: u32 = (2 << 16) | 4;
/// VMBus version Windows 8.1 / 2012 R2.
pub const VMBUS_VERSION_WIN8_1: u32 = (3 << 16) | 0;
/// VMBus version Windows 10.
pub const VMBUS_VERSION_WIN10: u32 = (4 << 16) | 0;
/// VMBus version Windows 10 v2 (RS3).
pub const VMBUS_VERSION_WIN10_V4_1: u32 = (4 << 16) | 1;
/// VMBus version Windows 10 v5 (Iron).
pub const VMBUS_VERSION_WIN10_V5: u32 = (5 << 16) | 0;

// ---------------------------------------------------------------------------
// VMBus channel-message types (`enum vmbus_channel_message_type`)
// ---------------------------------------------------------------------------

pub const CHANNELMSG_INVALID: u32 = 0;
pub const CHANNELMSG_OFFERCHANNEL: u32 = 1;
pub const CHANNELMSG_RESCIND_CHANNELOFFER: u32 = 2;
pub const CHANNELMSG_REQUESTOFFERS: u32 = 3;
pub const CHANNELMSG_ALLOFFERS_DELIVERED: u32 = 4;
pub const CHANNELMSG_OPENCHANNEL: u32 = 5;
pub const CHANNELMSG_OPENCHANNEL_RESULT: u32 = 6;
pub const CHANNELMSG_CLOSECHANNEL: u32 = 7;
pub const CHANNELMSG_GPADL_HEADER: u32 = 8;
pub const CHANNELMSG_GPADL_BODY: u32 = 9;
pub const CHANNELMSG_GPADL_CREATED: u32 = 10;
pub const CHANNELMSG_GPADL_TEARDOWN: u32 = 11;
pub const CHANNELMSG_GPADL_TORNDOWN: u32 = 12;
pub const CHANNELMSG_RELID_RELEASED: u32 = 13;
pub const CHANNELMSG_INITIATE_CONTACT: u32 = 14;
pub const CHANNELMSG_VERSION_RESPONSE: u32 = 15;
pub const CHANNELMSG_UNLOAD: u32 = 16;
pub const CHANNELMSG_UNLOAD_RESPONSE: u32 = 17;
pub const CHANNELMSG_18: u32 = 18;
pub const CHANNELMSG_19: u32 = 19;
pub const CHANNELMSG_20: u32 = 20;
pub const CHANNELMSG_TL_CONNECT_REQUEST: u32 = 21;

// ---------------------------------------------------------------------------
// VMBus driver-name constants
// ---------------------------------------------------------------------------

/// Userspace device path prefix for VMBus character devices.
pub const HV_DEV_PATH: &str = "/dev/vmbus";
/// Maximum channel-message payload size in bytes.
pub const VMBUS_MESSAGE_MAX_PAYLOAD: u32 = 240;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_monotonic() {
        // Newer Windows versions encode a larger major value.
        assert!(VMBUS_VERSION_WS2008 < VMBUS_VERSION_WIN7);
        assert!(VMBUS_VERSION_WIN7 < VMBUS_VERSION_WIN8);
        assert!(VMBUS_VERSION_WIN8 < VMBUS_VERSION_WIN8_1);
        assert!(VMBUS_VERSION_WIN8_1 < VMBUS_VERSION_WIN10);
        assert!(VMBUS_VERSION_WIN10 < VMBUS_VERSION_WIN10_V4_1);
        assert!(VMBUS_VERSION_WIN10_V4_1 < VMBUS_VERSION_WIN10_V5);
    }

    #[test]
    fn test_version_layout_major_minor() {
        // High 16 bits = major, low 16 bits = minor.
        assert_eq!(VMBUS_VERSION_WIN10 >> 16, 4);
        assert_eq!(VMBUS_VERSION_WIN10 & 0xFFFF, 0);
        assert_eq!(VMBUS_VERSION_WIN10_V4_1 >> 16, 4);
        assert_eq!(VMBUS_VERSION_WIN10_V4_1 & 0xFFFF, 1);
        assert_eq!(VMBUS_VERSION_WIN10_V5 >> 16, 5);
    }

    #[test]
    fn test_channelmsg_dense_0_to_21() {
        let m = [
            CHANNELMSG_INVALID,
            CHANNELMSG_OFFERCHANNEL,
            CHANNELMSG_RESCIND_CHANNELOFFER,
            CHANNELMSG_REQUESTOFFERS,
            CHANNELMSG_ALLOFFERS_DELIVERED,
            CHANNELMSG_OPENCHANNEL,
            CHANNELMSG_OPENCHANNEL_RESULT,
            CHANNELMSG_CLOSECHANNEL,
            CHANNELMSG_GPADL_HEADER,
            CHANNELMSG_GPADL_BODY,
            CHANNELMSG_GPADL_CREATED,
            CHANNELMSG_GPADL_TEARDOWN,
            CHANNELMSG_GPADL_TORNDOWN,
            CHANNELMSG_RELID_RELEASED,
            CHANNELMSG_INITIATE_CONTACT,
            CHANNELMSG_VERSION_RESPONSE,
            CHANNELMSG_UNLOAD,
            CHANNELMSG_UNLOAD_RESPONSE,
            CHANNELMSG_18,
            CHANNELMSG_19,
            CHANNELMSG_20,
            CHANNELMSG_TL_CONNECT_REQUEST,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_device_constants() {
        assert!(HV_DEV_PATH.starts_with("/dev/"));
        // Payload must leave room within a 256-byte SynIC slot.
        assert!(VMBUS_MESSAGE_MAX_PAYLOAD <= 240);
    }
}

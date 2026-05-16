//! `<linux/virtio_net.h>` — Virtio network device feature bits.
//!
//! Virtio-net is the standard paravirtualized network device.
//! Feature negotiation determines which offloads, multiqueue,
//! and header formats are supported between guest and host.

// ---------------------------------------------------------------------------
// Feature bits
// ---------------------------------------------------------------------------

/// Device has checksum offload.
pub const VIRTIO_NET_F_CSUM: u32 = 0;
/// Driver has checksum offload.
pub const VIRTIO_NET_F_GUEST_CSUM: u32 = 1;
/// Control channel VQ present.
pub const VIRTIO_NET_F_CTRL_VQ: u32 = 17;
/// Control channel RX mode.
pub const VIRTIO_NET_F_CTRL_RX: u32 = 18;
/// Control channel VLAN filtering.
pub const VIRTIO_NET_F_CTRL_VLAN: u32 = 19;
/// Device supports multiqueue.
pub const VIRTIO_NET_F_MQ: u32 = 22;
/// Device has MAC address.
pub const VIRTIO_NET_F_MAC: u32 = 5;
/// Device has GSO.
pub const VIRTIO_NET_F_GSO: u32 = 6;
/// Guest TSO4.
pub const VIRTIO_NET_F_GUEST_TSO4: u32 = 7;
/// Guest TSO6.
pub const VIRTIO_NET_F_GUEST_TSO6: u32 = 8;
/// Guest ECN.
pub const VIRTIO_NET_F_GUEST_ECN: u32 = 9;
/// Guest UFO.
pub const VIRTIO_NET_F_GUEST_UFO: u32 = 10;
/// Host TSO4.
pub const VIRTIO_NET_F_HOST_TSO4: u32 = 11;
/// Host TSO6.
pub const VIRTIO_NET_F_HOST_TSO6: u32 = 12;
/// Host ECN.
pub const VIRTIO_NET_F_HOST_ECN: u32 = 13;
/// Host UFO.
pub const VIRTIO_NET_F_HOST_UFO: u32 = 14;
/// Mergeable receive buffers.
pub const VIRTIO_NET_F_MRG_RXBUF: u32 = 15;
/// Device reports link status.
pub const VIRTIO_NET_F_STATUS: u32 = 16;
/// RSS hash support.
pub const VIRTIO_NET_F_RSS: u32 = 60;
/// Device speed/duplex report.
pub const VIRTIO_NET_F_SPEED_DUPLEX: u32 = 63;

// ---------------------------------------------------------------------------
// virtio_net_hdr flags
// ---------------------------------------------------------------------------

/// Needs checksum computation.
pub const VIRTIO_NET_HDR_F_NEEDS_CSUM: u8 = 1;
/// Data is valid (checksum verified).
pub const VIRTIO_NET_HDR_F_DATA_VALID: u8 = 2;
/// RSC coalesced info present.
pub const VIRTIO_NET_HDR_F_RSC_INFO: u8 = 4;

// ---------------------------------------------------------------------------
// virtio_net_hdr GSO types
// ---------------------------------------------------------------------------

/// No GSO.
pub const VIRTIO_NET_HDR_GSO_NONE: u8 = 0;
/// TCPv4 GSO.
pub const VIRTIO_NET_HDR_GSO_TCPV4: u8 = 1;
/// UDP GSO.
pub const VIRTIO_NET_HDR_GSO_UDP: u8 = 3;
/// TCPv6 GSO.
pub const VIRTIO_NET_HDR_GSO_TCPV6: u8 = 4;
/// UDP L4 GSO.
pub const VIRTIO_NET_HDR_GSO_UDP_L4: u8 = 5;
/// ECN bit in GSO type.
pub const VIRTIO_NET_HDR_GSO_ECN: u8 = 0x80;

// ---------------------------------------------------------------------------
// Link status
// ---------------------------------------------------------------------------

/// Link is up.
pub const VIRTIO_NET_S_LINK_UP: u16 = 1;
/// Announce needed (link changed).
pub const VIRTIO_NET_S_ANNOUNCE: u16 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_bits_distinct() {
        let features = [
            VIRTIO_NET_F_CSUM, VIRTIO_NET_F_GUEST_CSUM,
            VIRTIO_NET_F_MAC, VIRTIO_NET_F_GSO,
            VIRTIO_NET_F_GUEST_TSO4, VIRTIO_NET_F_GUEST_TSO6,
            VIRTIO_NET_F_GUEST_ECN, VIRTIO_NET_F_GUEST_UFO,
            VIRTIO_NET_F_HOST_TSO4, VIRTIO_NET_F_HOST_TSO6,
            VIRTIO_NET_F_HOST_ECN, VIRTIO_NET_F_HOST_UFO,
            VIRTIO_NET_F_MRG_RXBUF, VIRTIO_NET_F_STATUS,
            VIRTIO_NET_F_CTRL_VQ, VIRTIO_NET_F_CTRL_RX,
            VIRTIO_NET_F_CTRL_VLAN, VIRTIO_NET_F_MQ,
        ];
        for i in 0..features.len() {
            for j in (i + 1)..features.len() {
                assert_ne!(features[i], features[j]);
            }
        }
    }

    #[test]
    fn test_hdr_flags_distinct() {
        let flags = [
            VIRTIO_NET_HDR_F_NEEDS_CSUM,
            VIRTIO_NET_HDR_F_DATA_VALID,
            VIRTIO_NET_HDR_F_RSC_INFO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_gso_types_distinct() {
        let types = [
            VIRTIO_NET_HDR_GSO_NONE, VIRTIO_NET_HDR_GSO_TCPV4,
            VIRTIO_NET_HDR_GSO_UDP, VIRTIO_NET_HDR_GSO_TCPV6,
            VIRTIO_NET_HDR_GSO_UDP_L4,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_link_status() {
        assert_ne!(VIRTIO_NET_S_LINK_UP, VIRTIO_NET_S_ANNOUNCE);
    }
}

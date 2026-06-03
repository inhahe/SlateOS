//! `<linux/virtio_net.h>` — Virtio network device constants.
//!
//! Defines feature bits, header structures, and flags specific to
//! virtio-net devices used in virtual machines (QEMU/KVM, VirtualBox,
//! etc.).

pub use crate::linux_virtio_types::VIRTIO_ID_NET;

// ---------------------------------------------------------------------------
// Virtio-net feature bits
// ---------------------------------------------------------------------------

/// Device has checksum offload.
pub const VIRTIO_NET_F_CSUM: u32 = 0;
/// Driver handles partial checksum.
pub const VIRTIO_NET_F_GUEST_CSUM: u32 = 1;
/// Control channel available.
pub const VIRTIO_NET_F_CTRL_VQ: u32 = 17;
/// Control channel: RX mode.
pub const VIRTIO_NET_F_CTRL_RX: u32 = 18;
/// Control channel: VLAN filtering.
pub const VIRTIO_NET_F_CTRL_VLAN: u32 = 19;
/// Guest can handle TSO for IPv4.
pub const VIRTIO_NET_F_GUEST_TSO4: u32 = 7;
/// Guest can handle TSO for IPv6.
pub const VIRTIO_NET_F_GUEST_TSO6: u32 = 8;
/// Guest can handle TSO with ECN.
pub const VIRTIO_NET_F_GUEST_ECN: u32 = 9;
/// Guest can handle UFO.
pub const VIRTIO_NET_F_GUEST_UFO: u32 = 10;
/// Host can handle TSO for IPv4.
pub const VIRTIO_NET_F_HOST_TSO4: u32 = 11;
/// Host can handle TSO for IPv6.
pub const VIRTIO_NET_F_HOST_TSO6: u32 = 12;
/// Host can handle UFO.
pub const VIRTIO_NET_F_HOST_UFO: u32 = 14;
/// Device has merge-able rx buffers.
pub const VIRTIO_NET_F_MRG_RXBUF: u32 = 15;
/// Device has MAC.
pub const VIRTIO_NET_F_MAC: u32 = 5;
/// Device supports multiqueue.
pub const VIRTIO_NET_F_MQ: u32 = 22;
/// Device has given MAC address.
pub const VIRTIO_NET_F_STATUS: u32 = 16;

// ---------------------------------------------------------------------------
// Virtio-net header flags
// ---------------------------------------------------------------------------

/// Needs checksum.
pub const VIRTIO_NET_HDR_F_NEEDS_CSUM: u8 = 1;
/// Data is valid.
pub const VIRTIO_NET_HDR_F_DATA_VALID: u8 = 2;
/// Reports RSC (coalesced segments).
pub const VIRTIO_NET_HDR_F_RSC_INFO: u8 = 4;

// ---------------------------------------------------------------------------
// GSO types
// ---------------------------------------------------------------------------

/// No GSO.
pub const VIRTIO_NET_HDR_GSO_NONE: u8 = 0;
/// TCP v4.
pub const VIRTIO_NET_HDR_GSO_TCPV4: u8 = 1;
/// UDP.
pub const VIRTIO_NET_HDR_GSO_UDP: u8 = 3;
/// TCP v6.
pub const VIRTIO_NET_HDR_GSO_TCPV6: u8 = 4;
/// UDP L4.
pub const VIRTIO_NET_HDR_GSO_UDP_L4: u8 = 5;
/// ECN flag.
pub const VIRTIO_NET_HDR_GSO_ECN: u8 = 0x80;

// ---------------------------------------------------------------------------
// Virtio-net header struct
// ---------------------------------------------------------------------------

/// Virtio network header (12 bytes with mergeable buffers).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioNetHdr {
    /// Flags.
    pub flags: u8,
    /// GSO type.
    pub gso_type: u8,
    /// Header length (ethernet + IP + TCP/UDP).
    pub hdr_len: u16,
    /// GSO segment size.
    pub gso_size: u16,
    /// Checksum start offset.
    pub csum_start: u16,
    /// Checksum offset from csum_start.
    pub csum_offset: u16,
    /// Number of merged buffers.
    pub num_buffers: u16,
}

impl VirtioNetHdr {
    /// Create a zeroed virtio net header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_size() {
        assert_eq!(core::mem::size_of::<VirtioNetHdr>(), 12);
    }

    #[test]
    fn test_features_distinct() {
        let feats = [
            VIRTIO_NET_F_CSUM,
            VIRTIO_NET_F_GUEST_CSUM,
            VIRTIO_NET_F_MAC,
            VIRTIO_NET_F_GUEST_TSO4,
            VIRTIO_NET_F_GUEST_TSO6,
            VIRTIO_NET_F_MRG_RXBUF,
            VIRTIO_NET_F_STATUS,
            VIRTIO_NET_F_CTRL_VQ,
            VIRTIO_NET_F_MQ,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_gso_types_distinct() {
        let types = [
            VIRTIO_NET_HDR_GSO_NONE,
            VIRTIO_NET_HDR_GSO_TCPV4,
            VIRTIO_NET_HDR_GSO_UDP,
            VIRTIO_NET_HDR_GSO_TCPV6,
            VIRTIO_NET_HDR_GSO_UDP_L4,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_header_flags() {
        assert_eq!(VIRTIO_NET_HDR_F_NEEDS_CSUM, 1);
        assert_eq!(VIRTIO_NET_HDR_F_DATA_VALID, 2);
    }

    #[test]
    fn test_virtio_id() {
        assert_eq!(VIRTIO_ID_NET, 1);
    }
}

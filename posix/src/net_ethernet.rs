//! `<net/ethernet.h>` — Ethernet frame definitions.
//!
//! Defines the Ethernet header structure and EtherType constants
//! used in network programming.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Size of an Ethernet (MAC) address in bytes.
pub const ETH_ALEN: usize = 6;

/// Maximum payload (without VLAN tag).
pub const ETH_DATA_LEN: usize = 1500;

/// Maximum frame size (without FCS).
pub const ETH_FRAME_LEN: usize = 1514;

/// Minimum frame size (without FCS).
pub const ETH_ZLEN: usize = 60;

/// Size of the Ethernet header (destination + source + type).
pub const ETH_HLEN: usize = 14;

/// Frame Check Sequence length.
pub const ETH_FCS_LEN: usize = 4;

/// Minimum total frame size including FCS.
pub const ETH_MIN_FRAME: usize = ETH_ZLEN + ETH_FCS_LEN;

// ---------------------------------------------------------------------------
// EtherType values (in host byte order)
// ---------------------------------------------------------------------------

/// Internet Protocol version 4.
pub const ETH_P_IP: u16 = 0x0800;

/// Address Resolution Protocol.
pub const ETH_P_ARP: u16 = 0x0806;

/// Reverse ARP.
pub const ETH_P_RARP: u16 = 0x8035;

/// VLAN tagged frame (IEEE 802.1Q).
pub const ETH_P_8021Q: u16 = 0x8100;

/// Internet Protocol version 6.
pub const ETH_P_IPV6: u16 = 0x86DD;

/// PPP over Ethernet discovery stage.
pub const ETH_P_PPP_DISC: u16 = 0x8863;

/// PPP over Ethernet session stage.
pub const ETH_P_PPP_SES: u16 = 0x8864;

/// Link Layer Discovery Protocol.
pub const ETH_P_LLDP: u16 = 0x88CC;

/// All frames (BPF / packet socket wildcard).
pub const ETH_P_ALL: u16 = 0x0003;

/// Loopback protocol.
pub const ETH_P_LOOP: u16 = 0x0060;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Ethernet frame header.
///
/// 14 bytes: 6-byte destination + 6-byte source + 2-byte EtherType.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct EtherHeader {
    /// Destination MAC address.
    pub ether_dhost: [u8; ETH_ALEN],
    /// Source MAC address.
    pub ether_shost: [u8; ETH_ALEN],
    /// EtherType / length field (network byte order).
    pub ether_type: u16,
}

/// Ethernet address (MAC address).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EtherAddr {
    /// 6-byte MAC address.
    pub ether_addr_octet: [u8; ETH_ALEN],
}

// ---------------------------------------------------------------------------
// Well-known MAC addresses
// ---------------------------------------------------------------------------

/// Broadcast Ethernet address (FF:FF:FF:FF:FF:FF).
pub const ETH_BROADCAST: EtherAddr = EtherAddr {
    ether_addr_octet: [0xFF; ETH_ALEN],
};

/// Zero Ethernet address (00:00:00:00:00:00).
pub const ETH_ZERO: EtherAddr = EtherAddr {
    ether_addr_octet: [0x00; ETH_ALEN],
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Size constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_eth_alen() {
        assert_eq!(ETH_ALEN, 6);
    }

    #[test]
    fn test_eth_hlen() {
        assert_eq!(ETH_HLEN, 14);
        // 6 (dst) + 6 (src) + 2 (type) = 14
        assert_eq!(ETH_HLEN, 2 * ETH_ALEN + 2);
    }

    #[test]
    fn test_eth_data_len() {
        assert_eq!(ETH_DATA_LEN, 1500);
    }

    #[test]
    fn test_eth_frame_len() {
        assert_eq!(ETH_FRAME_LEN, 1514);
        assert_eq!(ETH_FRAME_LEN, ETH_HLEN + ETH_DATA_LEN);
    }

    #[test]
    fn test_eth_zlen() {
        assert_eq!(ETH_ZLEN, 60);
    }

    #[test]
    fn test_eth_fcs_len() {
        assert_eq!(ETH_FCS_LEN, 4);
    }

    #[test]
    fn test_eth_min_frame() {
        assert_eq!(ETH_MIN_FRAME, 64);
        assert_eq!(ETH_MIN_FRAME, ETH_ZLEN + ETH_FCS_LEN);
    }

    // -----------------------------------------------------------------------
    // EtherType constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_ethertype_ip() {
        assert_eq!(ETH_P_IP, 0x0800);
    }

    #[test]
    fn test_ethertype_arp() {
        assert_eq!(ETH_P_ARP, 0x0806);
    }

    #[test]
    fn test_ethertype_ipv6() {
        assert_eq!(ETH_P_IPV6, 0x86DD);
    }

    #[test]
    fn test_ethertype_vlan() {
        assert_eq!(ETH_P_8021Q, 0x8100);
    }

    #[test]
    fn test_ethertypes_distinct() {
        let types = [
            ETH_P_IP, ETH_P_ARP, ETH_P_RARP, ETH_P_8021Q,
            ETH_P_IPV6, ETH_P_PPP_DISC, ETH_P_PPP_SES,
            ETH_P_LLDP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i], types[j],
                    "EtherTypes must be distinct"
                );
            }
        }
    }

    #[test]
    fn test_ethertypes_above_1500() {
        // EtherType values ≥ 0x0600 (1536) are protocol identifiers;
        // values < 1536 are IEEE 802.3 length fields.
        let protocol_types = [
            ETH_P_IP, ETH_P_ARP, ETH_P_RARP, ETH_P_8021Q,
            ETH_P_IPV6, ETH_P_PPP_DISC, ETH_P_PPP_SES, ETH_P_LLDP,
        ];
        for &t in &protocol_types {
            assert!(
                t >= 0x0600,
                "protocol EtherType 0x{:04X} must be >= 0x0600",
                t
            );
        }
    }

    // -----------------------------------------------------------------------
    // EtherHeader struct
    // -----------------------------------------------------------------------

    #[test]
    fn test_ether_header_size() {
        assert_eq!(core::mem::size_of::<EtherHeader>(), ETH_HLEN);
    }

    #[test]
    fn test_ether_header_packed() {
        // Packed means no padding between fields.
        assert_eq!(core::mem::size_of::<EtherHeader>(), 14);
    }

    #[test]
    fn test_ether_header_fields() {
        let hdr = EtherHeader {
            ether_dhost: [0xFF; 6], // broadcast
            ether_shost: [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
            ether_type: ETH_P_IP.to_be(), // network byte order
        };
        assert_eq!(hdr.ether_dhost, [0xFF; 6]);
        assert_eq!(hdr.ether_shost[0], 0x00);
        assert_eq!(hdr.ether_shost[5], 0x55);
    }

    // -----------------------------------------------------------------------
    // EtherAddr struct
    // -----------------------------------------------------------------------

    #[test]
    fn test_ether_addr_size() {
        assert_eq!(core::mem::size_of::<EtherAddr>(), ETH_ALEN);
    }

    #[test]
    fn test_ether_addr_eq() {
        let a = EtherAddr { ether_addr_octet: [1, 2, 3, 4, 5, 6] };
        let b = EtherAddr { ether_addr_octet: [1, 2, 3, 4, 5, 6] };
        let c = EtherAddr { ether_addr_octet: [1, 2, 3, 4, 5, 7] };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // -----------------------------------------------------------------------
    // Well-known addresses
    // -----------------------------------------------------------------------

    #[test]
    fn test_eth_broadcast() {
        assert_eq!(ETH_BROADCAST.ether_addr_octet, [0xFF; 6]);
    }

    #[test]
    fn test_eth_zero() {
        assert_eq!(ETH_ZERO.ether_addr_octet, [0x00; 6]);
    }

    #[test]
    fn test_broadcast_not_zero() {
        assert_ne!(ETH_BROADCAST, ETH_ZERO);
    }

    // -----------------------------------------------------------------------
    // Multicast bit
    // -----------------------------------------------------------------------

    #[test]
    fn test_broadcast_is_multicast() {
        // The least-significant bit of the first octet indicates multicast.
        assert_ne!(ETH_BROADCAST.ether_addr_octet[0] & 0x01, 0);
    }

    #[test]
    fn test_zero_is_not_multicast() {
        assert_eq!(ETH_ZERO.ether_addr_octet[0] & 0x01, 0);
    }
}

//! `<linux/if_arcnet.h>` — ARCnet frame format and protocol IDs.
//!
//! ARCnet (Attached Resource Computer Network) is a legacy token-bus
//! LAN technology, still used in some industrial settings. The Linux
//! arcnet driver exposes the wire-format protocol IDs and the
//! ARP/RFC1201 packet-type constants to userspace.

// ---------------------------------------------------------------------------
// `ARPHRD_ARCNET` — value reported in `struct sockaddr_ll.sll_hatype`
// ---------------------------------------------------------------------------

pub const ARPHRD_ARCNET: u16 = 7;

// ---------------------------------------------------------------------------
// Address length (1 byte: source/destination node ID)
// ---------------------------------------------------------------------------

pub const ARCNET_ALEN: usize = 1;

// ---------------------------------------------------------------------------
// Reserved node IDs
// ---------------------------------------------------------------------------

/// All-nodes broadcast (0 = wire convention).
pub const ARCNET_BROADCAST: u8 = 0x00;
/// Reserved monitor/unused IDs.
pub const ARCNET_RESERVED_LOW: u8 = 0x00;
pub const ARCNET_RESERVED_HIGH: u8 = 0xFF;

// ---------------------------------------------------------------------------
// MTU values
// ---------------------------------------------------------------------------

/// Short-frame data length (fits in one "short" ARCnet packet).
pub const ARC_HDR_SIZE: usize = 4;
pub const ARC_MAX_SHORT_LEN: usize = 252;
pub const ARC_MAX_LONG_LEN: usize = 504;
pub const ARC_MAX_EXC_LEN: usize = 4_096;
pub const ARC_DEFAULT_MTU: usize = ARC_MAX_LONG_LEN - ARC_HDR_SIZE;

// ---------------------------------------------------------------------------
// Protocol IDs (RFC 1201, encapsulated in `arc_hardware.proto`)
// ---------------------------------------------------------------------------

pub const ARC_P_IP: u8 = 0xF0;
pub const ARC_P_IPV6: u8 = 0xC4;
pub const ARC_P_ARP: u8 = 0xF1;
pub const ARC_P_RARP: u8 = 0xF2;
pub const ARC_P_IPX: u8 = 0xFA;
pub const ARC_P_NOVELL_EC: u8 = 0x8D;
pub const ARC_P_LANSOFT: u8 = 0xFD;
pub const ARC_P_ATALK: u8 = 0xDD;

/// RFC 1051 (older, length-prefixed) variants of the above.
pub const ARC_P_IP_RFC1051: u8 = 0xAD;
pub const ARC_P_ARP_RFC1051: u8 = 0xAE;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arphrd_value_is_7() {
        // ARPHRD_ARCNET is the IANA-assigned hardware type for ARCnet.
        assert_eq!(ARPHRD_ARCNET, 7);
    }

    #[test]
    fn test_address_length_is_one_byte() {
        // Single-byte node addressing, 0..255.
        assert_eq!(ARCNET_ALEN, 1);
    }

    #[test]
    fn test_broadcast_is_zero() {
        // ARCnet broadcasts to node 0 (unlike Ethernet's all-ones).
        assert_eq!(ARCNET_BROADCAST, 0);
        // Reserved IDs at both ends.
        assert!(ARCNET_RESERVED_HIGH > ARCNET_RESERVED_LOW);
    }

    #[test]
    fn test_mtu_progression() {
        // Short (252) < long (504) < EXC (4096).
        assert!(ARC_MAX_SHORT_LEN < ARC_MAX_LONG_LEN);
        assert!(ARC_MAX_LONG_LEN < ARC_MAX_EXC_LEN);
        // Default MTU subtracts the 4-byte header.
        assert_eq!(ARC_DEFAULT_MTU, ARC_MAX_LONG_LEN - ARC_HDR_SIZE);
        assert_eq!(ARC_HDR_SIZE, 4);
    }

    #[test]
    fn test_protocol_ids_distinct() {
        let p = [
            ARC_P_IP,
            ARC_P_IPV6,
            ARC_P_ARP,
            ARC_P_RARP,
            ARC_P_IPX,
            ARC_P_NOVELL_EC,
            ARC_P_LANSOFT,
            ARC_P_ATALK,
            ARC_P_IP_RFC1051,
            ARC_P_ARP_RFC1051,
        ];
        for (i, &a) in p.iter().enumerate() {
            for &b in &p[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_arp_and_ip_codes_match_rfc1201() {
        // RFC 1201 assigns IP=F0, ARP=F1, RARP=F2.
        assert_eq!(ARC_P_IP, 0xF0);
        assert_eq!(ARC_P_ARP, 0xF1);
        assert_eq!(ARC_P_RARP, 0xF2);
        // RFC 1051 (older) uses lower-range codes (0xAD/0xAE).
        assert!(ARC_P_IP_RFC1051 < ARC_P_IP);
        assert!(ARC_P_ARP_RFC1051 < ARC_P_ARP);
    }
}

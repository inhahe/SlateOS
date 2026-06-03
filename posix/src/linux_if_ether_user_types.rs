//! `<linux/if_ether.h>` — Ethernet protocol numbers and frame layout.
//!
//! Every packet socket (`PF_PACKET`), every BPF filter, every VLAN
//! tag handler, every bridge / firewall rule looks at the EtherType
//! values defined here. They are the single most-referenced constants
//! in Linux networking.

// ---------------------------------------------------------------------------
// Frame sizes
// ---------------------------------------------------------------------------

/// Octets per Ethernet address.
pub const ETH_ALEN: u32 = 6;
/// Octets per type/length field.
pub const ETH_TLEN: u32 = 2;
/// Total octets in header.
pub const ETH_HLEN: u32 = 14;
/// Octets in the FCS (CRC).
pub const ETH_FCS_LEN: u32 = 4;
/// Min. octets in a frame sans FCS.
pub const ETH_ZLEN: u32 = 60;
/// Max. octets in payload.
pub const ETH_DATA_LEN: u32 = 1500;
/// Max. octets in frame sans FCS.
pub const ETH_FRAME_LEN: u32 = 1514;
/// Jumbo-frame default min for QinQ.
pub const ETH_MIN_MTU: u32 = 68;
/// Jumbo frame upper bound.
pub const ETH_MAX_MTU: u32 = 0xFFFF;

// ---------------------------------------------------------------------------
// Common EtherType values (network byte order; user-facing constants are
// host-order — convert with htons() before placing on the wire).
// ---------------------------------------------------------------------------

/// IEEE 802.3 LLC.
pub const ETH_P_802_3: u16 = 0x0001;
/// Ethernet loopback (deprecated).
pub const ETH_P_LOOP: u16 = 0x0060;
/// IPv4.
pub const ETH_P_IP: u16 = 0x0800;
/// CCITT X.25.
pub const ETH_P_X25: u16 = 0x0805;
/// ARP.
pub const ETH_P_ARP: u16 = 0x0806;
/// Frame Relay DEC LANBridge.
pub const ETH_P_DEC: u16 = 0x6000;
/// Reverse ARP.
pub const ETH_P_RARP: u16 = 0x8035;
/// AppleTalk.
pub const ETH_P_ATALK: u16 = 0x809B;
/// AppleTalk AARP.
pub const ETH_P_AARP: u16 = 0x80F3;
/// 802.1Q VLAN tag.
pub const ETH_P_8021Q: u16 = 0x8100;
/// IPv6.
pub const ETH_P_IPV6: u16 = 0x86DD;
/// 802.1X / EAPOL.
pub const ETH_P_PAE: u16 = 0x888E;
/// 802.1ad QinQ.
pub const ETH_P_8021AD: u16 = 0x88A8;
/// Link Layer Discovery Protocol.
pub const ETH_P_LLDP: u16 = 0x88CC;
/// MAC Security (802.1AE).
pub const ETH_P_MACSEC: u16 = 0x88E5;
/// Precision Time Protocol.
pub const ETH_P_1588: u16 = 0x88F7;
/// Multiprotocol Label Switching (unicast).
pub const ETH_P_MPLS_UC: u16 = 0x8847;
/// MPLS (multicast).
pub const ETH_P_MPLS_MC: u16 = 0x8848;
/// FCoE.
pub const ETH_P_FCOE: u16 = 0x8906;
/// TIPC.
pub const ETH_P_TIPC: u16 = 0x88CA;
/// PPP over Ethernet discovery.
pub const ETH_P_PPP_DISC: u16 = 0x8863;
/// PPP over Ethernet session.
pub const ETH_P_PPP_SES: u16 = 0x8864;

// ---------------------------------------------------------------------------
// Non-DIX-II EtherType selectors (host order; AF_PACKET only)
// ---------------------------------------------------------------------------

/// All protocols.
pub const ETH_P_ALL: u16 = 0x0003;
/// All 802.3 frames.
pub const ETH_P_802_2: u16 = 0x0004;
/// All SNAP frames.
pub const ETH_P_SNAP: u16 = 0x0005;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_size_relationships() {
        // Header = 2*MAC + EtherType = 6+6+2.
        assert_eq!(ETH_ALEN * 2 + ETH_TLEN, ETH_HLEN);
        // Min payload + header = ZLEN (60).
        assert_eq!(ETH_ZLEN, ETH_HLEN + 46);
        // Max frame = header + MTU.
        assert_eq!(ETH_FRAME_LEN, ETH_HLEN + ETH_DATA_LEN);
        // MTU floor is 68 (IPv4 min reassembly), ceiling is 64KiB-1.
        assert!(ETH_MIN_MTU < ETH_DATA_LEN);
        assert_eq!(ETH_MAX_MTU, 0xFFFF);
    }

    #[test]
    fn test_dix_etypes_in_dix_range() {
        // DIX-II EtherTypes are >= 0x0600 (1536).
        for &e in &[
            ETH_P_IP,
            ETH_P_X25,
            ETH_P_ARP,
            ETH_P_RARP,
            ETH_P_ATALK,
            ETH_P_AARP,
            ETH_P_8021Q,
            ETH_P_IPV6,
            ETH_P_PAE,
            ETH_P_8021AD,
            ETH_P_LLDP,
            ETH_P_MACSEC,
            ETH_P_1588,
            ETH_P_MPLS_UC,
            ETH_P_MPLS_MC,
            ETH_P_FCOE,
            ETH_P_TIPC,
            ETH_P_PPP_DISC,
            ETH_P_PPP_SES,
        ] {
            assert!(e >= 0x0600);
        }
    }

    #[test]
    fn test_ip_and_ipv6_well_known() {
        assert_eq!(ETH_P_IP, 0x0800);
        assert_eq!(ETH_P_IPV6, 0x86DD);
        assert_eq!(ETH_P_ARP, 0x0806);
    }

    #[test]
    fn test_packet_selectors_below_dix_range() {
        // AF_PACKET pseudo-protocols live in the < 1536 reserved space.
        for &e in &[ETH_P_ALL, ETH_P_802_2, ETH_P_SNAP] {
            assert!(e < 0x0600);
        }
    }

    #[test]
    fn test_mpls_unicast_and_multicast_adjacent() {
        assert_eq!(ETH_P_MPLS_MC, ETH_P_MPLS_UC + 1);
    }
}

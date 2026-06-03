//! `<linux/if_ether.h>` — Ethernet protocol definitions.
//!
//! Re-exports EtherType constants from the `net_ethernet` module and
//! adds additional Ethernet protocol constants used by Linux.

// ---------------------------------------------------------------------------
// EtherTypes (from net_ethernet.rs)
// ---------------------------------------------------------------------------

pub use crate::net_ethernet::ETH_ALEN;
pub use crate::net_ethernet::ETH_HLEN;
pub use crate::net_ethernet::ETH_P_8021Q;
pub use crate::net_ethernet::ETH_P_ARP;
pub use crate::net_ethernet::ETH_P_IP;
pub use crate::net_ethernet::ETH_P_IPV6;

// ---------------------------------------------------------------------------
// Additional EtherType constants
// ---------------------------------------------------------------------------

/// Reverse ARP (RARP).
pub const ETH_P_RARP: u16 = 0x8035;

/// IEEE 802.1ad (Q-in-Q).
pub const ETH_P_8021AD: u16 = 0x88A8;

/// PPPoE Discovery.
pub const ETH_P_PPP_DISC: u16 = 0x8863;

/// PPPoE Session.
pub const ETH_P_PPP_SES: u16 = 0x8864;

/// LLDP (Link Layer Discovery Protocol).
pub const ETH_P_LLDP: u16 = 0x88CC;

/// MPLS unicast.
pub const ETH_P_MPLS_UC: u16 = 0x8847;

/// MPLS multicast.
pub const ETH_P_MPLS_MC: u16 = 0x8848;

/// Loopback protocol.
pub const ETH_P_LOOPBACK: u16 = 0x9000;

/// IEEE 802.2 LLC (when EtherType <= 1500, it's length).
pub const ETH_P_802_2: u16 = 0x0004;

/// All packets (used in `AF_PACKET` socket filters).
pub const ETH_P_ALL: u16 = 0x0003;

/// Minimum Ethernet frame payload length.
pub const ETH_ZLEN: usize = 60;

/// Maximum Ethernet frame payload length (MTU).
pub const ETH_DATA_LEN: usize = 1500;

/// Maximum Ethernet frame size (header + payload + FCS).
pub const ETH_FRAME_LEN: usize = 1514;

/// Length of Ethernet FCS (CRC-32).
pub const ETH_FCS_LEN: usize = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eth_alen() {
        assert_eq!(ETH_ALEN, 6);
    }

    #[test]
    fn test_eth_hlen() {
        assert_eq!(ETH_HLEN, 14);
    }

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
    fn test_ethertypes_distinct() {
        let types = [
            ETH_P_IP,
            ETH_P_ARP,
            ETH_P_IPV6,
            ETH_P_8021Q,
            ETH_P_RARP,
            ETH_P_8021AD,
            ETH_P_PPP_DISC,
            ETH_P_PPP_SES,
            ETH_P_LLDP,
            ETH_P_MPLS_UC,
            ETH_P_MPLS_MC,
            ETH_P_LOOPBACK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j], "EtherTypes must be distinct");
            }
        }
    }

    #[test]
    fn test_frame_sizes() {
        assert_eq!(ETH_ZLEN, 60);
        assert_eq!(ETH_DATA_LEN, 1500);
        assert_eq!(ETH_FRAME_LEN, 1514);
        assert_eq!(ETH_FCS_LEN, 4);
        // frame_len = hlen + data_len = 14 + 1500 = 1514
        assert_eq!(ETH_FRAME_LEN, ETH_HLEN as usize + ETH_DATA_LEN);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(ETH_P_IP, crate::net_ethernet::ETH_P_IP);
        assert_eq!(ETH_P_ARP, crate::net_ethernet::ETH_P_ARP);
        assert_eq!(ETH_ALEN, crate::net_ethernet::ETH_ALEN);
    }
}

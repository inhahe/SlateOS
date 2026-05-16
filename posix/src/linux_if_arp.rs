//! `<linux/if_arp.h>` — ARP protocol definitions (Linux kernel view).
//!
//! Re-exports ARP hardware types, operation codes, and the `Arphdr`
//! struct from the POSIX `net_if_arp` module.

// ---------------------------------------------------------------------------
// Re-exports from net_if_arp
// ---------------------------------------------------------------------------

pub use crate::net_if_arp::ARPHRD_ETHER;
pub use crate::net_if_arp::ARPHRD_IEEE802;
pub use crate::net_if_arp::ARPHRD_ARCNET;
pub use crate::net_if_arp::ARPHRD_SLIP;
pub use crate::net_if_arp::ARPHRD_PPP;
pub use crate::net_if_arp::ARPHRD_LOOPBACK;
pub use crate::net_if_arp::ARPHRD_SIT;
pub use crate::net_if_arp::ARPHRD_IPGRE;
pub use crate::net_if_arp::ARPHRD_VOID;
pub use crate::net_if_arp::ARPHRD_NONE;

pub use crate::net_if_arp::ARPOP_REQUEST;
pub use crate::net_if_arp::ARPOP_REPLY;
pub use crate::net_if_arp::ARPOP_RREQUEST;
pub use crate::net_if_arp::ARPOP_RREPLY;
pub use crate::net_if_arp::ARPOP_InREQUEST;
pub use crate::net_if_arp::ARPOP_InREPLY;
pub use crate::net_if_arp::ARPOP_NAK;

pub use crate::net_if_arp::ArpHdr;

// ---------------------------------------------------------------------------
// Additional Linux-specific hardware types
// ---------------------------------------------------------------------------

/// AppleTalk.
pub const ARPHRD_APPLETLK: u16 = 8;
/// Frame relay DLCI.
pub const ARPHRD_DLCI: u16 = 15;
/// ATM.
pub const ARPHRD_ATM: u16 = 19;
/// Metricom STRIP.
pub const ARPHRD_METRICOM: u16 = 23;
/// IEEE 1394 (Firewire).
pub const ARPHRD_IEEE1394: u16 = 24;
/// InfiniBand.
pub const ARPHRD_INFINIBAND: u16 = 32;
/// Tunnel (IPIP).
pub const ARPHRD_TUNNEL: u16 = 768;
/// Tunnel (IPv6-in-IPv4).
pub const ARPHRD_TUNNEL6: u16 = 769;
/// IEEE 802.11 (Wi-Fi).
pub const ARPHRD_IEEE80211: u16 = 801;
/// IEEE 802.15.4 (WPAN).
pub const ARPHRD_IEEE802154: u16 = 804;
/// Phonet pipe.
pub const ARPHRD_PHONET_PIPE: u16 = 821;
/// GRE over IPv6.
pub const ARPHRD_IP6GRE: u16 = 823;
/// Netlink (internal).
pub const ARPHRD_NETLINK: u16 = 824;
/// 6LoWPAN.
pub const ARPHRD_6LOWPAN: u16 = 825;

// ---------------------------------------------------------------------------
// ARP flags (arp_flags field in ARP table entries)
// ---------------------------------------------------------------------------

/// Completed entry (have valid MAC address).
pub const ATF_COM: i32 = 0x02;
/// Permanent entry.
pub const ATF_PERM: i32 = 0x04;
/// Publish entry (proxy ARP).
pub const ATF_PUBL: i32 = 0x08;
/// Has requested trailers.
pub const ATF_USETRAILERS: i32 = 0x10;
/// Use a hardware map.
pub const ATF_NETMASK: i32 = 0x20;
/// Don't use trailers.
pub const ATF_DONTPUB: i32 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arphrd_values() {
        assert_eq!(ARPHRD_ETHER, 1);
        assert_eq!(ARPHRD_LOOPBACK, 772);
        assert_eq!(ARPHRD_INFINIBAND, 32);
    }

    #[test]
    fn test_arpop_values() {
        assert_eq!(ARPOP_REQUEST, 1);
        assert_eq!(ARPOP_REPLY, 2);
    }

    #[test]
    fn test_linux_arphrd_distinct() {
        let types = [
            ARPHRD_DLCI, ARPHRD_ATM, ARPHRD_METRICOM,
            ARPHRD_IEEE1394, ARPHRD_INFINIBAND,
            ARPHRD_IEEE802154, ARPHRD_6LOWPAN, ARPHRD_NETLINK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_atf_flags() {
        assert_eq!(ATF_COM, 0x02);
        assert_eq!(ATF_PERM, 0x04);
        assert_eq!(ATF_PUBL, 0x08);
        // Flags should not overlap.
        assert_eq!(ATF_COM & ATF_PERM, 0);
        assert_eq!(ATF_PERM & ATF_PUBL, 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(ARPHRD_ETHER, crate::net_if_arp::ARPHRD_ETHER);
        assert_eq!(ARPOP_REQUEST, crate::net_if_arp::ARPOP_REQUEST);
        assert_eq!(ARPOP_REPLY, crate::net_if_arp::ARPOP_REPLY);
    }
}

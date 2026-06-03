//! `<linux/if_arp.h>` — ARP hardware-type numbers and operations.
//!
//! `ifconfig`, `ip neigh`, `arping`, NetworkManager, and every link-layer
//! tool reads the `ARPHRD_*` value out of `sockaddr_ll.sll_hatype` to
//! figure out what kind of L2 it's looking at (Ethernet, IEEE 802.11,
//! Infiniband, Bluetooth, loopback). The `ARPOP_*` codes appear in
//! both ARP requests on the wire and `RTM_NEWNEIGH` netlink messages.

// ---------------------------------------------------------------------------
// Hardware-type identifiers (struct arphdr.ar_hrd)
// ---------------------------------------------------------------------------

pub const ARPHRD_NETROM: u16 = 0;
pub const ARPHRD_ETHER: u16 = 1;
pub const ARPHRD_EETHER: u16 = 2;
pub const ARPHRD_AX25: u16 = 3;
pub const ARPHRD_PRONET: u16 = 4;
pub const ARPHRD_CHAOS: u16 = 5;
pub const ARPHRD_IEEE802: u16 = 6;
pub const ARPHRD_ARCNET: u16 = 7;
pub const ARPHRD_APPLETLK: u16 = 8;
pub const ARPHRD_DLCI: u16 = 15;
pub const ARPHRD_ATM: u16 = 19;
pub const ARPHRD_METRICOM: u16 = 23;
pub const ARPHRD_IEEE1394: u16 = 24;
pub const ARPHRD_EUI64: u16 = 27;
pub const ARPHRD_INFINIBAND: u16 = 32;
/// Special "no link layer" pseudo-types.
pub const ARPHRD_SLIP: u16 = 256;
pub const ARPHRD_CSLIP: u16 = 257;
pub const ARPHRD_PPP: u16 = 512;
pub const ARPHRD_CISCO: u16 = 513;
pub const ARPHRD_LAPB: u16 = 516;
pub const ARPHRD_TUNNEL: u16 = 768;
pub const ARPHRD_TUNNEL6: u16 = 769;
pub const ARPHRD_FRAD: u16 = 770;
pub const ARPHRD_LOOPBACK: u16 = 772;
pub const ARPHRD_LOCALTLK: u16 = 773;
pub const ARPHRD_IEEE80211: u16 = 801;
pub const ARPHRD_IEEE80211_PRISM: u16 = 802;
pub const ARPHRD_IEEE80211_RADIOTAP: u16 = 803;
pub const ARPHRD_IEEE802154: u16 = 804;
pub const ARPHRD_PHONET: u16 = 820;
pub const ARPHRD_PHONET_PIPE: u16 = 821;
pub const ARPHRD_CAIF: u16 = 822;
pub const ARPHRD_IP6GRE: u16 = 823;
pub const ARPHRD_NETLINK: u16 = 824;
pub const ARPHRD_6LOWPAN: u16 = 825;
pub const ARPHRD_VSOCKMON: u16 = 826;
pub const ARPHRD_VOID: u16 = 0xFFFF;
pub const ARPHRD_NONE: u16 = 0xFFFE;

// ---------------------------------------------------------------------------
// ARP operation codes (struct arphdr.ar_op)
// ---------------------------------------------------------------------------

pub const ARPOP_REQUEST: u16 = 1;
pub const ARPOP_REPLY: u16 = 2;
pub const ARPOP_RREQUEST: u16 = 3;
pub const ARPOP_RREPLY: u16 = 4;
pub const ARPOP_INREQUEST: u16 = 8;
pub const ARPOP_INREPLY: u16 = 9;
/// NAK (Inverse ARP).
pub const ARPOP_NAK: u16 = 10;

// ---------------------------------------------------------------------------
// arpreq / siocgarp flags
// ---------------------------------------------------------------------------

pub const ATF_COM: u32 = 0x02;
pub const ATF_PERM: u32 = 0x04;
pub const ATF_PUBL: u32 = 0x08;
pub const ATF_USETRAILERS: u32 = 0x10;
pub const ATF_NETMASK: u32 = 0x20;
pub const ATF_DONTPUB: u32 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethernet_is_1() {
        // The number-one entry in every ARPHRD table since 4.2BSD.
        assert_eq!(ARPHRD_ETHER, 1);
    }

    #[test]
    fn test_loopback_and_void() {
        assert_eq!(ARPHRD_LOOPBACK, 772);
        // VOID and NONE share the high range and must differ.
        assert_ne!(ARPHRD_VOID, ARPHRD_NONE);
        assert!(ARPHRD_NONE < ARPHRD_VOID);
    }

    #[test]
    fn test_arpop_request_reply_pairs() {
        // request/reply pairs are odd/even within a family.
        assert_eq!(ARPOP_REPLY, ARPOP_REQUEST + 1);
        assert_eq!(ARPOP_RREPLY, ARPOP_RREQUEST + 1);
        assert_eq!(ARPOP_INREPLY, ARPOP_INREQUEST + 1);
        // NAK is the highest-numbered op we recognise.
        assert!(ARPOP_NAK > ARPOP_INREPLY);
    }

    #[test]
    fn test_atf_flags_pow2() {
        let f = [
            ATF_COM,
            ATF_PERM,
            ATF_PUBL,
            ATF_USETRAILERS,
            ATF_NETMASK,
            ATF_DONTPUB,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_ieee802_family_consecutive() {
        // 80211 variants are 801..803, IEEE 802.15.4 is 804.
        assert_eq!(ARPHRD_IEEE80211_PRISM, ARPHRD_IEEE80211 + 1);
        assert_eq!(ARPHRD_IEEE80211_RADIOTAP, ARPHRD_IEEE80211 + 2);
        assert_eq!(ARPHRD_IEEE802154, ARPHRD_IEEE80211 + 3);
    }
}

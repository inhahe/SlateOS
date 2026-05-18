//! `<net/if_arp.h>` — ARP hardware type constants.
//!
//! These constants identify the hardware (link-layer) type of a
//! network interface, as reported by `ioctl(SIOCGIFHWADDR)` and
//! used in ARP packets.

// ---------------------------------------------------------------------------
// ARP hardware types (ARPHRD_*)
// ---------------------------------------------------------------------------

/// Ethernet (10/100/1000/etc).
pub const ARPHRD_ETHER: u16 = 1;
/// IEEE 802.2 (LLC).
pub const ARPHRD_IEEE802: u16 = 6;
/// ARCnet.
pub const ARPHRD_ARCNET: u16 = 7;
/// SLIP.
pub const ARPHRD_SLIP: u16 = 256;
/// PPP.
pub const ARPHRD_PPP: u16 = 512;
/// Loopback.
pub const ARPHRD_LOOPBACK: u16 = 772;
/// FDDI.
pub const ARPHRD_FDDI: u16 = 774;
/// IEEE 802.11 (WiFi).
pub const ARPHRD_IEEE80211: u16 = 801;
/// IEEE 802.11 with radiotap header.
pub const ARPHRD_IEEE80211_RADIOTAP: u16 = 803;
/// IEEE 802.15.4 (Zigbee/WPAN).
pub const ARPHRD_IEEE802154: u16 = 804;
/// InfiniBand.
pub const ARPHRD_INFINIBAND: u16 = 32;
/// Tunnel (IP-in-IP).
pub const ARPHRD_TUNNEL: u16 = 768;
/// Tunnel (IPv6-in-IP).
pub const ARPHRD_TUNNEL6: u16 = 769;
/// GRE tunnel.
pub const ARPHRD_IPGRE: u16 = 778;
/// SIT tunnel (IPv6-over-IPv4).
pub const ARPHRD_SIT: u16 = 776;
/// IPoIB (IP over InfiniBand).
pub const ARPHRD_INFINIBAND_IP: u16 = 32;
/// CAN bus.
pub const ARPHRD_CAN: u16 = 280;
/// No hardware header (raw IP).
pub const ARPHRD_NONE: u16 = 0xFFFE;
/// Void (nothing known).
pub const ARPHRD_VOID: u16 = 0xFFFF;

// ---------------------------------------------------------------------------
// ARP operation codes
// ---------------------------------------------------------------------------

/// ARP request.
pub const ARPOP_REQUEST: u16 = 1;
/// ARP reply.
pub const ARPOP_REPLY: u16 = 2;
/// RARP request.
pub const ARPOP_RREQUEST: u16 = 3;
/// RARP reply.
pub const ARPOP_RREPLY: u16 = 4;
/// InARP request.
pub const ARPOP_InREQUEST: u16 = 8;
/// InARP reply.
pub const ARPOP_InREPLY: u16 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_types_distinct() {
        let types = [
            ARPHRD_ETHER, ARPHRD_IEEE802, ARPHRD_ARCNET,
            ARPHRD_SLIP, ARPHRD_PPP, ARPHRD_LOOPBACK,
            ARPHRD_FDDI, ARPHRD_IEEE80211, ARPHRD_IEEE80211_RADIOTAP,
            ARPHRD_IEEE802154, ARPHRD_INFINIBAND,
            ARPHRD_TUNNEL, ARPHRD_TUNNEL6, ARPHRD_IPGRE,
            ARPHRD_SIT, ARPHRD_CAN, ARPHRD_NONE, ARPHRD_VOID,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ether() {
        assert_eq!(ARPHRD_ETHER, 1);
    }

    #[test]
    fn test_loopback() {
        assert_eq!(ARPHRD_LOOPBACK, 772);
    }

    #[test]
    fn test_arp_ops_distinct() {
        let ops = [
            ARPOP_REQUEST, ARPOP_REPLY, ARPOP_RREQUEST,
            ARPOP_RREPLY, ARPOP_InREQUEST, ARPOP_InREPLY,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_arp_request() {
        assert_eq!(ARPOP_REQUEST, 1);
    }
}

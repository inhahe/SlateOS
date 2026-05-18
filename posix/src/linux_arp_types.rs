//! `<linux/if_arp.h>` — ARP (Address Resolution Protocol) constants.
//!
//! ARP maps network-layer addresses (IPv4) to link-layer addresses
//! (MAC). When a host needs to send a packet to an IP on the local
//! network, it broadcasts an ARP request ("who has 192.168.1.1?")
//! and the owner replies with its MAC address. ARP entries are cached
//! in the neighbor table with timeout-based expiry. The kernel also
//! supports proxy ARP, gratuitous ARP, and ARP filtering.

// ---------------------------------------------------------------------------
// ARP hardware types (ar_hrd)
// ---------------------------------------------------------------------------

/// Ethernet (10/100/1000/10G).
pub const ARPHRD_ETHER: u32 = 1;
/// IEEE 802.2 (LLC).
pub const ARPHRD_IEEE802: u32 = 6;
/// ARCnet.
pub const ARPHRD_ARCNET: u32 = 7;
/// Frame Relay DLCI.
pub const ARPHRD_DLCI: u32 = 15;
/// ATM (Asynchronous Transfer Mode).
pub const ARPHRD_ATM: u32 = 19;
/// HDLC.
pub const ARPHRD_HDLC: u32 = 513;
/// Fibre Channel.
pub const ARPHRD_FCPP: u32 = 784;
/// InfiniBand.
pub const ARPHRD_INFINIBAND: u32 = 32;
/// IEEE 802.11 (WiFi).
pub const ARPHRD_IEEE80211: u32 = 801;
/// Loopback.
pub const ARPHRD_LOOPBACK: u32 = 772;
/// IP-over-InfiniBand (IPoIB).
pub const ARPHRD_IPOIB: u32 = 32;
/// Tunnel (GRE, IPIP).
pub const ARPHRD_TUNNEL: u32 = 768;
/// Void (no hardware header).
pub const ARPHRD_VOID: u32 = 0xFFFF;
/// No ARP (point-to-point).
pub const ARPHRD_NONE: u32 = 0xFFFE;

// ---------------------------------------------------------------------------
// ARP operation codes (ar_op)
// ---------------------------------------------------------------------------

/// ARP request (who-has).
pub const ARPOP_REQUEST: u32 = 1;
/// ARP reply (is-at).
pub const ARPOP_REPLY: u32 = 2;
/// RARP request (reverse ARP, MAC→IP).
pub const ARPOP_RREQUEST: u32 = 3;
/// RARP reply.
pub const ARPOP_RREPLY: u32 = 4;
/// InARP request (Inverse ARP).
pub const ARPOP_INREQUEST: u32 = 8;
/// InARP reply.
pub const ARPOP_INREPLY: u32 = 9;
/// ARP NAK (negative acknowledgment).
pub const ARPOP_NAK: u32 = 10;

// ---------------------------------------------------------------------------
// ARP flags (arp_flags in neighbor entry)
// ---------------------------------------------------------------------------

/// Entry is complete (has MAC address).
pub const ATF_COM: u32 = 0x02;
/// Permanent entry (no timeout).
pub const ATF_PERM: u32 = 0x04;
/// Publish entry (proxy ARP).
pub const ATF_PUBL: u32 = 0x08;
/// Use trailers (obsolete).
pub const ATF_USETRAILERS: u32 = 0x10;
/// Don't use ARP for this entry.
pub const ATF_DONTPUB: u32 = 0x40;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            ARPOP_REQUEST, ARPOP_REPLY, ARPOP_RREQUEST,
            ARPOP_RREPLY, ARPOP_INREQUEST, ARPOP_INREPLY, ARPOP_NAK,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_hardware_types_nonzero() {
        // Most hardware types are non-zero (VOID is special)
        assert_ne!(ARPHRD_ETHER, 0);
        assert_ne!(ARPHRD_IEEE802, 0);
        assert_ne!(ARPHRD_LOOPBACK, 0);
    }

    #[test]
    fn test_flags() {
        // ATF flags should not overlap
        let flags = [ATF_COM, ATF_PERM, ATF_PUBL, ATF_USETRAILERS, ATF_DONTPUB];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

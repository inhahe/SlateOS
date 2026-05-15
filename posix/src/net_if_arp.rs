//! `<net/if_arp.h>` — ARP protocol definitions.
//!
//! Defines hardware type constants and ARP operation codes used in
//! the Address Resolution Protocol (RFC 826).

// ---------------------------------------------------------------------------
// ARP hardware types (ar_hrd)
// ---------------------------------------------------------------------------

/// Ethernet 10/100/1000Mbps.
pub const ARPHRD_ETHER: u16 = 1;

/// IEEE 802 (same as Ethernet in practice).
pub const ARPHRD_IEEE802: u16 = 6;

/// ARCnet.
pub const ARPHRD_ARCNET: u16 = 7;

/// Frame relay DLCI.
pub const ARPHRD_DLCI: u16 = 15;

/// ATM.
pub const ARPHRD_ATM: u16 = 19;

/// Metricom STRIP.
pub const ARPHRD_METRICOM: u16 = 23;

/// IEEE 1394 (FireWire).
pub const ARPHRD_IEEE1394: u16 = 24;

/// EUI-64 (RFC 2464).
pub const ARPHRD_EUI64: u16 = 27;

/// InfiniBand.
pub const ARPHRD_INFINIBAND: u16 = 32;

/// SLIP.
pub const ARPHRD_SLIP: u16 = 256;

/// CSLIP.
pub const ARPHRD_CSLIP: u16 = 257;

/// PPP.
pub const ARPHRD_PPP: u16 = 512;

/// Loopback.
pub const ARPHRD_LOOPBACK: u16 = 772;

/// Linux-SIT (IPv6-in-IPv4 tunnel).
pub const ARPHRD_SIT: u16 = 776;

/// IP-in-IP tunnel.
pub const ARPHRD_IPGRE: u16 = 778;

/// Void (nothing known about hardware).
pub const ARPHRD_VOID: u16 = 0xFFFF;

/// No ARP hardware type.
pub const ARPHRD_NONE: u16 = 0xFFFE;

// ---------------------------------------------------------------------------
// ARP operation codes (ar_op)
// ---------------------------------------------------------------------------

/// ARP request.
pub const ARPOP_REQUEST: u16 = 1;

/// ARP reply.
pub const ARPOP_REPLY: u16 = 2;

/// RARP request.
pub const ARPOP_RREQUEST: u16 = 3;

/// RARP reply.
pub const ARPOP_RREPLY: u16 = 4;

/// InARP request (RFC 2390).
pub const ARPOP_InREQUEST: u16 = 8;

/// InARP reply (RFC 2390).
pub const ARPOP_InREPLY: u16 = 9;

/// ARP NAK (RFC 1577, ATM).
pub const ARPOP_NAK: u16 = 10;

// ---------------------------------------------------------------------------
// ARP flags (for ioctl / routing table)
// ---------------------------------------------------------------------------

/// Completed entry (valid hardware address).
pub const ATF_COM: i32 = 0x02;

/// Permanent entry.
pub const ATF_PERM: i32 = 0x04;

/// Publish entry (proxy ARP).
pub const ATF_PUBL: i32 = 0x08;

/// Has requested trailers.
pub const ATF_USETRAILERS: i32 = 0x10;

/// Want to use a netmask.
pub const ATF_NETMASK: i32 = 0x20;

/// Don't use trailers.
pub const ATF_DONTPUB: i32 = 0x40;

// ---------------------------------------------------------------------------
// ARP header structure
// ---------------------------------------------------------------------------

/// ARP header (fixed portion, 8 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ArpHdr {
    /// Hardware type (ARPHRD_*).
    pub ar_hrd: u16,
    /// Protocol type (e.g., ETH_P_IP = 0x0800).
    pub ar_pro: u16,
    /// Hardware address length (6 for Ethernet).
    pub ar_hln: u8,
    /// Protocol address length (4 for IPv4).
    pub ar_pln: u8,
    /// Operation (ARPOP_*).
    pub ar_op: u16,
    // Followed by variable-length addresses:
    //   ar_sha[ar_hln]  sender hardware address
    //   ar_spa[ar_pln]  sender protocol address
    //   ar_tha[ar_hln]  target hardware address
    //   ar_tpa[ar_pln]  target protocol address
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Hardware types
    // -----------------------------------------------------------------------

    #[test]
    fn test_arphrd_ether() {
        assert_eq!(ARPHRD_ETHER, 1);
    }

    #[test]
    fn test_arphrd_loopback() {
        assert_eq!(ARPHRD_LOOPBACK, 772);
    }

    #[test]
    fn test_arphrd_types_distinct() {
        let types = [
            ARPHRD_ETHER, ARPHRD_IEEE802, ARPHRD_ARCNET,
            ARPHRD_DLCI, ARPHRD_ATM, ARPHRD_METRICOM,
            ARPHRD_IEEE1394, ARPHRD_EUI64, ARPHRD_INFINIBAND,
            ARPHRD_SLIP, ARPHRD_CSLIP, ARPHRD_PPP,
            ARPHRD_LOOPBACK, ARPHRD_SIT, ARPHRD_IPGRE,
            ARPHRD_VOID, ARPHRD_NONE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i], types[j],
                    "ARPHRD types must be distinct"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Operation codes
    // -----------------------------------------------------------------------

    #[test]
    fn test_arpop_request_reply() {
        assert_eq!(ARPOP_REQUEST, 1);
        assert_eq!(ARPOP_REPLY, 2);
    }

    #[test]
    fn test_arpop_distinct() {
        let ops = [
            ARPOP_REQUEST, ARPOP_REPLY, ARPOP_RREQUEST,
            ARPOP_RREPLY, ARPOP_InREQUEST, ARPOP_InREPLY,
            ARPOP_NAK,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // ARP flags
    // -----------------------------------------------------------------------

    #[test]
    fn test_atf_flags_distinct() {
        let flags = [
            ATF_COM, ATF_PERM, ATF_PUBL, ATF_USETRAILERS,
            ATF_NETMASK, ATF_DONTPUB,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    // -----------------------------------------------------------------------
    // ARP header struct
    // -----------------------------------------------------------------------

    #[test]
    fn test_arp_hdr_size() {
        assert_eq!(core::mem::size_of::<ArpHdr>(), 8);
    }

    #[test]
    fn test_arp_hdr_fields() {
        let hdr = ArpHdr {
            ar_hrd: ARPHRD_ETHER.to_be(),
            ar_pro: 0x0800u16.to_be(), // IPv4
            ar_hln: 6,                  // Ethernet address length
            ar_pln: 4,                  // IPv4 address length
            ar_op: ARPOP_REQUEST.to_be(),
        };
        assert_eq!(hdr.ar_hln, 6);
        assert_eq!(hdr.ar_pln, 4);
    }

    // -----------------------------------------------------------------------
    // Cross-module: Ethernet consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_arp_ethertype_matches() {
        // ARP EtherType should match net_ethernet's ETH_P_ARP.
        assert_eq!(crate::net_ethernet::ETH_P_ARP, 0x0806);
    }
}

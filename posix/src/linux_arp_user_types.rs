//! `<net/if_arp.h>` — ARP wire format, op-codes, and cache entry flags.
//!
//! The Address Resolution Protocol maps L3 (IPv4) addresses to L2
//! hardware addresses on a broadcast link. This module covers the
//! op-codes, the kernel's ARP-cache flags, and the well-known
//! hardware-type identifiers (`ARPHRD_*`).

// ---------------------------------------------------------------------------
// ARP op-codes (`arphdr.ar_op`, wire-format big-endian)
// ---------------------------------------------------------------------------

pub const ARPOP_REQUEST: u16 = 1;
pub const ARPOP_REPLY: u16 = 2;
pub const ARPOP_RREQUEST: u16 = 3;
pub const ARPOP_RREPLY: u16 = 4;
pub const ARPOP_INREQUEST: u16 = 8;
pub const ARPOP_INREPLY: u16 = 9;
pub const ARPOP_NAK: u16 = 10;

// ---------------------------------------------------------------------------
// ARP cache entry flags (`arpreq.arp_flags`)
// ---------------------------------------------------------------------------

pub const ATF_COM: u32 = 0x02;
pub const ATF_PERM: u32 = 0x04;
pub const ATF_PUBL: u32 = 0x08;
pub const ATF_USETRAILERS: u32 = 0x10;
pub const ATF_NETMASK: u32 = 0x20;
pub const ATF_DONTPUB: u32 = 0x40;
pub const ATF_MAGIC: u32 = 0x80;

// ---------------------------------------------------------------------------
// `ARPHRD_*` — hardware address types (matches sll_hatype)
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

/// Loopback "hardware" type.
pub const ARPHRD_LOOPBACK: u16 = 772;
pub const ARPHRD_TUNNEL: u16 = 768;
pub const ARPHRD_PPP: u16 = 512;
pub const ARPHRD_NONE: u16 = 0xFFFE;
pub const ARPHRD_VOID: u16 = 0xFFFF;

// ---------------------------------------------------------------------------
// /proc/net paths
// ---------------------------------------------------------------------------

pub const PROC_NET_ARP: &str = "/proc/net/arp";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct_and_in_range() {
        // Standard ARP/RARP opcodes 1..=4, InARP 8..=9, NAK=10.
        let ops = [
            ARPOP_REQUEST,
            ARPOP_REPLY,
            ARPOP_RREQUEST,
            ARPOP_RREPLY,
            ARPOP_INREQUEST,
            ARPOP_INREPLY,
            ARPOP_NAK,
        ];
        for (i, &a) in ops.iter().enumerate() {
            for &b in &ops[i + 1..] {
                assert_ne!(a, b);
            }
            assert!(a <= 10);
        }
        // Request/reply pairs are consecutive.
        assert_eq!(ARPOP_REPLY, ARPOP_REQUEST + 1);
        assert_eq!(ARPOP_RREPLY, ARPOP_RREQUEST + 1);
        assert_eq!(ARPOP_INREPLY, ARPOP_INREQUEST + 1);
    }

    #[test]
    fn test_atf_flags_each_a_power_of_two() {
        for v in [
            ATF_COM,
            ATF_PERM,
            ATF_PUBL,
            ATF_USETRAILERS,
            ATF_NETMASK,
            ATF_DONTPUB,
            ATF_MAGIC,
        ] {
            assert!(v.is_power_of_two());
        }
        // High byte of flags covers ATF_* bits 1..=7 -> 0xFE.
        let or = ATF_COM
            | ATF_PERM
            | ATF_PUBL
            | ATF_USETRAILERS
            | ATF_NETMASK
            | ATF_DONTPUB
            | ATF_MAGIC;
        assert_eq!(or, 0xFE);
    }

    #[test]
    fn test_arphrd_ether_is_one() {
        // ARPHRD_ETHER=1 is the IANA-blessed Ethernet value.
        assert_eq!(ARPHRD_ETHER, 1);
        // ARPHRD_NETROM=0 because the kernel historically used 0 for that.
        assert_eq!(ARPHRD_NETROM, 0);
    }

    #[test]
    fn test_arphrd_high_codes_for_pseudo_links() {
        // The kernel-internal pseudo-types live above 256.
        assert!(ARPHRD_PPP >= 512);
        assert!(ARPHRD_TUNNEL >= 768);
        assert_eq!(ARPHRD_LOOPBACK, 772);
        assert_eq!(ARPHRD_NONE, 0xFFFE);
        assert_eq!(ARPHRD_VOID, 0xFFFF);
        // NONE is one less than VOID.
        assert_eq!(ARPHRD_VOID - ARPHRD_NONE, 1);
    }

    #[test]
    fn test_proc_path() {
        assert_eq!(PROC_NET_ARP, "/proc/net/arp");
    }
}

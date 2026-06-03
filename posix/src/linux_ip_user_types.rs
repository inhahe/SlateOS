//! `<linux/ip.h>` / `<netinet/ip.h>` — IPv4 socket options and TOS.
//!
//! Every IPv4 raw socket, every multicast joiner (`pimd`, `avahi`),
//! every transparent proxy (`tproxy`), every Tor-style traffic-mark
//! tool reads/writes the `IP_*` setsockopt names below. The header
//! field constants are the on-wire layout enforced by `ip_send_check`.

// ---------------------------------------------------------------------------
// SOL_IP setsockopt names (numeric, kernel ABI)
// ---------------------------------------------------------------------------

pub const IP_TOS: u32 = 1;
pub const IP_TTL: u32 = 2;
pub const IP_HDRINCL: u32 = 3;
pub const IP_OPTIONS: u32 = 4;
pub const IP_ROUTER_ALERT: u32 = 5;
pub const IP_RECVOPTS: u32 = 6;
pub const IP_RETOPTS: u32 = 7;
pub const IP_PKTINFO: u32 = 8;
pub const IP_PKTOPTIONS: u32 = 9;
pub const IP_MTU_DISCOVER: u32 = 10;
pub const IP_RECVERR: u32 = 11;
pub const IP_RECVTTL: u32 = 12;
pub const IP_RECVTOS: u32 = 13;
pub const IP_MTU: u32 = 14;
pub const IP_FREEBIND: u32 = 15;
pub const IP_IPSEC_POLICY: u32 = 16;
pub const IP_XFRM_POLICY: u32 = 17;
pub const IP_PASSSEC: u32 = 18;
pub const IP_TRANSPARENT: u32 = 19;
pub const IP_ORIGDSTADDR: u32 = 20;
pub const IP_RECVORIGDSTADDR: u32 = IP_ORIGDSTADDR;
pub const IP_MINTTL: u32 = 21;
pub const IP_NODEFRAG: u32 = 22;
pub const IP_CHECKSUM: u32 = 23;
pub const IP_BIND_ADDRESS_NO_PORT: u32 = 24;
pub const IP_RECVFRAGSIZE: u32 = 25;
pub const IP_RECVERR_RFC4884: u32 = 26;

// ---------------------------------------------------------------------------
// IP_MTU_DISCOVER modes (RFC 1191 / RFC 4821)
// ---------------------------------------------------------------------------

pub const IP_PMTUDISC_DONT: u32 = 0;
pub const IP_PMTUDISC_WANT: u32 = 1;
pub const IP_PMTUDISC_DO: u32 = 2;
pub const IP_PMTUDISC_PROBE: u32 = 3;
pub const IP_PMTUDISC_INTERFACE: u32 = 4;
pub const IP_PMTUDISC_OMIT: u32 = 5;

// ---------------------------------------------------------------------------
// Multicast options
// ---------------------------------------------------------------------------

pub const IP_MULTICAST_IF: u32 = 32;
pub const IP_MULTICAST_TTL: u32 = 33;
pub const IP_MULTICAST_LOOP: u32 = 34;
pub const IP_ADD_MEMBERSHIP: u32 = 35;
pub const IP_DROP_MEMBERSHIP: u32 = 36;
pub const IP_UNBLOCK_SOURCE: u32 = 37;
pub const IP_BLOCK_SOURCE: u32 = 38;
pub const IP_ADD_SOURCE_MEMBERSHIP: u32 = 39;
pub const IP_DROP_SOURCE_MEMBERSHIP: u32 = 40;
pub const IP_MSFILTER: u32 = 41;
pub const MCAST_JOIN_GROUP: u32 = 42;
pub const MCAST_BLOCK_SOURCE: u32 = 43;
pub const MCAST_UNBLOCK_SOURCE: u32 = 44;
pub const MCAST_LEAVE_GROUP: u32 = 45;
pub const MCAST_JOIN_SOURCE_GROUP: u32 = 46;
pub const MCAST_LEAVE_SOURCE_GROUP: u32 = 47;
pub const MCAST_MSFILTER: u32 = 48;
pub const IP_MULTICAST_ALL: u32 = 49;
pub const IP_UNICAST_IF: u32 = 50;

// ---------------------------------------------------------------------------
// TOS / DSCP byte values
// ---------------------------------------------------------------------------

pub const IPTOS_LOWDELAY: u8 = 0x10;
pub const IPTOS_THROUGHPUT: u8 = 0x08;
pub const IPTOS_RELIABILITY: u8 = 0x04;
pub const IPTOS_MINCOST: u8 = 0x02;
pub const IPTOS_TOS_MASK: u8 = 0x1E;

// ---------------------------------------------------------------------------
// Header constants
// ---------------------------------------------------------------------------

pub const IPVERSION: u8 = 4;
pub const IP_MAXPACKET: u32 = 65535;
pub const MAXTTL: u8 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_sockopts_dense_1_to_26() {
        let o = [
            IP_TOS,
            IP_TTL,
            IP_HDRINCL,
            IP_OPTIONS,
            IP_ROUTER_ALERT,
            IP_RECVOPTS,
            IP_RETOPTS,
            IP_PKTINFO,
            IP_PKTOPTIONS,
            IP_MTU_DISCOVER,
            IP_RECVERR,
            IP_RECVTTL,
            IP_RECVTOS,
            IP_MTU,
            IP_FREEBIND,
            IP_IPSEC_POLICY,
            IP_XFRM_POLICY,
            IP_PASSSEC,
            IP_TRANSPARENT,
            IP_ORIGDSTADDR,
            IP_MINTTL,
            IP_NODEFRAG,
            IP_CHECKSUM,
            IP_BIND_ADDRESS_NO_PORT,
            IP_RECVFRAGSIZE,
            IP_RECVERR_RFC4884,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        // RECVORIGDSTADDR is the documented alias.
        assert_eq!(IP_RECVORIGDSTADDR, IP_ORIGDSTADDR);
    }

    #[test]
    fn test_pmtu_modes_dense_0_to_5() {
        let m = [
            IP_PMTUDISC_DONT,
            IP_PMTUDISC_WANT,
            IP_PMTUDISC_DO,
            IP_PMTUDISC_PROBE,
            IP_PMTUDISC_INTERFACE,
            IP_PMTUDISC_OMIT,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_multicast_sockopts_dense_32_to_50() {
        let m = [
            IP_MULTICAST_IF,
            IP_MULTICAST_TTL,
            IP_MULTICAST_LOOP,
            IP_ADD_MEMBERSHIP,
            IP_DROP_MEMBERSHIP,
            IP_UNBLOCK_SOURCE,
            IP_BLOCK_SOURCE,
            IP_ADD_SOURCE_MEMBERSHIP,
            IP_DROP_SOURCE_MEMBERSHIP,
            IP_MSFILTER,
            MCAST_JOIN_GROUP,
            MCAST_BLOCK_SOURCE,
            MCAST_UNBLOCK_SOURCE,
            MCAST_LEAVE_GROUP,
            MCAST_JOIN_SOURCE_GROUP,
            MCAST_LEAVE_SOURCE_GROUP,
            MCAST_MSFILTER,
            IP_MULTICAST_ALL,
            IP_UNICAST_IF,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v as usize, 32 + i);
        }
    }

    #[test]
    fn test_tos_bits_pow2_and_mask_includes_all() {
        for &b in &[
            IPTOS_LOWDELAY,
            IPTOS_THROUGHPUT,
            IPTOS_RELIABILITY,
            IPTOS_MINCOST,
        ] {
            assert!(b.is_power_of_two());
            assert_eq!(b & IPTOS_TOS_MASK, b);
        }
    }

    #[test]
    fn test_header_constants() {
        assert_eq!(IPVERSION, 4);
        // 16-bit total length field max.
        assert_eq!(IP_MAXPACKET, 0xFFFF);
        assert_eq!(MAXTTL, u8::MAX);
    }
}

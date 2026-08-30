//! `<netinet/in.h>` and `<netinet/tcp.h>` additional constants.
//!
//! Provides IP and TCP protocol constants that supplement the
//! definitions in the `socket` module.  The `socket` module contains
//! the primary `sockaddr_in`/`sockaddr_in6` structures and core
//! constants; this module adds protocol-level option constants and
//! well-known protocol numbers.

// ---------------------------------------------------------------------------
// IP protocol numbers (IPPROTO_*)
// ---------------------------------------------------------------------------

// Core protocols (re-exported from socket for convenience).
pub use crate::socket::IPPROTO_ICMP;
pub use crate::socket::IPPROTO_ICMPV6;
pub use crate::socket::IPPROTO_IPV6;
pub use crate::socket::IPPROTO_RAW;
pub use crate::socket::IPPROTO_TCP;
pub use crate::socket::IPPROTO_UDP;

/// Internet Group Management Protocol.
pub const IPPROTO_IGMP: i32 = 2;

/// IPv4 encapsulation.
pub const IPPROTO_IPIP: i32 = 4;

/// Routing header.
pub const IPPROTO_ROUTING: i32 = 43;

/// Fragment header.
pub const IPPROTO_FRAGMENT: i32 = 44;

/// Generic Routing Encapsulation.
pub const IPPROTO_GRE: i32 = 47;

/// Encapsulating Security Payload.
pub const IPPROTO_ESP: i32 = 50;

/// Authentication Header.
pub const IPPROTO_AH: i32 = 51;

/// No next header (IPv6).
pub const IPPROTO_NONE: i32 = 59;

/// Stream Control Transmission Protocol.
pub const IPPROTO_SCTP: i32 = 132;

// ---------------------------------------------------------------------------
// IP socket options (IPPROTO_IP level)
// ---------------------------------------------------------------------------

// Re-exported from socket.
pub use crate::socket::IP_MULTICAST_LOOP;
pub use crate::socket::IP_MULTICAST_TTL;
pub use crate::socket::IP_TOS;
pub use crate::socket::IP_TTL;

/// Include IP header in data.
pub const IP_HDRINCL: i32 = 3;

/// IP options.
pub const IP_OPTIONS: i32 = 4;

/// Receive TOS with datagram.
pub const IP_RECVTOS: i32 = 13;

/// Receive TTL with datagram.
pub const IP_RECVTTL: i32 = 12;

/// Set multicast interface.
pub const IP_MULTICAST_IF: i32 = 32;

// ---------------------------------------------------------------------------
// TCP socket options (IPPROTO_TCP level)
// ---------------------------------------------------------------------------

// Re-exported from socket.
pub use crate::socket::TCP_CORK;
pub use crate::socket::TCP_KEEPCNT;
pub use crate::socket::TCP_KEEPIDLE;
pub use crate::socket::TCP_KEEPINTVL;
pub use crate::socket::TCP_MAXSEG;
pub use crate::socket::TCP_NODELAY;
pub use crate::socket::TCP_USER_TIMEOUT;

/// Enable TCP Fast Open.
pub const TCP_FASTOPEN: i32 = 23;

/// Request for delayed acknowledgments (Linux 4.14+).
pub const TCP_QUICKACK: i32 = 12;

/// TCP congestion control algorithm name.
pub const TCP_CONGESTION: i32 = 13;

/// Enable TCP timestamps.
pub const TCP_TIMESTAMP: i32 = 24;

// ---------------------------------------------------------------------------
// IPv6 socket options
// ---------------------------------------------------------------------------

// All re-exported from socket.
pub use crate::socket::IPV6_JOIN_GROUP;
pub use crate::socket::IPV6_LEAVE_GROUP;
pub use crate::socket::IPV6_MULTICAST_HOPS;
pub use crate::socket::IPV6_MULTICAST_LOOP;
pub use crate::socket::IPV6_UNICAST_HOPS;
pub use crate::socket::IPV6_V6ONLY;

// ---------------------------------------------------------------------------
// INADDR_* constants
// ---------------------------------------------------------------------------

pub use crate::socket::INADDR_ANY;
pub use crate::socket::INADDR_BROADCAST;
pub use crate::socket::INADDR_LOOPBACK;

/// Loopback network (127.0.0.0/8) in host byte order.
pub const INADDR_LOOPBACK_NET: u32 = 0x7F000000;

/// Class A network mask.
pub const IN_CLASSA_NET: u32 = 0xFF000000;
/// Class B network mask.
pub const IN_CLASSB_NET: u32 = 0xFFFF0000;
/// Class C network mask.
pub const IN_CLASSC_NET: u32 = 0xFFFFFF00;

// ---------------------------------------------------------------------------
// Well-known ports
// ---------------------------------------------------------------------------

/// FTP data port.
pub const IPPORT_FTP_DATA: u16 = 20;
/// FTP control port.
pub const IPPORT_FTP: u16 = 21;
/// SSH port.
pub const IPPORT_SSH: u16 = 22;
/// Telnet port.
pub const IPPORT_TELNET: u16 = 23;
/// SMTP port.
pub const IPPORT_SMTP: u16 = 25;
/// DNS port.
pub const IPPORT_DNS: u16 = 53;
/// HTTP port.
pub const IPPORT_HTTP: u16 = 80;
/// HTTPS port.
pub const IPPORT_HTTPS: u16 = 443;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Protocol numbers
    // -----------------------------------------------------------------------

    #[test]
    fn test_ipproto_values() {
        assert_eq!(IPPROTO_ICMP, 1);
        assert_eq!(IPPROTO_TCP, 6);
        assert_eq!(IPPROTO_UDP, 17);
        assert_eq!(IPPROTO_IPV6, 41);
        assert_eq!(IPPROTO_ICMPV6, 58);
        assert_eq!(IPPROTO_SCTP, 132);
        assert_eq!(IPPROTO_RAW, 255);
    }

    #[test]
    fn test_ipproto_distinct() {
        let protos = [
            IPPROTO_ICMP,
            IPPROTO_IGMP,
            IPPROTO_IPIP,
            IPPROTO_TCP,
            IPPROTO_UDP,
            IPPROTO_IPV6,
            IPPROTO_ROUTING,
            IPPROTO_FRAGMENT,
            IPPROTO_GRE,
            IPPROTO_ESP,
            IPPROTO_AH,
            IPPROTO_ICMPV6,
            IPPROTO_NONE,
            IPPROTO_SCTP,
            IPPROTO_RAW,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j], "protocol numbers must be distinct");
            }
        }
    }

    #[test]
    fn test_ipproto_extended_values() {
        assert_eq!(IPPROTO_IGMP, 2);
        assert_eq!(IPPROTO_IPIP, 4);
        assert_eq!(IPPROTO_ROUTING, 43);
        assert_eq!(IPPROTO_FRAGMENT, 44);
        assert_eq!(IPPROTO_GRE, 47);
        assert_eq!(IPPROTO_ESP, 50);
        assert_eq!(IPPROTO_AH, 51);
        assert_eq!(IPPROTO_NONE, 59);
    }

    // -----------------------------------------------------------------------
    // IP options
    // -----------------------------------------------------------------------

    #[test]
    fn test_ip_options() {
        assert_eq!(IP_TOS, 1);
        assert_eq!(IP_TTL, 2);
        assert_eq!(IP_HDRINCL, 3);
        assert_eq!(IP_OPTIONS, 4);
    }

    #[test]
    fn test_ip_multicast_options() {
        assert_eq!(IP_MULTICAST_IF, 32);
        assert_eq!(IP_MULTICAST_TTL, 33);
        assert_eq!(IP_MULTICAST_LOOP, 34);
    }

    #[test]
    fn test_ip_recv_options() {
        assert_eq!(IP_RECVTTL, 12);
        assert_eq!(IP_RECVTOS, 13);
        assert_ne!(IP_RECVTTL, IP_RECVTOS);
    }

    // -----------------------------------------------------------------------
    // TCP options
    // -----------------------------------------------------------------------

    #[test]
    fn test_tcp_nodelay() {
        assert_eq!(TCP_NODELAY, 1);
    }

    #[test]
    fn test_tcp_options_distinct() {
        let opts = [
            TCP_NODELAY,
            TCP_MAXSEG,
            TCP_CORK,
            TCP_KEEPIDLE,
            TCP_KEEPINTVL,
            TCP_KEEPCNT,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_tcp_extended_options() {
        assert_eq!(TCP_FASTOPEN, 23);
        assert_eq!(TCP_QUICKACK, 12);
        assert_eq!(TCP_CONGESTION, 13);
        assert_eq!(TCP_TIMESTAMP, 24);
        assert_eq!(TCP_USER_TIMEOUT, 18);
    }

    // -----------------------------------------------------------------------
    // IPv6 options
    // -----------------------------------------------------------------------

    #[test]
    fn test_ipv6_v6only() {
        assert_eq!(IPV6_V6ONLY, 26);
    }

    #[test]
    fn test_ipv6_multicast_pair() {
        assert_ne!(IPV6_JOIN_GROUP, IPV6_LEAVE_GROUP);
    }

    #[test]
    fn test_ipv6_hop_limits() {
        assert_eq!(IPV6_UNICAST_HOPS, 16);
        assert_eq!(IPV6_MULTICAST_HOPS, 18);
        assert_ne!(IPV6_UNICAST_HOPS, IPV6_MULTICAST_HOPS);
    }

    // -----------------------------------------------------------------------
    // INADDR_* constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_inaddr_any() {
        assert_eq!(INADDR_ANY, 0);
    }

    #[test]
    fn test_inaddr_loopback() {
        assert_eq!(INADDR_LOOPBACK, 0x7F000001);
    }

    #[test]
    fn test_inaddr_broadcast() {
        assert_eq!(INADDR_BROADCAST, 0xFFFFFFFF);
    }

    #[test]
    fn test_inaddr_loopback_net() {
        // The loopback network (127.0.0.0) is the loopback address
        // with the host part zeroed.
        assert_eq!(INADDR_LOOPBACK_NET, INADDR_LOOPBACK & IN_CLASSA_NET);
    }

    // -----------------------------------------------------------------------
    // Network masks
    // -----------------------------------------------------------------------

    #[test]
    fn test_class_a_net() {
        assert_eq!(IN_CLASSA_NET, 0xFF000000);
    }

    #[test]
    fn test_class_b_net() {
        assert_eq!(IN_CLASSB_NET, 0xFFFF0000);
    }

    #[test]
    fn test_class_c_net() {
        assert_eq!(IN_CLASSC_NET, 0xFFFFFF00);
    }

    #[test]
    fn test_network_mask_containment() {
        // Class A is the broadest, C the narrowest.
        assert!(IN_CLASSA_NET < IN_CLASSB_NET);
        assert!(IN_CLASSB_NET < IN_CLASSC_NET);
    }

    // -----------------------------------------------------------------------
    // Well-known ports
    // -----------------------------------------------------------------------

    #[test]
    fn test_well_known_ports() {
        assert_eq!(IPPORT_FTP_DATA, 20);
        assert_eq!(IPPORT_FTP, 21);
        assert_eq!(IPPORT_SSH, 22);
        assert_eq!(IPPORT_TELNET, 23);
        assert_eq!(IPPORT_SMTP, 25);
        assert_eq!(IPPORT_DNS, 53);
        assert_eq!(IPPORT_HTTP, 80);
        assert_eq!(IPPORT_HTTPS, 443);
    }

    #[test]
    fn test_ports_distinct() {
        let ports = [
            IPPORT_FTP_DATA,
            IPPORT_FTP,
            IPPORT_SSH,
            IPPORT_TELNET,
            IPPORT_SMTP,
            IPPORT_DNS,
            IPPORT_HTTP,
            IPPORT_HTTPS,
        ];
        for i in 0..ports.len() {
            for j in (i + 1)..ports.len() {
                assert_ne!(ports[i], ports[j], "ports must be distinct");
            }
        }
    }

    #[test]
    fn test_ports_in_privileged_range() {
        // All these well-known ports are < 1024 (privileged).
        let ports = [
            IPPORT_FTP_DATA,
            IPPORT_FTP,
            IPPORT_SSH,
            IPPORT_TELNET,
            IPPORT_SMTP,
            IPPORT_DNS,
            IPPORT_HTTP,
            IPPORT_HTTPS,
        ];
        for &p in &ports {
            assert!(p < 1024, "well-known port {} should be < 1024", p);
        }
    }

    // -----------------------------------------------------------------------
    // Cross-module consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_cross_module_ipproto_consistency() {
        // Verify re-exported values match what socket defines.
        assert_eq!(IPPROTO_TCP, crate::socket::IPPROTO_TCP);
        assert_eq!(IPPROTO_UDP, crate::socket::IPPROTO_UDP);
        assert_eq!(IPPROTO_ICMP, crate::socket::IPPROTO_ICMP);
        assert_eq!(IPPROTO_RAW, crate::socket::IPPROTO_RAW);
    }

    #[test]
    fn test_cross_module_tcp_consistency() {
        assert_eq!(TCP_NODELAY, crate::socket::TCP_NODELAY);
        assert_eq!(TCP_CORK, crate::socket::TCP_CORK);
        assert_eq!(TCP_MAXSEG, crate::socket::TCP_MAXSEG);
        assert_eq!(TCP_KEEPIDLE, crate::socket::TCP_KEEPIDLE);
    }
}

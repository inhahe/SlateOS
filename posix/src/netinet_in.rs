//! `<netinet/in.h>` — Internet address family definitions.
//!
//! Re-exports Internet protocol constants, address structures, and
//! related definitions.  This is the header-specific module for
//! `<netinet/in.h>`; the combined `netinet` module covers both
//! `<netinet/in.h>` and `<netinet/tcp.h>`.

// ---------------------------------------------------------------------------
// Address structures
// ---------------------------------------------------------------------------

pub use crate::socket::InAddr;
pub use crate::socket::In6Addr;
pub use crate::socket::SockaddrIn;
pub use crate::socket::SockaddrIn6;
pub use crate::socket::SocklenT;
pub use crate::socket::IN6ADDR_ANY_INIT;
pub use crate::socket::IN6ADDR_LOOPBACK_INIT;

// ---------------------------------------------------------------------------
// Address families
// ---------------------------------------------------------------------------

pub use crate::socket::AF_INET;
pub use crate::socket::AF_INET6;

// ---------------------------------------------------------------------------
// Protocols
// ---------------------------------------------------------------------------

pub use crate::socket::IPPROTO_IP;
pub use crate::socket::IPPROTO_ICMP;
pub use crate::socket::IPPROTO_TCP;
pub use crate::socket::IPPROTO_UDP;
pub use crate::socket::IPPROTO_IPV6;
pub use crate::socket::IPPROTO_ICMPV6;
pub use crate::socket::IPPROTO_RAW;

// Additional protocols from netinet module.
pub use crate::netinet::IPPROTO_IGMP;
pub use crate::netinet::IPPROTO_IPIP;
pub use crate::netinet::IPPROTO_GRE;
pub use crate::netinet::IPPROTO_ESP;
pub use crate::netinet::IPPROTO_AH;
pub use crate::netinet::IPPROTO_SCTP;

// ---------------------------------------------------------------------------
// Address constants
// ---------------------------------------------------------------------------

pub use crate::socket::INADDR_ANY;
pub use crate::socket::INADDR_LOOPBACK;
pub use crate::socket::INADDR_BROADCAST;
pub use crate::socket::INADDR_NONE;
pub use crate::socket::INET_ADDRSTRLEN;
pub use crate::socket::INET6_ADDRSTRLEN;

// ---------------------------------------------------------------------------
// Socket options (IP level)
// ---------------------------------------------------------------------------

pub use crate::socket::IP_TOS;
pub use crate::socket::IP_TTL;
pub use crate::socket::IP_MULTICAST_TTL;
pub use crate::socket::IP_MULTICAST_LOOP;
pub use crate::socket::IP_ADD_MEMBERSHIP;
pub use crate::socket::IP_DROP_MEMBERSHIP;

// ---------------------------------------------------------------------------
// Socket options (IPv6 level)
// ---------------------------------------------------------------------------

pub use crate::socket::IPV6_V6ONLY;
pub use crate::socket::IPV6_UNICAST_HOPS;
pub use crate::socket::IPV6_MULTICAST_HOPS;
pub use crate::socket::IPV6_MULTICAST_LOOP;
pub use crate::socket::IPV6_JOIN_GROUP;
pub use crate::socket::IPV6_LEAVE_GROUP;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_addr_size() {
        assert_eq!(core::mem::size_of::<InAddr>(), 4);
    }

    #[test]
    fn test_in6_addr_size() {
        assert_eq!(core::mem::size_of::<In6Addr>(), 16);
    }

    #[test]
    fn test_sockaddr_in_size() {
        assert_eq!(core::mem::size_of::<SockaddrIn>(), 16);
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            IPPROTO_IP, IPPROTO_ICMP, IPPROTO_TCP,
            IPPROTO_UDP, IPPROTO_IPV6, IPPROTO_RAW,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_inaddr_constants() {
        assert_eq!(INADDR_ANY, 0);
        assert_ne!(INADDR_LOOPBACK, 0);
        assert_ne!(INADDR_BROADCAST, 0);
    }

    #[test]
    fn test_addrstrlen() {
        assert_eq!(INET_ADDRSTRLEN, 16);
        assert_eq!(INET6_ADDRSTRLEN, 46);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(IPPROTO_TCP, crate::socket::IPPROTO_TCP);
        assert_eq!(AF_INET, crate::socket::AF_INET);
        assert_eq!(INADDR_ANY, crate::socket::INADDR_ANY);
    }
}

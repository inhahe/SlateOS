//! `<linux/ipv6.h>` — IPv6 protocol header and socket options.
//!
//! Provides the IPv6 header structure, fragment header, and
//! Linux-specific IPv6 socket options beyond the POSIX defaults.

// ---------------------------------------------------------------------------
// IPv6 header
// ---------------------------------------------------------------------------

/// IPv6 packet header (40 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ipv6hdr {
    /// Version (4), traffic class (8), flow label (20).
    pub priority_flow: u32,
    /// Payload length.
    pub payload_len: u16,
    /// Next header (protocol).
    pub nexthdr: u8,
    /// Hop limit.
    pub hop_limit: u8,
    /// Source address.
    pub saddr: [u8; 16],
    /// Destination address.
    pub daddr: [u8; 16],
}

impl Ipv6hdr {
    /// Create a zeroed IPv6 header.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// IPv6 fragment header
// ---------------------------------------------------------------------------

/// IPv6 fragment header (8 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ipv6FragHdr {
    /// Next header.
    pub nexthdr: u8,
    /// Reserved.
    pub reserved: u8,
    /// Fragment offset (13 bits), res (2 bits), MF (1 bit).
    pub frag_off: u16,
    /// Identification.
    pub identification: u32,
}

// ---------------------------------------------------------------------------
// IPv6 extension header types (next header values)
// ---------------------------------------------------------------------------

/// Hop-by-hop options.
pub const IPPROTO_HOPOPTS: u8 = 0;
/// Routing header.
pub const IPPROTO_ROUTING: u8 = 43;
/// Fragment header.
pub const IPPROTO_FRAGMENT: u8 = 44;
/// Encapsulating security payload.
pub const IPPROTO_ESP: u8 = 50;
/// Authentication header.
pub const IPPROTO_AH: u8 = 51;
/// ICMPv6.
pub const IPPROTO_ICMPV6: u8 = 58;
/// No next header.
pub const IPPROTO_NONE: u8 = 59;
/// Destination options.
pub const IPPROTO_DSTOPTS: u8 = 60;
/// Mobility header.
pub const IPPROTO_MH: u8 = 135;

// ---------------------------------------------------------------------------
// IPv6 socket options (SOL_IPV6 = 41)
// ---------------------------------------------------------------------------

/// Socket option level for IPv6.
pub const SOL_IPV6: i32 = 41;
/// Restrict to IPv6 only (no IPv4-mapped).
pub const IPV6_V6ONLY: i32 = 26;
/// Set unicast hop limit.
pub const IPV6_UNICAST_HOPS: i32 = 16;
/// Set multicast hop limit.
pub const IPV6_MULTICAST_HOPS: i32 = 18;
/// Set multicast interface.
pub const IPV6_MULTICAST_IF: i32 = 17;
/// Set multicast loopback.
pub const IPV6_MULTICAST_LOOP: i32 = 19;
/// Join multicast group.
pub const IPV6_ADD_MEMBERSHIP: i32 = 20;
/// Leave multicast group.
pub const IPV6_DROP_MEMBERSHIP: i32 = 21;
/// Receive packet info.
pub const IPV6_RECVPKTINFO: i32 = 49;
/// Packet info.
pub const IPV6_PKTINFO: i32 = 50;
/// Receive hop limit.
pub const IPV6_RECVHOPLIMIT: i32 = 51;
/// Receive traffic class.
pub const IPV6_RECVTCLASS: i32 = 66;
/// Set traffic class.
pub const IPV6_TCLASS: i32 = 67;
/// Transparent proxy.
pub const IPV6_TRANSPARENT: i32 = 75;
/// Receive original destination address.
pub const IPV6_RECVORIGDSTADDR: i32 = 74;
/// Auto-flowlabel.
pub const IPV6_AUTOFLOWLABEL: i32 = 70;
/// Don't fragment.
pub const IPV6_DONTFRAG: i32 = 62;

// ---------------------------------------------------------------------------
// IPv6 constants
// ---------------------------------------------------------------------------

/// IPv6 header length.
pub const IPV6_HDRLEN: usize = 40;
/// Maximum MTU for IPv6.
pub const IPV6_MAXPLEN: u16 = 65535;
/// Minimum MTU for IPv6.
pub const IPV6_MIN_MTU: u16 = 1280;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv6hdr_size() {
        assert_eq!(core::mem::size_of::<Ipv6hdr>(), 40);
    }

    #[test]
    fn test_frag_hdr_size() {
        assert_eq!(core::mem::size_of::<Ipv6FragHdr>(), 8);
    }

    #[test]
    fn test_ipv6hdr_zeroed() {
        let hdr = Ipv6hdr::zeroed();
        assert_eq!(hdr.payload_len, 0);
        assert_eq!(hdr.nexthdr, 0);
        assert_eq!(hdr.hop_limit, 0);
        assert_eq!(hdr.saddr, [0u8; 16]);
        assert_eq!(hdr.daddr, [0u8; 16]);
    }

    #[test]
    fn test_extension_headers_distinct() {
        let hdrs = [
            IPPROTO_HOPOPTS, IPPROTO_ROUTING, IPPROTO_FRAGMENT,
            IPPROTO_ESP, IPPROTO_AH, IPPROTO_ICMPV6,
            IPPROTO_NONE, IPPROTO_DSTOPTS, IPPROTO_MH,
        ];
        for i in 0..hdrs.len() {
            for j in (i + 1)..hdrs.len() {
                assert_ne!(hdrs[i], hdrs[j]);
            }
        }
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            IPV6_V6ONLY, IPV6_UNICAST_HOPS, IPV6_MULTICAST_HOPS,
            IPV6_MULTICAST_IF, IPV6_MULTICAST_LOOP,
            IPV6_ADD_MEMBERSHIP, IPV6_DROP_MEMBERSHIP,
            IPV6_RECVPKTINFO, IPV6_PKTINFO, IPV6_RECVHOPLIMIT,
            IPV6_RECVTCLASS, IPV6_TCLASS, IPV6_DONTFRAG,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_ipv6_constants() {
        assert_eq!(IPV6_HDRLEN, 40);
        assert_eq!(IPV6_MIN_MTU, 1280);
    }
}

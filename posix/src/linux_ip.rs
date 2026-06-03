//! `<linux/ip.h>` — IP protocol header and options.
//!
//! Provides the IP header structure, option types, and TOS
//! (type of service) constants.

// Re-export IP protocol numbers and address types.
pub use crate::socket::IPPROTO_ICMP;
pub use crate::socket::IPPROTO_TCP;
pub use crate::socket::IPPROTO_UDP;

// ---------------------------------------------------------------------------
// IP header
// ---------------------------------------------------------------------------

/// IPv4 header (20 bytes without options).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Iphdr {
    /// Version (4 bits) + IHL (4 bits).
    /// On little-endian: ihl in low nibble, version in high.
    pub version_ihl: u8,
    /// Type of Service.
    pub tos: u8,
    /// Total length (header + data).
    pub tot_len: u16,
    /// Identification.
    pub id: u16,
    /// Fragment offset + flags.
    pub frag_off: u16,
    /// Time to live.
    pub ttl: u8,
    /// Protocol (IPPROTO_*).
    pub protocol: u8,
    /// Header checksum.
    pub check: u16,
    /// Source address.
    pub saddr: u32,
    /// Destination address.
    pub daddr: u32,
}

// ---------------------------------------------------------------------------
// IP fragment flags
// ---------------------------------------------------------------------------

/// Reserved flag (must be zero).
pub const IP_RF: u16 = 0x8000;
/// Don't Fragment.
pub const IP_DF: u16 = 0x4000;
/// More Fragments.
pub const IP_MF: u16 = 0x2000;
/// Fragment offset mask.
pub const IP_OFFMASK: u16 = 0x1FFF;

// ---------------------------------------------------------------------------
// TOS (Type of Service) values
// ---------------------------------------------------------------------------

/// Normal service.
pub const IPTOS_TOS_MASK: u8 = 0x1E;
/// Minimize delay.
pub const IPTOS_LOWDELAY: u8 = 0x10;
/// Maximize throughput.
pub const IPTOS_THROUGHPUT: u8 = 0x08;
/// Maximize reliability.
pub const IPTOS_RELIABILITY: u8 = 0x04;
/// Minimize cost.
pub const IPTOS_MINCOST: u8 = 0x02;

// ---------------------------------------------------------------------------
// DSCP (Differentiated Services Code Point) classes
// ---------------------------------------------------------------------------

/// Class selector 0 (best effort).
pub const IPTOS_CLASS_CS0: u8 = 0x00;
/// Class selector 1.
pub const IPTOS_CLASS_CS1: u8 = 0x20;
/// Class selector 2.
pub const IPTOS_CLASS_CS2: u8 = 0x40;
/// Class selector 3.
pub const IPTOS_CLASS_CS3: u8 = 0x60;
/// Class selector 4.
pub const IPTOS_CLASS_CS4: u8 = 0x80;
/// Class selector 5.
pub const IPTOS_CLASS_CS5: u8 = 0xA0;
/// Class selector 6.
pub const IPTOS_CLASS_CS6: u8 = 0xC0;
/// Class selector 7.
pub const IPTOS_CLASS_CS7: u8 = 0xE0;

// ---------------------------------------------------------------------------
// IP option types
// ---------------------------------------------------------------------------

/// End of option list.
pub const IPOPT_END: u8 = 0;
/// No operation.
pub const IPOPT_NOOP: u8 = 1;
/// Record route.
pub const IPOPT_RR: u8 = 7;
/// Timestamp.
pub const IPOPT_TIMESTAMP: u8 = 68;
/// Loose source routing.
pub const IPOPT_LSRR: u8 = 131;
/// Strict source routing.
pub const IPOPT_SSRR: u8 = 137;
/// Router alert.
pub const IPOPT_RA: u8 = 148;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum IP packet size.
pub const IP_MAXPACKET: usize = 65535;
/// Minimum IP header length (without options).
pub const IP_MINHDRSIZE: usize = 20;
/// Maximum IP header length (with options).
pub const IP_MAXHDRSIZE: usize = 60;
/// Default TTL.
pub const IPDEFTTL: u8 = 64;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iphdr_size() {
        assert_eq!(core::mem::size_of::<Iphdr>(), 20);
    }

    #[test]
    fn test_fragment_flags() {
        assert_eq!(IP_DF & IP_MF, 0);
        assert_eq!(IP_RF & IP_DF, 0);
        // Offset mask covers low 13 bits.
        assert_eq!(IP_OFFMASK, 0x1FFF);
    }

    #[test]
    fn test_tos_values() {
        assert_eq!(IPTOS_LOWDELAY & IPTOS_THROUGHPUT, 0);
        assert_eq!(IPTOS_RELIABILITY & IPTOS_MINCOST, 0);
    }

    #[test]
    fn test_dscp_classes() {
        assert_eq!(IPTOS_CLASS_CS0, 0);
        assert_eq!(IPTOS_CLASS_CS7, 0xE0);
    }

    #[test]
    fn test_ip_options() {
        assert_eq!(IPOPT_END, 0);
        assert_eq!(IPOPT_NOOP, 1);
        assert_ne!(IPOPT_RR, IPOPT_TIMESTAMP);
    }

    #[test]
    fn test_constants() {
        assert_eq!(IP_MAXPACKET, 65535);
        assert_eq!(IP_MINHDRSIZE, 20);
        assert_eq!(IP_MAXHDRSIZE, 60);
        assert_eq!(IPDEFTTL, 64);
    }
}

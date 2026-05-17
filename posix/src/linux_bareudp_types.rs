//! `<linux/bareudp.h>` — Bare UDP tunnel device constants.
//!
//! bareudp is a minimal UDP tunnel device that encapsulates L3
//! protocols (MPLS, IP, etc.) directly in UDP without any additional
//! tunnel header. Unlike VXLAN or Geneve, there is no shim header —
//! the inner protocol is placed immediately after the UDP header.
//! This provides the benefits of UDP encapsulation (NAT traversal,
//! ECMP via source port hashing) with zero overhead beyond the UDP
//! header itself. Used primarily for MPLS-in-UDP (RFC 7510).

// ---------------------------------------------------------------------------
// bareudp netlink attributes (IFLA_BAREUDP_*)
// ---------------------------------------------------------------------------

/// Local listening port.
pub const IFLA_BAREUDP_PORT: u32 = 1;
/// Inner EtherType (e.g., ETH_P_MPLS_UC for MPLS unicast).
pub const IFLA_BAREUDP_ETHERTYPE: u32 = 2;
/// Source port range minimum.
pub const IFLA_BAREUDP_SRCPORT_MIN: u32 = 3;
/// Enable multicast/broadcast mode.
pub const IFLA_BAREUDP_MULTIPROTO_MODE: u32 = 4;

// ---------------------------------------------------------------------------
// Common EtherTypes used with bareudp
// ---------------------------------------------------------------------------

/// MPLS unicast EtherType.
pub const ETH_P_MPLS_UC: u16 = 0x8847;
/// MPLS multicast EtherType.
pub const ETH_P_MPLS_MC: u16 = 0x8848;
/// IPv4 EtherType.
pub const ETH_P_IP_BAREUDP: u16 = 0x0800;
/// IPv6 EtherType.
pub const ETH_P_IPV6_BAREUDP: u16 = 0x86DD;

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

/// Default bareudp port for MPLS-in-UDP (RFC 7510).
pub const BAREUDP_MPLS_PORT: u16 = 6635;
/// Minimum source port for entropy (hashing).
pub const BAREUDP_SRCPORT_MIN_DEFAULT: u16 = 49152;
/// Maximum source port.
pub const BAREUDP_SRCPORT_MAX: u16 = 65535;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifla_attrs_distinct() {
        let attrs = [
            IFLA_BAREUDP_PORT, IFLA_BAREUDP_ETHERTYPE,
            IFLA_BAREUDP_SRCPORT_MIN, IFLA_BAREUDP_MULTIPROTO_MODE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ethertypes_distinct() {
        let types = [
            ETH_P_MPLS_UC, ETH_P_MPLS_MC,
            ETH_P_IP_BAREUDP, ETH_P_IPV6_BAREUDP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_mpls_port() {
        assert_eq!(BAREUDP_MPLS_PORT, 6635);
    }

    #[test]
    fn test_srcport_range() {
        assert!(BAREUDP_SRCPORT_MIN_DEFAULT < BAREUDP_SRCPORT_MAX);
    }

    #[test]
    fn test_mpls_ethertypes() {
        // MPLS unicast and multicast are adjacent EtherType values
        assert_eq!(ETH_P_MPLS_MC, ETH_P_MPLS_UC + 1);
    }
}

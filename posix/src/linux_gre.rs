//! `<linux/if_tunnel.h>` — GRE/IP tunnel constants.
//!
//! GRE (Generic Routing Encapsulation) tunnels encapsulate
//! arbitrary network-layer protocols inside IP. Used for
//! site-to-site VPNs, PPTP, and network virtualization.
//! Linux supports GRE, GRETAP (Ethernet over GRE), IP-in-IP,
//! and SIT (IPv6-in-IPv4) tunnels.

// ---------------------------------------------------------------------------
// GRE flags
// ---------------------------------------------------------------------------

/// Checksum present.
pub const GRE_CSUM: u16 = 1 << 15;
/// Routing present (deprecated).
pub const GRE_ROUTING: u16 = 1 << 14;
/// Key present.
pub const GRE_KEY: u16 = 1 << 13;
/// Sequence number present.
pub const GRE_SEQ: u16 = 1 << 12;
/// Strict source route (deprecated).
pub const GRE_STRICT: u16 = 1 << 11;
/// Acknowledgment present (enhanced GRE).
pub const GRE_ACK: u16 = 1 << 7;

// ---------------------------------------------------------------------------
// GRE protocol types (EtherType in GRE header)
// ---------------------------------------------------------------------------

/// Transparent Ethernet bridging.
pub const GRE_PROTO_TEB: u16 = 0x6558;
/// ERSPAN Type II.
pub const GRE_PROTO_ERSPAN_II: u16 = 0x88BE;

// ---------------------------------------------------------------------------
// IP protocol numbers
// ---------------------------------------------------------------------------

/// GRE protocol number.
pub const IPPROTO_GRE: u8 = 47;
/// IP-in-IP protocol number.
pub const IPPROTO_IPIP: u8 = 4;
/// IPv6-in-IPv4 (SIT) protocol number.
pub const IPPROTO_IPV6_IN_IPV4: u8 = 41;

// ---------------------------------------------------------------------------
// Tunnel ioctl commands
// ---------------------------------------------------------------------------

/// Add tunnel.
pub const SIOCADDTUNNEL: u32 = 0x89F1;
/// Delete tunnel.
pub const SIOCDELTUNNEL: u32 = 0x89F2;
/// Change tunnel parameters.
pub const SIOCCHGTUNNEL: u32 = 0x89F3;
/// Get tunnel parameters.
pub const SIOCGETTUNNEL: u32 = 0x89F0;

// ---------------------------------------------------------------------------
// Tunnel flags
// ---------------------------------------------------------------------------

/// Don't fragment inner packets.
pub const TUNNEL_DONT_FRAGMENT: u32 = 1 << 0;
/// Copy TOS from inner to outer.
pub const TUNNEL_SEQ: u32 = 1 << 1;
/// Use GRE key.
pub const TUNNEL_KEY: u32 = 1 << 2;
/// Use GRE checksum.
pub const TUNNEL_CSUM: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Link types
// ---------------------------------------------------------------------------

/// IFLA_INFO_KIND for GRE.
pub const GRE_KIND: &str = "gre";
/// IFLA_INFO_KIND for GRETAP (Ethernet over GRE).
pub const GRETAP_KIND: &str = "gretap";
/// IFLA_INFO_KIND for IP-in-IP tunnel.
pub const IPIP_KIND: &str = "ipip";
/// IFLA_INFO_KIND for SIT (IPv6-in-IPv4).
pub const SIT_KIND: &str = "sit";
/// IFLA_INFO_KIND for GRE over IPv6.
pub const IP6GRE_KIND: &str = "ip6gre";
/// IFLA_INFO_KIND for GRETAP over IPv6.
pub const IP6GRETAP_KIND: &str = "ip6gretap";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gre_flags_powers_of_two() {
        let flags = [GRE_CSUM, GRE_ROUTING, GRE_KEY, GRE_SEQ, GRE_STRICT, GRE_ACK];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_gre_flags_no_overlap() {
        let flags = [GRE_CSUM, GRE_ROUTING, GRE_KEY, GRE_SEQ, GRE_STRICT, GRE_ACK];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_proto_types_distinct() {
        assert_ne!(GRE_PROTO_TEB, GRE_PROTO_ERSPAN_II);
    }

    #[test]
    fn test_ip_protos_distinct() {
        let protos = [IPPROTO_GRE, IPPROTO_IPIP, IPPROTO_IPV6_IN_IPV4];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [SIOCADDTUNNEL, SIOCDELTUNNEL, SIOCCHGTUNNEL, SIOCGETTUNNEL];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_kinds_distinct() {
        let kinds = [GRE_KIND, GRETAP_KIND, IPIP_KIND, SIT_KIND, IP6GRE_KIND, IP6GRETAP_KIND];
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i], kinds[j]);
            }
        }
    }
}

//! `<linux/if_tunnel.h>` — Additional GRE tunnel constants.
//!
//! Supplementary GRE constants covering GRE flags,
//! tunnel types, and ERSPAN versions.

// ---------------------------------------------------------------------------
// GRE header flags
// ---------------------------------------------------------------------------

/// Checksum present.
pub const GRE_CSUM: u16 = 0x8000;
/// Routing present.
pub const GRE_ROUTING: u16 = 0x4000;
/// Key present.
pub const GRE_KEY: u16 = 0x2000;
/// Sequence number present.
pub const GRE_SEQ: u16 = 0x1000;
/// Strict source route.
pub const GRE_STRICT: u16 = 0x0800;
/// Recursion control.
pub const GRE_REC: u16 = 0x0700;
/// Acknowledgment present.
pub const GRE_ACK: u16 = 0x0080;
/// GRE version mask.
pub const GRE_VERSION: u16 = 0x0007;

// ---------------------------------------------------------------------------
// Tunnel types
// ---------------------------------------------------------------------------

/// GRE tunnel (IPv4).
pub const TUNNEL_TYPE_GRE: u32 = 0;
/// GRE tunnel (IPv6).
pub const TUNNEL_TYPE_GRE6: u32 = 1;
/// IP-in-IP tunnel (IPv4).
pub const TUNNEL_TYPE_IPIP: u32 = 2;
/// IP-in-IP tunnel (IPv6).
pub const TUNNEL_TYPE_IPIP6: u32 = 3;
/// SIT (Simple Internet Transition) tunnel.
pub const TUNNEL_TYPE_SIT: u32 = 4;
/// VTI (Virtual Tunnel Interface) tunnel.
pub const TUNNEL_TYPE_VTI: u32 = 5;
/// VTI6 tunnel.
pub const TUNNEL_TYPE_VTI6: u32 = 6;

// ---------------------------------------------------------------------------
// ERSPAN versions
// ---------------------------------------------------------------------------

/// ERSPAN Type II.
pub const ERSPAN_VERSION_1: u32 = 1;
/// ERSPAN Type III.
pub const ERSPAN_VERSION_2: u32 = 2;

// ---------------------------------------------------------------------------
// Tunnel flags (TUNNEL_*)
// ---------------------------------------------------------------------------

/// Tunnel has CSUM.
pub const TUNNEL_CSUM: u32 = 1 << 0;
/// Tunnel routing present.
pub const TUNNEL_ROUTING: u32 = 1 << 1;
/// Tunnel key present.
pub const TUNNEL_KEY: u32 = 1 << 2;
/// Tunnel sequence present.
pub const TUNNEL_SEQ: u32 = 1 << 3;
/// Don't fragment.
pub const TUNNEL_DONT_FRAGMENT: u32 = 1 << 4;
/// Outer IP options present.
pub const TUNNEL_OAM: u32 = 1 << 5;
/// Critical options present.
pub const TUNNEL_CRIT_OPT: u32 = 1 << 6;
/// Geneve options present.
pub const TUNNEL_GENEVE_OPT: u32 = 1 << 7;
/// VXLAN options present.
pub const TUNNEL_VXLAN_OPT: u32 = 1 << 8;
/// Encap ECN.
pub const TUNNEL_ERSPAN_OPT: u32 = 1 << 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gre_flags_distinct() {
        let flags = [GRE_CSUM, GRE_ROUTING, GRE_KEY, GRE_SEQ, GRE_STRICT];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_tunnel_types_distinct() {
        let types = [
            TUNNEL_TYPE_GRE,
            TUNNEL_TYPE_GRE6,
            TUNNEL_TYPE_IPIP,
            TUNNEL_TYPE_IPIP6,
            TUNNEL_TYPE_SIT,
            TUNNEL_TYPE_VTI,
            TUNNEL_TYPE_VTI6,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_erspan_versions_distinct() {
        assert_ne!(ERSPAN_VERSION_1, ERSPAN_VERSION_2);
    }

    #[test]
    fn test_tunnel_flags_power_of_two() {
        let flags = [
            TUNNEL_CSUM,
            TUNNEL_ROUTING,
            TUNNEL_KEY,
            TUNNEL_SEQ,
            TUNNEL_DONT_FRAGMENT,
            TUNNEL_OAM,
            TUNNEL_CRIT_OPT,
            TUNNEL_GENEVE_OPT,
            TUNNEL_VXLAN_OPT,
            TUNNEL_ERSPAN_OPT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_tunnel_flags_no_overlap() {
        let flags = [
            TUNNEL_CSUM,
            TUNNEL_ROUTING,
            TUNNEL_KEY,
            TUNNEL_SEQ,
            TUNNEL_DONT_FRAGMENT,
            TUNNEL_OAM,
            TUNNEL_CRIT_OPT,
            TUNNEL_GENEVE_OPT,
            TUNNEL_VXLAN_OPT,
            TUNNEL_ERSPAN_OPT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

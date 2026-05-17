//! `<linux/if_tunnel.h>` — IP tunnel constants.
//!
//! Linux supports multiple tunneling protocols that encapsulate
//! packets inside other packets for overlay networks, VPNs, and
//! carrier interconnects. Key types: IPIP (IPv4-in-IPv4), GRE
//! (Generic Routing Encapsulation), SIT (IPv6-in-IPv4), VTI
//! (Virtual Tunnel Interface for IPsec), and VXLAN.

// ---------------------------------------------------------------------------
// Tunnel types
// ---------------------------------------------------------------------------

/// IPIP tunnel (IPv4 in IPv4).
pub const TUNNEL_TYPE_IPIP: u32 = 0;
/// GRE tunnel (Generic Routing Encapsulation).
pub const TUNNEL_TYPE_GRE: u32 = 1;
/// SIT tunnel (IPv6 in IPv4, 6to4).
pub const TUNNEL_TYPE_SIT: u32 = 2;
/// ISATAP (Intra-Site Automatic Tunnel).
pub const TUNNEL_TYPE_ISATAP: u32 = 3;
/// VTI (Virtual Tunnel Interface, for IPsec).
pub const TUNNEL_TYPE_VTI: u32 = 4;
/// IP6IP6 (IPv6 in IPv6).
pub const TUNNEL_TYPE_IP6IP6: u32 = 5;
/// IP6GRE (GRE over IPv6).
pub const TUNNEL_TYPE_IP6GRE: u32 = 6;

// ---------------------------------------------------------------------------
// GRE flags (in GRE header)
// ---------------------------------------------------------------------------

/// Checksum present.
pub const GRE_CSUM: u16 = 0x8000;
/// Routing present (deprecated).
pub const GRE_ROUTING: u16 = 0x4000;
/// Key present.
pub const GRE_KEY: u16 = 0x2000;
/// Sequence number present.
pub const GRE_SEQ: u16 = 0x1000;
/// Strict source routing (deprecated).
pub const GRE_STRICT: u16 = 0x0800;
/// GRE version 0.
pub const GRE_VERSION_0: u16 = 0x0000;

// ---------------------------------------------------------------------------
// Tunnel flags (ioctl/netlink)
// ---------------------------------------------------------------------------

/// Don't fragment inner packet.
pub const TUNNEL_FLAG_DF: u32 = 1 << 0;
/// Copy TOS from inner to outer header.
pub const TUNNEL_FLAG_TOS_INHERIT: u32 = 1 << 1;
/// Copy TTL from inner to outer header.
pub const TUNNEL_FLAG_TTL_INHERIT: u32 = 1 << 2;
/// Enable path MTU discovery.
pub const TUNNEL_FLAG_PMTUDISC: u32 = 1 << 3;
/// Ignore DF bit from inner header.
pub const TUNNEL_FLAG_IGNORE_DF: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// VXLAN flags
// ---------------------------------------------------------------------------

/// VXLAN default port.
pub const VXLAN_PORT: u16 = 4789;
/// VXLAN GPE (Generic Protocol Extension) port.
pub const VXLAN_GPE_PORT: u16 = 4790;
/// VXLAN VNI mask (24 bits).
pub const VXLAN_VNI_MASK: u32 = 0x00FF_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunnel_types_distinct() {
        let types = [
            TUNNEL_TYPE_IPIP, TUNNEL_TYPE_GRE, TUNNEL_TYPE_SIT,
            TUNNEL_TYPE_ISATAP, TUNNEL_TYPE_VTI,
            TUNNEL_TYPE_IP6IP6, TUNNEL_TYPE_IP6GRE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_gre_flags_no_overlap() {
        let flags = [GRE_CSUM, GRE_ROUTING, GRE_KEY, GRE_SEQ, GRE_STRICT];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_tunnel_flags_no_overlap() {
        let flags = [
            TUNNEL_FLAG_DF, TUNNEL_FLAG_TOS_INHERIT,
            TUNNEL_FLAG_TTL_INHERIT, TUNNEL_FLAG_PMTUDISC,
            TUNNEL_FLAG_IGNORE_DF,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_vxlan_ports() {
        assert_ne!(VXLAN_PORT, VXLAN_GPE_PORT);
    }

    #[test]
    fn test_vxlan_vni_mask() {
        assert_eq!(VXLAN_VNI_MASK, 0x00FF_FFFF);
    }
}

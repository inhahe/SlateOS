//! `<linux/if_tunnel.h>` — GRE tunnel constants.
//!
//! GRE (Generic Routing Encapsulation) tunnels encapsulate
//! arbitrary network protocols within IP.  These constants
//! define GRE header flags, netlink attributes, and ERSPAN
//! parameters.

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
/// Record route.
pub const GRE_REC: u16 = 0x0700;
/// Acknowledgment present.
pub const GRE_ACK: u16 = 0x0080;
/// GRE flags mask.
pub const GRE_FLAGS: u16 = 0x00F8;
/// GRE version mask.
pub const GRE_VERSION: u16 = 0x0007;

// ---------------------------------------------------------------------------
// GRE netlink attribute types (IFLA_GRE_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_GRE_UNSPEC: u32 = 0;
/// Link (parent interface).
pub const IFLA_GRE_LINK: u32 = 1;
/// Input flags.
pub const IFLA_GRE_IFLAGS: u32 = 2;
/// Output flags.
pub const IFLA_GRE_OFLAGS: u32 = 3;
/// Input key.
pub const IFLA_GRE_IKEY: u32 = 4;
/// Output key.
pub const IFLA_GRE_OKEY: u32 = 5;
/// Local address.
pub const IFLA_GRE_LOCAL: u32 = 6;
/// Remote address.
pub const IFLA_GRE_REMOTE: u32 = 7;
/// TTL.
pub const IFLA_GRE_TTL: u32 = 8;
/// TOS.
pub const IFLA_GRE_TOS: u32 = 9;
/// PMTU discovery.
pub const IFLA_GRE_PMTUDISC: u32 = 10;
/// Encapsulation limit.
pub const IFLA_GRE_ENCAP_LIMIT: u32 = 11;
/// Flow info.
pub const IFLA_GRE_FLOWINFO: u32 = 12;
/// Flags.
pub const IFLA_GRE_FLAGS: u32 = 13;
/// Encapsulation type.
pub const IFLA_GRE_ENCAP_TYPE: u32 = 14;
/// Encapsulation flags.
pub const IFLA_GRE_ENCAP_FLAGS: u32 = 15;
/// Encapsulation source port.
pub const IFLA_GRE_ENCAP_SPORT: u32 = 16;
/// Encapsulation dest port.
pub const IFLA_GRE_ENCAP_DPORT: u32 = 17;
/// Collect metadata.
pub const IFLA_GRE_COLLECT_METADATA: u32 = 18;
/// Ignore DF.
pub const IFLA_GRE_IGNORE_DF: u32 = 19;
/// FW mark.
pub const IFLA_GRE_FWMARK: u32 = 20;
/// ERSPAN index.
pub const IFLA_GRE_ERSPAN_INDEX: u32 = 21;
/// ERSPAN version.
pub const IFLA_GRE_ERSPAN_VER: u32 = 22;
/// ERSPAN direction.
pub const IFLA_GRE_ERSPAN_DIR: u32 = 23;
/// ERSPAN hardware ID.
pub const IFLA_GRE_ERSPAN_HWID: u32 = 24;

// ---------------------------------------------------------------------------
// GRE protocol types
// ---------------------------------------------------------------------------

/// IPv4 over GRE.
pub const GRE_PROTO_IP: u16 = 0x0800;
/// IPv6 over GRE.
pub const GRE_PROTO_IPV6: u16 = 0x86DD;
/// Ethernet over GRE (GRETAP).
pub const GRE_PROTO_ETH: u16 = 0x6558;
/// ERSPAN Type II.
pub const GRE_PROTO_ERSPAN_II: u16 = 0x88BE;
/// Transparent Ethernet Bridging.
pub const GRE_PROTO_TEB: u16 = 0x6558;

// ---------------------------------------------------------------------------
// ERSPAN direction
// ---------------------------------------------------------------------------

/// Ingress.
pub const ERSPAN_DIR_INGRESS: u32 = 0;
/// Egress.
pub const ERSPAN_DIR_EGRESS: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gre_attrs_distinct() {
        let attrs = [
            IFLA_GRE_UNSPEC,
            IFLA_GRE_LINK,
            IFLA_GRE_IFLAGS,
            IFLA_GRE_OFLAGS,
            IFLA_GRE_IKEY,
            IFLA_GRE_OKEY,
            IFLA_GRE_LOCAL,
            IFLA_GRE_REMOTE,
            IFLA_GRE_TTL,
            IFLA_GRE_TOS,
            IFLA_GRE_PMTUDISC,
            IFLA_GRE_ENCAP_LIMIT,
            IFLA_GRE_FLOWINFO,
            IFLA_GRE_FLAGS,
            IFLA_GRE_ENCAP_TYPE,
            IFLA_GRE_ENCAP_FLAGS,
            IFLA_GRE_ENCAP_SPORT,
            IFLA_GRE_ENCAP_DPORT,
            IFLA_GRE_COLLECT_METADATA,
            IFLA_GRE_IGNORE_DF,
            IFLA_GRE_FWMARK,
            IFLA_GRE_ERSPAN_INDEX,
            IFLA_GRE_ERSPAN_VER,
            IFLA_GRE_ERSPAN_DIR,
            IFLA_GRE_ERSPAN_HWID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_gre_header_values() {
        assert_eq!(GRE_CSUM, 0x8000);
        assert_eq!(GRE_KEY, 0x2000);
        assert_eq!(GRE_SEQ, 0x1000);
    }

    #[test]
    fn test_proto_types_values() {
        assert_eq!(GRE_PROTO_IP, 0x0800);
        assert_eq!(GRE_PROTO_IPV6, 0x86DD);
    }

    #[test]
    fn test_erspan_dirs_distinct() {
        assert_ne!(ERSPAN_DIR_INGRESS, ERSPAN_DIR_EGRESS);
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(IFLA_GRE_UNSPEC, 0);
    }

    #[test]
    fn test_ingress_is_zero() {
        assert_eq!(ERSPAN_DIR_INGRESS, 0);
    }
}

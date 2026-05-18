//! `<linux/ip_tunnel.h>` — IP tunnel constants.
//!
//! IP tunnels encapsulate IP packets within IP.  These
//! constants define tunnel flags, IOCTL commands, encapsulation
//! types, and netlink attributes for IPIP, SIT, and GRE tunnels.

// ---------------------------------------------------------------------------
// Tunnel flags (TUNNEL_*)
// ---------------------------------------------------------------------------

/// Checksum present.
pub const TUNNEL_CSUM: u32 = 1 << 0;
/// Routing present.
pub const TUNNEL_ROUTING: u32 = 1 << 1;
/// Key present.
pub const TUNNEL_KEY: u32 = 1 << 2;
/// Sequence number present.
pub const TUNNEL_SEQ: u32 = 1 << 3;
/// Strict route.
pub const TUNNEL_STRICT: u32 = 1 << 4;
/// Record route.
pub const TUNNEL_REC: u32 = 1 << 5;
/// Version 1 (PPTP/GRE).
pub const TUNNEL_VERSION: u32 = 1 << 6;
/// No key (ignore key field).
pub const TUNNEL_NO_KEY: u32 = 1 << 7;
/// Don't fragment.
pub const TUNNEL_DONT_FRAGMENT: u32 = 1 << 8;
/// OAM (operations and maintenance).
pub const TUNNEL_OAM: u32 = 1 << 9;
/// Critical option.
pub const TUNNEL_CRIT_OPT: u32 = 1 << 10;
/// GENEVE options present.
pub const TUNNEL_GENEVE_OPT: u32 = 1 << 11;
/// VXLAN options present.
pub const TUNNEL_VXLAN_OPT: u32 = 1 << 12;
/// Detect PMTU.
pub const TUNNEL_NOCACHE: u32 = 1 << 13;
/// ERSPAN options present.
pub const TUNNEL_ERSPAN_OPT: u32 = 1 << 14;
/// GTP options present.
pub const TUNNEL_GTP_OPT: u32 = 1 << 15;

// ---------------------------------------------------------------------------
// Tunnel IOCTL commands
// ---------------------------------------------------------------------------

/// Add tunnel.
pub const SIOCADDTUNNEL: u32 = 0x89F1;
/// Delete tunnel.
pub const SIOCDELTUNNEL: u32 = 0x89F2;
/// Change tunnel.
pub const SIOCCHGTUNNEL: u32 = 0x89F3;
/// Get tunnel.
pub const SIOCGETTUNNEL: u32 = 0x89F0;

// ---------------------------------------------------------------------------
// Tunnel netlink attribute types (IFLA_IPTUN_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_IPTUN_UNSPEC: u32 = 0;
/// Link (parent interface).
pub const IFLA_IPTUN_LINK: u32 = 1;
/// Local address.
pub const IFLA_IPTUN_LOCAL: u32 = 2;
/// Remote address.
pub const IFLA_IPTUN_REMOTE: u32 = 3;
/// TTL.
pub const IFLA_IPTUN_TTL: u32 = 4;
/// TOS.
pub const IFLA_IPTUN_TOS: u32 = 5;
/// Encapsulation limit.
pub const IFLA_IPTUN_ENCAP_LIMIT: u32 = 6;
/// Flow info.
pub const IFLA_IPTUN_FLOWINFO: u32 = 7;
/// Flags.
pub const IFLA_IPTUN_FLAGS: u32 = 8;
/// Protocol.
pub const IFLA_IPTUN_PROTO: u32 = 9;
/// PMTU discovery.
pub const IFLA_IPTUN_PMTUDISC: u32 = 10;
/// 6rd prefix.
pub const IFLA_IPTUN_6RD_PREFIX: u32 = 11;
/// 6rd relay prefix.
pub const IFLA_IPTUN_6RD_RELAY_PREFIX: u32 = 12;
/// 6rd prefix length.
pub const IFLA_IPTUN_6RD_PREFIXLEN: u32 = 13;
/// 6rd relay prefix length.
pub const IFLA_IPTUN_6RD_RELAY_PREFIXLEN: u32 = 14;
/// Encapsulation type.
pub const IFLA_IPTUN_ENCAP_TYPE: u32 = 15;
/// Encapsulation flags.
pub const IFLA_IPTUN_ENCAP_FLAGS: u32 = 16;
/// Encapsulation source port.
pub const IFLA_IPTUN_ENCAP_SPORT: u32 = 17;
/// Encapsulation dest port.
pub const IFLA_IPTUN_ENCAP_DPORT: u32 = 18;
/// Collect metadata.
pub const IFLA_IPTUN_COLLECT_METADATA: u32 = 19;
/// FW mark.
pub const IFLA_IPTUN_FWMARK: u32 = 20;

// ---------------------------------------------------------------------------
// Tunnel encapsulation types
// ---------------------------------------------------------------------------

/// No encapsulation.
pub const TUNNEL_ENCAP_NONE: u32 = 0;
/// FOU (Foo-over-UDP).
pub const TUNNEL_ENCAP_FOU: u32 = 1;
/// GUE (Generic UDP Encapsulation).
pub const TUNNEL_ENCAP_GUE: u32 = 2;
/// MPLS.
pub const TUNNEL_ENCAP_MPLS: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            TUNNEL_CSUM, TUNNEL_ROUTING, TUNNEL_KEY, TUNNEL_SEQ,
            TUNNEL_STRICT, TUNNEL_REC, TUNNEL_VERSION,
            TUNNEL_NO_KEY, TUNNEL_DONT_FRAGMENT, TUNNEL_OAM,
            TUNNEL_CRIT_OPT, TUNNEL_GENEVE_OPT, TUNNEL_VXLAN_OPT,
            TUNNEL_NOCACHE, TUNNEL_ERSPAN_OPT, TUNNEL_GTP_OPT,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "flag {f:#x} not power of two");
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            TUNNEL_CSUM, TUNNEL_ROUTING, TUNNEL_KEY, TUNNEL_SEQ,
            TUNNEL_STRICT, TUNNEL_REC, TUNNEL_VERSION,
            TUNNEL_NO_KEY, TUNNEL_DONT_FRAGMENT, TUNNEL_OAM,
            TUNNEL_CRIT_OPT, TUNNEL_GENEVE_OPT, TUNNEL_VXLAN_OPT,
            TUNNEL_NOCACHE, TUNNEL_ERSPAN_OPT, TUNNEL_GTP_OPT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let cmds = [SIOCADDTUNNEL, SIOCDELTUNNEL, SIOCCHGTUNNEL, SIOCGETTUNNEL];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            IFLA_IPTUN_UNSPEC, IFLA_IPTUN_LINK, IFLA_IPTUN_LOCAL,
            IFLA_IPTUN_REMOTE, IFLA_IPTUN_TTL, IFLA_IPTUN_TOS,
            IFLA_IPTUN_ENCAP_LIMIT, IFLA_IPTUN_FLOWINFO,
            IFLA_IPTUN_FLAGS, IFLA_IPTUN_PROTO,
            IFLA_IPTUN_PMTUDISC, IFLA_IPTUN_6RD_PREFIX,
            IFLA_IPTUN_6RD_RELAY_PREFIX, IFLA_IPTUN_6RD_PREFIXLEN,
            IFLA_IPTUN_6RD_RELAY_PREFIXLEN,
            IFLA_IPTUN_ENCAP_TYPE, IFLA_IPTUN_ENCAP_FLAGS,
            IFLA_IPTUN_ENCAP_SPORT, IFLA_IPTUN_ENCAP_DPORT,
            IFLA_IPTUN_COLLECT_METADATA, IFLA_IPTUN_FWMARK,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_encap_types_distinct() {
        let types = [
            TUNNEL_ENCAP_NONE, TUNNEL_ENCAP_FOU,
            TUNNEL_ENCAP_GUE, TUNNEL_ENCAP_MPLS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_encap_none_is_zero() {
        assert_eq!(TUNNEL_ENCAP_NONE, 0);
    }
}

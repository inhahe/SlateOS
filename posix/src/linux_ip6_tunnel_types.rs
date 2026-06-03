//! `<linux/ip6_tunnel.h>` — IPv6 tunnel constants.
//!
//! IPv6 tunnels encapsulate traffic within IPv6.  These
//! constants define tunnel flags, netlink attributes,
//! and tunnel modes for ip6tnl and ip6gre.

// ---------------------------------------------------------------------------
// IPv6 tunnel flags (IP6_TNL_F_*)
// ---------------------------------------------------------------------------

/// Ignore encapsulation limit.
pub const IP6_TNL_F_IGN_ENCAP_LIMIT: u32 = 0x1;
/// Use original TOS.
pub const IP6_TNL_F_USE_ORIG_TCLASS: u32 = 0x2;
/// Use original flow label.
pub const IP6_TNL_F_USE_ORIG_FLOWLABEL: u32 = 0x4;
/// MIP6 Mobility Header.
pub const IP6_TNL_F_MIP6_DEV: u32 = 0x8;
/// RCV DSCP copy.
pub const IP6_TNL_F_RCV_DSCP_COPY: u32 = 0x10;
/// Use original FW mark.
pub const IP6_TNL_F_USE_ORIG_FWMARK: u32 = 0x20;
/// Allow local remote.
pub const IP6_TNL_F_ALLOW_LOCAL_REMOTE: u32 = 0x40;

// ---------------------------------------------------------------------------
// IPv6 tunnel netlink attributes (IFLA_IP6TNL_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_IP6TNL_UNSPEC: u32 = 0;
/// Link (parent interface).
pub const IFLA_IP6TNL_LINK: u32 = 1;
/// Local address.
pub const IFLA_IP6TNL_LOCAL: u32 = 2;
/// Remote address.
pub const IFLA_IP6TNL_REMOTE: u32 = 3;
/// TTL (hop limit).
pub const IFLA_IP6TNL_TTL: u32 = 4;
/// Encapsulation limit.
pub const IFLA_IP6TNL_ENCAP_LIMIT: u32 = 5;
/// Flow info.
pub const IFLA_IP6TNL_FLOWINFO: u32 = 6;
/// Flags.
pub const IFLA_IP6TNL_FLAGS: u32 = 7;
/// Protocol.
pub const IFLA_IP6TNL_PROTO: u32 = 8;
/// Encapsulation type.
pub const IFLA_IP6TNL_ENCAP_TYPE: u32 = 9;
/// Encapsulation flags.
pub const IFLA_IP6TNL_ENCAP_FLAGS: u32 = 10;
/// Encapsulation source port.
pub const IFLA_IP6TNL_ENCAP_SPORT: u32 = 11;
/// Encapsulation dest port.
pub const IFLA_IP6TNL_ENCAP_DPORT: u32 = 12;
/// Collect metadata.
pub const IFLA_IP6TNL_COLLECT_METADATA: u32 = 13;
/// FW mark.
pub const IFLA_IP6TNL_FWMARK: u32 = 14;

// ---------------------------------------------------------------------------
// IPv6 GRE netlink attributes (IFLA_IP6GRE_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const IFLA_IP6GRE_UNSPEC: u32 = 0;
/// Link.
pub const IFLA_IP6GRE_LINK: u32 = 1;
/// Input flags.
pub const IFLA_IP6GRE_IFLAGS: u32 = 2;
/// Output flags.
pub const IFLA_IP6GRE_OFLAGS: u32 = 3;
/// Input key.
pub const IFLA_IP6GRE_IKEY: u32 = 4;
/// Output key.
pub const IFLA_IP6GRE_OKEY: u32 = 5;
/// Local address.
pub const IFLA_IP6GRE_LOCAL: u32 = 6;
/// Remote address.
pub const IFLA_IP6GRE_REMOTE: u32 = 7;
/// TTL.
pub const IFLA_IP6GRE_TTL: u32 = 8;
/// Encapsulation limit.
pub const IFLA_IP6GRE_ENCAP_LIMIT: u32 = 9;
/// Flow info.
pub const IFLA_IP6GRE_FLOWINFO: u32 = 10;
/// Flags.
pub const IFLA_IP6GRE_FLAGS: u32 = 11;
/// Encap type.
pub const IFLA_IP6GRE_ENCAP_TYPE: u32 = 12;
/// Encap flags.
pub const IFLA_IP6GRE_ENCAP_FLAGS: u32 = 13;
/// Encap source port.
pub const IFLA_IP6GRE_ENCAP_SPORT: u32 = 14;
/// Encap dest port.
pub const IFLA_IP6GRE_ENCAP_DPORT: u32 = 15;
/// Collect metadata.
pub const IFLA_IP6GRE_COLLECT_METADATA: u32 = 16;
/// FW mark.
pub const IFLA_IP6GRE_FWMARK: u32 = 17;
/// ERSPAN version.
pub const IFLA_IP6GRE_ERSPAN_VER: u32 = 18;
/// ERSPAN index.
pub const IFLA_IP6GRE_ERSPAN_INDEX: u32 = 19;
/// ERSPAN direction.
pub const IFLA_IP6GRE_ERSPAN_DIR: u32 = 20;
/// ERSPAN HW ID.
pub const IFLA_IP6GRE_ERSPAN_HWID: u32 = 21;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunnel_flags_no_overlap() {
        let flags = [
            IP6_TNL_F_IGN_ENCAP_LIMIT,
            IP6_TNL_F_USE_ORIG_TCLASS,
            IP6_TNL_F_USE_ORIG_FLOWLABEL,
            IP6_TNL_F_MIP6_DEV,
            IP6_TNL_F_RCV_DSCP_COPY,
            IP6_TNL_F_USE_ORIG_FWMARK,
            IP6_TNL_F_ALLOW_LOCAL_REMOTE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ip6tnl_attrs_distinct() {
        let attrs = [
            IFLA_IP6TNL_UNSPEC,
            IFLA_IP6TNL_LINK,
            IFLA_IP6TNL_LOCAL,
            IFLA_IP6TNL_REMOTE,
            IFLA_IP6TNL_TTL,
            IFLA_IP6TNL_ENCAP_LIMIT,
            IFLA_IP6TNL_FLOWINFO,
            IFLA_IP6TNL_FLAGS,
            IFLA_IP6TNL_PROTO,
            IFLA_IP6TNL_ENCAP_TYPE,
            IFLA_IP6TNL_ENCAP_FLAGS,
            IFLA_IP6TNL_ENCAP_SPORT,
            IFLA_IP6TNL_ENCAP_DPORT,
            IFLA_IP6TNL_COLLECT_METADATA,
            IFLA_IP6TNL_FWMARK,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_ip6gre_attrs_distinct() {
        let attrs = [
            IFLA_IP6GRE_UNSPEC,
            IFLA_IP6GRE_LINK,
            IFLA_IP6GRE_IFLAGS,
            IFLA_IP6GRE_OFLAGS,
            IFLA_IP6GRE_IKEY,
            IFLA_IP6GRE_OKEY,
            IFLA_IP6GRE_LOCAL,
            IFLA_IP6GRE_REMOTE,
            IFLA_IP6GRE_TTL,
            IFLA_IP6GRE_ENCAP_LIMIT,
            IFLA_IP6GRE_FLOWINFO,
            IFLA_IP6GRE_FLAGS,
            IFLA_IP6GRE_ENCAP_TYPE,
            IFLA_IP6GRE_ENCAP_FLAGS,
            IFLA_IP6GRE_ENCAP_SPORT,
            IFLA_IP6GRE_ENCAP_DPORT,
            IFLA_IP6GRE_COLLECT_METADATA,
            IFLA_IP6GRE_FWMARK,
            IFLA_IP6GRE_ERSPAN_VER,
            IFLA_IP6GRE_ERSPAN_INDEX,
            IFLA_IP6GRE_ERSPAN_DIR,
            IFLA_IP6GRE_ERSPAN_HWID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(IFLA_IP6TNL_UNSPEC, 0);
        assert_eq!(IFLA_IP6GRE_UNSPEC, 0);
    }

    #[test]
    fn test_ign_encap_limit() {
        assert_eq!(IP6_TNL_F_IGN_ENCAP_LIMIT, 0x1);
    }
}

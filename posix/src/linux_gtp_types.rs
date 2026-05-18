//! `<linux/gtp.h>` — GTP (GPRS Tunneling Protocol) constants.
//!
//! GTP is used in mobile networks (3G/4G/5G) for tunneling
//! user data between network nodes.  These constants define
//! GTP netlink commands, attribute types, and protocol versions.

// ---------------------------------------------------------------------------
// GTP netlink commands (GTP_CMD_*)
// ---------------------------------------------------------------------------

/// Create a new PDP context (tunnel).
pub const GTP_CMD_NEWPDP: u32 = 0;
/// Delete a PDP context.
pub const GTP_CMD_DELPDP: u32 = 1;
/// Get a PDP context.
pub const GTP_CMD_GETPDP: u32 = 2;
/// Echo request.
pub const GTP_CMD_ECHOREQ: u32 = 3;

// ---------------------------------------------------------------------------
// GTP attribute types (GTPA_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const GTPA_UNSPEC: u32 = 0;
/// Link (interface index).
pub const GTPA_LINK: u32 = 1;
/// GTP version.
pub const GTPA_VERSION: u32 = 2;
/// Tunnel endpoint ID (data).
pub const GTPA_TID: u32 = 3;
/// Peer address (IPv4).
pub const GTPA_PEER_ADDRESS: u32 = 4;
/// MS (mobile station) address (IPv4).
pub const GTPA_MS_ADDRESS: u32 = 5;
/// Flow ID.
pub const GTPA_FLOW: u32 = 6;
/// Net namespace file descriptor.
pub const GTPA_NET_NS_FD: u32 = 7;
/// TEID for ingress.
pub const GTPA_I_TEI: u32 = 8;
/// TEID for egress.
pub const GTPA_O_TEI: u32 = 9;
/// Peer address (IPv6).
pub const GTPA_PEER_ADDR6: u32 = 10;
/// MS address (IPv6).
pub const GTPA_MS_ADDR6: u32 = 11;
/// Family (AF_INET or AF_INET6).
pub const GTPA_FAMILY: u32 = 14;

// ---------------------------------------------------------------------------
// GTP versions
// ---------------------------------------------------------------------------

/// GTPv0 (GSM/GPRS).
pub const GTP_V0: u32 = 0;
/// GTPv1 (UMTS/3G+).
pub const GTP_V1: u32 = 1;

// ---------------------------------------------------------------------------
// GTP port numbers
// ---------------------------------------------------------------------------

/// GTP-C (control) port for GTPv0.
pub const GTP0_PORT: u16 = 3386;
/// GTP-U (user data) port for GTPv1.
pub const GTP1U_PORT: u16 = 2152;

// ---------------------------------------------------------------------------
// GTP header flags
// ---------------------------------------------------------------------------

/// Protocol type (1 = GTP, 0 = GTP').
pub const GTP_FLAG_PT: u8 = 1 << 4;
/// Extension header present.
pub const GTP_FLAG_E: u8 = 1 << 2;
/// Sequence number present.
pub const GTP_FLAG_S: u8 = 1 << 1;
/// N-PDU number present.
pub const GTP_FLAG_PN: u8 = 1 << 0;

// ---------------------------------------------------------------------------
// GTP message types
// ---------------------------------------------------------------------------

/// Echo request.
pub const GTP_MSG_ECHO_REQ: u8 = 1;
/// Echo response.
pub const GTP_MSG_ECHO_RESP: u8 = 2;
/// Error indication.
pub const GTP_MSG_ERROR_IND: u8 = 26;
/// End marker.
pub const GTP_MSG_END_MARKER: u8 = 254;
/// T-PDU (tunneled user data).
pub const GTP_MSG_TPDU: u8 = 255;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [GTP_CMD_NEWPDP, GTP_CMD_DELPDP, GTP_CMD_GETPDP, GTP_CMD_ECHOREQ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            GTPA_UNSPEC, GTPA_LINK, GTPA_VERSION, GTPA_TID,
            GTPA_PEER_ADDRESS, GTPA_MS_ADDRESS, GTPA_FLOW,
            GTPA_NET_NS_FD, GTPA_I_TEI, GTPA_O_TEI,
            GTPA_PEER_ADDR6, GTPA_MS_ADDR6, GTPA_FAMILY,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_versions_distinct() {
        assert_ne!(GTP_V0, GTP_V1);
    }

    #[test]
    fn test_ports_distinct() {
        assert_ne!(GTP0_PORT, GTP1U_PORT);
    }

    #[test]
    fn test_gtp1u_port() {
        assert_eq!(GTP1U_PORT, 2152);
    }

    #[test]
    fn test_header_flags_no_overlap() {
        let flags = [GTP_FLAG_PT, GTP_FLAG_E, GTP_FLAG_S, GTP_FLAG_PN];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [
            GTP_MSG_ECHO_REQ, GTP_MSG_ECHO_RESP,
            GTP_MSG_ERROR_IND, GTP_MSG_END_MARKER, GTP_MSG_TPDU,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(GTPA_UNSPEC, 0);
    }

    #[test]
    fn test_v0_is_zero() {
        assert_eq!(GTP_V0, 0);
    }
}

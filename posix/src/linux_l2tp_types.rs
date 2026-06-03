//! `<linux/l2tp.h>` — L2TP (Layer 2 Tunneling Protocol) constants.
//!
//! L2TP creates tunnels for carrying PPP or Ethernet frames.
//! These constants define L2TP netlink commands, attribute
//! types, encapsulation types, and session parameters.

// ---------------------------------------------------------------------------
// L2TP netlink commands (L2TP_CMD_*)
// ---------------------------------------------------------------------------

/// Noop.
pub const L2TP_CMD_NOOP: u32 = 0;
/// Create tunnel.
pub const L2TP_CMD_TUNNEL_CREATE: u32 = 1;
/// Delete tunnel.
pub const L2TP_CMD_TUNNEL_DELETE: u32 = 2;
/// Modify tunnel.
pub const L2TP_CMD_TUNNEL_MODIFY: u32 = 3;
/// Get tunnel.
pub const L2TP_CMD_TUNNEL_GET: u32 = 4;
/// Create session.
pub const L2TP_CMD_SESSION_CREATE: u32 = 5;
/// Delete session.
pub const L2TP_CMD_SESSION_DELETE: u32 = 6;
/// Modify session.
pub const L2TP_CMD_SESSION_MODIFY: u32 = 7;
/// Get session.
pub const L2TP_CMD_SESSION_GET: u32 = 8;

// ---------------------------------------------------------------------------
// L2TP attribute types (L2TP_ATTR_*)
// ---------------------------------------------------------------------------

/// No attribute.
pub const L2TP_ATTR_NONE: u32 = 0;
/// Protocol version.
pub const L2TP_ATTR_PW_TYPE: u32 = 1;
/// Encapsulation type.
pub const L2TP_ATTR_ENCAP_TYPE: u32 = 2;
/// Offset.
pub const L2TP_ATTR_OFFSET: u32 = 3;
/// Data seq.
pub const L2TP_ATTR_DATA_SEQ: u32 = 4;
/// L2-specific type.
pub const L2TP_ATTR_L2SPEC_TYPE: u32 = 5;
/// L2-specific length.
pub const L2TP_ATTR_L2SPEC_LEN: u32 = 6;
/// Protocol version.
pub const L2TP_ATTR_PROTO_VERSION: u32 = 7;
/// Interface name.
pub const L2TP_ATTR_IFNAME: u32 = 8;
/// Connection ID.
pub const L2TP_ATTR_CONN_ID: u32 = 9;
/// Peer connection ID.
pub const L2TP_ATTR_PEER_CONN_ID: u32 = 10;
/// Session ID.
pub const L2TP_ATTR_SESSION_ID: u32 = 11;
/// Peer session ID.
pub const L2TP_ATTR_PEER_SESSION_ID: u32 = 12;
/// UDP source port.
pub const L2TP_ATTR_UDP_SPORT: u32 = 13;
/// UDP destination port.
pub const L2TP_ATTR_UDP_DPORT: u32 = 14;
/// Cookie.
pub const L2TP_ATTR_COOKIE: u32 = 15;
/// Peer cookie.
pub const L2TP_ATTR_PEER_COOKIE: u32 = 16;
/// Debug.
pub const L2TP_ATTR_DEBUG: u32 = 17;
/// Receive sequence.
pub const L2TP_ATTR_RECV_SEQ: u32 = 18;
/// Send sequence.
pub const L2TP_ATTR_SEND_SEQ: u32 = 19;
/// LNS mode.
pub const L2TP_ATTR_LNS_MODE: u32 = 20;
/// Using IPSEC.
pub const L2TP_ATTR_USING_IPSEC: u32 = 21;
/// FD (file descriptor for tunnel socket).
pub const L2TP_ATTR_FD: u32 = 23;
/// IPv4 source address.
pub const L2TP_ATTR_IP_SADDR: u32 = 24;
/// IPv4 destination address.
pub const L2TP_ATTR_IP_DADDR: u32 = 25;
/// UDP checksum.
pub const L2TP_ATTR_UDP_CSUM: u32 = 26;
/// VLAN ID.
pub const L2TP_ATTR_VLAN_ID: u32 = 27;

// ---------------------------------------------------------------------------
// L2TP encapsulation types
// ---------------------------------------------------------------------------

/// UDP encapsulation.
pub const L2TP_ENCAPTYPE_UDP: u32 = 0;
/// IP encapsulation.
pub const L2TP_ENCAPTYPE_IP: u32 = 1;

// ---------------------------------------------------------------------------
// L2TP pseudowire types
// ---------------------------------------------------------------------------

/// PPP pseudowire.
pub const L2TP_PWTYPE_PPP: u32 = 0x0007;
/// Ethernet pseudowire.
pub const L2TP_PWTYPE_ETH: u32 = 0x0005;
/// PPP-AC pseudowire.
pub const L2TP_PWTYPE_PPP_AC: u32 = 0x0001;
/// IP pseudowire.
pub const L2TP_PWTYPE_IP: u32 = 0x000B;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            L2TP_CMD_NOOP,
            L2TP_CMD_TUNNEL_CREATE,
            L2TP_CMD_TUNNEL_DELETE,
            L2TP_CMD_TUNNEL_MODIFY,
            L2TP_CMD_TUNNEL_GET,
            L2TP_CMD_SESSION_CREATE,
            L2TP_CMD_SESSION_DELETE,
            L2TP_CMD_SESSION_MODIFY,
            L2TP_CMD_SESSION_GET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            L2TP_ATTR_NONE,
            L2TP_ATTR_PW_TYPE,
            L2TP_ATTR_ENCAP_TYPE,
            L2TP_ATTR_OFFSET,
            L2TP_ATTR_DATA_SEQ,
            L2TP_ATTR_L2SPEC_TYPE,
            L2TP_ATTR_L2SPEC_LEN,
            L2TP_ATTR_PROTO_VERSION,
            L2TP_ATTR_IFNAME,
            L2TP_ATTR_CONN_ID,
            L2TP_ATTR_PEER_CONN_ID,
            L2TP_ATTR_SESSION_ID,
            L2TP_ATTR_PEER_SESSION_ID,
            L2TP_ATTR_UDP_SPORT,
            L2TP_ATTR_UDP_DPORT,
            L2TP_ATTR_COOKIE,
            L2TP_ATTR_PEER_COOKIE,
            L2TP_ATTR_DEBUG,
            L2TP_ATTR_RECV_SEQ,
            L2TP_ATTR_SEND_SEQ,
            L2TP_ATTR_LNS_MODE,
            L2TP_ATTR_USING_IPSEC,
            L2TP_ATTR_FD,
            L2TP_ATTR_IP_SADDR,
            L2TP_ATTR_IP_DADDR,
            L2TP_ATTR_UDP_CSUM,
            L2TP_ATTR_VLAN_ID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_encap_types_distinct() {
        assert_ne!(L2TP_ENCAPTYPE_UDP, L2TP_ENCAPTYPE_IP);
    }

    #[test]
    fn test_pw_types_distinct() {
        let pws = [
            L2TP_PWTYPE_PPP,
            L2TP_PWTYPE_ETH,
            L2TP_PWTYPE_PPP_AC,
            L2TP_PWTYPE_IP,
        ];
        for i in 0..pws.len() {
            for j in (i + 1)..pws.len() {
                assert_ne!(pws[i], pws[j]);
            }
        }
    }

    #[test]
    fn test_noop_is_zero() {
        assert_eq!(L2TP_CMD_NOOP, 0);
    }

    #[test]
    fn test_none_is_zero() {
        assert_eq!(L2TP_ATTR_NONE, 0);
    }

    #[test]
    fn test_udp_encap_is_zero() {
        assert_eq!(L2TP_ENCAPTYPE_UDP, 0);
    }
}

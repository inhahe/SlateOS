//! `<linux/l2tp.h>` — Additional L2TP constants.
//!
//! Supplementary L2TP tunneling constants covering command types,
//! attribute types, and session parameters.

// ---------------------------------------------------------------------------
// L2TP generic netlink commands
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
// L2TP attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const L2TP_ATTR_NONE: u32 = 0;
/// Protocol version.
pub const L2TP_ATTR_PW_TYPE: u32 = 1;
/// Encapsulation type.
pub const L2TP_ATTR_ENCAP_TYPE: u32 = 2;
/// Offset.
pub const L2TP_ATTR_OFFSET: u32 = 3;
/// Data sequence numbers.
pub const L2TP_ATTR_DATA_SEQ: u32 = 4;
/// L2 specific type.
pub const L2TP_ATTR_L2SPEC_TYPE: u32 = 5;
/// L2 specific length.
pub const L2TP_ATTR_L2SPEC_LEN: u32 = 6;
/// Protocol.
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

// ---------------------------------------------------------------------------
// L2TP encapsulation types
// ---------------------------------------------------------------------------

/// UDP encapsulation.
pub const L2TP_ENCAPTYPE_UDP: u32 = 0;
/// IP encapsulation.
pub const L2TP_ENCAPTYPE_IP: u32 = 1;

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
}

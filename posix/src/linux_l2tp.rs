//! `<linux/l2tp.h>` — Layer 2 Tunneling Protocol constants.
//!
//! L2TP (RFC 2661, RFC 3931) tunnels PPP and Ethernet frames over
//! UDP or IP. It is widely used for VPNs and ISP access networks.
//! L2TPv2 carries PPP; L2TPv3 is protocol-agnostic and can carry
//! Ethernet, ATM, Frame Relay, and other L2 protocols.

// ---------------------------------------------------------------------------
// L2TP versions
// ---------------------------------------------------------------------------

/// L2TPv2 (RFC 2661, PPP tunneling).
pub const L2TP_VERSION_2: u8 = 2;
/// L2TPv3 (RFC 3931, generic L2 tunneling).
pub const L2TP_VERSION_3: u8 = 3;

// ---------------------------------------------------------------------------
// Encapsulation types
// ---------------------------------------------------------------------------

/// UDP encapsulation.
pub const L2TP_ENCAPTYPE_UDP: u8 = 0;
/// IP encapsulation (protocol 115).
pub const L2TP_ENCAPTYPE_IP: u8 = 1;

// ---------------------------------------------------------------------------
// L2TP message types
// ---------------------------------------------------------------------------

/// Data message.
pub const L2TP_MSG_DATA: u16 = 0;
/// Control message.
pub const L2TP_MSG_CONTROL: u16 = 1;

// ---------------------------------------------------------------------------
// Pseudowire types (L2TPv3)
// ---------------------------------------------------------------------------

/// PPP pseudowire.
pub const L2TP_PWTYPE_PPP: u16 = 0x0007;
/// Ethernet pseudowire (port mode).
pub const L2TP_PWTYPE_ETH_VLAN: u16 = 0x0004;
/// Ethernet pseudowire.
pub const L2TP_PWTYPE_ETH: u16 = 0x0005;
/// PPP/AC pseudowire.
pub const L2TP_PWTYPE_PPP_AC: u16 = 0x0001;
/// IP pseudowire.
pub const L2TP_PWTYPE_IP: u16 = 0x000B;

// ---------------------------------------------------------------------------
// Socket option levels
// ---------------------------------------------------------------------------

/// L2TP protocol number (for setsockopt).
pub const SOL_L2TP: u32 = 115;

// ---------------------------------------------------------------------------
// Netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const L2TP_ATTR_NONE: u16 = 0;
/// Protocol version.
pub const L2TP_ATTR_PW_TYPE: u16 = 1;
/// Encapsulation type.
pub const L2TP_ATTR_ENCAP_TYPE: u16 = 2;
/// Tunnel ID.
pub const L2TP_ATTR_CONN_ID: u16 = 3;
/// Peer tunnel ID.
pub const L2TP_ATTR_PEER_CONN_ID: u16 = 4;
/// Session ID.
pub const L2TP_ATTR_SESSION_ID: u16 = 5;
/// Peer session ID.
pub const L2TP_ATTR_PEER_SESSION_ID: u16 = 6;
/// Debug flags.
pub const L2TP_ATTR_DEBUG: u16 = 7;

// ---------------------------------------------------------------------------
// Netlink commands
// ---------------------------------------------------------------------------

/// Create tunnel.
pub const L2TP_CMD_TUNNEL_CREATE: u8 = 1;
/// Delete tunnel.
pub const L2TP_CMD_TUNNEL_DELETE: u8 = 2;
/// Modify tunnel.
pub const L2TP_CMD_TUNNEL_MODIFY: u8 = 3;
/// Get tunnel.
pub const L2TP_CMD_TUNNEL_GET: u8 = 4;
/// Create session.
pub const L2TP_CMD_SESSION_CREATE: u8 = 5;
/// Delete session.
pub const L2TP_CMD_SESSION_DELETE: u8 = 6;
/// Modify session.
pub const L2TP_CMD_SESSION_MODIFY: u8 = 7;
/// Get session.
pub const L2TP_CMD_SESSION_GET: u8 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versions_distinct() {
        assert_ne!(L2TP_VERSION_2, L2TP_VERSION_3);
    }

    #[test]
    fn test_encap_types_distinct() {
        assert_ne!(L2TP_ENCAPTYPE_UDP, L2TP_ENCAPTYPE_IP);
    }

    #[test]
    fn test_msg_types_distinct() {
        assert_ne!(L2TP_MSG_DATA, L2TP_MSG_CONTROL);
    }

    #[test]
    fn test_pw_types_distinct() {
        let types = [
            L2TP_PWTYPE_PPP,
            L2TP_PWTYPE_ETH_VLAN,
            L2TP_PWTYPE_ETH,
            L2TP_PWTYPE_PPP_AC,
            L2TP_PWTYPE_IP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            L2TP_ATTR_NONE,
            L2TP_ATTR_PW_TYPE,
            L2TP_ATTR_ENCAP_TYPE,
            L2TP_ATTR_CONN_ID,
            L2TP_ATTR_PEER_CONN_ID,
            L2TP_ATTR_SESSION_ID,
            L2TP_ATTR_PEER_SESSION_ID,
            L2TP_ATTR_DEBUG,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
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
}

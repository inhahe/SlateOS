//! `<linux/l2tp.h>` — Additional L2TP constants (batch 3).
//!
//! Supplementary L2TP constants covering session types,
//! encapsulation modes, and debug flags.

// ---------------------------------------------------------------------------
// L2TP tunnel encapsulation types
// ---------------------------------------------------------------------------

/// UDP encapsulation.
pub const L2TP_ENCAPTYPE_UDP: u32 = 0;
/// IP encapsulation.
pub const L2TP_ENCAPTYPE_IP: u32 = 1;

// ---------------------------------------------------------------------------
// L2TP protocol versions
// ---------------------------------------------------------------------------

/// L2TP version 2.
pub const L2TP_VERSION_2: u32 = 2;
/// L2TP version 3.
pub const L2TP_VERSION_3: u32 = 3;

// ---------------------------------------------------------------------------
// L2TP pseudo-wire types
// ---------------------------------------------------------------------------

/// PPP pseudo-wire.
pub const L2TP_PWTYPE_PPP: u32 = 0x0007;
/// PPP AC pseudo-wire.
pub const L2TP_PWTYPE_PPP_AC: u32 = 0x0001;
/// Ethernet pseudo-wire.
pub const L2TP_PWTYPE_ETH: u32 = 0x0005;
/// Ethernet VLAN pseudo-wire.
pub const L2TP_PWTYPE_ETH_VLAN: u32 = 0x0004;
/// IP pseudo-wire.
pub const L2TP_PWTYPE_IP: u32 = 0x000B;

// ---------------------------------------------------------------------------
// L2TP genetlink commands
// ---------------------------------------------------------------------------

/// No operation.
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
// L2TP debug flags
// ---------------------------------------------------------------------------

/// Debug control messages.
pub const L2TP_MSG_CONTROL: u32 = 1 << 0;
/// Debug sequence numbers.
pub const L2TP_MSG_SEQ: u32 = 1 << 1;
/// Debug data messages.
pub const L2TP_MSG_DATA: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encap_types_distinct() {
        assert_ne!(L2TP_ENCAPTYPE_UDP, L2TP_ENCAPTYPE_IP);
    }

    #[test]
    fn test_versions_distinct() {
        assert_ne!(L2TP_VERSION_2, L2TP_VERSION_3);
    }

    #[test]
    fn test_pw_types_distinct() {
        let types = [
            L2TP_PWTYPE_PPP,
            L2TP_PWTYPE_PPP_AC,
            L2TP_PWTYPE_ETH,
            L2TP_PWTYPE_ETH_VLAN,
            L2TP_PWTYPE_IP,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_commands_distinct() {
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
    fn test_debug_flags_power_of_two() {
        assert!(L2TP_MSG_CONTROL.is_power_of_two());
        assert!(L2TP_MSG_SEQ.is_power_of_two());
        assert!(L2TP_MSG_DATA.is_power_of_two());
    }

    #[test]
    fn test_debug_flags_no_overlap() {
        let flags = [L2TP_MSG_CONTROL, L2TP_MSG_SEQ, L2TP_MSG_DATA];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

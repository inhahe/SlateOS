//! `<linux/l2tp.h>` — L2TPv3 socket and netlink ABI.
//!
//! `xl2tpd`, `accel-ppp`, and the kernel's L2TPv3 stack use these
//! constants to set up Layer 2 Tunneling Protocol tunnels over UDP
//! or IP. The socket-level constants (SOL_PPPOL2TP, …) and the netlink
//! command enums match `include/uapi/linux/l2tp.h`.

// ---------------------------------------------------------------------------
// Protocol families and IPPROTO
// ---------------------------------------------------------------------------

/// `IPPROTO_L2TP` (RFC 3931).
pub const IPPROTO_L2TP: u32 = 115;
/// `SOL_PPPOL2TP` setsockopt level.
pub const SOL_PPPOL2TP: u32 = 273;
/// Genetlink family name used by `xl2tpd`.
pub const L2TP_GENL_NAME: &str = "l2tp";
pub const L2TP_GENL_VERSION: u32 = 0x1;
/// Multicast group for tunnel/session notifications.
pub const L2TP_GENL_MCGROUP: &str = "l2tp";

// ---------------------------------------------------------------------------
// Genetlink commands (`enum l2tp_nl_commands`)
// ---------------------------------------------------------------------------

pub const L2TP_CMD_NOOP: u32 = 0;
pub const L2TP_CMD_TUNNEL_CREATE: u32 = 1;
pub const L2TP_CMD_TUNNEL_DELETE: u32 = 2;
pub const L2TP_CMD_TUNNEL_MODIFY: u32 = 3;
pub const L2TP_CMD_TUNNEL_GET: u32 = 4;
pub const L2TP_CMD_SESSION_CREATE: u32 = 5;
pub const L2TP_CMD_SESSION_DELETE: u32 = 6;
pub const L2TP_CMD_SESSION_MODIFY: u32 = 7;
pub const L2TP_CMD_SESSION_GET: u32 = 8;

// ---------------------------------------------------------------------------
// Encapsulation types
// ---------------------------------------------------------------------------

pub const L2TP_ENCAPTYPE_UDP: u32 = 0;
pub const L2TP_ENCAPTYPE_IP: u32 = 1;

// ---------------------------------------------------------------------------
// Pseudo-wire types (`enum l2tp_pwtype`)
// ---------------------------------------------------------------------------

pub const L2TP_PWTYPE_NONE: u32 = 0x0000;
pub const L2TP_PWTYPE_ETH_VLAN: u32 = 0x0004;
pub const L2TP_PWTYPE_ETH: u32 = 0x0005;
pub const L2TP_PWTYPE_PPP: u32 = 0x0007;
pub const L2TP_PWTYPE_PPP_AC: u32 = 0x0008;
pub const L2TP_PWTYPE_IP: u32 = 0x000B;

// ---------------------------------------------------------------------------
// L2TPv3 session-cookie sizes
// ---------------------------------------------------------------------------

pub const L2TP_COOKIE_LEN_4: usize = 4;
pub const L2TP_COOKIE_LEN_8: usize = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_numbers() {
        // IANA-assigned IPPROTO_L2TP = 115.
        assert_eq!(IPPROTO_L2TP, 115);
        // SOL_PPPOL2TP = 273 in <linux/socket.h>.
        assert_eq!(SOL_PPPOL2TP, 273);
    }

    #[test]
    fn test_genl_identity() {
        assert_eq!(L2TP_GENL_NAME, "l2tp");
        assert_eq!(L2TP_GENL_MCGROUP, "l2tp");
        assert_eq!(L2TP_GENL_VERSION, 1);
    }

    #[test]
    fn test_commands_dense_0_to_8() {
        let c = [
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
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_encap_types_distinct() {
        assert_eq!(L2TP_ENCAPTYPE_UDP, 0);
        assert_eq!(L2TP_ENCAPTYPE_IP, 1);
        assert_ne!(L2TP_ENCAPTYPE_UDP, L2TP_ENCAPTYPE_IP);
    }

    #[test]
    fn test_pwtype_values_match_iana() {
        // Values follow IANA L2TP Pseudowire Type Codes registry.
        assert_eq!(L2TP_PWTYPE_NONE, 0x0000);
        assert_eq!(L2TP_PWTYPE_ETH_VLAN, 4);
        assert_eq!(L2TP_PWTYPE_ETH, 5);
        assert_eq!(L2TP_PWTYPE_PPP, 7);
        assert_eq!(L2TP_PWTYPE_IP, 11);
        // ETH_VLAN < ETH < PPP < PPP_AC < IP monotonically.
        assert!(L2TP_PWTYPE_ETH_VLAN < L2TP_PWTYPE_ETH);
        assert!(L2TP_PWTYPE_ETH < L2TP_PWTYPE_PPP);
        assert!(L2TP_PWTYPE_PPP < L2TP_PWTYPE_PPP_AC);
        assert!(L2TP_PWTYPE_PPP_AC < L2TP_PWTYPE_IP);
    }

    #[test]
    fn test_cookie_lens() {
        // The two legal cookie sizes per RFC 3931 §3.3.
        assert_eq!(L2TP_COOKIE_LEN_4, 4);
        assert_eq!(L2TP_COOKIE_LEN_8, 8);
    }
}

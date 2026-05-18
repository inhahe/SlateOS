//! `<linux/xfrm.h>` — IPsec/XFRM transform and policy constants.
//!
//! XFRM (transform) is the kernel framework for IPsec. It manages
//! security associations (SA), security policies (SP), and the
//! protocol-specific transforms (ESP, AH, IPCOMP) that implement
//! packet encryption and authentication.

// ---------------------------------------------------------------------------
// XFRM protocols
// ---------------------------------------------------------------------------

/// Encapsulating Security Payload.
pub const IPPROTO_ESP: u8 = 50;
/// Authentication Header.
pub const IPPROTO_AH: u8 = 51;
/// IP Compression.
pub const IPPROTO_COMP: u8 = 108;
/// IPsec routing (internal).
pub const IPPROTO_ROUTING: u8 = 43;
/// Destination options.
pub const IPPROTO_DSTOPTS: u8 = 60;

// ---------------------------------------------------------------------------
// XFRM modes
// ---------------------------------------------------------------------------

/// Transport mode (host-to-host).
pub const XFRM_MODE_TRANSPORT: u8 = 0;
/// Tunnel mode (gateway-to-gateway).
pub const XFRM_MODE_TUNNEL: u8 = 1;
/// Route optimization (MIPv6).
pub const XFRM_MODE_ROUTEOPTIMIZATION: u8 = 2;
/// In-trigger mode (MIPv6).
pub const XFRM_MODE_IN_TRIGGER: u8 = 3;
/// BEET mode (Bound End-to-End Tunnel).
pub const XFRM_MODE_BEET: u8 = 4;

// ---------------------------------------------------------------------------
// XFRM policy directions
// ---------------------------------------------------------------------------

/// Incoming policy.
pub const XFRM_POLICY_IN: u8 = 0;
/// Outgoing policy.
pub const XFRM_POLICY_OUT: u8 = 1;
/// Forwarding policy.
pub const XFRM_POLICY_FWD: u8 = 2;

// ---------------------------------------------------------------------------
// XFRM SA flags
// ---------------------------------------------------------------------------

/// Don't encapsulate (bypass).
pub const XFRM_STATE_NOECN: u32 = 1;
/// Decapsulation is required.
pub const XFRM_STATE_DECAP_DSCP: u32 = 2;
/// Don't fragment inner packet.
pub const XFRM_STATE_NOPMTUDISC: u32 = 4;
/// SA uses wildcard source.
pub const XFRM_STATE_WILDRECV: u32 = 8;
/// SA uses ICMP error handling.
pub const XFRM_STATE_ICMP: u32 = 16;
/// SA uses AF_UNSPEC selector.
pub const XFRM_STATE_AF_UNSPEC: u32 = 32;
/// ESN (Extended Sequence Numbers).
pub const XFRM_STATE_ESN: u32 = 64;

// ---------------------------------------------------------------------------
// XFRM netlink message types
// ---------------------------------------------------------------------------

/// New SA.
pub const XFRM_MSG_NEWSA: u16 = 0x10;
/// Delete SA.
pub const XFRM_MSG_DELSA: u16 = 0x11;
/// Get SA.
pub const XFRM_MSG_GETSA: u16 = 0x12;
/// New policy.
pub const XFRM_MSG_NEWPOLICY: u16 = 0x13;
/// Delete policy.
pub const XFRM_MSG_DELPOLICY: u16 = 0x14;
/// Get policy.
pub const XFRM_MSG_GETPOLICY: u16 = 0x15;
/// SA expire notification.
pub const XFRM_MSG_EXPIRE: u16 = 0x18;
/// Flush all SAs.
pub const XFRM_MSG_FLUSHSA: u16 = 0x1C;
/// Flush all policies.
pub const XFRM_MSG_FLUSHPOLICY: u16 = 0x1D;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            IPPROTO_ESP, IPPROTO_AH, IPPROTO_COMP,
            IPPROTO_ROUTING, IPPROTO_DSTOPTS,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        let modes = [
            XFRM_MODE_TRANSPORT, XFRM_MODE_TUNNEL,
            XFRM_MODE_ROUTEOPTIMIZATION, XFRM_MODE_IN_TRIGGER,
            XFRM_MODE_BEET,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_policy_directions() {
        assert_eq!(XFRM_POLICY_IN, 0);
        assert_eq!(XFRM_POLICY_OUT, 1);
        assert_eq!(XFRM_POLICY_FWD, 2);
    }

    #[test]
    fn test_sa_flags_no_overlap() {
        let flags = [
            XFRM_STATE_NOECN, XFRM_STATE_DECAP_DSCP,
            XFRM_STATE_NOPMTUDISC, XFRM_STATE_WILDRECV,
            XFRM_STATE_ICMP, XFRM_STATE_AF_UNSPEC,
            XFRM_STATE_ESN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [
            XFRM_MSG_NEWSA, XFRM_MSG_DELSA, XFRM_MSG_GETSA,
            XFRM_MSG_NEWPOLICY, XFRM_MSG_DELPOLICY,
            XFRM_MSG_GETPOLICY, XFRM_MSG_EXPIRE,
            XFRM_MSG_FLUSHSA, XFRM_MSG_FLUSHPOLICY,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }
}

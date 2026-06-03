//! `<linux/xfrm.h>` — IPsec / XFRM transform constants.
//!
//! XFRM (transform) is the Linux kernel's IPsec framework for
//! security associations (SA), security policies (SP), and packet
//! encryption/authentication. Used by strongSwan, Libreswan,
//! racoon, and iproute2 `ip xfrm`.

// ---------------------------------------------------------------------------
// XFRM message types (via NETLINK_XFRM)
// ---------------------------------------------------------------------------

/// New security association.
pub const XFRM_MSG_NEWSA: u32 = 0x10;
/// Delete security association.
pub const XFRM_MSG_DELSA: u32 = 0x11;
/// Get security association.
pub const XFRM_MSG_GETSA: u32 = 0x12;
/// New security policy.
pub const XFRM_MSG_NEWPOLICY: u32 = 0x13;
/// Delete security policy.
pub const XFRM_MSG_DELPOLICY: u32 = 0x14;
/// Get security policy.
pub const XFRM_MSG_GETPOLICY: u32 = 0x15;
/// Allocate SPI.
pub const XFRM_MSG_ALLOCSPI: u32 = 0x16;
/// Acquire (kernel requests SA).
pub const XFRM_MSG_ACQUIRE: u32 = 0x17;
/// SA expiration.
pub const XFRM_MSG_EXPIRE: u32 = 0x18;
/// Update security policy.
pub const XFRM_MSG_UPDPOLICY: u32 = 0x19;
/// Update security association.
pub const XFRM_MSG_UPDSA: u32 = 0x1A;
/// Policy expiration.
pub const XFRM_MSG_POLEXPIRE: u32 = 0x1B;
/// Flush SAs.
pub const XFRM_MSG_FLUSHSA: u32 = 0x1C;
/// Flush policies.
pub const XFRM_MSG_FLUSHPOLICY: u32 = 0x1D;
/// Migrate SA.
pub const XFRM_MSG_MIGRATE: u32 = 0x1E;
/// Get SA info.
pub const XFRM_MSG_GETSADINFO: u32 = 0x1F;
/// Get SPD info.
pub const XFRM_MSG_GETSPDINFO: u32 = 0x20;
/// Mapping change.
pub const XFRM_MSG_MAPPING: u32 = 0x21;

// ---------------------------------------------------------------------------
// XFRM protocols
// ---------------------------------------------------------------------------

/// AH (Authentication Header).
pub const XFRM_PROTO_AH: u8 = 51;
/// ESP (Encapsulating Security Payload).
pub const XFRM_PROTO_ESP: u8 = 50;
/// IPCOMP (IP Payload Compression).
pub const XFRM_PROTO_COMP: u8 = 108;
/// Routing Header.
pub const XFRM_PROTO_ROUTING: u8 = 43;
/// Destination Options.
pub const XFRM_PROTO_DSTOPTS: u8 = 60;

// ---------------------------------------------------------------------------
// XFRM modes
// ---------------------------------------------------------------------------

/// Transport mode.
pub const XFRM_MODE_TRANSPORT: u8 = 0;
/// Tunnel mode.
pub const XFRM_MODE_TUNNEL: u8 = 1;
/// Route optimization (MIPv6).
pub const XFRM_MODE_ROUTEOPTIMIZATION: u8 = 2;
/// In-trigger mode.
pub const XFRM_MODE_IN_TRIGGER: u8 = 3;
/// BEET mode (Bound End-to-End Tunnel).
pub const XFRM_MODE_BEET: u8 = 4;

// ---------------------------------------------------------------------------
// XFRM policy directions
// ---------------------------------------------------------------------------

/// Inbound policy.
pub const XFRM_POLICY_IN: u8 = 0;
/// Outbound policy.
pub const XFRM_POLICY_OUT: u8 = 1;
/// Forward policy.
pub const XFRM_POLICY_FWD: u8 = 2;

// ---------------------------------------------------------------------------
// XFRM SA attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const XFRMA_UNSPEC: u16 = 0;
/// Algorithm (auth).
pub const XFRMA_ALG_AUTH: u16 = 1;
/// Algorithm (encrypt).
pub const XFRMA_ALG_CRYPT: u16 = 2;
/// Algorithm (compress).
pub const XFRMA_ALG_COMP: u16 = 3;
/// Encapsulation.
pub const XFRMA_ENCAP: u16 = 4;
/// Template.
pub const XFRMA_TMPL: u16 = 5;
/// SA.
pub const XFRMA_SA: u16 = 6;
/// Policy.
pub const XFRMA_POLICY: u16 = 7;
/// Replay value.
pub const XFRMA_REPLAY_VAL: u16 = 10;
/// Lifetime current.
pub const XFRMA_LTIME_VAL: u16 = 11;
/// Algorithm (AEAD).
pub const XFRMA_ALG_AEAD: u16 = 18;
/// Mark.
pub const XFRMA_MARK: u16 = 20;
/// Interface ID.
pub const XFRMA_IF_ID: u16 = 28;

// ---------------------------------------------------------------------------
// XFRM SA flags
// ---------------------------------------------------------------------------

/// Don't encapsulate (used for BEET mode).
pub const XFRM_STATE_NOECN: u32 = 1;
/// Decapsulation only.
pub const XFRM_STATE_DECAP_DSCP: u32 = 2;
/// Don't add path MTU.
pub const XFRM_STATE_NOPMTUDISC: u32 = 4;
/// ESN (Extended Sequence Numbers).
pub const XFRM_STATE_ESN: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_types_sequential() {
        assert_eq!(XFRM_MSG_NEWSA, 0x10);
        assert_eq!(XFRM_MSG_DELSA, 0x11);
        assert_eq!(XFRM_MSG_GETSA, 0x12);
        assert_eq!(XFRM_MSG_NEWPOLICY, 0x13);
    }

    #[test]
    fn test_msg_types_distinct() {
        let msgs = [
            XFRM_MSG_NEWSA,
            XFRM_MSG_DELSA,
            XFRM_MSG_GETSA,
            XFRM_MSG_NEWPOLICY,
            XFRM_MSG_DELPOLICY,
            XFRM_MSG_GETPOLICY,
            XFRM_MSG_ALLOCSPI,
            XFRM_MSG_ACQUIRE,
            XFRM_MSG_EXPIRE,
            XFRM_MSG_UPDPOLICY,
            XFRM_MSG_UPDSA,
            XFRM_MSG_POLEXPIRE,
            XFRM_MSG_FLUSHSA,
            XFRM_MSG_FLUSHPOLICY,
            XFRM_MSG_MIGRATE,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_protocols() {
        assert_eq!(XFRM_PROTO_ESP, 50);
        assert_eq!(XFRM_PROTO_AH, 51);
        assert_eq!(XFRM_PROTO_COMP, 108);
    }

    #[test]
    fn test_modes() {
        assert_eq!(XFRM_MODE_TRANSPORT, 0);
        assert_eq!(XFRM_MODE_TUNNEL, 1);
        assert_eq!(XFRM_MODE_BEET, 4);
    }

    #[test]
    fn test_policy_directions() {
        assert_eq!(XFRM_POLICY_IN, 0);
        assert_eq!(XFRM_POLICY_OUT, 1);
        assert_eq!(XFRM_POLICY_FWD, 2);
    }

    #[test]
    fn test_sa_attrs_distinct() {
        let attrs = [
            XFRMA_UNSPEC,
            XFRMA_ALG_AUTH,
            XFRMA_ALG_CRYPT,
            XFRMA_ALG_COMP,
            XFRMA_ENCAP,
            XFRMA_TMPL,
            XFRMA_SA,
            XFRMA_POLICY,
            XFRMA_REPLAY_VAL,
            XFRMA_LTIME_VAL,
            XFRMA_ALG_AEAD,
            XFRMA_MARK,
            XFRMA_IF_ID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

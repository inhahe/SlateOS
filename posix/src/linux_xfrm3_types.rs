//! `<linux/xfrm.h>` — Additional XFRM/IPsec constants (batch 3).
//!
//! Supplementary XFRM constants covering SA flags,
//! policy action types, and migrate types.

// ---------------------------------------------------------------------------
// XFRM SA flags (XFRM_STATE_*)
// ---------------------------------------------------------------------------

/// SA has no PMTU discovery.
pub const XFRM_STATE_NOPMTUDISC: u32 = 1 << 0;
/// SA supports wildcard.
pub const XFRM_STATE_WILDRECV: u32 = 1 << 1;
/// SA has been used for input.
pub const XFRM_STATE_ICMP: u32 = 1 << 2;
/// SA is in AF_UNSPEC mode.
pub const XFRM_STATE_AF_UNSPEC: u32 = 1 << 3;
/// SA uses per-flow alignment.
pub const XFRM_STATE_ALIGN4: u32 = 1 << 4;
/// SA uses ESN (Extended Sequence Number).
pub const XFRM_STATE_ESN: u32 = 1 << 5;
/// SA has no ECMP.
pub const XFRM_STATE_NOECN: u32 = 1 << 6;
/// SA has been output-only.
pub const XFRM_STATE_DECAP_DSCP: u32 = 1 << 7;
/// SA supports output marks.
pub const XFRM_STATE_OUTPUT_MARK: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// XFRM policy action
// ---------------------------------------------------------------------------

/// Allow traffic.
pub const XFRM_POLICY_ALLOW: u32 = 0;
/// Block traffic.
pub const XFRM_POLICY_BLOCK: u32 = 1;

// ---------------------------------------------------------------------------
// XFRM policy direction
// ---------------------------------------------------------------------------

/// Input policy.
pub const XFRM_POLICY_IN: u32 = 0;
/// Output policy.
pub const XFRM_POLICY_OUT: u32 = 1;
/// Forward policy.
pub const XFRM_POLICY_FWD: u32 = 2;

/// Maximum number of policy directions.
pub const XFRM_POLICY_MAX: u32 = 3;

// ---------------------------------------------------------------------------
// XFRM migrate types
// ---------------------------------------------------------------------------

/// Migrate SA.
pub const XFRM_MSG_MIGRATE: u32 = 0;
/// Migrate policy.
pub const XFRM_MSG_UPDPOLICY: u32 = 1;

// ---------------------------------------------------------------------------
// XFRM protocol types
// ---------------------------------------------------------------------------

/// AH (Authentication Header).
pub const XFRM_PROTO_AH: u32 = 51;
/// ESP (Encapsulating Security Payload).
pub const XFRM_PROTO_ESP: u32 = 50;
/// IPCOMP (IP Compression).
pub const XFRM_PROTO_COMP: u32 = 108;
/// Routing header (type 2).
pub const XFRM_PROTO_ROUTING: u32 = 43;
/// Destination options.
pub const XFRM_PROTO_DSTOPTS: u32 = 60;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sa_flags_power_of_two() {
        let flags = [
            XFRM_STATE_NOPMTUDISC,
            XFRM_STATE_WILDRECV,
            XFRM_STATE_ICMP,
            XFRM_STATE_AF_UNSPEC,
            XFRM_STATE_ALIGN4,
            XFRM_STATE_ESN,
            XFRM_STATE_NOECN,
            XFRM_STATE_DECAP_DSCP,
            XFRM_STATE_OUTPUT_MARK,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_sa_flags_no_overlap() {
        let flags = [
            XFRM_STATE_NOPMTUDISC,
            XFRM_STATE_WILDRECV,
            XFRM_STATE_ICMP,
            XFRM_STATE_AF_UNSPEC,
            XFRM_STATE_ALIGN4,
            XFRM_STATE_ESN,
            XFRM_STATE_NOECN,
            XFRM_STATE_DECAP_DSCP,
            XFRM_STATE_OUTPUT_MARK,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_policy_actions_distinct() {
        assert_ne!(XFRM_POLICY_ALLOW, XFRM_POLICY_BLOCK);
    }

    #[test]
    fn test_policy_directions_distinct() {
        let dirs = [XFRM_POLICY_IN, XFRM_POLICY_OUT, XFRM_POLICY_FWD];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [
            XFRM_PROTO_AH,
            XFRM_PROTO_ESP,
            XFRM_PROTO_COMP,
            XFRM_PROTO_ROUTING,
            XFRM_PROTO_DSTOPTS,
        ];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }
}

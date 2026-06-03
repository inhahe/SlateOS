//! `<linux/xfrm.h>` — Additional XFRM/IPsec constants.
//!
//! Supplementary XFRM constants covering message types,
//! modes, policy directions, and SA flags.

// ---------------------------------------------------------------------------
// XFRM message types (XFRM_MSG_*)
// ---------------------------------------------------------------------------

/// New SA.
pub const XFRM_MSG_NEWSA: u32 = 0x10;
/// Delete SA.
pub const XFRM_MSG_DELSA: u32 = 0x11;
/// Get SA.
pub const XFRM_MSG_GETSA: u32 = 0x12;
/// New policy.
pub const XFRM_MSG_NEWPOLICY: u32 = 0x13;
/// Delete policy.
pub const XFRM_MSG_DELPOLICY: u32 = 0x14;
/// Get policy.
pub const XFRM_MSG_GETPOLICY: u32 = 0x15;
/// SA allocation.
pub const XFRM_MSG_ALLOCSPI: u32 = 0x16;
/// Acquire.
pub const XFRM_MSG_ACQUIRE: u32 = 0x17;
/// SA expire.
pub const XFRM_MSG_EXPIRE: u32 = 0x18;
/// Update policy.
pub const XFRM_MSG_UPDPOLICY: u32 = 0x19;
/// Update SA.
pub const XFRM_MSG_UPDSA: u32 = 0x1A;
/// Policy expire.
pub const XFRM_MSG_POLEXPIRE: u32 = 0x1B;
/// Flush SA.
pub const XFRM_MSG_FLUSHSA: u32 = 0x1C;
/// Flush policy.
pub const XFRM_MSG_FLUSHPOLICY: u32 = 0x1D;
/// New AE (async event).
pub const XFRM_MSG_NEWAE: u32 = 0x1E;
/// Get AE.
pub const XFRM_MSG_GETAE: u32 = 0x1F;
/// Report.
pub const XFRM_MSG_REPORT: u32 = 0x20;
/// Migrate.
pub const XFRM_MSG_MIGRATE: u32 = 0x21;
/// Get SA/policy info.
pub const XFRM_MSG_GETSADINFO: u32 = 0x22;
/// Get SPD info.
pub const XFRM_MSG_GETSPDINFO: u32 = 0x23;
/// Mapping change.
pub const XFRM_MSG_MAPPING: u32 = 0x24;
/// Set default policy.
pub const XFRM_MSG_SETDEFAULT: u32 = 0x25;
/// Get default policy.
pub const XFRM_MSG_GETDEFAULT: u32 = 0x26;

// ---------------------------------------------------------------------------
// XFRM modes
// ---------------------------------------------------------------------------

/// Transport mode.
pub const XFRM_MODE_TRANSPORT: u8 = 0;
/// Tunnel mode.
pub const XFRM_MODE_TUNNEL: u8 = 1;
/// Route optimization.
pub const XFRM_MODE_ROUTEOPTIMIZATION: u8 = 2;
/// In-trigger mode.
pub const XFRM_MODE_IN_TRIGGER: u8 = 3;
/// BEET mode.
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
/// Policy mask.
pub const XFRM_POLICY_MASK: u8 = 3;

// ---------------------------------------------------------------------------
// XFRM SA flags
// ---------------------------------------------------------------------------

/// Don't encapsulate.
pub const XFRM_STATE_NOECN: u8 = 1;
/// Decap DSCP.
pub const XFRM_STATE_DECAP_DSCP: u8 = 2;
/// No PMTU discovery.
pub const XFRM_STATE_NOPMTUDISC: u8 = 4;
/// Wildcard receive.
pub const XFRM_STATE_WILDRECV: u8 = 8;
/// ICMP error.
pub const XFRM_STATE_ICMP: u8 = 16;
/// AF unspec.
pub const XFRM_STATE_AF_UNSPEC: u8 = 32;
/// ESN (extended sequence number).
pub const XFRM_STATE_ESN: u8 = 64;

// ---------------------------------------------------------------------------
// XFRM policy actions
// ---------------------------------------------------------------------------

/// Allow traffic.
pub const XFRM_POLICY_ALLOW: u8 = 0;
/// Block traffic.
pub const XFRM_POLICY_BLOCK: u8 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
            XFRM_MSG_NEWAE,
            XFRM_MSG_GETAE,
            XFRM_MSG_REPORT,
            XFRM_MSG_MIGRATE,
            XFRM_MSG_GETSADINFO,
            XFRM_MSG_GETSPDINFO,
            XFRM_MSG_MAPPING,
            XFRM_MSG_SETDEFAULT,
            XFRM_MSG_GETDEFAULT,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        let modes: [u8; 5] = [
            XFRM_MODE_TRANSPORT,
            XFRM_MODE_TUNNEL,
            XFRM_MODE_ROUTEOPTIMIZATION,
            XFRM_MODE_IN_TRIGGER,
            XFRM_MODE_BEET,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_policy_dirs() {
        assert_eq!(XFRM_POLICY_IN, 0);
        assert_eq!(XFRM_POLICY_OUT, 1);
        assert_eq!(XFRM_POLICY_FWD, 2);
        assert_eq!(XFRM_POLICY_MASK, 3);
    }

    #[test]
    fn test_sa_flags_power_of_two() {
        let flags: [u8; 7] = [
            XFRM_STATE_NOECN,
            XFRM_STATE_DECAP_DSCP,
            XFRM_STATE_NOPMTUDISC,
            XFRM_STATE_WILDRECV,
            XFRM_STATE_ICMP,
            XFRM_STATE_AF_UNSPEC,
            XFRM_STATE_ESN,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:02x} not power of two", f);
        }
    }

    #[test]
    fn test_sa_flags_no_overlap() {
        let flags: [u8; 7] = [
            XFRM_STATE_NOECN,
            XFRM_STATE_DECAP_DSCP,
            XFRM_STATE_NOPMTUDISC,
            XFRM_STATE_WILDRECV,
            XFRM_STATE_ICMP,
            XFRM_STATE_AF_UNSPEC,
            XFRM_STATE_ESN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_policy_actions() {
        assert_eq!(XFRM_POLICY_ALLOW, 0);
        assert_eq!(XFRM_POLICY_BLOCK, 1);
    }
}

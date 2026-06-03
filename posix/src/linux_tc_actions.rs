//! `<linux/tc_act/*.h>` — Traffic Control action constants.
//!
//! TC (Traffic Control) actions are the building blocks of the
//! Linux packet classifier/scheduler pipeline. Actions modify,
//! redirect, mirror, drop, or mark packets as they traverse
//! qdiscs and filters.

// ---------------------------------------------------------------------------
// Action verdicts (generic)
// ---------------------------------------------------------------------------

/// Continue to next action.
pub const TC_ACT_UNSPEC: i32 = -1;
/// Accept packet (pass through).
pub const TC_ACT_OK: i32 = 0;
/// Reclassify packet.
pub const TC_ACT_RECLASSIFY: i32 = 1;
/// Drop/shot packet.
pub const TC_ACT_SHOT: i32 = 2;
/// Pipe to next action.
pub const TC_ACT_PIPE: i32 = 3;
/// Steal packet (consume silently).
pub const TC_ACT_STOLEN: i32 = 4;
/// Queue packet.
pub const TC_ACT_QUEUED: i32 = 5;
/// Repeat action.
pub const TC_ACT_REPEAT: i32 = 6;
/// Redirect packet.
pub const TC_ACT_REDIRECT: i32 = 7;
/// Trap packet (send to CPU).
pub const TC_ACT_TRAP: i32 = 8;

// ---------------------------------------------------------------------------
// Action types (TCA_ACT_*)
// ---------------------------------------------------------------------------

/// Generic action.
pub const TCA_ACT_GACT: u16 = 5;
/// Mirror/redirect action.
pub const TCA_ACT_MIRRED: u16 = 8;
/// Packet edit action.
pub const TCA_ACT_PEDIT: u16 = 7;
/// Police (rate limit) action.
pub const TCA_ACT_POLICE: u16 = 100;
/// NAT action.
pub const TCA_ACT_NAT: u16 = 9;
/// VLAN action.
pub const TCA_ACT_VLAN: u16 = 12;
/// Tunnel key action.
pub const TCA_ACT_TUNNEL_KEY: u16 = 17;
/// Connection tracking action.
pub const TCA_ACT_CT: u16 = 20;
/// Skb edit action.
pub const TCA_ACT_SKBEDIT: u16 = 11;
/// MPLS action.
pub const TCA_ACT_MPLS: u16 = 22;
/// Csum (checksum) action.
pub const TCA_ACT_CSUM: u16 = 16;

// ---------------------------------------------------------------------------
// GACT (Generic Action) sub-actions
// ---------------------------------------------------------------------------

/// GACT: probabilistic drop.
pub const GACT_PROB_DROP: u8 = 0;
/// GACT: probabilistic pass.
pub const GACT_PROB_PASS: u8 = 1;
/// GACT: probabilistic reclassify.
pub const GACT_PROB_RECLASSIFY: u8 = 2;
/// GACT: deterministic (no probability).
pub const GACT_PROB_NONE: u8 = 3;

// ---------------------------------------------------------------------------
// Action bind/flags
// ---------------------------------------------------------------------------

/// Action bound to filter.
pub const TCA_ACT_BIND: u32 = 1;
/// Action does not bind.
pub const TCA_ACT_NOBIND: u32 = 0;
/// Replace existing action.
pub const TCA_ACT_REPLACE: u32 = 1;
/// Don't replace if exists.
pub const TCA_ACT_NOREPLACE: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdicts_distinct() {
        let verdicts = [
            TC_ACT_UNSPEC,
            TC_ACT_OK,
            TC_ACT_RECLASSIFY,
            TC_ACT_SHOT,
            TC_ACT_PIPE,
            TC_ACT_STOLEN,
            TC_ACT_QUEUED,
            TC_ACT_REPEAT,
            TC_ACT_REDIRECT,
            TC_ACT_TRAP,
        ];
        for i in 0..verdicts.len() {
            for j in (i + 1)..verdicts.len() {
                assert_ne!(verdicts[i], verdicts[j]);
            }
        }
    }

    #[test]
    fn test_action_types_distinct() {
        let types = [
            TCA_ACT_GACT,
            TCA_ACT_MIRRED,
            TCA_ACT_PEDIT,
            TCA_ACT_POLICE,
            TCA_ACT_NAT,
            TCA_ACT_VLAN,
            TCA_ACT_TUNNEL_KEY,
            TCA_ACT_CT,
            TCA_ACT_SKBEDIT,
            TCA_ACT_MPLS,
            TCA_ACT_CSUM,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_gact_prob_distinct() {
        let probs = [
            GACT_PROB_DROP,
            GACT_PROB_PASS,
            GACT_PROB_RECLASSIFY,
            GACT_PROB_NONE,
        ];
        for i in 0..probs.len() {
            for j in (i + 1)..probs.len() {
                assert_ne!(probs[i], probs[j]);
            }
        }
    }

    #[test]
    fn test_shot_drops() {
        // TC_ACT_SHOT is the standard "drop" verdict
        assert_eq!(TC_ACT_SHOT, 2);
    }
}

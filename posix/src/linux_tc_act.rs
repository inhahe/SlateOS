//! `<linux/tc_act/tc_gact.h>` + generic TC action constants.
//!
//! Traffic Control (TC) actions define what happens to packets that
//! match classifier rules. Actions include accept, drop, redirect,
//! mirror, and modify.

// ---------------------------------------------------------------------------
// Generic TC action verdicts
// ---------------------------------------------------------------------------

/// Continue processing.
pub const TC_ACT_UNSPEC: i32 = -1;
/// Accept (use default class).
pub const TC_ACT_OK: i32 = 0;
/// Reclassify.
pub const TC_ACT_RECLASSIFY: i32 = 1;
/// Drop (shot).
pub const TC_ACT_SHOT: i32 = 2;
/// Pipe to next action.
pub const TC_ACT_PIPE: i32 = 3;
/// Stolen (consumed by action).
pub const TC_ACT_STOLEN: i32 = 4;
/// Queued for later.
pub const TC_ACT_QUEUED: i32 = 5;
/// Repeat.
pub const TC_ACT_REPEAT: i32 = 6;
/// Redirect.
pub const TC_ACT_REDIRECT: i32 = 7;
/// Trap.
pub const TC_ACT_TRAP: i32 = 8;

// ---------------------------------------------------------------------------
// gact action attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_GACT_UNSPEC: u16 = 0;
/// Parameters.
pub const TCA_GACT_TM: u16 = 1;
/// Parameters.
pub const TCA_GACT_PARMS: u16 = 2;
/// Probability.
pub const TCA_GACT_PROB: u16 = 3;
/// Pad.
pub const TCA_GACT_PAD: u16 = 4;

// ---------------------------------------------------------------------------
// mirred action types
// ---------------------------------------------------------------------------

/// Egress redirect.
pub const TCA_EGRESS_REDIR: i32 = 1;
/// Egress mirror.
pub const TCA_EGRESS_MIRROR: i32 = 2;
/// Ingress redirect.
pub const TCA_INGRESS_REDIR: i32 = 3;
/// Ingress mirror.
pub const TCA_INGRESS_MIRROR: i32 = 4;

// ---------------------------------------------------------------------------
// Action attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_ACT_UNSPEC: u16 = 0;
/// Kind (action type name).
pub const TCA_ACT_KIND: u16 = 1;
/// Options.
pub const TCA_ACT_OPTIONS: u16 = 2;
/// Index.
pub const TCA_ACT_INDEX: u16 = 3;
/// Statistics.
pub const TCA_ACT_STATS: u16 = 4;
/// Pad.
pub const TCA_ACT_PAD: u16 = 5;
/// Cookie.
pub const TCA_ACT_COOKIE: u16 = 6;
/// Flags.
pub const TCA_ACT_FLAGS: u16 = 7;
/// Hardware stats.
pub const TCA_ACT_HW_STATS: u16 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdicts_sequential() {
        assert_eq!(TC_ACT_OK, 0);
        assert_eq!(TC_ACT_RECLASSIFY, 1);
        assert_eq!(TC_ACT_SHOT, 2);
        assert_eq!(TC_ACT_PIPE, 3);
        assert_eq!(TC_ACT_REDIRECT, 7);
    }

    #[test]
    fn test_verdicts_distinct() {
        let acts = [
            TC_ACT_UNSPEC, TC_ACT_OK, TC_ACT_RECLASSIFY,
            TC_ACT_SHOT, TC_ACT_PIPE, TC_ACT_STOLEN,
            TC_ACT_QUEUED, TC_ACT_REPEAT, TC_ACT_REDIRECT,
            TC_ACT_TRAP,
        ];
        for i in 0..acts.len() {
            for j in (i + 1)..acts.len() {
                assert_ne!(acts[i], acts[j]);
            }
        }
    }

    #[test]
    fn test_mirred_types_distinct() {
        let types = [
            TCA_EGRESS_REDIR, TCA_EGRESS_MIRROR,
            TCA_INGRESS_REDIR, TCA_INGRESS_MIRROR,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_act_attrs_distinct() {
        let attrs = [
            TCA_ACT_UNSPEC, TCA_ACT_KIND, TCA_ACT_OPTIONS,
            TCA_ACT_INDEX, TCA_ACT_STATS, TCA_ACT_PAD,
            TCA_ACT_COOKIE, TCA_ACT_FLAGS, TCA_ACT_HW_STATS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

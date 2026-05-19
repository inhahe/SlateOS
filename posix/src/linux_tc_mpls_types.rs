//! `<linux/tc_act/tc_mpls.h>` — TC MPLS action constants.
//!
//! Traffic control MPLS action constants covering attribute types
//! and action commands for MPLS label manipulation.

// ---------------------------------------------------------------------------
// TC MPLS action commands
// ---------------------------------------------------------------------------

/// Pop MPLS header.
pub const TCA_MPLS_ACT_POP: u32 = 1;
/// Push MPLS header.
pub const TCA_MPLS_ACT_PUSH: u32 = 2;
/// Modify MPLS header.
pub const TCA_MPLS_ACT_MODIFY: u32 = 3;
/// Decrement MPLS TTL.
pub const TCA_MPLS_ACT_DEC_TTL: u32 = 4;
/// Modify and push MPLS header.
pub const TCA_MPLS_ACT_MAC_PUSH: u32 = 5;

// ---------------------------------------------------------------------------
// TC MPLS attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_MPLS_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_MPLS_TM: u32 = 1;
/// Parameters.
pub const TCA_MPLS_PARMS: u32 = 2;
/// Protocol.
pub const TCA_MPLS_PROTO: u32 = 3;
/// Label.
pub const TCA_MPLS_LABEL: u32 = 4;
/// Traffic class.
pub const TCA_MPLS_TC: u32 = 5;
/// TTL.
pub const TCA_MPLS_TTL: u32 = 6;
/// Bottom of stack.
pub const TCA_MPLS_BOS: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_cmds_distinct() {
        let cmds = [
            TCA_MPLS_ACT_POP, TCA_MPLS_ACT_PUSH,
            TCA_MPLS_ACT_MODIFY, TCA_MPLS_ACT_DEC_TTL,
            TCA_MPLS_ACT_MAC_PUSH,
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
            TCA_MPLS_UNSPEC, TCA_MPLS_TM, TCA_MPLS_PARMS,
            TCA_MPLS_PROTO, TCA_MPLS_LABEL, TCA_MPLS_TC,
            TCA_MPLS_TTL, TCA_MPLS_BOS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

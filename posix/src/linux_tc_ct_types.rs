//! `<linux/tc_act/tc_ct.h>` — TC conntrack action constants.
//!
//! Traffic control conntrack action constants covering attribute types
//! and action flags for connection tracking integration.

// ---------------------------------------------------------------------------
// TC CT attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_CT_UNSPEC: u32 = 0;
/// Parameters.
pub const TCA_CT_PARMS: u32 = 1;
/// Timestamp.
pub const TCA_CT_TM: u32 = 2;
/// Action.
pub const TCA_CT_ACTION: u32 = 3;
/// Zone.
pub const TCA_CT_ZONE: u32 = 4;
/// Mark.
pub const TCA_CT_MARK: u32 = 5;
/// Mark mask.
pub const TCA_CT_MARK_MASK: u32 = 6;
/// Labels.
pub const TCA_CT_LABELS: u32 = 7;
/// Labels mask.
pub const TCA_CT_LABELS_MASK: u32 = 8;
/// NAT IPv4 min.
pub const TCA_CT_NAT_IPV4_MIN: u32 = 9;
/// NAT IPv4 max.
pub const TCA_CT_NAT_IPV4_MAX: u32 = 10;
/// NAT IPv6 min.
pub const TCA_CT_NAT_IPV6_MIN: u32 = 11;
/// NAT IPv6 max.
pub const TCA_CT_NAT_IPV6_MAX: u32 = 12;
/// NAT port min.
pub const TCA_CT_NAT_PORT_MIN: u32 = 13;
/// NAT port max.
pub const TCA_CT_NAT_PORT_MAX: u32 = 14;

// ---------------------------------------------------------------------------
// TC CT action flags
// ---------------------------------------------------------------------------

/// Commit.
pub const TCA_CT_ACT_COMMIT: u32 = 1 << 0;
/// Force.
pub const TCA_CT_ACT_FORCE: u32 = 1 << 1;
/// Clear.
pub const TCA_CT_ACT_CLEAR: u32 = 1 << 2;
/// NAT.
pub const TCA_CT_ACT_NAT: u32 = 1 << 3;
/// Source NAT.
pub const TCA_CT_ACT_NAT_SRC: u32 = 1 << 4;
/// Destination NAT.
pub const TCA_CT_ACT_NAT_DST: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_CT_UNSPEC, TCA_CT_PARMS, TCA_CT_TM,
            TCA_CT_ACTION, TCA_CT_ZONE, TCA_CT_MARK,
            TCA_CT_MARK_MASK, TCA_CT_LABELS, TCA_CT_LABELS_MASK,
            TCA_CT_NAT_IPV4_MIN, TCA_CT_NAT_IPV4_MAX,
            TCA_CT_NAT_IPV6_MIN, TCA_CT_NAT_IPV6_MAX,
            TCA_CT_NAT_PORT_MIN, TCA_CT_NAT_PORT_MAX,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_action_flags_no_overlap() {
        let flags = [
            TCA_CT_ACT_COMMIT, TCA_CT_ACT_FORCE, TCA_CT_ACT_CLEAR,
            TCA_CT_ACT_NAT, TCA_CT_ACT_NAT_SRC, TCA_CT_ACT_NAT_DST,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_action_flags_power_of_two() {
        let flags = [
            TCA_CT_ACT_COMMIT, TCA_CT_ACT_FORCE, TCA_CT_ACT_CLEAR,
            TCA_CT_ACT_NAT, TCA_CT_ACT_NAT_SRC, TCA_CT_ACT_NAT_DST,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not power of two", flag);
        }
    }
}

//! `<linux/tc_act/tc_ct.h>` — TC connection tracking action constants.
//!
//! The ct (connection tracking) action integrates netfilter conntrack
//! with the TC datapath. It can commit new connections, establish
//! tracking zones, set marks/labels, and force NAT — enabling full
//! stateful firewall offload in TC/hardware.

// ---------------------------------------------------------------------------
// CT action flags
// ---------------------------------------------------------------------------

/// Commit connection to conntrack table.
pub const TCA_CT_ACT_COMMIT: u32 = 1 << 0;
/// Force NAT (trigger NAT engine).
pub const TCA_CT_ACT_FORCE: u32 = 1 << 1;
/// Clear conntrack state.
pub const TCA_CT_ACT_CLEAR: u32 = 1 << 2;
/// Apply NAT (generic).
pub const TCA_CT_ACT_NAT: u32 = 1 << 3;
/// Source NAT.
pub const TCA_CT_ACT_NAT_SRC: u32 = 1 << 4;
/// Destination NAT.
pub const TCA_CT_ACT_NAT_DST: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// CT netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_CT_UNSPEC: u16 = 0;
/// Parameters.
pub const TCA_CT_PARMS: u16 = 1;
/// Timer info.
pub const TCA_CT_TM: u16 = 2;
/// CT action flags.
pub const TCA_CT_ACTION: u16 = 3;
/// Conntrack zone.
pub const TCA_CT_ZONE: u16 = 4;
/// Conntrack mark value.
pub const TCA_CT_MARK: u16 = 5;
/// Conntrack mark mask.
pub const TCA_CT_MARK_MASK: u16 = 6;
/// Conntrack label.
pub const TCA_CT_LABELS: u16 = 7;
/// Conntrack label mask.
pub const TCA_CT_LABELS_MASK: u16 = 8;
/// NAT IPv4 min address.
pub const TCA_CT_NAT_IPV4_MIN: u16 = 9;
/// NAT IPv4 max address.
pub const TCA_CT_NAT_IPV4_MAX: u16 = 10;
/// NAT IPv6 min address.
pub const TCA_CT_NAT_IPV6_MIN: u16 = 11;
/// NAT IPv6 max address.
pub const TCA_CT_NAT_IPV6_MAX: u16 = 12;
/// NAT port min.
pub const TCA_CT_NAT_PORT_MIN: u16 = 13;
/// NAT port max.
pub const TCA_CT_NAT_PORT_MAX: u16 = 14;
/// Padding.
pub const TCA_CT_PAD: u16 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_flags_no_overlap() {
        let flags = [
            TCA_CT_ACT_COMMIT,
            TCA_CT_ACT_FORCE,
            TCA_CT_ACT_CLEAR,
            TCA_CT_ACT_NAT,
            TCA_CT_ACT_NAT_SRC,
            TCA_CT_ACT_NAT_DST,
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
            TCA_CT_ACT_COMMIT,
            TCA_CT_ACT_FORCE,
            TCA_CT_ACT_CLEAR,
            TCA_CT_ACT_NAT,
            TCA_CT_ACT_NAT_SRC,
            TCA_CT_ACT_NAT_DST,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_CT_UNSPEC,
            TCA_CT_PARMS,
            TCA_CT_TM,
            TCA_CT_ACTION,
            TCA_CT_ZONE,
            TCA_CT_MARK,
            TCA_CT_MARK_MASK,
            TCA_CT_LABELS,
            TCA_CT_LABELS_MASK,
            TCA_CT_NAT_IPV4_MIN,
            TCA_CT_NAT_IPV4_MAX,
            TCA_CT_NAT_IPV6_MIN,
            TCA_CT_NAT_IPV6_MAX,
            TCA_CT_NAT_PORT_MIN,
            TCA_CT_NAT_PORT_MAX,
            TCA_CT_PAD,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

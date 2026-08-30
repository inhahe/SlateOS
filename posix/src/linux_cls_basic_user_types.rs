//! `<linux/pkt_cls.h>` (basic classifier) — TCA_BASIC_* attributes.
//!
//! The "basic" tc classifier matches packets against an ematch
//! expression. It's the simplest filter — useful for catch-all rules
//! or experimentation. Each filter has a classid, an ematch tree,
//! and an optional action chain.

// ---------------------------------------------------------------------------
// TCA_BASIC_* attribute IDs (netlink TLVs)
// ---------------------------------------------------------------------------

pub const TCA_BASIC_UNSPEC: u32 = 0;
pub const TCA_BASIC_CLASSID: u32 = 1;
pub const TCA_BASIC_EMATCHES: u32 = 2;
pub const TCA_BASIC_ACT: u32 = 3;
pub const TCA_BASIC_POLICE: u32 = 4;
pub const TCA_BASIC_PCNT: u32 = 5;
pub const TCA_BASIC_PAD: u32 = 6;

/// One past the highest valid attribute (kernel convention).
pub const TCA_BASIC_MAX: u32 = 6;

// ---------------------------------------------------------------------------
// Filter name in /proc/net/protocols and tc command
// ---------------------------------------------------------------------------

pub const CLS_BASIC_NAME: &str = "basic";

// ---------------------------------------------------------------------------
// Per-filter statistics — struct tc_basic_pcnt
// ---------------------------------------------------------------------------

/// `__u64 rcnt` — number of refusals.
pub const BASIC_PCNT_OFF_RCNT: usize = 0;
/// `__u64 rhit` — number of hits.
pub const BASIC_PCNT_OFF_RHIT: usize = 8;
pub const BASIC_PCNT_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attr_ids_dense_0_to_6() {
        let a = [
            TCA_BASIC_UNSPEC,
            TCA_BASIC_CLASSID,
            TCA_BASIC_EMATCHES,
            TCA_BASIC_ACT,
            TCA_BASIC_POLICE,
            TCA_BASIC_PCNT,
            TCA_BASIC_PAD,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_max_matches_highest_attr() {
        assert_eq!(TCA_BASIC_MAX, TCA_BASIC_PAD);
    }

    #[test]
    fn test_filter_name_is_basic() {
        assert_eq!(CLS_BASIC_NAME, "basic");
        assert!(CLS_BASIC_NAME.chars().all(|c| c.is_ascii_lowercase()));
    }

    #[test]
    fn test_pcnt_layout_two_u64s() {
        assert_eq!(BASIC_PCNT_OFF_RCNT, 0);
        assert_eq!(BASIC_PCNT_OFF_RHIT, 8);
        assert_eq!(BASIC_PCNT_SIZE, 16);
    }
}

//! `<linux/pkt_cls.h>` (matchall classifier) — TCA_MATCHALL_* attrs.
//!
//! The "matchall" tc classifier matches every packet. It's used to
//! apply an action chain unconditionally (e.g., redirect every packet
//! to another port). The classifier itself has no match logic — only
//! a classid and a list of actions.

// ---------------------------------------------------------------------------
// TCA_MATCHALL_* attribute IDs
// ---------------------------------------------------------------------------

pub const TCA_MATCHALL_UNSPEC: u32 = 0;
pub const TCA_MATCHALL_CLASSID: u32 = 1;
pub const TCA_MATCHALL_ACT: u32 = 2;
pub const TCA_MATCHALL_FLAGS: u32 = 3;
pub const TCA_MATCHALL_PCNT: u32 = 4;
pub const TCA_MATCHALL_PAD: u32 = 5;

pub const TCA_MATCHALL_MAX: u32 = 5;

// ---------------------------------------------------------------------------
// matchall flags
// ---------------------------------------------------------------------------

/// Skip software (only run if hardware offload available).
pub const TCA_CLS_FLAGS_SKIP_HW: u32 = 1 << 0;
/// Skip hardware (only run software classifier).
pub const TCA_CLS_FLAGS_SKIP_SW: u32 = 1 << 1;
/// Classifier is offloaded to hardware.
pub const TCA_CLS_FLAGS_IN_HW: u32 = 1 << 2;
/// Classifier was not offloaded.
pub const TCA_CLS_FLAGS_NOT_IN_HW: u32 = 1 << 3;
/// Classifier verdict marks this filter terminal.
pub const TCA_CLS_FLAGS_VERBOSE: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Filter name string
// ---------------------------------------------------------------------------

pub const CLS_MATCHALL_NAME: &str = "matchall";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attr_ids_dense_0_to_5() {
        let a = [
            TCA_MATCHALL_UNSPEC,
            TCA_MATCHALL_CLASSID,
            TCA_MATCHALL_ACT,
            TCA_MATCHALL_FLAGS,
            TCA_MATCHALL_PCNT,
            TCA_MATCHALL_PAD,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_max_matches_pad() {
        assert_eq!(TCA_MATCHALL_MAX, TCA_MATCHALL_PAD);
    }

    #[test]
    fn test_cls_flags_distinct_single_bit() {
        let f = [
            TCA_CLS_FLAGS_SKIP_HW,
            TCA_CLS_FLAGS_SKIP_SW,
            TCA_CLS_FLAGS_IN_HW,
            TCA_CLS_FLAGS_NOT_IN_HW,
            TCA_CLS_FLAGS_VERBOSE,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
        // OR of all 5 = 0x1F.
        let or_all = f.iter().fold(0u32, |a, &v| a | v);
        assert_eq!(or_all, 0x1F);
    }

    #[test]
    fn test_skip_hw_and_skip_sw_mutually_meaningless() {
        // Both set = neither hw nor sw runs (rejected by kernel, but
        // distinct bits so they CAN be combined in the netlink message).
        assert_eq!(TCA_CLS_FLAGS_SKIP_HW & TCA_CLS_FLAGS_SKIP_SW, 0);
    }

    #[test]
    fn test_filter_name() {
        assert_eq!(CLS_MATCHALL_NAME, "matchall");
    }
}

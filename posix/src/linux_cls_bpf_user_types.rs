//! `<linux/pkt_cls.h>` (BPF classifier) — TCA_BPF_FLAG_* and direct-action.
//!
//! The BPF tc classifier runs a BPF program to decide a packet's class.
//! Two flag bits control direct-action mode (skip the action chain) and
//! skip-software mode (only run if hardware offload available).

// ---------------------------------------------------------------------------
// BPF classifier flags (TCA_BPF_FLAGS)
// ---------------------------------------------------------------------------

/// "Direct action" — return value is the verdict, skip actions.
pub const TCA_BPF_FLAG_ACT_DIRECT: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// BPF classifier verdicts (return value when direct-action)
// ---------------------------------------------------------------------------

pub const TC_ACT_OK: i32 = 0;
pub const TC_ACT_RECLASSIFY: i32 = 1;
pub const TC_ACT_SHOT: i32 = 2;
pub const TC_ACT_PIPE: i32 = 3;
pub const TC_ACT_STOLEN: i32 = 4;
pub const TC_ACT_QUEUED: i32 = 5;
pub const TC_ACT_REPEAT: i32 = 6;
pub const TC_ACT_REDIRECT: i32 = 7;
pub const TC_ACT_TRAP: i32 = 8;

// ---------------------------------------------------------------------------
// Filter name strings
// ---------------------------------------------------------------------------

pub const CLS_BPF_NAME: &str = "bpf";
pub const CLS_CGROUP_NAME: &str = "cgroup";

// ---------------------------------------------------------------------------
// Classifier handle composition: TC_H_MAKE(major, minor)
// ---------------------------------------------------------------------------

/// Major handle in the high 16 bits, minor in the low 16 bits.
pub const TC_H_MAJ_MASK: u32 = 0xFFFF_0000;
pub const TC_H_MIN_MASK: u32 = 0x0000_FFFF;
pub const TC_H_MAJ_SHIFT: u32 = 16;

/// Special "unspecified" handle.
pub const TC_H_UNSPEC: u32 = 0;
/// Root qdisc handle.
pub const TC_H_ROOT: u32 = 0xFFFF_FFFF;
/// Ingress qdisc handle.
pub const TC_H_INGRESS: u32 = 0xFFFF_FFF1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_act_direct_single_bit() {
        assert!(TCA_BPF_FLAG_ACT_DIRECT.is_power_of_two());
        assert_eq!(TCA_BPF_FLAG_ACT_DIRECT, 1);
    }

    #[test]
    fn test_verdicts_dense_0_to_8() {
        let v = [
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
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i);
        }
    }

    #[test]
    fn test_filter_names_lowercase() {
        assert_eq!(CLS_BPF_NAME, "bpf");
        assert_eq!(CLS_CGROUP_NAME, "cgroup");
    }

    #[test]
    fn test_handle_mask_split() {
        assert_eq!(TC_H_MAJ_MASK | TC_H_MIN_MASK, 0xFFFF_FFFF);
        assert_eq!(TC_H_MAJ_MASK & TC_H_MIN_MASK, 0);
        assert_eq!(TC_H_MAJ_SHIFT, 16);
        // Extract major: (handle & MAJ) >> SHIFT.
        let h: u32 = 0x1234_5678;
        assert_eq!((h & TC_H_MAJ_MASK) >> TC_H_MAJ_SHIFT, 0x1234);
        assert_eq!(h & TC_H_MIN_MASK, 0x5678);
    }

    #[test]
    fn test_special_handles_distinct() {
        assert_eq!(TC_H_UNSPEC, 0);
        assert_eq!(TC_H_ROOT, u32::MAX);
        // Ingress is just below root.
        assert!(TC_H_INGRESS < TC_H_ROOT);
        assert_eq!(TC_H_ROOT - TC_H_INGRESS, 14);
    }
}

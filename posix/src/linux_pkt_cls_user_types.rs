//! `<linux/pkt_cls.h>` — traffic-control classifier ABI.
//!
//! `tc filter` and `tc action` configure the kernel's packet-classifier
//! pipeline through netlink. The ids here name the well-known qdisc
//! roots (root/ingress/clsact), the verdict enum that classifiers and
//! actions return, and the TCA_* attribute tree common to every
//! classifier type.

// ---------------------------------------------------------------------------
// `TC_H_*` handle constants
// ---------------------------------------------------------------------------

pub const TC_H_MAJ_MASK: u32 = 0xFFFF_0000;
pub const TC_H_MIN_MASK: u32 = 0x0000_FFFF;
pub const TC_H_UNSPEC: u32 = 0;
pub const TC_H_ROOT: u32 = 0xFFFF_FFFF;
pub const TC_H_INGRESS: u32 = 0xFFFF_FFF1;
pub const TC_H_CLSACT: u32 = TC_H_INGRESS;
pub const TC_H_MIN_INGRESS: u32 = 0xFFF2;
pub const TC_H_MIN_EGRESS: u32 = 0xFFF3;

// ---------------------------------------------------------------------------
// Classifier/action return verdicts (`TC_ACT_*`)
// ---------------------------------------------------------------------------

pub const TC_ACT_UNSPEC: i32 = -1;
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
// `TCA_*` top-level filter attributes (`enum`)
// ---------------------------------------------------------------------------

pub const TCA_UNSPEC: u32 = 0;
pub const TCA_KIND: u32 = 1;
pub const TCA_OPTIONS: u32 = 2;
pub const TCA_STATS: u32 = 3;
pub const TCA_XSTATS: u32 = 4;
pub const TCA_RATE: u32 = 5;
pub const TCA_FCNT: u32 = 6;
pub const TCA_STATS2: u32 = 7;
pub const TCA_STAB: u32 = 8;
pub const TCA_PAD: u32 = 9;
pub const TCA_DUMP_INVISIBLE: u32 = 10;
pub const TCA_CHAIN: u32 = 11;
pub const TCA_HW_OFFLOAD: u32 = 12;
pub const TCA_INGRESS_BLOCK: u32 = 13;
pub const TCA_EGRESS_BLOCK: u32 = 14;
pub const TCA_DUMP_FLAGS: u32 = 15;
pub const TCA_EXT_WARN_MSG: u32 = 16;
pub const TCA_MAX: u32 = 16;

// ---------------------------------------------------------------------------
// Classifier `TCA_DUMP_FLAGS` bits
// ---------------------------------------------------------------------------

pub const TCA_DUMP_FLAGS_TERSE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_masks_complementary() {
        // Maj/Min masks split a u32 into two halfwords and don't overlap.
        assert_eq!(TC_H_MAJ_MASK & TC_H_MIN_MASK, 0);
        assert_eq!(TC_H_MAJ_MASK | TC_H_MIN_MASK, 0xFFFF_FFFF);
    }

    #[test]
    fn test_root_and_ingress_sentinels() {
        // ROOT is the all-ones handle; INGRESS and CLSACT share it.
        assert_eq!(TC_H_ROOT, u32::MAX);
        assert_eq!(TC_H_INGRESS, 0xFFFF_FFF1);
        assert_eq!(TC_H_CLSACT, TC_H_INGRESS);
        // Minor handles for clsact's pseudo hooks.
        assert_eq!(TC_H_MIN_INGRESS, 0xFFF2);
        assert_eq!(TC_H_MIN_EGRESS, 0xFFF3);
        assert_eq!(TC_H_UNSPEC, 0);
    }

    #[test]
    fn test_tc_act_dense_minus1_to_8() {
        // -1 is UNSPEC, 0..=8 are the real verdicts.
        assert_eq!(TC_ACT_UNSPEC, -1);
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
    fn test_tca_attrs_dense_0_to_16() {
        let a = [
            TCA_UNSPEC,
            TCA_KIND,
            TCA_OPTIONS,
            TCA_STATS,
            TCA_XSTATS,
            TCA_RATE,
            TCA_FCNT,
            TCA_STATS2,
            TCA_STAB,
            TCA_PAD,
            TCA_DUMP_INVISIBLE,
            TCA_CHAIN,
            TCA_HW_OFFLOAD,
            TCA_INGRESS_BLOCK,
            TCA_EGRESS_BLOCK,
            TCA_DUMP_FLAGS,
            TCA_EXT_WARN_MSG,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(TCA_MAX, 16);
    }

    #[test]
    fn test_dump_flags_terse_is_bit_0() {
        // Only TERSE is defined today; sits at bit 0.
        assert_eq!(TCA_DUMP_FLAGS_TERSE, 1);
        assert!(TCA_DUMP_FLAGS_TERSE.is_power_of_two());
    }
}

//! `<linux/pkt_sched.h>` — Queueing discipline (qdisc) constants.
//!
//! Qdiscs manage packet scheduling and shaping.  These constants
//! define qdisc types, attribute types, and default parameters
//! for common qdiscs (pfifo_fast, fq_codel, etc.).

// ---------------------------------------------------------------------------
// Qdisc TCA message types
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_UNSPEC: u32 = 0;
/// Qdisc kind (string name).
pub const TCA_KIND: u32 = 1;
/// Qdisc options (type-specific).
pub const TCA_OPTIONS: u32 = 2;
/// Qdisc statistics.
pub const TCA_STATS: u32 = 3;
/// Qdisc XSTATS (extended statistics).
pub const TCA_XSTATS: u32 = 4;
/// Qdisc rate estimator.
pub const TCA_RATE: u32 = 5;
/// Forward chain (to another qdisc).
pub const TCA_FCNT: u32 = 6;
/// Statistics v2 (struct gnet_stats_basic).
pub const TCA_STATS2: u32 = 7;
/// Stab (size table).
pub const TCA_STAB: u32 = 8;
/// Chain index.
pub const TCA_CHAIN: u32 = 11;
/// HW offload flag.
pub const TCA_HW_OFFLOAD: u32 = 12;
/// Ingress block.
pub const TCA_INGRESS_BLOCK: u32 = 13;
/// Egress block.
pub const TCA_EGRESS_BLOCK: u32 = 14;

// ---------------------------------------------------------------------------
// Well-known qdisc handles
// ---------------------------------------------------------------------------

/// Root qdisc handle.
pub const TC_H_ROOT: u32 = 0xFFFFFFFF;
/// Ingress qdisc handle.
pub const TC_H_INGRESS: u32 = 0xFFFFFFF1;
/// Clsact qdisc handle.
pub const TC_H_CLSACT: u32 = TC_H_INGRESS;
/// Unspecified handle.
pub const TC_H_UNSPEC: u32 = 0;

// ---------------------------------------------------------------------------
// Handle encoding
// ---------------------------------------------------------------------------

/// Major number shift.
pub const TC_H_MAJ_SHIFT: u32 = 16;
/// Major number mask.
pub const TC_H_MAJ_MASK: u32 = 0xFFFF0000;
/// Minor number mask.
pub const TC_H_MIN_MASK: u32 = 0x0000FFFF;

// ---------------------------------------------------------------------------
// Default qdisc parameters
// ---------------------------------------------------------------------------

/// pfifo_fast number of bands.
pub const PFIFO_FAST_BANDS: u32 = 3;
/// Default tx queue length.
pub const DEFAULT_TX_QUEUE_LEN: u32 = 1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tca_types_distinct() {
        let types = [
            TCA_UNSPEC,
            TCA_KIND,
            TCA_OPTIONS,
            TCA_STATS,
            TCA_XSTATS,
            TCA_RATE,
            TCA_FCNT,
            TCA_STATS2,
            TCA_STAB,
            TCA_CHAIN,
            TCA_HW_OFFLOAD,
            TCA_INGRESS_BLOCK,
            TCA_EGRESS_BLOCK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_unspec_is_zero() {
        assert_eq!(TCA_UNSPEC, 0);
    }

    #[test]
    fn test_root_handle() {
        assert_eq!(TC_H_ROOT, 0xFFFFFFFF);
    }

    #[test]
    fn test_handle_masks() {
        assert_eq!(TC_H_MAJ_MASK | TC_H_MIN_MASK, 0xFFFFFFFF);
        assert_eq!(TC_H_MAJ_MASK & TC_H_MIN_MASK, 0);
    }

    #[test]
    fn test_maj_shift() {
        assert_eq!(TC_H_MAJ_SHIFT, 16);
    }

    #[test]
    fn test_pfifo_bands() {
        assert_eq!(PFIFO_FAST_BANDS, 3);
    }

    #[test]
    fn test_default_txqlen() {
        assert_eq!(DEFAULT_TX_QUEUE_LEN, 1000);
    }
}

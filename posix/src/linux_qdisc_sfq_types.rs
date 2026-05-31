//! `<linux/pkt_sched.h>` — TC SFQ qdisc constants.
//!
//! Traffic control SFQ (Stochastic Fairness Queuing) qdisc
//! constants covering attribute types.

// ---------------------------------------------------------------------------
// TC SFQ attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_SFQ_UNSPEC: u32 = 0;
/// Quantum.
pub const TCA_SFQ_PERTURB_PERIOD: u32 = 1;
/// Limit.
pub const TCA_SFQ_LIMIT: u32 = 2;
/// Total flows.
pub const TCA_SFQ_TOTAL: u32 = 3;
/// Flows.
pub const TCA_SFQ_FLOWS: u32 = 4;
/// Depth.
pub const TCA_SFQ_DEPTH: u32 = 5;
/// Head drop.
pub const TCA_SFQ_HEADDROP: u32 = 6;

// ---------------------------------------------------------------------------
// TC SFQ hash types
// ---------------------------------------------------------------------------

/// Classic hash.
pub const TCA_SFQ_HASH_CLASSIC: u32 = 0;
/// Destination hash.
pub const TCA_SFQ_HASH_DST: u32 = 1;
/// Source hash.
pub const TCA_SFQ_HASH_SRC: u32 = 2;
/// Full hash.
pub const TCA_SFQ_HASH_FULL: u32 = 3;
/// Conntrack hash.
pub const TCA_SFQ_HASH_CTORIGDST: u32 = 4;
/// Conntrack original source.
pub const TCA_SFQ_HASH_CTORIGSRC: u32 = 5;
/// Conntrack reply destination.
pub const TCA_SFQ_HASH_CTREPLDST: u32 = 6;
/// Conntrack reply source.
pub const TCA_SFQ_HASH_CTREPLSRC: u32 = 7;
/// Conntrack nat.
pub const TCA_SFQ_HASH_CTNATCHG: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_SFQ_UNSPEC, TCA_SFQ_PERTURB_PERIOD,
            TCA_SFQ_LIMIT, TCA_SFQ_TOTAL, TCA_SFQ_FLOWS,
            TCA_SFQ_DEPTH, TCA_SFQ_HEADDROP,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_hash_types_distinct() {
        let types = [
            TCA_SFQ_HASH_CLASSIC, TCA_SFQ_HASH_DST,
            TCA_SFQ_HASH_SRC, TCA_SFQ_HASH_FULL,
            TCA_SFQ_HASH_CTORIGDST, TCA_SFQ_HASH_CTORIGSRC,
            TCA_SFQ_HASH_CTREPLDST, TCA_SFQ_HASH_CTREPLSRC,
            TCA_SFQ_HASH_CTNATCHG,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}

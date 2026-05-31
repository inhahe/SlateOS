//! `<linux/pkt_sched.h>` — TC TBF qdisc constants.
//!
//! Traffic control TBF (Token Bucket Filter) qdisc constants
//! covering attribute types.

// ---------------------------------------------------------------------------
// TC TBF attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_TBF_UNSPEC: u32 = 0;
/// Parameters.
pub const TCA_TBF_PARMS: u32 = 1;
/// Rate table.
pub const TCA_TBF_RTAB: u32 = 2;
/// Peak rate table.
pub const TCA_TBF_PTAB: u32 = 3;
/// Rate 64-bit.
pub const TCA_TBF_RATE64: u32 = 4;
/// Peak rate 64-bit.
pub const TCA_TBF_PRATE64: u32 = 5;
/// Burst.
pub const TCA_TBF_BURST: u32 = 6;
/// Peak burst.
pub const TCA_TBF_PBURST: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_TBF_UNSPEC, TCA_TBF_PARMS, TCA_TBF_RTAB,
            TCA_TBF_PTAB, TCA_TBF_RATE64, TCA_TBF_PRATE64,
            TCA_TBF_BURST, TCA_TBF_PBURST,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

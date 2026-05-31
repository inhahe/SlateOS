//! `<linux/pkt_sched.h>` — TC HFSC qdisc constants.
//!
//! Traffic control HFSC (Hierarchical Fair Service Curve) qdisc
//! constants covering attribute types.

// ---------------------------------------------------------------------------
// TC HFSC attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_HFSC_UNSPEC: u32 = 0;
/// Real-time service curve.
pub const TCA_HFSC_RSC: u32 = 1;
/// Fair service curve.
pub const TCA_HFSC_FSC: u32 = 2;
/// Upper limit service curve.
pub const TCA_HFSC_USC: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_HFSC_UNSPEC, TCA_HFSC_RSC,
            TCA_HFSC_FSC, TCA_HFSC_USC,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

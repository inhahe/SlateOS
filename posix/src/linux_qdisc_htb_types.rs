//! `<linux/pkt_sched.h>` — TC HTB qdisc constants.
//!
//! Traffic control HTB (Hierarchical Token Bucket) qdisc constants
//! covering attribute types and class parameters.

// ---------------------------------------------------------------------------
// TC HTB attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_HTB_UNSPEC: u32 = 0;
/// Parameters.
pub const TCA_HTB_PARMS: u32 = 1;
/// Init.
pub const TCA_HTB_INIT: u32 = 2;
/// Class ctokens.
pub const TCA_HTB_CTAB: u32 = 3;
/// Class rtokens.
pub const TCA_HTB_RTAB: u32 = 4;
/// Direct queue len.
pub const TCA_HTB_DIRECT_QLEN: u32 = 5;
/// Rate 64-bit.
pub const TCA_HTB_RATE64: u32 = 6;
/// Ceil rate 64-bit.
pub const TCA_HTB_CEIL64: u32 = 7;
/// Offload.
pub const TCA_HTB_OFFLOAD: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_HTB_UNSPEC,
            TCA_HTB_PARMS,
            TCA_HTB_INIT,
            TCA_HTB_CTAB,
            TCA_HTB_RTAB,
            TCA_HTB_DIRECT_QLEN,
            TCA_HTB_RATE64,
            TCA_HTB_CEIL64,
            TCA_HTB_OFFLOAD,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

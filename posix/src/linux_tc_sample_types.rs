//! `<linux/tc_act/tc_sample.h>` — TC sample action constants.
//!
//! Traffic control sample action constants covering attribute types
//! for packet sampling configuration.

// ---------------------------------------------------------------------------
// TC sample attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_SAMPLE_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_SAMPLE_TM: u32 = 1;
/// Parameters.
pub const TCA_SAMPLE_PARMS: u32 = 2;
/// Rate.
pub const TCA_SAMPLE_RATE: u32 = 3;
/// Truncation size.
pub const TCA_SAMPLE_TRUNC_SIZE: u32 = 4;
/// Psample group.
pub const TCA_SAMPLE_PSAMPLE_GROUP: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_SAMPLE_UNSPEC, TCA_SAMPLE_TM, TCA_SAMPLE_PARMS,
            TCA_SAMPLE_RATE, TCA_SAMPLE_TRUNC_SIZE,
            TCA_SAMPLE_PSAMPLE_GROUP,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

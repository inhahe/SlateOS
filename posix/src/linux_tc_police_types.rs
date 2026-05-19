//! `<linux/tc_act/tc_police.h>` — TC police action constants.
//!
//! Traffic control police action constants covering attribute types
//! for traffic rate limiting and policing.

// ---------------------------------------------------------------------------
// TC police attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_POLICE_UNSPEC: u32 = 0;
/// TBF parameters.
pub const TCA_POLICE_TBF: u32 = 1;
/// Rate table.
pub const TCA_POLICE_RATE: u32 = 2;
/// Peak rate table.
pub const TCA_POLICE_PEAKRATE: u32 = 3;
/// Available size.
pub const TCA_POLICE_AVRATE: u32 = 4;
/// Result.
pub const TCA_POLICE_RESULT: u32 = 5;
/// Timestamp.
pub const TCA_POLICE_TM: u32 = 6;
/// Rate 64-bit.
pub const TCA_POLICE_RATE64: u32 = 7;
/// Peak rate 64-bit.
pub const TCA_POLICE_PEAKRATE64: u32 = 8;
/// Parameters extension.
pub const TCA_POLICE_PKTRATE64: u32 = 9;
/// Packet burst.
pub const TCA_POLICE_PKTBURST64: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_POLICE_UNSPEC, TCA_POLICE_TBF, TCA_POLICE_RATE,
            TCA_POLICE_PEAKRATE, TCA_POLICE_AVRATE,
            TCA_POLICE_RESULT, TCA_POLICE_TM,
            TCA_POLICE_RATE64, TCA_POLICE_PEAKRATE64,
            TCA_POLICE_PKTRATE64, TCA_POLICE_PKTBURST64,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

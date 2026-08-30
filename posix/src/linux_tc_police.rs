//! `<linux/tc_act/tc_police.h>` — TC police (rate limiting) action constants.
//!
//! The police action implements token-bucket rate limiting for TC
//! filters. It can rate-limit, burst-limit, and drop or reclassify
//! packets that exceed configured bandwidth thresholds.

// ---------------------------------------------------------------------------
// Police action results
// ---------------------------------------------------------------------------

/// Packet conforms (within rate).
pub const TC_POLICE_OK: i32 = 0;
/// Packet exceeds rate — reclassify.
pub const TC_POLICE_RECLASSIFY: i32 = 1;
/// Packet exceeds rate — drop.
pub const TC_POLICE_SHOT: i32 = 2;
/// Pipe to next action.
pub const TC_POLICE_PIPE: i32 = 3;
/// Unspecified action.
pub const TC_POLICE_UNSPEC: i32 = -1;

// ---------------------------------------------------------------------------
// Police netlink attributes
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const TCA_POLICE_UNSPEC: u16 = 0;
/// Timer info.
pub const TCA_POLICE_TBF: u16 = 1;
/// Rate parameters.
pub const TCA_POLICE_RATE: u16 = 2;
/// Peak rate parameters.
pub const TCA_POLICE_PEAKRATE: u16 = 3;
/// Available since counter.
pub const TCA_POLICE_AVRATE: u16 = 4;
/// Result action on exceed.
pub const TCA_POLICE_RESULT: u16 = 5;
/// Timer.
pub const TCA_POLICE_TM: u16 = 6;
/// Padding.
pub const TCA_POLICE_PAD: u16 = 7;
/// Rate64 (for rates > 4 Gbps).
pub const TCA_POLICE_RATE64: u16 = 8;
/// Peak rate64.
pub const TCA_POLICE_PEAKRATE64: u16 = 9;
/// PKT rate (packets per second).
pub const TCA_POLICE_PKTRATE64: u16 = 10;
/// PKT burst.
pub const TCA_POLICE_PKTBURST64: u16 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_results_distinct() {
        let results = [
            TC_POLICE_UNSPEC,
            TC_POLICE_OK,
            TC_POLICE_RECLASSIFY,
            TC_POLICE_SHOT,
            TC_POLICE_PIPE,
        ];
        for i in 0..results.len() {
            for j in (i + 1)..results.len() {
                assert_ne!(results[i], results[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_POLICE_UNSPEC,
            TCA_POLICE_TBF,
            TCA_POLICE_RATE,
            TCA_POLICE_PEAKRATE,
            TCA_POLICE_AVRATE,
            TCA_POLICE_RESULT,
            TCA_POLICE_TM,
            TCA_POLICE_PAD,
            TCA_POLICE_RATE64,
            TCA_POLICE_PEAKRATE64,
            TCA_POLICE_PKTRATE64,
            TCA_POLICE_PKTBURST64,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

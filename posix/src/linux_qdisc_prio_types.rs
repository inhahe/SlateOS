//! `<linux/pkt_sched.h>` — TC PRIO/MQ qdisc constants.
//!
//! Traffic control PRIO and MQ qdisc constants covering
//! attribute types and related multi-queue definitions.

// ---------------------------------------------------------------------------
// TC PRIO attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_PRIO_UNSPEC: u32 = 0;
/// Multi-queue mode.
pub const TCA_PRIO_MQ: u32 = 1;

// ---------------------------------------------------------------------------
// TC MQ attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_MQ_UNSPEC: u32 = 0;

// ---------------------------------------------------------------------------
// TC MQPRIO attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_MQPRIO_UNSPEC: u32 = 0;
/// Mode.
pub const TCA_MQPRIO_MODE: u32 = 1;
/// Shaper.
pub const TCA_MQPRIO_SHAPER: u32 = 2;
/// Min rate.
pub const TCA_MQPRIO_MIN_RATE64: u32 = 3;
/// Max rate.
pub const TCA_MQPRIO_MAX_RATE64: u32 = 4;
/// TC entry.
pub const TCA_MQPRIO_TC_ENTRY: u32 = 5;

// ---------------------------------------------------------------------------
// TC MQPRIO modes
// ---------------------------------------------------------------------------

/// DCB mode.
pub const TC_MQPRIO_MODE_DCB: u32 = 0;
/// Channel mode.
pub const TC_MQPRIO_MODE_CHANNEL: u32 = 1;

// ---------------------------------------------------------------------------
// TC MQPRIO shapers
// ---------------------------------------------------------------------------

/// DCB shaper.
pub const TC_MQPRIO_SHAPER_DCB: u32 = 0;
/// BW rate.
pub const TC_MQPRIO_SHAPER_BW_RATE: u32 = 1;

// ---------------------------------------------------------------------------
// TC priority bands
// ---------------------------------------------------------------------------

/// Default number of priority bands.
pub const TC_PRIO_MAX: u32 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mqprio_attrs_distinct() {
        let attrs = [
            TCA_MQPRIO_UNSPEC,
            TCA_MQPRIO_MODE,
            TCA_MQPRIO_SHAPER,
            TCA_MQPRIO_MIN_RATE64,
            TCA_MQPRIO_MAX_RATE64,
            TCA_MQPRIO_TC_ENTRY,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_modes_distinct() {
        assert_ne!(TC_MQPRIO_MODE_DCB, TC_MQPRIO_MODE_CHANNEL);
    }

    #[test]
    fn test_shapers_distinct() {
        assert_ne!(TC_MQPRIO_SHAPER_DCB, TC_MQPRIO_SHAPER_BW_RATE);
    }

    #[test]
    fn test_prio_max() {
        assert_eq!(TC_PRIO_MAX, 15);
    }
}

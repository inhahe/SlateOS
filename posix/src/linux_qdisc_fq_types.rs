//! `<linux/pkt_sched.h>` — TC FQ qdisc constants.
//!
//! Traffic control FQ (Fair Queuing) qdisc constants covering
//! attribute types for per-flow scheduling.

// ---------------------------------------------------------------------------
// TC FQ attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_FQ_UNSPEC: u32 = 0;
/// Limit.
pub const TCA_FQ_PLIMIT: u32 = 1;
/// Flow limit.
pub const TCA_FQ_FLOW_PLIMIT: u32 = 2;
/// Quantum.
pub const TCA_FQ_QUANTUM: u32 = 3;
/// Initial quantum.
pub const TCA_FQ_INITIAL_QUANTUM: u32 = 4;
/// Rate enable.
pub const TCA_FQ_RATE_ENABLE: u32 = 5;
/// Flow default rate.
pub const TCA_FQ_FLOW_DEFAULT_RATE: u32 = 6;
/// Flow max rate.
pub const TCA_FQ_FLOW_MAX_RATE: u32 = 7;
/// Buckets log.
pub const TCA_FQ_BUCKETS_LOG: u32 = 8;
/// Flow refill delay.
pub const TCA_FQ_FLOW_REFILL_DELAY: u32 = 9;
/// Orphan mask.
pub const TCA_FQ_ORPHAN_MASK: u32 = 10;
/// Low rate threshold.
pub const TCA_FQ_LOW_RATE_THRESHOLD: u32 = 11;
/// CE threshold.
pub const TCA_FQ_CE_THRESHOLD: u32 = 12;
/// Timer slack.
pub const TCA_FQ_TIMER_SLACK: u32 = 13;
/// Horizon.
pub const TCA_FQ_HORIZON: u32 = 14;
/// Horizon drop.
pub const TCA_FQ_HORIZON_DROP: u32 = 15;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_FQ_UNSPEC,
            TCA_FQ_PLIMIT,
            TCA_FQ_FLOW_PLIMIT,
            TCA_FQ_QUANTUM,
            TCA_FQ_INITIAL_QUANTUM,
            TCA_FQ_RATE_ENABLE,
            TCA_FQ_FLOW_DEFAULT_RATE,
            TCA_FQ_FLOW_MAX_RATE,
            TCA_FQ_BUCKETS_LOG,
            TCA_FQ_FLOW_REFILL_DELAY,
            TCA_FQ_ORPHAN_MASK,
            TCA_FQ_LOW_RATE_THRESHOLD,
            TCA_FQ_CE_THRESHOLD,
            TCA_FQ_TIMER_SLACK,
            TCA_FQ_HORIZON,
            TCA_FQ_HORIZON_DROP,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

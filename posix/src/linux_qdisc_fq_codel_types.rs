//! `<linux/pkt_sched.h>` — TC FQ_CoDel qdisc constants.
//!
//! Traffic control FQ_CoDel (Fair Queuing Controlled Delay) qdisc
//! constants covering attribute types.

// ---------------------------------------------------------------------------
// TC FQ_CoDel attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_FQ_CODEL_UNSPEC: u32 = 0;
/// Target delay.
pub const TCA_FQ_CODEL_TARGET: u32 = 1;
/// Limit.
pub const TCA_FQ_CODEL_LIMIT: u32 = 2;
/// Interval.
pub const TCA_FQ_CODEL_INTERVAL: u32 = 3;
/// ECN.
pub const TCA_FQ_CODEL_ECN: u32 = 4;
/// Flows.
pub const TCA_FQ_CODEL_FLOWS: u32 = 5;
/// Quantum.
pub const TCA_FQ_CODEL_QUANTUM: u32 = 6;
/// CE threshold.
pub const TCA_FQ_CODEL_CE_THRESHOLD: u32 = 7;
/// Drop batch size.
pub const TCA_FQ_CODEL_DROP_BATCH_SIZE: u32 = 8;
/// Memory limit.
pub const TCA_FQ_CODEL_MEMORY_LIMIT: u32 = 9;

// ---------------------------------------------------------------------------
// TC FQ_CoDel extended stats attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_FQ_CODEL_XSTATS_QDISC: u32 = 0;
/// Class stats.
pub const TCA_FQ_CODEL_XSTATS_CLASS: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_FQ_CODEL_UNSPEC, TCA_FQ_CODEL_TARGET,
            TCA_FQ_CODEL_LIMIT, TCA_FQ_CODEL_INTERVAL,
            TCA_FQ_CODEL_ECN, TCA_FQ_CODEL_FLOWS,
            TCA_FQ_CODEL_QUANTUM, TCA_FQ_CODEL_CE_THRESHOLD,
            TCA_FQ_CODEL_DROP_BATCH_SIZE, TCA_FQ_CODEL_MEMORY_LIMIT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_xstats_distinct() {
        assert_ne!(TCA_FQ_CODEL_XSTATS_QDISC, TCA_FQ_CODEL_XSTATS_CLASS);
    }
}

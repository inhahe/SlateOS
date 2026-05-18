//! `<linux/blk-cgroup.h>` — block I/O cgroup controller constants.
//!
//! The blkio cgroup controller throttles and accounts block I/O per
//! cgroup. It supports two policies: weight-based proportional share
//! (BFQ) and absolute bandwidth/IOPS limits. Used by container
//! runtimes and systemd to isolate I/O between services.

// ---------------------------------------------------------------------------
// Block cgroup policy types
// ---------------------------------------------------------------------------

/// No policy (direct I/O, no throttling).
pub const BLKCG_POLICY_NONE: u32 = 0;
/// Proportional weight policy (CFQ/BFQ).
pub const BLKCG_POLICY_PROP: u32 = 1;
/// Throttle policy (bandwidth/IOPS limits).
pub const BLKCG_POLICY_THROTL: u32 = 2;
/// I/O cost model policy (cost-based).
pub const BLKCG_POLICY_IOCOST: u32 = 3;
/// I/O latency policy (latency targets).
pub const BLKCG_POLICY_IOLATENCY: u32 = 4;

// ---------------------------------------------------------------------------
// Weight range (proportional policy)
// ---------------------------------------------------------------------------

/// Minimum I/O weight.
pub const BLKCG_WEIGHT_MIN: u32 = 10;
/// Default I/O weight.
pub const BLKCG_WEIGHT_DEFAULT: u32 = 100;
/// Maximum I/O weight.
pub const BLKCG_WEIGHT_MAX: u32 = 10000;

// ---------------------------------------------------------------------------
// Throttle limits
// ---------------------------------------------------------------------------

/// Maximum bytes per second (unlimited).
pub const BLKCG_BPS_MAX: u64 = u64::MAX;
/// Maximum IOPS (unlimited).
pub const BLKCG_IOPS_MAX: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// I/O latency controller targets
// ---------------------------------------------------------------------------

/// Default latency target: 75 milliseconds.
pub const BLKCG_IOLATENCY_TARGET_DEFAULT_US: u64 = 75_000;
/// Minimum latency target: 1 millisecond.
pub const BLKCG_IOLATENCY_TARGET_MIN_US: u64 = 1_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policies_distinct() {
        let policies = [
            BLKCG_POLICY_NONE, BLKCG_POLICY_PROP,
            BLKCG_POLICY_THROTL, BLKCG_POLICY_IOCOST,
            BLKCG_POLICY_IOLATENCY,
        ];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_weight_range() {
        assert!(BLKCG_WEIGHT_MIN < BLKCG_WEIGHT_DEFAULT);
        assert!(BLKCG_WEIGHT_DEFAULT < BLKCG_WEIGHT_MAX);
    }

    #[test]
    fn test_throttle_max() {
        assert_eq!(BLKCG_BPS_MAX, u64::MAX);
        assert_eq!(BLKCG_IOPS_MAX, u32::MAX);
    }

    #[test]
    fn test_latency_targets() {
        assert!(BLKCG_IOLATENCY_TARGET_MIN_US < BLKCG_IOLATENCY_TARGET_DEFAULT_US);
        assert!(BLKCG_IOLATENCY_TARGET_MIN_US > 0);
    }
}

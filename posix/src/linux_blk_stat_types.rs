//! `<linux/blk-stat.h>` — block layer statistics tracking constants.
//!
//! The block stat framework collects per-request timing statistics
//! for I/O scheduling, latency monitoring, and write-back throttling
//! decisions. Each request queue can have callback groups that
//! bucket requests by type and track completion latencies.

// ---------------------------------------------------------------------------
// Block stat operation types
// ---------------------------------------------------------------------------

/// Read operation.
pub const BLK_STAT_READ: u32 = 0;
/// Write operation.
pub const BLK_STAT_WRITE: u32 = 1;
/// Number of stat operation types.
pub const BLK_STAT_NR: u32 = 2;

// ---------------------------------------------------------------------------
// Block stat callback bucket thresholds
// ---------------------------------------------------------------------------

/// Number of latency buckets (powers-of-two ranges).
pub const BLK_STAT_NR_BUCKETS: u32 = 16;
/// Minimum latency tracked (1 microsecond in nanoseconds).
pub const BLK_STAT_MIN_LATENCY_NS: u64 = 1_000;
/// Maximum latency tracked (roughly 33 seconds in nanoseconds).
pub const BLK_STAT_MAX_LATENCY_NS: u64 = 1_000_000_000 * 33;

// ---------------------------------------------------------------------------
// I/O latency thresholds (for I/O schedulers, in microseconds)
// ---------------------------------------------------------------------------

/// Good read latency target (500 microseconds).
pub const BLK_LAT_TARGET_READ_US: u64 = 500;
/// Good write latency target (2 milliseconds).
pub const BLK_LAT_TARGET_WRITE_US: u64 = 2_000;
/// Warning latency threshold (10 milliseconds).
pub const BLK_LAT_WARNING_US: u64 = 10_000;
/// Critical latency threshold (100 milliseconds).
pub const BLK_LAT_CRITICAL_US: u64 = 100_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stat_ops_distinct() {
        assert_ne!(BLK_STAT_READ, BLK_STAT_WRITE);
    }

    #[test]
    fn test_stat_nr() {
        assert_eq!(BLK_STAT_NR, 2);
    }

    #[test]
    fn test_latency_range() {
        assert!(BLK_STAT_MIN_LATENCY_NS < BLK_STAT_MAX_LATENCY_NS);
    }

    #[test]
    fn test_latency_targets_ordered() {
        assert!(BLK_LAT_TARGET_READ_US < BLK_LAT_TARGET_WRITE_US);
        assert!(BLK_LAT_TARGET_WRITE_US < BLK_LAT_WARNING_US);
        assert!(BLK_LAT_WARNING_US < BLK_LAT_CRITICAL_US);
    }

    #[test]
    fn test_nr_buckets() {
        assert!(BLK_STAT_NR_BUCKETS > 0);
        assert!(BLK_STAT_NR_BUCKETS.is_power_of_two());
    }
}

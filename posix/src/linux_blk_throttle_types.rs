//! `<linux/blk-throttle.h>` — Block I/O throttling constants.
//!
//! Block throttling limits the I/O rate (IOPS and bandwidth) for
//! cgroups. It works at the block layer, throttling bios before
//! they reach the I/O scheduler. This ensures fair I/O access
//! between containers and prevents any single workload from
//! monopolizing disk bandwidth. Throttling supports both read
//! and write limits, and can be configured per-device per-cgroup.

// ---------------------------------------------------------------------------
// Throttle limit types
// ---------------------------------------------------------------------------

/// Bytes per second limit (bandwidth).
pub const BLK_THROTL_BPS: u32 = 0;
/// I/O operations per second limit.
pub const BLK_THROTL_IOPS: u32 = 1;

// ---------------------------------------------------------------------------
// Throttle direction
// ---------------------------------------------------------------------------

/// Read direction.
pub const BLK_THROTL_READ: u32 = 0;
/// Write direction.
pub const BLK_THROTL_WRITE: u32 = 1;

// ---------------------------------------------------------------------------
// Throttle special limit values
// ---------------------------------------------------------------------------

/// Unlimited (no throttling).
pub const BLK_THROTL_UNLIMITED: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Throttle states
// ---------------------------------------------------------------------------

/// Not throttled (within limits).
pub const BLK_THROTL_STATE_OK: u32 = 0;
/// Throttled (waiting for tokens).
pub const BLK_THROTL_STATE_THROTTLED: u32 = 1;

// ---------------------------------------------------------------------------
// Throttle dispatch quantum
// ---------------------------------------------------------------------------

/// Default dispatch quantum (how many bios to release at once).
pub const BLK_THROTL_QUANTUM: u32 = 8;
/// Token refill interval in milliseconds.
pub const BLK_THROTL_REFILL_MS: u32 = 10;
/// Maximum tokens that can accumulate (burst size, 1 second worth).
pub const BLK_THROTL_MAX_BURST_MS: u32 = 1000;

// ---------------------------------------------------------------------------
// I/O latency target (blk-iolatency controller)
// ---------------------------------------------------------------------------

/// No latency target (disabled).
pub const BLK_IOLAT_TARGET_NONE: u32 = 0;
/// Latency target calculation window in milliseconds.
pub const BLK_IOLAT_WINDOW_MS: u32 = 100;
/// Number of latency percentile buckets.
pub const BLK_IOLAT_NR_BUCKETS: u32 = 31;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limit_types_distinct() {
        assert_ne!(BLK_THROTL_BPS, BLK_THROTL_IOPS);
    }

    #[test]
    fn test_directions_distinct() {
        assert_ne!(BLK_THROTL_READ, BLK_THROTL_WRITE);
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(BLK_THROTL_STATE_OK, BLK_THROTL_STATE_THROTTLED);
    }

    #[test]
    fn test_unlimited_is_max() {
        assert_eq!(BLK_THROTL_UNLIMITED, u64::MAX);
    }

    #[test]
    fn test_dispatch_params() {
        assert!(BLK_THROTL_QUANTUM > 0);
        assert!(BLK_THROTL_REFILL_MS > 0);
        assert!(BLK_THROTL_MAX_BURST_MS > BLK_THROTL_REFILL_MS);
    }

    #[test]
    fn test_iolat_params() {
        assert!(BLK_IOLAT_WINDOW_MS > 0);
        assert!(BLK_IOLAT_NR_BUCKETS > 0);
    }
}

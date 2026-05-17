//! `<linux/blk-cgroup.h>` — Block I/O cgroup controller constants.
//!
//! The blkio/io cgroup controller limits and accounts block device
//! I/O. In cgroup v1 it's called "blkio"; in v2 it's "io". It can
//! throttle read/write IOPS and bandwidth per device, apply weight-
//! based proportional I/O scheduling (BFQ integration), and track
//! I/O statistics. Used by containers to prevent I/O-hungry workloads
//! from starving others.

// ---------------------------------------------------------------------------
// I/O control policies
// ---------------------------------------------------------------------------

/// No I/O policy (unregulated).
pub const BLKIO_POLICY_NONE: u32 = 0;
/// Proportional weight-based policy.
pub const BLKIO_POLICY_WEIGHT: u32 = 1;
/// Throttling (IOPS/bandwidth limits).
pub const BLKIO_POLICY_THROTTLE: u32 = 2;

// ---------------------------------------------------------------------------
// I/O weight limits
// ---------------------------------------------------------------------------

/// Minimum I/O weight.
pub const BLKIO_WEIGHT_MIN: u32 = 1;
/// Default I/O weight.
pub const BLKIO_WEIGHT_DEFAULT: u32 = 100;
/// Maximum I/O weight.
pub const BLKIO_WEIGHT_MAX: u32 = 10000;

// ---------------------------------------------------------------------------
// I/O operation types (for statistics)
// ---------------------------------------------------------------------------

/// Read operation.
pub const BLKIO_OP_READ: u32 = 0;
/// Write operation.
pub const BLKIO_OP_WRITE: u32 = 1;
/// Sync operation.
pub const BLKIO_OP_SYNC: u32 = 2;
/// Async operation.
pub const BLKIO_OP_ASYNC: u32 = 3;
/// Discard/trim operation.
pub const BLKIO_OP_DISCARD: u32 = 4;

// ---------------------------------------------------------------------------
// I/O throttle types (what to limit)
// ---------------------------------------------------------------------------

/// Limit read bytes per second.
pub const BLKIO_THROTL_READ_BPS: u32 = 0;
/// Limit write bytes per second.
pub const BLKIO_THROTL_WRITE_BPS: u32 = 1;
/// Limit read IOPS.
pub const BLKIO_THROTL_READ_IOPS: u32 = 2;
/// Limit write IOPS.
pub const BLKIO_THROTL_WRITE_IOPS: u32 = 3;

// ---------------------------------------------------------------------------
// I/O latency targets (cgroup v2 io.latency)
// ---------------------------------------------------------------------------

/// Latency target for high-priority workloads (in microseconds).
pub const BLKIO_LAT_TARGET_DEFAULT_US: u32 = 0;
/// Maximum latency percentile tracking window (ms).
pub const BLKIO_LAT_WINDOW_MS: u32 = 100;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policies_distinct() {
        let policies = [BLKIO_POLICY_NONE, BLKIO_POLICY_WEIGHT, BLKIO_POLICY_THROTTLE];
        for i in 0..policies.len() {
            for j in (i + 1)..policies.len() {
                assert_ne!(policies[i], policies[j]);
            }
        }
    }

    #[test]
    fn test_weight_range() {
        assert!(BLKIO_WEIGHT_MIN < BLKIO_WEIGHT_DEFAULT);
        assert!(BLKIO_WEIGHT_DEFAULT < BLKIO_WEIGHT_MAX);
    }

    #[test]
    fn test_op_types_distinct() {
        let ops = [
            BLKIO_OP_READ, BLKIO_OP_WRITE, BLKIO_OP_SYNC,
            BLKIO_OP_ASYNC, BLKIO_OP_DISCARD,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_throttle_types_distinct() {
        let types = [
            BLKIO_THROTL_READ_BPS, BLKIO_THROTL_WRITE_BPS,
            BLKIO_THROTL_READ_IOPS, BLKIO_THROTL_WRITE_IOPS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}

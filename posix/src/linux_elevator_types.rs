//! `<linux/elevator.h>` — I/O scheduler (elevator) constants.
//!
//! I/O schedulers reorder and merge block I/O requests to optimize
//! throughput and latency. Linux supports multiple schedulers:
//! mq-deadline (latency-focused), BFQ (fairness + latency, like CFS
//! for I/O), kyber (lightweight for fast NVMe), and none (FIFO, for
//! devices with their own scheduler like NVMe). The scheduler is
//! selected per block device via /sys/block/<dev>/queue/scheduler.

// ---------------------------------------------------------------------------
// I/O scheduler types
// ---------------------------------------------------------------------------

/// No I/O scheduler (FIFO, requests go directly to driver).
pub const ELEVATOR_NONE: u32 = 0;
/// mq-deadline scheduler (deadline-based, good for rotational).
pub const ELEVATOR_MQ_DEADLINE: u32 = 1;
/// BFQ scheduler (Budget Fair Queueing, per-process fairness).
pub const ELEVATOR_BFQ: u32 = 2;
/// Kyber scheduler (lightweight, latency targets for NVMe).
pub const ELEVATOR_KYBER: u32 = 3;

// ---------------------------------------------------------------------------
// I/O priority classes (ioprio, used by schedulers)
// ---------------------------------------------------------------------------

/// Real-time I/O priority class (highest).
pub const IOPRIO_CLASS_RT: u32 = 1;
/// Best-effort I/O priority class (default).
pub const IOPRIO_CLASS_BE: u32 = 2;
/// Idle I/O priority class (only when disk is idle).
pub const IOPRIO_CLASS_IDLE: u32 = 3;
/// No I/O priority set (inherit from CPU priority).
pub const IOPRIO_CLASS_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// I/O priority levels within a class
// ---------------------------------------------------------------------------

/// Highest priority within class (0).
pub const IOPRIO_LEVEL_HIGH: u32 = 0;
/// Default priority within class (4).
pub const IOPRIO_LEVEL_DEFAULT: u32 = 4;
/// Lowest priority within class (7).
pub const IOPRIO_LEVEL_LOW: u32 = 7;
/// Number of priority levels per class.
pub const IOPRIO_NR_LEVELS: u32 = 8;

// ---------------------------------------------------------------------------
// mq-deadline tunables
// ---------------------------------------------------------------------------

/// Read deadline in milliseconds (default).
pub const DEADLINE_READ_EXPIRE_MS: u32 = 500;
/// Write deadline in milliseconds (default).
pub const DEADLINE_WRITE_EXPIRE_MS: u32 = 5000;
/// Batch size for reads before switching to writes.
pub const DEADLINE_READS_BATCH: u32 = 16;

// ---------------------------------------------------------------------------
// BFQ weight range
// ---------------------------------------------------------------------------

/// Minimum BFQ weight.
pub const BFQ_WEIGHT_MIN: u32 = 1;
/// Default BFQ weight.
pub const BFQ_WEIGHT_DEFAULT: u32 = 100;
/// Maximum BFQ weight.
pub const BFQ_WEIGHT_MAX: u32 = 1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_types_distinct() {
        let types = [
            ELEVATOR_NONE, ELEVATOR_MQ_DEADLINE,
            ELEVATOR_BFQ, ELEVATOR_KYBER,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_ioprio_classes_distinct() {
        let classes = [
            IOPRIO_CLASS_NONE, IOPRIO_CLASS_RT,
            IOPRIO_CLASS_BE, IOPRIO_CLASS_IDLE,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_ioprio_levels() {
        assert!(IOPRIO_LEVEL_HIGH < IOPRIO_LEVEL_DEFAULT);
        assert!(IOPRIO_LEVEL_DEFAULT < IOPRIO_LEVEL_LOW);
        assert!(IOPRIO_LEVEL_LOW < IOPRIO_NR_LEVELS);
    }

    #[test]
    fn test_bfq_weight_range() {
        assert!(BFQ_WEIGHT_MIN < BFQ_WEIGHT_DEFAULT);
        assert!(BFQ_WEIGHT_DEFAULT < BFQ_WEIGHT_MAX);
    }
}

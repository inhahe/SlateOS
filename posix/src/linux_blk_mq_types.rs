//! `<linux/blk-mq.h>` — Block multi-queue (blk-mq) constants.
//!
//! blk-mq is the modern Linux block I/O path that replaces the
//! single-queue block layer. It provides per-CPU software submission
//! queues mapped to hardware dispatch queues, enabling parallel I/O
//! processing across CPUs without a single-lock bottleneck. blk-mq
//! is required for NVMe, virtio-blk, and all modern high-IOPS
//! storage. It supports I/O schedulers (mq-deadline, BFQ, kyber)
//! plugged between software and hardware queues.

// ---------------------------------------------------------------------------
// blk-mq request flags (cmd_flags)
// ---------------------------------------------------------------------------

/// Read operation.
pub const BLK_MQ_REQ_READ: u32 = 0;
/// Write operation.
pub const BLK_MQ_REQ_WRITE: u32 = 1 << 0;
/// Request is synchronous (caller waits).
pub const BLK_MQ_REQ_SYNC: u32 = 1 << 1;
/// Request is for metadata.
pub const BLK_MQ_REQ_META: u32 = 1 << 2;
/// FUA (Force Unit Access, bypass volatile cache).
pub const BLK_MQ_REQ_FUA: u32 = 1 << 3;
/// Preflush (flush cache before I/O).
pub const BLK_MQ_REQ_PREFLUSH: u32 = 1 << 4;
/// Rahead (readahead).
pub const BLK_MQ_REQ_RAHEAD: u32 = 1 << 5;
/// No merge (do not merge this request with others).
pub const BLK_MQ_REQ_NOMERGE: u32 = 1 << 6;
/// Idle (low-priority background I/O).
pub const BLK_MQ_REQ_IDLE: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// blk-mq I/O schedulers
// ---------------------------------------------------------------------------

/// No scheduler (direct dispatch to hardware).
pub const BLK_MQ_SCHED_NONE: u32 = 0;
/// mq-deadline scheduler (latency-focused).
pub const BLK_MQ_SCHED_DEADLINE: u32 = 1;
/// BFQ scheduler (bandwidth/fairness-focused).
pub const BLK_MQ_SCHED_BFQ: u32 = 2;
/// Kyber scheduler (latency target based).
pub const BLK_MQ_SCHED_KYBER: u32 = 3;

// ---------------------------------------------------------------------------
// blk-mq tag allocation types
// ---------------------------------------------------------------------------

/// Normal tag allocation (for regular I/O).
pub const BLK_MQ_TAG_NORMAL: u32 = 0;
/// Reserved tag allocation (for internal/urgent commands).
pub const BLK_MQ_TAG_RESERVED: u32 = 1;

// ---------------------------------------------------------------------------
// blk-mq hardware queue flags
// ---------------------------------------------------------------------------

/// Queue should use polling (not interrupts) for completion.
pub const BLK_MQ_F_SHOULD_MERGE: u32 = 1 << 0;
/// Tag set is shared across multiple queues.
pub const BLK_MQ_F_TAG_QUEUE_SHARED: u32 = 1 << 1;
/// Use managed IRQ affinity for this queue.
pub const BLK_MQ_F_MANAGED_IRQ: u32 = 1 << 2;
/// Queue supports polling mode.
pub const BLK_MQ_F_POLL: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// blk-mq request states
// ---------------------------------------------------------------------------

/// Request is idle (in tag pool).
pub const MQ_RQ_IDLE: u32 = 0;
/// Request is in flight (submitted to driver).
pub const MQ_RQ_IN_FLIGHT: u32 = 1;
/// Request is complete (driver reported completion).
pub const MQ_RQ_COMPLETE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_req_flags_no_overlap() {
        let flags = [
            BLK_MQ_REQ_WRITE,
            BLK_MQ_REQ_SYNC,
            BLK_MQ_REQ_META,
            BLK_MQ_REQ_FUA,
            BLK_MQ_REQ_PREFLUSH,
            BLK_MQ_REQ_RAHEAD,
            BLK_MQ_REQ_NOMERGE,
            BLK_MQ_REQ_IDLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_schedulers_distinct() {
        let scheds = [
            BLK_MQ_SCHED_NONE,
            BLK_MQ_SCHED_DEADLINE,
            BLK_MQ_SCHED_BFQ,
            BLK_MQ_SCHED_KYBER,
        ];
        for i in 0..scheds.len() {
            for j in (i + 1)..scheds.len() {
                assert_ne!(scheds[i], scheds[j]);
            }
        }
    }

    #[test]
    fn test_tag_types_distinct() {
        assert_ne!(BLK_MQ_TAG_NORMAL, BLK_MQ_TAG_RESERVED);
    }

    #[test]
    fn test_hw_queue_flags_no_overlap() {
        let flags = [
            BLK_MQ_F_SHOULD_MERGE,
            BLK_MQ_F_TAG_QUEUE_SHARED,
            BLK_MQ_F_MANAGED_IRQ,
            BLK_MQ_F_POLL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_request_states_distinct() {
        let states = [MQ_RQ_IDLE, MQ_RQ_IN_FLIGHT, MQ_RQ_COMPLETE];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}

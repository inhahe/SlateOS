//! `<linux/blk-mq.h>` — Block multi-queue (blk-mq) constants.
//!
//! blk-mq is the modern Linux block layer that uses per-CPU software
//! queues mapped to hardware dispatch queues. It replaced the legacy
//! single-queue block layer, enabling high IOPS with NVMe and
//! multi-queue SCSI (scsi-mq) devices.

// ---------------------------------------------------------------------------
// Request operation types
// ---------------------------------------------------------------------------

/// Read operation.
pub const REQ_OP_READ: u32 = 0;
/// Write operation.
pub const REQ_OP_WRITE: u32 = 1;
/// Flush (write cache to stable storage).
pub const REQ_OP_FLUSH: u32 = 2;
/// Discard (TRIM/unmap).
pub const REQ_OP_DISCARD: u32 = 3;
/// Secure erase.
pub const REQ_OP_SECURE_ERASE: u32 = 5;
/// Write same (fill range with pattern).
pub const REQ_OP_WRITE_SAME: u32 = 7;
/// Write zeroes.
pub const REQ_OP_WRITE_ZEROES: u32 = 9;
/// Zone reset.
pub const REQ_OP_ZONE_RESET: u32 = 13;
/// Zone open.
pub const REQ_OP_ZONE_OPEN: u32 = 14;
/// Zone close.
pub const REQ_OP_ZONE_CLOSE: u32 = 15;
/// Zone finish.
pub const REQ_OP_ZONE_FINISH: u32 = 16;
/// Zone append.
pub const REQ_OP_ZONE_APPEND: u32 = 17;

// ---------------------------------------------------------------------------
// Request flags (combined with op via bitwise OR)
// ---------------------------------------------------------------------------

/// Request is synchronous.
pub const REQ_SYNC: u32 = 1 << 12;
/// Metadata I/O.
pub const REQ_META: u32 = 1 << 13;
/// Request has higher priority.
pub const REQ_PRIO: u32 = 1 << 14;
/// Don't merge this request.
pub const REQ_NOMERGE: u32 = 1 << 15;
/// Idle I/O priority.
pub const REQ_IDLE: u32 = 1 << 16;
/// Integrity payload attached.
pub const REQ_INTEGRITY: u32 = 1 << 17;
/// Force Unit Access (bypass cache).
pub const REQ_FUA: u32 = 1 << 18;
/// Request preflush.
pub const REQ_PREFLUSH: u32 = 1 << 19;
/// Hint: short-lived data.
pub const REQ_RAHEAD: u32 = 1 << 20;
/// Background operation.
pub const REQ_BACKGROUND: u32 = 1 << 21;
/// Don't wait for allocation.
pub const REQ_NOWAIT: u32 = 1 << 22;

// ---------------------------------------------------------------------------
// Hardware queue flags
// ---------------------------------------------------------------------------

/// Tag set shared across queues.
pub const BLK_MQ_F_TAG_SHARED: u32 = 1 << 1;
/// Blocking allowed in driver.
pub const BLK_MQ_F_BLOCKING: u32 = 1 << 5;
/// No scheduler (passthrough).
pub const BLK_MQ_F_NO_SCHED: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Queue limits
// ---------------------------------------------------------------------------

/// Default queue depth.
pub const BLK_MQ_DEFAULT_QUEUE_DEPTH: u32 = 256;
/// Maximum hardware queues.
pub const BLK_MQ_MAX_HW_QUEUES: u32 = 256;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_req_ops_distinct() {
        let ops = [
            REQ_OP_READ,
            REQ_OP_WRITE,
            REQ_OP_FLUSH,
            REQ_OP_DISCARD,
            REQ_OP_SECURE_ERASE,
            REQ_OP_WRITE_SAME,
            REQ_OP_WRITE_ZEROES,
            REQ_OP_ZONE_RESET,
            REQ_OP_ZONE_OPEN,
            REQ_OP_ZONE_CLOSE,
            REQ_OP_ZONE_FINISH,
            REQ_OP_ZONE_APPEND,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_req_flags_no_overlap() {
        let flags = [
            REQ_SYNC,
            REQ_META,
            REQ_PRIO,
            REQ_NOMERGE,
            REQ_IDLE,
            REQ_INTEGRITY,
            REQ_FUA,
            REQ_PREFLUSH,
            REQ_RAHEAD,
            REQ_BACKGROUND,
            REQ_NOWAIT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_hq_flags_no_overlap() {
        let flags = [BLK_MQ_F_TAG_SHARED, BLK_MQ_F_BLOCKING, BLK_MQ_F_NO_SCHED];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_queue_limits() {
        assert!(BLK_MQ_DEFAULT_QUEUE_DEPTH > 0);
        assert!(BLK_MQ_MAX_HW_QUEUES > 0);
    }
}

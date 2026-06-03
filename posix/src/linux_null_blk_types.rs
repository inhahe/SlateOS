//! `<linux/null_blk.h>` — Null block device driver constants.
//!
//! null_blk is a kernel module that creates block devices backed by
//! no physical storage. It is used for benchmarking the block layer
//! itself, testing I/O schedulers, verifying filesystem behavior
//! under controlled conditions, and developing blk-mq drivers.
//! It supports configurable parameters: queue depth, number of HW
//! queues, block size, completion mode (softirq/timer/none), and
//! can simulate various device characteristics.

// ---------------------------------------------------------------------------
// Null block completion modes
// ---------------------------------------------------------------------------

/// No-op completion (instant, minimum overhead).
pub const NULL_BLK_COMP_NONE: u32 = 0;
/// Softirq completion (simulates interrupt-driven I/O).
pub const NULL_BLK_COMP_SOFTIRQ: u32 = 1;
/// Timer completion (configurable latency).
pub const NULL_BLK_COMP_TIMER: u32 = 2;

// ---------------------------------------------------------------------------
// Null block I/O modes
// ---------------------------------------------------------------------------

/// Direct I/O mode (no buffering simulation).
pub const NULL_BLK_IO_DIRECT: u32 = 0;
/// Bio-based submission (legacy path).
pub const NULL_BLK_IO_BIO: u32 = 1;
/// Request-based submission (blk-mq path).
pub const NULL_BLK_IO_RQ: u32 = 2;

// ---------------------------------------------------------------------------
// Null block queue modes
// ---------------------------------------------------------------------------

/// Single queue mode.
pub const NULL_BLK_Q_SINGLE: u32 = 0;
/// Multi-queue mode (blk-mq).
pub const NULL_BLK_Q_MULTI: u32 = 1;

// ---------------------------------------------------------------------------
// Null block fault injection types
// ---------------------------------------------------------------------------

/// No fault injection.
pub const NULL_BLK_FAULT_NONE: u32 = 0;
/// Inject timeout errors.
pub const NULL_BLK_FAULT_TIMEOUT: u32 = 1;
/// Inject I/O errors.
pub const NULL_BLK_FAULT_IO_ERROR: u32 = 2;
/// Inject requeue (retry) events.
pub const NULL_BLK_FAULT_REQUEUE: u32 = 3;

// ---------------------------------------------------------------------------
// Null block zone model (for zoned device simulation)
// ---------------------------------------------------------------------------

/// None (not a zoned device).
pub const NULL_BLK_ZONED_NONE: u32 = 0;
/// Host-aware zoned model.
pub const NULL_BLK_ZONED_HOST_AWARE: u32 = 1;
/// Host-managed zoned model.
pub const NULL_BLK_ZONED_HOST_MANAGED: u32 = 2;

// ---------------------------------------------------------------------------
// Null block configfs attributes
// ---------------------------------------------------------------------------

/// Block size (bytes).
pub const NULL_BLK_ATTR_BLOCKSIZE: u32 = 0;
/// Device size (MB).
pub const NULL_BLK_ATTR_SIZE: u32 = 1;
/// Queue depth.
pub const NULL_BLK_ATTR_QUEUE_DEPTH: u32 = 2;
/// Number of hardware queues.
pub const NULL_BLK_ATTR_HW_QUEUES: u32 = 3;
/// Completion latency (nanoseconds).
pub const NULL_BLK_ATTR_COMPLETION_NSEC: u32 = 4;
/// IRQ mode.
pub const NULL_BLK_ATTR_IRQ_MODE: u32 = 5;
/// Memory-backed (actually store data in RAM).
pub const NULL_BLK_ATTR_MEMORY_BACKED: u32 = 6;
/// Discard support.
pub const NULL_BLK_ATTR_DISCARD: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_modes_distinct() {
        let modes = [
            NULL_BLK_COMP_NONE,
            NULL_BLK_COMP_SOFTIRQ,
            NULL_BLK_COMP_TIMER,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_io_modes_distinct() {
        let modes = [NULL_BLK_IO_DIRECT, NULL_BLK_IO_BIO, NULL_BLK_IO_RQ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_queue_modes_distinct() {
        assert_ne!(NULL_BLK_Q_SINGLE, NULL_BLK_Q_MULTI);
    }

    #[test]
    fn test_fault_types_distinct() {
        let faults = [
            NULL_BLK_FAULT_NONE,
            NULL_BLK_FAULT_TIMEOUT,
            NULL_BLK_FAULT_IO_ERROR,
            NULL_BLK_FAULT_REQUEUE,
        ];
        for i in 0..faults.len() {
            for j in (i + 1)..faults.len() {
                assert_ne!(faults[i], faults[j]);
            }
        }
    }

    #[test]
    fn test_zone_models_distinct() {
        let models = [
            NULL_BLK_ZONED_NONE,
            NULL_BLK_ZONED_HOST_AWARE,
            NULL_BLK_ZONED_HOST_MANAGED,
        ];
        for i in 0..models.len() {
            for j in (i + 1)..models.len() {
                assert_ne!(models[i], models[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            NULL_BLK_ATTR_BLOCKSIZE,
            NULL_BLK_ATTR_SIZE,
            NULL_BLK_ATTR_QUEUE_DEPTH,
            NULL_BLK_ATTR_HW_QUEUES,
            NULL_BLK_ATTR_COMPLETION_NSEC,
            NULL_BLK_ATTR_IRQ_MODE,
            NULL_BLK_ATTR_MEMORY_BACKED,
            NULL_BLK_ATTR_DISCARD,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}

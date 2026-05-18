//! `<linux/nvme.h>` (queue subset) — NVMe queue configuration constants.
//!
//! NVMe uses paired submission/completion queues for command dispatch.
//! The admin queue (ID 0) handles management commands. I/O queues
//! (IDs 1+) handle data transfer. Modern NVMe devices support many
//! queues for multi-core scalability — typically one queue pair per
//! CPU core.

// ---------------------------------------------------------------------------
// Queue limits
// ---------------------------------------------------------------------------

/// Admin queue ID (always 0).
pub const NVME_AQ_ID: u16 = 0;
/// Default admin queue depth (entries).
pub const NVME_AQ_DEPTH: u16 = 32;
/// Minimum queue depth (2 entries per spec).
pub const NVME_MIN_QUEUE_DEPTH: u16 = 2;
/// Maximum queue depth (64K entries, minus one for head/tail wrap).
pub const NVME_MAX_QUEUE_DEPTH: u32 = 65536;
/// Default I/O queue depth.
pub const NVME_DEFAULT_IO_QUEUE_DEPTH: u16 = 1024;

// ---------------------------------------------------------------------------
// Queue entry sizes
// ---------------------------------------------------------------------------

/// Submission queue entry size (bytes) — always 64.
pub const NVME_SQ_ENTRY_SIZE: u32 = 64;
/// Completion queue entry size (bytes) — always 16.
pub const NVME_CQ_ENTRY_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// Queue priority levels (for weighted round-robin arbitration)
// ---------------------------------------------------------------------------

/// Urgent priority.
pub const NVME_QP_URGENT: u32 = 0;
/// High priority.
pub const NVME_QP_HIGH: u32 = 1;
/// Medium priority.
pub const NVME_QP_MEDIUM: u32 = 2;
/// Low priority.
pub const NVME_QP_LOW: u32 = 3;

// ---------------------------------------------------------------------------
// Completion queue phase bit
// ---------------------------------------------------------------------------

/// Phase tag bit position in completion status word.
pub const NVME_CQ_PHASE_BIT: u32 = 0;
/// Phase tag mask.
pub const NVME_CQ_PHASE_MASK: u32 = 1 << NVME_CQ_PHASE_BIT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_admin_queue_id() {
        assert_eq!(NVME_AQ_ID, 0);
    }

    #[test]
    fn test_queue_depth_limits() {
        assert!(NVME_MIN_QUEUE_DEPTH < NVME_AQ_DEPTH);
        assert!((NVME_AQ_DEPTH as u32) < NVME_MAX_QUEUE_DEPTH);
        assert!((NVME_DEFAULT_IO_QUEUE_DEPTH as u32) < NVME_MAX_QUEUE_DEPTH);
    }

    #[test]
    fn test_entry_sizes() {
        assert_eq!(NVME_SQ_ENTRY_SIZE, 64);
        assert_eq!(NVME_CQ_ENTRY_SIZE, 16);
        assert!(NVME_SQ_ENTRY_SIZE > NVME_CQ_ENTRY_SIZE);
    }

    #[test]
    fn test_priorities_ordered() {
        assert!(NVME_QP_URGENT < NVME_QP_HIGH);
        assert!(NVME_QP_HIGH < NVME_QP_MEDIUM);
        assert!(NVME_QP_MEDIUM < NVME_QP_LOW);
    }

    #[test]
    fn test_phase_mask() {
        assert_eq!(NVME_CQ_PHASE_MASK, 1);
    }
}

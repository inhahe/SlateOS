//! `<linux/io_uring.h>` — Additional io_uring constants (batch 4).
//!
//! Supplementary io_uring constants covering registration opcodes,
//! restriction opcodes, and buffer ring flags.

// ---------------------------------------------------------------------------
// io_uring register opcodes (IORING_REGISTER_*)
// ---------------------------------------------------------------------------

/// Register fixed buffers.
pub const IORING_REGISTER_BUFFERS: u32 = 0;
/// Unregister fixed buffers.
pub const IORING_UNREGISTER_BUFFERS: u32 = 1;
/// Register fixed files.
pub const IORING_REGISTER_FILES: u32 = 2;
/// Unregister fixed files.
pub const IORING_UNREGISTER_FILES: u32 = 3;
/// Register eventfd.
pub const IORING_REGISTER_EVENTFD: u32 = 4;
/// Unregister eventfd.
pub const IORING_UNREGISTER_EVENTFD: u32 = 5;
/// Register file update.
pub const IORING_REGISTER_FILES_UPDATE: u32 = 6;
/// Register eventfd async.
pub const IORING_REGISTER_EVENTFD_ASYNC: u32 = 7;
/// Register probe.
pub const IORING_REGISTER_PROBE: u32 = 8;
/// Register personality.
pub const IORING_REGISTER_PERSONALITY: u32 = 9;
/// Unregister personality.
pub const IORING_UNREGISTER_PERSONALITY: u32 = 10;
/// Register restrictions.
pub const IORING_REGISTER_RESTRICTIONS: u32 = 11;
/// Enable rings.
pub const IORING_REGISTER_ENABLE_RINGS: u32 = 12;
/// Register file alloc range.
pub const IORING_REGISTER_FILES2: u32 = 13;
/// Register file update2.
pub const IORING_REGISTER_FILES_UPDATE2: u32 = 14;
/// Register buffers2.
pub const IORING_REGISTER_BUFFERS2: u32 = 15;
/// Register buffers update.
pub const IORING_REGISTER_BUFFERS_UPDATE: u32 = 16;
/// Register IOWQ affinity.
pub const IORING_REGISTER_IOWQ_AFF: u32 = 17;
/// Unregister IOWQ affinity.
pub const IORING_UNREGISTER_IOWQ_AFF: u32 = 18;
/// Register IOWQ max workers.
pub const IORING_REGISTER_IOWQ_MAX_WORKERS: u32 = 19;
/// Register ring fd.
pub const IORING_REGISTER_RING_FDS: u32 = 20;
/// Unregister ring fd.
pub const IORING_UNREGISTER_RING_FDS: u32 = 21;
/// Register pbuf ring.
pub const IORING_REGISTER_PBUF_RING: u32 = 22;
/// Unregister pbuf ring.
pub const IORING_UNREGISTER_PBUF_RING: u32 = 23;
/// Sync cancel.
pub const IORING_REGISTER_SYNC_CANCEL: u32 = 24;
/// Register file alloc range.
pub const IORING_REGISTER_FILE_ALLOC_RANGE: u32 = 25;

// ---------------------------------------------------------------------------
// io_uring restriction opcodes
// ---------------------------------------------------------------------------

/// Allow SQE opcode.
pub const IORING_RESTRICTION_REGISTER_OP: u32 = 0;
/// Allow SQE flags.
pub const IORING_RESTRICTION_SQE_OP: u32 = 1;
/// Allow SQE flags mask.
pub const IORING_RESTRICTION_SQE_FLAGS_ALLOWED: u32 = 2;
/// Require SQE flags mask.
pub const IORING_RESTRICTION_SQE_FLAGS_REQUIRED: u32 = 3;

// ---------------------------------------------------------------------------
// Buffer ring flags
// ---------------------------------------------------------------------------

/// Buffer ring is managed by the kernel (INC mode).
pub const IOU_PBUF_RING_MMAP: u32 = 1 << 0;
/// Buffer ring increment mode.
pub const IOU_PBUF_RING_INC: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_opcodes_distinct() {
        let ops = [
            IORING_REGISTER_BUFFERS, IORING_UNREGISTER_BUFFERS,
            IORING_REGISTER_FILES, IORING_UNREGISTER_FILES,
            IORING_REGISTER_EVENTFD, IORING_UNREGISTER_EVENTFD,
            IORING_REGISTER_FILES_UPDATE, IORING_REGISTER_EVENTFD_ASYNC,
            IORING_REGISTER_PROBE, IORING_REGISTER_PERSONALITY,
            IORING_UNREGISTER_PERSONALITY, IORING_REGISTER_RESTRICTIONS,
            IORING_REGISTER_ENABLE_RINGS, IORING_REGISTER_FILES2,
            IORING_REGISTER_FILES_UPDATE2, IORING_REGISTER_BUFFERS2,
            IORING_REGISTER_BUFFERS_UPDATE, IORING_REGISTER_IOWQ_AFF,
            IORING_UNREGISTER_IOWQ_AFF, IORING_REGISTER_IOWQ_MAX_WORKERS,
            IORING_REGISTER_RING_FDS, IORING_UNREGISTER_RING_FDS,
            IORING_REGISTER_PBUF_RING, IORING_UNREGISTER_PBUF_RING,
            IORING_REGISTER_SYNC_CANCEL, IORING_REGISTER_FILE_ALLOC_RANGE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_register_values() {
        assert_eq!(IORING_REGISTER_BUFFERS, 0);
        assert_eq!(IORING_REGISTER_FILE_ALLOC_RANGE, 25);
    }

    #[test]
    fn test_restriction_opcodes_distinct() {
        let ops = [
            IORING_RESTRICTION_REGISTER_OP, IORING_RESTRICTION_SQE_OP,
            IORING_RESTRICTION_SQE_FLAGS_ALLOWED,
            IORING_RESTRICTION_SQE_FLAGS_REQUIRED,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_pbuf_ring_flags_power_of_two() {
        assert!(IOU_PBUF_RING_MMAP.is_power_of_two());
        assert!(IOU_PBUF_RING_INC.is_power_of_two());
    }

    #[test]
    fn test_pbuf_ring_flags_no_overlap() {
        assert_eq!(IOU_PBUF_RING_MMAP & IOU_PBUF_RING_INC, 0);
    }
}

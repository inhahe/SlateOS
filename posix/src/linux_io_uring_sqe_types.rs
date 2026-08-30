//! `<linux/io_uring.h>` — io_uring SQE field offset and size constants.
//!
//! The submission queue entry (SQE) is the primary structure for
//! submitting operations to an io_uring instance. These constants
//! define field sizes, offsets, and personality/buffer group IDs.

// ---------------------------------------------------------------------------
// SQE structure field sizes (in bytes)
// ---------------------------------------------------------------------------

/// Size of a standard SQE (64 bytes).
pub const IORING_SQE_SIZE: u32 = 64;
/// Size of a wide SQE (128 bytes, for large commands).
pub const IORING_SQE_SIZE_WIDE: u32 = 128;

// ---------------------------------------------------------------------------
// SQE personality (credential) constants
// ---------------------------------------------------------------------------

/// No personality (use submitter's credentials).
pub const IORING_PERSONALITY_NONE: u16 = 0;

// ---------------------------------------------------------------------------
// Buffer group IDs
// ---------------------------------------------------------------------------

/// Maximum buffer group ID.
pub const IORING_MAX_BUF_GROUP: u16 = 0xFFFF;
/// Default buffer group (none).
pub const IORING_BUF_GROUP_NONE: u16 = 0;

// ---------------------------------------------------------------------------
// SQE command flags (for IORING_OP_URING_CMD)
// ---------------------------------------------------------------------------

/// Fixed SQE for uring passthrough command.
pub const IORING_URING_CMD_FIXED: u32 = 1 << 0;
/// Polled completion for uring command.
pub const IORING_URING_CMD_POLLED: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// Accept flags (for IORING_OP_ACCEPT)
// ---------------------------------------------------------------------------

/// Multi-shot accept (re-arms).
pub const IORING_ACCEPT_MULTISHOT: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Recv flags
// ---------------------------------------------------------------------------

/// Multi-shot receive.
pub const IORING_RECV_MULTISHOT: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Send flags
// ---------------------------------------------------------------------------

/// Use zero-copy send.
pub const IORING_SEND_ZC_REPORT_USAGE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqe_sizes() {
        assert_eq!(IORING_SQE_SIZE, 64);
        assert_eq!(IORING_SQE_SIZE_WIDE, 128);
        assert_eq!(IORING_SQE_SIZE_WIDE, IORING_SQE_SIZE * 2);
    }

    #[test]
    fn test_personality_none() {
        assert_eq!(IORING_PERSONALITY_NONE, 0);
    }

    #[test]
    fn test_buf_group() {
        assert_eq!(IORING_BUF_GROUP_NONE, 0);
        assert_eq!(IORING_MAX_BUF_GROUP, u16::MAX);
    }

    #[test]
    fn test_uring_cmd_flags() {
        assert_eq!(IORING_URING_CMD_FIXED, 1);
        assert_eq!(IORING_URING_CMD_POLLED, 1 << 31);
        assert_eq!(IORING_URING_CMD_FIXED & IORING_URING_CMD_POLLED, 0);
    }

    #[test]
    fn test_accept_multishot() {
        assert_eq!(IORING_ACCEPT_MULTISHOT, 1);
    }

    #[test]
    fn test_recv_multishot() {
        assert_eq!(IORING_RECV_MULTISHOT, 2);
    }
}

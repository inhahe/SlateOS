//! `<linux/io_uring.h>` — io_uring CQE (completion queue entry) constants.
//!
//! Completion queue entries are written by the kernel to report the
//! result of submitted operations. These constants define CQE sizes,
//! result flags, and buffer selection metadata.

// ---------------------------------------------------------------------------
// CQE structure size
// ---------------------------------------------------------------------------

/// Size of a standard CQE (16 bytes).
pub const IORING_CQE_SIZE: u32 = 16;
/// Size of a big CQE (32 bytes, with extra data).
pub const IORING_CQE_SIZE_BIG: u32 = 32;

// ---------------------------------------------------------------------------
// CQE flags (cqe->flags field)
// ---------------------------------------------------------------------------

/// More completions coming for this request (multi-shot).
pub const IORING_CQE_F_MORE: u32 = 1 << 1;
/// Buffer ID is valid in the upper 16 bits.
pub const IORING_CQE_F_BUFFER: u32 = 1 << 0;
/// Socket is still readable (for recv multi-shot).
pub const IORING_CQE_F_SOCK_NONEMPTY: u32 = 1 << 2;
/// Notification CQE (zero-copy send notification).
pub const IORING_CQE_F_NOTIF: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// CQE buffer ID extraction
// ---------------------------------------------------------------------------

/// Shift to extract buffer ID from CQE flags.
pub const IORING_CQE_BUFFER_SHIFT: u32 = 16;

// ---------------------------------------------------------------------------
// Completion overflow handling
// ---------------------------------------------------------------------------

/// Indicates CQ ring has overflowed (from sq_flags).
pub const IORING_SQ_CQ_OVERFLOW: u32 = 1 << 1;
/// Indicates the task is going away.
pub const IORING_SQ_TASKRUN: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cqe_sizes() {
        assert_eq!(IORING_CQE_SIZE, 16);
        assert_eq!(IORING_CQE_SIZE_BIG, 32);
        assert_eq!(IORING_CQE_SIZE_BIG, IORING_CQE_SIZE * 2);
    }

    #[test]
    fn test_cqe_flags_power_of_two() {
        assert!(IORING_CQE_F_BUFFER.is_power_of_two());
        assert!(IORING_CQE_F_MORE.is_power_of_two());
        assert!(IORING_CQE_F_SOCK_NONEMPTY.is_power_of_two());
        assert!(IORING_CQE_F_NOTIF.is_power_of_two());
    }

    #[test]
    fn test_cqe_flags_no_overlap() {
        let flags = [
            IORING_CQE_F_BUFFER,
            IORING_CQE_F_MORE,
            IORING_CQE_F_SOCK_NONEMPTY,
            IORING_CQE_F_NOTIF,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_buffer_shift() {
        assert_eq!(IORING_CQE_BUFFER_SHIFT, 16);
    }

    #[test]
    fn test_sq_overflow_flags() {
        assert_eq!(IORING_SQ_CQ_OVERFLOW & IORING_SQ_TASKRUN, 0);
    }
}

//! `<linux/io_uring.h>` — io_uring SQE and general flag constants.
//!
//! These flags modify the behavior of individual submission queue
//! entries (SQEs). They control linking, draining, buffer selection,
//! and other per-operation semantics.

// ---------------------------------------------------------------------------
// SQE flags (sqe->flags field)
// ---------------------------------------------------------------------------

/// Use fixed file descriptor from registered set.
pub const IOSQE_FIXED_FILE: u8 = 1 << 0;
/// Issue after in-flight operations complete.
pub const IOSQE_IO_DRAIN: u8 = 1 << 1;
/// Link to next SQE (chain on success).
pub const IOSQE_IO_LINK: u8 = 1 << 2;
/// Hard link (chain even on failure).
pub const IOSQE_IO_HARDLINK: u8 = 1 << 3;
/// Always go async (never inline).
pub const IOSQE_ASYNC: u8 = 1 << 4;
/// Select buffer from provided buffer pool.
pub const IOSQE_BUFFER_SELECT: u8 = 1 << 5;
/// Don't post CQE on success (skip CQ).
pub const IOSQE_CQE_SKIP_SUCCESS: u8 = 1 << 6;

// ---------------------------------------------------------------------------
// FSYNC flags (for IORING_OP_FSYNC)
// ---------------------------------------------------------------------------

/// Sync only data, not metadata.
pub const IORING_FSYNC_DATASYNC: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// TIMEOUT flags (for IORING_OP_TIMEOUT)
// ---------------------------------------------------------------------------

/// Use absolute timeout (not relative).
pub const IORING_TIMEOUT_ABS: u32 = 1 << 0;
/// Update existing timeout value.
pub const IORING_TIMEOUT_UPDATE: u32 = 1 << 1;
/// Use boot time clock.
pub const IORING_TIMEOUT_BOOTTIME: u32 = 1 << 2;
/// Use real time clock.
pub const IORING_TIMEOUT_REALTIME: u32 = 1 << 3;
/// Link timeout to completion count.
pub const IORING_TIMEOUT_ETIME_SUCCESS: u32 = 1 << 5;
/// Multi-shot timeout (re-arms).
pub const IORING_TIMEOUT_MULTISHOT: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// POLL flags
// ---------------------------------------------------------------------------

/// Multi-shot poll (re-arms after each event).
pub const IORING_POLL_ADD_MULTI: u32 = 1 << 0;
/// Update existing poll request.
pub const IORING_POLL_UPDATE_EVENTS: u32 = 1 << 1;
/// Update user_data of poll request.
pub const IORING_POLL_UPDATE_USER_DATA: u32 = 1 << 2;
/// Level-triggered poll.
pub const IORING_POLL_ADD_LEVEL: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqe_flags_power_of_two() {
        let flags = [
            IOSQE_FIXED_FILE, IOSQE_IO_DRAIN, IOSQE_IO_LINK,
            IOSQE_IO_HARDLINK, IOSQE_ASYNC, IOSQE_BUFFER_SELECT,
            IOSQE_CQE_SKIP_SUCCESS,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_sqe_flags_no_overlap() {
        let flags = [
            IOSQE_FIXED_FILE, IOSQE_IO_DRAIN, IOSQE_IO_LINK,
            IOSQE_IO_HARDLINK, IOSQE_ASYNC, IOSQE_BUFFER_SELECT,
            IOSQE_CQE_SKIP_SUCCESS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_fsync_datasync() {
        assert_eq!(IORING_FSYNC_DATASYNC, 1);
    }

    #[test]
    fn test_timeout_flags_distinct() {
        let flags = [
            IORING_TIMEOUT_ABS, IORING_TIMEOUT_UPDATE,
            IORING_TIMEOUT_BOOTTIME, IORING_TIMEOUT_REALTIME,
            IORING_TIMEOUT_ETIME_SUCCESS, IORING_TIMEOUT_MULTISHOT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_poll_flags_power_of_two() {
        assert!(IORING_POLL_ADD_MULTI.is_power_of_two());
        assert!(IORING_POLL_UPDATE_EVENTS.is_power_of_two());
        assert!(IORING_POLL_UPDATE_USER_DATA.is_power_of_two());
        assert!(IORING_POLL_ADD_LEVEL.is_power_of_two());
    }
}

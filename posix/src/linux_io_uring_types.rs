//! `<linux/io_uring.h>` supplemental — io_uring SQE/CQE type constants.
//!
//! Additional io_uring types and flag constants that complement the
//! core `linux_io_uring` module's opcode and flag definitions.

// ---------------------------------------------------------------------------
// SQE flags (IOSQE_*)
// ---------------------------------------------------------------------------

/// Fixed file (use registered file index).
pub const IOSQE_FIXED_FILE: u8 = 1 << 0;
/// Drain (wait for prior requests).
pub const IOSQE_IO_DRAIN: u8 = 1 << 1;
/// Link with next SQE.
pub const IOSQE_IO_LINK: u8 = 1 << 2;
/// Hard link.
pub const IOSQE_IO_HARDLINK: u8 = 1 << 3;
/// Async (force offload to worker).
pub const IOSQE_ASYNC: u8 = 1 << 4;
/// Use registered buffer.
pub const IOSQE_BUFFER_SELECT: u8 = 1 << 5;
/// CQE skip (don't generate CQE).
pub const IOSQE_CQE_SKIP_SUCCESS: u8 = 1 << 6;

// ---------------------------------------------------------------------------
// CQE flags
// ---------------------------------------------------------------------------

/// More CQEs to process (buffer notification).
pub const IORING_CQE_F_BUFFER: u32 = 1 << 0;
/// CQE for multi-shot.
pub const IORING_CQE_F_MORE: u32 = 1 << 1;
/// Socket ready.
pub const IORING_CQE_F_SOCK_NONEMPTY: u32 = 1 << 2;
/// Notification CQE.
pub const IORING_CQE_F_NOTIF: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Setup flags
// ---------------------------------------------------------------------------

/// Use io_poll for completions.
pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
/// Use SQ poll thread.
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
/// Attach to existing wq.
pub const IORING_SETUP_ATTACH_WQ: u32 = 1 << 5;
/// Ring is disabled.
pub const IORING_SETUP_R_DISABLED: u32 = 1 << 6;
/// Submit all (no partial).
pub const IORING_SETUP_SUBMIT_ALL: u32 = 1 << 7;
/// Cooperative task running.
pub const IORING_SETUP_COOP_TASKRUN: u32 = 1 << 8;
/// Task running flag.
pub const IORING_SETUP_TASKRUN_FLAG: u32 = 1 << 9;
/// Single issuer.
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 1 << 12;
/// Defer task run.
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// Register opcodes
// ---------------------------------------------------------------------------

/// Register buffers.
pub const IORING_REGISTER_BUFFERS: u32 = 0;
/// Unregister buffers.
pub const IORING_UNREGISTER_BUFFERS: u32 = 1;
/// Register files.
pub const IORING_REGISTER_FILES: u32 = 2;
/// Unregister files.
pub const IORING_UNREGISTER_FILES: u32 = 3;
/// Register eventfd.
pub const IORING_REGISTER_EVENTFD: u32 = 4;
/// Unregister eventfd.
pub const IORING_UNREGISTER_EVENTFD: u32 = 5;
/// Register files update.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqe_flags_are_powers_of_two() {
        let flags = [
            IOSQE_FIXED_FILE,
            IOSQE_IO_DRAIN,
            IOSQE_IO_LINK,
            IOSQE_IO_HARDLINK,
            IOSQE_ASYNC,
            IOSQE_BUFFER_SELECT,
            IOSQE_CQE_SKIP_SUCCESS,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_cqe_flags_are_powers_of_two() {
        let flags = [
            IORING_CQE_F_BUFFER,
            IORING_CQE_F_MORE,
            IORING_CQE_F_SOCK_NONEMPTY,
            IORING_CQE_F_NOTIF,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_setup_flags_powers_of_two() {
        let flags = [
            IORING_SETUP_IOPOLL,
            IORING_SETUP_SQPOLL,
            IORING_SETUP_ATTACH_WQ,
            IORING_SETUP_R_DISABLED,
            IORING_SETUP_SUBMIT_ALL,
            IORING_SETUP_COOP_TASKRUN,
            IORING_SETUP_TASKRUN_FLAG,
            IORING_SETUP_SINGLE_ISSUER,
            IORING_SETUP_DEFER_TASKRUN,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_register_ops_distinct() {
        let ops = [
            IORING_REGISTER_BUFFERS,
            IORING_UNREGISTER_BUFFERS,
            IORING_REGISTER_FILES,
            IORING_UNREGISTER_FILES,
            IORING_REGISTER_EVENTFD,
            IORING_UNREGISTER_EVENTFD,
            IORING_REGISTER_FILES_UPDATE,
            IORING_REGISTER_EVENTFD_ASYNC,
            IORING_REGISTER_PROBE,
            IORING_REGISTER_PERSONALITY,
            IORING_UNREGISTER_PERSONALITY,
            IORING_REGISTER_RESTRICTIONS,
            IORING_REGISTER_ENABLE_RINGS,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }
}

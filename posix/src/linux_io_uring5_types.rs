//! `<linux/io_uring.h>` — Additional io_uring constants (part 5).
//!
//! Supplementary io_uring constants covering CQE flags,
//! SQE personality, and buffer selection.

// ---------------------------------------------------------------------------
// io_uring CQE flags
// ---------------------------------------------------------------------------

/// More data available in buffer ring.
pub const IORING_CQE_F_BUFFER: u32 = 1 << 0;
/// More completions coming.
pub const IORING_CQE_F_MORE: u32 = 1 << 1;
/// Socket readable notification.
pub const IORING_CQE_F_SOCK_NONEMPTY: u32 = 1 << 2;
/// Notification event.
pub const IORING_CQE_F_NOTIF: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// io_uring SQE flags
// ---------------------------------------------------------------------------

/// Fixed file.
pub const IOSQE_FIXED_FILE: u8 = 1 << 0;
/// IO drain.
pub const IOSQE_IO_DRAIN: u8 = 1 << 1;
/// IO link.
pub const IOSQE_IO_LINK: u8 = 1 << 2;
/// IO hardlink.
pub const IOSQE_IO_HARDLINK: u8 = 1 << 3;
/// Async.
pub const IOSQE_ASYNC: u8 = 1 << 4;
/// Buffer select.
pub const IOSQE_BUFFER_SELECT: u8 = 1 << 5;
/// CQE skip on success.
pub const IOSQE_CQE_SKIP_SUCCESS: u8 = 1 << 6;

// ---------------------------------------------------------------------------
// io_uring setup flags (additional)
// ---------------------------------------------------------------------------

/// Single issuer.
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 1 << 12;
/// Defer taskrun.
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 1 << 13;
/// No mmap.
pub const IORING_SETUP_NO_MMAP: u32 = 1 << 14;
/// Registered ring fd.
pub const IORING_SETUP_REGISTERED_FD_ONLY: u32 = 1 << 15;
/// No sqarray.
pub const IORING_SETUP_NO_SQARRAY: u32 = 1 << 16;

// ---------------------------------------------------------------------------
// io_uring feature flags
// ---------------------------------------------------------------------------

/// Single mmap.
pub const IORING_FEAT_SINGLE_MMAP: u32 = 1 << 0;
/// Nodrop.
pub const IORING_FEAT_NODROP: u32 = 1 << 1;
/// Submit stable.
pub const IORING_FEAT_SUBMIT_STABLE: u32 = 1 << 2;
/// RW cur pos.
pub const IORING_FEAT_RW_CUR_POS: u32 = 1 << 3;
/// Cur personality.
pub const IORING_FEAT_CUR_PERSONALITY: u32 = 1 << 4;
/// Fast poll.
pub const IORING_FEAT_FAST_POLL: u32 = 1 << 5;
/// Poll 32-bit.
pub const IORING_FEAT_POLL_32BITS: u32 = 1 << 6;
/// SQPoll nonfixed.
pub const IORING_FEAT_SQPOLL_NONFIXED: u32 = 1 << 7;
/// Ext arg.
pub const IORING_FEAT_EXT_ARG: u32 = 1 << 8;
/// Native workers.
pub const IORING_FEAT_NATIVE_WORKERS: u32 = 1 << 9;
/// Rsrc tags.
pub const IORING_FEAT_RSRC_TAGS: u32 = 1 << 10;
/// CQE skip.
pub const IORING_FEAT_CQE_SKIP: u32 = 1 << 11;
/// Linked file.
pub const IORING_FEAT_LINKED_FILE: u32 = 1 << 12;
/// Reg reg ring.
pub const IORING_FEAT_REG_REG_RING: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_sqe_flags_no_overlap() {
        let flags = [
            IOSQE_FIXED_FILE,
            IOSQE_IO_DRAIN,
            IOSQE_IO_LINK,
            IOSQE_IO_HARDLINK,
            IOSQE_ASYNC,
            IOSQE_BUFFER_SELECT,
            IOSQE_CQE_SKIP_SUCCESS,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_setup_flags_no_overlap() {
        let flags = [
            IORING_SETUP_SINGLE_ISSUER,
            IORING_SETUP_DEFER_TASKRUN,
            IORING_SETUP_NO_MMAP,
            IORING_SETUP_REGISTERED_FD_ONLY,
            IORING_SETUP_NO_SQARRAY,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_feat_flags_no_overlap() {
        let flags = [
            IORING_FEAT_SINGLE_MMAP,
            IORING_FEAT_NODROP,
            IORING_FEAT_SUBMIT_STABLE,
            IORING_FEAT_RW_CUR_POS,
            IORING_FEAT_CUR_PERSONALITY,
            IORING_FEAT_FAST_POLL,
            IORING_FEAT_POLL_32BITS,
            IORING_FEAT_SQPOLL_NONFIXED,
            IORING_FEAT_EXT_ARG,
            IORING_FEAT_NATIVE_WORKERS,
            IORING_FEAT_RSRC_TAGS,
            IORING_FEAT_CQE_SKIP,
            IORING_FEAT_LINKED_FILE,
            IORING_FEAT_REG_REG_RING,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

//! `<linux/io_uring.h>` — Additional io_uring constants (part 3).
//!
//! Supplementary io_uring constants covering setup flags,
//! feature flags, register opcodes, and CQE flags.

// ---------------------------------------------------------------------------
// io_uring setup flags (IORING_SETUP_*)
// ---------------------------------------------------------------------------

/// Kernel-side polling.
pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
/// Submission queue polling.
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
/// Attach to existing SQ poll thread.
pub const IORING_SETUP_SQ_AFF: u32 = 1 << 2;
/// App-owned CQ ring.
pub const IORING_SETUP_CQSIZE: u32 = 1 << 3;
/// Clamp ring sizes.
pub const IORING_SETUP_CLAMP: u32 = 1 << 4;
/// Attach to existing wq.
pub const IORING_SETUP_ATTACH_WQ: u32 = 1 << 5;
/// Start ring disabled.
pub const IORING_SETUP_R_DISABLED: u32 = 1 << 6;
/// Submit all on enter.
pub const IORING_SETUP_SUBMIT_ALL: u32 = 1 << 7;
/// Cooperative task running.
pub const IORING_SETUP_COOP_TASKRUN: u32 = 1 << 8;
/// Task run flag.
pub const IORING_SETUP_TASKRUN_FLAG: u32 = 1 << 9;
/// SQE128.
pub const IORING_SETUP_SQE128: u32 = 1 << 10;
/// CQE32.
pub const IORING_SETUP_CQE32: u32 = 1 << 11;
/// Single issuer.
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 1 << 12;
/// Defer taskrun.
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 1 << 13;
/// No mmap.
pub const IORING_SETUP_NO_MMAP: u32 = 1 << 14;
/// Registered fd only.
pub const IORING_SETUP_REGISTERED_FD_ONLY: u32 = 1 << 15;
/// No SQ array.
pub const IORING_SETUP_NO_SQARRAY: u32 = 1 << 16;

// ---------------------------------------------------------------------------
// io_uring feature flags (IORING_FEAT_*)
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
/// Poll 32 bits.
pub const IORING_FEAT_POLL_32BITS: u32 = 1 << 6;
/// SQPoll nonfixed.
pub const IORING_FEAT_SQPOLL_NONFIXED: u32 = 1 << 7;
/// Enter ext arg.
pub const IORING_FEAT_EXT_ARG: u32 = 1 << 8;
/// Native workers.
pub const IORING_FEAT_NATIVE_WORKERS: u32 = 1 << 9;
/// Resource tagging.
pub const IORING_FEAT_RSRC_TAGS: u32 = 1 << 10;
/// CQE skip.
pub const IORING_FEAT_CQE_SKIP: u32 = 1 << 11;
/// Linked file.
pub const IORING_FEAT_LINKED_FILE: u32 = 1 << 12;
/// Reg reg ring.
pub const IORING_FEAT_REG_REG_RING: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// io_uring CQE flags
// ---------------------------------------------------------------------------

/// Buffer shift.
pub const IORING_CQE_BUFFER_SHIFT: u32 = 16;
/// More CQEs.
pub const IORING_CQE_F_MORE: u32 = 1 << 1;
/// Socket readable.
pub const IORING_CQE_F_SOCK_NONEMPTY: u32 = 1 << 2;
/// Notification.
pub const IORING_CQE_F_NOTIF: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_flags_power_of_two() {
        let flags = [
            IORING_SETUP_IOPOLL,
            IORING_SETUP_SQPOLL,
            IORING_SETUP_SQ_AFF,
            IORING_SETUP_CQSIZE,
            IORING_SETUP_CLAMP,
            IORING_SETUP_ATTACH_WQ,
            IORING_SETUP_R_DISABLED,
            IORING_SETUP_SUBMIT_ALL,
            IORING_SETUP_COOP_TASKRUN,
            IORING_SETUP_TASKRUN_FLAG,
            IORING_SETUP_SQE128,
            IORING_SETUP_CQE32,
            IORING_SETUP_SINGLE_ISSUER,
            IORING_SETUP_DEFER_TASKRUN,
            IORING_SETUP_NO_MMAP,
            IORING_SETUP_REGISTERED_FD_ONLY,
            IORING_SETUP_NO_SQARRAY,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_feat_flags_power_of_two() {
        let feats = [
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
        for f in &feats {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_cqe_flags_distinct() {
        let flags = [
            IORING_CQE_F_MORE,
            IORING_CQE_F_SOCK_NONEMPTY,
            IORING_CQE_F_NOTIF,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}

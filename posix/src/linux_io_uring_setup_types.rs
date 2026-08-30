//! `<linux/io_uring.h>` — io_uring_setup() parameter and feature constants.
//!
//! The `io_uring_setup()` syscall creates a new io_uring instance.
//! These constants define the setup flags that control ring behavior
//! and the feature bits that report kernel capabilities.

// ---------------------------------------------------------------------------
// io_uring_setup() flags (params.flags)
// ---------------------------------------------------------------------------

/// Use io_poll for completions (busy-wait).
pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
/// Kernel-side SQ thread polling.
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
/// Bind SQ poll thread to specific CPU.
pub const IORING_SETUP_SQ_AFF: u32 = 1 << 2;
/// Custom CQ ring size.
pub const IORING_SETUP_CQSIZE: u32 = 1 << 3;
/// Clamp ring sizes to max.
pub const IORING_SETUP_CLAMP: u32 = 1 << 4;
/// Attach to existing wq.
pub const IORING_SETUP_ATTACH_WQ: u32 = 1 << 5;
/// Start ring in disabled state.
pub const IORING_SETUP_R_DISABLED: u32 = 1 << 6;
/// Submit from any task.
pub const IORING_SETUP_SUBMIT_ALL: u32 = 1 << 7;
/// Cooperative task running for SQPOLL.
pub const IORING_SETUP_COOP_TASKRUN: u32 = 1 << 8;
/// Task run flag is always set.
pub const IORING_SETUP_TASKRUN_FLAG: u32 = 1 << 9;
/// Use wide SQEs (128 bytes).
pub const IORING_SETUP_SQE128: u32 = 1 << 10;
/// Use big CQEs (32 bytes).
pub const IORING_SETUP_CQE32: u32 = 1 << 11;
/// Enable single-issuer optimization.
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 1 << 12;
/// Defer taskrun until user space.
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// io_uring feature flags (params.features, read-only from kernel)
// ---------------------------------------------------------------------------

/// Support for IORING_SETUP_SINGLE_MMAP.
pub const IORING_FEAT_SINGLE_MMAP: u32 = 1 << 0;
/// Support for NODROP CQ overflow.
pub const IORING_FEAT_NODROP: u32 = 1 << 1;
/// Support for IOSQE_ASYNC on all ops.
pub const IORING_FEAT_SUBMIT_STABLE: u32 = 1 << 2;
/// Support for RW_CUR_POS (-1 offset).
pub const IORING_FEAT_RW_CUR_POS: u32 = 1 << 3;
/// Support for CUR_PERSONALITY.
pub const IORING_FEAT_CUR_PERSONALITY: u32 = 1 << 4;
/// Support for fast poll.
pub const IORING_FEAT_FAST_POLL: u32 = 1 << 5;
/// Support for POLL_32BITS.
pub const IORING_FEAT_POLL_32BITS: u32 = 1 << 6;
/// Support for SQPOLL non-fixed files.
pub const IORING_FEAT_SQPOLL_NONFIXED: u32 = 1 << 7;
/// Support for enter with ext_arg.
pub const IORING_FEAT_EXT_ARG: u32 = 1 << 8;
/// Support for native workers.
pub const IORING_FEAT_NATIVE_WORKERS: u32 = 1 << 9;
/// Support for resource tagging.
pub const IORING_FEAT_RSRC_TAGS: u32 = 1 << 10;
/// CQE skip on success supported.
pub const IORING_FEAT_CQE_SKIP: u32 = 1 << 11;
/// Support for linked files.
pub const IORING_FEAT_LINKED_FILE: u32 = 1 << 12;

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
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_setup_flags_no_overlap() {
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
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
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
        ];
        for f in &feats {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_feat_flags_no_overlap() {
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
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }
}

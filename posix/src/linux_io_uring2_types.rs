//! `<linux/io_uring.h>` — Additional io_uring constants.
//!
//! Supplementary io_uring constants covering setup flags,
//! feature bits, SQ/CQ ring flags, and timeout options.

// ---------------------------------------------------------------------------
// io_uring setup flags (IORING_SETUP_*)
// ---------------------------------------------------------------------------

/// I/O polling mode.
pub const IORING_SETUP_IOPOLL: u32 = 1;
/// SQ polling mode.
pub const IORING_SETUP_SQPOLL: u32 = 2;
/// Bind SQ to specific CPU.
pub const IORING_SETUP_SQ_AFF: u32 = 4;
/// Application manages CQ ring size.
pub const IORING_SETUP_CQSIZE: u32 = 8;
/// Clamp entries to implementation max.
pub const IORING_SETUP_CLAMP: u32 = 16;
/// Attach to existing wq.
pub const IORING_SETUP_ATTACH_WQ: u32 = 32;
/// Start ring disabled.
pub const IORING_SETUP_R_DISABLED: u32 = 64;
/// Submit all SQEs as group.
pub const IORING_SETUP_SUBMIT_ALL: u32 = 128;
/// Cooperative task running.
pub const IORING_SETUP_COOP_TASKRUN: u32 = 256;
/// Deferred task running.
pub const IORING_SETUP_TASKRUN_FLAG: u32 = 512;
/// SQE128 mode.
pub const IORING_SETUP_SQE128: u32 = 1024;
/// CQE32 mode.
pub const IORING_SETUP_CQE32: u32 = 2048;
/// Single issuer.
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 4096;
/// Defer task run.
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 8192;
/// No mmap.
pub const IORING_SETUP_NO_MMAP: u32 = 16384;
/// Registered fd only.
pub const IORING_SETUP_REGISTERED_FD_ONLY: u32 = 32768;
/// No SQArray.
pub const IORING_SETUP_NO_SQARRAY: u32 = 65536;

// ---------------------------------------------------------------------------
// io_uring feature flags (IORING_FEAT_*)
// ---------------------------------------------------------------------------

/// Single mmap.
pub const IORING_FEAT_SINGLE_MMAP: u32 = 1;
/// Nodrop mode.
pub const IORING_FEAT_NODROP: u32 = 2;
/// Submit stable.
pub const IORING_FEAT_SUBMIT_STABLE: u32 = 4;
/// RW current pos.
pub const IORING_FEAT_RW_CUR_POS: u32 = 8;
/// Current personality.
pub const IORING_FEAT_CUR_PERSONALITY: u32 = 16;
/// Fast poll.
pub const IORING_FEAT_FAST_POLL: u32 = 32;
/// Poll 32 bits.
pub const IORING_FEAT_POLL_32BITS: u32 = 64;
/// SQPoll nonfixed.
pub const IORING_FEAT_SQPOLL_NONFIXED: u32 = 128;
/// Enter ext arg.
pub const IORING_FEAT_EXT_ARG: u32 = 256;
/// Native workers.
pub const IORING_FEAT_NATIVE_WORKERS: u32 = 512;
/// Resource tagging.
pub const IORING_FEAT_RSRC_TAGS: u32 = 1024;
/// CQE skip.
pub const IORING_FEAT_CQE_SKIP: u32 = 2048;
/// Linked file.
pub const IORING_FEAT_LINKED_FILE: u32 = 4096;
/// Registered ring.
pub const IORING_FEAT_REG_REG_RING: u32 = 8192;
/// Recvsend bundle.
pub const IORING_FEAT_RECVSEND_BUNDLE: u32 = 16384;

// ---------------------------------------------------------------------------
// SQ ring flags (IORING_SQ_*)
// ---------------------------------------------------------------------------

/// SQ needs wakeup.
pub const IORING_SQ_NEED_WAKEUP: u32 = 1;
/// CQ overflow.
pub const IORING_SQ_CQ_OVERFLOW: u32 = 2;
/// Task run flag.
pub const IORING_SQ_TASKRUN: u32 = 4;

// ---------------------------------------------------------------------------
// CQ ring flags (IORING_CQ_*)
// ---------------------------------------------------------------------------

/// CQ eventfd disabled.
pub const IORING_CQ_EVENTFD_DISABLED: u32 = 1;

// ---------------------------------------------------------------------------
// io_uring enter flags (IORING_ENTER_*)
// ---------------------------------------------------------------------------

/// Get events.
pub const IORING_ENTER_GETEVENTS: u32 = 1;
/// SQ wakeup.
pub const IORING_ENTER_SQ_WAKEUP: u32 = 2;
/// SQ wait.
pub const IORING_ENTER_SQ_WAIT: u32 = 4;
/// Extended arg.
pub const IORING_ENTER_EXT_ARG: u32 = 8;
/// Registered ring.
pub const IORING_ENTER_REGISTERED_RING: u32 = 16;

// ---------------------------------------------------------------------------
// Timeout flags (IORING_TIMEOUT_*)
// ---------------------------------------------------------------------------

/// Absolute timeout.
pub const IORING_TIMEOUT_ABS: u32 = 1;
/// Update existing timeout.
pub const IORING_TIMEOUT_UPDATE: u32 = 2;
/// Boot time clock.
pub const IORING_TIMEOUT_BOOTTIME: u32 = 4;
/// Realtime clock.
pub const IORING_TIMEOUT_REALTIME: u32 = 8;
/// Link timeout for connected SQEs.
pub const IORING_TIMEOUT_ETIME_SUCCESS: u32 = 16;
/// Multishot timeout.
pub const IORING_TIMEOUT_MULTISHOT: u32 = 32;
/// Clock source.
pub const IORING_TIMEOUT_CLOCK_MASK: u32 = IORING_TIMEOUT_BOOTTIME | IORING_TIMEOUT_REALTIME;

// ---------------------------------------------------------------------------
// io_uring register opcodes (IORING_REGISTER_*)
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
            assert!(f.is_power_of_two(), "{} not power of two", f);
        }
    }

    #[test]
    fn test_setup_flags_distinct() {
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
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
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
            IORING_FEAT_REG_REG_RING,
            IORING_FEAT_RECVSEND_BUNDLE,
        ];
        for f in &feats {
            assert!(f.is_power_of_two(), "{} not power of two", f);
        }
    }

    #[test]
    fn test_sq_flags_distinct() {
        let flags = [
            IORING_SQ_NEED_WAKEUP,
            IORING_SQ_CQ_OVERFLOW,
            IORING_SQ_TASKRUN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_enter_flags_power_of_two() {
        let flags = [
            IORING_ENTER_GETEVENTS,
            IORING_ENTER_SQ_WAKEUP,
            IORING_ENTER_SQ_WAIT,
            IORING_ENTER_EXT_ARG,
            IORING_ENTER_REGISTERED_RING,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "{} not power of two", f);
        }
    }

    #[test]
    fn test_timeout_clock_mask() {
        assert_eq!(
            IORING_TIMEOUT_CLOCK_MASK,
            IORING_TIMEOUT_BOOTTIME | IORING_TIMEOUT_REALTIME
        );
    }

    #[test]
    fn test_register_opcodes_sequential() {
        assert_eq!(IORING_REGISTER_BUFFERS, 0);
        assert_eq!(IORING_UNREGISTER_BUFFERS, 1);
        assert_eq!(IORING_REGISTER_FILES, 2);
        assert_eq!(IORING_UNREGISTER_FILES, 3);
    }

    #[test]
    fn test_register_opcodes_distinct() {
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

    #[test]
    fn test_cq_eventfd_disabled() {
        assert_eq!(IORING_CQ_EVENTFD_DISABLED, 1);
    }
}

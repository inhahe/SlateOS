//! `<linux/io_uring.h>` (SQE) — io_uring submission queue entry opcodes.
//!
//! io_uring is Linux's high-performance async I/O interface. Operations
//! are submitted via SQEs (Submission Queue Entries) in a shared ring
//! buffer, and completions are delivered via CQEs (Completion Queue
//! Entries). This eliminates syscall overhead for I/O-heavy workloads.

// ---------------------------------------------------------------------------
// io_uring opcodes (IORING_OP_*)
// ---------------------------------------------------------------------------

/// No operation.
pub const IORING_OP_NOP: u8 = 0;
/// Vectored read.
pub const IORING_OP_READV: u8 = 1;
/// Vectored write.
pub const IORING_OP_WRITEV: u8 = 2;
/// fsync.
pub const IORING_OP_FSYNC: u8 = 3;
/// Read fixed buffer.
pub const IORING_OP_READ_FIXED: u8 = 4;
/// Write fixed buffer.
pub const IORING_OP_WRITE_FIXED: u8 = 5;
/// Poll add.
pub const IORING_OP_POLL_ADD: u8 = 6;
/// Poll remove.
pub const IORING_OP_POLL_REMOVE: u8 = 7;
/// sync_file_range.
pub const IORING_OP_SYNC_FILE_RANGE: u8 = 8;
/// sendmsg.
pub const IORING_OP_SENDMSG: u8 = 9;
/// recvmsg.
pub const IORING_OP_RECVMSG: u8 = 10;
/// Timeout.
pub const IORING_OP_TIMEOUT: u8 = 11;
/// Timeout remove.
pub const IORING_OP_TIMEOUT_REMOVE: u8 = 12;
/// Accept.
pub const IORING_OP_ACCEPT: u8 = 13;
/// Cancel request.
pub const IORING_OP_ASYNC_CANCEL: u8 = 14;
/// Link timeout.
pub const IORING_OP_LINK_TIMEOUT: u8 = 15;
/// Connect.
pub const IORING_OP_CONNECT: u8 = 16;
/// Fallocate.
pub const IORING_OP_FALLOCATE: u8 = 17;
/// openat.
pub const IORING_OP_OPENAT: u8 = 18;
/// Close.
pub const IORING_OP_CLOSE: u8 = 19;
/// Read.
pub const IORING_OP_READ: u8 = 22;
/// Write.
pub const IORING_OP_WRITE: u8 = 23;
/// statx.
pub const IORING_OP_STATX: u8 = 21;
/// splice.
pub const IORING_OP_SPLICE: u8 = 30;
/// send.
pub const IORING_OP_SEND: u8 = 26;
/// recv.
pub const IORING_OP_RECV: u8 = 27;
/// Multishot accept.
pub const IORING_OP_ACCEPT_MULTI: u8 = 46;

// ---------------------------------------------------------------------------
// SQE flags
// ---------------------------------------------------------------------------

/// Fixed file (pre-registered fd).
pub const IOSQE_FIXED_FILE: u8 = 1 << 0;
/// Drain (wait for prior ops).
pub const IOSQE_IO_DRAIN: u8 = 1 << 1;
/// Link this SQE to next.
pub const IOSQE_IO_LINK: u8 = 1 << 2;
/// Hard link (fail chain on error).
pub const IOSQE_IO_HARDLINK: u8 = 1 << 3;
/// Async (force async execution).
pub const IOSQE_ASYNC: u8 = 1 << 4;
/// Use registered buffer.
pub const IOSQE_BUFFER_SELECT: u8 = 1 << 5;
/// CQE skip (no completion).
pub const IOSQE_CQE_SKIP_SUCCESS: u8 = 1 << 6;

// ---------------------------------------------------------------------------
// Setup flags (io_uring_setup)
// ---------------------------------------------------------------------------

/// io_uring with io-poll mode.
pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
/// SQ poll thread.
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
/// Bind SQ thread to CPU.
pub const IORING_SETUP_SQ_AFF: u32 = 1 << 2;
/// Per-CQE skip success.
pub const IORING_SETUP_CQSIZE: u32 = 1 << 3;
/// Single issuer (optimization).
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 1 << 12;
/// Defer taskrun.
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            IORING_OP_NOP,
            IORING_OP_READV,
            IORING_OP_WRITEV,
            IORING_OP_FSYNC,
            IORING_OP_READ_FIXED,
            IORING_OP_WRITE_FIXED,
            IORING_OP_POLL_ADD,
            IORING_OP_POLL_REMOVE,
            IORING_OP_SYNC_FILE_RANGE,
            IORING_OP_SENDMSG,
            IORING_OP_RECVMSG,
            IORING_OP_TIMEOUT,
            IORING_OP_TIMEOUT_REMOVE,
            IORING_OP_ACCEPT,
            IORING_OP_ASYNC_CANCEL,
            IORING_OP_LINK_TIMEOUT,
            IORING_OP_CONNECT,
            IORING_OP_FALLOCATE,
            IORING_OP_OPENAT,
            IORING_OP_CLOSE,
            IORING_OP_READ,
            IORING_OP_WRITE,
            IORING_OP_STATX,
            IORING_OP_SPLICE,
            IORING_OP_SEND,
            IORING_OP_RECV,
            IORING_OP_ACCEPT_MULTI,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
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
            IORING_SETUP_IOPOLL,
            IORING_SETUP_SQPOLL,
            IORING_SETUP_SQ_AFF,
            IORING_SETUP_CQSIZE,
            IORING_SETUP_SINGLE_ISSUER,
            IORING_SETUP_DEFER_TASKRUN,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}

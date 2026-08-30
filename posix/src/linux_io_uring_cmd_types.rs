//! `<linux/io_uring.h>` — io_uring opcode and SQE flag constants.
//!
//! io_uring is Linux's high-performance async I/O interface. User
//! submits entries (SQEs) to a submission queue and reaps completions
//! (CQEs) from a completion queue, with minimal/zero syscalls in the
//! fast path. Each opcode defines a specific I/O operation type.

// ---------------------------------------------------------------------------
// io_uring opcodes
// ---------------------------------------------------------------------------

/// No-op (for testing submission path).
pub const IORING_OP_NOP: u32 = 0;
/// Vectored read (preadv).
pub const IORING_OP_READV: u32 = 1;
/// Vectored write (pwritev).
pub const IORING_OP_WRITEV: u32 = 2;
/// Synchronize file (fsync).
pub const IORING_OP_FSYNC: u32 = 3;
/// Read from fixed buffer.
pub const IORING_OP_READ_FIXED: u32 = 4;
/// Write from fixed buffer.
pub const IORING_OP_WRITE_FIXED: u32 = 5;
/// Add to poll set.
pub const IORING_OP_POLL_ADD: u32 = 6;
/// Remove from poll set.
pub const IORING_OP_POLL_REMOVE: u32 = 7;
/// Synchronize data (fdatasync).
pub const IORING_OP_SYNC_FILE_RANGE: u32 = 8;
/// Send message on socket.
pub const IORING_OP_SENDMSG: u32 = 9;
/// Receive message from socket.
pub const IORING_OP_RECVMSG: u32 = 10;
/// Timeout (wait for N completions or time).
pub const IORING_OP_TIMEOUT: u32 = 11;
/// Remove a timeout.
pub const IORING_OP_TIMEOUT_REMOVE: u32 = 12;
/// Accept connection.
pub const IORING_OP_ACCEPT: u32 = 13;
/// Cancel an in-flight request.
pub const IORING_OP_ASYNC_CANCEL: u32 = 14;
/// Link timeout (cancel linked SQE on timeout).
pub const IORING_OP_LINK_TIMEOUT: u32 = 15;
/// Connect to a socket.
pub const IORING_OP_CONNECT: u32 = 16;
/// Allocate file table slots.
pub const IORING_OP_FALLOCATE: u32 = 17;
/// Open a file.
pub const IORING_OP_OPENAT: u32 = 18;
/// Close a file descriptor.
pub const IORING_OP_CLOSE: u32 = 19;
/// Register/update file table.
pub const IORING_OP_FILES_UPDATE: u32 = 20;
/// Get file status (statx).
pub const IORING_OP_STATX: u32 = 21;
/// Simple read.
pub const IORING_OP_READ: u32 = 22;
/// Simple write.
pub const IORING_OP_WRITE: u32 = 23;
/// File advisory (fadvise64).
pub const IORING_OP_FADVISE: u32 = 24;
/// Memory advisory (madvise).
pub const IORING_OP_MADVISE: u32 = 25;
/// Send data.
pub const IORING_OP_SEND: u32 = 26;
/// Receive data.
pub const IORING_OP_RECV: u32 = 27;
/// Open at (openat2).
pub const IORING_OP_OPENAT2: u32 = 28;
/// Splice (pipe data between fds).
pub const IORING_OP_SPLICE: u32 = 30;
/// Rename a file.
pub const IORING_OP_RENAMEAT: u32 = 35;
/// Unlink a file.
pub const IORING_OP_UNLINKAT: u32 = 36;
/// Make directory.
pub const IORING_OP_MKDIRAT: u32 = 37;

// ---------------------------------------------------------------------------
// SQE flags
// ---------------------------------------------------------------------------

/// Use fixed file (registered fd table).
pub const IOSQE_FIXED_FILE: u32 = 1 << 0;
/// Force async execution (never inline).
pub const IOSQE_ASYNC: u32 = 1 << 4;
/// Link this SQE to the next one.
pub const IOSQE_IO_LINK: u32 = 1 << 2;
/// Hard link (fail chain on any error).
pub const IOSQE_IO_HARDLINK: u32 = 1 << 3;
/// Drain prior submissions first.
pub const IOSQE_IO_DRAIN: u32 = 1 << 1;
/// Use registered buffer.
pub const IOSQE_BUFFER_SELECT: u32 = 1 << 5;
/// CQE is 32 bytes (not 16).
pub const IOSQE_CQE_SKIP_SUCCESS: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Setup flags (io_uring_setup)
// ---------------------------------------------------------------------------

/// Use io_poll for completion.
pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
/// Use submission queue polling.
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
/// Bind SQ thread to a CPU.
pub const IORING_SETUP_SQ_AFF: u32 = 1 << 2;
/// Application manages CQ ring size.
pub const IORING_SETUP_CQSIZE: u32 = 1 << 3;
/// Attach to existing wq.
pub const IORING_SETUP_ATTACH_WQ: u32 = 1 << 4;
/// Disable ring (start disabled).
pub const IORING_SETUP_R_DISABLED: u32 = 1 << 6;

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
            IORING_OP_FILES_UPDATE,
            IORING_OP_STATX,
            IORING_OP_READ,
            IORING_OP_WRITE,
            IORING_OP_FADVISE,
            IORING_OP_MADVISE,
            IORING_OP_SEND,
            IORING_OP_RECV,
            IORING_OP_OPENAT2,
            IORING_OP_SPLICE,
            IORING_OP_RENAMEAT,
            IORING_OP_UNLINKAT,
            IORING_OP_MKDIRAT,
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
            assert!(flags[i].is_power_of_two());
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
            IORING_SETUP_ATTACH_WQ,
            IORING_SETUP_R_DISABLED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_nop_is_zero() {
        assert_eq!(IORING_OP_NOP, 0);
    }
}

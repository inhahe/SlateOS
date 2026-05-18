//! `<linux/io_uring.h>` — io_uring operation code constants.
//!
//! Each submission queue entry (SQE) specifies an operation via the
//! `opcode` field. These constants enumerate all supported async I/O
//! operations that can be submitted to an io_uring instance.

// ---------------------------------------------------------------------------
// io_uring opcodes (IORING_OP_*)
// ---------------------------------------------------------------------------

/// No operation (used for testing).
pub const IORING_OP_NOP: u8 = 0;
/// Read from a file (vectored).
pub const IORING_OP_READV: u8 = 1;
/// Write to a file (vectored).
pub const IORING_OP_WRITEV: u8 = 2;
/// fsync a file descriptor.
pub const IORING_OP_FSYNC: u8 = 3;
/// Read from fixed buffer.
pub const IORING_OP_READ_FIXED: u8 = 4;
/// Write from fixed buffer.
pub const IORING_OP_WRITE_FIXED: u8 = 5;
/// Add to poll interest set.
pub const IORING_OP_POLL_ADD: u8 = 6;
/// Remove from poll interest set.
pub const IORING_OP_POLL_REMOVE: u8 = 7;
/// Sync file range.
pub const IORING_OP_SYNC_FILE_RANGE: u8 = 8;
/// sendmsg on a socket.
pub const IORING_OP_SENDMSG: u8 = 9;
/// recvmsg on a socket.
pub const IORING_OP_RECVMSG: u8 = 10;
/// Wait for a timeout.
pub const IORING_OP_TIMEOUT: u8 = 11;
/// Remove a pending timeout.
pub const IORING_OP_TIMEOUT_REMOVE: u8 = 12;
/// Accept a connection.
pub const IORING_OP_ACCEPT: u8 = 13;
/// Cancel an in-flight operation.
pub const IORING_OP_ASYNC_CANCEL: u8 = 14;
/// Link timeout to previous SQE.
pub const IORING_OP_LINK_TIMEOUT: u8 = 15;
/// Connect a socket.
pub const IORING_OP_CONNECT: u8 = 16;
/// fallocate a file.
pub const IORING_OP_FALLOCATE: u8 = 17;
/// Open a file (openat).
pub const IORING_OP_OPENAT: u8 = 18;
/// Close a file descriptor.
pub const IORING_OP_CLOSE: u8 = 19;
/// Update registered files.
pub const IORING_OP_FILES_UPDATE: u8 = 20;
/// statx a file.
pub const IORING_OP_STATX: u8 = 21;
/// Simple read.
pub const IORING_OP_READ: u8 = 22;
/// Simple write.
pub const IORING_OP_WRITE: u8 = 23;
/// fadvise on a file.
pub const IORING_OP_FADVISE: u8 = 24;
/// madvise on memory.
pub const IORING_OP_MADVISE: u8 = 25;
/// Send data on a socket.
pub const IORING_OP_SEND: u8 = 26;
/// Receive data from a socket.
pub const IORING_OP_RECV: u8 = 27;
/// openat2 with extended flags.
pub const IORING_OP_OPENAT2: u8 = 28;
/// Provide buffers to the ring.
pub const IORING_OP_PROVIDE_BUFFERS: u8 = 29;
/// Remove provided buffers.
pub const IORING_OP_REMOVE_BUFFERS: u8 = 30;
/// Tee between two pipes.
pub const IORING_OP_TEE: u8 = 31;
/// Shutdown a socket.
pub const IORING_OP_SHUTDOWN: u8 = 32;
/// renameat operation.
pub const IORING_OP_RENAMEAT: u8 = 33;
/// unlinkat operation.
pub const IORING_OP_UNLINKAT: u8 = 34;
/// mkdirat operation.
pub const IORING_OP_MKDIRAT: u8 = 35;
/// symlinkat operation.
pub const IORING_OP_SYMLINKAT: u8 = 36;
/// linkat operation.
pub const IORING_OP_LINKAT: u8 = 37;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            IORING_OP_NOP, IORING_OP_READV, IORING_OP_WRITEV,
            IORING_OP_FSYNC, IORING_OP_READ_FIXED, IORING_OP_WRITE_FIXED,
            IORING_OP_POLL_ADD, IORING_OP_POLL_REMOVE,
            IORING_OP_SYNC_FILE_RANGE, IORING_OP_SENDMSG,
            IORING_OP_RECVMSG, IORING_OP_TIMEOUT,
            IORING_OP_TIMEOUT_REMOVE, IORING_OP_ACCEPT,
            IORING_OP_ASYNC_CANCEL, IORING_OP_LINK_TIMEOUT,
            IORING_OP_CONNECT, IORING_OP_FALLOCATE,
            IORING_OP_OPENAT, IORING_OP_CLOSE,
            IORING_OP_FILES_UPDATE, IORING_OP_STATX,
            IORING_OP_READ, IORING_OP_WRITE,
            IORING_OP_FADVISE, IORING_OP_MADVISE,
            IORING_OP_SEND, IORING_OP_RECV,
            IORING_OP_OPENAT2, IORING_OP_PROVIDE_BUFFERS,
            IORING_OP_REMOVE_BUFFERS, IORING_OP_TEE,
            IORING_OP_SHUTDOWN, IORING_OP_RENAMEAT,
            IORING_OP_UNLINKAT, IORING_OP_MKDIRAT,
            IORING_OP_SYMLINKAT, IORING_OP_LINKAT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_nop_is_zero() {
        assert_eq!(IORING_OP_NOP, 0);
    }

    #[test]
    fn test_sequential_io_ops() {
        assert_eq!(IORING_OP_READV, 1);
        assert_eq!(IORING_OP_WRITEV, 2);
        assert_eq!(IORING_OP_FSYNC, 3);
    }

    #[test]
    fn test_last_op() {
        assert_eq!(IORING_OP_LINKAT, 37);
    }
}

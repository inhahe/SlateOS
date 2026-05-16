//! `<linux/io_uring.h>` — io_uring asynchronous I/O interface.
//!
//! Provides data structures and constants for the io_uring
//! submission/completion queue interface.

use crate::errno;

// ---------------------------------------------------------------------------
// io_uring_setup flags
// ---------------------------------------------------------------------------

/// Create I/O poll (busy-wait) mode.
pub const IORING_SETUP_IOPOLL: u32 = 1;
/// SQ poll thread (kernel-side submission polling).
pub const IORING_SETUP_SQPOLL: u32 = 2;
/// Bind SQ poll thread to a CPU.
pub const IORING_SETUP_SQ_AFF: u32 = 4;
/// Use fixed-size CQ ring.
pub const IORING_SETUP_CQSIZE: u32 = 8;
/// Clamp ring sizes.
pub const IORING_SETUP_CLAMP: u32 = 16;
/// Attach to existing wq.
pub const IORING_SETUP_ATTACH_WQ: u32 = 32;
/// Start disabled (requires IORING_REGISTER_ENABLE_RINGS).
pub const IORING_SETUP_R_DISABLED: u32 = 64;
/// Use a single issuer.
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 256;

// ---------------------------------------------------------------------------
// io_uring opcodes (SQE operations)
// ---------------------------------------------------------------------------

/// No-op.
pub const IORING_OP_NOP: u8 = 0;
/// Read (vectored).
pub const IORING_OP_READV: u8 = 1;
/// Write (vectored).
pub const IORING_OP_WRITEV: u8 = 2;
/// fsync.
pub const IORING_OP_FSYNC: u8 = 3;
/// Read (fixed buffer).
pub const IORING_OP_READ_FIXED: u8 = 4;
/// Write (fixed buffer).
pub const IORING_OP_WRITE_FIXED: u8 = 5;
/// Add poll.
pub const IORING_OP_POLL_ADD: u8 = 6;
/// Remove poll.
pub const IORING_OP_POLL_REMOVE: u8 = 7;
/// Sync file range.
pub const IORING_OP_SYNC_FILE_RANGE: u8 = 8;
/// Send message.
pub const IORING_OP_SENDMSG: u8 = 9;
/// Receive message.
pub const IORING_OP_RECVMSG: u8 = 10;
/// Timeout.
pub const IORING_OP_TIMEOUT: u8 = 11;
/// Remove timeout.
pub const IORING_OP_TIMEOUT_REMOVE: u8 = 12;
/// Accept connection.
pub const IORING_OP_ACCEPT: u8 = 13;
/// Cancel async operation.
pub const IORING_OP_ASYNC_CANCEL: u8 = 14;
/// Link timeout.
pub const IORING_OP_LINK_TIMEOUT: u8 = 15;
/// Connect.
pub const IORING_OP_CONNECT: u8 = 16;
/// fallocate.
pub const IORING_OP_FALLOCATE: u8 = 17;
/// Open file.
pub const IORING_OP_OPENAT: u8 = 18;
/// Close file.
pub const IORING_OP_CLOSE: u8 = 19;
/// Read.
pub const IORING_OP_READ: u8 = 22;
/// Write.
pub const IORING_OP_WRITE: u8 = 23;
/// fadvise.
pub const IORING_OP_FADVISE: u8 = 24;
/// madvise.
pub const IORING_OP_MADVISE: u8 = 25;
/// Send.
pub const IORING_OP_SEND: u8 = 26;
/// Receive.
pub const IORING_OP_RECV: u8 = 27;
/// Open file (openat2).
pub const IORING_OP_OPENAT2: u8 = 28;
/// statx.
pub const IORING_OP_STATX: u8 = 21;
/// Provide buffers.
pub const IORING_OP_PROVIDE_BUFFERS: u8 = 31;
/// Remove buffers.
pub const IORING_OP_REMOVE_BUFFERS: u8 = 32;
/// Rename.
pub const IORING_OP_RENAMEAT: u8 = 35;
/// Unlink.
pub const IORING_OP_UNLINKAT: u8 = 36;
/// mkdir.
pub const IORING_OP_MKDIRAT: u8 = 37;
/// symlink.
pub const IORING_OP_SYMLINKAT: u8 = 38;
/// link.
pub const IORING_OP_LINKAT: u8 = 39;
/// Cancel all.
pub const IORING_OP_CANCEL: u8 = 48;

// ---------------------------------------------------------------------------
// SQE flags
// ---------------------------------------------------------------------------

/// Fixed file (uses registered file set).
pub const IOSQE_FIXED_FILE: u8 = 1;
/// Drain I/O (ensure previous ops complete first).
pub const IOSQE_IO_DRAIN: u8 = 2;
/// Link this SQE to the next.
pub const IOSQE_IO_LINK: u8 = 4;
/// Hard link (fail dependent on error).
pub const IOSQE_IO_HARDLINK: u8 = 8;
/// Run async (don't inline).
pub const IOSQE_ASYNC: u8 = 16;
/// Use registered buffer.
pub const IOSQE_BUFFER_SELECT: u8 = 32;

// ---------------------------------------------------------------------------
// CQE flags
// ---------------------------------------------------------------------------

/// More CQEs for this SQE.
pub const IORING_CQE_F_BUFFER: u32 = 1;
/// More data available.
pub const IORING_CQE_F_MORE: u32 = 2;
/// Socket is readable.
pub const IORING_CQE_F_SOCK_NONEMPTY: u32 = 4;
/// Notification CQE.
pub const IORING_CQE_F_NOTIF: u32 = 8;

// ---------------------------------------------------------------------------
// io_uring_enter flags
// ---------------------------------------------------------------------------

/// Submit and wait for completions.
pub const IORING_ENTER_GETEVENTS: u32 = 1;
/// Wake SQ poll thread.
pub const IORING_ENTER_SQ_WAKEUP: u32 = 2;
/// Wait for SQ space.
pub const IORING_ENTER_SQ_WAIT: u32 = 4;
/// Extended argument.
pub const IORING_ENTER_EXT_ARG: u32 = 8;

// ---------------------------------------------------------------------------
// io_uring_register operations
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
/// Update registered files.
pub const IORING_REGISTER_FILES_UPDATE: u32 = 6;
/// Register eventfd (async only).
pub const IORING_REGISTER_EVENTFD_ASYNC: u32 = 7;
/// Register probe.
pub const IORING_REGISTER_PROBE: u32 = 8;
/// Enable rings.
pub const IORING_REGISTER_ENABLE_RINGS: u32 = 16;

// ---------------------------------------------------------------------------
// Submission Queue Entry (SQE)
// ---------------------------------------------------------------------------

/// io_uring submission queue entry.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoUringSqe {
    /// Opcode (IORING_OP_*).
    pub opcode: u8,
    /// Flags (IOSQE_*).
    pub flags: u8,
    /// I/O priority.
    pub ioprio: u16,
    /// File descriptor.
    pub fd: i32,
    /// Offset or addr2.
    pub off: u64,
    /// Buffer address or splice_off_in.
    pub addr: u64,
    /// Buffer length.
    pub len: u32,
    /// Operation-specific flags.
    pub op_flags: u32,
    /// User data (returned in CQE).
    pub user_data: u64,
    /// Buffer index or group.
    pub buf_index: u16,
    /// Personality.
    pub personality: u16,
    /// Splice fd in.
    pub splice_fd_in: i32,
    /// Address 3 (extended).
    pub addr3: u64,
    /// Padding.
    _pad2: u64,
}

/// io_uring completion queue entry.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct IoUringCqe {
    /// User data from the SQE.
    pub user_data: u64,
    /// Result (positive = success, negative = -errno).
    pub res: i32,
    /// Flags (IORING_CQE_F_*).
    pub flags: u32,
}

/// io_uring parameters (returned by io_uring_setup).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoUringParams {
    /// SQ entries.
    pub sq_entries: u32,
    /// CQ entries.
    pub cq_entries: u32,
    /// Flags (IORING_SETUP_*).
    pub flags: u32,
    /// SQ thread CPU.
    pub sq_thread_cpu: u32,
    /// SQ thread idle timeout (ms).
    pub sq_thread_idle: u32,
    /// Features supported.
    pub features: u32,
    /// WQ fd (for ATTACH_WQ).
    pub wq_fd: u32,
    /// Reserved.
    pub resv: [u32; 3],
    /// SQ ring offsets.
    pub sq_off: IoSqringOffsets,
    /// CQ ring offsets.
    pub cq_off: IoCqringOffsets,
}

/// Submission queue ring offsets.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoSqringOffsets {
    /// Offset to head.
    pub head: u32,
    /// Offset to tail.
    pub tail: u32,
    /// Offset to ring mask.
    pub ring_mask: u32,
    /// Offset to ring entries count.
    pub ring_entries: u32,
    /// Offset to flags.
    pub flags: u32,
    /// Offset to dropped count.
    pub dropped: u32,
    /// Offset to SQE array.
    pub array: u32,
    /// Reserved.
    pub resv1: u32,
    /// User address.
    pub user_addr: u64,
}

/// Completion queue ring offsets.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoCqringOffsets {
    /// Offset to head.
    pub head: u32,
    /// Offset to tail.
    pub tail: u32,
    /// Offset to ring mask.
    pub ring_mask: u32,
    /// Offset to ring entries count.
    pub ring_entries: u32,
    /// Offset to overflow count.
    pub overflow: u32,
    /// Offset to CQE array.
    pub cqes: u32,
    /// Offset to flags.
    pub flags: u32,
    /// Reserved.
    pub resv1: u32,
    /// User address.
    pub user_addr: u64,
}

// ---------------------------------------------------------------------------
// Syscall wrappers
// ---------------------------------------------------------------------------

/// Set up an io_uring instance.
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_uring_setup(_entries: u32, _params: *mut IoUringParams) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Submit and/or wait for io_uring operations.
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_uring_enter(
    _fd: i32,
    _to_submit: u32,
    _min_complete: u32,
    _flags: u32,
    _sig: *const u8,
    _sigsz: usize,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Register resources with an io_uring instance.
///
/// Stub — returns `-1` / sets `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_uring_register(
    _fd: i32,
    _opcode: u32,
    _arg: *mut u8,
    _nr_args: u32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqe_size() {
        assert_eq!(core::mem::size_of::<IoUringSqe>(), 64);
    }

    #[test]
    fn test_cqe_size() {
        assert_eq!(core::mem::size_of::<IoUringCqe>(), 16);
    }

    #[test]
    fn test_params_size() {
        assert!(core::mem::size_of::<IoUringParams>() >= 100);
    }

    #[test]
    fn test_opcodes_distinct() {
        let ops = [
            IORING_OP_NOP, IORING_OP_READV, IORING_OP_WRITEV,
            IORING_OP_FSYNC, IORING_OP_READ_FIXED, IORING_OP_WRITE_FIXED,
            IORING_OP_POLL_ADD, IORING_OP_POLL_REMOVE,
            IORING_OP_SENDMSG, IORING_OP_RECVMSG,
            IORING_OP_TIMEOUT, IORING_OP_ACCEPT,
            IORING_OP_READ, IORING_OP_WRITE,
            IORING_OP_CLOSE, IORING_OP_OPENAT,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_setup_flags_are_bits() {
        let flags = [
            IORING_SETUP_IOPOLL, IORING_SETUP_SQPOLL,
            IORING_SETUP_SQ_AFF, IORING_SETUP_CQSIZE,
            IORING_SETUP_CLAMP, IORING_SETUP_ATTACH_WQ,
            IORING_SETUP_R_DISABLED,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "Setup flags must not overlap");
            }
        }
    }

    #[test]
    fn test_sqe_flags_are_bits() {
        let flags = [
            IOSQE_FIXED_FILE, IOSQE_IO_DRAIN, IOSQE_IO_LINK,
            IOSQE_IO_HARDLINK, IOSQE_ASYNC, IOSQE_BUFFER_SELECT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_enter_flags() {
        assert_eq!(IORING_ENTER_GETEVENTS, 1);
        assert_eq!(IORING_ENTER_SQ_WAKEUP, 2);
    }

    #[test]
    fn test_register_ops_distinct() {
        let ops = [
            IORING_REGISTER_BUFFERS, IORING_UNREGISTER_BUFFERS,
            IORING_REGISTER_FILES, IORING_UNREGISTER_FILES,
            IORING_REGISTER_EVENTFD, IORING_UNREGISTER_EVENTFD,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_io_uring_setup_stub() {
        let mut params: IoUringParams = unsafe { core::mem::zeroed() };
        assert_eq!(io_uring_setup(32, &mut params), -1);
    }

    #[test]
    fn test_io_uring_enter_stub() {
        assert_eq!(io_uring_enter(0, 0, 0, 0, core::ptr::null(), 0), -1);
    }
}

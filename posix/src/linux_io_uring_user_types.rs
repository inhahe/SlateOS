//! `<linux/io_uring.h>` — io_uring submission/completion ring ABI.
//!
//! io_uring is the highest-throughput Linux I/O interface: a pair of
//! shared-memory rings between userspace and the kernel that submits
//! batches of operations without per-op syscalls. PostgreSQL, RocksDB,
//! Ceph, libuv, and io_uring-backed `cp` all rely on the opcodes,
//! flags, and feature bits below.

// ---------------------------------------------------------------------------
// Syscall numbers (architecture-independent uapi values)
// ---------------------------------------------------------------------------

/// `io_uring_setup(entries, params)` — system call number on x86_64.
pub const NR_IO_URING_SETUP: u32 = 425;
/// `io_uring_enter` — submit & wait.
pub const NR_IO_URING_ENTER: u32 = 426;
/// `io_uring_register` — pin fixed buffers/files.
pub const NR_IO_URING_REGISTER: u32 = 427;

// ---------------------------------------------------------------------------
// `struct io_uring_params.flags`
// ---------------------------------------------------------------------------

pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
pub const IORING_SETUP_SQ_AFF: u32 = 1 << 2;
pub const IORING_SETUP_CQSIZE: u32 = 1 << 3;
pub const IORING_SETUP_CLAMP: u32 = 1 << 4;
pub const IORING_SETUP_ATTACH_WQ: u32 = 1 << 5;
pub const IORING_SETUP_R_DISABLED: u32 = 1 << 6;
pub const IORING_SETUP_SUBMIT_ALL: u32 = 1 << 7;
pub const IORING_SETUP_COOP_TASKRUN: u32 = 1 << 8;
pub const IORING_SETUP_TASKRUN_FLAG: u32 = 1 << 9;
pub const IORING_SETUP_SQE128: u32 = 1 << 10;
pub const IORING_SETUP_CQE32: u32 = 1 << 11;
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 1 << 12;
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// `enum io_uring_op` — opcodes
// ---------------------------------------------------------------------------

pub const IORING_OP_NOP: u32 = 0;
pub const IORING_OP_READV: u32 = 1;
pub const IORING_OP_WRITEV: u32 = 2;
pub const IORING_OP_FSYNC: u32 = 3;
pub const IORING_OP_READ_FIXED: u32 = 4;
pub const IORING_OP_WRITE_FIXED: u32 = 5;
pub const IORING_OP_POLL_ADD: u32 = 6;
pub const IORING_OP_POLL_REMOVE: u32 = 7;
pub const IORING_OP_SYNC_FILE_RANGE: u32 = 8;
pub const IORING_OP_SENDMSG: u32 = 9;
pub const IORING_OP_RECVMSG: u32 = 10;
pub const IORING_OP_TIMEOUT: u32 = 11;
pub const IORING_OP_TIMEOUT_REMOVE: u32 = 12;
pub const IORING_OP_ACCEPT: u32 = 13;
pub const IORING_OP_ASYNC_CANCEL: u32 = 14;
pub const IORING_OP_LINK_TIMEOUT: u32 = 15;
pub const IORING_OP_CONNECT: u32 = 16;
pub const IORING_OP_FALLOCATE: u32 = 17;
pub const IORING_OP_OPENAT: u32 = 18;
pub const IORING_OP_CLOSE: u32 = 19;
pub const IORING_OP_FILES_UPDATE: u32 = 20;
pub const IORING_OP_STATX: u32 = 21;
pub const IORING_OP_READ: u32 = 22;
pub const IORING_OP_WRITE: u32 = 23;
pub const IORING_OP_FADVISE: u32 = 24;
pub const IORING_OP_MADVISE: u32 = 25;
pub const IORING_OP_SEND: u32 = 26;
pub const IORING_OP_RECV: u32 = 27;
pub const IORING_OP_OPENAT2: u32 = 28;
pub const IORING_OP_EPOLL_CTL: u32 = 29;
pub const IORING_OP_SPLICE: u32 = 30;
pub const IORING_OP_PROVIDE_BUFFERS: u32 = 31;
pub const IORING_OP_REMOVE_BUFFERS: u32 = 32;
pub const IORING_OP_TEE: u32 = 33;
pub const IORING_OP_SHUTDOWN: u32 = 34;
pub const IORING_OP_RENAMEAT: u32 = 35;
pub const IORING_OP_UNLINKAT: u32 = 36;
pub const IORING_OP_MKDIRAT: u32 = 37;
pub const IORING_OP_SYMLINKAT: u32 = 38;
pub const IORING_OP_LINKAT: u32 = 39;
pub const IORING_OP_MSG_RING: u32 = 40;
pub const IORING_OP_FSETXATTR: u32 = 41;
pub const IORING_OP_SETXATTR: u32 = 42;
pub const IORING_OP_FGETXATTR: u32 = 43;
pub const IORING_OP_GETXATTR: u32 = 44;
pub const IORING_OP_SOCKET: u32 = 45;
pub const IORING_OP_URING_CMD: u32 = 46;

// ---------------------------------------------------------------------------
// `struct io_uring_sqe.flags`
// ---------------------------------------------------------------------------

pub const IOSQE_FIXED_FILE: u8 = 1 << 0;
pub const IOSQE_IO_DRAIN: u8 = 1 << 1;
pub const IOSQE_IO_LINK: u8 = 1 << 2;
pub const IOSQE_IO_HARDLINK: u8 = 1 << 3;
pub const IOSQE_ASYNC: u8 = 1 << 4;
pub const IOSQE_BUFFER_SELECT: u8 = 1 << 5;
pub const IOSQE_CQE_SKIP_SUCCESS: u8 = 1 << 6;

// ---------------------------------------------------------------------------
// `io_uring_enter` flags
// ---------------------------------------------------------------------------

pub const IORING_ENTER_GETEVENTS: u32 = 1 << 0;
pub const IORING_ENTER_SQ_WAKEUP: u32 = 1 << 1;
pub const IORING_ENTER_SQ_WAIT: u32 = 1 << 2;
pub const IORING_ENTER_EXT_ARG: u32 = 1 << 3;
pub const IORING_ENTER_REGISTERED_RING: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Feature bits reported in `io_uring_params.features`
// ---------------------------------------------------------------------------

pub const IORING_FEAT_SINGLE_MMAP: u32 = 1 << 0;
pub const IORING_FEAT_NODROP: u32 = 1 << 1;
pub const IORING_FEAT_SUBMIT_STABLE: u32 = 1 << 2;
pub const IORING_FEAT_RW_CUR_POS: u32 = 1 << 3;
pub const IORING_FEAT_CUR_PERSONALITY: u32 = 1 << 4;
pub const IORING_FEAT_FAST_POLL: u32 = 1 << 5;
pub const IORING_FEAT_POLL_32BITS: u32 = 1 << 6;
pub const IORING_FEAT_SQPOLL_NONFIXED: u32 = 1 << 7;
pub const IORING_FEAT_EXT_ARG: u32 = 1 << 8;
pub const IORING_FEAT_NATIVE_WORKERS: u32 = 1 << 9;
pub const IORING_FEAT_RSRC_TAGS: u32 = 1 << 10;
pub const IORING_FEAT_CQE_SKIP: u32 = 1 << 11;
pub const IORING_FEAT_LINKED_FILE: u32 = 1 << 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscalls_consecutive() {
        // setup, enter, register: 425, 426, 427 on x86_64.
        assert_eq!(NR_IO_URING_ENTER, NR_IO_URING_SETUP + 1);
        assert_eq!(NR_IO_URING_REGISTER, NR_IO_URING_SETUP + 2);
    }

    #[test]
    fn test_setup_flags_pow2_and_distinct() {
        let f = [
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
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_opcodes_dense_0_to_46() {
        let o = [
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
            IORING_OP_EPOLL_CTL,
            IORING_OP_SPLICE,
            IORING_OP_PROVIDE_BUFFERS,
            IORING_OP_REMOVE_BUFFERS,
            IORING_OP_TEE,
            IORING_OP_SHUTDOWN,
            IORING_OP_RENAMEAT,
            IORING_OP_UNLINKAT,
            IORING_OP_MKDIRAT,
            IORING_OP_SYMLINKAT,
            IORING_OP_LINKAT,
            IORING_OP_MSG_RING,
            IORING_OP_FSETXATTR,
            IORING_OP_SETXATTR,
            IORING_OP_FGETXATTR,
            IORING_OP_GETXATTR,
            IORING_OP_SOCKET,
            IORING_OP_URING_CMD,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_sqe_flags_pow2() {
        for &b in &[
            IOSQE_FIXED_FILE,
            IOSQE_IO_DRAIN,
            IOSQE_IO_LINK,
            IOSQE_IO_HARDLINK,
            IOSQE_ASYNC,
            IOSQE_BUFFER_SELECT,
            IOSQE_CQE_SKIP_SUCCESS,
        ] {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_enter_flags_pow2_and_distinct() {
        let e = [
            IORING_ENTER_GETEVENTS,
            IORING_ENTER_SQ_WAKEUP,
            IORING_ENTER_SQ_WAIT,
            IORING_ENTER_EXT_ARG,
            IORING_ENTER_REGISTERED_RING,
        ];
        for &b in &e {
            assert!(b.is_power_of_two());
        }
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
    }

    #[test]
    fn test_feature_bits_pow2() {
        for &b in &[
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
        ] {
            assert!(b.is_power_of_two());
        }
    }
}

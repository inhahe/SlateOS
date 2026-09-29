//! `<linux/io_uring.h>` — io_uring asynchronous I/O interface.
//!
//! Provides data structures and constants for the io_uring
//! submission/completion queue interface, plus a real input-validator
//! front end for the three syscalls (`io_uring_setup`,
//! `io_uring_enter`, `io_uring_register`).
//!
//! There is no io_uring on SlateOS -- no ring pages, no SQPOLL thread, no
//! per-operation verbs -- so the three calls refuse what Linux 6.6 refuses,
//! in its order (io_uring/io_uring.c, io_uring/sqpoll.c), and then say so:
//!
//! * `io_uring_setup` checks what Linux checks before it allocates the rings
//!   -- the parameter block (`EFAULT`), its reserved words, its flags, the
//!   sizes, the flag combinations (`EINVAL`) -- and then answers `ENOSYS`,
//!   which is what programs that probe for io_uring (liburing's
//!   `io_uring_queue_init`, tokio-uring, glommio, PostgreSQL's
//!   `io_method`) test for before falling back to `epoll` or threads.  The
//!   checks Linux makes after allocating the rings (the `ATTACH_WQ`
//!   descriptor, `SQ_AFF`'s CPU) are not made: there is nothing they could
//!   be about.
//! * `io_uring_enter` and `io_uring_register` check their flags or opcode,
//!   then look for the ring -- and there is none: a descriptor that is not
//!   open is `EBADF`, an open one is not a ring (`EOPNOTSUPP`), and a
//!   registered-ring index has no table to be in (`EINVAL`).  Linux's later
//!   checks are about a ring, and never reached.
//!
//! Until 2026-09-26 these validated in an order of their own, with checks
//! Linux 6.6 does not have -- `min_complete` and signal-set sizes before the
//! descriptor, per-operation argument shapes before the ring, `SQPOLL` with
//! `IOPOLL` refused, `SQPOLL` gated on `CAP_SYS_NICE` -- and flag sets from
//! later kernels (`B-D-IO-URING-WAS-NOT-LINUXS`).
//!
//! ## Reached through `syscall()`
//!
//! glibc wraps none of these calls: programs -- liburing above all -- make them with
//! `syscall(SYS_io_uring_setup, …)`, which answers with the checks below and then
//! `ENOSYS`, as the kernel's own Linux table does.  Until 2026-09-26 the C
//! library also exported `io_uring_setup`, `io_uring_enter` and `io_uring_register` under their own names -- liburing's, which it defines itself since 2.2, which glibc does not, and
//! `syscall()` answered `ENOSYS` without a look; the names went as libaio's
//! did (design-decisions.md §1114).

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
/// Submit-all on enter (rather than draining the SQ).
pub const IORING_SETUP_SUBMIT_ALL: u32 = 128;
/// Tasks run their completion work when they next enter the kernel, not
/// at an interrupt.
pub const IORING_SETUP_COOP_TASKRUN: u32 = 1 << 8;
/// With `COOP_TASKRUN`: set `IORING_SQ_TASKRUN` in the SQ ring when there
/// is task work to run.
pub const IORING_SETUP_TASKRUN_FLAG: u32 = 1 << 9;
/// 128-byte SQEs.
pub const IORING_SETUP_SQE128: u32 = 1 << 10;
/// 32-byte CQEs.
pub const IORING_SETUP_CQE32: u32 = 1 << 11;
/// Only one task submits.  This was `1 << 8` until 2026-09-26 -- Linux's
/// `COOP_TASKRUN` -- so a program asking for one got the other.
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 1 << 12;
/// Run task work only when the task waits for completions.
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 1 << 13;
/// The application provides the rings' memory (Linux 6.5+).
pub const IORING_SETUP_NO_MMAP: u32 = 1 << 14;
/// Register the ring in itself and return its index, not a descriptor;
/// needs `NO_MMAP` (Linux 6.5+).
pub const IORING_SETUP_REGISTERED_FD_ONLY: u32 = 1 << 15;
/// No SQ index array (Linux 6.6+).
pub const IORING_SETUP_NO_SQARRAY: u32 = 1 << 16;
/// Hybrid I/O polling (Linux 6.13+; refused, as 6.6 refuses it).
pub const IORING_SETUP_HYBRID_IOPOLL: u32 = 1 << 17;
/// The flags Linux 6.6's `io_uring_setup` accepts (io_uring.c:4074): bits
/// 0 to 16.  `IORING_SETUP_HYBRID_IOPOLL` is 6.13's, and refused.
const IORING_SETUP_FLAGS_VALID: u32 = IORING_SETUP_IOPOLL
    | IORING_SETUP_SQPOLL
    | IORING_SETUP_SQ_AFF
    | IORING_SETUP_CQSIZE
    | IORING_SETUP_CLAMP
    | IORING_SETUP_ATTACH_WQ
    | IORING_SETUP_R_DISABLED
    | IORING_SETUP_SUBMIT_ALL
    | IORING_SETUP_COOP_TASKRUN
    | IORING_SETUP_TASKRUN_FLAG
    | IORING_SETUP_SQE128
    | IORING_SETUP_CQE32
    | IORING_SETUP_SINGLE_ISSUER
    | IORING_SETUP_DEFER_TASKRUN
    | IORING_SETUP_NO_MMAP
    | IORING_SETUP_REGISTERED_FD_ONLY
    | IORING_SETUP_NO_SQARRAY;

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
/// statx.
pub const IORING_OP_STATX: u8 = 21;
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
/// Cancel (extended).
pub const IORING_OP_CANCEL: u8 = 48;
/// First unknown opcode — anything ≥ this is rejected by SQE
/// validation in real implementations. We use a generous 64 to allow
/// for Linux 6.x opcodes we haven't enumerated above.
pub const IORING_OP_LAST: u8 = 64;

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
/// Registered ring (Linux 5.18+).
pub const IORING_ENTER_REGISTERED_RING: u32 = 16;
/// Abs timeout (Linux 6.12+).
pub const IORING_ENTER_ABS_TIMER: u32 = 32;
/// Extended argument is io_uring_getevents_arg (Linux 6.13+).
pub const IORING_ENTER_EXT_ARG_REG: u32 = 64;
/// The flags Linux 6.6's `io_uring_enter` accepts (io_uring.c:3609).
/// `ABS_TIMER` and `EXT_ARG_REG` are later kernels', and refused.
const IORING_ENTER_FLAGS_VALID: u32 = IORING_ENTER_GETEVENTS
    | IORING_ENTER_SQ_WAKEUP
    | IORING_ENTER_SQ_WAIT
    | IORING_ENTER_EXT_ARG
    | IORING_ENTER_REGISTERED_RING;

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
/// Register personality.
pub const IORING_REGISTER_PERSONALITY: u32 = 9;
/// Unregister personality.
pub const IORING_UNREGISTER_PERSONALITY: u32 = 10;
/// Restrictions.
pub const IORING_REGISTER_RESTRICTIONS: u32 = 11;
/// Enable rings.
pub const IORING_REGISTER_ENABLE_RINGS: u32 = 12;
/// Register file slot update.
pub const IORING_REGISTER_FILES2: u32 = 13;
/// Register buffer slot update.
pub const IORING_REGISTER_BUFFERS2: u32 = 15;
/// Buffer-tagged update.
pub const IORING_REGISTER_BUFFERS_UPDATE: u32 = 16;
/// IOWQ affinity.
pub const IORING_REGISTER_IOWQ_AFF: u32 = 17;
/// Unregister IOWQ affinity.
pub const IORING_UNREGISTER_IOWQ_AFF: u32 = 18;
/// IOWQ max workers.
pub const IORING_REGISTER_IOWQ_MAX_WORKERS: u32 = 19;
/// Register the io_uring fd itself.
pub const IORING_REGISTER_RING_FDS: u32 = 20;
/// Unregister registered ring fd.
pub const IORING_UNREGISTER_RING_FDS: u32 = 21;
/// Buffer pgroup.
pub const IORING_REGISTER_PBUF_RING: u32 = 22;
/// Unregister buffer pgroup.
pub const IORING_UNREGISTER_PBUF_RING: u32 = 23;
/// Sync cancel.
pub const IORING_REGISTER_SYNC_CANCEL: u32 = 24;
/// File alloc range.
pub const IORING_REGISTER_FILE_ALLOC_RANGE: u32 = 25;
/// PBUF status.
pub const IORING_REGISTER_PBUF_STATUS: u32 = 26;
/// Linux 6.6's `IORING_REGISTER_LAST`: its operations stop at
/// `IORING_REGISTER_FILE_ALLOC_RANGE`, so `PBUF_STATUS` and later are
/// refused.
const IORING_REGISTER_LAST: u32 = 26;
/// Or'd into `io_uring_register`'s opcode: `fd` is a registered ring index.
pub const IORING_REGISTER_USE_REGISTERED_RING: u32 = 1 << 31;

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
// Bounds
// ---------------------------------------------------------------------------

/// Linux 6.6's `IORING_MAX_ENTRIES`: the most SQ entries without
/// `IORING_SETUP_CLAMP`.
const IORING_MAX_ENTRIES: u32 = 32_768;
/// Linux 6.6's `IORING_MAX_CQ_ENTRIES`.
const IORING_MAX_CQ_ENTRIES: u32 = 2 * IORING_MAX_ENTRIES;

// ---------------------------------------------------------------------------
// The three calls
// ---------------------------------------------------------------------------

/// What Linux 6.6's `io_uring_setup` and `io_uring_create` refuse before
/// they allocate the rings, in their order.
fn check_setup(entries: u32, p: &IoUringParams) -> Result<(), i32> {
    if p.resv != [0; 3] {
        return Err(errno::EINVAL);
    }
    if p.flags & !IORING_SETUP_FLAGS_VALID != 0 {
        return Err(errno::EINVAL);
    }
    if entries == 0 {
        return Err(errno::EINVAL);
    }
    let clamp = p.flags & IORING_SETUP_CLAMP != 0;
    let entries = if entries <= IORING_MAX_ENTRIES {
        entries
    } else if clamp {
        IORING_MAX_ENTRIES
    } else {
        return Err(errno::EINVAL);
    };
    if p.flags & IORING_SETUP_REGISTERED_FD_ONLY != 0 && p.flags & IORING_SETUP_NO_MMAP == 0 {
        return Err(errno::EINVAL);
    }
    if p.flags & IORING_SETUP_CQSIZE != 0 {
        // Compared after both round up to a power of two, as Linux rounds
        // them; neither can overflow, both being clamped first.
        let cq = match p.cq_entries {
            0 => return Err(errno::EINVAL),
            n if n <= IORING_MAX_CQ_ENTRIES => n,
            _ if clamp => IORING_MAX_CQ_ENTRIES,
            _ => return Err(errno::EINVAL),
        };
        if cq.next_power_of_two() < entries.next_power_of_two() {
            return Err(errno::EINVAL);
        }
    }
    let f = p.flags;
    if f & IORING_SETUP_SQPOLL != 0 {
        // IPI-related flags make no sense with SQPOLL.
        if f & (IORING_SETUP_COOP_TASKRUN | IORING_SETUP_TASKRUN_FLAG | IORING_SETUP_DEFER_TASKRUN)
            != 0
        {
            return Err(errno::EINVAL);
        }
    } else if f & IORING_SETUP_COOP_TASKRUN == 0
        && f & IORING_SETUP_TASKRUN_FLAG != 0
        && f & IORING_SETUP_DEFER_TASKRUN == 0
    {
        return Err(errno::EINVAL);
    }
    if f & IORING_SETUP_DEFER_TASKRUN != 0 && f & IORING_SETUP_SINGLE_ISSUER == 0 {
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Set up an io_uring instance -- which SlateOS does not have.
///
/// Refuses what Linux 6.6 refuses before it allocates the rings, in its
/// order: `EFAULT` for a parameter block it cannot read, then `EINVAL` for
/// a reserved word, an unknown flag, no entries or too many without
/// `IORING_SETUP_CLAMP`, `REGISTERED_FD_ONLY` without `NO_MMAP`, a bad
/// `CQSIZE`, a flag combination it refuses.  Everything else is `ENOSYS`.
pub extern "C" fn io_uring_setup(entries: u32, params: *mut IoUringParams) -> i32 {
    // `copy_from_user(&p, params, sizeof(p))`, first.
    if params.is_null() || !crate::uio::access_ok(params.addr(), size_of::<IoUringParams>()) {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-NULL and in user memory (checked); `read_unaligned`
    // because the caller's block need not be aligned for the Rust type.
    let p = unsafe { core::ptr::read_unaligned(params) };
    if let Err(e) = check_setup(entries, &p) {
        errno::set_errno(e);
        return -1;
    }
    // Linux would allocate the rings here.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Where Linux 6.6 looks for the ring `io_uring_enter` and
/// `io_uring_register` act on, and why it finds none: SlateOS has no rings.
///
/// A registered-ring index needs the task's io_uring context, which exists
/// only once the task has used a ring: `EINVAL`, as for an index past
/// `IO_RINGFD_REG_MAX`.  Otherwise a descriptor that is not open is
/// `EBADF`, and an open one is not a ring: `EOPNOTSUPP`.
fn no_ring(fd: i32, registered: bool) -> i32 {
    let e = if registered {
        errno::EINVAL
    } else if crate::fdtable::get_fd(fd).is_none() {
        errno::EBADF
    } else {
        errno::EOPNOTSUPP
    };
    errno::set_errno(e);
    -1
}

/// Submit and/or wait for io_uring operations.
///
/// Linux 6.6's order: unknown flag bits are `EINVAL`, then the ring is looked
/// for -- see [`no_ring`].  Its later checks (`EXT_ARG`'s argument, the
/// signal set, the counts) are about a ring, so on SlateOS they are never
/// reached, as they are not on Linux for a descriptor that is not one.
pub extern "C" fn io_uring_enter(
    fd: i32,
    _to_submit: u32,
    _min_complete: u32,
    flags: u32,
    _arg: *const u8,
    _argsz: usize,
) -> i32 {
    if flags & !IORING_ENTER_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    no_ring(fd, flags & IORING_ENTER_REGISTERED_RING != 0)
}

/// Register resources with an io_uring instance.
///
/// Linux 6.6's order: an opcode past `IORING_REGISTER_LAST` (once
/// `IORING_REGISTER_USE_REGISTERED_RING` is taken off it) is `EINVAL`, then
/// the ring is looked for -- see [`no_ring`].  The per-operation checks
/// come after, and are never reached.
pub extern "C" fn io_uring_register(fd: i32, opcode: u32, _arg: *mut u8, _nr_args: u32) -> i32 {
    let registered = opcode & IORING_REGISTER_USE_REGISTERED_RING != 0;
    if opcode & !IORING_REGISTER_USE_REGISTERED_RING >= IORING_REGISTER_LAST {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    no_ring(fd, registered)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;
    use core::ptr;

    fn good_params() -> IoUringParams {
        // SAFETY: every field is plain integer / array of integers, so
        // `core::mem::zeroed()` produces a well-formed all-zero
        // value (no NonNull / niche-restricted fields).
        unsafe { mem::zeroed() }
    }

    #[test]
    fn test_sqe_size() {
        assert_eq!(mem::size_of::<IoUringSqe>(), 64);
    }

    #[test]
    fn test_cqe_size() {
        assert_eq!(mem::size_of::<IoUringCqe>(), 16);
    }

    #[test]
    fn test_params_size() {
        assert!(mem::size_of::<IoUringParams>() >= 100);
    }

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
            IORING_OP_SENDMSG,
            IORING_OP_RECVMSG,
            IORING_OP_TIMEOUT,
            IORING_OP_ACCEPT,
            IORING_OP_READ,
            IORING_OP_WRITE,
            IORING_OP_CLOSE,
            IORING_OP_OPENAT,
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
            IORING_SETUP_IOPOLL,
            IORING_SETUP_SQPOLL,
            IORING_SETUP_SQ_AFF,
            IORING_SETUP_CQSIZE,
            IORING_SETUP_CLAMP,
            IORING_SETUP_ATTACH_WQ,
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
            IOSQE_FIXED_FILE,
            IOSQE_IO_DRAIN,
            IOSQE_IO_LINK,
            IOSQE_IO_HARDLINK,
            IOSQE_ASYNC,
            IOSQE_BUFFER_SELECT,
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
        assert_eq!(IORING_ENTER_SQ_WAIT, 4);
        assert_eq!(IORING_ENTER_EXT_ARG, 8);
        assert_eq!(IORING_ENTER_REGISTERED_RING, 16);
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
            IORING_REGISTER_PROBE,
            IORING_REGISTER_PERSONALITY,
            IORING_REGISTER_ENABLE_RINGS,
            IORING_REGISTER_RING_FDS,
            IORING_REGISTER_PBUF_RING,
            IORING_REGISTER_SYNC_CANCEL,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    // -----------------------------------------------------------------
    // Linux 6.6's constants
    // -----------------------------------------------------------------

    #[test]
    fn each_setup_flag_is_linuxs_bit() {
        // SINGLE_ISSUER was 1 << 8 -- COOP_TASKRUN's bit.
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
        for (bit, flag) in flags.iter().enumerate() {
            assert_eq!(*flag, 1 << bit, "bit {bit}");
        }
        assert_eq!(
            IORING_SETUP_SINGLE_ISSUER,
            crate::linux_io_uring_setup_types::IORING_SETUP_SINGLE_ISSUER,
            "the types module had it right"
        );
    }

    #[test]
    fn the_flag_sets_are_linux_6_6s() {
        assert_eq!(IORING_SETUP_FLAGS_VALID, 0x1_FFFF, "bits 0 to 16");
        assert_eq!(IORING_ENTER_FLAGS_VALID, 0x1F, "bits 0 to 4");
        assert_eq!(IORING_REGISTER_LAST, 26);
        assert_eq!(mem::size_of::<IoUringParams>(), 120);
    }

    // -----------------------------------------------------------------
    // io_uring_setup
    // -----------------------------------------------------------------

    fn setup(entries: u32, p: &IoUringParams) -> i32 {
        let mut p = *p;
        errno::set_errno(0);
        assert_eq!(io_uring_setup(entries, &raw mut p), -1);
        errno::get_errno()
    }

    fn with_flags(flags: u32) -> IoUringParams {
        let mut p = good_params();
        p.flags = flags;
        p
    }

    #[test]
    fn setup_reads_the_block_first() {
        errno::set_errno(0);
        assert_eq!(io_uring_setup(0, ptr::null_mut()), -1);
        assert_eq!(
            errno::get_errno(),
            errno::EFAULT,
            "NULL, even with no entries"
        );
        let kernel_half = 0xFFFF_8000_0000_0000_usize as *mut IoUringParams;
        assert_eq!(io_uring_setup(8, kernel_half), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn setup_checks_in_linuxs_order() {
        let mut p = with_flags(1 << 20);
        p.resv[1] = 1;
        assert_eq!(
            setup(0, &p),
            errno::EINVAL,
            "resv, before the flags and entries"
        );
        p.resv[1] = 0;
        assert_eq!(
            setup(0, &p),
            errno::EINVAL,
            "an unknown flag, before the entries"
        );
        assert_eq!(
            setup(8, &with_flags(IORING_SETUP_HYBRID_IOPOLL)),
            errno::EINVAL,
            "6.13's"
        );
        assert_eq!(setup(0, &good_params()), errno::EINVAL, "no entries");
        assert_eq!(setup(32_769, &good_params()), errno::EINVAL, "too many");
        assert_eq!(
            setup(32_769, &with_flags(IORING_SETUP_CLAMP)),
            errno::ENOSYS,
            "clamped"
        );
        assert_eq!(setup(32_768, &good_params()), errno::ENOSYS);
    }

    #[test]
    fn setup_takes_6_6s_newest_flags() {
        assert_eq!(
            setup(8, &with_flags(IORING_SETUP_NO_SQARRAY)),
            errno::ENOSYS
        );
        assert_eq!(setup(8, &with_flags(IORING_SETUP_NO_MMAP)), errno::ENOSYS);
        assert_eq!(
            setup(8, &with_flags(IORING_SETUP_REGISTERED_FD_ONLY)),
            errno::EINVAL,
            "only with NO_MMAP"
        );
        assert_eq!(
            setup(
                8,
                &with_flags(IORING_SETUP_REGISTERED_FD_ONLY | IORING_SETUP_NO_MMAP)
            ),
            errno::ENOSYS
        );
    }

    #[test]
    fn setup_sizes_the_cq_as_linux_rounds_it() {
        let cq = |n: u32, extra: u32| {
            let mut p = with_flags(IORING_SETUP_CQSIZE | extra);
            p.cq_entries = n;
            p
        };
        assert_eq!(setup(8, &cq(0, 0)), errno::EINVAL);
        assert_eq!(setup(8, &cq(65_537, 0)), errno::EINVAL);
        assert_eq!(setup(8, &cq(65_537, IORING_SETUP_CLAMP)), errno::ENOSYS);
        assert_eq!(
            setup(8, &cq(5, 0)),
            errno::ENOSYS,
            "5 rounds up to 8, the SQ's 8"
        );
        assert_eq!(
            setup(8, &cq(3, 0)),
            errno::EINVAL,
            "3 rounds up to 4, below 8"
        );
        assert_eq!(setup(7, &cq(8, 0)), errno::ENOSYS, "7 rounds up to 8");
    }

    #[test]
    fn setup_refuses_linuxs_flag_combinations_and_no_others() {
        use crate::linux_io_uring as u;
        let sqpoll = u::IORING_SETUP_SQPOLL;
        for extra in [
            u::IORING_SETUP_COOP_TASKRUN,
            u::IORING_SETUP_TASKRUN_FLAG,
            u::IORING_SETUP_DEFER_TASKRUN | u::IORING_SETUP_SINGLE_ISSUER,
        ] {
            assert_eq!(
                setup(8, &with_flags(sqpoll | extra)),
                errno::EINVAL,
                "{extra:#x}"
            );
        }
        assert_eq!(
            setup(8, &with_flags(u::IORING_SETUP_TASKRUN_FLAG)),
            errno::EINVAL
        );
        assert_eq!(
            setup(
                8,
                &with_flags(u::IORING_SETUP_TASKRUN_FLAG | u::IORING_SETUP_COOP_TASKRUN)
            ),
            errno::ENOSYS
        );
        assert_eq!(
            setup(8, &with_flags(u::IORING_SETUP_DEFER_TASKRUN)),
            errno::EINVAL,
            "DEFER_TASKRUN needs SINGLE_ISSUER"
        );
        assert_eq!(
            setup(
                8,
                &with_flags(
                    u::IORING_SETUP_TASKRUN_FLAG
                        | u::IORING_SETUP_DEFER_TASKRUN
                        | u::IORING_SETUP_SINGLE_ISSUER
                )
            ),
            errno::ENOSYS
        );
        // Refused here until 2026-09-26, accepted by Linux 6.6.
        assert_eq!(
            setup(8, &with_flags(sqpoll | u::IORING_SETUP_IOPOLL)),
            errno::ENOSYS
        );
    }

    #[test]
    fn sqpoll_needs_no_capability() {
        // Linux 6.6's io_sq_offload_create asks only the LSM hook; this
        // answered EPERM without CAP_SYS_NICE.  (This test does not drop the
        // capability: with the check gone there is nothing to exercise.)
        assert_eq!(setup(8, &with_flags(IORING_SETUP_SQPOLL)), errno::ENOSYS);
    }

    #[test]
    fn checks_after_the_rings_are_not_made() {
        // Linux refuses these only after it has allocated the rings; there
        // are none to allocate, and ENOSYS says so.
        let mut p = with_flags(IORING_SETUP_ATTACH_WQ);
        p.wq_fd = u32::MAX;
        assert_eq!(setup(8, &p), errno::ENOSYS);
        assert_eq!(setup(8, &with_flags(IORING_SETUP_SQ_AFF)), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // io_uring_enter and io_uring_register: there is no ring
    // -----------------------------------------------------------------

    fn enter(fd: i32, flags: u32) -> i32 {
        errno::set_errno(0);
        assert_eq!(io_uring_enter(fd, 1, 1, flags, ptr::null(), 0), -1);
        errno::get_errno()
    }

    fn register(fd: i32, opcode: u32) -> i32 {
        errno::set_errno(0);
        assert_eq!(io_uring_register(fd, opcode, ptr::null_mut(), 0), -1);
        errno::get_errno()
    }

    /// An open descriptor that is not a ring, closed on drop.
    struct OpenFd(i32);
    impl OpenFd {
        fn new() -> Self {
            Self(crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0x1_0C1).unwrap())
        }
    }
    impl Drop for OpenFd {
        fn drop(&mut self) {
            // A test descriptor with no kernel object behind it; closing the
            // slot is all there is.
            let _ = crate::fdtable::close_fd(self.0);
        }
    }

    #[test]
    fn enter_checks_its_flags_then_finds_no_ring() {
        assert_eq!(
            enter(-1, 1 << 5),
            errno::EINVAL,
            "ABS_TIMER is 6.12's, and before the fd"
        );
        assert_eq!(enter(-1, 0), errno::EBADF);
        assert_eq!(
            enter(4_000, IORING_ENTER_GETEVENTS),
            errno::EBADF,
            "not open"
        );
        let open = OpenFd::new();
        assert_eq!(
            enter(open.0, IORING_ENTER_GETEVENTS),
            errno::EOPNOTSUPP,
            "not a ring"
        );
        assert_eq!(
            enter(0, IORING_ENTER_REGISTERED_RING),
            errno::EINVAL,
            "no ring was registered"
        );
        assert_eq!(enter(16, IORING_ENTER_REGISTERED_RING), errno::EINVAL);
    }

    #[test]
    fn enter_asks_nothing_about_a_ring_it_did_not_find() {
        // min_complete and the signal set were checked before the fd here;
        // Linux checks them only once it has a ring.
        errno::set_errno(0);
        assert_eq!(io_uring_enter(-1, 0, u32::MAX, 0, 1 as *const u8, 0), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn register_checks_its_opcode_then_finds_no_ring() {
        assert_eq!(
            register(-1, 26),
            errno::EINVAL,
            "PBUF_STATUS is 6.8's, and before the fd"
        );
        assert_eq!(register(-1, IORING_REGISTER_FILE_ALLOC_RANGE), errno::EBADF);
        let open = OpenFd::new();
        assert_eq!(register(open.0, IORING_REGISTER_PROBE), errno::EOPNOTSUPP);
        assert_eq!(
            register(
                0,
                IORING_REGISTER_USE_REGISTERED_RING | IORING_REGISTER_PROBE
            ),
            errno::EINVAL,
            "no ring was registered"
        );
        assert_eq!(
            register(0, IORING_REGISTER_USE_REGISTERED_RING | 26),
            errno::EINVAL
        );
        // Argument shapes were checked before the fd here; Linux checks them
        // only once it has a ring.
        assert_eq!(register(-1, IORING_REGISTER_BUFFERS), errno::EBADF);
        assert_eq!(register(-1, IORING_UNREGISTER_BUFFERS), errno::EBADF);
    }
}

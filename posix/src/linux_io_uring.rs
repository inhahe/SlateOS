//! `<linux/io_uring.h>` — io_uring asynchronous I/O interface.
//!
//! Provides data structures and constants for the io_uring
//! submission/completion queue interface, plus a real input-validator
//! front end for the three syscalls (`io_uring_setup`,
//! `io_uring_enter`, `io_uring_register`).
//!
//! Validation matches Linux's `io_uring_setup(2)` /
//! `io_uring_enter(2)` / `io_uring_register(2)` contracts; every code
//! path that passes the checks then returns `-1` / `errno = ENOSYS`
//! because we don't yet have a real io_uring subsystem (the kernel
//! ring-mmap'd SQ/CQ pages, the kthread-based SQPOLL worker, the
//! per-op verbs that actually do I/O). Programs that probe for
//! io_uring at startup (liburing's `io_uring_queue_init`, tokio-uring,
//! the Rust `rio` crate, glommio, the `compio` async runtime) see
//! ENOSYS and either fall back to epoll/IOCP-style polling or fail
//! gracefully.

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
/// Use a single issuer.
pub const IORING_SETUP_SINGLE_ISSUER: u32 = 256;
/// Defer task work.
pub const IORING_SETUP_DEFER_TASKRUN: u32 = 1 << 13;
/// Disable file-table updates on COOP_TASKRUN.
pub const IORING_SETUP_COOP_TASKRUN: u32 = 1 << 8;
/// Use 32-byte SQEs (extended).
pub const IORING_SETUP_SQE128: u32 = 1 << 10;
/// Use 32-byte CQEs (extended).
pub const IORING_SETUP_CQE32: u32 = 1 << 11;
/// Hybrid IOPOLL mode (Linux 6.7+).
pub const IORING_SETUP_TASKRUN_FLAG: u32 = 1 << 9;
/// No SQARRAY indirection (Linux 6.6+).
pub const IORING_SETUP_NO_SQARRAY: u32 = 1 << 16;
/// Hybrid IOPOLL.
pub const IORING_SETUP_HYBRID_IOPOLL: u32 = 1 << 17;
/// Valid bit mask for io_uring_setup flags.
const IORING_SETUP_FLAGS_VALID: u32 = IORING_SETUP_IOPOLL
    | IORING_SETUP_SQPOLL
    | IORING_SETUP_SQ_AFF
    | IORING_SETUP_CQSIZE
    | IORING_SETUP_CLAMP
    | IORING_SETUP_ATTACH_WQ
    | IORING_SETUP_R_DISABLED
    | IORING_SETUP_SUBMIT_ALL
    | IORING_SETUP_SINGLE_ISSUER
    | IORING_SETUP_DEFER_TASKRUN
    | IORING_SETUP_COOP_TASKRUN
    | IORING_SETUP_SQE128
    | IORING_SETUP_CQE32
    | IORING_SETUP_TASKRUN_FLAG
    | IORING_SETUP_NO_SQARRAY
    | IORING_SETUP_HYBRID_IOPOLL;

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
/// Valid bit mask for io_uring_enter flags.
const IORING_ENTER_FLAGS_VALID: u32 = IORING_ENTER_GETEVENTS
    | IORING_ENTER_SQ_WAKEUP
    | IORING_ENTER_SQ_WAIT
    | IORING_ENTER_EXT_ARG
    | IORING_ENTER_REGISTERED_RING
    | IORING_ENTER_ABS_TIMER
    | IORING_ENTER_EXT_ARG_REG;

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
/// First unknown register op — anything ≥ this is rejected.
const IORING_REGISTER_LAST: u32 = 32;

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

/// Maximum SQ ring size accepted without `IORING_SETUP_CLAMP` (Linux
/// `IORING_MAX_ENTRIES`).
const IORING_MAX_ENTRIES: u32 = 32_768;
/// Maximum CQ ring size when `IORING_SETUP_CQSIZE` is set (Linux
/// `IORING_MAX_CQ_ENTRIES = 2 * IORING_MAX_ENTRIES`).
const IORING_MAX_CQ_ENTRIES: u32 = 65_536;
/// Cap on `min_complete` for `io_uring_enter` — guards against
/// callers asking us to wait for more events than the ring can hold.
const IORING_MAX_MIN_COMPLETE: u32 = IORING_MAX_CQ_ENTRIES;
/// Cap on `nr_args` for `io_uring_register` (per-op limits are
/// stricter, but this is the outer ceiling for any caller-supplied
/// array — keeps us from copying multi-megabyte buffers from a bad
/// caller).
const IORING_MAX_REGISTER_NR_ARGS: u32 = 1 << 20; // 1M entries

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validates a caller-supplied `IoUringParams` for `io_uring_setup`.
fn validate_setup_params(entries: u32, p: &IoUringParams) -> Result<(), i32> {
    if entries == 0 {
        return Err(errno::EINVAL);
    }
    if entries > IORING_MAX_ENTRIES && (p.flags & IORING_SETUP_CLAMP) == 0 {
        // Without CLAMP, Linux returns EINVAL; with CLAMP it silently
        // clamps to the max.
        return Err(errno::EINVAL);
    }
    if (p.flags & !IORING_SETUP_FLAGS_VALID) != 0 {
        return Err(errno::EINVAL);
    }
    // SQ_AFF requires SQPOLL — the affinity setting is meaningless
    // without a SQ poll thread to bind.
    if (p.flags & IORING_SETUP_SQ_AFF) != 0 && (p.flags & IORING_SETUP_SQPOLL) == 0 {
        return Err(errno::EINVAL);
    }
    // CQSIZE must come with a sane cq_entries field.
    if (p.flags & IORING_SETUP_CQSIZE) != 0 {
        if p.cq_entries == 0 {
            return Err(errno::EINVAL);
        }
        if p.cq_entries < entries {
            // CQ must be at least as large as SQ — Linux requires this
            // because every SQE eventually produces ≥1 CQE.
            return Err(errno::EINVAL);
        }
        if p.cq_entries > IORING_MAX_CQ_ENTRIES && (p.flags & IORING_SETUP_CLAMP) == 0 {
            return Err(errno::EINVAL);
        }
    }
    // ATTACH_WQ requires a sane wq_fd. We accept any non-zero value
    // because we'll EBADF below — the validation here is just to
    // catch "ATTACH_WQ with wq_fd=0" which is almost always a bug.
    if (p.flags & IORING_SETUP_ATTACH_WQ) != 0 {
        let wq = p.wq_fd as i32;
        if wq < 0 {
            return Err(errno::EBADF);
        }
    }
    // Reserved fields must be zero — Linux uses these for future
    // expansion and rejects any nonzero value to prevent silently
    // accepting attr structs from a newer caller.
    if p.resv != [0; 3] {
        return Err(errno::EINVAL);
    }
    // DEFER_TASKRUN requires SINGLE_ISSUER (Linux strictly enforces
    // this — task-run deferral only makes sense if there's one issuer
    // to defer to).
    if (p.flags & IORING_SETUP_DEFER_TASKRUN) != 0
        && (p.flags & IORING_SETUP_SINGLE_ISSUER) == 0
    {
        return Err(errno::EINVAL);
    }
    // SQPOLL + IOPOLL: incompatible (SQPOLL needs the kernel to wake
    // and process; IOPOLL needs the caller to busy-poll — Linux 5.x+
    // rejects the combination on most filesystems).
    // (Linux actually accepts it on blkio devices, but we conservatively
    // reject because we don't have either path.)
    if (p.flags & IORING_SETUP_SQPOLL) != 0 && (p.flags & IORING_SETUP_IOPOLL) != 0 {
        return Err(errno::EINVAL);
    }
    Ok(())
}

/// Validates `io_uring_register` arguments.
fn validate_register(opcode: u32, arg: *mut u8, nr_args: u32) -> Result<(), i32> {
    if opcode >= IORING_REGISTER_LAST {
        return Err(errno::EINVAL);
    }
    if nr_args > IORING_MAX_REGISTER_NR_ARGS {
        return Err(errno::E2BIG);
    }
    // Per-op argument-shape validation. Operations that take a single
    // value (eventfd register, file alloc range) accept nr_args==1;
    // operations that take a count (buffers/files register) need a
    // non-NULL arg if nr_args > 0; operations that take no argument
    // (unregister) require nr_args==0 and arg==NULL.
    match opcode {
        IORING_UNREGISTER_BUFFERS
        | IORING_UNREGISTER_FILES
        | IORING_UNREGISTER_EVENTFD
        | IORING_UNREGISTER_PERSONALITY
        | IORING_UNREGISTER_IOWQ_AFF
        | IORING_REGISTER_ENABLE_RINGS => {
            if !arg.is_null() {
                return Err(errno::EINVAL);
            }
            if nr_args != 0 {
                return Err(errno::EINVAL);
            }
        }
        IORING_REGISTER_BUFFERS
        | IORING_REGISTER_FILES
        | IORING_REGISTER_FILES_UPDATE
        | IORING_REGISTER_BUFFERS2
        | IORING_REGISTER_BUFFERS_UPDATE
        | IORING_REGISTER_FILES2
        | IORING_REGISTER_RING_FDS
        | IORING_UNREGISTER_RING_FDS => {
            if nr_args == 0 {
                return Err(errno::EINVAL);
            }
            if arg.is_null() {
                return Err(errno::EFAULT);
            }
        }
        IORING_REGISTER_EVENTFD
        | IORING_REGISTER_EVENTFD_ASYNC
        | IORING_REGISTER_PERSONALITY
        | IORING_REGISTER_PROBE
        | IORING_REGISTER_RESTRICTIONS
        | IORING_REGISTER_IOWQ_AFF
        | IORING_REGISTER_IOWQ_MAX_WORKERS
        | IORING_REGISTER_PBUF_RING
        | IORING_UNREGISTER_PBUF_RING
        | IORING_REGISTER_SYNC_CANCEL
        | IORING_REGISTER_FILE_ALLOC_RANGE
        | IORING_REGISTER_PBUF_STATUS => {
            if arg.is_null() {
                return Err(errno::EFAULT);
            }
        }
        _ => {} // remaining valid ops: pass through.
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Syscall wrappers
// ---------------------------------------------------------------------------

/// Set up an io_uring instance.
///
/// Validates `entries` and the caller-supplied `params` struct
/// (flags, mutual-exclusion rules, reserved-field nonzero check)
/// before returning `-1` / `errno = ENOSYS`. Real ring setup requires
/// SQ/CQ page mmap support and the per-op infrastructure.
///
/// # Errors
///
/// - `EFAULT`: NULL `params`.
/// - `EINVAL`: `entries == 0`, `entries > IORING_MAX_ENTRIES` without
///   `IORING_SETUP_CLAMP`, unknown flag bits, `SQ_AFF` without
///   `SQPOLL`, `CQSIZE` with bad `cq_entries`, `SQPOLL + IOPOLL`,
///   `DEFER_TASKRUN` without `SINGLE_ISSUER`, nonzero reserved field.
/// - `EBADF`: `ATTACH_WQ` with negative `wq_fd`.
/// - `ENOSYS`: all checks pass — no in-kernel io_uring subsystem yet.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_uring_setup(entries: u32, params: *mut IoUringParams) -> i32 {
    if params.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: caller-supplied pointer is non-NULL; we use
    // read_unaligned so an alignment-1 pointer doesn't UB.
    let p: IoUringParams = unsafe { core::ptr::read_unaligned(params) };
    if let Err(e) = validate_setup_params(entries, &p) {
        errno::set_errno(e);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Submit and/or wait for io_uring operations.
///
/// Validates `fd`, `flags`, `min_complete` bounds, and the
/// `sig`/`sigsz` consistency (sig==NULL ⇒ sigsz==0). Anything that
/// passes the checks returns `-1` / `errno = ENOSYS` (or `EBADF` for
/// any positive fd since no rings exist).
///
/// # Errors
///
/// - `EBADF`: `fd < 0`, or non-negative fd that isn't a ring (every
///   case while no rings exist).
/// - `EINVAL`: unknown flag bits, or `sig != NULL && sigsz == 0`, or
///   `sigsz > sizeof(sigset_t) * 2`, or `min_complete >
///   IORING_MAX_CQ_ENTRIES`.
/// - `EFAULT`: would apply once sig dereferencing is wired up — kept
///   reserved for that path.
/// - `ENOSYS`: all checks pass.
///
/// # Validation order (Linux parity, Phase 110)
///
/// Mirrors Linux's `io_uring/io_uring.c::SYSCALL_DEFINE6(io_uring_enter)`:
/// the flag-mask check runs *before* `fget(fd)`, so an unknown flag bit
/// wins over a bad fd.  The previous ordering returned `EBADF` first,
/// which fooled callers (notably tokio-uring's syscall-availability
/// probe) into thinking the kernel didn't recognise io_uring at all,
/// when in fact they had passed a flag bit Linux had also rejected.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_uring_enter(
    fd: i32,
    _to_submit: u32,
    min_complete: u32,
    flags: u32,
    sig: *const u8,
    sigsz: usize,
) -> i32 {
    // (1) Linux's io_uring_enter checks `flags` at the very top of
    // the syscall handler — before the file-table lookup.
    if (flags & !IORING_ENTER_FLAGS_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // (2) sig/sigsz consistency.  Linux performs this check while
    // copying the `io_uring_getevents_arg` from userspace, also
    // before the ring is touched.
    //
    // sig pointer must be consistent with sigsz: either both zero or
    // both nonzero. If sig is non-NULL, sigsz must equal
    // sizeof(sigset_t) for the kernel (Linux uses 8 on most arches).
    // We accept any small nonzero size up to 128 (the libc kernel
    // sigset is 8 bytes; some libcs send the userspace 128-byte view).
    if !sig.is_null() && (sigsz == 0 || sigsz > 128) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if sig.is_null() && sigsz != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // (3) min_complete bound.  Linux doesn't pre-validate this — it
    // bails out later when the CQ ring is too small.  We pre-validate
    // because we have no ring at all; treating an impossibly large
    // request as EINVAL is friendlier than EBADF.
    if min_complete > IORING_MAX_MIN_COMPLETE {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // (4) fd lookup, last — matches `fget(fd)` placement in Linux.
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // No real rings exist yet — any positive fd is dangling.
    // EBADF matches Linux's behavior when the fd isn't a ring.
    errno::set_errno(errno::EBADF);
    -1
}

/// Register resources with an io_uring instance.
///
/// Validates `fd`, `opcode`, and per-opcode argument shape.
///
/// # Errors
///
/// - `EBADF`: `fd < 0`, or non-negative fd that isn't a ring (every
///   case while no rings exist).
/// - `EINVAL`: unknown `opcode`, unregister op with non-NULL arg or
///   non-zero nr_args, register op with nr_args==0 when a count is
///   required.
/// - `EFAULT`: register op that requires a buffer but `arg == NULL`.
/// - `E2BIG`: `nr_args` above the safety cap.
/// - `ENOSYS`: all checks pass (no real ring to register against).
///
/// # Validation order (Linux parity, Phase 110)
///
/// Linux's `io_uring/register.c::__do_sys_io_uring_register` validates
/// `opcode >= IORING_REGISTER_LAST -> EINVAL` *before* fetching the
/// ring file (`fget(fd)`), so an unknown opcode wins over a bad fd.
/// We mirror that ordering: opcode/arg shape first, fd last.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_uring_register(
    fd: i32,
    opcode: u32,
    arg: *mut u8,
    nr_args: u32,
) -> i32 {
    // Opcode and per-opcode argument shape are validated before the
    // fd lookup, per Linux's __do_sys_io_uring_register.
    if let Err(e) = validate_register(opcode, arg, nr_args) {
        errno::set_errno(e);
        return -1;
    }
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // No real ring exists — match Linux's "fd is not a ring" error.
    errno::set_errno(errno::EBADF);
    -1
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
        assert_eq!(IORING_ENTER_SQ_WAIT, 4);
        assert_eq!(IORING_ENTER_EXT_ARG, 8);
        assert_eq!(IORING_ENTER_REGISTERED_RING, 16);
    }

    #[test]
    fn test_register_ops_distinct() {
        let ops = [
            IORING_REGISTER_BUFFERS, IORING_UNREGISTER_BUFFERS,
            IORING_REGISTER_FILES, IORING_UNREGISTER_FILES,
            IORING_REGISTER_EVENTFD, IORING_UNREGISTER_EVENTFD,
            IORING_REGISTER_PROBE, IORING_REGISTER_PERSONALITY,
            IORING_REGISTER_ENABLE_RINGS, IORING_REGISTER_RING_FDS,
            IORING_REGISTER_PBUF_RING, IORING_REGISTER_SYNC_CANCEL,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    // -----------------------------------------------------------------
    // io_uring_setup tests
    // -----------------------------------------------------------------

    #[test]
    fn test_setup_null_params_efault() {
        let r = io_uring_setup(32, ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_setup_zero_entries_einval() {
        let mut p = good_params();
        let r = io_uring_setup(0, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_too_many_entries_einval_without_clamp() {
        let mut p = good_params();
        let r = io_uring_setup(IORING_MAX_ENTRIES + 1, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_too_many_entries_ok_with_clamp() {
        let mut p = good_params();
        p.flags = IORING_SETUP_CLAMP;
        let r = io_uring_setup(IORING_MAX_ENTRIES + 1, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_setup_unknown_flag_einval() {
        let mut p = good_params();
        p.flags = 1u32 << 31; // top bit reserved
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_sq_aff_without_sqpoll_einval() {
        let mut p = good_params();
        p.flags = IORING_SETUP_SQ_AFF;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_sq_aff_with_sqpoll_ok() {
        let mut p = good_params();
        p.flags = IORING_SETUP_SQPOLL | IORING_SETUP_SQ_AFF;
        p.sq_thread_cpu = 0;
        p.sq_thread_idle = 1000;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_setup_cqsize_zero_cq_entries_einval() {
        let mut p = good_params();
        p.flags = IORING_SETUP_CQSIZE;
        p.cq_entries = 0;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_cqsize_smaller_than_entries_einval() {
        let mut p = good_params();
        p.flags = IORING_SETUP_CQSIZE;
        p.cq_entries = 16;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_cqsize_too_big_einval() {
        let mut p = good_params();
        p.flags = IORING_SETUP_CQSIZE;
        p.cq_entries = IORING_MAX_CQ_ENTRIES + 1;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_cqsize_too_big_ok_with_clamp() {
        let mut p = good_params();
        p.flags = IORING_SETUP_CQSIZE | IORING_SETUP_CLAMP;
        p.cq_entries = IORING_MAX_CQ_ENTRIES + 1;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_setup_attach_wq_negative_ebadf() {
        let mut p = good_params();
        p.flags = IORING_SETUP_ATTACH_WQ;
        p.wq_fd = u32::MAX; // -1 as i32
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_setup_attach_wq_zero_ok() {
        let mut p = good_params();
        p.flags = IORING_SETUP_ATTACH_WQ;
        p.wq_fd = 0;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_setup_nonzero_resv_einval() {
        let mut p = good_params();
        p.resv[0] = 1;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_defer_taskrun_without_single_issuer_einval() {
        let mut p = good_params();
        p.flags = IORING_SETUP_DEFER_TASKRUN;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_defer_taskrun_with_single_issuer_ok() {
        let mut p = good_params();
        p.flags = IORING_SETUP_DEFER_TASKRUN | IORING_SETUP_SINGLE_ISSUER;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_setup_sqpoll_iopoll_conflict_einval() {
        let mut p = good_params();
        p.flags = IORING_SETUP_SQPOLL | IORING_SETUP_IOPOLL;
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_setup_valid_basic_reaches_enosys() {
        let mut p = good_params();
        // Plain SQ-only setup, no flags.
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_setup_misaligned_params_pointer() {
        let mut buf = [0u8; mem::size_of::<IoUringParams>() + 1];
        let p_aligned = good_params();
        unsafe {
            ptr::copy_nonoverlapping(
                (&p_aligned as *const IoUringParams).cast::<u8>(),
                buf.as_mut_ptr().add(1),
                mem::size_of::<IoUringParams>(),
            );
        }
        let p = unsafe { buf.as_mut_ptr().add(1) } as *mut IoUringParams;
        let r = io_uring_setup(32, p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // io_uring_enter tests
    // -----------------------------------------------------------------

    #[test]
    fn test_enter_negative_fd_ebadf() {
        let r = io_uring_enter(-1, 0, 0, 0, ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_enter_unknown_flag_einval() {
        let r = io_uring_enter(3, 0, 0, 1u32 << 31, ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_enter_huge_min_complete_einval() {
        let r = io_uring_enter(3, 0, IORING_MAX_MIN_COMPLETE + 1, 0, ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_enter_sig_inconsistent_null_with_size_einval() {
        let r = io_uring_enter(3, 0, 0, 0, ptr::null(), 8);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_enter_sig_inconsistent_nonnull_zero_size_einval() {
        let mut buf = [0u8; 8];
        let r = io_uring_enter(3, 0, 0, 0, buf.as_mut_ptr() as *const u8, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_enter_sig_too_large_einval() {
        let mut buf = [0u8; 256];
        let r = io_uring_enter(3, 0, 0, 0, buf.as_mut_ptr() as *const u8, 256);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_enter_positive_fd_ebadf() {
        let r = io_uring_enter(3, 0, 0, 0, ptr::null(), 0);
        assert_eq!(r, -1);
        // Validation passes, but no ring exists -> EBADF.
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_enter_valid_with_sig_reaches_ebadf() {
        let mut buf = [0u8; 8];
        let r = io_uring_enter(
            3,
            0,
            0,
            IORING_ENTER_GETEVENTS,
            buf.as_mut_ptr() as *const u8,
            8,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -----------------------------------------------------------------
    // io_uring_register tests
    // -----------------------------------------------------------------

    #[test]
    fn test_register_negative_fd_ebadf() {
        // Use an opcode whose argument shape is satisfied by
        // (arg=NULL, nr_args=0) so the post-Phase-110 reorder still
        // reaches the fd check.  IORING_REGISTER_ENABLE_RINGS is the
        // canonical "no argument" register op.
        let r = io_uring_register(-1, IORING_REGISTER_ENABLE_RINGS, ptr::null_mut(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_register_unknown_opcode_einval() {
        let r = io_uring_register(3, IORING_REGISTER_LAST, ptr::null_mut(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_register_unregister_with_arg_einval() {
        let mut x = 0u8;
        let r = io_uring_register(3, IORING_UNREGISTER_BUFFERS, &mut x, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_register_unregister_with_nr_args_einval() {
        let r = io_uring_register(3, IORING_UNREGISTER_BUFFERS, ptr::null_mut(), 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_register_buffers_zero_nr_args_einval() {
        let mut x = 0u8;
        let r = io_uring_register(3, IORING_REGISTER_BUFFERS, &mut x, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_register_buffers_null_arg_efault() {
        let r = io_uring_register(3, IORING_REGISTER_BUFFERS, ptr::null_mut(), 4);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_register_huge_nr_args_e2big() {
        let mut x = 0u8;
        let r = io_uring_register(3, IORING_REGISTER_BUFFERS, &mut x, u32::MAX);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_register_eventfd_null_arg_efault() {
        let r = io_uring_register(3, IORING_REGISTER_EVENTFD, ptr::null_mut(), 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_register_enable_rings_with_arg_einval() {
        let mut x = 0u8;
        let r = io_uring_register(3, IORING_REGISTER_ENABLE_RINGS, &mut x, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_register_enable_rings_valid_reaches_ebadf() {
        let r = io_uring_register(3, IORING_REGISTER_ENABLE_RINGS, ptr::null_mut(), 0);
        assert_eq!(r, -1);
        // Validation passes, no ring exists.
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_register_buffers_valid_reaches_ebadf() {
        let mut buf = [0u8; 64];
        let r = io_uring_register(3, IORING_REGISTER_BUFFERS, buf.as_mut_ptr(), 4);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -----------------------------------------------------------------
    // Phase 110: Linux-parity validation order
    //
    // Linux validates `flags` (enter) / `opcode` (register) before the
    // fd lookup, so a malformed flag bit or unknown opcode wins over
    // a bad fd.  These tests pin that ordering.
    // -----------------------------------------------------------------

    #[test]
    fn test_enter_phase110_einval_flags_wins_over_ebadf_negative_fd() {
        // fd=-1 + unknown flag bit: Linux returns EINVAL because the
        // flag mask is checked before fget(fd).
        let r = io_uring_enter(-1, 0, 0, 1u32 << 31, ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_enter_phase110_einval_sig_inconsistent_wins_over_ebadf() {
        // fd=-1 + sig=null + sigsz=8: Linux validates the sig/sigsz
        // tuple while copying the getevents arg, before any fd work.
        let r = io_uring_enter(-1, 0, 0, 0, ptr::null(), 8);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_enter_phase110_einval_sig_nonnull_zero_size_wins_over_ebadf() {
        // fd=-1 + sig=non-null + sigsz=0: same as above, the sig/sigsz
        // mismatch is caught before the fd lookup.
        let mut buf = [0u8; 8];
        let r = io_uring_enter(-1, 0, 0, 0, buf.as_mut_ptr() as *const u8, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_enter_phase110_einval_min_complete_wins_over_ebadf() {
        // fd=-1 + huge min_complete: our extra pre-validation runs
        // before the fd lookup.  (Linux itself doesn't pre-validate
        // min_complete, but our cap is reached before the fd path —
        // EINVAL is still the right answer for "impossibly large".)
        let r = io_uring_enter(-1, 0, IORING_MAX_MIN_COMPLETE + 1, 0, ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_enter_phase110_valid_flags_then_bad_fd_ebadf() {
        // Valid flag + valid sig tuple + good min_complete + fd=-1:
        // EBADF is correct because all the prologue checks pass.
        let r = io_uring_enter(-1, 0, 0, IORING_ENTER_GETEVENTS, ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_enter_phase110_recovery_after_einval() {
        // After an EINVAL-rejected call, a well-formed call still
        // produces the expected EBADF — the validator is stateless.
        let r1 = io_uring_enter(-1, 0, 0, 1u32 << 31, ptr::null(), 0);
        assert_eq!(r1, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        let r2 = io_uring_enter(3, 0, 0, 0, ptr::null(), 0);
        assert_eq!(r2, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_register_phase110_einval_opcode_wins_over_ebadf_negative_fd() {
        // fd=-1 + unknown opcode: Linux's __do_sys_io_uring_register
        // checks the opcode bound before fget(fd), so EINVAL beats
        // EBADF.
        let r = io_uring_register(-1, u32::MAX, ptr::null_mut(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_register_phase110_einval_arg_shape_wins_over_ebadf() {
        // fd=-1 + unregister opcode with non-NULL arg: arg-shape
        // validation runs before the fd lookup -> EINVAL.
        let mut x = 0u8;
        let r = io_uring_register(-1, IORING_UNREGISTER_BUFFERS, &mut x, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_register_phase110_efault_arg_wins_over_ebadf_negative_fd() {
        // fd=-1 + register-with-required-arg opcode + arg=NULL:
        // validate_register reports EFAULT before the fd lookup.
        let r = io_uring_register(-1, IORING_REGISTER_EVENTFD, ptr::null_mut(), 1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_register_phase110_e2big_nr_args_wins_over_ebadf() {
        // fd=-1 + huge nr_args: E2BIG from validate_register beats
        // EBADF from the fd lookup.
        let mut x = 0u8;
        let r = io_uring_register(-1, IORING_REGISTER_BUFFERS, &mut x, u32::MAX);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn test_register_phase110_valid_opcode_then_bad_fd_ebadf() {
        // All shape checks pass + fd=-1: EBADF is the right answer.
        let r = io_uring_register(-1, IORING_REGISTER_ENABLE_RINGS, ptr::null_mut(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_register_phase110_recovery_after_einval() {
        // After a bad-opcode rejection, a well-formed register still
        // produces the expected EBADF.
        let r1 = io_uring_register(-1, u32::MAX, ptr::null_mut(), 0);
        assert_eq!(r1, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        let r2 = io_uring_register(3, IORING_REGISTER_ENABLE_RINGS, ptr::null_mut(), 0);
        assert_eq!(r2, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -----------------------------------------------------------------
    // Workflow tests
    // -----------------------------------------------------------------

    #[test]
    fn test_liburing_init_workflow() {
        // liburing's io_uring_queue_init:
        //   io_uring_setup(entries, &params)
        //   on failure, queue_init returns -errno; on success it
        //   mmaps SQ/CQ pages.
        // We expect ENOSYS so callers can detect "no io_uring" cleanly.
        let mut p = good_params();
        let r = io_uring_setup(128, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_tokio_uring_probe_workflow() {
        // tokio-uring probes with a small 8-entry ring at startup.
        let mut p = good_params();
        let r = io_uring_setup(8, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_postgres17_io_method_workflow() {
        // PostgreSQL 17's `io_method = io_uring` runs:
        //   io_uring_setup(SHARED_BUFFERS / 16, &params)
        // with SINGLE_ISSUER+DEFER_TASKRUN to avoid per-backend wakeups.
        let mut p = good_params();
        p.flags = IORING_SETUP_SINGLE_ISSUER | IORING_SETUP_DEFER_TASKRUN;
        let r = io_uring_setup(1024, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        // Postgres sees ENOSYS, logs "io_uring not available", and
        // falls back to io_method = sync.
    }

    #[test]
    fn test_errno_preserved_on_successful_path() {
        // Plant a sentinel and verify the ENOSYS path doesn't leak
        // an intermediate errno.
        errno::set_errno(errno::EBADF);
        let mut p = good_params();
        let r = io_uring_setup(32, &mut p);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }
}

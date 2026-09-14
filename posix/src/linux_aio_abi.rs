// Indexing and arithmetic in this file operate on a fixed pool of
// `MAX_CTX` AIO contexts, each with a ring buffer of
// `MAX_EVENTS_PER_CTX` events.  `ctx_id` is validated to be in
// `1..=MAX_CTX` before any `ctx_id - 1` subtraction.  Head/tail/
// count arithmetic is taken modulo the ring size on every write.
// Bounds are established locally but clippy cannot see across the
// check.
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

//! `<linux/aio_abi.h>` — kernel asynchronous I/O (AIO) interface.
//!
//! This is the kernel-level AIO interface (via `io_setup`, `io_submit`,
//! `io_getevents`, `io_destroy` syscalls), distinct from the POSIX AIO
//! functions (`aio_read`, `aio_write`). Modern code should prefer
//! io_uring, but kernel AIO is still used for `O_DIRECT` I/O.
//!
//! ## Implementation
//!
//! Like our POSIX AIO (`aio.rs`), this is a synchronous wrapper: every
//! operation submitted via `io_submit` runs to completion immediately
//! using the corresponding sync primitive (`pread`, `pwrite`,
//! `preadv`, `pwritev`, `fsync`, `fdatasync`).  The completion event
//! is queued on the context and returned by the next `io_getevents`.
//! This is POSIX-conformant for correctness — programs see results
//! identical to a fast async backend — but offers no real parallelism.
//!
//! ## Limitations
//!
//! - Context capacity is bounded by `MAX_EVENTS_PER_CTX` (256).  Callers
//!   that request more than this via `io_setup`'s `nr_events` get
//!   `EINVAL`.
//! - At most `MAX_AIO_CONTEXTS` (8) contexts may be alive at once.
//! - `aio_resfd` / eventfd notification **is** honoured: an iocb carrying
//!   `IOCB_FLAG_RESFD` has its `aio_resfd` eventfd incremented as the
//!   completion is queued. Until 2026-09-13 this was accepted and ignored,
//!   which is worse than not supporting it: a caller that waits on that
//!   eventfd rather than calling `io_getevents` waits forever, and nothing
//!   tells it why. What is *not* diagnosed is an `aio_resfd` that is not a
//!   valid eventfd — Linux rejects that at submit time with `EINVAL`, and we
//!   queue the completion anyway, so `io_getevents` still returns it.
//! - Per-I/O `RWF_*` flags (`aio_rw_flags`) are honoured or refused, never
//!   ignored. `RWF_DSYNC`/`RWF_SYNC` sync the file after a successful write;
//!   `RWF_NOWAIT` is refused with `EAGAIN` (a synchronous executor always
//!   blocks, so "fail rather than block" can only be answered by failing);
//!   `RWF_APPEND` is refused with `EINVAL`; unknown bits are `EINVAL`.
//!   `RWF_HIPRI` is a scheduling hint with no observable semantics and is the
//!   only one genuinely ignored. Until 2026-09-13 all of them were ignored,
//!   which for `RWF_DSYNC` meant reporting a durable write that was not one.

use crate::errno;
use crate::perprocess::process_global;
use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// AIO commands (iocb.aio_lio_opcode)
// ---------------------------------------------------------------------------

/// Read operation.
pub const IOCB_CMD_PREAD: u16 = 0;
/// Write operation.
pub const IOCB_CMD_PWRITE: u16 = 1;
/// Fsync.
pub const IOCB_CMD_FSYNC: u16 = 2;
/// Fdatasync.
pub const IOCB_CMD_FDSYNC: u16 = 3;
/// Vectored read.
pub const IOCB_CMD_PREADV: u16 = 7;
/// Vectored write.
pub const IOCB_CMD_PWRITEV: u16 = 8;
/// No-op (poll).
pub const IOCB_CMD_NOOP: u16 = 6;

// ---------------------------------------------------------------------------
// AIO flags (iocb.aio_flags)
// ---------------------------------------------------------------------------

/// Set if using eventfd for notification.
pub const IOCB_FLAG_RESFD: u32 = 1 << 0;
/// Submit as `IOPRIO` class.
pub const IOCB_FLAG_IOPRIO: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// I/O control block
// ---------------------------------------------------------------------------

/// Kernel AIO I/O control block (64 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Iocb {
    /// Data returned in `io_event`.
    pub aio_data: u64,
    /// PADDED(aio_key, aio_rw_flags).
    pub aio_key: u32,
    /// Per-I/O RWF_* flags.
    pub aio_rw_flags: u32,
    /// Operation (`IOCB_CMD_*`).
    pub aio_lio_opcode: u16,
    /// Request priority.
    pub aio_reqprio: i16,
    /// File descriptor.
    pub aio_fildes: u32,
    /// Buffer address.
    pub aio_buf: u64,
    /// Number of bytes.
    pub aio_nbytes: u64,
    /// File offset.
    pub aio_offset: i64,
    /// Reserved.
    _reserved2: u64,
    /// Flags (`IOCB_FLAG_*`).
    pub aio_flags: u32,
    /// eventfd for signal completion.
    pub aio_resfd: u32,
}

impl Iocb {
    /// Create a zeroed I/O control block.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

/// Completion event (32 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoEvent {
    /// Data from `iocb`.
    pub data: u64,
    /// `iocb` address.
    pub obj: u64,
    /// Result (bytes transferred or negative errno).
    pub res: i64,
    /// Secondary result.
    pub res2: i64,
}

impl IoEvent {
    /// Create a zeroed I/O event.
    #[must_use]
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Context table (process-local state)
// ---------------------------------------------------------------------------

/// Maximum number of concurrently-open AIO contexts.
const MAX_AIO_CONTEXTS: usize = 8;

/// Maximum number of queued completion events per context.  Caller
/// chooses an `nr_events` ≤ this in `io_setup`; if more iocbs are
/// submitted than the queue can hold, the oldest events are dropped
/// (matching Linux's behavior when the ring overflows: the kernel
/// returns `EAGAIN` from `io_submit`, but our synchronous executor
/// has no way to "fail later", so we record the overflow and surface
/// it as an early-termination of the submitting `io_submit` batch).
const MAX_EVENTS_PER_CTX: usize = 256;

#[derive(Clone, Copy)]
struct AioContext {
    in_use: bool,
    /// Caller-requested capacity, clamped to `MAX_EVENTS_PER_CTX`.
    capacity: usize,
    events: [IoEvent; MAX_EVENTS_PER_CTX],
    /// Index of the next event to dequeue from `events`.
    head: usize,
    /// Number of unread events currently queued.
    count: usize,
}

const EMPTY_CONTEXT: AioContext = AioContext {
    in_use: false,
    capacity: 0,
    events: [IoEvent {
        data: 0,
        obj: 0,
        res: 0,
        res2: 0,
    }; MAX_EVENTS_PER_CTX],
    head: 0,
    count: 0,
};

static AIO_LOCK: AtomicBool = AtomicBool::new(false);
process_global! {
    /// This process's `io_setup`/`io_destroy` context table.  A context
    /// handle is an encoded index into it, so it must not be shared between
    /// host test threads: `io_setup` in one test would hand back a handle
    /// another test's `io_destroy` could free.
    fn aio_contexts_ptr() -> [AioContext; MAX_AIO_CONTEXTS] =
        [EMPTY_CONTEXT; MAX_AIO_CONTEXTS];
}

/// RAII guard for the AIO spinlock.
struct AioLockGuard;
impl Drop for AioLockGuard {
    fn drop(&mut self) {
        AIO_LOCK.store(false, Ordering::Release);
    }
}

fn lock_aio() -> AioLockGuard {
    while AIO_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    AioLockGuard
}

/// Encode a slot index (0..MAX_AIO_CONTEXTS) as a context handle.
/// We add 1 so that handle `0` is always invalid (matches the kernel
/// convention that `0` means "context not yet created").
fn slot_to_ctx_id(slot: usize) -> u64 {
    (slot as u64).wrapping_add(1)
}

/// Decode a context handle back into a slot index, or `None` if the
/// handle is out of range.
fn ctx_id_to_slot(ctx_id: u64) -> Option<usize> {
    if ctx_id == 0 || ctx_id > MAX_AIO_CONTEXTS as u64 {
        return None;
    }
    Some((ctx_id - 1) as usize)
}

// ---------------------------------------------------------------------------
// io_setup
// ---------------------------------------------------------------------------

/// Create an AIO context.
///
/// Allocates a context capable of holding up to `nr_events` outstanding
/// completion events.  On success writes the new context handle to
/// `*ctx_idp` and returns 0; on failure returns -1 with `errno`:
///
/// - `EINVAL` if `nr_events` is 0 or > `MAX_EVENTS_PER_CTX`, or if
///   `*ctx_idp` is already non-zero (kernel convention: caller must
///   pre-zero the handle).
/// - `EFAULT` if `ctx_idp` is null.
/// - `EAGAIN` if no free context slot is available.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_setup(nr_events: u32, ctx_idp: *mut u64) -> i32 {
    if ctx_idp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if nr_events == 0 || nr_events as usize > MAX_EVENTS_PER_CTX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Kernel convention: *ctx_idp must be pre-zeroed by the caller.
    // SAFETY: caller-supplied non-null pointer; we read one u64.
    let existing = unsafe { *ctx_idp };
    if existing != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let _g = lock_aio();
    // SAFETY: serialized by AIO_LOCK.
    let table = unsafe { &mut *aio_contexts_ptr() };
    for (i, ctx) in table.iter_mut().enumerate() {
        if !ctx.in_use {
            *ctx = EMPTY_CONTEXT;
            ctx.in_use = true;
            ctx.capacity = nr_events as usize;
            // SAFETY: caller-supplied non-null pointer.
            unsafe { *ctx_idp = slot_to_ctx_id(i) };
            return 0;
        }
    }
    errno::set_errno(errno::EAGAIN);
    -1
}

// ---------------------------------------------------------------------------
// io_destroy
// ---------------------------------------------------------------------------

/// Destroy an AIO context.
///
/// Frees the context's slot.  Any unread events are discarded.
/// Returns 0 on success, -1 with `EINVAL` if `ctx_id` is invalid or
/// the context is not currently allocated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_destroy(ctx_id: u64) -> i32 {
    let Some(slot) = ctx_id_to_slot(ctx_id) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let _g = lock_aio();
    // SAFETY: serialized by AIO_LOCK.
    let table = unsafe { &mut *aio_contexts_ptr() };
    let Some(ctx) = table.get_mut(slot) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if !ctx.in_use {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    *ctx = EMPTY_CONTEXT;
    0
}

use crate::file::{PostWriteSync, plan_rw_flags};

/// Whether this opcode writes, and so whether a post-write sync applies.
fn is_write_opcode(op: u16) -> bool {
    op == IOCB_CMD_PWRITE || op == IOCB_CMD_PWRITEV
}

/// The eventfd to poke when this iocb completes, if the caller asked for one.
///
/// Split out from `io_submit` so the decision is testable: the eventfd itself
/// is a kernel object and does nothing on the host triple, but *whether* a
/// notification is owed, and *which* descriptor it is owed to, is arithmetic.
/// The bug this replaces was not a failed notification, it was never asking
/// the question at all.
fn completion_resfd(iocb: &Iocb) -> Option<i32> {
    if iocb.aio_flags & IOCB_FLAG_RESFD == 0 {
        return None;
    }
    // aio_resfd is a u32 in the ABI and a descriptor is an i32. A value that
    // does not fit is not a descriptor anyone could have opened, so there is
    // nothing to notify -- and casting it would invent a plausible small fd
    // out of a large one and poke a stranger's file.
    i32::try_from(iocb.aio_resfd).ok()
}

// ---------------------------------------------------------------------------
// io_submit
// ---------------------------------------------------------------------------

/// Translate a single iocb into a completion event by executing the
/// requested operation synchronously.
fn execute_iocb(iocb: &Iocb) -> IoEvent {
    // Clear errno so the post-dispatch wrapper can distinguish "the
    // primitive set errno" from "the opcode itself was the error".
    // Without this, a stale errno from a previous call could leak into
    // a successful read's accounting (and previously a thread-local
    // zero would mask the unknown-opcode arm's encoded EINVAL).
    errno::set_errno(0);

    let fd = iocb.aio_fildes as crate::types::Fd;

    // Per-I/O flags, decided BEFORE the I/O runs: a flag we cannot honour has
    // to stop the operation, not be discovered after the bytes have moved.
    // i32 for the shared policy; a u32 that does not fit is a bit pattern
    // no RWF_ flag occupies, so it is refused rather than truncated.
    let raw_flags = match i32::try_from(iocb.aio_rw_flags) {
        Ok(f) => f,
        Err(_) => {
            errno::set_errno(errno::EINVAL);
            return IoEvent {
                data: iocb.aio_data,
                obj: core::ptr::from_ref::<Iocb>(iocb) as u64,
                res: -i64::from(errno::EINVAL),
                res2: 0,
            };
        }
    };
    let sync_after = match plan_rw_flags(raw_flags, is_write_opcode(iocb.aio_lio_opcode)) {
        Ok(s) => s,
        Err(e) => {
            errno::set_errno(e);
            return IoEvent {
                data: iocb.aio_data,
                obj: core::ptr::from_ref::<Iocb>(iocb) as u64,
                res: -i64::from(e),
                res2: 0,
            };
        }
    };

    let res: i64 = match iocb.aio_lio_opcode {
        IOCB_CMD_PREAD => {
            // Reject buffer addresses that don't fit in a usize on this
            // platform.  Treat zero-length as success with res=0.
            let buf = iocb.aio_buf as *mut u8;
            let n = iocb.aio_nbytes as usize;
            let off = iocb.aio_offset;
            let r = crate::file::pread(fd, buf, n, off);
            i64::from(r as i32).max(r as i64)
        }
        IOCB_CMD_PWRITE => {
            let buf = iocb.aio_buf as *const u8;
            let n = iocb.aio_nbytes as usize;
            let off = iocb.aio_offset;
            let r = crate::file::pwrite(fd, buf, n, off);
            r as i64
        }
        IOCB_CMD_PREADV => {
            let iov = iocb.aio_buf as *const crate::file::Iovec;
            let iovcnt = iocb.aio_nbytes as i32;
            let off = iocb.aio_offset;
            let r = crate::file::preadv(fd, iov, iovcnt, off);
            r as i64
        }
        IOCB_CMD_PWRITEV => {
            let iov = iocb.aio_buf as *const crate::file::Iovec;
            let iovcnt = iocb.aio_nbytes as i32;
            let off = iocb.aio_offset;
            let r = crate::file::pwritev(fd, iov, iovcnt, off);
            r as i64
        }
        IOCB_CMD_FSYNC => i64::from(crate::file::fsync(fd)),
        IOCB_CMD_FDSYNC => i64::from(crate::file::fdatasync(fd)),
        IOCB_CMD_NOOP => 0,
        _ => {
            // Unknown opcode: set errno so the wrapper below routes
            // through the standard -1+errno→event path and produces
            // -EINVAL in `res`.  (Just returning -EINVAL here without
            // touching errno would let the wrapper substitute EIO when
            // errno happens to be 0.)
            errno::set_errno(errno::EINVAL);
            -1
        }
    };

    // The kernel reports negative errno in `res` on failure.  Our
    // primitives set `errno` and return -1.  Translate that into the
    // kernel convention.
    let final_res = if res < 0 {
        let e = errno::get_errno();
        -i64::from(if e == 0 { errno::EIO } else { e })
    } else {
        res
    };

    // The durability the caller asked for, applied once the bytes are written.
    let final_res = if final_res >= 0 && is_write_opcode(iocb.aio_lio_opcode) {
        let rc = match sync_after {
            PostWriteSync::None => 0,
            PostWriteSync::Data => crate::file::fdatasync(fd),
            PostWriteSync::Full => crate::file::fsync(fd),
        };
        if rc < 0 {
            // The bytes reached the file but not stable storage, and stable
            // storage is what was asked for. Returning the byte count here
            // would be exactly the durability lie this replaced: a caller
            // whose journal write "succeeded" and is gone after a power cut.
            let e = errno::get_errno();
            -i64::from(if e == 0 { errno::EIO } else { e })
        } else {
            final_res
        }
    } else {
        final_res
    };

    IoEvent {
        data: iocb.aio_data,
        obj: core::ptr::from_ref::<Iocb>(iocb) as u64,
        res: final_res,
        res2: 0,
    }
}

/// Submit AIO requests.
///
/// Iterates `iocbpp[0..nr]`, executes each iocb synchronously, and
/// appends the resulting `IoEvent` to the context's completion queue.
/// Returns the number of iocbs successfully submitted (which, for a
/// synchronous executor, equals the number of iocbs whose events were
/// recorded — once the context queue fills, further iocbs are
/// dropped and submission stops).
///
/// Errors:
/// - `EINVAL` if `ctx_id` is invalid, `nr < 0`, or any iocb pointer is null.
/// - `EFAULT` if `iocbpp` is null with `nr > 0`.
///
/// On error before submitting any iocb, returns -1.  If at least one
/// iocb was submitted before an error, the partial count is returned.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_submit(ctx_id: u64, nr: i64, iocbpp: *mut *mut Iocb) -> i64 {
    let Some(slot) = ctx_id_to_slot(ctx_id) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if nr < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let _g = lock_aio();
    // SAFETY: serialized by AIO_LOCK.
    let table = unsafe { &mut *aio_contexts_ptr() };
    let Some(ctx) = table.get_mut(slot) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if !ctx.in_use {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if nr == 0 {
        return 0;
    }
    if iocbpp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let mut submitted: i64 = 0;
    let n_request = nr as usize;
    for i in 0..n_request {
        // SAFETY: caller asserts iocbpp is valid for nr entries.
        let iocb_ptr = unsafe { *iocbpp.add(i) };
        if iocb_ptr.is_null() {
            if submitted == 0 {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            return submitted;
        }
        // Refuse to overflow the context queue: report partial count.
        if ctx.count >= ctx.capacity {
            if submitted == 0 {
                errno::set_errno(errno::EAGAIN);
                return -1;
            }
            return submitted;
        }

        // SAFETY: non-null iocb pointer asserted by caller.
        let iocb = unsafe { &*iocb_ptr };
        let event = execute_iocb(iocb);

        // Enqueue at tail (wrap-around within MAX_EVENTS_PER_CTX, but
        // we cap by `capacity` for liveness).
        let tail = (ctx.head + ctx.count) % MAX_EVENTS_PER_CTX;
        if let Some(slot_ref) = ctx.events.get_mut(tail) {
            *slot_ref = event;
            ctx.count += 1;
            submitted += 1;
            // Notify the caller's eventfd, if it asked for one. Done here
            // rather than after the batch so the eventfd is never readable
            // before the event it announces is queued -- a waiter that woke
            // early and called io_getevents would get nothing and could
            // reasonably conclude the notification was spurious.
            //
            // LOCK ORDER: this takes the descriptor table's lock while the
            // AIO lock is held. That is the same order `execute_iocb` above
            // already establishes by doing read/write on a descriptor under
            // this lock, so it adds no new edge and cannot introduce a cycle.
            if let Some(resfd) = completion_resfd(iocb) {
                // The return value is deliberately not propagated: the I/O
                // itself already succeeded and its completion is queued, so
                // failing the submission over an undeliverable notification
                // would discard a real result. `io_getevents` still returns
                // it. See the module docs for the Linux divergence.
                let _ = crate::epoll::eventfd_write(resfd, 1);
            }
        } else {
            // Unreachable given the modulo, but keep it defensive.
            return submitted;
        }
    }
    submitted
}

// ---------------------------------------------------------------------------
// io_getevents
// ---------------------------------------------------------------------------

/// Get AIO completion events.
///
/// Copies up to `nr` queued events from the context into `events`.
/// Returns the number copied (which is always ≥ `min_nr` for a
/// synchronous backend, because every submitted iocb has already
/// completed).  The `timeout` argument is ignored.
///
/// Errors:
/// - `EINVAL` if `ctx_id` is invalid, `min_nr < 0`, `nr < 0`, or
///   `min_nr > nr`.
/// - `EFAULT` if `events` is null with `nr > 0`.
/// - `EAGAIN` if fewer than `min_nr` events are queued (would block
///   waiting for more, but since we don't have a real async path, we
///   surface this synchronously).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn io_getevents(
    ctx_id: u64,
    min_nr: i64,
    nr: i64,
    events: *mut IoEvent,
    _timeout: *mut u8,
) -> i64 {
    let Some(slot) = ctx_id_to_slot(ctx_id) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if min_nr < 0 || nr < 0 || min_nr > nr {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if nr == 0 {
        return 0;
    }
    if events.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let _g = lock_aio();
    // SAFETY: serialized by AIO_LOCK.
    let table = unsafe { &mut *aio_contexts_ptr() };
    let Some(ctx) = table.get_mut(slot) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if !ctx.in_use {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    if (ctx.count as i64) < min_nr {
        // Synchronous backend: we cannot wait for events that haven't
        // been submitted.  Surface EAGAIN rather than spinning forever.
        errno::set_errno(errno::EAGAIN);
        return -1;
    }

    let want = (nr as usize).min(ctx.count);
    for i in 0..want {
        let src_idx = (ctx.head + i) % MAX_EVENTS_PER_CTX;
        // SAFETY: events is non-null and the caller asserts it can hold
        // `nr` IoEvent entries.
        unsafe {
            *events.add(i) = ctx.events[src_idx];
        }
    }
    ctx.head = (ctx.head + want) % MAX_EVENTS_PER_CTX;
    ctx.count -= want;
    want as i64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // The `Mutex` and the `reset_aio_state()` helper these tests used to
    // open with are gone: the context table is per-thread on host builds,
    // so every test already starts from an empty table that no other test
    // can reach.  The production `lock_aio()` spinlock is untouched -- it
    // is what makes the table safe on the target.

    #[test]
    fn test_iocb_size() {
        assert_eq!(core::mem::size_of::<Iocb>(), 64);
    }

    #[test]
    fn test_io_event_size() {
        assert_eq!(core::mem::size_of::<IoEvent>(), 32);
    }

    #[test]
    fn test_iocb_zeroed() {
        let iocb = Iocb::zeroed();
        assert_eq!(iocb.aio_data, 0);
        assert_eq!(iocb.aio_lio_opcode, 0);
        assert_eq!(iocb.aio_fildes, 0);
        assert_eq!(iocb.aio_buf, 0);
        assert_eq!(iocb.aio_nbytes, 0);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            IOCB_CMD_PREAD,
            IOCB_CMD_PWRITE,
            IOCB_CMD_FSYNC,
            IOCB_CMD_FDSYNC,
            IOCB_CMD_NOOP,
            IOCB_CMD_PREADV,
            IOCB_CMD_PWRITEV,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags() {
        assert_eq!(IOCB_FLAG_RESFD, 1);
        assert_eq!(IOCB_FLAG_IOPRIO, 2);
        assert_eq!(IOCB_FLAG_RESFD & IOCB_FLAG_IOPRIO, 0);
    }

    // -- io_setup: validation & allocation --

    #[test]
    fn test_io_setup_null_ctxidp_efault() {
        errno::set_errno(0);
        let ret = io_setup(8, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_io_setup_zero_nr_einval() {
        let mut ctx: u64 = 0;
        errno::set_errno(0);
        assert_eq!(io_setup(0, &mut ctx as *mut u64), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(ctx, 0);
    }

    #[test]
    fn test_io_setup_too_large_nr_einval() {
        let mut ctx: u64 = 0;
        errno::set_errno(0);
        assert_eq!(
            io_setup((MAX_EVENTS_PER_CTX + 1) as u32, &mut ctx as *mut u64),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_setup_nonzero_ctxidp_einval() {
        // Kernel convention: caller must pre-zero *ctx_idp.
        let mut ctx: u64 = 42;
        errno::set_errno(0);
        assert_eq!(io_setup(8, &mut ctx as *mut u64), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_setup_succeeds_returns_nonzero_handle() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(8, &mut ctx as *mut u64), 0);
        assert_ne!(ctx, 0);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_setup_distinct_handles() {
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        assert_eq!(io_setup(4, &mut a as *mut u64), 0);
        assert_eq!(io_setup(4, &mut b as *mut u64), 0);
        assert_ne!(a, b);
        assert_eq!(io_destroy(a), 0);
        assert_eq!(io_destroy(b), 0);
    }

    #[test]
    fn test_io_setup_exhaustion_eagain() {
        let mut handles = [0u64; MAX_AIO_CONTEXTS];
        for h in handles.iter_mut() {
            assert_eq!(io_setup(1, h as *mut u64), 0);
        }
        let mut overflow: u64 = 0;
        errno::set_errno(0);
        assert_eq!(io_setup(1, &mut overflow as *mut u64), -1);
        assert_eq!(errno::get_errno(), errno::EAGAIN);
        for h in handles.iter() {
            assert_eq!(io_destroy(*h), 0);
        }
    }

    // -- io_destroy --

    #[test]
    fn test_io_destroy_zero_handle_einval() {
        errno::set_errno(0);
        assert_eq!(io_destroy(0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_destroy_oob_handle_einval() {
        errno::set_errno(0);
        assert_eq!(io_destroy(9999), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_destroy_unallocated_einval() {
        errno::set_errno(0);
        // Slot 1 is in range but never allocated.
        assert_eq!(io_destroy(1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_destroy_after_setup_succeeds() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        assert_eq!(io_destroy(ctx), 0);
        // Double-destroy fails.
        errno::set_errno(0);
        assert_eq!(io_destroy(ctx), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- io_submit: completion notification (IOCB_FLAG_RESFD) --

    /// An iocb with everything zeroed, so each test sets only what it means.
    fn blank_iocb() -> Iocb {
        // SAFETY: Iocb is a plain repr(C) POD of integers; all-zero is a
        // valid value for every field and is what a caller that memsets its
        // iocb (the usual idiom) actually passes.
        unsafe { core::mem::zeroed() }
    }

    #[test]
    fn no_resfd_flag_means_no_notification() {
        let iocb = blank_iocb();
        assert_eq!(completion_resfd(&iocb), None);
    }

    #[test]
    fn resfd_flag_names_the_descriptor_to_notify() {
        // The regression pin. This used to be unasked: aio_resfd was a field
        // nothing read, so a caller waiting on the eventfd waited forever.
        let mut iocb = blank_iocb();
        iocb.aio_flags = IOCB_FLAG_RESFD;
        iocb.aio_resfd = 7;
        assert_eq!(completion_resfd(&iocb), Some(7));
    }

    #[test]
    fn resfd_zero_is_a_real_descriptor() {
        // fd 0 is stdin and a perfectly legal eventfd target; treating the
        // zero value as "unset" would silently drop exactly one caller.
        let mut iocb = blank_iocb();
        iocb.aio_flags = IOCB_FLAG_RESFD;
        iocb.aio_resfd = 0;
        assert_eq!(completion_resfd(&iocb), Some(0));
    }

    #[test]
    fn a_resfd_too_large_for_a_descriptor_is_not_notified() {
        // u32 in the ABI, i32 as a descriptor. Casting would turn a huge
        // value into a plausible small fd and poke an unrelated file.
        let mut iocb = blank_iocb();
        iocb.aio_flags = IOCB_FLAG_RESFD;
        iocb.aio_resfd = u32::MAX;
        assert_eq!(completion_resfd(&iocb), None);
        iocb.aio_resfd = 0x8000_0000;
        assert_eq!(completion_resfd(&iocb), None);
        iocb.aio_resfd = 0x7fff_ffff;
        assert_eq!(completion_resfd(&iocb), Some(0x7fff_ffff));
    }

    #[test]
    fn other_flags_do_not_request_a_notification() {
        let mut iocb = blank_iocb();
        iocb.aio_flags = IOCB_FLAG_IOPRIO;
        iocb.aio_resfd = 7;
        assert_eq!(completion_resfd(&iocb), None);
        // ...but set alongside RESFD it still notifies.
        iocb.aio_flags = IOCB_FLAG_IOPRIO | IOCB_FLAG_RESFD;
        assert_eq!(completion_resfd(&iocb), Some(7));
    }

    // -- io_submit: validation --

    #[test]
    fn test_io_submit_bad_ctx_einval() {
        errno::set_errno(0);
        assert_eq!(io_submit(0, 1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_submit_negative_nr_einval() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        errno::set_errno(0);
        assert_eq!(io_submit(ctx, -1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_submit_zero_nr_returns_zero() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        assert_eq!(io_submit(ctx, 0, core::ptr::null_mut()), 0);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_submit_null_iocbpp_efault() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        errno::set_errno(0);
        assert_eq!(io_submit(ctx, 1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_submit_unallocated_ctx_einval() {
        errno::set_errno(0);
        // Slot 1 is in range but never allocated.
        assert_eq!(io_submit(1, 1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- io_submit: execution & queueing --

    #[test]
    fn test_io_submit_noop_queues_event() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);

        let mut iocb = Iocb::zeroed();
        iocb.aio_lio_opcode = IOCB_CMD_NOOP;
        iocb.aio_data = 0xDEAD_BEEF;
        let mut iocb_ptr: *mut Iocb = &mut iocb as *mut Iocb;
        let pp: *mut *mut Iocb = &mut iocb_ptr;
        assert_eq!(io_submit(ctx, 1, pp), 1);

        let mut ev = IoEvent::zeroed();
        let got = io_getevents(ctx, 1, 1, &mut ev as *mut IoEvent, core::ptr::null_mut());
        assert_eq!(got, 1);
        assert_eq!(ev.data, 0xDEAD_BEEF);
        assert_eq!(ev.res, 0);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_submit_unknown_opcode_reports_einval_in_event() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);

        let mut iocb = Iocb::zeroed();
        iocb.aio_lio_opcode = 99; // bogus
        let mut iocb_ptr: *mut Iocb = &mut iocb as *mut Iocb;
        let pp: *mut *mut Iocb = &mut iocb_ptr;
        assert_eq!(io_submit(ctx, 1, pp), 1);

        let mut ev = IoEvent::zeroed();
        let got = io_getevents(ctx, 1, 1, &mut ev as *mut IoEvent, core::ptr::null_mut());
        assert_eq!(got, 1);
        assert_eq!(ev.res, -i64::from(errno::EINVAL));
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_submit_capacity_overflow_partial_then_eagain() {
        let mut ctx: u64 = 0;
        // Capacity of 2.
        assert_eq!(io_setup(2, &mut ctx as *mut u64), 0);

        let mut a = Iocb::zeroed();
        a.aio_lio_opcode = IOCB_CMD_NOOP;
        let mut b = Iocb::zeroed();
        b.aio_lio_opcode = IOCB_CMD_NOOP;
        let mut c = Iocb::zeroed();
        c.aio_lio_opcode = IOCB_CMD_NOOP;
        let mut ptrs: [*mut Iocb; 3] = [
            &mut a as *mut Iocb,
            &mut b as *mut Iocb,
            &mut c as *mut Iocb,
        ];
        let pp: *mut *mut Iocb = ptrs.as_mut_ptr();
        // Should submit 2 and stop.
        assert_eq!(io_submit(ctx, 3, pp), 2);

        // A subsequent submit with the queue full returns -1/EAGAIN
        // because nothing was submitted in *this* call.
        let mut d = Iocb::zeroed();
        d.aio_lio_opcode = IOCB_CMD_NOOP;
        let mut dptr: *mut Iocb = &mut d as *mut Iocb;
        let pp2: *mut *mut Iocb = &mut dptr;
        errno::set_errno(0);
        assert_eq!(io_submit(ctx, 1, pp2), -1);
        assert_eq!(errno::get_errno(), errno::EAGAIN);
        assert_eq!(io_destroy(ctx), 0);
    }

    // -- io_getevents --

    #[test]
    fn test_io_getevents_bad_ctx_einval() {
        let mut ev = IoEvent::zeroed();
        errno::set_errno(0);
        assert_eq!(
            io_getevents(0, 0, 1, &mut ev as *mut IoEvent, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_getevents_negative_args_einval() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        let mut ev = IoEvent::zeroed();
        errno::set_errno(0);
        assert_eq!(
            io_getevents(ctx, -1, 1, &mut ev as *mut IoEvent, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        assert_eq!(
            io_getevents(ctx, 1, -1, &mut ev as *mut IoEvent, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        assert_eq!(
            io_getevents(ctx, 5, 1, &mut ev as *mut IoEvent, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_getevents_zero_nr_returns_zero() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        assert_eq!(
            io_getevents(ctx, 0, 0, core::ptr::null_mut(), core::ptr::null_mut()),
            0
        );
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_getevents_null_events_efault() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        errno::set_errno(0);
        assert_eq!(
            io_getevents(ctx, 0, 1, core::ptr::null_mut(), core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_getevents_eagain_when_below_min_nr() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        let mut ev = IoEvent::zeroed();
        errno::set_errno(0);
        // No events submitted; min_nr=1 cannot be satisfied.
        assert_eq!(
            io_getevents(ctx, 1, 1, &mut ev as *mut IoEvent, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EAGAIN);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_getevents_drains_in_fifo_order() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);

        let mut iocbs: [Iocb; 3] = [Iocb::zeroed(); 3];
        for (i, ic) in iocbs.iter_mut().enumerate() {
            ic.aio_lio_opcode = IOCB_CMD_NOOP;
            ic.aio_data = (i as u64).wrapping_add(100);
        }
        let mut ptrs: [*mut Iocb; 3] = [
            &mut iocbs[0] as *mut Iocb,
            &mut iocbs[1] as *mut Iocb,
            &mut iocbs[2] as *mut Iocb,
        ];
        assert_eq!(io_submit(ctx, 3, ptrs.as_mut_ptr()), 3);

        let mut out = [IoEvent::zeroed(); 3];
        let got = io_getevents(ctx, 1, 3, out.as_mut_ptr(), core::ptr::null_mut());
        assert_eq!(got, 3);
        assert_eq!(out[0].data, 100);
        assert_eq!(out[1].data, 101);
        assert_eq!(out[2].data, 102);

        // Queue should now be empty.
        let mut ev = IoEvent::zeroed();
        errno::set_errno(0);
        assert_eq!(
            io_getevents(ctx, 1, 1, &mut ev as *mut IoEvent, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EAGAIN);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_getevents_partial_drain_then_more() {
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);

        let mut iocbs: [Iocb; 4] = [Iocb::zeroed(); 4];
        for (i, ic) in iocbs.iter_mut().enumerate() {
            ic.aio_lio_opcode = IOCB_CMD_NOOP;
            ic.aio_data = (i as u64).wrapping_add(1);
        }
        let mut ptrs: [*mut Iocb; 4] = [
            &mut iocbs[0] as *mut Iocb,
            &mut iocbs[1] as *mut Iocb,
            &mut iocbs[2] as *mut Iocb,
            &mut iocbs[3] as *mut Iocb,
        ];
        assert_eq!(io_submit(ctx, 4, ptrs.as_mut_ptr()), 4);

        let mut out = [IoEvent::zeroed(); 2];
        let got = io_getevents(ctx, 2, 2, out.as_mut_ptr(), core::ptr::null_mut());
        assert_eq!(got, 2);
        assert_eq!(out[0].data, 1);
        assert_eq!(out[1].data, 2);

        let mut more = [IoEvent::zeroed(); 4];
        let got2 = io_getevents(ctx, 1, 4, more.as_mut_ptr(), core::ptr::null_mut());
        assert_eq!(got2, 2);
        assert_eq!(more[0].data, 3);
        assert_eq!(more[1].data, 4);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_submit_writes_obj_back_to_event() {
        // The kernel sets event.obj to the address of the originating
        // iocb so callers can correlate completions with submissions.
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);

        let mut iocb = Iocb::zeroed();
        iocb.aio_lio_opcode = IOCB_CMD_NOOP;
        let iocb_addr = core::ptr::from_ref::<Iocb>(&iocb) as u64;
        let mut iocb_ptr: *mut Iocb = &mut iocb as *mut Iocb;
        let pp: *mut *mut Iocb = &mut iocb_ptr;
        assert_eq!(io_submit(ctx, 1, pp), 1);

        let mut ev = IoEvent::zeroed();
        assert_eq!(
            io_getevents(ctx, 1, 1, &mut ev as *mut IoEvent, core::ptr::null_mut()),
            1
        );
        assert_eq!(ev.obj, iocb_addr);
        assert_eq!(io_destroy(ctx), 0);
    }
}

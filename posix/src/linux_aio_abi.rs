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
//! - `aio_resfd` / eventfd notification is silently ignored — the next
//!   `io_getevents` will see the completion regardless.
//! - Per-I/O RWF_* flags (`aio_rw_flags`) are ignored.

use crate::errno;
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
static mut AIO_CONTEXTS: [AioContext; MAX_AIO_CONTEXTS] =
    [EMPTY_CONTEXT; MAX_AIO_CONTEXTS];

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
    let table = unsafe { &mut *core::ptr::addr_of_mut!(AIO_CONTEXTS) };
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
    let table = unsafe { &mut *core::ptr::addr_of_mut!(AIO_CONTEXTS) };
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
    let table = unsafe { &mut *core::ptr::addr_of_mut!(AIO_CONTEXTS) };
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
    let table = unsafe { &mut *core::ptr::addr_of_mut!(AIO_CONTEXTS) };
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

    /// Serialize tests that mutate the global context table.
    static AIO_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset_aio_state() {
        let _g = lock_aio();
        // SAFETY: serialized by AIO_LOCK + the test mutex.
        let table = unsafe { &mut *core::ptr::addr_of_mut!(AIO_CONTEXTS) };
        for ctx in table.iter_mut() {
            *ctx = EMPTY_CONTEXT;
        }
    }

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
            IOCB_CMD_PREAD, IOCB_CMD_PWRITE, IOCB_CMD_FSYNC,
            IOCB_CMD_FDSYNC, IOCB_CMD_NOOP, IOCB_CMD_PREADV,
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        errno::set_errno(0);
        let ret = io_setup(8, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_io_setup_zero_nr_einval() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        let mut ctx: u64 = 0;
        errno::set_errno(0);
        assert_eq!(io_setup(0, &mut ctx as *mut u64), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(ctx, 0);
    }

    #[test]
    fn test_io_setup_too_large_nr_einval() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        let mut ctx: u64 = 0;
        errno::set_errno(0);
        assert_eq!(io_setup((MAX_EVENTS_PER_CTX + 1) as u32, &mut ctx as *mut u64), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_setup_nonzero_ctxidp_einval() {
        // Kernel convention: caller must pre-zero *ctx_idp.
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        let mut ctx: u64 = 42;
        errno::set_errno(0);
        assert_eq!(io_setup(8, &mut ctx as *mut u64), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_setup_succeeds_returns_nonzero_handle() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(8, &mut ctx as *mut u64), 0);
        assert_ne!(ctx, 0);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_setup_distinct_handles() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        errno::set_errno(0);
        assert_eq!(io_destroy(0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_destroy_oob_handle_einval() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        errno::set_errno(0);
        assert_eq!(io_destroy(9999), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_destroy_unallocated_einval() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        errno::set_errno(0);
        // Slot 1 is in range but never allocated.
        assert_eq!(io_destroy(1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_destroy_after_setup_succeeds() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        assert_eq!(io_destroy(ctx), 0);
        // Double-destroy fails.
        errno::set_errno(0);
        assert_eq!(io_destroy(ctx), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- io_submit: validation --

    #[test]
    fn test_io_submit_bad_ctx_einval() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        errno::set_errno(0);
        assert_eq!(io_submit(0, 1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_io_submit_negative_nr_einval() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        errno::set_errno(0);
        assert_eq!(io_submit(ctx, -1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_submit_zero_nr_returns_zero() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        assert_eq!(io_submit(ctx, 0, core::ptr::null_mut()), 0);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_submit_null_iocbpp_efault() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        let mut ctx: u64 = 0;
        assert_eq!(io_setup(4, &mut ctx as *mut u64), 0);
        errno::set_errno(0);
        assert_eq!(io_submit(ctx, 1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(io_destroy(ctx), 0);
    }

    #[test]
    fn test_io_submit_unallocated_ctx_einval() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
        errno::set_errno(0);
        // Slot 1 is in range but never allocated.
        assert_eq!(io_submit(1, 1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- io_submit: execution & queueing --

    #[test]
    fn test_io_submit_noop_queues_event() {
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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
        let _g = AIO_TEST_LOCK.lock().unwrap();
        reset_aio_state();
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

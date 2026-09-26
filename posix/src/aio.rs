//! POSIX asynchronous I/O (`<aio.h>`): glibc 2.39's semantics, performed
//! synchronously.
//!
//! `aio_read`, `aio_write`, `aio_fsync` and `lio_listio` perform each
//! request before they return, in the calling thread, and record its outcome
//! in the `aiocb` itself -- the two private words musl keeps there (`__err`
//! and `__ret`; glibc's `__error_code` and `__return_value`) -- where
//! `aio_error` and `aio_return` read it back.  What a caller can observe is
//! glibc's thread-pool implementation (rt/aio_misc.c and friends) except for
//! timing:
//!
//! - **Refused at once**, with `-1` and `errno`, is only what glibc refuses
//!   before it queues a request: a NULL `aiocb` (`EFAULT`, this libc's
//!   substitute for the fault, `design-decisions.md` §303), a priority
//!   outside `0..=AIO_PRIO_DELTA_MAX` (`EINVAL`), and for `aio_fsync` a bad
//!   `op` or a descriptor that is not open.  Everything else -- a bad
//!   descriptor, a bad buffer, a bad offset, an opcode `lio_listio` does not
//!   know -- is the request's own outcome, read through `aio_error` and
//!   `aio_return`, as glibc's worker thread reports it.
//! - **A request is `EINPROGRESS`** only while another thread of the process
//!   is performing it.  `aio_suspend` waits for such requests and
//!   `aio_cancel` answers `AIO_NOTCANCELED` for them; nothing is ever queued,
//!   so nothing is ever `AIO_CANCELED`.
//! - **Completion is notified** as the request's `aio_sigevent` asks:
//!   `SIGEV_THREAD` runs the function on a new detached thread, and
//!   `SIGEV_SIGNAL` raises the signal -- without its value, since
//!   `sigqueue` cannot deliver one yet; plain `raise` is glibc's own choice
//!   on a system without queued signals.
//!
//! ## What changed on 2026-09-26
//!
//! Every request's outcome used to live in a 16-entry table keyed by the
//! `aiocb`'s address, and the 17th request evicted the oldest outcome whether
//! or not anyone had collected it -- after which `aio_error` answered
//! `EINVAL`, which reads as "that I/O failed with EINVAL".  A bad descriptor,
//! buffer or offset was refused synchronously; `aio_suspend` refused an
//! empty list; `lio_listio` refused an unknown opcode synchronously and gave
//! the last request's errno where glibc gives `EIO`; `aio_cancel` answered
//! `AIO_ALLDONE` for a descriptor that is not open; `aio_fsync(O_DSYNC)` ran
//! `fsync`; a positioned read on a pipe failed with `ESPIPE` where glibc
//! falls back to a plain read; and `aio_sigevent` was ignored, so a program
//! waiting for its `SIGEV_THREAD` callback waited forever.
//! (`known-issues.md` → `B-D-AIO-OUTCOMES-EVICTED-AND-NEVER-NOTIFIED`.)

use crate::errno;
use core::sync::atomic::{AtomicI32, AtomicIsize, Ordering};

// ---------------------------------------------------------------------------
// aiocb — async I/O control block
// ---------------------------------------------------------------------------

/// Async I/O control block -- musl's `struct aiocb`, 168 bytes.
///
/// # "Matches the POSIX layout" was the claim, and the order was not musl's
///
/// This declared `aio_fildes, aio_offset, aio_buf, ...` and came to 136 bytes.
/// musl's order is `aio_fildes, aio_lio_opcode, aio_reqprio, aio_buf,
/// aio_nbytes, aio_sigevent`, then 32 bytes of its own per-request state, then
/// `aio_offset` at 128 and 32 more to 168. Measured with
/// `zig cc --target=x86_64-linux-musl`:
///
/// | field | ours, before | musl |
/// |---|---|---|
/// | `aio_offset` | 8 | 128 |
/// | `aio_reqprio` | 32 | 8 |
/// | `aio_sigevent` | 36 | 32 |
/// | `aio_lio_opcode` | 100 | 4 |
/// | size | 136 | 168 |
///
/// Every one of those is a field a C caller fills in before calling
/// `aio_read`, so a program setting an offset was setting the opcode. "Matches
/// the POSIX `struct aiocb` layout" is the kind of claim POSIX cannot settle:
/// POSIX names the members and leaves the order to the implementation, so
/// there is no such thing as *the* POSIX layout to match -- only a particular
/// C library's. Nothing in this tree calls the aio family, which is the only
/// reason it went unnoticed.
///
/// Found by `scripts/check-libc-abi.py`; `design-decisions.md` §1011.  Two
/// of musl's private words hold each request's outcome here, as they do in
/// musl.
#[repr(C)]
pub struct Aiocb {
    /// File descriptor.
    pub aio_fildes: i32,
    /// Operation for `lio_listio` (`LIO_READ`, `LIO_WRITE`, `LIO_NOP`).
    pub aio_lio_opcode: i32,
    /// Request priority offset, `0..=AIO_PRIO_DELTA_MAX`.
    pub aio_reqprio: i32,
    /// Alignment before the first pointer. musl's struct has the same hole.
    __pad0: u32,
    /// Buffer for I/O.
    pub aio_buf: *mut u8,
    /// Number of bytes to read/write.
    pub aio_nbytes: usize,
    /// How completion is notified: a `struct sigevent`.
    ///
    /// Opaque bytes rather than a type, because this crate's `Sigevent` does
    /// not name the `SIGEV_THREAD` fields; [`SigeventView`] reads them.  64
    /// is the right *size*, measured, so a caller's `aio_offset` lands where
    /// it belongs either way.
    pub aio_sigevent: [u8; 64],
    /// musl's `__td` and `__lock`.  Unused here.
    __private1: [u8; 16],
    /// The request's error status, as `aio_error` reports it: `EINPROGRESS`
    /// while it is being performed, then 0 or the error it failed with.
    /// musl's `__err`.
    error_code: AtomicI32,
    /// Alignment, as in musl.
    __pad1: u32,
    /// The request's return value, as `aio_return` reports it.  musl's
    /// `__ret`.
    return_value: AtomicIsize,
    /// Offset within file.
    pub aio_offset: i64,
    /// The rest of musl's private state (`__next`, `__prev`, padding), taking
    /// the structure to 168.  Unused here.
    __private2: [u8; 32],
}

/// The layout is musl's; the outcome words sit where musl keeps `__err` and
/// `__ret`, and nothing moved the public fields round them.
const _: () = {
    assert!(size_of::<Aiocb>() == 168);
    assert!(core::mem::offset_of!(Aiocb, aio_sigevent) == 32);
    assert!(core::mem::offset_of!(Aiocb, error_code) == 112);
    assert!(core::mem::offset_of!(Aiocb, return_value) == 120);
    assert!(core::mem::offset_of!(Aiocb, aio_offset) == 128);
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// `lio_listio`: wait for every request to complete.
pub const LIO_WAIT: i32 = 0;
/// `lio_listio`: return at once, and notify through `sig` when all complete.
pub const LIO_NOWAIT: i32 = 1;

/// Read operation for lio_listio.
pub const LIO_READ: i32 = 0;
/// Write operation for lio_listio.
pub const LIO_WRITE: i32 = 1;
/// No-op for lio_listio.
pub const LIO_NOP: i32 = 2;

/// Every request named was cancelled.
pub const AIO_CANCELED: i32 = 0;
/// A request named could not be cancelled: it is being performed.
pub const AIO_NOTCANCELED: i32 = 1;
/// Every request named had already completed.
pub const AIO_ALLDONE: i32 = 2;

/// The largest `aio_reqprio`: glibc's `<bits/local_lim.h>`.  Past it, or
/// below zero, a request is refused (`EINVAL`) before it is performed.
pub const AIO_PRIO_DELTA_MAX: i32 = 20;

// ---------------------------------------------------------------------------
// Process-wide state: completions and requests in flight
// ---------------------------------------------------------------------------

/// A request being performed, linked into the process's in-flight list for
/// as long as it runs, so that `aio_cancel(fd, NULL)` can tell whether any
/// request on `fd` is still going.  The node lives on the performing
/// thread's stack.
struct InFlight {
    fd: i32,
    next: *mut InFlight,
}

/// The requests being performed right now.
struct InFlightList {
    /// A low-level lock over `head` (`crate::lowlevellock`).
    lock: AtomicI32,
    head: *mut InFlight,
}

crate::perprocess::process_global! {
    /// Advanced by every completion; `aio_suspend` sleeps on it.
    fn completions() -> AtomicI32 = AtomicI32::new(0);
    /// Threads asleep in `aio_suspend`, so that a completion nobody is
    /// waiting for costs no syscall.
    fn suspended() -> AtomicI32 = AtomicI32::new(0);
    /// See [`InFlight`].
    fn in_flight() -> InFlightList = InFlightList {
        lock: AtomicI32::new(0),
        head: core::ptr::null_mut(),
    };
}

/// Unlinks its node from the in-flight list when dropped -- on every way out
/// of [`with_in_flight`].
struct InFlightGuard(*mut InFlight);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        let list = in_flight();
        // SAFETY: `list` is this process's list, whose `head` and links are
        // only touched under its lock; `self.0` is a node linked by
        // `with_in_flight`, still alive on its stack frame.
        unsafe {
            crate::lowlevellock::lll_lock(&(*list).lock);
            let mut link: *mut *mut InFlight = &raw mut (*list).head;
            while !(*link).is_null() {
                if *link == self.0 {
                    *link = (*self.0).next;
                    break;
                }
                link = &raw mut (**link).next;
            }
            crate::lowlevellock::lll_unlock(&(*list).lock);
        }
    }
}

/// Run `op` with `fd` in the in-flight list.
fn with_in_flight<R>(fd: i32, op: impl FnOnce() -> R) -> R {
    let mut node = InFlight {
        fd,
        next: core::ptr::null_mut(),
    };
    let node_ptr: *mut InFlight = &raw mut node;
    let list = in_flight();
    // SAFETY: as in `InFlightGuard::drop`; the node outlives its membership,
    // because the guard below unlinks it before this frame ends.
    unsafe {
        crate::lowlevellock::lll_lock(&(*list).lock);
        (*node_ptr).next = (*list).head;
        (*list).head = node_ptr;
        crate::lowlevellock::lll_unlock(&(*list).lock);
    }
    let _guard = InFlightGuard(node_ptr);
    op()
}

/// Is a request on `fd` being performed right now?
fn fd_in_flight(fd: i32) -> bool {
    let list = in_flight();
    // SAFETY: as in `InFlightGuard::drop`.
    unsafe {
        crate::lowlevellock::lll_lock(&(*list).lock);
        let mut node = (*list).head;
        let mut found = false;
        while !node.is_null() {
            if (*node).fd == fd {
                found = true;
                break;
            }
            node = (*node).next;
        }
        crate::lowlevellock::lll_unlock(&(*list).lock);
        found
    }
}

/// Wake the threads in `aio_suspend`: a request has completed.
fn completed() {
    // SAFETY: `completions()`/`suspended()` are this process's words.
    let (seq, sleepers) = unsafe { (&*completions(), &*suspended()) };
    seq.fetch_add(1, Ordering::SeqCst);
    if sleepers.load(Ordering::SeqCst) > 0 {
        crate::lowlevellock::futex_wake_all(seq);
    }
}

/// Is `fd` an open descriptor?  glibc's test is `fcntl(fd, F_GETFL)`.
fn fd_is_open(fd: i32) -> bool {
    crate::fcntl_ops::fcntl(fd, crate::fcntl_ops::F_GETFL, 0) != -1
}

// ---------------------------------------------------------------------------
// Performing a request
// ---------------------------------------------------------------------------

/// What a request does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Read,
    Write,
    /// `aio_fsync(O_SYNC, ...)`.
    Sync,
    /// `aio_fsync(O_DSYNC, ...)`.
    DataSync,
    /// An `lio_listio` opcode that is none of the above: performed as a
    /// failure, `EINVAL`, as glibc's worker performs it.
    Invalid,
}

/// Record a request's outcome where `aio_error` and `aio_return` read it.
fn record(cb: &Aiocb, ret: isize, err: i32) {
    cb.return_value.store(ret, Ordering::Release);
    cb.error_code.store(err, Ordering::Release);
}

/// `f` until it is not interrupted: glibc's `TEMP_FAILURE_RETRY`.
fn retry(mut f: impl FnMut() -> isize) -> isize {
    loop {
        let r = f();
        if r != -1 || errno::get_errno() != errno::EINTR {
            return r;
        }
    }
}

/// Perform the request, as glibc's worker does (`handle_fildes_io`): the
/// result, or -1 with `errno` set.  A positioned read or write that Linux
/// refuses with `ESPIPE` -- a pipe, a socket -- is retried as a plain one.
fn perform(cb: &Aiocb, op: Op) -> isize {
    let (fd, buf, n, off) = (cb.aio_fildes, cb.aio_buf, cb.aio_nbytes, cb.aio_offset);
    match op {
        Op::Read => {
            let r = retry(|| crate::file::pread(fd, buf, n, off));
            if r == -1 && errno::get_errno() == errno::ESPIPE {
                retry(|| crate::file::read(fd, buf, n))
            } else {
                r
            }
        }
        Op::Write => {
            let r = retry(|| crate::file::pwrite(fd, buf.cast_const(), n, off));
            if r == -1 && errno::get_errno() == errno::ESPIPE {
                retry(|| crate::file::write(fd, buf.cast_const(), n))
            } else {
                r
            }
        }
        Op::DataSync => retry(|| crate::file::fdatasync(fd) as isize),
        Op::Sync => retry(|| crate::file::fsync(fd) as isize),
        Op::Invalid => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// glibc's `__aio_enqueue_request` and its worker, in the caller.
///
/// Refuses what glibc refuses before queueing -- a priority out of range --
/// with `false` and `errno` set, recording the refusal in the `aiocb` as
/// glibc does.  Otherwise performs the request, records its outcome,
/// notifies as `aio_sigevent` asks, and returns `true` with the caller's
/// `errno` as it was: the request's own failure is for `aio_error` to report.
///
/// # Safety
///
/// `cb` is non-null and points to an `Aiocb` that stays valid, and is not
/// used by another request, for the whole call.
unsafe fn submit(cb: *mut Aiocb, op: Op) -> bool {
    // SAFETY: the caller's contract.
    let cb = unsafe { &*cb };
    // glibc exempts the sync operations (and sets their priority to 0).
    if !matches!(op, Op::Sync | Op::DataSync) && !(0..=AIO_PRIO_DELTA_MAX).contains(&cb.aio_reqprio)
    {
        record(cb, -1, errno::EINVAL);
        errno::set_errno(errno::EINVAL);
        return false;
    }
    let saved = errno::get_errno();
    record(cb, 0, errno::EINPROGRESS);
    let ret = with_in_flight(cb.aio_fildes, || perform(cb, op));
    record(cb, ret, if ret == -1 { errno::get_errno() } else { 0 });
    // After the outcome is recorded, not before: a `SIGEV_THREAD` function
    // or a signal handler commonly calls `aio_return` on this very `aiocb`.
    // A notification that cannot be made becomes the request's outcome, as
    // in glibc's `__aio_notify`.
    let sev = SigeventView::read(cb.aio_sigevent.as_ptr());
    if notify(&sev).is_err() {
        record(cb, -1, errno::get_errno());
    }
    errno::set_errno(saved);
    completed();
    true
}

// ---------------------------------------------------------------------------
// Notification
// ---------------------------------------------------------------------------

/// The fields of a `struct sigevent` (musl's x86_64 layout, 64 bytes) that
/// notification reads.
#[repr(C)]
#[derive(Clone, Copy)]
struct SigeventView {
    /// `union sigval`: an `int` or a pointer, passed to the `SIGEV_THREAD`
    /// function in one register.
    sigev_value: usize,
    sigev_signo: i32,
    sigev_notify: i32,
    /// `SIGEV_THREAD`'s function (musl's `__sev_fields.__sev_thread`).
    sigev_notify_function: Option<extern "C" fn(usize)>,
    /// And its thread's attributes; NULL for a detached default thread.
    sigev_notify_attributes: *const crate::pthread::PthreadAttrT,
    __pad: [u8; 32],
}

const _: () = assert!(size_of::<SigeventView>() == 64);

impl SigeventView {
    /// `SIGEV_NONE`, for a `lio_listio` given no `sig`.
    const NONE: Self = Self {
        sigev_value: 0,
        sigev_signo: 0,
        sigev_notify: crate::time::SIGEV_NONE,
        sigev_notify_function: None,
        sigev_notify_attributes: core::ptr::null(),
        __pad: [0; 32],
    };

    /// Read a `struct sigevent` at `p`.
    fn read(p: *const u8) -> Self {
        // SAFETY: callers pass the 64 bytes of an `aiocb`'s `aio_sigevent`
        // or a caller's `struct sigevent`; every bit pattern is a valid
        // `SigeventView` (the function pointer is an `Option`, and a non-null
        // fn pointer is only invalid to *call*).
        unsafe { core::ptr::read_unaligned(p.cast::<Self>()) }
    }
}

/// What a notification thread calls.  glibc copies it to the heap because
/// the `sigevent` may be gone before the thread runs.
struct NotifyCall {
    function: extern "C" fn(usize),
    value: usize,
}

extern "C" fn notify_thread(arg: *mut u8) -> *mut u8 {
    // SAFETY: `arg` is the `NotifyCall` `notify` allocated for this thread
    // alone.
    let call = unsafe { core::ptr::read(arg.cast::<NotifyCall>()) };
    // SAFETY: allocated by `malloc` in `notify`, and read above.
    unsafe { crate::malloc::free(arg) };
    (call.function)(call.value);
    core::ptr::null_mut()
}

/// Notify as `sev` asks (glibc's `__aio_notify_only`); `Err` with `errno`
/// set if it could not be done.  `SIGEV_NONE`, `SIGEV_THREAD_ID` and
/// anything unknown notify nothing, as in glibc.
fn notify(sev: &SigeventView) -> Result<(), ()> {
    match sev.sigev_notify {
        crate::time::SIGEV_SIGNAL => {
            // glibc sends it with `rt_sigqueueinfo`, for which signal 0 only
            // probes.
            if sev.sigev_signo == 0 || crate::signal::raise(sev.sigev_signo) == 0 {
                Ok(())
            } else {
                Err(())
            }
        }
        crate::time::SIGEV_THREAD => {
            let Some(function) = sev.sigev_notify_function else {
                // glibc would start a thread that calls NULL, and fault.
                errno::set_errno(errno::EFAULT);
                return Err(());
            };
            let call = crate::malloc::malloc(size_of::<NotifyCall>());
            if call.is_null() {
                errno::set_errno(errno::ENOMEM);
                return Err(());
            }
            // SAFETY: `malloc` returned a block big and aligned enough.
            unsafe {
                core::ptr::write(
                    call.cast::<NotifyCall>(),
                    NotifyCall {
                        function,
                        value: sev.sigev_value,
                    },
                );
            }
            let mut detached: crate::pthread::PthreadAttrT = [0; 56];
            let attr = if sev.sigev_notify_attributes.is_null() {
                // Both succeed on a valid attribute object and a valid state.
                let _ = crate::pthread::pthread_attr_init(&raw mut detached);
                let _ = crate::pthread::pthread_attr_setdetachstate(
                    &raw mut detached,
                    crate::pthread::PTHREAD_CREATE_DETACHED,
                );
                (&raw const detached).cast()
            } else {
                sev.sigev_notify_attributes
            };
            let mut tid: crate::pthread::PthreadT = 0;
            let rc = crate::pthread::pthread_create(&raw mut tid, attr, notify_thread, call);
            if rc != 0 {
                // glibc tests this with `< 0`, which a positive error number
                // never is, and so never notices; the request is told.
                // SAFETY: the thread never started, so the block is ours.
                unsafe { crate::malloc::free(call) };
                errno::set_errno(rc);
                return Err(());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Public AIO functions
// ---------------------------------------------------------------------------

/// Start an asynchronous read -- performed before this returns.
///
/// Returns 0 once the request has been performed, whatever its outcome;
/// `aio_error`/`aio_return` report that.  Returns -1 with `errno` only for
/// what glibc refuses before queueing: a NULL `aiocbp` (`EFAULT`) or an
/// `aio_reqprio` outside `0..=AIO_PRIO_DELTA_MAX` (`EINVAL`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_read(aiocbp: *mut Aiocb) -> i32 {
    if aiocbp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-null; the caller vouches for the aiocb for the request's
    // life, which ends before this returns.
    if unsafe { submit(aiocbp, Op::Read) } {
        0
    } else {
        -1
    }
}

/// Start an asynchronous write -- performed before this returns.  As
/// [`aio_read`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_write(aiocbp: *mut Aiocb) -> i32 {
    if aiocbp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: as in `aio_read`.
    if unsafe { submit(aiocbp, Op::Write) } {
        0
    } else {
        -1
    }
}

/// The request's error status: `EINPROGRESS` while another thread is
/// performing it, otherwise 0 or the error it failed with.
///
/// glibc and musl read the field and nothing else, so an `aiocb` never
/// submitted reads as whatever it holds -- 0 in a zeroed one -- and the
/// outcome stays readable after `aio_return`.  Until 2026-09-26 an unknown
/// `aiocb` answered `EINVAL`, and so did one whose outcome had been evicted.
///
/// A NULL `aiocbp`, where glibc faults, is -1 with `errno` `EFAULT`: POSIX's
/// shape for `aio_error` itself failing.  (It returned `EINVAL` as a value,
/// which reads as "the request failed with EINVAL".)
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_error(aiocbp: *const Aiocb) -> i32 {
    if aiocbp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-null; the caller's contract.
    unsafe { (*aiocbp).error_code.load(Ordering::Acquire) }
}

/// The request's return value: what `read`, `write` or `fsync` returned for
/// it.  glibc reads the field and leaves `errno` alone; so does this.  (It
/// used to set `errno` to the request's error and forget the outcome, so a
/// following `aio_error` answered `EINVAL`.)
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_return(aiocbp: *mut Aiocb) -> isize {
    if aiocbp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-null; the caller's contract.
    unsafe { (*aiocbp).return_value.load(Ordering::Acquire) }
}

/// Cancel the requests on `fd`, or the one `aiocbp` names (glibc's
/// rt/aio_cancel.c, in its order).
///
/// 1. `fd` not open → `EBADF`.
/// 2. `aiocbp` for another descriptor → `EINVAL`.
/// 3. A request being performed cannot be cancelled: `AIO_NOTCANCELED`.
///    Nothing is ever queued, so every other request is `AIO_ALLDONE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_cancel(fd: i32, aiocbp: *mut Aiocb) -> i32 {
    if !fd_is_open(fd) {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if !aiocbp.is_null() {
        // SAFETY: non-null; the caller's contract.
        let cb = unsafe { &*aiocbp };
        if cb.aio_fildes != fd {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        return if cb.error_code.load(Ordering::Acquire) == errno::EINPROGRESS {
            AIO_NOTCANCELED
        } else {
            AIO_ALLDONE
        };
    }
    if fd_in_flight(fd) {
        AIO_NOTCANCELED
    } else {
        AIO_ALLDONE
    }
}

/// Wait until one of the listed requests has completed (glibc's
/// rt/aio_suspend.c).
///
/// `nent < 0` is `EINVAL`.  Then the list is read -- so a NULL list with
/// entries is `EFAULT`, where glibc faults, and with none is 0.  It returns
/// 0 at once when any listed request is not in progress, or when every
/// entry is NULL; otherwise it sleeps until one completes.  A `timeout` is
/// relative and measured on `CLOCK_MONOTONIC`; running out is `EAGAIN`, and
/// a malformed one is `EINVAL` when it comes to be used.
///
/// Until 2026-09-26 this refused `nent == 0`, which glibc accepts, and never
/// waited: a request another thread was performing was reported complete.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_suspend(
    list: *const *const Aiocb,
    nent: i32,
    timeout: *const crate::stat::Timespec,
) -> i32 {
    let Ok(n) = usize::try_from(nent) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if n > 0 && list.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: `completions()`/`suspended()` are this process's words.
    let (seq, sleepers) = unsafe { (&*completions(), &*suspended()) };
    sleepers.fetch_add(1, Ordering::SeqCst);
    let mut deadline: Option<crate::stat::Timespec> = None;
    let result = loop {
        let seen = seq.load(Ordering::SeqCst);
        // SAFETY: `list` holds `n` entries, by the caller's contract.
        if !unsafe { all_in_progress(list, n) } {
            break 0;
        }
        if timeout.is_null() {
            crate::lowlevellock::futex_wait(seq, seen);
            continue;
        }
        let at = match deadline {
            Some(at) => at,
            None => {
                // SAFETY: non-null; the caller's contract.
                let rel = unsafe { core::ptr::read(timeout) };
                if !crate::time::valid_nanoseconds(rel.tv_nsec) || rel.tv_sec < 0 {
                    break errno::EINVAL;
                }
                let at = add_timespec(
                    &crate::lowlevellock::now_on(crate::time::CLOCK_MONOTONIC),
                    &rel,
                );
                deadline = Some(at);
                at
            }
        };
        let now = crate::lowlevellock::now_on(crate::time::CLOCK_MONOTONIC);
        match crate::lowlevellock::ns_until(&now, &at) {
            None => break errno::EAGAIN,
            Some(ns) => crate::lowlevellock::futex_wait_timeout(seq, seen, ns),
        }
    };
    sleepers.fetch_sub(1, Ordering::SeqCst);
    if result == 0 {
        0
    } else {
        errno::set_errno(result);
        -1
    }
}

/// Would glibc's `aio_suspend` sleep on this list: is there a non-NULL
/// entry, and is every non-NULL entry in progress?
///
/// # Safety
///
/// `list` points to `n` readable entries (or `n` is 0).
unsafe fn all_in_progress(list: *const *const Aiocb, n: usize) -> bool {
    let mut any = false;
    for i in 0..n {
        // SAFETY: `i < n`, by the caller's contract.
        let cb = unsafe { *list.add(i) };
        if cb.is_null() {
            continue;
        }
        // SAFETY: a listed aiocb is valid, by the caller's contract.
        if unsafe { (*cb).error_code.load(Ordering::Acquire) } != errno::EINPROGRESS {
            return false;
        }
        any = true;
    }
    any
}

/// `now + rel`, normalised; saturating far in the future.
fn add_timespec(now: &crate::stat::Timespec, rel: &crate::stat::Timespec) -> crate::stat::Timespec {
    let mut sec = now.tv_sec.saturating_add(rel.tv_sec);
    let mut nsec = now.tv_nsec.saturating_add(rel.tv_nsec);
    if nsec >= 1_000_000_000 {
        nsec = nsec.wrapping_sub(1_000_000_000);
        sec = sec.saturating_add(1);
    }
    crate::stat::Timespec {
        tv_sec: sec,
        tv_nsec: nsec,
    }
}

/// Sync a file for an async I/O operation -- performed before this returns.
///
/// Validation order is glibc 2.39's (rt/aio_fsync.c):
///
/// 1. `op` not `O_SYNC` and not `O_DSYNC` → `EINVAL` — tested first, before
///    `aiocbp` is touched.
/// 2. `aiocbp` NULL → `EFAULT`.  glibc has no such test; it reads
///    `aiocbp->aio_fildes` next and faults, and `EFAULT` is this libc's
///    substitute for that fault (design-decisions.md §303), in its place.
/// 3. `aio_fildes` not open → `EBADF`: glibc asks
///    `fcntl(aio_fildes, F_GETFL)`, so any descriptor that is not open is
///    refused synchronously, not only a negative one.
///
/// Then `O_SYNC` performs `fsync` and `O_DSYNC` `fdatasync`, as glibc's
/// worker does (this ran `fsync` for both until 2026-09-26), and the outcome
/// is the request's.  A sync request's priority is not checked.
///
/// Until 2026-09-26 the order was the reverse, under a comment calling it
/// "Linux's libaio/glibc convention", and only a negative descriptor was
/// `EBADF`: a closed one was accepted, and its failure deferred to the
/// asynchronous status.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_fsync(op: i32, aiocbp: *mut Aiocb) -> i32 {
    if op != crate::fcntl::O_SYNC && op != crate::fcntl::O_DSYNC {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if aiocbp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: aiocbp is non-null by check above; caller's contract.
    let fd = unsafe { (*aiocbp).aio_fildes };
    if !fd_is_open(fd) {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    let kind = if op == crate::fcntl::O_SYNC {
        Op::Sync
    } else {
        Op::DataSync
    };
    // SAFETY: as in `aio_read`.
    if unsafe { submit(aiocbp, kind) } {
        0
    } else {
        -1
    }
}

/// Start a list of requests (glibc's rt/lio_listio-common.c, in its order).
///
/// 1. `mode` neither `LIO_WAIT` nor `LIO_NOWAIT` → `EINVAL`.
/// 2. `nent < 0` → `EINVAL` (musl's test; glibc sizes an array by it, which
///    is undefined).
/// 3. The list is read: a NULL list with entries is `EFAULT`, where glibc
///    faults; with none it is an empty list.
/// 4. Each non-NULL entry that is not `LIO_NOP` is submitted as
///    `aio_read`/`aio_write` would be.  An opcode that is none of those is
///    performed as a failure (`EINVAL` in its `aio_error`), not refused --
///    glibc's worker, not its queue, rejects it.  A priority out of range is
///    refused, as by `aio_read`.
/// 5. Nothing submitted: the result of the refusals -- 0, or -1 with the last
///    one's `errno` -- after notifying `sig` for `LIO_NOWAIT`.
/// 6. `LIO_WAIT`: -1 with `EIO` if any request failed or was refused, else 0.
/// 7. `LIO_NOWAIT`: every request has completed, so `sig` is notified now;
///    -1 with `EINVAL` if one was refused, else 0.
///
/// `sig` is the list's own notification, used only for `LIO_NOWAIT`; each
/// request is also notified as its own `aio_sigevent` asks.  Until
/// 2026-09-26 an unknown opcode was refused synchronously, a failure gave
/// the last errno where glibc gives `EIO`, and `sig` was ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lio_listio(
    mode: i32,
    list: *const *mut Aiocb,
    nent: i32,
    sig: *mut crate::time::Sigevent,
) -> i32 {
    if mode != LIO_WAIT && mode != LIO_NOWAIT {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let Ok(n) = usize::try_from(nent) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if n > 0 && list.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let mut refused = false;
    let mut submitted = false;
    let mut failed = false;
    for i in 0..n {
        // SAFETY: `list` holds `n` entries, by the caller's contract.
        let cb = unsafe { *list.add(i) };
        if cb.is_null() {
            continue;
        }
        // SAFETY: a listed aiocb is valid, by the caller's contract.
        let op = match unsafe { (*cb).aio_lio_opcode } {
            LIO_NOP => continue,
            LIO_READ => Op::Read,
            LIO_WRITE => Op::Write,
            _ => Op::Invalid,
        };
        // SAFETY: as above; each entry is its own request.
        if unsafe { submit(cb, op) } {
            submitted = true;
            // SAFETY: as above.
            if unsafe { (*cb).return_value.load(Ordering::Acquire) } == -1 {
                failed = true;
            }
        } else {
            refused = true;
        }
    }
    if mode == LIO_WAIT && submitted {
        if refused || failed {
            errno::set_errno(errno::EIO);
            return -1;
        }
        return 0;
    }
    if mode == LIO_NOWAIT {
        let sev = if sig.is_null() {
            SigeventView::NONE
        } else {
            SigeventView::read(sig.cast_const().cast::<u8>())
        };
        // glibc ignores whether the list's notification could be made; it
        // must not disturb the `errno` a refusal left, either.
        let saved = errno::get_errno();
        let _ = notify(&sev);
        errno::set_errno(saved);
    }
    if refused { -1 } else { 0 }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A zeroed `aiocb`, as a C caller's `memset` makes one.
    fn blank() -> Aiocb {
        // SAFETY: every field is an integer, an atomic integer, a raw pointer
        // or a byte array, for all of which zero is valid.
        unsafe { core::mem::zeroed() }
    }

    /// An eventfd: the one descriptor whose reads and writes the host build
    /// simulates, and one Linux cannot `pread`/`pwrite` -- so every request on
    /// it also exercises the `ESPIPE` fallback.
    fn eventfd() -> i32 {
        let fd = crate::epoll::eventfd(0, crate::epoll::EFD_NONBLOCK);
        assert!(fd >= 0, "host eventfd");
        fd
    }

    fn write_request(fd: i32, value: &mut u64) -> Aiocb {
        let mut cb = blank();
        cb.aio_fildes = fd;
        cb.aio_buf = core::ptr::from_mut(value).cast::<u8>();
        cb.aio_nbytes = 8;
        cb
    }

    // -- layout --

    #[test]
    fn test_aiocb_is_musls_layout() {
        assert_eq!(size_of::<Aiocb>(), 168);
        assert_eq!(core::mem::offset_of!(Aiocb, aio_reqprio), 8);
        assert_eq!(core::mem::offset_of!(Aiocb, aio_buf), 16);
        assert_eq!(core::mem::offset_of!(Aiocb, error_code), 112);
        assert_eq!(core::mem::offset_of!(Aiocb, return_value), 120);
        assert_eq!(core::mem::offset_of!(Aiocb, aio_offset), 128);
    }

    #[test]
    fn test_constants() {
        assert_eq!((LIO_WAIT, LIO_NOWAIT), (0, 1));
        assert_eq!((LIO_READ, LIO_WRITE, LIO_NOP), (0, 1, 2));
        assert_eq!((AIO_CANCELED, AIO_NOTCANCELED, AIO_ALLDONE), (0, 1, 2));
        assert_eq!(AIO_PRIO_DELTA_MAX, 20);
    }

    // -- aio_read / aio_write --

    #[test]
    fn test_aio_read_and_write_null_efault() {
        errno::set_errno(0);
        assert_eq!(aio_read(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        errno::set_errno(0);
        assert_eq!(aio_write(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// A write and a read that complete: the outcome is in the aiocb, and
    /// both went through the `ESPIPE` fallback to a plain read/write.
    #[test]
    fn test_write_then_read_complete() {
        let fd = eventfd();
        let mut five = 5u64;
        let mut w = write_request(fd, &mut five);
        assert_eq!(aio_write(&raw mut w), 0);
        assert_eq!(aio_error(&w), 0);
        assert_eq!(aio_return(&raw mut w), 8);

        let mut got = 0u64;
        let mut r = write_request(fd, &mut got);
        assert_eq!(aio_read(&raw mut r), 0);
        assert_eq!(aio_error(&r), 0);
        assert_eq!(aio_return(&raw mut r), 8);
        assert_eq!(got, 5);
        // The outcome stays readable after `aio_return`, as in glibc.
        assert_eq!(aio_error(&r), 0);
        assert_eq!(aio_return(&raw mut r), 8);
        crate::file::close(fd);
    }

    /// A descriptor that is not open is the request's outcome, not a refusal
    /// -- and the caller's errno is left alone.
    #[test]
    fn test_bad_descriptor_is_the_requests_outcome() {
        let mut cb = blank();
        cb.aio_fildes = 250;
        assert!(crate::fdtable::get_fd(250).is_none());
        errno::set_errno(1234);
        assert_eq!(aio_read(&raw mut cb), 0);
        assert_eq!(errno::get_errno(), 1234);
        assert_eq!(aio_error(&cb), errno::EBADF);
        assert_eq!(aio_return(&raw mut cb), -1);
        cb.aio_fildes = -1;
        assert_eq!(aio_write(&raw mut cb), 0);
        assert_eq!(aio_error(&cb), errno::EBADF);
    }

    /// A negative offset and a NULL buffer are the request's outcome too:
    /// `pread`'s EINVAL, `read`'s EFAULT (after the ESPIPE fallback).
    #[test]
    fn test_bad_offset_and_buffer_are_the_requests_outcome() {
        let fd = eventfd();
        let mut cb = blank();
        cb.aio_fildes = fd;
        cb.aio_nbytes = 8;
        cb.aio_offset = -1;
        let mut v = 0u64;
        cb.aio_buf = core::ptr::from_mut(&mut v).cast::<u8>();
        assert_eq!(aio_read(&raw mut cb), 0);
        assert_eq!(aio_error(&cb), errno::EINVAL);
        cb.aio_offset = 0;
        cb.aio_buf = core::ptr::null_mut();
        assert_eq!(aio_write(&raw mut cb), 0);
        assert_eq!(aio_error(&cb), errno::EFAULT);
        assert_eq!(aio_return(&raw mut cb), -1);
        crate::file::close(fd);
    }

    /// A priority out of range is refused before anything is performed, and
    /// recorded in the aiocb, as glibc records it.
    #[test]
    fn test_priority_out_of_range_is_refused() {
        for prio in [-1, AIO_PRIO_DELTA_MAX + 1, i32::MIN, i32::MAX] {
            let mut cb = blank();
            cb.aio_fildes = 250;
            cb.aio_reqprio = prio;
            errno::set_errno(0);
            assert_eq!(aio_read(&raw mut cb), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
            assert_eq!(aio_error(&cb), errno::EINVAL);
            assert_eq!(aio_return(&raw mut cb), -1);
        }
        let mut cb = blank();
        cb.aio_fildes = 250;
        cb.aio_reqprio = AIO_PRIO_DELTA_MAX;
        assert_eq!(aio_write(&raw mut cb), 0, "the largest priority is fine");
        assert_eq!(aio_error(&cb), errno::EBADF);
    }

    /// The old table held 16 outcomes and evicted the oldest.
    #[test]
    fn test_no_outcome_is_evicted() {
        let fd = eventfd();
        let mut values = [1u64; 40];
        let mut cbs: Vec<Aiocb> = values.iter_mut().map(|v| write_request(fd, v)).collect();
        for cb in &mut cbs {
            assert_eq!(aio_write(core::ptr::from_mut(cb)), 0);
        }
        for cb in &mut cbs {
            assert_eq!(aio_error(cb), 0);
            assert_eq!(aio_return(core::ptr::from_mut(cb)), 8);
        }
        crate::file::close(fd);
    }

    // -- aio_error / aio_return --

    /// A NULL aiocb is aio_error *failing* (-1, errno EFAULT), not a request
    /// that failed with an error.
    #[test]
    fn test_aio_error_and_return_null_efault() {
        errno::set_errno(0);
        assert_eq!(aio_error(core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        errno::set_errno(0);
        assert_eq!(aio_return(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// Never submitted: the fields as the caller left them, as in glibc.
    #[test]
    fn test_unsubmitted_aiocb_reads_its_fields() {
        let mut cb = blank();
        assert_eq!(aio_error(&cb), 0);
        assert_eq!(aio_return(&raw mut cb), 0);
    }

    #[test]
    fn test_aio_return_leaves_errno_alone() {
        let mut cb = blank();
        cb.aio_fildes = 250;
        assert_eq!(aio_read(&raw mut cb), 0);
        errno::set_errno(7);
        assert_eq!(aio_return(&raw mut cb), -1);
        assert_eq!(errno::get_errno(), 7);
    }

    // -- aio_fsync --

    #[test]
    fn test_aio_fsync_validation_order() {
        errno::set_errno(0);
        assert_eq!(aio_fsync(i32::MIN, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL, "op before the pointer");
        errno::set_errno(0);
        assert_eq!(aio_fsync(crate::fcntl::O_SYNC, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        let mut cb = blank();
        cb.aio_fildes = -42;
        errno::set_errno(0);
        assert_eq!(aio_fsync(99, &raw mut cb), -1);
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "op before the descriptor"
        );
        for fd in [-1, 250] {
            cb.aio_fildes = fd;
            errno::set_errno(0);
            assert_eq!(aio_fsync(crate::fcntl::O_DSYNC, &raw mut cb), -1);
            assert_eq!(errno::get_errno(), errno::EBADF);
        }
        for op in [0, -1, 0xDEAD, i32::MAX] {
            cb.aio_fildes = 0;
            errno::set_errno(0);
            assert_eq!(aio_fsync(op, &raw mut cb), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }
    }

    /// Both ops run and complete; a refused call records nothing, so the
    /// aiocb still reads as the caller left it.
    #[test]
    fn test_aio_fsync_completes() {
        let mut cb = blank();
        cb.aio_fildes = 0;
        assert_eq!(aio_fsync(0xDEAD, &raw mut cb), -1);
        assert_eq!(aio_error(&cb), 0);
        cb.return_value.store(99, Ordering::Relaxed);
        assert_eq!(aio_fsync(crate::fcntl::O_SYNC, &raw mut cb), 0);
        assert_eq!(aio_error(&cb), 0);
        assert_eq!(aio_return(&raw mut cb), 0);
        assert_eq!(aio_fsync(crate::fcntl::O_DSYNC, &raw mut cb), 0);
        assert_eq!(aio_return(&raw mut cb), 0);
        assert_ne!(crate::fcntl::O_SYNC, crate::fcntl::O_DSYNC);
    }

    /// A sync request's priority is not checked (glibc zeroes it).
    #[test]
    fn test_aio_fsync_ignores_priority() {
        let mut cb = blank();
        cb.aio_fildes = 0;
        cb.aio_reqprio = -5;
        assert_eq!(aio_fsync(crate::fcntl::O_SYNC, &raw mut cb), 0);
        assert_eq!(aio_error(&cb), 0);
    }

    // -- aio_cancel --

    #[test]
    fn test_aio_cancel() {
        errno::set_errno(0);
        assert_eq!(aio_cancel(-1, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(aio_cancel(250, core::ptr::null_mut()), -1);

        let fd = eventfd();
        assert_eq!(aio_cancel(fd, core::ptr::null_mut()), AIO_ALLDONE);
        let mut cb = blank();
        cb.aio_fildes = fd;
        assert_eq!(aio_cancel(fd, &raw mut cb), AIO_ALLDONE);
        cb.aio_fildes = fd + 1;
        errno::set_errno(0);
        assert_eq!(aio_cancel(fd, &raw mut cb), -1);
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "another descriptor's aiocb"
        );
        cb.aio_fildes = fd;
        cb.error_code.store(errno::EINPROGRESS, Ordering::Relaxed);
        assert_eq!(aio_cancel(fd, &raw mut cb), AIO_NOTCANCELED);
        crate::file::close(fd);
    }

    /// While a request on the descriptor is being performed, cancelling the
    /// descriptor's requests answers NOTCANCELED.
    #[test]
    fn test_aio_cancel_sees_a_request_in_flight() {
        let fd = eventfd();
        let other = eventfd();
        let seen = with_in_flight(fd, || {
            (
                aio_cancel(fd, core::ptr::null_mut()),
                aio_cancel(other, core::ptr::null_mut()),
            )
        });
        assert_eq!(seen, (AIO_NOTCANCELED, AIO_ALLDONE));
        assert_eq!(aio_cancel(fd, core::ptr::null_mut()), AIO_ALLDONE);
        crate::file::close(fd);
        crate::file::close(other);
    }

    // -- aio_suspend --

    #[test]
    fn test_aio_suspend_argument_order() {
        errno::set_errno(0);
        assert_eq!(aio_suspend(core::ptr::null(), -1, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL, "nent before the list");
        errno::set_errno(0);
        assert_eq!(aio_suspend(core::ptr::null(), 5, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        // Nothing to read, so a NULL list is fine -- glibc returns 0.
        assert_eq!(aio_suspend(core::ptr::null(), 0, core::ptr::null()), 0);
    }

    #[test]
    fn test_aio_suspend_returns_at_once_for_completed_or_null_entries() {
        let done = blank();
        let list = [core::ptr::null(), &raw const done];
        assert_eq!(aio_suspend(list.as_ptr(), 2, core::ptr::null()), 0);
        let nulls: [*const Aiocb; 2] = [core::ptr::null(); 2];
        assert_eq!(aio_suspend(nulls.as_ptr(), 2, core::ptr::null()), 0);
    }

    /// Only in-progress requests are waited for; running out of time is
    /// EAGAIN, and a malformed timeout is EINVAL.
    #[test]
    fn test_aio_suspend_times_out_on_a_request_in_progress() {
        let busy = blank();
        busy.error_code.store(errno::EINPROGRESS, Ordering::Relaxed);
        let list = [&raw const busy];
        let ten_ms = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 10_000_000,
        };
        errno::set_errno(0);
        assert_eq!(aio_suspend(list.as_ptr(), 1, &raw const ten_ms), -1);
        assert_eq!(errno::get_errno(), errno::EAGAIN);
        let bad = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 2_000_000_000,
        };
        errno::set_errno(0);
        assert_eq!(aio_suspend(list.as_ptr(), 1, &raw const bad), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // One entry done is enough.
        let done = blank();
        let mixed = [&raw const busy, &raw const done];
        assert_eq!(aio_suspend(mixed.as_ptr(), 2, &raw const ten_ms), 0);
    }

    /// A request another thread completes wakes the suspended one.
    #[test]
    fn test_aio_suspend_wakes_when_a_request_completes() {
        struct Shared(*const Aiocb);
        // SAFETY: the aiocb outlives the thread, which is joined below.
        unsafe impl Send for Shared {}
        impl Shared {
            fn get(&self) -> &Aiocb {
                // SAFETY: as above.
                unsafe { &*self.0 }
            }
        }
        let busy = blank();
        busy.error_code.store(errno::EINPROGRESS, Ordering::Relaxed);
        let shared = Shared(&raw const busy);
        let finisher = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            record(shared.get(), 8, 0);
            completed();
        });
        let list = [&raw const busy];
        let five_s = crate::stat::Timespec {
            tv_sec: 5,
            tv_nsec: 0,
        };
        assert_eq!(aio_suspend(list.as_ptr(), 1, &raw const five_s), 0);
        assert_eq!(busy.error_code.load(Ordering::Acquire), 0);
        finisher.join().expect("finisher");
    }

    #[test]
    fn test_add_timespec_carries() {
        let now = crate::stat::Timespec {
            tv_sec: 10,
            tv_nsec: 900_000_000,
        };
        let rel = crate::stat::Timespec {
            tv_sec: 1,
            tv_nsec: 200_000_000,
        };
        let at = add_timespec(&now, &rel);
        assert_eq!((at.tv_sec, at.tv_nsec), (12, 100_000_000));
    }

    // -- lio_listio --

    #[test]
    fn test_lio_listio_argument_order() {
        errno::set_errno(0);
        assert_eq!(
            lio_listio(42, core::ptr::null(), -1, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL, "mode first");
        errno::set_errno(0);
        assert_eq!(
            lio_listio(LIO_WAIT, core::ptr::null(), -1, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL, "nent before the list");
        errno::set_errno(0);
        assert_eq!(
            lio_listio(LIO_WAIT, core::ptr::null(), 1, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        for mode in [LIO_WAIT, LIO_NOWAIT] {
            assert_eq!(
                lio_listio(mode, core::ptr::null(), 0, core::ptr::null_mut()),
                0
            );
        }
    }

    #[test]
    fn test_lio_listio_skips_null_and_nop() {
        let mut nop = blank();
        nop.aio_lio_opcode = LIO_NOP;
        nop.aio_fildes = 250;
        let list = [core::ptr::null_mut(), &raw mut nop];
        for mode in [LIO_WAIT, LIO_NOWAIT] {
            assert_eq!(lio_listio(mode, list.as_ptr(), 2, core::ptr::null_mut()), 0);
        }
        assert_eq!(aio_error(&nop), 0, "a NOP is not performed");
    }

    #[test]
    fn test_lio_listio_performs_the_list() {
        let fd = eventfd();
        let (mut a, mut b) = (2u64, 3u64);
        let mut wa = write_request(fd, &mut a);
        wa.aio_lio_opcode = LIO_WRITE;
        let mut wb = write_request(fd, &mut b);
        wb.aio_lio_opcode = LIO_WRITE;
        let list = [&raw mut wa, &raw mut wb];
        assert_eq!(
            lio_listio(LIO_WAIT, list.as_ptr(), 2, core::ptr::null_mut()),
            0
        );
        assert_eq!((aio_error(&wa), aio_error(&wb)), (0, 0));
        let mut sum = 0u64;
        let mut r = write_request(fd, &mut sum);
        r.aio_lio_opcode = LIO_READ;
        let list = [&raw mut r];
        assert_eq!(
            lio_listio(LIO_NOWAIT, list.as_ptr(), 1, core::ptr::null_mut()),
            0
        );
        assert_eq!(aio_return(&raw mut r), 8);
        assert_eq!(sum, 5);
        crate::file::close(fd);
    }

    /// An unknown opcode is performed as a failure, not refused: LIO_NOWAIT
    /// succeeds, LIO_WAIT reports EIO, and the entry's own status is EINVAL.
    #[test]
    fn test_lio_listio_unknown_opcode_fails_the_request() {
        let mut cb = blank();
        cb.aio_fildes = 0;
        cb.aio_lio_opcode = 7;
        let list = [&raw mut cb];
        assert_eq!(
            lio_listio(LIO_NOWAIT, list.as_ptr(), 1, core::ptr::null_mut()),
            0
        );
        assert_eq!(aio_error(&cb), errno::EINVAL);
        assert_eq!(aio_return(&raw mut cb), -1);
        errno::set_errno(0);
        assert_eq!(
            lio_listio(LIO_WAIT, list.as_ptr(), 1, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EIO);
    }

    /// A refusal alone is -1 with its EINVAL in either mode; beside a
    /// request that ran, LIO_WAIT turns it into EIO.
    #[test]
    fn test_lio_listio_refusals() {
        let mut bad = blank();
        bad.aio_fildes = 0;
        bad.aio_reqprio = -1;
        let list = [&raw mut bad];
        for mode in [LIO_WAIT, LIO_NOWAIT] {
            errno::set_errno(0);
            assert_eq!(
                lio_listio(mode, list.as_ptr(), 1, core::ptr::null_mut()),
                -1
            );
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }
        let fd = eventfd();
        let mut one = 1u64;
        let mut good = write_request(fd, &mut one);
        good.aio_lio_opcode = LIO_WRITE;
        let list = [&raw mut bad, &raw mut good];
        errno::set_errno(0);
        assert_eq!(
            lio_listio(LIO_WAIT, list.as_ptr(), 2, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EIO);
        assert_eq!(aio_error(&good), 0, "the good request still ran");
        errno::set_errno(0);
        assert_eq!(
            lio_listio(LIO_NOWAIT, list.as_ptr(), 2, core::ptr::null_mut()),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    // -- notification --

    fn with_sigevent(
        cb: &mut Aiocb,
        notify: i32,
        signo: i32,
        function: Option<extern "C" fn(usize)>,
    ) {
        let sev = SigeventView {
            sigev_notify: notify,
            sigev_signo: signo,
            sigev_notify_function: function,
            ..SigeventView::NONE
        };
        // SAFETY: `SigeventView` is 64 bytes, as `aio_sigevent` is.
        unsafe {
            core::ptr::write_unaligned(cb.aio_sigevent.as_mut_ptr().cast::<SigeventView>(), sev);
        }
    }

    extern "C" fn never_called(_: usize) {}

    /// Signal 0 is a probe; an invalid signal cannot be raised, and that
    /// failure becomes the request's outcome, as in glibc's `__aio_notify`.
    #[test]
    fn test_sigev_signal() {
        let mut cb = blank();
        cb.aio_fildes = 0;
        with_sigevent(&mut cb, crate::time::SIGEV_SIGNAL, 0, None);
        assert_eq!(aio_fsync(crate::fcntl::O_SYNC, &raw mut cb), 0);
        assert_eq!(aio_error(&cb), 0);
        with_sigevent(&mut cb, crate::time::SIGEV_SIGNAL, 100_000, None);
        errno::set_errno(3);
        assert_eq!(aio_fsync(crate::fcntl::O_SYNC, &raw mut cb), 0);
        assert_eq!(errno::get_errno(), 3, "the caller's errno is kept");
        assert_eq!(aio_error(&cb), errno::EINVAL);
        assert_eq!(aio_return(&raw mut cb), -1);
    }

    /// SIGEV_THREAD with no function would call NULL in glibc; here it is
    /// the request's EFAULT.  With a function, the host cannot create the
    /// thread, and says so through the request.
    #[test]
    fn test_sigev_thread() {
        let mut cb = blank();
        cb.aio_fildes = 0;
        with_sigevent(&mut cb, crate::time::SIGEV_THREAD, 0, None);
        assert_eq!(aio_fsync(crate::fcntl::O_SYNC, &raw mut cb), 0);
        assert_eq!(aio_error(&cb), errno::EFAULT);
        with_sigevent(&mut cb, crate::time::SIGEV_THREAD, 0, Some(never_called));
        assert_eq!(aio_fsync(crate::fcntl::O_SYNC, &raw mut cb), 0);
        assert_eq!(aio_error(&cb), errno::EAGAIN);
    }

    #[test]
    fn test_sigev_none_and_thread_id_notify_nothing() {
        for how in [crate::time::SIGEV_NONE, crate::time::SIGEV_THREAD_ID, 77] {
            let mut cb = blank();
            cb.aio_fildes = 0;
            with_sigevent(&mut cb, how, 100_000, None);
            assert_eq!(aio_fsync(crate::fcntl::O_SYNC, &raw mut cb), 0);
            assert_eq!(aio_error(&cb), 0);
        }
    }

    /// `lio_listio`'s own `sig` never changes its answer or its errno.
    #[test]
    fn test_lio_listio_sig_does_not_disturb_the_result() {
        let mut bad = blank();
        bad.aio_fildes = 0;
        bad.aio_reqprio = -1;
        let list = [&raw mut bad];
        let mut sev = SigeventView {
            sigev_notify: crate::time::SIGEV_SIGNAL,
            sigev_signo: 100_000,
            ..SigeventView::NONE
        };
        errno::set_errno(0);
        let sig = (&raw mut sev).cast::<crate::time::Sigevent>();
        assert_eq!(lio_listio(LIO_NOWAIT, list.as_ptr(), 1, sig), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }
}

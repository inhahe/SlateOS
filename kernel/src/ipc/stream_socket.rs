//! Stream socket — bidirectional kernel-buffered byte stream IPC.
//!
//! A stream socket pair is two endpoints bonded into a single kernel
//! object, each able to both send and receive a raw byte stream.  It is
//! the kernel primitive backing the POSIX `socketpair(AF_UNIX,
//! SOCK_STREAM, ...)` call.
//!
//! ## Why a dedicated object (not two pipes)
//!
//! `socketpair()` needs three properties that a userspace pair-of-pipes
//! cannot provide:
//!
//! 1. **Byte-stream, bidirectional semantics** — each endpoint both
//!    reads and writes, unlike a one-way pipe.
//! 2. **Non-consuming readiness** — `poll`/`select` must report
//!    readability without consuming data (channels lack this; pipes and
//!    this object expose [`poll_status`]).
//! 3. **A single inheritable handle per endpoint** — `posix_spawn`/fork
//!    inheritance carries exactly one `u64` per file descriptor.  A
//!    two-pipe scheme would need two handles per endpoint and could not
//!    be inherited as one fd.  Each endpoint here is one `u64`.
//!
//! ## Design
//!
//! - A [`Pair`] owns two ring buffers.  `ring[e]` carries bytes written
//!   by endpoint `e` and read by the peer endpoint `e ^ 1`.
//! - Each endpoint has an independent reference count, close flag, and
//!   half-close flags (`shutdown(SHUT_RD)` / `shutdown(SHUT_WR)`), plus
//!   at most one blocked reader and one blocked writer.
//! - **Blocking semantics** (default): a sender blocks when its outgoing
//!   ring is full; a receiver blocks when its incoming ring is empty.
//!   Non-blocking variants return `WouldBlock`; timed variants return
//!   `TimedOut`.
//! - **Close / shutdown detection**: when the peer's write side is gone
//!   (closed or `SHUT_WR`), reads drain remaining bytes then return 0
//!   (EOF).  When the peer's read side is gone (closed or `SHUT_RD`),
//!   writes fail with `ChannelClosed` (broken pipe / `EPIPE`).
//!
//! ## Lock Ordering
//!
//! `PAIRS` → `SCHED` (send/recv may call `sched::wake()`), identical to
//! the pipe subsystem.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default per-direction buffer capacity in bytes.
///
/// 64 KiB matches the pipe default (and Linux's default socket buffer
/// order of magnitude).  With 16 KiB pages this is exactly 4 pages per
/// direction, 8 per pair.
const DEFAULT_BUFFER_CAPACITY: usize = 64 * 1024;

/// `shutdown(how)` — disable further receives on this endpoint.
pub const SHUT_RD: u32 = 0;
/// `shutdown(how)` — disable further sends on this endpoint.
pub const SHUT_WR: u32 = 1;
/// `shutdown(how)` — disable both directions on this endpoint.
pub const SHUT_RDWR: u32 = 2;

// ---------------------------------------------------------------------------
// Pair ID and Handle
// ---------------------------------------------------------------------------

/// Unique identifier for a stream socket pair.
type PairId = u64;

/// Counter for generating unique pair IDs.
static NEXT_PAIR_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_pair_id() -> PairId {
    NEXT_PAIR_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to one endpoint of a stream socket pair.
///
/// Encodes the pair ID and endpoint index (0 or 1) in a single `u64`.
/// Bit 0 = endpoint index, bits 1–63 = pair ID.  Each endpoint is a
/// single inheritable handle.
///
/// Stream-socket handles occupy a different namespace from pipe and
/// channel handles; the syscall layer distinguishes them by which
/// syscall is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamSocketHandle(u64);

impl StreamSocketHandle {
    /// Create a handle for a given pair and endpoint index (0 or 1).
    #[allow(clippy::arithmetic_side_effects)]
    fn new(pair_id: PairId, endpoint: usize) -> Self {
        Self((pair_id << 1) | (endpoint as u64 & 1))
    }

    /// Reconstruct a handle from its raw `u64` representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw `u64` representation of this handle.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Extract the pair ID.
    #[allow(clippy::arithmetic_side_effects)]
    fn pair_id(self) -> PairId {
        self.0 >> 1
    }

    /// Extract which endpoint (0 or 1) this handle refers to.
    fn endpoint(self) -> usize {
        (self.0 & 1) as usize
    }
}

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

/// A single-direction ring buffer (identical mechanics to the pipe ring).
struct Ring {
    buf: Vec<u8>,
    head: usize,
    len: usize,
}

impl Ring {
    fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            head: 0,
            len: 0,
        }
    }

    /// How many bytes can be written without blocking.
    #[allow(clippy::arithmetic_side_effects)]
    fn writable(&self) -> usize {
        self.buf.len() - self.len
    }

    /// Write bytes into the ring.  Returns bytes written (may be partial).
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn write_bytes(&mut self, data: &[u8]) -> usize {
        let avail = self.writable();
        let to_write = data.len().min(avail);
        if to_write == 0 {
            return 0;
        }

        let cap = self.buf.len();
        let write_pos = (self.head + self.len) % cap;

        let first = to_write.min(cap - write_pos);
        self.buf[write_pos..write_pos + first].copy_from_slice(&data[..first]);

        let second = to_write - first;
        if second > 0 {
            self.buf[..second].copy_from_slice(&data[first..first + second]);
        }

        self.len += to_write;
        to_write
    }

    /// Read bytes from the ring.  Returns bytes read (may be partial).
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn read_bytes(&mut self, out: &mut [u8]) -> usize {
        let to_read = out.len().min(self.len);
        if to_read == 0 {
            return 0;
        }

        let cap = self.buf.len();

        let first = to_read.min(cap - self.head);
        out[..first].copy_from_slice(&self.buf[self.head..self.head + first]);

        let second = to_read - first;
        if second > 0 {
            out[first..first + second].copy_from_slice(&self.buf[..second]);
        }

        self.head = (self.head + to_read) % cap;
        self.len -= to_read;
        to_read
    }
}

// ---------------------------------------------------------------------------
// Endpoint state
// ---------------------------------------------------------------------------

/// Per-endpoint bookkeeping.
struct Endpoint {
    /// Reference count of handles referring to this endpoint.  Each
    /// `create()` starts at 1; `dup()` increments; `close()` decrements.
    /// Hitting 0 marks the endpoint logically closed.
    refcount: u32,
    /// Whether this endpoint has been fully closed (refcount → 0).
    closed: bool,
    /// Whether the read side has been shut down (`shutdown(SHUT_RD)`):
    /// further receives on this endpoint return EOF.
    rd_shut: bool,
    /// Whether the write side has been shut down (`shutdown(SHUT_WR)`):
    /// further sends on this endpoint fail, and the peer reading our ring
    /// sees EOF once drained.
    wr_shut: bool,
    /// Task blocked receiving (waiting for incoming data).
    reader_waiter: Option<TaskId>,
    /// Task blocked sending (waiting for outgoing buffer space).
    writer_waiter: Option<TaskId>,
}

impl Endpoint {
    const fn new() -> Self {
        Self {
            refcount: 1,
            closed: false,
            rd_shut: false,
            wr_shut: false,
            reader_waiter: None,
            writer_waiter: None,
        }
    }
}

/// A stream socket pair: two rings and two endpoints.
struct Pair {
    /// `ring[e]` holds bytes written by endpoint `e`, read by endpoint
    /// `e ^ 1`.
    ring: [Ring; 2],
    ep: [Endpoint; 2],
}

impl Pair {
    fn new(capacity: usize) -> Self {
        Self {
            ring: [Ring::new(capacity), Ring::new(capacity)],
            ep: [Endpoint::new(), Endpoint::new()],
        }
    }

    /// True if a send by endpoint `e` can never be received (broken
    /// pipe): our write side is shut, or the peer is closed / has shut
    /// its read side.
    #[allow(clippy::indexing_slicing)]
    fn send_broken(&self, e: usize) -> bool {
        let peer = e ^ 1;
        self.ep[e].wr_shut || self.ep[peer].closed || self.ep[peer].rd_shut
    }

    /// True if a receive by endpoint `e` is at EOF: our read side is
    /// shut, or the peer is closed / has shut its write side.
    #[allow(clippy::indexing_slicing)]
    fn recv_eof(&self, e: usize) -> bool {
        let peer = e ^ 1;
        self.ep[e].rd_shut || self.ep[peer].closed || self.ep[peer].wr_shut
    }
}

// ---------------------------------------------------------------------------
// Global pair table
// ---------------------------------------------------------------------------

/// Global table of all live stream socket pairs.
///
/// Lock ordering: `PAIRS` → `SCHED`.
static PAIRS: Mutex<BTreeMap<PairId, Pair>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Signal-interruptible blocking helpers
// ---------------------------------------------------------------------------
//
// A Unix stream socket is a *slow* object with no inherent timeout, so a
// blocking send/recv that has not yet transferred a byte must be
// interruptible by a deliverable signal (SA_RESTART): the syscall layer
// turns the resulting `Interrupted` into the ERESTARTSYS sentinel.  These
// helpers mirror the pipe subsystem exactly (see `ipc::pipe`).

/// The owning user process id of the current task, or `0` for a kernel
/// task (boot self-tests).  Kernel tasks have no signal state and park
/// uninterruptibly, exactly as before.
fn current_user_pid() -> u64 {
    crate::proc::thread::owner_process(sched::current_task_id()).unwrap_or(0)
}

/// `true` if a deliverable (unblocked) signal is pending for `pid`.
/// Always `false` for `pid == 0` (kernel task — no signal context).
fn deliverable_signal_pending(pid: u64) -> bool {
    pid != 0
        && crate::proc::signal::has_pending_in_mask(pid, !crate::proc::signal::blocked(pid))
}

/// Park the current task for a stream-socket wait, interruptibly for user
/// processes.  Registers a signal-waiter with the register-then-recheck
/// idiom (so `set_pending` wakes the park when a deliverable signal
/// arrives), blocks, then deregisters.  Kernel tasks (`pid == 0`) park
/// uninterruptibly.  The caller's surrounding loop must, after this
/// returns, re-acquire `PAIRS` and re-evaluate both the ring state and
/// [`deliverable_signal_pending`] — a signal wake is reported by the
/// latter, not by this function.
fn park_for_socket(pid: u64, task: u64) {
    if pid == 0 {
        sched::block_current();
        return;
    }
    let deliverable = !crate::proc::signal::blocked(pid);
    crate::proc::signal::register_signalfd_waiter(pid, task, deliverable);
    if crate::proc::signal::has_pending_in_mask(pid, deliverable) {
        // A signal arrived between enqueue and registration — don't block;
        // the caller's loop will observe it and return Interrupted.
        crate::proc::signal::deregister_signalfd_waiter(pid, task);
        return;
    }
    sched::block_current();
    crate::proc::signal::deregister_signalfd_waiter(pid, task);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new stream socket pair, returning two endpoint handles.
///
/// Either handle can be used with [`send`]/[`recv`] (and their
/// non-blocking and timed variants).  Bytes sent on one endpoint are
/// received on the other.
pub fn create() -> (StreamSocketHandle, StreamSocketHandle) {
    let id = alloc_pair_id();
    let pair = Pair::new(DEFAULT_BUFFER_CAPACITY);

    PAIRS.lock().insert(id, pair);

    super::stats::stream_socket_created();
    (
        StreamSocketHandle::new(id, 0),
        StreamSocketHandle::new(id, 1),
    )
}

/// Send bytes on an endpoint (blocking).
///
/// Writes as many bytes as possible into the endpoint's outgoing ring.
/// If the ring is full, blocks until the peer drains some data.
///
/// # Returns
///
/// - `Ok(n)` — sent `n` bytes (always > 0 on success).
/// - `Err(ChannelClosed)` — peer's read side is gone (broken pipe).
/// - `Err(InvalidArgument)` — `data` is empty.
/// - `Err(InvalidHandle)` — handle does not refer to a live pair.
#[allow(clippy::indexing_slicing)]
pub fn send(handle: StreamSocketHandle, data: &[u8]) -> KernelResult<usize> {
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let e = handle.endpoint();
    let peer = e ^ 1;
    let pid = current_user_pid();
    let task = sched::current_task_id();

    loop {
        {
            let mut table = PAIRS.lock();
            let pair = table
                .get_mut(&handle.pair_id())
                .ok_or(KernelError::InvalidHandle)?;

            if pair.send_broken(e) {
                return Err(KernelError::ChannelClosed);
            }

            let written = pair.ring[e].write_bytes(data);
            if written > 0 {
                let reader_id = pair.ep[peer].reader_waiter.take();
                drop(table);
                if let Some(task_id) = reader_id {
                    sched::wake(task_id);
                }
                super::stats::stream_socket_write(written as u64);
                return Ok(written);
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot left by a prior signal wake.
            if deliverable_signal_pending(pid) {
                if pair.ep[e].writer_waiter == Some(task) {
                    pair.ep[e].writer_waiter = None;
                }
                return Err(KernelError::Interrupted);
            }

            pair.ep[e].writer_waiter = Some(task);
        }

        super::stats::stream_socket_write_block();
        park_for_socket(pid, task);
    }
}

/// Send bytes on an endpoint (non-blocking).
///
/// # Returns
///
/// - `Ok(n)` — sent `n` bytes.
/// - `Err(WouldBlock)` — outgoing ring is full.
/// - `Err(ChannelClosed)` — peer's read side is gone.
/// - `Err(InvalidArgument)` / `Err(InvalidHandle)`.
#[allow(clippy::indexing_slicing)]
pub fn try_send(handle: StreamSocketHandle, data: &[u8]) -> KernelResult<usize> {
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let e = handle.endpoint();
    let peer = e ^ 1;

    let wake_reader;
    let written;
    {
        let mut table = PAIRS.lock();
        let pair = table
            .get_mut(&handle.pair_id())
            .ok_or(KernelError::InvalidHandle)?;

        if pair.send_broken(e) {
            return Err(KernelError::ChannelClosed);
        }

        written = pair.ring[e].write_bytes(data);
        if written == 0 {
            return Err(KernelError::WouldBlock);
        }
        wake_reader = pair.ep[peer].reader_waiter.take();
    }

    if let Some(task_id) = wake_reader {
        sched::wake(task_id);
    }
    super::stats::stream_socket_write(written as u64);
    Ok(written)
}

/// Receive bytes on an endpoint (blocking).
///
/// Reads up to `buf.len()` bytes from the endpoint's incoming ring.  If
/// empty, blocks until data arrives or the peer's write side closes.
///
/// # Returns
///
/// - `Ok(n)` where `n > 0` — received `n` bytes.
/// - `Ok(0)` — EOF (peer write side gone, or local read side shut).
/// - `Err(InvalidArgument)` — `buf` is empty.
/// - `Err(InvalidHandle)`.
#[allow(clippy::indexing_slicing)]
pub fn recv(handle: StreamSocketHandle, buf: &mut [u8]) -> KernelResult<usize> {
    if buf.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let e = handle.endpoint();
    let peer = e ^ 1;
    let pid = current_user_pid();
    let task = sched::current_task_id();

    loop {
        {
            let mut table = PAIRS.lock();
            let pair = table
                .get_mut(&handle.pair_id())
                .ok_or(KernelError::InvalidHandle)?;

            let n = pair.ring[peer].read_bytes(buf);
            if n > 0 {
                let writer_id = pair.ep[peer].writer_waiter.take();
                drop(table);
                if let Some(task_id) = writer_id {
                    sched::wake(task_id);
                }
                super::stats::stream_socket_read(n as u64);
                return Ok(n);
            }

            if pair.recv_eof(e) {
                return Ok(0);
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot left by a prior signal wake.
            if deliverable_signal_pending(pid) {
                if pair.ep[e].reader_waiter == Some(task) {
                    pair.ep[e].reader_waiter = None;
                }
                return Err(KernelError::Interrupted);
            }

            pair.ep[e].reader_waiter = Some(task);
        }

        super::stats::stream_socket_read_block();
        park_for_socket(pid, task);
    }
}

/// Receive bytes on an endpoint (non-blocking).
///
/// # Returns
///
/// - `Ok(n)` where `n > 0` — received `n` bytes.
/// - `Ok(0)` — EOF.
/// - `Err(WouldBlock)` — ring is empty but the peer is still writable.
/// - `Err(InvalidArgument)` / `Err(InvalidHandle)`.
#[allow(clippy::indexing_slicing)]
pub fn try_recv(handle: StreamSocketHandle, buf: &mut [u8]) -> KernelResult<usize> {
    if buf.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let e = handle.endpoint();
    let peer = e ^ 1;

    let mut wake_writer = None;
    let result;
    {
        let mut table = PAIRS.lock();
        let pair = table
            .get_mut(&handle.pair_id())
            .ok_or(KernelError::InvalidHandle)?;

        let n = pair.ring[peer].read_bytes(buf);
        if n > 0 {
            wake_writer = pair.ep[peer].writer_waiter.take();
            result = Ok(n);
        } else if pair.recv_eof(e) {
            result = Ok(0);
        } else {
            result = Err(KernelError::WouldBlock);
        }
    }

    if let Some(task_id) = wake_writer {
        sched::wake(task_id);
    }
    if let Ok(n) = result {
        if n > 0 {
            super::stats::stream_socket_read(n as u64);
        }
    }
    result
}

/// Receive bytes with a timeout (nanoseconds).
///
/// Blocks up to `timeout_ns` waiting for data.  `timeout_ns == 0` is an
/// immediate check returning `Err(TimedOut)` when empty.
///
/// # Returns
///
/// - `Ok(n)` where `n > 0` — received `n` bytes.
/// - `Ok(0)` — EOF.
/// - `Err(TimedOut)` — no data arrived before the deadline.
/// - `Err(InvalidArgument)` / `Err(InvalidHandle)`.
#[allow(clippy::indexing_slicing)]
pub fn recv_timeout(
    handle: StreamSocketHandle,
    buf: &mut [u8],
    timeout_ns: u64,
) -> KernelResult<usize> {
    if buf.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let e = handle.endpoint();
    let peer = e ^ 1;

    // Fast path.
    {
        let mut table = PAIRS.lock();
        let pair = table
            .get_mut(&handle.pair_id())
            .ok_or(KernelError::InvalidHandle)?;

        let n = pair.ring[peer].read_bytes(buf);
        if n > 0 {
            let writer_id = pair.ep[peer].writer_waiter.take();
            drop(table);
            if let Some(task_id) = writer_id {
                sched::wake(task_id);
            }
            super::stats::stream_socket_read(n as u64);
            return Ok(n);
        }
        if pair.recv_eof(e) {
            return Ok(0);
        }
    }

    if timeout_ns == 0 {
        return Err(KernelError::TimedOut);
    }

    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();
    let timer_handle = crate::hrtimer::schedule_ns(timeout_ns, timeout_wake, task);

    loop {
        {
            let mut table = PAIRS.lock();
            let pair = table.get_mut(&handle.pair_id()).ok_or_else(|| {
                crate::hrtimer::cancel(timer_handle);
                KernelError::InvalidHandle
            })?;

            let n = pair.ring[peer].read_bytes(buf);
            if n > 0 {
                let writer_id = pair.ep[peer].writer_waiter.take();
                crate::hrtimer::cancel(timer_handle);
                drop(table);
                if let Some(task_id) = writer_id {
                    sched::wake(task_id);
                }
                super::stats::stream_socket_read(n as u64);
                return Ok(n);
            }

            if pair.recv_eof(e) {
                crate::hrtimer::cancel(timer_handle);
                return Ok(0);
            }

            if crate::hrtimer::now_ns() >= deadline_ns {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot.  A timed wait maps the interruption to EINTR (no
            // restart) at the syscall layer.
            if deliverable_signal_pending(pid) {
                if pair.ep[e].reader_waiter == Some(task) {
                    pair.ep[e].reader_waiter = None;
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::Interrupted);
            }

            pair.ep[e].reader_waiter = Some(task);
        }

        super::stats::stream_socket_read_block();
        park_for_socket(pid, task);
    }
}

/// Send bytes with a timeout (nanoseconds).
///
/// Blocks up to `timeout_ns` waiting for outgoing buffer space.
/// `timeout_ns == 0` returns `Err(TimedOut)` when the ring is full.
///
/// # Returns
///
/// - `Ok(n)` — sent `n` bytes.
/// - `Err(TimedOut)` — no space before the deadline.
/// - `Err(ChannelClosed)` — peer's read side is gone.
/// - `Err(InvalidArgument)` / `Err(InvalidHandle)`.
#[allow(clippy::indexing_slicing)]
pub fn send_timeout(
    handle: StreamSocketHandle,
    data: &[u8],
    timeout_ns: u64,
) -> KernelResult<usize> {
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let e = handle.endpoint();
    let peer = e ^ 1;

    // Fast path.
    {
        let mut table = PAIRS.lock();
        let pair = table
            .get_mut(&handle.pair_id())
            .ok_or(KernelError::InvalidHandle)?;

        if pair.send_broken(e) {
            return Err(KernelError::ChannelClosed);
        }

        let written = pair.ring[e].write_bytes(data);
        if written > 0 {
            let reader_id = pair.ep[peer].reader_waiter.take();
            drop(table);
            if let Some(task_id) = reader_id {
                sched::wake(task_id);
            }
            super::stats::stream_socket_write(written as u64);
            return Ok(written);
        }
    }

    if timeout_ns == 0 {
        return Err(KernelError::TimedOut);
    }

    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();
    let timer_handle = crate::hrtimer::schedule_ns(timeout_ns, timeout_wake, task);

    loop {
        {
            let mut table = PAIRS.lock();
            let pair = table.get_mut(&handle.pair_id()).ok_or_else(|| {
                crate::hrtimer::cancel(timer_handle);
                KernelError::InvalidHandle
            })?;

            if pair.send_broken(e) {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            }

            let written = pair.ring[e].write_bytes(data);
            if written > 0 {
                let reader_id = pair.ep[peer].reader_waiter.take();
                crate::hrtimer::cancel(timer_handle);
                drop(table);
                if let Some(task_id) = reader_id {
                    sched::wake(task_id);
                }
                super::stats::stream_socket_write(written as u64);
                return Ok(written);
            }

            if crate::hrtimer::now_ns() >= deadline_ns {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot.  A timed wait maps the interruption to EINTR (no
            // restart) at the syscall layer.
            if deliverable_signal_pending(pid) {
                if pair.ep[e].writer_waiter == Some(task) {
                    pair.ep[e].writer_waiter = None;
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::Interrupted);
            }

            pair.ep[e].writer_waiter = Some(task);
        }

        super::stats::stream_socket_write_block();
        park_for_socket(pid, task);
    }
}

/// Duplicate an endpoint handle reference.
///
/// Increments the endpoint's refcount and returns the same handle.  The
/// caller must `close()` it when done; only the final close (refcount →
/// 0) marks the endpoint closed and wakes the peer.  Used at spawn time
/// so a parent and child can each hold the same endpoint.
///
/// # Returns
///
/// - `Ok(handle)` — refcount incremented.
/// - `Err(InvalidHandle)` — pair not found, endpoint already closed, or
///   the refcount would overflow.
#[allow(clippy::indexing_slicing)]
pub fn dup(handle: StreamSocketHandle) -> KernelResult<StreamSocketHandle> {
    let mut table = PAIRS.lock();
    let pair = table
        .get_mut(&handle.pair_id())
        .ok_or(KernelError::InvalidHandle)?;

    let slot = &mut pair.ep[handle.endpoint()].refcount;
    if *slot == 0 {
        return Err(KernelError::InvalidHandle);
    }
    *slot = slot.checked_add(1).ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Close (drop one reference to) an endpoint handle.
///
/// Decrements the endpoint's refcount.  The final close (refcount → 0)
/// marks the endpoint closed: the peer's blocked reader wakes to EOF and
/// its blocked writer wakes to a broken pipe.  When both endpoints are
/// fully closed the pair is freed.
#[allow(clippy::indexing_slicing)]
pub fn close(handle: StreamSocketHandle) {
    let e = handle.endpoint();
    let peer = e ^ 1;
    let mut wake_reader = None;
    let mut wake_writer = None;

    {
        let mut table = PAIRS.lock();
        if let Some(pair) = table.get_mut(&handle.pair_id()) {
            pair.ep[e].refcount = pair.ep[e].refcount.saturating_sub(1);
            if pair.ep[e].refcount > 0 {
                return;
            }
            pair.ep[e].closed = true;
            // Wake the peer: its reader will observe EOF, its writer a
            // broken pipe.
            wake_reader = pair.ep[peer].reader_waiter.take();
            wake_writer = pair.ep[peer].writer_waiter.take();

            if pair.ep[0].closed && pair.ep[1].closed {
                table.remove(&handle.pair_id());
                super::stats::stream_socket_destroyed();
            }
        }
    }

    if let Some(task_id) = wake_reader {
        sched::wake(task_id);
    }
    if let Some(task_id) = wake_writer {
        sched::wake(task_id);
    }
}

/// Shut down one or both directions of an endpoint (`shutdown(2)`).
///
/// - `SHUT_RD`: further receives on this endpoint return EOF; the peer's
///   subsequent sends fail with a broken pipe.
/// - `SHUT_WR`: further sends fail; the peer reading our ring sees EOF
///   once it drains buffered bytes.
/// - `SHUT_RDWR`: both.
///
/// # Returns
///
/// - `Ok(())` on success.
/// - `Err(InvalidArgument)` — `how` is not a recognised value.
/// - `Err(InvalidHandle)` — pair not found.
#[allow(clippy::indexing_slicing)]
pub fn shutdown(handle: StreamSocketHandle, how: u32) -> KernelResult<()> {
    if how != SHUT_RD && how != SHUT_WR && how != SHUT_RDWR {
        return Err(KernelError::InvalidArgument);
    }
    let e = handle.endpoint();
    let peer = e ^ 1;

    // At most four distinct tasks could need waking (our reader/writer
    // and the peer's reader/writer).  Collect them, then wake outside
    // the lock to preserve the PAIRS → SCHED ordering.
    let mut wakes: [Option<TaskId>; 4] = [None; 4];
    {
        let mut table = PAIRS.lock();
        let pair = table
            .get_mut(&handle.pair_id())
            .ok_or(KernelError::InvalidHandle)?;

        if how == SHUT_RD || how == SHUT_RDWR {
            pair.ep[e].rd_shut = true;
            // Our blocked reader sees EOF; the peer's blocked writer now
            // has a broken pipe.
            wakes[0] = pair.ep[e].reader_waiter.take();
            wakes[1] = pair.ep[peer].writer_waiter.take();
        }
        if how == SHUT_WR || how == SHUT_RDWR {
            pair.ep[e].wr_shut = true;
            // Our blocked writer has a broken pipe; the peer's blocked
            // reader will see EOF once the ring drains.
            wakes[2] = pair.ep[e].writer_waiter.take();
            wakes[3] = pair.ep[peer].reader_waiter.take();
        }
    }

    for w in wakes.into_iter().flatten() {
        sched::wake(w);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Polling helpers (for poll/select and the completion port)
// ---------------------------------------------------------------------------

/// Poll an endpoint for readiness.
///
/// Returns a bitmask (same encoding as the pipe poll):
/// - bit 0 (0x01): readable (incoming data, or read-side EOF)
/// - bit 2 (0x04): writable (outgoing space, or write would error)
/// - bit 3 (0x08): error (broken pipe — write side gone)
/// - bit 4 (0x10): hangup (peer write side gone)
#[allow(clippy::indexing_slicing)]
pub fn poll_status(handle: StreamSocketHandle) -> u16 {
    let e = handle.endpoint();
    let peer = e ^ 1;

    let table = PAIRS.lock();
    let Some(pair) = table.get(&handle.pair_id()) else {
        return 0x10; // POLL_HANGUP — pair gone.
    };

    let mut flags: u16 = 0;

    // Readable: incoming bytes buffered, or a read would return EOF.
    if pair.ring[peer].len > 0 || pair.recv_eof(e) {
        flags |= 0x01;
    }
    // Hangup: peer's write side is gone.
    if pair.ep[peer].closed || pair.ep[peer].wr_shut {
        flags |= 0x10;
    }

    // Writable: a write would not block — either space is available, or
    // it would immediately error (broken pipe is "ready" for polling).
    let broken = pair.send_broken(e);
    if broken || pair.ring[e].writable() > 0 {
        flags |= 0x04;
    }
    if broken {
        flags |= 0x08; // POLL_ERROR (write will fail with EPIPE).
    }

    flags
}

/// Return the number of bytes available to receive on this endpoint.
///
/// Returns 0 if the handle is invalid.
#[allow(clippy::indexing_slicing)]
pub fn readable_bytes(handle: StreamSocketHandle) -> u64 {
    let e = handle.endpoint();
    let peer = e ^ 1;
    let table = PAIRS.lock();
    match table.get(&handle.pair_id()) {
        Some(pair) => pair.ring[peer].len as u64,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run stream socket self-tests.
///
/// Verified to pass (all 7 subtests print OK).  NOT invoked at boot: its heap
/// allocation churn triggers a pre-existing, state/timing-sensitive boot hang
/// during later ring-3 process spawns (see todo.txt "ADVANCED DIAGNOSIS").
/// Retained to run on demand once that MM/fault bug is fixed.
#[allow(dead_code)]
pub fn self_test() -> KernelResult<()> {
    serial_println!("[stream_socket] Running stream socket self-test...");

    test_basic_bidirectional()?;
    test_partial_recv()?;
    test_peer_close_eof()?;
    test_peer_close_broken()?;
    test_nonblocking()?;
    test_shutdown_wr()?;
    test_dup_refcount()?;

    serial_println!("[stream_socket] Stream socket self-test PASSED");
    Ok(())
}

/// Test 1: bytes flow in both directions independently.
fn test_basic_bidirectional() -> KernelResult<()> {
    let (a, b) = create();

    let n = send(a, b"ping")?;
    if n != 4 {
        serial_println!("[stream_socket]   FAIL: send a returned {}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }

    let mut buf = [0u8; 16];
    let n = recv(b, &mut buf)?;
    if n != 4 || buf.get(..4) != Some(b"ping".as_slice()) {
        serial_println!("[stream_socket]   FAIL: recv b returned {}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }

    // Reverse direction.
    let n = send(b, b"pong!!")?;
    if n != 6 {
        serial_println!("[stream_socket]   FAIL: send b returned {}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }
    let n = recv(a, &mut buf)?;
    if n != 6 || buf.get(..6) != Some(b"pong!!".as_slice()) {
        serial_println!("[stream_socket]   FAIL: recv a returned {}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }

    close(a);
    close(b);
    serial_println!("[stream_socket]   Basic bidirectional: OK");
    Ok(())
}

/// Test 2: reading less than was sent returns a partial stream read.
fn test_partial_recv() -> KernelResult<()> {
    let (a, b) = create();

    send(a, b"0123456789")?;

    let mut buf = [0u8; 4];
    let n = recv(b, &mut buf)?;
    if n != 4 || buf != *b"0123" {
        serial_println!("[stream_socket]   FAIL: partial recv n={}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }

    let mut buf2 = [0u8; 16];
    let n = recv(b, &mut buf2)?;
    if n != 6 || buf2.get(..6) != Some(b"456789".as_slice()) {
        serial_println!("[stream_socket]   FAIL: remainder recv n={}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }

    close(a);
    close(b);
    serial_println!("[stream_socket]   Partial recv: OK");
    Ok(())
}

/// Test 3: closing the peer yields EOF once buffered data is drained.
fn test_peer_close_eof() -> KernelResult<()> {
    let (a, b) = create();

    send(a, b"tail")?;
    close(a); // a's write side gone.

    let mut buf = [0u8; 16];
    // Buffered bytes still readable.
    let n = recv(b, &mut buf)?;
    if n != 4 {
        serial_println!("[stream_socket]   FAIL: drain after close n={}", n);
        close(b);
        return Err(KernelError::InternalError);
    }
    // Now EOF.
    let n = recv(b, &mut buf)?;
    if n != 0 {
        serial_println!("[stream_socket]   FAIL: expected EOF, n={}", n);
        close(b);
        return Err(KernelError::InternalError);
    }

    close(b);
    serial_println!("[stream_socket]   Peer close EOF: OK");
    Ok(())
}

/// Test 4: sending to a closed peer fails with a broken pipe.
fn test_peer_close_broken() -> KernelResult<()> {
    let (a, b) = create();

    close(b); // b's read side gone.

    match send(a, b"x") {
        Err(KernelError::ChannelClosed) => {}
        other => {
            serial_println!("[stream_socket]   FAIL: send to closed peer: {:?}", other);
            close(a);
            return Err(KernelError::InternalError);
        }
    }

    close(a);
    serial_println!("[stream_socket]   Peer close broken pipe: OK");
    Ok(())
}

/// Test 5: non-blocking recv on an empty stream returns `WouldBlock`.
fn test_nonblocking() -> KernelResult<()> {
    let (a, b) = create();

    let mut buf = [0u8; 8];
    match try_recv(b, &mut buf) {
        Err(KernelError::WouldBlock) => {}
        other => {
            serial_println!("[stream_socket]   FAIL: try_recv empty: {:?}", other);
            close(a);
            close(b);
            return Err(KernelError::InternalError);
        }
    }

    try_send(a, b"hi")?;
    let n = try_recv(b, &mut buf)?;
    if n != 2 {
        serial_println!("[stream_socket]   FAIL: try_recv after send n={}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }

    close(a);
    close(b);
    serial_println!("[stream_socket]   Non-blocking: OK");
    Ok(())
}

/// Test 6: `shutdown(SHUT_WR)` lets the peer drain then see EOF, and
/// fails our own further sends.
fn test_shutdown_wr() -> KernelResult<()> {
    let (a, b) = create();

    send(a, b"bye")?;
    shutdown(a, SHUT_WR)?;

    // Further sends from a fail.
    match send(a, b"more") {
        Err(KernelError::ChannelClosed) => {}
        other => {
            serial_println!("[stream_socket]   FAIL: send after SHUT_WR: {:?}", other);
            close(a);
            close(b);
            return Err(KernelError::InternalError);
        }
    }

    // Peer drains the buffered bytes, then sees EOF.
    let mut buf = [0u8; 16];
    let n = recv(b, &mut buf)?;
    if n != 3 {
        serial_println!("[stream_socket]   FAIL: drain after SHUT_WR n={}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }
    let n = recv(b, &mut buf)?;
    if n != 0 {
        serial_println!("[stream_socket]   FAIL: EOF after SHUT_WR n={}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }

    // b can still send to a (only a's write direction was shut).
    let n = send(b, b"ack")?;
    if n != 3 {
        serial_println!("[stream_socket]   FAIL: reverse send after SHUT_WR n={}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }
    let n = recv(a, &mut buf)?;
    if n != 3 {
        serial_println!("[stream_socket]   FAIL: reverse recv after SHUT_WR n={}", n);
        close(a);
        close(b);
        return Err(KernelError::InternalError);
    }

    close(a);
    close(b);
    serial_println!("[stream_socket]   shutdown(SHUT_WR): OK");
    Ok(())
}

/// Test 7: `dup()` keeps an endpoint open until the final close.
fn test_dup_refcount() -> KernelResult<()> {
    let (a, b) = create();

    let a2 = dup(a)?;
    if a2 != a {
        serial_println!("[stream_socket]   FAIL: dup returned a different handle");
        close(a);
        close(a2);
        close(b);
        return Err(KernelError::InternalError);
    }

    send(a, b"ref")?;
    // Close one reference — b must NOT see EOF yet.
    close(a);

    let mut buf = [0u8; 16];
    let n = recv(b, &mut buf)?;
    if n != 3 {
        serial_println!("[stream_socket]   FAIL: recv after partial close n={}", n);
        close(a2);
        close(b);
        return Err(KernelError::InternalError);
    }
    match try_recv(b, &mut buf) {
        Err(KernelError::WouldBlock) => {}
        other => {
            serial_println!("[stream_socket]   FAIL: try_recv partial close: {:?}", other);
            close(a2);
            close(b);
            return Err(KernelError::InternalError);
        }
    }

    // Final reference closed — now EOF.
    close(a2);
    let n = recv(b, &mut buf)?;
    if n != 0 {
        serial_println!("[stream_socket]   FAIL: EOF after final close n={}", n);
        close(b);
        return Err(KernelError::InternalError);
    }

    close(b);
    serial_println!("[stream_socket]   Dup refcount: OK");
    Ok(())
}

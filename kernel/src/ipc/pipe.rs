//! Pipe — one-way kernel-buffered byte stream IPC.
//!
//! A pipe is a unidirectional byte stream between a writer and a reader.
//! Unlike channels (structured messages), pipes carry raw bytes with no
//! framing — the kernel does not interpret the data.
//!
//! ## Design
//!
//! - **One-way only**: a pipe has one write end and one read end.
//!   Two-way communication uses two pipes or a channel.
//! - **Kernel-buffered**: the kernel allocates a ring buffer.  The
//!   writer appends bytes; the reader consumes bytes.
//! - **Blocking semantics** (default):
//!   - Writer blocks if the buffer is full.
//!   - Reader blocks if the buffer is empty.
//!   - Non-blocking variants return `WouldBlock`.
//! - **Close detection**: when the writer closes, reads drain remaining
//!   bytes then return 0. When the reader closes, writes fail with
//!   `ChannelClosed` (broken pipe).
//!
//! ## Performance
//!
//! - Latency: ~1–5 µs per read/write syscall.
//! - Throughput limited by buffer size and syscall overhead — typically
//!   2–5 GB/s for large transfers.
//!
//! ## Future Optimizations (NOT YET IMPLEMENTED)
//!
//! - Splice/vmsplice: move pages between pipe and file handle (or
//!   between pipes) without copying to userspace.
//! - vmsplice: map userspace pages directly into the pipe buffer.
//!
//! ## Lock Ordering
//!
//! `PIPES` → `SCHED` (write/read may call `sched::wake()`).

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

/// Default pipe buffer capacity in bytes.
///
/// 64 KiB matches Linux's default pipe buffer and is a reasonable
/// balance between memory usage and throughput.  With 16 KiB pages,
/// this is exactly 4 pages.
const DEFAULT_BUFFER_CAPACITY: usize = 64 * 1024;

/// Minimum pipe buffer capacity exposed to userspace via
/// `fcntl(F_SETPIPE_SZ)`.  Linux requires the requested size to be
/// at least one page (typically 4096 bytes); below that it returns
/// EINVAL.  We mirror the same lower bound so userspace probes get
/// the same error code they would on Linux.
pub const MIN_PIPE_BUFFER_CAPACITY: usize = 4096;

/// Maximum pipe buffer capacity exposed to userspace via
/// `fcntl(F_SETPIPE_SZ)`.  Linux's default `/proc/sys/fs/pipe-max-size`
/// is 1 MiB; raising that on Linux requires `CAP_SYS_RESOURCE`.  We
/// treat this as a hard upper bound: anything above returns EPERM,
/// matching what an unprivileged Linux caller would see at the
/// default sysctl.
pub const MAX_PIPE_BUFFER_CAPACITY: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// Pipe ID and Handle
// ---------------------------------------------------------------------------

/// Unique identifier for a pipe.
type PipeId = u64;

/// Counter for generating unique pipe IDs.
static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_pipe_id() -> PipeId {
    NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to one end of a pipe.
///
/// Encodes the pipe ID and end (read=0, write=1) in a single `u64`.
/// Bit 0 = end (0=read, 1=write), bits 1–63 = pipe ID.
///
/// Pipe handles occupy a different namespace from channel handles.
/// The syscall layer distinguishes them by which syscall is used
/// (`pipe_read` vs `channel_recv`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PipeHandle(u64);

impl PipeHandle {
    /// Create a handle for a given pipe and end.
    #[allow(clippy::arithmetic_side_effects)]
    fn new(pipe_id: PipeId, end: PipeEnd) -> Self {
        Self((pipe_id << 1) | end.as_bit())
    }

    /// Reconstruct a handle from its raw u64 representation.
    ///
    /// Used by the syscall layer to convert register values back to
    /// typed handles.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw u64 representation of this handle.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Extract the pipe ID.
    #[allow(clippy::arithmetic_side_effects)]
    fn pipe_id(self) -> PipeId {
        self.0 >> 1
    }

    /// Extract which end this handle refers to.
    pub fn end(self) -> PipeEnd {
        if self.0 & 1 == 0 {
            PipeEnd::Read
        } else {
            PipeEnd::Write
        }
    }
}

/// Which end of the pipe a handle refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeEnd {
    Read,
    Write,
}

impl PipeEnd {
    const fn as_bit(self) -> u64 {
        match self {
            Self::Read => 0,
            Self::Write => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Pipe internals
// ---------------------------------------------------------------------------

/// A kernel pipe: a ring buffer with reader/writer state.
struct Pipe {
    /// The byte buffer.  Data lives in `buf[head..tail]` (logically),
    /// wrapping around.
    buf: Vec<u8>,
    /// Read position (index into `buf`).
    head: usize,
    /// Number of bytes currently in the buffer.
    len: usize,
    /// Whether the read end has been closed.
    read_closed: bool,
    /// Whether the write end has been closed.
    write_closed: bool,
    /// Task blocked on read (waiting for data).
    reader_waiter: Option<TaskId>,
    /// Task blocked on write (waiting for space).
    writer_waiter: Option<TaskId>,
    /// Reference count for the read end.  Each `create()` and each
    /// `dup()` of a read handle adds 1; each `close()` of a read
    /// handle subtracts 1.  When this hits 0 the read end is
    /// logically closed (waking any blocked writer with
    /// `ChannelClosed`).  Matches Linux pipe semantics: a pipe end
    /// stays open as long as at least one fd refers to it.
    reader_refcount: u32,
    /// Reference count for the write end.  Symmetric with
    /// `reader_refcount`.  Hitting 0 wakes blocked readers with EOF.
    writer_refcount: u32,
}

impl Pipe {
    /// Create a new pipe with the given buffer capacity.
    fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            head: 0,
            len: 0,
            read_closed: false,
            write_closed: false,
            reader_waiter: None,
            writer_waiter: None,
            reader_refcount: 1,
            writer_refcount: 1,
        }
    }

    /// How many bytes can be read without blocking.
    ///
    /// Used by future `ioctl`/`fstat`-like queries on pipe handles.
    #[allow(dead_code)]
    fn readable(&self) -> usize {
        self.len
    }

    /// How many bytes can be written without blocking.
    #[allow(clippy::arithmetic_side_effects)]
    fn writable(&self) -> usize {
        self.buf.len() - self.len
    }

    /// Write bytes into the ring buffer.  Returns number of bytes
    /// written (may be less than `data.len()` if buffer space is
    /// limited — partial writes fill available space).
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn write_bytes(&mut self, data: &[u8]) -> usize {
        let avail = self.writable();
        let to_write = data.len().min(avail);
        if to_write == 0 {
            return 0;
        }

        let cap = self.buf.len();
        let write_pos = (self.head + self.len) % cap;

        // First chunk: from write_pos to end of buffer (or to_write).
        let first = to_write.min(cap - write_pos);
        self.buf[write_pos..write_pos + first]
            .copy_from_slice(&data[..first]);

        // Second chunk: wrap around to start of buffer.
        let second = to_write - first;
        if second > 0 {
            self.buf[..second].copy_from_slice(&data[first..first + second]);
        }

        self.len += to_write;
        to_write
    }

    /// Read bytes from the ring buffer.  Returns number of bytes read
    /// (may be less than `buf.len()` if fewer bytes are available).
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn read_bytes(&mut self, out: &mut [u8]) -> usize {
        let to_read = out.len().min(self.len);
        if to_read == 0 {
            return 0;
        }

        let cap = self.buf.len();

        // First chunk: from head to end of buffer (or to_read).
        let first = to_read.min(cap - self.head);
        out[..first].copy_from_slice(&self.buf[self.head..self.head + first]);

        // Second chunk: wrap around to start of buffer.
        let second = to_read - first;
        if second > 0 {
            out[first..first + second]
                .copy_from_slice(&self.buf[..second]);
        }

        self.head = (self.head + to_read) % cap;
        self.len -= to_read;
        to_read
    }

    /// Copy up to `out.len()` bytes from the buffered data starting at logical
    /// `offset` (0 = oldest buffered byte) into `out`, WITHOUT consuming them
    /// (head/len are unchanged).  Returns the number of bytes copied, which is
    /// 0 once `offset >= len`.  Used by `tee`, which duplicates pipe data
    /// non-destructively.
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn peek_bytes_at(&self, offset: usize, out: &mut [u8]) -> usize {
        if offset >= self.len {
            return 0;
        }
        let avail = self.len - offset;
        let to_read = out.len().min(avail);
        if to_read == 0 {
            return 0;
        }
        let cap = self.buf.len();
        let start = (self.head + offset) % cap;

        // First chunk: from `start` to end of buffer (or to_read).
        let first = to_read.min(cap - start);
        out[..first].copy_from_slice(&self.buf[start..start + first]);

        // Second chunk: wrap around to the start of the buffer.
        let second = to_read - first;
        if second > 0 {
            out[first..first + second].copy_from_slice(&self.buf[..second]);
        }

        to_read
    }
}

// ---------------------------------------------------------------------------
// Global pipe table
// ---------------------------------------------------------------------------

/// Global table of all live pipes.
///
/// Protected by a single spinlock.  Pipes are identified by their
/// `PipeId`.  When both ends are closed, the pipe is removed.
///
/// Lock ordering: `PIPES` → `SCHED`.
static PIPES: Mutex<BTreeMap<PipeId, Pipe>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Signal-interruptible blocking helpers
// ---------------------------------------------------------------------------

/// The owning user process id of the current task, or `0` for a kernel task.
///
/// Pipe waits are interruptible by signals only for user processes; kernel
/// tasks (`pid == 0`) have no signal state and park uninterruptibly, exactly as
/// before.
fn current_user_pid() -> u64 {
    crate::proc::thread::owner_process(sched::current_task_id()).unwrap_or(0)
}

/// `true` if a deliverable (unblocked) signal is pending for `pid`.
///
/// Always `false` for `pid == 0` (kernel task — no signal context).
fn deliverable_signal_pending(pid: u64) -> bool {
    pid != 0
        && crate::proc::signal::has_pending_in_mask(pid, !crate::proc::signal::blocked(pid))
}

/// Park the current task for a pipe wait, interruptibly for user processes.
///
/// For a user process this registers a signal-waiter (so `set_pending` wakes the
/// park when a deliverable signal arrives) using the register-then-recheck idiom
/// to close the post-before-park race, blocks, then deregisters.  Kernel tasks
/// park uninterruptibly.  The caller's surrounding loop must, after this
/// returns, re-acquire the pipe lock and re-evaluate both the pipe state and
/// [`deliverable_signal_pending`] — a signal wake is reported by the latter, not
/// by this function.
fn park_for_pipe(pid: u64, task: u64) {
    if pid == 0 {
        sched::block_current();
        return;
    }
    let deliverable = !crate::proc::signal::blocked(pid);
    crate::proc::signal::register_signalfd_waiter(pid, task, deliverable);
    if crate::proc::signal::has_pending_in_mask(pid, deliverable) {
        // A signal arrived between enqueue and registration — don't block; the
        // caller's loop will observe the pending signal and return Interrupted.
        crate::proc::signal::deregister_signalfd_waiter(pid, task);
        return;
    }
    sched::block_current();
    crate::proc::signal::deregister_signalfd_waiter(pid, task);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new pipe, returning `(read_handle, write_handle)`.
///
/// The read handle can only be used with [`read`] / [`try_read`].
/// The write handle can only be used with [`write`] / [`try_write`].
pub fn create() -> (PipeHandle, PipeHandle) {
    let id = alloc_pipe_id();
    let pipe = Pipe::new(DEFAULT_BUFFER_CAPACITY);

    let mut table = PIPES.lock();
    table.insert(id, pipe);

    let read_handle = PipeHandle::new(id, PipeEnd::Read);
    let write_handle = PipeHandle::new(id, PipeEnd::Write);

    super::stats::pipe_created();
    (read_handle, write_handle)
}

/// Write bytes to a pipe (blocking).
///
/// Writes as many bytes as possible into the pipe's buffer.  If the
/// buffer is full, blocks the calling task until space is available
/// (the reader reads some data).
///
/// # Returns
///
/// - `Ok(n)` — wrote `n` bytes (always > 0 on success).
/// - `Err(ChannelClosed)` — the read end is closed (broken pipe).
/// - `Err(InvalidArgument)` — `data` is empty.
/// - `Err(InvalidHandle)` — handle is a read handle, not a write handle.
pub fn write(handle: PipeHandle, data: &[u8]) -> KernelResult<usize> {
    if handle.end() != PipeEnd::Write {
        return Err(KernelError::InvalidHandle);
    }
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();

    loop {
        {
            let mut table = PIPES.lock();
            let pipe = table
                .get_mut(&handle.pipe_id())
                .ok_or(KernelError::InvalidHandle)?;

            // Check if reader has closed.
            if pipe.read_closed {
                return Err(KernelError::ChannelClosed);
            }

            // Try to write some bytes.
            let written = pipe.write_bytes(data);
            if written > 0 {
                // Wake the reader if it was blocked waiting for data.
                let pipe_id = handle.pipe_id();
                let reader_id = pipe.reader_waiter.take();
                drop(table);

                if let Some(task_id) = reader_id {
                    sched::wake(task_id);
                }
                crate::ktrace::record(
                    crate::ktrace::Category::Ipc,
                    crate::ktrace::event::PIPE_WRITE,
                    pipe_id,
                    written as u64,
                );
                super::stats::pipe_write(written as u64);
                return Ok(written);
            }

            // Buffer is full.  Before parking, honour a deliverable signal —
            // otherwise a blocked writer could never be interrupted.  Clear any
            // stale waiter slot left by a prior signal wake.
            if deliverable_signal_pending(pid) {
                if pipe.writer_waiter == Some(task) {
                    pipe.writer_waiter = None;
                }
                return Err(KernelError::Interrupted);
            }

            // Block until space is available.
            pipe.writer_waiter = Some(task);
        }

        // Block (interruptibly for user processes).  The reader wakes us when it
        // drains data; a signal wakes us via the registered signal-waiter.
        super::stats::pipe_write_block();
        park_for_pipe(pid, task);

        // Re-check on wake (loop back to top).
    }
}

/// Write bytes to a pipe (non-blocking).
///
/// # Returns
///
/// - `Ok(n)` — wrote `n` bytes.
/// - `Err(WouldBlock)` — buffer is full, no bytes written.
/// - `Err(ChannelClosed)` — the read end is closed.
/// - `Err(InvalidHandle)` — not a write handle.
pub fn try_write(handle: PipeHandle, data: &[u8]) -> KernelResult<usize> {
    if handle.end() != PipeEnd::Write {
        return Err(KernelError::InvalidHandle);
    }
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let wake_reader;
    let result;

    {
        let mut table = PIPES.lock();
        let pipe = table
            .get_mut(&handle.pipe_id())
            .ok_or(KernelError::InvalidHandle)?;

        if pipe.read_closed {
            return Err(KernelError::ChannelClosed);
        }

        let written = pipe.write_bytes(data);
        if written == 0 {
            return Err(KernelError::WouldBlock);
        }

        wake_reader = pipe.reader_waiter.take();
        result = Ok(written);
    }

    if let Some(task_id) = wake_reader {
        sched::wake(task_id);
    }
    if let Ok(n) = result {
        super::stats::pipe_write(n as u64);
    }
    result
}

/// Read bytes from a pipe (blocking).
///
/// Reads up to `buf.len()` bytes from the pipe.  If the pipe is
/// empty, blocks the calling task until data is available or the
/// write end is closed.
///
/// # Returns
///
/// - `Ok(n)` where `n > 0` — read `n` bytes into `buf`.
/// - `Ok(0)` — the write end is closed and no data remains (EOF).
/// - `Err(InvalidArgument)` — `buf` is empty.
/// - `Err(InvalidHandle)` — handle is a write handle, not a read handle.
pub fn read(handle: PipeHandle, buf: &mut [u8]) -> KernelResult<usize> {
    if handle.end() != PipeEnd::Read {
        return Err(KernelError::InvalidHandle);
    }
    if buf.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();

    loop {
        {
            let mut table = PIPES.lock();
            let pipe = table
                .get_mut(&handle.pipe_id())
                .ok_or(KernelError::InvalidHandle)?;

            // Try to read some bytes.
            let n = pipe.read_bytes(buf);
            if n > 0 {
                // Wake the writer if it was blocked waiting for space.
                let pipe_id = handle.pipe_id();
                let writer_id = pipe.writer_waiter.take();
                drop(table);

                if let Some(task_id) = writer_id {
                    sched::wake(task_id);
                }
                crate::ktrace::record(
                    crate::ktrace::Category::Ipc,
                    crate::ktrace::event::PIPE_READ,
                    pipe_id,
                    n as u64,
                );
                super::stats::pipe_read(n as u64);
                return Ok(n);
            }

            // No data.  If writer is closed, return EOF.
            if pipe.write_closed {
                return Ok(0);
            }

            // Buffer empty, writer still open.  Honour a deliverable signal
            // before parking (otherwise a blocked reader is uninterruptible);
            // clear any stale waiter slot left by a prior signal wake.
            if deliverable_signal_pending(pid) {
                if pipe.reader_waiter == Some(task) {
                    pipe.reader_waiter = None;
                }
                return Err(KernelError::Interrupted);
            }

            // Block.
            pipe.reader_waiter = Some(task);
        }

        // Block (interruptibly for user processes).  The writer wakes us when it
        // writes data; a signal wakes us via the registered signal-waiter.
        super::stats::pipe_read_block();
        park_for_pipe(pid, task);
    }
}

/// Read bytes from a pipe (non-blocking).
///
/// # Returns
///
/// - `Ok(n)` where `n > 0` — read `n` bytes.
/// - `Ok(0)` — write end is closed and no data remains (EOF).
/// - `Err(WouldBlock)` — pipe is empty but writer is still open.
/// - `Err(InvalidHandle)` — not a read handle.
pub fn try_read(handle: PipeHandle, buf: &mut [u8]) -> KernelResult<usize> {
    if handle.end() != PipeEnd::Read {
        return Err(KernelError::InvalidHandle);
    }
    if buf.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let mut wake_writer = None;
    let result;

    {
        let mut table = PIPES.lock();
        let pipe = table
            .get_mut(&handle.pipe_id())
            .ok_or(KernelError::InvalidHandle)?;

        let n = pipe.read_bytes(buf);
        if n > 0 {
            wake_writer = pipe.writer_waiter.take();
            result = Ok(n);
        } else if pipe.write_closed {
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
            super::stats::pipe_read(n as u64);
        }
    }
    result
}

/// Peek at buffered pipe data WITHOUT consuming it.
///
/// Copies up to `buf.len()` bytes starting at logical `offset` (0 = the oldest
/// buffered byte) into `buf`, leaving the pipe contents untouched.  Returns the
/// number of bytes copied (0 once `offset` is at or past the buffered length).
///
/// This is the primitive behind `tee(2)`, which duplicates data from one pipe
/// into another non-destructively: the caller peeks successive offsets and
/// writes the copies into the destination pipe.
///
/// # Returns
///
/// - `Ok(n)` — copied `n` bytes (`n == 0` when nothing is buffered at `offset`).
/// - `Err(InvalidArgument)` — `buf` is empty.
/// - `Err(InvalidHandle)` — handle is a write handle, or the pipe is gone.
pub fn peek_at(handle: PipeHandle, offset: u64, buf: &mut [u8]) -> KernelResult<usize> {
    if handle.end() != PipeEnd::Read {
        return Err(KernelError::InvalidHandle);
    }
    if buf.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let table = PIPES.lock();
    let pipe = table
        .get(&handle.pipe_id())
        .ok_or(KernelError::InvalidHandle)?;
    #[allow(clippy::cast_possible_truncation)]
    let off = offset.min(usize::MAX as u64) as usize;
    Ok(pipe.peek_bytes_at(off, buf))
}

/// Block the calling task until the pipe has data to read or the write end
/// closes (EOF).  Unlike [`read`], this does not consume any bytes — it is the
/// blocking-wait primitive for `tee`, which must wait for input on an empty
/// source before duplicating it.
///
/// # Returns
///
/// - `Ok(true)` — data is now available to read.
/// - `Ok(false)` — the write end is closed and no data remains (EOF).
/// - `Err(InvalidHandle)` — handle is a write handle, or the pipe is gone.
pub fn wait_readable(handle: PipeHandle) -> KernelResult<bool> {
    if handle.end() != PipeEnd::Read {
        return Err(KernelError::InvalidHandle);
    }
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        {
            let mut table = PIPES.lock();
            let pipe = table
                .get_mut(&handle.pipe_id())
                .ok_or(KernelError::InvalidHandle)?;
            if pipe.len > 0 {
                return Ok(true);
            }
            if pipe.write_closed {
                return Ok(false);
            }
            // Empty, writer still open.  Honour a deliverable signal before
            // parking; clear any stale waiter slot from a prior signal wake.
            if deliverable_signal_pending(pid) {
                if pipe.reader_waiter == Some(task) {
                    pipe.reader_waiter = None;
                }
                return Err(KernelError::Interrupted);
            }
            // Register and block.
            pipe.reader_waiter = Some(task);
        }
        super::stats::pipe_read_block();
        park_for_pipe(pid, task);
    }
}

/// Read bytes from a pipe with a timeout (nanoseconds).
///
/// Blocks up to `timeout_ns` nanoseconds waiting for data.
/// Returns `Err(TimedOut)` if the timeout expires before any data
/// arrives.  Returns immediately if data is available or the writer
/// has closed (EOF).
///
/// `timeout_ns = 0` is equivalent to `try_read()` (immediate check),
/// returning `Err(TimedOut)` instead of `Err(WouldBlock)` when empty.
///
/// # Returns
///
/// - `Ok(n)` where `n > 0` — read `n` bytes.
/// - `Ok(0)` — write end is closed and no data remains (EOF).
/// - `Err(TimedOut)` — no data arrived within the deadline.
/// - `Err(InvalidHandle)` — not a read handle or pipe doesn't exist.
pub fn read_timeout(handle: PipeHandle, buf: &mut [u8], timeout_ns: u64) -> KernelResult<usize> {
    if handle.end() != PipeEnd::Read {
        return Err(KernelError::InvalidHandle);
    }
    if buf.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    // Fast path: try without blocking.
    {
        let mut table = PIPES.lock();
        let pipe = table
            .get_mut(&handle.pipe_id())
            .ok_or(KernelError::InvalidHandle)?;

        let n = pipe.read_bytes(buf);
        if n > 0 {
            let writer_id = pipe.writer_waiter.take();
            drop(table);
            if let Some(task_id) = writer_id {
                sched::wake(task_id);
            }
            super::stats::pipe_read(n as u64);
            return Ok(n);
        }

        if pipe.write_closed {
            return Ok(0);
        }
    }

    // Non-blocking mode.
    if timeout_ns == 0 {
        return Err(KernelError::TimedOut);
    }

    // Schedule a timer to wake us at the deadline.
    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();
    let timer_handle = crate::hrtimer::schedule_ns(timeout_ns, timeout_wake, task);

    // Block loop.
    loop {
        {
            let mut table = PIPES.lock();
            let pipe = table
                .get_mut(&handle.pipe_id())
                .ok_or_else(|| {
                    crate::hrtimer::cancel(timer_handle);
                    KernelError::InvalidHandle
                })?;

            let n = pipe.read_bytes(buf);
            if n > 0 {
                let writer_id = pipe.writer_waiter.take();
                crate::hrtimer::cancel(timer_handle);
                drop(table);
                if let Some(task_id) = writer_id {
                    sched::wake(task_id);
                }
                super::stats::pipe_read(n as u64);
                return Ok(n);
            }

            if pipe.write_closed {
                crate::hrtimer::cancel(timer_handle);
                return Ok(0);
            }

            // Check timeout.
            if crate::hrtimer::now_ns() >= deadline_ns {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot from a prior signal wake.  A timed wait maps the
            // interruption to EINTR (no restart) at the syscall layer.
            if deliverable_signal_pending(pid) {
                if pipe.reader_waiter == Some(task) {
                    pipe.reader_waiter = None;
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::Interrupted);
            }

            // Register as waiter.
            pipe.reader_waiter = Some(task);
        }

        super::stats::pipe_read_block();
        park_for_pipe(pid, task);
    }
}

/// Write bytes to a pipe with a timeout (nanoseconds).
///
/// Blocks up to `timeout_ns` nanoseconds waiting for buffer space.
/// Returns `Err(TimedOut)` if the deadline expires without writing.
///
/// `timeout_ns = 0` is equivalent to `try_write()` (returns `TimedOut`
/// instead of `WouldBlock` when buffer is full).
///
/// # Returns
///
/// - `Ok(n)` — wrote `n` bytes.
/// - `Err(TimedOut)` — no space within the deadline.
/// - `Err(ChannelClosed)` — reader closed.
/// - `Err(InvalidHandle)` — not a write handle.
pub fn write_timeout(handle: PipeHandle, data: &[u8], timeout_ns: u64) -> KernelResult<usize> {
    if handle.end() != PipeEnd::Write {
        return Err(KernelError::InvalidHandle);
    }
    if data.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    // Fast path: try without blocking.
    {
        let mut table = PIPES.lock();
        let pipe = table
            .get_mut(&handle.pipe_id())
            .ok_or(KernelError::InvalidHandle)?;

        if pipe.read_closed {
            return Err(KernelError::ChannelClosed);
        }

        let written = pipe.write_bytes(data);
        if written > 0 {
            let reader_id = pipe.reader_waiter.take();
            drop(table);
            if let Some(task_id) = reader_id {
                sched::wake(task_id);
            }
            super::stats::pipe_write(written as u64);
            return Ok(written);
        }
    }

    // Non-blocking mode.
    if timeout_ns == 0 {
        return Err(KernelError::TimedOut);
    }

    // Schedule a timer to wake us at the deadline.
    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();
    let timer_handle = crate::hrtimer::schedule_ns(timeout_ns, timeout_wake, task);

    // Block loop.
    loop {
        {
            let mut table = PIPES.lock();
            let pipe = table
                .get_mut(&handle.pipe_id())
                .ok_or_else(|| {
                    crate::hrtimer::cancel(timer_handle);
                    KernelError::InvalidHandle
                })?;

            if pipe.read_closed {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            }

            let written = pipe.write_bytes(data);
            if written > 0 {
                let reader_id = pipe.reader_waiter.take();
                crate::hrtimer::cancel(timer_handle);
                drop(table);
                if let Some(task_id) = reader_id {
                    sched::wake(task_id);
                }
                super::stats::pipe_write(written as u64);
                return Ok(written);
            }

            // Check timeout.
            if crate::hrtimer::now_ns() >= deadline_ns {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot from a prior signal wake.  A timed wait maps the
            // interruption to EINTR (no restart) at the syscall layer.
            if deliverable_signal_pending(pid) {
                if pipe.writer_waiter == Some(task) {
                    pipe.writer_waiter = None;
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::Interrupted);
            }

            // Register as waiter.
            pipe.writer_waiter = Some(task);
        }

        super::stats::pipe_write_block();
        park_for_pipe(pid, task);
    }
}

/// Duplicate a pipe handle reference.
///
/// Increments the refcount on the appropriate end (read or write) and
/// returns the same handle.  The caller must `close()` the handle when
/// done — only the final `close()` for an end (refcount → 0) marks
/// that end as logically closed and wakes the other side.
///
/// Used at spawn time so a parent and child can each hold the same
/// pipe end (matching Linux fork() pipe inheritance).
///
/// # Returns
///
/// - `Ok(handle)` — refcount incremented; same handle returned.
/// - `Err(InvalidHandle)` — pipe not found (already fully torn down)
///   or the refcount would overflow `u32::MAX`.
pub fn dup(handle: PipeHandle) -> KernelResult<PipeHandle> {
    let mut table = PIPES.lock();
    let pipe = table
        .get_mut(&handle.pipe_id())
        .ok_or(KernelError::InvalidHandle)?;

    let slot = match handle.end() {
        PipeEnd::Read => &mut pipe.reader_refcount,
        PipeEnd::Write => &mut pipe.writer_refcount,
    };

    // If the end is already at refcount 0 it should have been removed,
    // but defensively reject dup against a zero refcount.
    if *slot == 0 {
        return Err(KernelError::InvalidHandle);
    }

    *slot = slot.checked_add(1).ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Close (drop one reference to) a pipe handle.
///
/// Decrements the refcount on the handle's end.  Only the final close
/// (refcount → 0) marks that end as logically closed:
///
/// - Read end fully closed: wakes any blocked writer (`ChannelClosed`).
/// - Write end fully closed: wakes any blocked reader (sees EOF).
///
/// When both ends are fully closed, the pipe is removed from the
/// table.
pub fn close(handle: PipeHandle) {
    let mut wake_task = None;

    {
        let mut table = PIPES.lock();
        if let Some(pipe) = table.get_mut(&handle.pipe_id()) {
            match handle.end() {
                PipeEnd::Read => {
                    pipe.reader_refcount = pipe.reader_refcount.saturating_sub(1);
                    if pipe.reader_refcount > 0 {
                        // Still referenced — keep the end open.
                        return;
                    }
                    pipe.read_closed = true;
                    // Wake blocked writer — it will see ChannelClosed.
                    wake_task = pipe.writer_waiter.take();
                }
                PipeEnd::Write => {
                    pipe.writer_refcount = pipe.writer_refcount.saturating_sub(1);
                    if pipe.writer_refcount > 0 {
                        return;
                    }
                    pipe.write_closed = true;
                    // Wake blocked reader — it will see EOF (0 bytes).
                    wake_task = pipe.reader_waiter.take();
                }
            }

            // Remove pipe if both ends are fully closed.
            if pipe.read_closed && pipe.write_closed {
                table.remove(&handle.pipe_id());
            }
        }
    }

    if let Some(task_id) = wake_task {
        sched::wake(task_id);
    }
}

// ---------------------------------------------------------------------------
// Capacity query / resize (for fcntl F_GETPIPE_SZ / F_SETPIPE_SZ)
// ---------------------------------------------------------------------------

/// Return the byte capacity of the ring buffer behind `handle`.
///
/// Linux's `fcntl(F_GETPIPE_SZ)` reports the size of the kernel-side
/// pipe buffer regardless of which end the caller holds — the read
/// end and the write end share one buffer.  We mirror that: the
/// `handle.end()` is not consulted.
///
/// # Returns
///
/// - `Ok(cap)` — the buffer's current capacity in bytes.
/// - `Err(InvalidHandle)` — the pipe no longer exists.
pub fn capacity(handle: PipeHandle) -> KernelResult<usize> {
    let table = PIPES.lock();
    let pipe = table
        .get(&handle.pipe_id())
        .ok_or(KernelError::InvalidHandle)?;
    Ok(pipe.buf.len())
}

/// Resize the ring buffer behind `handle` to exactly `new_cap`
/// bytes, preserving any data currently buffered.
///
/// This helper enforces only the invariant that data must not be
/// silently dropped:
///
/// - `new_cap < currently buffered bytes` → `DeviceBusy`.
///
/// User-facing policy (the per-page lower bound and the
/// `MAX_PIPE_BUFFER_CAPACITY` upper bound that Linux distinguishes
/// as EINVAL vs EPERM) lives in the syscall layer — see
/// `sys_fcntl`'s `F_SETPIPE_SZ` arm.  Kernel callers that need to
/// resize within the [`MIN_PIPE_BUFFER_CAPACITY`, `MAX_PIPE_BUFFER_CAPACITY`]
/// window should consult those constants themselves.
///
/// On success the call replaces the underlying `Vec<u8>` and returns
/// the realised capacity (the same value as `new_cap`).  Linux rounds
/// the requested size up to a power of two; we keep the caller's
/// exact value because our buffer is a plain `Vec` and nothing else
/// relies on a power-of-two size.
///
/// `handle.end()` is not consulted: read and write ends share one
/// buffer, so resizing through either is semantically identical.
///
/// # Returns
///
/// - `Ok(new_cap)` — the new buffer capacity.
/// - `Err(InvalidHandle)` — the pipe no longer exists.
/// - `Err(DeviceBusy)` — buffered data wouldn't fit in `new_cap`.
pub fn set_capacity(handle: PipeHandle, new_cap: usize) -> KernelResult<usize> {
    let mut table = PIPES.lock();
    let pipe = table
        .get_mut(&handle.pipe_id())
        .ok_or(KernelError::InvalidHandle)?;

    if new_cap < pipe.len {
        return Err(KernelError::DeviceBusy);
    }

    // Allocate the new buffer and copy logical contents starting from
    // `head`.  After the move the new buffer is unwrapped: data sits
    // at indices [0, len) and head resets to 0.
    let mut new_buf = vec![0u8; new_cap];
    if pipe.len > 0 {
        let old_cap = pipe.buf.len();
        let head = pipe.head;
        let len = pipe.len;
        // First chunk: head..end-of-old-buffer (or len if no wrap).
        let first = len.min(old_cap.saturating_sub(head));
        // SAFETY-equivalent: indices computed from old_cap/head/len
        // which are all consistent within the pipe; `new_buf` has
        // exactly `new_cap >= len` slots, so writes stay in-bounds.
        new_buf[..first].copy_from_slice(&pipe.buf[head..head + first]);
        let second = len - first;
        if second > 0 {
            new_buf[first..first + second].copy_from_slice(&pipe.buf[..second]);
        }
    }

    pipe.buf = new_buf;
    pipe.head = 0;
    // pipe.len is unchanged — same number of bytes still buffered.

    Ok(new_cap)
}

// ---------------------------------------------------------------------------
// Polling helpers (for completion port)
// ---------------------------------------------------------------------------

/// Check if a pipe read-end has data available (or is at EOF).
///
/// Returns `true` if `read()` would not block (data available or
/// writer closed).  Returns `false` if the handle is invalid or if
/// the buffer is empty and the writer is still open.
pub fn readable(handle: PipeHandle) -> bool {
    let table = PIPES.lock();
    let Some(pipe) = table.get(&handle.pipe_id()) else {
        return false;
    };
    // Readable if there's data, or if writer closed (EOF).
    pipe.len > 0 || pipe.write_closed
}

/// Check if a pipe write-end has buffer space available.
///
/// Returns `true` if `write()` would not block (space available or
/// reader closed — broken pipe is also "ready" for write-end
/// polling).  Returns `false` if the buffer is full and reader is
/// alive.
#[allow(clippy::arithmetic_side_effects)]
pub fn writable(handle: PipeHandle) -> bool {
    let table = PIPES.lock();
    let Some(pipe) = table.get(&handle.pipe_id()) else {
        return false;
    };
    // Writable if there's space, or if reader closed (broken pipe).
    (pipe.buf.len() - pipe.len) > 0 || pipe.read_closed
}

/// Poll a pipe handle for readiness (used by SYS_PIPE_POLL).
///
/// Returns a bitmask:
/// - bit 0 (0x01): readable (data available or writer closed)
/// - bit 2 (0x04): writable (buffer space available or reader closed)
/// - bit 4 (0x10): hangup (other end closed)
pub fn poll_status(handle: PipeHandle) -> u16 {
    let mut flags: u16 = 0;
    let table = PIPES.lock();
    let Some(pipe) = table.get(&handle.pipe_id()) else {
        // Pipe not found — report error/hangup.
        return 0x10; // POLL_HANGUP
    };

    let is_read_end = handle.end() == PipeEnd::Read;

    if is_read_end {
        // Read end: readable if data available or writer closed (EOF).
        if pipe.len > 0 || pipe.write_closed {
            flags |= 0x01; // POLL_READABLE
        }
        if pipe.write_closed {
            flags |= 0x10; // POLL_HANGUP (writer gone)
        }
    } else {
        // Write end: writable if space available or reader closed (EPIPE).
        if (pipe.buf.len() - pipe.len) > 0 || pipe.read_closed {
            flags |= 0x04; // POLL_WRITABLE
        }
        if pipe.read_closed {
            // POSIX/Linux: write end of a broken pipe reports POLLERR
            // (not POLLHUP).  Programs check POLLERR to detect that a
            // write will fail with EPIPE.
            flags |= 0x08; // POLL_ERROR (broken pipe)
        }
    }

    flags
}

/// Return the number of bytes available for reading in the pipe.
///
/// For the read end: returns `pipe.len` (bytes buffered).
/// For the write end: returns available space in the buffer.
/// If the pipe is not found, returns 0.
pub fn readable_bytes(handle: PipeHandle) -> u64 {
    let table = PIPES.lock();
    let Some(pipe) = table.get(&handle.pipe_id()) else {
        return 0;
    };

    if handle.end() == PipeEnd::Read {
        pipe.len as u64
    } else {
        // Write end: report writable space (less useful but consistent).
        pipe.buf.len().saturating_sub(pipe.len) as u64
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run pipe self-tests.
///
/// Tests:
/// 1. Basic write and read (round-trip).
/// 2. Partial read (read less than written).
/// 3. Write-end close detection (EOF).
/// 4. Read-end close detection (broken pipe).
/// 5. Non-blocking operations.
/// 6. Blocking write + read via spawned task.
/// 7. `dup`/`close` refcount semantics.
/// 8. `capacity`/`set_capacity` round-trip (covers
///    `fcntl(F_GETPIPE_SZ)` and `F_SETPIPE_SZ` at the helper layer).
pub fn self_test() -> KernelResult<()> {
    serial_println!("[pipe] Running pipe self-test...");

    test_basic_write_read()?;
    test_partial_read()?;
    test_writer_close_eof()?;
    test_reader_close_broken_pipe()?;
    test_nonblocking()?;
    test_blocking_roundtrip()?;
    test_dup_refcount()?;
    test_capacity_roundtrip()?;
    test_peek_nondestructive()?;

    serial_println!("[pipe] Pipe self-test PASSED");
    Ok(())
}

/// Test the `tee(2)` primitives: `peek_at` copies buffered data without
/// consuming it, and `wait_readable` reports data/EOF without draining.
///
/// 1. `peek_at(0)` returns the whole payload; a later offset returns the tail;
///    an offset at/past the buffered length returns 0.
/// 2. After peeking, a real `read` still returns every byte (non-destructive).
/// 3. `peek_at` / `wait_readable` on a write-end handle → `InvalidHandle`.
/// 4. `wait_readable` returns `Ok(true)` when data is buffered.
/// 5. `wait_readable` returns `Ok(false)` once the writer closes with the
///    buffer drained (EOF), without blocking.
#[allow(clippy::cognitive_complexity)]
fn test_peek_nondestructive() -> KernelResult<()> {
    let (rh, wh) = create();
    let payload = b"tee-peek-payload";
    write(wh, payload)?;

    // (1) Peek the whole payload at offset 0 — contents match, nothing consumed.
    let mut buf = [0u8; 32];
    let n = peek_at(rh, 0, &mut buf)?;
    if n != payload.len() || &buf[..n] != payload {
        serial_println!("[pipe]   FAIL: peek@0 wrong (n={})", n);
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }
    // Peek from an interior offset — returns the tail only.
    let mut tail = [0u8; 32];
    let m = peek_at(rh, 4, &mut tail)?;
    if m != payload.len() - 4 || tail[..m] != payload[4..] {
        serial_println!("[pipe]   FAIL: peek@4 wrong (m={})", m);
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }
    // Offset at/past the buffered length yields nothing.
    if peek_at(rh, payload.len() as u64, &mut buf)? != 0
        || peek_at(rh, payload.len() as u64 + 100, &mut buf)? != 0
    {
        serial_println!("[pipe]   FAIL: peek past end returned data");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    // (4) Data buffered → wait_readable returns true without blocking.
    if !wait_readable(rh)? {
        serial_println!("[pipe]   FAIL: wait_readable false with data buffered");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    // (2) A real read still sees every byte — peeks consumed nothing.
    let r = read(rh, &mut buf)?;
    if r != payload.len() || &buf[..r] != payload {
        serial_println!("[pipe]   FAIL: read after peek lost data (r={})", r);
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    // (3) Peek / wait on the write end must be rejected.
    if !matches!(peek_at(wh, 0, &mut buf), Err(KernelError::InvalidHandle))
        || !matches!(wait_readable(wh), Err(KernelError::InvalidHandle))
    {
        serial_println!("[pipe]   FAIL: peek/wait on write end not rejected");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    // (5) Close the writer with the buffer drained → wait_readable = EOF (false),
    //     and it returns immediately rather than parking.
    close(wh);
    if wait_readable(rh)? {
        serial_println!("[pipe]   FAIL: wait_readable not EOF after writer close");
        close(rh);
        return Err(KernelError::InternalError);
    }
    close(rh);

    serial_println!("[pipe]   peek/wait_readable (tee primitives): OK");
    Ok(())
}

/// Test: `capacity` / `set_capacity` cover the `F_GETPIPE_SZ` /
/// `F_SETPIPE_SZ` semantics.  We verify:
///
/// 1. A fresh pipe reports `DEFAULT_BUFFER_CAPACITY` on both ends.
/// 2. Growing preserves buffered data.
/// 3. Shrinking below buffered data returns `DeviceBusy`.
/// 4. Shrinking to (or above) buffered data succeeds and unwraps
///    the ring (head resets to 0; data still readable in order).
/// 5. A closed pipe surfaces `InvalidHandle` from both queries.
#[allow(clippy::cognitive_complexity)]
fn test_capacity_roundtrip() -> KernelResult<()> {
    let (rh, wh) = create();

    // (1) Default capacity, observable from either end.
    if capacity(rh)? != DEFAULT_BUFFER_CAPACITY
        || capacity(wh)? != DEFAULT_BUFFER_CAPACITY
    {
        serial_println!("[pipe]   FAIL: default capacity wrong");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    // (2) Grow to MAX, then verify the new size sticks and prior
    //     state survives.  Write a payload first so the resize copy
    //     path is exercised.
    let payload = b"capacity-roundtrip-payload";
    write(wh, payload)?;
    let grown = set_capacity(wh, MAX_PIPE_BUFFER_CAPACITY)?;
    if grown != MAX_PIPE_BUFFER_CAPACITY
        || capacity(rh)? != MAX_PIPE_BUFFER_CAPACITY
    {
        serial_println!("[pipe]   FAIL: grow returned {}", grown);
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    // Read back the payload — must be byte-for-byte unchanged.
    let mut buf = [0u8; 64];
    let n = read(rh, &mut buf)?;
    if n != payload.len() || &buf[..n] != payload {
        serial_println!("[pipe]   FAIL: payload corrupted on grow");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    // (3) Buffer some data, then attempt to shrink below the
    //     buffered count — must fail without dropping bytes.
    write(wh, b"keep-me")?;
    match set_capacity(wh, MIN_PIPE_BUFFER_CAPACITY) {
        Ok(_) => {} // 4096 >= 7, so this is allowed; verify below.
        Err(e) => {
            serial_println!("[pipe]   FAIL: shrink rejected unexpectedly: {:?}", e);
            close(rh);
            close(wh);
            return Err(KernelError::InternalError);
        }
    }
    if capacity(rh)? != MIN_PIPE_BUFFER_CAPACITY {
        serial_println!("[pipe]   FAIL: shrink capacity wrong");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }
    // Data must survive shrink.
    let n = read(rh, &mut buf)?;
    if n != 7 || &buf[..n] != b"keep-me" {
        serial_println!("[pipe]   FAIL: data lost across shrink");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    // (4) Now force a DeviceBusy: write more than 8 bytes, then ask
    //     for a 4-byte buffer (below MIN — would also fail at the
    //     syscall layer, but here we go straight to the helper to
    //     prove the data-fit check fires; we use exactly the
    //     buffered byte-count threshold).
    //
    //     The helper itself does NOT enforce MIN_PIPE_BUFFER_CAPACITY
    //     — that lives in `sys_fcntl` — so we can request an
    //     arbitrarily small value to trip the buffered-bytes check.
    write(wh, b"twelve_bytes")?; // 12 bytes
    let busy = set_capacity(wh, 4);
    match busy {
        Err(KernelError::DeviceBusy) => {} // expected
        other => {
            serial_println!("[pipe]   FAIL: expected DeviceBusy, got {:?}", other);
            close(rh);
            close(wh);
            return Err(KernelError::InternalError);
        }
    }
    // Drain to leave the pipe clean before close.
    let _ = read(rh, &mut buf)?;

    // (5) After close, both queries surface InvalidHandle.
    close(rh);
    close(wh);
    if !matches!(capacity(rh), Err(KernelError::InvalidHandle))
        || !matches!(
            set_capacity(rh, DEFAULT_BUFFER_CAPACITY),
            Err(KernelError::InvalidHandle)
        )
    {
        serial_println!("[pipe]   FAIL: closed pipe didn't return InvalidHandle");
        return Err(KernelError::InternalError);
    }

    serial_println!("[pipe]   capacity/set_capacity round-trip: OK");
    Ok(())
}

/// Test: `dup()` increments the per-end refcount; the end stays open
/// until the final `close()`.
fn test_dup_refcount() -> KernelResult<()> {
    let (rh, wh) = create();

    // Dup the write end — refcount 1 → 2.
    let wh2 = dup(wh)?;
    if wh2 != wh {
        serial_println!("[pipe]   FAIL: dup returned a different write handle");
        close(rh);
        close(wh);
        close(wh2);
        return Err(KernelError::InternalError);
    }

    // Write something through the original handle.
    let n = write(wh, b"abc")?;
    if n != 3 {
        serial_println!("[pipe]   FAIL: write returned {}", n);
        close(rh);
        close(wh);
        close(wh2);
        return Err(KernelError::InternalError);
    }

    // Close one writer reference — refcount 2 → 1.  Reader must NOT
    // see EOF yet because the write end is still referenced.
    close(wh);

    let mut buf = [0u8; 16];
    let n = read(rh, &mut buf)?;
    if n != 3 || buf.get(..3) != Some(b"abc".as_slice()) {
        serial_println!("[pipe]   FAIL: read after partial close: n={}", n);
        close(rh);
        close(wh2);
        return Err(KernelError::InternalError);
    }

    // The pipe is empty and the writer is still open — try_read should
    // return WouldBlock (not EOF).
    match try_read(rh, &mut buf) {
        Err(KernelError::WouldBlock) => {}
        other => {
            serial_println!(
                "[pipe]   FAIL: try_read after partial writer close: {:?}",
                other
            );
            close(rh);
            close(wh2);
            return Err(KernelError::InternalError);
        }
    }

    // Final writer close — refcount 1 → 0.  Now the reader sees EOF.
    close(wh2);
    let n = read(rh, &mut buf)?;
    if n != 0 {
        serial_println!("[pipe]   FAIL: expected EOF after final writer close: n={}", n);
        close(rh);
        return Err(KernelError::InternalError);
    }

    close(rh);
    serial_println!("[pipe]   Dup refcount: OK");
    Ok(())
}

/// Test 1: basic write and read.
fn test_basic_write_read() -> KernelResult<()> {
    let (rh, wh) = create();

    let data = b"hello pipe";
    let written = write(wh, data)?;
    if written != data.len() {
        serial_println!("[pipe]   FAIL: wrote {} expected {}", written, data.len());
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    let mut buf = [0u8; 64];
    let n = read(rh, &mut buf)?;
    if n != data.len() {
        serial_println!("[pipe]   FAIL: read {} expected {}", n, data.len());
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }
    if buf.get(..n) != Some(data.as_slice()) {
        serial_println!("[pipe]   FAIL: data mismatch");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    close(rh);
    close(wh);
    serial_println!("[pipe]   Basic write/read: OK");
    Ok(())
}

/// Test 2: partial read.
fn test_partial_read() -> KernelResult<()> {
    let (rh, wh) = create();

    let data = b"abcdefgh";
    write(wh, data)?;

    // Read only 4 bytes.
    let mut buf = [0u8; 4];
    let n = read(rh, &mut buf)?;
    if n != 4 {
        serial_println!("[pipe]   FAIL: partial read got {} expected 4", n);
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }
    if &buf != b"abcd" {
        serial_println!("[pipe]   FAIL: partial read data mismatch");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    // Read remaining 4 bytes.
    let mut buf2 = [0u8; 4];
    let n2 = read(rh, &mut buf2)?;
    if n2 != 4 || &buf2 != b"efgh" {
        serial_println!("[pipe]   FAIL: second partial read failed");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    close(rh);
    close(wh);
    serial_println!("[pipe]   Partial read: OK");
    Ok(())
}

/// Test 3: writer closes → reader gets EOF after draining.
fn test_writer_close_eof() -> KernelResult<()> {
    let (rh, wh) = create();

    write(wh, b"last")?;
    close(wh); // Close write end.

    // Read should still return buffered data.
    let mut buf = [0u8; 16];
    let n = read(rh, &mut buf)?;
    if n != 4 || buf.get(..4) != Some(b"last".as_slice()) {
        serial_println!("[pipe]   FAIL: expected buffered data after writer close");
        close(rh);
        return Err(KernelError::InternalError);
    }

    // Next read should return 0 (EOF).
    let n2 = read(rh, &mut buf)?;
    if n2 != 0 {
        serial_println!("[pipe]   FAIL: expected EOF (0), got {}", n2);
        close(rh);
        return Err(KernelError::InternalError);
    }

    close(rh);
    serial_println!("[pipe]   Writer close EOF: OK");
    Ok(())
}

/// Test 4: reader closes → writer gets `ChannelClosed`.
fn test_reader_close_broken_pipe() -> KernelResult<()> {
    let (rh, wh) = create();

    close(rh); // Close read end.

    // Write should fail with ChannelClosed.
    match write(wh, b"broken") {
        Err(KernelError::ChannelClosed) => {} // Expected.
        Ok(n) => {
            serial_println!("[pipe]   FAIL: write succeeded ({}) after reader close", n);
            close(wh);
            return Err(KernelError::InternalError);
        }
        Err(e) => {
            serial_println!("[pipe]   FAIL: wrong error {:?} (expected ChannelClosed)", e);
            close(wh);
            return Err(KernelError::InternalError);
        }
    }

    close(wh);
    serial_println!("[pipe]   Reader close (broken pipe): OK");
    Ok(())
}

/// Test 5: non-blocking operations.
fn test_nonblocking() -> KernelResult<()> {
    let (rh, wh) = create();

    // try_read on empty pipe → WouldBlock.
    let mut buf = [0u8; 16];
    match try_read(rh, &mut buf) {
        Err(KernelError::WouldBlock) => {} // Expected.
        other => {
            serial_println!("[pipe]   FAIL: try_read on empty pipe: {:?}", other);
            close(rh);
            close(wh);
            return Err(KernelError::InternalError);
        }
    }

    // Write some data, then try_read should succeed.
    write(wh, b"nb")?;
    let n = try_read(rh, &mut buf)?;
    if n != 2 || buf.get(..2) != Some(b"nb".as_slice()) {
        serial_println!("[pipe]   FAIL: try_read data mismatch");
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    close(rh);
    close(wh);
    serial_println!("[pipe]   Non-blocking operations: OK");
    Ok(())
}

/// Counter for blocking test verification.
static PIPE_TEST_RESULT: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

/// Task for the blocking read test.
///
/// Reads from the pipe (blocks until data arrives), then stores the
/// first byte into `PIPE_TEST_RESULT`.
extern "C" fn pipe_reader_task(read_handle_raw: u64) {
    let rh = PipeHandle::from_raw(read_handle_raw);
    let mut buf = [0u8; 16];
    if let Ok(n) = read(rh, &mut buf)
        && n > 0
        && let Some(&byte) = buf.first()
    {
        PIPE_TEST_RESULT.store(
            u32::from(byte),
            core::sync::atomic::Ordering::SeqCst,
        );
    }
}

/// Test 6: blocking read via spawned task.
fn test_blocking_roundtrip() -> KernelResult<()> {
    PIPE_TEST_RESULT.store(0, core::sync::atomic::Ordering::SeqCst);

    let (rh, wh) = create();

    // Spawn a task that blocks on read.
    sched::spawn(b"pipe-test", 16, pipe_reader_task, rh.raw(), 0)?;

    // Yield to let the reader run and block.
    sched::yield_now();

    // Write data to wake the reader.
    write(wh, &[42])?;

    // Yield to let the reader process the data.
    sched::yield_now();
    sched::yield_now();

    let result = PIPE_TEST_RESULT.load(core::sync::atomic::Ordering::SeqCst);
    if result != 42 {
        serial_println!("[pipe]   FAIL: reader got {}, expected 42", result);
        close(rh);
        close(wh);
        return Err(KernelError::InternalError);
    }

    close(rh);
    close(wh);
    serial_println!("[pipe]   Blocking read/write: OK");
    Ok(())
}

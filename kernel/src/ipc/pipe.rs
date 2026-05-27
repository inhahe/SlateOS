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
use spin::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default pipe buffer capacity in bytes.
///
/// 64 KiB matches Linux's default pipe buffer and is a reasonable
/// balance between memory usage and throughput.  With 16 KiB pages,
/// this is exactly 4 pages.
const DEFAULT_BUFFER_CAPACITY: usize = 64 * 1024;

/// Maximum pipe buffer capacity (for future `fcntl`-style resizing).
const _MAX_BUFFER_CAPACITY: usize = 1024 * 1024; // 1 MiB

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

            // Buffer is full — block until space is available.
            pipe.writer_waiter = Some(sched::current_task_id());
        }

        // Block.  The reader will wake us when it drains some data.
        super::stats::pipe_write_block();
        sched::block_current();

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

            // Buffer empty, writer still open — block.
            pipe.reader_waiter = Some(sched::current_task_id());
        }

        // Block.  The writer will wake us when it writes data.
        super::stats::pipe_read_block();
        sched::block_current();
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

    let timer_handle = crate::hrtimer::schedule_ns(
        timeout_ns,
        timeout_wake,
        sched::current_task_id(),
    );

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

            // Register as waiter.
            pipe.reader_waiter = Some(sched::current_task_id());
        }

        super::stats::pipe_read_block();
        sched::block_current();
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

    let timer_handle = crate::hrtimer::schedule_ns(
        timeout_ns,
        timeout_wake,
        sched::current_task_id(),
    );

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

            // Register as waiter.
            pipe.writer_waiter = Some(sched::current_task_id());
        }

        super::stats::pipe_write_block();
        sched::block_current();
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
pub fn self_test() -> KernelResult<()> {
    serial_println!("[pipe] Running pipe self-test...");

    test_basic_write_read()?;
    test_partial_read()?;
    test_writer_close_eof()?;
    test_reader_close_broken_pipe()?;
    test_nonblocking()?;
    test_blocking_roundtrip()?;
    test_dup_refcount()?;

    serial_println!("[pipe] Pipe self-test PASSED");
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

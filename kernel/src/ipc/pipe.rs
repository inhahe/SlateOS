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
//! - **Whole writes**: a blocking write returns once every byte is in, and
//!   one of at most [`PIPE_BUF`] bytes goes in whole -- never split around
//!   another writer's -- as POSIX requires and Linux's `pipe_write` does.
//! - **Named pipes**: a FIFO's node leads to a pipe of this table
//!   ([`crate::ipc::fifo`]). Such a pipe starts with no end open, its ends
//!   open and close again as the node is opened and closed, and a handle may
//!   hold both ends at once (`O_RDWR`, [`PipeEnd::Both`]). It goes, with its
//!   bytes, when the last end closes.
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

use crate::error::{KernelError, KernelResult};
use crate::sched;
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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

/// A handle to one end of a pipe -- or, for a named pipe opened for reading
/// and writing, to both.
///
/// Encodes the pipe ID and the end in a single `u64`:
///
/// | bits | holds |
/// |---|---|
/// | 0 | the end: 0 read, 1 write |
/// | 1-40 | the pipe ID |
/// | 41-60 | a FIFO reader's open-time writer count ([`Self::seen_writers`]) |
/// | 61 | both ends ([`PipeEnd::Both`]) |
/// | 62 | bits 41-60 are set |
///
/// Bit 63 is never set, so a handle is a positive syscall answer. The two
/// ends of an ordinary pipe differ only in bit 0, which `/proc/<pid>/fd`'s
/// re-open relies on.
///
/// Pipe handles occupy a different namespace from channel handles.
/// The syscall layer distinguishes them by which syscall is used
/// (`pipe_read` vs `channel_recv`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PipeHandle(u64);

/// Bits of a [`PipeHandle`] that hold the pipe ID, above the end bit.
const ID_BITS: u32 = 40;
/// Where a FIFO reader's open-time writer count sits in its handle.
const SEEN_SHIFT: u32 = 41;
/// How many bits of that count are kept.
const SEEN_BITS: u32 = 20;
/// The handle holds both ends.
const BOTH_BIT: u64 = 1 << 61;
/// The handle carries an open-time writer count.
const SEEN_FLAG: u64 = 1 << 62;

impl PipeHandle {
    /// Create a handle for a given pipe and end.
    #[allow(clippy::arithmetic_side_effects)]
    fn new(pipe_id: PipeId, end: PipeEnd) -> Self {
        let id = (pipe_id & ((1u64 << ID_BITS) - 1)) << 1;
        match end {
            PipeEnd::Read => Self(id),
            PipeEnd::Write => Self(id | 1),
            PipeEnd::Both => Self(id | BOTH_BIT),
        }
    }

    /// This read handle, remembering `writers` -- the count of opens for
    /// writing its FIFO had when it was opened, with no writer: until another
    /// writer has come, the empty pipe is not end-of-file to `poll` (Linux's
    /// `f_version`, which `fifo_open` sets for exactly this case).
    #[allow(clippy::arithmetic_side_effects)]
    fn with_seen(self, writers: u32) -> Self {
        let seen = u64::from(writers) & ((1u64 << SEEN_BITS) - 1);
        Self(self.0 | SEEN_FLAG | (seen << SEEN_SHIFT))
    }

    /// The writer count [`Self::with_seen`] remembered, if any.
    #[allow(clippy::cast_possible_truncation, clippy::arithmetic_side_effects)]
    fn seen_writers(self) -> Option<u32> {
        (self.0 & SEEN_FLAG != 0)
            .then_some(((self.0 >> SEEN_SHIFT) & ((1u64 << SEEN_BITS) - 1)) as u32)
    }

    /// Whether the handle may read: a read end, or both.
    #[must_use]
    pub fn reads(self) -> bool {
        matches!(self.end(), PipeEnd::Read | PipeEnd::Both)
    }

    /// Whether the handle may write: a write end, or both.
    #[must_use]
    pub fn writes(self) -> bool {
        matches!(self.end(), PipeEnd::Write | PipeEnd::Both)
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
        (self.0 >> 1) & ((1u64 << ID_BITS) - 1)
    }

    /// The pipe's identity, the same for both ends: what `/proc/<pid>/fd`
    /// shows as `pipe:[N]`, as Linux shows one inode for both.
    #[must_use]
    pub fn pipe_number(self) -> u64 {
        self.pipe_id()
    }

    /// Extract which end this handle refers to.
    pub fn end(self) -> PipeEnd {
        if self.0 & BOTH_BIT != 0 {
            PipeEnd::Both
        } else if self.0 & 1 == 0 {
            PipeEnd::Read
        } else {
            PipeEnd::Write
        }
    }
}

/// Which end of the pipe a handle refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeEnd {
    /// The read end.
    Read,
    /// The write end.
    Write,
    /// Both: a named pipe opened for reading and writing (`O_RDWR`), which
    /// counts as a reader and a writer of its own.
    Both,
}

// ---------------------------------------------------------------------------
// Pipe internals
// ---------------------------------------------------------------------------

// The waiter bookkeeping lives in `super::waiters` because pipes, eventfds,
// stream sockets and timerfds all need exactly the same thing and all four
// originally carried the same single-slot defect.  A pipe end can legitimately
// have several tasks parked on it at once: `dup()` and process spawn hand the
// same end to multiple processes, and `wait_readable` (the `tee` primitive)
// parks on the read end without consuming, alongside a real reader.
pub use super::waiters::Patience;
use super::waiters::{
    Deadline, WaiterSet, current_user_pid, deliverable_signal_pending, park_interruptible, wake_all,
};
use crate::fs::path::PathBuf;
use crate::fs::vfs::{FileHold, FileId};
use crate::sched::task::TaskId;
use alloc::boxed::Box;
use alloc::sync::Arc;

/// The most bytes a write puts in whole (POSIX `PIPE_BUF`, Linux's 4096): a
/// write of at most this many waits for room for all of them, and is never
/// split around another writer's.
pub const PIPE_BUF: usize = 4096;

/// A named pipe's tie to its node ([`crate::ipc::fifo`]).
struct FifoLink {
    /// The node's identity, by which `fifo` finds this pipe.
    id: FileId,
    /// The node, held: pinned while the pipe lasts, and what `fstat` of an
    /// end describes.
    hold: Arc<FileHold>,
    /// The name it was first opened by, for `/proc/<pid>/fd`.
    path: PathBuf,
    /// Opens for reading so far (Linux's `r_counter`): what an open for
    /// writing that waits for a reader watches change.
    r_counter: u32,
    /// Opens for writing so far (`w_counter`), likewise for readers.
    w_counter: u32,
    /// Opens waiting for the other side ([`fifo_wait_partner`]).
    open_waiters: WaiterSet,
}

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
    /// Tasks blocked on read (waiting for data or EOF).
    reader_waiters: WaiterSet,
    /// Tasks blocked on write (waiting for space or EPIPE).
    writer_waiters: WaiterSet,
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
    /// The named pipe this pipe serves, if it is one.
    fifo: Option<Box<FifoLink>>,
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
            reader_waiters: WaiterSet::new(),
            writer_waiters: WaiterSet::new(),
            reader_refcount: 1,
            writer_refcount: 1,
            fifo: None,
        }
    }

    /// Whether a read end with `handle`'s open-time writer count is at end of
    /// file: no writer -- and, for a FIFO reader opened before any writer
    /// came, one has come and gone since ([`PipeHandle::with_seen`]).
    fn eof_for(&self, handle: PipeHandle) -> bool {
        self.write_closed
            && match (handle.seen_writers(), self.fifo.as_ref()) {
                (Some(seen), Some(link)) => {
                    link.w_counter & ((1u32 << SEEN_BITS).wrapping_sub(1)) != seen
                }
                _ => true,
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
        self.buf[write_pos..write_pos + first].copy_from_slice(&data[..first]);

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
            out[first..first + second].copy_from_slice(&self.buf[..second]);
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
// Public API
// ---------------------------------------------------------------------------

/// Create a new pipe, returning `(read_handle, write_handle)`.
///
/// The read handle can only be used with [`read`] / [`try_read`].
/// The write handle can only be used with [`fn@write`] / [`try_write`].
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

/// What a pipe read does with the bytes it finds.
pub enum ReadInto<'a> {
    /// Takes them into this buffer.
    Buffer(&'a mut [u8]),
    /// Only says whether there are any (`Ok(1)`), taking none: what a read
    /// whose buffer cannot take them answers before its copy. Linux's
    /// `pipe_read` copies page by page and stops at the first fault, the
    /// bytes it could not copy kept for the next read.
    Probe,
}

/// What a pipe write does with the room it finds.
pub enum WriteFrom<'a> {
    /// Puts these bytes there.
    Data(&'a [u8]),
    /// Only says whether there is room (`Ok(1)`), writing nothing: what a
    /// write whose buffer cannot be read answers before its copy.
    Probe,
}

/// The one body of [`fn@write`], [`try_write`], [`write_timeout`] and
/// [`probe_write`]: they differ only in how long they wait for room
/// ([`Patience`]) and in what they put there.
///
/// A reader gone is `ChannelClosed` first, whatever the room. A write of at
/// most [`PIPE_BUF`] bytes goes in whole once there is room for all of it --
/// never split around another writer's; a longer one goes in as room comes,
/// and readers are woken as it does. A blocking write returns once every byte
/// is in. What the wait `patience` allows ends it early: with nothing written
/// that is `WouldBlock`, `TimedOut`, or -- a deliverable signal -- `Interrupted`;
/// with some written, the count so far, as Linux's `pipe_write` answers. A
/// reader gone after some went in is the count so far too.
fn write_with(handle: PipeHandle, from: &WriteFrom<'_>, patience: Patience) -> KernelResult<usize> {
    if !handle.writes() {
        return Err(KernelError::InvalidHandle);
    }
    if let WriteFrom::Data(data) = from
        && data.is_empty()
    {
        return Err(KernelError::InvalidArgument);
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();
    let mut deadline = Deadline::new();
    // How much of the data is in so far.
    let mut done: usize = 0;

    loop {
        {
            let mut table = PIPES.lock();
            let pipe = table
                .get_mut(&handle.pipe_id())
                .ok_or(KernelError::InvalidHandle)?;

            // Deregister first: on any iteration after the first we may
            // still be listed (a signal or timer wake does not clear the
            // entry), and every path below either returns or re-registers.
            pipe.writer_waiters.remove(task);

            // A reader gone is a broken pipe, before anything else -- once
            // nothing went in; after, the count so far.
            if pipe.read_closed {
                return if done > 0 {
                    Ok(done)
                } else {
                    Err(KernelError::ChannelClosed)
                };
            }

            match from {
                WriteFrom::Data(data) => {
                    let rest = data.get(done..).unwrap_or_default();
                    // At most PIPE_BUF bytes go in whole or not at all.
                    let fits = if data.len() <= PIPE_BUF {
                        pipe.writable() >= rest.len()
                    } else {
                        pipe.writable() > 0
                    };
                    if fits {
                        let written = pipe.write_bytes(rest);
                        done = done.saturating_add(written);
                        // Wake readers blocked waiting for data.
                        let pipe_id = handle.pipe_id();
                        let readers = pipe.reader_waiters.take_all();
                        drop(table);

                        wake_all(readers);
                        if patience == Patience::Forever {
                            crate::ktrace::record(
                                crate::ktrace::Category::Ipc,
                                crate::ktrace::event::PIPE_WRITE,
                                pipe_id,
                                written as u64,
                            );
                        }
                        super::stats::pipe_write(written as u64);
                        if done >= data.len() {
                            return Ok(done);
                        }
                        // More to put in: look again, with the lock taken
                        // anew, before waiting for room.
                        continue;
                    }
                }
                WriteFrom::Probe => {
                    if pipe.writable() > 0 {
                        return Ok(1);
                    }
                }
            }

            // Full: no wait, a timeout, or a park -- or, with some in, the
            // count so far.
            if let Some(e) = deadline.ends_now(patience, task) {
                return if done > 0 { Ok(done) } else { Err(e) };
            }

            // Before parking, honour a deliverable signal -- otherwise a
            // blocked writer could never be interrupted. A timed wait maps
            // the interruption to EINTR (no restart) at the syscall layer.
            if deliverable_signal_pending(pid) {
                return if done > 0 {
                    Ok(done)
                } else {
                    Err(KernelError::Interrupted)
                };
            }

            // Block until space is available.
            pipe.writer_waiters.insert(task);
        }

        // Block (interruptibly for user processes). The reader wakes us when
        // it drains data; a signal wakes us via the registered signal-waiter,
        // a deadline by its timer.
        super::stats::pipe_write_block();
        park_interruptible(
            pid,
            task,
            crate::wchan::Wait::new(crate::wchan::WaitChannel::Pipe, handle.raw()),
        );
    }
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
/// - `Err(Interrupted)` — a deliverable signal arrived while it waited.
pub fn write(handle: PipeHandle, data: &[u8]) -> KernelResult<usize> {
    write_with(handle, &WriteFrom::Data(data), Patience::Forever)
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
    write_with(handle, &WriteFrom::Data(data), Patience::Never)
}

/// The one body of [`read`], [`try_read`], [`read_timeout`],
/// [`wait_readable`] and [`probe_read`]: they differ only in how long they
/// wait for bytes ([`Patience`]) and in what they do with them.
///
/// Bytes there are taken (or, probing, reported) and writers woken; none
/// and no writer is end of file, `Ok(0)`; none with a writer, the wait
/// `patience` allows: `WouldBlock`, `TimedOut`, or a park that a deliverable
/// signal ends with `Interrupted`.
fn read_with(
    handle: PipeHandle,
    into: &mut ReadInto<'_>,
    patience: Patience,
) -> KernelResult<usize> {
    if !handle.reads() {
        return Err(KernelError::InvalidHandle);
    }
    if let ReadInto::Buffer(buf) = into
        && buf.is_empty()
    {
        return Err(KernelError::InvalidArgument);
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();
    let mut deadline = Deadline::new();

    loop {
        {
            let mut table = PIPES.lock();
            let pipe = table
                .get_mut(&handle.pipe_id())
                .ok_or(KernelError::InvalidHandle)?;

            // Deregister first: a signal or timer wake leaves our entry in
            // place, and every path below either returns or re-registers.
            pipe.reader_waiters.remove(task);

            match into {
                ReadInto::Buffer(buf) => {
                    let n = pipe.read_bytes(buf);
                    if n > 0 {
                        // Wake writers blocked waiting for space.
                        let pipe_id = handle.pipe_id();
                        let writers = pipe.writer_waiters.take_all();
                        drop(table);

                        wake_all(writers);
                        if patience == Patience::Forever {
                            crate::ktrace::record(
                                crate::ktrace::Category::Ipc,
                                crate::ktrace::event::PIPE_READ,
                                pipe_id,
                                n as u64,
                            );
                        }
                        super::stats::pipe_read(n as u64);
                        return Ok(n);
                    }
                }
                ReadInto::Probe => {
                    if pipe.readable() > 0 {
                        return Ok(1);
                    }
                }
            }

            // No data.  If writer is closed, return EOF.
            if pipe.write_closed {
                return Ok(0);
            }

            // Buffer empty, writer still open: no wait, a timeout, or a park.
            if let Some(e) = deadline.ends_now(patience, task) {
                return Err(e);
            }

            // Honour a deliverable signal before parking (otherwise a
            // blocked reader is uninterruptible). A timed wait maps the
            // interruption to EINTR (no restart) at the syscall layer.
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }

            // Block.
            pipe.reader_waiters.insert(task);
        }

        // Block (interruptibly for user processes).  The writer wakes us when it
        // writes data; a signal wakes us via the registered signal-waiter, a
        // deadline by its timer.
        super::stats::pipe_read_block();
        park_interruptible(
            pid,
            task,
            crate::wchan::Wait::new(crate::wchan::WaitChannel::Pipe, handle.raw()),
        );
    }
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
/// - `Err(Interrupted)` — a deliverable signal arrived while it waited.
pub fn read(handle: PipeHandle, buf: &mut [u8]) -> KernelResult<usize> {
    read_with(handle, &mut ReadInto::Buffer(buf), Patience::Forever)
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
    read_with(handle, &mut ReadInto::Buffer(buf), Patience::Never)
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
    if !handle.reads() {
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
/// - `Err(Interrupted)` — a deliverable signal arrived while it waited.
pub fn wait_readable(handle: PipeHandle) -> KernelResult<bool> {
    probe_read(handle, Patience::Forever)
}

/// Whether a read of the pipe would deliver bytes, waiting for them as long
/// as `patience` allows, taking none: what [`read`] (`Forever`),
/// [`try_read`] (`Never`) and [`read_timeout`] (`Upto`) would do, short of
/// moving a byte. `Ok(true)`: bytes are there. `Ok(false)`: end of file. Its
/// errors are theirs.
///
/// # Errors
///
/// As [`read_timeout`] and [`try_read`]: `WouldBlock`, `TimedOut`,
/// `Interrupted`, `InvalidHandle`.
pub fn probe_read(handle: PipeHandle, patience: Patience) -> KernelResult<bool> {
    read_with(handle, &mut ReadInto::Probe, patience).map(|n| n > 0)
}

/// Whether a write to the pipe would find room, waiting for it as long as
/// `patience` allows, writing nothing: what [`fn@write`], [`try_write`] and
/// [`write_timeout`] would do, short of moving a byte. `Ok(())`: there is
/// room. Its errors are theirs -- `ChannelClosed` above all, a reader gone.
///
/// # Errors
///
/// As [`write_timeout`] and [`try_write`]: `ChannelClosed`, `WouldBlock`,
/// `TimedOut`, `Interrupted`, `InvalidHandle`.
pub fn probe_write(handle: PipeHandle, patience: Patience) -> KernelResult<()> {
    write_with(handle, &WriteFrom::Probe, patience).map(|_| ())
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
    read_with(
        handle,
        &mut ReadInto::Buffer(buf),
        Patience::Upto(timeout_ns),
    )
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
    write_with(handle, &WriteFrom::Data(data), Patience::Upto(timeout_ns))
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

    // Each end the handle holds gains a reference -- both, for a FIFO opened
    // for reading and writing.
    let (read, write) = (handle.reads(), handle.writes());
    // If an end is already at refcount 0 it should have been removed, but
    // defensively reject dup against a zero refcount.
    if (read && pipe.reader_refcount == 0) || (write && pipe.writer_refcount == 0) {
        return Err(KernelError::InvalidHandle);
    }
    let readers = if read {
        pipe.reader_refcount.checked_add(1)
    } else {
        Some(pipe.reader_refcount)
    };
    let writers = if write {
        pipe.writer_refcount.checked_add(1)
    } else {
        Some(pipe.writer_refcount)
    };
    let (Some(readers), Some(writers)) = (readers, writers) else {
        return Err(KernelError::InvalidHandle);
    };
    pipe.reader_refcount = readers;
    pipe.writer_refcount = writers;
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
    // Every waiter on the far end must be woken, not just one: EOF and EPIPE
    // are broadcast conditions — they stay true forever, so a task left
    // parked here would never be woken by anything else.
    let mut wake_tasks = Vec::new();
    // A named pipe's tie to its node, when its last end closed: given back
    // with no lock held (`FileHold`, `fifo::forget`).
    let mut gone_fifo = None;

    {
        let mut table = PIPES.lock();
        if let Some(pipe) = table.get_mut(&handle.pipe_id()) {
            if handle.reads() {
                pipe.reader_refcount = pipe.reader_refcount.saturating_sub(1);
                if pipe.reader_refcount == 0 && !pipe.read_closed {
                    pipe.read_closed = true;
                    // Wake blocked writers — they will see ChannelClosed.
                    wake_tasks.extend(pipe.writer_waiters.take_all());
                }
            }
            if handle.writes() {
                pipe.writer_refcount = pipe.writer_refcount.saturating_sub(1);
                if pipe.writer_refcount == 0 && !pipe.write_closed {
                    pipe.write_closed = true;
                    // Wake blocked readers — they will see EOF (0 bytes).
                    wake_tasks.extend(pipe.reader_waiters.take_all());
                }
            }

            // Remove pipe if both ends are fully closed.
            if pipe.read_closed && pipe.write_closed {
                gone_fifo = table
                    .remove(&handle.pipe_id())
                    .and_then(|mut gone| gone.fifo.take());
            }
        }
    }

    wake_all(wake_tasks);
    if let Some(link) = gone_fifo {
        super::fifo::forget(link.id, handle.pipe_id());
        // `link` goes here, its hold on the node with it.
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
/// This helper enforces two invariants:
///
/// - `new_cap < currently buffered bytes` → `DeviceBusy`: data must not be
///   silently dropped.
/// - `new_cap > MAX_PIPE_BUFFER_CAPACITY` → `InvalidArgument`: no pipe may
///   hold more than one call's worth, because the syscall layer bounds every
///   pipe call's copy of a user buffer by that constant (`PIPE_CALL_MAX` in
///   `syscall/handlers.rs`), and a bigger pipe would make those copies
///   truncate transfers silently.  `F_SETPIPE_SZ` refuses first, with the
///   EPERM Linux gives, so this arm guards kernel callers rather than being a
///   user-visible error.
///
/// User-facing policy (the per-page lower bound, and EINVAL vs EPERM) lives
/// in the syscall layer — see `sys_fcntl`'s `F_SETPIPE_SZ` arm.  The lower
/// bound is deliberately not enforced here: the self-test shrinks a pipe
/// below it to reach the `DeviceBusy` arm.
///
/// The new buffer is allocated before the pipe table is locked, and
/// fallibly: a 1 MiB request under memory pressure is `OutOfMemory`, not the
/// panic an infallible `vec!` makes of it, and no other pipe waits on the
/// global lock while the allocator works.
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
/// - `Err(InvalidArgument)` — `new_cap` exceeds `MAX_PIPE_BUFFER_CAPACITY`.
/// - `Err(OutOfMemory)` — the new buffer could not be allocated.
pub fn set_capacity(handle: PipeHandle, new_cap: usize) -> KernelResult<usize> {
    if new_cap > MAX_PIPE_BUFFER_CAPACITY {
        return Err(KernelError::InvalidArgument);
    }
    let mut new_buf: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    new_buf
        .try_reserve_exact(new_cap)
        .map_err(|_| KernelError::OutOfMemory)?;
    new_buf.resize(new_cap, 0);

    let mut table = PIPES.lock();
    let pipe = table
        .get_mut(&handle.pipe_id())
        .ok_or(KernelError::InvalidHandle)?;

    if new_cap < pipe.len {
        return Err(KernelError::DeviceBusy);
    }

    // Copy logical contents starting from `head` into the new buffer.
    // After the move it is unwrapped: data sits at indices [0, len) and
    // head resets to 0.
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
    pipe.len > 0 || pipe.eof_for(handle)
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
    // Writable with room for a whole PIPE_BUF write, or if reader closed
    // (broken pipe): as `poll_status`.
    pipe.writable() >= PIPE_BUF.min(pipe.buf.len()) || pipe.read_closed
}

/// Park `task` on this pipe so that any state change wakes it.
///
/// This is the multi-object (`ipc::multiwait`) counterpart to the
/// registration the blocking `read`/`write` loops above do inline: a task
/// waiting on *several* objects at once cannot use any one object's park loop,
/// so it registers on each of them, tests them all, and parks once.
///
/// Two properties, both deliberate:
///
/// - **Both sets, regardless of which end the handle names or what the caller
///   asked for.** A reader-only waiter still wants the writer-set wakes,
///   because the interesting transitions are not symmetric with the interesting
///   events: `close()` on the read end wakes *writers* with what a poller sees
///   as `POLLERR`, and the write-end-closed EOF that makes a reader ready is
///   delivered by [`close`] to whichever set it happens to hold. Filtering by
///   the requested event mask could therefore drop the very wake the caller is
///   waiting for, and cannot buy anything in exchange: [`WaiterSet`] is
///   wake-all, so an over-broad registration costs one re-check of a condition
///   that is about to be re-checked anyway.
/// - **A stale handle is a no-op**, not an error. The object may be closed
///   between the caller resolving the handle and this call; the readiness test
///   that follows reports hangup for exactly that case (see [`poll_status`]),
///   which is the answer POSIX wants, so there is nothing useful to report
///   here.
///
/// The caller **must** pair this with [`deregister_waiter`] on every exit path
/// — see that function for what a leaked entry does.
pub fn register_waiter(handle: PipeHandle, task: TaskId) {
    let mut table = PIPES.lock();
    let Some(pipe) = table.get_mut(&handle.pipe_id()) else {
        return;
    };
    pipe.reader_waiters.insert(task);
    pipe.writer_waiters.insert(task);
}

/// Undo [`register_waiter`]: remove `task` from both of this pipe's waiter
/// sets.
///
/// Idempotent, and a no-op for a task that was never registered or a handle
/// whose pipe is gone, so it is safe to call unconditionally from a `Drop`.
/// It has to be: a leaked entry names a task that is no longer parked, which
/// wakes an unrelated task once task ids recycle, and — sooner and without
/// needing any recycling — sets that still-live task's sticky `pending_wake`,
/// making its *next*, unrelated `block_current()` return early from a wait
/// nothing satisfied.
pub fn deregister_waiter(handle: PipeHandle, task: TaskId) {
    let mut table = PIPES.lock();
    let Some(pipe) = table.get_mut(&handle.pipe_id()) else {
        return;
    };
    pipe.reader_waiters.remove(task);
    pipe.writer_waiters.remove(task);
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

    if handle.reads() {
        // Read end: readable if data available or writer closed (EOF) -- for
        // a FIFO reader opened before any writer, not until one has come and
        // gone (`Pipe::eof_for`).
        let eof = pipe.eof_for(handle);
        if pipe.len > 0 || eof {
            flags |= 0x01; // POLL_READABLE
        }
        if eof {
            flags |= 0x10; // POLL_HANGUP (writer gone)
        }
    }
    if handle.writes() {
        // Write end: writable with room for a whole PIPE_BUF write -- what
        // Linux's free buffer slot means -- or with the reader gone (EPIPE).
        // With less, a write of PIPE_BUF bytes would wait, and a poller told
        // to write would spin on EAGAIN.
        if pipe.writable() >= PIPE_BUF.min(pipe.buf.len()) || pipe.read_closed {
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

    if handle.reads() {
        pipe.len as u64
    } else {
        // Write end: report writable space (less useful but consistent).
        pipe.buf.len().saturating_sub(pipe.len) as u64
    }
}

/// The bytes in the pipe, unread, whichever end `handle` is: Linux's
/// `FIONREAD` on a pipe (`pipe_ioctl`), which counts the buffer for either
/// end. [`readable_bytes`] answers the write end with the room left instead.
/// 0 for a pipe that does not exist.
#[must_use]
pub fn queued_bytes(handle: PipeHandle) -> u64 {
    PIPES
        .lock()
        .get(&handle.pipe_id())
        .map_or(0, |pipe| pipe.len as u64)
}

// ---------------------------------------------------------------------------
// Named pipes ([`crate::ipc::fifo`])
// ---------------------------------------------------------------------------

/// How a FIFO is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FifoAccess {
    /// For reading (`O_RDONLY`).
    Read,
    /// For writing (`O_WRONLY`).
    Write,
    /// For both (`O_RDWR`).
    Both,
}

/// What an open of a FIFO waits for ([`fifo_wait_partner`]): an open of the
/// other side, seen as its count of opens changing from this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FifoPartner {
    /// A writer: the count of opens for writing was this.
    Writer(u32),
    /// A reader: the count of opens for reading was this.
    Reader(u32),
}

/// Make the pipe of FIFO node `id`, held by `hold`, opened by `path`
/// (`fifo::open`): no end is open yet, so both read as closed until one is
/// attached ([`fifo_attach`]).
pub(crate) fn fifo_create(id: FileId, hold: Arc<FileHold>, path: PathBuf) -> PipeId {
    let pipe_id = alloc_pipe_id();
    let mut pipe = Pipe::new(DEFAULT_BUFFER_CAPACITY);
    pipe.reader_refcount = 0;
    pipe.writer_refcount = 0;
    pipe.read_closed = true;
    pipe.write_closed = true;
    pipe.fifo = Some(Box::new(FifoLink {
        id,
        hold,
        path,
        r_counter: 0,
        w_counter: 0,
        open_waiters: WaiterSet::new(),
    }));
    PIPES.lock().insert(pipe_id, pipe);
    super::stats::pipe_created();
    pipe_id
}

/// Whether FIFO pipe `pipe_id` is still there: its last end may have closed
/// since `fifo` looked it up.
pub(crate) fn fifo_alive(pipe_id: PipeId) -> bool {
    PIPES.lock().contains_key(&pipe_id)
}

/// Attach an opener of FIFO pipe `pipe_id` for `access`, counting it as
/// Linux's `fifo_open` counts one, and return its handle and what it must
/// wait for before the open is complete, if anything:
///
/// - for reading, a writer when there is none -- unless `nonblock`, when the
///   handle instead remembers the writer count, so that `poll` sees no
///   hang-up until a writer has come ([`PipeHandle::with_seen`]);
/// - for writing, a reader when there is none; with `nonblock` and no reader,
///   `NoSuchDeviceOrAddress` (`ENXIO`), and nothing attached;
/// - for both, nothing: the opener is its own reader and writer.
///
/// An opener whose side had nobody wakes the other side's waiting opens.
///
/// # Errors
///
/// `InvalidHandle` if the pipe has gone (its last end closed since it was
/// looked up -- the caller makes a new one), or is no FIFO's;
/// `NoSuchDeviceOrAddress` as above; `Overflow` past `u32::MAX` openers.
pub(crate) fn fifo_attach(
    pipe_id: PipeId,
    access: FifoAccess,
    nonblock: bool,
) -> KernelResult<(PipeHandle, Option<FifoPartner>)> {
    let mut table = PIPES.lock();
    let pipe = table.get_mut(&pipe_id).ok_or(KernelError::InvalidHandle)?;
    let (readers, writers) = (pipe.reader_refcount, pipe.writer_refcount);
    let link = pipe.fifo.as_mut().ok_or(KernelError::InvalidHandle)?;
    let (read, write) = match access {
        FifoAccess::Read => (true, false),
        FifoAccess::Write => (false, true),
        FifoAccess::Both => (true, true),
    };
    if write && !read && nonblock && readers == 0 {
        return Err(KernelError::NoSuchDeviceOrAddress);
    }
    let new_readers = if read {
        readers.checked_add(1).ok_or(KernelError::Overflow)?
    } else {
        readers
    };
    let new_writers = if write {
        writers.checked_add(1).ok_or(KernelError::Overflow)?
    } else {
        writers
    };
    if read {
        link.r_counter = link.r_counter.wrapping_add(1);
    }
    if write {
        link.w_counter = link.w_counter.wrapping_add(1);
    }
    let (handle, partner) = match access {
        FifoAccess::Read if writers == 0 && nonblock => (
            PipeHandle::new(pipe_id, PipeEnd::Read).with_seen(link.w_counter),
            None,
        ),
        FifoAccess::Read => (
            PipeHandle::new(pipe_id, PipeEnd::Read),
            (writers == 0).then_some(FifoPartner::Writer(link.w_counter)),
        ),
        FifoAccess::Write => (
            PipeHandle::new(pipe_id, PipeEnd::Write),
            (readers == 0).then_some(FifoPartner::Reader(link.r_counter)),
        ),
        FifoAccess::Both => (PipeHandle::new(pipe_id, PipeEnd::Both), None),
    };
    // An opener on a side that had nobody: the other side's waiting opens
    // may go on (Linux's `wake_up_partner`).
    let wake = if (read && readers == 0) || (write && writers == 0) {
        link.open_waiters.take_all()
    } else {
        Vec::new()
    };
    pipe.reader_refcount = new_readers;
    pipe.writer_refcount = new_writers;
    if read {
        pipe.read_closed = false;
    }
    if write {
        pipe.write_closed = false;
    }
    drop(table);
    wake_all(wake);
    Ok((handle, partner))
}

/// Wait, as an open of a FIFO through `handle`, until the other side has
/// opened since ([`fifo_attach`]'s `partner`). A deliverable signal ends the
/// wait with `Interrupted`: the caller then closes `handle`, undoing the open,
/// as Linux's `fifo_open` does before it answers `-ERESTARTSYS`.
///
/// # Errors
///
/// `Interrupted`; `InvalidHandle` for a pipe that is no FIFO's.
pub(crate) fn fifo_wait_partner(handle: PipeHandle, partner: FifoPartner) -> KernelResult<()> {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        {
            let mut table = PIPES.lock();
            let link = table
                .get_mut(&handle.pipe_id())
                .and_then(|p| p.fifo.as_mut())
                .ok_or(KernelError::InvalidHandle)?;
            link.open_waiters.remove(task);
            let came = match partner {
                FifoPartner::Writer(seen) => link.w_counter != seen,
                FifoPartner::Reader(seen) => link.r_counter != seen,
            };
            if came {
                return Ok(());
            }
            if deliverable_signal_pending(pid) {
                return Err(KernelError::Interrupted);
            }
            link.open_waiters.insert(task);
        }
        park_interruptible(
            pid,
            task,
            crate::wchan::Wait::new(crate::wchan::WaitChannel::Pipe, handle.raw()),
        );
    }
}

/// Take FIFO pipe `pipe_id` away if it has no end open -- made for an open
/// that then failed before attaching (`fifo::open`). Returns the node it was
/// tied to, for the caller to forget with no lock held.
pub(crate) fn fifo_drop_if_unused(pipe_id: PipeId) -> Option<FileId> {
    let link = {
        let mut table = PIPES.lock();
        let unused = table
            .get(&pipe_id)
            .is_some_and(|p| p.fifo.is_some() && p.reader_refcount == 0 && p.writer_refcount == 0);
        if !unused {
            return None;
        }
        table.remove(&pipe_id).and_then(|mut p| p.fifo.take())?
    };
    let id = link.id;
    drop(link);
    Some(id)
}

/// The node of the named pipe `handle` is an end of, held, and the name it
/// was opened by -- `None` for an ordinary pipe. What `fstat` of the end
/// describes and `/proc/<pid>/fd` shows.
#[must_use]
pub fn fifo_node(handle: PipeHandle) -> Option<(Arc<FileHold>, PathBuf)> {
    PIPES
        .lock()
        .get(&handle.pipe_id())
        .and_then(|p| p.fifo.as_ref())
        .map(|link| (Arc::clone(&link.hold), link.path.clone()))
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
    test_multi_waiter_wake()?;

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
    if capacity(rh)? != DEFAULT_BUFFER_CAPACITY || capacity(wh)? != DEFAULT_BUFFER_CAPACITY {
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
    if grown != MAX_PIPE_BUFFER_CAPACITY || capacity(rh)? != MAX_PIPE_BUFFER_CAPACITY {
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
        serial_println!(
            "[pipe]   FAIL: expected EOF after final writer close: n={}",
            n
        );
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
            serial_println!(
                "[pipe]   FAIL: wrong error {:?} (expected ChannelClosed)",
                e
            );
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
static PIPE_TEST_RESULT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

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
        PIPE_TEST_RESULT.store(u32::from(byte), core::sync::atomic::Ordering::SeqCst);
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

/// Number of multi-waiter readers that successfully read one byte.
static PIPE_MULTI_DATA_OK: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Number of multi-waiter readers that observed EOF (`Ok(0)`).
static PIPE_MULTI_EOF_OK: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Multi-waiter task: park on the read end until exactly one byte arrives.
///
/// Both spawned copies block on the *same* read end simultaneously, which is
/// the situation the old single-`Option<TaskId>` waiter slot could not
/// represent: the second parker overwrote the first, and the first was then
/// never woken by anyone.
extern "C" fn pipe_multi_data_task(read_handle_raw: u64) {
    let rh = PipeHandle::from_raw(read_handle_raw);
    // One-byte buffer so each reader consumes exactly one byte and both can
    // be satisfied by a single 2-byte write.
    let mut buf = [0u8; 1];
    if let Ok(1) = read(rh, &mut buf) {
        PIPE_MULTI_DATA_OK.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Multi-waiter task: park on an empty read end until the writer closes (EOF).
extern "C" fn pipe_multi_eof_task(read_handle_raw: u64) {
    let rh = PipeHandle::from_raw(read_handle_raw);
    let mut buf = [0u8; 1];
    if let Ok(0) = read(rh, &mut buf) {
        PIPE_MULTI_EOF_OK.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Test 10: **two** tasks parked on the same pipe end are both woken.
///
/// Regression test for `BUG-PIPE-SINGLE-WAITER-SLOT`: `Pipe` used to hold a
/// single `Option<TaskId>` per end, so a second parking task silently
/// overwrote the first and the overwritten task hung forever.  `dup()`,
/// process spawn and `wait_readable` (the `tee` primitive) all make
/// several-waiters-per-end a normal occurrence, so this must work.
///
/// Two phases, covering both classes of wake:
///
/// 1. **Data wake.** Two readers park on an empty pipe; one 2-byte write must
///    wake *both* (each reads one byte).  With the old code only the last
///    parker was recorded, so exactly one reader would have completed.
/// 2. **EOF broadcast.** Two fresh readers park on an empty pipe; closing the
///    write end must wake both with `Ok(0)`.  EOF is a permanent condition —
///    a waiter missed here can never be woken by anything else.
fn test_multi_waiter_wake() -> KernelResult<()> {
    use core::sync::atomic::Ordering::SeqCst;

    // Give the spawned readers plenty of scheduling opportunities; a single
    // yield only guarantees one of them ran.
    fn yield_a_few() {
        for _ in 0..8 {
            sched::yield_now();
        }
    }

    // --- Phase 1: one write wakes both parked readers. ---
    PIPE_MULTI_DATA_OK.store(0, SeqCst);
    let (rh, wh) = create();
    sched::spawn(b"pipe-multi-a", 16, pipe_multi_data_task, rh.raw(), 0)?;
    sched::spawn(b"pipe-multi-b", 16, pipe_multi_data_task, rh.raw(), 0)?;
    // Let both readers reach the park.
    yield_a_few();
    write(wh, &[7, 9])?;
    yield_a_few();

    let data_ok = PIPE_MULTI_DATA_OK.load(SeqCst);
    if data_ok != 2 {
        serial_println!(
            "[pipe]   FAIL: {} of 2 parked readers woke on data",
            data_ok
        );
        // Close both ends so any still-parked reader is released by EOF
        // rather than being leaked as a permanently blocked task.
        close(wh);
        yield_a_few();
        close(rh);
        return Err(KernelError::InternalError);
    }
    close(wh);
    close(rh);

    // --- Phase 2: writer close (EOF) wakes both parked readers. ---
    PIPE_MULTI_EOF_OK.store(0, SeqCst);
    let (rh2, wh2) = create();
    sched::spawn(b"pipe-multi-c", 16, pipe_multi_eof_task, rh2.raw(), 0)?;
    sched::spawn(b"pipe-multi-d", 16, pipe_multi_eof_task, rh2.raw(), 0)?;
    yield_a_few();
    close(wh2);
    yield_a_few();

    let eof_ok = PIPE_MULTI_EOF_OK.load(SeqCst);
    close(rh2);
    if eof_ok != 2 {
        serial_println!("[pipe]   FAIL: {} of 2 parked readers woke on EOF", eof_ok);
        return Err(KernelError::InternalError);
    }

    serial_println!("[pipe]   Multi-waiter wake (data + EOF): OK");
    Ok(())
}

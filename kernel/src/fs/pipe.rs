//! Named pipe (FIFO) support for the filesystem.
//!
//! Named pipes provide unidirectional byte-stream communication between
//! processes through the filesystem namespace.  A pipe appears as a
//! special entry in the filesystem; multiple writers and readers can
//! open it concurrently.
//!
//! ## Architecture
//!
//! Each named pipe has:
//! - A fixed-size **ring buffer** for data (default 64 KiB)
//! - **Reader** and **writer** reference counts
//! - Non-blocking semantics: read returns `WouldBlock` when empty,
//!   write returns `WouldBlock` when full
//!
//! ## Usage
//!
//! ```text
//! mkfifo /tmp/mypipe         # Create named pipe
//! echo "hello" > /tmp/mypipe  # Write (blocks if no reader in blocking mode)
//! cat /tmp/mypipe             # Read (blocks if no data in blocking mode)
//! ```
//!
//! ## Blocking vs non-blocking
//!
//! The pipe module itself is non-blocking.  Higher-level code (syscall
//! layer, shell) can implement blocking by polling + sleeping.  This
//! keeps the pipe module scheduler-independent.
//!
//! ## Capacity
//!
//! The default pipe capacity is 64 KiB (matching Linux's default for
//! pipes without F_SETPIPE_SZ).  Capacity can be configured per-pipe.
//!
//! ## Reference
//!
//! POSIX: mkfifo(3), pipe(7)
//! Linux: pipe(2), fifo(7), /proc/sys/fs/pipe-max-size

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default pipe buffer capacity in bytes (64 KiB).
pub const DEFAULT_CAPACITY: usize = 65536;

/// Minimum pipe capacity.
pub const MIN_CAPACITY: usize = 4096;

/// Maximum pipe capacity (1 MiB).
pub const MAX_CAPACITY: usize = 1048576;

/// Maximum number of named pipes system-wide.
const MAX_PIPES: usize = 1024;

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

/// A fixed-capacity circular byte buffer.
struct RingBuffer {
    /// Storage.
    buf: Vec<u8>,
    /// Read position (head).
    read_pos: usize,
    /// Write position (tail).
    write_pos: usize,
    /// Number of bytes currently in the buffer.
    len: usize,
}

impl RingBuffer {
    /// Create a new ring buffer with the given capacity.
    fn new(capacity: usize) -> Self {
        let buf = vec![0; capacity];
        Self {
            buf,
            read_pos: 0,
            write_pos: 0,
            len: 0,
        }
    }

    /// Capacity in bytes.
    fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Number of bytes available to read.
    fn available(&self) -> usize {
        self.len
    }

    /// Number of bytes of free space for writing.
    fn free(&self) -> usize {
        self.capacity().saturating_sub(self.len)
    }

    /// Read up to `max_bytes` from the buffer.
    ///
    /// Returns the data read (may be shorter than `max_bytes` if less
    /// data is available).  Returns an empty Vec if the buffer is empty.
    fn read(&mut self, max_bytes: usize) -> Vec<u8> {
        let to_read = max_bytes.min(self.len);
        if to_read == 0 {
            return Vec::new();
        }

        let cap = self.capacity();
        let mut result = Vec::with_capacity(to_read);

        // Handle wrap-around.
        let first_chunk = (cap - self.read_pos).min(to_read);
        result.extend_from_slice(&self.buf[self.read_pos..self.read_pos + first_chunk]);

        let remaining = to_read - first_chunk;
        if remaining > 0 {
            result.extend_from_slice(&self.buf[0..remaining]);
        }

        self.read_pos = (self.read_pos + to_read) % cap;
        self.len -= to_read;

        result
    }

    /// Write data to the buffer.
    ///
    /// Returns the number of bytes actually written (may be less than
    /// `data.len()` if the buffer is full or nearly full).
    fn write(&mut self, data: &[u8]) -> usize {
        let to_write = data.len().min(self.free());
        if to_write == 0 {
            return 0;
        }

        let cap = self.capacity();

        // Handle wrap-around.
        let first_chunk = (cap - self.write_pos).min(to_write);
        self.buf[self.write_pos..self.write_pos + first_chunk]
            .copy_from_slice(&data[..first_chunk]);

        let remaining = to_write - first_chunk;
        if remaining > 0 {
            self.buf[0..remaining].copy_from_slice(&data[first_chunk..first_chunk + remaining]);
        }

        self.write_pos = (self.write_pos + to_write) % cap;
        self.len += to_write;

        to_write
    }

    /// Peek at up to `max_bytes` without consuming them.
    fn peek(&self, max_bytes: usize) -> Vec<u8> {
        let to_peek = max_bytes.min(self.len);
        if to_peek == 0 {
            return Vec::new();
        }

        let cap = self.capacity();
        let mut result = Vec::with_capacity(to_peek);

        let first_chunk = (cap - self.read_pos).min(to_peek);
        result.extend_from_slice(&self.buf[self.read_pos..self.read_pos + first_chunk]);

        let remaining = to_peek - first_chunk;
        if remaining > 0 {
            result.extend_from_slice(&self.buf[0..remaining]);
        }

        result
    }

    /// Discard all data in the buffer.
    fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
        self.len = 0;
    }
}

// ---------------------------------------------------------------------------
// Named pipe
// ---------------------------------------------------------------------------

/// State of a named pipe.
struct NamedPipe {
    /// Filesystem path where this pipe appears.
    path: String,
    /// Ring buffer for pipe data.
    buffer: RingBuffer,
    /// Number of active readers.
    readers: u32,
    /// Number of active writers.
    writers: u32,
    /// Total bytes written since creation.
    bytes_written: u64,
    /// Total bytes read since creation.
    bytes_read: u64,
    /// Total write operations.
    write_ops: u64,
    /// Total read operations.
    read_ops: u64,
    /// Whether the pipe has been closed (all writers gone after some data).
    eof: bool,
}

/// Unique identifier for a pipe.
pub type PipeId = u64;

/// Handle to one end of a named pipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipeHandle {
    /// Which pipe this handle refers to.
    pub pipe_id: PipeId,
    /// Whether this is a reader or writer handle.
    pub mode: PipeMode,
}

/// Whether a handle is for reading or writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeMode {
    Read,
    Write,
    ReadWrite,
}

/// Summary info about a pipe.
#[derive(Debug, Clone)]
pub struct PipeInfo {
    pub path: String,
    pub capacity: usize,
    pub buffered: usize,
    pub readers: u32,
    pub writers: u32,
    pub bytes_written: u64,
    pub bytes_read: u64,
    pub write_ops: u64,
    pub read_ops: u64,
    pub eof: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct PipeInner {
    /// All named pipes, keyed by ID.
    pipes: BTreeMap<PipeId, NamedPipe>,
    /// Path → pipe ID lookup.
    path_index: BTreeMap<String, PipeId>,
    /// Next ID.
    next_id: PipeId,
}

static PIPES: Mutex<PipeInner> = Mutex::new(PipeInner {
    pipes: BTreeMap::new(),
    path_index: BTreeMap::new(),
    next_id: 1,
});

// ---------------------------------------------------------------------------
// Public API — lifecycle
// ---------------------------------------------------------------------------

/// Create a named pipe at the given filesystem path.
///
/// The pipe is not stored on any real filesystem — it exists only in
/// the kernel's pipe table.  The path is used for lookup only.
pub fn mkfifo(path: &str) -> KernelResult<PipeId> {
    mkfifo_with_capacity(path, DEFAULT_CAPACITY)
}

/// Create a named pipe with a specific buffer capacity.
pub fn mkfifo_with_capacity(path: &str, capacity: usize) -> KernelResult<PipeId> {
    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let cap = capacity.clamp(MIN_CAPACITY, MAX_CAPACITY);

    let mut inner = PIPES.lock();

    if inner.pipes.len() >= MAX_PIPES {
        return Err(KernelError::ResourceExhausted);
    }

    if inner.path_index.contains_key(path) {
        return Err(KernelError::AlreadyExists);
    }

    let id = inner.next_id;
    inner.next_id = inner.next_id.wrapping_add(1);

    inner.pipes.insert(id, NamedPipe {
        path: path.into(),
        buffer: RingBuffer::new(cap),
        readers: 0,
        writers: 0,
        bytes_written: 0,
        bytes_read: 0,
        write_ops: 0,
        read_ops: 0,
        eof: false,
    });
    inner.path_index.insert(path.into(), id);

    Ok(id)
}

/// Remove a named pipe.
///
/// Fails if the pipe has active readers or writers.
pub fn unlink(path: &str) -> KernelResult<()> {
    let mut inner = PIPES.lock();

    let id = *inner.path_index.get(path).ok_or(KernelError::NotFound)?;
    let pipe = inner.pipes.get(&id).ok_or(KernelError::NotFound)?;

    if pipe.readers > 0 || pipe.writers > 0 {
        return Err(KernelError::DeviceBusy);
    }

    inner.pipes.remove(&id);
    inner.path_index.remove(path);

    Ok(())
}

/// Force-remove a named pipe even if handles are open.
pub fn unlink_force(path: &str) -> KernelResult<()> {
    let mut inner = PIPES.lock();

    let id = *inner.path_index.get(path).ok_or(KernelError::NotFound)?;
    inner.pipes.remove(&id);
    inner.path_index.remove(path);

    Ok(())
}

/// Open a pipe for reading, writing, or both.
///
/// Returns a handle that must be closed when done.
pub fn open(path: &str, mode: PipeMode) -> KernelResult<PipeHandle> {
    let mut inner = PIPES.lock();

    let id = *inner.path_index.get(path).ok_or(KernelError::NotFound)?;
    let pipe = inner.pipes.get_mut(&id).ok_or(KernelError::NotFound)?;

    match mode {
        PipeMode::Read => pipe.readers = pipe.readers.saturating_add(1),
        PipeMode::Write => {
            pipe.writers = pipe.writers.saturating_add(1);
            // New writer clears EOF.
            pipe.eof = false;
        }
        PipeMode::ReadWrite => {
            pipe.readers = pipe.readers.saturating_add(1);
            pipe.writers = pipe.writers.saturating_add(1);
            pipe.eof = false;
        }
    }

    Ok(PipeHandle { pipe_id: id, mode })
}

/// Close a pipe handle.
pub fn close(handle: PipeHandle) -> KernelResult<()> {
    let mut inner = PIPES.lock();

    let pipe = inner.pipes.get_mut(&handle.pipe_id)
        .ok_or(KernelError::NotFound)?;

    match handle.mode {
        PipeMode::Read => {
            pipe.readers = pipe.readers.saturating_sub(1);
        }
        PipeMode::Write => {
            pipe.writers = pipe.writers.saturating_sub(1);
            // If last writer closed, signal EOF.
            if pipe.writers == 0 {
                pipe.eof = true;
            }
        }
        PipeMode::ReadWrite => {
            pipe.readers = pipe.readers.saturating_sub(1);
            pipe.writers = pipe.writers.saturating_sub(1);
            if pipe.writers == 0 {
                pipe.eof = true;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API — I/O
// ---------------------------------------------------------------------------

/// Read from a pipe (non-blocking).
///
/// Returns data available in the buffer (up to `max_bytes`).
/// Returns `WouldBlock` if the buffer is empty and writers still exist.
/// Returns empty `Vec` (Ok) if EOF (all writers closed and buffer empty).
pub fn read(handle: &PipeHandle, max_bytes: usize) -> KernelResult<Vec<u8>> {
    if handle.mode == PipeMode::Write {
        return Err(KernelError::InvalidArgument);
    }

    let mut inner = PIPES.lock();
    let pipe = inner.pipes.get_mut(&handle.pipe_id)
        .ok_or(KernelError::NotFound)?;

    let data = pipe.buffer.read(max_bytes);

    if data.is_empty() {
        if pipe.eof || pipe.writers == 0 {
            // EOF — all writers gone and buffer drained.
            return Ok(Vec::new());
        }
        // Buffer empty but writers still exist — would block.
        return Err(KernelError::WouldBlock);
    }

    let len = data.len() as u64;
    pipe.bytes_read = pipe.bytes_read.saturating_add(len);
    pipe.read_ops = pipe.read_ops.saturating_add(1);

    Ok(data)
}

/// Write to a pipe (non-blocking).
///
/// Returns the number of bytes actually written.  May be less than
/// `data.len()` if the buffer is full.  Returns `WouldBlock` if the
/// buffer is completely full.  Returns `ChannelClosed` if there are
/// no readers (broken pipe).
pub fn write(handle: &PipeHandle, data: &[u8]) -> KernelResult<usize> {
    if handle.mode == PipeMode::Read {
        return Err(KernelError::InvalidArgument);
    }

    if data.is_empty() {
        return Ok(0);
    }

    let mut inner = PIPES.lock();
    let pipe = inner.pipes.get_mut(&handle.pipe_id)
        .ok_or(KernelError::NotFound)?;

    // Broken pipe: no readers.
    if pipe.readers == 0 {
        return Err(KernelError::ChannelClosed);
    }

    let written = pipe.buffer.write(data);

    if written == 0 {
        return Err(KernelError::WouldBlock);
    }

    pipe.bytes_written = pipe.bytes_written.saturating_add(written as u64);
    pipe.write_ops = pipe.write_ops.saturating_add(1);

    Ok(written)
}

/// Write all data to a pipe, returning `WouldBlock` if not all data
/// could be written (call again later with remaining data).
///
/// Returns the number of bytes written.
pub fn write_all(handle: &PipeHandle, data: &[u8]) -> KernelResult<usize> {
    write(handle, data)
}

/// Peek at data in the pipe without consuming it.
pub fn peek(handle: &PipeHandle, max_bytes: usize) -> KernelResult<Vec<u8>> {
    if handle.mode == PipeMode::Write {
        return Err(KernelError::InvalidArgument);
    }

    let inner = PIPES.lock();
    let pipe = inner.pipes.get(&handle.pipe_id)
        .ok_or(KernelError::NotFound)?;

    Ok(pipe.buffer.peek(max_bytes))
}

// ---------------------------------------------------------------------------
// Public API — query
// ---------------------------------------------------------------------------

/// Get info about a pipe by path.
pub fn info(path: &str) -> KernelResult<PipeInfo> {
    let inner = PIPES.lock();
    let id = *inner.path_index.get(path).ok_or(KernelError::NotFound)?;
    let pipe = inner.pipes.get(&id).ok_or(KernelError::NotFound)?;

    Ok(PipeInfo {
        path: pipe.path.clone(),
        capacity: pipe.buffer.capacity(),
        buffered: pipe.buffer.available(),
        readers: pipe.readers,
        writers: pipe.writers,
        bytes_written: pipe.bytes_written,
        bytes_read: pipe.bytes_read,
        write_ops: pipe.write_ops,
        read_ops: pipe.read_ops,
        eof: pipe.eof,
    })
}

/// Get info about a pipe by ID.
pub fn info_by_id(id: PipeId) -> KernelResult<PipeInfo> {
    let inner = PIPES.lock();
    let pipe = inner.pipes.get(&id).ok_or(KernelError::NotFound)?;

    Ok(PipeInfo {
        path: pipe.path.clone(),
        capacity: pipe.buffer.capacity(),
        buffered: pipe.buffer.available(),
        readers: pipe.readers,
        writers: pipe.writers,
        bytes_written: pipe.bytes_written,
        bytes_read: pipe.bytes_read,
        write_ops: pipe.write_ops,
        read_ops: pipe.read_ops,
        eof: pipe.eof,
    })
}

/// Check if a path is a named pipe.
pub fn is_pipe(path: &str) -> bool {
    PIPES.lock().path_index.contains_key(path)
}

/// Find a pipe ID by path.
pub fn find(path: &str) -> Option<PipeId> {
    PIPES.lock().path_index.get(path).copied()
}

/// List all named pipes.
pub fn list() -> Vec<PipeInfo> {
    let inner = PIPES.lock();
    inner.pipes.values().map(|p| PipeInfo {
        path: p.path.clone(),
        capacity: p.buffer.capacity(),
        buffered: p.buffer.available(),
        readers: p.readers,
        writers: p.writers,
        bytes_written: p.bytes_written,
        bytes_read: p.bytes_read,
        write_ops: p.write_ops,
        read_ops: p.read_ops,
        eof: p.eof,
    }).collect()
}

/// Get the total number of active pipes.
pub fn count() -> usize {
    PIPES.lock().pipes.len()
}

/// Get buffered byte count for a pipe.
pub fn buffered(path: &str) -> KernelResult<usize> {
    let inner = PIPES.lock();
    let id = *inner.path_index.get(path).ok_or(KernelError::NotFound)?;
    let pipe = inner.pipes.get(&id).ok_or(KernelError::NotFound)?;
    Ok(pipe.buffer.available())
}

/// Flush (discard) all buffered data in a pipe.
pub fn flush(path: &str) -> KernelResult<()> {
    let mut inner = PIPES.lock();
    let id = *inner.path_index.get(path).ok_or(KernelError::NotFound)?;
    let pipe = inner.pipes.get_mut(&id).ok_or(KernelError::NotFound)?;
    pipe.buffer.clear();
    Ok(())
}

// ---------------------------------------------------------------------------
// Anonymous pipes (for shell | operator)
// ---------------------------------------------------------------------------

/// Create an anonymous pipe (not in the filesystem namespace).
///
/// Returns (read_handle, write_handle).
pub fn anonymous_pipe() -> KernelResult<(PipeHandle, PipeHandle)> {
    anonymous_pipe_with_capacity(DEFAULT_CAPACITY)
}

/// Create an anonymous pipe with a specific capacity.
pub fn anonymous_pipe_with_capacity(capacity: usize) -> KernelResult<(PipeHandle, PipeHandle)> {
    let cap = capacity.clamp(MIN_CAPACITY, MAX_CAPACITY);

    let mut inner = PIPES.lock();

    if inner.pipes.len() >= MAX_PIPES {
        return Err(KernelError::ResourceExhausted);
    }

    let id = inner.next_id;
    inner.next_id = inner.next_id.wrapping_add(1);

    // Anonymous pipes get a synthetic path for debugging.
    let path: String = alloc::format!("<pipe:{}>", id);

    inner.pipes.insert(id, NamedPipe {
        path: path.clone(),
        buffer: RingBuffer::new(cap),
        readers: 1,
        writers: 1,
        bytes_written: 0,
        bytes_read: 0,
        write_ops: 0,
        read_ops: 0,
        eof: false,
    });
    // Don't index anonymous pipes by path.

    let read_handle = PipeHandle { pipe_id: id, mode: PipeMode::Read };
    let write_handle = PipeHandle { pipe_id: id, mode: PipeMode::Write };

    Ok((read_handle, write_handle))
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for the named pipe module.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[pipe] Running self-test...");

    // --- Test 1: Create and destroy ---
    {
        let id = mkfifo("/tmp/test_pipe_1")?;
        if !is_pipe("/tmp/test_pipe_1") {
            serial_println!("[pipe]   ERROR: pipe not found after create");
            let _ = unlink_force("/tmp/test_pipe_1");
            return Err(KernelError::InternalError);
        }
        // No readers/writers, should be able to unlink.
        unlink("/tmp/test_pipe_1")?;
        if is_pipe("/tmp/test_pipe_1") {
            serial_println!("[pipe]   ERROR: pipe found after unlink");
            return Err(KernelError::InternalError);
        }
        let _ = id;  // suppress warning
        serial_println!("[pipe]   create + unlink: OK");
    }

    // --- Test 2: Duplicate creation rejected ---
    {
        let _id = mkfifo("/tmp/test_pipe_2")?;
        let result = mkfifo("/tmp/test_pipe_2");
        if result.is_ok() {
            serial_println!("[pipe]   ERROR: duplicate create allowed");
            let _ = unlink_force("/tmp/test_pipe_2");
            return Err(KernelError::InternalError);
        }
        let _ = unlink_force("/tmp/test_pipe_2");
        serial_println!("[pipe]   duplicate rejection: OK");
    }

    // --- Test 3: Basic write + read ---
    {
        let _id = mkfifo("/tmp/test_pipe_3")?;
        let wh = open("/tmp/test_pipe_3", PipeMode::Write)?;
        let rh = open("/tmp/test_pipe_3", PipeMode::Read)?;

        let written = write(&wh, b"hello pipe")?;
        if written != 10 {
            serial_println!("[pipe]   ERROR: write returned {}", written);
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_3");
            return Err(KernelError::InternalError);
        }

        let data = read(&rh, 1024)?;
        if data != b"hello pipe" {
            serial_println!("[pipe]   ERROR: read mismatch");
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_3");
            return Err(KernelError::InternalError);
        }

        close(rh)?;
        close(wh)?;
        let _ = unlink_force("/tmp/test_pipe_3");
        serial_println!("[pipe]   write + read: OK");
    }

    // --- Test 4: Partial read ---
    {
        let _id = mkfifo("/tmp/test_pipe_4")?;
        let wh = open("/tmp/test_pipe_4", PipeMode::Write)?;
        let rh = open("/tmp/test_pipe_4", PipeMode::Read)?;

        write(&wh, b"abcdefghij")?;

        // Read only 5 bytes.
        let data1 = read(&rh, 5)?;
        if data1 != b"abcde" {
            serial_println!("[pipe]   ERROR: partial read 1 mismatch");
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_4");
            return Err(KernelError::InternalError);
        }

        // Read remaining.
        let data2 = read(&rh, 1024)?;
        if data2 != b"fghij" {
            serial_println!("[pipe]   ERROR: partial read 2 mismatch");
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_4");
            return Err(KernelError::InternalError);
        }

        close(rh)?;
        close(wh)?;
        let _ = unlink_force("/tmp/test_pipe_4");
        serial_println!("[pipe]   partial read: OK");
    }

    // --- Test 5: WouldBlock when empty (writers exist) ---
    {
        let _id = mkfifo("/tmp/test_pipe_5")?;
        let wh = open("/tmp/test_pipe_5", PipeMode::Write)?;
        let rh = open("/tmp/test_pipe_5", PipeMode::Read)?;

        let result = read(&rh, 1024);
        match result {
            Err(KernelError::WouldBlock) => { /* correct */ }
            _ => {
                serial_println!("[pipe]   ERROR: expected WouldBlock, got {:?}", result);
                close(rh)?;
                close(wh)?;
                let _ = unlink_force("/tmp/test_pipe_5");
                return Err(KernelError::InternalError);
            }
        }

        close(rh)?;
        close(wh)?;
        let _ = unlink_force("/tmp/test_pipe_5");
        serial_println!("[pipe]   WouldBlock on empty: OK");
    }

    // --- Test 6: EOF when all writers close ---
    {
        let _id = mkfifo("/tmp/test_pipe_6")?;
        let wh = open("/tmp/test_pipe_6", PipeMode::Write)?;
        let rh = open("/tmp/test_pipe_6", PipeMode::Read)?;

        write(&wh, b"final")?;
        close(wh)?;  // Last writer closes.

        // Should still be able to read buffered data.
        let data = read(&rh, 1024)?;
        if data != b"final" {
            serial_println!("[pipe]   ERROR: EOF buffered read failed");
            close(rh)?;
            let _ = unlink_force("/tmp/test_pipe_6");
            return Err(KernelError::InternalError);
        }

        // Next read should return empty (EOF).
        let data2 = read(&rh, 1024)?;
        if !data2.is_empty() {
            serial_println!("[pipe]   ERROR: expected EOF, got {} bytes", data2.len());
            close(rh)?;
            let _ = unlink_force("/tmp/test_pipe_6");
            return Err(KernelError::InternalError);
        }

        close(rh)?;
        let _ = unlink_force("/tmp/test_pipe_6");
        serial_println!("[pipe]   EOF on writer close: OK");
    }

    // --- Test 7: Broken pipe (write with no readers) ---
    {
        let _id = mkfifo("/tmp/test_pipe_7")?;
        let wh = open("/tmp/test_pipe_7", PipeMode::Write)?;

        // No readers opened — write should get ChannelClosed.
        let result = write(&wh, b"broken");
        match result {
            Err(KernelError::ChannelClosed) => { /* correct */ }
            _ => {
                serial_println!("[pipe]   ERROR: expected ChannelClosed, got {:?}", result);
                close(wh)?;
                let _ = unlink_force("/tmp/test_pipe_7");
                return Err(KernelError::InternalError);
            }
        }

        close(wh)?;
        let _ = unlink_force("/tmp/test_pipe_7");
        serial_println!("[pipe]   broken pipe: OK");
    }

    // --- Test 8: Ring buffer wrap-around ---
    {
        let _id = mkfifo_with_capacity("/tmp/test_pipe_8", MIN_CAPACITY)?;
        let wh = open("/tmp/test_pipe_8", PipeMode::Write)?;
        let rh = open("/tmp/test_pipe_8", PipeMode::Read)?;

        // Fill most of the buffer, read it, then write more to force wrap.
        let chunk = vec![0xABu8; MIN_CAPACITY - 100];
        write(&wh, &chunk)?;
        let _ = read(&rh, MIN_CAPACITY);

        // Now write across the wrap boundary.
        let wrap_data = vec![0xCDu8; 200];
        let written = write(&wh, &wrap_data)?;
        if written != 200 {
            serial_println!("[pipe]   ERROR: wrap write returned {}", written);
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_8");
            return Err(KernelError::InternalError);
        }

        let read_back = read(&rh, 200)?;
        if read_back.len() != 200 || read_back.iter().any(|&b| b != 0xCD) {
            serial_println!("[pipe]   ERROR: wrap-around data mismatch");
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_8");
            return Err(KernelError::InternalError);
        }

        close(rh)?;
        close(wh)?;
        let _ = unlink_force("/tmp/test_pipe_8");
        serial_println!("[pipe]   ring buffer wrap-around: OK");
    }

    // --- Test 9: Anonymous pipe ---
    {
        let (rh, wh) = anonymous_pipe()?;

        write(&wh, b"anonymous")?;
        let data = read(&rh, 1024)?;
        if data != b"anonymous" {
            serial_println!("[pipe]   ERROR: anonymous pipe mismatch");
            close(rh)?;
            close(wh)?;
            return Err(KernelError::InternalError);
        }

        close(wh)?;
        // After writer closes, should get EOF.
        let eof_data = read(&rh, 1024)?;
        if !eof_data.is_empty() {
            serial_println!("[pipe]   ERROR: anon pipe not EOF");
            close(rh)?;
            return Err(KernelError::InternalError);
        }

        close(rh)?;
        serial_println!("[pipe]   anonymous pipe: OK");
    }

    // --- Test 10: Peek does not consume ---
    {
        let _id = mkfifo("/tmp/test_pipe_10")?;
        let wh = open("/tmp/test_pipe_10", PipeMode::Write)?;
        let rh = open("/tmp/test_pipe_10", PipeMode::Read)?;

        write(&wh, b"peekaboo")?;

        let peeked = peek(&rh, 4)?;
        if peeked != b"peek" {
            serial_println!("[pipe]   ERROR: peek mismatch");
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_10");
            return Err(KernelError::InternalError);
        }

        // Full data should still be available.
        let full = read(&rh, 1024)?;
        if full != b"peekaboo" {
            serial_println!("[pipe]   ERROR: data consumed by peek");
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_10");
            return Err(KernelError::InternalError);
        }

        close(rh)?;
        close(wh)?;
        let _ = unlink_force("/tmp/test_pipe_10");
        serial_println!("[pipe]   peek: OK");
    }

    // --- Test 11: Stats tracking ---
    {
        let _id = mkfifo("/tmp/test_pipe_11")?;
        let wh = open("/tmp/test_pipe_11", PipeMode::Write)?;
        let rh = open("/tmp/test_pipe_11", PipeMode::Read)?;

        write(&wh, b"stats1")?;
        write(&wh, b"stats2")?;
        let _ = read(&rh, 1024);

        let pi = info("/tmp/test_pipe_11")?;
        if pi.write_ops != 2 || pi.read_ops != 1 {
            serial_println!("[pipe]   ERROR: stats wrong (wops={} rops={})",
                pi.write_ops, pi.read_ops);
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_11");
            return Err(KernelError::InternalError);
        }
        if pi.bytes_written != 12 {
            serial_println!("[pipe]   ERROR: bytes_written={}", pi.bytes_written);
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_11");
            return Err(KernelError::InternalError);
        }

        close(rh)?;
        close(wh)?;
        let _ = unlink_force("/tmp/test_pipe_11");
        serial_println!("[pipe]   stats tracking: OK");
    }

    // --- Test 12: Capacity limit ---
    {
        let cap = MIN_CAPACITY;
        let _id = mkfifo_with_capacity("/tmp/test_pipe_12", cap)?;
        let wh = open("/tmp/test_pipe_12", PipeMode::Write)?;
        let rh = open("/tmp/test_pipe_12", PipeMode::Read)?;

        // Fill the buffer completely.
        let fill = vec![0xFFu8; cap];
        let written = write(&wh, &fill)?;
        if written != cap {
            serial_println!("[pipe]   ERROR: fill wrote {} instead of {}", written, cap);
            close(rh)?;
            close(wh)?;
            let _ = unlink_force("/tmp/test_pipe_12");
            return Err(KernelError::InternalError);
        }

        // Writing more should return WouldBlock.
        let result = write(&wh, b"overflow");
        match result {
            Err(KernelError::WouldBlock) => { /* correct */ }
            _ => {
                serial_println!("[pipe]   ERROR: expected WouldBlock on full, got {:?}", result);
                close(rh)?;
                close(wh)?;
                let _ = unlink_force("/tmp/test_pipe_12");
                return Err(KernelError::InternalError);
            }
        }

        close(rh)?;
        close(wh)?;
        let _ = unlink_force("/tmp/test_pipe_12");
        serial_println!("[pipe]   capacity limit: OK");
    }

    serial_println!("[pipe] Self-test passed (12 tests).");
    Ok(())
}

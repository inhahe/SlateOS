//! Pseudo-terminal (PTY) abstraction for OurOS.
//!
//! This module implements a PTY layer that connects the graphical terminal
//! emulator to child processes. Unlike Unix, OurOS does not have kernel-level
//! PTY devices. Instead, we build the PTY abstraction in userspace using
//! bidirectional byte channels (the OS's primary IPC primitive).
//!
//! ## Architecture
//!
//! A PTY pair consists of a **master** (terminal emulator side) and a
//! **slave** (child process side), connected by two unidirectional byte
//! channels:
//!
//! ```text
//!  Master ──write──▶ [channel A] ──read──▶ Slave
//!  Master ◀──read─── [channel B] ◀──write── Slave
//! ```
//!
//! The slave side optionally applies a **line discipline** (cooked mode)
//! that buffers input, echoes characters, and translates control keys
//! into signals. In raw mode, bytes pass through unmodified.
//!
//! ## Signals
//!
//! Terminal control characters (Ctrl+C, Ctrl+D, etc.) are translated
//! into `PtySignal` values by the cooked-mode processor. The terminal
//! emulator can then deliver these to the child process via the OS's
//! process control IPC messages (not Unix signals).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during PTY operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyError {
    /// The PTY channel has been closed by the peer.
    Closed,
    /// A non-blocking read found no data available.
    WouldBlock,
    /// The internal buffer is full and cannot accept more data.
    BufferFull,
    /// The PTY ID does not refer to a registered PTY.
    NotFound,
    /// The maximum number of PTYs has been reached.
    TooManyPtys,
    /// The child process could not be spawned.
    SpawnFailed(String),
    /// An invalid argument was provided.
    InvalidArgument(String),
    /// The PTY manager lock is poisoned (mutex was held during a panic).
    LockPoisoned,
}

impl core::fmt::Display for PtyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Closed => write!(f, "PTY channel closed"),
            Self::WouldBlock => write!(f, "operation would block"),
            Self::BufferFull => write!(f, "PTY buffer full"),
            Self::NotFound => write!(f, "PTY not found"),
            Self::TooManyPtys => write!(f, "maximum number of PTYs reached"),
            Self::SpawnFailed(msg) => write!(f, "spawn failed: {msg}"),
            Self::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
            Self::LockPoisoned => write!(f, "PTY lock poisoned"),
        }
    }
}

impl std::error::Error for PtyError {}

/// PTY result type.
pub type PtyResult<T> = Result<T, PtyError>;

// ---------------------------------------------------------------------------
// Internal byte channel
// ---------------------------------------------------------------------------

/// Default capacity of the internal byte buffer (64 KiB, matching the
/// kernel pipe buffer size).
const DEFAULT_CHANNEL_CAPACITY: usize = 64 * 1024;

/// A unidirectional byte channel connecting one side of the PTY to the
/// other. This is the userspace equivalent of a kernel pipe -- a ring
/// buffer with read/write cursors.
struct ByteChannel {
    /// Circular buffer storage.
    buf: Vec<u8>,
    /// Index of the next byte to read.
    head: usize,
    /// Index of the next byte to write.
    tail: usize,
    /// Number of bytes currently in the buffer.
    len: usize,
    /// Whether the write end has been closed.
    write_closed: bool,
    /// Whether the read end has been closed.
    read_closed: bool,
}

impl ByteChannel {
    /// Create a new byte channel with the given capacity.
    fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            head: 0,
            tail: 0,
            len: 0,
            write_closed: false,
            read_closed: false,
        }
    }

    /// Write bytes into the channel. Returns the number of bytes written.
    fn write(&mut self, data: &[u8]) -> PtyResult<usize> {
        if self.read_closed {
            return Err(PtyError::Closed);
        }
        if self.write_closed {
            return Err(PtyError::Closed);
        }

        let capacity = self.buf.len();
        let available = capacity.saturating_sub(self.len);
        if available == 0 {
            return Err(PtyError::BufferFull);
        }

        let to_write = data.len().min(available);
        for i in 0..to_write {
            if let (Some(dst), Some(src)) = (self.buf.get_mut(self.tail), data.get(i)) {
                *dst = *src;
            }
            self.tail = (self.tail + 1) % capacity;
        }
        self.len += to_write;
        Ok(to_write)
    }

    /// Read bytes from the channel. Returns the number of bytes read.
    fn read(&mut self, out: &mut [u8]) -> PtyResult<usize> {
        if self.len == 0 {
            if self.write_closed {
                return Ok(0); // EOF
            }
            return Err(PtyError::WouldBlock);
        }

        let capacity = self.buf.len();
        let to_read = out.len().min(self.len);
        for i in 0..to_read {
            if let (Some(dst), Some(src)) = (out.get_mut(i), self.buf.get(self.head)) {
                *dst = *src;
            }
            self.head = (self.head + 1) % capacity;
        }
        self.len -= to_read;
        Ok(to_read)
    }

    /// Check how many bytes are available to read.
    fn available(&self) -> usize {
        self.len
    }

    /// Close the write end. Future reads will drain then return 0 (EOF).
    fn close_write(&mut self) {
        self.write_closed = true;
    }

    /// Close the read end. Future writes will fail with Closed.
    fn close_read(&mut self) {
        self.read_closed = true;
    }

    /// Check whether the write end is closed.
    fn is_write_closed(&self) -> bool {
        self.write_closed
    }
}

// ---------------------------------------------------------------------------
// PTY signals
// ---------------------------------------------------------------------------

/// Process control signals generated by terminal control characters.
///
/// These are delivered to the child process via IPC messages (not Unix
/// signals). The terminal emulator translates control key combinations
/// into these signal values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtySignal {
    /// Interrupt the foreground process (Ctrl+C).
    Interrupt,
    /// Quit with core dump (Ctrl+\).
    Quit,
    /// Suspend the foreground process (Ctrl+Z).
    Suspend,
    /// End of input (Ctrl+D).
    Eof,
}

// ---------------------------------------------------------------------------
// Terminal mode / line discipline
// ---------------------------------------------------------------------------

/// Terminal mode controlling how input is processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyTerminalMode {
    /// Raw mode: bytes pass through unmodified. No echo, no line
    /// buffering, no signal generation.
    Raw,
    /// Cooked mode: line-buffered with echo and signal generation.
    /// Backspace edits the line buffer, Enter flushes it, and control
    /// characters generate signals.
    Cooked,
}

/// Actions produced by the cooked-mode line discipline when processing
/// a single input byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookedAction {
    /// The byte was added to the line buffer; no output yet.
    Buffer,
    /// A complete line is ready to be delivered to the slave.
    FlushLine(Vec<u8>),
    /// A control character generated a signal.
    Signal(PtySignal),
    /// A byte should be echoed back to the terminal display.
    Echo(u8),
}

/// Process a single input byte in cooked mode.
///
/// Updates `line_buf` and returns the action the caller should take.
/// This implements a basic line discipline: line buffering, backspace
/// editing, and control character translation.
pub fn cooked_process(input: u8, line_buf: &mut Vec<u8>) -> CookedAction {
    match input {
        // Ctrl+C → Interrupt
        0x03 => {
            line_buf.clear();
            CookedAction::Signal(PtySignal::Interrupt)
        }
        // Ctrl+D → EOF (only when line buffer is empty sends EOF,
        // otherwise flush what we have)
        0x04 => {
            if line_buf.is_empty() {
                CookedAction::Signal(PtySignal::Eof)
            } else {
                let flushed = line_buf.drain(..).collect();
                CookedAction::FlushLine(flushed)
            }
        }
        // Ctrl+Z → Suspend
        0x1A => {
            line_buf.clear();
            CookedAction::Signal(PtySignal::Suspend)
        }
        // Ctrl+\ → Quit
        0x1C => {
            line_buf.clear();
            CookedAction::Signal(PtySignal::Quit)
        }
        // Backspace or DEL → erase last character
        0x08 | 0x7F => {
            if line_buf.pop().is_some() {
                CookedAction::Echo(0x08) // echo backspace
            } else {
                CookedAction::Buffer // nothing to erase
            }
        }
        // Carriage return or newline → flush line
        0x0A | 0x0D => {
            line_buf.push(b'\n');
            let flushed = line_buf.drain(..).collect();
            CookedAction::Echo(b'\n')
                // The caller must handle both the echo and the flush.
                // We return FlushLine which is the primary action; the
                // echo of newline is implicit.
                ;
            CookedAction::FlushLine(flushed)
        }
        // Printable or other byte → buffer and echo
        _ => {
            line_buf.push(input);
            CookedAction::Echo(input)
        }
    }
}

// ---------------------------------------------------------------------------
// Window size
// ---------------------------------------------------------------------------

/// Terminal window size (columns and rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinSize {
    pub cols: u16,
    pub rows: u16,
}

impl WinSize {
    /// Create a new window size.
    pub fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows }
    }
}

impl Default for WinSize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

// ---------------------------------------------------------------------------
// PTY shared state
// ---------------------------------------------------------------------------

/// Shared state for a PTY pair, protected by a mutex.
///
/// Both the master and slave hold `Arc<Mutex<PtyInner>>` to access the
/// same underlying channels, mode, and window size.
struct PtyInner {
    /// Channel from master to slave (keyboard input → child stdin).
    master_to_slave: ByteChannel,
    /// Channel from slave to master (child stdout → terminal display).
    slave_to_master: ByteChannel,
    /// Current terminal mode.
    mode: PtyTerminalMode,
    /// Current window size.
    winsize: WinSize,
    /// Line buffer for cooked mode input processing.
    line_buf: Vec<u8>,
    /// Whether the master side has been closed.
    master_closed: bool,
    /// Whether the slave side has been closed.
    slave_closed: bool,
}

impl PtyInner {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            master_to_slave: ByteChannel::new(DEFAULT_CHANNEL_CAPACITY),
            slave_to_master: ByteChannel::new(DEFAULT_CHANNEL_CAPACITY),
            mode: PtyTerminalMode::Cooked,
            winsize: WinSize::new(cols, rows),
            line_buf: Vec::with_capacity(256),
            master_closed: false,
            slave_closed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// PtyMaster
// ---------------------------------------------------------------------------

/// The master (terminal emulator) side of a PTY.
///
/// The terminal emulator writes keyboard input here and reads child
/// process output from here.
pub struct PtyMaster {
    inner: Arc<Mutex<PtyInner>>,
}

impl PtyMaster {
    /// Write input data (from keyboard) to the child process.
    ///
    /// In cooked mode, the data passes through the line discipline
    /// which may buffer it, echo characters, or generate signals.
    /// In raw mode, the data is passed through directly.
    ///
    /// Returns the number of bytes consumed from `data`.
    pub fn write(&self, data: &[u8]) -> PtyResult<usize> {
        let mut inner = self.lock_inner()?;
        if inner.master_closed {
            return Err(PtyError::Closed);
        }
        if inner.slave_closed {
            return Err(PtyError::Closed);
        }

        match inner.mode {
            PtyTerminalMode::Raw => inner.master_to_slave.write(data),
            PtyTerminalMode::Cooked => {
                // In cooked mode, process each byte through the line
                // discipline. We consume all input bytes even if the
                // underlying channel cannot accept more yet -- the line
                // buffer absorbs them.
                let mut consumed = 0;
                for &byte in data {
                    let action = cooked_process(byte, &mut inner.line_buf);
                    match action {
                        CookedAction::FlushLine(line) => {
                            // Best-effort write of the completed line.
                            // If the channel is full we drop the data
                            // (real implementation would block or buffer).
                            let _ = inner.master_to_slave.write(&line);
                        }
                        CookedAction::Echo(echo_byte) => {
                            // Echo the byte back to the master's read side
                            // so the terminal emulator can display it.
                            let _ = inner.slave_to_master.write(&[echo_byte]);
                        }
                        CookedAction::Signal(_) | CookedAction::Buffer => {
                            // Signals are stored in the return value for
                            // callers who use write_cooked(). Buffer means
                            // the byte was accumulated -- nothing to do.
                        }
                    }
                    consumed += 1;
                }
                Ok(consumed)
            }
        }
    }

    /// Read output from the child process.
    ///
    /// Blocks conceptually until data is available. In this userspace
    /// implementation, returns `WouldBlock` when no data is ready (the
    /// caller should retry or poll).
    ///
    /// Returns 0 when the slave has closed and all buffered data has
    /// been drained (EOF).
    pub fn read(&self, buf: &mut [u8]) -> PtyResult<usize> {
        let mut inner = self.lock_inner()?;
        if inner.master_closed {
            return Err(PtyError::Closed);
        }
        inner.slave_to_master.read(buf)
    }

    /// Non-blocking read. Returns `Ok(None)` if no data is available.
    pub fn try_read(&self, buf: &mut [u8]) -> PtyResult<Option<usize>> {
        let mut inner = self.lock_inner()?;
        if inner.master_closed {
            return Err(PtyError::Closed);
        }
        match inner.slave_to_master.read(buf) {
            Ok(n) => Ok(Some(n)),
            Err(PtyError::WouldBlock) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Notify the slave of a terminal resize.
    ///
    /// This is the equivalent of `TIOCSWINSZ` on Unix. The child
    /// process can query the new size via `PtySlave::get_size()`.
    pub fn resize(&self, cols: u16, rows: u16) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.winsize = WinSize::new(cols, rows);
        }
    }

    /// Close the master side of the PTY.
    ///
    /// Signals EOF to the slave: future reads on the slave will drain
    /// remaining buffered data and then return 0.
    pub fn close(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.master_closed = true;
            inner.master_to_slave.close_write();
            inner.slave_to_master.close_read();
        }
    }

    /// Check whether this master endpoint is closed.
    pub fn is_closed(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.master_closed)
            .unwrap_or(true)
    }

    /// Lock the inner state, mapping mutex poison errors.
    fn lock_inner(&self) -> PtyResult<MutexGuard<'_, PtyInner>> {
        self.inner.lock().map_err(|_| PtyError::LockPoisoned)
    }
}

// ---------------------------------------------------------------------------
// PtySlave
// ---------------------------------------------------------------------------

/// The slave (child process) side of a PTY.
///
/// The child process writes its stdout/stderr here and reads stdin
/// from here.
pub struct PtySlave {
    inner: Arc<Mutex<PtyInner>>,
}

impl PtySlave {
    /// Write output data (child stdout/stderr) to the terminal.
    ///
    /// Returns the number of bytes written.
    pub fn write(&self, data: &[u8]) -> PtyResult<usize> {
        let mut inner = self.lock_inner()?;
        if inner.slave_closed {
            return Err(PtyError::Closed);
        }
        if inner.master_closed {
            return Err(PtyError::Closed);
        }
        inner.slave_to_master.write(data)
    }

    /// Read input data (from the terminal/keyboard) delivered by the
    /// master.
    ///
    /// In cooked mode, data appears only after the line discipline
    /// flushes a complete line. In raw mode, each keystroke is
    /// available immediately.
    ///
    /// Returns 0 when the master has closed and all data is drained.
    pub fn read(&self, buf: &mut [u8]) -> PtyResult<usize> {
        let mut inner = self.lock_inner()?;
        if inner.slave_closed {
            return Err(PtyError::Closed);
        }
        inner.master_to_slave.read(buf)
    }

    /// Query the current terminal window size.
    pub fn get_size(&self) -> (u16, u16) {
        self.inner
            .lock()
            .map(|inner| (inner.winsize.cols, inner.winsize.rows))
            .unwrap_or((80, 24))
    }

    /// Switch to raw mode (no line buffering, no echo, no signals).
    pub fn set_raw_mode(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.mode = PtyTerminalMode::Raw;
            // Flush any partially buffered line data through the channel
            // so it is not lost during the mode switch.
            if !inner.line_buf.is_empty() {
                let pending: Vec<u8> = inner.line_buf.drain(..).collect();
                let _ = inner.master_to_slave.write(&pending);
            }
        }
    }

    /// Switch to cooked mode (line buffering, echo, signal generation).
    pub fn set_cooked_mode(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.mode = PtyTerminalMode::Cooked;
            inner.line_buf.clear();
        }
    }

    /// Close the slave side of the PTY.
    ///
    /// Signals EOF to the master: future reads on the master will
    /// drain remaining buffered data and then return 0.
    pub fn close(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.slave_closed = true;
            inner.slave_to_master.close_write();
            inner.master_to_slave.close_read();
        }
    }

    /// Check whether this slave endpoint is closed.
    pub fn is_closed(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.slave_closed)
            .unwrap_or(true)
    }

    /// Lock the inner state, mapping mutex poison errors.
    fn lock_inner(&self) -> PtyResult<MutexGuard<'_, PtyInner>> {
        self.inner.lock().map_err(|_| PtyError::LockPoisoned)
    }
}

// ---------------------------------------------------------------------------
// PTY Pair
// ---------------------------------------------------------------------------

/// A matched master/slave PTY pair.
///
/// Cannot derive `Debug` because the inner state is behind
/// `Arc<Mutex<...>>` which would deadlock if printed while locked.
pub struct PtyPair {
    /// The terminal emulator (master) side.
    pub master: PtyMaster,
    /// The child process (slave) side.
    pub slave: PtySlave,
}

impl PtyPair {
    /// Create a new PTY pair with default window size (80x24).
    pub fn open() -> PtyResult<Self> {
        Self::open_with_size(80, 24)
    }

    /// Create a new PTY pair with the given initial window size.
    pub fn open_with_size(cols: u16, rows: u16) -> PtyResult<Self> {
        if cols == 0 || rows == 0 {
            return Err(PtyError::InvalidArgument(
                "window size must be non-zero".into(),
            ));
        }

        let inner = Arc::new(Mutex::new(PtyInner::new(cols, rows)));
        Ok(Self {
            master: PtyMaster {
                inner: Arc::clone(&inner),
            },
            slave: PtySlave { inner },
        })
    }
}

impl core::fmt::Debug for PtyPair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PtyPair")
            .field("master", &"PtyMaster { ... }")
            .field("slave", &"PtySlave { ... }")
            .finish()
    }
}

impl core::fmt::Debug for PtyMaster {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PtyMaster").finish_non_exhaustive()
    }
}

impl core::fmt::Debug for PtySlave {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PtySlave").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// PTY ID
// ---------------------------------------------------------------------------

/// Unique identifier for a registered PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PtyId(u64);

impl PtyId {
    /// Get the raw numeric value.
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl core::fmt::Display for PtyId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "pty{}", self.0)
    }
}

/// Atomic counter for generating unique PTY IDs.
static NEXT_PTY_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_pty_id() -> PtyId {
    PtyId(NEXT_PTY_ID.fetch_add(1, Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// PTY Manager
// ---------------------------------------------------------------------------

/// Maximum number of simultaneously active PTYs.
const MAX_PTYS: usize = 256;

/// Global PTY registry.
///
/// Manages allocation, lookup, and deallocation of PTY pairs. The
/// terminal emulator creates a PTY via `allocate()` when spawning a
/// new tab or session, and releases it via `release()` when the session
/// ends.
pub struct PtyManager {
    ptys: Mutex<HashMap<PtyId, PtyManagerEntry>>,
}

/// An entry in the PTY manager's registry.
struct PtyManagerEntry {
    /// The shared inner state (same Arc held by master and slave).
    inner: Arc<Mutex<PtyInner>>,
    /// Whether this entry is still active.
    active: bool,
}

impl PtyManager {
    /// Create a new empty PTY manager.
    pub fn new() -> Self {
        Self {
            ptys: Mutex::new(HashMap::new()),
        }
    }

    /// Allocate a new PTY pair and register it.
    ///
    /// Returns the assigned PTY ID and the master/slave pair. Fails
    /// with `TooManyPtys` if the registry already contains `MAX_PTYS`
    /// active entries.
    pub fn allocate(&self) -> PtyResult<(PtyId, PtyPair)> {
        self.allocate_with_size(80, 24)
    }

    /// Allocate a new PTY pair with the given initial window size.
    pub fn allocate_with_size(
        &self,
        cols: u16,
        rows: u16,
    ) -> PtyResult<(PtyId, PtyPair)> {
        let mut ptys = self.ptys.lock().map_err(|_| PtyError::LockPoisoned)?;

        // Count active entries to enforce the limit.
        let active_count = ptys.values().filter(|e| e.active).count();
        if active_count >= MAX_PTYS {
            return Err(PtyError::TooManyPtys);
        }

        let pair = PtyPair::open_with_size(cols, rows)?;
        let id = alloc_pty_id();

        // Grab the Arc from the master (master and slave share the
        // same Arc<Mutex<PtyInner>>).
        let inner = Arc::clone(&pair.master.inner);
        ptys.insert(
            id,
            PtyManagerEntry {
                inner,
                active: true,
            },
        );

        Ok((id, pair))
    }

    /// Release (close and unregister) a PTY by ID.
    pub fn release(&self, id: PtyId) -> PtyResult<()> {
        let mut ptys = self.ptys.lock().map_err(|_| PtyError::LockPoisoned)?;
        if let Some(entry) = ptys.get_mut(&id) {
            if entry.active {
                entry.active = false;
                // Close both sides of the inner channels.
                if let Ok(mut inner) = entry.inner.lock() {
                    inner.master_closed = true;
                    inner.slave_closed = true;
                    inner.master_to_slave.close_write();
                    inner.master_to_slave.close_read();
                    inner.slave_to_master.close_write();
                    inner.slave_to_master.close_read();
                }
                ptys.remove(&id);
                Ok(())
            } else {
                Err(PtyError::NotFound)
            }
        } else {
            Err(PtyError::NotFound)
        }
    }

    /// Look up a PTY's window size by ID.
    ///
    /// Returns `None` if the ID is not registered or inactive.
    pub fn get_winsize(&self, id: PtyId) -> Option<WinSize> {
        let ptys = self.ptys.lock().ok()?;
        let entry = ptys.get(&id)?;
        if !entry.active {
            return None;
        }
        let inner = entry.inner.lock().ok()?;
        Some(inner.winsize)
    }

    /// Check whether a PTY ID is registered and active.
    pub fn is_active(&self, id: PtyId) -> bool {
        self.ptys
            .lock()
            .ok()
            .and_then(|ptys| ptys.get(&id).map(|e| e.active))
            .unwrap_or(false)
    }

    /// List all active PTY IDs.
    pub fn list(&self) -> Vec<PtyId> {
        self.ptys
            .lock()
            .ok()
            .map(|ptys| {
                ptys.iter()
                    .filter(|(_, e)| e.active)
                    .map(|(id, _)| *id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Return the number of currently active PTYs.
    pub fn active_count(&self) -> usize {
        self.ptys
            .lock()
            .ok()
            .map(|ptys| ptys.values().filter(|e| e.active).count())
            .unwrap_or(0)
    }
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Exit status
// ---------------------------------------------------------------------------

/// Exit status of a child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// The process exited successfully (code 0).
    Success,
    /// The process exited with a non-zero code.
    Code(i32),
    /// The process was terminated by a signal.
    Signal(PtySignal),
}

impl ExitStatus {
    /// Check whether the process exited successfully.
    pub fn success(self) -> bool {
        matches!(self, Self::Success)
    }

    /// Get the exit code, or `None` if terminated by signal.
    pub fn code(self) -> Option<i32> {
        match self {
            Self::Success => Some(0),
            Self::Code(c) => Some(c),
            Self::Signal(_) => None,
        }
    }
}

impl core::fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Success => write!(f, "exit code 0"),
            Self::Code(c) => write!(f, "exit code {c}"),
            Self::Signal(s) => write!(f, "terminated by {s:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Child process
// ---------------------------------------------------------------------------

/// A child process running inside a PTY.
///
/// In the real OS, this would hold a kernel process handle obtained
/// via the `spawn` syscall with the slave PTY's channel handles wired
/// to the child's stdin/stdout/stderr. For now, we model the interface
/// with simulated state.
pub struct ChildProcess {
    /// Process ID.
    pid_val: u64,
    /// Program name.
    program: String,
    /// Whether the process has exited.
    exited: AtomicBool,
    /// Exit status (set when the process terminates).
    exit_status: Mutex<Option<ExitStatus>>,
    /// Whether the process has been killed.
    killed: AtomicBool,
}

/// Atomic counter for simulated PIDs.
static NEXT_PID: AtomicU64 = AtomicU64::new(1000);

impl ChildProcess {
    /// Spawn a child process with its stdin/stdout/stderr connected to
    /// the given PTY slave.
    ///
    /// In the real OS, this would:
    /// 1. Create a new process via the kernel `spawn` syscall.
    /// 2. Wire the slave's channel handles as the child's fd 0/1/2.
    /// 3. Set the child's controlling terminal to this PTY.
    /// 4. Execute the program.
    ///
    /// For now, we create a simulated process handle.
    pub fn spawn(
        program: &str,
        _args: &[&str],
        _pty: &PtySlave,
    ) -> PtyResult<Self> {
        if program.is_empty() {
            return Err(PtyError::SpawnFailed("empty program name".into()));
        }

        let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);

        Ok(Self {
            pid_val: pid,
            program: program.to_string(),
            exited: AtomicBool::new(false),
            exit_status: Mutex::new(None),
            killed: AtomicBool::new(false),
        })
    }

    /// Get the process ID.
    pub fn pid(&self) -> u64 {
        self.pid_val
    }

    /// Get the program name.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Wait for the process to exit, returning its exit status.
    ///
    /// In the real OS, this would block on the kernel's process-exit
    /// notification channel. Here we return immediately with the
    /// current status.
    pub fn wait(&self) -> PtyResult<ExitStatus> {
        // If already exited, return the stored status.
        if self.exited.load(Ordering::Acquire) {
            let status = self
                .exit_status
                .lock()
                .map_err(|_| PtyError::LockPoisoned)?;
            return Ok(status.unwrap_or(ExitStatus::Success));
        }

        // If killed, mark as exited with signal.
        if self.killed.load(Ordering::Acquire) {
            self.mark_exited(ExitStatus::Signal(PtySignal::Interrupt));
            return Ok(ExitStatus::Signal(PtySignal::Interrupt));
        }

        // Simulated: return success since we do not have a real process.
        self.mark_exited(ExitStatus::Success);
        Ok(ExitStatus::Success)
    }

    /// Non-blocking check for process exit.
    ///
    /// Returns `Ok(None)` if the process is still running.
    pub fn try_wait(&self) -> PtyResult<Option<ExitStatus>> {
        if self.exited.load(Ordering::Acquire) {
            let status = self
                .exit_status
                .lock()
                .map_err(|_| PtyError::LockPoisoned)?;
            Ok(Some(status.unwrap_or(ExitStatus::Success)))
        } else if self.killed.load(Ordering::Acquire) {
            let status = ExitStatus::Signal(PtySignal::Interrupt);
            self.mark_exited(status);
            Ok(Some(status))
        } else {
            Ok(None)
        }
    }

    /// Kill the child process.
    ///
    /// In the real OS, this would send a termination IPC message to
    /// the child process.
    pub fn kill(&self) -> PtyResult<()> {
        if self.exited.load(Ordering::Acquire) {
            return Ok(()); // already dead, idempotent
        }
        self.killed.store(true, Ordering::Release);
        self.mark_exited(ExitStatus::Signal(PtySignal::Interrupt));
        Ok(())
    }

    /// Check whether the process has exited.
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::Acquire)
    }

    /// Internal: mark the process as exited with the given status.
    fn mark_exited(&self, status: ExitStatus) {
        if let Ok(mut guard) = self.exit_status.lock() {
            if guard.is_none() {
                *guard = Some(status);
            }
        }
        self.exited.store(true, Ordering::Release);
    }

    /// Simulate the process exiting with a given status (for testing).
    #[cfg(test)]
    fn simulate_exit(&self, status: ExitStatus) {
        self.mark_exited(status);
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- PTY pair creation --

    #[test]
    fn test_pty_pair_open_default() {
        let pair = PtyPair::open().expect("should create PTY pair");
        let (cols, rows) = pair.slave.get_size();
        assert_eq!(cols, 80);
        assert_eq!(rows, 24);
    }

    #[test]
    fn test_pty_pair_open_with_size() {
        let pair =
            PtyPair::open_with_size(120, 40).expect("should create PTY pair");
        let (cols, rows) = pair.slave.get_size();
        assert_eq!(cols, 120);
        assert_eq!(rows, 40);
    }

    #[test]
    fn test_pty_pair_open_zero_cols() {
        let result = PtyPair::open_with_size(0, 24);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            PtyError::InvalidArgument("window size must be non-zero".into())
        );
    }

    #[test]
    fn test_pty_pair_open_zero_rows() {
        let result = PtyPair::open_with_size(80, 0);
        assert!(result.is_err());
    }

    // -- Bidirectional data flow (raw mode) --

    #[test]
    fn test_raw_mode_master_to_slave() {
        let pair = PtyPair::open().unwrap();
        pair.slave.set_raw_mode();

        let data = b"hello from master";
        let written = pair.master.write(data).unwrap();
        assert_eq!(written, data.len());

        let mut buf = [0u8; 64];
        let read = pair.slave.read(&mut buf).unwrap();
        assert_eq!(read, data.len());
        assert_eq!(&buf[..read], data);
    }

    #[test]
    fn test_raw_mode_slave_to_master() {
        let pair = PtyPair::open().unwrap();

        let data = b"hello from slave";
        let written = pair.slave.write(data).unwrap();
        assert_eq!(written, data.len());

        let mut buf = [0u8; 64];
        let read = pair.master.read(&mut buf).unwrap();
        assert_eq!(read, data.len());
        assert_eq!(&buf[..read], data);
    }

    #[test]
    fn test_raw_mode_bidirectional() {
        let pair = PtyPair::open().unwrap();
        pair.slave.set_raw_mode();

        // Master writes, slave reads
        pair.master.write(b"input").unwrap();
        let mut buf = [0u8; 32];
        let n = pair.slave.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"input");

        // Slave writes, master reads
        pair.slave.write(b"output").unwrap();
        let n = pair.master.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"output");
    }

    // -- Non-blocking reads --

    #[test]
    fn test_try_read_empty() {
        let pair = PtyPair::open().unwrap();
        let mut buf = [0u8; 64];
        let result = pair.master.try_read(&mut buf).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_try_read_with_data() {
        let pair = PtyPair::open().unwrap();
        pair.slave.write(b"data").unwrap();

        let mut buf = [0u8; 64];
        let result = pair.master.try_read(&mut buf).unwrap();
        assert_eq!(result, Some(4));
        assert_eq!(&buf[..4], b"data");
    }

    #[test]
    fn test_slave_read_empty_would_block() {
        let pair = PtyPair::open().unwrap();
        pair.slave.set_raw_mode();

        let mut buf = [0u8; 64];
        let result = pair.slave.read(&mut buf);
        assert_eq!(result.unwrap_err(), PtyError::WouldBlock);
    }

    // -- Resize --

    #[test]
    fn test_resize_notification() {
        let pair = PtyPair::open().unwrap();
        assert_eq!(pair.slave.get_size(), (80, 24));

        pair.master.resize(132, 43);
        assert_eq!(pair.slave.get_size(), (132, 43));
    }

    #[test]
    fn test_resize_multiple() {
        let pair = PtyPair::open().unwrap();
        pair.master.resize(100, 50);
        assert_eq!(pair.slave.get_size(), (100, 50));
        pair.master.resize(200, 60);
        assert_eq!(pair.slave.get_size(), (200, 60));
    }

    // -- Cooked mode line buffering --

    #[test]
    fn test_cooked_mode_line_buffering() {
        let pair = PtyPair::open().unwrap();
        // Default mode is cooked.

        // Type "hello\n" -- data should appear on the slave only after
        // the newline.
        pair.master.write(b"hello").unwrap();

        // Nothing flushed yet -- slave sees WouldBlock.
        let mut buf = [0u8; 64];
        let result = pair.slave.read(&mut buf);
        assert_eq!(result.unwrap_err(), PtyError::WouldBlock);

        // Now send the newline.
        pair.master.write(b"\n").unwrap();

        // Slave should now see "hello\n".
        let n = pair.slave.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"hello\n");
    }

    #[test]
    fn test_cooked_mode_carriage_return_flushes() {
        let pair = PtyPair::open().unwrap();

        pair.master.write(b"world\r").unwrap();

        let mut buf = [0u8; 64];
        let n = pair.slave.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"world\n");
    }

    // -- Cooked mode echo --

    #[test]
    fn test_cooked_mode_echo() {
        let pair = PtyPair::open().unwrap();

        // In cooked mode, typed characters are echoed back to the
        // master's read side.
        pair.master.write(b"a").unwrap();

        let mut buf = [0u8; 64];
        let result = pair.master.try_read(&mut buf).unwrap();
        // The echo of 'a' should be readable.
        assert_eq!(result, Some(1));
        assert_eq!(buf[0], b'a');
    }

    #[test]
    fn test_cooked_mode_backspace_echo() {
        let pair = PtyPair::open().unwrap();

        pair.master.write(b"ab").unwrap();
        // Drain echoes of 'a' and 'b'.
        let mut buf = [0u8; 64];
        pair.master.read(&mut buf).unwrap();

        // Send backspace.
        pair.master.write(&[0x7F]).unwrap();
        let n = pair.master.read(&mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], 0x08); // backspace echo
    }

    // -- Signal generation --

    #[test]
    fn test_cooked_ctrl_c_interrupt() {
        let mut line_buf = Vec::new();
        let action = cooked_process(0x03, &mut line_buf);
        assert_eq!(action, CookedAction::Signal(PtySignal::Interrupt));
    }

    #[test]
    fn test_cooked_ctrl_d_eof_empty_buffer() {
        let mut line_buf = Vec::new();
        let action = cooked_process(0x04, &mut line_buf);
        assert_eq!(action, CookedAction::Signal(PtySignal::Eof));
    }

    #[test]
    fn test_cooked_ctrl_d_flush_nonempty_buffer() {
        let mut line_buf = b"partial".to_vec();
        let action = cooked_process(0x04, &mut line_buf);
        assert_eq!(action, CookedAction::FlushLine(b"partial".to_vec()));
        assert!(line_buf.is_empty());
    }

    #[test]
    fn test_cooked_ctrl_z_suspend() {
        let mut line_buf = Vec::new();
        let action = cooked_process(0x1A, &mut line_buf);
        assert_eq!(action, CookedAction::Signal(PtySignal::Suspend));
    }

    #[test]
    fn test_cooked_ctrl_backslash_quit() {
        let mut line_buf = Vec::new();
        let action = cooked_process(0x1C, &mut line_buf);
        assert_eq!(action, CookedAction::Signal(PtySignal::Quit));
    }

    #[test]
    fn test_cooked_ctrl_c_clears_line_buffer() {
        let mut line_buf = b"some text".to_vec();
        let action = cooked_process(0x03, &mut line_buf);
        assert_eq!(action, CookedAction::Signal(PtySignal::Interrupt));
        assert!(line_buf.is_empty());
    }

    // -- Raw mode passthrough --

    #[test]
    fn test_raw_mode_no_line_buffering() {
        let pair = PtyPair::open().unwrap();
        pair.slave.set_raw_mode();

        // Each byte should appear immediately on the slave.
        pair.master.write(b"x").unwrap();

        let mut buf = [0u8; 64];
        let n = pair.slave.read(&mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], b'x');
    }

    #[test]
    fn test_raw_mode_no_echo() {
        let pair = PtyPair::open().unwrap();
        pair.slave.set_raw_mode();

        pair.master.write(b"y").unwrap();

        // In raw mode, nothing should be echoed back to master.
        let mut buf = [0u8; 64];
        let result = pair.master.try_read(&mut buf).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_raw_mode_ctrl_c_passthrough() {
        let pair = PtyPair::open().unwrap();
        pair.slave.set_raw_mode();

        // Ctrl+C (0x03) should pass through as a regular byte.
        pair.master.write(&[0x03]).unwrap();

        let mut buf = [0u8; 64];
        let n = pair.slave.read(&mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], 0x03);
    }

    // -- Mode switching --

    #[test]
    fn test_mode_switch_flushes_line_buffer() {
        let pair = PtyPair::open().unwrap();

        // Type something in cooked mode (not flushed yet).
        pair.master.write(b"partial").unwrap();

        // Switch to raw mode -- the partial line should be flushed.
        pair.slave.set_raw_mode();

        let mut buf = [0u8; 64];
        let n = pair.slave.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"partial");
    }

    // -- PtyManager --

    #[test]
    fn test_manager_allocate() {
        let mgr = PtyManager::new();
        let (id, _pair) = mgr.allocate().unwrap();
        assert!(mgr.is_active(id));
        assert_eq!(mgr.active_count(), 1);
    }

    #[test]
    fn test_manager_release() {
        let mgr = PtyManager::new();
        let (id, _pair) = mgr.allocate().unwrap();
        mgr.release(id).unwrap();
        assert!(!mgr.is_active(id));
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn test_manager_release_not_found() {
        let mgr = PtyManager::new();
        let fake_id = PtyId(99999);
        let result = mgr.release(fake_id);
        assert_eq!(result.unwrap_err(), PtyError::NotFound);
    }

    #[test]
    fn test_manager_double_release() {
        let mgr = PtyManager::new();
        let (id, _pair) = mgr.allocate().unwrap();
        mgr.release(id).unwrap();
        let result = mgr.release(id);
        assert_eq!(result.unwrap_err(), PtyError::NotFound);
    }

    #[test]
    fn test_manager_list() {
        let mgr = PtyManager::new();
        let (id1, _p1) = mgr.allocate().unwrap();
        let (id2, _p2) = mgr.allocate().unwrap();

        let mut list = mgr.list();
        list.sort_by_key(|id| id.raw());
        assert_eq!(list.len(), 2);
        assert!(list.contains(&id1));
        assert!(list.contains(&id2));
    }

    #[test]
    fn test_manager_max_ptys() {
        let mgr = PtyManager::new();
        let mut pairs = Vec::new();

        for _ in 0..MAX_PTYS {
            let (_id, pair) = mgr.allocate().unwrap();
            pairs.push(pair);
        }

        // The next allocation should fail.
        let result = mgr.allocate();
        assert_eq!(result.unwrap_err(), PtyError::TooManyPtys);
        assert_eq!(mgr.active_count(), MAX_PTYS);
    }

    #[test]
    fn test_manager_get_winsize() {
        let mgr = PtyManager::new();
        let (id, pair) = mgr.allocate_with_size(132, 50).unwrap();

        let ws = mgr.get_winsize(id).unwrap();
        assert_eq!(ws.cols, 132);
        assert_eq!(ws.rows, 50);

        // Resize via master and check again.
        pair.master.resize(200, 75);
        let ws = mgr.get_winsize(id).unwrap();
        assert_eq!(ws.cols, 200);
        assert_eq!(ws.rows, 75);
    }

    #[test]
    fn test_manager_get_winsize_after_release() {
        let mgr = PtyManager::new();
        let (id, _pair) = mgr.allocate().unwrap();
        mgr.release(id).unwrap();
        assert!(mgr.get_winsize(id).is_none());
    }

    // -- Close propagation --

    #[test]
    fn test_close_master_eof_to_slave() {
        let pair = PtyPair::open().unwrap();
        pair.slave.set_raw_mode();

        // Write some data, then close master.
        pair.master.write(b"goodbye").unwrap();
        pair.master.close();

        // Slave should still read buffered data.
        let mut buf = [0u8; 64];
        let n = pair.slave.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"goodbye");

        // After draining, slave gets EOF (0 bytes).
        let n = pair.slave.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn test_close_slave_error_to_master() {
        let pair = PtyPair::open().unwrap();

        pair.slave.close();

        // Master writes should fail with Closed.
        let result = pair.master.write(b"test");
        assert_eq!(result.unwrap_err(), PtyError::Closed);
    }

    #[test]
    fn test_close_slave_master_reads_buffered_then_eof() {
        let pair = PtyPair::open().unwrap();

        // Slave writes, then closes.
        pair.slave.write(b"final output").unwrap();
        pair.slave.close();

        // Master should read the buffered data.
        let mut buf = [0u8; 64];
        let n = pair.master.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"final output");

        // Then get EOF.
        let n = pair.master.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn test_master_is_closed() {
        let pair = PtyPair::open().unwrap();
        assert!(!pair.master.is_closed());
        pair.master.close();
        assert!(pair.master.is_closed());
    }

    #[test]
    fn test_slave_is_closed() {
        let pair = PtyPair::open().unwrap();
        assert!(!pair.slave.is_closed());
        pair.slave.close();
        assert!(pair.slave.is_closed());
    }

    // -- ChildProcess --

    #[test]
    fn test_child_spawn() {
        let pair = PtyPair::open().unwrap();
        let child =
            ChildProcess::spawn("/bin/sh", &["-l"], &pair.slave).unwrap();
        assert!(child.pid() >= 1000);
        assert_eq!(child.program(), "/bin/sh");
        assert!(!child.has_exited());
    }

    #[test]
    fn test_child_spawn_empty_program() {
        let pair = PtyPair::open().unwrap();
        let result = ChildProcess::spawn("", &[], &pair.slave);
        assert!(result.is_err());
    }

    #[test]
    fn test_child_wait() {
        let pair = PtyPair::open().unwrap();
        let child =
            ChildProcess::spawn("/bin/echo", &["hello"], &pair.slave).unwrap();

        let status = child.wait().unwrap();
        assert!(status.success());
        assert_eq!(status.code(), Some(0));
        assert!(child.has_exited());
    }

    #[test]
    fn test_child_try_wait_not_exited() {
        let pair = PtyPair::open().unwrap();
        let child =
            ChildProcess::spawn("/bin/sleep", &["10"], &pair.slave).unwrap();

        let result = child.try_wait().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_child_kill() {
        let pair = PtyPair::open().unwrap();
        let child =
            ChildProcess::spawn("/bin/sleep", &["10"], &pair.slave).unwrap();

        child.kill().unwrap();
        assert!(child.has_exited());

        let status = child.wait().unwrap();
        assert!(!status.success());
        assert_eq!(status, ExitStatus::Signal(PtySignal::Interrupt));
    }

    #[test]
    fn test_child_kill_idempotent() {
        let pair = PtyPair::open().unwrap();
        let child =
            ChildProcess::spawn("/bin/test", &[], &pair.slave).unwrap();

        child.kill().unwrap();
        child.kill().unwrap(); // should not panic or error
        assert!(child.has_exited());
    }

    #[test]
    fn test_child_simulate_exit() {
        let pair = PtyPair::open().unwrap();
        let child =
            ChildProcess::spawn("/bin/app", &[], &pair.slave).unwrap();

        child.simulate_exit(ExitStatus::Code(42));
        assert!(child.has_exited());

        let status = child.wait().unwrap();
        assert_eq!(status, ExitStatus::Code(42));
        assert_eq!(status.code(), Some(42));
    }

    // -- Multiple concurrent PTYs --

    #[test]
    fn test_multiple_concurrent_ptys() {
        let mgr = PtyManager::new();

        let (id1, pair1) = mgr.allocate().unwrap();
        let (id2, pair2) = mgr.allocate().unwrap();

        assert_ne!(id1, id2);

        pair1.slave.set_raw_mode();
        pair2.slave.set_raw_mode();

        // Write different data to each PTY.
        pair1.master.write(b"pty1").unwrap();
        pair2.master.write(b"pty2").unwrap();

        // Read back -- each should get its own data.
        let mut buf = [0u8; 64];
        let n = pair1.slave.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"pty1");

        let n = pair2.slave.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"pty2");

        assert_eq!(mgr.active_count(), 2);

        mgr.release(id1).unwrap();
        assert_eq!(mgr.active_count(), 1);
        mgr.release(id2).unwrap();
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn test_multiple_ptys_independent_modes() {
        let pair1 = PtyPair::open().unwrap();
        let pair2 = PtyPair::open().unwrap();

        pair1.slave.set_raw_mode();
        // pair2 stays in cooked mode (default).

        // pair1: raw mode -- data goes straight through.
        pair1.master.write(b"r").unwrap();
        let mut buf = [0u8; 64];
        let n = pair1.slave.read(&mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(buf[0], b'r');

        // pair2: cooked mode -- data is buffered until newline.
        pair2.master.write(b"c").unwrap();
        let result = pair2.slave.read(&mut buf);
        assert_eq!(result.unwrap_err(), PtyError::WouldBlock);
    }

    // -- Byte channel internals --

    #[test]
    fn test_byte_channel_wrap_around() {
        let mut ch = ByteChannel::new(8);

        // Fill the buffer.
        let data = [1, 2, 3, 4, 5, 6, 7, 8];
        let written = ch.write(&data).unwrap();
        assert_eq!(written, 8);

        // Read half.
        let mut buf = [0u8; 4];
        let read = ch.read(&mut buf).unwrap();
        assert_eq!(read, 4);
        assert_eq!(buf, [1, 2, 3, 4]);

        // Write more (wraps around).
        let written = ch.write(&[9, 10, 11, 12]).unwrap();
        assert_eq!(written, 4);

        // Read everything.
        let mut buf = [0u8; 8];
        let read = ch.read(&mut buf).unwrap();
        assert_eq!(read, 8);
        assert_eq!(&buf[..8], &[5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn test_byte_channel_buffer_full() {
        let mut ch = ByteChannel::new(4);
        ch.write(&[1, 2, 3, 4]).unwrap();

        let result = ch.write(&[5]);
        assert_eq!(result.unwrap_err(), PtyError::BufferFull);
    }

    #[test]
    fn test_byte_channel_partial_write() {
        let mut ch = ByteChannel::new(4);
        ch.write(&[1, 2, 3]).unwrap();

        // Only 1 byte of space remaining.
        let written = ch.write(&[4, 5, 6]).unwrap();
        assert_eq!(written, 1);
    }

    #[test]
    fn test_byte_channel_eof_after_close() {
        let mut ch = ByteChannel::new(16);
        ch.write(b"data").unwrap();
        ch.close_write();

        let mut buf = [0u8; 16];
        let n = ch.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"data");

        // Now we get EOF (0 bytes).
        let n = ch.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn test_byte_channel_available() {
        let mut ch = ByteChannel::new(16);
        assert_eq!(ch.available(), 0);
        ch.write(b"abc").unwrap();
        assert_eq!(ch.available(), 3);
        let mut buf = [0u8; 1];
        ch.read(&mut buf).unwrap();
        assert_eq!(ch.available(), 2);
    }

    #[test]
    fn test_byte_channel_write_after_read_close() {
        let mut ch = ByteChannel::new(16);
        ch.close_read();
        let result = ch.write(b"test");
        assert_eq!(result.unwrap_err(), PtyError::Closed);
    }

    // -- WinSize --

    #[test]
    fn test_winsize_default() {
        let ws = WinSize::default();
        assert_eq!(ws.cols, 80);
        assert_eq!(ws.rows, 24);
    }

    // -- ExitStatus --

    #[test]
    fn test_exit_status_success() {
        let s = ExitStatus::Success;
        assert!(s.success());
        assert_eq!(s.code(), Some(0));
    }

    #[test]
    fn test_exit_status_code() {
        let s = ExitStatus::Code(1);
        assert!(!s.success());
        assert_eq!(s.code(), Some(1));
    }

    #[test]
    fn test_exit_status_signal() {
        let s = ExitStatus::Signal(PtySignal::Interrupt);
        assert!(!s.success());
        assert_eq!(s.code(), None);
    }

    // -- PtyError display --

    #[test]
    fn test_pty_error_display() {
        assert_eq!(PtyError::Closed.to_string(), "PTY channel closed");
        assert_eq!(PtyError::WouldBlock.to_string(), "operation would block");
        assert_eq!(PtyError::TooManyPtys.to_string(), "maximum number of PTYs reached");
    }

    // -- PtyId display --

    #[test]
    fn test_pty_id_display() {
        let id = PtyId(42);
        assert_eq!(id.to_string(), "pty42");
        assert_eq!(id.raw(), 42);
    }

    // -- cooked_process edge cases --

    #[test]
    fn test_cooked_process_regular_byte() {
        let mut line_buf = Vec::new();
        let action = cooked_process(b'A', &mut line_buf);
        assert_eq!(action, CookedAction::Echo(b'A'));
        assert_eq!(line_buf, b"A");
    }

    #[test]
    fn test_cooked_process_backspace_empty() {
        let mut line_buf = Vec::new();
        let action = cooked_process(0x7F, &mut line_buf);
        // Nothing to erase -- returns Buffer.
        assert_eq!(action, CookedAction::Buffer);
    }

    #[test]
    fn test_cooked_process_newline() {
        let mut line_buf = b"test".to_vec();
        let action = cooked_process(b'\n', &mut line_buf);
        assert_eq!(action, CookedAction::FlushLine(b"test\n".to_vec()));
        assert!(line_buf.is_empty());
    }
}

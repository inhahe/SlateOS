//! Kernel structured logging — JSON-lines output.
//!
//! Provides a structured logging facility that outputs JSON-lines to
//! serial and stores entries in a kernel ring buffer for userspace
//! consumption.
//!
//! ## Design
//!
//! The OS design spec requires **text-based, not binary** logging in
//! JSON-lines format.  Each log entry is a single JSON object followed
//! by a newline, making it easy to parse, filter, and forward.
//!
//! ## Output Format
//!
//! ```json
//! {"t":1234567,"l":"info","m":"sched","msg":"Task 5 spawned (priority 16)"}
//! {"t":1234600,"l":"warn","m":"mm","msg":"Frame alloc: low memory (42 frames left)"}
//! ```
//!
//! Fields:
//! - `t`: timestamp in milliseconds since boot (from APIC tick count).
//! - `l`: log level (`trace`, `debug`, `info`, `warn`, `error`).
//! - `m`: module name (e.g., `sched`, `mm`, `ipc`, `fs`).
//! - `msg`: human-readable message.
//!
//! ## Usage
//!
//! ```rust,ignore
//! klog!(Info, "sched", "Task {} spawned (priority {})", task_id, priority);
//! klog!(Error, "mm", "Frame alloc failed: {}", err);
//! ```
//!
//! ## Ring Buffer
//!
//! Recent entries are stored in a fixed-size ring buffer so userspace
//! processes (e.g., the service manager) can read them via syscall.
//! When the buffer is full, oldest entries are silently dropped.
//!
//! ## Thread Safety
//!
//! The ring buffer and serial output are protected by a spinlock.
//! Log calls from interrupt context are safe (the lock is a spinlock,
//! not a sleeping mutex).

use core::fmt;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Log levels
// ---------------------------------------------------------------------------

/// Log severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    /// Very detailed tracing information.
    Trace = 0,
    /// Debugging information.
    Debug = 1,
    /// Normal operational messages.
    Info = 2,
    /// Something unexpected but recoverable.
    Warn = 3,
    /// A serious failure.
    Error = 4,
}

impl Level {
    /// String representation for JSON output.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

/// Maximum length of a single log message (bytes).
/// Messages longer than this are truncated.
const MAX_MSG_LEN: usize = 200;

/// Maximum length of a module name.
const MAX_MODULE_LEN: usize = 24;

/// Number of entries in the ring buffer.
/// Power of two for efficient modular indexing.
const RING_SIZE: usize = 256;

/// A single structured log entry.
#[derive(Clone)]
struct LogEntry {
    /// Timestamp in milliseconds since boot.
    timestamp_ms: u64,
    /// Log level.
    level: Level,
    /// Module name (fixed buffer, null-terminated).
    module: [u8; MAX_MODULE_LEN],
    /// Module name length.
    module_len: u8,
    /// Message content (fixed buffer).
    message: [u8; MAX_MSG_LEN],
    /// Message length.
    message_len: u8,
    /// Sequence number (monotonically increasing).
    seq: u64,
}

impl LogEntry {
    /// Create a zeroed (invalid) entry.
    const fn zeroed() -> Self {
        Self {
            timestamp_ms: 0,
            level: Level::Trace,
            module: [0; MAX_MODULE_LEN],
            module_len: 0,
            message: [0; MAX_MSG_LEN],
            message_len: 0,
            seq: 0,
        }
    }

    /// Get the module name as a string slice.
    fn module_str(&self) -> &str {
        let len = self.module_len as usize;
        // SAFETY: We only write valid UTF-8 into the module buffer.
        unsafe {
            core::str::from_utf8_unchecked(
                self.module.get(..len).unwrap_or(&[]),
            )
        }
    }

    /// Get the message as a string slice.
    fn message_str(&self) -> &str {
        let len = self.message_len as usize;
        // SAFETY: We only write valid UTF-8 into the message buffer.
        unsafe {
            core::str::from_utf8_unchecked(
                self.message.get(..len).unwrap_or(&[]),
            )
        }
    }
}

/// The global log ring buffer.
struct LogRing {
    /// Ring buffer entries.
    entries: [LogEntry; RING_SIZE],
    /// Write index (next slot to write).
    write_idx: usize,
    /// Total entries written (for sequence numbers and overflow detection).
    total_written: u64,
    /// Minimum log level for serial output.
    serial_level: Level,
}

impl LogRing {
    /// Create a new empty ring buffer.
    const fn new() -> Self {
        // Workaround: can't use array::from_fn in const context.
        // Use a const zeroed entry.
        Self {
            entries: [const { LogEntry::zeroed() }; RING_SIZE],
            write_idx: 0,
            total_written: 0,
            serial_level: Level::Info,
        }
    }

    /// Write a log entry.  Returns the sequence number assigned.
    fn write(&mut self, level: Level, module: &str, message: &str) -> u64 {
        let seq = self.total_written;
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.total_written = self.total_written.wrapping_add(1);
        }

        let timestamp_ms = boot_time_ms();

        // Build the entry in-place.
        let idx = self.write_idx;
        let entry = &mut self.entries[idx];

        entry.timestamp_ms = timestamp_ms;
        entry.level = level;
        entry.seq = seq;

        // Copy module name (truncate if too long).
        let mod_bytes = module.as_bytes();
        let mod_len = mod_bytes.len().min(MAX_MODULE_LEN);
        entry.module[..mod_len].copy_from_slice(
            mod_bytes.get(..mod_len).unwrap_or(&[]),
        );
        entry.module_len = mod_len as u8;

        // Copy message (truncate if too long).
        let msg_bytes = message.as_bytes();
        let msg_len = msg_bytes.len().min(MAX_MSG_LEN);
        entry.message[..msg_len].copy_from_slice(
            msg_bytes.get(..msg_len).unwrap_or(&[]),
        );
        entry.message_len = msg_len as u8;

        // Advance write index (wrap around).
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.write_idx = (self.write_idx + 1) % RING_SIZE;
        }

        // Output to serial if level is at or above the threshold.
        if level >= self.serial_level {
            emit_serial(entry);
        }

        seq
    }

    /// Read entries newer than `after_seq` into the provided buffer.
    ///
    /// Returns the number of entries written and the newest sequence
    /// number seen (for the next call's `after_seq`).
    ///
    /// Entries are returned oldest-first.
    fn read_since(
        &self,
        after_seq: u64,
        buf: &mut [u8],
        buf_cap: usize,
    ) -> (usize, u64) {
        let mut offset = 0usize;
        let mut count = 0usize;
        let mut newest_seq = after_seq;

        // Find the start: oldest entry still in the ring.
        let oldest_seq = if self.total_written > RING_SIZE as u64 {
            #[allow(clippy::arithmetic_side_effects)]
            { self.total_written - RING_SIZE as u64 }
        } else {
            0
        };

        let start_seq = if after_seq == u64::MAX {
            // Special case: u64::MAX means "read from the beginning".
            oldest_seq
        } else if after_seq >= oldest_seq {
            after_seq.wrapping_add(1)
        } else {
            oldest_seq
        };

        let mut seq = start_seq;
        while seq < self.total_written {
            let idx = (seq as usize) % RING_SIZE;
            let entry = &self.entries[idx];

            if entry.seq != seq {
                // Entry was overwritten by a newer one — skip.
                #[allow(clippy::arithmetic_side_effects)]
                { seq += 1; }
                continue;
            }

            // Serialize this entry as a JSON line.
            let line_len = json_line_len(entry);
            #[allow(clippy::arithmetic_side_effects)]
            if offset + line_len > buf_cap {
                break; // Buffer full.
            }

            let written = write_json_line(
                entry,
                buf.get_mut(offset..).unwrap_or(&mut []),
            );
            #[allow(clippy::arithmetic_side_effects)]
            {
                offset += written;
                count += 1;
            }
            newest_seq = seq;

            #[allow(clippy::arithmetic_side_effects)]
            { seq += 1; }
        }

        (count, newest_seq)
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// The global log ring buffer, protected by a spinlock.
static LOG_RING: Mutex<LogRing> = Mutex::new(LogRing::new());

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

/// Get milliseconds since boot from the APIC timer tick count.
///
/// APIC timer runs at 100 Hz, so each tick is 10 ms.
fn boot_time_ms() -> u64 {
    let ticks = crate::apic::tick_count();
    // Each tick = 10 ms at 100 Hz.
    #[allow(clippy::arithmetic_side_effects)]
    { ticks * 10 }
}

// ---------------------------------------------------------------------------
// Serial output
// ---------------------------------------------------------------------------

/// Emit a log entry to serial as a JSON line.
///
/// This writes directly to the serial port, bypassing the serial
/// lock — the caller holds LOG_RING which serializes all output.
fn emit_serial(entry: &LogEntry) {
    // We can't use serial_println! here because we're inside a lock
    // that might deadlock.  Use the serial writer directly.
    //
    // Format: {"t":<ms>,"l":"<level>","m":"<module>","msg":"<message>"}
    crate::serial_println!(
        "{{\"t\":{},\"l\":\"{}\",\"m\":\"{}\",\"msg\":\"{}\"}}",
        entry.timestamp_ms,
        entry.level.as_str(),
        entry.module_str(),
        // Escape special JSON characters in the message.
        JsonEscape(entry.message_str()),
    );
}

/// Wrapper that escapes special characters for JSON string output.
struct JsonEscape<'a>(&'a str);

impl<'a> fmt::Display for JsonEscape<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for ch in self.0.chars() {
            match ch {
                '"' => write!(f, "\\\"")?,
                '\\' => write!(f, "\\\\")?,
                '\n' => write!(f, "\\n")?,
                '\r' => write!(f, "\\r")?,
                '\t' => write!(f, "\\t")?,
                // Control characters.
                c if c.is_control() => write!(f, "\\u{:04x}", c as u32)?,
                c => write!(f, "{c}")?,
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// JSON serialization helpers
// ---------------------------------------------------------------------------

/// Estimate the JSON line length for a log entry.
///
/// Used to check if the entry fits in the read buffer before writing.
fn json_line_len(entry: &LogEntry) -> usize {
    // {"t":NNNNN,"l":"LEVEL","m":"MODULE","msg":"MESSAGE"}\n
    // Conservative estimate: overhead + module + message + some slack.
    #[allow(clippy::arithmetic_side_effects)]
    { 60 + entry.module_len as usize + (entry.message_len as usize * 2) }
}

/// Write a log entry as a JSON line into a byte buffer.
///
/// Returns the number of bytes written.
fn write_json_line(entry: &LogEntry, buf: &mut [u8]) -> usize {
    // Build the JSON line using a simple byte writer.
    let mut writer = BufWriter::new(buf);

    writer.write_str("{\"t\":");
    writer.write_u64(entry.timestamp_ms);
    writer.write_str(",\"l\":\"");
    writer.write_str(entry.level.as_str());
    writer.write_str("\",\"m\":\"");
    writer.write_str(entry.module_str());
    writer.write_str("\",\"msg\":\"");
    writer.write_json_str(entry.message_str());
    writer.write_str("\"}\n");

    writer.pos
}

/// A simple byte buffer writer (no alloc, no fmt overhead).
struct BufWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BufWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn write_str(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let avail = self.buf.len().saturating_sub(self.pos);
        let len = bytes.len().min(avail);
        if let Some(dest) = self.buf.get_mut(self.pos..self.pos.wrapping_add(len)) {
            if let Some(src) = bytes.get(..len) {
                dest.copy_from_slice(src);
            }
        }
        #[allow(clippy::arithmetic_side_effects)]
        { self.pos += len; }
    }

    fn write_u64(&mut self, val: u64) {
        // Stack-allocated decimal conversion (max 20 digits).
        let mut digits = [0u8; 20];
        let mut n = val;
        let mut i = 20usize;

        if n == 0 {
            self.write_str("0");
            return;
        }

        while n > 0 {
            i = i.wrapping_sub(1);
            #[allow(clippy::arithmetic_side_effects)]
            {
                digits[i] = b'0' + (n % 10) as u8;
                n /= 10;
            }
        }

        if let Some(slice) = digits.get(i..) {
            // SAFETY: digits[i..] contains only ASCII digits.
            let s = unsafe { core::str::from_utf8_unchecked(slice) };
            self.write_str(s);
        }
    }

    fn write_json_str(&mut self, s: &str) {
        for ch in s.bytes() {
            match ch {
                b'"' => self.write_str("\\\""),
                b'\\' => self.write_str("\\\\"),
                b'\n' => self.write_str("\\n"),
                b'\r' => self.write_str("\\r"),
                b'\t' => self.write_str("\\t"),
                b if b < 0x20 => {
                    // Control character — write as \u00XX.
                    let hex = b"0123456789abcdef";
                    let h = [
                        b'\\', b'u', b'0', b'0',
                        hex[(b >> 4) as usize],
                        hex[(b & 0x0f) as usize],
                    ];
                    // SAFETY: hex digits are valid ASCII.
                    let s = unsafe { core::str::from_utf8_unchecked(&h) };
                    self.write_str(s);
                }
                _ => {
                    let avail = self.buf.len().saturating_sub(self.pos);
                    if avail > 0 {
                        self.buf[self.pos] = ch;
                        #[allow(clippy::arithmetic_side_effects)]
                        { self.pos += 1; }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Write a structured log entry (string literal, no format args).
///
/// Writes to both the ring buffer and serial (if level >= serial
/// threshold).  Use `klog!()` for formatted messages.
#[allow(dead_code)] // Used by callers that have a pre-formatted string.
pub fn log(level: Level, module: &str, message: &str) {
    let mut ring = LOG_RING.lock();
    ring.write(level, module, message);
}

/// Write a structured log entry using `fmt::Arguments`.
///
/// Called by the `klog!()` macro when format arguments are used.
pub fn log_fmt(level: Level, module: &str, args: fmt::Arguments<'_>) {
    // Format the message into a stack-allocated buffer.
    let mut buf = [0u8; MAX_MSG_LEN];
    let mut writer = FmtBuf { buf: &mut buf, pos: 0 };
    let _ = fmt::write(&mut writer, args);
    let len = writer.pos.min(MAX_MSG_LEN);

    // SAFETY: fmt::write only produces valid UTF-8.
    let message = unsafe {
        core::str::from_utf8_unchecked(buf.get(..len).unwrap_or(&[]))
    };

    let mut ring = LOG_RING.lock();
    ring.write(level, module, message);
}

/// Set the minimum log level for serial output.
///
/// Entries below this level are stored in the ring buffer but not
/// printed to serial.
#[allow(dead_code)]
pub fn set_serial_level(level: Level) {
    let mut ring = LOG_RING.lock();
    ring.serial_level = level;
}

/// Read log entries newer than `after_seq` into a buffer.
///
/// Returns `(entry_count, newest_seq)`.  Caller should pass
/// `newest_seq` as `after_seq` on the next call.
///
/// Each entry is a JSON line (terminated with `\n`).
pub fn read_logs(after_seq: u64, buf: &mut [u8]) -> (usize, u64) {
    let ring = LOG_RING.lock();
    ring.read_since(after_seq, buf, buf.len())
}

/// Get the total number of log entries written (including overwritten ones).
#[allow(dead_code)]
pub fn total_entries() -> u64 {
    let ring = LOG_RING.lock();
    ring.total_written
}

/// Helper: `core::fmt::Write` adapter for a fixed-size byte buffer.
struct FmtBuf<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> fmt::Write for FmtBuf<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let avail = self.buf.len().saturating_sub(self.pos);
        let len = bytes.len().min(avail);
        if let Some(dest) = self.buf.get_mut(self.pos..self.pos.wrapping_add(len)) {
            if let Some(src) = bytes.get(..len) {
                dest.copy_from_slice(src);
            }
        }
        #[allow(clippy::arithmetic_side_effects)]
        { self.pos += len; }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// klog! macro
// ---------------------------------------------------------------------------

/// Structured log macro.
///
/// Writes a JSON-lines log entry to the kernel ring buffer and
/// (optionally) to serial output.
///
/// # Usage
///
/// ```rust,ignore
/// klog!(Info, "sched", "Task {} spawned", task_id);
/// klog!(Error, "mm", "OOM: only {} frames left", count);
/// klog!(Warn, "fs", "Path too long: {}", path);
/// ```
#[macro_export]
macro_rules! klog {
    ($level:ident, $module:expr, $($arg:tt)*) => {{
        $crate::klog::log_fmt(
            $crate::klog::Level::$level,
            $module,
            format_args!($($arg)*),
        );
    }};
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run structured logging self-tests.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;

    crate::serial_println!("[klog] Running structured logging self-test...");

    // Test 1: Write a log entry and verify it was stored.
    let before = total_entries();
    klog!(Info, "test", "Self-test entry {}", 42);
    let after = total_entries();
    #[allow(clippy::arithmetic_side_effects)]
    if after != before + 1 {
        crate::serial_println!(
            "[klog]   FAIL: expected {} entries, got {}",
            before + 1,
            after,
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[klog]   Write entry: OK");

    // Test 2: Read the entry back from the ring buffer.
    let mut buf = [0u8; 512];
    // Read everything since before the test entry.
    // Use u64::MAX as "read from beginning" if before is 0.
    let after_seq = if before == 0 { u64::MAX } else { before.wrapping_sub(1) };
    let (count, newest_seq) = read_logs(after_seq, &mut buf);
    if count == 0 {
        crate::serial_println!("[klog]   FAIL: read_logs returned 0 entries");
        return Err(KernelError::InternalError);
    }

    // The JSON line should contain our test message.
    let read_len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let output = core::str::from_utf8(buf.get(..read_len).unwrap_or(&[]));
    match output {
        Ok(s) if s.contains("Self-test entry 42") => {}
        Ok(s) => {
            crate::serial_println!(
                "[klog]   FAIL: read-back doesn't contain expected text: {}",
                s,
            );
            return Err(KernelError::InternalError);
        }
        Err(_) => {
            crate::serial_println!("[klog]   FAIL: read-back is not valid UTF-8");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[klog]   Read-back: OK (seq={})", newest_seq);

    // Test 3: JSON escaping.
    klog!(Debug, "test", "escape: \"quotes\" and \\backslash");
    let mut buf2 = [0u8; 512];
    let (count2, _) = read_logs(newest_seq, &mut buf2);
    if count2 == 0 {
        crate::serial_println!("[klog]   FAIL: escape entry not found");
        return Err(KernelError::InternalError);
    }
    let read_len2 = buf2.iter().position(|&b| b == 0).unwrap_or(buf2.len());
    let output2 = core::str::from_utf8(
        buf2.get(..read_len2).unwrap_or(&[])
    ).unwrap_or("");
    if !output2.contains("\\\"quotes\\\"") || !output2.contains("\\\\backslash") {
        crate::serial_println!(
            "[klog]   FAIL: JSON escaping incorrect: {}",
            output2,
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[klog]   JSON escaping: OK");

    // Test 4: Multiple levels.
    klog!(Trace, "test", "trace message");
    klog!(Warn, "test", "warn message");
    klog!(Error, "test", "error message");
    let total_after_levels = total_entries();
    #[allow(clippy::arithmetic_side_effects)]
    let expected = after + 4; // 1 escape + 3 levels
    if total_after_levels != expected {
        crate::serial_println!(
            "[klog]   FAIL: expected {} total, got {}",
            expected,
            total_after_levels,
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[klog]   Multi-level: OK");

    crate::serial_println!("[klog] Structured logging self-test PASSED");
    Ok(())
}

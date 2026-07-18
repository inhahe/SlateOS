//! Filesystem change journal — persistent change tracking across reboots.
//!
//! Provides a monotonically-increasing sequence of filesystem change events
//! that survives program restarts and OS reboots.  Primary use case: backup
//! programs that need to detect "what changed since my last run?" without
//! doing a full directory tree scan.
//!
//! ## Design
//!
//! - **In-memory ring buffer** (bounded) of journal entries, each with a
//!   sequence number, timestamp, event type, and path.
//! - **On-disk persistence** via a `/_JOURNAL` file (JSON-lines format,
//!   per the design spec's "no binary logs" rule).
//! - **On boot**: load the journal file to restore the sequence counter
//!   and recent entries.  Missing file means seq starts at 1.
//! - **On mutation**: append to the ring buffer.  Periodically flush to disk
//!   (or on explicit `flush()` / before unmount).
//! - **Reader API**: `read_since(seq)` returns all entries with sequence
//!   numbers > `seq`.  If old entries were evicted from the ring buffer,
//!   the gap is detectable (returned `start_seq > requested_seq`).
//!
//! ## Syscalls
//!
//! - `SYS_FS_JOURNAL_CURSOR` (625): returns the current highest sequence number.
//! - `SYS_FS_JOURNAL_READ` (626): read entries since a given sequence number.
//!
//! ## References
//!
//! - Windows USN (Update Sequence Number) Journal / NTFS Change Journal
//! - design.txt lines 1013-1035: "detect filesystem changes since last API call,
//!   even if program was closed or OS rebooted"

#![allow(dead_code)]

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Event types (reuses FsEventType concept from notify, but journal-specific)
// ---------------------------------------------------------------------------

/// Type of filesystem change recorded in the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JournalEventType {
    /// File or directory created.
    Created = 0,
    /// File content modified (write, write_at, truncate).
    Modified = 1,
    /// File or directory deleted.
    Deleted = 2,
    /// File or directory renamed/moved.
    Renamed = 3,
}

impl JournalEventType {
    /// Convert to a short string tag for JSON serialization.
    fn as_str(self) -> &'static str {
        match self {
            Self::Created => "create",
            Self::Modified => "modify",
            Self::Deleted => "delete",
            Self::Renamed => "rename",
        }
    }

    /// Parse from a JSON string tag.
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "create" => Some(Self::Created),
            "modify" => Some(Self::Modified),
            "delete" => Some(Self::Deleted),
            "rename" => Some(Self::Renamed),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Journal entry
// ---------------------------------------------------------------------------

/// A single filesystem change event in the journal.
#[derive(Debug, Clone)]
pub struct JournalEntry {
    /// Monotonically increasing sequence number (never reused, never decremented).
    pub seq: u64,
    /// Monotonic nanoseconds since boot (from HPET or TSC).
    pub timestamp_ns: u64,
    /// Type of change.
    pub event_type: JournalEventType,
    /// Affected path (destination for renames).
    pub path: String,
    /// Original path for rename events; empty for other event types.
    pub old_path: String,
}

impl JournalEntry {
    /// Serialize to a JSON-lines compatible string (no newlines in output).
    ///
    /// Format: `{"seq":N,"ts":N,"type":"...","path":"..."}` or with `"from":"..."` for renames.
    fn to_json_line(&self) -> String {
        let mut s = String::with_capacity(128);
        s.push_str("{\"seq\":");
        push_u64(&mut s, self.seq);
        s.push_str(",\"ts\":");
        push_u64(&mut s, self.timestamp_ns);
        s.push_str(",\"type\":\"");
        s.push_str(self.event_type.as_str());
        s.push_str("\",\"path\":\"");
        json_escape_into(&mut s, &self.path);
        s.push('"');
        if !self.old_path.is_empty() {
            s.push_str(",\"from\":\"");
            json_escape_into(&mut s, &self.old_path);
            s.push('"');
        }
        s.push('}');
        s
    }

    /// Parse a journal entry from a JSON-line string.
    ///
    /// Minimal parser — handles only the format produced by `to_json_line()`.
    fn from_json_line(line: &str) -> Option<Self> {
        let seq = json_extract_u64(line, "\"seq\":")?;
        let ts = json_extract_u64(line, "\"ts\":")?;
        let etype_str = json_extract_str(line, "\"type\":\"")?;
        let event_type = JournalEventType::from_str(&etype_str)?;
        let path = json_extract_str(line, "\"path\":\"")?;
        let old_path = json_extract_str(line, "\"from\":\"").unwrap_or_default();
        Some(Self {
            seq,
            timestamp_ns: ts,
            event_type,
            path,
            old_path,
        })
    }
}

// ---------------------------------------------------------------------------
// Global journal state
// ---------------------------------------------------------------------------

/// Maximum entries in the in-memory ring buffer.
const JOURNAL_MAX_ENTRIES: usize = 1024;

/// Number of new entries that trigger an auto-flush to disk.
const FLUSH_THRESHOLD: usize = 64;

/// Path of the on-disk journal file.
const JOURNAL_FILE: &str = "/_JOURNAL";

struct JournalInner {
    /// Ring buffer of journal entries (oldest at head, newest at tail).
    /// Uses VecDeque so eviction of the oldest entry is O(1) instead
    /// of O(n) with Vec::remove(0).
    entries: VecDeque<JournalEntry>,
    /// Next sequence number to assign.
    next_seq: u64,
    /// Number of entries written since last flush.
    unflushed: usize,
    /// Whether the journal has been initialized (loaded from disk).
    initialized: bool,
}

static JOURNAL: Mutex<JournalInner> = Mutex::new(JournalInner {
    entries: VecDeque::new(),
    next_seq: 1,
    unflushed: 0,
    initialized: false,
});

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the journal by loading existing entries from disk.
///
/// Call once after the root filesystem is mounted.  If no journal file
/// exists, starts fresh at sequence 1.
pub fn init() {
    let mut journal = JOURNAL.lock();
    if journal.initialized {
        return;
    }

    match crate::fs::Vfs::read_file(JOURNAL_FILE) {
        Ok(data) => {
            let text = core::str::from_utf8(&data).unwrap_or("");
            let mut max_seq = 0u64;
            let mut count = 0usize;
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(entry) = JournalEntry::from_json_line(line) {
                    if entry.seq > max_seq {
                        max_seq = entry.seq;
                    }
                    journal.entries.push_back(entry);
                    count = count.wrapping_add(1);
                }
            }
            // Trim to max size if the file was huge — O(1) per pop.
            while journal.entries.len() > JOURNAL_MAX_ENTRIES {
                journal.entries.pop_front();
            }
            journal.next_seq = max_seq.wrapping_add(1);
            journal.initialized = true;
            crate::serial_println!(
                "[journal] Loaded {} entries from disk (next seq: {})",
                count, journal.next_seq
            );
        }
        Err(KernelError::NotFound) => {
            // No journal file — start fresh.
            journal.initialized = true;
            crate::serial_println!("[journal] No journal file found, starting fresh (seq 1)");
        }
        Err(e) => {
            // I/O error reading journal — start fresh but log the issue.
            journal.initialized = true;
            crate::serial_println!("[journal] Error reading journal file: {:?}, starting fresh", e);
        }
    }
}

/// Record a filesystem change event.
///
/// Called by the VFS after each mutating operation.
pub fn record(event_type: JournalEventType, path: &str) {
    record_with_old_path(event_type, path, "");
}

/// Record a rename event with the old path.
pub fn record_rename(old_path: &str, new_path: &str) {
    record_with_old_path(JournalEventType::Renamed, new_path, old_path);
}

/// Internal: record an event with an optional old path.
fn record_with_old_path(event_type: JournalEventType, path: &str, old_path: &str) {
    let mut journal = JOURNAL.lock();
    if !journal.initialized {
        return; // Not yet initialized — drop silently.
    }

    let seq = journal.next_seq;
    journal.next_seq = seq.wrapping_add(1);

    let timestamp_ns = crate::hpet::elapsed_ns();

    let entry = JournalEntry {
        seq,
        timestamp_ns,
        event_type,
        path: String::from(path),
        old_path: String::from(old_path),
    };

    journal.entries.push_back(entry);

    // Evict oldest if over capacity — O(1) per pop with VecDeque.
    while journal.entries.len() > JOURNAL_MAX_ENTRIES {
        journal.entries.pop_front();
    }

    journal.unflushed = journal.unflushed.wrapping_add(1);

    // Auto-flush when threshold reached.
    // Drop the lock first to avoid holding JOURNAL while writing to VFS.
    let should_flush = journal.unflushed >= FLUSH_THRESHOLD;
    if should_flush {
        let data = serialize_entries(&journal.entries);
        journal.unflushed = 0;
        drop(journal);
        // Best-effort flush — don't propagate errors.
        if let Err(e) = crate::fs::Vfs::write_file(JOURNAL_FILE, data.as_bytes()) {
            crate::serial_println!("[journal] Auto-flush failed: {:?}", e);
        }
    }
}

/// Get the current (latest) sequence number.
///
/// Returns 0 if no events have been recorded yet.
pub fn cursor() -> u64 {
    let journal = JOURNAL.lock();
    if journal.next_seq > 1 {
        journal.next_seq.wrapping_sub(1)
    } else {
        0
    }
}

/// Read all entries with sequence number > `since_seq`.
///
/// Returns `(entries, current_seq)`.  If entries were evicted from the
/// ring buffer since `since_seq`, the first returned entry's seq will
/// be > `since_seq + 1` — the caller can detect the gap.
pub fn read_since(since_seq: u64) -> (Vec<JournalEntry>, u64) {
    let journal = JOURNAL.lock();
    let current = if journal.next_seq > 1 {
        journal.next_seq.wrapping_sub(1)
    } else {
        0
    };

    let entries: Vec<JournalEntry> = journal
        .entries
        .iter()
        .filter(|e| e.seq > since_seq)
        .cloned()
        .collect();

    (entries, current)
}

/// Flush the journal to disk immediately.
///
/// Called before unmount or on explicit user request.
pub fn flush() -> KernelResult<()> {
    let journal = JOURNAL.lock();
    if !journal.initialized || journal.entries.is_empty() {
        return Ok(());
    }
    let data = serialize_entries(&journal.entries);
    let unflushed = journal.unflushed;
    drop(journal);

    crate::fs::Vfs::write_file(JOURNAL_FILE, data.as_bytes())?;

    // Clear unflushed counter.
    let mut journal = JOURNAL.lock();
    // Only clear if no new entries arrived while we were writing.
    if journal.unflushed == unflushed {
        journal.unflushed = 0;
    }

    Ok(())
}

/// Return statistics about the journal.
pub fn stats() -> (usize, u64) {
    let journal = JOURNAL.lock();
    (journal.entries.len(), journal.next_seq.saturating_sub(1))
}

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

/// Serialize all entries to a JSON-lines string.
fn serialize_entries(entries: &VecDeque<JournalEntry>) -> String {
    let mut buf = String::with_capacity(entries.len() * 128);
    for entry in entries {
        buf.push_str(&entry.to_json_line());
        buf.push('\n');
    }
    buf
}

/// Append a u64 as decimal digits to a string.
fn push_u64(s: &mut String, mut val: u64) {
    if val == 0 {
        s.push('0');
        return;
    }
    // Max u64 is 20 digits.
    let mut digits = [0u8; 20];
    let mut i = 0usize;
    while val > 0 {
        // SAFETY: val > 0 so val % 10 is 0-9, fits in u8.
        digits[i] = (val % 10) as u8;
        val /= 10;
        i = i.wrapping_add(1);
    }
    // Write digits in reverse (most significant first).
    while i > 0 {
        i = i.wrapping_sub(1);
        s.push((b'0' + digits[i]) as char);
    }
}

/// Escape a string for JSON (handles quotes, backslashes, control chars).
fn json_escape_into(s: &mut String, input: &str) {
    for c in input.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if c.is_control() => {
                // \u00XX for other control characters.
                s.push_str("\\u00");
                let byte = c as u32;
                let hi = (byte >> 4) & 0xF;
                let lo = byte & 0xF;
                s.push(hex_digit(hi));
                s.push(hex_digit(lo));
            }
            c => s.push(c),
        }
    }
}

fn hex_digit(n: u32) -> char {
    if n < 10 {
        (b'0' + n as u8) as char
    } else {
        (b'a' + (n as u8 - 10)) as char
    }
}

/// Extract a u64 value following a key prefix in a JSON string.
///
/// For `{"seq":42,...}` with prefix `"seq":`, returns `Some(42)`.
fn json_extract_u64(json: &str, prefix: &str) -> Option<u64> {
    let start = json.find(prefix)?;
    let after = json.get(start + prefix.len()..)?;
    let end = after.find(|c: char| !c.is_ascii_digit())?;
    let num_str = after.get(..end)?;
    // Manual u64 parse (no std).
    let mut val = 0u64;
    for b in num_str.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        val = val.checked_mul(10)?.checked_add(u64::from(b - b'0'))?;
    }
    Some(val)
}

/// Extract a quoted string value following a key prefix in a JSON string.
///
/// For `{"path":"hello"}` with prefix `"path":"`, returns `Some("hello")`.
fn json_extract_str(json: &str, prefix: &str) -> Option<String> {
    let start = json.find(prefix)?;
    let after = json.get(start + prefix.len()..)?;
    // Find the closing quote (handle escaped quotes).
    let mut result = String::new();
    let mut chars = after.chars();
    loop {
        match chars.next()? {
            '"' => return Some(result),
            '\\' => {
                match chars.next()? {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    other => {
                        result.push('\\');
                        result.push(other);
                    }
                }
            }
            c => result.push(c),
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test: verify journal record, read, and persistence.
#[allow(clippy::arithmetic_side_effects)]
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[journal] Running self-test...");

    // Initialize the journal (may already be done).
    init();

    // Record the current cursor position.
    let start_seq = cursor();
    crate::serial_println!("[journal]   Start cursor: {}", start_seq);

    // Record some test events.
    record(JournalEventType::Created, "/TEST_JOURNAL.TXT");
    record(JournalEventType::Modified, "/TEST_JOURNAL.TXT");
    record_rename("/TEST_JOURNAL.TXT", "/TEST_JOURNAL_NEW.TXT");
    record(JournalEventType::Deleted, "/TEST_JOURNAL_NEW.TXT");

    // Read back events since our start position.
    let (entries, current) = read_since(start_seq);
    crate::serial_println!(
        "[journal]   Read {} entries (current seq: {})",
        entries.len(),
        current
    );

    if entries.len() < 4 {
        crate::serial_println!(
            "[journal]   FAILED: expected at least 4 entries, got {}",
            entries.len()
        );
        return Err(KernelError::IoError);
    }

    // Verify the events are in order and have the right types.
    let last_four = &entries[entries.len() - 4..];
    if last_four[0].event_type != JournalEventType::Created
        || last_four[1].event_type != JournalEventType::Modified
        || last_four[2].event_type != JournalEventType::Renamed
        || last_four[3].event_type != JournalEventType::Deleted
    {
        crate::serial_println!("[journal]   FAILED: event types don't match");
        return Err(KernelError::IoError);
    }

    // Verify sequence numbers are monotonically increasing.
    for i in 1..last_four.len() {
        if last_four[i].seq <= last_four[i - 1].seq {
            crate::serial_println!("[journal]   FAILED: seq not monotonic");
            return Err(KernelError::IoError);
        }
    }

    // Verify rename has old_path.
    if last_four[2].old_path != "/TEST_JOURNAL.TXT" {
        crate::serial_println!(
            "[journal]   FAILED: rename old_path wrong: '{}'",
            last_four[2].old_path
        );
        return Err(KernelError::IoError);
    }

    // Test serialization round-trip.
    let entry = &last_four[0];
    let json = entry.to_json_line();
    let parsed = JournalEntry::from_json_line(&json);
    match parsed {
        Some(p) if p.seq == entry.seq
            && p.event_type == entry.event_type
            && p.path == entry.path =>
        {
            crate::serial_println!("[journal]   JSON round-trip: OK");
        }
        _ => {
            crate::serial_println!(
                "[journal]   FAILED: JSON round-trip. JSON: {}",
                json
            );
            return Err(KernelError::IoError);
        }
    }

    // Flush to disk and verify the file exists.
    flush()?;
    match crate::fs::Vfs::stat(JOURNAL_FILE) {
        Ok(stat) => {
            crate::serial_println!(
                "[journal]   Flushed to disk: {} bytes",
                stat.size
            );
        }
        Err(e) => {
            crate::serial_println!("[journal]   FAILED: journal file not found after flush: {:?}", e);
            return Err(e);
        }
    }

    // Report stats.
    let (entry_count, max_seq) = stats();
    crate::serial_println!(
        "[journal]   Stats: {} entries, max seq {}",
        entry_count,
        max_seq
    );

    crate::serial_println!("[journal] Self-test PASSED");
    Ok(())
}

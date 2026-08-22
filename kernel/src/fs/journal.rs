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
//!
//! ## Paths in a JSON-only container
//!
//! A path is a byte string (every byte but `/` and NUL is legal), but a JSON
//! string must be valid Unicode.  The two cannot be reconciled in one field
//! without either losing bytes or emitting ill-formed Unicode, so each path is
//! written as **up to two** fields:
//!
//! - `"path"` — always present, always valid UTF-8: the lossy rendering
//!   (undecodable bytes as U+FFFD), for human readers and for the
//!   overwhelmingly common case where the path *is* UTF-8 and the rendering is
//!   exact.
//! - `"path_hex"` — present **only** when the path is not valid UTF-8:
//!   lowercase hex of the exact bytes.  Its presence is the signal that
//!   `"path"` is a rendering and must not be used to reopen anything.
//!
//! Renames use `"from"` / `"from_hex"` the same way.  A reader that cares
//! about exactness checks for the `_hex` field first and falls back to `path`.
//! See design-decisions.md §"Journal path encoding" for why this beats
//! surrogate escapes (`\udcXX`) — those are ill-formed Unicode that strict
//! parsers, notably Go's `encoding/json`, silently replace with U+FFFD.
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

use crate::sync::Mutex;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

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
    pub path: PathBuf,
    /// Original path for rename events; `None` for other event types.
    ///
    /// This was a `PathBuf` that was empty for a non-rename.  The empty path
    /// is not a legal path, so it could never name a real source — but the
    /// only way to say so was a value check at every use site, which is what
    /// `Option` says in the type.
    pub old_path: Option<PathBuf>,
}

impl JournalEntry {
    /// Serialize to a JSON-lines compatible string (no newlines in output).
    ///
    /// Format: `{"seq":N,"ts":N,"type":"...","path":"..."}`, plus
    /// `"path_hex":"..."` when the path is not valid UTF-8, plus
    /// `"from"`/`"from_hex"` for renames.  See the module docs.
    fn to_json_line(&self) -> String {
        let mut s = String::with_capacity(128);
        s.push_str("{\"seq\":");
        push_u64(&mut s, self.seq);
        s.push_str(",\"ts\":");
        push_u64(&mut s, self.timestamp_ns);
        s.push_str(",\"type\":\"");
        s.push_str(self.event_type.as_str());
        s.push('"');
        json_push_path(&mut s, "path", &self.path);
        if let Some(old_path) = &self.old_path {
            json_push_path(&mut s, "from", old_path);
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
        let path = json_extract_path(line, "path")?;
        // A missing "from" is a non-rename, not a rename from the empty
        // path — so the absent case stays absent rather than defaulting.
        let old_path = json_extract_path(line, "from");
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

/// Private bookkeeping files that the journal must never report as changes.
///
/// The journal exists to answer "what changed in the filesystem?" for backup
/// agents, indexers and sync daemons. These three files are not filesystem
/// changes; they are the answering machinery's own notes, and reporting them
/// is not merely noise — for two of them it is a feedback loop that makes the
/// subsystem structurally unable to reach a steady state:
///
/// - `/_CHANGE_CURSORS` is persisted by `changetrack::changes()` *after* it
///   advances the caller's cursor, so that write lands at a sequence number
///   past the one just handed out. The next `changes()` call therefore returns
///   the previous call's bookkeeping, persists again, and arms the call after
///   it. `changes()` could never return empty, and an agent polling until
///   quiescent would poll forever, doing real disk writes each round.
/// - `/_JOURNAL` is written by this module's own auto-flush, which re-enters
///   `record` and immediately dirties the journal it just cleaned. It does not
///   recurse (`unflushed` is zeroed before the write), but it guarantees the
///   journal is never actually flushed-clean.
/// - `/_TRASH/_INDEX` does not self-feed — it is written only when a user
///   trashes something, and *that* deletion is separately and correctly
///   recorded — but the index update is still internal state, and a consumer
///   that saw it would be told the same event twice in two vocabularies.
///
/// Matched exactly rather than by an `_` prefix: a prefix rule would silently
/// swallow a real user file named `/_notes.txt`, and losing a genuine change
/// is the more expensive direction of error.
const INTERNAL_METADATA: [&str; 3] = [JOURNAL_FILE, "/_CHANGE_CURSORS", "/_TRASH/_INDEX"];

/// True when `path` is one of the kernel's own bookkeeping files.
///
/// Compared as bytes, because a path need not be UTF-8.
fn is_internal_metadata(path: &Path) -> bool {
    INTERNAL_METADATA
        .iter()
        .any(|p| path.as_bytes() == p.as_bytes())
}

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
                count,
                journal.next_seq
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
            crate::serial_println!(
                "[journal] Error reading journal file: {:?}, starting fresh",
                e
            );
        }
    }
}

/// Record a filesystem change event.
///
/// Called by the VFS after each mutating operation.
pub fn record(event_type: JournalEventType, path: impl AsRef<Path>) {
    record_with_old_path(event_type, path.as_ref(), None);
}

/// Record a rename event with the old path.
pub fn record_rename(old_path: impl AsRef<Path>, new_path: impl AsRef<Path>) {
    record_with_old_path(
        JournalEventType::Renamed,
        new_path.as_ref(),
        Some(old_path.as_ref()),
    );
}

/// Internal: record an event with an optional old path.
fn record_with_old_path(event_type: JournalEventType, path: &Path, old_path: Option<&Path>) {
    // Drop the journal's own bookkeeping before taking the lock. See
    // `INTERNAL_METADATA`: recording these turns the change-tracking
    // subsystem into a generator of the changes it reports.
    if is_internal_metadata(path) || old_path.is_some_and(is_internal_metadata) {
        return;
    }

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
        path: path.to_path_buf(),
        old_path: old_path.map(Path::to_path_buf),
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

/// Escape a *byte* string for JSON.
///
/// JSON strings are Unicode, so undecodable bytes are rendered as U+FFFD —
/// lossy, and deliberately so: [`json_push_path`] carries the exact bytes in a
/// companion `_hex` field, and this field exists to be read by a human.
fn json_escape_bytes_into(s: &mut String, input: &[u8]) {
    let mut rendered = String::with_capacity(input.len());
    // Formatting into a `String` cannot fail (its `fmt::Write` impl is
    // infallible), so there is no error path to propagate.
    let _ = core::fmt::Write::write_fmt(
        &mut rendered,
        format_args!("{}", Path::new(input).display()),
    );
    json_escape_into(s, &rendered);
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
        char::from(b'0'.wrapping_add(n as u8))
    } else {
        char::from(b'a'.wrapping_add((n as u8).wrapping_sub(10)))
    }
}

/// Append `,"<key>":"<lossy>"` and, when `p` is not valid UTF-8, also
/// `,"<key>_hex":"<lowercase hex of the exact bytes>"`.
///
/// A JSON string must be valid Unicode; a path need not be.  Rather than
/// corrupt the path (lossy-only) or emit ill-formed Unicode (surrogate
/// escapes), the exact bytes get their own field and the readable field stays
/// a legal JSON string.  The `_hex` field is omitted entirely for UTF-8 paths,
/// so ordinary journals are byte-identical to what this module produced before
/// paths became bytes.
fn json_push_path(s: &mut String, key: &str, p: &Path) {
    s.push_str(",\"");
    s.push_str(key);
    s.push_str("\":\"");
    json_escape_bytes_into(s, p.as_bytes());
    s.push('"');
    if core::str::from_utf8(p.as_bytes()).is_err() {
        s.push_str(",\"");
        s.push_str(key);
        s.push_str("_hex\":\"");
        for &b in p.as_bytes() {
            s.push(hex_digit(u32::from(b >> 4)));
            s.push(hex_digit(u32::from(b & 0x0F)));
        }
        s.push('"');
    }
}

/// Read a path field written by [`json_push_path`].
///
/// Prefers `<key>_hex` (exact bytes) and falls back to `<key>` (the lossy
/// rendering) when it is absent — which, by construction, only happens when
/// the rendering *is* the exact path.
fn json_extract_path(json: &str, key: &str) -> Option<PathBuf> {
    let mut hex_prefix = String::with_capacity(key.len().saturating_add(8));
    hex_prefix.push('"');
    hex_prefix.push_str(key);
    hex_prefix.push_str("_hex\":\"");
    if let Some(hex) = json_extract_str(json, &hex_prefix) {
        if let Some(bytes) = hex_decode(&hex) {
            return Some(PathBuf::from(bytes));
        }
        // A malformed `_hex` field means the exact bytes are unrecoverable.
        // Falling back to the lossy `path` would hand the caller a path that
        // names a *different* file (or none), so refuse the entry instead.
        return None;
    }
    let mut prefix = String::with_capacity(key.len().saturating_add(4));
    prefix.push('"');
    prefix.push_str(key);
    prefix.push_str("\":\"");
    json_extract_str(json, &prefix).map(PathBuf::from)
}

/// Decode a lowercase-or-uppercase hex string to bytes; `None` if it has an
/// odd length or a non-hex digit.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = hex_val(*pair.first()?)?;
        let lo = hex_val(*pair.get(1)?)?;
        out.push(hi.wrapping_shl(4) | lo);
    }
    Some(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(b.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(b.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
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
            '\\' => match chars.next()? {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                other => {
                    result.push('\\');
                    result.push(other);
                }
            },
            c => result.push(c),
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Marker embedded in every path [`self_test`] records, and the only paths its
/// assertions count.
///
/// The journal is a *shared* append-only log: [`record`] is called from every
/// real VFS create/write/delete/rename (see `fs::vfs`), not just from this
/// suite. Asserting absolute counts over it was safe only while the `fat_ok`
/// gate kept the suite off a live root — the same trap `notify::self_test` fell
/// into the first time it ran in CI (see known-issues.md). Any concurrent file
/// operation would otherwise land between this suite's own records and turn a
/// correct journal into a failed boot.
///
/// Matched as a *substring* rather than a prefix so that the near-miss path
/// below can still begin with `JOURNAL_FILE` — being a near-miss is the whole
/// point of that case, so it cannot be moved under a prefix of its own.
const PROBE_MARKER: &[u8] = b"JOURNAL_ST_";

/// True when `path` is one this suite recorded itself.
fn is_probe_path(path: &Path) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= PROBE_MARKER.len() && bytes.windows(PROBE_MARKER.len()).any(|w| w == PROBE_MARKER)
}

/// Entries recorded since `since_seq` by this suite, ignoring the rest of the
/// kernel's filesystem traffic.
fn probe_entries_since(since_seq: u64) -> Vec<JournalEntry> {
    let (mut entries, _) = read_since(since_seq);
    entries.retain(|e| is_probe_path(e.path.as_path()));
    entries
}

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
    record(JournalEventType::Created, "/JOURNAL_ST_TEST.TXT");
    record(JournalEventType::Modified, "/JOURNAL_ST_TEST.TXT");
    record_rename("/JOURNAL_ST_TEST.TXT", "/JOURNAL_ST_TEST_NEW.TXT");
    record(JournalEventType::Deleted, "/JOURNAL_ST_TEST_NEW.TXT");

    // Read back events since our start position.  Filtered to this suite's own
    // paths: a concurrent VFS operation anywhere in the kernel also records
    // here, and could otherwise land in the middle of the four below.
    let entries = probe_entries_since(start_seq);
    crate::serial_println!("[journal]   Read {} own entries", entries.len());

    if entries.len() != 4 {
        crate::serial_println!(
            "[journal]   FAILED: expected exactly 4 own entries, got {}",
            entries.len()
        );
        return Err(KernelError::IoError);
    }

    // Verify the events are in order and have the right types.
    let last_four = &entries[..];
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
    if last_four[2].old_path.as_deref() != Some(Path::new("/JOURNAL_ST_TEST.TXT")) {
        crate::serial_println!(
            "[journal]   FAILED: rename old_path wrong: {:?}",
            last_four[2].old_path
        );
        return Err(KernelError::IoError);
    }
    // A non-rename carries no source path at all, rather than an empty one.
    if last_four[0].old_path.is_some() {
        crate::serial_println!("[journal]   FAILED: non-rename carries an old_path");
        return Err(KernelError::IoError);
    }

    // The journal must not report its own bookkeeping.  Recording these once
    // made `changetrack::changes()` structurally unable to return empty: it
    // persists `/_CHANGE_CURSORS` *after* advancing the caller's cursor, so
    // the write landed past the sequence just handed out and became the next
    // call's "change", which persisted again, forever.  See INTERNAL_METADATA.
    {
        let before = cursor();
        for path in INTERNAL_METADATA {
            record(JournalEventType::Modified, path);
            record(JournalEventType::Created, path);
        }
        // A rename *away from* an internal file is equally invisible: it is
        // how the flush path would spell a replace-by-rename.
        record_rename(JOURNAL_FILE, JOURNAL_FILE);
        let (all_entries, _) = read_since(before);
        let reported: Vec<&JournalEntry> = all_entries
            .iter()
            .filter(|e| is_internal_metadata(e.path.as_path()))
            .collect();
        if !reported.is_empty() {
            crate::serial_println!(
                "[journal]   FAILED: {} internal-metadata event(s) reported, first {:?}",
                reported.len(),
                reported.first().map(|e| e.path.clone())
            );
            return Err(KernelError::IoError);
        }
        // Internal metadata must consume no sequence number at all.  Stated as
        // "the cursor did not move" this was only true on a quiet filesystem --
        // a concurrent VFS write legitimately consumes one.  Stated as "every
        // seq consumed since `before` is accounted for by an entry we can see",
        // it is both robust to that traffic and strictly stronger: a record
        // that burned a seq without producing a visible entry is exactly the
        // bug this checks for, whoever made it.
        if cursor() != before.wrapping_add(all_entries.len() as u64) {
            crate::serial_println!(
                "[journal]   FAILED: {} seq(s) consumed but {} entries visible",
                cursor().wrapping_sub(before),
                all_entries.len()
            );
            return Err(KernelError::IoError);
        }
        // A path that merely *starts* like one is a real user file and must
        // still be reported -- the exclusion is exact, not a prefix rule.
        // (It carries the probe marker too, so `probe_entries_since` sees it;
        // that is why the marker is matched as a substring rather than a
        // prefix -- this path has to keep starting with `JOURNAL_FILE`.)
        record(JournalEventType::Created, "/_JOURNAL_ST_NEARMISS.bak");
        let entries = probe_entries_since(before);
        if entries.len() != 1 {
            crate::serial_println!(
                "[journal]   FAILED: /_JOURNAL_ST_NEARMISS.bak is a user file, got {} entries",
                entries.len()
            );
            return Err(KernelError::IoError);
        }
        crate::serial_println!("[journal]   internal metadata not reported: OK");
    }

    // Test serialization round-trip.
    let entry = &last_four[0];
    let json = entry.to_json_line();
    let parsed = JournalEntry::from_json_line(&json);
    match parsed {
        Some(p)
            if p.seq == entry.seq && p.event_type == entry.event_type && p.path == entry.path =>
        {
            crate::serial_println!("[journal]   JSON round-trip: OK");
        }
        _ => {
            crate::serial_println!("[journal]   FAILED: JSON round-trip. JSON: {}", json);
            return Err(KernelError::IoError);
        }
    }

    // A non-UTF-8 path must survive the JSON round-trip byte-for-byte.  This
    // is the case the `_hex` companion field exists for: the `"path"` field
    // alone renders `\xff` as U+FFFD and could never name the file again.
    {
        let raw = Path::new(b"/TEST_JOURNAL_\xff\xfe.TXT".as_slice());
        let entry = JournalEntry {
            seq: 424_242,
            timestamp_ns: 7,
            event_type: JournalEventType::Renamed,
            path: raw.to_path_buf(),
            old_path: Some(PathBuf::from(b"/o\xffld".as_slice().to_vec())),
        };
        let json = entry.to_json_line();
        // The readable field is lossy; the hex field is not.  Both must be
        // present, and the line must still be valid UTF-8 (it is a `String`).
        if !json.contains("\"path_hex\":\"") || !json.contains("\"from_hex\":\"") {
            crate::serial_println!(
                "[journal]   FAILED: no _hex field for non-UTF-8 path: {}",
                json
            );
            return Err(KernelError::IoError);
        }
        match JournalEntry::from_json_line(&json) {
            Some(p) if p.path == entry.path && p.old_path == entry.old_path => {
                crate::serial_println!("[journal]   non-UTF-8 path round-trip: OK");
            }
            other => {
                crate::serial_println!(
                    "[journal]   FAILED: non-UTF-8 round-trip gave {:?}, json {}",
                    other.map(|p| p.path),
                    json
                );
                return Err(KernelError::IoError);
            }
        }
        // A pure-UTF-8 path must NOT grow a `_hex` field — ordinary journals
        // stay byte-identical to the pre-byte-path format.
        let plain = JournalEntry {
            seq: 1,
            timestamp_ns: 2,
            event_type: JournalEventType::Created,
            path: PathBuf::from("/plain.txt"),
            old_path: None,
        };
        let plain_json = plain.to_json_line();
        if plain_json.contains("_hex") {
            crate::serial_println!(
                "[journal]   FAILED: UTF-8 path grew a _hex field: {}",
                plain_json
            );
            return Err(KernelError::IoError);
        }
        if plain_json.as_str() != "{\"seq\":1,\"ts\":2,\"type\":\"create\",\"path\":\"/plain.txt\"}"
        {
            crate::serial_println!("[journal]   FAILED: unexpected JSON layout: {}", plain_json);
            return Err(KernelError::IoError);
        }
    }

    // Flush to disk and verify the file exists.
    //
    // This is the only part of the suite that touches a filesystem at all --
    // everything above is in-memory bookkeeping and pure JSON round-tripping --
    // so it is the only part with a precondition: `flush()` writes
    // `JOURNAL_FILE` at the root, which requires the root to be *writable*.
    // A probe write rather than a mount-flag check because that is the real
    // precondition: it accounts for a read-only mount, a quota, a file tag and
    // anything else that could stand between here and a successful write.
    let probe = "/_journal_writable_probe";
    if crate::fs::Vfs::write_file(probe, b"").is_ok() {
        // Absence is the expected end state, so a failure here is nothing to
        // report.
        let _ = crate::fs::Vfs::remove(probe);
        flush()?;
        match crate::fs::Vfs::stat(JOURNAL_FILE) {
            Ok(stat) => {
                crate::serial_println!("[journal]   Flushed to disk: {} bytes", stat.size);
            }
            Err(e) => {
                crate::serial_println!(
                    "[journal]   FAILED: journal file not found after flush: {:?}",
                    e
                );
                return Err(e);
            }
        }
    } else {
        crate::serial_println!("[journal]   Root is not writable — skipping flush-to-disk.");
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

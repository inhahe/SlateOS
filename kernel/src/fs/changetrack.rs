//! Persistent filesystem change tracking.
//!
//! Provides a bookmark-based system for applications to track filesystem
//! changes across restarts.  Built on top of the journal subsystem's
//! sequence numbers, this module lets any application (or OS service)
//! register a named "cursor" that remembers the last-seen journal sequence.
//! On the next query, only changes *after* that sequence are returned.
//!
//! ## Design Requirement (from design.txt)
//!
//! > "We need a way to not only hook filesystem changes while the program
//! > is running, but to detect if there were any filesystem changes (on
//! > the specified files/dirs of the specified types) since it last called
//! > some API function, even if the program was closed and reopened since
//! > then or the OS rebooted, etc."
//!
//! ## Architecture
//!
//! ```text
//! Application                changetrack                journal
//!     │                         │                         │
//!     ├─ register("backup") ───►│                         │
//!     │                         ├─ store cursor(seq=0) ──►│
//!     │                         │                         │
//!     ├─ changes("backup", ──►  │                         │
//!     │    filter)              ├─ read_since(cursor) ───►│
//!     │  ◄── [entries] ─────────┤◄── [entries, seq] ──────┤
//!     │                         ├─ update cursor ─────────►│
//!     │                         │                         │
//!     ├─ (reboot) ─ ─ ─ ─ ─ ─ ─│─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─│
//!     │                         │                         │
//!     ├─ changes("backup", ──►  │                         │
//!     │    filter)              ├─ read_since(saved_seq)─►│
//!     │  ◄── [entries] ─────────┤◄── [entries, seq] ──────┤
//! ```
//!
//! ## Persistence
//!
//! Cursors are saved to `/_CHANGE_CURSORS` as JSON-lines.  On init,
//! cursors are loaded from disk so they survive reboots.
//!
//! ## Usage
//!
//! ```text
//! changetrack register backup             — register a cursor
//! changetrack changes backup              — show changes since last check
//! changetrack changes backup /etc         — only changes under /etc
//! changetrack peek backup                 — show changes without advancing cursor
//! changetrack reset backup                — reset cursor to current position
//! changetrack status                      — list all cursors
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::fs::journal::{self, JournalEntry, JournalEventType};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A named cursor tracking a position in the journal.
#[derive(Debug, Clone)]
struct Cursor {
    /// Unique name for this cursor (e.g., "backup", "sync-agent").
    name: String,
    /// Last-seen journal sequence number.  Changes with seq > this
    /// are considered "new" for this cursor.
    last_seq: u64,
    /// When this cursor was registered (epoch nanoseconds).
    created_ns: u64,
    /// When this cursor was last advanced (epoch nanoseconds).
    last_advanced_ns: u64,
    /// How many times this cursor has been advanced.
    advance_count: u64,
}

/// A filter for querying changes.
#[derive(Debug, Clone, Default)]
pub struct ChangeFilter {
    /// Only include changes under these path prefixes.
    /// Empty means "all paths".
    pub path_prefixes: Vec<String>,
    /// Only include these event types.
    /// Empty means "all types".
    pub event_types: Vec<JournalEventType>,
    /// Maximum number of entries to return (0 = unlimited).
    pub limit: usize,
}

/// A change entry returned to the caller.
#[derive(Debug, Clone)]
pub struct Change {
    /// Journal sequence number.
    pub seq: u64,
    /// Timestamp (nanoseconds since boot).
    pub timestamp_ns: u64,
    /// Type of change.
    pub event_type: JournalEventType,
    /// Affected path.
    pub path: String,
    /// Original path for renames.
    pub old_path: String,
}

impl From<&JournalEntry> for Change {
    fn from(e: &JournalEntry) -> Self {
        Self {
            seq: e.seq,
            timestamp_ns: e.timestamp_ns,
            event_type: e.event_type,
            path: e.path.clone(),
            old_path: e.old_path.clone(),
        }
    }
}

/// Result of a changes query.
#[derive(Debug, Clone)]
pub struct ChangeResult {
    /// The changes matching the filter.
    pub changes: Vec<Change>,
    /// The new cursor position (highest seq seen).
    pub new_seq: u64,
    /// Whether some entries may have been lost (evicted from journal
    /// ring buffer before this cursor could read them).
    pub gap_detected: bool,
    /// Total entries in journal that matched (before limit applied).
    pub total_matched: usize,
}

/// Public info about a cursor.
#[derive(Debug, Clone)]
pub struct CursorInfo {
    pub name: String,
    pub last_seq: u64,
    pub created_ns: u64,
    pub last_advanced_ns: u64,
    pub advance_count: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Path where cursors are persisted.
const CURSOR_FILE: &str = "/_CHANGE_CURSORS";

/// Maximum number of registered cursors.
const MAX_CURSORS: usize = 256;

struct ChangeTrackInner {
    cursors: BTreeMap<String, Cursor>,
    initialized: bool,
}

static STATE: Mutex<ChangeTrackInner> = Mutex::new(ChangeTrackInner {
    cursors: BTreeMap::new(),
    initialized: false,
});

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the change tracking system by loading persisted cursors.
///
/// Should be called after the journal and VFS are initialized.
pub fn init() {
    let mut state = STATE.lock();
    if state.initialized {
        return;
    }

    // Try to load cursors from disk.
    if let Ok(data) = crate::fs::Vfs::read_file(CURSOR_FILE) {
        if let Ok(text) = core::str::from_utf8(&data) {
            for line in text.lines() {
                if let Some(cursor) = parse_cursor_line(line) {
                    state.cursors.insert(cursor.name.clone(), cursor);
                }
            }
        }
    }

    state.initialized = true;
    serial_println!(
        "[changetrack] Initialized with {} cursors",
        state.cursors.len()
    );
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register a new named cursor at the current journal position.
///
/// If the cursor already exists, this is a no-op (returns Ok).
pub fn register(name: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(KernelError::InvalidArgument);
    }

    let mut state = STATE.lock();
    ensure_init(&mut state);

    if state.cursors.contains_key(name) {
        return Ok(()); // Already registered.
    }

    if state.cursors.len() >= MAX_CURSORS {
        return Err(KernelError::DiskFull); // Reuse for "too many cursors".
    }

    let current_seq = journal::cursor();
    let now = crate::timekeeping::clock_realtime();

    state.cursors.insert(
        String::from(name),
        Cursor {
            name: String::from(name),
            last_seq: current_seq,
            created_ns: now,
            last_advanced_ns: now,
            advance_count: 0,
        },
    );

    // Persist to disk (best-effort).
    let serialized = serialize_cursors(&state.cursors);
    drop(state);
    let _ = crate::fs::Vfs::write_file(CURSOR_FILE, serialized.as_bytes());

    Ok(())
}

/// Query changes since the cursor's last position, advancing the cursor.
///
/// Returns all journal entries newer than the cursor's saved position,
/// filtered by the given filter.  Advances the cursor to the newest
/// entry seen.
pub fn changes(name: &str, filter: &ChangeFilter) -> KernelResult<ChangeResult> {
    query_impl(name, filter, true)
}

/// Query changes without advancing the cursor ("peek").
///
/// Same as `changes()` but does not update the cursor position.
pub fn peek(name: &str, filter: &ChangeFilter) -> KernelResult<ChangeResult> {
    query_impl(name, filter, false)
}

/// Reset a cursor to the current journal position (skip all pending changes).
pub fn reset(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    ensure_init(&mut state);

    let cursor = state
        .cursors
        .get_mut(name)
        .ok_or(KernelError::NotFound)?;

    cursor.last_seq = journal::cursor();
    cursor.last_advanced_ns = crate::timekeeping::clock_realtime();

    let serialized = serialize_cursors(&state.cursors);
    drop(state);
    let _ = crate::fs::Vfs::write_file(CURSOR_FILE, serialized.as_bytes());

    Ok(())
}

/// Unregister a cursor, removing it permanently.
pub fn unregister(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    ensure_init(&mut state);

    if state.cursors.remove(name).is_none() {
        return Err(KernelError::NotFound);
    }

    let serialized = serialize_cursors(&state.cursors);
    drop(state);
    let _ = crate::fs::Vfs::write_file(CURSOR_FILE, serialized.as_bytes());

    Ok(())
}

/// List all registered cursors.
pub fn list() -> Vec<CursorInfo> {
    let mut state = STATE.lock();
    ensure_init(&mut state);

    state
        .cursors
        .values()
        .map(|c| CursorInfo {
            name: c.name.clone(),
            last_seq: c.last_seq,
            created_ns: c.created_ns,
            last_advanced_ns: c.last_advanced_ns,
            advance_count: c.advance_count,
        })
        .collect()
}

/// Get info about a specific cursor.
pub fn info(name: &str) -> KernelResult<CursorInfo> {
    let mut state = STATE.lock();
    ensure_init(&mut state);

    let c = state.cursors.get(name).ok_or(KernelError::NotFound)?;
    Ok(CursorInfo {
        name: c.name.clone(),
        last_seq: c.last_seq,
        created_ns: c.created_ns,
        last_advanced_ns: c.last_advanced_ns,
        advance_count: c.advance_count,
    })
}

/// Get the count of registered cursors.
pub fn cursor_count() -> usize {
    let state = STATE.lock();
    state.cursors.len()
}

/// Persist all cursors to disk.  Called automatically on register/advance,
/// but can be called explicitly (e.g., before shutdown).
pub fn flush() -> KernelResult<()> {
    let state = STATE.lock();
    let serialized = serialize_cursors(&state.cursors);
    drop(state);
    crate::fs::Vfs::write_file(CURSOR_FILE, serialized.as_bytes())
}

// ---------------------------------------------------------------------------
// Internal implementation
// ---------------------------------------------------------------------------

/// Ensure the subsystem is initialized (lazy init on first use).
fn ensure_init(state: &mut ChangeTrackInner) {
    if !state.initialized {
        // Try to load from disk.
        if let Ok(data) = crate::fs::Vfs::read_file(CURSOR_FILE) {
            if let Ok(text) = core::str::from_utf8(&data) {
                for line in text.lines() {
                    if let Some(cursor) = parse_cursor_line(line) {
                        state.cursors.insert(cursor.name.clone(), cursor);
                    }
                }
            }
        }
        state.initialized = true;
    }
}

/// Core query implementation shared by changes() and peek().
fn query_impl(name: &str, filter: &ChangeFilter, advance: bool) -> KernelResult<ChangeResult> {
    let cursor_seq = {
        let mut state = STATE.lock();
        ensure_init(&mut state);
        let cursor = state.cursors.get(name).ok_or(KernelError::NotFound)?;
        cursor.last_seq
    };

    // Read from journal.
    let (entries, current_seq) = journal::read_since(cursor_seq);

    // Detect if entries were lost (gap between cursor position and oldest
    // available entry).
    let gap_detected = if entries.is_empty() {
        false
    } else if let Some(first) = entries.first() {
        // If the first entry's seq is much larger than cursor_seq + 1,
        // entries were evicted.
        first.seq > cursor_seq.saturating_add(1)
    } else {
        false
    };

    // Apply filter.
    let mut matched: Vec<Change> = Vec::new();
    let mut total_matched: usize = 0;

    for entry in &entries {
        if !matches_filter(entry, filter) {
            continue;
        }
        total_matched = total_matched.saturating_add(1);

        if filter.limit == 0 || matched.len() < filter.limit {
            matched.push(Change::from(entry));
        }
    }

    let new_seq = current_seq;

    // Advance cursor if requested.
    if advance {
        let mut state = STATE.lock();
        if let Some(cursor) = state.cursors.get_mut(name) {
            cursor.last_seq = new_seq;
            cursor.last_advanced_ns = crate::timekeeping::clock_realtime();
            cursor.advance_count = cursor.advance_count.saturating_add(1);
        }
        let serialized = serialize_cursors(&state.cursors);
        drop(state);
        // Best-effort persist.
        let _ = crate::fs::Vfs::write_file(CURSOR_FILE, serialized.as_bytes());
    }

    Ok(ChangeResult {
        changes: matched,
        new_seq,
        gap_detected,
        total_matched,
    })
}

/// Check if a journal entry matches the given filter.
fn matches_filter(entry: &JournalEntry, filter: &ChangeFilter) -> bool {
    // Check event type filter.
    if !filter.event_types.is_empty()
        && !filter.event_types.contains(&entry.event_type)
    {
        return false;
    }

    // Check path prefix filter.
    if !filter.path_prefixes.is_empty() {
        // Canonical subtree predicate; see fs::pathutil.
        let path_matches = filter
            .path_prefixes
            .iter()
            .any(|pfx| crate::fs::pathutil::path_in_subtree(entry.path.as_str(), pfx.as_str()));
        if !path_matches {
            // For renames, also check old_path.
            if !entry.old_path.is_empty() {
                let old_matches = filter.path_prefixes.iter().any(|pfx| {
                    crate::fs::pathutil::path_in_subtree(entry.old_path.as_str(), pfx.as_str())
                });
                if !old_matches {
                    return false;
                }
            } else {
                return false;
            }
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

/// Serialize all cursors to a JSON-lines string for persistence.
fn serialize_cursors(cursors: &BTreeMap<String, Cursor>) -> String {
    let mut out = String::with_capacity(cursors.len() * 128);
    for cursor in cursors.values() {
        out.push_str("{\"name\":\"");
        json_escape_into(&mut out, &cursor.name);
        out.push_str("\",\"seq\":");
        push_u64(&mut out, cursor.last_seq);
        out.push_str(",\"created\":");
        push_u64(&mut out, cursor.created_ns);
        out.push_str(",\"advanced\":");
        push_u64(&mut out, cursor.last_advanced_ns);
        out.push_str(",\"count\":");
        push_u64(&mut out, cursor.advance_count);
        out.push_str("}\n");
    }
    out
}

/// Parse a single cursor JSON line.
fn parse_cursor_line(line: &str) -> Option<Cursor> {
    let name = json_extract_str(line, "\"name\":\"")?;
    let last_seq = json_extract_u64(line, "\"seq\":")?;
    let created_ns = json_extract_u64(line, "\"created\":").unwrap_or(0);
    let last_advanced_ns = json_extract_u64(line, "\"advanced\":").unwrap_or(0);
    let advance_count = json_extract_u64(line, "\"count\":").unwrap_or(0);

    Some(Cursor {
        name,
        last_seq,
        created_ns,
        last_advanced_ns,
        advance_count,
    })
}

// ---------------------------------------------------------------------------
// JSON helpers (minimal, no alloc-heavy parser)
// ---------------------------------------------------------------------------

fn json_escape_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
}

fn push_u64(out: &mut String, val: u64) {
    use core::fmt::Write;
    let _ = write!(out, "{}", val);
}

/// Extract a u64 value after a key prefix.
fn json_extract_u64(line: &str, prefix: &str) -> Option<u64> {
    let start = line.find(prefix)? + prefix.len();
    let rest = &line[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse::<u64>().ok()
}

/// Extract a string value after a `"key":"` prefix (stops at next unescaped quote).
fn json_extract_str(line: &str, prefix: &str) -> Option<String> {
    let start = line.find(prefix)? + prefix.len();
    let rest = &line[start..];
    let mut result = String::new();
    let mut chars = rest.chars();
    loop {
        match chars.next()? {
            '"' => break,
            '\\' => {
                match chars.next()? {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    c => {
                        result.push('\\');
                        result.push(c);
                    }
                }
            }
            c => result.push(c),
        }
    }
    Some(result)
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    serial_println!("[changetrack] Running self-test...");

    test_register_unregister();
    test_changes_basic();
    test_peek_no_advance();
    test_filter_path();
    test_filter_event_type();
    test_reset();
    test_persistence();
    test_gap_detection();

    serial_println!("[changetrack] Self-test passed (8 tests).");
    Ok(())
}

fn test_register_unregister() {
    // Register a new cursor.
    register("test_ct_1").expect("register failed");

    // Duplicate register should be OK (no-op).
    register("test_ct_1").expect("duplicate register should succeed");

    // Info should work.
    let i = info("test_ct_1").expect("info failed");
    assert_eq!(i.name, "test_ct_1");

    // Unregister.
    unregister("test_ct_1").expect("unregister failed");
    assert!(info("test_ct_1").is_err());

    serial_println!("[changetrack]   register/unregister: ok");
}

fn test_changes_basic() {
    register("test_ct_2").expect("register");

    // Create some journal entries.
    journal::record(JournalEventType::Created, "/tmp/ct_test_file.txt");
    journal::record(JournalEventType::Modified, "/tmp/ct_test_file.txt");

    // Query changes.
    let filter = ChangeFilter::default();
    let result = changes("test_ct_2", &filter).expect("changes failed");

    // Should see at least the 2 entries we just created.
    assert!(result.changes.len() >= 2, "expected at least 2 changes, got {}", result.changes.len());

    // Query again — should be empty since cursor advanced.
    let result2 = changes("test_ct_2", &filter).expect("changes 2 failed");
    assert!(result2.changes.is_empty(), "expected 0 changes after advance");

    unregister("test_ct_2").expect("unregister");
    serial_println!("[changetrack]   changes basic: ok");
}

fn test_peek_no_advance() {
    register("test_ct_3").expect("register");

    journal::record(JournalEventType::Created, "/tmp/ct_peek_test.txt");

    let filter = ChangeFilter::default();

    // Peek should return changes.
    let r1 = peek("test_ct_3", &filter).expect("peek 1");
    assert!(!r1.changes.is_empty());

    // Peek again — should still return same changes (cursor not advanced).
    let r2 = peek("test_ct_3", &filter).expect("peek 2");
    assert_eq!(r1.changes.len(), r2.changes.len());

    // Now consume with changes().
    let r3 = changes("test_ct_3", &filter).expect("changes");
    assert!(!r3.changes.is_empty());

    // Now peek should be empty.
    let r4 = peek("test_ct_3", &filter).expect("peek 3");
    assert!(r4.changes.is_empty());

    unregister("test_ct_3").expect("unregister");
    serial_println!("[changetrack]   peek no advance: ok");
}

fn test_filter_path() {
    register("test_ct_4").expect("register");

    journal::record(JournalEventType::Created, "/etc/ct_test_a.conf");
    journal::record(JournalEventType::Created, "/tmp/ct_test_b.tmp");
    journal::record(JournalEventType::Modified, "/etc/ct_test_c.conf");

    // Filter to only /etc.
    let filter = ChangeFilter {
        path_prefixes: alloc::vec![String::from("/etc")],
        ..ChangeFilter::default()
    };

    let result = changes("test_ct_4", &filter).expect("changes");

    // Should only see /etc entries.
    for c in &result.changes {
        assert!(
            c.path.starts_with("/etc"),
            "unexpected path: {}",
            c.path
        );
    }
    assert!(result.changes.len() >= 2, "expected at least 2 /etc changes");

    unregister("test_ct_4").expect("unregister");
    serial_println!("[changetrack]   filter path: ok");
}

fn test_filter_event_type() {
    register("test_ct_5").expect("register");

    journal::record(JournalEventType::Created, "/tmp/ct_type_a.txt");
    journal::record(JournalEventType::Modified, "/tmp/ct_type_b.txt");
    journal::record(JournalEventType::Deleted, "/tmp/ct_type_c.txt");

    // Filter to only Created events.
    let filter = ChangeFilter {
        event_types: alloc::vec![JournalEventType::Created],
        ..ChangeFilter::default()
    };

    let result = changes("test_ct_5", &filter).expect("changes");

    for c in &result.changes {
        assert_eq!(c.event_type, JournalEventType::Created);
    }
    assert!(!result.changes.is_empty());

    unregister("test_ct_5").expect("unregister");
    serial_println!("[changetrack]   filter event type: ok");
}

fn test_reset() {
    register("test_ct_6").expect("register");

    journal::record(JournalEventType::Created, "/tmp/ct_reset_test.txt");

    // Peek to confirm there are changes.
    let filter = ChangeFilter::default();
    let r1 = peek("test_ct_6", &filter).expect("peek");
    assert!(!r1.changes.is_empty());

    // Reset — skips all pending changes.
    reset("test_ct_6").expect("reset");

    // Now peek should be empty.
    let r2 = peek("test_ct_6", &filter).expect("peek after reset");
    assert!(r2.changes.is_empty());

    unregister("test_ct_6").expect("unregister");
    serial_println!("[changetrack]   reset: ok");
}

fn test_persistence() {
    // Register a cursor and verify it's in the list.
    register("test_ct_persist").expect("register");

    let cursors = list();
    assert!(
        cursors.iter().any(|c| c.name == "test_ct_persist"),
        "cursor should be in list"
    );

    // Verify cursor_count.
    assert!(cursor_count() > 0);

    unregister("test_ct_persist").expect("unregister");
    serial_println!("[changetrack]   persistence: ok");
}

fn test_gap_detection() {
    // We can't easily force journal eviction in a test, but we can
    // verify the gap detection logic by checking that gap_detected is
    // false when the cursor is current.
    register("test_ct_gap").expect("register");

    journal::record(JournalEventType::Created, "/tmp/ct_gap_test.txt");

    let filter = ChangeFilter::default();
    let result = changes("test_ct_gap", &filter).expect("changes");

    // With a fresh cursor close to the head, there should be no gap.
    assert!(!result.gap_detected, "should not detect gap on fresh cursor");

    unregister("test_ct_gap").expect("unregister");
    serial_println!("[changetrack]   gap detection: ok");
}

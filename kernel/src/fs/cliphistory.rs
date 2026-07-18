//! Clipboard history — persistent clipboard with search and pinning.
//!
//! Records clipboard copies with timestamps, supports text/image/file
//! entries, pinning frequently-used items, and search across history.
//!
//! ## Architecture
//!
//! ```text
//! User copies (Ctrl+C)
//!   → clipboard::copy() → cliphistory::record(entry)
//!
//! Clipboard history panel (Win+V equivalent)
//!   → cliphistory::list() → show history
//!   → cliphistory::paste(id) → paste from history
//!
//! Integration:
//!   → clipboard (current clipboard content)
//!   → hotkeys (Win+V to open history panel)
//!   → datausage (history size tracking)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Type of clipboard entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipType {
    Text,
    Image,
    FilePath,
    Html,
    RichText,
}

impl ClipType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Image => "Image",
            Self::FilePath => "File",
            Self::Html => "HTML",
            Self::RichText => "Rich Text",
        }
    }
}

/// A clipboard history entry.
#[derive(Debug, Clone)]
pub struct ClipEntry {
    /// Entry ID.
    pub id: u64,
    /// Content type.
    pub clip_type: ClipType,
    /// Content (text or path; images store path reference).
    pub content: String,
    /// Content size in bytes.
    pub size_bytes: u64,
    /// Source application.
    pub source_app: String,
    /// Timestamp (ns since boot).
    pub timestamp_ns: u64,
    /// Whether this entry is pinned.
    pub pinned: bool,
    /// Number of times pasted from history.
    pub paste_count: u32,
}

/// Clipboard history configuration.
#[derive(Debug, Clone)]
pub struct ClipHistoryConfig {
    pub enabled: bool,
    /// Max entries to keep.
    pub max_entries: usize,
    /// Max total size in bytes.
    pub max_size_bytes: u64,
    /// Sync across devices.
    pub sync_enabled: bool,
    /// Clear on lock screen.
    pub clear_on_lock: bool,
    /// Exclude passwords (content from password fields).
    pub exclude_passwords: bool,
}

impl Default for ClipHistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 100,
            max_size_bytes: 10 * 1024 * 1024,
            sync_enabled: false,
            clear_on_lock: false,
            exclude_passwords: true,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: ClipHistoryConfig,
    entries: Vec<ClipEntry>,
    next_id: u64,
    total_copies: u64,
    total_pastes: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        config: ClipHistoryConfig::default(),
        entries: Vec::new(),
        next_id: 1,
        total_copies: 0,
        total_pastes: 0,
        ops: 0,
    });
}

/// Record a new clipboard entry.
pub fn record(clip_type: ClipType, content: &str, source_app: &str) -> KernelResult<u64> {
    with_state(|state| {
        if !state.config.enabled { return Ok(0); }

        let id = state.next_id;
        state.next_id += 1;

        // Don't record duplicates of the most recent entry.
        if let Some(last) = state.entries.last() {
            if last.content == content && last.clip_type == clip_type {
                return Ok(last.id);
            }
        }

        let size = content.len() as u64;

        state.entries.push(ClipEntry {
            id,
            clip_type,
            content: String::from(content),
            size_bytes: size,
            source_app: String::from(source_app),
            timestamp_ns: crate::hpet::elapsed_ns(),
            pinned: false,
            paste_count: 0,
        });

        state.total_copies += 1;

        // Enforce max entries (keep pinned).
        while state.entries.len() > state.config.max_entries {
            if let Some(pos) = state.entries.iter().position(|e| !e.pinned) {
                state.entries.remove(pos);
            } else {
                break;
            }
        }

        // Enforce max size.
        let total_size: u64 = state.entries.iter().map(|e| e.size_bytes).sum();
        if total_size > state.config.max_size_bytes {
            // Remove oldest non-pinned.
            while state.entries.iter().map(|e| e.size_bytes).sum::<u64>() > state.config.max_size_bytes {
                if let Some(pos) = state.entries.iter().position(|e| !e.pinned) {
                    state.entries.remove(pos);
                } else {
                    break;
                }
            }
        }

        Ok(id)
    })
}

/// Paste from history (returns the content).
pub fn paste(id: u64) -> KernelResult<String> {
    with_state(|state| {
        let entry = state.entries.iter_mut().find(|e| e.id == id)
            .ok_or(KernelError::NotFound)?;
        entry.paste_count += 1;
        state.total_pastes += 1;
        Ok(entry.content.clone())
    })
}

/// Pin an entry (won't be evicted).
pub fn pin(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.entries.iter_mut().find(|e| e.id == id)
            .ok_or(KernelError::NotFound)?;
        entry.pinned = true;
        Ok(())
    })
}

/// Unpin an entry.
pub fn unpin(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.entries.iter_mut().find(|e| e.id == id)
            .ok_or(KernelError::NotFound)?;
        entry.pinned = false;
        Ok(())
    })
}

/// Delete an entry.
pub fn delete(id: u64) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.entries.iter().position(|e| e.id == id)
            .ok_or(KernelError::NotFound)?;
        state.entries.remove(pos);
        Ok(())
    })
}

/// Clear all non-pinned entries.
pub fn clear() -> KernelResult<usize> {
    with_state(|state| {
        let before = state.entries.len();
        state.entries.retain(|e| e.pinned);
        Ok(before - state.entries.len())
    })
}

/// Search entries by content substring.
pub fn search(query: &str) -> Vec<ClipEntry> {
    let guard = STATE.lock();
    guard.as_ref().map_or(Vec::new(), |s| {
        s.entries.iter()
            .filter(|e| {
                let content_lower: String = e.content.chars().map(|c| {
                    if c.is_ascii_uppercase() { (c as u8 + 32) as char } else { c }
                }).collect();
                let query_lower: String = query.chars().map(|c| {
                    if c.is_ascii_uppercase() { (c as u8 + 32) as char } else { c }
                }).collect();
                content_lower.contains(&query_lower)
            })
            .cloned()
            .collect()
    })
}

/// List recent entries (most recent first).
pub fn list(count: usize) -> Vec<ClipEntry> {
    let guard = STATE.lock();
    guard.as_ref().map_or(Vec::new(), |s| {
        let start = if s.entries.len() > count { s.entries.len() - count } else { 0 };
        s.entries[start..].iter().rev().cloned().collect()
    })
}

/// Get config.
pub fn get_config() -> ClipHistoryConfig {
    STATE.lock().as_ref().map_or(ClipHistoryConfig::default(), |s| s.config.clone())
}

/// Set enabled.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.enabled = enabled; Ok(()) })
}

/// Statistics: (entry_count, pinned_count, total_copies, total_pastes, size_bytes, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let pinned = s.entries.iter().filter(|e| e.pinned).count();
            let size: u64 = s.entries.iter().map(|e| e.size_bytes).sum();
            (s.entries.len(), pinned, s.total_copies, s.total_pastes, size, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cliphistory::self_test() — running tests...");
    init_defaults();

    // 1: Empty initially.
    assert!(list(10).is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Record entry.
    let id1 = record(ClipType::Text, "Hello, world!", "editor").expect("record 1");
    assert!(id1 > 0);
    crate::serial_println!("  [2/11] record: OK");

    // 3: Record more.
    let id2 = record(ClipType::Text, "Second copy", "browser").expect("record 2");
    let id3 = record(ClipType::FilePath, "/home/user/doc.txt", "filemanager").expect("record 3");
    assert_eq!(list(10).len(), 3);
    crate::serial_println!("  [3/11] multiple records: OK");

    // 4: Paste from history.
    let content = paste(id1).expect("paste");
    assert_eq!(content, "Hello, world!");
    crate::serial_println!("  [4/11] paste: OK");

    // 5: Duplicate not recorded.
    record(ClipType::FilePath, "/home/user/doc.txt", "filemanager").expect("dup");
    assert_eq!(list(10).len(), 3); // Still 3.
    crate::serial_println!("  [5/11] no duplicate: OK");

    // 6: Pin entry.
    pin(id2).expect("pin");
    let entries = list(10);
    let pinned = entries.iter().find(|e| e.id == id2).expect("find pinned");
    assert!(pinned.pinned);
    crate::serial_println!("  [6/11] pin: OK");

    // 7: Search.
    let results = search("Hello");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, id1);
    crate::serial_println!("  [7/11] search: OK");

    // 8: Delete entry.
    delete(id3).expect("delete");
    assert_eq!(list(10).len(), 2);
    crate::serial_println!("  [8/11] delete: OK");

    // 9: Clear (keeps pinned).
    let cleared = clear().expect("clear");
    assert_eq!(cleared, 1); // Removed id1, kept pinned id2.
    assert_eq!(list(10).len(), 1);
    crate::serial_println!("  [9/11] clear keeps pinned: OK");

    // 10: Unpin.
    unpin(id2).expect("unpin");
    let entries = list(10);
    assert!(!entries[0].pinned);
    crate::serial_println!("  [10/11] unpin: OK");

    // 11: Stats.
    let (count, pinned, copies, pastes, _, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(pinned, 0);
    assert!(copies >= 3);
    assert!(pastes >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("cliphistory::self_test() — all 11 tests passed");
}

//! Multi-Clipboard — clipboard history with pinning and slots.
//!
//! Extends the system clipboard with history, pinned items, and named slots
//! for quick access to frequently-used clipboard entries.
//!
//! ## Architecture
//!
//! ```text
//! User copies content
//!   → multiclip::push(content) → add to history
//!   → multiclip::paste(index) → retrieve from history
//!   → multiclip::pin(index) → pin item
//!
//! Integration:
//!   → clipboard (system clipboard)
//!   → cliphistory (clipboard history)
//!   → kbshortcuts (paste shortcuts)
//!   → contextmenu (paste menu)
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

/// Clipboard content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    PlainText,
    RichText,
    Html,
    Image,
    FilePath,
    Binary,
}

impl ContentType {
    pub fn label(self) -> &'static str {
        match self {
            Self::PlainText => "Text",
            Self::RichText => "Rich Text",
            Self::Html => "HTML",
            Self::Image => "Image",
            Self::FilePath => "File Path",
            Self::Binary => "Binary",
        }
    }
}

/// A clipboard history entry.
#[derive(Debug, Clone)]
pub struct ClipEntry {
    pub id: u32,
    pub content: String,
    pub content_type: ContentType,
    pub size_bytes: usize,
    pub timestamp_ns: u64,
    pub pinned: bool,
    pub paste_count: u32,
    /// Optional named slot (e.g., "address", "signature").
    pub slot_name: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HISTORY: usize = 100;
const MAX_CONTENT_SIZE: usize = 65536;

struct State {
    entries: Vec<ClipEntry>,
    next_id: u32,
    global_enabled: bool,
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
        entries: Vec::new(),
        next_id: 1,
        global_enabled: true,
        total_copies: 0,
        total_pastes: 0,
        ops: 0,
    });
}

/// Push content to clipboard history.
pub fn push(content: &str, content_type: ContentType) -> KernelResult<u32> {
    with_state(|state| {
        if !state.global_enabled { return Ok(0); }
        let now = crate::hpet::elapsed_ns();
        let size = content.len().min(MAX_CONTENT_SIZE);
        let truncated = if content.len() > MAX_CONTENT_SIZE {
            &content[..MAX_CONTENT_SIZE]
        } else {
            content
        };
        // Don't add duplicates of the most recent entry.
        if let Some(last) = state.entries.last() {
            if last.content == truncated && last.content_type == content_type {
                state.total_copies += 1;
                return Ok(last.id);
            }
        }
        // Evict oldest non-pinned if at capacity.
        while state.entries.len() >= MAX_HISTORY {
            if let Some(idx) = state.entries.iter().position(|e| !e.pinned) {
                state.entries.remove(idx);
            } else {
                return Err(KernelError::ResourceExhausted);
            }
        }
        let id = state.next_id;
        state.next_id += 1;
        state.entries.push(ClipEntry {
            id,
            content: String::from(truncated),
            content_type,
            size_bytes: size,
            timestamp_ns: now,
            pinned: false,
            paste_count: 0,
            slot_name: String::new(),
        });
        state.total_copies += 1;
        Ok(id)
    })
}

/// Paste (retrieve) from history by index (0 = most recent).
pub fn paste(index: usize) -> KernelResult<ClipEntry> {
    with_state(|state| {
        let len = state.entries.len();
        if index >= len {
            return Err(KernelError::NotFound);
        }
        let actual_idx = len - 1 - index;
        state.entries[actual_idx].paste_count += 1;
        state.total_pastes += 1;
        Ok(state.entries[actual_idx].clone())
    })
}

/// Paste from a named slot.
pub fn paste_slot(name: &str) -> KernelResult<ClipEntry> {
    with_state(|state| {
        let entry = state.entries.iter_mut().find(|e| e.slot_name == name)
            .ok_or(KernelError::NotFound)?;
        entry.paste_count += 1;
        state.total_pastes += 1;
        Ok(entry.clone())
    })
}

/// Pin an entry by id.
pub fn pin(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.entries.iter_mut().find(|e| e.id == id)
            .ok_or(KernelError::NotFound)?;
        entry.pinned = true;
        Ok(())
    })
}

/// Unpin an entry.
pub fn unpin(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.entries.iter_mut().find(|e| e.id == id)
            .ok_or(KernelError::NotFound)?;
        entry.pinned = false;
        Ok(())
    })
}

/// Assign a named slot to an entry.
pub fn set_slot(id: u32, name: &str) -> KernelResult<()> {
    with_state(|state| {
        // Remove slot name from any existing entry.
        for entry in state.entries.iter_mut() {
            if entry.slot_name == name {
                entry.slot_name.clear();
            }
        }
        let entry = state.entries.iter_mut().find(|e| e.id == id)
            .ok_or(KernelError::NotFound)?;
        entry.slot_name = String::from(name);
        entry.pinned = true; // Slotted entries are auto-pinned.
        Ok(())
    })
}

/// Remove an entry by id.
pub fn remove(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.entries.len();
        state.entries.retain(|e| e.id != id);
        if state.entries.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Clear all non-pinned entries.
pub fn clear_history() -> KernelResult<usize> {
    with_state(|state| {
        let before = state.entries.len();
        state.entries.retain(|e| e.pinned);
        Ok(before - state.entries.len())
    })
}

/// Enable/disable clipboard history.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        Ok(())
    })
}

/// List history entries, newest first.
pub fn list_history(max: usize) -> Vec<ClipEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut entries = s.entries.clone();
        entries.reverse();
        entries.truncate(max);
        entries
    })
}

/// List pinned entries.
pub fn list_pinned() -> Vec<ClipEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.entries.iter().filter(|e| e.pinned).cloned().collect()
    })
}

/// List named slots.
pub fn list_slots() -> Vec<ClipEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.entries.iter().filter(|e| !e.slot_name.is_empty()).cloned().collect()
    })
}

/// Statistics: (entry_count, pinned_count, total_copies, total_pastes, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let pinned = s.entries.iter().filter(|e| e.pinned).count();
            (s.entries.len(), pinned, s.total_copies, s.total_pastes, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("multiclip::self_test() — running tests...");
    init_defaults();

    // 1: Empty history.
    assert_eq!(list_history(10).len(), 0);
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Push entries.
    let _id1 = push("Hello", ContentType::PlainText).expect("push1");
    let id2 = push("World", ContentType::PlainText).expect("push2");
    let _id3 = push("<b>Bold</b>", ContentType::Html).expect("push3");
    assert_eq!(list_history(10).len(), 3);
    crate::serial_println!("  [2/8] push: OK");

    // 3: Paste most recent.
    let entry = paste(0).expect("paste");
    assert_eq!(entry.content, "<b>Bold</b>");
    assert_eq!(entry.content_type, ContentType::Html);
    crate::serial_println!("  [3/8] paste: OK");

    // 4: No duplicate of most recent.
    push("<b>Bold</b>", ContentType::Html).expect("push_dup");
    assert_eq!(list_history(10).len(), 3); // Still 3.
    crate::serial_println!("  [4/8] no duplicate: OK");

    // 5: Pin and clear.
    pin(id2).expect("pin");
    let cleared = clear_history().expect("clear");
    assert_eq!(cleared, 2);
    assert_eq!(list_history(10).len(), 1);
    assert_eq!(list_pinned().len(), 1);
    crate::serial_println!("  [5/8] pin+clear: OK");

    // 6: Named slot.
    let id4 = push("my.email@example.com", ContentType::PlainText).expect("push4");
    set_slot(id4, "email").expect("slot");
    let entry = paste_slot("email").expect("slot_paste");
    assert_eq!(entry.content, "my.email@example.com");
    assert_eq!(list_slots().len(), 1);
    crate::serial_println!("  [6/8] slots: OK");

    // 7: Remove.
    remove(id2).expect("remove");
    assert_eq!(list_pinned().len(), 1); // Only slotted entry remains pinned.
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (entries, pinned, copies, pastes, ops) = stats();
    assert_eq!(entries, 1);
    assert_eq!(pinned, 1);
    assert!(copies >= 4);
    assert!(pastes >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("multiclip::self_test() — all 8 tests passed");
}

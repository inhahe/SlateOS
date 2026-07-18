//! Quick Notes — system-level quick notes and scratchpad.
//!
//! Provides a persistent scratch space for quick notes, snippets,
//! and temporary text, accessible via keyboard shortcut.
//!
//! ## Architecture
//!
//! ```text
//! User opens quick notes
//!   → quicknote::create(text) → new note
//!   → quicknote::list() → all notes
//!   → quicknote::search(query) → find notes
//!
//! Integration:
//!   → clipboard (paste from clipboard)
//!   → kbshortcuts (quick access hotkey)
//!   → notifcenter (pinned notes)
//!   → widgets (sticky notes widget)
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

/// Note color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteColor {
    Yellow,
    Blue,
    Green,
    Pink,
    Purple,
    Orange,
    White,
}

impl NoteColor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Yellow => "Yellow",
            Self::Blue => "Blue",
            Self::Green => "Green",
            Self::Pink => "Pink",
            Self::Purple => "Purple",
            Self::Orange => "Orange",
            Self::White => "White",
        }
    }
}

/// A quick note.
#[derive(Debug, Clone)]
pub struct Note {
    pub id: u32,
    pub title: String,
    pub content: String,
    pub color: NoteColor,
    pub pinned: bool,
    pub created_ns: u64,
    pub modified_ns: u64,
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_NOTES: usize = 500;

struct State {
    notes: Vec<Note>,
    next_id: u32,
    total_created: u64,
    total_deleted: u64,
    total_edits: u64,
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
        notes: Vec::new(),
        next_id: 1,
        total_created: 0,
        total_deleted: 0,
        total_edits: 0,
        ops: 0,
    });
}

/// Create a new note.
pub fn create(title: &str, content: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.notes.len() >= MAX_NOTES {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.notes.push(Note {
            id,
            title: String::from(title),
            content: String::from(content),
            color: NoteColor::Yellow,
            pinned: false,
            created_ns: now,
            modified_ns: now,
            tags: Vec::new(),
        });
        state.total_created += 1;
        Ok(id)
    })
}

/// Edit note content.
pub fn edit(id: u32, content: &str) -> KernelResult<()> {
    with_state(|state| {
        let note = state.notes.iter_mut().find(|n| n.id == id)
            .ok_or(KernelError::NotFound)?;
        note.content = String::from(content);
        note.modified_ns = crate::hpet::elapsed_ns();
        state.total_edits += 1;
        Ok(())
    })
}

/// Set note title.
pub fn set_title(id: u32, title: &str) -> KernelResult<()> {
    with_state(|state| {
        let note = state.notes.iter_mut().find(|n| n.id == id)
            .ok_or(KernelError::NotFound)?;
        note.title = String::from(title);
        note.modified_ns = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Set note color.
pub fn set_color(id: u32, color: NoteColor) -> KernelResult<()> {
    with_state(|state| {
        let note = state.notes.iter_mut().find(|n| n.id == id)
            .ok_or(KernelError::NotFound)?;
        note.color = color;
        Ok(())
    })
}

/// Pin/unpin a note.
pub fn set_pinned(id: u32, pinned: bool) -> KernelResult<()> {
    with_state(|state| {
        let note = state.notes.iter_mut().find(|n| n.id == id)
            .ok_or(KernelError::NotFound)?;
        note.pinned = pinned;
        Ok(())
    })
}

/// Add a tag to a note.
pub fn add_tag(id: u32, tag: &str) -> KernelResult<()> {
    with_state(|state| {
        let note = state.notes.iter_mut().find(|n| n.id == id)
            .ok_or(KernelError::NotFound)?;
        if !note.tags.iter().any(|t| t == tag) {
            note.tags.push(String::from(tag));
        }
        Ok(())
    })
}

/// Delete a note.
pub fn delete(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.notes.len();
        state.notes.retain(|n| n.id != id);
        if state.notes.len() == before { return Err(KernelError::NotFound); }
        state.total_deleted += 1;
        Ok(())
    })
}

/// Get a note by id.
pub fn get(id: u32) -> Option<Note> {
    STATE.lock().as_ref().and_then(|s| s.notes.iter().find(|n| n.id == id).cloned())
}

/// List notes, sorted by modified time (newest first).
pub fn list(max: usize) -> Vec<Note> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut notes = s.notes.clone();
        notes.sort_by_key(|e| core::cmp::Reverse(e.modified_ns));
        notes.truncate(max);
        notes
    })
}

/// Search notes by content or title.
pub fn search(query: &str, max: usize) -> Vec<Note> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let q = query.to_lowercase();
        let mut found: Vec<Note> = s.notes.iter()
            .filter(|n| n.title.to_lowercase().contains(&q) || n.content.to_lowercase().contains(&q))
            .cloned()
            .collect();
        found.truncate(max);
        found
    })
}

/// List pinned notes.
pub fn list_pinned() -> Vec<Note> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.notes.iter().filter(|n| n.pinned).cloned().collect()
    })
}

/// Statistics: (note_count, pinned_count, total_created, total_edits, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let pinned = s.notes.iter().filter(|n| n.pinned).count();
            (s.notes.len(), pinned, s.total_created, s.total_edits, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("quicknote::self_test() — running tests...");
    init_defaults();

    // 1: Empty.
    assert_eq!(list(10).len(), 0);
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Create notes.
    let id1 = create("Shopping", "milk, bread, eggs").expect("create1");
    let id2 = create("Ideas", "new project concept").expect("create2");
    assert_eq!(list(10).len(), 2);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Edit.
    edit(id1, "milk, bread, eggs, butter").expect("edit");
    let note = get(id1).expect("get");
    assert!(note.content.contains("butter"));
    crate::serial_println!("  [3/8] edit: OK");

    // 4: Color.
    set_color(id1, NoteColor::Green).expect("color");
    let note = get(id1).expect("get2");
    assert_eq!(note.color, NoteColor::Green);
    crate::serial_println!("  [4/8] color: OK");

    // 5: Pin.
    set_pinned(id2, true).expect("pin");
    assert_eq!(list_pinned().len(), 1);
    crate::serial_println!("  [5/8] pin: OK");

    // 6: Tags.
    add_tag(id1, "groceries").expect("tag");
    let note = get(id1).expect("get3");
    assert_eq!(note.tags.len(), 1);
    crate::serial_println!("  [6/8] tags: OK");

    // 7: Search.
    let found = search("project", 10);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, id2);
    crate::serial_println!("  [7/8] search: OK");

    // 8: Stats.
    let (notes, pinned, created, edits, ops) = stats();
    assert_eq!(notes, 2);
    assert_eq!(pinned, 1);
    assert_eq!(created, 2);
    assert_eq!(edits, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("quicknote::self_test() — all 8 tests passed");
}

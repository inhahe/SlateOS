//! Recent Search — search query history and suggestions.
//!
//! Tracks recent search queries across the system (file search, settings search,
//! app search) and provides search suggestions based on history.
//!
//! ## Architecture
//!
//! ```text
//! User performs search
//!   → recentsearch::record(query, source) → save to history
//!   → recentsearch::suggest(prefix) → matching suggestions
//!
//! Integration:
//!   → search (file search)
//!   → findex (file indexer)
//!   → startmenu (start menu search)
//!   → rundialog (run dialog)
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

/// Search source/context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSource {
    FileSearch,
    Settings,
    AppLauncher,
    RunDialog,
    WebSearch,
    HelpSearch,
}

impl SearchSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::FileSearch => "Files",
            Self::Settings => "Settings",
            Self::AppLauncher => "Apps",
            Self::RunDialog => "Run",
            Self::WebSearch => "Web",
            Self::HelpSearch => "Help",
        }
    }
}

/// A search history entry.
#[derive(Debug, Clone)]
pub struct SearchEntry {
    pub query: String,
    pub source: SearchSource,
    pub timestamp_ns: u64,
    pub result_count: u32,
    pub selected_result: String,
    pub use_count: u32,
    pub pinned: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ENTRIES: usize = 500;

struct State {
    entries: Vec<SearchEntry>,
    global_enabled: bool,
    total_searches: u64,
    total_suggestions_used: u64,
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
        global_enabled: true,
        total_searches: 0,
        total_suggestions_used: 0,
        ops: 0,
    });
}

/// Record a search query.
pub fn record(query: &str, source: SearchSource, result_count: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.global_enabled { return Ok(()); }
        let now = crate::hpet::elapsed_ns();
        // Check if same query+source exists; if so, bump use_count.
        if let Some(entry) = state.entries.iter_mut().find(|e| e.query == query && e.source == source) {
            entry.use_count += 1;
            entry.timestamp_ns = now;
            entry.result_count = result_count;
            state.total_searches += 1;
            return Ok(());
        }
        // Evict oldest non-pinned if at capacity.
        if state.entries.len() >= MAX_ENTRIES {
            if let Some(idx) = state.entries.iter().position(|e| !e.pinned) {
                state.entries.remove(idx);
            } else {
                return Err(KernelError::ResourceExhausted);
            }
        }
        state.entries.push(SearchEntry {
            query: String::from(query),
            source,
            timestamp_ns: now,
            result_count,
            selected_result: String::new(),
            use_count: 1,
            pinned: false,
        });
        state.total_searches += 1;
        Ok(())
    })
}

/// Get search suggestions matching a prefix.
pub fn suggest(prefix: &str, max_results: usize) -> Vec<SearchEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let prefix_lower = prefix.to_lowercase();
        let mut matches: Vec<SearchEntry> = s.entries.iter()
            .filter(|e| e.query.to_lowercase().starts_with(&prefix_lower))
            .cloned()
            .collect();
        // Sort by use_count descending, then timestamp descending.
        matches.sort_by(|a, b| {
            b.use_count.cmp(&a.use_count)
                .then(b.timestamp_ns.cmp(&a.timestamp_ns))
        });
        matches.truncate(max_results);
        matches
    })
}

/// Record that a suggestion was used.
pub fn use_suggestion(query: &str) -> KernelResult<()> {
    with_state(|state| {
        if let Some(entry) = state.entries.iter_mut().find(|e| e.query == query) {
            entry.use_count += 1;
            entry.timestamp_ns = crate::hpet::elapsed_ns();
            state.total_suggestions_used += 1;
        }
        Ok(())
    })
}

/// Pin a search entry (prevent eviction).
pub fn pin(query: &str) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.entries.iter_mut().find(|e| e.query == query)
            .ok_or(KernelError::NotFound)?;
        entry.pinned = true;
        Ok(())
    })
}

/// Unpin a search entry.
pub fn unpin(query: &str) -> KernelResult<()> {
    with_state(|state| {
        let entry = state.entries.iter_mut().find(|e| e.query == query)
            .ok_or(KernelError::NotFound)?;
        entry.pinned = false;
        Ok(())
    })
}

/// Remove a specific entry.
pub fn remove(query: &str, source: SearchSource) -> KernelResult<()> {
    with_state(|state| {
        let before = state.entries.len();
        state.entries.retain(|e| !(e.query == query && e.source == source));
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

/// Clear entries for a specific source.
pub fn clear_source(source: SearchSource) -> KernelResult<usize> {
    with_state(|state| {
        let before = state.entries.len();
        state.entries.retain(|e| e.source != source || e.pinned);
        Ok(before - state.entries.len())
    })
}

/// Enable/disable search history.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        Ok(())
    })
}

/// List recent entries, newest first.
pub fn list_recent(max: usize) -> Vec<SearchEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut entries = s.entries.clone();
        entries.sort_by_key(|e| core::cmp::Reverse(e.timestamp_ns));
        entries.truncate(max);
        entries
    })
}

/// Statistics: (entry_count, pinned_count, total_searches, total_suggestions_used, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let pinned = s.entries.iter().filter(|e| e.pinned).count();
            (s.entries.len(), pinned, s.total_searches, s.total_suggestions_used, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("recentsearch::self_test() — running tests...");
    init_defaults();

    // 1: No entries initially.
    assert_eq!(list_recent(10).len(), 0);
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Record searches.
    record("hello world", SearchSource::FileSearch, 5).expect("rec1");
    record("system settings", SearchSource::Settings, 3).expect("rec2");
    record("terminal", SearchSource::AppLauncher, 1).expect("rec3");
    assert_eq!(list_recent(10).len(), 3);
    crate::serial_println!("  [2/8] record: OK");

    // 3: Suggestions.
    let sugg = suggest("hel", 5);
    assert_eq!(sugg.len(), 1);
    assert_eq!(sugg[0].query, "hello world");
    crate::serial_println!("  [3/8] suggest: OK");

    // 4: Duplicate bumps count.
    record("hello world", SearchSource::FileSearch, 10).expect("rec4");
    assert_eq!(list_recent(10).len(), 3); // Still 3 entries.
    let sugg = suggest("hello", 5);
    assert_eq!(sugg[0].use_count, 2);
    crate::serial_println!("  [4/8] deduplicate: OK");

    // 5: Pin entry.
    pin("terminal").expect("pin");
    let cleared = clear_history().expect("clear");
    assert_eq!(cleared, 2);
    assert_eq!(list_recent(10).len(), 1);
    assert_eq!(list_recent(10)[0].query, "terminal");
    crate::serial_println!("  [5/8] pin: OK");

    // 6: Unpin and clear.
    unpin("terminal").expect("unpin");
    let cleared = clear_history().expect("clear2");
    assert_eq!(cleared, 1);
    assert_eq!(list_recent(10).len(), 0);
    crate::serial_println!("  [6/8] clear: OK");

    // 7: Source-specific clear.
    record("a", SearchSource::FileSearch, 1).expect("ra");
    record("b", SearchSource::Settings, 1).expect("rb");
    let cleared = clear_source(SearchSource::FileSearch).expect("cs");
    assert_eq!(cleared, 1);
    assert_eq!(list_recent(10).len(), 1);
    crate::serial_println!("  [7/8] source clear: OK");

    // 8: Stats.
    let (entries, pinned, searches, suggestions, ops) = stats();
    assert_eq!(entries, 1);
    assert_eq!(pinned, 0);
    assert!(searches >= 5);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK (suggestions_used={})", suggestions);

    crate::serial_println!("recentsearch::self_test() — all 8 tests passed");
}

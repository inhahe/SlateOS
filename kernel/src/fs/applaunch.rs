//! App Launcher — search-based application launcher.
//!
//! Provides a unified search interface for launching applications,
//! opening files, running commands, and performing calculations.
//!
//! ## Architecture
//!
//! ```text
//! User activates launcher
//!   → applaunch::search(query) → ranked results
//!   → applaunch::launch(result_id) → execute action
//!
//! Learning
//!   → applaunch::record_launch(app) → boost ranking
//!
//! Integration:
//!   → appregistry (installed apps)
//!   → kbshortcuts (launcher hotkey)
//!   → recent (recent files)
//!   → rundialog (command execution)
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

/// Result type from launcher search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultType {
    Application,
    File,
    Setting,
    Command,
    Calculation,
    WebSearch,
    Bookmark,
}

impl ResultType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Application => "App",
            Self::File => "File",
            Self::Setting => "Setting",
            Self::Command => "Command",
            Self::Calculation => "Calc",
            Self::WebSearch => "Web",
            Self::Bookmark => "Bookmark",
        }
    }
}

/// A launcher search result.
#[derive(Debug, Clone)]
pub struct LaunchResult {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub result_type: ResultType,
    pub action: String,
    pub icon: String,
    pub score: u32,
    pub launch_count: u64,
}

/// A registered launchable item.
#[derive(Debug, Clone)]
pub struct LaunchItem {
    pub id: u32,
    pub name: String,
    pub keywords: Vec<String>,
    pub result_type: ResultType,
    pub action: String,
    pub icon: String,
    pub launch_count: u64,
    pub last_launched_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ITEMS: usize = 500;

struct State {
    items: Vec<LaunchItem>,
    next_id: u32,
    total_searches: u64,
    total_launches: u64,
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
        items: alloc::vec![
            LaunchItem { id: 1, name: String::from("Files"), keywords: alloc::vec![String::from("file"), String::from("explorer"), String::from("browse")], result_type: ResultType::Application, action: String::from("launch:files"), icon: String::from("folder"), launch_count: 0, last_launched_ns: 0 },
            LaunchItem { id: 2, name: String::from("Terminal"), keywords: alloc::vec![String::from("terminal"), String::from("console"), String::from("shell")], result_type: ResultType::Application, action: String::from("launch:terminal"), icon: String::from("terminal"), launch_count: 0, last_launched_ns: 0 },
            LaunchItem { id: 3, name: String::from("Browser"), keywords: alloc::vec![String::from("browser"), String::from("web"), String::from("internet")], result_type: ResultType::Application, action: String::from("launch:browser"), icon: String::from("globe"), launch_count: 0, last_launched_ns: 0 },
            LaunchItem { id: 4, name: String::from("Settings"), keywords: alloc::vec![String::from("settings"), String::from("preferences"), String::from("config")], result_type: ResultType::Setting, action: String::from("launch:settings"), icon: String::from("gear"), launch_count: 0, last_launched_ns: 0 },
            LaunchItem { id: 5, name: String::from("Text Editor"), keywords: alloc::vec![String::from("editor"), String::from("text"), String::from("notepad")], result_type: ResultType::Application, action: String::from("launch:editor"), icon: String::from("edit"), launch_count: 0, last_launched_ns: 0 },
        ],
        next_id: 6,
        total_searches: 0,
        total_launches: 0,
        ops: 0,
    });
}

/// Register a launchable item.
pub fn register(name: &str, keywords: Vec<String>, rtype: ResultType, action: &str, icon: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.items.len() >= MAX_ITEMS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.items.push(LaunchItem {
            id, name: String::from(name), keywords, result_type: rtype,
            action: String::from(action), icon: String::from(icon),
            launch_count: 0, last_launched_ns: 0,
        });
        Ok(id)
    })
}

/// Unregister an item.
pub fn unregister(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.items.len();
        state.items.retain(|i| i.id != id);
        if state.items.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Search for items matching query.
pub fn search(query: &str, max: usize) -> Vec<LaunchResult> {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return Vec::new(),
    };
    state.ops += 1;
    state.total_searches += 1;

    let q = query.to_lowercase();
    let mut results: Vec<LaunchResult> = state.items.iter()
        .filter_map(|item| {
            let name_match = item.name.to_lowercase().contains(&q);
            let keyword_match = item.keywords.iter().any(|k| k.to_lowercase().contains(&q));
            if name_match || keyword_match {
                // Score: name match is stronger, plus launch count boost.
                let mut score = if name_match { 100 } else { 50 };
                score += (item.launch_count as u32).min(50); // Boost up to 50 from history.
                if item.name.to_lowercase().starts_with(&q) {
                    score += 25; // Prefix match bonus.
                }
                Some(LaunchResult {
                    id: item.id,
                    name: item.name.clone(),
                    description: item.action.clone(),
                    result_type: item.result_type,
                    action: item.action.clone(),
                    icon: item.icon.clone(),
                    score,
                    launch_count: item.launch_count,
                })
            } else {
                None
            }
        })
        .collect();

    results.sort_by_key(|e| core::cmp::Reverse(e.score));
    results.truncate(max);
    results
}

/// Record a launch (boosts ranking).
pub fn record_launch(id: u32) -> KernelResult<String> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let item = state.items.iter_mut().find(|i| i.id == id)
            .ok_or(KernelError::NotFound)?;
        item.launch_count += 1;
        item.last_launched_ns = now;
        state.total_launches += 1;
        Ok(item.action.clone())
    })
}

/// List all items.
pub fn list_items() -> Vec<LaunchItem> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.items.clone())
}

/// Get top launched items.
pub fn top_launched(max: usize) -> Vec<LaunchItem> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut items = s.items.clone();
        items.sort_by_key(|e| core::cmp::Reverse(e.launch_count));
        items.truncate(max);
        items
    })
}

/// Statistics: (item_count, total_searches, total_launches, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.items.len(), s.total_searches, s.total_launches, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("applaunch::self_test() — running tests...");
    init_defaults();

    // 1: Default items.
    assert_eq!(list_items().len(), 5);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Search by name.
    let results = search("terminal", 10);
    assert!(!results.is_empty());
    assert_eq!(results[0].name, "Terminal");
    crate::serial_println!("  [2/8] search name: OK");

    // 3: Search by keyword.
    let results = search("web", 10);
    assert!(!results.is_empty());
    assert_eq!(results[0].name, "Browser");
    crate::serial_println!("  [3/8] search keyword: OK");

    // 4: Record launch boosts ranking.
    record_launch(2).expect("launch"); // Terminal.
    record_launch(2).expect("launch2");
    record_launch(2).expect("launch3");
    let results = search("t", 10); // Both Terminal and Text Editor match.
    // Terminal should rank higher due to launch count.
    assert_eq!(results[0].name, "Terminal");
    crate::serial_println!("  [4/8] ranking: OK");

    // 5: Register new item.
    let id = register("Calculator", alloc::vec![String::from("calc"), String::from("math")],
        ResultType::Application, "launch:calculator", "calc").expect("reg");
    let results = search("calc", 10);
    assert_eq!(results[0].name, "Calculator");
    crate::serial_println!("  [5/8] register: OK");

    // 6: Top launched.
    let top = top_launched(3);
    assert_eq!(top[0].name, "Terminal");
    assert_eq!(top[0].launch_count, 3);
    crate::serial_println!("  [6/8] top launched: OK");

    // 7: Unregister.
    unregister(id).expect("unreg");
    assert_eq!(list_items().len(), 5);
    crate::serial_println!("  [7/8] unregister: OK");

    // 8: Stats.
    let (items, searches, launches, ops) = stats();
    assert_eq!(items, 5);
    assert!(searches >= 4);
    assert_eq!(launches, 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("applaunch::self_test() — all 8 tests passed");
}

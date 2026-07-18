//! Input Method Framework — multilingual text input engine management.
//!
//! Manages input method engines (IMEs) for languages that require character
//! composition (CJK, Indic, Arabic, etc.), including candidate selection,
//! conversion, and per-app input mode tracking.
//!
//! ## Architecture
//!
//! ```text
//! User types keystrokes
//!   → inputmethod::process_key(key) → compose/convert
//!   → inputmethod::get_candidates() → candidate list
//!   → inputmethod::commit(index) → final text
//!
//! Integration:
//!   → ime (basic IME toggle)
//!   → keylayout (keyboard layout)
//!   → kbsettings (keyboard configuration)
//!   → langpack (language packs)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Input method engine type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineType {
    /// Direct input (no composition).
    Direct,
    /// Pinyin (Chinese phonetic).
    Pinyin,
    /// Wubi (Chinese shape-based).
    Wubi,
    /// Kana/Romaji → Kanji (Japanese).
    Japanese,
    /// Hangul (Korean).
    Hangul,
    /// Transliteration (Indic scripts).
    Transliteration,
    /// Handwriting recognition.
    Handwriting,
}

impl EngineType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Direct => "Direct",
            Self::Pinyin => "Pinyin",
            Self::Wubi => "Wubi",
            Self::Japanese => "Japanese",
            Self::Hangul => "Hangul",
            Self::Transliteration => "Transliteration",
            Self::Handwriting => "Handwriting",
        }
    }
}

/// Input method engine configuration.
#[derive(Debug, Clone)]
pub struct InputEngine {
    pub id: u32,
    pub name: String,
    pub engine_type: EngineType,
    pub language: String,
    pub enabled: bool,
    pub commit_count: u64,
}

/// Composition state for active input.
#[derive(Debug, Clone)]
pub struct CompositionState {
    /// Raw input buffer.
    pub preedit: String,
    /// Candidate list.
    pub candidates: Vec<String>,
    /// Selected candidate index.
    pub selected: usize,
    /// Whether composition is active.
    pub composing: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ENGINES: usize = 20;

struct State {
    engines: Vec<InputEngine>,
    active_engine_id: u32,
    composition: CompositionState,
    next_id: u32,
    total_commits: u64,
    total_switches: u64,
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
        engines: alloc::vec![
            InputEngine { id: 1, name: String::from("Direct Input"), engine_type: EngineType::Direct, language: String::from("en"), enabled: true, commit_count: 0 },
        ],
        active_engine_id: 1,
        composition: CompositionState {
            preedit: String::new(),
            candidates: Vec::new(),
            selected: 0,
            composing: false,
        },
        next_id: 2,
        total_commits: 0,
        total_switches: 0,
        ops: 0,
    });
}

/// Add an input engine.
pub fn add_engine(name: &str, engine_type: EngineType, language: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.engines.len() >= MAX_ENGINES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.engines.push(InputEngine {
            id, name: String::from(name), engine_type,
            language: String::from(language), enabled: true, commit_count: 0,
        });
        Ok(id)
    })
}

/// Remove an engine.
pub fn remove_engine(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if id == 1 { return Err(KernelError::PermissionDenied); } // Can't remove Direct
        let before = state.engines.len();
        state.engines.retain(|e| e.id != id);
        if state.engines.len() == before {
            return Err(KernelError::NotFound);
        }
        if state.active_engine_id == id {
            state.active_engine_id = 1;
        }
        Ok(())
    })
}

/// Switch active engine.
pub fn switch_engine(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.engines.iter().any(|e| e.id == id && e.enabled) {
            return Err(KernelError::NotFound);
        }
        // Cancel any active composition.
        state.composition.preedit.clear();
        state.composition.candidates.clear();
        state.composition.composing = false;
        state.active_engine_id = id;
        state.total_switches += 1;
        Ok(())
    })
}

/// Cycle to next engine.
pub fn cycle_engine() -> KernelResult<String> {
    with_state(|state| {
        let enabled: Vec<u32> = state.engines.iter()
            .filter(|e| e.enabled)
            .map(|e| e.id)
            .collect();
        if enabled.is_empty() {
            return Err(KernelError::NotFound);
        }
        let current_idx = enabled.iter().position(|&id| id == state.active_engine_id).unwrap_or(0);
        let next_idx = (current_idx + 1) % enabled.len();
        state.active_engine_id = enabled[next_idx];
        state.total_switches += 1;
        let name = state.engines.iter()
            .find(|e| e.id == state.active_engine_id)
            .map_or(String::new(), |e| e.name.clone());
        Ok(name)
    })
}

/// Start composition (typing in the preedit buffer).
pub fn start_composition(text: &str) -> KernelResult<()> {
    with_state(|state| {
        state.composition.preedit = String::from(text);
        state.composition.composing = true;
        // Generate mock candidates based on engine type.
        state.composition.candidates.clear();
        let engine = state.engines.iter().find(|e| e.id == state.active_engine_id);
        if let Some(e) = engine {
            match e.engine_type {
                EngineType::Direct => {
                    state.composition.candidates.push(String::from(text));
                }
                _ => {
                    // Mock: generate numbered candidates.
                    for i in 1..=5 {
                        state.composition.candidates.push(format!("{}_{}", text, i));
                    }
                }
            }
        }
        state.composition.selected = 0;
        Ok(())
    })
}

/// Select a candidate by index.
pub fn select_candidate(index: usize) -> KernelResult<()> {
    with_state(|state| {
        if index >= state.composition.candidates.len() {
            return Err(KernelError::InvalidArgument);
        }
        state.composition.selected = index;
        Ok(())
    })
}

/// Commit the selected candidate.
pub fn commit() -> KernelResult<String> {
    with_state(|state| {
        if !state.composition.composing || state.composition.candidates.is_empty() {
            return Ok(state.composition.preedit.clone());
        }
        let text = state.composition.candidates.get(state.composition.selected)
            .cloned()
            .unwrap_or_else(|| state.composition.preedit.clone());
        // Update engine stats.
        if let Some(e) = state.engines.iter_mut().find(|e| e.id == state.active_engine_id) {
            e.commit_count += 1;
        }
        state.total_commits += 1;
        // Clear composition.
        state.composition.preedit.clear();
        state.composition.candidates.clear();
        state.composition.composing = false;
        Ok(text)
    })
}

/// Cancel composition.
pub fn cancel_composition() -> KernelResult<()> {
    with_state(|state| {
        state.composition.preedit.clear();
        state.composition.candidates.clear();
        state.composition.composing = false;
        Ok(())
    })
}

/// Get current composition state.
pub fn get_composition() -> Option<CompositionState> {
    STATE.lock().as_ref().map(|s| s.composition.clone())
}

/// List engines.
pub fn list_engines() -> Vec<InputEngine> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.engines.clone())
}

/// Get active engine name.
pub fn active_engine_name() -> String {
    STATE.lock().as_ref().and_then(|s| {
        s.engines.iter().find(|e| e.id == s.active_engine_id).map(|e| e.name.clone())
    }).unwrap_or_default()
}

/// Statistics: (engine_count, total_commits, total_switches, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.engines.len(), s.total_commits, s.total_switches, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("inputmethod::self_test() — running tests...");
    init_defaults();

    // 1: Default engine.
    assert_eq!(active_engine_name(), "Direct Input");
    assert_eq!(list_engines().len(), 1);
    crate::serial_println!("  [1/8] default engine: OK");

    // 2: Add engine.
    let py_id = add_engine("Pinyin", EngineType::Pinyin, "zh").expect("add");
    assert_eq!(list_engines().len(), 2);
    crate::serial_println!("  [2/8] add engine: OK");

    // 3: Switch engine.
    switch_engine(py_id).expect("switch");
    assert_eq!(active_engine_name(), "Pinyin");
    crate::serial_println!("  [3/8] switch engine: OK");

    // 4: Composition.
    start_composition("ni hao").expect("compose");
    let comp = get_composition().expect("comp");
    assert!(comp.composing);
    assert_eq!(comp.candidates.len(), 5);
    crate::serial_println!("  [4/8] composition: OK");

    // 5: Select and commit.
    select_candidate(2).expect("select");
    let text = commit().expect("commit");
    assert_eq!(text, "ni hao_3");
    let comp = get_composition().expect("comp2");
    assert!(!comp.composing);
    crate::serial_println!("  [5/8] commit: OK");

    // 6: Cycle engine.
    let name = cycle_engine().expect("cycle");
    assert_eq!(name, "Direct Input"); // cycles back to first
    crate::serial_println!("  [6/8] cycle: OK");

    // 7: Can't remove Direct.
    assert!(remove_engine(1).is_err());
    crate::serial_println!("  [7/8] protect direct: OK");

    // 8: Stats.
    let (engines, commits, switches, ops) = stats();
    assert_eq!(engines, 2);
    assert_eq!(commits, 1);
    assert!(switches >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("inputmethod::self_test() — all 8 tests passed");
}

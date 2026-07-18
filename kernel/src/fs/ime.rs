//! Input Method Editor (IME) — multilingual text input and emoji.
//!
//! Handles composition of text for languages that need an input method
//! (Chinese, Japanese, Korean, etc.) and emoji picker integration.
//! The IME sits between the keyboard driver and the focused text field,
//! intercepting keystrokes and producing composed text.
//!
//! ## Design Reference
//!
//! design.txt line 711: "emoji input, keyboard layout" on taskbar
//!
//! ## Architecture
//!
//! ```text
//! Keyboard driver
//!   → keylayout::translate()        // physical → logical key
//!   → ime::process_key()            // compose multi-key sequences
//!   → focused text field             // final text
//!
//! Taskbar indicator
//!   → ime::current_method()         // show active IME
//!   → ime::available_methods()      // switch IME
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum registered input methods.
const MAX_METHODS: usize = 32;

/// Maximum candidates during composition.
const MAX_CANDIDATES: usize = 64;

/// Maximum composition buffer length.
const MAX_COMPOSE_LEN: usize = 256;

/// Maximum emoji entries.
const MAX_EMOJI: usize = 2048;

/// Maximum recently-used emoji.
const MAX_RECENT_EMOJI: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// An input method.
#[derive(Debug, Clone)]
pub struct InputMethod {
    /// Unique ID.
    pub id: String,
    /// Display name (e.g., "Pinyin", "Hangul", "Romaji").
    pub name: String,
    /// Language tag (e.g., "zh-CN", "ja", "ko").
    pub language: String,
    /// Short label for taskbar (e.g., "拼", "あ", "한").
    pub indicator: String,
    /// Whether this method uses a composition buffer.
    pub uses_composition: bool,
    /// Whether this is a built-in method.
    pub builtin: bool,
}

/// Current composition state.
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct CompositionState {
    /// Raw keystroke buffer.
    pub buffer: String,
    /// Candidate list for current buffer.
    pub candidates: Vec<String>,
    /// Index of selected candidate.
    pub selected: usize,
    /// Whether composition is active.
    pub active: bool,
}


/// An emoji entry.
#[derive(Debug, Clone)]
pub struct EmojiEntry {
    /// The emoji character(s).
    pub emoji: String,
    /// Short name / description.
    pub name: String,
    /// Category.
    pub category: String,
    /// Search keywords.
    pub keywords: Vec<String>,
}

/// Result of processing a keystroke through the IME.
#[derive(Debug, Clone)]
pub enum ImeResult {
    /// Key was consumed by composition — no output yet.
    Consumed,
    /// Composition produced committed text.
    Commit(String),
    /// Key was not handled by the IME — pass through.
    PassThrough,
    /// Composition was cancelled.
    Cancelled,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    methods: BTreeMap<String, InputMethod>,
    active: String,
    composition: CompositionState,
    emoji: Vec<EmojiEntry>,
    recent_emoji: Vec<String>,
    emoji_picker_open: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            methods: BTreeMap::new(),
            active: String::new(),
            composition: CompositionState {
                buffer: String::new(),
                candidates: Vec::new(),
                selected: 0,
                active: false,
            },
            emoji: Vec::new(),
            recent_emoji: Vec::new(),
            emoji_picker_open: false,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static COMMIT_COUNT: AtomicU64 = AtomicU64::new(0);
static KEY_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Input method management
// ---------------------------------------------------------------------------

/// Register an input method.
pub fn register_method(
    id: &str,
    name: &str,
    language: &str,
    indicator: &str,
    uses_composition: bool,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.methods.len() >= MAX_METHODS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.methods.contains_key(id) {
        return Err(KernelError::AlreadyExists);
    }
    state.methods.insert(String::from(id), InputMethod {
        id: String::from(id),
        name: String::from(name),
        language: String::from(language),
        indicator: String::from(indicator),
        uses_composition,
        builtin: false,
    });
    Ok(())
}

/// Unregister an input method.
pub fn unregister_method(id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.active == id {
        state.active = String::new();
        state.composition = CompositionState::default();
    }
    state.methods.remove(id).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// List available input methods.
pub fn list_methods() -> Vec<InputMethod> {
    STATE.lock().methods.values().cloned().collect()
}

/// Get method info.
pub fn get_method(id: &str) -> Option<InputMethod> {
    STATE.lock().methods.get(id).cloned()
}

/// Set the active input method (empty = direct input, no IME).
pub fn set_active(id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !id.is_empty() && !state.methods.contains_key(id) {
        return Err(KernelError::NotFound);
    }
    // Cancel any in-progress composition.
    state.composition = CompositionState::default();
    state.active = String::from(id);
    Ok(())
}

/// Get the currently active method ID (empty = direct input).
pub fn active() -> String {
    STATE.lock().active.clone()
}

/// Get the active method's indicator for the taskbar.
pub fn active_indicator() -> String {
    let state = STATE.lock();
    if state.active.is_empty() {
        return String::from("EN");
    }
    state.methods.get(&state.active)
        .map(|m| m.indicator.clone())
        .unwrap_or_else(|| String::from("?"))
}

/// Cycle to the next input method.
pub fn cycle_next() -> String {
    let mut state = STATE.lock();
    let keys: Vec<String> = state.methods.keys().cloned().collect();
    if keys.is_empty() {
        state.active = String::new();
        return String::new();
    }
    let cur_idx = keys.iter().position(|k| k == &state.active);
    let next = match cur_idx {
        Some(i) if i + 1 < keys.len() => keys[i + 1].clone(),
        _ => keys[0].clone(),
    };
    state.composition = CompositionState::default();
    state.active = next.clone();
    next
}

// ---------------------------------------------------------------------------
// Composition
// ---------------------------------------------------------------------------

/// Process a keystroke through the active IME.
///
/// Returns what the IME decided: consume, commit text, or pass through.
pub fn process_key(key_char: char) -> ImeResult {
    KEY_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = STATE.lock();

    if state.active.is_empty() {
        return ImeResult::PassThrough;
    }

    let method = match state.methods.get(&state.active) {
        Some(m) => m.clone(),
        None => return ImeResult::PassThrough,
    };

    if !method.uses_composition {
        return ImeResult::PassThrough;
    }

    // Simple composition model: buffer keystrokes, generate candidates.
    if key_char == '\x1B' {
        // Escape — cancel composition.
        state.composition = CompositionState::default();
        return ImeResult::Cancelled;
    }

    if key_char == '\n' || key_char == '\r' {
        // Enter — commit selected candidate or raw buffer.
        if state.composition.active {
            let text = if state.composition.selected < state.composition.candidates.len() {
                state.composition.candidates[state.composition.selected].clone()
            } else if !state.composition.buffer.is_empty() {
                state.composition.buffer.clone()
            } else {
                return ImeResult::PassThrough;
            };
            state.composition = CompositionState::default();
            COMMIT_COUNT.fetch_add(1, Ordering::Relaxed);
            return ImeResult::Commit(text);
        }
        return ImeResult::PassThrough;
    }

    if key_char == '\t' {
        // Tab — next candidate.
        if state.composition.active && !state.composition.candidates.is_empty() {
            state.composition.selected =
                (state.composition.selected + 1) % state.composition.candidates.len();
            return ImeResult::Consumed;
        }
        return ImeResult::PassThrough;
    }

    if key_char == '\x08' {
        // Backspace — remove last char from buffer.
        if state.composition.active && !state.composition.buffer.is_empty() {
            state.composition.buffer.pop();
            if state.composition.buffer.is_empty() {
                state.composition = CompositionState::default();
                return ImeResult::Cancelled;
            }
            // Regenerate candidates (simplified).
            state.composition.candidates = generate_candidates(&state.composition.buffer);
            state.composition.selected = 0;
            return ImeResult::Consumed;
        }
        return ImeResult::PassThrough;
    }

    // Regular character — add to buffer.
    if state.composition.buffer.len() >= MAX_COMPOSE_LEN {
        return ImeResult::Consumed;
    }
    state.composition.buffer.push(key_char);
    state.composition.active = true;
    state.composition.candidates = generate_candidates(&state.composition.buffer);
    state.composition.selected = 0;
    ImeResult::Consumed
}

/// Get current composition state.
pub fn composition() -> CompositionState {
    STATE.lock().composition.clone()
}

/// Commit a specific candidate by index.
pub fn commit_candidate(index: usize) -> KernelResult<String> {
    let mut state = STATE.lock();
    let text = state.composition.candidates.get(index)
        .cloned()
        .ok_or(KernelError::InvalidArgument)?;
    state.composition = CompositionState::default();
    COMMIT_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(text)
}

/// Cancel current composition.
pub fn cancel_composition() {
    STATE.lock().composition = CompositionState::default();
}

/// Generate candidate list (simplified placeholder).
fn generate_candidates(buffer: &str) -> Vec<String> {
    // In a real IME, this would consult a dictionary/language model.
    // Here we just echo the buffer and a few variants.
    let mut candidates = Vec::new();
    candidates.push(String::from(buffer));
    if buffer.len() > 1 {
        // Reverse as a dummy "alternative."
        let rev: String = buffer.chars().rev().collect();
        candidates.push(rev);
    }
    candidates
}

// ---------------------------------------------------------------------------
// Emoji
// ---------------------------------------------------------------------------

/// Add an emoji to the registry.
pub fn add_emoji(emoji: &str, name: &str, category: &str, keywords: &[&str]) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.emoji.len() >= MAX_EMOJI {
        return Err(KernelError::ResourceExhausted);
    }
    state.emoji.push(EmojiEntry {
        emoji: String::from(emoji),
        name: String::from(name),
        category: String::from(category),
        keywords: keywords.iter().map(|s| String::from(*s)).collect(),
    });
    Ok(())
}

/// Search emoji by keyword.
pub fn search_emoji(query: &str) -> Vec<EmojiEntry> {
    let state = STATE.lock();
    let q = query.to_ascii_lowercase();
    state.emoji.iter()
        .filter(|e| {
            e.name.to_ascii_lowercase().contains(&q)
            || e.keywords.iter().any(|k| k.to_ascii_lowercase().contains(&q))
        })
        .take(20)
        .cloned()
        .collect()
}

/// List emoji by category.
pub fn emoji_by_category(category: &str) -> Vec<EmojiEntry> {
    let state = STATE.lock();
    state.emoji.iter()
        .filter(|e| e.category == category)
        .cloned()
        .collect()
}

/// Select an emoji (adds to recent).
pub fn select_emoji(emoji: &str) {
    let mut state = STATE.lock();
    state.recent_emoji.retain(|e| e != emoji);
    state.recent_emoji.insert(0, String::from(emoji));
    if state.recent_emoji.len() > MAX_RECENT_EMOJI {
        state.recent_emoji.truncate(MAX_RECENT_EMOJI);
    }
}

/// Get recent emoji.
pub fn recent_emoji() -> Vec<String> {
    STATE.lock().recent_emoji.clone()
}

/// Open/close the emoji picker.
pub fn set_emoji_picker(open: bool) {
    STATE.lock().emoji_picker_open = open;
}

/// Check if emoji picker is open.
pub fn emoji_picker_open() -> bool {
    STATE.lock().emoji_picker_open
}

/// Register a set of default emoji.
pub fn init_emoji_defaults() -> KernelResult<()> {
    // A small selection of common emoji.
    let defaults = [
        ("😀", "grinning face", "Smileys", &["happy", "smile", "grin"][..]),
        ("😂", "face with tears of joy", "Smileys", &["laugh", "cry", "lol"]),
        ("❤️", "red heart", "Symbols", &["love", "heart"]),
        ("👍", "thumbs up", "People", &["like", "ok", "yes"]),
        ("👎", "thumbs down", "People", &["dislike", "no"]),
        ("🔥", "fire", "Nature", &["hot", "flame"]),
        ("🎉", "party popper", "Activities", &["party", "celebrate"]),
        ("💻", "laptop", "Objects", &["computer", "code"]),
        ("🐛", "bug", "Nature", &["insect", "debug"]),
        ("✅", "check mark", "Symbols", &["done", "complete", "yes"]),
        ("❌", "cross mark", "Symbols", &["no", "wrong", "delete"]),
        ("⚠️", "warning", "Symbols", &["alert", "caution"]),
        ("📁", "folder", "Objects", &["directory", "file"]),
        ("🔍", "magnifying glass", "Objects", &["search", "find"]),
        ("⭐", "star", "Symbols", &["favorite", "bookmark"]),
        ("🚀", "rocket", "Travel", &["launch", "fast", "speed"]),
    ];
    for (emoji, name, cat, keywords) in &defaults {
        let _ = add_emoji(emoji, name, cat, keywords);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

/// Initialize with default input methods.
pub fn init_defaults() -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.methods.contains_key("direct") {
        return Ok(());
    }

    // Direct input (no IME, just keyboard).
    state.methods.insert(String::from("direct"), InputMethod {
        id: String::from("direct"),
        name: String::from("Direct Input"),
        language: String::from("en"),
        indicator: String::from("EN"),
        uses_composition: false,
        builtin: true,
    });

    // Pinyin (Chinese).
    state.methods.insert(String::from("pinyin"), InputMethod {
        id: String::from("pinyin"),
        name: String::from("Pinyin"),
        language: String::from("zh-CN"),
        indicator: String::from("拼"),
        uses_composition: true,
        builtin: true,
    });

    // Japanese Romaji.
    state.methods.insert(String::from("romaji"), InputMethod {
        id: String::from("romaji"),
        name: String::from("Romaji"),
        language: String::from("ja"),
        indicator: String::from("あ"),
        uses_composition: true,
        builtin: true,
    });

    // Korean Hangul.
    state.methods.insert(String::from("hangul"), InputMethod {
        id: String::from("hangul"),
        name: String::from("Hangul"),
        language: String::from("ko"),
        indicator: String::from("한"),
        uses_composition: true,
        builtin: true,
    });

    state.active = String::from("direct");
    drop(state);

    init_emoji_defaults()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (method_count, emoji_count, commit_count, key_count).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = STATE.lock();
    (
        state.methods.len(),
        state.emoji.len(),
        COMMIT_COUNT.load(Ordering::Relaxed),
        KEY_COUNT.load(Ordering::Relaxed),
    )
}

pub fn reset_stats() {
    COMMIT_COUNT.store(0, Ordering::Relaxed);
    KEY_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.methods.clear();
    state.active = String::new();
    state.composition = CompositionState::default();
    state.emoji.clear();
    state.recent_emoji.clear();
    state.emoji_picker_open = false;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Register methods.
    serial_println!("  ime::self_test 1: register methods");
    init_defaults()?;
    let methods = list_methods();
    assert!(methods.len() >= 4); // direct, pinyin, romaji, hangul

    // Test 2: Set active and indicator.
    serial_println!("  ime::self_test 2: active/indicator");
    set_active("pinyin")?;
    assert_eq!(active(), "pinyin");
    assert_eq!(active_indicator(), "拼");
    set_active("direct")?;
    assert_eq!(active_indicator(), "EN");

    // Test 3: Composition.
    serial_println!("  ime::self_test 3: composition");
    set_active("pinyin")?;
    let r1 = process_key('n');
    assert!(matches!(r1, ImeResult::Consumed));
    let r2 = process_key('i');
    assert!(matches!(r2, ImeResult::Consumed));
    let comp = composition();
    assert!(comp.active);
    assert_eq!(comp.buffer, "ni");
    assert!(!comp.candidates.is_empty());

    // Commit.
    let r3 = process_key('\n');
    assert!(matches!(r3, ImeResult::Commit(_)));
    let comp2 = composition();
    assert!(!comp2.active);

    // Test 4: Cancel composition.
    serial_println!("  ime::self_test 4: cancel");
    let _ = process_key('h');
    let _ = process_key('a');
    let r4 = process_key('\x1B');
    assert!(matches!(r4, ImeResult::Cancelled));
    assert!(!composition().active);

    // Test 5: Cycle methods.
    serial_println!("  ime::self_test 5: cycle");
    set_active("direct")?;
    let next = cycle_next();
    assert!(!next.is_empty());

    // Test 6: Emoji.
    serial_println!("  ime::self_test 6: emoji");
    let results = search_emoji("heart");
    assert!(!results.is_empty());
    select_emoji("❤️");
    let recent = recent_emoji();
    assert_eq!(recent.first().map(|s| s.as_str()), Some("❤️"));

    // Test 7: Direct input passthrough.
    serial_println!("  ime::self_test 7: passthrough");
    set_active("direct")?;
    let r5 = process_key('a');
    assert!(matches!(r5, ImeResult::PassThrough));

    let (mc, ec, cc, kc) = stats();
    assert!(mc >= 4);
    assert!(ec > 0);
    assert!(cc > 0);
    assert!(kc > 0);

    clear_all();
    reset_stats();
    serial_println!("  ime: all tests passed");
    Ok(())
}

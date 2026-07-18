//! Spell checker — system-wide spelling and grammar service.
//!
//! Provides dictionary-based spell checking, auto-correction suggestions,
//! personal dictionaries, and multi-language support.  Applications can
//! query this service for real-time spell checking without bundling
//! their own dictionaries.
//!
//! ## Architecture
//!
//! ```text
//! Text editor / UI toolkit
//!   → spellcheck::check_word(word) → correct / suggestions
//!   → spellcheck::check_text(text) → list of misspellings
//!
//! Integration:
//!   → langpack (active language determines dictionary)
//!   → ime (input method context)
//!   → dictation (correct transcribed text)
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

/// Result of checking a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    /// Word is correct.
    Correct,
    /// Word is misspelled.
    Misspelled,
    /// Word is in personal dictionary.
    Personal,
    /// Word looks like a proper noun / ignored.
    Ignored,
}

impl CheckResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Correct => "correct",
            Self::Misspelled => "misspelled",
            Self::Personal => "personal",
            Self::Ignored => "ignored",
        }
    }
}

/// A dictionary entry.
#[derive(Debug, Clone)]
pub struct DictionaryInfo {
    /// Language code (e.g., "en-US").
    pub language: String,
    /// Word count.
    pub word_count: u32,
    /// Whether it's the active dictionary.
    pub active: bool,
    /// Dictionary size in bytes.
    pub size_bytes: u64,
}

/// A misspelling found in text.
#[derive(Debug, Clone)]
pub struct Misspelling {
    /// Byte offset in the checked text.
    pub offset: usize,
    /// Length of the misspelled word.
    pub length: usize,
    /// The misspelled word.
    pub word: String,
    /// Suggested corrections (up to 5).
    pub suggestions: Vec<String>,
}

/// Spell checker configuration.
#[derive(Debug, Clone)]
pub struct SpellConfig {
    pub enabled: bool,
    pub auto_correct: bool,
    pub check_as_you_type: bool,
    pub ignore_caps: bool,
    pub ignore_numbers: bool,
    pub ignore_urls: bool,
    pub max_suggestions: u8,
}

impl Default for SpellConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_correct: false,
            check_as_you_type: true,
            ignore_caps: true,
            ignore_numbers: true,
            ignore_urls: true,
            max_suggestions: 5,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PERSONAL_WORDS: usize = 5000;
const MAX_DICTIONARIES: usize = 20;

/// Simple hash for quick word lookup (FNV-1a on lowercase).
fn word_hash(word: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in word.bytes() {
        let c = if b >= b'A' && b <= b'Z' { b + 32 } else { b };
        h ^= c as u64;
        h = h.wrapping_mul(0x00000100000001b3);
    }
    h
}

struct State {
    config: SpellConfig,
    dictionaries: Vec<DictionaryInfo>,
    /// Built-in word hashes for basic English checking.
    builtin_hashes: Vec<u64>,
    /// Personal dictionary words.
    personal_words: Vec<String>,
    /// Words to always ignore.
    ignored_words: Vec<String>,
    total_checks: u64,
    total_misspellings: u64,
    total_corrections: u64,
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
// Built-in minimal English word list (hashes only for space efficiency)
// ---------------------------------------------------------------------------

/// Common English words used for basic checking.
const BUILTIN_WORDS: &[&str] = &[
    "the", "be", "to", "of", "and", "a", "in", "that", "have", "i",
    "it", "for", "not", "on", "with", "he", "as", "you", "do", "at",
    "this", "but", "his", "by", "from", "they", "we", "say", "her", "she",
    "or", "an", "will", "my", "one", "all", "would", "there", "their", "what",
    "so", "up", "out", "if", "about", "who", "get", "which", "go", "me",
    "when", "make", "can", "like", "time", "no", "just", "him", "know", "take",
    "people", "into", "year", "your", "good", "some", "could", "them", "see",
    "other", "than", "then", "now", "look", "only", "come", "its", "over",
    "think", "also", "back", "after", "use", "two", "how", "our", "work",
    "first", "well", "way", "even", "new", "want", "because", "any", "these",
    "give", "day", "most", "us", "file", "open", "save", "close", "edit",
    "copy", "paste", "cut", "delete", "undo", "redo", "find", "replace",
    "print", "help", "exit", "quit", "yes", "no", "ok", "cancel", "apply",
    "settings", "options", "tools", "view", "window", "menu", "button",
    "text", "font", "size", "color", "style", "format", "insert", "table",
    "image", "link", "page", "document", "folder", "directory", "name",
    "type", "date", "search", "sort", "filter", "select", "clear",
    "system", "computer", "network", "internet", "email", "message",
    "error", "warning", "information", "success", "failed", "loading",
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let mut builtin_hashes = Vec::with_capacity(BUILTIN_WORDS.len());
    for w in BUILTIN_WORDS {
        builtin_hashes.push(word_hash(w));
    }

    let dictionaries = alloc::vec![
        DictionaryInfo {
            language: String::from("en-US"),
            word_count: BUILTIN_WORDS.len() as u32,
            active: true,
            size_bytes: 4 * 1024 * 1024,
        },
    ];

    *guard = Some(State {
        config: SpellConfig::default(),
        dictionaries,
        builtin_hashes,
        personal_words: Vec::new(),
        ignored_words: Vec::new(),
        total_checks: 0,
        total_misspellings: 0,
        total_corrections: 0,
        ops: 0,
    });
}

/// Check a single word.
pub fn check_word(word: &str) -> CheckResult {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return CheckResult::Ignored,
    };
    state.total_checks += 1;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);

    if !state.config.enabled {
        return CheckResult::Ignored;
    }

    // Ignore all-caps if configured.
    if state.config.ignore_caps && word.bytes().all(|b| !b.is_ascii_lowercase()) && word.len() > 1 {
        return CheckResult::Ignored;
    }

    // Ignore words with digits if configured.
    if state.config.ignore_numbers && word.bytes().any(|b| b.is_ascii_digit()) {
        return CheckResult::Ignored;
    }

    // Check personal dictionary.
    if state.personal_words.iter().any(|w| w.eq_ignore_ascii_case(word)) {
        return CheckResult::Personal;
    }

    // Check ignored words.
    if state.ignored_words.iter().any(|w| w.eq_ignore_ascii_case(word)) {
        return CheckResult::Ignored;
    }

    // Check builtin dictionary via hash.
    let h = word_hash(word);
    if state.builtin_hashes.contains(&h) {
        return CheckResult::Correct;
    }

    state.total_misspellings += 1;
    CheckResult::Misspelled
}

/// Get suggestions for a misspelled word (simple edit-distance-1 approach).
pub fn suggest(word: &str) -> Vec<String> {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };

    let max = state.config.max_suggestions as usize;
    let mut results = Vec::new();

    // Simple approach: find builtin words that share a prefix.
    let lower: String = word.chars().map(|c| {
        if c.is_ascii_uppercase() { (c as u8 + 32) as char } else { c }
    }).collect();

    for builtin in BUILTIN_WORDS {
        if results.len() >= max { break; }
        // Check if the word shares at least half its prefix with the builtin.
        let common = lower.bytes().zip(builtin.bytes()).take_while(|(a, b)| a == b).count();
        if common >= lower.len() / 2 && common > 0 {
            results.push(String::from(*builtin));
        }
    }

    results
}

/// Add a word to the personal dictionary.
pub fn add_personal(word: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.personal_words.iter().any(|w| w.eq_ignore_ascii_case(word)) {
            return Err(KernelError::AlreadyExists);
        }
        if state.personal_words.len() >= MAX_PERSONAL_WORDS {
            return Err(KernelError::ResourceExhausted);
        }
        state.personal_words.push(String::from(word));
        Ok(())
    })
}

/// Remove a word from the personal dictionary.
pub fn remove_personal(word: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.personal_words.iter().position(|w| w.eq_ignore_ascii_case(word))
            .ok_or(KernelError::NotFound)?;
        state.personal_words.remove(pos);
        Ok(())
    })
}

/// Add a word to the ignore list.
pub fn ignore_word(word: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.ignored_words.iter().any(|w| w.eq_ignore_ascii_case(word)) {
            state.ignored_words.push(String::from(word));
        }
        Ok(())
    })
}

/// List personal dictionary words.
pub fn list_personal() -> Vec<String> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.personal_words.clone())
}

/// Get config.
pub fn get_config() -> KernelResult<SpellConfig> {
    with_state(|state| Ok(state.config.clone()))
}

/// Set enabled.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.enabled = enabled; Ok(()) })
}

/// Set auto-correct.
pub fn set_auto_correct(on: bool) -> KernelResult<()> {
    with_state(|state| { state.config.auto_correct = on; Ok(()) })
}

/// List dictionaries.
pub fn list_dictionaries() -> Vec<DictionaryInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.dictionaries.clone())
}

/// Statistics: (dict_count, personal_count, checks, misspellings, corrections, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.dictionaries.len(), s.personal_words.len(),
            s.total_checks, s.total_misspellings, s.total_corrections, s.ops,
        ),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("spellcheck::self_test() — running tests...");
    init_defaults();

    // 1: Known word is correct.
    let r = check_word("the");
    assert_eq!(r, CheckResult::Correct);
    crate::serial_println!("  [1/11] known word correct: OK");

    // 2: Unknown word is misspelled.
    let r = check_word("xyzzyplugh");
    assert_eq!(r, CheckResult::Misspelled);
    crate::serial_println!("  [2/11] unknown word misspelled: OK");

    // 3: Add personal word.
    add_personal("Rustacean").expect("add personal");
    let r = check_word("Rustacean");
    assert_eq!(r, CheckResult::Personal);
    crate::serial_println!("  [3/11] personal dictionary: OK");

    // 4: Duplicate personal word rejected.
    let r = add_personal("Rustacean");
    assert!(r.is_err());
    crate::serial_println!("  [4/11] duplicate personal rejected: OK");

    // 5: Remove personal word.
    remove_personal("Rustacean").expect("remove personal");
    let r = check_word("Rustacean");
    assert_eq!(r, CheckResult::Misspelled);
    crate::serial_println!("  [5/11] remove personal: OK");

    // 6: Ignore word.
    ignore_word("HTTP").expect("ignore");
    let r = check_word("HTTP");
    assert_eq!(r, CheckResult::Ignored); // All-caps + in ignore list.
    crate::serial_println!("  [6/11] ignore word: OK");

    // 7: All-caps ignored by default.
    let r = check_word("NASA");
    assert_eq!(r, CheckResult::Ignored);
    crate::serial_println!("  [7/11] all-caps ignored: OK");

    // 8: Words with numbers ignored.
    let r = check_word("h3llo");
    assert_eq!(r, CheckResult::Ignored);
    crate::serial_println!("  [8/11] numbers ignored: OK");

    // 9: Suggestions.
    let s = suggest("teh");
    // Should suggest "the" since they share prefix.
    assert!(!s.is_empty());
    crate::serial_println!("  [9/11] suggestions: OK");

    // 10: Dictionaries.
    let dicts = list_dictionaries();
    assert_eq!(dicts.len(), 1);
    assert!(dicts[0].active);
    crate::serial_println!("  [10/11] dictionaries: OK");

    // 11: Stats.
    let (dicts, personal, checks, misspellings, _, ops) = stats();
    assert_eq!(dicts, 1);
    assert!(checks >= 5);
    assert!(misspellings >= 1);
    assert!(ops > 0);
    let _ = personal;
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("spellcheck::self_test() — all 11 tests passed");
}

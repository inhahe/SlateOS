//! Dictation engine — speech-to-text input for accessibility.
//!
//! Provides voice-based text input as an accessibility feature,
//! with language selection, custom vocabulary, and per-app dictation
//! control. Works with the audio input pipeline for microphone access.
//!
//! ## Architecture
//!
//! ```text
//! Microphone → audio pipeline → dictation::process_audio()
//!   → transcription → insert into focused text field
//!
//! Settings panel → Accessibility → Dictation
//!   → dictation::set_enabled() / set_language()
//!
//! Integration:
//!   → audiodevice (microphone selection)
//!   → a11y (accessibility framework)
//!   → ime (input method integration)
//!   → keylayout (hotkey to toggle dictation)
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

/// Dictation engine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationState {
    /// Not listening.
    Idle,
    /// Actively listening and transcribing.
    Listening,
    /// Processing audio (brief delay).
    Processing,
    /// Paused (user paused dictation).
    Paused,
}

impl DictationState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Listening => "Listening",
            Self::Processing => "Processing",
            Self::Paused => "Paused",
        }
    }
}

/// Dictation language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationLanguage {
    EnUs,
    EnGb,
    DeDE,
    FrFR,
    EsES,
    JaJP,
    KoKR,
    ZhCN,
    PtBR,
    ItIT,
}

impl DictationLanguage {
    pub fn label(self) -> &'static str {
        match self {
            Self::EnUs => "English (US)",
            Self::EnGb => "English (UK)",
            Self::DeDE => "German",
            Self::FrFR => "French",
            Self::EsES => "Spanish",
            Self::JaJP => "Japanese",
            Self::KoKR => "Korean",
            Self::ZhCN => "Chinese (Simplified)",
            Self::PtBR => "Portuguese (Brazil)",
            Self::ItIT => "Italian",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
            Self::EnGb => "en-GB",
            Self::DeDE => "de-DE",
            Self::FrFR => "fr-FR",
            Self::EsES => "es-ES",
            Self::JaJP => "ja-JP",
            Self::KoKR => "ko-KR",
            Self::ZhCN => "zh-CN",
            Self::PtBR => "pt-BR",
            Self::ItIT => "it-IT",
        }
    }
}

/// A transcription result.
#[derive(Debug, Clone)]
pub struct Transcription {
    /// Unique ID.
    pub id: u64,
    /// Transcribed text.
    pub text: String,
    /// Language used.
    pub language: DictationLanguage,
    /// Confidence (0-100).
    pub confidence: u8,
    /// Timestamp (ns since boot).
    pub timestamp_ns: u64,
    /// Duration of audio (ms).
    pub duration_ms: u32,
    /// Target application.
    pub target_app: String,
}

/// Custom vocabulary entry.
#[derive(Debug, Clone)]
pub struct VocabEntry {
    /// The word or phrase.
    pub phrase: String,
    /// Phonetic hint.
    pub phonetic: String,
}

/// Dictation configuration.
#[derive(Debug, Clone)]
pub struct DictationConfig {
    /// Whether dictation is available.
    pub enabled: bool,
    /// Current language.
    pub language: DictationLanguage,
    /// Auto-punctuation.
    pub auto_punctuation: bool,
    /// Profanity filter.
    pub profanity_filter: bool,
    /// Play feedback sound on start/stop.
    pub feedback_sound: bool,
    /// Show visual indicator when listening.
    pub visual_indicator: bool,
    /// Hotkey to toggle (as string, e.g., "Ctrl+D").
    pub hotkey: String,
    /// Microphone device ID (0 = default).
    pub mic_device_id: u32,
}

impl Default for DictationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            language: DictationLanguage::EnUs,
            auto_punctuation: true,
            profanity_filter: false,
            feedback_sound: true,
            visual_indicator: true,
            hotkey: String::from("Super+D"),
            mic_device_id: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HISTORY: usize = 500;
const MAX_VOCAB: usize = 1000;

struct State {
    config: DictationConfig,
    state: DictationState,
    history: Vec<Transcription>,
    custom_vocab: Vec<VocabEntry>,
    next_id: u64,
    total_transcriptions: u64,
    total_words: u64,
    total_duration_ms: u64,
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
        config: DictationConfig::default(),
        state: DictationState::Idle,
        history: Vec::new(),
        custom_vocab: Vec::new(),
        next_id: 1,
        total_transcriptions: 0,
        total_words: 0,
        total_duration_ms: 0,
        ops: 0,
    });
}

pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.enabled = enabled;
        if !enabled {
            state.state = DictationState::Idle;
        }
        Ok(())
    })
}

pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.config.enabled)
}

/// Start listening.
pub fn start_listening() -> KernelResult<()> {
    with_state(|state| {
        if !state.config.enabled {
            return Err(KernelError::NotSupported);
        }
        state.state = DictationState::Listening;
        Ok(())
    })
}

/// Stop listening.
pub fn stop_listening() -> KernelResult<()> {
    with_state(|state| {
        state.state = DictationState::Idle;
        Ok(())
    })
}

/// Pause dictation.
pub fn pause() -> KernelResult<()> {
    with_state(|state| {
        if state.state == DictationState::Listening {
            state.state = DictationState::Paused;
        }
        Ok(())
    })
}

/// Resume dictation.
pub fn resume() -> KernelResult<()> {
    with_state(|state| {
        if state.state == DictationState::Paused {
            state.state = DictationState::Listening;
        }
        Ok(())
    })
}

/// Get current state.
pub fn current_state() -> DictationState {
    STATE.lock().as_ref().map_or(DictationState::Idle, |s| s.state)
}

/// Submit a transcription result (from speech engine).
pub fn submit_transcription(text: &str, confidence: u8, duration_ms: u32, target_app: &str) -> KernelResult<u64> {
    with_state(|state| {
        let id = state.next_id;
        state.next_id += 1;
        let now = crate::hpet::elapsed_ns();
        let word_count = text.split_whitespace().count() as u64;

        state.history.push(Transcription {
            id,
            text: String::from(text),
            language: state.config.language,
            confidence,
            timestamp_ns: now,
            duration_ms,
            target_app: String::from(target_app),
        });

        state.total_transcriptions += 1;
        state.total_words += word_count;
        state.total_duration_ms += duration_ms as u64;

        while state.history.len() > MAX_HISTORY {
            state.history.remove(0);
        }

        Ok(id)
    })
}

/// Get recent transcriptions.
pub fn recent_transcriptions(count: usize) -> Vec<Transcription> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let start = if s.history.len() > count { s.history.len() - count } else { 0 };
            s.history[start..].iter().rev().cloned().collect()
        }
        None => Vec::new(),
    }
}

pub fn set_language(language: DictationLanguage) -> KernelResult<()> {
    with_state(|state| { state.config.language = language; Ok(()) })
}

pub fn set_auto_punctuation(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.auto_punctuation = enabled; Ok(()) })
}

pub fn set_profanity_filter(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.profanity_filter = enabled; Ok(()) })
}

pub fn set_hotkey(hotkey: &str) -> KernelResult<()> {
    with_state(|state| { state.config.hotkey = String::from(hotkey); Ok(()) })
}

pub fn get_config() -> KernelResult<DictationConfig> {
    with_state(|state| Ok(state.config.clone()))
}

/// Add a custom vocabulary word.
pub fn add_vocab(phrase: &str, phonetic: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.custom_vocab.len() >= MAX_VOCAB {
            return Err(KernelError::ResourceExhausted);
        }
        if state.custom_vocab.iter().any(|v| v.phrase == phrase) {
            return Err(KernelError::AlreadyExists);
        }
        state.custom_vocab.push(VocabEntry {
            phrase: String::from(phrase),
            phonetic: String::from(phonetic),
        });
        Ok(())
    })
}

/// Remove a custom vocabulary word.
pub fn remove_vocab(phrase: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.custom_vocab.iter().position(|v| v.phrase == phrase)
            .ok_or(KernelError::NotFound)?;
        state.custom_vocab.remove(pos);
        Ok(())
    })
}

/// List custom vocabulary.
pub fn list_vocab() -> Vec<VocabEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.custom_vocab.clone())
}

/// Clear transcription history.
pub fn clear_history() -> KernelResult<usize> {
    with_state(|state| {
        let count = state.history.len();
        state.history.clear();
        Ok(count)
    })
}

/// Statistics: (state_label, language_code, transcriptions, words, vocab_count, ops).
pub fn stats() -> (&'static str, &'static str, u64, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.state.label(),
            s.config.language.code(),
            s.total_transcriptions,
            s.total_words,
            s.custom_vocab.len(),
            s.ops,
        ),
        None => ("N/A", "N/A", 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("dictation::self_test() — running tests...");
    init_defaults();

    // 1: Disabled by default.
    assert!(!is_enabled());
    assert_eq!(current_state(), DictationState::Idle);
    crate::serial_println!("  [1/11] disabled by default: OK");

    // 2: Cannot start when disabled.
    assert!(start_listening().is_err());
    crate::serial_println!("  [2/11] start while disabled: OK");

    // 3: Enable and start.
    set_enabled(true).expect("enable");
    start_listening().expect("start");
    assert_eq!(current_state(), DictationState::Listening);
    crate::serial_println!("  [3/11] enable and start: OK");

    // 4: Pause/resume.
    pause().expect("pause");
    assert_eq!(current_state(), DictationState::Paused);
    resume().expect("resume");
    assert_eq!(current_state(), DictationState::Listening);
    crate::serial_println!("  [4/11] pause/resume: OK");

    // 5: Submit transcription.
    let id = submit_transcription("Hello world this is a test", 95, 3000, "editor").expect("submit");
    assert!(id > 0);
    crate::serial_println!("  [5/11] submit transcription: OK");

    // 6: Recent transcriptions.
    let recent = recent_transcriptions(5);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].text, "Hello world this is a test");
    crate::serial_println!("  [6/11] recent transcriptions: OK");

    // 7: Set language.
    set_language(DictationLanguage::DeDE).expect("set language");
    let cfg = get_config().expect("get config");
    assert_eq!(cfg.language, DictationLanguage::DeDE);
    crate::serial_println!("  [7/11] set language: OK");

    // 8: Add custom vocabulary.
    add_vocab("Kubernetes", "koo-ber-net-eez").expect("add vocab");
    let vocab = list_vocab();
    assert_eq!(vocab.len(), 1);
    crate::serial_println!("  [8/11] custom vocabulary: OK");

    // 9: Remove vocabulary.
    remove_vocab("Kubernetes").expect("remove vocab");
    assert!(list_vocab().is_empty());
    crate::serial_println!("  [9/11] remove vocabulary: OK");

    // 10: Stop listening.
    stop_listening().expect("stop");
    assert_eq!(current_state(), DictationState::Idle);
    crate::serial_println!("  [10/11] stop listening: OK");

    // 11: Stats.
    let (state_label, lang_code, transcriptions, words, _vocab, ops) = stats();
    assert_eq!(state_label, "Idle");
    assert_eq!(lang_code, "de-DE");
    assert!(transcriptions >= 1);
    assert!(words >= 6);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("dictation::self_test() — all 11 tests passed");
}

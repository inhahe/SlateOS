//! Speech I/O — text-to-speech and speech recognition services.
//!
//! Provides a unified speech interface: TTS voice synthesis for
//! accessibility and notifications, plus speech recognition for
//! voice commands and dictation.
//!
//! ## Architecture
//!
//! ```text
//! TTS request
//!   → speechio::speak(text, voice, rate, pitch)
//!     → queued in utterance queue
//!     → synthesizer processes queue
//!
//! Speech recognition
//!   → speechio::start_listening(grammar)
//!     → audio captured from microphone
//!     → recognition results dispatched to app
//!
//! Integration:
//!   → screenreader (TTS output)
//!   → dictation (voice-to-text)
//!   → a11y (accessibility speech)
//!   → notifcenter (spoken notifications)
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

/// Available TTS voice.
#[derive(Debug, Clone)]
pub struct Voice {
    pub id: u32,
    pub name: String,
    pub language: String,
    pub gender: VoiceGender,
    pub is_default: bool,
}

/// Voice gender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceGender {
    Male,
    Female,
    Neutral,
}

impl VoiceGender {
    pub fn label(self) -> &'static str {
        match self {
            Self::Male => "Male",
            Self::Female => "Female",
            Self::Neutral => "Neutral",
        }
    }
}

/// TTS utterance state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtteranceState {
    Queued,
    Speaking,
    Completed,
    Cancelled,
}

impl UtteranceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Speaking => "Speaking",
            Self::Completed => "Completed",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// A TTS utterance (queued or active speech).
#[derive(Debug, Clone)]
pub struct Utterance {
    pub id: u32,
    pub text: String,
    pub voice_id: u32,
    /// Speech rate in percent (50 = half speed, 100 = normal, 200 = double).
    pub rate_percent: u32,
    /// Pitch in percent (50 = low, 100 = normal, 200 = high).
    pub pitch_percent: u32,
    /// Volume in percent (0-100).
    pub volume_percent: u32,
    pub state: UtteranceState,
    pub created_ns: u64,
}

/// Recognition session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionState {
    Idle,
    Listening,
    Processing,
    Error,
}

impl RecognitionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Listening => "Listening",
            Self::Processing => "Processing",
            Self::Error => "Error",
        }
    }
}

/// Speech recognition result.
#[derive(Debug, Clone)]
pub struct RecognitionResult {
    pub id: u32,
    pub text: String,
    /// Confidence 0-10000 (hundredths of percent).
    pub confidence: u32,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_VOICES: usize = 20;
const MAX_UTTERANCES: usize = 100;
const MAX_RESULTS: usize = 200;

struct State {
    voices: Vec<Voice>,
    utterances: Vec<Utterance>,
    results: Vec<RecognitionResult>,
    recognition_state: RecognitionState,
    next_voice_id: u32,
    next_utterance_id: u32,
    next_result_id: u32,
    default_voice_id: u32,
    tts_enabled: bool,
    recognition_enabled: bool,
    total_spoken: u64,
    total_recognized: u64,
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

    let voices = alloc::vec![
        Voice { id: 1, name: String::from("Default"), language: String::from("en-US"), gender: VoiceGender::Neutral, is_default: true },
        Voice { id: 2, name: String::from("Alex"), language: String::from("en-US"), gender: VoiceGender::Male, is_default: false },
        Voice { id: 3, name: String::from("Samantha"), language: String::from("en-US"), gender: VoiceGender::Female, is_default: false },
    ];

    *guard = Some(State {
        voices,
        utterances: Vec::new(),
        results: Vec::new(),
        recognition_state: RecognitionState::Idle,
        next_voice_id: 4,
        next_utterance_id: 1,
        next_result_id: 1,
        default_voice_id: 1,
        tts_enabled: true,
        recognition_enabled: false,
        total_spoken: 0,
        total_recognized: 0,
        ops: 0,
    });
}

/// Add a custom voice.
pub fn add_voice(name: &str, language: &str, gender: VoiceGender) -> KernelResult<u32> {
    with_state(|state| {
        if state.voices.len() >= MAX_VOICES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_voice_id;
        state.next_voice_id += 1;
        let is_first = state.voices.is_empty();
        state.voices.push(Voice {
            id,
            name: String::from(name),
            language: String::from(language),
            gender,
            is_default: is_first,
        });
        if is_first { state.default_voice_id = id; }
        Ok(id)
    })
}

/// Remove a voice.
pub fn remove_voice(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.voices.iter().position(|v| v.id == id)
            .ok_or(KernelError::NotFound)?;
        state.voices.remove(pos);
        if state.default_voice_id == id {
            state.default_voice_id = state.voices.first().map(|v| v.id).unwrap_or(0);
            if let Some(v) = state.voices.first_mut() {
                v.is_default = true;
            }
        }
        Ok(())
    })
}

/// Set default voice.
pub fn set_default_voice(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.voices.iter().any(|v| v.id == id) {
            return Err(KernelError::NotFound);
        }
        for v in state.voices.iter_mut() {
            v.is_default = v.id == id;
        }
        state.default_voice_id = id;
        Ok(())
    })
}

/// Queue a TTS utterance.
pub fn speak(text: &str, voice_id: Option<u32>, rate_percent: u32, pitch_percent: u32, volume_percent: u32) -> KernelResult<u32> {
    with_state(|state| {
        if !state.tts_enabled {
            return Err(KernelError::NotSupported);
        }
        if state.utterances.len() >= MAX_UTTERANCES {
            // Auto-evict completed utterances.
            state.utterances.retain(|u| u.state != UtteranceState::Completed && u.state != UtteranceState::Cancelled);
            if state.utterances.len() >= MAX_UTTERANCES {
                return Err(KernelError::ResourceExhausted);
            }
        }
        let vid = voice_id.unwrap_or(state.default_voice_id);
        if !state.voices.iter().any(|v| v.id == vid) {
            return Err(KernelError::NotFound);
        }
        let id = state.next_utterance_id;
        state.next_utterance_id += 1;
        state.total_spoken += 1;

        state.utterances.push(Utterance {
            id,
            text: String::from(text),
            voice_id: vid,
            rate_percent: rate_percent.clamp(25, 400),
            pitch_percent: pitch_percent.clamp(25, 400),
            volume_percent: volume_percent.clamp(0, 100),
            state: UtteranceState::Queued,
            created_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Cancel an utterance.
pub fn cancel_utterance(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let utt = state.utterances.iter_mut().find(|u| u.id == id)
            .ok_or(KernelError::NotFound)?;
        utt.state = UtteranceState::Cancelled;
        Ok(())
    })
}

/// Cancel all queued/active utterances (stop speaking).
pub fn stop_speaking() -> KernelResult<()> {
    with_state(|state| {
        for u in state.utterances.iter_mut() {
            if u.state == UtteranceState::Queued || u.state == UtteranceState::Speaking {
                u.state = UtteranceState::Cancelled;
            }
        }
        Ok(())
    })
}

/// Mark an utterance as speaking (for synthesis engine).
pub fn mark_speaking(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let utt = state.utterances.iter_mut().find(|u| u.id == id)
            .ok_or(KernelError::NotFound)?;
        utt.state = UtteranceState::Speaking;
        Ok(())
    })
}

/// Mark an utterance as completed.
pub fn mark_completed(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let utt = state.utterances.iter_mut().find(|u| u.id == id)
            .ok_or(KernelError::NotFound)?;
        utt.state = UtteranceState::Completed;
        Ok(())
    })
}

/// Enable/disable TTS.
pub fn set_tts_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.tts_enabled = enabled;
        if !enabled {
            for u in state.utterances.iter_mut() {
                if u.state == UtteranceState::Queued || u.state == UtteranceState::Speaking {
                    u.state = UtteranceState::Cancelled;
                }
            }
        }
        Ok(())
    })
}

/// Start speech recognition.
pub fn start_listening() -> KernelResult<()> {
    with_state(|state| {
        if !state.recognition_enabled {
            return Err(KernelError::NotSupported);
        }
        state.recognition_state = RecognitionState::Listening;
        Ok(())
    })
}

/// Stop speech recognition.
pub fn stop_listening() -> KernelResult<()> {
    with_state(|state| {
        state.recognition_state = RecognitionState::Idle;
        Ok(())
    })
}

/// Enable/disable recognition.
pub fn set_recognition_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.recognition_enabled = enabled;
        if !enabled {
            state.recognition_state = RecognitionState::Idle;
        }
        Ok(())
    })
}

/// Submit a recognition result (from recognition engine).
pub fn submit_result(text: &str, confidence: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.results.len() >= MAX_RESULTS {
            // Evict oldest.
            state.results.remove(0);
        }
        let id = state.next_result_id;
        state.next_result_id += 1;
        state.total_recognized += 1;
        state.results.push(RecognitionResult {
            id,
            text: String::from(text),
            confidence: confidence.min(10000),
            timestamp_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// List voices.
pub fn list_voices() -> Vec<Voice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.voices.clone())
}

/// List queued/active utterances.
pub fn list_utterances() -> Vec<Utterance> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.utterances.clone())
}

/// List recent recognition results.
pub fn list_results() -> Vec<RecognitionResult> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.results.clone())
}

/// Get recognition state.
pub fn recognition_state() -> RecognitionState {
    STATE.lock().as_ref().map_or(RecognitionState::Idle, |s| s.recognition_state)
}

/// Statistics: (voice_count, total_spoken, total_recognized, tts_enabled, ops).
pub fn stats() -> (usize, u64, u64, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.voices.len(), s.total_spoken, s.total_recognized, s.tts_enabled, s.ops),
        None => (0, 0, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("speechio::self_test() — running tests...");
    init_defaults();

    // 1: Default voices.
    let voices = list_voices();
    assert_eq!(voices.len(), 3);
    assert!(voices.iter().any(|v| v.is_default));
    crate::serial_println!("  [1/10] default voices: OK");

    // 2: Add voice.
    let vid = add_voice("Custom", "fr-FR", VoiceGender::Female).expect("add");
    assert_eq!(list_voices().len(), 4);
    crate::serial_println!("  [2/10] add voice: OK");

    // 3: Speak.
    let uid = speak("Hello world", None, 100, 100, 80).expect("speak");
    assert!(uid > 0);
    let utts = list_utterances();
    assert_eq!(utts.len(), 1);
    assert_eq!(utts[0].state, UtteranceState::Queued);
    crate::serial_println!("  [3/10] speak: OK");

    // 4: Mark speaking then completed.
    mark_speaking(uid).expect("speaking");
    let utts = list_utterances();
    assert_eq!(utts[0].state, UtteranceState::Speaking);
    mark_completed(uid).expect("complete");
    let utts = list_utterances();
    assert_eq!(utts[0].state, UtteranceState::Completed);
    crate::serial_println!("  [4/10] state transitions: OK");

    // 5: Cancel utterance.
    let uid2 = speak("Test cancel", Some(vid), 150, 100, 100).expect("speak2");
    cancel_utterance(uid2).expect("cancel");
    let u = list_utterances().into_iter().find(|u| u.id == uid2).expect("find");
    assert_eq!(u.state, UtteranceState::Cancelled);
    crate::serial_println!("  [5/10] cancel: OK");

    // 6: Stop all speaking.
    let _ = speak("Test 3", None, 100, 100, 80).expect("speak3");
    stop_speaking().expect("stop");
    crate::serial_println!("  [6/10] stop speaking: OK");

    // 7: Disable TTS rejects speak.
    set_tts_enabled(false).expect("disable");
    let result = speak("Should fail", None, 100, 100, 80);
    assert!(result.is_err());
    set_tts_enabled(true).expect("re-enable");
    crate::serial_println!("  [7/10] TTS disable: OK");

    // 8: Recognition (enable first).
    set_recognition_enabled(true).expect("rec_enable");
    start_listening().expect("listen");
    assert_eq!(recognition_state(), RecognitionState::Listening);
    crate::serial_println!("  [8/10] recognition: OK");

    // 9: Submit result.
    let rid = submit_result("hello computer", 9500).expect("result");
    assert!(rid > 0);
    let results = list_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].confidence, 9500);
    crate::serial_println!("  [9/10] recognition result: OK");

    // 10: Stats.
    stop_listening().expect("stop_listen");
    let (vcount, spoken, recognized, tts_on, ops) = stats();
    assert_eq!(vcount, 4);
    assert!(spoken >= 3);
    assert_eq!(recognized, 1);
    assert!(tts_on);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    crate::serial_println!("speechio::self_test() — all 10 tests passed");
}

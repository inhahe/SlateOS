//! Voice Control — voice commands for OS actions.
//!
//! Provides voice-driven control of OS features including window
//! management, app launching, system settings, and text dictation.
//!
//! ## Architecture
//!
//! ```text
//! Microphone input
//!   → voicecontrol::process_audio() → recognized command
//!   → voicecontrol::execute_command() → perform action
//!
//! Configuration
//!   → voicecontrol::set_wake_word(word)
//!   → voicecontrol::add_custom_command(phrase, action)
//!
//! Integration:
//!   → speechio (speech I/O)
//!   → dictation (dictation mode)
//!   → a11y (accessibility)
//!   → kbshortcuts (command execution)
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

/// Voice command category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    /// Window management (close, minimize, maximize).
    Window,
    /// App launching.
    AppLaunch,
    /// System settings.
    Settings,
    /// Text dictation.
    Dictation,
    /// Navigation (scroll, switch tabs).
    Navigation,
    /// Media controls.
    Media,
    /// Custom user command.
    Custom,
}

impl CommandCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Window => "Window",
            Self::AppLaunch => "App Launch",
            Self::Settings => "Settings",
            Self::Dictation => "Dictation",
            Self::Navigation => "Navigation",
            Self::Media => "Media",
            Self::Custom => "Custom",
        }
    }
}

/// Recognition confidence level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
    Rejected,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
            Self::Rejected => "Rejected",
        }
    }
}

/// A voice command definition.
#[derive(Debug, Clone)]
pub struct VoiceCommand {
    pub id: u32,
    pub phrase: String,
    pub category: CommandCategory,
    pub action: String,
    pub enabled: bool,
    pub use_count: u64,
}

/// A recognized command result.
#[derive(Debug, Clone)]
pub struct RecognitionResult {
    pub phrase: String,
    pub confidence: Confidence,
    pub matched_command_id: Option<u32>,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_COMMANDS: usize = 200;
const MAX_HISTORY: usize = 100;

struct State {
    commands: Vec<VoiceCommand>,
    history: Vec<RecognitionResult>,
    next_id: u32,
    wake_word: String,
    listening: bool,
    enabled: bool,
    min_confidence: Confidence,
    total_recognitions: u64,
    total_executed: u64,
    total_rejected: u64,
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
        commands: alloc::vec![
            VoiceCommand { id: 1, phrase: String::from("close window"), category: CommandCategory::Window, action: String::from("window.close"), enabled: true, use_count: 0 },
            VoiceCommand { id: 2, phrase: String::from("minimize window"), category: CommandCategory::Window, action: String::from("window.minimize"), enabled: true, use_count: 0 },
            VoiceCommand { id: 3, phrase: String::from("maximize window"), category: CommandCategory::Window, action: String::from("window.maximize"), enabled: true, use_count: 0 },
            VoiceCommand { id: 4, phrase: String::from("open browser"), category: CommandCategory::AppLaunch, action: String::from("launch.browser"), enabled: true, use_count: 0 },
            VoiceCommand { id: 5, phrase: String::from("open terminal"), category: CommandCategory::AppLaunch, action: String::from("launch.terminal"), enabled: true, use_count: 0 },
            VoiceCommand { id: 6, phrase: String::from("play music"), category: CommandCategory::Media, action: String::from("media.play"), enabled: true, use_count: 0 },
            VoiceCommand { id: 7, phrase: String::from("pause music"), category: CommandCategory::Media, action: String::from("media.pause"), enabled: true, use_count: 0 },
            VoiceCommand { id: 8, phrase: String::from("scroll down"), category: CommandCategory::Navigation, action: String::from("nav.scrolldown"), enabled: true, use_count: 0 },
            VoiceCommand { id: 9, phrase: String::from("scroll up"), category: CommandCategory::Navigation, action: String::from("nav.scrollup"), enabled: true, use_count: 0 },
            VoiceCommand { id: 10, phrase: String::from("take screenshot"), category: CommandCategory::Settings, action: String::from("screenshot.take"), enabled: true, use_count: 0 },
        ],
        history: Vec::new(),
        next_id: 11,
        wake_word: String::from("hey computer"),
        listening: false,
        enabled: false,
        min_confidence: Confidence::Medium,
        total_recognitions: 0,
        total_executed: 0,
        total_rejected: 0,
        ops: 0,
    });
}

/// Enable/disable voice control.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        Ok(())
    })
}

/// Start/stop listening.
pub fn set_listening(listening: bool) -> KernelResult<()> {
    with_state(|state| {
        if !state.enabled && listening {
            return Err(KernelError::NotSupported);
        }
        state.listening = listening;
        Ok(())
    })
}

/// Set wake word.
pub fn set_wake_word(word: &str) -> KernelResult<()> {
    with_state(|state| {
        state.wake_word = String::from(word);
        Ok(())
    })
}

/// Add a custom voice command.
pub fn add_command(phrase: &str, category: CommandCategory, action: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.commands.len() >= MAX_COMMANDS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.commands.push(VoiceCommand {
            id,
            phrase: String::from(phrase),
            category,
            action: String::from(action),
            enabled: true,
            use_count: 0,
        });
        Ok(id)
    })
}

/// Remove a command.
pub fn remove_command(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.commands.len();
        state.commands.retain(|c| c.id != id);
        if state.commands.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Enable/disable a command.
pub fn set_command_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let cmd = state.commands.iter_mut().find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        cmd.enabled = enabled;
        Ok(())
    })
}

/// Recognize a phrase (find matching command).
pub fn recognize(phrase: &str, confidence: Confidence) -> KernelResult<Option<String>> {
    with_state(|state| {
        state.total_recognitions += 1;
        let now = crate::hpet::elapsed_ns();
        let phrase_lower = phrase.to_lowercase();

        // Check minimum confidence.
        let conf_ok = matches!(
            (confidence, state.min_confidence),
            (Confidence::High, _)
                | (Confidence::Medium, Confidence::Medium | Confidence::Low)
                | (Confidence::Low, Confidence::Low)
        );

        if !conf_ok {
            state.total_rejected += 1;
            if state.history.len() >= MAX_HISTORY { state.history.remove(0); }
            state.history.push(RecognitionResult {
                phrase: String::from(phrase), confidence,
                matched_command_id: None, timestamp_ns: now,
            });
            return Ok(None);
        }

        // Find matching command.
        let matched = state.commands.iter_mut()
            .find(|c| c.enabled && c.phrase.to_lowercase() == phrase_lower);

        if let Some(cmd) = matched {
            cmd.use_count += 1;
            let action = cmd.action.clone();
            let cmd_id = cmd.id;
            state.total_executed += 1;
            if state.history.len() >= MAX_HISTORY { state.history.remove(0); }
            state.history.push(RecognitionResult {
                phrase: String::from(phrase), confidence,
                matched_command_id: Some(cmd_id), timestamp_ns: now,
            });
            Ok(Some(action))
        } else {
            if state.history.len() >= MAX_HISTORY { state.history.remove(0); }
            state.history.push(RecognitionResult {
                phrase: String::from(phrase), confidence,
                matched_command_id: None, timestamp_ns: now,
            });
            Ok(None)
        }
    })
}

/// List all commands.
pub fn list_commands() -> Vec<VoiceCommand> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.commands.clone())
}

/// List commands by category.
pub fn list_by_category(category: CommandCategory) -> Vec<VoiceCommand> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.commands.iter().filter(|c| c.category == category).cloned().collect()
    })
}

/// Get recognition history.
pub fn get_history(max: usize) -> Vec<RecognitionResult> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut h = s.history.clone();
        h.reverse();
        h.truncate(max);
        h
    })
}

/// Get current wake word.
pub fn get_wake_word() -> String {
    STATE.lock().as_ref().map_or(String::new(), |s| s.wake_word.clone())
}

/// Is voice control listening?
pub fn is_listening() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.listening)
}

/// Statistics: (command_count, total_recognitions, total_executed, total_rejected, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.commands.len(), s.total_recognitions, s.total_executed, s.total_rejected, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("voicecontrol::self_test() — running tests...");
    init_defaults();

    // 1: Default commands.
    assert_eq!(list_commands().len(), 10);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Enable and listen.
    set_enabled(true).expect("enable");
    set_listening(true).expect("listen");
    assert!(is_listening());
    crate::serial_println!("  [2/8] enable: OK");

    // 3: Recognize known command.
    let action = recognize("close window", Confidence::High).expect("rec1");
    assert_eq!(action, Some(String::from("window.close")));
    crate::serial_println!("  [3/8] recognize: OK");

    // 4: Unknown phrase.
    let action = recognize("do something random", Confidence::High).expect("rec2");
    assert!(action.is_none());
    crate::serial_println!("  [4/8] unknown: OK");

    // 5: Low confidence rejected.
    let action = recognize("close window", Confidence::Low).expect("rec3");
    assert!(action.is_none()); // Default min_confidence is Medium.
    crate::serial_println!("  [5/8] low confidence: OK");

    // 6: Add custom command.
    let _id = add_command("lock screen", CommandCategory::Settings, "screen.lock").expect("add");
    let action = recognize("lock screen", Confidence::High).expect("rec4");
    assert_eq!(action, Some(String::from("screen.lock")));
    crate::serial_println!("  [6/8] custom: OK");

    // 7: History.
    let history = get_history(10);
    assert!(history.len() >= 4);
    crate::serial_println!("  [7/8] history: OK");

    // 8: Stats.
    let (cmds, recognitions, executed, rejected, ops) = stats();
    assert_eq!(cmds, 11);
    assert_eq!(recognitions, 4);
    assert_eq!(executed, 2);
    assert_eq!(rejected, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("voicecontrol::self_test() — all 8 tests passed");
}

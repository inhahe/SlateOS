//! Screen reader — UI narration and accessible object model.
//!
//! Provides text-to-speech narration of UI elements, focus tracking,
//! accessible object tree traversal, and keyboard navigation for
//! visually impaired users.
//!
//! ## Architecture
//!
//! ```text
//! Compositor focus change / UI event
//!   → screenreader::announce(element) → TTS output
//!
//! Keyboard navigation
//!   → screenreader::next_element() / previous_element()
//!   → screenreader::activate() to click/press
//!
//! Integration:
//!   → a11y (accessibility framework)
//!   → audiodevice (audio output)
//!   → dictation (voice input complement)
//!   → keylayout (navigation hotkeys)
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

/// Speech speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechRate {
    VerySlow,
    Slow,
    Normal,
    Fast,
    VeryFast,
}

impl SpeechRate {
    pub fn label(self) -> &'static str {
        match self {
            Self::VerySlow => "Very Slow",
            Self::Slow => "Slow",
            Self::Normal => "Normal",
            Self::Fast => "Fast",
            Self::VeryFast => "Very Fast",
        }
    }

    /// Words per minute (approximate).
    pub fn wpm(self) -> u32 {
        match self {
            Self::VerySlow => 100,
            Self::Slow => 140,
            Self::Normal => 180,
            Self::Fast => 240,
            Self::VeryFast => 320,
        }
    }
}

/// Verbosity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    /// Only essential info (role + name).
    Low,
    /// Role, name, state.
    Medium,
    /// Role, name, state, description, shortcuts.
    High,
}

impl Verbosity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

/// UI element role for accessibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementRole {
    Button,
    TextInput,
    Label,
    Checkbox,
    RadioButton,
    ComboBox,
    List,
    ListItem,
    Menu,
    MenuItem,
    Tab,
    TabPanel,
    Slider,
    ProgressBar,
    Link,
    Image,
    Heading,
    Dialog,
    Window,
    Group,
    Other,
}

impl ElementRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::TextInput => "text input",
            Self::Label => "label",
            Self::Checkbox => "checkbox",
            Self::RadioButton => "radio button",
            Self::ComboBox => "combo box",
            Self::List => "list",
            Self::ListItem => "list item",
            Self::Menu => "menu",
            Self::MenuItem => "menu item",
            Self::Tab => "tab",
            Self::TabPanel => "tab panel",
            Self::Slider => "slider",
            Self::ProgressBar => "progress bar",
            Self::Link => "link",
            Self::Image => "image",
            Self::Heading => "heading",
            Self::Dialog => "dialog",
            Self::Window => "window",
            Self::Group => "group",
            Self::Other => "element",
        }
    }
}

/// An accessible UI element.
#[derive(Debug, Clone)]
pub struct AccessibleElement {
    /// Element ID.
    pub id: u32,
    /// Role.
    pub role: ElementRole,
    /// Name / accessible label.
    pub name: String,
    /// Value (for inputs, sliders, etc.).
    pub value: String,
    /// Description / help text.
    pub description: String,
    /// Whether the element is focused.
    pub focused: bool,
    /// Whether the element is enabled.
    pub enabled: bool,
    /// Whether the element is checked (checkboxes/radios).
    pub checked: bool,
    /// Keyboard shortcut.
    pub shortcut: String,
    /// Parent element ID (0 = root).
    pub parent_id: u32,
}

/// Screen reader configuration.
#[derive(Debug, Clone)]
pub struct ScreenReaderConfig {
    pub enabled: bool,
    pub speech_rate: SpeechRate,
    pub volume: u32,
    pub verbosity: Verbosity,
    pub speak_typed_chars: bool,
    pub speak_typed_words: bool,
    pub speak_punctuation: bool,
    pub announce_notifications: bool,
    pub cursor_follows_focus: bool,
    pub highlight_focused: bool,
    pub voice_name: String,
}

impl Default for ScreenReaderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            speech_rate: SpeechRate::Normal,
            volume: 80,
            verbosity: Verbosity::Medium,
            speak_typed_chars: false,
            speak_typed_words: true,
            speak_punctuation: false,
            announce_notifications: true,
            cursor_follows_focus: true,
            highlight_focused: true,
            voice_name: String::from("default"),
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: ScreenReaderConfig,
    elements: Vec<AccessibleElement>,
    focused_id: u32,
    speech_queue: Vec<String>,
    total_announcements: u64,
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
        config: ScreenReaderConfig::default(),
        elements: Vec::new(),
        focused_id: 0,
        speech_queue: Vec::new(),
        total_announcements: 0,
        ops: 0,
    });
}

pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.enabled = enabled;
        if enabled {
            state.speech_queue.push(String::from("Screen reader enabled"));
        }
        Ok(())
    })
}

pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.config.enabled)
}

pub fn set_speech_rate(rate: SpeechRate) -> KernelResult<()> {
    with_state(|state| { state.config.speech_rate = rate; Ok(()) })
}

pub fn set_volume(volume: u32) -> KernelResult<()> {
    with_state(|state| { state.config.volume = volume.min(100); Ok(()) })
}

pub fn set_verbosity(verbosity: Verbosity) -> KernelResult<()> {
    with_state(|state| { state.config.verbosity = verbosity; Ok(()) })
}

pub fn get_config() -> KernelResult<ScreenReaderConfig> {
    with_state(|state| Ok(state.config.clone()))
}

/// Register an accessible element (from toolkit).
pub fn register_element(
    id: u32, role: ElementRole, name: &str, parent_id: u32,
) -> KernelResult<()> {
    with_state(|state| {
        if state.elements.iter().any(|e| e.id == id) {
            return Err(KernelError::AlreadyExists);
        }
        state.elements.push(AccessibleElement {
            id, role, name: String::from(name),
            value: String::new(), description: String::new(),
            focused: false, enabled: true, checked: false,
            shortcut: String::new(), parent_id,
        });
        Ok(())
    })
}

/// Remove an element.
pub fn unregister_element(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.elements.iter().position(|e| e.id == id)
            .ok_or(KernelError::NotFound)?;
        state.elements.remove(pos);
        Ok(())
    })
}

/// Announce text (push to TTS queue).
pub fn announce(text: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.config.enabled { return Ok(()); }
        state.speech_queue.push(String::from(text));
        state.total_announcements += 1;
        // Cap queue size.
        while state.speech_queue.len() > 50 {
            state.speech_queue.remove(0);
        }
        Ok(())
    })
}

/// Focus changed — announce the new focused element.
pub fn focus_changed(element_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.config.enabled { return Ok(()); }

        // Unfocus old.
        if state.focused_id != 0 {
            if let Some(old) = state.elements.iter_mut().find(|e| e.id == state.focused_id) {
                old.focused = false;
            }
        }

        // Focus new.
        state.focused_id = element_id;
        if let Some(elem) = state.elements.iter_mut().find(|e| e.id == element_id) {
            elem.focused = true;

            // Build announcement based on verbosity.
            let announcement = match state.config.verbosity {
                Verbosity::Low => format!("{} {}", elem.name, elem.role.label()),
                Verbosity::Medium => {
                    let state_str = if !elem.enabled { " disabled" }
                        else if elem.checked { " checked" }
                        else { "" };
                    format!("{} {}{}", elem.name, elem.role.label(), state_str)
                }
                Verbosity::High => {
                    let state_str = if !elem.enabled { " disabled" }
                        else if elem.checked { " checked" }
                        else { "" };
                    let desc = if elem.description.is_empty() { String::new() }
                        else { format!(" — {}", elem.description) };
                    let shortcut = if elem.shortcut.is_empty() { String::new() }
                        else { format!(" ({})", elem.shortcut) };
                    format!("{} {}{}{}{}", elem.name, elem.role.label(), state_str, desc, shortcut)
                }
            };

            state.speech_queue.push(announcement);
            state.total_announcements += 1;
        }

        Ok(())
    })
}

/// Get the focused element.
pub fn focused_element() -> Option<AccessibleElement> {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| {
        s.elements.iter().find(|e| e.id == s.focused_id).cloned()
    })
}

/// Pop the next speech item from the queue (for TTS engine).
pub fn next_speech() -> Option<String> {
    let mut guard = STATE.lock();
    guard.as_mut().and_then(|s| {
        if s.speech_queue.is_empty() { None }
        else { Some(s.speech_queue.remove(0)) }
    })
}

/// Get speech queue length.
pub fn speech_queue_len() -> usize {
    STATE.lock().as_ref().map_or(0, |s| s.speech_queue.len())
}

/// List registered elements.
pub fn list_elements() -> Vec<AccessibleElement> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.elements.clone())
}

/// Statistics: (element_count, announcements, queue_len, enabled, speech_rate_label, ops).
pub fn stats() -> (usize, u64, usize, bool, &'static str, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.elements.len(), s.total_announcements, s.speech_queue.len(),
            s.config.enabled, s.config.speech_rate.label(), s.ops,
        ),
        None => (0, 0, 0, false, "N/A", 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("screenreader::self_test() — running tests...");
    init_defaults();

    // 1: Disabled by default.
    assert!(!is_enabled());
    crate::serial_println!("  [1/11] disabled by default: OK");

    // 2: Enable.
    set_enabled(true).expect("enable");
    assert!(is_enabled());
    crate::serial_println!("  [2/11] enable: OK");

    // 3: Register elements.
    register_element(1, ElementRole::Window, "Main Window", 0).expect("reg 1");
    register_element(2, ElementRole::Button, "OK", 1).expect("reg 2");
    register_element(3, ElementRole::TextInput, "Name", 1).expect("reg 3");
    crate::serial_println!("  [3/11] register elements: OK");

    // 4: Focus change announces.
    focus_changed(2).expect("focus");
    let speech = next_speech().expect("speech available");
    assert!(speech.contains("OK"));
    assert!(speech.contains("button"));
    crate::serial_println!("  [4/11] focus announcement: OK");

    // 5: Announce custom text.
    announce("File saved successfully").expect("announce");
    let speech = next_speech().expect("custom speech");
    assert!(speech.contains("saved"));
    crate::serial_println!("  [5/11] custom announce: OK");

    // 6: High verbosity.
    set_verbosity(Verbosity::High).expect("set verbosity");
    focus_changed(3).expect("focus 3");
    let speech = next_speech().expect("high verbosity speech");
    assert!(speech.contains("Name"));
    assert!(speech.contains("text input"));
    crate::serial_println!("  [6/11] high verbosity: OK");

    // 7: Speech rate.
    set_speech_rate(SpeechRate::Fast).expect("set rate");
    let cfg = get_config().expect("get config");
    assert_eq!(cfg.speech_rate, SpeechRate::Fast);
    crate::serial_println!("  [7/11] speech rate: OK");

    // 8: Volume.
    set_volume(60).expect("set volume");
    let cfg = get_config().expect("get config 2");
    assert_eq!(cfg.volume, 60);
    crate::serial_println!("  [8/11] volume: OK");

    // 9: Unregister element.
    unregister_element(2).expect("unreg");
    let elements = list_elements();
    assert_eq!(elements.len(), 2);
    crate::serial_println!("  [9/11] unregister: OK");

    // 10: Focused element.
    let focused = focused_element();
    assert!(focused.is_some());
    assert_eq!(focused.expect("focused").role, ElementRole::TextInput);
    crate::serial_println!("  [10/11] focused element: OK");

    // 11: Stats.
    let (elems, announcements, _queue, enabled, rate, ops) = stats();
    assert_eq!(elems, 2);
    assert!(announcements >= 3);
    assert!(enabled);
    assert_eq!(rate, "Fast");
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("screenreader::self_test() — all 11 tests passed");
}

//! Keyboard Macros — record and replay keyboard automation.
//!
//! Allows recording sequences of keyboard/mouse events and replaying
//! them as macros, with optional hotkey triggers and repeat counts.
//!
//! ## Architecture
//!
//! ```text
//! User records macro
//!   → kbmacro::start_recording() → recording events
//!   → kbmacro::stop_recording() → save macro
//!   → kbmacro::play(macro_id) → replay events
//!
//! Integration:
//!   → kbshortcuts (hotkey triggers)
//!   → hotkeys (macro activation)
//!   → kbsettings (keyboard config)
//!   → scriptlang (scripting alternative)
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

/// A recorded event.
#[derive(Debug, Clone)]
pub enum MacroEvent {
    /// Key press (keycode).
    KeyDown(u32),
    /// Key release (keycode).
    KeyUp(u32),
    /// Type text.
    TypeText(String),
    /// Delay in milliseconds.
    Delay(u32),
    /// Mouse move to (x, y).
    MouseMove(i32, i32),
    /// Mouse button press.
    MouseDown(u8),
    /// Mouse button release.
    MouseUp(u8),
}

impl MacroEvent {
    pub fn label(&self) -> &'static str {
        match self {
            Self::KeyDown(_) => "KeyDown",
            Self::KeyUp(_) => "KeyUp",
            Self::TypeText(_) => "TypeText",
            Self::Delay(_) => "Delay",
            Self::MouseMove(_, _) => "MouseMove",
            Self::MouseDown(_) => "MouseDown",
            Self::MouseUp(_) => "MouseUp",
        }
    }
}

/// A saved macro.
#[derive(Debug, Clone)]
pub struct Macro {
    pub id: u32,
    pub name: String,
    pub events: Vec<MacroEvent>,
    pub hotkey: Option<String>,
    pub repeat_count: u32,
    pub enabled: bool,
    pub play_count: u64,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_MACROS: usize = 100;
const MAX_EVENTS_PER_MACRO: usize = 1000;

struct State {
    macros: Vec<Macro>,
    next_id: u32,
    recording: bool,
    recording_events: Vec<MacroEvent>,
    recording_name: String,
    total_plays: u64,
    total_recorded: u64,
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
        macros: Vec::new(),
        next_id: 1,
        recording: false,
        recording_events: Vec::new(),
        recording_name: String::new(),
        total_plays: 0,
        total_recorded: 0,
        ops: 0,
    });
}

/// Start recording a macro.
pub fn start_recording(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.recording {
            return Err(KernelError::AlreadyExists);
        }
        state.recording = true;
        state.recording_events.clear();
        state.recording_name = String::from(name);
        Ok(())
    })
}

/// Record an event.
pub fn record_event(event: MacroEvent) -> KernelResult<()> {
    with_state(|state| {
        if !state.recording {
            return Err(KernelError::NotSupported);
        }
        if state.recording_events.len() >= MAX_EVENTS_PER_MACRO {
            return Err(KernelError::ResourceExhausted);
        }
        state.recording_events.push(event);
        Ok(())
    })
}

/// Stop recording and save.
pub fn stop_recording() -> KernelResult<u32> {
    with_state(|state| {
        if !state.recording {
            return Err(KernelError::NotSupported);
        }
        state.recording = false;
        if state.macros.len() >= MAX_MACROS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        let now = crate::hpet::elapsed_ns();
        state.macros.push(Macro {
            id,
            name: state.recording_name.clone(),
            events: state.recording_events.clone(),
            hotkey: None,
            repeat_count: 1,
            enabled: true,
            play_count: 0,
            created_ns: now,
        });
        state.recording_events.clear();
        state.recording_name.clear();
        state.total_recorded += 1;
        Ok(id)
    })
}

/// Cancel recording without saving.
pub fn cancel_recording() -> KernelResult<()> {
    with_state(|state| {
        if !state.recording {
            return Err(KernelError::NotSupported);
        }
        state.recording = false;
        state.recording_events.clear();
        state.recording_name.clear();
        Ok(())
    })
}

/// Create a macro from events directly (not recording).
pub fn create_macro(name: &str, events: Vec<MacroEvent>) -> KernelResult<u32> {
    with_state(|state| {
        if state.macros.len() >= MAX_MACROS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        let now = crate::hpet::elapsed_ns();
        state.macros.push(Macro {
            id,
            name: String::from(name),
            events,
            hotkey: None,
            repeat_count: 1,
            enabled: true,
            play_count: 0,
            created_ns: now,
        });
        state.total_recorded += 1;
        Ok(id)
    })
}

/// Play a macro (returns event count).
pub fn play(id: u32) -> KernelResult<usize> {
    with_state(|state| {
        let mac = state.macros.iter_mut().find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        if !mac.enabled {
            return Err(KernelError::NotSupported);
        }
        mac.play_count += 1;
        state.total_plays += 1;
        let count = mac.events.len() * mac.repeat_count as usize;
        Ok(count)
    })
}

/// Set hotkey for a macro.
pub fn set_hotkey(id: u32, hotkey: &str) -> KernelResult<()> {
    with_state(|state| {
        let mac = state.macros.iter_mut().find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        mac.hotkey = Some(String::from(hotkey));
        Ok(())
    })
}

/// Set repeat count.
pub fn set_repeat(id: u32, count: u32) -> KernelResult<()> {
    with_state(|state| {
        let mac = state.macros.iter_mut().find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        mac.repeat_count = count.max(1);
        Ok(())
    })
}

/// Enable/disable a macro.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let mac = state.macros.iter_mut().find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        mac.enabled = enabled;
        Ok(())
    })
}

/// Delete a macro.
pub fn delete_macro(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.macros.len();
        state.macros.retain(|m| m.id != id);
        if state.macros.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Is currently recording?
pub fn is_recording() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.recording)
}

/// Get a macro by id.
pub fn get_macro(id: u32) -> Option<Macro> {
    STATE.lock().as_ref().and_then(|s| s.macros.iter().find(|m| m.id == id).cloned())
}

/// List all macros.
pub fn list_macros() -> Vec<Macro> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.macros.clone())
}

/// Statistics: (macro_count, total_plays, total_recorded, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.macros.len(), s.total_plays, s.total_recorded, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("kbmacro::self_test() — running tests...");
    init_defaults();

    // 1: Empty.
    assert_eq!(list_macros().len(), 0);
    assert!(!is_recording());
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Record macro.
    start_recording("test_macro").expect("start");
    assert!(is_recording());
    record_event(MacroEvent::TypeText(String::from("hello"))).expect("ev1");
    record_event(MacroEvent::Delay(100)).expect("ev2");
    record_event(MacroEvent::KeyDown(13)).expect("ev3"); // Enter.
    let id = stop_recording().expect("stop");
    assert!(!is_recording());
    crate::serial_println!("  [2/8] record: OK");

    // 3: Verify macro.
    let mac = get_macro(id).expect("get");
    assert_eq!(mac.name, "test_macro");
    assert_eq!(mac.events.len(), 3);
    crate::serial_println!("  [3/8] verify: OK");

    // 4: Play macro.
    let count = play(id).expect("play");
    assert_eq!(count, 3);
    crate::serial_println!("  [4/8] play: OK");

    // 5: Set hotkey and repeat.
    set_hotkey(id, "Ctrl+Shift+M").expect("hotkey");
    set_repeat(id, 3).expect("repeat");
    let count = play(id).expect("play2");
    assert_eq!(count, 9); // 3 events * 3 repeats.
    crate::serial_println!("  [5/8] hotkey/repeat: OK");

    // 6: Create directly.
    let id2 = create_macro("quick", alloc::vec![
        MacroEvent::TypeText(String::from("world")),
    ]).expect("create");
    assert_eq!(list_macros().len(), 2);
    crate::serial_println!("  [6/8] create: OK");

    // 7: Disable and delete.
    set_enabled(id2, false).expect("disable");
    assert!(play(id2).is_err()); // Disabled macro can't play.
    delete_macro(id2).expect("delete");
    assert_eq!(list_macros().len(), 1);
    crate::serial_println!("  [7/8] disable/delete: OK");

    // 8: Stats.
    let (macros, plays, recorded, ops) = stats();
    assert_eq!(macros, 1);
    assert_eq!(plays, 2);
    assert_eq!(recorded, 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("kbmacro::self_test() — all 8 tests passed");
}

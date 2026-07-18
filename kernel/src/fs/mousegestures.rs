//! Mouse Gestures — gesture-based navigation and commands.
//!
//! Recognizes mouse button gestures (hold right-click and move)
//! for browser-style navigation, custom actions, and drawing gestures.
//!
//! ## Architecture
//!
//! ```text
//! Right-click + drag
//!   → mousegestures::track(dx, dy) → builds stroke
//!   → mousegestures::recognize(stroke) → matched gesture
//!   → mousegestures::execute(gesture) → trigger action
//!
//! Integration:
//!   → mousesettings (mouse configuration)
//!   → gestures (touchpad gestures)
//!   → kbshortcuts (action dispatch)
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

/// Gesture direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }

    pub fn symbol(self) -> char {
        match self {
            Self::Up => '↑',
            Self::Down => '↓',
            Self::Left => '←',
            Self::Right => '→',
        }
    }
}

/// A gesture pattern (sequence of directions).
#[derive(Debug, Clone)]
pub struct GesturePattern {
    pub directions: Vec<Direction>,
}

impl GesturePattern {
    pub fn label(&self) -> String {
        let symbols: Vec<char> = self.directions.iter().map(|d| d.symbol()).collect();
        symbols.iter().collect()
    }
}

/// A gesture binding.
#[derive(Debug, Clone)]
pub struct GestureBinding {
    pub id: u32,
    pub pattern: GesturePattern,
    pub action: String,
    pub description: String,
    pub enabled: bool,
    pub use_count: u64,
}

/// Mouse button for gesture trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureButton {
    RightClick,
    MiddleClick,
    SideButton,
}

impl GestureButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::RightClick => "Right Click",
            Self::MiddleClick => "Middle Click",
            Self::SideButton => "Side Button",
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_BINDINGS: usize = 100;
/// Minimum movement in pixels to register a direction.
const MIN_STROKE_PX: i32 = 30;

struct State {
    bindings: Vec<GestureBinding>,
    next_id: u32,
    trigger_button: GestureButton,
    enabled: bool,
    /// Sensitivity: minimum pixels for direction detection.
    sensitivity_px: i32,
    total_gestures: u64,
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

    let bindings = alloc::vec![
        GestureBinding { id: 1,
            pattern: GesturePattern { directions: alloc::vec![Direction::Left] },
            action: String::from("navigate_back"), description: String::from("Go Back"),
            enabled: true, use_count: 0 },
        GestureBinding { id: 2,
            pattern: GesturePattern { directions: alloc::vec![Direction::Right] },
            action: String::from("navigate_forward"), description: String::from("Go Forward"),
            enabled: true, use_count: 0 },
        GestureBinding { id: 3,
            pattern: GesturePattern { directions: alloc::vec![Direction::Down, Direction::Right] },
            action: String::from("close_tab"), description: String::from("Close Tab"),
            enabled: true, use_count: 0 },
        GestureBinding { id: 4,
            pattern: GesturePattern { directions: alloc::vec![Direction::Up] },
            action: String::from("scroll_top"), description: String::from("Scroll to Top"),
            enabled: true, use_count: 0 },
        GestureBinding { id: 5,
            pattern: GesturePattern { directions: alloc::vec![Direction::Down] },
            action: String::from("scroll_bottom"), description: String::from("Scroll to Bottom"),
            enabled: true, use_count: 0 },
        GestureBinding { id: 6,
            pattern: GesturePattern { directions: alloc::vec![Direction::Up, Direction::Down] },
            action: String::from("refresh"), description: String::from("Refresh Page"),
            enabled: true, use_count: 0 },
    ];

    *guard = Some(State {
        bindings,
        next_id: 7,
        trigger_button: GestureButton::RightClick,
        enabled: true,
        sensitivity_px: MIN_STROKE_PX,
        total_gestures: 0,
        total_recognized: 0,
        ops: 0,
    });
}

/// Recognize a gesture from a sequence of directions.
pub fn recognize(directions: &[Direction]) -> KernelResult<Option<String>> {
    with_state(|state| {
        state.total_gestures += 1;
        for binding in &mut state.bindings {
            if !binding.enabled { continue; }
            if binding.pattern.directions == directions {
                binding.use_count += 1;
                state.total_recognized += 1;
                return Ok(Some(binding.action.clone()));
            }
        }
        Ok(None)
    })
}

/// Bind a gesture pattern to an action.
pub fn bind(directions: Vec<Direction>, action: &str, description: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.bindings.len() >= MAX_BINDINGS {
            return Err(KernelError::ResourceExhausted);
        }
        // Check for conflict.
        if state.bindings.iter().any(|b| b.pattern.directions == directions && b.enabled) {
            return Err(KernelError::AlreadyExists);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.bindings.push(GestureBinding {
            id, pattern: GesturePattern { directions },
            action: String::from(action),
            description: String::from(description),
            enabled: true, use_count: 0,
        });
        Ok(id)
    })
}

/// Unbind a gesture.
pub fn unbind(binding_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.bindings.iter().position(|b| b.id == binding_id)
            .ok_or(KernelError::NotFound)?;
        state.bindings.remove(pos);
        Ok(())
    })
}

/// Set trigger button.
pub fn set_trigger_button(button: GestureButton) -> KernelResult<()> {
    with_state(|state| {
        state.trigger_button = button;
        Ok(())
    })
}

/// Enable/disable gestures.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        Ok(())
    })
}

/// Set sensitivity (minimum pixel movement).
pub fn set_sensitivity(px: i32) -> KernelResult<()> {
    with_state(|state| {
        state.sensitivity_px = px.clamp(10, 200);
        Ok(())
    })
}

/// List all bindings.
pub fn list_bindings() -> Vec<GestureBinding> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.bindings.clone())
}

/// Get binding.
pub fn get_binding(id: u32) -> KernelResult<GestureBinding> {
    with_state(|state| {
        state.bindings.iter().find(|b| b.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Statistics: (binding_count, total_gestures, total_recognized, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.bindings.len(), s.total_gestures, s.total_recognized, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("mousegestures::self_test() — running tests...");
    init_defaults();

    // 1: Default 6 gestures.
    let bindings = list_bindings();
    assert_eq!(bindings.len(), 6);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Recognize left swipe.
    let action = recognize(&[Direction::Left]).expect("rec");
    assert_eq!(action, Some(String::from("navigate_back")));
    crate::serial_println!("  [2/8] recognize left: OK");

    // 3: Recognize compound gesture.
    let action = recognize(&[Direction::Down, Direction::Right]).expect("rec2");
    assert_eq!(action, Some(String::from("close_tab")));
    crate::serial_println!("  [3/8] recognize compound: OK");

    // 4: Unrecognized gesture.
    let action = recognize(&[Direction::Left, Direction::Left, Direction::Left]).expect("rec3");
    assert!(action.is_none());
    crate::serial_println!("  [4/8] unrecognized: OK");

    // 5: Bind new gesture.
    let id = bind(
        alloc::vec![Direction::Left, Direction::Up],
        "new_tab", "New Tab"
    ).expect("bind");
    let action = recognize(&[Direction::Left, Direction::Up]).expect("rec4");
    assert_eq!(action, Some(String::from("new_tab")));
    crate::serial_println!("  [5/8] bind: OK");

    // 6: Conflict rejected.
    let result = bind(
        alloc::vec![Direction::Left, Direction::Up],
        "other", "Other"
    );
    assert!(result.is_err());
    crate::serial_println!("  [6/8] conflict: OK");

    // 7: Unbind.
    unbind(id).expect("unbind");
    assert_eq!(list_bindings().len(), 6);
    crate::serial_println!("  [7/8] unbind: OK");

    // 8: Stats.
    let (count, gestures, recognized, ops) = stats();
    assert_eq!(count, 6);
    assert_eq!(gestures, 4);
    assert_eq!(recognized, 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("mousegestures::self_test() — all 8 tests passed");
}

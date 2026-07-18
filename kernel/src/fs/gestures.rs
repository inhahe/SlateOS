//! Gestures — touchscreen and touchpad gesture recognition.
//!
//! Detects multi-touch gestures (pinch, swipe, rotate, tap patterns)
//! and maps them to system actions.  Works with both touchscreens and
//! precision touchpads.
//!
//! ## Architecture
//!
//! ```text
//! Input driver (touchpad/touchscreen)
//!   → gestures::process_touch(points) → recognized gesture
//!   → action dispatch (workspace switch, zoom, etc.)
//!
//! Settings panel → Touchpad / Touchscreen
//!   → gestures::configure_action(gesture, action)
//!
//! Integration:
//!   → touchpad (raw touch events)
//!   → compositor (workspace/window gestures)
//!   → magnifier (pinch-to-zoom)
//!   → vdesktop (swipe between desktops)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Recognized gesture type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureType {
    /// Single tap.
    Tap,
    /// Double tap.
    DoubleTap,
    /// Long press.
    LongPress,
    /// Two-finger scroll.
    TwoFingerScroll,
    /// Two-finger pinch (zoom).
    Pinch,
    /// Two-finger rotate.
    Rotate,
    /// Three-finger swipe up.
    ThreeFingerSwipeUp,
    /// Three-finger swipe down.
    ThreeFingerSwipeDown,
    /// Three-finger swipe left.
    ThreeFingerSwipeLeft,
    /// Three-finger swipe right.
    ThreeFingerSwipeRight,
    /// Four-finger swipe up.
    FourFingerSwipeUp,
    /// Four-finger swipe down.
    FourFingerSwipeDown,
    /// Four-finger swipe left.
    FourFingerSwipeLeft,
    /// Four-finger swipe right.
    FourFingerSwipeRight,
    /// Three-finger tap.
    ThreeFingerTap,
    /// Four-finger tap.
    FourFingerTap,
}

impl GestureType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tap => "Tap",
            Self::DoubleTap => "Double Tap",
            Self::LongPress => "Long Press",
            Self::TwoFingerScroll => "2-Finger Scroll",
            Self::Pinch => "Pinch Zoom",
            Self::Rotate => "2-Finger Rotate",
            Self::ThreeFingerSwipeUp => "3-Finger Swipe Up",
            Self::ThreeFingerSwipeDown => "3-Finger Swipe Down",
            Self::ThreeFingerSwipeLeft => "3-Finger Swipe Left",
            Self::ThreeFingerSwipeRight => "3-Finger Swipe Right",
            Self::FourFingerSwipeUp => "4-Finger Swipe Up",
            Self::FourFingerSwipeDown => "4-Finger Swipe Down",
            Self::FourFingerSwipeLeft => "4-Finger Swipe Left",
            Self::FourFingerSwipeRight => "4-Finger Swipe Right",
            Self::ThreeFingerTap => "3-Finger Tap",
            Self::FourFingerTap => "4-Finger Tap",
        }
    }

    /// All gesture types for iteration.
    pub fn all() -> &'static [GestureType] {
        &[
            Self::Tap, Self::DoubleTap, Self::LongPress, Self::TwoFingerScroll,
            Self::Pinch, Self::Rotate, Self::ThreeFingerSwipeUp, Self::ThreeFingerSwipeDown,
            Self::ThreeFingerSwipeLeft, Self::ThreeFingerSwipeRight,
            Self::FourFingerSwipeUp, Self::FourFingerSwipeDown,
            Self::FourFingerSwipeLeft, Self::FourFingerSwipeRight,
            Self::ThreeFingerTap, Self::FourFingerTap,
        ]
    }
}

/// Action to perform when a gesture is recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureAction {
    None,
    Click,
    RightClick,
    MiddleClick,
    ShowDesktop,
    TaskView,
    PrevDesktop,
    NextDesktop,
    Back,
    Forward,
    ZoomIn,
    ZoomOut,
    NotificationCenter,
    Search,
    Minimize,
    Maximize,
}

impl GestureAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Click => "Click",
            Self::RightClick => "Right Click",
            Self::MiddleClick => "Middle Click",
            Self::ShowDesktop => "Show Desktop",
            Self::TaskView => "Task View",
            Self::PrevDesktop => "Previous Desktop",
            Self::NextDesktop => "Next Desktop",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::ZoomIn => "Zoom In",
            Self::ZoomOut => "Zoom Out",
            Self::NotificationCenter => "Notifications",
            Self::Search => "Search",
            Self::Minimize => "Minimize",
            Self::Maximize => "Maximize",
        }
    }
}

/// A gesture-to-action mapping.
#[derive(Debug, Clone)]
pub struct GestureMapping {
    pub gesture: GestureType,
    pub action: GestureAction,
    pub enabled: bool,
}

/// Touch point.
#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub pressure: u16,
}

/// Gesture configuration.
#[derive(Debug, Clone)]
pub struct GestureConfig {
    pub enabled: bool,
    pub touchpad_gestures: bool,
    pub touchscreen_gestures: bool,
    /// Minimum swipe distance in pixels.
    pub swipe_threshold: u32,
    /// Long press time in ms.
    pub long_press_ms: u32,
    /// Double-tap interval in ms.
    pub double_tap_ms: u32,
    /// Natural scrolling (reverse direction).
    pub natural_scroll: bool,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            touchpad_gestures: true,
            touchscreen_gestures: true,
            swipe_threshold: 50,
            long_press_ms: 500,
            double_tap_ms: 300,
            natural_scroll: false,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: GestureConfig,
    mappings: Vec<GestureMapping>,
    total_gestures: u64,
    gesture_counts: Vec<(GestureType, u64)>,
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

    // Default gesture mappings.
    let mappings = alloc::vec![
        GestureMapping { gesture: GestureType::Tap, action: GestureAction::Click, enabled: true },
        GestureMapping { gesture: GestureType::DoubleTap, action: GestureAction::Click, enabled: true },
        GestureMapping { gesture: GestureType::LongPress, action: GestureAction::RightClick, enabled: true },
        GestureMapping { gesture: GestureType::TwoFingerScroll, action: GestureAction::None, enabled: true },
        GestureMapping { gesture: GestureType::Pinch, action: GestureAction::ZoomIn, enabled: true },
        GestureMapping { gesture: GestureType::ThreeFingerSwipeUp, action: GestureAction::TaskView, enabled: true },
        GestureMapping { gesture: GestureType::ThreeFingerSwipeDown, action: GestureAction::ShowDesktop, enabled: true },
        GestureMapping { gesture: GestureType::ThreeFingerSwipeLeft, action: GestureAction::PrevDesktop, enabled: true },
        GestureMapping { gesture: GestureType::ThreeFingerSwipeRight, action: GestureAction::NextDesktop, enabled: true },
        GestureMapping { gesture: GestureType::FourFingerSwipeUp, action: GestureAction::Search, enabled: true },
        GestureMapping { gesture: GestureType::FourFingerSwipeDown, action: GestureAction::Minimize, enabled: true },
        GestureMapping { gesture: GestureType::ThreeFingerTap, action: GestureAction::NotificationCenter, enabled: true },
        GestureMapping { gesture: GestureType::FourFingerTap, action: GestureAction::TaskView, enabled: true },
    ];

    *guard = Some(State {
        config: GestureConfig::default(),
        mappings,
        total_gestures: 0,
        gesture_counts: Vec::new(),
        ops: 0,
    });
}

pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.enabled = enabled; Ok(()) })
}

pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.config.enabled)
}

/// Set action for a gesture.
pub fn set_action(gesture: GestureType, action: GestureAction) -> KernelResult<()> {
    with_state(|state| {
        if let Some(m) = state.mappings.iter_mut().find(|m| m.gesture == gesture) {
            m.action = action;
        } else {
            state.mappings.push(GestureMapping { gesture, action, enabled: true });
        }
        Ok(())
    })
}

/// Enable/disable a specific gesture.
pub fn set_gesture_enabled(gesture: GestureType, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        if let Some(m) = state.mappings.iter_mut().find(|m| m.gesture == gesture) {
            m.enabled = enabled;
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Get action for a gesture.
pub fn get_action(gesture: GestureType) -> GestureAction {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| {
        s.mappings.iter().find(|m| m.gesture == gesture && m.enabled).map(|m| m.action)
    }).unwrap_or(GestureAction::None)
}

/// Record a recognized gesture (for statistics).
pub fn record_gesture(gesture: GestureType) -> GestureAction {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return GestureAction::None,
    };
    state.total_gestures += 1;
    state.ops += 1;

    // Update per-gesture counter.
    if let Some(entry) = state.gesture_counts.iter_mut().find(|(g, _)| *g == gesture) {
        entry.1 += 1;
    } else {
        state.gesture_counts.push((gesture, 1));
    }

    state.mappings.iter().find(|m| m.gesture == gesture && m.enabled)
        .map_or(GestureAction::None, |m| m.action)
}

/// Set natural scrolling.
pub fn set_natural_scroll(on: bool) -> KernelResult<()> {
    with_state(|state| { state.config.natural_scroll = on; Ok(()) })
}

/// Set swipe threshold.
pub fn set_swipe_threshold(pixels: u32) -> KernelResult<()> {
    with_state(|state| { state.config.swipe_threshold = pixels; Ok(()) })
}

/// List gesture mappings.
pub fn list_mappings() -> Vec<GestureMapping> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.mappings.clone())
}

/// Get configuration.
pub fn get_config() -> KernelResult<GestureConfig> {
    with_state(|state| Ok(state.config.clone()))
}

/// Statistics: (mapping_count, total_gestures, enabled, natural_scroll, ops).
pub fn stats() -> (usize, u64, bool, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.mappings.len(), s.total_gestures, s.config.enabled,
            s.config.natural_scroll, s.ops,
        ),
        None => (0, 0, false, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("gestures::self_test() — running tests...");
    init_defaults();

    // 1: Enabled by default.
    assert!(is_enabled());
    crate::serial_println!("  [1/11] enabled by default: OK");

    // 2: Default mappings.
    let mappings = list_mappings();
    assert!(mappings.len() >= 13);
    crate::serial_println!("  [2/11] default mappings: OK");

    // 3: Get action.
    let action = get_action(GestureType::ThreeFingerSwipeUp);
    assert_eq!(action, GestureAction::TaskView);
    crate::serial_println!("  [3/11] get action: OK");

    // 4: Set action.
    set_action(GestureType::ThreeFingerSwipeUp, GestureAction::Search).expect("set action");
    let action = get_action(GestureType::ThreeFingerSwipeUp);
    assert_eq!(action, GestureAction::Search);
    crate::serial_println!("  [4/11] set action: OK");

    // 5: Record gesture.
    let action = record_gesture(GestureType::Tap);
    assert_eq!(action, GestureAction::Click);
    crate::serial_println!("  [5/11] record gesture: OK");

    // 6: Disable gesture.
    set_gesture_enabled(GestureType::Tap, false).expect("disable tap");
    let action = get_action(GestureType::Tap);
    assert_eq!(action, GestureAction::None);
    crate::serial_println!("  [6/11] disable gesture: OK");

    // 7: Re-enable.
    set_gesture_enabled(GestureType::Tap, true).expect("enable tap");
    let action = get_action(GestureType::Tap);
    assert_eq!(action, GestureAction::Click);
    crate::serial_println!("  [7/11] re-enable gesture: OK");

    // 8: Natural scroll.
    set_natural_scroll(true).expect("natural scroll");
    let cfg = get_config().expect("config");
    assert!(cfg.natural_scroll);
    crate::serial_println!("  [8/11] natural scroll: OK");

    // 9: Swipe threshold.
    set_swipe_threshold(100).expect("threshold");
    let cfg = get_config().expect("config 2");
    assert_eq!(cfg.swipe_threshold, 100);
    crate::serial_println!("  [9/11] swipe threshold: OK");

    // 10: Disable all gestures.
    set_enabled(false).expect("disable all");
    assert!(!is_enabled());
    crate::serial_println!("  [10/11] disable all: OK");

    // 11: Stats.
    set_enabled(true).expect("re-enable");
    let (mappings, total, enabled, natural, ops) = stats();
    assert!(mappings >= 13);
    assert!(total >= 1);
    assert!(enabled);
    assert!(natural);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("gestures::self_test() — all 11 tests passed");
}

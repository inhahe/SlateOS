//! Input accessibility — sticky keys, filter keys, toggle keys, mouse keys.
//!
//! Provides keyboard accessibility features for users with motor
//! disabilities: sticky keys (sequential modifier input), filter keys
//! (ignore brief/repeat keypresses), toggle keys (audio on Caps/Num Lock),
//! and mouse keys (cursor movement via numpad).
//!
//! ## Architecture
//!
//! ```text
//! Keyboard driver / input subsystem
//!   → inputa11y::process_key(key) → modified key event
//!
//! Settings panel → Accessibility → Keyboard
//!   → inputa11y::set_sticky_keys(true)
//!
//! Integration:
//!   → keylayout (keyboard scan codes)
//!   → kbsettings (keyboard repeat rate)
//!   → a11y (accessibility framework)
//!   → soundevents (toggle key sounds)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Modifier key state for sticky keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickyState {
    /// Modifier not active.
    Off,
    /// Modifier latched (will apply to next key).
    Latched,
    /// Modifier locked (persists until pressed again).
    Locked,
}

impl StickyState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Latched => "latched",
            Self::Locked => "locked",
        }
    }
}

/// Modifier keys tracked by sticky keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
    Super,
}

impl Modifier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Shift => "Shift",
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
            Self::Super => "Super",
        }
    }

    pub fn all() -> &'static [Modifier] {
        &[Self::Shift, Self::Ctrl, Self::Alt, Self::Super]
    }
}

/// Input accessibility configuration.
#[derive(Debug, Clone)]
pub struct InputA11yConfig {
    /// Sticky keys: press modifiers sequentially instead of simultaneously.
    pub sticky_keys: bool,
    /// Sticky keys: turn off when two modifiers pressed simultaneously.
    pub sticky_two_keys_off: bool,
    /// Sticky keys: play sound on modifier.
    pub sticky_sound: bool,

    /// Filter keys: ignore brief keypresses.
    pub filter_keys: bool,
    /// Filter keys: acceptance delay in ms (key must be held this long).
    pub filter_delay_ms: u32,
    /// Filter keys: repeat delay in ms.
    pub filter_repeat_delay_ms: u32,
    /// Filter keys: repeat rate in ms.
    pub filter_repeat_rate_ms: u32,

    /// Toggle keys: play sound on Caps/Num/Scroll Lock.
    pub toggle_keys: bool,

    /// Mouse keys: move cursor with numpad.
    pub mouse_keys: bool,
    /// Mouse keys: speed (pixels per step).
    pub mouse_speed: u32,
    /// Mouse keys: acceleration factor.
    pub mouse_accel: u32,

    /// Bounce keys: ignore same key within interval.
    pub bounce_keys: bool,
    /// Bounce interval in ms.
    pub bounce_interval_ms: u32,
}

impl Default for InputA11yConfig {
    fn default() -> Self {
        Self {
            sticky_keys: false,
            sticky_two_keys_off: true,
            sticky_sound: true,
            filter_keys: false,
            filter_delay_ms: 500,
            filter_repeat_delay_ms: 500,
            filter_repeat_rate_ms: 200,
            toggle_keys: false,
            mouse_keys: false,
            mouse_speed: 10,
            mouse_accel: 2,
            bounce_keys: false,
            bounce_interval_ms: 300,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: InputA11yConfig,
    /// Sticky key states per modifier.
    sticky_states: [(Modifier, StickyState); 4],
    /// Total keys processed.
    total_keys: u64,
    /// Keys filtered/ignored.
    total_filtered: u64,
    /// Total mouse key movements.
    total_mouse_moves: u64,
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
        config: InputA11yConfig::default(),
        sticky_states: [
            (Modifier::Shift, StickyState::Off),
            (Modifier::Ctrl, StickyState::Off),
            (Modifier::Alt, StickyState::Off),
            (Modifier::Super, StickyState::Off),
        ],
        total_keys: 0,
        total_filtered: 0,
        total_mouse_moves: 0,
        ops: 0,
    });
}

/// Set sticky keys enabled.
pub fn set_sticky_keys(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.sticky_keys = enabled;
        if !enabled {
            for s in state.sticky_states.iter_mut() {
                s.1 = StickyState::Off;
            }
        }
        Ok(())
    })
}

/// Set filter keys enabled.
pub fn set_filter_keys(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.filter_keys = enabled; Ok(()) })
}

/// Set toggle keys enabled.
pub fn set_toggle_keys(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.toggle_keys = enabled; Ok(()) })
}

/// Set mouse keys enabled.
pub fn set_mouse_keys(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.mouse_keys = enabled; Ok(()) })
}

/// Set bounce keys enabled.
pub fn set_bounce_keys(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.bounce_keys = enabled; Ok(()) })
}

/// Set filter key delay.
pub fn set_filter_delay(ms: u32) -> KernelResult<()> {
    with_state(|state| { state.config.filter_delay_ms = ms; Ok(()) })
}

/// Set mouse speed.
pub fn set_mouse_speed(speed: u32) -> KernelResult<()> {
    with_state(|state| { state.config.mouse_speed = speed.clamp(1, 100); Ok(()) })
}

/// Press a modifier (for sticky keys).
pub fn modifier_pressed(modifier: Modifier) -> KernelResult<StickyState> {
    with_state(|state| {
        if !state.config.sticky_keys { return Ok(StickyState::Off); }
        state.total_keys += 1;

        let entry = state.sticky_states.iter_mut().find(|(m, _)| *m == modifier);
        let entry = match entry {
            Some(e) => e,
            None => return Ok(StickyState::Off),
        };

        // Cycle: Off → Latched → Locked → Off.
        entry.1 = match entry.1 {
            StickyState::Off => StickyState::Latched,
            StickyState::Latched => StickyState::Locked,
            StickyState::Locked => StickyState::Off,
        };

        Ok(entry.1)
    })
}

/// A non-modifier key was pressed — clear latched modifiers.
pub fn key_pressed() -> KernelResult<()> {
    with_state(|state| {
        state.total_keys += 1;
        if state.config.sticky_keys {
            for s in state.sticky_states.iter_mut() {
                if s.1 == StickyState::Latched {
                    s.1 = StickyState::Off;
                }
            }
        }
        Ok(())
    })
}

/// Record a filtered (ignored) key.
pub fn record_filtered() -> KernelResult<()> {
    with_state(|state| { state.total_filtered += 1; Ok(()) })
}

/// Record a mouse key movement.
pub fn record_mouse_move() -> KernelResult<()> {
    with_state(|state| { state.total_mouse_moves += 1; Ok(()) })
}

/// Get sticky key states.
pub fn get_sticky_states() -> Vec<(Modifier, StickyState)> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.sticky_states.to_vec()
    })
}

/// Get config.
pub fn get_config() -> KernelResult<InputA11yConfig> {
    with_state(|state| Ok(state.config.clone()))
}

/// Statistics: (sticky_on, filter_on, toggle_on, mouse_on, total_keys, total_filtered, ops).
pub fn stats() -> (bool, bool, bool, bool, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.config.sticky_keys, s.config.filter_keys,
            s.config.toggle_keys, s.config.mouse_keys,
            s.total_keys, s.total_filtered, s.ops,
        ),
        None => (false, false, false, false, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("inputa11y::self_test() — running tests...");
    init_defaults();

    // 1: All disabled by default.
    let (sticky, filter, toggle, mouse, _, _, _) = stats();
    assert!(!sticky);
    assert!(!filter);
    assert!(!toggle);
    assert!(!mouse);
    crate::serial_println!("  [1/11] all disabled: OK");

    // 2: Enable sticky keys.
    set_sticky_keys(true).expect("sticky on");
    let (sticky, _, _, _, _, _, _) = stats();
    assert!(sticky);
    crate::serial_println!("  [2/11] sticky keys: OK");

    // 3: Modifier press — latch.
    let state = modifier_pressed(Modifier::Shift).expect("shift");
    assert_eq!(state, StickyState::Latched);
    crate::serial_println!("  [3/11] modifier latch: OK");

    // 4: Modifier press again — lock.
    let state = modifier_pressed(Modifier::Shift).expect("shift 2");
    assert_eq!(state, StickyState::Locked);
    crate::serial_println!("  [4/11] modifier lock: OK");

    // 5: Modifier press again — off.
    let state = modifier_pressed(Modifier::Shift).expect("shift 3");
    assert_eq!(state, StickyState::Off);
    crate::serial_println!("  [5/11] modifier off: OK");

    // 6: Key press clears latched.
    modifier_pressed(Modifier::Ctrl).expect("ctrl latch");
    key_pressed().expect("key");
    let states = get_sticky_states();
    let ctrl = states.iter().find(|(m, _)| *m == Modifier::Ctrl).expect("find ctrl");
    assert_eq!(ctrl.1, StickyState::Off);
    crate::serial_println!("  [6/11] latch cleared on key: OK");

    // 7: Enable filter keys.
    set_filter_keys(true).expect("filter on");
    set_filter_delay(300).expect("delay");
    let cfg = get_config().expect("config");
    assert!(cfg.filter_keys);
    assert_eq!(cfg.filter_delay_ms, 300);
    crate::serial_println!("  [7/11] filter keys: OK");

    // 8: Enable toggle keys.
    set_toggle_keys(true).expect("toggle on");
    let (_, _, toggle, _, _, _, _) = stats();
    assert!(toggle);
    crate::serial_println!("  [8/11] toggle keys: OK");

    // 9: Enable mouse keys.
    set_mouse_keys(true).expect("mouse on");
    set_mouse_speed(20).expect("speed");
    let cfg = get_config().expect("config 2");
    assert!(cfg.mouse_keys);
    assert_eq!(cfg.mouse_speed, 20);
    crate::serial_println!("  [9/11] mouse keys: OK");

    // 10: Filtered key count.
    record_filtered().expect("filter 1");
    record_filtered().expect("filter 2");
    let (_, _, _, _, _, filtered, _) = stats();
    assert_eq!(filtered, 2);
    crate::serial_println!("  [10/11] filtered count: OK");

    // 11: Stats.
    let (sticky, filter, toggle, mouse, keys, filtered, ops) = stats();
    assert!(sticky);
    assert!(filter);
    assert!(toggle);
    assert!(mouse);
    assert!(keys >= 4);
    assert_eq!(filtered, 2);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("inputa11y::self_test() — all 11 tests passed");
}

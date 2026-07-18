//! Haptic Feedback — trackpad and controller haptic/vibration settings.
//!
//! Configures haptic feedback intensity, patterns, and per-event assignments
//! for trackpads, game controllers, and touchscreens.
//!
//! ## Architecture
//!
//! ```text
//! Input event occurs
//!   → haptfeedback::fire(event) → haptic pattern
//!
//! Configuration
//!   → haptfeedback::set_intensity(device, level)
//!   → haptfeedback::set_event_pattern(event, pattern)
//!
//! Integration:
//!   → touchpad (trackpad gestures)
//!   → gamepadinput (controller rumble)
//!   → peninput (stylus feedback)
//!   → a11y (accessibility haptics)
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

/// Haptic device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Trackpad,
    GameController,
    Touchscreen,
    Stylus,
}

impl DeviceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trackpad => "Trackpad",
            Self::GameController => "Game Controller",
            Self::Touchscreen => "Touchscreen",
            Self::Stylus => "Stylus",
        }
    }
}

/// Haptic event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HapticEvent {
    Click,
    DoubleClick,
    LongPress,
    Swipe,
    ScrollTick,
    SelectionChange,
    DragStart,
    DragEnd,
    Error,
    Success,
    Warning,
    KeyPress,
}

impl HapticEvent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Click => "Click",
            Self::DoubleClick => "Double Click",
            Self::LongPress => "Long Press",
            Self::Swipe => "Swipe",
            Self::ScrollTick => "Scroll Tick",
            Self::SelectionChange => "Selection Change",
            Self::DragStart => "Drag Start",
            Self::DragEnd => "Drag End",
            Self::Error => "Error",
            Self::Success => "Success",
            Self::Warning => "Warning",
            Self::KeyPress => "Key Press",
        }
    }
}

/// Haptic feedback pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HapticPattern {
    /// No feedback.
    None,
    /// Single short tap.
    Tap,
    /// Double tap.
    DoubleTap,
    /// Long buzz.
    Buzz,
    /// Short sharp impact.
    Impact,
    /// Soft rumble.
    Rumble,
    /// Rising intensity.
    Rising,
    /// Falling intensity.
    Falling,
    /// Heartbeat (two quick taps).
    Heartbeat,
}

impl HapticPattern {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Tap => "Tap",
            Self::DoubleTap => "Double Tap",
            Self::Buzz => "Buzz",
            Self::Impact => "Impact",
            Self::Rumble => "Rumble",
            Self::Rising => "Rising",
            Self::Falling => "Falling",
            Self::Heartbeat => "Heartbeat",
        }
    }
}

/// A haptic device with its settings.
#[derive(Debug, Clone)]
pub struct HapticDevice {
    pub id: u32,
    pub name: String,
    pub device_type: DeviceType,
    /// Intensity 0-100.
    pub intensity: u32,
    pub enabled: bool,
    pub fire_count: u64,
}

/// Event-to-pattern mapping.
#[derive(Debug, Clone)]
pub struct EventMapping {
    pub event: HapticEvent,
    pub pattern: HapticPattern,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 20;

struct State {
    devices: Vec<HapticDevice>,
    mappings: Vec<EventMapping>,
    global_enabled: bool,
    next_id: u32,
    total_fires: u64,
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

fn default_mappings() -> Vec<EventMapping> {
    alloc::vec![
        EventMapping { event: HapticEvent::Click, pattern: HapticPattern::Tap, enabled: true },
        EventMapping { event: HapticEvent::DoubleClick, pattern: HapticPattern::DoubleTap, enabled: true },
        EventMapping { event: HapticEvent::LongPress, pattern: HapticPattern::Buzz, enabled: true },
        EventMapping { event: HapticEvent::Swipe, pattern: HapticPattern::Impact, enabled: true },
        EventMapping { event: HapticEvent::ScrollTick, pattern: HapticPattern::Tap, enabled: true },
        EventMapping { event: HapticEvent::SelectionChange, pattern: HapticPattern::Tap, enabled: true },
        EventMapping { event: HapticEvent::DragStart, pattern: HapticPattern::Impact, enabled: true },
        EventMapping { event: HapticEvent::DragEnd, pattern: HapticPattern::Tap, enabled: true },
        EventMapping { event: HapticEvent::Error, pattern: HapticPattern::Heartbeat, enabled: true },
        EventMapping { event: HapticEvent::Success, pattern: HapticPattern::Rising, enabled: true },
        EventMapping { event: HapticEvent::Warning, pattern: HapticPattern::Rumble, enabled: true },
        EventMapping { event: HapticEvent::KeyPress, pattern: HapticPattern::None, enabled: false },
    ]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        devices: Vec::new(),
        mappings: default_mappings(),
        global_enabled: true,
        next_id: 1,
        total_fires: 0,
        ops: 0,
    });
}

/// Register a haptic device.
pub fn add_device(name: &str, dtype: DeviceType) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.devices.push(HapticDevice {
            id, name: String::from(name), device_type: dtype,
            intensity: 50, enabled: true, fire_count: 0,
        });
        Ok(id)
    })
}

/// Remove a device.
pub fn remove_device(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.id != id);
        if state.devices.len() == before {
            return Err(KernelError::NotFound);
        }
        Ok(())
    })
}

/// Set intensity for a device (0-100).
pub fn set_intensity(device_id: u32, intensity: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.intensity = intensity.min(100);
        Ok(())
    })
}

/// Enable/disable a device.
pub fn set_device_enabled(device_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.enabled = enabled;
        Ok(())
    })
}

/// Set pattern for an event.
pub fn set_event_pattern(event: HapticEvent, pattern: HapticPattern) -> KernelResult<()> {
    with_state(|state| {
        if let Some(m) = state.mappings.iter_mut().find(|m| m.event == event) {
            m.pattern = pattern;
            m.enabled = pattern != HapticPattern::None;
        }
        Ok(())
    })
}

/// Fire haptic feedback for an event on all enabled devices.
/// Returns the pattern played.
pub fn fire(event: HapticEvent) -> KernelResult<HapticPattern> {
    with_state(|state| {
        if !state.global_enabled {
            return Ok(HapticPattern::None);
        }
        let mapping = state.mappings.iter().find(|m| m.event == event);
        let pattern = match mapping {
            Some(m) if m.enabled => m.pattern,
            _ => return Ok(HapticPattern::None),
        };
        for dev in state.devices.iter_mut().filter(|d| d.enabled) {
            dev.fire_count += 1;
        }
        state.total_fires += 1;
        Ok(pattern)
    })
}

/// Enable/disable globally.
pub fn set_global_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        Ok(())
    })
}

/// List devices.
pub fn list_devices() -> Vec<HapticDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// List event mappings.
pub fn list_mappings() -> Vec<EventMapping> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.mappings.clone())
}

/// Statistics: (device_count, mapping_count, total_fires, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.mappings.len(), s.total_fires, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("haptfeedback::self_test() — running tests...");
    init_defaults();

    // 1: Default mappings.
    let mappings = list_mappings();
    assert_eq!(mappings.len(), 12);
    assert_eq!(mappings[0].event, HapticEvent::Click);
    assert_eq!(mappings[0].pattern, HapticPattern::Tap);
    crate::serial_println!("  [1/8] default mappings: OK");

    // 2: Add device.
    let id = add_device("Trackpad", DeviceType::Trackpad).expect("add");
    assert_eq!(list_devices().len(), 1);
    crate::serial_println!("  [2/8] add device: OK");

    // 3: Set intensity.
    set_intensity(id, 75).expect("intensity");
    let devs = list_devices();
    assert_eq!(devs[0].intensity, 75);
    crate::serial_println!("  [3/8] set intensity: OK");

    // 4: Fire event.
    let pattern = fire(HapticEvent::Click).expect("fire");
    assert_eq!(pattern, HapticPattern::Tap);
    let devs = list_devices();
    assert_eq!(devs[0].fire_count, 1);
    crate::serial_println!("  [4/8] fire event: OK");

    // 5: Disabled event returns None.
    let pattern = fire(HapticEvent::KeyPress).expect("fire2");
    assert_eq!(pattern, HapticPattern::None);
    crate::serial_println!("  [5/8] disabled event: OK");

    // 6: Change pattern.
    set_event_pattern(HapticEvent::Click, HapticPattern::Impact).expect("pattern");
    let pattern = fire(HapticEvent::Click).expect("fire3");
    assert_eq!(pattern, HapticPattern::Impact);
    crate::serial_println!("  [6/8] change pattern: OK");

    // 7: Global disable.
    set_global_enabled(false).expect("global");
    let pattern = fire(HapticEvent::Click).expect("fire4");
    assert_eq!(pattern, HapticPattern::None);
    set_global_enabled(true).expect("global2");
    crate::serial_println!("  [7/8] global disable: OK");

    // 8: Stats.
    let (devs, maps, fires, ops) = stats();
    assert_eq!(devs, 1);
    assert_eq!(maps, 12);
    assert!(fires >= 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("haptfeedback::self_test() — all 8 tests passed");
}

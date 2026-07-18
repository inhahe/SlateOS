//! Touchscreen Settings — touchscreen input configuration.
//!
//! Configures touchscreen behavior including gesture recognition,
//! palm rejection, sensitivity, and multi-touch settings.
//!
//! ## Architecture
//!
//! ```text
//! Touch input
//!   → touchscreen::configure(setting, value) → apply
//!   → touchscreen::calibrate() → run calibration
//!   → touchscreen::get_gesture(id) → gesture info
//!
//! Integration:
//!   → gestures (gesture recognition)
//!   → touchpad (trackpad settings)
//!   → display (screen mapping)
//!   → a11y (accessibility touch)
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

/// Touch gesture type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureType {
    Tap,
    DoubleTap,
    LongPress,
    Swipe,
    Pinch,
    Rotate,
    ThreeFingerSwipe,
    FourFingerSwipe,
    EdgeSwipe,
}

impl GestureType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tap => "Tap",
            Self::DoubleTap => "Double Tap",
            Self::LongPress => "Long Press",
            Self::Swipe => "Swipe",
            Self::Pinch => "Pinch",
            Self::Rotate => "Rotate",
            Self::ThreeFingerSwipe => "3-Finger Swipe",
            Self::FourFingerSwipe => "4-Finger Swipe",
            Self::EdgeSwipe => "Edge Swipe",
        }
    }
}

/// Gesture binding (what a gesture does).
#[derive(Debug, Clone)]
pub struct GestureBinding {
    pub id: u32,
    pub gesture: GestureType,
    pub action: String,
    pub enabled: bool,
    pub trigger_count: u64,
}

/// Touchscreen device.
#[derive(Debug, Clone)]
pub struct TouchDevice {
    pub id: u32,
    pub name: String,
    pub max_touches: u8,
    pub calibrated: bool,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_GESTURES: usize = 50;
const MAX_DEVICES: usize = 10;

struct State {
    devices: Vec<TouchDevice>,
    gestures: Vec<GestureBinding>,
    next_device_id: u32,
    next_gesture_id: u32,
    sensitivity: u32,        // 1-100.
    palm_rejection: bool,
    touch_sound: bool,
    touch_vibration: bool,
    total_touches: u64,
    total_gestures: u64,
    calibrations: u64,
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
        devices: Vec::new(),
        gestures: alloc::vec![
            GestureBinding { id: 1, gesture: GestureType::Tap, action: String::from("click"), enabled: true, trigger_count: 0 },
            GestureBinding { id: 2, gesture: GestureType::DoubleTap, action: String::from("double_click"), enabled: true, trigger_count: 0 },
            GestureBinding { id: 3, gesture: GestureType::LongPress, action: String::from("right_click"), enabled: true, trigger_count: 0 },
            GestureBinding { id: 4, gesture: GestureType::Pinch, action: String::from("zoom"), enabled: true, trigger_count: 0 },
            GestureBinding { id: 5, gesture: GestureType::Swipe, action: String::from("scroll"), enabled: true, trigger_count: 0 },
            GestureBinding { id: 6, gesture: GestureType::EdgeSwipe, action: String::from("show_panel"), enabled: true, trigger_count: 0 },
        ],
        next_device_id: 1,
        next_gesture_id: 7,
        sensitivity: 50,
        palm_rejection: true,
        touch_sound: false,
        touch_vibration: true,
        total_touches: 0,
        total_gestures: 0,
        calibrations: 0,
        ops: 0,
    });
}

/// Register a touchscreen device.
pub fn add_device(name: &str, max_touches: u8) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_device_id;
        state.next_device_id += 1;
        state.devices.push(TouchDevice {
            id, name: String::from(name), max_touches,
            calibrated: false, enabled: true,
        });
        Ok(id)
    })
}

/// Remove a touchscreen device.
pub fn remove_device(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.id != id);
        if state.devices.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Calibrate a device.
pub fn calibrate(device_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.calibrated = true;
        state.calibrations += 1;
        Ok(())
    })
}

/// Set sensitivity (1-100).
pub fn set_sensitivity(value: u32) -> KernelResult<()> {
    if value == 0 || value > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.sensitivity = value;
        Ok(())
    })
}

/// Toggle palm rejection.
pub fn set_palm_rejection(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.palm_rejection = enabled;
        Ok(())
    })
}

/// Toggle touch sound.
pub fn set_touch_sound(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.touch_sound = enabled;
        Ok(())
    })
}

/// Toggle touch vibration.
pub fn set_touch_vibration(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.touch_vibration = enabled;
        Ok(())
    })
}

/// Add or update a gesture binding.
pub fn set_gesture(gesture: GestureType, action: &str) -> KernelResult<u32> {
    with_state(|state| {
        if let Some(g) = state.gestures.iter_mut().find(|g| g.gesture == gesture) {
            g.action = String::from(action);
            Ok(g.id)
        } else {
            if state.gestures.len() >= MAX_GESTURES {
                return Err(KernelError::ResourceExhausted);
            }
            let id = state.next_gesture_id;
            state.next_gesture_id += 1;
            state.gestures.push(GestureBinding {
                id, gesture, action: String::from(action),
                enabled: true, trigger_count: 0,
            });
            Ok(id)
        }
    })
}

/// Enable/disable a gesture.
pub fn set_gesture_enabled(gesture: GestureType, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let g = state.gestures.iter_mut().find(|g| g.gesture == gesture)
            .ok_or(KernelError::NotFound)?;
        g.enabled = enabled;
        Ok(())
    })
}

/// Record a touch event.
pub fn record_touch() -> KernelResult<()> {
    with_state(|state| {
        state.total_touches += 1;
        Ok(())
    })
}

/// Record a gesture trigger.
pub fn trigger_gesture(gesture: GestureType) -> KernelResult<Option<String>> {
    with_state(|state| {
        if let Some(g) = state.gestures.iter_mut().find(|g| g.gesture == gesture && g.enabled) {
            g.trigger_count += 1;
            state.total_gestures += 1;
            Ok(Some(g.action.clone()))
        } else {
            Ok(None)
        }
    })
}

/// List devices.
pub fn list_devices() -> Vec<TouchDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// List gesture bindings.
pub fn list_gestures() -> Vec<GestureBinding> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.gestures.clone())
}

/// Get current sensitivity.
pub fn get_sensitivity() -> u32 {
    STATE.lock().as_ref().map_or(50, |s| s.sensitivity)
}

/// Statistics: (device_count, gesture_count, total_touches, total_gestures, calibrations, ops).
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.gestures.len(), s.total_touches, s.total_gestures, s.calibrations, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("touchscreen::self_test() — running tests...");
    init_defaults();

    // 1: Default gestures.
    assert_eq!(list_gestures().len(), 6);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Add device.
    let d1 = add_device("Main Display Touch", 10).expect("add");
    assert_eq!(list_devices().len(), 1);
    crate::serial_println!("  [2/8] add device: OK");

    // 3: Calibrate.
    calibrate(d1).expect("cal");
    let devs = list_devices();
    assert!(devs[0].calibrated);
    crate::serial_println!("  [3/8] calibrate: OK");

    // 4: Sensitivity.
    set_sensitivity(80).expect("sens");
    assert_eq!(get_sensitivity(), 80);
    assert!(set_sensitivity(0).is_err());
    assert!(set_sensitivity(101).is_err());
    crate::serial_println!("  [4/8] sensitivity: OK");

    // 5: Gesture trigger.
    let action = trigger_gesture(GestureType::Tap).expect("trigger");
    assert_eq!(action, Some(String::from("click")));
    crate::serial_println!("  [5/8] gesture trigger: OK");

    // 6: Custom gesture.
    set_gesture(GestureType::Rotate, "rotate_view").expect("custom");
    let action = trigger_gesture(GestureType::Rotate).expect("trigger2");
    assert_eq!(action, Some(String::from("rotate_view")));
    crate::serial_println!("  [6/8] custom gesture: OK");

    // 7: Disable gesture.
    set_gesture_enabled(GestureType::Tap, false).expect("disable");
    let action = trigger_gesture(GestureType::Tap).expect("trigger3");
    assert_eq!(action, None);
    crate::serial_println!("  [7/8] disable gesture: OK");

    // 8: Stats.
    let (devices, gestures, _touches, gest_count, cals, ops) = stats();
    assert_eq!(devices, 1);
    assert!(gestures >= 6);
    assert!(gest_count >= 2);
    assert_eq!(cals, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("touchscreen::self_test() — all 8 tests passed");
}

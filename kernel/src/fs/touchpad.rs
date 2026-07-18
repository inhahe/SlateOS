//! Touchpad settings — gestures, tapping, scrolling, sensitivity.
//!
//! Configures laptop/tablet touchpad input including tap-to-click,
//! two-finger/edge scrolling, multi-finger gestures, palm rejection,
//! and disable-while-typing.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Devices → Touchpad
//!   → touchpad::set_tap_to_click() / set_gesture()
//!
//! Input driver integration
//!   → touchpad::config() for current touchpad parameters
//!
//! Integration:
//!   → mousesettings (shared pointer speed)
//!   → a11y (gesture accessibility)
//!   → power (disable in clamshell mode)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_TOUCHPADS: usize = 4;
const MAX_GESTURES: usize = 32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Scroll method for touchpad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchScrollMethod {
    /// Two-finger scrolling.
    TwoFinger,
    /// Edge scrolling (right/bottom edge).
    Edge,
    /// Disabled.
    None,
}

impl TouchScrollMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::TwoFinger => "Two-finger",
            Self::Edge => "Edge",
            Self::None => "Disabled",
        }
    }
}

/// Click method for touchpad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickMethod {
    /// Button areas (bottom-left = primary, bottom-right = secondary).
    ButtonAreas,
    /// Click finger count (1 = primary, 2 = secondary, 3 = middle).
    ClickFinger,
    /// No click zones.
    None,
}

impl ClickMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::ButtonAreas => "Button Areas",
            Self::ClickFinger => "Click Finger",
            Self::None => "None",
        }
    }
}

/// Gesture type for multi-finger gestures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureType {
    /// Three-finger swipe up (e.g., overview).
    SwipeUp3,
    /// Three-finger swipe down (e.g., minimize).
    SwipeDown3,
    /// Three-finger swipe left (e.g., switch workspace).
    SwipeLeft3,
    /// Three-finger swipe right.
    SwipeRight3,
    /// Four-finger swipe up (e.g., show desktop).
    SwipeUp4,
    /// Four-finger swipe down.
    SwipeDown4,
    /// Four-finger swipe left.
    SwipeLeft4,
    /// Four-finger swipe right.
    SwipeRight4,
    /// Pinch in (zoom out).
    PinchIn,
    /// Pinch out (zoom in).
    PinchOut,
}

impl GestureType {
    pub fn label(self) -> &'static str {
        match self {
            Self::SwipeUp3 => "3-finger swipe up",
            Self::SwipeDown3 => "3-finger swipe down",
            Self::SwipeLeft3 => "3-finger swipe left",
            Self::SwipeRight3 => "3-finger swipe right",
            Self::SwipeUp4 => "4-finger swipe up",
            Self::SwipeDown4 => "4-finger swipe down",
            Self::SwipeLeft4 => "4-finger swipe left",
            Self::SwipeRight4 => "4-finger swipe right",
            Self::PinchIn => "Pinch in",
            Self::PinchOut => "Pinch out",
        }
    }
}

/// Gesture action assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureAction {
    /// No action.
    None,
    /// Show overview / app switcher.
    Overview,
    /// Show desktop.
    ShowDesktop,
    /// Minimize window.
    MinimizeWindow,
    /// Switch workspace left.
    WorkspaceLeft,
    /// Switch workspace right.
    WorkspaceRight,
    /// Switch workspace up.
    WorkspaceUp,
    /// Switch workspace down.
    WorkspaceDown,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
    /// Back (browser/file manager).
    Back,
    /// Forward.
    Forward,
}

impl GestureAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Overview => "Overview",
            Self::ShowDesktop => "Show Desktop",
            Self::MinimizeWindow => "Minimize",
            Self::WorkspaceLeft => "Workspace Left",
            Self::WorkspaceRight => "Workspace Right",
            Self::WorkspaceUp => "Workspace Up",
            Self::WorkspaceDown => "Workspace Down",
            Self::ZoomIn => "Zoom In",
            Self::ZoomOut => "Zoom Out",
            Self::Back => "Back",
            Self::Forward => "Forward",
        }
    }
}

/// Gesture binding.
#[derive(Debug, Clone)]
pub struct GestureBinding {
    pub gesture: GestureType,
    pub action: GestureAction,
    pub enabled: bool,
}

/// Touchpad device info.
#[derive(Debug, Clone)]
pub struct TouchpadDevice {
    /// Device ID.
    pub id: u32,
    /// Device name.
    pub name: String,
    /// Max simultaneous touch points.
    pub max_fingers: u8,
    /// Width (mm).
    pub width_mm: u32,
    /// Height (mm).
    pub height_mm: u32,
    /// Pressure sensitive.
    pub pressure_sensitive: bool,
    /// Enabled.
    pub enabled: bool,
}

/// Touchpad configuration.
#[derive(Debug, Clone)]
pub struct TouchpadConfig {
    /// Touchpad enabled.
    pub enabled: bool,
    /// Tap to click.
    pub tap_to_click: bool,
    /// Two-finger tap for right-click.
    pub two_finger_tap: bool,
    /// Three-finger tap for middle-click.
    pub three_finger_tap: bool,
    /// Tap and drag.
    pub tap_and_drag: bool,
    /// Drag lock (keep dragging after lift).
    pub drag_lock: bool,
    /// Sensitivity (1-20).
    pub sensitivity: u32,
    /// Scroll method.
    pub scroll_method: TouchScrollMethod,
    /// Natural scrolling.
    pub natural_scroll: bool,
    /// Scroll speed (1-20).
    pub scroll_speed: u32,
    /// Click method.
    pub click_method: ClickMethod,
    /// Disable while typing.
    pub disable_while_typing: bool,
    /// Disable when external mouse connected.
    pub disable_on_external_mouse: bool,
    /// Palm rejection.
    pub palm_rejection: bool,
    /// Palm rejection sensitivity (1-10).
    pub palm_sensitivity: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct TouchpadState {
    config: TouchpadConfig,
    devices: Vec<TouchpadDevice>,
    gestures: Vec<GestureBinding>,
    next_device_id: u32,
    ops: u64,
}

static STATE: Mutex<Option<TouchpadState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut TouchpadState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the touchpad settings subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let default_gestures = alloc::vec![
        GestureBinding { gesture: GestureType::SwipeUp3, action: GestureAction::Overview, enabled: true },
        GestureBinding { gesture: GestureType::SwipeDown3, action: GestureAction::ShowDesktop, enabled: true },
        GestureBinding { gesture: GestureType::SwipeLeft3, action: GestureAction::WorkspaceRight, enabled: true },
        GestureBinding { gesture: GestureType::SwipeRight3, action: GestureAction::WorkspaceLeft, enabled: true },
        GestureBinding { gesture: GestureType::SwipeUp4, action: GestureAction::Overview, enabled: true },
        GestureBinding { gesture: GestureType::SwipeDown4, action: GestureAction::ShowDesktop, enabled: true },
        GestureBinding { gesture: GestureType::SwipeLeft4, action: GestureAction::WorkspaceRight, enabled: true },
        GestureBinding { gesture: GestureType::SwipeRight4, action: GestureAction::WorkspaceLeft, enabled: true },
        GestureBinding { gesture: GestureType::PinchIn, action: GestureAction::ZoomOut, enabled: true },
        GestureBinding { gesture: GestureType::PinchOut, action: GestureAction::ZoomIn, enabled: true },
    ];

    *guard = Some(TouchpadState {
        config: TouchpadConfig {
            enabled: true,
            tap_to_click: true,
            two_finger_tap: true,
            three_finger_tap: true,
            tap_and_drag: true,
            drag_lock: false,
            sensitivity: 10,
            scroll_method: TouchScrollMethod::TwoFinger,
            natural_scroll: true,
            scroll_speed: 10,
            click_method: ClickMethod::ClickFinger,
            disable_while_typing: true,
            disable_on_external_mouse: false,
            palm_rejection: true,
            palm_sensitivity: 5,
        },
        devices: Vec::new(),
        gestures: default_gestures,
        next_device_id: 1,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get the current touchpad configuration.
pub fn config() -> KernelResult<TouchpadConfig> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    Ok(state.config.clone())
}

/// Enable or disable the touchpad.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.enabled = enabled;
        Ok(())
    })
}

/// Set tap-to-click.
pub fn set_tap_to_click(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.tap_to_click = enabled;
        Ok(())
    })
}

/// Set two-finger tap (right-click).
pub fn set_two_finger_tap(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.two_finger_tap = enabled;
        Ok(())
    })
}

/// Set three-finger tap (middle-click).
pub fn set_three_finger_tap(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.three_finger_tap = enabled;
        Ok(())
    })
}

/// Set tap-and-drag.
pub fn set_tap_and_drag(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.tap_and_drag = enabled;
        Ok(())
    })
}

/// Set drag lock.
pub fn set_drag_lock(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.drag_lock = enabled;
        Ok(())
    })
}

/// Set sensitivity (1-20).
pub fn set_sensitivity(val: u32) -> KernelResult<()> {
    if val < 1 || val > 20 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.sensitivity = val;
        Ok(())
    })
}

/// Set scroll method.
pub fn set_scroll_method(method: TouchScrollMethod) -> KernelResult<()> {
    with_state(|state| {
        state.config.scroll_method = method;
        Ok(())
    })
}

/// Set natural scrolling.
pub fn set_natural_scroll(natural: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.natural_scroll = natural;
        Ok(())
    })
}

/// Set scroll speed (1-20).
pub fn set_scroll_speed(speed: u32) -> KernelResult<()> {
    if speed < 1 || speed > 20 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.scroll_speed = speed;
        Ok(())
    })
}

/// Set click method.
pub fn set_click_method(method: ClickMethod) -> KernelResult<()> {
    with_state(|state| {
        state.config.click_method = method;
        Ok(())
    })
}

/// Set disable-while-typing.
pub fn set_disable_while_typing(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.disable_while_typing = enabled;
        Ok(())
    })
}

/// Set disable when external mouse connected.
pub fn set_disable_on_external_mouse(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.disable_on_external_mouse = enabled;
        Ok(())
    })
}

/// Set palm rejection.
pub fn set_palm_rejection(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.palm_rejection = enabled;
        Ok(())
    })
}

/// Set palm rejection sensitivity (1-10).
pub fn set_palm_sensitivity(val: u32) -> KernelResult<()> {
    if val < 1 || val > 10 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.palm_sensitivity = val;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Gestures
// ---------------------------------------------------------------------------

/// List all gesture bindings.
pub fn list_gestures() -> Vec<GestureBinding> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.gestures.clone())
}

/// Set gesture action.
pub fn set_gesture(gesture: GestureType, action: GestureAction) -> KernelResult<()> {
    with_state(|state| {
        if let Some(binding) = state.gestures.iter_mut().find(|g| g.gesture == gesture) {
            binding.action = action;
        } else {
            if state.gestures.len() >= MAX_GESTURES {
                return Err(KernelError::ResourceExhausted);
            }
            state.gestures.push(GestureBinding {
                gesture,
                action,
                enabled: true,
            });
        }
        Ok(())
    })
}

/// Enable or disable a gesture.
pub fn set_gesture_enabled(gesture: GestureType, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let binding = state.gestures.iter_mut()
            .find(|g| g.gesture == gesture)
            .ok_or(KernelError::NotFound)?;
        binding.enabled = enabled;
        Ok(())
    })
}

/// Get the action for a gesture.
pub fn gesture_action(gesture: GestureType) -> Option<GestureAction> {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| {
        s.gestures.iter()
            .find(|g| g.gesture == gesture && g.enabled)
            .map(|g| g.action)
    })
}

// ---------------------------------------------------------------------------
// Device management
// ---------------------------------------------------------------------------

/// Register a touchpad device.
pub fn add_device(name: &str, max_fingers: u8, width_mm: u32, height_mm: u32, pressure: bool) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_TOUCHPADS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_device_id;
        state.next_device_id += 1;
        state.devices.push(TouchpadDevice {
            id,
            name: String::from(name),
            max_fingers,
            width_mm,
            height_mm,
            pressure_sensitive: pressure,
            enabled: true,
        });
        Ok(id)
    })
}

/// Remove a touchpad device.
pub fn remove_device(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.devices.iter().position(|d| d.id == id) {
            state.devices.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// List all touchpad devices.
pub fn list_devices() -> Vec<TouchpadDevice> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.devices.clone())
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (device_count, gesture_count, tap_to_click, natural_scroll, sensitivity, ops).
pub fn stats() -> (usize, usize, bool, bool, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.devices.len(),
            s.gestures.len(),
            s.config.tap_to_click,
            s.config.natural_scroll,
            s.config.sensitivity,
            s.ops,
        ),
        None => (0, 0, false, false, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the touchpad settings module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[touchpad] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: default config.
    {
        let cfg = config().unwrap();
        assert!(cfg.enabled);
        assert!(cfg.tap_to_click);
        assert!(cfg.two_finger_tap);
        assert!(cfg.natural_scroll);
        assert!(cfg.disable_while_typing);
        assert!(cfg.palm_rejection);
        assert_eq!(cfg.sensitivity, 10);
    }
    serial_println!("[touchpad]  1/11 default config OK");

    // Test 2: tap settings.
    {
        set_tap_to_click(false).unwrap();
        assert!(!config().unwrap().tap_to_click);
        set_tap_to_click(true).unwrap();
        set_two_finger_tap(false).unwrap();
        assert!(!config().unwrap().two_finger_tap);
        set_two_finger_tap(true).unwrap();
    }
    serial_println!("[touchpad]  2/11 tap settings OK");

    // Test 3: drag settings.
    {
        set_tap_and_drag(false).unwrap();
        assert!(!config().unwrap().tap_and_drag);
        set_drag_lock(true).unwrap();
        assert!(config().unwrap().drag_lock);
        set_tap_and_drag(true).unwrap();
        set_drag_lock(false).unwrap();
    }
    serial_println!("[touchpad]  3/11 drag settings OK");

    // Test 4: sensitivity.
    {
        set_sensitivity(15).unwrap();
        assert_eq!(config().unwrap().sensitivity, 15);
        assert!(set_sensitivity(0).is_err());
        assert!(set_sensitivity(21).is_err());
    }
    serial_println!("[touchpad]  4/11 sensitivity OK");

    // Test 5: scrolling.
    {
        set_scroll_method(TouchScrollMethod::Edge).unwrap();
        assert_eq!(config().unwrap().scroll_method, TouchScrollMethod::Edge);
        set_natural_scroll(false).unwrap();
        assert!(!config().unwrap().natural_scroll);
        set_scroll_speed(18).unwrap();
        assert_eq!(config().unwrap().scroll_speed, 18);
    }
    serial_println!("[touchpad]  5/11 scroll settings OK");

    // Test 6: click method.
    {
        set_click_method(ClickMethod::ButtonAreas).unwrap();
        assert_eq!(config().unwrap().click_method, ClickMethod::ButtonAreas);
    }
    serial_println!("[touchpad]  6/11 click method OK");

    // Test 7: palm rejection.
    {
        set_palm_rejection(false).unwrap();
        assert!(!config().unwrap().palm_rejection);
        set_palm_sensitivity(8).unwrap();
        assert_eq!(config().unwrap().palm_sensitivity, 8);
        assert!(set_palm_sensitivity(0).is_err());
    }
    serial_println!("[touchpad]  7/11 palm rejection OK");

    // Test 8: disable features.
    {
        set_disable_while_typing(false).unwrap();
        assert!(!config().unwrap().disable_while_typing);
        set_disable_on_external_mouse(true).unwrap();
        assert!(config().unwrap().disable_on_external_mouse);
    }
    serial_println!("[touchpad]  8/11 disable features OK");

    // Test 9: default gestures.
    {
        let gestures = list_gestures();
        assert_eq!(gestures.len(), 10);
        let action = gesture_action(GestureType::SwipeUp3);
        assert_eq!(action, Some(GestureAction::Overview));
    }
    serial_println!("[touchpad]  9/11 default gestures OK");

    // Test 10: modify gesture.
    {
        set_gesture(GestureType::SwipeUp3, GestureAction::ShowDesktop).unwrap();
        assert_eq!(gesture_action(GestureType::SwipeUp3), Some(GestureAction::ShowDesktop));
        set_gesture_enabled(GestureType::SwipeUp3, false).unwrap();
        assert_eq!(gesture_action(GestureType::SwipeUp3), None);
    }
    serial_println!("[touchpad] 10/11 modify gesture OK");

    // Test 11: device management.
    {
        let id = add_device("SynPS/2 Synaptics TouchPad", 5, 100, 60, true).unwrap();
        let devices = list_devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices.first().unwrap().name, "SynPS/2 Synaptics TouchPad");
        assert_eq!(devices.first().unwrap().max_fingers, 5);
        remove_device(id).unwrap();
        assert!(list_devices().is_empty());
    }
    serial_println!("[touchpad] 11/11 device management OK");

    serial_println!("[touchpad] All self-tests passed.");
}

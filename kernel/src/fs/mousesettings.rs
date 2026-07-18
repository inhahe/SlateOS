//! Mouse settings — pointer speed, acceleration, buttons, scrolling.
//!
//! Provides a centralized configuration backend for mouse input devices,
//! covering sensitivity, acceleration profiles, button mapping, scroll
//! behaviour, and double-click timing.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Devices → Mouse
//!   → mousesettings::set_speed() / set_accel_profile()
//!
//! Input driver integration
//!   → mousesettings::config() for current pointer parameters
//!
//! Integration:
//!   → cursorsettings (cursor appearance)
//!   → a11y (pointer accessibility)
//!   → display (DPI scaling awareness)
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

const MIN_SPEED: u32 = 1;
const MAX_SPEED: u32 = 20;
const DEFAULT_SPEED: u32 = 10;
const MIN_DOUBLE_CLICK_MS: u32 = 100;
const MAX_DOUBLE_CLICK_MS: u32 = 1000;
const DEFAULT_DOUBLE_CLICK_MS: u32 = 400;
const MAX_MICE: usize = 8;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Acceleration profile for pointer movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccelProfile {
    /// No acceleration — linear movement.
    Flat,
    /// Adaptive acceleration (faster movement = higher gain).
    Adaptive,
    /// Custom acceleration curve.
    Custom,
}

impl AccelProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::Adaptive => "Adaptive",
            Self::Custom => "Custom",
        }
    }
}

/// Scroll method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollMethod {
    /// Standard discrete wheel scrolling.
    Wheel,
    /// Smooth (pixel-precise) scrolling.
    Smooth,
    /// No scrolling.
    None,
}

impl ScrollMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wheel => "Wheel",
            Self::Smooth => "Smooth",
            Self::None => "None",
        }
    }
}

/// Mouse button assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonAction {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
    None,
}

impl ButtonAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Secondary => "Secondary",
            Self::Middle => "Middle",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::None => "None",
        }
    }
}

/// Connected mouse device info.
#[derive(Debug, Clone)]
pub struct MouseDevice {
    /// Device ID.
    pub id: u32,
    /// Device name.
    pub name: String,
    /// Number of buttons.
    pub buttons: u8,
    /// Has scroll wheel.
    pub has_wheel: bool,
    /// DPI/CPI setting.
    pub dpi: u32,
    /// Is wireless.
    pub wireless: bool,
    /// Battery percentage (0-100, 0 for wired).
    pub battery_pct: u8,
    /// Connected.
    pub connected: bool,
}

/// Mouse configuration.
#[derive(Debug, Clone)]
pub struct MouseConfig {
    /// Pointer speed (1-20).
    pub speed: u32,
    /// Acceleration profile.
    pub accel_profile: AccelProfile,
    /// Acceleration factor (0 = off, 1-10 = strength).
    pub accel_factor: u32,
    /// Left-handed (swap primary and secondary buttons).
    pub left_handed: bool,
    /// Natural scrolling (reverse scroll direction).
    pub natural_scroll: bool,
    /// Scroll speed (1-20).
    pub scroll_speed: u32,
    /// Scroll method.
    pub scroll_method: ScrollMethod,
    /// Double-click interval (ms).
    pub double_click_ms: u32,
    /// Middle-click paste.
    pub middle_click_paste: bool,
    /// Scroll lines per notch.
    pub scroll_lines: u32,
    /// Button mapping: physical button index → action.
    pub button_map: Vec<(u8, ButtonAction)>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct MouseState {
    config: MouseConfig,
    devices: Vec<MouseDevice>,
    next_device_id: u32,
    ops: u64,
}

static STATE: Mutex<Option<MouseState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut MouseState) -> KernelResult<R>,
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

/// Initialize the mouse settings subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(MouseState {
        config: MouseConfig {
            speed: DEFAULT_SPEED,
            accel_profile: AccelProfile::Adaptive,
            accel_factor: 5,
            left_handed: false,
            natural_scroll: false,
            scroll_speed: 10,
            scroll_method: ScrollMethod::Wheel,
            double_click_ms: DEFAULT_DOUBLE_CLICK_MS,
            middle_click_paste: true,
            scroll_lines: 3,
            button_map: alloc::vec![
                (1, ButtonAction::Primary),
                (2, ButtonAction::Middle),
                (3, ButtonAction::Secondary),
                (4, ButtonAction::Back),
                (5, ButtonAction::Forward),
            ],
        },
        devices: Vec::new(),
        next_device_id: 1,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get the current mouse configuration.
pub fn config() -> KernelResult<MouseConfig> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    Ok(state.config.clone())
}

/// Set pointer speed (1-20).
pub fn set_speed(speed: u32) -> KernelResult<()> {
    if speed < MIN_SPEED || speed > MAX_SPEED {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.speed = speed;
        Ok(())
    })
}

/// Set acceleration profile.
pub fn set_accel_profile(profile: AccelProfile) -> KernelResult<()> {
    with_state(|state| {
        state.config.accel_profile = profile;
        Ok(())
    })
}

/// Set acceleration factor (0 = off, 1-10).
pub fn set_accel_factor(factor: u32) -> KernelResult<()> {
    if factor > 10 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.accel_factor = factor;
        Ok(())
    })
}

/// Set left-handed mode (swap primary/secondary buttons).
pub fn set_left_handed(left: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.left_handed = left;
        Ok(())
    })
}

/// Set natural scrolling (reversed direction).
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

/// Set scroll method.
pub fn set_scroll_method(method: ScrollMethod) -> KernelResult<()> {
    with_state(|state| {
        state.config.scroll_method = method;
        Ok(())
    })
}

/// Set double-click interval (100-1000 ms).
pub fn set_double_click_ms(ms: u32) -> KernelResult<()> {
    if ms < MIN_DOUBLE_CLICK_MS || ms > MAX_DOUBLE_CLICK_MS {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.double_click_ms = ms;
        Ok(())
    })
}

/// Set middle-click paste.
pub fn set_middle_click_paste(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.middle_click_paste = enabled;
        Ok(())
    })
}

/// Set scroll lines per notch.
pub fn set_scroll_lines(lines: u32) -> KernelResult<()> {
    if lines == 0 || lines > 20 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.scroll_lines = lines;
        Ok(())
    })
}

/// Set button mapping for a physical button.
pub fn set_button_map(physical: u8, action: ButtonAction) -> KernelResult<()> {
    with_state(|state| {
        if let Some(entry) = state.config.button_map.iter_mut().find(|(b, _)| *b == physical) {
            entry.1 = action;
        } else {
            state.config.button_map.push((physical, action));
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Device management
// ---------------------------------------------------------------------------

/// Register a mouse device.
pub fn add_device(name: &str, buttons: u8, has_wheel: bool, dpi: u32, wireless: bool) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_MICE {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_device_id;
        state.next_device_id += 1;
        state.devices.push(MouseDevice {
            id,
            name: String::from(name),
            buttons,
            has_wheel,
            dpi,
            wireless,
            battery_pct: if wireless { 100 } else { 0 },
            connected: true,
        });
        Ok(id)
    })
}

/// Remove a mouse device.
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

/// Get a mouse device by ID.
pub fn get_device(id: u32) -> KernelResult<MouseDevice> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.devices.iter()
        .find(|d| d.id == id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all mouse devices.
pub fn list_devices() -> Vec<MouseDevice> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.devices.clone())
}

/// Set device DPI.
pub fn set_device_dpi(id: u32, dpi: u32) -> KernelResult<()> {
    if dpi == 0 || dpi > 25600 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let dev = state.devices.iter_mut()
            .find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        dev.dpi = dpi;
        Ok(())
    })
}

/// Update device battery.
pub fn update_battery(id: u32, pct: u8) -> KernelResult<()> {
    if pct > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let dev = state.devices.iter_mut()
            .find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        dev.battery_pct = pct;
        Ok(())
    })
}

/// Set device connected state.
pub fn set_connected(id: u32, connected: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut()
            .find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        dev.connected = connected;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (device_count, speed, accel_profile, left_handed, natural_scroll, ops).
pub fn stats() -> (usize, u32, &'static str, bool, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.devices.len(),
            s.config.speed,
            s.config.accel_profile.label(),
            s.config.left_handed,
            s.config.natural_scroll,
            s.ops,
        ),
        None => (0, 0, "n/a", false, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the mouse settings module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[mousesettings] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: default config.
    {
        let cfg = config().unwrap();
        assert_eq!(cfg.speed, 10);
        assert_eq!(cfg.accel_profile, AccelProfile::Adaptive);
        assert!(!cfg.left_handed);
        assert!(!cfg.natural_scroll);
        assert_eq!(cfg.double_click_ms, 400);
        assert_eq!(cfg.scroll_lines, 3);
    }
    serial_println!("[mousesettings]  1/11 default config OK");

    // Test 2: set speed.
    {
        set_speed(15).unwrap();
        assert_eq!(config().unwrap().speed, 15);
        assert!(set_speed(0).is_err());
        assert!(set_speed(21).is_err());
    }
    serial_println!("[mousesettings]  2/11 speed OK");

    // Test 3: acceleration.
    {
        set_accel_profile(AccelProfile::Flat).unwrap();
        assert_eq!(config().unwrap().accel_profile, AccelProfile::Flat);
        set_accel_factor(8).unwrap();
        assert_eq!(config().unwrap().accel_factor, 8);
        assert!(set_accel_factor(11).is_err());
    }
    serial_println!("[mousesettings]  3/11 acceleration OK");

    // Test 4: left-handed.
    {
        set_left_handed(true).unwrap();
        assert!(config().unwrap().left_handed);
        set_left_handed(false).unwrap();
    }
    serial_println!("[mousesettings]  4/11 left-handed OK");

    // Test 5: natural scroll.
    {
        set_natural_scroll(true).unwrap();
        assert!(config().unwrap().natural_scroll);
        set_natural_scroll(false).unwrap();
    }
    serial_println!("[mousesettings]  5/11 natural scroll OK");

    // Test 6: scroll settings.
    {
        set_scroll_speed(18).unwrap();
        assert_eq!(config().unwrap().scroll_speed, 18);
        set_scroll_method(ScrollMethod::Smooth).unwrap();
        assert_eq!(config().unwrap().scroll_method, ScrollMethod::Smooth);
        set_scroll_lines(5).unwrap();
        assert_eq!(config().unwrap().scroll_lines, 5);
    }
    serial_println!("[mousesettings]  6/11 scroll settings OK");

    // Test 7: double-click.
    {
        set_double_click_ms(300).unwrap();
        assert_eq!(config().unwrap().double_click_ms, 300);
        assert!(set_double_click_ms(50).is_err());
        assert!(set_double_click_ms(1500).is_err());
    }
    serial_println!("[mousesettings]  7/11 double-click OK");

    // Test 8: button map.
    {
        set_button_map(1, ButtonAction::Secondary).unwrap();
        let cfg = config().unwrap();
        let b1 = cfg.button_map.iter().find(|(b, _)| *b == 1).unwrap();
        assert_eq!(b1.1, ButtonAction::Secondary);
    }
    serial_println!("[mousesettings]  8/11 button map OK");

    // Test 9: add device.
    {
        let id = add_device("Logitech G502", 8, true, 1600, true).unwrap();
        let dev = get_device(id).unwrap();
        assert_eq!(dev.name, "Logitech G502");
        assert_eq!(dev.buttons, 8);
        assert!(dev.has_wheel);
        assert_eq!(dev.dpi, 1600);
        assert!(dev.wireless);
        assert_eq!(dev.battery_pct, 100);
    }
    serial_println!("[mousesettings]  9/11 add device OK");

    // Test 10: device settings.
    {
        let devices = list_devices();
        let id = devices.first().unwrap().id;
        set_device_dpi(id, 3200).unwrap();
        assert_eq!(get_device(id).unwrap().dpi, 3200);
        update_battery(id, 75).unwrap();
        assert_eq!(get_device(id).unwrap().battery_pct, 75);
        set_connected(id, false).unwrap();
        assert!(!get_device(id).unwrap().connected);
    }
    serial_println!("[mousesettings] 10/11 device settings OK");

    // Test 11: remove device.
    {
        let devices = list_devices();
        let id = devices.first().unwrap().id;
        remove_device(id).unwrap();
        assert!(get_device(id).is_err());
        assert!(list_devices().is_empty());
    }
    serial_println!("[mousesettings] 11/11 remove device OK");

    serial_println!("[mousesettings] All self-tests passed.");
}

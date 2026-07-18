//! Brightness — display brightness control and ambient light sensing.
//!
//! Manages display backlight brightness with manual and automatic modes,
//! ambient light sensor integration, and per-display settings.
//!
//! ## Architecture
//!
//! ```text
//! User adjusts brightness
//!   → brightness::set_brightness(display_id, level)
//!     → writes to backlight driver
//!
//! Auto-brightness
//!   → brightness::update_ambient(lux) → recalculates
//!
//! Integration:
//!   → display (per-display backlight)
//!   → nightlight (color temperature)
//!   → power (battery saver dims display)
//!   → hotkeys (brightness up/down keys)
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

/// Brightness mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrightnessMode {
    Manual,
    Automatic,
    BatterySaver,
}

impl BrightnessMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Automatic => "Automatic",
            Self::BatterySaver => "Battery Saver",
        }
    }
}

/// A display with brightness control.
#[derive(Debug, Clone)]
pub struct DisplayBrightness {
    pub id: u32,
    pub name: String,
    /// Current brightness (0-100).
    pub brightness: u32,
    /// Minimum brightness (0-100).
    pub min_brightness: u32,
    pub mode: BrightnessMode,
    /// Auto-brightness curve: (ambient_lux, brightness_percent).
    pub auto_curve: Vec<(u32, u32)>,
    /// Current ambient light in lux (0 = no sensor).
    pub ambient_lux: u32,
    /// Whether dimmed for inactivity.
    pub dimmed: bool,
    /// Brightness before dimming.
    pub pre_dim_brightness: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DISPLAYS: usize = 8;

struct State {
    displays: Vec<DisplayBrightness>,
    next_id: u32,
    fade_step_ms: u32,
    total_adjustments: u64,
    total_auto_adjustments: u64,
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

    let default_curve = alloc::vec![
        (0, 10),      // Dark → 10%
        (50, 30),     // Dim room → 30%
        (200, 50),    // Normal → 50%
        (500, 75),    // Bright → 75%
        (1000, 100),  // Sunlight → 100%
    ];

    let display = DisplayBrightness {
        id: 1,
        name: String::from("Primary Display"),
        brightness: 70,
        min_brightness: 5,
        mode: BrightnessMode::Manual,
        auto_curve: default_curve,
        ambient_lux: 0,
        dimmed: false,
        pre_dim_brightness: 70,
    };

    *guard = Some(State {
        displays: alloc::vec![display],
        next_id: 2,
        fade_step_ms: 50,
        total_adjustments: 0,
        total_auto_adjustments: 0,
        ops: 0,
    });
}

/// Register a display.
pub fn register_display(name: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.displays.len() >= MAX_DISPLAYS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.displays.push(DisplayBrightness {
            id, name: String::from(name),
            brightness: 70, min_brightness: 5,
            mode: BrightnessMode::Manual,
            auto_curve: alloc::vec![(0, 10), (200, 50), (1000, 100)],
            ambient_lux: 0, dimmed: false, pre_dim_brightness: 70,
        });
        Ok(id)
    })
}

/// Set brightness manually (0-100).
pub fn set_brightness(display_id: u32, level: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.brightness = level.clamp(d.min_brightness, 100);
        d.dimmed = false;
        state.total_adjustments += 1;
        Ok(())
    })
}

/// Increase brightness by step.
pub fn brightness_up(display_id: u32, step: u32) -> KernelResult<u32> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.brightness = (d.brightness + step).min(100);
        d.dimmed = false;
        state.total_adjustments += 1;
        Ok(d.brightness)
    })
}

/// Decrease brightness by step.
pub fn brightness_down(display_id: u32, step: u32) -> KernelResult<u32> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.brightness = d.brightness.saturating_sub(step).max(d.min_brightness);
        d.dimmed = false;
        state.total_adjustments += 1;
        Ok(d.brightness)
    })
}

/// Update ambient light reading and auto-adjust if in auto mode.
pub fn update_ambient(display_id: u32, lux: u32) -> KernelResult<u32> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.ambient_lux = lux;

        if d.mode == BrightnessMode::Automatic {
            // Interpolate from curve.
            let mut target = d.auto_curve.last().map(|c| c.1).unwrap_or(50);
            for pair in d.auto_curve.windows(2) {
                let (lux_lo, bright_lo) = pair[0];
                let (lux_hi, bright_hi) = pair[1];
                if lux >= lux_lo && lux <= lux_hi {
                    let range_lux = lux_hi.saturating_sub(lux_lo).max(1);
                    let range_bright = bright_hi.saturating_sub(bright_lo);
                    target = bright_lo + (lux - lux_lo) * range_bright / range_lux;
                    break;
                }
                if lux < lux_lo {
                    target = bright_lo;
                    break;
                }
            }
            d.brightness = target.clamp(d.min_brightness, 100);
            state.total_auto_adjustments += 1;
        }
        Ok(d.brightness)
    })
}

/// Set brightness mode.
pub fn set_mode(display_id: u32, mode: BrightnessMode) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.mode = mode;
        if mode == BrightnessMode::BatterySaver {
            d.brightness = d.brightness.min(40);
        }
        Ok(())
    })
}

/// Dim display for inactivity.
pub fn dim(display_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        if !d.dimmed {
            d.pre_dim_brightness = d.brightness;
            d.brightness = d.min_brightness;
            d.dimmed = true;
        }
        Ok(())
    })
}

/// Undim display (restore previous brightness).
pub fn undim(display_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        if d.dimmed {
            d.brightness = d.pre_dim_brightness;
            d.dimmed = false;
        }
        Ok(())
    })
}

/// Set minimum brightness.
pub fn set_min_brightness(display_id: u32, min: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.min_brightness = min.clamp(0, 50);
        if d.brightness < d.min_brightness {
            d.brightness = d.min_brightness;
        }
        Ok(())
    })
}

/// List all displays.
pub fn list_displays() -> Vec<DisplayBrightness> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.displays.clone())
}

/// Get brightness for a display.
pub fn get_brightness(display_id: u32) -> KernelResult<u32> {
    with_state(|state| {
        let d = state.displays.iter().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        Ok(d.brightness)
    })
}

/// Statistics: (display_count, total_adjustments, total_auto, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.displays.len(), s.total_adjustments, s.total_auto_adjustments, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("brightness::self_test() — running tests...");
    init_defaults();

    // 1: Default display.
    let displays = list_displays();
    assert_eq!(displays.len(), 1);
    assert_eq!(displays[0].brightness, 70);
    crate::serial_println!("  [1/10] default display: OK");

    // 2: Set brightness.
    set_brightness(1, 50).expect("set");
    assert_eq!(get_brightness(1).expect("get"), 50);
    crate::serial_println!("  [2/10] set brightness: OK");

    // 3: Brightness up/down.
    let new = brightness_up(1, 10).expect("up");
    assert_eq!(new, 60);
    let new = brightness_down(1, 20).expect("down");
    assert_eq!(new, 40);
    crate::serial_println!("  [3/10] up/down: OK");

    // 4: Min brightness clamping.
    set_min_brightness(1, 10).expect("min");
    let new = brightness_down(1, 100).expect("down_max");
    assert_eq!(new, 10);
    crate::serial_println!("  [4/10] min brightness: OK");

    // 5: Dim/undim.
    set_brightness(1, 70).expect("set2");
    dim(1).expect("dim");
    let b = get_brightness(1).expect("get_dim");
    assert_eq!(b, 10); // min_brightness
    undim(1).expect("undim");
    assert_eq!(get_brightness(1).expect("get_undim"), 70);
    crate::serial_println!("  [5/10] dim/undim: OK");

    // 6: Auto mode.
    set_mode(1, BrightnessMode::Automatic).expect("auto");
    let b = update_ambient(1, 200).expect("ambient");
    assert!(b >= 40 && b <= 60); // Should be ~50% for 200 lux
    crate::serial_println!("  [6/10] auto brightness: OK");

    // 7: Battery saver.
    set_brightness(1, 80).expect("set3");
    set_mode(1, BrightnessMode::BatterySaver).expect("bsaver");
    assert!(get_brightness(1).expect("get_bs") <= 40);
    crate::serial_println!("  [7/10] battery saver: OK");

    // 8: Register second display.
    let d2 = register_display("External Monitor").expect("reg");
    assert_eq!(list_displays().len(), 2);
    crate::serial_println!("  [8/10] register display: OK");

    // 9: Independent controls.
    set_brightness(d2, 90).expect("set_d2");
    assert_eq!(get_brightness(d2).expect("get_d2"), 90);
    crate::serial_println!("  [9/10] independent: OK");

    // 10: Stats.
    let (count, adjustments, auto, ops) = stats();
    assert_eq!(count, 2);
    assert!(adjustments >= 4);
    assert!(auto >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] stats: OK");

    crate::serial_println!("brightness::self_test() — all 10 tests passed");
}

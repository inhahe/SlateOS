//! DPI Scaling — display scaling and HiDPI management.
//!
//! Manages per-display DPI scaling factors, fractional scaling,
//! and application-level DPI awareness settings.
//!
//! ## Architecture
//!
//! ```text
//! Display connected
//!   → dpiscaling::detect(display_id) → recommended scale
//!   → dpiscaling::set_scale(display_id, factor)
//!
//! Application launch
//!   → dpiscaling::get_app_scaling(app) → effective DPI
//!
//! Integration:
//!   → display (display management)
//!   → monitors (multi-display)
//!   → fontmgr (font rendering)
//!   → hdrdisplay (HDR displays)
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

/// DPI awareness level for an application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpiAwareness {
    /// App is unaware of DPI; system scales it.
    Unaware,
    /// App handles system DPI but not per-monitor.
    SystemAware,
    /// App handles per-monitor DPI changes.
    PerMonitorAware,
    /// App handles per-monitor DPI v2 (fine-grained).
    PerMonitorAwareV2,
}

impl DpiAwareness {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unaware => "Unaware",
            Self::SystemAware => "System Aware",
            Self::PerMonitorAware => "Per-Monitor Aware",
            Self::PerMonitorAwareV2 => "Per-Monitor Aware v2",
        }
    }
}

/// Scaling method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalingMethod {
    /// Integer scaling only (100%, 200%, 300%).
    Integer,
    /// Fractional scaling (125%, 150%, 175%, etc).
    Fractional,
    /// Render at integer scale, then resize.
    IntegerThenResize,
}

impl ScalingMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Integer => "Integer",
            Self::Fractional => "Fractional",
            Self::IntegerThenResize => "Integer + Resize",
        }
    }
}

/// Display scaling configuration.
#[derive(Debug, Clone)]
pub struct DisplayScale {
    pub id: u32,
    pub display_name: String,
    /// Scale factor in percent (100 = 1x, 200 = 2x, 150 = 1.5x).
    pub scale_percent: u32,
    /// Recommended scale percent based on physical size/resolution.
    pub recommended_percent: u32,
    pub method: ScalingMethod,
    /// Physical DPI of the display.
    pub physical_dpi: u32,
    /// Effective (logical) DPI.
    pub effective_dpi: u32,
}

/// Per-application DPI override.
#[derive(Debug, Clone)]
pub struct AppDpiOverride {
    pub app_name: String,
    pub awareness: DpiAwareness,
    /// Override scale in percent (0 = use system default).
    pub override_percent: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DISPLAYS: usize = 16;
const MAX_OVERRIDES: usize = 100;

struct State {
    displays: Vec<DisplayScale>,
    overrides: Vec<AppDpiOverride>,
    next_id: u32,
    global_method: ScalingMethod,
    total_changes: u64,
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

fn recommended_scale(physical_dpi: u32) -> u32 {
    if physical_dpi <= 96 { 100 }
    else if physical_dpi <= 120 { 125 }
    else if physical_dpi <= 144 { 150 }
    else if physical_dpi <= 192 { 200 }
    else if physical_dpi <= 288 { 300 }
    else { 400 }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let display = DisplayScale {
        id: 1, display_name: String::from("Primary Display"),
        scale_percent: 100, recommended_percent: 100,
        method: ScalingMethod::Integer,
        physical_dpi: 96, effective_dpi: 96,
    };

    *guard = Some(State {
        displays: alloc::vec![display],
        overrides: Vec::new(),
        next_id: 2,
        global_method: ScalingMethod::Fractional,
        total_changes: 0,
        ops: 0,
    });
}

/// Register a display with its physical DPI.
pub fn register_display(name: &str, physical_dpi: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.displays.len() >= MAX_DISPLAYS {
            return Err(KernelError::ResourceExhausted);
        }
        let rec = recommended_scale(physical_dpi);
        let id = state.next_id;
        state.next_id += 1;
        state.displays.push(DisplayScale {
            id, display_name: String::from(name),
            scale_percent: rec, recommended_percent: rec,
            method: state.global_method,
            physical_dpi,
            effective_dpi: physical_dpi * rec / 100,
        });
        Ok(id)
    })
}

/// Set scaling for a display.
pub fn set_scale(display_id: u32, percent: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.scale_percent = percent.clamp(50, 500);
        d.effective_dpi = d.physical_dpi * d.scale_percent / 100;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set scaling method for a display.
pub fn set_method(display_id: u32, method: ScalingMethod) -> KernelResult<()> {
    with_state(|state| {
        let d = state.displays.iter_mut().find(|d| d.id == display_id)
            .ok_or(KernelError::NotFound)?;
        d.method = method;
        Ok(())
    })
}

/// Set global scaling method.
pub fn set_global_method(method: ScalingMethod) -> KernelResult<()> {
    with_state(|state| {
        state.global_method = method;
        Ok(())
    })
}

/// Add app DPI override.
pub fn set_app_override(app_name: &str, awareness: DpiAwareness, override_percent: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(existing) = state.overrides.iter_mut().find(|o| o.app_name == app_name) {
            existing.awareness = awareness;
            existing.override_percent = override_percent;
        } else {
            if state.overrides.len() >= MAX_OVERRIDES {
                return Err(KernelError::ResourceExhausted);
            }
            state.overrides.push(AppDpiOverride {
                app_name: String::from(app_name),
                awareness, override_percent,
            });
        }
        Ok(())
    })
}

/// Remove app DPI override.
pub fn remove_app_override(app_name: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.overrides.iter().position(|o| o.app_name == app_name)
            .ok_or(KernelError::NotFound)?;
        state.overrides.remove(pos);
        Ok(())
    })
}

/// List all displays.
pub fn list_displays() -> Vec<DisplayScale> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.displays.clone())
}

/// Get display scale.
pub fn get_display(id: u32) -> KernelResult<DisplayScale> {
    with_state(|state| {
        state.displays.iter().find(|d| d.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List app overrides.
pub fn list_overrides() -> Vec<AppDpiOverride> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.overrides.clone())
}

/// Statistics: (display_count, override_count, total_changes, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.displays.len(), s.overrides.len(), s.total_changes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("dpiscaling::self_test() — running tests...");
    init_defaults();

    // 1: Default display at 100%.
    let displays = list_displays();
    assert_eq!(displays.len(), 1);
    assert_eq!(displays[0].scale_percent, 100);
    assert_eq!(displays[0].physical_dpi, 96);
    crate::serial_println!("  [1/8] default display: OK");

    // 2: Set scale.
    set_scale(1, 150).expect("scale");
    let d = get_display(1).expect("get");
    assert_eq!(d.scale_percent, 150);
    assert_eq!(d.effective_dpi, 144); // 96 * 150 / 100
    crate::serial_println!("  [2/8] set scale: OK");

    // 3: Register HiDPI display.
    let id2 = register_display("4K Monitor", 192).expect("reg");
    let d2 = get_display(id2).expect("get2");
    assert_eq!(d2.recommended_percent, 200);
    assert_eq!(d2.scale_percent, 200);
    crate::serial_println!("  [3/8] HiDPI register: OK");

    // 4: Clamp scale.
    set_scale(1, 1000).expect("clamp");
    let d = get_display(1).expect("get3");
    assert_eq!(d.scale_percent, 500);
    crate::serial_println!("  [4/8] clamp scale: OK");

    // 5: Scaling method.
    set_method(1, ScalingMethod::IntegerThenResize).expect("method");
    let d = get_display(1).expect("get4");
    assert_eq!(d.method, ScalingMethod::IntegerThenResize);
    crate::serial_println!("  [5/8] scaling method: OK");

    // 6: App override.
    set_app_override("legacy.exe", DpiAwareness::Unaware, 200).expect("override");
    let overrides = list_overrides();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].awareness, DpiAwareness::Unaware);
    crate::serial_println!("  [6/8] app override: OK");

    // 7: Remove override.
    remove_app_override("legacy.exe").expect("remove");
    assert_eq!(list_overrides().len(), 0);
    crate::serial_println!("  [7/8] remove override: OK");

    // 8: Stats.
    let (displays, overrides, changes, ops) = stats();
    assert_eq!(displays, 2);
    assert_eq!(overrides, 0);
    assert!(changes >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("dpiscaling::self_test() — all 8 tests passed");
}

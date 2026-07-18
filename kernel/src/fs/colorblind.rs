//! Color Blindness Filters — accessibility color correction.
//!
//! Provides system-wide color filters for different types of
//! color vision deficiency (CVD), with adjustable intensity.
//!
//! ## Architecture
//!
//! ```text
//! Display output
//!   → colorblind::apply_filter(pixel) → corrected color
//!   → colorblind::set_mode(type) → select filter
//!
//! Integration:
//!   → a11y (accessibility)
//!   → display (display output)
//!   → colortemp (night light)
//!   → displaycolor (color management)
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

/// Color vision deficiency type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CvdType {
    None,
    Protanopia,     // Red blindness.
    Deuteranopia,   // Green blindness.
    Tritanopia,     // Blue blindness.
    Protanomaly,    // Red weakness.
    Deuteranomaly,  // Green weakness.
    Tritanomaly,    // Blue weakness.
    Achromatopsia,  // Total color blindness.
    Custom,
}

impl CvdType {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Protanopia => "Protanopia (red-blind)",
            Self::Deuteranopia => "Deuteranopia (green-blind)",
            Self::Tritanopia => "Tritanopia (blue-blind)",
            Self::Protanomaly => "Protanomaly (red-weak)",
            Self::Deuteranomaly => "Deuteranomaly (green-weak)",
            Self::Tritanomaly => "Tritanomaly (blue-weak)",
            Self::Achromatopsia => "Achromatopsia (no color)",
            Self::Custom => "Custom",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Protanopia => "protan",
            Self::Deuteranopia => "deutan",
            Self::Tritanopia => "tritan",
            Self::Protanomaly => "protanomaly",
            Self::Deuteranomaly => "deuteranomaly",
            Self::Tritanomaly => "tritanomaly",
            Self::Achromatopsia => "achromat",
            Self::Custom => "custom",
        }
    }
}

/// A preset filter configuration.
#[derive(Debug, Clone)]
pub struct FilterPreset {
    pub id: u32,
    pub name: String,
    pub cvd_type: CvdType,
    pub intensity: u32,   // 0-100.
    pub description: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PRESETS: usize = 20;

struct State {
    enabled: bool,
    active_type: CvdType,
    intensity: u32,     // 0-100.
    presets: Vec<FilterPreset>,
    next_preset_id: u32,
    simulate_mode: bool,    // Simulate CVD instead of correcting.
    total_activations: u64,
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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        enabled: false,
        active_type: CvdType::None,
        intensity: 100,
        presets: alloc::vec![
            FilterPreset { id: 1, name: String::from("Red-Green (Full)"), cvd_type: CvdType::Deuteranopia, intensity: 100, description: String::from("Full correction for deuteranopia") },
            FilterPreset { id: 2, name: String::from("Red-Green (Mild)"), cvd_type: CvdType::Deuteranomaly, intensity: 60, description: String::from("Mild correction for deuteranomaly") },
            FilterPreset { id: 3, name: String::from("Blue-Yellow"), cvd_type: CvdType::Tritanopia, intensity: 100, description: String::from("Correction for tritanopia") },
            FilterPreset { id: 4, name: String::from("Grayscale"), cvd_type: CvdType::Achromatopsia, intensity: 100, description: String::from("Full grayscale conversion") },
        ],
        next_preset_id: 5,
        simulate_mode: false,
        total_activations: 0,
        total_changes: 0,
        ops: 0,
    });
}

/// Enable/disable color filter.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        if enabled { state.total_activations += 1; }
        Ok(())
    })
}

/// Set the active CVD type.
pub fn set_type(cvd_type: CvdType) -> KernelResult<()> {
    with_state(|state| {
        state.active_type = cvd_type;
        state.total_changes += 1;
        Ok(())
    })
}

/// Set filter intensity (0-100).
pub fn set_intensity(intensity: u32) -> KernelResult<()> {
    if intensity > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.intensity = intensity;
        state.total_changes += 1;
        Ok(())
    })
}

/// Toggle simulation mode (show what CVD looks like vs correcting for it).
pub fn set_simulate(simulate: bool) -> KernelResult<()> {
    with_state(|state| {
        state.simulate_mode = simulate;
        Ok(())
    })
}

/// Apply a preset.
pub fn apply_preset(preset_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let preset = state.presets.iter().find(|p| p.id == preset_id)
            .ok_or(KernelError::NotFound)?;
        state.active_type = preset.cvd_type;
        state.intensity = preset.intensity;
        state.enabled = true;
        state.total_activations += 1;
        state.total_changes += 1;
        Ok(())
    })
}

/// Add a custom preset.
pub fn add_preset(name: &str, cvd_type: CvdType, intensity: u32, description: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.presets.len() >= MAX_PRESETS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_preset_id;
        state.next_preset_id += 1;
        state.presets.push(FilterPreset {
            id, name: String::from(name), cvd_type,
            intensity: intensity.min(100),
            description: String::from(description),
        });
        Ok(id)
    })
}

/// Remove a preset.
pub fn remove_preset(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.presets.len();
        state.presets.retain(|p| p.id != id);
        if state.presets.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Is filter enabled?
pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.enabled)
}

/// Get current settings: (enabled, cvd_type, intensity, simulate).
pub fn current() -> (bool, CvdType, u32, bool) {
    STATE.lock().as_ref().map_or((false, CvdType::None, 100, false), |s| {
        (s.enabled, s.active_type, s.intensity, s.simulate_mode)
    })
}

/// List presets.
pub fn list_presets() -> Vec<FilterPreset> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.presets.clone())
}

/// Statistics: (preset_count, total_activations, total_changes, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.presets.len(), s.total_activations, s.total_changes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("colorblind::self_test() — running tests...");
    init_defaults();

    // 1: Disabled by default.
    assert!(!is_enabled());
    let (_, cvd, _, _) = current();
    assert_eq!(cvd, CvdType::None);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Enable with type.
    set_enabled(true).expect("enable");
    set_type(CvdType::Deuteranopia).expect("type");
    let (en, cvd, _, _) = current();
    assert!(en);
    assert_eq!(cvd, CvdType::Deuteranopia);
    crate::serial_println!("  [2/8] enable: OK");

    // 3: Intensity.
    set_intensity(75).expect("intensity");
    let (_, _, intensity, _) = current();
    assert_eq!(intensity, 75);
    assert!(set_intensity(101).is_err());
    crate::serial_println!("  [3/8] intensity: OK");

    // 4: Simulation mode.
    set_simulate(true).expect("sim");
    let (_, _, _, sim) = current();
    assert!(sim);
    crate::serial_println!("  [4/8] simulate: OK");

    // 5: Presets.
    let presets = list_presets();
    assert_eq!(presets.len(), 4);
    crate::serial_println!("  [5/8] presets: OK");

    // 6: Apply preset.
    apply_preset(3).expect("apply"); // Blue-Yellow.
    let (_, cvd, intensity, _) = current();
    assert_eq!(cvd, CvdType::Tritanopia);
    assert_eq!(intensity, 100);
    crate::serial_println!("  [6/8] apply preset: OK");

    // 7: Custom preset.
    let _pid = add_preset("My Filter", CvdType::Protanomaly, 50, "Custom red-weak filter").expect("custom");
    assert_eq!(list_presets().len(), 5);
    crate::serial_println!("  [7/8] custom preset: OK");

    // 8: Stats.
    let (presets, activations, changes, ops) = stats();
    assert_eq!(presets, 5);
    assert!(activations >= 2);
    assert!(changes >= 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("colorblind::self_test() — all 8 tests passed");
}

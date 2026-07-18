//! Display Calibration — monitor color calibration and profiling.
//!
//! Manages display color profiles, calibration data, and gamma
//! curves for accurate color reproduction across monitors.
//!
//! ## Architecture
//!
//! ```text
//! Display calibration
//!   → displaycal::set_profile(monitor, profile) → apply ICC profile
//!   → displaycal::adjust_gamma(monitor, gamma) → set gamma curve
//!   → displaycal::calibrate(monitor) → run calibration
//!
//! Integration:
//!   → displaycolor (display color management)
//!   → monitors (monitor configuration)
//!   → displayarrange (display arrangement)
//!   → hdrdisplay (HDR settings)
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

/// Color profile type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileType {
    Srgb,
    AdobeRgb,
    DciP3,
    Rec2020,
    Custom,
}

impl ProfileType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Srgb => "sRGB",
            Self::AdobeRgb => "Adobe RGB",
            Self::DciP3 => "DCI-P3",
            Self::Rec2020 => "Rec.2020",
            Self::Custom => "Custom",
        }
    }
}

/// Calibration status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationStatus {
    NotCalibrated,
    Calibrated,
    NeedsRecalibration,
    InProgress,
    Failed,
}

impl CalibrationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotCalibrated => "Not Calibrated",
            Self::Calibrated => "Calibrated",
            Self::NeedsRecalibration => "Needs Recalibration",
            Self::InProgress => "In Progress",
            Self::Failed => "Failed",
        }
    }
}

/// A monitor's calibration data.
#[derive(Debug, Clone)]
pub struct MonitorCalibration {
    pub id: u32,
    pub monitor_name: String,
    pub profile_type: ProfileType,
    pub status: CalibrationStatus,
    pub gamma_r: u32,  // Gamma * 100 (e.g., 220 = 2.20).
    pub gamma_g: u32,
    pub gamma_b: u32,
    pub brightness_target: u32, // cd/m².
    pub white_point_k: u32,     // Color temperature in Kelvin.
    pub calibrated_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_MONITORS: usize = 16;

struct State {
    monitors: Vec<MonitorCalibration>,
    next_id: u32,
    total_calibrations: u64,
    total_profile_changes: u64,
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
        monitors: alloc::vec![
            MonitorCalibration {
                id: 1, monitor_name: String::from("Primary Display"),
                profile_type: ProfileType::Srgb, status: CalibrationStatus::NotCalibrated,
                gamma_r: 220, gamma_g: 220, gamma_b: 220,
                brightness_target: 120, white_point_k: 6500, calibrated_ns: 0,
            },
        ],
        next_id: 2,
        total_calibrations: 0,
        total_profile_changes: 0,
        ops: 0,
    });
}

/// Add a monitor for calibration.
pub fn add_monitor(name: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.monitors.len() >= MAX_MONITORS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.monitors.push(MonitorCalibration {
            id, monitor_name: String::from(name),
            profile_type: ProfileType::Srgb, status: CalibrationStatus::NotCalibrated,
            gamma_r: 220, gamma_g: 220, gamma_b: 220,
            brightness_target: 120, white_point_k: 6500, calibrated_ns: 0,
        });
        Ok(id)
    })
}

/// Remove a monitor.
pub fn remove_monitor(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.monitors.len();
        state.monitors.retain(|m| m.id != id);
        if state.monitors.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Set color profile.
pub fn set_profile(id: u32, profile: ProfileType) -> KernelResult<()> {
    with_state(|state| {
        let mon = state.monitors.iter_mut().find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        mon.profile_type = profile;
        state.total_profile_changes += 1;
        Ok(())
    })
}

/// Set gamma for a monitor (values are gamma * 100, e.g., 220 = 2.20).
pub fn set_gamma(id: u32, r: u32, g: u32, b: u32) -> KernelResult<()> {
    with_state(|state| {
        let mon = state.monitors.iter_mut().find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        mon.gamma_r = r;
        mon.gamma_g = g;
        mon.gamma_b = b;
        Ok(())
    })
}

/// Set white point (color temperature in Kelvin).
pub fn set_white_point(id: u32, kelvin: u32) -> KernelResult<()> {
    with_state(|state| {
        let mon = state.monitors.iter_mut().find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        mon.white_point_k = kelvin;
        Ok(())
    })
}

/// Set brightness target (cd/m²).
pub fn set_brightness_target(id: u32, cdm2: u32) -> KernelResult<()> {
    with_state(|state| {
        let mon = state.monitors.iter_mut().find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        mon.brightness_target = cdm2;
        Ok(())
    })
}

/// Run calibration (simulate).
pub fn calibrate(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let mon = state.monitors.iter_mut().find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        mon.status = CalibrationStatus::Calibrated;
        mon.calibrated_ns = now;
        state.total_calibrations += 1;
        Ok(())
    })
}

/// Get monitor calibration.
pub fn get_monitor(id: u32) -> Option<MonitorCalibration> {
    STATE.lock().as_ref().and_then(|s| s.monitors.iter().find(|m| m.id == id).cloned())
}

/// List all monitors.
pub fn list_monitors() -> Vec<MonitorCalibration> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.monitors.clone())
}

/// Statistics: (monitor_count, total_calibrations, total_profile_changes, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.monitors.len(), s.total_calibrations, s.total_profile_changes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("displaycal::self_test() — running tests...");
    init_defaults();

    // 1: Default monitor.
    assert_eq!(list_monitors().len(), 1);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Add monitor.
    let id = add_monitor("External 4K").expect("add");
    assert_eq!(list_monitors().len(), 2);
    crate::serial_println!("  [2/8] add monitor: OK");

    // 3: Set profile.
    set_profile(id, ProfileType::DciP3).expect("profile");
    let mon = get_monitor(id).expect("get");
    assert_eq!(mon.profile_type, ProfileType::DciP3);
    crate::serial_println!("  [3/8] set profile: OK");

    // 4: Set gamma.
    set_gamma(id, 240, 230, 220).expect("gamma");
    let mon = get_monitor(id).expect("get2");
    assert_eq!(mon.gamma_r, 240);
    assert_eq!(mon.gamma_g, 230);
    crate::serial_println!("  [4/8] set gamma: OK");

    // 5: Set white point.
    set_white_point(id, 5000).expect("wp");
    let mon = get_monitor(id).expect("get3");
    assert_eq!(mon.white_point_k, 5000);
    crate::serial_println!("  [5/8] white point: OK");

    // 6: Calibrate.
    calibrate(id).expect("calibrate");
    let mon = get_monitor(id).expect("get4");
    assert_eq!(mon.status, CalibrationStatus::Calibrated);
    assert!(mon.calibrated_ns > 0);
    crate::serial_println!("  [6/8] calibrate: OK");

    // 7: Remove monitor.
    remove_monitor(id).expect("remove");
    assert_eq!(list_monitors().len(), 1);
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (count, calibrations, profile_changes, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(calibrations, 1);
    assert_eq!(profile_changes, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("displaycal::self_test() — all 8 tests passed");
}

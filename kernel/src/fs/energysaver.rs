//! Energy Saver — system power optimization.
//!
//! Manages power-saving features including app throttling,
//! display dimming schedules, and battery life estimation.
//!
//! ## Architecture
//!
//! ```text
//! Power optimization
//!   → energysaver::set_mode(mode) → apply power profile
//!   → energysaver::throttle_app(app) → limit background app
//!   → energysaver::estimate_remaining() → battery life prediction
//!
//! Integration:
//!   → power (power management)
//!   → powerprofile (power profiles)
//!   → battery (battery status)
//!   → brightness (display brightness)
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

/// Energy saver mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyMode {
    Performance,
    Balanced,
    PowerSaver,
    UltraSaver,
    Custom,
}

impl EnergyMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::Balanced => "Balanced",
            Self::PowerSaver => "Power Saver",
            Self::UltraSaver => "Ultra Saver",
            Self::Custom => "Custom",
        }
    }
}

/// App throttle state.
#[derive(Debug, Clone)]
pub struct ThrottledApp {
    pub name: String,
    pub cpu_limit_pct: u32,
    pub network_allowed: bool,
    pub background_allowed: bool,
    pub applied_ns: u64,
}

/// Energy profile settings.
#[derive(Debug, Clone)]
pub struct EnergyProfile {
    pub mode: EnergyMode,
    pub display_brightness_pct: u32,
    pub display_timeout_sec: u32,
    pub cpu_max_freq_pct: u32,
    pub wifi_power_save: bool,
    pub bluetooth_off: bool,
    pub reduce_animations: bool,
    pub background_app_limit: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_THROTTLED: usize = 100;

struct State {
    active_mode: EnergyMode,
    profile: EnergyProfile,
    throttled_apps: Vec<ThrottledApp>,
    auto_switch_enabled: bool,
    auto_switch_threshold_pct: u32,
    estimated_minutes: u64,
    total_mode_changes: u64,
    total_throttles: u64,
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

fn default_profile(mode: EnergyMode) -> EnergyProfile {
    match mode {
        EnergyMode::Performance => EnergyProfile {
            mode, display_brightness_pct: 100, display_timeout_sec: 600,
            cpu_max_freq_pct: 100, wifi_power_save: false, bluetooth_off: false,
            reduce_animations: false, background_app_limit: 0,
        },
        EnergyMode::Balanced => EnergyProfile {
            mode, display_brightness_pct: 70, display_timeout_sec: 300,
            cpu_max_freq_pct: 80, wifi_power_save: true, bluetooth_off: false,
            reduce_animations: false, background_app_limit: 20,
        },
        EnergyMode::PowerSaver => EnergyProfile {
            mode, display_brightness_pct: 40, display_timeout_sec: 120,
            cpu_max_freq_pct: 60, wifi_power_save: true, bluetooth_off: true,
            reduce_animations: true, background_app_limit: 5,
        },
        EnergyMode::UltraSaver => EnergyProfile {
            mode, display_brightness_pct: 20, display_timeout_sec: 30,
            cpu_max_freq_pct: 40, wifi_power_save: true, bluetooth_off: true,
            reduce_animations: true, background_app_limit: 2,
        },
        EnergyMode::Custom => EnergyProfile {
            mode, display_brightness_pct: 70, display_timeout_sec: 300,
            cpu_max_freq_pct: 80, wifi_power_save: false, bluetooth_off: false,
            reduce_animations: false, background_app_limit: 10,
        },
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        active_mode: EnergyMode::Balanced,
        profile: default_profile(EnergyMode::Balanced),
        throttled_apps: Vec::new(),
        auto_switch_enabled: true,
        auto_switch_threshold_pct: 20,
        estimated_minutes: 0,
        total_mode_changes: 0,
        total_throttles: 0,
        ops: 0,
    });
}

/// Set energy mode.
pub fn set_mode(mode: EnergyMode) -> KernelResult<()> {
    with_state(|state| {
        state.active_mode = mode;
        state.profile = default_profile(mode);
        state.total_mode_changes += 1;
        Ok(())
    })
}

/// Get current mode.
pub fn get_mode() -> EnergyMode {
    STATE.lock().as_ref().map_or(EnergyMode::Balanced, |s| s.active_mode)
}

/// Get current profile.
pub fn get_profile() -> Option<EnergyProfile> {
    STATE.lock().as_ref().map(|s| s.profile.clone())
}

/// Set a custom profile setting.
pub fn set_brightness(pct: u32) -> KernelResult<()> {
    with_state(|state| { state.profile.display_brightness_pct = pct.min(100); Ok(()) })
}

pub fn set_cpu_limit(pct: u32) -> KernelResult<()> {
    with_state(|state| { state.profile.cpu_max_freq_pct = pct.min(100); Ok(()) })
}

pub fn set_display_timeout(sec: u32) -> KernelResult<()> {
    with_state(|state| { state.profile.display_timeout_sec = sec; Ok(()) })
}

/// Throttle a background app.
pub fn throttle_app(name: &str, cpu_limit_pct: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(app) = state.throttled_apps.iter_mut().find(|a| a.name == name) {
            app.cpu_limit_pct = cpu_limit_pct;
            app.applied_ns = now;
        } else {
            if state.throttled_apps.len() >= MAX_THROTTLED {
                return Err(KernelError::ResourceExhausted);
            }
            state.throttled_apps.push(ThrottledApp {
                name: String::from(name), cpu_limit_pct,
                network_allowed: true, background_allowed: true, applied_ns: now,
            });
        }
        state.total_throttles += 1;
        Ok(())
    })
}

/// Remove app throttle.
pub fn unthrottle_app(name: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.throttled_apps.len();
        state.throttled_apps.retain(|a| a.name != name);
        if state.throttled_apps.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Set estimated battery remaining.
pub fn set_estimate(minutes: u64) -> KernelResult<()> {
    with_state(|state| { state.estimated_minutes = minutes; Ok(()) })
}

/// Set auto-switch settings.
pub fn set_auto_switch(enabled: bool, threshold_pct: u32) -> KernelResult<()> {
    with_state(|state| {
        state.auto_switch_enabled = enabled;
        state.auto_switch_threshold_pct = threshold_pct.min(100);
        Ok(())
    })
}

/// List throttled apps.
pub fn list_throttled() -> Vec<ThrottledApp> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.throttled_apps.clone())
}

/// Statistics: (throttled_count, mode_changes, total_throttles, estimated_min, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.throttled_apps.len(), s.total_mode_changes, s.total_throttles, s.estimated_minutes, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("energysaver::self_test() — running tests...");
    init_defaults();

    // 1: Default balanced mode.
    assert_eq!(get_mode(), EnergyMode::Balanced);
    crate::serial_println!("  [1/8] default: OK");

    // 2: Switch to power saver.
    set_mode(EnergyMode::PowerSaver).expect("mode");
    assert_eq!(get_mode(), EnergyMode::PowerSaver);
    let p = get_profile().expect("profile");
    assert_eq!(p.display_brightness_pct, 40);
    assert!(p.reduce_animations);
    crate::serial_println!("  [2/8] power saver: OK");

    // 3: Custom brightness.
    set_brightness(30).expect("bright");
    let p = get_profile().expect("profile2");
    assert_eq!(p.display_brightness_pct, 30);
    crate::serial_println!("  [3/8] custom setting: OK");

    // 4: Throttle app.
    throttle_app("game", 25).expect("throttle");
    let throttled = list_throttled();
    assert_eq!(throttled.len(), 1);
    assert_eq!(throttled[0].cpu_limit_pct, 25);
    crate::serial_println!("  [4/8] throttle: OK");

    // 5: Update throttle.
    throttle_app("game", 10).expect("rethrottle");
    let throttled = list_throttled();
    assert_eq!(throttled[0].cpu_limit_pct, 10);
    crate::serial_println!("  [5/8] update throttle: OK");

    // 6: Unthrottle.
    unthrottle_app("game").expect("unthrottle");
    assert!(list_throttled().is_empty());
    crate::serial_println!("  [6/8] unthrottle: OK");

    // 7: Estimate.
    set_estimate(180).expect("estimate");
    let (_, _, _, est, _) = stats();
    assert_eq!(est, 180);
    crate::serial_println!("  [7/8] estimate: OK");

    // 8: Stats.
    let (throttled, mode_changes, total_throttles, _, ops) = stats();
    assert_eq!(throttled, 0);
    assert_eq!(mode_changes, 1);
    assert_eq!(total_throttles, 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("energysaver::self_test() — all 8 tests passed");
}

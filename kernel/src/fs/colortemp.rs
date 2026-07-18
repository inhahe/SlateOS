//! Color Temperature — blue light reduction scheduling.
//!
//! Manages adaptive color temperature adjustments based on time-of-day
//! schedules, with configurable Kelvin ranges and transition curves.
//! More advanced than nightlight, providing full circadian rhythm support.
//!
//! ## Architecture
//!
//! ```text
//! Time-based trigger
//!   → colortemp::update(current_time) → adjusts temperature
//!   → colortemp::set_schedule(start, end, kelvin)
//!
//! Integration:
//!   → nightlight (simple on/off toggle)
//!   → displaycolor (color calibration)
//!   → hdrdisplay (HDR color space)
//!   → brightness (backlight interaction)
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

/// Color temperature mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempMode {
    /// Disabled — no color adjustment.
    Off,
    /// Manual — fixed temperature.
    Manual,
    /// Scheduled — time-based transitions.
    Scheduled,
    /// Location-based (sunrise/sunset).
    SunSync,
}

impl TempMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Manual => "Manual",
            Self::Scheduled => "Scheduled",
            Self::SunSync => "Sun Sync",
        }
    }
}

/// A scheduled transition point.
#[derive(Debug, Clone)]
pub struct SchedulePoint {
    /// Time in minutes from midnight (0-1439).
    pub time_min: u16,
    /// Target temperature in Kelvin.
    pub kelvin: u32,
}

/// Color temperature profile.
#[derive(Debug, Clone)]
pub struct TempProfile {
    pub id: u32,
    pub name: String,
    pub mode: TempMode,
    /// Current effective temperature in Kelvin (1000-10000).
    pub current_kelvin: u32,
    /// Day temperature (for scheduled mode).
    pub day_kelvin: u32,
    /// Night temperature (for scheduled mode).
    pub night_kelvin: u32,
    /// Transition start in minutes from midnight.
    pub sunset_min: u16,
    /// Transition end in minutes from midnight.
    pub sunrise_min: u16,
    /// Transition duration in minutes.
    pub transition_min: u16,
    /// Custom schedule points.
    pub schedule: Vec<SchedulePoint>,
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROFILES: usize = 8;

struct State {
    profiles: Vec<TempProfile>,
    next_id: u32,
    active_profile_id: u32,
    total_adjustments: u64,
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

    let profile = TempProfile {
        id: 1, name: String::from("Default"),
        mode: TempMode::Off,
        current_kelvin: 6500,
        day_kelvin: 6500, night_kelvin: 3400,
        sunset_min: 1200, // 20:00
        sunrise_min: 420,  // 07:00
        transition_min: 30,
        schedule: Vec::new(),
        enabled: false,
    };

    *guard = Some(State {
        profiles: alloc::vec![profile],
        next_id: 2,
        active_profile_id: 1,
        total_adjustments: 0,
        ops: 0,
    });
}

/// Create a new profile.
pub fn create_profile(name: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.profiles.len() >= MAX_PROFILES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.profiles.push(TempProfile {
            id, name: String::from(name),
            mode: TempMode::Off,
            current_kelvin: 6500,
            day_kelvin: 6500, night_kelvin: 3400,
            sunset_min: 1200, sunrise_min: 420,
            transition_min: 30,
            schedule: Vec::new(),
            enabled: false,
        });
        Ok(id)
    })
}

/// Set the mode for a profile.
pub fn set_mode(profile_id: u32, mode: TempMode) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.mode = mode;
        p.enabled = mode != TempMode::Off;
        Ok(())
    })
}

/// Set manual temperature.
pub fn set_temperature(profile_id: u32, kelvin: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.current_kelvin = kelvin.clamp(1000, 10000);
        p.mode = TempMode::Manual;
        p.enabled = true;
        state.total_adjustments += 1;
        Ok(())
    })
}

/// Set day/night temperatures for scheduled mode.
pub fn set_day_night(profile_id: u32, day_kelvin: u32, night_kelvin: u32) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.day_kelvin = day_kelvin.clamp(1000, 10000);
        p.night_kelvin = night_kelvin.clamp(1000, 10000);
        Ok(())
    })
}

/// Set schedule times (minutes from midnight).
pub fn set_schedule_times(profile_id: u32, sunset_min: u16, sunrise_min: u16, transition_min: u16) -> KernelResult<()> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        p.sunset_min = sunset_min.min(1439);
        p.sunrise_min = sunrise_min.min(1439);
        p.transition_min = transition_min.clamp(1, 120);
        Ok(())
    })
}

/// Update color temperature based on current time (minutes from midnight).
pub fn update_for_time(profile_id: u32, current_min: u16) -> KernelResult<u32> {
    with_state(|state| {
        let p = state.profiles.iter_mut().find(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        if !p.enabled || p.mode == TempMode::Off {
            return Ok(p.current_kelvin);
        }
        if p.mode == TempMode::Manual {
            return Ok(p.current_kelvin);
        }
        // Scheduled/SunSync: interpolate between day and night temperatures.
        let sunset = p.sunset_min;
        let sunrise = p.sunrise_min;
        let trans = p.transition_min;
        let day_k = p.day_kelvin;
        let night_k = p.night_kelvin;
        let current = current_min.min(1439);

        // Determine if we're in day, night, or transition.
        let kelvin = if sunrise < sunset {
            // Normal: sunrise in morning, sunset in evening.
            if current >= sunrise + trans && current < sunset {
                day_k
            } else if current >= sunset + trans || current < sunrise {
                night_k
            } else if current >= sunset && current < sunset + trans {
                // Evening transition: day → night.
                let elapsed = (current - sunset) as u32;
                let total = trans as u32;
                day_k - ((day_k - night_k) * elapsed / total.max(1))
            } else {
                // Morning transition: night → day.
                let elapsed = (current - sunrise) as u32;
                let total = trans as u32;
                night_k + ((day_k - night_k) * elapsed / total.max(1))
            }
        } else {
            // Wrapped: sunset before midnight, sunrise after.
            if current >= sunrise + trans && current < sunset {
                day_k
            } else if current >= sunset + trans || current < sunrise {
                night_k
            } else {
                // Approximate: use night temp for simplicity.
                night_k
            }
        };

        p.current_kelvin = kelvin.clamp(1000, 10000);
        state.total_adjustments += 1;
        Ok(p.current_kelvin)
    })
}

/// Set active profile.
pub fn set_active(profile_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.profiles.iter().any(|p| p.id == profile_id) {
            return Err(KernelError::NotFound);
        }
        state.active_profile_id = profile_id;
        Ok(())
    })
}

/// Remove a profile.
pub fn remove_profile(profile_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.profiles.iter().position(|p| p.id == profile_id)
            .ok_or(KernelError::NotFound)?;
        state.profiles.remove(pos);
        if state.active_profile_id == profile_id {
            state.active_profile_id = state.profiles.first().map_or(0, |p| p.id);
        }
        Ok(())
    })
}

/// List all profiles.
pub fn list_profiles() -> Vec<TempProfile> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.profiles.clone())
}

/// Get a profile.
pub fn get_profile(id: u32) -> KernelResult<TempProfile> {
    with_state(|state| {
        state.profiles.iter().find(|p| p.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Active profile ID.
pub fn active_profile_id() -> u32 {
    STATE.lock().as_ref().map_or(0, |s| s.active_profile_id)
}

/// Statistics: (profile_count, active_id, total_adjustments, ops).
pub fn stats() -> (usize, u32, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.profiles.len(), s.active_profile_id, s.total_adjustments, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("colortemp::self_test() — running tests...");
    init_defaults();

    // 1: Default profile.
    let profiles = list_profiles();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].mode, TempMode::Off);
    assert_eq!(profiles[0].current_kelvin, 6500);
    crate::serial_println!("  [1/8] default profile: OK");

    // 2: Set manual temperature.
    set_temperature(1, 4000).expect("temp");
    let p = get_profile(1).expect("get");
    assert_eq!(p.current_kelvin, 4000);
    assert_eq!(p.mode, TempMode::Manual);
    crate::serial_println!("  [2/8] manual temp: OK");

    // 3: Set scheduled mode.
    set_mode(1, TempMode::Scheduled).expect("mode");
    set_day_night(1, 6500, 3400).expect("daynight");
    let p = get_profile(1).expect("get2");
    assert_eq!(p.day_kelvin, 6500);
    assert_eq!(p.night_kelvin, 3400);
    crate::serial_println!("  [3/8] scheduled mode: OK");

    // 4: Update for daytime → day temperature.
    set_schedule_times(1, 1200, 420, 30).expect("times");
    let k = update_for_time(1, 720).expect("update_day"); // noon
    assert_eq!(k, 6500);
    crate::serial_println!("  [4/8] daytime update: OK");

    // 5: Update for nighttime → night temperature.
    let k = update_for_time(1, 100).expect("update_night"); // 1:40 AM
    assert_eq!(k, 3400);
    crate::serial_println!("  [5/8] nighttime update: OK");

    // 6: Clamp out-of-range temperature.
    set_temperature(1, 500).expect("low");
    let p = get_profile(1).expect("get3");
    assert_eq!(p.current_kelvin, 1000);
    set_temperature(1, 20000).expect("high");
    let p = get_profile(1).expect("get4");
    assert_eq!(p.current_kelvin, 10000);
    crate::serial_println!("  [6/8] clamp kelvin: OK");

    // 7: Create and set active.
    let id2 = create_profile("Night Owl").expect("create");
    set_active(id2).expect("active");
    assert_eq!(active_profile_id(), id2);
    crate::serial_println!("  [7/8] create/active: OK");

    // 8: Stats.
    let (count, active, adj, ops) = stats();
    assert_eq!(count, 2);
    assert_eq!(active, id2);
    assert!(adj >= 4);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("colortemp::self_test() — all 8 tests passed");
}

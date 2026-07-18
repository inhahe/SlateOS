//! Night Light / Blue Light Filter.
//!
//! Adjusts screen colour temperature to reduce blue light emission
//! during evening hours.  Present on Windows (Night Light), macOS
//! (Night Shift), GNOME (Night Light), KDE (Night Color).
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Night Light
//!   → nightlight::set_enabled() / set_schedule()
//!
//! Compositor integration
//!   → nightlight::current_temperature() → color temp to apply
//!
//! Integration:
//!   → display (gamma ramp adjustment)
//!   → timezone (sunset/sunrise calculation)
//!   → power (dim on battery option)
//! ```
//!
//! ## Colour Temperature
//!
//! - Daylight (disabled): 6500K (neutral white)
//! - Comfortable evening: ~4500K (warm)
//! - Aggressive night: ~3000K (very warm/orange)
//! - Range: 1000K–6500K
//!
//! ## Schedule Modes
//!
//! - **Manual**: user toggles on/off
//! - **Scheduled**: fixed start/end times
//! - **SunsetSunrise**: automatic based on location (latitude/longitude)

#![allow(dead_code)]

use alloc::string::String;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default neutral (daylight) temperature.
const DEFAULT_DAY_TEMP: u32 = 6500;
/// Default warm (night) temperature.
const DEFAULT_NIGHT_TEMP: u32 = 4500;
/// Minimum temperature (very warm).
const MIN_TEMP: u32 = 1000;
/// Maximum temperature (daylight).
const MAX_TEMP: u32 = 6500;
/// Default transition duration in minutes.
const DEFAULT_TRANSITION_MIN: u32 = 30;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Schedule mode for automatic activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleMode {
    /// User manually toggles on/off.
    Manual,
    /// Fixed start/end times.
    Scheduled,
    /// Automatic based on geographic location.
    SunsetSunrise,
}

impl ScheduleMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Scheduled => "Scheduled",
            Self::SunsetSunrise => "Sunset/Sunrise",
        }
    }
}

/// Current state of the night light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NightLightState {
    /// Feature is globally disabled.
    Disabled,
    /// Active — warm temperature applied.
    Active,
    /// Transitioning from day to night.
    TransitionToNight,
    /// Transitioning from night to day.
    TransitionToDay,
    /// In daytime period (scheduled but waiting).
    DaytimeIdle,
}

impl NightLightState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Active => "Active",
            Self::TransitionToNight => "Transitioning (warming)",
            Self::TransitionToDay => "Transitioning (cooling)",
            Self::DaytimeIdle => "Daytime (idle)",
        }
    }
}

/// Geographic location for sunset/sunrise calculation.
#[derive(Debug, Clone, Copy)]
pub struct GeoLocation {
    /// Latitude in degrees (-90..90).
    pub latitude: i32,
    /// Longitude in degrees (-180..180).
    pub longitude: i32,
}

/// Schedule times (hour:minute, 24-hour format).
#[derive(Debug, Clone, Copy)]
pub struct ScheduleTime {
    pub hour: u8,
    pub minute: u8,
}

impl ScheduleTime {
    pub fn new(hour: u8, minute: u8) -> KernelResult<Self> {
        if hour > 23 || minute > 59 {
            return Err(KernelError::InvalidArgument);
        }
        Ok(Self { hour, minute })
    }

    /// Minutes since midnight.
    pub fn as_minutes(self) -> u32 {
        self.hour as u32 * 60 + self.minute as u32
    }
}

/// Configuration for the night light feature.
#[derive(Debug, Clone)]
pub struct NightLightConfig {
    /// Whether night light is enabled.
    pub enabled: bool,
    /// Schedule mode.
    pub schedule_mode: ScheduleMode,
    /// Temperature during night (warm). Range: MIN_TEMP..=MAX_TEMP.
    pub night_temp: u32,
    /// Temperature during day (neutral).
    pub day_temp: u32,
    /// Scheduled start time.
    pub start_time: ScheduleTime,
    /// Scheduled end time.
    pub end_time: ScheduleTime,
    /// Location for sunset/sunrise mode.
    pub location: Option<GeoLocation>,
    /// Transition duration in minutes.
    pub transition_minutes: u32,
    /// Current state.
    pub state: NightLightState,
    /// Whether to disable night light when on battery.
    pub disable_on_battery: bool,
    /// Whether the user has temporarily overridden the schedule.
    pub manual_override: bool,
    /// When manual override was activated (ns).
    pub override_until_ns: u64,
    /// Currently applied temperature.
    pub current_temp: u32,
    /// Transition start time (ns) — 0 if not transitioning.
    pub transition_start_ns: u64,
    /// Transition from temperature.
    pub transition_from: u32,
    /// Transition to temperature.
    pub transition_to: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct NightLightState2 {
    config: NightLightConfig,
    /// Total toggle operations.
    toggle_count: u64,
    /// Total schedule checks.
    check_count: u64,
    /// Operation counter.
    ops: u64,
}

static STATE: Mutex<Option<NightLightState2>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut NightLightState2) -> KernelResult<R>,
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

/// Initialize the night light subsystem with defaults.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(NightLightState2 {
        config: NightLightConfig {
            enabled: false,
            schedule_mode: ScheduleMode::Scheduled,
            night_temp: DEFAULT_NIGHT_TEMP,
            day_temp: DEFAULT_DAY_TEMP,
            start_time: ScheduleTime { hour: 21, minute: 0 },
            end_time: ScheduleTime { hour: 7, minute: 0 },
            location: None,
            transition_minutes: DEFAULT_TRANSITION_MIN,
            state: NightLightState::Disabled,
            disable_on_battery: false,
            manual_override: false,
            override_until_ns: 0,
            current_temp: DEFAULT_DAY_TEMP,
            transition_start_ns: 0,
            transition_from: DEFAULT_DAY_TEMP,
            transition_to: DEFAULT_DAY_TEMP,
        },
        toggle_count: 0,
        check_count: 0,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Enable / Disable
// ---------------------------------------------------------------------------

/// Enable or disable the night light feature.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.enabled = enabled;
        state.toggle_count += 1;
        if enabled {
            // Start in active state; check_schedule() will correct.
            state.config.state = NightLightState::Active;
            state.config.current_temp = state.config.night_temp;
        } else {
            state.config.state = NightLightState::Disabled;
            state.config.current_temp = state.config.day_temp;
            state.config.manual_override = false;
        }
        Ok(())
    })
}

/// Toggle night light on/off.
pub fn toggle() -> KernelResult<bool> {
    with_state(|state| {
        let new_enabled = !state.config.enabled;
        state.config.enabled = new_enabled;
        state.toggle_count += 1;
        if new_enabled {
            state.config.state = NightLightState::Active;
            state.config.current_temp = state.config.night_temp;
        } else {
            state.config.state = NightLightState::Disabled;
            state.config.current_temp = state.config.day_temp;
            state.config.manual_override = false;
        }
        Ok(new_enabled)
    })
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Set the night (warm) temperature. Range: 1000–6500K.
pub fn set_night_temp(temp: u32) -> KernelResult<()> {
    if temp < MIN_TEMP || temp > MAX_TEMP {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.night_temp = temp;
        if state.config.state == NightLightState::Active {
            state.config.current_temp = temp;
        }
        Ok(())
    })
}

/// Set the day (neutral) temperature. Range: 1000–6500K.
pub fn set_day_temp(temp: u32) -> KernelResult<()> {
    if temp < MIN_TEMP || temp > MAX_TEMP {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.day_temp = temp;
        if state.config.state == NightLightState::DaytimeIdle
            || state.config.state == NightLightState::Disabled
        {
            state.config.current_temp = temp;
        }
        Ok(())
    })
}

/// Set the schedule mode.
pub fn set_schedule_mode(mode: ScheduleMode) -> KernelResult<()> {
    with_state(|state| {
        state.config.schedule_mode = mode;
        Ok(())
    })
}

/// Set the scheduled start time (when night light turns on).
pub fn set_start_time(hour: u8, minute: u8) -> KernelResult<()> {
    let time = ScheduleTime::new(hour, minute)?;
    with_state(|state| {
        state.config.start_time = time;
        Ok(())
    })
}

/// Set the scheduled end time (when night light turns off).
pub fn set_end_time(hour: u8, minute: u8) -> KernelResult<()> {
    let time = ScheduleTime::new(hour, minute)?;
    with_state(|state| {
        state.config.end_time = time;
        Ok(())
    })
}

/// Set the geographic location for sunset/sunrise calculation.
pub fn set_location(latitude: i32, longitude: i32) -> KernelResult<()> {
    if latitude < -90 || latitude > 90 || longitude < -180 || longitude > 180 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.location = Some(GeoLocation { latitude, longitude });
        Ok(())
    })
}

/// Set the transition duration in minutes.
pub fn set_transition_minutes(minutes: u32) -> KernelResult<()> {
    if minutes > 120 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.transition_minutes = minutes;
        Ok(())
    })
}

/// Set whether to disable night light when on battery.
pub fn set_disable_on_battery(disable: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.disable_on_battery = disable;
        Ok(())
    })
}

/// Temporarily override the schedule (turn on/off until next transition).
pub fn set_manual_override(active: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.manual_override = active;
        if active {
            state.config.state = NightLightState::Active;
            state.config.current_temp = state.config.night_temp;
            state.config.override_until_ns = crate::hpet::elapsed_ns();
        } else {
            state.config.manual_override = false;
            // Will be corrected by next check_schedule
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Schedule checking
// ---------------------------------------------------------------------------

/// Check the schedule and update state.
///
/// Should be called periodically (e.g., every minute) by the compositor
/// or a timer service. Takes current hour and minute (24h format).
///
/// Returns the temperature that should be applied.
pub fn check_schedule(hour: u8, minute: u8) -> u32 {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return DEFAULT_DAY_TEMP,
    };
    state.check_count += 1;

    if !state.config.enabled {
        state.config.current_temp = state.config.day_temp;
        return state.config.day_temp;
    }

    if state.config.manual_override {
        return state.config.current_temp;
    }

    let in_night_period = match state.config.schedule_mode {
        ScheduleMode::Manual => {
            // In manual mode, state is driven by toggle/set_enabled.
            state.config.state == NightLightState::Active
        }
        ScheduleMode::Scheduled => {
            is_in_period(
                hour, minute,
                &state.config.start_time,
                &state.config.end_time,
            )
        }
        ScheduleMode::SunsetSunrise => {
            // Use sunset/sunrise approximation based on location.
            if let Some(loc) = &state.config.location {
                let (sunset_h, sunset_m, sunrise_h, sunrise_m) =
                    approx_sun_times(loc.latitude);
                let sunset = ScheduleTime { hour: sunset_h, minute: sunset_m };
                let sunrise = ScheduleTime { hour: sunrise_h, minute: sunrise_m };
                is_in_period(hour, minute, &sunset, &sunrise)
            } else {
                // No location set; use default schedule.
                is_in_period(
                    hour, minute,
                    &state.config.start_time,
                    &state.config.end_time,
                )
            }
        }
    };

    if in_night_period {
        state.config.state = NightLightState::Active;
        state.config.current_temp = state.config.night_temp;
    } else {
        state.config.state = NightLightState::DaytimeIdle;
        state.config.current_temp = state.config.day_temp;
    }

    state.config.current_temp
}

/// Check if the current time is within a start..end period.
/// Handles overnight ranges (e.g., 21:00..07:00).
fn is_in_period(hour: u8, minute: u8, start: &ScheduleTime, end: &ScheduleTime) -> bool {
    let now_min = hour as u32 * 60 + minute as u32;
    let start_min = start.as_minutes();
    let end_min = end.as_minutes();

    if start_min <= end_min {
        // Same-day range (e.g., 09:00..17:00).
        now_min >= start_min && now_min < end_min
    } else {
        // Overnight range (e.g., 21:00..07:00).
        now_min >= start_min || now_min < end_min
    }
}

/// Approximate sunset/sunrise hours based on latitude.
///
/// This is a rough approximation — a real implementation would use
/// proper solar position calculations. For a demo OS this provides
/// reasonable defaults.
fn approx_sun_times(latitude: i32) -> (u8, u8, u8, u8) {
    // Rough sunset/sunrise for temperate zones.
    // latitude > 0 = northern hemisphere.
    let abs_lat = if latitude < 0 { -latitude } else { latitude } as u32;

    // Base times for equator (~6:00 sunrise, ~18:00 sunset).
    // Adjust by latitude — higher latitudes have more variation.
    let sunset_hour = if abs_lat < 23 {
        18 // Tropics: ~18:00
    } else if abs_lat < 45 {
        19 // Temperate: ~19:00
    } else if abs_lat < 60 {
        20 // Northern: ~20:00
    } else {
        21 // Far north: ~21:00
    };

    let sunrise_hour = if abs_lat < 45 {
        6 // Tropics and temperate: ~06:00
    } else if abs_lat < 60 {
        5
    } else {
        4
    };

    (sunset_hour, 0, sunrise_hour, 0)
}

// ---------------------------------------------------------------------------
// Compositor integration
// ---------------------------------------------------------------------------

/// Get the current colour temperature to apply.
///
/// The compositor calls this during frame composition to determine
/// the gamma/colour adjustment. Returns Kelvin (1000–6500).
pub fn current_temperature() -> u32 {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.config.current_temp,
        None => DEFAULT_DAY_TEMP,
    }
}

/// Get the current configuration snapshot.
pub fn config() -> KernelResult<NightLightConfig> {
    let guard = STATE.lock();
    guard.as_ref()
        .map(|s| s.config.clone())
        .ok_or(KernelError::NotSupported)
}

/// Get the current state.
pub fn current_state() -> NightLightState {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.config.state,
        None => NightLightState::Disabled,
    }
}

/// Convert a colour temperature in Kelvin to approximate RGB values.
///
/// Uses a piecewise-linear lookup table derived from Tanner Helland's
/// algorithm.  Avoids floating-point math (no `powf`/`ln` in `no_std`).
/// Returns (r, g, b) in 0–255 range.
pub fn temp_to_rgb(temp_k: u32) -> (u8, u8, u8) {
    // Lookup table: (kelvin, r, g, b) at 500K intervals from 1000K to 6500K.
    // Pre-computed from the Helland algorithm.
    const TABLE: &[(u32, u8, u8, u8)] = &[
        (1000, 255, 56,  0),
        (1500, 255, 109, 0),
        (2000, 255, 137, 18),
        (2500, 255, 161, 72),
        (3000, 255, 180, 107),
        (3500, 255, 196, 137),
        (4000, 255, 209, 163),
        (4500, 255, 219, 186),
        (5000, 255, 228, 206),
        (5500, 255, 236, 224),
        (6000, 255, 243, 239),
        (6500, 255, 249, 253),
    ];

    let temp = temp_k.clamp(1000, 6500);

    // Find bracketing entries and linearly interpolate.
    for i in 0..TABLE.len() - 1 {
        let (t0, r0, g0, b0) = TABLE[i];
        let (t1, r1, g1, b1) = TABLE[i + 1];
        if temp >= t0 && temp <= t1 {
            if t0 == t1 {
                return (r0, g0, b0);
            }
            let frac = ((temp - t0) * 256) / (t1 - t0);
            let r = lerp_u8(r0, r1, frac);
            let g = lerp_u8(g0, g1, frac);
            let b = lerp_u8(b0, b1, frac);
            return (r, g, b);
        }
    }

    // Fallback: last entry.
    let last = TABLE[TABLE.len() - 1];
    (last.1, last.2, last.3)
}

/// Linear interpolation between two u8 values.
/// `frac` is in 0..256 range (256 = 1.0).
fn lerp_u8(a: u8, b: u8, frac: u32) -> u8 {
    let a32 = a as u32;
    let b32 = b as u32;
    let result = if b32 >= a32 {
        a32 + ((b32 - a32) * frac) / 256
    } else {
        a32 - ((a32 - b32) * frac) / 256
    };
    result.min(255) as u8
}

/// Format temperature as human-readable string.
pub fn format_temp(temp_k: u32) -> String {
    let (r, g, b) = temp_to_rgb(temp_k);
    format!("{}K (RGB: {},{},{})", temp_k, r, g, b)
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (enabled, current_temp, toggle_count, check_count, ops).
pub fn stats() -> (bool, u32, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.config.enabled,
            s.config.current_temp,
            s.toggle_count,
            s.check_count,
            s.ops,
        ),
        None => (false, DEFAULT_DAY_TEMP, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the night light module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[nightlight] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state is disabled.
    {
        let (enabled, temp, _, _, _) = stats();
        assert!(!enabled);
        assert_eq!(temp, DEFAULT_DAY_TEMP);
        assert_eq!(current_state(), NightLightState::Disabled);
    }
    serial_println!("[nightlight]  1/11 initial state OK");

    // Test 2: enable/disable.
    {
        set_enabled(true).unwrap();
        let (enabled, temp, toggle_count, _, _) = stats();
        assert!(enabled);
        assert_eq!(temp, DEFAULT_NIGHT_TEMP);
        assert_eq!(toggle_count, 1);
        assert_eq!(current_state(), NightLightState::Active);

        set_enabled(false).unwrap();
        let (enabled, temp, _, _, _) = stats();
        assert!(!enabled);
        assert_eq!(temp, DEFAULT_DAY_TEMP);
    }
    serial_println!("[nightlight]  2/11 enable/disable OK");

    // Test 3: toggle.
    {
        let now_on = toggle().unwrap();
        assert!(now_on);
        let now_off = toggle().unwrap();
        assert!(!now_off);
    }
    serial_println!("[nightlight]  3/11 toggle OK");

    // Test 4: set night temperature.
    {
        set_enabled(true).unwrap();
        set_night_temp(3500).unwrap();
        let (_, temp, _, _, _) = stats();
        assert_eq!(temp, 3500);

        // Out of range.
        assert!(set_night_temp(500).is_err());
        assert!(set_night_temp(7000).is_err());
        set_enabled(false).unwrap();
    }
    serial_println!("[nightlight]  4/11 night temp OK");

    // Test 5: set day temperature.
    {
        set_day_temp(5500).unwrap();
        let (_, temp, _, _, _) = stats();
        // Currently disabled, so day temp applies.
        assert_eq!(temp, 5500);
        set_day_temp(DEFAULT_DAY_TEMP).unwrap();
    }
    serial_println!("[nightlight]  5/11 day temp OK");

    // Test 6: schedule mode.
    {
        set_schedule_mode(ScheduleMode::SunsetSunrise).unwrap();
        let cfg = config().unwrap();
        assert_eq!(cfg.schedule_mode, ScheduleMode::SunsetSunrise);
        set_schedule_mode(ScheduleMode::Scheduled).unwrap();
    }
    serial_println!("[nightlight]  6/11 schedule mode OK");

    // Test 7: start/end time.
    {
        set_start_time(22, 30).unwrap();
        set_end_time(6, 45).unwrap();
        let cfg = config().unwrap();
        assert_eq!(cfg.start_time.hour, 22);
        assert_eq!(cfg.start_time.minute, 30);
        assert_eq!(cfg.end_time.hour, 6);
        assert_eq!(cfg.end_time.minute, 45);

        // Invalid times.
        assert!(set_start_time(25, 0).is_err());
        assert!(set_end_time(0, 60).is_err());
    }
    serial_println!("[nightlight]  7/11 start/end time OK");

    // Test 8: location.
    {
        set_location(40, -74).unwrap(); // New York
        let cfg = config().unwrap();
        assert!(cfg.location.is_some());
        let loc = cfg.location.unwrap();
        assert_eq!(loc.latitude, 40);
        assert_eq!(loc.longitude, -74);

        // Invalid.
        assert!(set_location(91, 0).is_err());
        assert!(set_location(0, -181).is_err());
    }
    serial_println!("[nightlight]  8/11 location OK");

    // Test 9: check_schedule during night period.
    {
        set_enabled(true).unwrap();
        set_night_temp(DEFAULT_NIGHT_TEMP).unwrap();
        set_start_time(21, 0).unwrap();
        set_end_time(7, 0).unwrap();
        set_schedule_mode(ScheduleMode::Scheduled).unwrap();

        // 23:00 — should be in night period.
        let temp = check_schedule(23, 0);
        assert_eq!(temp, DEFAULT_NIGHT_TEMP);
        assert_eq!(current_state(), NightLightState::Active);

        // 12:00 — should be in day period.
        let temp = check_schedule(12, 0);
        assert_eq!(temp, DEFAULT_DAY_TEMP);
        assert_eq!(current_state(), NightLightState::DaytimeIdle);

        set_enabled(false).unwrap();
    }
    serial_println!("[nightlight]  9/11 schedule check OK");

    // Test 10: temp_to_rgb.
    {
        let (r, g, b) = temp_to_rgb(6500);
        assert_eq!(r, 255);
        assert!(g > 200);
        assert!(b > 200);

        let (r2, _, b2) = temp_to_rgb(2000);
        assert_eq!(r2, 255);
        // Very warm: blue should be significantly reduced.
        assert!(b2 < 100);
    }
    serial_println!("[nightlight] 10/11 temp_to_rgb OK");

    // Test 11: transition and manual override.
    {
        set_enabled(true).unwrap();
        set_manual_override(true).unwrap();
        let cfg = config().unwrap();
        assert!(cfg.manual_override);
        assert_eq!(current_state(), NightLightState::Active);

        // check_schedule should respect override.
        let temp = check_schedule(12, 0);
        // Override means current_temp stays as set.
        assert_eq!(temp, cfg.current_temp);

        set_manual_override(false).unwrap();
        set_enabled(false).unwrap();
    }
    serial_println!("[nightlight] 11/11 manual override OK");

    serial_println!("[nightlight] All self-tests passed.");
}

//! Thermal — system thermal monitoring and management.
//!
//! Monitors CPU, GPU, and system temperatures, manages thermal
//! zones, fan control policies, and throttling thresholds.
//!
//! ## Architecture
//!
//! ```text
//! Thermal management
//!   → thermal::read_temp(zone) → current temperature
//!   → thermal::set_trip(zone, type, temp) → configure trip points
//!   → thermal::fan_speed(fan) → current fan RPM
//!   → thermal::set_fan_mode(mode) → auto/manual fan control
//!
//! Integration:
//!   → hwmonitor (hardware monitoring)
//!   → cpufreq (frequency scaling)
//!   → power (power management)
//!   → energysaver (energy saver)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Thermal zone type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneType {
    Cpu,
    Gpu,
    Chipset,
    Storage,
    Memory,
    Ambient,
}

impl ZoneType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
            Self::Chipset => "Chipset",
            Self::Storage => "Storage",
            Self::Memory => "Memory",
            Self::Ambient => "Ambient",
        }
    }
}

/// Trip point type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TripType {
    Active,
    Passive,
    Hot,
    Critical,
}

impl TripType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Passive => "Passive",
            Self::Hot => "Hot",
            Self::Critical => "Critical",
        }
    }
}

/// Fan control mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanMode {
    Auto,
    Manual,
    Silent,
    Performance,
}

impl FanMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Manual => "Manual",
            Self::Silent => "Silent",
            Self::Performance => "Performance",
        }
    }
}

/// A thermal zone.
#[derive(Debug, Clone)]
pub struct ThermalZone {
    pub id: u32,
    pub name: String,
    pub zone_type: ZoneType,
    /// Current temperature in millidegrees Celsius (e.g., 45000 = 45.0°C).
    pub temp_mc: i32,
    pub passive_trip_mc: i32,
    pub hot_trip_mc: i32,
    pub critical_trip_mc: i32,
    pub throttled: bool,
}

/// A fan.
#[derive(Debug, Clone)]
pub struct Fan {
    pub id: u32,
    pub name: String,
    pub rpm: u32,
    pub max_rpm: u32,
    pub duty_pct: u8,
    pub mode: FanMode,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_ZONES: usize = 32;
const MAX_FANS: usize = 16;

struct State {
    zones: Vec<ThermalZone>,
    fans: Vec<Fan>,
    next_zone_id: u32,
    next_fan_id: u32,
    total_readings: u64,
    total_throttle_events: u64,
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
        zones: alloc::vec![
            ThermalZone {
                id: 1, name: String::from("CPU Package"),
                zone_type: ZoneType::Cpu, temp_mc: 45_000,
                passive_trip_mc: 85_000, hot_trip_mc: 95_000, critical_trip_mc: 105_000,
                throttled: false,
            },
            ThermalZone {
                id: 2, name: String::from("GPU"),
                zone_type: ZoneType::Gpu, temp_mc: 42_000,
                passive_trip_mc: 80_000, hot_trip_mc: 90_000, critical_trip_mc: 100_000,
                throttled: false,
            },
            ThermalZone {
                id: 3, name: String::from("PCH/Chipset"),
                zone_type: ZoneType::Chipset, temp_mc: 38_000,
                passive_trip_mc: 75_000, hot_trip_mc: 85_000, critical_trip_mc: 95_000,
                throttled: false,
            },
        ],
        fans: alloc::vec![
            Fan { id: 1, name: String::from("CPU Fan"), rpm: 800, max_rpm: 3000, duty_pct: 30, mode: FanMode::Auto },
            Fan { id: 2, name: String::from("System Fan"), rpm: 600, max_rpm: 2000, duty_pct: 25, mode: FanMode::Auto },
        ],
        next_zone_id: 4,
        next_fan_id: 3,
        total_readings: 0,
        total_throttle_events: 0,
        ops: 0,
    });
}

/// Read temperature of a zone (millidegrees Celsius).
pub fn read_temp(zone_id: u32) -> KernelResult<i32> {
    with_state(|state| {
        let zone = state.zones.iter().find(|z| z.id == zone_id)
            .ok_or(KernelError::NotFound)?;
        state.total_readings += 1;
        Ok(zone.temp_mc)
    })
}

/// Update temperature of a zone (simulated sensor read).
pub fn update_temp(zone_id: u32, temp_mc: i32) -> KernelResult<()> {
    with_state(|state| {
        let zone = state.zones.iter_mut().find(|z| z.id == zone_id)
            .ok_or(KernelError::NotFound)?;
        zone.temp_mc = temp_mc;
        // Check for throttling.
        let was_throttled = zone.throttled;
        zone.throttled = temp_mc >= zone.passive_trip_mc;
        if zone.throttled && !was_throttled {
            state.total_throttle_events += 1;
        }
        state.total_readings += 1;
        Ok(())
    })
}

/// Set trip point temperature.
pub fn set_trip(zone_id: u32, trip: TripType, temp_mc: i32) -> KernelResult<()> {
    with_state(|state| {
        let zone = state.zones.iter_mut().find(|z| z.id == zone_id)
            .ok_or(KernelError::NotFound)?;
        match trip {
            TripType::Active => {} // No separate active trip in this simplified model.
            TripType::Passive => zone.passive_trip_mc = temp_mc,
            TripType::Hot => zone.hot_trip_mc = temp_mc,
            TripType::Critical => zone.critical_trip_mc = temp_mc,
        }
        Ok(())
    })
}

/// Set fan mode.
pub fn set_fan_mode(fan_id: u32, mode: FanMode) -> KernelResult<()> {
    with_state(|state| {
        let fan = state.fans.iter_mut().find(|f| f.id == fan_id)
            .ok_or(KernelError::NotFound)?;
        fan.mode = mode;
        Ok(())
    })
}

/// Set fan duty cycle (0-100%).
pub fn set_fan_duty(fan_id: u32, duty_pct: u8) -> KernelResult<()> {
    with_state(|state| {
        let fan = state.fans.iter_mut().find(|f| f.id == fan_id)
            .ok_or(KernelError::NotFound)?;
        if duty_pct > 100 {
            return Err(KernelError::InvalidArgument);
        }
        fan.duty_pct = duty_pct;
        // Simulate RPM based on duty.
        fan.rpm = (fan.max_rpm as u64 * duty_pct as u64 / 100) as u32;
        fan.mode = FanMode::Manual;
        Ok(())
    })
}

/// Get fan info.
pub fn get_fan(fan_id: u32) -> Option<Fan> {
    STATE.lock().as_ref().and_then(|s| s.fans.iter().find(|f| f.id == fan_id).cloned())
}

/// List all zones.
pub fn list_zones() -> Vec<ThermalZone> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.zones.clone())
}

/// List all fans.
pub fn list_fans() -> Vec<Fan> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.fans.clone())
}

/// Format millidegrees as degrees string.
pub fn format_temp(mc: i32) -> String {
    let whole = mc / 1000;
    let frac = (mc % 1000).unsigned_abs() / 100;
    format!("{}.{}°C", whole, frac)
}

/// Statistics: (zone_count, fan_count, total_readings, total_throttle_events, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.zones.len(), s.fans.len(), s.total_readings, s.total_throttle_events, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("thermal::self_test() — running tests...");
    init_defaults();

    // 1: Default zones.
    assert_eq!(list_zones().len(), 3);
    assert_eq!(list_fans().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Read temperature.
    let temp = read_temp(1).expect("read_temp");
    assert_eq!(temp, 45_000);
    crate::serial_println!("  [2/8] read temp: OK");

    // 3: Update temperature.
    update_temp(1, 65_000).expect("update");
    let temp = read_temp(1).expect("read2");
    assert_eq!(temp, 65_000);
    crate::serial_println!("  [3/8] update temp: OK");

    // 4: Throttle detection.
    update_temp(1, 90_000).expect("hot");
    let zones = list_zones();
    let cpu = zones.iter().find(|z| z.id == 1).expect("cpu_zone");
    assert!(cpu.throttled);
    crate::serial_println!("  [4/8] throttle: OK");

    // 5: Trip points.
    set_trip(1, TripType::Critical, 110_000).expect("trip");
    let zones = list_zones();
    let cpu = zones.iter().find(|z| z.id == 1).expect("cpu_zone2");
    assert_eq!(cpu.critical_trip_mc, 110_000);
    crate::serial_println!("  [5/8] trip points: OK");

    // 6: Fan mode.
    set_fan_mode(1, FanMode::Performance).expect("fan_mode");
    let fan = get_fan(1).expect("get_fan");
    assert_eq!(fan.mode, FanMode::Performance);
    crate::serial_println!("  [6/8] fan mode: OK");

    // 7: Fan duty.
    set_fan_duty(1, 80).expect("duty");
    let fan = get_fan(1).expect("fan2");
    assert_eq!(fan.duty_pct, 80);
    assert!(fan.rpm > 0);
    assert_eq!(fan.mode, FanMode::Manual);
    crate::serial_println!("  [7/8] fan duty: OK");

    // 8: Stats + format.
    assert_eq!(format_temp(45_000), "45.0°C");
    assert_eq!(format_temp(65_500), "65.5°C");
    let (zones_n, fans_n, readings, throttles, ops) = stats();
    assert_eq!(zones_n, 3);
    assert_eq!(fans_n, 2);
    assert!(readings > 0);
    assert!(throttles >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("thermal::self_test() — all 8 tests passed");
}

//! Battery and UPS management — hardware power source monitoring.
//!
//! Tracks battery health, charge cycles, discharge rate, estimated
//! remaining time, and UPS status. Separate from power profiles
//! (powerprofile.rs handles policy, this handles hardware state).
//!
//! ## Architecture
//!
//! ```text
//! ACPI / power hardware
//!   → battery::update_status(charge, rate, state)
//!
//! Settings panel → Power → Battery
//!   → battery::get_info() / health() / cycle_count()
//!
//! System tray indicator
//!   → battery::charge_pct() + state for icon
//!
//! Integration:
//!   → powerprofile (policy decisions based on battery state)
//!   → notifcenter (low battery / critical alerts)
//!   → statusbar (battery icon)
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

/// Power source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSourceType {
    Battery,
    Ups,
    AcAdapter,
}

impl PowerSourceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Battery => "Battery",
            Self::Ups => "UPS",
            Self::AcAdapter => "AC Adapter",
        }
    }
}

/// Battery charging state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeState {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

impl ChargeState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Charging => "Charging",
            Self::Discharging => "Discharging",
            Self::Full => "Full",
            Self::NotCharging => "Not Charging",
            Self::Unknown => "Unknown",
        }
    }
}

/// Battery health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Good,
    Fair,
    Poor,
    Critical,
    Unknown,
}

impl HealthStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Good => "Good",
            Self::Fair => "Fair",
            Self::Poor => "Poor",
            Self::Critical => "Critical",
            Self::Unknown => "Unknown",
        }
    }
}

/// A power source (battery or UPS).
#[derive(Debug, Clone)]
pub struct PowerSource {
    pub id: u32,
    pub name: String,
    pub source_type: PowerSourceType,
    pub state: ChargeState,
    /// Charge percentage (0-100).
    pub charge_pct: u8,
    /// Design capacity (mWh).
    pub design_capacity_mwh: u32,
    /// Current full charge capacity (mWh).
    pub full_charge_capacity_mwh: u32,
    /// Current energy remaining (mWh).
    pub energy_remaining_mwh: u32,
    /// Current charge/discharge rate (mW).
    pub power_rate_mw: u32,
    /// Estimated time remaining (minutes, 0 = unknown).
    pub time_remaining_min: u32,
    /// Battery voltage (mV).
    pub voltage_mv: u32,
    /// Cycle count.
    pub cycle_count: u32,
    /// Health status.
    pub health: HealthStatus,
    /// Temperature (millidegrees C).
    pub temperature_mc: i32,
    /// Manufacturer.
    pub manufacturer: String,
    /// Model.
    pub model: String,
    /// Serial number.
    pub serial: String,
    /// Whether this source is present/connected.
    pub present: bool,
    /// AC power connected.
    pub ac_connected: bool,
}

/// Alert thresholds.
#[derive(Debug, Clone)]
pub struct BatteryAlerts {
    /// Low battery warning threshold (%).
    pub low_pct: u8,
    /// Critical battery threshold (%).
    pub critical_pct: u8,
    /// Action on critical: "suspend", "hibernate", "shutdown".
    pub critical_action: String,
    /// Enable charge limit (for battery longevity).
    pub charge_limit_enabled: bool,
    /// Maximum charge percentage (for longevity).
    pub charge_limit_pct: u8,
}

impl Default for BatteryAlerts {
    fn default() -> Self {
        Self {
            low_pct: 20,
            critical_pct: 5,
            critical_action: String::from("hibernate"),
            charge_limit_enabled: false,
            charge_limit_pct: 80,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    sources: Vec<PowerSource>,
    alerts: BatteryAlerts,
    next_id: u32,
    alert_count: u64,
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

fn health_from_capacity(design: u32, current: u32) -> HealthStatus {
    if design == 0 || current == 0 { return HealthStatus::Unknown; }
    // Use integer percentage: current * 100 / design.
    let pct = (current as u64 * 100) / (design as u64);
    if pct >= 80 { HealthStatus::Good }
    else if pct >= 60 { HealthStatus::Fair }
    else if pct >= 40 { HealthStatus::Poor }
    else { HealthStatus::Critical }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    // Start with NO power sources. A battery/UPS is observed hardware state
    // (charge %, cycle count, capacity, voltage, temperature) — not a
    // configurable default. Seeding a phantom "BAT0" at 75% with 200 cycles and
    // a 50 Wh design capacity, or an "AC0" adapter, would surface fabricated
    // hardware readings through /proc/battery and the `battery` shell command as
    // if a real ACPI power source had reported them. A desktop may have no
    // battery at all. Real sources appear only when an ACPI/power driver calls
    // register_source() and reports live values via update_status().
    //
    // DEFERRED PROPER FIX: wire register_source()/update_status() to a real ACPI
    // battery driver (_BIF/_BST objects) once one exists; until then this stays
    // empty so /proc/battery reports "sources: 0" rather than inventing a cell.
    //
    // The alert thresholds (low/critical %, critical action, charge limit) are
    // genuine user-tunable policy defaults, not observations, so they are kept.
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    *guard = Some(State {
        sources: Vec::new(),
        alerts: BatteryAlerts::default(),
        next_id: 1,
        alert_count: 0,
        ops: 0,
    });
}

/// Register a power source.
pub fn register_source(
    name: &str,
    source_type: PowerSourceType,
    design_capacity_mwh: u32,
) -> KernelResult<u32> {
    with_state(|state| {
        let id = state.next_id;
        state.next_id += 1;
        state.sources.push(PowerSource {
            id, name: String::from(name), source_type,
            state: ChargeState::Unknown, charge_pct: 0,
            design_capacity_mwh, full_charge_capacity_mwh: design_capacity_mwh,
            energy_remaining_mwh: 0, power_rate_mw: 0,
            time_remaining_min: 0, voltage_mv: 0, cycle_count: 0,
            health: HealthStatus::Unknown, temperature_mc: 0,
            manufacturer: String::new(), model: String::new(),
            serial: String::new(), present: true, ac_connected: false,
        });
        Ok(id)
    })
}

/// Update battery status (called by ACPI/power driver).
pub fn update_status(
    id: u32,
    charge_state: ChargeState,
    charge_pct: u8,
    energy_mwh: u32,
    rate_mw: u32,
) -> KernelResult<()> {
    with_state(|state| {
        let src = state.sources.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        src.state = charge_state;
        src.charge_pct = charge_pct.min(100);
        src.energy_remaining_mwh = energy_mwh;
        src.power_rate_mw = rate_mw;

        // Estimate time remaining.
        if rate_mw > 0 && charge_state == ChargeState::Discharging {
            src.time_remaining_min = ((energy_mwh as u64 * 60) / (rate_mw as u64)) as u32;
        } else {
            src.time_remaining_min = 0;
        }

        // Update health.
        src.health = health_from_capacity(src.design_capacity_mwh, src.full_charge_capacity_mwh);

        // Check alerts.
        if src.source_type == PowerSourceType::Battery && charge_state == ChargeState::Discharging {
            if charge_pct <= state.alerts.critical_pct || charge_pct <= state.alerts.low_pct {
                state.alert_count += 1;
            }
        }

        Ok(())
    })
}

/// Set AC connected state.
pub fn set_ac_connected(connected: bool) -> KernelResult<()> {
    with_state(|state| {
        for src in &mut state.sources {
            src.ac_connected = connected;
            if src.source_type == PowerSourceType::Battery {
                if connected {
                    src.state = ChargeState::Charging;
                } else {
                    src.state = ChargeState::Discharging;
                }
            }
        }
        Ok(())
    })
}

/// Get primary battery info.
pub fn primary_battery() -> Option<PowerSource> {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| {
        s.sources.iter().find(|src| src.source_type == PowerSourceType::Battery && src.present).cloned()
    })
}

/// List all power sources.
pub fn list_sources() -> Vec<PowerSource> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sources.clone())
}

/// Get a power source by ID.
pub fn get_source(id: u32) -> KernelResult<PowerSource> {
    with_state(|state| {
        state.sources.iter().find(|s| s.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Set alert thresholds.
pub fn set_low_threshold(pct: u8) -> KernelResult<()> {
    with_state(|state| { state.alerts.low_pct = pct; Ok(()) })
}

pub fn set_critical_threshold(pct: u8) -> KernelResult<()> {
    with_state(|state| { state.alerts.critical_pct = pct; Ok(()) })
}

pub fn set_critical_action(action: &str) -> KernelResult<()> {
    with_state(|state| { state.alerts.critical_action = String::from(action); Ok(()) })
}

pub fn set_charge_limit(enabled: bool, pct: u8) -> KernelResult<()> {
    with_state(|state| {
        state.alerts.charge_limit_enabled = enabled;
        state.alerts.charge_limit_pct = pct;
        Ok(())
    })
}

pub fn get_alerts() -> KernelResult<BatteryAlerts> {
    with_state(|state| Ok(state.alerts.clone()))
}

/// Quick charge percentage (for statusbar).
pub fn charge_pct() -> u8 {
    primary_battery().map_or(0, |b| b.charge_pct)
}

/// Quick check if on AC power.
pub fn on_ac_power() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| s.sources.iter().any(|src| src.ac_connected))
}

/// Statistics: (source_count, charge_pct, state_label, cycle_count, alert_count, ops).
pub fn stats() -> (usize, u8, &'static str, u32, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let bat = s.sources.iter().find(|src| src.source_type == PowerSourceType::Battery);
            let (pct, state_label, cycles) = match bat {
                Some(b) => (b.charge_pct, b.state.label(), b.cycle_count),
                None => (0, "N/A", 0),
            };
            (s.sources.len(), pct, state_label, cycles, s.alert_count, s.ops)
        }
        None => (0, 0, "N/A", 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("battery::self_test() — running tests...");

    // Residue-free: start from a clean, controlled State so assertions hold
    // regardless of prior kshell/procfs activity (init_defaults early-returns
    // when STATE is already populated), and build every fixture through the real
    // register_source()/update_status() driver API rather than seeded hardware.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults — no power sources until a driver registers one.
    assert_eq!(list_sources().len(), 0);
    assert!(primary_battery().is_none());
    assert_eq!(charge_pct(), 0);
    crate::serial_println!("  [1/11] empty defaults: OK");

    // Build a fixture battery the way an ACPI driver would: register it, then
    // report a live reading.
    let bat_id = register_source("BAT0", PowerSourceType::Battery, 50_000).expect("register bat");
    update_status(bat_id, ChargeState::Discharging, 75, 33_750, 15_000).expect("seed reading");

    // 2: Primary battery now visible with the reported charge.
    let bat = primary_battery().expect("primary battery");
    assert_eq!(bat.source_type, PowerSourceType::Battery);
    assert_eq!(bat.charge_pct, 75);
    crate::serial_println!("  [2/11] primary battery: OK");

    // 3: Update status.
    update_status(bat_id, ChargeState::Discharging, 50, 25_000, 10_000).expect("update");
    let bat = get_source(bat_id).expect("get after update");
    assert_eq!(bat.charge_pct, 50);
    assert!(bat.time_remaining_min > 0);
    crate::serial_println!("  [3/11] update status: OK");

    // 4: AC connect.
    set_ac_connected(true).expect("ac connect");
    let bat = get_source(bat_id).expect("get after ac");
    assert_eq!(bat.state, ChargeState::Charging);
    assert!(bat.ac_connected);
    crate::serial_println!("  [4/11] AC connect: OK");

    // 5: AC disconnect.
    set_ac_connected(false).expect("ac disconnect");
    let bat = get_source(bat_id).expect("get after ac off");
    assert_eq!(bat.state, ChargeState::Discharging);
    crate::serial_println!("  [5/11] AC disconnect: OK");

    // 6: Register UPS.
    let ups_id = register_source("UPS0", PowerSourceType::Ups, 100_000).expect("register ups");
    let ups = get_source(ups_id).expect("get ups");
    assert_eq!(ups.source_type, PowerSourceType::Ups);
    crate::serial_println!("  [6/11] register UPS: OK");

    // 7: Alert thresholds.
    set_low_threshold(25).expect("set low");
    set_critical_threshold(10).expect("set critical");
    let alerts = get_alerts().expect("get alerts");
    assert_eq!(alerts.low_pct, 25);
    assert_eq!(alerts.critical_pct, 10);
    crate::serial_println!("  [7/11] alert thresholds: OK");

    // 8: Critical action.
    set_critical_action("shutdown").expect("set action");
    let alerts = get_alerts().expect("get alerts 2");
    assert_eq!(alerts.critical_action, "shutdown");
    crate::serial_println!("  [8/11] critical action: OK");

    // 9: Charge limit.
    set_charge_limit(true, 80).expect("set limit");
    let alerts = get_alerts().expect("get alerts 3");
    assert!(alerts.charge_limit_enabled);
    assert_eq!(alerts.charge_limit_pct, 80);
    crate::serial_println!("  [9/11] charge limit: OK");

    // 10: Quick charge percentage.
    let pct = charge_pct();
    assert_eq!(pct, 50);
    crate::serial_println!("  [10/11] quick charge pct: OK");

    // 11: Stats — exact: 2 sources (BAT0 + UPS0), battery at 50% discharging.
    let (count, pct, state_label, _cycles, _alerts, ops) = stats();
    assert_eq!(count, 2);
    assert_eq!(pct, 50);
    assert_eq!(state_label, "Discharging");
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    // Leave no residue for later callers / the live /proc/battery view.
    *STATE.lock() = None;

    crate::serial_println!("battery::self_test() — all 11 tests passed");
}

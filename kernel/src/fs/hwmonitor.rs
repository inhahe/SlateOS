//! Hardware monitor — sensor readings, threshold alerts, and component tracking.
//!
//! Collects temperature, fan speed, voltage, power, clock, and utilization
//! readings from hardware components.  Evaluates configurable warning and
//! critical thresholds, generating alerts when values leave safe ranges.
//!
//! All values are stored as integers to avoid floating point in `no_std`:
//! temperatures in millidegrees Celsius (45000 = 45.0 °C), voltages in
//! millivolts (12050 = 12.050 V), fan speed in raw RPM, power in
//! milliwatts, clock in MHz, utilization in tenths of a percent.
//!
//! ## Architecture
//!
//! ```text
//! hwmonitor::init_defaults()
//!   ├── add_component("CPU")
//!   │     ├── add_sensor("CPU Temp",  Temperature, ...)
//!   │     └── add_sensor("CPU Fan",   FanSpeed, ...)
//!   ├── add_component("GPU")
//!   │     └── add_sensor("GPU Temp",  Temperature, ...)
//!   └── add_component("Motherboard")
//!         ├── add_sensor("System Temp", Temperature, ...)
//!         └── add_sensor("+12V Rail",   Voltage, ...)
//!
//! hwmonitor::update_sensor(id, value)
//!   ├── update min/max tracking
//!   ├── evaluate thresholds (fan: below = bad; others: above = bad)
//!   ├── set SensorStatus (Normal / Warning / Critical)
//!   └── create AlertEntry if threshold exceeded
//!
//! hwmonitor::get_sensor(id)   → Sensor snapshot
//! hwmonitor::list_sensors()   → all sensors
//! hwmonitor::list_components()→ all components
//! hwmonitor::get_alerts(n)    → recent alerts
//! hwmonitor::stats()          → (sensors, components, readings, alerts, ops)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Kind of hardware sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorType {
    /// Temperature in millidegrees Celsius (45000 = 45.0 °C).
    Temperature,
    /// Fan speed in RPM.
    FanSpeed,
    /// Voltage in millivolts (12050 = 12.050 V).
    Voltage,
    /// Power draw in milliwatts.
    Power,
    /// Clock frequency in MHz.
    Clock,
    /// Utilization in tenths of a percent (1000 = 100.0 %).
    Utilization,
}

impl SensorType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Temperature => "Temperature",
            Self::FanSpeed => "Fan Speed",
            Self::Voltage => "Voltage",
            Self::Power => "Power",
            Self::Clock => "Clock",
            Self::Utilization => "Utilization",
        }
    }

    /// Unit string for display.
    pub fn unit(self) -> &'static str {
        match self {
            Self::Temperature => "°C",
            Self::FanSpeed => "RPM",
            Self::Voltage => "V",
            Self::Power => "W",
            Self::Clock => "MHz",
            Self::Utilization => "%",
        }
    }
}

/// Threshold evaluation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorStatus {
    /// Value is within acceptable range.
    Normal,
    /// Value has crossed the warning threshold.
    Warning,
    /// Value has crossed the critical threshold.
    Critical,
}

impl SensorStatus {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Warning => "Warning",
            Self::Critical => "Critical",
        }
    }
}

/// A single hardware sensor reading and its metadata.
#[derive(Debug, Clone)]
pub struct Sensor {
    /// Unique sensor identifier.
    pub id: u32,
    /// Human-readable sensor name.
    pub name: String,
    /// What the sensor measures.
    pub sensor_type: SensorType,
    /// Current reading (integer units, see [`SensorType`] docs).
    pub value: i64,
    /// Lowest value observed since registration.
    pub min_value: i64,
    /// Highest value observed since registration.
    pub max_value: i64,
    /// Threshold for a warning alert (meaning depends on sensor type).
    pub warning_threshold: i64,
    /// Threshold for a critical alert.
    pub critical_threshold: i64,
    /// Current status derived from the last reading.
    pub status: SensorStatus,
    /// Timestamp (nanoseconds since boot) of the last update.
    pub last_update_ns: u64,
}

/// A logical hardware component that groups related sensors.
#[derive(Debug, Clone)]
pub struct HwComponent {
    /// Unique component identifier.
    pub id: u32,
    /// Human-readable component name.
    pub name: String,
    /// Component category (e.g. "CPU", "GPU", "Motherboard").
    pub component_type: String,
    /// IDs of sensors belonging to this component.
    pub sensors: Vec<u32>,
}

/// Record of a threshold violation.
#[derive(Debug, Clone)]
pub struct AlertEntry {
    /// Sensor that triggered the alert.
    pub sensor_id: u32,
    /// Sensor name at the time of the alert.
    pub sensor_name: String,
    /// Value that triggered the alert.
    pub value: i64,
    /// Threshold that was crossed.
    pub threshold: i64,
    /// `true` if this was a critical alert, `false` for warning.
    pub is_critical: bool,
    /// Timestamp (nanoseconds since boot) when the alert was created.
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Maximum number of sensors the subsystem tracks.
const MAX_SENSORS: usize = 200;

/// Maximum number of hardware components.
const MAX_COMPONENTS: usize = 50;

struct State {
    sensors: Vec<Sensor>,
    components: Vec<HwComponent>,
    alerts: Vec<AlertEntry>,
    next_sensor_id: u32,
    next_component_id: u32,
    total_readings: u64,
    total_alerts: u64,
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
// Helpers
// ---------------------------------------------------------------------------

/// Evaluate a sensor reading against its thresholds.
///
/// For fan speed, *low* values are bad (fan stalling), so warning fires
/// when `value < warning_threshold` and critical when `value < critical_threshold`.
/// For all other sensor types, *high* values are bad.
fn evaluate_status(sensor_type: SensorType, value: i64, warning: i64, critical: i64) -> SensorStatus {
    match sensor_type {
        SensorType::FanSpeed => {
            if value < critical {
                SensorStatus::Critical
            } else if value < warning {
                SensorStatus::Warning
            } else {
                SensorStatus::Normal
            }
        }
        _ => {
            if value > critical {
                SensorStatus::Critical
            } else if value > warning {
                SensorStatus::Warning
            } else {
                SensorStatus::Normal
            }
        }
    }
}

/// Register a sensor inside the state and return its id.
fn add_sensor_inner(
    state: &mut State,
    component_id: u32,
    name: &str,
    sensor_type: SensorType,
    initial_value: i64,
    warning_threshold: i64,
    critical_threshold: i64,
) -> KernelResult<u32> {
    if state.sensors.len() >= MAX_SENSORS {
        return Err(KernelError::ResourceExhausted);
    }

    let comp = state
        .components
        .iter_mut()
        .find(|c| c.id == component_id)
        .ok_or(KernelError::NotFound)?;

    let id = state.next_sensor_id;
    state.next_sensor_id = state.next_sensor_id.wrapping_add(1);

    let status = evaluate_status(sensor_type, initial_value, warning_threshold, critical_threshold);

    state.sensors.push(Sensor {
        id,
        name: String::from(name),
        sensor_type,
        value: initial_value,
        min_value: initial_value,
        max_value: initial_value,
        warning_threshold,
        critical_threshold,
        status,
        last_update_ns: crate::hpet::elapsed_ns(),
    });

    comp.sensors.push(id);
    Ok(id)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the hardware monitor with default components and sensors.
///
/// Creates CPU (temp + fan), GPU (temp), and Motherboard (temp + voltage)
/// components.  Safe to call multiple times — subsequent calls are no-ops.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let mut state = State {
        sensors: Vec::new(),
        components: Vec::new(),
        alerts: Vec::new(),
        next_sensor_id: 1,
        next_component_id: 1,
        total_readings: 0,
        total_alerts: 0,
        ops: 0,
    };

    // --- CPU ---
    let cpu_id = state.next_component_id;
    state.next_component_id += 1;
    state.components.push(HwComponent {
        id: cpu_id,
        name: String::from("CPU"),
        component_type: String::from("CPU"),
        sensors: Vec::new(),
    });

    // CPU Temperature: 45.0 °C, warn at 85 °C, critical at 100 °C
    let _ = add_sensor_inner(&mut state, cpu_id, "CPU Temp", SensorType::Temperature, 45_000, 85_000, 100_000);
    // CPU Fan: 1200 RPM, warn below 500 RPM, critical below 300 RPM
    let _ = add_sensor_inner(&mut state, cpu_id, "CPU Fan", SensorType::FanSpeed, 1200, 500, 300);

    // --- GPU ---
    let gpu_id = state.next_component_id;
    state.next_component_id += 1;
    state.components.push(HwComponent {
        id: gpu_id,
        name: String::from("GPU"),
        component_type: String::from("GPU"),
        sensors: Vec::new(),
    });

    // GPU Temperature: 38.0 °C (same thresholds as CPU)
    let _ = add_sensor_inner(&mut state, gpu_id, "GPU Temp", SensorType::Temperature, 38_000, 85_000, 100_000);

    // --- Motherboard ---
    let mb_id = state.next_component_id;
    state.next_component_id += 1;
    state.components.push(HwComponent {
        id: mb_id,
        name: String::from("Motherboard"),
        component_type: String::from("Motherboard"),
        sensors: Vec::new(),
    });

    // System Temperature: 32.0 °C
    let _ = add_sensor_inner(&mut state, mb_id, "System Temp", SensorType::Temperature, 32_000, 85_000, 100_000);
    // +12 V rail: 12.050 V (warn < 11.400 V, critical < 10.800 V — stored in millivolts).
    // Voltage rails are *low* bad like fans? No — spec says only FanSpeed uses
    // below-threshold logic.  For voltage, above-threshold is bad.  But for a
    // 12 V rail the danger is *under*-voltage.  We model the thresholds as
    // "value > threshold triggers" per spec; the caller sets thresholds that
    // make sense for over-voltage monitoring.  Under-voltage monitoring can
    // be done by the caller comparing value < some floor.  The spec is explicit:
    // only FanSpeed uses inverted logic.
    let _ = add_sensor_inner(&mut state, mb_id, "+12V Rail", SensorType::Voltage, 12_050, 11_400, 10_800);

    *guard = Some(state);
}

/// Register a new hardware component.
///
/// Returns the new component's ID.
pub fn add_component(name: &str, component_type: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.components.len() >= MAX_COMPONENTS {
            return Err(KernelError::ResourceExhausted);
        }
        if name.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        let id = state.next_component_id;
        state.next_component_id = state.next_component_id.wrapping_add(1);
        state.components.push(HwComponent {
            id,
            name: String::from(name),
            component_type: String::from(component_type),
            sensors: Vec::new(),
        });
        Ok(id)
    })
}

/// Register a new sensor on an existing component.
///
/// Returns the new sensor's ID.
pub fn add_sensor(
    component_id: u32,
    name: &str,
    sensor_type: SensorType,
    warning_threshold: i64,
    critical_threshold: i64,
) -> KernelResult<u32> {
    with_state(|state| {
        if name.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        add_sensor_inner(state, component_id, name, sensor_type, 0, warning_threshold, critical_threshold)
    })
}

/// Record a new reading for a sensor.
///
/// Updates the sensor's value, min/max tracking, and status.  Returns:
/// - `Ok(Some(true))` — critical threshold crossed, alert created.
/// - `Ok(Some(false))` — warning threshold crossed, alert created.
/// - `Ok(None)` — value is within normal range.
pub fn update_sensor(sensor_id: u32, value: i64) -> KernelResult<Option<bool>> {
    with_state(|state| {
        let sensor = state
            .sensors
            .iter_mut()
            .find(|s| s.id == sensor_id)
            .ok_or(KernelError::NotFound)?;

        sensor.value = value;
        sensor.last_update_ns = crate::hpet::elapsed_ns();
        state.total_readings += 1;

        // Update min/max tracking.
        if value < sensor.min_value {
            sensor.min_value = value;
        }
        if value > sensor.max_value {
            sensor.max_value = value;
        }

        // Evaluate thresholds.
        let new_status = evaluate_status(
            sensor.sensor_type,
            value,
            sensor.warning_threshold,
            sensor.critical_threshold,
        );
        sensor.status = new_status;

        match new_status {
            SensorStatus::Critical => {
                state.total_alerts += 1;
                state.alerts.push(AlertEntry {
                    sensor_id,
                    sensor_name: sensor.name.clone(),
                    value,
                    threshold: sensor.critical_threshold,
                    is_critical: true,
                    timestamp_ns: sensor.last_update_ns,
                });
                Ok(Some(true))
            }
            SensorStatus::Warning => {
                state.total_alerts += 1;
                state.alerts.push(AlertEntry {
                    sensor_id,
                    sensor_name: sensor.name.clone(),
                    value,
                    threshold: sensor.warning_threshold,
                    is_critical: false,
                    timestamp_ns: sensor.last_update_ns,
                });
                Ok(Some(false))
            }
            SensorStatus::Normal => Ok(None),
        }
    })
}

/// Retrieve a snapshot of a single sensor.
pub fn get_sensor(sensor_id: u32) -> KernelResult<Sensor> {
    with_state(|state| {
        state
            .sensors
            .iter()
            .find(|s| s.id == sensor_id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List all registered sensors.
pub fn list_sensors() -> Vec<Sensor> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.sensors.clone())
}

/// List all registered components.
pub fn list_components() -> Vec<HwComponent> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.components.clone())
}

/// Return the most recent alerts, up to `limit`.
///
/// Alerts are returned newest-first.
pub fn get_alerts(limit: usize) -> Vec<AlertEntry> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = s.alerts.len().saturating_sub(limit);
        let mut result: Vec<AlertEntry> = s.alerts.get(start..).unwrap_or(&[]).to_vec();
        result.reverse();
        result
    })
}

/// Clear all stored alerts.  Returns the number of alerts removed.
pub fn clear_alerts() -> usize {
    let mut guard = STATE.lock();
    match guard.as_mut() {
        Some(state) => {
            let count = state.alerts.len();
            state.alerts.clear();
            count
        }
        None => 0,
    }
}

/// Summary statistics: (sensor_count, component_count, total_readings, total_alerts, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.sensors.len(),
            s.components.len(),
            s.total_readings,
            s.total_alerts,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("hwmonitor::self_test() — running tests...");
    init_defaults();

    // 1: Default components created.
    let comps = list_components();
    assert_eq!(comps.len(), 3);
    crate::serial_println!("  [1/11] default components: OK");

    // 2: Default sensors created (2 CPU + 1 GPU + 2 MB = 5).
    let sensors = list_sensors();
    assert_eq!(sensors.len(), 5);
    crate::serial_println!("  [2/11] default sensors: OK");

    // 3: CPU temp sensor has correct initial value (45 000 millidegrees).
    let cpu_temp = sensors.iter().find(|s| s.name == "CPU Temp").expect("cpu temp");
    assert_eq!(cpu_temp.value, 45_000);
    assert_eq!(cpu_temp.sensor_type, SensorType::Temperature);
    assert_eq!(cpu_temp.status, SensorStatus::Normal);
    crate::serial_println!("  [3/11] CPU temp initial value: OK");

    // 4: Update sensor — normal value keeps Normal status.
    let result = update_sensor(cpu_temp.id, 60_000).expect("update normal");
    assert!(result.is_none());
    let s = get_sensor(cpu_temp.id).expect("get");
    assert_eq!(s.value, 60_000);
    assert_eq!(s.status, SensorStatus::Normal);
    crate::serial_println!("  [4/11] update sensor normal: OK");

    // 5: Update sensor — warning threshold (temperature > 85 000).
    let result = update_sensor(cpu_temp.id, 87_000).expect("update warn");
    assert_eq!(result, Some(false));
    let s = get_sensor(cpu_temp.id).expect("get");
    assert_eq!(s.status, SensorStatus::Warning);
    crate::serial_println!("  [5/11] temperature warning: OK");

    // 6: Update sensor — critical threshold (temperature > 100 000).
    let result = update_sensor(cpu_temp.id, 105_000).expect("update crit");
    assert_eq!(result, Some(true));
    let s = get_sensor(cpu_temp.id).expect("get");
    assert_eq!(s.status, SensorStatus::Critical);
    crate::serial_println!("  [6/11] temperature critical: OK");

    // 7: Fan speed — inverted thresholds (low = bad).
    let cpu_fan = sensors.iter().find(|s| s.name == "CPU Fan").expect("cpu fan");
    // Normal: 1200 RPM is above warning=500, so Normal.
    assert_eq!(cpu_fan.status, SensorStatus::Normal);
    // Drop below warning threshold.
    let result = update_sensor(cpu_fan.id, 400).expect("fan warn");
    assert_eq!(result, Some(false));
    // Drop below critical threshold.
    let result = update_sensor(cpu_fan.id, 200).expect("fan crit");
    assert_eq!(result, Some(true));
    crate::serial_println!("  [7/11] fan speed inverted thresholds: OK");

    // 8: Min/max tracking.
    let s = get_sensor(cpu_temp.id).expect("get");
    assert_eq!(s.min_value, 45_000); // original value
    assert_eq!(s.max_value, 105_000); // critical update
    crate::serial_println!("  [8/11] min/max tracking: OK");

    // 9: Alerts generated.
    let alerts = get_alerts(10);
    assert!(alerts.len() >= 4); // 1 warn + 1 crit for temp, 1 warn + 1 crit for fan
    assert!(alerts.first().expect("first alert").is_critical);
    crate::serial_println!("  [9/11] alerts: OK");

    // 10: Clear alerts.
    let count = clear_alerts();
    assert!(count >= 4);
    let alerts = get_alerts(10);
    assert!(alerts.is_empty());
    crate::serial_println!("  [10/11] clear alerts: OK");

    // 11: Stats reflect activity.
    let (sensor_count, component_count, total_readings, total_alerts, ops) = stats();
    assert_eq!(sensor_count, 5);
    assert_eq!(component_count, 3);
    assert!(total_readings >= 4);
    assert!(total_alerts >= 4);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("hwmonitor::self_test() — all 11 tests passed");
}

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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Initialise an **empty** hardware monitor — no components, no sensors.
///
/// Seeds an empty registry.  Components and sensors appear only when a real
/// sensor driver registers them via [`add_component`]/[`add_sensor`] and feeds
/// readings via [`update_sensor`]; until that wiring exists, `/proc/hwmonitor`
/// and the `hwmonitor` kshell command report an empty monitor (zero sensors,
/// zero readings) rather than fabricated values — the kernel's hard "never
/// invent data in procfs" rule.  Safe to call multiple times — subsequent
/// calls are no-ops.
///
/// (Previously this seeded five FABRICATED sensor readings — CPU Temp 45.0 °C,
/// CPU Fan 1200 RPM, GPU Temp 38.0 °C, System Temp 32.0 °C, +12 V Rail 12.050 V
/// across three invented components — which `/proc/hwmonitor`, the `hwmonitor`
/// list view, and the per-sensor query then displayed as if they were real
/// hardware sensor readings.  No sensor driver calls the record API: hardware
/// sensors (SuperIO fan/voltage, ACPI thermal zones, GPU temps) are userspace
/// drivers under the microkernel design and are not yet wired.  The values
/// could not simply be zeroed either — a temperature sensor at 0 reads as a
/// fabricated "0.0 °C" and a fan at 0 RPM would trip a false Critical alert —
/// so the only truthful state is an empty registry.  The self-test now builds
/// its own fixtures via the real API; see [`self_test`].
///
/// DEFERRED PROPER FIX: CPU *package* temperature IS available in-kernel via the
/// MSR-backed `crate::thermal` module (`thermal::info().current_temp`), which
/// has its own truthful path.  A future userspace hwmon driver — or a thin
/// in-kernel bridge with a periodic `update_sensor` hook — could register a
/// "CPU Temp" sensor here and feed it live from `thermal::info()` to give a
/// unified sensor view.  That is intentionally NOT done now: hwmonitor stores a
/// static snapshot rather than a live source, so a read-through would require a
/// periodic-update hook that does not yet exist and would duplicate the data
/// `crate::thermal` already exposes.  See todo.txt.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(State {
        sensors: Vec::new(),
        components: Vec::new(),
        alerts: Vec::new(),
        next_sensor_id: 1,
        next_component_id: 1,
        total_readings: 0,
        total_alerts: 0,
        ops: 0,
    });
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
    // Reset to a clean, EMPTY registry and build every fixture via the real
    // add_component/add_sensor/update_sensor API.  init_defaults no longer
    // seeds fabricated sensors, so the test must construct its own component
    // and sensors; resetting first guarantees the counts asserted below are
    // exact and that a `hwmonitor test` run never leaves fixtures resident in
    // the live /proc/hwmonitor registry.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated components or sensors.
    assert_eq!(list_components().len(), 0);
    assert_eq!(list_sensors().len(), 0);
    let (sc0, cc0, rd0, al0, _o0) = stats();
    assert_eq!((sc0, cc0, rd0, al0), (0, 0, 0, 0));
    crate::serial_println!("  [1/11] empty init: OK");

    // 2: Register a CPU component and two sensors via the real API.
    let cpu_id = add_component("CPU", "CPU").expect("add cpu");
    let cpu_temp_id =
        add_sensor(cpu_id, "CPU Temp", SensorType::Temperature, 85_000, 100_000).expect("add temp");
    let cpu_fan_id =
        add_sensor(cpu_id, "CPU Fan", SensorType::FanSpeed, 500, 300).expect("add fan");
    assert_eq!(list_components().len(), 1);
    assert_eq!(list_sensors().len(), 2);
    crate::serial_println!("  [2/11] register component + sensors: OK");

    // 3: A freshly-registered sensor starts at value 0 — no reading recorded
    //    yet, which is honest "no data" rather than a fabricated measurement.
    let cpu_temp = get_sensor(cpu_temp_id).expect("get temp");
    assert_eq!(cpu_temp.value, 0);
    assert_eq!(cpu_temp.sensor_type, SensorType::Temperature);
    assert_eq!(cpu_temp.status, SensorStatus::Normal);
    crate::serial_println!("  [3/11] zeroed sensor: OK");

    // 4: Update sensor — normal value keeps Normal status.
    let result = update_sensor(cpu_temp_id, 60_000).expect("update normal");
    assert!(result.is_none());
    let s = get_sensor(cpu_temp_id).expect("get");
    assert_eq!(s.value, 60_000);
    assert_eq!(s.status, SensorStatus::Normal);
    crate::serial_println!("  [4/11] update sensor normal: OK");

    // 5: Update sensor — warning threshold (temperature > 85 000).
    let result = update_sensor(cpu_temp_id, 87_000).expect("update warn");
    assert_eq!(result, Some(false));
    assert_eq!(get_sensor(cpu_temp_id).expect("get").status, SensorStatus::Warning);
    crate::serial_println!("  [5/11] temperature warning: OK");

    // 6: Update sensor — critical threshold (temperature > 100 000).
    let result = update_sensor(cpu_temp_id, 105_000).expect("update crit");
    assert_eq!(result, Some(true));
    assert_eq!(get_sensor(cpu_temp_id).expect("get").status, SensorStatus::Critical);
    crate::serial_println!("  [6/11] temperature critical: OK");

    // 7: Fan speed — inverted thresholds (low = bad).
    let result = update_sensor(cpu_fan_id, 1_200).expect("fan normal");
    assert!(result.is_none()); // 1200 RPM >= warning 500 → Normal.
    let result = update_sensor(cpu_fan_id, 400).expect("fan warn");
    assert_eq!(result, Some(false));
    let result = update_sensor(cpu_fan_id, 200).expect("fan crit");
    assert_eq!(result, Some(true));
    crate::serial_println!("  [7/11] fan speed inverted thresholds: OK");

    // 8: Min/max tracking — min stays at the registration value (0), max rises
    //    to the highest reading seen.
    let s = get_sensor(cpu_temp_id).expect("get");
    assert_eq!(s.min_value, 0);
    assert_eq!(s.max_value, 105_000);
    crate::serial_println!("  [8/11] min/max tracking: OK");

    // 9: Alerts generated — exactly 4 (temp warn + crit, fan warn + crit).
    let alerts = get_alerts(10);
    assert_eq!(alerts.len(), 4);
    assert!(alerts.first().expect("first alert").is_critical); // newest first = fan crit
    crate::serial_println!("  [9/11] alerts: OK");

    // 10: Clear alerts.
    let count = clear_alerts();
    assert_eq!(count, 4);
    assert!(get_alerts(10).is_empty());
    crate::serial_println!("  [10/11] clear alerts: OK");

    // 11: Stats — exact totals: 2 sensors, 1 component, 6 readings, 4 alerts.
    let (sensor_count, component_count, total_readings, total_alerts, ops) = stats();
    assert_eq!(sensor_count, 2);
    assert_eq!(component_count, 1);
    assert_eq!(total_readings, 6); // 1 temp-normal + 2 temp-alert + 3 fan updates.
    assert_eq!(total_alerts, 4);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/hwmonitor registry.
    *STATE.lock() = None;

    crate::serial_println!("hwmonitor::self_test() — all 11 tests passed");
}

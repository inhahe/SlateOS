//! Disk S.M.A.R.T. — disk health monitoring and failure prediction.
//!
//! Reads Self-Monitoring, Analysis, and Reporting Technology data from
//! storage devices to detect degradation and predict failures before
//! data loss occurs.
//!
//! ## Architecture
//!
//! ```text
//! Storage driver (NVMe/SATA)
//!   → disksmart::update_attributes(device, attrs)
//!
//! Settings panel → Storage → Health
//!   → disksmart::health_summary(device)
//!
//! Notification system
//!   → disksmart::check_thresholds() → warning/critical alerts
//!
//! Integration:
//!   → devicemgr (storage device list)
//!   → sysdiag (system diagnostics)
//!   → notifcenter (health alerts)
//!   → backup (trigger backup on degradation)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Overall health assessment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Good,
    Caution,
    Warning,
    Critical,
    Unknown,
}

impl HealthStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Good => "Good",
            Self::Caution => "Caution",
            Self::Warning => "Warning",
            Self::Critical => "Critical",
            Self::Unknown => "Unknown",
        }
    }
}

/// Drive interface type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveInterface {
    Sata,
    Nvme,
    Usb,
    Scsi,
    Virtual,
}

impl DriveInterface {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sata => "SATA",
            Self::Nvme => "NVMe",
            Self::Usb => "USB",
            Self::Scsi => "SCSI",
            Self::Virtual => "Virtual",
        }
    }
}

/// A S.M.A.R.T. attribute.
#[derive(Debug, Clone)]
pub struct SmartAttribute {
    /// Attribute ID (standard SMART IDs: 5=reallocated, 9=power-on hours, etc.).
    pub id: u8,
    /// Attribute name.
    pub name: String,
    /// Current value (0-253, higher is better for most).
    pub current: u16,
    /// Worst ever value.
    pub worst: u16,
    /// Threshold (below = failing).
    pub threshold: u16,
    /// Raw value (interpretation varies).
    pub raw_value: u64,
    /// Whether this attribute is pre-fail (true) or old-age (false).
    pub pre_fail: bool,
}

/// A monitored drive.
#[derive(Debug, Clone)]
pub struct SmartDrive {
    /// Device path (e.g., "/dev/sda").
    pub device: String,
    /// Model name.
    pub model: String,
    /// Serial number.
    pub serial: String,
    /// Firmware version.
    pub firmware: String,
    /// Interface type.
    pub interface: DriveInterface,
    /// Capacity in bytes.
    pub capacity_bytes: u64,
    /// Overall health.
    pub health: HealthStatus,
    /// Temperature in Celsius.
    pub temperature_c: i16,
    /// Power-on hours.
    pub power_on_hours: u64,
    /// Power cycle count.
    pub power_cycles: u32,
    /// Reallocated sector count.
    pub reallocated_sectors: u32,
    /// Pending sectors.
    pub pending_sectors: u32,
    /// Wear level percentage (0-100, for SSDs).
    pub wear_level_pct: u8,
    /// S.M.A.R.T. attributes.
    pub attributes: Vec<SmartAttribute>,
    /// Self-test running.
    pub self_test_running: bool,
    /// Last self-test result.
    pub last_test_result: String,
}

/// Alert configuration.
#[derive(Debug, Clone)]
pub struct AlertConfig {
    pub enabled: bool,
    pub temp_warning_c: i16,
    pub temp_critical_c: i16,
    pub wear_warning_pct: u8,
    pub reallocated_warning: u32,
    pub check_interval_secs: u64,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            temp_warning_c: 50,
            temp_critical_c: 60,
            wear_warning_pct: 80,
            reallocated_warning: 10,
            check_interval_secs: 3600,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DRIVES: usize = 32;
const MAX_ATTRS_PER_DRIVE: usize = 30;

struct State {
    drives: Vec<SmartDrive>,
    config: AlertConfig,
    total_checks: u64,
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

/// Evaluate health based on attributes.
fn evaluate_health(drive: &SmartDrive) -> HealthStatus {
    if drive.reallocated_sectors > 100 || drive.pending_sectors > 50 || drive.wear_level_pct > 95 {
        return HealthStatus::Critical;
    }
    if drive.reallocated_sectors > 10 || drive.pending_sectors > 5 || drive.wear_level_pct > 80 {
        return HealthStatus::Warning;
    }
    if drive.reallocated_sectors > 0 || drive.pending_sectors > 0 || drive.wear_level_pct > 60 {
        return HealthStatus::Caution;
    }
    if drive.temperature_c > 60 {
        return HealthStatus::Warning;
    }
    if drive.temperature_c > 50 {
        return HealthStatus::Caution;
    }
    HealthStatus::Good
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let default_attrs = alloc::vec![
        SmartAttribute { id: 1, name: String::from("Raw Read Error Rate"), current: 200, worst: 200, threshold: 51, raw_value: 0, pre_fail: true },
        SmartAttribute { id: 5, name: String::from("Reallocated Sector Count"), current: 200, worst: 200, threshold: 140, raw_value: 0, pre_fail: true },
        SmartAttribute { id: 9, name: String::from("Power-On Hours"), current: 98, worst: 98, threshold: 0, raw_value: 4380, pre_fail: false },
        SmartAttribute { id: 194, name: String::from("Temperature"), current: 113, worst: 100, threshold: 0, raw_value: 37, pre_fail: false },
        SmartAttribute { id: 197, name: String::from("Current Pending Sector"), current: 200, worst: 200, threshold: 0, raw_value: 0, pre_fail: false },
    ];

    let drives = alloc::vec![
        SmartDrive {
            device: String::from("/dev/sda"),
            model: String::from("Virtual SATA SSD 512GB"),
            serial: String::from("VSSD-001"),
            firmware: String::from("1.0.0"),
            interface: DriveInterface::Sata,
            capacity_bytes: 512 * 1024 * 1024 * 1024,
            health: HealthStatus::Good,
            temperature_c: 37,
            power_on_hours: 4380,
            power_cycles: 1200,
            reallocated_sectors: 0,
            pending_sectors: 0,
            wear_level_pct: 12,
            attributes: default_attrs,
            self_test_running: false,
            last_test_result: String::from("PASSED"),
        },
    ];

    *guard = Some(State {
        drives,
        config: AlertConfig::default(),
        total_checks: 0,
        total_alerts: 0,
        ops: 0,
    });
}

/// Register a drive for monitoring.
pub fn register_drive(
    device: &str, model: &str, serial: &str, interface: DriveInterface, capacity_bytes: u64,
) -> KernelResult<()> {
    with_state(|state| {
        if state.drives.iter().any(|d| d.device == device) {
            return Err(KernelError::AlreadyExists);
        }
        if state.drives.len() >= MAX_DRIVES {
            return Err(KernelError::ResourceExhausted);
        }
        state.drives.push(SmartDrive {
            device: String::from(device),
            model: String::from(model),
            serial: String::from(serial),
            firmware: String::from("unknown"),
            interface,
            capacity_bytes,
            health: HealthStatus::Unknown,
            temperature_c: 0,
            power_on_hours: 0,
            power_cycles: 0,
            reallocated_sectors: 0,
            pending_sectors: 0,
            wear_level_pct: 0,
            attributes: Vec::new(),
            self_test_running: false,
            last_test_result: String::new(),
        });
        Ok(())
    })
}

/// Remove a drive from monitoring.
pub fn unregister_drive(device: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.drives.iter().position(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        state.drives.remove(pos);
        Ok(())
    })
}

/// Update drive temperature.
pub fn set_temperature(device: &str, temp_c: i16) -> KernelResult<()> {
    with_state(|state| {
        let drive = state.drives.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        drive.temperature_c = temp_c;
        drive.health = evaluate_health(drive);
        Ok(())
    })
}

/// Update reallocated sector count.
pub fn set_reallocated(device: &str, count: u32) -> KernelResult<()> {
    with_state(|state| {
        let drive = state.drives.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        drive.reallocated_sectors = count;
        drive.health = evaluate_health(drive);
        Ok(())
    })
}

/// Update wear level percentage.
pub fn set_wear_level(device: &str, pct: u8) -> KernelResult<()> {
    with_state(|state| {
        let drive = state.drives.iter_mut().find(|d| d.device == device)
            .ok_or(KernelError::NotFound)?;
        drive.wear_level_pct = pct.min(100);
        drive.health = evaluate_health(drive);
        Ok(())
    })
}

/// Get drive info.
pub fn get_drive(device: &str) -> KernelResult<SmartDrive> {
    with_state(|state| {
        state.drives.iter().find(|d| d.device == device).cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List all monitored drives.
pub fn list_drives() -> Vec<SmartDrive> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.drives.clone())
}

/// Check all drives against thresholds, return number of alerts.
pub fn check_thresholds() -> u32 {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return 0,
    };
    state.total_checks += 1;
    state.ops += 1;

    let mut alerts = 0u32;
    for drive in &state.drives {
        if drive.temperature_c >= state.config.temp_critical_c {
            alerts += 1;
        } else if drive.temperature_c >= state.config.temp_warning_c {
            alerts += 1;
        }
        if drive.reallocated_sectors >= state.config.reallocated_warning {
            alerts += 1;
        }
        if drive.wear_level_pct >= state.config.wear_warning_pct {
            alerts += 1;
        }
    }
    state.total_alerts += alerts as u64;
    alerts
}

/// Get alert configuration.
pub fn get_alert_config() -> AlertConfig {
    STATE.lock().as_ref().map_or(AlertConfig::default(), |s| s.config.clone())
}

/// Statistics: (drive_count, good_count, warning_count, total_checks, total_alerts, ops).
pub fn stats() -> (usize, usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let good = s.drives.iter().filter(|d| d.health == HealthStatus::Good).count();
            let warn = s.drives.iter().filter(|d| matches!(d.health, HealthStatus::Warning | HealthStatus::Critical | HealthStatus::Caution)).count();
            (s.drives.len(), good, warn, s.total_checks, s.total_alerts, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("disksmart::self_test() — running tests...");
    init_defaults();

    // 1: Default drive exists.
    let drives = list_drives();
    assert_eq!(drives.len(), 1);
    assert_eq!(drives[0].device, "/dev/sda");
    crate::serial_println!("  [1/11] default drive: OK");

    // 2: Default health is good.
    let d = get_drive("/dev/sda").expect("get sda");
    assert_eq!(d.health, HealthStatus::Good);
    crate::serial_println!("  [2/11] default health good: OK");

    // 3: Register new drive.
    register_drive("/dev/nvme0n1", "Test NVMe 1TB", "NV-001", DriveInterface::Nvme, 1024 * 1024 * 1024 * 1024).expect("register");
    assert_eq!(list_drives().len(), 2);
    crate::serial_println!("  [3/11] register drive: OK");

    // 4: Duplicate rejected.
    let r = register_drive("/dev/nvme0n1", "Dup", "Dup", DriveInterface::Nvme, 0);
    assert!(r.is_err());
    crate::serial_println!("  [4/11] duplicate rejected: OK");

    // 5: Temperature update.
    set_temperature("/dev/sda", 55).expect("set temp");
    let d = get_drive("/dev/sda").expect("get sda");
    assert_eq!(d.temperature_c, 55);
    assert_eq!(d.health, HealthStatus::Caution);
    crate::serial_println!("  [5/11] temperature warning: OK");

    // 6: Reallocated sectors.
    set_reallocated("/dev/sda", 15).expect("set realloc");
    let d = get_drive("/dev/sda").expect("get sda");
    assert_eq!(d.health, HealthStatus::Warning);
    crate::serial_println!("  [6/11] reallocated sectors: OK");

    // 7: Wear level.
    set_wear_level("/dev/nvme0n1", 50).expect("set wear");
    let d = get_drive("/dev/nvme0n1").expect("get nvme");
    assert_eq!(d.wear_level_pct, 50);
    crate::serial_println!("  [7/11] wear level: OK");

    // 8: Check thresholds.
    let alerts = check_thresholds();
    assert!(alerts >= 1); // At least temp warning on sda.
    crate::serial_println!("  [8/11] check thresholds: OK");

    // 9: Unregister drive.
    unregister_drive("/dev/nvme0n1").expect("unregister");
    assert_eq!(list_drives().len(), 1);
    crate::serial_println!("  [9/11] unregister: OK");

    // 10: Not-found error.
    let r = get_drive("/dev/nonexistent");
    assert!(r.is_err());
    crate::serial_println!("  [10/11] not found error: OK");

    // 11: Stats.
    let (total, good, warn, checks, alerts, ops) = stats();
    assert_eq!(total, 1);
    assert!(checks >= 1);
    assert!(ops > 0);
    let _ = (good, warn, alerts);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("disksmart::self_test() — all 11 tests passed");
}

//! Disk Health — storage device health monitoring and prediction.
//!
//! Monitors disk health via S.M.A.R.T. attributes, temperature,
//! error rates, and provides failure prediction.
//!
//! ## Architecture
//!
//! ```text
//! Disk health monitoring
//!   → diskhealth::check(device) → health report
//!   → diskhealth::get_temperature(device) → current temp
//!   → diskhealth::predict_failure(device) → failure risk
//!
//! Integration:
//!   → disksmart (SMART data)
//!   → raidmgr (RAID arrays)
//!   → partmgr (partition management)
//!   → eventlog (event logging)
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

/// Overall health grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthGrade {
    Excellent,
    Good,
    Fair,
    Poor,
    Critical,
    Unknown,
}

impl HealthGrade {
    pub fn label(self) -> &'static str {
        match self {
            Self::Excellent => "Excellent",
            Self::Good => "Good",
            Self::Fair => "Fair",
            Self::Poor => "Poor",
            Self::Critical => "Critical",
            Self::Unknown => "Unknown",
        }
    }
}

/// Disk type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskType {
    Hdd,
    Ssd,
    Nvme,
    Usb,
    Unknown,
}

impl DiskType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hdd => "HDD",
            Self::Ssd => "SSD",
            Self::Nvme => "NVMe",
            Self::Usb => "USB",
            Self::Unknown => "Unknown",
        }
    }
}

/// A monitored disk.
#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub id: u32,
    pub device_name: String,
    pub model: String,
    pub serial: String,
    pub disk_type: DiskType,
    pub capacity_bytes: u64,
    pub health: HealthGrade,
    pub temperature_c: u32,
    pub power_on_hours: u64,
    pub read_error_rate: u64,
    pub write_error_rate: u64,
    pub reallocated_sectors: u64,
    pub remaining_life_pct: u32,
    pub last_check_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DISKS: usize = 32;

struct State {
    disks: Vec<DiskInfo>,
    next_id: u32,
    total_checks: u64,
    total_warnings: u64,
    total_failures_predicted: u64,
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

fn compute_health(disk: &DiskInfo) -> HealthGrade {
    if disk.reallocated_sectors > 100 || disk.remaining_life_pct < 5 {
        HealthGrade::Critical
    } else if disk.reallocated_sectors > 20 || disk.remaining_life_pct < 20 || disk.temperature_c > 60 {
        HealthGrade::Poor
    } else if disk.reallocated_sectors > 5 || disk.remaining_life_pct < 50 || disk.temperature_c > 50 {
        HealthGrade::Fair
    } else if disk.remaining_life_pct < 80 || disk.temperature_c > 40 {
        HealthGrade::Good
    } else {
        HealthGrade::Excellent
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise an **empty** disk-health table.
///
/// Seeds NO disks and zero counters.  Real disks are wired through [`add_disk`]
/// (one row per storage device the SMART/storage layer enumerates) and their
/// SMART attributes through [`update_attrs`]; until those are called the table is
/// genuinely empty, so `/proc/diskhealth` and the `diskhealth` kshell command
/// report nothing rather than fabricated numbers — the kernel's hard "never
/// invent data in procfs" rule.
///
/// NOTE: this previously seeded two fictional disks with INVENTED model and
/// serial numbers presented as real hardware ("sda": "WDC WD10EZEX" HDD / serial
/// "WD-XXXX1234" / 1 TB / Good / 35°C / 12000 power-on hours / 100% life;
/// "nvme0": "Samsung 970 EVO" NVMe / serial "S4XX1234" / 500 GB / Excellent /
/// 32°C / 5000 hours / 95% life), which `/proc/diskhealth` (and the
/// `list_disks`/`get_disk`/`check_health` views) then displayed as if they were
/// genuine S.M.A.R.T. readings from attached drives.  That demo data was removed;
/// the self-test now builds its own fixtures explicitly via the real API (see
/// [`self_test`]).  The storage layer is expected to call [`add_disk`] when a
/// drive is enumerated and [`update_attrs`] when SMART data is polled.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        disks: Vec::new(),
        next_id: 1,
        total_checks: 0,
        total_warnings: 0,
        total_failures_predicted: 0,
        ops: 0,
    });
}

/// Add a disk to monitor.
pub fn add_disk(name: &str, model: &str, serial: &str, dtype: DiskType, capacity: u64) -> KernelResult<u32> {
    with_state(|state| {
        if state.disks.len() >= MAX_DISKS {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.disks.push(DiskInfo {
            id, device_name: String::from(name), model: String::from(model),
            serial: String::from(serial), disk_type: dtype, capacity_bytes: capacity,
            health: HealthGrade::Unknown, temperature_c: 0, power_on_hours: 0,
            read_error_rate: 0, write_error_rate: 0, reallocated_sectors: 0,
            remaining_life_pct: 100, last_check_ns: now,
        });
        Ok(id)
    })
}

/// Remove a monitored disk.
pub fn remove_disk(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.disks.len();
        state.disks.retain(|d| d.id != id);
        if state.disks.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Run a health check on a disk.
pub fn check_health(id: u32) -> KernelResult<HealthGrade> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let disk = state.disks.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        disk.health = compute_health(disk);
        disk.last_check_ns = now;
        state.total_checks += 1;
        if disk.health == HealthGrade::Poor || disk.health == HealthGrade::Critical {
            state.total_warnings += 1;
        }
        if disk.health == HealthGrade::Critical {
            state.total_failures_predicted += 1;
        }
        Ok(disk.health)
    })
}

/// Update SMART attributes.
pub fn update_attrs(id: u32, temp: u32, power_hours: u64, read_err: u64, write_err: u64, realloc: u64, life_pct: u32) -> KernelResult<()> {
    with_state(|state| {
        let disk = state.disks.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        disk.temperature_c = temp;
        disk.power_on_hours = power_hours;
        disk.read_error_rate = read_err;
        disk.write_error_rate = write_err;
        disk.reallocated_sectors = realloc;
        disk.remaining_life_pct = life_pct;
        Ok(())
    })
}

/// Get disk info.
pub fn get_disk(id: u32) -> Option<DiskInfo> {
    STATE.lock().as_ref().and_then(|s| s.disks.iter().find(|d| d.id == id).cloned())
}

/// List all disks.
pub fn list_disks() -> Vec<DiskInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.disks.clone())
}

/// Statistics: (disk_count, total_checks, total_warnings, total_failures_predicted, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.disks.len(), s.total_checks, s.total_warnings, s.total_failures_predicted, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("diskhealth::self_test() — running tests...");
    // Begin from a clean, EMPTY table and build every fixture via the real API,
    // so the test exercises genuine accounting paths and never relies on
    // fabricated seed data (which /proc/diskhealth must never surface).  Resetting
    // first clears any residue from a prior `diskhealth test` run so the totals
    // asserted below are exact.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty after init — no fabricated disks or counters; check on an
    // unknown id fails.
    assert_eq!(list_disks().len(), 0);
    let (c0, chk0, w0, f0, _o0) = stats();
    assert_eq!((c0, chk0, w0, f0), (0, 0, 0, 0));
    assert!(check_health(1).is_err()); // no phantom disk exists yet
    crate::serial_println!("  [1/8] empty init: OK");

    // 2: Add a disk — monotonic id, Unknown health until first check.
    let id = add_disk("sdb", "TEST-MODEL", "TEST-SERIAL", DiskType::Hdd, 1_000_000_000_000).expect("add");
    assert_eq!(list_disks().len(), 1);
    let d = get_disk(id).expect("get");
    assert_eq!(d.health, HealthGrade::Unknown);
    assert_eq!(d.capacity_bytes, 1_000_000_000_000);
    crate::serial_println!("  [2/8] add disk: OK");

    // 3: Healthy attrs → Excellent (no realloc, full life, cool).
    update_attrs(id, 30, 100, 0, 0, 0, 100).expect("update healthy");
    assert_eq!(check_health(id).expect("check healthy"), HealthGrade::Excellent);
    crate::serial_println!("  [3/8] excellent health: OK");

    // 4: Poor attrs (55°C, 30 realloc, 40% life) → Poor; bumps warnings.
    update_attrs(id, 55, 50000, 100, 50, 30, 40).expect("update poor");
    assert_eq!(check_health(id).expect("check poor"), HealthGrade::Poor);
    crate::serial_println!("  [4/8] poor health: OK");

    // 5: Critical attrs (200 realloc, 3% life) → Critical; bumps warnings +
    // failures-predicted.
    update_attrs(id, 65, 80000, 500, 200, 200, 3).expect("update crit");
    assert_eq!(check_health(id).expect("check crit"), HealthGrade::Critical);
    crate::serial_println!("  [5/8] critical health: OK");

    // 6: get_disk reflects the updated SMART attributes exactly.
    let info = get_disk(id).expect("get");
    assert_eq!(info.temperature_c, 65);
    assert_eq!(info.reallocated_sectors, 200);
    assert_eq!(info.remaining_life_pct, 3);
    crate::serial_println!("  [6/8] disk info: OK");

    // 7: Remove — row gone; second remove + later check fail.
    remove_disk(id).expect("remove");
    assert_eq!(list_disks().len(), 0);
    assert!(remove_disk(id).is_err());
    assert!(check_health(id).is_err());
    assert!(update_attrs(id, 0, 0, 0, 0, 0, 0).is_err());
    crate::serial_println!("  [7/8] remove + not found: OK");

    // 8: Aggregate totals are exact: 3 checks (Excellent, Poor, Critical), 2
    // warnings (Poor + Critical), 1 failure predicted (Critical only).
    let (count, checks, warnings, failures, ops) = stats();
    assert_eq!(count, 0);
    assert_eq!(checks, 3);
    assert_eq!(warnings, 2);
    assert_eq!(failures, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave NO residue: reset to the uninitialised state so a diagnostic run
    // never leaves fixtures resident in the live /proc/diskhealth table.
    *STATE.lock() = None;

    crate::serial_println!("diskhealth::self_test() — all 8 tests passed");
}

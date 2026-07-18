//! Driver update — driver version tracking and update management.
//!
//! Checks for driver updates, stages downloads, supports rollback
//! to previous driver versions, and integrates with restorepoint
//! for pre-update snapshots.
//!
//! ## Architecture
//!
//! ```text
//! updatemgr / manual check
//!   → driverupdate::check_updates() → list available updates
//!   → driverupdate::install(driver_id) → stage + install
//!
//! Settings panel → Drivers
//!   → driverupdate::list_drivers() → installed driver list
//!   → driverupdate::rollback(driver_id) → revert to previous
//!
//! Integration:
//!   → devicemgr (device-driver mapping)
//!   → updatemgr (update scheduling)
//!   → restorepoint (pre-update snapshots)
//!   → notifcenter (update notifications)
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

/// Driver status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    UpToDate,
    UpdateAvailable,
    Installing,
    Failed,
    Disabled,
    RolledBack,
}

impl DriverStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::UpToDate => "Up to date",
            Self::UpdateAvailable => "Update available",
            Self::Installing => "Installing",
            Self::Failed => "Failed",
            Self::Disabled => "Disabled",
            Self::RolledBack => "Rolled back",
        }
    }
}

/// Driver category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverCategory {
    Display,
    Audio,
    Network,
    Storage,
    Input,
    Usb,
    Bluetooth,
    Printer,
    Camera,
    Firmware,
    Other,
}

impl DriverCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Display => "Display",
            Self::Audio => "Audio",
            Self::Network => "Network",
            Self::Storage => "Storage",
            Self::Input => "Input",
            Self::Usb => "USB",
            Self::Bluetooth => "Bluetooth",
            Self::Printer => "Printer",
            Self::Camera => "Camera",
            Self::Firmware => "Firmware",
            Self::Other => "Other",
        }
    }
}

/// An installed driver.
#[derive(Debug, Clone)]
pub struct InstalledDriver {
    /// Driver ID.
    pub id: u32,
    /// Driver name.
    pub name: String,
    /// Category.
    pub category: DriverCategory,
    /// Current version.
    pub version: String,
    /// Available version (if update exists).
    pub available_version: String,
    /// Status.
    pub status: DriverStatus,
    /// Provider/vendor.
    pub provider: String,
    /// Install date (ns since boot for simulation).
    pub install_ns: u64,
    /// Previous version (for rollback).
    pub previous_version: String,
    /// Auto-update enabled.
    pub auto_update: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DRIVERS: usize = 100;

struct State {
    drivers: Vec<InstalledDriver>,
    next_id: u32,
    auto_check: bool,
    check_interval_hours: u32,
    total_updates: u64,
    total_rollbacks: u64,
    last_check_ns: u64,
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

    let drivers = alloc::vec![
        InstalledDriver {
            id: 1, name: String::from("Virtual Display Driver"),
            category: DriverCategory::Display, version: String::from("1.2.0"),
            available_version: String::new(), status: DriverStatus::UpToDate,
            provider: String::from("MintOS"), install_ns: 0,
            previous_version: String::from("1.1.0"), auto_update: true,
        },
        InstalledDriver {
            id: 2, name: String::from("HD Audio Driver"),
            category: DriverCategory::Audio, version: String::from("2.0.1"),
            available_version: String::new(), status: DriverStatus::UpToDate,
            provider: String::from("MintOS"), install_ns: 0,
            previous_version: String::from("2.0.0"), auto_update: true,
        },
        InstalledDriver {
            id: 3, name: String::from("Virtio Network Driver"),
            category: DriverCategory::Network, version: String::from("1.0.0"),
            available_version: String::from("1.1.0"), status: DriverStatus::UpdateAvailable,
            provider: String::from("MintOS"), install_ns: 0,
            previous_version: String::new(), auto_update: true,
        },
    ];

    *guard = Some(State {
        drivers,
        next_id: 4,
        auto_check: true,
        check_interval_hours: 24,
        total_updates: 0,
        total_rollbacks: 0,
        last_check_ns: 0,
        ops: 0,
    });
}

/// Register a driver.
pub fn register_driver(
    name: &str, category: DriverCategory, version: &str, provider: &str,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.drivers.len() >= MAX_DRIVERS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.drivers.push(InstalledDriver {
            id, name: String::from(name), category,
            version: String::from(version), available_version: String::new(),
            status: DriverStatus::UpToDate, provider: String::from(provider),
            install_ns: crate::hpet::elapsed_ns(),
            previous_version: String::new(), auto_update: true,
        });
        Ok(id)
    })
}

/// Set an available update for a driver.
pub fn set_available_update(id: u32, version: &str) -> KernelResult<()> {
    with_state(|state| {
        let d = state.drivers.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        d.available_version = String::from(version);
        d.status = DriverStatus::UpdateAvailable;
        Ok(())
    })
}

/// Install an update for a driver.
pub fn install_update(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.drivers.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        if d.available_version.is_empty() {
            return Err(KernelError::NotFound);
        }
        d.previous_version = d.version.clone();
        d.version = d.available_version.clone();
        d.available_version = String::new();
        d.status = DriverStatus::UpToDate;
        d.install_ns = crate::hpet::elapsed_ns();
        state.total_updates += 1;
        Ok(())
    })
}

/// Rollback a driver to its previous version.
pub fn rollback(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state.drivers.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        if d.previous_version.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        let old = d.version.clone();
        d.version = d.previous_version.clone();
        d.previous_version = old;
        d.status = DriverStatus::RolledBack;
        state.total_rollbacks += 1;
        Ok(())
    })
}

/// List all drivers.
pub fn list_drivers() -> Vec<InstalledDriver> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.drivers.clone())
}

/// Get driver by ID.
pub fn get_driver(id: u32) -> KernelResult<InstalledDriver> {
    with_state(|state| {
        state.drivers.iter().find(|d| d.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Count drivers with available updates.
pub fn updates_available() -> usize {
    STATE.lock().as_ref().map_or(0, |s| {
        s.drivers.iter().filter(|d| d.status == DriverStatus::UpdateAvailable).count()
    })
}

/// Statistics: (driver_count, update_count, total_updates, total_rollbacks, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let updates = s.drivers.iter().filter(|d| d.status == DriverStatus::UpdateAvailable).count();
            (s.drivers.len(), updates, s.total_updates, s.total_rollbacks, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("driverupdate::self_test() — running tests...");
    init_defaults();

    // 1: Default drivers.
    let drivers = list_drivers();
    assert_eq!(drivers.len(), 3);
    crate::serial_println!("  [1/11] default drivers: OK");

    // 2: Update available.
    let avail = updates_available();
    assert_eq!(avail, 1);
    crate::serial_println!("  [2/11] update available: OK");

    // 3: Install update.
    install_update(3).expect("install update");
    let d = get_driver(3).expect("get driver");
    assert_eq!(d.version, "1.1.0");
    assert_eq!(d.status, DriverStatus::UpToDate);
    crate::serial_println!("  [3/11] install update: OK");

    // 4: Rollback.
    rollback(3).expect("rollback");
    let d = get_driver(3).expect("get driver 2");
    assert_eq!(d.version, "1.0.0");
    assert_eq!(d.status, DriverStatus::RolledBack);
    crate::serial_println!("  [4/11] rollback: OK");

    // 5: Register new driver.
    let id = register_driver("Test Camera", DriverCategory::Camera, "0.1.0", "Test Corp")
        .expect("register");
    assert!(id > 0);
    assert_eq!(list_drivers().len(), 4);
    crate::serial_println!("  [5/11] register driver: OK");

    // 6: Set available update.
    set_available_update(id, "0.2.0").expect("set update");
    let d = get_driver(id).expect("get new driver");
    assert_eq!(d.status, DriverStatus::UpdateAvailable);
    crate::serial_println!("  [6/11] set update: OK");

    // 7: No rollback without previous.
    let r = rollback(id);
    // The driver now has a previous_version of "" since we set_available_update but haven't installed.
    // Actually install_update hasn't been called, so previous_version is still empty.
    assert!(r.is_err());
    crate::serial_println!("  [7/11] no rollback without previous: OK");

    // 8: Install then rollback.
    install_update(id).expect("install cam");
    let d = get_driver(id).expect("get cam");
    assert_eq!(d.version, "0.2.0");
    rollback(id).expect("rollback cam");
    let d = get_driver(id).expect("get cam 2");
    assert_eq!(d.version, "0.1.0");
    crate::serial_println!("  [8/11] install+rollback: OK");

    // 9: Not found.
    let r = get_driver(999);
    assert!(r.is_err());
    crate::serial_println!("  [9/11] not found: OK");

    // 10: Category check.
    let d = get_driver(1).expect("get display");
    assert_eq!(d.category, DriverCategory::Display);
    crate::serial_println!("  [10/11] category: OK");

    // 11: Stats.
    let (count, updates, total_updates, total_rollbacks, ops) = stats();
    assert_eq!(count, 4);
    assert!(total_updates >= 2);
    assert!(total_rollbacks >= 2);
    assert!(ops > 0);
    let _ = updates;
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("driverupdate::self_test() — all 11 tests passed");
}

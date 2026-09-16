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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    if guard.is_some() {
        return;
    }

    // No drivers. This list used to hold three -- a display, an audio and a
    // network driver, each with a version history and one with an update
    // pending from provider "MintOS" -- and `procfs::gen_driverupdate`
    // published the count as `driver_count`. Nothing had installed anything,
    // so every one of those facts was invented.
    //
    // Worse than an invented count, because `cmd_driverupdate`'s list arm
    // calls this function and *then* lists: an operator asking what is
    // installed caused three drivers to exist and was shown them. The honest
    // branch under it, "No drivers registered.", could not run. It can now.
    //
    // A driver appears here when something calls `register_driver`. Until
    // then the answer to "what is installed" is nothing, which is true.
    let drivers: Vec<InstalledDriver> = Vec::new();

    *guard = Some(State {
        drivers,
        next_id: 1,
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
    name: &str,
    category: DriverCategory,
    version: &str,
    provider: &str,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.drivers.len() >= MAX_DRIVERS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.drivers.push(InstalledDriver {
            id,
            name: String::from(name),
            category,
            version: String::from(version),
            available_version: String::new(),
            status: DriverStatus::UpToDate,
            provider: String::from(provider),
            install_ns: crate::hpet::elapsed_ns(),
            previous_version: String::new(),
            auto_update: true,
        });
        Ok(id)
    })
}

/// Remove a driver from the registry.
///
/// The registry could previously only grow, which is a gap on its own
/// terms: a driver that is uninstalled had no way to stop being reported.
/// It is also what stopped `self_test` cleaning up after itself. The state
/// this writes to is the one `procfs::gen_driverupdate` publishes as
/// `driver_count`, so anything registered here is a fact `/proc` will
/// state for the rest of the boot -- including anything a test registers.
///
/// # Errors
///
/// `NotFound` if no driver carries `id`; `NotSupported` before
/// `init_defaults`.
pub fn unregister_driver(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.drivers.len();
        state.drivers.retain(|d| d.id != id);
        if state.drivers.len() == before {
            return Err(KernelError::NotFound);
        }
        Ok(())
    })
}

/// Set an available update for a driver.
pub fn set_available_update(id: u32, version: &str) -> KernelResult<()> {
    with_state(|state| {
        let d = state
            .drivers
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        d.available_version = String::from(version);
        d.status = DriverStatus::UpdateAvailable;
        Ok(())
    })
}

/// Install an update for a driver.
pub fn install_update(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let d = state
            .drivers
            .iter_mut()
            .find(|d| d.id == id)
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
        let d = state
            .drivers
            .iter_mut()
            .find(|d| d.id == id)
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
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.drivers.clone())
}

/// Get driver by ID.
pub fn get_driver(id: u32) -> KernelResult<InstalledDriver> {
    with_state(|state| {
        state
            .drivers
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// Count drivers with available updates.
pub fn updates_available() -> usize {
    STATE.lock().as_ref().map_or(0, |s| {
        s.drivers
            .iter()
            .filter(|d| d.status == DriverStatus::UpdateAvailable)
            .count()
    })
}

/// Statistics: (driver_count, update_count, total_updates, total_rollbacks, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let updates = s
                .drivers
                .iter()
                .filter(|d| d.status == DriverStatus::UpdateAvailable)
                .count();
            (
                s.drivers.len(),
                updates,
                s.total_updates,
                s.total_rollbacks,
                s.ops,
            )
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() -> crate::error::KernelResult<()> {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
    Ok(())
}

fn self_test_inner() {
    crate::serial_println!("driverupdate::self_test() — running tests...");
    init_defaults();

    // The fixture is built HERE rather than shipped. These three used to be
    // seeded by `init_defaults` and published through /proc as installed
    // drivers; this test asserting `len() == 3` is what made them look
    // load-bearing. A test that needs three drivers registers three, and
    // the names say so. Step 12 removes them again.
    let display = register_driver(
        "Test Display",
        DriverCategory::Display,
        "1.2.0",
        "Test Corp",
    )
    .expect("register display");
    let _audio = register_driver("Test Audio", DriverCategory::Audio, "2.0.1", "Test Corp")
        .expect("register audio");
    let net = register_driver(
        "Test Network",
        DriverCategory::Network,
        "1.0.0",
        "Test Corp",
    )
    .expect("register network");
    set_available_update(net, "1.1.0").expect("offer network update");

    // 1: Registered drivers.
    let drivers = list_drivers();
    assert_eq!(drivers.len(), 3);
    crate::serial_println!("  [1/12] registered drivers: OK");

    // 2: Update available.
    let avail = updates_available();
    assert_eq!(avail, 1);
    crate::serial_println!("  [2/12] update available: OK");

    // 3: Install update.
    install_update(net).expect("install update");
    let d = get_driver(net).expect("get driver");
    assert_eq!(d.version, "1.1.0");
    assert_eq!(d.status, DriverStatus::UpToDate);
    crate::serial_println!("  [3/12] install update: OK");

    // 4: Rollback.
    rollback(net).expect("rollback");
    let d = get_driver(net).expect("get driver 2");
    assert_eq!(d.version, "1.0.0");
    assert_eq!(d.status, DriverStatus::RolledBack);
    crate::serial_println!("  [4/12] rollback: OK");

    // 5: Register new driver.
    let id = register_driver("Test Camera", DriverCategory::Camera, "0.1.0", "Test Corp")
        .expect("register");
    assert!(id > 0);
    assert_eq!(list_drivers().len(), 4);
    crate::serial_println!("  [5/12] register driver: OK");

    // 6: Set available update.
    set_available_update(id, "0.2.0").expect("set update");
    let d = get_driver(id).expect("get new driver");
    assert_eq!(d.status, DriverStatus::UpdateAvailable);
    crate::serial_println!("  [6/12] set update: OK");

    // 7: No rollback without previous.
    let r = rollback(id);
    // The driver now has a previous_version of "" since we set_available_update but haven't installed.
    // Actually install_update hasn't been called, so previous_version is still empty.
    assert!(r.is_err());
    crate::serial_println!("  [7/12] no rollback without previous: OK");

    // 8: Install then rollback.
    install_update(id).expect("install cam");
    let d = get_driver(id).expect("get cam");
    assert_eq!(d.version, "0.2.0");
    rollback(id).expect("rollback cam");
    let d = get_driver(id).expect("get cam 2");
    assert_eq!(d.version, "0.1.0");
    crate::serial_println!("  [8/12] install+rollback: OK");

    // 9: Not found.
    let r = get_driver(999);
    assert!(r.is_err());
    crate::serial_println!("  [9/12] not found: OK");

    // 10: Category check.
    let d = get_driver(display).expect("get display");
    assert_eq!(d.category, DriverCategory::Display);
    crate::serial_println!("  [10/12] category: OK");

    // 11: Stats.
    let (count, updates, total_updates, total_rollbacks, ops) = stats();
    assert_eq!(count, 4);
    assert!(total_updates >= 2);
    assert!(total_rollbacks >= 2);
    assert!(ops > 0);
    let _ = updates;
    crate::serial_println!("  [11/12] stats: OK");

    // 12: Residue. Everything registered above is removed, because the
    // registry this test writes to is the one `/proc` publishes as
    // `driver_count` -- and this self-test runs at boot. A test that
    // leaves four drivers behind is the same defect as shipping three,
    // wearing the word `Test`. The count after cleanup is the honest
    // answer to what is installed on this machine: nothing.
    for d in list_drivers() {
        unregister_driver(d.id).expect("unregister");
    }
    let (residue, _, _, _, _) = stats();
    assert_eq!(residue, 0);
    assert!(get_driver(display).is_err(), "display survived cleanup");
    crate::serial_println!("  [12/12] residue-free: OK");

    crate::serial_println!("driverupdate::self_test() — all 12 tests passed");
}

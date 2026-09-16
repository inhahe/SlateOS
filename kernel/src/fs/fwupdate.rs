//! Firmware Update — firmware version management and update tracking.
//!
//! Manages firmware versions for system components (BIOS/UEFI, EC,
//! TPM, NIC, SSD), tracks available updates, and records update
//! history.
//!
//! ## Architecture
//!
//! ```text
//! Firmware management
//!   → fwupdate::list_devices() → firmware-updatable devices
//!   → fwupdate::check_updates() → available updates
//!   → fwupdate::apply(device_id) → apply update (simulated)
//!
//! Integration:
//!   → secureboot (secure boot)
//!   → devicemgr (device manager)
//!   → driverupdate (driver updates)
//!   → updatemgr (update manager)
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

/// Firmware component type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareType {
    Bios,
    Uefi,
    EmbeddedController,
    Tpm,
    NetworkCard,
    StorageController,
    Gpu,
    Thunderbolt,
}

impl FirmwareType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bios => "BIOS",
            Self::Uefi => "UEFI",
            Self::EmbeddedController => "EC",
            Self::Tpm => "TPM",
            Self::NetworkCard => "NIC",
            Self::StorageController => "Storage",
            Self::Gpu => "GPU",
            Self::Thunderbolt => "Thunderbolt",
        }
    }
}

/// Update status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    UpdateAvailable,
    Downloading,
    Installing,
    PendingReboot,
    Failed,
}

impl UpdateStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::UpToDate => "Up to date",
            Self::UpdateAvailable => "Update available",
            Self::Downloading => "Downloading",
            Self::Installing => "Installing",
            Self::PendingReboot => "Pending reboot",
            Self::Failed => "Failed",
        }
    }
}

/// A firmware device entry.
#[derive(Debug, Clone)]
pub struct FirmwareDevice {
    pub id: u32,
    pub name: String,
    pub fw_type: FirmwareType,
    pub current_version: String,
    pub available_version: Option<String>,
    pub status: UpdateStatus,
    pub vendor: String,
    pub last_updated_ns: u64,
}

/// Firmware update history entry.
#[derive(Debug, Clone)]
pub struct UpdateRecord {
    pub device_id: u32,
    pub device_name: String,
    pub from_version: String,
    pub to_version: String,
    pub timestamp_ns: u64,
    pub success: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 32;
const MAX_HISTORY: usize = 200;

struct State {
    devices: Vec<FirmwareDevice>,
    history: Vec<UpdateRecord>,
    next_id: u32,
    total_updates: u64,
    total_failures: u64,
    total_checks: u64,
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
    *guard = Some(State {
        // No devices. This list used to hold three: a "System UEFI" with an
        // update available, a "TPM 2.0", and an "Intel I225-V" -- a real
        // 2.5GbE product -- each with a version and a vendor. The kernel
        // enumerates no firmware at all, so all of it was invented, and
        // `cmd_fwupdate` calls this function before listing: an operator
        // asking what firmware is present caused three devices to exist and
        // was shown them, two of them offering updates.
        //
        // A device appears here when something calls `register_device`. There
        // is no firmware enumeration yet, so nothing does, and the honest
        // answer to what firmware this machine has is: not known.
        devices: Vec::new(),
        history: Vec::new(),
        next_id: 1,
        total_updates: 0,
        total_failures: 0,
        total_checks: 0,
        ops: 0,
    });
}

/// Register a firmware device. **Private on purpose.**
///
/// Nothing outside this module calls it, because the kernel enumerates no
/// firmware, so nothing outside has anything to register. It exists
/// because the list it appends to previously had no way to be filled
/// except a seeded constant, and a registry whose only contents are
/// invented is worse than an empty one: `/proc` cannot tell them apart.
///
/// It is `fn` and not `pub fn` because
/// `scripts/check-unreachable-mutators.py` is right to refuse a public
/// mutator with no caller -- "a counter that cannot fall does not look
/// like a gap, it looks like data". Deliberately *not* wired to a shell
/// command to make it reachable: a `fwupdate register` subcommand would be
/// a way to invent firmware entries by hand, which is what the seeded
/// devices did and what removing them was for.
///
/// When real firmware enumeration exists, make this `pub` and wire the
/// caller in the same commit.
///
/// `available` is the version an update would move this device to, or
/// `None` when none is offered.
///
/// # Errors
///
/// `ResourceExhausted` past `MAX_DEVICES`; `NotSupported` before
/// `init_defaults`.
fn register_device(
    name: &str,
    fw_type: FirmwareType,
    current_version: &str,
    vendor: &str,
    available: Option<&str>,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.devices.push(FirmwareDevice {
            id,
            name: String::from(name),
            fw_type,
            current_version: String::from(current_version),
            available_version: available.map(String::from),
            status: if available.is_some() {
                UpdateStatus::UpdateAvailable
            } else {
                UpdateStatus::UpToDate
            },
            vendor: String::from(vendor),
            last_updated_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Remove a firmware device from the registry. Private, as
/// [`register_device`] is and for the same reason.
///
/// # Errors
///
/// `NotFound` if no device carries `id`; `NotSupported` before
/// `init_defaults`.
fn unregister_device(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.id != id);
        if state.devices.len() == before {
            return Err(KernelError::NotFound);
        }
        Ok(())
    })
}

/// List all firmware devices.
pub fn list_devices() -> Vec<FirmwareDevice> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.devices.clone())
}

/// Get device by ID.
pub fn get_device(id: u32) -> Option<FirmwareDevice> {
    STATE
        .lock()
        .as_ref()
        .and_then(|s| s.devices.iter().find(|d| d.id == id).cloned())
}

/// Check for updates (simulated).
pub fn check_updates() -> KernelResult<u32> {
    with_state(|state| {
        state.total_checks += 1;
        let available = state
            .devices
            .iter()
            .filter(|d| d.status == UpdateStatus::UpdateAvailable)
            .count();
        Ok(available as u32)
    })
}

/// Record a firmware update. **Writes no firmware.**
///
/// There is no firmware writer in this kernel. This moves the device's
/// reported version to `available_version`, sets `PendingReboot`, and
/// pushes an `UpdateRecord` whose `success` field means *the record was
/// written*, not *the flash succeeded*. Every caller that shows the result
/// to a human must say so: `kshell` does.
///
/// The distinction matters more here than anywhere else in this module,
/// because `PendingReboot` is an instruction. An operator who believes it
/// reboots to complete a flash that never began.
///
/// # Errors
///
/// `NotFound` for an unknown id; `InvalidArgument` if the device is not
/// offering an update.
pub fn apply_update(device_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let dev = state
            .devices
            .iter_mut()
            .find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        if dev.status != UpdateStatus::UpdateAvailable {
            return Err(KernelError::InvalidArgument);
        }
        let from = dev.current_version.clone();
        let to = dev.available_version.clone().unwrap_or_default();
        let dev_name = dev.name.clone();
        dev.current_version = to.clone();
        dev.available_version = None;
        dev.status = UpdateStatus::PendingReboot;
        dev.last_updated_ns = now;
        state.total_updates += 1;
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(UpdateRecord {
            device_id,
            device_name: dev_name,
            from_version: from,
            to_version: to,
            timestamp_ns: now,
            success: true,
        });
        Ok(())
    })
}

/// Get update history.
pub fn update_history() -> Vec<UpdateRecord> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.history.clone())
}

/// Statistics: (device_count, total_updates, total_failures, total_checks, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.devices.len(),
            s.total_updates,
            s.total_failures,
            s.total_checks,
            s.ops,
        ),
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
    crate::serial_println!("fwupdate::self_test() — running tests...");
    init_defaults();

    // The fixture is built HERE rather than shipped. These three used to
    // be seeded by `init_defaults`, and one of them named a real product
    // (Intel I225-V) on a machine that had enumerated nothing. This test
    // asserting `len() == 3` is what made them look required. Step 9
    // removes them again.
    let uefi = register_device(
        "Test UEFI",
        FirmwareType::Uefi,
        "1.20",
        "Test Vendor",
        Some("1.22"),
    )
    .expect("register uefi");
    let _tpm = register_device("Test TPM", FirmwareType::Tpm, "7.85", "Test Vendor", None)
        .expect("register tpm");
    let nic = register_device(
        "Test NIC",
        FirmwareType::NetworkCard,
        "1.68",
        "Test Vendor",
        Some("1.70"),
    )
    .expect("register nic");

    // 1: Registered devices.
    assert_eq!(list_devices().len(), 3);
    crate::serial_println!("  [1/9] registered: OK");

    // 2: Get device.
    let dev = get_device(uefi).expect("get");
    assert_eq!(dev.fw_type, FirmwareType::Uefi);
    assert_eq!(dev.current_version, "1.20");
    crate::serial_println!("  [2/9] get device: OK");

    // 3: Check updates.
    let available = check_updates().expect("check");
    assert_eq!(available, 2); // UEFI and NIC.
    crate::serial_println!("  [3/9] check updates: OK");

    // 4: Apply update.
    apply_update(uefi).expect("apply");
    let dev = get_device(uefi).expect("get2");
    assert_eq!(dev.current_version, "1.22");
    assert_eq!(dev.status, UpdateStatus::PendingReboot);
    crate::serial_println!("  [4/9] apply: OK");

    // 5: Can't re-apply.
    assert!(apply_update(uefi).is_err());
    crate::serial_println!("  [5/9] no re-apply: OK");

    // 6: Apply another.
    apply_update(nic).expect("apply2");
    let dev = get_device(nic).expect("get3");
    assert_eq!(dev.current_version, "1.70");
    crate::serial_println!("  [6/9] apply nic: OK");

    // 7: History.
    let hist = update_history();
    assert_eq!(hist.len(), 2);
    assert!(hist[0].success);
    crate::serial_println!("  [7/9] history: OK");

    // 8: Stats.
    let (devs, updates, failures, checks, ops) = stats();
    assert_eq!(devs, 3);
    assert_eq!(updates, 2);
    assert_eq!(failures, 0);
    assert!(checks >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/9] stats: OK");

    // 9: Residue. `with_pristine` would restore the table anyway, so this
    // is not what keeps the fixture out of /proc -- it is what proves
    // `unregister_device` works, which nothing else would.
    for d in list_devices() {
        unregister_device(d.id).expect("unregister");
    }
    let (residue, _, _, _, _) = stats();
    assert_eq!(residue, 0);
    crate::serial_println!("  [9/9] residue-free: OK");

    crate::serial_println!("fwupdate::self_test() — all 9 tests passed");
}

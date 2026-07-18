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

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        devices: alloc::vec![
            FirmwareDevice {
                id: 1, name: String::from("System UEFI"), fw_type: FirmwareType::Uefi,
                current_version: String::from("1.20"), available_version: Some(String::from("1.22")),
                status: UpdateStatus::UpdateAvailable, vendor: String::from("SystemVendor"),
                last_updated_ns: now,
            },
            FirmwareDevice {
                id: 2, name: String::from("TPM 2.0"), fw_type: FirmwareType::Tpm,
                current_version: String::from("7.85"), available_version: None,
                status: UpdateStatus::UpToDate, vendor: String::from("TPMVendor"),
                last_updated_ns: now,
            },
            FirmwareDevice {
                id: 3, name: String::from("Intel I225-V"), fw_type: FirmwareType::NetworkCard,
                current_version: String::from("1.68"), available_version: Some(String::from("1.70")),
                status: UpdateStatus::UpdateAvailable, vendor: String::from("Intel"),
                last_updated_ns: now,
            },
        ],
        history: Vec::new(),
        next_id: 4,
        total_updates: 0,
        total_failures: 0,
        total_checks: 0,
        ops: 0,
    });
}

/// List all firmware devices.
pub fn list_devices() -> Vec<FirmwareDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Get device by ID.
pub fn get_device(id: u32) -> Option<FirmwareDevice> {
    STATE.lock().as_ref().and_then(|s| s.devices.iter().find(|d| d.id == id).cloned())
}

/// Check for updates (simulated).
pub fn check_updates() -> KernelResult<u32> {
    with_state(|state| {
        state.total_checks += 1;
        let available = state.devices.iter()
            .filter(|d| d.status == UpdateStatus::UpdateAvailable)
            .count();
        Ok(available as u32)
    })
}

/// Apply firmware update (simulated).
pub fn apply_update(device_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
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
            device_id, device_name: dev_name,
            from_version: from, to_version: to,
            timestamp_ns: now, success: true,
        });
        Ok(())
    })
}

/// Get update history.
pub fn update_history() -> Vec<UpdateRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.history.clone())
}

/// Statistics: (device_count, total_updates, total_failures, total_checks, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_updates, s.total_failures, s.total_checks, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("fwupdate::self_test() — running tests...");
    init_defaults();

    // 1: Default devices.
    assert_eq!(list_devices().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get device.
    let dev = get_device(1).expect("get");
    assert_eq!(dev.fw_type, FirmwareType::Uefi);
    assert_eq!(dev.current_version, "1.20");
    crate::serial_println!("  [2/8] get device: OK");

    // 3: Check updates.
    let available = check_updates().expect("check");
    assert_eq!(available, 2); // UEFI and NIC.
    crate::serial_println!("  [3/8] check updates: OK");

    // 4: Apply update.
    apply_update(1).expect("apply");
    let dev = get_device(1).expect("get2");
    assert_eq!(dev.current_version, "1.22");
    assert_eq!(dev.status, UpdateStatus::PendingReboot);
    crate::serial_println!("  [4/8] apply: OK");

    // 5: Can't re-apply.
    assert!(apply_update(1).is_err());
    crate::serial_println!("  [5/8] no re-apply: OK");

    // 6: Apply another.
    apply_update(3).expect("apply2");
    let dev = get_device(3).expect("get3");
    assert_eq!(dev.current_version, "1.70");
    crate::serial_println!("  [6/8] apply nic: OK");

    // 7: History.
    let hist = update_history();
    assert_eq!(hist.len(), 2);
    assert!(hist[0].success);
    crate::serial_println!("  [7/8] history: OK");

    // 8: Stats.
    let (devs, updates, failures, checks, ops) = stats();
    assert_eq!(devs, 3);
    assert_eq!(updates, 2);
    assert_eq!(failures, 0);
    assert!(checks >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("fwupdate::self_test() — all 8 tests passed");
}

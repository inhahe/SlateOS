//! File Transfer — device-to-device file sharing (AirDrop-like).
//!
//! Provides nearby device discovery and file transfer functionality
//! using Wi-Fi Direct / Bluetooth for device-to-device sharing.
//!
//! ## Architecture
//!
//! ```text
//! User shares file
//!   → filetransfer::discover_devices() → nearby devices
//!   → filetransfer::send(device, files) → initiate transfer
//!   → filetransfer::accept(transfer_id) → receive files
//!
//! Integration:
//!   → bluetooth (BT transport)
//!   → netshare (network sharing)
//!   → mobilelink (phone link)
//!   → notifcenter (transfer notifications)
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

/// Discovery visibility mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Not discoverable.
    Hidden,
    /// Only contacts can see us.
    ContactsOnly,
    /// Anyone nearby can see us.
    Everyone,
}

impl Visibility {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hidden => "Hidden",
            Self::ContactsOnly => "Contacts Only",
            Self::Everyone => "Everyone",
        }
    }
}

/// Transport protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Bluetooth,
    WifiDirect,
    LocalNetwork,
    Auto,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bluetooth => "Bluetooth",
            Self::WifiDirect => "Wi-Fi Direct",
            Self::LocalNetwork => "Local Network",
            Self::Auto => "Auto",
        }
    }
}

/// Transfer status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Accepted,
    Transferring,
    Completed,
    Rejected,
    Failed,
    Cancelled,
}

impl TransferStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Accepted => "Accepted",
            Self::Transferring => "Transferring",
            Self::Completed => "Completed",
            Self::Rejected => "Rejected",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// A discovered nearby device.
#[derive(Debug, Clone)]
pub struct NearbyDevice {
    pub id: u32,
    pub name: String,
    pub device_type: String,
    pub transport: Transport,
    pub signal_strength: i32,
    pub discovered_ns: u64,
}

/// A file transfer record.
#[derive(Debug, Clone)]
pub struct Transfer {
    pub id: u32,
    pub device_name: String,
    pub file_name: String,
    pub file_size: u64,
    pub bytes_transferred: u64,
    pub status: TransferStatus,
    pub outgoing: bool,
    pub started_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 50;
const MAX_TRANSFERS: usize = 200;

struct State {
    visibility: Visibility,
    save_path: String,
    auto_accept_contacts: bool,
    devices: Vec<NearbyDevice>,
    transfers: Vec<Transfer>,
    next_device_id: u32,
    next_transfer_id: u32,
    total_sent: u64,
    total_received: u64,
    total_bytes_sent: u64,
    total_bytes_received: u64,
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
        visibility: Visibility::ContactsOnly,
        save_path: String::from("/home/Downloads"),
        auto_accept_contacts: false,
        devices: Vec::new(),
        transfers: Vec::new(),
        next_device_id: 1,
        next_transfer_id: 1,
        total_sent: 0,
        total_received: 0,
        total_bytes_sent: 0,
        total_bytes_received: 0,
        ops: 0,
    });
}

/// Set visibility.
pub fn set_visibility(vis: Visibility) -> KernelResult<()> {
    with_state(|state| {
        state.visibility = vis;
        Ok(())
    })
}

/// Set save path for received files.
pub fn set_save_path(path: &str) -> KernelResult<()> {
    with_state(|state| {
        state.save_path = String::from(path);
        Ok(())
    })
}

/// Set auto-accept for contacts.
pub fn set_auto_accept(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.auto_accept_contacts = enabled;
        Ok(())
    })
}

/// Simulate discovering a nearby device.
pub fn discover_device(name: &str, device_type: &str, transport: Transport, signal: i32) -> KernelResult<u32> {
    with_state(|state| {
        // Check for existing device with same name.
        if let Some(d) = state.devices.iter_mut().find(|d| d.name == name) {
            d.signal_strength = signal;
            d.discovered_ns = crate::hpet::elapsed_ns();
            return Ok(d.id);
        }
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_device_id;
        state.next_device_id += 1;
        state.devices.push(NearbyDevice {
            id,
            name: String::from(name),
            device_type: String::from(device_type),
            transport,
            signal_strength: signal,
            discovered_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Remove a device from discovered list.
pub fn remove_device(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.id != id);
        if state.devices.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Initiate a file transfer (outgoing).
pub fn send_file(device_id: u32, file_name: &str, file_size: u64) -> KernelResult<u32> {
    with_state(|state| {
        let device = state.devices.iter().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        if state.transfers.len() >= MAX_TRANSFERS {
            // Remove oldest completed transfers.
            state.transfers.retain(|t| !matches!(t.status,
                TransferStatus::Completed | TransferStatus::Rejected |
                TransferStatus::Failed | TransferStatus::Cancelled));
        }
        let id = state.next_transfer_id;
        state.next_transfer_id += 1;
        state.transfers.push(Transfer {
            id,
            device_name: device.name.clone(),
            file_name: String::from(file_name),
            file_size,
            bytes_transferred: 0,
            status: TransferStatus::Pending,
            outgoing: true,
            started_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Accept an incoming transfer.
pub fn accept_transfer(transfer_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let transfer = state.transfers.iter_mut().find(|t| t.id == transfer_id)
            .ok_or(KernelError::NotFound)?;
        if transfer.status != TransferStatus::Pending {
            return Err(KernelError::NotSupported);
        }
        transfer.status = TransferStatus::Accepted;
        Ok(())
    })
}

/// Reject an incoming transfer.
pub fn reject_transfer(transfer_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let transfer = state.transfers.iter_mut().find(|t| t.id == transfer_id)
            .ok_or(KernelError::NotFound)?;
        if transfer.status != TransferStatus::Pending {
            return Err(KernelError::NotSupported);
        }
        transfer.status = TransferStatus::Rejected;
        Ok(())
    })
}

/// Simulate transfer progress (for testing).
pub fn complete_transfer(transfer_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let transfer = state.transfers.iter_mut().find(|t| t.id == transfer_id)
            .ok_or(KernelError::NotFound)?;
        transfer.bytes_transferred = transfer.file_size;
        transfer.status = TransferStatus::Completed;
        if transfer.outgoing {
            state.total_sent += 1;
            state.total_bytes_sent += transfer.file_size;
        } else {
            state.total_received += 1;
            state.total_bytes_received += transfer.file_size;
        }
        Ok(())
    })
}

/// Cancel a transfer.
pub fn cancel_transfer(transfer_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let transfer = state.transfers.iter_mut().find(|t| t.id == transfer_id)
            .ok_or(KernelError::NotFound)?;
        transfer.status = TransferStatus::Cancelled;
        Ok(())
    })
}

/// List nearby devices.
pub fn list_devices() -> Vec<NearbyDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// List transfers.
pub fn list_transfers(max: usize) -> Vec<Transfer> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut t = s.transfers.clone();
        t.reverse();
        t.truncate(max);
        t
    })
}

/// Get current visibility.
pub fn get_visibility() -> Visibility {
    STATE.lock().as_ref().map_or(Visibility::Hidden, |s| s.visibility)
}

/// Statistics: (devices, total_sent, total_received, total_bytes_sent, total_bytes_received, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_sent, s.total_received,
                    s.total_bytes_sent, s.total_bytes_received, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("filetransfer::self_test() — running tests...");
    init_defaults();

    // 1: Default visibility.
    assert_eq!(get_visibility(), Visibility::ContactsOnly);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Set visibility.
    set_visibility(Visibility::Everyone).expect("vis");
    assert_eq!(get_visibility(), Visibility::Everyone);
    crate::serial_println!("  [2/8] visibility: OK");

    // 3: Discover devices.
    let d1 = discover_device("Phone", "mobile", Transport::Bluetooth, -40).expect("disc1");
    let d2 = discover_device("Laptop", "computer", Transport::WifiDirect, -30).expect("disc2");
    assert_eq!(list_devices().len(), 2);
    crate::serial_println!("  [3/8] discover: OK");

    // 4: Re-discover updates signal.
    let d1b = discover_device("Phone", "mobile", Transport::Bluetooth, -35).expect("redisc");
    assert_eq!(d1, d1b); // Same ID returned.
    assert_eq!(list_devices().len(), 2);
    crate::serial_println!("  [4/8] re-discover: OK");

    // 5: Send file.
    let tid = send_file(d2, "photo.jpg", 1024000).expect("send");
    let transfers = list_transfers(10);
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].status, TransferStatus::Pending);
    crate::serial_println!("  [5/8] send: OK");

    // 6: Complete transfer.
    complete_transfer(tid).expect("complete");
    let transfers = list_transfers(10);
    assert_eq!(transfers[0].status, TransferStatus::Completed);
    crate::serial_println!("  [6/8] complete: OK");

    // 7: Remove device.
    remove_device(d1).expect("remove");
    assert_eq!(list_devices().len(), 1);
    crate::serial_println!("  [7/8] remove: OK");

    // 8: Stats.
    let (devices, sent, _received, bytes_sent, _bytes_recv, ops) = stats();
    assert_eq!(devices, 1);
    assert_eq!(sent, 1);
    assert_eq!(bytes_sent, 1024000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("filetransfer::self_test() — all 8 tests passed");
}

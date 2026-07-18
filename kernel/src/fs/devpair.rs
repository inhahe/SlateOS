//! Device Pairing — Bluetooth/Wi-Fi device pairing workflow.
//!
//! Manages the pairing lifecycle for external devices including
//! discovery, PIN verification, trust management, and auto-reconnect.
//!
//! ## Architecture
//!
//! ```text
//! Pairing flow
//!   → devpair::scan() → discover devices
//!   → devpair::pair(device) → initiate pairing
//!   → devpair::confirm_pin(device, pin) → complete pairing
//!   → devpair::trust(device) → auto-connect
//!
//! Integration:
//!   → bluetooth (BT transport)
//!   → usbmgr (USB devices)
//!   → audiodevice (audio devices)
//!   → devicemgr (device management)
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

/// Device pairing state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairState {
    Discovered,
    Pairing,
    PinRequired,
    Paired,
    Trusted,
    Failed,
    Disconnected,
}

impl PairState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Discovered => "Discovered",
            Self::Pairing => "Pairing",
            Self::PinRequired => "PIN Required",
            Self::Paired => "Paired",
            Self::Trusted => "Trusted",
            Self::Failed => "Failed",
            Self::Disconnected => "Disconnected",
        }
    }
}

/// Device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairDeviceType {
    Headphones,
    Speaker,
    Keyboard,
    Mouse,
    Gamepad,
    Phone,
    Computer,
    Printer,
    Display,
    Other,
}

impl PairDeviceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Headphones => "Headphones",
            Self::Speaker => "Speaker",
            Self::Keyboard => "Keyboard",
            Self::Mouse => "Mouse",
            Self::Gamepad => "Gamepad",
            Self::Phone => "Phone",
            Self::Computer => "Computer",
            Self::Printer => "Printer",
            Self::Display => "Display",
            Self::Other => "Other",
        }
    }
}

/// A pairable device.
#[derive(Debug, Clone)]
pub struct PairDevice {
    pub id: u32,
    pub name: String,
    pub address: String,
    pub device_type: PairDeviceType,
    pub state: PairState,
    pub trusted: bool,
    pub auto_connect: bool,
    pub signal_strength: i32,
    pub pair_count: u64,
    pub last_connected_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 100;

struct State {
    devices: Vec<PairDevice>,
    next_id: u32,
    scanning: bool,
    total_paired: u64,
    total_failed: u64,
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
        devices: Vec::new(),
        next_id: 1,
        scanning: false,
        total_paired: 0,
        total_failed: 0,
        ops: 0,
    });
}

/// Start scanning for devices.
pub fn start_scan() -> KernelResult<()> {
    with_state(|state| {
        state.scanning = true;
        Ok(())
    })
}

/// Stop scanning.
pub fn stop_scan() -> KernelResult<()> {
    with_state(|state| {
        state.scanning = false;
        Ok(())
    })
}

/// Discover a device (simulate).
pub fn discover(name: &str, address: &str, device_type: PairDeviceType, signal: i32) -> KernelResult<u32> {
    with_state(|state| {
        // Check for existing device by address.
        if let Some(d) = state.devices.iter_mut().find(|d| d.address == address) {
            d.signal_strength = signal;
            if d.state == PairState::Disconnected {
                d.state = PairState::Discovered;
            }
            return Ok(d.id);
        }
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.devices.push(PairDevice {
            id,
            name: String::from(name),
            address: String::from(address),
            device_type,
            state: PairState::Discovered,
            trusted: false,
            auto_connect: false,
            signal_strength: signal,
            pair_count: 0,
            last_connected_ns: 0,
        });
        Ok(id)
    })
}

/// Initiate pairing.
pub fn pair(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        if dev.state != PairState::Discovered && dev.state != PairState::Disconnected {
            return Err(KernelError::NotSupported);
        }
        dev.state = PairState::Pairing;
        Ok(())
    })
}

/// Confirm pairing (simulate PIN acceptance).
pub fn confirm_pair(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let dev = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        if dev.state != PairState::Pairing && dev.state != PairState::PinRequired {
            return Err(KernelError::NotSupported);
        }
        dev.state = PairState::Paired;
        dev.pair_count += 1;
        dev.last_connected_ns = now;
        state.total_paired += 1;
        Ok(())
    })
}

/// Reject/fail pairing.
pub fn fail_pair(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        dev.state = PairState::Failed;
        state.total_failed += 1;
        Ok(())
    })
}

/// Trust a device (auto-connect).
pub fn trust(id: u32, trusted: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        dev.trusted = trusted;
        dev.auto_connect = trusted;
        if trusted && dev.state == PairState::Paired {
            dev.state = PairState::Trusted;
        }
        Ok(())
    })
}

/// Disconnect a device.
pub fn disconnect(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        dev.state = PairState::Disconnected;
        Ok(())
    })
}

/// Remove (unpair/forget) a device.
pub fn forget(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.id != id);
        if state.devices.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// List all devices.
pub fn list_devices() -> Vec<PairDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// List paired devices.
pub fn list_paired() -> Vec<PairDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.devices.iter()
            .filter(|d| matches!(d.state, PairState::Paired | PairState::Trusted))
            .cloned()
            .collect()
    })
}

/// List trusted devices.
pub fn list_trusted() -> Vec<PairDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.devices.iter().filter(|d| d.trusted).cloned().collect()
    })
}

/// Is scanning?
pub fn is_scanning() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.scanning)
}

/// Statistics: (device_count, paired_count, trusted_count, total_paired, total_failed, ops).
pub fn stats() -> (usize, usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let paired = s.devices.iter().filter(|d| matches!(d.state, PairState::Paired | PairState::Trusted)).count();
            let trusted = s.devices.iter().filter(|d| d.trusted).count();
            (s.devices.len(), paired, trusted, s.total_paired, s.total_failed, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("devpair::self_test() — running tests...");
    init_defaults();

    // 1: Empty.
    assert_eq!(list_devices().len(), 0);
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Discover devices.
    let d1 = discover("BT Headphones", "AA:BB:CC:DD:EE:01", PairDeviceType::Headphones, -40).expect("disc1");
    let d2 = discover("BT Keyboard", "AA:BB:CC:DD:EE:02", PairDeviceType::Keyboard, -50).expect("disc2");
    assert_eq!(list_devices().len(), 2);
    crate::serial_println!("  [2/8] discover: OK");

    // 3: Pair.
    pair(d1).expect("pair");
    confirm_pair(d1).expect("confirm");
    let paired = list_paired();
    assert_eq!(paired.len(), 1);
    crate::serial_println!("  [3/8] pair: OK");

    // 4: Trust.
    trust(d1, true).expect("trust");
    let trusted = list_trusted();
    assert_eq!(trusted.len(), 1);
    assert_eq!(trusted[0].state, PairState::Trusted);
    crate::serial_println!("  [4/8] trust: OK");

    // 5: Fail pairing.
    pair(d2).expect("pair2");
    fail_pair(d2).expect("fail");
    let dev = list_devices();
    let d2_dev = dev.iter().find(|d| d.id == d2).expect("find");
    assert_eq!(d2_dev.state, PairState::Failed);
    crate::serial_println!("  [5/8] fail: OK");

    // 6: Disconnect.
    disconnect(d1).expect("disconnect");
    assert_eq!(list_paired().len(), 0);
    crate::serial_println!("  [6/8] disconnect: OK");

    // 7: Forget device.
    forget(d2).expect("forget");
    assert_eq!(list_devices().len(), 1);
    crate::serial_println!("  [7/8] forget: OK");

    // 8: Stats.
    let (devices, _paired, _trusted, total_paired, total_failed, ops) = stats();
    assert_eq!(devices, 1);
    assert_eq!(total_paired, 1);
    assert_eq!(total_failed, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("devpair::self_test() — all 8 tests passed");
}

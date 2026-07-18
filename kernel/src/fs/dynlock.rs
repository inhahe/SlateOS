//! Dynamic Lock — proximity-based automatic screen locking.
//!
//! Automatically locks the screen when a paired Bluetooth device (e.g., phone)
//! goes out of range, and optionally unlocks when the device returns.
//!
//! ## Architecture
//!
//! ```text
//! Bluetooth device goes out of range
//!   → dynlock::check_proximity() → lock if device absent
//!
//! Configuration
//!   → dynlock::add_device(name, addr) → register trusted device
//!   → dynlock::set_timeout(secs) → grace period before locking
//!
//! Integration:
//!   → bluetooth (device detection)
//!   → screenlock (lock screen)
//!   → sessionmgr (session management)
//!   → wakesensor (proximity sensors)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Device proximity status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximityStatus {
    /// Device is in range.
    InRange,
    /// Device is out of range.
    OutOfRange,
    /// Device status unknown.
    Unknown,
}

impl ProximityStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::InRange => "In Range",
            Self::OutOfRange => "Out of Range",
            Self::Unknown => "Unknown",
        }
    }
}

/// Dynamic lock state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    /// Dynamic lock is not active (user unlocked manually).
    Idle,
    /// Monitoring proximity.
    Monitoring,
    /// Grace period before locking.
    GracePeriod,
    /// Screen has been locked by dynamic lock.
    Locked,
}

impl LockState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Monitoring => "Monitoring",
            Self::GracePeriod => "Grace Period",
            Self::Locked => "Locked",
        }
    }
}

/// A paired device for proximity detection.
#[derive(Debug, Clone)]
pub struct PairedDevice {
    pub name: String,
    pub address: String,
    pub status: ProximityStatus,
    pub enabled: bool,
    pub last_seen_ns: u64,
    pub lock_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 20;

struct State {
    devices: Vec<PairedDevice>,
    lock_state: LockState,
    /// Grace period in seconds before locking.
    grace_secs: u32,
    /// Whether to auto-unlock when device returns.
    auto_unlock: bool,
    global_enabled: bool,
    total_locks: u64,
    total_unlocks: u64,
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
        lock_state: LockState::Idle,
        grace_secs: 30,
        auto_unlock: false,
        global_enabled: false,
        total_locks: 0,
        total_unlocks: 0,
        ops: 0,
    });
}

/// Add a paired device.
pub fn add_device(name: &str, address: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.devices.iter().any(|d| d.address == address) {
            return Err(KernelError::AlreadyExists);
        }
        state.devices.push(PairedDevice {
            name: String::from(name),
            address: String::from(address),
            status: ProximityStatus::Unknown,
            enabled: true,
            last_seen_ns: 0,
            lock_count: 0,
        });
        Ok(())
    })
}

/// Remove a paired device.
pub fn remove_device(address: &str) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.address != address);
        if state.devices.len() == before {
            return Err(KernelError::NotFound);
        }
        Ok(())
    })
}

/// Update device proximity status.
pub fn update_proximity(address: &str, status: ProximityStatus) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let dev = state.devices.iter_mut().find(|d| d.address == address)
            .ok_or(KernelError::NotFound)?;
        dev.status = status;
        if status == ProximityStatus::InRange {
            dev.last_seen_ns = now;
        }
        Ok(())
    })
}

/// Check all devices and determine lock action.
/// Returns true if screen should be locked.
pub fn check_proximity() -> KernelResult<bool> {
    with_state(|state| {
        if !state.global_enabled || state.devices.is_empty() {
            return Ok(false);
        }

        // Check if any enabled device is in range.
        let any_in_range = state.devices.iter()
            .filter(|d| d.enabled)
            .any(|d| d.status == ProximityStatus::InRange);

        if any_in_range {
            // Device present: cancel any pending lock.
            if state.lock_state == LockState::Locked {
                if state.auto_unlock {
                    state.lock_state = LockState::Monitoring;
                    state.total_unlocks += 1;
                }
            } else {
                state.lock_state = LockState::Monitoring;
            }
            Ok(false)
        } else {
            // No device in range: should lock.
            match state.lock_state {
                LockState::Monitoring | LockState::GracePeriod => {
                    state.lock_state = LockState::Locked;
                    state.total_locks += 1;
                    // Increment per-device lock counts.
                    for dev in state.devices.iter_mut().filter(|d| d.enabled) {
                        dev.lock_count += 1;
                    }
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
    })
}

/// Set grace period.
pub fn set_grace(secs: u32) -> KernelResult<()> {
    with_state(|state| {
        state.grace_secs = secs.clamp(5, 300);
        Ok(())
    })
}

/// Set auto-unlock.
pub fn set_auto_unlock(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.auto_unlock = enabled;
        Ok(())
    })
}

/// Enable/disable dynamic lock.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        if enabled {
            state.lock_state = LockState::Monitoring;
        } else {
            state.lock_state = LockState::Idle;
        }
        Ok(())
    })
}

/// Manual unlock (user entered password).
pub fn manual_unlock() -> KernelResult<()> {
    with_state(|state| {
        if state.lock_state == LockState::Locked {
            state.lock_state = LockState::Monitoring;
            state.total_unlocks += 1;
        }
        Ok(())
    })
}

/// Get current lock state.
pub fn lock_state() -> LockState {
    STATE.lock().as_ref().map_or(LockState::Idle, |s| s.lock_state)
}

/// List paired devices.
pub fn list_devices() -> Vec<PairedDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Statistics: (device_count, total_locks, total_unlocks, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.total_locks, s.total_unlocks, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("dynlock::self_test() — running tests...");
    init_defaults();

    // 1: Initial state.
    assert_eq!(lock_state(), LockState::Idle);
    assert_eq!(list_devices().len(), 0);
    crate::serial_println!("  [1/8] initial state: OK");

    // 2: Add device.
    add_device("My Phone", "AA:BB:CC:DD:EE:FF").expect("add");
    assert_eq!(list_devices().len(), 1);
    crate::serial_println!("  [2/8] add device: OK");

    // 3: Duplicate rejection.
    assert!(add_device("Phone 2", "AA:BB:CC:DD:EE:FF").is_err());
    crate::serial_println!("  [3/8] duplicate rejection: OK");

    // 4: Enable and start monitoring.
    set_enabled(true).expect("enable");
    assert_eq!(lock_state(), LockState::Monitoring);
    crate::serial_println!("  [4/8] enable: OK");

    // 5: Device out of range → lock.
    update_proximity("AA:BB:CC:DD:EE:FF", ProximityStatus::OutOfRange).expect("update");
    let should_lock = check_proximity().expect("check");
    assert!(should_lock);
    assert_eq!(lock_state(), LockState::Locked);
    crate::serial_println!("  [5/8] proximity lock: OK");

    // 6: Device returns with auto-unlock.
    set_auto_unlock(true).expect("auto");
    update_proximity("AA:BB:CC:DD:EE:FF", ProximityStatus::InRange).expect("update2");
    let should_lock = check_proximity().expect("check2");
    assert!(!should_lock);
    assert_eq!(lock_state(), LockState::Monitoring);
    crate::serial_println!("  [6/8] auto-unlock: OK");

    // 7: Remove device.
    remove_device("AA:BB:CC:DD:EE:FF").expect("remove");
    assert_eq!(list_devices().len(), 0);
    crate::serial_println!("  [7/8] remove device: OK");

    // 8: Stats.
    let (devs, locks, unlocks, ops) = stats();
    assert_eq!(devs, 0);
    assert_eq!(locks, 1);
    assert!(unlocks >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("dynlock::self_test() — all 8 tests passed");
}

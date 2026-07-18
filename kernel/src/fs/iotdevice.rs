//! IoT Device Manager — smart home device management.
//!
//! Manages IoT and smart home devices including discovery,
//! state control, grouping, and automation rules.
//!
//! ## Architecture
//!
//! ```text
//! Device management
//!   → iotdevice::discover(device) → add to inventory
//!   → iotdevice::set_state(device, state) → control device
//!   → iotdevice::create_group(devices) → group control
//!
//! Integration:
//!   → bluetooth (BLE devices)
//!   → wifiscan (Wi-Fi devices)
//!   → netsettings (network config)
//!   → tasksched (automation schedules)
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

/// IoT device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Light,
    Switch,
    Thermostat,
    Camera,
    Lock,
    Speaker,
    Sensor,
    Plug,
    Fan,
    Other,
}

impl DeviceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Switch => "Switch",
            Self::Thermostat => "Thermostat",
            Self::Camera => "Camera",
            Self::Lock => "Lock",
            Self::Speaker => "Speaker",
            Self::Sensor => "Sensor",
            Self::Plug => "Plug",
            Self::Fan => "Fan",
            Self::Other => "Other",
        }
    }
}

/// Device connection protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Wifi,
    Bluetooth,
    Zigbee,
    Zwave,
    Thread,
    Matter,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Bluetooth => "Bluetooth",
            Self::Zigbee => "Zigbee",
            Self::Zwave => "Z-Wave",
            Self::Thread => "Thread",
            Self::Matter => "Matter",
        }
    }
}

/// An IoT device.
#[derive(Debug, Clone)]
pub struct IoTDevice {
    pub id: u32,
    pub name: String,
    pub device_type: DeviceType,
    pub protocol: Protocol,
    pub room: String,
    pub online: bool,
    pub state_value: String,    // e.g., "on", "off", "72°F", "locked"
    pub last_seen_ns: u64,
    pub command_count: u64,
}

/// A device group.
#[derive(Debug, Clone)]
pub struct DeviceGroup {
    pub id: u32,
    pub name: String,
    pub device_ids: Vec<u32>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 200;
const MAX_GROUPS: usize = 50;

struct State {
    devices: Vec<IoTDevice>,
    groups: Vec<DeviceGroup>,
    next_device_id: u32,
    next_group_id: u32,
    total_commands: u64,
    total_discoveries: u64,
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
        groups: Vec::new(),
        next_device_id: 1,
        next_group_id: 1,
        total_commands: 0,
        total_discoveries: 0,
        ops: 0,
    });
}

/// Discover/add a device.
pub fn discover(name: &str, dtype: DeviceType, protocol: Protocol, room: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_device_id;
        state.next_device_id += 1;
        state.total_discoveries += 1;
        state.devices.push(IoTDevice {
            id, name: String::from(name), device_type: dtype,
            protocol, room: String::from(room), online: true,
            state_value: String::from("off"), last_seen_ns: now,
            command_count: 0,
        });
        Ok(id)
    })
}

/// Remove a device.
pub fn remove_device(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.devices.len();
        state.devices.retain(|d| d.id != id);
        if state.devices.len() == before { return Err(KernelError::NotFound); }
        // Remove from groups.
        for g in &mut state.groups {
            g.device_ids.retain(|did| *did != id);
        }
        Ok(())
    })
}

/// Set device state.
pub fn set_state(id: u32, value: &str) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let dev = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        if !dev.online {
            return Err(KernelError::NotSupported);
        }
        dev.state_value = String::from(value);
        dev.last_seen_ns = now;
        dev.command_count += 1;
        state.total_commands += 1;
        Ok(())
    })
}

/// Set device online/offline.
pub fn set_online(id: u32, online: bool) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let dev = state.devices.iter_mut().find(|d| d.id == id)
            .ok_or(KernelError::NotFound)?;
        dev.online = online;
        if online { dev.last_seen_ns = now; }
        Ok(())
    })
}

/// Create a device group.
pub fn create_group(name: &str, device_ids: Vec<u32>) -> KernelResult<u32> {
    with_state(|state| {
        if state.groups.len() >= MAX_GROUPS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_group_id;
        state.next_group_id += 1;
        state.groups.push(DeviceGroup {
            id, name: String::from(name), device_ids,
        });
        Ok(id)
    })
}

/// Control all devices in a group.
pub fn group_command(group_id: u32, value: &str) -> KernelResult<usize> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let group = state.groups.iter().find(|g| g.id == group_id)
            .ok_or(KernelError::NotFound)?;
        let ids = group.device_ids.clone();
        let mut count = 0usize;
        for did in &ids {
            if let Some(dev) = state.devices.iter_mut().find(|d| d.id == *did && d.online) {
                dev.state_value = String::from(value);
                dev.last_seen_ns = now;
                dev.command_count += 1;
                state.total_commands += 1;
                count += 1;
            }
        }
        Ok(count)
    })
}

/// List all devices.
pub fn list_devices() -> Vec<IoTDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// List devices by room.
pub fn by_room(room: &str) -> Vec<IoTDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.devices.iter().filter(|d| d.room == room).cloned().collect()
    })
}

/// List groups.
pub fn list_groups() -> Vec<DeviceGroup> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.groups.clone())
}

/// Statistics: (device_count, group_count, online_count, total_commands, total_discoveries, ops).
pub fn stats() -> (usize, usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let online = s.devices.iter().filter(|d| d.online).count();
            (s.devices.len(), s.groups.len(), online, s.total_commands, s.total_discoveries, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("iotdevice::self_test() — running tests...");
    init_defaults();

    // 1: Empty initially.
    assert!(list_devices().is_empty());
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Discover devices.
    let d1 = discover("Living Room Light", DeviceType::Light, Protocol::Zigbee, "Living Room").expect("d1");
    let d2 = discover("Front Door Lock", DeviceType::Lock, Protocol::Zwave, "Hallway").expect("d2");
    let d3 = discover("Thermostat", DeviceType::Thermostat, Protocol::Wifi, "Living Room").expect("d3");
    assert_eq!(list_devices().len(), 3);
    crate::serial_println!("  [2/8] discover: OK");

    // 3: Set state.
    set_state(d1, "on").expect("state1");
    set_state(d3, "72°F").expect("state2");
    let devs = list_devices();
    let light = devs.iter().find(|d| d.id == d1).expect("light");
    assert_eq!(light.state_value, "on");
    crate::serial_println!("  [3/8] set state: OK");

    // 4: By room.
    let lr = by_room("Living Room");
    assert_eq!(lr.len(), 2);
    crate::serial_println!("  [4/8] by room: OK");

    // 5: Create group.
    let gid = create_group("All Lights", alloc::vec![d1]).expect("group");
    assert_eq!(list_groups().len(), 1);
    crate::serial_println!("  [5/8] group: OK");

    // 6: Group command.
    let count = group_command(gid, "off").expect("gcmd");
    assert_eq!(count, 1);
    let devs = list_devices();
    let light = devs.iter().find(|d| d.id == d1).expect("light2");
    assert_eq!(light.state_value, "off");
    crate::serial_println!("  [6/8] group command: OK");

    // 7: Offline device.
    set_online(d2, false).expect("offline");
    assert!(set_state(d2, "unlock").is_err());
    crate::serial_println!("  [7/8] offline: OK");

    // 8: Stats.
    let (devices, groups, online, commands, discoveries, ops) = stats();
    assert_eq!(devices, 3);
    assert_eq!(groups, 1);
    assert_eq!(online, 2); // d2 is offline.
    assert!(commands >= 3);
    assert_eq!(discoveries, 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("iotdevice::self_test() — all 8 tests passed");
}

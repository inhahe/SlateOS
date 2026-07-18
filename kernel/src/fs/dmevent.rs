//! Device Events — device hotplug and state change monitoring.
//!
//! Tracks device connection, disconnection, and state changes.
//! Provides an event queue for udev-style device management,
//! rule-based auto-actions, and device enumeration.
//!
//! ## Architecture
//!
//! ```text
//! Device events
//!   → dmevent::notify(event) → push device event
//!   → dmevent::poll() → poll for events
//!   → dmevent::add_rule(rule) → auto-action rule
//!   → dmevent::list_devices() → current devices
//!
//! Integration:
//!   → devicemgr (device manager)
//!   → usbmgr (USB manager)
//!   → hwmonitor (hardware monitor)
//!   → driverupdate (driver updates)
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

/// Device event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    Add,
    Remove,
    Change,
    Online,
    Offline,
    Bind,
    Unbind,
}

impl EventType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Remove => "remove",
            Self::Change => "change",
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Bind => "bind",
            Self::Unbind => "unbind",
        }
    }
}

/// Device subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subsystem {
    Block,
    Net,
    Usb,
    Pci,
    Input,
    Tty,
    Sound,
    Gpu,
}

impl Subsystem {
    pub fn label(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Net => "net",
            Self::Usb => "usb",
            Self::Pci => "pci",
            Self::Input => "input",
            Self::Tty => "tty",
            Self::Sound => "sound",
            Self::Gpu => "gpu",
        }
    }
}

/// A device event.
#[derive(Debug, Clone)]
pub struct DeviceEvent {
    pub seq: u64,
    pub event_type: EventType,
    pub subsystem: Subsystem,
    pub devpath: String,
    pub devname: String,
    pub timestamp_ns: u64,
    pub properties: Vec<(String, String)>,
}

/// An auto-action rule.
#[derive(Debug, Clone)]
pub struct EventRule {
    pub id: u32,
    pub event_type: EventType,
    pub subsystem: Subsystem,
    pub devname_pattern: String,   // Simple prefix match.
    pub action: String,            // Action to take (e.g. "mount", "notify").
    pub enabled: bool,
}

/// A known device.
#[derive(Debug, Clone)]
pub struct KnownDevice {
    pub devpath: String,
    pub devname: String,
    pub subsystem: Subsystem,
    pub online: bool,
    pub first_seen_ns: u64,
    pub last_event_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_EVENTS: usize = 2048;
const MAX_RULES: usize = 128;
const MAX_DEVICES: usize = 512;

struct State {
    events: Vec<DeviceEvent>,
    rules: Vec<EventRule>,
    devices: Vec<KnownDevice>,
    next_seq: u64,
    next_rule_id: u32,
    total_events: u64,
    total_matched: u64,
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
        events: Vec::new(),
        rules: alloc::vec![
            EventRule {
                id: 1, event_type: EventType::Add, subsystem: Subsystem::Block,
                devname_pattern: String::from("sd"), action: String::from("automount"),
                enabled: true,
            },
            EventRule {
                id: 2, event_type: EventType::Add, subsystem: Subsystem::Usb,
                devname_pattern: String::from("usb"), action: String::from("notify"),
                enabled: true,
            },
        ],
        devices: alloc::vec![
            KnownDevice {
                devpath: String::from("/sys/block/sda"), devname: String::from("sda"),
                subsystem: Subsystem::Block, online: true,
                first_seen_ns: now, last_event_ns: now,
            },
            KnownDevice {
                devpath: String::from("/sys/class/net/eth0"), devname: String::from("eth0"),
                subsystem: Subsystem::Net, online: true,
                first_seen_ns: now, last_event_ns: now,
            },
            KnownDevice {
                devpath: String::from("/sys/class/input/keyboard0"), devname: String::from("keyboard0"),
                subsystem: Subsystem::Input, online: true,
                first_seen_ns: now, last_event_ns: now,
            },
        ],
        next_seq: 1,
        next_rule_id: 3,
        total_events: 0,
        total_matched: 0,
        ops: 0,
    });
}

/// Push a device event.
pub fn notify(event_type: EventType, subsystem: Subsystem, devpath: &str, devname: &str, props: &[(&str, &str)]) -> KernelResult<u64> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let seq = state.next_seq;
        state.next_seq += 1;
        // Update or add known device.
        match event_type {
            EventType::Add | EventType::Online | EventType::Bind => {
                if let Some(d) = state.devices.iter_mut().find(|d| d.devpath == devpath) {
                    d.online = true;
                    d.last_event_ns = now;
                } else if state.devices.len() < MAX_DEVICES {
                    state.devices.push(KnownDevice {
                        devpath: String::from(devpath), devname: String::from(devname),
                        subsystem, online: true, first_seen_ns: now, last_event_ns: now,
                    });
                }
            }
            EventType::Remove | EventType::Offline | EventType::Unbind => {
                if let Some(d) = state.devices.iter_mut().find(|d| d.devpath == devpath) {
                    d.online = false;
                    d.last_event_ns = now;
                }
            }
            EventType::Change => {
                if let Some(d) = state.devices.iter_mut().find(|d| d.devpath == devpath) {
                    d.last_event_ns = now;
                }
            }
        }
        // Check rules.
        let matched = state.rules.iter()
            .filter(|r| r.enabled && r.event_type == event_type && r.subsystem == subsystem)
            .any(|r| devname.starts_with(&r.devname_pattern));
        if matched { state.total_matched += 1; }
        // Store event.
        let properties = props.iter()
            .map(|(k, v)| (String::from(*k), String::from(*v)))
            .collect();
        if state.events.len() >= MAX_EVENTS {
            state.events.remove(0);
        }
        state.events.push(DeviceEvent {
            seq, event_type, subsystem, devpath: String::from(devpath),
            devname: String::from(devname), timestamp_ns: now, properties,
        });
        state.total_events += 1;
        Ok(seq)
    })
}

/// Poll recent events (last N).
pub fn poll(last_n: usize) -> Vec<DeviceEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if last_n >= s.events.len() { 0 } else { s.events.len() - last_n };
        s.events[start..].to_vec()
    })
}

/// Clear event log.
pub fn clear_events() -> KernelResult<()> {
    with_state(|state| { state.events.clear(); Ok(()) })
}

/// Add an auto-action rule.
pub fn add_rule(event_type: EventType, subsystem: Subsystem, pattern: &str, action: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.rules.len() >= MAX_RULES { return Err(KernelError::ResourceExhausted); }
        let id = state.next_rule_id;
        state.next_rule_id += 1;
        state.rules.push(EventRule {
            id, event_type, subsystem, devname_pattern: String::from(pattern),
            action: String::from(action), enabled: true,
        });
        Ok(id)
    })
}

/// Remove a rule.
pub fn remove_rule(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.rules.len();
        state.rules.retain(|r| r.id != id);
        if state.rules.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// List rules.
pub fn list_rules() -> Vec<EventRule> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.rules.clone())
}

/// List known devices.
pub fn list_devices() -> Vec<KnownDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Get device by path.
pub fn get_device(devpath: &str) -> Option<KnownDevice> {
    STATE.lock().as_ref().and_then(|s| s.devices.iter().find(|d| d.devpath == devpath).cloned())
}

/// Statistics: (device_count, event_count, rule_count, total_events, total_matched, ops).
pub fn stats() -> (usize, usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.devices.len(), s.events.len(), s.rules.len(), s.total_events, s.total_matched, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("dmevent::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    assert_eq!(list_devices().len(), 3);
    assert_eq!(list_rules().len(), 2);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Notify add (matches rule).
    let seq = notify(EventType::Add, Subsystem::Block, "/sys/block/sdb", "sdb1", &[("size", "1000000")]).expect("notify");
    assert!(seq >= 1);
    assert_eq!(list_devices().len(), 4);
    crate::serial_println!("  [2/8] notify add: OK");

    // 3: Poll events.
    let events = poll(10);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].devname, "sdb1");
    assert_eq!(events[0].properties.len(), 1);
    crate::serial_println!("  [3/8] poll: OK");

    // 4: Notify remove.
    notify(EventType::Remove, Subsystem::Block, "/sys/block/sdb", "sdb1", &[]).expect("remove");
    let d = get_device("/sys/block/sdb").expect("get");
    assert!(!d.online);
    crate::serial_println!("  [4/8] remove: OK");

    // 5: Add rule.
    let rid = add_rule(EventType::Add, Subsystem::Net, "wlan", "scan_wifi").expect("rule");
    assert!(rid >= 3);
    crate::serial_println!("  [5/8] add rule: OK");

    // 6: Remove rule.
    remove_rule(rid).expect("rm_rule");
    assert!(remove_rule(999).is_err());
    crate::serial_println!("  [6/8] remove rule: OK");

    // 7: Clear events.
    clear_events().expect("clear");
    assert_eq!(poll(10).len(), 0);
    crate::serial_println!("  [7/8] clear: OK");

    // 8: Stats.
    let (devs, evs, rules, total, matched, ops) = stats();
    assert!(devs >= 3);
    assert_eq!(evs, 0); // Cleared.
    assert_eq!(rules, 2);
    assert!(total >= 2);
    assert!(matched >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("dmevent::self_test() — all 8 tests passed");
}

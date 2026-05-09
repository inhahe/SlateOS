//! Device manager — hardware device tree, driver binding, hotplug management.
//!
//! Provides a user-facing view of all detected hardware devices with
//! driver information, status, and configuration. Handles hotplug events
//! and driver pairing for USB and PCI devices.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Devices / Device Manager
//!   → devicemgr::list_devices() / device_info()
//!
//! Driver subsystem
//!   → devicemgr::register_device() on detection
//!   → devicemgr::bind_driver() for driver pairing
//!
//! Integration:
//!   → sysfs (hardware enumeration)
//!   → audiodevice (audio device subset)
//!   → bluetooth (BT device subset)
//!   → notifcenter (hotplug notifications)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Device bus type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusType {
    Pci,
    Usb,
    Acpi,
    Platform,
    I2c,
    Spi,
    Bluetooth,
    Virtual,
}

impl BusType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pci => "PCI",
            Self::Usb => "USB",
            Self::Acpi => "ACPI",
            Self::Platform => "Platform",
            Self::I2c => "I2C",
            Self::Spi => "SPI",
            Self::Bluetooth => "Bluetooth",
            Self::Virtual => "Virtual",
        }
    }
}

/// Device class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    Display,
    Audio,
    Network,
    Storage,
    Input,
    Usb,
    Bluetooth,
    Multimedia,
    Processor,
    Memory,
    Bridge,
    Communication,
    Printer,
    Power,
    Sensor,
    Other,
}

impl DeviceClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::Display => "Display",
            Self::Audio => "Audio",
            Self::Network => "Network",
            Self::Storage => "Storage",
            Self::Input => "Input",
            Self::Usb => "USB Controller",
            Self::Bluetooth => "Bluetooth",
            Self::Multimedia => "Multimedia",
            Self::Processor => "Processor",
            Self::Memory => "Memory",
            Self::Bridge => "Bridge",
            Self::Communication => "Communication",
            Self::Printer => "Printer",
            Self::Power => "Power",
            Self::Sensor => "Sensor",
            Self::Other => "Other",
        }
    }
}

/// Device operational status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Working normally.
    Ok,
    /// No driver bound.
    NoDriver,
    /// Driver error.
    Error,
    /// Disabled by user.
    Disabled,
    /// Disconnected (hotplug).
    Disconnected,
    /// Unknown / initializing.
    Unknown,
}

impl DeviceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::NoDriver => "No Driver",
            Self::Error => "Error",
            Self::Disabled => "Disabled",
            Self::Disconnected => "Disconnected",
            Self::Unknown => "Unknown",
        }
    }
}

/// A registered hardware device.
#[derive(Debug, Clone)]
pub struct HwDevice {
    /// Device ID (internal).
    pub id: u32,
    /// Device name / description.
    pub name: String,
    /// Bus type.
    pub bus: BusType,
    /// Device class.
    pub class: DeviceClass,
    /// Vendor ID.
    pub vendor_id: u16,
    /// Product ID.
    pub product_id: u16,
    /// Vendor name.
    pub vendor_name: String,
    /// Current status.
    pub status: DeviceStatus,
    /// Bound driver name (empty if no driver).
    pub driver: String,
    /// Driver version string.
    pub driver_version: String,
    /// Bus address (e.g., "00:1f.3" for PCI, "2-1.3" for USB).
    pub bus_address: String,
    /// Whether the device is enabled.
    pub enabled: bool,
    /// Whether it was hot-plugged (vs present at boot).
    pub hotplugged: bool,
    /// Registration timestamp (ns since boot).
    pub detected_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 256;

struct State {
    devices: Vec<HwDevice>,
    next_id: u32,
    hotplug_events: u64,
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

/// Initialise device manager with common platform devices.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let now = crate::hpet::elapsed_ns();
    let devices = alloc::vec![
        HwDevice {
            id: 1, name: String::from("CPU"), bus: BusType::Platform,
            class: DeviceClass::Processor, vendor_id: 0, product_id: 0,
            vendor_name: String::from("Generic"), status: DeviceStatus::Ok,
            driver: String::from("cpu"), driver_version: String::from("1.0"),
            bus_address: String::from("cpu0"), enabled: true, hotplugged: false,
            detected_ns: now,
        },
        HwDevice {
            id: 2, name: String::from("System Memory"), bus: BusType::Platform,
            class: DeviceClass::Memory, vendor_id: 0, product_id: 0,
            vendor_name: String::from("Generic"), status: DeviceStatus::Ok,
            driver: String::from("memory"), driver_version: String::from("1.0"),
            bus_address: String::from("mem0"), enabled: true, hotplugged: false,
            detected_ns: now,
        },
        HwDevice {
            id: 3, name: String::from("PCI Host Bridge"), bus: BusType::Pci,
            class: DeviceClass::Bridge, vendor_id: 0x8086, product_id: 0x0001,
            vendor_name: String::from("Intel"), status: DeviceStatus::Ok,
            driver: String::from("pci-bridge"), driver_version: String::from("1.0"),
            bus_address: String::from("00:00.0"), enabled: true, hotplugged: false,
            detected_ns: now,
        },
    ];

    *guard = Some(State {
        devices,
        next_id: 4,
        hotplug_events: 0,
        ops: 0,
    });
}

/// Register a new hardware device.
pub fn register_device(
    name: &str,
    bus: BusType,
    class: DeviceClass,
    vendor_id: u16,
    product_id: u16,
    vendor_name: &str,
    bus_address: &str,
    hotplugged: bool,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }

        let id = state.next_id;
        state.next_id += 1;
        let now = crate::hpet::elapsed_ns();

        if hotplugged {
            state.hotplug_events += 1;
        }

        state.devices.push(HwDevice {
            id,
            name: String::from(name),
            bus,
            class,
            vendor_id,
            product_id,
            vendor_name: String::from(vendor_name),
            status: DeviceStatus::NoDriver,
            driver: String::new(),
            driver_version: String::new(),
            bus_address: String::from(bus_address),
            enabled: true,
            hotplugged,
            detected_ns: now,
        });

        Ok(id)
    })
}

/// Bind a driver to a device.
pub fn bind_driver(device_id: u32, driver: &str, version: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.driver = String::from(driver);
        dev.driver_version = String::from(version);
        dev.status = DeviceStatus::Ok;
        Ok(())
    })
}

/// Unbind the driver from a device.
pub fn unbind_driver(device_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.driver = String::new();
        dev.driver_version = String::new();
        dev.status = DeviceStatus::NoDriver;
        Ok(())
    })
}

/// Remove a device (hot-unplug).
pub fn remove_device(device_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.devices.iter().position(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        state.devices.remove(pos);
        state.hotplug_events += 1;
        Ok(())
    })
}

/// Enable or disable a device.
pub fn set_enabled(device_id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut().find(|d| d.id == device_id)
            .ok_or(KernelError::NotFound)?;
        dev.enabled = enabled;
        dev.status = if enabled {
            if dev.driver.is_empty() { DeviceStatus::NoDriver } else { DeviceStatus::Ok }
        } else {
            DeviceStatus::Disabled
        };
        Ok(())
    })
}

/// Get device info.
pub fn get_device(device_id: u32) -> KernelResult<HwDevice> {
    with_state(|state| {
        state.devices.iter().find(|d| d.id == device_id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List all devices.
pub fn list_devices() -> Vec<HwDevice> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.devices.clone(),
        None => Vec::new(),
    }
}

/// List devices by class.
pub fn devices_by_class(class: DeviceClass) -> Vec<HwDevice> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.devices.iter().filter(|d| d.class == class).cloned().collect(),
        None => Vec::new(),
    }
}

/// List devices by bus.
pub fn devices_by_bus(bus: BusType) -> Vec<HwDevice> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.devices.iter().filter(|d| d.bus == bus).cloned().collect(),
        None => Vec::new(),
    }
}

/// Count devices needing drivers.
pub fn devices_needing_driver() -> usize {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.devices.iter().filter(|d| d.status == DeviceStatus::NoDriver).count(),
        None => 0,
    }
}

/// Statistics: (total_devices, ok_count, no_driver_count, hotplug_events, ops).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let ok = s.devices.iter().filter(|d| d.status == DeviceStatus::Ok).count();
            let no_drv = s.devices.iter().filter(|d| d.status == DeviceStatus::NoDriver).count();
            (s.devices.len(), ok, no_drv, s.hotplug_events, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("devicemgr::self_test() — running tests...");

    init_defaults();

    // Test 1: Default devices present.
    let devices = list_devices();
    assert!(devices.len() >= 3);
    crate::serial_println!("  [1/11] default devices: OK");

    // Test 2: Register a new device.
    let id = register_device(
        "USB Flash Drive", BusType::Usb, DeviceClass::Storage,
        0x0781, 0x5567, "SanDisk", "2-1.3", true,
    ).expect("register");
    assert!(id > 0);
    crate::serial_println!("  [2/11] register device: OK");

    // Test 3: Device has NoDriver status initially.
    let dev = get_device(id).expect("get device");
    assert_eq!(dev.status, DeviceStatus::NoDriver);
    assert!(dev.hotplugged);
    crate::serial_println!("  [3/11] initial status NoDriver: OK");

    // Test 4: Bind a driver.
    bind_driver(id, "usb-storage", "1.0").expect("bind");
    let dev = get_device(id).expect("get after bind");
    assert_eq!(dev.status, DeviceStatus::Ok);
    assert_eq!(dev.driver, "usb-storage");
    crate::serial_println!("  [4/11] bind driver: OK");

    // Test 5: Unbind driver.
    unbind_driver(id).expect("unbind");
    let dev = get_device(id).expect("get after unbind");
    assert_eq!(dev.status, DeviceStatus::NoDriver);
    assert!(dev.driver.is_empty());
    crate::serial_println!("  [5/11] unbind driver: OK");

    // Test 6: Disable device.
    set_enabled(id, false).expect("disable");
    let dev = get_device(id).expect("get disabled");
    assert_eq!(dev.status, DeviceStatus::Disabled);
    assert!(!dev.enabled);
    crate::serial_println!("  [6/11] disable device: OK");

    // Test 7: Re-enable device.
    set_enabled(id, true).expect("enable");
    let dev = get_device(id).expect("get enabled");
    assert_eq!(dev.status, DeviceStatus::NoDriver);
    assert!(dev.enabled);
    crate::serial_println!("  [7/11] re-enable device: OK");

    // Test 8: Filter by class.
    let storage = devices_by_class(DeviceClass::Storage);
    assert!(storage.len() >= 1);
    crate::serial_println!("  [8/11] filter by class: OK");

    // Test 9: Filter by bus.
    let usb = devices_by_bus(BusType::Usb);
    assert!(usb.len() >= 1);
    crate::serial_println!("  [9/11] filter by bus: OK");

    // Test 10: Remove device.
    remove_device(id).expect("remove");
    assert!(get_device(id).is_err());
    crate::serial_println!("  [10/11] remove device: OK");

    // Test 11: Stats.
    let (total, ok, no_drv, hotplug, ops) = stats();
    assert!(total >= 3);
    assert!(ok >= 3);
    assert!(hotplug >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("devicemgr::self_test() — all 11 tests passed");
}

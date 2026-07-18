//! USB manager — USB device enumeration and management.
//!
//! Tracks connected USB devices, supports safe removal, and provides
//! device information (class, speed, power draw).  Works with the
//! devicemgr for general device management.
//!
//! ## Architecture
//!
//! ```text
//! USB host controller IRQ
//!   → usbmgr::device_connected(descriptor) → enumerate
//!   → usbmgr::device_disconnected(port) → cleanup
//!
//! Settings panel → Devices → USB
//!   → usbmgr::list_devices() → device listing
//!   → usbmgr::safe_remove(port) → eject device
//!
//! Integration:
//!   → devicemgr (general device list)
//!   → notifcenter (connect/disconnect notifications)
//!   → soundevents (device plug sounds)
//!   → disksmart (USB storage health)
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

/// USB speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    Low,        // 1.5 Mbps (USB 1.0)
    Full,       // 12 Mbps (USB 1.1)
    High,       // 480 Mbps (USB 2.0)
    Super,      // 5 Gbps (USB 3.0)
    SuperPlus,  // 10 Gbps (USB 3.1)
    SuperPlus2, // 20 Gbps (USB 3.2)
    Usb4,       // 40 Gbps (USB4)
}

impl UsbSpeed {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low (1.5 Mbps)",
            Self::Full => "Full (12 Mbps)",
            Self::High => "High (480 Mbps)",
            Self::Super => "SuperSpeed (5 Gbps)",
            Self::SuperPlus => "SuperSpeed+ (10 Gbps)",
            Self::SuperPlus2 => "SuperSpeed+ (20 Gbps)",
            Self::Usb4 => "USB4 (40 Gbps)",
        }
    }

    pub fn mbps(self) -> u32 {
        match self {
            Self::Low => 1,
            Self::Full => 12,
            Self::High => 480,
            Self::Super => 5000,
            Self::SuperPlus => 10000,
            Self::SuperPlus2 => 20000,
            Self::Usb4 => 40000,
        }
    }
}

/// USB device class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbClass {
    MassStorage,
    HumanInterface,
    Audio,
    Video,
    Printer,
    Hub,
    Wireless,
    Communication,
    SmartCard,
    Imaging,
    Vendor,
    Other,
}

impl UsbClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::MassStorage => "Mass Storage",
            Self::HumanInterface => "HID",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Printer => "Printer",
            Self::Hub => "Hub",
            Self::Wireless => "Wireless",
            Self::Communication => "Communication",
            Self::SmartCard => "Smart Card",
            Self::Imaging => "Imaging",
            Self::Vendor => "Vendor Specific",
            Self::Other => "Other",
        }
    }
}

/// A connected USB device.
#[derive(Debug, Clone)]
pub struct UsbDevice {
    /// Bus number.
    pub bus: u8,
    /// Port number.
    pub port: u8,
    /// Device address on bus.
    pub address: u8,
    /// Vendor ID.
    pub vendor_id: u16,
    /// Product ID.
    pub product_id: u16,
    /// Manufacturer string.
    pub manufacturer: String,
    /// Product string.
    pub product: String,
    /// Serial number.
    pub serial: String,
    /// Device class.
    pub class: UsbClass,
    /// Connection speed.
    pub speed: UsbSpeed,
    /// Power draw in milliamps.
    pub power_ma: u16,
    /// Whether device can be safely removed.
    pub removable: bool,
    /// Driver name.
    pub driver: String,
    /// Connect timestamp (ns).
    pub connected_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 127; // USB max per bus.

struct State {
    devices: Vec<UsbDevice>,
    total_connects: u64,
    total_disconnects: u64,
    total_safe_removes: u64,
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

/// Initialise the USB device registry as EMPTY.
///
/// We never fabricate connected hardware. A desktop may have any (or no) USB
/// devices, so the registry starts empty and real devices are added only via
/// `device_connected()`, called by the USB host-controller driver as it
/// enumerates ports (and on hotplug).
///
/// DEFERRED PROPER FIX: wire `device_connected()` / `device_disconnected()` to
/// a real USB host-controller driver (xHCI/EHCI enumeration + hotplug events)
/// once one exists. Until then the registry is honestly empty rather than
/// seeded with a phantom keyboard and mouse.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    *guard = Some(State {
        devices: Vec::new(),
        total_connects: 0,
        total_disconnects: 0,
        total_safe_removes: 0,
        ops: 0,
    });
}

/// Register a newly connected USB device.
pub fn device_connected(
    bus: u8, port: u8, vendor_id: u16, product_id: u16,
    manufacturer: &str, product: &str, class: UsbClass, speed: UsbSpeed, power_ma: u16,
) -> KernelResult<u8> {
    with_state(|state| {
        if state.devices.len() >= MAX_DEVICES {
            return Err(KernelError::ResourceExhausted);
        }
        // Assign next address.
        let address = state.devices.iter().map(|d| d.address).max().unwrap_or(0) + 1;
        state.devices.push(UsbDevice {
            bus, port, address,
            vendor_id, product_id,
            manufacturer: String::from(manufacturer),
            product: String::from(product),
            serial: String::new(),
            class, speed, power_ma,
            removable: true,
            driver: String::new(),
            connected_ns: crate::hpet::elapsed_ns(),
        });
        state.total_connects += 1;
        Ok(address)
    })
}

/// Remove a disconnected device.
pub fn device_disconnected(bus: u8, port: u8) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.devices.iter().position(|d| d.bus == bus && d.port == port)
            .ok_or(KernelError::NotFound)?;
        state.devices.remove(pos);
        state.total_disconnects += 1;
        Ok(())
    })
}

/// Safely remove a device (flush caches, unmount, etc.).
pub fn safe_remove(bus: u8, port: u8) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.devices.iter().position(|d| d.bus == bus && d.port == port)
            .ok_or(KernelError::NotFound)?;
        if !state.devices[pos].removable {
            return Err(KernelError::PermissionDenied);
        }
        state.devices.remove(pos);
        state.total_safe_removes += 1;
        state.total_disconnects += 1;
        Ok(())
    })
}

/// List all connected devices.
pub fn list_devices() -> Vec<UsbDevice> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.devices.clone())
}

/// Get device by bus and port.
pub fn get_device(bus: u8, port: u8) -> KernelResult<UsbDevice> {
    with_state(|state| {
        state.devices.iter().find(|d| d.bus == bus && d.port == port).cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// Total power draw in milliamps.
pub fn total_power_draw() -> u32 {
    STATE.lock().as_ref().map_or(0, |s| {
        s.devices.iter().map(|d| d.power_ma as u32).sum()
    })
}

/// Statistics: (device_count, connects, disconnects, safe_removes, total_power_ma, ops).
pub fn stats() -> (usize, u64, u64, u64, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let power: u32 = s.devices.iter().map(|d| d.power_ma as u32).sum();
            (s.devices.len(), s.total_connects, s.total_disconnects, s.total_safe_removes, power, s.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("usbmgr::self_test() — running tests...");

    // Residue-free: start from a known-empty registry.
    *STATE.lock() = None;
    init_defaults();

    // 1: Registry starts empty — we never fabricate connected hardware.
    assert_eq!(list_devices().len(), 0);
    let (c0, conn0, _d0, _s0, _p0, _o0) = stats();
    assert_eq!(c0, 0);
    assert_eq!(conn0, 0);
    crate::serial_println!("  [1/11] empty registry: OK");

    // Build deterministic fixtures via the real connect entry point: a
    // keyboard and a mouse (what the old fabricated default invented, now
    // installed explicitly inside the test rather than at boot).
    device_connected(1, 1, 0x046d, 0xc52b, "Logitech", "USB Keyboard",
        UsbClass::HumanInterface, UsbSpeed::Full, 100).expect("connect kb");
    device_connected(1, 2, 0x046d, 0xc077, "Logitech", "USB Mouse",
        UsbClass::HumanInterface, UsbSpeed::Full, 100).expect("connect mouse");
    assert_eq!(list_devices().len(), 2);

    // 2: Device info.
    let kb = get_device(1, 1).expect("get keyboard");
    assert_eq!(kb.class, UsbClass::HumanInterface);
    assert!(kb.product.contains("Keyboard"));
    crate::serial_println!("  [2/11] device info: OK");

    // 3: Connect new device.
    let addr = device_connected(
        1, 3, 0x0781, 0x5583, "SanDisk", "Ultra USB 3.0",
        UsbClass::MassStorage, UsbSpeed::Super, 896,
    ).expect("connect flash");
    assert!(addr > 0);
    assert_eq!(list_devices().len(), 3);
    crate::serial_println!("  [3/11] connect device: OK");

    // 4: Power draw (100 + 100 + 896 = 1096, exact).
    assert_eq!(total_power_draw(), 1096);
    crate::serial_println!("  [4/11] power draw: OK");

    // 5: Safe remove.
    safe_remove(1, 3).expect("safe remove flash");
    assert_eq!(list_devices().len(), 2);
    crate::serial_println!("  [5/11] safe remove: OK");

    // 6: Disconnect.
    device_connected(2, 1, 0x1234, 0x5678, "Generic", "USB Hub",
        UsbClass::Hub, UsbSpeed::High, 0).expect("connect hub");
    device_disconnected(2, 1).expect("disconnect hub");
    assert_eq!(list_devices().len(), 2);
    crate::serial_println!("  [6/11] disconnect: OK");

    // 7: Not found error.
    let r = get_device(99, 99);
    assert!(r.is_err());
    crate::serial_println!("  [7/11] not found: OK");

    // 8: Speed info.
    let kb = get_device(1, 1).expect("get kb 2");
    assert_eq!(kb.speed.mbps(), 12);
    crate::serial_println!("  [8/11] speed info: OK");

    // 9: Vendor info.
    assert_eq!(kb.vendor_id, 0x046d);
    assert_eq!(kb.manufacturer, "Logitech");
    crate::serial_println!("  [9/11] vendor info: OK");

    // 10: Multiple connects.
    for i in 0..5u8 {
        device_connected(3, i, 0xAAAA, i as u16, "Test", "Device",
            UsbClass::Other, UsbSpeed::High, 50).expect("connect batch");
    }
    assert_eq!(list_devices().len(), 7);
    crate::serial_println!("  [10/11] batch connect: OK");

    // 11: Stats — exact totals.
    // connects: kb,mouse,flash,hub + 5 batch = 9; disconnects: flash(safe)+hub = 2;
    // safe_removes: 1; power: kb 100 + mouse 100 + 5×50 = 450.
    let (count, connects, disconnects, safe_removes, power, ops) = stats();
    assert_eq!(count, 7);
    assert_eq!(connects, 9);
    assert_eq!(disconnects, 2);
    assert_eq!(safe_removes, 1);
    assert_eq!(power, 450);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    // Residue-free: leave no fixtures behind.
    *STATE.lock() = None;

    crate::serial_println!("usbmgr::self_test() — all 11 tests passed");
}

//! Device hotplug event system — connects hardware discovery to the
//! userspace driver framework.
//!
//! Orchestrates the lifecycle when hardware appears or disappears:
//!
//! 1. **Discovery**: PCI enumeration or hotplug interrupt fires.
//! 2. **Registration**: Device registered with [`crate::udriver`].
//! 3. **Matching**: Device matched against known driver database.
//! 4. **Notification**: Event emitted for driver loader service.
//! 5. **Binding**: Driver process binds via udriver framework.
//!
//! On device removal:
//! 1. **Removal detected**: hotplug interrupt or explicit removal.
//! 2. **Driver notified**: graceful shutdown signal.
//! 3. **Cleanup**: resources freed via udriver crash/unregister path.
//!
//! ## Driver Database
//!
//! A simple vendor:device → driver name mapping table. The driver loader
//! service reads this to know which driver binary to start for each device.
//! This is the kernel's static "known drivers" list; the full database
//! lives in userspace (package manager can install additional drivers).
//!
//! ## Event Queue
//!
//! Hotplug events are queued for the driver loader service to consume.
//! This decouples hardware detection (interrupt context, fast) from
//! driver loading (userspace, may involve disk I/O).
//!
//! ## Integration
//!
//! - Called by PCI enumeration during boot (batch discovery).
//! - Called by PCIe hotplug interrupt handler (runtime discovery).
//! - Called by xHCI when USB devices appear/disappear.
//! - Events consumed by the driver loader service via IPC.
//! - Kshell `hotplug` command for status and manual operations.
//! - `/proc/hotplug` for monitoring.
//!
//! ## References
//!
//! - Linux `drivers/pci/hotplug/` — PCIe native hotplug
//! - Linux `lib/kobject_uevent.c` — uevent mechanism
//! - Fuchsia driver manager — device/driver binding

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::udriver::{DeviceAddr, DeviceId};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum events in the queue before oldest are dropped.
const MAX_EVENT_QUEUE: usize = 256;

/// Maximum entries in the driver database.
const MAX_DRIVER_DB_ENTRIES: usize = 512;

/// Maximum event history (for diagnostics).
const MAX_EVENT_HISTORY: usize = 128;

// ---------------------------------------------------------------------------
// Types — Events
// ---------------------------------------------------------------------------

/// A hotplug event describing hardware appearing or disappearing.
#[derive(Debug, Clone)]
pub struct HotplugEvent {
    /// Unique event ID (monotonically increasing).
    pub id: u64,
    /// What happened.
    pub kind: EventKind,
    /// Bus type where the event occurred.
    pub bus: BusType,
    /// PCI address (for PCI/PCIe devices).
    pub device_addr: Option<DeviceAddr>,
    /// Device identification.
    pub device_id: Option<DeviceId>,
    /// USB-specific info (for USB devices).
    pub usb_info: Option<UsbDeviceInfo>,
    /// Matched driver name (if found in database).
    pub matched_driver: Option<String>,
    /// Timestamp (ns since boot).
    pub timestamp_ns: u64,
    /// Whether this event has been consumed by the driver loader.
    pub consumed: bool,
}

/// What kind of hotplug event occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// New device detected (PCI enumeration, USB attach, etc.).
    DeviceArrived,
    /// Device removed (USB unplug, PCIe hot-remove).
    DeviceRemoved,
    /// Device entered error state (PCIe AER, USB overcurrent).
    DeviceError,
    /// Driver successfully bound to device.
    DriverBound,
    /// Driver unbound from device (graceful or crash).
    DriverUnbound,
    /// Device reset (recovery after error).
    DeviceReset,
}

impl EventKind {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::DeviceArrived => "arrived",
            Self::DeviceRemoved => "removed",
            Self::DeviceError => "error",
            Self::DriverBound => "driver-bound",
            Self::DriverUnbound => "driver-unbound",
            Self::DeviceReset => "reset",
        }
    }
}

/// Bus type for hotplug events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusType {
    /// PCI / PCIe device.
    Pci,
    /// USB device (via xHCI).
    Usb,
    /// Platform / ACPI enumerated device.
    Platform,
    /// Virtual device (e.g., virtio without PCI).
    Virtual,
}

impl BusType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pci => "PCI",
            Self::Usb => "USB",
            Self::Platform => "platform",
            Self::Virtual => "virtual",
        }
    }
}

/// USB-specific device information for hotplug events.
#[derive(Debug, Clone)]
pub struct UsbDeviceInfo {
    /// USB vendor ID.
    pub vendor_id: u16,
    /// USB product ID.
    pub product_id: u16,
    /// USB device class.
    pub class: u8,
    /// USB device subclass.
    pub subclass: u8,
    /// USB port path (e.g., "1-2.3" for hub topology).
    pub port_path: String,
    /// USB speed.
    pub speed: UsbSpeed,
}

/// USB device speed classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    Low,        // 1.5 Mbps (USB 1.0)
    Full,       // 12 Mbps (USB 1.1)
    High,       // 480 Mbps (USB 2.0)
    Super,      // 5 Gbps (USB 3.0)
    SuperPlus,  // 10 Gbps (USB 3.1)
    SuperPlus2, // 20 Gbps (USB 3.2)
}

impl UsbSpeed {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "1.5 Mbps",
            Self::Full => "12 Mbps",
            Self::High => "480 Mbps",
            Self::Super => "5 Gbps",
            Self::SuperPlus => "10 Gbps",
            Self::SuperPlus2 => "20 Gbps",
        }
    }
}

// ---------------------------------------------------------------------------
// Types — Driver database
// ---------------------------------------------------------------------------

/// An entry in the driver matching database.
///
/// Maps device identification to a driver name. The driver loader service
/// uses this to decide which driver binary to start.
#[derive(Debug, Clone)]
pub struct DriverDbEntry {
    /// Unique entry ID.
    pub id: u32,
    /// PCI vendor ID to match (0xFFFF = wildcard).
    pub vendor_id: u16,
    /// PCI device ID to match (0xFFFF = wildcard).
    pub device_id: u16,
    /// PCI class to match (0xFF = wildcard).
    pub class: u8,
    /// PCI subclass to match (0xFF = wildcard).
    pub subclass: u8,
    /// Name of the driver (used to locate the driver binary).
    pub driver_name: String,
    /// Priority (higher = preferred when multiple entries match).
    pub priority: u8,
    /// Whether this entry is enabled.
    pub enabled: bool,
}

impl DriverDbEntry {
    /// Check if this entry matches a given device.
    pub fn matches(&self, id: &DeviceId) -> bool {
        if !self.enabled {
            return false;
        }
        (self.vendor_id == 0xFFFF || self.vendor_id == id.vendor_id)
            && (self.device_id == 0xFFFF || self.device_id == id.device_id)
            && (self.class == 0xFF || self.class == id.class)
            && (self.subclass == 0xFF || self.subclass == id.subclass)
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    /// Pending hotplug events (not yet consumed by driver loader).
    event_queue: Vec<HotplugEvent>,
    /// Historical events (for diagnostics, ring buffer).
    event_history: Vec<HotplugEvent>,
    /// Driver matching database.
    driver_db: Vec<DriverDbEntry>,
    /// Next event ID.
    next_event_id: u64,
    /// Next driver DB entry ID.
    next_db_id: u32,
    /// Total devices arrived since boot.
    total_arrived: u64,
    /// Total devices removed since boot.
    total_removed: u64,
    /// Total driver bindings since boot.
    total_bindings: u64,
    /// Total events dropped (queue overflow).
    events_dropped: u64,
}

impl State {
    const fn new() -> Self {
        Self {
            event_queue: Vec::new(),
            event_history: Vec::new(),
            driver_db: Vec::new(),
            next_event_id: 1,
            next_db_id: 1,
            total_arrived: 0,
            total_removed: 0,
            total_bindings: 0,
            events_dropped: 0,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Initialization — populate built-in driver database
// ---------------------------------------------------------------------------

/// Initialize the hotplug system with built-in driver entries.
///
/// Called during boot before PCI enumeration. Populates the driver database
/// with entries for known device types (virtio, common chipsets).
pub fn init() {
    let mut state = STATE.lock();

    // Virtio devices (vendor 0x1AF4).
    add_db_entry_locked(&mut state, 0x1AF4, 0x1000, 0xFF, 0xFF, "virtio-net");
    add_db_entry_locked(&mut state, 0x1AF4, 0x1001, 0xFF, 0xFF, "virtio-blk");
    add_db_entry_locked(&mut state, 0x1AF4, 0x1002, 0xFF, 0xFF, "virtio-balloon");
    add_db_entry_locked(&mut state, 0x1AF4, 0x1003, 0xFF, 0xFF, "virtio-console");
    add_db_entry_locked(&mut state, 0x1AF4, 0x1004, 0xFF, 0xFF, "virtio-scsi");
    add_db_entry_locked(&mut state, 0x1AF4, 0x1005, 0xFF, 0xFF, "virtio-rng");
    add_db_entry_locked(&mut state, 0x1AF4, 0x1009, 0xFF, 0xFF, "virtio-9p");
    add_db_entry_locked(&mut state, 0x1AF4, 0x1050, 0xFF, 0xFF, "virtio-gpu");
    add_db_entry_locked(&mut state, 0x1AF4, 0x1059, 0xFF, 0xFF, "virtio-sound");

    // Intel e1000/e1000e (common in VMs).
    add_db_entry_locked(&mut state, 0x8086, 0x100E, 0xFF, 0xFF, "e1000");
    add_db_entry_locked(&mut state, 0x8086, 0x100F, 0xFF, 0xFF, "e1000");
    add_db_entry_locked(&mut state, 0x8086, 0x10D3, 0xFF, 0xFF, "e1000e");
    add_db_entry_locked(&mut state, 0x8086, 0x153A, 0xFF, 0xFF, "e1000e");

    // Realtek RTL8139 (common in VMs and older hardware).
    add_db_entry_locked(&mut state, 0x10EC, 0x8139, 0xFF, 0xFF, "rtl8139");

    // Intel HDA audio (common in VMs and real hardware).
    add_db_entry_locked(&mut state, 0x8086, 0x2668, 0xFF, 0xFF, "hda-intel");
    add_db_entry_locked(&mut state, 0x8086, 0x293E, 0xFF, 0xFF, "hda-intel");

    // AHCI / SATA controllers (class 01:06).
    add_db_entry_locked(&mut state, 0xFFFF, 0xFFFF, 0x01, 0x06, "ahci");

    // NVMe controllers (class 01:08).
    add_db_entry_locked(&mut state, 0xFFFF, 0xFFFF, 0x01, 0x08, "nvme");

    // xHCI USB controllers (class 0C:03, prog_if 30).
    add_db_entry_locked(&mut state, 0xFFFF, 0xFFFF, 0x0C, 0x03, "xhci");

    // AC'97 audio (class 04:01).
    add_db_entry_locked(&mut state, 0xFFFF, 0xFFFF, 0x04, 0x01, "ac97");

    // VGA-compatible display (class 03:00) — fallback framebuffer.
    add_db_entry_locked(&mut state, 0xFFFF, 0xFFFF, 0x03, 0x00, "vga-fb");

    crate::syslog!(
        "devhotplug",
        Info,
        "initialized with {} built-in driver entries",
        state.driver_db.len()
    );
}

fn add_db_entry_locked(
    state: &mut State,
    vendor: u16,
    device: u16,
    class: u8,
    subclass: u8,
    name: &str,
) {
    let id = state.next_db_id;
    state.next_db_id = state.next_db_id.wrapping_add(1);
    state.driver_db.push(DriverDbEntry {
        id,
        vendor_id: vendor,
        device_id: device,
        class,
        subclass,
        driver_name: String::from(name),
        priority: 100, // Default priority for built-in entries.
        enabled: true,
    });
}

// ---------------------------------------------------------------------------
// Event emission
// ---------------------------------------------------------------------------

/// Report a new PCI device discovered during enumeration or hotplug.
///
/// Creates a `DeviceArrived` event, tries to match it against the driver
/// database, and queues the event for the driver loader service.
///
/// Also registers the device with the udriver framework.
pub fn device_arrived_pci(addr: DeviceAddr, id: DeviceId) {
    let now = crate::hpet::elapsed_ns();

    // Register with udriver framework (ignore error if already there).
    let _ = crate::udriver::register_device(addr, id);

    // Match against driver database.
    let matched = find_matching_driver(&id);

    let mut state = STATE.lock();
    let event_id = state.next_event_id;
    state.next_event_id = state.next_event_id.wrapping_add(1);
    state.total_arrived = state.total_arrived.saturating_add(1);

    let event = HotplugEvent {
        id: event_id,
        kind: EventKind::DeviceArrived,
        bus: BusType::Pci,
        device_addr: Some(addr),
        device_id: Some(id),
        usb_info: None,
        matched_driver: matched.clone(),
        timestamp_ns: now,
        consumed: false,
    };

    push_event(&mut state, event);

    let match_str = matched.as_deref().unwrap_or("(no match)");
    crate::syslog!(
        "devhotplug",
        Info,
        "PCI device arrived: {:02x}:{:02x}.{} vendor={:04x} device={:04x} → {}",
        addr.bus, addr.device, addr.function,
        id.vendor_id, id.device_id, match_str
    );
}

/// Report a PCI device removed (PCIe hotplug or administrative removal).
pub fn device_removed_pci(addr: DeviceAddr) {
    let now = crate::hpet::elapsed_ns();

    let mut state = STATE.lock();
    let event_id = state.next_event_id;
    state.next_event_id = state.next_event_id.wrapping_add(1);
    state.total_removed = state.total_removed.saturating_add(1);

    let event = HotplugEvent {
        id: event_id,
        kind: EventKind::DeviceRemoved,
        bus: BusType::Pci,
        device_addr: Some(addr),
        device_id: None,
        usb_info: None,
        matched_driver: None,
        timestamp_ns: now,
        consumed: false,
    };

    push_event(&mut state, event);

    crate::syslog!(
        "devhotplug",
        Info,
        "PCI device removed: {:02x}:{:02x}.{}",
        addr.bus, addr.device, addr.function
    );
}

/// Report a USB device arrival.
pub fn device_arrived_usb(info: UsbDeviceInfo) {
    let now = crate::hpet::elapsed_ns();

    let mut state = STATE.lock();
    let event_id = state.next_event_id;
    state.next_event_id = state.next_event_id.wrapping_add(1);
    state.total_arrived = state.total_arrived.saturating_add(1);

    // USB matching uses class/subclass (vendor:product matching is
    // available but less common for kernel-level driver selection).
    let event = HotplugEvent {
        id: event_id,
        kind: EventKind::DeviceArrived,
        bus: BusType::Usb,
        device_addr: None,
        device_id: None,
        usb_info: Some(info),
        matched_driver: None, // USB matching done separately
        timestamp_ns: now,
        consumed: false,
    };

    push_event(&mut state, event);
}

/// Report a USB device removal.
pub fn device_removed_usb(port_path: &str) {
    let now = crate::hpet::elapsed_ns();

    let mut state = STATE.lock();
    let event_id = state.next_event_id;
    state.next_event_id = state.next_event_id.wrapping_add(1);
    state.total_removed = state.total_removed.saturating_add(1);

    let event = HotplugEvent {
        id: event_id,
        kind: EventKind::DeviceRemoved,
        bus: BusType::Usb,
        device_addr: None,
        device_id: None,
        usb_info: Some(UsbDeviceInfo {
            vendor_id: 0,
            product_id: 0,
            class: 0,
            subclass: 0,
            port_path: String::from(port_path),
            speed: UsbSpeed::Full,
        }),
        matched_driver: None,
        timestamp_ns: now,
        consumed: false,
    };

    push_event(&mut state, event);
}

/// Report that a driver has been successfully bound to a device.
pub fn driver_bound(addr: DeviceAddr, driver_name: &str) {
    let now = crate::hpet::elapsed_ns();

    let mut state = STATE.lock();
    let event_id = state.next_event_id;
    state.next_event_id = state.next_event_id.wrapping_add(1);
    state.total_bindings = state.total_bindings.saturating_add(1);

    let event = HotplugEvent {
        id: event_id,
        kind: EventKind::DriverBound,
        bus: BusType::Pci,
        device_addr: Some(addr),
        device_id: None,
        usb_info: None,
        matched_driver: Some(String::from(driver_name)),
        timestamp_ns: now,
        consumed: false,
    };

    push_event(&mut state, event);
}

/// Report a device error (e.g., PCIe AER correctable/uncorrectable error).
pub fn device_error(addr: DeviceAddr, device_id: Option<DeviceId>) {
    let now = crate::hpet::elapsed_ns();

    let mut state = STATE.lock();
    let event_id = state.next_event_id;
    state.next_event_id = state.next_event_id.wrapping_add(1);

    let event = HotplugEvent {
        id: event_id,
        kind: EventKind::DeviceError,
        bus: BusType::Pci,
        device_addr: Some(addr),
        device_id,
        usb_info: None,
        matched_driver: None,
        timestamp_ns: now,
        consumed: false,
    };

    push_event(&mut state, event);

    crate::syslog!(
        "devhotplug",
        Error,
        "device error: {:02x}:{:02x}.{}",
        addr.bus, addr.device, addr.function
    );
}

// ---------------------------------------------------------------------------
// Event consumption (for driver loader service)
// ---------------------------------------------------------------------------

/// Get all unconsumed events (for the driver loader service to process).
///
/// Returns the events and marks them as consumed.
pub fn drain_events() -> Vec<HotplugEvent> {
    let mut state = STATE.lock();
    let mut events = Vec::new();

    for event in &mut state.event_queue {
        if !event.consumed {
            event.consumed = true;
            events.push(event.clone());
        }
    }

    // Remove consumed events from the queue.
    state.event_queue.retain(|e| !e.consumed);

    events
}

/// Peek at pending events without consuming them.
#[must_use]
pub fn pending_events() -> Vec<HotplugEvent> {
    STATE.lock().event_queue.iter()
        .filter(|e| !e.consumed)
        .cloned()
        .collect()
}

/// Get the number of pending (unconsumed) events.
#[must_use]
pub fn pending_count() -> usize {
    STATE.lock().event_queue.iter()
        .filter(|e| !e.consumed)
        .count()
}

// ---------------------------------------------------------------------------
// Driver database management
// ---------------------------------------------------------------------------

/// Add an entry to the driver matching database.
///
/// Returns the entry ID on success.
pub fn add_driver_entry(
    vendor_id: u16,
    device_id: u16,
    class: u8,
    subclass: u8,
    driver_name: &str,
    priority: u8,
) -> KernelResult<u32> {
    let mut state = STATE.lock();

    if state.driver_db.len() >= MAX_DRIVER_DB_ENTRIES {
        return Err(KernelError::ResourceExhausted);
    }

    let id = state.next_db_id;
    state.next_db_id = state.next_db_id.wrapping_add(1);

    state.driver_db.push(DriverDbEntry {
        id,
        vendor_id,
        device_id,
        class,
        subclass,
        driver_name: String::from(driver_name),
        priority,
        enabled: true,
    });

    Ok(id)
}

/// Remove a driver database entry by ID.
pub fn remove_driver_entry(id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state.driver_db.iter().position(|e| e.id == id)
        .ok_or(KernelError::NotFound)?;
    state.driver_db.swap_remove(idx);
    Ok(())
}

/// Enable/disable a driver database entry.
pub fn set_driver_entry_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let entry = state.driver_db.iter_mut().find(|e| e.id == id)
        .ok_or(KernelError::NotFound)?;
    entry.enabled = enabled;
    Ok(())
}

/// List all driver database entries.
#[must_use]
pub fn driver_database() -> Vec<DriverDbEntry> {
    STATE.lock().driver_db.clone()
}

/// Find the best matching driver for a given device ID.
///
/// Returns the driver name if a match is found, preferring higher-priority
/// entries. Returns `None` if no match.
#[must_use]
pub fn find_matching_driver(id: &DeviceId) -> Option<String> {
    let state = STATE.lock();

    let mut best: Option<&DriverDbEntry> = None;

    for entry in &state.driver_db {
        if entry.matches(id) {
            match best {
                None => best = Some(entry),
                Some(current) => {
                    if entry.priority > current.priority {
                        best = Some(entry);
                    }
                }
            }
        }
    }

    best.map(|e| e.driver_name.clone())
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Recent event history (for diagnostics).
#[must_use]
pub fn event_history() -> Vec<HotplugEvent> {
    STATE.lock().event_history.clone()
}

/// Summary statistics.
#[derive(Debug, Clone)]
pub struct HotplugStats {
    pub pending_events: usize,
    pub total_arrived: u64,
    pub total_removed: u64,
    pub total_bindings: u64,
    pub events_dropped: u64,
    pub driver_db_entries: usize,
    pub history_entries: usize,
}

/// Get hotplug system statistics.
#[must_use]
pub fn stats() -> HotplugStats {
    let state = STATE.lock();
    HotplugStats {
        pending_events: state.event_queue.iter().filter(|e| !e.consumed).count(),
        total_arrived: state.total_arrived,
        total_removed: state.total_removed,
        total_bindings: state.total_bindings,
        events_dropped: state.events_dropped,
        driver_db_entries: state.driver_db.len(),
        history_entries: state.event_history.len(),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Push an event to both the queue and history, handling overflow.
fn push_event(state: &mut State, event: HotplugEvent) {
    // Add to event queue (for driver loader consumption).
    if state.event_queue.len() >= MAX_EVENT_QUEUE {
        // Drop oldest unconsumed event.
        if let Some(pos) = state.event_queue.iter().position(|e| !e.consumed) {
            state.event_queue.remove(pos);
            state.events_dropped = state.events_dropped.saturating_add(1);
        }
    }
    state.event_queue.push(event.clone());

    // Add to history ring buffer (for diagnostics).
    if state.event_history.len() >= MAX_EVENT_HISTORY {
        state.event_history.remove(0);
    }
    state.event_history.push(event);
}

// ---------------------------------------------------------------------------
// procfs content
// ---------------------------------------------------------------------------

/// Generate `/proc/hotplug` content.
#[must_use]
pub fn procfs_content() -> String {
    let state = STATE.lock();
    let mut out = String::with_capacity(4096);

    out.push_str("=== Device Hotplug System ===\n\n");

    // Stats.
    let pending = state.event_queue.iter().filter(|e| !e.consumed).count();
    out.push_str(&format!("Pending events:     {}\n", pending));
    out.push_str(&format!("Total arrived:      {}\n", state.total_arrived));
    out.push_str(&format!("Total removed:      {}\n", state.total_removed));
    out.push_str(&format!("Total bindings:     {}\n", state.total_bindings));
    out.push_str(&format!("Events dropped:     {}\n", state.events_dropped));
    out.push_str(&format!("Driver DB entries:  {}\n", state.driver_db.len()));
    out.push_str(&format!("History entries:    {}\n\n", state.event_history.len()));

    // Pending events.
    if pending > 0 {
        out.push_str("Pending events:\n");
        for event in state.event_queue.iter().filter(|e| !e.consumed) {
            format_event(&mut out, event, "  ");
        }
        out.push('\n');
    }

    // Recent history (last 20).
    if !state.event_history.is_empty() {
        out.push_str("Recent history:\n");
        let start = if state.event_history.len() > 20 {
            state.event_history.len() - 20
        } else {
            0
        };
        for event in &state.event_history[start..] {
            format_event(&mut out, event, "  ");
        }
        out.push('\n');
    }

    // Driver database.
    if !state.driver_db.is_empty() {
        out.push_str("Driver database:\n");
        for entry in &state.driver_db {
            let vendor_str = if entry.vendor_id == 0xFFFF {
                String::from("*")
            } else {
                format!("{:04x}", entry.vendor_id)
            };
            let device_str = if entry.device_id == 0xFFFF {
                String::from("*")
            } else {
                format!("{:04x}", entry.device_id)
            };
            let class_str = if entry.class == 0xFF {
                String::from("*")
            } else {
                format!("{:02x}", entry.class)
            };
            let sub_str = if entry.subclass == 0xFF {
                String::from("*")
            } else {
                format!("{:02x}", entry.subclass)
            };
            let enabled = if entry.enabled { "" } else { " [disabled]" };
            out.push_str(&format!(
                "  #{:<3} {}:{} class={}:{} → '{}' (pri={}){}",
                entry.id, vendor_str, device_str,
                class_str, sub_str,
                entry.driver_name, entry.priority, enabled,
            ));
            out.push('\n');
        }
    }

    out
}

fn format_event(out: &mut String, event: &HotplugEvent, indent: &str) {
    let ts_ms = event.timestamp_ns / 1_000_000;

    out.push_str(&format!(
        "{}[{}] #{} {} {}",
        indent, ts_ms, event.id, event.bus.label(), event.kind.label()
    ));

    if let Some(addr) = &event.device_addr {
        out.push_str(&format!(" {:02x}:{:02x}.{}", addr.bus, addr.device, addr.function));
    }

    if let Some(id) = &event.device_id {
        out.push_str(&format!(" (vendor={:04x} device={:04x})", id.vendor_id, id.device_id));
    }

    if let Some(usb) = &event.usb_info {
        out.push_str(&format!(" USB {} port={}", usb.speed.label(), usb.port_path));
    }

    if let Some(drv) = &event.matched_driver {
        out.push_str(&format!(" → {}", drv));
    }

    out.push('\n');
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("[devhotplug] running self-tests...");

    test_init();
    test_driver_db_matching();
    test_wildcard_matching();
    test_priority_matching();
    test_device_arrived();
    test_device_removed();
    test_drain_events();
    test_event_overflow();
    test_add_remove_db_entry();
    test_enable_disable_entry();
    test_usb_event();
    test_stats();
    test_procfs();

    crate::serial_println!("[devhotplug] all self-tests passed");
}

fn reset_state() {
    let mut state = STATE.lock();
    *state = State::new();
}

fn test_init() {
    reset_state();
    init();

    let db = driver_database();
    assert!(!db.is_empty());

    // Should have virtio entries.
    let has_virtio_blk = db.iter().any(|e| e.driver_name == "virtio-blk");
    assert!(has_virtio_blk);

    // Should have NVMe entry.
    let has_nvme = db.iter().any(|e| e.driver_name == "nvme");
    assert!(has_nvme);

    crate::serial_println!("  [devhotplug] test_init: ok");
}

fn test_driver_db_matching() {
    reset_state();
    init();

    // Match virtio-blk (vendor=0x1AF4, device=0x1001).
    let id = DeviceId {
        vendor_id: 0x1AF4,
        device_id: 0x1001,
        class: 0x01,
        subclass: 0x00,
    };
    let matched = find_matching_driver(&id);
    assert_eq!(matched.as_deref(), Some("virtio-blk"));

    // Match e1000 (vendor=0x8086, device=0x100E).
    let id = DeviceId {
        vendor_id: 0x8086,
        device_id: 0x100E,
        class: 0x02,
        subclass: 0x00,
    };
    let matched = find_matching_driver(&id);
    assert_eq!(matched.as_deref(), Some("e1000"));

    crate::serial_println!("  [devhotplug] test_driver_db_matching: ok");
}

fn test_wildcard_matching() {
    reset_state();
    init();

    // NVMe: class=01:08, any vendor/device.
    let id = DeviceId {
        vendor_id: 0x1234,
        device_id: 0x5678,
        class: 0x01,
        subclass: 0x08,
    };
    let matched = find_matching_driver(&id);
    assert_eq!(matched.as_deref(), Some("nvme"));

    // AHCI: class=01:06, any vendor/device.
    let id = DeviceId {
        vendor_id: 0xABCD,
        device_id: 0xEF01,
        class: 0x01,
        subclass: 0x06,
    };
    let matched = find_matching_driver(&id);
    assert_eq!(matched.as_deref(), Some("ahci"));

    crate::serial_println!("  [devhotplug] test_wildcard_matching: ok");
}

fn test_priority_matching() {
    reset_state();

    // Add two entries matching the same device, different priorities.
    add_driver_entry(0x1234, 0x5678, 0xFF, 0xFF, "low-pri", 50).unwrap();
    add_driver_entry(0x1234, 0x5678, 0xFF, 0xFF, "high-pri", 200).unwrap();

    let id = DeviceId {
        vendor_id: 0x1234,
        device_id: 0x5678,
        class: 0x01,
        subclass: 0x00,
    };
    let matched = find_matching_driver(&id);
    assert_eq!(matched.as_deref(), Some("high-pri"));

    crate::serial_println!("  [devhotplug] test_priority_matching: ok");
}

fn test_device_arrived() {
    reset_state();
    init();

    let addr = DeviceAddr::new(0, 3, 0);
    let id = DeviceId {
        vendor_id: 0x1AF4,
        device_id: 0x1001,
        class: 0x01,
        subclass: 0x00,
    };

    device_arrived_pci(addr, id);

    let pending = pending_events();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].kind, EventKind::DeviceArrived);
    assert_eq!(pending[0].matched_driver.as_deref(), Some("virtio-blk"));

    let st = stats();
    assert_eq!(st.total_arrived, 1);
    assert_eq!(st.pending_events, 1);

    crate::serial_println!("  [devhotplug] test_device_arrived: ok");
}

fn test_device_removed() {
    reset_state();

    let addr = DeviceAddr::new(0, 4, 0);
    device_removed_pci(addr);

    let pending = pending_events();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].kind, EventKind::DeviceRemoved);

    let st = stats();
    assert_eq!(st.total_removed, 1);

    crate::serial_println!("  [devhotplug] test_device_removed: ok");
}

fn test_drain_events() {
    reset_state();
    init();

    let addr = DeviceAddr::new(0, 5, 0);
    let id = DeviceId {
        vendor_id: 0x8086,
        device_id: 0x100E,
        class: 0x02,
        subclass: 0x00,
    };
    device_arrived_pci(addr, id);

    // Drain should return the event and mark it consumed.
    let events = drain_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::DeviceArrived);

    // No more pending events.
    assert_eq!(pending_count(), 0);

    // Second drain should be empty.
    let events2 = drain_events();
    assert!(events2.is_empty());

    crate::serial_println!("  [devhotplug] test_drain_events: ok");
}

fn test_event_overflow() {
    reset_state();

    // Fill the queue beyond MAX_EVENT_QUEUE.
    for i in 0..MAX_EVENT_QUEUE + 10 {
        let addr = DeviceAddr::new(0, (i & 0x1F) as u8, 0);
        device_removed_pci(addr);
    }

    let st = stats();
    // Should have dropped some events.
    assert!(st.events_dropped > 0);
    // Queue should not exceed MAX_EVENT_QUEUE.
    assert!(st.pending_events <= MAX_EVENT_QUEUE);

    crate::serial_println!("  [devhotplug] test_event_overflow: ok");
}

fn test_add_remove_db_entry() {
    reset_state();

    let id = add_driver_entry(0x1234, 0x5678, 0xFF, 0xFF, "test-driver", 100).unwrap();
    assert!(id > 0);

    let db = driver_database();
    assert_eq!(db.len(), 1);
    assert_eq!(db[0].driver_name, "test-driver");

    assert!(remove_driver_entry(id).is_ok());
    assert!(driver_database().is_empty());

    // Removing again should fail.
    assert_eq!(remove_driver_entry(id), Err(KernelError::NotFound));

    crate::serial_println!("  [devhotplug] test_add_remove_db_entry: ok");
}

fn test_enable_disable_entry() {
    reset_state();

    let id = add_driver_entry(0x1234, 0x5678, 0xFF, 0xFF, "toggle-drv", 100).unwrap();

    // Disable it.
    set_driver_entry_enabled(id, false).unwrap();

    // Should no longer match.
    let dev_id = DeviceId {
        vendor_id: 0x1234,
        device_id: 0x5678,
        class: 0x01,
        subclass: 0x00,
    };
    assert!(find_matching_driver(&dev_id).is_none());

    // Re-enable.
    set_driver_entry_enabled(id, true).unwrap();
    assert!(find_matching_driver(&dev_id).is_some());

    crate::serial_println!("  [devhotplug] test_enable_disable_entry: ok");
}

fn test_usb_event() {
    reset_state();

    let usb = UsbDeviceInfo {
        vendor_id: 0x046D,
        product_id: 0xC52B,
        class: 0x03,
        subclass: 0x01,
        port_path: String::from("1-2"),
        speed: UsbSpeed::Full,
    };

    device_arrived_usb(usb);

    let pending = pending_events();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].bus, BusType::Usb);
    assert!(pending[0].usb_info.is_some());

    crate::serial_println!("  [devhotplug] test_usb_event: ok");
}

fn test_stats() {
    reset_state();

    let st = stats();
    assert_eq!(st.total_arrived, 0);
    assert_eq!(st.total_removed, 0);
    assert_eq!(st.pending_events, 0);
    assert_eq!(st.events_dropped, 0);

    crate::serial_println!("  [devhotplug] test_stats: ok");
}

fn test_procfs() {
    reset_state();
    init();

    let addr = DeviceAddr::new(0, 6, 0);
    let id = DeviceId {
        vendor_id: 0x1AF4,
        device_id: 0x1050,
        class: 0x03,
        subclass: 0x00,
    };
    device_arrived_pci(addr, id);

    let content = procfs_content();
    assert!(content.contains("Device Hotplug System"));
    assert!(content.contains("virtio-gpu"));
    assert!(content.contains("arrived"));

    crate::serial_println!("  [devhotplug] test_procfs: ok");
}

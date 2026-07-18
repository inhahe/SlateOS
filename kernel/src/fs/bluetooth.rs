//! Bluetooth device management.
//!
//! Settings panel for Bluetooth discovery, pairing, and device management.
//! Similar to Windows Bluetooth settings, macOS Bluetooth preferences,
//! or GNOME/KDE Bluetooth panels.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Bluetooth
//!   → bluetooth::set_enabled() / scan() / pair()
//!
//! System tray indicator
//!   → bluetooth::is_enabled() / connected_devices()
//!
//! Integration:
//!   → soundmixer (audio routing for BT headphones)
//!   → kbsettings (BT keyboard detection)
//!   → power (BT power save on battery)
//! ```
//!
//! ## Device Types
//!
//! Audio (headphones, speakers), input (keyboard, mouse, gamepad),
//! phone, computer, wearable, health, printer, other.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_DEVICES: usize = 64;
const MAX_PAIRED: usize = 32;
const MAX_SCAN_RESULTS: usize = 32;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Bluetooth device class/type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    AudioHeadphones,
    AudioSpeaker,
    AudioHeadset,
    Keyboard,
    Mouse,
    Gamepad,
    Phone,
    Computer,
    Wearable,
    HealthDevice,
    Printer,
    Other,
}

impl DeviceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::AudioHeadphones => "Headphones",
            Self::AudioSpeaker => "Speaker",
            Self::AudioHeadset => "Headset",
            Self::Keyboard => "Keyboard",
            Self::Mouse => "Mouse",
            Self::Gamepad => "Gamepad",
            Self::Phone => "Phone",
            Self::Computer => "Computer",
            Self::Wearable => "Wearable",
            Self::HealthDevice => "Health Device",
            Self::Printer => "Printer",
            Self::Other => "Other",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::AudioHeadphones | Self::AudioHeadset => "🎧",
            Self::AudioSpeaker => "🔊",
            Self::Keyboard => "⌨",
            Self::Mouse => "🖱",
            Self::Gamepad => "🎮",
            Self::Phone => "📱",
            Self::Computer => "💻",
            Self::Wearable => "⌚",
            Self::HealthDevice => "❤",
            Self::Printer => "🖨",
            Self::Other => "📡",
        }
    }

    /// Whether this device type supports audio profiles.
    pub fn is_audio(self) -> bool {
        matches!(self, Self::AudioHeadphones | Self::AudioSpeaker | Self::AudioHeadset)
    }
}

/// Connection state of a Bluetooth device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Failed,
}

impl ConnectionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting",
            Self::Connected => "Connected",
            Self::Disconnecting => "Disconnecting",
            Self::Failed => "Failed",
        }
    }
}

/// Bluetooth adapter power state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterState {
    Off,
    On,
    Discoverable,
    Scanning,
}

impl AdapterState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::On => "On",
            Self::Discoverable => "Discoverable",
            Self::Scanning => "Scanning",
        }
    }
}

/// Bluetooth protocol profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtProfile {
    /// Advanced Audio Distribution Profile.
    A2dp,
    /// Hands-Free Profile.
    Hfp,
    /// Human Interface Device.
    Hid,
    /// Serial Port Profile.
    Spp,
    /// Personal Area Network.
    Pan,
    /// Object Push Profile (file transfer).
    Opp,
    /// Audio/Video Remote Control.
    Avrcp,
}

impl BtProfile {
    pub fn label(self) -> &'static str {
        match self {
            Self::A2dp => "A2DP",
            Self::Hfp => "HFP",
            Self::Hid => "HID",
            Self::Spp => "SPP",
            Self::Pan => "PAN",
            Self::Opp => "OPP",
            Self::Avrcp => "AVRCP",
        }
    }
}

/// A known/paired Bluetooth device.
#[derive(Debug, Clone)]
pub struct BtDevice {
    /// Unique device address (MAC-like, e.g., "AA:BB:CC:DD:EE:FF").
    pub address: String,
    /// Device display name.
    pub name: String,
    /// Device type.
    pub device_type: DeviceType,
    /// Connection state.
    pub state: ConnectionState,
    /// Whether the device is paired.
    pub paired: bool,
    /// Whether the device is trusted (auto-connect).
    pub trusted: bool,
    /// Whether the device is blocked.
    pub blocked: bool,
    /// Signal strength (RSSI) in dBm (-100..0, higher = closer).
    pub rssi: i8,
    /// Battery level (0–100), if reported by device.
    pub battery_pct: Option<u8>,
    /// Supported profiles.
    pub profiles: Vec<BtProfile>,
    /// Last seen timestamp (ns).
    pub last_seen_ns: u64,
    /// Last connected timestamp (ns).
    pub last_connected_ns: u64,
    /// Connection count.
    pub connect_count: u64,
    /// Firmware version, if available.
    pub firmware: String,
}

/// Scan result for a nearby device.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub address: String,
    pub name: String,
    pub device_type: DeviceType,
    pub rssi: i8,
    pub paired: bool,
}

/// Bluetooth adapter configuration.
#[derive(Debug, Clone)]
pub struct BtConfig {
    /// Whether Bluetooth is enabled.
    pub enabled: bool,
    /// Adapter state.
    pub adapter_state: AdapterState,
    /// Adapter name (visible to other devices).
    pub adapter_name: String,
    /// Whether the adapter is discoverable.
    pub discoverable: bool,
    /// Discoverable timeout in seconds (0 = always).
    pub discoverable_timeout: u32,
    /// Whether to auto-connect trusted devices.
    pub auto_connect: bool,
    /// Whether to disable Bluetooth on battery.
    pub disable_on_battery: bool,
    /// Bluetooth version (e.g., "5.3").
    pub bt_version: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct BtState {
    config: BtConfig,
    devices: Vec<BtDevice>,
    scan_results: Vec<ScanResult>,
    scan_count: u64,
    pair_count: u64,
    ops: u64,
}

static STATE: Mutex<Option<BtState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut BtState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the Bluetooth subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(BtState {
        config: BtConfig {
            enabled: false,
            adapter_state: AdapterState::Off,
            adapter_name: String::from("My Computer"),
            discoverable: false,
            discoverable_timeout: 120,
            auto_connect: true,
            disable_on_battery: false,
            bt_version: String::from("5.3"),
        },
        devices: Vec::new(),
        scan_results: Vec::new(),
        scan_count: 0,
        pair_count: 0,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Adapter control
// ---------------------------------------------------------------------------

/// Enable Bluetooth.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.enabled = enabled;
        state.config.adapter_state = if enabled {
            AdapterState::On
        } else {
            AdapterState::Off
        };
        if !enabled {
            // Disconnect all devices when turning off.
            for dev in &mut state.devices {
                dev.state = ConnectionState::Disconnected;
            }
            state.scan_results.clear();
        }
        Ok(())
    })
}

/// Check if Bluetooth is enabled.
pub fn is_enabled() -> bool {
    let guard = STATE.lock();
    guard.as_ref().is_some_and(|s| s.config.enabled)
}

/// Set the adapter display name.
pub fn set_adapter_name(name: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        state.config.adapter_name = String::from(name);
        Ok(())
    })
}

/// Set discoverable mode.
pub fn set_discoverable(discoverable: bool) -> KernelResult<()> {
    with_state(|state| {
        if !state.config.enabled {
            return Err(KernelError::NotSupported);
        }
        state.config.discoverable = discoverable;
        if discoverable {
            state.config.adapter_state = AdapterState::Discoverable;
        } else if state.config.enabled {
            state.config.adapter_state = AdapterState::On;
        }
        Ok(())
    })
}

/// Set discoverable timeout in seconds.
pub fn set_discoverable_timeout(seconds: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.discoverable_timeout = seconds;
        Ok(())
    })
}

/// Set auto-connect for trusted devices.
pub fn set_auto_connect(auto: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.auto_connect = auto;
        Ok(())
    })
}

/// Set disable-on-battery mode.
pub fn set_disable_on_battery(disable: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.disable_on_battery = disable;
        Ok(())
    })
}

/// Get current adapter configuration.
pub fn config() -> KernelResult<BtConfig> {
    let guard = STATE.lock();
    guard.as_ref()
        .map(|s| s.config.clone())
        .ok_or(KernelError::NotSupported)
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Start a scan for nearby devices.
///
/// In a real OS, this would trigger HCI inquiry. Here we simulate
/// by populating the scan results with any unpaired devices that
/// have been added via `add_scan_result`.
pub fn scan() -> KernelResult<Vec<ScanResult>> {
    with_state(|state| {
        if !state.config.enabled {
            return Err(KernelError::NotSupported);
        }
        state.config.adapter_state = AdapterState::Scanning;
        state.scan_count += 1;
        Ok(state.scan_results.clone())
    })
}

/// Add a scan result (for testing/simulation).
pub fn add_scan_result(
    address: &str,
    name: &str,
    device_type: DeviceType,
    rssi: i8,
) -> KernelResult<()> {
    with_state(|state| {
        if state.scan_results.len() >= MAX_SCAN_RESULTS {
            state.scan_results.remove(0);
        }
        let paired = state.devices.iter().any(|d| d.address == address && d.paired);
        state.scan_results.push(ScanResult {
            address: String::from(address),
            name: String::from(name),
            device_type,
            rssi,
            paired,
        });
        Ok(())
    })
}

/// Clear scan results.
pub fn clear_scan() -> KernelResult<()> {
    with_state(|state| {
        state.scan_results.clear();
        state.config.adapter_state = if state.config.discoverable {
            AdapterState::Discoverable
        } else {
            AdapterState::On
        };
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Pairing and device management
// ---------------------------------------------------------------------------

/// Pair with a device by address.
pub fn pair(address: &str, name: &str, device_type: DeviceType) -> KernelResult<()> {
    with_state(|state| {
        if !state.config.enabled {
            return Err(KernelError::NotSupported);
        }
        if state.devices.iter().any(|d| d.address == address && d.paired) {
            return Err(KernelError::AlreadyExists);
        }
        if state.devices.len() >= MAX_PAIRED {
            return Err(KernelError::ResourceExhausted);
        }

        let now = crate::hpet::elapsed_ns();
        let profiles = default_profiles(device_type);

        state.devices.push(BtDevice {
            address: String::from(address),
            name: String::from(name),
            device_type,
            state: ConnectionState::Disconnected,
            paired: true,
            trusted: false,
            blocked: false,
            rssi: -50,
            battery_pct: None,
            profiles,
            last_seen_ns: now,
            last_connected_ns: 0,
            connect_count: 0,
            firmware: String::new(),
        });
        state.pair_count += 1;
        Ok(())
    })
}

/// Unpair a device.
pub fn unpair(address: &str) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.devices.iter().position(|d| d.address == address) {
            state.devices.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Connect to a paired device.
pub fn connect(address: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.config.enabled {
            return Err(KernelError::NotSupported);
        }
        let dev = state.devices.iter_mut()
            .find(|d| d.address == address)
            .ok_or(KernelError::NotFound)?;
        if dev.blocked {
            return Err(KernelError::PermissionDenied);
        }
        dev.state = ConnectionState::Connected;
        dev.last_connected_ns = crate::hpet::elapsed_ns();
        dev.connect_count += 1;
        Ok(())
    })
}

/// Disconnect from a device.
pub fn disconnect(address: &str) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut()
            .find(|d| d.address == address)
            .ok_or(KernelError::NotFound)?;
        dev.state = ConnectionState::Disconnected;
        Ok(())
    })
}

/// Set a device as trusted (auto-connect on discovery).
pub fn set_trusted(address: &str, trusted: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut()
            .find(|d| d.address == address)
            .ok_or(KernelError::NotFound)?;
        dev.trusted = trusted;
        Ok(())
    })
}

/// Block a device (prevent connection).
pub fn set_blocked(address: &str, blocked: bool) -> KernelResult<()> {
    with_state(|state| {
        let dev = state.devices.iter_mut()
            .find(|d| d.address == address)
            .ok_or(KernelError::NotFound)?;
        dev.blocked = blocked;
        if blocked && dev.state == ConnectionState::Connected {
            dev.state = ConnectionState::Disconnected;
        }
        Ok(())
    })
}

/// Rename a paired device.
pub fn set_device_name(address: &str, name: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let dev = state.devices.iter_mut()
            .find(|d| d.address == address)
            .ok_or(KernelError::NotFound)?;
        dev.name = String::from(name);
        Ok(())
    })
}

/// Update device battery level.
pub fn update_battery(address: &str, pct: u8) -> KernelResult<()> {
    if pct > 100 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let dev = state.devices.iter_mut()
            .find(|d| d.address == address)
            .ok_or(KernelError::NotFound)?;
        dev.battery_pct = Some(pct);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Get a specific device by address.
pub fn get_device(address: &str) -> KernelResult<BtDevice> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.devices.iter()
        .find(|d| d.address == address)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all paired devices.
pub fn list_devices() -> Vec<BtDevice> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.devices.clone())
}

/// List connected devices.
pub fn connected_devices() -> Vec<BtDevice> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.devices.iter()
            .filter(|d| d.state == ConnectionState::Connected)
            .cloned()
            .collect()
    })
}

/// List devices by type.
pub fn devices_by_type(dt: DeviceType) -> Vec<BtDevice> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.devices.iter()
            .filter(|d| d.device_type == dt)
            .cloned()
            .collect()
    })
}

/// Get connected audio devices (for soundmixer integration).
pub fn audio_devices() -> Vec<BtDevice> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.devices.iter()
            .filter(|d| d.device_type.is_audio() && d.state == ConnectionState::Connected)
            .cloned()
            .collect()
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_profiles(dt: DeviceType) -> Vec<BtProfile> {
    match dt {
        DeviceType::AudioHeadphones | DeviceType::AudioSpeaker => {
            alloc::vec![BtProfile::A2dp, BtProfile::Avrcp]
        }
        DeviceType::AudioHeadset => {
            alloc::vec![BtProfile::A2dp, BtProfile::Hfp, BtProfile::Avrcp]
        }
        DeviceType::Keyboard | DeviceType::Mouse | DeviceType::Gamepad => {
            alloc::vec![BtProfile::Hid]
        }
        DeviceType::Phone => {
            alloc::vec![BtProfile::A2dp, BtProfile::Hfp, BtProfile::Opp]
        }
        DeviceType::Computer => {
            alloc::vec![BtProfile::Pan, BtProfile::Opp]
        }
        DeviceType::Printer => {
            alloc::vec![BtProfile::Spp]
        }
        _ => alloc::vec![BtProfile::Spp],
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (device_count, connected_count, scan_count, pair_count, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let connected = s.devices.iter()
                .filter(|d| d.state == ConnectionState::Connected)
                .count();
            (s.devices.len(), connected, s.scan_count, s.pair_count, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the Bluetooth module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[bluetooth] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state.
    {
        assert!(!is_enabled());
        let cfg = config().unwrap();
        assert_eq!(cfg.adapter_state, AdapterState::Off);
        assert!(cfg.auto_connect);
    }
    serial_println!("[bluetooth]  1/11 initial state OK");

    // Test 2: enable/disable.
    {
        set_enabled(true).unwrap();
        assert!(is_enabled());
        let cfg = config().unwrap();
        assert_eq!(cfg.adapter_state, AdapterState::On);

        set_enabled(false).unwrap();
        assert!(!is_enabled());
    }
    serial_println!("[bluetooth]  2/11 enable/disable OK");

    // Test 3: adapter name.
    {
        set_adapter_name("Test PC").unwrap();
        let cfg = config().unwrap();
        assert_eq!(cfg.adapter_name, "Test PC");
        assert!(set_adapter_name("").is_err());
    }
    serial_println!("[bluetooth]  3/11 adapter name OK");

    // Test 4: discoverable.
    {
        set_enabled(true).unwrap();
        set_discoverable(true).unwrap();
        let cfg = config().unwrap();
        assert!(cfg.discoverable);
        assert_eq!(cfg.adapter_state, AdapterState::Discoverable);
        set_discoverable(false).unwrap();
    }
    serial_println!("[bluetooth]  4/11 discoverable OK");

    // Test 5: pair device.
    {
        pair("AA:BB:CC:DD:EE:01", "My Headphones", DeviceType::AudioHeadphones).unwrap();
        let dev = get_device("AA:BB:CC:DD:EE:01").unwrap();
        assert_eq!(dev.name, "My Headphones");
        assert!(dev.paired);
        assert_eq!(dev.state, ConnectionState::Disconnected);
        assert!(dev.device_type.is_audio());

        // Duplicate pairing should fail.
        assert!(pair("AA:BB:CC:DD:EE:01", "Dup", DeviceType::Other).is_err());
    }
    serial_println!("[bluetooth]  5/11 pair OK");

    // Test 6: connect/disconnect.
    {
        connect("AA:BB:CC:DD:EE:01").unwrap();
        let dev = get_device("AA:BB:CC:DD:EE:01").unwrap();
        assert_eq!(dev.state, ConnectionState::Connected);
        assert_eq!(dev.connect_count, 1);

        disconnect("AA:BB:CC:DD:EE:01").unwrap();
        let dev = get_device("AA:BB:CC:DD:EE:01").unwrap();
        assert_eq!(dev.state, ConnectionState::Disconnected);
    }
    serial_println!("[bluetooth]  6/11 connect/disconnect OK");

    // Test 7: trust/block.
    {
        set_trusted("AA:BB:CC:DD:EE:01", true).unwrap();
        let dev = get_device("AA:BB:CC:DD:EE:01").unwrap();
        assert!(dev.trusted);

        set_blocked("AA:BB:CC:DD:EE:01", true).unwrap();
        let dev = get_device("AA:BB:CC:DD:EE:01").unwrap();
        assert!(dev.blocked);
        // Cannot connect while blocked.
        assert!(connect("AA:BB:CC:DD:EE:01").is_err());
        set_blocked("AA:BB:CC:DD:EE:01", false).unwrap();
    }
    serial_println!("[bluetooth]  7/11 trust/block OK");

    // Test 8: scan.
    {
        add_scan_result("FF:FF:FF:FF:FF:01", "BT Speaker", DeviceType::AudioSpeaker, -60).unwrap();
        let results = scan().unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|r| r.name == "BT Speaker"));
    }
    serial_println!("[bluetooth]  8/11 scan OK");

    // Test 9: battery update.
    {
        update_battery("AA:BB:CC:DD:EE:01", 85).unwrap();
        let dev = get_device("AA:BB:CC:DD:EE:01").unwrap();
        assert_eq!(dev.battery_pct, Some(85));
        assert!(update_battery("AA:BB:CC:DD:EE:01", 101).is_err());
    }
    serial_println!("[bluetooth]  9/11 battery OK");

    // Test 10: unpair.
    {
        pair("AA:BB:CC:DD:EE:02", "Test Mouse", DeviceType::Mouse).unwrap();
        unpair("AA:BB:CC:DD:EE:02").unwrap();
        assert!(get_device("AA:BB:CC:DD:EE:02").is_err());
    }
    serial_println!("[bluetooth] 10/11 unpair OK");

    // Test 11: device queries.
    {
        connect("AA:BB:CC:DD:EE:01").unwrap();
        let connected = connected_devices();
        assert!(!connected.is_empty());
        let audio = audio_devices();
        assert!(!audio.is_empty());

        let (total, conn, scans, pairs, _) = stats();
        assert!(total > 0);
        assert!(conn > 0);
        assert!(scans > 0);
        assert!(pairs > 0);

        set_enabled(false).unwrap();
    }
    serial_println!("[bluetooth] 11/11 queries OK");

    // Leave no residue for later callers / the live /proc/bluetooth view: the
    // test paired "My Headphones" (connected), trusted it, and added a scan
    // result — none of which represents real hardware. Reset to None so the
    // procfs view and `bluetooth` shell command report an empty adapter.
    *STATE.lock() = None;

    serial_println!("[bluetooth] All self-tests passed.");
}

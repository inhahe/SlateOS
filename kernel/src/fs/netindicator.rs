//! Network status indicator — WiFi/Ethernet status for taskbar and settings.
//!
//! Provides network connection state for the system tray indicator, WiFi
//! network scanning/selection, and connection management UI.
//!
//! ## Design Reference
//!
//! design.txt line 711: "wifi" icon on taskbar
//! design.txt lines 1254-1260: network settings (ethernet, wifi, dns, dhcp)
//!
//! ## Architecture
//!
//! ```text
//! Network driver
//!   → netindicator::update_interface(name, state)
//!
//! System tray
//!   → netindicator::connection_state() → icon + tooltip
//!
//! Settings panel
//!   → netindicator::list_wifi()
//!   → netindicator::connect_wifi(ssid, password)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum network interfaces tracked.
const MAX_INTERFACES: usize = 16;

/// Maximum visible WiFi networks.
const MAX_WIFI_NETWORKS: usize = 128;

/// Maximum saved WiFi profiles.
const MAX_SAVED_PROFILES: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Type of network interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceType {
    Ethernet,
    Wifi,
    Loopback,
    Vpn,
    Bridge,
    Virtual,
}

impl InterfaceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "ethernet",
            Self::Wifi => "wifi",
            Self::Loopback => "loopback",
            Self::Vpn => "vpn",
            Self::Bridge => "bridge",
            Self::Virtual => "virtual",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "ethernet" | "eth" => Some(Self::Ethernet),
            "wifi" | "wlan" => Some(Self::Wifi),
            "loopback" | "lo" => Some(Self::Loopback),
            "vpn" => Some(Self::Vpn),
            "bridge" | "br" => Some(Self::Bridge),
            "virtual" | "virt" => Some(Self::Virtual),
            _ => None,
        }
    }
}

/// Connection state of an interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Limited,
    NoInternet,
    Disabled,
}

impl ConnectionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Limited => "limited",
            Self::NoInternet => "no-internet",
            Self::Disabled => "disabled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "disconnected" | "down" => Some(Self::Disconnected),
            "connecting" => Some(Self::Connecting),
            "connected" | "up" => Some(Self::Connected),
            "limited" => Some(Self::Limited),
            "no-internet" => Some(Self::NoInternet),
            "disabled" | "off" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// WiFi security type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    Wep,
    Wpa,
    WPA2,
    WPA3,
    Enterprise,
}

impl WifiSecurity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Wep => "WEP",
            Self::Wpa => "WPA",
            Self::WPA2 => "WPA2",
            Self::WPA3 => "WPA3",
            Self::Enterprise => "enterprise",
        }
    }
}

/// A network interface.
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    /// Interface name (e.g., "eth0", "wlan0").
    pub name: String,
    /// Type.
    pub iface_type: InterfaceType,
    /// Connection state.
    pub state: ConnectionState,
    /// IPv4 address (if connected).
    pub ipv4: String,
    /// IPv6 address (if connected).
    pub ipv6: String,
    /// Gateway.
    pub gateway: String,
    /// DNS servers.
    pub dns: Vec<String>,
    /// Whether DHCP is active.
    pub dhcp: bool,
    /// MAC address.
    pub mac: String,
    /// Link speed in Mbps (0 = unknown).
    pub speed_mbps: u32,
    /// For WiFi: connected SSID.
    pub ssid: String,
    /// For WiFi: signal strength (0-100).
    pub signal: u8,
}

/// A visible WiFi network.
#[derive(Debug, Clone)]
pub struct WifiNetwork {
    /// SSID name.
    pub ssid: String,
    /// Signal strength (0-100).
    pub signal: u8,
    /// Security type.
    pub security: WifiSecurity,
    /// Channel.
    pub channel: u32,
    /// Frequency in MHz.
    pub frequency: u32,
    /// Whether we have a saved profile for this network.
    pub saved: bool,
}

/// A saved WiFi profile.
#[derive(Debug, Clone)]
pub struct WifiProfile {
    /// SSID.
    pub ssid: String,
    /// Security type.
    pub security: WifiSecurity,
    /// Auto-connect on discovery.
    pub auto_connect: bool,
    /// Hidden network (must probe for it).
    pub hidden: bool,
    /// Metered connection (limit background data).
    pub metered: bool,
}

/// DNS configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsMode {
    /// Automatic from DHCP.
    Auto,
    /// Manual DNS servers.
    Manual,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    interfaces: Vec<NetworkInterface>,
    wifi_networks: Vec<WifiNetwork>,
    profiles: Vec<WifiProfile>,
    /// Whether airplane mode is on.
    airplane_mode: bool,
    /// Global DNS mode.
    dns_mode: DnsMode,
    /// Manual DNS servers (when dns_mode = Manual).
    manual_dns: Vec<String>,
}

impl State {
    const fn new() -> Self {
        Self {
            interfaces: Vec::new(),
            wifi_networks: Vec::new(),
            profiles: Vec::new(),
            airplane_mode: false,
            dns_mode: DnsMode::Auto,
            manual_dns: Vec::new(),
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static SCAN_COUNT: AtomicU64 = AtomicU64::new(0);
static CONNECT_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Interface management
// ---------------------------------------------------------------------------

/// Add or update a network interface.
pub fn update_interface(
    name: &str,
    iface_type: InterfaceType,
    mac: &str,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    if let Some(iface) = state.interfaces.iter_mut().find(|i| i.name == name) {
        iface.iface_type = iface_type;
        iface.mac = String::from(mac);
        return Ok(());
    }
    if state.interfaces.len() >= MAX_INTERFACES {
        return Err(KernelError::ResourceExhausted);
    }
    state.interfaces.push(NetworkInterface {
        name: String::from(name),
        iface_type,
        state: ConnectionState::Disconnected,
        ipv4: String::new(),
        ipv6: String::new(),
        gateway: String::new(),
        dns: Vec::new(),
        dhcp: true,
        mac: String::from(mac),
        speed_mbps: 0,
        ssid: String::new(),
        signal: 0,
    });
    Ok(())
}

/// Remove an interface.
pub fn remove_interface(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.interfaces.len();
    state.interfaces.retain(|i| i.name != name);
    if state.interfaces.len() == len {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

/// Set interface connection state.
pub fn set_state(name: &str, conn_state: ConnectionState) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    iface.state = conn_state;
    Ok(())
}

/// Set interface IP address.
pub fn set_ip(name: &str, ipv4: &str, gateway: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    iface.ipv4 = String::from(ipv4);
    iface.gateway = String::from(gateway);
    Ok(())
}

/// Set WiFi status for an interface.
pub fn set_wifi_status(name: &str, ssid: &str, signal: u8) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    iface.ssid = String::from(ssid);
    iface.signal = signal.min(100);
    Ok(())
}

/// Set link speed.
pub fn set_speed(name: &str, mbps: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    iface.speed_mbps = mbps;
    Ok(())
}

/// List all interfaces.
pub fn list_interfaces() -> Vec<NetworkInterface> {
    STATE.lock().interfaces.clone()
}

/// Get interface by name.
pub fn get_interface(name: &str) -> Option<NetworkInterface> {
    STATE.lock().interfaces.iter().find(|i| i.name == name).cloned()
}

/// Get the primary connected interface (first connected one).
pub fn primary_interface() -> Option<NetworkInterface> {
    let state = STATE.lock();
    state.interfaces.iter()
        .find(|i| i.state == ConnectionState::Connected)
        .cloned()
}

/// Overall connection summary for system tray icon.
pub fn connection_summary() -> (ConnectionState, String) {
    let state = STATE.lock();
    if state.airplane_mode {
        return (ConnectionState::Disabled, String::from("Airplane mode"));
    }
    for iface in &state.interfaces {
        if iface.state == ConnectionState::Connected {
            let desc = if iface.iface_type == InterfaceType::Wifi {
                alloc::format!("{} ({}%)", iface.ssid, iface.signal)
            } else {
                alloc::format!("{} {}Mbps", iface.name, iface.speed_mbps)
            };
            return (ConnectionState::Connected, desc);
        }
    }
    (ConnectionState::Disconnected, String::from("Not connected"))
}

// ---------------------------------------------------------------------------
// WiFi scanning
// ---------------------------------------------------------------------------

/// Report a WiFi scan result.
pub fn report_wifi(ssid: &str, signal: u8, security: WifiSecurity, channel: u32, freq: u32) {
    let mut state = STATE.lock();
    // Check profiles first (before taking mutable borrow on wifi_networks).
    let is_saved = state.profiles.iter().any(|p| p.ssid == ssid);
    // Update or add.
    if let Some(n) = state.wifi_networks.iter_mut().find(|n| n.ssid == ssid) {
        n.signal = signal;
        n.security = security;
        n.channel = channel;
        n.frequency = freq;
        n.saved = is_saved;
    } else if state.wifi_networks.len() < MAX_WIFI_NETWORKS {
        state.wifi_networks.push(WifiNetwork {
            ssid: String::from(ssid),
            signal,
            security,
            channel,
            frequency: freq,
            saved: is_saved,
        });
    }
}

/// Trigger a WiFi scan (clears old results).
pub fn scan_wifi() {
    let mut state = STATE.lock();
    state.wifi_networks.clear();
    SCAN_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Get visible WiFi networks (sorted by signal strength).
pub fn wifi_networks() -> Vec<WifiNetwork> {
    let state = STATE.lock();
    let mut nets = state.wifi_networks.clone();
    nets.sort_by_key(|e| core::cmp::Reverse(e.signal));
    nets
}

/// Simulate connecting to a WiFi network.
pub fn connect_wifi(ssid: &str, _password: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Look up signal before taking mutable borrow on interfaces.
    let signal = state.wifi_networks.iter().find(|n| n.ssid == ssid)
        .map(|n| n.signal)
        .ok_or(KernelError::NotFound)?;
    // Find the WiFi interface and update it.
    if let Some(iface) = state.interfaces.iter_mut().find(|i| i.iface_type == InterfaceType::Wifi) {
        iface.state = ConnectionState::Connected;
        iface.ssid = String::from(ssid);
        iface.signal = signal;
    }
    CONNECT_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Disconnect WiFi.
pub fn disconnect_wifi() -> KernelResult<()> {
    let mut state = STATE.lock();
    if let Some(iface) = state.interfaces.iter_mut().find(|i| i.iface_type == InterfaceType::Wifi) {
        iface.state = ConnectionState::Disconnected;
        iface.ssid.clear();
        iface.signal = 0;
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

// ---------------------------------------------------------------------------
// WiFi profiles
// ---------------------------------------------------------------------------

/// Save a WiFi profile.
pub fn save_profile(ssid: &str, security: WifiSecurity, auto_connect: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    if let Some(p) = state.profiles.iter_mut().find(|p| p.ssid == ssid) {
        p.security = security;
        p.auto_connect = auto_connect;
        return Ok(());
    }
    if state.profiles.len() >= MAX_SAVED_PROFILES {
        return Err(KernelError::ResourceExhausted);
    }
    state.profiles.push(WifiProfile {
        ssid: String::from(ssid),
        security,
        auto_connect,
        hidden: false,
        metered: false,
    });
    Ok(())
}

/// Remove a saved profile.
pub fn forget_profile(ssid: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.profiles.len();
    state.profiles.retain(|p| p.ssid != ssid);
    if state.profiles.len() == len { return Err(KernelError::NotFound); }
    Ok(())
}

/// List saved profiles.
pub fn list_profiles() -> Vec<WifiProfile> {
    STATE.lock().profiles.clone()
}

// ---------------------------------------------------------------------------
// Airplane mode
// ---------------------------------------------------------------------------

/// Set airplane mode.
pub fn set_airplane_mode(on: bool) {
    STATE.lock().airplane_mode = on;
}

/// Get airplane mode.
pub fn airplane_mode() -> bool {
    STATE.lock().airplane_mode
}

// ---------------------------------------------------------------------------
// DNS
// ---------------------------------------------------------------------------

/// Set DNS to auto (from DHCP).
pub fn set_dns_auto() {
    let mut state = STATE.lock();
    state.dns_mode = DnsMode::Auto;
    state.manual_dns.clear();
}

/// Set manual DNS servers.
pub fn set_dns_manual(servers: &[&str]) -> KernelResult<()> {
    if servers.is_empty() { return Err(KernelError::InvalidArgument); }
    let mut state = STATE.lock();
    state.dns_mode = DnsMode::Manual;
    state.manual_dns = servers.iter().map(|s| String::from(*s)).collect();
    Ok(())
}

/// Get DNS config.
pub fn dns_config() -> (DnsMode, Vec<String>) {
    let state = STATE.lock();
    (state.dns_mode, state.manual_dns.clone())
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (interface_count, wifi_count, profile_count, scan_count, connect_count).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let state = STATE.lock();
    (
        state.interfaces.len(),
        state.wifi_networks.len(),
        state.profiles.len(),
        SCAN_COUNT.load(Ordering::Relaxed),
        CONNECT_COUNT.load(Ordering::Relaxed),
    )
}

pub fn reset_stats() {
    SCAN_COUNT.store(0, Ordering::Relaxed);
    CONNECT_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.interfaces.clear();
    state.wifi_networks.clear();
    state.profiles.clear();
    state.airplane_mode = false;
    state.dns_mode = DnsMode::Auto;
    state.manual_dns.clear();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Add interfaces.
    serial_println!("  netindicator::self_test 1: interfaces");
    update_interface("eth0", InterfaceType::Ethernet, "AA:BB:CC:DD:EE:FF")?;
    update_interface("wlan0", InterfaceType::Wifi, "11:22:33:44:55:66")?;
    assert_eq!(list_interfaces().len(), 2);

    // Test 2: Set state and IP.
    serial_println!("  netindicator::self_test 2: state/ip");
    set_state("eth0", ConnectionState::Connected)?;
    set_ip("eth0", "192.168.1.100", "192.168.1.1")?;
    set_speed("eth0", 1000)?;
    let eth = get_interface("eth0").unwrap();
    assert_eq!(eth.state, ConnectionState::Connected);
    assert_eq!(eth.ipv4, "192.168.1.100");
    assert_eq!(eth.speed_mbps, 1000);

    // Test 3: Connection summary.
    serial_println!("  netindicator::self_test 3: summary");
    let (cs, desc) = connection_summary();
    assert_eq!(cs, ConnectionState::Connected);
    assert!(desc.contains("eth0"));

    // Test 4: WiFi scan.
    serial_println!("  netindicator::self_test 4: wifi scan");
    scan_wifi();
    report_wifi("HomeNetwork", 80, WifiSecurity::WPA2, 6, 2437);
    report_wifi("CoffeeShop", 45, WifiSecurity::Open, 11, 2462);
    report_wifi("Office5G", 90, WifiSecurity::WPA3, 36, 5180);
    let nets = wifi_networks();
    assert_eq!(nets.len(), 3);
    assert_eq!(nets[0].ssid, "Office5G"); // Strongest first.

    // Test 5: Connect/disconnect WiFi.
    serial_println!("  netindicator::self_test 5: wifi connect");
    connect_wifi("HomeNetwork", "password123")?;
    let wlan = get_interface("wlan0").unwrap();
    assert_eq!(wlan.state, ConnectionState::Connected);
    assert_eq!(wlan.ssid, "HomeNetwork");
    disconnect_wifi()?;
    let wlan2 = get_interface("wlan0").unwrap();
    assert_eq!(wlan2.state, ConnectionState::Disconnected);

    // Test 6: Profiles.
    serial_println!("  netindicator::self_test 6: profiles");
    save_profile("HomeNetwork", WifiSecurity::WPA2, true)?;
    save_profile("Office5G", WifiSecurity::WPA3, true)?;
    assert_eq!(list_profiles().len(), 2);
    forget_profile("HomeNetwork")?;
    assert_eq!(list_profiles().len(), 1);

    // Test 7: Airplane mode and DNS.
    serial_println!("  netindicator::self_test 7: airplane/dns");
    set_airplane_mode(true);
    let (cs2, _) = connection_summary();
    assert_eq!(cs2, ConnectionState::Disabled);
    set_airplane_mode(false);
    set_dns_manual(&["8.8.8.8", "1.1.1.1"])?;
    let (mode, servers) = dns_config();
    assert_eq!(mode, DnsMode::Manual);
    assert_eq!(servers.len(), 2);
    set_dns_auto();
    let (mode2, _) = dns_config();
    assert_eq!(mode2, DnsMode::Auto);

    let (ic, _wc, _pc, sc, cc) = stats();
    assert_eq!(ic, 2);
    assert!(sc > 0);
    assert!(cc > 0);

    clear_all();
    reset_stats();
    serial_println!("  netindicator: all tests passed");
    Ok(())
}

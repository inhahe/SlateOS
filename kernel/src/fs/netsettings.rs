//! Comprehensive network settings panel.
//!
//! Manages Ethernet, Wi-Fi, DNS, DHCP, IPv4/IPv6 configuration,
//! gateway/router detection, LAN/WAN IP display, and advanced
//! network options.
//!
//! ## Design Reference
//!
//! design.txt lines 1254-1269:
//! - ethernet, wifi, wifi selection, password
//! - DNS servers (auto or manual)
//! - DHCP or static
//! - IPv4 / IPv6
//! - firewall (basic toggle — detailed rules in firewall module)
//! - router detection (default gateway)
//! - LAN IP address, internet IP addresses
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Network
//!   → netsettings::list_interfaces()
//!   → netsettings::wifi_scan()
//!   → netsettings::connect_wifi(ssid, password)
//!   → netsettings::set_dns(interface, servers)
//!
//! Network daemon
//!   → netsettings::active_config(iface)
//!   → applies DHCP or static config to interface
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Network interface type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceType {
    /// Wired Ethernet.
    Ethernet,
    /// Wireless (Wi-Fi).
    Wifi,
    /// Loopback.
    Loopback,
    /// Virtual / bridge.
    Virtual,
    /// VPN tunnel.
    Vpn,
}

/// Interface link state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// Cable connected / associated.
    Up,
    /// No cable / not associated.
    Down,
    /// Disabled by user.
    Disabled,
}

/// IP configuration method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpMethod {
    /// Automatic (DHCP / SLAAC).
    Auto,
    /// Manual / static.
    Manual,
    /// Disabled.
    Disabled,
}

/// Wi-Fi security type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    /// Open (no encryption).
    Open,
    /// WEP (legacy, insecure).
    Wep,
    /// WPA2 Personal (PSK).
    Wpa2Personal,
    /// WPA3 Personal (SAE).
    Wpa3Personal,
    /// WPA2 Enterprise (802.1X).
    Wpa2Enterprise,
    /// WPA3 Enterprise.
    Wpa3Enterprise,
}

/// Wi-Fi band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiBand {
    /// 2.4 GHz.
    Band2g,
    /// 5 GHz.
    Band5g,
    /// 6 GHz (Wi-Fi 6E).
    Band6g,
}

/// A scanned Wi-Fi network.
#[derive(Debug, Clone)]
pub struct WifiNetwork {
    /// Network name (SSID).
    pub ssid: String,
    /// Signal strength (0-100).
    pub signal: u32,
    /// Security type.
    pub security: WifiSecurity,
    /// Band.
    pub band: WifiBand,
    /// Channel number.
    pub channel: u32,
    /// Whether this is a known/saved network.
    pub saved: bool,
}

/// DNS configuration.
#[derive(Debug, Clone)]
pub struct DnsConfig {
    /// Whether to use auto (DHCP-provided) DNS.
    pub auto_dns: bool,
    /// Manual DNS servers (IPv4 or IPv6 strings).
    pub servers: Vec<String>,
    /// DNS over HTTPS endpoint (empty = disabled).
    pub doh_url: String,
    /// DNS search domains.
    pub search_domains: Vec<String>,
}

/// IPv4 configuration for an interface.
#[derive(Debug, Clone)]
pub struct Ipv4Config {
    /// Method (Auto/Manual/Disabled).
    pub method: IpMethod,
    /// IP address (dotted notation).
    pub address: String,
    /// Subnet mask.
    pub netmask: String,
    /// Default gateway.
    pub gateway: String,
}

/// IPv6 configuration for an interface.
#[derive(Debug, Clone)]
pub struct Ipv6Config {
    /// Method (Auto/Manual/Disabled).
    pub method: IpMethod,
    /// IP address.
    pub address: String,
    /// Prefix length.
    pub prefix_len: u8,
    /// Gateway.
    pub gateway: String,
    /// Privacy extensions (temporary addresses).
    pub privacy_extensions: bool,
}

/// Network interface.
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    /// Interface name (e.g. "eth0", "wlan0").
    pub name: String,
    /// Display name (e.g. "Ethernet", "Wi-Fi").
    pub display_name: String,
    /// Interface type.
    pub iface_type: InterfaceType,
    /// Link state.
    pub link_state: LinkState,
    /// MAC address (hex string).
    pub mac_address: String,
    /// MTU.
    pub mtu: u32,
    /// IPv4 configuration.
    pub ipv4: Ipv4Config,
    /// IPv6 configuration.
    pub ipv6: Ipv6Config,
    /// DNS configuration.
    pub dns: DnsConfig,
    /// Connected SSID (Wi-Fi only).
    pub connected_ssid: String,
    /// Speed in Mbps.
    pub speed_mbps: u32,
    /// Bytes received.
    pub rx_bytes: u64,
    /// Bytes transmitted.
    pub tx_bytes: u64,
}

/// Router / gateway information.
#[derive(Debug, Clone)]
pub struct RouterInfo {
    /// Gateway IP address.
    pub gateway_ip: String,
    /// Whether gateway is reachable.
    pub reachable: bool,
    /// Router model (if detected via UPnP).
    pub model: String,
    /// External / WAN IP address (IPv4).
    pub external_ipv4: String,
    /// External / WAN IP address (IPv6).
    pub external_ipv6: String,
}

/// Saved Wi-Fi network.
#[derive(Debug, Clone)]
pub struct SavedNetwork {
    /// SSID.
    pub ssid: String,
    /// Security type.
    pub security: WifiSecurity,
    /// Password (stored, not shown by default).
    pub password: String,
    /// Whether to auto-connect.
    pub auto_connect: bool,
    /// Last connected timestamp (ns).
    pub last_connected_ns: u64,
    /// Priority (higher = preferred).
    pub priority: u32,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_INTERFACES: usize = 32;
const MAX_SCANNED: usize = 64;
const MAX_SAVED: usize = 128;
const MAX_DNS_SERVERS: usize = 8;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    interfaces: Vec<NetworkInterface>,
    scanned_wifi: Vec<WifiNetwork>,
    saved_networks: Vec<SavedNetwork>,
    router: RouterInfo,
    hostname: String,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    interfaces: Vec::new(),
    scanned_wifi: Vec::new(),
    saved_networks: Vec::new(),
    router: RouterInfo {
        gateway_ip: String::new(),
        reachable: false,
        model: String::new(),
        external_ipv4: String::new(),
        external_ipv6: String::new(),
    },
    hostname: String::new(),
    changes: 0,
});

static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Interface management
// ---------------------------------------------------------------------------

/// List all interfaces.
pub fn list_interfaces() -> Vec<NetworkInterface> {
    STATE.lock().interfaces.clone()
}

/// Get a specific interface.
pub fn get_interface(name: &str) -> KernelResult<NetworkInterface> {
    STATE.lock().interfaces.iter().find(|i| i.name == name)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// Add an interface.
pub fn add_interface(
    name: &str,
    display_name: &str,
    iface_type: InterfaceType,
    mac: &str,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.interfaces.len() >= MAX_INTERFACES {
        return Err(KernelError::ResourceExhausted);
    }
    if state.interfaces.iter().any(|i| i.name == name) {
        return Err(KernelError::AlreadyExists);
    }
    state.interfaces.push(NetworkInterface {
        name: String::from(name),
        display_name: String::from(display_name),
        iface_type,
        link_state: LinkState::Down,
        mac_address: String::from(mac),
        mtu: 1500,
        ipv4: Ipv4Config {
            method: IpMethod::Auto,
            address: String::new(),
            netmask: String::from("255.255.255.0"),
            gateway: String::new(),
        },
        ipv6: Ipv6Config {
            method: IpMethod::Auto,
            address: String::new(),
            prefix_len: 64,
            gateway: String::new(),
            privacy_extensions: true,
        },
        dns: DnsConfig {
            auto_dns: true,
            servers: Vec::new(),
            doh_url: String::new(),
            search_domains: Vec::new(),
        },
        connected_ssid: String::new(),
        speed_mbps: 0,
        rx_bytes: 0,
        tx_bytes: 0,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Remove an interface.
pub fn remove_interface(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state.interfaces.iter().position(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    state.interfaces.remove(idx);
    state.changes += 1;
    Ok(())
}

/// Set link state.
pub fn set_link_state(name: &str, link: LinkState) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    iface.link_state = link;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// IPv4 / IPv6 configuration
// ---------------------------------------------------------------------------

/// Set IPv4 configuration.
pub fn set_ipv4(
    name: &str,
    method: IpMethod,
    address: &str,
    netmask: &str,
    gateway: &str,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    iface.ipv4 = Ipv4Config {
        method,
        address: String::from(address),
        netmask: String::from(netmask),
        gateway: String::from(gateway),
    };
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set IPv6 configuration.
pub fn set_ipv6(
    name: &str,
    method: IpMethod,
    address: &str,
    prefix_len: u8,
    gateway: &str,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    iface.ipv6 = Ipv6Config {
        method,
        address: String::from(address),
        prefix_len,
        gateway: String::from(gateway),
        privacy_extensions: iface.ipv6.privacy_extensions,
    };
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set IPv6 privacy extensions.
pub fn set_ipv6_privacy(name: &str, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    iface.ipv6.privacy_extensions = enabled;
    state.changes += 1;
    Ok(())
}

/// Set MTU.
pub fn set_mtu(name: &str, mtu: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    if mtu < 68 || mtu > 9000 {
        return Err(KernelError::InvalidArgument);
    }
    iface.mtu = mtu;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// DNS
// ---------------------------------------------------------------------------

/// Set DNS configuration for an interface.
pub fn set_dns(name: &str, auto_dns: bool, servers: &[&str]) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    if servers.len() > MAX_DNS_SERVERS {
        return Err(KernelError::ResourceExhausted);
    }
    iface.dns.auto_dns = auto_dns;
    iface.dns.servers = servers.iter().map(|s| String::from(*s)).collect();
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set DNS-over-HTTPS URL.
pub fn set_doh(name: &str, url: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    iface.dns.doh_url = String::from(url);
    state.changes += 1;
    Ok(())
}

/// Add a DNS search domain.
pub fn add_search_domain(name: &str, domain: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut().find(|i| i.name == name)
        .ok_or(KernelError::NotFound)?;
    if !iface.dns.search_domains.iter().any(|d| d == domain) {
        iface.dns.search_domains.push(String::from(domain));
    }
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Wi-Fi
// ---------------------------------------------------------------------------

/// Simulate a Wi-Fi scan.
pub fn wifi_scan() -> Vec<WifiNetwork> {
    let state = STATE.lock();
    state.scanned_wifi.clone()
}

/// Add a scanned Wi-Fi result (for simulation / testing).
pub fn add_scanned_wifi(
    ssid: &str,
    signal: u32,
    security: WifiSecurity,
    band: WifiBand,
    channel: u32,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.scanned_wifi.len() >= MAX_SCANNED {
        return Err(KernelError::ResourceExhausted);
    }
    let saved = state.saved_networks.iter().any(|s| s.ssid == ssid);
    state.scanned_wifi.push(WifiNetwork {
        ssid: String::from(ssid),
        signal: signal.min(100),
        security,
        band,
        channel,
        saved,
    });
    state.changes += 1;
    Ok(())
}

/// Connect to a Wi-Fi network.
pub fn connect_wifi(iface_name: &str, ssid: &str, password: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut()
        .find(|i| i.name == iface_name && i.iface_type == InterfaceType::Wifi)
        .ok_or(KernelError::NotFound)?;
    iface.connected_ssid = String::from(ssid);
    iface.link_state = LinkState::Up;
    // Save network if not already saved.
    if !state.saved_networks.iter().any(|s| s.ssid == ssid) {
        if state.saved_networks.len() < MAX_SAVED {
            state.saved_networks.push(SavedNetwork {
                ssid: String::from(ssid),
                security: WifiSecurity::Wpa2Personal,
                password: String::from(password),
                auto_connect: true,
                last_connected_ns: crate::hpet::elapsed_ns(),
                priority: 0,
            });
        }
    }
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Disconnect Wi-Fi.
pub fn disconnect_wifi(iface_name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let iface = state.interfaces.iter_mut()
        .find(|i| i.name == iface_name && i.iface_type == InterfaceType::Wifi)
        .ok_or(KernelError::NotFound)?;
    iface.connected_ssid = String::new();
    iface.link_state = LinkState::Down;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// List saved networks.
pub fn saved_networks() -> Vec<SavedNetwork> {
    STATE.lock().saved_networks.clone()
}

/// Forget a saved network.
pub fn forget_network(ssid: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state.saved_networks.iter().position(|s| s.ssid == ssid)
        .ok_or(KernelError::NotFound)?;
    state.saved_networks.remove(idx);
    state.changes += 1;
    Ok(())
}

/// Set auto-connect for saved network.
pub fn set_auto_connect(ssid: &str, auto_connect: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let net = state.saved_networks.iter_mut().find(|s| s.ssid == ssid)
        .ok_or(KernelError::NotFound)?;
    net.auto_connect = auto_connect;
    state.changes += 1;
    Ok(())
}

/// Set priority for saved network.
pub fn set_network_priority(ssid: &str, priority: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let net = state.saved_networks.iter_mut().find(|s| s.ssid == ssid)
        .ok_or(KernelError::NotFound)?;
    net.priority = priority;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Router / external IP
// ---------------------------------------------------------------------------

/// Set router information.
pub fn set_router_info(gateway_ip: &str, reachable: bool, model: &str) {
    let mut state = STATE.lock();
    state.router.gateway_ip = String::from(gateway_ip);
    state.router.reachable = reachable;
    state.router.model = String::from(model);
    state.changes += 1;
}

/// Set external IP addresses.
pub fn set_external_ips(ipv4: &str, ipv6: &str) {
    let mut state = STATE.lock();
    state.router.external_ipv4 = String::from(ipv4);
    state.router.external_ipv6 = String::from(ipv6);
    state.changes += 1;
}

/// Get router / gateway info.
pub fn router_info() -> RouterInfo {
    STATE.lock().router.clone()
}

/// Set hostname.
pub fn set_hostname(name: &str) {
    let mut state = STATE.lock();
    state.hostname = String::from(name);
    state.changes += 1;
}

/// Get hostname.
pub fn hostname() -> String {
    STATE.lock().hostname.clone()
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with deterministic, non-fabricated defaults.
///
/// Seeds ONLY facts that are true by construction rather than observed:
///   - the loopback interface `lo` — 127.0.0.1 / ::1 are RFC constants that
///     exist on every networked system, not measured hardware state, and
///   - the default hostname (a configuration default).
///
/// It deliberately does NOT seed eth0/wlan0 with assigned IP addresses, MAC
/// addresses, link-up state, link speeds, or DNS servers; nor a reachable
/// gateway; nor Wi-Fi scan results. Those are all *observed* facts about real
/// hardware and the surrounding network. Inventing them (the previous
/// "192.168.1.100 eth0 up", a reachable "192.168.1.1" router, and phantom
/// "HomeNetwork"/"NeighborWifi" scan entries) surfaced fabricated interfaces,
/// a fake router, and phantom Wi-Fi networks — a privacy surface — through
/// `/proc/netsettings` and the `netsettings` shell command as if they had
/// really been detected. Real interfaces arrive via add_interface() from
/// driver enumeration; scan results via wifi_scan(); the gateway via probing.
///
/// DEFERRED PROPER FIX: wire add_interface()/set_link_state()/set_router_info()
/// to the real network driver + DHCP client so this panel reflects genuine NICs.
pub fn init_defaults() {
    let mut state = STATE.lock();

    state.interfaces = vec![
        NetworkInterface {
            name: String::from("lo"),
            display_name: String::from("Loopback"),
            iface_type: InterfaceType::Loopback,
            link_state: LinkState::Up,
            mac_address: String::from("00:00:00:00:00:00"),
            mtu: 65535,
            ipv4: Ipv4Config {
                method: IpMethod::Manual,
                address: String::from("127.0.0.1"),
                netmask: String::from("255.0.0.0"),
                gateway: String::new(),
            },
            ipv6: Ipv6Config {
                method: IpMethod::Manual,
                address: String::from("::1"),
                prefix_len: 128,
                gateway: String::new(),
                privacy_extensions: false,
            },
            dns: DnsConfig {
                auto_dns: true,
                servers: Vec::new(),
                doh_url: String::new(),
                search_domains: Vec::new(),
            },
            connected_ssid: String::new(),
            speed_mbps: 0,
            rx_bytes: 0,
            tx_bytes: 0,
        },
    ];

    // No detected gateway until real probing populates it.
    state.router = RouterInfo {
        gateway_ip: String::new(),
        reachable: false,
        model: String::new(),
        external_ipv4: String::new(),
        external_ipv6: String::new(),
    };

    // Hostname is a configuration default, not observed data.
    state.hostname = String::from("mintos");

    // No phantom Wi-Fi scan results: scans come from wifi_scan() / real radios.
    state.scanned_wifi.clear();
    state.saved_networks.clear();
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Return (iface_count, connected_count, saved_count, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let connected = state.interfaces.iter()
        .filter(|i| i.link_state == LinkState::Up && i.iface_type != InterfaceType::Loopback)
        .count();
    (state.interfaces.len(),
     connected,
     state.saved_networks.len(),
     OP_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.interfaces.clear();
    state.scanned_wifi.clear();
    state.saved_networks.clear();
    state.router = RouterInfo {
        gateway_ip: String::new(),
        reachable: false,
        model: String::new(),
        external_ipv4: String::new(),
        external_ipv6: String::new(),
    };
    state.hostname = String::new();
    state.changes = 0;
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: add interfaces.
    serial_println!("netsettings::self_test 1: add interfaces");
    add_interface("eth0", "Ethernet", InterfaceType::Ethernet, "52:54:00:12:34:56")?;
    add_interface("wlan0", "Wi-Fi", InterfaceType::Wifi, "52:54:00:ab:cd:ef")?;
    assert_eq!(list_interfaces().len(), 2);

    // Test 2: duplicate.
    serial_println!("netsettings::self_test 2: duplicate");
    assert!(add_interface("eth0", "Dup", InterfaceType::Ethernet, "00:00:00:00:00:00").is_err());

    // Test 3: set IPv4.
    serial_println!("netsettings::self_test 3: IPv4");
    set_ipv4("eth0", IpMethod::Manual, "10.0.0.5", "255.255.255.0", "10.0.0.1")?;
    let iface = get_interface("eth0")?;
    assert_eq!(iface.ipv4.method, IpMethod::Manual);
    assert_eq!(iface.ipv4.address, "10.0.0.5");

    // Test 4: DNS.
    serial_println!("netsettings::self_test 4: DNS");
    set_dns("eth0", false, &["1.1.1.1", "8.8.8.8"])?;
    let iface = get_interface("eth0")?;
    assert!(!iface.dns.auto_dns);
    assert_eq!(iface.dns.servers.len(), 2);

    // Test 5: Wi-Fi scan and connect.
    serial_println!("netsettings::self_test 5: Wi-Fi");
    add_scanned_wifi("TestNet", 75, WifiSecurity::Wpa2Personal, WifiBand::Band5g, 44)?;
    let scan = wifi_scan();
    assert_eq!(scan.len(), 1);
    connect_wifi("wlan0", "TestNet", "password123")?;
    let iface = get_interface("wlan0")?;
    assert_eq!(iface.connected_ssid, "TestNet");
    assert_eq!(iface.link_state, LinkState::Up);
    let saved = saved_networks();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].ssid, "TestNet");

    // Test 6: disconnect.
    serial_println!("netsettings::self_test 6: disconnect");
    disconnect_wifi("wlan0")?;
    let iface = get_interface("wlan0")?;
    assert_eq!(iface.link_state, LinkState::Down);

    // Test 7: forget network.
    serial_println!("netsettings::self_test 7: forget");
    forget_network("TestNet")?;
    assert!(saved_networks().is_empty());

    // Test 8: router info.
    serial_println!("netsettings::self_test 8: router");
    set_router_info("192.168.1.1", true, "Generic Router");
    set_external_ips("203.0.113.42", "2001:db8::1");
    let ri = router_info();
    assert!(ri.reachable);
    assert_eq!(ri.external_ipv4, "203.0.113.42");

    // Test 9: MTU.
    serial_println!("netsettings::self_test 9: MTU");
    set_mtu("eth0", 9000)?;
    assert_eq!(get_interface("eth0")?.mtu, 9000);
    assert!(set_mtu("eth0", 50).is_err()); // Too small.

    // Test 10: hostname.
    serial_println!("netsettings::self_test 10: hostname");
    set_hostname("myhost");
    assert_eq!(hostname(), "myhost");

    // Test 11: init_defaults seeds only honest, non-fabricated defaults —
    // just the loopback interface and a hostname, with no phantom eth0/wlan0,
    // no reachable gateway, and no invented Wi-Fi scan results.
    serial_println!("netsettings::self_test 11: defaults");
    init_defaults();
    let ifaces = list_interfaces();
    assert_eq!(ifaces.len(), 1);
    assert_eq!(ifaces[0].name, "lo");
    assert_eq!(ifaces[0].iface_type, InterfaceType::Loopback);
    assert!(!router_info().reachable);
    assert!(wifi_scan().is_empty());
    assert_eq!(hostname(), "mintos");

    clear_all();
    serial_println!("netsettings::self_test: all 11 tests passed");
    Ok(())
}

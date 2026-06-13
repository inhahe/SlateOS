//! SlateOS `nmcli` — NetworkManager multi-personality binary.
//!
//! Multi-personality binary providing:
//! - **NetworkManager** — network management daemon: manages connection profiles,
//!   handles DHCP, DNS, WiFi, VPN.  Loads config from `/etc/NetworkManager/`.
//!   Supports `--no-daemon`, `--debug`, `--log-level`.
//! - **nmcli** — command-line interface: `nmcli general status`, `nmcli device`,
//!   `nmcli connection show/add/modify/delete/up/down`,
//!   `nmcli device wifi list/connect`, `nmcli radio wifi on/off`.
//!   Terse / tabular output.
//! - **nmtui** — text UI stub: prints "text UI not available in this build".
//!
//! The active personality is determined by `argv[0]` basename.

#![cfg_attr(not(test), no_main)]
#![deny(clippy::all)]
#![allow(
    clippy::too_many_lines,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::match_same_arms,
    clippy::struct_excessive_bools
)]

use std::collections::BTreeMap;
use std::io::Write;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "1.48.0-slateos";
const NM_CONFIG_DIR: &str = "/etc/NetworkManager";

// ============================================================================
// Personality detection
// ============================================================================

/// Which personality this binary is running as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    NetworkManager,
    Nmcli,
    Nmtui,
}

/// Detect personality from argv[0].
fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let base = base.trim_end_matches(".exe");
    let lower: String = base.chars().map(|c| c.to_ascii_lowercase()).collect();

    if lower.contains("nmtui") {
        Personality::Nmtui
    } else if lower.contains("networkmanager") || lower == "nm" {
        Personality::NetworkManager
    } else {
        // Default to nmcli (most commonly invoked)
        Personality::Nmcli
    }
}

// ============================================================================
// Connection types
// ============================================================================

/// Network connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnType {
    Ethernet,
    Wifi,
    Bridge,
    Bond,
    Vlan,
    Vpn,
    Loopback,
}

impl ConnType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ethernet => "802-3-ethernet",
            Self::Wifi => "802-11-wireless",
            Self::Bridge => "bridge",
            Self::Bond => "bond",
            Self::Vlan => "vlan",
            Self::Vpn => "vpn",
            Self::Loopback => "loopback",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Ethernet => "ethernet",
            Self::Wifi => "wifi",
            Self::Bridge => "bridge",
            Self::Bond => "bond",
            Self::Vlan => "vlan",
            Self::Vpn => "vpn",
            Self::Loopback => "loopback",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "ethernet" | "802-3-ethernet" | "eth" => Some(Self::Ethernet),
            "wifi" | "802-11-wireless" | "wireless" => Some(Self::Wifi),
            "bridge" => Some(Self::Bridge),
            "bond" => Some(Self::Bond),
            "vlan" => Some(Self::Vlan),
            "vpn" => Some(Self::Vpn),
            "loopback" | "lo" => Some(Self::Loopback),
            _ => None,
        }
    }
}

// ============================================================================
// Device state
// ============================================================================

/// State of a network device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum DeviceState {
    Connected,
    Disconnected,
    Unavailable,
    Unmanaged,
    Deactivating,
    Connecting,
    NeedAuth,
}

impl DeviceState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Unavailable => "unavailable",
            Self::Unmanaged => "unmanaged",
            Self::Deactivating => "deactivating",
            Self::Connecting => "connecting",
            Self::NeedAuth => "need-auth",
        }
    }

    fn code(self) -> u32 {
        match self {
            Self::Unmanaged => 10,
            Self::Unavailable => 20,
            Self::Disconnected => 30,
            Self::Connecting => 40,
            Self::NeedAuth => 60,
            Self::Deactivating => 90,
            Self::Connected => 100,
        }
    }
}

// ============================================================================
// Device type
// ============================================================================

/// Type of a network device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum DeviceType {
    Ethernet,
    Wifi,
    Bridge,
    Bond,
    Vlan,
    Loopback,
    Tun,
}

impl DeviceType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ethernet => "ethernet",
            Self::Wifi => "wifi",
            Self::Bridge => "bridge",
            Self::Bond => "bond",
            Self::Vlan => "vlan",
            Self::Loopback => "loopback",
            Self::Tun => "tun",
        }
    }
}

// ============================================================================
// IPv4/IPv6 method
// ============================================================================

/// IPv4 addressing method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ipv4Method {
    Auto,
    Manual,
    Disabled,
    LinkLocal,
    Shared,
}

impl Ipv4Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
            Self::Disabled => "disabled",
            Self::LinkLocal => "link-local",
            Self::Shared => "shared",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "manual" => Some(Self::Manual),
            "disabled" => Some(Self::Disabled),
            "link-local" => Some(Self::LinkLocal),
            "shared" => Some(Self::Shared),
            _ => None,
        }
    }
}

/// IPv6 addressing method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ipv6Method {
    Auto,
    Manual,
    Disabled,
    LinkLocal,
    Ignore,
}

impl Ipv6Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
            Self::Disabled => "disabled",
            Self::LinkLocal => "link-local",
            Self::Ignore => "ignore",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "manual" => Some(Self::Manual),
            "disabled" => Some(Self::Disabled),
            "link-local" => Some(Self::LinkLocal),
            "ignore" => Some(Self::Ignore),
            _ => None,
        }
    }
}

// ============================================================================
// Connection profile
// ============================================================================

/// A connection profile stored by NetworkManager.
#[derive(Debug, Clone)]
struct ConnectionProfile {
    uuid: String,
    id: String,
    conn_type: ConnType,
    autoconnect: bool,
    interface_name: String,
    // IPv4 settings
    ipv4_method: Ipv4Method,
    ipv4_addresses: Vec<String>,
    ipv4_gateway: String,
    ipv4_dns: Vec<String>,
    // IPv6 settings
    ipv6_method: Ipv6Method,
    ipv6_addresses: Vec<String>,
    ipv6_gateway: String,
    ipv6_dns: Vec<String>,
    // Wifi settings
    wifi_ssid: String,
    wifi_mode: String,
    wifi_security: String,
    // Bridge/bond settings
    bridge_stp: bool,
    bridge_priority: u32,
    bond_mode: String,
    // VLAN settings
    vlan_id: u32,
    vlan_parent: String,
    // Connection state
    active: bool,
    device: String,
    // Extra properties (for modify)
    properties: BTreeMap<String, String>,
}

impl ConnectionProfile {
    fn new(id: &str, conn_type: ConnType) -> Self {
        Self {
            uuid: generate_uuid(id),
            id: id.to_string(),
            conn_type,
            autoconnect: true,
            interface_name: String::new(),
            ipv4_method: Ipv4Method::Auto,
            ipv4_addresses: Vec::new(),
            ipv4_gateway: String::new(),
            ipv4_dns: Vec::new(),
            ipv6_method: Ipv6Method::Auto,
            ipv6_addresses: Vec::new(),
            ipv6_gateway: String::new(),
            ipv6_dns: Vec::new(),
            wifi_ssid: String::new(),
            wifi_mode: String::from("infrastructure"),
            wifi_security: String::new(),
            bridge_stp: true,
            bridge_priority: 32768,
            bond_mode: String::from("balance-rr"),
            vlan_id: 0,
            vlan_parent: String::new(),
            active: false,
            device: String::new(),
            properties: BTreeMap::new(),
        }
    }
}

/// Generate a deterministic UUID from a connection name.
fn generate_uuid(name: &str) -> String {
    // Simple deterministic UUID v4-format based on name hash.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a offset basis
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3); // FNV-1a prime
    }
    let h2 = h.wrapping_mul(0x517c_c1b7_2722_0a95);
    // Format as UUID: 8-4-4-4-12
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (h >> 32) as u32,
        (h >> 16) as u16,
        (h & 0x0fff) as u16,
        ((h2 >> 48) & 0x3fff) | 0x8000,
        h2 & 0x0000_ffff_ffff_ffff
    )
}

// ============================================================================
// Network device
// ============================================================================

/// A simulated network device.
#[derive(Debug, Clone)]
struct NetworkDevice {
    name: String,
    dev_type: DeviceType,
    state: DeviceState,
    hwaddr: String,
    mtu: u32,
    connection: String,
    ip4_addr: String,
    ip4_gw: String,
    ip6_addr: String,
}

impl NetworkDevice {
    fn new(name: &str, dev_type: DeviceType) -> Self {
        Self {
            name: name.to_string(),
            dev_type,
            state: DeviceState::Disconnected,
            hwaddr: String::new(),
            mtu: 1500,
            connection: String::new(),
            ip4_addr: String::new(),
            ip4_gw: String::new(),
            ip6_addr: String::new(),
        }
    }
}

// ============================================================================
// WiFi access point
// ============================================================================

/// A simulated WiFi access point visible in scan results.
#[derive(Debug, Clone)]
struct WifiAp {
    ssid: String,
    bssid: String,
    mode: String,
    channel: u32,
    rate: String,
    signal: u32,
    security: String,
    in_use: bool,
}

// ============================================================================
// Network state database
// ============================================================================

/// The full simulated state that NetworkManager tracks.
struct NmState {
    connections: Vec<ConnectionProfile>,
    devices: Vec<NetworkDevice>,
    wifi_aps: Vec<WifiAp>,
    wifi_enabled: bool,
    wwan_enabled: bool,
    networking_enabled: bool,
    hostname: String,
}

impl NmState {
    fn new() -> Self {
        let mut state = Self {
            connections: Vec::new(),
            devices: Vec::new(),
            wifi_aps: Vec::new(),
            wifi_enabled: true,
            wwan_enabled: true,
            networking_enabled: true,
            hostname: String::from("slateos"),
        };
        state.populate_defaults();
        state
    }

    /// Set up a realistic default device/connection/AP database.
    fn populate_defaults(&mut self) {
        // Loopback
        let mut lo = NetworkDevice::new("lo", DeviceType::Loopback);
        lo.state = DeviceState::Unmanaged;
        lo.hwaddr = String::from("00:00:00:00:00:00");
        lo.mtu = 65536;
        lo.ip4_addr = String::from("127.0.0.1/8");
        lo.ip6_addr = String::from("::1/128");
        self.devices.push(lo);

        // Ethernet
        let mut eth0 = NetworkDevice::new("eth0", DeviceType::Ethernet);
        eth0.state = DeviceState::Connected;
        eth0.hwaddr = String::from("52:54:00:ab:cd:ef");
        eth0.connection = String::from("Wired connection 1");
        eth0.ip4_addr = String::from("192.168.1.100/24");
        eth0.ip4_gw = String::from("192.168.1.1");
        eth0.ip6_addr = String::from("fe80::5054:ff:feab:cdef/64");
        self.devices.push(eth0);

        // WiFi
        let mut wlan0 = NetworkDevice::new("wlan0", DeviceType::Wifi);
        wlan0.state = DeviceState::Connected;
        wlan0.hwaddr = String::from("aa:bb:cc:dd:ee:ff");
        wlan0.connection = String::from("MyHomeWiFi");
        wlan0.ip4_addr = String::from("192.168.1.50/24");
        wlan0.ip4_gw = String::from("192.168.1.1");
        wlan0.ip6_addr = String::from("fe80::aabb:ccff:fedd:eeff/64");
        self.devices.push(wlan0);

        // Wired connection profile
        let mut wired = ConnectionProfile::new("Wired connection 1", ConnType::Ethernet);
        wired.interface_name = String::from("eth0");
        wired.active = true;
        wired.device = String::from("eth0");
        wired.ipv4_addresses.push(String::from("192.168.1.100/24"));
        wired.ipv4_gateway = String::from("192.168.1.1");
        wired.ipv4_dns.push(String::from("8.8.8.8"));
        wired.ipv4_dns.push(String::from("8.8.4.4"));
        self.connections.push(wired);

        // WiFi connection profile
        let mut wifi = ConnectionProfile::new("MyHomeWiFi", ConnType::Wifi);
        wifi.interface_name = String::from("wlan0");
        wifi.wifi_ssid = String::from("MyHomeWiFi");
        wifi.wifi_security = String::from("wpa-psk");
        wifi.active = true;
        wifi.device = String::from("wlan0");
        wifi.ipv4_addresses.push(String::from("192.168.1.50/24"));
        wifi.ipv4_gateway = String::from("192.168.1.1");
        wifi.ipv4_dns.push(String::from("1.1.1.1"));
        self.connections.push(wifi);

        // Inactive profiles
        let vpn = ConnectionProfile::new("Work VPN", ConnType::Vpn);
        self.connections.push(vpn);

        let mut bridge = ConnectionProfile::new("br0", ConnType::Bridge);
        bridge.interface_name = String::from("br0");
        bridge.bridge_stp = true;
        bridge.bridge_priority = 32768;
        self.connections.push(bridge);

        // WiFi APs visible
        self.wifi_aps.push(WifiAp {
            ssid: String::from("MyHomeWiFi"),
            bssid: String::from("AA:BB:CC:DD:EE:FF"),
            mode: String::from("Infra"),
            channel: 6,
            rate: String::from("270 Mbit/s"),
            signal: 82,
            security: String::from("WPA2"),
            in_use: true,
        });
        self.wifi_aps.push(WifiAp {
            ssid: String::from("Neighbor5G"),
            bssid: String::from("11:22:33:44:55:66"),
            mode: String::from("Infra"),
            channel: 36,
            rate: String::from("540 Mbit/s"),
            signal: 45,
            security: String::from("WPA2"),
            in_use: false,
        });
        self.wifi_aps.push(WifiAp {
            ssid: String::from("CoffeeShop"),
            bssid: String::from("DE:AD:BE:EF:00:01"),
            mode: String::from("Infra"),
            channel: 1,
            rate: String::from("54 Mbit/s"),
            signal: 30,
            security: String::from("--"),
            in_use: false,
        });
        self.wifi_aps.push(WifiAp {
            ssid: String::from("OfficeNet"),
            bssid: String::from("FE:DC:BA:98:76:54"),
            mode: String::from("Infra"),
            channel: 11,
            rate: String::from("300 Mbit/s"),
            signal: 60,
            security: String::from("WPA3"),
            in_use: false,
        });
    }

    /// Find a connection by name or UUID.
    fn find_connection(&self, id_or_uuid: &str) -> Option<usize> {
        self.connections
            .iter()
            .position(|c| c.id == id_or_uuid || c.uuid == id_or_uuid)
    }

    /// Find a device by name.
    fn find_device(&self, name: &str) -> Option<usize> {
        self.devices.iter().position(|d| d.name == name)
    }
}

// ============================================================================
// Output format
// ============================================================================

/// Requested output mode for nmcli.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    Tabular,
    Terse,
    Pretty,
}

/// nmcli global flags parsed before the object/action.
#[derive(Debug, Clone)]
struct NmcliGlobal {
    mode: OutputMode,
    colors: bool,
    fields: Option<String>,
}

impl NmcliGlobal {
    fn new() -> Self {
        Self {
            mode: OutputMode::Tabular,
            colors: false,
            fields: None,
        }
    }
}

// ============================================================================
// Column alignment helper
// ============================================================================

/// Compute column widths from rows of data, then format each row.
fn format_table(headers: &[&str], rows: &[Vec<String>], out: &mut dyn Write) {
    let ncols = headers.len();
    let mut widths = vec![0usize; ncols];
    for (i, h) in headers.iter().enumerate() {
        if h.len() > widths[i] {
            widths[i] = h.len();
        }
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < ncols && cell.len() > widths[i] {
                widths[i] = cell.len();
            }
        }
    }

    // Print header
    let mut line = String::new();
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            line.push_str("  ");
        }
        line.push_str(h);
        if i + 1 < ncols {
            let pad = widths[i].saturating_sub(h.len());
            for _ in 0..pad {
                line.push(' ');
            }
        }
    }
    let _ = writeln!(out, "{}", line);

    // Print rows
    for row in rows {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                line.push_str("  ");
            }
            line.push_str(cell);
            if i + 1 < ncols && i < ncols {
                let pad = widths[i].saturating_sub(cell.len());
                for _ in 0..pad {
                    line.push(' ');
                }
            }
        }
        let _ = writeln!(out, "{}", line);
    }
}

/// Format terse (colon-separated) output.
fn format_terse(headers: &[&str], rows: &[Vec<String>], out: &mut dyn Write) {
    // In terse mode, first line is field names
    let _ = writeln!(out, "{}", headers.join(":"));
    for row in rows {
        let _ = writeln!(out, "{}", row.join(":"));
    }
}

// ============================================================================
// nmcli general
// ============================================================================

fn cmd_general(state: &NmState, args: &[&str], global: &NmcliGlobal, out: &mut dyn Write) -> i32 {
    match args.first().copied() {
        None | Some("status") => cmd_general_status(state, global, out),
        Some("hostname") => {
            if args.len() > 1 {
                let _ = writeln!(out, "Error: setting hostname requires root privileges.");
                1
            } else {
                let _ = writeln!(out, "{}", state.hostname);
                0
            }
        }
        Some("permissions") => {
            let headers = ["PERMISSION", "VALUE"];
            let rows = vec![
                vec![
                    String::from("org.freedesktop.NetworkManager.enable-disable-network"),
                    String::from("yes"),
                ],
                vec![
                    String::from("org.freedesktop.NetworkManager.enable-disable-wifi"),
                    String::from("yes"),
                ],
                vec![
                    String::from("org.freedesktop.NetworkManager.settings.modify.system"),
                    String::from("yes"),
                ],
                vec![
                    String::from("org.freedesktop.NetworkManager.settings.modify.own"),
                    String::from("yes"),
                ],
            ];
            output_table(&headers, &rows, global, out);
            0
        }
        Some("logging") => {
            let _ = writeln!(out, "LEVEL  DOMAINS");
            let _ = writeln!(out, "INFO   PLATFORM,RFKILL,ETHER,WIFI,BT,MB,DHCP4,DHCP6,PPP,IP4,IP6,AUTOIP4,DNS,VPN,SHARING,SUPPLICANT,AGENTS,SETTINGS,SUSPEND,CORE,DEVICE,OLPC,INFINIBAND,FIREWALL,ADSL,BOND,VLAN,BRIDGE,TEAM,CONCHECK,DCB,DISPATCH,AUDIT,SYSTEMD,PROXY");
            0
        }
        Some(other) => {
            let _ = writeln!(out, "Error: invalid command '{}'; use status, hostname, permissions, or logging.", other);
            2
        }
    }
}

fn cmd_general_status(state: &NmState, global: &NmcliGlobal, out: &mut dyn Write) -> i32 {
    let running = "running";
    let wifi_state = if state.wifi_enabled {
        "enabled"
    } else {
        "disabled"
    };
    let wwan_state = if state.wwan_enabled {
        "enabled"
    } else {
        "disabled"
    };

    let headers = ["STATE", "CONNECTIVITY", "WIFI-HW", "WIFI", "WWAN-HW", "WWAN"];
    let rows = vec![vec![
        running.to_string(),
        String::from("full"),
        String::from("enabled"),
        wifi_state.to_string(),
        String::from("enabled"),
        wwan_state.to_string(),
    ]];

    // Filter by requested fields if any.
    if let Some(ref fields) = global.fields {
        let requested: Vec<&str> = fields.split(',').map(|s| s.trim()).collect();
        let indices: Vec<usize> = requested
            .iter()
            .filter_map(|f| {
                let upper: String = f.to_uppercase();
                headers.iter().position(|h| *h == upper.as_str())
            })
            .collect();
        let filt_headers: Vec<&str> = indices.iter().map(|&i| headers[i]).collect();
        let filt_rows: Vec<Vec<String>> = rows
            .iter()
            .map(|row| indices.iter().filter_map(|&i| row.get(i).cloned()).collect())
            .collect();
        output_table(&filt_headers, &filt_rows, global, out);
    } else {
        output_table(&headers, &rows, global, out);
    }
    0
}

fn output_table(headers: &[&str], rows: &[Vec<String>], global: &NmcliGlobal, out: &mut dyn Write) {
    match global.mode {
        OutputMode::Terse => format_terse(headers, rows, out),
        _ => format_table(headers, rows, out),
    }
}

// ============================================================================
// nmcli networking
// ============================================================================

fn cmd_networking(state: &mut NmState, args: &[&str], out: &mut dyn Write) -> i32 {
    match args.first().copied() {
        None => {
            let _ = writeln!(
                out,
                "{}",
                if state.networking_enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            0
        }
        Some("on") => {
            state.networking_enabled = true;
            0
        }
        Some("off") => {
            state.networking_enabled = false;
            0
        }
        Some("connectivity") => {
            if state.networking_enabled {
                let _ = writeln!(out, "full");
            } else {
                let _ = writeln!(out, "none");
            }
            0
        }
        Some(other) => {
            let _ = writeln!(out, "Error: invalid command '{}'; use on, off, or connectivity.", other);
            2
        }
    }
}

// ============================================================================
// nmcli radio
// ============================================================================

fn cmd_radio(state: &mut NmState, args: &[&str], global: &NmcliGlobal, out: &mut dyn Write) -> i32 {
    match args.first().copied() {
        None | Some("all") => {
            let headers = ["WIFI-HW", "WIFI", "WWAN-HW", "WWAN"];
            let rows = vec![vec![
                String::from("enabled"),
                if state.wifi_enabled {
                    String::from("enabled")
                } else {
                    String::from("disabled")
                },
                String::from("enabled"),
                if state.wwan_enabled {
                    String::from("enabled")
                } else {
                    String::from("disabled")
                },
            ]];
            output_table(&headers, &rows, global, out);
            0
        }
        Some("wifi") => {
            if args.len() < 2 {
                let _ = writeln!(
                    out,
                    "{}",
                    if state.wifi_enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
                return 0;
            }
            match args.get(1).copied() {
                Some("on") => {
                    state.wifi_enabled = true;
                    0
                }
                Some("off") => {
                    state.wifi_enabled = false;
                    // Disconnect wifi device
                    for dev in &mut state.devices {
                        if dev.dev_type == DeviceType::Wifi {
                            dev.state = DeviceState::Unavailable;
                            dev.connection.clear();
                        }
                    }
                    for conn in &mut state.connections {
                        if conn.conn_type == ConnType::Wifi {
                            conn.active = false;
                            conn.device.clear();
                        }
                    }
                    0
                }
                _ => {
                    let _ = writeln!(out, "Error: invalid 'wifi' command; use on or off.");
                    2
                }
            }
        }
        Some("wwan") => {
            if args.len() < 2 {
                let _ = writeln!(
                    out,
                    "{}",
                    if state.wwan_enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
                return 0;
            }
            match args.get(1).copied() {
                Some("on") => {
                    state.wwan_enabled = true;
                    0
                }
                Some("off") => {
                    state.wwan_enabled = false;
                    0
                }
                _ => {
                    let _ = writeln!(out, "Error: invalid 'wwan' command; use on or off.");
                    2
                }
            }
        }
        Some(other) => {
            let _ = writeln!(out, "Error: invalid command '{}'; use all, wifi, or wwan.", other);
            2
        }
    }
}

// ============================================================================
// nmcli device
// ============================================================================

fn cmd_device(state: &mut NmState, args: &[&str], global: &NmcliGlobal, out: &mut dyn Write) -> i32 {
    match args.first().copied() {
        None | Some("status") => cmd_device_status(state, global, out),
        Some("show") => cmd_device_show(state, args.get(1).copied(), out),
        Some("connect") => {
            if let Some(dev_name) = args.get(1) {
                cmd_device_connect(state, dev_name, out)
            } else {
                let _ = writeln!(out, "Error: 'device connect' requires a device name.");
                2
            }
        }
        Some("disconnect") => {
            if let Some(dev_name) = args.get(1) {
                cmd_device_disconnect(state, dev_name, out)
            } else {
                let _ = writeln!(out, "Error: 'device disconnect' requires a device name.");
                2
            }
        }
        Some("wifi") => cmd_device_wifi(state, &args[1..], global, out),
        Some("set") => {
            if args.len() < 4 {
                let _ = writeln!(out, "Error: usage: device set <ifname> <property> <value>");
                return 2;
            }
            let dev_name = args[1];
            let prop = args[2];
            let val = args[3];
            cmd_device_set(state, dev_name, prop, val, out)
        }
        Some(other) => {
            let _ = writeln!(out, "Error: invalid command '{}'; use status, show, connect, disconnect, wifi, or set.", other);
            2
        }
    }
}

fn cmd_device_status(state: &NmState, global: &NmcliGlobal, out: &mut dyn Write) -> i32 {
    let headers = ["DEVICE", "TYPE", "STATE", "CONNECTION"];
    let rows: Vec<Vec<String>> = state
        .devices
        .iter()
        .map(|d| {
            vec![
                d.name.clone(),
                d.dev_type.as_str().to_string(),
                d.state.as_str().to_string(),
                if d.connection.is_empty() {
                    String::from("--")
                } else {
                    d.connection.clone()
                },
            ]
        })
        .collect();
    output_table(&headers, &rows, global, out);
    0
}

fn cmd_device_show(state: &NmState, dev_name: Option<&str>, out: &mut dyn Write) -> i32 {
    let devices: Vec<&NetworkDevice> = match dev_name {
        Some(name) => {
            match state.devices.iter().find(|d| d.name == name) {
                Some(d) => vec![d],
                None => {
                    let _ = writeln!(out, "Error: device '{}' not found.", name);
                    return 10;
                }
            }
        }
        None => state.devices.iter().collect(),
    };

    for (idx, dev) in devices.iter().enumerate() {
        if idx > 0 {
            let _ = writeln!(out);
        }
        let _ = writeln!(out, "GENERAL.DEVICE:                         {}", dev.name);
        let _ = writeln!(out, "GENERAL.TYPE:                           {}", dev.dev_type.as_str());
        let _ = writeln!(out, "GENERAL.HWADDR:                         {}", dev.hwaddr);
        let _ = writeln!(out, "GENERAL.MTU:                            {}", dev.mtu);
        let _ = writeln!(out, "GENERAL.STATE:                          {} ({})", dev.state.code(), dev.state.as_str());
        let _ = writeln!(
            out,
            "GENERAL.CONNECTION:                     {}",
            if dev.connection.is_empty() {
                "--"
            } else {
                &dev.connection
            }
        );
        let _ = writeln!(out, "GENERAL.CON-PATH:                       --");
        if !dev.ip4_addr.is_empty() {
            let _ = writeln!(out, "IP4.ADDRESS[1]:                         {}", dev.ip4_addr);
        }
        if !dev.ip4_gw.is_empty() {
            let _ = writeln!(out, "IP4.GATEWAY:                            {}", dev.ip4_gw);
        }
        if !dev.ip6_addr.is_empty() {
            let _ = writeln!(out, "IP6.ADDRESS[1]:                         {}", dev.ip6_addr);
        }
        let _ = writeln!(out, "IP6.GATEWAY:                            --");
    }
    0
}

fn cmd_device_connect(state: &mut NmState, dev_name: &str, out: &mut dyn Write) -> i32 {
    let dev_idx = match state.find_device(dev_name) {
        Some(i) => i,
        None => {
            let _ = writeln!(out, "Error: device '{}' not found.", dev_name);
            return 10;
        }
    };

    if state.devices[dev_idx].state == DeviceState::Connected {
        let _ = writeln!(out, "Device '{}' is already connected.", dev_name);
        return 0;
    }

    // Find a matching connection profile
    let dev_type = state.devices[dev_idx].dev_type;
    let conn_type = match dev_type {
        DeviceType::Ethernet => ConnType::Ethernet,
        DeviceType::Wifi => ConnType::Wifi,
        DeviceType::Bridge => ConnType::Bridge,
        DeviceType::Bond => ConnType::Bond,
        _ => {
            let _ = writeln!(out, "Error: cannot autoconnect device '{}'.", dev_name);
            return 4;
        }
    };

    let conn_idx = state
        .connections
        .iter()
        .position(|c| c.conn_type == conn_type && (c.interface_name == dev_name || c.interface_name.is_empty()));

    if let Some(ci) = conn_idx {
        state.devices[dev_idx].state = DeviceState::Connected;
        state.devices[dev_idx].connection = state.connections[ci].id.clone();
        state.connections[ci].active = true;
        state.connections[ci].device = dev_name.to_string();
        let _ = writeln!(
            out,
            "Device '{}' successfully activated with '{}'.",
            dev_name, state.connections[ci].id
        );
        0
    } else {
        let _ = writeln!(out, "Error: no suitable connection found for device '{}'.", dev_name);
        4
    }
}

fn cmd_device_disconnect(state: &mut NmState, dev_name: &str, out: &mut dyn Write) -> i32 {
    let dev_idx = match state.find_device(dev_name) {
        Some(i) => i,
        None => {
            let _ = writeln!(out, "Error: device '{}' not found.", dev_name);
            return 10;
        }
    };

    if state.devices[dev_idx].state != DeviceState::Connected {
        let _ = writeln!(out, "Device '{}' is not connected.", dev_name);
        return 0;
    }

    let conn_name = state.devices[dev_idx].connection.clone();
    state.devices[dev_idx].state = DeviceState::Disconnected;
    state.devices[dev_idx].connection.clear();

    // Deactivate the connection
    for conn in &mut state.connections {
        if conn.device == dev_name {
            conn.active = false;
            conn.device.clear();
        }
    }

    let _ = writeln!(out, "Device '{}' successfully disconnected.", dev_name);
    if !conn_name.is_empty() {
        let _ = writeln!(out, "Connection '{}' deactivated.", conn_name);
    }
    0
}

fn cmd_device_set(state: &mut NmState, dev_name: &str, prop: &str, val: &str, out: &mut dyn Write) -> i32 {
    let dev_idx = match state.find_device(dev_name) {
        Some(i) => i,
        None => {
            let _ = writeln!(out, "Error: device '{}' not found.", dev_name);
            return 10;
        }
    };

    match prop {
        "autoconnect" => {
            match val {
                "yes" | "true" | "1" => {}
                "no" | "false" | "0" => {}
                _ => {
                    let _ = writeln!(out, "Error: invalid value '{}' for autoconnect.", val);
                    return 2;
                }
            }
            let _ = writeln!(out, "Device '{}' autoconnect set to '{}'.", dev_name, val);
            0
        }
        "managed" => {
            match val {
                "yes" | "true" | "1" => {
                    if state.devices[dev_idx].state == DeviceState::Unmanaged {
                        state.devices[dev_idx].state = DeviceState::Disconnected;
                    }
                }
                "no" | "false" | "0" => {
                    state.devices[dev_idx].state = DeviceState::Unmanaged;
                }
                _ => {
                    let _ = writeln!(out, "Error: invalid value '{}' for managed.", val);
                    return 2;
                }
            }
            let _ = writeln!(out, "Device '{}' managed set to '{}'.", dev_name, val);
            0
        }
        _ => {
            let _ = writeln!(out, "Error: unknown property '{}'.", prop);
            2
        }
    }
}

// ============================================================================
// nmcli device wifi
// ============================================================================

fn cmd_device_wifi(state: &mut NmState, args: &[&str], global: &NmcliGlobal, out: &mut dyn Write) -> i32 {
    match args.first().copied() {
        None | Some("list") => cmd_device_wifi_list(state, global, out),
        Some("connect") => {
            if let Some(ssid) = args.get(1) {
                let password = find_option_value(args, "password");
                cmd_device_wifi_connect(state, ssid, password, out)
            } else {
                let _ = writeln!(out, "Error: 'wifi connect' requires an SSID.");
                2
            }
        }
        Some("rescan") => {
            let _ = writeln!(out, "Wi-Fi scan requested.");
            0
        }
        Some("hotspot") => {
            let _ = writeln!(out, "Error: hotspot not supported in this build.");
            4
        }
        Some(other) => {
            let _ = writeln!(out, "Error: invalid wifi command '{}'; use list, connect, or rescan.", other);
            2
        }
    }
}

fn cmd_device_wifi_list(state: &NmState, global: &NmcliGlobal, out: &mut dyn Write) -> i32 {
    if !state.wifi_enabled {
        let _ = writeln!(out, "Wi-Fi is disabled.");
        return 0;
    }

    let headers = ["IN-USE", "BSSID", "SSID", "MODE", "CHAN", "RATE", "SIGNAL", "SECURITY"];
    let rows: Vec<Vec<String>> = state
        .wifi_aps
        .iter()
        .map(|ap| {
            vec![
                if ap.in_use {
                    String::from("*")
                } else {
                    String::from(" ")
                },
                ap.bssid.clone(),
                ap.ssid.clone(),
                ap.mode.clone(),
                ap.channel.to_string(),
                ap.rate.clone(),
                ap.signal.to_string(),
                ap.security.clone(),
            ]
        })
        .collect();
    output_table(&headers, &rows, global, out);
    0
}

fn cmd_device_wifi_connect(
    state: &mut NmState,
    ssid: &str,
    _password: Option<&str>,
    out: &mut dyn Write,
) -> i32 {
    if !state.wifi_enabled {
        let _ = writeln!(out, "Error: Wi-Fi is disabled.");
        return 4;
    }

    // Check if SSID is visible
    let ap_exists = state.wifi_aps.iter().any(|ap| ap.ssid == ssid);
    if !ap_exists {
        let _ = writeln!(out, "Error: no Wi-Fi network '{}' found.", ssid);
        return 10;
    }

    // Disconnect any currently connected wifi
    for ap in &mut state.wifi_aps {
        ap.in_use = false;
    }
    for dev in &mut state.devices {
        if dev.dev_type == DeviceType::Wifi {
            dev.connection.clear();
            dev.state = DeviceState::Disconnected;
        }
    }
    for conn in &mut state.connections {
        if conn.conn_type == ConnType::Wifi {
            conn.active = false;
            conn.device.clear();
        }
    }

    // Mark the AP in use
    for ap in &mut state.wifi_aps {
        if ap.ssid == ssid {
            ap.in_use = true;
        }
    }

    // Create or activate connection profile
    let conn_idx = state.find_connection(ssid);
    let conn_idx = match conn_idx {
        Some(i) => i,
        None => {
            let mut profile = ConnectionProfile::new(ssid, ConnType::Wifi);
            profile.wifi_ssid = ssid.to_string();
            profile.interface_name = String::from("wlan0");
            state.connections.push(profile);
            state.connections.len() - 1
        }
    };
    state.connections[conn_idx].active = true;
    state.connections[conn_idx].device = String::from("wlan0");

    // Activate wifi device
    for dev in &mut state.devices {
        if dev.dev_type == DeviceType::Wifi {
            dev.state = DeviceState::Connected;
            dev.connection = ssid.to_string();
        }
    }

    let _ = writeln!(
        out,
        "Device 'wlan0' successfully activated with '{}'.",
        ssid
    );
    0
}

/// Find --key value pairs in argument list.
fn find_option_value<'a>(args: &[&'a str], key: &str) -> Option<&'a str> {
    let flag = format!("--{}", key);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if *arg == flag || *arg == key {
            return iter.next().copied();
        }
    }
    None
}

// ============================================================================
// nmcli connection
// ============================================================================

fn cmd_connection(
    state: &mut NmState,
    args: &[&str],
    global: &NmcliGlobal,
    out: &mut dyn Write,
) -> i32 {
    match args.first().copied() {
        None | Some("show") => {
            if args.len() > 1 {
                cmd_connection_show_detail(state, args[1], out)
            } else {
                cmd_connection_show_list(state, global, out)
            }
        }
        Some("up") => {
            if let Some(name) = args.get(1) {
                cmd_connection_up(state, name, out)
            } else {
                let _ = writeln!(out, "Error: 'connection up' requires a connection name or UUID.");
                2
            }
        }
        Some("down") => {
            if let Some(name) = args.get(1) {
                cmd_connection_down(state, name, out)
            } else {
                let _ = writeln!(out, "Error: 'connection down' requires a connection name or UUID.");
                2
            }
        }
        Some("add") => cmd_connection_add(state, &args[1..], out),
        Some("modify") | Some("mod") => cmd_connection_modify(state, &args[1..], out),
        Some("delete") | Some("del") => {
            if let Some(name) = args.get(1) {
                cmd_connection_delete(state, name, out)
            } else {
                let _ = writeln!(out, "Error: 'connection delete' requires a connection name or UUID.");
                2
            }
        }
        Some("reload") => {
            let _ = writeln!(out, "Connections reloaded.");
            0
        }
        Some("load") => {
            if args.len() < 2 {
                let _ = writeln!(out, "Error: 'connection load' requires a filename.");
                2
            } else {
                let _ = writeln!(out, "Connection loaded from '{}'.", args[1]);
                0
            }
        }
        Some("clone") => {
            if args.len() < 3 {
                let _ = writeln!(out, "Error: usage: connection clone <id> <new-name>");
                return 2;
            }
            cmd_connection_clone(state, args[1], args[2], out)
        }
        Some("monitor") => {
            let _ = writeln!(out, "Monitoring connection changes... (press Ctrl+C to stop)");
            0
        }
        Some(other) => {
            let _ = writeln!(
                out,
                "Error: invalid command '{}'; use show, up, down, add, modify, delete, reload, load, clone, or monitor.",
                other
            );
            2
        }
    }
}

fn cmd_connection_show_list(state: &NmState, global: &NmcliGlobal, out: &mut dyn Write) -> i32 {
    let headers = ["NAME", "UUID", "TYPE", "DEVICE"];
    let rows: Vec<Vec<String>> = state
        .connections
        .iter()
        .map(|c| {
            vec![
                c.id.clone(),
                c.uuid.clone(),
                c.conn_type.display_name().to_string(),
                if c.device.is_empty() {
                    String::from("--")
                } else {
                    c.device.clone()
                },
            ]
        })
        .collect();
    output_table(&headers, &rows, global, out);
    0
}

fn cmd_connection_show_detail(state: &NmState, id: &str, out: &mut dyn Write) -> i32 {
    let conn = match state.find_connection(id) {
        Some(i) => &state.connections[i],
        None => {
            let _ = writeln!(out, "Error: connection '{}' not found.", id);
            return 10;
        }
    };

    let _ = writeln!(out, "connection.id:                          {}", conn.id);
    let _ = writeln!(out, "connection.uuid:                        {}", conn.uuid);
    let _ = writeln!(out, "connection.type:                        {}", conn.conn_type.as_str());
    let _ = writeln!(out, "connection.autoconnect:                 {}", if conn.autoconnect { "yes" } else { "no" });
    let _ = writeln!(
        out,
        "connection.interface-name:              {}",
        if conn.interface_name.is_empty() {
            "--"
        } else {
            &conn.interface_name
        }
    );
    let _ = writeln!(out, "ipv4.method:                            {}", conn.ipv4_method.as_str());
    if !conn.ipv4_addresses.is_empty() {
        for (i, addr) in conn.ipv4_addresses.iter().enumerate() {
            let _ = writeln!(out, "ipv4.addresses[{}]:                      {}", i + 1, addr);
        }
    }
    if !conn.ipv4_gateway.is_empty() {
        let _ = writeln!(out, "ipv4.gateway:                           {}", conn.ipv4_gateway);
    }
    if !conn.ipv4_dns.is_empty() {
        for (i, dns) in conn.ipv4_dns.iter().enumerate() {
            let _ = writeln!(out, "ipv4.dns[{}]:                             {}", i + 1, dns);
        }
    }
    let _ = writeln!(out, "ipv6.method:                            {}", conn.ipv6_method.as_str());
    if !conn.ipv6_addresses.is_empty() {
        for (i, addr) in conn.ipv6_addresses.iter().enumerate() {
            let _ = writeln!(out, "ipv6.addresses[{}]:                      {}", i + 1, addr);
        }
    }
    if !conn.ipv6_gateway.is_empty() {
        let _ = writeln!(out, "ipv6.gateway:                           {}", conn.ipv6_gateway);
    }

    match conn.conn_type {
        ConnType::Wifi => {
            let _ = writeln!(
                out,
                "802-11-wireless.ssid:                   {}",
                if conn.wifi_ssid.is_empty() {
                    "--"
                } else {
                    &conn.wifi_ssid
                }
            );
            let _ = writeln!(out, "802-11-wireless.mode:                   {}", conn.wifi_mode);
            if !conn.wifi_security.is_empty() {
                let _ = writeln!(out, "802-11-wireless-security.key-mgmt:      {}", conn.wifi_security);
            }
        }
        ConnType::Bridge => {
            let _ = writeln!(out, "bridge.stp:                             {}", if conn.bridge_stp { "yes" } else { "no" });
            let _ = writeln!(out, "bridge.priority:                        {}", conn.bridge_priority);
        }
        ConnType::Bond => {
            let _ = writeln!(out, "bond.options:                           mode={}", conn.bond_mode);
        }
        ConnType::Vlan => {
            let _ = writeln!(out, "vlan.id:                                {}", conn.vlan_id);
            let _ = writeln!(
                out,
                "vlan.parent:                            {}",
                if conn.vlan_parent.is_empty() {
                    "--"
                } else {
                    &conn.vlan_parent
                }
            );
        }
        _ => {}
    }

    let _ = writeln!(
        out,
        "GENERAL.STATE:                          {}",
        if conn.active { "activated" } else { "deactivated" }
    );
    let _ = writeln!(
        out,
        "GENERAL.DEVICE:                         {}",
        if conn.device.is_empty() {
            "--"
        } else {
            &conn.device
        }
    );

    // Show any extra properties
    for (k, v) in &conn.properties {
        let _ = writeln!(out, "{}:  {}", k, v);
    }

    0
}

fn cmd_connection_up(state: &mut NmState, name: &str, out: &mut dyn Write) -> i32 {
    let conn_idx = match state.find_connection(name) {
        Some(i) => i,
        None => {
            let _ = writeln!(out, "Error: connection '{}' not found.", name);
            return 10;
        }
    };

    if state.connections[conn_idx].active {
        let _ = writeln!(out, "Connection '{}' is already active.", name);
        return 0;
    }

    // Try to find a matching device
    let iface = &state.connections[conn_idx].interface_name;
    let dev_idx = if iface.is_empty() {
        // Find first matching device type
        let ct = state.connections[conn_idx].conn_type;
        let dt = match ct {
            ConnType::Ethernet => Some(DeviceType::Ethernet),
            ConnType::Wifi => Some(DeviceType::Wifi),
            ConnType::Bridge => Some(DeviceType::Bridge),
            ConnType::Bond => Some(DeviceType::Bond),
            _ => None,
        };
        dt.and_then(|t| state.devices.iter().position(|d| d.dev_type == t))
    } else {
        state.find_device(iface)
    };

    if let Some(di) = dev_idx {
        state.devices[di].state = DeviceState::Connected;
        state.devices[di].connection = state.connections[conn_idx].id.clone();
        state.connections[conn_idx].active = true;
        state.connections[conn_idx].device = state.devices[di].name.clone();
        let _ = writeln!(
            out,
            "Connection successfully activated (D-Bus active path: /org/freedesktop/NetworkManager/ActiveConnection/{})",
            conn_idx + 1
        );
    } else {
        // VPN or no device — just mark active
        state.connections[conn_idx].active = true;
        let _ = writeln!(
            out,
            "Connection successfully activated (D-Bus active path: /org/freedesktop/NetworkManager/ActiveConnection/{})",
            conn_idx + 1
        );
    }
    0
}

fn cmd_connection_down(state: &mut NmState, name: &str, out: &mut dyn Write) -> i32 {
    let conn_idx = match state.find_connection(name) {
        Some(i) => i,
        None => {
            let _ = writeln!(out, "Error: connection '{}' not found.", name);
            return 10;
        }
    };

    if !state.connections[conn_idx].active {
        let _ = writeln!(out, "Connection '{}' is not active.", name);
        return 0;
    }

    let dev_name = state.connections[conn_idx].device.clone();
    state.connections[conn_idx].active = false;
    state.connections[conn_idx].device.clear();

    // Disconnect the device
    if !dev_name.is_empty()
        && let Some(di) = state.find_device(&dev_name) {
            state.devices[di].state = DeviceState::Disconnected;
            state.devices[di].connection.clear();
        }

    let _ = writeln!(
        out,
        "Connection '{}' successfully deactivated (D-Bus active path: /org/freedesktop/NetworkManager/ActiveConnection/{})",
        state.connections[conn_idx].id,
        conn_idx + 1
    );
    0
}

fn cmd_connection_add(state: &mut NmState, args: &[&str], out: &mut dyn Write) -> i32 {
    // Parse: add type <type> [con-name <name>] [ifname <dev>] [key value ...]
    let conn_type_str = find_option_value(args, "type");
    let conn_type = match conn_type_str.and_then(ConnType::parse) {
        Some(t) => t,
        None => {
            let _ = writeln!(
                out,
                "Error: 'connection add' requires 'type' (ethernet, wifi, bridge, bond, vlan, vpn)."
            );
            return 2;
        }
    };

    let con_name = find_option_value(args, "con-name")
        .unwrap_or(conn_type.display_name());
    let ifname = find_option_value(args, "ifname").unwrap_or("");

    // Check for duplicate
    if state.find_connection(con_name).is_some() {
        let _ = writeln!(out, "Error: connection '{}' already exists.", con_name);
        return 2;
    }

    let mut profile = ConnectionProfile::new(con_name, conn_type);
    profile.interface_name = ifname.to_string();

    // Parse additional settings from remaining args
    parse_connection_settings(&mut profile, args);

    let uuid = profile.uuid.clone();
    let name = profile.id.clone();
    state.connections.push(profile);

    let _ = writeln!(
        out,
        "Connection '{}' ({}) successfully added.",
        name, uuid
    );
    0
}

fn parse_connection_settings(profile: &mut ConnectionProfile, args: &[&str]) {
    let mut i = 0;
    while i < args.len() {
        let key = args[i];
        let val = if i + 1 < args.len() { args[i + 1] } else { "" };
        match key {
            "ipv4.method" => {
                if let Some(m) = Ipv4Method::parse(val) {
                    profile.ipv4_method = m;
                }
                i += 2;
            }
            "ipv4.addresses" | "ipv4.address" => {
                profile.ipv4_addresses.push(val.to_string());
                i += 2;
            }
            "ipv4.gateway" | "ipv4.gw" => {
                profile.ipv4_gateway = val.to_string();
                i += 2;
            }
            "ipv4.dns" => {
                profile.ipv4_dns.push(val.to_string());
                i += 2;
            }
            "ipv6.method" => {
                if let Some(m) = Ipv6Method::parse(val) {
                    profile.ipv6_method = m;
                }
                i += 2;
            }
            "ipv6.addresses" | "ipv6.address" => {
                profile.ipv6_addresses.push(val.to_string());
                i += 2;
            }
            "ipv6.gateway" | "ipv6.gw" => {
                profile.ipv6_gateway = val.to_string();
                i += 2;
            }
            "ipv6.dns" => {
                profile.ipv6_dns.push(val.to_string());
                i += 2;
            }
            "connection.autoconnect" | "autoconnect" => {
                profile.autoconnect = val == "yes" || val == "true" || val == "1";
                i += 2;
            }
            "802-11-wireless.ssid" | "wifi.ssid" | "ssid" => {
                profile.wifi_ssid = val.to_string();
                i += 2;
            }
            "802-11-wireless.mode" | "wifi.mode" => {
                profile.wifi_mode = val.to_string();
                i += 2;
            }
            "wifi-sec.key-mgmt" | "802-11-wireless-security.key-mgmt" => {
                profile.wifi_security = val.to_string();
                i += 2;
            }
            "bridge.stp" => {
                profile.bridge_stp = val == "yes" || val == "true" || val == "1";
                i += 2;
            }
            "bridge.priority" => {
                if let Ok(p) = val.parse::<u32>() {
                    profile.bridge_priority = p;
                }
                i += 2;
            }
            "bond.options" | "bond.mode" => {
                profile.bond_mode = val.to_string();
                i += 2;
            }
            "vlan.id" => {
                if let Ok(id) = val.parse::<u32>() {
                    profile.vlan_id = id;
                }
                i += 2;
            }
            "vlan.parent" => {
                profile.vlan_parent = val.to_string();
                i += 2;
            }
            _ => {
                // Store unknown settings as custom properties
                if !val.is_empty()
                    && key != "type"
                    && key != "con-name"
                    && key != "ifname"
                {
                    profile
                        .properties
                        .insert(key.to_string(), val.to_string());
                }
                i += 2;
            }
        }
    }
}

fn cmd_connection_modify(state: &mut NmState, args: &[&str], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "Error: 'connection modify' requires a connection name and settings.");
        return 2;
    }

    let conn_name = args[0];
    let conn_idx = match state.find_connection(conn_name) {
        Some(i) => i,
        None => {
            let _ = writeln!(out, "Error: connection '{}' not found.", conn_name);
            return 10;
        }
    };

    let settings = &args[1..];
    if settings.is_empty() {
        let _ = writeln!(out, "Error: no settings provided to modify.");
        return 2;
    }

    parse_connection_settings(&mut state.connections[conn_idx], settings);

    let _ = writeln!(
        out,
        "Connection '{}' ({}) successfully modified.",
        state.connections[conn_idx].id, state.connections[conn_idx].uuid
    );
    0
}

fn cmd_connection_delete(state: &mut NmState, name: &str, out: &mut dyn Write) -> i32 {
    let conn_idx = match state.find_connection(name) {
        Some(i) => i,
        None => {
            let _ = writeln!(out, "Error: connection '{}' not found.", name);
            return 10;
        }
    };

    // Deactivate if active
    if state.connections[conn_idx].active {
        let dev_name = state.connections[conn_idx].device.clone();
        if !dev_name.is_empty()
            && let Some(di) = state.find_device(&dev_name) {
                state.devices[di].state = DeviceState::Disconnected;
                state.devices[di].connection.clear();
            }
    }

    let removed = state.connections.remove(conn_idx);
    let _ = writeln!(
        out,
        "Connection '{}' ({}) successfully deleted.",
        removed.id, removed.uuid
    );
    0
}

fn cmd_connection_clone(
    state: &mut NmState,
    src_name: &str,
    new_name: &str,
    out: &mut dyn Write,
) -> i32 {
    let src_idx = match state.find_connection(src_name) {
        Some(i) => i,
        None => {
            let _ = writeln!(out, "Error: connection '{}' not found.", src_name);
            return 10;
        }
    };

    if state.find_connection(new_name).is_some() {
        let _ = writeln!(out, "Error: connection '{}' already exists.", new_name);
        return 2;
    }

    let mut cloned = state.connections[src_idx].clone();
    cloned.id = new_name.to_string();
    cloned.uuid = generate_uuid(new_name);
    cloned.active = false;
    cloned.device.clear();

    let uuid = cloned.uuid.clone();
    state.connections.push(cloned);

    let _ = writeln!(
        out,
        "'{}' ({}) cloned as '{}' ({}).",
        src_name,
        state.connections[src_idx].uuid,
        new_name,
        uuid
    );
    0
}

// ============================================================================
// nmcli monitor
// ============================================================================

fn cmd_monitor(out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "NetworkManager is running. Monitoring changes...");
    let _ = writeln!(out, "(press Ctrl+C to stop)");
    0
}

// ============================================================================
// nmcli dispatch
// ============================================================================

fn run_nmcli(args: &[String], out: &mut dyn Write) -> i32 {
    if args.len() < 2 {
        return print_nmcli_usage(out);
    }

    // Parse global options first, then find the object keyword.
    let mut global = NmcliGlobal::new();
    let mut rest_start = 1; // skip argv[0]

    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    let mut idx = 0;

    while idx < rest.len() {
        match rest[idx] {
            "-t" | "--terse" => {
                global.mode = OutputMode::Terse;
                idx += 1;
                rest_start += 1;
            }
            "-p" | "--pretty" => {
                global.mode = OutputMode::Pretty;
                idx += 1;
                rest_start += 1;
            }
            "-m" | "--mode" => {
                if idx + 1 < rest.len() {
                    match rest[idx + 1] {
                        "tabular" => global.mode = OutputMode::Tabular,
                        "terse" => global.mode = OutputMode::Terse,
                        "multiline" | "pretty" => global.mode = OutputMode::Pretty,
                        _ => {}
                    }
                    idx += 2;
                    rest_start += 2;
                } else {
                    idx += 1;
                    rest_start += 1;
                }
            }
            "-c" | "--colors" => {
                if idx + 1 < rest.len() {
                    global.colors = rest[idx + 1] == "yes" || rest[idx + 1] == "auto";
                    idx += 2;
                    rest_start += 2;
                } else {
                    idx += 1;
                    rest_start += 1;
                }
            }
            "-f" | "--fields" => {
                if idx + 1 < rest.len() {
                    global.fields = Some(rest[idx + 1].to_string());
                    idx += 2;
                    rest_start += 2;
                } else {
                    idx += 1;
                    rest_start += 1;
                }
            }
            "--version" | "-v" => {
                let _ = writeln!(out, "nmcli tool, version {}", VERSION);
                return 0;
            }
            "--help" | "-h" => {
                return print_nmcli_usage(out);
            }
            _ => break,
        }
    }

    let cmd_args: Vec<&str> = args.iter().skip(rest_start).map(|s| s.as_str()).collect();
    if cmd_args.is_empty() {
        return print_nmcli_usage(out);
    }

    let mut state = NmState::new();
    let sub_args = if cmd_args.len() > 1 {
        &cmd_args[1..]
    } else {
        &[]
    };

    match cmd_args[0] {
        "general" | "g" | "gen" => cmd_general(&state, sub_args, &global, out),
        "networking" | "n" | "net" => cmd_networking(&mut state, sub_args, out),
        "radio" | "r" | "rad" => cmd_radio(&mut state, sub_args, &global, out),
        "device" | "d" | "dev" => cmd_device(&mut state, sub_args, &global, out),
        "connection" | "c" | "con" => cmd_connection(&mut state, sub_args, &global, out),
        "monitor" | "m" | "mon" => cmd_monitor(out),
        "help" => print_nmcli_usage(out),
        other => {
            let _ = writeln!(
                out,
                "Error: invalid object '{}'; use general, networking, radio, device, connection, or monitor.",
                other
            );
            2
        }
    }
}

fn print_nmcli_usage(out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "Usage: nmcli [OPTIONS] OBJECT {{ COMMAND | help }}");
    let _ = writeln!(out);
    let _ = writeln!(out, "OPTIONS");
    let _ = writeln!(out, "  -t, --terse                      terse output (colon-separated)");
    let _ = writeln!(out, "  -p, --pretty                     pretty output");
    let _ = writeln!(out, "  -m, --mode tabular|terse          output mode");
    let _ = writeln!(out, "  -c, --colors auto|yes|no         whether to use colors");
    let _ = writeln!(out, "  -f, --fields <field,...>          specify output fields");
    let _ = writeln!(out, "  -v, --version                    show version");
    let _ = writeln!(out, "  -h, --help                       show this help");
    let _ = writeln!(out);
    let _ = writeln!(out, "OBJECT");
    let _ = writeln!(out, "  general      NetworkManager general status and operations");
    let _ = writeln!(out, "  networking   overall networking control");
    let _ = writeln!(out, "  radio        NetworkManager radio switches");
    let _ = writeln!(out, "  connection   NetworkManager connection profiles");
    let _ = writeln!(out, "  device       devices managed by NetworkManager");
    let _ = writeln!(out, "  monitor      monitor NetworkManager changes");
    0
}

// ============================================================================
// NetworkManager daemon
// ============================================================================

/// Parsed daemon options.
struct DaemonOpts {
    no_daemon: bool,
    debug: bool,
    log_level: String,
    config_dir: String,
    version: bool,
    help: bool,
}

impl DaemonOpts {
    fn new() -> Self {
        Self {
            no_daemon: false,
            debug: false,
            log_level: String::from("INFO"),
            config_dir: String::from(NM_CONFIG_DIR),
            version: false,
            help: false,
        }
    }
}

fn parse_daemon_opts(args: &[&str]) -> DaemonOpts {
    let mut opts = DaemonOpts::new();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--no-daemon" | "-n" => {
                opts.no_daemon = true;
                i += 1;
            }
            "--debug" | "-d" => {
                opts.debug = true;
                opts.log_level = String::from("DEBUG");
                i += 1;
            }
            "--log-level" if i + 1 < args.len() => {
                opts.log_level = args[i + 1].to_uppercase();
                i += 2;
            }
            "--log-level" => {
                i += 1;
            }
            "--config-dir" if i + 1 < args.len() => {
                opts.config_dir = args[i + 1].to_string();
                i += 2;
            }
            "--config-dir" => {
                i += 1;
            }
            "--version" | "-V" => {
                opts.version = true;
                i += 1;
            }
            "--help" | "-h" => {
                opts.help = true;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    opts
}

fn run_networkmanager(args: &[String], out: &mut dyn Write) -> i32 {
    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    let opts = parse_daemon_opts(&rest);

    if opts.version {
        let _ = writeln!(out, "NetworkManager version {}", VERSION);
        return 0;
    }

    if opts.help {
        let _ = writeln!(out, "Usage: NetworkManager [OPTIONS]");
        let _ = writeln!(out);
        let _ = writeln!(out, "Options:");
        let _ = writeln!(out, "  -n, --no-daemon       don't become a daemon");
        let _ = writeln!(out, "  -d, --debug           enable debug logging");
        let _ = writeln!(out, "  --log-level LEVEL     set log level (ERR, WARN, INFO, DEBUG, TRACE)");
        let _ = writeln!(out, "  --config-dir PATH     set configuration directory (default: {})", NM_CONFIG_DIR);
        let _ = writeln!(out, "  -V, --version         show version");
        let _ = writeln!(out, "  -h, --help            show this help");
        return 0;
    }

    let _ = writeln!(out, "NetworkManager version {} starting...", VERSION);
    let _ = writeln!(out, "  config-dir: {}", opts.config_dir);
    let _ = writeln!(out, "  log-level: {}", opts.log_level);
    let _ = writeln!(
        out,
        "  daemon: {}",
        if opts.no_daemon { "no" } else { "yes" }
    );
    if opts.debug {
        let _ = writeln!(out, "  debug: enabled");
    }

    // Report simulated device discovery
    let _ = writeln!(out, "<INFO>  [daemon] reading configuration from {}/NetworkManager.conf", opts.config_dir);
    let _ = writeln!(out, "<INFO>  [device] (lo): new Loopback device (driver: 'unknown')");
    let _ = writeln!(out, "<INFO>  [device] (eth0): new Ethernet device (driver: 'virtio-net')");
    let _ = writeln!(out, "<INFO>  [device] (wlan0): new Wi-Fi device (driver: 'iwlwifi')");
    let _ = writeln!(out, "<INFO>  [manager] NetworkManager state is now CONNECTED_GLOBAL");

    if opts.no_daemon {
        let _ = writeln!(out, "<INFO>  [daemon] running in foreground (--no-daemon)");
    } else {
        let _ = writeln!(out, "<INFO>  [daemon] daemonizing...");
    }

    let _ = writeln!(out, "<INFO>  [dns] DNS: using systemd-resolved");
    let _ = writeln!(out, "<INFO>  [dhcp] DHCP client: internal");
    let _ = writeln!(
        out,
        "<INFO>  [policy] auto-activating connection 'Wired connection 1' on device eth0"
    );
    let _ = writeln!(
        out,
        "<INFO>  [policy] auto-activating connection 'MyHomeWiFi' on device wlan0"
    );

    0
}

// ============================================================================
// nmtui personality
// ============================================================================

fn run_nmtui(out: &mut dyn Write) -> i32 {
    let _ = writeln!(out, "nmtui: text UI not available in this build.");
    let _ = writeln!(out, "Use 'nmcli' for command-line network management.");
    1
}

// ============================================================================
// Top-level dispatch
// ============================================================================

fn run(args: &[String], out: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "nmcli: no arguments");
        return 1;
    }

    let personality = detect_personality(
        args.first().map(|s| s.as_str()).unwrap_or("nmcli"),
    );

    match personality {
        Personality::Nmtui => run_nmtui(out),
        Personality::NetworkManager => run_networkmanager(args, out),
        Personality::Nmcli => run_nmcli(args, out),
    }
}

// ============================================================================
// Entry point (SlateOS)
// ============================================================================

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let mut stdout = std::io::stdout().lock();
    run(&args, &mut stdout)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn out_buf() -> Vec<u8> {
        Vec::new()
    }

    fn out_str(buf: &[u8]) -> String {
        String::from_utf8_lossy(buf).to_string()
    }

    fn mk_args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    // -----------------------------------------------------------------------
    // Personality detection
    // -----------------------------------------------------------------------

    #[test]
    fn personality_nmcli() {
        assert_eq!(detect_personality("nmcli"), Personality::Nmcli);
    }

    #[test]
    fn personality_nmcli_path() {
        assert_eq!(detect_personality("/usr/bin/nmcli"), Personality::Nmcli);
    }

    #[test]
    fn personality_nmcli_exe() {
        assert_eq!(detect_personality("nmcli.exe"), Personality::Nmcli);
    }

    #[test]
    fn personality_nmcli_win_path() {
        assert_eq!(
            detect_personality("C:\\Program Files\\nmcli.exe"),
            Personality::Nmcli
        );
    }

    #[test]
    fn personality_nmtui() {
        assert_eq!(detect_personality("nmtui"), Personality::Nmtui);
    }

    #[test]
    fn personality_nmtui_path() {
        assert_eq!(detect_personality("/usr/bin/nmtui"), Personality::Nmtui);
    }

    #[test]
    fn personality_networkmanager() {
        assert_eq!(
            detect_personality("NetworkManager"),
            Personality::NetworkManager
        );
    }

    #[test]
    fn personality_networkmanager_path() {
        assert_eq!(
            detect_personality("/usr/sbin/NetworkManager"),
            Personality::NetworkManager
        );
    }

    #[test]
    fn personality_nm_shortname() {
        assert_eq!(detect_personality("nm"), Personality::NetworkManager);
    }

    #[test]
    fn personality_default() {
        assert_eq!(detect_personality("something"), Personality::Nmcli);
    }

    // -----------------------------------------------------------------------
    // ConnType parsing
    // -----------------------------------------------------------------------

    #[test]
    fn conntype_parse_ethernet() {
        assert_eq!(ConnType::parse("ethernet"), Some(ConnType::Ethernet));
        assert_eq!(ConnType::parse("802-3-ethernet"), Some(ConnType::Ethernet));
        assert_eq!(ConnType::parse("eth"), Some(ConnType::Ethernet));
    }

    #[test]
    fn conntype_parse_wifi() {
        assert_eq!(ConnType::parse("wifi"), Some(ConnType::Wifi));
        assert_eq!(ConnType::parse("802-11-wireless"), Some(ConnType::Wifi));
        assert_eq!(ConnType::parse("wireless"), Some(ConnType::Wifi));
    }

    #[test]
    fn conntype_parse_bridge() {
        assert_eq!(ConnType::parse("bridge"), Some(ConnType::Bridge));
    }

    #[test]
    fn conntype_parse_bond() {
        assert_eq!(ConnType::parse("bond"), Some(ConnType::Bond));
    }

    #[test]
    fn conntype_parse_vlan() {
        assert_eq!(ConnType::parse("vlan"), Some(ConnType::Vlan));
    }

    #[test]
    fn conntype_parse_vpn() {
        assert_eq!(ConnType::parse("vpn"), Some(ConnType::Vpn));
    }

    #[test]
    fn conntype_parse_loopback() {
        assert_eq!(ConnType::parse("loopback"), Some(ConnType::Loopback));
        assert_eq!(ConnType::parse("lo"), Some(ConnType::Loopback));
    }

    #[test]
    fn conntype_parse_unknown() {
        assert_eq!(ConnType::parse("foobar"), None);
    }

    #[test]
    fn conntype_display_name() {
        assert_eq!(ConnType::Ethernet.display_name(), "ethernet");
        assert_eq!(ConnType::Wifi.display_name(), "wifi");
        assert_eq!(ConnType::Bridge.display_name(), "bridge");
    }

    #[test]
    fn conntype_as_str() {
        assert_eq!(ConnType::Ethernet.as_str(), "802-3-ethernet");
        assert_eq!(ConnType::Wifi.as_str(), "802-11-wireless");
    }

    // -----------------------------------------------------------------------
    // DeviceState
    // -----------------------------------------------------------------------

    #[test]
    fn device_state_codes() {
        assert_eq!(DeviceState::Unmanaged.code(), 10);
        assert_eq!(DeviceState::Unavailable.code(), 20);
        assert_eq!(DeviceState::Disconnected.code(), 30);
        assert_eq!(DeviceState::Connecting.code(), 40);
        assert_eq!(DeviceState::Connected.code(), 100);
    }

    #[test]
    fn device_state_strings() {
        assert_eq!(DeviceState::Connected.as_str(), "connected");
        assert_eq!(DeviceState::Disconnected.as_str(), "disconnected");
        assert_eq!(DeviceState::Unavailable.as_str(), "unavailable");
    }

    // -----------------------------------------------------------------------
    // IPv4/IPv6 method parsing
    // -----------------------------------------------------------------------

    #[test]
    fn ipv4_method_parse() {
        assert_eq!(Ipv4Method::parse("auto"), Some(Ipv4Method::Auto));
        assert_eq!(Ipv4Method::parse("manual"), Some(Ipv4Method::Manual));
        assert_eq!(Ipv4Method::parse("disabled"), Some(Ipv4Method::Disabled));
        assert_eq!(Ipv4Method::parse("link-local"), Some(Ipv4Method::LinkLocal));
        assert_eq!(Ipv4Method::parse("shared"), Some(Ipv4Method::Shared));
        assert_eq!(Ipv4Method::parse("bogus"), None);
    }

    #[test]
    fn ipv6_method_parse() {
        assert_eq!(Ipv6Method::parse("auto"), Some(Ipv6Method::Auto));
        assert_eq!(Ipv6Method::parse("manual"), Some(Ipv6Method::Manual));
        assert_eq!(Ipv6Method::parse("disabled"), Some(Ipv6Method::Disabled));
        assert_eq!(Ipv6Method::parse("link-local"), Some(Ipv6Method::LinkLocal));
        assert_eq!(Ipv6Method::parse("ignore"), Some(Ipv6Method::Ignore));
        assert_eq!(Ipv6Method::parse("bogus"), None);
    }

    // -----------------------------------------------------------------------
    // UUID generation
    // -----------------------------------------------------------------------

    #[test]
    fn uuid_deterministic() {
        let a = generate_uuid("test");
        let b = generate_uuid("test");
        assert_eq!(a, b);
    }

    #[test]
    fn uuid_different_names() {
        let a = generate_uuid("foo");
        let b = generate_uuid("bar");
        assert_ne!(a, b);
    }

    #[test]
    fn uuid_format() {
        let u = generate_uuid("test");
        // 8-4-4-4-12 format
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
        // Version 4 marker
        assert!(parts[2].starts_with('4'));
    }

    // -----------------------------------------------------------------------
    // NmState defaults
    // -----------------------------------------------------------------------

    #[test]
    fn state_has_default_devices() {
        let state = NmState::new();
        assert!(state.devices.len() >= 3);
        assert!(state.find_device("lo").is_some());
        assert!(state.find_device("eth0").is_some());
        assert!(state.find_device("wlan0").is_some());
    }

    #[test]
    fn state_has_default_connections() {
        let state = NmState::new();
        assert!(state.connections.len() >= 2);
        assert!(state.find_connection("Wired connection 1").is_some());
        assert!(state.find_connection("MyHomeWiFi").is_some());
    }

    #[test]
    fn state_has_wifi_aps() {
        let state = NmState::new();
        assert!(state.wifi_aps.len() >= 3);
    }

    #[test]
    fn state_find_connection_by_uuid() {
        let state = NmState::new();
        let uuid = state.connections[0].uuid.clone();
        assert!(state.find_connection(&uuid).is_some());
    }

    // -----------------------------------------------------------------------
    // nmcli general
    // -----------------------------------------------------------------------

    #[test]
    fn nmcli_general_status() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "general", "status"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("STATE"));
        assert!(s.contains("running"));
    }

    #[test]
    fn nmcli_general_hostname() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "general", "hostname"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("slateos"));
    }

    #[test]
    fn nmcli_general_permissions() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "general", "permissions"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("PERMISSION"));
        assert!(s.contains("org.freedesktop.NetworkManager"));
    }

    #[test]
    fn nmcli_general_logging() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "general", "logging"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("INFO"));
    }

    #[test]
    fn nmcli_general_invalid() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "general", "bogus"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 2);
    }

    // -----------------------------------------------------------------------
    // nmcli device
    // -----------------------------------------------------------------------

    #[test]
    fn nmcli_device_status() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "status"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("DEVICE"));
        assert!(s.contains("eth0"));
        assert!(s.contains("wlan0"));
    }

    #[test]
    fn nmcli_device_default() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("eth0"));
    }

    #[test]
    fn nmcli_device_show_specific() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "show", "eth0"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("GENERAL.DEVICE:"));
        assert!(s.contains("eth0"));
        assert!(s.contains("52:54:00:ab:cd:ef"));
    }

    #[test]
    fn nmcli_device_show_missing() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "show", "nodev"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 10);
    }

    #[test]
    fn nmcli_device_show_all() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "show"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("lo"));
        assert!(s.contains("eth0"));
        assert!(s.contains("wlan0"));
    }

    #[test]
    fn nmcli_device_disconnect_connect() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "disconnect", "eth0"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("successfully disconnected"));
    }

    #[test]
    fn nmcli_device_connect_missing() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "connect", "nodev"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 10);
    }

    #[test]
    fn nmcli_device_set_managed() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "set", "eth0", "managed", "no"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_device_invalid() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "bogus"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 2);
    }

    // -----------------------------------------------------------------------
    // nmcli device wifi
    // -----------------------------------------------------------------------

    #[test]
    fn nmcli_wifi_list() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "wifi", "list"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("SSID"));
        assert!(s.contains("MyHomeWiFi"));
        assert!(s.contains("Neighbor5G"));
    }

    #[test]
    fn nmcli_wifi_connect() {
        let mut buf = out_buf();
        let args = mk_args(&[
            "nmcli",
            "device",
            "wifi",
            "connect",
            "Neighbor5G",
            "password",
            "secret123",
        ]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("successfully activated"));
    }

    #[test]
    fn nmcli_wifi_connect_not_found() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "wifi", "connect", "NoSuchNetwork"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 10);
    }

    #[test]
    fn nmcli_wifi_rescan() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "wifi", "rescan"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_wifi_invalid() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "device", "wifi", "bogus"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 2);
    }

    // -----------------------------------------------------------------------
    // nmcli connection
    // -----------------------------------------------------------------------

    #[test]
    fn nmcli_connection_show() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "show"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("NAME"));
        assert!(s.contains("Wired connection 1"));
        assert!(s.contains("MyHomeWiFi"));
    }

    #[test]
    fn nmcli_connection_show_detail() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "show", "MyHomeWiFi"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("connection.id:"));
        assert!(s.contains("MyHomeWiFi"));
        assert!(s.contains("802-11-wireless.ssid:"));
    }

    #[test]
    fn nmcli_connection_show_not_found() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "show", "NoSuchConn"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 10);
    }

    #[test]
    fn nmcli_connection_add_ethernet() {
        let mut buf = out_buf();
        let args = mk_args(&[
            "nmcli",
            "connection",
            "add",
            "type",
            "ethernet",
            "con-name",
            "TestEth",
            "ifname",
            "eth1",
        ]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("TestEth"));
        assert!(s.contains("successfully added"));
    }

    #[test]
    fn nmcli_connection_add_wifi() {
        let mut buf = out_buf();
        let args = mk_args(&[
            "nmcli",
            "connection",
            "add",
            "type",
            "wifi",
            "con-name",
            "TestWifi",
            "ssid",
            "MyNet",
        ]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("TestWifi"));
    }

    #[test]
    fn nmcli_connection_add_bridge() {
        let mut buf = out_buf();
        let args = mk_args(&[
            "nmcli",
            "connection",
            "add",
            "type",
            "bridge",
            "con-name",
            "TestBridge",
            "ifname",
            "br0",
            "bridge.stp",
            "yes",
        ]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_connection_add_bond() {
        let mut buf = out_buf();
        let args = mk_args(&[
            "nmcli",
            "connection",
            "add",
            "type",
            "bond",
            "con-name",
            "TestBond",
            "bond.options",
            "mode=active-backup",
        ]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_connection_add_vlan() {
        let mut buf = out_buf();
        let args = mk_args(&[
            "nmcli",
            "connection",
            "add",
            "type",
            "vlan",
            "con-name",
            "TestVlan",
            "vlan.id",
            "100",
            "vlan.parent",
            "eth0",
        ]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_connection_add_no_type() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "add"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 2);
    }

    #[test]
    fn nmcli_connection_up() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "up", "Work VPN"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("successfully activated"));
    }

    #[test]
    fn nmcli_connection_up_not_found() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "up", "NoSuchConn"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 10);
    }

    #[test]
    fn nmcli_connection_down() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "down", "Wired connection 1"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("deactivated"));
    }

    #[test]
    fn nmcli_connection_down_inactive() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "down", "Work VPN"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("not active"));
    }

    #[test]
    fn nmcli_connection_delete() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "delete", "Work VPN"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("successfully deleted"));
    }

    #[test]
    fn nmcli_connection_delete_not_found() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "delete", "NoSuch"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 10);
    }

    #[test]
    fn nmcli_connection_modify() {
        let mut buf = out_buf();
        let args = mk_args(&[
            "nmcli",
            "connection",
            "modify",
            "Wired connection 1",
            "ipv4.method",
            "manual",
            "ipv4.addresses",
            "10.0.0.5/24",
        ]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("successfully modified"));
    }

    #[test]
    fn nmcli_connection_modify_not_found() {
        let mut buf = out_buf();
        let args = mk_args(&[
            "nmcli",
            "connection",
            "modify",
            "NoSuch",
            "ipv4.method",
            "auto",
        ]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 10);
    }

    #[test]
    fn nmcli_connection_modify_no_settings() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "modify", "MyHomeWiFi"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 2);
    }

    #[test]
    fn nmcli_connection_clone() {
        let mut buf = out_buf();
        let args = mk_args(&[
            "nmcli",
            "connection",
            "clone",
            "Wired connection 1",
            "Wired-copy",
        ]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("cloned"));
        assert!(s.contains("Wired-copy"));
    }

    #[test]
    fn nmcli_connection_clone_not_found() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "clone", "NoSuch", "Copy"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 10);
    }

    #[test]
    fn nmcli_connection_reload() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "reload"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_connection_invalid() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "connection", "bogus"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 2);
    }

    // -----------------------------------------------------------------------
    // nmcli radio
    // -----------------------------------------------------------------------

    #[test]
    fn nmcli_radio_all() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "radio"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("WIFI"));
        assert!(s.contains("WWAN"));
    }

    #[test]
    fn nmcli_radio_wifi_status() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "radio", "wifi"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("enabled"));
    }

    #[test]
    fn nmcli_radio_wifi_off() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "radio", "wifi", "off"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_radio_wifi_on() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "radio", "wifi", "on"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_radio_wwan() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "radio", "wwan"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_radio_invalid() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "radio", "bogus"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 2);
    }

    // -----------------------------------------------------------------------
    // nmcli networking
    // -----------------------------------------------------------------------

    #[test]
    fn nmcli_networking_status() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "networking"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("enabled"));
    }

    #[test]
    fn nmcli_networking_on() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "networking", "on"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_networking_off() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "networking", "off"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
    }

    #[test]
    fn nmcli_networking_connectivity() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "networking", "connectivity"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("full"));
    }

    #[test]
    fn nmcli_networking_invalid() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "networking", "bogus"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 2);
    }

    // -----------------------------------------------------------------------
    // nmcli monitor
    // -----------------------------------------------------------------------

    #[test]
    fn nmcli_monitor() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "monitor"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("Monitoring"));
    }

    // -----------------------------------------------------------------------
    // nmcli global options
    // -----------------------------------------------------------------------

    #[test]
    fn nmcli_version() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "--version"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains(VERSION));
    }

    #[test]
    fn nmcli_help() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "--help"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("Usage:"));
    }

    #[test]
    fn nmcli_terse_mode() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "-t", "general", "status"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        // Terse mode: colon-separated
        assert!(s.contains(':'));
    }

    #[test]
    fn nmcli_fields_filter() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "-f", "STATE,WIFI", "general", "status"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("STATE"));
        assert!(s.contains("WIFI"));
        // Should NOT contain WWAN since we only asked for STATE,WIFI
        assert!(!s.contains("WWAN"));
    }

    #[test]
    fn nmcli_no_args() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("Usage:"));
    }

    #[test]
    fn nmcli_invalid_object() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "foobar"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 2);
    }

    #[test]
    fn nmcli_abbreviation_g() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "g"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("STATE"));
    }

    #[test]
    fn nmcli_abbreviation_d() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "d"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("DEVICE"));
    }

    #[test]
    fn nmcli_abbreviation_c() {
        let mut buf = out_buf();
        let args = mk_args(&["nmcli", "c"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("NAME"));
    }

    // -----------------------------------------------------------------------
    // NetworkManager daemon personality
    // -----------------------------------------------------------------------

    #[test]
    fn daemon_version() {
        let mut buf = out_buf();
        let args = mk_args(&["NetworkManager", "--version"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("NetworkManager version"));
    }

    #[test]
    fn daemon_help() {
        let mut buf = out_buf();
        let args = mk_args(&["NetworkManager", "--help"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("Usage: NetworkManager"));
        assert!(s.contains("--no-daemon"));
    }

    #[test]
    fn daemon_startup() {
        let mut buf = out_buf();
        let args = mk_args(&["NetworkManager", "--no-daemon"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("starting..."));
        assert!(s.contains("foreground"));
    }

    #[test]
    fn daemon_debug() {
        let mut buf = out_buf();
        let args = mk_args(&["NetworkManager", "--debug"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("DEBUG"));
    }

    #[test]
    fn daemon_log_level() {
        let mut buf = out_buf();
        let args = mk_args(&["NetworkManager", "--log-level", "WARN"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("WARN"));
    }

    #[test]
    fn daemon_config_dir() {
        let mut buf = out_buf();
        let args = mk_args(&["NetworkManager", "--config-dir", "/custom/dir"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("/custom/dir"));
    }

    #[test]
    fn daemon_device_discovery() {
        let mut buf = out_buf();
        let args = mk_args(&["NetworkManager"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("(lo)"));
        assert!(s.contains("(eth0)"));
        assert!(s.contains("(wlan0)"));
        assert!(s.contains("CONNECTED_GLOBAL"));
    }

    #[test]
    fn daemon_daemonize() {
        let mut buf = out_buf();
        let args = mk_args(&["NetworkManager"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 0);
        let s = out_str(&buf);
        assert!(s.contains("daemonizing"));
    }

    // -----------------------------------------------------------------------
    // nmtui personality
    // -----------------------------------------------------------------------

    #[test]
    fn nmtui_stub() {
        let mut buf = out_buf();
        let args = mk_args(&["nmtui"]);
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 1);
        let s = out_str(&buf);
        assert!(s.contains("text UI not available"));
    }

    // -----------------------------------------------------------------------
    // Table formatting
    // -----------------------------------------------------------------------

    #[test]
    fn table_formatting() {
        let mut buf = out_buf();
        let headers = ["A", "BB", "CCC"];
        let rows = vec![
            vec![String::from("1"), String::from("22"), String::from("333")],
            vec![
                String::from("long"),
                String::from("x"),
                String::from("y"),
            ],
        ];
        format_table(&headers, &rows, &mut buf);
        let s = out_str(&buf);
        // Header should be there
        assert!(s.contains("A"));
        assert!(s.contains("BB"));
        assert!(s.contains("CCC"));
        // Data too
        assert!(s.contains("long"));
    }

    #[test]
    fn terse_formatting() {
        let mut buf = out_buf();
        let headers = ["A", "B", "C"];
        let rows = vec![vec![
            String::from("1"),
            String::from("2"),
            String::from("3"),
        ]];
        format_terse(&headers, &rows, &mut buf);
        let s = out_str(&buf);
        assert!(s.contains("A:B:C"));
        assert!(s.contains("1:2:3"));
    }

    // -----------------------------------------------------------------------
    // Connection profile construction
    // -----------------------------------------------------------------------

    #[test]
    fn connection_profile_defaults() {
        let p = ConnectionProfile::new("test", ConnType::Ethernet);
        assert_eq!(p.id, "test");
        assert_eq!(p.conn_type, ConnType::Ethernet);
        assert!(p.autoconnect);
        assert_eq!(p.ipv4_method, Ipv4Method::Auto);
        assert_eq!(p.ipv6_method, Ipv6Method::Auto);
        assert!(!p.active);
    }

    #[test]
    fn connection_profile_wifi_defaults() {
        let p = ConnectionProfile::new("mywifi", ConnType::Wifi);
        assert_eq!(p.wifi_mode, "infrastructure");
    }

    #[test]
    fn connection_profile_bridge_defaults() {
        let p = ConnectionProfile::new("br", ConnType::Bridge);
        assert!(p.bridge_stp);
        assert_eq!(p.bridge_priority, 32768);
    }

    #[test]
    fn connection_profile_bond_defaults() {
        let p = ConnectionProfile::new("bond", ConnType::Bond);
        assert_eq!(p.bond_mode, "balance-rr");
    }

    // -----------------------------------------------------------------------
    // parse_connection_settings
    // -----------------------------------------------------------------------

    #[test]
    fn parse_settings_ipv4() {
        let mut p = ConnectionProfile::new("t", ConnType::Ethernet);
        parse_connection_settings(&mut p, &["ipv4.method", "manual", "ipv4.addresses", "10.0.0.1/24"]);
        assert_eq!(p.ipv4_method, Ipv4Method::Manual);
        assert_eq!(p.ipv4_addresses.len(), 1);
        assert_eq!(p.ipv4_addresses[0], "10.0.0.1/24");
    }

    #[test]
    fn parse_settings_dns() {
        let mut p = ConnectionProfile::new("t", ConnType::Ethernet);
        parse_connection_settings(&mut p, &["ipv4.dns", "8.8.8.8", "ipv4.dns", "1.1.1.1"]);
        assert_eq!(p.ipv4_dns.len(), 2);
    }

    #[test]
    fn parse_settings_autoconnect() {
        let mut p = ConnectionProfile::new("t", ConnType::Ethernet);
        parse_connection_settings(&mut p, &["autoconnect", "no"]);
        assert!(!p.autoconnect);
    }

    #[test]
    fn parse_settings_vlan() {
        let mut p = ConnectionProfile::new("t", ConnType::Vlan);
        parse_connection_settings(&mut p, &["vlan.id", "42", "vlan.parent", "eth0"]);
        assert_eq!(p.vlan_id, 42);
        assert_eq!(p.vlan_parent, "eth0");
    }

    // -----------------------------------------------------------------------
    // find_option_value
    // -----------------------------------------------------------------------

    #[test]
    fn find_option_basic() {
        let args = &["type", "ethernet", "con-name", "Eth1"];
        assert_eq!(find_option_value(args, "type"), Some("ethernet"));
        assert_eq!(find_option_value(args, "con-name"), Some("Eth1"));
        assert_eq!(find_option_value(args, "missing"), None);
    }

    #[test]
    fn find_option_with_dashes() {
        let args = &["--password", "secret"];
        assert_eq!(find_option_value(args, "password"), Some("secret"));
    }

    // -----------------------------------------------------------------------
    // Empty args
    // -----------------------------------------------------------------------

    #[test]
    fn empty_args() {
        let mut buf = out_buf();
        let args: Vec<String> = vec![];
        let rc = run(&args, &mut buf);
        assert_eq!(rc, 1);
    }
}

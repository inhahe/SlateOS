//! Network settings panel for the desktop shell.
//!
//! Provides comprehensive network configuration including WiFi network
//! management, Ethernet settings, DNS configuration, proxy settings,
//! VPN profiles, and firewall rules. Communicates with the network
//! stack via IPC for actual configuration changes.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Connection types and states
// ============================================================================

/// Type of network interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfaceType {
    Ethernet,
    WiFi,
    Loopback,
    Virtual,
    Bridge,
    VPN,
}

impl InterfaceType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "Ethernet",
            Self::WiFi => "Wi-Fi",
            Self::Loopback => "Loopback",
            Self::Virtual => "Virtual",
            Self::Bridge => "Bridge",
            Self::VPN => "VPN",
        }
    }

    /// Icon character for display.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Ethernet => "\u{1F5A7}", // desktop computer
            Self::WiFi => "\u{1F4F6}",     // antenna bars
            Self::Loopback => "\u{1F501}", // repeat
            Self::Virtual => "\u{1F4BB}",  // laptop
            Self::Bridge => "\u{1F309}",   // bridge
            Self::VPN => "\u{1F512}",      // lock
        }
    }
}

/// Current connection state of an interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Limited,
    NoInternet,
    Disabled,
}

impl ConnectionState {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting...",
            Self::Connected => "Connected",
            Self::Limited => "Limited connectivity",
            Self::NoInternet => "No internet",
            Self::Disabled => "Disabled",
        }
    }

    /// Status color.
    pub fn color(self) -> Color {
        match self {
            Self::Connected => GREEN,
            Self::Connecting => YELLOW,
            Self::Limited | Self::NoInternet => PEACH,
            Self::Disconnected | Self::Disabled => OVERLAY0,
        }
    }
}

// ============================================================================
// IP configuration
// ============================================================================

/// IPv4 address (simplified representation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ipv4Addr {
    pub octets: [u8; 4],
}

impl std::fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.octets[0], self.octets[1], self.octets[2], self.octets[3]
        )
    }
}

impl Ipv4Addr {
    /// Create from four octets.
    pub fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self { octets: [a, b, c, d] }
    }

    /// Parse from dotted decimal string.
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 4 {
            return None;
        }
        let mut octets = [0u8; 4];
        for (i, part) in parts.iter().enumerate() {
            octets[i] = part.parse().ok()?;
        }
        Some(Self { octets })
    }

    /// Check if this is a private address.
    pub fn is_private(&self) -> bool {
        matches!(
            self.octets,
            [10, ..] | [172, 16..=31, ..] | [192, 168, ..]
        )
    }

    /// Check if this is a loopback address.
    pub fn is_loopback(&self) -> bool {
        self.octets[0] == 127
    }
}

/// How IP configuration is obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpConfigMethod {
    /// Automatic via DHCP.
    Dhcp,
    /// Static/manual configuration.
    Static,
    /// Link-local (169.254.x.x).
    LinkLocal,
}

impl IpConfigMethod {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Dhcp => "Automatic (DHCP)",
            Self::Static => "Manual",
            Self::LinkLocal => "Link-local",
        }
    }
}

/// IPv4 configuration for an interface.
#[derive(Clone, Debug)]
pub struct Ipv4Config {
    pub method: IpConfigMethod,
    pub address: Option<Ipv4Addr>,
    pub subnet_mask: Option<Ipv4Addr>,
    pub gateway: Option<Ipv4Addr>,
    pub dns_primary: Option<Ipv4Addr>,
    pub dns_secondary: Option<Ipv4Addr>,
}

impl Default for Ipv4Config {
    fn default() -> Self {
        Self {
            method: IpConfigMethod::Dhcp,
            address: None,
            subnet_mask: None,
            gateway: None,
            dns_primary: None,
            dns_secondary: None,
        }
    }
}

impl Ipv4Config {
    /// Validate that static configuration has required fields.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.method == IpConfigMethod::Static {
            if self.address.is_none() {
                return Err("Static configuration requires an IP address");
            }
            if self.subnet_mask.is_none() {
                return Err("Static configuration requires a subnet mask");
            }
        }
        if let Some(ref addr) = self.address
            && addr.is_loopback() {
                return Err("Cannot assign loopback address to interface");
            }
        Ok(())
    }

    /// Get a formatted summary of the configuration.
    pub fn summary(&self) -> String {
        match self.method {
            IpConfigMethod::Dhcp => {
                if let Some(ref addr) = self.address {
                    format!("DHCP ({addr})")
                } else {
                    "DHCP (obtaining...)".to_string()
                }
            }
            IpConfigMethod::Static => {
                if let Some(ref addr) = self.address {
                    addr.to_string()
                } else {
                    "Static (not configured)".to_string()
                }
            }
            IpConfigMethod::LinkLocal => "Link-local".to_string(),
        }
    }
}

// ============================================================================
// WiFi types
// ============================================================================

/// WiFi security type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WiFiSecurity {
    Open,
    WEP,
    WPA,
    WPA2Personal,
    WPA2Enterprise,
    WPA3Personal,
    WPA3Enterprise,
}

impl WiFiSecurity {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::WEP => "WEP",
            Self::WPA => "WPA",
            Self::WPA2Personal => "WPA2-Personal",
            Self::WPA2Enterprise => "WPA2-Enterprise",
            Self::WPA3Personal => "WPA3-Personal",
            Self::WPA3Enterprise => "WPA3-Enterprise",
        }
    }

    /// Whether a password is required.
    pub fn requires_password(self) -> bool {
        !matches!(self, Self::Open)
    }

    /// Security strength indicator (0-3).
    pub fn strength(self) -> u8 {
        match self {
            Self::Open => 0,
            Self::WEP => 1,
            Self::WPA => 1,
            Self::WPA2Personal => 2,
            Self::WPA2Enterprise => 3,
            Self::WPA3Personal => 3,
            Self::WPA3Enterprise => 3,
        }
    }

    /// Color based on security strength.
    pub fn color(self) -> Color {
        match self.strength() {
            0 => RED,
            1 => PEACH,
            2 => YELLOW,
            _ => GREEN,
        }
    }
}

/// WiFi signal quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignalQuality {
    /// < -80 dBm
    Weak,
    /// -80 to -67 dBm
    Fair,
    /// -67 to -50 dBm
    Good,
    /// > -50 dBm
    Excellent,
}

impl SignalQuality {
    /// Create from signal strength in dBm.
    pub fn from_dbm(dbm: i32) -> Self {
        if dbm > -50 {
            Self::Excellent
        } else if dbm > -67 {
            Self::Good
        } else if dbm > -80 {
            Self::Fair
        } else {
            Self::Weak
        }
    }

    /// Number of bars (1-4).
    pub fn bars(self) -> u8 {
        match self {
            Self::Weak => 1,
            Self::Fair => 2,
            Self::Good => 3,
            Self::Excellent => 4,
        }
    }

    /// Label text.
    pub fn label(self) -> &'static str {
        match self {
            Self::Weak => "Weak",
            Self::Fair => "Fair",
            Self::Good => "Good",
            Self::Excellent => "Excellent",
        }
    }

    /// Color for signal indicator.
    pub fn color(self) -> Color {
        match self {
            Self::Weak => RED,
            Self::Fair => PEACH,
            Self::Good => YELLOW,
            Self::Excellent => GREEN,
        }
    }
}

/// A WiFi network visible in scanning.
#[derive(Clone, Debug)]
pub struct WiFiNetwork {
    pub ssid: String,
    pub bssid: String,
    pub security: WiFiSecurity,
    pub signal_dbm: i32,
    pub channel: u32,
    pub frequency_mhz: u32,
    pub is_hidden: bool,
    pub is_saved: bool,
    pub is_connected: bool,
}

impl WiFiNetwork {
    /// Get signal quality.
    pub fn signal_quality(&self) -> SignalQuality {
        SignalQuality::from_dbm(self.signal_dbm)
    }

    /// Get frequency band label.
    pub fn band(&self) -> &'static str {
        // 4900-4999 MHz is the public-safety/unlicensed slice that's still
        // colloquially called "5 GHz" alongside the regular 5 GHz band.
        if self.frequency_mhz >= 4900 {
            "5 GHz"
        } else {
            "2.4 GHz"
        }
    }
}

/// Saved WiFi network profile.
#[derive(Clone, Debug)]
pub struct SavedWiFiProfile {
    pub ssid: String,
    pub security: WiFiSecurity,
    pub auto_connect: bool,
    pub metered: bool,
    pub random_mac: bool,
    pub priority: u32,
    pub last_connected: Option<u64>,
}

impl Default for SavedWiFiProfile {
    fn default() -> Self {
        Self {
            ssid: String::new(),
            security: WiFiSecurity::WPA2Personal,
            auto_connect: true,
            metered: false,
            random_mac: false,
            priority: 0,
            last_connected: None,
        }
    }
}

// ============================================================================
// DNS configuration
// ============================================================================

/// DNS resolution mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DnsMode {
    /// Use DNS from DHCP.
    Automatic,
    /// Use manually specified DNS servers.
    Manual,
}

/// DNS over HTTPS provider presets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DohProvider {
    pub name: String,
    pub url: String,
    pub description: String,
}

/// Default well-known DoH providers.
pub fn default_doh_providers() -> Vec<DohProvider> {
    vec![
        DohProvider {
            name: "Cloudflare".to_string(),
            url: "https://1.1.1.1/dns-query".to_string(),
            description: "Fast, privacy-focused".to_string(),
        },
        DohProvider {
            name: "Google".to_string(),
            url: "https://dns.google/dns-query".to_string(),
            description: "Google Public DNS".to_string(),
        },
        DohProvider {
            name: "Quad9".to_string(),
            url: "https://dns.quad9.net/dns-query".to_string(),
            description: "Security-focused, blocks malware".to_string(),
        },
        DohProvider {
            name: "AdGuard".to_string(),
            url: "https://dns.adguard.com/dns-query".to_string(),
            description: "Ad-blocking DNS".to_string(),
        },
    ]
}

/// DNS configuration.
#[derive(Clone, Debug)]
pub struct DnsConfig {
    pub mode: DnsMode,
    pub primary: Option<Ipv4Addr>,
    pub secondary: Option<Ipv4Addr>,
    pub search_domains: Vec<String>,
    pub dns_over_https: bool,
    pub doh_url: Option<String>,
    pub cache_enabled: bool,
    pub cache_size: u32,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            mode: DnsMode::Automatic,
            primary: None,
            secondary: None,
            search_domains: Vec::new(),
            dns_over_https: false,
            doh_url: None,
            cache_enabled: true,
            cache_size: 1024,
        }
    }
}

// ============================================================================
// Proxy configuration
// ============================================================================

/// Proxy type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyType {
    None,
    Http,
    Https,
    Socks4,
    Socks5,
    Auto,
}

impl ProxyType {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No proxy",
            Self::Http => "HTTP",
            Self::Https => "HTTPS",
            Self::Socks4 => "SOCKS4",
            Self::Socks5 => "SOCKS5",
            Self::Auto => "Auto-detect",
        }
    }
}

/// Proxy server configuration.
#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub proxy_type: ProxyType,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub requires_auth: bool,
    pub bypass_list: Vec<String>,
    pub bypass_local: bool,
    pub pac_url: Option<String>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            proxy_type: ProxyType::None,
            host: String::new(),
            port: 8080,
            username: None,
            requires_auth: false,
            bypass_list: vec!["localhost".to_string(), "127.0.0.1".to_string()],
            bypass_local: true,
            pac_url: None,
        }
    }
}

impl ProxyConfig {
    /// Check if proxy is active.
    pub fn is_active(&self) -> bool {
        self.proxy_type != ProxyType::None
    }

    /// Validate the proxy configuration.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.proxy_type == ProxyType::None {
            return Ok(());
        }
        if self.proxy_type == ProxyType::Auto {
            if self.pac_url.as_ref().is_none_or(|u| u.is_empty()) {
                return Err("Auto-detect proxy requires a PAC URL");
            }
            return Ok(());
        }
        if self.host.is_empty() {
            return Err("Proxy host is required");
        }
        if self.port == 0 {
            return Err("Proxy port must be non-zero");
        }
        Ok(())
    }
}

// ============================================================================
// Firewall
// ============================================================================

/// Firewall rule action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirewallAction {
    Allow,
    Block,
    Ask,
}

impl FirewallAction {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Block => "Block",
            Self::Ask => "Ask",
        }
    }

    /// Color for the action.
    pub fn color(self) -> Color {
        match self {
            Self::Allow => GREEN,
            Self::Block => RED,
            Self::Ask => YELLOW,
        }
    }
}

/// Firewall rule direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirewallDirection {
    Inbound,
    Outbound,
    Both,
}

impl FirewallDirection {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Inbound => "Inbound",
            Self::Outbound => "Outbound",
            Self::Both => "Both",
        }
    }
}

/// Firewall rule protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirewallProtocol {
    Any,
    Tcp,
    Udp,
    Icmp,
}

impl FirewallProtocol {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
            Self::Icmp => "ICMP",
        }
    }
}

/// A firewall rule.
#[derive(Clone, Debug)]
pub struct FirewallRule {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    pub action: FirewallAction,
    pub direction: FirewallDirection,
    pub protocol: FirewallProtocol,
    pub port_range: Option<(u16, u16)>,
    pub remote_address: Option<String>,
    pub application: Option<String>,
    pub description: String,
}

impl FirewallRule {
    /// Format the port range for display.
    pub fn port_display(&self) -> String {
        match self.port_range {
            Some((start, end)) if start == end => format!("{start}"),
            Some((start, end)) => format!("{start}-{end}"),
            None => "Any".to_string(),
        }
    }
}

/// Firewall configuration.
#[derive(Clone, Debug)]
pub struct FirewallConfig {
    pub enabled: bool,
    pub default_inbound: FirewallAction,
    pub default_outbound: FirewallAction,
    pub rules: Vec<FirewallRule>,
    pub log_blocked: bool,
    pub block_icmp: bool,
    pub stealth_mode: bool,
    pub next_rule_id: u64,
}

impl Default for FirewallConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_inbound: FirewallAction::Block,
            default_outbound: FirewallAction::Allow,
            rules: Vec::new(),
            log_blocked: true,
            block_icmp: false,
            stealth_mode: false,
            next_rule_id: 1,
        }
    }
}

impl FirewallConfig {
    /// Add a new rule, returning its ID.
    pub fn add_rule(&mut self, mut rule: FirewallRule) -> u64 {
        let id = self.next_rule_id;
        self.next_rule_id += 1;
        rule.id = id;
        self.rules.push(rule);
        id
    }

    /// Remove a rule by ID.
    pub fn remove_rule(&mut self, id: u64) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.id != id);
        self.rules.len() < before
    }

    /// Toggle a rule's enabled state.
    pub fn toggle_rule(&mut self, id: u64) -> Option<bool> {
        if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
            rule.enabled = !rule.enabled;
            Some(rule.enabled)
        } else {
            None
        }
    }

    /// Get all enabled rules matching a direction.
    pub fn active_rules(&self, direction: FirewallDirection) -> Vec<&FirewallRule> {
        self.rules
            .iter()
            .filter(|r| {
                r.enabled
                    && (r.direction == direction || r.direction == FirewallDirection::Both)
            })
            .collect()
    }

    /// Count enabled rules.
    pub fn active_rule_count(&self) -> usize {
        self.rules.iter().filter(|r| r.enabled).count()
    }
}

// ============================================================================
// Network interfaces
// ============================================================================

/// A network interface with all its configuration.
#[derive(Clone, Debug)]
pub struct NetworkInterface {
    pub name: String,
    pub display_name: String,
    pub interface_type: InterfaceType,
    pub state: ConnectionState,
    pub mac_address: String,
    pub ipv4: Ipv4Config,
    pub mtu: u32,
    pub speed_mbps: Option<u32>,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_errors: u64,
    pub rx_errors: u64,
    pub is_default: bool,
    pub enabled: bool,
}

impl NetworkInterface {
    /// Create a default Ethernet interface.
    pub fn default_ethernet() -> Self {
        Self {
            name: "eth0".to_string(),
            display_name: "Ethernet".to_string(),
            interface_type: InterfaceType::Ethernet,
            state: ConnectionState::Connected,
            mac_address: "AA:BB:CC:DD:EE:FF".to_string(),
            ipv4: Ipv4Config {
                method: IpConfigMethod::Dhcp,
                address: Some(Ipv4Addr::new(192, 168, 1, 100)),
                subnet_mask: Some(Ipv4Addr::new(255, 255, 255, 0)),
                gateway: Some(Ipv4Addr::new(192, 168, 1, 1)),
                dns_primary: Some(Ipv4Addr::new(8, 8, 8, 8)),
                dns_secondary: Some(Ipv4Addr::new(8, 8, 4, 4)),
            },
            mtu: 1500,
            speed_mbps: Some(1000),
            tx_bytes: 0,
            rx_bytes: 0,
            tx_packets: 0,
            rx_packets: 0,
            tx_errors: 0,
            rx_errors: 0,
            is_default: true,
            enabled: true,
        }
    }

    /// Format speed for display.
    pub fn speed_display(&self) -> String {
        match self.speed_mbps {
            Some(speed) if speed >= 1000 => format!("{} Gbps", speed / 1000),
            Some(speed) => format!("{speed} Mbps"),
            None => "Unknown".to_string(),
        }
    }

    /// Format transfer amount.
    pub fn format_bytes(bytes: u64) -> String {
        if bytes >= 1_073_741_824 {
            format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
        } else if bytes >= 1_048_576 {
            format!("{:.1} MB", bytes as f64 / 1_048_576.0)
        } else if bytes >= 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else {
            format!("{bytes} B")
        }
    }

    /// Get a summary string for this interface.
    pub fn summary(&self) -> String {
        format!(
            "{} — {} — {}",
            self.display_name,
            self.state.label(),
            self.ipv4.summary()
        )
    }
}

// ============================================================================
// Network settings aggregate
// ============================================================================

/// Complete network settings.
#[derive(Clone, Debug)]
pub struct NetworkSettings {
    pub interfaces: Vec<NetworkInterface>,
    pub wifi_networks: Vec<WiFiNetwork>,
    pub saved_wifi: Vec<SavedWiFiProfile>,
    pub wifi_enabled: bool,
    pub wifi_scanning: bool,
    pub airplane_mode: bool,
    pub dns: DnsConfig,
    pub proxy: ProxyConfig,
    pub firewall: FirewallConfig,
    pub hostname: String,
    pub data_usage_tracking: bool,
    pub metered_connection: bool,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            interfaces: vec![NetworkInterface::default_ethernet()],
            wifi_networks: Vec::new(),
            saved_wifi: Vec::new(),
            wifi_enabled: true,
            wifi_scanning: false,
            airplane_mode: false,
            dns: DnsConfig::default(),
            proxy: ProxyConfig::default(),
            firewall: FirewallConfig::default(),
            hostname: "ouros-desktop".to_string(),
            data_usage_tracking: true,
            metered_connection: false,
        }
    }
}

impl NetworkSettings {
    /// Get the default (primary) interface.
    pub fn default_interface(&self) -> Option<&NetworkInterface> {
        self.interfaces.iter().find(|i| i.is_default)
    }

    /// Get a mutable reference to an interface by name.
    pub fn interface_mut(&mut self, name: &str) -> Option<&mut NetworkInterface> {
        self.interfaces.iter_mut().find(|i| i.name == name)
    }

    /// Toggle airplane mode. Disables all wireless when on.
    pub fn set_airplane_mode(&mut self, enabled: bool) {
        self.airplane_mode = enabled;
        if enabled {
            self.wifi_enabled = false;
            self.wifi_scanning = false;
        }
    }

    /// Toggle WiFi.
    pub fn set_wifi_enabled(&mut self, enabled: bool) {
        if self.airplane_mode && enabled {
            return; // Cannot enable WiFi in airplane mode
        }
        self.wifi_enabled = enabled;
        if !enabled {
            self.wifi_scanning = false;
            // Disconnect any WiFi interfaces
            for iface in &mut self.interfaces {
                if iface.interface_type == InterfaceType::WiFi {
                    iface.state = ConnectionState::Disconnected;
                }
            }
        }
    }

    /// Start a WiFi scan.
    pub fn start_wifi_scan(&mut self) {
        if self.wifi_enabled && !self.airplane_mode {
            self.wifi_scanning = true;
        }
    }

    /// Update the available WiFi networks from a scan result.
    pub fn update_wifi_networks(&mut self, networks: Vec<WiFiNetwork>) {
        self.wifi_networks = networks;
        // Mark saved networks
        for net in &mut self.wifi_networks {
            net.is_saved = self.saved_wifi.iter().any(|s| s.ssid == net.ssid);
        }
        self.wifi_scanning = false;
    }

    /// Get WiFi networks sorted by signal strength.
    pub fn sorted_wifi_networks(&self) -> Vec<&WiFiNetwork> {
        let mut nets: Vec<&WiFiNetwork> = self.wifi_networks.iter().collect();
        // Connected first, then saved, then by signal
        nets.sort_by(|a, b| {
            b.is_connected
                .cmp(&a.is_connected)
                .then(b.is_saved.cmp(&a.is_saved))
                .then(b.signal_dbm.cmp(&a.signal_dbm))
        });
        nets
    }

    /// Save a WiFi profile.
    pub fn save_wifi_profile(&mut self, ssid: &str, security: WiFiSecurity) {
        if !self.saved_wifi.iter().any(|p| p.ssid == ssid) {
            self.saved_wifi.push(SavedWiFiProfile {
                ssid: ssid.to_string(),
                security,
                ..SavedWiFiProfile::default()
            });
        }
    }

    /// Remove a saved WiFi profile.
    pub fn forget_wifi(&mut self, ssid: &str) -> bool {
        let before = self.saved_wifi.len();
        self.saved_wifi.retain(|p| p.ssid != ssid);
        self.saved_wifi.len() < before
    }

    /// Overall connection status text.
    pub fn connection_status(&self) -> &'static str {
        if self.airplane_mode {
            return "Airplane mode";
        }
        if let Some(iface) = self.default_interface() {
            return iface.state.label();
        }
        "No network"
    }

    /// Count active interfaces.
    pub fn active_interface_count(&self) -> usize {
        self.interfaces
            .iter()
            .filter(|i| matches!(i.state, ConnectionState::Connected | ConnectionState::Limited))
            .count()
    }
}

// ============================================================================
// Settings UI
// ============================================================================

/// Tabs in the network settings panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkSettingsTab {
    Status,
    WiFi,
    Ethernet,
    Dns,
    Proxy,
    Firewall,
}

impl NetworkSettingsTab {
    /// All available tabs.
    pub fn all() -> &'static [Self] {
        &[
            Self::Status,
            Self::WiFi,
            Self::Ethernet,
            Self::Dns,
            Self::Proxy,
            Self::Firewall,
        ]
    }

    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::WiFi => "Wi-Fi",
            Self::Ethernet => "Ethernet",
            Self::Dns => "DNS",
            Self::Proxy => "Proxy",
            Self::Firewall => "Firewall",
        }
    }
}

/// Network settings UI state.
pub struct NetworkSettingsUI {
    pub settings: NetworkSettings,
    pub active_tab: NetworkSettingsTab,
    pub wifi_search: String,
    pub selected_interface: Option<String>,
    pub selected_wifi: Option<String>,
    pub editing_firewall_rule: Option<u64>,
    pub show_advanced: bool,
    pub dirty: bool,
    pub scroll_offset: f32,
}

impl NetworkSettingsUI {
    /// Create with default settings.
    pub fn new() -> Self {
        Self {
            settings: NetworkSettings::default(),
            active_tab: NetworkSettingsTab::Status,
            wifi_search: String::new(),
            selected_interface: None,
            selected_wifi: None,
            editing_firewall_rule: None,
            show_advanced: false,
            dirty: false,
            scroll_offset: 0.0,
        }
    }

    /// Switch to a different tab.
    pub fn set_tab(&mut self, tab: NetworkSettingsTab) {
        self.active_tab = tab;
        self.scroll_offset = 0.0;
    }

    /// Get filtered WiFi networks matching search.
    pub fn filtered_wifi(&self) -> Vec<&WiFiNetwork> {
        let search = self.wifi_search.to_lowercase();
        let sorted = self.settings.sorted_wifi_networks();
        if search.is_empty() {
            sorted
        } else {
            sorted
                .into_iter()
                .filter(|n| n.ssid.to_lowercase().contains(&search))
                .collect()
        }
    }

    /// Render the complete settings panel.
    pub fn render(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 20.0,
            text: "Network & Internet".to_string(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Tab bar
        let tab_y = y + 56.0;
        let mut tab_x = x + 16.0;
        for tab in NetworkSettingsTab::all() {
            let label = tab.label();
            let tw = label.len() as f32 * 8.0 + 24.0;
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: tw,
                    height: 32.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: tab_y + 8.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active { BLUE } else { SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            tab_x += tw + 4.0;
        }

        // Tab content area
        let content_y = tab_y + 44.0;
        let content_h = height - (content_y - y) - 16.0;

        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: content_y,
            width: width - 16.0,
            height: content_h,
            color: CRUST,
            corner_radii: CornerRadii::all(6.0),
        });

        // Render active tab
        let cx = x + 24.0;
        let cy = content_y + 16.0;
        let cw = width - 48.0;

        match self.active_tab {
            NetworkSettingsTab::Status => {
                self.render_status_tab(&mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::WiFi => {
                self.render_wifi_tab(&mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::Ethernet => {
                self.render_ethernet_tab(&mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::Dns => {
                self.render_dns_tab(&mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::Proxy => {
                self.render_proxy_tab(&mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::Firewall => {
                self.render_firewall_tab(&mut cmds, cx, cy, cw);
            }
        }

        cmds
    }

    /// Render the status overview tab.
    fn render_status_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        // Connection status card
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 80.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        let status = self.settings.connection_status();
        let status_color = if let Some(iface) = self.settings.default_interface() {
            iface.state.color()
        } else {
            OVERLAY0
        };

        // Status dot
        cmds.push(RenderCommand::FillRect {
            x: x + 16.0,
            y: row_y + 20.0,
            width: 12.0,
            height: 12.0,
            color: status_color,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 36.0,
            y: row_y + 16.0,
            text: status.to_string(),
            font_size: 18.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        if let Some(iface) = self.settings.default_interface() {
            cmds.push(RenderCommand::Text {
                x: x + 36.0,
                y: row_y + 44.0,
                text: format!(
                    "{} — {} — {}",
                    iface.display_name,
                    iface.ipv4.summary(),
                    iface.speed_display()
                ),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 52.0),
            });
        }

        row_y += 96.0;

        // Quick toggles
        let toggles = [
            ("Wi-Fi", self.settings.wifi_enabled),
            ("Airplane mode", self.settings.airplane_mode),
            ("Metered connection", self.settings.metered_connection),
            ("Data usage tracking", self.settings.data_usage_tracking),
        ];

        for (label, enabled) in &toggles {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 36.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 10.0,
                text: label.to_string(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Toggle indicator
            let toggle_x = x + width - 56.0;
            let toggle_bg = if *enabled { BLUE } else { SURFACE2 };
            cmds.push(RenderCommand::FillRect {
                x: toggle_x,
                y: row_y + 8.0,
                width: 40.0,
                height: 20.0,
                color: toggle_bg,
                corner_radii: CornerRadii::all(10.0),
            });

            let knob_x = if *enabled {
                toggle_x + 22.0
            } else {
                toggle_x + 2.0
            };
            cmds.push(RenderCommand::FillRect {
                x: knob_x,
                y: row_y + 10.0,
                width: 16.0,
                height: 16.0,
                color: TEXT,
                corner_radii: CornerRadii::all(8.0),
            });

            row_y += 44.0;
        }

        // Interface list
        row_y += 8.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Network interfaces".to_string(),
            font_size: 14.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 24.0;

        for iface in &self.settings.interfaces {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 48.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: format!("{} ({})", iface.display_name, iface.name),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 28.0,
                text: format!(
                    "{} — TX: {} / RX: {}",
                    iface.state.label(),
                    NetworkInterface::format_bytes(iface.tx_bytes),
                    NetworkInterface::format_bytes(iface.rx_bytes)
                ),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 80.0),
            });

            // Status dot
            cmds.push(RenderCommand::FillRect {
                x: x + width - 28.0,
                y: row_y + 18.0,
                width: 12.0,
                height: 12.0,
                color: iface.state.color(),
                corner_radii: CornerRadii::all(6.0),
            });

            row_y += 56.0;
        }

        // Hostname
        row_y += 8.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!("Hostname: {}", self.settings.hostname),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the WiFi tab.
    fn render_wifi_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        // WiFi toggle and status
        let wifi_status = if self.settings.airplane_mode {
            "Disabled (Airplane mode)"
        } else if self.settings.wifi_enabled {
            if self.settings.wifi_scanning {
                "Scanning..."
            } else {
                "Enabled"
            }
        } else {
            "Disabled"
        };

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!("Wi-Fi: {wifi_status}"),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 28.0;

        // Search bar
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        let search_text = if self.wifi_search.is_empty() {
            "Search networks...".to_string()
        } else {
            self.wifi_search.clone()
        };

        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: row_y + 8.0,
            text: search_text,
            font_size: 12.0,
            color: if self.wifi_search.is_empty() {
                OVERLAY0
            } else {
                TEXT
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });
        row_y += 44.0;

        // Available networks
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Available networks".to_string(),
            font_size: 13.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 22.0;

        let networks = self.filtered_wifi();
        if networks.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: if self.settings.wifi_enabled {
                    "No networks found".to_string()
                } else {
                    "Wi-Fi is disabled".to_string()
                },
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else {
            for net in &networks {
                let is_selected = self
                    .selected_wifi
                    .as_ref() == Some(&net.ssid);

                let bg = if is_selected { SURFACE1 } else { SURFACE0 };
                let row_h = if is_selected { 64.0 } else { 44.0 };

                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: row_h,
                    color: bg,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Signal bars
                let quality = net.signal_quality();
                let bar_x = x + 12.0;
                for bar_idx in 0u8..4 {
                    let bar_h = 4.0 + bar_idx as f32 * 3.0;
                    let bar_color = if bar_idx < quality.bars() {
                        quality.color()
                    } else {
                        SURFACE2
                    };
                    cmds.push(RenderCommand::FillRect {
                        x: bar_x + bar_idx as f32 * 5.0,
                        y: row_y + 16.0 - bar_h + 8.0,
                        width: 3.0,
                        height: bar_h,
                        color: bar_color,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // SSID
                cmds.push(RenderCommand::Text {
                    x: x + 36.0,
                    y: row_y + 8.0,
                    text: net.ssid.clone(),
                    font_size: 13.0,
                    color: if net.is_connected { GREEN } else { TEXT },
                    font_weight: if net.is_connected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(width - 140.0),
                });

                // Security and band info
                cmds.push(RenderCommand::Text {
                    x: x + 36.0,
                    y: row_y + 26.0,
                    text: format!(
                        "{} — {} — ch {}",
                        net.security.label(),
                        net.band(),
                        net.channel
                    ),
                    font_size: 10.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                // Connected/saved badge
                if net.is_connected {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 80.0,
                        y: row_y + 12.0,
                        text: "Connected".to_string(),
                        font_size: 11.0,
                        color: GREEN,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                } else if net.is_saved {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 60.0,
                        y: row_y + 12.0,
                        text: "Saved".to_string(),
                        font_size: 11.0,
                        color: SUBTEXT0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }

                // Security lock icon
                if net.security.requires_password() {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 24.0,
                        y: row_y + 12.0,
                        text: "\u{1F512}".to_string(),
                        font_size: 12.0,
                        color: net.security.color(),
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }

                row_y += row_h + 4.0;
            }
        }

        // Saved networks section
        row_y += 16.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!("Saved networks ({})", self.settings.saved_wifi.len()),
            font_size: 13.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 22.0;

        for profile in &self.settings.saved_wifi {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 36.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 10.0,
                text: format!(
                    "{} — {}{}",
                    profile.ssid,
                    profile.security.label(),
                    if profile.auto_connect {
                        " (auto)"
                    } else {
                        ""
                    }
                ),
                font_size: 12.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 80.0),
            });

            // Forget button
            cmds.push(RenderCommand::Text {
                x: x + width - 60.0,
                y: row_y + 10.0,
                text: "Forget".to_string(),
                font_size: 11.0,
                color: RED,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            row_y += 42.0;
        }
    }

    /// Render the Ethernet tab.
    fn render_ethernet_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        let eth_ifaces: Vec<&NetworkInterface> = self
            .settings
            .interfaces
            .iter()
            .filter(|i| i.interface_type == InterfaceType::Ethernet)
            .collect();

        if eth_ifaces.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y: row_y,
                text: "No Ethernet interfaces detected".to_string(),
                font_size: 14.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            return;
        }

        for iface in &eth_ifaces {
            // Interface card
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 200.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 12.0,
                text: format!("{} ({})", iface.display_name, iface.name),
                font_size: 16.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Status
            cmds.push(RenderCommand::FillRect {
                x: x + 16.0,
                y: row_y + 38.0,
                width: 8.0,
                height: 8.0,
                color: iface.state.color(),
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 30.0,
                y: row_y + 34.0,
                text: iface.state.label().to_string(),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Properties table
            let props = [
                ("MAC Address", iface.mac_address.clone()),
                ("IP Address", iface.ipv4.summary()),
                (
                    "Subnet Mask",
                    iface
                        .ipv4
                        .subnet_mask
                        .as_ref()
                        .map_or("—".to_string(), |m| m.to_string()),
                ),
                (
                    "Gateway",
                    iface
                        .ipv4
                        .gateway
                        .as_ref()
                        .map_or("—".to_string(), |g| g.to_string()),
                ),
                ("Speed", iface.speed_display()),
                ("MTU", format!("{}", iface.mtu)),
            ];

            let mut prop_y = row_y + 56.0;
            for (label, value) in &props {
                cmds.push(RenderCommand::Text {
                    x: x + 24.0,
                    y: prop_y,
                    text: format!("{label}:"),
                    font_size: 11.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                cmds.push(RenderCommand::Text {
                    x: x + 140.0,
                    y: prop_y,
                    text: value.clone(),
                    font_size: 11.0,
                    color: SUBTEXT1,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 160.0),
                });

                prop_y += 18.0;
            }

            // IP config method selector
            prop_y += 8.0;
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: prop_y,
                text: format!("IP Configuration: {}", iface.ipv4.method.label()),
                font_size: 12.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            row_y += 216.0;
        }
    }

    /// Render the DNS tab.
    fn render_dns_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "DNS Configuration".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 28.0;

        // DNS mode
        let modes = [DnsMode::Automatic, DnsMode::Manual];
        for mode in &modes {
            let is_active = *mode == self.settings.dns.mode;

            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width: width / 2.0 - 4.0,
                height: 32.0,
                color: if is_active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 8.0,
                text: match mode {
                    DnsMode::Automatic => "Automatic",
                    DnsMode::Manual => "Manual",
                }
                .to_string(),
                font_size: 13.0,
                color: if is_active { CRUST } else { TEXT },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
        }
        row_y += 44.0;

        // DNS servers
        if self.settings.dns.mode == DnsMode::Manual {
            let fields = [
                (
                    "Primary DNS",
                    self.settings
                        .dns
                        .primary
                        .as_ref()
                        .map_or(String::new(), |a| a.to_string()),
                ),
                (
                    "Secondary DNS",
                    self.settings
                        .dns
                        .secondary
                        .as_ref()
                        .map_or(String::new(), |a| a.to_string()),
                ),
            ];

            for (label, value) in &fields {
                cmds.push(RenderCommand::Text {
                    x,
                    y: row_y,
                    text: label.to_string(),
                    font_size: 12.0,
                    color: SUBTEXT1,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                row_y += 18.0;

                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 32.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: row_y + 8.0,
                    text: if value.is_empty() {
                        "e.g. 8.8.8.8".to_string()
                    } else {
                        value.clone()
                    },
                    font_size: 12.0,
                    color: if value.is_empty() { OVERLAY0 } else { TEXT },
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                row_y += 40.0;
            }
        }

        // DNS over HTTPS
        row_y += 8.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "DNS over HTTPS (DoH)".to_string(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 24.0;

        let doh_toggle_bg = if self.settings.dns.dns_over_https {
            BLUE
        } else {
            SURFACE2
        };
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width: 40.0,
            height: 20.0,
            color: doh_toggle_bg,
            corner_radii: CornerRadii::all(10.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 48.0,
            y: row_y + 2.0,
            text: if self.settings.dns.dns_over_https {
                "Enabled"
            } else {
                "Disabled"
            }
            .to_string(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        row_y += 32.0;

        // DoH providers
        if self.settings.dns.dns_over_https {
            let providers = default_doh_providers();
            for provider in &providers {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 36.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 4.0,
                    text: provider.name.clone(),
                    font_size: 13.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 20.0,
                    text: provider.description.clone(),
                    font_size: 10.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                row_y += 42.0;
            }
        }

        // Search domains
        row_y += 8.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Search domains".to_string(),
            font_size: 13.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 20.0;

        if self.settings.dns.search_domains.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: row_y,
                text: "None configured".to_string(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else {
            for domain in &self.settings.dns.search_domains {
                cmds.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: row_y,
                    text: domain.clone(),
                    font_size: 11.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                row_y += 18.0;
            }
        }

        // Cache settings
        row_y += 16.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!(
                "DNS cache: {} (max {} entries)",
                if self.settings.dns.cache_enabled {
                    "Enabled"
                } else {
                    "Disabled"
                },
                self.settings.dns.cache_size
            ),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the Proxy tab.
    fn render_proxy_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Proxy Configuration".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 28.0;

        // Proxy type buttons
        let types = [
            ProxyType::None,
            ProxyType::Http,
            ProxyType::Socks5,
            ProxyType::Auto,
        ];
        let btn_w = (width - 12.0) / types.len() as f32;

        for (i, ptype) in types.iter().enumerate() {
            let bx = x + i as f32 * (btn_w + 4.0);
            let is_active = *ptype == self.settings.proxy.proxy_type;

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: row_y,
                width: btn_w,
                height: 32.0,
                color: if is_active { BLUE } else { SURFACE0 },
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: row_y + 8.0,
                text: ptype.label().to_string(),
                font_size: 12.0,
                color: if is_active { CRUST } else { TEXT },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
        }
        row_y += 44.0;

        // Proxy details
        if self.settings.proxy.is_active() {
            if self.settings.proxy.proxy_type == ProxyType::Auto {
                cmds.push(RenderCommand::Text {
                    x,
                    y: row_y,
                    text: "PAC URL".to_string(),
                    font_size: 12.0,
                    color: SUBTEXT1,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                row_y += 18.0;

                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 32.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: row_y + 8.0,
                    text: self
                        .settings
                        .proxy
                        .pac_url
                        .as_deref()
                        .unwrap_or("https://example.com/proxy.pac")
                        .to_string(),
                    font_size: 12.0,
                    color: if self.settings.proxy.pac_url.is_some() {
                        TEXT
                    } else {
                        OVERLAY0
                    },
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 24.0),
                });
            } else {
                // Host and port
                let fields = [
                    ("Proxy host", self.settings.proxy.host.clone()),
                    ("Port", format!("{}", self.settings.proxy.port)),
                ];

                for (label, value) in &fields {
                    cmds.push(RenderCommand::Text {
                        x,
                        y: row_y,
                        text: label.to_string(),
                        font_size: 12.0,
                        color: SUBTEXT1,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                    row_y += 18.0;

                    cmds.push(RenderCommand::FillRect {
                        x,
                        y: row_y,
                        width,
                        height: 32.0,
                        color: SURFACE0,
                        corner_radii: CornerRadii::all(4.0),
                    });

                    cmds.push(RenderCommand::Text {
                        x: x + 12.0,
                        y: row_y + 8.0,
                        text: value.clone(),
                        font_size: 12.0,
                        color: TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                    row_y += 40.0;
                }

                // Authentication toggle
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 36.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 10.0,
                    text: "Requires authentication".to_string(),
                    font_size: 13.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                let auth_bg = if self.settings.proxy.requires_auth {
                    BLUE
                } else {
                    SURFACE2
                };
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 56.0,
                    y: row_y + 8.0,
                    width: 40.0,
                    height: 20.0,
                    color: auth_bg,
                    corner_radii: CornerRadii::all(10.0),
                });
            }
            row_y += 44.0;

            // Bypass list
            cmds.push(RenderCommand::Text {
                x,
                y: row_y,
                text: "Bypass proxy for:".to_string(),
                font_size: 12.0,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            row_y += 18.0;

            for addr in &self.settings.proxy.bypass_list {
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: row_y,
                    text: addr.clone(),
                    font_size: 11.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                row_y += 16.0;
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: "No proxy configured. Direct connection to the internet.".to_string(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 32.0),
            });
        }
    }

    /// Render the Firewall tab.
    fn render_firewall_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        // Firewall status
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 60.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        let fw_color = if self.settings.firewall.enabled {
            GREEN
        } else {
            RED
        };
        cmds.push(RenderCommand::FillRect {
            x: x + 16.0,
            y: row_y + 16.0,
            width: 10.0,
            height: 10.0,
            color: fw_color,
            corner_radii: CornerRadii::all(5.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 34.0,
            y: row_y + 12.0,
            text: format!(
                "Firewall: {}",
                if self.settings.firewall.enabled {
                    "Active"
                } else {
                    "Inactive"
                }
            ),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: x + 34.0,
            y: row_y + 36.0,
            text: format!(
                "{} rules ({} active) — Inbound: {} / Outbound: {}",
                self.settings.firewall.rules.len(),
                self.settings.firewall.active_rule_count(),
                self.settings.firewall.default_inbound.label(),
                self.settings.firewall.default_outbound.label()
            ),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 52.0),
        });
        row_y += 72.0;

        // Options
        let options = [
            ("Log blocked connections", self.settings.firewall.log_blocked),
            ("Block ICMP (ping)", self.settings.firewall.block_icmp),
            ("Stealth mode", self.settings.firewall.stealth_mode),
        ];

        for (label, enabled) in &options {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 36.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 10.0,
                text: label.to_string(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            let toggle_bg = if *enabled { BLUE } else { SURFACE2 };
            cmds.push(RenderCommand::FillRect {
                x: x + width - 56.0,
                y: row_y + 8.0,
                width: 40.0,
                height: 20.0,
                color: toggle_bg,
                corner_radii: CornerRadii::all(10.0),
            });

            row_y += 44.0;
        }

        // Rules list
        row_y += 8.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Firewall rules".to_string(),
            font_size: 14.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Add rule button
        cmds.push(RenderCommand::FillRect {
            x: x + width - 80.0,
            y: row_y - 4.0,
            width: 80.0,
            height: 24.0,
            color: BLUE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + width - 68.0,
            y: row_y,
            text: "+ Add rule".to_string(),
            font_size: 11.0,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 28.0;

        if self.settings.firewall.rules.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y,
                text: "No custom rules. Using default policies.".to_string(),
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else {
            for rule in &self.settings.firewall.rules {
                let rule_color = if rule.enabled {
                    SURFACE0
                } else {
                    Color::rgba(49, 50, 68, 128)
                };

                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 48.0,
                    color: rule_color,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Action badge
                cmds.push(RenderCommand::FillRect {
                    x: x + 8.0,
                    y: row_y + 8.0,
                    width: 50.0,
                    height: 18.0,
                    color: rule.action.color(),
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 14.0,
                    y: row_y + 10.0,
                    text: rule.action.label().to_string(),
                    font_size: 10.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                // Rule name and details
                cmds.push(RenderCommand::Text {
                    x: x + 66.0,
                    y: row_y + 8.0,
                    text: rule.name.clone(),
                    font_size: 12.0,
                    color: if rule.enabled { TEXT } else { OVERLAY0 },
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width - 140.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 66.0,
                    y: row_y + 26.0,
                    text: format!(
                        "{} {} port {}",
                        rule.direction.label(),
                        rule.protocol.label(),
                        rule.port_display()
                    ),
                    font_size: 10.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });

                // Enabled/disabled text
                if !rule.enabled {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 60.0,
                        y: row_y + 16.0,
                        text: "Disabled".to_string(),
                        font_size: 10.0,
                        color: OVERLAY0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }

                row_y += 56.0;
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // IPv4 tests
    #[test]
    fn test_ipv4_new() {
        let addr = Ipv4Addr::new(192, 168, 1, 1);
        assert_eq!(addr.octets, [192, 168, 1, 1]);
    }

    #[test]
    fn test_ipv4_to_string() {
        let addr = Ipv4Addr::new(10, 0, 0, 1);
        assert_eq!(addr.to_string(), "10.0.0.1");
    }

    #[test]
    fn test_ipv4_parse_valid() {
        let addr = Ipv4Addr::parse("192.168.1.100").unwrap();
        assert_eq!(addr.octets, [192, 168, 1, 100]);
    }

    #[test]
    fn test_ipv4_parse_invalid() {
        assert!(Ipv4Addr::parse("300.0.0.1").is_none());
        assert!(Ipv4Addr::parse("1.2.3").is_none());
        assert!(Ipv4Addr::parse("not.an.ip.addr").is_none());
        assert!(Ipv4Addr::parse("").is_none());
    }

    #[test]
    fn test_ipv4_private() {
        assert!(Ipv4Addr::new(10, 0, 0, 1).is_private());
        assert!(Ipv4Addr::new(172, 16, 0, 1).is_private());
        assert!(Ipv4Addr::new(172, 31, 255, 255).is_private());
        assert!(Ipv4Addr::new(192, 168, 0, 1).is_private());
        assert!(!Ipv4Addr::new(8, 8, 8, 8).is_private());
        assert!(!Ipv4Addr::new(172, 15, 0, 1).is_private());
    }

    #[test]
    fn test_ipv4_loopback() {
        assert!(Ipv4Addr::new(127, 0, 0, 1).is_loopback());
        assert!(Ipv4Addr::new(127, 255, 255, 255).is_loopback());
        assert!(!Ipv4Addr::new(128, 0, 0, 1).is_loopback());
    }

    // Ipv4Config validation
    #[test]
    fn test_config_dhcp_valid() {
        let config = Ipv4Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_static_missing_address() {
        let config = Ipv4Config {
            method: IpConfigMethod::Static,
            address: None,
            subnet_mask: Some(Ipv4Addr::new(255, 255, 255, 0)),
            ..Ipv4Config::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_static_missing_mask() {
        let config = Ipv4Config {
            method: IpConfigMethod::Static,
            address: Some(Ipv4Addr::new(192, 168, 1, 10)),
            subnet_mask: None,
            ..Ipv4Config::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_static_loopback_rejected() {
        let config = Ipv4Config {
            method: IpConfigMethod::Static,
            address: Some(Ipv4Addr::new(127, 0, 0, 1)),
            subnet_mask: Some(Ipv4Addr::new(255, 0, 0, 0)),
            ..Ipv4Config::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_summary_dhcp() {
        let config = Ipv4Config {
            method: IpConfigMethod::Dhcp,
            address: Some(Ipv4Addr::new(192, 168, 1, 100)),
            ..Ipv4Config::default()
        };
        assert!(config.summary().contains("DHCP"));
        assert!(config.summary().contains("192.168.1.100"));
    }

    // WiFi tests
    #[test]
    fn test_signal_quality_from_dbm() {
        assert_eq!(SignalQuality::from_dbm(-40), SignalQuality::Excellent);
        assert_eq!(SignalQuality::from_dbm(-55), SignalQuality::Good);
        assert_eq!(SignalQuality::from_dbm(-75), SignalQuality::Fair);
        assert_eq!(SignalQuality::from_dbm(-90), SignalQuality::Weak);
    }

    #[test]
    fn test_signal_bars() {
        assert_eq!(SignalQuality::Weak.bars(), 1);
        assert_eq!(SignalQuality::Fair.bars(), 2);
        assert_eq!(SignalQuality::Good.bars(), 3);
        assert_eq!(SignalQuality::Excellent.bars(), 4);
    }

    #[test]
    fn test_wifi_security_requires_password() {
        assert!(!WiFiSecurity::Open.requires_password());
        assert!(WiFiSecurity::WPA2Personal.requires_password());
        assert!(WiFiSecurity::WPA3Enterprise.requires_password());
        assert!(WiFiSecurity::WEP.requires_password());
    }

    #[test]
    fn test_wifi_security_strength() {
        assert_eq!(WiFiSecurity::Open.strength(), 0);
        assert_eq!(WiFiSecurity::WEP.strength(), 1);
        assert_eq!(WiFiSecurity::WPA2Personal.strength(), 2);
        assert_eq!(WiFiSecurity::WPA3Personal.strength(), 3);
    }

    #[test]
    fn test_wifi_band() {
        let net = WiFiNetwork {
            ssid: "Test".to_string(),
            bssid: "AA:BB:CC:DD:EE:FF".to_string(),
            security: WiFiSecurity::WPA2Personal,
            signal_dbm: -50,
            channel: 6,
            frequency_mhz: 2437,
            is_hidden: false,
            is_saved: false,
            is_connected: false,
        };
        assert_eq!(net.band(), "2.4 GHz");

        let net5 = WiFiNetwork {
            frequency_mhz: 5180,
            ..net
        };
        assert_eq!(net5.band(), "5 GHz");
    }

    // Proxy tests
    #[test]
    fn test_proxy_default_inactive() {
        let proxy = ProxyConfig::default();
        assert!(!proxy.is_active());
    }

    #[test]
    fn test_proxy_validate_none() {
        let proxy = ProxyConfig::default();
        assert!(proxy.validate().is_ok());
    }

    #[test]
    fn test_proxy_validate_http_missing_host() {
        let proxy = ProxyConfig {
            proxy_type: ProxyType::Http,
            host: String::new(),
            ..ProxyConfig::default()
        };
        assert!(proxy.validate().is_err());
    }

    #[test]
    fn test_proxy_validate_auto_missing_pac() {
        let proxy = ProxyConfig {
            proxy_type: ProxyType::Auto,
            pac_url: None,
            ..ProxyConfig::default()
        };
        assert!(proxy.validate().is_err());
    }

    #[test]
    fn test_proxy_validate_zero_port() {
        let proxy = ProxyConfig {
            proxy_type: ProxyType::Http,
            host: "proxy.example.com".to_string(),
            port: 0,
            ..ProxyConfig::default()
        };
        assert!(proxy.validate().is_err());
    }

    // Firewall tests
    #[test]
    fn test_firewall_default() {
        let fw = FirewallConfig::default();
        assert!(fw.enabled);
        assert_eq!(fw.default_inbound, FirewallAction::Block);
        assert_eq!(fw.default_outbound, FirewallAction::Allow);
        assert!(fw.rules.is_empty());
    }

    #[test]
    fn test_firewall_add_rule() {
        let mut fw = FirewallConfig::default();
        let rule = FirewallRule {
            id: 0,
            name: "Allow SSH".to_string(),
            enabled: true,
            action: FirewallAction::Allow,
            direction: FirewallDirection::Inbound,
            protocol: FirewallProtocol::Tcp,
            port_range: Some((22, 22)),
            remote_address: None,
            application: None,
            description: "Allow SSH connections".to_string(),
        };
        let id = fw.add_rule(rule);
        assert_eq!(id, 1);
        assert_eq!(fw.rules.len(), 1);
    }

    #[test]
    fn test_firewall_remove_rule() {
        let mut fw = FirewallConfig::default();
        let rule = FirewallRule {
            id: 0,
            name: "Test".to_string(),
            enabled: true,
            action: FirewallAction::Block,
            direction: FirewallDirection::Both,
            protocol: FirewallProtocol::Any,
            port_range: None,
            remote_address: None,
            application: None,
            description: String::new(),
        };
        let id = fw.add_rule(rule);
        assert!(fw.remove_rule(id));
        assert!(fw.rules.is_empty());
        assert!(!fw.remove_rule(999));
    }

    #[test]
    fn test_firewall_toggle_rule() {
        let mut fw = FirewallConfig::default();
        let rule = FirewallRule {
            id: 0,
            name: "Test".to_string(),
            enabled: true,
            action: FirewallAction::Allow,
            direction: FirewallDirection::Inbound,
            protocol: FirewallProtocol::Tcp,
            port_range: Some((80, 80)),
            remote_address: None,
            application: None,
            description: String::new(),
        };
        let id = fw.add_rule(rule);
        assert_eq!(fw.toggle_rule(id), Some(false));
        assert_eq!(fw.toggle_rule(id), Some(true));
        assert_eq!(fw.toggle_rule(999), None);
    }

    #[test]
    fn test_firewall_active_rules() {
        let mut fw = FirewallConfig::default();
        let r1 = FirewallRule {
            id: 0,
            name: "In1".to_string(),
            enabled: true,
            action: FirewallAction::Allow,
            direction: FirewallDirection::Inbound,
            protocol: FirewallProtocol::Tcp,
            port_range: Some((80, 80)),
            remote_address: None,
            application: None,
            description: String::new(),
        };
        let r2 = FirewallRule {
            id: 0,
            name: "Out1".to_string(),
            enabled: true,
            action: FirewallAction::Block,
            direction: FirewallDirection::Outbound,
            protocol: FirewallProtocol::Udp,
            port_range: None,
            remote_address: None,
            application: None,
            description: String::new(),
        };
        let r3 = FirewallRule {
            id: 0,
            name: "Both1".to_string(),
            enabled: false,
            action: FirewallAction::Allow,
            direction: FirewallDirection::Both,
            protocol: FirewallProtocol::Any,
            port_range: None,
            remote_address: None,
            application: None,
            description: String::new(),
        };
        fw.add_rule(r1);
        fw.add_rule(r2);
        fw.add_rule(r3);

        assert_eq!(fw.active_rules(FirewallDirection::Inbound).len(), 1);
        assert_eq!(fw.active_rules(FirewallDirection::Outbound).len(), 1);
        assert_eq!(fw.active_rule_count(), 2);
    }

    #[test]
    fn test_firewall_port_display() {
        let rule = FirewallRule {
            id: 1,
            name: "test".to_string(),
            enabled: true,
            action: FirewallAction::Allow,
            direction: FirewallDirection::Inbound,
            protocol: FirewallProtocol::Tcp,
            port_range: Some((80, 80)),
            remote_address: None,
            application: None,
            description: String::new(),
        };
        assert_eq!(rule.port_display(), "80");

        let range_rule = FirewallRule {
            port_range: Some((8000, 9000)),
            ..rule.clone()
        };
        assert_eq!(range_rule.port_display(), "8000-9000");

        let any_rule = FirewallRule {
            port_range: None,
            ..rule
        };
        assert_eq!(any_rule.port_display(), "Any");
    }

    // Network interface tests
    #[test]
    fn test_interface_speed_display() {
        let mut iface = NetworkInterface::default_ethernet();
        iface.speed_mbps = Some(1000);
        assert_eq!(iface.speed_display(), "1 Gbps");

        iface.speed_mbps = Some(100);
        assert_eq!(iface.speed_display(), "100 Mbps");

        iface.speed_mbps = None;
        assert_eq!(iface.speed_display(), "Unknown");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(NetworkInterface::format_bytes(500), "500 B");
        assert_eq!(NetworkInterface::format_bytes(1500), "1.5 KB");
        assert_eq!(NetworkInterface::format_bytes(1_500_000), "1.4 MB");
        assert_eq!(NetworkInterface::format_bytes(2_000_000_000), "1.9 GB");
    }

    // NetworkSettings tests
    #[test]
    fn test_settings_default() {
        let settings = NetworkSettings::default();
        assert_eq!(settings.interfaces.len(), 1);
        assert!(settings.wifi_enabled);
        assert!(!settings.airplane_mode);
    }

    #[test]
    fn test_airplane_mode_disables_wifi() {
        let mut settings = NetworkSettings::default();
        settings.set_airplane_mode(true);
        assert!(settings.airplane_mode);
        assert!(!settings.wifi_enabled);
    }

    #[test]
    fn test_wifi_cannot_enable_in_airplane() {
        let mut settings = NetworkSettings::default();
        settings.set_airplane_mode(true);
        settings.set_wifi_enabled(true);
        assert!(!settings.wifi_enabled);
    }

    #[test]
    fn test_disable_wifi_disconnects() {
        let mut settings = NetworkSettings::default();
        settings.interfaces.push(NetworkInterface {
            name: "wlan0".to_string(),
            display_name: "Wi-Fi".to_string(),
            interface_type: InterfaceType::WiFi,
            state: ConnectionState::Connected,
            mac_address: "11:22:33:44:55:66".to_string(),
            ipv4: Ipv4Config::default(),
            mtu: 1500,
            speed_mbps: Some(300),
            tx_bytes: 0,
            rx_bytes: 0,
            tx_packets: 0,
            rx_packets: 0,
            tx_errors: 0,
            rx_errors: 0,
            is_default: false,
            enabled: true,
        });
        settings.set_wifi_enabled(false);
        let wifi = settings
            .interfaces
            .iter()
            .find(|i| i.interface_type == InterfaceType::WiFi)
            .unwrap();
        assert_eq!(wifi.state, ConnectionState::Disconnected);
    }

    #[test]
    fn test_save_and_forget_wifi() {
        let mut settings = NetworkSettings::default();
        settings.save_wifi_profile("TestNet", WiFiSecurity::WPA2Personal);
        assert_eq!(settings.saved_wifi.len(), 1);
        assert_eq!(settings.saved_wifi[0].ssid, "TestNet");

        // Duplicate save ignored
        settings.save_wifi_profile("TestNet", WiFiSecurity::WPA2Personal);
        assert_eq!(settings.saved_wifi.len(), 1);

        assert!(settings.forget_wifi("TestNet"));
        assert!(settings.saved_wifi.is_empty());
        assert!(!settings.forget_wifi("Nonexistent"));
    }

    #[test]
    fn test_sorted_wifi_networks() {
        let mut settings = NetworkSettings::default();
        settings.wifi_networks = vec![
            WiFiNetwork {
                ssid: "Weak".to_string(),
                bssid: "AA:BB:CC:00:00:01".to_string(),
                security: WiFiSecurity::Open,
                signal_dbm: -90,
                channel: 1,
                frequency_mhz: 2412,
                is_hidden: false,
                is_saved: false,
                is_connected: false,
            },
            WiFiNetwork {
                ssid: "Connected".to_string(),
                bssid: "AA:BB:CC:00:00:02".to_string(),
                security: WiFiSecurity::WPA2Personal,
                signal_dbm: -60,
                channel: 6,
                frequency_mhz: 2437,
                is_hidden: false,
                is_saved: true,
                is_connected: true,
            },
            WiFiNetwork {
                ssid: "Strong".to_string(),
                bssid: "AA:BB:CC:00:00:03".to_string(),
                security: WiFiSecurity::WPA3Personal,
                signal_dbm: -30,
                channel: 36,
                frequency_mhz: 5180,
                is_hidden: false,
                is_saved: false,
                is_connected: false,
            },
        ];

        let sorted = settings.sorted_wifi_networks();
        assert_eq!(sorted[0].ssid, "Connected"); // Connected first
        assert_eq!(sorted[1].ssid, "Strong"); // Then by signal
        assert_eq!(sorted[2].ssid, "Weak");
    }

    #[test]
    fn test_active_interface_count() {
        let mut settings = NetworkSettings::default();
        assert_eq!(settings.active_interface_count(), 1);

        settings.interfaces[0].state = ConnectionState::Disconnected;
        assert_eq!(settings.active_interface_count(), 0);
    }

    #[test]
    fn test_connection_status_airplane() {
        let mut settings = NetworkSettings::default();
        settings.set_airplane_mode(true);
        assert_eq!(settings.connection_status(), "Airplane mode");
    }

    #[test]
    fn test_default_interface() {
        let settings = NetworkSettings::default();
        let iface = settings.default_interface().unwrap();
        assert_eq!(iface.name, "eth0");
        assert!(iface.is_default);
    }

    #[test]
    fn test_interface_mut() {
        let mut settings = NetworkSettings::default();
        let iface = settings.interface_mut("eth0").unwrap();
        iface.mtu = 9000;
        assert_eq!(settings.interfaces[0].mtu, 9000);
        assert!(settings.interface_mut("nonexistent").is_none());
    }

    // DNS tests
    #[test]
    fn test_dns_default() {
        let dns = DnsConfig::default();
        assert_eq!(dns.mode, DnsMode::Automatic);
        assert!(!dns.dns_over_https);
        assert!(dns.cache_enabled);
    }

    #[test]
    fn test_doh_providers() {
        let providers = default_doh_providers();
        assert!(!providers.is_empty());
        assert!(providers.iter().any(|p| p.name == "Cloudflare"));
    }

    // UI tests
    #[test]
    fn test_ui_new() {
        let ui = NetworkSettingsUI::new();
        assert_eq!(ui.active_tab, NetworkSettingsTab::Status);
        assert!(ui.wifi_search.is_empty());
        assert!(!ui.dirty);
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = NetworkSettingsUI::new();
        ui.scroll_offset = 100.0;
        ui.set_tab(NetworkSettingsTab::WiFi);
        assert_eq!(ui.active_tab, NetworkSettingsTab::WiFi);
        assert_eq!(ui.scroll_offset, 0.0);
    }

    #[test]
    fn test_ui_filtered_wifi() {
        let mut ui = NetworkSettingsUI::new();
        ui.settings.wifi_networks = vec![
            WiFiNetwork {
                ssid: "HomeNet".to_string(),
                bssid: "AA:BB:CC:00:00:01".to_string(),
                security: WiFiSecurity::WPA2Personal,
                signal_dbm: -50,
                channel: 6,
                frequency_mhz: 2437,
                is_hidden: false,
                is_saved: false,
                is_connected: false,
            },
            WiFiNetwork {
                ssid: "OfficeWiFi".to_string(),
                bssid: "AA:BB:CC:00:00:02".to_string(),
                security: WiFiSecurity::WPA2Enterprise,
                signal_dbm: -60,
                channel: 11,
                frequency_mhz: 2462,
                is_hidden: false,
                is_saved: false,
                is_connected: false,
            },
        ];

        assert_eq!(ui.filtered_wifi().len(), 2);

        ui.wifi_search = "home".to_string();
        assert_eq!(ui.filtered_wifi().len(), 1);
        assert_eq!(ui.filtered_wifi()[0].ssid, "HomeNet");
    }

    #[test]
    fn test_ui_render_produces_commands() {
        let ui = NetworkSettingsUI::new();
        let cmds = ui.render(0.0, 0.0, 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_tab_all() {
        let tabs = NetworkSettingsTab::all();
        assert_eq!(tabs.len(), 6);
    }

    // Interface type tests
    #[test]
    fn test_interface_type_labels() {
        assert_eq!(InterfaceType::Ethernet.label(), "Ethernet");
        assert_eq!(InterfaceType::WiFi.label(), "Wi-Fi");
        assert_eq!(InterfaceType::VPN.label(), "VPN");
    }

    #[test]
    fn test_connection_state_colors() {
        // Just verify colors don't panic
        let _c1 = ConnectionState::Connected.color();
        let _c2 = ConnectionState::Disconnected.color();
        let _c3 = ConnectionState::Limited.color();
    }

    #[test]
    fn test_wifi_scan_state() {
        let mut settings = NetworkSettings::default();
        settings.start_wifi_scan();
        assert!(settings.wifi_scanning);

        settings.update_wifi_networks(vec![]);
        assert!(!settings.wifi_scanning);
    }

    #[test]
    fn test_wifi_scan_disabled_wifi() {
        let mut settings = NetworkSettings::default();
        settings.wifi_enabled = false;
        settings.start_wifi_scan();
        assert!(!settings.wifi_scanning);
    }
}

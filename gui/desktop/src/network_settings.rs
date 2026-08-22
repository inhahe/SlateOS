//! Network settings panel for the desktop shell.
//!
//! Provides comprehensive network configuration including WiFi network
//! management, Ethernet settings, DNS configuration, proxy settings,
//! VPN profiles, and firewall rules. Communicates with the network
//! stack via IPC for actual configuration changes.

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

// ============================================================================
// Colour
// ============================================================================
//
// This panel used to keep fourteen Catppuccin Mocha constants of its own, so
// it stayed dark whatever the user chose. It now draws from the resolved
// `Palette` it is handed. Three judgements are worth stating, because none of
// them is recoverable from the code alone:
//
// * **Eight sites take the accent.** The active tab's label; four *loops* of
//   toggle switch — the status tab's four quick toggles, the three firewall
//   options, DNS-over-HTTPS, and proxy authentication; the two segmented
//   pickers (DNS mode and proxy type); and the "+ Add rule" button. Each of
//   those loops is one site in the source even where it draws several pills
//   on screen, which is why the tests below assert per *loop* and not per
//   pill. All eight mark *where you are* or *what pressing this will do*,
//   which is the accent's job.
//
// * **Five scales stay categorical**, and deliberately do not follow the
//   accent: connection state (connected / connecting / limited / off), Wi-Fi
//   security strength, signal strength, a firewall rule's allow / block / ask,
//   and whether the firewall is up at all. Every one of these is read as a
//   *measurement or a category*, and three are drawn as a column down a list —
//   a Wi-Fi list where every visible row carries its own signal bars and its
//   own lock is the strongest case there is for keeping hues distinct, because
//   the rows are compared against each other in one glance. On a Green desktop
//   an accented "connected" would be indistinguishable from "allow", and a Red
//   one would make a strong signal look like a blocked rule.
//
// * **Four labels are chosen from the fill they sit on**, not fixed. All four
//   were `p.crust`, which is right only while the thing underneath is
//   guaranteed pale. Three of them — the active segment of each picker and
//   the "+ Add rule" button — sit on the accent, so they take `on_accent()`.
//   The fourth is a firewall rule's action badge, which sits on a
//   *categorical* fill, so it takes `readable_on` of that fill directly.
//   `p.crust` happens to stay legible on all six categorical values today only
//   because Mocha's green/red/yellow are pale while Latte's are deep, so
//   crust flips with them; that is a coincidence of two palettes, not a
//   property anyone maintains, and `readable_on` is the thing that actually
//   answers the question being asked.

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
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::Connected => p.green,
            Self::Connecting => p.yellow,
            Self::Limited | Self::NoInternet => p.peach,
            Self::Disconnected | Self::Disabled => p.overlay0,
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
        Self {
            octets: [a, b, c, d],
        }
    }

    /// Parse from dotted decimal string.
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.split('.');
        // Pulling exactly four fields out of the iterator makes "an address
        // has four octets" a property of this expression, rather than a
        // length check standing three lines away from the indexing it
        // justifies — and it drops the intermediate `Vec` allocation that
        // collecting only existed to count. `split` yields at least one
        // field, so a short address fails on a missing part's `?`; the
        // trailing `next` is what rejects a fifth.
        let (a, b, c, d) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            octets: [
                a.parse().ok()?,
                b.parse().ok()?,
                c.parse().ok()?,
                d.parse().ok()?,
            ],
        })
    }

    /// Check if this is a private address.
    pub fn is_private(&self) -> bool {
        matches!(self.octets, [10, ..] | [172, 16..=31, ..] | [192, 168, ..])
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
            && addr.is_loopback()
        {
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
    pub fn color(self, p: &Palette) -> Color {
        match self.strength() {
            0 => p.red,
            1 => p.peach,
            2 => p.yellow,
            _ => p.green,
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
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::Weak => p.red,
            Self::Fair => p.peach,
            Self::Good => p.yellow,
            Self::Excellent => p.green,
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
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::Allow => p.green,
            Self::Block => p.red,
            Self::Ask => p.yellow,
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
    pub rule_ids: IdSeq,
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
            rule_ids: IdSeq::new(),
        }
    }
}

impl FirewallConfig {
    /// Add a new rule, returning its ID.
    pub fn add_rule(&mut self, mut rule: FirewallRule) -> u64 {
        let id = self.rule_ids.issue_infallible();
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
                r.enabled && (r.direction == direction || r.direction == FirewallDirection::Both)
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
        guitk::bytes::si(bytes)
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
            hostname: "slateos-desktop".to_string(),
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
            .filter(|i| {
                matches!(
                    i.state,
                    ConnectionState::Connected | ConnectionState::Limited
                )
            })
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

/// The gap between two segments of a segmented picker.
const SEGMENT_GAP: f32 = 4.0;

/// The x and width of segment `i` of `n`, in a picker row `width` wide at `x`.
///
/// Both segmented pickers in this panel lay out the same way — equal segments
/// separated by [`SEGMENT_GAP`], together filling the row exactly — but they
/// had each drifted from it in a different direction, which is why this is one
/// function rather than two open-coded expressions:
///
/// * The DNS mode picker advanced no x at all. Both of its segments were drawn
///   at the row's own x, so "Automatic" was painted and then covered outright
///   by "Manual": the option existed, was never visible, and could not be
///   picked. `draw_check`'s "drawn and never seen" rule is what surfaced it.
/// * The proxy type picker sized its segments as `(width - 12) / n` and then
///   spaced them by `btn_w + 4`, which is only consistent for one particular
///   `n`; at four segments the row already ran ~4px past its right edge, and
///   at six it would have run past by ~20.
///
/// Taking the gap out of the total *before* dividing is what makes the row fit
/// for any `n`: `n` segments have `n - 1` gaps between them.
fn segment_bounds(x: f32, width: f32, i: usize, n: usize) -> (f32, f32) {
    let n_f = n as f32;
    let seg_w = (width - SEGMENT_GAP * (n_f - 1.0)) / n_f;
    (x + i as f32 * (seg_w + SEGMENT_GAP), seg_w)
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
    pub fn render(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 20.0,
            text: "Network & Internet".to_string(),
            font_size: 22.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Tab bar
        let tab_y = y + 56.0;
        let mut tab_x = x + 16.0;
        for tab in NetworkSettingsTab::all() {
            let label = tab.label();
            let tw = text::padded_width_any_weight(label, 12.0, 13.0);
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: tw,
                    height: 32.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: tab_y + 8.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active { p.accent } else { p.subtext0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
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
            color: p.crust,
            corner_radii: CornerRadii::all(6.0),
        });

        // Render active tab
        let cx = x + 24.0;
        let cy = content_y + 16.0;
        let cw = width - 48.0;

        match self.active_tab {
            NetworkSettingsTab::Status => {
                self.render_status_tab(p, &mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::WiFi => {
                self.render_wifi_tab(p, &mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::Ethernet => {
                self.render_ethernet_tab(p, &mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::Dns => {
                self.render_dns_tab(p, &mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::Proxy => {
                self.render_proxy_tab(p, &mut cmds, cx, cy, cw);
            }
            NetworkSettingsTab::Firewall => {
                self.render_firewall_tab(p, &mut cmds, cx, cy, cw);
            }
        }

        cmds
    }

    /// Render the status overview tab.
    fn render_status_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        // Connection status card
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 80.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(8.0),
        });

        let status = self.settings.connection_status();
        let status_color = if let Some(iface) = self.settings.default_interface() {
            iface.state.color(p)
        } else {
            p.overlay0
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
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 52.0),
                overflow: TextOverflow::Ellipsis,
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
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 10.0,
                text: label.to_string(),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Toggle indicator
            let toggle_x = x + width - 56.0;
            let toggle_bg = if *enabled { p.accent } else { p.surface2 };
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
                color: p.text,
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
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 24.0;

        for iface in &self.settings.interfaces {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 48.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: format!("{} ({})", iface.display_name, iface.name),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 80.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Status dot
            cmds.push(RenderCommand::FillRect {
                x: x + width - 28.0,
                y: row_y + 18.0,
                width: 12.0,
                height: 12.0,
                color: iface.state.color(p),
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
            color: p.overlay0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render the WiFi tab.
    fn render_wifi_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
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
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 28.0;

        // Search bar
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 32.0,
            color: p.surface0,
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
                p.overlay0
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 44.0;

        // Available networks
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Available networks".to_string(),
            font_size: 13.0,
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        } else {
            for net in &networks {
                let is_selected = self.selected_wifi.as_ref() == Some(&net.ssid);

                let bg = if is_selected { p.surface1 } else { p.surface0 };
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
                        quality.color(p)
                    } else {
                        p.surface2
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
                    color: if net.is_connected { p.green } else { p.text },
                    font_weight: if net.is_connected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(width - 140.0),
                    overflow: TextOverflow::Ellipsis,
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
                    color: p.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Connected/saved badge
                if net.is_connected {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 80.0,
                        y: row_y + 12.0,
                        text: "Connected".to_string(),
                        font_size: 11.0,
                        color: p.green,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                } else if net.is_saved {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 60.0,
                        y: row_y + 12.0,
                        text: "Saved".to_string(),
                        font_size: 11.0,
                        color: p.subtext0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                // Security lock icon
                if net.security.requires_password() {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 24.0,
                        y: row_y + 12.0,
                        text: "\u{1F512}".to_string(),
                        font_size: 12.0,
                        color: net.security.color(p),
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
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
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 22.0;

        for profile in &self.settings.saved_wifi {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 36.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 10.0,
                text: format!(
                    "{} — {}{}",
                    profile.ssid,
                    profile.security.label(),
                    if profile.auto_connect { " (auto)" } else { "" }
                ),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 80.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Forget button
            cmds.push(RenderCommand::Text {
                x: x + width - 60.0,
                y: row_y + 10.0,
                text: "Forget".to_string(),
                font_size: 11.0,
                color: p.red,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            row_y += 42.0;
        }
    }

    /// Render the Ethernet tab.
    fn render_ethernet_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
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
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                color: p.surface0,
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 12.0,
                text: format!("{} ({})", iface.display_name, iface.name),
                font_size: 16.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Status
            cmds.push(RenderCommand::FillRect {
                x: x + 16.0,
                y: row_y + 38.0,
                width: 8.0,
                height: 8.0,
                color: iface.state.color(p),
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 30.0,
                y: row_y + 34.0,
                text: iface.state.label().to_string(),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                    color: p.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                cmds.push(RenderCommand::Text {
                    x: x + 140.0,
                    y: prop_y,
                    text: value.clone(),
                    font_size: 11.0,
                    color: p.subtext1,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 160.0),
                    overflow: TextOverflow::Ellipsis,
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
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            row_y += 216.0;
        }
    }

    /// Render the DNS tab.
    fn render_dns_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "DNS Configuration".to_string(),
            font_size: 16.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 28.0;

        // DNS mode
        let modes = [DnsMode::Automatic, DnsMode::Manual];
        for (i, mode) in modes.iter().enumerate() {
            let is_active = *mode == self.settings.dns.mode;
            let (bx, bw) = segment_bounds(x, width, i, modes.len());

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: row_y,
                width: bw,
                height: 32.0,
                color: if is_active { p.accent } else { p.surface0 },
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: bx + 12.0,
                y: row_y + 8.0,
                text: match mode {
                    DnsMode::Automatic => "Automatic",
                    DnsMode::Manual => "Manual",
                }
                .to_string(),
                font_size: 13.0,
                color: if is_active { p.on_accent() } else { p.text },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
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
                    color: p.subtext1,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                row_y += 18.0;

                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 32.0,
                    color: p.surface0,
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
                    color: if value.is_empty() { p.overlay0 } else { p.text },
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
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
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 24.0;

        let doh_toggle_bg = if self.settings.dns.dns_over_https {
            p.accent
        } else {
            p.surface2
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
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                    color: p.surface0,
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 4.0,
                    text: provider.name.clone(),
                    font_size: 13.0,
                    color: p.text,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 20.0,
                    text: provider.description.clone(),
                    font_size: 10.0,
                    color: p.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
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
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 20.0;

        if self.settings.dns.search_domains.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: row_y,
                text: "None configured".to_string(),
                font_size: 11.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        } else {
            for domain in &self.settings.dns.search_domains {
                cmds.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: row_y,
                    text: domain.clone(),
                    font_size: 11.0,
                    color: p.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
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
            color: p.overlay0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render the Proxy tab.
    fn render_proxy_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Proxy Configuration".to_string(),
            font_size: 16.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 28.0;

        // Proxy type buttons.
        //
        // Every variant of `ProxyType`, not a subset. The row used to offer
        // four of the six, which left a configuration the type system permits
        // — and `ProxyConfig::validate` accepts — with no segment highlighted
        // and no way back to it: a user on HTTPS or SOCKS4 saw a picker with
        // nothing selected, and touching it could only move them off.
        let types = [
            ProxyType::None,
            ProxyType::Http,
            ProxyType::Https,
            ProxyType::Socks4,
            ProxyType::Socks5,
            ProxyType::Auto,
        ];

        for (i, ptype) in types.iter().enumerate() {
            let (bx, btn_w) = segment_bounds(x, width, i, types.len());
            let is_active = *ptype == self.settings.proxy.proxy_type;

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: row_y,
                width: btn_w,
                height: 32.0,
                color: if is_active { p.accent } else { p.surface0 },
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: row_y + 8.0,
                text: ptype.label().to_string(),
                font_size: 12.0,
                color: if is_active { p.on_accent() } else { p.text },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
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
                    color: p.subtext1,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                row_y += 18.0;

                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 32.0,
                    color: p.surface0,
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
                        p.text
                    } else {
                        p.overlay0
                    },
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 24.0),
                    overflow: TextOverflow::Ellipsis,
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
                        color: p.subtext1,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                    row_y += 18.0;

                    cmds.push(RenderCommand::FillRect {
                        x,
                        y: row_y,
                        width,
                        height: 32.0,
                        color: p.surface0,
                        corner_radii: CornerRadii::all(4.0),
                    });

                    cmds.push(RenderCommand::Text {
                        x: x + 12.0,
                        y: row_y + 8.0,
                        text: value.clone(),
                        font_size: 12.0,
                        color: p.text,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                    row_y += 40.0;
                }

                // Authentication toggle
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 36.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 10.0,
                    text: "Requires authentication".to_string(),
                    font_size: 13.0,
                    color: p.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                let auth_bg = if self.settings.proxy.requires_auth {
                    p.accent
                } else {
                    p.surface2
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
                color: p.subtext1,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            row_y += 18.0;

            for addr in &self.settings.proxy.bypass_list {
                cmds.push(RenderCommand::Text {
                    x: x + 12.0,
                    y: row_y,
                    text: addr.clone(),
                    font_size: 11.0,
                    color: p.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                row_y += 16.0;
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: "No proxy configured. Direct connection to the internet.".to_string(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// Render the Firewall tab.
    fn render_firewall_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        // Firewall status
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 60.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(8.0),
        });

        let fw_color = if self.settings.firewall.enabled {
            p.green
        } else {
            p.red
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
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 52.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 72.0;

        // Options
        let options = [
            (
                "Log blocked connections",
                self.settings.firewall.log_blocked,
            ),
            ("Block ICMP (ping)", self.settings.firewall.block_icmp),
            ("Stealth mode", self.settings.firewall.stealth_mode),
        ];

        for (label, enabled) in &options {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 36.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 10.0,
                text: label.to_string(),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            let toggle_bg = if *enabled { p.accent } else { p.surface2 };
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
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Add rule button
        cmds.push(RenderCommand::FillRect {
            x: x + width - 80.0,
            y: row_y - 4.0,
            width: 80.0,
            height: 24.0,
            color: p.accent,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + width - 68.0,
            y: row_y,
            text: "+ Add rule".to_string(),
            font_size: 11.0,
            color: p.on_accent(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 28.0;

        if self.settings.firewall.rules.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y,
                text: "No custom rules. Using default policies.".to_string(),
                font_size: 12.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        } else {
            for rule in &self.settings.firewall.rules {
                let rule_color = if rule.enabled {
                    p.surface0
                } else {
                    Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 128)
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
                    color: rule.action.color(p),
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 14.0,
                    y: row_y + 10.0,
                    text: rule.action.label().to_string(),
                    font_size: 10.0,
                    color: readable_on(rule.action.color(p)),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Rule name and details
                cmds.push(RenderCommand::Text {
                    x: x + 66.0,
                    y: row_y + 8.0,
                    text: rule.name.clone(),
                    font_size: 12.0,
                    color: if rule.enabled { p.text } else { p.overlay0 },
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width - 140.0),
                    overflow: TextOverflow::Ellipsis,
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
                    color: p.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                // Enabled/disabled text
                if !rule.enabled {
                    cmds.push(RenderCommand::Text {
                        x: x + width - 60.0,
                        y: row_y + 16.0,
                        text: "Disabled".to_string(),
                        font_size: 10.0,
                        color: p.overlay0,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }

                row_y += 56.0;
            }
        }
    }
}

impl Default for NetworkSettingsUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::draw_check::assert_nothing_is_drawn_and_never_seen;
    use crate::palette_check::assert_drawn_from;

    /// An address has exactly four fields. The count and the reads used to be
    /// separate statements; these are the inputs that tell them apart.
    #[test]
    fn an_address_is_four_fields_and_a_field_is_one_octet() {
        assert_eq!(
            Ipv4Addr::parse("192.168.1.1").map(|a| a.octets),
            Some([192, 168, 1, 1]),
        );
        assert_eq!(Ipv4Addr::parse("0.0.0.0").map(|a| a.octets), Some([0; 4]));
        assert_eq!(
            Ipv4Addr::parse("255.255.255.255").map(|a| a.octets),
            Some([255; 4]),
        );

        for bad in [
            "",                  // no fields at all
            "1",                 // one
            "1.2.3",             // three
            "1.2.3.4.5",         // five
            "1.2.3.",            // four, the last empty
            ".1.2.3",            // four, the first empty
            "1.2.3.256",         // in range for the parse, not for a u8
            "1.2.3.-1",          // signed
            "1.2.3.4 ",          // trailing space is not whitespace-tolerated
            "1.2.3.0x4",         // not decimal
            "192.168.1.1.1.1.1", // seven
        ] {
            assert!(
                Ipv4Addr::parse(bad).is_none(),
                "{bad:?} parsed as an address",
            );
        }
    }

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
        assert_eq!(NetworkInterface::format_bytes(1500), "1.5 kB");
        assert_eq!(NetworkInterface::format_bytes(1_500_000), "1.5 MB");
        assert_eq!(NetworkInterface::format_bytes(2_000_000_000), "2.0 GB");
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
        let cmds = ui.render(&Palette::for_mode(false), 0.0, 0.0, 600.0, 800.0);
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
        let p = Palette::for_mode(false);
        let _c1 = ConnectionState::Connected.color(&p);
        let _c2 = ConnectionState::Disconnected.color(&p);
        let _c3 = ConnectionState::Limited.color(&p);
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

    // ========================================================================
    // The palette
    // ========================================================================

    /// A panel wound up so that every colour-bearing branch actually draws.
    ///
    /// Every control here is coloured by a boolean, so a fixture that only
    /// renders the `true` arm proves nothing about the `false` one: both
    /// halves of every switch are on screen at once. The Wi-Fi list carries
    /// all four signal qualities and a spread of securities including the open
    /// one that draws no lock; the interface list carries every connection
    /// state; the firewall carries all three actions and one disabled rule,
    /// which is the only thing that draws the half-alpha row.
    /// One interface per connection state, so `ConnectionState::color` is
    /// exercised across its whole range in a single render.
    fn every_connection_state() -> Vec<NetworkInterface> {
        [
            ConnectionState::Connected,
            ConnectionState::Connecting,
            ConnectionState::Limited,
            ConnectionState::NoInternet,
            ConnectionState::Disconnected,
            ConnectionState::Disabled,
        ]
        .iter()
        .enumerate()
        .map(|(i, state)| {
            let mut iface = NetworkInterface::default_ethernet();
            iface.name = format!("eth{i}");
            iface.display_name = format!("Ethernet {i}");
            iface.state = *state;
            iface.is_default = i == 0;
            iface
        })
        .collect()
    }

    /// One network per signal quality, with securities spread so that the open
    /// one (which draws no lock at all) and all four strength bands appear.
    fn every_visible_network() -> Vec<WiFiNetwork> {
        [
            ("strong", WiFiSecurity::WPA3Personal, -40, true, true),
            ("good", WiFiSecurity::WPA2Personal, -60, false, true),
            ("fair", WiFiSecurity::WEP, -75, false, false),
            ("weak", WiFiSecurity::Open, -90, false, false),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (ssid, security, dbm, connected, saved))| WiFiNetwork {
            ssid: ssid.to_string(),
            bssid: format!("00:11:22:33:44:{i:02}"),
            security,
            signal_dbm: dbm,
            channel: 1 + i as u32,
            frequency_mhz: if i % 2 == 0 { 2412 } else { 5180 },
            is_hidden: false,
            is_saved: saved,
            is_connected: connected,
        })
        .collect()
    }

    /// All three firewall actions, the last rule disabled so that the
    /// half-alpha row gets drawn at all.
    fn every_firewall_action() -> Vec<FirewallRule> {
        [
            FirewallAction::Allow,
            FirewallAction::Block,
            FirewallAction::Ask,
        ]
        .iter()
        .enumerate()
        .map(|(i, action)| FirewallRule {
            id: 0,
            name: format!("rule {i}"),
            enabled: i < 2,
            action: *action,
            direction: FirewallDirection::Inbound,
            protocol: FirewallProtocol::Tcp,
            port_range: Some((80, 80)),
            remote_address: None,
            application: None,
            description: format!("rule {i} description"),
        })
        .collect()
    }

    fn wound(tab: NetworkSettingsTab, up: bool) -> NetworkSettingsUI {
        let mut ui = NetworkSettingsUI::new();
        ui.active_tab = tab;

        ui.settings.interfaces = every_connection_state();
        ui.settings.wifi_networks = every_visible_network();
        // One row selected, because a selected row is a different fill and a
        // taller box than an unselected one.
        ui.selected_wifi = Some("good".to_string());

        ui.settings.saved_wifi = vec![SavedWiFiProfile {
            ssid: "good".to_string(),
            ..SavedWiFiProfile::default()
        }];

        ui.settings.wifi_enabled = up;
        ui.settings.airplane_mode = !up;
        ui.settings.metered_connection = up;
        ui.settings.data_usage_tracking = !up;

        ui.settings.dns.mode = if up {
            DnsMode::Manual
        } else {
            DnsMode::Automatic
        };
        ui.settings.dns.primary = Some(Ipv4Addr::new(1, 1, 1, 1));
        ui.settings.dns.secondary = Some(Ipv4Addr::new(9, 9, 9, 9));
        ui.settings.dns.search_domains = vec!["lan".to_string()];
        ui.settings.dns.dns_over_https = up;
        ui.settings.dns.doh_url = Some("https://1.1.1.1/dns-query".to_string());

        ui.settings.proxy.proxy_type = if up { ProxyType::Http } else { ProxyType::None };
        ui.settings.proxy.host = "proxy.lan".to_string();
        ui.settings.proxy.requires_auth = up;
        ui.settings.proxy.username = Some("u".to_string());

        ui.settings.firewall.enabled = up;
        ui.settings.firewall.log_blocked = up;
        ui.settings.firewall.block_icmp = !up;
        ui.settings.firewall.stealth_mode = up;
        ui.settings.firewall.rules.clear();
        for rule in every_firewall_action() {
            ui.settings.firewall.add_rule(rule);
        }
        ui
    }

    /// Every state the panel can be in, so no branch escapes the sweep below.
    fn every_state() -> Vec<(NetworkSettingsUI, String)> {
        let mut out = Vec::new();
        for tab in NetworkSettingsTab::all() {
            for up in [false, true] {
                out.push((
                    wound(*tab, up),
                    format!("network panel (tab={tab:?}, up={up})"),
                ));
            }
        }
        // Every empty-list caption: each draws a line no populated panel does.
        for tab in NetworkSettingsTab::all() {
            let mut bare = NetworkSettingsUI::new();
            bare.active_tab = *tab;
            bare.settings.interfaces.clear();
            bare.settings.wifi_networks.clear();
            bare.settings.saved_wifi.clear();
            bare.settings.firewall.rules.clear();
            out.push((bare, format!("network panel ({tab:?}, nothing to show)")));
        }
        // Wi-Fi off draws a different empty caption from Wi-Fi on and empty.
        let mut off = NetworkSettingsUI::new();
        off.active_tab = NetworkSettingsTab::WiFi;
        off.settings.wifi_enabled = false;
        off.settings.wifi_networks.clear();
        out.push((off, "network panel (WiFi, radio off)".to_string()));
        // A proxy set to Auto draws the PAC row nothing else does.
        let mut pac = NetworkSettingsUI::new();
        pac.active_tab = NetworkSettingsTab::Proxy;
        pac.settings.proxy.proxy_type = ProxyType::Auto;
        pac.settings.proxy.pac_url = Some("http://wpad/wpad.dat".to_string());
        out.push((pac, "network panel (Proxy, auto/PAC)".to_string()));
        out
    }

    fn render(ui: &NetworkSettingsUI, p: &Palette) -> Vec<RenderCommand> {
        ui.render(p, 0.0, 0.0, 600.0, 800.0)
    }

    /// The membership sweep: nothing the panel draws is outside its palette.
    ///
    /// Every constant this module used to hold was a Catppuccin *Mocha* value,
    /// so the light render is where a survivor gives itself away — Latte does
    /// not contain it, and the failure names the colour back.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (ui, what) in every_state() {
                assert_drawn_from(&p, &render(&ui, &p), &[], &format!("{what}, light={light}"));
            }
        }
    }

    /// Nothing is painted and then erased before anyone could see it.
    #[test]
    fn the_panel_draws_nothing_that_is_immediately_erased() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (ui, what) in every_state() {
                assert_nothing_is_drawn_and_never_seen(
                    &render(&ui, &p),
                    &format!("{what}, light={light}"),
                );
            }
        }
    }

    // -- Extractors, one per class of control the accent is supposed to reach --

    /// The tab strip's labels, in the order they are drawn.
    ///
    /// The strip is the only thing drawn at y 64 in a 13pt face; every tab
    /// body starts at y 116 or below.
    fn tab_labels(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y: 64.0,
                    font_size: 13.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every switch pill: the 40x20 fully-rounded fill each toggle row draws.
    ///
    /// Deliberately *not* "any fill wider than it is tall with a full radius" —
    /// that shape also matches a firewall action badge (50x18) if its radius is
    /// ever rounded up, and the size is what the four toggle loops actually
    /// share. The knob is 16x16, so it is excluded by width alone.
    fn switch_fills(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width: 40.0,
                    height: 20.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The segmented pickers' fills: the 32-tall buttons inside the content.
    ///
    /// Seven things in this module are 32 tall, and neither the height nor the
    /// width alone tells the two pickers from the other five. Four of the five
    /// — the Wi-Fi search bar and the DNS-server, PAC-URL and proxy-host text
    /// fields — span the full content width, so the width bound excludes them.
    /// The fifth is the *active tab's own pill*, which is `tw` wide, a padded
    /// text width, and therefore passes any width bound a segment passes. Only
    /// its position separates it: the tab strip is the one thing drawn above
    /// the content well, which starts at y 100 with the tab bodies at 116.
    ///
    /// Getting this wrong would be a false negative, not a false positive:
    /// the same pattern is subtracted in [`colors_apart_from_the_controls`],
    /// so a pill wrongly counted as a segment is a colour silently removed
    /// from the frozen union.
    fn segment_fills(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    y,
                    width,
                    height: 32.0,
                    color,
                    ..
                } if *y > 100.0 && *width < 400.0 => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Whether `text` is a segmented picker's own label.
    ///
    /// `IpConfigMethod::Static` is also labelled "Manual", but it is only ever
    /// drawn interpolated into "IP Configuration: Manual", so exact equality
    /// does not reach it.
    fn is_picker_label(text: &str) -> bool {
        matches!(
            text,
            "Automatic"
                | "Manual"
                | "No proxy"
                | "HTTP"
                | "HTTPS"
                | "SOCKS4"
                | "SOCKS5"
                | "Auto-detect"
        )
    }

    /// The fill of the button labelled `label`, and that label's own colour.
    fn button(cmds: &[RenderCommand], label: &str) -> (Color, Color) {
        let mut fill = None;
        for c in cmds {
            match c {
                RenderCommand::FillRect { color, .. } => fill = Some(*color),
                RenderCommand::Text { text, color, .. } if text == label => {
                    return (fill.expect("a button's fill precedes its label"), *color);
                }
                _ => {}
            }
        }
        panic!("no button labelled {label:?} was drawn");
    }

    /// Every colour the panel draws that no control above claimed.
    ///
    /// This is the frozen half: an `assert_eq!` over it fails if *anything*
    /// that is not one of the named controls moves with the accent, including
    /// sites nobody thought to name. The exclusions are exactly the extractors
    /// above and the three `on_accent()` labels, and nothing more.
    ///
    /// A firewall rule's action badge label is deliberately *kept*. It is
    /// `readable_on` of a categorical fill, which does not move with the
    /// accent, so it belongs here and its presence is real coverage.
    fn colors_apart_from_the_controls(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter(|c| {
                !matches!(
                    c,
                    RenderCommand::Text {
                        y: 64.0,
                        font_size: 13.0,
                        ..
                    } | RenderCommand::FillRect {
                        width: 40.0,
                        height: 20.0,
                        ..
                    } | RenderCommand::FillRect {
                        width: 80.0,
                        height: 24.0,
                        ..
                    }
                ) && !matches!(
                    c,
                    RenderCommand::FillRect { y, width, height: 32.0, .. }
                        if *y > 100.0 && *width < 400.0
                ) && !matches!(
                    c,
                    RenderCommand::Text { text, .. }
                        if text == "+ Add rule" || is_picker_label(text)
                )
            })
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every control that offers something follows the accent — each proved
    /// separately.
    ///
    /// Eight sites means eight `assert_ne!`s. Over their union one moving
    /// control would hide seven frozen ones, which is the failure the earlier
    /// modules in this conversion established.
    ///
    /// The three `on_accent()` labels are deliberately *not* among the eight.
    /// An `assert_ne!` on them would be a bug in the test rather than a check:
    /// `on_accent()` is `readable_on(accent)`, and every accent on offer is
    /// pale enough that all fourteen resolve to the same near-black, so
    /// correct code draws the same label under any two accents. What separates
    /// `p.on_accent()` from a frozen `p.crust` is the *mode*, which is what
    /// [`each_label_is_legible_on_the_fill_beneath_it`] asserts instead.
    #[test]
    fn every_control_that_offers_something_follows_the_accent() {
        let mut a = Palette::for_mode(false);
        a.accent = appearance::MAUVE;
        let mut b = Palette::for_mode(false);
        b.accent = appearance::TEAL;

        // Walk every tab as the active one: the tab label's colour is chosen
        // by a boolean, so a fixture pinned to one tab leaves five unproven.
        for tab in NetworkSettingsTab::all() {
            let ui = wound(*tab, true);
            let x = render(&ui, &a);
            let y = render(&ui, &b);

            assert_eq!(tab_labels(&x).len(), 6, "six tabs are labelled");
            assert_ne!(
                tab_labels(&x),
                tab_labels(&y),
                "the {tab:?} tab's label did not move with the accent"
            );

            match tab {
                NetworkSettingsTab::Status => {
                    assert_eq!(switch_fills(&x).len(), 4, "four quick toggles");
                    assert_ne!(
                        switch_fills(&x),
                        switch_fills(&y),
                        "the status tab's quick toggles did not move with the accent"
                    );
                }
                NetworkSettingsTab::Dns => {
                    // Two sites here, and they must be split: the picker alone
                    // moving would make a union of the two differ even with
                    // the DoH switch frozen.
                    assert_eq!(segment_fills(&x).len(), 2, "two DNS mode segments");
                    assert_ne!(
                        segment_fills(&x),
                        segment_fills(&y),
                        "the DNS mode picker did not move with the accent"
                    );
                    assert_eq!(switch_fills(&x).len(), 1, "one DoH switch");
                    assert_ne!(
                        switch_fills(&x),
                        switch_fills(&y),
                        "the DNS-over-HTTPS switch did not move with the accent"
                    );
                }
                NetworkSettingsTab::Proxy => {
                    assert_eq!(segment_fills(&x).len(), 6, "six proxy type segments");
                    assert_ne!(
                        segment_fills(&x),
                        segment_fills(&y),
                        "the proxy type picker did not move with the accent"
                    );
                    assert_eq!(switch_fills(&x).len(), 1, "one authentication switch");
                    assert_ne!(
                        switch_fills(&x),
                        switch_fills(&y),
                        "the proxy authentication switch did not move with the accent"
                    );
                }
                NetworkSettingsTab::Firewall => {
                    assert_eq!(switch_fills(&x).len(), 3, "three firewall options");
                    assert_ne!(
                        switch_fills(&x),
                        switch_fills(&y),
                        "a firewall option's switch did not move with the accent"
                    );
                    let (fa, _) = button(&x, "+ Add rule");
                    let (fb, _) = button(&y, "+ Add rule");
                    assert_ne!(
                        (fa.r, fa.g, fa.b),
                        (fb.r, fb.g, fb.b),
                        "\"+ Add rule\" did not move with the accent"
                    );
                }
                NetworkSettingsTab::WiFi | NetworkSettingsTab::Ethernet => {}
            }

            assert_eq!(
                colors_apart_from_the_controls(&x),
                colors_apart_from_the_controls(&y),
                "something that is not a control moved with the accent \
                 (tab={tab:?}) — connection state, Wi-Fi security, signal \
                 strength, a rule's allow/block/ask and whether the firewall \
                 is up are all categories, and a category read against its \
                 neighbours in a list must not be the accent"
            );
        }
    }

    /// The panel's own two surfaces are the palette's, in both modes.
    ///
    /// This is the one thing the membership sweep structurally cannot check.
    /// `assert_drawn_from` allows `0x11111B` and `0xEFF1F5` at any alpha,
    /// because those are the two answers [`appearance::readable_on`] can give
    /// and a legitimately-converted foreground will be one of them. But
    /// `0x11111B` is *also* Mocha's `crust` — so putting the literal back where
    /// `p.crust` belongs produces a render the sweep is obliged to accept.
    ///
    /// Membership is the wrong question for these two. They are not "some
    /// palette colour", they are one specific role each, so the test names the
    /// role and asserts equality — which also fails in *dark* mode if the role
    /// is wrong, where a membership check could only ever fail in light.
    #[test]
    fn the_panels_own_surfaces_come_from_the_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let cmds = render(&wound(NetworkSettingsTab::Status, true), &p);

            let backdrop = cmds.iter().find_map(|c| match c {
                RenderCommand::FillRect {
                    x: 0.0,
                    y: 0.0,
                    width: 600.0,
                    height: 800.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            });
            assert_eq!(
                backdrop,
                Some(p.base),
                "the panel's backdrop is not p.base (light={light})"
            );

            // The well the tab content sits in: the full-width inset at x 8.
            let well = cmds.iter().find_map(|c| match c {
                RenderCommand::FillRect {
                    x: 8.0,
                    width: 584.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            });
            assert_eq!(
                well,
                Some(p.crust),
                "the content well is not p.crust (light={light})"
            );
        }
    }

    /// Each label that sits on a coloured fill is picked *for that fill*.
    ///
    /// The accent test above cannot reach this. Every accent on offer is pale,
    /// so [`appearance::readable_on`] answers the same near-black for all of
    /// them and an `assert_ne!` between two accents would fail on correct
    /// code. What separates a chosen label from a hard-coded `p.crust` is the
    /// *mode*: Latte's `crust` is near-white, which on a pale accent is
    /// illegible.
    ///
    /// The firewall action badge is here for a different reason. Its fill is
    /// *categorical*, not the accent, and `p.crust` stays legible on all six
    /// of green/red/yellow across both modes purely because Mocha's are pale
    /// while Latte's are deep — a coincidence of the two palettes that nobody
    /// maintains. Asserting `readable_on` of the badge's own fill is what
    /// makes that legibility a property rather than a coincidence.
    #[test]
    fn each_label_is_legible_on_the_fill_beneath_it() {
        for light in [false, true] {
            for accent in [
                appearance::BLUE,
                appearance::GREEN,
                appearance::RED,
                appearance::YELLOW,
                appearance::MAUVE,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let want = appearance::readable_on(accent);

                // "+ Add rule": an accent fill and its label.
                let fw = render(&wound(NetworkSettingsTab::Firewall, true), &p);
                let (fill, text) = button(&fw, "+ Add rule");
                assert_eq!(
                    (fill.r, fill.g, fill.b),
                    (accent.r, accent.g, accent.b),
                    "\"+ Add rule\" is not filled with the accent (light={light})"
                );
                assert_eq!(
                    (text.r, text.g, text.b),
                    (want.r, want.g, want.b),
                    "\"+ Add rule\"'s label is not chosen for its own fill \
                     (light={light}); a fixed colour is legible on one mode's \
                     accents and not the other's"
                );

                // Each picker's *active* segment label, likewise.
                for (tab, label) in [
                    (NetworkSettingsTab::Dns, "Manual"),
                    (NetworkSettingsTab::Proxy, "HTTP"),
                ] {
                    let cmds = render(&wound(tab, true), &p);
                    let (fill, text) = button(&cmds, label);
                    assert_eq!(
                        (fill.r, fill.g, fill.b),
                        (accent.r, accent.g, accent.b),
                        "the active {label:?} segment is not filled with the \
                         accent (light={light})"
                    );
                    assert_eq!(
                        (text.r, text.g, text.b),
                        (want.r, want.g, want.b),
                        "the active {label:?} segment's label is not chosen \
                         for its own fill (light={light})"
                    );
                }

                // A firewall rule's action badge: a *categorical* fill, so the
                // expected label is readable_on of that, not of the accent.
                for action in [
                    FirewallAction::Allow,
                    FirewallAction::Block,
                    FirewallAction::Ask,
                ] {
                    let (fill, text) = button(&fw, action.label());
                    let badge = action.color(&p);
                    assert_eq!(
                        (fill.r, fill.g, fill.b),
                        (badge.r, badge.g, badge.b),
                        "the {action:?} badge is not filled with its own \
                         category's colour (light={light})"
                    );
                    let want_badge = appearance::readable_on(badge);
                    assert_eq!(
                        (text.r, text.g, text.b),
                        (want_badge.r, want_badge.g, want_badge.b),
                        "the {action:?} badge's label is not chosen for its \
                         own fill (light={light}); p.crust is legible on all \
                         six of these only by coincidence of the two palettes"
                    );
                }
            }
        }
    }

    /// Every categorical scale stays tellable apart, under every accent and in
    /// both modes.
    ///
    /// These are drawn down a list — a Wi-Fi row's signal bars and lock sit
    /// beside the next row's, and a firewall rule's badge beside the next
    /// rule's — so two values sharing a colour do not merely confuse a learnt
    /// code, they make a blocked rule look like an allowed one in the same
    /// glance. Several of the hues involved are themselves selectable accents,
    /// which is why the accent has to be varied and not merely defaulted.
    #[test]
    fn every_category_stays_distinct_under_every_accent() {
        for light in [false, true] {
            for accent in [
                appearance::BLUE,
                appearance::GREEN,
                appearance::RED,
                appearance::YELLOW,
                appearance::PEACH,
                appearance::MAUVE,
                appearance::TEAL,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let what = format!("light={light}, accent={accent:?}");

                // Connection state: connected / connecting / trouble / off are
                // four answers and must read as four.
                let states = [
                    ConnectionState::Connected,
                    ConnectionState::Connecting,
                    ConnectionState::Limited,
                    ConnectionState::Disconnected,
                ];
                for (i, s1) in states.iter().enumerate() {
                    for s2 in states.iter().skip(i + 1) {
                        assert_ne!(
                            s1.color(&p),
                            s2.color(&p),
                            "{s1:?} and {s2:?} are the same colour ({what})"
                        );
                    }
                }

                // Signal strength is a four-step measurement; two steps that
                // share a colour turn four bars of information into three.
                let quals = [
                    SignalQuality::Weak,
                    SignalQuality::Fair,
                    SignalQuality::Good,
                    SignalQuality::Excellent,
                ];
                for (i, q1) in quals.iter().enumerate() {
                    for q2 in quals.iter().skip(i + 1) {
                        assert_ne!(
                            q1.color(&p),
                            q2.color(&p),
                            "{q1:?} and {q2:?} signal are the same colour ({what})"
                        );
                    }
                }

                // Security strength, by band rather than by variant: WPA2 and
                // WPA3 are deliberately the same strength and so deliberately
                // the same colour, which is why this walks one member of each
                // of the four bands.
                let secs = [
                    WiFiSecurity::Open,
                    WiFiSecurity::WEP,
                    WiFiSecurity::WPA2Personal,
                    WiFiSecurity::WPA3Personal,
                ];
                for (i, s1) in secs.iter().enumerate() {
                    for s2 in secs.iter().skip(i + 1) {
                        assert_ne!(
                            s1.color(&p),
                            s2.color(&p),
                            "{s1:?} and {s2:?} are the same colour ({what})"
                        );
                    }
                }

                // Allow / block / ask is the one where a collision is a
                // security misreading rather than a cosmetic one.
                let acts = [
                    FirewallAction::Allow,
                    FirewallAction::Block,
                    FirewallAction::Ask,
                ];
                for (i, a1) in acts.iter().enumerate() {
                    for a2 in acts.iter().skip(i + 1) {
                        assert_ne!(
                            a1.color(&p),
                            a2.color(&p),
                            "{a1:?} and {a2:?} are the same colour ({what})"
                        );
                    }
                }
            }
        }
    }

    /// A category is not the accent — proved by moving the accent onto it.
    ///
    /// The distinctness test above is necessary but not sufficient: a scale
    /// whose values were all rewritten to `p.accent` would collapse and be
    /// caught, but a scale where only *one* value became the accent would
    /// still be pairwise-distinct under most accents. This asserts the
    /// stronger property directly — every one of these colours is the same
    /// under two different accents, because none of them is the accent.
    #[test]
    fn no_category_follows_the_accent() {
        let mut a = Palette::for_mode(false);
        a.accent = appearance::MAUVE;
        let mut b = Palette::for_mode(false);
        b.accent = appearance::TEAL;

        for s in [
            ConnectionState::Connected,
            ConnectionState::Connecting,
            ConnectionState::Limited,
            ConnectionState::NoInternet,
            ConnectionState::Disconnected,
            ConnectionState::Disabled,
        ] {
            assert_eq!(s.color(&a), s.color(&b), "{s:?} follows the accent");
        }
        for q in [
            SignalQuality::Weak,
            SignalQuality::Fair,
            SignalQuality::Good,
            SignalQuality::Excellent,
        ] {
            assert_eq!(q.color(&a), q.color(&b), "{q:?} signal follows the accent");
        }
        for s in [
            WiFiSecurity::Open,
            WiFiSecurity::WEP,
            WiFiSecurity::WPA,
            WiFiSecurity::WPA2Personal,
            WiFiSecurity::WPA2Enterprise,
            WiFiSecurity::WPA3Personal,
            WiFiSecurity::WPA3Enterprise,
        ] {
            assert_eq!(s.color(&a), s.color(&b), "{s:?} follows the accent");
        }
        for act in [
            FirewallAction::Allow,
            FirewallAction::Block,
            FirewallAction::Ask,
        ] {
            assert_eq!(act.color(&a), act.color(&b), "{act:?} follows the accent");
        }
    }

    /// A disabled firewall rule's row is its enabled row, made translucent.
    ///
    /// The row was `Color::rgba(49, 50, 68, 128)` — Mocha's `surface0` at half
    /// alpha, written out by hand. The membership sweep cannot see the
    /// difference, because it compares RGB and ignores alpha by design, so
    /// this is the test that says the RGB is a *role* and the alpha is not.
    #[test]
    fn a_disabled_rule_is_the_enabled_row_made_translucent() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let cmds = render(&wound(NetworkSettingsTab::Firewall, true), &p);
            let rows: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        height: 48.0,
                        color,
                        ..
                    } => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(rows.len(), 3, "three rules are drawn (light={light})");
            for row in &rows[..2] {
                assert_eq!(*row, p.surface0, "an enabled rule's row is not p.surface0");
            }
            let off = rows[2];
            assert_eq!(
                (off.r, off.g, off.b),
                (p.surface0.r, p.surface0.g, p.surface0.b),
                "a disabled rule's row is not p.surface0 underneath its alpha \
                 (light={light}); the membership sweep ignores alpha, so a \
                 hand-written Mocha rgba survives it"
            );
            assert_eq!(off.a, 128, "a disabled rule's row is not half-alpha");
        }
    }

    /// Every segment of a picker is somewhere the user can see and reach.
    ///
    /// Both bugs this asserts against were live in the panel, and the palette
    /// conversion is only how they came to light — neither has anything to do
    /// with colour:
    ///
    /// * The DNS mode picker advanced no x, so both segments were drawn at the
    ///   row's own x and "Automatic" was covered outright by "Manual". It was
    ///   on screen in the sense that a command existed for it, and invisible
    ///   and unclickable in every sense that matters.
    /// * The proxy type picker sized segments as if there were no gaps between
    ///   them, so the row ran past its own right edge — by ~4px at the four
    ///   segments it then had, and by ~20 at the six it has now.
    ///
    /// `draw_check` catches the first as a special case of "drawn and never
    /// seen", but only because the overlap happened to be exact; two segments
    /// half on top of each other would slip past it, since partial overlap is
    /// deliberately exempt there. So this checks the property directly.
    #[test]
    fn no_picker_segment_hides_another_or_leaves_the_row() {
        let p = Palette::for_mode(false);
        for (tab, want) in [(NetworkSettingsTab::Dns, 2), (NetworkSettingsTab::Proxy, 6)] {
            let cmds = render(&wound(tab, true), &p);
            let segs: Vec<(f32, f32)> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height: 32.0,
                        ..
                    } if *y > 100.0 && *width < 400.0 => Some((*x, *width)),
                    _ => None,
                })
                .collect();
            assert_eq!(segs.len(), want, "{tab:?} draws {want} segments");

            // Every variant of the enum is offered, so no reachable setting is
            // one the picker cannot show as chosen.
            if tab == NetworkSettingsTab::Proxy {
                for t in [
                    ProxyType::None,
                    ProxyType::Http,
                    ProxyType::Https,
                    ProxyType::Socks4,
                    ProxyType::Socks5,
                    ProxyType::Auto,
                ] {
                    let mut ui = wound(tab, true);
                    ui.settings.proxy.proxy_type = t;
                    let c = render(&ui, &p);
                    let lit = segment_fills(&c)
                        .into_iter()
                        .filter(|f| *f == p.accent)
                        .count();
                    assert_eq!(lit, 1, "{t:?} lights exactly one segment");
                }
            }

            // The content row starts at cx = 24 and is cw = 600 - 48 wide.
            let (row_x, row_w) = (24.0_f32, 552.0_f32);
            for (i, (sx, sw)) in segs.iter().enumerate() {
                assert!(
                    *sx >= row_x && sx + sw <= row_x + row_w + 0.01,
                    "{tab:?} segment {i} spans {sx}..{} , outside the row \
                     {row_x}..{}",
                    sx + sw,
                    row_x + row_w
                );
                if let Some((px, pw)) = segs.get(i.wrapping_sub(1)).filter(|_| i > 0) {
                    assert!(
                        *sx >= px + pw,
                        "{tab:?} segment {i} starts at {sx}, inside segment \
                         {} which ends at {}",
                        i - 1,
                        px + pw
                    );
                }
            }
        }
    }
}

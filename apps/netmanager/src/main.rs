//! OurOS Network Connections Manager
//!
//! GUI application for managing network interfaces, connections, and
//! configuration. Provides:
//! - Network interface listing with status indicators
//! - IP configuration (DHCP/static) editing
//! - DNS server management (add/remove/reorder)
//! - WiFi network scanning and connection
//! - VPN configuration and connect/disconnect
//! - Network profile management
//! - Traffic statistics (RX/TX bytes, speeds)
//! - Connection diagnostics (ping, traceroute status)
//! - Adapter enable/disable
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha dark theme.
//! Network I/O is performed through OurOS syscalls; simulated with
//! representative data for initial development.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha Theme Colors
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const BLUE: Color = Color::from_hex(0x89B4FA);
const RED: Color = Color::from_hex(0xF38BA8);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const TEAL: Color = Color::from_hex(0x94E2D5);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Layout Constants
// ============================================================================

const WINDOW_WIDTH: f32 = 960.0;
const WINDOW_HEIGHT: f32 = 680.0;
const TITLE_BAR_HEIGHT: f32 = 40.0;
const TOOLBAR_HEIGHT: f32 = 36.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const SIDEBAR_ITEM_HEIGHT: f32 = 52.0;
const SECTION_PADDING: f32 = 16.0;
const FIELD_HEIGHT: f32 = 28.0;
const FIELD_LABEL_WIDTH: f32 = 120.0;
const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_WIDTH: f32 = 100.0;
const WIFI_ITEM_HEIGHT: f32 = 40.0;
const VPN_ITEM_HEIGHT: f32 = 44.0;
const GRAPH_BAR_WIDTH: f32 = 8.0;
const GRAPH_BAR_GAP: f32 = 2.0;
const TRAFFIC_GRAPH_HEIGHT: f32 = 100.0;
const DNS_ROW_HEIGHT: f32 = 28.0;

// ============================================================================
// Core Types
// ============================================================================

/// Type of network interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfaceType {
    Ethernet,
    WiFi,
    VPN,
    Bridge,
    Loopback,
    Virtual,
}

impl InterfaceType {
    /// Human-readable label for this interface type.
    fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "Ethernet",
            Self::WiFi => "Wi-Fi",
            Self::VPN => "VPN",
            Self::Bridge => "Bridge",
            Self::Loopback => "Loopback",
            Self::Virtual => "Virtual",
        }
    }

    /// Color used for the type indicator circle in the sidebar.
    fn indicator_color(self) -> Color {
        match self {
            Self::Ethernet => BLUE,
            Self::WiFi => TEAL,
            Self::VPN => PEACH,
            Self::Bridge => YELLOW,
            Self::Loopback => OVERLAY0,
            Self::Virtual => SUBTEXT0,
        }
    }
}

/// Connection state of a network interface.
#[derive(Clone, Debug, PartialEq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Connecting,
    Error(String),
}

impl ConnectionState {
    fn label(&self) -> &str {
        match self {
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting...",
            Self::Error(_) => "Error",
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::Connected => GREEN,
            Self::Disconnected => OVERLAY0,
            Self::Connecting => YELLOW,
            Self::Error(_) => RED,
        }
    }

    fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

/// IP address configuration for a network interface.
#[derive(Clone, Debug, PartialEq)]
pub struct IpConfig {
    pub ip_address: String,
    pub subnet_mask: String,
    pub gateway: String,
    pub dns_servers: Vec<String>,
    pub dhcp_enabled: bool,
}

impl Default for IpConfig {
    fn default() -> Self {
        Self {
            ip_address: String::new(),
            subnet_mask: String::from("255.255.255.0"),
            gateway: String::new(),
            dns_servers: Vec::new(),
            dhcp_enabled: true,
        }
    }
}

impl IpConfig {
    /// Validate basic IP configuration fields.
    fn validate(&self) -> Result<(), String> {
        if !self.dhcp_enabled {
            if self.ip_address.is_empty() {
                return Err("IP address is required for static configuration".into());
            }
            if !is_valid_ipv4(&self.ip_address) {
                return Err(format!("Invalid IP address: {}", self.ip_address));
            }
            if !is_valid_ipv4(&self.subnet_mask) {
                return Err(format!("Invalid subnet mask: {}", self.subnet_mask));
            }
            if !self.gateway.is_empty() && !is_valid_ipv4(&self.gateway) {
                return Err(format!("Invalid gateway: {}", self.gateway));
            }
        }
        for dns in &self.dns_servers {
            if !is_valid_ipv4(dns) {
                return Err(format!("Invalid DNS server: {dns}"));
            }
        }
        Ok(())
    }
}

/// WiFi network discovered during scanning.
#[derive(Clone, Debug)]
pub struct WiFiNetwork {
    pub ssid: String,
    pub signal_strength: u8,
    pub security_type: String,
    pub channel: u32,
    pub frequency_ghz: f32,
}

impl WiFiNetwork {
    /// Number of signal bars (0-4) based on signal strength.
    fn signal_bars(&self) -> u8 {
        match self.signal_strength {
            0..=20 => 0,
            21..=40 => 1,
            41..=60 => 2,
            61..=80 => 3,
            _ => 4,
        }
    }

    /// Frequency band label.
    fn band_label(&self) -> &str {
        if self.frequency_ghz >= 5.0 {
            "5 GHz"
        } else {
            "2.4 GHz"
        }
    }
}

/// A network interface known to the system.
#[derive(Clone, Debug)]
pub struct NetworkInterface {
    pub id: u32,
    pub name: String,
    pub interface_type: InterfaceType,
    pub mac_address: String,
    pub ip_config: IpConfig,
    pub state: ConnectionState,
    pub speed_mbps: Option<u32>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub enabled: bool,
}

impl NetworkInterface {
    /// Format byte count as human-readable string.
    fn format_bytes(bytes: u64) -> String {
        if bytes < 1024 {
            return format!("{bytes} B");
        }
        let kb = bytes as f64 / 1024.0;
        if kb < 1024.0 {
            return format!("{kb:.1} KB");
        }
        let mb = kb / 1024.0;
        if mb < 1024.0 {
            return format!("{mb:.1} MB");
        }
        let gb = mb / 1024.0;
        format!("{gb:.2} GB")
    }

    /// Summary status line for the status bar.
    fn status_summary(&self) -> String {
        if self.state.is_connected() {
            format!(
                "{}: {} ({})",
                self.name,
                self.ip_config.ip_address,
                self.state.label(),
            )
        } else {
            format!("{}: {}", self.name, self.state.label())
        }
    }
}

/// VPN connection configuration.
#[derive(Clone, Debug)]
pub struct VpnConfig {
    pub name: String,
    pub server_address: String,
    pub protocol: VpnProtocol,
    pub auto_connect: bool,
}

/// Supported VPN protocols.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpnProtocol {
    OpenVPN,
    WireGuard,
    IPSec,
}

impl VpnProtocol {
    fn label(self) -> &'static str {
        match self {
            Self::OpenVPN => "OpenVPN",
            Self::WireGuard => "WireGuard",
            Self::IPSec => "IPSec",
        }
    }
}

/// Network profile with security settings.
#[derive(Clone, Debug)]
pub struct NetworkProfile {
    pub name: String,
    pub security_level: SecurityLevel,
    pub firewall_enabled: bool,
}

/// Security level for a network profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityLevel {
    Private,
    Public,
    Domain,
}

impl SecurityLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Private => "Private",
            Self::Public => "Public",
            Self::Domain => "Domain",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Private => GREEN,
            Self::Public => YELLOW,
            Self::Domain => BLUE,
        }
    }
}

/// A diagnostic check result.
#[derive(Clone, Debug)]
pub struct DiagnosticResult {
    pub name: String,
    pub status: DiagnosticStatus,
    pub details: String,
}

/// Status of a diagnostic check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticStatus {
    Passed,
    Warning,
    Failed,
    Running,
}

impl DiagnosticStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Passed => "Passed",
            Self::Warning => "Warning",
            Self::Failed => "Failed",
            Self::Running => "Running",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Passed => GREEN,
            Self::Warning => YELLOW,
            Self::Failed => RED,
            Self::Running => BLUE,
        }
    }
}

/// A throughput sample for the traffic graph.
#[derive(Clone, Copy, Debug)]
pub struct ThroughputSample {
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
}

// ============================================================================
// Detail Tab
// ============================================================================

/// Which tab is shown in the main detail panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailTab {
    Properties,
    IpConfig,
    Dns,
    WiFi,
    Vpn,
    Profiles,
    Traffic,
    Diagnostics,
}

impl DetailTab {
    fn label(self) -> &'static str {
        match self {
            Self::Properties => "Properties",
            Self::IpConfig => "IP Config",
            Self::Dns => "DNS",
            Self::WiFi => "Wi-Fi",
            Self::Vpn => "VPN",
            Self::Profiles => "Profiles",
            Self::Traffic => "Traffic",
            Self::Diagnostics => "Diagnostics",
        }
    }

    fn all() -> &'static [DetailTab] {
        &[
            Self::Properties,
            Self::IpConfig,
            Self::Dns,
            Self::WiFi,
            Self::Vpn,
            Self::Profiles,
            Self::Traffic,
            Self::Diagnostics,
        ]
    }
}

// ============================================================================
// Application State
// ============================================================================

/// Main application state.
pub struct NetManagerApp {
    /// All known network interfaces.
    pub interfaces: Vec<NetworkInterface>,
    /// Index of the currently selected interface in the sidebar.
    pub selected_interface: usize,
    /// Currently active detail tab.
    pub active_tab: DetailTab,
    /// WiFi networks visible from the selected WiFi interface.
    pub wifi_networks: Vec<WiFiNetwork>,
    /// Selected WiFi network index (within `wifi_networks`).
    pub selected_wifi: Option<usize>,
    /// VPN configurations.
    pub vpn_configs: Vec<VpnConfig>,
    /// VPN connection states indexed by vpn_configs position.
    pub vpn_states: Vec<ConnectionState>,
    /// Network profiles.
    pub profiles: Vec<NetworkProfile>,
    /// Selected profile index.
    pub selected_profile: Option<usize>,
    /// Throughput history for traffic graph.
    pub throughput_history: VecDeque<ThroughputSample>,
    /// Maximum samples kept in throughput history.
    pub max_throughput_samples: usize,
    /// Diagnostic results.
    pub diagnostics: Vec<DiagnosticResult>,
    /// Whether diagnostics are currently running.
    pub diagnostics_running: bool,
    /// Editing state for the DNS field being added.
    pub dns_input: String,
    /// Whether we are currently editing IP config (vs just viewing).
    pub editing_ip: bool,
    /// Temporary IP config for editing.
    pub edit_ip_config: IpConfig,
    /// Status bar message.
    pub status_message: String,
    /// Whether the sidebar is scrolled (future: scroll offset).
    pub sidebar_scroll: f32,
}

impl NetManagerApp {
    /// Create a new application with sample data.
    pub fn new() -> Self {
        let interfaces = sample_interfaces();
        let wifi_networks = sample_wifi_networks();
        let vpn_configs = sample_vpn_configs();
        let vpn_states = vec![
            ConnectionState::Disconnected,
            ConnectionState::Connected,
            ConnectionState::Disconnected,
        ];
        let profiles = sample_profiles();
        let diagnostics = Vec::new();
        let throughput_history = sample_throughput_history();

        let edit_ip_config = interfaces
            .first()
            .map(|iface| iface.ip_config.clone())
            .unwrap_or_default();

        let status_message = interfaces
            .first()
            .map(|iface| iface.status_summary())
            .unwrap_or_else(|| "No interfaces".into());

        Self {
            interfaces,
            selected_interface: 0,
            active_tab: DetailTab::Properties,
            wifi_networks,
            selected_wifi: None,
            vpn_configs,
            vpn_states,
            profiles,
            selected_profile: None,
            throughput_history,
            max_throughput_samples: 60,
            diagnostics,
            diagnostics_running: false,
            dns_input: String::new(),
            editing_ip: false,
            edit_ip_config,
            status_message,
            sidebar_scroll: 0.0,
        }
    }

    /// Get a reference to the currently selected interface, if any.
    pub fn selected_iface(&self) -> Option<&NetworkInterface> {
        self.interfaces.get(self.selected_interface)
    }

    /// Select an interface by sidebar index.
    pub fn select_interface(&mut self, index: usize) {
        if index < self.interfaces.len() {
            self.selected_interface = index;
            self.editing_ip = false;
            if let Some(iface) = self.interfaces.get(index) {
                self.edit_ip_config = iface.ip_config.clone();
                self.status_message = iface.status_summary();
            }
        }
    }

    /// Toggle enabled/disabled state for the selected interface.
    pub fn toggle_selected_enabled(&mut self) {
        if let Some(iface) = self.interfaces.get_mut(self.selected_interface) {
            iface.enabled = !iface.enabled;
            if !iface.enabled {
                iface.state = ConnectionState::Disconnected;
            } else {
                iface.state = ConnectionState::Connecting;
            }
            self.status_message = iface.status_summary();
        }
    }

    /// Start editing the IP configuration for the selected interface.
    pub fn start_editing_ip(&mut self) {
        if let Some(iface) = self.interfaces.get(self.selected_interface) {
            self.edit_ip_config = iface.ip_config.clone();
            self.editing_ip = true;
        }
    }

    /// Apply the edited IP configuration to the selected interface.
    pub fn apply_ip_config(&mut self) -> Result<(), String> {
        self.edit_ip_config.validate()?;
        if let Some(iface) = self.interfaces.get_mut(self.selected_interface) {
            iface.ip_config = self.edit_ip_config.clone();
            self.editing_ip = false;
            self.status_message = format!("IP configuration updated for {}", iface.name);
            Ok(())
        } else {
            Err("No interface selected".into())
        }
    }

    /// Cancel IP configuration editing.
    pub fn cancel_editing_ip(&mut self) {
        self.editing_ip = false;
        if let Some(iface) = self.interfaces.get(self.selected_interface) {
            self.edit_ip_config = iface.ip_config.clone();
        }
    }

    /// Add a DNS server to the edited IP config.
    pub fn add_dns_server(&mut self, server: &str) -> Result<(), String> {
        if server.is_empty() {
            return Err("DNS server address is empty".into());
        }
        if !is_valid_ipv4(server) {
            return Err(format!("Invalid DNS address: {server}"));
        }
        if self
            .edit_ip_config
            .dns_servers
            .contains(&server.to_string())
        {
            return Err("DNS server already in list".into());
        }
        self.edit_ip_config.dns_servers.push(server.to_string());
        Ok(())
    }

    /// Remove a DNS server by index from the edited IP config.
    pub fn remove_dns_server(&mut self, index: usize) -> Result<(), String> {
        if index >= self.edit_ip_config.dns_servers.len() {
            return Err("DNS server index out of range".into());
        }
        self.edit_ip_config.dns_servers.remove(index);
        Ok(())
    }

    /// Move a DNS server up in priority (lower index = higher priority).
    pub fn move_dns_up(&mut self, index: usize) -> Result<(), String> {
        if index == 0 {
            return Err("Already at top".into());
        }
        if index >= self.edit_ip_config.dns_servers.len() {
            return Err("Index out of range".into());
        }
        self.edit_ip_config.dns_servers.swap(index, index - 1);
        Ok(())
    }

    /// Move a DNS server down in priority.
    pub fn move_dns_down(&mut self, index: usize) -> Result<(), String> {
        if index + 1 >= self.edit_ip_config.dns_servers.len() {
            return Err("Already at bottom".into());
        }
        self.edit_ip_config.dns_servers.swap(index, index + 1);
        Ok(())
    }

    /// Select a WiFi network by index.
    pub fn select_wifi(&mut self, index: usize) {
        if index < self.wifi_networks.len() {
            self.selected_wifi = Some(index);
        }
    }

    /// Attempt to connect to the selected WiFi network.
    pub fn connect_wifi(&mut self) -> Result<String, String> {
        let wifi_idx = self.selected_wifi.ok_or("No WiFi network selected")?;
        let network = self
            .wifi_networks
            .get(wifi_idx)
            .ok_or("WiFi network index out of range")?;
        let ssid = network.ssid.clone();

        // In a real implementation this would trigger an OS-level connection.
        // For now, update the selected WiFi interface to Connecting state.
        if let Some(iface) = self.interfaces.get_mut(self.selected_interface) {
            if iface.interface_type == InterfaceType::WiFi {
                iface.state = ConnectionState::Connecting;
                self.status_message = format!("Connecting to {ssid}...");
            }
        }
        Ok(ssid)
    }

    /// Toggle VPN connection state by index.
    pub fn toggle_vpn(&mut self, index: usize) -> Result<(), String> {
        if index >= self.vpn_states.len() {
            return Err("VPN index out of range".into());
        }
        let new_state = if self.vpn_states[index].is_connected() {
            ConnectionState::Disconnected
        } else {
            ConnectionState::Connecting
        };
        self.vpn_states[index] = new_state;

        if let Some(vpn) = self.vpn_configs.get(index) {
            self.status_message = format!("VPN '{}' {}", vpn.name, self.vpn_states[index].label());
        }
        Ok(())
    }

    /// Run network diagnostics (simulated).
    pub fn run_diagnostics(&mut self) {
        self.diagnostics_running = true;
        self.diagnostics = vec![
            DiagnosticResult {
                name: "Ping Gateway".into(),
                status: DiagnosticStatus::Passed,
                details: "Gateway 192.168.1.1 responded in 1.2ms".into(),
            },
            DiagnosticResult {
                name: "DNS Resolution".into(),
                status: DiagnosticStatus::Passed,
                details: "Resolved ouros.local in 5ms".into(),
            },
            DiagnosticResult {
                name: "Internet Connectivity".into(),
                status: DiagnosticStatus::Passed,
                details: "Successfully reached external host".into(),
            },
            DiagnosticResult {
                name: "Traceroute".into(),
                status: DiagnosticStatus::Warning,
                details: "12 hops, 45ms avg latency".into(),
            },
            DiagnosticResult {
                name: "Packet Loss".into(),
                status: DiagnosticStatus::Passed,
                details: "0% packet loss over 100 pings".into(),
            },
            DiagnosticResult {
                name: "MTU Test".into(),
                status: DiagnosticStatus::Passed,
                details: "MTU 1500 confirmed".into(),
            },
        ];
        self.diagnostics_running = false;
        self.status_message = "Diagnostics complete".into();
    }

    /// Add a throughput sample to the history ring buffer.
    pub fn push_throughput(&mut self, sample: ThroughputSample) {
        if self.throughput_history.len() >= self.max_throughput_samples {
            self.throughput_history.pop_front();
        }
        self.throughput_history.push_back(sample);
    }

    /// Set the active detail tab.
    pub fn set_tab(&mut self, tab: DetailTab) {
        self.active_tab = tab;
    }

    /// Create a new network profile.
    pub fn add_profile(&mut self, name: &str, level: SecurityLevel, firewall: bool) {
        self.profiles.push(NetworkProfile {
            name: name.to_string(),
            security_level: level,
            firewall_enabled: firewall,
        });
    }

    /// Remove a network profile by index.
    pub fn remove_profile(&mut self, index: usize) -> Result<(), String> {
        if index >= self.profiles.len() {
            return Err("Profile index out of range".into());
        }
        self.profiles.remove(index);
        if let Some(sel) = self.selected_profile {
            if sel >= self.profiles.len() {
                self.selected_profile = if self.profiles.is_empty() {
                    None
                } else {
                    Some(self.profiles.len() - 1)
                };
            }
        }
        Ok(())
    }
}

impl Default for NetManagerApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Rendering
// ============================================================================

/// Render the entire application UI into a render tree.
pub fn render_app(app: &NetManagerApp) -> RenderTree {
    let mut tree = RenderTree::new();

    // Background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: WINDOW_WIDTH,
        height: WINDOW_HEIGHT,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    render_title_bar(&mut tree);
    render_toolbar(&mut tree, app);
    render_sidebar(&mut tree, app);
    render_detail_panel(&mut tree, app);
    render_status_bar(&mut tree, app);

    tree
}

/// Render the title bar at the top of the window.
fn render_title_bar(tree: &mut RenderTree) {
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: WINDOW_WIDTH,
        height: TITLE_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });
    tree.push(RenderCommand::Text {
        x: 14.0,
        y: 12.0,
        text: "Network Connections".into(),
        color: TEXT_COLOR,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
}

/// Render the toolbar below the title bar.
fn render_toolbar(tree: &mut RenderTree, app: &NetManagerApp) {
    let y = TITLE_BAR_HEIGHT;

    // Toolbar background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width: WINDOW_WIDTH,
        height: TOOLBAR_HEIGHT,
        color: SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    // Toolbar buttons
    let buttons = ["Refresh", "Diagnose", "Properties"];
    let mut bx = 12.0;
    for label in &buttons {
        let bw = label.len() as f32 * 8.0 + 24.0;
        tree.push(RenderCommand::FillRect {
            x: bx,
            y: y + 4.0,
            width: bw,
            height: TOOLBAR_HEIGHT - 8.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: bx + 12.0,
            y: y + 10.0,
            text: label.to_string(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        bx += bw + 8.0;
    }

    // Enable/Disable toggle on the right side
    let toggle_label = if app.selected_iface().map_or(false, |iface| iface.enabled) {
        "Disable"
    } else {
        "Enable"
    };
    let tw = toggle_label.len() as f32 * 8.0 + 24.0;
    let tx = WINDOW_WIDTH - tw - 12.0;
    tree.push(RenderCommand::FillRect {
        x: tx,
        y: y + 4.0,
        width: tw,
        height: TOOLBAR_HEIGHT - 8.0,
        color: SURFACE1,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: tx + 12.0,
        y: y + 10.0,
        text: toggle_label.into(),
        color: TEXT_COLOR,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

/// Render the sidebar interface list.
fn render_sidebar(tree: &mut RenderTree, app: &NetManagerApp) {
    let sx = 0.0;
    let sy = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let sh = WINDOW_HEIGHT - sy - STATUS_BAR_HEIGHT;

    // Sidebar background
    tree.push(RenderCommand::FillRect {
        x: sx,
        y: sy,
        width: SIDEBAR_WIDTH,
        height: sh,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Sidebar header
    tree.push(RenderCommand::Text {
        x: sx + 12.0,
        y: sy + 10.0,
        text: "Interfaces".into(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Separator
    tree.push(RenderCommand::Line {
        x1: sx + 8.0,
        y1: sy + 28.0,
        x2: sx + SIDEBAR_WIDTH - 8.0,
        y2: sy + 28.0,
        color: SURFACE0,
        width: 1.0,
    });

    // Interface items
    let list_y = sy + 32.0;
    for (i, iface) in app.interfaces.iter().enumerate() {
        let item_y = list_y + i as f32 * SIDEBAR_ITEM_HEIGHT;
        let is_selected = i == app.selected_interface;

        // Selection highlight
        if is_selected {
            tree.push(RenderCommand::FillRect {
                x: sx + 4.0,
                y: item_y,
                width: SIDEBAR_WIDTH - 8.0,
                height: SIDEBAR_ITEM_HEIGHT - 2.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Type indicator circle
        let circle_x = sx + 16.0;
        let circle_y = item_y + SIDEBAR_ITEM_HEIGHT / 2.0 - 6.0;
        tree.push(RenderCommand::FillRect {
            x: circle_x,
            y: circle_y,
            width: 12.0,
            height: 12.0,
            color: iface.interface_type.indicator_color(),
            corner_radii: CornerRadii::all(6.0),
        });

        // Interface name
        tree.push(RenderCommand::Text {
            x: sx + 36.0,
            y: item_y + 8.0,
            text: iface.name.clone(),
            color: TEXT_COLOR,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 50.0),
        });

        // Status line
        let status_text = format!("{} - {}", iface.interface_type.label(), iface.state.label());
        tree.push(RenderCommand::Text {
            x: sx + 36.0,
            y: item_y + 26.0,
            text: status_text,
            color: iface.state.color(),
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(SIDEBAR_WIDTH - 50.0),
        });

        // Status dot (right side)
        tree.push(RenderCommand::FillRect {
            x: sx + SIDEBAR_WIDTH - 20.0,
            y: item_y + SIDEBAR_ITEM_HEIGHT / 2.0 - 4.0,
            width: 8.0,
            height: 8.0,
            color: iface.state.color(),
            corner_radii: CornerRadii::all(4.0),
        });
    }
}

/// Render the main detail panel area.
fn render_detail_panel(tree: &mut RenderTree, app: &NetManagerApp) {
    let px = SIDEBAR_WIDTH;
    let py = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let pw = WINDOW_WIDTH - SIDEBAR_WIDTH;
    let ph = WINDOW_HEIGHT - py - STATUS_BAR_HEIGHT;

    // Panel background
    tree.push(RenderCommand::FillRect {
        x: px,
        y: py,
        width: pw,
        height: ph,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    // Tab bar
    render_tab_bar(tree, app, px, py, pw);

    // Tab content area
    let content_y = py + 32.0;
    let content_h = ph - 32.0;

    tree.push(RenderCommand::PushClip {
        x: px,
        y: content_y,
        width: pw,
        height: content_h,
    });

    match app.active_tab {
        DetailTab::Properties => render_tab_properties(tree, app, px, content_y, pw),
        DetailTab::IpConfig => render_tab_ip_config(tree, app, px, content_y, pw),
        DetailTab::Dns => render_tab_dns(tree, app, px, content_y, pw),
        DetailTab::WiFi => render_tab_wifi(tree, app, px, content_y, pw),
        DetailTab::Vpn => render_tab_vpn(tree, app, px, content_y, pw),
        DetailTab::Profiles => render_tab_profiles(tree, app, px, content_y, pw),
        DetailTab::Traffic => render_tab_traffic(tree, app, px, content_y, pw),
        DetailTab::Diagnostics => render_tab_diagnostics(tree, app, px, content_y, pw),
    }

    tree.push(RenderCommand::PopClip);
}

/// Render tab headers at the top of the detail panel.
fn render_tab_bar(tree: &mut RenderTree, app: &NetManagerApp, px: f32, py: f32, _pw: f32) {
    // Tab bar background
    tree.push(RenderCommand::FillRect {
        x: px,
        y: py,
        width: WINDOW_WIDTH - px,
        height: 30.0,
        color: SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    let mut tx = px + 8.0;
    for tab in DetailTab::all() {
        let label = tab.label();
        let tw = label.len() as f32 * 7.5 + 16.0;
        let is_active = *tab == app.active_tab;

        if is_active {
            tree.push(RenderCommand::FillRect {
                x: tx,
                y: py + 2.0,
                width: tw,
                height: 26.0,
                color: BASE,
                corner_radii: CornerRadii {
                    top_left: 4.0,
                    top_right: 4.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });
        }

        let text_color = if is_active { TEXT_COLOR } else { SUBTEXT0 };
        tree.push(RenderCommand::Text {
            x: tx + 8.0,
            y: py + 9.0,
            text: label.to_string(),
            color: text_color,
            font_size: 11.0,
            font_weight: if is_active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: None,
        });

        tx += tw + 4.0;
    }
}

/// Render the Properties tab content.
fn render_tab_properties(tree: &mut RenderTree, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let Some(iface) = app.selected_iface() else {
        render_no_selection(tree, px, py, pw);
        return;
    };

    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;
    let vx = lx + FIELD_LABEL_WIDTH;

    // Section title
    y = render_section_title(tree, "Interface Details", lx, y);

    // Fields
    let fields: &[(&str, String)] = &[
        ("Name:", iface.name.clone()),
        ("Type:", iface.interface_type.label().to_string()),
        ("MAC Address:", iface.mac_address.clone()),
        ("Status:", iface.state.label().to_string()),
        (
            "Speed:",
            iface
                .speed_mbps
                .map_or("N/A".to_string(), |s| format!("{s} Mbps")),
        ),
        (
            "Enabled:",
            if iface.enabled { "Yes" } else { "No" }.to_string(),
        ),
    ];

    for (label, value) in fields {
        render_field_row(tree, label, value, lx, vx, y);
        y += FIELD_HEIGHT + 4.0;
    }

    // Traffic section
    y += 12.0;
    y = render_section_title(tree, "Traffic Statistics", lx, y);

    let traffic_fields: &[(&str, String)] = &[
        ("Received:", NetworkInterface::format_bytes(iface.rx_bytes)),
        (
            "Transmitted:",
            NetworkInterface::format_bytes(iface.tx_bytes),
        ),
    ];

    for (label, value) in traffic_fields {
        render_field_row(tree, label, value, lx, vx, y);
        y += FIELD_HEIGHT + 4.0;
    }

    // IP summary
    y += 12.0;
    y = render_section_title(tree, "IP Configuration Summary", lx, y);

    let ip = &iface.ip_config;
    let ip_fields: &[(&str, &str)] = &[
        (
            "DHCP:",
            if ip.dhcp_enabled {
                "Enabled"
            } else {
                "Disabled"
            },
        ),
        ("IP Address:", &ip.ip_address),
        ("Subnet Mask:", &ip.subnet_mask),
        ("Gateway:", &ip.gateway),
    ];

    for (label, value) in ip_fields {
        render_field_row(tree, label, &value.to_string(), lx, vx, y);
        y += FIELD_HEIGHT + 4.0;
    }
}

/// Render the IP Config tab content.
fn render_tab_ip_config(tree: &mut RenderTree, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let Some(_iface) = app.selected_iface() else {
        render_no_selection(tree, px, py, pw);
        return;
    };

    let ip = &app.edit_ip_config;
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;
    let vx = lx + FIELD_LABEL_WIDTH;

    y = render_section_title(tree, "IP Configuration", lx, y);

    // DHCP toggle
    let dhcp_label = if ip.dhcp_enabled {
        "DHCP: Enabled"
    } else {
        "DHCP: Disabled (Static)"
    };
    render_toggle_row(tree, dhcp_label, ip.dhcp_enabled, lx, y);
    y += FIELD_HEIGHT + 8.0;

    // IP fields (dimmed if DHCP is on and not editing)
    let field_color = if ip.dhcp_enabled && !app.editing_ip {
        OVERLAY0
    } else {
        TEXT_COLOR
    };

    let ip_fields: &[(&str, &str)] = &[
        ("IP Address:", &ip.ip_address),
        ("Subnet Mask:", &ip.subnet_mask),
        ("Gateway:", &ip.gateway),
    ];

    for (label, value) in ip_fields {
        render_editable_field(tree, label, value, lx, vx, y, field_color, app.editing_ip);
        y += FIELD_HEIGHT + 6.0;
    }

    // Buttons
    y += 12.0;
    if app.editing_ip {
        render_button(tree, "Apply", lx, y, BUTTON_WIDTH, BUTTON_HEIGHT, GREEN);
        render_button(
            tree,
            "Cancel",
            lx + BUTTON_WIDTH + 12.0,
            y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            RED,
        );
    } else {
        render_button(tree, "Edit", lx, y, BUTTON_WIDTH, BUTTON_HEIGHT, BLUE);
    }
}

/// Render the DNS tab content.
fn render_tab_dns(tree: &mut RenderTree, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let Some(_iface) = app.selected_iface() else {
        render_no_selection(tree, px, py, pw);
        return;
    };

    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(tree, "DNS Servers", lx, y);

    // DNS server list
    let dns = &app.edit_ip_config.dns_servers;
    if dns.is_empty() {
        tree.push(RenderCommand::Text {
            x: lx,
            y,
            text: "No DNS servers configured".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        y += DNS_ROW_HEIGHT;
    } else {
        for (i, server) in dns.iter().enumerate() {
            // Row background
            let row_bg = if i % 2 == 0 { SURFACE0 } else { BASE };
            tree.push(RenderCommand::FillRect {
                x: lx,
                y,
                width: pw - SECTION_PADDING * 2.0,
                height: DNS_ROW_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::all(3.0),
            });

            // Priority number
            tree.push(RenderCommand::Text {
                x: lx + 8.0,
                y: y + 7.0,
                text: format!("{}.", i + 1),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Server address
            tree.push(RenderCommand::Text {
                x: lx + 32.0,
                y: y + 7.0,
                text: server.clone(),
                color: TEXT_COLOR,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Up/Down/Remove buttons (small)
            let btn_y = y + 3.0;
            let btn_x = lx + pw - SECTION_PADDING * 2.0 - 100.0;

            if i > 0 {
                render_mini_button(tree, "^", btn_x, btn_y, BLUE);
            }
            if i + 1 < dns.len() {
                render_mini_button(tree, "v", btn_x + 24.0, btn_y, BLUE);
            }
            render_mini_button(tree, "X", btn_x + 48.0, btn_y, RED);

            y += DNS_ROW_HEIGHT + 2.0;
        }
    }

    // Add DNS input
    y += 12.0;
    y = render_section_title(tree, "Add DNS Server", lx, y);

    // Input field
    tree.push(RenderCommand::FillRect {
        x: lx,
        y,
        width: 200.0,
        height: FIELD_HEIGHT,
        color: SURFACE0,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::StrokeRect {
        x: lx,
        y,
        width: 200.0,
        height: FIELD_HEIGHT,
        color: OVERLAY0,
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });
    let dns_display = if app.dns_input.is_empty() {
        "e.g. 8.8.8.8"
    } else {
        &app.dns_input
    };
    let dns_color = if app.dns_input.is_empty() {
        OVERLAY0
    } else {
        TEXT_COLOR
    };
    tree.push(RenderCommand::Text {
        x: lx + 8.0,
        y: y + 7.0,
        text: dns_display.to_string(),
        color: dns_color,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(184.0),
    });

    // Add button
    render_button(tree, "Add", lx + 212.0, y, 60.0, FIELD_HEIGHT, GREEN);
}

/// Render the WiFi tab content.
fn render_tab_wifi(tree: &mut RenderTree, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(tree, "Available WiFi Networks", lx, y);

    if app.wifi_networks.is_empty() {
        tree.push(RenderCommand::Text {
            x: lx,
            y,
            text: "No WiFi networks found. Click Refresh to scan.".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        return;
    }

    for (i, network) in app.wifi_networks.iter().enumerate() {
        let is_selected = app.selected_wifi == Some(i);
        let item_y = y;

        // Selection / hover background
        let bg = if is_selected { SURFACE0 } else { BASE };
        tree.push(RenderCommand::FillRect {
            x: lx,
            y: item_y,
            width: pw - SECTION_PADDING * 2.0,
            height: WIFI_ITEM_HEIGHT,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });

        // Signal bars
        let bars = network.signal_bars();
        render_signal_bars(tree, bars, lx + 8.0, item_y + 8.0);

        // SSID
        tree.push(RenderCommand::Text {
            x: lx + 40.0,
            y: item_y + 6.0,
            text: network.ssid.clone(),
            color: TEXT_COLOR,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Details line
        let detail = format!(
            "{} | Ch {} | {} | {}%",
            network.security_type,
            network.channel,
            network.band_label(),
            network.signal_strength,
        );
        tree.push(RenderCommand::Text {
            x: lx + 40.0,
            y: item_y + 22.0,
            text: detail,
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

        // Connect button
        if is_selected {
            render_button(
                tree,
                "Connect",
                lx + pw - SECTION_PADDING * 2.0 - 80.0,
                item_y + 6.0,
                70.0,
                28.0,
                GREEN,
            );
        }

        y += WIFI_ITEM_HEIGHT + 4.0;
    }
}

/// Render WiFi signal bars.
fn render_signal_bars(tree: &mut RenderTree, bars: u8, x: f32, y: f32) {
    for i in 0u8..4 {
        let bar_h = 6.0 + (i as f32) * 4.0;
        let bar_y = y + 20.0 - bar_h;
        let bar_color = if i < bars { GREEN } else { SURFACE1 };
        tree.push(RenderCommand::FillRect {
            x: x + i as f32 * 7.0,
            y: bar_y,
            width: 5.0,
            height: bar_h,
            color: bar_color,
            corner_radii: CornerRadii::all(1.0),
        });
    }
}

/// Render the VPN tab content.
fn render_tab_vpn(tree: &mut RenderTree, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(tree, "VPN Connections", lx, y);

    if app.vpn_configs.is_empty() {
        tree.push(RenderCommand::Text {
            x: lx,
            y,
            text: "No VPN connections configured".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        return;
    }

    for (i, vpn) in app.vpn_configs.iter().enumerate() {
        let state = app
            .vpn_states
            .get(i)
            .cloned()
            .unwrap_or(ConnectionState::Disconnected);
        let item_y = y;

        // Card background
        tree.push(RenderCommand::FillRect {
            x: lx,
            y: item_y,
            width: pw - SECTION_PADDING * 2.0,
            height: VPN_ITEM_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Status indicator
        tree.push(RenderCommand::FillRect {
            x: lx + 12.0,
            y: item_y + VPN_ITEM_HEIGHT / 2.0 - 5.0,
            width: 10.0,
            height: 10.0,
            color: state.color(),
            corner_radii: CornerRadii::all(5.0),
        });

        // VPN name
        tree.push(RenderCommand::Text {
            x: lx + 30.0,
            y: item_y + 6.0,
            text: vpn.name.clone(),
            color: TEXT_COLOR,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(250.0),
        });

        // Details
        let detail = format!(
            "{} | {} | Auto: {}",
            vpn.server_address,
            vpn.protocol.label(),
            if vpn.auto_connect { "Yes" } else { "No" },
        );
        tree.push(RenderCommand::Text {
            x: lx + 30.0,
            y: item_y + 24.0,
            text: detail,
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(350.0),
        });

        // Connect/Disconnect button
        let btn_label = if state.is_connected() {
            "Disconnect"
        } else {
            "Connect"
        };
        let btn_color = if state.is_connected() { RED } else { GREEN };
        render_button(
            tree,
            btn_label,
            lx + pw - SECTION_PADDING * 2.0 - 100.0,
            item_y + 8.0,
            88.0,
            28.0,
            btn_color,
        );

        y += VPN_ITEM_HEIGHT + 6.0;
    }
}

/// Render the Profiles tab content.
fn render_tab_profiles(tree: &mut RenderTree, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(tree, "Network Profiles", lx, y);

    if app.profiles.is_empty() {
        tree.push(RenderCommand::Text {
            x: lx,
            y,
            text: "No profiles configured".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        return;
    }

    for (i, profile) in app.profiles.iter().enumerate() {
        let is_selected = app.selected_profile == Some(i);
        let item_y = y;
        let row_h = 36.0;

        // Row background
        let bg = if is_selected { SURFACE0 } else { BASE };
        tree.push(RenderCommand::FillRect {
            x: lx,
            y: item_y,
            width: pw - SECTION_PADDING * 2.0,
            height: row_h,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });

        // Security level indicator
        tree.push(RenderCommand::FillRect {
            x: lx + 8.0,
            y: item_y + row_h / 2.0 - 5.0,
            width: 10.0,
            height: 10.0,
            color: profile.security_level.color(),
            corner_radii: CornerRadii::all(5.0),
        });

        // Profile name
        tree.push(RenderCommand::Text {
            x: lx + 26.0,
            y: item_y + 6.0,
            text: profile.name.clone(),
            color: TEXT_COLOR,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Details
        let detail = format!(
            "{} | Firewall: {}",
            profile.security_level.label(),
            if profile.firewall_enabled {
                "On"
            } else {
                "Off"
            },
        );
        tree.push(RenderCommand::Text {
            x: lx + 26.0,
            y: item_y + 22.0,
            text: detail,
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Remove button
        render_mini_button(
            tree,
            "X",
            lx + pw - SECTION_PADDING * 2.0 - 30.0,
            item_y + 8.0,
            RED,
        );

        y += row_h + 4.0;
    }

    // Add profile button
    y += 12.0;
    render_button(tree, "Add Profile", lx, y, 120.0, BUTTON_HEIGHT, BLUE);
}

/// Render the Traffic tab content with a simple bar chart.
fn render_tab_traffic(tree: &mut RenderTree, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let Some(iface) = app.selected_iface() else {
        render_no_selection(tree, px, py, pw);
        return;
    };

    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;
    let vx = lx + FIELD_LABEL_WIDTH;

    y = render_section_title(tree, "Traffic Overview", lx, y);

    // Current stats
    render_field_row(
        tree,
        "Received:",
        &NetworkInterface::format_bytes(iface.rx_bytes),
        lx,
        vx,
        y,
    );
    y += FIELD_HEIGHT + 4.0;
    render_field_row(
        tree,
        "Transmitted:",
        &NetworkInterface::format_bytes(iface.tx_bytes),
        lx,
        vx,
        y,
    );
    y += FIELD_HEIGHT + 16.0;

    // Throughput graph
    y = render_section_title(tree, "Throughput (recent)", lx, y);

    let graph_x = lx;
    let graph_w = pw - SECTION_PADDING * 2.0;
    let graph_h = TRAFFIC_GRAPH_HEIGHT;

    // Graph background
    tree.push(RenderCommand::FillRect {
        x: graph_x,
        y,
        width: graph_w,
        height: graph_h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(4.0),
    });

    // Find max throughput for scaling
    let max_throughput = app
        .throughput_history
        .iter()
        .map(|s| s.rx_bytes_per_sec.max(s.tx_bytes_per_sec))
        .fold(1.0_f64, f64::max);

    // Draw bars
    let bar_total_w = GRAPH_BAR_WIDTH + GRAPH_BAR_GAP;
    let max_bars = ((graph_w - 8.0) / (bar_total_w * 2.0 + GRAPH_BAR_GAP)) as usize;
    let samples: Vec<&ThroughputSample> = app
        .throughput_history
        .iter()
        .rev()
        .take(max_bars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    for (i, sample) in samples.iter().enumerate() {
        let bx = graph_x + 4.0 + i as f32 * (bar_total_w * 2.0 + GRAPH_BAR_GAP);

        // RX bar (teal)
        let rx_h = (sample.rx_bytes_per_sec / max_throughput * (graph_h - 8.0) as f64) as f32;
        tree.push(RenderCommand::FillRect {
            x: bx,
            y: y + graph_h - 4.0 - rx_h,
            width: GRAPH_BAR_WIDTH,
            height: rx_h.max(1.0),
            color: TEAL,
            corner_radii: CornerRadii {
                top_left: 2.0,
                top_right: 2.0,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // TX bar (peach)
        let tx_h = (sample.tx_bytes_per_sec / max_throughput * (graph_h - 8.0) as f64) as f32;
        tree.push(RenderCommand::FillRect {
            x: bx + GRAPH_BAR_WIDTH + GRAPH_BAR_GAP,
            y: y + graph_h - 4.0 - tx_h,
            width: GRAPH_BAR_WIDTH,
            height: tx_h.max(1.0),
            color: PEACH,
            corner_radii: CornerRadii {
                top_left: 2.0,
                top_right: 2.0,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });
    }

    // Legend
    let legend_y = y + graph_h + 8.0;
    tree.push(RenderCommand::FillRect {
        x: lx,
        y: legend_y,
        width: 12.0,
        height: 12.0,
        color: TEAL,
        corner_radii: CornerRadii::all(2.0),
    });
    tree.push(RenderCommand::Text {
        x: lx + 16.0,
        y: legend_y + 1.0,
        text: "RX".into(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
    tree.push(RenderCommand::FillRect {
        x: lx + 50.0,
        y: legend_y,
        width: 12.0,
        height: 12.0,
        color: PEACH,
        corner_radii: CornerRadii::all(2.0),
    });
    tree.push(RenderCommand::Text {
        x: lx + 66.0,
        y: legend_y + 1.0,
        text: "TX".into(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

/// Render the Diagnostics tab content.
fn render_tab_diagnostics(tree: &mut RenderTree, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(tree, "Network Diagnostics", lx, y);

    if app.diagnostics.is_empty() {
        tree.push(RenderCommand::Text {
            x: lx,
            y,
            text: "Click 'Diagnose' in the toolbar to run diagnostics.".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        y += 24.0;
        render_button(tree, "Run", lx, y, 80.0, BUTTON_HEIGHT, BLUE);
        return;
    }

    for diag in &app.diagnostics {
        let row_h = 32.0;

        // Row background
        tree.push(RenderCommand::FillRect {
            x: lx,
            y,
            width: pw - SECTION_PADDING * 2.0,
            height: row_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Status indicator
        tree.push(RenderCommand::FillRect {
            x: lx + 10.0,
            y: y + row_h / 2.0 - 5.0,
            width: 10.0,
            height: 10.0,
            color: diag.status.color(),
            corner_radii: CornerRadii::all(5.0),
        });

        // Name
        tree.push(RenderCommand::Text {
            x: lx + 28.0,
            y: y + 4.0,
            text: diag.name.clone(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Details
        tree.push(RenderCommand::Text {
            x: lx + 28.0,
            y: y + 18.0,
            text: diag.details.clone(),
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(pw - SECTION_PADDING * 2.0 - 120.0),
        });

        // Status label on right
        tree.push(RenderCommand::Text {
            x: lx + pw - SECTION_PADDING * 2.0 - 60.0,
            y: y + 10.0,
            text: diag.status.label().to_string(),
            color: diag.status.color(),
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        y += row_h + 4.0;
    }
}

/// Render the status bar at the bottom of the window.
fn render_status_bar(tree: &mut RenderTree, app: &NetManagerApp) {
    let sy = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;

    // Status bar background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: sy,
        width: WINDOW_WIDTH,
        height: STATUS_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Separator line
    tree.push(RenderCommand::Line {
        x1: 0.0,
        y1: sy,
        x2: WINDOW_WIDTH,
        y2: sy,
        color: SURFACE0,
        width: 1.0,
    });

    // Status message
    tree.push(RenderCommand::Text {
        x: 12.0,
        y: sy + 8.0,
        text: app.status_message.clone(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(WINDOW_WIDTH - 200.0),
    });

    // Interface count on right
    let iface_count = format!("{} interfaces", app.interfaces.len());
    tree.push(RenderCommand::Text {
        x: WINDOW_WIDTH - 120.0,
        y: sy + 8.0,
        text: iface_count,
        color: OVERLAY0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

// ============================================================================
// Rendering Helpers
// ============================================================================

/// Render a section title with underline.
fn render_section_title(tree: &mut RenderTree, title: &str, x: f32, y: f32) -> f32 {
    tree.push(RenderCommand::Text {
        x,
        y,
        text: title.to_string(),
        color: BLUE,
        font_size: 13.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    tree.push(RenderCommand::Line {
        x1: x,
        y1: y + 18.0,
        x2: x + title.len() as f32 * 8.0,
        y2: y + 18.0,
        color: SURFACE1,
        width: 1.0,
    });
    y + 26.0
}

/// Render a label-value field row.
fn render_field_row(tree: &mut RenderTree, label: &str, value: &str, lx: f32, vx: f32, y: f32) {
    tree.push(RenderCommand::Text {
        x: lx,
        y,
        text: label.to_string(),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
    tree.push(RenderCommand::Text {
        x: vx,
        y,
        text: value.to_string(),
        color: TEXT_COLOR,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

/// Render an editable field row with input box styling.
fn render_editable_field(
    tree: &mut RenderTree,
    label: &str,
    value: &str,
    lx: f32,
    vx: f32,
    y: f32,
    color: Color,
    editing: bool,
) {
    tree.push(RenderCommand::Text {
        x: lx,
        y: y + 6.0,
        text: label.to_string(),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    if editing {
        // Input box background
        tree.push(RenderCommand::FillRect {
            x: vx,
            y,
            width: 200.0,
            height: FIELD_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x: vx,
            y,
            width: 200.0,
            height: FIELD_HEIGHT,
            color: BLUE,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });
    }

    let display = if value.is_empty() { "---" } else { value };
    tree.push(RenderCommand::Text {
        x: vx + if editing { 8.0 } else { 0.0 },
        y: y + 7.0,
        text: display.to_string(),
        color,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(190.0),
    });
}

/// Render a toggle indicator row.
fn render_toggle_row(tree: &mut RenderTree, label: &str, enabled: bool, x: f32, y: f32) {
    // Toggle track
    let track_w = 36.0;
    let track_h = 18.0;
    let track_color = if enabled { GREEN } else { SURFACE1 };

    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: track_w,
        height: track_h,
        color: track_color,
        corner_radii: CornerRadii::all(9.0),
    });

    // Toggle knob
    let knob_x = if enabled {
        x + track_w - track_h + 2.0
    } else {
        x + 2.0
    };
    tree.push(RenderCommand::FillRect {
        x: knob_x,
        y: y + 2.0,
        width: track_h - 4.0,
        height: track_h - 4.0,
        color: TEXT_COLOR,
        corner_radii: CornerRadii::all(7.0),
    });

    // Label
    tree.push(RenderCommand::Text {
        x: x + track_w + 10.0,
        y: y + 2.0,
        text: label.to_string(),
        color: TEXT_COLOR,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

/// Render a standard-sized button.
fn render_button(
    tree: &mut RenderTree,
    label: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    accent: Color,
) {
    // Button background with muted accent
    let bg = Color::rgba(accent.r / 3, accent.g / 3, accent.b / 3, 200);
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: bg,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::StrokeRect {
        x,
        y,
        width: w,
        height: h,
        color: accent,
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });

    // Approximate center text
    let text_w = label.len() as f32 * 7.0;
    let text_x = x + (w - text_w) / 2.0;
    let text_y = y + (h - 12.0) / 2.0;
    tree.push(RenderCommand::Text {
        x: text_x,
        y: text_y,
        text: label.to_string(),
        color: TEXT_COLOR,
        font_size: 12.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(w - 8.0),
    });
}

/// Render a small inline button (for DNS reorder/remove).
fn render_mini_button(tree: &mut RenderTree, label: &str, x: f32, y: f32, color: Color) {
    let size = 20.0;
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: size,
        height: size,
        color: SURFACE1,
        corner_radii: CornerRadii::all(3.0),
    });
    tree.push(RenderCommand::Text {
        x: x + 5.0,
        y: y + 4.0,
        text: label.to_string(),
        color,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
}

/// Render a "no interface selected" placeholder.
fn render_no_selection(tree: &mut RenderTree, px: f32, py: f32, pw: f32) {
    tree.push(RenderCommand::Text {
        x: px + pw / 2.0 - 80.0,
        y: py + 40.0,
        text: "No interface selected".into(),
        color: OVERLAY0,
        font_size: 14.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

// ============================================================================
// Validation Helpers
// ============================================================================

/// Check whether a string looks like a valid IPv4 address.
fn is_valid_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    for part in &parts {
        match part.parse::<u16>() {
            Ok(n) if n <= 255 => {}
            _ => return false,
        }
    }
    true
}

// ============================================================================
// Sample Data Generators
// ============================================================================

fn sample_interfaces() -> Vec<NetworkInterface> {
    vec![
        NetworkInterface {
            id: 1,
            name: "Ethernet 1".into(),
            interface_type: InterfaceType::Ethernet,
            mac_address: "00:1A:2B:3C:4D:5E".into(),
            ip_config: IpConfig {
                ip_address: "192.168.1.100".into(),
                subnet_mask: "255.255.255.0".into(),
                gateway: "192.168.1.1".into(),
                dns_servers: vec!["8.8.8.8".into(), "8.8.4.4".into(), "1.1.1.1".into()],
                dhcp_enabled: true,
            },
            state: ConnectionState::Connected,
            speed_mbps: Some(1000),
            rx_bytes: 2_457_600_000,
            tx_bytes: 384_000_000,
            enabled: true,
        },
        NetworkInterface {
            id: 2,
            name: "Wi-Fi".into(),
            interface_type: InterfaceType::WiFi,
            mac_address: "AA:BB:CC:DD:EE:FF".into(),
            ip_config: IpConfig {
                ip_address: "192.168.1.101".into(),
                subnet_mask: "255.255.255.0".into(),
                gateway: "192.168.1.1".into(),
                dns_servers: vec!["8.8.8.8".into()],
                dhcp_enabled: true,
            },
            state: ConnectionState::Connected,
            speed_mbps: Some(300),
            rx_bytes: 1_024_000_000,
            tx_bytes: 128_000_000,
            enabled: true,
        },
        NetworkInterface {
            id: 3,
            name: "VPN Tunnel".into(),
            interface_type: InterfaceType::VPN,
            mac_address: String::new(),
            ip_config: IpConfig {
                ip_address: "10.0.0.5".into(),
                subnet_mask: "255.255.255.0".into(),
                gateway: "10.0.0.1".into(),
                dns_servers: vec!["10.0.0.1".into()],
                dhcp_enabled: false,
            },
            state: ConnectionState::Disconnected,
            speed_mbps: None,
            rx_bytes: 0,
            tx_bytes: 0,
            enabled: true,
        },
        NetworkInterface {
            id: 4,
            name: "br0".into(),
            interface_type: InterfaceType::Bridge,
            mac_address: "02:00:00:00:00:01".into(),
            ip_config: IpConfig {
                ip_address: "10.10.0.1".into(),
                subnet_mask: "255.255.0.0".into(),
                gateway: String::new(),
                dns_servers: Vec::new(),
                dhcp_enabled: false,
            },
            state: ConnectionState::Connected,
            speed_mbps: Some(10000),
            rx_bytes: 50_000_000,
            tx_bytes: 50_000_000,
            enabled: true,
        },
        NetworkInterface {
            id: 5,
            name: "lo".into(),
            interface_type: InterfaceType::Loopback,
            mac_address: "00:00:00:00:00:00".into(),
            ip_config: IpConfig {
                ip_address: "127.0.0.1".into(),
                subnet_mask: "255.0.0.0".into(),
                gateway: String::new(),
                dns_servers: Vec::new(),
                dhcp_enabled: false,
            },
            state: ConnectionState::Connected,
            speed_mbps: None,
            rx_bytes: 10_000_000,
            tx_bytes: 10_000_000,
            enabled: true,
        },
        NetworkInterface {
            id: 6,
            name: "veth0".into(),
            interface_type: InterfaceType::Virtual,
            mac_address: "FE:ED:FA:CE:00:01".into(),
            ip_config: IpConfig {
                ip_address: "172.17.0.1".into(),
                subnet_mask: "255.255.0.0".into(),
                gateway: String::new(),
                dns_servers: Vec::new(),
                dhcp_enabled: false,
            },
            state: ConnectionState::Disconnected,
            speed_mbps: None,
            rx_bytes: 0,
            tx_bytes: 0,
            enabled: false,
        },
    ]
}

fn sample_wifi_networks() -> Vec<WiFiNetwork> {
    vec![
        WiFiNetwork {
            ssid: "HomeNetwork".into(),
            signal_strength: 85,
            security_type: "WPA3".into(),
            channel: 6,
            frequency_ghz: 2.437,
        },
        WiFiNetwork {
            ssid: "OfficeWiFi-5G".into(),
            signal_strength: 72,
            security_type: "WPA2-Enterprise".into(),
            channel: 36,
            frequency_ghz: 5.180,
        },
        WiFiNetwork {
            ssid: "CoffeeShop".into(),
            signal_strength: 45,
            security_type: "WPA2".into(),
            channel: 11,
            frequency_ghz: 2.462,
        },
        WiFiNetwork {
            ssid: "Neighbor-Net".into(),
            signal_strength: 30,
            security_type: "WPA2".into(),
            channel: 1,
            frequency_ghz: 2.412,
        },
        WiFiNetwork {
            ssid: "FreeWiFi".into(),
            signal_strength: 15,
            security_type: "Open".into(),
            channel: 9,
            frequency_ghz: 2.452,
        },
    ]
}

fn sample_vpn_configs() -> Vec<VpnConfig> {
    vec![
        VpnConfig {
            name: "Work VPN".into(),
            server_address: "vpn.company.com".into(),
            protocol: VpnProtocol::WireGuard,
            auto_connect: false,
        },
        VpnConfig {
            name: "Privacy VPN".into(),
            server_address: "us-east.privatevpn.net".into(),
            protocol: VpnProtocol::OpenVPN,
            auto_connect: true,
        },
        VpnConfig {
            name: "Site-to-Site".into(),
            server_address: "gateway.branch-office.local".into(),
            protocol: VpnProtocol::IPSec,
            auto_connect: false,
        },
    ]
}

fn sample_profiles() -> Vec<NetworkProfile> {
    vec![
        NetworkProfile {
            name: "Home".into(),
            security_level: SecurityLevel::Private,
            firewall_enabled: true,
        },
        NetworkProfile {
            name: "Office".into(),
            security_level: SecurityLevel::Domain,
            firewall_enabled: true,
        },
        NetworkProfile {
            name: "Public Hotspot".into(),
            security_level: SecurityLevel::Public,
            firewall_enabled: true,
        },
    ]
}

fn sample_throughput_history() -> VecDeque<ThroughputSample> {
    let mut history = VecDeque::with_capacity(60);
    // Simulate varying throughput over time
    for i in 0..30 {
        let phase = i as f64 * 0.3;
        let rx = 500_000.0 + 400_000.0 * phase.sin().abs();
        let tx = 100_000.0 + 80_000.0 * (phase * 1.5).cos().abs();
        history.push_back(ThroughputSample {
            rx_bytes_per_sec: rx,
            tx_bytes_per_sec: tx,
        });
    }
    history
}

// ============================================================================
// Entry Point
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- InterfaceType tests ---

    #[test]
    fn test_interface_type_label() {
        assert_eq!(InterfaceType::Ethernet.label(), "Ethernet");
        assert_eq!(InterfaceType::WiFi.label(), "Wi-Fi");
        assert_eq!(InterfaceType::VPN.label(), "VPN");
        assert_eq!(InterfaceType::Bridge.label(), "Bridge");
        assert_eq!(InterfaceType::Loopback.label(), "Loopback");
        assert_eq!(InterfaceType::Virtual.label(), "Virtual");
    }

    #[test]
    fn test_interface_type_indicator_colors_are_distinct() {
        let types = [
            InterfaceType::Ethernet,
            InterfaceType::WiFi,
            InterfaceType::VPN,
            InterfaceType::Bridge,
            InterfaceType::Loopback,
            InterfaceType::Virtual,
        ];
        // Each type should have a unique color
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                let c1 = types[i].indicator_color();
                let c2 = types[j].indicator_color();
                assert!(
                    c1 != c2,
                    "{:?} and {:?} should have different colors",
                    types[i],
                    types[j],
                );
            }
        }
    }

    // --- ConnectionState tests ---

    #[test]
    fn test_connection_state_label() {
        assert_eq!(ConnectionState::Connected.label(), "Connected");
        assert_eq!(ConnectionState::Disconnected.label(), "Disconnected");
        assert_eq!(ConnectionState::Connecting.label(), "Connecting...");
        assert_eq!(ConnectionState::Error("fail".into()).label(), "Error",);
    }

    #[test]
    fn test_connection_state_is_connected() {
        assert!(ConnectionState::Connected.is_connected());
        assert!(!ConnectionState::Disconnected.is_connected());
        assert!(!ConnectionState::Connecting.is_connected());
        assert!(!ConnectionState::Error("x".into()).is_connected());
    }

    #[test]
    fn test_connection_state_colors_differ() {
        let c = ConnectionState::Connected.color();
        let d = ConnectionState::Disconnected.color();
        let e = ConnectionState::Error("x".into()).color();
        assert_ne!(c, d);
        assert_ne!(c, e);
    }

    // --- IpConfig tests ---

    #[test]
    fn test_ip_config_default() {
        let cfg = IpConfig::default();
        assert!(cfg.dhcp_enabled);
        assert!(cfg.ip_address.is_empty());
        assert_eq!(cfg.subnet_mask, "255.255.255.0");
        assert!(cfg.dns_servers.is_empty());
    }

    #[test]
    fn test_ip_config_validate_dhcp_ok() {
        let cfg = IpConfig {
            dhcp_enabled: true,
            ..IpConfig::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_ip_config_validate_static_missing_ip() {
        let cfg = IpConfig {
            dhcp_enabled: false,
            ip_address: String::new(),
            ..IpConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_ip_config_validate_static_invalid_ip() {
        let cfg = IpConfig {
            dhcp_enabled: false,
            ip_address: "999.999.999.999".into(),
            subnet_mask: "255.255.255.0".into(),
            ..IpConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_ip_config_validate_static_valid() {
        let cfg = IpConfig {
            dhcp_enabled: false,
            ip_address: "192.168.1.100".into(),
            subnet_mask: "255.255.255.0".into(),
            gateway: "192.168.1.1".into(),
            dns_servers: vec!["8.8.8.8".into()],
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_ip_config_validate_bad_dns() {
        let cfg = IpConfig {
            dhcp_enabled: true,
            dns_servers: vec!["not-an-ip".into()],
            ..IpConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_ip_config_validate_bad_gateway() {
        let cfg = IpConfig {
            dhcp_enabled: false,
            ip_address: "10.0.0.1".into(),
            subnet_mask: "255.255.255.0".into(),
            gateway: "bad".into(),
            dns_servers: Vec::new(),
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_ip_config_validate_empty_gateway_ok() {
        let cfg = IpConfig {
            dhcp_enabled: false,
            ip_address: "10.0.0.1".into(),
            subnet_mask: "255.255.255.0".into(),
            gateway: String::new(),
            dns_servers: Vec::new(),
        };
        assert!(cfg.validate().is_ok());
    }

    // --- WiFiNetwork tests ---

    #[test]
    fn test_wifi_signal_bars_zero() {
        let net = WiFiNetwork {
            ssid: "test".into(),
            signal_strength: 10,
            security_type: "WPA2".into(),
            channel: 1,
            frequency_ghz: 2.4,
        };
        assert_eq!(net.signal_bars(), 0);
    }

    #[test]
    fn test_wifi_signal_bars_max() {
        let net = WiFiNetwork {
            ssid: "test".into(),
            signal_strength: 95,
            security_type: "WPA2".into(),
            channel: 1,
            frequency_ghz: 2.4,
        };
        assert_eq!(net.signal_bars(), 4);
    }

    #[test]
    fn test_wifi_signal_bars_mid() {
        let net = WiFiNetwork {
            ssid: "test".into(),
            signal_strength: 50,
            security_type: "WPA2".into(),
            channel: 1,
            frequency_ghz: 2.4,
        };
        assert_eq!(net.signal_bars(), 2);
    }

    #[test]
    fn test_wifi_band_label_2g() {
        let net = WiFiNetwork {
            ssid: "t".into(),
            signal_strength: 50,
            security_type: "WPA2".into(),
            channel: 6,
            frequency_ghz: 2.437,
        };
        assert_eq!(net.band_label(), "2.4 GHz");
    }

    #[test]
    fn test_wifi_band_label_5g() {
        let net = WiFiNetwork {
            ssid: "t".into(),
            signal_strength: 50,
            security_type: "WPA2".into(),
            channel: 36,
            frequency_ghz: 5.180,
        };
        assert_eq!(net.band_label(), "5 GHz");
    }

    // --- NetworkInterface tests ---

    #[test]
    fn test_format_bytes_small() {
        assert_eq!(NetworkInterface::format_bytes(0), "0 B");
        assert_eq!(NetworkInterface::format_bytes(512), "512 B");
    }

    #[test]
    fn test_format_bytes_kb() {
        assert_eq!(NetworkInterface::format_bytes(1024), "1.0 KB");
        assert_eq!(NetworkInterface::format_bytes(2048), "2.0 KB");
    }

    #[test]
    fn test_format_bytes_mb() {
        assert_eq!(NetworkInterface::format_bytes(1_048_576), "1.0 MB");
    }

    #[test]
    fn test_format_bytes_gb() {
        assert_eq!(NetworkInterface::format_bytes(1_073_741_824), "1.00 GB");
    }

    #[test]
    fn test_status_summary_connected() {
        let iface = NetworkInterface {
            id: 1,
            name: "eth0".into(),
            interface_type: InterfaceType::Ethernet,
            mac_address: "AA:BB:CC:DD:EE:FF".into(),
            ip_config: IpConfig {
                ip_address: "10.0.0.1".into(),
                ..IpConfig::default()
            },
            state: ConnectionState::Connected,
            speed_mbps: Some(100),
            rx_bytes: 0,
            tx_bytes: 0,
            enabled: true,
        };
        let summary = iface.status_summary();
        assert!(summary.contains("eth0"));
        assert!(summary.contains("10.0.0.1"));
    }

    #[test]
    fn test_status_summary_disconnected() {
        let iface = NetworkInterface {
            id: 1,
            name: "wlan0".into(),
            interface_type: InterfaceType::WiFi,
            mac_address: "AA:BB:CC:DD:EE:FF".into(),
            ip_config: IpConfig::default(),
            state: ConnectionState::Disconnected,
            speed_mbps: None,
            rx_bytes: 0,
            tx_bytes: 0,
            enabled: true,
        };
        let summary = iface.status_summary();
        assert!(summary.contains("wlan0"));
        assert!(summary.contains("Disconnected"));
    }

    // --- VpnProtocol tests ---

    #[test]
    fn test_vpn_protocol_labels() {
        assert_eq!(VpnProtocol::OpenVPN.label(), "OpenVPN");
        assert_eq!(VpnProtocol::WireGuard.label(), "WireGuard");
        assert_eq!(VpnProtocol::IPSec.label(), "IPSec");
    }

    // --- SecurityLevel tests ---

    #[test]
    fn test_security_level_labels() {
        assert_eq!(SecurityLevel::Private.label(), "Private");
        assert_eq!(SecurityLevel::Public.label(), "Public");
        assert_eq!(SecurityLevel::Domain.label(), "Domain");
    }

    #[test]
    fn test_security_level_colors_differ() {
        let p = SecurityLevel::Private.color();
        let pub_c = SecurityLevel::Public.color();
        let d = SecurityLevel::Domain.color();
        assert_ne!(p, pub_c);
        assert_ne!(p, d);
        assert_ne!(pub_c, d);
    }

    // --- DiagnosticStatus tests ---

    #[test]
    fn test_diagnostic_status_labels() {
        assert_eq!(DiagnosticStatus::Passed.label(), "Passed");
        assert_eq!(DiagnosticStatus::Warning.label(), "Warning");
        assert_eq!(DiagnosticStatus::Failed.label(), "Failed");
        assert_eq!(DiagnosticStatus::Running.label(), "Running");
    }

    #[test]
    fn test_diagnostic_status_colors_differ() {
        let statuses = [
            DiagnosticStatus::Passed,
            DiagnosticStatus::Warning,
            DiagnosticStatus::Failed,
            DiagnosticStatus::Running,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(
                    statuses[i].color(),
                    statuses[j].color(),
                    "{:?} and {:?} should differ",
                    statuses[i],
                    statuses[j],
                );
            }
        }
    }

    // --- DetailTab tests ---

    #[test]
    fn test_detail_tab_labels() {
        assert_eq!(DetailTab::Properties.label(), "Properties");
        assert_eq!(DetailTab::IpConfig.label(), "IP Config");
        assert_eq!(DetailTab::Dns.label(), "DNS");
        assert_eq!(DetailTab::WiFi.label(), "Wi-Fi");
        assert_eq!(DetailTab::Vpn.label(), "VPN");
        assert_eq!(DetailTab::Profiles.label(), "Profiles");
        assert_eq!(DetailTab::Traffic.label(), "Traffic");
        assert_eq!(DetailTab::Diagnostics.label(), "Diagnostics");
    }

    #[test]
    fn test_detail_tab_all_count() {
        assert_eq!(DetailTab::all().len(), 8);
    }

    // --- NetManagerApp tests ---

    #[test]
    fn test_app_new_has_interfaces() {
        let app = NetManagerApp::new();
        assert!(!app.interfaces.is_empty());
    }

    #[test]
    fn test_app_default_selected_interface() {
        let app = NetManagerApp::new();
        assert_eq!(app.selected_interface, 0);
        assert!(app.selected_iface().is_some());
    }

    #[test]
    fn test_select_interface_valid() {
        let mut app = NetManagerApp::new();
        app.select_interface(1);
        assert_eq!(app.selected_interface, 1);
    }

    #[test]
    fn test_select_interface_out_of_bounds() {
        let mut app = NetManagerApp::new();
        app.select_interface(999);
        // Should not change
        assert_eq!(app.selected_interface, 0);
    }

    #[test]
    fn test_toggle_enabled() {
        let mut app = NetManagerApp::new();
        let was_enabled = app.interfaces[0].enabled;
        app.toggle_selected_enabled();
        assert_ne!(app.interfaces[0].enabled, was_enabled);
    }

    #[test]
    fn test_toggle_enabled_disconnects() {
        let mut app = NetManagerApp::new();
        // First interface is connected and enabled
        app.toggle_selected_enabled();
        assert!(!app.interfaces[0].enabled);
        assert_eq!(app.interfaces[0].state, ConnectionState::Disconnected);
    }

    #[test]
    fn test_toggle_enabled_reconnects() {
        let mut app = NetManagerApp::new();
        app.interfaces[0].enabled = false;
        app.interfaces[0].state = ConnectionState::Disconnected;
        app.toggle_selected_enabled();
        assert!(app.interfaces[0].enabled);
        assert_eq!(app.interfaces[0].state, ConnectionState::Connecting);
    }

    #[test]
    fn test_start_editing_ip() {
        let mut app = NetManagerApp::new();
        app.start_editing_ip();
        assert!(app.editing_ip);
    }

    #[test]
    fn test_cancel_editing_ip() {
        let mut app = NetManagerApp::new();
        app.start_editing_ip();
        app.edit_ip_config.ip_address = "changed".into();
        app.cancel_editing_ip();
        assert!(!app.editing_ip);
        // Should revert to original
        assert_ne!(app.edit_ip_config.ip_address, "changed");
    }

    #[test]
    fn test_apply_ip_config_valid() {
        let mut app = NetManagerApp::new();
        app.start_editing_ip();
        app.edit_ip_config.dhcp_enabled = false;
        app.edit_ip_config.ip_address = "10.0.0.50".into();
        app.edit_ip_config.subnet_mask = "255.255.255.0".into();
        app.edit_ip_config.gateway = "10.0.0.1".into();
        assert!(app.apply_ip_config().is_ok());
        assert!(!app.editing_ip);
        assert_eq!(app.interfaces[0].ip_config.ip_address, "10.0.0.50");
    }

    #[test]
    fn test_apply_ip_config_invalid() {
        let mut app = NetManagerApp::new();
        app.start_editing_ip();
        app.edit_ip_config.dhcp_enabled = false;
        app.edit_ip_config.ip_address = "bad".into();
        assert!(app.apply_ip_config().is_err());
    }

    #[test]
    fn test_add_dns_server_valid() {
        let mut app = NetManagerApp::new();
        let before = app.edit_ip_config.dns_servers.len();
        // Use an address NOT already in the default config (which seeds 8.8.8.8,
        // 8.8.4.4 and 1.1.1.1); add_dns_server correctly rejects duplicates.
        assert!(app.add_dns_server("9.9.9.9").is_ok());
        assert_eq!(app.edit_ip_config.dns_servers.len(), before + 1);
    }

    #[test]
    fn test_add_dns_server_empty() {
        let mut app = NetManagerApp::new();
        assert!(app.add_dns_server("").is_err());
    }

    #[test]
    fn test_add_dns_server_invalid() {
        let mut app = NetManagerApp::new();
        assert!(app.add_dns_server("not.valid.ip.addr").is_err());
    }

    #[test]
    fn test_add_dns_server_duplicate() {
        let mut app = NetManagerApp::new();
        // 8.8.8.8 is already in the default list
        assert!(app.add_dns_server("8.8.8.8").is_err());
    }

    #[test]
    fn test_remove_dns_server() {
        let mut app = NetManagerApp::new();
        let before = app.edit_ip_config.dns_servers.len();
        assert!(app.remove_dns_server(0).is_ok());
        assert_eq!(app.edit_ip_config.dns_servers.len(), before - 1);
    }

    #[test]
    fn test_remove_dns_server_out_of_bounds() {
        let mut app = NetManagerApp::new();
        assert!(app.remove_dns_server(999).is_err());
    }

    #[test]
    fn test_move_dns_up() {
        let mut app = NetManagerApp::new();
        let second = app.edit_ip_config.dns_servers[1].clone();
        assert!(app.move_dns_up(1).is_ok());
        assert_eq!(app.edit_ip_config.dns_servers[0], second);
    }

    #[test]
    fn test_move_dns_up_at_top() {
        let mut app = NetManagerApp::new();
        assert!(app.move_dns_up(0).is_err());
    }

    #[test]
    fn test_move_dns_down() {
        let mut app = NetManagerApp::new();
        let first = app.edit_ip_config.dns_servers[0].clone();
        assert!(app.move_dns_down(0).is_ok());
        assert_eq!(app.edit_ip_config.dns_servers[1], first);
    }

    #[test]
    fn test_move_dns_down_at_bottom() {
        let mut app = NetManagerApp::new();
        let last = app.edit_ip_config.dns_servers.len() - 1;
        assert!(app.move_dns_down(last).is_err());
    }

    #[test]
    fn test_select_wifi() {
        let mut app = NetManagerApp::new();
        app.select_wifi(2);
        assert_eq!(app.selected_wifi, Some(2));
    }

    #[test]
    fn test_connect_wifi_no_selection() {
        let mut app = NetManagerApp::new();
        app.selected_wifi = None;
        assert!(app.connect_wifi().is_err());
    }

    #[test]
    fn test_connect_wifi_valid() {
        let mut app = NetManagerApp::new();
        // Select the WiFi interface
        app.select_interface(1);
        app.select_wifi(0);
        let result = app.connect_wifi();
        assert!(result.is_ok());
        assert_eq!(result.ok(), Some("HomeNetwork".into()));
    }

    #[test]
    fn test_toggle_vpn_connect() {
        let mut app = NetManagerApp::new();
        // VPN 0 is disconnected
        assert!(app.toggle_vpn(0).is_ok());
        assert_eq!(app.vpn_states[0], ConnectionState::Connecting);
    }

    #[test]
    fn test_toggle_vpn_disconnect() {
        let mut app = NetManagerApp::new();
        // VPN 1 is connected
        assert!(app.toggle_vpn(1).is_ok());
        assert_eq!(app.vpn_states[1], ConnectionState::Disconnected);
    }

    #[test]
    fn test_toggle_vpn_out_of_bounds() {
        let mut app = NetManagerApp::new();
        assert!(app.toggle_vpn(999).is_err());
    }

    #[test]
    fn test_run_diagnostics() {
        let mut app = NetManagerApp::new();
        assert!(app.diagnostics.is_empty());
        app.run_diagnostics();
        assert!(!app.diagnostics.is_empty());
        assert!(!app.diagnostics_running);
    }

    #[test]
    fn test_push_throughput() {
        let mut app = NetManagerApp::new();
        let before = app.throughput_history.len();
        app.push_throughput(ThroughputSample {
            rx_bytes_per_sec: 100.0,
            tx_bytes_per_sec: 50.0,
        });
        assert_eq!(app.throughput_history.len(), before + 1);
    }

    #[test]
    fn test_push_throughput_caps_at_max() {
        let mut app = NetManagerApp::new();
        app.max_throughput_samples = 5;
        app.throughput_history.clear();
        for i in 0..10 {
            app.push_throughput(ThroughputSample {
                rx_bytes_per_sec: i as f64,
                tx_bytes_per_sec: 0.0,
            });
        }
        assert_eq!(app.throughput_history.len(), 5);
    }

    #[test]
    fn test_set_tab() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::WiFi);
        assert_eq!(app.active_tab, DetailTab::WiFi);
    }

    #[test]
    fn test_add_profile() {
        let mut app = NetManagerApp::new();
        let before = app.profiles.len();
        app.add_profile("Test", SecurityLevel::Public, true);
        assert_eq!(app.profiles.len(), before + 1);
        assert_eq!(app.profiles.last().map(|p| p.name.as_str()), Some("Test"));
    }

    #[test]
    fn test_remove_profile() {
        let mut app = NetManagerApp::new();
        let before = app.profiles.len();
        assert!(app.remove_profile(0).is_ok());
        assert_eq!(app.profiles.len(), before - 1);
    }

    #[test]
    fn test_remove_profile_out_of_bounds() {
        let mut app = NetManagerApp::new();
        assert!(app.remove_profile(999).is_err());
    }

    #[test]
    fn test_remove_profile_adjusts_selection() {
        let mut app = NetManagerApp::new();
        app.selected_profile = Some(2);
        // Remove last, selection should adjust
        let last = app.profiles.len() - 1;
        assert!(app.remove_profile(last).is_ok());
        if let Some(sel) = app.selected_profile {
            assert!(sel < app.profiles.len());
        }
    }

    // --- is_valid_ipv4 tests ---

    #[test]
    fn test_valid_ipv4() {
        assert!(is_valid_ipv4("192.168.1.1"));
        assert!(is_valid_ipv4("0.0.0.0"));
        assert!(is_valid_ipv4("255.255.255.255"));
        assert!(is_valid_ipv4("10.0.0.1"));
    }

    #[test]
    fn test_invalid_ipv4() {
        assert!(!is_valid_ipv4(""));
        assert!(!is_valid_ipv4("abc"));
        assert!(!is_valid_ipv4("256.0.0.1"));
        assert!(!is_valid_ipv4("1.2.3"));
        assert!(!is_valid_ipv4("1.2.3.4.5"));
        assert!(!is_valid_ipv4("1.2.3.abc"));
    }

    // --- Rendering tests ---

    #[test]
    fn test_render_app_produces_commands() {
        let app = NetManagerApp::new();
        let tree = render_app(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_has_title_bar() {
        let app = NetManagerApp::new();
        let tree = render_app(&app);
        // Should contain the title text
        let has_title = tree.commands.iter().any(
            |cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "Network Connections"),
        );
        assert!(has_title);
    }

    #[test]
    fn test_render_has_interface_names() {
        let app = NetManagerApp::new();
        let tree = render_app(&app);
        let has_eth = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "Ethernet 1"));
        assert!(has_eth);
    }

    #[test]
    fn test_render_different_tabs() {
        let mut app = NetManagerApp::new();
        for tab in DetailTab::all() {
            app.set_tab(*tab);
            let tree = render_app(&app);
            // Each tab should produce some render commands
            assert!(
                tree.commands.len() > 10,
                "Tab {:?} produced too few commands",
                tab,
            );
        }
    }

    #[test]
    fn test_render_with_diagnostics() {
        let mut app = NetManagerApp::new();
        app.run_diagnostics();
        app.set_tab(DetailTab::Diagnostics);
        let tree = render_app(&app);
        let has_ping = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Ping")));
        assert!(has_ping);
    }

    // --- Sample data tests ---

    #[test]
    fn test_sample_interfaces_complete() {
        let ifaces = sample_interfaces();
        assert!(ifaces.len() >= 4);
        // Should have at least one of each major type
        let has_eth = ifaces
            .iter()
            .any(|i| i.interface_type == InterfaceType::Ethernet);
        let has_wifi = ifaces
            .iter()
            .any(|i| i.interface_type == InterfaceType::WiFi);
        let has_vpn = ifaces
            .iter()
            .any(|i| i.interface_type == InterfaceType::VPN);
        assert!(has_eth);
        assert!(has_wifi);
        assert!(has_vpn);
    }

    #[test]
    fn test_sample_wifi_networks_have_ssids() {
        let nets = sample_wifi_networks();
        assert!(!nets.is_empty());
        for net in &nets {
            assert!(!net.ssid.is_empty());
            assert!(net.signal_strength <= 100);
        }
    }

    #[test]
    fn test_sample_vpn_configs_nonempty() {
        let vpns = sample_vpn_configs();
        assert!(!vpns.is_empty());
        for vpn in &vpns {
            assert!(!vpn.name.is_empty());
            assert!(!vpn.server_address.is_empty());
        }
    }

    #[test]
    fn test_sample_profiles_nonempty() {
        let profiles = sample_profiles();
        assert!(!profiles.is_empty());
    }

    #[test]
    fn test_sample_throughput_history() {
        let hist = sample_throughput_history();
        assert!(!hist.is_empty());
        for sample in &hist {
            assert!(sample.rx_bytes_per_sec >= 0.0);
            assert!(sample.tx_bytes_per_sec >= 0.0);
        }
    }
}

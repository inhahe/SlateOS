//! Slate OS Network Connections Manager
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
//! Network I/O is performed through Slate OS syscalls; simulated with
//! representative data for initial development.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::scroll_window;
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::collections::VecDeque;
use std::process::ExitCode;

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

/// The size the window asks for. Not the size it draws at: the compositor is
/// free to hand back something else, and the first frame goes out before any
/// `Event::Resize` arrives, so every renderer below takes its width and height
/// from the [`Frame`] it is given rather than from these.
const WINDOW_WIDTH: f32 = 960.0;
const WINDOW_HEIGHT: f32 = 680.0;
/// Below this the sidebar and the detail panel would each be unusable, so the
/// window stops shrinking the layout and lets the compositor clip instead.
const MIN_WIDTH: f32 = 640.0;
/// Enough for the chrome (title, toolbar, tab strip, status bar) plus one row.
const MIN_HEIGHT: f32 = 320.0;
const TITLE_BAR_HEIGHT: f32 = 40.0;
const TOOLBAR_HEIGHT: f32 = 36.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const SIDEBAR_ITEM_HEIGHT: f32 = 52.0;
const SECTION_PADDING: f32 = 16.0;
const FIELD_HEIGHT: f32 = 28.0;
const FIELD_LABEL_WIDTH: f32 = 120.0;
/// Width of a text input box. Named because it is now the *click* width too,
/// not just the painted one, so a change here moves both together.
const FIELD_INPUT_WIDTH: f32 = 200.0;
/// Side of the square reorder/remove buttons in lists.
const MINI_BUTTON_SIZE: f32 = 20.0;
const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_WIDTH: f32 = 100.0;
const WIFI_ITEM_HEIGHT: f32 = 40.0;
const VPN_ITEM_HEIGHT: f32 = 44.0;
/// Point size of the second line of a diagnostics row.
const DIAG_DETAIL_FONT_SIZE: f32 = 10.0;
const GRAPH_BAR_WIDTH: f32 = 8.0;
const GRAPH_BAR_GAP: f32 = 2.0;
const TRAFFIC_GRAPH_HEIGHT: f32 = 100.0;
const DNS_ROW_HEIGHT: f32 = 28.0;
/// Vertical space the sidebar keeps for its "N more" line.
///
/// Reserved whether or not the line is drawn, so that how many interfaces fit
/// does not depend on how many interfaces fit.
const LIST_MORE_HEIGHT: f32 = 16.0;

/// Font size of a toolbar button's label.
const TOOLBAR_TEXT: f32 = 12.0;
/// Font size of a detail-pane tab's label.
const TAB_TEXT: f32 = 11.0;
/// Font size of a section heading.
const SECTION_TEXT: f32 = 13.0;

/// Width of the tab drawn for `tab`.
///
/// Measured bold whatever the tab's state, because the *active* tab is drawn
/// bold: sizing each tab to its current weight would reflow the whole strip
/// every time the selection moved.
fn tab_width(tab: DetailTab) -> f32 {
    text::measure(tab.label(), TAB_TEXT, FontWeightHint::Bold) + 16.0
}

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
    /// Format an interface's transfer counter.
    ///
    /// Decimal, not binary: this is the same `rx_bytes`/`tx_bytes` the tray
    /// indicator and the network settings page show, and two windows quoting
    /// one counter must not disagree about what it says. See
    /// design-decisions.md §489 -- bytes moved over a link are SI, bytes
    /// occupying storage are IEC.
    fn format_bytes(bytes: u64) -> String {
        guitk::bytes::si(bytes)
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
    /// Index of the first interface drawn in the sidebar.
    ///
    /// A request rather than an index: an offset left over from a longer list
    /// shows the last page instead of a blank sidebar, because
    /// [`scroll_window::visible`] clamps the *result* and leaves this alone.
    pub sidebar_scroll: usize,
    /// Which text field the keyboard is typing into, if any.
    ///
    /// `None` means keystrokes are navigation, not text. Kept as an explicit
    /// field rather than inferred from `editing_ip` because the DNS input is
    /// typeable on a tab where nothing is "being edited".
    pub focus: Option<Field>,
    /// Carries the fraction of a row a wheel event is worth.
    ///
    /// Rounding each event on its own would discard it, and a precision
    /// trackpad sends nothing but fractions — the list would never move.
    pub wheel: wheel::Accumulator,
    /// The size the last frame was drawn at.
    ///
    /// The hit-test re-renders to find what is under a click, and must do it
    /// at the size the user is actually looking at. Seeded with the requested
    /// size and corrected by the first `render`, which is handed the truth.
    pub window_size: (f32, f32),
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
            sidebar_scroll: 0,
            focus: None,
            wheel: wheel::Accumulator::default(),
            window_size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// Scroll the sidebar's interface list by `delta` rows.
    ///
    /// Only the top is clamped here; how far down the list can go depends on
    /// the sidebar's height, which the renderer knows and this does not. An
    /// offset run past the end is a request for the last page, not an error.
    pub fn scroll_sidebar_by(&mut self, delta: isize) {
        self.sidebar_scroll = scroll_window::shift(self.sidebar_scroll, delta);
    }

    /// Scroll the sidebar back to the first interface.
    pub fn scroll_sidebar_to_top(&mut self) {
        self.sidebar_scroll = 0;
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
        // `index` is known non-zero above, so the saturation never fires; it is
        // written this way so the subtraction cannot underflow even if the
        // guard above is ever changed.
        self.edit_ip_config
            .dns_servers
            .swap(index, index.saturating_sub(1));
        Ok(())
    }

    /// Move a DNS server down in priority.
    pub fn move_dns_down(&mut self, index: usize) -> Result<(), String> {
        // A row at `usize::MAX` cannot have one below it, so an overflow here
        // is the same answer as "already at bottom" rather than a panic.
        let below = index
            .checked_add(1)
            .ok_or_else(|| "Already at bottom".to_string())?;
        if below >= self.edit_ip_config.dns_servers.len() {
            return Err("Already at bottom".into());
        }
        self.edit_ip_config.dns_servers.swap(index, below);
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
        if let Some(iface) = self.interfaces.get_mut(self.selected_interface)
            && iface.interface_type == InterfaceType::WiFi
        {
            iface.state = ConnectionState::Connecting;
            self.status_message = format!("Connecting to {ssid}...");
        }
        Ok(ssid)
    }

    /// Toggle VPN connection state by index.
    pub fn toggle_vpn(&mut self, index: usize) -> Result<(), String> {
        // The bounds check and the write are the same lookup, so there is no
        // window in which the length could have been read from a different
        // vector than the one written to.
        let state = self
            .vpn_states
            .get_mut(index)
            .ok_or_else(|| "VPN index out of range".to_string())?;
        *state = if state.is_connected() {
            ConnectionState::Disconnected
        } else {
            ConnectionState::Connecting
        };
        let label = state.label();

        if let Some(vpn) = self.vpn_configs.get(index) {
            self.status_message = format!("VPN '{}' {label}", vpn.name);
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
                details: "Resolved slateos.local in 5ms".into(),
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
        if let Some(sel) = self.selected_profile
            && sel >= self.profiles.len()
        {
            // `checked_sub` on an empty list yields `None`, which is exactly the
            // answer wanted there: no profiles, no selection.
            self.selected_profile = self.profiles.len().checked_sub(1);
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
// Targets, and the frame that carries them
// ============================================================================

/// A text field that can hold the keyboard.
///
/// Without this the IP tab drew three input boxes that could be opened for
/// editing and then not typed into, and the DNS tab drew a text box whose only
/// possible content was the placeholder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Ip,
    Mask,
    Gateway,
    DnsInput,
}

/// What a click at a point means.
///
/// The rects these name are produced by the renderer as it draws, so there is
/// no second copy of the geometry to drift out of step with the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Refresh,
    Diagnose,
    ShowProperties,
    ToggleEnabled,
    Interface(usize),
    Tab(DetailTab),
    DhcpToggle,
    EditIp,
    ApplyIp,
    CancelIp,
    Focus(Field),
    DnsUp(usize),
    DnsDown(usize),
    DnsRemove(usize),
    DnsAdd,
    Wifi(usize),
    WifiConnect,
    Vpn(usize),
    Profile(usize),
    ProfileRemove(usize),
    ProfileAdd,
    RunDiagnostics,
}

/// A frame being built: the commands to draw, and the clickable rects that
/// drawing them created.
///
/// Rendering and hit-testing are the *same walk* — see [`guitk::frame`] for
/// why, and for how the clip in force around the detail panel trims what is
/// clickable. This alias is here so the renderers below can keep saying
/// `&mut Frame` without repeating the target type on every signature.
pub type Frame = guitk::frame::Frame<Target>;

/// A frame sized for a window of `width` by `height`, never smaller than the
/// minimum the layout is designed for.
///
/// Below that the panels overlap and the window is unusable, so the renderer
/// draws the smallest sensible layout and lets it clip rather than producing
/// negative widths.
fn new_frame(width: f32, height: f32) -> Frame {
    Frame::new(width.max(MIN_WIDTH), height.max(MIN_HEIGHT))
}

// ============================================================================
// Rendering
// ============================================================================

/// Draw the whole window at `width` by `height`, collecting as it goes every
/// rect a click could land on.
///
/// This is the only place the window's geometry exists. `hit_test` runs it and
/// reads `Frame::hits`; it does not recompute anything.
#[must_use]
pub fn render_frame(app: &NetManagerApp, width: f32, height: f32) -> Frame {
    let mut frame = new_frame(width, height);

    // Background
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: frame.width,
        height: frame.height,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    render_title_bar(&mut frame);
    render_toolbar(&mut frame, app);
    render_sidebar(&mut frame, app);
    render_detail_panel(&mut frame, app);
    render_status_bar(&mut frame, app);

    frame
}

/// Render the application at its default size.
///
/// A convenience for callers with no viewport to hand; the window itself goes
/// through [`render_frame`] with the size the compositor actually gave it.
#[must_use]
pub fn render_app(app: &NetManagerApp) -> RenderTree {
    render_frame(app, WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()
}

/// Render the title bar at the top of the window.
fn render_title_bar(frame: &mut Frame) {
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: frame.width,
        height: TITLE_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });
    frame.push(RenderCommand::Text {
        x: 14.0,
        y: 12.0,
        text: "Network Connections".into(),
        color: TEXT_COLOR,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// Render the toolbar below the title bar.
fn render_toolbar(frame: &mut Frame, app: &NetManagerApp) {
    let y = TITLE_BAR_HEIGHT;

    // Toolbar background
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width: frame.width,
        height: TOOLBAR_HEIGHT,
        color: SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    // Toolbar buttons
    let buttons = [
        ("Refresh", Target::Refresh),
        ("Diagnose", Target::Diagnose),
        ("Properties", Target::ShowProperties),
    ];
    let mut bx = 12.0;
    for (label, target) in &buttons {
        let bw = text::measure(label, TOOLBAR_TEXT, FontWeightHint::Regular) + 24.0;
        let rect = Rect::new(bx, y + 4.0, bw, TOOLBAR_HEIGHT - 8.0);
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.hit(*target, rect);
        frame.push(RenderCommand::Text {
            x: bx + 12.0,
            y: y + 10.0,
            text: label.to_string(),
            color: TEXT_COLOR,
            font_size: TOOLBAR_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        bx += bw + 8.0;
    }

    // Enable/Disable toggle on the right side
    let toggle_label = if app.selected_iface().is_some_and(|iface| iface.enabled) {
        "Disable"
    } else {
        "Enable"
    };
    let tw = text::measure(toggle_label, TOOLBAR_TEXT, FontWeightHint::Regular) + 24.0;
    let tx = frame.width - tw - 12.0;
    let toggle_rect = Rect::new(tx, y + 4.0, tw, TOOLBAR_HEIGHT - 8.0);
    frame.push(RenderCommand::FillRect {
        x: toggle_rect.x,
        y: toggle_rect.y,
        width: toggle_rect.w,
        height: toggle_rect.h,
        color: SURFACE1,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.hit(Target::ToggleEnabled, toggle_rect);
    frame.push(RenderCommand::Text {
        x: tx + 12.0,
        y: y + 10.0,
        text: toggle_label.into(),
        color: TEXT_COLOR,
        font_size: TOOLBAR_TEXT,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// Render the sidebar interface list.
fn render_sidebar(frame: &mut Frame, app: &NetManagerApp) {
    let sx = 0.0;
    let sy = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let sh = frame.height - sy - STATUS_BAR_HEIGHT;

    // Sidebar background
    frame.push(RenderCommand::FillRect {
        x: sx,
        y: sy,
        width: SIDEBAR_WIDTH,
        height: sh,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Sidebar header
    frame.push(RenderCommand::Text {
        x: sx + 12.0,
        y: sy + 10.0,
        text: "Interfaces".into(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Separator
    frame.push(RenderCommand::Line {
        x1: sx + 8.0,
        y1: sy + 28.0,
        x2: sx + SIDEBAR_WIDTH - 8.0,
        y2: sy + 28.0,
        color: SURFACE0,
        width: 1.0,
    });

    // Interface items.
    //
    // The list is bounded by the sidebar's own height, not by the window: the
    // status bar is drawn across the bottom, and the old loop — which had no
    // break of any kind — ran a long interface list straight through it and off
    // the screen, with no way to scroll the tail back into view.
    let list_y = sy + 32.0;
    let window = scroll_window::visible(
        app.interfaces.len(),
        SIDEBAR_ITEM_HEIGHT,
        sy + sh - list_y - LIST_MORE_HEIGHT,
        app.sidebar_scroll,
    );
    for (drawn, iface) in app
        .interfaces
        .get(window.start..window.end())
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        // The selection is an index into the whole list, so compare against the
        // absolute position, not the position on screen.
        let i = window.start.saturating_add(drawn);
        let item_y = list_y + drawn as f32 * SIDEBAR_ITEM_HEIGHT;
        let is_selected = i == app.selected_interface;

        // The clickable band is exactly the band the selection highlight
        // paints, so what the user sees light up when a row is selected is
        // what they have to hit to select it.
        let row = Rect::new(
            sx + 4.0,
            item_y,
            SIDEBAR_WIDTH - 8.0,
            SIDEBAR_ITEM_HEIGHT - 2.0,
        );
        frame.hit(Target::Interface(i), row);

        // Selection highlight
        if is_selected {
            frame.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });
        }

        // Type indicator circle
        let circle_x = sx + 16.0;
        let circle_y = item_y + SIDEBAR_ITEM_HEIGHT / 2.0 - 6.0;
        frame.push(RenderCommand::FillRect {
            x: circle_x,
            y: circle_y,
            width: 12.0,
            height: 12.0,
            color: iface.interface_type.indicator_color(),
            corner_radii: CornerRadii::all(6.0),
        });

        // Interface name
        frame.push(RenderCommand::Text {
            x: sx + 36.0,
            y: item_y + 8.0,
            text: iface.name.clone(),
            color: TEXT_COLOR,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 50.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Status line
        let status_text = format!("{} - {}", iface.interface_type.label(), iface.state.label());
        frame.push(RenderCommand::Text {
            x: sx + 36.0,
            y: item_y + 26.0,
            text: status_text,
            color: iface.state.color(),
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(SIDEBAR_WIDTH - 50.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Status dot (right side)
        frame.push(RenderCommand::FillRect {
            x: sx + SIDEBAR_WIDTH - 20.0,
            y: item_y + SIDEBAR_ITEM_HEIGHT / 2.0 - 4.0,
            width: 8.0,
            height: 8.0,
            color: iface.state.color(),
            corner_radii: CornerRadii::all(4.0),
        });
    }

    // A list that is hiding interfaces says so. Without this the sidebar is
    // indistinguishable from one showing everything there is, which is how a
    // truncated list goes unnoticed.
    let hidden = app.interfaces.len().saturating_sub(window.count);
    if hidden > 0 {
        frame.push(RenderCommand::Text {
            x: sx + 12.0,
            y: list_y + window.count as f32 * SIDEBAR_ITEM_HEIGHT,
            text: format!("{hidden} more"),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

/// Render the main detail panel area.
fn render_detail_panel(frame: &mut Frame, app: &NetManagerApp) {
    let px = SIDEBAR_WIDTH;
    let py = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let pw = frame.width - SIDEBAR_WIDTH;
    let ph = frame.height - py - STATUS_BAR_HEIGHT;

    // Panel background
    frame.push(RenderCommand::FillRect {
        x: px,
        y: py,
        width: pw,
        height: ph,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    // Tab bar
    render_tab_bar(frame, app, px, py, pw);

    // Tab content area
    let content_y = py + 32.0;
    let content_h = ph - 32.0;

    frame.push(RenderCommand::PushClip {
        x: px,
        y: content_y,
        width: pw,
        height: content_h,
    });

    match app.active_tab {
        DetailTab::Properties => render_tab_properties(frame, app, px, content_y, pw),
        DetailTab::IpConfig => render_tab_ip_config(frame, app, px, content_y, pw),
        DetailTab::Dns => render_tab_dns(frame, app, px, content_y, pw),
        DetailTab::WiFi => render_tab_wifi(frame, app, px, content_y, pw),
        DetailTab::Vpn => render_tab_vpn(frame, app, px, content_y, pw),
        DetailTab::Profiles => render_tab_profiles(frame, app, px, content_y, pw),
        DetailTab::Traffic => render_tab_traffic(frame, app, px, content_y, pw),
        DetailTab::Diagnostics => render_tab_diagnostics(frame, app, px, content_y, pw),
    }

    frame.push(RenderCommand::PopClip);
}

/// Render tab headers at the top of the detail panel.
fn render_tab_bar(frame: &mut Frame, app: &NetManagerApp, px: f32, py: f32, _pw: f32) {
    // Tab bar background
    frame.push(RenderCommand::FillRect {
        x: px,
        y: py,
        width: frame.width - px,
        height: 30.0,
        color: SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    let mut tx = px + 8.0;
    for tab in DetailTab::all() {
        let label = tab.label();
        let tw = tab_width(*tab);
        let is_active = *tab == app.active_tab;

        // The whole tab, not just its label, switches the panel — including
        // the padding either side of the text, which is what a user aims at.
        frame.hit(Target::Tab(*tab), Rect::new(tx, py + 2.0, tw, 26.0));

        if is_active {
            frame.push(RenderCommand::FillRect {
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
        frame.push(RenderCommand::Text {
            x: tx + 8.0,
            y: py + 9.0,
            text: label.to_string(),
            color: text_color,
            font_size: TAB_TEXT,
            font_weight: if is_active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        tx += tw + 4.0;
    }
}

/// Render the Properties tab content.
fn render_tab_properties(frame: &mut Frame, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let Some(iface) = app.selected_iface() else {
        render_no_selection(frame, px, py, pw);
        return;
    };

    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;
    let vx = lx + FIELD_LABEL_WIDTH;

    // Section title
    y = render_section_title(frame, "Interface Details", lx, y);

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
        render_field_row(frame, label, value, lx, vx, y);
        y += FIELD_HEIGHT + 4.0;
    }

    // Traffic section
    y += 12.0;
    y = render_section_title(frame, "Traffic Statistics", lx, y);

    let traffic_fields: &[(&str, String)] = &[
        ("Received:", NetworkInterface::format_bytes(iface.rx_bytes)),
        (
            "Transmitted:",
            NetworkInterface::format_bytes(iface.tx_bytes),
        ),
    ];

    for (label, value) in traffic_fields {
        render_field_row(frame, label, value, lx, vx, y);
        y += FIELD_HEIGHT + 4.0;
    }

    // IP summary
    y += 12.0;
    y = render_section_title(frame, "IP Configuration Summary", lx, y);

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
        render_field_row(frame, label, value, lx, vx, y);
        y += FIELD_HEIGHT + 4.0;
    }
}

/// Render the IP Config tab content.
fn render_tab_ip_config(frame: &mut Frame, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let Some(_iface) = app.selected_iface() else {
        render_no_selection(frame, px, py, pw);
        return;
    };

    let ip = &app.edit_ip_config;
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;
    let vx = lx + FIELD_LABEL_WIDTH;

    y = render_section_title(frame, "IP Configuration", lx, y);

    // DHCP toggle
    let dhcp_label = if ip.dhcp_enabled {
        "DHCP: Enabled"
    } else {
        "DHCP: Disabled (Static)"
    };
    let dhcp = render_toggle_row(frame, dhcp_label, ip.dhcp_enabled, lx, y);
    frame.hit(Target::DhcpToggle, dhcp);
    y += FIELD_HEIGHT + 8.0;

    // IP fields (dimmed if DHCP is on and not editing)
    let field_color = if ip.dhcp_enabled && !app.editing_ip {
        OVERLAY0
    } else {
        TEXT_COLOR
    };

    let ip_fields: &[(&str, &str, Field)] = &[
        ("IP Address:", &ip.ip_address, Field::Ip),
        ("Subnet Mask:", &ip.subnet_mask, Field::Mask),
        ("Gateway:", &ip.gateway, Field::Gateway),
    ];

    for (label, value, field) in ip_fields {
        let editing = app.editing_ip;
        let value = if editing && app.focus == Some(*field) {
            // The caret is drawn into the text rather than as a separate
            // command, so that a field with focus is distinguishable in the
            // render tree — which is the only thing a test can see.
            format!("{value}_")
        } else {
            (*value).to_string()
        };
        let box_rect = render_editable_field(frame, label, &value, lx, vx, y, field_color, editing);
        if editing {
            // Only while editing: outside edit mode the boxes are not drawn,
            // and a click target with nothing under it is a trap.
            frame.hit(Target::Focus(*field), box_rect);
        }
        y += FIELD_HEIGHT + 6.0;
    }

    // Buttons
    y += 12.0;
    if app.editing_ip {
        let apply = render_button(frame, "Apply", lx, y, BUTTON_WIDTH, BUTTON_HEIGHT, GREEN);
        frame.hit(Target::ApplyIp, apply);
        let cancel = render_button(
            frame,
            "Cancel",
            lx + BUTTON_WIDTH + 12.0,
            y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            RED,
        );
        frame.hit(Target::CancelIp, cancel);
    } else {
        let edit = render_button(frame, "Edit", lx, y, BUTTON_WIDTH, BUTTON_HEIGHT, BLUE);
        frame.hit(Target::EditIp, edit);
    }
}

/// Render the DNS tab content.
fn render_tab_dns(frame: &mut Frame, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let Some(_iface) = app.selected_iface() else {
        render_no_selection(frame, px, py, pw);
        return;
    };

    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(frame, "DNS Servers", lx, y);

    // DNS server list
    let dns = &app.edit_ip_config.dns_servers;
    if dns.is_empty() {
        frame.push(RenderCommand::Text {
            x: lx,
            y,
            text: "No DNS servers configured".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        y += DNS_ROW_HEIGHT;
    } else {
        for (i, server) in dns.iter().enumerate() {
            // Row background
            let row_bg = if i % 2 == 0 { SURFACE0 } else { BASE };
            frame.push(RenderCommand::FillRect {
                x: lx,
                y,
                width: pw - SECTION_PADDING * 2.0,
                height: DNS_ROW_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::all(3.0),
            });

            // Priority number
            frame.push(RenderCommand::Text {
                x: lx + 8.0,
                y: y + 7.0,
                text: format!("{}.", i.saturating_add(1)),
                color: SUBTEXT0,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Server address
            frame.push(RenderCommand::Text {
                x: lx + 32.0,
                y: y + 7.0,
                text: server.clone(),
                color: TEXT_COLOR,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Up/Down/Remove buttons (small)
            let btn_y = y + 3.0;
            let btn_x = lx + pw - SECTION_PADDING * 2.0 - 100.0;

            if i > 0 {
                let up = render_mini_button(frame, "^", btn_x, btn_y, BLUE);
                frame.hit(Target::DnsUp(i), up);
            }
            if i.saturating_add(1) < dns.len() {
                let down = render_mini_button(frame, "v", btn_x + 24.0, btn_y, BLUE);
                frame.hit(Target::DnsDown(i), down);
            }
            let remove = render_mini_button(frame, "X", btn_x + 48.0, btn_y, RED);
            frame.hit(Target::DnsRemove(i), remove);

            y += DNS_ROW_HEIGHT + 2.0;
        }
    }

    // Add DNS input
    y += 12.0;
    y = render_section_title(frame, "Add DNS Server", lx, y);

    // Input field
    let input = Rect::new(lx, y, FIELD_INPUT_WIDTH, FIELD_HEIGHT);
    let focused = app.focus == Some(Field::DnsInput);
    frame.push(RenderCommand::FillRect {
        x: input.x,
        y: input.y,
        width: input.w,
        height: input.h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::StrokeRect {
        x: input.x,
        y: input.y,
        width: input.w,
        height: input.h,
        // A focused box is outlined in the accent colour, so that typing has
        // somewhere visible to go before the first character arrives.
        color: if focused { BLUE } else { OVERLAY0 },
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.hit(Target::Focus(Field::DnsInput), input);
    let typed = if focused {
        format!("{}_", app.dns_input)
    } else {
        app.dns_input.clone()
    };
    let dns_display = if app.dns_input.is_empty() && !focused {
        "e.g. 8.8.8.8"
    } else {
        &typed
    };
    let dns_color = if app.dns_input.is_empty() && !focused {
        OVERLAY0
    } else {
        TEXT_COLOR
    };
    frame.push(RenderCommand::Text {
        x: lx + 8.0,
        y: y + 7.0,
        text: dns_display.to_string(),
        color: dns_color,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(FIELD_INPUT_WIDTH - 16.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Add button
    let add = render_button(frame, "Add", lx + 212.0, y, 60.0, FIELD_HEIGHT, GREEN);
    frame.hit(Target::DnsAdd, add);
}

/// Render the WiFi tab content.
fn render_tab_wifi(frame: &mut Frame, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(frame, "Available WiFi Networks", lx, y);

    if app.wifi_networks.is_empty() {
        frame.push(RenderCommand::Text {
            x: lx,
            y,
            text: "No WiFi networks found. Click Refresh to scan.".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        return;
    }

    for (i, network) in app.wifi_networks.iter().enumerate() {
        let is_selected = app.selected_wifi == Some(i);
        let item_y = y;

        // Selection / hover background
        let bg = if is_selected { SURFACE0 } else { BASE };
        let row = Rect::new(lx, item_y, pw - SECTION_PADDING * 2.0, WIFI_ITEM_HEIGHT);
        frame.push(RenderCommand::FillRect {
            x: row.x,
            y: row.y,
            width: row.w,
            height: row.h,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.hit(Target::Wifi(i), row);

        // Signal bars
        let bars = network.signal_bars();
        render_signal_bars(frame, bars, lx + 8.0, item_y + 8.0);

        // SSID
        frame.push(RenderCommand::Text {
            x: lx + 40.0,
            y: item_y + 6.0,
            text: network.ssid.clone(),
            color: TEXT_COLOR,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Details line
        let detail = format!(
            "{} | Ch {} | {} | {}%",
            network.security_type,
            network.channel,
            network.band_label(),
            network.signal_strength,
        );
        frame.push(RenderCommand::Text {
            x: lx + 40.0,
            y: item_y + 22.0,
            text: detail,
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Connect button. Recorded after the row it sits on, and the hit-test
        // takes the last match, so the button wins the pixels it covers.
        if is_selected {
            let connect = render_button(
                frame,
                "Connect",
                lx + pw - SECTION_PADDING * 2.0 - 80.0,
                item_y + 6.0,
                70.0,
                28.0,
                GREEN,
            );
            frame.hit(Target::WifiConnect, connect);
        }

        y += WIFI_ITEM_HEIGHT + 4.0;
    }
}

/// Render WiFi signal bars.
fn render_signal_bars(frame: &mut Frame, bars: u8, x: f32, y: f32) {
    for i in 0u8..4 {
        let bar_h = 6.0 + (i as f32) * 4.0;
        let bar_y = y + 20.0 - bar_h;
        let bar_color = if i < bars { GREEN } else { SURFACE1 };
        frame.push(RenderCommand::FillRect {
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
fn render_tab_vpn(frame: &mut Frame, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(frame, "VPN Connections", lx, y);

    if app.vpn_configs.is_empty() {
        frame.push(RenderCommand::Text {
            x: lx,
            y,
            text: "No VPN connections configured".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
        frame.push(RenderCommand::FillRect {
            x: lx,
            y: item_y,
            width: pw - SECTION_PADDING * 2.0,
            height: VPN_ITEM_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Status indicator
        frame.push(RenderCommand::FillRect {
            x: lx + 12.0,
            y: item_y + VPN_ITEM_HEIGHT / 2.0 - 5.0,
            width: 10.0,
            height: 10.0,
            color: state.color(),
            corner_radii: CornerRadii::all(5.0),
        });

        // VPN name
        frame.push(RenderCommand::Text {
            x: lx + 30.0,
            y: item_y + 6.0,
            text: vpn.name.clone(),
            color: TEXT_COLOR,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(250.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Details
        let detail = format!(
            "{} | {} | Auto: {}",
            vpn.server_address,
            vpn.protocol.label(),
            if vpn.auto_connect { "Yes" } else { "No" },
        );
        frame.push(RenderCommand::Text {
            x: lx + 30.0,
            y: item_y + 24.0,
            text: detail,
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(350.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Connect/Disconnect button
        let btn_label = if state.is_connected() {
            "Disconnect"
        } else {
            "Connect"
        };
        let btn_color = if state.is_connected() { RED } else { GREEN };
        let toggle = render_button(
            frame,
            btn_label,
            lx + pw - SECTION_PADDING * 2.0 - 100.0,
            item_y + 8.0,
            88.0,
            28.0,
            btn_color,
        );
        frame.hit(Target::Vpn(i), toggle);

        y += VPN_ITEM_HEIGHT + 6.0;
    }
}

/// Render the Profiles tab content.
fn render_tab_profiles(frame: &mut Frame, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(frame, "Network Profiles", lx, y);

    // An empty list says so and then *keeps going* to the Add button below.
    // Returning here — as this did — hid the only control that can create the
    // first profile, in precisely the state where it is needed.
    if app.profiles.is_empty() {
        frame.push(RenderCommand::Text {
            x: lx,
            y,
            text: "No profiles configured".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        y += FIELD_HEIGHT;
    }

    for (i, profile) in app.profiles.iter().enumerate() {
        let is_selected = app.selected_profile == Some(i);
        let item_y = y;
        let row_h = 36.0;

        // Row background
        let bg = if is_selected { SURFACE0 } else { BASE };
        let row = Rect::new(lx, item_y, pw - SECTION_PADDING * 2.0, row_h);
        frame.push(RenderCommand::FillRect {
            x: row.x,
            y: row.y,
            width: row.w,
            height: row.h,
            color: bg,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.hit(Target::Profile(i), row);

        // Security level indicator
        frame.push(RenderCommand::FillRect {
            x: lx + 8.0,
            y: item_y + row_h / 2.0 - 5.0,
            width: 10.0,
            height: 10.0,
            color: profile.security_level.color(),
            corner_radii: CornerRadii::all(5.0),
        });

        // Profile name
        frame.push(RenderCommand::Text {
            x: lx + 26.0,
            y: item_y + 6.0,
            text: profile.name.clone(),
            color: TEXT_COLOR,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
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
        frame.push(RenderCommand::Text {
            x: lx + 26.0,
            y: item_y + 22.0,
            text: detail,
            color: SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Remove button
        let remove = render_mini_button(
            frame,
            "X",
            lx + pw - SECTION_PADDING * 2.0 - 30.0,
            item_y + 8.0,
            RED,
        );
        frame.hit(Target::ProfileRemove(i), remove);

        y += row_h + 4.0;
    }

    // Add profile button
    y += 12.0;
    let add = render_button(frame, "Add Profile", lx, y, 120.0, BUTTON_HEIGHT, BLUE);
    frame.hit(Target::ProfileAdd, add);
}

/// Render the Traffic tab content with a simple bar chart.
fn render_tab_traffic(frame: &mut Frame, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let Some(iface) = app.selected_iface() else {
        render_no_selection(frame, px, py, pw);
        return;
    };

    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;
    let vx = lx + FIELD_LABEL_WIDTH;

    y = render_section_title(frame, "Traffic Overview", lx, y);

    // Current stats
    render_field_row(
        frame,
        "Received:",
        &NetworkInterface::format_bytes(iface.rx_bytes),
        lx,
        vx,
        y,
    );
    y += FIELD_HEIGHT + 4.0;
    render_field_row(
        frame,
        "Transmitted:",
        &NetworkInterface::format_bytes(iface.tx_bytes),
        lx,
        vx,
        y,
    );
    y += FIELD_HEIGHT + 16.0;

    // Throughput graph
    y = render_section_title(frame, "Throughput (recent)", lx, y);

    let graph_x = lx;
    let graph_w = pw - SECTION_PADDING * 2.0;
    let graph_h = TRAFFIC_GRAPH_HEIGHT;

    // Graph background
    frame.push(RenderCommand::FillRect {
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
        frame.push(RenderCommand::FillRect {
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
        frame.push(RenderCommand::FillRect {
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
    frame.push(RenderCommand::FillRect {
        x: lx,
        y: legend_y,
        width: 12.0,
        height: 12.0,
        color: TEAL,
        corner_radii: CornerRadii::all(2.0),
    });
    frame.push(RenderCommand::Text {
        x: lx + 16.0,
        y: legend_y + 1.0,
        text: "RX".into(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.push(RenderCommand::FillRect {
        x: lx + 50.0,
        y: legend_y,
        width: 12.0,
        height: 12.0,
        color: PEACH,
        corner_radii: CornerRadii::all(2.0),
    });
    frame.push(RenderCommand::Text {
        x: lx + 66.0,
        y: legend_y + 1.0,
        text: "TX".into(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// Render the Diagnostics tab content.
fn render_tab_diagnostics(frame: &mut Frame, app: &NetManagerApp, px: f32, py: f32, pw: f32) {
    let mut y = py + SECTION_PADDING;
    let lx = px + SECTION_PADDING;

    y = render_section_title(frame, "Network Diagnostics", lx, y);

    if app.diagnostics.is_empty() {
        frame.push(RenderCommand::Text {
            x: lx,
            y,
            text: "Click 'Diagnose' in the toolbar to run diagnostics.".into(),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        y += 24.0;
        let run = render_button(frame, "Run", lx, y, 80.0, BUTTON_HEIGHT, BLUE);
        frame.hit(Target::RunDiagnostics, run);
        return;
    }

    for diag in &app.diagnostics {
        let row_h = 32.0;

        // Row background
        frame.push(RenderCommand::FillRect {
            x: lx,
            y,
            width: pw - SECTION_PADDING * 2.0,
            height: row_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Status indicator
        frame.push(RenderCommand::FillRect {
            x: lx + 10.0,
            y: y + row_h / 2.0 - 5.0,
            width: 10.0,
            height: 10.0,
            color: diag.status.color(),
            corner_radii: CornerRadii::all(5.0),
        });

        // Name
        frame.push(RenderCommand::Text {
            x: lx + 28.0,
            y: y + 4.0,
            text: diag.name.clone(),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Details. A row is a fixed two-line cell in a list meant to be
        // scanned, so a detail wider than the row is cut rather than wrapped —
        // but cut with a mark. `max_width` on its own stops mid-glyph, which
        // leaves a truncated sentence reading as a complete one.
        let detail_width = pw - SECTION_PADDING * 2.0 - 120.0;
        frame.push(RenderCommand::Text {
            x: lx + 28.0,
            y: y + 18.0,
            text: text::elide(
                &diag.details,
                detail_width,
                "…",
                DIAG_DETAIL_FONT_SIZE,
                FontWeightHint::Regular,
            ),
            color: SUBTEXT0,
            font_size: DIAG_DETAIL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(detail_width),
            overflow: TextOverflow::Ellipsis,
        });

        // Status label on right
        frame.push(RenderCommand::Text {
            x: lx + pw - SECTION_PADDING * 2.0 - 60.0,
            y: y + 10.0,
            text: diag.status.label().to_string(),
            color: diag.status.color(),
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        y += row_h + 4.0;
    }
}

/// Render the status bar at the bottom of the window.
fn render_status_bar(frame: &mut Frame, app: &NetManagerApp) {
    let sy = frame.height - STATUS_BAR_HEIGHT;

    // Status bar background
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: sy,
        width: frame.width,
        height: STATUS_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Separator line
    frame.push(RenderCommand::Line {
        x1: 0.0,
        y1: sy,
        x2: frame.width,
        y2: sy,
        color: SURFACE0,
        width: 1.0,
    });

    // Status message
    frame.push(RenderCommand::Text {
        x: 12.0,
        y: sy + 8.0,
        text: app.status_message.clone(),
        color: SUBTEXT0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(frame.width - 200.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Interface count on right
    let iface_count = format!("{} interfaces", app.interfaces.len());
    frame.push(RenderCommand::Text {
        x: frame.width - 120.0,
        y: sy + 8.0,
        text: iface_count,
        color: OVERLAY0,
        font_size: 11.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

// ============================================================================
// Rendering Helpers
// ============================================================================

/// Render a section title with underline.
fn render_section_title(frame: &mut Frame, title: &str, x: f32, y: f32) -> f32 {
    frame.push(RenderCommand::Text {
        x,
        y,
        text: title.to_string(),
        color: BLUE,
        font_size: SECTION_TEXT,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.push(RenderCommand::Line {
        x1: x,
        y1: y + 18.0,
        // The rule underlines the heading, so it ends where the heading does.
        // `len * 8.0` ran it past a short title and stopped short of a long
        // one, and did both worse the moment the title held an accent.
        x2: x + text::measure(title, SECTION_TEXT, FontWeightHint::Bold),
        y2: y + 18.0,
        color: SURFACE1,
        width: 1.0,
    });
    y + 26.0
}

/// Render a label-value field row.
fn render_field_row(frame: &mut Frame, label: &str, value: &str, lx: f32, vx: f32, y: f32) {
    frame.push(RenderCommand::Text {
        x: lx,
        y,
        text: label.to_string(),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.push(RenderCommand::Text {
        x: vx,
        y,
        text: value.to_string(),
        color: TEXT_COLOR,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// Render an editable field row with input box styling.
///
/// Returns the input box, so the caller can record it as a click target
/// without measuring it a second time.
// label + value strings + 3 geometry floats + color + editing flag + tree.
// Grouping would not help.
#[allow(clippy::too_many_arguments)]
fn render_editable_field(
    frame: &mut Frame,
    label: &str,
    value: &str,
    lx: f32,
    vx: f32,
    y: f32,
    color: Color,
    editing: bool,
) -> Rect {
    frame.push(RenderCommand::Text {
        x: lx,
        y: y + 6.0,
        text: label.to_string(),
        color: SUBTEXT0,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    let box_rect = Rect::new(vx, y, FIELD_INPUT_WIDTH, FIELD_HEIGHT);
    if editing {
        // Input box background
        frame.push(RenderCommand::FillRect {
            x: box_rect.x,
            y: box_rect.y,
            width: box_rect.w,
            height: box_rect.h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::StrokeRect {
            x: box_rect.x,
            y: box_rect.y,
            width: box_rect.w,
            height: box_rect.h,
            color: BLUE,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });
    }

    let display = if value.is_empty() { "---" } else { value };
    frame.push(RenderCommand::Text {
        x: vx + if editing { 8.0 } else { 0.0 },
        y: y + 7.0,
        text: display.to_string(),
        color,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(FIELD_INPUT_WIDTH - 10.0),
        overflow: TextOverflow::Ellipsis,
    });

    box_rect
}

/// Render a toggle indicator row.
///
/// Returns the track — the part that looks like a switch, and so the part a
/// user expects to be able to click.
fn render_toggle_row(frame: &mut Frame, label: &str, enabled: bool, x: f32, y: f32) -> Rect {
    // Toggle track
    let track_w = 36.0;
    let track_h = 18.0;
    let track_color = if enabled { GREEN } else { SURFACE1 };

    frame.push(RenderCommand::FillRect {
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
    frame.push(RenderCommand::FillRect {
        x: knob_x,
        y: y + 2.0,
        width: track_h - 4.0,
        height: track_h - 4.0,
        color: TEXT_COLOR,
        corner_radii: CornerRadii::all(7.0),
    });

    // Label
    frame.push(RenderCommand::Text {
        x: x + track_w + 10.0,
        y: y + 2.0,
        text: label.to_string(),
        color: TEXT_COLOR,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    Rect::new(x, y, track_w, track_h)
}

/// Render a standard-sized button.
///
/// Returns the button's own rect so the caller records exactly the box that
/// was drawn — the click area and the painted area are one value.
fn render_button(
    frame: &mut Frame,
    label: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    accent: Color,
) -> Rect {
    // Button background with muted accent
    let bg = Color::rgba(accent.r / 3, accent.g / 3, accent.b / 3, 200);
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: bg,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::StrokeRect {
        x,
        y,
        width: w,
        height: h,
        color: accent,
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });

    let text_x = text::center_x(label, x + w / 2.0, TOOLBAR_TEXT, FontWeightHint::Bold);
    let text_y = y + (h - TOOLBAR_TEXT) / 2.0;
    frame.push(RenderCommand::Text {
        x: text_x,
        y: text_y,
        text: label.to_string(),
        color: TEXT_COLOR,
        font_size: TOOLBAR_TEXT,
        font_weight: FontWeightHint::Bold,
        max_width: Some(w - 8.0),
        overflow: TextOverflow::Ellipsis,
    });

    Rect::new(x, y, w, h)
}

/// Render a small inline button (for DNS reorder/remove).
///
/// Returns the square it drew, for the caller to record as a click target.
fn render_mini_button(frame: &mut Frame, label: &str, x: f32, y: f32, color: Color) -> Rect {
    let size = MINI_BUTTON_SIZE;
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: size,
        height: size,
        color: SURFACE1,
        corner_radii: CornerRadii::all(3.0),
    });
    frame.push(RenderCommand::Text {
        x: x + 5.0,
        y: y + 4.0,
        text: label.to_string(),
        color,
        font_size: 11.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    Rect::new(x, y, size, size)
}

/// Render a "no interface selected" placeholder.
fn render_no_selection(frame: &mut Frame, px: f32, py: f32, pw: f32) {
    let empty = "No interface selected";
    frame.push(RenderCommand::Text {
        x: text::center_x(empty, px + pw / 2.0, 14.0, FontWeightHint::Regular),
        y: py + 40.0,
        text: empty.into(),
        color: OVERLAY0,
        font_size: 14.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

// ============================================================================
// Interaction
// ============================================================================

/// What handling an event asks the host window to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing changed; the window need not repaint.
    None,
    /// State changed; repaint.
    Redraw,
    /// The user asked to close the window.
    Quit,
}

impl NetManagerApp {
    /// The text behind a focused field.
    ///
    /// One place that maps a [`Field`] to the string it edits, so typing,
    /// backspacing and committing can never disagree about which string a
    /// field means.
    fn field_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Ip => &mut self.edit_ip_config.ip_address,
            Field::Mask => &mut self.edit_ip_config.subnet_mask,
            Field::Gateway => &mut self.edit_ip_config.gateway,
            Field::DnsInput => &mut self.dns_input,
        }
    }

    /// Give a field the keyboard.
    ///
    /// The three IP fields only accept focus while the IP config is being
    /// edited: outside edit mode their boxes are not drawn, and a caret in an
    /// invisible box is a keystroke going somewhere the user cannot see.
    fn focus_field(&mut self, field: Field) -> Action {
        if field != Field::DnsInput && !self.editing_ip {
            return Action::None;
        }
        if self.focus == Some(field) {
            return Action::None;
        }
        self.focus = Some(field);
        Action::Redraw
    }

    /// The field after `field` in tab order, staying within the same tab's
    /// fields — Tab out of the DNS box goes nowhere, because there is nowhere
    /// on that tab for it to go.
    fn next_field(field: Field) -> Field {
        match field {
            Field::Ip => Field::Mask,
            Field::Mask => Field::Gateway,
            Field::Gateway | Field::DnsInput => Field::Ip,
        }
    }

    /// Re-scan: pick up the interface list and the air afresh.
    ///
    /// Sample data today — the real scan belongs to the network service, which
    /// this program does not yet talk to (see `known-issues.md`). It is wired
    /// as a real action rather than left inert so that the button does the
    /// observable thing it claims to: the WiFi list is replaced and the status
    /// line says when.
    fn refresh(&mut self) {
        // The selection follows its SSID across a scan, not its index. A scan
        // reorders the air — signal strengths change, networks come and go —
        // so keeping the index would silently leave the user pointed at a
        // different network than the one they chose, and Connect would join
        // that one instead. If the chosen network is no longer there, neither
        // is the selection.
        let chosen = self
            .selected_wifi
            .and_then(|i| self.wifi_networks.get(i))
            .map(|network| network.ssid.clone());
        self.wifi_networks = sample_wifi_networks();
        self.selected_wifi =
            chosen.and_then(|ssid| self.wifi_networks.iter().position(|n| n.ssid == ssid));
        self.status_message = format!("Scanned: {} networks found", self.wifi_networks.len());
    }

    /// A name no existing profile has, so repeated Add never makes two rows
    /// that cannot be told apart.
    fn unused_profile_name(&self) -> String {
        let mut n = self.profiles.len().saturating_add(1);
        loop {
            let candidate = format!("Profile {n}");
            if !self.profiles.iter().any(|p| p.name == candidate) {
                return candidate;
            }
            n = n.saturating_add(1);
        }
    }

    /// Act on the target under a click.
    ///
    /// Split from [`Self::handle_click`] so that a test can drive a target
    /// directly and a test can drive a *coordinate*, and the two exercise the
    /// same code below the hit-test.
    pub fn activate(&mut self, target: Target) -> Action {
        match target {
            Target::Refresh => {
                self.refresh();
                Action::Redraw
            }
            Target::Diagnose | Target::RunDiagnostics => {
                self.run_diagnostics();
                self.set_tab(DetailTab::Diagnostics);
                Action::Redraw
            }
            Target::ShowProperties => {
                self.set_tab(DetailTab::Properties);
                Action::Redraw
            }
            Target::ToggleEnabled => {
                self.toggle_selected_enabled();
                Action::Redraw
            }
            Target::Interface(i) => {
                self.select_interface(i);
                self.focus = None;
                Action::Redraw
            }
            Target::Tab(tab) => {
                self.set_tab(tab);
                // The caret belongs to a field on the tab being left.
                self.focus = None;
                Action::Redraw
            }
            Target::DhcpToggle => {
                // Flipping DHCP is an edit, so it opens the editor if it is not
                // already open. Otherwise the switch would move, the address
                // fields would grey out, and nothing would ever be applied.
                if !self.editing_ip {
                    self.start_editing_ip();
                }
                self.edit_ip_config.dhcp_enabled = !self.edit_ip_config.dhcp_enabled;
                Action::Redraw
            }
            Target::EditIp => {
                self.start_editing_ip();
                self.focus = Some(Field::Ip);
                Action::Redraw
            }
            Target::ApplyIp => {
                match self.apply_ip_config() {
                    Ok(()) => self.focus = None,
                    Err(why) => self.status_message = why,
                }
                Action::Redraw
            }
            Target::CancelIp => {
                self.cancel_editing_ip();
                self.focus = None;
                Action::Redraw
            }
            Target::Focus(field) => self.focus_field(field),
            Target::DnsUp(i) => {
                let outcome = self.move_dns_up(i);
                self.report(outcome);
                Action::Redraw
            }
            Target::DnsDown(i) => {
                let outcome = self.move_dns_down(i);
                self.report(outcome);
                Action::Redraw
            }
            Target::DnsRemove(i) => {
                let outcome = self.remove_dns_server(i);
                self.report(outcome);
                Action::Redraw
            }
            Target::DnsAdd => {
                self.commit_dns_input();
                Action::Redraw
            }
            Target::Wifi(i) => {
                self.select_wifi(i);
                Action::Redraw
            }
            Target::WifiConnect => {
                match self.connect_wifi() {
                    Ok(_ssid) => {}
                    Err(why) => self.status_message = why,
                }
                Action::Redraw
            }
            Target::Vpn(i) => {
                let outcome = self.toggle_vpn(i);
                self.report(outcome);
                Action::Redraw
            }
            Target::Profile(i) => {
                self.selected_profile = Some(i);
                Action::Redraw
            }
            Target::ProfileRemove(i) => {
                let outcome = self.remove_profile(i);
                self.report(outcome);
                Action::Redraw
            }
            Target::ProfileAdd => {
                let name = self.unused_profile_name();
                // A profile nobody has described yet is a network nobody has
                // vouched for, so it starts at the least trusting setting with
                // the firewall on. Loosening it is a deliberate act; a new
                // profile that silently started out `Private` would not be.
                self.add_profile(&name, SecurityLevel::Public, true);
                self.status_message = format!("Added profile '{name}'");
                Action::Redraw
            }
        }
    }

    /// Put a failed operation's reason on the status line.
    ///
    /// Every list operation here can fail for a reason the user caused (moving
    /// the top entry up, removing an entry that just went away), and a failure
    /// that is silently dropped reads as the program ignoring the click.
    fn report(&mut self, outcome: Result<(), String>) {
        if let Err(why) = outcome {
            self.status_message = why;
        }
    }

    /// Take what has been typed into the DNS box and add it to the list.
    fn commit_dns_input(&mut self) {
        let typed = self.dns_input.clone();
        match self.add_dns_server(&typed) {
            Ok(()) => {
                self.dns_input.clear();
                self.status_message = format!("Added DNS server {typed}");
            }
            Err(why) => self.status_message = why,
        }
    }

    /// Route a click at window coordinates.
    ///
    /// The geometry comes from rendering a frame and asking it what is under
    /// the point — the same walk that draws. There is no second copy of the
    /// layout for the hit-test to drift from.
    pub fn handle_click(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        size: (f32, f32),
    ) -> Action {
        if button != MouseButton::Left {
            return Action::None;
        }
        let (width, height) = size;
        let Some(target) = render_frame(self, width, height).hit_test(x, y) else {
            // Clicking bare background puts the caret away, so a stray
            // keystroke afterwards does not land in a field the user has
            // stopped looking at.
            if self.focus.is_some() {
                self.focus = None;
                return Action::Redraw;
            }
            return Action::None;
        };
        self.activate(target)
    }

    /// Route a keystroke.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Action {
        if !key.pressed {
            return Action::None;
        }

        if let Some(field) = self.focus {
            return self.handle_key_in_field(key, field);
        }

        match key.key {
            Key::Escape => Action::Quit,
            Key::Down => self.move_selection(1),
            Key::Up => self.move_selection(-1),
            Key::Right => self.move_tab(1),
            Key::Left => self.move_tab(-1),
            Key::PageDown => {
                self.scroll_sidebar_by(1);
                Action::Redraw
            }
            Key::PageUp => {
                self.scroll_sidebar_by(-1);
                Action::Redraw
            }
            Key::Home => {
                self.scroll_sidebar_to_top();
                Action::Redraw
            }
            Key::F5 => self.activate(Target::Refresh),
            _ => Action::None,
        }
    }

    /// A keystroke while a text field holds the keyboard.
    fn handle_key_in_field(&mut self, key: &KeyEvent, field: Field) -> Action {
        match key.key {
            Key::Escape => {
                self.focus = None;
                Action::Redraw
            }
            Key::Tab => {
                let next = Self::next_field(field);
                self.focus = Some(next);
                Action::Redraw
            }
            Key::Enter => {
                if field == Field::DnsInput {
                    self.commit_dns_input();
                } else {
                    self.activate(Target::ApplyIp);
                }
                Action::Redraw
            }
            Key::Backspace => {
                if self.field_mut(field).pop().is_some() {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            _ => {
                // `typed()` already drops the control characters that Enter,
                // Tab, Escape and Backspace produce on most layouts, so an
                // unmatched key cannot smuggle a `\r` into an address.
                let typed: String = key.typed().collect();
                if typed.is_empty() {
                    return Action::None;
                }
                self.field_mut(field).push_str(&typed);
                Action::Redraw
            }
        }
    }

    /// Move the sidebar selection by `delta` rows, clamped at both ends.
    fn move_selection(&mut self, delta: isize) -> Action {
        if self.interfaces.is_empty() {
            return Action::None;
        }
        let last = self.interfaces.len().saturating_sub(1);
        let next = if delta < 0 {
            self.selected_interface.saturating_sub(delta.unsigned_abs())
        } else {
            self.selected_interface
                .saturating_add(delta.unsigned_abs())
                .min(last)
        };
        if next == self.selected_interface {
            return Action::None;
        }
        self.select_interface(next);
        Action::Redraw
    }

    /// Move to the next or previous detail tab, wrapping.
    fn move_tab(&mut self, delta: isize) -> Action {
        let tabs = DetailTab::all();
        let Some(here) = tabs.iter().position(|t| *t == self.active_tab) else {
            return Action::None;
        };
        let len = tabs.len();
        // Stepping back is stepping forward by one short of a full lap, which
        // keeps the wrap in unsigned arithmetic and out of `-1`.
        let step = if delta < 0 { len.saturating_sub(1) } else { 1 };
        // `checked_rem` is `None` only for an empty tab strip, which has no tab
        // to move to anyway.
        let Some(next) = here.saturating_add(step).checked_rem(len) else {
            return Action::None;
        };
        let Some(tab) = tabs.get(next) else {
            return Action::None;
        };
        self.set_tab(*tab);
        self.focus = None;
        Action::Redraw
    }

    /// Route a whole event.
    pub fn handle_event(&mut self, event: &Event, size: (f32, f32)) -> Action {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Press(button) => self.handle_click(mouse.x, mouse.y, button, size),
                MouseEventKind::Scroll { dy, .. } => {
                    // Only the sidebar scrolls, so a wheel anywhere scrolls it
                    // rather than nothing. The accumulator keeps the fractions
                    // a trackpad sends and already returns a row delta with the
                    // sign `scroll_sidebar_by` wants.
                    let rows = self.wheel.rows(dy);
                    if rows == 0 {
                        return Action::None;
                    }
                    self.scroll_sidebar_by(rows);
                    Action::Redraw
                }
                _ => Action::None,
            },
            Event::Key(key) => self.handle_key(key),
            Event::CloseRequested => Action::Quit,
            _ => Action::None,
        }
    }
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

impl App for NetManagerApp {
    fn title(&self) -> String {
        "Network Manager".to_string()
    }

    fn app_id(&self) -> String {
        "slateos.netmanager".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // The size the last frame was drawn at is the size the hit-test must
        // use, so it is remembered here rather than guessed from the constants
        // — a resized window whose clicks are still tested against 960x680
        // would mis-route every click in the panel.
        if let Event::Resize { width, height } = *event {
            self.window_size = (width as f32, height as f32);
            return Response::Redraw;
        }
        match self.handle_event(event, self.window_size) {
            Action::None => Response::Idle,
            Action::Redraw => Response::Redraw,
            Action::Quit => Response::Exit,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Believe the size handed in: the first frame goes out before any
        // `Resize` arrives, so this is the only place the real size is known
        // on frame one.
        self.window_size = (width, height);
        render_frame(self, width, height).into_tree()
    }
}

/// Lets the tests drive this window by naming its controls rather than
/// measuring them. Three lines of forwarding; the helpers are in
/// [`guitk::probe`].
impl Probe for NetManagerApp {
    type Target = Target;
    type Outcome = Action;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        render_frame(self, size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Action {
        self.handle_click(x, y, button, size)
    }

    fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> Action {
        self.handle_key(key)
    }
}

fn main() -> ExitCode {
    let mut app = NetManagerApp::new();
    app::launch("netmanager", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test: an `expect` that fires is
    // a failure report, and an index that is out of range is the assertion.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::event::{Modifiers, MouseEvent};

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
        assert_eq!(NetworkInterface::format_bytes(1024), "1.0 kB");
        assert_eq!(NetworkInterface::format_bytes(2048), "2.0 kB");
    }

    #[test]
    fn test_format_bytes_mb() {
        assert_eq!(NetworkInterface::format_bytes(1_048_576), "1.0 MB");
    }

    #[test]
    fn test_format_bytes_gb() {
        assert_eq!(NetworkInterface::format_bytes(1_073_741_824), "1.1 GB");
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

    #[test]
    fn an_overlong_diagnostic_detail_is_marked_as_cut() {
        let mut app = NetManagerApp::new();
        app.run_diagnostics();
        // Far wider than a diagnostics row, which is fixed height by design.
        let long = "The gateway did not answer within the timeout, and the \
            route to it goes through an interface that is currently down."
            .to_string();
        app.diagnostics[0].details = long.clone();
        app.set_tab(DetailTab::Diagnostics);

        let mut frame = Frame::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        render_tab_diagnostics(&mut frame, &app, 0.0, 0.0, 600.0);
        let cmds = frame.commands();
        let detail = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    text, font_size, ..
                } if (*font_size - DIAG_DETAIL_FONT_SIZE).abs() < 0.01 => Some(text.clone()),
                _ => None,
            })
            .expect("the first diagnostic's detail line is drawn");
        assert_ne!(detail, long, "a detail too wide for its row was left whole");
        assert!(
            detail.ends_with('…'),
            "a detail cut to fit its row was not marked as cut: {detail}"
        );
        assert!(
            long.starts_with(detail.trim_end_matches('…')),
            "the kept part of the detail is not its own beginning: {detail}"
        );
    }

    #[test]
    fn a_short_diagnostic_detail_is_left_alone() {
        let mut app = NetManagerApp::new();
        app.run_diagnostics();
        app.diagnostics[0].details = "OK".to_string();
        let mut frame = Frame::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        render_tab_diagnostics(&mut frame, &app, 0.0, 0.0, 600.0);
        let cmds = frame.commands();
        let detail = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    text, font_size, ..
                } if (*font_size - DIAG_DETAIL_FONT_SIZE).abs() < 0.01 => Some(text.clone()),
                _ => None,
            })
            .expect("the first diagnostic's detail line is drawn");
        assert_eq!(detail, "OK");
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

    // ------------------------------------------------------------------
    // Text measurement
    // ------------------------------------------------------------------

    /// Every tab is measured in the bold weight the active one is drawn in, so
    /// the strip does not reflow when the selection moves, and every label
    /// fits inside its own tab with its 8 px of padding each side.
    #[test]
    fn tab_labels_fit_and_the_strip_does_not_reflow() {
        for tab in DetailTab::all() {
            let w = tab_width(*tab);
            let bold = text::measure(tab.label(), TAB_TEXT, FontWeightHint::Bold);
            let regular = text::measure(tab.label(), TAB_TEXT, FontWeightHint::Regular);
            assert!(bold + 16.0 <= w + 0.01, "{:?} overflows its tab", tab);
            assert!(
                w >= regular + 16.0,
                "{:?} is too narrow when drawn bold",
                tab
            );
        }
    }

    /// A toolbar button is sized to hold its label with 12 px each side.
    #[test]
    fn toolbar_labels_fit_their_buttons() {
        for label in ["Refresh", "Diagnose", "Properties", "Enable", "Disable"] {
            let bw = text::measure(label, TOOLBAR_TEXT, FontWeightHint::Regular) + 24.0;
            assert!(bw > 24.0, "{label:?} produced an empty button");
            let drawn = text::measure(label, TOOLBAR_TEXT, FontWeightHint::Regular);
            assert!(drawn + 24.0 <= bw + 0.01, "{label:?} overflows its button");
        }
    }

    /// A section heading's rule underlines the heading, so it ends where the
    /// heading does. `len * 8.0` ran past a short title and stopped short of a
    /// long one, and did both worse the moment a title held an accent.
    #[test]
    fn a_section_rule_is_as_long_as_its_heading() {
        for title in ["IP Configuration", "DNS", "Wi-Fi Networks"] {
            let rule = text::measure(title, SECTION_TEXT, FontWeightHint::Bold);
            assert!(rule > 0.0, "{title:?} got an invisible rule");
            // Bold, because the heading is drawn bold.
            let regular = text::measure(title, SECTION_TEXT, FontWeightHint::Regular);
            assert!(rule >= regular, "{title:?}: rule sized at the wrong weight");
        }
    }

    /// Measuring counts characters, not UTF-8 bytes -- the failure that made
    /// every one of these estimates two to four times too wide for accented
    /// text while looking correct for ASCII.
    #[test]
    fn measuring_is_not_driven_by_byte_length() {
        let accented = text::measure("Réseau", TOOLBAR_TEXT, FontWeightHint::Regular);
        let ascii = text::measure("Reseau", TOOLBAR_TEXT, FontWeightHint::Regular);
        assert!(
            (accented - ascii).abs() < ascii * 0.25,
            "{accented} should be close to {ascii}, not to the byte count"
        );
    }

    // --- Sidebar scrolling ---

    /// An app whose sidebar holds `n` interfaces, named so that a rendered row
    /// can be recognised by the *shape* of its text rather than by where on
    /// screen it landed. A pixel filter is what makes a helper quietly report
    /// that nothing was drawn.
    fn app_with_interfaces(n: usize) -> NetManagerApp {
        let mut app = NetManagerApp::new();
        let template = app.interfaces.first().cloned().expect("sample interfaces");
        app.interfaces = (0..n)
            .map(|i| {
                let mut iface = template.clone();
                iface.id = u32::try_from(i).unwrap_or(u32::MAX).saturating_add(1);
                iface.name = format!("I{i:03}");
                iface
            })
            .collect();
        app.selected_interface = 0;
        app
    }

    /// The interface names the sidebar actually drew, in the order it drew
    /// them. Keyed on the `I000` shape given by `app_with_interfaces`, so a
    /// change to the sidebar's indentation cannot turn this into an empty list.
    fn drawn_interfaces(app: &NetManagerApp) -> Vec<String> {
        let mut frame = Frame::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        render_sidebar(&mut frame, app);
        let cmds = frame.commands();
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. }
                    if text.len() == 4
                        && text.starts_with('I')
                        && text
                            .get(1..)
                            .is_some_and(|d| d.chars().all(|ch| ch.is_ascii_digit())) =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The whole point of the fix: the old loop had no break at all, so an
    /// interface list longer than the sidebar drew through the status bar and
    /// off the bottom of the window.
    #[test]
    fn no_sidebar_row_is_drawn_past_the_bottom_of_the_sidebar() {
        let bottom = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;
        for n in [0_usize, 1, 8, 9, 10, 40, 200] {
            for offset in [0_usize, 3, 50, 10_000] {
                let mut app = app_with_interfaces(n);
                app.sidebar_scroll = offset;
                let mut frame = Frame::new(WINDOW_WIDTH, WINDOW_HEIGHT);
                render_sidebar(&mut frame, &app);
                let cmds = frame.commands();
                for cmd in cmds {
                    let (label, y) = match cmd {
                        RenderCommand::Text { text, y, .. } => (text.clone(), *y),
                        RenderCommand::FillRect { y, height, .. } => {
                            ("rect".to_string(), y + height)
                        }
                        _ => continue,
                    };
                    // The sidebar's own background fills exactly `sh`, so it
                    // ends on the boundary rather than inside it.
                    assert!(
                        y <= bottom + 0.01,
                        "{label:?} drawn to y={y}, past the sidebar bottom \
                         {bottom} (n={n}, offset={offset})"
                    );
                }
            }
        }
    }

    /// A sidebar that fits everything shows everything and says nothing about
    /// hidden rows -- the case that must not regress when the window is added.
    #[test]
    fn a_sidebar_that_fits_its_interfaces_shows_all_of_them() {
        let app = app_with_interfaces(4);
        assert_eq!(drawn_interfaces(&app), ["I000", "I001", "I002", "I003"]);
        let mut frame = Frame::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        render_sidebar(&mut frame, &app);
        let cmds = frame.commands();
        assert!(
            !cmds.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text.ends_with(" more")
            )),
            "nothing is hidden, so nothing should claim to be"
        );
    }

    /// Every interface is reachable by scrolling. A list that stops at the
    /// sidebar's bottom edge and has no offset is a list whose tail cannot be
    /// seen at all, which is the half of the bug that a clamp alone leaves in
    /// place.
    #[test]
    fn every_interface_is_reachable_by_scrolling() {
        let n = 60;
        let mut seen: Vec<String> = Vec::new();
        for offset in 0..n {
            let mut app = app_with_interfaces(n);
            app.sidebar_scroll = offset;
            for name in drawn_interfaces(&app) {
                if !seen.contains(&name) {
                    seen.push(name);
                }
            }
        }
        assert_eq!(
            seen.len(),
            n,
            "only {} of {n} interfaces were ever drawn",
            seen.len()
        );
        for i in 0..n {
            assert!(seen.contains(&format!("I{i:03}")), "I{i:03} unreachable");
        }
    }

    /// Scrolling by one row moves the window by exactly one row -- the
    /// property that fails when an offset field exists but is never read,
    /// which is what `sidebar_scroll: f32` was for the life of this app.
    #[test]
    fn scrolling_the_sidebar_moves_it_by_exactly_one_interface() {
        let mut app = app_with_interfaces(60);
        let first = drawn_interfaces(&app);
        assert!(first.len() >= 2, "test needs a sidebar that holds rows");
        app.scroll_sidebar_by(1);
        let second = drawn_interfaces(&app);
        assert_eq!(second.first().map(String::as_str), Some("I001"));
        assert_eq!(
            second.len(),
            first.len(),
            "a full page stays full while scrolling"
        );
    }

    /// An offset outliving the list it was taken against shows the last page,
    /// not a blank sidebar.
    #[test]
    fn a_sidebar_under_a_stale_offset_shows_its_last_page() {
        let mut app = app_with_interfaces(60);
        app.sidebar_scroll = 55;
        let page = drawn_interfaces(&app);
        app.interfaces.truncate(6);
        let after = drawn_interfaces(&app);
        assert_eq!(
            after,
            ["I000", "I001", "I002", "I003", "I004", "I005"],
            "a shrunken list should scroll up to meet the offset, not go blank"
        );
        assert!(!page.is_empty(), "the deep offset should still show a page");
        assert_eq!(
            page.last().map(String::as_str),
            Some("I059"),
            "the last page ends at the end of the list"
        );
    }

    /// Scrolling up from the top stays at the top rather than wrapping to the
    /// far end of the list, which is what an unsigned subtraction would do.
    #[test]
    fn scrolling_the_sidebar_up_from_the_top_stays_at_the_top() {
        let mut app = app_with_interfaces(60);
        app.scroll_sidebar_by(-1);
        assert_eq!(app.sidebar_scroll, 0);
        app.scroll_sidebar_by(isize::MIN);
        assert_eq!(app.sidebar_scroll, 0);
        assert_eq!(
            drawn_interfaces(&app).first().map(String::as_str),
            Some("I000")
        );
        app.scroll_sidebar_by(4);
        app.scroll_sidebar_to_top();
        assert_eq!(app.sidebar_scroll, 0);
    }

    /// A sidebar hiding interfaces says how many, and the count is the truth.
    #[test]
    fn a_sidebar_that_is_hiding_interfaces_says_so() {
        let app = app_with_interfaces(60);
        let shown = drawn_interfaces(&app).len();
        assert!(shown < 60, "60 interfaces should not fit a 680px window");
        let mut frame = Frame::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        render_sidebar(&mut frame, &app);
        let cmds = frame.commands();
        let note = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, .. } if text.ends_with(" more") => Some(text.clone()),
                _ => None,
            })
            .expect("a truncated sidebar should say it is truncated");
        assert_eq!(note, format!("{} more", 60 - shown));
    }

    /// The selection highlight follows the selected interface as the list
    /// scrolls, rather than staying on whichever row is at that position on
    /// screen. Comparing the loop counter to `selected_interface` after
    /// windowing is exactly the mistake this rules out.
    #[test]
    fn the_selection_highlight_tracks_the_selected_interface_not_the_screen_row() {
        let mut app = app_with_interfaces(60);
        app.selected_interface = 20;
        app.sidebar_scroll = 20;
        let names = drawn_interfaces(&app);
        assert_eq!(names.first().map(String::as_str), Some("I020"));

        let mut frame = Frame::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        render_sidebar(&mut frame, &app);
        let cmds = frame.commands();
        // The highlight is the only rounded full-width row rect in SURFACE0.
        let highlights: Vec<f32> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    y, width, color, ..
                } if *color == SURFACE0 && (*width - (SIDEBAR_WIDTH - 8.0)).abs() < 0.01 => {
                    Some(*y)
                }
                _ => None,
            })
            .collect();
        let list_y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + 32.0;
        assert_eq!(
            highlights,
            vec![list_y],
            "interface 20 is the first row on screen, so the highlight belongs there"
        );
    }

    // ========================================================================
    // Interaction
    //
    // Every test below finds what it clicks by *rendering* and asking the
    // frame, never by recomputing a coordinate from the layout constants. A
    // test that recomputes geometry agrees with the renderer only by accident,
    // and keeps agreeing after the renderer moves — which is exactly the bug
    // it was supposed to catch.
    // ========================================================================

    /// The size the tests click at. Not `WINDOW_WIDTH`/`WINDOW_HEIGHT` by
    /// coincidence: the app remembers the size it last drew at, and these
    /// tests exercise the same path the window does.
    const SIZE: (f32, f32) = <NetManagerApp as Probe>::SIZE;

    /// Finding a control by name, clicking it, and typing at it are the same
    /// four lines in every program, so they live in the toolkit — see
    /// [`guitk::probe`] for what each one guarantees. Imported under their
    /// bare names because that is what the tests below already say.
    use guitk::probe::{click, press, rect_of, type_str, typing};

    #[test]
    fn clicking_a_sidebar_row_selects_that_interface() {
        let mut app = NetManagerApp::new();
        assert!(app.interfaces.len() >= 2, "needs two rows to tell apart");
        app.select_interface(0);

        assert_eq!(click(&mut app, Target::Interface(1)), Action::Redraw);
        assert_eq!(app.selected_interface, 1);
    }

    #[test]
    fn a_sidebar_row_is_clickable_across_its_whole_painted_band() {
        let mut app = NetManagerApp::new();
        let row = rect_of(&app, Target::Interface(1)).expect("row 1 is on screen");

        // Just inside each corner. `Rect::contains` is half-open, so the far
        // edges belong to the *next* row and are deliberately not tested here.
        for (x, y) in [
            (row.x, row.y),
            (row.x + row.w - 0.5, row.y),
            (row.x, row.y + row.h - 0.5),
            (row.x + row.w - 0.5, row.y + row.h - 0.5),
        ] {
            app.select_interface(0);
            app.handle_click(x, y, MouseButton::Left, SIZE);
            assert_eq!(
                app.selected_interface, 1,
                "click at ({x}, {y}) missed row 1"
            );
        }
    }

    #[test]
    fn the_far_edge_of_a_row_belongs_to_the_row_below_it() {
        // Half-open rects: if both rows claimed the boundary pixel the
        // topmost-wins rule would silently decide which, and the answer would
        // depend on draw order rather than on where the user pointed.
        let app = NetManagerApp::new();
        let row0 = rect_of(&app, Target::Interface(0)).expect("row 0");
        let row1 = rect_of(&app, Target::Interface(1)).expect("row 1");
        let frame = render_frame(&app, SIZE.0, SIZE.1);
        let x = row0.x + 4.0;

        assert_eq!(
            frame.hit_test(x, row0.y + row0.h - 0.5),
            Some(Target::Interface(0))
        );
        assert_eq!(frame.hit_test(x, row1.y), Some(Target::Interface(1)));
    }

    #[test]
    fn clicking_a_tab_switches_the_panel() {
        let mut app = NetManagerApp::new();
        for tab in DetailTab::all() {
            assert_eq!(click(&mut app, Target::Tab(*tab)), Action::Redraw);
            assert_eq!(app.active_tab, *tab);
        }
    }

    #[test]
    fn every_tab_in_the_strip_is_clickable_and_none_overlap() {
        // The strip is laid out by accumulating widths; an off-by-one in that
        // sum would leave two tabs sharing a band, and the wrong one would
        // open. Checked by walking the recorded rects rather than the labels.
        let app = NetManagerApp::new();
        let frame = render_frame(&app, SIZE.0, SIZE.1);
        let mut previous: Option<Rect> = None;
        for tab in DetailTab::all() {
            let rect = frame
                .hits()
                .iter()
                .find(|(t, _)| *t == Target::Tab(*tab))
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("tab {tab:?} has no click band"));
            if let Some(before) = previous {
                assert!(
                    rect.x >= before.x + before.w,
                    "tab {tab:?} starts at {} but the one before ends at {}",
                    rect.x,
                    before.x + before.w
                );
            }
            previous = Some(rect);
        }
    }

    #[test]
    fn the_toolbar_buttons_do_what_they_say() {
        let mut app = NetManagerApp::new();

        app.set_tab(DetailTab::Traffic);
        click(&mut app, Target::ShowProperties);
        assert_eq!(app.active_tab, DetailTab::Properties);

        app.diagnostics.clear();
        click(&mut app, Target::Diagnose);
        assert!(!app.diagnostics.is_empty(), "Diagnose ran no diagnostics");
        assert_eq!(
            app.active_tab,
            DetailTab::Diagnostics,
            "Diagnose left the results on a tab the user cannot see"
        );

        let was = app.interfaces[app.selected_interface].enabled;
        click(&mut app, Target::ToggleEnabled);
        assert_ne!(app.interfaces[app.selected_interface].enabled, was);
    }

    #[test]
    fn refresh_rescans_and_says_so() {
        let mut app = NetManagerApp::new();
        app.wifi_networks.clear();

        click(&mut app, Target::Refresh);

        assert!(!app.wifi_networks.is_empty(), "Refresh found no networks");
        assert!(
            app.status_message.contains("Scanned"),
            "status line did not report the scan: {}",
            app.status_message
        );
    }

    #[test]
    fn a_wifi_selection_follows_its_network_across_a_scan() {
        // Keeping the *index* would leave the user pointed at whatever network
        // happened to land in that slot after the rescan, and Connect would
        // join that one instead of the one they picked.
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::WiFi);
        // Start from the reverse of what the next scan will report, so that
        // tracking by index and tracking by name give different answers.
        app.wifi_networks.reverse();
        click(&mut app, Target::Wifi(0));
        let chosen = app.wifi_networks[0].ssid.clone();

        click(&mut app, Target::Refresh);

        assert_ne!(
            app.selected_wifi,
            Some(0),
            "the scan reordered the list but the selection stayed put, which \
             is the bug this test exists for"
        );
        let now = app
            .selected_wifi
            .and_then(|i| app.wifi_networks.get(i))
            .map(|n| n.ssid.clone());
        assert_eq!(now, Some(chosen));
    }

    #[test]
    fn a_wifi_selection_that_goes_off_the_air_is_dropped() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::WiFi);
        // A network only this app knows about; the scan will not find it.
        app.wifi_networks.push(WiFiNetwork {
            ssid: "gone-by-morning".into(),
            signal_strength: 50,
            security_type: "WPA2".into(),
            channel: 6,
            frequency_ghz: 2.4,
        });
        app.selected_wifi = Some(app.wifi_networks.len() - 1);

        click(&mut app, Target::Refresh);

        assert_eq!(
            app.selected_wifi, None,
            "a network that is no longer on the air is still selected"
        );
    }

    #[test]
    fn the_edit_button_opens_the_editor_and_apply_writes_it_back() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::IpConfig);

        assert!(
            rect_of(&app, Target::ApplyIp).is_none(),
            "Apply before Edit"
        );
        click(&mut app, Target::EditIp);
        assert!(app.editing_ip);
        assert_eq!(app.focus, Some(Field::Ip), "Edit put the caret nowhere");

        // Retype the address through the keyboard, exactly as a user would.
        for _ in 0..64 {
            app.handle_key(&press(Key::Backspace));
        }
        app.edit_ip_config.dhcp_enabled = false;
        type_str(&mut app, "10.0.0.7");

        click(&mut app, Target::ApplyIp);
        assert!(!app.editing_ip, "Apply left the editor open");
        assert_eq!(
            app.interfaces[app.selected_interface].ip_config.ip_address,
            "10.0.0.7"
        );
    }

    #[test]
    fn apply_refuses_a_bad_address_and_says_why() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::IpConfig);
        click(&mut app, Target::EditIp);
        app.edit_ip_config.dhcp_enabled = false;
        app.edit_ip_config.ip_address = "999.1.1.1".into();

        click(&mut app, Target::ApplyIp);

        assert!(app.editing_ip, "a rejected Apply closed the editor anyway");
        assert!(
            app.status_message.to_lowercase().contains("ip"),
            "the refusal was silent: {}",
            app.status_message
        );
    }

    #[test]
    fn cancel_puts_the_interfaces_own_address_back() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::IpConfig);
        let original = app.interfaces[app.selected_interface]
            .ip_config
            .ip_address
            .clone();

        click(&mut app, Target::EditIp);
        app.edit_ip_config.ip_address = "10.9.9.9".into();
        click(&mut app, Target::CancelIp);

        assert!(!app.editing_ip);
        assert_eq!(app.edit_ip_config.ip_address, original);
        assert_eq!(app.focus, None);
    }

    #[test]
    fn the_ip_fields_only_take_the_keyboard_while_the_editor_is_open() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::IpConfig);

        assert!(
            rect_of(&app, Target::Focus(Field::Ip)).is_none(),
            "an unopened editor still advertised a text box to click"
        );
        assert_eq!(app.activate(Target::Focus(Field::Ip)), Action::None);
        assert_eq!(app.focus, None);

        click(&mut app, Target::EditIp);
        assert!(rect_of(&app, Target::Focus(Field::Gateway)).is_some());
        click(&mut app, Target::Focus(Field::Gateway));
        assert_eq!(app.focus, Some(Field::Gateway));
    }

    #[test]
    fn tab_walks_the_ip_fields_and_typing_lands_in_the_focused_one() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::IpConfig);
        click(&mut app, Target::EditIp);
        app.edit_ip_config.subnet_mask.clear();
        let address_before = app.edit_ip_config.ip_address.clone();

        app.handle_key(&press(Key::Tab));
        assert_eq!(app.focus, Some(Field::Mask));
        type_str(&mut app, "255.0.0.0");
        assert_eq!(app.edit_ip_config.subnet_mask, "255.0.0.0");
        assert_eq!(
            app.edit_ip_config.ip_address, address_before,
            "typing leaked into the field that no longer had focus"
        );

        app.handle_key(&press(Key::Tab));
        assert_eq!(app.focus, Some(Field::Gateway));
        app.handle_key(&press(Key::Tab));
        assert_eq!(app.focus, Some(Field::Ip), "tab order did not come round");
    }

    #[test]
    fn escape_in_a_field_puts_the_caret_away_rather_than_closing_the_window() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::IpConfig);
        click(&mut app, Target::EditIp);

        assert_eq!(app.handle_key(&press(Key::Escape)), Action::Redraw);
        assert_eq!(app.focus, None);
        assert!(app.editing_ip, "escape from a field abandoned the edit");

        // Only once nothing is holding the keyboard does escape mean "close".
        assert_eq!(app.handle_key(&press(Key::Escape)), Action::Quit);
    }

    #[test]
    fn a_focused_field_shows_a_caret_so_the_keyboard_has_somewhere_visible_to_go() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::IpConfig);
        click(&mut app, Target::EditIp);
        app.edit_ip_config.ip_address = "1.2.3.4".into();

        let carets = |app: &NetManagerApp| {
            render_frame(app, SIZE.0, SIZE.1)
                .commands()
                .iter()
                .filter(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "1.2.3.4_"))
                .count()
        };

        app.focus = Some(Field::Ip);
        assert_eq!(carets(&app), 1, "the focused address drew no caret");
        app.focus = Some(Field::Gateway);
        assert_eq!(carets(&app), 0, "an unfocused address drew a caret");
    }

    #[test]
    fn control_keys_do_not_type_their_control_characters_into_a_field() {
        // Enter, Tab, Escape and Backspace all produce text on most layouts
        // (`\r`, `\t`, `\x1b`, `\x08`). A field that appends whatever arrives
        // fills with unprintable bytes the first time someone presses Escape.
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Dns);
        click(&mut app, Target::Focus(Field::DnsInput));

        for text in ["\r", "\t", "\x1b", "\u{8}"] {
            app.handle_key(&KeyEvent {
                key: Key::F1,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: text.to_string(),
            });
        }
        assert_eq!(app.dns_input, "");
    }

    #[test]
    fn a_key_release_types_nothing() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Dns);
        click(&mut app, Target::Focus(Field::DnsInput));

        let mut release = typing("8");
        release.pressed = false;
        assert_eq!(app.handle_key(&release), Action::None);
        assert_eq!(app.dns_input, "");
    }

    #[test]
    fn the_dns_box_takes_typing_and_add_moves_it_into_the_list() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Dns);
        let before = app.edit_ip_config.dns_servers.len();

        click(&mut app, Target::Focus(Field::DnsInput));
        assert_eq!(app.focus, Some(Field::DnsInput));
        type_str(&mut app, "9.9.9.9");
        assert_eq!(app.dns_input, "9.9.9.9");

        click(&mut app, Target::DnsAdd);
        assert_eq!(app.edit_ip_config.dns_servers.len(), before + 1);
        assert_eq!(
            app.edit_ip_config.dns_servers.last().map(String::as_str),
            Some("9.9.9.9")
        );
        assert_eq!(app.dns_input, "", "the box kept what it had already added");
    }

    #[test]
    fn enter_in_the_dns_box_adds_without_reaching_for_the_button() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Dns);
        let before = app.edit_ip_config.dns_servers.len();

        click(&mut app, Target::Focus(Field::DnsInput));
        type_str(&mut app, "1.0.0.1");
        app.handle_key(&press(Key::Enter));

        assert_eq!(app.edit_ip_config.dns_servers.len(), before + 1);
    }

    #[test]
    fn a_rejected_dns_address_stays_in_the_box_with_the_reason_on_screen() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Dns);
        let before = app.edit_ip_config.dns_servers.clone();

        click(&mut app, Target::Focus(Field::DnsInput));
        type_str(&mut app, "not.an.address");
        click(&mut app, Target::DnsAdd);

        assert_eq!(app.edit_ip_config.dns_servers, before);
        assert_eq!(
            app.dns_input, "not.an.address",
            "the box was cleared, so the user must retype to correct it"
        );
        assert!(
            app.status_message.contains("not.an.address"),
            "the refusal did not name the address: {}",
            app.status_message
        );
    }

    #[test]
    fn backspace_removes_the_last_character_typed() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Dns);
        click(&mut app, Target::Focus(Field::DnsInput));
        type_str(&mut app, "8.8");
        app.handle_key(&press(Key::Backspace));
        assert_eq!(app.dns_input, "8.");

        // Backspace on an empty field changes nothing, so it must not ask for
        // a repaint either.
        app.handle_key(&press(Key::Backspace));
        app.handle_key(&press(Key::Backspace));
        assert_eq!(app.handle_key(&press(Key::Backspace)), Action::None);
    }

    #[test]
    fn the_dns_reorder_buttons_move_the_row_they_sit_on() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Dns);
        app.edit_ip_config.dns_servers = vec!["1.1.1.1".into(), "2.2.2.2".into(), "3.3.3.3".into()];

        click(&mut app, Target::DnsDown(0));
        assert_eq!(app.edit_ip_config.dns_servers[0], "2.2.2.2");
        click(&mut app, Target::DnsUp(1));
        assert_eq!(app.edit_ip_config.dns_servers[0], "1.1.1.1");
        click(&mut app, Target::DnsRemove(1));
        assert_eq!(
            app.edit_ip_config.dns_servers,
            vec!["1.1.1.1".to_string(), "3.3.3.3".to_string()]
        );
    }

    #[test]
    fn the_first_row_has_no_up_button_and_the_last_has_no_down_button() {
        // A button drawn where the operation cannot succeed is a button that
        // answers a click with an error message.
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Dns);
        app.edit_ip_config.dns_servers = vec!["1.1.1.1".into(), "2.2.2.2".into()];

        assert!(rect_of(&app, Target::DnsUp(0)).is_none());
        assert!(rect_of(&app, Target::DnsDown(1)).is_none());
        assert!(rect_of(&app, Target::DnsDown(0)).is_some());
        assert!(rect_of(&app, Target::DnsUp(1)).is_some());
    }

    #[test]
    fn the_dhcp_switch_opens_the_editor_rather_than_changing_nothing() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::IpConfig);
        assert!(!app.editing_ip);
        let was = app.edit_ip_config.dhcp_enabled;

        click(&mut app, Target::DhcpToggle);

        assert_ne!(app.edit_ip_config.dhcp_enabled, was);
        assert!(
            app.editing_ip,
            "the switch moved but Apply never appeared, so the change could \
             not be committed"
        );
        assert!(rect_of(&app, Target::ApplyIp).is_some());
    }

    #[test]
    fn selecting_a_wifi_network_is_what_reveals_its_connect_button() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::WiFi);
        app.selected_wifi = None;
        assert!(rect_of(&app, Target::WifiConnect).is_none());

        click(&mut app, Target::Wifi(1));
        assert_eq!(app.selected_wifi, Some(1));

        let connect = rect_of(&app, Target::WifiConnect).expect("Connect appeared");
        let row = rect_of(&app, Target::Wifi(1)).expect("row 1");
        // The button sits on the row and is recorded after it, so the click
        // lands on the button and not on the row underneath.
        let frame = render_frame(&app, SIZE.0, SIZE.1);
        assert_eq!(
            frame.hit_test(connect.x + connect.w / 2.0, connect.y + connect.h / 2.0),
            Some(Target::WifiConnect),
            "the row swallowed the click meant for its Connect button"
        );
        assert!(connect.y >= row.y && connect.y < row.y + row.h);
    }

    #[test]
    fn connecting_to_a_wifi_network_names_it_on_the_status_line() {
        let mut app = NetManagerApp::new();
        // Select a WiFi interface so the connection has something to happen to.
        let wifi_iface = app
            .interfaces
            .iter()
            .position(|i| i.interface_type == InterfaceType::WiFi)
            .expect("the sample data has a WiFi interface");
        app.select_interface(wifi_iface);
        app.set_tab(DetailTab::WiFi);

        click(&mut app, Target::Wifi(0));
        let ssid = app.wifi_networks[0].ssid.clone();
        click(&mut app, Target::WifiConnect);

        assert!(
            app.status_message.contains(&ssid),
            "status said {:?}, which does not name {ssid}",
            app.status_message
        );
    }

    #[test]
    fn the_vpn_button_toggles_the_row_it_belongs_to() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Vpn);
        assert!(app.vpn_configs.len() >= 2, "needs two VPNs to tell apart");
        let other = app.vpn_states[0].clone();
        let was_connected = app.vpn_states[1].is_connected();

        click(&mut app, Target::Vpn(1));

        assert_ne!(
            app.vpn_states[1].is_connected(),
            was_connected,
            "the button did not change the connection it names"
        );
        assert_eq!(app.vpn_states[0], other, "the wrong row changed state");
    }

    #[test]
    fn add_profile_is_reachable_when_there_are_no_profiles_yet() {
        // The empty-list branch used to return before the button was drawn, so
        // the only control that creates the first profile was hidden in
        // exactly the state that needs it.
        let mut app = NetManagerApp::new();
        app.profiles.clear();
        app.selected_profile = None;
        app.set_tab(DetailTab::Profiles);

        assert!(rect_of(&app, Target::ProfileAdd).is_some());
        click(&mut app, Target::ProfileAdd);
        assert_eq!(app.profiles.len(), 1);
    }

    #[test]
    fn repeated_adds_never_make_two_profiles_with_the_same_name() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Profiles);
        for _ in 0..5 {
            click(&mut app, Target::ProfileAdd);
        }
        let mut names: Vec<&str> = app.profiles.iter().map(|p| p.name.as_str()).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "two profiles share a name");
    }

    #[test]
    fn a_new_profile_starts_at_the_least_trusting_setting() {
        let mut app = NetManagerApp::new();
        app.profiles.clear();
        app.set_tab(DetailTab::Profiles);
        click(&mut app, Target::ProfileAdd);

        let made = app.profiles.first().expect("a profile was added");
        assert_eq!(made.security_level, SecurityLevel::Public);
        assert!(made.firewall_enabled);
    }

    #[test]
    fn removing_a_profile_removes_the_one_whose_button_was_pressed() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Profiles);
        assert!(app.profiles.len() >= 2);
        let doomed = app.profiles[1].name.clone();

        click(&mut app, Target::ProfileRemove(1));

        assert!(!app.profiles.iter().any(|p| p.name == doomed));
    }

    #[test]
    fn the_run_button_appears_only_while_there_is_nothing_to_show() {
        let mut app = NetManagerApp::new();
        app.diagnostics.clear();
        app.set_tab(DetailTab::Diagnostics);

        assert!(rect_of(&app, Target::RunDiagnostics).is_some());
        click(&mut app, Target::RunDiagnostics);
        assert!(!app.diagnostics.is_empty());
        assert!(
            rect_of(&app, Target::RunDiagnostics).is_none(),
            "the Run button covered the results it had just produced"
        );
    }

    #[test]
    fn a_click_on_bare_background_hits_nothing_and_asks_for_no_repaint() {
        let mut app = NetManagerApp::new();
        // The title bar carries no controls.
        assert_eq!(
            app.handle_click(SIZE.0 / 2.0, 4.0, MouseButton::Left, SIZE),
            Action::None
        );
    }

    #[test]
    fn a_click_on_bare_background_puts_the_caret_away() {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Dns);
        click(&mut app, Target::Focus(Field::DnsInput));
        assert_eq!(app.focus, Some(Field::DnsInput));

        assert_eq!(
            app.handle_click(SIZE.0 / 2.0, 4.0, MouseButton::Left, SIZE),
            Action::Redraw
        );
        assert_eq!(app.focus, None);
    }

    #[test]
    fn only_the_left_button_activates_anything() {
        let mut app = NetManagerApp::new();
        let row = rect_of(&app, Target::Interface(1)).expect("row 1");
        app.select_interface(0);

        for button in [MouseButton::Right, MouseButton::Middle] {
            let acted = app.handle_click(row.x + 4.0, row.y + 4.0, button, SIZE);
            assert_eq!(acted, Action::None);
            assert_eq!(app.selected_interface, 0);
        }
    }

    #[test]
    fn the_arrow_keys_walk_the_interface_list_and_stop_at_both_ends() {
        let mut app = NetManagerApp::new();
        let last = app.interfaces.len() - 1;

        app.select_interface(0);
        assert_eq!(app.handle_key(&press(Key::Up)), Action::None);
        assert_eq!(app.selected_interface, 0);

        assert_eq!(app.handle_key(&press(Key::Down)), Action::Redraw);
        assert_eq!(app.selected_interface, 1);

        app.select_interface(last);
        assert_eq!(app.handle_key(&press(Key::Down)), Action::None);
        assert_eq!(app.selected_interface, last);
    }

    #[test]
    fn the_left_and_right_keys_walk_the_tabs_and_wrap() {
        let mut app = NetManagerApp::new();
        let tabs = DetailTab::all();
        app.set_tab(tabs[0]);

        app.handle_key(&press(Key::Left));
        assert_eq!(app.active_tab, tabs[tabs.len() - 1], "left did not wrap");

        app.handle_key(&press(Key::Right));
        assert_eq!(app.active_tab, tabs[0], "right did not wrap");
    }

    #[test]
    fn the_wheel_scrolls_the_sidebar_and_a_trackpads_fractions_are_not_lost() {
        let mut app = app_with_interfaces(60);
        let wheel = |dy: f32| {
            Event::Mouse(MouseEvent {
                x: 40.0,
                y: 300.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })
        };

        // One notch away from the user moves the list towards its start; at the
        // top that is already where it is.
        app.handle_event(&wheel(1.0), SIZE);
        assert_eq!(app.sidebar_scroll, 0);

        app.handle_event(&wheel(-1.0), SIZE);
        assert!(app.sidebar_scroll > 0, "a whole notch scrolled nothing");

        // Ten tenths of a notch must add up to the same thing a notch does,
        // rather than rounding to zero ten times.
        app.scroll_sidebar_to_top();
        app.wheel = wheel::Accumulator::default();
        let one_notch = {
            let mut probe = app_with_interfaces(60);
            probe.handle_event(&wheel(-1.0), SIZE);
            probe.sidebar_scroll
        };
        for _ in 0..10 {
            app.handle_event(&wheel(-0.1), SIZE);
        }
        assert_eq!(app.sidebar_scroll, one_notch);
    }

    #[test]
    fn a_close_request_closes_the_window() {
        let mut app = NetManagerApp::new();
        assert_eq!(app.handle_event(&Event::CloseRequested, SIZE), Action::Quit);
    }

    #[test]
    fn a_resize_is_believed_and_the_hit_test_follows_the_window() {
        // The panel is measured from the right-hand edge, so a control near it
        // moves when the window does. A hit-test still using the old width
        // would miss by exactly the difference.
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Profiles);

        let wide = (1400.0_f32, 900.0_f32);
        let at_wide = render_frame(&app, wide.0, wide.1)
            .hits()
            .iter()
            .rev()
            .find(|(t, _)| *t == Target::ProfileRemove(0))
            .map(|(_, r)| *r)
            .expect("remove button");
        let at_narrow = rect_of(&app, Target::ProfileRemove(0)).expect("remove button");
        assert!(
            at_wide.x > at_narrow.x,
            "the remove button did not follow the right-hand edge"
        );

        app.on_event(&Event::Resize {
            width: wide.0 as u32,
            height: wide.1 as u32,
        });
        assert_eq!(app.window_size, wide);

        let before = app.profiles.len();
        app.on_event(&Event::Mouse(MouseEvent {
            x: at_wide.x + at_wide.w / 2.0,
            y: at_wide.y + at_wide.h / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(
            app.profiles.len(),
            before - 1,
            "the click was tested against the pre-resize layout"
        );
    }

    #[test]
    fn render_believes_the_size_it_is_handed_on_the_very_first_frame() {
        // No `Resize` arrives before frame one, so `render` is the only thing
        // that knows the real size then. A window that ignored it would draw
        // 960x680 into whatever it was actually given.
        let mut app = NetManagerApp::new();
        let tree = app.render(1280.0, 800.0);
        assert_eq!(app.window_size, (1280.0, 800.0));
        assert!(
            tree.commands.iter().any(|cmd| matches!(
                *cmd,
                RenderCommand::FillRect { width, .. } if (width - 1280.0).abs() < 0.01
            )),
            "nothing was drawn at the width the frame was handed"
        );
    }

    #[test]
    fn a_window_dragged_smaller_than_its_minimum_is_drawn_at_the_minimum() {
        // Below this the sidebar and the panel would each be unusable. Drawing
        // at the requested size instead would put the panel at negative width,
        // and every rect in it at a coordinate no click can reach.
        let app = NetManagerApp::new();
        let frame = render_frame(&app, 100.0, 50.0);
        assert!(frame.width >= MIN_WIDTH);
        assert!(frame.height >= MIN_HEIGHT);
        for (target, rect) in frame.hits() {
            assert!(
                rect.w > 0.0 && rect.h > 0.0,
                "{target:?} was given an unclickable rect {rect:?}"
            );
        }
    }

    #[test]
    fn no_two_controls_claim_the_same_pixel_on_any_tab() {
        // The hit-test takes the topmost match, which is right for a button
        // drawn *on* a row. Anywhere else an overlap means one control is
        // unreachable, so the pairs that are allowed to overlap are named.
        let mut app = NetManagerApp::new();
        app.selected_wifi = Some(0);
        for tab in DetailTab::all() {
            app.set_tab(*tab);
            let frame = render_frame(&app, SIZE.0, SIZE.1);
            for (i, (a, ra)) in frame.hits().iter().enumerate() {
                for (b, rb) in frame.hits().iter().skip(i + 1) {
                    let overlaps = ra.x < rb.x + rb.w
                        && rb.x < ra.x + ra.w
                        && ra.y < rb.y + rb.h
                        && rb.y < ra.y + ra.h;
                    if !overlaps {
                        continue;
                    }
                    let on_top_of_a_row = matches!(
                        (a, b),
                        (Target::Wifi(_), Target::WifiConnect)
                            | (Target::Profile(_), Target::ProfileRemove(_))
                    );
                    assert!(
                        on_top_of_a_row,
                        "on {tab:?}, {a:?} at {ra:?} overlaps {b:?} at {rb:?}"
                    );
                }
            }
        }
    }

    /// A list long enough to overflow the detail panel at the smallest window
    /// the layout supports.
    fn app_with_overflowing_profiles() -> NetManagerApp {
        let mut app = NetManagerApp::new();
        app.set_tab(DetailTab::Profiles);
        for i in 0..24 {
            app.add_profile(&format!("profile-{i}"), SecurityLevel::Public, false);
        }
        app
    }

    #[test]
    fn a_list_row_past_the_bottom_of_its_panel_is_not_clickable() {
        // `render_tab_profiles` draws *every* profile, with no visible-row
        // limit — it relies on the `PushClip` around the detail panel to stop
        // the overflow. That works for the ink, because the compositor honours
        // the clip. It did not work for the clicks: the frame recorded each
        // row's full rect regardless, so with 24 profiles in a 320px-tall
        // window, twenty rows were "clickable" below the bottom edge of the
        // window and one straddled the status bar — a click on the status bar
        // selected a profile that was nowhere on screen.
        let app = app_with_overflowing_profiles();
        let frame = render_frame(&app, MIN_WIDTH, MIN_HEIGHT);
        let panel_bottom = MIN_HEIGHT - STATUS_BAR_HEIGHT;

        for (target, rect) in frame.hits() {
            assert!(
                rect.bottom() <= panel_bottom + 0.01 || !matches!(target, Target::Profile(_)),
                "{target:?} is clickable down to {} but the panel ends at {panel_bottom}",
                rect.bottom()
            );
        }

        let reachable = frame
            .hits()
            .iter()
            .filter(|(t, _)| matches!(t, Target::Profile(_)))
            .count();
        assert!(
            reachable < app.profiles.len(),
            "all {} profiles were recorded as clickable, so the clip is not \
             being applied to the hit rects at all",
            app.profiles.len()
        );
        assert!(reachable > 0, "the visible rows must still be clickable");
    }

    #[test]
    fn clicking_the_status_bar_does_not_select_an_off_screen_profile() {
        let mut app = app_with_overflowing_profiles();
        assert_eq!(app.selected_profile, None);
        // Just inside the status bar, which is where the first row to overflow
        // the panel spills to. Aiming at the middle of the status bar instead
        // would miss: that row only reaches a little way past the panel edge,
        // so a test that clicked the centre would pass even with the clip
        // removed and would prove nothing.
        let y = MIN_HEIGHT - STATUS_BAR_HEIGHT + 2.0;
        let size = (MIN_WIDTH, MIN_HEIGHT);
        app.handle_click(MIN_WIDTH / 2.0, y, MouseButton::Left, size);
        assert_eq!(
            app.selected_profile, None,
            "a click on the status bar selected a profile drawn behind it"
        );
    }
}

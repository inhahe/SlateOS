//! Slate OS VPN Connection Manager
//!
//! GUI application for managing VPN connections, profiles, and tunneling
//! configuration. Provides:
//! - VPN profile management (create, edit, delete, import, export)
//! - Connection lifecycle (connect, disconnect, reconnect)
//! - Protocol support: OpenVPN, WireGuard, IPSec, L2TP, PPTP, SSTP
//! - Authentication methods: certificate, username/password, pre-shared key, token
//! - Split tunneling configuration (route specific IPs through VPN)
//! - Kill switch (block traffic when VPN drops)
//! - Connection statistics (latency, throughput, uptime)
//! - Connection event log with timestamps
//! - Auto-reconnect and auto-connect on startup
//! - Data usage tracking per profile
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
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha Theme Colors
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Layout Constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1000.0;
const WINDOW_HEIGHT: f32 = 700.0;
/// The narrowest the layout survives: the sidebar is a fixed 280 wide and the
/// toolbar's last button ends at 606, so below this the detail panel has no
/// room and the toolbar wraps under itself.
const MIN_WIDTH: f32 = 640.0;
/// The shortest the layout survives: title bar, toolbar, tab bar and status bar
/// are all fixed, so below this the detail panel would have negative height.
const MIN_HEIGHT: f32 = 320.0;
const TITLE_BAR_HEIGHT: f32 = 40.0;
const TOOLBAR_HEIGHT: f32 = 36.0;
const SIDEBAR_WIDTH: f32 = 280.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const SIDEBAR_ITEM_HEIGHT: f32 = 56.0;
const SECTION_PADDING: f32 = 16.0;
const FIELD_HEIGHT: f32 = 28.0;
const FIELD_LABEL_WIDTH: f32 = 130.0;
const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_WIDTH: f32 = 110.0;
const LOG_ENTRY_HEIGHT: f32 = 22.0;
/// Height reserved under the log for its "N more" line. Reserved whether
/// or not the line is drawn, so how many entries fit does not depend on
/// whether any are hidden.
const LOG_MORE_HEIGHT: f32 = 16.0;
const TAB_HEIGHT: f32 = 32.0;
/// Point size of a profile's free-text notes.
const NOTES_FONT_SIZE: f32 = 12.0;
/// Line spacing of a profile's notes.
const NOTES_LINE_HEIGHT: f32 = 17.0;

// ============================================================================
// Core Data Types
// ============================================================================

/// Supported VPN protocols.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VpnProtocol {
    OpenVPN,
    WireGuard,
    IPSec,
    L2TP,
    PPTP,
    SSTP,
}

impl VpnProtocol {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenVPN => "OpenVPN",
            Self::WireGuard => "WireGuard",
            Self::IPSec => "IPSec/IKEv2",
            Self::L2TP => "L2TP/IPSec",
            Self::PPTP => "PPTP",
            Self::SSTP => "SSTP",
        }
    }

    /// Color for protocol indicator in the UI.
    pub fn color(self) -> Color {
        match self {
            Self::OpenVPN => GREEN,
            Self::WireGuard => BLUE,
            Self::IPSec => LAVENDER,
            Self::L2TP => PEACH,
            Self::PPTP => YELLOW,
            Self::SSTP => SUBTEXT0,
        }
    }

    /// All protocol variants for iteration.
    pub fn all() -> &'static [Self] {
        &[
            Self::OpenVPN,
            Self::WireGuard,
            Self::IPSec,
            Self::L2TP,
            Self::PPTP,
            Self::SSTP,
        ]
    }

    /// The next protocol in [`Self::all`] order, wrapping.
    ///
    /// The dialog's Protocol box cycles rather than opening a list: there are
    /// six of them, they fit in a click each, and a popup list is a second
    /// surface to hit-test for no gain.
    pub fn next(self) -> Self {
        let all = Self::all();
        let i = all.iter().position(|p| *p == self).unwrap_or(0);
        // Written as a comparison rather than `% all.len()` so the wrap needs
        // no division, and so there is no divisor to reason about being zero.
        let next = i.saturating_add(1);
        let next = if next >= all.len() { 0 } else { next };
        all.get(next).copied().unwrap_or(self)
    }

    /// Default port for the protocol.
    pub fn default_port(self) -> u16 {
        match self {
            Self::OpenVPN => 1194,
            Self::WireGuard => 51820,
            Self::IPSec => 500,
            Self::L2TP => 1701,
            Self::PPTP => 1723,
            Self::SSTP => 443,
        }
    }
}

/// Authentication method for a VPN connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthMethod {
    /// X.509 certificate file path.
    Certificate { cert_path: String, key_path: String },
    /// Username/password credentials.
    UsernamePassword { username: String, password: String },
    /// Pre-shared key.
    PreSharedKey { key: String },
    /// Token-based (e.g., TOTP/HOTP).
    Token { token: String },
}

impl AuthMethod {
    /// Label for the auth method kind.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Certificate { .. } => "Certificate",
            Self::UsernamePassword { .. } => "Username/Password",
            Self::PreSharedKey { .. } => "Pre-Shared Key",
            Self::Token { .. } => "Token",
        }
    }

    /// Create a default certificate auth.
    pub fn default_certificate() -> Self {
        Self::Certificate {
            cert_path: String::new(),
            key_path: String::new(),
        }
    }

    /// Create a default username/password auth.
    pub fn default_username_password() -> Self {
        Self::UsernamePassword {
            username: String::new(),
            password: String::new(),
        }
    }

    /// Create a default pre-shared key auth.
    pub fn default_psk() -> Self {
        Self::PreSharedKey { key: String::new() }
    }

    /// Create a default token auth.
    pub fn default_token() -> Self {
        Self::Token {
            token: String::new(),
        }
    }

    /// The next auth *kind*, wrapping, with empty credentials.
    ///
    /// Cycling discards whatever was typed into the previous kind, because
    /// there is nothing to carry: a certificate path is not a password and a
    /// token is not a pre-shared key. Keeping the old string would put a
    /// certificate path in the password field, which is worse than blank.
    pub fn next(&self) -> Self {
        match self {
            Self::Certificate { .. } => Self::default_username_password(),
            Self::UsernamePassword { .. } => Self::default_psk(),
            Self::PreSharedKey { .. } => Self::default_token(),
            Self::Token { .. } => Self::default_certificate(),
        }
    }
}

/// Connection status of a VPN profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error(String),
}

impl ConnectionStatus {
    /// Human-readable label.
    pub fn label(&self) -> &str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting...",
            Self::Connected => "Connected",
            Self::Reconnecting => "Reconnecting...",
            Self::Error(_) => "Error",
        }
    }

    /// Indicator color for the status.
    pub fn color(&self) -> Color {
        match self {
            Self::Connected => GREEN,
            Self::Connecting | Self::Reconnecting => YELLOW,
            Self::Disconnected => OVERLAY0,
            Self::Error(_) => RED,
        }
    }

    /// Whether the connection is active (connected or reconnecting).
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Connected | Self::Reconnecting)
    }
}

/// Protocol-specific settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolSettings {
    OpenVpn {
        config_file: String,
        cipher: String,
        compression: bool,
    },
    WireGuard {
        peer_public_key: String,
        endpoint: String,
        persistent_keepalive: u16,
    },
    IPSec {
        ike_version: u8,
        phase1_algo: String,
        phase2_algo: String,
    },
    Generic,
}

impl ProtocolSettings {
    /// Create default settings for a given protocol.
    pub fn for_protocol(proto: VpnProtocol) -> Self {
        match proto {
            VpnProtocol::OpenVPN => Self::OpenVpn {
                config_file: String::new(),
                cipher: String::from("AES-256-GCM"),
                compression: false,
            },
            VpnProtocol::WireGuard => Self::WireGuard {
                peer_public_key: String::new(),
                endpoint: String::new(),
                persistent_keepalive: 25,
            },
            VpnProtocol::IPSec => Self::IPSec {
                ike_version: 2,
                phase1_algo: String::from("aes256-sha256-modp2048"),
                phase2_algo: String::from("aes256-sha256"),
            },
            _ => Self::Generic,
        }
    }
}

/// A VPN profile containing all configuration needed to establish a connection.
#[derive(Clone, Debug)]
pub struct VpnProfile {
    pub id: u32,
    pub name: String,
    pub server_address: String,
    pub port: u16,
    pub protocol: VpnProtocol,
    pub auth_method: AuthMethod,
    pub auto_connect: bool,
    pub dns_override: Vec<String>,
    pub split_tunnel: bool,
    pub allowed_ips: Vec<String>,
    pub kill_switch: bool,
    pub mtu: u16,
    pub notes: String,
    pub enabled: bool,
    pub auto_reconnect: bool,
    pub protocol_settings: ProtocolSettings,
    // Cumulative usage stats
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub total_connection_time_secs: u64,
}

impl VpnProfile {
    /// Create a new profile with default values.
    pub fn new(id: u32, name: &str, server: &str, protocol: VpnProtocol) -> Self {
        Self {
            id,
            name: name.to_string(),
            server_address: server.to_string(),
            port: protocol.default_port(),
            protocol,
            auth_method: AuthMethod::default_username_password(),
            auto_connect: false,
            dns_override: Vec::new(),
            split_tunnel: false,
            allowed_ips: Vec::new(),
            kill_switch: false,
            mtu: 1500,
            notes: String::new(),
            enabled: true,
            auto_reconnect: true,
            protocol_settings: ProtocolSettings::for_protocol(protocol),
            total_bytes_sent: 0,
            total_bytes_received: 0,
            total_connection_time_secs: 0,
        }
    }

    /// Validate the profile configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("Profile name is required".into());
        }
        if self.server_address.is_empty() {
            return Err("Server address is required".into());
        }
        if self.port == 0 {
            return Err("Port must be non-zero".into());
        }
        for dns in &self.dns_override {
            if !is_valid_ipv4(dns) {
                return Err(format!("Invalid DNS server: {dns}"));
            }
        }
        for ip in &self.allowed_ips {
            if !is_valid_cidr_or_ip(ip) {
                return Err(format!("Invalid IP/CIDR in split tunnel: {ip}"));
            }
        }
        if self.mtu < 576 || self.mtu > 9000 {
            return Err(format!(
                "MTU must be between 576 and 9000, got {}",
                self.mtu
            ));
        }
        Ok(())
    }

    /// Export profile to a simple text representation.
    pub fn export_text(&self) -> String {
        let mut lines = Vec::new();
        lines.push("[VpnProfile]".to_string());
        lines.push(format!("name={}", self.name));
        lines.push(format!("server={}", self.server_address));
        lines.push(format!("port={}", self.port));
        lines.push(format!("protocol={}", self.protocol.label()));
        lines.push(format!("auth={}", self.auth_method.label()));
        lines.push(format!("auto_connect={}", self.auto_connect));
        lines.push(format!("kill_switch={}", self.kill_switch));
        lines.push(format!("split_tunnel={}", self.split_tunnel));
        lines.push(format!("mtu={}", self.mtu));
        lines.push(format!("auto_reconnect={}", self.auto_reconnect));
        if !self.dns_override.is_empty() {
            lines.push(format!("dns={}", self.dns_override.join(",")));
        }
        if !self.allowed_ips.is_empty() {
            lines.push(format!("allowed_ips={}", self.allowed_ips.join(",")));
        }
        if !self.notes.is_empty() {
            lines.push(format!("notes={}", self.notes));
        }
        lines.join("\n")
    }
}

/// An active VPN connection (runtime state for a connected profile).
#[derive(Clone, Debug)]
pub struct VpnConnection {
    pub profile_id: u32,
    pub status: ConnectionStatus,
    pub local_ip: String,
    pub remote_ip: String,
    pub latency_ms: u32,
    pub uptime_secs: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub connected_since: Option<u64>,
}

impl VpnConnection {
    /// Create a new connection in disconnected state for the given profile.
    pub fn new(profile_id: u32) -> Self {
        Self {
            profile_id,
            status: ConnectionStatus::Disconnected,
            local_ip: String::new(),
            remote_ip: String::new(),
            latency_ms: 0,
            uptime_secs: 0,
            bytes_sent: 0,
            bytes_received: 0,
            connected_since: None,
        }
    }

    /// Format uptime as HH:MM:SS.
    pub fn format_uptime(&self) -> String {
        let h = self.uptime_secs / 3600;
        let m = (self.uptime_secs % 3600) / 60;
        let s = self.uptime_secs % 60;
        format!("{h:02}:{m:02}:{s:02}")
    }
}

/// A timestamped connection log entry.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub timestamp: u64,
    pub profile_name: String,
    pub message: String,
    pub level: LogLevel,
}

/// Severity level for log entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Info => BLUE,
            Self::Warning => YELLOW,
            Self::Error => RED,
        }
    }
}

/// Sort order for the profile list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Name,
    Status,
    Protocol,
}

impl SortOrder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Status => "Status",
            Self::Protocol => "Protocol",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Name, Self::Status, Self::Protocol]
    }

    /// The next order in [`Self::all`] order, wrapping.
    ///
    /// The toolbar's Sort control cycles rather than dropping a menu: three
    /// options is at most three clicks to reach any of them, and a popup menu
    /// is a second surface to hit-test for no gain.
    pub fn next(self) -> Self {
        let all = Self::all();
        let i = all.iter().position(|o| *o == self).unwrap_or(0);
        // A comparison rather than `% all.len()`: the wrap needs no division,
        // so there is no divisor to reason about being zero.
        let next = i.saturating_add(1);
        let next = if next >= all.len() { 0 } else { next };
        all.get(next).copied().unwrap_or(self)
    }
}

/// Which tab is shown in the detail panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailTab {
    Overview,
    Connection,
    SplitTunnel,
    ProtocolConfig,
    Log,
    Stats,
}

impl DetailTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Connection => "Connection",
            Self::SplitTunnel => "Split Tunnel",
            Self::ProtocolConfig => "Protocol",
            Self::Log => "Log",
            Self::Stats => "Statistics",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Overview,
            Self::Connection,
            Self::SplitTunnel,
            Self::ProtocolConfig,
            Self::Log,
            Self::Stats,
        ]
    }
}

// ============================================================================
// VPN Manager (core logic)
// ============================================================================

/// Central VPN management state.
pub struct VpnManager {
    pub profiles: Vec<VpnProfile>,
    pub connections: Vec<VpnConnection>,
    pub log: VecDeque<LogEntry>,
    pub selected_profile: Option<usize>,
    pub current_tab: DetailTab,
    pub sort_order: SortOrder,
    pub global_kill_switch: bool,
    pub last_connected_id: Option<u32>,
    next_profile_id: u32,
    next_log_timestamp: u64,
    pub editing_profile: Option<VpnProfile>,
    pub show_add_dialog: bool,
    pub scroll_offset: f32,
    /// First log row drawn, counted in rows rather than pixels: the log
    /// only ever scrolls a whole row at a time, so a pixel offset could only
    /// express positions the renderer then rounds away. A value past the end
    /// is not an error, and shows the last page.
    pub log_scroll_offset: usize,
    pub search_query: String,
    /// Which text box holds the keyboard, if any.
    pub focus: Option<Field>,
    /// Why the last Save was refused, shown inside the dialog.
    ///
    /// Not `status_message`: the dialog draws a 63%-black scrim over the whole
    /// window, and the status bar is behind it. A complaint the user cannot
    /// read is a Save button that does nothing.
    pub dialog_error: String,
    /// What has been typed into the split-tunnel tab's new-range box.
    pub allowed_ip_input: String,
    /// The last thing the user did, or the reason the last thing failed.
    ///
    /// Every state method that can fail returns `Err(String)`. Before the
    /// window existed those strings had no reader and every caller dropped
    /// them, so a Connect that was refused because the profile was disabled
    /// looked exactly like a Connect that worked. The status bar reads this.
    pub status_message: String,
    /// The size the last frame was drawn at, so a click is tested against the
    /// geometry that produced it rather than against the default constants.
    pub window_size: (f32, f32),
    /// Carries the fractional part of a trackpad's scroll between events, so
    /// slow scrolling moves rather than rounding to nothing every time.
    wheel: wheel::Accumulator,
    /// Milliseconds seen by [`Self::advance`] that did not yet make a whole
    /// second. Kept so a sub-second tick interval still moves the clock.
    uptime_carry_ms: u64,
}

impl VpnManager {
    /// Create a new VPN manager with sample data.
    pub fn new() -> Self {
        let profiles = sample_profiles();
        let connections = profiles.iter().map(|p| VpnConnection::new(p.id)).collect();
        let log = sample_log();
        Self {
            profiles,
            connections,
            log,
            selected_profile: Some(0),
            current_tab: DetailTab::Overview,
            sort_order: SortOrder::Name,
            global_kill_switch: false,
            last_connected_id: None,
            next_profile_id: 100,
            next_log_timestamp: 1_700_000_100,
            editing_profile: None,
            show_add_dialog: false,
            scroll_offset: 0.0,
            log_scroll_offset: 0,
            search_query: String::new(),
            focus: None,
            dialog_error: String::new(),
            allowed_ip_input: String::new(),
            status_message: String::new(),
            window_size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            wheel: wheel::Accumulator::default(),
            uptime_carry_ms: 0,
        }
    }
}

impl Default for VpnManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VpnManager {
    /// Add a new profile. Returns the ID assigned.
    pub fn add_profile(&mut self, mut profile: VpnProfile) -> Result<u32, String> {
        profile.validate()?;
        profile.id = self.next_profile_id;
        self.next_profile_id = self.next_profile_id.wrapping_add(1);
        let id = profile.id;
        self.connections.push(VpnConnection::new(id));
        self.add_log(&profile.name, "Profile created", LogLevel::Info);
        self.profiles.push(profile);
        Ok(id)
    }

    /// Remove a profile by index. Returns the removed profile, or None if invalid.
    pub fn remove_profile(&mut self, index: usize) -> Option<VpnProfile> {
        if index >= self.profiles.len() {
            return None;
        }
        let profile = self.profiles.remove(index);
        // Remove matching connection
        self.connections.retain(|c| c.profile_id != profile.id);
        self.add_log(&profile.name, "Profile deleted", LogLevel::Info);
        // Fix selection
        if self.profiles.is_empty() {
            self.selected_profile = None;
        } else if let Some(sel) = self.selected_profile
            && sel >= self.profiles.len()
        {
            self.selected_profile = Some(self.profiles.len().saturating_sub(1));
        }
        Some(profile)
    }

    /// Update a profile at the given index.
    pub fn update_profile(&mut self, index: usize, updated: VpnProfile) -> Result<(), String> {
        updated.validate()?;
        let new_name = updated.name.clone();
        let slot = self
            .profiles
            .get_mut(index)
            .ok_or("Invalid profile index")?;
        let old_name = std::mem::replace(slot, updated).name;
        self.add_log(
            &new_name,
            &format!("Profile updated (was: {old_name})"),
            LogLevel::Info,
        );
        Ok(())
    }

    /// Get a reference to the currently selected profile.
    pub fn selected(&self) -> Option<&VpnProfile> {
        self.selected_profile.and_then(|i| self.profiles.get(i))
    }

    /// Get the connection state for a profile by ID.
    pub fn connection_for(&self, profile_id: u32) -> Option<&VpnConnection> {
        self.connections.iter().find(|c| c.profile_id == profile_id)
    }

    /// Get a mutable connection for a profile by ID.
    fn connection_for_mut(&mut self, profile_id: u32) -> Option<&mut VpnConnection> {
        self.connections
            .iter_mut()
            .find(|c| c.profile_id == profile_id)
    }

    /// Get the connection state for the currently selected profile.
    pub fn selected_connection(&self) -> Option<&VpnConnection> {
        self.selected().and_then(|p| self.connection_for(p.id))
    }

    /// Initiate connection for the profile at the given index.
    pub fn connect(&mut self, index: usize) -> Result<(), String> {
        let profile = self.profiles.get(index).ok_or("Invalid profile index")?;
        if !profile.enabled {
            return Err("Profile is disabled".into());
        }
        let pid = profile.id;
        let name = profile.name.clone();
        let server = profile.server_address.clone();

        let ts = self.next_log_timestamp;
        if let Some(conn) = self.connection_for_mut(pid) {
            if conn.status == ConnectionStatus::Connected {
                return Err("Already connected".into());
            }
            conn.status = ConnectionStatus::Connecting;
            conn.local_ip = String::from("10.8.0.2");
            conn.remote_ip = server.clone();
            conn.latency_ms = 42;
            conn.uptime_secs = 0;
            conn.bytes_sent = 0;
            conn.bytes_received = 0;
            conn.connected_since = Some(ts);
        }

        self.last_connected_id = Some(pid);
        self.add_log(&name, &format!("Connecting to {server}..."), LogLevel::Info);

        // Simulate immediate connection success for UI purposes
        if let Some(conn) = self.connection_for_mut(pid) {
            conn.status = ConnectionStatus::Connected;
        }
        self.add_log(&name, "Connected successfully", LogLevel::Info);

        Ok(())
    }

    /// Disconnect the profile at the given index.
    pub fn disconnect(&mut self, index: usize) -> Result<(), String> {
        let profile = self.profiles.get(index).ok_or("Invalid profile index")?;
        let pid = profile.id;
        let name = profile.name.clone();

        // Read stats from connection before modifying it
        let (was_active, sent, recv, uptime) = {
            let conn = match self.connection_for(pid) {
                Some(c) => c,
                None => return Err("No connection found".into()),
            };
            let active = conn.status.is_active() || conn.status == ConnectionStatus::Connecting;
            (
                active,
                conn.bytes_sent,
                conn.bytes_received,
                conn.uptime_secs,
            )
        };

        if !was_active {
            return Err("Not connected".into());
        }

        // Accumulate stats to profile
        if let Some(profile) = self.profiles.iter_mut().find(|p| p.id == pid) {
            profile.total_bytes_sent = profile.total_bytes_sent.saturating_add(sent);
            profile.total_bytes_received = profile.total_bytes_received.saturating_add(recv);
            profile.total_connection_time_secs =
                profile.total_connection_time_secs.saturating_add(uptime);
        }

        // Reset connection state
        if let Some(conn) = self.connection_for_mut(pid) {
            conn.status = ConnectionStatus::Disconnected;
            conn.local_ip.clear();
            conn.latency_ms = 0;
            conn.uptime_secs = 0;
            conn.bytes_sent = 0;
            conn.bytes_received = 0;
            conn.connected_since = None;
        }

        self.add_log(&name, "Disconnected", LogLevel::Info);
        Ok(())
    }

    /// Reconnect (disconnect then connect) the profile at the given index.
    pub fn reconnect(&mut self, index: usize) -> Result<(), String> {
        let profile = self.profiles.get(index).ok_or("Invalid profile index")?;
        let pid = profile.id;
        let name = profile.name.clone();

        if let Some(conn) = self.connection_for_mut(pid) {
            conn.status = ConnectionStatus::Reconnecting;
        }
        self.add_log(&name, "Reconnecting...", LogLevel::Warning);

        // Simulate: disconnect then reconnect
        let _ = self.disconnect(index);
        self.connect(index)
    }

    /// Quick connect to the last used profile.
    pub fn quick_connect(&mut self) -> Result<(), String> {
        let id = self.last_connected_id.ok_or("No previous connection")?;
        let index = self
            .profiles
            .iter()
            .position(|p| p.id == id)
            .ok_or("Last connected profile no longer exists")?;
        self.connect(index)
    }

    /// Disconnect all active connections.
    pub fn disconnect_all(&mut self) {
        let active_indices: Vec<usize> = self
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                self.connections
                    .iter()
                    .any(|c| c.profile_id == p.id && c.status.is_active())
            })
            .map(|(i, _)| i)
            .collect();

        for idx in active_indices.into_iter().rev() {
            let _ = self.disconnect(idx);
        }
    }

    /// Set sort order and re-sort profiles.
    pub fn set_sort_order(&mut self, order: SortOrder) {
        self.sort_order = order;
        self.sort_profiles();
    }

    /// Sort profiles by the current sort order, keeping the selection on the
    /// profile the user actually chose.
    ///
    /// `selected_profile` is an *index* into `profiles`, and sorting moves rows
    /// out from under it. Without the re-lookup below, pressing the toolbar's
    /// Sort control silently moved the selection to whichever profile happened
    /// to land on the old row number — and every control that acts on "the
    /// selected profile" (Connect, Disconnect, Remove, every toggle on the
    /// detail panel) then acted on that one instead. The list is small, so a
    /// linear search after a sort costs nothing worth measuring.
    pub fn sort_profiles(&mut self) {
        let chosen = self
            .selected_profile
            .and_then(|i| self.profiles.get(i))
            .map(|p| p.id);
        let connections = &self.connections;
        match self.sort_order {
            SortOrder::Name => self.profiles.sort_by(|a, b| a.name.cmp(&b.name)),
            SortOrder::Protocol => self
                .profiles
                .sort_by(|a, b| a.protocol.label().cmp(b.protocol.label())),
            SortOrder::Status => {
                self.profiles.sort_by(|a, b| {
                    let status_a = connections
                        .iter()
                        .find(|c| c.profile_id == a.id)
                        .map_or(3, |c| status_sort_key(&c.status));
                    let status_b = connections
                        .iter()
                        .find(|c| c.profile_id == b.id)
                        .map_or(3, |c| status_sort_key(&c.status));
                    status_a.cmp(&status_b)
                });
            }
        }
        if let Some(id) = chosen {
            self.selected_profile = self.profiles.iter().position(|p| p.id == id);
        }
    }

    /// Toggle the kill switch for a specific profile.
    pub fn toggle_kill_switch(&mut self, index: usize) {
        if let Some(profile) = self.profiles.get_mut(index) {
            profile.kill_switch = !profile.kill_switch;
            let state = if profile.kill_switch {
                "enabled"
            } else {
                "disabled"
            };
            let name = profile.name.clone();
            self.add_log(&name, &format!("Kill switch {state}"), LogLevel::Info);
        }
    }

    /// Toggle the global kill switch.
    pub fn toggle_global_kill_switch(&mut self) {
        self.global_kill_switch = !self.global_kill_switch;
        let state = if self.global_kill_switch {
            "enabled"
        } else {
            "disabled"
        };
        self.add_log(
            "System",
            &format!("Global kill switch {state}"),
            LogLevel::Warning,
        );
    }

    /// Add a DNS override server to a profile.
    pub fn add_dns_override(&mut self, index: usize, dns: &str) -> Result<(), String> {
        if !is_valid_ipv4(dns) {
            return Err(format!("Invalid DNS server address: {dns}"));
        }
        let profile = self
            .profiles
            .get_mut(index)
            .ok_or("Invalid profile index")?;
        if profile.dns_override.iter().any(|d| d == dns) {
            return Err("DNS server already exists".into());
        }
        profile.dns_override.push(dns.to_string());
        Ok(())
    }

    /// Remove a DNS override server from a profile.
    pub fn remove_dns_override(
        &mut self,
        profile_index: usize,
        dns_index: usize,
    ) -> Result<(), String> {
        let profile = self
            .profiles
            .get_mut(profile_index)
            .ok_or("Invalid profile index")?;
        if dns_index >= profile.dns_override.len() {
            return Err("Invalid DNS index".into());
        }
        profile.dns_override.remove(dns_index);
        Ok(())
    }

    /// Add an allowed IP range for split tunneling.
    pub fn add_allowed_ip(&mut self, index: usize, ip: &str) -> Result<(), String> {
        if !is_valid_cidr_or_ip(ip) {
            return Err(format!("Invalid IP/CIDR: {ip}"));
        }
        let profile = self
            .profiles
            .get_mut(index)
            .ok_or("Invalid profile index")?;
        if profile.allowed_ips.iter().any(|a| a == ip) {
            return Err("IP range already exists".into());
        }
        profile.allowed_ips.push(ip.to_string());
        Ok(())
    }

    /// Remove an allowed IP range from split tunneling.
    pub fn remove_allowed_ip(
        &mut self,
        profile_index: usize,
        ip_index: usize,
    ) -> Result<(), String> {
        let profile = self
            .profiles
            .get_mut(profile_index)
            .ok_or("Invalid profile index")?;
        if ip_index >= profile.allowed_ips.len() {
            return Err("Invalid IP index".into());
        }
        profile.allowed_ips.remove(ip_index);
        Ok(())
    }

    /// Toggle split tunnel on/off for a profile.
    pub fn toggle_split_tunnel(&mut self, index: usize) {
        if let Some(profile) = self.profiles.get_mut(index) {
            profile.split_tunnel = !profile.split_tunnel;
        }
    }

    /// Toggle OpenVPN's compression flag for a profile.
    ///
    /// Returns whether anything changed: the flag only exists on OpenVPN, and
    /// the Protocol tab only draws the row for OpenVPN profiles, so a `false`
    /// here means the caller reached a control that is not on screen.
    pub fn toggle_compression(&mut self, index: usize) -> bool {
        let Some(profile) = self.profiles.get_mut(index) else {
            return false;
        };
        if let ProtocolSettings::OpenVpn { compression, .. } = &mut profile.protocol_settings {
            *compression = !*compression;
            true
        } else {
            false
        }
    }

    /// Toggle auto-connect for a profile.
    pub fn toggle_auto_connect(&mut self, index: usize) {
        if let Some(profile) = self.profiles.get_mut(index) {
            profile.auto_connect = !profile.auto_connect;
        }
    }

    /// Toggle auto-reconnect for a profile.
    pub fn toggle_auto_reconnect(&mut self, index: usize) {
        if let Some(profile) = self.profiles.get_mut(index) {
            profile.auto_reconnect = !profile.auto_reconnect;
        }
    }

    /// Toggle the enabled state of a profile.
    pub fn toggle_enabled(&mut self, index: usize) {
        if let Some(profile) = self.profiles.get_mut(index) {
            profile.enabled = !profile.enabled;
            if !profile.enabled {
                // Disconnect if disabling
                let pid = profile.id;
                if let Some(conn) = self.connections.iter_mut().find(|c| c.profile_id == pid)
                    && (conn.status.is_active() || conn.status == ConnectionStatus::Connecting)
                {
                    conn.status = ConnectionStatus::Disconnected;
                    conn.local_ip.clear();
                    conn.latency_ms = 0;
                    conn.connected_since = None;
                }
            }
        }
    }

    /// Set the current detail tab.
    pub fn set_tab(&mut self, tab: DetailTab) {
        self.current_tab = tab;
    }

    /// Select a profile by index.
    pub fn select_profile(&mut self, index: usize) {
        if index < self.profiles.len() {
            self.selected_profile = Some(index);
        }
    }

    /// Export all profiles as text.
    pub fn export_all(&self) -> String {
        self.profiles
            .iter()
            .map(|p| p.export_text())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Import a profile from text representation. Returns ID on success.
    pub fn import_profile(&mut self, text: &str) -> Result<u32, String> {
        let profile = parse_profile_text(text, self.next_profile_id)?;
        self.add_profile(profile)
    }

    /// Get the number of active connections.
    pub fn active_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|c| c.status.is_active())
            .count()
    }

    /// Get the total data transferred across all active connections.
    pub fn total_transfer(&self) -> (u64, u64) {
        self.connections
            .iter()
            .filter(|c| c.status.is_active())
            .fold((0u64, 0u64), |(sent, recv), c| {
                (
                    sent.saturating_add(c.bytes_sent),
                    recv.saturating_add(c.bytes_received),
                )
            })
    }

    /// Get profiles matching the current search query.
    pub fn filtered_profiles(&self) -> Vec<usize> {
        if self.search_query.is_empty() {
            return (0..self.profiles.len()).collect();
        }
        let query = self.search_query.to_lowercase();
        self.profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.name.to_lowercase().contains(&query)
                    || p.server_address.to_lowercase().contains(&query)
                    || p.protocol.label().to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Add a log entry.
    fn add_log(&mut self, profile_name: &str, message: &str, level: LogLevel) {
        self.log.push_back(LogEntry {
            timestamp: self.next_log_timestamp,
            profile_name: profile_name.to_string(),
            message: message.to_string(),
            level,
        });
        self.next_log_timestamp = self.next_log_timestamp.wrapping_add(1);
        // Keep log bounded
        while self.log.len() > 500 {
            self.log.pop_front();
        }
    }

    /// Clear the connection log.
    pub fn clear_log(&mut self) {
        self.log.clear();
        self.log_scroll_offset = 0;
    }

    /// Move the log `delta` rows, negative for up (towards the newest entry).
    ///
    /// Clamped at the top only: how many rows fit depends on the panel height,
    /// which this method is not given. The render clamps against what it is
    /// actually drawing, so an offset past the end shows the last page.
    pub fn scroll_log_by(&mut self, delta: isize) {
        self.log_scroll_offset = scroll_window::shift(self.log_scroll_offset, delta);
    }

    /// Back to the newest log entry.
    pub fn scroll_log_to_top(&mut self) {
        self.log_scroll_offset = 0;
    }

    /// Begin editing by opening the add dialog with a new blank profile.
    pub fn start_add_profile(&mut self) {
        let proto = VpnProtocol::WireGuard;
        self.editing_profile = Some(VpnProfile::new(0, "", "", proto));
        self.show_add_dialog = true;
        self.dialog_error.clear();
    }

    /// Begin editing the currently selected profile.
    pub fn start_edit_profile(&mut self) {
        if let Some(profile) = self.selected().cloned() {
            self.editing_profile = Some(profile);
            self.show_add_dialog = true;
            self.dialog_error.clear();
        }
    }

    /// Cancel editing.
    pub fn cancel_edit(&mut self) {
        self.editing_profile = None;
        self.show_add_dialog = false;
        self.dialog_error.clear();
    }

    /// Confirm the add/edit dialog.
    ///
    /// The dialog only closes on success. It used to take the profile out of
    /// `editing_profile` and clear `show_add_dialog` *before* validating, so a
    /// Save rejected for one blank field closed the dialog and threw away
    /// everything else that had been typed into it — with the complaint
    /// returned to a caller that no longer had a dialog to show it in. That
    /// went unnoticed for as long as nothing clicked the button.
    pub fn confirm_edit(&mut self) -> Result<(), String> {
        let profile = self
            .editing_profile
            .as_ref()
            .ok_or("No profile being edited")?
            .clone();

        // A profile that already exists is an update; one the list has never
        // seen is an addition. New profiles carry id 0, which `add_profile`
        // replaces with a fresh one, and no stored profile can hold.
        let outcome = if let Some(idx) = self.profiles.iter().position(|p| p.id == profile.id) {
            self.update_profile(idx, profile)
        } else {
            self.add_profile(profile).map(|_| ())
        };
        if outcome.is_ok() {
            self.editing_profile = None;
            self.show_add_dialog = false;
        }
        outcome
    }

    /// Simulate connection data changing (for UI testing).
    pub fn simulate_traffic(&mut self, profile_id: u32, sent: u64, received: u64) {
        if let Some(conn) = self.connection_for_mut(profile_id)
            && conn.status == ConnectionStatus::Connected
        {
            conn.bytes_sent = conn.bytes_sent.saturating_add(sent);
            conn.bytes_received = conn.bytes_received.saturating_add(received);
            conn.uptime_secs = conn.uptime_secs.saturating_add(1);
        }
    }

    /// Age every live connection by `elapsed_ms` of wall clock.
    ///
    /// Driven from the window's tick. Without it a Connected profile reads
    /// `00:00:00` for as long as it stays up, and the Statistics tab's session
    /// figures never move: a plausible zero over a clock that never started,
    /// which is `known-issues.md` lesson 47.
    ///
    /// Only *time* advances here. Byte counters are left alone, because the
    /// seconds since the user pressed Connect are a real measurement this
    /// program can make, whereas traffic on a tunnel it is not carrying would
    /// be a number invented to look busy.
    ///
    /// The sub-second remainder is carried rather than truncated: at a 500 ms
    /// tick, rounding each tick down to zero whole seconds would freeze the
    /// clock exactly as completely as having no tick at all.
    ///
    /// Returns whether any connection moved, so the caller can decline to
    /// repaint a window with nothing connected.
    pub fn advance(&mut self, elapsed_ms: u64) -> bool {
        self.uptime_carry_ms = self.uptime_carry_ms.saturating_add(elapsed_ms);
        let whole = self.uptime_carry_ms / 1000;
        if whole == 0 {
            return false;
        }
        self.uptime_carry_ms = self
            .uptime_carry_ms
            .saturating_sub(whole.saturating_mul(1000));
        let mut moved = false;
        for conn in &mut self.connections {
            if conn.status.is_active() {
                conn.uptime_secs = conn.uptime_secs.saturating_add(whole);
                moved = true;
            }
        }
        moved
    }
}

// ============================================================================
// Validation Helpers
// ============================================================================

/// Basic IPv4 address validation.
fn is_valid_ipv4(addr: &str) -> bool {
    let parts: Vec<&str> = addr.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| p.parse::<u8>().is_ok())
}

/// Validate an IP address or CIDR notation (e.g. 10.0.0.0/8).
fn is_valid_cidr_or_ip(s: &str) -> bool {
    if let Some((ip, prefix)) = s.split_once('/') {
        if !is_valid_ipv4(ip) {
            return false;
        }
        match prefix.parse::<u8>() {
            Ok(p) => p <= 32,
            Err(_) => false,
        }
    } else {
        is_valid_ipv4(s)
    }
}

/// Numeric sort key for connection status (connected first).
fn status_sort_key(status: &ConnectionStatus) -> u8 {
    match status {
        ConnectionStatus::Connected => 0,
        ConnectionStatus::Connecting | ConnectionStatus::Reconnecting => 1,
        ConnectionStatus::Error(_) => 2,
        ConnectionStatus::Disconnected => 3,
    }
}

/// Parse a simple text representation of a profile.
fn parse_profile_text(text: &str, default_id: u32) -> Result<VpnProfile, String> {
    let mut name = String::new();
    let mut server = String::new();
    let mut port: Option<u16> = None;
    let mut protocol = VpnProtocol::WireGuard;
    let mut auto_connect = false;
    let mut kill_switch = false;
    let mut split_tunnel = false;
    let mut mtu: u16 = 1500;
    let mut auto_reconnect = true;
    let mut dns_servers: Vec<String> = Vec::new();
    let mut allowed_ips: Vec<String> = Vec::new();
    let mut notes = String::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "name" => name = value.trim().to_string(),
                "server" => server = value.trim().to_string(),
                "port" => {
                    port = value.trim().parse().ok();
                }
                "protocol" => {
                    protocol = match value.trim() {
                        "OpenVPN" => VpnProtocol::OpenVPN,
                        "WireGuard" => VpnProtocol::WireGuard,
                        "IPSec/IKEv2" => VpnProtocol::IPSec,
                        "L2TP/IPSec" => VpnProtocol::L2TP,
                        "PPTP" => VpnProtocol::PPTP,
                        "SSTP" => VpnProtocol::SSTP,
                        _ => VpnProtocol::WireGuard,
                    };
                }
                "auto_connect" => auto_connect = value.trim() == "true",
                "kill_switch" => kill_switch = value.trim() == "true",
                "split_tunnel" => split_tunnel = value.trim() == "true",
                "auto_reconnect" => auto_reconnect = value.trim() == "true",
                "mtu" => {
                    mtu = value.trim().parse().unwrap_or(1500);
                }
                "dns" => {
                    dns_servers = value.split(',').map(|s| s.trim().to_string()).collect();
                }
                "allowed_ips" => {
                    allowed_ips = value.split(',').map(|s| s.trim().to_string()).collect();
                }
                "notes" => notes = value.trim().to_string(),
                _ => {}
            }
        }
    }

    if name.is_empty() {
        return Err("Profile name is required".into());
    }
    if server.is_empty() {
        return Err("Server address is required".into());
    }

    let actual_port = port.unwrap_or_else(|| protocol.default_port());

    let mut profile = VpnProfile::new(default_id, &name, &server, protocol);
    profile.port = actual_port;
    profile.auto_connect = auto_connect;
    profile.kill_switch = kill_switch;
    profile.split_tunnel = split_tunnel;
    profile.mtu = mtu;
    profile.auto_reconnect = auto_reconnect;
    profile.dns_override = dns_servers;
    profile.allowed_ips = allowed_ips;
    profile.notes = notes;

    Ok(profile)
}

/// Format a tunnel's transfer counter.
///
/// Decimal, not binary: every number this formats is a `bytes_sent` or
/// `bytes_received` counter on a link, the same quantity the tray indicator
/// and the network settings page report. See design-decisions.md §489 --
/// bytes moved over a link are SI, bytes occupying storage are IEC.
fn format_bytes(bytes: u64) -> String {
    guitk::bytes::si(bytes)
}

/// Format a timestamp as HH:MM:SS.
fn format_timestamp(ts: u64) -> String {
    let hours = (ts / 3600) % 24;
    let minutes = (ts / 60) % 60;
    let seconds = ts % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

// ============================================================================
// Sample Data
// ============================================================================

fn sample_profiles() -> Vec<VpnProfile> {
    let mut profiles = Vec::new();

    let mut p1 = VpnProfile::new(1, "Work VPN", "vpn.company.com", VpnProtocol::OpenVPN);
    p1.auth_method = AuthMethod::Certificate {
        cert_path: String::from("/etc/vpn/work.crt"),
        key_path: String::from("/etc/vpn/work.key"),
    };
    p1.kill_switch = true;
    p1.dns_override = vec![String::from("10.0.0.1"), String::from("10.0.0.2")];
    p1.auto_connect = true;
    p1.protocol_settings = ProtocolSettings::OpenVpn {
        config_file: String::from("/etc/vpn/work.ovpn"),
        cipher: String::from("AES-256-GCM"),
        compression: false,
    };
    p1.total_bytes_sent = 1_500_000_000;
    p1.total_bytes_received = 8_200_000_000;
    p1.total_connection_time_secs = 360_000;
    profiles.push(p1);

    let mut p2 = VpnProfile::new(2, "Personal WG", "wg.myvpn.net", VpnProtocol::WireGuard);
    p2.auth_method = AuthMethod::PreSharedKey {
        key: String::from("psk_placeholder_key"),
    };
    p2.split_tunnel = true;
    p2.allowed_ips = vec![String::from("10.0.0.0/8"), String::from("172.16.0.0/12")];
    p2.protocol_settings = ProtocolSettings::WireGuard {
        peer_public_key: String::from("aB3dEfGhIjKlMnOpQrStUvWxYz0123456789+/="),
        endpoint: String::from("wg.myvpn.net:51820"),
        persistent_keepalive: 25,
    };
    p2.total_bytes_sent = 500_000_000;
    p2.total_bytes_received = 2_100_000_000;
    p2.total_connection_time_secs = 180_000;
    profiles.push(p2);

    let mut p3 = VpnProfile::new(3, "Travel VPN", "travel.securevpn.io", VpnProtocol::IPSec);
    p3.auth_method = AuthMethod::UsernamePassword {
        username: String::from("traveler"),
        password: String::new(),
    };
    p3.kill_switch = true;
    p3.dns_override = vec![String::from("1.1.1.1"), String::from("8.8.8.8")];
    p3.notes = String::from("For use on public WiFi networks");
    p3.total_bytes_sent = 250_000_000;
    p3.total_bytes_received = 900_000_000;
    p3.total_connection_time_secs = 72_000;
    profiles.push(p3);

    let mut p4 = VpnProfile::new(4, "Gaming VPN", "game.fastvpn.com", VpnProtocol::WireGuard);
    p4.auth_method = AuthMethod::PreSharedKey {
        key: String::from("gaming_psk_key"),
    };
    p4.mtu = 1400;
    p4.notes = String::from("Low latency server for gaming");
    p4.protocol_settings = ProtocolSettings::WireGuard {
        peer_public_key: String::from("GamePeerKey0123456789abcdef="),
        endpoint: String::from("game.fastvpn.com:51820"),
        persistent_keepalive: 15,
    };
    profiles.push(p4);

    let mut p5 = VpnProfile::new(5, "Legacy Office", "old.office.net", VpnProtocol::L2TP);
    p5.auth_method = AuthMethod::UsernamePassword {
        username: String::from("admin"),
        password: String::new(),
    };
    p5.enabled = false;
    p5.notes = String::from("Deprecated - migrate to WireGuard");
    profiles.push(p5);

    profiles
}

fn sample_log() -> VecDeque<LogEntry> {
    let mut log = VecDeque::new();
    log.push_back(LogEntry {
        timestamp: 1_700_000_000,
        profile_name: String::from("Work VPN"),
        message: String::from("Connected to vpn.company.com"),
        level: LogLevel::Info,
    });
    log.push_back(LogEntry {
        timestamp: 1_700_000_010,
        profile_name: String::from("Work VPN"),
        message: String::from("Assigned IP 10.8.0.2"),
        level: LogLevel::Info,
    });
    log.push_back(LogEntry {
        timestamp: 1_700_000_050,
        profile_name: String::from("Personal WG"),
        message: String::from("Handshake completed with peer"),
        level: LogLevel::Info,
    });
    log.push_back(LogEntry {
        timestamp: 1_700_000_060,
        profile_name: String::from("Travel VPN"),
        message: String::from("Connection timed out"),
        level: LogLevel::Error,
    });
    log.push_back(LogEntry {
        timestamp: 1_700_000_070,
        profile_name: String::from("System"),
        message: String::from("Kill switch activated - traffic blocked"),
        level: LogLevel::Warning,
    });
    log
}

// ============================================================================
// Rendering
// ============================================================================

/// Render the entire VPN Manager application into a render tree.
/// A text box that can hold the keyboard.
///
/// Without these the window drew a search box that could not be searched with,
/// a split-tunnel tab whose Add button had nothing to add, and an Add-profile
/// dialog whose every field was a picture of a field — `start_add_profile`
/// makes a blank profile, and a blank profile fails `validate`, so the dialog
/// could only ever be cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// The sidebar's profile filter.
    Search,
    /// The split-tunnel tab's new-IP-range box.
    AllowedIp,
    /// Add/edit dialog: profile name.
    Name,
    /// Add/edit dialog: server address.
    Server,
    /// Add/edit dialog: port. Digits only — see [`VpnManager::type_into`].
    Port,
    /// Add/edit dialog: MTU. Digits only.
    Mtu,
}

impl Field {
    /// The field after this one in tab order.
    ///
    /// The dialog's four fields form one cycle and the two loose boxes are
    /// each their own, because Tab out of the search box has nowhere on the
    /// sidebar to go.
    fn next(self) -> Self {
        match self {
            Self::Search => Self::Search,
            Self::AllowedIp => Self::AllowedIp,
            Self::Name => Self::Server,
            Self::Server => Self::Port,
            Self::Port => Self::Mtu,
            Self::Mtu => Self::Name,
        }
    }

    /// Whether this field belongs to the add/edit dialog.
    ///
    /// A dialog field cannot hold the keyboard while the dialog is closed: its
    /// box is not drawn, and a caret in an undrawn box is a keystroke going
    /// somewhere the user cannot see.
    fn is_dialog(self) -> bool {
        matches!(self, Self::Name | Self::Server | Self::Port | Self::Mtu)
    }
}

/// The text to draw in a box that may hold the caret.
///
/// One place, so a focused empty box and a focused full one cannot end up
/// disagreeing about whether the caret is drawn.
fn caret_text(value: &str, focused: bool, placeholder: &str) -> String {
    match (value.is_empty(), focused) {
        (true, false) => placeholder.to_string(),
        (true, true) => String::from("|"),
        (false, true) => format!("{value}|"),
        (false, false) => value.to_string(),
    }
}

/// What a click at a point means.
///
/// Profiles are named by **id**, never by index: `sort_profiles` reorders
/// `profiles` in place whenever the sort order changes or a connection changes
/// state, so an index recorded while drawing can name a different profile by
/// the time the click is acted on. The same goes for `filtered_profiles`, whose
/// indices move as the search box is typed into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    // Title bar
    ToggleGlobalKillSwitch,
    // Toolbar
    AddProfile,
    RemoveProfile,
    ConnectSelected,
    DisconnectSelected,
    QuickConnect,
    Import,
    Export,
    CycleSort,
    // Sidebar
    Profile(u32),
    /// Give a text box the keyboard.
    Focus(Field),
    // Detail panel
    Tab(DetailTab),
    EditProfile,
    ReconnectSelected,
    ToggleEnabled,
    ToggleAutoConnect,
    ToggleAutoReconnect,
    ToggleKillSwitch,
    ToggleSplitTunnel,
    /// OpenVPN's compression flag, on the Protocol tab.
    ToggleCompression,
    /// Drop the allowed-IP range at this position in the selected profile's
    /// list. An index is safe here where it is not for profiles: the list is
    /// re-read from the state the click is acted on, and nothing between the
    /// render and the click reorders it.
    RemoveAllowedIp(usize),
    AddAllowedIp,
    ClearLog,
    // Add/edit dialog
    CycleProtocol,
    CycleAuth,
    DialogToggleKillSwitch,
    DialogToggleAutoConnect,
    DialogToggleSplitTunnel,
    DialogSave,
    DialogCancel,
}

/// A frame being built: the commands to draw, and the clickable rects that
/// drawing them created.
///
/// Rendering and hit-testing are the *same walk* — see [`guitk::frame`] for why,
/// and for how the clips around the profile list and the detail panel trim what
/// is clickable.
pub type Frame = guitk::frame::Frame<Target>;

/// A frame sized for a window of `width` by `height`, never smaller than the
/// minimum the layout is designed for.
fn new_frame(width: f32, height: f32) -> Frame {
    Frame::new(width.max(MIN_WIDTH), height.max(MIN_HEIGHT))
}

/// Draw the whole window at `width` by `height`, collecting as it goes every
/// rect a click could land on.
///
/// This is the only place the window's geometry exists. [`VpnManager::hit_test`]
/// runs it and reads the frame's hits back; it recomputes nothing.
#[must_use]
pub fn render_frame(app: &VpnManager, width: f32, height: f32) -> Frame {
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

    render_title_bar(&mut frame, app);
    render_toolbar(&mut frame, app);

    let content_y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let content_h = frame.height - content_y - STATUS_BAR_HEIGHT;

    render_sidebar(&mut frame, app, content_y, content_h);
    render_detail_panel(&mut frame, app, content_y, content_h);
    render_status_bar(&mut frame, app);

    if app.show_add_dialog {
        // The modal covers the window, so everything recorded under it is
        // unreachable. Dropping those hits is what makes the overlay modal
        // rather than merely dark: a click on a toolbar button behind the
        // dialog would otherwise still fire.
        frame.discard_hits();
        render_add_dialog(&mut frame, app);
    }

    frame
}

/// The render tree for a window at the default size.
#[must_use]
pub fn render_app(app: &VpnManager) -> RenderTree {
    render_frame(app, WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()
}

fn render_title_bar(frame: &mut Frame, app: &VpnManager) {
    // Title bar background
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: frame.width,
        height: TITLE_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Title text
    frame.push(RenderCommand::Text {
        x: 16.0,
        y: 12.0,
        text: String::from("VPN Manager"),
        font_size: 16.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Connection count indicator
    let active = app.active_count();
    let indicator_text = if active > 0 {
        format!("{active} active")
    } else {
        String::from("No connections")
    };
    let indicator_color = if active > 0 { GREEN } else { OVERLAY0 };

    frame.push(RenderCommand::Text {
        x: frame.width - 160.0,
        y: 14.0,
        text: indicator_text,
        font_size: 12.0,
        color: indicator_color,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Global kill switch badge.
    //
    // Drawn whether or not the switch is on. It used to appear only while on,
    // which left the switch with no way to be turned *on* from anywhere in the
    // window — the state existed and `toggle_global_kill_switch` existed, but
    // nothing the user could click reached it.
    let badge = Rect::new(frame.width - 240.0, 10.0, 70.0, 20.0);
    let on = app.global_kill_switch;
    frame.push(RenderCommand::FillRect {
        x: badge.x,
        y: badge.y,
        width: badge.w,
        height: badge.h,
        color: if on {
            RED
        } else {
            Color::rgba(RED.r, RED.g, RED.b, 40)
        },
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::Text {
        x: badge.x + 5.0,
        y: 13.0,
        text: String::from("KILL SW"),
        font_size: 11.0,
        color: if on { MANTLE } else { OVERLAY0 },
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.hit(Target::ToggleGlobalKillSwitch, badge);

    // Separator line
    frame.push(RenderCommand::Line {
        x1: 0.0,
        y1: TITLE_BAR_HEIGHT,
        x2: frame.width,
        y2: TITLE_BAR_HEIGHT,
        color: SURFACE0,
        width: 1.0,
    });
}

fn render_toolbar(frame: &mut Frame, app: &VpnManager) {
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

    let btn_y = y + 4.0;

    // One table, walked once, so the ink and the click target cannot disagree.
    // These used to be seven calls with seven hand-computed x positions and a
    // width derived from the label length — which meant "Import" ended at
    // exactly 550, where "Export" began, and any relabelling would have
    // silently overlapped them.
    let mut x = TOOLBAR_PADDING;
    for &(label, color, target) in TOOLBAR_BUTTONS {
        let width = toolbar_button_width(label);
        render_toolbar_button(frame, label, x, btn_y, color);
        frame.hit(target, Rect::new(x, btn_y, width, TOOLBAR_BUTTON_HEIGHT));
        x += width + TOOLBAR_GAP;
    }

    // Sort control. Clicking it cycles the order, which is the whole of the
    // "dropdown" this used to only look like.
    let sort_text = format!("Sort: {}", app.sort_order.label());
    let sort_rect = Rect::new(frame.width - 180.0, y + 6.0, 172.0, 24.0);
    frame.push(RenderCommand::Text {
        x: sort_rect.x,
        y: y + 10.0,
        text: sort_text,
        font_size: 12.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(sort_rect.w),
        overflow: TextOverflow::Ellipsis,
    });
    frame.hit(Target::CycleSort, sort_rect);

    // Separator
    frame.push(RenderCommand::Line {
        x1: 0.0,
        y1: y + TOOLBAR_HEIGHT,
        x2: frame.width,
        y2: y + TOOLBAR_HEIGHT,
        color: SURFACE1,
        width: 1.0,
    });
}

/// The toolbar, left to right: label, colour, and what pressing it means.
const TOOLBAR_BUTTONS: &[(&str, Color, Target)] = &[
    ("Add", GREEN, Target::AddProfile),
    ("Remove", RED, Target::RemoveProfile),
    ("Connect", BLUE, Target::ConnectSelected),
    ("Disconnect", PEACH, Target::DisconnectSelected),
    ("Quick Connect", LAVENDER, Target::QuickConnect),
    ("Import", SUBTEXT0, Target::Import),
    ("Export", SUBTEXT0, Target::Export),
];

const TOOLBAR_PADDING: f32 = 8.0;
const TOOLBAR_GAP: f32 = 8.0;
const TOOLBAR_BUTTON_HEIGHT: f32 = 28.0;

/// How wide a toolbar button is drawn.
///
/// An estimate from the label length rather than a measurement, because the
/// renderer has no font metrics here — but it is the *same* estimate the ink
/// and the hit box both use, which is the property that matters.
fn toolbar_button_width(label: &str) -> f32 {
    (label.len() as f32) * 8.0 + 16.0
}

fn render_toolbar_button(frame: &mut Frame, label: &str, x: f32, y: f32, color: Color) {
    let width = toolbar_button_width(label);
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: TOOLBAR_BUTTON_HEIGHT,
        color: Color::rgba(color.r, color.g, color.b, 40),
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::StrokeRect {
        x,
        y,
        width,
        height: TOOLBAR_BUTTON_HEIGHT,
        color: Color::rgba(color.r, color.g, color.b, 80),
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::Text {
        x: x + 8.0,
        y: y + 7.0,
        text: label.to_string(),
        font_size: 12.0,
        color,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width - 16.0),
        overflow: TextOverflow::Ellipsis,
    });
}

fn render_sidebar(frame: &mut Frame, app: &VpnManager, content_y: f32, content_h: f32) {
    // Sidebar background
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: content_y,
        width: SIDEBAR_WIDTH,
        height: content_h,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Search box
    let search_y = content_y + 8.0;
    let search_rect = Rect::new(8.0, search_y, SIDEBAR_WIDTH - 16.0, 28.0);
    let focused = app.focus == Some(Field::Search);
    frame.push(RenderCommand::FillRect {
        x: search_rect.x,
        y: search_rect.y,
        width: search_rect.w,
        height: search_rect.h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(4.0),
    });
    if focused {
        frame.push(RenderCommand::StrokeRect {
            x: search_rect.x,
            y: search_rect.y,
            width: search_rect.w,
            height: search_rect.h,
            color: BLUE,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });
    }
    let empty = app.search_query.is_empty();
    frame.push(RenderCommand::Text {
        x: 16.0,
        y: search_y + 7.0,
        // A focused empty box shows a caret rather than the placeholder, so
        // there is somewhere visible for the next keystroke to appear.
        text: caret_text(&app.search_query, focused, "Search profiles..."),
        font_size: 12.0,
        color: if empty && !focused {
            OVERLAY0
        } else {
            TEXT_COLOR
        },
        font_weight: FontWeightHint::Regular,
        max_width: Some(SIDEBAR_WIDTH - 32.0),
        overflow: TextOverflow::Ellipsis,
    });
    frame.hit(Target::Focus(Field::Search), search_rect);

    // Profile list
    let list_y = search_y + 36.0;
    let filtered = app.filtered_profiles();

    frame.push(RenderCommand::PushClip {
        x: 0.0,
        y: list_y,
        width: SIDEBAR_WIDTH,
        height: content_h - 44.0,
    });

    for (vis_idx, &prof_idx) in filtered.iter().enumerate() {
        let item_y = list_y + (vis_idx as f32) * SIDEBAR_ITEM_HEIGHT - app.scroll_offset;

        if item_y + SIDEBAR_ITEM_HEIGHT < list_y || item_y > content_y + content_h {
            continue;
        }

        if let Some(profile) = app.profiles.get(prof_idx) {
            let is_selected = app.selected_profile == Some(prof_idx);
            let conn = app.connection_for(profile.id);
            render_sidebar_item(frame, profile, conn, item_y, is_selected);
            // By id, not by index: `sort_profiles` runs whenever the sort order
            // changes or a connection changes state, and it reorders `profiles`
            // under whatever the pointer was over.
            frame.hit(
                Target::Profile(profile.id),
                Rect::new(0.0, item_y, SIDEBAR_WIDTH, SIDEBAR_ITEM_HEIGHT - 4.0),
            );
        }
    }

    frame.push(RenderCommand::PopClip);

    // Sidebar separator
    frame.push(RenderCommand::Line {
        x1: SIDEBAR_WIDTH,
        y1: content_y,
        x2: SIDEBAR_WIDTH,
        y2: content_y + content_h,
        color: SURFACE0,
        width: 1.0,
    });
}

fn render_sidebar_item(
    frame: &mut Frame,
    profile: &VpnProfile,
    connection: Option<&VpnConnection>,
    y: f32,
    selected: bool,
) {
    // Selection highlight
    if selected {
        frame.push(RenderCommand::FillRect {
            x: 4.0,
            y,
            width: SIDEBAR_WIDTH - 8.0,
            height: SIDEBAR_ITEM_HEIGHT - 4.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    // Status indicator dot
    let status_color = connection.map(|c| c.status.color()).unwrap_or(OVERLAY0);
    frame.push(RenderCommand::FillRect {
        x: 16.0,
        y: y + 12.0,
        width: 10.0,
        height: 10.0,
        color: status_color,
        corner_radii: CornerRadii::all(5.0),
    });

    // Profile name
    let name_color = if profile.enabled {
        TEXT_COLOR
    } else {
        OVERLAY0
    };
    frame.push(RenderCommand::Text {
        x: 34.0,
        y: y + 8.0,
        text: profile.name.clone(),
        font_size: 13.0,
        color: name_color,
        font_weight: FontWeightHint::Bold,
        max_width: Some(SIDEBAR_WIDTH - 50.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Server and protocol
    frame.push(RenderCommand::Text {
        x: 34.0,
        y: y + 26.0,
        text: format!("{} - {}", profile.server_address, profile.protocol.label()),
        font_size: 11.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(SIDEBAR_WIDTH - 50.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Status text
    let status_text = connection
        .map(|c| c.status.label().to_string())
        .unwrap_or_else(|| String::from("Disconnected"));
    frame.push(RenderCommand::Text {
        x: 34.0,
        y: y + 40.0,
        text: status_text,
        font_size: 10.0,
        color: status_color,
        font_weight: FontWeightHint::Light,
        max_width: Some(SIDEBAR_WIDTH - 50.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Kill switch badge
    if profile.kill_switch {
        frame.push(RenderCommand::FillRect {
            x: SIDEBAR_WIDTH - 50.0,
            y: y + 8.0,
            width: 32.0,
            height: 16.0,
            color: Color::rgba(RED.r, RED.g, RED.b, 60),
            corner_radii: CornerRadii::all(3.0),
        });
        frame.push(RenderCommand::Text {
            x: SIDEBAR_WIDTH - 48.0,
            y: y + 10.0,
            text: String::from("KS"),
            font_size: 10.0,
            color: RED,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

fn render_detail_panel(frame: &mut Frame, app: &VpnManager, content_y: f32, content_h: f32) {
    let px = SIDEBAR_WIDTH + 1.0;
    let pw = frame.width - SIDEBAR_WIDTH - 1.0;

    // Background
    frame.push(RenderCommand::FillRect {
        x: px,
        y: content_y,
        width: pw,
        height: content_h,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    if app.selected_profile.is_none() || app.profiles.is_empty() {
        render_no_selection(frame, px, content_y, pw, content_h);
        return;
    }

    // Tab bar
    render_tab_bar(frame, app, px, content_y, pw);

    let panel_y = content_y + TAB_HEIGHT + 8.0;
    let panel_h = content_h - TAB_HEIGHT - 8.0;

    frame.push(RenderCommand::PushClip {
        x: px,
        y: panel_y,
        width: pw,
        height: panel_h,
    });

    match app.current_tab {
        DetailTab::Overview => render_tab_overview(frame, app, px, panel_y, pw),
        DetailTab::Connection => render_tab_connection(frame, app, px, panel_y, pw),
        DetailTab::SplitTunnel => render_tab_split_tunnel(frame, app, px, panel_y, pw),
        DetailTab::ProtocolConfig => render_tab_protocol(frame, app, px, panel_y, pw),
        DetailTab::Log => render_tab_log(frame, app, px, panel_y, pw, panel_h),
        DetailTab::Stats => render_tab_stats(frame, app, px, panel_y, pw),
    }

    frame.push(RenderCommand::PopClip);
}

fn render_tab_bar(frame: &mut Frame, app: &VpnManager, px: f32, py: f32, pw: f32) {
    // Tab bar background
    frame.push(RenderCommand::FillRect {
        x: px,
        y: py,
        width: pw,
        height: TAB_HEIGHT,
        color: SURFACE0,
        corner_radii: CornerRadii::ZERO,
    });

    let tabs = DetailTab::all();
    let tab_w = pw / tabs.len() as f32;

    for (i, tab) in tabs.iter().enumerate() {
        let tx = px + i as f32 * tab_w;
        let active = app.current_tab == *tab;

        if active {
            frame.push(RenderCommand::FillRect {
                x: tx,
                y: py,
                width: tab_w,
                height: TAB_HEIGHT,
                color: BASE,
                corner_radii: CornerRadii::ZERO,
            });
            // Active indicator line
            frame.push(RenderCommand::FillRect {
                x: tx,
                y: py + TAB_HEIGHT - 2.0,
                width: tab_w,
                height: 2.0,
                color: BLUE,
                corner_radii: CornerRadii::ZERO,
            });
        }

        frame.push(RenderCommand::Text {
            x: tx + 8.0,
            y: py + 9.0,
            text: tab.label().to_string(),
            font_size: 12.0,
            color: if active { TEXT_COLOR } else { SUBTEXT0 },
            font_weight: if active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(tab_w - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        frame.hit(Target::Tab(*tab), Rect::new(tx, py, tab_w, TAB_HEIGHT));
    }
}

fn render_no_selection(frame: &mut Frame, px: f32, py: f32, pw: f32, ph: f32) {
    frame.push(RenderCommand::Text {
        x: px + pw / 2.0 - 80.0,
        y: py + ph / 2.0 - 10.0,
        text: String::from("Select a VPN profile"),
        font_size: 16.0,
        color: OVERLAY0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

fn render_tab_overview(frame: &mut Frame, app: &VpnManager, px: f32, py: f32, pw: f32) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let conn = app.connection_for(profile.id);
    let mut y = py + SECTION_PADDING;

    // Section: Profile Info
    y = render_section_title(frame, "Profile Information", px + SECTION_PADDING, y);

    y = render_field_row(frame, "Name:", &profile.name, px + SECTION_PADDING, y, pw);
    y = render_field_row(
        frame,
        "Server:",
        &profile.server_address,
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        frame,
        "Port:",
        &profile.port.to_string(),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        frame,
        "Protocol:",
        profile.protocol.label(),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        frame,
        "Auth:",
        profile.auth_method.label(),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        frame,
        "MTU:",
        &profile.mtu.to_string(),
        px + SECTION_PADDING,
        y,
        pw,
    );

    y += 8.0;

    // Toggles
    let toggles: [(&str, bool, Target); 5] = [
        ("Enabled", profile.enabled, Target::ToggleEnabled),
        (
            "Auto Connect",
            profile.auto_connect,
            Target::ToggleAutoConnect,
        ),
        (
            "Auto Reconnect",
            profile.auto_reconnect,
            Target::ToggleAutoReconnect,
        ),
        ("Kill Switch", profile.kill_switch, Target::ToggleKillSwitch),
        (
            "Split Tunnel",
            profile.split_tunnel,
            Target::ToggleSplitTunnel,
        ),
    ];
    for (label, on, target) in toggles {
        y = render_toggle_row(frame, label, on, px + SECTION_PADDING, y, target);
    }

    y += 12.0;

    // Edit button, so the dialog that `start_edit_profile` opens has a way to
    // be opened. Placed under the toggles rather than in the toolbar, which is
    // already full.
    render_action_button(
        frame,
        "Edit...",
        px + SECTION_PADDING,
        y,
        BLUE,
        Target::EditProfile,
    );
    y += BUTTON_HEIGHT + 12.0;

    // DNS overrides
    if !profile.dns_override.is_empty() {
        y = render_section_title(frame, "DNS Override", px + SECTION_PADDING, y);
        for dns in &profile.dns_override {
            y = render_field_row(frame, "", dns, px + SECTION_PADDING + 8.0, y, pw);
        }
        y += 4.0;
    }

    // Notes
    if !profile.notes.is_empty() {
        y = render_section_title(frame, "Notes", px + SECTION_PADDING, y);
        // `RenderCommand::Text` clips at `max_width` rather than wrapping, so
        // notes longer than the panel is wide used to be shown as their first
        // line and no more. Nothing is drawn under them and the whole tab is
        // clipped to the panel, so they can simply run as long as they are.
        text::Paragraph::new(&profile.notes, SUBTEXT0)
            .at(
                px + SECTION_PADDING + 8.0,
                y,
                pw - SECTION_PADDING * 2.0 - 16.0,
            )
            .font(NOTES_FONT_SIZE, FontWeightHint::Regular)
            .line_height(NOTES_LINE_HEIGHT)
            .draw(frame);
    }

    // Connection summary
    if let Some(c) = conn
        && c.status == ConnectionStatus::Connected
    {
        let summary_y = py + SECTION_PADDING;
        let summary_x = px + pw - 180.0;

        frame.push(RenderCommand::FillRect {
            x: summary_x - 8.0,
            y: summary_y - 4.0,
            width: 170.0,
            height: 80.0,
            color: Color::rgba(GREEN.r, GREEN.g, GREEN.b, 20),
            corner_radii: CornerRadii::all(8.0),
        });

        frame.push(RenderCommand::Text {
            x: summary_x,
            y: summary_y,
            text: String::from("Connected"),
            font_size: 14.0,
            color: GREEN,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.push(RenderCommand::Text {
            x: summary_x,
            y: summary_y + 20.0,
            text: format!("IP: {}", c.local_ip),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.push(RenderCommand::Text {
            x: summary_x,
            y: summary_y + 36.0,
            text: format!("Latency: {}ms", c.latency_ms),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        frame.push(RenderCommand::Text {
            x: summary_x,
            y: summary_y + 52.0,
            text: format!("Uptime: {}", c.format_uptime()),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

fn render_tab_connection(frame: &mut Frame, app: &VpnManager, px: f32, py: f32, pw: f32) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let conn = app.connection_for(profile.id);
    let mut y = py + SECTION_PADDING;

    y = render_section_title(frame, "Connection Details", px + SECTION_PADDING, y);

    if let Some(c) = conn {
        // Status with colored indicator
        let status_label = c.status.label();
        let status_color = c.status.color();

        frame.push(RenderCommand::FillRect {
            x: px + SECTION_PADDING,
            y: y + 2.0,
            width: 8.0,
            height: 8.0,
            color: status_color,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 14.0,
            y,
            text: format!("Status: {status_label}"),
            font_size: 13.0,
            color: status_color,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        y += FIELD_HEIGHT;

        if c.status == ConnectionStatus::Connected {
            y = render_field_row(frame, "Local IP:", &c.local_ip, px + SECTION_PADDING, y, pw);
            y = render_field_row(
                frame,
                "Remote IP:",
                &c.remote_ip,
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(
                frame,
                "Latency:",
                &format!("{}ms", c.latency_ms),
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(
                frame,
                "Uptime:",
                &c.format_uptime(),
                px + SECTION_PADDING,
                y,
                pw,
            );

            y += 8.0;
            y = render_section_title(frame, "Data Transfer", px + SECTION_PADDING, y);
            y = render_field_row(
                frame,
                "Sent:",
                &format_bytes(c.bytes_sent),
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(
                frame,
                "Received:",
                &format_bytes(c.bytes_received),
                px + SECTION_PADDING,
                y,
                pw,
            );
        }

        y += 16.0;

        // Action buttons
        let btn_x = px + SECTION_PADDING;
        if c.status == ConnectionStatus::Connected || c.status.is_active() {
            render_action_button(
                frame,
                "Disconnect",
                btn_x,
                y,
                PEACH,
                Target::DisconnectSelected,
            );
            render_action_button(
                frame,
                "Reconnect",
                btn_x + BUTTON_WIDTH + 8.0,
                y,
                YELLOW,
                Target::ReconnectSelected,
            );
        } else {
            render_action_button(frame, "Connect", btn_x, y, GREEN, Target::ConnectSelected);
        }
    } else {
        frame.push(RenderCommand::Text {
            x: px + SECTION_PADDING,
            y,
            text: String::from("No connection data available"),
            font_size: 13.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    let _ = y; // suppress unused
}

fn render_tab_split_tunnel(frame: &mut Frame, app: &VpnManager, px: f32, py: f32, pw: f32) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let mut y = py + SECTION_PADDING;

    y = render_section_title(frame, "Split Tunneling", px + SECTION_PADDING, y);

    // Toggle
    y = render_toggle_row(
        frame,
        "Enable Split Tunneling",
        profile.split_tunnel,
        px + SECTION_PADDING,
        y,
        Target::ToggleSplitTunnel,
    );
    y += 8.0;

    // Explanation text
    frame.push(RenderCommand::Text {
        x: px + SECTION_PADDING,
        y,
        text: String::from("When enabled, only traffic to the allowed IP ranges"),
        font_size: 11.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(pw - SECTION_PADDING * 2.0),
        overflow: TextOverflow::Ellipsis,
    });
    y += 16.0;
    frame.push(RenderCommand::Text {
        x: px + SECTION_PADDING,
        y,
        text: String::from("goes through the VPN tunnel."),
        font_size: 11.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(pw - SECTION_PADDING * 2.0),
        overflow: TextOverflow::Ellipsis,
    });
    y += 24.0;

    // Allowed IPs list
    y = render_section_title(frame, "Allowed IP Ranges", px + SECTION_PADDING, y);

    if profile.allowed_ips.is_empty() {
        frame.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 8.0,
            y,
            text: String::from("No IP ranges configured"),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        y += FIELD_HEIGHT;
    } else {
        for (i, ip) in profile.allowed_ips.iter().enumerate() {
            // Row background
            let row_bg = if i % 2 == 0 { SURFACE0 } else { BASE };
            frame.push(RenderCommand::FillRect {
                x: px + SECTION_PADDING,
                y,
                width: pw - SECTION_PADDING * 2.0,
                height: FIELD_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::all(3.0),
            });
            frame.push(RenderCommand::Text {
                x: px + SECTION_PADDING + 8.0,
                y: y + 6.0,
                text: ip.clone(),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(pw - SECTION_PADDING * 2.0 - 80.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Remove button
            let remove = Rect::new(px + pw - SECTION_PADDING - 54.0, y, 54.0, FIELD_HEIGHT);
            frame.push(RenderCommand::Text {
                x: remove.x + 4.0,
                y: y + 6.0,
                text: String::from("Remove"),
                font_size: 11.0,
                color: RED,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            frame.hit(Target::RemoveAllowedIp(i), remove);

            y += FIELD_HEIGHT + 2.0;
        }
    }

    y += 8.0;

    // New-range box, then the button that commits it. The button used to stand
    // alone with nothing to add: `add_allowed_ip` takes a string and there was
    // nowhere in the window to type one.
    let input = Rect::new(
        px + SECTION_PADDING,
        y,
        (pw - SECTION_PADDING * 2.0 - BUTTON_WIDTH - 8.0).max(80.0),
        FIELD_HEIGHT,
    );
    let focused = app.focus == Some(Field::AllowedIp);
    frame.push(RenderCommand::FillRect {
        x: input.x,
        y: input.y,
        width: input.w,
        height: input.h,
        color: MANTLE,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::StrokeRect {
        x: input.x,
        y: input.y,
        width: input.w,
        height: input.h,
        color: if focused { BLUE } else { SURFACE1 },
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::Text {
        x: input.x + 8.0,
        y: input.y + 6.0,
        text: caret_text(&app.allowed_ip_input, focused, "10.0.0.0/8"),
        font_size: 12.0,
        color: if app.allowed_ip_input.is_empty() && !focused {
            OVERLAY0
        } else {
            TEXT_COLOR
        },
        font_weight: FontWeightHint::Regular,
        max_width: Some(input.w - 16.0),
        overflow: TextOverflow::Ellipsis,
    });
    frame.hit(Target::Focus(Field::AllowedIp), input);

    render_action_button(
        frame,
        "Add Range",
        input.right() + 8.0,
        y - 2.0,
        BLUE,
        Target::AddAllowedIp,
    );

    let _ = y; // suppress unused
}

fn render_tab_protocol(frame: &mut Frame, app: &VpnManager, px: f32, py: f32, pw: f32) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let mut y = py + SECTION_PADDING;

    y = render_section_title(
        frame,
        &format!("{} Settings", profile.protocol.label()),
        px + SECTION_PADDING,
        y,
    );

    match &profile.protocol_settings {
        ProtocolSettings::OpenVpn {
            config_file,
            cipher,
            compression,
        } => {
            y = render_field_row(
                frame,
                "Config File:",
                config_file,
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(frame, "Cipher:", cipher, px + SECTION_PADDING, y, pw);
            y = render_toggle_row(
                frame,
                "Compression",
                *compression,
                px + SECTION_PADDING,
                y,
                Target::ToggleCompression,
            );
        }
        ProtocolSettings::WireGuard {
            peer_public_key,
            endpoint,
            persistent_keepalive,
        } => {
            y = render_field_row(
                frame,
                "Peer Key:",
                peer_public_key,
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(frame, "Endpoint:", endpoint, px + SECTION_PADDING, y, pw);
            y = render_field_row(
                frame,
                "Keepalive:",
                &format!("{persistent_keepalive}s"),
                px + SECTION_PADDING,
                y,
                pw,
            );
        }
        ProtocolSettings::IPSec {
            ike_version,
            phase1_algo,
            phase2_algo,
        } => {
            y = render_field_row(
                frame,
                "IKE Version:",
                &format!("v{ike_version}"),
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(frame, "Phase 1:", phase1_algo, px + SECTION_PADDING, y, pw);
            y = render_field_row(frame, "Phase 2:", phase2_algo, px + SECTION_PADDING, y, pw);
        }
        ProtocolSettings::Generic => {
            frame.push(RenderCommand::Text {
                x: px + SECTION_PADDING,
                y,
                text: String::from("No protocol-specific settings for this protocol."),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(pw - SECTION_PADDING * 2.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += FIELD_HEIGHT;
        }
    }

    let _ = y; // suppress unused
}

fn render_tab_log(frame: &mut Frame, app: &VpnManager, px: f32, py: f32, pw: f32, ph: f32) {
    let mut y = py + SECTION_PADDING;

    y = render_section_title(frame, "Connection Log", px + SECTION_PADDING, y);

    // Clear log button
    render_action_button(
        frame,
        "Clear Log",
        px + pw - SECTION_PADDING - BUTTON_WIDTH,
        y - 24.0,
        RED,
        Target::ClearLog,
    );

    if app.log.is_empty() {
        frame.push(RenderCommand::Text {
            x: px + SECTION_PADDING,
            y,
            text: String::from("No log entries"),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        return;
    }

    // Log header
    frame.push(RenderCommand::FillRect {
        x: px + SECTION_PADDING,
        y,
        width: pw - SECTION_PADDING * 2.0,
        height: LOG_ENTRY_HEIGHT,
        color: SURFACE1,
        corner_radii: CornerRadii::all(3.0),
    });
    frame.push(RenderCommand::Text {
        x: px + SECTION_PADDING + 8.0,
        y: y + 4.0,
        text: String::from("Time"),
        font_size: 11.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.push(RenderCommand::Text {
        x: px + SECTION_PADDING + 80.0,
        y: y + 4.0,
        text: String::from("Level"),
        font_size: 11.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.push(RenderCommand::Text {
        x: px + SECTION_PADDING + 130.0,
        y: y + 4.0,
        text: String::from("Profile"),
        font_size: 11.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    frame.push(RenderCommand::Text {
        x: px + SECTION_PADDING + 250.0,
        y: y + 4.0,
        text: String::from("Message"),
        font_size: 11.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    y += LOG_ENTRY_HEIGHT + 2.0;

    // Log entries (reverse chronological). The old loop stopped at a
    // hard-coded `py + 500.0`, a number with no relation to the panel it was
    // drawing into, and read no offset at all -- so on a short panel it
    // overran, on a tall one it wasted the space, and either way every entry
    // past the cut was unreachable rather than merely off-screen.
    //
    // It also found each row's stripe parity by scanning the whole log for the
    // row's own address, which is O(n) per row over a log of up to 500. The
    // index is what `enumerate` already hands us.
    let entries: Vec<&LogEntry> = app.log.iter().rev().collect();
    let rows_top = y;
    let window = scroll_window::visible(
        entries.len(),
        LOG_ENTRY_HEIGHT,
        (py + ph) - rows_top - LOG_MORE_HEIGHT,
        app.log_scroll_offset,
    );
    for (drawn, entry) in entries
        .get(window.start..window.end())
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        // Stripe by absolute position, not by position on screen, so the
        // banding does not invert as the log scrolls.
        let i = window.start.saturating_add(drawn);
        let row_y = rows_top + (drawn as f32) * LOG_ENTRY_HEIGHT;
        let row_bg = if i % 2 == 0 {
            Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, 80)
        } else {
            Color::TRANSPARENT
        };

        frame.push(RenderCommand::FillRect {
            x: px + SECTION_PADDING,
            y: row_y,
            width: pw - SECTION_PADDING * 2.0,
            height: LOG_ENTRY_HEIGHT,
            color: row_bg,
            corner_radii: CornerRadii::ZERO,
        });

        frame.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 8.0,
            y: row_y + 4.0,
            text: format_timestamp(entry.timestamp),
            font_size: 10.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        frame.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 80.0,
            y: row_y + 4.0,
            text: entry.level.label().to_string(),
            font_size: 10.0,
            color: entry.level.color(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        frame.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 130.0,
            y: row_y + 4.0,
            text: entry.profile_name.clone(),
            font_size: 10.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(110.0),
            overflow: TextOverflow::Ellipsis,
        });

        frame.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 250.0,
            y: row_y + 4.0,
            text: entry.message.clone(),
            font_size: 10.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(pw - SECTION_PADDING * 2.0 - 260.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    // A log hiding entries says how many.
    let hidden = entries.len().saturating_sub(window.count);
    if hidden > 0 {
        frame.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 8.0,
            y: rows_top + (window.count as f32) * LOG_ENTRY_HEIGHT,
            text: format!("{hidden} more"),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

fn render_tab_stats(frame: &mut Frame, app: &VpnManager, px: f32, py: f32, pw: f32) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let mut y = py + SECTION_PADDING;

    y = render_section_title(frame, "Usage Statistics", px + SECTION_PADDING, y);

    // Cumulative stats
    y = render_field_row(
        frame,
        "Total Sent:",
        &format_bytes(profile.total_bytes_sent),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        frame,
        "Total Received:",
        &format_bytes(profile.total_bytes_received),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        frame,
        "Total Time:",
        &format_duration_long(profile.total_connection_time_secs),
        px + SECTION_PADDING,
        y,
        pw,
    );

    y += 16.0;

    // Current session stats
    if let Some(conn) = app.connection_for(profile.id)
        && conn.status == ConnectionStatus::Connected
    {
        y = render_section_title(frame, "Current Session", px + SECTION_PADDING, y);
        y = render_field_row(
            frame,
            "Session Sent:",
            &format_bytes(conn.bytes_sent),
            px + SECTION_PADDING,
            y,
            pw,
        );
        y = render_field_row(
            frame,
            "Session Recv:",
            &format_bytes(conn.bytes_received),
            px + SECTION_PADDING,
            y,
            pw,
        );
        y = render_field_row(
            frame,
            "Uptime:",
            &conn.format_uptime(),
            px + SECTION_PADDING,
            y,
            pw,
        );
        y = render_field_row(
            frame,
            "Latency:",
            &format!("{}ms", conn.latency_ms),
            px + SECTION_PADDING,
            y,
            pw,
        );
    }

    y += 16.0;

    // Data usage bar chart
    y = render_section_title(frame, "Data Usage Comparison", px + SECTION_PADDING, y);

    let chart_x = px + SECTION_PADDING;
    let chart_w = pw - SECTION_PADDING * 2.0;
    let bar_h = 24.0;

    // Find max for scaling
    let max_bytes = app
        .profiles
        .iter()
        .map(|p| p.total_bytes_sent.saturating_add(p.total_bytes_received))
        .max()
        .unwrap_or(1)
        .max(1);

    for profile in &app.profiles {
        let total = profile
            .total_bytes_sent
            .saturating_add(profile.total_bytes_received);
        let ratio = total as f32 / max_bytes as f32;
        let bar_w = (chart_w - 140.0) * ratio;

        // Label
        frame.push(RenderCommand::Text {
            x: chart_x,
            y: y + 5.0,
            text: profile.name.clone(),
            font_size: 11.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Bar
        frame.push(RenderCommand::FillRect {
            x: chart_x + 130.0,
            y: y + 2.0,
            width: bar_w.max(2.0),
            height: bar_h - 4.0,
            color: BLUE,
            corner_radii: CornerRadii::all(3.0),
        });

        // Value
        frame.push(RenderCommand::Text {
            x: chart_x + 134.0 + bar_w,
            y: y + 5.0,
            text: format_bytes(total),
            font_size: 10.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        y += bar_h + 4.0;
    }

    let _ = y; // suppress unused
}

fn render_add_dialog(frame: &mut Frame, app: &VpnManager) {
    // Modal overlay
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: frame.width,
        height: frame.height,
        color: Color::rgba(0, 0, 0, 160),
        corner_radii: CornerRadii::ZERO,
    });

    let dialog_w = 480.0;
    let dialog_h = 420.0;
    let dx = (frame.width - dialog_w) / 2.0;
    let dy = (frame.height - dialog_h) / 2.0;

    // Dialog background
    frame.push(RenderCommand::FillRect {
        x: dx,
        y: dy,
        width: dialog_w,
        height: dialog_h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(12.0),
    });
    frame.push(RenderCommand::StrokeRect {
        x: dx,
        y: dy,
        width: dialog_w,
        height: dialog_h,
        color: SURFACE1,
        line_width: 1.0,
        corner_radii: CornerRadii::all(12.0),
    });

    // Title
    frame.push(RenderCommand::Text {
        x: dx + 20.0,
        y: dy + 16.0,
        text: if app.editing_profile.as_ref().is_some_and(|p| p.id != 0) {
            String::from("Edit VPN Profile")
        } else {
            String::from("Add VPN Profile")
        },
        font_size: 16.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Form fields
    if let Some(profile) = &app.editing_profile {
        let mut y = dy + 50.0;
        let fw = dialog_w - 40.0;

        // Four typed fields, then two that cycle through a fixed set. Protocol
        // and auth method are enums with no free-text form, so a box to type
        // into would only be a box to type a wrong answer into.
        let typed: [(&str, String, Field); 4] = [
            ("Name:", profile.name.clone(), Field::Name),
            ("Server:", profile.server_address.clone(), Field::Server),
            ("Port:", profile.port.to_string(), Field::Port),
            ("MTU:", profile.mtu.to_string(), Field::Mtu),
        ];
        for (label, value, field) in typed {
            y = render_dialog_field(
                frame,
                label,
                &value,
                dx + 20.0,
                y,
                fw,
                app.focus == Some(field),
                Target::Focus(field),
            );
        }

        y = render_dialog_field(
            frame,
            "Protocol:",
            profile.protocol.label(),
            dx + 20.0,
            y,
            fw,
            false,
            Target::CycleProtocol,
        );
        y = render_dialog_field(
            frame,
            "Auth:",
            profile.auth_method.label(),
            dx + 20.0,
            y,
            fw,
            false,
            Target::CycleAuth,
        );

        y += 8.0;
        let toggles: [(&str, bool, Target); 3] = [
            (
                "Kill Switch",
                profile.kill_switch,
                Target::DialogToggleKillSwitch,
            ),
            (
                "Auto Connect",
                profile.auto_connect,
                Target::DialogToggleAutoConnect,
            ),
            (
                "Split Tunnel",
                profile.split_tunnel,
                Target::DialogToggleSplitTunnel,
            ),
        ];
        for (label, on, target) in toggles {
            y = render_toggle_row(frame, label, on, dx + 20.0, y, target);
        }

        let _ = y; // suppress unused
    }

    // Why the last Save was refused, under the fields it is about. The status
    // bar cannot serve here: it is behind the scrim this dialog just drew over
    // the window, at 63% black.
    if !app.dialog_error.is_empty() {
        frame.push(RenderCommand::Text {
            x: dx + 20.0,
            y: dy + dialog_h - 74.0,
            text: app.dialog_error.clone(),
            font_size: 12.0,
            color: RED,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 40.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    // Buttons
    let btn_y = dy + dialog_h - 50.0;
    render_action_button(
        frame,
        "Cancel",
        dx + dialog_w - 240.0,
        btn_y,
        RED,
        Target::DialogCancel,
    );
    render_action_button(
        frame,
        "Save",
        dx + dialog_w - 130.0,
        btn_y,
        GREEN,
        Target::DialogSave,
    );
}

fn render_status_bar(frame: &mut Frame, app: &VpnManager) {
    let y = frame.height - STATUS_BAR_HEIGHT;

    // Background
    frame.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width: frame.width,
        height: STATUS_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Separator
    frame.push(RenderCommand::Line {
        x1: 0.0,
        y1: y,
        x2: frame.width,
        y2: y,
        color: SURFACE0,
        width: 1.0,
    });

    // Profile count
    frame.push(RenderCommand::Text {
        x: 12.0,
        y: y + 8.0,
        text: format!("{} profiles", app.profiles.len()),
        font_size: 11.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Active connections
    let active = app.active_count();
    frame.push(RenderCommand::Text {
        x: 120.0,
        y: y + 8.0,
        text: format!("{active} connected"),
        font_size: 11.0,
        color: if active > 0 { GREEN } else { OVERLAY0 },
        font_weight: FontWeightHint::Regular,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Total transfer
    let (sent, recv) = app.total_transfer();
    if sent > 0 || recv > 0 {
        frame.push(RenderCommand::Text {
            x: 260.0,
            y: y + 8.0,
            text: format!("TX: {}  RX: {}", format_bytes(sent), format_bytes(recv)),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // Kill switch status
    if app.global_kill_switch {
        frame.push(RenderCommand::Text {
            x: frame.width - 120.0,
            y: y + 8.0,
            text: String::from("Kill Switch: ON"),
            font_size: 11.0,
            color: RED,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // What the last action did, or why it did not. Every state method that can
    // fail returns `Err(String)`; before this line those strings had no reader.
    if !app.status_message.is_empty() {
        let left = 460.0;
        let right = if app.global_kill_switch {
            frame.width - 130.0
        } else {
            frame.width - 12.0
        };
        frame.push(RenderCommand::Text {
            x: left,
            y: y + 8.0,
            text: app.status_message.clone(),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((right - left).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

// ============================================================================
// Render Helpers
// ============================================================================

fn render_section_title(frame: &mut Frame, title: &str, x: f32, y: f32) -> f32 {
    frame.push(RenderCommand::Text {
        x,
        y,
        text: title.to_string(),
        font_size: 14.0,
        color: LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    // Underline
    frame.push(RenderCommand::Line {
        x1: x,
        y1: y + 18.0,
        x2: x + 200.0,
        y2: y + 18.0,
        color: Color::rgba(LAVENDER.r, LAVENDER.g, LAVENDER.b, 60),
        width: 1.0,
    });
    y + 26.0
}

fn render_field_row(frame: &mut Frame, label: &str, value: &str, x: f32, y: f32, _pw: f32) -> f32 {
    if !label.is_empty() {
        frame.push(RenderCommand::Text {
            x,
            y: y + 4.0,
            text: label.to_string(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(FIELD_LABEL_WIDTH),
            overflow: TextOverflow::Ellipsis,
        });
    }
    frame.push(RenderCommand::Text {
        x: x + FIELD_LABEL_WIDTH,
        y: y + 4.0,
        text: value.to_string(),
        font_size: 12.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Regular,
        max_width: Some(400.0),
        overflow: TextOverflow::Ellipsis,
    });
    y + FIELD_HEIGHT
}

/// Draw a labelled on/off switch, and make it operable.
///
/// The whole row — label and switch — is the click target, because a 36x18
/// switch is a small thing to ask a pointer to find and there is nothing else
/// on the row to hit by accident.
fn render_toggle_row(
    frame: &mut Frame,
    label: &str,
    enabled: bool,
    x: f32,
    y: f32,
    target: Target,
) -> f32 {
    let bottom = render_toggle_row_ink(frame, label, enabled, x, y);
    frame.hit(
        target,
        Rect::new(x, y, FIELD_LABEL_WIDTH + 36.0, FIELD_HEIGHT),
    );
    bottom
}

fn render_toggle_row_ink(frame: &mut Frame, label: &str, enabled: bool, x: f32, y: f32) -> f32 {
    frame.push(RenderCommand::Text {
        x,
        y: y + 4.0,
        text: label.to_string(),
        font_size: 12.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(FIELD_LABEL_WIDTH),
        overflow: TextOverflow::Ellipsis,
    });

    // Toggle track
    let track_x = x + FIELD_LABEL_WIDTH;
    let track_color = if enabled {
        Color::rgba(GREEN.r, GREEN.g, GREEN.b, 120)
    } else {
        SURFACE1
    };
    frame.push(RenderCommand::FillRect {
        x: track_x,
        y: y + 4.0,
        width: 36.0,
        height: 18.0,
        color: track_color,
        corner_radii: CornerRadii::all(9.0),
    });

    // Toggle knob
    let knob_x = if enabled {
        track_x + 20.0
    } else {
        track_x + 2.0
    };
    frame.push(RenderCommand::FillRect {
        x: knob_x,
        y: y + 6.0,
        width: 14.0,
        height: 14.0,
        color: if enabled { GREEN } else { OVERLAY0 },
        corner_radii: CornerRadii::all(7.0),
    });

    y + FIELD_HEIGHT
}

fn render_action_button(
    frame: &mut Frame,
    label: &str,
    x: f32,
    y: f32,
    color: Color,
    target: Target,
) {
    render_action_button_ink(frame, label, x, y, color);
    frame.hit(target, Rect::new(x, y, BUTTON_WIDTH, BUTTON_HEIGHT));
}

fn render_action_button_ink(frame: &mut Frame, label: &str, x: f32, y: f32, color: Color) {
    frame.push(RenderCommand::FillRect {
        x,
        y,
        width: BUTTON_WIDTH,
        height: BUTTON_HEIGHT,
        color: Color::rgba(color.r, color.g, color.b, 40),
        corner_radii: CornerRadii::all(6.0),
    });
    frame.push(RenderCommand::StrokeRect {
        x,
        y,
        width: BUTTON_WIDTH,
        height: BUTTON_HEIGHT,
        color: Color::rgba(color.r, color.g, color.b, 100),
        line_width: 1.0,
        corner_radii: CornerRadii::all(6.0),
    });
    frame.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 9.0,
        text: label.to_string(),
        font_size: 12.0,
        color,
        font_weight: FontWeightHint::Bold,
        max_width: Some(BUTTON_WIDTH - 24.0),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Draw one row of the add/edit dialog: a label and a box.
///
/// `target` is what clicking the box means — [`Target::Focus`] for the boxes
/// that are typed into, a cycle for the two that are chosen from a fixed set.
fn render_dialog_field(
    frame: &mut Frame,
    label: &str,
    value: &str,
    x: f32,
    y: f32,
    fw: f32,
    focused: bool,
    target: Target,
) -> f32 {
    frame.push(RenderCommand::Text {
        x,
        y: y + 4.0,
        text: label.to_string(),
        font_size: 12.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(100.0),
        overflow: TextOverflow::Ellipsis,
    });

    // Input box
    let box_rect = Rect::new(x + 100.0, y, fw - 100.0, FIELD_HEIGHT);
    frame.push(RenderCommand::FillRect {
        x: box_rect.x,
        y: box_rect.y,
        width: box_rect.w,
        height: box_rect.h,
        color: MANTLE,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::StrokeRect {
        x: box_rect.x,
        y: box_rect.y,
        width: box_rect.w,
        height: box_rect.h,
        color: if focused { BLUE } else { SURFACE1 },
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });
    frame.push(RenderCommand::Text {
        x: x + 108.0,
        y: y + 6.0,
        text: caret_text(value, focused, "..."),
        font_size: 12.0,
        color: if value.is_empty() && !focused {
            OVERLAY0
        } else {
            TEXT_COLOR
        },
        font_weight: FontWeightHint::Regular,
        max_width: Some(fw - 120.0),
        overflow: TextOverflow::Ellipsis,
    });
    frame.hit(target, box_rect);

    y + FIELD_HEIGHT + 6.0
}

/// Format seconds as a human-readable duration (e.g. "4d 4h").
///
/// The only caller is a profile's lifetime `total_connection_time_secs`,
/// which for a VPN left up passes a day within a day. This used to have no
/// days field at all and reported that as `100h 0m`.
fn format_duration_long(secs: u64) -> String {
    guitk::duration::coarse(secs)
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

/// Where Export writes and Import reads, relative to `$HOME`.
///
/// A fixed, *named* path rather than a chooser. `guitk::dialog::FileDialog`
/// cannot serve as one here: it is keyboard-only, records no hit targets, and
/// does not read the filesystem — it has to be handed its directory listing —
/// so wiring it in would be a second application rather than a button. A path
/// the status bar names after every Export is honest and usable; a button that
/// says "not implemented" is neither. Tracked as
/// `known-issues.md` → `C-VPNMANAGER-IMPORT-EXPORT-HAVE-NO-FILE-PICKER`.
const PROFILE_FILE: &str = ".config/slateos/vpn/profiles.txt";

/// The absolute path behind [`PROFILE_FILE`], or `None` when `$HOME` is unset.
///
/// `OsStr`, not `String`: a home directory is a path, and paths here allow every
/// byte but `/` and NUL. Forcing it through UTF-8 would refuse to export for a
/// user whose home directory is spelled in bytes Rust will not call a `str`.
fn profile_file() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    Some(std::path::Path::new(&home).join(PROFILE_FILE))
}

/// Split an exported file into the blocks [`parse_profile_text`] understands.
///
/// `export_all` joins profiles with a blank line and starts each with a
/// `[VpnProfile]` header, but `parse_profile_text` skips header lines and reads
/// every `key=value` it finds — so handed the whole file it would fold every
/// profile into one, last value winning. Splitting on the header is what makes
/// exporting three profiles and importing them back yield three.
fn split_profile_blocks(text: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("[VpnProfile]") && !current.trim().is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        blocks.push(current);
    }
    blocks
}

impl VpnManager {
    /// The index of the selected profile, or a status-bar complaint.
    fn require_selection(&mut self) -> Option<usize> {
        match self.selected_profile {
            Some(i) if i < self.profiles.len() => Some(i),
            _ => {
                self.status_message = String::from("Select a profile first");
                None
            }
        }
    }

    /// Put the outcome of a fallible state change in the status bar.
    ///
    /// Every state method that can fail returns `Err(String)` describing why.
    /// Before the window existed those strings had no reader, so a Connect
    /// refused because the profile was disabled looked exactly like one that
    /// worked. One helper, so no control can forget to say.
    fn report(&mut self, outcome: Result<(), String>, done: &str) -> Action {
        self.status_message = match outcome {
            Ok(()) => done.to_string(),
            Err(why) => why,
        };
        Action::Redraw
    }

    /// Give a text box the keyboard.
    fn focus_field(&mut self, field: Field) -> Action {
        // A dialog box only takes focus while the dialog is up, and the two
        // window boxes only while it is not: a caret in a box that is not
        // drawn is a keystroke going somewhere the user cannot see.
        if field.is_dialog() != self.show_add_dialog {
            return Action::None;
        }
        if self.focus == Some(field) {
            return Action::None;
        }
        self.focus = Some(field);
        Action::Redraw
    }

    /// The `u16` behind a numeric dialog field.
    ///
    /// Port and MTU are numbers, and are edited as numbers — `n * 10 + digit`
    /// going in, `n / 10` coming out. A `String` beside the `u16` would be a
    /// second copy of the same value that has to be parsed back on save, and
    /// the two would disagree the moment anything but the keyboard wrote it —
    /// which `start_edit_profile` does, every time the dialog opens.
    fn number_field_mut(&mut self, field: Field) -> Option<&mut u16> {
        let profile = self.editing_profile.as_mut()?;
        match field {
            Field::Port => Some(&mut profile.port),
            Field::Mtu => Some(&mut profile.mtu),
            _ => None,
        }
    }

    /// The `String` behind a textual field, if that field edits one.
    fn text_field_mut(&mut self, field: Field) -> Option<&mut String> {
        match field {
            Field::Search => Some(&mut self.search_query),
            Field::AllowedIp => Some(&mut self.allowed_ip_input),
            Field::Name => Some(&mut self.editing_profile.as_mut()?.name),
            Field::Server => Some(&mut self.editing_profile.as_mut()?.server_address),
            Field::Port | Field::Mtu => None,
        }
    }

    /// Append typed text to whichever box holds the keyboard.
    fn type_into(&mut self, field: Field, typed: &str) -> Action {
        // Editing a dialog field is the user answering the complaint, so the
        // complaint goes away rather than sitting under the corrected value
        // still saying the field is blank.
        if field.is_dialog() {
            self.dialog_error.clear();
        }
        if let Some(slot) = self.number_field_mut(field) {
            let mut changed = false;
            for c in typed.chars() {
                let Some(digit) = c.to_digit(10) else {
                    // Letters in a port box are not an edit. Dropping them
                    // silently beats accepting them and failing validation
                    // later with a number the user never typed.
                    continue;
                };
                // Saturating, not wrapping: holding a digit key should end at
                // 65535, not roll back through zero to a port that works.
                *slot = slot.saturating_mul(10).saturating_add(digit as u16);
                changed = true;
            }
            return if changed {
                Action::Redraw
            } else {
                Action::None
            };
        }
        let Some(slot) = self.text_field_mut(field) else {
            return Action::None;
        };
        slot.push_str(typed);
        if field == Field::Search {
            // The list just shrank under the selection. Selection is by index,
            // and a search that filters the chosen profile out leaves that
            // index pointing at a row the user cannot see, so scroll back to
            // the top rather than leave the view somewhere off the new list.
            self.scroll_offset = 0.0;
        }
        Action::Redraw
    }

    /// Remove the last character from whichever box holds the keyboard.
    fn backspace(&mut self, field: Field) -> Action {
        if field.is_dialog() {
            self.dialog_error.clear();
        }
        if let Some(slot) = self.number_field_mut(field) {
            let next = *slot / 10;
            if next == *slot {
                return Action::None;
            }
            *slot = next;
            return Action::Redraw;
        }
        let Some(slot) = self.text_field_mut(field) else {
            return Action::None;
        };
        if slot.pop().is_none() {
            return Action::None;
        }
        if field == Field::Search {
            self.scroll_offset = 0.0;
        }
        Action::Redraw
    }

    /// Commit the split-tunnel tab's new-range box.
    fn commit_allowed_ip(&mut self) -> Action {
        let Some(index) = self.require_selection() else {
            return Action::Redraw;
        };
        let typed = self.allowed_ip_input.trim().to_string();
        if typed.is_empty() {
            self.status_message = String::from("Type an IP or CIDR range first");
            return Action::Redraw;
        }
        match self.add_allowed_ip(index, &typed) {
            Ok(()) => {
                self.allowed_ip_input.clear();
                self.status_message = format!("Added {typed} to the split tunnel");
            }
            Err(why) => self.status_message = why,
        }
        Action::Redraw
    }

    /// Write every profile to [`PROFILE_FILE`].
    fn export_to_file(&mut self) -> Action {
        let Some(path) = profile_file() else {
            self.status_message = String::from("Cannot export: $HOME is not set");
            return Action::Redraw;
        };
        if let Some(dir) = path.parent()
            && let Err(why) = std::fs::create_dir_all(dir)
        {
            self.status_message = format!("Cannot create {}: {why}", dir.display());
            return Action::Redraw;
        }
        self.status_message = match std::fs::write(&path, self.export_all()) {
            Ok(()) => format!(
                "Exported {} profiles to {}",
                self.profiles.len(),
                path.display()
            ),
            Err(why) => format!("Export failed: {why}"),
        };
        Action::Redraw
    }

    /// Read profiles back out of [`PROFILE_FILE`], adding each as a new one.
    ///
    /// Imported profiles are *added*, never matched against what is already
    /// loaded: `add_profile` assigns a fresh id, and there is no field in the
    /// exported text that identifies a profile across two runs. Merging on name
    /// would silently overwrite a profile the user had since edited.
    fn import_from_file(&mut self) -> Action {
        let Some(path) = profile_file() else {
            self.status_message = String::from("Cannot import: $HOME is not set");
            return Action::Redraw;
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(why) => {
                self.status_message = format!("Cannot read {}: {why}", path.display());
                return Action::Redraw;
            }
        };
        let mut added = 0usize;
        // The first complaint, not the last: a file whose second block is
        // malformed should say so, rather than be overwritten by whatever the
        // fifth block said.
        let mut first_error: Option<String> = None;
        for block in split_profile_blocks(&text) {
            match self.import_profile(&block) {
                Ok(_) => added = added.saturating_add(1),
                Err(why) => {
                    if first_error.is_none() {
                        first_error = Some(why);
                    }
                }
            }
        }
        self.status_message = match first_error {
            None if added == 0 => format!("No profiles found in {}", path.display()),
            None => format!("Imported {added} profiles"),
            Some(why) => format!("Imported {added} profiles; first failure: {why}"),
        };
        Action::Redraw
    }

    /// Act on the target under a click.
    ///
    /// Split from [`Self::handle_click`] so that one test can drive a target
    /// directly and another can drive a *coordinate*, and the two exercise the
    /// same code below the hit-test.
    #[allow(clippy::too_many_lines)] // One arm per control; splitting it would
    // only move the list somewhere else and cost the reader the single place
    // that says what every control does.
    pub fn activate(&mut self, target: Target) -> Action {
        match target {
            Target::ToggleGlobalKillSwitch => {
                self.toggle_global_kill_switch();
                self.status_message = if self.global_kill_switch {
                    String::from("Global kill switch on")
                } else {
                    String::from("Global kill switch off")
                };
                Action::Redraw
            }
            Target::AddProfile => {
                self.start_add_profile();
                // The caret starts in Name because a blank profile fails
                // `validate` on exactly that field, so there is no reason to
                // make the user click before typing.
                self.focus = Some(Field::Name);
                self.status_message = String::from("New profile");
                Action::Redraw
            }
            Target::EditProfile => {
                if self.require_selection().is_none() {
                    return Action::Redraw;
                }
                self.start_edit_profile();
                self.focus = Some(Field::Name);
                Action::Redraw
            }
            Target::RemoveProfile => {
                let Some(index) = self.require_selection() else {
                    return Action::Redraw;
                };
                self.status_message = match self.remove_profile(index) {
                    Some(profile) => format!("Removed {}", profile.name),
                    None => String::from("Could not remove profile"),
                };
                Action::Redraw
            }
            Target::ConnectSelected => {
                let Some(index) = self.require_selection() else {
                    return Action::Redraw;
                };
                let outcome = self.connect(index);
                let name = self.profile_name(index);
                self.report(outcome, &format!("Connected to {name}"))
            }
            Target::DisconnectSelected => {
                let Some(index) = self.require_selection() else {
                    return Action::Redraw;
                };
                let name = self.profile_name(index);
                let outcome = self.disconnect(index);
                self.report(outcome, &format!("Disconnected from {name}"))
            }
            Target::ReconnectSelected => {
                let Some(index) = self.require_selection() else {
                    return Action::Redraw;
                };
                let outcome = self.reconnect(index);
                let name = self.profile_name(index);
                self.report(outcome, &format!("Reconnected to {name}"))
            }
            Target::QuickConnect => {
                let outcome = self.quick_connect();
                let name = self
                    .last_connected_id
                    .and_then(|id| self.profiles.iter().find(|p| p.id == id))
                    .map_or_else(String::new, |p| p.name.clone());
                self.report(outcome, &format!("Connected to {name}"))
            }
            Target::Import => self.import_from_file(),
            Target::Export => self.export_to_file(),
            Target::CycleSort => {
                self.set_sort_order(self.sort_order.next());
                self.status_message = format!("Sorted by {}", self.sort_order.label());
                Action::Redraw
            }
            Target::Profile(id) => {
                let Some(index) = self.profiles.iter().position(|p| p.id == id) else {
                    return Action::None;
                };
                if self.selected_profile == Some(index) {
                    return Action::None;
                }
                self.select_profile(index);
                // A row click takes the keyboard away from the search box, so
                // the arrow keys move the selection the click just made rather
                // than typing into a box the user has stopped looking at.
                self.focus = None;
                Action::Redraw
            }
            Target::Focus(field) => self.focus_field(field),
            Target::Tab(tab) => {
                if self.current_tab == tab {
                    return Action::None;
                }
                self.set_tab(tab);
                self.focus = None;
                Action::Redraw
            }
            Target::ToggleEnabled => self.on_selected("Enabled", |mgr, i| mgr.toggle_enabled(i)),
            Target::ToggleAutoConnect => {
                self.on_selected("Auto connect", |mgr, i| mgr.toggle_auto_connect(i))
            }
            Target::ToggleAutoReconnect => {
                self.on_selected("Auto reconnect", |mgr, i| mgr.toggle_auto_reconnect(i))
            }
            Target::ToggleKillSwitch => {
                self.on_selected("Kill switch", |mgr, i| mgr.toggle_kill_switch(i))
            }
            Target::ToggleSplitTunnel => {
                self.on_selected("Split tunnel", |mgr, i| mgr.toggle_split_tunnel(i))
            }
            Target::ToggleCompression => {
                let Some(index) = self.require_selection() else {
                    return Action::Redraw;
                };
                if !self.toggle_compression(index) {
                    self.status_message = String::from("Compression is an OpenVPN setting");
                    return Action::Redraw;
                }
                self.status_message = String::from("Compression toggled");
                Action::Redraw
            }
            Target::RemoveAllowedIp(ip_index) => {
                let Some(index) = self.require_selection() else {
                    return Action::Redraw;
                };
                let outcome = self.remove_allowed_ip(index, ip_index);
                self.report(outcome, "Removed range from the split tunnel")
            }
            Target::AddAllowedIp => self.commit_allowed_ip(),
            Target::ClearLog => {
                self.clear_log();
                self.status_message = String::from("Log cleared");
                Action::Redraw
            }
            Target::CycleProtocol => {
                let Some(profile) = self.editing_profile.as_mut() else {
                    return Action::None;
                };
                profile.protocol = profile.protocol.next();
                // The port and the protocol-specific settings follow the
                // protocol, because a WireGuard profile carrying OpenVPN's
                // 1194 and an `OpenVpn { cipher }` block is not a profile any
                // back end could use.
                profile.port = profile.protocol.default_port();
                profile.protocol_settings = ProtocolSettings::for_protocol(profile.protocol);
                Action::Redraw
            }
            Target::CycleAuth => {
                let Some(profile) = self.editing_profile.as_mut() else {
                    return Action::None;
                };
                profile.auth_method = profile.auth_method.next();
                Action::Redraw
            }
            Target::DialogToggleKillSwitch => {
                self.on_editing(|profile| profile.kill_switch = !profile.kill_switch)
            }
            Target::DialogToggleAutoConnect => {
                self.on_editing(|profile| profile.auto_connect = !profile.auto_connect)
            }
            Target::DialogToggleSplitTunnel => {
                self.on_editing(|profile| profile.split_tunnel = !profile.split_tunnel)
            }
            Target::DialogSave => {
                let outcome = self.confirm_edit();
                match &outcome {
                    Ok(()) => {
                        // The dialog is gone, so the caret must go with it —
                        // the box it was in is no longer drawn.
                        self.focus = None;
                        self.dialog_error.clear();
                    }
                    // `confirm_edit` leaves the dialog up with what was typed
                    // still in it; the complaint goes where it can be read.
                    Err(why) => self.dialog_error.clone_from(why),
                }
                self.report(outcome, "Profile saved")
            }
            Target::DialogCancel => {
                self.cancel_edit();
                self.focus = None;
                self.dialog_error.clear();
                self.status_message = String::from("Cancelled");
                Action::Redraw
            }
        }
    }

    /// The selected profile's name, for a status line, or `""`.
    fn profile_name(&self, index: usize) -> String {
        self.profiles
            .get(index)
            .map_or_else(String::new, |p| p.name.clone())
    }

    /// Run `change` against the selected profile and say what it was.
    ///
    /// The five overview toggles differ only in which `bool` they flip and what
    /// the status bar calls it, so they share one arm rather than five copies
    /// of the same selection check and the same message-building.
    fn on_selected(&mut self, label: &str, change: impl FnOnce(&mut Self, usize)) -> Action {
        let Some(index) = self.require_selection() else {
            return Action::Redraw;
        };
        change(self, index);
        self.status_message = format!("{label} toggled");
        Action::Redraw
    }

    /// Run `change` against the profile in the dialog, if one is open.
    fn on_editing(&mut self, change: impl FnOnce(&mut VpnProfile)) -> Action {
        let Some(profile) = self.editing_profile.as_mut() else {
            return Action::None;
        };
        change(profile);
        Action::Redraw
    }

    /// What is under a point, by rendering the frame and asking it.
    ///
    /// The geometry comes from the same walk that draws, so there is no second
    /// copy of the layout for the hit-test to drift from — and a control that
    /// scrolled out of its panel is not clickable, because the clip that hid it
    /// also dropped its target.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32, size: (f32, f32)) -> Option<Target> {
        let (width, height) = size;
        render_frame(self, width, height).hit_test(x, y)
    }

    /// Route a click at window coordinates.
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
        let Some(target) = self.hit_test(x, y, size) else {
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
        // With the dialog up, the keys below would move a selection and switch
        // tabs behind it. Escape closes it; nothing else reaches past it.
        if self.show_add_dialog {
            return match key.key {
                Key::Escape => self.activate(Target::DialogCancel),
                Key::Enter => self.activate(Target::DialogSave),
                Key::Tab => self.focus_field(Field::Name),
                _ => Action::None,
            };
        }
        match key.key {
            Key::Escape => Action::Quit,
            Key::Down => self.move_selection(1),
            Key::Up => self.move_selection(-1),
            Key::Right => self.move_tab(1),
            Key::Left => self.move_tab(-1),
            Key::PageDown => {
                self.scroll_by(1);
                Action::Redraw
            }
            Key::PageUp => {
                self.scroll_by(-1);
                Action::Redraw
            }
            Key::Home => {
                self.scroll_to_top();
                Action::Redraw
            }
            _ => Action::None,
        }
    }

    /// A keystroke while a text box holds the keyboard.
    fn handle_key_in_field(&mut self, key: &KeyEvent, field: Field) -> Action {
        match key.key {
            Key::Escape => {
                self.focus = None;
                Action::Redraw
            }
            Key::Tab => {
                self.focus = Some(field.next());
                Action::Redraw
            }
            Key::Enter => match field {
                Field::AllowedIp => self.commit_allowed_ip(),
                Field::Search => {
                    self.focus = None;
                    Action::Redraw
                }
                _ => self.activate(Target::DialogSave),
            },
            Key::Backspace => self.backspace(field),
            _ => {
                // `typed()` already drops the control characters Enter, Tab,
                // Escape and Backspace produce on most layouts, so an unmatched
                // key cannot smuggle a `\r` into a server address.
                let typed: String = key.typed().collect();
                if typed.is_empty() {
                    return Action::None;
                }
                self.type_into(field, &typed)
            }
        }
    }

    /// Move the sidebar selection by `delta` rows within the *filtered* list.
    ///
    /// Within the filtered list, because that is what is on screen: stepping
    /// through the unfiltered indices while a search is active would walk the
    /// selection onto rows the sidebar is not drawing.
    fn move_selection(&mut self, delta: isize) -> Action {
        let visible = self.filtered_profiles();
        if visible.is_empty() {
            return Action::None;
        }
        let here = self
            .selected_profile
            .and_then(|sel| visible.iter().position(|i| *i == sel));
        let last = visible.len().saturating_sub(1);
        let next_row = match here {
            None if delta < 0 => last,
            None => 0,
            Some(row) if delta < 0 => row.saturating_sub(delta.unsigned_abs()),
            Some(row) => row.saturating_add(delta.unsigned_abs()).min(last),
        };
        let Some(&index) = visible.get(next_row) else {
            return Action::None;
        };
        if self.selected_profile == Some(index) {
            return Action::None;
        }
        self.select_profile(index);
        self.scroll_row_into_view(next_row);
        Action::Redraw
    }

    /// Move to the next or previous detail tab, wrapping.
    fn move_tab(&mut self, delta: isize) -> Action {
        let tabs = DetailTab::all();
        let Some(here) = tabs.iter().position(|t| *t == self.current_tab) else {
            return Action::None;
        };
        let len = tabs.len();
        // Stepping back is stepping forward by one short of a full lap, which
        // keeps the wrap in unsigned arithmetic and out of `-1`.
        let step = if delta < 0 { len.saturating_sub(1) } else { 1 };
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

    /// How many sidebar rows fit at the last size the window was drawn at.
    fn visible_rows(&self) -> f32 {
        let (_, height) = self.window_size;
        // The list starts 44 px below the panel — an 8 px gap, a 28 px search
        // box and its 8 px gap — which is exactly what `render_sidebar`
        // subtracts for its clip.
        let list_h =
            (height - TITLE_BAR_HEIGHT - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT - 44.0).max(0.0);
        (list_h / SIDEBAR_ITEM_HEIGHT).floor().max(1.0)
    }

    /// The furthest down the sidebar can scroll before the last row is at the
    /// bottom of the panel.
    fn max_scroll(&self) -> f32 {
        let rows = self.filtered_profiles().len() as f32;
        (rows - self.visible_rows()).max(0.0) * SIDEBAR_ITEM_HEIGHT
    }

    /// Scroll the sidebar by whole rows, clamped at both ends.
    ///
    /// Clamped at the bottom as well as the top: a list scrolled past its own
    /// end shows an empty panel with no way back but scrolling up through the
    /// blank space it just created.
    fn scroll_by(&mut self, rows: isize) {
        let delta = (rows as f32) * SIDEBAR_ITEM_HEIGHT;
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + delta).clamp(0.0, max);
    }

    /// Back to the first sidebar row.
    fn scroll_to_top(&mut self) {
        self.scroll_offset = 0.0;
    }

    /// Bring `row` of the filtered list inside the panel.
    ///
    /// Arrowing down past the bottom row has to move the view, or the selection
    /// walks off the panel and the user is choosing profiles they cannot see —
    /// which is the same class of bug the clip-aware hit-test fixed from the
    /// other direction.
    fn scroll_row_into_view(&mut self, row: usize) {
        let top = (row as f32) * SIDEBAR_ITEM_HEIGHT;
        let bottom = top + SIDEBAR_ITEM_HEIGHT;
        let view_h = self.visible_rows() * SIDEBAR_ITEM_HEIGHT;
        if top < self.scroll_offset {
            self.scroll_offset = top;
        } else if bottom > self.scroll_offset + view_h {
            self.scroll_offset = (bottom - view_h).max(0.0);
        }
    }

    /// Route a whole event.
    pub fn handle_event(&mut self, event: &Event, size: (f32, f32)) -> Action {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Press(button) => self.handle_click(mouse.x, mouse.y, button, size),
                MouseEventKind::Scroll { dy, .. } => {
                    // The wheel scrolls whichever list is under it: the log if
                    // the pointer is over the log tab's panel, the profile list
                    // otherwise. The accumulator keeps the fractions a trackpad
                    // sends, so a slow drag moves instead of rounding to zero.
                    let rows = self.wheel.rows(dy);
                    if rows == 0 {
                        return Action::None;
                    }
                    if mouse.x > SIDEBAR_WIDTH && self.current_tab == DetailTab::Log {
                        self.scroll_log_by(rows);
                    } else {
                        self.scroll_by(rows);
                    }
                    Action::Redraw
                }
                _ => Action::None,
            },
            Event::Key(key) => self.handle_key(key),
            Event::Tick { elapsed_ms } => {
                if self.advance(*elapsed_ms) {
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            Event::CloseRequested => Action::Quit,
            _ => Action::None,
        }
    }
}

// ============================================================================
// Entry Point
// ============================================================================

impl App for VpnManager {
    fn title(&self) -> String {
        String::from("VPN Manager")
    }

    fn app_id(&self) -> String {
        String::from("slateos.vpnmanager")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// A second, and only while something is connected.
    ///
    /// Uptime is displayed to the second, so a faster clock would repaint the
    /// window for a figure that had not changed; a slower one would visibly
    /// skip. With nothing connected there is nothing to age, and an app that
    /// keeps ticking with nothing to advance holds the whole desktop awake.
    fn tick_interval(&self) -> Option<Duration> {
        if self.connections.iter().any(|c| c.status.is_active()) {
            Some(Duration::from_secs(1))
        } else {
            None
        }
    }

    fn on_event(&mut self, event: &Event) -> Response {
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
        // Believe the size we are handed: the first frame goes out before any
        // `Event::Resize`, so the stored size is only a starting guess.
        self.window_size = (width, height);
        render_frame(self, width, height).into_tree()
    }
}

/// Lets the tests drive this window by naming its controls rather than
/// measuring them. Three lines of forwarding; the helpers are in
/// [`guitk::probe`].
impl Probe for VpnManager {
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
    let mut app = VpnManager::new();
    app::launch("vpnmanager", &mut app)
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
    use guitk::event::MouseEvent;

    // --- VpnProtocol tests ---

    #[test]
    fn test_protocol_labels() {
        assert_eq!(VpnProtocol::OpenVPN.label(), "OpenVPN");
        assert_eq!(VpnProtocol::WireGuard.label(), "WireGuard");
        assert_eq!(VpnProtocol::IPSec.label(), "IPSec/IKEv2");
        assert_eq!(VpnProtocol::L2TP.label(), "L2TP/IPSec");
        assert_eq!(VpnProtocol::PPTP.label(), "PPTP");
        assert_eq!(VpnProtocol::SSTP.label(), "SSTP");
    }

    #[test]
    fn test_protocol_colors_distinct() {
        let protos = VpnProtocol::all();
        for (i, a) in protos.iter().enumerate() {
            for b in &protos[i + 1..] {
                assert_ne!(a.color(), b.color(), "{a:?} and {b:?} share a color");
            }
        }
    }

    #[test]
    fn test_protocol_all_contains_all_variants() {
        let all = VpnProtocol::all();
        assert_eq!(all.len(), 6);
        assert!(all.contains(&VpnProtocol::OpenVPN));
        assert!(all.contains(&VpnProtocol::WireGuard));
        assert!(all.contains(&VpnProtocol::IPSec));
        assert!(all.contains(&VpnProtocol::L2TP));
        assert!(all.contains(&VpnProtocol::PPTP));
        assert!(all.contains(&VpnProtocol::SSTP));
    }

    #[test]
    fn test_protocol_default_ports() {
        assert_eq!(VpnProtocol::OpenVPN.default_port(), 1194);
        assert_eq!(VpnProtocol::WireGuard.default_port(), 51820);
        assert_eq!(VpnProtocol::IPSec.default_port(), 500);
        assert_eq!(VpnProtocol::L2TP.default_port(), 1701);
        assert_eq!(VpnProtocol::PPTP.default_port(), 1723);
        assert_eq!(VpnProtocol::SSTP.default_port(), 443);
    }

    // --- AuthMethod tests ---

    #[test]
    fn test_auth_method_labels() {
        assert_eq!(AuthMethod::default_certificate().label(), "Certificate");
        assert_eq!(
            AuthMethod::default_username_password().label(),
            "Username/Password"
        );
        assert_eq!(AuthMethod::default_psk().label(), "Pre-Shared Key");
        assert_eq!(AuthMethod::default_token().label(), "Token");
    }

    #[test]
    fn test_auth_method_certificate_fields() {
        let auth = AuthMethod::Certificate {
            cert_path: String::from("/certs/my.crt"),
            key_path: String::from("/certs/my.key"),
        };
        if let AuthMethod::Certificate {
            cert_path,
            key_path,
        } = &auth
        {
            assert_eq!(cert_path, "/certs/my.crt");
            assert_eq!(key_path, "/certs/my.key");
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_auth_method_username_password_fields() {
        let auth = AuthMethod::UsernamePassword {
            username: String::from("user"),
            password: String::from("pass"),
        };
        if let AuthMethod::UsernamePassword { username, password } = &auth {
            assert_eq!(username, "user");
            assert_eq!(password, "pass");
        } else {
            panic!("Wrong variant");
        }
    }

    // --- ConnectionStatus tests ---

    #[test]
    fn test_connection_status_labels() {
        assert_eq!(ConnectionStatus::Disconnected.label(), "Disconnected");
        assert_eq!(ConnectionStatus::Connecting.label(), "Connecting...");
        assert_eq!(ConnectionStatus::Connected.label(), "Connected");
        assert_eq!(ConnectionStatus::Reconnecting.label(), "Reconnecting...");
        assert_eq!(
            ConnectionStatus::Error(String::from("fail")).label(),
            "Error"
        );
    }

    #[test]
    fn test_connection_status_is_active() {
        assert!(!ConnectionStatus::Disconnected.is_active());
        assert!(!ConnectionStatus::Connecting.is_active());
        assert!(ConnectionStatus::Connected.is_active());
        assert!(ConnectionStatus::Reconnecting.is_active());
        assert!(!ConnectionStatus::Error(String::from("x")).is_active());
    }

    #[test]
    fn test_connection_status_colors_differ() {
        assert_ne!(
            ConnectionStatus::Connected.color(),
            ConnectionStatus::Disconnected.color()
        );
        assert_ne!(
            ConnectionStatus::Connected.color(),
            ConnectionStatus::Error(String::new()).color()
        );
        assert_ne!(
            ConnectionStatus::Disconnected.color(),
            ConnectionStatus::Connecting.color()
        );
    }

    // --- ProtocolSettings tests ---

    #[test]
    fn test_protocol_settings_for_openvpn() {
        let settings = ProtocolSettings::for_protocol(VpnProtocol::OpenVPN);
        assert!(matches!(settings, ProtocolSettings::OpenVpn { .. }));
    }

    #[test]
    fn test_protocol_settings_for_wireguard() {
        let settings = ProtocolSettings::for_protocol(VpnProtocol::WireGuard);
        if let ProtocolSettings::WireGuard {
            persistent_keepalive,
            ..
        } = settings
        {
            assert_eq!(persistent_keepalive, 25);
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_protocol_settings_for_ipsec() {
        let settings = ProtocolSettings::for_protocol(VpnProtocol::IPSec);
        if let ProtocolSettings::IPSec { ike_version, .. } = settings {
            assert_eq!(ike_version, 2);
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_protocol_settings_generic_for_l2tp() {
        let settings = ProtocolSettings::for_protocol(VpnProtocol::L2TP);
        assert!(matches!(settings, ProtocolSettings::Generic));
    }

    #[test]
    fn test_protocol_settings_generic_for_pptp() {
        let settings = ProtocolSettings::for_protocol(VpnProtocol::PPTP);
        assert!(matches!(settings, ProtocolSettings::Generic));
    }

    #[test]
    fn test_protocol_settings_generic_for_sstp() {
        let settings = ProtocolSettings::for_protocol(VpnProtocol::SSTP);
        assert!(matches!(settings, ProtocolSettings::Generic));
    }

    // --- VpnProfile tests ---

    #[test]
    fn test_profile_new_defaults() {
        let p = VpnProfile::new(1, "Test", "vpn.test.com", VpnProtocol::WireGuard);
        assert_eq!(p.id, 1);
        assert_eq!(p.name, "Test");
        assert_eq!(p.server_address, "vpn.test.com");
        assert_eq!(p.port, 51820);
        assert_eq!(p.protocol, VpnProtocol::WireGuard);
        assert!(p.enabled);
        assert!(!p.auto_connect);
        assert!(p.auto_reconnect);
        assert!(!p.kill_switch);
        assert!(!p.split_tunnel);
        assert_eq!(p.mtu, 1500);
        assert!(p.dns_override.is_empty());
        assert!(p.allowed_ips.is_empty());
        assert!(p.notes.is_empty());
    }

    #[test]
    fn test_profile_validate_ok() {
        let p = VpnProfile::new(1, "Valid", "1.2.3.4", VpnProtocol::OpenVPN);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn test_profile_validate_empty_name() {
        let p = VpnProfile::new(1, "", "1.2.3.4", VpnProtocol::OpenVPN);
        let err = p.validate().unwrap_err();
        assert!(err.contains("name"));
    }

    #[test]
    fn test_profile_validate_empty_server() {
        let p = VpnProfile::new(1, "Test", "", VpnProtocol::OpenVPN);
        let err = p.validate().unwrap_err();
        assert!(err.contains("Server"));
    }

    #[test]
    fn test_profile_validate_zero_port() {
        let mut p = VpnProfile::new(1, "Test", "1.2.3.4", VpnProtocol::OpenVPN);
        p.port = 0;
        let err = p.validate().unwrap_err();
        assert!(err.contains("Port"));
    }

    #[test]
    fn test_profile_validate_bad_dns() {
        let mut p = VpnProfile::new(1, "Test", "1.2.3.4", VpnProtocol::OpenVPN);
        p.dns_override.push(String::from("not-an-ip"));
        let err = p.validate().unwrap_err();
        assert!(err.contains("DNS"));
    }

    #[test]
    fn test_profile_validate_bad_allowed_ip() {
        let mut p = VpnProfile::new(1, "Test", "1.2.3.4", VpnProtocol::OpenVPN);
        p.split_tunnel = true;
        p.allowed_ips.push(String::from("bad-cidr"));
        let err = p.validate().unwrap_err();
        assert!(err.contains("CIDR"));
    }

    #[test]
    fn test_profile_validate_mtu_too_low() {
        let mut p = VpnProfile::new(1, "Test", "1.2.3.4", VpnProtocol::OpenVPN);
        p.mtu = 100;
        let err = p.validate().unwrap_err();
        assert!(err.contains("MTU"));
    }

    #[test]
    fn test_profile_validate_mtu_too_high() {
        let mut p = VpnProfile::new(1, "Test", "1.2.3.4", VpnProtocol::OpenVPN);
        p.mtu = 10000;
        let err = p.validate().unwrap_err();
        assert!(err.contains("MTU"));
    }

    #[test]
    fn test_profile_export_text() {
        let mut p = VpnProfile::new(1, "Export Test", "vpn.test.com", VpnProtocol::WireGuard);
        p.kill_switch = true;
        p.dns_override = vec![String::from("1.1.1.1")];
        let text = p.export_text();
        assert!(text.contains("name=Export Test"));
        assert!(text.contains("server=vpn.test.com"));
        assert!(text.contains("protocol=WireGuard"));
        assert!(text.contains("kill_switch=true"));
        assert!(text.contains("dns=1.1.1.1"));
    }

    #[test]
    fn test_profile_export_text_no_optional_fields() {
        let p = VpnProfile::new(1, "Minimal", "server.com", VpnProtocol::PPTP);
        let text = p.export_text();
        assert!(text.contains("name=Minimal"));
        assert!(!text.contains("dns="));
        assert!(!text.contains("notes="));
    }

    // --- VpnConnection tests ---

    #[test]
    fn test_connection_new_defaults() {
        let c = VpnConnection::new(42);
        assert_eq!(c.profile_id, 42);
        assert_eq!(c.status, ConnectionStatus::Disconnected);
        assert!(c.local_ip.is_empty());
        assert!(c.remote_ip.is_empty());
        assert_eq!(c.latency_ms, 0);
        assert_eq!(c.uptime_secs, 0);
        assert_eq!(c.bytes_sent, 0);
        assert_eq!(c.bytes_received, 0);
        assert!(c.connected_since.is_none());
    }

    #[test]
    fn test_connection_format_uptime_zero() {
        let c = VpnConnection::new(1);
        assert_eq!(c.format_uptime(), "00:00:00");
    }

    #[test]
    fn test_connection_format_uptime_hours() {
        let mut c = VpnConnection::new(1);
        c.uptime_secs = 3661;
        assert_eq!(c.format_uptime(), "01:01:01");
    }

    #[test]
    fn test_connection_format_uptime_large() {
        let mut c = VpnConnection::new(1);
        c.uptime_secs = 100 * 3600 + 59 * 60 + 59;
        assert_eq!(c.format_uptime(), "100:59:59");
    }

    // --- LogEntry / LogLevel tests ---

    #[test]
    fn test_log_level_labels() {
        assert_eq!(LogLevel::Info.label(), "INFO");
        assert_eq!(LogLevel::Warning.label(), "WARN");
        assert_eq!(LogLevel::Error.label(), "ERROR");
    }

    #[test]
    fn test_log_level_colors_distinct() {
        assert_ne!(LogLevel::Info.color(), LogLevel::Warning.color());
        assert_ne!(LogLevel::Warning.color(), LogLevel::Error.color());
        assert_ne!(LogLevel::Info.color(), LogLevel::Error.color());
    }

    // --- SortOrder tests ---

    #[test]
    fn test_sort_order_labels() {
        assert_eq!(SortOrder::Name.label(), "Name");
        assert_eq!(SortOrder::Status.label(), "Status");
        assert_eq!(SortOrder::Protocol.label(), "Protocol");
    }

    #[test]
    fn test_sort_order_all() {
        let all = SortOrder::all();
        assert_eq!(all.len(), 3);
    }

    // --- DetailTab tests ---

    #[test]
    fn test_detail_tab_labels() {
        assert_eq!(DetailTab::Overview.label(), "Overview");
        assert_eq!(DetailTab::Connection.label(), "Connection");
        assert_eq!(DetailTab::SplitTunnel.label(), "Split Tunnel");
        assert_eq!(DetailTab::ProtocolConfig.label(), "Protocol");
        assert_eq!(DetailTab::Log.label(), "Log");
        assert_eq!(DetailTab::Stats.label(), "Statistics");
    }

    #[test]
    fn test_detail_tab_all_count() {
        assert_eq!(DetailTab::all().len(), 6);
    }

    // --- Validation helpers ---

    #[test]
    fn test_is_valid_ipv4_good() {
        assert!(is_valid_ipv4("1.2.3.4"));
        assert!(is_valid_ipv4("0.0.0.0"));
        assert!(is_valid_ipv4("255.255.255.255"));
        assert!(is_valid_ipv4("192.168.1.1"));
    }

    #[test]
    fn test_is_valid_ipv4_bad() {
        assert!(!is_valid_ipv4(""));
        assert!(!is_valid_ipv4("not-an-ip"));
        assert!(!is_valid_ipv4("1.2.3"));
        assert!(!is_valid_ipv4("1.2.3.4.5"));
        assert!(!is_valid_ipv4("256.1.1.1"));
        assert!(!is_valid_ipv4("1.2.3.abc"));
    }

    #[test]
    fn test_is_valid_cidr_or_ip_plain() {
        assert!(is_valid_cidr_or_ip("10.0.0.1"));
        assert!(!is_valid_cidr_or_ip("garbage"));
    }

    #[test]
    fn test_is_valid_cidr_or_ip_cidr() {
        assert!(is_valid_cidr_or_ip("10.0.0.0/8"));
        assert!(is_valid_cidr_or_ip("192.168.0.0/16"));
        assert!(is_valid_cidr_or_ip("172.16.0.0/12"));
    }

    #[test]
    fn test_is_valid_cidr_or_ip_bad_cidr() {
        assert!(!is_valid_cidr_or_ip("10.0.0.0/33"));
        assert!(!is_valid_cidr_or_ip("10.0.0.0/abc"));
        assert!(!is_valid_cidr_or_ip("bad/8"));
    }

    #[test]
    fn test_status_sort_key_ordering() {
        assert!(
            status_sort_key(&ConnectionStatus::Connected)
                < status_sort_key(&ConnectionStatus::Connecting)
        );
        assert!(
            status_sort_key(&ConnectionStatus::Connecting)
                < status_sort_key(&ConnectionStatus::Disconnected)
        );
        assert!(
            status_sort_key(&ConnectionStatus::Error(String::new()))
                < status_sort_key(&ConnectionStatus::Disconnected)
        );
    }

    // --- format_bytes tests ---
    //
    // A tunnel's `rx_bytes`/`tx_bytes` is a link counter, so `format_bytes` is
    // decimal (design-decisions.md §489). The inputs below are therefore powers
    // of ten: these tests used to feed powers of two to a formatter that had
    // been base-1024-labelled-`KB`, which made the expectations agree with the
    // bug rather than with the unit.

    #[test]
    fn test_format_bytes_small() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(999), "999 B");
    }

    #[test]
    fn test_format_bytes_kb() {
        assert_eq!(format_bytes(1000), "1.0 kB");
        assert_eq!(format_bytes(1500), "1.5 kB");
    }

    #[test]
    fn test_format_bytes_mb() {
        assert_eq!(format_bytes(1_000_000), "1.0 MB");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
    }

    #[test]
    fn test_format_bytes_gb() {
        assert_eq!(format_bytes(1_000_000_000), "1.0 GB");
    }

    /// The decimal choice is deliberate, so pin the one place it is visible: a
    /// mebibyte of traffic is 1.0 MiB but 1.05 MB, and this counter reports the
    /// latter — the same figure the tray indicator and the network settings
    /// page show for the same bytes.
    #[test]
    fn a_binary_quantity_is_reported_in_decimal_units() {
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.1 GB");
    }

    #[test]
    fn test_format_timestamp() {
        assert_eq!(format_timestamp(0), "00:00:00");
        assert_eq!(format_timestamp(3661), "01:01:01");
    }

    #[test]
    fn test_format_duration_long_zero() {
        // Was "0m". A profile that has never connected has no minutes
        // either; the unit shown is now the one the number actually has.
        assert_eq!(format_duration_long(0), "0s");
    }

    #[test]
    fn test_format_duration_long_hours() {
        assert_eq!(format_duration_long(7200), "2h 0m");
        assert_eq!(format_duration_long(3660), "1h 1m");
        // Was "100h 0m", because there was no days field.
        assert_eq!(format_duration_long(360_000), "4d 4h");
    }

    // --- Overview tab prose ---

    /// Longer than the overview panel is wide at 12pt, with a distinctive
    /// final word to check nothing was dropped off the end.
    const LONG_NOTES: &str = "Use this profile for the Stockholm office only \
        — the London gateway rejects the certificate that ships with it, and \
        the support desk has asked twice now that we stop opening tickets \
        about it. Renew in March.";

    /// The lines of the notes field: every 12pt body text drawn below the
    /// "Notes" section heading, top to bottom.
    fn notes_lines(mgr: &VpnManager) -> Vec<(f32, String)> {
        let mut frame = new_frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        render_tab_overview(&mut frame, mgr, 0.0, 0.0, 600.0);
        let tree = frame.into_tree();
        let heading_y = tree
            .commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, y, .. } if text == "Notes" => Some(*y),
                _ => None,
            })
            .expect("the notes heading is drawn");
        let mut lines: Vec<(f32, String)> = tree
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    text,
                    color,
                    font_size,
                    ..
                } if *color == SUBTEXT0
                    && (*font_size - NOTES_FONT_SIZE).abs() < 0.01
                    && *y > heading_y =>
                {
                    Some((*y, text.clone()))
                }
                _ => None,
            })
            .collect();
        lines.sort_by(|a, b| a.0.total_cmp(&b.0));
        lines
    }

    #[test]
    fn long_profile_notes_are_wrapped_not_truncated() {
        let mut mgr = VpnManager::new();
        mgr.profiles[0].notes = LONG_NOTES.to_string();
        mgr.selected_profile = Some(0);
        let lines = notes_lines(&mgr);
        assert!(
            lines.len() > 1,
            "paragraph-length notes were drawn as {} line(s)",
            lines.len()
        );
        let drawn = lines
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            drawn.contains("March"),
            "the end of the notes was cut off: {drawn}"
        );
    }

    #[test]
    fn profile_notes_lines_do_not_sit_on_top_of_one_another() {
        let mut mgr = VpnManager::new();
        mgr.profiles[0].notes = LONG_NOTES.to_string();
        mgr.selected_profile = Some(0);
        let lines = notes_lines(&mgr);
        for pair in lines.windows(2) {
            let gap = pair[1].0 - pair[0].0;
            assert!(
                gap >= NOTES_FONT_SIZE,
                "two notes lines are {gap} apart, closer than the text is tall"
            );
        }
    }

    // --- VpnManager CRUD tests ---

    #[test]
    fn test_manager_new_has_profiles() {
        let mgr = VpnManager::new();
        assert!(!mgr.profiles.is_empty());
        assert_eq!(mgr.profiles.len(), mgr.connections.len());
    }

    #[test]
    fn test_manager_new_default_selection() {
        let mgr = VpnManager::new();
        assert_eq!(mgr.selected_profile, Some(0));
    }

    #[test]
    fn test_manager_add_profile() {
        let mut mgr = VpnManager::new();
        let initial = mgr.profiles.len();
        let p = VpnProfile::new(0, "New VPN", "new.vpn.com", VpnProtocol::SSTP);
        let id = mgr.add_profile(p).unwrap();
        assert_eq!(mgr.profiles.len(), initial + 1);
        assert_eq!(mgr.connections.len(), initial + 1);
        assert!(mgr.profiles.iter().any(|p| p.id == id));
    }

    #[test]
    fn test_manager_add_profile_invalid() {
        let mut mgr = VpnManager::new();
        let p = VpnProfile::new(0, "", "server.com", VpnProtocol::PPTP);
        assert!(mgr.add_profile(p).is_err());
    }

    #[test]
    fn test_manager_remove_profile() {
        let mut mgr = VpnManager::new();
        let initial = mgr.profiles.len();
        let removed = mgr.remove_profile(0);
        assert!(removed.is_some());
        assert_eq!(mgr.profiles.len(), initial - 1);
    }

    #[test]
    fn test_manager_remove_profile_out_of_bounds() {
        let mut mgr = VpnManager::new();
        assert!(mgr.remove_profile(999).is_none());
    }

    #[test]
    fn test_manager_remove_last_profile_clears_selection() {
        let mut mgr = VpnManager::new();
        while mgr.profiles.len() > 1 {
            mgr.remove_profile(0);
        }
        mgr.remove_profile(0);
        assert!(mgr.selected_profile.is_none());
    }

    #[test]
    fn test_manager_update_profile() {
        let mut mgr = VpnManager::new();
        let mut updated = mgr.profiles[0].clone();
        updated.name = String::from("Updated Name");
        assert!(mgr.update_profile(0, updated).is_ok());
        assert_eq!(mgr.profiles[0].name, "Updated Name");
    }

    #[test]
    fn test_manager_update_profile_invalid_index() {
        let mut mgr = VpnManager::new();
        let p = VpnProfile::new(1, "X", "1.2.3.4", VpnProtocol::PPTP);
        assert!(mgr.update_profile(999, p).is_err());
    }

    #[test]
    fn test_manager_selected() {
        let mgr = VpnManager::new();
        assert!(mgr.selected().is_some());
    }

    #[test]
    fn test_manager_selected_none() {
        let mut mgr = VpnManager::new();
        mgr.selected_profile = None;
        assert!(mgr.selected().is_none());
    }

    #[test]
    fn test_manager_select_profile() {
        let mut mgr = VpnManager::new();
        mgr.select_profile(2);
        assert_eq!(mgr.selected_profile, Some(2));
    }

    #[test]
    fn test_manager_select_profile_out_of_bounds() {
        let mut mgr = VpnManager::new();
        let old = mgr.selected_profile;
        mgr.select_profile(999);
        assert_eq!(mgr.selected_profile, old);
    }

    // --- Connection tests ---

    #[test]
    fn test_manager_connect() {
        let mut mgr = VpnManager::new();
        assert!(mgr.connect(0).is_ok());
        let conn = mgr.connection_for(mgr.profiles[0].id).unwrap();
        assert_eq!(conn.status, ConnectionStatus::Connected);
    }

    #[test]
    fn test_manager_connect_disabled_profile() {
        let mut mgr = VpnManager::new();
        mgr.profiles[0].enabled = false;
        assert!(mgr.connect(0).is_err());
    }

    #[test]
    fn test_manager_connect_already_connected() {
        let mut mgr = VpnManager::new();
        mgr.connect(0).unwrap();
        assert!(mgr.connect(0).is_err());
    }

    #[test]
    fn test_manager_connect_invalid_index() {
        let mut mgr = VpnManager::new();
        assert!(mgr.connect(999).is_err());
    }

    #[test]
    fn test_manager_disconnect() {
        let mut mgr = VpnManager::new();
        mgr.connect(0).unwrap();
        assert!(mgr.disconnect(0).is_ok());
        let conn = mgr.connection_for(mgr.profiles[0].id).unwrap();
        assert_eq!(conn.status, ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_manager_disconnect_not_connected() {
        let mut mgr = VpnManager::new();
        assert!(mgr.disconnect(0).is_err());
    }

    #[test]
    fn test_manager_disconnect_accumulates_stats() {
        let mut mgr = VpnManager::new();
        mgr.connect(0).unwrap();
        let pid = mgr.profiles[0].id;
        mgr.simulate_traffic(pid, 1000, 2000);
        let old_sent = mgr.profiles[0].total_bytes_sent;
        mgr.disconnect(0).unwrap();
        assert!(mgr.profiles[0].total_bytes_sent > old_sent);
    }

    #[test]
    fn test_manager_reconnect() {
        let mut mgr = VpnManager::new();
        mgr.connect(0).unwrap();
        assert!(mgr.reconnect(0).is_ok());
        let conn = mgr.connection_for(mgr.profiles[0].id).unwrap();
        assert_eq!(conn.status, ConnectionStatus::Connected);
    }

    #[test]
    fn test_manager_quick_connect_no_previous() {
        let mut mgr = VpnManager::new();
        assert!(mgr.quick_connect().is_err());
    }

    #[test]
    fn test_manager_quick_connect_after_connect() {
        let mut mgr = VpnManager::new();
        mgr.connect(0).unwrap();
        mgr.disconnect(0).unwrap();
        assert!(mgr.quick_connect().is_ok());
    }

    #[test]
    fn test_manager_disconnect_all() {
        let mut mgr = VpnManager::new();
        mgr.connect(0).unwrap();
        mgr.connect(1).unwrap();
        mgr.disconnect_all();
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn test_manager_active_count() {
        let mut mgr = VpnManager::new();
        assert_eq!(mgr.active_count(), 0);
        mgr.connect(0).unwrap();
        assert_eq!(mgr.active_count(), 1);
        mgr.connect(1).unwrap();
        assert_eq!(mgr.active_count(), 2);
    }

    #[test]
    fn test_manager_total_transfer() {
        let mut mgr = VpnManager::new();
        let (s, r) = mgr.total_transfer();
        assert_eq!(s, 0);
        assert_eq!(r, 0);

        mgr.connect(0).unwrap();
        let pid = mgr.profiles[0].id;
        mgr.simulate_traffic(pid, 500, 1000);
        let (s2, r2) = mgr.total_transfer();
        assert_eq!(s2, 500);
        assert_eq!(r2, 1000);
    }

    // --- Sort tests ---

    #[test]
    fn test_manager_sort_by_name() {
        let mut mgr = VpnManager::new();
        mgr.set_sort_order(SortOrder::Name);
        let names: Vec<String> = mgr.profiles.iter().map(|p| p.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_manager_sort_by_protocol() {
        let mut mgr = VpnManager::new();
        mgr.set_sort_order(SortOrder::Protocol);
        let labels: Vec<&str> = mgr.profiles.iter().map(|p| p.protocol.label()).collect();
        let mut sorted = labels.clone();
        sorted.sort_unstable();
        assert_eq!(labels, sorted);
    }

    #[test]
    fn test_manager_sort_by_status() {
        let mut mgr = VpnManager::new();
        mgr.connect(0).unwrap();
        mgr.set_sort_order(SortOrder::Status);
        // Connected profiles should be first
        let first_conn = mgr.connection_for(mgr.profiles[0].id).unwrap();
        assert_eq!(first_conn.status, ConnectionStatus::Connected);
    }

    // --- Kill switch tests ---

    #[test]
    fn test_toggle_kill_switch() {
        let mut mgr = VpnManager::new();
        let initial = mgr.profiles[0].kill_switch;
        mgr.toggle_kill_switch(0);
        assert_ne!(mgr.profiles[0].kill_switch, initial);
        mgr.toggle_kill_switch(0);
        assert_eq!(mgr.profiles[0].kill_switch, initial);
    }

    #[test]
    fn test_toggle_global_kill_switch() {
        let mut mgr = VpnManager::new();
        assert!(!mgr.global_kill_switch);
        mgr.toggle_global_kill_switch();
        assert!(mgr.global_kill_switch);
        mgr.toggle_global_kill_switch();
        assert!(!mgr.global_kill_switch);
    }

    // --- DNS tests ---

    #[test]
    fn test_add_dns_override() {
        let mut mgr = VpnManager::new();
        let initial = mgr.profiles[0].dns_override.len();
        assert!(mgr.add_dns_override(0, "8.8.4.4").is_ok());
        assert_eq!(mgr.profiles[0].dns_override.len(), initial + 1);
    }

    #[test]
    fn test_add_dns_override_invalid() {
        let mut mgr = VpnManager::new();
        assert!(mgr.add_dns_override(0, "not-an-ip").is_err());
    }

    #[test]
    fn test_add_dns_override_duplicate() {
        let mut mgr = VpnManager::new();
        if !mgr.profiles[0].dns_override.is_empty() {
            let existing = mgr.profiles[0].dns_override[0].clone();
            assert!(mgr.add_dns_override(0, &existing).is_err());
        }
    }

    #[test]
    fn test_remove_dns_override() {
        let mut mgr = VpnManager::new();
        if !mgr.profiles[0].dns_override.is_empty() {
            let initial = mgr.profiles[0].dns_override.len();
            assert!(mgr.remove_dns_override(0, 0).is_ok());
            assert_eq!(mgr.profiles[0].dns_override.len(), initial - 1);
        }
    }

    #[test]
    fn test_remove_dns_override_out_of_bounds() {
        let mut mgr = VpnManager::new();
        assert!(mgr.remove_dns_override(0, 999).is_err());
    }

    // --- Split tunnel tests ---

    #[test]
    fn test_add_allowed_ip() {
        let mut mgr = VpnManager::new();
        assert!(mgr.add_allowed_ip(0, "10.0.0.0/8").is_ok());
    }

    #[test]
    fn test_add_allowed_ip_plain() {
        let mut mgr = VpnManager::new();
        assert!(mgr.add_allowed_ip(0, "192.168.1.1").is_ok());
    }

    #[test]
    fn test_add_allowed_ip_invalid() {
        let mut mgr = VpnManager::new();
        assert!(mgr.add_allowed_ip(0, "garbage").is_err());
    }

    #[test]
    fn test_add_allowed_ip_duplicate() {
        let mut mgr = VpnManager::new();
        mgr.add_allowed_ip(0, "10.0.0.0/8").unwrap();
        assert!(mgr.add_allowed_ip(0, "10.0.0.0/8").is_err());
    }

    #[test]
    fn test_remove_allowed_ip() {
        let mut mgr = VpnManager::new();
        mgr.add_allowed_ip(0, "10.0.0.0/8").unwrap();
        let initial = mgr.profiles[0].allowed_ips.len();
        assert!(mgr.remove_allowed_ip(0, 0).is_ok());
        assert_eq!(mgr.profiles[0].allowed_ips.len(), initial - 1);
    }

    #[test]
    fn test_remove_allowed_ip_out_of_bounds() {
        let mut mgr = VpnManager::new();
        assert!(mgr.remove_allowed_ip(0, 999).is_err());
    }

    #[test]
    fn test_toggle_split_tunnel() {
        let mut mgr = VpnManager::new();
        let initial = mgr.profiles[0].split_tunnel;
        mgr.toggle_split_tunnel(0);
        assert_ne!(mgr.profiles[0].split_tunnel, initial);
    }

    // --- Toggle tests ---

    #[test]
    fn test_toggle_auto_connect() {
        let mut mgr = VpnManager::new();
        let initial = mgr.profiles[0].auto_connect;
        mgr.toggle_auto_connect(0);
        assert_ne!(mgr.profiles[0].auto_connect, initial);
    }

    #[test]
    fn test_toggle_auto_reconnect() {
        let mut mgr = VpnManager::new();
        let initial = mgr.profiles[0].auto_reconnect;
        mgr.toggle_auto_reconnect(0);
        assert_ne!(mgr.profiles[0].auto_reconnect, initial);
    }

    #[test]
    fn test_toggle_enabled_disconnects_active() {
        let mut mgr = VpnManager::new();
        mgr.connect(0).unwrap();
        mgr.toggle_enabled(0);
        assert!(!mgr.profiles[0].enabled);
        let conn = mgr.connection_for(mgr.profiles[0].id).unwrap();
        assert_eq!(conn.status, ConnectionStatus::Disconnected);
    }

    // --- Tab tests ---

    #[test]
    fn test_set_tab() {
        let mut mgr = VpnManager::new();
        mgr.set_tab(DetailTab::Log);
        assert_eq!(mgr.current_tab, DetailTab::Log);
    }

    // --- Import/Export tests ---

    #[test]
    fn test_export_all() {
        let mgr = VpnManager::new();
        let exported = mgr.export_all();
        assert!(exported.contains("[VpnProfile]"));
        for profile in &mgr.profiles {
            assert!(exported.contains(&profile.name));
        }
    }

    #[test]
    fn test_import_profile() {
        let mut mgr = VpnManager::new();
        let text = "[VpnProfile]\nname=Imported\nserver=import.vpn.com\nprotocol=SSTP\nport=443";
        let initial = mgr.profiles.len();
        let result = mgr.import_profile(text);
        assert!(result.is_ok());
        assert_eq!(mgr.profiles.len(), initial + 1);
    }

    #[test]
    fn test_import_profile_missing_name() {
        let mut mgr = VpnManager::new();
        let text = "server=import.vpn.com\nprotocol=SSTP";
        assert!(mgr.import_profile(text).is_err());
    }

    #[test]
    fn test_import_profile_missing_server() {
        let mut mgr = VpnManager::new();
        let text = "name=NoServer";
        assert!(mgr.import_profile(text).is_err());
    }

    #[test]
    fn test_import_roundtrip() {
        let mgr = VpnManager::new();
        let original = &mgr.profiles[0];
        let exported = original.export_text();
        let reimported = parse_profile_text(&exported, 999).unwrap();
        assert_eq!(reimported.name, original.name);
        assert_eq!(reimported.server_address, original.server_address);
        assert_eq!(reimported.protocol, original.protocol);
        assert_eq!(reimported.kill_switch, original.kill_switch);
        assert_eq!(reimported.auto_connect, original.auto_connect);
    }

    // --- Log tests ---

    #[test]
    fn test_log_grows_on_actions() {
        let mut mgr = VpnManager::new();
        let initial = mgr.log.len();
        mgr.connect(0).unwrap();
        assert!(mgr.log.len() > initial);
    }

    #[test]
    fn test_clear_log() {
        let mut mgr = VpnManager::new();
        assert!(!mgr.log.is_empty());
        mgr.clear_log();
        assert!(mgr.log.is_empty());
    }

    #[test]
    fn test_log_bounded() {
        let mut mgr = VpnManager::new();
        for i in 0..600 {
            mgr.connect(0).unwrap_or(());
            let _ = mgr.disconnect(0);
            let _ = i;
        }
        assert!(mgr.log.len() <= 500);
    }

    // --- Search / filter tests ---

    #[test]
    fn test_filtered_profiles_no_query() {
        let mgr = VpnManager::new();
        assert_eq!(mgr.filtered_profiles().len(), mgr.profiles.len());
    }

    #[test]
    fn test_filtered_profiles_name_match() {
        let mut mgr = VpnManager::new();
        mgr.search_query = String::from("Work");
        let filtered = mgr.filtered_profiles();
        assert!(!filtered.is_empty());
        for &idx in &filtered {
            let p = &mgr.profiles[idx];
            assert!(
                p.name.to_lowercase().contains("work")
                    || p.server_address.to_lowercase().contains("work")
                    || p.protocol.label().to_lowercase().contains("work")
            );
        }
    }

    #[test]
    fn test_filtered_profiles_protocol_match() {
        let mut mgr = VpnManager::new();
        mgr.search_query = String::from("WireGuard");
        let filtered = mgr.filtered_profiles();
        assert!(!filtered.is_empty());
        for &idx in &filtered {
            assert_eq!(mgr.profiles[idx].protocol, VpnProtocol::WireGuard);
        }
    }

    #[test]
    fn test_filtered_profiles_no_match() {
        let mut mgr = VpnManager::new();
        mgr.search_query = String::from("zzzznonexistent");
        assert!(mgr.filtered_profiles().is_empty());
    }

    // --- Edit dialog tests ---

    #[test]
    fn test_start_add_profile_dialog() {
        let mut mgr = VpnManager::new();
        mgr.start_add_profile();
        assert!(mgr.show_add_dialog);
        assert!(mgr.editing_profile.is_some());
        assert_eq!(mgr.editing_profile.as_ref().unwrap().id, 0);
    }

    #[test]
    fn test_start_edit_profile_dialog() {
        let mut mgr = VpnManager::new();
        mgr.start_edit_profile();
        assert!(mgr.show_add_dialog);
        let editing = mgr.editing_profile.as_ref().unwrap();
        assert_eq!(editing.name, mgr.profiles[0].name);
    }

    #[test]
    fn test_cancel_edit() {
        let mut mgr = VpnManager::new();
        mgr.start_add_profile();
        mgr.cancel_edit();
        assert!(!mgr.show_add_dialog);
        assert!(mgr.editing_profile.is_none());
    }

    #[test]
    fn test_confirm_edit_add() {
        let mut mgr = VpnManager::new();
        let initial = mgr.profiles.len();
        mgr.editing_profile = Some(VpnProfile::new(
            0,
            "Confirmed",
            "1.2.3.4",
            VpnProtocol::PPTP,
        ));
        mgr.show_add_dialog = true;
        assert!(mgr.confirm_edit().is_ok());
        assert_eq!(mgr.profiles.len(), initial + 1);
        assert!(!mgr.show_add_dialog);
    }

    #[test]
    fn test_confirm_edit_update() {
        let mut mgr = VpnManager::new();
        let mut edit = mgr.profiles[0].clone();
        edit.name = String::from("Edited Name");
        mgr.editing_profile = Some(edit);
        mgr.show_add_dialog = true;
        assert!(mgr.confirm_edit().is_ok());
        assert_eq!(mgr.profiles[0].name, "Edited Name");
    }

    #[test]
    fn test_confirm_edit_no_profile() {
        let mut mgr = VpnManager::new();
        assert!(mgr.confirm_edit().is_err());
    }

    // --- simulate_traffic tests ---

    #[test]
    fn test_simulate_traffic_connected() {
        let mut mgr = VpnManager::new();
        mgr.connect(0).unwrap();
        let pid = mgr.profiles[0].id;
        mgr.simulate_traffic(pid, 100, 200);
        let conn = mgr.connection_for(pid).unwrap();
        assert_eq!(conn.bytes_sent, 100);
        assert_eq!(conn.bytes_received, 200);
        assert_eq!(conn.uptime_secs, 1);
    }

    #[test]
    fn test_simulate_traffic_disconnected_no_effect() {
        let mut mgr = VpnManager::new();
        let pid = mgr.profiles[0].id;
        mgr.simulate_traffic(pid, 100, 200);
        let conn = mgr.connection_for(pid).unwrap();
        assert_eq!(conn.bytes_sent, 0);
        assert_eq!(conn.bytes_received, 0);
    }

    // --- Render tests ---

    #[test]
    fn test_render_app_produces_commands() {
        let app = VpnManager::new();
        let tree = render_app(&app);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_render_app_has_title() {
        let app = VpnManager::new();
        let tree = render_app(&app);
        let has_title = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "VPN Manager"));
        assert!(has_title);
    }

    #[test]
    fn test_render_app_has_profile_names() {
        let app = VpnManager::new();
        let tree = render_app(&app);
        let has_work = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "Work VPN"));
        assert!(has_work);
    }

    #[test]
    fn test_render_app_different_tabs() {
        let mut app = VpnManager::new();
        for tab in DetailTab::all() {
            app.set_tab(*tab);
            let tree = render_app(&app);
            assert!(
                tree.commands.len() > 10,
                "Tab {:?} produced too few commands",
                tab,
            );
        }
    }

    #[test]
    fn test_render_app_no_selection() {
        let mut app = VpnManager::new();
        app.selected_profile = None;
        let tree = render_app(&app);
        let has_placeholder = tree.commands.iter().any(
            |cmd| matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Select a VPN")),
        );
        assert!(has_placeholder);
    }

    #[test]
    fn test_render_app_with_dialog() {
        let mut app = VpnManager::new();
        app.start_add_profile();
        let tree = render_app(&app);
        let has_dialog_title = tree.commands.iter().any(
            |cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "Add VPN Profile"),
        );
        assert!(has_dialog_title);
    }

    #[test]
    fn test_render_app_connected_profile() {
        let mut app = VpnManager::new();
        app.connect(0).unwrap();
        let tree = render_app(&app);
        let has_connected = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "Connected"));
        assert!(has_connected);
    }

    #[test]
    fn test_render_global_kill_switch_visible() {
        let mut app = VpnManager::new();
        app.toggle_global_kill_switch();
        let tree = render_app(&app);
        let has_ks = tree
            .commands
            .iter()
            .any(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "KILL SW"));
        assert!(has_ks);
    }

    #[test]
    fn test_render_status_bar_shows_count() {
        let app = VpnManager::new();
        let tree = render_app(&app);
        let has_count = tree.commands.iter().any(
            |cmd| matches!(cmd, RenderCommand::Text { text, .. } if text.contains("profiles")),
        );
        assert!(has_count);
    }

    // --- Sample data tests ---

    #[test]
    fn test_sample_profiles_have_unique_ids() {
        let profiles = sample_profiles();
        let mut ids: Vec<u32> = profiles.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), profiles.len());
    }

    #[test]
    fn test_sample_profiles_all_valid() {
        let profiles = sample_profiles();
        for p in &profiles {
            assert!(p.validate().is_ok(), "Profile {} failed validation", p.name);
        }
    }

    #[test]
    fn test_sample_profiles_have_different_protocols() {
        let profiles = sample_profiles();
        let protocols: std::collections::HashSet<VpnProtocol> =
            profiles.iter().map(|p| p.protocol).collect();
        assert!(protocols.len() >= 3);
    }

    #[test]
    fn test_sample_log_nonempty() {
        let log = sample_log();
        assert!(!log.is_empty());
        for entry in &log {
            assert!(!entry.message.is_empty());
            assert!(!entry.profile_name.is_empty());
        }
    }

    // --- parse_profile_text tests ---

    #[test]
    fn test_parse_profile_text_minimal() {
        let text = "name=Test\nserver=1.2.3.4";
        let p = parse_profile_text(text, 10).unwrap();
        assert_eq!(p.name, "Test");
        assert_eq!(p.server_address, "1.2.3.4");
        assert_eq!(p.id, 10);
    }

    #[test]
    fn test_parse_profile_text_all_fields() {
        let text = "[VpnProfile]\n\
                     name=Full\n\
                     server=full.vpn.com\n\
                     port=9999\n\
                     protocol=OpenVPN\n\
                     auto_connect=true\n\
                     kill_switch=true\n\
                     split_tunnel=true\n\
                     mtu=1400\n\
                     auto_reconnect=false\n\
                     dns=1.1.1.1,8.8.8.8\n\
                     allowed_ips=10.0.0.0/8,172.16.0.0/12\n\
                     notes=Test notes";
        let p = parse_profile_text(text, 1).unwrap();
        assert_eq!(p.name, "Full");
        assert_eq!(p.server_address, "full.vpn.com");
        assert_eq!(p.port, 9999);
        assert_eq!(p.protocol, VpnProtocol::OpenVPN);
        assert!(p.auto_connect);
        assert!(p.kill_switch);
        assert!(p.split_tunnel);
        assert_eq!(p.mtu, 1400);
        assert!(!p.auto_reconnect);
        assert_eq!(p.dns_override.len(), 2);
        assert_eq!(p.allowed_ips.len(), 2);
        assert_eq!(p.notes, "Test notes");
    }

    #[test]
    fn test_parse_profile_text_empty() {
        assert!(parse_profile_text("", 1).is_err());
    }

    #[test]
    fn test_parse_profile_text_no_server() {
        assert!(parse_profile_text("name=X", 1).is_err());
    }

    #[test]
    fn test_connection_for_nonexistent() {
        let mgr = VpnManager::new();
        assert!(mgr.connection_for(99999).is_none());
    }

    // --- connection log scrolling -------------------------------------------

    /// A manager on the Log tab with `n` entries whose messages are `L000`
    /// shaped, so they are told apart from every other string in the render
    /// without depending on a pixel position. The log renders newest first,
    /// so L{n-1} is the top row and L000 the last.
    fn mgr_with_log(n: usize) -> VpnManager {
        let mut mgr = VpnManager::new();
        mgr.log.clear();
        for i in 0..n {
            mgr.log.push_back(LogEntry {
                timestamp: 1_700_000_000 + i as u64,
                profile_name: String::from("Work VPN"),
                message: format!("L{i:03}"),
                level: LogLevel::Info,
            });
        }
        mgr.set_tab(DetailTab::Log);
        mgr
    }

    fn drawn_log_rows(mgr: &VpnManager) -> Vec<String> {
        render_app(mgr)
            .commands
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. }
                    if text.len() == 4
                        && text.starts_with('L')
                        && text
                            .get(1..)
                            .is_some_and(|d| d.chars().all(|c| c.is_ascii_digit())) =>
                {
                    Some(text)
                }
                _ => None,
            })
            .collect()
    }

    /// The bug: the log stopped at a hard-coded `py + 500.0`, a number with no
    /// relation to the panel it was drawing into, and read no scroll offset at
    /// all. Everything past the cut was unreachable, not merely off-screen.
    #[test]
    fn the_log_stops_at_the_last_entry_that_fits() {
        let mgr = mgr_with_log(200);
        let drawn = drawn_log_rows(&mgr);
        assert!(!drawn.is_empty(), "the log drew no entries at all");
        assert!(drawn.len() < 200, "the log drew all 200 entries");
        assert_eq!(
            drawn.first().map(String::as_str),
            Some("L199"),
            "the log is newest first"
        );
    }

    /// No row is drawn past the bottom of the log panel, at any offset.
    #[test]
    fn no_log_row_is_drawn_past_the_bottom_of_the_panel() {
        for offset in [0, 5, 1_000] {
            let mut mgr = mgr_with_log(200);
            mgr.scroll_log_by(offset);
            for cmd in render_app(&mgr).commands {
                if let RenderCommand::Text { y, text, .. } = cmd
                    && text.len() == 4
                    && text.starts_with('L')
                {
                    assert!(
                        y + LOG_ENTRY_HEIGHT <= WINDOW_HEIGHT,
                        "log row {text:?} at y={y} overruns the window bottom \
                         {WINDOW_HEIGHT} (offset={offset})"
                    );
                }
            }
        }
    }

    /// The entries past the fold are reachable, which is the fix.
    #[test]
    fn scrolling_reaches_the_log_entries_that_did_not_fit() {
        let mut mgr = mgr_with_log(200);
        assert!(!drawn_log_rows(&mgr).contains(&String::from("L000")));
        mgr.scroll_log_by(200);
        assert!(
            drawn_log_rows(&mgr).contains(&String::from("L000")),
            "the oldest log entry is unreachable after scrolling to the end"
        );
    }

    /// An offset past the end means the last page, not a blank log.
    #[test]
    fn a_log_that_shrinks_under_a_stale_offset_shows_its_last_page() {
        let mut mgr = mgr_with_log(200);
        mgr.scroll_log_by(199);
        mgr.log.truncate(4);
        let drawn = drawn_log_rows(&mgr);
        assert_eq!(drawn.len(), 4, "the log must not go blank");
        assert_eq!(drawn.last().map(String::as_str), Some("L000"));
    }

    /// Scrolling up from the top stays at the top rather than wrapping, and
    /// clearing the log takes the offset back with it.
    #[test]
    fn scrolling_the_log_up_from_the_top_stays_at_the_top() {
        let mut mgr = mgr_with_log(200);
        mgr.scroll_log_by(-10);
        assert_eq!(mgr.log_scroll_offset, 0);
        mgr.scroll_log_by(5);
        mgr.scroll_log_to_top();
        assert_eq!(mgr.log_scroll_offset, 0);

        mgr.scroll_log_by(30);
        mgr.clear_log();
        assert_eq!(
            mgr.log_scroll_offset, 0,
            "clearing the log should not leave the view scrolled into nothing"
        );
    }

    /// A log hiding entries says how many.
    #[test]
    fn a_log_that_is_hiding_entries_says_so() {
        let mgr = mgr_with_log(200);
        let shown = drawn_log_rows(&mgr).len();
        let labels: Vec<String> = render_app(&mgr)
            .commands
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            labels.contains(&format!("{} more", 200 - shown)),
            "expected a \"{} more\" line",
            200 - shown
        );

        // ...and a log with room for everything says nothing.
        let mgr = mgr_with_log(3);
        let labels: Vec<String> = render_app(&mgr)
            .commands
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            !labels.iter().any(|t| t.ends_with(" more")),
            "a complete log should not claim to be hiding entries"
        );
    }

    // ------------------------------------------------------------------
    // Interaction
    //
    // Everything below drives the window the way a user does: it finds a
    // control by asking the *renderer* where it drew it, clicks the middle of
    // that rect, and then checks the state the click was supposed to change.
    // Nothing here recomputes a coordinate from a layout constant — a test
    // that did would keep passing after the renderer moved the control, which
    // is the one thing these exist to catch.
    // ------------------------------------------------------------------

    /// The size the tests click at. The app remembers the size it last drew
    /// at, so the tests and the window take the same path.
    const SIZE: (f32, f32) = <VpnManager as Probe>::SIZE;

    /// Finding a control by name, clicking it, and typing at it are the same
    /// four lines in every program, so they live in the toolkit — see
    /// [`guitk::probe`] for what each one guarantees. Imported under their
    /// bare names because that is what ninety tests below already say.
    use guitk::probe::{click, control_names, press, rect_of, type_str};

    /// A control and the profile field it is supposed to flip, so the
    /// toggle tests can be written once and run per row.
    type FieldCheck = (Target, fn(&VpnProfile) -> bool);

    /// The id of the profile at `index`, so a test can name a row without
    /// depending on where sorting later puts it.
    fn id_at(app: &VpnManager, index: usize) -> u32 {
        app.profiles[index].id
    }

    /// Enough profiles that the sidebar cannot show them all at once.
    fn crowded() -> VpnManager {
        let mut app = VpnManager::new();
        for i in 0..12 {
            let profile = VpnProfile::new(
                0,
                &format!("Extra {i:02}"),
                "extra.example.com",
                VpnProtocol::WireGuard,
            );
            app.add_profile(profile).expect("a valid extra profile");
        }
        app
    }

    // --- Sidebar ---

    #[test]
    fn clicking_a_sidebar_row_selects_that_profile() {
        let mut app = VpnManager::new();
        assert!(app.profiles.len() >= 3, "needs rows to tell apart");
        app.select_profile(0);

        let id = id_at(&app, 2);
        assert_eq!(click(&mut app, Target::Profile(id)), Action::Redraw);
        assert_eq!(app.selected().expect("a row click selects").id, id);

        // Clicking the row that is already selected changes nothing, so the
        // window is not repainted for it.
        assert_eq!(click(&mut app, Target::Profile(id)), Action::None);
    }

    #[test]
    fn a_sidebar_row_is_clickable_across_its_whole_painted_band() {
        let app = VpnManager::new();
        let id = id_at(&app, 1);
        let rect = rect_of(&app, Target::Profile(id)).expect("row 1 is on screen");

        // `Rect::contains` is half-open on both axes, so the far edge is not
        // inside it — half a pixel short of it is.
        for x in [rect.x, rect.x + rect.w / 2.0, rect.x + rect.w - 0.5] {
            for y in [rect.y, rect.y + rect.h / 2.0, rect.y + rect.h - 0.5] {
                assert_eq!(
                    app.hit_test(x, y, SIZE),
                    Some(Target::Profile(id)),
                    "({x}, {y}) is inside the row the renderer painted"
                );
            }
        }
    }

    #[test]
    fn a_profile_scrolled_past_the_bottom_of_the_sidebar_is_not_clickable() {
        let mut app = crowded();
        let last = app.profiles.last().expect("profiles").id;

        assert!(
            rect_of(&app, Target::Profile(last)).is_none(),
            "a row below the panel records no target, so it cannot be clicked"
        );

        app.scroll_by(20);
        assert!(
            rect_of(&app, Target::Profile(last)).is_some(),
            "scrolling to the bottom brings the last row into reach"
        );
    }

    #[test]
    fn a_half_clipped_row_is_only_clickable_where_it_is_actually_drawn() {
        let app = crowded();
        // The bottom row of the visible page is cut off by the panel's clip.
        // Find one whose recorded rect is shorter than a full row: that is the
        // clipped one, and the part below the cut must not be clickable.
        let frame = render_frame(&app, SIZE.0, SIZE.1);
        let clipped = frame
            .hits()
            .iter()
            .find(|(target, rect)| {
                matches!(target, Target::Profile(_)) && rect.h < SIDEBAR_ITEM_HEIGHT - 4.0
            })
            .map(|(target, rect)| (*target, *rect))
            .expect("a crowded sidebar cuts its bottom row off");

        let (target, rect) = clipped;
        assert_eq!(
            app.hit_test(rect.x + rect.w / 2.0, rect.bottom() - 0.5, SIZE),
            Some(target),
            "the drawn part of the row is clickable"
        );
        assert_ne!(
            app.hit_test(rect.x + rect.w / 2.0, rect.bottom() + 1.0, SIZE),
            Some(target),
            "the clipped-away part of the row is not"
        );
    }

    #[test]
    fn a_row_is_addressed_by_id_so_re_sorting_does_not_move_the_click() {
        let mut app = VpnManager::new();
        app.set_sort_order(SortOrder::Protocol);

        let ids: Vec<u32> = render_frame(&app, SIZE.0, SIZE.1)
            .hits()
            .iter()
            .filter_map(|(target, _)| match target {
                Target::Profile(id) => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), app.profiles.len(), "every profile draws a row");

        for id in ids {
            click(&mut app, Target::Profile(id));
            assert_eq!(
                app.selected().expect("a row click selects").id,
                id,
                "the row named {id} selected something else"
            );
        }
    }

    #[test]
    fn sorting_keeps_the_selection_on_the_profile_the_user_chose() {
        let mut app = VpnManager::new();
        app.set_sort_order(SortOrder::Name);

        let chosen = id_at(&app, 0);
        click(&mut app, Target::Profile(chosen));
        let row_before = app.selected_profile;

        // Name -> Status -> Protocol. The last of those genuinely reorders the
        // sample list, which is what makes this test able to fail.
        click(&mut app, Target::CycleSort);
        click(&mut app, Target::CycleSort);

        assert_eq!(app.sort_order, SortOrder::Protocol);
        assert_ne!(
            app.selected_profile, row_before,
            "the sample list must actually move, or this proves nothing"
        );
        assert_eq!(
            app.selected().expect("something is still selected").id,
            chosen,
            "sorting moved the selection to a different profile"
        );
    }

    #[test]
    fn the_search_box_takes_the_keyboard_and_filters_as_it_is_typed() {
        let mut app = VpnManager::new();
        assert_eq!(
            click(&mut app, Target::Focus(Field::Search)),
            Action::Redraw
        );
        assert_eq!(app.focus, Some(Field::Search));

        type_str(&mut app, "gam");
        assert_eq!(app.search_query, "gam");

        let shown: Vec<&str> = app
            .filtered_profiles()
            .iter()
            .map(|i| app.profiles[*i].name.as_str())
            .collect();
        assert_eq!(shown, vec!["Gaming VPN"]);

        let rows = control_names(&app)
            .iter()
            .filter(|n| n.as_str() == "Profile")
            .count();
        assert_eq!(rows, 1, "the sidebar draws only the rows that match");

        // Backspace widens the filter again — "ga" also matches "Legacy
        // Office", so the search is a substring match anywhere in the name and
        // not a prefix one.
        app.handle_key(&press(Key::Backspace));
        assert_eq!(app.search_query, "ga");
        let widened: Vec<&str> = app
            .filtered_profiles()
            .iter()
            .map(|i| app.profiles[*i].name.as_str())
            .collect();
        assert!(widened.contains(&"Gaming VPN"));
        assert!(widened.contains(&"Legacy Office"));
    }

    // --- Toolbar ---

    #[test]
    fn every_toolbar_button_is_on_screen_and_none_overlap() {
        let app = VpnManager::new();
        let frame = render_frame(&app, SIZE.0, SIZE.1);

        let mut boxes: Vec<(&str, Rect)> = Vec::new();
        for &(label, _, target) in TOOLBAR_BUTTONS {
            let rect = frame
                .rect_of(|t| *t == target)
                .unwrap_or_else(|| panic!("{label} is not on screen"));
            assert!(rect.right() <= SIZE.0, "{label} runs off the window");
            boxes.push((label, rect));
        }

        for (i, (a_label, a)) in boxes.iter().enumerate() {
            for (b_label, b) in boxes.iter().skip(i + 1) {
                assert!(
                    a.intersect(*b).is_none_or(Rect::is_empty),
                    "{a_label} overlaps {b_label}: a click would hit whichever was drawn last"
                );
            }
        }
    }

    #[test]
    fn the_sort_control_cycles_through_every_order() {
        let mut app = VpnManager::new();
        let start = app.sort_order;

        let mut seen = vec![start];
        for _ in 0..SortOrder::all().len() {
            click(&mut app, Target::CycleSort);
            seen.push(app.sort_order);
        }

        assert_eq!(
            seen.last().copied(),
            Some(start),
            "a full lap must come back to where it started"
        );
        for order in SortOrder::all() {
            assert!(seen.contains(order), "{order:?} is unreachable");
        }
        assert!(app.status_message.contains("Sorted by"));
    }

    #[test]
    fn the_global_kill_switch_badge_can_turn_itself_both_ways() {
        let mut app = VpnManager::new();
        assert!(!app.global_kill_switch);

        // The badge used to be drawn only while the switch was *on*, which left
        // no way to turn it on. Both directions, from the same rect.
        click(&mut app, Target::ToggleGlobalKillSwitch);
        assert!(app.global_kill_switch);
        assert_eq!(app.status_message, "Global kill switch on");

        click(&mut app, Target::ToggleGlobalKillSwitch);
        assert!(!app.global_kill_switch);
        assert_eq!(app.status_message, "Global kill switch off");
    }

    #[test]
    fn connect_says_why_it_refused() {
        let mut app = VpnManager::new();
        let disabled = app
            .profiles
            .iter()
            .position(|p| !p.enabled)
            .expect("the sample data has a disabled profile");
        let id = id_at(&app, disabled);
        click(&mut app, Target::Profile(id));

        click(&mut app, Target::ConnectSelected);
        assert_eq!(app.status_message, "Profile is disabled");
        assert!(
            !app.connection_for(id)
                .expect("a connection row")
                .status
                .is_active(),
            "a refused Connect must not have connected anything"
        );
    }

    #[test]
    fn connect_with_nothing_selected_asks_for_a_selection() {
        let mut app = VpnManager::new();
        app.selected_profile = None;
        assert_eq!(click(&mut app, Target::ConnectSelected), Action::Redraw);
        assert_eq!(app.status_message, "Select a profile first");
    }

    #[test]
    fn connect_then_disconnect_moves_the_selected_profile_and_says_so() {
        let mut app = VpnManager::new();
        let id = id_at(&app, 0);
        click(&mut app, Target::Profile(id));

        click(&mut app, Target::ConnectSelected);
        assert!(
            app.connection_for(id)
                .expect("a connection row")
                .status
                .is_active(),
            "Connect must actually connect the selected profile"
        );
        assert!(app.status_message.starts_with("Connected to "));

        click(&mut app, Target::DisconnectSelected);
        assert!(
            !app.connection_for(id)
                .expect("a connection row")
                .status
                .is_active()
        );
        assert!(app.status_message.starts_with("Disconnected from "));
    }

    // --- Detail panel ---

    #[test]
    fn clicking_a_tab_switches_the_panel() {
        let mut app = VpnManager::new();
        for tab in DetailTab::all() {
            if app.current_tab == *tab {
                assert_eq!(
                    click(&mut app, Target::Tab(*tab)),
                    Action::None,
                    "re-clicking the open tab changes nothing"
                );
            } else {
                assert_eq!(click(&mut app, Target::Tab(*tab)), Action::Redraw);
            }
            assert_eq!(app.current_tab, *tab);
        }
    }

    #[test]
    fn every_overview_toggle_flips_the_field_it_names() {
        let checks: [FieldCheck; 5] = [
            (Target::ToggleEnabled, |p| p.enabled),
            (Target::ToggleAutoConnect, |p| p.auto_connect),
            (Target::ToggleAutoReconnect, |p| p.auto_reconnect),
            (Target::ToggleKillSwitch, |p| p.kill_switch),
            (Target::ToggleSplitTunnel, |p| p.split_tunnel),
        ];

        for (target, read) in checks {
            let mut app = VpnManager::new();
            app.set_tab(DetailTab::Overview);
            app.select_profile(0);
            let before = read(app.selected().expect("a selection"));

            assert_eq!(click(&mut app, target), Action::Redraw);
            assert_ne!(
                read(app.selected().expect("a selection")),
                before,
                "{target:?} did not flip the field it is labelled with"
            );

            click(&mut app, target);
            assert_eq!(
                read(app.selected().expect("a selection")),
                before,
                "{target:?} does not toggle back"
            );
        }
    }

    #[test]
    fn the_edit_button_opens_the_dialog_on_the_selected_profile() {
        let mut app = VpnManager::new();
        app.select_profile(1);
        let name = app.selected().expect("a selection").name.clone();

        click(&mut app, Target::EditProfile);
        assert!(app.show_add_dialog);
        assert_eq!(
            app.editing_profile
                .as_ref()
                .expect("a profile in the dialog")
                .name,
            name
        );
        assert_eq!(app.focus, Some(Field::Name));
    }

    #[test]
    fn reconnect_is_only_offered_once_there_is_a_connection_to_reconnect() {
        let mut app = VpnManager::new();
        app.select_profile(0);
        app.set_tab(DetailTab::Connection);

        assert!(
            rect_of(&app, Target::ReconnectSelected).is_none(),
            "nothing to reconnect while disconnected"
        );

        click(&mut app, Target::ConnectSelected);
        assert!(rect_of(&app, Target::ReconnectSelected).is_some());

        click(&mut app, Target::ReconnectSelected);
        assert!(app.status_message.starts_with("Reconnected to "));
    }

    // --- Split tunnel tab ---

    #[test]
    fn the_split_tunnel_box_adds_a_range_and_empties_itself() {
        let mut app = VpnManager::new();
        app.select_profile(0);
        app.set_tab(DetailTab::SplitTunnel);
        let before = app.selected().expect("a selection").allowed_ips.len();

        assert_eq!(
            click(&mut app, Target::Focus(Field::AllowedIp)),
            Action::Redraw
        );
        assert_eq!(app.focus, Some(Field::AllowedIp));
        type_str(&mut app, "192.168.5.0/24");
        assert_eq!(app.allowed_ip_input, "192.168.5.0/24");

        click(&mut app, Target::AddAllowedIp);
        assert_eq!(
            app.selected().expect("a selection").allowed_ips.len(),
            before + 1
        );
        assert!(
            app.selected()
                .expect("a selection")
                .allowed_ips
                .contains(&String::from("192.168.5.0/24"))
        );
        assert!(
            app.allowed_ip_input.is_empty(),
            "an accepted range must leave the box empty for the next one"
        );
        assert!(app.status_message.contains("192.168.5.0/24"));
    }

    #[test]
    fn a_rejected_range_is_reported_and_left_in_the_box_to_fix() {
        let mut app = VpnManager::new();
        app.select_profile(0);
        app.set_tab(DetailTab::SplitTunnel);
        let before = app.selected().expect("a selection").allowed_ips.len();

        click(&mut app, Target::Focus(Field::AllowedIp));
        type_str(&mut app, "not-an-address");
        click(&mut app, Target::AddAllowedIp);

        assert_eq!(
            app.selected().expect("a selection").allowed_ips.len(),
            before
        );
        assert_eq!(
            app.allowed_ip_input, "not-an-address",
            "a rejected range must stay in the box: retyping it is the user's work, not ours"
        );
        assert!(app.status_message.contains("not-an-address"));
    }

    #[test]
    fn the_add_range_button_says_the_box_is_empty_rather_than_doing_nothing() {
        let mut app = VpnManager::new();
        app.select_profile(0);
        app.set_tab(DetailTab::SplitTunnel);

        click(&mut app, Target::AddAllowedIp);
        assert_eq!(app.status_message, "Type an IP or CIDR range first");
    }

    #[test]
    fn removing_a_range_removes_the_one_that_was_clicked() {
        let mut app = VpnManager::new();
        let index = app
            .profiles
            .iter()
            .position(|p| p.allowed_ips.len() >= 2)
            .expect("the sample data has a profile with two ranges");
        app.select_profile(index);
        app.set_tab(DetailTab::SplitTunnel);

        let doomed = app.profiles[index].allowed_ips[1].clone();
        let survivor = app.profiles[index].allowed_ips[0].clone();

        click(&mut app, Target::RemoveAllowedIp(1));
        assert!(!app.profiles[index].allowed_ips.contains(&doomed));
        assert!(app.profiles[index].allowed_ips.contains(&survivor));
    }

    // --- Protocol tab ---

    #[test]
    fn the_compression_row_is_only_offered_for_openvpn() {
        let mut app = VpnManager::new();
        app.set_tab(DetailTab::ProtocolConfig);

        let openvpn = app
            .profiles
            .iter()
            .position(|p| matches!(p.protocol_settings, ProtocolSettings::OpenVpn { .. }))
            .expect("the sample data has an OpenVPN profile");
        let other = app
            .profiles
            .iter()
            .position(|p| !matches!(p.protocol_settings, ProtocolSettings::OpenVpn { .. }))
            .expect("the sample data has a non-OpenVPN profile");

        app.select_profile(other);
        assert!(
            rect_of(&app, Target::ToggleCompression).is_none(),
            "compression is not a setting a WireGuard profile has"
        );

        app.select_profile(openvpn);
        let before = matches!(
            app.profiles[openvpn].protocol_settings,
            ProtocolSettings::OpenVpn {
                compression: true,
                ..
            }
        );
        click(&mut app, Target::ToggleCompression);
        let after = matches!(
            app.profiles[openvpn].protocol_settings,
            ProtocolSettings::OpenVpn {
                compression: true,
                ..
            }
        );
        assert_ne!(before, after);
        assert_eq!(app.status_message, "Compression toggled");
    }

    // --- Log tab ---

    #[test]
    fn clearing_the_log_empties_it() {
        let mut app = VpnManager::new();
        app.set_tab(DetailTab::Log);
        assert!(!app.log.is_empty());

        click(&mut app, Target::ClearLog);
        assert!(app.log.is_empty());
        assert_eq!(app.log_scroll_offset, 0);
        assert_eq!(app.status_message, "Log cleared");
    }

    // --- The add/edit dialog ---

    #[test]
    fn the_dialog_swallows_clicks_meant_for_the_window_behind_it() {
        let mut app = VpnManager::new();
        let add = rect_of(&app, Target::AddProfile).expect("the Add button");
        let (ax, ay) = add.centre();
        let before = app.profiles.len();

        click(&mut app, Target::AddProfile);
        assert!(app.show_add_dialog);

        assert_ne!(
            app.hit_test(ax, ay, SIZE),
            Some(Target::AddProfile),
            "a modal that only looks in front is not modal"
        );
        app.handle_click(ax, ay, MouseButton::Left, SIZE);
        assert_eq!(app.profiles.len(), before);
        assert!(app.show_add_dialog, "the dialog is still up");
    }

    #[test]
    fn typing_a_name_and_a_server_into_the_dialog_adds_a_profile() {
        let mut app = VpnManager::new();
        let before = app.profiles.len();

        click(&mut app, Target::AddProfile);
        assert_eq!(
            app.focus,
            Some(Field::Name),
            "the caret starts where it must"
        );
        type_str(&mut app, "Home Lab");

        click(&mut app, Target::Focus(Field::Server));
        assert_eq!(app.focus, Some(Field::Server));
        type_str(&mut app, "lab.example.net");

        click(&mut app, Target::DialogSave);
        assert!(!app.show_add_dialog, "a good Save closes the dialog");
        assert_eq!(app.focus, None);
        assert_eq!(app.profiles.len(), before + 1);
        let added = app
            .profiles
            .iter()
            .find(|p| p.name == "Home Lab")
            .expect("the profile that was typed in");
        assert_eq!(added.server_address, "lab.example.net");
        assert_eq!(app.status_message, "Profile saved");
    }

    #[test]
    fn a_rejected_save_keeps_the_dialog_up_with_everything_typed_into_it() {
        let mut app = VpnManager::new();
        let before = app.profiles.len();

        click(&mut app, Target::AddProfile);
        type_str(&mut app, "Half Filled");
        // No server address, which `validate` refuses.
        click(&mut app, Target::DialogSave);

        assert!(
            app.show_add_dialog,
            "a refused Save must not throw away the rest of the form"
        );
        assert_eq!(
            app.editing_profile.as_ref().expect("still editing").name,
            "Half Filled"
        );
        assert_eq!(app.profiles.len(), before);
        assert_eq!(app.dialog_error, "Server address is required");

        // The complaint has to be inside the dialog: the status bar is behind
        // the scrim the dialog drew over the window.
        let drawn: Vec<String> = render_frame(&app, SIZE.0, SIZE.1)
            .into_tree()
            .commands
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            drawn.contains(&String::from("Server address is required")),
            "the reason the Save failed is not on screen"
        );

        // Answering the complaint clears it.
        click(&mut app, Target::Focus(Field::Server));
        type_str(&mut app, "s.example.com");
        assert!(app.dialog_error.is_empty());

        click(&mut app, Target::DialogSave);
        assert!(!app.show_add_dialog);
        assert_eq!(app.profiles.len(), before + 1);
    }

    #[test]
    fn the_port_box_takes_digits_and_ignores_everything_else() {
        let mut app = VpnManager::new();
        click(&mut app, Target::AddProfile);
        click(&mut app, Target::Focus(Field::Port));

        // Clear whatever the protocol's default put there.
        for _ in 0..6 {
            app.handle_key(&press(Key::Backspace));
        }
        assert_eq!(app.editing_profile.as_ref().expect("editing").port, 0);

        type_str(&mut app, "4a4b3");
        assert_eq!(
            app.editing_profile.as_ref().expect("editing").port,
            443,
            "letters in a port box are not an edit"
        );

        app.handle_key(&press(Key::Backspace));
        assert_eq!(app.editing_profile.as_ref().expect("editing").port, 44);
    }

    #[test]
    fn the_port_box_saturates_rather_than_wrapping_back_to_a_working_port() {
        let mut app = VpnManager::new();
        click(&mut app, Target::AddProfile);
        click(&mut app, Target::Focus(Field::Port));
        for _ in 0..6 {
            app.handle_key(&press(Key::Backspace));
        }

        type_str(&mut app, "99999999");
        assert_eq!(
            app.editing_profile.as_ref().expect("editing").port,
            u16::MAX,
            "holding a digit key must end at 65535, not roll through zero"
        );
    }

    #[test]
    fn cycling_the_protocol_moves_the_port_and_the_settings_with_it() {
        let mut app = VpnManager::new();
        click(&mut app, Target::AddProfile);

        let mut seen = Vec::new();
        for _ in 0..VpnProtocol::all().len() {
            let profile = app.editing_profile.as_ref().expect("editing");
            seen.push(profile.protocol);
            assert_eq!(
                profile.port,
                profile.protocol.default_port(),
                "a {:?} profile carrying another protocol's port is not usable",
                profile.protocol
            );
            click(&mut app, Target::CycleProtocol);
        }

        for protocol in VpnProtocol::all() {
            assert!(seen.contains(protocol), "{protocol:?} is unreachable");
        }
        assert_eq!(
            app.editing_profile.as_ref().expect("editing").protocol,
            seen[0],
            "a full lap comes back to where it started"
        );
    }

    #[test]
    fn cycling_the_auth_method_reaches_every_kind_and_wraps() {
        let mut app = VpnManager::new();
        click(&mut app, Target::AddProfile);

        let start = app
            .editing_profile
            .as_ref()
            .expect("editing")
            .auth_method
            .label();
        let mut seen = vec![start];
        for _ in 0..4 {
            click(&mut app, Target::CycleAuth);
            seen.push(
                app.editing_profile
                    .as_ref()
                    .expect("editing")
                    .auth_method
                    .label(),
            );
        }

        assert_eq!(
            seen.last().copied(),
            Some(start),
            "four steps is a full lap"
        );
        let mut kinds = seen.clone();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(
            kinds.len(),
            4,
            "every auth kind must be reachable: {seen:?}"
        );
    }

    #[test]
    fn cancel_closes_the_dialog_and_adds_nothing() {
        let mut app = VpnManager::new();
        let before = app.profiles.len();

        click(&mut app, Target::AddProfile);
        type_str(&mut app, "Discarded");
        click(&mut app, Target::DialogCancel);

        assert!(!app.show_add_dialog);
        assert!(app.editing_profile.is_none());
        assert_eq!(app.focus, None);
        assert_eq!(app.profiles.len(), before);
        assert_eq!(app.status_message, "Cancelled");
    }

    #[test]
    fn the_dialog_toggles_change_the_profile_being_edited_not_the_selected_one() {
        let checks: [FieldCheck; 3] = [
            (Target::DialogToggleKillSwitch, |p| p.kill_switch),
            (Target::DialogToggleAutoConnect, |p| p.auto_connect),
            (Target::DialogToggleSplitTunnel, |p| p.split_tunnel),
        ];

        for (target, read) in checks {
            let mut app = VpnManager::new();
            app.select_profile(0);
            let selected_before = read(app.selected().expect("a selection"));
            click(&mut app, Target::AddProfile);

            let before = read(app.editing_profile.as_ref().expect("editing"));
            assert_eq!(click(&mut app, target), Action::Redraw);
            assert_ne!(
                read(app.editing_profile.as_ref().expect("editing")),
                before,
                "{target:?} did not flip the field it is labelled with"
            );
            assert_eq!(
                read(app.selected().expect("a selection")),
                selected_before,
                "{target:?} reached past the dialog to the selected profile"
            );
        }
    }

    #[test]
    fn edit_loads_the_selected_profile_and_saving_updates_it_in_place() {
        let mut app = VpnManager::new();
        app.select_profile(1);
        let id = app.selected().expect("a selection").id;
        let count = app.profiles.len();

        click(&mut app, Target::EditProfile);
        click(&mut app, Target::Focus(Field::Name));
        type_str(&mut app, " II");
        click(&mut app, Target::DialogSave);

        assert_eq!(
            app.profiles.len(),
            count,
            "editing a profile must not add a second copy of it"
        );
        let edited = app
            .profiles
            .iter()
            .find(|p| p.id == id)
            .expect("the profile that was edited");
        assert!(edited.name.ends_with(" II"));
    }

    // --- Keyboard ---

    #[test]
    fn the_arrow_keys_walk_the_filtered_list_not_the_whole_one() {
        let mut app = VpnManager::new();
        click(&mut app, Target::Focus(Field::Search));
        type_str(&mut app, "vpn");
        app.handle_key(&press(Key::Escape));
        assert_eq!(app.focus, None, "Escape leaves the box before the program");

        let visible: Vec<usize> = app.filtered_profiles();
        assert!(
            visible.len() < app.profiles.len(),
            "the search must actually hide something"
        );

        app.select_profile(visible[0]);
        for expected in visible.iter().skip(1) {
            assert_eq!(app.handle_key(&press(Key::Down)), Action::Redraw);
            assert_eq!(
                app.selected_profile,
                Some(*expected),
                "Down walked onto a row the sidebar is not drawing"
            );
        }
        // And it stops at the end rather than wrapping onto a hidden row.
        assert_eq!(app.handle_key(&press(Key::Down)), Action::None);
        assert_eq!(app.selected_profile, visible.last().copied());
    }

    #[test]
    fn arrowing_past_the_bottom_row_scrolls_it_into_view() {
        let mut app = crowded();
        app.select_profile(0);
        app.scroll_to_top();

        let last = app.profiles.last().expect("profiles").id;
        for _ in 0..app.profiles.len() {
            app.handle_key(&press(Key::Down));
        }

        assert_eq!(app.selected().expect("a selection").id, last);
        assert!(
            app.scroll_offset > 0.0,
            "the view has to follow the selection off the bottom"
        );
        assert!(
            rect_of(&app, Target::Profile(last)).is_some(),
            "the selected row must be one the user can see"
        );
    }

    #[test]
    fn the_arrow_keys_walk_the_tab_strip_and_wrap_both_ways() {
        let mut app = VpnManager::new();
        let tabs = DetailTab::all();

        app.set_tab(tabs[0]);
        for expected in tabs.iter().skip(1) {
            app.handle_key(&press(Key::Right));
            assert_eq!(app.current_tab, *expected);
        }
        app.handle_key(&press(Key::Right));
        assert_eq!(app.current_tab, tabs[0], "Right wraps at the end");

        app.handle_key(&press(Key::Left));
        assert_eq!(
            app.current_tab,
            *tabs.last().expect("tabs"),
            "Left wraps at the start"
        );
    }

    #[test]
    fn escape_leaves_a_field_before_it_leaves_the_program() {
        let mut app = VpnManager::new();
        click(&mut app, Target::Focus(Field::Search));

        assert_eq!(app.handle_key(&press(Key::Escape)), Action::Redraw);
        assert_eq!(app.focus, None);
        assert_eq!(
            app.handle_key(&press(Key::Escape)),
            Action::Quit,
            "with nothing focused, Escape closes the window"
        );
    }

    #[test]
    fn escape_closes_the_dialog_rather_than_the_window() {
        let mut app = VpnManager::new();
        click(&mut app, Target::AddProfile);
        // The caret is in Name, so the first Escape only leaves the field.
        app.handle_key(&press(Key::Escape));
        assert!(app.show_add_dialog);

        assert_eq!(app.handle_key(&press(Key::Escape)), Action::Redraw);
        assert!(!app.show_add_dialog);
        assert_eq!(
            app.handle_key(&press(Key::Escape)),
            Action::Quit,
            "once the dialog is gone Escape means what it always did"
        );
    }

    #[test]
    fn the_arrow_keys_do_not_reach_past_an_open_dialog() {
        let mut app = VpnManager::new();
        app.select_profile(0);
        click(&mut app, Target::AddProfile);
        app.focus = None;

        assert_eq!(app.handle_key(&press(Key::Down)), Action::None);
        assert_eq!(app.selected_profile, Some(0));
        assert_eq!(app.handle_key(&press(Key::Right)), Action::None);
        assert_eq!(app.current_tab, DetailTab::Overview);
    }

    #[test]
    fn tab_walks_the_dialog_fields_in_a_ring() {
        let mut app = VpnManager::new();
        click(&mut app, Target::AddProfile);

        let ring = [Field::Name, Field::Server, Field::Port, Field::Mtu];
        assert_eq!(app.focus, Some(ring[0]));
        for expected in ring.iter().skip(1) {
            app.handle_key(&press(Key::Tab));
            assert_eq!(app.focus, Some(*expected));
        }
        app.handle_key(&press(Key::Tab));
        assert_eq!(app.focus, Some(ring[0]), "the ring closes");
    }

    #[test]
    fn a_dialog_field_cannot_hold_the_keyboard_while_the_dialog_is_closed() {
        let mut app = VpnManager::new();
        assert_eq!(app.focus_field(Field::Name), Action::None);
        assert_eq!(app.focus, None);

        click(&mut app, Target::AddProfile);
        assert_eq!(
            app.focus_field(Field::Search),
            Action::None,
            "and the window's own boxes cannot hold it while the dialog is up"
        );
        assert_eq!(app.focus, Some(Field::Name));
    }

    #[test]
    fn enter_in_the_split_tunnel_box_commits_the_range() {
        let mut app = VpnManager::new();
        app.select_profile(0);
        app.set_tab(DetailTab::SplitTunnel);
        click(&mut app, Target::Focus(Field::AllowedIp));
        type_str(&mut app, "10.9.0.0/16");

        app.handle_key(&press(Key::Enter));
        assert!(
            app.selected()
                .expect("a selection")
                .allowed_ips
                .contains(&String::from("10.9.0.0/16"))
        );
    }

    // --- Mouse edges ---

    #[test]
    fn clicking_the_background_puts_the_caret_away() {
        let mut app = VpnManager::new();
        click(&mut app, Target::Focus(Field::Search));
        assert_eq!(app.focus, Some(Field::Search));

        // The status bar draws no controls, so this is bare background.
        let bare = (SIZE.0 / 2.0, SIZE.1 - STATUS_BAR_HEIGHT / 2.0);
        assert!(app.hit_test(bare.0, bare.1, SIZE).is_none());

        assert_eq!(
            app.handle_click(bare.0, bare.1, MouseButton::Left, SIZE),
            Action::Redraw
        );
        assert_eq!(app.focus, None);
        assert_eq!(
            app.handle_click(bare.0, bare.1, MouseButton::Left, SIZE),
            Action::None,
            "with no caret to put away, background is inert"
        );
    }

    #[test]
    fn a_right_click_does_nothing_at_all() {
        let mut app = VpnManager::new();
        app.select_profile(0);
        let id = id_at(&app, 2);
        let rect = rect_of(&app, Target::Profile(id)).expect("row 2");
        let (cx, cy) = rect.centre();

        assert_eq!(
            app.handle_click(cx, cy, MouseButton::Right, SIZE),
            Action::None
        );
        assert_eq!(app.selected_profile, Some(0));
    }

    #[test]
    fn the_wheel_over_the_log_scrolls_the_log_and_over_the_sidebar_the_list() {
        let mut app = crowded();
        app.set_tab(DetailTab::Log);

        // Over the detail panel with the log open: the log moves.
        app.handle_event(
            &Event::Mouse(MouseEvent {
                x: SIDEBAR_WIDTH + 200.0,
                y: 400.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
            }),
            SIZE,
        );
        assert_eq!(app.log_scroll_offset, wheel::ROWS_PER_NOTCH as usize);
        assert!(
            app.scroll_offset.abs() < f32::EPSILON,
            "the profile list stayed put"
        );

        // Over the sidebar: the profile list moves.
        app.handle_event(
            &Event::Mouse(MouseEvent {
                x: SIDEBAR_WIDTH / 2.0,
                y: 400.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
            }),
            SIZE,
        );
        assert!(app.scroll_offset > 0.0);
        assert_eq!(
            app.log_scroll_offset,
            wheel::ROWS_PER_NOTCH as usize,
            "the log stayed put"
        );
    }

    #[test]
    fn the_sidebar_cannot_be_scrolled_past_either_end() {
        let mut app = crowded();
        let max = app.max_scroll();
        assert!(max > 0.0, "the fixture has to overflow the panel");

        app.scroll_by(1000);
        assert!(
            (app.scroll_offset - max).abs() < f32::EPSILON,
            "scrolled to {} but the end is {max}",
            app.scroll_offset
        );

        app.scroll_by(-1000);
        assert!((app.scroll_offset).abs() < f32::EPSILON);
    }

    // --- The clock ---

    #[test]
    fn the_clock_runs_only_while_something_is_connected() {
        let mut app = VpnManager::new();
        assert_eq!(
            app.tick_interval(),
            None,
            "an app that ticks with nothing to age holds the desktop awake"
        );

        app.connect(0).expect("sample profile 0 is enabled");
        assert_eq!(app.tick_interval(), Some(Duration::from_secs(1)));

        app.disconnect(0).expect("it was just connected");
        assert_eq!(app.tick_interval(), None);
    }

    #[test]
    fn a_tick_ages_a_live_connection_and_leaves_a_dead_one_alone() {
        let mut app = VpnManager::new();
        let live = id_at(&app, 0);
        let dead = id_at(&app, 1);
        app.connect(0).expect("sample profile 0 is enabled");

        assert_eq!(
            app.handle_event(&Event::Tick { elapsed_ms: 3000 }, SIZE),
            Action::Redraw
        );
        assert_eq!(app.connection_for(live).expect("live").uptime_secs, 3);
        assert_eq!(app.connection_for(dead).expect("dead").uptime_secs, 0);

        // And only the clock moves: nothing here invents traffic that did not
        // happen.
        assert_eq!(app.connection_for(live).expect("live").bytes_sent, 0);
        assert_eq!(app.connection_for(live).expect("live").bytes_received, 0);
    }

    #[test]
    fn sub_second_ticks_accumulate_instead_of_rounding_to_nothing() {
        let mut app = VpnManager::new();
        let live = id_at(&app, 0);
        app.connect(0).expect("sample profile 0 is enabled");

        for _ in 0..3 {
            assert!(!app.advance(250), "a quarter second is not a second yet");
        }
        assert!(app.advance(250), "four quarters are");
        assert_eq!(app.connection_for(live).expect("live").uptime_secs, 1);
    }

    #[test]
    fn a_tick_with_nothing_connected_does_not_ask_for_a_repaint() {
        let mut app = VpnManager::new();
        assert_eq!(
            app.handle_event(&Event::Tick { elapsed_ms: 5000 }, SIZE),
            Action::None
        );
    }

    // --- Import / export ---

    #[test]
    fn an_exported_file_splits_back_into_the_profiles_it_was_made_from() {
        let app = VpnManager::new();
        let text = app.export_all();
        let blocks = split_profile_blocks(&text);
        assert_eq!(
            blocks.len(),
            app.profiles.len(),
            "the whole file handed to `parse_profile_text` folds into one profile"
        );

        let mut fresh = VpnManager::new();
        fresh.profiles.clear();
        fresh.connections.clear();
        for block in &blocks {
            fresh
                .import_profile(block)
                .expect("a block we just exported");
        }

        assert_eq!(fresh.profiles.len(), app.profiles.len());
        for original in &app.profiles {
            let round_tripped = fresh
                .profiles
                .iter()
                .find(|p| p.name == original.name)
                .unwrap_or_else(|| panic!("{} did not survive the round trip", original.name));
            assert_eq!(round_tripped.server_address, original.server_address);
            assert_eq!(round_tripped.port, original.port);
            assert_eq!(round_tripped.protocol, original.protocol);
        }
    }

    #[test]
    fn splitting_an_empty_or_headerless_file_invents_nothing() {
        assert!(split_profile_blocks("").is_empty());
        assert!(split_profile_blocks("  \n\n \n").is_empty());
        assert_eq!(
            split_profile_blocks("name=X\nserver=Y\n").len(),
            1,
            "a file with no header is still one profile's worth of keys"
        );
    }

    #[test]
    fn the_export_path_is_under_the_home_directory_the_user_actually_has() {
        // `$HOME` is not guaranteed on every host, so the claim is conditional:
        // whatever it returns must be the documented path, and it must refuse
        // rather than guess when there is no home directory.
        assert!(profile_file().is_none_or(|path| path.ends_with(PROFILE_FILE)));
    }

    // --- Coverage ---

    #[test]
    fn every_control_the_program_knows_about_is_reachable_from_some_screen() {
        let mut seen: Vec<String> = Vec::new();
        let note = |app: &VpnManager, seen: &mut Vec<String>| {
            for name in control_names(app) {
                if !seen.contains(&name) {
                    seen.push(name);
                }
            }
        };

        let mut app = VpnManager::new();
        app.select_profile(0);
        note(&app, &mut seen);

        app.set_tab(DetailTab::Connection);
        note(&app, &mut seen);
        app.connect(0).expect("sample profile 0 is enabled");
        note(&app, &mut seen);

        let split = app
            .profiles
            .iter()
            .position(|p| !p.allowed_ips.is_empty())
            .expect("a profile with a range");
        app.select_profile(split);
        app.set_tab(DetailTab::SplitTunnel);
        note(&app, &mut seen);

        let openvpn = app
            .profiles
            .iter()
            .position(|p| matches!(p.protocol_settings, ProtocolSettings::OpenVpn { .. }))
            .expect("an OpenVPN profile");
        app.select_profile(openvpn);
        app.set_tab(DetailTab::ProtocolConfig);
        note(&app, &mut seen);

        app.set_tab(DetailTab::Log);
        note(&app, &mut seen);

        app.set_tab(DetailTab::Stats);
        note(&app, &mut seen);

        app.start_add_profile();
        note(&app, &mut seen);

        // One name per `Target` variant. A variant added without a control to
        // reach it — which is how this window came to have a kill switch that
        // could not be switched on — fails here.
        let expected = [
            "ToggleGlobalKillSwitch",
            "AddProfile",
            "RemoveProfile",
            "ConnectSelected",
            "DisconnectSelected",
            "QuickConnect",
            "Import",
            "Export",
            "CycleSort",
            "Profile",
            "Focus",
            "Tab",
            "EditProfile",
            "ReconnectSelected",
            "ToggleEnabled",
            "ToggleAutoConnect",
            "ToggleAutoReconnect",
            "ToggleKillSwitch",
            "ToggleSplitTunnel",
            "ToggleCompression",
            "RemoveAllowedIp",
            "AddAllowedIp",
            "ClearLog",
            "CycleProtocol",
            "CycleAuth",
            "DialogToggleKillSwitch",
            "DialogToggleAutoConnect",
            "DialogToggleSplitTunnel",
            "DialogSave",
            "DialogCancel",
        ];
        for name in expected {
            assert!(
                seen.iter().any(|s| s == name),
                "{name} is drawn nowhere the user can click it"
            );
        }
    }

    #[test]
    fn the_frame_balances_its_clips_in_every_state() {
        let mut app = VpnManager::new();
        for tab in DetailTab::all() {
            app.set_tab(*tab);
            assert!(
                render_frame(&app, SIZE.0, SIZE.1).is_balanced(),
                "{tab:?} left a clip or a translation on the stack"
            );
        }
        app.start_add_profile();
        assert!(render_frame(&app, SIZE.0, SIZE.1).is_balanced());

        app.selected_profile = None;
        assert!(render_frame(&app, SIZE.0, SIZE.1).is_balanced());
    }

    #[test]
    fn the_window_is_drawn_at_the_size_it_is_handed_not_the_one_it_remembers() {
        let mut app = VpnManager::new();
        // The first frame goes out before any `Event::Resize`, so `render` has
        // to believe its arguments.
        let tree = app.render(1400.0, 900.0);
        assert_eq!(app.window_size, (1400.0, 900.0));

        let widest = tree
            .commands
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::FillRect { x, width, .. } => Some(x + width),
                _ => None,
            })
            .fold(0.0_f32, f32::max);
        assert!(
            widest > WINDOW_WIDTH,
            "nothing was drawn past the default width: the size was ignored"
        );
    }

    #[test]
    fn a_window_smaller_than_the_layout_allows_still_draws_and_still_clicks() {
        let mut app = VpnManager::new();
        let size = (320.0, 200.0);
        let tree = app.render(size.0, size.1);
        assert!(!tree.commands.is_empty());

        // `new_frame` floors the size, so the toolbar is still laid out at the
        // minimum the design assumes rather than collapsing to nothing.
        let frame = render_frame(&app, size.0, size.1);
        assert!(frame.width >= MIN_WIDTH);
        assert!(frame.height >= MIN_HEIGHT);
        assert!(
            frame.rect_of(|t| *t == Target::AddProfile).is_some(),
            "the toolbar has to survive a small window"
        );
    }
}

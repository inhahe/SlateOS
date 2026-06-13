//! SlateOS VPN Connection Manager
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
//! Network I/O is performed through SlateOS syscalls; simulated with
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
const TAB_HEIGHT: f32 = 32.0;

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
            return Err(format!("MTU must be between 576 and 9000, got {}", self.mtu));
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
    pub log_scroll_offset: f32,
    pub search_query: String,
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
            log_scroll_offset: 0.0,
            search_query: String::new(),
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
            && sel >= self.profiles.len() {
                self.selected_profile = Some(self.profiles.len().saturating_sub(1));
            }
        Some(profile)
    }

    /// Update a profile at the given index.
    pub fn update_profile(&mut self, index: usize, updated: VpnProfile) -> Result<(), String> {
        updated.validate()?;
        if index >= self.profiles.len() {
            return Err("Invalid profile index".into());
        }
        let old_name = self.profiles[index].name.clone();
        self.profiles[index] = updated;
        let new_name = self.profiles[index].name.clone();
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
        self.connections.iter_mut().find(|c| c.profile_id == profile_id)
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
            (active, conn.bytes_sent, conn.bytes_received, conn.uptime_secs)
        };

        if !was_active {
            return Err("Not connected".into());
        }

        // Accumulate stats to profile
        if let Some(profile) = self.profiles.iter_mut().find(|p| p.id == pid) {
            profile.total_bytes_sent = profile.total_bytes_sent.saturating_add(sent);
            profile.total_bytes_received = profile.total_bytes_received.saturating_add(recv);
            profile.total_connection_time_secs = profile
                .total_connection_time_secs
                .saturating_add(uptime);
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

    /// Sort profiles by the current sort order.
    pub fn sort_profiles(&mut self) {
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
    }

    /// Toggle the kill switch for a specific profile.
    pub fn toggle_kill_switch(&mut self, index: usize) {
        if let Some(profile) = self.profiles.get_mut(index) {
            profile.kill_switch = !profile.kill_switch;
            let state = if profile.kill_switch { "enabled" } else { "disabled" };
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
        self.add_log("System", &format!("Global kill switch {state}"), LogLevel::Warning);
    }

    /// Add a DNS override server to a profile.
    pub fn add_dns_override(&mut self, index: usize, dns: &str) -> Result<(), String> {
        if !is_valid_ipv4(dns) {
            return Err(format!("Invalid DNS server address: {dns}"));
        }
        let profile = self.profiles.get_mut(index).ok_or("Invalid profile index")?;
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
        let profile = self.profiles.get_mut(index).ok_or("Invalid profile index")?;
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
                    && (conn.status.is_active() || conn.status == ConnectionStatus::Connecting) {
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
        self.connections.iter().filter(|c| c.status.is_active()).count()
    }

    /// Get the total data transferred across all active connections.
    pub fn total_transfer(&self) -> (u64, u64) {
        self.connections
            .iter()
            .filter(|c| c.status.is_active())
            .fold((0u64, 0u64), |(sent, recv), c| {
                (sent.saturating_add(c.bytes_sent), recv.saturating_add(c.bytes_received))
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
    }

    /// Begin editing by opening the add dialog with a new blank profile.
    pub fn start_add_profile(&mut self) {
        let proto = VpnProtocol::WireGuard;
        self.editing_profile = Some(VpnProfile::new(0, "", "", proto));
        self.show_add_dialog = true;
    }

    /// Begin editing the currently selected profile.
    pub fn start_edit_profile(&mut self) {
        if let Some(profile) = self.selected().cloned() {
            self.editing_profile = Some(profile);
            self.show_add_dialog = true;
        }
    }

    /// Cancel editing.
    pub fn cancel_edit(&mut self) {
        self.editing_profile = None;
        self.show_add_dialog = false;
    }

    /// Confirm the add/edit dialog.
    pub fn confirm_edit(&mut self) -> Result<(), String> {
        let profile = self.editing_profile.take().ok_or("No profile being edited")?;
        self.show_add_dialog = false;

        // Check if this is an update to an existing profile
        if let Some(idx) = self.profiles.iter().position(|p| p.id == profile.id) {
            self.update_profile(idx, profile)
        } else {
            self.add_profile(profile).map(|_| ())
        }
    }

    /// Simulate connection data changing (for UI testing).
    pub fn simulate_traffic(&mut self, profile_id: u32, sent: u64, received: u64) {
        if let Some(conn) = self.connection_for_mut(profile_id)
            && conn.status == ConnectionStatus::Connected {
                conn.bytes_sent = conn.bytes_sent.saturating_add(sent);
                conn.bytes_received = conn.bytes_received.saturating_add(received);
                conn.uptime_secs = conn.uptime_secs.saturating_add(1);
            }
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

/// Format bytes as a human-readable string.
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
    p2.allowed_ips = vec![
        String::from("10.0.0.0/8"),
        String::from("172.16.0.0/12"),
    ];
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
        password: String::from(""),
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
        password: String::from(""),
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
pub fn render_app(app: &VpnManager) -> RenderTree {
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

    render_title_bar(&mut tree, app);
    render_toolbar(&mut tree, app);

    let content_y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;
    let content_h = WINDOW_HEIGHT - content_y - STATUS_BAR_HEIGHT;

    render_sidebar(&mut tree, app, content_y, content_h);
    render_detail_panel(&mut tree, app, content_y, content_h);
    render_status_bar(&mut tree, app);

    if app.show_add_dialog {
        render_add_dialog(&mut tree, app);
    }

    tree
}

fn render_title_bar(tree: &mut RenderTree, app: &VpnManager) {
    // Title bar background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: WINDOW_WIDTH,
        height: TITLE_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Title text
    tree.push(RenderCommand::Text {
        x: 16.0,
        y: 12.0,
        text: String::from("VPN Manager"),
        font_size: 16.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Connection count indicator
    let active = app.active_count();
    let indicator_text = if active > 0 {
        format!("{active} active")
    } else {
        String::from("No connections")
    };
    let indicator_color = if active > 0 { GREEN } else { OVERLAY0 };

    tree.push(RenderCommand::Text {
        x: WINDOW_WIDTH - 160.0,
        y: 14.0,
        text: indicator_text,
        font_size: 12.0,
        color: indicator_color,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Global kill switch indicator
    if app.global_kill_switch {
        tree.push(RenderCommand::FillRect {
            x: WINDOW_WIDTH - 240.0,
            y: 10.0,
            width: 70.0,
            height: 20.0,
            color: RED,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 235.0,
            y: 13.0,
            text: String::from("KILL SW"),
            font_size: 11.0,
            color: MANTLE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    // Separator line
    tree.push(RenderCommand::Line {
        x1: 0.0,
        y1: TITLE_BAR_HEIGHT,
        x2: WINDOW_WIDTH,
        y2: TITLE_BAR_HEIGHT,
        color: SURFACE0,
        width: 1.0,
    });
}

fn render_toolbar(tree: &mut RenderTree, app: &VpnManager) {
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

    let btn_y = y + 4.0;

    // Add button
    render_toolbar_button(tree, "Add", 8.0, btn_y, GREEN);
    // Remove button
    render_toolbar_button(tree, "Remove", 80.0, btn_y, RED);
    // Connect button
    render_toolbar_button(tree, "Connect", 166.0, btn_y, BLUE);
    // Disconnect button
    render_toolbar_button(tree, "Disconnect", 248.0, btn_y, PEACH);
    // Quick Connect button
    render_toolbar_button(tree, "Quick Connect", 358.0, btn_y, LAVENDER);
    // Import button
    render_toolbar_button(tree, "Import", 486.0, btn_y, SUBTEXT0);
    // Export button
    render_toolbar_button(tree, "Export", 550.0, btn_y, SUBTEXT0);

    // Sort dropdown
    tree.push(RenderCommand::Text {
        x: WINDOW_WIDTH - 180.0,
        y: y + 10.0,
        text: format!("Sort: {}", app.sort_order.label()),
        font_size: 12.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Separator
    tree.push(RenderCommand::Line {
        x1: 0.0,
        y1: y + TOOLBAR_HEIGHT,
        x2: WINDOW_WIDTH,
        y2: y + TOOLBAR_HEIGHT,
        color: SURFACE1,
        width: 1.0,
    });
}

fn render_toolbar_button(tree: &mut RenderTree, label: &str, x: f32, y: f32, color: Color) {
    let width = (label.len() as f32) * 8.0 + 16.0;
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width,
        height: 28.0,
        color: Color::rgba(color.r, color.g, color.b, 40),
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::StrokeRect {
        x,
        y,
        width,
        height: 28.0,
        color: Color::rgba(color.r, color.g, color.b, 80),
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: x + 8.0,
        y: y + 7.0,
        text: label.to_string(),
        font_size: 12.0,
        color,
        font_weight: FontWeightHint::Regular,
        max_width: Some(width - 16.0),
    });
}

fn render_sidebar(tree: &mut RenderTree, app: &VpnManager, content_y: f32, content_h: f32) {
    // Sidebar background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: content_y,
        width: SIDEBAR_WIDTH,
        height: content_h,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Search box
    let search_y = content_y + 8.0;
    tree.push(RenderCommand::FillRect {
        x: 8.0,
        y: search_y,
        width: SIDEBAR_WIDTH - 16.0,
        height: 28.0,
        color: SURFACE0,
        corner_radii: CornerRadii::all(4.0),
    });
    let search_text = if app.search_query.is_empty() {
        String::from("Search profiles...")
    } else {
        app.search_query.clone()
    };
    let search_color = if app.search_query.is_empty() {
        OVERLAY0
    } else {
        TEXT_COLOR
    };
    tree.push(RenderCommand::Text {
        x: 16.0,
        y: search_y + 7.0,
        text: search_text,
        font_size: 12.0,
        color: search_color,
        font_weight: FontWeightHint::Regular,
        max_width: Some(SIDEBAR_WIDTH - 32.0),
    });

    // Profile list
    let list_y = search_y + 36.0;
    let filtered = app.filtered_profiles();

    tree.push(RenderCommand::PushClip {
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
            render_sidebar_item(tree, profile, conn, item_y, is_selected);
        }
    }

    tree.push(RenderCommand::PopClip);

    // Sidebar separator
    tree.push(RenderCommand::Line {
        x1: SIDEBAR_WIDTH,
        y1: content_y,
        x2: SIDEBAR_WIDTH,
        y2: content_y + content_h,
        color: SURFACE0,
        width: 1.0,
    });
}

fn render_sidebar_item(
    tree: &mut RenderTree,
    profile: &VpnProfile,
    connection: Option<&VpnConnection>,
    y: f32,
    selected: bool,
) {
    // Selection highlight
    if selected {
        tree.push(RenderCommand::FillRect {
            x: 4.0,
            y,
            width: SIDEBAR_WIDTH - 8.0,
            height: SIDEBAR_ITEM_HEIGHT - 4.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
    }

    // Status indicator dot
    let status_color = connection
        .map(|c| c.status.color())
        .unwrap_or(OVERLAY0);
    tree.push(RenderCommand::FillRect {
        x: 16.0,
        y: y + 12.0,
        width: 10.0,
        height: 10.0,
        color: status_color,
        corner_radii: CornerRadii::all(5.0),
    });

    // Profile name
    let name_color = if profile.enabled { TEXT_COLOR } else { OVERLAY0 };
    tree.push(RenderCommand::Text {
        x: 34.0,
        y: y + 8.0,
        text: profile.name.clone(),
        font_size: 13.0,
        color: name_color,
        font_weight: FontWeightHint::Bold,
        max_width: Some(SIDEBAR_WIDTH - 50.0),
    });

    // Server and protocol
    tree.push(RenderCommand::Text {
        x: 34.0,
        y: y + 26.0,
        text: format!("{} - {}", profile.server_address, profile.protocol.label()),
        font_size: 11.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(SIDEBAR_WIDTH - 50.0),
    });

    // Status text
    let status_text = connection
        .map(|c| c.status.label().to_string())
        .unwrap_or_else(|| String::from("Disconnected"));
    tree.push(RenderCommand::Text {
        x: 34.0,
        y: y + 40.0,
        text: status_text,
        font_size: 10.0,
        color: status_color,
        font_weight: FontWeightHint::Light,
        max_width: Some(SIDEBAR_WIDTH - 50.0),
    });

    // Kill switch badge
    if profile.kill_switch {
        tree.push(RenderCommand::FillRect {
            x: SIDEBAR_WIDTH - 50.0,
            y: y + 8.0,
            width: 32.0,
            height: 16.0,
            color: Color::rgba(RED.r, RED.g, RED.b, 60),
            corner_radii: CornerRadii::all(3.0),
        });
        tree.push(RenderCommand::Text {
            x: SIDEBAR_WIDTH - 48.0,
            y: y + 10.0,
            text: String::from("KS"),
            font_size: 10.0,
            color: RED,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }
}

fn render_detail_panel(
    tree: &mut RenderTree,
    app: &VpnManager,
    content_y: f32,
    content_h: f32,
) {
    let px = SIDEBAR_WIDTH + 1.0;
    let pw = WINDOW_WIDTH - SIDEBAR_WIDTH - 1.0;

    // Background
    tree.push(RenderCommand::FillRect {
        x: px,
        y: content_y,
        width: pw,
        height: content_h,
        color: BASE,
        corner_radii: CornerRadii::ZERO,
    });

    if app.selected_profile.is_none() || app.profiles.is_empty() {
        render_no_selection(tree, px, content_y, pw, content_h);
        return;
    }

    // Tab bar
    render_tab_bar(tree, app, px, content_y, pw);

    let panel_y = content_y + TAB_HEIGHT + 8.0;
    let panel_h = content_h - TAB_HEIGHT - 8.0;

    tree.push(RenderCommand::PushClip {
        x: px,
        y: panel_y,
        width: pw,
        height: panel_h,
    });

    match app.current_tab {
        DetailTab::Overview => render_tab_overview(tree, app, px, panel_y, pw),
        DetailTab::Connection => render_tab_connection(tree, app, px, panel_y, pw),
        DetailTab::SplitTunnel => render_tab_split_tunnel(tree, app, px, panel_y, pw),
        DetailTab::ProtocolConfig => render_tab_protocol(tree, app, px, panel_y, pw),
        DetailTab::Log => render_tab_log(tree, app, px, panel_y, pw),
        DetailTab::Stats => render_tab_stats(tree, app, px, panel_y, pw),
    }

    tree.push(RenderCommand::PopClip);
}

fn render_tab_bar(tree: &mut RenderTree, app: &VpnManager, px: f32, py: f32, pw: f32) {
    // Tab bar background
    tree.push(RenderCommand::FillRect {
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
            tree.push(RenderCommand::FillRect {
                x: tx,
                y: py,
                width: tab_w,
                height: TAB_HEIGHT,
                color: BASE,
                corner_radii: CornerRadii::ZERO,
            });
            // Active indicator line
            tree.push(RenderCommand::FillRect {
                x: tx,
                y: py + TAB_HEIGHT - 2.0,
                width: tab_w,
                height: 2.0,
                color: BLUE,
                corner_radii: CornerRadii::ZERO,
            });
        }

        tree.push(RenderCommand::Text {
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
        });
    }
}

fn render_no_selection(tree: &mut RenderTree, px: f32, py: f32, pw: f32, ph: f32) {
    tree.push(RenderCommand::Text {
        x: px + pw / 2.0 - 80.0,
        y: py + ph / 2.0 - 10.0,
        text: String::from("Select a VPN profile"),
        font_size: 16.0,
        color: OVERLAY0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

fn render_tab_overview(
    tree: &mut RenderTree,
    app: &VpnManager,
    px: f32,
    py: f32,
    pw: f32,
) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let conn = app.connection_for(profile.id);
    let mut y = py + SECTION_PADDING;

    // Section: Profile Info
    y = render_section_title(tree, "Profile Information", px + SECTION_PADDING, y);

    y = render_field_row(tree, "Name:", &profile.name, px + SECTION_PADDING, y, pw);
    y = render_field_row(
        tree,
        "Server:",
        &profile.server_address,
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        tree,
        "Port:",
        &profile.port.to_string(),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        tree,
        "Protocol:",
        profile.protocol.label(),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        tree,
        "Auth:",
        profile.auth_method.label(),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        tree,
        "MTU:",
        &profile.mtu.to_string(),
        px + SECTION_PADDING,
        y,
        pw,
    );

    y += 8.0;

    // Toggles
    y = render_toggle_row(tree, "Enabled", profile.enabled, px + SECTION_PADDING, y);
    y = render_toggle_row(
        tree,
        "Auto Connect",
        profile.auto_connect,
        px + SECTION_PADDING,
        y,
    );
    y = render_toggle_row(
        tree,
        "Auto Reconnect",
        profile.auto_reconnect,
        px + SECTION_PADDING,
        y,
    );
    y = render_toggle_row(
        tree,
        "Kill Switch",
        profile.kill_switch,
        px + SECTION_PADDING,
        y,
    );
    y = render_toggle_row(
        tree,
        "Split Tunnel",
        profile.split_tunnel,
        px + SECTION_PADDING,
        y,
    );

    y += 12.0;

    // DNS overrides
    if !profile.dns_override.is_empty() {
        y = render_section_title(tree, "DNS Override", px + SECTION_PADDING, y);
        for dns in &profile.dns_override {
            y = render_field_row(tree, "", dns, px + SECTION_PADDING + 8.0, y, pw);
        }
        y += 4.0;
    }

    // Notes
    if !profile.notes.is_empty() {
        y = render_section_title(tree, "Notes", px + SECTION_PADDING, y);
        tree.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 8.0,
            y,
            text: profile.notes.clone(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(pw - SECTION_PADDING * 2.0 - 16.0),
        });
        let _ = y; // suppress unused
    }

    // Connection summary
    if let Some(c) = conn
        && c.status == ConnectionStatus::Connected {
            let summary_y = py + SECTION_PADDING;
            let summary_x = px + pw - 180.0;

            tree.push(RenderCommand::FillRect {
                x: summary_x - 8.0,
                y: summary_y - 4.0,
                width: 170.0,
                height: 80.0,
                color: Color::rgba(GREEN.r, GREEN.g, GREEN.b, 20),
                corner_radii: CornerRadii::all(8.0),
            });

            tree.push(RenderCommand::Text {
                x: summary_x,
                y: summary_y,
                text: String::from("Connected"),
                font_size: 14.0,
                color: GREEN,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: summary_x,
                y: summary_y + 20.0,
                text: format!("IP: {}", c.local_ip),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: summary_x,
                y: summary_y + 36.0,
                text: format!("Latency: {}ms", c.latency_ms),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            tree.push(RenderCommand::Text {
                x: summary_x,
                y: summary_y + 52.0,
                text: format!("Uptime: {}", c.format_uptime()),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
}

fn render_tab_connection(
    tree: &mut RenderTree,
    app: &VpnManager,
    px: f32,
    py: f32,
    pw: f32,
) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let conn = app.connection_for(profile.id);
    let mut y = py + SECTION_PADDING;

    y = render_section_title(tree, "Connection Details", px + SECTION_PADDING, y);

    if let Some(c) = conn {
        // Status with colored indicator
        let status_label = c.status.label();
        let status_color = c.status.color();

        tree.push(RenderCommand::FillRect {
            x: px + SECTION_PADDING,
            y: y + 2.0,
            width: 8.0,
            height: 8.0,
            color: status_color,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 14.0,
            y,
            text: format!("Status: {status_label}"),
            font_size: 13.0,
            color: status_color,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += FIELD_HEIGHT;

        if c.status == ConnectionStatus::Connected {
            y = render_field_row(tree, "Local IP:", &c.local_ip, px + SECTION_PADDING, y, pw);
            y = render_field_row(tree, "Remote IP:", &c.remote_ip, px + SECTION_PADDING, y, pw);
            y = render_field_row(
                tree,
                "Latency:",
                &format!("{}ms", c.latency_ms),
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(
                tree,
                "Uptime:",
                &c.format_uptime(),
                px + SECTION_PADDING,
                y,
                pw,
            );

            y += 8.0;
            y = render_section_title(tree, "Data Transfer", px + SECTION_PADDING, y);
            y = render_field_row(
                tree,
                "Sent:",
                &format_bytes(c.bytes_sent),
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(
                tree,
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
            render_action_button(tree, "Disconnect", btn_x, y, PEACH);
            render_action_button(tree, "Reconnect", btn_x + BUTTON_WIDTH + 8.0, y, YELLOW);
        } else {
            render_action_button(tree, "Connect", btn_x, y, GREEN);
        }
    } else {
        tree.push(RenderCommand::Text {
            x: px + SECTION_PADDING,
            y,
            text: String::from("No connection data available"),
            font_size: 13.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    let _ = y; // suppress unused
}

fn render_tab_split_tunnel(
    tree: &mut RenderTree,
    app: &VpnManager,
    px: f32,
    py: f32,
    pw: f32,
) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let mut y = py + SECTION_PADDING;

    y = render_section_title(tree, "Split Tunneling", px + SECTION_PADDING, y);

    // Toggle
    y = render_toggle_row(
        tree,
        "Enable Split Tunneling",
        profile.split_tunnel,
        px + SECTION_PADDING,
        y,
    );
    y += 8.0;

    // Explanation text
    tree.push(RenderCommand::Text {
        x: px + SECTION_PADDING,
        y,
        text: String::from("When enabled, only traffic to the allowed IP ranges"),
        font_size: 11.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(pw - SECTION_PADDING * 2.0),
    });
    y += 16.0;
    tree.push(RenderCommand::Text {
        x: px + SECTION_PADDING,
        y,
        text: String::from("goes through the VPN tunnel."),
        font_size: 11.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(pw - SECTION_PADDING * 2.0),
    });
    y += 24.0;

    // Allowed IPs list
    y = render_section_title(tree, "Allowed IP Ranges", px + SECTION_PADDING, y);

    if profile.allowed_ips.is_empty() {
        tree.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 8.0,
            y,
            text: String::from("No IP ranges configured"),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        y += FIELD_HEIGHT;
    } else {
        for (i, ip) in profile.allowed_ips.iter().enumerate() {
            // Row background
            let row_bg = if i % 2 == 0 { SURFACE0 } else { BASE };
            tree.push(RenderCommand::FillRect {
                x: px + SECTION_PADDING,
                y,
                width: pw - SECTION_PADDING * 2.0,
                height: FIELD_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::all(3.0),
            });
            tree.push(RenderCommand::Text {
                x: px + SECTION_PADDING + 8.0,
                y: y + 6.0,
                text: ip.clone(),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(pw - SECTION_PADDING * 2.0 - 80.0),
            });

            // Remove button
            tree.push(RenderCommand::Text {
                x: px + pw - SECTION_PADDING - 50.0,
                y: y + 6.0,
                text: String::from("Remove"),
                font_size: 11.0,
                color: RED,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            y += FIELD_HEIGHT + 2.0;
        }
    }

    y += 8.0;

    // Add IP button
    render_action_button(tree, "Add IP Range", px + SECTION_PADDING, y, BLUE);

    let _ = y; // suppress unused
}

fn render_tab_protocol(
    tree: &mut RenderTree,
    app: &VpnManager,
    px: f32,
    py: f32,
    pw: f32,
) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let mut y = py + SECTION_PADDING;

    y = render_section_title(
        tree,
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
            y = render_field_row(tree, "Config File:", config_file, px + SECTION_PADDING, y, pw);
            y = render_field_row(tree, "Cipher:", cipher, px + SECTION_PADDING, y, pw);
            y = render_toggle_row(tree, "Compression", *compression, px + SECTION_PADDING, y);
        }
        ProtocolSettings::WireGuard {
            peer_public_key,
            endpoint,
            persistent_keepalive,
        } => {
            y = render_field_row(tree, "Peer Key:", peer_public_key, px + SECTION_PADDING, y, pw);
            y = render_field_row(tree, "Endpoint:", endpoint, px + SECTION_PADDING, y, pw);
            y = render_field_row(
                tree,
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
                tree,
                "IKE Version:",
                &format!("v{ike_version}"),
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(tree, "Phase 1:", phase1_algo, px + SECTION_PADDING, y, pw);
            y = render_field_row(tree, "Phase 2:", phase2_algo, px + SECTION_PADDING, y, pw);
        }
        ProtocolSettings::Generic => {
            tree.push(RenderCommand::Text {
                x: px + SECTION_PADDING,
                y,
                text: String::from("No protocol-specific settings for this protocol."),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(pw - SECTION_PADDING * 2.0),
            });
            y += FIELD_HEIGHT;
        }
    }

    let _ = y; // suppress unused
}

fn render_tab_log(
    tree: &mut RenderTree,
    app: &VpnManager,
    px: f32,
    py: f32,
    pw: f32,
) {
    let mut y = py + SECTION_PADDING;

    y = render_section_title(tree, "Connection Log", px + SECTION_PADDING, y);

    // Clear log button
    render_action_button(tree, "Clear Log", px + pw - SECTION_PADDING - BUTTON_WIDTH, y - 24.0, RED);

    if app.log.is_empty() {
        tree.push(RenderCommand::Text {
            x: px + SECTION_PADDING,
            y,
            text: String::from("No log entries"),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        return;
    }

    // Log header
    tree.push(RenderCommand::FillRect {
        x: px + SECTION_PADDING,
        y,
        width: pw - SECTION_PADDING * 2.0,
        height: LOG_ENTRY_HEIGHT,
        color: SURFACE1,
        corner_radii: CornerRadii::all(3.0),
    });
    tree.push(RenderCommand::Text {
        x: px + SECTION_PADDING + 8.0,
        y: y + 4.0,
        text: String::from("Time"),
        font_size: 11.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    tree.push(RenderCommand::Text {
        x: px + SECTION_PADDING + 80.0,
        y: y + 4.0,
        text: String::from("Level"),
        font_size: 11.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    tree.push(RenderCommand::Text {
        x: px + SECTION_PADDING + 130.0,
        y: y + 4.0,
        text: String::from("Profile"),
        font_size: 11.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    tree.push(RenderCommand::Text {
        x: px + SECTION_PADDING + 250.0,
        y: y + 4.0,
        text: String::from("Message"),
        font_size: 11.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    y += LOG_ENTRY_HEIGHT + 2.0;

    // Log entries (reverse chronological)
    for entry in app.log.iter().rev() {
        let row_y = y;
        if row_y > py + 500.0 {
            break;
        }

        // Alternating row colors
        let row_bg = if app
            .log
            .iter()
            .rev()
            .position(|e| std::ptr::eq(e, entry))
            .unwrap_or(0)
            % 2
            == 0
        {
            Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, 80)
        } else {
            Color::TRANSPARENT
        };

        tree.push(RenderCommand::FillRect {
            x: px + SECTION_PADDING,
            y: row_y,
            width: pw - SECTION_PADDING * 2.0,
            height: LOG_ENTRY_HEIGHT,
            color: row_bg,
            corner_radii: CornerRadii::ZERO,
        });

        tree.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 8.0,
            y: row_y + 4.0,
            text: format_timestamp(entry.timestamp),
            font_size: 10.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        tree.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 80.0,
            y: row_y + 4.0,
            text: entry.level.label().to_string(),
            font_size: 10.0,
            color: entry.level.color(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        tree.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 130.0,
            y: row_y + 4.0,
            text: entry.profile_name.clone(),
            font_size: 10.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(110.0),
        });

        tree.push(RenderCommand::Text {
            x: px + SECTION_PADDING + 250.0,
            y: row_y + 4.0,
            text: entry.message.clone(),
            font_size: 10.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(pw - SECTION_PADDING * 2.0 - 260.0),
        });

        y += LOG_ENTRY_HEIGHT;
    }
}

fn render_tab_stats(
    tree: &mut RenderTree,
    app: &VpnManager,
    px: f32,
    py: f32,
    pw: f32,
) {
    let profile = match app.selected() {
        Some(p) => p,
        None => return,
    };
    let mut y = py + SECTION_PADDING;

    y = render_section_title(tree, "Usage Statistics", px + SECTION_PADDING, y);

    // Cumulative stats
    y = render_field_row(
        tree,
        "Total Sent:",
        &format_bytes(profile.total_bytes_sent),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        tree,
        "Total Received:",
        &format_bytes(profile.total_bytes_received),
        px + SECTION_PADDING,
        y,
        pw,
    );
    y = render_field_row(
        tree,
        "Total Time:",
        &format_duration_long(profile.total_connection_time_secs),
        px + SECTION_PADDING,
        y,
        pw,
    );

    y += 16.0;

    // Current session stats
    if let Some(conn) = app.connection_for(profile.id)
        && conn.status == ConnectionStatus::Connected {
            y = render_section_title(tree, "Current Session", px + SECTION_PADDING, y);
            y = render_field_row(
                tree,
                "Session Sent:",
                &format_bytes(conn.bytes_sent),
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(
                tree,
                "Session Recv:",
                &format_bytes(conn.bytes_received),
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(
                tree,
                "Uptime:",
                &conn.format_uptime(),
                px + SECTION_PADDING,
                y,
                pw,
            );
            y = render_field_row(
                tree,
                "Latency:",
                &format!("{}ms", conn.latency_ms),
                px + SECTION_PADDING,
                y,
                pw,
            );
        }

    y += 16.0;

    // Data usage bar chart
    y = render_section_title(tree, "Data Usage Comparison", px + SECTION_PADDING, y);

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
        let total = profile.total_bytes_sent.saturating_add(profile.total_bytes_received);
        let ratio = total as f32 / max_bytes as f32;
        let bar_w = (chart_w - 140.0) * ratio;

        // Label
        tree.push(RenderCommand::Text {
            x: chart_x,
            y: y + 5.0,
            text: profile.name.clone(),
            font_size: 11.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
        });

        // Bar
        tree.push(RenderCommand::FillRect {
            x: chart_x + 130.0,
            y: y + 2.0,
            width: bar_w.max(2.0),
            height: bar_h - 4.0,
            color: BLUE,
            corner_radii: CornerRadii::all(3.0),
        });

        // Value
        tree.push(RenderCommand::Text {
            x: chart_x + 134.0 + bar_w,
            y: y + 5.0,
            text: format_bytes(total),
            font_size: 10.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        y += bar_h + 4.0;
    }

    let _ = y; // suppress unused
}

fn render_add_dialog(tree: &mut RenderTree, app: &VpnManager) {
    // Modal overlay
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y: 0.0,
        width: WINDOW_WIDTH,
        height: WINDOW_HEIGHT,
        color: Color::rgba(0, 0, 0, 160),
        corner_radii: CornerRadii::ZERO,
    });

    let dialog_w = 480.0;
    let dialog_h = 420.0;
    let dx = (WINDOW_WIDTH - dialog_w) / 2.0;
    let dy = (WINDOW_HEIGHT - dialog_h) / 2.0;

    // Dialog background
    tree.push(RenderCommand::FillRect {
        x: dx,
        y: dy,
        width: dialog_w,
        height: dialog_h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(12.0),
    });
    tree.push(RenderCommand::StrokeRect {
        x: dx,
        y: dy,
        width: dialog_w,
        height: dialog_h,
        color: SURFACE1,
        line_width: 1.0,
        corner_radii: CornerRadii::all(12.0),
    });

    // Title
    tree.push(RenderCommand::Text {
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
    });

    // Form fields
    if let Some(profile) = &app.editing_profile {
        let mut y = dy + 50.0;
        let fw = dialog_w - 40.0;

        y = render_dialog_field(tree, "Name:", &profile.name, dx + 20.0, y, fw);
        y = render_dialog_field(tree, "Server:", &profile.server_address, dx + 20.0, y, fw);
        y = render_dialog_field(tree, "Port:", &profile.port.to_string(), dx + 20.0, y, fw);
        y = render_dialog_field(
            tree,
            "Protocol:",
            profile.protocol.label(),
            dx + 20.0,
            y,
            fw,
        );
        y = render_dialog_field(tree, "Auth:", profile.auth_method.label(), dx + 20.0, y, fw);
        y = render_dialog_field(tree, "MTU:", &profile.mtu.to_string(), dx + 20.0, y, fw);

        y += 8.0;
        y = render_toggle_row(tree, "Kill Switch", profile.kill_switch, dx + 20.0, y);
        y = render_toggle_row(tree, "Auto Connect", profile.auto_connect, dx + 20.0, y);
        y = render_toggle_row(tree, "Split Tunnel", profile.split_tunnel, dx + 20.0, y);

        let _ = y; // suppress unused
    }

    // Buttons
    let btn_y = dy + dialog_h - 50.0;
    render_action_button(tree, "Cancel", dx + dialog_w - 240.0, btn_y, RED);
    render_action_button(tree, "Save", dx + dialog_w - 130.0, btn_y, GREEN);
}

fn render_status_bar(tree: &mut RenderTree, app: &VpnManager) {
    let y = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;

    // Background
    tree.push(RenderCommand::FillRect {
        x: 0.0,
        y,
        width: WINDOW_WIDTH,
        height: STATUS_BAR_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::ZERO,
    });

    // Separator
    tree.push(RenderCommand::Line {
        x1: 0.0,
        y1: y,
        x2: WINDOW_WIDTH,
        y2: y,
        color: SURFACE0,
        width: 1.0,
    });

    // Profile count
    tree.push(RenderCommand::Text {
        x: 12.0,
        y: y + 8.0,
        text: format!("{} profiles", app.profiles.len()),
        font_size: 11.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Active connections
    let active = app.active_count();
    tree.push(RenderCommand::Text {
        x: 120.0,
        y: y + 8.0,
        text: format!("{active} connected"),
        font_size: 11.0,
        color: if active > 0 { GREEN } else { OVERLAY0 },
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Total transfer
    let (sent, recv) = app.total_transfer();
    if sent > 0 || recv > 0 {
        tree.push(RenderCommand::Text {
            x: 260.0,
            y: y + 8.0,
            text: format!("TX: {}  RX: {}", format_bytes(sent), format_bytes(recv)),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // Kill switch status
    if app.global_kill_switch {
        tree.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 120.0,
            y: y + 8.0,
            text: String::from("Kill Switch: ON"),
            font_size: 11.0,
            color: RED,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }
}

// ============================================================================
// Render Helpers
// ============================================================================

fn render_section_title(tree: &mut RenderTree, title: &str, x: f32, y: f32) -> f32 {
    tree.push(RenderCommand::Text {
        x,
        y,
        text: title.to_string(),
        font_size: 14.0,
        color: LAVENDER,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    // Underline
    tree.push(RenderCommand::Line {
        x1: x,
        y1: y + 18.0,
        x2: x + 200.0,
        y2: y + 18.0,
        color: Color::rgba(LAVENDER.r, LAVENDER.g, LAVENDER.b, 60),
        width: 1.0,
    });
    y + 26.0
}

fn render_field_row(
    tree: &mut RenderTree,
    label: &str,
    value: &str,
    x: f32,
    y: f32,
    _pw: f32,
) -> f32 {
    if !label.is_empty() {
        tree.push(RenderCommand::Text {
            x,
            y: y + 4.0,
            text: label.to_string(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(FIELD_LABEL_WIDTH),
        });
    }
    tree.push(RenderCommand::Text {
        x: x + FIELD_LABEL_WIDTH,
        y: y + 4.0,
        text: value.to_string(),
        font_size: 12.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Regular,
        max_width: Some(400.0),
    });
    y + FIELD_HEIGHT
}

fn render_toggle_row(tree: &mut RenderTree, label: &str, enabled: bool, x: f32, y: f32) -> f32 {
    tree.push(RenderCommand::Text {
        x,
        y: y + 4.0,
        text: label.to_string(),
        font_size: 12.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(FIELD_LABEL_WIDTH),
    });

    // Toggle track
    let track_x = x + FIELD_LABEL_WIDTH;
    let track_color = if enabled {
        Color::rgba(GREEN.r, GREEN.g, GREEN.b, 120)
    } else {
        SURFACE1
    };
    tree.push(RenderCommand::FillRect {
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
    tree.push(RenderCommand::FillRect {
        x: knob_x,
        y: y + 6.0,
        width: 14.0,
        height: 14.0,
        color: if enabled { GREEN } else { OVERLAY0 },
        corner_radii: CornerRadii::all(7.0),
    });

    y + FIELD_HEIGHT
}

fn render_action_button(tree: &mut RenderTree, label: &str, x: f32, y: f32, color: Color) {
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: BUTTON_WIDTH,
        height: BUTTON_HEIGHT,
        color: Color::rgba(color.r, color.g, color.b, 40),
        corner_radii: CornerRadii::all(6.0),
    });
    tree.push(RenderCommand::StrokeRect {
        x,
        y,
        width: BUTTON_WIDTH,
        height: BUTTON_HEIGHT,
        color: Color::rgba(color.r, color.g, color.b, 100),
        line_width: 1.0,
        corner_radii: CornerRadii::all(6.0),
    });
    tree.push(RenderCommand::Text {
        x: x + 12.0,
        y: y + 9.0,
        text: label.to_string(),
        font_size: 12.0,
        color,
        font_weight: FontWeightHint::Bold,
        max_width: Some(BUTTON_WIDTH - 24.0),
    });
}

fn render_dialog_field(
    tree: &mut RenderTree,
    label: &str,
    value: &str,
    x: f32,
    y: f32,
    fw: f32,
) -> f32 {
    tree.push(RenderCommand::Text {
        x,
        y: y + 4.0,
        text: label.to_string(),
        font_size: 12.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(100.0),
    });

    // Input box
    tree.push(RenderCommand::FillRect {
        x: x + 100.0,
        y,
        width: fw - 100.0,
        height: FIELD_HEIGHT,
        color: MANTLE,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::StrokeRect {
        x: x + 100.0,
        y,
        width: fw - 100.0,
        height: FIELD_HEIGHT,
        color: SURFACE1,
        line_width: 1.0,
        corner_radii: CornerRadii::all(4.0),
    });
    tree.push(RenderCommand::Text {
        x: x + 108.0,
        y: y + 6.0,
        text: if value.is_empty() {
            String::from("...")
        } else {
            value.to_string()
        },
        font_size: 12.0,
        color: if value.is_empty() { OVERLAY0 } else { TEXT_COLOR },
        font_weight: FontWeightHint::Regular,
        max_width: Some(fw - 120.0),
    });

    y + FIELD_HEIGHT + 6.0
}

/// Format seconds as a human-readable duration (e.g. "100h 0m").
fn format_duration_long(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
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
            cert_path, key_path,
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
        assert_ne!(ConnectionStatus::Connected.color(), ConnectionStatus::Disconnected.color());
        assert_ne!(ConnectionStatus::Connected.color(), ConnectionStatus::Error(String::new()).color());
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
        assert!(status_sort_key(&ConnectionStatus::Connected) < status_sort_key(&ConnectionStatus::Connecting));
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

    #[test]
    fn test_format_bytes_small() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_bytes_kb() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
    }

    #[test]
    fn test_format_bytes_mb() {
        let mb = 1024 * 1024;
        assert_eq!(format_bytes(mb), "1.0 MB");
        assert_eq!(format_bytes(mb + mb / 2), "1.5 MB");
    }

    #[test]
    fn test_format_bytes_gb() {
        let gb = 1024 * 1024 * 1024;
        assert_eq!(format_bytes(gb), "1.00 GB");
    }

    #[test]
    fn test_format_timestamp() {
        assert_eq!(format_timestamp(0), "00:00:00");
        assert_eq!(format_timestamp(3661), "01:01:01");
    }

    #[test]
    fn test_format_duration_long_zero() {
        assert_eq!(format_duration_long(0), "0m");
    }

    #[test]
    fn test_format_duration_long_hours() {
        assert_eq!(format_duration_long(7200), "2h 0m");
        assert_eq!(format_duration_long(3660), "1h 1m");
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
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_manager_sort_by_protocol() {
        let mut mgr = VpnManager::new();
        mgr.set_sort_order(SortOrder::Protocol);
        let labels: Vec<&str> = mgr.profiles.iter().map(|p| p.protocol.label()).collect();
        let mut sorted = labels.clone();
        sorted.sort();
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
        let has_title = tree.commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "VPN Manager")
        });
        assert!(has_title);
    }

    #[test]
    fn test_render_app_has_profile_names() {
        let app = VpnManager::new();
        let tree = render_app(&app);
        let has_work = tree.commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "Work VPN")
        });
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
        let has_placeholder = tree.commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Select a VPN"))
        });
        assert!(has_placeholder);
    }

    #[test]
    fn test_render_app_with_dialog() {
        let mut app = VpnManager::new();
        app.start_add_profile();
        let tree = render_app(&app);
        let has_dialog_title = tree.commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "Add VPN Profile")
        });
        assert!(has_dialog_title);
    }

    #[test]
    fn test_render_app_connected_profile() {
        let mut app = VpnManager::new();
        app.connect(0).unwrap();
        let tree = render_app(&app);
        let has_connected = tree.commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "Connected")
        });
        assert!(has_connected);
    }

    #[test]
    fn test_render_global_kill_switch_visible() {
        let mut app = VpnManager::new();
        app.toggle_global_kill_switch();
        let tree = render_app(&app);
        let has_ks = tree.commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "KILL SW")
        });
        assert!(has_ks);
    }

    #[test]
    fn test_render_status_bar_shows_count() {
        let app = VpnManager::new();
        let tree = render_app(&app);
        let has_count = tree.commands.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("profiles"))
        });
        assert!(has_count);
    }

    // --- Sample data tests ---

    #[test]
    fn test_sample_profiles_have_unique_ids() {
        let profiles = sample_profiles();
        let mut ids: Vec<u32> = profiles.iter().map(|p| p.id).collect();
        ids.sort();
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
}

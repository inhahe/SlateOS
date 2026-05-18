//! OurOS Remote Desktop Viewer/Client
//!
//! GUI application for viewing and controlling remote desktops over RDP, VNC,
//! and SSH protocols. Provides:
//! - Connection manager with saved profiles (hostname, port, username, protocol)
//! - Connection profiles with custom display names and groups
//! - Display settings: resolution scaling, color depth, refresh rate
//! - Input forwarding: keyboard/mouse capture with configurable key mapping
//! - Clipboard sync: bidirectional clipboard sharing (text, images)
//! - File transfer: drag-and-drop panel with progress bars
//! - Multi-monitor: remote monitor detection and selection
//! - Session management: active sessions, disconnect/reconnect, thumbnails
//! - Performance monitoring: bandwidth, latency, frame rate overlay
//! - Quality presets: Auto, Low Bandwidth, Balanced, High Quality
//! - Connection history with quick-connect
//! - Screenshot capture of remote desktop
//! - Fullscreen mode with configurable escape hotkey
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
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
#[allow(dead_code)]
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
#[allow(dead_code)]
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
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

const WINDOW_WIDTH: f32 = 1100.0;
const WINDOW_HEIGHT: f32 = 750.0;
const TITLE_BAR_HEIGHT: f32 = 40.0;
const TOOLBAR_HEIGHT: f32 = 36.0;
const SIDEBAR_WIDTH: f32 = 260.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const SIDEBAR_ITEM_HEIGHT: f32 = 52.0;
const SECTION_PADDING: f32 = 14.0;
const FIELD_HEIGHT: f32 = 28.0;
const FIELD_LABEL_WIDTH: f32 = 130.0;
const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_WIDTH: f32 = 110.0;
const TAB_HEIGHT: f32 = 32.0;
const TRANSFER_ITEM_HEIGHT: f32 = 48.0;
const HISTORY_ITEM_HEIGHT: f32 = 44.0;

// ============================================================================
// Core Data Types
// ============================================================================

/// Supported remote desktop protocols.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Protocol {
    Rdp,
    Vnc,
    Ssh,
}

impl Protocol {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rdp => "RDP",
            Self::Vnc => "VNC",
            Self::Ssh => "SSH",
        }
    }

    /// Default port for the protocol.
    pub fn default_port(self) -> u16 {
        match self {
            Self::Rdp => 3389,
            Self::Vnc => 5900,
            Self::Ssh => 22,
        }
    }

    /// Color indicator for protocol in UI.
    pub fn color(self) -> Color {
        match self {
            Self::Rdp => BLUE,
            Self::Vnc => GREEN,
            Self::Ssh => PEACH,
        }
    }
}

/// Resolution scaling mode for remote display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalingMode {
    AutoFit,
    Fixed100,
    Custom(u8), // percentage 25-400
}

impl ScalingMode {
    pub fn label(self) -> String {
        match self {
            Self::AutoFit => "Auto-fit".into(),
            Self::Fixed100 => "100%".into(),
            Self::Custom(pct) => format!("{pct}%"),
        }
    }
}

/// Color depth setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorDepth {
    Bit8,
    Bit16,
    Bit24,
    Bit32,
}

impl ColorDepth {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bit8 => "8-bit (256)",
            Self::Bit16 => "16-bit (High)",
            Self::Bit24 => "24-bit (True)",
            Self::Bit32 => "32-bit (Full)",
        }
    }

    pub fn bits(self) -> u8 {
        match self {
            Self::Bit8 => 8,
            Self::Bit16 => 16,
            Self::Bit24 => 24,
            Self::Bit32 => 32,
        }
    }
}

/// Quality preset for connection performance tuning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityPreset {
    Auto,
    LowBandwidth,
    Balanced,
    HighQuality,
}

impl QualityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::LowBandwidth => "Low Bandwidth",
            Self::Balanced => "Balanced",
            Self::HighQuality => "High Quality",
        }
    }

    /// Returns (color_depth, refresh_rate, scaling) for this preset.
    pub fn settings(self) -> (ColorDepth, u8, ScalingMode) {
        match self {
            Self::Auto => (ColorDepth::Bit24, 30, ScalingMode::AutoFit),
            Self::LowBandwidth => (ColorDepth::Bit8, 15, ScalingMode::AutoFit),
            Self::Balanced => (ColorDepth::Bit16, 30, ScalingMode::AutoFit),
            Self::HighQuality => (ColorDepth::Bit32, 60, ScalingMode::AutoFit),
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Auto => BLUE,
            Self::LowBandwidth => YELLOW,
            Self::Balanced => GREEN,
            Self::HighQuality => LAVENDER,
        }
    }
}

/// Display settings for the remote connection.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplaySettings {
    pub scaling: ScalingMode,
    pub color_depth: ColorDepth,
    pub refresh_rate: u8,
    pub width: u16,
    pub height: u16,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            scaling: ScalingMode::AutoFit,
            color_depth: ColorDepth::Bit24,
            refresh_rate: 30,
            width: 1920,
            height: 1080,
        }
    }
}

/// Input forwarding configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct InputConfig {
    pub forward_keyboard: bool,
    pub forward_mouse: bool,
    pub grab_keyboard: bool,
    pub key_mappings: Vec<KeyMapping>,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            forward_keyboard: true,
            forward_mouse: true,
            grab_keyboard: false,
            key_mappings: vec![
                KeyMapping {
                    local_key: Key::F11,
                    remote_key: Key::F11,
                    label: "F11 -> F11".into(),
                },
                KeyMapping {
                    local_key: Key::LeftCtrl,
                    remote_key: Key::LeftCtrl,
                    label: "Ctrl -> Ctrl".into(),
                },
            ],
        }
    }
}

/// A single key mapping entry.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyMapping {
    pub local_key: Key,
    pub remote_key: Key,
    pub label: String,
}

/// Clipboard sync state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardSyncMode {
    Disabled,
    LocalToRemote,
    RemoteToLocal,
    Bidirectional,
}

impl ClipboardSyncMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::LocalToRemote => "Local -> Remote",
            Self::RemoteToLocal => "Remote -> Local",
            Self::Bidirectional => "Bidirectional",
        }
    }
}

/// Clipboard content type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardContentType {
    Text,
    Image,
    Empty,
}

/// Clipboard sync state tracking.
#[derive(Clone, Debug)]
pub struct ClipboardState {
    pub mode: ClipboardSyncMode,
    pub last_sync_direction: Option<&'static str>,
    pub content_type: ClipboardContentType,
    pub content_size_bytes: u64,
    pub sync_count: u32,
}

impl Default for ClipboardState {
    fn default() -> Self {
        Self {
            mode: ClipboardSyncMode::Bidirectional,
            last_sync_direction: None,
            content_type: ClipboardContentType::Empty,
            content_size_bytes: 0,
            sync_count: 0,
        }
    }
}

/// File transfer direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// File transfer state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    Queued,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl TransferState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::InProgress => "Transferring",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Queued => SUBTEXT0,
            Self::InProgress => BLUE,
            Self::Completed => GREEN,
            Self::Failed => RED,
            Self::Cancelled => YELLOW,
        }
    }
}

/// A file transfer entry in the transfer queue.
#[derive(Clone, Debug)]
pub struct FileTransfer {
    pub id: u32,
    pub filename: String,
    pub size_bytes: u64,
    pub transferred_bytes: u64,
    pub direction: TransferDirection,
    pub state: TransferState,
}

impl FileTransfer {
    /// Progress as a percentage (0.0..=100.0).
    pub fn progress_percent(&self) -> f32 {
        if self.size_bytes == 0 {
            return 100.0;
        }
        (self.transferred_bytes as f64 / self.size_bytes as f64 * 100.0) as f32
    }

    /// Format file size for display.
    pub fn size_label(&self) -> String {
        format_bytes(self.size_bytes)
    }

    /// Format transferred size for display.
    pub fn transferred_label(&self) -> String {
        format_bytes(self.transferred_bytes)
    }
}

/// Remote monitor info for multi-monitor support.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteMonitor {
    pub id: u8,
    pub width: u16,
    pub height: u16,
    pub primary: bool,
    pub label: String,
}

/// Multi-monitor configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MonitorMode {
    SingleMonitor(u8), // which monitor ID to use
    SpanAll,
}

impl MonitorMode {
    pub fn label(&self) -> String {
        match self {
            Self::SingleMonitor(id) => format!("Monitor {}", id.saturating_add(1)),
            Self::SpanAll => "Span All".into(),
        }
    }
}

/// Session connection state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Authenticating,
    Connected,
    Reconnecting,
    Error,
}

impl SessionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting...",
            Self::Authenticating => "Authenticating...",
            Self::Connected => "Connected",
            Self::Reconnecting => "Reconnecting...",
            Self::Error => "Error",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Disconnected => OVERLAY0,
            Self::Connecting | Self::Authenticating => YELLOW,
            Self::Connected => GREEN,
            Self::Reconnecting => PEACH,
            Self::Error => RED,
        }
    }
}

/// An active remote session.
#[derive(Clone, Debug)]
pub struct RemoteSession {
    pub id: u32,
    pub profile_id: u32,
    pub display_name: String,
    pub state: SessionState,
    pub connected_at: Option<u64>,
    pub duration_secs: u64,
}

/// Performance metrics for an active session.
#[derive(Clone, Debug, Default)]
pub struct PerfMetrics {
    pub bandwidth_kbps: f32,
    pub latency_ms: f32,
    pub frame_rate: f32,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// A connection history entry.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub profile_name: String,
    pub hostname: String,
    pub protocol: Protocol,
    pub timestamp: u64,
    pub duration_secs: u64,
    pub success: bool,
}

/// A saved connection profile.
#[derive(Clone, Debug)]
pub struct ConnectionProfile {
    pub id: u32,
    pub display_name: String,
    pub group: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub protocol: Protocol,
    pub quality: QualityPreset,
    pub display: DisplaySettings,
    pub input: InputConfig,
    pub clipboard_mode: ClipboardSyncMode,
    pub auto_reconnect: bool,
}

impl ConnectionProfile {
    pub fn new_default(id: u32) -> Self {
        Self {
            id,
            display_name: String::new(),
            group: "Default".into(),
            hostname: String::new(),
            port: 3389,
            username: String::new(),
            protocol: Protocol::Rdp,
            quality: QualityPreset::Auto,
            display: DisplaySettings::default(),
            input: InputConfig::default(),
            clipboard_mode: ClipboardSyncMode::Bidirectional,
            auto_reconnect: true,
        }
    }
}

// ============================================================================
// UI Navigation / Tabs
// ============================================================================

/// Main view tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainView {
    Connections,
    ActiveSessions,
    FileTransfer,
    History,
}

impl MainView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connections => "Connections",
            Self::ActiveSessions => "Sessions",
            Self::FileTransfer => "File Transfer",
            Self::History => "History",
        }
    }
}

/// Detail panel tabs (right side) when editing a connection profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailTab {
    General,
    Display,
    Input,
    Advanced,
}

impl DetailTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Display => "Display",
            Self::Input => "Input",
            Self::Advanced => "Advanced",
        }
    }
}

// ============================================================================
// Application State
// ============================================================================

/// Complete application state for the remote desktop viewer.
pub struct RemoteDesktopApp {
    // --- Profiles ---
    pub profiles: Vec<ConnectionProfile>,
    pub selected_profile: Option<usize>,
    pub next_profile_id: u32,
    pub editing_profile: Option<ConnectionProfile>,

    // --- Sessions ---
    pub sessions: Vec<RemoteSession>,
    pub selected_session: Option<usize>,
    pub next_session_id: u32,

    // --- Performance ---
    pub perf_metrics: PerfMetrics,
    pub show_perf_overlay: bool,

    // --- File transfer ---
    pub transfers: Vec<FileTransfer>,
    pub next_transfer_id: u32,

    // --- Clipboard ---
    pub clipboard: ClipboardState,

    // --- Multi-monitor ---
    pub remote_monitors: Vec<RemoteMonitor>,
    pub monitor_mode: MonitorMode,

    // --- History ---
    pub history: VecDeque<HistoryEntry>,

    // --- Display ---
    pub fullscreen: bool,
    pub escape_hotkey: Key,

    // --- Navigation ---
    pub current_view: MainView,
    pub detail_tab: DetailTab,

    // --- Scroll state ---
    pub sidebar_scroll: f32,
    pub content_scroll: f32,

    // --- Misc ---
    pub status_message: Option<String>,
    pub confirm_delete: Option<usize>,
}

impl RemoteDesktopApp {
    pub fn new() -> Self {
        let mut app = Self {
            profiles: Vec::new(),
            selected_profile: None,
            next_profile_id: 1,
            editing_profile: None,
            sessions: Vec::new(),
            selected_session: None,
            next_session_id: 1,
            perf_metrics: PerfMetrics::default(),
            show_perf_overlay: false,
            transfers: Vec::new(),
            next_transfer_id: 1,
            clipboard: ClipboardState::default(),
            remote_monitors: Vec::new(),
            monitor_mode: MonitorMode::SpanAll,
            history: VecDeque::new(),
            fullscreen: false,
            escape_hotkey: Key::F11,
            current_view: MainView::Connections,
            detail_tab: DetailTab::General,
            sidebar_scroll: 0.0,
            content_scroll: 0.0,
            status_message: None,
            confirm_delete: None,
        };
        app.load_sample_data();
        app
    }

    /// Populate with representative sample data for UI development.
    fn load_sample_data(&mut self) {
        // Sample profiles
        let profiles = vec![
            ConnectionProfile {
                id: 1,
                display_name: "Dev Server".into(),
                group: "Development".into(),
                hostname: "dev.example.com".into(),
                port: 3389,
                username: "admin".into(),
                protocol: Protocol::Rdp,
                quality: QualityPreset::HighQuality,
                display: DisplaySettings {
                    scaling: ScalingMode::AutoFit,
                    color_depth: ColorDepth::Bit32,
                    refresh_rate: 60,
                    width: 1920,
                    height: 1080,
                },
                input: InputConfig::default(),
                clipboard_mode: ClipboardSyncMode::Bidirectional,
                auto_reconnect: true,
            },
            ConnectionProfile {
                id: 2,
                display_name: "Production DB".into(),
                group: "Production".into(),
                hostname: "db.prod.example.com".into(),
                port: 5900,
                username: "dbadmin".into(),
                protocol: Protocol::Vnc,
                quality: QualityPreset::Balanced,
                display: DisplaySettings {
                    scaling: ScalingMode::Fixed100,
                    color_depth: ColorDepth::Bit24,
                    refresh_rate: 30,
                    width: 1920,
                    height: 1080,
                },
                input: InputConfig::default(),
                clipboard_mode: ClipboardSyncMode::LocalToRemote,
                auto_reconnect: true,
            },
            ConnectionProfile {
                id: 3,
                display_name: "Build Machine".into(),
                group: "Development".into(),
                hostname: "build01.internal".into(),
                port: 22,
                username: "ci".into(),
                protocol: Protocol::Ssh,
                quality: QualityPreset::LowBandwidth,
                display: DisplaySettings::default(),
                input: InputConfig::default(),
                clipboard_mode: ClipboardSyncMode::Bidirectional,
                auto_reconnect: false,
            },
            ConnectionProfile {
                id: 4,
                display_name: "Staging Web".into(),
                group: "Staging".into(),
                hostname: "staging.example.com".into(),
                port: 3389,
                username: "deploy".into(),
                protocol: Protocol::Rdp,
                quality: QualityPreset::Auto,
                display: DisplaySettings::default(),
                input: InputConfig::default(),
                clipboard_mode: ClipboardSyncMode::Disabled,
                auto_reconnect: true,
            },
        ];
        self.profiles = profiles;
        self.next_profile_id = 5;
        self.selected_profile = Some(0);

        // Sample active session
        self.sessions.push(RemoteSession {
            id: 1,
            profile_id: 1,
            display_name: "Dev Server".into(),
            state: SessionState::Connected,
            connected_at: Some(1_700_000_000),
            duration_secs: 3672,
        });
        self.next_session_id = 2;

        // Sample perf metrics
        self.perf_metrics = PerfMetrics {
            bandwidth_kbps: 4500.0,
            latency_ms: 23.5,
            frame_rate: 58.2,
            packets_sent: 152_340,
            packets_received: 287_102,
            bytes_sent: 45_320_000,
            bytes_received: 312_450_000,
        };

        // Sample file transfers
        self.transfers = vec![
            FileTransfer {
                id: 1,
                filename: "project-backup.zip".into(),
                size_bytes: 52_428_800,
                transferred_bytes: 36_700_160,
                direction: TransferDirection::Download,
                state: TransferState::InProgress,
            },
            FileTransfer {
                id: 2,
                filename: "config.yaml".into(),
                size_bytes: 4096,
                transferred_bytes: 4096,
                direction: TransferDirection::Upload,
                state: TransferState::Completed,
            },
            FileTransfer {
                id: 3,
                filename: "database-dump.sql".into(),
                size_bytes: 104_857_600,
                transferred_bytes: 0,
                direction: TransferDirection::Download,
                state: TransferState::Queued,
            },
        ];
        self.next_transfer_id = 4;

        // Sample remote monitors
        self.remote_monitors = vec![
            RemoteMonitor {
                id: 0,
                width: 1920,
                height: 1080,
                primary: true,
                label: "Primary (1920x1080)".into(),
            },
            RemoteMonitor {
                id: 1,
                width: 2560,
                height: 1440,
                primary: false,
                label: "Secondary (2560x1440)".into(),
            },
        ];

        // Sample history
        self.history = VecDeque::from(vec![
            HistoryEntry {
                profile_name: "Dev Server".into(),
                hostname: "dev.example.com".into(),
                protocol: Protocol::Rdp,
                timestamp: 1_700_000_000,
                duration_secs: 7200,
                success: true,
            },
            HistoryEntry {
                profile_name: "Production DB".into(),
                hostname: "db.prod.example.com".into(),
                protocol: Protocol::Vnc,
                timestamp: 1_699_900_000,
                duration_secs: 1800,
                success: true,
            },
            HistoryEntry {
                profile_name: "Build Machine".into(),
                hostname: "build01.internal".into(),
                protocol: Protocol::Ssh,
                timestamp: 1_699_800_000,
                duration_secs: 0,
                success: false,
            },
        ]);
    }

    // ========================================================================
    // Profile CRUD
    // ========================================================================

    /// Add a new connection profile.
    pub fn add_profile(&mut self, mut profile: ConnectionProfile) -> u32 {
        profile.id = self.next_profile_id;
        let id = profile.id;
        self.next_profile_id = self.next_profile_id.saturating_add(1);
        self.profiles.push(profile);
        self.selected_profile = Some(self.profiles.len().saturating_sub(1));
        id
    }

    /// Delete a profile by index.
    pub fn delete_profile(&mut self, index: usize) -> bool {
        if index < self.profiles.len() {
            self.profiles.remove(index);
            if self.profiles.is_empty() {
                self.selected_profile = None;
            } else if let Some(sel) = self.selected_profile {
                if sel >= self.profiles.len() {
                    self.selected_profile = Some(self.profiles.len().saturating_sub(1));
                }
            }
            true
        } else {
            false
        }
    }

    /// Update a profile at the given index.
    pub fn update_profile(&mut self, index: usize, profile: ConnectionProfile) -> bool {
        if let Some(p) = self.profiles.get_mut(index) {
            *p = profile;
            true
        } else {
            false
        }
    }

    /// Get profile by index.
    pub fn get_profile(&self, index: usize) -> Option<&ConnectionProfile> {
        self.profiles.get(index)
    }

    /// Find profile by id.
    pub fn find_profile_by_id(&self, id: u32) -> Option<usize> {
        self.profiles.iter().position(|p| p.id == id)
    }

    /// Get all distinct group names.
    pub fn groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = self
            .profiles
            .iter()
            .map(|p| p.group.clone())
            .collect();
        groups.sort();
        groups.dedup();
        groups
    }

    /// Get profiles in a group.
    pub fn profiles_in_group(&self, group: &str) -> Vec<usize> {
        self.profiles
            .iter()
            .enumerate()
            .filter(|(_, p)| p.group == group)
            .map(|(i, _)| i)
            .collect()
    }

    // ========================================================================
    // Session Management
    // ========================================================================

    /// Start a new session from a profile.
    pub fn connect_profile(&mut self, profile_index: usize) -> Option<u32> {
        let profile = self.profiles.get(profile_index)?;
        let session = RemoteSession {
            id: self.next_session_id,
            profile_id: profile.id,
            display_name: profile.display_name.clone(),
            state: SessionState::Connecting,
            connected_at: None,
            duration_secs: 0,
        };
        let id = session.id;
        self.next_session_id = self.next_session_id.saturating_add(1);
        self.sessions.push(session);
        self.history.push_front(HistoryEntry {
            profile_name: profile.display_name.clone(),
            hostname: profile.hostname.clone(),
            protocol: profile.protocol,
            timestamp: 0, // would use real time in production
            duration_secs: 0,
            success: true,
        });
        // Trim history
        while self.history.len() > 50 {
            self.history.pop_back();
        }
        Some(id)
    }

    /// Disconnect a session by index.
    pub fn disconnect_session(&mut self, index: usize) -> bool {
        if let Some(session) = self.sessions.get_mut(index) {
            session.state = SessionState::Disconnected;
            true
        } else {
            false
        }
    }

    /// Reconnect a session by index.
    pub fn reconnect_session(&mut self, index: usize) -> bool {
        if let Some(session) = self.sessions.get_mut(index) {
            session.state = SessionState::Reconnecting;
            true
        } else {
            false
        }
    }

    /// Advance session state (simulated for UI development).
    pub fn advance_session_state(&mut self, index: usize) {
        if let Some(session) = self.sessions.get_mut(index) {
            session.state = match session.state {
                SessionState::Connecting => SessionState::Authenticating,
                SessionState::Authenticating => SessionState::Connected,
                SessionState::Reconnecting => SessionState::Connected,
                other => other,
            };
        }
    }

    /// Remove disconnected sessions.
    pub fn cleanup_sessions(&mut self) {
        self.sessions
            .retain(|s| s.state != SessionState::Disconnected);
        if let Some(sel) = self.selected_session {
            if sel >= self.sessions.len() {
                self.selected_session = if self.sessions.is_empty() {
                    None
                } else {
                    Some(self.sessions.len().saturating_sub(1))
                };
            }
        }
    }

    // ========================================================================
    // File Transfer
    // ========================================================================

    /// Queue a new file transfer.
    pub fn queue_transfer(
        &mut self,
        filename: String,
        size_bytes: u64,
        direction: TransferDirection,
    ) -> u32 {
        let id = self.next_transfer_id;
        self.next_transfer_id = self.next_transfer_id.saturating_add(1);
        self.transfers.push(FileTransfer {
            id,
            filename,
            size_bytes,
            transferred_bytes: 0,
            direction,
            state: TransferState::Queued,
        });
        id
    }

    /// Cancel a transfer by index.
    pub fn cancel_transfer(&mut self, index: usize) -> bool {
        if let Some(t) = self.transfers.get_mut(index) {
            if t.state == TransferState::Queued || t.state == TransferState::InProgress {
                t.state = TransferState::Cancelled;
                return true;
            }
        }
        false
    }

    /// Update transfer progress.
    pub fn update_transfer_progress(&mut self, index: usize, bytes: u64) {
        if let Some(t) = self.transfers.get_mut(index) {
            t.transferred_bytes = bytes.min(t.size_bytes);
            if t.transferred_bytes >= t.size_bytes {
                t.state = TransferState::Completed;
            } else if t.state == TransferState::Queued {
                t.state = TransferState::InProgress;
            }
        }
    }

    /// Clear completed/failed/cancelled transfers.
    pub fn clear_finished_transfers(&mut self) {
        self.transfers.retain(|t| {
            t.state == TransferState::Queued || t.state == TransferState::InProgress
        });
    }

    // ========================================================================
    // Clipboard Sync
    // ========================================================================

    /// Sync clipboard content.
    pub fn sync_clipboard(
        &mut self,
        direction: &'static str,
        content_type: ClipboardContentType,
        size_bytes: u64,
    ) {
        if self.clipboard.mode == ClipboardSyncMode::Disabled {
            return;
        }
        self.clipboard.last_sync_direction = Some(direction);
        self.clipboard.content_type = content_type;
        self.clipboard.content_size_bytes = size_bytes;
        self.clipboard.sync_count = self.clipboard.sync_count.saturating_add(1);
    }

    // ========================================================================
    // Display Settings
    // ========================================================================

    /// Apply a quality preset to the current display settings.
    pub fn apply_quality_preset(&mut self, preset: QualityPreset) {
        let (depth, rate, scaling) = preset.settings();
        if let Some(idx) = self.selected_profile {
            if let Some(profile) = self.profiles.get_mut(idx) {
                profile.quality = preset;
                profile.display.color_depth = depth;
                profile.display.refresh_rate = rate;
                profile.display.scaling = scaling;
            }
        }
    }

    /// Set monitor mode.
    pub fn set_monitor_mode(&mut self, mode: MonitorMode) {
        self.monitor_mode = mode;
    }

    // ========================================================================
    // Screenshot
    // ========================================================================

    /// Capture a screenshot of the remote desktop (simulated).
    pub fn capture_screenshot(&mut self) -> String {
        let name = format!("screenshot_{}.png", self.next_transfer_id);
        self.next_transfer_id = self.next_transfer_id.saturating_add(1);
        self.status_message = Some(format!("Screenshot saved: {name}"));
        name
    }

    // ========================================================================
    // Fullscreen Toggle
    // ========================================================================

    /// Toggle fullscreen mode.
    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
    }

    /// Set the escape hotkey for exiting fullscreen.
    pub fn set_escape_hotkey(&mut self, key: Key) {
        self.escape_hotkey = key;
    }

    // ========================================================================
    // History
    // ========================================================================

    /// Add a history entry.
    pub fn add_history_entry(&mut self, entry: HistoryEntry) {
        self.history.push_front(entry);
        while self.history.len() > 100 {
            self.history.pop_back();
        }
    }

    /// Clear all history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Get recent history entries.
    pub fn recent_history(&self, count: usize) -> Vec<&HistoryEntry> {
        self.history.iter().take(count).collect()
    }

    // ========================================================================
    // Event Handling
    // ========================================================================

    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Key(key) => self.handle_key(key),
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, mouse: &guitk::event::MouseEvent) -> EventResult {
        match mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let x = mouse.x;
                let y = mouse.y;

                // Title bar (ignored for interactions)
                if y < TITLE_BAR_HEIGHT {
                    return EventResult::Consumed;
                }

                // Toolbar buttons
                let toolbar_y = TITLE_BAR_HEIGHT;
                if y >= toolbar_y && y < toolbar_y + TOOLBAR_HEIGHT {
                    return self.handle_toolbar_click(x);
                }

                // Main content area
                let content_y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + TAB_HEIGHT;
                if y >= content_y {
                    // Sidebar click
                    if x < SIDEBAR_WIDTH {
                        return self.handle_sidebar_click(x, y - content_y);
                    }
                }

                EventResult::Consumed
            }
            MouseEventKind::Scroll { dy, .. } => {
                let x = mouse.x;
                if x < SIDEBAR_WIDTH {
                    self.sidebar_scroll = (self.sidebar_scroll - dy).max(0.0);
                } else {
                    self.content_scroll = (self.content_scroll - dy).max(0.0);
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_toolbar_click(&mut self, x: f32) -> EventResult {
        // Toolbar button positions (spaced at ~90px each)
        let buttons_start = SECTION_PADDING;
        let btn_width = 85.0;
        let gap = 6.0;
        let idx = ((x - buttons_start) / (btn_width + gap)) as usize;
        match idx {
            0 => {
                // New Connection
                let profile = ConnectionProfile::new_default(0);
                let _id = self.add_profile(profile);
                self.current_view = MainView::Connections;
            }
            1 => {
                // Connect
                if let Some(sel) = self.selected_profile {
                    let _id = self.connect_profile(sel);
                    self.current_view = MainView::ActiveSessions;
                }
            }
            2 => {
                // Screenshot
                let _name = self.capture_screenshot();
            }
            3 => {
                // Fullscreen toggle
                self.toggle_fullscreen();
            }
            4 => {
                // Performance overlay toggle
                self.show_perf_overlay = !self.show_perf_overlay;
            }
            _ => {}
        }
        EventResult::Consumed
    }

    fn handle_sidebar_click(&mut self, _x: f32, y: f32) -> EventResult {
        match self.current_view {
            MainView::Connections => {
                let scrolled_y = y + self.sidebar_scroll;
                let idx = (scrolled_y / SIDEBAR_ITEM_HEIGHT) as usize;
                if idx < self.profiles.len() {
                    self.selected_profile = Some(idx);
                }
            }
            MainView::ActiveSessions => {
                let scrolled_y = y + self.sidebar_scroll;
                let idx = (scrolled_y / SIDEBAR_ITEM_HEIGHT) as usize;
                if idx < self.sessions.len() {
                    self.selected_session = Some(idx);
                }
            }
            _ => {}
        }
        EventResult::Consumed
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }

        // Fullscreen toggle check
        if key.key == self.escape_hotkey && self.fullscreen {
            self.toggle_fullscreen();
            return EventResult::Consumed;
        }

        match key.key {
            // Tab navigation
            Key::Num1 if key.modifiers.ctrl => {
                self.current_view = MainView::Connections;
                EventResult::Consumed
            }
            Key::Num2 if key.modifiers.ctrl => {
                self.current_view = MainView::ActiveSessions;
                EventResult::Consumed
            }
            Key::Num3 if key.modifiers.ctrl => {
                self.current_view = MainView::FileTransfer;
                EventResult::Consumed
            }
            Key::Num4 if key.modifiers.ctrl => {
                self.current_view = MainView::History;
                EventResult::Consumed
            }
            // Delete selected profile
            Key::Delete if self.current_view == MainView::Connections => {
                if let Some(sel) = self.selected_profile {
                    self.confirm_delete = Some(sel);
                }
                EventResult::Consumed
            }
            // New profile
            Key::N if key.modifiers.ctrl => {
                let profile = ConnectionProfile::new_default(0);
                let _id = self.add_profile(profile);
                EventResult::Consumed
            }
            // Connect selected profile
            Key::Enter
                if self.current_view == MainView::Connections
                    && self.selected_profile.is_some() =>
            {
                if let Some(sel) = self.selected_profile {
                    let _id = self.connect_profile(sel);
                }
                EventResult::Consumed
            }
            // Screenshot
            Key::S if key.modifiers.ctrl && key.modifiers.shift => {
                let _name = self.capture_screenshot();
                EventResult::Consumed
            }
            // Navigate profiles up/down
            Key::Up if self.current_view == MainView::Connections => {
                if let Some(sel) = self.selected_profile {
                    if sel > 0 {
                        self.selected_profile = Some(sel.saturating_sub(1));
                    }
                }
                EventResult::Consumed
            }
            Key::Down if self.current_view == MainView::Connections => {
                if let Some(sel) = self.selected_profile {
                    let next = sel.saturating_add(1);
                    if next < self.profiles.len() {
                        self.selected_profile = Some(next);
                    }
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Produce all render commands for the current frame.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(256);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_title_bar(&mut cmds);
        self.render_toolbar(&mut cmds);
        self.render_tabs(&mut cmds);

        let content_y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + TAB_HEIGHT;
        let content_h = WINDOW_HEIGHT - content_y - STATUS_BAR_HEIGHT;

        // Content area
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y,
            width: WINDOW_WIDTH,
            height: content_h,
        });

        match self.current_view {
            MainView::Connections => self.render_connections(&mut cmds, content_y, content_h),
            MainView::ActiveSessions => self.render_sessions(&mut cmds, content_y, content_h),
            MainView::FileTransfer => self.render_file_transfer(&mut cmds, content_y, content_h),
            MainView::History => self.render_history(&mut cmds, content_y, content_h),
        }

        cmds.push(RenderCommand::PopClip);

        self.render_status_bar(&mut cmds);

        if self.show_perf_overlay {
            self.render_perf_overlay(&mut cmds);
        }

        cmds
    }

    fn render_title_bar(&self, cmds: &mut Vec<RenderCommand>) {
        // Title bar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: TITLE_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // App icon (simple monitor shape)
        cmds.push(RenderCommand::StrokeRect {
            x: 12.0,
            y: 8.0,
            width: 22.0,
            height: 16.0,
            color: BLUE,
            line_width: 2.0,
            corner_radii: CornerRadii::all(3.0),
        });
        // Monitor stand
        cmds.push(RenderCommand::Line {
            x1: 23.0,
            y1: 24.0,
            x2: 23.0,
            y2: 30.0,
            color: BLUE,
            width: 2.0,
        });
        cmds.push(RenderCommand::Line {
            x1: 17.0,
            y1: 30.0,
            x2: 29.0,
            y2: 30.0,
            color: BLUE,
            width: 2.0,
        });

        // Title text
        cmds.push(RenderCommand::Text {
            x: 42.0,
            y: 12.0,
            text: "Remote Desktop Viewer".into(),
            font_size: 15.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
        });

        // Fullscreen indicator
        if self.fullscreen {
            cmds.push(RenderCommand::FillRect {
                x: WINDOW_WIDTH - 120.0,
                y: 8.0,
                width: 80.0,
                height: 22.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: WINDOW_WIDTH - 112.0,
                y: 12.0,
                text: "Fullscreen".into(),
                font_size: 11.0,
                color: GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: Some(70.0),
            });
        }

        // Separator line
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TITLE_BAR_HEIGHT,
            x2: WINDOW_WIDTH,
            y2: TITLE_BAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TITLE_BAR_HEIGHT;

        // Toolbar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: TOOLBAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let btn_labels = ["+ New", "Connect", "Screenshot", "Fullscreen", "Perf"];
        let btn_colors = [GREEN, BLUE, PEACH, LAVENDER, YELLOW];
        let btn_width = 85.0;
        let gap = 6.0;
        let mut bx = SECTION_PADDING;

        for (i, label) in btn_labels.iter().enumerate() {
            let color = if let Some(&c) = btn_colors.get(i) {
                c
            } else {
                SURFACE1
            };

            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: y + 4.0,
                width: btn_width,
                height: TOOLBAR_HEIGHT - 8.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: y + 10.0,
                text: (*label).into(),
                font_size: 12.0,
                color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(btn_width - 16.0),
            });
            bx += btn_width + gap;
        }

        // Quality preset selector on right side
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 200.0,
            y: y + 10.0,
            text: "Quality:".into(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });

        let preset = self
            .selected_profile
            .and_then(|i| self.profiles.get(i))
            .map_or(QualityPreset::Auto, |p| p.quality);

        cmds.push(RenderCommand::FillRect {
            x: WINDOW_WIDTH - 140.0,
            y: y + 6.0,
            width: 120.0,
            height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 132.0,
            y: y + 10.0,
            text: preset.label().into(),
            font_size: 12.0,
            color: preset.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(104.0),
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TOOLBAR_HEIGHT,
            x2: WINDOW_WIDTH,
            y2: y + TOOLBAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_tabs(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: TAB_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let tabs = [
            MainView::Connections,
            MainView::ActiveSessions,
            MainView::FileTransfer,
            MainView::History,
        ];
        let tab_width = 130.0;
        let mut tx = SECTION_PADDING;

        for tab in &tabs {
            let is_active = *tab == self.current_view;
            let bg = if is_active { SURFACE0 } else { MANTLE };
            let fg = if is_active { TEXT_COLOR } else { SUBTEXT0 };

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: y + 2.0,
                width: tab_width,
                height: TAB_HEIGHT - 2.0,
                color: bg,
                corner_radii: CornerRadii {
                    top_left: 6.0,
                    top_right: 6.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            if is_active {
                // Active tab underline
                cmds.push(RenderCommand::Line {
                    x1: tx,
                    y1: y + TAB_HEIGHT - 1.0,
                    x2: tx + tab_width,
                    y2: y + TAB_HEIGHT - 1.0,
                    color: BLUE,
                    width: 2.0,
                });
            }

            cmds.push(RenderCommand::Text {
                x: tx + 12.0,
                y: y + 9.0,
                text: tab.label().into(),
                font_size: 12.0,
                color: fg,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width - 24.0),
            });

            tx += tab_width + 2.0;
        }

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TAB_HEIGHT,
            x2: WINDOW_WIDTH,
            y2: y + TAB_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_connections(&self, cmds: &mut Vec<RenderCommand>, content_y: f32, content_h: f32) {
        // Sidebar: profile list
        self.render_sidebar_profiles(cmds, content_y, content_h);

        // Vertical separator
        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: content_y,
            x2: SIDEBAR_WIDTH,
            y2: content_y + content_h,
            color: SURFACE0,
            width: 1.0,
        });

        // Detail panel
        if let Some(idx) = self.selected_profile {
            if let Some(profile) = self.profiles.get(idx) {
                self.render_profile_detail(cmds, profile, content_y, content_h);
            }
        } else {
            // Empty state
            cmds.push(RenderCommand::Text {
                x: SIDEBAR_WIDTH + 60.0,
                y: content_y + content_h / 2.0 - 10.0,
                text: "Select a connection profile or create a new one".into(),
                font_size: 14.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(400.0),
            });
        }
    }

    fn render_sidebar_profiles(
        &self,
        cmds: &mut Vec<RenderCommand>,
        content_y: f32,
        content_h: f32,
    ) {
        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: content_y,
            width: SIDEBAR_WIDTH,
            height: content_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y,
            width: SIDEBAR_WIDTH,
            height: content_h,
        });

        let mut cy = content_y - self.sidebar_scroll;

        // Group-based listing
        let groups = self.groups();
        for group in &groups {
            // Group header
            cmds.push(RenderCommand::Text {
                x: SECTION_PADDING,
                y: cy + 6.0,
                text: group.clone(),
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(SIDEBAR_WIDTH - 2.0 * SECTION_PADDING),
            });
            cy += 22.0;

            let indices = self.profiles_in_group(group);
            for &pi in &indices {
                if let Some(profile) = self.profiles.get(pi) {
                    let is_selected = self.selected_profile == Some(pi);
                    let bg = if is_selected { SURFACE0 } else { MANTLE };

                    cmds.push(RenderCommand::FillRect {
                        x: 4.0,
                        y: cy,
                        width: SIDEBAR_WIDTH - 8.0,
                        height: SIDEBAR_ITEM_HEIGHT - 4.0,
                        color: bg,
                        corner_radii: CornerRadii::all(6.0),
                    });

                    if is_selected {
                        cmds.push(RenderCommand::Line {
                            x1: 4.0,
                            y1: cy,
                            x2: 4.0,
                            y2: cy + SIDEBAR_ITEM_HEIGHT - 4.0,
                            color: BLUE,
                            width: 3.0,
                        });
                    }

                    // Protocol badge
                    cmds.push(RenderCommand::FillRect {
                        x: SECTION_PADDING,
                        y: cy + 6.0,
                        width: 36.0,
                        height: 18.0,
                        color: protocol_badge_bg(profile.protocol),
                        corner_radii: CornerRadii::all(3.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: SECTION_PADDING + 4.0,
                        y: cy + 8.0,
                        text: profile.protocol.label().into(),
                        font_size: 10.0,
                        color: CRUST,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(28.0),
                    });

                    // Display name
                    cmds.push(RenderCommand::Text {
                        x: SECTION_PADDING + 42.0,
                        y: cy + 8.0,
                        text: profile.display_name.clone(),
                        font_size: 13.0,
                        color: TEXT_COLOR,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(SIDEBAR_WIDTH - 80.0),
                    });

                    // Hostname
                    cmds.push(RenderCommand::Text {
                        x: SECTION_PADDING + 42.0,
                        y: cy + 26.0,
                        text: format!("{}:{}", profile.hostname, profile.port),
                        font_size: 11.0,
                        color: SUBTEXT0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(SIDEBAR_WIDTH - 80.0),
                    });

                    cy += SIDEBAR_ITEM_HEIGHT;
                }
            }
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_profile_detail(
        &self,
        cmds: &mut Vec<RenderCommand>,
        profile: &ConnectionProfile,
        content_y: f32,
        _content_h: f32,
    ) {
        let px = SIDEBAR_WIDTH + SECTION_PADDING;
        let pw = WINDOW_WIDTH - SIDEBAR_WIDTH - 2.0 * SECTION_PADDING;

        // Detail tab bar
        let detail_tabs = [
            DetailTab::General,
            DetailTab::Display,
            DetailTab::Input,
            DetailTab::Advanced,
        ];
        let dtab_width = 90.0;
        let mut dtx = px;
        for dt in &detail_tabs {
            let is_active = *dt == self.detail_tab;
            let bg = if is_active { SURFACE0 } else { BASE };
            let fg = if is_active { BLUE } else { SUBTEXT0 };

            cmds.push(RenderCommand::FillRect {
                x: dtx,
                y: content_y + 4.0,
                width: dtab_width,
                height: 26.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: dtx + 10.0,
                y: content_y + 10.0,
                text: dt.label().into(),
                font_size: 12.0,
                color: fg,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(dtab_width - 20.0),
            });
            dtx += dtab_width + 4.0;
        }

        let fields_y = content_y + 40.0;
        match self.detail_tab {
            DetailTab::General => {
                self.render_detail_general(cmds, profile, px, fields_y, pw);
            }
            DetailTab::Display => {
                self.render_detail_display(cmds, profile, px, fields_y, pw);
            }
            DetailTab::Input => {
                self.render_detail_input(cmds, profile, px, fields_y, pw);
            }
            DetailTab::Advanced => {
                self.render_detail_advanced(cmds, profile, px, fields_y, pw);
            }
        }
    }

    fn render_detail_general(
        &self,
        cmds: &mut Vec<RenderCommand>,
        profile: &ConnectionProfile,
        px: f32,
        mut cy: f32,
        pw: f32,
    ) {
        let fields: Vec<(&str, String)> = vec![
            ("Display Name", profile.display_name.clone()),
            ("Group", profile.group.clone()),
            ("Hostname", profile.hostname.clone()),
            ("Port", profile.port.to_string()),
            ("Username", profile.username.clone()),
            ("Protocol", profile.protocol.label().into()),
            ("Quality", profile.quality.label().into()),
        ];

        for (label, value) in &fields {
            render_field_row(cmds, px, cy, pw, label, value);
            cy += FIELD_HEIGHT + 6.0;
        }

        // Buttons
        cy += 10.0;
        render_button(cmds, px, cy, BUTTON_WIDTH, BUTTON_HEIGHT, "Save", GREEN);
        render_button(
            cmds,
            px + BUTTON_WIDTH + 10.0,
            cy,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Delete",
            RED,
        );
        render_button(
            cmds,
            px + 2.0 * (BUTTON_WIDTH + 10.0),
            cy,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Connect",
            BLUE,
        );
    }

    fn render_detail_display(
        &self,
        cmds: &mut Vec<RenderCommand>,
        profile: &ConnectionProfile,
        px: f32,
        mut cy: f32,
        pw: f32,
    ) {
        let fields: Vec<(&str, String)> = vec![
            ("Scaling", profile.display.scaling.label()),
            ("Color Depth", profile.display.color_depth.label().into()),
            ("Refresh Rate", format!("{} Hz", profile.display.refresh_rate)),
            ("Resolution", format!("{}x{}", profile.display.width, profile.display.height)),
        ];

        for (label, value) in &fields {
            render_field_row(cmds, px, cy, pw, label, value);
            cy += FIELD_HEIGHT + 6.0;
        }

        // Multi-monitor section
        cy += 10.0;
        cmds.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Multi-Monitor".into(),
            font_size: 13.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(pw),
        });
        cy += 22.0;

        render_field_row(cmds, px, cy, pw, "Mode", &self.monitor_mode.label());
        cy += FIELD_HEIGHT + 6.0;

        for monitor in &self.remote_monitors {
            let primary_tag = if monitor.primary { " (Primary)" } else { "" };
            render_field_row(
                cmds,
                px,
                cy,
                pw,
                &format!("Monitor {}", monitor.id.saturating_add(1)),
                &format!("{}x{}{}", monitor.width, monitor.height, primary_tag),
            );
            cy += FIELD_HEIGHT + 6.0;
        }
    }

    fn render_detail_input(
        &self,
        cmds: &mut Vec<RenderCommand>,
        profile: &ConnectionProfile,
        px: f32,
        mut cy: f32,
        pw: f32,
    ) {
        let input_fields: Vec<(&str, String)> = vec![
            (
                "Keyboard",
                if profile.input.forward_keyboard {
                    "Forwarding"
                } else {
                    "Disabled"
                }
                .into(),
            ),
            (
                "Mouse",
                if profile.input.forward_mouse {
                    "Forwarding"
                } else {
                    "Disabled"
                }
                .into(),
            ),
            (
                "Grab Keyboard",
                if profile.input.grab_keyboard {
                    "Yes"
                } else {
                    "No"
                }
                .into(),
            ),
        ];

        for (label, value) in &input_fields {
            render_field_row(cmds, px, cy, pw, label, value);
            cy += FIELD_HEIGHT + 6.0;
        }

        // Key mappings
        cy += 10.0;
        cmds.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Key Mappings".into(),
            font_size: 13.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(pw),
        });
        cy += 22.0;

        for mapping in &profile.input.key_mappings {
            cmds.push(RenderCommand::FillRect {
                x: px,
                y: cy,
                width: pw.min(400.0),
                height: FIELD_HEIGHT,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: px + 10.0,
                y: cy + 6.0,
                text: mapping.label.clone(),
                font_size: 12.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(pw.min(380.0)),
            });
            cy += FIELD_HEIGHT + 4.0;
        }
    }

    fn render_detail_advanced(
        &self,
        cmds: &mut Vec<RenderCommand>,
        profile: &ConnectionProfile,
        px: f32,
        mut cy: f32,
        pw: f32,
    ) {
        let fields: Vec<(&str, String)> = vec![
            ("Clipboard", profile.clipboard_mode.label().into()),
            (
                "Auto Reconnect",
                if profile.auto_reconnect { "Yes" } else { "No" }.into(),
            ),
            (
                "Escape Hotkey",
                format!("{:?}", self.escape_hotkey),
            ),
        ];

        for (label, value) in &fields {
            render_field_row(cmds, px, cy, pw, label, value);
            cy += FIELD_HEIGHT + 6.0;
        }

        // Clipboard sync status
        cy += 10.0;
        cmds.push(RenderCommand::Text {
            x: px,
            y: cy,
            text: "Clipboard Status".into(),
            font_size: 13.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(pw),
        });
        cy += 22.0;

        let clip_fields: Vec<(&str, String)> = vec![
            ("Mode", self.clipboard.mode.label().into()),
            (
                "Last Sync",
                self.clipboard
                    .last_sync_direction
                    .unwrap_or("Never")
                    .into(),
            ),
            (
                "Content",
                format!("{:?}", self.clipboard.content_type),
            ),
            ("Sync Count", self.clipboard.sync_count.to_string()),
        ];

        for (label, value) in &clip_fields {
            render_field_row(cmds, px, cy, pw, label, value);
            cy += FIELD_HEIGHT + 6.0;
        }
    }

    fn render_sessions(&self, cmds: &mut Vec<RenderCommand>, content_y: f32, content_h: f32) {
        // Sidebar: session list
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: content_y,
            width: SIDEBAR_WIDTH,
            height: content_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y,
            width: SIDEBAR_WIDTH,
            height: content_h,
        });

        if self.sessions.is_empty() {
            cmds.push(RenderCommand::Text {
                x: SECTION_PADDING,
                y: content_y + 20.0,
                text: "No active sessions".into(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 2.0 * SECTION_PADDING),
            });
        } else {
            let mut cy = content_y;
            for (i, session) in self.sessions.iter().enumerate() {
                let is_sel = self.selected_session == Some(i);
                let bg = if is_sel { SURFACE0 } else { MANTLE };

                cmds.push(RenderCommand::FillRect {
                    x: 4.0,
                    y: cy,
                    width: SIDEBAR_WIDTH - 8.0,
                    height: SIDEBAR_ITEM_HEIGHT - 4.0,
                    color: bg,
                    corner_radii: CornerRadii::all(6.0),
                });

                // State indicator dot
                cmds.push(RenderCommand::FillRect {
                    x: SECTION_PADDING,
                    y: cy + 10.0,
                    width: 10.0,
                    height: 10.0,
                    color: session.state.color(),
                    corner_radii: CornerRadii::all(5.0),
                });

                cmds.push(RenderCommand::Text {
                    x: SECTION_PADDING + 16.0,
                    y: cy + 8.0,
                    text: session.display_name.clone(),
                    font_size: 13.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(SIDEBAR_WIDTH - 60.0),
                });

                cmds.push(RenderCommand::Text {
                    x: SECTION_PADDING + 16.0,
                    y: cy + 26.0,
                    text: session.state.label().into(),
                    font_size: 11.0,
                    color: session.state.color(),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(SIDEBAR_WIDTH - 60.0),
                });

                cy += SIDEBAR_ITEM_HEIGHT;
            }
        }

        cmds.push(RenderCommand::PopClip);

        // Vertical separator
        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: content_y,
            x2: SIDEBAR_WIDTH,
            y2: content_y + content_h,
            color: SURFACE0,
            width: 1.0,
        });

        // Session detail panel
        let px = SIDEBAR_WIDTH + SECTION_PADDING;
        let pw = WINDOW_WIDTH - SIDEBAR_WIDTH - 2.0 * SECTION_PADDING;

        if let Some(idx) = self.selected_session {
            if let Some(session) = self.sessions.get(idx) {
                let mut cy = content_y + SECTION_PADDING;

                cmds.push(RenderCommand::Text {
                    x: px,
                    y: cy,
                    text: session.display_name.clone(),
                    font_size: 16.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(pw),
                });
                cy += 28.0;

                let detail_fields: Vec<(&str, String)> = vec![
                    ("Status", session.state.label().into()),
                    ("Session ID", session.id.to_string()),
                    ("Duration", format_duration(session.duration_secs)),
                ];

                for (label, value) in &detail_fields {
                    render_field_row(cmds, px, cy, pw, label, value);
                    cy += FIELD_HEIGHT + 6.0;
                }

                // Action buttons
                cy += 10.0;
                if session.state == SessionState::Connected {
                    render_button(cmds, px, cy, BUTTON_WIDTH, BUTTON_HEIGHT, "Disconnect", RED);
                } else if session.state == SessionState::Disconnected {
                    render_button(
                        cmds,
                        px,
                        cy,
                        BUTTON_WIDTH,
                        BUTTON_HEIGHT,
                        "Reconnect",
                        GREEN,
                    );
                }
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: px + 60.0,
                y: content_y + content_h / 2.0 - 10.0,
                text: "Select a session to view details".into(),
                font_size: 14.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(400.0),
            });
        }
    }

    fn render_file_transfer(&self, cmds: &mut Vec<RenderCommand>, content_y: f32, content_h: f32) {
        let px = SECTION_PADDING;
        let pw = WINDOW_WIDTH - 2.0 * SECTION_PADDING;

        // Header
        cmds.push(RenderCommand::Text {
            x: px,
            y: content_y + SECTION_PADDING,
            text: "File Transfers".into(),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(pw),
        });

        // Action buttons
        let btn_y = content_y + SECTION_PADDING + 28.0;
        render_button(cmds, px, btn_y, 100.0, 28.0, "Upload File", BLUE);
        render_button(cmds, px + 110.0, btn_y, 120.0, 28.0, "Clear Finished", OVERLAY0);

        // Transfer list
        let list_y = btn_y + 40.0;
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: list_y,
            width: WINDOW_WIDTH,
            height: content_h - (list_y - content_y),
        });

        if self.transfers.is_empty() {
            cmds.push(RenderCommand::Text {
                x: px + 40.0,
                y: list_y + 20.0,
                text: "No file transfers. Drag files here to start.".into(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(pw - 80.0),
            });
        } else {
            let mut ty = list_y;
            for transfer in &self.transfers {
                self.render_transfer_item(cmds, transfer, px, ty, pw);
                ty += TRANSFER_ITEM_HEIGHT + 4.0;
            }
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_transfer_item(
        &self,
        cmds: &mut Vec<RenderCommand>,
        transfer: &FileTransfer,
        px: f32,
        ty: f32,
        pw: f32,
    ) {
        // Background card
        cmds.push(RenderCommand::FillRect {
            x: px,
            y: ty,
            width: pw,
            height: TRANSFER_ITEM_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Direction arrow
        let arrow = match transfer.direction {
            TransferDirection::Upload => "^",
            TransferDirection::Download => "v",
        };
        let arrow_color = match transfer.direction {
            TransferDirection::Upload => BLUE,
            TransferDirection::Download => GREEN,
        };
        cmds.push(RenderCommand::Text {
            x: px + 10.0,
            y: ty + 8.0,
            text: arrow.into(),
            font_size: 16.0,
            color: arrow_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(20.0),
        });

        // Filename
        cmds.push(RenderCommand::Text {
            x: px + 30.0,
            y: ty + 6.0,
            text: transfer.filename.clone(),
            font_size: 13.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(pw - 200.0),
        });

        // Size and status
        cmds.push(RenderCommand::Text {
            x: px + 30.0,
            y: ty + 24.0,
            text: format!(
                "{} / {} - {}",
                transfer.transferred_label(),
                transfer.size_label(),
                transfer.state.label()
            ),
            font_size: 11.0,
            color: transfer.state.color(),
            font_weight: FontWeightHint::Regular,
            max_width: Some(pw - 200.0),
        });

        // Progress bar
        let bar_x = pw - 160.0;
        let bar_w = 140.0;
        let bar_h = 8.0;
        let bar_y = ty + 20.0;

        // Background track
        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: bar_y,
            width: bar_w,
            height: bar_h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });

        // Progress fill
        let progress = transfer.progress_percent() / 100.0;
        if progress > 0.0 {
            let fill_w = bar_w * progress;
            cmds.push(RenderCommand::FillRect {
                x: bar_x,
                y: bar_y,
                width: fill_w,
                height: bar_h,
                color: transfer.state.color(),
                corner_radii: CornerRadii::all(4.0),
            });
        }

        // Percentage text
        cmds.push(RenderCommand::Text {
            x: bar_x + bar_w + 8.0,
            y: ty + 17.0,
            text: format!("{:.0}%", transfer.progress_percent()),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(40.0),
        });
    }

    fn render_history(&self, cmds: &mut Vec<RenderCommand>, content_y: f32, content_h: f32) {
        let px = SECTION_PADDING;
        let pw = WINDOW_WIDTH - 2.0 * SECTION_PADDING;

        // Header
        cmds.push(RenderCommand::Text {
            x: px,
            y: content_y + SECTION_PADDING,
            text: "Connection History".into(),
            font_size: 16.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(pw),
        });

        render_button(
            cmds,
            WINDOW_WIDTH - SECTION_PADDING - 100.0,
            content_y + SECTION_PADDING - 2.0,
            100.0,
            28.0,
            "Clear All",
            RED,
        );

        let list_y = content_y + SECTION_PADDING + 32.0;
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: list_y,
            width: WINDOW_WIDTH,
            height: content_h - (list_y - content_y),
        });

        if self.history.is_empty() {
            cmds.push(RenderCommand::Text {
                x: px + 40.0,
                y: list_y + 20.0,
                text: "No connection history".into(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(pw - 80.0),
            });
        } else {
            let mut hy = list_y;
            for entry in &self.history {
                self.render_history_entry(cmds, entry, px, hy, pw);
                hy += HISTORY_ITEM_HEIGHT + 4.0;
            }
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_history_entry(
        &self,
        cmds: &mut Vec<RenderCommand>,
        entry: &HistoryEntry,
        px: f32,
        hy: f32,
        pw: f32,
    ) {
        cmds.push(RenderCommand::FillRect {
            x: px,
            y: hy,
            width: pw,
            height: HISTORY_ITEM_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Status dot
        let status_color = if entry.success { GREEN } else { RED };
        cmds.push(RenderCommand::FillRect {
            x: px + 10.0,
            y: hy + 16.0,
            width: 10.0,
            height: 10.0,
            color: status_color,
            corner_radii: CornerRadii::all(5.0),
        });

        // Protocol badge
        cmds.push(RenderCommand::FillRect {
            x: px + 26.0,
            y: hy + 6.0,
            width: 36.0,
            height: 18.0,
            color: protocol_badge_bg(entry.protocol),
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: px + 30.0,
            y: hy + 8.0,
            text: entry.protocol.label().into(),
            font_size: 10.0,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(28.0),
        });

        // Name and host
        cmds.push(RenderCommand::Text {
            x: px + 70.0,
            y: hy + 6.0,
            text: entry.profile_name.clone(),
            font_size: 13.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

        cmds.push(RenderCommand::Text {
            x: px + 70.0,
            y: hy + 24.0,
            text: format!("{} - {}", entry.hostname, format_duration(entry.duration_secs)),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

        // Quick connect button
        render_button(
            cmds,
            pw - 80.0,
            hy + 8.0,
            80.0,
            28.0,
            "Connect",
            BLUE,
        );
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: WINDOW_WIDTH,
            y2: y,
            color: SURFACE0,
            width: 1.0,
        });

        // Connection count
        let active_count = self
            .sessions
            .iter()
            .filter(|s| s.state == SessionState::Connected)
            .count();
        cmds.push(RenderCommand::Text {
            x: SECTION_PADDING,
            y: y + 7.0,
            text: format!("{active_count} active"),
            font_size: 11.0,
            color: if active_count > 0 { GREEN } else { OVERLAY0 },
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });

        // Profile count
        cmds.push(RenderCommand::Text {
            x: 120.0,
            y: y + 7.0,
            text: format!("{} profiles", self.profiles.len()),
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });

        // Transfer count
        let active_transfers = self
            .transfers
            .iter()
            .filter(|t| t.state == TransferState::InProgress || t.state == TransferState::Queued)
            .count();
        if active_transfers > 0 {
            cmds.push(RenderCommand::Text {
                x: 250.0,
                y: y + 7.0,
                text: format!("{active_transfers} transfers"),
                font_size: 11.0,
                color: BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(100.0),
            });
        }

        // Status message
        if let Some(msg) = &self.status_message {
            cmds.push(RenderCommand::Text {
                x: 380.0,
                y: y + 7.0,
                text: msg.clone(),
                font_size: 11.0,
                color: YELLOW,
                font_weight: FontWeightHint::Regular,
                max_width: Some(300.0),
            });
        }

        // Latency on right side
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 160.0,
            y: y + 7.0,
            text: format!("{:.1}ms", self.perf_metrics.latency_ms),
            font_size: 11.0,
            color: latency_color(self.perf_metrics.latency_ms),
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });

        // FPS
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 80.0,
            y: y + 7.0,
            text: format!("{:.0} fps", self.perf_metrics.frame_rate),
            font_size: 11.0,
            color: fps_color(self.perf_metrics.frame_rate),
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });
    }

    fn render_perf_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let ox = WINDOW_WIDTH - 240.0;
        let oy = TITLE_BAR_HEIGHT + TOOLBAR_HEIGHT + TAB_HEIGHT + 10.0;
        let ow = 220.0;
        let oh = 160.0;

        // Semi-transparent background
        cmds.push(RenderCommand::BoxShadow {
            x: ox,
            y: oy,
            width: ow,
            height: oh,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 12.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 120),
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::FillRect {
            x: ox,
            y: oy,
            width: ow,
            height: oh,
            color: Color::rgba(17, 17, 27, 220),
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::StrokeRect {
            x: ox,
            y: oy,
            width: ow,
            height: oh,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: ox + 10.0,
            y: oy + 8.0,
            text: "Performance".into(),
            font_size: 12.0,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(ow - 20.0),
        });

        let metrics: Vec<(&str, String, Color)> = vec![
            (
                "Bandwidth",
                format!("{:.1} Kbps", self.perf_metrics.bandwidth_kbps),
                BLUE,
            ),
            (
                "Latency",
                format!("{:.1} ms", self.perf_metrics.latency_ms),
                latency_color(self.perf_metrics.latency_ms),
            ),
            (
                "Frame Rate",
                format!("{:.1} fps", self.perf_metrics.frame_rate),
                fps_color(self.perf_metrics.frame_rate),
            ),
            (
                "Sent",
                format_bytes(self.perf_metrics.bytes_sent),
                SUBTEXT0,
            ),
            (
                "Received",
                format_bytes(self.perf_metrics.bytes_received),
                SUBTEXT0,
            ),
            (
                "Packets",
                format!(
                    "{}/{}",
                    self.perf_metrics.packets_sent, self.perf_metrics.packets_received
                ),
                SUBTEXT0,
            ),
        ];

        let mut my = oy + 28.0;
        for (label, value, color) in &metrics {
            cmds.push(RenderCommand::Text {
                x: ox + 10.0,
                y: my,
                text: (*label).into(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(90.0),
            });
            cmds.push(RenderCommand::Text {
                x: ox + 100.0,
                y: my,
                text: value.clone(),
                font_size: 11.0,
                color: *color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(ow - 110.0),
            });
            my += 20.0;
        }
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Render a labeled field row.
fn render_field_row(
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    _width: f32,
    label: &str,
    value: &str,
) {
    cmds.push(RenderCommand::Text {
        x,
        y: y + 5.0,
        text: label.into(),
        font_size: 12.0,
        color: SUBTEXT0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(FIELD_LABEL_WIDTH),
    });
    cmds.push(RenderCommand::FillRect {
        x: x + FIELD_LABEL_WIDTH,
        y,
        width: 260.0,
        height: FIELD_HEIGHT,
        color: SURFACE0,
        corner_radii: CornerRadii::all(4.0),
    });
    cmds.push(RenderCommand::Text {
        x: x + FIELD_LABEL_WIDTH + 8.0,
        y: y + 6.0,
        text: value.into(),
        font_size: 12.0,
        color: TEXT_COLOR,
        font_weight: FontWeightHint::Regular,
        max_width: Some(244.0),
    });
}

/// Render a simple button.
fn render_button(
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    color: Color,
) {
    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color: SURFACE0,
        corner_radii: CornerRadii::all(6.0),
    });
    cmds.push(RenderCommand::StrokeRect {
        x,
        y,
        width: w,
        height: h,
        color,
        line_width: 1.0,
        corner_radii: CornerRadii::all(6.0),
    });
    cmds.push(RenderCommand::Text {
        x: x + 10.0,
        y: y + (h - 12.0) / 2.0,
        text: label.into(),
        font_size: 12.0,
        color,
        font_weight: FontWeightHint::Bold,
        max_width: Some(w - 20.0),
    });
}

/// Protocol badge background color.
fn protocol_badge_bg(proto: Protocol) -> Color {
    proto.color()
}

/// Color for latency value (green = good, yellow = ok, red = bad).
fn latency_color(ms: f32) -> Color {
    if ms < 30.0 {
        GREEN
    } else if ms < 100.0 {
        YELLOW
    } else {
        RED
    }
}

/// Color for frame rate value (green = good, yellow = ok, red = bad).
fn fps_color(fps: f32) -> Color {
    if fps >= 50.0 {
        GREEN
    } else if fps >= 25.0 {
        YELLOW
    } else {
        RED
    }
}

/// Format bytes to human-readable size.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Format duration in seconds to a human-readable string.
fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut app = RemoteDesktopApp::new();
    let _cmds = app.render();

    // The actual event loop would be driven by the compositor.
    // For now, verify rendering and event handling work.
    let test_event = Event::Key(KeyEvent {
        key: Key::F11,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: None,
    });
    let _result = app.handle_event(&test_event);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ====================================================================
    // Profile CRUD tests
    // ====================================================================

    #[test]
    fn test_add_profile_assigns_unique_id() {
        let mut app = RemoteDesktopApp::new();
        let initial_count = app.profiles.len();
        let profile = ConnectionProfile::new_default(0);
        let id = app.add_profile(profile);
        assert!(id > 0);
        assert_eq!(app.profiles.len(), initial_count + 1);
    }

    #[test]
    fn test_add_multiple_profiles_unique_ids() {
        let mut app = RemoteDesktopApp::new();
        let id1 = app.add_profile(ConnectionProfile::new_default(0));
        let id2 = app.add_profile(ConnectionProfile::new_default(0));
        let id3 = app.add_profile(ConnectionProfile::new_default(0));
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_delete_profile_valid_index() {
        let mut app = RemoteDesktopApp::new();
        let initial_count = app.profiles.len();
        assert!(app.delete_profile(0));
        assert_eq!(app.profiles.len(), initial_count - 1);
    }

    #[test]
    fn test_delete_profile_invalid_index() {
        let mut app = RemoteDesktopApp::new();
        assert!(!app.delete_profile(999));
    }

    #[test]
    fn test_delete_all_profiles_clears_selection() {
        let mut app = RemoteDesktopApp::new();
        while !app.profiles.is_empty() {
            app.delete_profile(0);
        }
        assert!(app.selected_profile.is_none());
    }

    #[test]
    fn test_update_profile_valid() {
        let mut app = RemoteDesktopApp::new();
        let mut updated = app.profiles[0].clone();
        updated.display_name = "Updated Name".into();
        assert!(app.update_profile(0, updated));
        assert_eq!(app.profiles[0].display_name, "Updated Name");
    }

    #[test]
    fn test_update_profile_invalid_index() {
        let mut app = RemoteDesktopApp::new();
        let profile = ConnectionProfile::new_default(0);
        assert!(!app.update_profile(999, profile));
    }

    #[test]
    fn test_get_profile_valid() {
        let app = RemoteDesktopApp::new();
        assert!(app.get_profile(0).is_some());
    }

    #[test]
    fn test_get_profile_invalid() {
        let app = RemoteDesktopApp::new();
        assert!(app.get_profile(999).is_none());
    }

    #[test]
    fn test_find_profile_by_id() {
        let app = RemoteDesktopApp::new();
        let idx = app.find_profile_by_id(1);
        assert!(idx.is_some());
        assert_eq!(idx.unwrap(), 0);
    }

    #[test]
    fn test_find_profile_by_id_missing() {
        let app = RemoteDesktopApp::new();
        assert!(app.find_profile_by_id(9999).is_none());
    }

    #[test]
    fn test_groups_returns_unique_sorted() {
        let app = RemoteDesktopApp::new();
        let groups = app.groups();
        // Should be deduplicated and sorted
        let mut sorted = groups.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(groups, sorted);
    }

    #[test]
    fn test_profiles_in_group() {
        let app = RemoteDesktopApp::new();
        let dev_profiles = app.profiles_in_group("Development");
        assert!(!dev_profiles.is_empty());
        for &idx in &dev_profiles {
            assert_eq!(app.profiles[idx].group, "Development");
        }
    }

    #[test]
    fn test_profiles_in_nonexistent_group() {
        let app = RemoteDesktopApp::new();
        let profiles = app.profiles_in_group("NonExistent");
        assert!(profiles.is_empty());
    }

    #[test]
    fn test_profile_default_values() {
        let profile = ConnectionProfile::new_default(42);
        assert_eq!(profile.id, 42);
        assert!(profile.display_name.is_empty());
        assert_eq!(profile.group, "Default");
        assert_eq!(profile.protocol, Protocol::Rdp);
        assert_eq!(profile.port, 3389);
    }

    // ====================================================================
    // Protocol settings tests
    // ====================================================================

    #[test]
    fn test_protocol_labels() {
        assert_eq!(Protocol::Rdp.label(), "RDP");
        assert_eq!(Protocol::Vnc.label(), "VNC");
        assert_eq!(Protocol::Ssh.label(), "SSH");
    }

    #[test]
    fn test_protocol_default_ports() {
        assert_eq!(Protocol::Rdp.default_port(), 3389);
        assert_eq!(Protocol::Vnc.default_port(), 5900);
        assert_eq!(Protocol::Ssh.default_port(), 22);
    }

    #[test]
    fn test_protocol_colors_differ() {
        let c1 = Protocol::Rdp.color();
        let c2 = Protocol::Vnc.color();
        let c3 = Protocol::Ssh.color();
        assert_ne!(c1, c2);
        assert_ne!(c2, c3);
    }

    // ====================================================================
    // Display config tests
    // ====================================================================

    #[test]
    fn test_display_settings_default() {
        let ds = DisplaySettings::default();
        assert_eq!(ds.scaling, ScalingMode::AutoFit);
        assert_eq!(ds.color_depth, ColorDepth::Bit24);
        assert_eq!(ds.refresh_rate, 30);
        assert_eq!(ds.width, 1920);
        assert_eq!(ds.height, 1080);
    }

    #[test]
    fn test_scaling_mode_labels() {
        assert_eq!(ScalingMode::AutoFit.label(), "Auto-fit");
        assert_eq!(ScalingMode::Fixed100.label(), "100%");
        assert_eq!(ScalingMode::Custom(75).label(), "75%");
    }

    #[test]
    fn test_color_depth_bits() {
        assert_eq!(ColorDepth::Bit8.bits(), 8);
        assert_eq!(ColorDepth::Bit16.bits(), 16);
        assert_eq!(ColorDepth::Bit24.bits(), 24);
        assert_eq!(ColorDepth::Bit32.bits(), 32);
    }

    #[test]
    fn test_color_depth_labels() {
        assert!(ColorDepth::Bit8.label().contains("256"));
        assert!(ColorDepth::Bit32.label().contains("Full"));
    }

    // ====================================================================
    // Quality preset tests
    // ====================================================================

    #[test]
    fn test_quality_preset_labels() {
        assert_eq!(QualityPreset::Auto.label(), "Auto");
        assert_eq!(QualityPreset::LowBandwidth.label(), "Low Bandwidth");
        assert_eq!(QualityPreset::Balanced.label(), "Balanced");
        assert_eq!(QualityPreset::HighQuality.label(), "High Quality");
    }

    #[test]
    fn test_quality_preset_settings_auto() {
        let (depth, rate, scaling) = QualityPreset::Auto.settings();
        assert_eq!(depth, ColorDepth::Bit24);
        assert_eq!(rate, 30);
        assert_eq!(scaling, ScalingMode::AutoFit);
    }

    #[test]
    fn test_quality_preset_settings_low() {
        let (depth, rate, _scaling) = QualityPreset::LowBandwidth.settings();
        assert_eq!(depth, ColorDepth::Bit8);
        assert_eq!(rate, 15);
    }

    #[test]
    fn test_quality_preset_settings_high() {
        let (depth, rate, _scaling) = QualityPreset::HighQuality.settings();
        assert_eq!(depth, ColorDepth::Bit32);
        assert_eq!(rate, 60);
    }

    #[test]
    fn test_apply_quality_preset() {
        let mut app = RemoteDesktopApp::new();
        app.selected_profile = Some(0);
        app.apply_quality_preset(QualityPreset::HighQuality);
        let profile = &app.profiles[0];
        assert_eq!(profile.quality, QualityPreset::HighQuality);
        assert_eq!(profile.display.color_depth, ColorDepth::Bit32);
        assert_eq!(profile.display.refresh_rate, 60);
    }

    #[test]
    fn test_quality_preset_colors_differ() {
        let c1 = QualityPreset::Auto.color();
        let c2 = QualityPreset::LowBandwidth.color();
        let c3 = QualityPreset::Balanced.color();
        let c4 = QualityPreset::HighQuality.color();
        assert_ne!(c1, c2);
        assert_ne!(c3, c4);
    }

    // ====================================================================
    // Session state machine tests
    // ====================================================================

    #[test]
    fn test_connect_profile_creates_session() {
        let mut app = RemoteDesktopApp::new();
        let initial_sessions = app.sessions.len();
        let result = app.connect_profile(0);
        assert!(result.is_some());
        assert_eq!(app.sessions.len(), initial_sessions + 1);
    }

    #[test]
    fn test_connect_invalid_profile() {
        let mut app = RemoteDesktopApp::new();
        assert!(app.connect_profile(999).is_none());
    }

    #[test]
    fn test_session_initial_state_connecting() {
        let mut app = RemoteDesktopApp::new();
        let _id = app.connect_profile(0);
        let session = app.sessions.last().unwrap();
        assert_eq!(session.state, SessionState::Connecting);
    }

    #[test]
    fn test_advance_session_connecting_to_authenticating() {
        let mut app = RemoteDesktopApp::new();
        let _id = app.connect_profile(0);
        let last_idx = app.sessions.len() - 1;
        app.advance_session_state(last_idx);
        assert_eq!(app.sessions[last_idx].state, SessionState::Authenticating);
    }

    #[test]
    fn test_advance_session_authenticating_to_connected() {
        let mut app = RemoteDesktopApp::new();
        let _id = app.connect_profile(0);
        let last_idx = app.sessions.len() - 1;
        app.advance_session_state(last_idx); // -> Authenticating
        app.advance_session_state(last_idx); // -> Connected
        assert_eq!(app.sessions[last_idx].state, SessionState::Connected);
    }

    #[test]
    fn test_disconnect_session() {
        let mut app = RemoteDesktopApp::new();
        assert!(app.disconnect_session(0));
        assert_eq!(app.sessions[0].state, SessionState::Disconnected);
    }

    #[test]
    fn test_disconnect_invalid_session() {
        let mut app = RemoteDesktopApp::new();
        assert!(!app.disconnect_session(999));
    }

    #[test]
    fn test_reconnect_session() {
        let mut app = RemoteDesktopApp::new();
        app.disconnect_session(0);
        assert!(app.reconnect_session(0));
        assert_eq!(app.sessions[0].state, SessionState::Reconnecting);
    }

    #[test]
    fn test_reconnect_advances_to_connected() {
        let mut app = RemoteDesktopApp::new();
        app.sessions[0].state = SessionState::Reconnecting;
        app.advance_session_state(0);
        assert_eq!(app.sessions[0].state, SessionState::Connected);
    }

    #[test]
    fn test_cleanup_sessions_removes_disconnected() {
        let mut app = RemoteDesktopApp::new();
        app.disconnect_session(0);
        app.cleanup_sessions();
        assert!(app
            .sessions
            .iter()
            .all(|s| s.state != SessionState::Disconnected));
    }

    #[test]
    fn test_session_state_labels() {
        assert_eq!(SessionState::Connected.label(), "Connected");
        assert_eq!(SessionState::Disconnected.label(), "Disconnected");
        assert_eq!(SessionState::Connecting.label(), "Connecting...");
        assert_eq!(SessionState::Error.label(), "Error");
    }

    #[test]
    fn test_session_state_colors_differ() {
        let c1 = SessionState::Connected.color();
        let c2 = SessionState::Disconnected.color();
        let c3 = SessionState::Error.color();
        assert_ne!(c1, c2);
        assert_ne!(c1, c3);
    }

    // ====================================================================
    // Clipboard sync state tests
    // ====================================================================

    #[test]
    fn test_clipboard_default_bidirectional() {
        let app = RemoteDesktopApp::new();
        assert_eq!(app.clipboard.mode, ClipboardSyncMode::Bidirectional);
    }

    #[test]
    fn test_clipboard_sync_updates_state() {
        let mut app = RemoteDesktopApp::new();
        app.sync_clipboard("local->remote", ClipboardContentType::Text, 256);
        assert_eq!(
            app.clipboard.last_sync_direction,
            Some("local->remote")
        );
        assert_eq!(app.clipboard.content_type, ClipboardContentType::Text);
        assert_eq!(app.clipboard.content_size_bytes, 256);
        assert_eq!(app.clipboard.sync_count, 1);
    }

    #[test]
    fn test_clipboard_sync_disabled_noop() {
        let mut app = RemoteDesktopApp::new();
        app.clipboard.mode = ClipboardSyncMode::Disabled;
        app.sync_clipboard("local->remote", ClipboardContentType::Text, 100);
        assert!(app.clipboard.last_sync_direction.is_none());
        assert_eq!(app.clipboard.sync_count, 0);
    }

    #[test]
    fn test_clipboard_sync_increments_count() {
        let mut app = RemoteDesktopApp::new();
        app.sync_clipboard("local->remote", ClipboardContentType::Text, 10);
        app.sync_clipboard("remote->local", ClipboardContentType::Image, 5000);
        assert_eq!(app.clipboard.sync_count, 2);
        assert_eq!(
            app.clipboard.last_sync_direction,
            Some("remote->local")
        );
    }

    #[test]
    fn test_clipboard_sync_mode_labels() {
        assert_eq!(ClipboardSyncMode::Disabled.label(), "Disabled");
        assert_eq!(ClipboardSyncMode::Bidirectional.label(), "Bidirectional");
        assert_eq!(
            ClipboardSyncMode::LocalToRemote.label(),
            "Local -> Remote"
        );
        assert_eq!(
            ClipboardSyncMode::RemoteToLocal.label(),
            "Remote -> Local"
        );
    }

    // ====================================================================
    // File transfer queue tests
    // ====================================================================

    #[test]
    fn test_queue_transfer() {
        let mut app = RemoteDesktopApp::new();
        let initial = app.transfers.len();
        let id = app.queue_transfer("test.txt".into(), 1024, TransferDirection::Upload);
        assert!(id > 0);
        assert_eq!(app.transfers.len(), initial + 1);
    }

    #[test]
    fn test_transfer_initial_state_queued() {
        let mut app = RemoteDesktopApp::new();
        let _id = app.queue_transfer("test.txt".into(), 1024, TransferDirection::Upload);
        let transfer = app.transfers.last().unwrap();
        assert_eq!(transfer.state, TransferState::Queued);
        assert_eq!(transfer.transferred_bytes, 0);
    }

    #[test]
    fn test_update_transfer_progress() {
        let mut app = RemoteDesktopApp::new();
        let _id = app.queue_transfer("test.txt".into(), 1000, TransferDirection::Download);
        let idx = app.transfers.len() - 1;
        app.update_transfer_progress(idx, 500);
        assert_eq!(app.transfers[idx].transferred_bytes, 500);
        assert_eq!(app.transfers[idx].state, TransferState::InProgress);
    }

    #[test]
    fn test_transfer_completes_at_full() {
        let mut app = RemoteDesktopApp::new();
        let _id = app.queue_transfer("test.txt".into(), 100, TransferDirection::Upload);
        let idx = app.transfers.len() - 1;
        app.update_transfer_progress(idx, 100);
        assert_eq!(app.transfers[idx].state, TransferState::Completed);
    }

    #[test]
    fn test_transfer_progress_clamped() {
        let mut app = RemoteDesktopApp::new();
        let _id = app.queue_transfer("test.txt".into(), 100, TransferDirection::Upload);
        let idx = app.transfers.len() - 1;
        app.update_transfer_progress(idx, 200); // over the size
        assert_eq!(app.transfers[idx].transferred_bytes, 100);
    }

    #[test]
    fn test_cancel_transfer() {
        let mut app = RemoteDesktopApp::new();
        let _id = app.queue_transfer("test.txt".into(), 1000, TransferDirection::Upload);
        let idx = app.transfers.len() - 1;
        assert!(app.cancel_transfer(idx));
        assert_eq!(app.transfers[idx].state, TransferState::Cancelled);
    }

    #[test]
    fn test_cancel_completed_transfer_fails() {
        let mut app = RemoteDesktopApp::new();
        let _id = app.queue_transfer("test.txt".into(), 100, TransferDirection::Upload);
        let idx = app.transfers.len() - 1;
        app.update_transfer_progress(idx, 100);
        assert!(!app.cancel_transfer(idx));
    }

    #[test]
    fn test_clear_finished_transfers() {
        let mut app = RemoteDesktopApp::new();
        // has sample data with various states
        app.clear_finished_transfers();
        for t in &app.transfers {
            assert!(
                t.state == TransferState::Queued || t.state == TransferState::InProgress
            );
        }
    }

    #[test]
    fn test_file_transfer_progress_percent() {
        let t = FileTransfer {
            id: 1,
            filename: "test".into(),
            size_bytes: 200,
            transferred_bytes: 100,
            direction: TransferDirection::Upload,
            state: TransferState::InProgress,
        };
        let pct = t.progress_percent();
        assert!((pct - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_file_transfer_progress_zero_size() {
        let t = FileTransfer {
            id: 1,
            filename: "empty".into(),
            size_bytes: 0,
            transferred_bytes: 0,
            direction: TransferDirection::Upload,
            state: TransferState::Completed,
        };
        assert!((t.progress_percent() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_transfer_state_labels() {
        assert_eq!(TransferState::Queued.label(), "Queued");
        assert_eq!(TransferState::InProgress.label(), "Transferring");
        assert_eq!(TransferState::Completed.label(), "Completed");
        assert_eq!(TransferState::Failed.label(), "Failed");
        assert_eq!(TransferState::Cancelled.label(), "Cancelled");
    }

    // ====================================================================
    // History tests
    // ====================================================================

    #[test]
    fn test_history_loaded_from_sample() {
        let app = RemoteDesktopApp::new();
        assert!(!app.history.is_empty());
    }

    #[test]
    fn test_add_history_entry() {
        let mut app = RemoteDesktopApp::new();
        let initial = app.history.len();
        app.add_history_entry(HistoryEntry {
            profile_name: "Test".into(),
            hostname: "test.local".into(),
            protocol: Protocol::Ssh,
            timestamp: 100,
            duration_secs: 60,
            success: true,
        });
        assert_eq!(app.history.len(), initial + 1);
        assert_eq!(app.history.front().unwrap().profile_name, "Test");
    }

    #[test]
    fn test_clear_history() {
        let mut app = RemoteDesktopApp::new();
        app.clear_history();
        assert!(app.history.is_empty());
    }

    #[test]
    fn test_recent_history() {
        let app = RemoteDesktopApp::new();
        let recent = app.recent_history(2);
        assert!(recent.len() <= 2);
    }

    #[test]
    fn test_history_trimmed_at_cap() {
        let mut app = RemoteDesktopApp::new();
        for i in 0..110 {
            app.add_history_entry(HistoryEntry {
                profile_name: format!("Entry {i}"),
                hostname: "host".into(),
                protocol: Protocol::Rdp,
                timestamp: i as u64,
                duration_secs: 0,
                success: true,
            });
        }
        assert!(app.history.len() <= 100);
    }

    #[test]
    fn test_connect_profile_adds_history() {
        let mut app = RemoteDesktopApp::new();
        let old_len = app.history.len();
        let _id = app.connect_profile(0);
        assert_eq!(app.history.len(), old_len + 1);
    }

    // ====================================================================
    // Input mapping tests
    // ====================================================================

    #[test]
    fn test_input_config_default() {
        let cfg = InputConfig::default();
        assert!(cfg.forward_keyboard);
        assert!(cfg.forward_mouse);
        assert!(!cfg.grab_keyboard);
        assert!(!cfg.key_mappings.is_empty());
    }

    #[test]
    fn test_key_mapping_labels() {
        let cfg = InputConfig::default();
        for m in &cfg.key_mappings {
            assert!(!m.label.is_empty());
        }
    }

    #[test]
    fn test_key_event_navigation_up() {
        let mut app = RemoteDesktopApp::new();
        app.selected_profile = Some(2);
        app.current_view = MainView::Connections;
        let event = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.selected_profile, Some(1));
    }

    #[test]
    fn test_key_event_navigation_down() {
        let mut app = RemoteDesktopApp::new();
        app.selected_profile = Some(0);
        app.current_view = MainView::Connections;
        let event = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.selected_profile, Some(1));
    }

    #[test]
    fn test_key_event_navigation_up_at_top() {
        let mut app = RemoteDesktopApp::new();
        app.selected_profile = Some(0);
        app.current_view = MainView::Connections;
        let event = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.selected_profile, Some(0));
    }

    #[test]
    fn test_ctrl_n_creates_profile() {
        let mut app = RemoteDesktopApp::new();
        let initial = app.profiles.len();
        let event = Event::Key(KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.profiles.len(), initial + 1);
    }

    // ====================================================================
    // Multi-monitor config tests
    // ====================================================================

    #[test]
    fn test_monitor_mode_span_label() {
        let mode = MonitorMode::SpanAll;
        assert_eq!(mode.label(), "Span All");
    }

    #[test]
    fn test_monitor_mode_single_label() {
        let mode = MonitorMode::SingleMonitor(0);
        assert_eq!(mode.label(), "Monitor 1");
    }

    #[test]
    fn test_set_monitor_mode() {
        let mut app = RemoteDesktopApp::new();
        app.set_monitor_mode(MonitorMode::SingleMonitor(1));
        assert_eq!(app.monitor_mode, MonitorMode::SingleMonitor(1));
    }

    #[test]
    fn test_remote_monitors_loaded() {
        let app = RemoteDesktopApp::new();
        assert!(!app.remote_monitors.is_empty());
    }

    #[test]
    fn test_remote_monitor_has_primary() {
        let app = RemoteDesktopApp::new();
        assert!(app.remote_monitors.iter().any(|m| m.primary));
    }

    // ====================================================================
    // Rendering output tests
    // ====================================================================

    #[test]
    fn test_render_produces_commands() {
        let app = RemoteDesktopApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_starts_with_background() {
        let app = RemoteDesktopApp::new();
        let cmds = app.render();
        match &cmds[0] {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                ..
            } => {
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 0.0);
                assert_eq!(*width, WINDOW_WIDTH);
                assert_eq!(*height, WINDOW_HEIGHT);
                assert_eq!(*color, BASE);
            }
            _ => panic!("First command should be background FillRect"),
        }
    }

    #[test]
    fn test_render_connections_view() {
        let mut app = RemoteDesktopApp::new();
        app.current_view = MainView::Connections;
        let cmds = app.render();
        // Should produce a reasonable number of commands
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_sessions_view() {
        let mut app = RemoteDesktopApp::new();
        app.current_view = MainView::ActiveSessions;
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_file_transfer_view() {
        let mut app = RemoteDesktopApp::new();
        app.current_view = MainView::FileTransfer;
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_history_view() {
        let mut app = RemoteDesktopApp::new();
        app.current_view = MainView::History;
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_perf_overlay() {
        let mut app = RemoteDesktopApp::new();
        app.show_perf_overlay = true;
        let cmds = app.render();
        // Should include overlay rendering (more commands)
        assert!(cmds.len() > 30);
    }

    #[test]
    fn test_render_empty_sessions_view() {
        let mut app = RemoteDesktopApp::new();
        app.sessions.clear();
        app.current_view = MainView::ActiveSessions;
        let cmds = app.render();
        // Should have a message about no active sessions
        let has_text = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("No active sessions")
            } else {
                false
            }
        });
        assert!(has_text);
    }

    #[test]
    fn test_render_no_profile_selected() {
        let mut app = RemoteDesktopApp::new();
        app.selected_profile = None;
        app.current_view = MainView::Connections;
        let cmds = app.render();
        let has_empty_state = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Select a connection")
            } else {
                false
            }
        });
        assert!(has_empty_state);
    }

    #[test]
    fn test_render_fullscreen_indicator() {
        let mut app = RemoteDesktopApp::new();
        app.fullscreen = true;
        let cmds = app.render();
        let has_indicator = cmds.iter().any(|c| {
            if let RenderCommand::Text { text, .. } = c {
                text.contains("Fullscreen")
            } else {
                false
            }
        });
        assert!(has_indicator);
    }

    // ====================================================================
    // Screenshot and fullscreen tests
    // ====================================================================

    #[test]
    fn test_capture_screenshot() {
        let mut app = RemoteDesktopApp::new();
        let name = app.capture_screenshot();
        assert!(name.contains("screenshot"));
        assert!(name.contains(".png"));
        assert!(app.status_message.is_some());
    }

    #[test]
    fn test_toggle_fullscreen() {
        let mut app = RemoteDesktopApp::new();
        assert!(!app.fullscreen);
        app.toggle_fullscreen();
        assert!(app.fullscreen);
        app.toggle_fullscreen();
        assert!(!app.fullscreen);
    }

    #[test]
    fn test_set_escape_hotkey() {
        let mut app = RemoteDesktopApp::new();
        app.set_escape_hotkey(Key::Escape);
        assert_eq!(app.escape_hotkey, Key::Escape);
    }

    #[test]
    fn test_fullscreen_exit_via_hotkey() {
        let mut app = RemoteDesktopApp::new();
        app.fullscreen = true;
        app.escape_hotkey = Key::F11;
        let event = Event::Key(KeyEvent {
            key: Key::F11,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        let result = app.handle_event(&event);
        assert_eq!(result, EventResult::Consumed);
        assert!(!app.fullscreen);
    }

    // ====================================================================
    // Misc / helper tests
    // ====================================================================

    #[test]
    fn test_format_bytes_b() {
        assert_eq!(format_bytes(100), "100 B");
    }

    #[test]
    fn test_format_bytes_kb() {
        let s = format_bytes(2048);
        assert!(s.contains("KB"));
    }

    #[test]
    fn test_format_bytes_mb() {
        let s = format_bytes(5_000_000);
        assert!(s.contains("MB"));
    }

    #[test]
    fn test_format_bytes_gb() {
        let s = format_bytes(2_000_000_000);
        assert!(s.contains("GB"));
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(45), "45s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(125), "2m 5s");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3661), "1h 1m 1s");
    }

    #[test]
    fn test_latency_color_good() {
        assert_eq!(latency_color(10.0), GREEN);
    }

    #[test]
    fn test_latency_color_medium() {
        assert_eq!(latency_color(50.0), YELLOW);
    }

    #[test]
    fn test_latency_color_bad() {
        assert_eq!(latency_color(150.0), RED);
    }

    #[test]
    fn test_fps_color_good() {
        assert_eq!(fps_color(60.0), GREEN);
    }

    #[test]
    fn test_fps_color_medium() {
        assert_eq!(fps_color(30.0), YELLOW);
    }

    #[test]
    fn test_fps_color_bad() {
        assert_eq!(fps_color(10.0), RED);
    }

    #[test]
    fn test_view_switching_ctrl_keys() {
        let mut app = RemoteDesktopApp::new();
        let views = [
            (Key::Num1, MainView::Connections),
            (Key::Num2, MainView::ActiveSessions),
            (Key::Num3, MainView::FileTransfer),
            (Key::Num4, MainView::History),
        ];
        for (key, expected_view) in &views {
            let event = Event::Key(KeyEvent {
                key: *key,
                pressed: true,
                modifiers: Modifiers::ctrl(),
                text: None,
            });
            app.handle_event(&event);
            assert_eq!(app.current_view, *expected_view);
        }
    }

    #[test]
    fn test_delete_key_sets_confirm() {
        let mut app = RemoteDesktopApp::new();
        app.selected_profile = Some(1);
        app.current_view = MainView::Connections;
        let event = Event::Key(KeyEvent {
            key: Key::Delete,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.confirm_delete, Some(1));
    }

    #[test]
    fn test_scroll_event_updates_sidebar() {
        let mut app = RemoteDesktopApp::new();
        let event = Event::Mouse(guitk::event::MouseEvent {
            x: 50.0, // inside sidebar
            y: 200.0,
            kind: MouseEventKind::Scroll {
                dx: 0.0,
                dy: -10.0,
            },
        });
        app.handle_event(&event);
        assert!(app.sidebar_scroll > 0.0);
    }
}

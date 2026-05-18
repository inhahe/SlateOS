//! DynDNS and remote desktop settings page.
//!
//! Provides configuration for dynamic DNS providers (NoIP, DuckDNS, Dynu,
//! FreeDNS, or custom update URLs) and remote desktop access (port, auth,
//! encryption, firewall/UPnP integration). Renders a settings UI using guitk.

#![allow(dead_code)]

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
use guitk::style::CornerRadii;

use core::fmt;

// ============================================================================
// Theme colors (same Catppuccin Mocha palette as main settings)
// ============================================================================

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_SURFACE2: Color = Color::from_hex(0x585B70);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
#[allow(dead_code)]
const COL_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const COL_ACCENT: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);

// ============================================================================
// Layout constants
// ============================================================================

const ROW_HEIGHT: f32 = 52.0;
const SECTION_SPACING: f32 = 24.0;
const FIELD_LABEL_WIDTH: f32 = 160.0;
const FIELD_INPUT_WIDTH: f32 = 280.0;
const FIELD_HEIGHT: f32 = 36.0;
const TOGGLE_WIDTH: f32 = 44.0;
const TOGGLE_HEIGHT: f32 = 24.0;
const CONTENT_WIDTH: f32 = 580.0;
const BUTTON_HEIGHT: f32 = 32.0;

/// Default DynDNS update interval in minutes.
const DEFAULT_UPDATE_INTERVAL: u32 = 30;

/// Default remote desktop port (RDP standard).
const DEFAULT_RDP_PORT: u16 = 3389;

/// Maximum remote desktop sessions.
const DEFAULT_MAX_SESSIONS: u32 = 2;

/// Default idle timeout in minutes.
const DEFAULT_IDLE_TIMEOUT: u32 = 30;

/// Minimum allowed port number.
const MIN_PORT: u16 = 1;

/// Maximum allowed port number.
const MAX_PORT: u16 = 65535;

// ============================================================================
// DynDNS types
// ============================================================================

/// Supported DynDNS providers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynDnsProvider {
    NoIP,
    DuckDNS,
    Dynu,
    FreeDNS,
    Custom,
}

impl DynDnsProvider {
    pub const ALL: &[Self] = &[
        Self::NoIP,
        Self::DuckDNS,
        Self::Dynu,
        Self::FreeDNS,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::NoIP => "No-IP",
            Self::DuckDNS => "DuckDNS",
            Self::Dynu => "Dynu",
            Self::FreeDNS => "FreeDNS",
            Self::Custom => "Custom",
        }
    }

    /// Default update URL template for this provider.
    ///
    /// Placeholders: `{hostname}`, `{username}`, `{password}`, `{token}`, `{ip}`.
    pub fn default_update_url(self) -> &'static str {
        match self {
            Self::NoIP => "https://dynupdate.no-ip.com/nic/update?hostname={hostname}&myip={ip}",
            Self::DuckDNS => "https://www.duckdns.org/update?domains={hostname}&token={token}&ip={ip}",
            Self::Dynu => "https://api.dynu.com/nic/update?hostname={hostname}&myip={ip}",
            Self::FreeDNS => "https://freedns.afraid.org/dynamic/update.php?{token}&address={ip}",
            Self::Custom => "",
        }
    }

    /// Whether this provider uses username/password auth (vs. token auth).
    pub fn uses_credentials(self) -> bool {
        matches!(self, Self::NoIP | Self::Dynu)
    }

    /// Whether this provider uses a token for authentication.
    pub fn uses_token(self) -> bool {
        matches!(self, Self::DuckDNS | Self::FreeDNS)
    }
}

impl fmt::Display for DynDnsProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Current status of the DynDNS updater.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DynDnsStatus {
    /// Not running.
    Idle,
    /// Currently updating the DNS record.
    Updating,
    /// Last update completed successfully.
    Success,
    /// Last update failed with the given reason.
    Error(String),
}

impl DynDnsStatus {
    pub fn label(&self) -> &str {
        match self {
            Self::Idle => "Idle",
            Self::Updating => "Updating...",
            Self::Success => "Up to date",
            Self::Error(_) => "Error",
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::Idle => COL_OVERLAY0,
            Self::Updating => COL_ACCENT,
            Self::Success => COL_GREEN,
            Self::Error(_) => COL_RED,
        }
    }
}

/// HTTP method for custom DynDNS update requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    pub const ALL: &[Self] = &[Self::Get, Self::Post];

    pub fn label(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Provider-specific credential/settings variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderSettings {
    /// NoIP: email + password.
    NoIP {
        hostname: String,
        email: String,
        password: String,
    },
    /// DuckDNS: domain + token.
    DuckDNS {
        domain: String,
        token: String,
    },
    /// Dynu: hostname + username + password.
    Dynu {
        hostname: String,
        username: String,
        password: String,
    },
    /// FreeDNS: domain + auth token.
    FreeDNS {
        domain: String,
        auth_token: String,
    },
    /// Custom: arbitrary URL template and HTTP method.
    Custom {
        update_url: String,
        method: HttpMethod,
    },
}

impl ProviderSettings {
    /// Create default settings for the given provider.
    pub fn default_for(provider: DynDnsProvider) -> Self {
        match provider {
            DynDnsProvider::NoIP => Self::NoIP {
                hostname: String::new(),
                email: String::new(),
                password: String::new(),
            },
            DynDnsProvider::DuckDNS => Self::DuckDNS {
                domain: String::new(),
                token: String::new(),
            },
            DynDnsProvider::Dynu => Self::Dynu {
                hostname: String::new(),
                username: String::new(),
                password: String::new(),
            },
            DynDnsProvider::FreeDNS => Self::FreeDNS {
                domain: String::new(),
                auth_token: String::new(),
            },
            DynDnsProvider::Custom => Self::Custom {
                update_url: String::new(),
                method: HttpMethod::Get,
            },
        }
    }

    /// Return which provider this settings variant corresponds to.
    pub fn provider(&self) -> DynDnsProvider {
        match self {
            Self::NoIP { .. } => DynDnsProvider::NoIP,
            Self::DuckDNS { .. } => DynDnsProvider::DuckDNS,
            Self::Dynu { .. } => DynDnsProvider::Dynu,
            Self::FreeDNS { .. } => DynDnsProvider::FreeDNS,
            Self::Custom { .. } => DynDnsProvider::Custom,
        }
    }

    /// Build the concrete update URL by substituting placeholders.
    ///
    /// For built-in providers, uses the provider's default URL template.
    /// For Custom, uses the user-supplied template.
    pub fn render_update_url(&self, ip: &str) -> String {
        let template = match self {
            Self::Custom { update_url, .. } => update_url.as_str(),
            _ => self.provider().default_update_url(),
        };

        let mut url = template.replace("{ip}", ip);

        match self {
            Self::NoIP { hostname, .. } => {
                url = url.replace("{hostname}", hostname);
            }
            Self::DuckDNS { domain, token } => {
                url = url.replace("{hostname}", domain);
                url = url.replace("{token}", token);
            }
            Self::Dynu { hostname, .. } => {
                url = url.replace("{hostname}", hostname);
            }
            Self::FreeDNS { auth_token, .. } => {
                url = url.replace("{token}", auth_token);
            }
            Self::Custom { .. } => {
                // Custom templates may use any placeholder; we only substitute {ip}
                // which is already done above. Users can embed other values directly
                // in their template.
            }
        }
        url
    }
}

/// DynDNS configuration.
#[derive(Clone, Debug)]
pub struct DynDnsConfig {
    /// Selected provider.
    pub provider: DynDnsProvider,
    /// General hostname (used for display; provider-specific settings hold the
    /// authoritative hostname/domain for URL rendering).
    pub hostname: String,
    /// Username (for providers that need it).
    pub username: String,
    /// Password (stored as encrypted reference in production; plain here for
    /// config plumbing).
    pub password: String,
    /// How often to push an IP update, in minutes.
    pub update_interval_minutes: u32,
    /// Whether the DynDNS updater is enabled.
    pub enabled: bool,
    /// Epoch timestamp of the last successful update, if any.
    pub last_update: Option<u64>,
    /// External IP from the last successful update, if known.
    pub last_ip: Option<String>,
    /// Current operational status.
    pub status: DynDnsStatus,
    /// Provider-specific credential/settings block.
    pub provider_settings: ProviderSettings,
}

impl DynDnsConfig {
    /// Create a new config with sensible defaults for the given provider.
    pub fn new(provider: DynDnsProvider) -> Self {
        Self {
            provider,
            hostname: String::new(),
            username: String::new(),
            password: String::new(),
            update_interval_minutes: DEFAULT_UPDATE_INTERVAL,
            enabled: false,
            last_update: None,
            last_ip: None,
            status: DynDnsStatus::Idle,
            provider_settings: ProviderSettings::default_for(provider),
        }
    }
}

impl Default for DynDnsConfig {
    fn default() -> Self {
        Self::new(DynDnsProvider::NoIP)
    }
}

// ============================================================================
// DynDNS manager
// ============================================================================

/// Manages DynDNS state: config, status, and update triggers.
pub struct DynDnsManager {
    config: DynDnsConfig,
}

impl DynDnsManager {
    pub fn new() -> Self {
        Self {
            config: DynDnsConfig::default(),
        }
    }

    pub fn with_config(config: DynDnsConfig) -> Self {
        Self { config }
    }

    /// Replace the entire configuration.
    pub fn set_config(&mut self, config: DynDnsConfig) {
        self.config = config;
    }

    /// Borrow the current configuration.
    pub fn config(&self) -> &DynDnsConfig {
        &self.config
    }

    /// Mutably borrow the configuration.
    pub fn config_mut(&mut self) -> &mut DynDnsConfig {
        &mut self.config
    }

    /// Trigger an immediate IP update.
    ///
    /// In a live system this would spawn a network request. Here we transition
    /// the status to `Updating` so the UI can reflect it.
    pub fn force_update(&mut self) {
        if !self.config.enabled {
            return;
        }
        self.config.status = DynDnsStatus::Updating;
    }

    /// Current status of the updater.
    pub fn status(&self) -> &DynDnsStatus {
        &self.config.status
    }

    /// Query the external IP address.
    ///
    /// Stub: returns a placeholder. A real implementation would contact an
    /// external service such as `https://api.ipify.org`.
    pub fn get_external_ip(&self) -> String {
        // Stub value for UI rendering and tests.
        "203.0.113.42".to_string()
    }

    /// Test whether the stored credentials work with the selected provider.
    ///
    /// Stub: succeeds if credentials are non-empty, errors otherwise.
    pub fn test_connection(&mut self) -> bool {
        let ok = match &self.config.provider_settings {
            ProviderSettings::NoIP { email, password, .. } => {
                !email.is_empty() && !password.is_empty()
            }
            ProviderSettings::DuckDNS { token, .. } => !token.is_empty(),
            ProviderSettings::Dynu { username, password, .. } => {
                !username.is_empty() && !password.is_empty()
            }
            ProviderSettings::FreeDNS { auth_token, .. } => !auth_token.is_empty(),
            ProviderSettings::Custom { update_url, .. } => !update_url.is_empty(),
        };

        if ok {
            self.config.status = DynDnsStatus::Success;
        } else {
            self.config.status =
                DynDnsStatus::Error("Credentials are incomplete".to_string());
        }
        ok
    }

    /// Simulate a successful update (for testing / UI preview).
    pub fn simulate_success(&mut self, ip: &str, timestamp: u64) {
        self.config.last_ip = Some(ip.to_string());
        self.config.last_update = Some(timestamp);
        self.config.status = DynDnsStatus::Success;
    }

    /// Simulate a failed update (for testing / UI preview).
    pub fn simulate_error(&mut self, reason: &str) {
        self.config.status = DynDnsStatus::Error(reason.to_string());
    }
}

impl Default for DynDnsManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Remote desktop types
// ============================================================================

/// Encryption strength for remote desktop sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncryptionLevel {
    Low,
    Medium,
    High,
}

impl EncryptionLevel {
    pub const ALL: &[Self] = &[Self::Low, Self::Medium, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low (fastest)",
            Self::Medium => "Medium",
            Self::High => "High (recommended)",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Low => "Basic encryption, maximum performance",
            Self::Medium => "AES-128, balanced security/performance",
            Self::High => "AES-256, strongest security",
        }
    }
}

impl fmt::Display for EncryptionLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Firewall/UPnP integration settings for remote desktop.
#[derive(Clone, Debug)]
pub struct FirewallSettings {
    /// Whether to automatically open the port in the OS firewall.
    pub auto_open_port: bool,
    /// Whether to attempt UPnP port forwarding on the router.
    pub upnp_forwarding: bool,
    /// Whether the port is currently detected as blocked.
    pub port_blocked: bool,
    /// Whether UPnP is currently available on the network.
    pub upnp_available: bool,
}

impl Default for FirewallSettings {
    fn default() -> Self {
        Self {
            auto_open_port: true,
            upnp_forwarding: false,
            port_blocked: false,
            upnp_available: false,
        }
    }
}

/// Remote desktop configuration.
#[derive(Clone, Debug)]
pub struct RemoteDesktopConfig {
    /// Whether remote desktop access is enabled.
    pub enabled: bool,
    /// TCP port to listen on.
    pub port: u16,
    /// Whether connecting clients must authenticate.
    pub require_authentication: bool,
    /// Usernames allowed to connect.
    pub allowed_users: Vec<String>,
    /// Encryption strength.
    pub encryption_level: EncryptionLevel,
    /// Maximum concurrent sessions.
    pub max_sessions: u32,
    /// Disconnect idle sessions after this many minutes.
    pub idle_timeout_minutes: u32,
    /// Firewall/UPnP integration.
    pub firewall: FirewallSettings,
}

impl RemoteDesktopConfig {
    /// Create a new config with secure defaults.
    pub fn new() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_RDP_PORT,
            require_authentication: true,
            allowed_users: Vec::new(),
            encryption_level: EncryptionLevel::High,
            max_sessions: DEFAULT_MAX_SESSIONS,
            idle_timeout_minutes: DEFAULT_IDLE_TIMEOUT,
            firewall: FirewallSettings::default(),
        }
    }

    /// Validate the port number.
    pub fn validate_port(port: u16) -> Result<(), &'static str> {
        if port == 0 {
            return Err("Port must be between 1 and 65535");
        }
        // Warn about well-known ports but allow them.
        Ok(())
    }

    /// Set the port, returning an error if invalid.
    pub fn set_port(&mut self, port: u16) -> Result<(), &'static str> {
        Self::validate_port(port)?;
        self.port = port;
        Ok(())
    }

    /// Add a user to the allowed list. Returns false if already present.
    pub fn add_user(&mut self, username: &str) -> bool {
        if username.is_empty() {
            return false;
        }
        let name = username.to_string();
        if self.allowed_users.contains(&name) {
            return false;
        }
        self.allowed_users.push(name);
        true
    }

    /// Remove a user from the allowed list. Returns false if not found.
    pub fn remove_user(&mut self, username: &str) -> bool {
        if let Some(pos) = self.allowed_users.iter().position(|u| u == username) {
            self.allowed_users.remove(pos);
            true
        } else {
            false
        }
    }

    /// Build a connection string for display purposes.
    pub fn connection_string(&self, hostname: &str) -> String {
        if self.port == DEFAULT_RDP_PORT {
            hostname.to_string()
        } else {
            format!("{hostname}:{}", self.port)
        }
    }

    /// Whether the port falls in the well-known range (< 1024).
    pub fn is_well_known_port(&self) -> bool {
        self.port < 1024
    }
}

impl Default for RemoteDesktopConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Combined remote-access state
// ============================================================================

/// Combined state for the remote-access settings page.
pub struct RemoteAccessSettings {
    pub dns: DynDnsManager,
    pub remote_desktop: RemoteDesktopConfig,
}

impl RemoteAccessSettings {
    pub fn new() -> Self {
        Self {
            dns: DynDnsManager::new(),
            remote_desktop: RemoteDesktopConfig::new(),
        }
    }
}

impl Default for RemoteAccessSettings {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Rendering helpers
// ============================================================================

/// Draw a rounded filled rectangle.
fn fill_rounded(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
    radius: f32,
) {
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: w,
        height: h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

/// Draw bold text (uses FontWeightHint::Bold).
fn text_bold(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    content: &str,
    color: Color,
    size: f32,
) {
    tree.text(x, y, content, color, size);
    tree.push(RenderCommand::Text {
        x: x + 0.5,
        y,
        text: content.to_string(),
        color,
        font_size: size,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
}

/// Draw a section header with underline.
fn render_section_header(tree: &mut RenderTree, x: f32, y: f32, title: &str) -> f32 {
    text_bold(tree, x, y, title, COL_TEXT, 16.0);
    tree.push(RenderCommand::FillRect {
        x,
        y: y + 24.0,
        width: CONTENT_WIDTH,
        height: 1.0,
        color: COL_SURFACE1,
        corner_radii: CornerRadii::ZERO,
    });
    y + 36.0
}

/// Draw a clickable button.
fn render_button(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    label: &str,
    color: Color,
) -> f32 {
    let width = label.len() as f32 * 7.5 + 24.0;
    fill_rounded(tree, x, y, width, BUTTON_HEIGHT, color, 6.0);
    tree.text(x + 12.0, y + 8.0, label, COL_BASE, 13.0);
    width
}

/// Draw a toggle switch (on/off).
fn render_toggle(tree: &mut RenderTree, x: f32, y: f32, enabled: bool) {
    let bg = if enabled { COL_GREEN } else { COL_SURFACE2 };
    fill_rounded(tree, x, y, TOGGLE_WIDTH, TOGGLE_HEIGHT, bg, TOGGLE_HEIGHT / 2.0);

    // Knob
    let knob_x = if enabled {
        x + TOGGLE_WIDTH - TOGGLE_HEIGHT + 2.0
    } else {
        x + 2.0
    };
    fill_rounded(
        tree,
        knob_x,
        y + 2.0,
        TOGGLE_HEIGHT - 4.0,
        TOGGLE_HEIGHT - 4.0,
        COL_TEXT,
        (TOGGLE_HEIGHT - 4.0) / 2.0,
    );
}

/// Draw a labeled text field (label on the left, value box on the right).
fn render_text_field(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    label: &str,
    value: &str,
    is_password: bool,
) -> f32 {
    // Label
    tree.text(x, y + 10.0, label, COL_SUBTEXT0, 13.0);

    // Input box
    let input_x = x + FIELD_LABEL_WIDTH;
    fill_rounded(tree, input_x, y, FIELD_INPUT_WIDTH, FIELD_HEIGHT, COL_SURFACE0, 6.0);
    tree.push(RenderCommand::StrokeRect {
        x: input_x,
        y,
        width: FIELD_INPUT_WIDTH,
        height: FIELD_HEIGHT,
        color: COL_SURFACE2,
        line_width: 1.0,
        corner_radii: CornerRadii::all(6.0),
    });

    // Display value (mask passwords)
    let display = if is_password && !value.is_empty() {
        "\u{2022}".repeat(value.len().min(20))
    } else if value.is_empty() {
        "\u{2014}".to_string() // em dash placeholder
    } else {
        value.to_string()
    };
    let text_color = if value.is_empty() { COL_OVERLAY0 } else { COL_TEXT };
    tree.push(RenderCommand::Text {
        x: input_x + 10.0,
        y: y + 10.0,
        text: display,
        color: text_color,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(FIELD_INPUT_WIDTH - 20.0),
    });

    y + FIELD_HEIGHT + 8.0
}

/// Draw a label + toggle row.
fn render_toggle_row(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    label: &str,
    enabled: bool,
) -> f32 {
    tree.text(x, y + 2.0, label, COL_SUBTEXT0, 13.0);
    render_toggle(tree, x + FIELD_LABEL_WIDTH, y, enabled);
    y + TOGGLE_HEIGHT + 12.0
}

/// Draw a label + dropdown-style display.
fn render_dropdown_row(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    label: &str,
    value: &str,
) -> f32 {
    tree.text(x, y + 10.0, label, COL_SUBTEXT0, 13.0);

    let dd_x = x + FIELD_LABEL_WIDTH;
    fill_rounded(tree, dd_x, y, FIELD_INPUT_WIDTH, FIELD_HEIGHT, COL_SURFACE0, 6.0);
    tree.push(RenderCommand::StrokeRect {
        x: dd_x,
        y,
        width: FIELD_INPUT_WIDTH,
        height: FIELD_HEIGHT,
        color: COL_SURFACE2,
        line_width: 1.0,
        corner_radii: CornerRadii::all(6.0),
    });

    tree.push(RenderCommand::Text {
        x: dd_x + 10.0,
        y: y + 10.0,
        text: value.to_string(),
        color: COL_TEXT,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(FIELD_INPUT_WIDTH - 36.0),
    });

    // Dropdown arrow indicator
    tree.text(
        dd_x + FIELD_INPUT_WIDTH - 24.0,
        y + 10.0,
        "\u{25BC}",
        COL_OVERLAY0,
        11.0,
    );

    y + FIELD_HEIGHT + 8.0
}

/// Draw a warning banner.
fn render_warning(tree: &mut RenderTree, x: f32, y: f32, message: &str) -> f32 {
    fill_rounded(tree, x, y, CONTENT_WIDTH, 36.0, COL_SURFACE0, 6.0);
    tree.push(RenderCommand::FillRect {
        x,
        y,
        width: 4.0,
        height: 36.0,
        color: COL_PEACH,
        corner_radii: CornerRadii::ZERO,
    });
    tree.text(x + 14.0, y + 10.0, "\u{26A0}", COL_PEACH, 14.0);
    tree.push(RenderCommand::Text {
        x: x + 34.0,
        y: y + 10.0,
        text: message.to_string(),
        color: COL_PEACH,
        font_size: 12.0,
        font_weight: FontWeightHint::Regular,
        max_width: Some(CONTENT_WIDTH - 48.0),
    });
    y + 44.0
}

/// Draw a status indicator dot and label.
fn render_status_indicator(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    label: &str,
    color: Color,
) -> f32 {
    // Dot
    fill_rounded(tree, x, y + 4.0, 10.0, 10.0, color, 5.0);
    tree.text(x + 16.0, y + 2.0, label, color, 12.0);
    y + 20.0
}

// ============================================================================
// DynDNS page rendering
// ============================================================================

/// Render the DynDNS settings section.
fn render_dyndns_section(
    tree: &mut RenderTree,
    x: f32,
    start_y: f32,
    manager: &DynDnsManager,
) -> f32 {
    let cfg = manager.config();
    let mut y = render_section_header(tree, x, start_y, "Dynamic DNS (DynDNS)");

    tree.text(
        x,
        y + 4.0,
        "Keep a hostname pointing to your changing IP address:",
        COL_SUBTEXT0,
        13.0,
    );
    y += 28.0;

    // Enable toggle
    y = render_toggle_row(tree, x, y, "Enable DynDNS", cfg.enabled);

    if !cfg.enabled {
        tree.text(x + 16.0, y, "DynDNS is disabled.", COL_OVERLAY0, 12.0);
        return y + 24.0;
    }

    // Provider dropdown
    y = render_dropdown_row(tree, x, y, "Provider", cfg.provider.label());

    // Provider-specific credential fields
    match &cfg.provider_settings {
        ProviderSettings::NoIP { hostname, email, password } => {
            y = render_text_field(tree, x, y, "Hostname", hostname, false);
            y = render_text_field(tree, x, y, "Email", email, false);
            y = render_text_field(tree, x, y, "Password", password, true);
        }
        ProviderSettings::DuckDNS { domain, token } => {
            y = render_text_field(tree, x, y, "Domain", domain, false);
            y = render_text_field(tree, x, y, "Token", token, true);
        }
        ProviderSettings::Dynu { hostname, username, password } => {
            y = render_text_field(tree, x, y, "Hostname", hostname, false);
            y = render_text_field(tree, x, y, "Username", username, false);
            y = render_text_field(tree, x, y, "Password", password, true);
        }
        ProviderSettings::FreeDNS { domain, auth_token } => {
            y = render_text_field(tree, x, y, "Domain", domain, false);
            y = render_text_field(tree, x, y, "Auth Token", auth_token, true);
        }
        ProviderSettings::Custom { update_url, method } => {
            y = render_text_field(tree, x, y, "Update URL", update_url, false);
            y = render_dropdown_row(tree, x, y, "HTTP Method", method.label());
        }
    }

    // Update interval
    let interval_str = format!("{} min", cfg.update_interval_minutes);
    y = render_dropdown_row(tree, x, y, "Update Interval", &interval_str);

    y += 8.0;

    // Status display
    y = render_section_header(tree, x, y, "Status");

    // Status indicator
    y = render_status_indicator(
        tree,
        x,
        y,
        cfg.status.label(),
        cfg.status.color(),
    );

    // Error detail
    if let DynDnsStatus::Error(ref msg) = cfg.status {
        tree.push(RenderCommand::Text {
            x: x + 16.0,
            y,
            text: msg.clone(),
            color: COL_RED,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(CONTENT_WIDTH - 32.0),
        });
        y += 18.0;
    }

    // Last update info
    if let Some(timestamp) = cfg.last_update {
        let info = format!("Last update: {} (epoch)", timestamp);
        tree.text(x, y + 4.0, &info, COL_SUBTEXT0, 12.0);
        y += 20.0;
    }
    if let Some(ref ip) = cfg.last_ip {
        let info = format!("Current IP: {ip}");
        tree.text(x, y + 4.0, &info, COL_SUBTEXT0, 12.0);
        y += 20.0;
    }

    // External IP display
    let ext_ip = manager.get_external_ip();
    let ext_label = format!("Detected external IP: {ext_ip}");
    tree.text(x, y + 4.0, &ext_label, COL_SUBTEXT0, 12.0);
    y += 28.0;

    // Action buttons
    let mut btn_x = x;
    let w = render_button(tree, btn_x, y, "Update Now", COL_ACCENT);
    btn_x += w + 12.0;
    render_button(tree, btn_x, y, "Test Connection", COL_GREEN);
    y += BUTTON_HEIGHT + 8.0;

    y
}

// ============================================================================
// Remote Desktop page rendering
// ============================================================================

/// Render the remote desktop settings section.
fn render_remote_desktop_section(
    tree: &mut RenderTree,
    x: f32,
    start_y: f32,
    config: &RemoteDesktopConfig,
    hostname: &str,
) -> f32 {
    let mut y = render_section_header(tree, x, start_y, "Remote Desktop");

    // Security warning when enabled
    if config.enabled {
        y = render_warning(
            tree,
            x,
            y,
            "Remote Desktop is enabled. Ensure strong passwords and firewall rules.",
        );
    }

    // Enable toggle
    y = render_toggle_row(tree, x, y, "Enable Remote Desktop", config.enabled);

    if !config.enabled {
        tree.text(x + 16.0, y, "Remote Desktop is disabled.", COL_OVERLAY0, 12.0);
        return y + 24.0;
    }

    // Port
    let port_str = config.port.to_string();
    y = render_text_field(tree, x, y, "Port", &port_str, false);

    if config.is_well_known_port() {
        y = render_warning(
            tree,
            x,
            y,
            "Using a well-known port (< 1024) may require elevated privileges.",
        );
    }

    // Authentication
    y = render_toggle_row(
        tree,
        x,
        y,
        "Require Authentication",
        config.require_authentication,
    );

    // Encryption level
    y = render_dropdown_row(tree, x, y, "Encryption", config.encryption_level.label());
    tree.text(
        x + FIELD_LABEL_WIDTH,
        y - 4.0,
        config.encryption_level.description(),
        COL_OVERLAY0,
        11.0,
    );
    y += 12.0;

    // Max sessions
    let sessions_str = config.max_sessions.to_string();
    y = render_text_field(tree, x, y, "Max Sessions", &sessions_str, false);

    // Idle timeout
    let timeout_str = format!("{} min", config.idle_timeout_minutes);
    y = render_dropdown_row(tree, x, y, "Idle Timeout", &timeout_str);

    y += 8.0;

    // Allowed users section
    y = render_section_header(tree, x, y, "Allowed Users");
    if config.allowed_users.is_empty() {
        tree.text(
            x + 16.0,
            y + 4.0,
            "No users configured (all authenticated users can connect).",
            COL_OVERLAY0,
            12.0,
        );
        y += 28.0;
    } else {
        for user in &config.allowed_users {
            fill_rounded(tree, x, y, CONTENT_WIDTH, 36.0, COL_SURFACE0, 6.0);
            tree.text(x + 12.0, y + 10.0, user, COL_TEXT, 13.0);
            // Remove button (rendered as red "X")
            tree.text(
                x + CONTENT_WIDTH - 28.0,
                y + 10.0,
                "\u{2715}",
                COL_RED,
                13.0,
            );
            y += 40.0;
        }
    }

    // Add user button
    render_button(tree, x, y, "Add User", COL_ACCENT);
    y += BUTTON_HEIGHT + 16.0;

    // Firewall settings
    y = render_section_header(tree, x, y, "Firewall & Network");

    y = render_toggle_row(tree, x, y, "Auto-open firewall port", config.firewall.auto_open_port);
    y = render_toggle_row(tree, x, y, "UPnP port forwarding", config.firewall.upnp_forwarding);

    // Firewall status indicators
    if config.firewall.port_blocked {
        y = render_warning(
            tree,
            x,
            y,
            &format!("Port {} appears to be blocked by the firewall.", config.port),
        );
    }
    if config.firewall.upnp_forwarding && !config.firewall.upnp_available {
        y = render_warning(
            tree,
            x,
            y,
            "UPnP is not available on this network.",
        );
    }

    y += 8.0;

    // Connection info display
    y = render_section_header(tree, x, y, "Connection Info");
    let conn_str = config.connection_string(hostname);
    let connect_label = format!("Connect to: {conn_str}");
    fill_rounded(tree, x, y, CONTENT_WIDTH, 40.0, COL_SURFACE0, 6.0);
    tree.push(RenderCommand::Text {
        x: x + 16.0,
        y: y + 12.0,
        text: connect_label,
        color: COL_ACCENT,
        font_size: 14.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(CONTENT_WIDTH - 32.0),
    });
    y += 48.0;

    y
}

// ============================================================================
// Full page render
// ============================================================================

/// Render the full Remote Access settings page (DynDNS + Remote Desktop).
pub fn render_remote_access_page(
    tree: &mut RenderTree,
    x: f32,
    start_y: f32,
    settings: &RemoteAccessSettings,
) {
    let mut y = start_y;

    // Page title
    tree.push(RenderCommand::Text {
        x,
        y,
        text: "Remote Access".into(),
        color: COL_TEXT,
        font_size: 20.0,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });
    tree.push(RenderCommand::Text {
        x,
        y: y + 28.0,
        text: "Configure dynamic DNS and remote desktop access".into(),
        color: COL_SUBTEXT0,
        font_size: 13.0,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
    y += 56.0;

    // DynDNS section
    y = render_dyndns_section(tree, x, y, &settings.dns);
    y += SECTION_SPACING;

    // Remote Desktop section
    let hostname = if settings.dns.config().enabled {
        if settings.dns.config().hostname.is_empty() {
            "this-pc"
        } else {
            &settings.dns.config().hostname
        }
    } else {
        "this-pc"
    };
    render_remote_desktop_section(tree, x, y, &settings.remote_desktop, hostname);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- DynDnsConfig defaults ----

    #[test]
    fn config_defaults_no_ip() {
        let cfg = DynDnsConfig::new(DynDnsProvider::NoIP);
        assert_eq!(cfg.provider, DynDnsProvider::NoIP);
        assert!(!cfg.enabled);
        assert_eq!(cfg.update_interval_minutes, DEFAULT_UPDATE_INTERVAL);
        assert!(cfg.last_update.is_none());
        assert!(cfg.last_ip.is_none());
        assert_eq!(cfg.status, DynDnsStatus::Idle);
    }

    #[test]
    fn config_defaults_duckdns() {
        let cfg = DynDnsConfig::new(DynDnsProvider::DuckDNS);
        assert_eq!(cfg.provider, DynDnsProvider::DuckDNS);
        assert!(matches!(cfg.provider_settings, ProviderSettings::DuckDNS { .. }));
    }

    #[test]
    fn config_defaults_custom() {
        let cfg = DynDnsConfig::new(DynDnsProvider::Custom);
        assert!(matches!(
            cfg.provider_settings,
            ProviderSettings::Custom { method: HttpMethod::Get, .. }
        ));
    }

    // ---- Provider-specific fields ----

    #[test]
    fn provider_uses_credentials_vs_token() {
        assert!(DynDnsProvider::NoIP.uses_credentials());
        assert!(DynDnsProvider::Dynu.uses_credentials());
        assert!(!DynDnsProvider::DuckDNS.uses_credentials());

        assert!(DynDnsProvider::DuckDNS.uses_token());
        assert!(DynDnsProvider::FreeDNS.uses_token());
        assert!(!DynDnsProvider::NoIP.uses_token());
    }

    #[test]
    fn provider_settings_round_trip() {
        for &prov in DynDnsProvider::ALL {
            let settings = ProviderSettings::default_for(prov);
            assert_eq!(settings.provider(), prov, "provider() must match for {prov:?}");
        }
    }

    // ---- Update URL template rendering ----

    #[test]
    fn noip_url_rendering() {
        let settings = ProviderSettings::NoIP {
            hostname: "mypc.ddns.net".to_string(),
            email: "user@example.com".to_string(),
            password: "secret".to_string(),
        };
        let url = settings.render_update_url("1.2.3.4");
        assert!(url.contains("mypc.ddns.net"), "hostname substituted");
        assert!(url.contains("1.2.3.4"), "IP substituted");
        assert!(url.contains("dynupdate.no-ip.com"), "correct base URL");
    }

    #[test]
    fn duckdns_url_rendering() {
        let settings = ProviderSettings::DuckDNS {
            domain: "myhost".to_string(),
            token: "tok-abc-123".to_string(),
        };
        let url = settings.render_update_url("10.0.0.1");
        assert!(url.contains("myhost"), "domain substituted");
        assert!(url.contains("tok-abc-123"), "token substituted");
        assert!(url.contains("10.0.0.1"), "IP substituted");
    }

    #[test]
    fn freedns_url_rendering() {
        let settings = ProviderSettings::FreeDNS {
            domain: "example.mooo.com".to_string(),
            auth_token: "auth123".to_string(),
        };
        let url = settings.render_update_url("192.168.1.1");
        assert!(url.contains("auth123"));
        assert!(url.contains("192.168.1.1"));
    }

    #[test]
    fn custom_url_rendering() {
        let settings = ProviderSettings::Custom {
            update_url: "https://my.dns/update?ip={ip}&key=abc".to_string(),
            method: HttpMethod::Post,
        };
        let url = settings.render_update_url("5.6.7.8");
        assert_eq!(url, "https://my.dns/update?ip=5.6.7.8&key=abc");
    }

    // ---- DynDnsManager operations ----

    #[test]
    fn manager_force_update_only_when_enabled() {
        let mut mgr = DynDnsManager::new();
        mgr.force_update();
        assert_eq!(*mgr.status(), DynDnsStatus::Idle, "should not update when disabled");

        mgr.config_mut().enabled = true;
        mgr.force_update();
        assert_eq!(*mgr.status(), DynDnsStatus::Updating);
    }

    #[test]
    fn manager_test_connection_empty_credentials() {
        let mut mgr = DynDnsManager::new();
        // Default NoIP has empty fields.
        let ok = mgr.test_connection();
        assert!(!ok);
        assert!(matches!(*mgr.status(), DynDnsStatus::Error(_)));
    }

    #[test]
    fn manager_test_connection_valid_credentials() {
        let mut mgr = DynDnsManager::with_config(DynDnsConfig {
            provider_settings: ProviderSettings::DuckDNS {
                domain: "test".to_string(),
                token: "my-token".to_string(),
            },
            provider: DynDnsProvider::DuckDNS,
            ..DynDnsConfig::default()
        });
        let ok = mgr.test_connection();
        assert!(ok);
        assert_eq!(*mgr.status(), DynDnsStatus::Success);
    }

    #[test]
    fn manager_simulate_success() {
        let mut mgr = DynDnsManager::new();
        mgr.simulate_success("1.1.1.1", 1_700_000_000);
        assert_eq!(mgr.config().last_ip.as_deref(), Some("1.1.1.1"));
        assert_eq!(mgr.config().last_update, Some(1_700_000_000));
        assert_eq!(*mgr.status(), DynDnsStatus::Success);
    }

    #[test]
    fn manager_simulate_error() {
        let mut mgr = DynDnsManager::new();
        mgr.simulate_error("timeout");
        assert_eq!(*mgr.status(), DynDnsStatus::Error("timeout".to_string()));
    }

    #[test]
    fn manager_get_external_ip_returns_stub() {
        let mgr = DynDnsManager::new();
        let ip = mgr.get_external_ip();
        assert!(!ip.is_empty());
    }

    // ---- DynDnsStatus transitions ----

    #[test]
    fn status_labels_non_empty() {
        let statuses = [
            DynDnsStatus::Idle,
            DynDnsStatus::Updating,
            DynDnsStatus::Success,
            DynDnsStatus::Error("fail".to_string()),
        ];
        for s in &statuses {
            assert!(!s.label().is_empty(), "status {s:?} should have a label");
        }
    }

    // ---- RemoteDesktopConfig defaults ----

    #[test]
    fn remote_desktop_defaults() {
        let cfg = RemoteDesktopConfig::new();
        assert!(!cfg.enabled);
        assert_eq!(cfg.port, DEFAULT_RDP_PORT);
        assert!(cfg.require_authentication);
        assert!(cfg.allowed_users.is_empty());
        assert_eq!(cfg.encryption_level, EncryptionLevel::High);
        assert_eq!(cfg.max_sessions, DEFAULT_MAX_SESSIONS);
        assert_eq!(cfg.idle_timeout_minutes, DEFAULT_IDLE_TIMEOUT);
    }

    // ---- Port validation ----

    #[test]
    fn port_validation_zero_rejected() {
        assert!(RemoteDesktopConfig::validate_port(0).is_err());
    }

    #[test]
    fn port_validation_valid_ports() {
        assert!(RemoteDesktopConfig::validate_port(1).is_ok());
        assert!(RemoteDesktopConfig::validate_port(3389).is_ok());
        assert!(RemoteDesktopConfig::validate_port(65535).is_ok());
    }

    #[test]
    fn set_port_updates_value() {
        let mut cfg = RemoteDesktopConfig::new();
        assert!(cfg.set_port(8080).is_ok());
        assert_eq!(cfg.port, 8080);
    }

    #[test]
    fn set_port_rejects_zero() {
        let mut cfg = RemoteDesktopConfig::new();
        assert!(cfg.set_port(0).is_err());
        assert_eq!(cfg.port, DEFAULT_RDP_PORT, "port should not change on error");
    }

    #[test]
    fn well_known_port_detection() {
        let mut cfg = RemoteDesktopConfig::new();
        assert!(!cfg.is_well_known_port()); // 3389 >= 1024

        cfg.port = 443;
        assert!(cfg.is_well_known_port());

        cfg.port = 1024;
        assert!(!cfg.is_well_known_port());
    }

    // ---- User list management ----

    #[test]
    fn add_user_success() {
        let mut cfg = RemoteDesktopConfig::new();
        assert!(cfg.add_user("alice"));
        assert_eq!(cfg.allowed_users, vec!["alice"]);
    }

    #[test]
    fn add_user_duplicate_rejected() {
        let mut cfg = RemoteDesktopConfig::new();
        assert!(cfg.add_user("alice"));
        assert!(!cfg.add_user("alice"), "duplicate should be rejected");
        assert_eq!(cfg.allowed_users.len(), 1);
    }

    #[test]
    fn add_user_empty_rejected() {
        let mut cfg = RemoteDesktopConfig::new();
        assert!(!cfg.add_user(""));
        assert!(cfg.allowed_users.is_empty());
    }

    #[test]
    fn remove_user_success() {
        let mut cfg = RemoteDesktopConfig::new();
        cfg.add_user("alice");
        cfg.add_user("bob");
        assert!(cfg.remove_user("alice"));
        assert_eq!(cfg.allowed_users, vec!["bob"]);
    }

    #[test]
    fn remove_user_not_found() {
        let mut cfg = RemoteDesktopConfig::new();
        assert!(!cfg.remove_user("ghost"));
    }

    #[test]
    fn add_multiple_users_ordering() {
        let mut cfg = RemoteDesktopConfig::new();
        cfg.add_user("charlie");
        cfg.add_user("alice");
        cfg.add_user("bob");
        assert_eq!(cfg.allowed_users, vec!["charlie", "alice", "bob"]);
    }

    // ---- Connection string ----

    #[test]
    fn connection_string_default_port() {
        let cfg = RemoteDesktopConfig::new();
        assert_eq!(cfg.connection_string("mypc.local"), "mypc.local");
    }

    #[test]
    fn connection_string_custom_port() {
        let mut cfg = RemoteDesktopConfig::new();
        cfg.port = 5900;
        assert_eq!(cfg.connection_string("mypc.local"), "mypc.local:5900");
    }

    // ---- Encryption level ----

    #[test]
    fn encryption_levels_have_labels_and_descriptions() {
        for &level in EncryptionLevel::ALL {
            assert!(!level.label().is_empty());
            assert!(!level.description().is_empty());
        }
    }

    // ---- RemoteAccessSettings ----

    #[test]
    fn combined_settings_creation() {
        let settings = RemoteAccessSettings::new();
        assert!(!settings.dns.config().enabled);
        assert!(!settings.remote_desktop.enabled);
    }

    // ---- Render smoke test ----

    #[test]
    fn render_page_does_not_panic() {
        let settings = RemoteAccessSettings::new();
        let mut tree = RenderTree::new();
        render_remote_access_page(&mut tree, 0.0, 0.0, &settings);
        // The tree should have some commands in it.
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn render_page_enabled_with_data() {
        let mut settings = RemoteAccessSettings::new();
        settings.dns.config_mut().enabled = true;
        settings.dns.config_mut().hostname = "mypc.ddns.net".to_string();
        settings.dns.config_mut().provider_settings = ProviderSettings::NoIP {
            hostname: "mypc.ddns.net".to_string(),
            email: "user@example.com".to_string(),
            password: "secret".to_string(),
        };
        settings.dns.simulate_success("1.2.3.4", 1_700_000_000);

        settings.remote_desktop.enabled = true;
        settings.remote_desktop.add_user("admin");
        settings.remote_desktop.add_user("user1");
        settings.remote_desktop.firewall.port_blocked = true;

        let mut tree = RenderTree::new();
        render_remote_access_page(&mut tree, 10.0, 20.0, &settings);
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn render_dyndns_disabled_is_short() {
        let manager = DynDnsManager::new();
        let mut tree = RenderTree::new();
        render_dyndns_section(&mut tree, 0.0, 0.0, &manager);
        let disabled_count = tree.commands.len();

        let mut manager2 = DynDnsManager::new();
        manager2.config_mut().enabled = true;
        let mut tree2 = RenderTree::new();
        render_dyndns_section(&mut tree2, 0.0, 0.0, &manager2);

        assert!(
            tree2.commands.len() > disabled_count,
            "enabled section should have more render commands"
        );
    }
}

//! Network status tray indicator.
//!
//! Shows a compact network icon in the system tray with connection type,
//! signal strength, and data transfer rates. Clicking opens a quick-connect
//! flyout listing available WiFi networks.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Connection type
// ============================================================================

/// Network connection type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionType {
    Ethernet,
    Wifi,
    VPN,
    Cellular,
    None,
}

impl ConnectionType {
    pub fn icon_label(self) -> &'static str {
        match self {
            Self::Ethernet => "🔌",
            Self::Wifi => "📶",
            Self::VPN => "🔒",
            Self::Cellular => "📱",
            Self::None => "✕",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "Ethernet",
            Self::Wifi => "Wi-Fi",
            Self::VPN => "VPN",
            Self::Cellular => "Cellular",
            Self::None => "Not connected",
        }
    }
}

// ============================================================================
// WiFi signal strength
// ============================================================================

/// WiFi signal strength tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SignalStrength {
    /// No signal.
    None,
    /// Weak (1 bar).
    Weak,
    /// Fair (2 bars).
    Fair,
    /// Good (3 bars).
    Good,
    /// Excellent (4 bars).
    Excellent,
}

impl SignalStrength {
    /// Convert RSSI dBm value to a signal tier.
    pub fn from_rssi(rssi: i32) -> Self {
        match rssi {
            r if r >= -50 => Self::Excellent,
            r if r >= -60 => Self::Good,
            r if r >= -70 => Self::Fair,
            r if r >= -80 => Self::Weak,
            _ => Self::None,
        }
    }

    pub fn bars(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Weak => 1,
            Self::Fair => 2,
            Self::Good => 3,
            Self::Excellent => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No signal",
            Self::Weak => "Weak",
            Self::Fair => "Fair",
            Self::Good => "Good",
            Self::Excellent => "Excellent",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::None | Self::Weak => RED,
            Self::Fair => YELLOW,
            Self::Good => GREEN,
            Self::Excellent => BLUE,
        }
    }
}

// ============================================================================
// WiFi security
// ============================================================================

/// WiFi security type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    WEP,
    WPA2,
    WPA3,
    Enterprise,
}

impl WifiSecurity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::WEP => "WEP",
            Self::WPA2 => "WPA2",
            Self::WPA3 => "WPA3",
            Self::Enterprise => "Enterprise",
        }
    }

    pub fn is_secure(self) -> bool {
        !matches!(self, Self::Open | Self::WEP)
    }
}

// ============================================================================
// Visible WiFi network
// ============================================================================

/// A WiFi network visible in scan results.
#[derive(Clone, Debug)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: SignalStrength,
    pub security: WifiSecurity,
    /// Whether we have saved credentials for this network.
    pub saved: bool,
    /// Whether this is the currently connected network.
    pub connected: bool,
    /// Channel number.
    pub channel: u32,
}

impl WifiNetwork {
    pub fn new(ssid: &str, signal: SignalStrength, security: WifiSecurity) -> Self {
        Self {
            ssid: ssid.into(),
            signal,
            security,
            saved: false,
            connected: false,
            channel: 0,
        }
    }
}

// ============================================================================
// Data transfer rates
// ============================================================================

/// Current network data transfer rates.
#[derive(Clone, Debug, Default)]
pub struct TransferRates {
    /// Bytes received per second.
    pub rx_bytes_per_sec: u64,
    /// Bytes sent per second.
    pub tx_bytes_per_sec: u64,
    /// Total bytes received since connection started.
    pub total_rx: u64,
    /// Total bytes sent since connection started.
    pub total_tx: u64,
}

impl TransferRates {
    /// Format a byte rate as human-readable (e.g. "1.5 MB/s").
    pub fn format_rate(bytes_per_sec: u64) -> String {
        if bytes_per_sec >= 1_000_000_000 {
            format!("{:.1} GB/s", bytes_per_sec as f64 / 1_000_000_000.0)
        } else if bytes_per_sec >= 1_000_000 {
            format!("{:.1} MB/s", bytes_per_sec as f64 / 1_000_000.0)
        } else if bytes_per_sec >= 1_000 {
            format!("{:.1} KB/s", bytes_per_sec as f64 / 1_000.0)
        } else {
            format!("{} B/s", bytes_per_sec)
        }
    }

    /// Format total bytes.
    pub fn format_bytes(bytes: u64) -> String {
        if bytes >= 1_000_000_000 {
            format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
        } else if bytes >= 1_000_000 {
            format!("{:.1} MB", bytes as f64 / 1_000_000.0)
        } else if bytes >= 1_000 {
            format!("{:.0} KB", bytes as f64 / 1_000.0)
        } else {
            format!("{} B", bytes)
        }
    }

    pub fn rx_formatted(&self) -> String {
        Self::format_rate(self.rx_bytes_per_sec)
    }

    pub fn tx_formatted(&self) -> String {
        Self::format_rate(self.tx_bytes_per_sec)
    }
}

// ============================================================================
// Network state
// ============================================================================

/// Overall network state for the tray indicator.
#[derive(Clone, Debug)]
pub struct NetworkState {
    /// Primary connection type.
    pub connection_type: ConnectionType,
    /// Name of the connected network (SSID, interface name, VPN name).
    pub network_name: String,
    /// IP address (v4 string).
    pub ip_address: Option<String>,
    /// Signal strength (for WiFi / cellular).
    pub signal: SignalStrength,
    /// Whether airplane mode is on.
    pub airplane_mode: bool,
    /// Transfer rates.
    pub rates: TransferRates,
    /// Uptime in seconds since connection established.
    pub connected_secs: u64,
}

impl NetworkState {
    pub fn disconnected() -> Self {
        Self {
            connection_type: ConnectionType::None,
            network_name: String::new(),
            ip_address: None,
            signal: SignalStrength::None,
            airplane_mode: false,
            rates: TransferRates::default(),
            connected_secs: 0,
        }
    }

    pub fn ethernet(name: &str, ip: &str) -> Self {
        Self {
            connection_type: ConnectionType::Ethernet,
            network_name: name.into(),
            ip_address: Some(ip.into()),
            signal: SignalStrength::Excellent,
            airplane_mode: false,
            rates: TransferRates::default(),
            connected_secs: 0,
        }
    }

    pub fn wifi(ssid: &str, ip: &str, signal: SignalStrength) -> Self {
        Self {
            connection_type: ConnectionType::Wifi,
            network_name: ssid.into(),
            ip_address: Some(ip.into()),
            signal,
            airplane_mode: false,
            rates: TransferRates::default(),
            connected_secs: 0,
        }
    }

    /// Uptime formatted as "Xh Ym" or "Xs".
    pub fn uptime_formatted(&self) -> String {
        let s = self.connected_secs;
        if s >= 3600 {
            format!("{}h {}m", s / 3600, (s % 3600) / 60)
        } else if s >= 60 {
            format!("{}m {}s", s / 60, s % 60)
        } else {
            format!("{}s", s)
        }
    }

    /// Tooltip text for the tray icon.
    pub fn tooltip(&self) -> String {
        if self.airplane_mode {
            return "Airplane mode".into();
        }
        match self.connection_type {
            ConnectionType::None => "Not connected".into(),
            ConnectionType::Ethernet => {
                format!("Ethernet — {}", self.ip_address.as_deref().unwrap_or("No IP"))
            }
            ConnectionType::Wifi => {
                format!("{} — {} — {}", self.network_name, self.signal.label(),
                    self.ip_address.as_deref().unwrap_or("No IP"))
            }
            ConnectionType::VPN => {
                format!("VPN: {} — {}", self.network_name,
                    self.ip_address.as_deref().unwrap_or("No IP"))
            }
            ConnectionType::Cellular => {
                format!("Cellular — {}", self.signal.label())
            }
        }
    }
}

// ============================================================================
// Network indicator
// ============================================================================

/// The network tray indicator widget.
pub struct NetworkIndicator {
    state: NetworkState,
    /// Scanned WiFi networks.
    wifi_networks: Vec<WifiNetwork>,
    /// Whether the flyout popup is open.
    flyout_open: bool,
    /// Whether WiFi is enabled.
    wifi_enabled: bool,
    /// Selected network index in the flyout (for keyboard nav).
    selected_index: Option<usize>,
    /// Rate history ring buffer (last N samples of rx/tx for sparkline).
    rate_history: Vec<(u64, u64)>,
    /// Maximum history samples.
    max_history: usize,
}

impl NetworkIndicator {
    pub fn new() -> Self {
        Self {
            state: NetworkState::disconnected(),
            wifi_networks: Vec::new(),
            flyout_open: false,
            wifi_enabled: true,
            selected_index: None,
            rate_history: Vec::new(),
            max_history: 60,
        }
    }

    pub fn state(&self) -> &NetworkState {
        &self.state
    }

    pub fn update_state(&mut self, state: NetworkState) {
        self.state = state;
    }

    pub fn set_wifi_networks(&mut self, networks: Vec<WifiNetwork>) {
        self.wifi_networks = networks;
        // Sort by signal strength descending, connected first.
        self.wifi_networks.sort_by(|a, b| {
            b.connected.cmp(&a.connected)
                .then(b.signal.cmp(&a.signal))
                .then(a.ssid.cmp(&b.ssid))
        });
    }

    pub fn wifi_networks(&self) -> &[WifiNetwork] {
        &self.wifi_networks
    }

    pub fn toggle_flyout(&mut self) {
        self.flyout_open = !self.flyout_open;
        if self.flyout_open {
            self.selected_index = None;
        }
    }

    pub fn is_flyout_open(&self) -> bool {
        self.flyout_open
    }

    pub fn close_flyout(&mut self) {
        self.flyout_open = false;
        self.selected_index = None;
    }

    pub fn wifi_enabled(&self) -> bool {
        self.wifi_enabled
    }

    pub fn set_wifi_enabled(&mut self, enabled: bool) {
        self.wifi_enabled = enabled;
    }

    pub fn toggle_airplane_mode(&mut self) {
        self.state.airplane_mode = !self.state.airplane_mode;
    }

    pub fn select_next(&mut self) {
        if self.wifi_networks.is_empty() {
            return;
        }
        let max = self.wifi_networks.len().saturating_sub(1);
        self.selected_index = Some(match self.selected_index {
            Some(i) if i < max => i + 1,
            _ => 0,
        });
    }

    pub fn select_prev(&mut self) {
        if self.wifi_networks.is_empty() {
            return;
        }
        let max = self.wifi_networks.len().saturating_sub(1);
        self.selected_index = Some(match self.selected_index {
            Some(0) | None => max,
            Some(i) => i - 1,
        });
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    pub fn record_rates(&mut self, rx: u64, tx: u64) {
        if self.rate_history.len() >= self.max_history {
            self.rate_history.remove(0);
        }
        self.rate_history.push((rx, tx));
    }

    pub fn rate_history(&self) -> &[(u64, u64)] {
        &self.rate_history
    }

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    /// Render the tray icon (compact, ~24x24).
    pub fn render_icon(&self, x: f32, y: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let icon_color = if self.state.airplane_mode {
            OVERLAY0
        } else {
            match self.state.connection_type {
                ConnectionType::None => RED,
                ConnectionType::Ethernet => GREEN,
                ConnectionType::Wifi => self.state.signal.color(),
                ConnectionType::VPN => LAVENDER,
                ConnectionType::Cellular => PEACH,
            }
        };

        // Icon background circle
        cmds.push(RenderCommand::FillRect {
            x, y, width: 24.0, height: 24.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Icon text (emoji or signal bars)
        let label = if self.state.airplane_mode {
            "✈"
        } else {
            self.state.connection_type.icon_label()
        };
        cmds.push(RenderCommand::Text {
            x: x + 4.0, y: y + 4.0,
            text: label.into(),
            font_size: 13.0, color: icon_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(20.0),
        });

        cmds
    }

    /// Render the WiFi network flyout popup.
    pub fn render_flyout(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        if !self.flyout_open {
            return Vec::new();
        }

        let mut cmds = Vec::new();
        let pad = 12.0_f32;
        let inner = width - 2.0 * pad;
        let mut cy = y;

        // Flyout background
        let height = 60.0 + self.wifi_networks.len() as f32 * 44.0 + 80.0;
        cmds.push(RenderCommand::FillRect {
            x, y, width, height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Current connection summary
        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad, y: cy,
            text: self.state.tooltip(),
            font_size: 13.0, color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
        });
        cy += 20.0;

        // Transfer rates
        cmds.push(RenderCommand::Text {
            x: x + pad, y: cy,
            text: format!("↓ {}  ↑ {}", self.state.rates.rx_formatted(), self.state.rates.tx_formatted()),
            font_size: 11.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner),
        });
        cy += 20.0;

        // Toggles row
        // Airplane mode toggle
        let airplane_label = if self.state.airplane_mode { "✈ Airplane ON" } else { "✈ Airplane OFF" };
        let airplane_color = if self.state.airplane_mode { PEACH } else { OVERLAY0 };
        cmds.push(RenderCommand::FillRect {
            x: x + pad, y: cy, width: inner * 0.48, height: 28.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 8.0, y: cy + 6.0,
            text: airplane_label.into(),
            font_size: 12.0, color: airplane_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner * 0.45),
        });

        // WiFi toggle
        let wifi_label = if self.wifi_enabled { "Wi-Fi ON" } else { "Wi-Fi OFF" };
        let wifi_color = if self.wifi_enabled { GREEN } else { OVERLAY0 };
        cmds.push(RenderCommand::FillRect {
            x: x + pad + inner * 0.52, y: cy, width: inner * 0.48, height: 28.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + inner * 0.52 + 8.0, y: cy + 6.0,
            text: wifi_label.into(),
            font_size: 12.0, color: wifi_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner * 0.45),
        });
        cy += 36.0;

        // WiFi networks list
        if self.wifi_enabled && !self.state.airplane_mode {
            for (i, net) in self.wifi_networks.iter().enumerate() {
                let selected = self.selected_index == Some(i);
                let bg = if net.connected {
                    SURFACE0
                } else if selected {
                    SURFACE1
                } else {
                    MANTLE
                };

                cmds.push(RenderCommand::FillRect {
                    x: x + pad, y: cy, width: inner, height: 40.0,
                    color: bg,
                    corner_radii: CornerRadii::all(6.0),
                });

                // SSID
                let connected_marker = if net.connected { " ✓" } else { "" };
                let saved_marker = if net.saved && !net.connected { " ★" } else { "" };
                cmds.push(RenderCommand::Text {
                    x: x + pad + 8.0, y: cy + 4.0,
                    text: format!("{}{}{}", net.ssid, connected_marker, saved_marker),
                    font_size: 13.0,
                    color: if net.connected { BLUE } else { TEXT },
                    font_weight: if net.connected { FontWeightHint::Bold } else { FontWeightHint::Regular },
                    max_width: Some(inner * 0.6),
                });

                // Signal + security
                let bars_str: String = (0..4)
                    .map(|b| if b < net.signal.bars() { '█' } else { '░' })
                    .collect();
                cmds.push(RenderCommand::Text {
                    x: x + pad + 8.0, y: cy + 22.0,
                    text: format!("{} {} ch{}", bars_str, net.security.label(), net.channel),
                    font_size: 11.0, color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(inner - 16.0),
                });

                // Signal bars color indicator
                cmds.push(RenderCommand::FillRect {
                    x: x + pad + inner - 28.0, y: cy + 8.0,
                    width: 20.0, height: 20.0,
                    color: net.signal.color(),
                    corner_radii: CornerRadii::all(10.0),
                });

                cy += 44.0;
            }
        }

        cmds
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_type_labels() {
        for ct in [ConnectionType::Ethernet, ConnectionType::Wifi, ConnectionType::VPN, ConnectionType::Cellular, ConnectionType::None] {
            assert!(!ct.label().is_empty());
            assert!(!ct.icon_label().is_empty());
        }
    }

    #[test]
    fn signal_strength_from_rssi() {
        assert_eq!(SignalStrength::from_rssi(-40), SignalStrength::Excellent);
        assert_eq!(SignalStrength::from_rssi(-55), SignalStrength::Good);
        assert_eq!(SignalStrength::from_rssi(-65), SignalStrength::Fair);
        assert_eq!(SignalStrength::from_rssi(-75), SignalStrength::Weak);
        assert_eq!(SignalStrength::from_rssi(-90), SignalStrength::None);
    }

    #[test]
    fn signal_bars() {
        assert_eq!(SignalStrength::None.bars(), 0);
        assert_eq!(SignalStrength::Weak.bars(), 1);
        assert_eq!(SignalStrength::Fair.bars(), 2);
        assert_eq!(SignalStrength::Good.bars(), 3);
        assert_eq!(SignalStrength::Excellent.bars(), 4);
    }

    #[test]
    fn signal_labels_and_colors() {
        for s in [SignalStrength::None, SignalStrength::Weak, SignalStrength::Fair, SignalStrength::Good, SignalStrength::Excellent] {
            assert!(!s.label().is_empty());
            let _ = s.color();
        }
    }

    #[test]
    fn wifi_security_labels() {
        assert!(!WifiSecurity::Open.label().is_empty());
        assert!(!WifiSecurity::WPA3.label().is_empty());
    }

    #[test]
    fn wifi_security_is_secure() {
        assert!(!WifiSecurity::Open.is_secure());
        assert!(!WifiSecurity::WEP.is_secure());
        assert!(WifiSecurity::WPA2.is_secure());
        assert!(WifiSecurity::WPA3.is_secure());
        assert!(WifiSecurity::Enterprise.is_secure());
    }

    #[test]
    fn transfer_rate_format() {
        assert_eq!(TransferRates::format_rate(500), "500 B/s");
        assert_eq!(TransferRates::format_rate(1500), "1.5 KB/s");
        assert_eq!(TransferRates::format_rate(1_500_000), "1.5 MB/s");
        assert_eq!(TransferRates::format_rate(1_500_000_000), "1.5 GB/s");
    }

    #[test]
    fn transfer_bytes_format() {
        assert_eq!(TransferRates::format_bytes(500), "500 B");
        assert!(TransferRates::format_bytes(1_500_000).contains("MB"));
    }

    #[test]
    fn network_state_tooltip_disconnected() {
        let s = NetworkState::disconnected();
        assert_eq!(s.tooltip(), "Not connected");
    }

    #[test]
    fn network_state_tooltip_ethernet() {
        let s = NetworkState::ethernet("eth0", "192.168.1.5");
        assert!(s.tooltip().contains("Ethernet"));
        assert!(s.tooltip().contains("192.168.1.5"));
    }

    #[test]
    fn network_state_tooltip_wifi() {
        let s = NetworkState::wifi("MyNetwork", "10.0.0.2", SignalStrength::Good);
        assert!(s.tooltip().contains("MyNetwork"));
        assert!(s.tooltip().contains("Good"));
    }

    #[test]
    fn network_state_tooltip_airplane() {
        let mut s = NetworkState::disconnected();
        s.airplane_mode = true;
        assert_eq!(s.tooltip(), "Airplane mode");
    }

    #[test]
    fn uptime_formatted() {
        let mut s = NetworkState::disconnected();
        s.connected_secs = 7265;
        assert_eq!(s.uptime_formatted(), "2h 1m");
        s.connected_secs = 125;
        assert_eq!(s.uptime_formatted(), "2m 5s");
        s.connected_secs = 30;
        assert_eq!(s.uptime_formatted(), "30s");
    }

    #[test]
    fn indicator_new() {
        let ind = NetworkIndicator::new();
        assert!(!ind.is_flyout_open());
        assert!(ind.wifi_enabled());
        assert!(ind.wifi_networks().is_empty());
    }

    #[test]
    fn indicator_toggle_flyout() {
        let mut ind = NetworkIndicator::new();
        ind.toggle_flyout();
        assert!(ind.is_flyout_open());
        ind.toggle_flyout();
        assert!(!ind.is_flyout_open());
    }

    #[test]
    fn indicator_close_flyout() {
        let mut ind = NetworkIndicator::new();
        ind.toggle_flyout();
        ind.close_flyout();
        assert!(!ind.is_flyout_open());
    }

    #[test]
    fn indicator_set_wifi_networks_sorted() {
        let mut ind = NetworkIndicator::new();
        let nets = vec![
            WifiNetwork::new("Weak", SignalStrength::Weak, WifiSecurity::WPA2),
            WifiNetwork::new("Strong", SignalStrength::Excellent, WifiSecurity::WPA3),
        ];
        ind.set_wifi_networks(nets);
        assert_eq!(ind.wifi_networks()[0].ssid, "Strong");
        assert_eq!(ind.wifi_networks()[1].ssid, "Weak");
    }

    #[test]
    fn indicator_connected_network_sorted_first() {
        let mut ind = NetworkIndicator::new();
        let mut connected = WifiNetwork::new("B", SignalStrength::Weak, WifiSecurity::Open);
        connected.connected = true;
        let strong = WifiNetwork::new("A", SignalStrength::Excellent, WifiSecurity::WPA3);
        ind.set_wifi_networks(vec![strong, connected]);
        assert_eq!(ind.wifi_networks()[0].ssid, "B"); // connected comes first
    }

    #[test]
    fn indicator_select_navigation() {
        let mut ind = NetworkIndicator::new();
        ind.set_wifi_networks(vec![
            WifiNetwork::new("A", SignalStrength::Good, WifiSecurity::WPA2),
            WifiNetwork::new("B", SignalStrength::Fair, WifiSecurity::WPA2),
            WifiNetwork::new("C", SignalStrength::Weak, WifiSecurity::Open),
        ]);
        ind.select_next();
        assert_eq!(ind.selected_index(), Some(0));
        ind.select_next();
        assert_eq!(ind.selected_index(), Some(1));
        ind.select_next();
        assert_eq!(ind.selected_index(), Some(2));
        ind.select_next(); // wraps
        assert_eq!(ind.selected_index(), Some(0));
    }

    #[test]
    fn indicator_select_prev() {
        let mut ind = NetworkIndicator::new();
        ind.set_wifi_networks(vec![
            WifiNetwork::new("A", SignalStrength::Good, WifiSecurity::WPA2),
            WifiNetwork::new("B", SignalStrength::Fair, WifiSecurity::Open),
        ]);
        ind.select_prev(); // wraps to last
        assert_eq!(ind.selected_index(), Some(1));
        ind.select_prev();
        assert_eq!(ind.selected_index(), Some(0));
    }

    #[test]
    fn indicator_select_empty() {
        let mut ind = NetworkIndicator::new();
        ind.select_next();
        assert_eq!(ind.selected_index(), None);
    }

    #[test]
    fn indicator_toggle_airplane() {
        let mut ind = NetworkIndicator::new();
        assert!(!ind.state().airplane_mode);
        ind.toggle_airplane_mode();
        assert!(ind.state().airplane_mode);
    }

    #[test]
    fn indicator_record_rates() {
        let mut ind = NetworkIndicator::new();
        for i in 0..5 {
            ind.record_rates(i * 100, i * 50);
        }
        assert_eq!(ind.rate_history().len(), 5);
    }

    #[test]
    fn indicator_rate_history_ring() {
        let mut ind = NetworkIndicator::new();
        for i in 0..100 {
            ind.record_rates(i, i);
        }
        assert_eq!(ind.rate_history().len(), 60);
    }

    #[test]
    fn indicator_render_icon() {
        let ind = NetworkIndicator::new();
        let cmds = ind.render_icon(0.0, 0.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn indicator_render_flyout_closed() {
        let ind = NetworkIndicator::new();
        let cmds = ind.render_flyout(0.0, 0.0, 300.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn indicator_render_flyout_open() {
        let mut ind = NetworkIndicator::new();
        ind.set_wifi_networks(vec![
            WifiNetwork::new("TestNet", SignalStrength::Good, WifiSecurity::WPA2),
        ]);
        ind.toggle_flyout();
        let cmds = ind.render_flyout(0.0, 0.0, 300.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn indicator_update_state() {
        let mut ind = NetworkIndicator::new();
        ind.update_state(NetworkState::ethernet("eth0", "10.0.0.1"));
        assert_eq!(ind.state().connection_type, ConnectionType::Ethernet);
    }

    #[test]
    fn wifi_network_new() {
        let n = WifiNetwork::new("Test", SignalStrength::Fair, WifiSecurity::WPA3);
        assert_eq!(n.ssid, "Test");
        assert!(!n.saved);
        assert!(!n.connected);
    }

    #[test]
    fn indicator_wifi_toggle() {
        let mut ind = NetworkIndicator::new();
        ind.set_wifi_enabled(false);
        assert!(!ind.wifi_enabled());
        ind.set_wifi_enabled(true);
        assert!(ind.wifi_enabled());
    }

    #[test]
    fn flyout_resets_selection() {
        let mut ind = NetworkIndicator::new();
        ind.set_wifi_networks(vec![
            WifiNetwork::new("A", SignalStrength::Good, WifiSecurity::WPA2),
        ]);
        ind.select_next();
        assert!(ind.selected_index().is_some());
        ind.toggle_flyout(); // opens, resets selection
        assert!(ind.selected_index().is_none());
    }
}

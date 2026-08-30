//! Network status tray indicator.
//!
//! Shows a compact network icon in the system tray with connection type,
//! signal strength, and data transfer rates. Clicking opens a quick-connect
//! flyout listing available WiFi networks.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::step;
use guitk::style::CornerRadii;

// Colour
// ------
//
// Every colour here is read out of the resolved [`Palette`] the shell hands
// down, so this indicator follows the light/dark mode and the accent the user
// chose. Two judgements are baked into which role each site takes.
//
// 1. **Exactly one site follows the accent:** the SSID of the network you are
//    currently on, in the flyout's list. That is the "which of these am I on"
//    question the accent exists to answer -- the same role the selected power
//    plan takes in [`crate::power_settings`].
//
// 2. **Signal strength is a ladder and stays fixed.** [`SignalStrength::color`]
//    runs red -> yellow -> green -> blue, and the blue rung is a trap: blue is
//    also the *default* accent, so `Excellent => p.accent` would look right on
//    a fresh install and collapse onto the "Good" green the moment a user
//    picked Green. A ladder whose rungs can collide has stopped measuring. The
//    five connection types (none red, ethernet green, wifi by signal, VPN
//    lavender, cellular peach) are categorical for the same reason, and the
//    airplane-mode peach is a status rather than an invitation.

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

    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::None | Self::Weak => p.red,
            Self::Fair => p.yellow,
            Self::Good => p.green,
            Self::Excellent => p.blue,
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
    ///
    /// Decimal, matching the counters below and the resource monitor's network
    /// graph. See design-decisions.md §489.
    pub fn format_rate(bytes_per_sec: u64) -> String {
        guitk::bytes::si_rate(bytes_per_sec)
    }

    /// Format total bytes.
    pub fn format_bytes(bytes: u64) -> String {
        guitk::bytes::si(bytes)
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

    /// Uptime formatted as "Xd Yh", "Xh Ym", "Xm Ys" or "Xs".
    ///
    /// This had no days field, so an Ethernet link left up for a week — the
    /// normal state of a desktop — reported `168h 0m`.
    pub fn uptime_formatted(&self) -> String {
        guitk::duration::coarse(self.connected_secs)
    }

    /// Tooltip text for the tray icon.
    pub fn tooltip(&self) -> String {
        if self.airplane_mode {
            return "Airplane mode".into();
        }
        match self.connection_type {
            ConnectionType::None => "Not connected".into(),
            ConnectionType::Ethernet => {
                format!(
                    "Ethernet — {}",
                    self.ip_address.as_deref().unwrap_or("No IP")
                )
            }
            ConnectionType::Wifi => {
                format!(
                    "{} — {} — {}",
                    self.network_name,
                    self.signal.label(),
                    self.ip_address.as_deref().unwrap_or("No IP")
                )
            }
            ConnectionType::VPN => {
                format!(
                    "VPN: {} — {}",
                    self.network_name,
                    self.ip_address.as_deref().unwrap_or("No IP")
                )
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
            b.connected
                .cmp(&a.connected)
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

    /// Move the highlight to the next network, wrapping past the last.
    ///
    /// Wrapping, unlike the launcher's clamped list: this is a short menu of
    /// nearby networks with no meaningful order, so thumbing off the bottom
    /// and round to the top is what a user expects. The empty-list guard
    /// stays because "no networks" must leave the selection *absent*, which
    /// is a different thing from selecting the zeroth of nothing.
    pub fn select_next(&mut self) {
        if self.wifi_networks.is_empty() {
            return;
        }
        let len = self.wifi_networks.len();
        self.selected_index = Some(match self.selected_index {
            Some(i) => step::wrapping_after(len, i),
            // Nothing selected yet: the first network, not the one after it.
            None => 0,
        });
    }

    /// Move the highlight to the previous network, wrapping past the first.
    pub fn select_prev(&mut self) {
        if self.wifi_networks.is_empty() {
            return;
        }
        let len = self.wifi_networks.len();
        // With nothing selected, stepping back enters the list at the end —
        // which is exactly where wrapping back from the first one lands.
        let from = self.selected_index.unwrap_or(0);
        self.selected_index = Some(step::wrapping_before(len, from));
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
    pub fn render_icon(&self, p: &Palette, x: f32, y: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let icon_color = if self.state.airplane_mode {
            p.overlay0
        } else {
            match self.state.connection_type {
                ConnectionType::None => p.red,
                ConnectionType::Ethernet => p.green,
                ConnectionType::Wifi => self.state.signal.color(p),
                ConnectionType::VPN => p.lavender,
                ConnectionType::Cellular => p.peach,
            }
        };

        // Icon background circle
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 24.0,
            height: 24.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Icon text (emoji or signal bars)
        let label = if self.state.airplane_mode {
            "✈"
        } else {
            self.state.connection_type.icon_label()
        };
        cmds.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: label.into(),
            font_size: 13.0,
            color: icon_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(20.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds
    }

    /// Render the WiFi network flyout popup.
    pub fn render_flyout(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
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
            x,
            y,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Current connection summary
        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: self.state.tooltip(),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 20.0;

        // Transfer rates
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: format!(
                "↓ {}  ↑ {}",
                self.state.rates.rx_formatted(),
                self.state.rates.tx_formatted()
            ),
            font_size: 11.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 20.0;

        // Toggles row
        // Airplane mode toggle
        let airplane_label = if self.state.airplane_mode {
            "✈ Airplane ON"
        } else {
            "✈ Airplane OFF"
        };
        let airplane_color = if self.state.airplane_mode {
            p.peach
        } else {
            p.overlay0
        };
        cmds.push(RenderCommand::FillRect {
            x: x + pad,
            y: cy,
            width: inner * 0.48,
            height: 28.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 8.0,
            y: cy + 6.0,
            text: airplane_label.into(),
            font_size: 12.0,
            color: airplane_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner * 0.45),
            overflow: TextOverflow::Ellipsis,
        });

        // WiFi toggle
        let wifi_label = if self.wifi_enabled {
            "Wi-Fi ON"
        } else {
            "Wi-Fi OFF"
        };
        let wifi_color = if self.wifi_enabled {
            p.green
        } else {
            p.overlay0
        };
        cmds.push(RenderCommand::FillRect {
            x: x + pad + inner * 0.52,
            y: cy,
            width: inner * 0.48,
            height: 28.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + inner * 0.52 + 8.0,
            y: cy + 6.0,
            text: wifi_label.into(),
            font_size: 12.0,
            color: wifi_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner * 0.45),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 36.0;

        // WiFi networks list
        if self.wifi_enabled && !self.state.airplane_mode {
            for (i, net) in self.wifi_networks.iter().enumerate() {
                let selected = self.selected_index == Some(i);
                let bg = if net.connected {
                    p.surface0
                } else if selected {
                    p.surface1
                } else {
                    p.mantle
                };

                cmds.push(RenderCommand::FillRect {
                    x: x + pad,
                    y: cy,
                    width: inner,
                    height: 40.0,
                    color: bg,
                    corner_radii: CornerRadii::all(6.0),
                });

                // SSID
                let connected_marker = if net.connected { " ✓" } else { "" };
                let saved_marker = if net.saved && !net.connected {
                    " ★"
                } else {
                    ""
                };
                cmds.push(RenderCommand::Text {
                    x: x + pad + 8.0,
                    y: cy + 4.0,
                    text: format!("{}{}{}", net.ssid, connected_marker, saved_marker),
                    font_size: 13.0,
                    color: if net.connected { p.accent } else { p.text },
                    font_weight: if net.connected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(inner * 0.6),
                    overflow: TextOverflow::Ellipsis,
                });

                // Signal + security
                let bars_str: String = (0..4)
                    .map(|b| if b < net.signal.bars() { '█' } else { '░' })
                    .collect();
                cmds.push(RenderCommand::Text {
                    x: x + pad + 8.0,
                    y: cy + 22.0,
                    text: format!("{} {} ch{}", bars_str, net.security.label(), net.channel),
                    font_size: 11.0,
                    color: p.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(inner - 16.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Signal bars color indicator
                cmds.push(RenderCommand::FillRect {
                    x: x + pad + inner - 28.0,
                    y: cy + 8.0,
                    width: 20.0,
                    height: 20.0,
                    color: net.signal.color(p),
                    corner_radii: CornerRadii::all(10.0),
                });

                cy += 44.0;
            }
        }

        cmds
    }
}

impl Default for NetworkIndicator {
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

    use super::*;
    use crate::palette_check::assert_drawn_from;

    /// The palette these tests render against: the shell's own dark answer.
    fn test_palette() -> Palette {
        Palette::for_mode(false)
    }

    #[test]
    fn connection_type_labels() {
        for ct in [
            ConnectionType::Ethernet,
            ConnectionType::Wifi,
            ConnectionType::VPN,
            ConnectionType::Cellular,
            ConnectionType::None,
        ] {
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
        for s in [
            SignalStrength::None,
            SignalStrength::Weak,
            SignalStrength::Fair,
            SignalStrength::Good,
            SignalStrength::Excellent,
        ] {
            assert!(!s.label().is_empty());
            let _ = s.color(&test_palette());
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
        assert_eq!(TransferRates::format_rate(1500), "1.5 kB/s");
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
        let cmds = ind.render_icon(&test_palette(), 0.0, 0.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn indicator_render_flyout_closed() {
        let ind = NetworkIndicator::new();
        let cmds = ind.render_flyout(&test_palette(), 0.0, 0.0, 300.0);
        assert!(cmds.is_empty());
    }

    #[test]
    fn indicator_render_flyout_open() {
        let mut ind = NetworkIndicator::new();
        ind.set_wifi_networks(vec![WifiNetwork::new(
            "TestNet",
            SignalStrength::Good,
            WifiSecurity::WPA2,
        )]);
        ind.toggle_flyout();
        let cmds = ind.render_flyout(&test_palette(), 0.0, 0.0, 300.0);
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
        ind.set_wifi_networks(vec![WifiNetwork::new(
            "A",
            SignalStrength::Good,
            WifiSecurity::WPA2,
        )]);
        ind.select_next();
        assert!(ind.selected_index().is_some());
        ind.toggle_flyout(); // opens, resets selection
        assert!(ind.selected_index().is_none());
    }

    // --- Palette conversion --------------------------------------------------

    /// An indicator wound to one state, with a scan list to draw.
    ///
    /// The list carries a connected network, a saved-but-not-connected one and
    /// a stranger, because the SSID row colours all three differently and the
    /// row background distinguishes connected from merely selected.
    fn wound(state: NetworkState, wifi_on: bool, selected: Option<usize>) -> NetworkIndicator {
        let mut ind = NetworkIndicator::new();
        ind.update_state(state);
        ind.set_wifi_enabled(wifi_on);
        let mut here = WifiNetwork::new("here", SignalStrength::Excellent, WifiSecurity::WPA3);
        here.connected = true;
        here.saved = true;
        here.channel = 44;
        let mut known = WifiNetwork::new("known", SignalStrength::Fair, WifiSecurity::WPA2);
        known.saved = true;
        known.channel = 6;
        let stranger = WifiNetwork::new("stranger", SignalStrength::Weak, WifiSecurity::Open);
        ind.set_wifi_networks(vec![here, known, stranger]);
        ind.toggle_flyout();
        for _ in 0..selected.map_or(0, |i| i + 1) {
            ind.select_next();
        }
        ind
    }

    /// Every state either render can be in, walked in both modes.
    ///
    /// A leftover constant is a Mocha value the Latte palette does not contain,
    /// so the light render names it. Airplane mode and a disabled radio are
    /// walked because each *removes* rather than recolours: airplane mode
    /// replaces the tray icon's hue with one grey and the flyout's list with
    /// nothing, and a disabled radio drops the list alone. A fixture that was
    /// always connected with the radio on would render neither branch.
    #[test]
    fn every_colour_either_render_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for signal in [
                SignalStrength::None,
                SignalStrength::Weak,
                SignalStrength::Fair,
                SignalStrength::Good,
                SignalStrength::Excellent,
            ] {
                for ct in [
                    ConnectionType::None,
                    ConnectionType::Ethernet,
                    ConnectionType::Wifi,
                    ConnectionType::VPN,
                    ConnectionType::Cellular,
                ] {
                    for airplane in [false, true] {
                        for wifi_on in [false, true] {
                            for selected in [None, Some(0), Some(1)] {
                                let mut state = NetworkState::wifi("here", "10.0.0.2", signal);
                                state.connection_type = ct;
                                state.airplane_mode = airplane;
                                state.rates.rx_bytes_per_sec = 1_500_000;
                                state.rates.tx_bytes_per_sec = 96_000;
                                state.connected_secs = 90_061;
                                let ind = wound(state, wifi_on, selected);
                                assert_drawn_from(
                                    &p,
                                    &ind.render_icon(&p, 0.0, 0.0),
                                    &[],
                                    "network tray icon",
                                );
                                for width in [240.0_f32, 300.0, 420.0] {
                                    let cmds = ind.render_flyout(&p, 0.0, 0.0, width);
                                    assert_drawn_from(&p, &cmds, &[], "network flyout");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// The SSID of the network you are on — 13pt, bold, carrying the ✓ marker.
    fn connected_ssid_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    color,
                    font_size: 13.0,
                    font_weight: FontWeightHint::Bold,
                    ..
                } if text.contains('\u{2713}') => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The round signal swatch at the right of every row in the scan list.
    fn signal_swatch_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width: 20.0,
                    height: 20.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The tray icon's glyph — the only text `render_icon` emits.
    fn tray_glyph_color(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The two toggle labels in the flyout — 12pt, regular.
    fn toggle_label_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    color,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Exactly one thing here follows the accent, and it is not the signal.
    ///
    /// The membership sweep is blind to this: `p.blue` and `p.accent` are both
    /// legal palette roles, so writing one where the other belongs still draws
    /// a colour the light palette contains. Only varying the accent separates
    /// them.
    #[test]
    fn only_the_network_you_are_on_follows_the_accent() {
        let mut blue = Palette::for_mode(false);
        blue.accent = appearance::BLUE;
        let mut mauve = Palette::for_mode(false);
        mauve.accent = appearance::MAUVE;

        let state = NetworkState::wifi("here", "10.0.0.2", SignalStrength::Excellent);
        let ind = wound(state, true, Some(1));
        let a = ind.render_flyout(&blue, 0.0, 0.0, 300.0);
        let b = ind.render_flyout(&mauve, 0.0, 0.0, 300.0);

        // The negative half. Without it every assertion below would also pass
        // on an indicator that ignored the accent entirely.
        let ssid = connected_ssid_colors(&a);
        assert_eq!(
            ssid.len(),
            1,
            "exactly one network is connected in this fixture"
        );
        assert_ne!(
            ssid,
            connected_ssid_colors(&b),
            "the connected network's name did not move with the accent"
        );

        // The positive half, one assertion per site that must not move.
        let swatches = signal_swatch_colors(&a);
        assert_eq!(swatches.len(), 3, "one swatch per network in the scan list");
        assert_eq!(
            swatches,
            signal_swatch_colors(&b),
            "a signal-strength swatch moved with the accent; signal is a \
             measurement, and its ladder has to stay legible under every accent"
        );

        // Both toggles have an on colour and an off colour, and only one of
        // the two is on screen at a time, so *both settings have to be walked*.
        // A fixture that left airplane mode off renders the off grey twice and
        // compares it with itself: harness defect PPPP repainted the airplane
        // *on* peach with the accent and no test noticed, which is what put
        // this loop here. Whenever a colour is chosen by a boolean, the test
        // needs the boolean, not just the render.
        for airplane in [false, true] {
            for wifi_on in [false, true] {
                let mut state = NetworkState::wifi("here", "10.0.0.2", SignalStrength::Excellent);
                state.airplane_mode = airplane;
                let ind = wound(state, wifi_on, None);
                let toggles = toggle_label_colors(&ind.render_flyout(&blue, 0.0, 0.0, 300.0));
                assert_eq!(toggles.len(), 2, "the airplane and Wi-Fi toggles");
                assert_eq!(
                    toggles,
                    toggle_label_colors(&ind.render_flyout(&mauve, 0.0, 0.0, 300.0)),
                    "a toggle label moved with the accent (airplane={airplane}, \
                     wifi={wifi_on}); airplane-on and radio-on are statuses, not \
                     invitations"
                );
            }
        }

        for ct in [
            ConnectionType::None,
            ConnectionType::Ethernet,
            ConnectionType::Wifi,
            ConnectionType::VPN,
            ConnectionType::Cellular,
        ] {
            let mut state = NetworkState::wifi("here", "10.0.0.2", SignalStrength::Excellent);
            state.connection_type = ct;
            let ind = wound(state, true, None);
            assert_eq!(
                tray_glyph_color(&ind.render_icon(&blue, 0.0, 0.0)),
                tray_glyph_color(&ind.render_icon(&mauve, 0.0, 0.0)),
                "the tray icon for {ct:?} moved with the accent; which kind of \
                 link you have is a category, not a selection"
            );
        }
    }

    /// Signal strength is a ladder, and its rungs may not collide.
    ///
    /// This is the strongest form of the distinctness argument so far: the
    /// flyout lists *every* visible network with its own swatch, so two rungs
    /// sharing a colour do not merely confuse a user's memory — they make two
    /// networks of different strength look identical side by side in one
    /// glance. The blue rung is the trap: blue is also the default accent, so
    /// `Excellent => p.accent` looks correct on a fresh install and collapses
    /// onto the green "Good" rung the moment a user picks Green.
    #[test]
    fn signal_strength_stays_a_ladder_under_every_accent() {
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

                let mut seen: Vec<(SignalStrength, Color)> = Vec::new();
                for s in [
                    SignalStrength::Weak,
                    SignalStrength::Fair,
                    SignalStrength::Good,
                    SignalStrength::Excellent,
                ] {
                    let c = s.color(&p);
                    if let Some((other, _)) = seen
                        .iter()
                        .find(|(_, o)| o.r == c.r && o.g == c.g && o.b == c.b)
                    {
                        panic!(
                            "a {s:?} signal is drawn exactly like a {other:?} one \
                             (light={light}), so the scan list cannot say which \
                             network is stronger"
                        );
                    }
                    seen.push((s, c));
                }
            }
        }
    }

    /// The five kinds of link stay tellable apart in the tray.
    ///
    /// Weaker than the ladder above — only one icon is on screen at a time, so
    /// this is about a code the user has learnt rather than a side-by-side
    /// comparison. It still has to hold: an icon that is green for Ethernet and
    /// green for a VPN has stopped saying anything. `Wifi` is excluded because
    /// it has no colour of its own; it delegates to the signal ladder, which
    /// legitimately overlaps the other four.
    #[test]
    fn the_five_kinds_of_link_stay_distinct_in_both_modes() {
        for light in [false, true] {
            for accent in [appearance::BLUE, appearance::GREEN, appearance::MAUVE] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                let mut seen: Vec<(ConnectionType, Color)> = Vec::new();
                for ct in [
                    ConnectionType::None,
                    ConnectionType::Ethernet,
                    ConnectionType::VPN,
                    ConnectionType::Cellular,
                ] {
                    let mut state = NetworkState::wifi("here", "10.0.0.2", SignalStrength::Good);
                    state.connection_type = ct;
                    let ind = wound(state, true, None);
                    let cmds = ind.render_icon(&p, 0.0, 0.0);
                    let c = *tray_glyph_color(&cmds)
                        .first()
                        .expect("the tray icon draws exactly one glyph");
                    if let Some((other, _)) = seen
                        .iter()
                        .find(|(_, o)| o.r == c.r && o.g == c.g && o.b == c.b)
                    {
                        panic!(
                            "the tray icon for {ct:?} is drawn exactly like the \
                             one for {other:?} (light={light})"
                        );
                    }
                    seen.push((ct, c));
                }
            }
        }
    }
}

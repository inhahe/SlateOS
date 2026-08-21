//! Bluetooth Manager
//!
//! Desktop bluetooth device management:
//!
//! - Device discovery and pairing
//! - Connected device list with battery levels
//! - Audio device routing
//! - File transfer (OBEX)
//! - Device profiles (A2DP, HFP, HID, etc.)
//! - Auto-connect for known devices
//! - System tray indicator

use guitk::color::Color;
use guitk::ratio;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::scroll_window;
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);
const MOCHA_LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ============================================================================
// Device types and profiles
// ============================================================================

/// Bluetooth device type/category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BluetoothDeviceType {
    Headphones,
    Speaker,
    Keyboard,
    Mouse,
    Gamepad,
    Phone,
    Computer,
    Watch,
    Printer,
    Other,
}

impl BluetoothDeviceType {
    pub fn label(&self) -> &str {
        match self {
            Self::Headphones => "Headphones",
            Self::Speaker => "Speaker",
            Self::Keyboard => "Keyboard",
            Self::Mouse => "Mouse",
            Self::Gamepad => "Gamepad",
            Self::Phone => "Phone",
            Self::Computer => "Computer",
            Self::Watch => "Watch",
            Self::Printer => "Printer",
            Self::Other => "Other",
        }
    }

    pub fn icon_char(&self) -> char {
        match self {
            Self::Headphones => 'H',
            Self::Speaker => 'S',
            Self::Keyboard => 'K',
            Self::Mouse => 'M',
            Self::Gamepad => 'G',
            Self::Phone => 'P',
            Self::Computer => 'C',
            Self::Watch => 'W',
            Self::Printer => 'R',
            Self::Other => '?',
        }
    }
}

/// Bluetooth profiles supported by a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BluetoothProfile {
    /// Advanced Audio Distribution Profile (music streaming).
    A2dp,
    /// Hands-Free Profile (phone calls).
    Hfp,
    /// Human Interface Device (keyboard/mouse).
    Hid,
    /// Audio/Video Remote Control.
    Avrcp,
    /// Object Push Profile (file transfer).
    Opp,
    /// Serial Port Profile.
    Spp,
    /// Personal Area Network.
    Pan,
}

impl BluetoothProfile {
    pub fn label(&self) -> &str {
        match self {
            Self::A2dp => "A2DP (Audio)",
            Self::Hfp => "HFP (Hands-Free)",
            Self::Hid => "HID (Input)",
            Self::Avrcp => "AVRCP (Remote)",
            Self::Opp => "OPP (File Transfer)",
            Self::Spp => "SPP (Serial)",
            Self::Pan => "PAN (Network)",
        }
    }
}

// ============================================================================
// Connection state
// ============================================================================

/// Bluetooth connection state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Paired,
    /// Paired but not currently connected.
    PairedNotConnected,
}

impl ConnectionState {
    pub fn label(&self) -> &str {
        match self {
            Self::Disconnected => "Not paired",
            Self::Connecting => "Connecting...",
            Self::Connected => "Connected",
            Self::Disconnecting => "Disconnecting...",
            Self::Paired => "Paired",
            Self::PairedNotConnected => "Paired (not connected)",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::Connected => MOCHA_GREEN,
            Self::Connecting | Self::Disconnecting => MOCHA_YELLOW,
            Self::Paired | Self::PairedNotConnected => MOCHA_BLUE,
            Self::Disconnected => MOCHA_OVERLAY0,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

// ============================================================================
// Bluetooth device
// ============================================================================

/// A discovered or paired bluetooth device.
#[derive(Clone, Debug)]
pub struct BluetoothDevice {
    /// Unique device address (XX:XX:XX:XX:XX:XX).
    pub address: String,
    /// Friendly name.
    pub name: String,
    /// Device type.
    pub device_type: BluetoothDeviceType,
    /// Current connection state.
    pub state: ConnectionState,
    /// Signal strength (RSSI, negative dBm, closer to 0 = stronger).
    pub rssi: Option<i8>,
    /// Battery level (0-100%), if reported.
    pub battery: Option<u8>,
    /// Supported profiles.
    pub profiles: Vec<BluetoothProfile>,
    /// Auto-connect when in range.
    pub auto_connect: bool,
    /// Whether this device is trusted (no confirmation needed).
    pub trusted: bool,
    /// Last seen timestamp.
    pub last_seen: u64,
}

impl BluetoothDevice {
    /// Signal strength as bars (0-4).
    pub fn signal_bars(&self) -> u8 {
        match self.rssi {
            None => 0,
            Some(rssi) => {
                if rssi > -50 {
                    4
                } else if rssi > -60 {
                    3
                } else if rssi > -70 {
                    2
                } else if rssi > -80 {
                    1
                } else {
                    0
                }
            }
        }
    }

    /// Battery display string.
    pub fn battery_display(&self) -> String {
        match self.battery {
            Some(pct) => format!("{}%", pct),
            None => "N/A".to_string(),
        }
    }

    /// Whether this device supports audio.
    pub fn is_audio(&self) -> bool {
        self.profiles.contains(&BluetoothProfile::A2dp)
            || self.profiles.contains(&BluetoothProfile::Hfp)
    }

    /// Whether this device is an input device.
    pub fn is_input(&self) -> bool {
        self.profiles.contains(&BluetoothProfile::Hid)
    }
}

// ============================================================================
// Bluetooth adapter
// ============================================================================

/// The local bluetooth adapter/controller.
#[derive(Clone, Debug)]
pub struct BluetoothAdapter {
    pub name: String,
    pub address: String,
    pub powered: bool,
    pub discoverable: bool,
    pub discovering: bool,
    pub version: String,
}

impl BluetoothAdapter {
    pub fn default_adapter() -> Self {
        Self {
            name: "Built-in Bluetooth".to_string(),
            address: "00:00:00:00:00:00".to_string(),
            powered: true,
            discoverable: false,
            discovering: false,
            version: "5.3".to_string(),
        }
    }
}

// ============================================================================
// File transfer
// ============================================================================

/// A file transfer operation (OBEX).
#[derive(Clone, Debug)]
pub struct FileTransfer {
    pub id: u32,
    pub device_address: String,
    pub filename: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub sending: bool, // true = sending, false = receiving
    pub completed: bool,
    pub failed: bool,
}

impl FileTransfer {
    /// Progress as a percentage (0-100). A transfer that has not yet been
    /// told its size is at 0%, not at 100%.
    #[must_use]
    pub fn progress_pct(&self) -> u32 {
        ratio::percent_whole(self.transferred_bytes, self.total_bytes).unwrap_or(0)
    }
}

// ============================================================================
// Bluetooth Manager
// ============================================================================

const MAX_DEVICES: usize = 64;

/// Manages bluetooth adapter, devices, and connections.
pub struct BluetoothManager {
    pub adapter: BluetoothAdapter,
    pub devices: Vec<BluetoothDevice>,
    pub transfers: Vec<FileTransfer>,
    next_transfer_id: u32,
}

impl BluetoothManager {
    pub fn new() -> Self {
        Self {
            adapter: BluetoothAdapter::default_adapter(),
            devices: Vec::new(),
            transfers: Vec::new(),
            next_transfer_id: 1,
        }
    }

    /// Toggle bluetooth power.
    pub fn set_powered(&mut self, on: bool) {
        self.adapter.powered = on;
        if !on {
            self.adapter.discovering = false;
            self.adapter.discoverable = false;
            // Disconnect all.
            for d in &mut self.devices {
                if d.state.is_connected() {
                    d.state = ConnectionState::PairedNotConnected;
                }
            }
        }
    }

    /// Start device discovery scan.
    pub fn start_discovery(&mut self) -> bool {
        if !self.adapter.powered {
            return false;
        }
        self.adapter.discovering = true;
        true
    }

    /// Stop discovery.
    pub fn stop_discovery(&mut self) {
        self.adapter.discovering = false;
    }

    /// Toggle discoverable mode.
    pub fn set_discoverable(&mut self, on: bool) -> bool {
        if !self.adapter.powered {
            return false;
        }
        self.adapter.discoverable = on;
        true
    }

    /// Add a discovered device. Returns true if new.
    pub fn add_discovered_device(&mut self, device: BluetoothDevice) -> bool {
        if self.devices.len() >= MAX_DEVICES {
            return false;
        }
        if self.devices.iter().any(|d| d.address == device.address) {
            // Update existing.
            if let Some(d) = self
                .devices
                .iter_mut()
                .find(|d| d.address == device.address)
            {
                d.rssi = device.rssi;
                d.last_seen = device.last_seen;
                if d.name.is_empty() && !device.name.is_empty() {
                    d.name = device.name;
                }
            }
            false
        } else {
            self.devices.push(device);
            true
        }
    }

    /// Pair with a device.
    pub fn pair(&mut self, address: &str) -> bool {
        if let Some(d) = self.devices.iter_mut().find(|d| d.address == address)
            && d.state == ConnectionState::Disconnected
        {
            d.state = ConnectionState::Connecting;
            return true;
        }
        false
    }

    /// Complete pairing (callback from pairing agent).
    pub fn complete_pairing(&mut self, address: &str, success: bool) {
        if let Some(d) = self.devices.iter_mut().find(|d| d.address == address) {
            if success {
                d.state = ConnectionState::Connected;
                d.trusted = true;
            } else {
                d.state = ConnectionState::Disconnected;
            }
        }
    }

    /// Connect to a paired device.
    pub fn connect(&mut self, address: &str) -> bool {
        if let Some(d) = self.devices.iter_mut().find(|d| d.address == address)
            && matches!(
                d.state,
                ConnectionState::Paired | ConnectionState::PairedNotConnected
            )
        {
            d.state = ConnectionState::Connecting;
            return true;
        }
        false
    }

    /// Complete connection.
    pub fn complete_connect(&mut self, address: &str, success: bool) {
        if let Some(d) = self.devices.iter_mut().find(|d| d.address == address) {
            if success {
                d.state = ConnectionState::Connected;
            } else {
                d.state = ConnectionState::PairedNotConnected;
            }
        }
    }

    /// Disconnect a device.
    pub fn disconnect(&mut self, address: &str) -> bool {
        if let Some(d) = self.devices.iter_mut().find(|d| d.address == address)
            && d.state.is_connected()
        {
            d.state = ConnectionState::PairedNotConnected;
            return true;
        }
        false
    }

    /// Remove (unpair) a device.
    pub fn remove_device(&mut self, address: &str) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.address != address);
        self.devices.len() < before
    }

    /// Toggle auto-connect for a device.
    pub fn set_auto_connect(&mut self, address: &str, auto: bool) -> bool {
        if let Some(d) = self.devices.iter_mut().find(|d| d.address == address) {
            d.auto_connect = auto;
            true
        } else {
            false
        }
    }

    /// Get all connected devices.
    pub fn connected_devices(&self) -> Vec<&BluetoothDevice> {
        self.devices
            .iter()
            .filter(|d| d.state.is_connected())
            .collect()
    }

    /// Get all paired devices (connected or not).
    pub fn paired_devices(&self) -> Vec<&BluetoothDevice> {
        self.devices
            .iter()
            .filter(|d| {
                matches!(
                    d.state,
                    ConnectionState::Connected
                        | ConnectionState::Paired
                        | ConnectionState::PairedNotConnected
                )
            })
            .collect()
    }

    /// Get nearby (discovered but not paired) devices.
    pub fn nearby_devices(&self) -> Vec<&BluetoothDevice> {
        self.devices
            .iter()
            .filter(|d| d.state == ConnectionState::Disconnected)
            .collect()
    }

    /// Start a file transfer.
    pub fn send_file(&mut self, address: &str, filename: &str, size: u64) -> Option<u32> {
        if !self
            .devices
            .iter()
            .any(|d| d.address == address && d.state.is_connected())
        {
            return None;
        }
        let id = self.next_transfer_id;
        self.next_transfer_id = self.next_transfer_id.saturating_add(1);
        self.transfers.push(FileTransfer {
            id,
            device_address: address.to_string(),
            filename: filename.to_string(),
            total_bytes: size,
            transferred_bytes: 0,
            sending: true,
            completed: false,
            failed: false,
        });
        Some(id)
    }

    /// Advance a transfer.
    pub fn advance_transfer(&mut self, id: u32, bytes: u64) {
        if let Some(t) = self
            .transfers
            .iter_mut()
            .find(|t| t.id == id && !t.completed && !t.failed)
        {
            t.transferred_bytes = t.transferred_bytes.saturating_add(bytes);
            if t.transferred_bytes >= t.total_bytes {
                t.transferred_bytes = t.total_bytes;
                t.completed = true;
            }
        }
    }

    /// Fail a transfer.
    pub fn fail_transfer(&mut self, id: u32) {
        if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
            t.failed = true;
        }
    }

    /// Count connected audio devices.
    pub fn audio_device_count(&self) -> usize {
        self.connected_devices()
            .iter()
            .filter(|d| d.is_audio())
            .count()
    }
}

impl Default for BluetoothManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Settings UI
// ============================================================================

/// One line of the flattened device list: either a section heading or a device.
///
/// Flattening the three sections into a single sequence is what lets one
/// `scroll_offset` and one height budget govern all of them; kept private
/// because it is a rendering detail, not part of the panel's interface.
enum DeviceListRow<'a> {
    Heading(String),
    Device(&'a BluetoothDevice),
}

impl DeviceListRow<'_> {
    /// Vertical space this row occupies, including the gap beneath it.
    fn height(&self) -> f32 {
        match self {
            // 8px of lead-in above the label plus the label's own 22px.
            Self::Heading(_) => 30.0,
            Self::Device(_) => 48.0,
        }
    }
}

/// Bluetooth settings panel.
pub struct BluetoothSettingsUI {
    pub selected_device_idx: Option<usize>,
    pub scroll_offset: usize,
    pub show_nearby: bool,
}

impl BluetoothSettingsUI {
    /// Height reserved below the device list for the "n more" line.
    const LIST_FOOTER_H: f32 = 16.0;

    pub fn new() -> Self {
        Self {
            selected_device_idx: None,
            scroll_offset: 0,
            show_nearby: false,
        }
    }

    /// Render the bluetooth settings panel.
    pub fn render(
        &self,
        mgr: &BluetoothManager,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title bar.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: 40.0,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Bluetooth".to_string(),
            font_size: 16.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Power toggle.
        let power_x = x + w - 80.0;
        cmds.push(RenderCommand::FillRect {
            x: power_x,
            y: y + 11.0,
            width: 36.0,
            height: 18.0,
            color: if mgr.adapter.powered {
                MOCHA_BLUE
            } else {
                MOCHA_SURFACE1
            },
            corner_radii: CornerRadii::all(9.0),
        });
        let knob_x = if mgr.adapter.powered {
            power_x + 20.0
        } else {
            power_x + 2.0
        };
        cmds.push(RenderCommand::FillRect {
            x: knob_x,
            y: y + 13.0,
            width: 14.0,
            height: 14.0,
            color: MOCHA_TEXT,
            corner_radii: CornerRadii::all(7.0),
        });

        if !mgr.adapter.powered {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: y + 60.0,
                text: "Bluetooth is turned off".to_string(),
                font_size: 14.0,
                color: MOCHA_OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            return cmds;
        }

        let mut cy = y + 48.0;

        // Adapter info.
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: cy,
            text: format!("{} (v{})", mgr.adapter.name, mgr.adapter.version),
            font_size: 11.0,
            color: MOCHA_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 20.0;

        // Discovery button.
        let disc_color = if mgr.adapter.discovering {
            MOCHA_PEACH
        } else {
            MOCHA_BLUE
        };
        let disc_label = if mgr.adapter.discovering {
            "Scanning..."
        } else {
            "Scan for devices"
        };
        cmds.push(RenderCommand::FillRect {
            x: x + 16.0,
            y: cy,
            width: 140.0,
            height: 28.0,
            color: disc_color,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 28.0,
            y: cy + 7.0,
            text: disc_label.to_string(),
            font_size: 12.0,
            color: MOCHA_BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 40.0;

        // The three sections become one flat list before anything is drawn, so
        // that the panel height bounds the whole thing rather than each section
        // separately. Headings are entries in their own right: a list scrolled
        // to start mid-section would otherwise show devices under no label.
        let connected = mgr.connected_devices();
        let paired_not_connected: Vec<&BluetoothDevice> = mgr
            .devices
            .iter()
            .filter(|d| {
                matches!(
                    d.state,
                    ConnectionState::PairedNotConnected | ConnectionState::Paired
                )
            })
            .collect();
        let nearby = if self.show_nearby {
            mgr.nearby_devices()
        } else {
            Vec::new()
        };

        let mut rows: Vec<DeviceListRow<'_>> = Vec::new();
        for (label, devices) in [
            ("Connected", &connected),
            ("Paired", &paired_not_connected),
            ("Nearby", &nearby),
        ] {
            if devices.is_empty() {
                continue;
            }
            rows.push(DeviceListRow::Heading(format!(
                "{label} ({})",
                devices.len()
            )));
            rows.extend(devices.iter().map(|d| DeviceListRow::Device(d)));
        }

        let heights: Vec<f32> = rows.iter().map(DeviceListRow::height).collect();
        // Leave room for the "n more" line that goes underneath.
        let window = scroll_window::visible_variable(
            &heights,
            y + h - cy - Self::LIST_FOOTER_H,
            self.scroll_offset,
        );

        for row in rows.get(window.start..window.end()).unwrap_or_default() {
            match row {
                DeviceListRow::Heading(text) => {
                    cmds.push(RenderCommand::Text {
                        x: x + 16.0,
                        y: cy + 8.0,
                        text: text.clone(),
                        font_size: 13.0,
                        color: MOCHA_TEXT,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }
                DeviceListRow::Device(dev) => {
                    self.render_device_row(&mut cmds, dev, x + 16.0, cy, w - 32.0);
                }
            }
            cy += row.height();
        }

        // Only say something when there is something the user cannot see; an
        // unscrolled list that fits needs no commentary.
        if window.count < rows.len() {
            let hidden = rows.len().saturating_sub(window.count);
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: cy + 2.0,
                text: format!("{hidden} more - scroll to see the rest"),
                font_size: 10.0,
                color: MOCHA_OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        cmds
    }

    fn render_device_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        dev: &BluetoothDevice,
        x: f32,
        y: f32,
        w: f32,
    ) {
        // Row background.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: 44.0,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Icon circle.
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: y + 8.0,
            width: 28.0,
            height: 28.0,
            color: MOCHA_LAVENDER,
            corner_radii: CornerRadii::all(14.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 13.0,
            text: dev.device_type.icon_char().to_string(),
            font_size: 14.0,
            color: MOCHA_BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Name.
        cmds.push(RenderCommand::Text {
            x: x + 44.0,
            y: y + 8.0,
            text: dev.name.clone(),
            font_size: 12.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Status.
        cmds.push(RenderCommand::Text {
            x: x + 44.0,
            y: y + 26.0,
            text: dev.state.label().to_string(),
            font_size: 10.0,
            color: dev.state.color(),
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Battery (right side).
        if let Some(bat) = dev.battery {
            let bat_color = if bat > 50 {
                MOCHA_GREEN
            } else if bat > 20 {
                MOCHA_YELLOW
            } else {
                MOCHA_RED
            };
            cmds.push(RenderCommand::Text {
                x: x + w - 60.0,
                y: y + 8.0,
                text: format!("{}%", bat),
                font_size: 11.0,
                color: bat_color,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Signal bars.
        let bars = dev.signal_bars();
        let bar_x = x + w - 60.0;
        for i in 0..4u8 {
            let bh = 4.0 + (i as f32) * 3.0;
            let color = if i < bars { MOCHA_BLUE } else { MOCHA_SURFACE1 };
            cmds.push(RenderCommand::FillRect {
                x: bar_x + (i as f32) * 6.0,
                y: y + 30.0 - bh,
                width: 4.0,
                height: bh,
                color,
                corner_radii: CornerRadii::all(1.0),
            });
        }
    }
}

impl Default for BluetoothSettingsUI {
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

    fn sample_device(address: &str, name: &str) -> BluetoothDevice {
        BluetoothDevice {
            address: address.to_string(),
            name: name.to_string(),
            device_type: BluetoothDeviceType::Headphones,
            state: ConnectionState::Disconnected,
            rssi: Some(-55),
            battery: Some(80),
            profiles: vec![BluetoothProfile::A2dp, BluetoothProfile::Avrcp],
            auto_connect: false,
            trusted: false,
            last_seen: 1000,
        }
    }

    // --- Device list bounding -----------------------------------------------
    //
    // Same defect as `touchpad.rs` had, in the same shape: `render` took the
    // panel height, spent it on the background rectangle, and then looped over
    // three device lists without ever consulting it, while `scroll_offset` sat
    // public and unread. A machine that has seen a lot of nearby devices drew
    // them over whatever was beneath the panel, unscrollably. See
    // `scroll_window.rs` for the shared primitive both now use.

    /// A powered manager holding `n` paired devices, each with a unique name.
    fn mgr_with_paired(n: usize) -> BluetoothManager {
        let mut mgr = BluetoothManager::new();
        mgr.adapter.powered = true;
        mgr.devices = (0..n)
            .map(|i| {
                let mut d = sample_device(&format!("00:00:00:00:00:{i:02X}"), &format!("dev{i}"));
                d.state = ConnectionState::PairedNotConnected;
                d
            })
            .collect();
        mgr
    }

    /// The device names actually drawn, in order.
    fn drawn_device_names(cmds: &[RenderCommand]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } if text.starts_with("dev") => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn lowest_pixel(cmds: &[RenderCommand]) -> Option<f32> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { y, height, .. } => Some(y + height),
                RenderCommand::Text { y, font_size, .. } => Some(y + font_size),
                RenderCommand::Line { y1, y2, .. } => Some(y1.max(*y2)),
                _ => None,
            })
            .fold(None, |acc: Option<f32>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            })
    }

    #[test]
    fn a_device_list_longer_than_the_panel_is_not_drawn_past_its_bottom_edge() {
        let ui = BluetoothSettingsUI::new();
        for count in [0_usize, 1, 4, 30, 300] {
            let mgr = mgr_with_paired(count);
            for h in [300.0_f32, 420.0, 517.0, 800.0] {
                let cmds = ui.render(&mgr, 10.0, 20.0, 600.0, h);
                let bottom = 20.0 + h;
                let low = lowest_pixel(&cmds).unwrap_or(bottom);
                assert!(
                    low <= bottom,
                    "{count} devices in a {h}px panel drew down to {low}, \
                     past the bottom edge at {bottom}"
                );
            }
        }
    }

    #[test]
    fn scrolling_the_device_list_moves_it_by_one_entry() {
        // Entry, not device: the section heading is an entry too, so the first
        // scroll step retires the heading and the next retires a device. That is
        // the intended behaviour — scrolling past a heading should be possible —
        // and asserting it here is what stops a later "skip headings" change
        // from silently making the last device unreachable again.
        let mgr = mgr_with_paired(40);
        let mut ui = BluetoothSettingsUI::new();

        ui.scroll_offset = 0;
        let unscrolled = drawn_device_names(&ui.render(&mgr, 10.0, 20.0, 600.0, 500.0));
        assert!(!unscrolled.is_empty(), "a 500px panel should show devices");
        assert_eq!(
            unscrolled[0], "dev0",
            "an unscrolled list starts at the top"
        );

        ui.scroll_offset = 1;
        let past_heading = drawn_device_names(&ui.render(&mgr, 10.0, 20.0, 600.0, 500.0));
        assert_eq!(
            past_heading[0], "dev0",
            "the first step scrolls the heading away, not a device"
        );

        for step in 2..=6_usize {
            ui.scroll_offset = step;
            let rows = drawn_device_names(&ui.render(&mgr, 10.0, 20.0, 600.0, 500.0));
            assert_eq!(
                rows[0],
                format!("dev{}", step - 1),
                "offset {step} should start at device {}",
                step - 1
            );
        }
    }

    #[test]
    fn a_device_scroll_offset_past_the_end_shows_the_last_page() {
        let mgr = mgr_with_paired(12);
        let mut ui = BluetoothSettingsUI::new();

        ui.scroll_offset = 0;
        let page = drawn_device_names(&ui.render(&mgr, 10.0, 20.0, 600.0, 500.0));
        assert!(
            page.len() < 12,
            "the panel must truncate the list for this test to mean anything"
        );

        for offset in [13_usize, 50, usize::MAX] {
            ui.scroll_offset = offset;
            let rows = drawn_device_names(&ui.render(&mgr, 10.0, 20.0, 600.0, 500.0));
            assert_eq!(
                rows.last().map(String::as_str),
                Some("dev11"),
                "offset {offset} should be pinned to the end of the list"
            );
        }
    }

    #[test]
    fn a_truncated_device_list_says_how_many_are_hidden() {
        let ui = BluetoothSettingsUI::new();
        let cmds = ui.render(&mgr_with_paired(60), 10.0, 20.0, 600.0, 500.0);
        let shown = drawn_device_names(&cmds).len();
        // 60 devices plus one heading, minus what fits.
        let hidden = 61 - shown - 1;
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == &format!("{hidden} more - scroll to see the rest")
            )),
            "a truncated list should say how much is hidden (expected {hidden})"
        );

        let short = ui.render(&mgr_with_paired(2), 10.0, 20.0, 600.0, 500.0);
        assert!(
            !short.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text.ends_with("scroll to see the rest")
            )),
            "a list that fits should not claim anything is hidden"
        );
    }

    #[test]
    fn a_device_panel_with_no_room_draws_no_devices_and_does_not_panic() {
        let ui = BluetoothSettingsUI::new();
        for h in [0.0_f32, 1.0, 100.0, -30.0, f32::NAN] {
            let cmds = ui.render(&mgr_with_paired(40), 10.0, 20.0, 600.0, h);
            assert!(
                drawn_device_names(&cmds).is_empty(),
                "a {h}px panel has no room for device rows, but drew some"
            );
        }
    }

    // --- BluetoothDeviceType ---
    #[test]
    fn test_device_type_labels() {
        assert_eq!(BluetoothDeviceType::Headphones.label(), "Headphones");
        assert_eq!(BluetoothDeviceType::Mouse.label(), "Mouse");
    }

    #[test]
    fn test_device_type_icons() {
        assert_eq!(BluetoothDeviceType::Headphones.icon_char(), 'H');
        assert_eq!(BluetoothDeviceType::Keyboard.icon_char(), 'K');
    }

    // --- BluetoothProfile ---
    #[test]
    fn test_profile_labels() {
        assert!(BluetoothProfile::A2dp.label().contains("Audio"));
        assert!(BluetoothProfile::Hid.label().contains("Input"));
    }

    // --- ConnectionState ---
    #[test]
    fn test_connection_state_labels() {
        assert_eq!(ConnectionState::Connected.label(), "Connected");
        assert_eq!(ConnectionState::Disconnected.label(), "Not paired");
    }

    #[test]
    fn test_connection_state_is_connected() {
        assert!(ConnectionState::Connected.is_connected());
        assert!(!ConnectionState::Paired.is_connected());
        assert!(!ConnectionState::Disconnected.is_connected());
    }

    // --- BluetoothDevice ---
    #[test]
    fn test_signal_bars() {
        let mut d = sample_device("AA:BB", "Test");
        d.rssi = Some(-45);
        assert_eq!(d.signal_bars(), 4);
        d.rssi = Some(-65);
        assert_eq!(d.signal_bars(), 2);
        d.rssi = Some(-85);
        assert_eq!(d.signal_bars(), 0);
        d.rssi = None;
        assert_eq!(d.signal_bars(), 0);
    }

    #[test]
    fn test_battery_display() {
        let mut d = sample_device("AA:BB", "Test");
        assert_eq!(d.battery_display(), "80%");
        d.battery = None;
        assert_eq!(d.battery_display(), "N/A");
    }

    #[test]
    fn test_is_audio() {
        let d = sample_device("AA:BB", "Headphones");
        assert!(d.is_audio());
    }

    #[test]
    fn test_is_input() {
        let mut d = sample_device("AA:BB", "KB");
        d.profiles = vec![BluetoothProfile::Hid];
        assert!(d.is_input());
        assert!(!d.is_audio());
    }

    // --- BluetoothAdapter ---
    #[test]
    fn test_default_adapter() {
        let a = BluetoothAdapter::default_adapter();
        assert!(a.powered);
        assert!(!a.discoverable);
    }

    // --- FileTransfer ---
    #[test]
    fn test_transfer_progress() {
        let t = FileTransfer {
            id: 1,
            device_address: "AA:BB".to_string(),
            filename: "test.jpg".to_string(),
            total_bytes: 1000,
            transferred_bytes: 500,
            sending: true,
            completed: false,
            failed: false,
        };
        assert_eq!(t.progress_pct(), 50);
    }

    #[test]
    fn test_transfer_progress_zero() {
        let t = FileTransfer {
            id: 1,
            device_address: "AA:BB".to_string(),
            filename: "x".to_string(),
            total_bytes: 0,
            transferred_bytes: 0,
            sending: true,
            completed: false,
            failed: false,
        };
        assert_eq!(t.progress_pct(), 0);
    }

    // --- BluetoothManager ---
    #[test]
    fn test_manager_new() {
        let mgr = BluetoothManager::new();
        assert!(mgr.adapter.powered);
        assert!(mgr.devices.is_empty());
    }

    #[test]
    fn test_power_off() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA:BB", "Test"));
        mgr.pair("AA:BB");
        mgr.complete_pairing("AA:BB", true);
        assert!(mgr.connected_devices().len() == 1);

        mgr.set_powered(false);
        assert!(!mgr.adapter.powered);
        assert!(mgr.connected_devices().is_empty());
    }

    #[test]
    fn test_discovery() {
        let mut mgr = BluetoothManager::new();
        assert!(mgr.start_discovery());
        assert!(mgr.adapter.discovering);
        mgr.stop_discovery();
        assert!(!mgr.adapter.discovering);
    }

    #[test]
    fn test_discovery_when_off() {
        let mut mgr = BluetoothManager::new();
        mgr.set_powered(false);
        assert!(!mgr.start_discovery());
    }

    #[test]
    fn test_discoverable() {
        let mut mgr = BluetoothManager::new();
        assert!(mgr.set_discoverable(true));
        assert!(mgr.adapter.discoverable);
    }

    #[test]
    fn test_add_device() {
        let mut mgr = BluetoothManager::new();
        assert!(mgr.add_discovered_device(sample_device("AA:BB:CC:DD:EE:FF", "WH-1000")));
        assert_eq!(mgr.devices.len(), 1);
    }

    #[test]
    fn test_add_duplicate_updates() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA:BB", "Test"));
        let mut d2 = sample_device("AA:BB", "Updated Name");
        d2.rssi = Some(-30);
        assert!(!mgr.add_discovered_device(d2)); // Not new
        assert_eq!(mgr.devices.len(), 1);
        assert_eq!(mgr.devices[0].rssi, Some(-30)); // Updated
    }

    #[test]
    fn test_pair_and_connect() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA:BB", "Test"));
        assert!(mgr.pair("AA:BB"));
        assert_eq!(mgr.devices[0].state, ConnectionState::Connecting);
        mgr.complete_pairing("AA:BB", true);
        assert_eq!(mgr.devices[0].state, ConnectionState::Connected);
    }

    #[test]
    fn test_pair_fails() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA:BB", "Test"));
        mgr.pair("AA:BB");
        mgr.complete_pairing("AA:BB", false);
        assert_eq!(mgr.devices[0].state, ConnectionState::Disconnected);
    }

    #[test]
    fn test_disconnect() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA:BB", "Test"));
        mgr.pair("AA:BB");
        mgr.complete_pairing("AA:BB", true);
        assert!(mgr.disconnect("AA:BB"));
        assert_eq!(mgr.devices[0].state, ConnectionState::PairedNotConnected);
    }

    #[test]
    fn test_reconnect() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA:BB", "Test"));
        mgr.pair("AA:BB");
        mgr.complete_pairing("AA:BB", true);
        mgr.disconnect("AA:BB");
        assert!(mgr.connect("AA:BB"));
        mgr.complete_connect("AA:BB", true);
        assert_eq!(mgr.devices[0].state, ConnectionState::Connected);
    }

    #[test]
    fn test_remove_device() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA:BB", "Test"));
        assert!(mgr.remove_device("AA:BB"));
        assert!(mgr.devices.is_empty());
    }

    #[test]
    fn test_auto_connect() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA:BB", "Test"));
        assert!(mgr.set_auto_connect("AA:BB", true));
        assert!(mgr.devices[0].auto_connect);
    }

    #[test]
    fn test_connected_devices() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA", "A"));
        mgr.add_discovered_device(sample_device("BB", "B"));
        mgr.pair("AA");
        mgr.complete_pairing("AA", true);
        assert_eq!(mgr.connected_devices().len(), 1);
    }

    #[test]
    fn test_nearby_devices() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA", "A"));
        assert_eq!(mgr.nearby_devices().len(), 1); // Disconnected = nearby
        mgr.pair("AA");
        mgr.complete_pairing("AA", true);
        assert_eq!(mgr.nearby_devices().len(), 0);
    }

    #[test]
    fn test_file_transfer() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA", "A"));
        mgr.pair("AA");
        mgr.complete_pairing("AA", true);
        let tid = mgr.send_file("AA", "photo.jpg", 1000).unwrap();
        assert_eq!(mgr.transfers.len(), 1);
        mgr.advance_transfer(tid, 500);
        assert_eq!(mgr.transfers[0].progress_pct(), 50);
        mgr.advance_transfer(tid, 600); // Total > 1000
        assert!(mgr.transfers[0].completed);
    }

    #[test]
    fn test_send_file_not_connected() {
        let mut mgr = BluetoothManager::new();
        assert!(mgr.send_file("AA", "test.jpg", 100).is_none());
    }

    #[test]
    fn test_fail_transfer() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA", "A"));
        mgr.pair("AA");
        mgr.complete_pairing("AA", true);
        let tid = mgr.send_file("AA", "x.jpg", 100).unwrap();
        mgr.fail_transfer(tid);
        assert!(mgr.transfers[0].failed);
    }

    #[test]
    fn test_audio_device_count() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA", "Headphones"));
        mgr.pair("AA");
        mgr.complete_pairing("AA", true);
        assert_eq!(mgr.audio_device_count(), 1);
    }

    // --- UI ---
    #[test]
    fn test_ui_render_powered_on() {
        let mgr = BluetoothManager::new();
        let ui = BluetoothSettingsUI::new();
        let cmds = ui.render(&mgr, 0.0, 0.0, 400.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_powered_off() {
        let mut mgr = BluetoothManager::new();
        mgr.set_powered(false);
        let ui = BluetoothSettingsUI::new();
        let cmds = ui.render(&mgr, 0.0, 0.0, 400.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_with_devices() {
        let mut mgr = BluetoothManager::new();
        mgr.add_discovered_device(sample_device("AA", "AirPods"));
        mgr.pair("AA");
        mgr.complete_pairing("AA", true);
        let ui = BluetoothSettingsUI::new();
        let cmds = ui.render(&mgr, 0.0, 0.0, 400.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_default_trait_impls() {
        let _ = BluetoothManager::default();
        let _ = BluetoothSettingsUI::default();
    }
}

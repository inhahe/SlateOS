//! Device management settings panel for the desktop shell.
//!
//! Provides an overview of connected devices (USB, Bluetooth, audio,
//! display, input) with driver status, safely-remove functionality,
//! auto-mount preferences, and power management per device.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Device types and status
// ============================================================================

/// Category of device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceCategory {
    Usb,
    Bluetooth,
    Audio,
    Display,
    Input,
    Storage,
    Network,
    Printer,
    Camera,
    Other,
}

impl DeviceCategory {
    /// All categories.
    pub fn all() -> &'static [Self] {
        &[
            Self::Usb,
            Self::Bluetooth,
            Self::Audio,
            Self::Display,
            Self::Input,
            Self::Storage,
            Self::Network,
            Self::Printer,
            Self::Camera,
            Self::Other,
        ]
    }

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Bluetooth => "Bluetooth",
            Self::Audio => "Audio",
            Self::Display => "Display",
            Self::Input => "Input",
            Self::Storage => "Storage",
            Self::Network => "Network",
            Self::Printer => "Printers",
            Self::Camera => "Cameras",
            Self::Other => "Other",
        }
    }

    /// Icon character.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Usb => "\u{1F50C}",      // plug
            Self::Bluetooth => "\u{1F4F6}", // signal
            Self::Audio => "\u{1F3A7}",     // headphones
            Self::Display => "\u{1F5B5}",   // display
            Self::Input => "\u{2328}",      // keyboard
            Self::Storage => "\u{1F4BE}",   // floppy
            Self::Network => "\u{1F310}",   // globe
            Self::Printer => "\u{1F5A8}",   // printer
            Self::Camera => "\u{1F4F7}",    // camera
            Self::Other => "\u{2699}",      // gear
        }
    }
}

/// Current connection/status of a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceStatus {
    Connected,
    Disconnected,
    Pairing,
    Error,
    Sleeping,
    Disabled,
}

impl DeviceStatus {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
            Self::Pairing => "Pairing...",
            Self::Error => "Error",
            Self::Sleeping => "Sleeping",
            Self::Disabled => "Disabled",
        }
    }

    /// Status color.
    pub fn color(self) -> Color {
        match self {
            Self::Connected => GREEN,
            Self::Disconnected => OVERLAY0,
            Self::Pairing => YELLOW,
            Self::Error => RED,
            Self::Sleeping => LAVENDER,
            Self::Disabled => SURFACE2,
        }
    }
}

/// Driver status for a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverStatus {
    Loaded,
    NotFound,
    Error,
    Updating,
    Disabled,
}

impl DriverStatus {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Loaded => "Driver loaded",
            Self::NotFound => "No driver",
            Self::Error => "Driver error",
            Self::Updating => "Updating...",
            Self::Disabled => "Disabled",
        }
    }

    /// Status color.
    pub fn color(self) -> Color {
        match self {
            Self::Loaded => GREEN,
            Self::NotFound => YELLOW,
            Self::Error => RED,
            Self::Updating => BLUE,
            Self::Disabled => OVERLAY0,
        }
    }
}

// ============================================================================
// Device info
// ============================================================================

/// Information about a connected/known device.
#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub category: DeviceCategory,
    pub status: DeviceStatus,
    pub driver: DriverStatus,
    pub driver_name: Option<String>,
    pub driver_version: Option<String>,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub serial_number: Option<String>,
    pub bus_path: String,
    pub power_state: DevicePowerState,
    pub removable: bool,
    pub auto_mount: bool,
    pub connected_since: Option<u64>,
}

impl DeviceInfo {
    /// Format vendor:product ID.
    pub fn id_string(&self) -> String {
        match (self.vendor_id, self.product_id) {
            (Some(v), Some(p)) => format!("{v:04x}:{p:04x}"),
            _ => "unknown".to_string(),
        }
    }

    /// Check if this device has a working driver.
    pub fn has_driver(&self) -> bool {
        self.driver == DriverStatus::Loaded
    }

    /// Check if this device can be safely removed.
    pub fn can_safely_remove(&self) -> bool {
        self.removable && self.status == DeviceStatus::Connected
    }

    /// Connection duration in seconds (if connected).
    pub fn connected_duration(&self, now: u64) -> Option<u64> {
        self.connected_since.map(|since| now.saturating_sub(since))
    }

    /// Format connection duration for display.
    pub fn uptime_display(&self, now: u64) -> String {
        match self.connected_duration(now) {
            Some(secs) if secs < 60 => format!("{secs}s"),
            Some(secs) if secs < 3600 => format!("{}m", secs / 60),
            Some(secs) if secs < 86400 => format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
            Some(secs) => format!("{}d {}h", secs / 86400, (secs % 86400) / 3600),
            None => "—".to_string(),
        }
    }
}

/// Power state of a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePowerState {
    On,
    Standby,
    Suspended,
    Off,
}

impl DevicePowerState {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::On => "Active",
            Self::Standby => "Standby",
            Self::Suspended => "Suspended",
            Self::Off => "Off",
        }
    }
}

// ============================================================================
// Device preferences
// ============================================================================

/// Per-device user preferences.
#[derive(Clone, Debug)]
pub struct DevicePrefs {
    pub device_id: String,
    pub custom_name: Option<String>,
    pub auto_mount: bool,
    pub allow_wake: bool,
    pub power_management: bool,
    pub trusted: bool,
}

impl Default for DevicePrefs {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            custom_name: None,
            auto_mount: true,
            allow_wake: false,
            power_management: true,
            trusted: false,
        }
    }
}

// ============================================================================
// Device manager
// ============================================================================

/// Manages all known devices and their preferences.
#[derive(Clone, Debug)]
pub struct DeviceManager {
    pub devices: Vec<DeviceInfo>,
    pub preferences: Vec<DevicePrefs>,
    pub show_disconnected: bool,
    pub auto_install_drivers: bool,
    pub safely_remove_notifications: bool,
    pub usb_power_saving: bool,
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            preferences: Vec::new(),
            show_disconnected: false,
            auto_install_drivers: true,
            safely_remove_notifications: true,
            usb_power_saving: true,
        }
    }
}

impl DeviceManager {
    /// Add or update a device.
    pub fn register_device(&mut self, device: DeviceInfo) {
        if let Some(existing) = self.devices.iter_mut().find(|d| d.id == device.id) {
            *existing = device;
        } else {
            self.devices.push(device);
        }
    }

    /// Remove a device.
    pub fn unregister_device(&mut self, id: &str) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.id != id);
        self.devices.len() < before
    }

    /// Get a device by ID.
    pub fn device(&self, id: &str) -> Option<&DeviceInfo> {
        self.devices.iter().find(|d| d.id == id)
    }

    /// Get a mutable device by ID.
    pub fn device_mut(&mut self, id: &str) -> Option<&mut DeviceInfo> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    /// Get devices filtered by category.
    pub fn devices_by_category(&self, category: DeviceCategory) -> Vec<&DeviceInfo> {
        self.devices
            .iter()
            .filter(|d| d.category == category)
            .filter(|d| self.show_disconnected || d.status != DeviceStatus::Disconnected)
            .collect()
    }

    /// Count connected devices.
    pub fn connected_count(&self) -> usize {
        self.devices
            .iter()
            .filter(|d| d.status == DeviceStatus::Connected)
            .count()
    }

    /// Count devices with driver problems.
    pub fn problem_count(&self) -> usize {
        self.devices
            .iter()
            .filter(|d| {
                d.status == DeviceStatus::Error
                    || d.driver == DriverStatus::Error
                    || d.driver == DriverStatus::NotFound
            })
            .count()
    }

    /// Get all removable devices that can be safely removed.
    pub fn safely_removable(&self) -> Vec<&DeviceInfo> {
        self.devices
            .iter()
            .filter(|d| d.can_safely_remove())
            .collect()
    }

    /// Count devices per category (only connected/active).
    pub fn category_counts(&self) -> Vec<(DeviceCategory, usize)> {
        DeviceCategory::all()
            .iter()
            .map(|cat| {
                let count = self.devices_by_category(*cat).len();
                (*cat, count)
            })
            .filter(|(_, count)| *count > 0)
            .collect()
    }

    /// Get or create preferences for a device.
    pub fn get_prefs(&self, device_id: &str) -> Option<&DevicePrefs> {
        self.preferences.iter().find(|p| p.device_id == device_id)
    }

    /// Set preferences for a device.
    pub fn set_prefs(&mut self, prefs: DevicePrefs) {
        if let Some(existing) = self
            .preferences
            .iter_mut()
            .find(|p| p.device_id == prefs.device_id)
        {
            *existing = prefs;
        } else {
            self.preferences.push(prefs);
        }
    }

    /// Search devices by name or manufacturer.
    pub fn search(&self, query: &str) -> Vec<&DeviceInfo> {
        let q = query.to_lowercase();
        self.devices
            .iter()
            .filter(|d| {
                d.name.to_lowercase().contains(&q)
                    || d.manufacturer.to_lowercase().contains(&q)
                    || d.id.to_lowercase().contains(&q)
            })
            .collect()
    }
}

// ============================================================================
// Settings UI
// ============================================================================

/// Tabs in the device settings panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceSettingsTab {
    Overview,
    ByCategory,
    Drivers,
    SafeRemove,
}

impl DeviceSettingsTab {
    /// All tabs.
    pub fn all() -> &'static [Self] {
        &[Self::Overview, Self::ByCategory, Self::Drivers, Self::SafeRemove]
    }

    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::ByCategory => "Devices",
            Self::Drivers => "Drivers",
            Self::SafeRemove => "Safe Remove",
        }
    }
}

/// Device settings UI state.
pub struct DeviceSettingsUI {
    pub manager: DeviceManager,
    pub active_tab: DeviceSettingsTab,
    pub search_query: String,
    pub selected_device: Option<String>,
    pub expanded_category: Option<DeviceCategory>,
    pub scroll_offset: f32,
}

impl DeviceSettingsUI {
    /// Create with default state and sample devices.
    pub fn new() -> Self {
        let mut manager = DeviceManager::default();

        // Add some default/example devices
        manager.register_device(DeviceInfo {
            id: "usb-keyboard-1".to_string(),
            name: "USB Keyboard".to_string(),
            manufacturer: "Generic".to_string(),
            category: DeviceCategory::Input,
            status: DeviceStatus::Connected,
            driver: DriverStatus::Loaded,
            driver_name: Some("usbhid".to_string()),
            driver_version: Some("1.0.0".to_string()),
            vendor_id: Some(0x046d),
            product_id: Some(0xc52b),
            serial_number: None,
            bus_path: "/dev/usb/0/1".to_string(),
            power_state: DevicePowerState::On,
            removable: false,
            auto_mount: false,
            connected_since: Some(0),
        });

        manager.register_device(DeviceInfo {
            id: "usb-mouse-1".to_string(),
            name: "USB Mouse".to_string(),
            manufacturer: "Generic".to_string(),
            category: DeviceCategory::Input,
            status: DeviceStatus::Connected,
            driver: DriverStatus::Loaded,
            driver_name: Some("usbhid".to_string()),
            driver_version: Some("1.0.0".to_string()),
            vendor_id: Some(0x046d),
            product_id: Some(0xc077),
            serial_number: None,
            bus_path: "/dev/usb/0/2".to_string(),
            power_state: DevicePowerState::On,
            removable: false,
            auto_mount: false,
            connected_since: Some(0),
        });

        manager.register_device(DeviceInfo {
            id: "audio-speakers-1".to_string(),
            name: "Built-in Speakers".to_string(),
            manufacturer: "Intel HD Audio".to_string(),
            category: DeviceCategory::Audio,
            status: DeviceStatus::Connected,
            driver: DriverStatus::Loaded,
            driver_name: Some("snd_hda_intel".to_string()),
            driver_version: Some("2.1.0".to_string()),
            vendor_id: Some(0x8086),
            product_id: Some(0xa170),
            serial_number: None,
            bus_path: "/dev/audio/0".to_string(),
            power_state: DevicePowerState::On,
            removable: false,
            auto_mount: false,
            connected_since: Some(0),
        });

        manager.register_device(DeviceInfo {
            id: "display-hdmi-1".to_string(),
            name: "HDMI Display".to_string(),
            manufacturer: "Dell".to_string(),
            category: DeviceCategory::Display,
            status: DeviceStatus::Connected,
            driver: DriverStatus::Loaded,
            driver_name: Some("i915".to_string()),
            driver_version: Some("3.0.0".to_string()),
            vendor_id: Some(0x8086),
            product_id: Some(0x5917),
            serial_number: Some("DELL-12345".to_string()),
            bus_path: "/dev/gpu/0/hdmi-1".to_string(),
            power_state: DevicePowerState::On,
            removable: false,
            auto_mount: false,
            connected_since: Some(0),
        });

        Self {
            manager,
            active_tab: DeviceSettingsTab::Overview,
            search_query: String::new(),
            selected_device: None,
            expanded_category: None,
            scroll_offset: 0.0,
        }
    }

    /// Switch tab.
    pub fn set_tab(&mut self, tab: DeviceSettingsTab) {
        self.active_tab = tab;
        self.scroll_offset = 0.0;
    }

    /// Render the panel.
    pub fn render(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 20.0,
            text: "Devices".to_string(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Tab bar
        let tab_y = y + 56.0;
        let mut tab_x = x + 16.0;
        for tab in DeviceSettingsTab::all() {
            let label = tab.label();
            let tw = label.len() as f32 * 8.0 + 24.0;
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: tw,
                    height: 32.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: tab_y + 8.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active { BLUE } else { SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            tab_x += tw + 4.0;
        }

        // Content
        let content_y = tab_y + 44.0;
        let content_h = height - (content_y - y) - 16.0;

        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: content_y,
            width: width - 16.0,
            height: content_h,
            color: CRUST,
            corner_radii: CornerRadii::all(6.0),
        });

        let cx = x + 24.0;
        let cy = content_y + 16.0;
        let cw = width - 48.0;

        match self.active_tab {
            DeviceSettingsTab::Overview => self.render_overview(&mut cmds, cx, cy, cw),
            DeviceSettingsTab::ByCategory => self.render_by_category(&mut cmds, cx, cy, cw),
            DeviceSettingsTab::Drivers => self.render_drivers(&mut cmds, cx, cy, cw),
            DeviceSettingsTab::SafeRemove => self.render_safe_remove(&mut cmds, cx, cy, cw),
        }

        cmds
    }

    /// Render overview tab.
    fn render_overview(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        // Summary cards
        let stats = [
            ("Connected", format!("{}", self.manager.connected_count()), GREEN),
            ("Total", format!("{}", self.manager.devices.len()), BLUE),
            ("Problems", format!("{}", self.manager.problem_count()), if self.manager.problem_count() > 0 { RED } else { OVERLAY0 }),
            ("Removable", format!("{}", self.manager.safely_removable().len()), LAVENDER),
        ];

        let card_w = (width - 24.0) / 4.0;
        for (i, (label, value, color)) in stats.iter().enumerate() {
            let cx = x + i as f32 * (card_w + 8.0);

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: row_y,
                width: card_w,
                height: 56.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: row_y + 8.0,
                text: label.to_string(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: row_y + 26.0,
                text: value.clone(),
                font_size: 18.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        row_y += 72.0;

        // Category breakdown
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Device categories".to_string(),
            font_size: 14.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 24.0;

        for (category, count) in self.manager.category_counts() {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 36.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 8.0,
                text: category.icon().to_string(),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: x + 36.0,
                y: row_y + 10.0,
                text: category.label().to_string(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: x + width - 40.0,
                y: row_y + 10.0,
                text: format!("{count}"),
                font_size: 13.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            row_y += 42.0;
        }

        // Settings
        row_y += 8.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Settings".to_string(),
            font_size: 14.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 24.0;

        let settings = [
            ("Show disconnected devices", self.manager.show_disconnected),
            ("Auto-install drivers", self.manager.auto_install_drivers),
            ("Safe-remove notifications", self.manager.safely_remove_notifications),
            ("USB power saving", self.manager.usb_power_saving),
        ];

        for (label, enabled) in &settings {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 32.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: label.to_string(),
                font_size: 12.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            let toggle_bg = if *enabled { BLUE } else { SURFACE2 };
            cmds.push(RenderCommand::FillRect {
                x: x + width - 56.0,
                y: row_y + 6.0,
                width: 40.0,
                height: 20.0,
                color: toggle_bg,
                corner_radii: CornerRadii::all(10.0),
            });

            row_y += 38.0;
        }
    }

    /// Render devices by category tab.
    fn render_by_category(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        // Search bar
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 32.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        let search_text = if self.search_query.is_empty() {
            "Search devices...".to_string()
        } else {
            self.search_query.clone()
        };

        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: row_y + 8.0,
            text: search_text,
            font_size: 12.0,
            color: if self.search_query.is_empty() { OVERLAY0 } else { TEXT },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });
        row_y += 44.0;

        let devices: Vec<&DeviceInfo> = if self.search_query.is_empty() {
            self.manager
                .devices
                .iter()
                .filter(|d| self.manager.show_disconnected || d.status != DeviceStatus::Disconnected)
                .collect()
        } else {
            self.manager.search(&self.search_query)
        };

        // Group by category
        for category in DeviceCategory::all() {
            let cat_devices: Vec<&&DeviceInfo> = devices
                .iter()
                .filter(|d| d.category == *category)
                .collect();

            if cat_devices.is_empty() {
                continue;
            }

            let is_expanded = self.expanded_category == Some(*category) || !self.search_query.is_empty();

            // Category header
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 6.0,
                text: format!(
                    "{} {} ({})",
                    if is_expanded { "\u{25BC}" } else { "\u{25B6}" },
                    category.label(),
                    cat_devices.len()
                ),
                font_size: 12.0,
                color: SUBTEXT1,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            row_y += 34.0;

            if is_expanded {
                for device in &cat_devices {
                    let is_selected = self
                        .selected_device
                        .as_ref()
                        .map_or(false, |id| id == &device.id);

                    cmds.push(RenderCommand::FillRect {
                        x: x + 8.0,
                        y: row_y,
                        width: width - 16.0,
                        height: 48.0,
                        color: if is_selected { SURFACE1 } else { SURFACE0 },
                        corner_radii: CornerRadii::all(4.0),
                    });

                    // Status dot
                    cmds.push(RenderCommand::FillRect {
                        x: x + 20.0,
                        y: row_y + 14.0,
                        width: 8.0,
                        height: 8.0,
                        color: device.status.color(),
                        corner_radii: CornerRadii::all(4.0),
                    });

                    // Device name
                    cmds.push(RenderCommand::Text {
                        x: x + 36.0,
                        y: row_y + 6.0,
                        text: device.name.clone(),
                        font_size: 13.0,
                        color: TEXT,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(width - 140.0),
                    });

                    // Manufacturer and status
                    cmds.push(RenderCommand::Text {
                        x: x + 36.0,
                        y: row_y + 24.0,
                        text: format!(
                            "{} — {} — {}",
                            device.manufacturer,
                            device.status.label(),
                            device.id_string()
                        ),
                        font_size: 10.0,
                        color: SUBTEXT0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width - 80.0),
                    });

                    // Driver badge
                    cmds.push(RenderCommand::FillRect {
                        x: x + width - 84.0,
                        y: row_y + 8.0,
                        width: 60.0,
                        height: 16.0,
                        color: device.driver.color(),
                        corner_radii: CornerRadii::all(3.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + width - 80.0,
                        y: row_y + 10.0,
                        text: if device.has_driver() { "OK" } else { "No drv" }.to_string(),
                        font_size: 9.0,
                        color: CRUST,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });

                    row_y += 54.0;
                }
            }
        }
    }

    /// Render drivers tab.
    fn render_drivers(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Device drivers".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 28.0;

        // Problem devices first
        let problems: Vec<&DeviceInfo> = self
            .manager
            .devices
            .iter()
            .filter(|d| d.driver == DriverStatus::Error || d.driver == DriverStatus::NotFound)
            .collect();

        if !problems.is_empty() {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 32.0,
                color: Color::rgba(243, 139, 168, 30),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 8.0,
                text: format!("{} device(s) with driver issues", problems.len()),
                font_size: 13.0,
                color: RED,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            row_y += 40.0;
        }

        // All devices with driver info
        for device in &self.manager.devices {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 52.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 6.0,
                text: device.name.clone(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 100.0),
            });

            let driver_info = match (&device.driver_name, &device.driver_version) {
                (Some(name), Some(ver)) => format!("{name} v{ver}"),
                (Some(name), None) => name.clone(),
                _ => "No driver".to_string(),
            };

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 24.0,
                text: driver_info,
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Status badge
            cmds.push(RenderCommand::FillRect {
                x: x + width - 80.0,
                y: row_y + 10.0,
                width: 64.0,
                height: 18.0,
                color: device.driver.color(),
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + width - 74.0,
                y: row_y + 12.0,
                text: device.driver.label().to_string(),
                font_size: 9.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            row_y += 58.0;
        }
    }

    /// Render safe remove tab.
    fn render_safe_remove(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Safely remove devices".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 28.0;

        let removable = self.manager.safely_removable();

        if removable.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: "No removable devices connected.".to_string(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        } else {
            for device in &removable {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 56.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 8.0,
                    text: device.name.clone(),
                    font_size: 14.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 28.0,
                    text: format!("{} — {}", device.manufacturer, device.bus_path),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 120.0),
                });

                // Eject button
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 72.0,
                    y: row_y + 14.0,
                    width: 56.0,
                    height: 28.0,
                    color: PEACH,
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 62.0,
                    y: row_y + 20.0,
                    text: "Eject".to_string(),
                    font_size: 12.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                row_y += 62.0;
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_device(id: &str, category: DeviceCategory) -> DeviceInfo {
        DeviceInfo {
            id: id.to_string(),
            name: format!("Device {id}"),
            manufacturer: "TestCorp".to_string(),
            category,
            status: DeviceStatus::Connected,
            driver: DriverStatus::Loaded,
            driver_name: Some("testdrv".to_string()),
            driver_version: Some("1.0".to_string()),
            vendor_id: Some(0x1234),
            product_id: Some(0x5678),
            serial_number: None,
            bus_path: format!("/dev/test/{id}"),
            power_state: DevicePowerState::On,
            removable: false,
            auto_mount: false,
            connected_since: Some(1000),
        }
    }

    #[test]
    fn test_category_all() {
        assert_eq!(DeviceCategory::all().len(), 10);
    }

    #[test]
    fn test_device_id_string() {
        let dev = sample_device("test", DeviceCategory::Usb);
        assert_eq!(dev.id_string(), "1234:5678");

        let dev_no_ids = DeviceInfo {
            vendor_id: None,
            product_id: None,
            ..sample_device("test2", DeviceCategory::Usb)
        };
        assert_eq!(dev_no_ids.id_string(), "unknown");
    }

    #[test]
    fn test_device_has_driver() {
        let dev = sample_device("test", DeviceCategory::Usb);
        assert!(dev.has_driver());

        let dev_no_drv = DeviceInfo {
            driver: DriverStatus::NotFound,
            ..sample_device("test2", DeviceCategory::Usb)
        };
        assert!(!dev_no_drv.has_driver());
    }

    #[test]
    fn test_device_safely_removable() {
        let dev = DeviceInfo {
            removable: true,
            ..sample_device("usb-drive", DeviceCategory::Storage)
        };
        assert!(dev.can_safely_remove());

        let dev_not_removable = sample_device("internal", DeviceCategory::Storage);
        assert!(!dev_not_removable.can_safely_remove());

        let dev_disconnected = DeviceInfo {
            removable: true,
            status: DeviceStatus::Disconnected,
            ..sample_device("disconnected", DeviceCategory::Storage)
        };
        assert!(!dev_disconnected.can_safely_remove());
    }

    #[test]
    fn test_uptime_display() {
        let dev = DeviceInfo {
            connected_since: Some(1000),
            ..sample_device("test", DeviceCategory::Usb)
        };
        assert_eq!(dev.uptime_display(1030), "30s");
        assert_eq!(dev.uptime_display(4600), "1h 0m");
        assert_eq!(dev.uptime_display(90400), "1d 0h");

        let no_conn = DeviceInfo {
            connected_since: None,
            ..sample_device("test2", DeviceCategory::Usb)
        };
        assert_eq!(no_conn.uptime_display(5000), "—");
    }

    #[test]
    fn test_manager_register_device() {
        let mut mgr = DeviceManager::default();
        let dev = sample_device("d1", DeviceCategory::Usb);
        mgr.register_device(dev);
        assert_eq!(mgr.devices.len(), 1);

        // Update existing
        let dev2 = DeviceInfo {
            name: "Updated".to_string(),
            ..sample_device("d1", DeviceCategory::Usb)
        };
        mgr.register_device(dev2);
        assert_eq!(mgr.devices.len(), 1);
        assert_eq!(mgr.devices[0].name, "Updated");
    }

    #[test]
    fn test_manager_unregister() {
        let mut mgr = DeviceManager::default();
        mgr.register_device(sample_device("d1", DeviceCategory::Usb));
        assert!(mgr.unregister_device("d1"));
        assert!(mgr.devices.is_empty());
        assert!(!mgr.unregister_device("d1"));
    }

    #[test]
    fn test_manager_device_lookup() {
        let mut mgr = DeviceManager::default();
        mgr.register_device(sample_device("d1", DeviceCategory::Audio));
        assert!(mgr.device("d1").is_some());
        assert!(mgr.device("nonexistent").is_none());
    }

    #[test]
    fn test_manager_by_category() {
        let mut mgr = DeviceManager::default();
        mgr.register_device(sample_device("u1", DeviceCategory::Usb));
        mgr.register_device(sample_device("u2", DeviceCategory::Usb));
        mgr.register_device(sample_device("a1", DeviceCategory::Audio));

        assert_eq!(mgr.devices_by_category(DeviceCategory::Usb).len(), 2);
        assert_eq!(mgr.devices_by_category(DeviceCategory::Audio).len(), 1);
        assert_eq!(mgr.devices_by_category(DeviceCategory::Display).len(), 0);
    }

    #[test]
    fn test_manager_connected_count() {
        let mut mgr = DeviceManager::default();
        mgr.register_device(sample_device("d1", DeviceCategory::Usb));
        mgr.register_device(DeviceInfo {
            status: DeviceStatus::Disconnected,
            ..sample_device("d2", DeviceCategory::Usb)
        });
        assert_eq!(mgr.connected_count(), 1);
    }

    #[test]
    fn test_manager_problem_count() {
        let mut mgr = DeviceManager::default();
        mgr.register_device(sample_device("ok", DeviceCategory::Usb));
        mgr.register_device(DeviceInfo {
            driver: DriverStatus::Error,
            ..sample_device("err", DeviceCategory::Usb)
        });
        mgr.register_device(DeviceInfo {
            driver: DriverStatus::NotFound,
            ..sample_device("nodrv", DeviceCategory::Usb)
        });
        assert_eq!(mgr.problem_count(), 2);
    }

    #[test]
    fn test_manager_safely_removable() {
        let mut mgr = DeviceManager::default();
        mgr.register_device(DeviceInfo {
            removable: true,
            ..sample_device("usb1", DeviceCategory::Storage)
        });
        mgr.register_device(sample_device("internal", DeviceCategory::Storage));
        assert_eq!(mgr.safely_removable().len(), 1);
    }

    #[test]
    fn test_manager_category_counts() {
        let mut mgr = DeviceManager::default();
        mgr.register_device(sample_device("u1", DeviceCategory::Usb));
        mgr.register_device(sample_device("u2", DeviceCategory::Usb));
        mgr.register_device(sample_device("a1", DeviceCategory::Audio));

        let counts = mgr.category_counts();
        assert!(counts.iter().any(|(c, n)| *c == DeviceCategory::Usb && *n == 2));
        assert!(counts.iter().any(|(c, n)| *c == DeviceCategory::Audio && *n == 1));
    }

    #[test]
    fn test_manager_search() {
        let mut mgr = DeviceManager::default();
        mgr.register_device(DeviceInfo {
            name: "USB Flash Drive".to_string(),
            manufacturer: "SanDisk".to_string(),
            ..sample_device("flash1", DeviceCategory::Storage)
        });
        mgr.register_device(DeviceInfo {
            name: "Wireless Mouse".to_string(),
            manufacturer: "Logitech".to_string(),
            ..sample_device("mouse1", DeviceCategory::Input)
        });

        assert_eq!(mgr.search("flash").len(), 1);
        assert_eq!(mgr.search("logitech").len(), 1);
        assert_eq!(mgr.search("nonexistent").len(), 0);
    }

    #[test]
    fn test_manager_preferences() {
        let mut mgr = DeviceManager::default();
        let prefs = DevicePrefs {
            device_id: "d1".to_string(),
            auto_mount: false,
            ..DevicePrefs::default()
        };
        mgr.set_prefs(prefs);
        let p = mgr.get_prefs("d1").unwrap();
        assert!(!p.auto_mount);

        // Update
        mgr.set_prefs(DevicePrefs {
            device_id: "d1".to_string(),
            auto_mount: true,
            ..DevicePrefs::default()
        });
        let p2 = mgr.get_prefs("d1").unwrap();
        assert!(p2.auto_mount);
    }

    #[test]
    fn test_device_status_colors() {
        let _c1 = DeviceStatus::Connected.color();
        let _c2 = DeviceStatus::Error.color();
    }

    #[test]
    fn test_driver_status_labels() {
        assert_eq!(DriverStatus::Loaded.label(), "Driver loaded");
        assert_eq!(DriverStatus::NotFound.label(), "No driver");
    }

    #[test]
    fn test_power_state_labels() {
        assert_eq!(DevicePowerState::On.label(), "Active");
        assert_eq!(DevicePowerState::Suspended.label(), "Suspended");
    }

    // UI tests
    #[test]
    fn test_ui_new() {
        let ui = DeviceSettingsUI::new();
        assert_eq!(ui.active_tab, DeviceSettingsTab::Overview);
        assert!(!ui.manager.devices.is_empty());
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = DeviceSettingsUI::new();
        ui.scroll_offset = 50.0;
        ui.set_tab(DeviceSettingsTab::Drivers);
        assert_eq!(ui.active_tab, DeviceSettingsTab::Drivers);
        assert_eq!(ui.scroll_offset, 0.0);
    }

    #[test]
    fn test_ui_render() {
        let ui = DeviceSettingsUI::new();
        let cmds = ui.render(0.0, 0.0, 600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_tab_all() {
        assert_eq!(DeviceSettingsTab::all().len(), 4);
    }

    #[test]
    fn test_disconnected_filter() {
        let mut mgr = DeviceManager::default();
        mgr.register_device(sample_device("c1", DeviceCategory::Usb));
        mgr.register_device(DeviceInfo {
            status: DeviceStatus::Disconnected,
            ..sample_device("d1", DeviceCategory::Usb)
        });

        assert_eq!(mgr.devices_by_category(DeviceCategory::Usb).len(), 1);
        mgr.show_disconnected = true;
        assert_eq!(mgr.devices_by_category(DeviceCategory::Usb).len(), 2);
    }
}

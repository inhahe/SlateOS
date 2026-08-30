//! Device management settings panel for the desktop shell.
//!
//! Provides an overview of connected devices (USB, Bluetooth, audio,
//! display, input) with driver status, safely-remove functionality,
//! auto-mount preferences, and power management per device.

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

// ============================================================================
// Colour
// ============================================================================
//
// The fifteen Catppuccin Mocha constants that used to be spelled out here are
// gone; the caller passes a resolved `Palette` and every colour below is a role
// on it. See known-issues.md
// TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE.
//
// **A device's status and its driver's status are categorical.** Connected,
// pairing, error, sleeping, disabled — and loaded, missing, updating — are
// *kinds*, and they are drawn as a row of dots and badges that are only legible
// because they differ from each other. So `DeviceStatus::color` and
// `DriverStatus::color` read the named hues (`p.green`, `p.yellow`, `p.red`,
// `p.lavender`, `p.blue`), which follow the mode but never the accent. The same
// goes for the four summary figures on the overview tab, which are one such row
// by another name: a Red desktop that painted "Connected", "Total", "Problems"
// and "Removable" all the same colour would have lost the only thing that
// distinguishes them.
//
// `DriverStatus::Updating` is worth naming explicitly: it is blue, and blue is
// also the default accent, so it looks like a candidate for `p.accent`. It is
// not. It is one of five fixed states in a badge, and following the accent
// would put it in the user's colour while its four siblings stayed put.
//
// **Two things do follow the accent**, because both mean "this is the one you
// are on / the one that is on": the active tab's label, and a settings toggle
// in its enabled position.
//
// Text drawn *on* a status badge is `readable_on(...)` of the badge rather than
// the fixed near-black it was, because the badge's colour is now a palette role
// and in Latte those hues are light enough that dark text is the legible
// choice.

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
            Self::Usb => "\u{1F50C}",       // plug
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
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::Connected => p.green,
            Self::Disconnected => p.overlay0,
            Self::Pairing => p.yellow,
            Self::Error => p.red,
            Self::Sleeping => p.lavender,
            Self::Disabled => p.surface2,
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
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::Loaded => p.green,
            Self::NotFound => p.yellow,
            Self::Error => p.red,
            Self::Updating => p.blue,
            Self::Disabled => p.overlay0,
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
    ///
    /// This function's shape is the one `guitk::duration::coarse_minutes` was
    /// derived from — it was already right, and is now shared.
    pub fn uptime_display(&self, now: u64) -> String {
        match self.connected_duration(now) {
            Some(secs) => guitk::duration::coarse_minutes(secs),
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
        &[
            Self::Overview,
            Self::ByCategory,
            Self::Drivers,
            Self::SafeRemove,
        ]
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
    pub fn render(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 20.0,
            text: "Devices".to_string(),
            font_size: 22.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Tab bar
        let tab_y = y + 56.0;
        let mut tab_x = x + 16.0;
        for tab in DeviceSettingsTab::all() {
            let label = tab.label();
            let tw = text::padded_width_any_weight(label, 12.0, 13.0);
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: tw,
                    height: 32.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: tab_y + 8.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active { p.accent } else { p.subtext0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
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
            color: p.crust,
            corner_radii: CornerRadii::all(6.0),
        });

        let cx = x + 24.0;
        let cy = content_y + 16.0;
        let cw = width - 48.0;

        match self.active_tab {
            DeviceSettingsTab::Overview => self.render_overview(p, &mut cmds, cx, cy, cw),
            DeviceSettingsTab::ByCategory => self.render_by_category(p, &mut cmds, cx, cy, cw),
            DeviceSettingsTab::Drivers => self.render_drivers(p, &mut cmds, cx, cy, cw),
            DeviceSettingsTab::SafeRemove => self.render_safe_remove(p, &mut cmds, cx, cy, cw),
        }

        cmds
    }

    /// Render overview tab.
    fn render_overview(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        // Summary cards
        let stats = [
            (
                "Connected",
                format!("{}", self.manager.connected_count()),
                p.green,
            ),
            ("Total", format!("{}", self.manager.devices.len()), p.blue),
            (
                "Problems",
                format!("{}", self.manager.problem_count()),
                if self.manager.problem_count() > 0 {
                    p.red
                } else {
                    p.overlay0
                },
            ),
            (
                "Removable",
                format!("{}", self.manager.safely_removable().len()),
                p.lavender,
            ),
        ];

        let card_w = (width - 24.0) / 4.0;
        for (i, (label, value, color)) in stats.iter().enumerate() {
            let cx = x + i as f32 * (card_w + 8.0);

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: row_y,
                width: card_w,
                height: 56.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: row_y + 8.0,
                text: label.to_string(),
                font_size: 10.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: row_y + 26.0,
                text: value.clone(),
                font_size: 18.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        row_y += 72.0;

        // Category breakdown
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Device categories".to_string(),
            font_size: 14.0,
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 24.0;

        for (category, count) in self.manager.category_counts() {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 36.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 8.0,
                text: category.icon().to_string(),
                font_size: 14.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: x + 36.0,
                y: row_y + 10.0,
                text: category.label().to_string(),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: x + width - 40.0,
                y: row_y + 10.0,
                text: format!("{count}"),
                font_size: 13.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
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
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 24.0;

        let settings = [
            ("Show disconnected devices", self.manager.show_disconnected),
            ("Auto-install drivers", self.manager.auto_install_drivers),
            (
                "Safe-remove notifications",
                self.manager.safely_remove_notifications,
            ),
            ("USB power saving", self.manager.usb_power_saving),
        ];

        for (label, enabled) in &settings {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 32.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: label.to_string(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            let toggle_bg = if *enabled { p.accent } else { p.surface2 };
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
    fn render_by_category(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        // Search bar
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 32.0,
            color: p.surface0,
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
            color: if self.search_query.is_empty() {
                p.overlay0
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 44.0;

        let devices: Vec<&DeviceInfo> = if self.search_query.is_empty() {
            self.manager
                .devices
                .iter()
                .filter(|d| {
                    self.manager.show_disconnected || d.status != DeviceStatus::Disconnected
                })
                .collect()
        } else {
            self.manager.search(&self.search_query)
        };

        // Group by category
        for category in DeviceCategory::all() {
            let cat_devices: Vec<&&DeviceInfo> =
                devices.iter().filter(|d| d.category == *category).collect();

            if cat_devices.is_empty() {
                continue;
            }

            let is_expanded =
                self.expanded_category == Some(*category) || !self.search_query.is_empty();

            // Category header
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 28.0,
                color: p.surface0,
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
                color: p.subtext1,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            row_y += 34.0;

            if is_expanded {
                for device in &cat_devices {
                    let is_selected = self.selected_device.as_ref() == Some(&device.id);

                    cmds.push(RenderCommand::FillRect {
                        x: x + 8.0,
                        y: row_y,
                        width: width - 16.0,
                        height: 48.0,
                        color: if is_selected { p.surface1 } else { p.surface0 },
                        corner_radii: CornerRadii::all(4.0),
                    });

                    // Status dot
                    cmds.push(RenderCommand::FillRect {
                        x: x + 20.0,
                        y: row_y + 14.0,
                        width: 8.0,
                        height: 8.0,
                        color: device.status.color(p),
                        corner_radii: CornerRadii::all(4.0),
                    });

                    // Device name
                    cmds.push(RenderCommand::Text {
                        x: x + 36.0,
                        y: row_y + 6.0,
                        text: device.name.clone(),
                        font_size: 13.0,
                        color: p.text,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(width - 140.0),
                        overflow: TextOverflow::Ellipsis,
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
                        color: p.subtext0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width - 80.0),
                        overflow: TextOverflow::Ellipsis,
                    });

                    // Driver badge
                    cmds.push(RenderCommand::FillRect {
                        x: x + width - 84.0,
                        y: row_y + 8.0,
                        width: 60.0,
                        height: 16.0,
                        color: device.driver.color(p),
                        corner_radii: CornerRadii::all(3.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + width - 80.0,
                        y: row_y + 10.0,
                        text: if device.has_driver() { "OK" } else { "No drv" }.to_string(),
                        font_size: 9.0,
                        color: readable_on(device.driver.color(p)),
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });

                    row_y += 54.0;
                }
            }
        }
    }

    /// Render drivers tab.
    fn render_drivers(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Device drivers".to_string(),
            font_size: 16.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                color: {
                    // A wash of the same red the banner's text is in. Written
                    // as `Color::rgba(243, 139, 168, 30)` at this call site
                    // before the conversion -- a hardcoded palette value that
                    // was never in the block of constants, and so would have
                    // survived a survey that only emptied that block.
                    let c = p.red;
                    Color::rgba(c.r, c.g, c.b, 30)
                },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 8.0,
                text: format!("{} device(s) with driver issues", problems.len()),
                font_size: 13.0,
                color: p.red,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
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
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 6.0,
                text: device.name.clone(),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width - 100.0),
                overflow: TextOverflow::Ellipsis,
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
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Status badge
            cmds.push(RenderCommand::FillRect {
                x: x + width - 80.0,
                y: row_y + 10.0,
                width: 64.0,
                height: 18.0,
                color: device.driver.color(p),
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + width - 74.0,
                y: row_y + 12.0,
                text: device.driver.label().to_string(),
                font_size: 9.0,
                color: readable_on(device.driver.color(p)),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            row_y += 58.0;
        }
    }

    /// Render safe remove tab.
    fn render_safe_remove(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Safely remove devices".to_string(),
            font_size: 16.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 28.0;

        let removable = self.manager.safely_removable();

        if removable.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: "No removable devices connected.".to_string(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        } else {
            for device in &removable {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 56.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(6.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 8.0,
                    text: device.name.clone(),
                    font_size: 14.0,
                    color: p.text,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                cmds.push(RenderCommand::Text {
                    x: x + 16.0,
                    y: row_y + 28.0,
                    text: format!("{} — {}", device.manufacturer, device.bus_path),
                    font_size: 11.0,
                    color: p.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 120.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Eject button
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 72.0,
                    y: row_y + 14.0,
                    width: 56.0,
                    height: 28.0,
                    color: p.peach,
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 62.0,
                    y: row_y + 20.0,
                    text: "Eject".to_string(),
                    font_size: 12.0,
                    color: readable_on(p.peach),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                row_y += 62.0;
            }
        }
    }
}

impl Default for DeviceSettingsUI {
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
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::palette_check;

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
        assert!(
            counts
                .iter()
                .any(|(c, n)| *c == DeviceCategory::Usb && *n == 2)
        );
        assert!(
            counts
                .iter()
                .any(|(c, n)| *c == DeviceCategory::Audio && *n == 1)
        );
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
        let p = Palette::for_mode(false);
        let _c1 = DeviceStatus::Connected.color(&p);
        let _c2 = DeviceStatus::Error.color(&p);
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
        let cmds = ui.render(&Palette::for_mode(false), 0.0, 0.0, 600.0, 800.0);
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
    // ========================================================================
    // Colour
    // ========================================================================

    /// Every device status and every driver status, one device each, so a
    /// render reaches all eleven hues rather than the three a default panel
    /// happens to show.
    fn every_state_device_set() -> Vec<DeviceInfo> {
        let statuses = [
            DeviceStatus::Connected,
            DeviceStatus::Disconnected,
            DeviceStatus::Pairing,
            DeviceStatus::Error,
            DeviceStatus::Sleeping,
            DeviceStatus::Disabled,
        ];
        let drivers = [
            DriverStatus::Loaded,
            DriverStatus::NotFound,
            DriverStatus::Error,
            DriverStatus::Updating,
            DriverStatus::Disabled,
        ];
        let mut out = Vec::new();
        for (i, status) in statuses.iter().enumerate() {
            out.push(DeviceInfo {
                status: *status,
                driver: drivers[i % drivers.len()],
                removable: i % 2 == 0,
                ..sample_device(&format!("s{i}"), DeviceCategory::all()[i])
            });
        }
        for (i, driver) in drivers.iter().enumerate() {
            out.push(DeviceInfo {
                driver: *driver,
                ..sample_device(&format!("d{i}"), DeviceCategory::Usb)
            });
        }
        out
    }

    /// A panel wound into one state. Every axis here changes *which* colours get
    /// drawn: the driver-problem banner only exists when a driver is broken, the
    /// eject button only on the safe-remove tab with something to eject, the
    /// selected-row fill only with a selection, the placeholder grey only with
    /// an empty search box.
    fn wound_panel(
        devices: Vec<DeviceInfo>,
        tab: DeviceSettingsTab,
        selected: bool,
        expanded: bool,
        search: &str,
        toggles_on: bool,
    ) -> DeviceSettingsUI {
        let mut ui = DeviceSettingsUI {
            manager: DeviceManager::default(),
            active_tab: tab,
            search_query: search.to_string(),
            selected_device: None,
            expanded_category: expanded.then_some(DeviceCategory::Usb),
            scroll_offset: 0.0,
        };
        for d in devices {
            ui.manager.register_device(d);
        }
        if selected {
            ui.selected_device = ui.manager.devices.first().map(|d| d.id.clone());
        }
        ui.manager.show_disconnected = toggles_on;
        ui.manager.auto_install_drivers = toggles_on;
        ui.manager.safely_remove_notifications = toggles_on;
        ui.manager.usb_power_saving = toggles_on;
        ui
    }

    /// Every colour the panel paints is a role on the palette it was handed.
    ///
    /// Rendered in both modes; the light render is the one that does the work,
    /// because the fifteen deleted constants were Catppuccin Mocha values and
    /// none of them is a member of Latte. That includes the sixteenth, which was
    /// never a constant at all — the driver-problem banner's wash was
    /// `Color::rgba(243, 139, 168, 30)`, Mocha red's channels written out at the
    /// call site. The sweep compares roles on RGB alone, precisely so that a
    /// wash of a role still counts as that role, so no `derived` declaration is
    /// needed and the literal is still caught.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        let sets: [Vec<DeviceInfo>; 3] = [
            every_state_device_set(),
            vec![sample_device("quiet", DeviceCategory::Audio)],
            Vec::new(),
        ];
        for light in [false, true] {
            let p = Palette::for_mode(light);
            // The lettering on a filled badge is `readable_on` ink, which is a
            // computed colour rather than a role and so must be declared. The
            // fills are written out by hand rather than read back from
            // `DriverStatus::color` — an expectation taken from the code under
            // test is an echo of it, not a check on it.
            let ink = [
                readable_on(p.green),
                readable_on(p.yellow),
                readable_on(p.red),
                readable_on(p.blue),
                readable_on(p.overlay0),
                readable_on(p.peach),
            ];
            for set in &sets {
                for tab in DeviceSettingsTab::all() {
                    for selected in [false, true] {
                        for expanded in [false, true] {
                            for search in ["", "Device"] {
                                for toggles_on in [false, true] {
                                    let ui = wound_panel(
                                        set.clone(),
                                        *tab,
                                        selected,
                                        expanded,
                                        search,
                                        toggles_on,
                                    );
                                    let cmds = ui.render(&p, 0.0, 0.0, 600.0, 800.0);
                                    assert!(!cmds.is_empty(), "the panel always draws its frame");
                                    palette_check::assert_drawn_from(
                                        &p,
                                        &cmds,
                                        &ink,
                                        "device_settings",
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// A device's status, its driver's status and the four overview figures are
    /// *kinds*, and must not move when the user changes their accent.
    ///
    /// The assertion the membership sweep structurally cannot make: `p.accent`
    /// and `p.green` are both members of the palette, so a status dot wrongly
    /// painted in the accent passes the sweep in either mode. Only rendering the
    /// same state twice under two different accents can tell them apart.
    ///
    /// `DriverStatus::Updating` is the reason this test is here rather than
    /// merely nice to have: it is blue, blue is the default accent, and a
    /// converter reaching for the obvious role would have made it follow the
    /// user's colour while its four sibling states stayed put.
    #[test]
    fn a_device_status_does_not_follow_the_accent() {
        // The status dot is 8x8; the two driver badges are 60 and 64 wide; the
        // four overview figures are the only 18pt text the panel draws.
        let categorical = |accent: Color| -> Vec<Color> {
            let mut p = Palette::for_mode(false);
            p.accent = accent;
            let mut out = Vec::new();
            for tab in DeviceSettingsTab::all() {
                let ui = wound_panel(every_state_device_set(), *tab, false, true, "", true);
                for cmd in ui.render(&p, 0.0, 0.0, 600.0, 800.0) {
                    match cmd {
                        RenderCommand::FillRect { width, color, .. }
                            if width == 8.0 || width == 60.0 || width == 64.0 =>
                        {
                            out.push(color);
                        }
                        RenderCommand::Text {
                            font_size: 18.0,
                            color,
                            ..
                        } => out.push(color),
                        _ => {}
                    }
                }
            }
            out
        };

        // The settings toggle in its enabled position is the control that *does*
        // follow the accent, and is checked so this test cannot pass by the two
        // renders being identical everywhere.
        let accented = |accent: Color| -> Vec<Color> {
            let mut p = Palette::for_mode(false);
            p.accent = accent;
            let ui = wound_panel(
                every_state_device_set(),
                DeviceSettingsTab::Overview,
                false,
                true,
                "",
                true,
            );
            ui.render(&p, 0.0, 0.0, 600.0, 800.0)
                .into_iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        width,
                        height,
                        color,
                        ..
                    } if width == 40.0 && height == 20.0 => Some(color),
                    _ => None,
                })
                .collect()
        };

        let blue = Palette::for_mode(false).blue;
        let mauve = Palette::for_mode(false).mauve;
        assert_ne!(blue, mauve, "the two accents must differ");

        let with_blue = categorical(blue);
        assert!(
            !with_blue.is_empty(),
            "nothing categorical was found to check"
        );
        assert_eq!(
            with_blue,
            categorical(mauve),
            "a status colour moved when the accent did"
        );

        let toggles = accented(blue);
        assert!(!toggles.is_empty(), "no settings toggle was found");
        assert_ne!(
            toggles,
            accented(mauve),
            "nothing followed the accent, so this test measures nothing"
        );
    }
}

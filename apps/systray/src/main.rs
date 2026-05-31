#![allow(dead_code)]
//! OurOS System Tray — notification area and quick settings
//!
//! Provides the system tray (notification area) for the OurOS taskbar.
//! Features:
//! - Tray icons with notification badges and tooltip support
//! - Built-in system icons (volume, network, battery, clock, notifications, power)
//! - Quick settings flyout with toggles and sliders
//! - Volume popup with per-app mixing
//! - Network status popup
//! - Calendar popup from the clock
//! - Context menus and app popup menus
//!
//! Uses the guitk library for all rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

mod palette {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const SURFACE2: Color = Color::from_hex(0x585B70);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    pub const TEAL: Color = Color::from_hex(0x94E2D5);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
}

// ============================================================================
// Constants
// ============================================================================

/// Width of a single tray icon cell in pixels.
const ICON_CELL_SIZE: f32 = 36.0;

/// Height of the tray bar.
const TRAY_HEIGHT: f32 = 40.0;

/// Padding within popups.
const POPUP_PADDING: f32 = 12.0;

/// Popup corner radius.
const POPUP_RADIUS: f32 = 8.0;

/// Spacing between popup items.
const ITEM_SPACING: f32 = 6.0;

/// Slider track height.
const SLIDER_TRACK_HEIGHT: f32 = 4.0;

/// Slider thumb radius.
const SLIDER_THUMB_RADIUS: f32 = 8.0;

/// Font size for tray icons.
const ICON_FONT_SIZE: f32 = 18.0;

/// Font size for popup text.
const POPUP_FONT_SIZE: f32 = 13.0;

/// Font size for popup headers.
const HEADER_FONT_SIZE: f32 = 15.0;

/// Calendar cell size.
const CALENDAR_CELL: f32 = 32.0;

/// Toggle pill width/height.
const TOGGLE_WIDTH: f32 = 40.0;
const TOGGLE_HEIGHT: f32 = 22.0;

// ============================================================================
// Tray icon types
// ============================================================================

/// Unique identifier for a tray icon.
pub type TrayIconId = u64;

/// A tray icon registered by an application.
#[derive(Clone, Debug)]
pub struct TrayIcon {
    pub id: TrayIconId,
    pub app_name: String,
    /// Single character used as icon placeholder (until bitmap icons are supported).
    pub icon_char: char,
    pub tooltip: String,
    pub visible: bool,
    pub has_notification_badge: bool,
}

impl TrayIcon {
    pub fn new(id: TrayIconId, app_name: &str, icon_char: char, tooltip: &str) -> Self {
        Self {
            id,
            app_name: app_name.to_string(),
            icon_char,
            tooltip: tooltip.to_string(),
            visible: true,
            has_notification_badge: false,
        }
    }
}

// ============================================================================
// Built-in system icon identifiers
// ============================================================================

/// Reserved IDs for built-in system icons (0..99 range).
const ICON_ID_VOLUME: TrayIconId = 1;
const ICON_ID_NETWORK: TrayIconId = 2;
const ICON_ID_BATTERY: TrayIconId = 3;
const ICON_ID_CLOCK: TrayIconId = 4;
const ICON_ID_NOTIFICATIONS: TrayIconId = 5;
const ICON_ID_POWER: TrayIconId = 6;

// ============================================================================
// Popup types
// ============================================================================

/// Which popup is currently open (only one at a time).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PopupType {
    None,
    QuickSettings,
    Volume,
    Network,
    Calendar,
    AppMenu(TrayIconId),
    ContextMenu(TrayIconId),
    PowerMenu,
}

// ============================================================================
// Quick settings state
// ============================================================================

/// Toggle state for quick settings items.
#[derive(Clone, Debug)]
pub struct QuickSettingsState {
    pub wifi_enabled: bool,
    pub wifi_network_name: String,
    pub bluetooth_enabled: bool,
    pub do_not_disturb: bool,
    pub night_light: bool,
    pub battery_saver: bool,
    pub airplane_mode: bool,
    pub brightness: u8,
}

impl Default for QuickSettingsState {
    fn default() -> Self {
        Self {
            wifi_enabled: true,
            wifi_network_name: String::from("HomeNetwork"),
            bluetooth_enabled: true,
            do_not_disturb: false,
            night_light: false,
            battery_saver: false,
            airplane_mode: false,
            brightness: 80,
        }
    }
}

// ============================================================================
// Volume state
// ============================================================================

/// Per-app volume entry.
#[derive(Clone, Debug)]
pub struct AppVolume {
    pub app_name: String,
    pub volume: u8,
    pub muted: bool,
}

/// Volume subsystem state.
#[derive(Clone, Debug)]
pub struct VolumeState {
    pub master_volume: u8,
    pub muted: bool,
    pub output_device: String,
    pub app_volumes: Vec<AppVolume>,
}

impl Default for VolumeState {
    fn default() -> Self {
        Self {
            master_volume: 75,
            muted: false,
            output_device: String::from("Built-in Speakers"),
            app_volumes: vec![
                AppVolume {
                    app_name: String::from("Music Player"),
                    volume: 100,
                    muted: false,
                },
                AppVolume {
                    app_name: String::from("Browser"),
                    volume: 60,
                    muted: false,
                },
            ],
        }
    }
}

// ============================================================================
// Network state
// ============================================================================

/// Network connection info.
#[derive(Clone, Debug)]
pub struct NetworkInfo {
    pub connected: bool,
    pub ssid: String,
    /// Signal strength 0-100.
    pub signal_strength: u8,
    pub ip_address: String,
}

impl Default for NetworkInfo {
    fn default() -> Self {
        Self {
            connected: true,
            ssid: String::from("HomeNetwork"),
            signal_strength: 85,
            ip_address: String::from("192.168.1.42"),
        }
    }
}

// ============================================================================
// Battery state
// ============================================================================

/// Battery status.
#[derive(Clone, Debug)]
pub struct BatteryInfo {
    pub percentage: u8,
    pub charging: bool,
    /// Estimated minutes remaining (None if plugged in/unknown).
    pub estimated_minutes: Option<u32>,
}

impl Default for BatteryInfo {
    fn default() -> Self {
        Self {
            percentage: 72,
            charging: false,
            estimated_minutes: Some(195),
        }
    }
}

// ============================================================================
// Date/time state
// ============================================================================

/// Minimal date/time representation (no external crate dependency).
#[derive(Clone, Debug)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// Day of week (0=Sunday, 6=Saturday).
    pub weekday: u8,
}

impl Default for DateTime {
    fn default() -> Self {
        Self {
            year: 2026,
            month: 5,
            day: 17,
            hour: 14,
            minute: 30,
            second: 0,
            weekday: 0,
        }
    }
}

impl DateTime {
    /// Format time as HH:MM.
    pub fn time_str(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// Format date as Month Day, Year.
    pub fn date_str(&self) -> String {
        let month_name = match self.month {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            12 => "Dec",
            _ => "???",
        };
        format!("{} {}, {}", month_name, self.day, self.year)
    }

    /// Number of days in the current month.
    pub fn days_in_month(&self) -> u8 {
        match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                let y = self.year as u32;
                if (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }

    /// Day of week for the first day of the current month (0=Sunday).
    /// Uses Zeller-like calculation.
    pub fn first_weekday_of_month(&self) -> u8 {
        // Tomohiko Sakamoto's algorithm
        let y = self.year as i32;
        let m = self.month as usize;
        let d = 1_i32;
        let offsets: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let y_adj = if m < 3 { y - 1 } else { y };
        let offset = offsets.get(m.wrapping_sub(1)).copied().unwrap_or(0);
        let result = (y_adj + y_adj / 4 - y_adj / 100 + y_adj / 400 + offset + d) % 7;
        if result < 0 {
            (result + 7) as u8
        } else {
            result as u8
        }
    }
}

// ============================================================================
// System Tray — main struct
// ============================================================================

/// The system tray / notification area component.
pub struct SystemTray {
    /// Registered tray icons (both built-in and third-party).
    pub icons: Vec<TrayIcon>,
    /// Currently active popup.
    pub active_popup: PopupType,
    /// Quick settings state.
    pub quick_settings: QuickSettingsState,
    /// Volume state.
    pub volume: VolumeState,
    /// Network info.
    pub network: NetworkInfo,
    /// Battery info.
    pub battery: BatteryInfo,
    /// Current date/time.
    pub datetime: DateTime,
    /// X position of the tray area (set by taskbar layout).
    pub tray_x: f32,
    /// Y position of the tray area (top of the tray bar).
    pub tray_y: f32,
    /// Total width of the tray area (computed from icon count).
    pub tray_width: f32,
    /// Counter for generating unique icon IDs.
    next_icon_id: TrayIconId,
}

impl SystemTray {
    /// Create a new system tray with built-in icons.
    pub fn new(tray_x: f32, tray_y: f32) -> Self {
        let icons = vec![
            TrayIcon::new(
                ICON_ID_NOTIFICATIONS,
                "Notifications",
                '\u{1F514}',
                "Notifications",
            ),
            TrayIcon::new(ICON_ID_VOLUME, "Volume", '\u{1F50A}', "Volume: 75%"),
            TrayIcon::new(ICON_ID_NETWORK, "Network", '\u{1F4F6}', "Connected"),
            TrayIcon::new(ICON_ID_BATTERY, "Battery", '\u{1F50B}', "72% remaining"),
            TrayIcon::new(ICON_ID_CLOCK, "Clock", '\u{1F552}', "14:30"),
            TrayIcon::new(ICON_ID_POWER, "Power", '\u{23FB}', "Power options"),
        ];
        let icon_count = icons.iter().filter(|i| i.visible).count() as f32;
        let tray_width = icon_count * ICON_CELL_SIZE;

        Self {
            icons,
            active_popup: PopupType::None,
            quick_settings: QuickSettingsState::default(),
            volume: VolumeState::default(),
            network: NetworkInfo::default(),
            battery: BatteryInfo::default(),
            datetime: DateTime::default(),
            tray_x,
            tray_y,
            tray_width,
            next_icon_id: 100, // Reserve 0-99 for built-in icons
        }
    }

    /// Register a new third-party tray icon. Returns its assigned ID.
    pub fn register_icon(&mut self, app_name: &str, icon_char: char, tooltip: &str) -> TrayIconId {
        let id = self.next_icon_id;
        self.next_icon_id = self.next_icon_id.saturating_add(1);
        self.icons
            .push(TrayIcon::new(id, app_name, icon_char, tooltip));
        self.recalculate_width();
        id
    }

    /// Remove a tray icon by ID.
    pub fn remove_icon(&mut self, id: TrayIconId) {
        self.icons.retain(|icon| icon.id != id);
        self.recalculate_width();
    }

    /// Set notification badge on an icon.
    pub fn set_badge(&mut self, id: TrayIconId, has_badge: bool) {
        if let Some(icon) = self.icons.iter_mut().find(|i| i.id == id) {
            icon.has_notification_badge = has_badge;
        }
    }

    /// Update the clock/datetime (called by tick).
    pub fn tick(&mut self, elapsed_ms: u64) {
        // Advance seconds by elapsed time
        let total_seconds = elapsed_ms / 1000;
        if total_seconds == 0 {
            return;
        }
        let new_sec = self.datetime.second as u64 + total_seconds;
        self.datetime.second = (new_sec % 60) as u8;
        let carry_min = new_sec / 60;
        if carry_min > 0 {
            let new_min = self.datetime.minute as u64 + carry_min;
            self.datetime.minute = (new_min % 60) as u8;
            let carry_hour = new_min / 60;
            if carry_hour > 0 {
                let new_hour = self.datetime.hour as u64 + carry_hour;
                self.datetime.hour = (new_hour % 24) as u8;
            }
        }
        // Update battery estimation
        if let Some(ref mut mins) = self.battery.estimated_minutes {
            *mins = mins.saturating_sub(total_seconds as u32 / 60);
        }
        // Update clock icon tooltip
        if let Some(clock_icon) = self.icons.iter_mut().find(|i| i.id == ICON_ID_CLOCK) {
            clock_icon.tooltip = self.datetime.time_str();
        }
    }

    /// Handle a click event. Returns true if the event was consumed.
    pub fn handle_click(&mut self, x: f32, y: f32, button: MouseButton) -> bool {
        // Check if click is within tray bar area
        if y >= self.tray_y
            && y <= self.tray_y + TRAY_HEIGHT
            && x >= self.tray_x
            && x <= self.tray_x + self.tray_width
        {
            let relative_x = x - self.tray_x;
            let icon_index = (relative_x / ICON_CELL_SIZE) as usize;

            let visible_icons: Vec<TrayIconId> = self
                .icons
                .iter()
                .filter(|i| i.visible)
                .map(|i| i.id)
                .collect();

            if let Some(&icon_id) = visible_icons.get(icon_index) {
                match button {
                    MouseButton::Left => self.handle_icon_left_click(icon_id),
                    MouseButton::Right => self.handle_icon_right_click(icon_id),
                    _ => {}
                }
                return true;
            }

            // Click on empty tray area -> quick settings
            if button == MouseButton::Left {
                self.toggle_popup(PopupType::QuickSettings);
            }
            return true;
        }

        // If a popup is open, check if click is within it — otherwise close
        if self.active_popup != PopupType::None {
            self.active_popup = PopupType::None;
            return true;
        }

        false
    }

    /// Handle a double-click on a tray icon (open associated app).
    pub fn handle_double_click(&mut self, x: f32, y: f32) -> Option<TrayIconId> {
        if y >= self.tray_y
            && y <= self.tray_y + TRAY_HEIGHT
            && x >= self.tray_x
            && x <= self.tray_x + self.tray_width
        {
            let relative_x = x - self.tray_x;
            let icon_index = (relative_x / ICON_CELL_SIZE) as usize;

            let visible_icons: Vec<TrayIconId> = self
                .icons
                .iter()
                .filter(|i| i.visible)
                .map(|i| i.id)
                .collect();

            return visible_icons.get(icon_index).copied();
        }
        None
    }

    /// Render the entire tray (bar + active popup) to a `RenderTree`.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Render tray bar background
        tree.push(RenderCommand::FillRect {
            x: self.tray_x,
            y: self.tray_y,
            width: self.tray_width,
            height: TRAY_HEIGHT,
            color: palette::MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Render each visible icon
        let mut offset_x = self.tray_x;
        for icon in self.icons.iter().filter(|i| i.visible) {
            self.render_icon(&mut tree, icon, offset_x);
            offset_x += ICON_CELL_SIZE;
        }

        // Render active popup
        match &self.active_popup {
            PopupType::None => {}
            PopupType::QuickSettings => self.render_quick_settings(&mut tree),
            PopupType::Volume => self.render_volume_popup(&mut tree),
            PopupType::Network => self.render_network_popup(&mut tree),
            PopupType::Calendar => self.render_calendar_popup(&mut tree),
            PopupType::AppMenu(id) => self.render_app_menu(&mut tree, *id),
            PopupType::ContextMenu(id) => self.render_context_menu(&mut tree, *id),
            PopupType::PowerMenu => self.render_power_menu(&mut tree),
        }

        tree
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    fn recalculate_width(&mut self) {
        let count = self.icons.iter().filter(|i| i.visible).count() as f32;
        self.tray_width = count * ICON_CELL_SIZE;
    }

    fn toggle_popup(&mut self, popup: PopupType) {
        if self.active_popup == popup {
            self.active_popup = PopupType::None;
        } else {
            self.active_popup = popup;
        }
    }

    fn handle_icon_left_click(&mut self, icon_id: TrayIconId) {
        match icon_id {
            ICON_ID_VOLUME => self.toggle_popup(PopupType::Volume),
            ICON_ID_NETWORK => self.toggle_popup(PopupType::Network),
            ICON_ID_CLOCK => self.toggle_popup(PopupType::Calendar),
            ICON_ID_NOTIFICATIONS => self.toggle_popup(PopupType::QuickSettings),
            ICON_ID_POWER => self.toggle_popup(PopupType::PowerMenu),
            ICON_ID_BATTERY => self.toggle_popup(PopupType::QuickSettings),
            other => self.toggle_popup(PopupType::AppMenu(other)),
        }
    }

    fn handle_icon_right_click(&mut self, icon_id: TrayIconId) {
        self.toggle_popup(PopupType::ContextMenu(icon_id));
    }

    /// Get the X position for a popup aligned to a specific icon.
    fn popup_x_for_icon(&self, icon_id: TrayIconId) -> f32 {
        let index = self
            .icons
            .iter()
            .filter(|i| i.visible)
            .position(|i| i.id == icon_id)
            .unwrap_or(0);
        self.tray_x + (index as f32 * ICON_CELL_SIZE)
    }

    fn render_icon(&self, tree: &mut RenderTree, icon: &TrayIcon, x: f32) {
        let center_x = x + ICON_CELL_SIZE / 2.0;
        let center_y = self.tray_y + TRAY_HEIGHT / 2.0;

        // Icon character (for clock, show time instead)
        let display_text = if icon.id == ICON_ID_CLOCK {
            self.datetime.time_str()
        } else if icon.id == ICON_ID_VOLUME {
            if self.volume.muted {
                String::from("\u{1F507}") // muted
            } else {
                String::from("\u{1F50A}") // speaker
            }
        } else if icon.id == ICON_ID_NETWORK {
            if self.network.connected {
                String::from("\u{1F4F6}") // signal bars
            } else {
                String::from("\u{274C}") // X mark
            }
        } else {
            icon.icon_char.to_string()
        };

        let font_size = if icon.id == ICON_ID_CLOCK {
            11.0
        } else {
            ICON_FONT_SIZE
        };

        // Render the icon text centered in the cell
        tree.push(RenderCommand::Text {
            x: center_x - font_size / 2.0,
            y: center_y - font_size / 2.0,
            text: display_text,
            color: palette::TEXT,
            font_size,
            font_weight: FontWeightHint::Regular,
            max_width: Some(ICON_CELL_SIZE),
        });

        // Notification badge (small colored dot in top-right)
        if icon.has_notification_badge {
            tree.push(RenderCommand::FillRect {
                x: x + ICON_CELL_SIZE - 10.0,
                y: self.tray_y + 4.0,
                width: 8.0,
                height: 8.0,
                color: palette::RED,
                corner_radii: CornerRadii::all(4.0),
            });
        }
    }

    // ========================================================================
    // Quick Settings popup
    // ========================================================================

    fn render_quick_settings(&self, tree: &mut RenderTree) {
        let popup_width = 300.0;
        let popup_height = 340.0;
        let popup_x = self.tray_x + self.tray_width - popup_width;
        let popup_y = self.tray_y - popup_height - 8.0;

        self.render_popup_background(tree, popup_x, popup_y, popup_width, popup_height);

        let mut y = popup_y + POPUP_PADDING;
        let content_x = popup_x + POPUP_PADDING;
        let content_width = popup_width - POPUP_PADDING * 2.0;

        // Header
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: String::from("Quick Settings"),
            color: palette::TEXT,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_width),
        });
        y += HEADER_FONT_SIZE + ITEM_SPACING * 2.0;

        // WiFi toggle
        y = self.render_toggle_row(
            tree,
            content_x,
            y,
            content_width,
            "WiFi",
            &self.quick_settings.wifi_network_name,
            self.quick_settings.wifi_enabled,
        );

        // Bluetooth toggle
        y = self.render_toggle_row(
            tree,
            content_x,
            y,
            content_width,
            "Bluetooth",
            "On",
            self.quick_settings.bluetooth_enabled,
        );

        // Do Not Disturb
        y = self.render_toggle_row(
            tree,
            content_x,
            y,
            content_width,
            "Do Not Disturb",
            "",
            self.quick_settings.do_not_disturb,
        );

        // Night Light
        y = self.render_toggle_row(
            tree,
            content_x,
            y,
            content_width,
            "Night Light",
            "",
            self.quick_settings.night_light,
        );

        // Battery Saver
        y = self.render_toggle_row(
            tree,
            content_x,
            y,
            content_width,
            "Battery Saver",
            "",
            self.quick_settings.battery_saver,
        );

        // Airplane Mode
        y = self.render_toggle_row(
            tree,
            content_x,
            y,
            content_width,
            "Airplane Mode",
            "",
            self.quick_settings.airplane_mode,
        );

        // Brightness slider
        y += ITEM_SPACING;
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: format!("Brightness: {}%", self.quick_settings.brightness),
            color: palette::SUBTEXT0,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        y += POPUP_FONT_SIZE + 4.0;
        self.render_slider(
            tree,
            content_x,
            y,
            content_width,
            self.quick_settings.brightness,
            palette::YELLOW,
        );
        y += SLIDER_THUMB_RADIUS * 2.0 + ITEM_SPACING;

        // Volume slider
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: format!("Volume: {}%", self.volume.master_volume),
            color: palette::SUBTEXT0,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        y += POPUP_FONT_SIZE + 4.0;
        self.render_slider(
            tree,
            content_x,
            y,
            content_width,
            self.volume.master_volume,
            palette::BLUE,
        );
        let _ = y; // suppress unused warning for last y value
    }

    // ========================================================================
    // Volume popup
    // ========================================================================

    fn render_volume_popup(&self, tree: &mut RenderTree) {
        let popup_width = 280.0;
        let app_count = self.volume.app_volumes.len();
        let popup_height = 140.0 + (app_count as f32 * 50.0);
        let popup_x = self.popup_x_for_icon(ICON_ID_VOLUME);
        let popup_y = self.tray_y - popup_height - 8.0;

        self.render_popup_background(tree, popup_x, popup_y, popup_width, popup_height);

        let mut y = popup_y + POPUP_PADDING;
        let content_x = popup_x + POPUP_PADDING;
        let content_width = popup_width - POPUP_PADDING * 2.0;

        // Header
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: String::from("Volume"),
            color: palette::TEXT,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_width),
        });
        y += HEADER_FONT_SIZE + ITEM_SPACING;

        // Output device
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: self.volume.output_device.clone(),
            color: palette::SUBTEXT0,
            font_size: POPUP_FONT_SIZE - 1.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        y += POPUP_FONT_SIZE + ITEM_SPACING;

        // Master volume label + mute indicator
        let mute_label = if self.volume.muted { " (Muted)" } else { "" };
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: format!("Master: {}%{}", self.volume.master_volume, mute_label),
            color: if self.volume.muted {
                palette::OVERLAY0
            } else {
                palette::TEXT
            },
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        y += POPUP_FONT_SIZE + 4.0;

        // Master volume slider
        let slider_color = if self.volume.muted {
            palette::SURFACE2
        } else {
            palette::BLUE
        };
        self.render_slider(
            tree,
            content_x,
            y,
            content_width,
            self.volume.master_volume,
            slider_color,
        );
        y += SLIDER_THUMB_RADIUS * 2.0 + ITEM_SPACING * 2.0;

        // Separator
        tree.push(RenderCommand::FillRect {
            x: content_x,
            y,
            width: content_width,
            height: 1.0,
            color: palette::SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });
        y += ITEM_SPACING * 2.0;

        // Per-app volumes
        for app_vol in &self.volume.app_volumes {
            let mute_marker = if app_vol.muted { " \u{1F507}" } else { "" };
            tree.push(RenderCommand::Text {
                x: content_x,
                y,
                text: format!("{}: {}%{}", app_vol.app_name, app_vol.volume, mute_marker),
                color: if app_vol.muted {
                    palette::OVERLAY0
                } else {
                    palette::SUBTEXT1
                },
                font_size: POPUP_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_width),
            });
            y += POPUP_FONT_SIZE + 4.0;

            let app_color = if app_vol.muted {
                palette::SURFACE2
            } else {
                palette::TEAL
            };
            self.render_slider(tree, content_x, y, content_width, app_vol.volume, app_color);
            y += SLIDER_THUMB_RADIUS * 2.0 + ITEM_SPACING;
        }
        let _ = y;
    }

    // ========================================================================
    // Network popup
    // ========================================================================

    fn render_network_popup(&self, tree: &mut RenderTree) {
        let popup_width = 260.0;
        let popup_height = 160.0;
        let popup_x = self.popup_x_for_icon(ICON_ID_NETWORK);
        let popup_y = self.tray_y - popup_height - 8.0;

        self.render_popup_background(tree, popup_x, popup_y, popup_width, popup_height);

        let mut y = popup_y + POPUP_PADDING;
        let content_x = popup_x + POPUP_PADDING;
        let content_width = popup_width - POPUP_PADDING * 2.0;

        // Header
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: String::from("Network"),
            color: palette::TEXT,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_width),
        });
        y += HEADER_FONT_SIZE + ITEM_SPACING * 2.0;

        // Status
        let status_text = if self.network.connected {
            "Connected"
        } else {
            "Disconnected"
        };
        let status_color = if self.network.connected {
            palette::GREEN
        } else {
            palette::RED
        };
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: format!("Status: {}", status_text),
            color: status_color,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        y += POPUP_FONT_SIZE + ITEM_SPACING;

        // SSID
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: format!("Network: {}", self.network.ssid),
            color: palette::SUBTEXT1,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        y += POPUP_FONT_SIZE + ITEM_SPACING;

        // Signal strength bar
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: format!("Signal: {}%", self.network.signal_strength),
            color: palette::SUBTEXT0,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        y += POPUP_FONT_SIZE + 4.0;
        self.render_slider(
            tree,
            content_x,
            y,
            content_width,
            self.network.signal_strength,
            palette::GREEN,
        );
        y += SLIDER_THUMB_RADIUS * 2.0 + ITEM_SPACING;

        // IP address
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: format!("IP: {}", self.network.ip_address),
            color: palette::SUBTEXT0,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        y += POPUP_FONT_SIZE + ITEM_SPACING * 2.0;

        // Network settings link
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: String::from("Network Settings..."),
            color: palette::BLUE,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        let _ = y;
    }

    // ========================================================================
    // Calendar popup
    // ========================================================================

    fn render_calendar_popup(&self, tree: &mut RenderTree) {
        let popup_width = 7.0 * CALENDAR_CELL + POPUP_PADDING * 2.0;
        let popup_height = 280.0;
        let popup_x = self.popup_x_for_icon(ICON_ID_CLOCK);
        let popup_y = self.tray_y - popup_height - 8.0;

        self.render_popup_background(tree, popup_x, popup_y, popup_width, popup_height);

        let mut y = popup_y + POPUP_PADDING;
        let content_x = popup_x + POPUP_PADDING;
        let content_width = popup_width - POPUP_PADDING * 2.0;

        // Month/Year header with nav arrows
        let month_name = match self.datetime.month {
            1 => "January",
            2 => "February",
            3 => "March",
            4 => "April",
            5 => "May",
            6 => "June",
            7 => "July",
            8 => "August",
            9 => "September",
            10 => "October",
            11 => "November",
            12 => "December",
            _ => "Unknown",
        };

        // Left arrow
        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: String::from("\u{25C0}"),
            color: palette::SUBTEXT0,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Month Year centered
        let header_text = format!("{} {}", month_name, self.datetime.year);
        tree.push(RenderCommand::Text {
            x: content_x + content_width / 2.0 - 40.0,
            y,
            text: header_text,
            color: palette::TEXT,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_width),
        });

        // Right arrow
        tree.push(RenderCommand::Text {
            x: content_x + content_width - POPUP_FONT_SIZE,
            y,
            text: String::from("\u{25B6}"),
            color: palette::SUBTEXT0,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        y += HEADER_FONT_SIZE + ITEM_SPACING * 2.0;

        // Day-of-week headers
        let day_headers = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
        for (i, header) in day_headers.iter().enumerate() {
            tree.push(RenderCommand::Text {
                x: content_x + (i as f32) * CALENDAR_CELL + CALENDAR_CELL / 2.0 - 6.0,
                y,
                text: String::from(*header),
                color: palette::OVERLAY0,
                font_size: POPUP_FONT_SIZE - 1.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
        y += POPUP_FONT_SIZE + ITEM_SPACING;

        // Calendar grid
        let first_weekday = self.datetime.first_weekday_of_month();
        let days_in_month = self.datetime.days_in_month();
        let mut col = first_weekday as usize;
        let mut row_y = y;

        for day in 1..=days_in_month {
            let cell_x = content_x + (col as f32) * CALENDAR_CELL;
            let cell_center_x = cell_x + CALENDAR_CELL / 2.0 - 5.0;

            // Highlight today
            let is_today = day == self.datetime.day;
            if is_today {
                tree.push(RenderCommand::FillRect {
                    x: cell_x + 2.0,
                    y: row_y - 2.0,
                    width: CALENDAR_CELL - 4.0,
                    height: CALENDAR_CELL - 4.0,
                    color: palette::BLUE,
                    corner_radii: CornerRadii::all(CALENDAR_CELL / 2.0 - 2.0),
                });
            }

            let text_color = if is_today {
                palette::CRUST
            } else {
                palette::TEXT
            };
            tree.push(RenderCommand::Text {
                x: cell_center_x,
                y: row_y + 4.0,
                text: format!("{}", day),
                color: text_color,
                font_size: POPUP_FONT_SIZE,
                font_weight: if is_today {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            col += 1;
            if col >= 7 {
                col = 0;
                row_y += CALENDAR_CELL;
            }
        }

        // Upcoming events placeholder
        let events_y = popup_y + popup_height - 40.0;
        tree.push(RenderCommand::FillRect {
            x: content_x,
            y: events_y - 4.0,
            width: content_width,
            height: 1.0,
            color: palette::SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });
        tree.push(RenderCommand::Text {
            x: content_x,
            y: events_y + 4.0,
            text: String::from("No upcoming events"),
            color: palette::OVERLAY0,
            font_size: POPUP_FONT_SIZE - 1.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
    }

    // ========================================================================
    // App menu popup
    // ========================================================================

    fn render_app_menu(&self, tree: &mut RenderTree, icon_id: TrayIconId) {
        let popup_width = 180.0;
        let popup_height = 80.0;
        let popup_x = self.popup_x_for_icon(icon_id);
        let popup_y = self.tray_y - popup_height - 8.0;

        self.render_popup_background(tree, popup_x, popup_y, popup_width, popup_height);

        let content_x = popup_x + POPUP_PADDING;
        let content_width = popup_width - POPUP_PADDING * 2.0;
        let mut y = popup_y + POPUP_PADDING;

        // App name as header
        let app_name = self
            .icons
            .iter()
            .find(|i| i.id == icon_id)
            .map(|i| i.app_name.as_str())
            .unwrap_or("Unknown");

        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: String::from(app_name),
            color: palette::TEXT,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(content_width),
        });
        y += HEADER_FONT_SIZE + ITEM_SPACING;

        tree.push(RenderCommand::Text {
            x: content_x,
            y,
            text: String::from("Open"),
            color: palette::SUBTEXT1,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_width),
        });
        let _ = y;
    }

    // ========================================================================
    // Context menu popup (right-click)
    // ========================================================================

    fn render_context_menu(&self, tree: &mut RenderTree, icon_id: TrayIconId) {
        let popup_width = 160.0;
        let popup_height = 100.0;
        let popup_x = self.popup_x_for_icon(icon_id);
        let popup_y = self.tray_y - popup_height - 8.0;

        self.render_popup_background(tree, popup_x, popup_y, popup_width, popup_height);

        let content_x = popup_x + POPUP_PADDING;
        let content_width = popup_width - POPUP_PADDING * 2.0;
        let mut y = popup_y + POPUP_PADDING;

        let items = ["Show", "Hide", "Remove from tray"];
        for item in &items {
            tree.push(RenderCommand::Text {
                x: content_x,
                y,
                text: String::from(*item),
                color: palette::TEXT,
                font_size: POPUP_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_width),
            });
            y += POPUP_FONT_SIZE + ITEM_SPACING;
        }
        let _ = y;
    }

    // ========================================================================
    // Power menu popup
    // ========================================================================

    fn render_power_menu(&self, tree: &mut RenderTree) {
        let popup_width = 180.0;
        let popup_height = 140.0;
        let popup_x = self.popup_x_for_icon(ICON_ID_POWER);
        let popup_y = self.tray_y - popup_height - 8.0;

        self.render_popup_background(tree, popup_x, popup_y, popup_width, popup_height);

        let content_x = popup_x + POPUP_PADDING;
        let content_width = popup_width - POPUP_PADDING * 2.0;
        let mut y = popup_y + POPUP_PADDING;

        let items = [
            ("\u{23FB}", "Shut Down"),
            ("\u{1F504}", "Restart"),
            ("\u{1F4A4}", "Sleep"),
            ("\u{1F512}", "Lock"),
            ("\u{1F6AA}", "Sign Out"),
        ];

        for (icon, label) in &items {
            tree.push(RenderCommand::Text {
                x: content_x,
                y,
                text: format!("{}  {}", icon, label),
                color: palette::TEXT,
                font_size: POPUP_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_width),
            });
            y += POPUP_FONT_SIZE + ITEM_SPACING;
        }
        let _ = y;
    }

    // ========================================================================
    // Shared rendering helpers
    // ========================================================================

    /// Render a popup background with shadow, rounded corners, and border.
    fn render_popup_background(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Shadow
        tree.push(RenderCommand::BoxShadow {
            x,
            y,
            width,
            height,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 2.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(POPUP_RADIUS),
        });

        // Background fill
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: palette::BASE,
            corner_radii: CornerRadii::all(POPUP_RADIUS),
        });

        // Border
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            color: palette::SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(POPUP_RADIUS),
        });
    }

    /// Render a horizontal slider track + thumb.
    fn render_slider(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        width: f32,
        value: u8,
        active_color: Color,
    ) {
        let track_y = y + SLIDER_THUMB_RADIUS - SLIDER_TRACK_HEIGHT / 2.0;
        let fill_fraction = (value as f32) / 100.0;
        let fill_width = width * fill_fraction;

        // Track background
        tree.push(RenderCommand::FillRect {
            x,
            y: track_y,
            width,
            height: SLIDER_TRACK_HEIGHT,
            color: palette::SURFACE0,
            corner_radii: CornerRadii::all(SLIDER_TRACK_HEIGHT / 2.0),
        });

        // Track fill (active portion)
        if fill_width > 0.0 {
            tree.push(RenderCommand::FillRect {
                x,
                y: track_y,
                width: fill_width,
                height: SLIDER_TRACK_HEIGHT,
                color: active_color,
                corner_radii: CornerRadii::all(SLIDER_TRACK_HEIGHT / 2.0),
            });
        }

        // Thumb circle
        let thumb_x = x + fill_width - SLIDER_THUMB_RADIUS;
        let thumb_y = y;
        tree.push(RenderCommand::FillRect {
            x: thumb_x,
            y: thumb_y,
            width: SLIDER_THUMB_RADIUS * 2.0,
            height: SLIDER_THUMB_RADIUS * 2.0,
            color: palette::TEXT,
            corner_radii: CornerRadii::all(SLIDER_THUMB_RADIUS),
        });
    }

    /// Render a toggle row (label + optional subtitle + toggle pill).
    /// Returns the Y position after the row.
    fn render_toggle_row(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        subtitle: &str,
        enabled: bool,
    ) -> f32 {
        // Label
        tree.push(RenderCommand::Text {
            x,
            y,
            text: String::from(label),
            color: palette::TEXT,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - TOGGLE_WIDTH - 8.0),
        });

        // Subtitle (if non-empty)
        if !subtitle.is_empty() {
            tree.push(RenderCommand::Text {
                x,
                y: y + POPUP_FONT_SIZE + 2.0,
                text: String::from(subtitle),
                color: palette::OVERLAY0,
                font_size: POPUP_FONT_SIZE - 2.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - TOGGLE_WIDTH - 8.0),
            });
        }

        // Toggle pill (right-aligned)
        let toggle_x = x + width - TOGGLE_WIDTH;
        let toggle_y = y + (POPUP_FONT_SIZE - TOGGLE_HEIGHT) / 2.0 + 2.0;

        let pill_color = if enabled {
            palette::BLUE
        } else {
            palette::SURFACE1
        };
        tree.push(RenderCommand::FillRect {
            x: toggle_x,
            y: toggle_y,
            width: TOGGLE_WIDTH,
            height: TOGGLE_HEIGHT,
            color: pill_color,
            corner_radii: CornerRadii::all(TOGGLE_HEIGHT / 2.0),
        });

        // Toggle knob
        let knob_radius = TOGGLE_HEIGHT / 2.0 - 3.0;
        let knob_x = if enabled {
            toggle_x + TOGGLE_WIDTH - knob_radius * 2.0 - 3.0
        } else {
            toggle_x + 3.0
        };
        let knob_y = toggle_y + 3.0;
        tree.push(RenderCommand::FillRect {
            x: knob_x,
            y: knob_y,
            width: knob_radius * 2.0,
            height: knob_radius * 2.0,
            color: palette::TEXT,
            corner_radii: CornerRadii::all(knob_radius),
        });

        let row_height = if subtitle.is_empty() {
            POPUP_FONT_SIZE + ITEM_SPACING * 2.0
        } else {
            POPUP_FONT_SIZE * 2.0 + ITEM_SPACING * 2.0
        };

        y + row_height
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    // The system tray runs as a service, receiving events from the compositor
    // and rendering into its assigned taskbar region. In a real environment,
    // it would connect to the compositor via IPC and run an event loop.
    //
    // For now, create an instance and render one frame as a demonstration.

    let tray = SystemTray::new(800.0, 760.0);
    let frame = tray.render();

    // In production, `frame.commands` would be submitted to the compositor.
    // Here we just confirm it renders without error.
    let _ = frame.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_system_tray() {
        let tray = SystemTray::new(0.0, 0.0);
        assert_eq!(tray.icons.len(), 6); // 6 built-in icons
        assert_eq!(tray.active_popup, PopupType::None);
    }

    #[test]
    fn test_register_and_remove_icon() {
        let mut tray = SystemTray::new(0.0, 0.0);
        let initial_count = tray.icons.len();

        let id = tray.register_icon("TestApp", 'T', "Test Application");
        assert_eq!(tray.icons.len(), initial_count + 1);
        assert!(id >= 100); // Third-party IDs start at 100

        tray.remove_icon(id);
        assert_eq!(tray.icons.len(), initial_count);
    }

    #[test]
    fn test_notification_badge() {
        let mut tray = SystemTray::new(0.0, 0.0);
        let id = tray.register_icon("App", 'A', "App");

        tray.set_badge(id, true);
        let icon = tray.icons.iter().find(|i| i.id == id);
        assert!(icon.is_some());
        assert!(icon.map_or(false, |i| i.has_notification_badge));

        tray.set_badge(id, false);
        let icon = tray.icons.iter().find(|i| i.id == id);
        assert!(icon.map_or(false, |i| !i.has_notification_badge));
    }

    #[test]
    fn test_tray_width_calculation() {
        let mut tray = SystemTray::new(0.0, 0.0);
        let initial_width = tray.tray_width;

        tray.register_icon("Extra", 'E', "Extra icon");
        assert!(tray.tray_width > initial_width);
        assert!((tray.tray_width - (initial_width + ICON_CELL_SIZE)).abs() < 0.01);
    }

    #[test]
    fn test_click_on_icon() {
        let mut tray = SystemTray::new(100.0, 500.0);
        // Click on the first icon (notifications, at x=100)
        let consumed = tray.handle_click(110.0, 520.0, MouseButton::Left);
        assert!(consumed);
        // Should have opened quick settings (notifications icon)
        assert_eq!(tray.active_popup, PopupType::QuickSettings);
    }

    #[test]
    fn test_click_toggles_popup() {
        let mut tray = SystemTray::new(100.0, 500.0);

        // First click opens volume popup (volume is the second icon at index 1)
        let volume_x = 100.0 + ICON_CELL_SIZE + 5.0;
        tray.handle_click(volume_x, 520.0, MouseButton::Left);
        assert_eq!(tray.active_popup, PopupType::Volume);

        // Second click on same icon closes it
        tray.handle_click(volume_x, 520.0, MouseButton::Left);
        assert_eq!(tray.active_popup, PopupType::None);
    }

    #[test]
    fn test_right_click_opens_context_menu() {
        let mut tray = SystemTray::new(100.0, 500.0);
        let consumed = tray.handle_click(110.0, 520.0, MouseButton::Right);
        assert!(consumed);
        assert!(matches!(tray.active_popup, PopupType::ContextMenu(_)));
    }

    #[test]
    fn test_click_outside_closes_popup() {
        let mut tray = SystemTray::new(100.0, 500.0);
        tray.active_popup = PopupType::Volume;

        // Click far outside the tray area
        let consumed = tray.handle_click(0.0, 0.0, MouseButton::Left);
        assert!(consumed); // Popup was open, so click was consumed to close it
        assert_eq!(tray.active_popup, PopupType::None);
    }

    #[test]
    fn test_click_outside_not_consumed_when_no_popup() {
        let mut tray = SystemTray::new(100.0, 500.0);
        let consumed = tray.handle_click(0.0, 0.0, MouseButton::Left);
        assert!(!consumed);
    }

    #[test]
    fn test_double_click_returns_icon_id() {
        let mut tray = SystemTray::new(100.0, 500.0);
        let result = tray.handle_double_click(110.0, 520.0);
        assert!(result.is_some());
        assert_eq!(result, Some(ICON_ID_NOTIFICATIONS));
    }

    #[test]
    fn test_double_click_outside_returns_none() {
        let mut tray = SystemTray::new(100.0, 500.0);
        let result = tray.handle_double_click(0.0, 0.0);
        assert!(result.is_none());
    }

    #[test]
    fn test_tick_advances_time() {
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.datetime.hour = 14;
        tray.datetime.minute = 59;
        tray.datetime.second = 55;

        tray.tick(10_000); // 10 seconds
        assert_eq!(tray.datetime.second, 5);
        assert_eq!(tray.datetime.minute, 0);
        assert_eq!(tray.datetime.hour, 15);
    }

    #[test]
    fn test_tick_updates_battery() {
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.battery.estimated_minutes = Some(100);

        tray.tick(60_000); // 1 minute
        assert_eq!(tray.battery.estimated_minutes, Some(99));
    }

    #[test]
    fn test_tick_zero_elapsed() {
        let mut tray = SystemTray::new(0.0, 0.0);
        let original_sec = tray.datetime.second;
        tray.tick(500); // Less than 1 second
        assert_eq!(tray.datetime.second, original_sec);
    }

    #[test]
    fn test_render_produces_commands() {
        let tray = SystemTray::new(0.0, 0.0);
        let frame = tray.render();
        // Should have at least the background rect + icon text for each visible icon
        assert!(frame.len() >= 7); // 1 bg + 6 icons
    }

    #[test]
    fn test_render_with_popup() {
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.active_popup = PopupType::Volume;
        let frame = tray.render();
        // Should have more commands than just the tray bar
        assert!(frame.len() > 10);
    }

    #[test]
    fn test_render_calendar() {
        let mut tray = SystemTray::new(0.0, 760.0);
        tray.active_popup = PopupType::Calendar;
        let frame = tray.render();
        // Calendar has many cells: 7 headers + up to 31 day cells + header
        assert!(frame.len() > 30);
    }

    #[test]
    fn test_render_quick_settings() {
        let mut tray = SystemTray::new(0.0, 760.0);
        tray.active_popup = PopupType::QuickSettings;
        let frame = tray.render();
        // Quick settings has toggles, sliders, labels
        assert!(frame.len() > 20);
    }

    #[test]
    fn test_render_power_menu() {
        let mut tray = SystemTray::new(0.0, 760.0);
        tray.active_popup = PopupType::PowerMenu;
        let frame = tray.render();
        // Power menu has 5 items + background
        assert!(frame.len() > 5);
    }

    #[test]
    fn test_render_network_popup() {
        let mut tray = SystemTray::new(0.0, 760.0);
        tray.active_popup = PopupType::Network;
        let frame = tray.render();
        assert!(frame.len() > 10);
    }

    #[test]
    fn test_datetime_time_str() {
        let dt = DateTime {
            year: 2026,
            month: 5,
            day: 17,
            hour: 9,
            minute: 5,
            second: 0,
            weekday: 0,
        };
        assert_eq!(dt.time_str(), "09:05");
    }

    #[test]
    fn test_datetime_date_str() {
        let dt = DateTime {
            year: 2026,
            month: 12,
            day: 25,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 4,
        };
        assert_eq!(dt.date_str(), "Dec 25, 2026");
    }

    #[test]
    fn test_days_in_month_leap_year() {
        let mut dt = DateTime::default();
        dt.year = 2024;
        dt.month = 2;
        assert_eq!(dt.days_in_month(), 29);

        dt.year = 2023;
        assert_eq!(dt.days_in_month(), 28);

        dt.year = 2000; // divisible by 400
        assert_eq!(dt.days_in_month(), 29);

        dt.year = 1900; // divisible by 100 but not 400
        assert_eq!(dt.days_in_month(), 28);
    }

    #[test]
    fn test_days_in_month_all_months() {
        let mut dt = DateTime::default();
        let expected = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        dt.year = 2025; // non-leap year
        for (i, &expected_days) in expected.iter().enumerate() {
            dt.month = (i + 1) as u8;
            assert_eq!(dt.days_in_month(), expected_days, "month {}", i + 1);
        }
    }

    #[test]
    fn test_first_weekday_of_month() {
        // May 2026 starts on Friday (5)
        let dt = DateTime {
            year: 2026,
            month: 5,
            day: 17,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 0,
        };
        assert_eq!(dt.first_weekday_of_month(), 5); // Friday
    }

    #[test]
    fn test_icon_visibility() {
        let mut tray = SystemTray::new(0.0, 0.0);
        let id = tray.register_icon("Hidden", 'H', "Hidden app");

        if let Some(icon) = tray.icons.iter_mut().find(|i| i.id == id) {
            icon.visible = false;
        }
        tray.recalculate_width();

        // Width should not include the hidden icon
        let visible_count = tray.icons.iter().filter(|i| i.visible).count() as f32;
        assert!((tray.tray_width - visible_count * ICON_CELL_SIZE).abs() < 0.01);
    }

    #[test]
    fn test_volume_mute_state() {
        let mut tray = SystemTray::new(0.0, 0.0);
        assert!(!tray.volume.muted);
        tray.volume.muted = true;
        // Render should still work
        let frame = tray.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_network_disconnected_state() {
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.network.connected = false;
        tray.active_popup = PopupType::Network;
        let frame = tray.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_quick_settings_all_toggles() {
        let mut tray = SystemTray::new(0.0, 760.0);
        tray.quick_settings.wifi_enabled = false;
        tray.quick_settings.bluetooth_enabled = false;
        tray.quick_settings.do_not_disturb = true;
        tray.quick_settings.night_light = true;
        tray.quick_settings.battery_saver = true;
        tray.quick_settings.airplane_mode = true;
        tray.active_popup = PopupType::QuickSettings;
        let frame = tray.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_context_menu_render() {
        let mut tray = SystemTray::new(0.0, 760.0);
        tray.active_popup = PopupType::ContextMenu(ICON_ID_VOLUME);
        let frame = tray.render();
        assert!(frame.len() > 3); // bg + 3 menu items
    }

    #[test]
    fn test_app_menu_render() {
        let mut tray = SystemTray::new(0.0, 760.0);
        let id = tray.register_icon("MyApp", 'M', "My Application");
        tray.active_popup = PopupType::AppMenu(id);
        let frame = tray.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_battery_saturating_subtraction() {
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.battery.estimated_minutes = Some(0);
        tray.tick(120_000); // 2 minutes
        // Should saturate at 0, not underflow
        assert_eq!(tray.battery.estimated_minutes, Some(0));
    }

    #[test]
    fn test_icon_id_generation_monotonic() {
        let mut tray = SystemTray::new(0.0, 0.0);
        let id1 = tray.register_icon("A", 'A', "A");
        let id2 = tray.register_icon("B", 'B', "B");
        let id3 = tray.register_icon("C", 'C', "C");
        assert!(id1 < id2);
        assert!(id2 < id3);
    }

    #[test]
    fn test_popup_position_alignment() {
        let tray = SystemTray::new(100.0, 760.0);
        // Volume is the second visible icon (index 1)
        let x = tray.popup_x_for_icon(ICON_ID_VOLUME);
        assert!((x - (100.0 + ICON_CELL_SIZE)).abs() < 0.01);
    }
}

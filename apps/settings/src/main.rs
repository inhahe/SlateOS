//! Slate OS Settings — Centralized System Configuration UI
//!
//! A Windows-style settings/control panel application providing unified
//! access to system configuration: display, sound, network, personalization,
//! accounts, privacy, accessibility, and system updates.
//!
//! Uses the guitk library for rendering. Dark theme (Catppuccin Mocha) by default.

mod associations;
mod remote;
mod snapshots;

use appearance::{AccentColor, AnimationSpeed, AppearanceFile, ThemeMode, TransparencyLevel};
#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[allow(unused_imports)]
use guitk::layout::{FlexAlign, FlexDirection, FlexWrap, Size};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::scroll_window;
#[allow(unused_imports)]
use guitk::style::{CornerRadii, Edges};
use guitk::text;

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

/// Background (base)
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
/// Surface layer 0
const COL_SURFACE0: Color = Color::from_hex(0x313244);
/// Surface layer 1 (sidebar)
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
/// Surface layer 2 (hover)
const COL_SURFACE2: Color = Color::from_hex(0x585B70);
/// Overlay 0
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
/// Main text
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Subtext (dimmer)
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
/// Subtext (dimmest)
const COL_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
/// Accent (blue)
const COL_ACCENT: Color = Color::from_hex(0x89B4FA);
/// Green (for toggles on)
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
/// Red (for destructive actions)
const COL_RED: Color = Color::from_hex(0xF38BA8);
/// Peach (for warnings)
const COL_PEACH: Color = Color::from_hex(0xFAB387);
/// Lavender
#[allow(dead_code)]
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
/// Teal
#[allow(dead_code)]
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
/// Mauve
#[allow(dead_code)]
const COL_MAUVE: Color = Color::from_hex(0xCBA6F7);
/// Crust (darkest)
const COL_CRUST: Color = Color::from_hex(0x11111B);

// ============================================================================
// Layout constants
// ============================================================================

const SIDEBAR_WIDTH: f32 = 260.0;
const HEADER_HEIGHT: f32 = 60.0;
const SEARCH_BAR_HEIGHT: f32 = 40.0;
const CATEGORY_ITEM_HEIGHT: f32 = 44.0;
const CONTENT_PADDING: f32 = 32.0;
const SECTION_SPACING: f32 = 24.0;

/// Width of the page tab drawn for `label`, including the 8 px padding each side.
///
/// The renderer and the click handler both need this, and when they each had
/// their own copy of the arithmetic a change to one silently moved the hit
/// targets off the tabs.
fn page_tab_width(label: &str) -> f32 {
    text::padded_width(label, 8.0, 13.0, FontWeightHint::Regular)
}
const ITEM_HEIGHT: f32 = 48.0;
const TOGGLE_WIDTH: f32 = 44.0;
const TOGGLE_HEIGHT: f32 = 24.0;
const SLIDER_WIDTH: f32 = 200.0;
const SLIDER_HEIGHT: f32 = 6.0;
const SLIDER_HANDLE_RADIUS: f32 = 8.0;
const DROPDOWN_WIDTH: f32 = 200.0;
const DROPDOWN_ITEM_HEIGHT: f32 = 36.0;
/// Padding above the first dropdown item and below the last.
const DROPDOWN_PADDING: f32 = 8.0;
/// How close an open dropdown may come to the window's edge.
const DROPDOWN_MARGIN: f32 = 8.0;
/// Vertical space a scrolled dropdown keeps for its "N more" line.
const LIST_MORE_HEIGHT: f32 = 16.0;
/// How many items one wheel notch moves an open dropdown.
const WHEEL_ROWS: isize = 3;

// ============================================================================
// Settings categories and pages
// ============================================================================

/// Top-level settings categories (sidebar items).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsCategory {
    System,
    Network,
    Personalization,
    Apps,
    Accounts,
    Privacy,
    Accessibility,
    Update,
}

impl SettingsCategory {
    const ALL: &[Self] = &[
        Self::System,
        Self::Network,
        Self::Personalization,
        Self::Apps,
        Self::Accounts,
        Self::Privacy,
        Self::Accessibility,
        Self::Update,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Network => "Network",
            Self::Personalization => "Personalization",
            Self::Apps => "Apps",
            Self::Accounts => "Accounts",
            Self::Privacy => "Privacy & Security",
            Self::Accessibility => "Accessibility",
            Self::Update => "Update & Recovery",
        }
    }

    fn icon_char(self) -> &'static str {
        match self {
            Self::System => "\u{2699}",           // gear
            Self::Network => "\u{1F310}",         // globe
            Self::Personalization => "\u{1F3A8}", // palette
            Self::Apps => "\u{1F4E6}",            // package
            Self::Accounts => "\u{1F464}",        // person
            Self::Privacy => "\u{1F512}",         // lock
            Self::Accessibility => "\u{267F}",    // accessibility
            Self::Update => "\u{1F504}",          // refresh
        }
    }

    fn default_page(self) -> SettingsPage {
        match self {
            Self::System => SettingsPage::Display,
            Self::Network => SettingsPage::NetworkStatus,
            Self::Personalization => SettingsPage::Themes,
            Self::Apps => SettingsPage::DefaultApps,
            Self::Accounts => SettingsPage::UserAccounts,
            Self::Privacy => SettingsPage::Permissions,
            Self::Accessibility => SettingsPage::Visual,
            Self::Update => SettingsPage::SystemUpdates,
        }
    }

    fn pages(self) -> &'static [SettingsPage] {
        match self {
            Self::System => &[
                SettingsPage::Display,
                SettingsPage::Sound,
                SettingsPage::Notifications,
                SettingsPage::Power,
            ],
            Self::Network => &[
                SettingsPage::NetworkStatus,
                SettingsPage::WiFi,
                SettingsPage::Ethernet,
                SettingsPage::VPN,
                SettingsPage::Proxy,
            ],
            Self::Personalization => &[
                SettingsPage::Themes,
                SettingsPage::Colors,
                SettingsPage::Wallpaper,
                SettingsPage::Fonts,
                SettingsPage::LockScreen,
            ],
            Self::Apps => &[
                SettingsPage::DefaultApps,
                SettingsPage::StartupApps,
                SettingsPage::InstalledApps,
            ],
            Self::Accounts => &[SettingsPage::UserAccounts, SettingsPage::LoginOptions],
            Self::Privacy => &[SettingsPage::Permissions, SettingsPage::Capabilities],
            Self::Accessibility => &[
                SettingsPage::Visual,
                SettingsPage::Audio,
                SettingsPage::Interaction,
            ],
            Self::Update => &[
                SettingsPage::SystemUpdates,
                SettingsPage::Recovery,
                SettingsPage::Snapshots,
            ],
        }
    }
}

/// Individual settings pages within categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    // System
    Display,
    Sound,
    Notifications,
    Power,
    // Network
    NetworkStatus,
    WiFi,
    Ethernet,
    VPN,
    Proxy,
    // Personalization
    Themes,
    Colors,
    Wallpaper,
    Fonts,
    LockScreen,
    // Apps
    DefaultApps,
    StartupApps,
    InstalledApps,
    // Accounts
    UserAccounts,
    LoginOptions,
    // Privacy
    Permissions,
    Capabilities,
    // Accessibility
    Visual,
    Audio,
    Interaction,
    // Update
    SystemUpdates,
    Recovery,
    Snapshots,
}

impl SettingsPage {
    fn label(self) -> &'static str {
        match self {
            Self::Display => "Display",
            Self::Sound => "Sound",
            Self::Notifications => "Notifications",
            Self::Power => "Power",
            Self::NetworkStatus => "Status",
            Self::WiFi => "Wi-Fi",
            Self::Ethernet => "Ethernet",
            Self::VPN => "VPN",
            Self::Proxy => "Proxy",
            Self::Themes => "Themes",
            Self::Colors => "Colors",
            Self::Wallpaper => "Wallpaper",
            Self::Fonts => "Fonts",
            Self::LockScreen => "Lock Screen",
            Self::DefaultApps => "Default Apps",
            Self::StartupApps => "Startup Apps",
            Self::InstalledApps => "Installed Apps",
            Self::UserAccounts => "User Accounts",
            Self::LoginOptions => "Login Options",
            Self::Permissions => "Permissions",
            Self::Capabilities => "Capabilities",
            Self::Visual => "Visual",
            Self::Audio => "Audio",
            Self::Interaction => "Interaction",
            Self::SystemUpdates => "System Updates",
            Self::Recovery => "Recovery",
            Self::Snapshots => "Snapshots",
        }
    }
}

// ============================================================================
// Display settings types
// ============================================================================

/// A screen resolution option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    fn label(self) -> String {
        format!("{} x {}", self.width, self.height)
    }
}

const RESOLUTIONS: &[Resolution] = &[
    Resolution::new(3840, 2160),
    Resolution::new(2560, 1440),
    Resolution::new(1920, 1080),
    Resolution::new(1680, 1050),
    Resolution::new(1600, 900),
    Resolution::new(1440, 900),
    Resolution::new(1366, 768),
    Resolution::new(1280, 720),
];

const REFRESH_RATES: &[u32] = &[30, 60, 75, 120, 144, 165, 240];

/// Scaling percentage options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalePercent {
    S100,
    S125,
    S150,
    S200,
}

impl ScalePercent {
    const ALL: &[Self] = &[Self::S100, Self::S125, Self::S150, Self::S200];

    fn label(self) -> &'static str {
        match self {
            Self::S100 => "100%",
            Self::S125 => "125%",
            Self::S150 => "150%",
            Self::S200 => "200%",
        }
    }
}

// ============================================================================
// Theme and personalization types
// ============================================================================
//
// There are none here any more. The theme mode, the accent palette, the
// transparency level and the animation speed all live in the `appearance`
// crate, because the desktop shell paints from the same values and one
// preference cannot have two owners: the copy this file used to hold listed
// twelve accents where the shell has fourteen, named them in a different
// order, and stored the user's choice as a *position in that list* — a format
// that would silently remap everyone's accent the first time a colour was
// inserted. See known-issues.md TD-THREE-INDEPENDENT-APPEARANCE-MODELS.

// ============================================================================
// Network types
// ============================================================================

/// Network adapter entry for the network page.
#[derive(Clone, Debug)]
pub struct NetworkAdapter {
    pub name: String,
    pub adapter_type: AdapterType,
    pub connected: bool,
    pub ip_address: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterType {
    Ethernet,
    WiFi,
    Loopback,
}

impl AdapterType {
    fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "Ethernet",
            Self::WiFi => "Wi-Fi",
            Self::Loopback => "Loopback",
        }
    }
}

/// IP configuration mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpConfigMode {
    Dhcp,
    Static,
}

impl IpConfigMode {
    fn label(self) -> &'static str {
        match self {
            Self::Dhcp => "DHCP (Automatic)",
            Self::Static => "Static",
        }
    }
}

// ============================================================================
// Sound types
// ============================================================================

/// Audio device for output/input selection.
#[derive(Clone, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
}

/// Per-application volume entry.
#[derive(Clone, Debug)]
pub struct AppVolume {
    pub app_name: String,
    pub volume: u8,
    pub muted: bool,
}

// ============================================================================
// Accounts types
// ============================================================================

/// Type of user account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountType {
    Admin,
    Standard,
    Child,
}

impl AccountType {
    fn label(self) -> &'static str {
        match self {
            Self::Admin => "Administrator",
            Self::Standard => "Standard",
            Self::Child => "Child",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Admin => COL_ACCENT,
            Self::Standard => COL_GREEN,
            Self::Child => COL_PEACH,
        }
    }
}

/// A user account entry.
#[derive(Clone, Debug)]
pub struct UserAccount {
    pub name: String,
    pub email: String,
    pub account_type: AccountType,
    pub login_count: u32,
    pub last_login: String,
    pub is_current: bool,
}

// ============================================================================
// Privacy types
// ============================================================================

/// Per-app permission entry.
#[derive(Clone, Debug)]
pub struct AppPermission {
    pub app_name: String,
    pub allowed: bool,
}

/// Diagnostic data collection level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticLevel {
    None,
    Basic,
    Full,
}

impl DiagnosticLevel {
    const ALL: &[Self] = &[Self::None, Self::Basic, Self::Full];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Basic => "Basic",
            Self::Full => "Full",
        }
    }
}

// ============================================================================
// Accessibility types
// ============================================================================

/// Color filter mode for visual accessibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorFilter {
    None,
    Grayscale,
    Deuteranopia,
    Protanopia,
    Tritanopia,
}

impl ColorFilter {
    const ALL: &[Self] = &[
        Self::None,
        Self::Grayscale,
        Self::Deuteranopia,
        Self::Protanopia,
        Self::Tritanopia,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Grayscale => "Grayscale",
            Self::Deuteranopia => "Deuteranopia",
            Self::Protanopia => "Protanopia",
            Self::Tritanopia => "Tritanopia",
        }
    }
}

/// Cursor size option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorSize {
    Small,
    Medium,
    Large,
    XLarge,
}

impl CursorSize {
    const ALL: &[Self] = &[Self::Small, Self::Medium, Self::Large, Self::XLarge];

    fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::XLarge => "Extra Large",
        }
    }
}

/// Narrator verbosity level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NarratorVerbosity {
    Low,
    Medium,
    High,
}

impl NarratorVerbosity {
    const ALL: &[Self] = &[Self::Low, Self::Medium, Self::High];

    fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

// ============================================================================
// Update types
// ============================================================================

/// Status of an installed update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    Installed,
    Failed,
    Pending,
}

impl UpdateStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Installed => "Installed",
            Self::Failed => "Failed",
            Self::Pending => "Pending",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Installed => COL_GREEN,
            Self::Failed => COL_RED,
            Self::Pending => COL_PEACH,
        }
    }
}

/// A historical update entry.
#[derive(Clone, Debug)]
pub struct UpdateEntry {
    pub date: String,
    pub kb_number: String,
    pub description: String,
    pub status: UpdateStatus,
}

// ============================================================================
// Main application state
// ============================================================================

/// Complete application state for the settings UI.
pub struct SettingsState {
    // Navigation
    pub current_category: SettingsCategory,
    pub current_page: SettingsPage,
    pub search_query: String,
    pub search_focused: bool,
    pub sidebar_hovered: Option<usize>,

    // Window dimensions
    pub window_width: f32,
    pub window_height: f32,

    // Display settings
    pub resolution_index: usize,
    pub refresh_rate_index: usize,
    pub scale: ScalePercent,
    pub night_light_enabled: bool,
    pub night_light_temperature: f32, // 0.0 (warm) to 1.0 (cool)
    pub monitor_count: u8,

    // Sound settings
    pub output_devices: Vec<AudioDevice>,
    pub output_device_index: usize,
    pub output_volume: u8,
    pub output_muted: bool,
    pub input_devices: Vec<AudioDevice>,
    pub input_device_index: usize,
    pub input_volume: u8,
    pub system_sounds_enabled: bool,
    pub app_volumes: Vec<AppVolume>,

    // Personalization — the whole shared model, not a subset, and carrying
    // the file it was read from. These pages edit only some of the settings
    // today, but the file is saved whole: writing back a partial copy would
    // erase the fields the desktop's own appearance panel set (fonts,
    // cursors, corners) along with the user's comments.
    pub appearance: AppearanceFile,

    // Network
    pub adapters: Vec<NetworkAdapter>,
    pub selected_adapter: usize,
    pub ip_config_mode: IpConfigMode,
    pub static_ip: String,
    pub static_gateway: String,
    pub dns_primary: String,
    pub dns_secondary: String,
    pub proxy_enabled: bool,
    pub proxy_address: String,
    pub proxy_port: String,

    // Accounts settings
    pub user_accounts: Vec<UserAccount>,
    pub selected_account: usize,
    pub auto_login_enabled: bool,

    // Privacy settings
    pub location_enabled: bool,
    pub location_apps: Vec<AppPermission>,
    pub camera_enabled: bool,
    pub camera_apps: Vec<AppPermission>,
    pub microphone_enabled: bool,
    pub microphone_apps: Vec<AppPermission>,
    pub background_apps: Vec<AppPermission>,
    pub diagnostic_level: DiagnosticLevel,

    // Accessibility settings
    pub text_size_percent: u16, // 50-250
    pub high_contrast: bool,
    pub cursor_size: CursorSize,
    pub reduce_animations: bool,
    pub color_filter: ColorFilter,
    pub reduce_transparency: bool,
    pub mono_audio: bool,
    pub visual_alerts: bool,
    pub sticky_keys: bool,
    pub filter_keys: bool,
    pub toggle_keys: bool,
    pub onscreen_keyboard: bool,
    pub pointer_size: u8, // 1-5
    pub mouse_keys: bool,
    pub narrator_enabled: bool,
    pub narrator_rate: f32, // 0.0-1.0
    pub narrator_verbosity: NarratorVerbosity,

    // Update settings
    pub os_version: String,
    pub update_history: Vec<UpdateEntry>,
    pub auto_update_enabled: bool,
    pub active_hours_start: u8, // 0-23
    pub active_hours_end: u8,   // 0-23
    pub defer_feature_days: u16,
    pub defer_quality_days: u16,
    pub checking_for_updates: bool,

    // Dropdown state
    pub open_dropdown: Option<DropdownId>,
    /// Index of the first item drawn in the open dropdown.
    ///
    /// A request rather than an index: an offset left over from a longer list
    /// shows the last page instead of a blank popup, because
    /// [`scroll_window::visible`] clamps the *result* and leaves this alone.
    pub dropdown_scroll: usize,
}

/// Where an open dropdown's popup is, and which of its items are on screen.
///
/// Computed once by [`SettingsState::dropdown_layout`] and used by both the
/// renderer and the click handler. When only the renderer knew, the click
/// handler had nothing to test a click against, and the comment standing where
/// the hit-test should have been read:
///
/// ```text
/// // For simplicity, any click closes the dropdown
/// // A real implementation would check if click is inside the dropdown
/// ```
///
/// which is to say the dropdowns could be opened but not used —
/// [`SettingsState::apply_dropdown_selection`] was correct, complete, and
/// reachable only from the tests.
#[derive(Clone, Debug, PartialEq)]
pub struct DropdownLayout {
    /// Every item in the dropdown, in order, whether on screen or not.
    pub items: Vec<String>,
    /// Index of the currently-chosen item, as an index into `items`.
    pub selected: usize,
    /// Left edge of the popup.
    pub x: f32,
    /// Top edge of the popup, after being pulled up to fit in the window.
    pub y: f32,
    /// Popup width.
    pub width: f32,
    /// Popup height, never more than the window can hold.
    pub height: f32,
    /// The items actually drawn, after clamping to the popup's height.
    pub window: scroll_window::Rows,
}

impl DropdownLayout {
    /// Y of the top of the `row`-th item *drawn* (not the `row`-th item).
    #[must_use]
    fn row_top(&self, row: usize) -> f32 {
        self.y + DROPDOWN_PADDING / 2.0 + (row as f32) * DROPDOWN_ITEM_HEIGHT
    }

    /// The item under `(mx, my)`, as an index into [`Self::items`].
    ///
    /// `None` for a click anywhere else — including on the popup's padding and
    /// on its "N more" line, neither of which names an item. The inverse of
    /// [`Self::row_top`] by construction: both are here so that a change to
    /// one is a change to the other.
    #[must_use]
    pub fn item_at(&self, mx: f32, my: f32) -> Option<usize> {
        if mx < self.x || mx >= self.x + self.width {
            return None;
        }
        let top = self.row_top(0);
        if my < top {
            return None;
        }
        // Truncating rather than rounding: a click 35.9px below the first row's
        // top is on the first row, not adjacent to the second.
        let row = ((my - top) / DROPDOWN_ITEM_HEIGHT) as usize;
        if row >= self.window.count {
            return None;
        }
        Some(self.window.start.saturating_add(row))
    }

    /// How many items are not on screen.
    #[must_use]
    pub fn hidden(&self) -> usize {
        self.items.len().saturating_sub(self.window.count)
    }
}

/// Identifies which dropdown is currently open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropdownId {
    Resolution,
    RefreshRate,
    Scale,
    OutputDevice,
    InputDevice,
    IpConfig,
    DiagnosticLevel,
    ColorFilter,
    CursorSize,
    NarratorVerbosity,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsState {
    /// Read the user's saved appearance settings over the defaults.
    ///
    /// Separate from [`new`](Self::new) so the constructor stays free of I/O:
    /// a `new()` that read `$HOME` would make every test's result depend on
    /// the machine running it.
    pub fn load_appearance(&mut self) {
        self.appearance = AppearanceFile::load();
    }

    /// Write the appearance settings back to `appearance.yaml`.
    ///
    /// Called after each change rather than from a Save button, because the
    /// Personalization pages apply immediately — clicking an accent recolours
    /// the preview, and a user who saw the change happen has no reason to
    /// expect it to need confirming.
    ///
    /// A failed write is reported to stderr and otherwise dropped: the choice
    /// is in effect in this process either way, and there is nowhere in this
    /// UI to surface an error yet. When there is a status line, this is the
    /// call site that should feed it.
    fn save_appearance(&mut self) {
        if let Err(err) = self.appearance.save() {
            eprintln!("settings: could not save appearance.yaml: {err}");
        }
    }

    /// Create a new settings state with sensible defaults.
    pub fn new() -> Self {
        Self {
            current_category: SettingsCategory::System,
            current_page: SettingsPage::Display,
            search_query: String::new(),
            search_focused: false,
            sidebar_hovered: None,

            window_width: 1200.0,
            window_height: 800.0,

            // Display defaults
            resolution_index: 2,   // 1920x1080
            refresh_rate_index: 1, // 60 Hz
            scale: ScalePercent::S100,
            night_light_enabled: false,
            night_light_temperature: 0.5,
            monitor_count: 1,

            // Sound defaults
            output_devices: vec![
                AudioDevice {
                    name: "Speakers (Built-in)".into(),
                    is_default: true,
                },
                AudioDevice {
                    name: "HDMI Audio Output".into(),
                    is_default: false,
                },
                AudioDevice {
                    name: "Bluetooth Headphones".into(),
                    is_default: false,
                },
            ],
            output_device_index: 0,
            output_volume: 75,
            output_muted: false,
            input_devices: vec![
                AudioDevice {
                    name: "Microphone (Built-in)".into(),
                    is_default: true,
                },
                AudioDevice {
                    name: "USB Microphone".into(),
                    is_default: false,
                },
            ],
            input_device_index: 0,
            input_volume: 80,
            system_sounds_enabled: true,
            app_volumes: vec![
                AppVolume {
                    app_name: "System".into(),
                    volume: 100,
                    muted: false,
                },
                AppVolume {
                    app_name: "Browser".into(),
                    volume: 85,
                    muted: false,
                },
                AppVolume {
                    app_name: "Music Player".into(),
                    volume: 60,
                    muted: false,
                },
                AppVolume {
                    app_name: "Video Player".into(),
                    volume: 90,
                    muted: false,
                },
            ],

            // Personalization defaults, not a read of the configuration
            // file: a constructor that touched $HOME would make every test's
            // result depend on the machine running it.
            // `load_appearance()` does the I/O, from `main`.
            appearance: AppearanceFile::new(),

            // Network defaults
            adapters: vec![
                NetworkAdapter {
                    name: "eth0".into(),
                    adapter_type: AdapterType::Ethernet,
                    connected: true,
                    ip_address: "192.168.1.100".into(),
                },
                NetworkAdapter {
                    name: "wlan0".into(),
                    adapter_type: AdapterType::WiFi,
                    connected: false,
                    ip_address: String::new(),
                },
                NetworkAdapter {
                    name: "lo".into(),
                    adapter_type: AdapterType::Loopback,
                    connected: true,
                    ip_address: "127.0.0.1".into(),
                },
            ],
            selected_adapter: 0,
            ip_config_mode: IpConfigMode::Dhcp,
            static_ip: "192.168.1.100".into(),
            static_gateway: "192.168.1.1".into(),
            dns_primary: "1.1.1.1".into(),
            dns_secondary: "8.8.8.8".into(),
            proxy_enabled: false,
            proxy_address: String::new(),
            proxy_port: String::new(),

            // Accounts defaults
            user_accounts: vec![
                UserAccount {
                    name: "Alice".into(),
                    email: "alice@example.com".into(),
                    account_type: AccountType::Admin,
                    login_count: 142,
                    last_login: "2026-05-17 09:34".into(),
                    is_current: true,
                },
                UserAccount {
                    name: "Bob".into(),
                    email: "bob@example.com".into(),
                    account_type: AccountType::Standard,
                    login_count: 56,
                    last_login: "2026-05-16 18:20".into(),
                    is_current: false,
                },
                UserAccount {
                    name: "Charlie".into(),
                    email: "charlie@example.com".into(),
                    account_type: AccountType::Child,
                    login_count: 23,
                    last_login: "2026-05-15 14:05".into(),
                    is_current: false,
                },
            ],
            selected_account: 0,
            auto_login_enabled: false,

            // Privacy defaults
            location_enabled: true,
            location_apps: vec![
                AppPermission {
                    app_name: "Maps".into(),
                    allowed: true,
                },
                AppPermission {
                    app_name: "Weather".into(),
                    allowed: true,
                },
                AppPermission {
                    app_name: "Camera".into(),
                    allowed: false,
                },
                AppPermission {
                    app_name: "Browser".into(),
                    allowed: true,
                },
            ],
            camera_enabled: true,
            camera_apps: vec![
                AppPermission {
                    app_name: "Video Chat".into(),
                    allowed: true,
                },
                AppPermission {
                    app_name: "Browser".into(),
                    allowed: true,
                },
                AppPermission {
                    app_name: "Social Media".into(),
                    allowed: false,
                },
            ],
            microphone_enabled: true,
            microphone_apps: vec![
                AppPermission {
                    app_name: "Video Chat".into(),
                    allowed: true,
                },
                AppPermission {
                    app_name: "Voice Recorder".into(),
                    allowed: true,
                },
                AppPermission {
                    app_name: "Browser".into(),
                    allowed: false,
                },
            ],
            background_apps: vec![
                AppPermission {
                    app_name: "Email".into(),
                    allowed: true,
                },
                AppPermission {
                    app_name: "Music Player".into(),
                    allowed: true,
                },
                AppPermission {
                    app_name: "Updater".into(),
                    allowed: true,
                },
                AppPermission {
                    app_name: "Social Media".into(),
                    allowed: false,
                },
                AppPermission {
                    app_name: "News Reader".into(),
                    allowed: false,
                },
            ],
            diagnostic_level: DiagnosticLevel::Basic,

            // Accessibility defaults
            text_size_percent: 100,
            high_contrast: false,
            cursor_size: CursorSize::Small,
            reduce_animations: false,
            color_filter: ColorFilter::None,
            reduce_transparency: false,
            mono_audio: false,
            visual_alerts: false,
            sticky_keys: false,
            filter_keys: false,
            toggle_keys: false,
            onscreen_keyboard: false,
            pointer_size: 1,
            mouse_keys: false,
            narrator_enabled: false,
            narrator_rate: 0.5,
            narrator_verbosity: NarratorVerbosity::Medium,

            // Update defaults
            os_version: "Slate OS 1.0.0 Build 2600".into(),
            update_history: vec![
                UpdateEntry {
                    date: "2026-05-15".into(),
                    kb_number: "KB5032100".into(),
                    description: "Security update for kernel".into(),
                    status: UpdateStatus::Installed,
                },
                UpdateEntry {
                    date: "2026-05-10".into(),
                    kb_number: "KB5031980".into(),
                    description: "Cumulative update for .NET runtime".into(),
                    status: UpdateStatus::Installed,
                },
                UpdateEntry {
                    date: "2026-05-08".into(),
                    kb_number: "KB5031875".into(),
                    description: "Driver update for GPU".into(),
                    status: UpdateStatus::Failed,
                },
                UpdateEntry {
                    date: "2026-05-01".into(),
                    kb_number: "KB5031700".into(),
                    description: "Feature update: compositor improvements".into(),
                    status: UpdateStatus::Installed,
                },
            ],
            auto_update_enabled: true,
            active_hours_start: 8,
            active_hours_end: 22,
            defer_feature_days: 0,
            defer_quality_days: 0,
            checking_for_updates: false,

            // Dropdown state
            open_dropdown: None,
            dropdown_scroll: 0,
        }
    }
}

// ============================================================================
// Rendering helpers
// ============================================================================

/// Push a rounded rectangle fill command.
fn fill_rounded(tree: &mut RenderTree, x: f32, y: f32, w: f32, h: f32, color: Color, radius: f32) {
    tree.fill_rounded_rect(x, y, w, h, color, CornerRadii::all(radius));
}

/// Push a text command with bold weight.
fn text_bold(tree: &mut RenderTree, x: f32, y: f32, content: &str, color: Color, size: f32) {
    tree.push(RenderCommand::Text {
        x,
        y,
        text: content.to_string(),
        color,
        font_size: size,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// Push a text command with regular weight and optional max_width.
fn text_clipped(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    content: &str,
    color: Color,
    size: f32,
    max_width: f32,
) {
    tree.push(RenderCommand::Text {
        x,
        y,
        text: content.to_string(),
        color,
        font_size: size,
        font_weight: FontWeightHint::Regular,
        max_width: Some(max_width),
        overflow: TextOverflow::Ellipsis,
    });
}

/// Render a toggle switch (on/off).
fn render_toggle(tree: &mut RenderTree, x: f32, y: f32, enabled: bool) {
    let track_color = if enabled { COL_GREEN } else { COL_SURFACE2 };
    fill_rounded(
        tree,
        x,
        y,
        TOGGLE_WIDTH,
        TOGGLE_HEIGHT,
        track_color,
        TOGGLE_HEIGHT / 2.0,
    );

    // Handle circle
    let handle_margin = 3.0;
    let handle_diameter = TOGGLE_HEIGHT - handle_margin * 2.0;
    let handle_x = if enabled {
        x + TOGGLE_WIDTH - handle_diameter - handle_margin
    } else {
        x + handle_margin
    };
    let handle_y = y + handle_margin;
    fill_rounded(
        tree,
        handle_x,
        handle_y,
        handle_diameter,
        handle_diameter,
        COL_TEXT,
        handle_diameter / 2.0,
    );
}

/// Render a horizontal slider at the given position.
/// Returns nothing; slider_value should be 0.0..=1.0.
fn render_slider(tree: &mut RenderTree, x: f32, y: f32, value: f32) {
    let track_y = y + (ITEM_HEIGHT - SLIDER_HEIGHT) / 2.0;

    // Track background
    fill_rounded(
        tree,
        x,
        track_y,
        SLIDER_WIDTH,
        SLIDER_HEIGHT,
        COL_SURFACE2,
        SLIDER_HEIGHT / 2.0,
    );

    // Filled portion
    let fill_width = SLIDER_WIDTH * value.clamp(0.0, 1.0);
    if fill_width > 0.5 {
        fill_rounded(
            tree,
            x,
            track_y,
            fill_width,
            SLIDER_HEIGHT,
            COL_ACCENT,
            SLIDER_HEIGHT / 2.0,
        );
    }

    // Handle
    let handle_x = x + fill_width - SLIDER_HANDLE_RADIUS;
    let handle_y = track_y + SLIDER_HEIGHT / 2.0 - SLIDER_HANDLE_RADIUS;
    fill_rounded(
        tree,
        handle_x,
        handle_y,
        SLIDER_HANDLE_RADIUS * 2.0,
        SLIDER_HANDLE_RADIUS * 2.0,
        COL_TEXT,
        SLIDER_HANDLE_RADIUS,
    );
}

/// Render a labeled setting row with a label on the left and value widget on the right.
/// Returns the y offset for the next item.
fn render_setting_row(
    tree: &mut RenderTree,
    x: f32,
    y: f32,
    label: &str,
    content_width: f32,
) -> f32 {
    tree.text(x, y + 14.0, label, COL_TEXT, 14.0);
    let _ = content_width; // used by caller to position right-side widget
    y + ITEM_HEIGHT
}

/// Render a dropdown button (closed state).
fn render_dropdown_button(tree: &mut RenderTree, x: f32, y: f32, label: &str, width: f32) {
    fill_rounded(tree, x, y + 6.0, width, 32.0, COL_SURFACE1, 6.0);
    tree.push(RenderCommand::StrokeRect {
        x,
        y: y + 6.0,
        width,
        height: 32.0,
        color: COL_OVERLAY0,
        line_width: 1.0,
        corner_radii: CornerRadii::all(6.0),
    });
    text_clipped(
        tree,
        x + 10.0,
        y + 16.0,
        label,
        COL_TEXT,
        13.0,
        width - 30.0,
    );
    // Down arrow indicator
    tree.text(x + width - 20.0, y + 16.0, "\u{25BC}", COL_SUBTEXT0, 10.0);
}

/// Render a section header (bold text with divider line below).
fn render_section_header(tree: &mut RenderTree, x: f32, y: f32, title: &str) -> f32 {
    text_bold(tree, x, y, title, COL_TEXT, 16.0);
    let line_y = y + 24.0;
    tree.push(RenderCommand::Line {
        x1: x,
        y1: line_y,
        x2: x + 600.0,
        y2: line_y,
        color: COL_SURFACE1,
        width: 1.0,
    });
    line_y + 12.0
}

// --- Pill rows -------------------------------------------------------------
//
// A row of small selectable buttons, used wherever a setting is one choice
// out of a handful (theme transparency, animation speed). The renderer and
// the click handler share this geometry rather than each carrying a copy of
// the numbers: a second copy is how a button ends up drawn in one place and
// clicked in another, and the drift is invisible until someone tries it.

/// Width of one pill.
const PILL_WIDTH: f32 = 72.0;
/// Distance from one pill's left edge to the next.
const PILL_PITCH: f32 = 80.0;
const PILL_HEIGHT: f32 = 28.0;
/// How far below the row's top edge the pills sit.
const PILL_INSET_Y: f32 = 8.0;
/// Distance from the content's left edge to the first pill — the same right
/// column the toggles and dropdowns on other rows use.
const PILL_ROW_X: f32 = 350.0;

/// Draw a row of pills, the selected one filled with the accent color.
fn render_pill_row(tree: &mut RenderTree, x: f32, y: f32, items: &[(&str, bool)]) {
    for (idx, (label, active)) in items.iter().enumerate() {
        let px = x + (idx as f32) * PILL_PITCH;
        let (bg, fg) = if *active {
            (COL_ACCENT, COL_CRUST)
        } else {
            (COL_SURFACE1, COL_SUBTEXT0)
        };
        fill_rounded(tree, px, y + PILL_INSET_Y, PILL_WIDTH, PILL_HEIGHT, bg, 6.0);
        tree.text(px + 10.0, y + PILL_INSET_Y + 7.0, label, fg, 12.0);
    }
}

/// Which pill of a row drawn at (`x`, `y`) the click landed on, if any.
fn pill_row_hit(mx: f32, my: f32, x: f32, y: f32, count: usize) -> Option<usize> {
    if my < y + PILL_INSET_Y || my >= y + PILL_INSET_Y + PILL_HEIGHT {
        return None;
    }
    (0..count).find(|idx| {
        let px = x + (*idx as f32) * PILL_PITCH;
        mx >= px && mx < px + PILL_WIDTH
    })
}

// ============================================================================
// Page renderers
// ============================================================================

impl SettingsState {
    /// Render the complete settings UI frame.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background
        tree.fill_rect(0.0, 0.0, self.window_width, self.window_height, COL_BASE);

        // Sidebar
        self.render_sidebar(&mut tree);

        // Content area
        let content_x = SIDEBAR_WIDTH;
        let content_w = self.window_width - SIDEBAR_WIDTH;
        tree.clip(content_x, 0.0, content_w, self.window_height);

        // Page header with breadcrumb
        self.render_page_header(&mut tree, content_x);

        // Page content
        let page_y = HEADER_HEIGHT + 8.0;
        tree.clip(content_x, page_y, content_w, self.window_height - page_y);
        self.render_current_page(&mut tree, content_x + CONTENT_PADDING, page_y);
        tree.unclip();

        tree.unclip();

        // Dropdown overlay (rendered on top of everything)
        if self.open_dropdown.is_some() {
            self.render_open_dropdown(&mut tree);
        }

        tree
    }

    /// Render the left sidebar with search bar and category list.
    fn render_sidebar(&self, tree: &mut RenderTree) {
        // Sidebar background
        tree.fill_rect(0.0, 0.0, SIDEBAR_WIDTH, self.window_height, COL_CRUST);

        // App title
        text_bold(tree, 20.0, 18.0, "Settings", COL_TEXT, 20.0);

        // Search bar
        let search_y = HEADER_HEIGHT;
        fill_rounded(
            tree,
            12.0,
            search_y,
            SIDEBAR_WIDTH - 24.0,
            SEARCH_BAR_HEIGHT,
            COL_SURFACE0,
            8.0,
        );
        if self.search_query.is_empty() {
            tree.text(
                24.0,
                search_y + 12.0,
                "\u{1F50D} Search settings...",
                COL_OVERLAY0,
                13.0,
            );
        } else {
            text_clipped(
                tree,
                24.0,
                search_y + 12.0,
                &self.search_query,
                COL_TEXT,
                13.0,
                SIDEBAR_WIDTH - 52.0,
            );
        }

        // Category list
        let list_y = search_y + SEARCH_BAR_HEIGHT + 16.0;
        for (idx, category) in SettingsCategory::ALL.iter().enumerate() {
            let item_y = list_y + (idx as f32) * CATEGORY_ITEM_HEIGHT;
            let is_selected = *category == self.current_category;
            let is_hovered = self.sidebar_hovered == Some(idx);

            // Background highlight
            if is_selected {
                fill_rounded(
                    tree,
                    8.0,
                    item_y,
                    SIDEBAR_WIDTH - 16.0,
                    CATEGORY_ITEM_HEIGHT - 4.0,
                    COL_SURFACE0,
                    8.0,
                );
                // Accent bar on the left
                fill_rounded(
                    tree,
                    4.0,
                    item_y + 8.0,
                    3.0,
                    CATEGORY_ITEM_HEIGHT - 20.0,
                    COL_ACCENT,
                    2.0,
                );
            } else if is_hovered {
                fill_rounded(
                    tree,
                    8.0,
                    item_y,
                    SIDEBAR_WIDTH - 16.0,
                    CATEGORY_ITEM_HEIGHT - 4.0,
                    COL_SURFACE1,
                    8.0,
                );
            }

            // Icon
            tree.text(
                24.0,
                item_y + 12.0,
                category.icon_char(),
                COL_SUBTEXT0,
                16.0,
            );

            // Label
            let label_color = if is_selected { COL_TEXT } else { COL_SUBTEXT1 };
            tree.text(52.0, item_y + 14.0, category.label(), label_color, 14.0);
        }
    }

    /// Render the page header with breadcrumb navigation.
    fn render_page_header(&self, tree: &mut RenderTree, content_x: f32) {
        // Header background
        tree.fill_rect(
            content_x,
            0.0,
            self.window_width - content_x,
            HEADER_HEIGHT,
            COL_BASE,
        );

        // Breadcrumb: Category > Page
        let breadcrumb = format!(
            "{}  \u{203A}  {}",
            self.current_category.label(),
            self.current_page.label()
        );
        text_bold(
            tree,
            content_x + CONTENT_PADDING,
            22.0,
            &breadcrumb,
            COL_TEXT,
            18.0,
        );

        // Sub-page tabs
        let pages = self.current_category.pages();
        let tab_y = HEADER_HEIGHT - 20.0;
        let mut tab_x = content_x + CONTENT_PADDING;
        for page in pages {
            let is_active = *page == self.current_page;
            let label = page.label();
            let tab_width = page_tab_width(label);

            if is_active {
                // Active tab underline
                tree.push(RenderCommand::Line {
                    x1: tab_x,
                    y1: tab_y + 16.0,
                    x2: tab_x + tab_width,
                    y2: tab_y + 16.0,
                    color: COL_ACCENT,
                    width: 2.0,
                });
                tree.text(tab_x + 8.0, tab_y, label, COL_ACCENT, 13.0);
            } else {
                tree.text(tab_x + 8.0, tab_y, label, COL_SUBTEXT0, 13.0);
            }
            tab_x += tab_width + 8.0;
        }
    }

    /// Dispatch to the correct page renderer.
    fn render_current_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        match self.current_page {
            SettingsPage::Display => self.render_display_page(tree, x, start_y),
            SettingsPage::Sound => self.render_sound_page(tree, x, start_y),
            SettingsPage::Themes => self.render_themes_page(tree, x, start_y),
            SettingsPage::Colors => self.render_colors_page(tree, x, start_y),
            SettingsPage::NetworkStatus => self.render_network_page(tree, x, start_y),
            SettingsPage::Proxy => self.render_proxy_page(tree, x, start_y),
            SettingsPage::UserAccounts | SettingsPage::LoginOptions => {
                self.render_accounts_page(tree, x, start_y);
            }
            SettingsPage::Permissions | SettingsPage::Capabilities => {
                self.render_privacy_page(tree, x, start_y);
            }
            SettingsPage::Visual | SettingsPage::Audio | SettingsPage::Interaction => {
                self.render_accessibility_page(tree, x, start_y);
            }
            SettingsPage::SystemUpdates | SettingsPage::Recovery | SettingsPage::Snapshots => {
                self.render_update_page(tree, x, start_y);
            }
            _ => self.render_placeholder_page(tree, x, start_y),
        }
    }

    // --- Display page ---

    fn render_display_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;
        let right_x = x + 350.0;

        // Monitor preview
        y = render_section_header(tree, x, y, "Monitor Arrangement");
        self.render_monitor_preview(tree, x, y);
        y += 120.0 + SECTION_SPACING;

        // Resolution
        y = render_section_header(tree, x, y, "Resolution & Scaling");
        render_setting_row(tree, x, y, "Resolution", 0.0);
        let res = RESOLUTIONS.get(self.resolution_index);
        let res_label = res
            .map(|r| r.label())
            .unwrap_or_else(|| "Unknown".to_string());
        render_dropdown_button(tree, right_x, y, &res_label, DROPDOWN_WIDTH);
        y += ITEM_HEIGHT;

        // Refresh rate
        render_setting_row(tree, x, y, "Refresh Rate", 0.0);
        let rate = REFRESH_RATES
            .get(self.refresh_rate_index)
            .copied()
            .unwrap_or(60);
        let rate_label = format!("{} Hz", rate);
        render_dropdown_button(tree, right_x, y, &rate_label, DROPDOWN_WIDTH);
        y += ITEM_HEIGHT;

        // Scaling
        render_setting_row(tree, x, y, "Display Scaling", 0.0);
        render_dropdown_button(tree, right_x, y, self.scale.label(), DROPDOWN_WIDTH);
        y += ITEM_HEIGHT + SECTION_SPACING;

        // Night light
        y = render_section_header(tree, x, y, "Night Light");
        render_setting_row(tree, x, y, "Night Light", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.night_light_enabled);
        y += ITEM_HEIGHT;

        if self.night_light_enabled {
            render_setting_row(tree, x, y, "Color Temperature", 0.0);
            render_slider(tree, right_x, y, self.night_light_temperature);
            y += ITEM_HEIGHT;

            // Temperature labels
            tree.text(right_x, y, "Warm", COL_PEACH, 11.0);
            tree.text(right_x + SLIDER_WIDTH - 30.0, y, "Cool", COL_ACCENT, 11.0);
        }
    }

    /// Render a simplified monitor arrangement preview.
    fn render_monitor_preview(&self, tree: &mut RenderTree, x: f32, y: f32) {
        let preview_bg_w = 500.0;
        let preview_bg_h = 110.0;
        fill_rounded(tree, x, y, preview_bg_w, preview_bg_h, COL_SURFACE0, 8.0);

        let monitor_w = 100.0;
        let monitor_h = 70.0;
        let spacing = 20.0;
        let total_w =
            (self.monitor_count as f32) * monitor_w + ((self.monitor_count as f32) - 1.0) * spacing;
        let start_x = x + (preview_bg_w - total_w) / 2.0;
        let start_y = y + (preview_bg_h - monitor_h) / 2.0;

        for i in 0..self.monitor_count {
            let mx = start_x + (i as f32) * (monitor_w + spacing);
            // Monitor bezel
            fill_rounded(tree, mx, start_y, monitor_w, monitor_h, COL_SURFACE2, 4.0);
            // Screen area
            fill_rounded(
                tree,
                mx + 4.0,
                start_y + 4.0,
                monitor_w - 8.0,
                monitor_h - 12.0,
                COL_ACCENT,
                2.0,
            );
            // Monitor number
            let num_label = format!("{}", i + 1);
            tree.text(
                mx + monitor_w / 2.0 - 4.0,
                start_y + monitor_h / 2.0 - 12.0,
                &num_label,
                COL_BASE,
                16.0,
            );
        }
    }

    // --- Sound page ---

    fn render_sound_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;
        let right_x = x + 350.0;

        // Output section
        y = render_section_header(tree, x, y, "Output");
        render_setting_row(tree, x, y, "Output Device", 0.0);
        let output_name = self
            .output_devices
            .get(self.output_device_index)
            .map(|d| d.name.as_str())
            .unwrap_or("None");
        render_dropdown_button(tree, right_x, y, output_name, DROPDOWN_WIDTH);
        y += ITEM_HEIGHT;

        // Volume slider
        render_setting_row(tree, x, y, "Volume", 0.0);
        let vol_norm = self.output_volume as f32 / 100.0;
        render_slider(tree, right_x, y, vol_norm);
        // Volume percentage label
        let vol_label = format!("{}%", self.output_volume);
        tree.text(
            right_x + SLIDER_WIDTH + 12.0,
            y + 14.0,
            &vol_label,
            COL_SUBTEXT0,
            12.0,
        );
        y += ITEM_HEIGHT;

        // Mute toggle
        render_setting_row(tree, x, y, "Mute", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.output_muted);
        y += ITEM_HEIGHT + SECTION_SPACING;

        // Input section
        y = render_section_header(tree, x, y, "Input");
        render_setting_row(tree, x, y, "Input Device", 0.0);
        let input_name = self
            .input_devices
            .get(self.input_device_index)
            .map(|d| d.name.as_str())
            .unwrap_or("None");
        render_dropdown_button(tree, right_x, y, input_name, DROPDOWN_WIDTH);
        y += ITEM_HEIGHT;

        render_setting_row(tree, x, y, "Input Volume", 0.0);
        let in_vol_norm = self.input_volume as f32 / 100.0;
        render_slider(tree, right_x, y, in_vol_norm);
        let in_vol_label = format!("{}%", self.input_volume);
        tree.text(
            right_x + SLIDER_WIDTH + 12.0,
            y + 14.0,
            &in_vol_label,
            COL_SUBTEXT0,
            12.0,
        );
        y += ITEM_HEIGHT + SECTION_SPACING;

        // System sounds
        y = render_section_header(tree, x, y, "System Sounds");
        render_setting_row(tree, x, y, "Enable System Sounds", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.system_sounds_enabled);
        y += ITEM_HEIGHT + SECTION_SPACING;

        // Per-app volume
        y = render_section_header(tree, x, y, "Per-Application Volume");
        for app_vol in &self.app_volumes {
            render_setting_row(tree, x, y, &app_vol.app_name, 0.0);
            let app_norm = app_vol.volume as f32 / 100.0;
            render_slider(tree, right_x, y, app_norm);
            let app_label = format!("{}%", app_vol.volume);
            tree.text(
                right_x + SLIDER_WIDTH + 12.0,
                y + 14.0,
                &app_label,
                COL_SUBTEXT0,
                12.0,
            );
            // Mute indicator
            if app_vol.muted {
                tree.text(
                    right_x + SLIDER_WIDTH + 50.0,
                    y + 14.0,
                    "(muted)",
                    COL_RED,
                    11.0,
                );
            }
            y += ITEM_HEIGHT;
        }
    }

    // --- Themes page ---

    fn render_themes_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;

        // Theme mode selection
        y = render_section_header(tree, x, y, "Theme Mode");
        let card_w = 140.0;
        let card_h = 100.0;
        let card_spacing = 16.0;
        for (idx, mode) in ThemeMode::ALL.iter().enumerate() {
            let cx = x + (idx as f32) * (card_w + card_spacing);
            let is_selected = *mode == self.appearance.settings.theme_mode;

            // Card background
            let card_bg = if is_selected {
                COL_SURFACE1
            } else {
                COL_SURFACE0
            };
            fill_rounded(tree, cx, y, card_w, card_h, card_bg, 8.0);

            // Selection border
            if is_selected {
                tree.push(RenderCommand::StrokeRect {
                    x: cx,
                    y,
                    width: card_w,
                    height: card_h,
                    color: COL_ACCENT,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(8.0),
                });
            }

            // Theme preview (mini window mockup)
            let preview_x = cx + 15.0;
            let preview_y = y + 12.0;
            let preview_w = card_w - 30.0;
            let preview_h = 50.0;

            let (win_bg, win_text) = match mode {
                ThemeMode::Light => (Color::from_hex(0xEFF1F5), Color::from_hex(0x4C4F69)),
                ThemeMode::Dark => (Color::from_hex(0x1E1E2E), Color::from_hex(0xCDD6F4)),
                ThemeMode::System => (Color::from_hex(0x313244), Color::from_hex(0xBAC2DE)),
            };
            fill_rounded(
                tree, preview_x, preview_y, preview_w, preview_h, win_bg, 4.0,
            );
            tree.text(preview_x + 8.0, preview_y + 18.0, "Aa", win_text, 16.0);

            // Label
            let label_y = y + card_h - 22.0;
            let label_color = if is_selected {
                COL_ACCENT
            } else {
                COL_SUBTEXT0
            };
            tree.text(
                cx + card_w / 2.0 - 16.0,
                label_y,
                mode.label(),
                label_color,
                13.0,
            );
        }
        y += card_h + SECTION_SPACING;

        // Transparency. A row of levels rather than the on/off switch this
        // page used to show: the setting has four values, and a switch that
        // meant "Off or whatever it was" would forget a user's choice of
        // Full every time they turned it off and on again.
        y = render_section_header(tree, x, y, "Effects");
        render_setting_row(tree, x, y, "Transparency", 0.0);
        let levels: Vec<(&str, bool)> = TransparencyLevel::ALL
            .iter()
            .map(|l| (l.label(), *l == self.appearance.settings.transparency))
            .collect();
        render_pill_row(tree, x + PILL_ROW_X, y, &levels);
        y += ITEM_HEIGHT;

        // Animation speed
        render_setting_row(tree, x, y, "Animation Speed", 0.0);
        let speeds: Vec<(&str, bool)> = AnimationSpeed::ALL
            .iter()
            .map(|s| (s.label(), *s == self.appearance.settings.animation_speed))
            .collect();
        render_pill_row(tree, x + PILL_ROW_X, y, &speeds);
    }

    // --- Colors page (accent color picker) ---

    fn render_colors_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;

        y = render_section_header(tree, x, y, "Accent Color");
        tree.text(
            x,
            y,
            "Choose an accent color for buttons, links, and highlights:",
            COL_SUBTEXT0,
            13.0,
        );
        y += 28.0;

        // Color grid
        let swatch_size = 36.0;
        let swatch_spacing = 10.0;
        let cols = 6;

        for (idx, accent) in AccentColor::presets().iter().enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            let sx = x + (col as f32) * (swatch_size + swatch_spacing);
            let sy = y + (row as f32) * (swatch_size + swatch_spacing);

            // The swatch shows what this accent will actually look like in
            // the mode the user is in — the light palette is a darkened set,
            // so showing the dark one would promise a colour they won't get.
            let color = if self.appearance.settings.theme_mode.is_light() {
                accent.color_light()
            } else {
                accent.color()
            };
            fill_rounded(
                tree,
                sx,
                sy,
                swatch_size,
                swatch_size,
                color,
                swatch_size / 2.0,
            );

            // Selection ring
            if *accent == self.appearance.settings.accent_color {
                tree.push(RenderCommand::StrokeRect {
                    x: sx - 3.0,
                    y: sy - 3.0,
                    width: swatch_size + 6.0,
                    height: swatch_size + 6.0,
                    color: COL_TEXT,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all((swatch_size + 6.0) / 2.0),
                });
            }
        }

        let grid_rows = AccentColor::presets().len().div_ceil(cols);
        y += (grid_rows as f32) * (swatch_size + swatch_spacing) + SECTION_SPACING;

        // Preview of current accent color
        y = render_section_header(tree, x, y, "Preview");
        let preview_color = self.appearance.settings.effective_accent();

        // Sample button
        fill_rounded(tree, x, y, 120.0, 36.0, preview_color, 6.0);
        tree.text(x + 20.0, y + 10.0, "Sample Button", COL_CRUST, 13.0);

        // Sample link text
        tree.text(x + 150.0, y + 10.0, "Sample link text", preview_color, 13.0);

        // Sample progress bar
        y += 50.0;
        fill_rounded(tree, x, y, 300.0, 8.0, COL_SURFACE1, 4.0);
        fill_rounded(tree, x, y, 200.0, 8.0, preview_color, 4.0);
    }

    // --- Network status page ---

    fn render_network_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;
        let right_x = x + 350.0;

        // Adapter list
        y = render_section_header(tree, x, y, "Network Adapters");
        for (idx, adapter) in self.adapters.iter().enumerate() {
            let row_y = y;
            let is_selected = idx == self.selected_adapter;
            let row_bg = if is_selected {
                COL_SURFACE0
            } else {
                Color::TRANSPARENT
            };
            fill_rounded(tree, x - 8.0, row_y, 600.0, ITEM_HEIGHT, row_bg, 6.0);

            // Status indicator
            let status_color = if adapter.connected {
                COL_GREEN
            } else {
                COL_OVERLAY0
            };
            fill_rounded(tree, x, row_y + 18.0, 10.0, 10.0, status_color, 5.0);

            // Adapter name and type
            tree.text(x + 20.0, row_y + 8.0, &adapter.name, COL_TEXT, 14.0);
            tree.text(
                x + 20.0,
                row_y + 26.0,
                adapter.adapter_type.label(),
                COL_SUBTEXT0,
                11.0,
            );

            // IP address or status
            let status_text = if adapter.connected {
                &adapter.ip_address
            } else {
                "Disconnected"
            };
            tree.text(right_x, row_y + 14.0, status_text, COL_SUBTEXT0, 13.0);

            y += ITEM_HEIGHT + 4.0;
        }
        y += SECTION_SPACING;

        // IP Configuration for selected adapter
        y = render_section_header(tree, x, y, "IP Configuration");
        render_setting_row(tree, x, y, "Mode", 0.0);
        render_dropdown_button(
            tree,
            right_x,
            y,
            self.ip_config_mode.label(),
            DROPDOWN_WIDTH,
        );
        y += ITEM_HEIGHT;

        if self.ip_config_mode == IpConfigMode::Static {
            render_setting_row(tree, x, y, "IP Address", 0.0);
            self.render_text_field(tree, right_x, y, &self.static_ip, 180.0);
            y += ITEM_HEIGHT;

            render_setting_row(tree, x, y, "Gateway", 0.0);
            self.render_text_field(tree, right_x, y, &self.static_gateway, 180.0);
            y += ITEM_HEIGHT;
        }
        y += SECTION_SPACING;

        // DNS
        y = render_section_header(tree, x, y, "DNS Servers");
        render_setting_row(tree, x, y, "Primary DNS", 0.0);
        self.render_text_field(tree, right_x, y, &self.dns_primary, 180.0);
        y += ITEM_HEIGHT;

        render_setting_row(tree, x, y, "Secondary DNS", 0.0);
        self.render_text_field(tree, right_x, y, &self.dns_secondary, 180.0);
    }

    // --- Proxy page ---

    fn render_proxy_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;
        let right_x = x + 350.0;

        y = render_section_header(tree, x, y, "Proxy Configuration");

        render_setting_row(tree, x, y, "Use Proxy Server", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.proxy_enabled);
        y += ITEM_HEIGHT;

        if self.proxy_enabled {
            render_setting_row(tree, x, y, "Proxy Address", 0.0);
            self.render_text_field(tree, right_x, y, &self.proxy_address, 220.0);
            y += ITEM_HEIGHT;

            render_setting_row(tree, x, y, "Port", 0.0);
            self.render_text_field(tree, right_x, y, &self.proxy_port, 80.0);
        }
    }

    // --- Accounts page ---

    fn render_accounts_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;
        let right_x = x + 350.0;

        if let SettingsPage::LoginOptions = self.current_page {
            // Login Options sub-page
            y = render_section_header(tree, x, y, "Login Options");

                render_setting_row(tree, x, y, "Auto-login on startup", 0.0);
                render_toggle(tree, right_x, y + 12.0, self.auto_login_enabled);
                y += ITEM_HEIGHT;

                // Password change button
                render_setting_row(tree, x, y, "Password", 0.0);
                self.render_button(tree, right_x, y + 6.0, "Change Password", COL_ACCENT);
                y += ITEM_HEIGHT + SECTION_SPACING;

                // Account picture
                y = render_section_header(tree, x, y, "Account Picture");
                tree.text(
                    x,
                    y + 4.0,
                    "Choose a picture for your account:",
                    COL_SUBTEXT0,
                    13.0,
                );
                y += 28.0;

                // Picture selection grid (placeholder icons)
                let icon_size = 48.0;
                let icon_spacing = 12.0;
                let icons = [
                    "\u{1F464}",
                    "\u{1F468}",
                    "\u{1F469}",
                    "\u{1F474}",
                    "\u{1F475}",
                    "\u{1F476}",
                ];
                for (idx, icon) in icons.iter().enumerate() {
                    let ix = x + (idx as f32) * (icon_size + icon_spacing);
                    let is_selected = idx == 0; // first is default selected
                    let bg = if is_selected {
                        COL_SURFACE1
                    } else {
                        COL_SURFACE0
                    };
                    fill_rounded(tree, ix, y, icon_size, icon_size, bg, 8.0);
                    if is_selected {
                        tree.push(RenderCommand::StrokeRect {
                            x: ix,
                            y,
                            width: icon_size,
                            height: icon_size,
                            color: COL_ACCENT,
                            line_width: 2.0,
                            corner_radii: CornerRadii::all(8.0),
                        });
                    }
                    tree.text(ix + 12.0, y + 12.0, icon, COL_TEXT, 20.0);
                }
                return;
        }

        // User account list (default UserAccounts page)
        y = render_section_header(tree, x, y, "User Accounts");

        for (idx, account) in self.user_accounts.iter().enumerate() {
            let row_y = y;
            let is_selected = idx == self.selected_account;
            let row_bg = if is_selected {
                COL_SURFACE0
            } else {
                Color::TRANSPARENT
            };
            fill_rounded(tree, x - 8.0, row_y, 620.0, 60.0, row_bg, 8.0);

            // Avatar placeholder
            let avatar_size = 40.0;
            fill_rounded(
                tree,
                x + 4.0,
                row_y + 10.0,
                avatar_size,
                avatar_size,
                COL_SURFACE2,
                avatar_size / 2.0,
            );
            tree.text(x + 16.0, row_y + 20.0, "\u{1F464}", COL_TEXT, 16.0);

            // Name and email
            text_bold(tree, x + 56.0, row_y + 12.0, &account.name, COL_TEXT, 14.0);
            tree.text(x + 56.0, row_y + 32.0, &account.email, COL_SUBTEXT0, 12.0);

            // Account type badge
            let badge_color = account.account_type.color();
            let badge_label = account.account_type.label();
            let badge_x = right_x;
            fill_rounded(tree, badge_x, row_y + 18.0, 90.0, 22.0, badge_color, 4.0);
            tree.text(badge_x + 8.0, row_y + 22.0, badge_label, COL_CRUST, 11.0);

            // Current user indicator
            if account.is_current {
                tree.text(badge_x + 100.0, row_y + 22.0, "(You)", COL_ACCENT, 11.0);
            }

            y += 64.0;
        }
        y += SECTION_SPACING;

        // Add/Remove buttons
        self.render_button(tree, x, y, "+ Add Account", COL_ACCENT);
        self.render_button(tree, x + 140.0, y, "- Remove Account", COL_RED);
        y += 44.0 + SECTION_SPACING;

        // Current user details
        if let Some(account) = self.user_accounts.get(self.selected_account) {
            y = render_section_header(tree, x, y, "Account Details");

            render_setting_row(tree, x, y, "Name", 0.0);
            tree.text(right_x, y + 14.0, &account.name, COL_TEXT, 13.0);
            y += ITEM_HEIGHT;

            render_setting_row(tree, x, y, "Email", 0.0);
            tree.text(right_x, y + 14.0, &account.email, COL_TEXT, 13.0);
            y += ITEM_HEIGHT;

            render_setting_row(tree, x, y, "Account Type", 0.0);
            tree.text(
                right_x,
                y + 14.0,
                account.account_type.label(),
                account.account_type.color(),
                13.0,
            );
            y += ITEM_HEIGHT;

            render_setting_row(tree, x, y, "Login Count", 0.0);
            let login_str = format!("{}", account.login_count);
            tree.text(right_x, y + 14.0, &login_str, COL_TEXT, 13.0);
            y += ITEM_HEIGHT;

            render_setting_row(tree, x, y, "Last Login", 0.0);
            tree.text(right_x, y + 14.0, &account.last_login, COL_TEXT, 13.0);
            y += ITEM_HEIGHT;

            // Family safety for child accounts
            if account.account_type == AccountType::Child {
                y += SECTION_SPACING;
                y = render_section_header(tree, x, y, "Family Safety");
                tree.text(
                    x,
                    y + 4.0,
                    "Screen time limits and content filters are active",
                    COL_SUBTEXT0,
                    13.0,
                );
                y += 24.0;
                self.render_button(tree, x, y, "Manage Family Settings", COL_PEACH);
            }
        }
    }

    // --- Privacy page ---

    fn render_privacy_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;
        let right_x = x + 350.0;

        if let SettingsPage::Capabilities = self.current_page {
            // App permissions summary sub-page
            y = render_section_header(tree, x, y, "App Permissions Summary");
                tree.text(
                    x,
                    y + 4.0,
                    "Overview of which apps have access to sensitive resources:",
                    COL_SUBTEXT0,
                    13.0,
                );
                y += 32.0;

                // Summary table header
                text_bold(tree, x, y, "App", COL_TEXT, 13.0);
                text_bold(tree, x + 200.0, y, "Location", COL_TEXT, 13.0);
                text_bold(tree, x + 290.0, y, "Camera", COL_TEXT, 13.0);
                text_bold(tree, x + 370.0, y, "Mic", COL_TEXT, 13.0);
                text_bold(tree, x + 440.0, y, "Background", COL_TEXT, 13.0);
                y += 24.0;

                // Divider
                tree.push(RenderCommand::Line {
                    x1: x,
                    y1: y,
                    x2: x + 560.0,
                    y2: y,
                    color: COL_SURFACE1,
                    width: 1.0,
                });
                y += 8.0;

                // Build summary from all apps mentioned
                let all_apps = [
                    "Maps",
                    "Weather",
                    "Camera",
                    "Browser",
                    "Video Chat",
                    "Social Media",
                    "Voice Recorder",
                    "Email",
                    "Music Player",
                ];
                for app_name in all_apps {
                    let loc = self.location_apps.iter().find(|a| a.app_name == app_name);
                    let cam = self.camera_apps.iter().find(|a| a.app_name == app_name);
                    let mic = self.microphone_apps.iter().find(|a| a.app_name == app_name);
                    let bg = self.background_apps.iter().find(|a| a.app_name == app_name);

                    tree.text(x, y + 4.0, app_name, COL_TEXT, 12.0);

                    let check = "\u{2713}";
                    let cross = "\u{2717}";

                    // Location
                    if let Some(p) = loc {
                        let (sym, col) = if p.allowed {
                            (check, COL_GREEN)
                        } else {
                            (cross, COL_RED)
                        };
                        tree.text(x + 220.0, y + 4.0, sym, col, 13.0);
                    } else {
                        tree.text(x + 220.0, y + 4.0, "-", COL_OVERLAY0, 13.0);
                    }
                    // Camera
                    if let Some(p) = cam {
                        let (sym, col) = if p.allowed {
                            (check, COL_GREEN)
                        } else {
                            (cross, COL_RED)
                        };
                        tree.text(x + 310.0, y + 4.0, sym, col, 13.0);
                    } else {
                        tree.text(x + 310.0, y + 4.0, "-", COL_OVERLAY0, 13.0);
                    }
                    // Mic
                    if let Some(p) = mic {
                        let (sym, col) = if p.allowed {
                            (check, COL_GREEN)
                        } else {
                            (cross, COL_RED)
                        };
                        tree.text(x + 385.0, y + 4.0, sym, col, 13.0);
                    } else {
                        tree.text(x + 385.0, y + 4.0, "-", COL_OVERLAY0, 13.0);
                    }
                    // Background
                    if let Some(p) = bg {
                        let (sym, col) = if p.allowed {
                            (check, COL_GREEN)
                        } else {
                            (cross, COL_RED)
                        };
                        tree.text(x + 465.0, y + 4.0, sym, col, 13.0);
                    } else {
                        tree.text(x + 465.0, y + 4.0, "-", COL_OVERLAY0, 13.0);
                    }

                    y += 28.0;
                }
                return;
        }

        // Location access (default Permissions page)
        y = render_section_header(tree, x, y, "Location");
        render_setting_row(tree, x, y, "Allow apps to access location", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.location_enabled);
        y += ITEM_HEIGHT;

        if self.location_enabled {
            for app in &self.location_apps {
                tree.text(x + 16.0, y + 14.0, &app.app_name, COL_SUBTEXT1, 13.0);
                render_toggle(tree, right_x, y + 12.0, app.allowed);
                y += ITEM_HEIGHT - 8.0;
            }
        }
        y += SECTION_SPACING;

        // Camera access
        y = render_section_header(tree, x, y, "Camera");
        render_setting_row(tree, x, y, "Allow apps to access camera", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.camera_enabled);
        y += ITEM_HEIGHT;

        if self.camera_enabled {
            for app in &self.camera_apps {
                tree.text(x + 16.0, y + 14.0, &app.app_name, COL_SUBTEXT1, 13.0);
                render_toggle(tree, right_x, y + 12.0, app.allowed);
                y += ITEM_HEIGHT - 8.0;
            }
        }
        y += SECTION_SPACING;

        // Microphone access
        y = render_section_header(tree, x, y, "Microphone");
        render_setting_row(tree, x, y, "Allow apps to access microphone", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.microphone_enabled);
        y += ITEM_HEIGHT;

        if self.microphone_enabled {
            for app in &self.microphone_apps {
                tree.text(x + 16.0, y + 14.0, &app.app_name, COL_SUBTEXT1, 13.0);
                render_toggle(tree, right_x, y + 12.0, app.allowed);
                y += ITEM_HEIGHT - 8.0;
            }
        }
        y += SECTION_SPACING;

        // Background apps
        y = render_section_header(tree, x, y, "Background Apps");
        tree.text(
            x,
            y + 4.0,
            "Choose which apps can run in the background:",
            COL_SUBTEXT0,
            13.0,
        );
        y += 28.0;

        for app in &self.background_apps {
            tree.text(x + 16.0, y + 14.0, &app.app_name, COL_SUBTEXT1, 13.0);
            render_toggle(tree, right_x, y + 12.0, app.allowed);
            y += ITEM_HEIGHT - 8.0;
        }
        y += SECTION_SPACING;

        // Diagnostics
        y = render_section_header(tree, x, y, "Diagnostics & Data");
        render_setting_row(tree, x, y, "Diagnostic data collection", 0.0);
        render_dropdown_button(
            tree,
            right_x,
            y,
            self.diagnostic_level.label(),
            DROPDOWN_WIDTH,
        );
        y += ITEM_HEIGHT + SECTION_SPACING;

        // Activity history
        y = render_section_header(tree, x, y, "Activity History");
        tree.text(
            x,
            y + 4.0,
            "Clear your activity history stored on this device.",
            COL_SUBTEXT0,
            13.0,
        );
        y += 28.0;
        self.render_button(tree, x, y, "Clear Activity History", COL_RED);
    }

    // --- Accessibility page ---

    fn render_accessibility_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;
        let right_x = x + 350.0;

        match self.current_page {
            SettingsPage::Audio => {
                // Audio accessibility sub-page
                y = render_section_header(tree, x, y, "Audio Accessibility");

                render_setting_row(tree, x, y, "Mono audio", 0.0);
                render_toggle(tree, right_x, y + 12.0, self.mono_audio);
                y += ITEM_HEIGHT;

                render_setting_row(tree, x, y, "Visual alerts for sounds", 0.0);
                render_toggle(tree, right_x, y + 12.0, self.visual_alerts);
                y += ITEM_HEIGHT + SECTION_SPACING;

                // Narrator
                y = render_section_header(tree, x, y, "Narrator");
                render_setting_row(tree, x, y, "Enable Narrator", 0.0);
                render_toggle(tree, right_x, y + 12.0, self.narrator_enabled);
                y += ITEM_HEIGHT;

                if self.narrator_enabled {
                    render_setting_row(tree, x, y, "Voice Rate", 0.0);
                    render_slider(tree, right_x, y, self.narrator_rate);
                    tree.text(right_x, y + 36.0, "Slow", COL_SUBTEXT0, 11.0);
                    tree.text(
                        right_x + SLIDER_WIDTH - 24.0,
                        y + 36.0,
                        "Fast",
                        COL_SUBTEXT0,
                        11.0,
                    );
                    y += ITEM_HEIGHT + 16.0;

                    render_setting_row(tree, x, y, "Verbosity", 0.0);
                    render_dropdown_button(
                        tree,
                        right_x,
                        y,
                        self.narrator_verbosity.label(),
                        DROPDOWN_WIDTH,
                    );
                }
                return;
            }
            SettingsPage::Interaction => {
                // Input/Interaction accessibility sub-page
                y = render_section_header(tree, x, y, "Keyboard");

                render_setting_row(tree, x, y, "Sticky Keys", 0.0);
                render_toggle(tree, right_x, y + 12.0, self.sticky_keys);
                y += ITEM_HEIGHT;
                tree.text(
                    x + 16.0,
                    y - 4.0,
                    "Press modifier keys one at a time",
                    COL_SUBTEXT0,
                    11.0,
                );
                y += 12.0;

                render_setting_row(tree, x, y, "Filter Keys", 0.0);
                render_toggle(tree, right_x, y + 12.0, self.filter_keys);
                y += ITEM_HEIGHT;
                tree.text(
                    x + 16.0,
                    y - 4.0,
                    "Ignore brief or repeated keystrokes",
                    COL_SUBTEXT0,
                    11.0,
                );
                y += 12.0;

                render_setting_row(tree, x, y, "Toggle Keys", 0.0);
                render_toggle(tree, right_x, y + 12.0, self.toggle_keys);
                y += ITEM_HEIGHT;
                tree.text(
                    x + 16.0,
                    y - 4.0,
                    "Play a sound when pressing Caps/Num/Scroll Lock",
                    COL_SUBTEXT0,
                    11.0,
                );
                y += 12.0;

                render_setting_row(tree, x, y, "On-Screen Keyboard", 0.0);
                render_toggle(tree, right_x, y + 12.0, self.onscreen_keyboard);
                y += ITEM_HEIGHT + SECTION_SPACING;

                // Mouse
                y = render_section_header(tree, x, y, "Mouse & Pointer");

                render_setting_row(tree, x, y, "Pointer Size", 0.0);
                // Render pointer size as segmented buttons (1-5)
                for i in 1u8..=5 {
                    let btn_x = right_x + ((i - 1) as f32) * 40.0;
                    let is_active = i == self.pointer_size;
                    let btn_bg = if is_active { COL_ACCENT } else { COL_SURFACE1 };
                    let btn_fg = if is_active { COL_CRUST } else { COL_SUBTEXT0 };
                    fill_rounded(tree, btn_x, y + 10.0, 32.0, 26.0, btn_bg, 4.0);
                    let size_label = format!("{}", i);
                    tree.text(btn_x + 12.0, y + 16.0, &size_label, btn_fg, 12.0);
                }
                y += ITEM_HEIGHT;

                render_setting_row(tree, x, y, "Mouse Keys (numpad controls pointer)", 0.0);
                render_toggle(tree, right_x, y + 12.0, self.mouse_keys);
                return;
            }
            _ => {} // Visual (default)
        }

        // Display section
        y = render_section_header(tree, x, y, "Display");

        render_setting_row(tree, x, y, "Text Size", 0.0);
        let text_size_norm = (self.text_size_percent as f32 - 50.0) / 200.0;
        render_slider(tree, right_x, y, text_size_norm);
        let size_label = format!("{}%", self.text_size_percent);
        tree.text(
            right_x + SLIDER_WIDTH + 12.0,
            y + 14.0,
            &size_label,
            COL_SUBTEXT0,
            12.0,
        );
        y += ITEM_HEIGHT;
        // Range labels
        tree.text(right_x, y - 8.0, "50%", COL_SUBTEXT0, 11.0);
        tree.text(
            right_x + SLIDER_WIDTH - 28.0,
            y - 8.0,
            "250%",
            COL_SUBTEXT0,
            11.0,
        );
        y += 8.0;

        render_setting_row(tree, x, y, "High Contrast", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.high_contrast);
        y += ITEM_HEIGHT;

        render_setting_row(tree, x, y, "Cursor Size", 0.0);
        render_dropdown_button(tree, right_x, y, self.cursor_size.label(), DROPDOWN_WIDTH);
        y += ITEM_HEIGHT;

        render_setting_row(tree, x, y, "Reduce Animations", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.reduce_animations);
        y += ITEM_HEIGHT + SECTION_SPACING;

        // Visual section
        y = render_section_header(tree, x, y, "Color & Transparency");

        render_setting_row(tree, x, y, "Color Filters", 0.0);
        render_dropdown_button(tree, right_x, y, self.color_filter.label(), DROPDOWN_WIDTH);
        y += ITEM_HEIGHT;

        render_setting_row(tree, x, y, "Reduce Transparency", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.reduce_transparency);
        y += ITEM_HEIGHT;

        // Color filter preview
        if self.color_filter != ColorFilter::None {
            y += 8.0;
            fill_rounded(tree, x, y, 300.0, 40.0, COL_SURFACE0, 6.0);
            tree.text(
                x + 12.0,
                y + 12.0,
                "Color filter active: ",
                COL_SUBTEXT0,
                12.0,
            );
            tree.text(
                x + 150.0,
                y + 12.0,
                self.color_filter.label(),
                COL_ACCENT,
                12.0,
            );
        }
    }

    // --- Update page ---

    fn render_update_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;
        let right_x = x + 350.0;

        match self.current_page {
            SettingsPage::Recovery => {
                // Recovery sub-page
                y = render_section_header(tree, x, y, "Recovery Options");
                tree.text(
                    x,
                    y + 4.0,
                    "If your PC isn't working well, recovering may help.",
                    COL_SUBTEXT0,
                    13.0,
                );
                y += 32.0;

                // Go back option
                fill_rounded(tree, x, y, 580.0, 80.0, COL_SURFACE0, 8.0);
                text_bold(
                    tree,
                    x + 16.0,
                    y + 12.0,
                    "Go Back to Previous Version",
                    COL_TEXT,
                    14.0,
                );
                tree.text(
                    x + 16.0,
                    y + 34.0,
                    "Revert to the previous OS build. Available for 10 days",
                    COL_SUBTEXT0,
                    12.0,
                );
                tree.text(x + 16.0, y + 50.0, "after an update.", COL_SUBTEXT0, 12.0);
                self.render_button(tree, x + 440.0, y + 28.0, "Go Back", COL_PEACH);
                y += 96.0;

                // Fresh start
                fill_rounded(tree, x, y, 580.0, 80.0, COL_SURFACE0, 8.0);
                text_bold(tree, x + 16.0, y + 12.0, "Fresh Start", COL_TEXT, 14.0);
                tree.text(
                    x + 16.0,
                    y + 34.0,
                    "Reinstall the OS while keeping your personal files.",
                    COL_SUBTEXT0,
                    12.0,
                );
                tree.text(
                    x + 16.0,
                    y + 50.0,
                    "All apps and settings will be removed.",
                    COL_SUBTEXT0,
                    12.0,
                );
                self.render_button(tree, x + 440.0, y + 28.0, "Reset", COL_RED);
                return;
            }
            SettingsPage::Snapshots => {
                // Snapshots sub-page (system restore points)
                y = render_section_header(tree, x, y, "System Snapshots");
                tree.text(
                    x,
                    y + 4.0,
                    "Package generation snapshots for safe rollback:",
                    COL_SUBTEXT0,
                    13.0,
                );
                y += 32.0;

                let snapshots = [
                    ("Gen 42", "2026-05-17 09:00", "Current"),
                    ("Gen 41", "2026-05-15 14:30", "After KB5032100"),
                    ("Gen 40", "2026-05-10 11:00", "After KB5031980"),
                    ("Gen 39", "2026-05-01 08:45", "After compositor update"),
                ];

                for (name, date, desc) in snapshots {
                    let is_current = desc == "Current";
                    let bg = if is_current {
                        COL_SURFACE1
                    } else {
                        COL_SURFACE0
                    };
                    fill_rounded(tree, x, y, 580.0, 48.0, bg, 6.0);

                    text_bold(tree, x + 12.0, y + 8.0, name, COL_TEXT, 13.0);
                    tree.text(x + 12.0, y + 28.0, desc, COL_SUBTEXT0, 11.0);
                    tree.text(right_x, y + 16.0, date, COL_SUBTEXT0, 12.0);

                    if is_current {
                        tree.text(x + 520.0, y + 16.0, "\u{2713}", COL_GREEN, 16.0);
                    }

                    y += 56.0;
                }
                return;
            }
            _ => {} // SystemUpdates (default)
        }

        // Version info
        y = render_section_header(tree, x, y, "System Information");
        fill_rounded(tree, x, y, 580.0, 60.0, COL_SURFACE0, 8.0);
        text_bold(tree, x + 16.0, y + 12.0, "Slate OS", COL_TEXT, 16.0);
        tree.text(x + 16.0, y + 36.0, &self.os_version, COL_SUBTEXT0, 13.0);
        y += 72.0;

        // Check for updates
        let btn_label = if self.checking_for_updates {
            "Checking..."
        } else {
            "Check for Updates"
        };
        self.render_button(tree, x, y, btn_label, COL_ACCENT);
        if !self.checking_for_updates {
            tree.text(
                x + 160.0,
                y + 10.0,
                "Your device is up to date",
                COL_GREEN,
                13.0,
            );
        }
        y += 44.0 + SECTION_SPACING;

        // Auto-update
        y = render_section_header(tree, x, y, "Update Preferences");
        render_setting_row(tree, x, y, "Automatic updates", 0.0);
        render_toggle(tree, right_x, y + 12.0, self.auto_update_enabled);
        y += ITEM_HEIGHT;

        // Active hours
        render_setting_row(tree, x, y, "Active hours (no restart)", 0.0);
        let hours_label = format!(
            "{:02}:00 - {:02}:00",
            self.active_hours_start, self.active_hours_end
        );
        tree.text(right_x, y + 14.0, &hours_label, COL_TEXT, 13.0);
        y += ITEM_HEIGHT + SECTION_SPACING;

        // Advanced deferral
        y = render_section_header(tree, x, y, "Advanced");
        render_setting_row(tree, x, y, "Defer feature updates (days)", 0.0);
        let defer_feat_norm = self.defer_feature_days as f32 / 365.0;
        render_slider(tree, right_x, y, defer_feat_norm);
        let feat_label = format!("{} days", self.defer_feature_days);
        tree.text(
            right_x + SLIDER_WIDTH + 12.0,
            y + 14.0,
            &feat_label,
            COL_SUBTEXT0,
            12.0,
        );
        y += ITEM_HEIGHT;

        render_setting_row(tree, x, y, "Defer quality updates (days)", 0.0);
        let defer_qual_norm = self.defer_quality_days as f32 / 30.0;
        render_slider(tree, right_x, y, defer_qual_norm);
        let qual_label = format!("{} days", self.defer_quality_days);
        tree.text(
            right_x + SLIDER_WIDTH + 12.0,
            y + 14.0,
            &qual_label,
            COL_SUBTEXT0,
            12.0,
        );
        y += ITEM_HEIGHT + SECTION_SPACING;

        // Update history
        y = render_section_header(tree, x, y, "Update History");
        for entry in &self.update_history {
            fill_rounded(tree, x, y, 580.0, 44.0, COL_SURFACE0, 6.0);

            tree.text(x + 12.0, y + 8.0, &entry.kb_number, COL_TEXT, 13.0);
            tree.text(x + 120.0, y + 8.0, &entry.description, COL_SUBTEXT0, 12.0);
            tree.text(x + 12.0, y + 26.0, &entry.date, COL_OVERLAY0, 11.0);

            // Status badge
            let status_color = entry.status.color();
            let status_label = entry.status.label();
            fill_rounded(tree, x + 490.0, y + 12.0, 72.0, 20.0, status_color, 4.0);
            tree.text(x + 500.0, y + 15.0, status_label, COL_CRUST, 11.0);

            y += 52.0;
        }
    }

    // --- Helper: render a button ---

    #[allow(dead_code)]
    fn render_button(&self, tree: &mut RenderTree, x: f32, y: f32, label: &str, color: Color) {
        let btn_w = text::padded_width(label, 12.0, 13.0, FontWeightHint::Regular);
        let btn_h = 32.0;
        fill_rounded(tree, x, y, btn_w, btn_h, color, 6.0);
        tree.text(x + 12.0, y + 8.0, label, COL_CRUST, 13.0);
    }

    // --- Placeholder for unimplemented pages ---

    fn render_placeholder_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let page_name = self.current_page.label();
        text_bold(tree, x, start_y + 20.0, page_name, COL_TEXT, 22.0);
        tree.text(
            x,
            start_y + 56.0,
            "This page is under construction.",
            COL_SUBTEXT0,
            14.0,
        );

        // Visual placeholder: a card with icon
        let card_y = start_y + 100.0;
        fill_rounded(tree, x, card_y, 400.0, 150.0, COL_SURFACE0, 12.0);
        tree.text(x + 170.0, card_y + 50.0, "\u{1F6A7}", COL_PEACH, 36.0);
        tree.text(
            x + 120.0,
            card_y + 110.0,
            "Coming soon...",
            COL_SUBTEXT0,
            14.0,
        );
    }

    // --- Helper: render a text field ---

    fn render_text_field(&self, tree: &mut RenderTree, x: f32, y: f32, value: &str, width: f32) {
        let field_y = y + 6.0;
        let field_h = 32.0;
        fill_rounded(tree, x, field_y, width, field_h, COL_SURFACE0, 6.0);
        tree.push(RenderCommand::StrokeRect {
            x,
            y: field_y,
            width,
            height: field_h,
            color: COL_OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });
        text_clipped(
            tree,
            x + 8.0,
            field_y + 8.0,
            value,
            COL_TEXT,
            13.0,
            width - 16.0,
        );
    }

    // --- Dropdown overlay rendering ---

    /// Where the open dropdown's popup goes, or `None` if none is open.
    ///
    /// The single source of the popup's geometry. Both the renderer and the
    /// click handler ask this, so a click can never land on a different item
    /// from the one drawn under the pointer — the failure that made these
    /// dropdowns unusable was precisely that only the renderer knew where
    /// anything was.
    #[must_use]
    pub fn dropdown_layout(&self) -> Option<DropdownLayout> {
        let dropdown_id = self.open_dropdown?;

        let (items, selected, dropdown_x, dropdown_y) = match dropdown_id {
            DropdownId::Resolution => {
                let items: Vec<String> = RESOLUTIONS.iter().map(|r| r.label()).collect();
                (
                    items,
                    self.resolution_index,
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 180.0,
                )
            }
            DropdownId::RefreshRate => {
                let items: Vec<String> =
                    REFRESH_RATES.iter().map(|r| format!("{} Hz", r)).collect();
                (
                    items,
                    self.refresh_rate_index,
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 228.0,
                )
            }
            DropdownId::Scale => {
                let items: Vec<String> = ScalePercent::ALL
                    .iter()
                    .map(|s| s.label().to_string())
                    .collect();
                (
                    items,
                    ScalePercent::ALL
                        .iter()
                        .position(|s| *s == self.scale)
                        .unwrap_or(0),
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 276.0,
                )
            }
            DropdownId::OutputDevice => {
                let items: Vec<String> =
                    self.output_devices.iter().map(|d| d.name.clone()).collect();
                (
                    items,
                    self.output_device_index,
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 68.0,
                )
            }
            DropdownId::InputDevice => {
                let items: Vec<String> =
                    self.input_devices.iter().map(|d| d.name.clone()).collect();
                (
                    items,
                    self.input_device_index,
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 250.0,
                )
            }
            DropdownId::IpConfig => {
                let items = vec![
                    IpConfigMode::Dhcp.label().to_string(),
                    IpConfigMode::Static.label().to_string(),
                ];
                let sel = if self.ip_config_mode == IpConfigMode::Dhcp {
                    0
                } else {
                    1
                };
                (
                    items,
                    sel,
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 300.0,
                )
            }
            DropdownId::DiagnosticLevel => {
                let items: Vec<String> = DiagnosticLevel::ALL
                    .iter()
                    .map(|d| d.label().to_string())
                    .collect();
                let sel = DiagnosticLevel::ALL
                    .iter()
                    .position(|d| *d == self.diagnostic_level)
                    .unwrap_or(0);
                (
                    items,
                    sel,
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 200.0,
                )
            }
            DropdownId::ColorFilter => {
                let items: Vec<String> = ColorFilter::ALL
                    .iter()
                    .map(|f| f.label().to_string())
                    .collect();
                let sel = ColorFilter::ALL
                    .iter()
                    .position(|f| *f == self.color_filter)
                    .unwrap_or(0);
                (
                    items,
                    sel,
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 200.0,
                )
            }
            DropdownId::CursorSize => {
                let items: Vec<String> = CursorSize::ALL
                    .iter()
                    .map(|c| c.label().to_string())
                    .collect();
                let sel = CursorSize::ALL
                    .iter()
                    .position(|c| *c == self.cursor_size)
                    .unwrap_or(0);
                (
                    items,
                    sel,
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 160.0,
                )
            }
            DropdownId::NarratorVerbosity => {
                let items: Vec<String> = NarratorVerbosity::ALL
                    .iter()
                    .map(|v| v.label().to_string())
                    .collect();
                let sel = NarratorVerbosity::ALL
                    .iter()
                    .position(|v| *v == self.narrator_verbosity)
                    .unwrap_or(0);
                (
                    items,
                    sel,
                    SIDEBAR_WIDTH + CONTENT_PADDING + 350.0,
                    HEADER_HEIGHT + 250.0,
                )
            }
        };

        let popup_w = DROPDOWN_WIDTH + 20.0;
        let wanted_h = (items.len() as f32) * DROPDOWN_ITEM_HEIGHT + DROPDOWN_PADDING;
        let room = self.window_height - 2.0 * DROPDOWN_MARGIN;
        // Whether every item fits is decidable before anything is laid out, so
        // the "N more" line's space can be subtracted only when it is going to
        // be drawn without the budget depending on its own result: a popup that
        // fits whole loses no row to a footer it will not have.
        let fits = wanted_h <= room;
        let popup_h = if fits { wanted_h } else { room.max(0.0) };
        let rows_h = popup_h - DROPDOWN_PADDING - if fits { 0.0 } else { LIST_MORE_HEIGHT };

        // Pull the popup up rather than letting it run off the bottom, and
        // never above the top edge. A popup drawn past the window's end is
        // items the user can see are missing but cannot reach.
        let lowest_top = (self.window_height - DROPDOWN_MARGIN - popup_h).max(DROPDOWN_MARGIN);
        let dropdown_y = dropdown_y.min(lowest_top).max(DROPDOWN_MARGIN);

        let window = scroll_window::visible(
            items.len(),
            DROPDOWN_ITEM_HEIGHT,
            rows_h,
            self.dropdown_scroll,
        );

        Some(DropdownLayout {
            items,
            selected,
            x: dropdown_x,
            y: dropdown_y,
            width: popup_w,
            height: popup_h,
            window,
        })
    }

    fn render_open_dropdown(&self, tree: &mut RenderTree) {
        let Some(layout) = self.dropdown_layout() else {
            return;
        };
        let DropdownLayout {
            ref items,
            selected,
            x: dropdown_x,
            y: dropdown_y,
            width: popup_w,
            height: popup_h,
            window,
        } = layout;

        // Shadow
        tree.push(RenderCommand::BoxShadow {
            x: dropdown_x,
            y: dropdown_y,
            width: popup_w,
            height: popup_h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 12.0,
            spread: 2.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(8.0),
        });

        // Background
        fill_rounded(
            tree,
            dropdown_x,
            dropdown_y,
            popup_w,
            popup_h,
            COL_SURFACE0,
            8.0,
        );
        tree.push(RenderCommand::StrokeRect {
            x: dropdown_x,
            y: dropdown_y,
            width: popup_w,
            height: popup_h,
            color: COL_OVERLAY0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Items
        for (row, item) in items
            .get(window.start..window.end())
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let iy = layout.row_top(row);
            // The tick marks the chosen item, which is an index into the whole
            // list, so compare against the absolute position rather than the
            // position on screen.
            let idx = window.start.saturating_add(row);
            let is_selected = idx == selected;

            if is_selected {
                fill_rounded(
                    tree,
                    dropdown_x + 4.0,
                    iy,
                    popup_w - 8.0,
                    DROPDOWN_ITEM_HEIGHT - 2.0,
                    COL_SURFACE1,
                    4.0,
                );
            }

            let item_color = if is_selected { COL_ACCENT } else { COL_TEXT };
            text_clipped(
                tree,
                dropdown_x + 12.0,
                iy + 10.0,
                item,
                item_color,
                13.0,
                popup_w - 24.0,
            );

            // Checkmark for selected
            if is_selected {
                tree.text(
                    dropdown_x + popup_w - 24.0,
                    iy + 10.0,
                    "\u{2713}",
                    COL_ACCENT,
                    14.0,
                );
            }
        }

        // A popup that is hiding items has to say so: an option that exists and
        // is merely below the fold is otherwise indistinguishable from one the
        // system does not offer.
        let hidden = layout.hidden();
        if hidden > 0 {
            tree.text(
                dropdown_x + 12.0,
                layout.row_top(window.count) + 10.0,
                &format!("{hidden} more"),
                COL_OVERLAY0,
                11.0,
            );
        }
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle an input event, returning whether it was consumed.
    ///
    /// Any appearance setting the event changed is written back before this
    /// returns. The check is a whole-struct comparison rather than a flag each
    /// control sets, for the same reason the desktop's panel compares rather
    /// than lists: a flag is one more thing a new control can forget, and the
    /// symptom — a setting that works until you close the window — is one of
    /// the harder ones to notice.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        let before = self.appearance.settings.clone();
        let result = self.dispatch_event(event);
        if self.appearance.settings != before {
            self.save_appearance();
        }
        result
    }

    /// Route an event to its handler. Split from [`handle_event`](Self::handle_event)
    /// so the routing can be exercised without writing to the user's home
    /// directory.
    fn dispatch_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key_evt) => self.handle_key(key_evt),
            Event::Mouse(mouse_evt) => self.handle_mouse(mouse_evt),
            Event::Resize { width, height } => {
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, evt: &KeyEvent) -> EventResult {
        if !evt.pressed {
            return EventResult::Ignored;
        }

        // Close dropdown on Escape
        if evt.key == Key::Escape {
            if self.open_dropdown.is_some() {
                self.open_dropdown = None;
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        // Search focus with Ctrl+F
        if evt.modifiers.ctrl && evt.key == Key::F {
            self.search_focused = true;
            return EventResult::Consumed;
        }

        // Text input for search
        if self.search_focused {
            match evt.key {
                Key::Backspace => {
                    self.search_query.pop();
                    return EventResult::Consumed;
                }
                Key::Escape => {
                    self.search_focused = false;
                    self.search_query.clear();
                    return EventResult::Consumed;
                }
                _ => {
                    if let Some(ch) = evt.text {
                        self.search_query.push(ch);
                        return EventResult::Consumed;
                    }
                }
            }
        }

        // Category navigation with Up/Down when sidebar focused
        match evt.key {
            Key::Up => {
                let current_idx = SettingsCategory::ALL
                    .iter()
                    .position(|c| *c == self.current_category)
                    .unwrap_or(0);
                if current_idx > 0 {
                    let new_cat = SettingsCategory::ALL[current_idx - 1];
                    self.current_category = new_cat;
                    self.current_page = new_cat.default_page();
                }
                EventResult::Consumed
            }
            Key::Down => {
                let current_idx = SettingsCategory::ALL
                    .iter()
                    .position(|c| *c == self.current_category)
                    .unwrap_or(0);
                if current_idx + 1 < SettingsCategory::ALL.len() {
                    let new_cat = SettingsCategory::ALL[current_idx + 1];
                    self.current_category = new_cat;
                    self.current_page = new_cat.default_page();
                }
                EventResult::Consumed
            }
            Key::Tab => {
                // Cycle through pages within category
                let pages = self.current_category.pages();
                let current_idx = pages
                    .iter()
                    .position(|p| *p == self.current_page)
                    .unwrap_or(0);
                let next_idx = (current_idx + 1) % pages.len();
                self.current_page = pages[next_idx];
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, evt: &MouseEvent) -> EventResult {
        match &evt.kind {
            MouseEventKind::Press(MouseButton::Left) => self.handle_click(evt.x, evt.y),
            MouseEventKind::Move => self.handle_hover(evt.x, evt.y),
            // Only the sign of `dy` is used, not its magnitude:
            // `MouseEventKind::Scroll` documents it as pixels, but every
            // producer in this tree sets it to a notch count instead. See
            // known-issues.md.
            MouseEventKind::Scroll { dy, .. } if self.open_dropdown.is_some() => {
                let rows = if *dy > 0.0 {
                    WHEEL_ROWS.saturating_neg()
                } else if *dy < 0.0 {
                    WHEEL_ROWS
                } else {
                    0
                };
                self.scroll_dropdown_by(rows);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_click(&mut self, mx: f32, my: f32) -> EventResult {
        // An open dropdown swallows the click: either it names one of the
        // dropdown's items, or it dismisses the popup. It never reaches the
        // controls underneath, which are covered by it.
        if let Some(layout) = self.dropdown_layout() {
            match layout.item_at(mx, my) {
                Some(index) => self.apply_dropdown_selection(index),
                None => self.open_dropdown = None,
            }
            return EventResult::Consumed;
        }

        // Sidebar category clicks
        if mx < SIDEBAR_WIDTH {
            let list_y = HEADER_HEIGHT + SEARCH_BAR_HEIGHT + 16.0;
            if my >= list_y {
                let idx = ((my - list_y) / CATEGORY_ITEM_HEIGHT) as usize;
                if idx < SettingsCategory::ALL.len() {
                    let new_cat = SettingsCategory::ALL[idx];
                    self.current_category = new_cat;
                    self.current_page = new_cat.default_page();
                    return EventResult::Consumed;
                }
            }

            // Search bar click
            let search_y = HEADER_HEIGHT;
            if my >= search_y && my < search_y + SEARCH_BAR_HEIGHT {
                self.search_focused = true;
                return EventResult::Consumed;
            }
        }

        // Page tab clicks
        if (HEADER_HEIGHT - 20.0..HEADER_HEIGHT).contains(&my) {
            let pages = self.current_category.pages();
            let mut tab_x = SIDEBAR_WIDTH + CONTENT_PADDING;
            for page in pages {
                let label = page.label();
                let tab_width = page_tab_width(label);
                if mx >= tab_x && mx < tab_x + tab_width + 8.0 {
                    self.current_page = *page;
                    return EventResult::Consumed;
                }
                tab_x += tab_width + 8.0;
            }
        }

        // Content area clicks — delegate to page-specific handlers
        let content_x = SIDEBAR_WIDTH + CONTENT_PADDING;
        let right_x = content_x + 350.0;

        match self.current_page {
            SettingsPage::Display => {
                self.handle_display_click(mx, my, right_x);
            }
            SettingsPage::Sound => {
                self.handle_sound_click(mx, my, right_x);
            }
            SettingsPage::Themes => {
                self.handle_themes_click(mx, my, content_x);
            }
            SettingsPage::Colors => {
                self.handle_colors_click(mx, my, content_x);
            }
            SettingsPage::NetworkStatus => {
                self.handle_network_click(mx, my, content_x, right_x);
            }
            SettingsPage::Proxy => {
                self.handle_proxy_click(mx, my, right_x);
            }
            SettingsPage::UserAccounts | SettingsPage::LoginOptions => {
                self.handle_accounts_click(mx, my, right_x);
            }
            SettingsPage::Permissions => {
                self.handle_privacy_click(mx, my, right_x);
            }
            SettingsPage::Visual | SettingsPage::Audio | SettingsPage::Interaction => {
                self.handle_accessibility_click(mx, my, right_x);
            }
            SettingsPage::SystemUpdates => {
                self.handle_update_click(mx, my, right_x);
            }
            _ => {}
        }

        EventResult::Consumed
    }

    fn handle_hover(&mut self, mx: f32, my: f32) -> EventResult {
        // Sidebar hover
        if mx < SIDEBAR_WIDTH {
            let list_y = HEADER_HEIGHT + SEARCH_BAR_HEIGHT + 16.0;
            if my >= list_y {
                let idx = ((my - list_y) / CATEGORY_ITEM_HEIGHT) as usize;
                if idx < SettingsCategory::ALL.len() {
                    self.sidebar_hovered = Some(idx);
                } else {
                    self.sidebar_hovered = None;
                }
            } else {
                self.sidebar_hovered = None;
            }
            return EventResult::Consumed;
        }

        self.sidebar_hovered = None;
        EventResult::Ignored
    }

    // --- Page-specific click handlers ---

    fn handle_display_click(&mut self, _mx: f32, my: f32, right_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;
        // After monitor preview (section header + 120px preview + spacing + section header)
        let settings_start = base_y + 24.0 + 12.0 + 120.0 + SECTION_SPACING + 24.0 + 12.0;

        let _ = right_x;

        // Resolution row
        let res_y = settings_start;
        if my >= res_y && my < res_y + ITEM_HEIGHT {
            self.show_dropdown(DropdownId::Resolution);
            return;
        }

        // Refresh rate row
        let rate_y = res_y + ITEM_HEIGHT;
        if my >= rate_y && my < rate_y + ITEM_HEIGHT {
            self.show_dropdown(DropdownId::RefreshRate);
            return;
        }

        // Scale row
        let scale_y = rate_y + ITEM_HEIGHT;
        if my >= scale_y && my < scale_y + ITEM_HEIGHT {
            self.show_dropdown(DropdownId::Scale);
            return;
        }

        // Night light toggle
        let nl_section_y = scale_y + ITEM_HEIGHT + SECTION_SPACING + 24.0 + 12.0;
        if my >= nl_section_y && my < nl_section_y + ITEM_HEIGHT {
            self.night_light_enabled = !self.night_light_enabled;
        }
    }

    fn handle_sound_click(&mut self, _mx: f32, my: f32, _right_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;
        let section_y = base_y + 24.0 + 12.0;

        // Output device
        if my >= section_y && my < section_y + ITEM_HEIGHT {
            self.show_dropdown(DropdownId::OutputDevice);
            return;
        }

        // Mute toggle (third row in output section)
        let mute_y = section_y + ITEM_HEIGHT * 2.0;
        if my >= mute_y && my < mute_y + ITEM_HEIGHT {
            self.output_muted = !self.output_muted;
            return;
        }

        // System sounds toggle
        let sys_section_y = section_y + ITEM_HEIGHT * 3.0 + SECTION_SPACING * 2.0 + 72.0;
        if my >= sys_section_y && my < sys_section_y + ITEM_HEIGHT {
            self.system_sounds_enabled = !self.system_sounds_enabled;
        }
    }

    fn handle_themes_click(&mut self, mx: f32, my: f32, content_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;
        let cards_y = base_y + 24.0 + 12.0;
        let card_w = 140.0;
        let card_h = 100.0;
        let card_spacing = 16.0;

        // Theme mode cards
        if my >= cards_y && my < cards_y + card_h {
            for (idx, mode) in ThemeMode::ALL.iter().enumerate() {
                let cx = content_x + (idx as f32) * (card_w + card_spacing);
                if mx >= cx && mx < cx + card_w {
                    self.appearance.settings.theme_mode = *mode;
                    return;
                }
            }
        }

        // Transparency levels
        let effects_y = cards_y + card_h + SECTION_SPACING + 24.0 + 12.0;
        let pill_x = content_x + PILL_ROW_X;
        if let Some(idx) = pill_row_hit(mx, my, pill_x, effects_y, TransparencyLevel::ALL.len())
            && let Some(level) = TransparencyLevel::ALL.get(idx)
        {
            self.appearance.settings.transparency = *level;
            return;
        }

        // Animation speed
        let anim_y = effects_y + ITEM_HEIGHT;
        if let Some(idx) = pill_row_hit(mx, my, pill_x, anim_y, AnimationSpeed::ALL.len())
            && let Some(speed) = AnimationSpeed::ALL.get(idx)
        {
            self.appearance.settings.animation_speed = *speed;
        }
    }

    fn handle_colors_click(&mut self, mx: f32, my: f32, content_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;
        // Section header + description text
        let grid_y = base_y + 24.0 + 12.0 + 28.0;
        let swatch_size = 36.0;
        let swatch_spacing = 10.0;
        let cols = 6;

        // Color swatches
        for (idx, accent) in AccentColor::presets().iter().enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            let sx = content_x + (col as f32) * (swatch_size + swatch_spacing);
            let sy = grid_y + (row as f32) * (swatch_size + swatch_spacing);

            if mx >= sx && mx < sx + swatch_size && my >= sy && my < sy + swatch_size {
                self.appearance.settings.accent_color = *accent;
                return;
            }
        }
    }

    fn handle_network_click(&mut self, mx: f32, my: f32, content_x: f32, right_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;
        let adapters_y = base_y + 24.0 + 12.0;

        let _ = (right_x, mx);

        // Adapter list clicks
        for (idx, _adapter) in self.adapters.iter().enumerate() {
            let row_y = adapters_y + (idx as f32) * (ITEM_HEIGHT + 4.0);
            if my >= row_y && my < row_y + ITEM_HEIGHT {
                self.selected_adapter = idx;
                return;
            }
        }

        // IP config dropdown
        let ip_section_y = adapters_y
            + (self.adapters.len() as f32) * (ITEM_HEIGHT + 4.0)
            + SECTION_SPACING
            + 24.0
            + 12.0;
        if my >= ip_section_y && my < ip_section_y + ITEM_HEIGHT && mx >= content_x + 350.0 {
            self.show_dropdown(DropdownId::IpConfig);
        }
    }

    fn handle_proxy_click(&mut self, _mx: f32, my: f32, _right_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;
        let toggle_y = base_y + 24.0 + 12.0;

        if my >= toggle_y && my < toggle_y + ITEM_HEIGHT {
            self.proxy_enabled = !self.proxy_enabled;
        }
    }

    fn handle_accounts_click(&mut self, _mx: f32, my: f32, _right_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;

        match self.current_page {
            SettingsPage::LoginOptions => {
                let toggle_y = base_y + 24.0 + 12.0;
                if my >= toggle_y && my < toggle_y + ITEM_HEIGHT {
                    self.auto_login_enabled = !self.auto_login_enabled;
                }
            }
            _ => {
                // Account list selection
                let list_y = base_y + 24.0 + 12.0;
                for (idx, _) in self.user_accounts.iter().enumerate() {
                    let row_y = list_y + (idx as f32) * 64.0;
                    if my >= row_y && my < row_y + 60.0 {
                        self.selected_account = idx;
                        return;
                    }
                }
            }
        }
    }

    fn handle_privacy_click(&mut self, mx: f32, my: f32, right_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;
        let section_y = base_y + 24.0 + 12.0;

        // Location master toggle
        if my >= section_y
            && my < section_y + ITEM_HEIGHT
            && mx >= right_x
            && mx < right_x + TOGGLE_WIDTH
        {
            self.location_enabled = !self.location_enabled;
            return;
        }

        // Per-app toggles for location
        if self.location_enabled {
            let mut y = section_y + ITEM_HEIGHT;
            for app in &mut self.location_apps {
                if my >= y
                    && my < y + ITEM_HEIGHT - 8.0
                    && mx >= right_x
                    && mx < right_x + TOGGLE_WIDTH
                {
                    app.allowed = !app.allowed;
                    return;
                }
                y += ITEM_HEIGHT - 8.0;
            }
        }
    }

    fn handle_accessibility_click(&mut self, mx: f32, my: f32, right_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;

        match self.current_page {
            SettingsPage::Audio => {
                let section_y = base_y + 24.0 + 12.0;
                // Mono audio toggle
                if my >= section_y
                    && my < section_y + ITEM_HEIGHT
                    && mx >= right_x
                    && mx < right_x + TOGGLE_WIDTH
                {
                    self.mono_audio = !self.mono_audio;
                    return;
                }
                // Visual alerts toggle
                let va_y = section_y + ITEM_HEIGHT;
                if my >= va_y
                    && my < va_y + ITEM_HEIGHT
                    && mx >= right_x
                    && mx < right_x + TOGGLE_WIDTH
                {
                    self.visual_alerts = !self.visual_alerts;
                    return;
                }
                // Narrator toggle
                let narrator_section_y = va_y + ITEM_HEIGHT + SECTION_SPACING + 24.0 + 12.0;
                if my >= narrator_section_y
                    && my < narrator_section_y + ITEM_HEIGHT
                    && mx >= right_x
                    && mx < right_x + TOGGLE_WIDTH
                {
                    self.narrator_enabled = !self.narrator_enabled;
                }
            }
            SettingsPage::Interaction => {
                let section_y = base_y + 24.0 + 12.0;
                let mut y = section_y;
                // Sticky keys
                if my >= y && my < y + ITEM_HEIGHT && mx >= right_x && mx < right_x + TOGGLE_WIDTH {
                    self.sticky_keys = !self.sticky_keys;
                    return;
                }
                y += ITEM_HEIGHT + 12.0;
                // Filter keys
                if my >= y && my < y + ITEM_HEIGHT && mx >= right_x && mx < right_x + TOGGLE_WIDTH {
                    self.filter_keys = !self.filter_keys;
                    return;
                }
                y += ITEM_HEIGHT + 12.0;
                // Toggle keys
                if my >= y && my < y + ITEM_HEIGHT && mx >= right_x && mx < right_x + TOGGLE_WIDTH {
                    self.toggle_keys = !self.toggle_keys;
                    return;
                }
                y += ITEM_HEIGHT + 12.0;
                // On-screen keyboard
                if my >= y && my < y + ITEM_HEIGHT && mx >= right_x && mx < right_x + TOGGLE_WIDTH {
                    self.onscreen_keyboard = !self.onscreen_keyboard;
                }
            }
            _ => {
                // Visual page
                let section_y = base_y + 24.0 + 12.0;
                let mut y = section_y + ITEM_HEIGHT; // skip text size slider row
                y += 8.0; // range labels

                // High contrast toggle
                if my >= y && my < y + ITEM_HEIGHT && mx >= right_x && mx < right_x + TOGGLE_WIDTH {
                    self.high_contrast = !self.high_contrast;
                    return;
                }
                y += ITEM_HEIGHT;
                // Cursor size dropdown
                if my >= y && my < y + ITEM_HEIGHT {
                    self.show_dropdown(DropdownId::CursorSize);
                    return;
                }
                y += ITEM_HEIGHT;
                // Reduce animations
                if my >= y && my < y + ITEM_HEIGHT && mx >= right_x && mx < right_x + TOGGLE_WIDTH {
                    self.reduce_animations = !self.reduce_animations;
                    return;
                }
                y += ITEM_HEIGHT + SECTION_SPACING + 24.0 + 12.0;
                // Color filter dropdown
                if my >= y && my < y + ITEM_HEIGHT {
                    self.show_dropdown(DropdownId::ColorFilter);
                    return;
                }
                y += ITEM_HEIGHT;
                // Reduce transparency
                if my >= y && my < y + ITEM_HEIGHT && mx >= right_x && mx < right_x + TOGGLE_WIDTH {
                    self.reduce_transparency = !self.reduce_transparency;
                }
            }
        }
    }

    fn handle_update_click(&mut self, _mx: f32, my: f32, right_x: f32) {
        let base_y = HEADER_HEIGHT + 8.0;

        // Version info section takes 72px, then check button at y + 72
        let check_btn_y = base_y + 24.0 + 12.0 + 72.0;
        if my >= check_btn_y && my < check_btn_y + 32.0 {
            self.checking_for_updates = !self.checking_for_updates;
            return;
        }

        // Auto-update toggle
        let prefs_y = check_btn_y + 44.0 + SECTION_SPACING + 24.0 + 12.0;
        if my >= prefs_y && my < prefs_y + ITEM_HEIGHT {
            self.auto_update_enabled = !self.auto_update_enabled;
        }

        let _ = right_x;
    }

    /// Opens `id`'s dropdown, scrolled so the current choice is on screen.
    ///
    /// The reveal matters for the long lists: opening the resolution dropdown
    /// on a short window and finding no ticked item anywhere in it is not a
    /// list of choices, it is a list of choices with the answer torn off.
    pub fn show_dropdown(&mut self, id: DropdownId) {
        self.open_dropdown = Some(id);
        self.dropdown_scroll = 0;
        // Ask for the layout now that the id is set: how far to scroll depends
        // on how many items fit, which depends on which dropdown this is.
        if let Some(layout) = self.dropdown_layout() {
            let capacity = layout.window.count;
            if capacity > 0 && layout.selected >= capacity {
                self.dropdown_scroll = layout
                    .selected
                    .saturating_add(1)
                    .saturating_sub(capacity);
            }
        }
    }

    /// Scrolls the open dropdown by `delta` items. No-op when none is open.
    pub fn scroll_dropdown_by(&mut self, delta: isize) {
        if self.open_dropdown.is_some() {
            self.dropdown_scroll = scroll_window::shift(self.dropdown_scroll, delta);
        }
    }

    /// Apply a dropdown selection.
    pub fn apply_dropdown_selection(&mut self, index: usize) {
        let dropdown_id = match self.open_dropdown {
            Some(id) => id,
            None => return,
        };

        match dropdown_id {
            DropdownId::Resolution => {
                if index < RESOLUTIONS.len() {
                    self.resolution_index = index;
                }
            }
            DropdownId::RefreshRate => {
                if index < REFRESH_RATES.len() {
                    self.refresh_rate_index = index;
                }
            }
            DropdownId::Scale => {
                if let Some(scale) = ScalePercent::ALL.get(index) {
                    self.scale = *scale;
                }
            }
            DropdownId::OutputDevice => {
                if index < self.output_devices.len() {
                    self.output_device_index = index;
                }
            }
            DropdownId::InputDevice => {
                if index < self.input_devices.len() {
                    self.input_device_index = index;
                }
            }
            DropdownId::IpConfig => {
                self.ip_config_mode = if index == 0 {
                    IpConfigMode::Dhcp
                } else {
                    IpConfigMode::Static
                };
            }
            DropdownId::DiagnosticLevel => {
                if let Some(level) = DiagnosticLevel::ALL.get(index) {
                    self.diagnostic_level = *level;
                }
            }
            DropdownId::ColorFilter => {
                if let Some(filter) = ColorFilter::ALL.get(index) {
                    self.color_filter = *filter;
                }
            }
            DropdownId::CursorSize => {
                if let Some(size) = CursorSize::ALL.get(index) {
                    self.cursor_size = *size;
                }
            }
            DropdownId::NarratorVerbosity => {
                if let Some(verbosity) = NarratorVerbosity::ALL.get(index) {
                    self.narrator_verbosity = *verbosity;
                }
            }
        }

        self.open_dropdown = None;
    }

    /// Check if a category/page matches the current search query.
    pub fn matches_search(&self, text: &str) -> bool {
        if self.search_query.is_empty() {
            return true;
        }
        let query_lower = self.search_query.to_lowercase();
        let text_lower = text.to_lowercase();
        if text_lower.contains(&query_lower) {
            return true;
        }
        // Be punctuation/whitespace-insensitive so a query like "wifi" matches a
        // label like "Wi-Fi" (and "nightlight" matches "Night Light"). Compare
        // with all non-alphanumeric characters stripped from both sides.
        let strip = |s: &str| {
            s.chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        };
        let stripped_query = strip(&query_lower);
        // An all-punctuation query strips to empty; don't let that match
        // everything — fall back to "no match" in that case.
        !stripped_query.is_empty() && strip(&text_lower).contains(&stripped_query)
    }

    /// Get filtered categories based on search query.
    pub fn filtered_categories(&self) -> Vec<SettingsCategory> {
        if self.search_query.is_empty() {
            return SettingsCategory::ALL.to_vec();
        }
        SettingsCategory::ALL
            .iter()
            .filter(|cat| {
                // Match category name
                if self.matches_search(cat.label()) {
                    return true;
                }
                // Match any page name within category
                cat.pages().iter().any(|p| self.matches_search(p.label()))
            })
            .copied()
            .collect()
    }
}

// ============================================================================
// Application entry point
// ============================================================================

fn main() {
    let mut state = SettingsState::new();
    // The Personalization pages open on what the user actually has, which is
    // the same file the desktop shell paints from.
    state.load_appearance();

    // In a real Slate OS environment, this would enter the compositor event loop.
    // For now, render one frame to verify the UI builds correctly.
    let tree = state.render();

    // The render tree would be submitted to the compositor.
    // For a basic sanity check, confirm we produced output.
    assert!(!tree.is_empty(), "Settings UI must produce render commands");

    // Simulate a resize event
    let resize_event = Event::Resize {
        width: 1400,
        height: 900,
    };
    let result = state.handle_event(&resize_event);
    assert_eq!(result, EventResult::Consumed);

    // Re-render after resize
    let tree2 = state.render();
    assert!(!tree2.is_empty(), "Settings UI must render after resize");
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use appearance::AppearanceSettings;

    // ---- Measured widths ----

    #[test]
    fn page_tabs_fit_their_labels() {
        for category in SettingsCategory::ALL {
            for page in category.pages() {
                let label = page.label();
                assert!(
                    page_tab_width(label)
                        >= text::measure(label, 13.0, FontWeightHint::Regular) + 16.0,
                    "{label} overflows its tab"
                );
            }
        }
    }

    #[test]
    fn the_tab_hit_target_is_the_tab_that_was_drawn() {
        // Renderer and click handler share one function, so a click at the
        // centre of the nth tab lands on the nth page whatever the labels are.
        let mut state = SettingsState::new();
        let pages: Vec<SettingsPage> = state.current_category.pages().to_vec();
        let mut tab_x = SIDEBAR_WIDTH + CONTENT_PADDING;
        for page in &pages {
            let w = page_tab_width(page.label());
            let centre = tab_x + w / 2.0;
            state.handle_click(centre, HEADER_HEIGHT - 10.0);
            tab_x += w + 8.0;
        }
        // The last click leaves us on the last page.
        assert_eq!(Some(state.current_page), pages.last().copied());
    }

    #[test]
    fn a_tab_is_not_sized_by_byte_length() {
        let ascii = page_tab_width("Anzeige");
        let accented = page_tab_width("Anzeig\u{e9}");
        assert!(
            (ascii - accented).abs() < 4.0,
            "an accent changed the tab width by more than a glyph's worth"
        );
    }

    #[test]
    fn test_initial_state() {
        let state = SettingsState::new();
        assert_eq!(state.current_category, SettingsCategory::System);
        assert_eq!(state.current_page, SettingsPage::Display);
        assert!(state.search_query.is_empty());
        assert!(!state.night_light_enabled);
        assert_eq!(state.appearance.settings.theme_mode, ThemeMode::Dark);
    }

    #[test]
    fn test_render_produces_commands() {
        let state = SettingsState::new();
        let tree = state.render();
        assert!(!tree.is_empty());
        // Should have at minimum: background rect + sidebar + header + content
        assert!(tree.len() > 20);
    }

    #[test]
    fn test_category_navigation() {
        let mut state = SettingsState::new();
        let down = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        state.handle_event(&down);
        assert_eq!(state.current_category, SettingsCategory::Network);
        assert_eq!(state.current_page, SettingsPage::NetworkStatus);
    }

    #[test]
    fn test_page_tab_cycle() {
        let mut state = SettingsState::new();
        assert_eq!(state.current_page, SettingsPage::Display);

        let tab = Event::Key(KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        state.handle_event(&tab);
        assert_eq!(state.current_page, SettingsPage::Sound);

        state.handle_event(&tab);
        assert_eq!(state.current_page, SettingsPage::Notifications);
    }

    #[test]
    fn test_toggle_night_light() {
        let mut state = SettingsState::new();
        assert!(!state.night_light_enabled);
        state.night_light_enabled = true;

        // Render with night light on should show temperature slider
        let tree = state.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_theme_mode_selection() {
        let mut state = SettingsState::new();
        assert_eq!(state.appearance.settings.theme_mode, ThemeMode::Dark);
        state.appearance.settings.theme_mode = ThemeMode::Light;
        let tree = state.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_dropdown_selection() {
        let mut state = SettingsState::new();
        state.open_dropdown = Some(DropdownId::Resolution);
        state.apply_dropdown_selection(0); // 3840x2160
        assert_eq!(state.resolution_index, 0);
        assert!(state.open_dropdown.is_none());
    }

    #[test]
    fn test_dropdown_selection_out_of_bounds() {
        let mut state = SettingsState::new();
        let original = state.resolution_index;
        state.open_dropdown = Some(DropdownId::Resolution);
        state.apply_dropdown_selection(999); // out of bounds
        assert_eq!(state.resolution_index, original);
    }

    #[test]
    fn test_search_filter() {
        let state = SettingsState::new();
        assert!(state.matches_search("Display"));
        assert!(state.matches_search("display")); // case insensitive
    }

    #[test]
    fn test_search_filter_categories() {
        let mut state = SettingsState::new();
        state.search_query = "wifi".to_string();
        let filtered = state.filtered_categories();
        assert!(filtered.contains(&SettingsCategory::Network));
        assert!(!filtered.contains(&SettingsCategory::System));
    }

    #[test]
    fn test_resize_event() {
        let mut state = SettingsState::new();
        let evt = Event::Resize {
            width: 1600,
            height: 1000,
        };
        let result = state.handle_event(&evt);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(state.window_width, 1600.0);
        assert_eq!(state.window_height, 1000.0);
    }

    #[test]
    fn test_escape_closes_dropdown() {
        let mut state = SettingsState::new();
        state.open_dropdown = Some(DropdownId::Scale);

        let esc = Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        let result = state.handle_event(&esc);
        assert_eq!(result, EventResult::Consumed);
        assert!(state.open_dropdown.is_none());
    }

    #[test]
    fn test_sidebar_click() {
        let mut state = SettingsState::new();
        // Click on the third category item (Personalization)
        let list_y = HEADER_HEIGHT + SEARCH_BAR_HEIGHT + 16.0;
        let click_y = list_y + 2.0 * CATEGORY_ITEM_HEIGHT + 10.0;
        let click = Event::Mouse(MouseEvent {
            x: 100.0,
            y: click_y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        state.handle_event(&click);
        assert_eq!(state.current_category, SettingsCategory::Personalization);
        assert_eq!(state.current_page, SettingsPage::Themes);
    }

    #[test]
    fn test_all_pages_render() {
        let mut state = SettingsState::new();
        for category in SettingsCategory::ALL {
            state.current_category = *category;
            for page in category.pages() {
                state.current_page = *page;
                let tree = state.render();
                assert!(!tree.is_empty(), "Page {:?} must render", page);
            }
        }
    }

    #[test]
    fn test_network_adapter_selection() {
        let mut state = SettingsState::new();
        assert_eq!(state.selected_adapter, 0);
        state.selected_adapter = 1;
        state.current_page = SettingsPage::NetworkStatus;
        let tree = state.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_proxy_toggle() {
        let mut state = SettingsState::new();
        assert!(!state.proxy_enabled);
        state.proxy_enabled = true;
        state.current_page = SettingsPage::Proxy;
        let tree = state.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_accent_color_selection() {
        let mut state = SettingsState::new();
        assert_eq!(state.appearance.settings.accent_color, AccentColor::Blue);
        state.appearance.settings.accent_color = AccentColor::Green;
        state.current_page = SettingsPage::Colors;
        let tree = state.render();
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_volume_bounds() {
        let state = SettingsState::new();
        assert!(state.output_volume <= 100);
        assert!(state.input_volume <= 100);
        for app in &state.app_volumes {
            assert!(app.volume <= 100);
        }
    }

    #[test]
    fn test_resolution_labels() {
        for res in RESOLUTIONS {
            let label = res.label();
            assert!(label.contains('x'));
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn test_ip_config_mode_toggle() {
        let mut state = SettingsState::new();
        state.open_dropdown = Some(DropdownId::IpConfig);
        state.apply_dropdown_selection(1); // Static
        assert_eq!(state.ip_config_mode, IpConfigMode::Static);
        state.open_dropdown = Some(DropdownId::IpConfig);
        state.apply_dropdown_selection(0); // DHCP
        assert_eq!(state.ip_config_mode, IpConfigMode::Dhcp);
    }

    #[test]
    fn test_hover_sidebar() {
        let mut state = SettingsState::new();
        let list_y = HEADER_HEIGHT + SEARCH_BAR_HEIGHT + 16.0;
        let hover = Event::Mouse(MouseEvent {
            x: 100.0,
            y: list_y + 10.0,
            kind: MouseEventKind::Move,
        });
        state.handle_event(&hover);
        assert_eq!(state.sidebar_hovered, Some(0));
    }

    #[test]
    fn test_open_dropdown_renders() {
        let mut state = SettingsState::new();
        state.open_dropdown = Some(DropdownId::Resolution);
        let tree = state.render();
        // Should have more commands when dropdown is open (shadow + background + items)
        assert!(tree.len() > 30);
    }

    // ---- Personalization: the shared appearance model ----
    //
    // These drive the click handlers directly rather than through
    // `handle_event`, which would write to the user's real `appearance.yaml`.
    // The write itself is covered by `appearance`'s own config tests, which
    // point $HOME at a temporary directory; what needs testing here is that
    // the panel's geometry and the model agree.

    /// Where the Themes page draws its pill rows, mirroring the layout in
    /// `render_themes_page` / `handle_themes_click`.
    fn themes_effects_y() -> f32 {
        let cards_y = HEADER_HEIGHT + 8.0 + 24.0 + 12.0;
        cards_y + 100.0 + SECTION_SPACING + 24.0 + 12.0
    }

    #[test]
    fn test_theme_mode_card_click_sets_the_mode() {
        let mut state = SettingsState::new();
        let cards_y = HEADER_HEIGHT + 8.0 + 24.0 + 12.0;
        // Second card: Light, since ThemeMode::ALL is Dark, Light, System.
        state.handle_themes_click(100.0 + 156.0, cards_y + 10.0, 100.0);
        assert_eq!(state.appearance.settings.theme_mode, ThemeMode::Light);
    }

    #[test]
    fn test_every_transparency_level_can_be_picked() {
        // The row this replaced was an on/off switch, which could not express
        // Subtle or Moderate at all.
        let y = themes_effects_y();
        for (idx, level) in TransparencyLevel::ALL.iter().enumerate() {
            let mut state = SettingsState::new();
            let px = 100.0 + PILL_ROW_X + (idx as f32) * PILL_PITCH + 4.0;
            state.handle_themes_click(px, y + PILL_INSET_Y + 4.0, 100.0);
            assert_eq!(
                state.appearance.settings.transparency, *level,
                "pill {idx} should select {}",
                level.label()
            );
        }
    }

    #[test]
    fn test_every_animation_speed_can_be_picked() {
        let y = themes_effects_y() + ITEM_HEIGHT;
        for (idx, speed) in AnimationSpeed::ALL.iter().enumerate() {
            let mut state = SettingsState::new();
            let px = 100.0 + PILL_ROW_X + (idx as f32) * PILL_PITCH + 4.0;
            state.handle_themes_click(px, y + PILL_INSET_Y + 4.0, 100.0);
            assert_eq!(state.appearance.settings.animation_speed, *speed);
        }
    }

    #[test]
    fn test_a_click_in_the_gap_between_pills_changes_nothing() {
        let mut state = SettingsState::new();
        let y = themes_effects_y();
        // Between the first and second pill.
        let gap = 100.0 + PILL_ROW_X + PILL_WIDTH + 2.0;
        state.handle_themes_click(gap, y + PILL_INSET_Y + 4.0, 100.0);
        assert_eq!(
            state.appearance.settings.transparency,
            AppearanceSettings::default().transparency
        );
    }

    #[test]
    fn test_every_accent_swatch_can_be_picked() {
        // The old page stored the choice as an index into a local twelve-colour
        // array; the model has fourteen named accents and this walks all of
        // them, so a swatch that cannot be reached is a failing test rather
        // than a colour nobody can select.
        let grid_y = HEADER_HEIGHT + 8.0 + 24.0 + 12.0 + 28.0;
        for (idx, accent) in AccentColor::presets().iter().enumerate() {
            let mut state = SettingsState::new();
            let sx = 100.0 + ((idx % 6) as f32) * 46.0 + 4.0;
            let sy = grid_y + ((idx / 6) as f32) * 46.0 + 4.0;
            state.handle_colors_click(sx, sy, 100.0);
            assert_eq!(
                state.appearance.settings.accent_color, *accent,
                "swatch {idx} should select {}",
                accent.label()
            );
        }
    }

    #[test]
    fn test_the_swatches_show_the_colors_of_the_mode_in_use() {
        // A light-mode swatch drawn in the dark accent would promise a colour
        // the user will not get: the light palette is a darkened set chosen to
        // stay readable on a light background.
        let mut state = SettingsState::new();
        state.current_page = SettingsPage::Colors;
        state.appearance.settings.theme_mode = ThemeMode::Light;
        let light = state.render();
        state.appearance.settings.theme_mode = ThemeMode::Dark;
        let dark = state.render();
        assert_ne!(light.len(), 0);
        assert_ne!(
            format!("{light:?}"),
            format!("{dark:?}"),
            "the accent grid should not paint identically in both modes"
        );
    }

    #[test]
    fn test_a_click_that_changes_an_accent_reaches_the_file() {
        // The end-to-end claim of the whole rewiring: what this app writes is
        // what the desktop shell reads. Runs against a scratch configuration
        // directory, so it exercises the real save without touching anyone's
        // `~/.config/slateos`.
        appearance::config::testing::with_scratch_config("settings-accent", |root| {
            let mut state = SettingsState::new();
            state.current_category = SettingsCategory::Personalization;
            state.current_page = SettingsPage::Colors;

            // Third swatch of the first row — Teal, per AccentColor::presets.
            let grid_y = HEADER_HEIGHT + 8.0 + 24.0 + 12.0 + 28.0;
            let content_x = SIDEBAR_WIDTH + CONTENT_PADDING;
            let evt = Event::Mouse(MouseEvent {
                x: content_x + 2.0 * 46.0 + 4.0,
                y: grid_y + 4.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            });
            state.handle_event(&evt);
            assert_eq!(state.appearance.settings.accent_color, AccentColor::Teal);

            let path = appearance::config::testing::scratch_path(root, appearance::CONFIG_NAME);
            assert!(path.is_file(), "the click should have written {path:?}");

            // Read it back the way the shell does.
            let saved = AppearanceSettings::read_from(&appearance::config::load(
                appearance::CONFIG_NAME,
            ));
            assert_eq!(saved.accent_color, AccentColor::Teal);
        });
    }

    #[test]
    fn test_an_appearance_change_is_what_triggers_a_save() {
        // The save is driven by comparing the settings before and after the
        // event, so a control added later cannot forget to request one. Guard
        // the converse here: an event that touches nothing must not decide the
        // settings changed.
        let mut state = SettingsState::new();
        let before = state.appearance.settings.clone();
        let evt = Event::Resize {
            width: 1200,
            height: 800,
        };
        state.dispatch_event(&evt);
        assert_eq!(state.appearance.settings, before);
    }

    // ---- Dropdowns: the popup you can see and the popup you can click ----
    //
    // These dropdowns could be opened but not used. The popup's geometry
    // existed only inside `render_open_dropdown`, so the click handler had
    // nothing to test a click against and settled for
    //
    //     // For simplicity, any click closes the dropdown
    //
    // which meant `apply_dropdown_selection` -- correct and complete -- was
    // reachable only from the tests below it. The popup was also unbounded:
    // `popup_h` was `item_count * 36 + 8` with no reference to the window, and
    // `dropdown_scroll` was read by nothing, so a popup taller than the window
    // ran off the bottom with no way to reach what was down there.
    //
    // Every test here asks about the geometry the *renderer* produced, because
    // a hit-test verified against its own arithmetic verifies nothing.

    /// The item labels the open dropdown actually drew, top to bottom.
    ///
    /// Scoped to the commands the popup itself emitted — everything from its
    /// drop shadow onwards — rather than to an x/y box, so that moving the
    /// popup cannot silently turn this into a filter that matches nothing.
    fn drawn_dropdown_items(state: &SettingsState) -> Vec<String> {
        let tree = state.render();
        let start = tree
            .commands
            .iter()
            .position(|c| matches!(c, RenderCommand::BoxShadow { .. }))
            .expect("an open dropdown draws a shadow");
        tree.commands
            .get(start..)
            .unwrap_or_default()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text, font_size, ..
                } if (font_size - 13.0).abs() < 0.01 => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The "N more" line the popup draws when it is hiding items, if any.
    fn dropdown_more_line(state: &SettingsState) -> Option<String> {
        let tree = state.render();
        let start = tree
            .commands
            .iter()
            .position(|c| matches!(c, RenderCommand::BoxShadow { .. }))?;
        tree.commands
            .get(start..)
            .unwrap_or_default()
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    text, font_size, ..
                } if (font_size - 11.0).abs() < 0.01 && text.ends_with(" more") => {
                    Some(text.clone())
                }
                _ => None,
            })
    }

    fn click(state: &mut SettingsState, x: f32, y: f32) -> EventResult {
        state.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }))
    }

    /// The regression test for the hit-test that did not exist. Before the fix
    /// this passed only because nothing checked what a click did.
    #[test]
    fn clicking_a_dropdown_item_chooses_it() {
        for index in 0..RESOLUTIONS.len() {
            let mut state = SettingsState::new();
            state.resolution_index = usize::MAX; // no valid choice yet
            state.show_dropdown(DropdownId::Resolution);
            let layout = state.dropdown_layout().expect("a dropdown is open");
            let row = index
                .checked_sub(layout.window.start)
                .expect("the whole list fits in the default window");
            let y = layout.row_top(row) + DROPDOWN_ITEM_HEIGHT / 2.0;
            assert_eq!(
                click(&mut state, layout.x + 20.0, y),
                EventResult::Consumed
            );
            assert_eq!(
                state.resolution_index, index,
                "clicking item {index} should have chosen it"
            );
            assert!(state.open_dropdown.is_none(), "choosing closes the popup");
        }
    }

    /// The property the two halves of the fix exist to guarantee: whatever the
    /// renderer drew at a row, the hit-test names that same item.
    #[test]
    fn the_hit_test_names_the_item_that_was_drawn_under_the_pointer() {
        for height in [800.0_f32, 500.0, 320.0, 200.0] {
            let mut state = SettingsState::new();
            state.window_height = height;
            state.show_dropdown(DropdownId::Resolution);
            let layout = state.dropdown_layout().expect("a dropdown is open");
            let drawn = drawn_dropdown_items(&state);
            assert_eq!(
                drawn.len(),
                layout.window.count,
                "at {height}px the popup drew {} rows but claims {}",
                drawn.len(),
                layout.window.count
            );
            for (row, label) in drawn.iter().enumerate() {
                let y = layout.row_top(row) + DROPDOWN_ITEM_HEIGHT / 2.0;
                let index = layout
                    .item_at(layout.x + 20.0, y)
                    .unwrap_or_else(|| panic!("row {row} at y={y} hit nothing"));
                assert_eq!(
                    layout.items.get(index),
                    Some(label),
                    "at {height}px, row {row} shows {label} but hit-tests to \
                     item {index}"
                );
            }
        }
    }

    #[test]
    fn clicking_outside_an_open_dropdown_closes_it_without_choosing() {
        let mut state = SettingsState::new();
        state.resolution_index = 2;
        state.show_dropdown(DropdownId::Resolution);
        let layout = state.dropdown_layout().expect("a dropdown is open");
        // Well to the left of the popup, over the sidebar.
        assert_eq!(click(&mut state, 10.0, layout.y + 10.0), EventResult::Consumed);
        assert!(state.open_dropdown.is_none());
        assert_eq!(state.resolution_index, 2, "a dismissal is not a choice");
    }

    #[test]
    fn clicking_the_popups_padding_chooses_nothing() {
        let mut state = SettingsState::new();
        state.resolution_index = 2;
        state.show_dropdown(DropdownId::Resolution);
        let layout = state.dropdown_layout().expect("a dropdown is open");
        // Inside the popup's width, but above its first row.
        assert!(layout.item_at(layout.x + 20.0, layout.y + 1.0).is_none());
        click(&mut state, layout.x + 20.0, layout.y + 1.0);
        assert_eq!(state.resolution_index, 2);
        assert!(state.open_dropdown.is_none());
    }

    #[test]
    fn a_dropdown_popup_never_runs_off_the_window() {
        for height in [1000.0_f32, 800.0, 600.0, 400.0, 260.0, 120.0] {
            for id in [
                DropdownId::Resolution,
                DropdownId::RefreshRate,
                DropdownId::Scale,
                DropdownId::CursorSize,
            ] {
                let mut state = SettingsState::new();
                state.window_height = height;
                state.show_dropdown(id);
                let layout = state.dropdown_layout().expect("a dropdown is open");
                assert!(
                    layout.y >= 0.0,
                    "{id:?} at {height}px starts above the window at {}",
                    layout.y
                );
                assert!(
                    layout.y + layout.height <= height,
                    "{id:?} at {height}px reaches {} in a {height}px window",
                    layout.y + layout.height
                );
                // And nothing is drawn past the popup's own bottom edge.
                let rows_bottom = layout.row_top(layout.window.count);
                assert!(
                    rows_bottom <= layout.y + layout.height + 0.01,
                    "{id:?} at {height}px draws rows to {rows_bottom}, past {}",
                    layout.y + layout.height
                );
            }
        }
    }

    #[test]
    fn a_dropdown_too_tall_for_the_window_says_what_it_is_hiding() {
        let mut state = SettingsState::new();
        state.window_height = 200.0;
        state.show_dropdown(DropdownId::Resolution);
        let layout = state.dropdown_layout().expect("a dropdown is open");
        assert!(
            layout.window.count < RESOLUTIONS.len(),
            "a 200px window cannot show all {} resolutions",
            RESOLUTIONS.len()
        );
        assert_eq!(
            dropdown_more_line(&state),
            Some(format!("{} more", layout.hidden())),
            "a popup with items below the fold must say how many"
        );
    }

    #[test]
    fn a_dropdown_that_fits_says_nothing_and_shows_everything() {
        let state = {
            let mut s = SettingsState::new();
            s.show_dropdown(DropdownId::Resolution);
            s
        };
        assert_eq!(drawn_dropdown_items(&state).len(), RESOLUTIONS.len());
        assert_eq!(dropdown_more_line(&state), None);
    }

    #[test]
    fn every_dropdown_item_is_reachable_by_scrolling() {
        let mut state = SettingsState::new();
        state.window_height = 200.0;
        state.show_dropdown(DropdownId::Resolution);

        let mut seen: Vec<String> = Vec::new();
        for _ in 0..RESOLUTIONS.len() {
            for label in drawn_dropdown_items(&state) {
                if !seen.contains(&label) {
                    seen.push(label);
                }
            }
            state.scroll_dropdown_by(1);
        }
        let all: Vec<String> = RESOLUTIONS.iter().map(|r| r.label()).collect();
        for label in &all {
            assert!(seen.contains(label), "{label} was never reachable");
        }
    }

    #[test]
    fn a_dropdown_scrolled_past_the_end_shows_its_last_page() {
        let mut state = SettingsState::new();
        state.window_height = 200.0;
        state.show_dropdown(DropdownId::Resolution);
        let capacity = state
            .dropdown_layout()
            .expect("a dropdown is open")
            .window
            .count;
        for offset in [RESOLUTIONS.len(), RESOLUTIONS.len() + 5, usize::MAX] {
            state.dropdown_scroll = offset;
            let layout = state.dropdown_layout().expect("a dropdown is open");
            assert_eq!(
                layout.window.count, capacity,
                "offset {offset} left the popup part-empty"
            );
            assert_eq!(
                layout.window.end(),
                RESOLUTIONS.len(),
                "offset {offset} should pin to the last page"
            );
        }
    }

    #[test]
    fn scrolling_a_dropdown_up_from_the_top_stays_at_the_top() {
        let mut state = SettingsState::new();
        state.window_height = 200.0;
        state.show_dropdown(DropdownId::Resolution);
        state.dropdown_scroll = 0;
        state.scroll_dropdown_by(-100);
        assert_eq!(state.dropdown_scroll, 0);
    }

    /// Opening a dropdown whose choice is below the fold must not show a list
    /// with nothing ticked in it.
    #[test]
    fn a_dropdown_opens_showing_the_choice_it_already_has() {
        for index in 0..RESOLUTIONS.len() {
            let mut state = SettingsState::new();
            state.window_height = 200.0;
            state.resolution_index = index;
            state.show_dropdown(DropdownId::Resolution);
            let layout = state.dropdown_layout().expect("a dropdown is open");
            assert!(
                (layout.window.start..layout.window.end()).contains(&index),
                "choice {index} is outside the opened window {:?}",
                layout.window
            );
            let label = RESOLUTIONS
                .get(index)
                .map(|r| r.label())
                .expect("a resolution exists at this index");
            assert!(
                drawn_dropdown_items(&state).contains(&label),
                "choice {index} ({label}) was not drawn"
            );
        }
    }

    #[test]
    fn the_wheel_scrolls_an_open_dropdown() {
        let mut state = SettingsState::new();
        state.window_height = 200.0;
        state.show_dropdown(DropdownId::Resolution);
        state.dropdown_scroll = 0;

        let wheel = |dy: f32| {
            Event::Mouse(MouseEvent {
                x: 700.0,
                y: 300.0,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })
        };
        assert_eq!(state.handle_event(&wheel(-1.0)), EventResult::Consumed);
        assert_eq!(state.dropdown_scroll, WHEEL_ROWS.unsigned_abs());
        state.handle_event(&wheel(1.0));
        assert_eq!(state.dropdown_scroll, 0);
    }

    #[test]
    fn the_wheel_is_ignored_when_no_dropdown_is_open() {
        let mut state = SettingsState::new();
        let wheel = Event::Mouse(MouseEvent {
            x: 700.0,
            y: 300.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
        });
        assert_eq!(state.handle_event(&wheel), EventResult::Ignored);
        assert_eq!(state.dropdown_scroll, 0);
    }

    /// A click that lands on the "N more" line names no item, and must not be
    /// rounded onto the last one that is drawn.
    #[test]
    fn clicking_below_the_last_drawn_item_chooses_nothing() {
        let mut state = SettingsState::new();
        state.window_height = 200.0;
        state.resolution_index = 1;
        state.show_dropdown(DropdownId::Resolution);
        let layout = state.dropdown_layout().expect("a dropdown is open");
        assert!(layout.hidden() > 0, "the popup should be hiding items");
        let y = layout.row_top(layout.window.count) + 4.0;
        assert!(layout.item_at(layout.x + 20.0, y).is_none());
        click(&mut state, layout.x + 20.0, y);
        assert_eq!(state.resolution_index, 1);
    }

}

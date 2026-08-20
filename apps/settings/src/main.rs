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
use guitk::wheel;

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
    /// Index into [`ACCOUNT_PICTURES`] of the picture this account shows.
    ///
    /// An index rather than the icon itself, so the grid the user picks from
    /// and the avatar drawn next to the account's name cannot come to offer
    /// different sets of pictures.
    pub picture: usize,
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
    /// Warm at 0.0, cool at 1.0. Range stated by [`SliderId::range`].
    pub night_light_temperature: f32,
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
    /// Range stated by [`SliderId::range`], not repeated here.
    pub text_size_percent: u16,
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
    /// Slow at 0.0, fast at 1.0. Range stated by [`SliderId::range`].
    pub narrator_rate: f32,
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
    /// Fractions of an item left over from previous wheel events.
    ///
    /// [`dropdown_scroll`](Self::dropdown_scroll) counts whole items, so a
    /// trackpad's tenth-of-a-notch has nowhere to go the moment it is
    /// converted. Banking the remainder is what lets ten small pushes cross
    /// three items exactly as one detent does; the handler this replaced read
    /// only the *sign* of `dy`, so a twentieth of a notch and a hard flick of
    /// the wheel both moved the list by the same three items.
    dropdown_wheel: wheel::Accumulator,
    /// The slider the pointer is dragging, if a button is down on one.
    ///
    /// A slider is the only control here that is not finished by the press
    /// that starts it: the pointer keeps carrying the value until it is
    /// released. Holding the *name* rather than a rectangle means a drag
    /// survives the page redrawing underneath it — the track is re-derived
    /// from the page on every move, so the value follows the same track the
    /// user can see.
    dragging: Option<SliderId>,
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
        // A NaN compares false against everything, so it passes every bounds
        // test below by failing all of them, and `NaN as usize` is 0 rather
        // than a trap — between them that made a coordinate that is nowhere
        // select the popup's first item. Rejected up front, where it is one
        // condition rather than three double negatives.
        if !mx.is_finite() || !my.is_finite() {
            return None;
        }
        if mx < self.x || mx >= self.x + self.width {
            return None;
        }
        let top = self.row_top(0);
        if my < top {
            return None;
        }
        // Truncating rather than rounding: a click 35.9px below the first row's
        // top is on the first row, not adjacent to the second.
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
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

impl DropdownId {
    /// Every dropdown in the application.
    ///
    /// Exists so a test can walk the whole set and check each one is reachable.
    /// Three of these were drawn with nothing that could open them, and the
    /// only cheap way to keep an eleventh from joining them is to iterate the
    /// enum rather than trust that whoever adds it also wires it.
    pub const ALL: [Self; 10] = [
        Self::Resolution,
        Self::RefreshRate,
        Self::Scale,
        Self::OutputDevice,
        Self::InputDevice,
        Self::IpConfig,
        Self::DiagnosticLevel,
        Self::ColorFilter,
        Self::CursorSize,
        Self::NarratorVerbosity,
    ];
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
                    picture: 2,
                },
                UserAccount {
                    name: "Bob".into(),
                    email: "bob@example.com".into(),
                    account_type: AccountType::Standard,
                    login_count: 56,
                    last_login: "2026-05-16 18:20".into(),
                    is_current: false,
                    picture: 1,
                },
                UserAccount {
                    name: "Charlie".into(),
                    email: "charlie@example.com".into(),
                    account_type: AccountType::Child,
                    login_count: 23,
                    last_login: "2026-05-15 14:05".into(),
                    is_current: false,
                    picture: 5,
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
            dropdown_wheel: wheel::Accumulator::default(),
            dragging: None,
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

/// The track of a slider drawn at (`x`, `y`), as `(x, y)`.
///
/// The only place a slider's track is positioned. The bar that is painted, the
/// band the pointer grabs, and the origin a drag measures from all come from
/// here — a slider whose visible track and draggable range differ is the same
/// class of defect the page-sink split exists to prevent, and this is the same
/// remedy [`pill_rect`] applies to pills.
fn slider_track(x: f32, y: f32) -> (f32, f32) {
    (x, y + (ITEM_HEIGHT - SLIDER_HEIGHT) / 2.0)
}

/// The band in which a slider drawn at (`x`, `y`) can be grabbed, as
/// `(x, y, width, height)`.
///
/// Wider than the track by a handle's radius at each end, so the handle is
/// still grabbable when it sits at either extreme, and as tall as the whole
/// row, because a six-pixel-high drag target is not one a pointer can hit. It
/// deliberately stops short of the row's label: a press anywhere in the band
/// jumps the value to the pointer, and a press on the words "Text Size" should
/// not slam the text size to its minimum.
fn slider_band(x: f32, y: f32) -> (f32, f32, f32, f32) {
    (
        x - SLIDER_HANDLE_RADIUS,
        y,
        SLIDER_WIDTH + SLIDER_HANDLE_RADIUS * 2.0,
        ITEM_HEIGHT,
    )
}

/// Render a horizontal slider at the given position.
/// Returns nothing; slider_value should be 0.0..=1.0.
fn render_slider(tree: &mut RenderTree, x: f32, y: f32, value: f32) {
    let (x, track_y) = slider_track(x, y);

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
/// column the toggles and dropdowns on other rows use, which is why it is that
/// column's constant rather than a second copy of the number.
const PILL_ROW_X: f32 = CONTROL_COLUMN_DX;

/// The rectangle of pill `index` in a row drawn at (`x`, `y`), as
/// `(x, y, width, height)`. The only place a pill's position is computed —
/// both the drawing and the click band come from here.
fn pill_rect(index: usize, x: f32, y: f32) -> (f32, f32, f32, f32) {
    #[allow(clippy::cast_precision_loss)]
    let px = x + (index as f32) * PILL_PITCH;
    (px, y + PILL_INSET_Y, PILL_WIDTH, PILL_HEIGHT)
}

/// Draw a row of pills, the selected one filled with the accent color.
fn render_pill_row(tree: &mut RenderTree, x: f32, y: f32, items: &[(&str, bool)]) {
    for (idx, (label, active)) in items.iter().enumerate() {
        let (px, py, pw, ph) = pill_rect(idx, x, y);
        let (bg, fg) = if *active {
            (COL_ACCENT, COL_CRUST)
        } else {
            (COL_SURFACE1, COL_SUBTEXT0)
        };
        fill_rounded(tree, px, py, pw, ph, bg, 6.0);
        tree.text(px + 10.0, py + 7.0, label, fg, 12.0);
    }
}

/// `value` rounded to the nearest `u8`.
///
/// Callers pass a value already clamped to the slider's range, so the
/// saturation below is belt-and-braces; it is written out because a float that
/// escaped its range would otherwise wrap silently to the wrong end of the
/// scale, and a volume that reads 3% when the handle is at the far right is
/// worse than one that reads 100%.
fn round_u8(value: f32) -> u8 {
    // Float-to-integer `as` saturates at the bounds and maps NaN to 0 in Rust,
    // which is exactly the behaviour wanted for a pointer-derived value.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        value.round().clamp(0.0, f32::from(u8::MAX)) as u8
    }
}

/// `value` rounded to the nearest `u16`. See [`round_u8`].
fn round_u16(value: f32) -> u16 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        value.round().clamp(0.0, f32::from(u16::MAX)) as u16
    }
}

// --- Buttons and text fields ----------------------------------------------

/// Width of the button [`render_button`] draws for `label`. The click target
/// reads this rather than remeasuring, for the same reason the pills do.
fn button_width(label: &str) -> f32 {
    text::padded_width(label, 12.0, 13.0, FontWeightHint::Regular)
}

/// Draw a filled push button with its top-left corner at (`x`, `y`).
///
/// Only for a button that has somewhere to send a click; one that does not is
/// drawn by [`render_disabled_button`]. Which of the two runs is not a choice
/// any caller makes — see [`PageSink::button_at`].
fn render_button(tree: &mut RenderTree, x: f32, y: f32, label: &str, color: Color) {
    fill_rounded(tree, x, y, button_width(label), BUTTON_HEIGHT, color, 6.0);
    tree.text(x + 12.0, y + 8.0, label, COL_CRUST, 13.0);
}

/// Draw a push button that has nothing behind it: dimmed fill, muted label.
///
/// Takes no colour, deliberately. The colour a live button carries says what
/// *kind* of action it is — accent for the ordinary one, red for the
/// destructive one — and a button that cannot act has no kind. A greyed-out
/// "Remove Account" painted red would be an alarm about something that cannot
/// happen.
///
/// Same width and height as the live button, from the same [`button_width`],
/// so nothing on the page moves depending on whether a feature exists yet.
fn render_disabled_button(tree: &mut RenderTree, x: f32, y: f32, label: &str) {
    fill_rounded(
        tree,
        x,
        y,
        button_width(label),
        BUTTON_HEIGHT,
        COL_SURFACE0,
        6.0,
    );
    tree.text(x + 12.0, y + 8.0, label, COL_OVERLAY0, 13.0);
}

/// Draw a read-only text field showing `value`, inset within a row at `y`.
fn render_text_field(tree: &mut RenderTree, x: f32, y: f32, value: &str, width: f32) {
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

// --- Theme cards and colour swatches ---------------------------------------
//
// Grids rather than rows, so they get their geometry named here for the same
// reason the pills do: the card the user sees and the card the click resolves
// to have to be computed from one set of numbers.

/// Width of a theme-mode card on the Themes page.
const THEME_CARD_WIDTH: f32 = 140.0;
/// Height of a theme-mode card.
const THEME_CARD_HEIGHT: f32 = 100.0;
/// Gap between one theme-mode card's right edge and the next card's left.
const THEME_CARD_SPACING: f32 = 16.0;

/// Draw one theme-mode card with its top-left corner at (`x`, `y`).
fn render_theme_card(tree: &mut RenderTree, x: f32, y: f32, mode: ThemeMode, selected: bool) {
    let card_bg = if selected { COL_SURFACE1 } else { COL_SURFACE0 };
    fill_rounded(
        tree,
        x,
        y,
        THEME_CARD_WIDTH,
        THEME_CARD_HEIGHT,
        card_bg,
        8.0,
    );

    if selected {
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: THEME_CARD_WIDTH,
            height: THEME_CARD_HEIGHT,
            color: COL_ACCENT,
            line_width: 2.0,
            corner_radii: CornerRadii::all(8.0),
        });
    }

    // Theme preview (mini window mockup)
    let preview_x = x + 15.0;
    let preview_y = y + 12.0;
    let (win_bg, win_text) = match mode {
        ThemeMode::Light => (Color::from_hex(0xEFF1F5), Color::from_hex(0x4C4F69)),
        ThemeMode::Dark => (Color::from_hex(0x1E1E2E), Color::from_hex(0xCDD6F4)),
        ThemeMode::System => (Color::from_hex(0x313244), Color::from_hex(0xBAC2DE)),
    };
    fill_rounded(
        tree,
        preview_x,
        preview_y,
        THEME_CARD_WIDTH - 30.0,
        50.0,
        win_bg,
        4.0,
    );
    tree.text(preview_x + 8.0, preview_y + 18.0, "Aa", win_text, 16.0);

    let label_color = if selected { COL_ACCENT } else { COL_SUBTEXT0 };
    tree.text(
        x + THEME_CARD_WIDTH / 2.0 - 16.0,
        y + THEME_CARD_HEIGHT - 22.0,
        mode.label(),
        label_color,
        13.0,
    );
}

// --- Account pictures -------------------------------------------------------
//
// Another grid, named here for the same reason the theme cards are: the tile
// the user sees and the tile a click resolves to are computed from one set of
// numbers, so a tile cannot be drawn anywhere the click does not follow.

/// The pictures an account may be given, in the order the grid draws them.
///
/// [`UserAccount::picture`] indexes this array, so one list decides both what
/// the Login Options page offers and what the Accounts page draws.
const ACCOUNT_PICTURES: [&str; 6] = [
    "\u{1F464}",
    "\u{1F468}",
    "\u{1F469}",
    "\u{1F474}",
    "\u{1F475}",
    "\u{1F476}",
];

/// Width and height of one account-picture tile.
const PICTURE_TILE_SIZE: f32 = 48.0;
/// Gap between one tile's right edge and the next tile's left edge.
const PICTURE_TILE_SPACING: f32 = 12.0;

/// The icon for picture `index`, falling back to the first.
///
/// An out-of-range index is a bug rather than a state the UI offers, and the
/// fallback keeps it a *visible* bug: a blank square in the account list would
/// read as "no picture chosen" instead of as something wrong.
fn account_picture_icon(index: usize) -> &'static str {
    match ACCOUNT_PICTURES.get(index) {
        Some(icon) => icon,
        None => ACCOUNT_PICTURES.first().copied().unwrap_or("?"),
    }
}

/// How far right of the grid's left edge tile `index` is drawn.
fn picture_tile_dx(index: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let index = index as f32;
    index * (PICTURE_TILE_SIZE + PICTURE_TILE_SPACING)
}

/// Draw one account-picture tile with its top-left corner at (`x`, `y`).
fn render_account_picture(tree: &mut RenderTree, x: f32, y: f32, icon: &str, selected: bool) {
    let bg = if selected { COL_SURFACE1 } else { COL_SURFACE0 };
    fill_rounded(tree, x, y, PICTURE_TILE_SIZE, PICTURE_TILE_SIZE, bg, 8.0);
    if selected {
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: PICTURE_TILE_SIZE,
            height: PICTURE_TILE_SIZE,
            color: COL_ACCENT,
            line_width: 2.0,
            corner_radii: CornerRadii::all(8.0),
        });
    }
    tree.text(x + 12.0, y + 12.0, icon, COL_TEXT, 20.0);
}

// --- Pointer size buttons ---------------------------------------------------

/// How many pointer sizes the Interaction page offers.
const POINTER_SIZE_COUNT: usize = 5;
/// Width of one pointer-size button.
const POINTER_BUTTON_WIDTH: f32 = 32.0;
/// Height of one pointer-size button.
const POINTER_BUTTON_HEIGHT: f32 = 26.0;
/// Distance from one pointer-size button's left edge to the next.
const POINTER_BUTTON_PITCH: f32 = 40.0;
/// How far below the row's top edge the pointer-size buttons sit.
const POINTER_BUTTON_INSET_Y: f32 = 10.0;

/// The pointer size the button at `index` selects. Sizes run 1..=5, indices
/// 0..5, and this is the one place that off-by-one is written down.
fn pointer_size_of(index: usize) -> u8 {
    u8::try_from(index).unwrap_or(0).saturating_add(1)
}

/// Draw one pointer-size button, inset within a row whose top edge is `y`.
fn render_pointer_size_button(tree: &mut RenderTree, x: f32, y: f32, size: u8, active: bool) {
    let (bg, fg) = if active {
        (COL_ACCENT, COL_CRUST)
    } else {
        (COL_SURFACE1, COL_SUBTEXT0)
    };
    fill_rounded(
        tree,
        x,
        y + POINTER_BUTTON_INSET_Y,
        POINTER_BUTTON_WIDTH,
        POINTER_BUTTON_HEIGHT,
        bg,
        4.0,
    );
    let label = size.to_string();
    tree.text(x + 12.0, y + POINTER_BUTTON_INSET_Y + 6.0, &label, fg, 12.0);
}

/// Diameter of an accent-colour swatch.
const SWATCH_SIZE: f32 = 36.0;
/// Gap between adjacent swatches, both across and down.
const SWATCH_SPACING: f32 = 10.0;
/// Swatches per row of the accent-colour grid.
const SWATCH_COLS: usize = 6;

/// Where the swatch at `index` of the accent grid sits, relative to the grid's
/// top-left corner. The one place the grid's row-major order is written down.
fn swatch_offset(index: usize) -> (f32, f32) {
    #[allow(clippy::cast_precision_loss)]
    let col = (index % SWATCH_COLS) as f32;
    #[allow(clippy::cast_precision_loss)]
    let row = (index / SWATCH_COLS) as f32;
    (
        col * (SWATCH_SIZE + SWATCH_SPACING),
        row * (SWATCH_SIZE + SWATCH_SPACING),
    )
}

/// Draw one accent swatch, ringed when it is the chosen one.
fn render_swatch(tree: &mut RenderTree, x: f32, y: f32, color: Color, selected: bool) {
    fill_rounded(
        tree,
        x,
        y,
        SWATCH_SIZE,
        SWATCH_SIZE,
        color,
        SWATCH_SIZE / 2.0,
    );
    if selected {
        // The ring stands off the swatch on every side, so it is that much
        // wider and its corner radius that much larger — one number, not three.
        const RING_GAP: f32 = 3.0;
        let ring = SWATCH_SIZE + 2.0 * RING_GAP;
        tree.push(RenderCommand::StrokeRect {
            x: x - RING_GAP,
            y: y - RING_GAP,
            width: ring,
            height: ring,
            color: COL_TEXT,
            line_width: 2.0,
            corner_radii: CornerRadii::all(ring / 2.0),
        });
    }
}

// ============================================================================
// Page layout: one walk down the page, both drawn and hit-tested
// ============================================================================
//
// Every settings page used to be written twice. `render_*_page` walked a `y`
// cursor down the page emitting rows, and `handle_*_click` re-derived the same
// cursor from scratch — `base_y + 24.0 + 12.0 + 120.0 + SECTION_SPACING +
// 24.0 + 12.0` — to work out what a click had landed on. Two spellings of one
// layout are two layouts, and these had already drifted apart: the Sound page
// drew an Input Device dropdown that nothing could open, because the handler's
// arithmetic stopped three rows short of it. Three of the ten dropdowns were
// unreachable that way, and so was every per-app permission toggle outside the
// Location section, and the pointer-size buttons on the Interaction page.
//
// So the walk exists once. A page is described by one `build_*_page` method
// that calls into a `PageSink`, and there are two sinks. `DrawSink` paints
// each row as the cursor passes it; `HitSink` paints nothing and remembers
// which row's band contained the pointer. The row drawn at a given y *is* the
// row clicked there, because a single function put both of them there — the
// same collapse `pill_rect` above already applies within one row, and
// `dropdown_layout` applies to the popup.

/// Distance from the content column's left edge to the column the row controls
/// — toggles, dropdown buttons, sliders, pill rows — are drawn in.
const CONTROL_COLUMN_DX: f32 = 350.0;

/// How far a row's click band reaches left of the content column. Matches the
/// inset of the selection highlight the list rows paint, so the band and the
/// visible row start at the same place.
const ROW_HIT_INSET: f32 = 8.0;

/// How wide a row's click band is: enough to cover the label, the control in
/// the right column, and the value text beside it. A row holds exactly one
/// control, so the whole row is that control's target — a 44-pixel toggle is a
/// needle to thread with a mouse, and the label is what the user is aiming at
/// anyway.
const ROW_HIT_WIDTH: f32 = 620.0;

/// Vertical space a section header occupies: the title, its divider, and the
/// gap beneath. The same total [`render_section_header`] returns.
const SECTION_HEADER_HEIGHT: f32 = 36.0;

/// Height of a button drawn by [`render_button`].
const BUTTON_HEIGHT: f32 = 32.0;

/// How far below a row's top edge a button inside that row is drawn.
const BUTTON_ROW_INSET_Y: f32 = 6.0;

/// Which of a page's per-application permission lists a toggle belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionKind {
    Location,
    Camera,
    Microphone,
    Background,
}

/// A boolean setting, named so a click can find its field without the click
/// handler knowing where on the page the row was drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleId {
    NightLight,
    OutputMuted,
    SystemSounds,
    ProxyEnabled,
    AutoLogin,
    LocationEnabled,
    CameraEnabled,
    MicrophoneEnabled,
    /// The per-app switch at `index` of `kind`'s list.
    AppPermission(PermissionKind, usize),
    MonoAudio,
    VisualAlerts,
    NarratorEnabled,
    StickyKeys,
    FilterKeys,
    ToggleKeys,
    OnscreenKeyboard,
    MouseKeys,
    HighContrast,
    ReduceAnimations,
    ReduceTransparency,
    AutoUpdate,
}

/// A row of small selectable buttons — see [`render_pill_row`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PillId {
    Transparency,
    AnimationSpeed,
}

/// A one-of-many choice laid out as something other than a pill row: a grid of
/// cards or swatches, a list of rows, a strip of numbered buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectId {
    ThemeMode,
    AccentColor,
    Adapter,
    Account,
    PointerSize,
    AccountPicture,
}

/// A continuously-valued setting the pointer can drag along a track.
///
/// Named rather than addressed by coordinates for the same reason
/// [`ToggleId`] is: the mapping between a control and the field behind it is
/// written once, in [`SliderId::range`] and
/// [`SettingsState::set_slider_fraction`], instead of being re-derived
/// wherever the slider happens to be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SliderId {
    NightLightTemperature,
    OutputVolume,
    InputVolume,
    /// The volume of `app_volumes[index]`.
    AppVolume(usize),
    NarratorRate,
    TextSize,
    DeferFeatureDays,
    DeferQualityDays,
}

impl SliderId {
    /// Every slider whose identity does not depend on a list index.
    ///
    /// The per-application volumes are left out because how many of them there
    /// are is state, not a constant; a test that wants those enumerates
    /// `app_volumes` instead. Exists so a test can walk the rest and check each
    /// one is draggable, the way [`DropdownId::ALL`] does for dropdowns.
    #[cfg(test)]
    const FIXED: [Self; 7] = [
        Self::NightLightTemperature,
        Self::OutputVolume,
        Self::InputVolume,
        Self::NarratorRate,
        Self::TextSize,
        Self::DeferFeatureDays,
        Self::DeferQualityDays,
    ];

    /// The lowest and highest value this slider can hold, in the units the
    /// state stores it in — percent for the volumes, days for the deferrals,
    /// a bare 0–1 fraction for the two that have no natural unit.
    ///
    /// Written here once so that the position the handle is drawn at and the
    /// value a drag produces are exact inverses; `test_every_slider_round_trips`
    /// holds them to it.
    fn range(self) -> (f32, f32) {
        match self {
            Self::NightLightTemperature | Self::NarratorRate => (0.0, 1.0),
            Self::OutputVolume | Self::InputVolume | Self::AppVolume(_) => (0.0, 100.0),
            Self::TextSize => (50.0, 250.0),
            Self::DeferFeatureDays => (0.0, 365.0),
            Self::DeferQualityDays => (0.0, 30.0),
        }
    }

    /// The text printed beside the track, given the slider's value in stored
    /// units — or `None` for the two whose ends are labelled ("Warm"/"Cool",
    /// "Slow"/"Fast") because the number itself would mean nothing.
    ///
    /// Derived from the same value the handle is drawn at, so the figure beside
    /// a slider cannot disagree with where its handle is.
    fn readout(self, value: f32) -> Option<String> {
        let whole = round_u16(value);
        match self {
            Self::NightLightTemperature | Self::NarratorRate => None,
            Self::OutputVolume | Self::InputVolume | Self::AppVolume(_) | Self::TextSize => {
                Some(format!("{whole}%"))
            }
            Self::DeferFeatureDays | Self::DeferQualityDays => Some(format!("{whole} days")),
        }
    }
}

/// A thing whose drawn position some other part of the window needs to know:
/// a dropdown's popup must open under its own button, and a slider drag must
/// measure from its own track.
///
/// Both answers come from walking the page rather than from a table of
/// coordinates kept beside it, which is what stops a popup opening away from
/// its button or a drag measuring from a track that is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnchorId {
    /// The top-left of a dropdown's closed button.
    Dropdown(DropdownId),
    /// The left end of a slider's track.
    Slider(SliderId),
}

/// A push button that does something when pressed.
///
/// Only buttons with an effect are listed. The rest of the page's buttons —
/// Change Password, Add/Remove Account, Clear Activity History, Go Back, Fresh
/// Start, Manage Family Settings — have no state behind them yet, so they
/// register no click target rather than swallowing a click and doing nothing.
/// See known-issues.md `C-SETTINGS-BUTTONS-WITH-NOTHING-BEHIND-THEM`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonId {
    CheckForUpdates,
}

/// What a click on a page landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowHit {
    Dropdown(DropdownId),
    Toggle(ToggleId),
    Pill(PillId, usize),
    Select(SelectId, usize),
    Press(ButtonId),
    /// The grab band of a slider. Which value the press means is not recorded
    /// here — it depends on where along the track the pointer landed, and the
    /// track's origin is asked of the page via [`AnchorId::Slider`] rather
    /// than carried alongside the name.
    Slider(SliderId),
}

/// The one description of a settings page, interpreted either as drawing or as
/// hit-testing.
///
/// Implementors provide the four primitives — where the cursor is, how to move
/// it, how to draw at it, and how to register a click target. Everything a
/// page actually says (`section`, `toggle_row`, `dropdown_row`, …) is a
/// provided method built from those, so a row's geometry is written down once
/// for both sinks and cannot drift between them.
trait PageSink {
    /// Left edge of the content column.
    fn x(&self) -> f32;

    /// Top edge of the row about to be emitted.
    fn y(&self) -> f32;

    /// Move the cursor down by `dy`.
    fn advance(&mut self, dy: f32);

    /// Draw at the cursor. `f` receives the render tree and the cursor's
    /// `(x, y)`; [`HitSink`] discards it unevaluated.
    fn draw(&mut self, f: impl FnOnce(&mut RenderTree, f32, f32));

    /// Register `what` as clickable over the given rectangle, in absolute
    /// window coordinates. [`DrawSink`] discards it.
    fn hit_rect(&mut self, x: f32, y: f32, w: f32, h: f32, what: RowHit);

    // ---- provided: the vocabulary a page is written in ----

    /// The column every row's control sits in.
    fn control_x(&self) -> f32 {
        self.x() + CONTROL_COLUMN_DX
    }

    /// A section title with its divider rule.
    fn section(&mut self, title: &str) {
        self.draw(|tree, x, y| {
            render_section_header(tree, x, y, title);
        });
        self.advance(SECTION_HEADER_HEIGHT);
    }

    /// The blank space between one section and the next.
    fn gap(&mut self) {
        self.advance(SECTION_SPACING);
    }

    /// A line of explanatory prose beneath a section header.
    fn note(&mut self, text: &str, height: f32) {
        self.draw(|tree, x, y| tree.text(x, y + 4.0, text, COL_SUBTEXT0, 13.0));
        self.advance(height);
    }

    /// A labelled row: text on the left, `control` drawn at the control
    /// column, and — when `what` is `Some` — the whole row as its click band.
    fn row(
        &mut self,
        label: &str,
        what: Option<RowHit>,
        height: f32,
        control: impl FnOnce(&mut RenderTree, f32, f32),
    ) {
        let control_x = self.control_x();
        if let Some(what) = what {
            let (x, y) = (self.x(), self.y());
            self.hit_rect(x - ROW_HIT_INSET, y, ROW_HIT_WIDTH, height, what);
        }
        self.draw(|tree, x, y| {
            render_setting_row(tree, x, y, label, 0.0);
            control(tree, control_x, y);
        });
        self.advance(height);
    }

    /// Note where a named control was drawn, so something outside the page can
    /// be positioned against it. Only [`AnchorSink`] records this; the default
    /// ignores it, which is why it costs a draw or a hit-test nothing.
    fn anchor(&mut self, _id: AnchorId, _x: f32, _y: f32) {}

    /// A row whose control is a closed dropdown button.
    fn dropdown_row(&mut self, label: &str, id: DropdownId, value: &str) {
        let (cx, y) = (self.control_x(), self.y());
        self.anchor(AnchorId::Dropdown(id), cx, y);
        self.row(
            label,
            Some(RowHit::Dropdown(id)),
            ITEM_HEIGHT,
            |tree, cx, y| {
                render_dropdown_button(tree, cx, y, value, DROPDOWN_WIDTH);
            },
        );
    }

    /// A row whose control is an on/off switch.
    fn toggle_row(&mut self, label: &str, id: ToggleId, on: bool) {
        self.row(
            label,
            Some(RowHit::Toggle(id)),
            ITEM_HEIGHT,
            |tree, cx, y| {
                render_toggle(tree, cx, y + 12.0, on);
            },
        );
    }

    /// An indented per-application switch, as used by the permission lists.
    /// Tighter than a full row, and its own label rather than a setting row's.
    fn app_toggle_row(&mut self, label: &str, id: ToggleId, on: bool) {
        let height = ITEM_HEIGHT - 8.0;
        let control_x = self.control_x();
        let (x, y) = (self.x(), self.y());
        self.hit_rect(x, y, ROW_HIT_WIDTH, height, RowHit::Toggle(id));
        self.draw(|tree, x, y| {
            tree.text(x + 16.0, y + 14.0, label, COL_SUBTEXT1, 13.0);
            render_toggle(tree, control_x, y + 12.0, on);
        });
        self.advance(height);
    }

    /// A row whose control is a draggable slider, plus whatever else the page
    /// wants drawn beside it. `extra` is given the render tree and the control
    /// column's `(x, y)` — the same origin the track is drawn from.
    ///
    /// `value` is the handle's position along the track, 0.0–1.0. Pages do not
    /// call this directly; they go through [`SettingsState::slider`], which
    /// takes the position from [`SettingsState::slider_fraction`] so that what
    /// is drawn and what a drag produces are one mapping read in each
    /// direction.
    fn slider_row(
        &mut self,
        label: &str,
        id: SliderId,
        value: f32,
        extra: impl FnOnce(&mut RenderTree, f32, f32),
    ) {
        let (track_x, track_y) = slider_track(self.control_x(), self.y());
        self.anchor(AnchorId::Slider(id), track_x, track_y);
        let (bx, by, bw, bh) = slider_band(track_x, self.y());
        self.hit_rect(bx, by, bw, bh, RowHit::Slider(id));
        self.row(label, None, ITEM_HEIGHT, move |tree, cx, y| {
            render_slider(tree, cx, y, value);
            extra(tree, cx, y);
        });
    }

    /// A row that only reports a value; nothing to click.
    fn value_row(&mut self, label: &str, value: &str, color: Color) {
        self.row(label, None, ITEM_HEIGHT, |tree, cx, y| {
            tree.text(cx, y + 14.0, value, color, 13.0);
        });
    }

    /// A row whose control is a read-only text field.
    fn field_row(&mut self, label: &str, value: &str, width: f32) {
        self.row(label, None, ITEM_HEIGHT, |tree, cx, y| {
            render_text_field(tree, cx, y, value, width);
        });
    }

    /// A row whose control is a strip of pills, one of them selected.
    fn pill_row(&mut self, label: &str, id: PillId, items: &[(&str, bool)]) {
        let pill_x = self.x() + PILL_ROW_X;
        let y = self.y();
        for idx in 0..items.len() {
            let (px, py, pw, ph) = pill_rect(idx, pill_x, y);
            self.hit_rect(px, py, pw, ph, RowHit::Pill(id, idx));
        }
        self.row(label, None, ITEM_HEIGHT, |tree, _cx, y| {
            render_pill_row(tree, pill_x, y, items);
        });
    }

    /// A button offset from the cursor by (`dx`, `dy`). Buttons sit inside
    /// blocks of bespoke content, so this does not move the cursor — the
    /// caller advances past the whole block.
    ///
    /// **A button looks live exactly when it is live.** `what` is the one
    /// value that decides both: it registers the click band, and its
    /// `Some`-ness picks which of [`render_button`] / [`render_disabled_button`]
    /// paints it. There is deliberately no way to ask for the live colour
    /// while passing `None`, because that combination is the bug this is
    /// fixing — seven buttons that promised an action the app cannot perform.
    ///
    /// A `None` button still registers no band, which stays correct: a band
    /// that swallowed the click would take the "nothing happened here"
    /// feedback away and block anything drawn beneath it. Dimming is what
    /// tells the user *why* nothing happened.
    fn button_at(&mut self, dx: f32, dy: f32, label: &str, color: Color, what: Option<RowHit>) {
        match what {
            Some(what) => {
                let (x, y) = (self.x(), self.y());
                self.hit_rect(x + dx, y + dy, button_width(label), BUTTON_HEIGHT, what);
                self.draw(|tree, x, y| render_button(tree, x + dx, y + dy, label, color));
            }
            None => self.draw(|tree, x, y| render_disabled_button(tree, x + dx, y + dy, label)),
        }
    }

    /// A labelled row whose control is a push button.
    ///
    /// Routes through [`Self::button_at`] rather than painting the button in a
    /// [`Self::row`] closure, so a button inside a row is subject to the same
    /// one-value rule as a free-standing one. The row itself takes no click
    /// band — the button is the target, not the whole row, because the rest of
    /// the row is a label and pressing a label should do nothing.
    fn button_row(&mut self, label: &str, button: &str, color: Color, what: Option<RowHit>) {
        self.draw(|tree, x, y| {
            render_setting_row(tree, x, y, label, 0.0);
        });
        self.button_at(CONTROL_COLUMN_DX, BUTTON_ROW_INSET_Y, button, color, what);
        self.advance(ITEM_HEIGHT);
    }

    /// One row of a selectable list: a click anywhere in it selects `index`,
    /// and `draw` paints the row's contents at the cursor.
    fn list_row(
        &mut self,
        id: SelectId,
        index: usize,
        height: f32,
        pitch: f32,
        draw: impl FnOnce(&mut RenderTree, f32, f32),
    ) {
        let (x, y) = (self.x(), self.y());
        self.hit_rect(
            x - ROW_HIT_INSET,
            y,
            ROW_HIT_WIDTH,
            height,
            RowHit::Select(id, index),
        );
        self.draw(draw);
        self.advance(pitch);
    }
}

/// The sink that paints the page.
struct DrawSink<'a> {
    tree: &'a mut RenderTree,
    x: f32,
    y: f32,
}

impl PageSink for DrawSink<'_> {
    fn x(&self) -> f32 {
        self.x
    }
    fn y(&self) -> f32 {
        self.y
    }
    fn advance(&mut self, dy: f32) {
        self.y += dy;
    }
    fn draw(&mut self, f: impl FnOnce(&mut RenderTree, f32, f32)) {
        f(self.tree, self.x, self.y);
    }
    fn hit_rect(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _what: RowHit) {}
}

/// The sink that answers "what is under the pointer?".
///
/// It walks the identical sequence of calls `DrawSink` does and keeps the
/// first target whose rectangle contains the point. First rather than last
/// because the old hand-written handlers returned on their first match, and
/// because a page's bands are disjoint by construction anyway — if two ever
/// overlap, the earlier one is the one drawn underneath the pointer first.
struct HitSink {
    mx: f32,
    my: f32,
    x: f32,
    y: f32,
    hit: Option<RowHit>,
}

impl PageSink for HitSink {
    fn x(&self) -> f32 {
        self.x
    }
    fn y(&self) -> f32 {
        self.y
    }
    fn advance(&mut self, dy: f32) {
        self.y += dy;
    }
    fn draw(&mut self, _f: impl FnOnce(&mut RenderTree, f32, f32)) {}
    fn hit_rect(&mut self, x: f32, y: f32, w: f32, h: f32, what: RowHit) {
        if self.hit.is_none() && self.mx >= x && self.mx < x + w && self.my >= y && self.my < y + h
        {
            self.hit = Some(what);
        }
    }
}

/// The sink that answers "where was this control drawn?".
///
/// The popup used to carry its own table of anchor coordinates — a third copy
/// of each page's arithmetic, and the one that decided *where the list appears*
/// rather than merely where a click lands. Walking the page for the answer
/// means a popup cannot open somewhere other than under its own button, and a
/// slider drag cannot measure from a track other than the one on screen.
struct AnchorSink {
    want: AnchorId,
    x: f32,
    y: f32,
    found: Option<(f32, f32)>,
}

impl PageSink for AnchorSink {
    fn x(&self) -> f32 {
        self.x
    }
    fn y(&self) -> f32 {
        self.y
    }
    fn advance(&mut self, dy: f32) {
        self.y += dy;
    }
    fn draw(&mut self, _f: impl FnOnce(&mut RenderTree, f32, f32)) {}
    fn hit_rect(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _what: RowHit) {}
    fn anchor(&mut self, id: AnchorId, x: f32, y: f32) {
        if self.found.is_none() && id == self.want {
            self.found = Some((x, y));
        }
    }
}

/// The indented run of per-app switches beneath a permission's master toggle.
///
/// One function for all four lists, taking the list it is describing, so a
/// click on a Camera row cannot resolve against the Location list's indices —
/// which is what a per-list copy of this loop invites.
fn build_permission_list<S: PageSink>(s: &mut S, kind: PermissionKind, apps: &[AppPermission]) {
    for (idx, app) in apps.iter().enumerate() {
        s.app_toggle_row(
            &app.app_name,
            ToggleId::AppPermission(kind, idx),
            app.allowed,
        );
    }
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

    // ---------------------------------------------------------------- sidebar
    //
    // The category list's top edge used to be written out longhand in four
    // places -- the renderer, the click handler, the hover handler and
    // `test_sidebar_click` -- as `HEADER_HEIGHT + SEARCH_BAR_HEIGHT + 16.0`.
    // Four copies of one number is four chances to change three of them, and
    // the test was the worst copy of the lot: it recomputed the constant
    // rather than reading the rectangle the renderer emitted, and probed the
    // *centre* of a row, so it would have gone on passing while the painted
    // list and the clickable list drifted apart.
    //
    // These four functions are the single spelling. The renderer draws from
    // `category_row_top`, both hit tests answer from `category_at`, and
    // `category_at` is `category_row_top` inverted -- so the rows drawn are
    // exactly the rows clickable, down to the gap between them.

    /// Y of the top of the search box, which sits directly under the title.
    const fn search_top() -> f32 {
        HEADER_HEIGHT
    }

    /// Y of the top of the first category row.
    const fn category_list_top() -> f32 {
        HEADER_HEIGHT + SEARCH_BAR_HEIGHT + 16.0
    }

    /// Y of the top of the `idx`-th category row's slot.
    ///
    /// The slot is [`CATEGORY_ITEM_HEIGHT`] tall; the *painted* row is
    /// [`Self::CATEGORY_ROW_PAINTED_HEIGHT`] tall, the difference being the gap
    /// the renderer leaves between one highlight and the next.
    #[allow(clippy::cast_precision_loss)]
    fn category_row_top(idx: usize) -> f32 {
        Self::category_list_top() + (idx as f32) * CATEGORY_ITEM_HEIGHT
    }

    /// Height of the highlight actually drawn for a category row.
    const CATEGORY_ROW_PAINTED_HEIGHT: f32 = CATEGORY_ITEM_HEIGHT - 4.0;

    /// Y just past the last category row -- the bottom of the whole list.
    fn category_list_bottom() -> f32 {
        Self::category_row_top(SettingsCategory::ALL.len().saturating_sub(1))
            + Self::CATEGORY_ROW_PAINTED_HEIGHT
    }

    /// The category under `(mx, my)`, as an index into
    /// [`SettingsCategory::ALL`].
    ///
    /// `None` for a point outside the sidebar, above the list, past its last
    /// row, or in one of the four-pixel gaps between rows. The gap is not an
    /// oversight: the renderer paints nothing there, and a hit test that
    /// answers for a pixel the renderer left blank is how a hover highlight
    /// ends up sitting a few pixels above the pointer that summoned it.
    fn category_at(mx: f32, my: f32) -> Option<usize> {
        if !mx.is_finite() || mx < 0.0 || mx >= SIDEBAR_WIDTH {
            return None;
        }
        let from_top = my - Self::category_list_top();
        if !from_top.is_finite() || from_top < 0.0 || my >= Self::category_list_bottom() {
            return None;
        }
        // Truncating rather than rounding: a point 43.9px below the first
        // row's top is in the first row's slot, not adjacent to the second.
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let idx = (from_top / CATEGORY_ITEM_HEIGHT) as usize;
        // `category_list_bottom` has already ruled this out. It stays because
        // the cast above saturates rather than wrapping, and an index into
        // `ALL` is not a thing to leave to a float's rounding.
        if idx >= SettingsCategory::ALL.len() {
            return None;
        }
        if my >= Self::category_row_top(idx) + Self::CATEGORY_ROW_PAINTED_HEIGHT {
            return None;
        }
        Some(idx)
    }

    /// Render the left sidebar with search bar and category list.
    fn render_sidebar(&self, tree: &mut RenderTree) {
        // Sidebar background
        tree.fill_rect(0.0, 0.0, SIDEBAR_WIDTH, self.window_height, COL_CRUST);

        // App title
        text_bold(tree, 20.0, 18.0, "Settings", COL_TEXT, 20.0);

        // Search bar
        let search_y = Self::search_top();
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
        for (idx, category) in SettingsCategory::ALL.iter().enumerate() {
            let item_y = Self::category_row_top(idx);
            let is_selected = *category == self.current_category;
            let is_hovered = self.sidebar_hovered == Some(idx);

            // Background highlight
            if is_selected {
                fill_rounded(
                    tree,
                    8.0,
                    item_y,
                    SIDEBAR_WIDTH - 16.0,
                    Self::CATEGORY_ROW_PAINTED_HEIGHT,
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
                    Self::CATEGORY_ROW_PAINTED_HEIGHT,
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

    /// Describe the current page to `sink`, top to bottom.
    ///
    /// The single walk. [`Self::render_current_page`] runs it through a
    /// [`DrawSink`] to paint the page and [`Self::row_at`] runs it through a
    /// [`HitSink`] to find what a click hit, so the two can never disagree
    /// about where a row is.
    fn build_page<S: PageSink>(&self, sink: &mut S) {
        match self.current_page {
            SettingsPage::Display => self.build_display_page(sink),
            SettingsPage::Sound => self.build_sound_page(sink),
            SettingsPage::Themes => self.build_themes_page(sink),
            SettingsPage::Colors => self.build_colors_page(sink),
            SettingsPage::NetworkStatus => self.build_network_page(sink),
            SettingsPage::Proxy => self.build_proxy_page(sink),
            SettingsPage::UserAccounts | SettingsPage::LoginOptions => {
                self.build_accounts_page(sink);
            }
            SettingsPage::Permissions | SettingsPage::Capabilities => {
                self.build_privacy_page(sink);
            }
            SettingsPage::Visual | SettingsPage::Audio | SettingsPage::Interaction => {
                self.build_accessibility_page(sink);
            }
            SettingsPage::SystemUpdates | SettingsPage::Recovery | SettingsPage::Snapshots => {
                self.build_update_page(sink);
            }
            _ => self.build_placeholder_page(sink),
        }
    }

    /// Left edge of the content column, in window coordinates.
    fn content_x() -> f32 {
        SIDEBAR_WIDTH + CONTENT_PADDING
    }

    /// Top edge of the page content, in window coordinates.
    fn content_top() -> f32 {
        HEADER_HEIGHT + 8.0
    }

    /// Paint the current page.
    fn render_current_page(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut sink = DrawSink {
            tree,
            x,
            y: start_y,
        };
        self.build_page(&mut sink);
    }

    /// Emit a slider row for `id`.
    ///
    /// A page names the slider and nothing else. Its handle position and the
    /// figure beside it both come from the state, through one mapping, so a
    /// page cannot draw a handle somewhere other than where a drag would put
    /// it — which is what the call sites used to invite by spelling out
    /// `(f32::from(self.text_size_percent) - 50.0) / 200.0` for themselves.
    /// Nothing is drawn if the slider has no value, which happens only for a
    /// per-application volume whose row is gone.
    fn slider<S: PageSink>(&self, s: &mut S, label: &str, id: SliderId) {
        self.slider_with(s, label, id, |_, _, _| {});
    }

    /// [`slider`](Self::slider) with something drawn after the readout, for the
    /// per-application rows that also mark themselves muted.
    fn slider_with<S: PageSink>(
        &self,
        s: &mut S,
        label: &str,
        id: SliderId,
        extra: impl FnOnce(&mut RenderTree, f32, f32),
    ) {
        let (Some(raw), Some(value)) = (self.slider_raw(id), self.slider_fraction(id)) else {
            return;
        };
        let readout = id.readout(raw);
        s.slider_row(label, id, value, move |tree, cx, y| {
            if let Some(readout) = &readout {
                tree.text(
                    cx + SLIDER_WIDTH + 12.0,
                    y + 14.0,
                    readout,
                    COL_SUBTEXT0,
                    12.0,
                );
            }
            extra(tree, cx, y);
        });
    }

    /// What the page control at (`mx`, `my`) is, if the point is on one.
    fn row_at(&self, mx: f32, my: f32) -> Option<RowHit> {
        let mut sink = HitSink {
            mx,
            my,
            x: Self::content_x(),
            y: Self::content_top(),
            hit: None,
        };
        self.build_page(&mut sink);
        sink.hit
    }

    // --- Display page ---

    fn build_display_page<S: PageSink>(&self, s: &mut S) {
        s.section("Monitor Arrangement");
        let monitors = self.monitor_count;
        s.draw(move |tree, x, y| Self::render_monitor_preview(tree, x, y, monitors));
        s.advance(120.0);
        s.gap();

        s.section("Resolution & Scaling");
        let res_label = RESOLUTIONS
            .get(self.resolution_index)
            .map_or_else(|| "Unknown".to_string(), |r| r.label());
        s.dropdown_row("Resolution", DropdownId::Resolution, &res_label);

        let rate = REFRESH_RATES
            .get(self.refresh_rate_index)
            .copied()
            .unwrap_or(60);
        let rate_label = format!("{rate} Hz");
        s.dropdown_row("Refresh Rate", DropdownId::RefreshRate, &rate_label);

        s.dropdown_row("Display Scaling", DropdownId::Scale, self.scale.label());
        s.gap();

        s.section("Night Light");
        s.toggle_row(
            "Night Light",
            ToggleId::NightLight,
            self.night_light_enabled,
        );

        if self.night_light_enabled {
            self.slider(s, "Color Temperature", SliderId::NightLightTemperature);
            // Range labels, sitting on the row boundary beneath the slider.
            s.draw(|tree, x, y| {
                let cx = x + CONTROL_COLUMN_DX;
                tree.text(cx, y, "Warm", COL_PEACH, 11.0);
                tree.text(cx + SLIDER_WIDTH - 30.0, y, "Cool", COL_ACCENT, 11.0);
            });
        }
    }

    /// Render a simplified monitor arrangement preview.
    fn render_monitor_preview(tree: &mut RenderTree, x: f32, y: f32, monitor_count: u8) {
        let preview_bg_w = 500.0;
        let preview_bg_h = 110.0;
        fill_rounded(tree, x, y, preview_bg_w, preview_bg_h, COL_SURFACE0, 8.0);

        let monitor_w = 100.0;
        let monitor_h = 70.0;
        let spacing = 20.0;
        #[allow(clippy::cast_precision_loss)]
        let count = monitor_count as f32;
        let total_w = count * monitor_w + (count - 1.0) * spacing;
        let start_x = x + (preview_bg_w - total_w) / 2.0;
        let start_y = y + (preview_bg_h - monitor_h) / 2.0;

        for i in 0..monitor_count {
            #[allow(clippy::cast_precision_loss)]
            let mx = start_x + f32::from(i) * (monitor_w + spacing);
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
            // Monitor number. Displays are numbered from one; `saturating_add`
            // rather than `+` because `monitor_count` is a `u8` the caller sets.
            let num_label = i.saturating_add(1).to_string();
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

    fn build_sound_page<S: PageSink>(&self, s: &mut S) {
        s.section("Output");
        let output_name = self
            .output_devices
            .get(self.output_device_index)
            .map_or("None", |d| d.name.as_str());
        s.dropdown_row("Output Device", DropdownId::OutputDevice, output_name);

        self.slider(s, "Volume", SliderId::OutputVolume);
        s.toggle_row("Mute", ToggleId::OutputMuted, self.output_muted);
        s.gap();

        s.section("Input");
        let input_name = self
            .input_devices
            .get(self.input_device_index)
            .map_or("None", |d| d.name.as_str());
        s.dropdown_row("Input Device", DropdownId::InputDevice, input_name);

        self.slider(s, "Input Volume", SliderId::InputVolume);
        s.gap();

        s.section("System Sounds");
        s.toggle_row(
            "Enable System Sounds",
            ToggleId::SystemSounds,
            self.system_sounds_enabled,
        );
        s.gap();

        s.section("Per-Application Volume");
        for (index, app_vol) in self.app_volumes.iter().enumerate() {
            let muted = app_vol.muted;
            self.slider_with(
                s,
                &app_vol.app_name,
                SliderId::AppVolume(index),
                move |tree, cx, y| {
                    if muted {
                        tree.text(cx + SLIDER_WIDTH + 50.0, y + 14.0, "(muted)", COL_RED, 11.0);
                    }
                },
            );
        }
    }

    // --- Themes page ---

    fn build_themes_page<S: PageSink>(&self, s: &mut S) {
        s.section("Theme Mode");

        let selected = self.appearance.settings.theme_mode;
        for (idx, mode) in ThemeMode::ALL.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let dx = (idx as f32) * (THEME_CARD_WIDTH + THEME_CARD_SPACING);
            let (x, y) = (s.x(), s.y());
            s.hit_rect(
                x + dx,
                y,
                THEME_CARD_WIDTH,
                THEME_CARD_HEIGHT,
                RowHit::Select(SelectId::ThemeMode, idx),
            );
            let mode = *mode;
            s.draw(move |tree, x, y| {
                render_theme_card(tree, x + dx, y, mode, mode == selected);
            });
        }
        s.advance(THEME_CARD_HEIGHT);
        s.gap();

        // Transparency. A row of levels rather than the on/off switch this
        // page used to show: the setting has four values, and a switch that
        // meant "Off or whatever it was" would forget a user's choice of
        // Full every time they turned it off and on again.
        s.section("Effects");
        let levels: Vec<(&str, bool)> = TransparencyLevel::ALL
            .iter()
            .map(|l| (l.label(), *l == self.appearance.settings.transparency))
            .collect();
        s.pill_row("Transparency", PillId::Transparency, &levels);

        let speeds: Vec<(&str, bool)> = AnimationSpeed::ALL
            .iter()
            .map(|sp| (sp.label(), *sp == self.appearance.settings.animation_speed))
            .collect();
        s.pill_row("Animation Speed", PillId::AnimationSpeed, &speeds);
    }

    // --- Colors page (accent color picker) ---

    fn build_colors_page<S: PageSink>(&self, s: &mut S) {
        s.section("Accent Color");
        s.draw(|tree, x, y| {
            tree.text(
                x,
                y,
                "Choose an accent color for buttons, links, and highlights:",
                COL_SUBTEXT0,
                13.0,
            );
        });
        s.advance(28.0);

        let presets = AccentColor::presets();
        let light = self.appearance.settings.theme_mode.is_light();
        let chosen = self.appearance.settings.accent_color;
        for (idx, accent) in presets.iter().enumerate() {
            let (dx, dy) = swatch_offset(idx);
            let (x, y) = (s.x(), s.y());
            s.hit_rect(
                x + dx,
                y + dy,
                SWATCH_SIZE,
                SWATCH_SIZE,
                RowHit::Select(SelectId::AccentColor, idx),
            );
            // The swatch shows what this accent will actually look like in
            // the mode the user is in — the light palette is a darkened set,
            // so showing the dark one would promise a colour they won't get.
            let color = if light {
                accent.color_light()
            } else {
                accent.color()
            };
            let selected = *accent == chosen;
            s.draw(move |tree, x, y| {
                render_swatch(tree, x + dx, y + dy, color, selected);
            });
        }

        #[allow(clippy::cast_precision_loss)]
        let grid_rows = presets.len().div_ceil(SWATCH_COLS) as f32;
        s.advance(grid_rows * (SWATCH_SIZE + SWATCH_SPACING));
        s.gap();

        s.section("Preview");
        let preview_color = self.appearance.settings.effective_accent();
        s.draw(move |tree, x, y| {
            fill_rounded(tree, x, y, 120.0, 36.0, preview_color, 6.0);
            tree.text(x + 20.0, y + 10.0, "Sample Button", COL_CRUST, 13.0);
            tree.text(x + 150.0, y + 10.0, "Sample link text", preview_color, 13.0);
        });
        s.advance(50.0);
        s.draw(move |tree, x, y| {
            fill_rounded(tree, x, y, 300.0, 8.0, COL_SURFACE1, 4.0);
            fill_rounded(tree, x, y, 200.0, 8.0, preview_color, 4.0);
        });
    }

    // --- Network status page ---

    fn build_network_page<S: PageSink>(&self, s: &mut S) {
        s.section("Network Adapters");
        let control_x = s.control_x();
        for (idx, adapter) in self.adapters.iter().enumerate() {
            let selected = idx == self.selected_adapter;
            s.list_row(
                SelectId::Adapter,
                idx,
                ITEM_HEIGHT,
                ITEM_HEIGHT + 4.0,
                move |tree, x, y| {
                    let row_bg = if selected {
                        COL_SURFACE0
                    } else {
                        Color::TRANSPARENT
                    };
                    fill_rounded(tree, x - 8.0, y, 600.0, ITEM_HEIGHT, row_bg, 6.0);

                    let status_color = if adapter.connected {
                        COL_GREEN
                    } else {
                        COL_OVERLAY0
                    };
                    fill_rounded(tree, x, y + 18.0, 10.0, 10.0, status_color, 5.0);

                    tree.text(x + 20.0, y + 8.0, &adapter.name, COL_TEXT, 14.0);
                    tree.text(
                        x + 20.0,
                        y + 26.0,
                        adapter.adapter_type.label(),
                        COL_SUBTEXT0,
                        11.0,
                    );

                    let status_text = if adapter.connected {
                        &adapter.ip_address
                    } else {
                        "Disconnected"
                    };
                    tree.text(control_x, y + 14.0, status_text, COL_SUBTEXT0, 13.0);
                },
            );
        }
        s.gap();

        s.section("IP Configuration");
        s.dropdown_row("Mode", DropdownId::IpConfig, self.ip_config_mode.label());

        if self.ip_config_mode == IpConfigMode::Static {
            s.field_row("IP Address", &self.static_ip, 180.0);
            s.field_row("Gateway", &self.static_gateway, 180.0);
        }
        s.gap();

        s.section("DNS Servers");
        s.field_row("Primary DNS", &self.dns_primary, 180.0);
        s.field_row("Secondary DNS", &self.dns_secondary, 180.0);
    }

    // --- Proxy page ---

    fn build_proxy_page<S: PageSink>(&self, s: &mut S) {
        s.section("Proxy Configuration");
        s.toggle_row(
            "Use Proxy Server",
            ToggleId::ProxyEnabled,
            self.proxy_enabled,
        );

        if self.proxy_enabled {
            s.field_row("Proxy Address", &self.proxy_address, 220.0);
            s.field_row("Port", &self.proxy_port, 80.0);
        }
    }

    // --- Accounts page ---

    fn build_accounts_page<S: PageSink>(&self, s: &mut S) {
        if self.current_page == SettingsPage::LoginOptions {
            self.build_login_options_page(s);
            return;
        }

        // User account list (default UserAccounts page)
        s.section("User Accounts");

        let control_x = s.control_x();
        for (idx, account) in self.user_accounts.iter().enumerate() {
            let selected = idx == self.selected_account;
            let avatar = account_picture_icon(account.picture);
            s.list_row(SelectId::Account, idx, 60.0, 64.0, move |tree, x, y| {
                let row_bg = if selected {
                    COL_SURFACE0
                } else {
                    Color::TRANSPARENT
                };
                fill_rounded(tree, x - 8.0, y, 620.0, 60.0, row_bg, 8.0);

                // Avatar placeholder
                let avatar_size = 40.0;
                fill_rounded(
                    tree,
                    x + 4.0,
                    y + 10.0,
                    avatar_size,
                    avatar_size,
                    COL_SURFACE2,
                    avatar_size / 2.0,
                );
                tree.text(x + 16.0, y + 20.0, avatar, COL_TEXT, 16.0);

                text_bold(tree, x + 56.0, y + 12.0, &account.name, COL_TEXT, 14.0);
                tree.text(x + 56.0, y + 32.0, &account.email, COL_SUBTEXT0, 12.0);

                let badge_color = account.account_type.color();
                fill_rounded(tree, control_x, y + 18.0, 90.0, 22.0, badge_color, 4.0);
                tree.text(
                    control_x + 8.0,
                    y + 22.0,
                    account.account_type.label(),
                    COL_CRUST,
                    11.0,
                );

                if account.is_current {
                    tree.text(control_x + 100.0, y + 22.0, "(You)", COL_ACCENT, 11.0);
                }
            });
        }
        s.gap();

        s.button_at(0.0, 0.0, "+ Add Account", COL_ACCENT, None);
        s.button_at(140.0, 0.0, "- Remove Account", COL_RED, None);
        s.advance(44.0);
        s.gap();

        // Current user details
        if let Some(account) = self.user_accounts.get(self.selected_account) {
            s.section("Account Details");
            s.value_row("Name", &account.name, COL_TEXT);
            s.value_row("Email", &account.email, COL_TEXT);
            s.value_row(
                "Account Type",
                account.account_type.label(),
                account.account_type.color(),
            );
            s.value_row("Login Count", &account.login_count.to_string(), COL_TEXT);
            s.value_row("Last Login", &account.last_login, COL_TEXT);

            // Family safety for child accounts
            if account.account_type == AccountType::Child {
                s.gap();
                s.section("Family Safety");
                s.note("Screen time limits and content filters are active", 24.0);
                s.button_at(0.0, 0.0, "Manage Family Settings", COL_PEACH, None);
            }
        }
    }

    fn build_login_options_page<S: PageSink>(&self, s: &mut S) {
        s.section("Login Options");
        s.toggle_row(
            "Auto-login on startup",
            ToggleId::AutoLogin,
            self.auto_login_enabled,
        );

        s.button_row("Password", "Change Password", COL_ACCENT, None);
        s.gap();

        s.section("Account Picture");
        s.note("Choose a picture for your account:", 28.0);

        // One tile at a time, so each gets its own click band at exactly the
        // offset it is drawn at. Drawing all six in a single closure -- which
        // is how this started -- leaves the sink no place to hang six separate
        // bands, which is why the grid was inert.
        let chosen = self.current_account_picture();
        for (idx, icon) in ACCOUNT_PICTURES.iter().copied().enumerate() {
            let dx = picture_tile_dx(idx);
            let (x, y) = (s.x(), s.y());
            s.hit_rect(
                x + dx,
                y,
                PICTURE_TILE_SIZE,
                PICTURE_TILE_SIZE,
                RowHit::Select(SelectId::AccountPicture, idx),
            );
            let selected = chosen == Some(idx);
            s.draw(move |tree, x, y| {
                render_account_picture(tree, x + dx, y, icon, selected);
            });
        }
        s.advance(PICTURE_TILE_SIZE);
    }

    // --- Privacy page ---

    /// The Capabilities sub-page: a read-only summary of which apps hold
    /// which permissions. Nothing on it is clickable, so it is pure drawing.
    fn render_capabilities_summary(&self, tree: &mut RenderTree, x: f32, start_y: f32) {
        let mut y = start_y;
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
    }

    // --- Privacy page ---

    fn build_privacy_page<S: PageSink>(&self, s: &mut S) {
        if self.current_page == SettingsPage::Capabilities {
            s.draw(|tree, x, y| self.render_capabilities_summary(tree, x, y));
            return;
        }

        // Location access (default Permissions page)
        s.section("Location");
        s.toggle_row(
            "Allow apps to access location",
            ToggleId::LocationEnabled,
            self.location_enabled,
        );
        if self.location_enabled {
            build_permission_list(s, PermissionKind::Location, &self.location_apps);
        }
        s.gap();

        s.section("Camera");
        s.toggle_row(
            "Allow apps to access camera",
            ToggleId::CameraEnabled,
            self.camera_enabled,
        );
        if self.camera_enabled {
            build_permission_list(s, PermissionKind::Camera, &self.camera_apps);
        }
        s.gap();

        s.section("Microphone");
        s.toggle_row(
            "Allow apps to access microphone",
            ToggleId::MicrophoneEnabled,
            self.microphone_enabled,
        );
        if self.microphone_enabled {
            build_permission_list(s, PermissionKind::Microphone, &self.microphone_apps);
        }
        s.gap();

        s.section("Background Apps");
        s.note("Choose which apps can run in the background:", 28.0);
        build_permission_list(s, PermissionKind::Background, &self.background_apps);
        s.gap();

        s.section("Diagnostics & Data");
        s.dropdown_row(
            "Diagnostic data collection",
            DropdownId::DiagnosticLevel,
            self.diagnostic_level.label(),
        );
        s.gap();

        s.section("Activity History");
        s.note("Clear your activity history stored on this device.", 28.0);
        s.button_at(0.0, 0.0, "Clear Activity History", COL_RED, None);
    }

    // --- Accessibility page ---

    fn build_accessibility_page<S: PageSink>(&self, s: &mut S) {
        match self.current_page {
            SettingsPage::Audio => self.build_audio_accessibility_page(s),
            SettingsPage::Interaction => self.build_interaction_accessibility_page(s),
            _ => self.build_visual_accessibility_page(s),
        }
    }

    fn build_audio_accessibility_page<S: PageSink>(&self, s: &mut S) {
        s.section("Audio Accessibility");
        s.toggle_row("Mono audio", ToggleId::MonoAudio, self.mono_audio);
        s.toggle_row(
            "Visual alerts for sounds",
            ToggleId::VisualAlerts,
            self.visual_alerts,
        );
        s.gap();

        s.section("Narrator");
        s.toggle_row(
            "Enable Narrator",
            ToggleId::NarratorEnabled,
            self.narrator_enabled,
        );

        if self.narrator_enabled {
            self.slider(s, "Voice Rate", SliderId::NarratorRate);
            s.draw(|tree, x, y| {
                let cx = x + CONTROL_COLUMN_DX;
                tree.text(cx, y - 12.0, "Slow", COL_SUBTEXT0, 11.0);
                tree.text(
                    cx + SLIDER_WIDTH - 24.0,
                    y - 12.0,
                    "Fast",
                    COL_SUBTEXT0,
                    11.0,
                );
            });
            s.advance(16.0);

            s.dropdown_row(
                "Verbosity",
                DropdownId::NarratorVerbosity,
                self.narrator_verbosity.label(),
            );
        }
    }

    fn build_interaction_accessibility_page<S: PageSink>(&self, s: &mut S) {
        s.section("Keyboard");

        for (label, id, on, hint) in [
            (
                "Sticky Keys",
                ToggleId::StickyKeys,
                self.sticky_keys,
                "Press modifier keys one at a time",
            ),
            (
                "Filter Keys",
                ToggleId::FilterKeys,
                self.filter_keys,
                "Ignore brief or repeated keystrokes",
            ),
            (
                "Toggle Keys",
                ToggleId::ToggleKeys,
                self.toggle_keys,
                "Play a sound when pressing Caps/Num/Scroll Lock",
            ),
        ] {
            s.toggle_row(label, id, on);
            s.draw(move |tree, x, y| tree.text(x + 16.0, y - 4.0, hint, COL_SUBTEXT0, 11.0));
            s.advance(12.0);
        }

        s.toggle_row(
            "On-Screen Keyboard",
            ToggleId::OnscreenKeyboard,
            self.onscreen_keyboard,
        );
        s.gap();

        s.section("Mouse & Pointer");

        // Pointer size as a strip of numbered buttons, one per size.
        let chosen = self.pointer_size;
        for idx in 0..POINTER_SIZE_COUNT {
            #[allow(clippy::cast_precision_loss)]
            let dx = CONTROL_COLUMN_DX + (idx as f32) * POINTER_BUTTON_PITCH;
            let (x, y) = (s.x(), s.y());
            s.hit_rect(
                x + dx,
                y + POINTER_BUTTON_INSET_Y,
                POINTER_BUTTON_WIDTH,
                POINTER_BUTTON_HEIGHT,
                RowHit::Select(SelectId::PointerSize, idx),
            );
            let size = pointer_size_of(idx);
            s.draw(move |tree, x, y| {
                render_pointer_size_button(tree, x + dx, y, size, size == chosen);
            });
        }
        s.row("Pointer Size", None, ITEM_HEIGHT, |_tree, _cx, _y| {});

        s.toggle_row(
            "Mouse Keys (numpad controls pointer)",
            ToggleId::MouseKeys,
            self.mouse_keys,
        );
    }

    fn build_visual_accessibility_page<S: PageSink>(&self, s: &mut S) {
        s.section("Display");

        self.slider(s, "Text Size", SliderId::TextSize);
        // Range labels, on the boundary between this row and the next.
        s.draw(|tree, x, y| {
            let cx = x + CONTROL_COLUMN_DX;
            tree.text(cx, y - 8.0, "50%", COL_SUBTEXT0, 11.0);
            tree.text(
                cx + SLIDER_WIDTH - 28.0,
                y - 8.0,
                "250%",
                COL_SUBTEXT0,
                11.0,
            );
        });
        s.advance(8.0);

        s.toggle_row("High Contrast", ToggleId::HighContrast, self.high_contrast);
        s.dropdown_row(
            "Cursor Size",
            DropdownId::CursorSize,
            self.cursor_size.label(),
        );
        s.toggle_row(
            "Reduce Animations",
            ToggleId::ReduceAnimations,
            self.reduce_animations,
        );
        s.gap();

        s.section("Color & Transparency");
        s.dropdown_row(
            "Color Filters",
            DropdownId::ColorFilter,
            self.color_filter.label(),
        );
        s.toggle_row(
            "Reduce Transparency",
            ToggleId::ReduceTransparency,
            self.reduce_transparency,
        );

        if self.color_filter != ColorFilter::None {
            s.advance(8.0);
            let label = self.color_filter.label();
            s.draw(move |tree, x, y| {
                fill_rounded(tree, x, y, 300.0, 40.0, COL_SURFACE0, 6.0);
                tree.text(
                    x + 12.0,
                    y + 12.0,
                    "Color filter active: ",
                    COL_SUBTEXT0,
                    12.0,
                );
                tree.text(x + 150.0, y + 12.0, label, COL_ACCENT, 12.0);
            });
            s.advance(40.0);
        }
    }

    fn build_update_page<S: PageSink>(&self, s: &mut S) {
        match self.current_page {
            SettingsPage::Recovery => {
                self.build_recovery_page(s);
                return;
            }
            SettingsPage::Snapshots => {
                Self::build_snapshots_page(s);
                return;
            }
            _ => {} // SystemUpdates (default)
        }

        s.section("System Information");
        let version = self.os_version.clone();
        s.draw(move |tree, x, y| {
            fill_rounded(tree, x, y, 580.0, 60.0, COL_SURFACE0, 8.0);
            text_bold(tree, x + 16.0, y + 12.0, "Slate OS", COL_TEXT, 16.0);
            tree.text(x + 16.0, y + 36.0, &version, COL_SUBTEXT0, 13.0);
        });
        s.advance(72.0);

        // The button's label changes while a check is running, and the label is
        // what `button_width` measures — so the click band follows the wider
        // "Check for Updates" down to the narrower "Checking...", rather than
        // staying at whichever width happened to be hard-coded.
        let checking = self.checking_for_updates;
        let btn_label = if checking {
            "Checking..."
        } else {
            "Check for Updates"
        };
        s.button_at(
            0.0,
            0.0,
            btn_label,
            COL_ACCENT,
            Some(RowHit::Press(ButtonId::CheckForUpdates)),
        );
        if !checking {
            s.draw(|tree, x, y| {
                tree.text(
                    x + 160.0,
                    y + 10.0,
                    "Your device is up to date",
                    COL_GREEN,
                    13.0,
                );
            });
        }
        s.advance(44.0);
        s.gap();

        s.section("Update Preferences");
        s.toggle_row(
            "Automatic updates",
            ToggleId::AutoUpdate,
            self.auto_update_enabled,
        );
        let hours_label = format!(
            "{:02}:00 - {:02}:00",
            self.active_hours_start, self.active_hours_end
        );
        s.value_row("Active hours (no restart)", &hours_label, COL_TEXT);
        s.gap();

        s.section("Advanced");
        self.slider(
            s,
            "Defer feature updates (days)",
            SliderId::DeferFeatureDays,
        );
        self.slider(
            s,
            "Defer quality updates (days)",
            SliderId::DeferQualityDays,
        );
        s.gap();

        s.section("Update History");
        for entry in &self.update_history {
            let (kb, desc, date) = (
                entry.kb_number.clone(),
                entry.description.clone(),
                entry.date.clone(),
            );
            let (status_color, status_label) = (entry.status.color(), entry.status.label());
            s.draw(move |tree, x, y| {
                fill_rounded(tree, x, y, 580.0, 44.0, COL_SURFACE0, 6.0);
                tree.text(x + 12.0, y + 8.0, &kb, COL_TEXT, 13.0);
                tree.text(x + 120.0, y + 8.0, &desc, COL_SUBTEXT0, 12.0);
                tree.text(x + 12.0, y + 26.0, &date, COL_OVERLAY0, 11.0);
                fill_rounded(tree, x + 490.0, y + 12.0, 72.0, 20.0, status_color, 4.0);
                tree.text(x + 500.0, y + 15.0, status_label, COL_CRUST, 11.0);
            });
            s.advance(52.0);
        }
    }

    /// The Recovery sub-page: two cards, each with a button that has no state
    /// behind it yet and so registers no click target.
    fn build_recovery_page<S: PageSink>(&self, s: &mut S) {
        s.section("Recovery Options");
        s.note("If your PC isn't working well, recovering may help.", 32.0);

        s.draw(|tree, x, y| {
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
        });
        s.button_at(440.0, 28.0, "Go Back", COL_PEACH, None);
        s.advance(96.0);

        s.draw(|tree, x, y| {
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
        });
        s.button_at(440.0, 28.0, "Reset", COL_RED, None);
        s.advance(96.0);
    }

    /// The Snapshots sub-page: package generations available for rollback.
    /// Read-only until the package manager exposes a rollback call.
    fn build_snapshots_page<S: PageSink>(s: &mut S) {
        s.section("System Snapshots");
        s.note("Package generation snapshots for safe rollback:", 32.0);

        let snapshots = [
            ("Gen 42", "2026-05-17 09:00", "Current"),
            ("Gen 41", "2026-05-15 14:30", "After KB5032100"),
            ("Gen 40", "2026-05-10 11:00", "After KB5031980"),
            ("Gen 39", "2026-05-01 08:45", "After compositor update"),
        ];

        for (name, date, desc) in snapshots {
            let control_x = s.control_x();
            let is_current = desc == "Current";
            s.draw(move |tree, x, y| {
                let bg = if is_current {
                    COL_SURFACE1
                } else {
                    COL_SURFACE0
                };
                fill_rounded(tree, x, y, 580.0, 48.0, bg, 6.0);
                text_bold(tree, x + 12.0, y + 8.0, name, COL_TEXT, 13.0);
                tree.text(x + 12.0, y + 28.0, desc, COL_SUBTEXT0, 11.0);
                tree.text(control_x, y + 16.0, date, COL_SUBTEXT0, 12.0);
                if is_current {
                    tree.text(x + 520.0, y + 16.0, "\u{2713}", COL_GREEN, 16.0);
                }
            });
            s.advance(56.0);
        }
    }

    // --- Placeholder for unimplemented pages ---

    fn build_placeholder_page<S: PageSink>(&self, s: &mut S) {
        let page_name = self.current_page.label();
        s.draw(move |tree, x, y| {
            text_bold(tree, x, y + 20.0, page_name, COL_TEXT, 22.0);
            tree.text(
                x,
                y + 56.0,
                "This page is under construction.",
                COL_SUBTEXT0,
                14.0,
            );

            // Visual placeholder: a card with icon
            let card_y = y + 100.0;
            fill_rounded(tree, x, card_y, 400.0, 150.0, COL_SURFACE0, 12.0);
            tree.text(x + 170.0, card_y + 50.0, "\u{1F6A7}", COL_PEACH, 36.0);
            tree.text(
                x + 120.0,
                card_y + 110.0,
                "Coming soon...",
                COL_SUBTEXT0,
                14.0,
            );
        });
        s.advance(250.0);
    }

    // --- Dropdown overlay rendering ---

    /// Where `id` was drawn on the current page, or `None` if the page does
    /// not show that control.
    fn anchor_at(&self, id: AnchorId) -> Option<(f32, f32)> {
        let mut sink = AnchorSink {
            want: id,
            x: Self::content_x(),
            y: Self::content_top(),
            found: None,
        };
        self.build_page(&mut sink);
        sink.found
    }

    /// Where the open dropdown's popup goes, or `None` if none is open.
    ///
    /// The single source of the popup's geometry. Both the renderer and the
    /// click handler ask this, so a click can never land on a different item
    /// from the one drawn under the pointer — the failure that made these
    /// dropdowns unusable was precisely that only the renderer knew where
    /// anything was.
    ///
    /// The anchor comes from walking the page rather than from a table of
    /// per-dropdown coordinates, so the popup opens under the button the user
    /// pressed even after the rows above it change height.
    #[must_use]
    pub fn dropdown_layout(&self) -> Option<DropdownLayout> {
        let dropdown_id = self.open_dropdown?;
        // No anchor means the page moved out from under an open dropdown. Draw
        // nothing rather than guess a position; the next click closes it.
        let (dropdown_x, dropdown_y) = self.anchor_at(AnchorId::Dropdown(dropdown_id))?;

        let (items, selected) = match dropdown_id {
            DropdownId::Resolution => {
                let items: Vec<String> = RESOLUTIONS.iter().map(|r| r.label()).collect();
                (items, self.resolution_index)
            }
            DropdownId::RefreshRate => {
                let items: Vec<String> =
                    REFRESH_RATES.iter().map(|r| format!("{} Hz", r)).collect();
                (items, self.refresh_rate_index)
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
                )
            }
            DropdownId::OutputDevice => {
                let items: Vec<String> =
                    self.output_devices.iter().map(|d| d.name.clone()).collect();
                (items, self.output_device_index)
            }
            DropdownId::InputDevice => {
                let items: Vec<String> =
                    self.input_devices.iter().map(|d| d.name.clone()).collect();
                (items, self.input_device_index)
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
                (items, sel)
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
                (items, sel)
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
                (items, sel)
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
                (items, sel)
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
                (items, sel)
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
                self.step_category(-1);
                EventResult::Consumed
            }
            Key::Down => {
                self.step_category(1);
                EventResult::Consumed
            }
            Key::Tab => {
                // Cycle through pages within category
                let pages = self.current_category.pages();
                let current_idx = pages
                    .iter()
                    .position(|p| *p == self.current_page)
                    .unwrap_or(0);
                // Wrap by index rather than by `%` on a possibly-empty slice:
                // every category has pages today, and an empty one would make
                // the remainder a division by zero rather than a no-op.
                let next = current_idx.saturating_add(1);
                if let Some(page) = pages.get(next).or_else(|| pages.first()) {
                    self.current_page = *page;
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Move the sidebar selection by `delta` categories, stopping at the ends.
    ///
    /// Stopping rather than wrapping: the sidebar is a visible list, and an Up
    /// at the top that jumps to the bottom moves the highlight further than the
    /// eye follows.
    fn step_category(&mut self, delta: isize) {
        let current = SettingsCategory::ALL
            .iter()
            .position(|c| *c == self.current_category)
            .unwrap_or(0);
        let Some(next) = current.checked_add_signed(delta) else {
            return;
        };
        if let Some(&new_cat) = SettingsCategory::ALL.get(next) {
            self.current_category = new_cat;
            self.current_page = new_cat.default_page();
        }
    }

    fn handle_mouse(&mut self, evt: &MouseEvent) -> EventResult {
        match &evt.kind {
            MouseEventKind::Press(MouseButton::Left) => self.handle_click(evt.x, evt.y),
            MouseEventKind::Move => self.handle_hover(evt.x, evt.y),
            // A drag ends wherever the button comes up, on the control or not.
            // Releasing outside is the ordinary way to finish a slider gesture,
            // so this must not be conditional on the pointer still being over
            // the track.
            MouseEventKind::Release(MouseButton::Left) => {
                if self.dragging.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // Only the sign of `dy` is used, not its magnitude:
            // `dy` is in notches; `wheel::rows` turns it into whole items and
            // banks the fraction. What was here read only the *sign* of `dy`,
            // so a trackpad's twitch moved three items just as a hard flick of
            // the wheel did, and a three-notch spin also moved three.
            MouseEventKind::Scroll { dy, .. } if self.open_dropdown.is_some() => {
                let rows = self.dropdown_wheel.rows(*dy);
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
            if let Some(idx) = Self::category_at(mx, my) {
                if let Some(&new_cat) = SettingsCategory::ALL.get(idx) {
                    self.current_category = new_cat;
                    self.current_page = new_cat.default_page();
                    return EventResult::Consumed;
                }
            }

            // Search bar click
            let search_y = Self::search_top();
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

        // Content area clicks — ask the page itself what is under the pointer,
        // rather than a per-page handler that re-derives the row positions.
        if let Some(hit) = self.row_at(mx, my) {
            self.apply_row_hit(hit, mx);
        }

        EventResult::Consumed
    }

    /// Apply the effect of clicking `hit` at horizontal position `mx`.
    ///
    /// Nothing here knows where anything was drawn — that is the whole point of
    /// the split. [`row_at`](Self::row_at) turns a point into a named control,
    /// and this turns a named control into a state change. `mx` is the one
    /// exception, and only the sliders read it: their value *is* a position
    /// along a track, so unlike every other control here the name alone does
    /// not say what the press meant. Even they do not do their own arithmetic —
    /// the track is asked of the page, in [`drag_slider_to`](Self::drag_slider_to).
    fn apply_row_hit(&mut self, hit: RowHit, mx: f32) {
        match hit {
            RowHit::Dropdown(id) => self.show_dropdown(id),
            RowHit::Slider(id) => {
                // A press both jumps the handle to the pointer and takes hold
                // of it: the same gesture that sets a value coarsely is the one
                // that then adjusts it, without a separate grab on the handle.
                self.dragging = Some(id);
                self.drag_slider_to(id, mx);
            }
            RowHit::Toggle(id) => {
                if let Some(flag) = self.toggle_mut(id) {
                    *flag = !*flag;
                }
            }
            RowHit::Pill(PillId::Transparency, idx) => {
                if let Some(level) = TransparencyLevel::ALL.get(idx) {
                    self.appearance.settings.transparency = *level;
                }
            }
            RowHit::Pill(PillId::AnimationSpeed, idx) => {
                if let Some(speed) = AnimationSpeed::ALL.get(idx) {
                    self.appearance.settings.animation_speed = *speed;
                }
            }
            RowHit::Select(SelectId::ThemeMode, idx) => {
                if let Some(mode) = ThemeMode::ALL.get(idx) {
                    self.appearance.settings.theme_mode = *mode;
                }
            }
            RowHit::Select(SelectId::AccentColor, idx) => {
                if let Some(accent) = AccentColor::presets().get(idx) {
                    self.appearance.settings.accent_color = *accent;
                }
            }
            RowHit::Select(SelectId::Adapter, idx) => {
                if idx < self.adapters.len() {
                    self.selected_adapter = idx;
                }
            }
            RowHit::Select(SelectId::Account, idx) => {
                if idx < self.user_accounts.len() {
                    self.selected_account = idx;
                }
            }
            RowHit::Select(SelectId::PointerSize, idx) => {
                self.pointer_size = pointer_size_of(idx);
            }
            RowHit::Select(SelectId::AccountPicture, idx) => {
                self.set_current_account_picture(idx);
            }
            RowHit::Press(ButtonId::CheckForUpdates) => {
                self.checking_for_updates = !self.checking_for_updates;
            }
        }
    }

    /// The picture the signed-in account is showing, or `None` if no account
    /// is marked as the signed-in one.
    ///
    /// `None` is drawn as "no tile is ringed" rather than as tile 0, so a
    /// machine with nobody signed in does not claim a choice was made.
    fn current_account_picture(&self) -> Option<usize> {
        self.user_accounts
            .iter()
            .find(|account| account.is_current)
            .map(|account| account.picture)
    }

    /// Give the signed-in account picture `index`.
    ///
    /// Writes to the account marked `is_current`, not to
    /// `user_accounts[selected_account]`: the Login Options page offers a
    /// picture for *your* account, so which row the Accounts page happens to
    /// have highlighted must not decide whose picture changes.
    ///
    /// An index past the end of [`ACCOUNT_PICTURES`] is refused rather than
    /// stored, so the field can never name a picture that does not exist.
    fn set_current_account_picture(&mut self, index: usize) {
        if index >= ACCOUNT_PICTURES.len() {
            return;
        }
        if let Some(account) = self
            .user_accounts
            .iter_mut()
            .find(|account| account.is_current)
        {
            account.picture = index;
        }
    }

    /// Move `id`'s handle to the pointer's horizontal position `mx`.
    ///
    /// The track's left edge is asked of the page rather than recomputed, so a
    /// drag measures from the very bar the user can see. If the page no longer
    /// shows this slider — the pointer left, the page changed under a held
    /// button — the drag simply does nothing rather than writing a value
    /// derived from a track that is not on screen.
    fn drag_slider_to(&mut self, id: SliderId, mx: f32) {
        let Some((track_x, _)) = self.anchor_at(AnchorId::Slider(id)) else {
            return;
        };
        self.set_slider_fraction(id, (mx - track_x) / SLIDER_WIDTH);
    }

    /// The current value of `id`, in the units the state stores it in.
    ///
    /// `None` when the slider is a per-application volume whose index no longer
    /// exists; the list is editable in principle and a stale index must not
    /// panic.
    fn slider_raw(&self, id: SliderId) -> Option<f32> {
        Some(match id {
            SliderId::NightLightTemperature => self.night_light_temperature,
            SliderId::NarratorRate => self.narrator_rate,
            SliderId::OutputVolume => f32::from(self.output_volume),
            SliderId::InputVolume => f32::from(self.input_volume),
            SliderId::AppVolume(index) => f32::from(self.app_volumes.get(index)?.volume),
            SliderId::TextSize => f32::from(self.text_size_percent),
            SliderId::DeferFeatureDays => f32::from(self.defer_feature_days),
            SliderId::DeferQualityDays => f32::from(self.defer_quality_days),
        })
    }

    /// How far along its track `id`'s handle sits, 0.0–1.0.
    ///
    /// The only way a page turns a stored setting into a handle position; the
    /// pages used to spell out `(f32::from(self.text_size_percent) - 50.0) /
    /// 200.0` at the call site, which is a second copy of a range that
    /// [`SliderId::range`] already states.
    fn slider_fraction(&self, id: SliderId) -> Option<f32> {
        let (lo, hi) = id.range();
        Some(((self.slider_raw(id)? - lo) / (hi - lo)).clamp(0.0, 1.0))
    }

    /// Set `id` from a position along its track, clamped to 0.0–1.0.
    ///
    /// The exact inverse of [`slider_fraction`](Self::slider_fraction), up to
    /// the rounding the integer-valued settings impose.
    fn set_slider_fraction(&mut self, id: SliderId, fraction: f32) {
        // A position that is not a number names no point on the track. Leave
        // the setting alone rather than store a NaN, which would then poison
        // the handle's drawn position and never wash out.
        if fraction.is_nan() {
            return;
        }
        let (lo, hi) = id.range();
        let value = (hi - lo).mul_add(fraction.clamp(0.0, 1.0), lo);
        match id {
            SliderId::NightLightTemperature => self.night_light_temperature = value,
            SliderId::NarratorRate => self.narrator_rate = value,
            SliderId::OutputVolume => self.output_volume = round_u8(value),
            SliderId::InputVolume => self.input_volume = round_u8(value),
            SliderId::AppVolume(index) => {
                if let Some(app) = self.app_volumes.get_mut(index) {
                    app.volume = round_u8(value);
                }
            }
            SliderId::TextSize => self.text_size_percent = round_u16(value),
            SliderId::DeferFeatureDays => self.defer_feature_days = round_u16(value),
            SliderId::DeferQualityDays => self.defer_quality_days = round_u16(value),
        }
    }

    /// The field behind a named boolean setting.
    ///
    /// One match for every switch on every page, so a new toggle is wired by
    /// adding one arm here rather than by inventing a new run of coordinates.
    /// `None` when a per-app index no longer exists — the lists are editable in
    /// principle, and a stale index must not panic.
    fn toggle_mut(&mut self, id: ToggleId) -> Option<&mut bool> {
        Some(match id {
            ToggleId::NightLight => &mut self.night_light_enabled,
            ToggleId::OutputMuted => &mut self.output_muted,
            ToggleId::SystemSounds => &mut self.system_sounds_enabled,
            ToggleId::ProxyEnabled => &mut self.proxy_enabled,
            ToggleId::AutoLogin => &mut self.auto_login_enabled,
            ToggleId::LocationEnabled => &mut self.location_enabled,
            ToggleId::CameraEnabled => &mut self.camera_enabled,
            ToggleId::MicrophoneEnabled => &mut self.microphone_enabled,
            ToggleId::AppPermission(kind, index) => {
                let list = match kind {
                    PermissionKind::Location => &mut self.location_apps,
                    PermissionKind::Camera => &mut self.camera_apps,
                    PermissionKind::Microphone => &mut self.microphone_apps,
                    PermissionKind::Background => &mut self.background_apps,
                };
                &mut list.get_mut(index)?.allowed
            }
            ToggleId::MonoAudio => &mut self.mono_audio,
            ToggleId::VisualAlerts => &mut self.visual_alerts,
            ToggleId::NarratorEnabled => &mut self.narrator_enabled,
            ToggleId::StickyKeys => &mut self.sticky_keys,
            ToggleId::FilterKeys => &mut self.filter_keys,
            ToggleId::ToggleKeys => &mut self.toggle_keys,
            ToggleId::OnscreenKeyboard => &mut self.onscreen_keyboard,
            ToggleId::MouseKeys => &mut self.mouse_keys,
            ToggleId::HighContrast => &mut self.high_contrast,
            ToggleId::ReduceAnimations => &mut self.reduce_animations,
            ToggleId::ReduceTransparency => &mut self.reduce_transparency,
            ToggleId::AutoUpdate => &mut self.auto_update_enabled,
        })
    }

    fn handle_hover(&mut self, mx: f32, my: f32) -> EventResult {
        // A held slider follows the pointer anywhere, including off the track
        // and out over the sidebar. Dropping the value the moment the pointer
        // strays above or below a six-pixel bar would make the control
        // unusable; every other toolkit lets the grab outlive the hover, and
        // the gesture ends on release rather than on leaving.
        if let Some(id) = self.dragging {
            self.drag_slider_to(id, mx);
            return EventResult::Consumed;
        }

        // Sidebar hover
        if mx < SIDEBAR_WIDTH {
            self.sidebar_hovered = Self::category_at(mx, my);
            return EventResult::Consumed;
        }

        self.sidebar_hovered = None;
        EventResult::Ignored
    }

    /// Opens `id`'s dropdown, scrolled so the current choice is on screen.
    ///
    /// The reveal matters for the long lists: opening the resolution dropdown
    /// on a short window and finding no ticked item anywhere in it is not a
    /// list of choices, it is a list of choices with the answer torn off.
    pub fn show_dropdown(&mut self, id: DropdownId) {
        self.open_dropdown = Some(id);
        self.dropdown_scroll = 0;
        // The offset is being set from scratch, so the fraction that was
        // pushing the last dropdown's offset must not survive into this one.
        self.dropdown_wheel.reset();
        // Ask for the layout now that the id is set: how far to scroll depends
        // on how many items fit, which depends on which dropdown this is.
        if let Some(layout) = self.dropdown_layout() {
            let capacity = layout.window.count;
            if capacity > 0 && layout.selected >= capacity {
                self.dropdown_scroll = layout.selected.saturating_add(1).saturating_sub(capacity);
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
// Panicking on bad data is what a test is *for*: an `unwrap` that fires names
// the broken invariant far better than a silent `if let` that skips the
// assertion would.
//
// `float_cmp` is here for a sharper reason. Several of these tests compare a
// coordinate the renderer emitted against the helper that produced it, and
// exact equality *is* the assertion -- an epsilon would let the renderer and
// the hit test drift by half a pixel with the suite still green, which is the
// exact failure the tests exist to catch.
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]
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

    // ---- Sidebar category list ----
    //
    // These read the rectangles the renderer emitted and probe *those*. The
    // test they replace recomputed `HEADER_HEIGHT + SEARCH_BAR_HEIGHT + 16.0`
    // itself and clicked the middle of a row, which is precisely the shape of
    // test that cannot see a renderer and a hit test drifting apart: both
    // sides of the comparison move together, and the middle of a row is the
    // last place an off-by-a-few-pixels error shows up.

    /// Every category highlight the renderer actually painted, as
    /// `(top, height)`, in the order drawn.
    ///
    /// The signature is the highlight's left edge and width, which nothing
    /// else in the sidebar shares -- the search box is inset differently and
    /// the content area starts at `SIDEBAR_WIDTH`.
    fn category_highlights(state: &SettingsState) -> Vec<(f32, f32)> {
        state
            .render()
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                    // Exact equality on purpose: these are the very floats
                    // the renderer pushed, not a measurement of them.
                } if *x == 8.0 && *width == SIDEBAR_WIDTH - 16.0 => Some((*y, *height)),
                _ => None,
            })
            .collect()
    }

    fn click_sidebar(state: &mut SettingsState, y: f32) {
        state.handle_event(&Event::Mouse(MouseEvent {
            x: 100.0,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
    }

    fn hover_sidebar(state: &mut SettingsState, y: f32) -> Option<usize> {
        state.handle_event(&Event::Mouse(MouseEvent {
            x: 100.0,
            y,
            kind: MouseEventKind::Move,
        }));
        state.sidebar_hovered
    }

    #[test]
    fn every_category_is_clickable_exactly_where_it_was_painted() {
        for (idx, category) in SettingsCategory::ALL.iter().enumerate() {
            // Render with this category selected so its highlight -- and only
            // its highlight -- is in the tree, then take the rectangle from
            // there rather than recomputing it.
            let mut state = SettingsState::new();
            state.current_category = *category;
            state.current_page = category.default_page();
            let rects = category_highlights(&state);
            assert_eq!(
                rects.len(),
                1,
                "{category:?}: exactly one highlight is drawn when nothing is hovered"
            );
            let (top, height) = rects[0];

            // Sweep the painted row rather than probing its centre: a
            // divergence between the drawn rectangle and the accepted region
            // lives at the edges, never in the middle.
            for step in 0..8 {
                let probe = top + (step as f32 + 0.5) * height / 8.0;
                // Start from a category that is definitely not the answer, so
                // a hit test that silently does nothing cannot pass.
                let other = if idx == 0 { 1 } else { 0 };
                let mut probe_state = SettingsState::new();
                probe_state.current_category = SettingsCategory::ALL[other];
                click_sidebar(&mut probe_state, probe);
                assert_eq!(
                    probe_state.current_category,
                    *category,
                    "the row painted at {top}..{} answers y={probe} as {:?}",
                    top + height,
                    probe_state.current_category
                );
                assert_eq!(
                    probe_state.current_page,
                    category.default_page(),
                    "selecting {category:?} opens its default page"
                );
                assert_eq!(
                    hover_sidebar(&mut probe_state, probe),
                    Some(idx),
                    "hover and click must agree at y={probe}"
                );
            }
        }
    }

    #[test]
    fn the_gaps_between_category_rows_belong_to_no_row() {
        // The renderer leaves four pixels blank between one highlight and the
        // next. A hit test that claimed them would put the hover highlight a
        // few pixels above the pointer that summoned it.
        let mut state = SettingsState::new();
        for idx in 0..SettingsCategory::ALL.len() {
            let painted_bottom =
                SettingsState::category_row_top(idx) + SettingsState::CATEGORY_ROW_PAINTED_HEIGHT;
            let next_top = SettingsState::category_row_top(idx + 1);
            assert!(next_top > painted_bottom, "there is a gap to test");
            let mut probe = painted_bottom;
            while probe < next_top {
                assert_eq!(
                    hover_sidebar(&mut state, probe),
                    None,
                    "y={probe} is in the gap below row {idx}, which is painted blank"
                );
                probe += 1.0;
            }
        }
    }

    #[test]
    fn nothing_outside_the_category_list_selects_a_category() {
        let mut state = SettingsState::new();
        let start = state.current_category;

        // Above the list: the title and the search box.
        for y in [0.0, 10.0, SettingsState::search_top(), 105.0] {
            click_sidebar(&mut state, y);
            assert_eq!(
                state.current_category, start,
                "y={y} is above the category list"
            );
            assert_eq!(hover_sidebar(&mut state, y), None);
        }

        // Below the last row, all the way to the bottom of the window.
        let mut y = SettingsState::category_list_bottom();
        while y < state.window_height {
            click_sidebar(&mut state, y);
            assert_eq!(
                state.current_category, start,
                "y={y} is past the last category row"
            );
            assert_eq!(hover_sidebar(&mut state, y), None, "y={y} is past the list");
            y += 7.0;
        }

        // And to the right of the sidebar, level with a row that would
        // otherwise answer.
        let inside = SettingsState::category_row_top(3) + 4.0;
        assert_eq!(SettingsState::category_at(SIDEBAR_WIDTH, inside), None);
        assert_eq!(SettingsState::category_at(f32::NAN, inside), None);
        assert_eq!(SettingsState::category_at(100.0, f32::NAN), None);
        assert_eq!(SettingsState::category_at(100.0, f32::INFINITY), None);
        assert_eq!(SettingsState::category_at(100.0, f32::NEG_INFINITY), None);
    }

    #[test]
    fn the_hover_highlight_lands_under_the_pointer() {
        // Selected and hovered rows are drawn with the same rectangle, so this
        // hovers a row that is *not* selected and checks that the extra
        // rectangle which appears contains the pointer.
        let mut state = SettingsState::new();
        state.current_category = SettingsCategory::ALL[0];
        state.current_page = SettingsCategory::ALL[0].default_page();

        let probe = SettingsState::category_row_top(5) + 1.0;
        assert_eq!(hover_sidebar(&mut state, probe), Some(5));

        let rects = category_highlights(&state);
        assert_eq!(rects.len(), 2, "the selected row and the hovered row");
        assert!(
            rects
                .iter()
                .any(|&(top, h)| probe >= top && probe < top + h),
            "the pointer at {probe} is inside one of {rects:?}"
        );
    }

    #[test]
    fn the_whole_category_list_fits_the_window() {
        // There is no clip and no scroll on the sidebar: every category is
        // drawn at a fixed offset, so a list taller than the window has rows
        // that cannot be reached at all. When this fires, the fix is to give
        // the sidebar a `guitk::scroll_window` and a wheel accumulator like
        // the notification pane's, not to shrink the row height.
        let state = SettingsState::new();
        assert!(
            SettingsState::category_list_bottom() <= state.window_height,
            "{} categories need {}px but the window is {}px tall",
            SettingsCategory::ALL.len(),
            SettingsState::category_list_bottom(),
            state.window_height
        );
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

    // ---- Clicking what is actually drawn ----
    //
    // These ask the page where it put a control and then click there, rather
    // than recomputing the position from the same constants the page used. A
    // test that carries its own copy of the layout arithmetic passes happily
    // while the real hit test disagrees with the real renderer — which is the
    // exact bug this file was restructured to make impossible, so the tests
    // must not reintroduce it one level up.

    /// A sink that records every click band the page registers.
    struct RectSink {
        x: f32,
        y: f32,
        rects: Vec<(RowHit, (f32, f32, f32, f32))>,
    }

    impl PageSink for RectSink {
        fn x(&self) -> f32 {
            self.x
        }
        fn y(&self) -> f32 {
            self.y
        }
        fn advance(&mut self, dy: f32) {
            self.y += dy;
        }
        fn draw(&mut self, _f: impl FnOnce(&mut RenderTree, f32, f32)) {}
        fn hit_rect(&mut self, x: f32, y: f32, w: f32, h: f32, what: RowHit) {
            self.rects.push((what, (x, y, w, h)));
        }
    }

    /// Every click band the current page registers, in the order it draws them.
    fn hit_bands(state: &SettingsState) -> Vec<(RowHit, (f32, f32, f32, f32))> {
        let mut sink = RectSink {
            x: SettingsState::content_x(),
            y: SettingsState::content_top(),
            rects: Vec::new(),
        };
        state.build_page(&mut sink);
        sink.rects
    }

    /// The middle of the band the current page gives `what`, or `None` if it
    /// has none.
    fn center_of(state: &SettingsState, what: RowHit) -> Option<(f32, f32)> {
        hit_bands(state)
            .into_iter()
            .find(|(w, _)| *w == what)
            .map(|(_, (x, y, w, h))| (x + w / 2.0, y + h / 2.0))
    }

    /// Click the middle of `what`'s band on `page`. Panics if the page has no
    /// such band, which is the failure these tests are looking for.
    fn click_control(state: &mut SettingsState, page: SettingsPage, what: RowHit) {
        state.current_page = page;
        let (cx, cy) = center_of(state, what)
            .unwrap_or_else(|| panic!("{} has no click target for {what:?}", page.label()));
        state.handle_click(cx, cy);
    }

    /// Every page the sidebar can reach.
    fn all_pages() -> Vec<SettingsPage> {
        SettingsCategory::ALL
            .iter()
            .flat_map(|c| c.pages().iter().copied())
            .collect()
    }

    /// The switches a page currently offers.
    fn toggles_on_page(state: &SettingsState) -> Vec<ToggleId> {
        hit_bands(state)
            .into_iter()
            .filter_map(|(what, _)| match what {
                RowHit::Toggle(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    /// `page` with every switch on it turned on.
    ///
    /// Several controls only exist once the feature above them is enabled —
    /// the narrator's verbosity, the per-app permission lists — and a sweep
    /// over the default state would report those as unreachable when they are
    /// merely not shown yet.
    fn fully_expanded(page: SettingsPage) -> SettingsState {
        let mut state = SettingsState::new();
        state.current_page = page;
        // Turning one switch on can reveal another, so repeat until the set
        // stops growing. Bounded because nothing here turns a switch back off.
        for _ in 0..8 {
            let mut changed = false;
            for id in toggles_on_page(&state) {
                if let Some(flag) = state.toggle_mut(id)
                    && !*flag
                {
                    *flag = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        state
    }

    /// A state sitting on the page that draws `what`'s band, or `None` if no
    /// page draws it at all.
    fn state_showing(what: RowHit) -> Option<SettingsState> {
        all_pages()
            .into_iter()
            .map(fully_expanded)
            .find(|s| center_of(s, what).is_some())
    }

    // --- Buttons that cannot act -------------------------------------------
    //
    // A button is drawn live exactly when it has somewhere to send a click.
    // The checks below take the two halves of that claim from two independent
    // places -- the paint comes out of the render tree, the clickability out of
    // the page's click bands -- so neither can move the other with it.

    /// Every push button the current page actually painted, as
    /// `(label, x, y, fill colour, label colour)`.
    ///
    /// Recovered from the render tree rather than from the page walk. A button
    /// is a fill exactly [`BUTTON_HEIGHT`] tall, as wide as [`button_width`]
    /// makes its label, immediately followed by that label drawn at the
    /// renderer's fixed offset inside it. Reading the pixels is the point: a
    /// test that asked the page walk which buttons exist could not notice a
    /// button drawn by some other path, which is exactly how "Change Password"
    /// came to be painted with no click band and no way to find out.
    fn painted_buttons(state: &SettingsState) -> Vec<(String, f32, f32, Color, Color)> {
        const LABEL_DX: f32 = 12.0;
        const LABEL_DY: f32 = 8.0;
        const LABEL_SIZE: f32 = 13.0;

        let tree = state.render();
        let mut out = Vec::new();
        for pair in tree.commands.windows(2) {
            let (
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                },
                RenderCommand::Text {
                    x: tx,
                    y: ty,
                    text,
                    color: ink,
                    font_size,
                    ..
                },
            ) = (&pair[0], &pair[1])
            else {
                continue;
            };
            let close = |a: f32, b: f32| (a - b).abs() < 0.01;
            if close(*height, BUTTON_HEIGHT)
                && close(*font_size, LABEL_SIZE)
                && close(*tx, x + LABEL_DX)
                && close(*ty, y + LABEL_DY)
                && close(button_width(text), *width)
            {
                out.push((text.clone(), *x, *y, *color, *ink));
            }
        }
        out
    }

    /// Every state worth sweeping for painted buttons: each page with its
    /// switches turned on, and one such state per user account.
    ///
    /// The per-account repetition is not padding. "Manage Family Settings" is
    /// drawn only while a child account is selected, so a sweep that took the
    /// default selection would report six inert buttons where there are seven
    /// and would go on passing if the seventh were wired wrongly.
    fn states_to_sweep() -> Vec<(SettingsPage, SettingsState)> {
        let mut out = Vec::new();
        for page in all_pages() {
            let accounts = fully_expanded(page).user_accounts.len().max(1);
            for index in 0..accounts {
                let mut state = fully_expanded(page);
                state.selected_account = index;
                out.push((page, state));
            }
        }
        out
    }

    /// Whether a `Press` band on this page covers (`cx`, `cy`).
    fn press_band_covers(state: &SettingsState, cx: f32, cy: f32) -> bool {
        hit_bands(state).into_iter().any(|(what, (x, y, w, h))| {
            matches!(what, RowHit::Press(_)) && cx >= x && cx < x + w && cy >= y && cy < y + h
        })
    }

    /// The claim, over every button on every page: a button that looks
    /// pressable is pressable, and one that is not says so.
    ///
    /// Both directions matter and they fail differently. A live-looking button
    /// with no band is the original complaint — the app promising an action it
    /// cannot perform, leaving the user unable to tell "not implemented" from
    /// "my click missed". A dimmed button that *is* clickable is the opposite
    /// and worse: a working feature the user has been told not to try.
    ///
    /// The fill and the label are checked separately because they are two
    /// commands and can disagree. A dimmed fill under a full-brightness label
    /// is half a disabled button, and reads on screen as a live one — mutating
    /// the label colour back was not caught until this assertion existed.
    #[test]
    fn a_button_looks_live_exactly_when_it_is_live() {
        let mut seen = 0_usize;
        for (page, state) in states_to_sweep() {
            for (label, bx, by, fill, ink) in painted_buttons(&state) {
                seen += 1;
                let cx = bx + button_width(&label) / 2.0;
                let cy = by + BUTTON_HEIGHT / 2.0;
                let clickable = press_band_covers(&state, cx, cy);
                let looks_live = fill != COL_SURFACE0;
                assert_eq!(
                    looks_live,
                    clickable,
                    "on {}, \"{label}\" is painted {} but is {}",
                    page.label(),
                    if looks_live { "live" } else { "dimmed" },
                    if clickable { "clickable" } else { "inert" },
                );
                let want_ink = if looks_live { COL_CRUST } else { COL_OVERLAY0 };
                assert_eq!(
                    ink,
                    want_ink,
                    "on {}, \"{label}\" has a {} fill under a {} label",
                    page.label(),
                    if looks_live { "live" } else { "dimmed" },
                    if ink == COL_CRUST { "live" } else { "dimmed" },
                );
            }
        }
        assert!(
            seen >= 8,
            "the scan found only {seen} buttons, so it is not finding them"
        );
    }

    /// The census, pinned. These seven are the buttons whose features do not
    /// exist yet -- an accounts service, a stored activity log, package
    /// generation rollback, a reinstall path -- and they are on record in
    /// `known-issues.md` under `C-SETTINGS-BUTTONS-WITH-NOTHING-BEHIND-THEM`.
    ///
    /// Written as an exact list rather than a count so it fails in both
    /// directions: wiring one up without striking it off here fails, and — the
    /// case actually worth catching — adding an eighth inert button fails too,
    /// rather than quietly enlarging the set of things the app cannot do.
    #[test]
    fn the_buttons_with_nothing_behind_them_are_the_ones_on_record() {
        let mut inert: Vec<String> = Vec::new();
        for (_, state) in states_to_sweep() {
            for (label, _, _, fill, _) in painted_buttons(&state) {
                if fill == COL_SURFACE0 && !inert.contains(&label) {
                    inert.push(label);
                }
            }
        }
        inert.sort();
        let found: Vec<&str> = inert.iter().map(String::as_str).collect();
        assert_eq!(
            found,
            [
                "+ Add Account",
                "- Remove Account",
                "Change Password",
                "Clear Activity History",
                "Go Back",
                "Manage Family Settings",
                "Reset",
            ]
        );
    }

    /// A dimmed button must not swallow the click it cannot use.
    ///
    /// Registering a band and doing nothing would look identical on screen and
    /// be worse underneath: the click would stop there instead of reaching
    /// whatever is drawn beneath, and the user would lose even the "nothing
    /// happened here" feedback. So the check is that pressing one changes
    /// nothing about the app at all.
    #[test]
    fn pressing_a_dimmed_button_is_ignored_rather_than_swallowed() {
        let mut checked = 0_usize;
        for (page, state) in states_to_sweep() {
            for (label, bx, by, fill, _) in painted_buttons(&state) {
                if fill != COL_SURFACE0 {
                    continue;
                }
                checked += 1;
                let cx = bx + button_width(&label) / 2.0;
                let cy = by + BUTTON_HEIGHT / 2.0;
                let mut after = SettingsState::new();
                after.current_page = page;
                after.selected_account = state.selected_account;
                let before = after.render().commands.len();
                after.handle_click(cx, cy);
                assert_eq!(
                    after.render().commands.len(),
                    before,
                    "pressing the dimmed \"{label}\" on {} changed the page",
                    page.label()
                );
            }
        }
        assert!(checked >= 7, "only {checked} dimmed buttons were pressed");
    }

    // --- The account picture grid ------------------------------------------
    //
    // Six tiles that used to be painted inside one closure and hit-tested
    // nowhere. The checks below recover the tiles from the render tree and
    // drive them with real clicks, so where a tile is drawn and where a click
    // lands have to agree without either being asked to describe the other.

    /// Every account-picture tile the current page painted, in the order they
    /// were drawn, as `(icon, x, y, ringed)`.
    ///
    /// `ringed` is the accent outline that marks the chosen picture. It is
    /// looked up across the whole tree rather than assumed to follow its own
    /// tile's fill, so a ring drawn over the wrong tile shows up as a mismatch
    /// instead of being silently credited to the right one.
    fn painted_picture_tiles(state: &SettingsState) -> Vec<(String, f32, f32, bool)> {
        let close = |a: f32, b: f32| (a - b).abs() < 0.01;
        let tree = state.render();

        let mut rings: Vec<(f32, f32)> = Vec::new();
        for cmd in &tree.commands {
            if let RenderCommand::StrokeRect {
                x,
                y,
                width,
                height,
                color,
                ..
            } = cmd
                && *color == COL_ACCENT
                && close(*width, PICTURE_TILE_SIZE)
                && close(*height, PICTURE_TILE_SIZE)
            {
                rings.push((*x, *y));
            }
        }

        let mut out = Vec::new();
        for (idx, cmd) in tree.commands.iter().enumerate() {
            let RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } = cmd
            else {
                continue;
            };
            if !close(*width, PICTURE_TILE_SIZE) || !close(*height, PICTURE_TILE_SIZE) {
                continue;
            }
            // The icon follows its own fill within a command or two -- a
            // chosen tile has its ring in between.
            let icon = tree
                .commands
                .iter()
                .skip(idx + 1)
                .take(2)
                .find_map(|c| match c {
                    RenderCommand::Text {
                        x: tx, y: ty, text, ..
                    } if close(*tx, x + 12.0)
                        && close(*ty, y + 12.0)
                        && ACCOUNT_PICTURES.contains(&text.as_str()) =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                });
            let Some(icon) = icon else {
                continue;
            };
            let ringed = rings
                .iter()
                .any(|(rx, ry)| close(*rx, *x) && close(*ry, *y));
            out.push((icon, *x, *y, ringed));
        }
        out
    }

    /// The avatars drawn beside the names in the account list, row by row.
    ///
    /// Restricted to the content area on purpose: the sidebar draws the
    /// Accounts category with the same person glyph at the same size, and a
    /// scan that swept the whole window would count it as a fourth account.
    fn account_list_avatars(state: &SettingsState) -> Vec<String> {
        state
            .render()
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text {
                    x, text, font_size, ..
                } if *x >= SIDEBAR_WIDTH
                    && (*font_size - 16.0).abs() < 0.01
                    && ACCOUNT_PICTURES.contains(&text.as_str()) =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The page that carries the picture grid.
    fn login_options() -> SettingsState {
        let mut state = SettingsState::new();
        state.current_page = SettingsPage::LoginOptions;
        state
    }

    /// Every tile is pressable at the place it was painted, and pressing one
    /// moves the ring onto it and off everything else.
    ///
    /// This is the whole of the original complaint: the grid looked like a
    /// choice and was not one, because six tiles drawn in a single closure
    /// left the sink nowhere to hang six separate click bands.
    #[test]
    fn clicking_an_account_picture_chooses_the_one_that_was_clicked() {
        let tiles = painted_picture_tiles(&login_options());
        assert_eq!(
            tiles.len(),
            ACCOUNT_PICTURES.len(),
            "the page painted {} tiles, not {}",
            tiles.len(),
            ACCOUNT_PICTURES.len()
        );

        let bands = hit_bands(&login_options())
            .into_iter()
            .filter(|(what, _)| matches!(what, RowHit::Select(SelectId::AccountPicture, _)))
            .count();
        assert_eq!(
            bands,
            tiles.len(),
            "{bands} bands for {} tiles",
            tiles.len()
        );

        // Probed at the edges as well as the middle. A band shifted off its
        // tile still catches a click aimed at the centre -- the bands are as
        // wide as the tiles, so a small offset leaves the middle covered --
        // and only the corners tell whether the clickable square is the
        // square the user can see.
        let probes = [1.0, PICTURE_TILE_SIZE / 2.0, PICTURE_TILE_SIZE - 1.0];
        for (icon, x, y, _) in tiles {
            for px in probes {
                for py in probes {
                    let mut state = login_options();
                    state.handle_click(x + px, y + py);
                    let ringed: Vec<String> = painted_picture_tiles(&state)
                        .into_iter()
                        .filter(|(_, _, _, ringed)| *ringed)
                        .map(|(icon, _, _, _)| icon)
                        .collect();
                    assert_eq!(
                        ringed,
                        [icon.as_str()],
                        "clicking {px}px right and {py}px down from the {icon} tile's corner"
                    );
                }
            }
        }
    }

    /// The tiles stand apart and the whole strip fits the content column.
    ///
    /// This is the one claim about the grid's geometry that reading the paint
    /// against the click bands cannot make. Both come from
    /// [`picture_tile_dx`], deliberately -- that is what stops a tile being
    /// drawn where a click does not follow -- but it also means a wrong
    /// [`picture_tile_dx`] moves them together and looks consistent. So this
    /// checks the pixels against the *window*, which has no such shared
    /// origin: tiles that overlapped would make the leftmost of a stack the
    /// only reachable one, and a strip that ran past the content column would
    /// put pictures where nothing is clipped to catch them.
    #[test]
    fn the_picture_tiles_stand_apart_inside_the_content_column() {
        let state = login_options();
        let tiles = painted_picture_tiles(&state);
        for pair in tiles.windows(2) {
            assert!(
                pair[1].1 > pair[0].1 + PICTURE_TILE_SIZE,
                "the {} tile at x={} runs into the {} tile at x={}",
                pair[0].0,
                pair[0].1,
                pair[1].0,
                pair[1].1
            );
        }
        let right = tiles.last().expect("the grid painted no tiles").1 + PICTURE_TILE_SIZE;
        assert!(
            right <= state.window_width - CONTENT_PADDING,
            "the picture strip reaches x={right} in a {}px window",
            state.window_width
        );
    }

    /// The ring sits on the picture the signed-in account actually has --
    /// not on whichever tile happens to be drawn first, which is what it did
    /// before there was anywhere to store the answer.
    #[test]
    fn the_ringed_tile_is_the_signed_in_account_s_own_picture() {
        for index in 0..ACCOUNT_PICTURES.len() {
            let mut state = login_options();
            for account in &mut state.user_accounts {
                if account.is_current {
                    account.picture = index;
                }
            }
            let ringed: Vec<String> = painted_picture_tiles(&state)
                .into_iter()
                .filter(|(_, _, _, ringed)| *ringed)
                .map(|(icon, _, _, _)| icon)
                .collect();
            assert_eq!(ringed, [ACCOUNT_PICTURES[index].to_string()]);
        }
    }

    /// "Choose a picture for *your* account" means the account that is signed
    /// in. Which row the Accounts page has highlighted is a different
    /// question and must not decide whose picture changes.
    #[test]
    fn choosing_a_picture_edits_the_signed_in_account_not_the_highlighted_one() {
        let mut state = login_options();
        state.selected_account = 1;

        // The ring, too, and before any click: reading the ring from
        // `selected_account` gives the same answer as reading it from
        // `is_current` on a fresh state, because both are Alice. Moving the
        // highlight is what tells the two apart.
        let ringed: Vec<String> = painted_picture_tiles(&state)
            .into_iter()
            .filter(|(_, _, _, ringed)| *ringed)
            .map(|(icon, _, _, _)| icon)
            .collect();
        let signed_in = state
            .user_accounts
            .iter()
            .find(|a| a.is_current)
            .expect("somebody is signed in")
            .picture;
        assert_eq!(
            ringed,
            [ACCOUNT_PICTURES[signed_in].to_string()],
            "the ring followed the highlighted account, not the signed-in one"
        );

        let others: Vec<usize> = state
            .user_accounts
            .iter()
            .filter(|a| !a.is_current)
            .map(|a| a.picture)
            .collect();

        let (_, x, y, _) = painted_picture_tiles(&state)[3].clone();
        state.handle_click(x + PICTURE_TILE_SIZE / 2.0, y + PICTURE_TILE_SIZE / 2.0);

        assert_eq!(
            state
                .user_accounts
                .iter()
                .find(|a| a.is_current)
                .map(|a| a.picture),
            Some(3)
        );
        assert_eq!(
            state
                .user_accounts
                .iter()
                .filter(|a| !a.is_current)
                .map(|a| a.picture)
                .collect::<Vec<_>>(),
            others,
            "a click on the picture grid moved somebody else's picture"
        );
    }

    /// The account list shows each account its own picture.
    ///
    /// The expected icons are spelled out rather than read back out of the
    /// accounts, because a list that drew one hardcoded avatar three times --
    /// which is what it did -- would otherwise pass by agreeing with itself.
    #[test]
    fn the_account_list_draws_each_account_s_own_picture() {
        let mut state = SettingsState::new();
        state.current_page = SettingsPage::UserAccounts;
        assert_eq!(
            account_list_avatars(&state),
            ["\u{1F469}", "\u{1F468}", "\u{1F476}"]
        );
    }

    /// Choosing a picture is visible where the picture is used, not only on
    /// the page that offers the choice.
    #[test]
    fn choosing_a_picture_changes_the_avatar_in_the_account_list() {
        let mut state = login_options();
        let (icon, x, y, _) = painted_picture_tiles(&state)[4].clone();
        state.handle_click(x + PICTURE_TILE_SIZE / 2.0, y + PICTURE_TILE_SIZE / 2.0);
        state.current_page = SettingsPage::UserAccounts;
        assert_eq!(
            account_list_avatars(&state),
            [icon.as_str(), "\u{1F468}", "\u{1F476}"]
        );
    }

    /// A picture index the grid does not offer is refused rather than stored,
    /// so the field can never name a picture that does not exist -- which is
    /// what lets the account list draw it without a fallback path that would
    /// go untested.
    #[test]
    fn a_picture_the_grid_does_not_offer_is_refused() {
        let mut state = login_options();
        let before: Vec<usize> = state.user_accounts.iter().map(|a| a.picture).collect();
        state.apply_row_hit(
            RowHit::Select(SelectId::AccountPicture, ACCOUNT_PICTURES.len()),
            0.0,
        );
        assert_eq!(
            state
                .user_accounts
                .iter()
                .map(|a| a.picture)
                .collect::<Vec<_>>(),
            before
        );
    }

    /// Press the left button at (`mx`, `my`), drag to `to_x`, and release.
    ///
    /// Goes through `handle_event` rather than the individual handlers so the
    /// test exercises the same three events the compositor delivers; a slider
    /// that only worked when its press and move were called directly would be
    /// a slider that does not work.
    fn drag(state: &mut SettingsState, mx: f32, my: f32, to_x: f32) {
        for (x, kind) in [
            (mx, MouseEventKind::Press(MouseButton::Left)),
            (to_x, MouseEventKind::Move),
            (to_x, MouseEventKind::Release(MouseButton::Left)),
        ] {
            state.handle_event(&Event::Mouse(MouseEvent { x, y: my, kind }));
        }
    }

    #[test]
    fn test_every_dropdown_has_something_that_opens_it() {
        // Three of the ten were drawn with no opener at all: the page's click
        // handler stopped short of them. Walking the enum rather than a list
        // written by hand is what makes this catch the eleventh as well.
        for id in DropdownId::ALL {
            let mut state = state_showing(RowHit::Dropdown(id))
                .unwrap_or_else(|| panic!("no page draws a row for {id:?}"));
            let (cx, cy) = center_of(&state, RowHit::Dropdown(id)).expect("just found it");
            state.handle_click(cx, cy);
            assert_eq!(
                state.open_dropdown,
                Some(id),
                "clicking {id:?}'s row on {} opened {:?}",
                state.current_page.label(),
                state.open_dropdown
            );
        }
    }

    #[test]
    fn test_an_open_dropdown_appears_under_its_own_button() {
        // The popup used to carry a hand-written anchor per dropdown, so it
        // could open several rows away from the button that was pressed.
        for id in DropdownId::ALL {
            let mut state = state_showing(RowHit::Dropdown(id))
                .unwrap_or_else(|| panic!("no page draws a row for {id:?}"));
            let (cx, cy) = center_of(&state, RowHit::Dropdown(id)).expect("just found it");
            state.handle_click(cx, cy);
            let layout = state
                .dropdown_layout()
                .unwrap_or_else(|| panic!("{id:?} opened but has no layout"));
            assert!(
                (layout.x - SettingsState::content_x() - CONTROL_COLUMN_DX).abs() < 0.01,
                "{id:?} popup is not in the control column"
            );
            // The popup is pulled up if it would run off the bottom, so it may
            // sit above the row — never below it.
            assert!(
                layout.y <= cy,
                "{id:?} popup starts at {} but its button is at {cy}",
                layout.y
            );
        }
    }

    #[test]
    fn test_every_slider_has_a_page_that_draws_it_draggable() {
        // All eight sliders painted correctly and none of them moved: the pages
        // registered no click band for a slider at all. Walking the enum is what
        // makes this catch the ninth as well.
        for id in SliderId::FIXED {
            assert!(
                state_showing(RowHit::Slider(id)).is_some(),
                "no page offers a grab band for {id:?}"
            );
        }
        // The per-application volumes are indexed, so they are checked against
        // the list the Sound page actually shows rather than a constant.
        let state = fully_expanded(SettingsPage::Sound);
        assert!(
            !state.app_volumes.is_empty(),
            "the Sound page has no per-app volumes to check"
        );
        for index in 0..state.app_volumes.len() {
            assert!(
                center_of(&state, RowHit::Slider(SliderId::AppVolume(index))).is_some(),
                "per-app volume {index} has no grab band"
            );
        }
    }

    #[test]
    fn test_a_press_on_the_painted_track_grabs_the_slider() {
        // The band and the bar are two readings of `slider_track`, and this is
        // what holds them together: it presses a point that is unambiguously on
        // the *painted* bar — taken from the anchor the renderer draws from, not
        // from the band — and requires that the press take hold.
        //
        // Without it, a band that drifted a row down from its own track would
        // pass every other slider test here, because those measure from the
        // band and so drift with it. The user would see a slider that does
        // nothing and a row below it that jumps when brushed.
        for id in SliderId::FIXED {
            let mut state =
                state_showing(RowHit::Slider(id)).unwrap_or_else(|| panic!("{id:?} is not drawn"));
            let (track_x, track_y) = state
                .anchor_at(AnchorId::Slider(id))
                .expect("a drawn slider has a track");
            for along in [0.0, 0.5, 1.0] {
                state.dragging = None;
                state.handle_click(
                    SLIDER_WIDTH.mul_add(along, track_x),
                    track_y + SLIDER_HEIGHT / 2.0,
                );
                assert_eq!(
                    state.dragging,
                    Some(id),
                    "a press {along} of the way along {id:?}'s painted track did not grab it"
                );
            }
        }
    }

    #[test]
    fn test_dragging_a_slider_to_each_end_reaches_its_limits() {
        // Both ends, because a mapping that is off by its offset still moves
        // the handle — it just never reaches one extreme. Text Size is the case
        // that would slip past a from-zero check: its range starts at 50.
        for id in SliderId::FIXED {
            let mut state =
                state_showing(RowHit::Slider(id)).unwrap_or_else(|| panic!("{id:?} is not drawn"));
            let (_, cy) = center_of(&state, RowHit::Slider(id)).expect("just found it");
            let (track_x, _) = state
                .anchor_at(AnchorId::Slider(id))
                .expect("a drawn slider has a track");
            let (lo, hi) = id.range();

            drag(&mut state, track_x, cy, track_x + SLIDER_WIDTH);
            assert_eq!(
                state.slider_raw(id),
                Some(hi),
                "{id:?} dragged to the right end did not reach its maximum"
            );

            drag(&mut state, track_x + SLIDER_WIDTH, cy, track_x);
            assert_eq!(
                state.slider_raw(id),
                Some(lo),
                "{id:?} dragged to the left end did not reach its minimum"
            );
        }
    }

    #[test]
    fn test_a_slider_drag_follows_the_pointer_past_the_track() {
        // The pointer routinely leaves the six-pixel bar mid-gesture. A drag
        // that stopped there — or that clamped to the wrong end — would make
        // the control unusable in exactly the way a user would first try it.
        let id = SliderId::OutputVolume;
        let mut state = state_showing(RowHit::Slider(id)).expect("Sound draws the volume slider");
        let (_, cy) = center_of(&state, RowHit::Slider(id)).expect("just found it");
        let (track_x, _) = state.anchor_at(AnchorId::Slider(id)).expect("has a track");

        // Press in the middle, then wander far above the row and off to the
        // right. The value should follow x and ignore y entirely.
        state.handle_event(&Event::Mouse(MouseEvent {
            x: track_x + SLIDER_WIDTH / 2.0,
            y: cy,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(
            state.output_volume, 50,
            "the press did not jump to midpoint"
        );
        state.handle_event(&Event::Mouse(MouseEvent {
            x: track_x + SLIDER_WIDTH * 4.0,
            y: 0.0,
            kind: MouseEventKind::Move,
        }));
        assert_eq!(
            state.output_volume, 100,
            "the drag did not follow past the end"
        );
        state.handle_event(&Event::Mouse(MouseEvent {
            x: track_x + SLIDER_WIDTH / 4.0,
            y: 10_000.0,
            kind: MouseEventKind::Move,
        }));
        assert_eq!(state.output_volume, 25, "the drag stopped following");

        // After release the pointer moves freely again.
        state.handle_event(&Event::Mouse(MouseEvent {
            x: track_x,
            y: cy,
            kind: MouseEventKind::Release(MouseButton::Left),
        }));
        state.handle_event(&Event::Mouse(MouseEvent {
            x: track_x,
            y: cy,
            kind: MouseEventKind::Move,
        }));
        assert_eq!(state.output_volume, 25, "a released slider still followed");
    }

    #[test]
    fn test_a_slider_handle_is_drawn_where_a_drag_would_put_it() {
        // The fraction the page draws at and the value a drag produces are one
        // mapping read in each direction. If they ever part, the handle sits
        // somewhere other than under the pointer that placed it.
        for id in SliderId::FIXED {
            let mut state =
                state_showing(RowHit::Slider(id)).unwrap_or_else(|| panic!("{id:?} is not drawn"));
            let (_, cy) = center_of(&state, RowHit::Slider(id)).expect("just found it");
            let (track_x, _) = state.anchor_at(AnchorId::Slider(id)).expect("has a track");

            for tenth in 0_u8..=10 {
                let wanted = f32::from(tenth) / 10.0;
                drag(
                    &mut state,
                    track_x,
                    cy,
                    SLIDER_WIDTH.mul_add(wanted, track_x),
                );
                let drawn = state
                    .slider_fraction(id)
                    .unwrap_or_else(|| panic!("{id:?} has no value"));
                // Rounding to a whole percent or a whole day moves the handle by
                // at most half a step, which is what this tolerance allows for.
                let (lo, hi) = id.range();
                let step = 1.0 / (hi - lo);
                assert!(
                    (drawn - wanted).abs() <= step / 2.0 + 0.001,
                    "{id:?} dragged to {wanted} draws its handle at {drawn}"
                );
            }
        }
    }

    #[test]
    fn test_a_slider_readout_agrees_with_its_handle() {
        // The number beside the track used to be formatted at the call site
        // from the same field the fraction was computed from by hand, so the
        // two could drift apart. Now both come from one value; this holds them
        // to it at the ends, where a drifted mapping shows up first.
        let id = SliderId::TextSize;
        let mut state = state_showing(RowHit::Slider(id)).expect("drawn on Visual accessibility");
        let (_, cy) = center_of(&state, RowHit::Slider(id)).expect("just found it");
        let (track_x, _) = state.anchor_at(AnchorId::Slider(id)).expect("has a track");

        drag(&mut state, track_x, cy, track_x);
        assert_eq!(
            id.readout(state.slider_raw(id).unwrap()).as_deref(),
            Some("50%")
        );
        drag(&mut state, track_x, cy, track_x + SLIDER_WIDTH);
        assert_eq!(
            id.readout(state.slider_raw(id).unwrap()).as_deref(),
            Some("250%")
        );
    }

    #[test]
    fn test_a_press_on_a_row_label_does_not_move_its_slider() {
        // The grab band deliberately stops short of the label. A whole-row band
        // — which is what every other control here uses — would mean brushing
        // the words "Text Size" slammed it to 50%.
        let id = SliderId::TextSize;
        let mut state = state_showing(RowHit::Slider(id)).expect("drawn on Visual accessibility");
        let before = state.text_size_percent;
        let (_, cy) = center_of(&state, RowHit::Slider(id)).expect("just found it");
        state.handle_click(SettingsState::content_x() + 4.0, cy);
        assert_eq!(
            state.text_size_percent, before,
            "clicking the label moved the slider"
        );
        assert!(
            state.dragging.is_none(),
            "clicking the label started a drag"
        );
    }

    #[test]
    fn test_dragging_one_per_app_volume_leaves_the_others_alone() {
        // Indexed controls are where a shared handler goes wrong quietly: every
        // row looks right, and the wrong application gets muted.
        let id = SliderId::AppVolume(1);
        let mut state = fully_expanded(SettingsPage::Sound);
        assert!(
            state.app_volumes.len() >= 2,
            "need two apps to tell them apart"
        );
        let others: Vec<u8> = state
            .app_volumes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, a)| a.volume)
            .collect();
        let (_, cy) = center_of(&state, RowHit::Slider(id)).expect("app 1 has a band");
        let (track_x, _) = state.anchor_at(AnchorId::Slider(id)).expect("has a track");

        drag(&mut state, track_x, cy, track_x + SLIDER_WIDTH);
        assert_eq!(state.app_volumes[1].volume, 100);
        let after: Vec<u8> = state
            .app_volumes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, a)| a.volume)
            .collect();
        assert_eq!(after, others, "dragging one app's volume moved another's");
    }

    #[test]
    fn test_every_toggle_on_every_page_flips_its_own_field() {
        // A toggle wired to the wrong field is invisible until someone changes
        // a setting and a different one moves. Reading the field back through
        // its own id catches that, and catches a toggle that flips nothing.
        for page in all_pages() {
            for id in toggles_on_page(&fully_expanded(page)) {
                let mut state = fully_expanded(page);
                let before = *state
                    .toggle_mut(id)
                    .unwrap_or_else(|| panic!("{id:?} has no field"));
                let (cx, cy) = center_of(&state, RowHit::Toggle(id))
                    .unwrap_or_else(|| panic!("{id:?} vanished between walks"));
                state.handle_click(cx, cy);
                let after = *state
                    .toggle_mut(id)
                    .unwrap_or_else(|| panic!("{id:?} has no field"));
                assert_ne!(
                    before,
                    after,
                    "clicking {id:?} on {} changed nothing",
                    page.label()
                );
            }
        }
    }

    #[test]
    fn test_every_per_app_permission_switch_is_clickable() {
        // Only the Location list had a handler; Camera, Microphone and
        // Background were drawn and inert.
        let mut state = fully_expanded(SettingsPage::Permissions);
        for kind in [
            PermissionKind::Location,
            PermissionKind::Camera,
            PermissionKind::Microphone,
            PermissionKind::Background,
        ] {
            let count = match kind {
                PermissionKind::Location => state.location_apps.len(),
                PermissionKind::Camera => state.camera_apps.len(),
                PermissionKind::Microphone => state.microphone_apps.len(),
                PermissionKind::Background => state.background_apps.len(),
            };
            assert!(count > 0, "{kind:?} has no apps to test with");
            for idx in 0..count {
                let id = ToggleId::AppPermission(kind, idx);
                let before = *state.toggle_mut(id).expect("app exists");
                let (cx, cy) = center_of(&state, RowHit::Toggle(id))
                    .unwrap_or_else(|| panic!("{kind:?} app {idx} has no click target"));
                state.handle_click(cx, cy);
                assert_ne!(before, *state.toggle_mut(id).expect("app exists"));
            }
        }
    }

    // ---- Personalization: the shared appearance model ----
    //
    // These drive `handle_click` rather than `handle_event`, which would write
    // to the user's real `appearance.yaml`. The write itself is covered by
    // `appearance`'s own config tests, which point $HOME at a temporary
    // directory; what needs testing here is that the panel's geometry and the
    // model agree.

    #[test]
    fn test_theme_mode_card_click_sets_the_mode() {
        let mut state = SettingsState::new();
        // Second card: Light, since ThemeMode::ALL is Dark, Light, System.
        click_control(
            &mut state,
            SettingsPage::Themes,
            RowHit::Select(SelectId::ThemeMode, 1),
        );
        assert_eq!(state.appearance.settings.theme_mode, ThemeMode::Light);
    }

    #[test]
    fn test_every_transparency_level_can_be_picked() {
        // The row this replaced was an on/off switch, which could not express
        // Subtle or Moderate at all.
        for (idx, level) in TransparencyLevel::ALL.iter().enumerate() {
            let mut state = SettingsState::new();
            click_control(
                &mut state,
                SettingsPage::Themes,
                RowHit::Pill(PillId::Transparency, idx),
            );
            assert_eq!(
                state.appearance.settings.transparency,
                *level,
                "pill {idx} should select {}",
                level.label()
            );
        }
    }

    #[test]
    fn test_every_animation_speed_can_be_picked() {
        for (idx, speed) in AnimationSpeed::ALL.iter().enumerate() {
            let mut state = SettingsState::new();
            click_control(
                &mut state,
                SettingsPage::Themes,
                RowHit::Pill(PillId::AnimationSpeed, idx),
            );
            assert_eq!(state.appearance.settings.animation_speed, *speed);
        }
    }

    #[test]
    fn test_a_click_in_the_gap_between_pills_changes_nothing() {
        let mut state = SettingsState::new();
        state.current_page = SettingsPage::Themes;
        let (px, py) = center_of(&state, RowHit::Pill(PillId::Transparency, 0))
            .expect("the transparency row has pills");
        // Between the first and second pill: past the first pill's right edge,
        // short of where the second begins.
        state.handle_click(px + PILL_WIDTH / 2.0 + 2.0, py);
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
        for (idx, accent) in AccentColor::presets().iter().enumerate() {
            let mut state = SettingsState::new();
            click_control(
                &mut state,
                SettingsPage::Colors,
                RowHit::Select(SelectId::AccentColor, idx),
            );
            assert_eq!(
                state.appearance.settings.accent_color,
                *accent,
                "swatch {idx} should select {}",
                accent.label()
            );
        }
    }

    #[test]
    fn test_every_pointer_size_can_be_picked() {
        // The Interaction page drew five size buttons and had no handler for
        // any of them.
        for idx in 0..POINTER_SIZE_COUNT {
            let mut state = SettingsState::new();
            click_control(
                &mut state,
                SettingsPage::Interaction,
                RowHit::Select(SelectId::PointerSize, idx),
            );
            assert_eq!(state.pointer_size, pointer_size_of(idx));
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
            let saved =
                AppearanceSettings::read_from(&appearance::config::load(appearance::CONFIG_NAME));
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
        drawn_dropdown_rows(state)
            .into_iter()
            .map(|(label, _)| label)
            .collect()
    }

    /// The baseline offset the dropdown renderer draws an item's text at,
    /// measured down from the top of the item's own row.
    ///
    /// This is the anchor that lets a test recover the *painted* row top from
    /// the render tree. It is checked against a rectangle rather than trusted:
    /// see `the_selected_rows_highlight_confirms_where_the_rows_are_painted`.
    const DRAWN_ITEM_TEXT_BASELINE: f32 = 10.0;

    /// Every item the open dropdown drew, as `(label, painted row top)`.
    ///
    /// Scoped to the commands the popup itself emitted — everything from its
    /// drop shadow onwards — rather than to an x/y box, so that moving the
    /// popup cannot silently turn this into a filter that matches nothing.
    ///
    /// The row top comes out of the `Text` command the renderer pushed, *not*
    /// out of `DropdownLayout::row_top`. That distinction is the whole point:
    /// a test that positions its probes with the same function the hit test
    /// inverts is not testing the hit test at all — move the renderer and the
    /// probes move with it, and the test passes through the drift it exists to
    /// catch. This one caught nothing when `row_top` was shifted three pixels,
    /// which is how the flaw was found.
    fn drawn_dropdown_rows(state: &SettingsState) -> Vec<(String, f32)> {
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
                    text, y, font_size, ..
                } if (font_size - 13.0).abs() < 0.01 => {
                    Some((text.clone(), y - DRAWN_ITEM_TEXT_BASELINE))
                }
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
            assert_eq!(click(&mut state, layout.x + 20.0, y), EventResult::Consumed);
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
            let drawn = drawn_dropdown_rows(&state);
            assert_eq!(
                drawn.len(),
                layout.window.count,
                "at {height}px the popup drew {} rows but claims {}",
                drawn.len(),
                layout.window.count
            );
            for (row, (label, top)) in drawn.iter().enumerate() {
                // `top` is where the renderer *put* this row, recovered from
                // the command it pushed — not `layout.row_top(row)`, which is
                // the same arithmetic `item_at` inverts and so would move in
                // step with any drift.
                let top = *top;
                // Sweep the row rather than probing its middle. A drift of a
                // few pixels is invisible at the centre of a 36px row and
                // shows up only at the edges, so a centre probe would pass
                // straight through the fault it exists to catch. The last
                // sample is the row's final pixel: a row owns its top edge and
                // not its bottom one.
                for step in 0..8 {
                    let y = top + (step as f32) * DROPDOWN_ITEM_HEIGHT / 8.0;
                    let index = layout
                        .item_at(layout.x + 20.0, y)
                        .unwrap_or_else(|| panic!("row {row} at y={y} hit nothing"));
                    assert_eq!(
                        layout.items.get(index),
                        Some(label),
                        "at {height}px, row {row} shows {label} but y={y} \
                         hit-tests to item {index}"
                    );
                }
                let last = top + DROPDOWN_ITEM_HEIGHT - 0.01;
                assert_eq!(
                    layout
                        .item_at(layout.x + 20.0, last)
                        .and_then(|i| layout.items.get(i)),
                    Some(label),
                    "at {height}px, row {row}'s last pixel does not belong to it"
                );
            }
        }
    }

    /// The row tops `drawn_dropdown_rows` recovers are read off a *text*
    /// baseline, which only works if the renderer really does draw an item's
    /// text `DRAWN_ITEM_TEXT_BASELINE` below the item's own top.
    ///
    /// The selected row is the one place the renderer paints a rectangle at a
    /// row's top edge, so it is the one place that assumption can be checked
    /// against something other than itself. Without this, moving the baseline
    /// would quietly move every probe in
    /// `the_hit_test_names_the_item_that_was_drawn_under_the_pointer` and turn
    /// it back into a test of nothing.
    #[test]
    fn the_selected_rows_highlight_confirms_where_the_rows_are_painted() {
        let mut state = SettingsState::new();
        state.resolution_index = 2;
        state.show_dropdown(DropdownId::Resolution);
        let layout = state.dropdown_layout().expect("a dropdown is open");
        let row_on_screen = layout
            .window
            .start
            .checked_sub(0)
            .and_then(|start| 2usize.checked_sub(start))
            .expect("the chosen item is on screen");
        let (_, from_text) = drawn_dropdown_rows(&state)
            .into_iter()
            .nth(row_on_screen)
            .expect("the chosen row was drawn");

        let tree = state.render();
        let shadow = tree
            .commands
            .iter()
            .position(|c| matches!(c, RenderCommand::BoxShadow { .. }))
            .expect("an open dropdown draws a shadow");
        let from_rect = tree
            .commands
            .get(shadow..)
            .unwrap_or_default()
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if (*x - (layout.x + 4.0)).abs() < 0.01
                    && (*width - (layout.width - 8.0)).abs() < 0.01
                    && (*height - (DROPDOWN_ITEM_HEIGHT - 2.0)).abs() < 0.01 =>
                {
                    Some(*y)
                }
                _ => None,
            })
            .expect("the chosen item draws a highlight at its own top edge");

        assert!(
            (from_text - from_rect).abs() < 0.01,
            "the text baseline says the row starts at {from_text}, the \
             highlight says {from_rect} -- the offset the test recovers row \
             tops with is stale"
        );
    }

    #[test]
    fn a_dropdown_coordinate_that_is_not_a_number_chooses_nothing() {
        // Every bounds test in `item_at` is a `<` or a `>=`, and a NaN is
        // neither less nor greater than anything -- so it passed all of them
        // by failing all of them, and `NaN as usize` is 0 rather than a trap.
        // The popup's first item was chosen for a click that is nowhere.
        let mut state = SettingsState::new();
        state.resolution_index = 2;
        state.show_dropdown(DropdownId::Resolution);
        let layout = state.dropdown_layout().expect("a dropdown is open");
        let inside_y = layout.row_top(0) + DROPDOWN_ITEM_HEIGHT / 2.0;
        let inside_x = layout.x + 20.0;
        for (mx, my) in [
            (f32::NAN, inside_y),
            (inside_x, f32::NAN),
            (f32::NAN, f32::NAN),
            (inside_x, f32::INFINITY),
            (f32::NEG_INFINITY, inside_y),
        ] {
            assert_eq!(layout.item_at(mx, my), None, "({mx}, {my}) named an item");
        }
        click(&mut state, f32::NAN, f32::NAN);
        assert_eq!(state.resolution_index, 2, "a NaN click changed the choice");
    }

    #[test]
    fn clicking_outside_an_open_dropdown_closes_it_without_choosing() {
        let mut state = SettingsState::new();
        state.resolution_index = 2;
        state.show_dropdown(DropdownId::Resolution);
        let layout = state.dropdown_layout().expect("a dropdown is open");
        // Well to the left of the popup, over the sidebar.
        assert_eq!(
            click(&mut state, 10.0, layout.y + 10.0),
            EventResult::Consumed
        );
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
                // On the page that actually draws it: the popup is anchored to
                // its own button, so opening it from elsewhere has no position
                // to be checked against.
                let mut state = state_showing(RowHit::Dropdown(id))
                    .unwrap_or_else(|| panic!("no page draws {id:?}"));
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
        assert_eq!(state.dropdown_scroll, 3, "one notch is three items");
        state.handle_event(&wheel(1.0));
        assert_eq!(state.dropdown_scroll, 0);
    }

    /// A trackpad sends fractions of a notch. `dropdown_scroll` counts whole
    /// items and cannot hold one, so the remainder has to be banked.
    ///
    /// The handler this replaced read only `dy`'s sign, which is the same bug
    /// seen from the other side: it could not tell a twentieth of a notch from
    /// a hard flick, so both moved the list three items and a genuine
    /// three-notch spin moved three items as well.
    #[test]
    fn a_trackpads_fractions_add_up_instead_of_each_moving_three_items() {
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
        // A tenth of a notch is three tenths of an item: nothing yet.
        state.handle_event(&wheel(-0.1));
        assert_eq!(state.dropdown_scroll, 0, "a twitch is not three items");
        for _ in 0..9 {
            state.handle_event(&wheel(-0.1));
        }
        assert_eq!(
            state.dropdown_scroll, 3,
            "ten tenths of a notch is one notch, which is three items"
        );
    }

    /// How hard the wheel is turned has to matter.
    #[test]
    fn three_notches_move_three_times_as_far_as_one() {
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
        state.handle_event(&wheel(-3.0));
        assert_eq!(state.dropdown_scroll, 9);
    }

    /// A fraction earned scrolling one dropdown must not step the next one.
    #[test]
    fn opening_a_dropdown_forgets_the_leftover_fraction() {
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
        // Two tenths of a notch: 0.6 of an item owed, none delivered.
        state.handle_event(&wheel(-0.1));
        state.handle_event(&wheel(-0.1));
        assert_eq!(state.dropdown_scroll, 0);

        state.show_dropdown(DropdownId::Resolution);
        state.dropdown_scroll = 0;
        // Were the 0.6 still banked, this 0.6 would complete an item.
        state.handle_event(&wheel(-0.1));
        state.handle_event(&wheel(-0.1));
        assert_eq!(state.dropdown_scroll, 0);
    }

    /// Input events come from outside the process, and a `NaN` in the residue
    /// would stop the dropdown scrolling for the rest of the run.
    #[test]
    fn a_nonfinite_delta_does_not_freeze_the_dropdown() {
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
        state.handle_event(&wheel(f32::NAN));
        state.handle_event(&wheel(f32::INFINITY));
        assert_eq!(state.dropdown_scroll, 0);
        state.handle_event(&wheel(-1.0));
        assert_eq!(state.dropdown_scroll, 3, "an ordinary notch still works");
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

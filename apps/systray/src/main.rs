#![allow(clippy::too_many_arguments)]
//! Slate OS System Tray — notification area and quick settings
//!
//! Provides the system tray (notification area) for the Slate OS taskbar.
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

use guitk::color::Color;
// The calendar popup's date arithmetic comes from the shared civil-date
// module rather than a local copy. See known-issues.md
// C-SIX-APPS-EACH-CARRIED-THEIR-OWN-CIVIL-DATE-ARITHMETIC.
use guitk::date;
use guitk::event::{Event, Key, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use oswindow::app::Response;
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

// A palette is a table: it is named in full so that a colour picked later
// reads as a choice from the scheme rather than a fresh hex literal, and the
// entries nothing currently draws with are the point of having it.
#[allow(dead_code)]
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

/// Gap between the tray bar and the popup that hangs above it.
const POPUP_GAP: f32 = 8.0;

/// Inset of the tray bar from the bottom-right corner of the screen.
const TRAY_INSET: f32 = 8.0;

// ============================================================================
// Shared geometry
// ============================================================================

/// An axis-aligned rectangle in window coordinates.
///
/// Every interactive element of the tray is placed *once*, into one of these,
/// by the layout functions below — and both the renderer and the hit-test read
/// it from there. Before this existed the renderers computed their positions
/// inline with a running `y` and there was no hit-test at all, so a click
/// anywhere in an open popup simply closed it: the toggles, the sliders and
/// the calendar's month arrows were drawn, and all of them were dead.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Half-open on both axes, so adjacent rects cannot both claim a pixel.
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// The fraction of the way across this rect that `x` falls, clamped to
    /// `0.0..=1.0` so a drag that leaves the track still tracks the pointer.
    fn fraction_across(self, x: f32) -> f32 {
        if self.w <= 0.0 {
            return 0.0;
        }
        ((x - self.x) / self.w).clamp(0.0, 1.0)
    }
}

// ============================================================================
// Interactive element identity
// ============================================================================

/// One of the quick-settings switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    Wifi,
    Bluetooth,
    DoNotDisturb,
    NightLight,
    BatterySaver,
    AirplaneMode,
}

/// One of the draggable sliders.
///
/// The network popup's signal-strength bar is drawn with the same helper but
/// is deliberately **not** here: signal strength is something the tray reports,
/// not something the user sets, and a draggable one would let you "turn up"
/// the reception.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slider {
    Brightness,
    MasterVolume,
    /// Index into [`VolumeState::app_volumes`].
    AppVolume(usize),
}

/// What the user asked the desktop to do. Everything the tray can do *to
/// itself* — toggling a switch, moving a slider, opening or closing a popup —
/// is applied in place and reported as [`TrayAction::None`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayAction {
    None,
    /// Bring the application that owns this tray icon to the front.
    OpenApp(TrayIconId),
    /// Open the network settings page.
    OpenNetworkSettings,
    Power(PowerAction),
}

/// The five entries of the power menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    ShutDown,
    Restart,
    Sleep,
    Lock,
    SignOut,
}

impl PowerAction {
    const ALL: [(PowerAction, char, &'static str); 5] = [
        (PowerAction::ShutDown, '\u{23FB}', "Shut Down"),
        (PowerAction::Restart, '\u{1F504}', "Restart"),
        (PowerAction::Sleep, '\u{1F4A4}', "Sleep"),
        (PowerAction::Lock, '\u{1F512}', "Lock"),
        (PowerAction::SignOut, '\u{1F6AA}', "Sign Out"),
    ];
}

/// A row of one of the three list-shaped popups (app menu, context menu,
/// power menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuEntry {
    OpenApp(TrayIconId),
    ShowIcon(TrayIconId),
    HideIcon(TrayIconId),
    RemoveIcon(TrayIconId),
    Power(PowerAction),
}

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
        // Behaviour change: a month outside 1..=12 used to render "???" and
        // now renders "Dec". Nothing constructs such a `DateTime` -- the
        // field is filled from the clock -- and a wrong-but-plausible month
        // name is no worse than a placeholder that has never been seen.
        let month_name = date::month_short_name(u32::from(self.month));
        format!("{} {}, {}", month_name, self.day, self.year)
    }

    /// Number of days in the current month.
    pub fn days_in_month(&self) -> u8 {
        // Behaviour change: an out-of-range month used to answer 30 and now
        // answers 31 (December, by the clamp). Both are arbitrary; the
        // difference is that this one comes from the same table the calendar
        // popup lays its grid out with.
        u8::try_from(date::days_in_month(
            i32::from(self.year),
            u32::from(self.month),
        ))
        .unwrap_or(31)
    }

    /// Day of week for the first day of the current month (0=Sunday).
    pub fn first_weekday_of_month(&self) -> u8 {
        // Was a local transcription of Sakamoto's algorithm. The calendar
        // popup indexes its first row by this value, so it has to agree with
        // the day count above; taking both from `guitk::date` is what makes
        // that agreement structural rather than a coincidence of two
        // separately-correct copies.
        let first = date::Date::from_ymd(i32::from(self.year), u32::from(self.month), 1);
        // `Weekday::index` is 0..=6 with 0 = Sunday, which is this method's
        // documented convention.
        u8::try_from(first.weekday().index()).unwrap_or(0)
    }
}

// ============================================================================
// Popup layouts
// ============================================================================

/// A quick-settings switch as the layout placed it.
struct ToggleRowLayout {
    toggle: Toggle,
    rect: Rect,
    label: &'static str,
    subtitle: String,
    enabled: bool,
}

/// Geometry of the quick-settings flyout.
struct QuickSettingsLayout {
    frame: Rect,
    content_x: f32,
    content_width: f32,
    header_y: f32,
    rows: Vec<ToggleRowLayout>,
    brightness_label_y: f32,
    brightness: Rect,
    volume_label_y: f32,
    volume: Rect,
}

/// One application's row in the volume mixer.
struct AppVolumeLayout {
    index: usize,
    label: Rect,
    slider: Rect,
}

/// Geometry of the volume flyout.
struct VolumeLayout {
    frame: Rect,
    content_x: f32,
    content_width: f32,
    header_y: f32,
    device_y: f32,
    master_label: Rect,
    master_slider: Rect,
    separator_y: f32,
    apps: Vec<AppVolumeLayout>,
}

/// Geometry of the network flyout.
struct NetworkLayout {
    frame: Rect,
    content_x: f32,
    content_width: f32,
    header_y: f32,
    status_y: f32,
    ssid_y: f32,
    signal_label_y: f32,
    signal_bar: Rect,
    ip_y: f32,
    settings: Rect,
}

/// Geometry of the calendar flyout.
///
/// The month drawn is not necessarily the current one: the header's arrows
/// move `month_offset` on the tray, and this layout resolves that into the
/// concrete year/month it is drawing. `today` is `None` when the displayed
/// month is not the month the clock is in, which is what makes the highlight
/// follow the real date instead of following whatever month is on screen.
struct CalendarLayout {
    frame: Rect,
    content_x: f32,
    content_width: f32,
    header_y: f32,
    prev: Rect,
    next: Rect,
    /// Clicking the month name returns the calendar to today.
    title: Rect,
    year: i32,
    month: u32,
    first_weekday: u32,
    days_in_month: u32,
    weekday_header_y: f32,
    grid_top: f32,
    today: Option<u32>,
    events_y: f32,
}

impl CalendarLayout {
    /// Rows the grid needs, which is what the popup's height is built from.
    /// A 31-day month that starts on a Saturday needs six; the popup used to
    /// be a fixed 280px tall, which is not enough for six, so the last row
    /// used to be drawn over the events separator.
    fn rows_needed(first_weekday: u32, days_in_month: u32) -> u32 {
        first_weekday
            .saturating_add(days_in_month)
            .saturating_add(6)
            .saturating_div(7)
    }

    fn cell(&self, day: u32) -> Rect {
        let index = self.first_weekday.saturating_add(day.saturating_sub(1));
        let col = index % 7;
        let row = index / 7;
        Rect::new(
            self.content_x + (col as f32) * CALENDAR_CELL,
            self.grid_top + (row as f32) * CALENDAR_CELL,
            CALENDAR_CELL,
            CALENDAR_CELL,
        )
    }
}

/// A row of one of the three list-shaped popups.
struct MenuItemLayout {
    entry: MenuEntry,
    rect: Rect,
    label: String,
}

/// Geometry of the app menu, the context menu and the power menu, which
/// differ only in their header and their rows.
struct MenuLayout {
    frame: Rect,
    content_x: f32,
    content_width: f32,
    header: Option<(String, f32)>,
    items: Vec<MenuItemLayout>,
}

/// What a click did: whether the tray took it, and what it wants done.
///
/// The two are genuinely separate — a click that lands on the padding of an
/// open popup is consumed (it must not fall through to the desktop, and it
/// must not dismiss the popup) and asks for nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClickOutcome {
    pub consumed: bool,
    pub action: TrayAction,
}

/// The layout of whichever popup is open.
enum PopupLayout {
    QuickSettings(QuickSettingsLayout),
    Volume(VolumeLayout),
    Network(NetworkLayout),
    Calendar(CalendarLayout),
    Menu(MenuLayout),
}

impl PopupLayout {
    fn frame(&self) -> Rect {
        match self {
            PopupLayout::QuickSettings(l) => l.frame,
            PopupLayout::Volume(l) => l.frame,
            PopupLayout::Network(l) => l.frame,
            PopupLayout::Calendar(l) => l.frame,
            PopupLayout::Menu(l) => l.frame,
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
    /// Size of the window the tray is anchored inside, or `(0.0, 0.0)` when it
    /// has not been told one — in which case `tray_x`/`tray_y` are left exactly
    /// where the caller put them.
    viewport: (f32, f32),
    /// Months the calendar popup is displaced from the month the clock is in.
    /// The header arrows move this; opening the popup afresh resets it, so the
    /// calendar always opens on today.
    calendar_offset: i32,
    /// The slider the pointer is currently dragging, if any.
    ///
    /// guitk's mouse events carry no held-button state, so a drag is a `Press`
    /// that arms this, `Move`s that follow it, and a `Release` that disarms it.
    /// Without it a slider could only be jumped to, never dragged.
    dragging: Option<Slider>,
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
            viewport: (0.0, 0.0),
            calendar_offset: 0,
            dragging: None,
        }
    }

    /// Anchor the tray to the bottom-right of a window of this size.
    ///
    /// `render` is handed the size the window actually is, which is not
    /// necessarily the size the app asked for, so this is called from there
    /// every frame rather than once at startup.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.reposition();
    }

    /// Put the tray bar back in its corner after its size or the window's
    /// changed. A tray that grew an icon has to move left by a cell, or the
    /// new icon hangs off the edge of the screen.
    fn reposition(&mut self) {
        let (width, height) = self.viewport;
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        self.tray_x = width - self.tray_width - TRAY_INSET;
        self.tray_y = height - TRAY_HEIGHT - TRAY_INSET;
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

    /// Age the battery estimate by `elapsed_ms`.
    ///
    /// This used to advance the clock too, by adding elapsed seconds onto
    /// `datetime` — and it carried seconds into minutes and minutes into
    /// hours, but **never hours into days**. At midnight the hour wrapped to
    /// zero and the date stayed where `DateTime::default` put it, so the
    /// calendar popup was pinned to 2026-05-17 for the life of the process.
    /// The clock now has exactly one writer, [`Self::set_time_from_utc`]; a
    /// clock with two writers is a clock that disagrees with itself.
    pub fn tick(&mut self, elapsed_ms: u64) {
        let total_seconds = elapsed_ms / 1000;
        if total_seconds == 0 {
            return;
        }
        if let Some(ref mut mins) = self.battery.estimated_minutes {
            *mins = mins.saturating_sub(u32::try_from(total_seconds / 60).unwrap_or(u32::MAX));
        }
    }

    /// Set the clock from a Unix instant, read in the tray's display zone.
    ///
    /// UTC today, explicitly rather than by accident: the taskbar clock once
    /// shipped `secs % 86_400` and read five hours out for everyone east of
    /// Greenwich. Going through `tzrules` means the day this rolls over on is
    /// the zone's day, and `rg 'Tz::utc' apps/ gui/` finds every surface that
    /// still has to be told about a real zone when we have one.
    pub fn set_time_from_utc(&mut self, utc_secs: i64) {
        let zone = tzrules::Tz::utc();
        let local = utc_secs.saturating_add(i64::from(zone.lookup(utc_secs).gmtoff));
        let civil = date::Date::from_unix_utc(local);
        let (year, month, day) = civil.ymd();
        // `rem_euclid`, not `%`: a pre-1970 instant with `%` gives a negative
        // remainder, which is not a time of day at all.
        let secs_into_day = local.rem_euclid(86_400);
        self.datetime = DateTime {
            year: u16::try_from(year).unwrap_or(1970),
            month: u8::try_from(month).unwrap_or(1),
            day: u8::try_from(day).unwrap_or(1),
            // All three are in range by construction, so the fallbacks are
            // unreachable; they are written rather than unwrapped because an
            // unreachable panic in a tray is still a panic in a tray.
            hour: u8::try_from(secs_into_day / 3600).unwrap_or(0),
            minute: u8::try_from(secs_into_day.rem_euclid(3600) / 60).unwrap_or(0),
            second: u8::try_from(secs_into_day.rem_euclid(60)).unwrap_or(0),
            weekday: u8::try_from(civil.weekday().index()).unwrap_or(0),
        };
        let time = self.datetime.time_str();
        if let Some(clock_icon) = self.icons.iter_mut().find(|i| i.id == ICON_ID_CLOCK) {
            clock_icon.tooltip = time;
        }
    }

    /// Read the host clock into [`Self::set_time_from_utc`].
    ///
    /// Split from the pure half so the pure half can be asserted against a
    /// known instant; this half is the one that cannot be.
    pub fn refresh_clock(&mut self) {
        if let Ok(since_epoch) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        {
            self.set_time_from_utc(i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX));
        }
    }

    /// The tray bar's own rectangle.
    fn tray_rect(&self) -> Rect {
        Rect::new(self.tray_x, self.tray_y, self.tray_width, TRAY_HEIGHT)
    }

    /// The IDs of the icons that are actually drawn, in draw order.
    fn visible_icon_ids(&self) -> Vec<TrayIconId> {
        self.icons
            .iter()
            .filter(|i| i.visible)
            .map(|i| i.id)
            .collect()
    }

    /// The icon under a point, or `None` if the point misses the icons.
    fn icon_at(&self, x: f32, y: f32) -> Option<TrayIconId> {
        if !self.tray_rect().contains(x, y) {
            return None;
        }
        let index = ((x - self.tray_x) / ICON_CELL_SIZE) as usize;
        self.visible_icon_ids().get(index).copied()
    }

    /// Handle a click. Returns whether the tray consumed it, and what — if
    /// anything — it wants the desktop to do about it.
    pub fn handle_click_at(&mut self, x: f32, y: f32, button: MouseButton) -> ClickOutcome {
        // The popup is drawn last, so it is on top, so it is asked first.
        if let Some(layout) = self.popup_layout()
            && layout.frame().contains(x, y)
        {
            let action = if button == MouseButton::Left {
                self.click_in_popup(&layout, x, y)
            } else {
                TrayAction::None
            };
            return ClickOutcome {
                consumed: true,
                action,
            };
        }

        if self.tray_rect().contains(x, y) {
            if let Some(icon_id) = self.icon_at(x, y) {
                match button {
                    MouseButton::Left => self.handle_icon_left_click(icon_id),
                    MouseButton::Right => self.handle_icon_right_click(icon_id),
                    _ => {}
                }
            } else if button == MouseButton::Left {
                // Click on empty tray area -> quick settings
                self.toggle_popup(PopupType::QuickSettings);
            }
            return ClickOutcome {
                consumed: true,
                action: TrayAction::None,
            };
        }

        if self.active_popup != PopupType::None {
            self.active_popup = PopupType::None;
            return ClickOutcome {
                consumed: true,
                action: TrayAction::None,
            };
        }

        ClickOutcome {
            consumed: false,
            action: TrayAction::None,
        }
    }

    /// Handle a click event. Returns true if the event was consumed.
    pub fn handle_click(&mut self, x: f32, y: f32, button: MouseButton) -> bool {
        self.handle_click_at(x, y, button).consumed
    }

    /// Handle a double-click on a tray icon (open associated app).
    pub fn handle_double_click(&mut self, x: f32, y: f32) -> Option<TrayIconId> {
        self.icon_at(x, y)
    }

    /// Route a click that landed inside the open popup to whatever it landed
    /// on. Anything that misses every control — the header text, the padding,
    /// the separator — is swallowed, because a click *inside* the popup the
    /// user is aiming at must not dismiss it.
    fn click_in_popup(&mut self, layout: &PopupLayout, x: f32, y: f32) -> TrayAction {
        match layout {
            PopupLayout::QuickSettings(l) => {
                if let Some(row) = l.rows.iter().find(|row| row.rect.contains(x, y)) {
                    let toggle = row.toggle;
                    self.set_toggle(toggle, !self.toggle_enabled(toggle));
                } else if l.brightness.contains(x, y) {
                    self.begin_drag(Slider::Brightness, l.brightness, x);
                } else if l.volume.contains(x, y) {
                    self.begin_drag(Slider::MasterVolume, l.volume, x);
                }
                TrayAction::None
            }
            PopupLayout::Volume(l) => {
                if l.master_label.contains(x, y) {
                    self.volume.muted = !self.volume.muted;
                } else if l.master_slider.contains(x, y) {
                    self.begin_drag(Slider::MasterVolume, l.master_slider, x);
                } else if let Some(app) = l
                    .apps
                    .iter()
                    .find(|app| app.label.contains(x, y) || app.slider.contains(x, y))
                {
                    if app.label.contains(x, y) {
                        if let Some(entry) = self.volume.app_volumes.get_mut(app.index) {
                            entry.muted = !entry.muted;
                        }
                    } else {
                        self.begin_drag(Slider::AppVolume(app.index), app.slider, x);
                    }
                }
                TrayAction::None
            }
            PopupLayout::Network(l) => {
                if l.settings.contains(x, y) {
                    self.active_popup = PopupType::None;
                    return TrayAction::OpenNetworkSettings;
                }
                TrayAction::None
            }
            PopupLayout::Calendar(l) => {
                if l.prev.contains(x, y) {
                    self.calendar_offset = self.calendar_offset.saturating_sub(1);
                } else if l.next.contains(x, y) {
                    self.calendar_offset = self.calendar_offset.saturating_add(1);
                } else if l.title.contains(x, y) {
                    // The standard "back to today" affordance. Without it a
                    // user who arrowed six months forward has to arrow six
                    // months back.
                    self.calendar_offset = 0;
                }
                TrayAction::None
            }
            PopupLayout::Menu(l) => {
                let Some(item) = l.items.iter().find(|item| item.rect.contains(x, y)) else {
                    return TrayAction::None;
                };
                let entry = item.entry;
                self.active_popup = PopupType::None;
                match entry {
                    MenuEntry::OpenApp(id) => TrayAction::OpenApp(id),
                    MenuEntry::ShowIcon(id) => {
                        self.set_icon_visible(id, true);
                        TrayAction::None
                    }
                    MenuEntry::HideIcon(id) => {
                        self.set_icon_visible(id, false);
                        TrayAction::None
                    }
                    MenuEntry::RemoveIcon(id) => {
                        self.remove_icon(id);
                        TrayAction::None
                    }
                    MenuEntry::Power(action) => TrayAction::Power(action),
                }
            }
        }
    }

    /// Arm a drag and move the slider to where it was grabbed, so the first
    /// press already takes effect rather than waiting for the pointer to move.
    fn begin_drag(&mut self, slider: Slider, track: Rect, x: f32) {
        self.dragging = Some(slider);
        self.set_slider(slider, Self::value_at(track, x));
    }

    fn value_at(track: Rect, x: f32) -> u8 {
        let fraction = track.fraction_across(x);
        // `0.0..=1.0 * 100.0` rounds into `0..=100`, which fits a `u8`.
        u8::try_from((fraction * 100.0).round() as i32).unwrap_or(0)
    }

    /// Whether a quick-settings switch is on.
    #[must_use]
    pub fn toggle_enabled(&self, toggle: Toggle) -> bool {
        match toggle {
            Toggle::Wifi => self.quick_settings.wifi_enabled,
            Toggle::Bluetooth => self.quick_settings.bluetooth_enabled,
            Toggle::DoNotDisturb => self.quick_settings.do_not_disturb,
            Toggle::NightLight => self.quick_settings.night_light,
            Toggle::BatterySaver => self.quick_settings.battery_saver,
            Toggle::AirplaneMode => self.quick_settings.airplane_mode,
        }
    }

    /// Set a quick-settings switch.
    ///
    /// Airplane mode is not just another switch: turning it on turns the
    /// radios off, which is the whole point of it, and a tray that drew
    /// "Airplane Mode: on" above "WiFi: on" would be lying about the machine.
    pub fn set_toggle(&mut self, toggle: Toggle, enabled: bool) {
        match toggle {
            Toggle::Wifi => self.quick_settings.wifi_enabled = enabled,
            Toggle::Bluetooth => self.quick_settings.bluetooth_enabled = enabled,
            Toggle::DoNotDisturb => self.quick_settings.do_not_disturb = enabled,
            Toggle::NightLight => self.quick_settings.night_light = enabled,
            Toggle::BatterySaver => self.quick_settings.battery_saver = enabled,
            Toggle::AirplaneMode => {
                self.quick_settings.airplane_mode = enabled;
                if enabled {
                    self.quick_settings.wifi_enabled = false;
                    self.quick_settings.bluetooth_enabled = false;
                }
            }
        }
        if toggle == Toggle::Wifi && enabled {
            self.quick_settings.airplane_mode = false;
        }
        if !self.quick_settings.wifi_enabled {
            self.network.connected = false;
        }
    }

    /// The current value of a slider, `0..=100`.
    #[must_use]
    pub fn slider_value(&self, slider: Slider) -> u8 {
        match slider {
            Slider::Brightness => self.quick_settings.brightness,
            Slider::MasterVolume => self.volume.master_volume,
            Slider::AppVolume(index) => self
                .volume
                .app_volumes
                .get(index)
                .map_or(0, |entry| entry.volume),
        }
    }

    /// Set a slider's value, clamped to `0..=100`.
    pub fn set_slider(&mut self, slider: Slider, value: u8) {
        let value = value.min(100);
        match slider {
            Slider::Brightness => self.quick_settings.brightness = value,
            Slider::MasterVolume => {
                self.volume.master_volume = value;
                // Dragging the master slider off zero is the most direct way a
                // user can say "I want to hear this"; leaving it muted would
                // move the fill and change nothing audible.
                if value > 0 {
                    self.volume.muted = false;
                }
            }
            Slider::AppVolume(index) => {
                if let Some(entry) = self.volume.app_volumes.get_mut(index) {
                    entry.volume = value;
                }
            }
        }
    }

    /// Show or hide a tray icon, re-laying out the bar around it.
    pub fn set_icon_visible(&mut self, id: TrayIconId, visible: bool) {
        if let Some(icon) = self.icons.iter_mut().find(|i| i.id == id) {
            icon.visible = visible;
        }
        self.recalculate_width();
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

        // Render active popup, from the same layout the hit-test reads.
        if let Some(layout) = self.popup_layout() {
            match &layout {
                PopupLayout::QuickSettings(l) => self.render_quick_settings(&mut tree, l),
                PopupLayout::Volume(l) => self.render_volume_popup(&mut tree, l),
                PopupLayout::Network(l) => self.render_network_popup(&mut tree, l),
                PopupLayout::Calendar(l) => self.render_calendar_popup(&mut tree, l),
                PopupLayout::Menu(l) => Self::render_menu(&mut tree, l),
            }
        }

        tree
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    fn recalculate_width(&mut self) {
        let count = self.icons.iter().filter(|i| i.visible).count() as f32;
        self.tray_width = count * ICON_CELL_SIZE;
        self.reposition();
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

    // ========================================================================
    // Layout — the single source of every interactive position
    // ========================================================================

    /// The layout of the open popup, or `None` when none is open.
    fn popup_layout(&self) -> Option<PopupLayout> {
        match self.active_popup {
            PopupType::None => None,
            PopupType::QuickSettings => {
                Some(PopupLayout::QuickSettings(self.quick_settings_layout()))
            }
            PopupType::Volume => Some(PopupLayout::Volume(self.volume_layout())),
            PopupType::Network => Some(PopupLayout::Network(self.network_layout())),
            PopupType::Calendar => Some(PopupLayout::Calendar(self.calendar_layout())),
            PopupType::AppMenu(id) => Some(PopupLayout::Menu(self.app_menu_layout(id))),
            PopupType::ContextMenu(id) => Some(PopupLayout::Menu(self.context_menu_layout(id))),
            PopupType::PowerMenu => Some(PopupLayout::Menu(self.power_menu_layout())),
        }
    }

    /// Height of a toggle row, which is taller when it carries a subtitle.
    fn toggle_row_height(subtitle: &str) -> f32 {
        if subtitle.is_empty() {
            POPUP_FONT_SIZE + ITEM_SPACING * 2.0
        } else {
            POPUP_FONT_SIZE * 2.0 + ITEM_SPACING * 2.0
        }
    }

    fn quick_settings_layout(&self) -> QuickSettingsLayout {
        let popup_width = 300.0;
        let content_width = popup_width - POPUP_PADDING * 2.0;

        let specs: [(Toggle, &'static str, String); 6] = [
            (
                Toggle::Wifi,
                "WiFi",
                self.quick_settings.wifi_network_name.clone(),
            ),
            (
                Toggle::Bluetooth,
                "Bluetooth",
                String::from(if self.quick_settings.bluetooth_enabled {
                    "On"
                } else {
                    "Off"
                }),
            ),
            (Toggle::DoNotDisturb, "Do Not Disturb", String::new()),
            (Toggle::NightLight, "Night Light", String::new()),
            (Toggle::BatterySaver, "Battery Saver", String::new()),
            (Toggle::AirplaneMode, "Airplane Mode", String::new()),
        ];

        // Measured relative to the top of the content box, because the
        // popup's own Y depends on how tall it turns out to be: it hangs from
        // the tray bar, upwards. The height used to be a hardcoded 340, which
        // is 35px more than the content needs; the network popup's hardcoded
        // 160 was 6px *less* than its content needs, and clipped.
        let mut dy = 0.0_f32;
        let header_dy = dy;
        dy += HEADER_FONT_SIZE + ITEM_SPACING * 2.0;

        let mut row_dy: Vec<f32> = Vec::with_capacity(specs.len());
        for (_, _, subtitle) in &specs {
            row_dy.push(dy);
            dy += Self::toggle_row_height(subtitle);
        }

        dy += ITEM_SPACING;
        let brightness_label_dy = dy;
        dy += POPUP_FONT_SIZE + 4.0;
        let brightness_dy = dy;
        dy += SLIDER_THUMB_RADIUS * 2.0 + ITEM_SPACING;
        let volume_label_dy = dy;
        dy += POPUP_FONT_SIZE + 4.0;
        let volume_dy = dy;
        dy += SLIDER_THUMB_RADIUS * 2.0;

        let popup_height = dy + POPUP_PADDING * 2.0;
        let frame = Rect::new(
            self.tray_x + self.tray_width - popup_width,
            self.tray_y - popup_height - POPUP_GAP,
            popup_width,
            popup_height,
        );
        let content_x = frame.x + POPUP_PADDING;
        let content_y = frame.y + POPUP_PADDING;

        let rows = specs
            .into_iter()
            .zip(row_dy)
            .map(|((toggle, label, subtitle), row_top)| {
                let height = Self::toggle_row_height(&subtitle);
                ToggleRowLayout {
                    toggle,
                    rect: Rect::new(content_x, content_y + row_top, content_width, height),
                    label,
                    subtitle,
                    enabled: self.toggle_enabled(toggle),
                }
            })
            .collect();

        QuickSettingsLayout {
            frame,
            content_x,
            content_width,
            header_y: content_y + header_dy,
            rows,
            brightness_label_y: content_y + brightness_label_dy,
            brightness: Rect::new(
                content_x,
                content_y + brightness_dy,
                content_width,
                SLIDER_THUMB_RADIUS * 2.0,
            ),
            volume_label_y: content_y + volume_label_dy,
            volume: Rect::new(
                content_x,
                content_y + volume_dy,
                content_width,
                SLIDER_THUMB_RADIUS * 2.0,
            ),
        }
    }

    fn volume_layout(&self) -> VolumeLayout {
        let popup_width = 280.0;
        let content_width = popup_width - POPUP_PADDING * 2.0;
        let label_height = POPUP_FONT_SIZE + 4.0;
        let slider_height = SLIDER_THUMB_RADIUS * 2.0;

        let mut dy = 0.0_f32;
        let header_dy = dy;
        dy += HEADER_FONT_SIZE + ITEM_SPACING;
        let device_dy = dy;
        dy += POPUP_FONT_SIZE + ITEM_SPACING;
        let master_label_dy = dy;
        dy += label_height;
        let master_slider_dy = dy;
        dy += slider_height + ITEM_SPACING * 2.0;
        let separator_dy = dy;
        dy += ITEM_SPACING * 2.0;

        let mut app_dy: Vec<(f32, f32)> = Vec::with_capacity(self.volume.app_volumes.len());
        for _ in &self.volume.app_volumes {
            let label_top = dy;
            dy += label_height;
            let slider_top = dy;
            dy += slider_height + ITEM_SPACING;
            app_dy.push((label_top, slider_top));
        }

        let popup_height = dy + POPUP_PADDING * 2.0;
        let frame = Rect::new(
            self.popup_x_for_icon(ICON_ID_VOLUME),
            self.tray_y - popup_height - POPUP_GAP,
            popup_width,
            popup_height,
        );
        let content_x = frame.x + POPUP_PADDING;
        let content_y = frame.y + POPUP_PADDING;

        VolumeLayout {
            frame,
            content_x,
            content_width,
            header_y: content_y + header_dy,
            device_y: content_y + device_dy,
            master_label: Rect::new(
                content_x,
                content_y + master_label_dy,
                content_width,
                label_height,
            ),
            master_slider: Rect::new(
                content_x,
                content_y + master_slider_dy,
                content_width,
                slider_height,
            ),
            separator_y: content_y + separator_dy,
            apps: app_dy
                .into_iter()
                .enumerate()
                .map(|(index, (label_top, slider_top))| AppVolumeLayout {
                    index,
                    label: Rect::new(
                        content_x,
                        content_y + label_top,
                        content_width,
                        label_height,
                    ),
                    slider: Rect::new(
                        content_x,
                        content_y + slider_top,
                        content_width,
                        slider_height,
                    ),
                })
                .collect(),
        }
    }

    fn network_layout(&self) -> NetworkLayout {
        let popup_width = 260.0;
        let content_width = popup_width - POPUP_PADDING * 2.0;

        let mut dy = 0.0_f32;
        let header_dy = dy;
        dy += HEADER_FONT_SIZE + ITEM_SPACING * 2.0;
        let status_dy = dy;
        dy += POPUP_FONT_SIZE + ITEM_SPACING;
        let ssid_dy = dy;
        dy += POPUP_FONT_SIZE + ITEM_SPACING;
        let signal_label_dy = dy;
        dy += POPUP_FONT_SIZE + 4.0;
        let signal_bar_dy = dy;
        dy += SLIDER_THUMB_RADIUS * 2.0 + ITEM_SPACING;
        let ip_dy = dy;
        dy += POPUP_FONT_SIZE + ITEM_SPACING * 2.0;
        let settings_dy = dy;
        dy += POPUP_FONT_SIZE + 4.0;

        let popup_height = dy + POPUP_PADDING * 2.0;
        let frame = Rect::new(
            self.popup_x_for_icon(ICON_ID_NETWORK),
            self.tray_y - popup_height - POPUP_GAP,
            popup_width,
            popup_height,
        );
        let content_x = frame.x + POPUP_PADDING;
        let content_y = frame.y + POPUP_PADDING;

        NetworkLayout {
            frame,
            content_x,
            content_width,
            header_y: content_y + header_dy,
            status_y: content_y + status_dy,
            ssid_y: content_y + ssid_dy,
            signal_label_y: content_y + signal_label_dy,
            signal_bar: Rect::new(
                content_x,
                content_y + signal_bar_dy,
                content_width,
                SLIDER_THUMB_RADIUS * 2.0,
            ),
            ip_y: content_y + ip_dy,
            settings: Rect::new(
                content_x,
                content_y + settings_dy,
                content_width,
                POPUP_FONT_SIZE + 4.0,
            ),
        }
    }

    /// The month the calendar popup draws: the clock's month moved
    /// `calendar_offset` months.
    ///
    /// It is returned as a `DateTime` rather than a `date::Date` so the grid
    /// is laid out by `DateTime::days_in_month` and
    /// `DateTime::first_weekday_of_month` — the two methods the tests below
    /// cover. A tested method that production does not call is the shape every
    /// frozen clock in this tree has had.
    fn displayed_month(&self) -> DateTime {
        let first = date::Date::from_ymd(
            i32::from(self.datetime.year),
            u32::from(self.datetime.month),
            1,
        )
        .add_months(self.calendar_offset);
        let (year, month, day) = first.ymd();
        DateTime {
            year: u16::try_from(year).unwrap_or(self.datetime.year),
            month: u8::try_from(month).unwrap_or(1),
            day: u8::try_from(day).unwrap_or(1),
            hour: 0,
            minute: 0,
            second: 0,
            weekday: u8::try_from(first.weekday().index()).unwrap_or(0),
        }
    }

    fn calendar_layout(&self) -> CalendarLayout {
        let popup_width = 7.0 * CALENDAR_CELL + POPUP_PADDING * 2.0;
        let content_width = popup_width - POPUP_PADDING * 2.0;

        let shown = self.displayed_month();
        let year = i32::from(shown.year);
        let month = u32::from(shown.month);
        let days_in_month = u32::from(shown.days_in_month());
        let first_weekday = u32::from(shown.first_weekday_of_month());
        let rows = CalendarLayout::rows_needed(first_weekday, days_in_month);

        let header_height = HEADER_FONT_SIZE + 4.0;
        let mut dy = 0.0_f32;
        let header_dy = dy;
        dy += HEADER_FONT_SIZE + ITEM_SPACING * 2.0;
        let weekday_header_dy = dy;
        dy += POPUP_FONT_SIZE + ITEM_SPACING;
        let grid_dy = dy;
        dy += (rows as f32) * CALENDAR_CELL;
        dy += ITEM_SPACING * 2.0;
        let events_dy = dy;
        dy += POPUP_FONT_SIZE + 4.0;

        let popup_height = dy + POPUP_PADDING * 2.0;
        let frame = Rect::new(
            self.popup_x_for_icon(ICON_ID_CLOCK),
            self.tray_y - popup_height - POPUP_GAP,
            popup_width,
            popup_height,
        );
        let content_x = frame.x + POPUP_PADDING;
        let content_y = frame.y + POPUP_PADDING;

        let arrow_width = POPUP_FONT_SIZE + 8.0;
        let today = (year == i32::from(self.datetime.year)
            && month == u32::from(self.datetime.month))
        .then_some(u32::from(self.datetime.day));

        CalendarLayout {
            frame,
            content_x,
            content_width,
            header_y: content_y + header_dy,
            prev: Rect::new(content_x, content_y + header_dy, arrow_width, header_height),
            next: Rect::new(
                content_x + content_width - arrow_width,
                content_y + header_dy,
                arrow_width,
                header_height,
            ),
            title: Rect::new(
                content_x + arrow_width,
                content_y + header_dy,
                content_width - arrow_width * 2.0,
                header_height,
            ),
            year,
            month,
            first_weekday,
            days_in_month,
            weekday_header_y: content_y + weekday_header_dy,
            grid_top: content_y + grid_dy,
            today,
            events_y: content_y + events_dy,
        }
    }

    /// Lay a vertical list of rows out under an optional header.
    fn menu_layout(
        &self,
        anchor_icon: TrayIconId,
        popup_width: f32,
        header: Option<String>,
        entries: Vec<(MenuEntry, String)>,
    ) -> MenuLayout {
        let content_width = popup_width - POPUP_PADDING * 2.0;
        let item_height = POPUP_FONT_SIZE + ITEM_SPACING;

        let mut dy = 0.0_f32;
        let header_dy = header.is_some().then(|| {
            let at = dy;
            dy += HEADER_FONT_SIZE + ITEM_SPACING;
            at
        });
        let first_item_dy = dy;
        dy += (entries.len() as f32) * item_height;

        let popup_height = dy + POPUP_PADDING * 2.0;
        let frame = Rect::new(
            self.popup_x_for_icon(anchor_icon),
            self.tray_y - popup_height - POPUP_GAP,
            popup_width,
            popup_height,
        );
        let content_x = frame.x + POPUP_PADDING;
        let content_y = frame.y + POPUP_PADDING;

        MenuLayout {
            frame,
            content_x,
            content_width,
            header: header
                .zip(header_dy)
                .map(|(text, at)| (text, content_y + at)),
            items: entries
                .into_iter()
                .enumerate()
                .map(|(i, (entry, label))| MenuItemLayout {
                    entry,
                    rect: Rect::new(
                        content_x,
                        content_y + first_item_dy + (i as f32) * item_height,
                        content_width,
                        item_height,
                    ),
                    label,
                })
                .collect(),
        }
    }

    fn app_menu_layout(&self, icon_id: TrayIconId) -> MenuLayout {
        let app_name = self
            .icons
            .iter()
            .find(|i| i.id == icon_id)
            .map_or("Unknown", |i| i.app_name.as_str());
        self.menu_layout(
            icon_id,
            180.0,
            Some(String::from(app_name)),
            vec![(MenuEntry::OpenApp(icon_id), String::from("Open"))],
        )
    }

    fn context_menu_layout(&self, icon_id: TrayIconId) -> MenuLayout {
        self.menu_layout(
            icon_id,
            160.0,
            None,
            vec![
                (MenuEntry::ShowIcon(icon_id), String::from("Show")),
                (MenuEntry::HideIcon(icon_id), String::from("Hide")),
                (
                    MenuEntry::RemoveIcon(icon_id),
                    String::from("Remove from tray"),
                ),
            ],
        )
    }

    fn power_menu_layout(&self) -> MenuLayout {
        self.menu_layout(
            ICON_ID_POWER,
            180.0,
            None,
            PowerAction::ALL
                .iter()
                .map(|(action, icon, label)| {
                    (MenuEntry::Power(*action), format!("{icon}  {label}"))
                })
                .collect(),
        )
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
            overflow: TextOverflow::Ellipsis,
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
    // Popup rendering
    //
    // Every position below comes out of the layout structs above. Nothing here
    // computes a coordinate of its own, which is what makes it impossible for
    // a control to be drawn somewhere the hit-test is not looking.
    // ========================================================================

    /// Push a single run of text. Every popup draws a dozen of these and the
    /// struct literal is nine lines each; the noise was hiding the geometry.
    fn push_text(
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        text: String,
        color: Color,
        font_size: f32,
        max_width: Option<f32>,
    ) {
        tree.push(RenderCommand::Text {
            x,
            y,
            text,
            color,
            font_size,
            font_weight: FontWeightHint::Regular,
            max_width,
            overflow: if max_width.is_some() {
                TextOverflow::Ellipsis
            } else {
                TextOverflow::Clip
            },
        });
    }

    /// Push a bold header run.
    fn push_header(tree: &mut RenderTree, x: f32, y: f32, text: String, max_width: f32) {
        tree.push(RenderCommand::Text {
            x,
            y,
            text,
            color: palette::TEXT,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_width),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_quick_settings(&self, tree: &mut RenderTree, l: &QuickSettingsLayout) {
        Self::render_popup_background(tree, l.frame);
        Self::push_header(
            tree,
            l.content_x,
            l.header_y,
            String::from("Quick Settings"),
            l.content_width,
        );

        for row in &l.rows {
            Self::render_toggle_row(tree, row);
        }

        Self::push_text(
            tree,
            l.content_x,
            l.brightness_label_y,
            format!("Brightness: {}%", self.quick_settings.brightness),
            palette::SUBTEXT0,
            POPUP_FONT_SIZE,
            Some(l.content_width),
        );
        Self::render_slider(
            tree,
            l.brightness,
            self.quick_settings.brightness,
            palette::YELLOW,
        );

        Self::push_text(
            tree,
            l.content_x,
            l.volume_label_y,
            format!("Volume: {}%", self.volume.master_volume),
            palette::SUBTEXT0,
            POPUP_FONT_SIZE,
            Some(l.content_width),
        );
        Self::render_slider(
            tree,
            l.volume,
            self.volume.master_volume,
            if self.volume.muted {
                palette::SURFACE2
            } else {
                palette::BLUE
            },
        );
    }

    fn render_volume_popup(&self, tree: &mut RenderTree, l: &VolumeLayout) {
        Self::render_popup_background(tree, l.frame);
        Self::push_header(
            tree,
            l.content_x,
            l.header_y,
            String::from("Volume"),
            l.content_width,
        );

        Self::push_text(
            tree,
            l.content_x,
            l.device_y,
            self.volume.output_device.clone(),
            palette::SUBTEXT0,
            POPUP_FONT_SIZE - 1.0,
            Some(l.content_width),
        );

        let mute_label = if self.volume.muted { " (Muted)" } else { "" };
        Self::push_text(
            tree,
            l.master_label.x,
            l.master_label.y,
            format!("Master: {}%{}", self.volume.master_volume, mute_label),
            if self.volume.muted {
                palette::OVERLAY0
            } else {
                palette::TEXT
            },
            POPUP_FONT_SIZE,
            Some(l.content_width),
        );
        Self::render_slider(
            tree,
            l.master_slider,
            self.volume.master_volume,
            if self.volume.muted {
                palette::SURFACE2
            } else {
                palette::BLUE
            },
        );

        tree.push(RenderCommand::FillRect {
            x: l.content_x,
            y: l.separator_y,
            width: l.content_width,
            height: 1.0,
            color: palette::SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });

        for app in &l.apps {
            let Some(entry) = self.volume.app_volumes.get(app.index) else {
                continue;
            };
            let mute_marker = if entry.muted { " \u{1F507}" } else { "" };
            Self::push_text(
                tree,
                app.label.x,
                app.label.y,
                format!("{}: {}%{}", entry.app_name, entry.volume, mute_marker),
                if entry.muted {
                    palette::OVERLAY0
                } else {
                    palette::SUBTEXT1
                },
                POPUP_FONT_SIZE,
                Some(l.content_width),
            );
            Self::render_slider(
                tree,
                app.slider,
                entry.volume,
                if entry.muted {
                    palette::SURFACE2
                } else {
                    palette::TEAL
                },
            );
        }
    }

    fn render_network_popup(&self, tree: &mut RenderTree, l: &NetworkLayout) {
        Self::render_popup_background(tree, l.frame);
        Self::push_header(
            tree,
            l.content_x,
            l.header_y,
            String::from("Network"),
            l.content_width,
        );

        let (status_text, status_color) = if self.network.connected {
            ("Connected", palette::GREEN)
        } else {
            ("Disconnected", palette::RED)
        };
        Self::push_text(
            tree,
            l.content_x,
            l.status_y,
            format!("Status: {status_text}"),
            status_color,
            POPUP_FONT_SIZE,
            Some(l.content_width),
        );
        Self::push_text(
            tree,
            l.content_x,
            l.ssid_y,
            format!("Network: {}", self.network.ssid),
            palette::SUBTEXT1,
            POPUP_FONT_SIZE,
            Some(l.content_width),
        );
        Self::push_text(
            tree,
            l.content_x,
            l.signal_label_y,
            format!("Signal: {}%", self.network.signal_strength),
            palette::SUBTEXT0,
            POPUP_FONT_SIZE,
            Some(l.content_width),
        );
        // A read-out drawn in a slider's clothing. It is deliberately not in
        // `Slider`, so no click can move it.
        Self::render_slider(
            tree,
            l.signal_bar,
            self.network.signal_strength,
            palette::GREEN,
        );
        Self::push_text(
            tree,
            l.content_x,
            l.ip_y,
            format!("IP: {}", self.network.ip_address),
            palette::SUBTEXT0,
            POPUP_FONT_SIZE,
            Some(l.content_width),
        );
        Self::push_text(
            tree,
            l.settings.x,
            l.settings.y,
            String::from("Network Settings..."),
            palette::BLUE,
            POPUP_FONT_SIZE,
            Some(l.content_width),
        );
    }

    fn render_calendar_popup(&self, tree: &mut RenderTree, l: &CalendarLayout) {
        Self::render_popup_background(tree, l.frame);

        Self::push_text(
            tree,
            l.prev.x,
            l.header_y,
            String::from("\u{25C0}"),
            palette::SUBTEXT0,
            POPUP_FONT_SIZE,
            None,
        );
        Self::push_header(
            tree,
            l.title.x,
            l.header_y,
            format!("{} {}", date::month_name(l.month), l.year),
            l.title.w,
        );
        Self::push_text(
            tree,
            l.next.x,
            l.header_y,
            String::from("\u{25B6}"),
            palette::SUBTEXT0,
            POPUP_FONT_SIZE,
            None,
        );

        for (i, header) in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
            .into_iter()
            .enumerate()
        {
            Self::push_text(
                tree,
                l.content_x + (i as f32) * CALENDAR_CELL + CALENDAR_CELL / 2.0 - 6.0,
                l.weekday_header_y,
                String::from(header),
                palette::OVERLAY0,
                POPUP_FONT_SIZE - 1.0,
                None,
            );
        }

        for day in 1..=l.days_in_month {
            let cell = l.cell(day);
            let is_today = l.today == Some(day);
            if is_today {
                tree.push(RenderCommand::FillRect {
                    x: cell.x + 2.0,
                    y: cell.y + 2.0,
                    width: CALENDAR_CELL - 4.0,
                    height: CALENDAR_CELL - 4.0,
                    color: palette::BLUE,
                    corner_radii: CornerRadii::all(CALENDAR_CELL / 2.0 - 2.0),
                });
            }
            tree.push(RenderCommand::Text {
                x: cell.x + CALENDAR_CELL / 2.0 - 5.0,
                y: cell.y + 8.0,
                text: format!("{day}"),
                color: if is_today {
                    palette::CRUST
                } else {
                    palette::TEXT
                },
                font_size: POPUP_FONT_SIZE,
                font_weight: if is_today {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        tree.push(RenderCommand::FillRect {
            x: l.content_x,
            y: l.events_y - ITEM_SPACING,
            width: l.content_width,
            height: 1.0,
            color: palette::SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });
        Self::push_text(
            tree,
            l.content_x,
            l.events_y,
            String::from("No upcoming events"),
            palette::OVERLAY0,
            POPUP_FONT_SIZE - 1.0,
            Some(l.content_width),
        );
    }

    fn render_menu(tree: &mut RenderTree, l: &MenuLayout) {
        Self::render_popup_background(tree, l.frame);
        if let Some((text, y)) = &l.header {
            Self::push_header(tree, l.content_x, *y, text.clone(), l.content_width);
        }
        for item in &l.items {
            Self::push_text(
                tree,
                item.rect.x,
                item.rect.y,
                item.label.clone(),
                palette::TEXT,
                POPUP_FONT_SIZE,
                Some(l.content_width),
            );
        }
    }

    // ========================================================================
    // Shared rendering helpers
    // ========================================================================

    /// Render a popup background with shadow, rounded corners, and border.
    fn render_popup_background(tree: &mut RenderTree, frame: Rect) {
        let Rect {
            x,
            y,
            w: width,
            h: height,
        } = frame;
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

    /// Render a horizontal slider track + thumb into its track rect.
    ///
    /// `track` is the rect the hit-test uses, so the thumb cannot be drawn
    /// anywhere the pointer is not accepted.
    fn render_slider(tree: &mut RenderTree, track: Rect, value: u8, active_color: Color) {
        let Rect {
            x,
            y,
            w: width,
            h: _,
        } = track;
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

    /// Render a toggle row (label + optional subtitle + toggle pill) into the
    /// rect the layout gave it.
    fn render_toggle_row(tree: &mut RenderTree, row: &ToggleRowLayout) {
        let Rect {
            x,
            y,
            w: width,
            h: height,
        } = row.rect;
        let label = row.label;
        let subtitle = row.subtitle.as_str();
        let enabled = row.enabled;
        // Label
        tree.push(RenderCommand::Text {
            x,
            y,
            text: String::from(label),
            color: palette::TEXT,
            font_size: POPUP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - TOGGLE_WIDTH - 8.0),
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Toggle pill (right-aligned), centred in the row's own rect.
        //
        // It used to be centred on the *label's* font size, which put its top
        // 2.5px above the row -- so the top sliver of every pill was inside
        // the row above it, and clicking there toggled the wrong switch.
        let toggle_x = x + width - TOGGLE_WIDTH;
        let toggle_y = y + (height - TOGGLE_HEIGHT) / 2.0;

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
    }
}

// ============================================================================
// Event routing
// ============================================================================

impl SystemTray {
    /// Route one window event into the tray.
    ///
    /// This is what `handle_click` was missing: a caller. It existed, it was
    /// correct, it carried ten tests, and no line of production code reached
    /// it, because `fn main` rendered a single frame and returned.
    pub fn handle_event(&mut self, event: &Event) -> TrayAction {
        match event {
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Key(key) if key.pressed && key.key == Key::Escape => {
                self.active_popup = PopupType::None;
                TrayAction::None
            }
            Event::Resize { width, height } => {
                self.set_viewport(*width as f32, *height as f32);
                TrayAction::None
            }
            Event::Tick { .. } => {
                self.refresh_clock();
                TrayAction::None
            }
            _ => TrayAction::None,
        }
    }

    fn handle_mouse(&mut self, mouse: &MouseEvent) -> TrayAction {
        match mouse.kind {
            MouseEventKind::Press(button) => self.handle_click_at(mouse.x, mouse.y, button).action,
            MouseEventKind::Move => {
                // A drag only continues while a slider is armed. Without the
                // guard every pointer movement across an open popup would be
                // a volume change.
                if let Some(slider) = self.dragging
                    && let Some(track) = self.track_of(slider)
                {
                    self.set_slider(slider, Self::value_at(track, mouse.x));
                }
                TrayAction::None
            }
            MouseEventKind::Release(_) => {
                // The release carries a position like any other pointer event,
                // and it is the last one the drag will see. Dropping it would
                // leave the slider one `Move` behind wherever the pointer
                // actually let go -- visible as a thumb that snaps backwards
                // on release if the release outruns the final move.
                if let Some(slider) = self.dragging
                    && let Some(track) = self.track_of(slider)
                {
                    self.set_slider(slider, Self::value_at(track, mouse.x));
                }
                self.dragging = None;
                TrayAction::None
            }
            _ => TrayAction::None,
        }
    }

    /// The track rect a slider currently occupies, re-derived from the layout
    /// rather than captured when the drag began: if the popup changed under
    /// the pointer, the drag must stop writing rather than keep writing to a
    /// rect that is no longer on screen.
    fn track_of(&self, slider: Slider) -> Option<Rect> {
        match self.popup_layout()? {
            PopupLayout::QuickSettings(l) => match slider {
                Slider::Brightness => Some(l.brightness),
                Slider::MasterVolume => Some(l.volume),
                Slider::AppVolume(_) => None,
            },
            PopupLayout::Volume(l) => match slider {
                Slider::MasterVolume => Some(l.master_slider),
                Slider::AppVolume(index) => l
                    .apps
                    .iter()
                    .find(|app| app.index == index)
                    .map(|app| app.slider),
                Slider::Brightness => None,
            },
            _ => None,
        }
    }

    /// Everything a frame depends on, cheap enough to snapshot every event.
    ///
    /// A tray that redrew on every mouse move would repaint the desktop
    /// corner sixty times a second while the pointer merely crossed it.
    fn display_revision(&self) -> DisplayRevision {
        DisplayRevision {
            popup: self.active_popup.clone(),
            calendar_offset: self.calendar_offset,
            clock: self.datetime.time_str(),
            date: self.datetime.date_str(),
            wifi: self.quick_settings.wifi_enabled,
            bluetooth: self.quick_settings.bluetooth_enabled,
            do_not_disturb: self.quick_settings.do_not_disturb,
            night_light: self.quick_settings.night_light,
            battery_saver: self.quick_settings.battery_saver,
            airplane_mode: self.quick_settings.airplane_mode,
            brightness: self.quick_settings.brightness,
            master_volume: self.volume.master_volume,
            muted: self.volume.muted,
            app_volumes: self
                .volume
                .app_volumes
                .iter()
                .map(|app| (app.volume, app.muted))
                .collect(),
            connected: self.network.connected,
            icons: self
                .icons
                .iter()
                .map(|icon| (icon.id, icon.visible, icon.has_notification_badge))
                .collect(),
        }
    }
}

/// A snapshot of everything [`SystemTray::render`] reads.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DisplayRevision {
    popup: PopupType,
    calendar_offset: i32,
    clock: String,
    date: String,
    wifi: bool,
    bluetooth: bool,
    do_not_disturb: bool,
    night_light: bool,
    battery_saver: bool,
    airplane_mode: bool,
    brightness: u8,
    master_volume: u8,
    muted: bool,
    app_volumes: Vec<(u8, bool)>,
    connected: bool,
    icons: Vec<(TrayIconId, bool, bool)>,
}

// ============================================================================
// Entry point
// ============================================================================

/// One second, not one minute.
///
/// The clock is displayed in whole minutes, but a sixty-second timer started
/// at an arbitrary moment turns the minute over up to fifty-nine seconds late,
/// so the tray would spend most of every minute showing the previous one.
const CLOCK_TICK: Duration = Duration::from_secs(1);

/// The size the tray asks for. `render` is still handed the size the window
/// actually is and believes that one instead — the first frame goes out before
/// any `Event::Resize`, and the hit-test reads the fields the renderer places
/// from.
const DEFAULT_VIEWPORT: (u32, u32) = (1920, 1080);

impl oswindow::app::App for SystemTray {
    fn title(&self) -> String {
        String::from("System Tray")
    }

    fn app_id(&self) -> String {
        String::from("systray")
    }

    fn initial_size(&self) -> (u32, u32) {
        DEFAULT_VIEWPORT
    }

    fn tick_interval(&self) -> Option<Duration> {
        // Never `None`. A tray that stopped ticking would stop knowing what
        // time it is, and having asked for no more ticks would never get
        // another chance to find out.
        Some(CLOCK_TICK)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        let before = self.display_revision();
        let action = self.handle_event(event);
        // A resize moves everything the tray draws without changing any of
        // the state the revision samples, so it is asked about separately.
        let moved = matches!(event, Event::Resize { .. }) || before != self.display_revision();
        match action {
            TrayAction::Power(PowerAction::ShutDown | PowerAction::SignOut) => Response::Exit,
            _ => {
                if moved {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.set_viewport(width, height);
        SystemTray::render(self)
    }
}

fn main() -> ExitCode {
    let mut tray = SystemTray::new(0.0, 0.0);
    tray.set_viewport(
        f32::from(u16::try_from(DEFAULT_VIEWPORT.0).unwrap_or(u16::MAX)),
        f32::from(u16::try_from(DEFAULT_VIEWPORT.1).unwrap_or(u16::MAX)),
    );
    // Before the first frame, not after: a tray whose first frame reads
    // `DateTime::default` shows 14:30 on 17 May 2026 for one frame no matter
    // what day it actually is.
    tray.refresh_clock();
    oswindow::app::launch("systray", &mut tray)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test: an `expect` that fires is
    // a failure report, and an index that is out of range is the assertion.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

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
        assert!(icon.is_some_and(|i| i.has_notification_badge));

        tray.set_badge(id, false);
        let icon = tray.icons.iter().find(|i| i.id == id);
        assert!(icon.is_some_and(|i| !i.has_notification_badge));
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
    fn test_tick_updates_battery() {
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.battery.estimated_minutes = Some(100);

        tray.tick(60_000); // 1 minute
        assert_eq!(tray.battery.estimated_minutes, Some(99));
    }

    #[test]
    fn test_tick_zero_elapsed() {
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.battery.estimated_minutes = Some(100);
        tray.tick(500); // Less than 1 second
        assert_eq!(tray.battery.estimated_minutes, Some(100));
    }

    /// `tick` used to advance the clock as well, and it carried seconds into
    /// minutes and minutes into hours but never hours into days. This is the
    /// test that used to prove the carry worked; what it could not see is that
    /// the date underneath never moved at all.
    #[test]
    fn the_clock_no_longer_has_a_second_writer() {
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.set_time_from_utc(1_787_745_600); // 2026-08-24 12:00:00 UTC
        let before = tray.datetime.clone();

        tray.tick(10_000);

        assert_eq!(tray.datetime.hour, before.hour);
        assert_eq!(tray.datetime.minute, before.minute);
        assert_eq!(tray.datetime.second, before.second);
        assert_eq!(tray.datetime.day, before.day);
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
        let mut dt = DateTime {
            year: 2024,
            month: 2,
            ..DateTime::default()
        };
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
        let mut dt = DateTime {
            year: 2025, // non-leap year
            ..DateTime::default()
        };
        let expected = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
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
    fn each_month_starts_where_the_previous_one_ran_out() {
        // The calendar popup indexes the first row of its grid by
        // `first_weekday_of_month` and fills it with `days_in_month` cells, so
        // the two have to agree: month n+1 must begin exactly
        // `days_in_month(n)` days after month n did. The single-month case
        // above cannot see a disagreement -- a weekday table wrong in eleven
        // of twelve entries passes it.
        //
        // 2024 is a leap year and 2025 is not, so February is checked both
        // ways, and the year rollover is checked in both directions.
        for year in [2024_u16, 2025] {
            for month in 1..=11_u8 {
                let this = DateTime {
                    year,
                    month,
                    ..DateTime::default()
                };
                let next = DateTime {
                    year,
                    month: month + 1,
                    ..DateTime::default()
                };
                let expected = (this.first_weekday_of_month() + this.days_in_month()) % 7;
                assert_eq!(
                    next.first_weekday_of_month(),
                    expected,
                    "{year}-{:02} starts on the wrong day given {year}-{month:02}",
                    month + 1
                );
            }
            let dec = DateTime {
                year,
                month: 12,
                ..DateTime::default()
            };
            let jan = DateTime {
                year: year + 1,
                month: 1,
                ..DateTime::default()
            };
            assert_eq!(
                jan.first_weekday_of_month(),
                (dec.first_weekday_of_month() + dec.days_in_month()) % 7,
                "{}-01 starts on the wrong day given {year}-12",
                year + 1
            );
        }
        // Anchors, so the relation above cannot be satisfied by two functions
        // that are consistently wrong together.
        let anchor = |year, month| {
            DateTime {
                year,
                month,
                ..DateTime::default()
            }
            .first_weekday_of_month()
        };
        assert_eq!(anchor(2024, 1), 1, "2024-01-01 was a Monday");
        assert_eq!(anchor(2024, 2), 4, "2024-02-01 was a Thursday");
        assert_eq!(anchor(2000, 1), 6, "2000-01-01 was a Saturday");
        assert_eq!(anchor(1970, 1), 4, "1970-01-01 was a Thursday");
    }

    #[test]
    fn every_month_renders_its_own_three_letter_name() {
        // `date_str` used to carry its own twelve-arm match. The single "Dec"
        // case above would pass over eleven wrong arms.
        let want = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        for (i, name) in want.iter().enumerate() {
            let month = u8::try_from(i + 1).unwrap_or(1);
            let dt = DateTime {
                year: 2026,
                month,
                day: 9,
                ..DateTime::default()
            };
            assert_eq!(dt.date_str(), format!("{name} 9, 2026"), "month {month}");
        }
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

    // ========================================================================
    // The clock
    //
    // `DateTime::default` is 2026-05-17 14:30, and until `set_time_from_utc`
    // existed nothing ever moved the date off it: `tick` carried seconds into
    // minutes and minutes into hours and stopped there. The tray therefore
    // showed a plausible time of day on a fixed day in May, forever.
    // ========================================================================

    /// 2026-08-24 12:00:00 UTC, a Monday.
    const NOON: i64 = 1_787_572_800;

    fn at(instant: i64) -> SystemTray {
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.set_time_from_utc(instant);
        tray
    }

    #[test]
    fn the_clock_reads_the_instant_it_is_given() {
        let tray = at(NOON);
        assert_eq!(tray.datetime.year, 2026);
        assert_eq!(tray.datetime.month, 8);
        assert_eq!(tray.datetime.day, 24);
        assert_eq!(tray.datetime.hour, 12);
        assert_eq!(tray.datetime.minute, 0);
        assert_eq!(tray.datetime.second, 0);
        assert_eq!(tray.datetime.weekday, 1, "2026-08-24 was a Monday");
        assert_eq!(tray.datetime.time_str(), "12:00");
        assert_eq!(tray.datetime.date_str(), "Aug 24, 2026");
    }

    #[test]
    fn the_clock_rolls_the_day_over_at_midnight() {
        // The exact failure `tick` had. One second apart, across midnight: the
        // old code wrapped the hour to zero and left the date in May.
        let before = at(NOON + 43_199); // 23:59:59 the same day
        let after = at(NOON + 43_200); // 00:00:00 the next day

        assert_eq!((before.datetime.day, before.datetime.hour), (24, 23));
        assert_eq!((after.datetime.day, after.datetime.hour), (25, 0));
        assert_eq!(after.datetime.weekday, 2, "the day after Monday");
    }

    #[test]
    fn a_pre_epoch_instant_is_still_a_time_of_day() {
        // `%` would give -1 here, which clamps to nothing sensible; the date
        // has to round *down* to the day that contains the instant.
        let tray = at(-1);
        assert_eq!(tray.datetime.year, 1969);
        assert_eq!(tray.datetime.month, 12);
        assert_eq!(tray.datetime.day, 31);
        assert_eq!(tray.datetime.time_str(), "23:59");
        assert_eq!(tray.datetime.second, 59);
    }

    #[test]
    fn the_clock_icon_carries_the_time_as_its_tooltip() {
        let tray = at(NOON + 3_660); // 13:01
        let clock = tray
            .icons
            .iter()
            .find(|i| i.id == ICON_ID_CLOCK)
            .expect("the clock icon is built in");
        assert_eq!(clock.tooltip, "13:01");
    }

    #[test]
    fn a_tick_advances_the_clock_through_the_event_loop() {
        // The wiring test: not "does `set_time_from_utc` work" but "does
        // anything call it". Seeded to a fixed instant in the past, one tick
        // through `handle_event` has to move the tray off it.
        let mut tray = at(NOON);
        tray.handle_event(&Event::Tick { elapsed_ms: 1_000 });
        assert_ne!(
            (tray.datetime.year, tray.datetime.month, tray.datetime.day),
            (2026, 8, 24),
            "the tick did not read the host clock"
        );
    }

    #[test]
    fn a_tray_that_shows_a_clock_never_stops_ticking() {
        use oswindow::app::App;
        let tray = SystemTray::new(0.0, 0.0);
        assert_eq!(tray.tick_interval(), Some(Duration::from_secs(1)));
    }

    // ========================================================================
    // Placement
    // ========================================================================

    #[test]
    fn render_believes_the_size_it_is_given_not_the_one_it_asked_for() {
        // The first frame goes out before any `Event::Resize`, so a tray that
        // trusted `initial_size` would draw itself off the right-hand edge of
        // every window that is not 1920 wide.
        use oswindow::app::App;
        let mut tray = SystemTray::new(0.0, 0.0);
        let tree = App::render(&mut tray, 800.0, 600.0);
        let (x, y, w, h) = tree
            .commands
            .iter()
            .find_map(|cmd| match *cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some((x, y, width, height)),
                _ => None,
            })
            .expect("the tray bar is the first thing drawn");
        assert!(
            x + w <= 800.0,
            "the tray reaches x={} on an 800px screen",
            x + w
        );
        assert!(
            y + h <= 600.0,
            "the tray reaches y={} on a 600px screen",
            y + h
        );
        assert!(x + w > 700.0 && y + h > 500.0, "and it is in the corner");
    }

    #[test]
    fn registering_an_icon_keeps_the_tray_in_its_corner() {
        // `recalculate_width` used to widen the bar without moving it, so a
        // right-anchored tray grew off the edge of the screen.
        let mut tray = SystemTray::new(0.0, 0.0);
        tray.set_viewport(800.0, 600.0);
        let right_before = tray.tray_x + tray.tray_width;
        tray.register_icon("Mail", 'M', "Mail");
        assert!((tray.tray_x + tray.tray_width - right_before).abs() < 0.01);
    }

    // ========================================================================
    // Hit-testing
    //
    // Every landmark below is read out of the render output. Computing the
    // expected position from `Rect`/the layout would make the assertion agree
    // with the hit-test however wrong both were.
    // ========================================================================

    fn open(popup: PopupType) -> SystemTray {
        let mut tray = at(NOON);
        tray.set_viewport(1280.0, 800.0);
        tray.active_popup = popup;
        tray
    }

    /// The popup's frame, as drawn. Only a popup emits a `BoxShadow`.
    fn drawn_popup_frame(tray: &SystemTray) -> Rect {
        tray.render()
            .commands
            .iter()
            .find_map(|cmd| match *cmd {
                RenderCommand::BoxShadow {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(x, y, width, height)),
                _ => None,
            })
            .expect("a popup is open")
    }

    /// The drawn toggle pills, in draw order. Only a pill is exactly
    /// `TOGGLE_WIDTH` x `TOGGLE_HEIGHT`.
    fn drawn_pills(tray: &SystemTray) -> Vec<Rect> {
        tray.render()
            .commands
            .iter()
            .filter_map(|cmd| match *cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if (width - TOGGLE_WIDTH).abs() < 0.01
                    && (height - TOGGLE_HEIGHT).abs() < 0.01 =>
                {
                    Some(Rect::new(x, y, width, height))
                }
                _ => None,
            })
            .collect()
    }

    /// The drawn slider tracks, in draw order: the full-width background run
    /// of each slider, `SLIDER_TRACK_HEIGHT` tall.
    fn drawn_tracks(tray: &SystemTray) -> Vec<Rect> {
        tray.render()
            .commands
            .iter()
            .filter_map(|cmd| match *cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if (height - SLIDER_TRACK_HEIGHT).abs() < 0.01 && color == palette::SURFACE0 => {
                    Some(Rect::new(x, y, width, height))
                }
                _ => None,
            })
            .collect()
    }

    fn all_toggles() -> [Toggle; 6] {
        [
            Toggle::Wifi,
            Toggle::Bluetooth,
            Toggle::DoNotDisturb,
            Toggle::NightLight,
            Toggle::BatterySaver,
            Toggle::AirplaneMode,
        ]
    }

    fn toggle_snapshot(tray: &SystemTray) -> Vec<bool> {
        all_toggles()
            .iter()
            .map(|&t| tray.toggle_enabled(t))
            .collect()
    }

    /// Which switch — if any — a click at this point flips, found by trying it
    /// on a fresh tray. Nothing here consults the layout.
    fn toggle_hit_at(x: f32, y: f32) -> Option<Toggle> {
        let mut tray = open(PopupType::QuickSettings);
        // Primed with both radios already off, so that one switch's side
        // effects cannot be read as another switch having been clicked:
        // turning airplane mode *on* turns wifi and bluetooth off, so on a
        // default tray a click on the airplane row moves three switches and
        // the first one that moved is not the one under the pointer. With the
        // radios off to begin with, every row moves exactly its own switch.
        // Neither subtitle disappears when its radio does, so the rows are the
        // same height either way and the geometry being probed is unchanged.
        tray.set_toggle(Toggle::Wifi, false);
        tray.set_toggle(Toggle::Bluetooth, false);
        let before = toggle_snapshot(&tray);
        tray.handle_click_at(x, y, MouseButton::Left);
        let after = toggle_snapshot(&tray);
        all_toggles()
            .into_iter()
            .zip(before.iter().zip(after.iter()))
            .find(|(_, (b, a))| b != a)
            .map(|(toggle, _)| toggle)
    }

    #[test]
    fn every_drawn_toggle_pill_is_inside_its_own_clickable_row() {
        // The assertion that an off-by-anything in the row rects fails. The
        // pill positions come from the render output and the row extents come
        // from probing clicks, so the two are found independently.
        let tray = open(PopupType::QuickSettings);
        let pills = drawn_pills(&tray);
        assert_eq!(pills.len(), 6, "six switches are drawn");

        for (pill, expected) in pills.iter().zip(all_toggles()) {
            let probe_x = pill.x + pill.w / 2.0;
            // Top edge, bottom edge and middle: a row rect displaced by even
            // one pixel loses one of the three.
            for probe_y in [pill.y, pill.y + pill.h / 2.0, pill.y + pill.h - 0.5] {
                assert_eq!(
                    toggle_hit_at(probe_x, probe_y),
                    Some(expected),
                    "the pill drawn at y={} is not inside {expected:?}'s row",
                    pill.y
                );
            }
        }
    }

    #[test]
    fn the_toggle_rows_tile_without_a_gap_or_an_overlap() {
        // Between one pill's bottom and the next pill's top there is padding
        // that belongs to one row or the other, and it must belong to exactly
        // one: a gap there is a dead strip in the middle of the list.
        let tray = open(PopupType::QuickSettings);
        let pills = drawn_pills(&tray);
        let x = pills[0].x + pills[0].w / 2.0;

        let top = pills[0].y;
        let bottom = pills[5].y + pills[5].h;
        let mut seen: Vec<Toggle> = Vec::new();
        let steps = ((bottom - top) as i32).max(1);
        for step in 0..steps {
            let y = top + step as f32;
            let hit = toggle_hit_at(x, y)
                .unwrap_or_else(|| panic!("y={y} is inside the list but flips nothing"));
            if seen.last() != Some(&hit) {
                assert!(!seen.contains(&hit), "{hit:?} owns two separate bands");
                seen.push(hit);
            }
        }
        assert_eq!(seen, all_toggles().to_vec(), "in the order they are drawn");
    }

    #[test]
    fn a_click_above_the_first_row_flips_nothing() {
        let tray = open(PopupType::QuickSettings);
        let pills = drawn_pills(&tray);
        let x = pills[0].x + pills[0].w / 2.0;
        // The header sits above the first row; clicking it must do nothing at
        // all -- and in particular must not dismiss the popup under the
        // pointer.
        let mut probe = open(PopupType::QuickSettings);
        let before = toggle_snapshot(&probe);
        probe.handle_click_at(x, pills[0].y - 12.0, MouseButton::Left);
        assert_eq!(toggle_snapshot(&probe), before);
        assert_eq!(probe.active_popup, PopupType::QuickSettings);
    }

    #[test]
    fn a_click_inside_the_popup_does_not_dismiss_it() {
        let mut tray = open(PopupType::Network);
        let frame = drawn_popup_frame(&tray);
        let outcome = tray.handle_click_at(frame.x + 4.0, frame.y + 4.0, MouseButton::Left);
        assert!(outcome.consumed);
        assert_eq!(tray.active_popup, PopupType::Network);
    }

    #[test]
    fn a_click_outside_the_popup_dismisses_it() {
        let mut tray = open(PopupType::Network);
        let frame = drawn_popup_frame(&tray);
        tray.handle_click_at(frame.x - 4.0, frame.y - 4.0, MouseButton::Left);
        assert_eq!(tray.active_popup, PopupType::None);
    }

    // ========================================================================
    // Sliders
    // ========================================================================

    #[test]
    fn a_click_on_a_slider_sets_it_to_where_it_was_clicked() {
        let mut tray = open(PopupType::QuickSettings);
        let track = drawn_tracks(&tray)[0]; // brightness is drawn first
        let y = track.y + track.h / 2.0;

        tray.handle_click_at(track.x + track.w * 0.25, y, MouseButton::Left);
        assert_eq!(tray.quick_settings.brightness, 25);

        tray.handle_click_at(track.x, y, MouseButton::Left);
        assert_eq!(tray.quick_settings.brightness, 0);

        // The far end of the track is its last contained pixel, not the pixel
        // after it: `Rect::contains` is half-open so that two adjacent rects
        // cannot both claim one column.
        tray.handle_click_at(track.x + track.w - 0.5, y, MouseButton::Left);
        assert!(
            tray.quick_settings.brightness >= 99,
            "the right end of the track is full brightness, got {}",
            tray.quick_settings.brightness
        );

        // And one pixel further right is not the slider at all.
        tray.set_slider(Slider::Brightness, 42);
        tray.handle_click_at(track.x + track.w, y, MouseButton::Left);
        assert_eq!(tray.quick_settings.brightness, 42);
    }

    #[test]
    fn a_slider_follows_the_pointer_while_it_is_held_and_stops_when_released() {
        let mut tray = open(PopupType::QuickSettings);
        let track = drawn_tracks(&tray)[0];
        let y = track.y + track.h / 2.0;

        tray.handle_event(&Event::Mouse(MouseEvent {
            x: track.x + track.w * 0.2,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(tray.quick_settings.brightness, 20);

        // Dragged past the end of the track, and above it: the pointer does
        // not have to stay inside a control it has already grabbed.
        tray.handle_event(&Event::Mouse(MouseEvent {
            x: track.x + track.w * 2.0,
            y: y - 400.0,
            kind: MouseEventKind::Move,
        }));
        assert_eq!(tray.quick_settings.brightness, 100);

        tray.handle_event(&Event::Mouse(MouseEvent {
            x: track.x,
            y,
            kind: MouseEventKind::Release(MouseButton::Left),
        }));
        assert_eq!(tray.quick_settings.brightness, 0);

        // And after the release the pointer is just a pointer again.
        tray.handle_event(&Event::Mouse(MouseEvent {
            x: track.x + track.w,
            y,
            kind: MouseEventKind::Move,
        }));
        assert_eq!(tray.quick_settings.brightness, 0);
    }

    #[test]
    fn a_bare_mouse_move_over_the_tray_changes_nothing() {
        let mut tray = open(PopupType::QuickSettings);
        let before = tray.quick_settings.brightness;
        tray.handle_event(&Event::Mouse(MouseEvent {
            x: drawn_tracks(&tray)[0].x,
            y: drawn_tracks(&tray)[0].y,
            kind: MouseEventKind::Move,
        }));
        assert_eq!(tray.quick_settings.brightness, before);
    }

    #[test]
    fn the_signal_strength_bar_is_not_a_slider() {
        // It is drawn with the slider helper because it looks like one. It is
        // a read-out: a click must not "turn up" the reception.
        let mut tray = open(PopupType::Network);
        let track = drawn_tracks(&tray)[0];
        tray.handle_click_at(
            track.x + track.w,
            track.y + track.h / 2.0,
            MouseButton::Left,
        );
        assert_eq!(tray.network.signal_strength, 85);
        assert_eq!(tray.active_popup, PopupType::Network, "and it is swallowed");
    }

    #[test]
    fn moving_the_master_volume_off_zero_unmutes() {
        let mut tray = open(PopupType::Volume);
        tray.volume.muted = true;
        let track = drawn_tracks(&tray)[0]; // master is drawn first
        tray.handle_click_at(
            track.x + track.w / 2.0,
            track.y + track.h / 2.0,
            MouseButton::Left,
        );
        assert_eq!(tray.volume.master_volume, 50);
        assert!(!tray.volume.muted);
    }

    #[test]
    fn a_click_on_an_app_row_mutes_that_app_and_only_that_app() {
        let mut tray = open(PopupType::Volume);
        let tracks = drawn_tracks(&tray);
        // master, then one per app.
        assert_eq!(tracks.len(), 1 + tray.volume.app_volumes.len());
        // The label sits directly above its own slider; the row between the
        // previous slider and this one belongs to it.
        let label_y = tracks[1].y - SLIDER_THUMB_RADIUS;
        tray.handle_click_at(tracks[1].x + 4.0, label_y, MouseButton::Left);
        assert!(tray.volume.app_volumes[0].muted);
        assert!(!tray.volume.app_volumes[1].muted);
        assert!(!tray.volume.muted, "the master is untouched");
    }

    // ========================================================================
    // Calendar
    // ========================================================================

    /// The blue "today" pill, if the drawn month contains today.
    fn drawn_today_highlight(tray: &SystemTray) -> Option<Rect> {
        tray.render().commands.iter().find_map(|cmd| match *cmd {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                ..
            } if color == palette::BLUE && (width - (CALENDAR_CELL - 4.0)).abs() < 0.01 => {
                Some(Rect::new(x, y, width, height))
            }
            _ => None,
        })
    }

    #[test]
    fn the_calendar_arrows_move_the_month_and_the_title_comes_back() {
        // The popup is as tall as the month it is showing, so its frame moves
        // as the month changes -- a five-row September hangs from the tray
        // bar lower than a six-row August. Every click below therefore aims at
        // the frame as it is *now*, read back out of the render output; aiming
        // them all at the first frame put the pointer above the popup after
        // the first arrow and dismissed it.
        fn header_click(tray: &mut SystemTray, place: fn(Rect) -> f32) {
            let frame = drawn_popup_frame(tray);
            let x = place(frame);
            tray.handle_click_at(x, frame.y + POPUP_PADDING + 2.0, MouseButton::Left);
        }
        fn next(tray: &mut SystemTray) {
            header_click(tray, |f| f.x + f.w - POPUP_PADDING - 4.0);
        }
        fn prev(tray: &mut SystemTray) {
            header_click(tray, |f| f.x + POPUP_PADDING + 4.0);
        }
        fn title(tray: &mut SystemTray) {
            header_click(tray, |f| f.x + f.w / 2.0);
        }

        let mut tray = open(PopupType::Calendar);
        assert!(
            drawn_today_highlight(&tray).is_some(),
            "August contains today"
        );

        next(&mut tray);
        assert_eq!(tray.calendar_offset, 1);
        assert!(
            drawn_today_highlight(&tray).is_none(),
            "September does not contain 24 August"
        );

        prev(&mut tray);
        assert_eq!(tray.calendar_offset, 0);

        // Six months forward, then one click on the month name to come home.
        for _ in 0..6 {
            next(&mut tray);
        }
        assert_eq!(tray.calendar_offset, 6);
        title(&mut tray);
        assert_eq!(tray.calendar_offset, 0);
        assert!(drawn_today_highlight(&tray).is_some());
    }

    #[test]
    fn a_six_row_month_still_fits_inside_the_popup() {
        // August 2026 starts on a Saturday and has 31 days, so its grid needs
        // six rows. The popup used to be a fixed 280px tall, which fits five.
        let tray = open(PopupType::Calendar);
        let frame = drawn_popup_frame(&tray);
        let tree = tray.render();
        let mut popup_seen = false;
        for cmd in &tree.commands {
            if matches!(cmd, RenderCommand::BoxShadow { .. }) {
                popup_seen = true;
                continue;
            }
            if !popup_seen {
                continue; // the tray bar itself is outside the popup
            }
            let (y, height) = match *cmd {
                RenderCommand::FillRect { y, height, .. }
                | RenderCommand::StrokeRect { y, height, .. } => (y, height),
                RenderCommand::Text { y, font_size, .. } => (y, font_size),
                _ => continue,
            };
            assert!(
                y >= frame.y && y + height <= frame.y + frame.h,
                "something is drawn at y={y}..{} outside the popup {}..{}",
                y + height,
                frame.y,
                frame.y + frame.h
            );
        }

        // Staying inside the frame is not enough on its own: a grid one row
        // too short for its month still *fits*, because the sixth row lands in
        // the space reserved for the events section and is merely drawn on top
        // of it. The rule under the grid is the boundary that says so -- it is
        // the only 1px-tall fill in the popup -- and no day may cross it.
        let rule_y = tree
            .commands
            .iter()
            .find_map(|cmd| match *cmd {
                RenderCommand::FillRect { y, height, .. } if (height - 1.0).abs() < 0.01 => Some(y),
                _ => None,
            })
            .expect("the calendar draws a rule between the grid and the events line");
        let mut days_drawn = 0_u32;
        for cmd in &tree.commands {
            let RenderCommand::Text {
                y,
                ref text,
                font_size,
                ..
            } = *cmd
            else {
                continue;
            };
            if text.parse::<u32>().is_err() {
                continue; // the month name, the arrows, the weekday initials
            }
            days_drawn += 1;
            assert!(
                y + font_size <= rule_y,
                "day {text} is drawn at y={y}..{}, over the rule at {rule_y}",
                y + font_size
            );
        }
        assert_eq!(days_drawn, 31, "August has 31 days and draws all of them");
    }

    #[test]
    fn every_popup_is_tall_enough_for_what_it_draws() {
        // The heights used to be hardcoded: 340 for a 305px quick-settings
        // panel, and 160 for a network panel that needs 166.
        for popup in [
            PopupType::QuickSettings,
            PopupType::Volume,
            PopupType::Network,
            PopupType::Calendar,
            PopupType::PowerMenu,
            PopupType::ContextMenu(ICON_ID_VOLUME),
            PopupType::AppMenu(ICON_ID_VOLUME),
        ] {
            let tray = open(popup.clone());
            let frame = drawn_popup_frame(&tray);
            let tree = tray.render();
            let mut popup_seen = false;
            for cmd in &tree.commands {
                if matches!(cmd, RenderCommand::BoxShadow { .. }) {
                    popup_seen = true;
                    continue;
                }
                if !popup_seen {
                    continue;
                }
                let (y, height) = match *cmd {
                    RenderCommand::FillRect { y, height, .. }
                    | RenderCommand::StrokeRect { y, height, .. } => (y, height),
                    RenderCommand::Text { y, font_size, .. } => (y, font_size),
                    _ => continue,
                };
                assert!(
                    y >= frame.y && y + height <= frame.y + frame.h,
                    "{popup:?} draws at y={y}..{} outside its {}..{} frame",
                    y + height,
                    frame.y,
                    frame.y + frame.h
                );
            }
        }
    }

    // ========================================================================
    // Menus
    // ========================================================================

    /// Click every row of the open menu in turn, on a fresh tray each time,
    /// and report what each one asked for.
    fn menu_actions(popup: PopupType, rows: usize) -> Vec<TrayAction> {
        let probe = open(popup.clone());
        let frame = drawn_popup_frame(&probe);
        let row_height = POPUP_FONT_SIZE + ITEM_SPACING;
        // Rows start below the header, if there is one; find the first row by
        // walking down from the top of the content box until a click does
        // something.
        (0..rows)
            .map(|i| {
                let mut tray = open(popup.clone());
                let content_top = frame.y + frame.h - POPUP_PADDING - (rows as f32) * row_height;
                let y = content_top + (i as f32 + 0.5) * row_height;
                tray.handle_click_at(frame.x + frame.w / 2.0, y, MouseButton::Left)
                    .action
            })
            .collect()
    }

    #[test]
    fn the_power_menu_reports_the_item_that_was_clicked() {
        let actions = menu_actions(PopupType::PowerMenu, 5);
        assert_eq!(
            actions,
            vec![
                TrayAction::Power(PowerAction::ShutDown),
                TrayAction::Power(PowerAction::Restart),
                TrayAction::Power(PowerAction::Sleep),
                TrayAction::Power(PowerAction::Lock),
                TrayAction::Power(PowerAction::SignOut),
            ]
        );
    }

    #[test]
    fn the_app_menu_opens_the_app_it_belongs_to() {
        let actions = menu_actions(PopupType::AppMenu(ICON_ID_VOLUME), 1);
        assert_eq!(actions, vec![TrayAction::OpenApp(ICON_ID_VOLUME)]);
    }

    #[test]
    fn the_context_menu_hides_and_removes_the_icon_it_belongs_to() {
        let popup = PopupType::ContextMenu(ICON_ID_BATTERY);
        let probe = open(popup.clone());
        let frame = drawn_popup_frame(&probe);
        let row_height = POPUP_FONT_SIZE + ITEM_SPACING;
        let content_top = frame.y + frame.h - POPUP_PADDING - 3.0 * row_height;
        let x = frame.x + frame.w / 2.0;

        let mut hiding = open(popup.clone());
        hiding.handle_click_at(x, content_top + 1.5 * row_height, MouseButton::Left);
        assert!(
            !hiding
                .icons
                .iter()
                .any(|i| i.id == ICON_ID_BATTERY && i.visible)
        );
        assert_eq!(hiding.active_popup, PopupType::None, "the menu closes");

        let mut removing = open(popup);
        removing.handle_click_at(x, content_top + 2.5 * row_height, MouseButton::Left);
        assert!(!removing.icons.iter().any(|i| i.id == ICON_ID_BATTERY));
    }

    #[test]
    fn the_network_link_asks_for_the_settings_page() {
        let mut tray = open(PopupType::Network);
        let frame = drawn_popup_frame(&tray);
        let link_height = POPUP_FONT_SIZE + 4.0;
        let y = frame.y + frame.h - POPUP_PADDING - link_height / 2.0;
        let outcome = tray.handle_click_at(frame.x + POPUP_PADDING + 4.0, y, MouseButton::Left);
        assert_eq!(outcome.action, TrayAction::OpenNetworkSettings);
        assert_eq!(tray.active_popup, PopupType::None);
    }

    // ========================================================================
    // Toggle semantics
    // ========================================================================

    #[test]
    fn airplane_mode_turns_the_radios_off() {
        let mut tray = at(NOON);
        tray.set_toggle(Toggle::AirplaneMode, true);
        assert!(!tray.quick_settings.wifi_enabled);
        assert!(!tray.quick_settings.bluetooth_enabled);
        assert!(
            !tray.network.connected,
            "and the network drops with the radio"
        );

        tray.set_toggle(Toggle::Wifi, true);
        assert!(
            !tray.quick_settings.airplane_mode,
            "turning WiFi back on leaves airplane mode"
        );
    }

    // ========================================================================
    // The event loop's contract
    // ========================================================================

    #[test]
    fn an_event_the_tray_ignores_does_not_ask_for_a_frame() {
        use oswindow::app::App;
        let mut tray = at(NOON);
        tray.set_viewport(1280.0, 800.0);
        // A mouse move over empty desktop, and a key the tray has no use for.
        assert_eq!(
            tray.on_event(&Event::Mouse(MouseEvent {
                x: 10.0,
                y: 10.0,
                kind: MouseEventKind::Move,
            })),
            Response::Idle
        );
        assert_eq!(tray.on_event(&Event::FocusIn), Response::Idle);
    }

    #[test]
    fn opening_a_popup_asks_for_a_frame() {
        use oswindow::app::App;
        let mut tray = at(NOON);
        tray.set_viewport(1280.0, 800.0);
        let icon_centre_y = tray.tray_y + TRAY_HEIGHT / 2.0;
        let response = tray.on_event(&Event::Mouse(MouseEvent {
            x: tray.tray_x + ICON_CELL_SIZE * 1.5, // the volume icon
            y: icon_centre_y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(response, Response::Redraw);
        assert_eq!(tray.active_popup, PopupType::Volume);
    }

    #[test]
    fn escape_closes_the_open_popup() {
        use guitk::event::{KeyEvent, Modifiers};
        use oswindow::app::App;
        let mut tray = open(PopupType::PowerMenu);
        let response = tray.on_event(&Event::Key(KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }));
        assert_eq!(response, Response::Redraw);
        assert_eq!(tray.active_popup, PopupType::None);
    }

    #[test]
    fn shutting_down_closes_the_window() {
        use oswindow::app::App;
        let popup = PopupType::PowerMenu;
        let probe = open(popup.clone());
        let frame = drawn_popup_frame(&probe);
        let row_height = POPUP_FONT_SIZE + ITEM_SPACING;
        let content_top = frame.y + frame.h - POPUP_PADDING - 5.0 * row_height;

        let mut tray = open(popup);
        let response = tray.on_event(&Event::Mouse(MouseEvent {
            x: frame.x + frame.w / 2.0,
            y: content_top + 0.5 * row_height,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(response, Response::Exit);
    }
}

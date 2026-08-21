//! Slate OS Desktop Shell
//!
//! Window manager and desktop environment providing:
//! - Window management (move, resize, minimize, maximize, close)
//! - Taskbar with running application list
//! - System tray (clock, notifications, quick settings)
//! - Start menu / application launcher
//! - Virtual desktops
//! - Keyboard shortcuts (Alt+Tab, Alt+F4, Super key, etc.)
//!
//! Every one of those produces a `guitk` `RenderTree` for something else to
//! paint. This crate is a library so that something else can exist: the binary
//! beside it is a scripted demonstration, not the shell.
//!
//! # What this crate does not do yet
//!
//! Two things a reader would reasonably assume from the list above, stated
//! plainly because assuming them is how wrong code gets written against this.
//! Both replace doc claims that used to be here and were not true.
//!
//! - **It does not talk to the compositor.** There is no connection, no event
//!   loop, and nothing that submits a rendered tree to a surface. The protocol
//!   it would use exists (`guiremote`, including the window-list subscription
//!   a taskbar needs); the loop that would use it does not. See
//!   `known-issues.md`
//!   `TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO`.
//! - **Theme support reaches five surfaces, not the desktop.** The appearance
//!   settings are read and honoured by [`DesktopShell`]'s own render methods.
//!   The 49 modules beside it — every settings page, dialog and OSD — each hold
//!   a private hardcoded palette and ignore the user's choice entirely. See
//!   `known-issues.md`
//!   `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.

// The desktop shell is a widget-heavy crate: render/draw functions
// commonly take many positional parameters (font, theme, geometry,
// hit-test state, cursor, etc.), and several network/graphics protocol
// names use capitalized acronyms (VPN, WEP, WPA, WPA2, SRGB) that
// match RFC / spec terminology.
#![allow(clippy::too_many_arguments, clippy::upper_case_acronyms)]
#![cfg_attr(
    test,
    allow(
        clippy::field_reassign_with_default,
        clippy::bool_assert_comparison,
        clippy::needless_borrows_for_generic_args,
        clippy::manual_range_contains,
    )
)]

pub mod a11y;
pub mod about;
pub mod accessibility_settings;
pub mod animations;
pub mod appearance_settings;
pub mod backup_settings;
pub mod bluetooth;
pub mod blur;
pub mod calendar;
pub mod clipboard_viewer;
pub mod context_ext;
pub mod datetime_settings;
pub mod default_apps;
pub mod device_settings;
pub mod display_settings;
pub mod file_drop;
pub mod focus_assist;
pub mod hotkeys;
pub mod icons;
pub mod input_method;
pub mod language_settings;
pub mod launcher;
pub mod login_screen;
pub mod mouse_settings;
pub mod multimon;
pub mod network_indicator;
pub mod network_settings;
pub mod notif_pane;
pub mod notification_settings;
pub mod osd;
pub mod overview;
pub mod power;
pub mod power_settings;
pub mod print_manager;
pub mod privacy_settings;
pub mod resmon;
pub mod run_dialog;
pub mod screen_capture;
pub mod security_dialog;
pub mod session_mgr;
pub mod snap;
pub mod sound_settings;
pub mod startup_settings;
pub mod storage_settings;
pub mod taskbar;
pub mod taskbar_autohide;
pub mod touchpad;
pub mod tray_dnd;
pub mod update_settings;
pub mod user_accounts;
pub mod wallpaper;
pub mod widgets;
pub mod window_peek;
pub mod window_rules;

#[cfg(test)]
mod pointer_tests;

use appearance::config;
use appearance::{AppearanceSettings, TaskbarStyle, TransparencyLevel};
use guitk::color::Color;
use guitk::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::RenderTree;
use guitk::step;
use guitk::style::{Border, CornerRadii, Shadow};
use guitk::text;
use guitk::wheel;
use launcher::{AppEntry, Category};
// The same zone engine the libc's `localtime`, osh's `printf '%(…)T'`, the
// calendar panel and the Date & Time settings page render through, so the
// taskbar cannot disagree with `date` about what time it is.
use tzrules::Tz;

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// Geometry
// ============================================================================

/// An axis-aligned rectangle in screen pixels.
///
/// Every clickable part of the shell is described by exactly one `*_rect`
/// accessor, which both the renderer and the mouse handler call. The
/// alternative — a literal in the draw call and a matching literal in the hit
/// test — produces a button that is clickable somewhere other than where it is
/// drawn as soon as one of the two is edited, and nothing about the code makes
/// the second one obviously wrong.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Whether a point is inside.
    ///
    /// The left and top edges count as inside and the right and bottom edges as
    /// outside, so two rectangles that share an edge cannot both claim the same
    /// pixel — which is how a row of adjacent buttons must behave.
    #[must_use]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Paint a rectangle. A thin wrapper so a rect can be passed as one value
/// rather than unpacked into four arguments at every call site.
fn fill(tree: &mut RenderTree, rect: Rect, color: Color) {
    tree.fill_rect(rect.x, rect.y, rect.w, rect.h, color);
}

/// Paint a rectangle with rounded corners.
fn fill_round(tree: &mut RenderTree, rect: Rect, color: Color, radii: CornerRadii) {
    tree.fill_rounded_rect(rect.x, rect.y, rect.w, rect.h, color, radii);
}

/// Outline a rectangle.
///
/// There is no square-cornered counterpart because there is nothing the shell
/// outlines that does not follow the user's corner setting: pass
/// [`CornerRadii::ZERO`] for a square one.
fn stroke_round(tree: &mut RenderTree, rect: Rect, color: Color, width: f32, radii: CornerRadii) {
    tree.stroke_rounded_rect(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        Border { width, color },
        radii,
    );
}

/// Cast a drop shadow under a rectangle.
///
/// Emitted before the surface that casts it, since the command list is painted
/// in order.
fn shadow(tree: &mut RenderTree, rect: Rect, radii: CornerRadii) {
    tree.box_shadow(rect.x, rect.y, rect.w, rect.h, WINDOW_SHADOW, radii);
}

/// How many whole rows a wheel event of `dy` **notches** moves a list of
/// fixed-height rows, carrying the fraction in `acc`.
///
/// This used to divide `dy` by `START_MENU_ROW_HEIGHT`, as if `dy` were a
/// pixel measurement — see [`MouseEventKind::Scroll`], which is measured in
/// notches. A notch is `1.0` and a row is 36 px, so the quotient truncated to
/// zero for every delta any real device produces, and the whole computation was
/// dead: what actually scrolled the menu was the fallback below it, which moved
/// exactly one row for any non-zero `dy` whatsoever. The menu therefore ignored
/// how hard the wheel was turned — three notches moved one row, and so did a
/// trackpad's twitch of a twentieth of one.
///
/// The remainder has to be carried because `start_menu_scroll` counts whole
/// rows and cannot hold a fraction: rounding each event separately would
/// discard every sub-row delta a trackpad sends, which is the same "scrolls
/// nothing at all" failure in a different disguise.
fn scroll_rows(acc: &mut wheel::Accumulator, dy: f32) -> i32 {
    let rows = acc.rows(dy);
    i32::try_from(rows).unwrap_or(if rows < 0 { i32::MIN } else { i32::MAX })
}

// --- Taskbar ---------------------------------------------------------------

/// Width of the start button at the left end of the taskbar.
const START_BUTTON_WIDTH: f32 = 48.0;
/// Gap between the start button and the first window button.
const TASKBAR_BUTTON_START_GAP: f32 = 8.0;
/// Gap between adjacent window buttons.
const TASKBAR_BUTTON_GAP: f32 = 4.0;
/// Vertical inset of a window button inside the panel.
const TASKBAR_BUTTON_INSET: f32 = 4.0;
/// Widest a window button gets, however few windows are open.
const TASKBAR_BUTTON_MAX_WIDTH: f32 = 160.0;
/// Narrowest the system tray gets, however little is in it.
///
/// The tray's real width is *measured* — see
/// [`DesktopShell::tray_width`] — because the clock's width is a setting.
/// This floor only stops a bare `16:30` from letting the window buttons run
/// almost to the display edge.
const TRAY_MIN_WIDTH: f32 = 120.0;
/// Gap at the tray's outer edge and between the items inside it.
const TRAY_PADDING: f32 = 8.0;
/// Extra room the window buttons leave beyond the tray, so the last button does
/// not end flush against the desktop indicator.
const TRAY_RESERVE_GAP: f32 = 20.0;

// --- Start menu ------------------------------------------------------------

const START_MENU_WIDTH: f32 = 300.0;
const START_MENU_HEIGHT: f32 = 400.0;
/// Space above the first application row, holding the "Applications" heading.
const START_MENU_TOP_PADDING: f32 = 50.0;
const START_MENU_ROW_HEIGHT: f32 = 36.0;
/// Space below the last application row, holding the power options.
const START_MENU_FOOTER: f32 = 48.0;
/// Width of the scroll indicator drawn when the list is longer than the menu.
const START_MENU_SCROLLBAR_WIDTH: f32 = 4.0;

// --- Power menu ------------------------------------------------------------

/// Width of the power button in the start menu's footer.
const POWER_BUTTON_WIDTH: f32 = 110.0;
/// Inset of the power button from the menu's left and bottom edges.
const POWER_BUTTON_INSET: f32 = 8.0;
const POWER_MENU_WIDTH: f32 = 170.0;
const POWER_MENU_ROW_HEIGHT: f32 = 32.0;
/// Space above the first and below the last row of the popup.
const POWER_MENU_PADDING: f32 = 6.0;
/// Gap between the power button and the popup that rises from it.
const POWER_MENU_GAP: f32 = 6.0;
/// Distance from a popup row's left edge to the start of its label.
const POWER_MENU_TEXT_INSET: f32 = 14.0;

// --- Window decorations ----------------------------------------------------

const TITLE_BAR_HEIGHT: f32 = 30.0;
const WINDOW_BUTTON_SIZE: f32 = 16.0;
/// Distance between the left edges of two adjacent title-bar buttons.
const WINDOW_BUTTON_PITCH: f32 = 24.0;
/// Distance from the window's right edge to the left edge of the close button.
const WINDOW_BUTTON_MARGIN_RIGHT: f32 = 30.0;
const WINDOW_BUTTON_MARGIN_TOP: f32 = 7.0;

// --- Drop shadows ----------------------------------------------------------

/// The shadow every floating surface casts — windows, the start menu, the
/// Alt+Tab overlay. One shadow rather than one per surface: they are all
/// floating the same distance above the same desktop, and shadows that
/// disagreed about the light source would look like a rendering fault.
const WINDOW_SHADOW: Shadow = Shadow::drop(4.0, 12.0, Color::rgba(0, 0, 0, 90));

// --- Type scale ------------------------------------------------------------

/// What a piece of text is *for*, which is what decides how large it is drawn.
///
/// Sizes are multiples of the user's chosen UI font size rather than pixel
/// literals. A literal `13.0` at a draw call silently ignores both the font
/// size in the appearance panel and the display scaling, so raising either one
/// enlarges the chrome around the text and leaves the text itself behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRole {
    /// Icon glyphs — the start button's hamburger.
    Glyph,
    /// A panel heading, such as the start menu's "Applications".
    Heading,
    /// A row in a list the user picks from.
    Item,
    /// Ordinary interface text: window titles, the clock.
    Body,
    /// Secondary text: the desktop indicator, the power label.
    Caption,
}

impl TextRole {
    /// Size as a multiple of the base UI font size.
    ///
    /// Chosen so that at the default 13pt and 100% scaling the shell draws at
    /// very nearly the sizes it always has (20/16/14/13/11.7 px), which keeps
    /// this a generalisation of the old literals rather than a restyle.
    #[must_use]
    pub fn ratio(self) -> f32 {
        match self {
            Self::Glyph => 1.5,
            Self::Heading => 1.25,
            Self::Item => 1.1,
            Self::Body => 1.0,
            Self::Caption => 0.9,
        }
    }
}

// ============================================================================
// Window Management
// ============================================================================

/// Unique window identifier (assigned by compositor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

/// Window state tracked by the window manager.
#[derive(Clone, Debug)]
pub struct ManagedWindow {
    pub id: WindowId,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub state: WindowState,
    pub desktop: u32,
    /// Whether this window has focus.
    pub focused: bool,
    /// Whether the window is visible (not minimized to taskbar).
    pub visible: bool,
    /// Process ID owning this window.
    pub pid: u32,
    /// Icon ID (index into icon registry).
    pub icon_id: u32,
    /// Z-order (higher = on top).
    pub z_order: u32,
    /// Where the window sat before it was maximized.
    ///
    /// Maximizing overwrites the window's geometry, so un-maximizing has
    /// nothing to go back to unless the old geometry was kept: without this a
    /// "restore" leaves the window exactly where maximizing put it, which looks
    /// like the button doing nothing.
    pub restored: Option<WindowGeometry>,
}

/// A window's position and size, in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl ManagedWindow {
    /// The window's outer rectangle, title bar included.
    ///
    /// The only part of a window's geometry that is the window's own: it comes
    /// from the compositor in physical pixels and is not the shell's to scale.
    /// Everything drawn *around* it is — see
    /// [`window_chrome`](DesktopShell::window_chrome).
    #[must_use]
    pub fn frame_rect(&self) -> Rect {
        Rect::new(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        )
    }
}

/// Every rectangle of one window's decorations, resolved together.
///
/// Resolved together, and by the shell rather than by the window, because the
/// sizes depend on the user's display scaling: a `ManagedWindow` knows where it
/// is but not how large a title bar this desktop draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowChrome {
    /// The whole window, decorations included.
    pub frame: Rect,
    /// The title bar, across the top of the frame.
    pub title_bar: Rect,
    /// What the client owns — everything below the title bar.
    pub content: Rect,
    /// The buttons, right to left.
    pub close: Rect,
    pub maximize: Rect,
    pub minimize: Rect,
}

/// Window state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowState {
    Normal,
    Maximized,
    Minimized,
    Fullscreen,
}

// ============================================================================
// Pointer input
// ============================================================================

/// What lies under a point on the screen.
///
/// Hit testing is separated from acting on the hit so that "where is the
/// pointer" can be asserted directly in tests, and so that the press, release
/// and scroll paths all agree about what a point belongs to instead of each
/// re-deriving it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The start button on the taskbar.
    StartButton,
    /// An application row of the open start menu, by index into
    /// [`start_menu_entries`](DesktopShell::start_menu_entries).
    StartMenuEntry(usize),
    /// The open start menu, but not one of its rows.
    StartMenuPanel,
    /// The power button at the foot of the open start menu.
    PowerButton,
    /// An entry of the open power menu, by index into
    /// [`power_menu_entries`](DesktopShell::power_menu_entries).
    PowerMenuEntry(usize),
    /// The open power menu, but not one of its rows.
    PowerMenuPanel,
    /// A window button on the taskbar, by index into
    /// [`visible_windows`](DesktopShell::visible_windows).
    TaskbarButton(usize),
    /// The taskbar panel, but not one of its controls.
    TaskbarPanel,
    /// The tray clock, which opens the calendar popup.
    Clock,
    /// A control of the open calendar popup — including
    /// [`calendar::CalendarHit::Panel`], which is the popup's own inert space
    /// and must **not** dismiss it. A point off the popup is not this variant
    /// at all, which is how the two are told apart.
    CalendarControl(calendar::CalendarHit),
    /// A title-bar button.
    WindowClose(WindowId),
    WindowMaximize(WindowId),
    WindowMinimize(WindowId),
    /// A window's title bar, away from its buttons.
    WindowTitleBar(WindowId),
    /// A window's content area, which belongs to the client, not the shell.
    WindowContent(WindowId),
    /// Bare desktop.
    Desktop,
}

impl Hit {
    /// Whether the shell owns this pixel.
    ///
    /// Only the client's own content and the bare desktop are not the shell's;
    /// everything the shell draws it also consumes clicks on.
    #[must_use]
    pub fn is_shell_chrome(self) -> bool {
        !matches!(self, Self::WindowContent(_) | Self::Desktop)
    }
}

/// What the shell wants its host — the compositor's event loop — to do about a
/// pointer event.
///
/// The shell cannot start a process itself: it has no connection to the process
/// server, and inventing one here would put policy about *how* programs start
/// inside the window manager. It reports the intent instead, exactly as
/// [`launcher::LauncherAction`] already does for the search dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellAction {
    /// The shell did not want this event; deliver it to the window under the
    /// pointer. Focus may still have changed — click-to-focus raises a window
    /// *and* lets the click through to it, which is what makes the first click
    /// on an unfocused window press the button it landed on.
    Pass,
    /// The shell handled the event; no window should see it.
    Consumed,
    /// Start the program at this path. Implies [`Consumed`](Self::Consumed).
    Launch(String),
}

/// A left-button press at a point — the first event a click delivers.
#[must_use]
pub fn click(x: f32, y: f32) -> MouseEvent {
    MouseEvent {
        x,
        y,
        kind: MouseEventKind::Press(MouseButton::Left),
    }
}

/// A wheel event at a point, `dy` positive towards the start of a list.
#[must_use]
pub fn scroll(x: f32, y: f32, dy: f32) -> MouseEvent {
    MouseEvent {
        x,
        y,
        kind: MouseEventKind::Scroll { dx: 0.0, dy },
    }
}

// ============================================================================
// Desktop state
// ============================================================================

/// Complete desktop shell state.
pub struct DesktopShell {
    /// All managed windows.
    pub windows: BTreeMap<WindowId, ManagedWindow>,
    /// Currently focused window.
    pub focused_window: Option<WindowId>,
    /// Current virtual desktop index (0-based).
    pub current_desktop: u32,
    /// Number of virtual desktops.
    pub num_desktops: u32,
    /// Screen dimensions.
    pub screen_width: u32,
    pub screen_height: u32,
    /// Taskbar height.
    pub taskbar_height: u32,
    /// Whether the start menu is open.
    pub start_menu_open: bool,
    /// Index of the first application row the start menu shows.
    ///
    /// The menu is shorter than the application list, and a list that silently
    /// stops at the eighth program makes the ninth unreachable rather than
    /// merely unseen.
    pub start_menu_scroll: usize,
    /// Fractions of a row left over from previous wheel events over the menu.
    ///
    /// Reset when the menu closes, alongside the offset itself: a fraction
    /// earned scrolling one session of the menu must not deliver a row to the
    /// next one, which would jump the list the instant it opened.
    start_menu_wheel: wheel::Accumulator,
    /// Whether the power menu is showing.
    ///
    /// Only ever true while [`start_menu_open`](Self::start_menu_open) is: it
    /// is a submenu of the start menu and rises from a button inside it, so a
    /// power menu left over a closed start menu would be a popup with nothing
    /// to have opened it. [`close_start_menu`](Self::close_start_menu) is what
    /// keeps the two in step.
    pub power_menu_open: bool,
    /// The programs this desktop can start, shared with the search launcher so
    /// that the two front ends cannot offer different applications.
    pub apps: Vec<AppEntry>,
    /// Whether Alt+Tab switcher is active.
    pub alt_tab_active: bool,
    /// Alt+Tab selection index.
    pub alt_tab_index: usize,
    /// What the user chose in the appearance panel.
    ///
    /// Kept whole rather than reduced to [`theme`](Self::theme) because it
    /// carries more than colours — font sizes, DPI scaling, animation speed
    /// and corner radius all live here, and each is read by a different part
    /// of the shell.
    pub appearance: AppearanceSettings,
    /// Theme configuration, derived from [`appearance`](Self::appearance).
    ///
    /// Never assign to this directly: it would disagree with `appearance` at
    /// the next thing that re-derives it. Go through
    /// [`set_appearance`](Self::set_appearance).
    pub theme: DesktopTheme,
    /// Next Z-order value.
    next_z: u32,
    /// Next window ID (for local tracking; compositor assigns real IDs).
    next_window_id: u64,
    /// What the user chose in the Date & Time panel.
    ///
    /// Held whole, for the same reason [`appearance`](Self::appearance) is:
    /// the taskbar clock reads the zone *and* the three `show_*` flags, whose
    /// doc comments in [`datetime_settings::DateTimeSettings`] each say "in
    /// the taskbar clock" — and until this field existed, none of them reached
    /// one. See [`current_clock_string`](Self::current_clock_string).
    pub datetime: datetime_settings::DateTimeSettings,
    /// The calendar popup the tray clock opens.
    ///
    /// `calendar.rs` used to be reachable only through `mod calendar;`: it had
    /// a month grid, a year overview, an event store and a reminder manager,
    /// all tested, and no surface at all — nothing in the shell ever built a
    /// `CalendarView`, so the clock was not clickable. `visible` on the view
    /// **is** the open flag; a second `calendar_open: bool` here would be one
    /// missed assignment away from a popup that is drawn and not clickable, or
    /// the reverse. See `design-decisions.md` §493.
    pub calendar: calendar::CalendarView,
    /// The events the popup marks and lists.
    ///
    /// Empty until something fills it. It lives on the shell rather than
    /// inside the view because a calendar *view* is a way of looking at events,
    /// not a place to keep them — the reminder path and any future agenda
    /// surface read the same store.
    pub events: calendar::EventStore,
    /// The shell's **one** snap implementation.
    ///
    /// `snap.rs` used to be dead code — `mod snap;` was its only reference —
    /// while the shell computed snapped geometry itself, in integers, with no
    /// gap and only halves. Two answers to one question, of which the user
    /// could see the less capable one. Every snapped rectangle now comes from
    /// here; see `known-issues.md`
    /// `C-TWO-SNAP-IMPLEMENTATIONS-WITH-DIFFERENT-GAP-POLICIES` and
    /// `design-decisions.md` §469.
    ///
    /// Its work area is **not** kept in sync by notification.
    /// [`screen_width`](Self::screen_width), `taskbar_height` and `appearance`
    /// are all public fields that anything may assign, and `work_area()`
    /// derives from all three, so an "update on change" scheme would be one
    /// forgotten call site away from tiling a screen size that no longer
    /// exists. [`sync_snap_area`](Self::sync_snap_area) re-seeds it at the top
    /// of every operation that reads geometry instead — cheap (a layout is at
    /// most eight rectangles) and impossible to forget in a way that compiles
    /// but is wrong.
    pub snap: snap::SnapManager,
}

/// Desktop visual theme — every colour the shell paints with.
///
/// This is a *resolved* palette, not a set of preferences: by the time a
/// colour is in here, the light/dark choice, the accent, and the transparency
/// level have all been folded in, and the render functions below do nothing
/// but read fields. That split is deliberate. A renderer that consulted
/// [`AppearanceSettings`] directly would re-derive the same colour at every
/// frame and, worse, would be free to derive it slightly differently in each
/// of the dozen places it is drawn.
///
/// Each surface carries its own foreground rather than sharing one. A single
/// `fg` is only correct while every surface has a similar brightness, which
/// stops being true the moment "accent on the taskbar" turns one surface into
/// a saturated colour and leaves the others alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopTheme {
    /// The taskbar panel itself. Alpha may be below 255 — see
    /// [`taskbar_alpha`].
    pub taskbar_bg: Color,
    /// Text and icons on the taskbar.
    pub taskbar_fg: Color,
    /// The pressed/focused button background on the taskbar.
    pub taskbar_active_bg: Color,
    /// Accent-coloured marks on the taskbar, such as the start glyph.
    ///
    /// Distinct from [`accent_color`](Self::accent_color) because when the
    /// taskbar *is* the accent colour, an accent-coloured glyph on it would be
    /// invisible; this field is then the contrasting colour instead.
    pub taskbar_accent: Color,
    pub window_border_color: Color,
    pub window_title_bg: Color,
    pub window_title_fg: Color,
    pub window_title_inactive_bg: Color,
    /// Title text on an unfocused window, whose background may differ from the
    /// focused one by more than a shade once accented title bars are on.
    pub window_title_inactive_fg: Color,
    pub desktop_bg: Color,
    /// The theme's accent, as drawn on ordinary surfaces.
    pub accent_color: Color,
    pub start_menu_bg: Color,
    pub start_menu_fg: Color,
    /// Floating overlays such as the Alt+Tab switcher.
    pub overlay_bg: Color,
    pub overlay_fg: Color,
    pub overlay_selected_bg: Color,
}

impl Default for DesktopTheme {
    fn default() -> Self {
        Self::dark()
    }
}

impl DesktopTheme {
    /// The dark palette (Catppuccin Mocha), before any setting is applied.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            taskbar_bg: Color::from_hex(0x1E1E2E),
            taskbar_fg: Color::from_hex(0xCDD6F4),
            taskbar_active_bg: Color::from_hex(0x45475A),
            taskbar_accent: Color::from_hex(0x89B4FA),
            window_border_color: Color::from_hex(0x585B70),
            window_title_bg: Color::from_hex(0x313244),
            window_title_fg: Color::from_hex(0xCDD6F4),
            window_title_inactive_bg: Color::from_hex(0x1E1E2E),
            window_title_inactive_fg: Color::from_hex(0xA6ADC8),
            desktop_bg: Color::from_hex(0x11111B),
            accent_color: Color::from_hex(0x89B4FA),
            start_menu_bg: Color::from_hex(0x1E1E2E),
            start_menu_fg: Color::from_hex(0xCDD6F4),
            overlay_bg: Color::from_hex(0x1E1E2E),
            overlay_fg: Color::from_hex(0xCDD6F4),
            overlay_selected_bg: Color::from_hex(0x45475A),
        }
    }

    /// The light palette (Catppuccin Latte), before any setting is applied.
    ///
    /// Surface for surface, this is the same structure as [`dark`](Self::dark)
    /// — base, surface0, surface1, surface2, crust — so that a setting applied
    /// on top lands on the same role in either mode.
    #[must_use]
    pub fn light() -> Self {
        Self {
            taskbar_bg: Color::from_hex(0xEFF1F5),
            taskbar_fg: Color::from_hex(0x4C4F69),
            taskbar_active_bg: Color::from_hex(0xBCC0CC),
            taskbar_accent: Color::from_hex(0x1E66F5),
            window_border_color: Color::from_hex(0xACB0BE),
            window_title_bg: Color::from_hex(0xCCD0DA),
            window_title_fg: Color::from_hex(0x4C4F69),
            window_title_inactive_bg: Color::from_hex(0xEFF1F5),
            window_title_inactive_fg: Color::from_hex(0x6C6F85),
            desktop_bg: Color::from_hex(0xDCE0E8),
            accent_color: Color::from_hex(0x1E66F5),
            start_menu_bg: Color::from_hex(0xEFF1F5),
            start_menu_fg: Color::from_hex(0x4C4F69),
            overlay_bg: Color::from_hex(0xEFF1F5),
            overlay_fg: Color::from_hex(0x4C4F69),
            overlay_selected_bg: Color::from_hex(0xBCC0CC),
        }
    }

    /// Resolve a full palette from what the user chose.
    ///
    /// The order matters and is the same one the settings panel presents:
    /// pick the base palette from the mode, recolour what the accent options
    /// claim, then apply transparency last — alpha is a property of a surface
    /// that has already been given its colour.
    #[must_use]
    pub fn from_settings(settings: &AppearanceSettings) -> Self {
        let mut theme = if settings.theme_mode.is_light() {
            Self::light()
        } else {
            Self::dark()
        };

        let accent = settings.effective_accent();
        theme.accent_color = accent;
        theme.taskbar_accent = accent;

        if settings.accent_taskbar {
            theme.taskbar_bg = accent;
            theme.taskbar_fg = readable_on(accent);
            theme.taskbar_active_bg = emphasized(accent);
            // The start glyph is drawn in the accent; on an accent-coloured
            // panel it has to become the contrasting colour or it disappears.
            theme.taskbar_accent = readable_on(accent);
        }

        if settings.accent_titlebars {
            theme.window_title_bg = accent;
            theme.window_title_fg = readable_on(accent);
            // The *inactive* bar deliberately keeps the base palette: an
            // accent that marks every window marks none of them, and telling
            // the focused window apart is the title bar's first job.
        }

        theme.taskbar_bg = with_alpha(theme.taskbar_bg, taskbar_alpha(settings));
        let overlay = settings.transparency.panel_alpha();
        theme.overlay_bg = with_alpha(theme.overlay_bg, overlay);
        theme.start_menu_bg = with_alpha(theme.start_menu_bg, overlay);

        theme
    }
}

/// Black-ish or white-ish, whichever can be read on `bg`.
///
/// The endpoints are the palettes' own extremes rather than pure `#000`/`#fff`
/// so that accented surfaces still look like part of this desktop. Perceived
/// brightness uses the usual luma weights: the eye is far more sensitive to
/// green than to blue, so an average of the channels would call a saturated
/// blue "bright" and put black text on it.
fn readable_on(bg: Color) -> Color {
    let luma = 0.299 * f32::from(bg.r) + 0.587 * f32::from(bg.g) + 0.114 * f32::from(bg.b);
    if luma > 140.0 {
        Color::from_hex(0x11111B)
    } else {
        Color::from_hex(0xEFF1F5)
    }
}

/// A visibly different shade of `color`, for the pressed state of a control
/// whose resting state is already `color`.
///
/// Moves away from whichever extreme `color` is nearer, so the emphasis is
/// visible on both a pale and a deep accent instead of vanishing at one end.
fn emphasized(color: Color) -> Color {
    let toward = readable_on(color);
    color.lerp(toward, 0.25)
}

/// `color` at a given opacity.
fn with_alpha(color: Color, alpha: u8) -> Color {
    Color::rgba(color.r, color.g, color.b, alpha)
}

/// How opaque the taskbar's own panel is.
///
/// Two settings meet here, and they are not redundant. `transparency` is the
/// master level and carries a documented *scope*: `Subtle` reaches "overlays
/// and popups only", so it deliberately leaves the taskbar solid, while
/// `Moderate` and `Full` include it. `taskbar_style` then speaks for the
/// taskbar specifically — `Solid` opts out even when the master level is up,
/// and `Transparent` asks for the panel to disappear entirely and leave the
/// buttons floating.
///
/// Turning transparency `Off` wins over everything, because that is what an
/// off switch means; a "transparent" taskbar style would otherwise keep a
/// transparency effect alive after the user disabled transparency.
fn taskbar_alpha(settings: &AppearanceSettings) -> u8 {
    if !settings.transparency_enabled() {
        return 255;
    }
    match settings.taskbar_style {
        TaskbarStyle::Solid => 255,
        TaskbarStyle::Transparent => 0,
        TaskbarStyle::Translucent => match settings.transparency {
            TransparencyLevel::Off | TransparencyLevel::Subtle => 255,
            level => level.panel_alpha(),
        },
    }
}

/// Round a snap module coordinate to the integer geometry windows carry.
///
/// `as` on a float is a *saturating* cast in Rust — out-of-range values clamp
/// to the bound and NaN becomes 0 — so this cannot wrap a 4000-pixel screen
/// into a negative coordinate the way the equivalent C would. Rounding rather
/// than truncating matters at the seam: a zone starting at `x = 966.5` next to
/// one ending there must not both truncate to 966 and leave a column drawn
/// twice.
fn round_to_i32(v: f32) -> i32 {
    v.round() as i32
}

/// As [`round_to_i32`], for a width or height. Negative rounds to 0.
fn round_to_u32(v: f32) -> u32 {
    v.round().max(0.0) as u32
}

/// Convert a snap zone's floating-point rectangle to the integer geometry a
/// window carries, by rounding its **edges** rather than its origin and its
/// extent independently.
///
/// Rounding the two separately is what the obvious `(round(x), round(w))`
/// does, and it is wrong exactly where it matters. On a 1921-pixel screen the
/// right half sits at `x = 963.5` with `width = 957.5`; rounded separately
/// that is 964 wide 958, an edge at **1922** — one column past the screen the
/// zone was supposed to tile. Rounding the edges gives `964..1921`, flush,
/// because the right edge is rounded as the coordinate it actually is. The
/// same argument applies to two adjacent zones: their shared edge is one
/// number, so rounding it once makes them meet by construction instead of by
/// arithmetic luck.
fn round_rect(x: f32, y: f32, width: f32, height: f32) -> (i32, i32, u32, u32) {
    let x0 = round_to_i32(x);
    let y0 = round_to_i32(y);
    let x1 = round_to_i32(x + width);
    let y1 = round_to_i32(y + height);
    (
        x0,
        y0,
        u32::try_from(x1.saturating_sub(x0)).unwrap_or(0),
        u32::try_from(y1.saturating_sub(y0)).unwrap_or(0),
    )
}

impl DesktopShell {
    pub fn new(screen_width: u32, screen_height: u32) -> Self {
        let mut shell = Self {
            windows: BTreeMap::new(),
            focused_window: None,
            current_desktop: 0,
            num_desktops: 4,
            screen_width,
            screen_height,
            taskbar_height: 40,
            start_menu_open: false,
            start_menu_scroll: 0,
            start_menu_wheel: wheel::Accumulator::default(),
            power_menu_open: false,
            apps: launcher::builtin_app_database(),
            alt_tab_active: false,
            alt_tab_index: 0,
            appearance: AppearanceSettings::default(),
            theme: DesktopTheme::default(),
            next_z: 1,
            next_window_id: 1,
            datetime: datetime_settings::DateTimeSettings::default(),
            calendar: calendar::CalendarView::new(calendar::CalendarConfig::default()),
            events: calendar::EventStore::new(),
            // Placeholder: the real area needs `taskbar_rect()`, which needs
            // the appearance scaling that is only set two fields up. Seeded
            // immediately below rather than left to the first snap, so that a
            // caller reading `shell.snap.layout()` before ever snapping gets
            // the screen it is actually on.
            snap: snap::SnapManager::new(snap::WorkArea::whole_screen(0.0, 0.0)),
        };
        shell.sync_snap_area();
        shell
    }

    /// The work area as the snap module wants it.
    fn snap_area(&self) -> snap::WorkArea {
        let (x, y, width, height) = self.work_area();
        snap::WorkArea::new(x as f32, y as f32, width as f32, height as f32)
    }

    /// Re-seed the snap manager's work area from the shell's current geometry.
    ///
    /// Called at the top of every snap operation. See the field's doc for why
    /// this is pull-on-use rather than push-on-change.
    fn sync_snap_area(&mut self) {
        let area = self.snap_area();
        if self.snap.work_area() != area {
            self.snap.set_work_area(area);
        }
    }

    /// Adopt a new set of appearance settings and repaint from them.
    ///
    /// The single door through which the palette changes, so that `theme` can
    /// never be a stale derivation of some earlier `appearance`.
    ///
    /// Also publishes the display scaling to the toolkit, which keeps its own
    /// process-wide scale for the widgets it draws. Without this a `guitk`
    /// widget hosted in the shell — a dialog, a menu — would lay itself out at
    /// 100% inside chrome drawn at 200%. The shell is the right one to publish
    /// it: it is the process that reads the user's setting.
    ///
    /// The shell still scales its *own* geometry from `self.appearance` rather
    /// than reading the value back out of the toolkit. Geometry that depended
    /// on process-global mutable state would be geometry that two shells (or
    /// two tests) in one process could not disagree about safely — and that
    /// same property is why the publish itself has no unit test: every test
    /// that builds a shell would be writing the one value an assertion could
    /// read. It is one line, and it is here rather than at the call sites so
    /// that a later appearance change cannot forget it.
    pub fn set_appearance(&mut self, appearance: AppearanceSettings) {
        self.theme = DesktopTheme::from_settings(&appearance);
        guitk::scaling::set_global_scale(appearance.scale_factor());
        self.appearance = appearance;
    }

    /// Load the user's saved appearance settings from disk and apply them.
    ///
    /// Kept out of [`new`](Self::new) on purpose. A constructor that reads the
    /// user's home directory gives every caller — including every test — a
    /// result that depends on the machine it runs on, and a shell that cannot
    /// be built without a filesystem. Startup calls this; tests that care
    /// about a particular look call [`set_appearance`](Self::set_appearance)
    /// with settings they built themselves.
    ///
    /// A missing or unreadable file is not an error: [`config::load`] yields
    /// an empty document, which reads back as the defaults.
    pub fn load_appearance(&mut self) {
        let doc = config::load(appearance_settings::CONFIG_NAME);
        self.set_appearance(AppearanceSettings::read_from(&doc));
    }

    /// Usable area for windows (excluding taskbar).
    ///
    /// The taskbar is subtracted at its drawn thickness, not its logical
    /// height: a maximized window that stopped at the unscaled height would sit
    /// under the bar on any display scaled above 100%.
    pub fn work_area(&self) -> (i32, i32, u32, u32) {
        let bar = self.taskbar_rect();
        (0, 0, self.screen_width, bar.y.max(0.0) as u32)
    }

    // ======================================================================
    // Chrome geometry
    //
    // Every rectangle the shell draws or clicks comes from here. See [`Rect`].
    //
    // The constants above are *logical* pixels — the size the chrome would be
    // on a 100%-scaling display — and every one of them passes through
    // [`scale`](Self::scale) on the way out. Only measurements that come from
    // the compositor (the screen size, a window's frame) are already physical
    // and must not be scaled a second time.
    // ======================================================================

    /// A logical length in physical pixels, at the user's display scaling.
    #[must_use]
    pub fn scale(&self, logical: f32) -> f32 {
        logical * self.appearance.scale_factor()
    }

    /// The size to draw text of a given role at, in physical pixels.
    #[must_use]
    pub fn font_size(&self, role: TextRole) -> f32 {
        self.scale(self.appearance.fonts.ui_size * role.ratio())
    }

    /// The corner rounding the user asked for, in physical pixels.
    ///
    /// Scaled like every other length: a 8px radius that stayed 8px at 200%
    /// would look like a sharper corner on the larger chrome, not the same one.
    #[must_use]
    pub fn corner_radii(&self) -> CornerRadii {
        CornerRadii::all(self.scale(self.appearance.window_corners.radius()))
    }

    /// [`corner_radii`](Self::corner_radii), rounded across the top only — for
    /// a title bar, which shares its lower edge with the client area.
    #[must_use]
    pub fn top_corner_radii(&self) -> CornerRadii {
        CornerRadii::top(self.scale(self.appearance.window_corners.radius()))
    }

    /// The taskbar panel.
    #[must_use]
    pub fn taskbar_rect(&self) -> Rect {
        let height = self.taskbar_thickness();
        Rect::new(
            0.0,
            (self.screen_height as f32 - height).max(0.0),
            self.screen_width as f32,
            height.min(self.screen_height as f32),
        )
    }

    /// How thick the taskbar is on screen.
    ///
    /// [`taskbar_height`](Self::taskbar_height) is a logical height — what the
    /// user (or the taskbar settings panel) asked for — so the bar grows with
    /// the display scaling like the rest of the chrome.
    #[must_use]
    pub fn taskbar_thickness(&self) -> f32 {
        self.scale(self.taskbar_height as f32)
    }

    /// The start button at the left end of the taskbar.
    #[must_use]
    pub fn start_button_rect(&self) -> Rect {
        let bar = self.taskbar_rect();
        Rect::new(
            bar.x,
            bar.y,
            self.scale(START_BUTTON_WIDTH).min(bar.w),
            bar.h,
        )
    }

    /// Where the system tray begins — the left edge of its leftmost item.
    #[must_use]
    pub fn tray_x(&self) -> f32 {
        (self.taskbar_rect().w - self.tray_width()).max(0.0)
    }

    /// How wide each taskbar window button is.
    ///
    /// The buttons shrink as windows are opened, so this cannot be a constant
    /// in either the renderer or the hit test.
    fn taskbar_button_width(&self) -> f32 {
        let bar = self.taskbar_rect();
        let available = (bar.w
            - self.scale(START_BUTTON_WIDTH)
            - self.tray_width()
            - self.scale(TRAY_RESERVE_GAP))
        .max(0.0);
        let count = self.visible_windows().len().max(1) as f32;
        self.scale(TASKBAR_BUTTON_MAX_WIDTH).min(available / count)
    }

    /// The taskbar button for the `index`-th visible window.
    #[must_use]
    pub fn taskbar_button_rect(&self, index: usize) -> Rect {
        let bar = self.taskbar_rect();
        let w = self.taskbar_button_width();
        let inset = self.scale(TASKBAR_BUTTON_INSET).min(bar.h / 2.0);
        let x = bar.x
            + self.scale(START_BUTTON_WIDTH)
            + self.scale(TASKBAR_BUTTON_START_GAP)
            + index as f32 * (w + self.scale(TASKBAR_BUTTON_GAP));
        Rect::new(x, bar.y + inset, w, bar.h - inset * 2.0)
    }

    /// The start menu panel, anchored to the start button's corner.
    ///
    /// Clamped to the room above the taskbar and to the screen's width. At 200%
    /// scaling the menu's nominal height is 800px, which on an 800px screen
    /// would start above the top edge — and a row drawn off-screen cannot be
    /// clicked at all, which is the same unreachable-program bug that
    /// [`start_menu_scroll`](Self::start_menu_scroll) exists to prevent. A
    /// shorter menu simply shows fewer rows and scrolls for the rest.
    #[must_use]
    pub fn start_menu_rect(&self) -> Rect {
        let bar = self.taskbar_rect();
        let h = self.scale(START_MENU_HEIGHT).min(bar.y.max(0.0));
        let w = self
            .scale(START_MENU_WIDTH)
            .min(self.screen_width as f32)
            .max(0.0);
        Rect::new(0.0, bar.y - h, w, h)
    }

    /// How many application rows fit between the heading and the power options.
    ///
    /// Scale-invariant: it divides one scaled length by another, so the same
    /// programs are on screen at 200% as at 100% — they are simply larger.
    #[must_use]
    pub fn start_menu_visible_rows(&self) -> usize {
        let usable = self.start_menu_rect().h
            - self.scale(START_MENU_TOP_PADDING)
            - self.scale(START_MENU_FOOTER);
        let row = self.scale(START_MENU_ROW_HEIGHT);
        if row <= 0.0 || usable < row {
            return 0;
        }
        (usable / row) as usize
    }

    /// The `row`-th drawn row of the start menu.
    ///
    /// `row` counts from the top of the visible list, so it is the index of the
    /// entry at `start_menu_scroll + row` — the renderer and the hit test agree
    /// about that offset because both go through
    /// [`start_menu_entry_at`](Self::start_menu_entry_at).
    #[must_use]
    pub fn start_menu_row_rect(&self, row: usize) -> Rect {
        let menu = self.start_menu_rect();
        let height = self.scale(START_MENU_ROW_HEIGHT);
        Rect::new(
            menu.x,
            menu.y + self.scale(START_MENU_TOP_PADDING) + row as f32 * height,
            menu.w,
            height,
        )
    }

    /// The power button in the start menu's footer, which opens the power menu.
    ///
    /// The footer is the last [`START_MENU_FOOTER`] of the menu, or the whole
    /// menu if the menu has been clamped shorter than that — a button drawn
    /// above the menu's own top edge would be as unreachable as a row drawn off
    /// the screen.
    #[must_use]
    pub fn power_button_rect(&self) -> Rect {
        let menu = self.start_menu_rect();
        let inset = self.scale(POWER_BUTTON_INSET);
        let footer = self.scale(START_MENU_FOOTER).min(menu.h);
        let h = (footer - inset * 2.0).max(0.0);
        let w = self
            .scale(POWER_BUTTON_WIDTH)
            .min((menu.w - inset * 2.0).max(0.0));
        Rect::new(menu.x + inset, menu.y + menu.h - footer + inset, w, h)
    }

    /// The power menu popup, rising from the power button.
    ///
    /// A submenu is allowed to cover the menu it opened from, so when there is
    /// not enough room above the button the popup slides down over the
    /// application list rather than shrinking: losing "Shutdown" off the top of
    /// the screen would defeat the whole point of the menu. It is clamped to
    /// the screen's height only because a popup taller than the display has
    /// nowhere left to go — see
    /// [`power_menu_visible_rows`](Self::power_menu_visible_rows).
    #[must_use]
    pub fn power_menu_rect(&self) -> Rect {
        let button = self.power_button_rect();
        let rows = self.power_menu_entries().len() as f32;
        let pad = self.scale(POWER_MENU_PADDING);
        let h =
            (rows * self.scale(POWER_MENU_ROW_HEIGHT) + pad * 2.0).min(self.screen_height as f32);
        let w = self
            .scale(POWER_MENU_WIDTH)
            .min(self.screen_width as f32)
            .max(0.0);
        let y = (button.y - self.scale(POWER_MENU_GAP) - h).max(0.0);
        Rect::new(button.x, y, w, h)
    }

    /// How many popup rows fit, which is every entry unless the popup had to be
    /// clamped to a screen shorter than itself.
    #[must_use]
    pub fn power_menu_visible_rows(&self) -> usize {
        let row = self.scale(POWER_MENU_ROW_HEIGHT);
        if row <= 0.0 {
            return 0;
        }
        let usable = self.power_menu_rect().h - self.scale(POWER_MENU_PADDING) * 2.0;
        ((usable / row).max(0.0) as usize).min(self.power_menu_entries().len())
    }

    /// The `row`-th drawn row of the power menu.
    #[must_use]
    pub fn power_menu_row_rect(&self, row: usize) -> Rect {
        let menu = self.power_menu_rect();
        let height = self.scale(POWER_MENU_ROW_HEIGHT);
        Rect::new(
            menu.x,
            menu.y + self.scale(POWER_MENU_PADDING) + row as f32 * height,
            menu.w,
            height,
        )
    }

    /// The system actions the power menu lists, in menu order.
    ///
    /// Exactly the entries [`start_menu_entries`](Self::start_menu_entries)
    /// leaves out, from the same database, so a system action can never be in
    /// both lists or in neither.
    #[must_use]
    pub fn power_menu_entries(&self) -> Vec<&AppEntry> {
        self.apps
            .iter()
            .filter(|app| matches!(app.category, Category::System))
            .collect()
    }

    /// Open or close the power menu.
    pub fn toggle_power_menu(&mut self) {
        self.power_menu_open = !self.power_menu_open;
    }

    /// Every rectangle of one window's decorations.
    ///
    /// The shell's, not the window's, because the sizes depend on the display
    /// scaling — see [`WindowChrome`].
    #[must_use]
    pub fn window_chrome(&self, window: &ManagedWindow) -> WindowChrome {
        let frame = window.frame_rect();

        // A window shorter than the title bar it would be given gets a title
        // bar the height of the window and an empty content area, rather than
        // a content rect of negative height that `contains` would answer for
        // no point at all — or worse, that a renderer would draw inverted.
        let bar_h = self.scale(TITLE_BAR_HEIGHT).min(frame.h);
        let title_bar = Rect::new(frame.x, frame.y, frame.w, bar_h);
        let content = Rect::new(frame.x, frame.y + bar_h, frame.w, frame.h - bar_h);

        let size = self.scale(WINDOW_BUTTON_SIZE);
        let pitch = self.scale(WINDOW_BUTTON_PITCH);
        let button_y = frame.y + self.scale(WINDOW_BUTTON_MARGIN_TOP);
        let close_x = frame.x + frame.w - self.scale(WINDOW_BUTTON_MARGIN_RIGHT);
        let button = |slot: f32| Rect::new(close_x - slot * pitch, button_y, size, size);

        WindowChrome {
            frame,
            title_bar,
            content,
            close: button(0.0),
            maximize: button(1.0),
            minimize: button(2.0),
        }
    }

    /// The programs the start menu lists, in menu order.
    ///
    /// System actions — shutdown, lock, log out — are deliberately excluded:
    /// they belong to the power options at the foot of the menu, not among the
    /// applications, and mixing them in would make "Shutdown" one mis-click
    /// away from "Screenshot".
    #[must_use]
    pub fn start_menu_entries(&self) -> Vec<&AppEntry> {
        self.apps
            .iter()
            .filter(|app| matches!(app.category, Category::Application | Category::Setting))
            .collect()
    }

    /// Which entry the `row`-th drawn row shows, if any.
    fn start_menu_entry_at(&self, row: usize) -> Option<usize> {
        let index = self.start_menu_scroll.checked_add(row)?;
        (index < self.start_menu_entries().len()).then_some(index)
    }

    /// The furthest the menu can scroll and still be full.
    fn start_menu_max_scroll(&self) -> usize {
        self.start_menu_entries()
            .len()
            .saturating_sub(self.start_menu_visible_rows())
    }

    /// Move the start menu's list by whole rows, positive meaning towards the
    /// *last* entry — the direction of the row index itself, which is the
    /// convention `guitk::wheel::Accumulator` and `guitk::scroll_window::shift`
    /// both use.
    ///
    /// This used to be the other way round, and said so in a doc comment that
    /// claimed to be matching `guitk` — it was not; it was the one place in the
    /// tree where a positive scroll delta moved towards row 0. A caller that
    /// believed the comment scrolled the menu backwards.
    pub fn scroll_start_menu(&mut self, rows: i32) {
        let max = self.start_menu_max_scroll();
        let moved = if rows >= 0 {
            self.start_menu_scroll
                .saturating_add(rows.unsigned_abs() as usize)
        } else {
            self.start_menu_scroll
                .saturating_sub(rows.unsigned_abs() as usize)
        };
        self.start_menu_scroll = moved.min(max);
    }

    /// Open or close the start menu.
    ///
    /// Opening rewinds the list: a menu that reopens where it was last left
    /// hides the first application from a user who has no idea it scrolled.
    pub fn toggle_start_menu(&mut self) {
        if self.start_menu_open {
            self.close_start_menu();
        } else {
            self.start_menu_open = true;
            self.start_menu_scroll = 0;
            // The offset is being rewound, so the fraction that was pushing it
            // must be rewound too — otherwise a menu opened just after a
            // part-notch scroll steps off row 0 on the next small delta.
            self.start_menu_wheel.reset();
        }
    }

    /// Close the start menu, and the power menu with it.
    ///
    /// The single place the menu closes, so that the submenu cannot be left
    /// open over an empty desktop. Clearing `start_menu_open` directly is what
    /// would strand it.
    pub fn close_start_menu(&mut self) {
        self.start_menu_open = false;
        self.power_menu_open = false;
    }

    // ======================================================================
    // Pointer input
    // ======================================================================

    /// What is under a point, topmost surface first.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32) -> Hit {
        // The power menu is tested first because it is drawn last: it rises
        // over the start menu's own rows, and a point inside both belongs to
        // the surface on top.
        if self.power_menu_open {
            let menu = self.power_menu_rect();
            if menu.contains(x, y) {
                for row in 0..self.power_menu_visible_rows() {
                    if self.power_menu_row_rect(row).contains(x, y) {
                        return Hit::PowerMenuEntry(row);
                    }
                }
                return Hit::PowerMenuPanel;
            }
        }

        if self.start_menu_open {
            let menu = self.start_menu_rect();
            if self.power_button_rect().contains(x, y) {
                return Hit::PowerButton;
            }
            if menu.contains(x, y) {
                for row in 0..self.start_menu_visible_rows() {
                    if self.start_menu_row_rect(row).contains(x, y) {
                        return match self.start_menu_entry_at(row) {
                            Some(index) => Hit::StartMenuEntry(index),
                            // A drawn-but-empty row past the end of the list is
                            // still the menu, not what lies behind it.
                            None => Hit::StartMenuPanel,
                        };
                    }
                }
                return Hit::StartMenuPanel;
            }
        }

        // The calendar popup, tested against the same layout it was drawn
        // from. `hit_test` returning `None` means the point is not on the
        // popup at all, which falls through to whatever is behind it.
        if self.calendar.visible {
            let (cx, cy) = self.calendar_origin();
            if let Some(hit) =
                self.calendar
                    .hit_test(cx, cy, self.calendar_scale(), x, y, &self.events)
            {
                return Hit::CalendarControl(hit);
            }
        }

        if self.start_button_rect().contains(x, y) {
            return Hit::StartButton;
        }

        if self.taskbar_rect().contains(x, y) {
            // Before the window buttons: the tray is at the far end and the
            // buttons never reach it (`taskbar_button_width` subtracts the
            // tray), but the order is what makes that a fact rather than a
            // coincidence the two could stop sharing.
            if self.clock_rect().contains(x, y) {
                return Hit::Clock;
            }
            for index in 0..self.visible_windows().len() {
                if self.taskbar_button_rect(index).contains(x, y) {
                    return Hit::TaskbarButton(index);
                }
            }
            return Hit::TaskbarPanel;
        }

        // `visible_windows` is sorted bottom-to-top, and the topmost window is
        // the one that receives the click.
        for window in self.visible_windows().into_iter().rev() {
            let chrome = self.window_chrome(window);
            if !chrome.frame.contains(x, y) {
                continue;
            }
            if chrome.close.contains(x, y) {
                return Hit::WindowClose(window.id);
            }
            if chrome.maximize.contains(x, y) {
                return Hit::WindowMaximize(window.id);
            }
            if chrome.minimize.contains(x, y) {
                return Hit::WindowMinimize(window.id);
            }
            if chrome.title_bar.contains(x, y) {
                return Hit::WindowTitleBar(window.id);
            }
            return Hit::WindowContent(window.id);
        }

        Hit::Desktop
    }

    /// Handle a pointer event.
    ///
    /// Returns what the caller should do with it — see [`ShellAction`].
    pub fn handle_mouse(&mut self, event: &MouseEvent) -> ShellAction {
        match event.kind {
            MouseEventKind::Press(button) => self.handle_press(event.x, event.y, button, false),
            MouseEventKind::DoubleClick(button) => {
                self.handle_press(event.x, event.y, button, true)
            }
            MouseEventKind::Scroll { dy, .. } => self.handle_scroll(event.x, event.y, dy),
            // A release belongs to whoever took the press, so chrome swallows
            // it: a client that saw a release it had no press for would treat a
            // click on the title bar as a click on itself.
            MouseEventKind::Release(_) => {
                if self.hit_test(event.x, event.y).is_shell_chrome() {
                    ShellAction::Consumed
                } else {
                    ShellAction::Pass
                }
            }
            // Motion is not the shell's until it grows window dragging; until
            // then forwarding it is what keeps hover states alive in clients.
            MouseEventKind::Move | MouseEventKind::Enter | MouseEventKind::Leave => {
                ShellAction::Pass
            }
        }
    }

    /// Whether a click here is part of the start menu rather than outside it.
    ///
    /// The start button counts because clicking it while the menu is open is
    /// how the menu is closed again, and that path has to reach the toggle
    /// rather than the dismiss-on-click-outside rule above it.
    fn keeps_start_menu_open(hit: Hit) -> bool {
        matches!(
            hit,
            Hit::StartButton
                | Hit::StartMenuEntry(_)
                | Hit::StartMenuPanel
                | Hit::PowerButton
                | Hit::PowerMenuEntry(_)
                | Hit::PowerMenuPanel
        )
    }

    fn handle_press(&mut self, x: f32, y: f32, button: MouseButton, double: bool) -> ShellAction {
        let hit = self.hit_test(x, y);

        // A click anywhere outside an open menu dismisses it, and is spent
        // doing so rather than also reaching what it landed on. Dismissing is
        // what the user aimed at; acting as well would make the click do
        // something they could not see coming.
        //
        // The submenu is dismissed first and on its own: a click that lands on
        // the application list while the power menu is open closes the power
        // menu without also launching the program underneath it.
        if self.power_menu_open && !matches!(hit, Hit::PowerMenuEntry(_) | Hit::PowerMenuPanel) {
            self.power_menu_open = false;
            if !Self::keeps_start_menu_open(hit) {
                self.start_menu_open = false;
            }
            return ShellAction::Consumed;
        }

        if self.start_menu_open && !Self::keeps_start_menu_open(hit) {
            self.close_start_menu();
            return ShellAction::Consumed;
        }

        // Same rule for the calendar. `Hit::CalendarControl` covers the
        // popup's inert space as well as its controls, so a click in its own
        // margin does not close it — which is the single most irritating way
        // for a popup to behave — while a click anywhere off it does.
        if self.calendar.visible && !matches!(hit, Hit::Clock | Hit::CalendarControl(_)) {
            self.calendar.set_visible(false);
            return ShellAction::Consumed;
        }

        // Only the primary button acts. The rest still cannot fall through to a
        // client when they land on the shell's own surfaces.
        if button != MouseButton::Left {
            return if hit.is_shell_chrome() {
                ShellAction::Consumed
            } else {
                ShellAction::Pass
            };
        }

        match hit {
            Hit::StartButton => {
                self.toggle_start_menu();
                ShellAction::Consumed
            }
            Hit::StartMenuEntry(index) => {
                let path = self
                    .start_menu_entries()
                    .get(index)
                    .map(|entry| entry.executable_path.clone());
                match path {
                    Some(path) => {
                        self.close_start_menu();
                        ShellAction::Launch(path)
                    }
                    None => ShellAction::Consumed,
                }
            }
            Hit::PowerButton => {
                self.toggle_power_menu();
                ShellAction::Consumed
            }
            // A system action starts a program like any other menu entry: the
            // shell has no more business shutting the machine down itself than
            // it has starting a text editor itself. `/sbin/shutdown` and its
            // neighbours are what actually do it.
            Hit::PowerMenuEntry(index) => {
                let path = self
                    .power_menu_entries()
                    .get(index)
                    .map(|entry| entry.executable_path.clone());
                match path {
                    Some(path) => {
                        self.close_start_menu();
                        ShellAction::Launch(path)
                    }
                    None => ShellAction::Consumed,
                }
            }
            Hit::Clock => {
                self.toggle_calendar();
                ShellAction::Consumed
            }
            Hit::CalendarControl(control) => {
                self.calendar.apply(control);
                ShellAction::Consumed
            }
            Hit::StartMenuPanel | Hit::PowerMenuPanel | Hit::TaskbarPanel => ShellAction::Consumed,
            Hit::TaskbarButton(index) => {
                let id = self.visible_windows().get(index).map(|w| w.id);
                if let Some(id) = id {
                    // The button of the window you are already looking at
                    // minimises it — the taskbar button is a toggle, not a
                    // second way to focus what is already focused.
                    if self.focused_window == Some(id) {
                        self.minimize_window(id);
                    } else {
                        self.focus_window(id);
                    }
                }
                ShellAction::Consumed
            }
            Hit::WindowClose(id) => {
                self.remove_window(id);
                ShellAction::Consumed
            }
            Hit::WindowMaximize(id) => {
                self.focus_window(id);
                self.toggle_maximize(id);
                ShellAction::Consumed
            }
            Hit::WindowMinimize(id) => {
                self.minimize_window(id);
                ShellAction::Consumed
            }
            Hit::WindowTitleBar(id) => {
                self.focus_window(id);
                if double {
                    self.toggle_maximize(id);
                }
                ShellAction::Consumed
            }
            Hit::WindowContent(id) => {
                self.focus_window(id);
                ShellAction::Pass
            }
            Hit::Desktop => ShellAction::Pass,
        }
    }

    fn handle_scroll(&mut self, x: f32, y: f32, dy: f32) -> ShellAction {
        // Asked of the hit test rather than of `start_menu_rect` directly, so
        // that a wheel over the power menu — which covers part of the list —
        // does not scroll the rows hidden behind it.
        if matches!(
            self.hit_test(x, y),
            Hit::StartMenuEntry(_) | Hit::StartMenuPanel | Hit::PowerButton
        ) {
            let rows = scroll_rows(&mut self.start_menu_wheel, dy);
            self.scroll_start_menu(rows);
            return ShellAction::Consumed;
        }
        if self.hit_test(x, y).is_shell_chrome() {
            return ShellAction::Consumed;
        }
        ShellAction::Pass
    }

    /// Maximize a window, or restore it if it already is.
    pub fn toggle_maximize(&mut self, id: WindowId) {
        let maximized = self
            .windows
            .get(&id)
            .is_some_and(|w| w.state == WindowState::Maximized);
        if maximized {
            self.restore_window(id);
        } else {
            self.maximize_window(id);
        }
    }

    // ======================================================================
    // Window management
    // ======================================================================

    /// The next window id, and never the same one twice.
    ///
    /// A repeated id would not merely confuse a caller: `windows` is keyed by
    /// it, so inserting the second window with a repeated id silently *evicts*
    /// the first. A `u64` bumped once per window cannot run out — at a billion
    /// windows a second it would take some six centuries — so this saturates
    /// rather than refusing, and says so instead of leaving the reader to work
    /// out whether the addition can wrap.
    fn take_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id = self.next_window_id.saturating_add(1);
        id
    }

    /// The next z-order, putting its holder above every existing window.
    ///
    /// Unlike a window id, this counter really can run out: it is bumped on
    /// every *focus change*, not every window, and it is only a `u32`. So
    /// rather than saturate — which would freeze the stacking order the moment
    /// it topped out, with every later window tied for topmost — it renumbers
    /// the existing windows and carries on. A z-order is only ever compared
    /// against another z-order, so renumbering is invisible from outside.
    fn take_z(&mut self) -> u32 {
        if self.next_z == u32::MAX {
            self.compact_z_order();
        }
        let z = self.next_z;
        self.next_z = self.next_z.saturating_add(1);
        z
    }

    /// Renumbers every window's z-order to `0..n`, preserving their order.
    fn compact_z_order(&mut self) {
        let mut ids: Vec<WindowId> = self.windows.keys().copied().collect();
        ids.sort_by_key(|id| self.windows.get(id).map_or(0, |w| w.z_order));
        self.next_z = 0;
        for id in ids {
            if let Some(w) = self.windows.get_mut(&id) {
                w.z_order = self.next_z;
                self.next_z = self.next_z.saturating_add(1);
            }
        }
    }

    /// Register a new window.
    pub fn add_window(
        &mut self,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        pid: u32,
    ) -> WindowId {
        let id = self.take_window_id();
        let z_order = self.take_z();

        let window = ManagedWindow {
            id,
            title: title.to_string(),
            x,
            y,
            width,
            height,
            state: WindowState::Normal,
            desktop: self.current_desktop,
            focused: false,
            visible: true,
            pid,
            icon_id: 0,
            z_order,
            restored: None,
        };

        self.windows.insert(id, window);
        self.focus_window(id);
        id
    }

    /// Remove a window.
    pub fn remove_window(&mut self, id: WindowId) {
        // Window ids are handed out by a counter that never repeats within a
        // session, so a stale entry would not be *mis*-applied to a later
        // window — but it would still accumulate for the lifetime of the shell,
        // one per snapped-then-closed window, which is a leak whatever it is
        // called.
        self.snap.history.remove(id.0);
        self.windows.remove(&id);
        if self.focused_window == Some(id) {
            // Focus the topmost remaining window
            self.focused_window = self.visible_windows().last().map(|w| w.id);
            if let Some(fid) = self.focused_window
                && let Some(w) = self.windows.get_mut(&fid)
            {
                w.focused = true;
            }
        }
    }

    /// Focus a window (bring to front).
    pub fn focus_window(&mut self, id: WindowId) {
        // Unfocus previous
        if let Some(prev) = self.focused_window
            && let Some(w) = self.windows.get_mut(&prev)
        {
            w.focused = false;
        }

        let z_order = self.take_z();
        if let Some(w) = self.windows.get_mut(&id) {
            w.focused = true;
            w.z_order = z_order;
            // Restore if minimized
            if w.state == WindowState::Minimized {
                w.state = WindowState::Normal;
                w.visible = true;
            }
        }

        self.focused_window = Some(id);
    }

    /// Minimize a window to the taskbar.
    pub fn minimize_window(&mut self, id: WindowId) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.state = WindowState::Minimized;
            w.visible = false;
            w.focused = false;
        }
        if self.focused_window == Some(id) {
            self.focused_window = None;
            // Focus next visible window
            if let Some(next) = self.visible_windows().last() {
                let next_id = next.id;
                self.focus_window(next_id);
            }
        }
    }

    /// Maximize a window to fill the work area.
    ///
    /// Remembers where the window was, so that restoring can put it back. Only
    /// the first maximize records it: maximizing an already-maximized window
    /// would otherwise record the maximized geometry as the one to return to.
    pub fn maximize_window(&mut self, id: WindowId) {
        let (wx, wy, ww, wh) = self.work_area();
        if let Some(w) = self.windows.get_mut(&id) {
            if w.state != WindowState::Maximized {
                w.restored = Some(WindowGeometry {
                    x: w.x,
                    y: w.y,
                    width: w.width,
                    height: w.height,
                });
            }
            w.state = WindowState::Maximized;
            w.x = wx;
            w.y = wy;
            w.width = ww;
            w.height = wh;
            w.visible = true;
        }
    }

    /// Restore a window to normal state, back where it was before it was
    /// maximized if that is known.
    pub fn restore_window(&mut self, id: WindowId) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.state = WindowState::Normal;
            w.visible = true;
            if let Some(geometry) = w.restored.take() {
                w.x = geometry.x;
                w.y = geometry.y;
                w.width = geometry.width;
                w.height = geometry.height;
            }
        }
    }

    /// Move a window.
    ///
    /// A move by any route — drag, keyboard, a program placing its own window
    /// — ends the window's tenancy of a snap zone. It is no longer in the zone
    /// it was put in, so keeping the entry would leave
    /// [`unsnap_window`](Self::unsnap_window) able to yank a window the user
    /// has since placed by hand back to a position it left minutes ago.
    /// [`snap_window_to_zone`](Self::snap_window_to_zone) sets the position
    /// directly rather than going through here, so this does not undo the snap
    /// it is part of.
    pub fn move_window(&mut self, id: WindowId, x: i32, y: i32) {
        self.snap.history.remove(id.0);
        if let Some(w) = self.windows.get_mut(&id) {
            w.x = x;
            w.y = y;
            if w.state == WindowState::Maximized {
                w.state = WindowState::Normal;
                // The user has just placed the window themselves; the geometry
                // it had before it was maximized is no longer where it should
                // spring back to.
                w.restored = None;
            }
        }
    }

    /// Resize a window.
    ///
    /// Ends a snap for the same reason [`move_window`](Self::move_window)
    /// does: a resized window no longer fills the zone it was snapped to.
    pub fn resize_window(&mut self, id: WindowId, width: u32, height: u32) {
        self.snap.history.remove(id.0);
        if let Some(w) = self.windows.get_mut(&id) {
            w.width = width;
            w.height = height;
            if w.state == WindowState::Maximized {
                w.state = WindowState::Normal;
                w.restored = None;
            }
        }
    }

    /// Snap window to left/right half of screen.
    ///
    /// A thin naming over [`snap_window_to_zone`](Self::snap_window_to_zone):
    /// the two halves are zones 0 and 1 of the `TwoEqualHalves` preset. The
    /// shell used to compute the two rectangles itself, which is what made
    /// `snap.rs` dead code and let the two disagree about whether snapped
    /// windows touch.
    pub fn snap_window(&mut self, id: WindowId, left: bool) {
        let zone = if left { 0 } else { 1 };
        self.snap_window_to_zone(id, snap::SnapLayoutPreset::TwoEqualHalves, zone);
    }

    /// Snap a window into one zone of a layout preset.
    ///
    /// Returns `false` if the window or the zone does not exist, in which case
    /// nothing is changed — including the layout, which is only adopted once
    /// the target is known to be real.
    ///
    /// The window's pre-snap geometry is recorded so that
    /// [`unsnap_window`](Self::unsnap_window) can put it back. That is separate
    /// from [`restored`](ManagedWindow::restored), which is the *maximize*
    /// memory and is deliberately cleared here: a snap is the user placing the
    /// window, so a later un-maximize must not yank it somewhere else. The two
    /// memories answer different questions and a window can be in both states'
    /// history without ambiguity.
    pub fn snap_window_to_zone(
        &mut self,
        id: WindowId,
        preset: snap::SnapLayoutPreset,
        zone: snap::ZoneId,
    ) -> bool {
        self.sync_snap_area();
        let Some(w) = self.windows.get(&id) else {
            return false;
        };
        let before = snap::SavedGeometry {
            x: w.x as f32,
            y: w.y as f32,
            width: w.width as f32,
            height: w.height as f32,
        };

        let previous = self.snap.active_preset();
        self.snap.set_layout(preset);
        // Recorded *before* the snap, because `SnapManager::snap_window` fills
        // in a zero-geometry placeholder for a window it has not seen — which
        // would restore to a 0x0 window at the origin.
        self.snap.history.record(id.0, zone, before);
        let Some((x, y, width, height)) = self.snap.snap_window(id.0, zone) else {
            // No such zone in this preset. Undo both the layout switch and the
            // history entry, so a bad zone id is not observable at all.
            self.snap.history.remove(id.0);
            self.snap.set_layout(previous);
            return false;
        };

        let Some(w) = self.windows.get_mut(&id) else {
            return false;
        };
        let (wx, wy, ww, wh) = round_rect(x, y, width, height);
        w.x = wx;
        w.y = wy;
        w.width = ww;
        w.height = wh;
        w.state = WindowState::Normal;
        // Snapping is the user placing the window, same as moving it.
        w.restored = None;
        true
    }

    /// Put a snapped window back where it was before it was snapped.
    ///
    /// Returns `false` if the window is not snapped, which is also what makes
    /// this safe to call unconditionally from a drag handler.
    pub fn unsnap_window(&mut self, id: WindowId) -> bool {
        let Some(geometry) = self.snap.history.restore(id.0) else {
            return false;
        };
        let Some(w) = self.windows.get_mut(&id) else {
            return false;
        };
        w.x = round_to_i32(geometry.x);
        w.y = round_to_i32(geometry.y);
        w.width = round_to_u32(geometry.width);
        w.height = round_to_u32(geometry.height);
        w.state = WindowState::Normal;
        true
    }

    /// Whether a window is currently occupying a snap zone.
    pub fn is_snapped(&self, id: WindowId) -> bool {
        self.snap.history.snapped_zone(id.0).is_some()
    }

    /// Get visible windows on current desktop, sorted by Z-order.
    pub fn visible_windows(&self) -> Vec<&ManagedWindow> {
        let mut windows: Vec<&ManagedWindow> = self
            .windows
            .values()
            .filter(|w| w.visible && w.desktop == self.current_desktop)
            .collect();
        windows.sort_by_key(|w| w.z_order);
        windows
    }

    // ======================================================================
    // Virtual desktops
    // ======================================================================

    /// The desktop one to the left of the current one, if there is one.
    pub const fn previous_desktop(&self) -> Option<u32> {
        self.current_desktop.checked_sub(1)
    }

    /// The desktop one to the right of the current one, if there is one.
    ///
    /// `num_desktops` is a public field that nothing clamps, so the obvious
    /// test — `current_desktop < num_desktops - 1` — underflowed on a shell
    /// configured with no desktops at all.
    pub const fn next_desktop(&self) -> Option<u32> {
        match self.current_desktop.checked_add(1) {
            Some(next) if next < self.num_desktops => Some(next),
            _ => None,
        }
    }

    /// The current desktop's number as the user sees it, counting from one.
    ///
    /// Every place that shows a desktop to a person adds one to the index;
    /// saying it here means none of them has to.
    pub const fn current_desktop_number(&self) -> u32 {
        self.current_desktop.saturating_add(1)
    }

    pub fn switch_desktop(&mut self, desktop: u32) {
        if desktop < self.num_desktops {
            self.current_desktop = desktop;
            self.focused_window = None;
            // Focus topmost window on new desktop
            if let Some(w) = self.visible_windows().last() {
                let id = w.id;
                self.focus_window(id);
            }
        }
    }

    pub fn move_window_to_desktop(&mut self, id: WindowId, desktop: u32) {
        if desktop < self.num_desktops
            && let Some(w) = self.windows.get_mut(&id)
        {
            w.desktop = desktop;
        }
    }

    // ======================================================================
    // Alt+Tab window switcher
    // ======================================================================

    /// Open the window switcher, on the window below the top one.
    ///
    /// That is the window the user was in before this one, which is what
    /// Alt+Tab is for. `visible_windows` is ordered bottom to top, so it is the
    /// second entry from the *end* — not index 1, which is what this used to
    /// say. With exactly two windows index 1 *is* the focused window, so
    /// press-and-release Alt+Tab — much the commonest use there is — re-focused
    /// the window you were already in and appeared to do nothing at all.
    pub fn start_alt_tab(&mut self) {
        let count = self.visible_windows().len();
        if count > 1 {
            self.alt_tab_active = true;
            self.alt_tab_index = step::wrapping_before(count, count.saturating_sub(1));
        }
    }

    pub fn next_alt_tab(&mut self) {
        let count = self.visible_windows().len();
        if count > 0 {
            // `step::wrapping_after` carries the "the list is not empty" condition
            // inside the expression that depends on it, and lands on the first
            // window rather than an arbitrary one if the index has gone stale
            // because windows closed while the switcher was open.
            self.alt_tab_index = step::wrapping_after(count, self.alt_tab_index);
        }
    }

    /// Step the switcher to the previous window, for Shift+Alt+Tab.
    pub fn prev_alt_tab(&mut self) {
        let count = self.visible_windows().len();
        if let Some(last) = count.checked_sub(1) {
            // Clamping first matters: a stale index — windows closed while the
            // switcher was open — would otherwise step from one out-of-range
            // index to another rather than back into the list.
            self.alt_tab_index = step::wrapping_before(count, self.alt_tab_index.min(last));
        }
    }

    pub fn finish_alt_tab(&mut self) {
        if self.alt_tab_active {
            let windows = self.visible_windows();
            if let Some(w) = windows.get(self.alt_tab_index) {
                let id = w.id;
                self.focus_window(id);
            }
            self.alt_tab_active = false;
        }
    }

    pub fn cancel_alt_tab(&mut self) {
        self.alt_tab_active = false;
    }

    // ======================================================================
    // Input handling
    // ======================================================================

    /// Handle a keyboard shortcut at the desktop level.
    ///
    /// Returns true if the shortcut was consumed.
    pub fn handle_hotkey(&mut self, key: &KeyEvent) -> bool {
        if !key.pressed {
            // Key release — check for Alt+Tab completion
            if (key.key == Key::LeftAlt || key.key == Key::RightAlt) && self.alt_tab_active {
                self.finish_alt_tab();
                return true;
            }
            return false;
        }

        match DesktopAction::for_chord(key.modifiers, key.key) {
            Some(action) => self.run_desktop_action(action),
            None => false,
        }
    }

    /// Carry out a shortcut that has already been recognised.
    ///
    /// Returns whether the press is consumed. Every binding but
    /// [`DismissPopup`](DesktopAction::DismissPopup) always is; that one is
    /// bare Escape, and a key the shell claims unconditionally is a key no
    /// window can ever see. Closing a dialog is what Escape does far more
    /// often than closing the start menu.
    fn run_desktop_action(&mut self, action: DesktopAction) -> bool {
        match action {
            DesktopAction::CycleWindows => {
                if self.alt_tab_active {
                    self.next_alt_tab();
                } else {
                    self.start_alt_tab();
                }
            }
            DesktopAction::CycleWindowsBackwards => {
                if !self.alt_tab_active {
                    self.start_alt_tab();
                }
                if self.alt_tab_active {
                    self.prev_alt_tab();
                }
            }
            DesktopAction::CloseFocused => {
                if let Some(id) = self.focused_window {
                    self.remove_window(id);
                }
            }
            DesktopAction::ToggleStartMenu => self.toggle_start_menu(),
            DesktopAction::ShowDesktop => {
                let ids: Vec<WindowId> = self
                    .windows
                    .values()
                    .filter(|w| w.visible && w.desktop == self.current_desktop)
                    .map(|w| w.id)
                    .collect();
                for id in ids {
                    self.minimize_window(id);
                }
            }
            DesktopAction::SnapLeft | DesktopAction::SnapRight => {
                if let Some(id) = self.focused_window {
                    self.snap_window(id, action == DesktopAction::SnapLeft);
                }
            }
            DesktopAction::Maximize => {
                if let Some(id) = self.focused_window {
                    self.maximize_window(id);
                }
            }
            DesktopAction::RestoreOrMinimize => {
                if let Some(id) = self.focused_window
                    && let Some(w) = self.windows.get(&id)
                {
                    if w.state == WindowState::Maximized {
                        self.restore_window(id);
                    } else {
                        self.minimize_window(id);
                    }
                }
            }
            DesktopAction::PreviousDesktop => {
                if let Some(target) = self.previous_desktop() {
                    self.switch_desktop(target);
                }
            }
            DesktopAction::NextDesktop => {
                if let Some(target) = self.next_desktop() {
                    self.switch_desktop(target);
                }
            }
            DesktopAction::DismissPopup => return self.dismiss_popups(),
        }
        true
    }
}

/// A desktop-level keyboard shortcut, named separately from the chord that
/// invokes it and from the code that carries it out.
///
/// The bindings used to be a chain of `if`s, each testing only the modifiers it
/// happened to care about, so a loose chord earlier in the chain swallowed a
/// tighter one later on: `Super+Left`/`Super+Right` (snap the focused window)
/// were tested before `Ctrl+Super+Left`/`Ctrl+Super+Right` (switch virtual
/// desktop), and matched with Ctrl held as well — so the virtual-desktop
/// shortcuts were unreachable and had never once fired. Pressing them snapped
/// a window instead.
///
/// Matching the whole modifier set at once makes that class of bug impossible:
/// every binding states its full chord, a modifier the chord does not list is a
/// modifier the binding does *not* fire with, and two arms claiming the same
/// chord are an unreachable-pattern warning rather than a silently dead
/// shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DesktopAction {
    CycleWindows,
    CycleWindowsBackwards,
    CloseFocused,
    ToggleStartMenu,
    ShowDesktop,
    SnapLeft,
    SnapRight,
    Maximize,
    RestoreOrMinimize,
    PreviousDesktop,
    NextDesktop,
    /// Close whatever popup is open. Unlike every other action here, this one
    /// can decline: see [`DesktopShell::run_desktop_action`].
    DismissPopup,
}

impl DesktopAction {
    /// The action a chord invokes, if it invokes one.
    ///
    /// This match is the whole binding table — there is no desktop shortcut
    /// recognised anywhere else.
    fn for_chord(modifiers: Modifiers, key: Key) -> Option<Self> {
        let Modifiers {
            shift,
            ctrl,
            alt,
            super_key,
        } = modifiers;
        match (shift, ctrl, alt, super_key, key) {
            (false, false, true, false, Key::Tab) => Some(Self::CycleWindows),
            (true, false, true, false, Key::Tab) => Some(Self::CycleWindowsBackwards),
            (false, false, true, false, Key::F4) => Some(Self::CloseFocused),
            // The Super key on its own. It is itself a modifier, so whether the
            // modifier bit is already set when it is the key being pressed is
            // the keyboard driver's business, and neither answer should change
            // what the key does.
            (false, false, false, _, Key::LeftSuper | Key::RightSuper) => {
                Some(Self::ToggleStartMenu)
            }
            (false, false, false, true, Key::D) => Some(Self::ShowDesktop),
            (false, false, false, true, Key::Left) => Some(Self::SnapLeft),
            (false, false, false, true, Key::Right) => Some(Self::SnapRight),
            (false, false, false, true, Key::Up) => Some(Self::Maximize),
            (false, false, false, true, Key::Down) => Some(Self::RestoreOrMinimize),
            (false, true, false, true, Key::Left) => Some(Self::PreviousDesktop),
            (false, true, false, true, Key::Right) => Some(Self::NextDesktop),
            // Bare Escape. The shell had no binding for it at all, so the only
            // way to close the start menu or the calendar was to click
            // somewhere else — and a popup that a click opened but Escape
            // cannot close is the one every other desktop has taught the user
            // to expect. It is claimed *conditionally*: with nothing open the
            // press is not consumed and reaches the focused window, whose own
            // dialog may be what the user meant to dismiss.
            (false, false, false, false, Key::Escape) => Some(Self::DismissPopup),
            _ => None,
        }
    }
}

impl DesktopShell {
    // ======================================================================
    // Rendering
    // ======================================================================

    /// Render the taskbar using the GUI toolkit.
    pub fn render_taskbar(&self) -> RenderTree {
        let bar = self.taskbar_rect();
        let mut tree = RenderTree::new();

        // Taskbar background
        fill(&mut tree, bar, self.theme.taskbar_bg);

        // Start button
        let start = self.start_button_rect();
        let start_bg = if self.start_menu_open {
            self.theme.taskbar_active_bg
        } else {
            self.theme.taskbar_bg
        };
        fill(&mut tree, start, start_bg);
        tree.text(
            start.x + self.scale(12.0),
            start.y + self.scale(12.0),
            "\u{2261}", // hamburger menu icon
            self.theme.taskbar_accent,
            self.font_size(TextRole::Glyph),
        );

        // Window buttons. Rounded like the windows they stand for — the corner
        // style is a property of the desktop, not of one surface in it.
        let radii = self.corner_radii();
        for (index, window) in self.visible_windows().iter().enumerate() {
            let button = self.taskbar_button_rect(index);

            let bg = if Some(window.id) == self.focused_window {
                self.theme.taskbar_active_bg
            } else {
                self.theme.taskbar_bg
            };

            fill_round(&mut tree, button, bg, radii);

            // Window title, fitted to what the button can hold — by the
            // renderer, which is the only thing that knows how wide the title
            // will be drawn. This used to take `button.w / (size * 0.62)`
            // *characters*: a guessed average advance applied to a proportional
            // face, so a title of capitals ("WWW Browser") overran the button
            // and one of narrow letters ("initialising…") was cut with the
            // space to spare. Scaling the guess with the font size, which the
            // old comment was pleased about, fixes only the half of the error
            // that depends on size; the half that depends on *which letters*
            // cannot be fixed by any constant.
            //
            // `text_in` also marks the cut with `…`, so a truncated title is
            // distinguishable from a short one — a silently clipped one is not,
            // and a window called "Save changes to report.docx?" reading as
            // "Save changes to rep" is a different sentence.
            let title_size = self.font_size(TextRole::Caption);
            let inset = self.scale(8.0);
            tree.text_in(
                button.x + inset,
                button.y + inset,
                (button.w - inset - inset).max(0.0),
                &window.title,
                self.theme.taskbar_fg,
                title_size,
            );
        }

        // System tray (right side). Both items are placed from the display's
        // right edge inwards, so a wider clock — the date and weekday switches
        // roughly triple it — pushes the tray left instead of running off the
        // screen.
        let tray_x = self.tray_x();
        let padding = self.scale(TRAY_PADDING);
        let tray_text_y = bar.y + self.scale(12.0);

        // The clock sits in a fixed-width slot at the right end, and its text
        // starts at the slot's left edge. Aligning to the slot rather than to
        // the reading keeps it still: the slot is sized for the widest reading
        // these switches can produce, so a narrower one leaves a few pixels of
        // slack at the end instead of sliding the text sideways every minute.
        let time_str = self.current_clock_string();
        tree.text(
            bar.w - padding - self.clock_width(),
            tray_text_y,
            &time_str,
            self.theme.taskbar_fg,
            self.font_size(TextRole::Body),
        );

        // Desktop indicator, at the tray's left edge.
        tree.text(
            tray_x + padding,
            tray_text_y,
            &self.desktop_indicator_string(),
            self.theme.taskbar_fg,
            self.font_size(TextRole::Caption),
        );

        tree
    }

    /// Render window decorations (title bar, borders) for all visible windows.
    pub fn render_window_decorations(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        let radii = self.corner_radii();
        let top_radii = self.top_corner_radii();
        // The buttons are round when the windows are: a circular close button
        // beside a square corner is the mismatch, not the consistency.
        let button_radii =
            CornerRadii::all(radii.top_left.min(self.scale(WINDOW_BUTTON_SIZE) / 2.0));

        for window in self.visible_windows() {
            let chrome = self.window_chrome(window);

            // A shadow under the whole frame, before anything that casts it.
            // Maximized and fullscreen windows have no edge to cast from —
            // there is nothing beside them for a shadow to fall on, and one
            // drawn anyway would bleed over the screen border.
            if self.appearance.drop_shadows
                && !matches!(
                    window.state,
                    WindowState::Maximized | WindowState::Fullscreen
                )
            {
                shadow(&mut tree, chrome.frame, radii);
            }

            // Title bar. The foreground travels with the background: with
            // accented title bars the two differ by more than a shade, so one
            // shared text colour would be unreadable on one of them.
            let (title_bg, title_fg) = if window.focused {
                (self.theme.window_title_bg, self.theme.window_title_fg)
            } else {
                (
                    self.theme.window_title_inactive_bg,
                    self.theme.window_title_inactive_fg,
                )
            };

            fill_round(&mut tree, chrome.title_bar, title_bg, top_radii);

            // Title text
            let title: String = window.title.chars().take(40).collect();
            tree.text(
                chrome.title_bar.x + self.scale(12.0),
                chrome.title_bar.y + self.scale(8.0),
                &title,
                title_fg,
                self.font_size(TextRole::Body),
            );

            // Window control buttons, right to left.
            fill_round(
                &mut tree,
                chrome.close,
                Color::from_hex(0xF38BA8),
                button_radii,
            );
            tree.text(
                chrome.close.x + self.scale(3.0),
                chrome.close.y + self.scale(1.0),
                "x",
                Color::WHITE,
                self.font_size(TextRole::Caption),
            );
            fill_round(
                &mut tree,
                chrome.maximize,
                Color::from_hex(0xA6E3A1),
                button_radii,
            );
            fill_round(
                &mut tree,
                chrome.minimize,
                Color::from_hex(0xF9E2AF),
                button_radii,
            );

            // Border
            stroke_round(
                &mut tree,
                chrome.frame,
                self.theme.window_border_color,
                self.scale(1.0),
                radii,
            );
        }

        tree
    }

    /// Render the Alt+Tab window switcher overlay.
    pub fn render_alt_tab(&self) -> Option<RenderTree> {
        if !self.alt_tab_active {
            return None;
        }

        let mut tree = RenderTree::new();
        let windows = self.visible_windows();

        if windows.is_empty() {
            return None;
        }

        // Overlay background
        let overlay_w = self
            .scale(400.0)
            .min(self.screen_width as f32 - self.scale(100.0))
            .max(0.0);
        let overlay_h = self.scale(80.0);
        let overlay_x = (self.screen_width as f32 - overlay_w) / 2.0;
        let overlay_y = (self.screen_height as f32 - overlay_h) / 2.0;
        let overlay = Rect::new(overlay_x, overlay_y, overlay_w, overlay_h);
        let radii = self.corner_radii();

        // The switcher floats over whatever is behind it, so it casts a shadow
        // for the same reason a window does.
        if self.appearance.drop_shadows {
            shadow(&mut tree, overlay, radii);
        }
        fill_round(&mut tree, overlay, self.theme.overlay_bg, radii);
        stroke_round(
            &mut tree,
            overlay,
            self.theme.accent_color,
            self.scale(2.0),
            radii,
        );

        // Window entries
        let item_w = overlay_w / windows.len().max(1) as f32;
        let inset = self.scale(4.0);
        for (i, window) in windows.iter().enumerate() {
            let ix = overlay_x + i as f32 * item_w;

            if i == self.alt_tab_index {
                fill_round(
                    &mut tree,
                    Rect::new(
                        ix + inset,
                        overlay_y + inset,
                        (item_w - inset * 2.0).max(0.0),
                        (overlay_h - inset * 2.0).max(0.0),
                    ),
                    self.theme.overlay_selected_bg,
                    radii,
                );
            }

            let title: String = window.title.chars().take(12).collect();
            tree.text(
                ix + self.scale(10.0),
                overlay_y + overlay_h / 2.0 - self.scale(6.0),
                &title,
                self.theme.overlay_fg,
                self.font_size(TextRole::Caption),
            );
        }

        Some(tree)
    }

    /// Render the start menu.
    pub fn render_start_menu(&self) -> Option<RenderTree> {
        if !self.start_menu_open {
            return None;
        }

        let mut tree = RenderTree::new();
        let menu = self.start_menu_rect();
        let radii = self.corner_radii();

        // Background
        if self.appearance.drop_shadows {
            shadow(&mut tree, menu, radii);
        }
        fill_round(&mut tree, menu, self.theme.start_menu_bg, radii);
        stroke_round(
            &mut tree,
            menu,
            self.theme.window_border_color,
            self.scale(1.0),
            radii,
        );

        // Title
        tree.text(
            menu.x + self.scale(16.0),
            menu.y + self.scale(16.0),
            "Applications",
            self.theme.accent_color,
            self.font_size(TextRole::Heading),
        );

        // Application entries. Which entry a row shows is asked of
        // `start_menu_entry_at`, the same function the hit test asks, so a
        // scrolled menu cannot launch the program on the row above the one
        // that was clicked.
        let entries = self.start_menu_entries();
        let rows = self.start_menu_visible_rows();
        for row in 0..rows {
            let Some(index) = self.start_menu_entry_at(row) else {
                break;
            };
            let Some(entry) = entries.get(index) else {
                break;
            };
            let rect = self.start_menu_row_rect(row);
            tree.text(
                rect.x + self.scale(24.0),
                rect.y + self.scale(8.0),
                &entry.name,
                self.theme.start_menu_fg,
                self.font_size(TextRole::Item),
            );
        }

        // A scroll indicator, so a list that continues past the last row says
        // so. Sized and placed in proportion to the part of the list on screen.
        let total = entries.len();
        if total > rows && rows > 0 {
            let row_h = self.scale(START_MENU_ROW_HEIGHT);
            let bar_w = self.scale(START_MENU_SCROLLBAR_WIDTH);
            let track_top = self.start_menu_row_rect(0).y;
            let track_h = rows as f32 * row_h;
            let thumb_h = (track_h * rows as f32 / total as f32).max(row_h / 2.0);
            let max_scroll = self.start_menu_max_scroll().max(1) as f32;
            let progress = self.start_menu_scroll as f32 / max_scroll;
            fill_round(
                &mut tree,
                Rect::new(
                    menu.x + menu.w - bar_w - self.scale(2.0),
                    track_top + (track_h - thumb_h) * progress,
                    bar_w,
                    thumb_h,
                ),
                self.theme.accent_color,
                CornerRadii::all(bar_w / 2.0),
            );
        }

        // The power button. Drawn as pressed while its menu is showing, so the
        // popup that appears over the list has something visible that it came
        // from.
        let button = self.power_button_rect();
        let button_radii = CornerRadii::all(radii.top_left.min(button.h / 2.0));
        if self.power_menu_open {
            fill_round(&mut tree, button, self.theme.accent_color, button_radii);
        }
        let label_size = self.font_size(TextRole::Body);
        tree.text(
            button.x + self.scale(POWER_MENU_TEXT_INSET),
            button.y + (button.h - label_size).max(0.0) / 2.0,
            "Power",
            if self.power_menu_open {
                self.theme.start_menu_bg
            } else {
                self.theme.start_menu_fg
            },
            label_size,
        );

        if self.power_menu_open {
            self.render_power_menu(&mut tree);
        }

        Some(tree)
    }

    /// Draw the power menu into the start menu's tree.
    ///
    /// The popup itself is drawn by [`power`], which owns everything about
    /// system power; this method's job is only to hand it the geometry the hit
    /// test uses and the colours the user chose.
    fn render_power_menu(&self, tree: &mut RenderTree) {
        let panel = self.power_menu_rect();
        let radii = self.corner_radii();

        if self.appearance.drop_shadows {
            shadow(tree, panel, radii);
        }

        let entries = self.power_menu_entries();
        let rows: Vec<power::PowerMenuRow<'_>> = (0..self.power_menu_visible_rows())
            .filter_map(|row| {
                entries.get(row).map(|entry| power::PowerMenuRow {
                    label: &entry.name,
                    rect: self.power_menu_row_rect(row),
                })
            })
            .collect();

        tree.extend(power::render_power_menu(
            panel,
            &rows,
            power::PowerMenuStyle {
                background: self.theme.start_menu_bg,
                foreground: self.theme.start_menu_fg,
                border: Border {
                    width: self.scale(1.0),
                    color: self.theme.window_border_color,
                },
                radii,
                font_size: self.font_size(TextRole::Item),
                text_inset: self.scale(POWER_MENU_TEXT_INSET),
            },
        ));
    }

    // ======================================================================
    // Utilities
    // ======================================================================

    /// The zone the taskbar clock reads in.
    ///
    /// UTC when the configured zone is not in the table — which is honest
    /// rather than convenient: a zone we cannot resolve is not a licence to
    /// invent an offset, and `datetime_settings` already refuses to read a
    /// zoneinfo *name* for the same reason (there is no tzdata on disk yet;
    /// `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`).
    fn local_zone(&self) -> Tz {
        self.datetime
            .current_timezone()
            .map_or_else(Tz::utc, |tz| tz.rule)
    }

    /// The clock as the Date & Time panel has configured it.
    ///
    /// Derived on every read rather than cached beside `datetime`, on the same
    /// reasoning as [`sync_snap_area`](Self::sync_snap_area): `datetime` is a
    /// public field that anything may assign, so a cached copy would be one
    /// forgotten call site away from showing a setting the user has changed.
    /// It is three bools and an empty `Vec`, which does not allocate.
    fn clock(&self) -> calendar::ClockDisplay {
        let mut clock = calendar::ClockDisplay::new();
        clock.show_seconds = self.datetime.show_seconds;
        clock.show_day_of_week = self.datetime.show_day_of_week;
        clock.show_date = self.datetime.show_date;
        clock
    }

    /// The taskbar clock reading.
    ///
    /// This used to be four lines of `secs % 86400` — which is **UTC**, with no
    /// zone applied at all. The shipped default zone is `America/New_York`, so
    /// out of the box the corner of the screen was five hours wrong, and no
    /// setting on the Date & Time panel could correct it: `show_seconds`,
    /// `show_day_of_week` and `show_date` are each documented as applying "in
    /// the taskbar clock" and reached nothing.
    ///
    /// Meanwhile [`calendar::ClockDisplay`] — a complete taskbar clock, with
    /// zone handling, a seconds switch, a 12/24-hour switch and its own tests —
    /// had **no callers anywhere in the tree**. That is the same defect as the
    /// two snap implementations (see [`snap`](Self::snap) and
    /// design-decisions §469): the shell drawing its own lesser copy of
    /// something the tree already did properly, with the user able to see only
    /// the lesser one.
    fn current_clock_string(&self) -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.clock_string_at(secs)
    }

    /// The taskbar clock reading for a given UTC instant.
    ///
    /// Split from [`current_clock_string`](Self::current_clock_string) so that
    /// the reading can be asserted at all: everything above reads the wall
    /// clock, and a test of a function that consults `SystemTime::now` can
    /// only ever check its *shape*, never its value — which is exactly the
    /// hole the UTC bug lived in.
    fn clock_string_at(&self, utc_secs: u64) -> String {
        self.clock().format_taskbar(utc_secs, &self.local_zone())
    }

    /// How wide the clock's slot in the tray is.
    ///
    /// The **widest** reading the current switches can produce, not the current
    /// one: see [`calendar::ClockDisplay::reading_width`]. Everything else in
    /// the tray is positioned from this, so a width that followed the current
    /// second would shuffle the tray once a minute.
    fn clock_width(&self) -> f32 {
        self.clock().reading_width(self.font_size(TextRole::Body))
    }

    /// How wide the virtual-desktop indicator's text is.
    fn desktop_indicator_width(&self) -> f32 {
        text::width(
            &self.desktop_indicator_string(),
            self.font_size(TextRole::Caption),
        )
    }

    /// What the virtual-desktop indicator reads.
    fn desktop_indicator_string(&self) -> String {
        format!("Desktop {}", self.current_desktop_number())
    }

    /// How much of the taskbar's right end the tray occupies.
    ///
    /// Derived from what is actually in it rather than fixed at 180 px, because
    /// the clock's width is a *setting*: turning the date on takes it from
    /// `16:30` to `Thu Aug 21 16:30`, roughly tripling it. With a constant
    /// reserve the extra simply ran off the right edge of the display — which
    /// is how a shipped default of `show_date: true` could have gone unnoticed,
    /// since nothing about a clipped clock says which end was cut.
    fn tray_width(&self) -> f32 {
        let padding = self.scale(TRAY_PADDING);
        let content = self.clock_width() + self.desktop_indicator_width();
        // Padding at the right edge, between the two items, and at the left of
        // the tray.
        (content + padding * 3.0).max(self.scale(TRAY_MIN_WIDTH))
    }

    // ========================================================================
    // Calendar popup
    // ========================================================================

    /// The clock's clickable area at the right end of the taskbar.
    ///
    /// The slot plus the padding to its right, and the bar's full height: the
    /// reading is one line of text in the middle of a 40-px bar, and a target
    /// that was only as tall as the glyphs would miss most presses aimed at it.
    /// Derived from the same `clock_width` and `TRAY_PADDING` the renderer
    /// places the text with, so it cannot drift from what is drawn.
    #[must_use]
    pub fn clock_rect(&self) -> Rect {
        let bar = self.taskbar_rect();
        let padding = self.scale(TRAY_PADDING);
        let width = self.clock_width() + padding;
        Rect::new((bar.w - width).max(0.0), bar.y, width.min(bar.w), bar.h)
    }

    /// The scale the popup is laid out at.
    ///
    /// The shell's own, not the toolkit's global: the popup hangs off taskbar
    /// chrome that [`scale`](Self::scale) has already multiplied, so a popup
    /// laid out in logical pixels would be half-size at 200% and anchored to
    /// the wrong pixel.
    #[must_use]
    pub fn calendar_scale(&self) -> f32 {
        self.appearance.scale_factor()
    }

    /// Where the popup's top-left corner goes: above the taskbar, right-aligned
    /// to the display edge with the tray's padding.
    ///
    /// Both axes are clamped to the display, and on a display too small to
    /// hold the popup above the taskbar the clamp means it overlaps the bar
    /// rather than running off the top. That is the right way round: the
    /// popup's controls — the arrows, the title and the clock band — are all
    /// in its first eighty pixels, so losing the bottom of the grid leaves it
    /// usable while losing the top would not. A 640×480 display at 200%
    /// scaling is the case; the popup is 480 px tall there and the space above
    /// the taskbar is 400.
    #[must_use]
    pub fn calendar_origin(&self) -> (f32, f32) {
        let scale = self.calendar_scale();
        let size = self.calendar.popup_rect(0.0, 0.0, scale);
        let padding = self.scale(TRAY_PADDING);
        let x = (self.screen_width as f32 - size.w - padding).max(0.0);
        let y = (self.taskbar_rect().y - size.h - padding).max(0.0);
        (x, y)
    }

    /// The clock the popup's header band shows.
    ///
    /// This is where [`datetime_settings::AdditionalClock`] finally reaches a
    /// surface. The field has existed since the Date & Time panel was written
    /// — the panel can add up to four zones, name them, and hide them — and
    /// nothing anywhere drew one, so `visible` was a flag whose only effect
    /// was to print "Hidden" beside its own row in the panel that set it.
    ///
    /// The popup rather than the tray, because the tray is already the width
    /// of its widest possible reading (see [`clock_width`](Self::clock_width))
    /// and four more zones there would push the window buttons off the bar.
    /// See `design-decisions.md` §493.
    fn popup_clock(&self) -> calendar::ClockDisplay {
        let mut clock = self.clock();
        // The header band has room for the full reading, so it shows the date
        // regardless of whether the *taskbar* is configured to.
        clock.show_date = true;
        for extra in &self.datetime.additional_clocks {
            if !extra.visible {
                continue;
            }
            // A zone the table cannot resolve is dropped rather than shown at
            // UTC under its own label, which would be a wrong clock presented
            // as a right one. `local_zone` refuses the same way.
            let Some(info) = self
                .datetime
                .available_timezones
                .iter()
                .find(|tz| tz.tz_id == extra.tz_id)
            else {
                continue;
            };
            clock.extra_timezones.push(calendar::TimezoneEntry {
                label: extra.label.clone(),
                tz: info.rule,
            });
        }
        clock
    }

    /// Open the calendar popup, or close it if it is already open.
    pub fn toggle_calendar(&mut self) {
        if self.calendar.visible {
            self.calendar.set_visible(false);
            return;
        }
        // Opening a popup closes the other one: two panels covering the same
        // taskbar at once is a state the user cannot have asked for.
        self.start_menu_open = false;
        self.power_menu_open = false;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let zone = self.local_zone();
        // Today comes from the zone's *rules*, so the popup cannot open on a
        // different day than the reading that opened it.
        self.calendar.set_today_from_zone(now, &zone);
        self.calendar.header = Some(calendar::ClockHeader {
            clock: self.popup_clock(),
            zone,
        });
        self.calendar.set_visible(true);
    }

    /// Close whatever popup is open. Returns whether anything was.
    ///
    /// The return value is what keeps Escape from being swallowed: a press
    /// with nothing open must reach the focused window, whose own dialog may
    /// be what the user meant to dismiss.
    pub fn dismiss_popups(&mut self) -> bool {
        let any = self.start_menu_open || self.power_menu_open || self.calendar.visible;
        self.start_menu_open = false;
        self.power_menu_open = false;
        self.calendar.set_visible(false);
        any
    }

    /// Render the calendar popup, if it is open.
    #[must_use]
    pub fn render_calendar(&self) -> Option<RenderTree> {
        if !self.calendar.visible {
            return None;
        }
        let (x, y) = self.calendar_origin();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut tree = RenderTree::new();
        tree.commands.extend(
            self.calendar
                .render(x, y, self.calendar_scale(), now, &self.events),
        );
        Some(tree)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod theme_tests {
    use super::*;
    use appearance::{AccentColor, ThemeMode};

    /// Contrast ratio per WCAG 2.x, for asserting that text is readable rather
    /// than merely "a different colour".
    fn contrast(a: Color, b: Color) -> f32 {
        fn channel(c: u8) -> f32 {
            let c = f32::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        fn luminance(c: Color) -> f32 {
            0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
        }
        let (l1, l2) = (luminance(a), luminance(b));
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    fn settings() -> AppearanceSettings {
        AppearanceSettings::default()
    }

    #[test]
    fn the_default_theme_is_the_dark_one() {
        assert_eq!(DesktopTheme::default(), DesktopTheme::dark());
        assert_eq!(DesktopShell::new(800, 600).theme, DesktopTheme::dark());
    }

    #[test]
    fn the_mode_picks_the_base_palette() {
        let mut s = settings();
        s.theme_mode = ThemeMode::Dark;
        assert_eq!(
            DesktopTheme::from_settings(&s).desktop_bg,
            DesktopTheme::dark().desktop_bg
        );
        s.theme_mode = ThemeMode::Light;
        assert_eq!(
            DesktopTheme::from_settings(&s).desktop_bg,
            DesktopTheme::light().desktop_bg
        );
        // "System" has no schedule to follow yet, so it stays on dark rather
        // than flipping the desktop for a user who asked to be left alone.
        s.theme_mode = ThemeMode::System;
        assert_eq!(
            DesktopTheme::from_settings(&s).desktop_bg,
            DesktopTheme::dark().desktop_bg
        );
    }

    #[test]
    fn the_accent_reaches_the_theme() {
        let mut s = settings();
        s.accent_color = AccentColor::Green;
        let theme = DesktopTheme::from_settings(&s);
        assert_eq!(theme.accent_color, AccentColor::Green.color());
        assert_eq!(theme.taskbar_accent, AccentColor::Green.color());
    }

    #[test]
    fn a_light_theme_uses_the_light_accent() {
        let mut s = settings();
        s.theme_mode = ThemeMode::Light;
        s.accent_color = AccentColor::Blue;
        let theme = DesktopTheme::from_settings(&s);
        assert_eq!(theme.accent_color, AccentColor::Blue.color_light());
        assert_ne!(theme.accent_color, AccentColor::Blue.color());
    }

    /// The reason `color_light` exists: the shell draws the accent as text —
    /// the start glyph, the start-menu heading — so every accent has to clear
    /// the body-text contrast bar on the surface its own mode paints. This is
    /// the test that rejects using Catppuccin's Latte accents unmodified.
    #[test]
    fn every_accent_is_readable_as_text_in_both_modes() {
        for &accent in AccentColor::presets() {
            let mut s = settings();
            s.accent_color = accent;

            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                s.theme_mode = mode;
                let theme = DesktopTheme::from_settings(&s);
                let ratio = contrast(theme.accent_color, theme.start_menu_bg);
                assert!(
                    ratio >= 4.5,
                    "{mode:?} {accent:?}: contrast {ratio:.2} < 4.5"
                );
            }
        }
    }

    #[test]
    fn a_custom_accent_is_used_exactly_as_chosen() {
        let mut s = settings();
        s.accent_color = AccentColor::Custom;
        s.custom_accent = Color::from_hex(0x123456);
        assert_eq!(
            DesktopTheme::from_settings(&s).accent_color,
            Color::from_hex(0x123456)
        );
        // Including in light mode: the user named a colour, not a role.
        s.theme_mode = ThemeMode::Light;
        assert_eq!(
            DesktopTheme::from_settings(&s).accent_color,
            Color::from_hex(0x123456)
        );
    }

    /// An accented taskbar must not swallow the start glyph, which is drawn in
    /// the accent colour on the taskbar's own background.
    #[test]
    fn an_accented_taskbar_keeps_its_glyph_visible() {
        for &accent in AccentColor::presets() {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                let mut s = settings();
                s.theme_mode = mode;
                s.accent_color = accent;
                s.accent_taskbar = true;
                s.taskbar_style = TaskbarStyle::Solid;
                let theme = DesktopTheme::from_settings(&s);

                assert_eq!(theme.taskbar_bg, s.effective_accent());
                assert_ne!(theme.taskbar_accent, theme.taskbar_bg);
                let glyph = contrast(theme.taskbar_accent, theme.taskbar_bg);
                assert!(
                    glyph >= 4.5,
                    "{mode:?} {accent:?}: glyph contrast {glyph:.2}"
                );
                let text = contrast(theme.taskbar_fg, theme.taskbar_bg);
                assert!(text >= 4.5, "{mode:?} {accent:?}: text contrast {text:.2}");
                assert_ne!(theme.taskbar_active_bg, theme.taskbar_bg);
            }
        }
    }

    #[test]
    fn an_unaccented_taskbar_is_left_on_the_base_palette() {
        let mut s = settings();
        s.accent_color = AccentColor::Red;
        s.accent_taskbar = false;
        s.taskbar_style = TaskbarStyle::Solid;
        let theme = DesktopTheme::from_settings(&s);
        assert_eq!(theme.taskbar_bg, DesktopTheme::dark().taskbar_bg);
        assert_eq!(theme.taskbar_fg, DesktopTheme::dark().taskbar_fg);
    }

    /// Accenting title bars must not cost the focused/unfocused distinction,
    /// and must not leave the inactive bar's text on the wrong background.
    #[test]
    fn accented_title_bars_still_mark_only_the_focused_window() {
        for &accent in AccentColor::presets() {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                let mut s = settings();
                s.theme_mode = mode;
                s.accent_color = accent;
                s.accent_titlebars = true;
                let theme = DesktopTheme::from_settings(&s);
                let base = if mode == ThemeMode::Light {
                    DesktopTheme::light()
                } else {
                    DesktopTheme::dark()
                };

                assert_eq!(theme.window_title_bg, s.effective_accent());
                assert_ne!(theme.window_title_bg, theme.window_title_inactive_bg);
                assert_eq!(
                    theme.window_title_inactive_bg,
                    base.window_title_inactive_bg
                );

                let active = contrast(theme.window_title_fg, theme.window_title_bg);
                assert!(
                    active >= 4.5,
                    "{mode:?} {accent:?}: active title {active:.2}"
                );
                let inactive = contrast(
                    theme.window_title_inactive_fg,
                    theme.window_title_inactive_bg,
                );
                assert!(
                    inactive >= 3.0,
                    "{mode:?} {accent:?}: inactive title {inactive:.2}"
                );
            }
        }
    }

    #[test]
    fn the_taskbar_style_and_the_transparency_level_both_have_a_say() {
        let mut s = settings();

        // Solid opts out however high the master level is.
        s.taskbar_style = TaskbarStyle::Solid;
        for level in [
            TransparencyLevel::Off,
            TransparencyLevel::Subtle,
            TransparencyLevel::Moderate,
            TransparencyLevel::Full,
        ] {
            s.transparency = level;
            assert_eq!(
                DesktopTheme::from_settings(&s).taskbar_bg.a,
                255,
                "{level:?}"
            );
        }

        // Translucent follows the level — but `Subtle` is documented as
        // reaching overlays and popups only, so it leaves the taskbar alone.
        s.taskbar_style = TaskbarStyle::Translucent;
        s.transparency = TransparencyLevel::Off;
        assert_eq!(DesktopTheme::from_settings(&s).taskbar_bg.a, 255);
        s.transparency = TransparencyLevel::Subtle;
        assert_eq!(DesktopTheme::from_settings(&s).taskbar_bg.a, 255);
        s.transparency = TransparencyLevel::Moderate;
        assert_eq!(
            DesktopTheme::from_settings(&s).taskbar_bg.a,
            TransparencyLevel::Moderate.panel_alpha()
        );
        s.transparency = TransparencyLevel::Full;
        assert_eq!(
            DesktopTheme::from_settings(&s).taskbar_bg.a,
            TransparencyLevel::Full.panel_alpha()
        );

        // Transparent asks the panel to disappear — unless transparency is
        // off, where an off switch has to mean off.
        s.taskbar_style = TaskbarStyle::Transparent;
        s.transparency = TransparencyLevel::Moderate;
        assert_eq!(DesktopTheme::from_settings(&s).taskbar_bg.a, 0);
        s.transparency = TransparencyLevel::Off;
        assert_eq!(DesktopTheme::from_settings(&s).taskbar_bg.a, 255);
    }

    #[test]
    fn overlays_and_menus_follow_the_transparency_level_alone() {
        let mut s = settings();
        // The taskbar style must not reach them.
        s.taskbar_style = TaskbarStyle::Solid;
        for level in [
            TransparencyLevel::Off,
            TransparencyLevel::Subtle,
            TransparencyLevel::Moderate,
            TransparencyLevel::Full,
        ] {
            s.transparency = level;
            let theme = DesktopTheme::from_settings(&s);
            assert_eq!(theme.overlay_bg.a, level.panel_alpha(), "{level:?}");
            assert_eq!(theme.start_menu_bg.a, level.panel_alpha(), "{level:?}");
        }
    }

    /// Transparency is applied to the surface, not to what is drawn on it: a
    /// see-through panel with see-through text on it would be unreadable.
    #[test]
    fn transparency_does_not_reach_the_foreground() {
        let mut s = settings();
        s.transparency = TransparencyLevel::Full;
        s.taskbar_style = TaskbarStyle::Translucent;
        let theme = DesktopTheme::from_settings(&s);
        assert!(theme.taskbar_bg.a < 255);
        assert_eq!(theme.taskbar_fg.a, 255);
        assert_eq!(theme.taskbar_accent.a, 255);
        assert_eq!(theme.taskbar_active_bg.a, 255);
        assert_eq!(theme.overlay_fg.a, 255);
        assert_eq!(theme.start_menu_fg.a, 255);
    }

    #[test]
    fn set_appearance_keeps_the_theme_and_the_settings_agreeing() {
        let mut shell = DesktopShell::new(800, 600);
        let mut s = settings();
        s.theme_mode = ThemeMode::Light;
        s.accent_color = AccentColor::Mauve;
        shell.set_appearance(s.clone());

        assert_eq!(shell.appearance, s);
        assert_eq!(shell.theme, DesktopTheme::from_settings(&s));
        assert_eq!(shell.theme.accent_color, AccentColor::Mauve.color_light());
    }

    /// The rendered frame, not just the palette, has to change with the theme.
    #[test]
    fn the_taskbar_is_painted_in_the_current_theme() {
        let mut shell = DesktopShell::new(800, 600);
        let dark = shell.render_taskbar();

        let mut s = settings();
        s.theme_mode = ThemeMode::Light;
        shell.set_appearance(s);
        let light = shell.render_taskbar();

        assert_eq!(dark.len(), light.len());
        assert_ne!(format!("{dark:?}"), format!("{light:?}"));
    }
}

/// Tests for the window manager itself — window lifecycle, stacking, snapping,
/// virtual desktops and Alt+Tab.
///
/// Until now `main.rs` was covered only by `theme_tests`, so none of this had
/// any: the code that decides which window is on top, which desktop it lives
/// on and where a snapped window's edges fall was checked by nothing but the
/// demo `main`. Each test below pins one rule the rest of the shell relies on,
/// and several of them exist because rewriting the counters and the desktop
/// arithmetic turned up a case that used to be wrong — the one-pixel gap
/// between snapped halves, the underflow on a shell with no desktops, and the
/// z-order counter running out.
#[cfg(test)]
mod window_manager_tests {
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

    use super::{DesktopShell, Key, KeyEvent, Modifiers, TextRole, WindowId, WindowState, text};

    fn shell() -> DesktopShell {
        DesktopShell::new(1920, 1080)
    }

    fn open(shell: &mut DesktopShell, title: &str) -> WindowId {
        shell.add_window(title, 100, 100, 400, 300, 1)
    }

    /// A window's rectangle, for tests that assert one operation left it alone.
    /// `ManagedWindow` is neither `Copy` nor `PartialEq` — it carries a title
    /// and an icon — so the comparison has to name the fields that matter.
    fn geometry(shell: &DesktopShell, id: WindowId) -> (i32, i32, u32, u32) {
        let w = shell.windows.get(&id).unwrap();
        (w.x, w.y, w.width, w.height)
    }

    fn press(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: None,
        }
    }

    fn super_only() -> Modifiers {
        Modifiers {
            super_key: true,
            ..Modifiers::NONE
        }
    }

    fn ctrl_super() -> Modifiers {
        Modifiers {
            ctrl: true,
            super_key: true,
            ..Modifiers::NONE
        }
    }

    fn z_of(shell: &DesktopShell, id: WindowId) -> u32 {
        shell
            .windows
            .get(&id)
            .expect("window is still open")
            .z_order
    }

    // ==================================================================
    // Identity and stacking
    // ==================================================================

    /// Two windows must never share an id: the id is the key everything else
    /// looks a window up by, so a repeat would silently merge two windows.
    #[test]
    fn every_window_gets_an_id_of_its_own() {
        let mut shell = shell();
        let ids: Vec<WindowId> = (0..200)
            .map(|i| open(&mut shell, &format!("w{i}")))
            .collect();

        let mut distinct = ids.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), ids.len());
        assert_eq!(shell.windows.len(), ids.len());
    }

    #[test]
    fn a_new_window_opens_above_the_existing_ones_and_takes_focus() {
        let mut shell = shell();
        let first = open(&mut shell, "first");
        let second = open(&mut shell, "second");

        assert!(z_of(&shell, second) > z_of(&shell, first));
        assert_eq!(shell.focused_window, Some(second));
        assert_eq!(shell.visible_windows().last().map(|w| w.id), Some(second));
    }

    #[test]
    fn focusing_a_window_raises_it_above_every_other() {
        let mut shell = shell();
        let bottom = open(&mut shell, "bottom");
        let middle = open(&mut shell, "middle");
        let top = open(&mut shell, "top");

        shell.focus_window(bottom);

        let raised = z_of(&shell, bottom);
        assert!(raised > z_of(&shell, middle));
        assert!(raised > z_of(&shell, top));
        assert_eq!(shell.visible_windows().last().map(|w| w.id), Some(bottom));
        assert!(!shell.windows.get(&middle).unwrap().focused);
    }

    /// The z counter is bumped on every focus change, so a long-running session
    /// can genuinely exhaust it. Saturating would freeze the stacking order and
    /// wrapping would invert it; renumbering keeps it working, which is what
    /// this checks.
    #[test]
    fn the_stacking_order_survives_the_z_counter_running_out() {
        let mut shell = shell();
        let bottom = open(&mut shell, "bottom");
        let middle = open(&mut shell, "middle");
        let top = open(&mut shell, "top");

        shell.next_z = u32::MAX;
        let raised = open(&mut shell, "raised");

        assert!(shell.next_z < u32::MAX, "the counter must have room again");
        let order: Vec<WindowId> = shell.visible_windows().iter().map(|w| w.id).collect();
        assert_eq!(order, vec![bottom, middle, top, raised]);

        // And it must keep working afterwards.
        shell.focus_window(middle);
        assert_eq!(shell.visible_windows().last().map(|w| w.id), Some(middle));
    }

    // ==================================================================
    // Closing and minimizing
    // ==================================================================

    #[test]
    fn closing_the_focused_window_focuses_the_one_below_it() {
        let mut shell = shell();
        let below = open(&mut shell, "below");
        let focused = open(&mut shell, "focused");

        shell.remove_window(focused);

        assert_eq!(shell.focused_window, Some(below));
        assert!(shell.windows.get(&below).unwrap().focused);
    }

    #[test]
    fn closing_the_last_window_leaves_nothing_focused() {
        let mut shell = shell();
        let only = open(&mut shell, "only");

        shell.remove_window(only);

        assert_eq!(shell.focused_window, None);
        assert!(shell.windows.is_empty());
    }

    #[test]
    fn a_minimized_window_comes_back_when_it_is_focused() {
        let mut shell = shell();
        let id = open(&mut shell, "app");

        shell.minimize_window(id);
        assert!(!shell.windows.get(&id).unwrap().visible);
        assert!(shell.visible_windows().is_empty());

        shell.focus_window(id);
        let window = shell.windows.get(&id).unwrap();
        assert!(window.visible);
        assert_eq!(window.state, WindowState::Normal);
        assert_eq!(shell.focused_window, Some(id));
    }

    // ==================================================================
    // Snapping
    // ==================================================================

    /// Both halves used to be `width / 2` computed in the shell, which left a
    /// one-pixel strip of desktop down the middle of any odd-width screen and
    /// no deliberate gap at all. They now come from `snap.rs`, whose zones are
    /// separated by [`crate::snap::ZONE_GAP`] — so the property is no longer "the
    /// halves meet" but "they are exactly one gap apart, and with that gap they
    /// span the work area precisely".
    ///
    /// The widths swept are not decoration. `snap.rs` subtracts the gap before
    /// halving, so below 18 pixels the subtraction would produce a *negative*
    /// half and place the right zone outside the area it is tiling; the gap is
    /// therefore dropped under that threshold, and 17/18 pin the boundary. 1921
    /// pins the rounding rule: the right half lands on `x = 963.5, width =
    /// 957.5`, which rounds to an edge at 1922 unless the edges are rounded
    /// rather than the origin and extent separately.
    #[test]
    fn the_two_snapped_halves_span_the_work_area_with_one_gap_between_them() {
        let gap = crate::round_to_i32(crate::snap::ZONE_GAP);

        for screen_width in [1920_u32, 1921, 2560, 19, 18, 17, 3, 2, 1, 0] {
            let mut shell = DesktopShell::new(screen_width, 1080);
            let left = open(&mut shell, "left");
            let right = open(&mut shell, "right");

            shell.snap_window(left, true);
            shell.snap_window(right, false);

            let (wx, wy, ww, wh) = shell.work_area();
            let l = shell.windows.get(&left).unwrap();
            let r = shell.windows.get(&right).unwrap();
            let l_right_edge = l.x + i32::try_from(l.width).unwrap();
            let r_right_edge = r.x + i32::try_from(r.width).unwrap();

            // The gap is affordable exactly when it leaves each half no
            // narrower than the gap itself.
            let expected_gap = if screen_width >= 3 * u32::try_from(gap).unwrap() {
                gap
            } else {
                0
            };

            assert_eq!(
                l.x, wx,
                "the left half starts at the work area, width={screen_width}"
            );
            assert_eq!(
                r.x - l_right_edge,
                expected_gap,
                "the halves must be one gap apart, width={screen_width}"
            );
            assert_eq!(
                r_right_edge,
                wx + i32::try_from(ww).unwrap(),
                "the right half must end flush with the work area, width={screen_width}"
            );
            assert!(
                l.width.abs_diff(r.width) <= 1,
                "the halves must be equal up to rounding, width={screen_width}: \
                 {} vs {}",
                l.width,
                r.width
            );
            for w in [l, r] {
                assert_eq!(w.y, wy, "width={screen_width}");
                assert_eq!(w.height, wh, "width={screen_width}");
                assert_eq!(w.state, WindowState::Normal);
            }
        }
    }

    /// `work_area()` is derived from three fields the shell publishes as
    /// mutable — screen size, taskbar height, appearance — so a snap has to
    /// re-read it rather than trust a copy taken when the shell was built.
    /// Without that, changing resolution or moving the taskbar would tile a
    /// screen that no longer exists, and nothing would say so.
    #[test]
    fn snapping_follows_the_screen_and_the_taskbar_after_they_change() {
        let mut shell = shell();
        let id = open(&mut shell, "app");

        shell.screen_width = 1280;
        shell.screen_height = 800;
        shell.taskbar_height = 96;
        shell.snap_window(id, false);

        let (wx, wy, ww, wh) = shell.work_area();
        let w = shell.windows.get(&id).unwrap();
        assert_eq!(
            w.x + i32::try_from(w.width).unwrap(),
            wx + i32::try_from(ww).unwrap(),
            "the right half must end at the new screen's right edge"
        );
        assert_eq!(w.y, wy);
        assert_eq!(w.height, wh, "and stop above the taller taskbar");
    }

    /// Unsnapping puts the window back exactly where it was, not merely
    /// somewhere unsnapped.
    #[test]
    fn unsnapping_restores_the_geometry_the_window_had_before() {
        let mut shell = shell();
        let id = open(&mut shell, "app");
        shell.move_window(id, 137, 249);
        shell.resize_window(id, 640, 480);
        let before = geometry(&shell, id);

        assert!(!shell.is_snapped(id));
        shell.snap_window(id, true);
        assert!(shell.is_snapped(id));
        assert_ne!(shell.windows.get(&id).unwrap().width, before.2);

        assert!(shell.unsnap_window(id));
        assert_eq!(geometry(&shell, id), before);
        assert!(
            !shell.is_snapped(id),
            "and the window must no longer claim a zone"
        );
    }

    /// Unsnapping a window that was never snapped is a no-op, which is what
    /// makes it safe to call unconditionally from a drag handler.
    #[test]
    fn unsnapping_an_unsnapped_window_changes_nothing() {
        let mut shell = shell();
        let id = open(&mut shell, "app");
        let before = geometry(&shell, id);

        assert!(!shell.unsnap_window(id));
        assert_eq!(geometry(&shell, id), before);
    }

    /// A window the user has since moved or resized is no longer in its zone,
    /// so it must stop answering to "unsnap" — otherwise a later unsnap would
    /// teleport a window the user had just placed by hand.
    #[test]
    fn moving_or_resizing_a_snapped_window_ends_the_snap() {
        for (label, act) in [
            (
                "move",
                (|s: &mut DesktopShell, id| s.move_window(id, 10, 10))
                    as fn(&mut DesktopShell, WindowId),
            ),
            ("resize", |s: &mut DesktopShell, id| {
                s.resize_window(id, 300, 200);
            }),
        ] {
            let mut shell = shell();
            let id = open(&mut shell, "app");
            shell.snap_window(id, true);
            assert!(shell.is_snapped(id), "{label}: precondition");

            act(&mut shell, id);

            assert!(!shell.is_snapped(id), "{label} must end the snap");
            assert!(!shell.unsnap_window(id), "{label}: nothing left to restore");
        }
    }

    /// Closing a snapped window must not leave its saved geometry behind: the
    /// history is keyed by window id, and ids are not reused within a session,
    /// so every snapped-then-closed window would leak one entry forever.
    #[test]
    fn closing_a_snapped_window_forgets_its_saved_geometry() {
        let mut shell = shell();
        let id = open(&mut shell, "app");
        shell.snap_window(id, true);
        assert!(!shell.snap.history.is_empty());

        shell.remove_window(id);

        assert!(shell.snap.history.is_empty());
        assert!(!shell.is_snapped(id));
    }

    /// A zone id that the preset does not have must leave the shell exactly as
    /// it found it — including the active layout, which is otherwise switched
    /// before the zone is known to exist.
    #[test]
    fn snapping_to_a_zone_that_does_not_exist_changes_nothing() {
        let mut shell = shell();
        let id = open(&mut shell, "app");
        let before = geometry(&shell, id);
        let layout = shell.snap.active_preset();
        // The preset asked for is deliberately *not* the one already in force.
        // Asking for a bad zone of the current layout leaves the failed layout
        // switch invisible — which is exactly what the first version of this
        // test did, and it passed with the undo deleted.
        let other = crate::snap::SnapLayoutPreset::SixGrid;
        assert_ne!(layout, other, "precondition: the switch must be observable");

        assert!(!shell.snap_window_to_zone(id, other, 99));

        assert_eq!(geometry(&shell, id), before);
        assert_eq!(
            shell.snap.active_preset(),
            layout,
            "a rejected snap must not leave the layout switched"
        );
        assert!(!shell.is_snapped(id));
    }

    /// The zones of a preset are the shell's to use, not just `snap.rs`'s: a
    /// quadrant snap must land in the quadrant the module says it does.
    #[test]
    fn snapping_to_a_quadrant_lands_in_that_quadrant() {
        let mut shell = shell();
        let (wx, wy, ww, wh) = shell.work_area();
        let mid_x = wx + i32::try_from(ww).unwrap() / 2;
        let mid_y = wy + i32::try_from(wh).unwrap() / 2;

        for (zone, right, bottom) in [
            (0, false, false),
            (1, true, false),
            (2, false, true),
            (3, true, true),
        ] {
            let id = open(&mut shell, "app");
            assert!(shell.snap_window_to_zone(
                id,
                crate::snap::SnapLayoutPreset::FourQuadrants,
                zone
            ));

            let w = shell.windows.get(&id).unwrap();
            assert_eq!(
                w.x > mid_x - 1,
                right,
                "zone {zone} horizontal half: x={}",
                w.x
            );
            assert_eq!(
                w.y > mid_y - 1,
                bottom,
                "zone {zone} vertical half: y={}",
                w.y
            );
            assert!(
                i32::try_from(w.width).unwrap() < i32::try_from(ww).unwrap(),
                "zone {zone} must be a quadrant, not the whole width"
            );
        }
    }

    /// Snapping is the user placing the window, so a later "restore" must not
    /// yank it back to wherever it was before it was maximized.
    #[test]
    fn snapping_forgets_the_pre_maximize_geometry() {
        let mut shell = shell();
        let id = open(&mut shell, "app");

        shell.maximize_window(id);
        assert!(shell.windows.get(&id).unwrap().restored.is_some());

        shell.snap_window(id, true);
        assert!(shell.windows.get(&id).unwrap().restored.is_none());
    }

    // ==================================================================
    // Virtual desktops
    // ==================================================================

    #[test]
    fn desktop_navigation_stops_at_both_ends() {
        let mut shell = shell();
        let last = shell.num_desktops - 1;

        assert_eq!(shell.previous_desktop(), None);
        assert!(shell.handle_hotkey(&press(Key::Left, ctrl_super())));
        assert_eq!(shell.current_desktop, 0);

        for expected in 1..=last {
            assert!(shell.handle_hotkey(&press(Key::Right, ctrl_super())));
            assert_eq!(shell.current_desktop, expected);
        }

        assert_eq!(shell.next_desktop(), None);
        assert!(shell.handle_hotkey(&press(Key::Right, ctrl_super())));
        assert_eq!(shell.current_desktop, last);
    }

    /// `num_desktops` is a public field that nothing clamps. The old bound
    /// `current_desktop < num_desktops - 1` underflowed when it was zero.
    #[test]
    fn a_shell_with_no_desktops_does_not_underflow() {
        let mut shell = shell();
        shell.num_desktops = 0;

        assert_eq!(shell.previous_desktop(), None);
        assert_eq!(shell.next_desktop(), None);
        assert!(shell.handle_hotkey(&press(Key::Left, ctrl_super())));
        assert!(shell.handle_hotkey(&press(Key::Right, ctrl_super())));
        assert_eq!(shell.current_desktop, 0);
    }

    #[test]
    fn the_desktop_indicator_counts_from_one() {
        let mut shell = shell();
        assert_eq!(shell.current_desktop_number(), 1);

        shell.switch_desktop(2);
        assert_eq!(shell.current_desktop_number(), 3);
    }

    #[test]
    fn a_window_is_only_visible_on_its_own_desktop() {
        let mut shell = shell();
        let id = open(&mut shell, "app");

        shell.switch_desktop(1);
        assert!(shell.visible_windows().is_empty());
        assert_eq!(shell.focused_window, None);

        shell.move_window_to_desktop(id, 1);
        assert_eq!(
            shell
                .visible_windows()
                .iter()
                .map(|w| w.id)
                .collect::<Vec<_>>(),
            vec![id]
        );
    }

    #[test]
    fn a_window_cannot_be_moved_to_a_desktop_that_does_not_exist() {
        let mut shell = shell();
        let id = open(&mut shell, "app");

        shell.move_window_to_desktop(id, shell.num_desktops);
        shell.move_window_to_desktop(id, u32::MAX);

        assert_eq!(shell.windows.get(&id).unwrap().desktop, 0);
    }

    // ==================================================================
    // Alt+Tab
    // ==================================================================

    /// Two windows and one press-and-release is what Alt+Tab is mostly used
    /// for, and it used to be exactly the case that did nothing: the switcher
    /// opened on the window that already had focus.
    #[test]
    fn alt_tab_between_two_windows_swaps_them() {
        let mut shell = shell();
        let first = open(&mut shell, "first");
        let second = open(&mut shell, "second");
        assert_eq!(shell.focused_window, Some(second));

        shell.start_alt_tab();
        shell.finish_alt_tab();
        assert_eq!(shell.focused_window, Some(first));

        shell.start_alt_tab();
        shell.finish_alt_tab();
        assert_eq!(shell.focused_window, Some(second), "and back again");
    }

    #[test]
    fn alt_tab_visits_every_window_and_comes_back_round() {
        let mut shell = shell();
        for i in 0..3 {
            open(&mut shell, &format!("w{i}"));
        }

        shell.start_alt_tab();
        assert!(shell.alt_tab_active);
        let first = shell.alt_tab_index;

        let mut seen = vec![first];
        for _ in 0..2 {
            shell.next_alt_tab();
            seen.push(shell.alt_tab_index);
        }
        let mut distinct = seen.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 3, "every window is offered exactly once");

        shell.next_alt_tab();
        assert_eq!(shell.alt_tab_index, first, "and then it comes back round");
    }

    /// The switcher's index is into a list recomputed on every step, so windows
    /// closing while it is open can leave it past the end. It must land back in
    /// range rather than picking nothing or panicking.
    #[test]
    fn alt_tab_survives_the_windows_closing_underneath_it() {
        let mut shell = shell();
        let ids: Vec<WindowId> = (0..3).map(|i| open(&mut shell, &format!("w{i}"))).collect();

        shell.start_alt_tab();
        shell.next_alt_tab();

        shell.remove_window(ids[1]);
        shell.remove_window(ids[2]);

        shell.next_alt_tab();
        assert!(shell.alt_tab_index < shell.visible_windows().len());

        shell.finish_alt_tab();
        assert!(!shell.alt_tab_active);
        assert_eq!(shell.focused_window, Some(ids[0]));
    }

    #[test]
    fn alt_tab_on_an_empty_desktop_does_nothing() {
        let mut shell = shell();

        shell.start_alt_tab();
        assert!(!shell.alt_tab_active);

        shell.next_alt_tab();
        shell.finish_alt_tab();
        assert_eq!(shell.focused_window, None);
    }

    /// A single window is not worth a switcher, but the keystroke must still be
    /// consumed rather than falling through to the focused app.
    #[test]
    fn alt_tab_with_one_window_is_consumed_without_opening_the_switcher() {
        let mut shell = shell();
        let id = open(&mut shell, "only");

        assert!(shell.handle_hotkey(&press(Key::Tab, Modifiers::alt())));
        assert!(!shell.alt_tab_active);
        assert_eq!(shell.focused_window, Some(id));
    }

    // ==================================================================
    // The binding table
    // ==================================================================

    /// The bug this whole table exists to prevent: `Super+Right` snapped the
    /// focused window and also swallowed `Ctrl+Super+Right`, so switching
    /// desktop by keyboard was impossible. The two chords must do two things.
    #[test]
    fn snapping_and_switching_desktops_are_different_shortcuts() {
        let mut shell = shell();
        let id = open(&mut shell, "app");
        let (wx, _, ww, _) = shell.work_area();

        assert!(shell.handle_hotkey(&press(Key::Right, super_only())));
        assert_eq!(
            shell.current_desktop, 0,
            "plain Super+Right snaps; it must not switch desktop"
        );
        // Stated as "it occupies the right half", not as a literal x. The exact
        // coordinate is `snap.rs`'s business — it depends on the zone gap, and
        // pinning it here is what made this test fail for a reason that had
        // nothing to do with the two chords it exists to keep apart.
        let w = shell.windows.get(&id).unwrap();
        assert!(shell.is_snapped(id), "and it must actually have snapped");
        assert!(
            w.x > wx + i32::try_from(ww / 2).unwrap() - 1,
            "to the right half, not the left: x={} of {ww}",
            w.x
        );
        assert_eq!(
            w.x + i32::try_from(w.width).unwrap(),
            wx + i32::try_from(ww).unwrap(),
            "flush with the right edge of the work area"
        );

        assert!(shell.handle_hotkey(&press(Key::Right, ctrl_super())));
        assert_eq!(
            shell.current_desktop, 1,
            "Ctrl+Super+Right switches desktop; it must not snap"
        );
    }

    /// A chord with a modifier the binding does not name is a different chord,
    /// and must fall through to the focused application rather than firing a
    /// shortcut the user did not ask for.
    #[test]
    fn an_extra_modifier_makes_it_a_different_chord() {
        let mut shell = shell();
        open(&mut shell, "app");

        let shift_super = Modifiers {
            shift: true,
            super_key: true,
            ..Modifiers::NONE
        };
        assert!(!shell.handle_hotkey(&press(Key::Left, shift_super)));
        assert!(!shell.handle_hotkey(&press(Key::Up, ctrl_super())));
    }

    /// A key release is never a shortcut — except the Alt that ends a window
    /// switch, which is not a chord at all.
    #[test]
    fn a_key_release_only_ends_the_window_switcher() {
        let mut shell = shell();
        open(&mut shell, "one");
        let second = open(&mut shell, "two");

        let release = KeyEvent {
            key: Key::LeftAlt,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        };
        assert!(!shell.handle_hotkey(&release), "nothing to finish yet");

        shell.start_alt_tab();
        assert!(shell.handle_hotkey(&release));
        assert!(!shell.alt_tab_active);
        assert_ne!(shell.focused_window, Some(second));
    }

    #[test]
    fn shift_alt_tab_goes_round_the_other_way() {
        let mut shell = shell();
        for i in 0..4 {
            open(&mut shell, &format!("w{i}"));
        }

        assert!(shell.handle_hotkey(&press(Key::Tab, Modifiers::alt())));
        let forwards = shell.alt_tab_index;
        assert!(shell.handle_hotkey(&press(Key::Tab, Modifiers::alt())));
        assert_eq!(shell.alt_tab_index, forwards + 1);

        let shift_alt = Modifiers {
            shift: true,
            alt: true,
            ..Modifiers::NONE
        };
        assert!(shell.handle_hotkey(&press(Key::Tab, shift_alt)));
        assert_eq!(shell.alt_tab_index, forwards);
    }

    /// Stepping backwards from a stale index must land inside the list, not on
    /// another index past the end.
    #[test]
    fn stepping_backwards_survives_the_windows_closing_underneath_it() {
        let mut shell = shell();
        let ids: Vec<WindowId> = (0..4).map(|i| open(&mut shell, &format!("w{i}"))).collect();

        shell.start_alt_tab();
        shell.next_alt_tab();
        shell.next_alt_tab();

        for id in &ids[1..] {
            shell.remove_window(*id);
        }

        shell.prev_alt_tab();
        assert!(shell.alt_tab_index < shell.visible_windows().len());
    }

    #[test]
    fn super_d_minimizes_everything_on_the_current_desktop() {
        let mut shell = shell();
        open(&mut shell, "one");
        open(&mut shell, "two");
        let elsewhere = open(&mut shell, "elsewhere");
        shell.move_window_to_desktop(elsewhere, 1);

        let super_d = Modifiers {
            super_key: true,
            ..Modifiers::NONE
        };
        assert!(shell.handle_hotkey(&press(Key::D, super_d)));

        assert!(shell.visible_windows().is_empty());
        assert!(
            shell.windows.get(&elsewhere).unwrap().visible,
            "another desktop's windows are not this shortcut's business"
        );
    }

    // ======================================================================
    // The taskbar clock
    //
    // It used to be four lines of `secs % 86400` — UTC, with no zone applied
    // — while `calendar::ClockDisplay` sat in the tree with no callers and
    // the Date & Time panel offered three settings that reached nothing.
    // ======================================================================

    /// 2026-08-18 16:30:45 UTC — a Tuesday.
    ///
    /// The comment used to say 2026-08-21, which this constant has never been:
    /// nothing asserted on the date, so a wrong date in a doc comment had
    /// nowhere to show up. The assertions below now name the weekday and the
    /// day of the month, which is what caught it.
    const INSTANT: u64 = 1_787_070_645;

    /// A shell whose clock shows the time and nothing else.
    ///
    /// The shipped default shows the weekday and date too, so a test about the
    /// *zone* would otherwise be reading three fields to check one. The tests
    /// that are about the date switches turn them back on explicitly.
    fn time_only_shell() -> DesktopShell {
        let mut shell = shell();
        shell.datetime.show_day_of_week = false;
        shell.datetime.show_date = false;
        shell
    }

    #[test]
    fn the_taskbar_clock_reads_in_the_configured_zone_not_utc() {
        let mut shell = time_only_shell();

        assert!(shell.datetime.set_timezone("UTC"));
        assert_eq!(shell.clock_string_at(INSTANT), "16:30");

        // The shipped *default* is New York, which is the whole point: out of
        // the box the corner of the screen used to read 16:30 in a zone where
        // it was half past noon.
        assert!(shell.datetime.set_timezone("America/New_York"));
        assert_eq!(
            shell.clock_string_at(INSTANT),
            "12:30",
            "August is EDT, UTC-4 — a fixed-offset entry would have said 11:30"
        );

        assert!(shell.datetime.set_timezone("Asia/Tokyo"));
        assert_eq!(
            shell.clock_string_at(INSTANT),
            "01:30",
            "UTC+9 crosses midnight into the next day"
        );
    }

    #[test]
    fn the_default_shell_does_not_show_utc() {
        // Nothing here sets a zone: this is the desktop as it first boots.
        let shell = shell();
        assert!(
            !shell.clock_string_at(INSTANT).ends_with("16:30"),
            "a fresh desktop must apply its own default zone, not fall to UTC"
        );
    }

    #[test]
    fn the_show_seconds_setting_reaches_the_taskbar_clock() {
        let mut shell = time_only_shell();
        assert!(shell.datetime.set_timezone("Atlantic/Reykjavik"));

        assert_eq!(shell.clock_string_at(INSTANT), "16:30");
        shell.datetime.show_seconds = true;
        assert_eq!(shell.clock_string_at(INSTANT), "16:30:45");
    }

    /// The two switches the Date & Time panel drew and nothing read.
    ///
    /// They were `pub` fields on `DateTimeSettings`, each documented as
    /// applying "in the taskbar clock", each rendered as a toggle row in the
    /// settings UI — and the only other reference to either was one test. The
    /// user could turn them on and off all day and the corner of the screen
    /// never changed.
    #[test]
    fn the_date_and_weekday_switches_reach_the_taskbar_clock() {
        let mut shell = shell();
        assert!(shell.datetime.set_timezone("UTC"));

        // Shipped defaults: both on.
        assert!(shell.datetime.show_day_of_week && shell.datetime.show_date);
        assert_eq!(shell.clock_string_at(INSTANT), "Tue Aug 18 16:30");

        shell.datetime.show_day_of_week = false;
        assert_eq!(shell.clock_string_at(INSTANT), "Aug 18 16:30");

        shell.datetime.show_date = false;
        shell.datetime.show_day_of_week = true;
        assert_eq!(shell.clock_string_at(INSTANT), "Tue 16:30");

        shell.datetime.show_day_of_week = false;
        assert_eq!(
            shell.clock_string_at(INSTANT),
            "16:30",
            "with everything off it is still a clock"
        );
    }

    /// The date on the taskbar is the date *in the configured zone*, taken from
    /// the same shifted instant as the time — or a clock reading just after
    /// local midnight would show yesterday beside today's hour.
    #[test]
    fn the_taskbar_date_crosses_midnight_with_the_zone() {
        let mut shell = shell();

        assert!(shell.datetime.set_timezone("UTC"));
        assert_eq!(shell.clock_string_at(INSTANT), "Tue Aug 18 16:30");

        // UTC+9: half past one the *next* morning.
        assert!(shell.datetime.set_timezone("Asia/Tokyo"));
        assert_eq!(shell.clock_string_at(INSTANT), "Wed Aug 19 01:30");
    }

    #[test]
    fn an_unresolvable_zone_falls_back_to_utc_rather_than_inventing_an_offset() {
        let mut shell = time_only_shell();
        // `set_timezone` validates, so reach past it — this is the state a
        // configuration file naming a zone we do not ship would produce.
        shell.datetime.timezone = "Mars/Olympus_Mons".to_string();
        assert!(shell.datetime.current_timezone().is_none());
        assert_eq!(shell.clock_string_at(INSTANT), "16:30");
    }

    /// A clock the tray has no room for is a setting that did not arrive.
    ///
    /// The tray used to be a flat 180 px with the clock drawn 100 px into it,
    /// which is 80 px for the reading — enough for `16:30` and not for
    /// `Tue Aug 18 16:30`. Nothing about a clipped clock says which end was
    /// cut, so the shipped default of `show_date: true` could have shown a
    /// truncated date indefinitely.
    #[test]
    fn every_reading_fits_the_slot_the_tray_reserves_for_it() {
        let mut shell = shell();
        assert!(shell.datetime.set_timezone("UTC"));

        for (dow, date, secs) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (true, true, true),
        ] {
            shell.datetime.show_day_of_week = dow;
            shell.datetime.show_date = date;
            shell.datetime.show_seconds = secs;
            let slot = shell.clock_width();

            // Three years of readings sampled every 25 hours, so the sample
            // walks through every weekday, every month and every day of the
            // month rather than landing on the same hour each time.
            for step in 0..1100_u64 {
                let t = INSTANT + step * 25 * 3600;
                let reading = shell.clock_string_at(t);
                let w = text::width(&reading, shell.font_size(TextRole::Body));
                assert!(
                    w <= slot,
                    "{reading:?} is {w} wide but the tray reserves {slot} \
                     (weekday {dow}, date {date}, seconds {secs})"
                );
            }
        }
    }

    /// The tray is sized from what is in it, so a wider clock moves the window
    /// buttons rather than running off the display.
    #[test]
    fn turning_the_date_on_widens_the_tray_and_narrows_the_buttons() {
        let mut shell = shell();
        // Enough windows that the buttons are sharing the leftover space rather
        // than sitting at their maximum width, where a narrower taskbar would
        // change nothing.
        for i in 0..24 {
            open(&mut shell, &format!("window {i}"));
        }

        shell.datetime.show_day_of_week = false;
        shell.datetime.show_date = false;
        let narrow_tray = shell.tray_width();
        let wide_buttons = shell.taskbar_button_width();

        shell.datetime.show_day_of_week = true;
        shell.datetime.show_date = true;
        assert!(
            shell.tray_width() > narrow_tray,
            "the tray must grow to hold the longer reading"
        );
        assert!(
            shell.taskbar_button_width() < wide_buttons,
            "and the space has to come from somewhere"
        );
        assert!(
            shell.tray_x() + shell.tray_width() <= shell.taskbar_rect().w + 0.5,
            "the tray still ends at the display edge"
        );
    }

    #[test]
    fn the_clock_is_the_shared_one_and_not_a_second_implementation() {
        // The bug was not that the arithmetic was wrong; it was that the shell
        // had its own. Assert the reading equals what `ClockDisplay` gives for
        // the same instant and zone, so a re-introduced private copy fails
        // here rather than in a screenshot.
        let mut shell = shell();
        assert!(shell.datetime.set_timezone("Europe/London"));
        let zone = shell.local_zone();
        // Built here from the settings rather than taken from `shell.clock()`,
        // so this also checks that `clock()` carries every switch across: a
        // comparison against the shell's own clock object would agree with
        // itself no matter which fields it forgot.
        let mut shared = crate::calendar::ClockDisplay::new();
        shared.show_seconds = shell.datetime.show_seconds;
        shared.show_day_of_week = shell.datetime.show_day_of_week;
        shared.show_date = shell.datetime.show_date;

        for t in [0_u64, INSTANT, 1_766_000_000, 4_000_000_000] {
            assert_eq!(
                shell.clock_string_at(t),
                shared.format_taskbar(t, &zone),
                "{t} rendered by the shell and by the shared clock"
            );
        }
    }
}

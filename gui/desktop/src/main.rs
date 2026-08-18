//! Slate OS Desktop Shell
//!
//! Window manager and desktop environment providing:
//! - Window management (move, resize, minimize, maximize, close)
//! - Taskbar with running application list
//! - System tray (clock, notifications, quick settings)
//! - Start menu / application launcher
//! - Virtual desktops
//! - Keyboard shortcuts (Alt+Tab, Alt+F4, Super key, etc.)
//! - Theme support
//!
//! Communicates with the compositor via IPC messages (channels).
//! Uses the guitk library for UI rendering.

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

#[allow(dead_code)]
mod a11y;
#[allow(dead_code)]
mod about;
#[allow(dead_code)]
mod accessibility_settings;
#[allow(dead_code)]
mod animations;
#[allow(dead_code)]
mod appearance_settings;
#[allow(dead_code)]
mod backup_settings;
#[allow(dead_code)]
mod bluetooth;
#[allow(dead_code)]
mod blur;
#[allow(dead_code)]
mod calendar;
#[allow(dead_code)]
mod clipboard_viewer;
#[allow(dead_code)]
mod context_ext;
#[allow(dead_code)]
mod datetime_settings;
#[allow(dead_code)]
mod default_apps;
#[allow(dead_code)]
mod device_settings;
#[allow(dead_code)]
mod display_settings;
#[allow(dead_code)]
mod file_drop;
#[allow(dead_code)]
mod focus_assist;
#[allow(dead_code)]
mod hotkeys;
#[allow(dead_code)]
mod icons;
#[allow(dead_code)]
mod input_method;
#[allow(dead_code)]
mod language_settings;
#[allow(dead_code)]
mod launcher;
#[allow(dead_code)]
mod login_screen;
#[allow(dead_code)]
mod mouse_settings;
#[allow(dead_code)]
mod multimon;
#[allow(dead_code)]
mod network_indicator;
#[allow(dead_code)]
mod network_settings;
#[allow(dead_code)]
mod notif_pane;
#[allow(dead_code)]
mod notification_settings;
#[allow(dead_code)]
mod osd;
#[allow(dead_code)]
mod overview;
#[allow(dead_code)]
mod power;
#[allow(dead_code)]
mod power_settings;
#[allow(dead_code)]
mod print_manager;
#[allow(dead_code)]
mod privacy_settings;
#[allow(dead_code)]
mod resmon;
#[allow(dead_code)]
mod run_dialog;
#[allow(dead_code)]
mod screen_capture;
#[allow(dead_code)]
mod security_dialog;
#[allow(dead_code)]
mod session_mgr;
#[allow(dead_code)]
mod snap;
#[allow(dead_code)]
mod sound_settings;
#[allow(dead_code)]
mod startup_settings;
#[allow(dead_code)]
mod storage_settings;
#[allow(dead_code)]
mod taskbar;
#[allow(dead_code)]
mod taskbar_autohide;
#[allow(dead_code)]
mod touchpad;
#[allow(dead_code)]
mod tray_dnd;
#[allow(dead_code)]
mod update_settings;
#[allow(dead_code)]
mod user_accounts;
#[allow(dead_code)]
mod wallpaper;
#[allow(dead_code)]
mod widgets;
#[allow(dead_code)]
mod window_peek;
#[allow(dead_code)]
mod window_rules;

#[cfg(test)]
mod pointer_tests;

use appearance::config;
use appearance::{AppearanceSettings, TaskbarStyle, TransparencyLevel};
use guitk::color::Color;
use guitk::cycle;
use guitk::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::RenderTree;
use guitk::style::{Border, CornerRadii, Shadow};
use launcher::{AppEntry, Category};

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

/// How many whole rows a scroll of `dy` pixels moves a list of fixed-height
/// rows.
///
/// A menu whose rows are all one height should not come to rest halfway
/// between two of them, so a delta too small to cross a row boundary still
/// moves one row rather than nothing — otherwise a mouse that reports small
/// deltas cannot scroll the menu at all.
fn scroll_rows(dy: f32) -> i32 {
    let whole = (dy / START_MENU_ROW_HEIGHT) as i32;
    if whole != 0 {
        whole
    } else if dy > 0.0 {
        1
    } else if dy < 0.0 {
        -1
    } else {
        0
    }
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
/// Where the system tray (clock, desktop indicator) begins, measured from the
/// right edge.
const TRAY_WIDTH: f32 = 180.0;
/// How much room the window buttons leave for the tray. Wider than the tray
/// itself so the last button does not end flush against the clock.
const TRAY_RESERVE: f32 = 200.0;

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

impl DesktopShell {
    pub fn new(screen_width: u32, screen_height: u32) -> Self {
        Self {
            windows: BTreeMap::new(),
            focused_window: None,
            current_desktop: 0,
            num_desktops: 4,
            screen_width,
            screen_height,
            taskbar_height: 40,
            start_menu_open: false,
            start_menu_scroll: 0,
            power_menu_open: false,
            apps: launcher::builtin_app_database(),
            alt_tab_active: false,
            alt_tab_index: 0,
            appearance: AppearanceSettings::default(),
            theme: DesktopTheme::default(),
            next_z: 1,
            next_window_id: 1,
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

    /// Where the system tray begins.
    #[must_use]
    pub fn tray_x(&self) -> f32 {
        self.taskbar_rect().w - self.scale(TRAY_WIDTH)
    }

    /// How wide each taskbar window button is.
    ///
    /// The buttons shrink as windows are opened, so this cannot be a constant
    /// in either the renderer or the hit test.
    fn taskbar_button_width(&self) -> f32 {
        let bar = self.taskbar_rect();
        let available =
            (bar.w - self.scale(START_BUTTON_WIDTH) - self.scale(TRAY_RESERVE)).max(0.0);
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
    /// first entry — the direction convention `guitk`'s grid already uses for a
    /// positive scroll delta.
    pub fn scroll_start_menu(&mut self, rows: i32) {
        let max = self.start_menu_max_scroll();
        let moved = if rows >= 0 {
            self.start_menu_scroll
                .saturating_sub(rows.unsigned_abs() as usize)
        } else {
            self.start_menu_scroll
                .saturating_add(rows.unsigned_abs() as usize)
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

        if self.start_button_rect().contains(x, y) {
            return Hit::StartButton;
        }

        if self.taskbar_rect().contains(x, y) {
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
            self.scroll_start_menu(scroll_rows(dy));
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
    pub fn move_window(&mut self, id: WindowId, x: i32, y: i32) {
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
    pub fn resize_window(&mut self, id: WindowId, width: u32, height: u32) {
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
    /// The two halves are derived from each other rather than both from
    /// `ww / 2`: the right one starts where the left one ends and runs to the
    /// work area's edge, so an odd width leaves its last column to the right
    /// window instead of leaving a one-pixel strip of desktop between them.
    pub fn snap_window(&mut self, id: WindowId, left: bool) {
        let (wx, wy, ww, wh) = self.work_area();
        let left_width = ww / 2;
        let right_width = ww.saturating_sub(left_width);
        let right_x = wx.saturating_add(i32::try_from(left_width).unwrap_or(i32::MAX));
        if let Some(w) = self.windows.get_mut(&id) {
            w.y = wy;
            w.height = wh;
            w.width = if left { left_width } else { right_width };
            w.x = if left { wx } else { right_x };
            w.state = WindowState::Normal;
            // Snapping is the user placing the window, same as moving it.
            w.restored = None;
        }
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
            self.alt_tab_index = cycle::before(count, count.saturating_sub(1));
        }
    }

    pub fn next_alt_tab(&mut self) {
        let count = self.visible_windows().len();
        if count > 0 {
            // `cycle::after` carries the "the list is not empty" condition
            // inside the expression that depends on it, and lands on the first
            // window rather than an arbitrary one if the index has gone stale
            // because windows closed while the switcher was open.
            self.alt_tab_index = cycle::after(count, self.alt_tab_index);
        }
    }

    /// Step the switcher to the previous window, for Shift+Alt+Tab.
    pub fn prev_alt_tab(&mut self) {
        let count = self.visible_windows().len();
        if let Some(last) = count.checked_sub(1) {
            // Clamping first matters: a stale index — windows closed while the
            // switcher was open — would otherwise step from one out-of-range
            // index to another rather than back into the list.
            self.alt_tab_index = cycle::before(count, self.alt_tab_index.min(last));
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
            Some(action) => {
                self.run_desktop_action(action);
                true
            }
            None => false,
        }
    }

    /// Carry out a shortcut that has already been recognised.
    fn run_desktop_action(&mut self, action: DesktopAction) {
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
        }
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

        // System tray (right side)
        let tray_x = self.tray_x();

        // Clock
        let time_str = self.current_time_string();
        tree.text(
            tray_x + self.scale(100.0),
            bar.y + self.scale(12.0),
            &time_str,
            self.theme.taskbar_fg,
            self.font_size(TextRole::Body),
        );

        // Desktop indicator
        let desk_str = format!("Desktop {}", self.current_desktop_number());
        tree.text(
            tray_x + self.scale(8.0),
            bar.y + self.scale(12.0),
            &desk_str,
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

    fn current_time_string(&self) -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let time_secs = secs % 86400;
        let hours = time_secs / 3600;
        let minutes = (time_secs % 3600) / 60;
        format!("{hours:02}:{minutes:02}")
    }
}

// ============================================================================
// Main — desktop shell entry point
// ============================================================================

fn main() {
    // In a real implementation, this would:
    // 1. Connect to the compositor via IPC channel
    // 2. Register as the window manager
    // 3. Enter an event loop processing compositor events
    // 4. Render taskbar, window decorations, and overlays
    //
    // For now, demonstrate the API:

    let mut desktop = DesktopShell::new(1920, 1080);

    // Adopt whatever the user last chose in the appearance panel. Nothing to
    // recover from if the file is absent — that is a fresh install, and the
    // defaults are what it should look like.
    desktop.load_appearance();
    println!(
        "Appearance: {} theme, {:.0}% scaling, UI font {} at {}pt",
        if desktop.appearance.theme_mode.is_light() {
            "light"
        } else {
            "dark"
        },
        desktop.appearance.scale_factor() * 100.0,
        desktop.appearance.fonts.ui_font,
        desktop.appearance.fonts.ui_size,
    );

    // Simulate some windows
    let w1 = desktop.add_window("Terminal", 100, 100, 800, 600, 1001);
    let w2 = desktop.add_window("File Explorer", 200, 150, 700, 500, 1002);
    let _w3 = desktop.add_window("Text Editor", 300, 200, 900, 650, 1003);

    // Render taskbar
    let taskbar = desktop.render_taskbar();
    println!("Taskbar rendered: {} commands", taskbar.len());

    // Render window decorations
    let decorations = desktop.render_window_decorations();
    println!("Window decorations: {} commands", decorations.len());

    // Test keyboard shortcuts
    let alt_f4 = KeyEvent {
        key: Key::F4,
        pressed: true,
        modifiers: Modifiers::alt(),
        text: None,
    };
    desktop.handle_hotkey(&alt_f4);
    println!("After Alt+F4: {} windows remaining", desktop.windows.len());

    // Open the start menu and pick Settings out of it, the way a click would.
    let start = desktop.start_button_rect();
    desktop.handle_mouse(&click(start.x + 8.0, start.y + 8.0));
    let settings_row = desktop
        .start_menu_entries()
        .iter()
        .position(|entry| entry.name == "Settings")
        .and_then(|index| index.checked_sub(desktop.start_menu_scroll));
    if let Some(row) = settings_row {
        let rect = desktop.start_menu_row_rect(row);
        match desktop.handle_mouse(&click(rect.x + 8.0, rect.y + 8.0)) {
            ShellAction::Launch(path) => println!("Start menu asked to launch: {path}"),
            other => println!("Start menu returned {other:?}"),
        }
    }

    // Open the power menu from the start menu's footer and pick Shutdown, the
    // way a user reaching for the power button would.
    desktop.handle_mouse(&click(start.x + 8.0, start.y + 8.0));
    let power = desktop.power_button_rect();
    desktop.handle_mouse(&click(power.x + 8.0, power.y + 8.0));
    let shutdown_row = desktop
        .power_menu_entries()
        .iter()
        .position(|entry| entry.name == "Shutdown");
    if let Some(row) = shutdown_row {
        let rect = desktop.power_menu_row_rect(row);
        match desktop.handle_mouse(&click(rect.x + 8.0, rect.y + 8.0)) {
            ShellAction::Launch(path) => println!("Power menu asked to launch: {path}"),
            other => println!("Power menu returned {other:?}"),
        }
    }

    // Test window snapping
    desktop.snap_window(w1, true);
    desktop.snap_window(w2, false);
    if let Some(w) = desktop.windows.get(&w1) {
        println!(
            "Window 1 snapped left: {}x{} at ({},{})",
            w.width, w.height, w.x, w.y
        );
    }

    // Test virtual desktop switching
    desktop.switch_desktop(1);
    println!(
        "Switched to desktop {}: {} visible windows",
        desktop.current_desktop_number(),
        desktop.visible_windows().len()
    );

    println!("\nDesktop shell initialized successfully.");
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

    use super::{DesktopShell, Key, KeyEvent, Modifiers, WindowId, WindowState};

    fn shell() -> DesktopShell {
        DesktopShell::new(1920, 1080)
    }

    fn open(shell: &mut DesktopShell, title: &str) -> WindowId {
        shell.add_window(title, 100, 100, 400, 300, 1)
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

    /// Both halves used to be `width / 2`, which left a one-pixel strip of
    /// desktop down the middle of any odd-width screen. The two halves must
    /// meet exactly and between them cover the whole work area, at every width
    /// including the degenerate ones.
    #[test]
    fn the_two_snapped_halves_cover_the_work_area_with_no_gap() {
        for screen_width in [1920_u32, 1921, 2560, 3, 2, 1, 0] {
            let mut shell = DesktopShell::new(screen_width, 1080);
            let left = open(&mut shell, "left");
            let right = open(&mut shell, "right");

            shell.snap_window(left, true);
            shell.snap_window(right, false);

            let (wx, wy, ww, wh) = shell.work_area();
            let l = shell.windows.get(&left).unwrap();
            let r = shell.windows.get(&right).unwrap();

            assert_eq!(l.x, wx, "width={screen_width}");
            assert_eq!(
                r.x,
                wx + i32::try_from(l.width).unwrap(),
                "the halves must meet, width={screen_width}"
            );
            assert_eq!(
                l.width + r.width,
                ww,
                "the halves must cover the work area, width={screen_width}"
            );
            for w in [l, r] {
                assert_eq!(w.y, wy, "width={screen_width}");
                assert_eq!(w.height, wh, "width={screen_width}");
                assert_eq!(w.state, WindowState::Normal);
            }
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
        assert_eq!(
            shell.windows.get(&id).unwrap().x,
            wx + i32::try_from(ww / 2).unwrap(),
            "and it must actually have snapped"
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
}

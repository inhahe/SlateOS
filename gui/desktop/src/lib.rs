//! Slate OS Desktop Shell
//!
//! Window manager and desktop environment providing:
//! - Window *control*: asking the compositor to minimize, maximize, tile, raise
//!   or close a window. Not placing one — where a window sits is decided by the
//!   compositor, and the shell is never told.
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
//! # The two halves, and which one you want
//!
//! [`DesktopShell`] is the shell's *model and appearance*: it decides what the
//! desktop looks like and what a click on it means, and it does that with no
//! connection, no display and no window system — which is what keeps every test
//! around it offline. It is *told* what windows exist
//! ([`DesktopShell::apply_window_list`]) rather than keeping its own answer, and
//! what a click or a keystroke wants done comes back out as a
//! [`ShellRequest`] — in [`ShellAction::Control`] from the pointer, in
//! [`HotkeyOutcome::requests`] from the keyboard — which is a request to be sent
//! on, not a change already made. Switching virtual desktop is one of those
//! requests and not a field the shell sets: the compositor holds each window's
//! desktop number and hides the ones filed elsewhere, so a shell that switched
//! by itself would relabel a taskbar over an unchanged screen — which is
//! exactly what it used to do.
//!
//! [`session::ShellSession`] is the loop that does the sending: it opens the
//! shell's three compositor surfaces, feeds input in, submits the render trees
//! out, and forwards the intents. That is the piece that used to be missing —
//! five public render methods whose only caller was this crate's own demo.
//! It is deliberately the only part of the crate that needs a compositor.
//!
//! # What this crate does not do yet
//!
//! Stated plainly, because assuming otherwise is how wrong code gets written
//! against this.
//!
//! - **A launch has nowhere to go.** [`ShellAction::Launch`] names a program and
//!   [`session::ShellSession`] queues it, but nothing starts a process: policy
//!   about *how* a program starts belongs to the process server, not to the
//!   window manager. See `known-issues.md`
//!   `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`.
//! - **Edge-drag tiling is not this crate's.** Super+Z opens the zone chooser
//!   and a click in it tiles the focused window; the *other* way every desktop
//!   offers the same thing — drag a window to an edge and drop — lives in the
//!   compositor, which owns the drag grab and can answer on every motion event
//!   without a round trip. The rules are `guiremote::zones::drop_at`, shared by
//!   both. The shell used to carry its own copy of them with no drag to fire on
//!   and no caller; it was deleted rather than kept as a second opinion.
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
/// The sweep that proves a module draws nothing that is immediately erased.
///
/// Test-only, like [`palette_check`]: it exists to check the other modules'
/// render output, and a release build has nothing to check.
#[cfg(test)]
pub mod draw_check;
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
/// The sweep that proves a module was converted off its own colour constants.
///
/// Test-only: it exists to check the other modules' render output, and a
/// release build has nothing to check.
#[cfg(test)]
pub mod palette_check;
pub mod power;
pub mod power_settings;
pub mod print_manager;
pub mod privacy_settings;
pub mod resmon;
pub mod run_dialog;
pub mod screen_capture;
pub mod security_dialog;
pub mod session;
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
use appearance::{
    AppearanceSettings, DecorationColors, Palette, TaskbarStyle, TransparencyLevel, emphasized,
    readable_on,
};
// The protocol's words, not its wire. `ShellControlAction` is what a taskbar
// button asks for and `WindowInfo` is what a taskbar is drawn from; re-exported
// below so a caller wiring the shell to a compositor need not name `guiremote`
// itself. `Layer` arrives with them because the list carries the shell's own
// surfaces too, and telling those apart is the whole reason the field exists.
pub use guiremote::control::{Layer, ShellControlAction};
// `WindowList` comes with it because a window's own desktop and the desktop
// being shown arrive together, in one frame, and comparing them is the only way
// to know what the user can see. Taking the windows without the header is what
// made virtual desktops a taskbar filter.
pub use guiremote::window_list::{WindowInfo, WindowList};
use guitk::color::Color;
use guitk::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::RenderTree;
use guitk::step;
use guitk::style::{Border, CornerRadii, Shadow};
use guitk::text;
use guitk::theme::with_alpha;
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

/// One window, as much of it as the shell is entitled to know.
///
/// Everything here comes from the compositor's window list except
/// [`desktop`](Self::desktop) and [`icon_id`](Self::icon_id), which are
/// shell-local and have no counterpart there. There is deliberately **no
/// geometry**: the shell does not place windows, so a position and size kept
/// here could only ever be a second, staler answer to a question the
/// compositor already answers — and was, until the fields were deleted. What
/// the shell draws about a window is a taskbar button and a switcher row,
/// neither of which is anywhere near the window itself.
#[derive(Clone, Debug)]
pub struct ManagedWindow {
    pub id: WindowId,
    pub title: String,
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
    /// Where in the stack the window sits: higher is nearer the front.
    ///
    /// Not a counter the shell keeps. It is the window's index in the list the
    /// compositor last sent, which that list emits bottom-to-top — so the
    /// shell's stacking order *is* the compositor's, and cannot drift from it
    /// between one list and the next.
    pub z_order: u32,
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
    /// A zone of the open tiling overlay, by
    /// [`snap::ZoneId`] within the layout the picker currently has selected.
    ///
    /// Carries the id and not the rectangle, for the same reason the request
    /// does: the rectangle the shell drew is a picture of the compositor's
    /// answer, not the answer, and a click that reported pixels would be asking
    /// the compositor to trust the shell's arithmetic about a display the shell
    /// does not own.
    SnapZone(snap::ZoneId),
    /// The open layout picker's panel — a thumbnail, or its own inert margin.
    ///
    /// One variant for both, because a click on either must stay on the picker:
    /// selecting is driven by which thumbnail is hovered, and a press in the
    /// margin selects nothing while still not dismissing the panel it landed
    /// on.
    SnapPicker,
    /// The tiling overlay's own space — the scrim, and the gutters between
    /// zones. A press here cancels the gesture without placing anything.
    SnapOverlay,
    /// Not the shell's: a window, or the bare desktop behind them all.
    ///
    /// One variant for both because the shell cannot tell them apart and does
    /// not need to. It knows no window's rectangle — `WindowInfo` carries none,
    /// because placing windows is the compositor's job — and there used to be a
    /// `WindowContent(WindowId)` variant that a live session could never
    /// produce: every window's geometry was zero, so it matched nothing. The
    /// compositor routes a press to the topmost window containing it and only
    /// offers the shell what landed on the shell.
    Desktop,
}

impl Hit {
    /// Whether the shell owns this pixel.
    ///
    /// Everything the shell draws it also consumes clicks on; anything else is
    /// [`Hit::Desktop`] and belongs to whatever is underneath.
    #[must_use]
    pub fn is_shell_chrome(self) -> bool {
        !matches!(self, Self::Desktop)
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
    /// Ask the compositor to act on a window the shell does not own. Implies
    /// [`Consumed`](Self::Consumed).
    ///
    /// A taskbar button does not minimise a window; it *asks* for one to be
    /// minimised. The distinction is the difference between a shell and a
    /// second window manager: the compositor owns whether a window is
    /// minimised, focused and stacked, and a shell that decided those for
    /// itself would hold a second answer that drifts from the first the moment
    /// anything else — an Alt-Tab, the window's own close button, a program
    /// exiting — changes one without telling the other.
    ///
    /// The caller sends this on as
    /// [`guiremote::control::RequestBody::ShellControl`] and learns the result
    /// the same way it learns everything else about the desktop: from the next
    /// window list, fed back in through
    /// [`apply_window_list`](DesktopShell::apply_window_list). Nothing about
    /// the shell's own state changes here, which is why a click that is refused
    /// — the window closed between the list the button was drawn from and the
    /// click — needs no undo.
    Control(ShellRequest),
}

/// Something the shell wants done to a window it does not own.
///
/// One type for both input paths on purpose. The pointer path produces these
/// singly, wrapped in [`ShellAction::Control`]; the keyboard path produces them
/// in batches, in [`HotkeyOutcome::requests`] — Super+D asks for every window on
/// the desktop to be minimised, so a shortcut cannot be limited to one. Both end
/// up at the same `control_window` call, and a second struct meaning the same
/// pair would be a second place to forget a new action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowRequest {
    /// The window to act on, in the compositor's numbering.
    pub window: WindowId,
    /// What to ask for.
    pub action: ShellControlAction,
}

impl WindowRequest {
    /// Ask for `action` on `window`.
    #[must_use]
    pub const fn new(window: WindowId, action: ShellControlAction) -> Self {
        Self { window, action }
    }
}

/// Anything the shell wants the compositor to do.
///
/// [`WindowRequest`] answered this on its own for a long time, because
/// everything a shortcut could ask for named exactly one window. Virtual
/// desktops broke that: *show desktop 3* names no window at all, and *put this
/// window on desktop 3* names one plus a number that has nowhere to live in a
/// `(window, action)` pair. The alternative — inventing a
/// `ShellControlAction::SwitchDesktop(n)` and sending it against some arbitrary
/// window — would have made the wire lie about what the request was aimed at,
/// and left "which window?" unanswerable on an empty desktop.
///
/// The desktop variants exist at all because the *compositor*, not the shell,
/// decides which desktop is showing. Before that it did not: switching desktop
/// changed which windows the taskbar *listed* and nothing else, so the windows
/// of the desktop being left stayed on screen. See `known-issues.md`
/// `TD-C-VIRTUAL-DESKTOPS-HIDE-NOTHING`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellRequest {
    /// Activate, minimise, restore, maximise, tile or close a window.
    Window(WindowRequest),
    /// Show a different virtual desktop.
    ///
    /// The compositor answers by hiding every `Layer::Normal` window filed
    /// elsewhere and handing the keyboard to the topmost one that is left — one
    /// recomposite, with no intermediate state in which half the desktop has
    /// changed. It picks the new focus itself, which is why nothing here says
    /// who should get it.
    SwitchDesktop {
        /// The desktop to show, counting from zero.
        desktop: u32,
    },
    /// File a window on a different virtual desktop.
    ///
    /// If that is the desktop showing, the window appears; if not, it
    /// disappears. Either way the shell learns which from the next window list
    /// rather than from having asked.
    MoveWindowToDesktop {
        /// The window to file.
        window: WindowId,
        /// Where to file it, counting from zero.
        desktop: u32,
    },
}

impl ShellRequest {
    /// Ask for `action` on `window` — the common case, spelt short.
    #[must_use]
    pub const fn window(window: WindowId, action: ShellControlAction) -> Self {
        Self::Window(WindowRequest::new(window, action))
    }
}

/// What a keyboard shortcut did, and what it wants the compositor to do.
///
/// The `consumed` flag is what [`DesktopShell::handle_hotkey`] used to return on
/// its own: false means no shortcut matched and the key belongs to the focused
/// window. `requests` is the half that was missing — the shortcuts that act on a
/// window used to act on the shell's *own* copy of the window list, which on a
/// live session is a copy the next
/// [`apply_window_list`](DesktopShell::apply_window_list) overwrites. Alt+F4
/// removed a taskbar button and the window stayed open.
///
/// An empty `requests` with `consumed` set is normal and means the shortcut was
/// genuinely shell-local: opening the start menu, stepping the Alt-Tab switcher,
/// dismissing a popup. Nothing here is ordered against anything else the shell
/// does — the requests are independent asks, and the compositor may refuse any
/// of them without the others being wrong.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct HotkeyOutcome {
    /// Whether the shell claimed the key. When false no window should be denied
    /// the event.
    pub consumed: bool,
    /// What to ask the compositor for, in the order the shortcut named it.
    pub requests: Vec<ShellRequest>,
}

impl HotkeyOutcome {
    /// The key was not a shortcut; pass it to the focused window.
    fn ignored() -> Self {
        Self::default()
    }

    /// The shell claimed the key and wants nothing from the compositor.
    fn consumed() -> Self {
        Self {
            consumed: true,
            requests: Vec::new(),
        }
    }

    /// The shell claimed the key and wants one thing.
    fn ask(request: Option<ShellRequest>) -> Self {
        Self {
            consumed: true,
            requests: request.into_iter().collect(),
        }
    }

    /// The shell claimed the key and wants several things.
    fn ask_all(requests: Vec<ShellRequest>) -> Self {
        Self {
            consumed: true,
            requests,
        }
    }
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
    /// The Exposé overlay: every window on every desktop, laid out to scale.
    ///
    /// Its lanes are refreshed from the same `WindowList` that
    /// [`apply_window_list`](Self::apply_window_list) folds into `windows`, in
    /// that one call, so the two cannot disagree about which desktop is showing
    /// or which windows exist. That is why the overlay's state lives on the
    /// shell rather than beside it: a copy refreshed from somewhere else would
    /// be refreshed at some other moment.
    ///
    /// Note what this does *not* put on `ManagedWindow`: the thumbnails carry
    /// rectangles and `ManagedWindow` still does not. The shell has no opinion
    /// about where a window sits (§506); the overlay is a picture drawn from the
    /// last frame and thrown away on the next one.
    pub overview: overview::OverviewState,
    /// How the overview looks — cell padding, column cap, animation length.
    pub overview_config: overview::OverviewConfig,
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
    /// The zone-tiling overlay: which layout is chosen, and whether it is up.
    ///
    /// State only. The rectangles it draws are a *picture* of what the user is
    /// choosing between; the window is placed by the compositor, from the
    /// [`SnapSlot`](snap::SnapSlot) this shell names in a
    /// [`ShellControlAction::SnapToZone`]. An earlier version of this field had
    /// the shell computing the snapped rectangle itself and returning it to a
    /// caller that could not use it — the shell moves no windows — so the
    /// geometry was computed, returned and dropped while the window stayed put.
    ///
    /// Its work area is **not** kept in sync by notification.
    /// [`screen_width`](Self::screen_width), `taskbar_height` and `appearance`
    /// are all public fields that anything may assign, and `work_area()`
    /// derives from all three, so an "update on change" scheme would be one
    /// forgotten call site away from tiling a screen size that no longer
    /// exists. [`sync_snap_area`](Self::sync_snap_area) re-seeds it at the top
    /// of every gesture that reads it instead.
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
    /// The hairline around the shell's own floating panels — the start menu,
    /// the power popup.
    ///
    /// Taken from the window frame's border rather than chosen separately, so
    /// that a panel and a window sitting side by side are outlined in the same
    /// shade. It is not a *window's* border: the shell draws no window borders.
    pub panel_border_color: Color,
    /// The emptiness behind every window, painted by the compositor.
    ///
    /// Kept here because the shell reports it — the desktop is the surface its
    /// panels are seen against — not because the shell paints it.
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
    /// The palette for a mode, before any of the user's other choices apply.
    ///
    /// Every field is a *role* read out of [`Palette`] rather than a hex
    /// value: the taskbar is `base`, its text is `text`, a pressed button is
    /// `surface1`. That is what makes the two modes structurally the same
    /// rather than the-same-if-someone-kept-two-lists-in-step, which is what
    /// they were — two hand-written tables whose correspondence was asserted
    /// only by a doc comment.
    ///
    /// The border and desktop fields are not this crate's to choose — see
    /// [`from_palette`](Self::from_palette).
    #[must_use]
    pub fn for_mode(light: bool) -> Self {
        Self::from_palette(&Palette::for_mode(light), DecorationColors::for_mode(light))
    }

    /// Which role of `palette` each surface of the shell is.
    ///
    /// Two of the twelve fields come from `frame` rather than from `palette`,
    /// and that is the point of taking both. Neither is the shell's to choose:
    /// the desktop background is painted by the compositor and merely
    /// *reported* here, and the border is the one drawn around every window on
    /// that desktop — a start menu outlined in a different shade from the
    /// window beside it looks like a bug, and would be, because two processes
    /// had each picked a colour. Deriving them from the palette here would be
    /// that second derivation. Everything else — the taskbar, the menu
    /// surfaces, the overlays — is drawn by this process alone and is a role.
    #[must_use]
    fn from_palette(p: &Palette, frame: DecorationColors) -> Self {
        Self {
            taskbar_bg: p.base,
            taskbar_fg: p.text,
            taskbar_active_bg: p.surface1,
            taskbar_accent: p.accent,
            panel_border_color: frame.border_focused,
            desktop_bg: frame.desktop_bg,
            accent_color: p.accent,
            start_menu_bg: p.base,
            start_menu_fg: p.text,
            overlay_bg: p.base,
            overlay_fg: p.text,
            overlay_selected_bg: p.surface1,
        }
    }

    /// The dark palette (Catppuccin Mocha), before any setting is applied.
    #[must_use]
    pub fn dark() -> Self {
        Self::for_mode(false)
    }

    /// The light palette (Catppuccin Latte), before any setting is applied.
    #[must_use]
    pub fn light() -> Self {
        Self::for_mode(true)
    }

    /// Resolve a full palette from what the user chose.
    ///
    /// The order matters and is the same one the settings panel presents:
    /// pick the base palette from the mode, recolour what the accent options
    /// claim, then apply transparency last — alpha is a property of a surface
    /// that has already been given its colour.
    #[must_use]
    pub fn from_settings(settings: &AppearanceSettings) -> Self {
        // The mode *and* the accent, in one resolution. Building from
        // `for_mode` and then assigning the accent over the top would work
        // today and would stop working the moment any other field came to
        // depend on the accent, because the default one would already have
        // been baked in — the shape of the defect the reintroduction proof for
        // `DecorationColors` turned up.
        let palette = Palette::from_settings(settings);
        let accent = palette.accent;
        let mut theme = Self::from_palette(&palette, DecorationColors::from_settings(settings));

        if settings.accent_taskbar {
            theme.taskbar_bg = accent;
            theme.taskbar_fg = readable_on(accent);
            theme.taskbar_active_bg = emphasized(accent);
            // The start glyph is drawn in the accent; on an accent-coloured
            // panel it has to become the contrasting colour or it disappears.
            theme.taskbar_accent = palette.on_accent();
        }

        theme.taskbar_bg = with_alpha(theme.taskbar_bg, taskbar_alpha(settings));
        let overlay = settings.transparency.panel_alpha();
        theme.overlay_bg = with_alpha(theme.overlay_bg, overlay);
        theme.start_menu_bg = with_alpha(theme.start_menu_bg, overlay);

        theme
    }
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
            overview: overview::OverviewState::new(),
            overview_config: overview::OverviewConfig::default(),
            appearance: AppearanceSettings::default(),
            theme: DesktopTheme::default(),
            datetime: datetime_settings::DateTimeSettings::default(),
            calendar: calendar::CalendarView::new(calendar::CalendarConfig::default()),
            events: calendar::EventStore::new(),
            // Placeholder: the real area needs `taskbar_rect()`, which needs
            // the appearance scaling that is only set two fields up. Seeded
            // immediately below rather than left to the first gesture, so that
            // a caller reading `shell.snap.layout()` before ever opening the
            // overlay gets the screen it is actually on.
            snap: snap::SnapManager::new(snap::WorkArea::whole_screen(0.0, 0.0)),
        };
        shell.sync_snap_area();
        shell
    }

    /// The work area as the snap module wants it.
    #[allow(
        clippy::cast_precision_loss,
        reason = "screen dimensions are far inside f32's exact-integer range"
    )]
    fn snap_area(&self) -> snap::WorkArea {
        let (x, y, width, height) = self.work_area();
        snap::WorkArea::new(x as f32, y as f32, width as f32, height as f32)
    }

    /// Re-seed the snap manager's work area from the shell's current geometry.
    ///
    /// Called at the top of every gesture that reads the zone layout. See the
    /// field's doc for why this is pull-on-use rather than push-on-change.
    ///
    /// Guarded on inequality because [`snap::SnapManager::set_work_area`]
    /// rebuilds the layout: an unconditional call would rebuild eleven
    /// rectangles on every pointer motion over the overlay to arrive at the
    /// eleven that were already there.
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
        // The tiling overlay is tested before everything else because it is
        // drawn over everything else, and because opening it closes the menus
        // (`open_zone_overlay`) — so a point that matched both would be a point
        // on a menu that is not on screen.
        //
        // It claims the work area only. The taskbar is outside that rectangle
        // by construction, so a point on the bar still reports the bar's own
        // control — which is what lets `press_on_zone_overlay` tell "abandoned
        // the choice by clicking away" from "chose a zone" without having to
        // re-derive the geometry the overlay was drawn from.
        if self.snap.is_overlay_visible() {
            if self.snap.picker_hit(x, y) {
                return Hit::SnapPicker;
            }
            if let Some(zone) = self.snap.hit_test(x, y) {
                return Hit::SnapZone(zone.id);
            }
            if self.snap.work_area().contains(x, y) {
                return Hit::SnapOverlay;
            }
        }

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

        // Anything that is not one of the shell's own surfaces is somebody
        // else's. Which window — or none — is not asked here: the compositor
        // has already decided that, and a press it hands to the shell is one it
        // decided landed on the shell.
        Hit::Desktop
    }

    /// Handle a pointer event.
    ///
    /// Returns what the caller should do with it — see [`ShellAction`].
    pub fn handle_mouse(&mut self, event: &MouseEvent) -> ShellAction {
        match event.kind {
            // The two are the same event to this shell. Nothing it draws does
            // anything on the second click that it did not do on the first:
            // double-click-to-maximize is a *title bar* gesture, and the title
            // bar belongs to the compositor, which resolves the timing itself
            // rather than being told about it here.
            MouseEventKind::Press(button) | MouseEventKind::DoubleClick(button) => {
                self.handle_press(event.x, event.y, button)
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
            // Motion is not the shell's until it grows window dragging, with
            // the one exception below; forwarding the rest is what keeps hover
            // states alive in clients.
            MouseEventKind::Move | MouseEventKind::Enter | MouseEventKind::Leave => {
                // The overview covers the screen, so while it is up nothing
                // behind it is reachable — including by a motion event, which is
                // how a client keeps its own hover states alive. Forwarding one
                // would light a button under an opaque overlay.
                if self.overview.visible {
                    let layouts = self.overview_layout();
                    overview::on_mouse_move(&mut self.overview, event.x, event.y, &layouts);
                    return ShellAction::Consumed;
                }
                // The tiling overlay is the shell's only hover-driven surface,
                // and while it is up nothing behind it can be hovered anyway.
                if self.snap.is_overlay_visible() {
                    self.sync_snap_area();
                    self.hover_zone_overlay(event.x, event.y);
                    return ShellAction::Consumed;
                }
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

    fn handle_press(&mut self, x: f32, y: f32, button: MouseButton) -> ShellAction {
        // Before `hit_test`, and before everything: the overview covers the
        // whole screen, so a press while it is up landed on it whatever the
        // taskbar geometry says. Asking `hit_test` first would let a press over
        // the strip the taskbar occupies raise a window from behind the overlay.
        if self.overview.visible {
            return self.press_on_overview(x, y, button);
        }

        self.sync_snap_area();
        let hit = self.hit_test(x, y);

        // The tiling overlay answers its own presses and nothing else's. It is
        // a modal choice — the user is picking where one window goes — so every
        // press while it is up either makes that choice or abandons it, and
        // none of the dismiss rules below can fire underneath it.
        if self.snap.is_overlay_visible() {
            return self.press_on_zone_overlay(x, y, hit, button);
        }

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
                match self.visible_windows().get(index).map(|w| w.id) {
                    Some(id) => ShellAction::Control(ShellRequest::window(
                        id,
                        // The button of the window you are already looking at
                        // minimises it — the taskbar button is a toggle, not a
                        // second way to focus what is already focused.
                        if self.focused_window == Some(id) {
                            ShellControlAction::Minimize
                        } else {
                            // `Activate`, not `Restore`: a window minimised
                            // while maximised has to come back maximised, and
                            // restoring would silently drop a state the user
                            // never asked to leave. See the compositor's
                            // `activate_window`.
                            ShellControlAction::Activate
                        },
                    )),
                    // The button is gone from under the click — the window
                    // closed between the frame it was drawn in and this press.
                    // Consumed rather than passed on: the click landed on the
                    // taskbar, and a taskbar must not leak clicks to whatever
                    // is behind it just because a button vanished.
                    None => ShellAction::Consumed,
                }
            }
            // Not the shell's pixel, so not the shell's press. It used to focus
            // the window it thought was there, which was both a guess — the
            // shell holds no window rectangles — and a change to a list the
            // next one from the compositor would overwrite.
            Hit::Desktop => ShellAction::Pass,
            // Not reachable: `hit_test` only reports these while the overlay is
            // up, and the branch at the top of this method answers every press
            // in that case. Consumed rather than `unreachable!()` because the
            // cost of being wrong is then a swallowed click rather than a dead
            // shell, and the two conditions live in different methods.
            Hit::SnapZone(_) | Hit::SnapPicker | Hit::SnapOverlay => ShellAction::Consumed,
        }
    }

    // ======================================================================
    // Zone tiling
    //
    // The shell's whole part in it: choose a tile and name it. The rectangle
    // the chosen slot resolves to is the compositor's, worked out against the
    // display the window is actually on — see `snap_window_to_zone` there, and
    // `guiremote::zones::SnapSlot` for why a name crosses the wire rather than
    // four numbers.
    // ======================================================================

    /// Open the tiling overlay over the focused window, or close it if it is
    /// already up.
    ///
    /// Returns whether it is now open. With nothing focused there is nothing to
    /// place, so the overlay does not open: a full-screen chooser whose every
    /// zone would decline the click is worse than no chooser, because only one
    /// of the two tells the user immediately that the gesture was pointless.
    pub fn toggle_zone_overlay(&mut self) -> bool {
        if self.snap.is_overlay_visible() {
            self.snap.hide_overlay();
            return false;
        }
        if self.focused_window.is_none() {
            return false;
        }
        self.sync_snap_area();
        // The overlay covers the work area and is drawn over everything, so a
        // menu left open beneath it would be a menu the user can neither see
        // nor click. Dismissed rather than drawn on top for that reason.
        self.dismiss_popups();
        self.snap.show_overlay();
        true
    }

    /// Follow the cursor while the tiling overlay is up.
    ///
    /// The layout picker is summoned by the top-edge band rather than shown
    /// with the overlay, because it is a 340×284 panel over the middle of the
    /// top of the work area and several presets put a zone's centre under it:
    /// a picker that were always up would cover the very zone the user is
    /// aiming at, and the click would change the layout instead of placing the
    /// window.
    fn hover_zone_overlay(&mut self, x: f32, y: f32) {
        if self.snap.is_in_picker_trigger(x, y) {
            self.snap.show_picker();
        } else if !self.snap.picker_hit(x, y) {
            // Leaving both the band and the panel puts it away. Asked in this
            // order so that a cursor moving *down* off the band and onto the
            // panel keeps it — the panel hangs below the band it rises from.
            self.snap.hide_picker();
        }
        // After the visibility, never before: `update_hover` gives the picker
        // precedence where the two overlap, so a hover taken first would light
        // a zone under a panel that is about to appear over it.
        self.snap.update_hover(x, y);
    }

    /// Answer one press while the tiling overlay is up.
    ///
    /// Every press either makes the choice or abandons it; nothing falls
    /// through to a window, because a modal chooser that let clicks past it
    /// would place a window *and* press a button in it.
    fn press_on_zone_overlay(
        &mut self,
        x: f32,
        y: f32,
        hit: Hit,
        button: MouseButton,
    ) -> ShellAction {
        // A non-primary press abandons the choice rather than making one.
        // Right-clicking a zone to snap into it is not a gesture any desktop
        // has, and guessing at one here would be a second way to move a window.
        if button != MouseButton::Left {
            self.snap.hide_overlay();
            return ShellAction::Consumed;
        }

        match hit {
            // Hover is re-derived from this very press rather than trusted from
            // the last motion event: a press is a position, and a pointer that
            // was warped — or a caller that reports presses without motion —
            // would otherwise select whichever thumbnail the cursor last
            // crossed.
            Hit::SnapPicker => {
                self.snap.update_hover(x, y);
                self.snap.picker_select();
                ShellAction::Consumed
            }
            Hit::SnapZone(zone_id) => match self.zone_request(zone_id) {
                Some(request) => {
                    self.snap.hide_overlay();
                    ShellAction::Control(request)
                }
                // The zone is not one the active layout has, or nothing is
                // focused any more — the window closed while the overlay was
                // up. Neither is a reason to leave a chooser on screen that
                // cannot choose.
                None => {
                    self.snap.hide_overlay();
                    ShellAction::Consumed
                }
            },
            _ => {
                self.snap.hide_overlay();
                ShellAction::Consumed
            }
        }
    }

    /// One press while the overview is up.
    ///
    /// Modal, like the tiling overlay: every press either picks a window,
    /// switches a desktop, closes a window, or abandons the overview, and none
    /// of them reach what is behind it.
    fn press_on_overview(&mut self, x: f32, y: f32, button: MouseButton) -> ShellAction {
        // A non-primary press abandons rather than picks, matching
        // `press_on_zone_overlay`. Right-clicking a thumbnail to get a menu is a
        // gesture this overlay does not have, and inventing one here would be a
        // second window menu that the taskbar's would then drift from.
        if button != MouseButton::Left {
            self.overview.hide();
            return ShellAction::Consumed;
        }
        let layouts = self.overview_layout();
        let action = overview::on_mouse_click(&mut self.overview, x, y, &layouts);
        self.act_on_overview(action)
    }

    /// Turn what the overview decided into what the session should do.
    ///
    /// Every arm is `Consumed` or better: the overview covers the screen, so
    /// there is no such thing as a press it saw and something behind it should
    /// also see.
    fn act_on_overview(&mut self, action: overview::OverviewAction) -> ShellAction {
        match action {
            overview::OverviewAction::Request(request) => ShellAction::Control(request),
            overview::OverviewAction::Close => {
                self.overview.hide();
                ShellAction::Consumed
            }
            // A press on the backdrop, an arrow key, a typed character. The
            // overlay has redrawn itself either way, which is what `Consumed`
            // buys — the session repaints on it.
            overview::OverviewAction::None
            | overview::OverviewAction::NavigateSelection
            | overview::OverviewAction::SearchChanged => ShellAction::Consumed,
        }
    }

    /// Ask for the focused window to be tiled into `zone_id` of the active
    /// layout.
    ///
    /// `None` when there is no focused window or the layout has no such zone.
    fn zone_request(&self, zone_id: snap::ZoneId) -> Option<ShellRequest> {
        let slot = self.snap.slot_for_zone(zone_id)?;
        self.request_on_focused(ShellControlAction::SnapToZone(slot))
    }

    fn handle_scroll(&mut self, x: f32, y: f32, dy: f32) -> ShellAction {
        // Before the hit test, for the reason `handle_press` is: the overview
        // covers the screen, so a wheel event while it is up is the overview's
        // wherever the pointer happens to be.
        if self.overview.visible {
            let action = overview::on_mouse_scroll(&mut self.overview, dy);
            return self.act_on_overview(action);
        }
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

    // ======================================================================
    // Window management
    //
    // What is left of it. The shell used to keep its own window list and its
    // own answers about that list: `add_window` handed out ids, `focus_window`
    // bumped a z counter, `maximize_window` computed a rectangle, `snap_window`
    // computed two. None of it survived contact with a live session, because
    // `apply_window_list` below *replaces* the list rather than merging into
    // it — so every one of those edits was overwritten by the compositor's
    // next snapshot, unread. They are gone; what remains is the door the
    // compositor's answers come in through, and the shell-local facts
    // (which virtual desktop, which icon) that nothing else holds a copy of.
    // ======================================================================

    /// Replace what the shell believes about the desktop's windows with what
    /// the compositor just said.
    ///
    /// This is the authority. Everything a taskbar shows — which buttons exist,
    /// what they are labelled, which one is lit — comes from here, because the
    /// compositor is the only thing that knows: a window can appear, be
    /// retitled, be minimised by its own program or vanish without the shell
    /// being involved at all. It is also the *only* door: there is no longer an
    /// `add_window` beside it, because a second way in meant a second answer,
    /// and the shell's own was always the one that lost.
    ///
    /// # What is deliberately dropped
    ///
    /// **Windows outside [`Layer::Normal`].** The list describes every surface
    /// on the display, the shell's own included — a taskbar that listed itself,
    /// the wallpaper and its own start menu would be mostly buttons for itself.
    /// `Layer` is the field that tells them apart, and this is the only place
    /// that reads it.
    ///
    /// # What is kept
    ///
    /// Per-window shell-local state that the compositor has no opinion about —
    /// now only the icon. A window already known keeps it across the update.
    ///
    /// **Which virtual desktop a window is on used to be kept here too**, and
    /// that was the bug. The compositor had no notion of desktops, so switching
    /// one changed which windows the taskbar listed and left every one of them
    /// on screen. The number now comes down with the window
    /// ([`WindowInfo::workspace`]) and the desktop being *shown* comes down in
    /// the list's header ([`WindowList::current_workspace`]) — both read, never
    /// remembered, because the compositor changes desktops on its own account:
    /// activating a window filed elsewhere is a switch nobody asked for.
    ///
    /// Stacking comes from the list's own order, which the compositor emits
    /// bottom-to-top, so `visible_windows().last()` is the topmost window here
    /// for the same reason it is there.
    pub fn apply_window_list(&mut self, list: &WindowList) {
        let mut kept: BTreeMap<WindowId, ManagedWindow> = BTreeMap::new();
        let mut focused = None;
        self.current_desktop = list.current_workspace;

        for (index, info) in list.windows.iter().enumerate() {
            if info.layer != Layer::Normal {
                continue;
            }
            let id = WindowId(info.id);
            let previous = self.windows.get(&id);
            if info.focused {
                focused = Some(id);
            }
            kept.insert(
                id,
                ManagedWindow {
                    id,
                    title: info.title.clone(),
                    state: if info.minimized {
                        WindowState::Minimized
                    } else if info.maximized {
                        WindowState::Maximized
                    } else {
                        WindowState::Normal
                    },
                    desktop: info.workspace,
                    focused: info.focused,
                    // Both flags have to hold: the compositor distinguishes a
                    // window that is unmapped from one that is minimised, and a
                    // taskbar button belongs to the second but not the first.
                    visible: info.visible && !info.minimized,
                    // `WindowInfo::pid` is the compositor's `u64`; the shell's
                    // field is a `u32` for the same reason a pid is one
                    // everywhere else. Truncating would make two processes
                    // indistinguishable to a "group this program's windows"
                    // feature, so it saturates instead — a pid that large is
                    // already outside anything the system can produce.
                    pid: u32::try_from(info.pid).unwrap_or(u32::MAX),
                    icon_id: previous.map_or(0, |w| w.icon_id),
                    z_order: u32::try_from(index).unwrap_or(u32::MAX),
                },
            );
        }

        self.windows = kept;
        // Taken from the list rather than preserved: the compositor is the
        // authority on focus too, and "no window is focused" is a state it can
        // genuinely be in — every window minimised, or the desktop empty.
        self.focused_window = focused;
        // In the same call, from the same frame. The overview is a second view
        // of exactly this data, and folding it here rather than at the moment
        // the overlay opens is what makes "the overview and the taskbar
        // disagree" unrepresentable: there is no second instant at which one of
        // them could have been refreshed and the other not.
        self.overview
            .apply_window_list(list, self.num_desktops.max(1));
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

    /// Ask for a different virtual desktop to be shown.
    ///
    /// **Nothing changes here.** This used to assign `self.current_desktop` and
    /// return an `Activate` for the topmost window on the new desktop, which is
    /// as far as a virtual desktop ever got: the taskbar relabelled itself, the
    /// windows of the desktop being left stayed on screen, and the one raised
    /// was raised *over* them. Hiding windows is the compositor's to do — it
    /// owns the z-stack, the scene and the keyboard — so this returns the ask
    /// and reads the result out of the next window list like everything else.
    /// Optimism would only buy a taskbar that showed the new desktop's buttons
    /// over the old desktop's windows for a frame.
    ///
    /// `None` if the desktop does not exist. *How many* there are is the
    /// shell's — it is a user preference with nothing on the wire behind it —
    /// which is why that bound is checked here and not by the compositor.
    #[must_use]
    pub const fn switch_desktop(&self, desktop: u32) -> Option<ShellRequest> {
        if desktop >= self.num_desktops {
            return None;
        }
        Some(ShellRequest::SwitchDesktop { desktop })
    }

    /// Ask for a window to be filed on a different virtual desktop.
    ///
    /// As [`switch_desktop`](Self::switch_desktop), and for the same reason:
    /// this used to edit the shell's own copy of the window's desktop number,
    /// which the next [`apply_window_list`](Self::apply_window_list) discarded
    /// unread. `None` if the desktop does not exist; a window that has closed
    /// since the list it was named from is *not* checked for, because the
    /// compositor's refusal is the only answer that cannot be stale.
    #[must_use]
    pub const fn move_window_to_desktop(
        &self,
        window: WindowId,
        desktop: u32,
    ) -> Option<ShellRequest> {
        if desktop >= self.num_desktops {
            return None;
        }
        Some(ShellRequest::MoveWindowToDesktop { window, desktop })
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

    /// Close the switcher and say which window it landed on.
    ///
    /// Returns `None` when the switcher was not open, or was open on an index
    /// that no longer names a window because it closed while the user was
    /// holding Alt. Closing the switcher is the shell's own business; raising
    /// the window it chose is the compositor's.
    pub fn finish_alt_tab(&mut self) -> Option<ShellRequest> {
        if !self.alt_tab_active {
            return None;
        }
        self.alt_tab_active = false;
        let id = self.visible_windows().get(self.alt_tab_index)?.id;
        Some(ShellRequest::window(id, ShellControlAction::Activate))
    }

    pub fn cancel_alt_tab(&mut self) {
        self.alt_tab_active = false;
    }

    // ======================================================================
    // Input handling
    // ======================================================================

    /// Handle a keyboard shortcut at the desktop level.
    ///
    /// Returns whether the key was consumed and what the shell wants done about
    /// it; see [`HotkeyOutcome`]. The caller has to send the requests on — a
    /// shortcut that acts on a window does not act on it here, for the same
    /// reason a taskbar click does not: the compositor owns which windows exist
    /// and what state they are in, and the shell finds out from the next window
    /// list like everything else.
    pub fn handle_hotkey(&mut self, key: &KeyEvent) -> HotkeyOutcome {
        if !key.pressed {
            // Key release — check for Alt+Tab completion
            if (key.key == Key::LeftAlt || key.key == Key::RightAlt) && self.alt_tab_active {
                return HotkeyOutcome::ask(self.finish_alt_tab());
            }
            return HotkeyOutcome::ignored();
        }

        // The overview gets every press before the shortcut table does, and
        // swallows the ones it does not recognise. It has a text field in it:
        // if the table went first, typing "d" into the search bar would show the
        // desktop out from under the overlay the user is typing into, and "e"
        // would open a file manager behind it. A modal surface with a text field
        // has to be modal about keys as well as clicks.
        if self.overview.visible {
            return self.key_on_overview(key);
        }

        match DesktopAction::for_chord(key.modifiers, key.key) {
            Some(action) => self.run_desktop_action(action),
            None => HotkeyOutcome::ignored(),
        }
    }

    /// One press while the overview is up.
    fn key_on_overview(&mut self, key: &KeyEvent) -> HotkeyOutcome {
        // The one shortcut that still reaches the table: the chord that opened
        // the overview closes it. Without this the binding would be one-way —
        // Super+Tab would open the overlay and then, arriving as a bare Tab,
        // cycle its mode — and a toggle you cannot press twice is a trap.
        if DesktopAction::for_chord(key.modifiers, key.key) == Some(DesktopAction::ToggleOverview) {
            return self.run_desktop_action(DesktopAction::ToggleOverview);
        }
        let Some(ok) = Self::overview_key(key) else {
            // Not a key the overview has a meaning for — a bare modifier, a
            // function key. Consumed rather than passed on, because the overlay
            // is modal: a press it did not use is not therefore the desktop's.
            return HotkeyOutcome::consumed();
        };
        let action = overview::on_key(&mut self.overview, ok);
        match self.act_on_overview(action) {
            ShellAction::Control(request) => HotkeyOutcome::ask(Some(request)),
            // `act_on_overview` returns only `Control` or `Consumed`; the other
            // two arms exist because `ShellAction` has them, not because this
            // call can produce them.
            _ => HotkeyOutcome::consumed(),
        }
    }

    /// Translate a key press into the overview's own small vocabulary.
    ///
    /// `None` for a press the overview has no meaning for. The printable case
    /// comes from [`KeyEvent::text`] rather than from mapping [`Key::A`] to
    /// `'a'`: `text` is what the keyboard layout produced, so searching works on
    /// a Dvorak or an AZERTY keyboard, and a `Key`-to-letter table would search
    /// for the character printed on a US keycap the user does not have.
    fn overview_key(key: &KeyEvent) -> Option<overview::OverviewKey> {
        use overview::OverviewKey as K;
        // Checked before `text`, because on many layouts Enter, Tab, Escape and
        // Backspace all *have* a `text` value ('\r', '\t', '\x1b', '\x08'), and
        // taking that branch first would type a control character into the
        // search box instead of acting.
        let named = match key.key {
            Key::Escape => Some(K::Escape),
            Key::Enter => Some(K::Enter),
            Key::Up => Some(K::ArrowUp),
            Key::Down => Some(K::ArrowDown),
            Key::Left => Some(K::ArrowLeft),
            Key::Right => Some(K::ArrowRight),
            Key::Backspace => Some(K::Backspace),
            Key::Tab => Some(K::Tab),
            _ => None,
        };
        if named.is_some() {
            return named;
        }
        // A held Ctrl or Alt makes the press a shortcut attempt, not typing.
        // Ctrl+C is not the letter c, and putting it in the search box would
        // both fail to copy and quietly filter the overview to nothing.
        if key.modifiers.ctrl || key.modifiers.alt || key.modifiers.super_key {
            return None;
        }
        key.text
            .filter(|ch| !ch.is_control())
            .map(overview::OverviewKey::Char)
    }

    /// Carry out a shortcut that has already been recognised.
    ///
    /// Every binding but [`DismissPopup`](DesktopAction::DismissPopup) consumes
    /// the press; that one is bare Escape, and a key the shell claims
    /// unconditionally is a key no window can ever see. Closing a dialog is what
    /// Escape does far more often than closing the start menu.
    ///
    /// The arms divide into two kinds, and the division is the whole point of
    /// the return type. The start menu, the Alt-Tab switcher's *stepping*, and
    /// popup dismissal are the shell's own surfaces and are done here. Anything
    /// naming a window — close, minimise, maximise, tile, raise — is a
    /// [`WindowRequest`] handed back for the caller to send. This method used to
    /// do the second kind itself, against the shell's private copy of the window
    /// list, which on a live session the next
    /// [`apply_window_list`](DesktopShell::apply_window_list) discards: Alt+F4
    /// removed a taskbar button and left the window open.
    fn run_desktop_action(&mut self, action: DesktopAction) -> HotkeyOutcome {
        match action {
            DesktopAction::CycleWindows => {
                if self.alt_tab_active {
                    self.next_alt_tab();
                } else {
                    self.start_alt_tab();
                }
                HotkeyOutcome::consumed()
            }
            DesktopAction::CycleWindowsBackwards => {
                if !self.alt_tab_active {
                    self.start_alt_tab();
                }
                if self.alt_tab_active {
                    self.prev_alt_tab();
                }
                HotkeyOutcome::consumed()
            }
            DesktopAction::CloseFocused => {
                HotkeyOutcome::ask(self.request_on_focused(ShellControlAction::Close))
            }
            DesktopAction::ToggleStartMenu => {
                self.toggle_start_menu();
                HotkeyOutcome::consumed()
            }
            // The one shortcut that names more than one window, and the reason
            // `handle_hotkey` cannot return a single request.
            DesktopAction::ShowDesktop => HotkeyOutcome::ask_all(
                self.windows
                    .values()
                    .filter(|w| w.visible && w.desktop == self.current_desktop)
                    .map(|w| ShellRequest::window(w.id, ShellControlAction::Minimize))
                    .collect(),
            ),
            DesktopAction::SnapLeft => {
                HotkeyOutcome::ask(self.request_on_focused(ShellControlAction::SnapLeft))
            }
            DesktopAction::SnapRight => {
                HotkeyOutcome::ask(self.request_on_focused(ShellControlAction::SnapRight))
            }
            DesktopAction::Maximize => {
                HotkeyOutcome::ask(self.request_on_focused(ShellControlAction::Maximize))
            }
            // Consumed whether or not the overlay opened. Super+Z is the
            // shell's key in either case, and letting it through to the focused
            // window on an empty desktop would make a shortcut that sometimes
            // types a `z`.
            // Consumed unconditionally, for the same reason as the zone
            // overlay: Super+Tab is the shell's chord whether or not there is
            // anything to show, and a shortcut that sometimes reaches the
            // focused window is a shortcut that sometimes types a Tab into it.
            DesktopAction::ToggleOverview => {
                self.overview.toggle(overview::OverviewMode::AllDesktops);
                HotkeyOutcome::consumed()
            }
            DesktopAction::ToggleZoneOverlay => {
                self.toggle_zone_overlay();
                HotkeyOutcome::consumed()
            }
            DesktopAction::RestoreOrMinimize => {
                // Which of the two it is depends on the state the *compositor*
                // last reported, not on anything the shell decided: Super+Down
                // un-maximizes a maximized window and minimizes an ordinary one,
                // so the same key walks a window down one step each press.
                let restore = self
                    .focused_window
                    .and_then(|id| self.windows.get(&id))
                    .is_some_and(|w| w.state == WindowState::Maximized);
                let want = if restore {
                    ShellControlAction::Restore
                } else {
                    ShellControlAction::Minimize
                };
                HotkeyOutcome::ask(self.request_on_focused(want))
            }
            DesktopAction::PreviousDesktop => {
                HotkeyOutcome::ask(self.previous_desktop().and_then(|d| self.switch_desktop(d)))
            }
            DesktopAction::NextDesktop => {
                HotkeyOutcome::ask(self.next_desktop().and_then(|d| self.switch_desktop(d)))
            }
            DesktopAction::DismissPopup => {
                if self.dismiss_popups() {
                    HotkeyOutcome::consumed()
                } else {
                    HotkeyOutcome::ignored()
                }
            }
        }
    }

    /// `action` aimed at whatever is focused, or `None` if nothing is.
    ///
    /// Every window shortcut acts on the focused window and on nothing else, so
    /// "there is no focused window" is answered once here rather than in each
    /// arm — a shortcut pressed on an empty desktop is consumed and asks for
    /// nothing, which is not the same as not being a shortcut.
    fn request_on_focused(&self, action: ShellControlAction) -> Option<ShellRequest> {
        Some(ShellRequest::window(self.focused_window?, action))
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
    /// Open (or close) the multi-zone tiling chooser for the focused window.
    ///
    /// Distinct from [`SnapLeft`](Self::SnapLeft) and its neighbours, which are
    /// one keystroke each and place the window immediately. This one opens a
    /// chooser, because there are twenty-two zones across the six layouts and
    /// no plausible set of chords for them.
    ToggleZoneOverlay,
    /// Open (or close) the Exposé overlay — every window on every desktop.
    ///
    /// Distinct from [`CycleWindows`](Self::CycleWindows), which is the same
    /// job for the common case: Alt-Tab is fast and blind, showing a strip of
    /// titles you step through without looking. This shows all of them at once,
    /// to scale, and is what you reach for when you do not remember how many
    /// presses away the window is — or which desktop it is on, which Alt-Tab
    /// cannot answer at all.
    ToggleOverview,
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
            // Super+Z, as in "zones". Super plus an arrow is already taken by
            // the four one-press placements above, and the chooser needs a key
            // that is not one of them.
            (false, false, false, true, Key::Z) => Some(Self::ToggleZoneOverlay),
            // Super+Tab, which is the chord every other desktop uses for this
            // and is not one of the four above. Alt+Tab is deliberately left
            // alone: the two are complements, not alternatives.
            (false, false, false, true, Key::Tab) => Some(Self::ToggleOverview),
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
            self.theme.panel_border_color,
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
                    color: self.theme.panel_border_color,
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

    /// How far into the local day a UTC instant is, in seconds.
    ///
    /// For anything that changes with the *time of day* rather than with the
    /// clock reading — [`wallpaper::WallpaperMode::Dynamic`], which fades a
    /// palette from dawn to night, is the one caller today.
    ///
    /// It exists rather than letting each such caller write `utc_secs % 86_400`
    /// because that expression is the exact bug
    /// [`current_clock_string`](Self::current_clock_string) documents: it is
    /// UTC, so on the shipped default zone the desktop would turn to its
    /// evening colours five hours early while the clock beside it read the
    /// correct local time. One zone answer, asked in one place.
    ///
    /// Saturates rather than wrapping on a pre-1970 instant, which cannot
    /// arrive from `SystemTime::now` and would be a nonsense time of day if it
    /// did.
    #[must_use]
    pub fn seconds_since_local_midnight(&self, utc_secs: u64) -> u64 {
        let utc = i64::try_from(utc_secs).unwrap_or(i64::MAX);
        let local = utc.saturating_add(i64::from(self.local_zone().lookup(utc).gmtoff));
        // `rem_euclid`, not `%`: a negative local instant with `%` yields a
        // negative remainder, which is not a time of day at all.
        u64::try_from(local.rem_euclid(86_400)).unwrap_or(0)
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
        let any = self.start_menu_open
            || self.power_menu_open
            || self.calendar.visible
            || self.snap.is_overlay_visible()
            || self.overview.visible;
        self.start_menu_open = false;
        self.power_menu_open = false;
        self.calendar.set_visible(false);
        self.snap.hide_overlay();
        // Escape closes the overview along with everything else. It is a
        // fullscreen overlay that covers the whole desktop, so it is the
        // *most* important thing on this list to be able to get out of: with
        // no binding for it, the only way back would be the same chord that
        // opened it, and a user who does not remember what that was is left
        // looking at a screen they cannot dismiss.
        self.overview.hide();
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

    /// Render the zone-tiling overlay, if it is open.
    ///
    /// Three layers in the order they are stacked: the zones, the highlight on
    /// the one under the cursor, and the layout picker over both. The highlight
    /// is drawn from [`snap::SnapManager::hovered_zone`] rather than from a
    /// cursor position passed in here, so that what is lit and what a press
    /// would place are the same answer to the same question.
    #[must_use]
    pub fn render_zone_overlay(&self) -> Option<RenderTree> {
        if !self.snap.is_overlay_visible() {
            return None;
        }
        let mut tree = RenderTree::new();
        tree.commands.extend(self.snap.render_overlay());
        if let Some(zone) = self.snap.hovered_zone() {
            tree.commands.extend(self.snap.render_zone_highlight(zone));
        }
        tree.commands.extend(self.snap.render_picker());
        Some(tree)
    }

    /// Render the Exposé-style overview, if it is open.
    ///
    /// The thumbnails are proportioned from the rectangles that arrived in the
    /// last window list (§519) — which is why this method exists now and could
    /// not before: with no geometry on the wire every thumbnail was zero by
    /// zero, and [`overview::compute_grid_layout`] would return a screen of
    /// cards that rasterised to no pixels and matched no click.
    #[must_use]
    pub fn render_overview(&self) -> Option<RenderTree> {
        if !self.overview.visible {
            return None;
        }
        let mut tree = RenderTree::new();
        tree.commands.extend(overview::render_overview(
            &self.overview,
            &self.overview_config,
            self.screen_width as f32,
            self.screen_height as f32,
        ));
        Some(tree)
    }

    /// Where each overview thumbnail is on screen, for hit-testing a click.
    ///
    /// Deliberately the same call [`Self::render_overview`] draws from, rather
    /// than a second computation that agrees with it today.
    #[must_use]
    pub fn overview_layout(&self) -> Vec<overview::ThumbnailLayout> {
        overview::overview_layout(
            &self.overview,
            &self.overview_config,
            self.screen_width as f32,
            self.screen_height as f32,
        )
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
    fn every_surface_of_the_theme_is_a_role_out_of_the_shared_palette() {
        // The shell must not have its own idea of what a background is. This
        // crate held two hand-written colour tables — one per mode — and the
        // only thing claiming they described the same surfaces was a comment;
        // `gui/desktop` then went on to grow 549 more constants for the same
        // reason (known-issues.md
        // `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`).
        //
        // Asserting the mapping rather than the values is what makes the test
        // survive a palette change: recolouring Catppuccin would break a test
        // that named hexes, and should not, whereas a taskbar that quietly
        // stopped being `base` is exactly what should fail here.
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let t = DesktopTheme::for_mode(light);
            assert_eq!(t.taskbar_bg, p.base);
            assert_eq!(t.taskbar_fg, p.text);
            assert_eq!(t.taskbar_active_bg, p.surface1);
            assert_eq!(t.taskbar_accent, p.accent);
            assert_eq!(t.accent_color, p.accent);
            assert_eq!(t.start_menu_bg, p.base);
            assert_eq!(t.start_menu_fg, p.text);
            assert_eq!(t.overlay_bg, p.base);
            assert_eq!(t.overlay_fg, p.text);
            assert_eq!(t.overlay_selected_bg, p.surface1);
            // The two the compositor also draws stay the frame's, not the
            // palette's read a second time — see `from_palette`.
            let frame = DecorationColors::for_mode(light);
            assert_eq!(t.panel_border_color, frame.border_focused);
            assert_eq!(t.desktop_bg, frame.desktop_bg);
        }
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

    /// The two colours this shell shares with the compositor are the
    /// compositor's, whatever the settings say.
    ///
    /// The shell draws no window frames — the compositor does — but it still
    /// draws *against* them: its panels are outlined in the frame border and
    /// sit on the desktop background. Those two it must read rather than
    /// choose, or the panel beside a window is outlined in a different shade
    /// from the window. The test fails the moment someone reintroduces a
    /// literal here, which is the natural place to put one and the wrong one.
    #[test]
    fn the_shell_reads_its_frame_colours_rather_than_choosing_them() {
        for &accent in AccentColor::presets() {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                for accent_titlebars in [false, true] {
                    let mut s = settings();
                    s.theme_mode = mode;
                    s.accent_color = accent;
                    s.accent_titlebars = accent_titlebars;

                    let theme = DesktopTheme::from_settings(&s);
                    let frame = DecorationColors::from_settings(&s);
                    let what = format!("{mode:?} {accent:?} accent_titlebars={accent_titlebars}");

                    assert_eq!(theme.panel_border_color, frame.border_focused, "{what}");
                    assert_eq!(theme.desktop_bg, frame.desktop_bg, "{what}");
                }
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

/// Tests for what the shell still decides about windows: which virtual desktop
/// they are on, which one the switcher lands on, and what a keyboard shortcut
/// asks the compositor for.
///
/// Shorter than it was, deliberately. This module used to test a *private
/// window manager*: `add_window` minting ids, `focus_window` bumping a z
/// counter, `maximize_window` and `snap_window` computing rectangles. All of it
/// worked in isolation and none of it was reachable, because a live shell
/// learns what exists from [`DesktopShell::apply_window_list`], which
/// **replaces** its window list — so every edit those methods made was
/// overwritten, unread, by the compositor's next snapshot. The code is gone,
/// and with it the tests that pinned it; the same behaviour is pinned in
/// `compositor`, where it is what actually runs.
///
/// Everything below therefore reaches the shell the way a live session does:
/// through a window list. The helpers at the top build one, and standing in for
/// "the compositor did as it was asked" is another list, which is exactly how a
/// real session finds out.
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

    use super::{
        DesktopShell, HotkeyOutcome, Key, KeyEvent, ManagedWindow, Modifiers, ShellControlAction,
        ShellRequest, TextRole, WindowId, WindowInfo, WindowList, WindowState, text,
    };

    fn shell() -> DesktopShell {
        DesktopShell::new(1920, 1080)
    }

    /// One window turned back into the description it arrived as.
    fn info(window: &ManagedWindow) -> WindowInfo {
        let mut info = WindowInfo::new(window.id.0, u64::from(window.pid), window.title.clone());
        info.minimized = window.state == WindowState::Minimized;
        info.maximized = window.state == WindowState::Maximized;
        info.focused = window.focused;
        // Round-tripped, because it is the compositor's field now: a helper
        // that dropped it would move every window to desktop 0 on the next
        // list, and the desktop tests below would pass by accident.
        info.workspace = window.desktop;
        info
    }

    /// What the shell currently believes, in the compositor's own order, ready
    /// to be handed back with one thing changed.
    ///
    /// Every helper below builds on this rather than calling a method on the
    /// shell — there is no longer a method to call, and a live session never
    /// had one. The list is the only thing that moves the shell's idea of the
    /// desktop.
    fn as_list(shell: &DesktopShell) -> Vec<WindowInfo> {
        let mut windows: Vec<&ManagedWindow> = shell.windows.values().collect();
        // The compositor emits bottom-to-top, which is where `z_order` came
        // from in the first place.
        windows.sort_by_key(|window| window.z_order);
        windows.into_iter().map(info).collect()
    }

    /// A window opens: one more entry, on top and holding focus, which is what
    /// a newly-mapped window is.
    ///
    /// The id is the compositor's to choose; this stands in for it by taking
    /// the next one this shell has not seen. Ids must not repeat — `windows` is
    /// keyed by them, so a repeat would silently merge two windows.
    fn open(shell: &mut DesktopShell, title: &str) -> WindowId {
        let id = WindowId(
            shell
                .windows
                .keys()
                .map(|id| id.0)
                .max()
                .map_or(1, |top| top + 1),
        );
        let mut list = as_list(shell);
        for other in &mut list {
            other.focused = false;
        }
        let mut fresh = WindowInfo::new(id.0, 1, title);
        fresh.focused = true;
        // Where a window the user just opened belongs: the desktop they are
        // looking at. The compositor is what decides that in a live session.
        fresh.workspace = shell.current_desktop;
        list.push(fresh);
        shell.apply_window_list(&WindowList::new(shell.current_desktop, list));
        id
    }

    /// The compositor closed a window, and focused whatever was under it.
    fn close(shell: &mut DesktopShell, id: WindowId) {
        let mut list: Vec<WindowInfo> = as_list(shell)
            .into_iter()
            .filter(|info| info.id != id.0)
            .collect();
        if let Some(top) = list.last_mut() {
            top.focused = true;
        }
        shell.apply_window_list(&WindowList::new(shell.current_desktop, list));
    }

    /// The compositor raised a window to the front and gave it the keyboard —
    /// what it does when it grants an `Activate`.
    fn raise(shell: &mut DesktopShell, id: WindowId) {
        let mut list = as_list(shell);
        let Some(at) = list.iter().position(|info| info.id == id.0) else {
            panic!("no window {id:?} to raise");
        };
        let mut window = list.remove(at);
        window.focused = true;
        window.minimized = false;
        for other in &mut list {
            other.focused = false;
        }
        list.push(window);
        shell.apply_window_list(&WindowList::new(shell.current_desktop, list));
    }

    /// The compositor maximized a window.
    fn maximize(shell: &mut DesktopShell, id: WindowId) {
        let mut list = as_list(shell);
        for info in &mut list {
            if info.id == id.0 {
                info.maximized = true;
            }
        }
        shell.apply_window_list(&WindowList::new(shell.current_desktop, list));
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
    // Stacking, which is the compositor's order and nothing else
    // ==================================================================

    #[test]
    fn a_new_window_opens_above_the_existing_ones_and_takes_focus() {
        let mut shell = shell();
        let first = open(&mut shell, "first");
        let second = open(&mut shell, "second");

        assert!(z_of(&shell, second) > z_of(&shell, first));
        assert_eq!(shell.focused_window, Some(second));
        assert_eq!(shell.visible_windows().last().map(|w| w.id), Some(second));
    }

    /// The shell's stacking *is* the list's order. It used to be a counter the
    /// shell bumped on every focus change — a second answer to a question the
    /// compositor had already answered, and the one that lost every time the
    /// two disagreed.
    #[test]
    fn the_stacking_order_is_the_one_the_list_arrived_in() {
        let mut shell = shell();
        let bottom = open(&mut shell, "bottom");
        let middle = open(&mut shell, "middle");
        let top = open(&mut shell, "top");

        raise(&mut shell, bottom);

        let raised = z_of(&shell, bottom);
        assert!(raised > z_of(&shell, middle));
        assert!(raised > z_of(&shell, top));
        assert_eq!(shell.visible_windows().last().map(|w| w.id), Some(bottom));
        assert!(!shell.windows.get(&middle).unwrap().focused);
    }

    // ==================================================================
    // Virtual desktops
    //
    // Which desktop is showing is the compositor's answer, never the shell's
    // decision: every test here presses the key, checks what was *asked*, and
    // then plays the compositor's part. A test that asserted on
    // `shell.current_desktop` straight after the keystroke would be asserting
    // the old bug -- a taskbar that relabelled itself over an unchanged screen.
    // ==================================================================

    /// The compositor granted a switch: the same windows, a new desktop showing.
    ///
    /// It takes the keyboard off a window it has just hidden -- its own tested
    /// behaviour, modelled here so a shell test can rely on it.
    fn compositor_switched(shell: &mut DesktopShell, desktop: u32) {
        let mut list = as_list(shell);
        for info in &mut list {
            if info.workspace != desktop {
                info.focused = false;
            }
        }
        shell.apply_window_list(&WindowList::new(desktop, list));
    }

    /// The compositor granted a move: the window is filed on another desktop.
    fn compositor_moved(shell: &mut DesktopShell, id: WindowId, desktop: u32) {
        let showing = shell.current_desktop;
        let mut list = as_list(shell);
        for info in &mut list {
            if info.id == id.0 {
                info.workspace = desktop;
                info.focused = info.focused && desktop == showing;
            }
        }
        shell.apply_window_list(&WindowList::new(showing, list));
    }

    /// Which desktop a shortcut asked for, or `None` if it asked for nothing.
    fn desktop_asked(outcome: &HotkeyOutcome) -> Option<u32> {
        match outcome.requests.as_slice() {
            [ShellRequest::SwitchDesktop { desktop }] => Some(*desktop),
            [] => None,
            other => panic!("expected at most one desktop switch, got {other:?}"),
        }
    }

    #[test]
    fn desktop_navigation_stops_at_both_ends() {
        let mut shell = shell();
        let last = shell.num_desktops - 1;

        assert_eq!(shell.previous_desktop(), None);
        let outcome = shell.handle_hotkey(&press(Key::Left, ctrl_super()));
        assert!(
            outcome.consumed,
            "the chord is the shell's whether or not there is a desktop to go to"
        );
        assert_eq!(
            desktop_asked(&outcome),
            None,
            "there is nothing left of the first desktop to ask for"
        );

        for expected in 1..=last {
            let outcome = shell.handle_hotkey(&press(Key::Right, ctrl_super()));
            assert!(outcome.consumed);
            assert_eq!(desktop_asked(&outcome), Some(expected));
            assert_eq!(
                shell.current_desktop,
                expected - 1,
                "asking is not arriving: nothing moves until the next list"
            );
            compositor_switched(&mut shell, expected);
            assert_eq!(shell.current_desktop, expected);
        }

        assert_eq!(shell.next_desktop(), None);
        let outcome = shell.handle_hotkey(&press(Key::Right, ctrl_super()));
        assert!(outcome.consumed);
        assert_eq!(desktop_asked(&outcome), None);
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
        for key in [Key::Left, Key::Right] {
            let outcome = shell.handle_hotkey(&press(key, ctrl_super()));
            assert!(outcome.consumed);
            assert_eq!(desktop_asked(&outcome), None);
        }
        assert_eq!(shell.current_desktop, 0);
    }

    /// How many desktops there are is the shell's, and this is the only place
    /// the bound is enforced. The compositor takes any `u32`: a desktop with
    /// nothing on it is a legal thing to show, and the count is a user
    /// preference the compositor has never been told.
    #[test]
    fn a_desktop_that_does_not_exist_is_not_asked_for() {
        let shell = shell();
        assert_eq!(shell.switch_desktop(shell.num_desktops), None);
        assert_eq!(shell.switch_desktop(u32::MAX), None);
    }

    #[test]
    fn the_desktop_indicator_counts_from_one() {
        let mut shell = shell();
        assert_eq!(shell.current_desktop_number(), 1);

        assert_eq!(
            shell.switch_desktop(2),
            Some(ShellRequest::SwitchDesktop { desktop: 2 })
        );
        assert_eq!(
            shell.current_desktop_number(),
            1,
            "the ask is not the answer -- the screen has not changed yet"
        );

        compositor_switched(&mut shell, 2);
        assert_eq!(shell.current_desktop_number(), 3);
    }

    #[test]
    fn a_window_is_only_visible_on_its_own_desktop() {
        let mut shell = shell();
        let id = open(&mut shell, "app");

        compositor_switched(&mut shell, 1);
        assert!(shell.visible_windows().is_empty());
        assert_eq!(
            shell.focused_window, None,
            "the window that had the keyboard is not on screen to have it"
        );

        compositor_moved(&mut shell, id, 1);
        assert_eq!(
            shell
                .visible_windows()
                .iter()
                .map(|w| w.id)
                .collect::<Vec<_>>(),
            vec![id]
        );
    }

    /// The inverse of what this used to assert, and the whole of the fix.
    ///
    /// Which desktop a window is on used to be shell-local -- the compositor
    /// had no notion of desktops -- so a new list had to leave it alone. It is
    /// the compositor's now, so a new list is exactly what sets it. That is
    /// what lets the *screen* change when the taskbar does.
    #[test]
    fn a_window_list_is_what_says_which_desktop_a_window_is_on() {
        let mut shell = shell();
        let id = open(&mut shell, "app");
        assert_eq!(shell.windows.get(&id).unwrap().desktop, 0);

        compositor_moved(&mut shell, id, 1);
        assert_eq!(shell.windows.get(&id).unwrap().desktop, 1);
        assert!(
            shell.visible_windows().is_empty(),
            "and it is no longer on the desktop being shown"
        );

        // And back, with the shell never having asked: another shell, or the
        // compositor answering an activation, can move a window too.
        compositor_moved(&mut shell, id, 0);
        assert_eq!(shell.windows.get(&id).unwrap().desktop, 0);
    }

    #[test]
    fn a_window_cannot_be_moved_to_a_desktop_that_does_not_exist() {
        let shell = shell();
        let id = WindowId(1);
        let last = shell.num_desktops - 1;

        assert_eq!(shell.move_window_to_desktop(id, shell.num_desktops), None);
        assert_eq!(shell.move_window_to_desktop(id, u32::MAX), None);
        assert_eq!(
            shell.move_window_to_desktop(id, last),
            Some(ShellRequest::MoveWindowToDesktop {
                window: id,
                desktop: last
            })
        );
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
        assert_eq!(
            shell.finish_alt_tab(),
            Some(ShellRequest::window(first, ShellControlAction::Activate)),
        );
        // Standing in for the compositor doing as it was asked. The switcher
        // *asks* for the window to be raised; nothing about the shell's own
        // focus has moved at the point of the assertion above, which is the
        // whole difference between this and what it used to do.
        raise(&mut shell, first);

        shell.start_alt_tab();
        assert_eq!(
            shell.finish_alt_tab(),
            Some(ShellRequest::window(second, ShellControlAction::Activate)),
            "and back again"
        );
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

        close(&mut shell, ids[1]);
        close(&mut shell, ids[2]);

        shell.next_alt_tab();
        assert!(shell.alt_tab_index < shell.visible_windows().len());

        assert_eq!(
            shell.finish_alt_tab(),
            Some(ShellRequest::window(ids[0], ShellControlAction::Activate)),
            "the one window left is the one it lands on"
        );
        assert!(!shell.alt_tab_active);
    }

    #[test]
    fn alt_tab_on_an_empty_desktop_does_nothing() {
        let mut shell = shell();

        shell.start_alt_tab();
        assert!(!shell.alt_tab_active);

        shell.next_alt_tab();
        assert_eq!(shell.finish_alt_tab(), None);
        assert_eq!(shell.focused_window, None);
    }

    /// A single window is not worth a switcher, but the keystroke must still be
    /// consumed rather than falling through to the focused app.
    #[test]
    fn alt_tab_with_one_window_is_consumed_without_opening_the_switcher() {
        let mut shell = shell();
        let id = open(&mut shell, "only");

        let outcome = shell.handle_hotkey(&press(Key::Tab, Modifiers::alt()));
        assert!(outcome.consumed);
        assert!(outcome.requests.is_empty(), "and asks for nothing");
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

        // Which pixels the halves occupy is not asserted here any more, because
        // the shell no longer decides: it names the edge and the compositor
        // works the rectangle out from its own bounds. That the two halves tile
        // the display exactly is `compositor`'s
        // `the_two_snapped_halves_tile_the_display_with_no_seam`.
        let right = shell.handle_hotkey(&press(Key::Right, super_only()));
        assert!(right.consumed);
        assert_eq!(
            right.requests,
            vec![ShellRequest::window(id, ShellControlAction::SnapRight)],
            "plain Super+Right tiles the focused window"
        );
        assert_eq!(
            shell.current_desktop, 0,
            "plain Super+Right snaps; it must not switch desktop"
        );

        let left = shell.handle_hotkey(&press(Key::Left, super_only()));
        assert_eq!(
            left.requests,
            vec![ShellRequest::window(id, ShellControlAction::SnapLeft)],
            "and Super+Left tiles it the other way, not the same way"
        );

        let switch = shell.handle_hotkey(&press(Key::Right, ctrl_super()));
        assert!(switch.consumed);
        assert_eq!(
            desktop_asked(&switch),
            Some(1),
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
        assert!(!shell.handle_hotkey(&press(Key::Left, shift_super)).consumed);
        assert!(!shell.handle_hotkey(&press(Key::Up, ctrl_super())).consumed);
    }

    /// A key release is never a shortcut — except the Alt that ends a window
    /// switch, which is not a chord at all.
    #[test]
    fn a_key_release_only_ends_the_window_switcher() {
        let mut shell = shell();
        let first = open(&mut shell, "one");
        let second = open(&mut shell, "two");

        let release = KeyEvent {
            key: Key::LeftAlt,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        };
        let idle = shell.handle_hotkey(&release);
        assert!(!idle.consumed, "nothing to finish yet");
        assert!(idle.requests.is_empty());

        shell.start_alt_tab();
        let finished = shell.handle_hotkey(&release);
        assert!(finished.consumed);
        assert!(!shell.alt_tab_active);
        assert_eq!(
            finished.requests,
            vec![ShellRequest::window(first, ShellControlAction::Activate)],
            "the window it landed on, which is not the one already focused"
        );
        assert_eq!(shell.focused_window, Some(second), "not yet, anyway");
    }

    #[test]
    fn shift_alt_tab_goes_round_the_other_way() {
        let mut shell = shell();
        for i in 0..4 {
            open(&mut shell, &format!("w{i}"));
        }

        assert!(
            shell
                .handle_hotkey(&press(Key::Tab, Modifiers::alt()))
                .consumed
        );
        let forwards = shell.alt_tab_index;
        assert!(
            shell
                .handle_hotkey(&press(Key::Tab, Modifiers::alt()))
                .consumed
        );
        assert_eq!(shell.alt_tab_index, forwards + 1);

        let shift_alt = Modifiers {
            shift: true,
            alt: true,
            ..Modifiers::NONE
        };
        assert!(shell.handle_hotkey(&press(Key::Tab, shift_alt)).consumed);
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
            close(&mut shell, *id);
        }

        shell.prev_alt_tab();
        assert!(shell.alt_tab_index < shell.visible_windows().len());
    }

    #[test]
    fn super_d_minimizes_everything_on_the_current_desktop() {
        let mut shell = shell();
        let one = open(&mut shell, "one");
        let two = open(&mut shell, "two");
        let elsewhere = open(&mut shell, "elsewhere");
        compositor_moved(&mut shell, elsewhere, 1);

        let super_d = Modifiers {
            super_key: true,
            ..Modifiers::NONE
        };
        let outcome = shell.handle_hotkey(&press(Key::D, super_d));
        assert!(outcome.consumed);

        let mut asked: Vec<WindowId> = outcome
            .requests
            .iter()
            .map(|request| match request {
                ShellRequest::Window(w) => {
                    assert_eq!(w.action, ShellControlAction::Minimize);
                    w.window
                }
                other => panic!("Super+D names windows and nothing else, got {other:?}"),
            })
            .collect();
        asked.sort_unstable();
        assert_eq!(
            asked,
            vec![one, two],
            "every window on this desktop, and only those — another desktop's \
             windows are not this shortcut's business"
        );
    }

    /// The shortcut that names more than one window is the reason
    /// [`HotkeyOutcome`] carries a list, so an empty desktop has to come back
    /// consumed-and-empty rather than not-a-shortcut.
    #[test]
    fn super_d_on_an_empty_desktop_is_still_a_shortcut() {
        let mut shell = shell();
        let outcome = shell.handle_hotkey(&press(
            Key::D,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ));
        assert!(outcome.consumed);
        assert!(outcome.requests.is_empty());
    }

    /// Every shortcut that acts on a window acts on the focused one, so with
    /// nothing focused each must be claimed and ask for nothing — not fall
    /// through to an application that is not there.
    #[test]
    fn a_window_shortcut_with_nothing_focused_asks_for_nothing() {
        for (key, modifiers) in [
            (Key::F4, Modifiers::alt()),
            (Key::Left, super_only()),
            (Key::Right, super_only()),
            (Key::Up, super_only()),
            (Key::Down, super_only()),
        ] {
            let mut shell = shell();
            let outcome = shell.handle_hotkey(&press(key, modifiers));
            assert!(outcome.consumed, "{key:?} must still be claimed");
            assert!(outcome.requests.is_empty(), "{key:?} must ask for nothing");
        }
    }

    /// Super+Down walks a window down one step per press: a maximized window
    /// un-maximizes, anything else minimizes. Which it is depends on the state
    /// the compositor last reported, so it is read and not decided.
    #[test]
    fn super_down_restores_a_maximized_window_and_minimizes_any_other() {
        let mut shell = shell();
        let id = open(&mut shell, "app");

        assert_eq!(
            shell
                .handle_hotkey(&press(Key::Down, super_only()))
                .requests,
            vec![ShellRequest::window(id, ShellControlAction::Minimize)],
        );

        maximize(&mut shell, id);
        assert_eq!(
            shell
                .handle_hotkey(&press(Key::Down, super_only()))
                .requests,
            vec![ShellRequest::window(id, ShellControlAction::Restore)],
        );
    }

    /// Alt+F4 asks; it does not close. The shell's own list is unchanged until
    /// the compositor sends the next one, which is what makes a refusal — the
    /// program showing a "save changes?" dialog — need no undo here.
    #[test]
    fn alt_f4_asks_the_compositor_and_changes_nothing_itself() {
        let mut shell = shell();
        let id = open(&mut shell, "app");

        let outcome = shell.handle_hotkey(&press(Key::F4, Modifiers::alt()));
        assert_eq!(
            outcome.requests,
            vec![ShellRequest::window(id, ShellControlAction::Close)],
        );
        assert!(
            shell.windows.contains_key(&id),
            "the window is the compositor's to remove, and it has not answered yet"
        );
    }

    /// Switching desktop names no window, and that is the change.
    ///
    /// This used to assert that the shell picked the topmost window on the
    /// desktop it was arriving at and asked for it to be activated -- which was
    /// the closest a virtual desktop ever got to working, and was wrong twice
    /// over: the window was raised *over* the windows of the desktop being
    /// left, which nothing hid, and the shell had to guess a focus target from
    /// a list it did not own. The compositor hides the one and chooses the
    /// other, in the same recomposite, and says so in the next list.
    #[test]
    fn switching_desktop_names_no_window() {
        let mut shell = shell();
        let stays = open(&mut shell, "stays");
        let moves = open(&mut shell, "moves");
        compositor_moved(&mut shell, moves, 1);

        let outcome = shell.handle_hotkey(&press(Key::Right, ctrl_super()));
        assert_eq!(
            outcome.requests,
            vec![ShellRequest::SwitchDesktop { desktop: 1 }],
            "one ask, naming a desktop and no window"
        );
        assert_eq!(
            shell
                .visible_windows()
                .iter()
                .map(|w| w.id)
                .collect::<Vec<_>>(),
            vec![stays],
            "and nothing has moved: the screen still shows desktop 0"
        );

        // The compositor did it, and picked the focus itself.
        let mut list = as_list(&shell);
        for info in &mut list {
            info.focused = info.id == moves.0;
        }
        shell.apply_window_list(&WindowList::new(1, list));
        assert_eq!(shell.current_desktop, 1);
        assert_eq!(shell.focused_window, Some(moves));
        assert_eq!(
            shell
                .visible_windows()
                .iter()
                .map(|w| w.id)
                .collect::<Vec<_>>(),
            vec![moves]
        );

        let back = shell.handle_hotkey(&press(Key::Left, ctrl_super()));
        assert_eq!(
            back.requests,
            vec![ShellRequest::SwitchDesktop { desktop: 0 }]
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

/// The overview, as the rest of the shell sees it.
///
/// Everything here is about the *seams*: that the same window list refreshes
/// the overview and the taskbar, that a rectangle survives the trip from the
/// wire to a thumbnail, that a press while the overlay is up cannot reach past
/// it, and that the layout a click is tested against is the layout that was
/// drawn. The overview's own behaviour — grids, search, navigation — is tested
/// in `overview.rs` beside the code that implements it.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]
mod overview_wiring_tests {
    use super::{
        DesktopShell, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
        RenderTree, ShellAction, ShellControlAction, ShellRequest, WindowId, WindowInfo,
        WindowList, overview,
    };
    use guitk::render::RenderCommand;

    fn shell() -> DesktopShell {
        DesktopShell::new(1920, 1080)
    }

    /// One window, placed, on desktop `workspace`.
    fn placed(id: u64, title: &str, workspace: u32, rect: (i32, i32, u32, u32)) -> WindowInfo {
        let mut info = WindowInfo::new(id, 1, title.to_string()).at(rect.0, rect.1, rect.2, rect.3);
        info.workspace = workspace;
        info
    }

    fn press(shell: &mut DesktopShell, x: f32, y: f32) -> ShellAction {
        shell.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn key(k: Key, modifiers: Modifiers, text: Option<char>) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text,
        }
    }

    fn super_tab() -> KeyEvent {
        key(
            Key::Tab,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
            None,
        )
    }

    // -- The seam between the window list and the overview -------------------

    #[test]
    fn the_list_that_refreshes_the_taskbar_refreshes_the_overview() {
        // There is one call, in `apply_window_list`, and this is why: two
        // refreshes at two instants is how the overview comes to show a window
        // the taskbar has already dropped. The shell should not be able to hold
        // one opinion about which windows exist.
        let mut s = shell();
        s.apply_window_list(&WindowList::new(
            0,
            vec![
                placed(1, "Terminal", 0, (0, 0, 800, 600)),
                placed(2, "Editor", 0, (100, 100, 640, 480)),
            ],
        ));
        let titles: Vec<&str> = s.overview.lanes[0]
            .thumbnails
            .iter()
            .map(|t| t.title.as_str())
            .collect();
        assert_eq!(titles, ["Terminal", "Editor"]);
        assert_eq!(
            titles.len(),
            s.visible_windows().len(),
            "the overview and the taskbar disagree about how many windows there are"
        );
    }

    #[test]
    fn a_thumbnail_carries_the_window_s_real_rectangle() {
        // The whole reason §519 put geometry on the wire. If the projection
        // dropped it, every thumbnail would be zero by zero, `fit_aspect` would
        // return `(0.0, 0.0)`, and the overview would be a screen of cards that
        // rasterise to no pixels and match no click.
        let mut s = shell();
        s.apply_window_list(&WindowList::new(
            0,
            vec![placed(1, "Placed", 0, (-100, 250, 1024, 768))],
        ));
        let thumb = &s.overview.lanes[0].thumbnails[0];
        assert_eq!((thumb.x, thumb.y), (-100.0, 250.0));
        assert_eq!((thumb.width, thumb.height), (1024.0, 768.0));
    }

    #[test]
    fn a_thumbnail_with_a_real_rectangle_lays_out_to_real_pixels() {
        // Stated separately from the field-by-field check above, because that
        // one would still pass if `compute_grid_layout` threw the numbers away.
        // This is the claim that actually matters on screen: something is drawn.
        let mut s = shell();
        s.apply_window_list(&WindowList::new(
            0,
            vec![placed(1, "Placed", 0, (0, 0, 1024, 768))],
        ));
        s.overview.show(overview::OverviewMode::AllWindows);
        let layouts = s.overview_layout();
        assert_eq!(layouts.len(), 1);
        assert!(
            layouts[0].render_width > 1.0 && layouts[0].render_height > 1.0,
            "the thumbnail laid out to {}x{}",
            layouts[0].render_width,
            layouts[0].render_height
        );
        // The aspect ratio of the window it stands for, not the cell's.
        let ratio = layouts[0].render_width / layouts[0].render_height;
        assert!(
            (ratio - 1024.0 / 768.0).abs() < 0.01,
            "aspect ratio {ratio} is not the window's"
        );
    }

    #[test]
    fn every_desktop_gets_a_lane_even_the_empty_ones() {
        // Lanes come from the shell's desktop count, not from the windows that
        // happen to exist. Deriving them from the windows would make an empty
        // desktop invisible in the very screen whose job is to show you where
        // everything is — and there would be nothing to drag a window onto.
        let mut s = shell();
        s.num_desktops = 4;
        s.apply_window_list(&WindowList::new(
            0,
            vec![placed(1, "Only", 2, (0, 0, 8, 8))],
        ));
        assert_eq!(s.overview.lanes.len(), 4);
        assert_eq!(s.overview.lanes[2].thumbnails.len(), 1);
        assert!(s.overview.lanes[0].thumbnails.is_empty());
        assert!(s.overview.lanes[3].thumbnails.is_empty());
    }

    #[test]
    fn a_window_hovered_in_the_overview_stops_being_hovered_when_it_closes() {
        // The hover is a window id held across frames, which makes it the one
        // piece of overview state that can outlive the window it names. Pressing
        // Enter on a stale one would ask the compositor to raise a window that
        // is gone — harmless — but it would also *draw* a highlight on a card
        // that is no longer there, which is not.
        let mut s = shell();
        s.apply_window_list(&WindowList::new(
            0,
            vec![
                placed(1, "Going", 0, (0, 0, 800, 600)),
                placed(2, "Staying", 0, (0, 0, 800, 600)),
            ],
        ));
        s.overview.hovered_window = Some(1);
        s.apply_window_list(&WindowList::new(
            0,
            vec![placed(2, "Staying", 0, (0, 0, 800, 600))],
        ));
        assert_eq!(s.overview.hovered_window, None);
    }

    // -- Modality ------------------------------------------------------------

    #[test]
    fn a_press_over_the_taskbar_does_not_reach_it_while_the_overview_is_up() {
        // The overview covers the whole screen, so the taskbar is behind it.
        // `hit_test` answers from geometry alone and does not know that, which
        // is why the overview is consulted first: ask it second and a click on
        // the strip the taskbar occupies raises a window from behind an opaque
        // overlay.
        let mut s = shell();
        s.apply_window_list(&WindowList::new(
            0,
            vec![placed(1, "Terminal", 0, (0, 0, 800, 600))],
        ));
        let button = s.taskbar_button_rect(0);
        let (x, y) = (button.x + button.w / 2.0, button.y + button.h / 2.0);
        // The control this is contrasted against: with the overview closed, the
        // same press is the taskbar's and asks for something.
        assert!(
            matches!(press(&mut s, x, y), ShellAction::Control(_)),
            "the test's premise is wrong: that press is not a taskbar button"
        );

        s.overview.show(overview::OverviewMode::AllWindows);
        assert_eq!(press(&mut s, x, y), ShellAction::Consumed);
    }

    #[test]
    fn typing_in_the_overview_search_does_not_run_a_desktop_shortcut() {
        // The overview has a text field in it. If the shortcut table saw keys
        // first, typing would fire whatever the letters happen to be bound to —
        // behind the overlay the user is typing into, where they cannot see it.
        let mut s = shell();
        s.overview.show(overview::OverviewMode::AllWindows);
        let outcome = s.handle_hotkey(&key(Key::E, Modifiers::NONE, Some('e')));
        assert!(outcome.consumed);
        assert!(outcome.requests.is_empty());
        assert_eq!(s.overview.search_query, "e");
    }

    #[test]
    fn the_chord_that_opens_the_overview_closes_it() {
        // Super+Tab arrives at an open overview as a Tab, which the overview
        // spends on cycling its own mode. Without the toggle being checked
        // first the binding is one-way, and a toggle you cannot press twice is
        // a trap: the only way out would be a key the user has to guess.
        let mut s = shell();
        assert!(s.handle_hotkey(&super_tab()).consumed);
        assert!(s.overview.visible);
        assert!(s.handle_hotkey(&super_tab()).consumed);
        assert!(!s.overview.visible);
    }

    #[test]
    fn escape_leaves_the_overview() {
        let mut s = shell();
        s.overview.show(overview::OverviewMode::AllWindows);
        assert!(
            s.handle_hotkey(&key(Key::Escape, Modifiers::NONE, None))
                .consumed
        );
        assert!(!s.overview.visible);
    }

    // -- Drawing and hit-testing are one answer ------------------------------

    #[test]
    fn a_click_selects_the_window_whose_card_is_under_it() {
        // The click point comes out of the *render tree*, not out of
        // `overview_layout`. That distinction is the whole test. Asking
        // `overview_layout` where a card is and then clicking there proves only
        // that a function agrees with itself: transpose the screen inside it and
        // both the question and the answer move together, so the assertion holds
        // just as well against a layout that has nothing to do with what is on
        // the glass. (Measured — the earlier version of this test passed
        // unchanged against the `overviewclickrelayout` marker, which swaps
        // width for height in the hit-test path.)
        //
        // Reading the coordinate back off the drawn commands is the only way to
        // ask the question the user asks: I clicked the middle of the card I can
        // see — did that select the window whose title is written on it?
        let mut s = shell();
        s.apply_window_list(&WindowList::new(
            0,
            vec![
                placed(1, "First", 0, (0, 0, 800, 600)),
                placed(2, "Second", 0, (0, 0, 800, 600)),
                placed(3, "Third", 0, (0, 0, 800, 600)),
            ],
        ));
        s.overview.show(overview::OverviewMode::AllWindows);

        // The middle card, so that an off-by-one layout lands on a neighbour
        // rather than off the edge where it would miss and be caught anyway.
        let tree = s.render_overview().expect("the overview is open");
        let (cx, cy) = drawn_card_centre(&tree, "Second");
        assert_eq!(
            press(&mut s, cx, cy),
            ShellAction::Control(ShellRequest::window(
                WindowId(2),
                ShellControlAction::Activate
            ))
        );
    }

    /// The centre of the card `title` is written on, taken from the drawn
    /// commands rather than from any layout function.
    ///
    /// `render_thumbnail_card` emits the card's background `FillRect` and then
    /// the `Text` holding its title, so the nearest `FillRect` above a title is
    /// that title's card. This is deliberately a reader of the output and not a
    /// second caller of `overview_layout`: a test that recomputes the layout it
    /// is checking cannot fail when the layout is wrong.
    fn drawn_card_centre(tree: &RenderTree, title: &str) -> (f32, f32) {
        let mut last_rect = None;
        for cmd in &tree.commands {
            match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => last_rect = Some((*x, *y, *width, *height)),
                RenderCommand::Text { text, .. } if text == title => {
                    let (x, y, w, h) = last_rect.expect("a card is drawn before its title");
                    return (x + w / 2.0, y + h / 2.0);
                }
                _ => {}
            }
        }
        panic!("no card drawn for {title}");
    }

    #[test]
    fn the_overview_is_drawn_only_while_it_is_open() {
        let mut s = shell();
        assert!(s.render_overview().is_none());
        s.overview.show(overview::OverviewMode::AllWindows);
        let tree = s.render_overview().expect("an open overview draws");
        assert!(
            !tree.commands.is_empty(),
            "an open overview drew no commands"
        );
    }

    #[test]
    fn dismissing_popups_dismisses_the_overview() {
        // Anything that clears the shell's transient surfaces has to clear this
        // one too, or the overlay survives a desktop switch and covers the
        // desktop it switched to.
        let mut s = shell();
        s.overview.show(overview::OverviewMode::AllWindows);
        s.dismiss_popups();
        assert!(!s.overview.visible);
    }
}

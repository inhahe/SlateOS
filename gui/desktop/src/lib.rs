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
//! - **A launch is started by this crate's binary, not by a process server.**
//!   [`ShellAction::Launch`] names a program and its arguments,
//!   [`session::ShellSession`] queues it, and the `desktop` binary spawns it
//!   (`drain` in `main.rs`). That spawn inherits the shell's own environment
//!   and privileges, which is acceptable on a development host and not on
//!   SlateOS: policy about *how* a program starts belongs to the process
//!   server, and nothing here has a channel to one yet. See `known-issues.md`
//!   `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`. (This bullet said until
//!   2026-09-25 that nothing started a process at all, which stopped being
//!   true on 2026-09-13.)
//! - **Edge-drag tiling is not this crate's.** Super+Z opens the zone chooser
//!   and a click in it tiles the focused window; the *other* way every desktop
//!   offers the same thing — drag a window to an edge and drop — lives in the
//!   compositor, which owns the drag grab and can answer on every motion event
//!   without a round trip. The rules are `guiremote::zones::drop_at`, shared by
//!   both. The shell used to carry its own copy of them with no drag to fire on
//!   and no caller; it was deleted rather than kept as a second opinion.
//!
//! (A third bullet here said the 49 modules beside [`DesktopShell`] each drew
//! from a private hardcoded palette and ignored the user's theme. That was
//! fixed on 2026-08-24 --
//! `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE` -- and
//! the bullet outlived it by a month.)

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

pub mod about;
pub mod animations;
pub mod bluetooth;
pub mod calendar;
pub mod clipboard_viewer;
pub mod context_ext;
pub mod datetime_settings;
pub mod device_settings;
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
pub mod idle_lock;
pub mod input_method;
pub mod language_settings;
pub mod launcher;
pub mod login_screen;
pub mod multimon;
pub mod network_indicator;
pub mod network_settings;
pub mod notif_pane;
pub mod osd;
pub mod overview;
/// The sweep that proves a module was converted off its own colour constants.
///
pub mod power;
pub mod power_settings;
pub mod print_manager;
pub mod resmon;
pub mod run_dialog;
pub mod screen_capture;
pub mod security_dialog;
pub mod session;
pub mod session_mgr;
pub mod shortcut_editor;
/// The horizontal value slider every settings panel draws, in one place.
///
/// Five panels drew it by hand and disagreed about the thumb's colour; one of
/// them inked it the same accent as the fill underneath it. The thumb is now
/// `text` — and deliberately *not* derived from the fill, because unlike a
/// switch knob it overhangs its track.
pub mod slider;
pub mod snap;
pub mod sound_settings;
pub mod startup_settings;
pub mod storage_settings;
/// The on/off switch every settings panel draws, in one place.
///
/// Seventeen panels drew it by hand and all seventeen filled the knob with
/// `p.text`, which on an accent track is 1.35:1 against it. The knob is now
/// derived from the track it sits on.
pub mod switch;
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
use guitk::menu::{ContextMenu, MenuAction, MenuItem, MenuItemId};

use crate::widgets::{DesktopWidgetManager, WidgetInstanceId, WidgetKind};
use appearance::{
    AppearanceSettings, DecorationColors, Palette, TaskbarStyle, TransparencyLevel, emphasized,
    readable_on,
};
// The protocol's words, not its wire. `ShellControlAction` is what a taskbar
// button asks for and `WindowInfo` is what a taskbar is drawn from; re-exported
// below so a caller wiring the shell to a compositor need not name `guiremote`
// itself. `Layer` arrives with them because the list carries the shell's own
// surfaces too, and telling those apart is the whole reason the field exists.
pub use guiremote::control::{Layer, ShellControlAction, StackTier, WindowPolicy};
// `WindowList` comes with it because a window's own desktop and the desktop
// being shown arrive together, in one frame, and comparing them is the only way
// to know what the user can see. Taking the windows without the header is what
// made virtual desktops a taskbar filter.
pub use guiremote::window_list::{WindowInfo, WindowList};
use guitk::color::Color;
use guitk::event::{
    EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
use guitk::render::RenderTree;
use guitk::step;
use guitk::style::{Border, CornerRadii, Shadow};
use guitk::text;
use guitk::textedit::{self, SingleLine};
use guitk::textinput::{KeyEdit, TextInput};
use guitk::theme::with_alpha;
use guitk::wheel;
use hotkeys::HotkeyAction;
use launcher::{AppEntry, Category};
// The same zone engine the libc's `localtime`, osh's `printf '%(…)T'`, the
// calendar panel and the Date & Time settings page render through, so the
// taskbar cannot disagree with `date` about what time it is.
use tzrules::Tz;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ============================================================================
// Geometry
// ============================================================================

/// The file the pinned applications live in.
const TASKBAR_CONFIG_NAME: &str = "taskbar";

/// The file the programs pinned to the start menu live in.
const START_MENU_CONFIG_NAME: &str = "startmenu";

/// The toolkit's rectangle, re-exported so the shell and its widgets share
/// one. This crate declared an identical copy -- same four floats, same
/// half-open `contains`, documented with the same reasoning -- until
/// 2026-09-17. See `known-issues.md`
/// `TD-C-TEN-RECTANGLE-TYPES-IN-THREE-SPELLINGS`.
pub use guitk::frame::Rect;

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
/// Gap between the last pinned button and the first window's, with the
/// divider drawn in its middle: `design.txt` asks for "a small space and a
/// divider between the two sections". Only while both sections have buttons.
const TASKBAR_SECTION_GAP: f32 = 13.0;
/// How strongly the divider between the sections is drawn: the bar's own
/// text colour at this alpha, so it follows the theme and stays quieter than
/// anything that can be clicked.
const TASKBAR_DIVIDER_ALPHA: u8 = 80;
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
/// Width of the notification bell's slot in the tray.
///
/// A fixed square rather than a measured one: the bell is a glyph, not a
/// reading, so nothing about it changes width when the clock's switches do.
/// The unread badge is drawn *inside* this slot for the same reason — a count
/// that widened the tray as notifications arrived would shuffle the window
/// buttons sideways every time something was posted.
const TRAY_BELL_WIDTH: f32 = 24.0;

/// How wide one application tray icon's slot is.
///
/// The same as the bell, so the tray reads as a row of equal things rather
/// than a ragged line -- and fixed rather than measured, so that a program
/// changing its glyph cannot reflow the tray and move every window button
/// sideways.
const TRAY_ICON_SLOT: f32 = 24.0;

/// The most of the taskbar the application icons may occupy.
///
/// **Without this the taskbar is destroyable by any program that can connect.**
/// Measured on a 1920-wide bar before the cap existed: eighty icons made
/// `tray_width` 2167, so `tray_x` clamped to zero, the icon run covered the
/// whole bar including the clock at x=1805, and every window button was
/// computed at *zero* width -- no way to switch windows, on a shell whose only
/// window switcher that is. Nothing rationed it: `MAX_TRAY_ICONS` is 4096 and
/// a program picks its own ids.
///
/// A share of the bar rather than a fixed count, because the question is how
/// much room the *taskbar* can spare and that is a width. A fixed twelve would
/// still crowd a 1024-wide netbook and would waste two thirds of a 4K bar.
const TRAY_ICON_SHARE: f32 = 0.25;

/// The glyph for "there are more icons than fit".
const TRAY_OVERFLOW_GLYPH: &str = "\u{2039}";

/// A press on a tray icon, in flight.
struct TrayDrag {
    /// Threshold and click-versus-drag bookkeeping.
    source: tray_dnd::TrayDragSource,
    /// Which button went down, so the release reports the same one.
    ///
    /// The tray is the one place in this shell where the right button is a
    /// first-class gesture rather than a context menu the shell itself owns:
    /// the icon belongs to another program, and what right-clicking it means
    /// is that program's to decide. `ClickTrayIcon` has carried a button
    /// since it was defined, for exactly this.
    button: MouseButton,
}
/// Opacity of an icon while it is being dragged.
///
/// Faint enough to read as "this one is in flight" and solid enough to still
/// be identifiable -- the user is dragging it because they know which one it
/// is, and a ghost they cannot recognise is worse than none.
const GHOST_ALPHA: u8 = 110;

/// How far the shortcut card's outcome line sits from its left and bottom
/// edges.
///
/// One constant for both, so the message is inset by the same amount it is
/// lifted and cannot drift into a corner as the card resizes.
const SHORTCUT_MESSAGE_INSET: f32 = 20.0;

/// How far the shortcut card stays from the top and bottom of the screen.
///
/// A card that reaches the display's edges reads as a mode the desktop has
/// entered rather than as a sheet laid over it, and the shadow it draws has
/// nowhere to fall. One constant, because the card's layout, its placement and
/// the editor's page size all derive from the room it leaves.
const SHORTCUT_CARD_MARGIN: f32 = 48.0;

/// What the card's bottom line says when nothing has happened yet: the keys it
/// answers. Without it the editor's keys are discoverable only by reading the
/// source, which is the fate of every keyboard feature nobody is told about.
const SHORTCUT_CARD_HINT: &str = "Enter: new keys \u{b7} F2: change action \u{b7} Insert: add \u{b7} Delete: remove \u{b7} Esc: close";

/// The bell the tray draws when nothing is being silenced.
///
/// Not read by the renderer, which asks the focus manager for the glyph of
/// whatever mode is in force; this is the same codepoint that
/// [`focus_assist::FocusMode::Off`] answers, kept so a test can say *which*
/// glyph "not silencing anything" is without asserting it against the very
/// function under test. `focus_assist` and `widgets` use the same codepoint, so
/// the desktop has one bell rather than three.
#[cfg(test)]
const NOTIF_BELL_GLYPH: &str = "\u{1F514}";
/// Extra room the window buttons leave beyond the tray, so the last button does
/// not end flush against the desktop indicator.
const TRAY_RESERVE_GAP: f32 = 20.0;

// --- Start menu ------------------------------------------------------------

const START_MENU_WIDTH: f32 = 300.0;
const START_MENU_HEIGHT: f32 = 400.0;
/// Space above the first application row, holding the "Applications" heading.
const START_MENU_TOP_PADDING: f32 = 50.0;
const START_MENU_ROW_HEIGHT: f32 = 36.0;
/// How strongly the start menu marks the row the keyboard is on: the accent
/// at this alpha, under the row's own text.
const START_MENU_SELECTED_ALPHA: u8 = 70;
/// How strongly the start menu draws a hint -- the empty search field's
/// "Type to search", and what Enter will do when nothing is found: the
/// menu's text colour at this alpha, quieter than anything that can be
/// chosen.
const START_MENU_HINT_ALPHA: u8 = 150;
/// Space below the last application row, holding the power options.
const START_MENU_FOOTER: f32 = 48.0;
/// Width of the scroll indicator drawn when the list is longer than the menu.
const START_MENU_SCROLLBAR_WIDTH: f32 = 4.0;

// --- Power menu ------------------------------------------------------------

/// Width of the power button in the start menu's footer.
const POWER_BUTTON_WIDTH: f32 = 110.0;
/// Inset of the power button from the menu's left and bottom edges.
const POWER_BUTTON_INSET: f32 = 8.0;
/// Widest a start-menu footer button beside Power gets.
const START_SHORTCUT_WIDTH: f32 = 80.0;
/// Gap between the footer's buttons.
const START_SHORTCUT_GAP: f32 = 6.0;
const POWER_MENU_WIDTH: f32 = 170.0;
const POWER_MENU_ROW_HEIGHT: f32 = 32.0;
/// Space above the first and below the last row of the popup.
const POWER_MENU_PADDING: f32 = 6.0;
/// Gap between the power button and the popup that rises from it.
const POWER_MENU_GAP: f32 = 6.0;
/// Distance from a popup row's left edge to the start of its label.
const POWER_MENU_TEXT_INSET: f32 = 14.0;

// --- The Run box's file chooser --------------------------------------------

/// How wide the chooser the Browse button raises is drawn.
///
/// Wider than the Run box it covers, and deliberately so: the box is a single
/// line and the chooser is a list with a sidebar, a path bar and four columns.
/// Sized so that the name column still has room after the sidebar takes its
/// fixed share, rather than by matching anything else on the desktop.
const RUN_BROWSER_WIDTH: f32 = 640.0;
/// How tall the chooser is drawn. Enough rows to scan a `/bin` without
/// scrolling being the only way to see anything.
const RUN_BROWSER_HEIGHT: f32 = 440.0;

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
/// What a pin menu was opened on.
///
/// The two places pinning can be reached from name the same program in
/// different ways, and neither can be converted to the other: the start menu
/// knows a row of its own list, the taskbar knows a slot of the pinned list,
/// and the pinned list is a filtered, reordered thing. Carrying *which kind*
/// is what stops a row index being read as a pin index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PinTarget {
    /// A row of the start menu, which may or may not already be pinned.
    StartMenuRow(usize),
    /// An application already pinned, by index into the pinned list.
    Pinned(usize),
}

/// A button in the start menu's footer beside Power, starting a program the
/// start menu is asked to keep at hand (`design.txt` line 721: "start menu,
/// contains applications tree, settings icon, terminal, power off, ...").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartShortcut {
    /// The settings application.
    Settings,
    /// A terminal.
    Terminal,
}

impl StartShortcut {
    /// Both, in the order they stand, left to right.
    pub const ALL: &'static [Self] = &[Self::Settings, Self::Terminal];

    /// The program the button starts.
    #[must_use]
    pub const fn program(self) -> &'static str {
        match self {
            Self::Settings => launcher::SETTINGS,
            Self::Terminal => launcher::TERMINAL,
        }
    }

    /// The word on the button. Words rather than icons: the UI face is not
    /// guaranteed to have a gear, and a box where a gear should be says
    /// nothing at all.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Terminal => "Terminal",
        }
    }
}

/// Where a program carried from the start menu, the taskbar or the desktop
/// would go if it were let go at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CarryTarget {
    /// Pinned to the taskbar, at the gap nearest the pointer.
    Taskbar,
    /// A shortcut to it on the desktop, where it was let go.
    Desktop,
    /// Pinned to the start menu: among its pinned rows where it was let go
    /// on them, after them when it was let go on the start button.
    StartMenu,
}

/// A window's taskbar button pressed and perhaps being dragged.
struct WindowPress {
    /// The press and its drag threshold, keyed by the window.
    source: tray_dnd::DragSource<WindowId>,
    /// Whether the window was the focused one when the button was pressed --
    /// which decides what a click does: a toggle minimises the window in
    /// front and summons any other. Taken at the press, because pressing the
    /// bar can itself move the focus before the release arrives, and a click
    /// on the front window's button must not turn into "summon" on the way.
    was_focused: bool,
}

/// A start-menu row pressed and perhaps being dragged.
struct StartDrag {
    /// The press and its drag threshold, keyed by the program's path -- the
    /// same source the pinned buttons and the tray use.
    source: tray_dnd::DragSource<String>,
    /// The program's name, as the start menu shows it: what a pin or a
    /// desktop shortcut made from it is called.
    name: String,
}

/// What one taskbar button stands for.
///
/// The taskbar used to show one button per window and nothing else, so an
/// index into it was an index into [`taskbar_windows`](DesktopShell::taskbar_windows).
/// Pinned applications share the same run of buttons, so the index is now into
/// *this* -- and it is a named enum rather than a bare `usize` for the reason
/// `Hit` is: a number that means "the third button" is read as "the third
/// window" by whoever forgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarSlot {
    /// An open window.
    Window(WindowId),
    /// An application pinned to the taskbar, by index into the pinned list.
    Pinned(usize),
}

#[derive(Clone, Debug)]
pub struct ManagedWindow {
    pub id: WindowId,
    pub title: String,
    /// Which program the window belongs to, as that program declares it.
    ///
    /// Read from the compositor's list on every update and never remembered
    /// across one, for the same reason the title is not: the compositor is the
    /// authority on what a window says about itself. Empty means the window
    /// named no program.
    ///
    /// This is what [`crate::window_rules`] matches on, and the
    /// reason it can match anything useful: a rule keyed on
    /// [`title`](Self::title) would stop applying the moment the user saved the
    /// document under a new name.
    pub app_id: String,
    /// Whether a window rule asked for this window to have no taskbar button.
    ///
    /// Shell-local, and carried across window lists the way the icon is rather
    /// than re-read the way the title is: the rule that set it fired once, when
    /// the window arrived, and there is nothing in the compositor's list that
    /// could confirm or deny it afterwards. Re-deriving it per list would mean
    /// re-running the rules per list, which is the thing
    /// [`WindowRulesManager::evaluate`](crate::window_rules::WindowRulesManager::evaluate)
    /// must not have done to it.
    pub skip_taskbar: bool,
    /// Whether a window rule asked for this window to be left out of Alt+Tab.
    ///
    /// Separate from [`skip_taskbar`](Self::skip_taskbar) because the two are
    /// separate rule actions and users mean different things by them: a chat
    /// window docked to the edge might want no button but must still be
    /// reachable by keyboard, and a background helper the reverse.
    pub skip_alt_tab: bool,
    pub state: WindowState,
    pub desktop: u32,
    /// Whether this window has focus.
    pub focused: bool,
    /// Whether the compositor has this window **mapped** — i.e. it exists on
    /// the desktop as something the user can get back to.
    ///
    /// A *minimised* window is still mapped. That distinction is the whole
    /// point of the field: the compositor reports "unmapped" (the program took
    /// its window away) and "minimised" (the user put it away) as two separate
    /// flags, and only the first means the window is gone. Ask
    /// [`on_glass`](Self::on_glass) for the narrower question of whether it is
    /// currently *drawn*.
    ///
    /// This field used to be called `visible` and used to fold the two
    /// together, which is what stranded every minimised window — see
    /// [`taskbar_windows`](DesktopShell::taskbar_windows).
    pub mapped: bool,
    /// Process ID owning this window.
    pub pid: u32,
    /// Icon ID (index into icon registry).
    pub icon_id: u32,
    /// Where the window is, in screen pixels, decorations included, as the
    /// compositor last reported it.
    ///
    /// Asked by [`window_at`](DesktopShell::window_at) alone, and for one
    /// question: whether a program carried from the start menu was let go
    /// over somebody's window rather than on the desktop. The compositor
    /// routes presses by what is on top, so nothing else in the shell needs
    /// to know where a window is -- and nothing here decides where one goes.
    pub frame: Rect,
    /// Where in the stack the window sits: higher is nearer the front.
    ///
    /// Not a counter the shell keeps. It is the window's index in the list the
    /// compositor last sent, which that list emits bottom-to-top — so the
    /// shell's stacking order *is* the compositor's, and cannot drift from it
    /// between one list and the next.
    pub z_order: u32,
}

impl ManagedWindow {
    /// Whether the window is being **drawn** right now.
    ///
    /// Narrower than [`mapped`](Self::mapped): a minimised window is mapped but
    /// not on the glass. Only one caller wants this question — "Show Desktop",
    /// which minimises what is on screen and has nothing to say to a window
    /// already put away. Everything else the shell lists (taskbar buttons, the
    /// Alt+Tab switcher, the overview) wants `mapped`, because the entire
    /// purpose of those lists is to get a put-away window *back*.
    #[must_use]
    pub fn on_glass(&self) -> bool {
        self.mapped && self.state != WindowState::Minimized
    }
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
    /// One of the buttons beside it -- Settings, Terminal.
    StartMenuShortcut(StartShortcut),
    /// An entry of the open power menu, by index into
    /// [`power_menu_entries`](DesktopShell::power_menu_entries).
    PowerMenuEntry(usize),
    /// The open power menu, but not one of its rows.
    PowerMenuPanel,
    /// A window button on the taskbar, carrying **the window it stands for**.
    ///
    /// The id and not the slot number. A slot is a position on a bar whose
    /// contents change, so a hit that carried one would have to be resolved
    /// against the window list a second time, by the code that acts on it —
    /// and that second lookup can fail, which is a state the shell then has to
    /// have an answer for. It had one (`None => Consumed`, with a comment
    /// about the window closing under the click) and the answer was
    /// unreachable: `hit_test` reads the same list in the same call, so a slot
    /// it reports is a slot that was occupied a microsecond ago and still is.
    /// Naming the window makes the impossible state unrepresentable instead of
    /// merely unvisited, and it makes the hit test's own assertion mean
    /// something — "the button drawn third belongs to the third window" rather
    /// than "the third button is the third button".
    TaskbarButton(WindowId),
    /// A pinned application's button, by index into the pinned list.
    ///
    /// An index and not an executable path, because the path is the pinned
    /// list's business and a `Hit` that carried one would be a second place
    /// holding it.
    TaskbarPinned(usize),
    /// The taskbar panel, but not one of its controls.
    TaskbarPanel,
    /// The tray clock, which opens the calendar popup.
    Clock,
    /// The tray's notification bell, which opens the notification pane.
    NotificationBell,
    /// The chevron at the left of the icon run, which lists the icons the
    /// bar had no room for.
    TrayOverflow,
    /// An application's tray icon, by its index in
    /// [`DesktopShell::tray_icons`].
    ///
    /// An index rather than an id, for the reason `TaskbarButton` gives about
    /// window slots: the list that produced the rectangle is in hand at the
    /// moment of the hit, and resolving later would resolve against a list
    /// that may have changed.
    TrayIcon(usize),
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
    /// Start a program, with its arguments. Implies
    /// [`Consumed`](Self::Consumed).
    ///
    /// A [`hotkeys::Launch`], program and arguments, rather than a bare path:
    /// until 2026-09-25 this carried only a program, so the one thing a desktop
    /// icon exists for -- opening a folder or a document, which means starting
    /// a program *with that path* -- could not be said at all, and a folder
    /// icon asked the operating system to execute the folder. The program is a
    /// `PathBuf` and each argument an `OsString`, for the reason given on
    /// `Launch`: a program the user *pointed at* may have no UTF-8 spelling,
    /// and a lossy one would name a different program or none.
    Launch(hotkeys::Launch),
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

impl ShellAction {
    /// Whether the shell did anything with the event: everything but
    /// [`Pass`](Self::Pass). The question a caller that only repaints on a
    /// change asks, and the one [`DesktopShell::activate_desktop_menu_item`]
    /// used to answer with a `bool` before one of its items could start a
    /// program.
    #[must_use]
    pub const fn changed(&self) -> bool {
        !matches!(self, Self::Pass)
    }
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
    /// Tell the compositor that the user clicked a tray icon, so it can tell
    /// the program that registered it.
    ///
    /// The shell is the only thing that can raise this: it owns the strip the
    /// icons are drawn in and did the hit test. `owner` and `id` are copied
    /// from the tray list, not invented.
    ClickTrayIcon {
        /// The process that registered the icon.
        owner: u64,
        /// That program's own id for it.
        id: u32,
        /// Which button.
        button: guitk::event::MouseButton,
    },
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
    /// Make a window translucent, as a window rule asks.
    SetOpacity {
        /// The window to fade.
        window: WindowId,
        /// 0 is invisible, 255 fully opaque.
        ///
        /// A byte rather than the `f32` the rule stores and the wire carries,
        /// for two reasons. It keeps `Eq` on this enum -- `f32` is not `Eq`,
        /// and dropping it here would cascade through `ShellAction` and
        /// `HotkeyOutcome`, neither of which has any business losing it over
        /// one field. And it is what survives anyway: the compositor blends
        /// with an eight-bit alpha, so a finer opacity is discarded a layer
        /// below this one.
        alpha: u8,
    },
    /// Put a window at a place, as a window rule asks.
    MoveWindow {
        /// The window to move.
        window: WindowId,
        /// Top-left corner, in display coordinates.
        x: i32,
        /// Top-left corner, in display coordinates.
        y: i32,
    },
    /// Say what the user may not do to a window, as a window rule asks.
    SetWindowPolicy {
        /// The window to restrain.
        window: WindowId,
        /// What the user may not do.
        policy: WindowPolicy,
    },
    /// Constrain a window's size, as a window rule asks.
    SetSizeLimits {
        /// The window to constrain.
        window: WindowId,
        /// Smallest client area; `(0, 0)` for no minimum.
        min: (u32, u32),
        /// Largest client area; `(0, 0)` for no maximum.
        max: (u32, u32),
    },
    /// Keep a window above or below its neighbours, as a window rule asks.
    SetStackTier {
        /// The window to re-file.
        window: WindowId,
        /// Where it goes within its layer.
        tier: StackTier,
    },
    /// Give a window a size, as a window rule asks.
    ResizeWindow {
        /// The window to resize.
        window: WindowId,
        /// Client-area width.
        width: u32,
        /// Client-area height.
        height: u32,
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
    /// Programs to start, by path, in the order the keystroke named them.
    ///
    /// The keyboard counterpart of [`ShellAction::Launch`], and it exists for
    /// the same reason that variant does: the shell has no connection to the
    /// process server and reports the intent instead of acting on it. Until the
    /// Run box was wired up there was no keystroke that could start a program —
    /// every shortcut in the table acts on a window that already exists — so
    /// this half of the parallel was simply missing, and Enter in the command
    /// box had nowhere to go.
    ///
    /// A `Vec` rather than an `Option` to match `requests` above: both are
    /// unordered, independent asks, and a caller that can already loop over one
    /// should not need a second shape for the other.
    pub launches: Vec<hotkeys::Launch>,
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
            ..Self::default()
        }
    }

    /// The shell claimed the key and wants one thing.
    fn ask(request: Option<ShellRequest>) -> Self {
        Self {
            consumed: true,
            requests: request.into_iter().collect(),
            ..Self::default()
        }
    }

    /// The shell claimed the key and wants several things.
    fn ask_all(requests: Vec<ShellRequest>) -> Self {
        Self {
            consumed: true,
            requests,
            ..Self::default()
        }
    }

    /// The shell claimed the key and wants these programs started.
    fn start(launches: Vec<hotkeys::Launch>) -> Self {
        Self {
            consumed: true,
            launches,
            ..Self::default()
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
    /// The last processor sample, kept so the next one can be differenced.
    ///
    /// **A single sample cannot answer the question a gauge asks.** Everything
    /// in `/proc/stat` counts since boot, so dividing one sample by its own
    /// total says how the machine has spent its *life* -- after a few hours of
    /// uptime that barely moves whatever the machine is doing, while looking
    /// exactly like a live reading. `procinfo`'s `CpuTimes::since` exists for
    /// this and says so in its own docs.
    prev_cpu: Option<procinfo::CpuTimes>,
    /// Processor and memory as of the last widget-due tick.
    sampled: crate::widgets::SystemSample,
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
    /// Whether the card listing every keyboard shortcut is showing.
    ///
    /// A plain `bool` rather than a widget with state of its own, because the
    /// card has none: [`hotkeys::render_settings_panel`] draws it from
    /// [`hotkeys`](Self::hotkeys) every frame, so what it shows is whatever the
    /// registry says right now — a shortcut rebound while the card is up is
    /// redrawn under its new chord without anyone telling the card.
    pub shortcut_card_open: bool,
    /// The shortcut card's editor: which row the keyboard is on, what it is in
    /// the middle of -- recording keys, choosing an action, typing a command,
    /// confirming a move -- and what the last change did.
    ///
    /// Kept while the card is shut, so reopening it returns to the row the
    /// user was looking at; but whatever it was *in the middle of* is dropped
    /// on reopening (`toggle_shortcut_card`), and keys reach it only while the
    /// card is open. A card closed some other way mid-recording -- the start
    /// menu opening, the notification pane -- must not leave a recording
    /// behind that swallows the next keystroke on a desktop showing no card.
    pub(crate) shortcut_editor: shortcut_editor::ShortcutEditor,
    /// The programs this desktop can start, shared with the search launcher so
    /// that the two front ends cannot offer different applications.
    pub apps: Vec<AppEntry>,
    /// Whether Alt+Tab switcher is active.
    /// The installed keyboard layouts and which one is active.
    ///
    /// The shell owns *which* layout is chosen; the compositor owns applying it
    /// to keystrokes. The two meet at `input.yaml`: switching writes the
    /// setting, and the compositor's existing settings watcher picks it up --
    /// the same route the Settings app already uses, so a layout chosen with
    /// the keyboard and one chosen in a panel cannot disagree, and the choice
    /// survives a restart the way a user expects.
    pub input_methods: input_method::InputMethodManager,
    /// The icons other programs have put in the tray.
    ///
    /// Held rather than derived: they come from the compositor over `TRAY`
    /// frames, which is the only place they exist. The shell does not own them
    /// and cannot invent one -- `design-decisions.md` 842 put the tray here,
    /// and `guiremote::tray` put the registry in the compositor so that a shell
    /// restarting does not lose every program's icon.
    ///
    /// Order is the compositor's (registration order) and is kept as given: a
    /// tray whose icons move when an unrelated program registers one is a tray
    /// where the user's muscle memory is wrong.
    tray_icons: Vec<guiremote::tray::TrayIcon>,
    /// The tray icon the pointer is resting on, and the tooltip naming it.
    ///
    /// A tray icon is a single glyph chosen by another program, and the
    /// `tooltip` it registers alongside is the only words anywhere saying what
    /// that glyph is. Until this existed the shell received that string, held
    /// it, and never put it on screen -- so a user faced a row of symbols with
    /// no way to learn what any of them were.
    ///
    /// Keyed, so that sliding along the row replaces the tooltip rather than
    /// leaving the first icon's name under the fourth icon's glyph.
    tray_tooltip: Option<(tray_dnd::TrayIconKey, guitk::menu::Tooltip)>,
    /// The popup listing icons the bar had no room for, and which icons
    /// those were when it opened.
    ///
    /// The keys are captured with the menu rather than recomputed on
    /// selection: between opening the list and picking from it a program can
    /// exit, and re-deriving the list would hand row 3 to whoever moved up
    /// into position 3.
    tray_overflow_menu: Option<(guitk::menu::ContextMenu, Vec<tray_dnd::TrayIconKey>)>,
    /// The pin menu a right-click on a start-menu row opens, and which row it
    /// was opened on.
    ///
    /// The row is remembered rather than the executable path, for the reason
    /// [`Hit::TaskbarPinned`] carries an index: the list is the authority, and
    /// a copy of a path here would be a second one to keep in step.
    pin_menu: Option<(guitk::menu::ContextMenu, PinTarget)>,
    /// A pinned button being dragged along the bar.
    ///
    /// Keyed on the executable path rather than the slot, for the reason the
    /// tray's drag is keyed on an icon's name: the row rearranges itself under
    /// the pointer as the drag proceeds, so an index taken at press time
    /// stops meaning the thing that was pressed.
    pin_drag: Option<tray_dnd::DragSource<String>>,
    /// Whether the pinned button being dragged has been carried away from
    /// the row of pins -- up off the bar towards the desktop, where letting
    /// go puts a shortcut to it, or onto the start button, where letting go
    /// pins it to the start menu. While it is, the row stops rearranging
    /// under it.
    pin_drag_off_bar: bool,
    /// A start-menu row pressed and not yet let go: a click that starts the
    /// program, or the start of a drag that carries it to the taskbar, the
    /// desktop or the menu's own pinned rows. See
    /// [`finish_start_press`](Self::finish_start_press).
    start_drag: Option<StartDrag>,
    /// What has been typed into the start menu's search field. Empty is the
    /// ordinary menu; anything else lists only the programs it finds, best
    /// first. Emptied each time the menu opens, as a search box is.
    start_query: TextInput,
    /// The start-menu row the keyboard is on, as an index into
    /// [`start_menu_entries`](Self::start_menu_entries): `None` until an arrow
    /// key is pressed, and again whenever the search changes. Enter starts it;
    /// with none, Enter starts the best match.
    start_selected: Option<usize>,
    /// Programs the user pinned to the top of the start menu, in their
    /// order: dropped there, or chosen with "Pin to Start menu". Listed
    /// above the launcher's programs, and a pinned program is still listed
    /// among them too, as on every start menu that has pins.
    start_pins: Vec<AppEntry>,
    /// Whether [`start_pins`](Self::start_pins) changed since it was last
    /// written. The session writes it -- see
    /// [`take_start_pins_dirty`](Self::take_start_pins_dirty) -- so that a
    /// write that fails can be reported rather than printed and forgotten.
    start_pins_dirty: bool,
    /// Where the pointer is in a drag that carries a program, for the label
    /// that follows it.
    carry_at: (f32, f32),
    /// The order the running programs' buttons stand in on the taskbar: the
    /// order their windows arrived, as the user has since rearranged them by
    /// dragging. Every window the shell holds is in it, shown or not, so a
    /// window moved to another desktop and back returns to its place.
    ///
    /// The shell's own, because nothing else knows it. The compositor lists
    /// windows in *stacking* order, which changes every time one is raised;
    /// a bar drawn in that order moved a button to the end each time it was
    /// clicked, so no button was ever where the user had last seen it.
    button_order: Vec<WindowId>,
    /// A window's taskbar button pressed and not yet let go: a click, or the
    /// start of dragging it along the row. See
    /// [`finish_window_press`](Self::finish_window_press).
    window_press: Option<WindowPress>,
    /// A press that landed on a tray icon and has not been released.
    ///
    /// Held from press to release because until the release the shell does
    /// not know which gesture this was: a click, which the owning program is
    /// told about, or a drag, which is the shell's own and which the program
    /// never hears about at all.
    tray_drag: Option<TrayDrag>,
    /// The order the shell shows those icons in, which is the user's.
    ///
    /// Separate from the list above because the two answer different
    /// questions and change at different times: the compositor says which
    /// icons exist, and re-sends the whole list whenever any program touches
    /// its own; the arrangement says where they sit, and changes only when
    /// the user moves one.
    tray_arrangement: tray_dnd::TrayIconArrangement,
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
    /// Set when the shell itself has written `appearance.yaml` and the
    /// compositor has not been told.
    appearance_dirty: bool,
    /// The menu that opens on a right-click over the desktop.
    ///
    /// Rebuilt at each opening by
    /// [`open_desktop_menu`](Self::open_desktop_menu): what it lists depends
    /// on what was clicked -- a widget or bare desktop -- and its View submenu
    /// ticks the icon size and arrangement in force, so a menu built once
    /// would show whatever was true when it was built.
    pub desktop_menu: ContextMenu,
    /// The icons on the desktop, and where the user left them.
    ///
    /// `design-decisions.md` 933 (open-questions A-Q8) makes this layer the
    /// layout authority: icon positions are not a kernel concern. Lane A
    /// deleted `fs::deskicons` and `/proc/deskicons` once this read and wrote
    /// them, which it has -- checked 2026-09-16, neither exists. It is
    /// populated and its saved layout applied by
    /// [`populate_icons`](Self::populate_icons), which the session calls
    /// before its first frame, so the icons are drawn where they were left
    /// rather than where the defaults put them and then jumping. Not in
    /// [`new`](Self::new), which must not read the user's files.
    pub icons: icons::DesktopIconLayer,
    /// Whether the icon layout has changed since it was last written: a drop
    /// that moved something, or a choice from the View submenu.
    ///
    /// A flag the session drains, like [`widgets_dirty`](Self::widgets_dirty),
    /// rather than a write here, for the two reasons that one is: a pump that
    /// changed the layout twice writes it once, and a failed write reaches the
    /// session, which can say so -- the shell has nowhere to. The release used
    /// to write the file itself and drop the error.
    icons_dirty: bool,
    /// Widget panels drawn on the desktop background.
    ///
    /// Empty until the user adds one from [`desktop_menu`](Self::desktop_menu),
    /// which is what keeps an untouched desktop identical to how it was before
    /// widgets existed -- and keeps it idle, since a desktop with no widgets
    /// has nothing that needs ticking.
    pub widgets: DesktopWidgetManager,
    /// The widget the open menu is about, if it was opened over one.
    ///
    /// Held rather than re-hit-tested when the item is chosen: by then the
    /// pointer is over the menu, which is drawn *on top of* the widget, so a
    /// second hit test would answer about wherever the menu happens to sit.
    menu_widget: Option<WidgetInstanceId>,
    /// The icon the open menu is about, if it was opened over one -- held for
    /// the reason `menu_widget` is: by the time an item is chosen the pointer
    /// is over the menu, not the icon.
    menu_icon: Option<icons::IconId>,
    /// The widget being dragged, and where inside it the pointer took hold.
    ///
    /// The offset is what stops a drag snapping the widget's corner to the
    /// pointer on the first pixel of movement.
    widget_drag: Option<(WidgetInstanceId, f32, f32)>,
    /// Whether the widget layout has changed since it was last written.
    ///
    /// Set only where a change is *committed* -- a menu action, a drag that
    /// ended -- and deliberately not on every step of a drag. A flag set per
    /// pointer move would be a file write per frame while the user is still
    /// deciding where to put the thing.
    widgets_dirty: bool,
    /// Watches `appearance.yaml` so a change made in another process reaches
    /// this one without a restart.
    ///
    /// Private, and the shell's *only* reader of that file: the startup load
    /// and the reloads go through the same watcher, so there is no window
    /// between "read once at startup" and "start watching" for a save to fall
    /// into. See [`poll_appearance`](Self::poll_appearance).
    appearance_watch: config::Watcher,
    /// `notifications.yaml`, which says which programs may interrupt.
    ///
    /// A watcher rather than a one-off read, for the reason the appearance
    /// one is: the Settings application writes this file from another
    /// process, so "has it changed" is a question the shell has to be able to
    /// ask again, not something it learns once at login.
    notif_watch: config::Watcher,
    /// The applications pinned to the taskbar.
    ///
    /// `taskbar::Taskbar` is used for its *model* only -- `add_pinned`,
    /// `remove_pinned`, `pinned_apps` -- and never for its renderer, which is
    /// design-decisions 849. That module had no caller at all until this; see
    /// `TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`.
    taskbar: taskbar::TaskbarState,
    /// When an automatic schedule may take effect again, if the user has just
    /// switched it off from inside one.
    ///
    /// A wall-clock instant rather than a countdown: the desktop parks for
    /// hours at a time, and a countdown would have to be decremented by
    /// something, which is the per-minute wake-up this whole path exists to
    /// avoid. See `snooze_schedule_if_it_would_resume`.
    schedule_snooze: Option<u64>,
    /// The rules with the document they came from.
    ///
    /// Needed because a save splices into the document that was read, so that
    /// a user's comments and any key a newer desktop wrote survive being
    /// edited from here. `focus_assist::app_overrides` is the copy the
    /// decision is made against; this is the copy that can be written back.
    notif: notifsettings::NotifFile,
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
    /// one. See `current_clock_string`.
    pub datetime: datetime_settings::DateTimeSettings,
    /// The zone this machine is in -- what the clock shows when
    /// [`datetime`](Self::datetime) names none.
    ///
    /// UTC until [`load_datetime`](Self::load_datetime) reads the real one,
    /// and deliberately not read in [`new`](Self::new): `new` is what the unit
    /// tests build, and a shell that read the host's `TZ` and `/etc/localtime`
    /// there would give every clock test a different answer on every machine.
    system_zone: Tz,
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
    /// The notification pane the tray bell opens: the desktop's only place for
    /// a message the user did not ask for.
    ///
    /// `notif_pane.rs` was the same kind of island `calendar.rs` was — a full
    /// pane with quick settings, per-app rules, a history and 130 tests, and
    /// nothing anywhere that constructed a `NotificationPane`. The shell could
    /// therefore *notice* things and had nowhere to say them: the wallpaper
    /// that failed to decode set `ShellSession::wallpaper_error` and the user
    /// saw a plain colour with no explanation.
    ///
    /// As with the calendar, the pane's own
    /// [`PaneState::is_visible`](notif_pane::PaneState::is_visible) **is** the
    /// open flag. See `design-decisions.md` §493.
    pub notifications: notif_pane::NotificationPane,
    /// Whether — and how much — notifications are being silenced right now.
    ///
    /// The single truth for Do Not Disturb. The pane's two quick-setting
    /// switches are *views* of this: pressing one is reported to the shell,
    /// applied here, and pushed back onto both switches by
    /// `sync_quick_settings`, so the pair can
    /// never disagree with what is actually happening to arriving
    /// notifications.
    ///
    /// It was the second-largest orphan island in the shell (1,466 lines that
    /// nothing constructed) for the same reason the pane was: a complete
    /// module with a settings page, per-app priorities and automatic rules,
    /// and no owner. See known-issues.md
    /// `TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`.
    ///
    /// One half is still unwired: [`evaluate_auto_rules`] needs a wall clock
    /// and a "the foreground window went fullscreen" signal, neither of which
    /// reaches the shell yet, so [`effective_mode`] is today exactly the manual
    /// mode. That is recorded in known-issues.md rather than papered over with
    /// a clock read from the wrong place.
    ///
    /// [`evaluate_auto_rules`]: focus_assist::FocusAssistManager::evaluate_auto_rules
    /// [`effective_mode`]: focus_assist::FocusAssistManager::effective_mode
    pub focus: focus_assist::FocusAssistManager,
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
    /// exists. `sync_snap_area` re-seeds it at the top
    /// of every gesture that reads it instead.
    pub snap: snap::SnapManager,
    /// The heads-up overlays: what the volume keys put on screen.
    ///
    /// The only part of the shell that draws *without* the user having asked to
    /// look at anything — every other surface here is opened by a click or a
    /// chord and stays until it is closed. An OSD appears because something
    /// changed, says what it was, and goes away on a timer.
    ///
    /// Consequences, both of which the session honours: it is the one surface
    /// that must not take clicks (a volume indicator that ate the press aimed
    /// at the document beneath it would be worse than no indicator — see
    /// `design-decisions.md` 566), and it is the one that keeps animating with
    /// no input at all, so it has to be counted in
    /// [`ShellSession::anything_moving`](session::ShellSession) or its fade
    /// stops halfway and the overlay stays on screen for ever.
    ///
    /// Timed off `osd_clock_ms` rather than off the wall
    /// clock; see that field for why the difference does not matter here.
    pub osd: osd::OsdManager,
    /// Milliseconds of animation time the shell has been told about, and the
    /// only clock [`show_osd`](Self::show_osd) consults.
    ///
    /// [`OsdManager`](osd::OsdManager) works in absolute stamps, because a
    /// timeout is a deadline and a deadline cannot be computed from a delta.
    /// But the *origin* of those stamps is arbitrary: every one the manager
    /// compares is one this counter issued, so an epoch of "when this shell was
    /// constructed" answers every question the manager asks, and the shell does
    /// not have to be handed a wall clock — which is what keeps it testable
    /// without also controlling the machine's time.
    ///
    /// It advances only when [`advance_osd`](Self::advance_osd) is called, so
    /// it stands still while the desktop is idle. That is exactly right rather
    /// than merely tolerable: nothing is scheduled while the desktop is idle,
    /// so no deadline can fall due during the gap, and `oswindow` resets a
    /// window's delta origin when it is ticked without re-arming — so the first
    /// frame after a park reports the length of *that frame*, not the length of
    /// the pause, and the pause never lands on an overlay's deadline at all.
    osd_clock_ms: u64,
    /// The Run box: type a command, press Enter, the program starts.
    ///
    /// The only shell surface with a text field that is *also* a dialog — the
    /// overview and the start menu have search boxes, but neither has an OK
    /// button or an error line. Two consequences follow, and both are why it
    /// could not simply be added to the popup list and left there:
    ///
    /// It is modal about keys, because every printable key belongs to it while
    /// it is up. `handle_hotkey_inner` gives it every press before the binding
    /// table sees one, exactly as it does for the overview: without that, typing
    /// `d` into the command box would show the desktop out from under the box
    /// being typed into.
    ///
    /// And Enter is a *launch*, which is the one thing the shell cannot do
    /// itself (see [`ShellAction::Launch`]). The keyboard path had no way to
    /// report one until this surface existed, which is what
    /// [`HotkeyOutcome::launches`] is for.
    pub run_dialog: run_dialog::RunDialog,
    /// The file chooser the Run box's Browse button puts up, while it is up.
    ///
    /// Modal over the Run box in exactly the way the Run box is modal over the
    /// desktop, and by the same two mechanisms: it is offered every press
    /// before the box is ([`handle_hotkey_inner`](Self::handle_hotkey_inner))
    /// and every pointer event before the box is
    /// ([`handle_mouse_inner`](Self::handle_mouse_inner)), and it is drawn last
    /// of all. Its answer goes back into the box's command field rather than
    /// starting anything, which is what distinguishes browsing from launching —
    /// the user asked *which* program, not for a file manager.
    ///
    /// `Option` rather than a `visible` flag on a permanent one, unlike the Run
    /// box itself: a chooser holds a directory listing, and a shell that kept
    /// one for the lifetime of the session would keep a listing of a directory
    /// nobody is looking at, going staler by the hour.
    run_browser: Option<guitk::dialog::FileDialog>,
    /// The directory the chooser has actually been given a listing for.
    ///
    /// The shell reads no files — that is what keeps every test in this module
    /// runnable with no filesystem, and it is the same split
    /// [`wallpaper::WallpaperManager`] uses, where the shell names a picture
    /// and the session reads it. So the chooser's listing arrives from outside:
    /// [`run_browser_wants`](Self::run_browser_wants) reports a directory whose
    /// listing has not been delivered yet, and
    /// [`set_run_browser_entries`](Self::set_run_browser_entries) delivers it.
    ///
    /// Held as "what was last delivered" and compared against the chooser's
    /// current directory, rather than as a "needs listing" flag that navigation
    /// would have to remember to set. A flag can be forgotten by a new
    /// navigation path; a comparison cannot go stale.
    run_browser_listed: Option<PathBuf>,
    /// The window rules, and the state of the ones that have fired.
    ///
    /// Consulted from exactly one place —
    /// [`apply_window_list`](Self::apply_window_list), for each window the
    /// shell has not seen before. That "not seen before" is the whole contract:
    /// [`evaluate`](window_rules::WindowRulesManager::evaluate) takes `&mut
    /// self` because it counts firings and deletes one-shot rules, so a shell
    /// that ran it per *list* rather than per *arrival* would delete every
    /// one-shot rule on the first frame after it was written, and would
    /// re-maximise a window one frame after the user restored it.
    ///
    /// It was an orphan island of 2,600 lines before this: a rules engine, a
    /// settings panel and 120 tests, and nothing that ever called `evaluate`.
    /// It could not have been wired sooner — its two program criteria were a
    /// process name and a window class, neither of which existed anywhere in
    /// this system. See `design-decisions.md` §569.
    pub rules: window_rules::WindowRulesManager,

    /// Which chord does what.
    ///
    /// The desktop's whole binding table, and the only one: the shortcuts used
    /// to live in a private `DesktopAction::for_chord` match hardcoded a few
    /// hundred lines below, while [`hotkeys`] sat beside it with a registry, a
    /// config file and 2,400 lines and was reachable from nothing. Two tables
    /// meant two answers to "what does Super+E do", and the *live* one was the
    /// one a user could not change. See `design-decisions.md` §571.
    ///
    /// Public so a settings panel can rebind without going through the shell,
    /// on the same terms as [`rules`](Self::rules) above. Anything that changes
    /// it has to re-grab: see [`global_chords`](Self::global_chords).
    pub hotkeys: hotkeys::HotkeyRegistry,
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
    /// `taskbar_alpha`.
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
    /// `from_palette`.
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

/// The icon layer's button enum from the toolkit's.
///
/// Two enums for one concept, which is this tree's most-repeated defect -- but
/// `icons.rs` predates the shell's dependency on `guitk::event` and converting
/// at the one seam is smaller than changing a module's public vocabulary. The
/// conversion lives here, once, rather than at each of the three call sites.
const fn icon_button(button: MouseButton) -> icons::MouseButton {
    match button {
        MouseButton::Left => icons::MouseButton::Left,
        MouseButton::Right => icons::MouseButton::Right,
        _ => icons::MouseButton::Middle,
    }
}

/// Whether a file may be run as a program: any of its execute bits set.
#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    (meta.permissions().mode() & 0o111) != 0
}

/// The development host's answer. SlateOS is `target-family = "unix"`, so
/// this is only ever asked on a machine none of whose programs are
/// SlateOS's, and "no" sends such a file to the "nothing opens it" notice
/// rather than to a program it was never built to be.
#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    false
}

impl DesktopShell {
    pub fn new(screen_width: u32, screen_height: u32) -> Self {
        let mut shell = Self {
            prev_cpu: None,
            sampled: crate::widgets::SystemSample::default(),
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
            shortcut_card_open: false,
            shortcut_editor: shortcut_editor::ShortcutEditor::new(),
            apps: launcher::builtin_app_database(),
            // The layouts this machine has, from `keylayout`'s built-in set.
            // Which one is *active* is corrected from `input.yaml` by
            // `load_input_settings`; this is only the list.
            input_methods: input_method::InputMethodManager::with_builtins(),
            tray_icons: Vec::new(),
            tray_arrangement: tray_dnd::TrayIconArrangement::new(),
            tray_drag: None,
            tray_overflow_menu: None,
            pin_menu: None,
            pin_drag: None,
            pin_drag_off_bar: false,
            start_drag: None,
            start_pins: Vec::new(),
            start_pins_dirty: false,
            start_query: TextInput::new(),
            start_selected: None,
            carry_at: (0.0, 0.0),
            button_order: Vec::new(),
            window_press: None,
            tray_tooltip: None,
            alt_tab_active: false,
            alt_tab_index: 0,
            overview: overview::OverviewState::new(),
            overview_config: overview::OverviewConfig::default(),
            appearance: AppearanceSettings::default(),
            appearance_dirty: false,
            // Closed, and rebuilt from the state in force whenever it opens;
            // what it is built with here is never shown.
            desktop_menu: ContextMenu::new(Self::desktop_menu_items(
                AppearanceSettings::default().icon_size,
                icons::ArrangementMode::default(),
            )),
            // 40 is the `taskbar_height` two lines below; both are the
            // literal because this is the initialiser that establishes it.
            icons: icons::DesktopIconLayer::new(screen_width, screen_height, 40),
            icons_dirty: false,
            widgets: DesktopWidgetManager::new(),
            menu_widget: None,
            menu_icon: None,
            widget_drag: None,
            widgets_dirty: false,
            appearance_watch: config::Watcher::new(appearance::CONFIG_NAME),
            notif_watch: config::Watcher::new(notifsettings::CONFIG_NAME),
            taskbar: taskbar::TaskbarState::new(taskbar::TaskbarConfig::default()),
            schedule_snooze: None,
            notif: notifsettings::NotifFile::new(),
            theme: DesktopTheme::default(),
            datetime: datetime_settings::DateTimeSettings::default(),
            system_zone: Tz::utc(),
            calendar: calendar::CalendarView::new(calendar::CalendarConfig::default()),
            notifications: notif_pane::NotificationPane::new(),
            focus: focus_assist::FocusAssistManager::new(),
            events: calendar::EventStore::new(),
            // Placeholder: the real area needs `taskbar_rect()`, which needs
            // the appearance scaling that is only set two fields up. Seeded
            // immediately below rather than left to the first gesture, so that
            // a caller reading `shell.snap.layout()` before ever opening the
            // overlay gets the screen it is actually on.
            snap: snap::SnapManager::new(snap::WorkArea::whole_screen(0.0, 0.0)),
            // Screen-sized from the start, unlike `snap` above: the OSD centres
            // itself on the whole display rather than on the work area, because
            // it is a heads-up overlay and may sit over the taskbar.
            osd: osd::OsdManager::new(screen_width as f32, screen_height as f32),
            osd_clock_ms: 0,
            // Not positioned here. `set_position` needs a screen size, and this
            // one would go stale the moment the display changed; the dialog is
            // centred on the screen it is opened on instead, in
            // `toggle_run_dialog`.
            run_dialog: run_dialog::RunDialog::new(),
            run_browser: None,
            run_browser_listed: None,
            rules: window_rules::WindowRulesManager::new(),
            hotkeys: hotkeys::HotkeyRegistry::defaults(),
        };
        shell.sync_snap_area();
        // The icon size the settings start with, not the layer's own starting
        // size: a shell that has not loaded the user's appearance yet should
        // still draw what the default settings say.
        shell
            .icons
            .set_icon_size(shell.appearance.icon_size.pixels());
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
        // The caret width goes to the surface that draws one. Pushed here
        // rather than read at draw time because `render` is handed a
        // `Palette`, and a palette is colours: 839 put the caret's width in
        // the appearance settings, not in the theme's colour table. Pushing it
        // from the one place that knows the settings changed is the same shape
        // as `sync_animation_speed`, and for the same reason -- a second door
        // the caller has to remember is a door somebody forgets.
        self.run_dialog.set_caret_width(appearance.caret_width());
        self.icons.set_caret_width(appearance.caret_width());
        // The icon size goes to the layer that draws icons, for the same
        // reason: it was a setting with a working control and no reader --
        // `known-issues.md` TD-C-FOUR-APPEARANCE-SETTINGS-HAVE-A-WORKING-CONTROL-
        // AND-NO-READER -- chosen, saved, restored at login, and drawn by
        // nothing.
        self.icons.set_icon_size(appearance.icon_size.pixels());
        guitk::scaling::set_global_scale(appearance.scale_factor());
        self.appearance = appearance;
        // After the store: the taskbar's thickness follows the scale just
        // set, and the icons must stay clear of the bar as it is drawn.
        self.sync_icon_area();
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
    /// A missing or unreadable file is not an error: the watcher yields an
    /// empty document, which reads back as the defaults.
    ///
    /// Goes through the same watcher as
    /// [`poll_appearance`](Self::poll_appearance) rather than reading the file
    /// itself, so the shell has one reader of `appearance.yaml` instead of
    /// two. Reading it here and *then* constructing a watcher would leave a
    /// gap: a save landing between the two would be recorded as already-seen
    /// and never reported, and the desktop would sit on stale settings until
    /// the next unrelated change.
    pub fn load_appearance(&mut self) {
        let settings = match self.appearance_watch.poll() {
            Some(doc) => AppearanceSettings::read_from(&doc),
            // Already seen, which on this path means the caller loaded twice.
            // Re-apply what is held rather than doing nothing, so that the
            // scale is published either way.
            None => self.appearance.clone(),
        };
        self.set_appearance(settings);
    }

    /// Read the user's saved notification rules and adopt them.
    ///
    /// The rules go to [`focus_assist`](focus_assist::FocusAssistManager),
    /// which is what already reads them: `should_show_notification` asks an
    /// app's importance on every notification the shell is handed. Until this
    /// existed that list was only ever empty, so every program got the
    /// default and a rule was something no user could set.
    pub fn load_notification_rules(&mut self) {
        if let Some(doc) = self.notif_watch.poll() {
            self.notif = notifsettings::NotifFile::from_document(doc);
            self.focus.app_overrides = self.notif.settings.apps.clone();
            self.focus.set_quiet_hours(&self.notif.settings.quiet_hours);
            self.notifications
                .adopt_app_rules(&self.notif.settings.apps);
        }
    }

    /// Re-read the rules if the file changed, and say whether they differ.
    ///
    /// Answers on the *settings*, not on the file, for the reason
    /// [`poll_appearance`](Self::poll_appearance) does: a comment added to the
    /// file, or a key written by a newer desktop, changes the document and
    /// changes nothing a caller should act on.
    ///
    /// Like `poll_appearance` it does not merge. The file is the authority,
    /// and the shell has no notification-rules editor of its own whose unsaved
    /// work could be lost — the Settings application owns that page.
    pub fn poll_notification_rules(&mut self) -> bool {
        let Some(doc) = self.notif_watch.poll() else {
            return false;
        };
        let file = notifsettings::NotifFile::from_document(doc);
        let apps_changed = file.settings.apps != self.focus.app_overrides;
        // Compared against the settings the shell holds rather than against
        // the rule built out of them: `set_quiet_hours` drops a schedule that
        // runs on no day, so a rule list that is equal either way would report
        // "unchanged" for a real edit the user made.
        let quiet_changed = file.settings.quiet_hours != self.notif.settings.quiet_hours;
        if apps_changed {
            self.focus.app_overrides = file.settings.apps.clone();
            // The pane draws a card per program; without this a rule changed
            // in the Settings application would show as its old value here,
            // and a toggle made against that stale card would write it back.
            self.notifications.adopt_app_rules(&file.settings.apps);
        }
        if quiet_changed {
            self.focus.set_quiet_hours(&file.settings.quiet_hours);
        }
        let changed = apps_changed || quiet_changed;
        // Adopted either way: the document is what a later save splices into,
        // so keeping the old one would write back a file stripped of whatever
        // comment or unknown key this read just brought in.
        self.notif = file;
        changed
    }

    /// The local clock reading the schedules are compared against.
    ///
    /// Takes the instant rather than reading the clock itself, so that a test
    /// can ask what the desktop does at half past two on a Saturday morning
    /// without waiting until then.
    #[must_use]
    pub fn clock_reading_at(&self, utc_secs: u64) -> (u8, u8, u8) {
        calendar::local_clock_reading(utc_secs, &self.local_zone())
    }

    /// Put the user's quiet hours into force for this instant.
    ///
    /// Answers whether the effective focus mode changed, which is what a
    /// caller repaints on: the taskbar shows whether focus assist is on.
    pub fn evaluate_schedules(&mut self, utc_secs: u64) -> bool {
        let before = self.focus.effective_mode();
        // A snooze ends at the schedule's own next boundary, which is the
        // instant the loop is already awake for -- see `next_schedule_change`.
        if self.schedule_snooze.is_some_and(|until| utc_secs >= until) {
            self.schedule_snooze = None;
        }
        self.focus.auto_suppressed = self.schedule_snooze.is_some();
        // With no rules at all the answer cannot depend on the time, so the
        // zone lookup and the civil-date arithmetic below are skipped -- this
        // runs on every tick and quiet hours ship switched off, so the
        // no-rules case is the one that has to be free. The call itself still
        // happens: a rule removed a moment ago may have left focus assist on,
        // and returning early without asking would strand it there.
        let (hour, minute, weekday) = if self.focus.auto_rules.is_empty() {
            (0, 0, 0)
        } else {
            self.clock_reading_at(utc_secs)
        };
        self.focus.evaluate_auto_rules(hour, minute, weekday);
        self.focus.effective_mode() != before
    }

    /// How long until quiet hours next start or stop, if they ever do.
    ///
    /// **The shell sleeps exactly this long rather than watching the clock.**
    /// `design-decisions.md` 812 has an idle desktop registering no wake-up at
    /// all, which rules out checking the time every minute and finding that
    /// nothing has changed 1 439 times a day. `None` -- the common answer,
    /// since quiet hours ship switched off -- means arm no timer.
    ///
    /// The seconds already spent in the current minute are taken off, because
    /// [`minutes_until_change`](notifsettings::QuietHours::minutes_until_change)
    /// counts whole minutes from the *start* of this one. Without that the
    /// timer lands up to 59 seconds late and the desktop is briefly noisy
    /// inside the hours the user asked to be left alone -- a lateness nobody
    /// would ever catch, because the only symptom is one notification that
    /// should have been held.
    #[must_use]
    pub fn next_schedule_change(&self, utc_secs: u64) -> Option<Duration> {
        let (hour, minute, weekday) = self.clock_reading_at(utc_secs);
        let quiet = &self.notif.settings.quiet_hours;
        let minutes = quiet.minutes_until_change(hour, minute, weekday)?;
        let into_minute = utc_secs % 60;
        // Never zero: a timer of no length would fire before the boundary it
        // is waiting for and re-arm for zero again, which is a poll as fast as
        // the loop can run.
        Some(Duration::from_secs(
            u64::from(minutes)
                .saturating_mul(60)
                .saturating_sub(into_minute)
                .max(1),
        ))
    }

    /// Apply one per-app change the user made in the notification pane.
    ///
    /// Writes `notifications.yaml`, which is safe here and was not safe on the
    /// path that *receives* a notification -- this runs because a person
    /// clicked a switch. See
    /// `TD-C-THE-NOTIFICATIONS-PAGE-HAS-NO-PROGRAMS-TO-LIST` for the version
    /// that ran on the wrong path and what it cost.
    ///
    /// A failed write is reported and the change still applies to this
    /// session: refusing a toggle because a disk is full would leave someone
    /// unable to silence a program that is interrupting them, which is the one
    /// moment they can least tolerate it.
    fn apply_app_notification_setting(
        &mut self,
        app: &str,
        setting: notif_pane::AppSettingKind,
        value: notif_pane::SettingValue,
    ) {
        let mut rule = self
            .notif
            .settings
            .apps
            .iter()
            .find(|r| r.app_name == app)
            .cloned()
            .unwrap_or_else(|| notifsettings::AppRule::new(app));
        match (setting, value) {
            // `Enabled` is the pane's word and `Silent` is the file's. One
            // switch, two vocabularies, and the translation belongs here:
            // `notifsettings` has no opinion about a pane and the pane has
            // none about a focus mode.
            //
            // Switching back on restores `Normal` rather than whatever was
            // there before, because the pane's switch carries no memory of it
            // and inventing one here would be a third place storing a value.
            (notif_pane::AppSettingKind::Enabled, notif_pane::SettingValue::Bool(on)) => {
                rule.importance = if on {
                    notifsettings::Importance::Normal
                } else {
                    notifsettings::Importance::Silent
                };
            }
            (notif_pane::AppSettingKind::Sound, notif_pane::SettingValue::Bool(on)) => {
                rule.sound = on;
            }
            (notif_pane::AppSettingKind::Banner, notif_pane::SettingValue::Bool(on)) => {
                rule.banner = on;
            }
            // Not applied, and not an oversight. The pane's `Priority`
            // carries `notif_pane::NotifPriority` -- Low/Normal/High/Urgent,
            // the scale a *message* is drawn with -- and a rule's importance
            // is Silent/Normal/Priority/Critical, the scale a *program* is
            // trusted with. There is no honest mapping between them: "this
            // notification is Urgent" and "this program may interrupt a focus
            // mode" are different claims. The pane does not emit this today;
            // if it grows a control that does, the control should speak
            // `Importance` and this arm should go, rather than a conversion
            // being invented here.
            (notif_pane::AppSettingKind::Priority, _) => return,
            // A kind paired with a value of the wrong shape. Ignored rather
            // than guessed at: there is no reading of "the sound switch was
            // set to Critical", and acting on half of it would write a rule
            // nobody asked for.
            _ => return,
        }
        self.notif.settings.set_rule(rule.clone());
        self.focus.set_app_override(rule.clone());
        self.notifications.adopt_app_rules(&[rule]);
        if let Err(err) = self.notif.save() {
            eprintln!("desktop: could not save notifications.yaml: {err}");
        }
    }

    /// Read the user's saved *input* settings and adopt the parts the shell
    /// owns.
    ///
    /// Today that is one field: which shortcut cycles the keyboard layout.
    /// It lives in `input.yaml` beside the layout itself, because the two are
    /// the same subject and because
    /// [`persist_input_layout`](Self::persist_input_layout) already writes
    /// that file — a second configuration format for one neighbouring value
    /// would be a second thing to keep in step.
    ///
    /// Returns whether anything changed, so a caller reacting to an
    /// announcement can skip the work when a different field moved.
    ///
    /// Reads the file rather than a watch, on `persist_input_layout`'s
    /// reasoning: an input-settings announcement happens when a person clicks
    /// Apply, and a file read then costs nothing anybody can perceive.
    pub fn load_input_settings(&mut self) -> bool {
        let file = inputsettings::InputFile::load();
        let wanted =
            crate::input_method::SwitchShortcut::from_id(&file.settings.keyboard.layout_switch);
        if self.input_methods.switch_shortcut == wanted {
            return false;
        }
        self.input_methods.switch_shortcut = wanted;
        true
    }

    /// Look once for an appearance change made by another process, and apply
    /// it. Returns whether anything changed.
    ///
    /// This is what makes a setting take effect while the desktop is running.
    /// The Settings application writes `appearance.yaml`; nothing tells the
    /// shell, so the shell looks. Until then the two agreed only across a
    /// restart -- change your accent colour and the desktop kept the old one.
    ///
    /// # How often to call it
    ///
    /// That is the caller's decision, not this method's: it looks exactly once
    /// per call and holds no clock, for the same reason
    /// [`advance_osd`](Self::advance_osd) takes its own elapsed time. A
    /// once-a-second cadence from whatever already drives the frame loop is
    /// ample -- this is a person moving a slider in another window, not a
    /// stream. Calling it every frame is *safe* but wasteful: an unchanged
    /// file costs one small read, and `Watcher` is careful to answer `None`
    /// so that no parsing or repaint follows.
    ///
    /// # What it deliberately does not do
    ///
    /// It does not merge. The file is the authority, so a change on disk
    /// replaces what the shell holds -- which is right because the shell has
    /// no appearance editor of its own to lose work from. When the settings
    /// *window* is finally hosted here, this becomes a question with a real
    /// answer (whose copy wins while a panel is open with unsaved edits?) and
    /// this method is where it will have to be answered. See `todo.txt`.
    pub fn poll_appearance(&mut self) -> bool {
        let Some(doc) = self.appearance_watch.poll() else {
            return false;
        };
        let settings = AppearanceSettings::read_from(&doc);
        // A *file* change is not a *settings* change, and this is the
        // difference that matters to a caller who repaints on `true`.
        //
        // Two ways they come apart, one of which is on every startup path.
        // The watcher's first look always reports, having seen nothing yet --
        // so a session whose caller never called `load_appearance` would
        // repaint on its first pump for no reason, which is exactly what the
        // shell's own "a press it does not want repaints nothing" and "an idle
        // park is bounded" tests caught when this method compared nothing. And
        // a file can change without any setting changing: somebody adds a
        // comment, or a newer desktop writes a key this one does not read.
        //
        // `AppearanceSettings` derives `PartialEq` over the whole struct
        // precisely so this cannot fall behind a field, the way the
        // hand-written `is_dirty` field list it replaced did.
        if settings == self.appearance {
            return false;
        }
        self.set_appearance(settings);
        true
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

    /// Follow the display to a new size -- the shell's own idea of the
    /// screen, and the icon layer's, which brings every icon back onto the
    /// desktop that is now there.
    ///
    /// The session used to set the two fields directly, and the icon layer
    /// kept the size it was built with for the rest of the session.
    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;
        self.sync_icon_area();
    }

    /// Tell the icon layer how much of the screen is desktop: all of it but
    /// the taskbar, at the thickness the bar is actually drawn -- which the
    /// scale setting changes, so an appearance change calls this too.
    fn sync_icon_area(&mut self) {
        let bar = self.taskbar_thickness().round() as u32;
        self.icons
            .set_desktop_area(self.screen_width, self.screen_height, bar);
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
        let count = self.taskbar_slots().len().max(1);
        // Everything between the start button and the tray that is not a
        // button: the gap after the start button, the gap after every button
        // but the last, the wider one between the sections, and the reserve
        // before the tray. The gaps used to be left out, so the buttons were
        // each given their share of the space and then spaced apart as well
        // -- fine while they were at their widest, and past the tray's edge
        // once enough windows were open: 108 px into it with thirty-one.
        let gaps = self.scale(TASKBAR_BUTTON_START_GAP)
            + count.saturating_sub(1) as f32 * self.scale(TASKBAR_BUTTON_GAP)
            + self.section_gap_extra()
            + self.scale(TRAY_RESERVE_GAP);
        let available =
            (bar.w - self.scale(START_BUTTON_WIDTH) - self.tray_width() - gaps).max(0.0);
        self.scale(TASKBAR_BUTTON_MAX_WIDTH)
            .min(available / count as f32)
    }

    /// How much wider than an ordinary gap the one between the pinned and
    /// the running sections is -- nothing unless both sections have buttons,
    /// since a divider with nothing on one side divides nothing.
    fn section_gap_extra(&self) -> f32 {
        let pins = self.taskbar.pinned_apps().len();
        let running = self.taskbar_slots().len().saturating_sub(pins);
        if pins > 0 && running > 0 {
            self.scale(TASKBAR_SECTION_GAP - TASKBAR_BUTTON_GAP)
        } else {
            0.0
        }
    }

    /// The taskbar button in slot `index` -- pinned applications first, then
    /// the windows, the two set apart by [`taskbar_divider_rect`](Self::taskbar_divider_rect).
    #[must_use]
    pub fn taskbar_button_rect(&self, index: usize) -> Rect {
        let bar = self.taskbar_rect();
        let w = self.taskbar_button_width();
        let inset = self.scale(TASKBAR_BUTTON_INSET).min(bar.h / 2.0);
        let past_the_pins = if index >= self.taskbar.pinned_apps().len() {
            self.section_gap_extra()
        } else {
            0.0
        };
        let x = bar.x
            + self.scale(START_BUTTON_WIDTH)
            + self.scale(TASKBAR_BUTTON_START_GAP)
            + index as f32 * (w + self.scale(TASKBAR_BUTTON_GAP))
            + past_the_pins;
        Rect::new(x, bar.y + inset, w, bar.h - inset * 2.0)
    }

    /// The line between the pinned buttons and the windows', in the middle of
    /// the gap between them -- `None` unless both sections have buttons.
    #[must_use]
    pub fn taskbar_divider_rect(&self) -> Option<Rect> {
        let pins = self.taskbar.pinned_apps().len();
        if pins == 0 || self.section_gap_extra() <= 0.0 {
            return None;
        }
        let last_pin = self.taskbar_button_rect(pins.saturating_sub(1));
        let first_window = self.taskbar_button_rect(pins);
        let thickness = self.scale(1.0).max(1.0);
        let middle = (last_pin.x + last_pin.w + first_window.x) / 2.0;
        Some(Rect::new(
            middle - thickness / 2.0,
            last_pin.y + last_pin.h * 0.2,
            thickness,
            last_pin.h * 0.6,
        ))
    }

    /// Every button the taskbar shows, pinned applications first.
    ///
    /// Pinned first and always shown, rather than merged with a window of the
    /// same program. Merging needs a name both ends agree on, and there is
    /// none: a window carries the `app_id` its program declares, while a
    /// pinned entry carries the executable path the launcher knows it by, and
    /// nothing in the tree maps one to the other. Showing both is honest --
    /// the pinned button is a *launcher*, and it keeps meaning that while the
    /// program runs -- and it is what a quick-launch strip has always done.
    /// Merging is a refinement for the day an application identity exists.
    #[must_use]
    pub fn taskbar_slots(&self) -> Vec<TaskbarSlot> {
        let mut slots: Vec<TaskbarSlot> = (0..self.taskbar.pinned_apps().len())
            .map(TaskbarSlot::Pinned)
            .collect();
        slots.extend(
            self.taskbar_button_windows()
                .iter()
                .map(|window| TaskbarSlot::Window(window.id)),
        );
        slots
    }

    /// The windows that have taskbar buttons, in the order the buttons stand:
    /// [`taskbar_windows`](Self::taskbar_windows)' set, in the taskbar's own
    /// order (see `button_order`) rather than the stacking order. Stable: the
    /// buttons stay put when a window is raised, and move only when the user
    /// drags one.
    #[must_use]
    pub fn taskbar_button_windows(&self) -> Vec<&ManagedWindow> {
        let mut windows = self.taskbar_windows();
        // A window the order has not seen yet -- which `apply_window_list`
        // makes impossible -- would go last rather than first.
        windows.sort_by_key(|w| {
            self.button_order
                .iter()
                .position(|id| *id == w.id)
                .unwrap_or(usize::MAX)
        });
        windows
    }

    /// The applications pinned to the taskbar, in the order they are shown.
    #[must_use]
    pub fn pinned_apps(&self) -> &[taskbar::PinnedApp] {
        self.taskbar.pinned_apps()
    }

    /// Whether `exec` is already pinned.
    #[must_use]
    pub fn is_pinned(&self, exec: &str) -> bool {
        self.taskbar
            .pinned_apps()
            .iter()
            .any(|a| a.exec_path == exec)
    }

    /// Pin an application to the taskbar, and remember it.
    ///
    /// Keyed on the executable path, which is what the launcher knows an
    /// application by and what a click has to hand to
    /// [`ShellAction::Launch`]. `app_id` is set to the same string so that
    /// `Taskbar::add_pinned`'s duplicate check -- which is on `app_id` -- means
    /// "already pinned" rather than "has the same empty name".
    pub fn pin_app(&mut self, exec: &str, name: &str) {
        if exec.is_empty() || self.is_pinned(exec) {
            return;
        }
        let position = u32::try_from(self.taskbar.pinned_apps().len()).unwrap_or(u32::MAX);
        self.taskbar.add_pinned(taskbar::PinnedApp {
            app_id: exec.to_string(),
            display_name: name.to_string(),
            icon_type: taskbar::IconType::Generic,
            exec_path: exec.to_string(),
            position,
        });
        self.save_pinned();
    }

    /// Unpin an application, and forget it.
    pub fn unpin_app(&mut self, exec: &str) {
        if !self.is_pinned(exec) {
            return;
        }
        self.taskbar.remove_pinned(exec);
        self.save_pinned();
    }

    /// Read the pinned applications back from `taskbar.yaml`.
    ///
    /// Names are *not* stored: a pin is an executable path, and the name shown
    /// on it comes from the launcher's entry for that path at the moment it is
    /// drawn. Storing the name too would be a second copy of it, stale the
    /// first time an application is renamed.
    pub fn load_pinned(&mut self) {
        let doc = config::load(TASKBAR_CONFIG_NAME);
        let Some(execs) = doc.get_seq(&["pinned"]) else {
            return;
        };
        for exec in execs {
            let name = self.app_name_for(&exec);
            self.pin_app_without_saving(&exec, &name);
        }
    }

    /// The launcher's name for an executable, or its file name.
    ///
    /// The fallback is the file name rather than the whole path: a button is
    /// narrow, and a pinned program the launcher has never heard of is still
    /// better labelled "editor" than "/usr/local/bin/editor".
    fn app_name_for(&self, exec: &str) -> String {
        if let Some(entry) = self.apps.iter().find(|a| a.executable_path == exec) {
            return entry.name.clone();
        }
        Path::new(exec)
            .file_name()
            .map_or_else(|| exec.to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// `pin_app` without the write, for the load path.
    ///
    /// Loading is not a change the user made, and writing the file back while
    /// reading it is how a partial read becomes a truncated file.
    fn pin_app_without_saving(&mut self, exec: &str, name: &str) {
        if exec.is_empty() || self.is_pinned(exec) {
            return;
        }
        let position = u32::try_from(self.taskbar.pinned_apps().len()).unwrap_or(u32::MAX);
        self.taskbar.add_pinned(taskbar::PinnedApp {
            app_id: exec.to_string(),
            display_name: name.to_string(),
            icon_type: taskbar::IconType::Generic,
            exec_path: exec.to_string(),
            position,
        });
    }

    /// Write the pinned applications to `taskbar.yaml`.
    ///
    /// A failed write is reported and the pin still applies to this session:
    /// refusing to pin because a disk is full helps nobody, and the user can
    /// see whether the button is there.
    fn save_pinned(&mut self) {
        let mut doc = config::load(TASKBAR_CONFIG_NAME);
        let execs: Vec<&str> = self
            .taskbar
            .pinned_apps()
            .iter()
            .map(|a| a.exec_path.as_str())
            .collect();
        doc.set_seq(&["pinned"], &execs);
        if let Err(err) = config::store(TASKBAR_CONFIG_NAME, &doc) {
            eprintln!("desktop: could not save taskbar.yaml: {err}");
        }
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
    /// `start_menu_entry_at`.
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
    /// The footer is the last `START_MENU_FOOTER` of the menu, or the whole
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

    /// A footer button beside Power: the two share what is left of the footer
    /// to its right, the terminal at the far end. They shrink before they
    /// overlap -- a menu clamped narrow at a large scale gives them less room
    /// -- and are the power button's height, so the footer reads as one row.
    #[must_use]
    pub fn start_shortcut_rect(&self, which: StartShortcut) -> Rect {
        let menu = self.start_menu_rect();
        let power = self.power_button_rect();
        let inset = self.scale(POWER_BUTTON_INSET);
        let gap = self.scale(START_SHORTCUT_GAP);
        let right = menu.x + menu.w - inset;
        let room = (right - (power.x + power.w + gap)).max(0.0);
        let w = self
            .scale(START_SHORTCUT_WIDTH)
            .min(((room - gap) / 2.0).max(0.0));
        let x = match which {
            StartShortcut::Terminal => right - w,
            StartShortcut::Settings => right - w - gap - w,
        };
        Rect::new(x, power.y, w, power.h)
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

    /// The programs the start menu lists, in menu order: the ones the user
    /// pinned first, then every program the launcher knows.
    ///
    /// System actions — shutdown, lock, log out — are deliberately excluded:
    /// they belong to the power options at the foot of the menu, not among the
    /// applications, and mixing them in would make "Shutdown" one mis-click
    /// away from "Screenshot".
    ///
    /// One list for the pins and the rest, rather than a second list drawn
    /// above the first: every row -- its hit test, its scroll, its
    /// right-click menu, a drag from it -- is then the same row, asked of
    /// the same index.
    ///
    /// While something is typed in the search field, only the programs it
    /// finds, best first and each once -- a pinned program is also in the
    /// list below, and a search that found it twice would say so twice.
    /// Ranked by the launcher's own rule (`launcher::search_score`); ties
    /// keep menu order.
    #[must_use]
    pub fn start_menu_entries(&self) -> Vec<&AppEntry> {
        let listed = self.start_pins.iter().chain(
            self.apps
                .iter()
                .filter(|app| matches!(app.category, Category::Application | Category::Setting)),
        );
        let query = self.start_query.text().trim();
        if query.is_empty() {
            return listed.collect();
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut found: Vec<(u32, usize, &AppEntry)> = listed
            .filter(|entry| seen.insert(entry.executable_path.as_str()))
            .enumerate()
            .filter_map(|(order, entry)| {
                launcher::search_score(query, entry).map(|score| (score, order, entry))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        found.into_iter().map(|(_, _, entry)| entry).collect()
    }

    /// How many of the rows at the top of the start menu are its pinned
    /// programs: all of them in the ordinary menu, none while searching --
    /// search results are in the order they were found, and not the user's to
    /// arrange by dropping onto them.
    fn start_pins_listed(&self) -> usize {
        if self.start_query.text().trim().is_empty() {
            self.start_pins.len()
        } else {
            0
        }
    }

    /// The start menu's search field, in the space above the rows.
    #[must_use]
    pub fn start_search_rect(&self) -> Rect {
        let menu = self.start_menu_rect();
        let inset = self.scale(12.0);
        Rect::new(
            menu.x + inset,
            menu.y + self.scale(10.0),
            (menu.w - inset * 2.0).max(0.0),
            self.scale(30.0),
        )
    }

    /// The programs pinned to the top of the start menu, in order.
    #[must_use]
    pub fn start_pins(&self) -> &[AppEntry] {
        &self.start_pins
    }

    /// Whether `exec` is pinned to the start menu.
    #[must_use]
    pub fn is_pinned_to_start(&self, exec: &str) -> bool {
        self.start_pins
            .iter()
            .any(|entry| entry.executable_path == exec)
    }

    /// Pin a program to the start menu, after the ones pinned already.
    pub fn pin_to_start(&mut self, exec: &str) {
        let end = self.start_pins.len();
        self.start_pin_into_gap(exec, end);
    }

    /// Take a program off the top of the start menu. It is still listed
    /// below if the launcher knows it.
    pub fn unpin_from_start(&mut self, exec: &str) {
        let before = self.start_pins.len();
        self.start_pins
            .retain(|entry| entry.executable_path != exec);
        if self.start_pins.len() != before {
            // The list is a row shorter, and a menu scrolled to its end
            // would otherwise show a blank row past it.
            self.start_menu_scroll = self.start_menu_scroll.min(self.start_menu_max_scroll());
            self.start_pins_dirty = true;
        }
    }

    /// Pin `exec` into gap `gap` of the start menu's pinned rows (`0..=len`,
    /// before the pin of that index), or move it there if it is pinned
    /// already. Answers the gap after where it ended up, as
    /// [`pin_into_gap`](Self::pin_into_gap) does for the taskbar, so several
    /// programs dropped together keep their order.
    fn start_pin_into_gap(&mut self, exec: &str, gap: usize) -> usize {
        if exec.is_empty() {
            return gap;
        }
        let from = self
            .start_pins
            .iter()
            .position(|entry| entry.executable_path == exec);
        let entry = match from {
            Some(from) => self.start_pins.remove(from),
            None => self.start_entry_for(exec),
        };
        // Taking it out closed its own gap: a gap past it is one lower now,
        // and the gaps either side of it are both where it already was.
        let to = match from {
            Some(from) if gap > from => gap.saturating_sub(1),
            _ => gap,
        }
        .min(self.start_pins.len());
        self.start_pins.insert(to, entry);
        if from != Some(to) {
            self.start_pins_dirty = true;
        }
        to.saturating_add(1)
    }

    /// What a program pinned to the start menu is listed as: the launcher's
    /// entry when it knows the program, so the name matches the row below;
    /// otherwise one made from the path and named for its file.
    ///
    /// Never the name it was dropped with -- a desktop icon the user renamed,
    /// say. Names are not stored (see [`load_pinned`](Self::load_pinned)), so
    /// a pin named any other way would change its name at the next login.
    fn start_entry_for(&self, exec: &str) -> AppEntry {
        self.apps
            .iter()
            .find(|app| app.executable_path == exec)
            .cloned()
            .unwrap_or_else(|| AppEntry {
                name: self.app_name_for(exec),
                description: String::new(),
                executable_path: exec.to_string(),
                keywords: Vec::new(),
                category: Category::Application,
                launch_count: 0,
            })
    }

    /// Read the programs pinned to the start menu back from `startmenu.yaml`.
    pub fn load_start_pins(&mut self) {
        let doc = config::load(START_MENU_CONFIG_NAME);
        let Some(execs) = doc.get_seq(&["pinned"]) else {
            return;
        };
        for exec in execs {
            if exec.is_empty() || self.is_pinned_to_start(&exec) {
                continue;
            }
            let entry = self.start_entry_for(&exec);
            self.start_pins.push(entry);
        }
    }

    /// Whether the start menu's pins need writing, clearing the flag.
    pub fn take_start_pins_dirty(&mut self) -> bool {
        core::mem::take(&mut self.start_pins_dirty)
    }

    /// Write the programs pinned to the start menu to `startmenu.yaml`.
    ///
    /// # Errors
    ///
    /// The write's own error. The pins still apply to this session.
    pub fn save_start_pins(&self) -> std::io::Result<()> {
        let mut doc = config::load(START_MENU_CONFIG_NAME);
        let execs: Vec<&str> = self
            .start_pins
            .iter()
            .map(|entry| entry.executable_path.as_str())
            .collect();
        doc.set_seq(&["pinned"], &execs);
        config::store(START_MENU_CONFIG_NAME, &doc)
    }

    /// The gap among the start menu's pinned rows a program let go at
    /// `(x, y)` goes into: before the pinned row it was let go on when on
    /// its upper half, after it when on its lower half, and after them all
    /// when let go on the start button.
    fn start_pin_insert_boundary(&self, x: f32, y: f32) -> usize {
        let pins = self.start_pins_listed();
        if let Hit::StartMenuEntry(index) = self.hit_test(x, y)
            && index < pins
            && let Some(row) = index.checked_sub(self.start_menu_scroll)
        {
            let rect = self.start_menu_row_rect(row);
            return if y < rect.y + rect.h / 2.0 {
                index
            } else {
                index.saturating_add(1)
            };
        }
        pins
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
            // The card is centred and the menu rises from the corner, so the two
            // do not overlap — but the card is dismissed anyway, because it is
            // opened to be *read* and a user who has gone to the start menu has
            // stopped reading it. Symmetrical with `toggle_shortcut_card`, which
            // closes this menu.
            self.shortcut_card_open = false;
            self.start_menu_scroll = 0;
            // The offset is being rewound, so the fraction that was pushing it
            // must be rewound too — otherwise a menu opened just after a
            // part-notch scroll steps off row 0 on the next small delta.
            self.start_menu_wheel.reset();
            // A search box opens empty: last time's search is not a question
            // anyone is asking now.
            self.start_query.clear();
            self.start_selected = None;
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
        // A drag from it has nothing left to drop from.
        self.start_drag = None;
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
            for which in StartShortcut::ALL {
                if self.start_shortcut_rect(*which).contains(x, y) {
                    return Hit::StartMenuShortcut(*which);
                }
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
            // Left of the clock, and tested after it: the two slots abut, and
            // `bell_rect` is derived from `clock_rect` so they cannot overlap —
            // but the order makes the clock's edge belong to the clock by rule
            // rather than by a rounding error going the way it happens to.
            if self.bell_rect().contains(x, y) {
                return Hit::NotificationBell;
            }
            // The application icons, left of the shell's own tray items and
            // tested before the window buttons: `tray_width` already reserved
            // this space, so a button cannot be here, but testing in the same
            // order the tray is laid out keeps the two from disagreeing if it
            // ever is.
            for (index, rect) in self.tray_icon_rects().iter().enumerate() {
                if rect.contains(x, y) {
                    return Hit::TrayIcon(index);
                }
            }
            // The chevron, if the run has one. After the icons only in source
            // order; its slot is disjoint from theirs.
            if let Some(rect) = self.tray_overflow_rect()
                && rect.contains(x, y)
            {
                return Hit::TrayOverflow;
            }
            // The slot is resolved to a window *here*, while the list that
            // produced the rectangle is still in hand — see
            // [`Hit::TaskbarButton`].
            for (index, slot) in self.taskbar_slots().iter().enumerate() {
                if self.taskbar_button_rect(index).contains(x, y) {
                    return match *slot {
                        TaskbarSlot::Window(id) => Hit::TaskbarButton(id),
                        TaskbarSlot::Pinned(pin) => Hit::TaskbarPinned(pin),
                    };
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
    ///
    /// The drain is here, wrapping the whole of the handling, rather than on
    /// the branch that forwards to the pane. *Every* path out of this function
    /// can have touched the pane — the bell toggles it, a press elsewhere on
    /// the bar dismisses it, `dismiss_popups` closes it along with everything
    /// else — and each of those reports a [`Closed`] nobody else empties. A
    /// drain on the forwarding branch alone bounded the buffer for clicks
    /// *inside* the pane and let it grow once per click that closed it, which
    /// is the more common of the two.
    ///
    /// [`Closed`]: notif_pane::NotifPaneEvent::Closed
    pub fn handle_mouse(&mut self, event: &MouseEvent) -> ShellAction {
        let action = self.handle_mouse_inner(event);
        match (action, self.apply_pane_events()) {
            // A click on a notification card that names a program. The pane
            // consumed the press and marked the card read; starting the
            // program is the part only the caller can do.
            // `PathBuf::from` at the edge: a notification's path comes from
            // its sender as text, so this is where text becomes a path.
            (ShellAction::Consumed, Some(path)) => {
                ShellAction::Launch(hotkeys::Launch::program(PathBuf::from(path)))
            }
            (action, _) => action,
        }
    }

    fn handle_mouse_inner(&mut self, event: &MouseEvent) -> ShellAction {
        // A rename under way owns the presses on its own field -- they place
        // the caret -- and any other press keeps the new name before it does
        // whatever it does, as a click away does on every desktop. First,
        // before the menus: a right-click that opens a menu is a click away
        // too, and the name must not be left half-typed under it.
        if self.icons.renaming().is_some()
            && let MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) = event.kind
        {
            if self.icons.rename_field_contains(event.x, event.y) {
                self.icons.rename_click(event.x);
                return ShellAction::Consumed;
            }
            self.icons_dirty |= self.icons.commit_rename();
        }

        // The desktop menu first, for the same reason the Run box's chooser is
        // first below: it is drawn over everything, so a press either landed on
        // it or dismissed it, and either way no control underneath should see
        // the same press.
        if self.desktop_menu.is_visible() {
            match event.kind {
                MouseEventKind::Move => {
                    self.desktop_menu.handle_mouse_move(event.x, event.y);
                    return ShellAction::Consumed;
                }
                MouseEventKind::Press(_) => {
                    match self.desktop_menu.handle_click(event.x, event.y) {
                        Some(id) => {
                            self.desktop_menu.hide();
                            // An icon's Open starts a program; every other
                            // item has already done its work.
                            if let ShellAction::Launch(launch) = self.activate_desktop_menu_item(id)
                            {
                                return ShellAction::Launch(launch);
                            }
                        }
                        // A press that named no item: on the panel's own
                        // padding, or outside it. `handle_click` cannot tell
                        // the caller which, and closing on both is the
                        // behaviour every menu has -- a click into the gap
                        // between two rows is a miss, not a hold.
                        None => self.desktop_menu.hide(),
                    }
                    return ShellAction::Consumed;
                }
                _ => return ShellAction::Consumed,
            }
        }
        // A drag in progress owns the pointer until the button comes up,
        // wherever it goes -- including over the taskbar and off the screen
        // edge. Dropping the widget the moment the pointer left the desktop
        // would make a widget near the bottom impossible to move, and every
        // toolkit lets a grab outlive the surface it started on.
        if self.widget_drag.is_some() {
            match event.kind {
                MouseEventKind::Move => {
                    self.drag_widget_to(event.x, event.y);
                    return ShellAction::Consumed;
                }
                MouseEventKind::Release(_) => {
                    // Marked dirty on release whether or not this last step
                    // moved it: the widget may have been carried across three
                    // cells and dropped back where the final `Move` already
                    // put it, and that is still a layout that differs from the
                    // one on disk.
                    self.drag_widget_to(event.x, event.y);
                    self.widget_drag = None;
                    self.widgets_dirty = true;
                    return ShellAction::Consumed;
                }
                _ => return ShellAction::Consumed,
            }
        }
        // The pin menu, for the same reason as the overflow list below it: it
        // is drawn over everything, so a press either landed on it or
        // dismissed it, and nothing underneath should see the same press.
        if self.pin_menu.is_some() {
            match event.kind {
                MouseEventKind::Move => {
                    if let Some((menu, _)) = self.pin_menu.as_mut() {
                        menu.handle_mouse_move(event.x, event.y);
                    }
                    return ShellAction::Consumed;
                }
                MouseEventKind::Press(_) => return self.click_pin_menu(event.x, event.y),
                _ => return ShellAction::Consumed,
            }
        }
        // The overflow list, ahead of everything below it for the same reason
        // the desktop menu is: it is drawn over the bar, so a press either
        // landed on it or dismissed it.
        if self.tray_overflow_menu.is_some() {
            match event.kind {
                MouseEventKind::Move => {
                    if let Some((menu, _)) = self.tray_overflow_menu.as_mut() {
                        menu.handle_mouse_move(event.x, event.y);
                    }
                    return ShellAction::Consumed;
                }
                MouseEventKind::Press(_) => return self.click_tray_overflow(event.x, event.y),
                _ => return ShellAction::Consumed,
            }
        }
        // A press on a tray icon owns the pointer until the button comes up,
        // like the widget drag above. The difference is what the grab is for:
        // a widget is being moved from the first pixel, whereas a tray icon
        // does not yet know what it is -- a click belongs to the program that
        // owns the icon, a drag belongs to the shell, and only the release
        // can tell them apart.
        // A pinned button owns the pointer until the button comes up, the
        // way a tray icon does and for the same reason: the press does not yet
        // know whether it is a click or a drag.
        // A start-menu row, the same way: the press does not yet know whether
        // it is a click or a carry. While the menu is open its surface covers
        // the screen, so the drag reaches the shell wherever the pointer goes.
        if self.start_drag.is_some() {
            match event.kind {
                MouseEventKind::Move => {
                    if let Some(drag) = self.start_drag.as_mut() {
                        drag.source.on_move(event.x, event.y);
                    }
                    self.carry_at = (event.x, event.y);
                    return ShellAction::Consumed;
                }
                MouseEventKind::Release(_) => {
                    return self.finish_start_press(event.x, event.y);
                }
                _ => {}
            }
        }
        if self.window_press.is_some() {
            match event.kind {
                MouseEventKind::Move => {
                    self.drag_window_button_to(event.x, event.y);
                    return ShellAction::Consumed;
                }
                MouseEventKind::Release(_) => return self.finish_window_press(),
                _ => {}
            }
        }
        if self.pin_drag.is_some() {
            match event.kind {
                MouseEventKind::Move => {
                    self.drag_pinned_to(event.x, event.y);
                    return ShellAction::Consumed;
                }
                MouseEventKind::Release(_) => {
                    return self.finish_pinned_press(event.x, event.y);
                }
                _ => {}
            }
        }
        if self.tray_drag.is_some() {
            match event.kind {
                MouseEventKind::Move => {
                    self.drag_tray_icon_to(event.x, event.y);
                    return ShellAction::Consumed;
                }
                MouseEventKind::Release(_) => return self.finish_tray_press(),
                _ => return ShellAction::Consumed,
            }
        }
        // A left press on a widget takes hold of it. Before the right-click
        // below only in source order; the two cannot both match, since a press
        // carries one button.
        if let MouseEventKind::Press(MouseButton::Left) = event.kind
            && !self.any_popup_open()
            && !self.taskbar_rect().contains(event.x, event.y)
            && self.begin_widget_drag(event.x, event.y)
        {
            return ShellAction::Consumed;
        }
        // A right-click on bare desktop opens it. Checked here rather than at
        // the bottom with the other background presses because the menu must
        // not be opened by a right-click that landed on the taskbar or a
        // window, and everything below this point has already claimed its own
        // rectangle.
        if let MouseEventKind::Press(MouseButton::Right) = event.kind
            && !self.any_popup_open()
            && !self.taskbar_rect().contains(event.x, event.y)
        {
            self.open_desktop_menu(event.x, event.y);
            return ShellAction::Consumed;
        }
        // The chooser ahead of the Run box that raised it, for the same reason
        // the Run box comes ahead of everything else: it is drawn last, so a
        // press anywhere landed on it or on the space around it.
        //
        // A press outside its rectangle does *nothing* rather than dismissing,
        // which is where this differs from the Run box below. The two are not
        // inconsistent: dismissing the Run box costs the user a line they can
        // retype, while dismissing the chooser costs them the navigation that
        // got them to the directory they are looking at. Escape is the way out,
        // and the Cancel button is the visible one.
        if self.run_browser.is_some() {
            let (x, y, width, height) = self.run_browser_rect();
            // The chooser lays itself out from its own origin — see
            // `FileDialog::frame` — so the event has to arrive in its space or
            // the clicks land somewhere other than the ink.
            let local = MouseEvent {
                x: event.x - x,
                y: event.y - y,
                kind: event.kind.clone(),
            };
            if let Some(dialog) = self.run_browser.as_mut() {
                let action = dialog.handle_mouse(&local, width, height);
                self.apply_run_browser_action(action);
            }
            return ShellAction::Consumed;
        }

        // The Run box first, ahead of even the pane's scrim: it is painted over
        // everything, so a press anywhere landed on it or on the space around
        // it, and the dialog answers both — inside, a button; outside, dismiss.
        // Either way the press is consumed, which is what "modal" means and is
        // the behaviour `RunDialog::handle_mouse_event` already implements.
        //
        // Motion is the one exception, and it is the dialog that makes it: a
        // move outside the box returns `Ignored` rather than dismissing, because
        // only a press dismisses. Consuming it anyway would freeze every hover
        // highlight on the desktop while the box is up; passing it on lets the
        // taskbar keep lighting under the cursor, which is what a user moving
        // the mouse *towards* the box sees.
        if self.run_dialog.is_visible() {
            let handled = self.run_dialog.handle_mouse_event(event);
            // `next()` rather than a loop, and nothing is thrown away by it: one
            // press reaches at most one button, and only the OK button executes,
            // so the drained list holds at most one path. The keyboard path
            // returns the whole `Vec` because `HotkeyOutcome` can carry one;
            // `ShellAction::Launch` names a single launch and cannot. A request
            // that opened nothing -- a path whose kind nothing opens -- has
            // already said so in a notification.
            if let Some(request) = self.drain_run_dialog().into_iter().next() {
                return self
                    .run_request(request)
                    .map_or(ShellAction::Consumed, ShellAction::Launch);
            }
            if handled == EventResult::Consumed {
                return ShellAction::Consumed;
            }
        }

        // Before the match, and before `handle_press`'s own overview check: the
        // notification pane draws a scrim across everything above the taskbar,
        // so while it is up an event there landed on it whatever is underneath.
        //
        // The taskbar is the exception, and deliberately so — see
        // [`notification_pane_height`](Self::notification_pane_height). The bar
        // is neither covered nor dimmed while the pane is open, so a press on
        // it is a real press on it, and falls through to `hit_test` below.
        // That is what lets the bell close the pane it opened.
        //
        // Everything else is consumed unconditionally rather than forwarding
        // the pane's `Ignored`. The pane ignores motion outside its column
        // because it has no hover state out there, not because the event
        // belongs to whatever is behind it; passing that through would light a
        // button under an overlay, which is exactly what the overview arm below
        // guards against.
        if self.notifications.pane_state().is_visible()
            && !self.taskbar_rect().contains(event.x, event.y)
        {
            // Result deliberately discarded: see the paragraph above. Whether
            // the pane found something under the cursor does not change the
            // answer, which is that the click cannot reach past the scrim.
            let _ = self.notifications.handle_mouse_event(
                event,
                self.screen_width as f32,
                self.notification_pane_height(),
            );
            // The events this produced are drained by `handle_mouse`, which
            // wraps this call and turns a clicked action into a `Launch`.
            return ShellAction::Consumed;
        }

        match event.kind {
            // These were one arm until the desktop icons were drawn, on the
            // stated grounds that "nothing it draws does anything on the second
            // click that it did not do on the first". That was true while
            // double-click-to-maximize was the only such gesture and belonged
            // to the compositor's title bar. An icon ends it: one click selects
            // it and two open it, which is the whole of what an icon is for.
            MouseEventKind::Press(button) => self.handle_press(event.x, event.y, button),
            MouseEventKind::DoubleClick(button) => {
                self.handle_icon_activate(event.x, event.y, button)
            }
            MouseEventKind::Scroll { dy, .. } => self.handle_scroll(event.x, event.y, dy),
            // A release belongs to whoever took the press, so chrome swallows
            // it: a client that saw a release it had no press for would treat a
            // click on the title bar as a click on itself.
            MouseEventKind::Release(button) => {
                // The icon layer first, and only when it is mid-gesture. A
                // press on an icon leaves it non-idle and *only* a release
                // returns it, so a release routed anywhere else would strand
                // the layer in `PendingDrag` for the rest of the session.
                // A program icon let go over the taskbar is pinned there and
                // stays where it was on the desktop: a drag between two
                // places copies. Anything else let go there -- a folder, a
                // document -- is refused by staying put, since a taskbar
                // button can only start a program.
                // Only a drag that got under way, and only the left button's
                // release, which is the one that ends it: a press that never
                // moved far enough is a click, and belongs to the icon layer
                // wherever it is let go.
                //
                // On the start button, the same programs are pinned to the
                // start menu instead, after the pins already there.
                if button == MouseButton::Left
                    && self.icons.drag_in_progress().is_some()
                    && self.taskbar_rect().contains(event.x, event.y)
                {
                    let to_start = matches!(
                        self.carry_target(event.x, event.y),
                        Some(CarryTarget::StartMenu)
                    );
                    let mut gap = if to_start {
                        self.start_pins.len()
                    } else {
                        self.pinned_insert_boundary(event.x)
                    };
                    for id in self.icons.cancel_drag() {
                        if let Some(exec) = self.icon_program(id) {
                            gap = if to_start {
                                self.start_pin_into_gap(&exec, gap)
                            } else {
                                let name = self
                                    .icons
                                    .get_icon(id)
                                    .map_or_else(|| exec.clone(), |icon| icon.label.clone());
                                self.pin_into_gap(&exec, &name, gap)
                            };
                        }
                    }
                    return ShellAction::Consumed;
                }
                if self.icons.is_interacting() {
                    // Where the icons ended up is what the user just chose, so
                    // it is saved at the end of this pump rather than at some
                    // later checkpoint that may never come -- by the session,
                    // which can report a failure; see `icons_dirty`. Only when
                    // something moved: a click that selected an icon has
                    // nothing to write.
                    self.icons_dirty |=
                        self.icons
                            .handle_mouse_up(event.x, event.y, icon_button(button));
                    return ShellAction::Consumed;
                }
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
                // A drag or a rubber-band on the desktop owns the pointer until
                // it is released. Gated on the layer being mid-gesture rather
                // than on the position, because a drag that leaves the icon
                // area still belongs to it -- that is what makes dragging an
                // icon to the far edge of the screen work at all.
                if self.icons.is_interacting() {
                    self.icons.handle_mouse_move(event.x, event.y, false);
                    return ShellAction::Consumed;
                }
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
                // Observed, not consumed: the pointer is only passing over the
                // bar on its way somewhere, and a client that stopped
                // receiving motion because the shell was showing a tooltip
                // would lose its own hover states.
                self.hover_tray(event.x, event.y);
                ShellAction::Pass
            }
        }
    }

    /// Note that the pointer is over a tray icon, or is no longer.
    ///
    /// Resolved through [`hit_test`](Self::hit_test) rather than by walking
    /// `tray_icon_rects` again, so that the icon a tooltip names and the icon
    /// a click reaches are decided by one piece of geometry. Two hit tests
    /// over the same row would be two chances to disagree, and the disagreement
    /// would read as the wrong name on the right icon.
    fn hover_tray(&mut self, x: f32, y: f32) {
        let over = match self.hit_test(x, y) {
            Hit::TrayIcon(index) => self.ordered_tray_icons().get(index).and_then(|icon| {
                // A program that registered no tooltip has given the shell
                // nothing to say. An empty bubble is worse than none.
                (!icon.tooltip.is_empty())
                    .then(|| (tray_dnd::TrayIconKey::of(icon), icon.tooltip.clone()))
            }),
            _ => None,
        };
        match over {
            None => self.tray_tooltip = None,
            Some((key, text)) => {
                // Already resting on this one: leave the hover running, or the
                // delay would restart on every motion event and the tooltip
                // would never appear.
                if self.tray_tooltip.as_ref().is_some_and(|(at, _)| *at == key) {
                    return;
                }
                let mut tip = guitk::menu::Tooltip::new(&text);
                tip.start_hover(x, y, self.osd_clock_ms, self.viewport());
                self.tray_tooltip = Some((key, tip));
            }
        }
    }

    /// Whether `icon` is the one currently under a drag.
    ///
    /// Asks the drag source rather than comparing positions: the icon keeps
    /// its slot while the *insertion point* moves, which is the whole design
    /// -- `DragSource` is keyed by `TrayIconKey` and not by index precisely
    /// because the run can rearrange under the pointer.
    fn tray_icon_is_being_dragged(&self, icon: &guiremote::tray::TrayIcon) -> bool {
        self.tray_drag.as_ref().is_some_and(|drag| {
            drag.source.show_ghost
                && drag.source.dragging_key.as_ref() == Some(&tray_dnd::TrayIconKey::of(icon))
        })
    }

    /// The tray tooltip's draw commands, empty unless one is showing.
    ///
    /// Drawn on the overlay surface beside the on-screen display, which is
    /// full-screen, above the menus and `input_transparent`. That last is why
    /// it belongs there rather than with the popups: `design-decisions.md`
    /// 566 put the overlay surface beyond the reach of the mouse because it is
    /// there to be read, and a tooltip is the same kind of thing -- a press
    /// aimed at the icon under it must reach the icon.
    #[must_use]
    pub fn render_tray_tooltip(&self) -> Option<RenderTree> {
        let (_, tip) = self.tray_tooltip.as_ref()?;
        if !tip.is_visible() {
            return None;
        }
        let mut tree = RenderTree::new();
        tree.commands
            .extend(tip.render(&Palette::from_settings(&self.appearance)));
        Some(tree)
    }

    /// What is being carried, and where it would go if let go now: a label
    /// that follows the pointer, naming the program and what letting go will
    /// do -- "Pin to taskbar", "Add to desktop", or nothing when it would do
    /// nothing. Drawn on the overlay surface, which takes no input, so the
    /// label never stands between the pointer and what it is over.
    ///
    /// Also for a program icon dragged over the taskbar, where the icon
    /// layer's own ghost is hidden under the bar.
    #[must_use]
    pub fn render_carry(&self) -> Option<RenderTree> {
        let (name, at) = if let Some(drag) = self.start_drag.as_ref() {
            if !drag.source.is_dragging() {
                return None;
            }
            (drag.name.clone(), self.carry_at)
        } else if let Some(drag) = self.pin_drag.as_ref() {
            if !drag.is_dragging() || !self.pin_drag_off_bar {
                return None;
            }
            let exec = drag.pressed_key()?;
            (self.app_name_for(&exec), self.carry_at)
        } else {
            let (at, ids) = self.icons.drag_in_progress()?;
            if !self.taskbar_rect().contains(at.0, at.1) {
                return None;
            }
            let program = ids
                .into_iter()
                .find(|id| self.icon_program(*id).is_some())?;
            (self.icons.get_icon(program)?.label.clone(), at)
        };
        let hint = match self.carry_target(at.0, at.1) {
            Some(CarryTarget::Taskbar) => Some("Pin to taskbar"),
            Some(CarryTarget::Desktop) => Some("Add to desktop"),
            Some(CarryTarget::StartMenu) => Some("Pin to Start menu"),
            None => None,
        };
        let p = Palette::from_settings(&self.appearance);
        let size = self.font_size(TextRole::Body);
        let (gap, pad) = (self.scale(2.0), self.scale(8.0));
        let weight = guitk::render::FontWeightHint::Regular;
        let line = text::line_height(size, weight);
        let name_w = text::measure(&name, size, weight);
        let hint_w = hint.map_or(0.0, |h| text::measure(h, size, weight));
        let h = match hint {
            Some(_) => line * 2.0 + gap + pad * 2.0,
            None => line + pad * 2.0,
        };
        let w = name_w.max(hint_w) + pad * 2.0;
        // Below and right of the pointer, so the pointer itself stays on
        // what it is over -- and flipped to the other side where that would
        // run off the screen, which over a taskbar along the bottom edge is
        // every time: the label is there to be read exactly when the pointer
        // is on the bar.
        let off = self.scale(14.0);
        let (screen_w, screen_h) = (self.screen_width as f32, self.screen_height as f32);
        let x = if at.0 + off + w <= screen_w {
            at.0 + off
        } else {
            (at.0 - off - w).max(0.0)
        };
        let y = if at.1 + off + h <= screen_h {
            at.1 + off
        } else {
            (at.1 - off - h).max(0.0)
        };
        let mut tree = RenderTree::new();
        tree.push(guitk::render::RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: p.surface0,
            corner_radii: CornerRadii::all(self.scale(6.0)),
        });
        tree.push(guitk::render::RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color: p.accent,
            line_width: 1.0,
            corner_radii: CornerRadii::all(self.scale(6.0)),
        });
        tree.push(guitk::render::RenderCommand::Text {
            x: x + pad,
            y: y + pad,
            text: name,
            color: p.text,
            font_size: size,
            font_weight: weight,
            max_width: None,
            overflow: guitk::render::TextOverflow::Clip,
        });
        if let Some(hint) = hint {
            tree.push(guitk::render::RenderCommand::Text {
                x: x + pad,
                y: y + pad + line + gap,
                text: hint.to_string(),
                color: p.subtext0,
                font_size: size,
                font_weight: weight,
                max_width: None,
                overflow: guitk::render::TextOverflow::Clip,
            });
        }
        Some(tree)
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
                | Hit::StartMenuShortcut(_)
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
                self.close_start_menu();
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

        // And for the notification pane, which by this point can only be a
        // press on the taskbar: every other press was consumed by the pane
        // above. Pressing the bell falls through to its own arm, which toggles
        // — otherwise the button that opened the pane would close it here and
        // then reopen it a line later. Anything else on the bar closes the pane
        // and is spent doing so, like every other dismissal in this function.
        if self.notifications.pane_state().is_visible() && !matches!(hit, Hit::NotificationBell) {
            self.notifications.hide();
            return ShellAction::Consumed;
        }

        // The chevron is the shell's own control, so unlike the icons beside
        // it only the primary button opens it -- consistent with the clock and
        // the bell, which are the other two things the shell owns in the tray.
        if matches!(hit, Hit::TrayOverflow) && button == MouseButton::Left {
            self.open_tray_overflow();
            return ShellAction::Consumed;
        }

        // A tray icon answers both buttons, so it is handled before the
        // primary-button gate below. Right-click is the tray's own gesture --
        // it is how every tray in existence opens an application's menu -- and
        // the icon belongs to another program, so what it means there is that
        // program's to decide, not this shell's to swallow.
        if let Hit::TrayIcon(index) = hit
            && let Some(icon) = self.ordered_tray_icons().get(index)
        {
            let mut source = tray_dnd::TrayDragSource::new();
            source.on_press(tray_dnd::TrayIconKey::of(icon), x, y);
            self.tray_drag = Some(TrayDrag { source, button });
            return ShellAction::Consumed;
        }

        // A right-click on a start-menu row offers to pin it. This is the one
        // place pinning can be offered from: pinning needs an executable path,
        // a window carries only the `app_id` its program declares, and nothing
        // in the tree maps one to the other -- so the taskbar itself cannot
        // say "pin this", however much that is where the button ends up.
        if button == MouseButton::Right {
            match hit {
                Hit::StartMenuEntry(index) => {
                    self.open_pin_menu(PinTarget::StartMenuRow(index), x, y);
                    return ShellAction::Consumed;
                }
                // And off it again from the button itself, which is where a
                // user looks for it. Only *unpinning* is offered here: a
                // taskbar button for a window cannot be pinned, because
                // pinning needs an executable path and a window does not
                // carry one. See
                // `TD-C-NOTHING-CONNECTS-A-LAUNCHER-ENTRY-TO-THE-WINDOWS-IT-OPENS`.
                Hit::TaskbarPinned(index) => {
                    self.open_pin_menu(PinTarget::Pinned(index), x, y);
                    return ShellAction::Consumed;
                }
                _ => {}
            }
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
            // `PathBuf::from` is the edge conversion: a menu entry's path comes
            // from a YAML config file, which the parser has already decoded as
            // UTF-8, so nothing is lost turning it back into a path here. It is
            // the *browsed* paths — see `RunDialog` — that cannot survive a
            // round trip through `String`, and those never pass through here.
            Hit::TaskbarPinned(index) => {
                // The press only takes hold; the *release* decides whether it
                // was a click or a drag. Launching here would start the
                // program every time the user began to rearrange the bar,
                // which is the same reason the tray waits for the release.
                if let Some(app) = self.taskbar.pinned_apps().get(index) {
                    let mut source = tray_dnd::DragSource::default();
                    source.on_press(app.exec_path.clone(), x, y);
                    self.pin_drag = Some(source);
                }
                ShellAction::Consumed
            }
            // The press only takes hold, as a pinned button's does: the
            // release decides whether it was a click, which starts the
            // program, or a drag, which carries it to the taskbar or the
            // desktop (`design.txt` line 712). This used to start the program
            // on the press, which is what made the row impossible to drag.
            Hit::StartMenuEntry(index) => {
                if let Some(entry) = self.start_menu_entries().get(index) {
                    let (exec, name) = (entry.executable_path.clone(), entry.name.clone());
                    let mut source = tray_dnd::DragSource::default();
                    source.on_press(exec, x, y);
                    self.start_drag = Some(StartDrag { source, name });
                    self.carry_at = (x, y);
                }
                ShellAction::Consumed
            }
            Hit::PowerButton => {
                self.toggle_power_menu();
                ShellAction::Consumed
            }
            // Starts its program, as a row does, and the menu gets out of the
            // way of the window it is about to open.
            Hit::StartMenuShortcut(which) => {
                self.close_start_menu();
                ShellAction::Launch(hotkeys::Launch::program(which.program()))
            }
            // A system action starts a program like any other menu entry: the
            // shell has no more business shutting the machine down itself than
            // it has starting a text editor itself. `/sbin/shutdown` and its
            // neighbours are what actually do it.
            Hit::PowerMenuEntry(index) => {
                let path = self
                    .power_menu_entries()
                    .get(index)
                    .map(|entry| PathBuf::from(&entry.executable_path));
                match path {
                    Some(path) => {
                        self.close_start_menu();
                        ShellAction::Launch(hotkeys::Launch::program(path))
                    }
                    None => ShellAction::Consumed,
                }
            }
            Hit::Clock => {
                self.toggle_calendar();
                ShellAction::Consumed
            }
            Hit::NotificationBell => {
                self.toggle_notifications();
                ShellAction::Consumed
            }
            // Handled above the primary-button gate, along with the chevron
            // beside it, because a tray icon answers the right button too. Reaching here means the icon went
            // away between the frame that drew it and the press; consumed
            // rather than passed on, because the user aimed at the tray and
            // letting the press fall through to whatever is behind it would
            // act on something they were not pointing at.
            Hit::TrayIcon(_) => ShellAction::Consumed,
            // Likewise: opened above. Reaching here is a non-primary press on
            // the chevron, which the shell owns and so swallows.
            Hit::TrayOverflow => ShellAction::Consumed,
            Hit::CalendarControl(control) => {
                self.calendar.apply(control);
                ShellAction::Consumed
            }
            Hit::StartMenuPanel | Hit::PowerMenuPanel | Hit::TaskbarPanel => ShellAction::Consumed,
            // The press only takes hold, as a pinned button's does: the
            // release decides whether it was a click, which summons or
            // minimises the window, or a drag along the row, which moves the
            // button. See `finish_window_press`.
            Hit::TaskbarButton(id) => {
                let mut source = tray_dnd::DragSource::default();
                source.on_press(id, x, y);
                self.window_press = Some(WindowPress {
                    source,
                    was_focused: self.focused_window == Some(id),
                });
                ShellAction::Consumed
            }
            // The desktop is the icon layer's. A press here selects an icon,
            // clears the selection, or starts a rubber-band, and the layer
            // answers whether it took it.
            //
            // It used to focus the window it thought was there, which was both
            // a guess -- the shell knew no window rectangles then -- and a
            // change to a list the next event from the compositor would
            // overwrite.
            Hit::Desktop => {
                let before = self.icons.selected_ids();
                self.icons
                    .handle_mouse_down(x, y, icon_button(button), false);
                if self.icons.selected_ids() == before {
                    // Nothing the user can see changed -- a press on empty
                    // desktop with nothing selected. Still `Pass`, which is the
                    // property `the_bare_desktop_is_not_the_shells_to_consume`
                    // has asserted since before there were icons: the shell
                    // does not claim a press it did not act on, and `Consumed`
                    // is what marks the frame dirty.
                    //
                    // The first version of this arm consumed unconditionally,
                    // on the theory that a rubber-band is a gesture in
                    // progress. It is -- but the gesture does not need the
                    // press *claimed*: the release reaches this surface either
                    // way. Three existing tests said so and were right.
                    ShellAction::Pass
                } else {
                    ShellAction::Consumed
                }
            }
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
    /// Per-window shell-local state that the compositor has no opinion about:
    /// the icon, and the two "leave me out of that list" flags a window rule
    /// may have set. A window already known keeps all three across the update.
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
    /// bottom-to-top, so `taskbar_windows().last()` is the topmost window here
    /// for the same reason it is there.
    ///
    /// # What comes back
    ///
    /// The [window rules](crate::window_rules) a *newly arrived* window matched,
    /// turned into asks for the compositor. The caller must send them — see
    /// [`ShellSession`](session::ShellSession), which does. Returning them
    /// rather than sending them keeps this method what the rest of the shell
    /// is: something that computes an answer and hands it over, with no
    /// connection of its own.
    ///
    /// "Newly arrived" means an id the shell was not already holding, which is
    /// why it can be read off `previous` and needs no second set to remember.
    /// Ids are issued by a sequence and never reused, so a window cannot arrive
    /// twice under the same name.
    pub fn apply_window_list(&mut self, list: &WindowList) -> Vec<ShellRequest> {
        let mut kept: BTreeMap<WindowId, ManagedWindow> = BTreeMap::new();
        let mut focused = None;
        let mut requests = Vec::new();
        self.current_desktop = list.current_workspace;

        for (index, info) in list.windows.iter().enumerate() {
            if info.layer != Layer::Normal {
                continue;
            }
            let id = WindowId(info.id);
            // Read out rather than held: `evaluate` below needs `&mut
            // self.rules`, and a live borrow of `self.windows` across it would
            // be a borrow of the same `self`.
            let carried = self
                .windows
                .get(&id)
                .map(|w| (w.icon_id, w.skip_taskbar, w.skip_alt_tab));
            let (icon_id, mut skip_taskbar, mut skip_alt_tab) =
                carried.unwrap_or((0, false, false));

            if carried.is_none() {
                let actions = self.rules.evaluate(&info.title, &info.app_id);
                skip_taskbar = actions.skip_taskbar.unwrap_or(false);
                skip_alt_tab = actions.skip_alt_tab.unwrap_or(false);
                requests.extend(Self::rule_requests(
                    id,
                    info,
                    &actions,
                    self.num_desktops,
                    (self.screen_width, self.screen_height),
                ));
            }

            // Recorded on every list, not only on arrival: "remember last
            // position" means the position the window was last *at*, and the
            // window moves long after it arrives. Skipped while minimised or
            // maximised, because the rectangle then is the state's and not the
            // window's — restoring one would hand back a full-screen size the
            // user never chose.
            if !info.minimized && !info.maximized {
                self.rules
                    .remember_state(&info.app_id, info.x, info.y, info.width, info.height);
            }

            if info.focused {
                focused = Some(id);
            }
            kept.insert(
                id,
                ManagedWindow {
                    id,
                    title: info.title.clone(),
                    // Not carried over from `previous` the way the icon is. An
                    // icon is shell-local state the compositor has no opinion
                    // about; this is the compositor's own report, and a window
                    // whose program changed its mind should be described the
                    // way the latest list describes it.
                    app_id: info.app_id.clone(),
                    skip_taskbar,
                    skip_alt_tab,
                    state: if info.minimized {
                        WindowState::Minimized
                    } else if info.maximized {
                        WindowState::Maximized
                    } else {
                        WindowState::Normal
                    },
                    desktop: info.workspace,
                    focused: info.focused,
                    // The compositor's mapped flag, alone and unmodified. It
                    // used to be `info.visible && !info.minimized`, which
                    // conflated "the program took its window away" with "the
                    // user put it away" — and since the taskbar, the Alt+Tab
                    // switcher and the overview all list this set, a minimised
                    // window lost its button, dropped out of the switcher and
                    // became unreachable by any means the shell offers. The
                    // minimised state is recorded in `state` and asked for by
                    // name where it matters (`on_glass`).
                    mapped: info.visible,
                    // `WindowInfo::pid` is the compositor's `u64`; the shell's
                    // field is a `u32` for the same reason a pid is one
                    // everywhere else. Truncating would make two processes
                    // indistinguishable to a "group this program's windows"
                    // feature, so it saturates instead — a pid that large is
                    // already outside anything the system can produce.
                    pid: u32::try_from(info.pid).unwrap_or(u32::MAX),
                    icon_id,
                    frame: Rect::new(
                        info.x as f32,
                        info.y as f32,
                        info.width as f32,
                        info.height as f32,
                    ),
                    z_order: u32::try_from(index).unwrap_or(u32::MAX),
                },
            );
        }

        self.windows = kept;
        // The taskbar's own order follows the list without taking its order:
        // a window that went leaves it, a window that arrived joins the end
        // -- several arriving together, in the order they were stacked,
        // which is the order they were opened in.
        self.button_order.retain(|id| self.windows.contains_key(id));
        for info in &list.windows {
            let id = WindowId(info.id);
            if self.windows.contains_key(&id) && !self.button_order.contains(&id) {
                self.button_order.push(id);
            }
        }
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
        requests
    }

    /// Turn the actions a rule matched into asks the compositor understands.
    ///
    /// Five of [`RuleActions`](window_rules::RuleActions)'s seventeen fields
    /// have somewhere to go. Two of those — `skip_taskbar` and `skip_alt_tab` —
    /// are the shell's own business and are handled by the caller; the three
    /// here need the compositor.
    ///
    /// The twelve that are missing are missing on purpose, not by oversight.
    /// `position` and `size` are the loudest: the control protocol's `Move` and
    /// `Resize` resolve against the *sender's own* window, so the shell cannot
    /// use them on somebody else's, and placement is the compositor's to decide
    /// (§506) — the shell is not even told the display bounds. `always_on_top`,
    /// `opacity`, `no_decorations`, `prevent_close` and the rest have no
    /// request at all. They are stored, exported and shown, and doing nothing
    /// visible is a better failure than a shell that moves windows to the
    /// wrong place. See `known-issues.md`
    /// `TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO`.
    fn rule_requests(
        id: WindowId,
        info: &WindowInfo,
        actions: &window_rules::RuleActions,
        num_desktops: u32,
        // Passed rather than read off `self`, like `num_desktops` beside it:
        // this stays an associated function so a test can drive one rule
        // without standing a whole shell up around it.
        screen: (u32, u32),
    ) -> Vec<ShellRequest> {
        let mut out = Vec::new();

        // Desktop first: where the window lives, before anything about how it
        // looks there. Asking for the desktop it is already on would be a
        // request the compositor answers by switching to it — activating a
        // window files nothing, but `MoveWindowToDesktop` still costs a
        // recomposite — so the no-op is dropped here.
        if let Some(desktop) = actions.desktop {
            if desktop < num_desktops && desktop != info.workspace {
                out.push(ShellRequest::MoveWindowToDesktop {
                    window: id,
                    desktop,
                });
            }
        }

        // `Normal` is not "restore it": a rule that says a window should start
        // normal is describing what a window already is, and sending `Restore`
        // to a window that opened maximized of its own accord would be the rule
        // overriding the program rather than the default.
        match actions.initial_state {
            Some(window_rules::InitialState::Minimized) => {
                out.push(ShellRequest::window(id, ShellControlAction::Minimize));
            }
            Some(window_rules::InitialState::Maximized) => {
                out.push(ShellRequest::window(id, ShellControlAction::Maximize));
            }
            // Fullscreen is the display; Maximize is the work area. A rule
            // asking for fullscreen wants the taskbar covered too, so this is
            // not a synonym for the arm above.
            Some(window_rules::InitialState::Fullscreen) => {
                out.push(ShellRequest::window(id, ShellControlAction::Fullscreen));
            }
            Some(window_rules::InitialState::Normal) | None => {}
        }

        // What the user may not do. Sent whenever the rule names any of the
        // three, and it names them as a set: a rule that stops saying
        // `prevent_close` is taking it back, so the policy is replaced whole
        // rather than merged into whatever was there.
        let policy = WindowPolicy {
            prevent_close: actions.prevent_close.unwrap_or(false),
            prevent_move: actions.prevent_move.unwrap_or(false),
            prevent_resize: actions.prevent_resize.unwrap_or(false),
        };
        if actions.prevent_close.is_some()
            || actions.prevent_move.is_some()
            || actions.prevent_resize.is_some()
        {
            out.push(ShellRequest::SetWindowPolicy { window: id, policy });
        }

        // Size limits before the size itself, so a rule that sets both does
        // not resize to something its own maximum then claws back.
        if actions.min_size.is_some() || actions.max_size.is_some() {
            out.push(ShellRequest::SetSizeLimits {
                window: id,
                // `(0, 0)` is "this rule says nothing about that limit", which
                // leaves whatever the program asked for in place. A rule that
                // names only a maximum must not discard the program's own
                // minimum.
                min: actions.min_size.unwrap_or((0, 0)),
                max: actions.max_size.unwrap_or((0, 0)),
            });
        }

        // Size before position, and both before the state match below. A
        // window resized after being placed keeps its top-left corner, so the
        // order does not change where it lands -- but a *centred* placement is
        // computed from the size, so the size has to be settled first.
        if let Some(size) = actions.size {
            if let Some((width, height)) = Self::rule_size_px(size, screen) {
                out.push(ShellRequest::ResizeWindow {
                    window: id,
                    width,
                    height,
                });
            }
        }

        if let Some(position) = actions.position {
            if let Some((x, y)) = Self::rule_position_px(position, actions.size, screen) {
                out.push(ShellRequest::MoveWindow { window: id, x, y });
            }
        }

        // "Always on top" and "always on bottom" are one setting with three
        // values, not two independent flags: a rule asking for both would be
        // asking for a window to be above and below its neighbours at once.
        // `always_on_top` wins that argument here rather than the compositor
        // being handed a contradiction to resolve.
        let tier = match (actions.always_on_top, actions.always_on_bottom) {
            (Some(true), _) => Some(StackTier::Top),
            (_, Some(true)) => Some(StackTier::Bottom),
            // An explicit `false` is a rule saying "ordinary", which is a
            // request: it undoes a tier a higher-priority rule set. `None` is
            // a rule that says nothing, and says nothing here too.
            (Some(false), _) | (_, Some(false)) => Some(StackTier::Normal),
            (None, None) => None,
        };
        if let Some(tier) = tier {
            out.push(ShellRequest::SetStackTier { window: id, tier });
        }

        // Opacity is independent of state and zone -- a window can be
        // maximised *and* translucent -- so it is not part of the match above
        // and does not compete with it for ordering.
        if let Some(opacity) = actions.opacity {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0.0..=1.0 first, so the product is 0.0..=255.0                           and rounds into a u8 exactly"
            )]
            let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.push(ShellRequest::SetOpacity { window: id, alpha });
        }

        // Last, so that a rule setting both a state and a zone lands in the
        // zone: a snap is the more specific of the two instructions, and the
        // compositor applies whichever it is told about second.
        if let Some(zone) = actions.snap_zone {
            if let Some(slot) = u8::try_from(zone).ok().and_then(snap::SnapSlot::from_index) {
                out.push(ShellRequest::window(
                    id,
                    ShellControlAction::SnapToZone(slot),
                ));
            }
        }

        out
    }

    /// The windows the shell **lists** on the current desktop, in stacking
    /// order (bottom-to-top, so `.last()` is the topmost).
    ///
    /// This is the taskbar's set and the overview's set, and it deliberately
    /// **includes minimised windows**: a taskbar button exists precisely so
    /// that a window the user put away can be got back. It used to filter on a
    /// flag that folded "unmapped" together with "minimised", which meant
    /// minimising a window removed its button *and* its switcher row in the
    /// same instant — leaving it reachable by nothing the shell draws. The
    /// `Activate`-rather-than-`Restore` care taken in `handle_press` was
    /// unreachable code for exactly as long as that lasted.
    ///
    /// It is no longer the Alt+Tab switcher's set: see
    /// [`switcher_windows`](Self::switcher_windows).
    ///
    /// For the narrower "is it being drawn?" question, ask
    /// [`ManagedWindow::on_glass`].
    #[must_use]
    pub fn taskbar_windows(&self) -> Vec<&ManagedWindow> {
        self.listed_windows(|w| !w.skip_taskbar)
    }

    /// The windows Alt+Tab cycles through, in the same order.
    ///
    /// The same set as [`taskbar_windows`](Self::taskbar_windows) until a
    /// window rule says otherwise, and a separate method because
    /// `skip_taskbar` and `skip_alt_tab` are separate rule actions: a docked
    /// chat panel may want no button but must stay reachable from the
    /// keyboard, and a background helper the other way round. One list serving
    /// both would silently make each flag mean the other as well.
    ///
    /// The switcher's index counts into *this* list. That is why the two are
    /// derived from one filter with one sort rather than written twice — an
    /// index into a differently-ordered list would switch to a window the user
    /// was not looking at.
    #[must_use]
    pub fn switcher_windows(&self) -> Vec<&ManagedWindow> {
        self.listed_windows(|w| !w.skip_alt_tab)
    }

    /// The windows on the current desktop that `also` admits, bottom-to-top.
    fn listed_windows(&self, also: impl Fn(&ManagedWindow) -> bool) -> Vec<&ManagedWindow> {
        let mut windows: Vec<&ManagedWindow> = self
            .windows
            .values()
            .filter(|w| w.mapped && w.desktop == self.current_desktop && also(w))
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
    /// Alt+Tab is for. [`switcher_windows`](Self::switcher_windows) is ordered
    /// bottom to top, so it is the second entry from the *end* — not index 1,
    /// which is what this used to say. With exactly two windows index 1 *is*
    /// the focused window, so press-and-release Alt+Tab — much the commonest
    /// use there is — re-focused the window you were already in and appeared to
    /// do nothing at all.
    pub fn start_alt_tab(&mut self) {
        let count = self.switcher_windows().len();
        if count > 1 {
            self.alt_tab_active = true;
            self.alt_tab_index = step::wrapping_before(count, count.saturating_sub(1));
        }
    }

    pub fn next_alt_tab(&mut self) {
        let count = self.switcher_windows().len();
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
        let count = self.switcher_windows().len();
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
        let id = self.switcher_windows().get(self.alt_tab_index)?.id;
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
    ///
    /// Wrapped for the same reason [`handle_mouse`](Self::handle_mouse) is: a
    /// key can close the pane by several routes — Escape into the pane itself,
    /// Escape into `dismiss_popups`, Super+N — and every one of them reports a
    /// `Closed` that nothing else empties.
    ///
    /// Nothing the pane reports from a key is a launch, and it is as well:
    /// [`HotkeyOutcome`] carries compositor requests, not program starts.
    /// Should the pane grow an "Enter opens the selected card" key, it needs a
    /// launch channel here rather than a path quietly dropped — see todo.txt.
    pub fn handle_hotkey(&mut self, key: &KeyEvent) -> HotkeyOutcome {
        let outcome = self.handle_hotkey_inner(key);
        drop(self.apply_pane_events());
        outcome
    }

    /// Act on a modifier-only chord the shell claimed.
    ///
    /// [`handle_hotkey`](Self::handle_hotkey)'s counterpart for the gestures
    /// that have no key. The mapping from chord to action lives here rather
    /// than in the session for the same reason the hotkey registry does: the
    /// shell is what asked the compositor for this chord, so the shell is the
    /// only thing that knows what it asked for it *for*.
    ///
    /// A chord that matches nothing is ignored rather than assumed. Today
    /// exactly one is ever claimed — the keyboard-layout switcher — and
    /// "whatever arrives must be that one" would be right today and silently
    /// wrong the first time a second chord is added.
    pub fn handle_modifier_chord(&mut self, modifiers: Modifiers) -> HotkeyOutcome {
        // While the user is recording a new shortcut, every keystroke belongs
        // to the recording. Letting go of Alt+Shift mid-capture must not also
        // change the keyboard layout out from under them. Only while
        // *recording*: typing a search into the card is exactly when a user
        // may need the other layout.
        if self.shortcut_card_open && self.shortcut_editor.is_recording() {
            return HotkeyOutcome::ignored();
        }
        if self.input_methods.switch_shortcut.as_modifier_chord() != Some(modifiers) {
            return HotkeyOutcome::ignored();
        }
        let outcome = self.run_desktop_action(&HotkeyAction::SwitchInputLayout);
        drop(self.apply_pane_events());
        outcome
    }

    fn handle_hotkey_inner(&mut self, key: &KeyEvent) -> HotkeyOutcome {
        if !key.pressed {
            // Key release — check for Alt+Tab completion
            if (key.key == Key::LeftAlt || key.key == Key::RightAlt) && self.alt_tab_active {
                return HotkeyOutcome::ask(self.finish_alt_tab());
            }
            return HotkeyOutcome::ignored();
        }

        // Before everything, including the modal surfaces below. While the card
        // is recording keys, a keystroke is data, and the one thing that must
        // not happen is the shell running it -- a user rebinding "close window"
        // would otherwise close a window while trying to say which keys mean
        // it. While it is choosing an action or taking a command, keystrokes
        // are typing. Either way the editor owns them, and only on the plain
        // list does it hand back what is not its own, so a shortcut still works
        // with the card open.
        if self.shortcut_card_open {
            let cx = self.shortcut_context();
            match self.shortcut_editor.handle_key(key, &mut self.hotkeys, cx) {
                shortcut_editor::Outcome::NotMine => {}
                shortcut_editor::Outcome::Handled => return HotkeyOutcome::ignored(),
                shortcut_editor::Outcome::Changed => {
                    self.save_edited_shortcuts();
                    return HotkeyOutcome::ignored();
                }
                shortcut_editor::Outcome::Close => {
                    self.shortcut_card_open = false;
                    self.shortcut_editor.reset();
                    return HotkeyOutcome::ignored();
                }
            }
        }

        // The pin menu, on the same terms as the two below it: a popup that
        // owns the keyboard while it is up. Without this it could be opened
        // and then only used with the mouse, and Escape would do whatever the
        // global table says rather than closing the thing in front of you.
        if self.pin_menu.is_some() {
            let chosen = self.pin_menu.as_mut().map(|(menu, _)| menu.handle_key(key));
            match chosen {
                Some(Some(MenuAction::Selected(id))) => {
                    let target = self.pin_menu.as_ref().map(|(_, target)| *target);
                    self.pin_menu = None;
                    if let Some(target) = target {
                        self.activate_pin_menu_item(id, target);
                    }
                }
                Some(Some(MenuAction::Closed)) => self.pin_menu = None,
                // It moved its highlight, or ignored the key. Either way it
                // stays open and the press goes no further.
                _ => {}
            }
            return HotkeyOutcome::consumed();
        }

        // The overflow list, on the same terms as the desktop menu below and
        // ahead of it only because the two cannot both be open. Without this
        // the list could be opened and then only used with the mouse, which
        // for a popup whose entire purpose is to reach icons too small to see
        // would be a strange place to require fine pointing.
        if self.tray_overflow_menu.is_some() {
            let outcome = match self
                .tray_overflow_menu
                .as_mut()
                .map(|(menu, _)| menu.handle_key(key))
            {
                Some(Some(MenuAction::Selected(id))) => self.activate_overflow_row(id),
                Some(Some(MenuAction::Closed)) => {
                    self.tray_overflow_menu = None;
                    HotkeyOutcome::consumed()
                }
                // The list moved its highlight, or ignored the key. Either way
                // it stays open and the press goes no further.
                _ => HotkeyOutcome::consumed(),
            };
            return outcome;
        }

        // The desktop menu owns the keyboard while it is up, for the reason
        // every modal surface here does: arrows walk its rows, Enter chooses,
        // Escape closes, and none of those should also do whatever the global
        // shortcut table says. Without this the menu could be opened and then
        // only dismissed with the mouse.
        if self.desktop_menu.is_visible() {
            return match self.desktop_menu.handle_key(key) {
                Some(MenuAction::Selected(id)) => {
                    self.desktop_menu.hide();
                    match self.activate_desktop_menu_item(id) {
                        ShellAction::Launch(launch) => HotkeyOutcome::start(vec![launch]),
                        _ => HotkeyOutcome::consumed(),
                    }
                }
                Some(MenuAction::Closed) => {
                    self.desktop_menu.hide();
                    HotkeyOutcome::consumed()
                }
                // The menu moved its highlight, or ignored the key. Either way
                // it stays open and the press goes no further.
                Some(MenuAction::None) | None => HotkeyOutcome::consumed(),
            };
        }

        // The start menu owns the keyboard while it is open, as every popup
        // here does: typing searches it, the arrows walk its rows, Enter
        // starts one and Escape empties the search, then closes the menu.
        // After the pin menu, which opens over it and so owns the keys first.
        if self.start_menu_open {
            return self.key_on_start_menu(key);
        }

        // The overview gets every press before the shortcut table does, and
        // swallows the ones it does not recognise. It has a text field in it:
        // if the table went first, typing "d" into the search bar would show the
        // desktop out from under the overlay the user is typing into, and "e"
        // would open a file manager behind it. A modal surface with a text field
        // has to be modal about keys as well as clicks.
        // The Run box first, because it is drawn last: `paint_chrome` puts it
        // over every other popup, and the surface on top is the one that owns
        // the keyboard. In practice nothing else is open underneath it —
        // `toggle_run_dialog` dismisses the popups on the way in and a click
        // outside the box closes the box — but the ordering here has to agree
        // with the paint order regardless of whether the case arises, or the
        // day it does arise is the day keys go to a window the user cannot see.
        // And the chooser ahead of the Run box that raised it, for exactly the
        // same reason: it is drawn over the box, so it owns the keyboard while
        // it is up. Without this, the arrow keys that walk the file list would
        // be walking the box's history at the same time, and Escape would close
        // the box out from under the chooser rather than closing the chooser.
        // Note that Super+R is *not* special-cased through here the way it is in
        // `key_on_run_dialog`: while a chooser is up, the box the chord toggles
        // is not the surface the user is looking at.
        if self.run_browser.is_some() {
            return self.key_on_run_browser(key);
        }

        if self.run_dialog.is_visible() {
            return self.key_on_run_dialog(key);
        }

        if self.overview.visible {
            return self.key_on_overview(key);
        }

        // The notification pane is modal for the same reason and gets presses
        // on the same terms — with the same one-way-toggle exception, so that
        // the chord that opened it closes it again.
        if self.notifications.pane_state().is_visible() {
            if self.bound_action(key) == Some(HotkeyAction::ToggleNotifications) {
                return self.run_desktop_action(&HotkeyAction::ToggleNotifications);
            }
            // Pull-on-use, like `sync_osd_screen` and `sync_snap_area` and for
            // the same reason: `screen_height` is a public field anything may
            // assign, so a push-on-resize scheme is one forgotten call site
            // away from being wrong. The pane learns the height from every
            // *mouse* event by itself; the keyboard path carries no geometry,
            // and without this the arrow keys clamped against the pane's
            // pre-first-render default of 1080 -- so on a shorter display the
            // last notifications could not be reached by keyboard at all, and
            // on a taller one the list scrolled past its own end.
            self.sync_notification_screen();
            // Result deliberately discarded: consumed even when the pane had no
            // meaning for the key, because a press the overlay did not use is
            // not therefore the desktop's.
            let _ = self.notifications.handle_key_event(key);
            return HotkeyOutcome::consumed();
        }

        match self.bound_action(key) {
            Some(action) => self.run_desktop_action(&action),
            None => HotkeyOutcome::ignored(),
        }
    }

    /// One press while the start menu is open.
    ///
    /// Typed text goes to the search field, and every key that is not the
    /// field's goes no further than the menu -- except a chord with Super,
    /// which is the desktop's: the Super key that opened the menu closes it
    /// again, and Super+E closes it and opens the file manager.
    fn key_on_start_menu(&mut self, key: &KeyEvent) -> HotkeyOutcome {
        let is_super =
            key.modifiers.super_key || matches!(key.key, Key::LeftSuper | Key::RightSuper);
        if is_super {
            return match self.bound_action(key) {
                // The chord that opened the menu closes it.
                Some(HotkeyAction::ToggleStartMenu) => {
                    self.run_desktop_action(&HotkeyAction::ToggleStartMenu)
                }
                // Any other gets the menu out of its way first. The bare Super
                // key opens the menu as it goes down, so Super+E arrives with
                // the menu open; left there, it would sit over the file
                // manager the chord just asked for.
                Some(action) => {
                    self.close_start_menu();
                    self.run_desktop_action(&action)
                }
                None => HotkeyOutcome::consumed(),
            };
        }
        match key.key {
            Key::Escape => {
                if self.start_query.text().is_empty() {
                    self.close_start_menu();
                } else {
                    self.start_query.clear();
                    self.search_changed();
                }
                HotkeyOutcome::consumed()
            }
            Key::Down => {
                self.move_start_selection(true);
                HotkeyOutcome::consumed()
            }
            Key::Up => {
                self.move_start_selection(false);
                HotkeyOutcome::consumed()
            }
            Key::Enter => self.start_menu_enter(),
            _ => {
                let size = self.font_size(TextRole::Body);
                match self
                    .start_query
                    .edit_key(key, size, guitk::render::FontWeightHint::Regular)
                {
                    KeyEdit::Changed => {
                        self.search_changed();
                        HotkeyOutcome::consumed()
                    }
                    KeyEdit::Handled => HotkeyOutcome::consumed(),
                    // Not the field's -- Tab, a function key, Alt+Tab. A
                    // shortcut still works with the menu up; anything else
                    // goes no further than the menu.
                    KeyEdit::Unhandled => match self.bound_action(key) {
                        Some(action) => self.run_desktop_action(&action),
                        None => HotkeyOutcome::consumed(),
                    },
                }
            }
        }
    }

    /// The search changed: the list is a different list, so the keyboard's
    /// row and the scroll through the old one mean nothing now.
    fn search_changed(&mut self) {
        self.start_selected = None;
        self.start_menu_scroll = 0;
        self.start_menu_wheel.reset();
    }

    /// Move the keyboard's row one step, keeping it on screen. From no row,
    /// Down goes to the first and Up to the last.
    fn move_start_selection(&mut self, down: bool) {
        let count = self.start_menu_entries().len();
        let Some(last) = count.checked_sub(1) else {
            return;
        };
        let next = match (self.start_selected, down) {
            (None, true) => 0,
            (None, false) => last,
            (Some(row), true) => row.saturating_add(1).min(last),
            (Some(row), false) => row.saturating_sub(1),
        };
        self.start_selected = Some(next);
        let rows = self.start_menu_visible_rows().max(1);
        if next < self.start_menu_scroll {
            self.start_menu_scroll = next;
        } else if next >= self.start_menu_scroll.saturating_add(rows) {
            self.start_menu_scroll = next.saturating_add(1).saturating_sub(rows);
        }
    }

    /// Enter in the start menu: start the row the keyboard is on, or the best
    /// match for what was typed -- or, when nothing listed matches, run what
    /// was typed as the Run box would, since the field is for "finding *and
    /// running*" (`design.txt`).
    fn start_menu_enter(&mut self) -> HotkeyOutcome {
        let query = self.start_query.text().trim().to_string();
        let row = self
            .start_selected
            .or_else(|| (!query.is_empty()).then_some(0));
        let chosen = row.and_then(|row| {
            self.start_menu_entries()
                .get(row)
                .map(|entry| entry.executable_path.clone())
        });
        if let Some(exec) = chosen {
            self.close_start_menu();
            return HotkeyOutcome::start(vec![hotkeys::Launch::program(PathBuf::from(exec))]);
        }
        if query.is_empty() {
            return HotkeyOutcome::consumed();
        }
        // A quote left open is not something to guess the end of; the menu
        // stays up with the line as it was, to be finished.
        let Ok(words) = run_dialog::split_words(&query) else {
            return HotkeyOutcome::consumed();
        };
        let request = run_dialog::RunRequest {
            whole: std::ffi::OsString::from(&query),
            words: words.into_iter().map(std::ffi::OsString::from).collect(),
        };
        self.close_start_menu();
        match self.run_request(request) {
            Some(launch) => HotkeyOutcome::start(vec![launch]),
            None => HotkeyOutcome::consumed(),
        }
    }

    /// What the user has bound this press to, if anything.
    ///
    /// Cloned out of the registry rather than borrowed, because carrying the
    /// action out takes `&mut self` and two of them own a `String`. One clone
    /// per *recognised* shortcut is not on any path that runs per frame.
    fn bound_action(&self, key: &KeyEvent) -> Option<HotkeyAction> {
        self.hotkeys.lookup(key.key, &key.modifiers).cloned()
    }

    /// One press while the overview is up.
    fn key_on_overview(&mut self, key: &KeyEvent) -> HotkeyOutcome {
        // The one shortcut that still reaches the table: the chord that opened
        // the overview closes it. Without this the binding would be one-way —
        // Super+Tab would open the overlay and then, arriving as a bare Tab,
        // cycle its mode — and a toggle you cannot press twice is a trap.
        if self.bound_action(key) == Some(HotkeyAction::ToggleOverview) {
            return self.run_desktop_action(&HotkeyAction::ToggleOverview);
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
        // Controls are dropped character by character rather than rejecting the
        // whole run: they arrive mixed in with real text only in the odd cases
        // — a composition that failed on a control-producing key — and typing
        // nothing at all there would lose the part the user meant.
        let typed: String = key.typed().collect();
        (!typed.is_empty()).then_some(overview::OverviewKey::Text(typed))
    }

    /// Flip night light, and leave the file and the compositor agreeing.
    ///
    /// Load, modify, save -- the shape
    /// [`persist_input_layout`](Self::persist_input_layout) uses, and for the
    /// same reason: the file is the authority and this process holds no
    /// unsaved edits of it, so re-reading first means a setting the Settings
    /// application changed a moment ago is not overwritten by a stale copy.
    ///
    /// Writing a settings file from here is safe because a person clicked a
    /// switch. The rule and the counter-example are in
    /// `TD-C-THE-NOTIFICATIONS-PAGE-HAS-NO-PROGRAMS-TO-LIST`.
    ///
    /// A failed write is reported and the session still gets the new state:
    /// refusing to warm the screen because a disk is full helps nobody, and
    /// the switch can be flipped again.
    fn toggle_night_light(&mut self) {
        let mut file = appearance::AppearanceFile::load();
        file.settings.night_light = !file.settings.night_light;
        self.appearance.night_light = file.settings.night_light;
        if let Err(err) = file.save() {
            eprintln!("desktop: could not save appearance.yaml: {err}");
        }
        // The compositor warms the frame, and it reads the file rather than
        // being handed a value. Saying "go and read it again" is the session's
        // job because only it holds the connection.
        self.appearance_dirty = true;
    }

    /// Draw the desktop icons at `size` -- the View submenu's size items.
    /// Answers whether anything changed.
    ///
    /// Written to `appearance.yaml`, not kept here: the icon size is an
    /// appearance setting the Settings application edits too, and the file
    /// is the one place both read. Load, modify, save, for the reason
    /// [`toggle_night_light`](Self::toggle_night_light) gives, and a failed
    /// write is reported and the desktop still changes size, for the reason it
    /// gives too.
    ///
    /// The compositor is not told: it reads nothing about desktop icons, which
    /// this shell draws itself, so a "read the file again" would be a reload
    /// that changes nothing. The shell's own watcher will see the write and
    /// hand back the size it already has.
    fn choose_icon_size(&mut self, size: appearance::IconSize) -> bool {
        if self.appearance.icon_size == size {
            return false;
        }
        let mut file = appearance::AppearanceFile::load();
        file.settings.icon_size = size;
        if let Err(err) = file.save() {
            eprintln!("desktop: could not save appearance.yaml: {err}");
        }
        self.appearance.icon_size = size;
        self.icons.set_icon_size(size.pixels());
        // The icons moved with their cells, onto a grid of a different pitch;
        // the layout file records both.
        self.icons_dirty = true;
        true
    }

    /// Whether the shell has rewritten `appearance.yaml` since this was last
    /// asked, clearing the flag.
    ///
    /// The session calls this after handling events and, if it is true, tells
    /// the compositor to re-read -- which is what makes a quick toggle warm
    /// the screen now rather than at the next login.
    pub fn take_appearance_change(&mut self) -> bool {
        core::mem::take(&mut self.appearance_dirty)
    }

    /// Write the newly-chosen keyboard layout to `input.yaml`.
    ///
    /// This is what makes the switch reach the *keys*. The shell decides which
    /// layout is active; the compositor decides what a scancode means, and it
    /// reads that from the settings file it already watches. Going through the
    /// file rather than inventing a protocol message has three things to
    /// recommend it: the mechanism exists and is tested, a layout chosen with
    /// the keyboard and one chosen in the Settings panel cannot disagree
    /// because they are the same value in the same place, and the choice
    /// survives a restart, which is what a user expects of a layout.
    ///
    /// A failure is swallowed deliberately, and is the one place in this file
    /// where that is right: the layout has already changed in the shell's own
    /// model and the indicator will show it, so a read-only configuration
    /// directory costs the user persistence, not the feature. Refusing the
    /// keystroke because a file could not be written would be worse.
    fn persist_input_layout(&mut self) {
        let Some(id) = self.input_methods.active_layout_id() else {
            return;
        };
        let mut file = inputsettings::InputFile::load();
        if file.settings.keyboard.layout == id {
            return;
        }
        file.settings.keyboard.layout = id.to_string();
        // Swallowed on purpose; see this method's doc comment.
        let _ = file.save();
    }

    /// Carry out a shortcut that has already been recognised.
    ///
    /// Every binding but [`DismissPopup`](HotkeyAction::DismissPopup) consumes
    /// the press; that one is bare Escape, and a key the shell claims
    /// unconditionally is a key no window can ever see. Closing a dialog is what
    /// Escape does far more often than closing the start menu.
    ///
    /// The arms divide into three kinds, and the division is the whole point of
    /// the return type. The start menu, the Alt-Tab switcher's *stepping*, and
    /// popup dismissal are the shell's own surfaces and are done here. Anything
    /// naming a window — close, minimise, maximise, tile, raise — is a
    /// [`WindowRequest`] handed back for the caller to send. This method used to
    /// do the second kind itself, against the shell's private copy of the window
    /// list, which on a live session the next
    /// [`apply_window_list`](DesktopShell::apply_window_list) discards: Alt+F4
    /// removed a taskbar button and left the window open. The third kind starts
    /// a program, and is handed back for the same reason in
    /// [`launches`](HotkeyOutcome::launches): the shell has no connection to the
    /// process server either.
    fn run_desktop_action(&mut self, action: &HotkeyAction) -> HotkeyOutcome {
        match action {
            HotkeyAction::SwitchInputLayout => {
                self.input_methods.next_layout();
                self.persist_input_layout();
                HotkeyOutcome::consumed()
            }
            HotkeyAction::CycleWindows => {
                if self.alt_tab_active {
                    self.next_alt_tab();
                } else {
                    self.start_alt_tab();
                }
                HotkeyOutcome::consumed()
            }
            HotkeyAction::CycleWindowsBackwards => {
                if !self.alt_tab_active {
                    self.start_alt_tab();
                }
                if self.alt_tab_active {
                    self.prev_alt_tab();
                }
                HotkeyOutcome::consumed()
            }
            HotkeyAction::CloseWindow => {
                HotkeyOutcome::ask(self.request_on_focused(ShellControlAction::Close))
            }
            HotkeyAction::MinimizeWindow => {
                HotkeyOutcome::ask(self.request_on_focused(ShellControlAction::Minimize))
            }
            HotkeyAction::ToggleStartMenu => {
                self.toggle_start_menu();
                HotkeyOutcome::consumed()
            }
            // The one shortcut that names more than one window, and the reason
            // `handle_hotkey` cannot return a single request.
            HotkeyAction::ShowDesktop => HotkeyOutcome::ask_all(
                self.windows
                    .values()
                    // `on_glass`, not `mapped`: this is the one caller that
                    // wants the narrower question. Asking a window that is
                    // already minimised to minimise again is a request the
                    // compositor would have to ignore, and one the user would
                    // have to un-do twice.
                    .filter(|w| w.on_glass() && w.desktop == self.current_desktop)
                    .map(|w| ShellRequest::window(w.id, ShellControlAction::Minimize))
                    .collect(),
            ),
            HotkeyAction::SnapLeft => {
                HotkeyOutcome::ask(self.request_on_focused(ShellControlAction::SnapLeft))
            }
            HotkeyAction::SnapRight => {
                HotkeyOutcome::ask(self.request_on_focused(ShellControlAction::SnapRight))
            }
            HotkeyAction::MaximizeWindow => {
                HotkeyOutcome::ask(self.request_on_focused(ShellControlAction::Maximize))
            }
            // Consumed unconditionally: Super+Tab is the shell's chord whether
            // or not there is anything to show, and a shortcut that sometimes
            // reaches the focused window is a shortcut that sometimes types a
            // Tab into it.
            HotkeyAction::ToggleOverview => {
                self.overview.toggle(overview::OverviewMode::AllDesktops);
                HotkeyOutcome::consumed()
            }
            // Consumed whether or not the overlay opened, for the same reason.
            // Super+Z is the shell's key in either case, and letting it through
            // to the focused window on an empty desktop would make a shortcut
            // that sometimes types a `z`.
            HotkeyAction::ToggleZoneOverlay => {
                self.toggle_zone_overlay();
                HotkeyOutcome::consumed()
            }
            // Consumed unconditionally, like the two above: Super+N is the
            // shell's chord whether or not there is anything in the pane, and a
            // shortcut that sometimes types an `n` into the focused window is
            // worse than one that sometimes opens an empty panel.
            HotkeyAction::ToggleNotifications => {
                self.toggle_notifications();
                HotkeyOutcome::consumed()
            }
            // Consumed unconditionally, for the same reason as the three above.
            // The card is also the one surface that can be *closed* by the
            // chord that lists it, which is why it must not fall through: a
            // Super+/ that reached the focused window while the card was up
            // would leave the user unable to shut what they opened.
            HotkeyAction::ToggleShortcutCard => {
                self.toggle_shortcut_card();
                HotkeyOutcome::consumed()
            }
            HotkeyAction::RestoreOrMinimize => {
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
            HotkeyAction::PreviousDesktop => {
                HotkeyOutcome::ask(self.previous_desktop().and_then(|d| self.switch_desktop(d)))
            }
            HotkeyAction::NextDesktop => {
                HotkeyOutcome::ask(self.next_desktop().and_then(|d| self.switch_desktop(d)))
            }
            // A number the user wrote in a config file, so it can name a desktop
            // that does not exist. `switch_desktop` answers `None` for one that
            // is already showing or out of range, and the press is consumed
            // either way: the chord is the shell's whether or not the desktop is
            // there, and letting it through would type into the focused window.
            HotkeyAction::SwitchDesktop(index) => {
                HotkeyOutcome::ask(self.switch_desktop(u32::from(*index)))
            }
            HotkeyAction::DismissPopup => {
                if self.dismiss_popups() {
                    HotkeyOutcome::consumed()
                } else {
                    HotkeyOutcome::ignored()
                }
            }
            // The overlay is shown from the level the pane reports back, not
            // from the level this arm asked for: `adjust_volume` clamps, so at
            // either end of the range the two differ, and an indicator that
            // read 105% would be reporting a keystroke rather than a volume.
            HotkeyAction::VolumeUp | HotkeyAction::VolumeDown => {
                let step = if matches!(action, HotkeyAction::VolumeUp) {
                    VolumeStep::Up
                } else {
                    VolumeStep::Down
                };
                let level = self.notifications.adjust_volume(step.delta());
                self.show_osd(osd::OsdKind::Volume {
                    level,
                    muted: self.notifications.is_muted(),
                });
                HotkeyOutcome::consumed()
            }
            HotkeyAction::VolumeMute => {
                let muted = self.notifications.toggle_mute();
                // The level as well as the flag, because muting does not change
                // the level and the overlay is what tells you what unmuting
                // will bring back.
                self.show_osd(osd::OsdKind::Volume {
                    level: self.notifications.volume(),
                    muted,
                });
                HotkeyOutcome::consumed()
            }
            // Consumed unconditionally, like the three toggles above. Super+R is
            // the shell's chord in either direction, and a second press that
            // reached the focused window would type an `r` into the program the
            // *first* press was used to start.
            HotkeyAction::ToggleRunDialog => {
                self.toggle_run_dialog();
                HotkeyOutcome::consumed()
            }
            // The six that start a program instead of touching a window. The
            // command is the action's own — see [`HotkeyAction::command`] — and
            // it is reported rather than run, because the shell has no
            // connection to the process server.
            HotkeyAction::LaunchApp(_)
            | HotkeyAction::ShowTaskManager
            | HotkeyAction::SystemSettings
            | HotkeyAction::ScreenLock
            | HotkeyAction::Screenshot
            | HotkeyAction::ScreenshotRegion => {
                HotkeyOutcome::start(action.launch().into_iter().collect())
            }
            // Nothing can carry these out: there is no backlight channel out of
            // the shell, and inventing one would mean a request the compositor
            // has no verb for. Consumed regardless — the user bound the chord,
            // so passing it to the focused window would be worse than doing
            // nothing visibly. See `known-issues.md` →
            // `TD-C-BRIGHTNESS-KEYS-ARE-NOT-KEYS`.
            HotkeyAction::BrightnessUp | HotkeyAction::BrightnessDown => HotkeyOutcome::consumed(),
        }
    }

    /// Open the Run box, or close it if it is already open.
    ///
    /// Centred on each opening rather than once at construction: the display can
    /// change size under a shell that is already running
    /// ([`ShellSession::resize_display`](session::ShellSession)), and a position
    /// computed at startup would put the box off the edge of the new screen. The
    /// same pull-on-use reasoning as `sync_osd_screen`,
    /// for the same reason: `screen_width` and `screen_height` are public fields
    /// that anything may assign.
    pub fn toggle_run_dialog(&mut self) {
        if self.run_dialog.is_visible() {
            self.run_dialog.hide();
            return;
        }
        // The other popups close, because the Run box is modal about keys: with
        // the start menu still open behind it, the menu would be showing a
        // search field that can no longer be typed into.
        self.dismiss_popups();
        self.run_dialog.show();
        self.centre_run_dialog();
    }

    /// Put the Run box in the middle of the display.
    #[allow(
        clippy::cast_precision_loss,
        reason = "screen dimensions are far inside f32's exact-integer range"
    )]
    fn centre_run_dialog(&mut self) {
        self.run_dialog
            .centre_on(self.screen_width as f32, self.screen_height as f32);
    }

    /// One press while the Run box is up.
    ///
    /// Modal on the same terms as [`key_on_overview`](Self::key_on_overview),
    /// and for the same reason — the box has a text field, so a press that
    /// reached the binding table would run a shortcut instead of typing a
    /// character. The chord that opened it still reaches the table, so that
    /// Super+R closes what Super+R opened.
    fn key_on_run_dialog(&mut self, key: &KeyEvent) -> HotkeyOutcome {
        if self.bound_action(key) == Some(HotkeyAction::ToggleRunDialog) {
            return self.run_desktop_action(&HotkeyAction::ToggleRunDialog);
        }
        // Result deliberately discarded, exactly as the notification pane's is:
        // a press the dialog had no meaning for is still not the desktop's while
        // the dialog is up.
        let _ = self.run_dialog.handle_key_event(key);
        // Each request opened or run by the one rule the OK button uses. This
        // comment used to explain why the box took no arguments -- a quoting
        // rule invented silently "would make `my program` two words to the
        // shell and one to the filesystem". The rule is now stated
        // (`run_dialog::split_words`, design-decisions.md §870), and the
        // whole line is tried as a path before anything is split, which is
        // what keeps `my program` one thing when it names one.
        let requests = self.drain_run_dialog();
        HotkeyOutcome::start(
            requests
                .into_iter()
                .filter_map(|request| self.run_request(request))
                .collect(),
        )
    }

    /// Carry out one thing the Run box was asked for, the way Windows' Run
    /// box does -- which `design.txt` asks this one to be like:
    ///
    /// - if the **whole line is an absolute path that exists**, open it by
    ///   the rules a double-click on the desktop uses
    ///   ([`open_path`](Self::open_path)): a folder in the file manager, a
    ///   document in its program, a program run. Tried first, so a path with
    ///   a space in it needs no quotes when it is the whole line;
    /// - otherwise **run the first word with the rest as its arguments**,
    ///   split by [`run_dialog::split_words`].
    ///
    /// `None` when nothing is to start: a path whose kind nothing opens (and
    /// which has said so in a notification), or a line with no words.
    fn run_request(&mut self, request: run_dialog::RunRequest) -> Option<hotkeys::Launch> {
        let whole = Path::new(&request.whole);
        if whole.is_absolute() && std::fs::metadata(whole).is_ok() {
            let label = whole.display().to_string();
            return match self.open_path(whole, &label) {
                ShellAction::Launch(launch) => Some(launch),
                _ => None,
            };
        }
        let mut words = request.words.into_iter();
        let program = words.next()?;
        Some(hotkeys::Launch {
            program: PathBuf::from(program),
            args: words.collect(),
        })
    }

    /// Answer whatever the Run box has asked for since it was last emptied, and
    /// hand back what it was asked to run or open (see
    /// [`run_request`](Self::run_request)).
    ///
    /// `Cancel` and `Closed` need no answer — the dialog has already hidden
    /// itself by the time it reports them — but they must still be drained, or
    /// the buffer grows by one on every dismissal. That is the whole reason this
    /// is called on *every* press rather than only on the ones that could have
    /// executed something.
    ///
    /// [`Browse`](run_dialog::RunDialogEvent::Browse) puts the file chooser up
    /// — see [`run_browser`](Self::run_browser). It asks for a *picker*, whose
    /// answer goes back into the command box; starting the file explorer
    /// instead would be the tempting substitute and is the wrong one, because
    /// the user would get a window they did not ask for and an empty command
    /// box.
    ///
    /// A `for` loop rather than the `filter_map` this used to be, because
    /// opening the chooser needs `&mut self` and a closure passed to `filter_map`
    /// would be holding a borrow of it. The drained `Vec` is owned, so the loop
    /// borrows nothing.
    fn drain_run_dialog(&mut self) -> Vec<run_dialog::RunRequest> {
        let mut launches = Vec::new();
        for event in self.run_dialog.drain_events() {
            match event {
                run_dialog::RunDialogEvent::Execute(request) => launches.push(request),
                run_dialog::RunDialogEvent::Browse => self.open_run_browser(),
                // No answer needed — the dialog has already hidden itself by the
                // time it reports these — but they must still be drained, or the
                // buffer grows by one on every dismissal. That is the whole
                // reason this is called on *every* press rather than only on the
                // ones that could have executed something.
                run_dialog::RunDialogEvent::Cancel | run_dialog::RunDialogEvent::Closed => {}
            }
        }
        launches
    }

    /// Put up the file chooser the Run box's Browse button asks for.
    ///
    /// Opened where the box's current text points, which is what makes a second
    /// Browse a correction of the first rather than a fresh start from the root
    /// — see [`RunDialog::browse_start`](run_dialog::RunDialog::browse_start).
    ///
    /// The chooser arrives with no entries in it. It cannot arrive with any:
    /// listing a directory is a filesystem read and this module performs none.
    /// The first [`run_browser_wants`](Self::run_browser_wants) after this
    /// reports the directory, and the host answers. A host that never answers
    /// gets an empty chooser rather than a wrong one, which is the right way
    /// round for a shell whose tests all run with no filesystem at all.
    fn open_run_browser(&mut self) {
        let start = self.run_dialog.browse_start();
        self.run_browser = Some(guitk::dialog::FileDialog::open().with_initial_path(&start));
        self.run_browser_listed = None;
    }

    /// Take the chooser down, whether it was cancelled or answered.
    fn close_run_browser(&mut self) {
        self.run_browser = None;
        self.run_browser_listed = None;
    }

    /// Whether the Run box's file chooser is on screen.
    #[must_use]
    pub fn run_browser_open(&self) -> bool {
        self.run_browser.is_some()
    }

    /// The directory the chooser is showing and has not been given a listing
    /// for, if there is one.
    ///
    /// The read half of the split described on
    /// `run_browser_listed`: a host that draws this
    /// shell should call this before each paint and answer any `Some` with
    /// [`set_run_browser_entries`](Self::set_run_browser_entries), the way it
    /// already answers [`WallpaperManager::current_image_path`] with pixels.
    ///
    /// [`WallpaperManager::current_image_path`]: wallpaper::WallpaperManager::current_image_path
    #[must_use]
    pub fn run_browser_wants(&self) -> Option<&Path> {
        let path = self.run_browser.as_ref()?.current_path();
        (self.run_browser_listed.as_deref() != Some(path)).then_some(path)
    }

    /// Answer [`run_browser_wants`](Self::run_browser_wants) with a listing.
    ///
    /// The entries are recorded as belonging to whatever directory the chooser
    /// is showing *now*, not to whatever it was showing when they were read.
    /// The two are the same in every real sequence — the host lists and answers
    /// between events — and if they ever were not, the mismatch resolves itself
    /// on the next call rather than leaving the chooser permanently showing one
    /// directory's contents under another's name.
    ///
    /// Silently ignored when no chooser is up. A listing that arrives after the
    /// user cancelled is not an error; it is a read that was already in flight.
    pub fn set_run_browser_entries(&mut self, entries: Vec<guitk::dialog::DirEntry>) {
        let Some(dialog) = self.run_browser.as_mut() else {
            return;
        };
        self.run_browser_listed = Some(dialog.current_path().to_path_buf());
        dialog.set_entries(entries);
    }

    /// Where the chooser is drawn, as `(x, y, width, height)`.
    ///
    /// Computed on demand from the screen size rather than stored, for the
    /// reason [`toggle_run_dialog`](Self::toggle_run_dialog) gives: the display
    /// can change size under a running shell, and a position worked out when
    /// the chooser opened would put it off the edge of the new screen. It is
    /// also what keeps the drawing and the hit-testing in step — both call
    /// this, so neither can be laying the chooser out at a size the other is
    /// not.
    #[allow(
        clippy::cast_precision_loss,
        reason = "screen dimensions are far inside f32's exact-integer range"
    )]
    fn run_browser_rect(&self) -> (f32, f32, f32, f32) {
        let screen_w = self.screen_width as f32;
        let screen_h = self.screen_height as f32;
        // Clamped down to the screen, then the origin clamped up to zero: a
        // chooser wider than the display would otherwise be centred by placing
        // its left edge off the left side, where the sidebar and the `^` button
        // are the half that gets cut.
        let width = RUN_BROWSER_WIDTH.min(screen_w);
        let height = RUN_BROWSER_HEIGHT.min(screen_h);
        let x = ((screen_w - width) / 2.0).max(0.0);
        let y = ((screen_h - height) / 2.0).max(0.0);
        (x, y, width, height)
    }

    /// One press while the chooser is up.
    fn key_on_run_browser(&mut self, key: &KeyEvent) -> HotkeyOutcome {
        let (_, _, _, height) = self.run_browser_rect();
        let action = match self.run_browser.as_mut() {
            Some(dialog) => dialog.handle_event(key, height),
            None => return HotkeyOutcome::default(),
        };
        self.apply_run_browser_action(action);
        // Consumed unconditionally. The chooser is modal, so a press it had no
        // meaning for is still not the desktop's — and certainly not the Run
        // box's, which is directly underneath and would otherwise be typed into
        // through the chooser covering it.
        HotkeyOutcome::consumed()
    }

    /// What the chooser did in answer to an event.
    fn apply_run_browser_action(&mut self, action: guitk::dialog::DialogAction) {
        match action {
            // A navigation needs nothing done here: `run_browser_wants`
            // compares the chooser's directory against the last one delivered,
            // so the new directory is already reported as wanting a listing.
            guitk::dialog::DialogAction::None | guitk::dialog::DialogAction::NavigatedTo(_) => {}
            // The whole point of the round trip. The path goes into the command
            // field as bytes, not as text, so a program whose name has no UTF-8
            // spelling is the program that starts — see
            // `RunDialog::set_command_path`.
            guitk::dialog::DialogAction::Selected(path) => {
                self.run_dialog.set_command_path(&path);
                self.close_run_browser();
            }
            // The box is left exactly as it was, text and all. A Browse that
            // cleared what the user had typed is a Browse nobody uses twice.
            guitk::dialog::DialogAction::Cancelled => self.close_run_browser(),
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

/// Which way a volume key moves the level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VolumeStep {
    Up,
    Down,
}

impl VolumeStep {
    /// How far one press moves the volume, in percentage points.
    ///
    /// Five, so twenty presses cross the whole range: a step small enough to
    /// aim with and large enough that turning the volume down from a startle is
    /// a few presses rather than a drum roll. The same number every desktop
    /// picks, for the same reason.
    const SIZE: i16 = 5;

    /// The signed step this direction applies.
    const fn delta(self) -> i16 {
        match self {
            Self::Up => Self::SIZE,
            Self::Down => -Self::SIZE,
        }
    }
}

impl DesktopShell {
    /// The chords the shell needs delivered wherever the keyboard is.
    ///
    /// Derived from [`hotkeys`](Self::hotkeys) — every binding in the registry
    /// except the [conditional](hotkeys::HotkeyAction::is_conditional) ones,
    /// expanded to the exact chords a grab can name. This used to be a `const`
    /// list written out beside the binding table and kept in step with it by two
    /// tests, which is an arrangement that only works while the table is fixed at
    /// compile time; the moment a user can rebind a shortcut, the grab set has
    /// to be computed from what they bound.
    ///
    /// Built rather than returned by reference because of that expansion. It is
    /// read at startup and again whenever the bindings change, neither of which
    /// is a path that runs per frame.
    #[must_use]
    pub fn global_chords(&self) -> Vec<(Key, Modifiers)> {
        self.hotkeys.global_chords()
    }

    /// The modifier-only chords the shell wants held, from the user's choice
    /// of keyboard-layout switcher.
    ///
    /// A list rather than an `Option` because the session reconciles it as a
    /// set, exactly as it does [`global_chords`](Self::global_chords), and a
    /// second chord — a future "cycle input method" — costs nothing to add.
    ///
    /// Empty when the user's choice is Super+Space, which is an ordinary key
    /// chord and is already in the hotkey registry.
    #[must_use]
    pub fn modifier_chords(&self) -> Vec<Modifiers> {
        self.input_methods
            .switch_shortcut
            .as_modifier_chord()
            .into_iter()
            .collect()
    }

    /// The chords to hold only while [`any_popup_open`](Self::any_popup_open).
    ///
    /// Bare Escape, out of the box. A grab is not conditional, so this set is
    /// grabbed and released as the shell's surfaces open and close instead of
    /// being held for the session — holding Escape permanently would break it in
    /// every dialog on the desktop.
    #[must_use]
    pub fn conditional_chords(&self) -> Vec<(Key, Modifiers)> {
        self.hotkeys.conditional_chords()
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
        // Over *slots*, not windows: a pinned application has a button whether
        // or not it is running, and it stands to the left of the windows.
        let windows = self.taskbar_windows();
        for (index, slot) in self.taskbar_slots().iter().enumerate() {
            let button = self.taskbar_button_rect(index);

            let (label, bg) = match *slot {
                TaskbarSlot::Pinned(pin) => {
                    let Some(app) = self.taskbar.pinned_apps().get(pin) else {
                        continue;
                    };
                    // Never the focused colour: a pinned button is a way to
                    // *start* the program, so drawing it as though it were the
                    // window in front would say something untrue about it.
                    (app.display_name.as_str(), self.theme.taskbar_bg)
                }
                TaskbarSlot::Window(id) => {
                    let Some(window) = windows.iter().find(|w| w.id == id) else {
                        continue;
                    };
                    let bg = if Some(id) == self.focused_window {
                        self.theme.taskbar_active_bg
                    } else {
                        self.theme.taskbar_bg
                    };
                    (window.title.as_str(), bg)
                }
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
                label,
                self.theme.taskbar_fg,
                title_size,
            );
        }

        // The divider between the pinned buttons and the windows'.
        if let Some(divider) = self.taskbar_divider_rect() {
            fill(
                &mut tree,
                divider,
                with_alpha(self.theme.taskbar_fg, TASKBAR_DIVIDER_ALPHA),
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

        // The notification bell, in its own slot immediately left of the clock.
        // Without it the pane this shell owns had exactly one way in — Super+N
        // — which is a route nobody discovers, and the pane's own module doc
        // has always claimed a "system tray click" as the other.
        //
        // The glyph is the focus mode's own — a plain bell when nothing is
        // being silenced, a struck bell / alarm clock / no-entry sign when
        // something is — so one slot carries both "here is your history" and
        // "here is why it has been quiet", without the tray growing a second
        // item and shuffling every window button sideways.
        // The application icons, left of everything the shell draws itself.
        //
        // Drawn in the bar's own ink rather than a colour the program chose:
        // 842 put the tray in the shell, and the tray is shell chrome. A
        // program picking its own colour would be choosing against a palette
        // it cannot see -- the defect `Palette::ink` exists to prevent -- and
        // on a taskbar the one pair this theme guarantees legible is the bar's
        // foreground on the bar.
        // The chevron first, at the left of the run, so that a reader of this
        // function meets the strip in the order it is drawn.
        if let Some(rect) = self.tray_overflow_rect() {
            tree.text(
                rect.x,
                tray_text_y,
                TRAY_OVERFLOW_GLYPH,
                self.theme.taskbar_fg,
                self.font_size(TextRole::Glyph),
            );
        }
        for (rect, icon) in self.tray_icon_rects().iter().zip(self.ordered_tray_icons()) {
            // The icon being dragged is drawn faint.
            //
            // `DragSource` has maintained `show_ghost` and `dragging_key`
            // since it was written -- set once the drag threshold is crossed,
            // cleared on drop or on Escape -- and nothing read either, so the
            // whole state machine was invisible. The icon sat at full opacity
            // exactly where it started while the insertion point moved under
            // the pointer, which reads as "the drag did not take".
            let color = if self.tray_icon_is_being_dragged(icon) {
                with_alpha(self.theme.taskbar_fg, GHOST_ALPHA)
            } else {
                self.theme.taskbar_fg
            };
            tree.text(
                rect.x,
                tray_text_y,
                &icon.glyph,
                color,
                self.font_size(TextRole::Glyph),
            );
        }

        let bell = self.bell_rect();
        let unread = self.notifications.attention_count();
        tree.text(
            bell.x,
            tray_text_y,
            self.focus.effective_mode().icon(),
            // Accent when something is waiting. The glyph alone would be a
            // silent difference: a bell that looks the same whether or not it
            // has anything behind it is a bell nobody presses.
            //
            // The mode is deliberately *not* tinted with `focus_assist`'s
            // severity hues here. Those are measured against that module's
            // pill fill; on the taskbar the only pair this theme guarantees
            // legible is accent-on-background (pinned by
            // `the_taskbar_accent_contrasts_with_the_bar`), and the changed
            // glyph already carries the mode.
            if unread == 0 {
                self.theme.taskbar_fg
            } else {
                self.theme.taskbar_accent
            },
            self.font_size(TextRole::Glyph),
        );
        if unread > 0 {
            // Two characters at most, so a hundred notifications cannot widen
            // the badge past its slot. The pill is filled with the accent and
            // lettered in the bar's own colour, which is the one pair the theme
            // guarantees contrasts — see `DesktopTheme::taskbar_accent`.
            let label = if unread > 9 {
                "9+".to_owned()
            } else {
                unread.to_string()
            };
            let badge_size = self.font_size(TextRole::Caption);
            let badge_w = text::width(&label, badge_size) + self.scale(6.0);
            let badge_h = badge_size + self.scale(2.0);
            // Right-aligned inside the slot: a badge that grew leftwards from
            // the glyph would push the clock, and one that grew rightwards
            // would sit on top of it.
            let badge_x = (bell.x + bell.w - badge_w).max(bell.x);
            let badge_y = bar.y + self.scale(4.0);
            fill_round(
                &mut tree,
                Rect::new(badge_x, badge_y, badge_w, badge_h),
                self.theme.taskbar_accent,
                CornerRadii::all(badge_h / 2.0),
            );
            tree.text(
                badge_x + self.scale(3.0),
                badge_y + self.scale(1.0),
                &label,
                self.theme.taskbar_bg,
                badge_size,
            );
        }

        // Desktop indicator, at the tray's left edge.
        tree.text(
            tray_x + padding,
            tray_text_y,
            &self.desktop_indicator_string(),
            self.theme.taskbar_fg,
            self.font_size(TextRole::Caption),
        );

        // The keyboard layout, immediately right of it. Without this, Super+Space
        // changes what every key on the keyboard produces and nothing on screen
        // says so -- which is worse than not having the shortcut.
        let layout_w = self.layout_indicator_width();
        if layout_w > 0.0 {
            tree.text(
                tray_x + padding + self.desktop_indicator_width() + self.scale(TRAY_PADDING),
                tray_text_y,
                self.input_methods.tray_label(),
                self.theme.taskbar_fg,
                self.font_size(TextRole::Caption),
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
        // The same list `alt_tab_index` counts into — see
        // [`switcher_windows`](Self::switcher_windows). Drawing the taskbar's
        // list instead would highlight the wrong entry the moment any window
        // was in one list and not the other.
        let windows = self.switcher_windows();

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

        // The search field, where the title was: the menu is a list of
        // programs either way, and the field says how to find one in it.
        self.render_start_search(&mut tree);

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
            // The keyboard's row, marked as the accent marks "you are here"
            // everywhere else in the shell.
            if self.start_selected == Some(index) {
                fill_round(
                    &mut tree,
                    Rect::new(
                        rect.x + self.scale(6.0),
                        rect.y + self.scale(2.0),
                        (rect.w - self.scale(12.0)).max(0.0),
                        (rect.h - self.scale(4.0)).max(0.0),
                    ),
                    with_alpha(self.theme.accent_color, START_MENU_SELECTED_ALPHA),
                    CornerRadii::all(self.scale(4.0)),
                );
            }
            tree.text(
                rect.x + self.scale(24.0),
                rect.y + self.scale(8.0),
                &entry.name,
                self.theme.start_menu_fg,
                self.font_size(TextRole::Item),
            );
            // A line along the top of the first row after the pins, so the
            // user's own choices read as a group apart from the launcher's
            // list -- which may name the same programs again below.
            if index == self.start_pins_listed() && index > 0 {
                let inset = self.scale(16.0);
                tree.push(guitk::render::RenderCommand::FillRect {
                    x: rect.x + inset,
                    y: rect.y,
                    width: (rect.w - inset * 2.0).max(0.0),
                    height: self.scale(1.0).max(1.0),
                    color: self.theme.panel_border_color,
                    corner_radii: CornerRadii::all(0.0),
                });
            }
        }

        // A scroll indicator, so a list that continues past the last row says
        // so. Sized and placed in proportion to the part of the list on screen.
        let total = entries.len();
        if total > rows && rows > 0 {
            let row_h = self.scale(START_MENU_ROW_HEIGHT);
            let bar_w = self.scale(START_MENU_SCROLLBAR_WIDTH);
            let track_top = self.start_menu_row_rect(0).y;
            let track_h = rows as f32 * row_h;
            let max_scroll = self.start_menu_max_scroll().max(1) as f32;
            // `guitk::scrollbar`'s arithmetic, shared with the file dialog,
            // the menus and `apps/dictionary`. The floor stays half a row --
            // this bar is sized in rows of a start menu, not pixels of a
            // dialog -- which is why the module takes it as an argument.
            let thumb = guitk::scrollbar::thumb_of(
                guitk::frame::Rect::new(
                    menu.x + menu.w - bar_w - self.scale(2.0),
                    track_top,
                    bar_w,
                    track_h,
                ),
                rows as f32 / total as f32,
                self.start_menu_scroll as f32 / max_scroll,
                row_h / 2.0,
            );
            fill_round(
                &mut tree,
                Rect::new(thumb.x, thumb.y, thumb.w, thumb.h),
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

        // Settings and Terminal beside it, in the same words-on-the-footer
        // style, fitted to their buttons.
        for which in StartShortcut::ALL {
            let button = self.start_shortcut_rect(*which);
            let inset = self.scale(6.0);
            tree.text_in(
                button.x + inset,
                button.y + (button.h - label_size).max(0.0) / 2.0,
                (button.w - inset * 2.0).max(0.0),
                which.label(),
                self.theme.start_menu_fg,
                label_size,
            );
        }

        if self.power_menu_open {
            self.render_power_menu(&mut tree);
        }

        Some(tree)
    }

    /// Draw the start menu's search field: what has been typed, with a caret,
    /// or a hint saying what typing does -- and, when the search finds
    /// nothing, what Enter will do instead.
    fn render_start_search(&self, tree: &mut RenderTree) {
        let field = self.start_search_rect();
        let size = self.font_size(TextRole::Body);
        let radii = CornerRadii::all(self.scale(4.0));
        stroke_round(tree, field, self.theme.accent_color, self.scale(1.0), radii);
        let inset = self.scale(8.0);
        let line = text::line_height(size, guitk::render::FontWeightHint::Regular);
        let y = field.y + ((field.h - line) / 2.0).max(0.0);
        let query = self.start_query.text();
        textedit::draw(
            tree,
            &SingleLine {
                text: query,
                cursor: self.start_query.cursor(),
                selection_anchor: self.start_query.selection_anchor(),
                focused: true,
                x: field.x + inset,
                y,
                width: (field.w - inset * 2.0).max(1.0),
                line_height: line,
                font_size: size,
                weight: guitk::render::FontWeightHint::Regular,
                color: self.theme.start_menu_fg,
                selection_bg: self.theme.accent_color,
                selection_fg: self.theme.start_menu_bg,
                caret_width: self.appearance.caret_width(),
            },
        );
        if query.is_empty() {
            tree.text_in(
                field.x + inset + self.scale(4.0),
                y,
                (field.w - inset * 2.0).max(0.0),
                "Type to search",
                with_alpha(self.theme.start_menu_fg, START_MENU_HINT_ALPHA),
                size,
            );
        } else if self.start_menu_entries().is_empty() {
            let row = self.start_menu_row_rect(0);
            tree.text_in(
                row.x + self.scale(24.0),
                row.y + self.scale(8.0),
                (row.w - self.scale(36.0)).max(0.0),
                "Nothing found. Press Enter to run it.",
                with_alpha(self.theme.start_menu_fg, START_MENU_HINT_ALPHA),
                self.font_size(TextRole::Item),
            );
        }
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

    /// The zone the taskbar clock reads in: the one the user chose, or the
    /// machine's own.
    ///
    /// The machine's also when the chosen one is not in the table -- a file
    /// edited by hand -- which is honest rather than convenient: a zone we
    /// cannot resolve is not a licence to invent an offset, and the machine's
    /// zone is the one `date` would show.
    fn local_zone(&self) -> Tz {
        self.datetime.rule(self.system_zone)
    }

    /// Read how the user wants the time told (`datetime.yaml`) and the zone
    /// the machine is in.
    ///
    /// Both at once, because the second is what the first falls back to. Not
    /// in [`new`](Self::new), for the reason on
    /// [`system_zone`](Self::system_zone); the session calls this when it
    /// starts. See `design-decisions.md` §875.
    pub fn load_datetime(&mut self) {
        self.datetime = datetimesettings::DateTimeFile::load().settings;
        self.system_zone = datetimesettings::system_zone();
    }

    /// Adopt the machine's zone as `zone` -- what [`load_datetime`] reads,
    /// for a test that needs a zone of its own choosing rather than the
    /// host's.
    ///
    /// [`load_datetime`]: Self::load_datetime
    pub fn set_system_zone(&mut self, zone: Tz) {
        self.system_zone = zone;
    }

    /// How far into the local day a UTC instant is, in seconds.
    ///
    /// For anything that changes with the *time of day* rather than with the
    /// clock reading — [`wallpaper::WallpaperMode::Dynamic`], which fades a
    /// palette from dawn to night, is the one caller today.
    ///
    /// It exists rather than letting each such caller write `utc_secs % 86_400`
    /// because that expression is the exact bug
    /// `current_clock_string` documents: it is
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

    /// How wide the keyboard-layout indicator is, or zero when there is only
    /// one layout installed.
    ///
    /// Zero rather than a fixed reserve: a machine with one layout has nothing
    /// to switch between, and an indicator that always read "US" would be a
    /// permanent label for a control that does nothing. The tray's width is
    /// derived from its contents (see `tray_width`), so returning zero removes
    /// the space as well as the text.
    fn layout_indicator_width(&self) -> f32 {
        if self.input_methods.layouts.len() < 2 {
            return 0.0;
        }
        text::width(
            self.input_methods.tray_label(),
            self.font_size(TextRole::Caption),
        ) + self.scale(TRAY_PADDING)
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
        let content = self.clock_width()
            + self.scale(TRAY_BELL_WIDTH)
            + self.desktop_indicator_width()
            + self.layout_indicator_width()
            // The application icons are part of the tray's width, or the
            // window buttons would be laid out into space the icons occupy and
            // the rightmost button would sit under them.
            + self.app_tray_width();
        // Padding at the right edge, between each pair of items, and at the
        // left of the tray.
        (content + padding * 4.0).max(self.scale(TRAY_MIN_WIDTH))
    }

    /// The icons other programs have put in the tray, as the compositor
    /// reported them.
    ///
    /// **This is not the order they are drawn in** -- see
    /// [`ordered_tray_icons`](Self::ordered_tray_icons). This is the raw
    /// membership, which is what a caller asking "is there an icon for X"
    /// wants; anything to do with position must use the ordered form.
    #[must_use]
    pub fn tray_icons(&self) -> &[guiremote::tray::TrayIcon] {
        &self.tray_icons
    }

    /// The tray's icons in the order the shell shows them.
    ///
    /// Joined on demand rather than stored joined. The compositor's list and
    /// the arrangement change independently -- a program relabelling its icon
    /// rewrites the first and must not touch the second -- and a joined copy
    /// would be a third record of the same fact, which is how this tree ended
    /// up with four models of a tray icon in the first place.
    ///
    /// Every key in the arrangement has an icon in the list, because
    /// [`sync`](tray_dnd::TrayIconArrangement::sync) is fed the very list this
    /// reads.
    #[must_use]
    pub fn ordered_tray_icons(&self) -> Vec<&guiremote::tray::TrayIcon> {
        self.join_tray(
            &self
                .tray_arrangement
                .visible_icons(self.tray_icon_capacity())
                .iter()
                .map(|slot| slot.key)
                .collect::<Vec<_>>(),
        )
    }

    /// Resolve keys to the compositor's records of them, dropping any that
    /// have since departed.
    fn join_tray(&self, keys: &[tray_dnd::TrayIconKey]) -> Vec<&guiremote::tray::TrayIcon> {
        keys.iter()
            .filter_map(|wanted| {
                self.tray_icons
                    .iter()
                    .find(|icon| tray_dnd::TrayIconKey::of(icon) == *wanted)
            })
            .collect()
    }

    /// Adopt a tray list from the compositor.
    ///
    /// Answers whether anything changed, so the session can repaint only when
    /// it must. The compositor already sends a frame only when the list it
    /// would send differs, so this is a second net rather than the first --
    /// kept because `apply_window_list` has one for the same reason, and
    /// because a shell that repainted on every frame it received would repaint
    /// on reconnection for a list identical to the one it already had.
    pub fn apply_tray_icons(&mut self, icons: Vec<guiremote::tray::TrayIcon>) -> bool {
        if self.tray_icons == icons {
            return false;
        }
        // Fold before storing, so the arrangement sees both lists and can tell
        // a program that relabelled its icon from one that just registered.
        // The answer is discarded: reaching here already means the membership
        // or a glyph changed, so the tray repaints either way.
        self.tray_arrangement.sync(&icons);
        self.tray_icons = icons;
        true
    }

    /// Open the list of icons the bar had no room for.
    ///
    /// A menu rather than a second strip of icons. The strip is what ran out
    /// of room in the first place, and a popup one can also read: the entries
    /// carry the program's tooltip as their label, which is the only place in
    /// this shell a tray icon's name is ever shown -- a 24-pixel glyph on a
    /// bar has nowhere to put one.
    fn open_tray_overflow(&mut self) {
        // Opening a popup closes the others, as every other popup here does.
        self.close_start_menu();
        self.shortcut_card_open = false;
        self.calendar.set_visible(false);
        self.notifications.hide();

        let hidden = self.overflowed_tray_icons();
        if hidden.is_empty() {
            return;
        }
        let mut keys = Vec::with_capacity(hidden.len());
        let mut items = Vec::with_capacity(hidden.len());
        for (index, icon) in hidden.iter().enumerate() {
            keys.push(tray_dnd::TrayIconKey::of(icon));
            items.push(guitk::menu::MenuItem::Action {
                // The row's position, resolved against `keys` rather than
                // against the live tray -- see the field's documentation.
                id: index as u64,
                label: if icon.tooltip.is_empty() {
                    // A program that registered no tooltip still has to be
                    // nameable, or its row is a blank line.
                    icon.glyph.clone()
                } else {
                    icon.tooltip.clone()
                },
                shortcut: None,
                icon: Some(icon.glyph.clone()),
                enabled: true,
                checked: None,
            });
        }
        let Some(rect) = self.tray_overflow_rect() else {
            return;
        };
        let mut menu = guitk::menu::ContextMenu::new(items);
        // The real screen, not the toolkit's assumed one: this menu opens
        // from the taskbar at the bottom edge, which is exactly where a wrong
        // viewport puts the rows off the display.
        menu.show(rect.x, rect.y, self.viewport());
        self.tray_overflow_menu = Some((menu, keys));
    }

    /// Offer to pin or unpin the start-menu row at `index`.
    ///
    /// One item, and its label is the *action*, not the state: "Pin to
    /// taskbar" when it is not pinned and "Unpin from taskbar" when it is. A
    /// menu that said "Pinned" with a tick would be a second way of saying
    /// what the taskbar already shows, and would leave the user to work out
    /// that clicking it reverses the thing.
    fn open_pin_menu(&mut self, target: PinTarget, x: f32, y: f32) {
        let Some(exec) = self.exec_of(target) else {
            return;
        };
        let label = if self.is_pinned(&exec) {
            "Unpin from taskbar"
        } else {
            "Pin to taskbar"
        };
        let start_label = if self.is_pinned_to_start(&exec) {
            "Unpin from Start menu"
        } else {
            "Pin to Start menu"
        };
        let items = vec![
            guitk::menu::MenuItem::Action {
                id: Self::MENU_PIN_TOGGLE,
                label: label.to_string(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: None,
            },
            guitk::menu::MenuItem::Action {
                id: Self::MENU_START_PIN_TOGGLE,
                label: start_label.to_string(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: None,
            },
            guitk::menu::MenuItem::Action {
                id: Self::MENU_ADD_TO_DESKTOP,
                label: "Add to desktop".to_string(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: None,
            },
        ];
        let mut menu = guitk::menu::ContextMenu::new(items);
        // The real screen, not the toolkit's assumed one -- the same reason
        // the overflow list passes it: this opens from wherever the start menu
        // is, which is near the bottom edge.
        menu.show(x, y, self.viewport());
        self.pin_menu = Some((menu, target));
    }

    /// A press while the pin menu is open.
    fn click_pin_menu(&mut self, x: f32, y: f32) -> ShellAction {
        let Some((menu, target)) = self.pin_menu.as_mut() else {
            return ShellAction::Consumed;
        };
        let target = *target;
        // A press that named no row -- on the panel's padding, or outside it.
        // Either way the menu closes, as the desktop menu does.
        let Some(id) = menu.handle_click(x, y) else {
            self.pin_menu = None;
            return ShellAction::Consumed;
        };
        self.pin_menu = None;
        self.activate_pin_menu_item(id, target);
        ShellAction::Consumed
    }

    /// One row of the pin menu, chosen by click or by key.
    fn activate_pin_menu_item(&mut self, id: MenuItemId, target: PinTarget) {
        match id {
            Self::MENU_PIN_TOGGLE => self.toggle_pin(target),
            Self::MENU_START_PIN_TOGGLE => {
                if let Some(exec) = self.exec_of(target) {
                    self.toggle_start_pin(&exec);
                }
            }
            Self::MENU_ADD_TO_DESKTOP => self.add_to_desktop(target),
            _ => {}
        }
    }

    /// Pin `exec` to the start menu, or unpin it if it is pinned there
    /// already -- "Pin to Start menu" on the pin menu and on a program
    /// icon's own menu, whose label says which it will do.
    fn toggle_start_pin(&mut self, exec: &str) {
        if self.is_pinned_to_start(exec) {
            self.unpin_from_start(exec);
        } else {
            self.pin_to_start(exec);
        }
    }

    /// Put a shortcut to the program `target` names on the desktop -- the pin
    /// menu's "Add to desktop" -- or select the one already there.
    ///
    /// Named as the start menu or the taskbar names it, and saved with the
    /// layout, so it is still there after a login.
    fn add_to_desktop(&mut self, target: PinTarget) {
        let Some(exec) = self.exec_of(target) else {
            return;
        };
        let name = match target {
            PinTarget::StartMenuRow(index) => self
                .start_menu_entries()
                .get(index)
                .map(|entry| entry.name.clone()),
            PinTarget::Pinned(index) => self
                .taskbar
                .pinned_apps()
                .get(index)
                .map(|app| app.display_name.clone()),
        }
        .unwrap_or_else(|| exec.clone());
        let (_, added) = self.icons.add_shortcut(
            &name,
            icons::IconType::Executable,
            icons::IconAction::OpenPath(PathBuf::from(&exec)),
        );
        self.icons_dirty |= added;
    }

    /// The executable a pin menu target names, if it still names one.
    ///
    /// Cloned out of the borrow in both arms: `start_menu_entries` builds its
    /// list on demand, so an entry does not outlive the call that produced it,
    /// and the pinned list is behind `&self` for the same reason.
    fn exec_of(&self, target: PinTarget) -> Option<String> {
        match target {
            PinTarget::StartMenuRow(index) => self
                .start_menu_entries()
                .get(index)
                .map(|entry| entry.executable_path.clone()),
            PinTarget::Pinned(index) => self
                .taskbar
                .pinned_apps()
                .get(index)
                .map(|app| app.exec_path.clone()),
        }
    }

    /// Pin what `target` names, or unpin it if it is already pinned.
    ///
    /// A target that no longer names anything does nothing. The lists are
    /// rebuilt between the menu opening and the row being taken -- a program
    /// can be unpinned from elsewhere in between -- and acting on a stale
    /// index would unpin whichever program had moved into that slot.
    fn toggle_pin(&mut self, target: PinTarget) {
        let Some(exec) = self.exec_of(target) else {
            return;
        };
        if self.is_pinned(&exec) {
            self.unpin_app(&exec);
            return;
        }
        let name = match target {
            PinTarget::StartMenuRow(index) => self
                .start_menu_entries()
                .get(index)
                .map(|entry| entry.name.clone()),
            // Already pinned by construction, so this arm is unreachable in
            // practice; the name it would use is the launcher's.
            PinTarget::Pinned(_) => None,
        }
        .unwrap_or_else(|| self.app_name_for(&exec));
        self.pin_app(&exec, &name);
    }

    /// The pin menu's draw commands, empty when it is closed.
    #[must_use]
    pub fn render_pin_menu(&self) -> Option<RenderTree> {
        let (menu, _) = self.pin_menu.as_ref()?;
        let mut tree = RenderTree::new();
        tree.commands
            .extend(menu.render(&Palette::from_settings(&self.appearance)));
        Some(tree)
    }

    /// A press while the overflow list is open.
    fn click_tray_overflow(&mut self, x: f32, y: f32) -> ShellAction {
        let Some((menu, _)) = self.tray_overflow_menu.as_mut() else {
            return ShellAction::Consumed;
        };
        // A press that named no row: on the panel's own padding, or outside
        // it. Either way the list closes, as the desktop menu does.
        let Some(id) = menu.handle_click(x, y) else {
            self.tray_overflow_menu = None;
            return ShellAction::Consumed;
        };
        match self.take_overflow_row(id) {
            Some(request) => ShellAction::Control(request),
            None => ShellAction::Consumed,
        }
    }

    /// Resolve a chosen row, closing the list either way.
    ///
    /// Shared by the pointer and the keyboard so that the two cannot come to
    /// disagree about which program a row names -- the failure that would
    /// produce is a click delivered to the wrong program, and it would show up
    /// in only one of the two ways of choosing.
    fn take_overflow_row(&mut self, id: guitk::menu::MenuItemId) -> Option<ShellRequest> {
        let key = self
            .tray_overflow_menu
            .as_ref()
            .and_then(|(_, keys)| usize::try_from(id).ok().and_then(|at| keys.get(at)))
            .copied();
        self.tray_overflow_menu = None;
        let key = key?;
        // The program may have exited while the list was open.
        if !self
            .tray_icons
            .iter()
            .any(|icon| tray_dnd::TrayIconKey::of(icon) == key)
        {
            return None;
        }
        Some(ShellRequest::ClickTrayIcon {
            owner: key.owner,
            id: key.id,
            // A row in a list is a primary activation whatever button opened
            // it, and only the primary button can open this one.
            button: MouseButton::Left,
        })
    }

    /// The keyboard's counterpart to [`click_tray_overflow`](Self::click_tray_overflow).
    fn activate_overflow_row(&mut self, id: guitk::menu::MenuItemId) -> HotkeyOutcome {
        HotkeyOutcome::ask(self.take_overflow_row(id))
    }

    /// Carry a pressed tray icon to `(x, y)`.
    ///
    /// **The icons shuffle under the pointer rather than waiting for the
    /// drop.** The row is the only feedback a 24-pixel glyph can give -- there
    /// is no room for an insertion line and no ghosting in the render tree --
    /// and without it a drag is invisible until it is over, which is
    /// indistinguishable from a drag that is not working.
    ///
    /// Only the left button drags. A right press keeps the grab, so that
    /// wandering off the icon before releasing does not fire a click on a
    /// different one, but it does not rearrange anything: dragging with the
    /// right button is a gesture nothing else in this shell has.
    fn drag_tray_icon_to(&mut self, x: f32, y: f32) -> bool {
        let Some(drag) = self.tray_drag.as_mut() else {
            return false;
        };
        if drag.button != MouseButton::Left {
            return false;
        }
        drag.source.on_move(x, y);
        if !drag.source.is_dragging() {
            return false;
        }
        let Some(key) = drag.source.pressed_key() else {
            return false;
        };
        // The boundary is measured over the icons *on the bar*, which are a
        // filtered slice of the arrangement -- the user may have hidden one,
        // and the rest may not fit. So it is turned into the name of the icon
        // to land in front of before it crosses into the arrangement, where an
        // index from the drawn run would mean a different place.
        //
        // Resolved against the shown list rather than the drawn one, so that
        // a drop at the right-hand end of a run with a chevron lands in front
        // of the first icon that did not fit, rather than at the very end
        // behind all of them.
        let boundary = self.tray_drop_boundary(x);
        let shown = self.tray_arrangement.shown_keys();
        let anchor = shown.get(boundary).copied();
        self.tray_arrangement.move_before(key, anchor)
    }

    /// Move a pinned button along the bar. Answers whether anything moved.
    fn drag_pinned_to(&mut self, x: f32, y: f32) -> bool {
        let bar_top = self.taskbar_rect().y;
        let on_start = self.start_button_rect().contains(x, y);
        let Some(drag) = self.pin_drag.as_mut() else {
            return false;
        };
        drag.on_move(x, y);
        if !drag.is_dragging() {
            return false;
        }
        self.carry_at = (x, y);
        // Carried up off the bar, the button is on its way to the desktop;
        // on the start button, to the start menu. Either way the row stops
        // rearranging under a pointer that is not on it.
        self.pin_drag_off_bar = y < bar_top || on_start;
        if self.pin_drag_off_bar {
            return false;
        }
        let Some(exec) = drag.pressed_key() else {
            return false;
        };
        let Some(from) = self.pinned_index_of(&exec) else {
            // Unpinned from elsewhere while the drag was in flight. There is
            // nothing left to move, and inventing a position for it would put
            // a program back on the bar the user had just taken off.
            return false;
        };
        let count = self.taskbar.pinned_apps().len();
        let to = self.row_drop_index(0, count, from, x);
        if to == from {
            return false;
        }
        self.taskbar.reorder_pinned(from, to);
        self.save_pinned();
        true
    }

    /// Move a window's button along the row of window buttons, to the place
    /// under the pointer. Answers whether anything moved.
    ///
    /// Only along its own row: the pinned buttons are launchers, in an order
    /// of their own, and a window's button among them would be neither.
    /// Carried off the bar, it stays where it last was.
    fn drag_window_button_to(&mut self, x: f32, y: f32) -> bool {
        let Some(press) = self.window_press.as_mut() else {
            return false;
        };
        press.source.on_move(x, y);
        if !press.source.is_dragging() {
            return false;
        }
        let Some(id) = press.source.pressed_key() else {
            return false;
        };
        if !self.taskbar_rect().contains(x, y) {
            return false;
        }
        let shown: Vec<WindowId> = self.taskbar_button_windows().iter().map(|w| w.id).collect();
        // Gone from the bar while it was held -- closed, or moved to another
        // desktop. Nothing left to move.
        let Some(from) = shown.iter().position(|w| *w == id) else {
            return false;
        };
        let pins = self.taskbar.pinned_apps().len();
        let to = self.row_drop_index(pins, shown.len(), from, x);
        if to == from {
            return false;
        }
        let mut row = shown.clone();
        let moved = row.remove(from);
        row.insert(to, moved);
        // Written back into the places the shown windows hold in the whole
        // order, so a window on another desktop keeps its own place.
        let mut next = row.into_iter();
        for slot in &mut self.button_order {
            if shown.contains(slot)
                && let Some(id) = next.next()
            {
                *slot = id;
            }
        }
        true
    }

    /// Where, in a row of `count` taskbar buttons starting at slot
    /// `first_slot`, the button at `from` belongs when dragged to `x`: after
    /// every *other* button in the row whose middle `x` has passed. So it
    /// moves one place each time the pointer crosses a neighbour's middle,
    /// in either direction, and can never leave its row -- the pins' row and
    /// the windows' row are each asked about separately.
    ///
    /// Counting only the others is the point. The rule this replaced
    /// ("before the first button whose middle is right of `x`") counted the
    /// dragged button's own middle too, so a pin dragged *rightwards* swapped
    /// with its neighbour as soon as the pointer crossed its own centre -- a
    /// few pixels into the drag -- while a leftward drag behaved.
    fn row_drop_index(&self, first_slot: usize, count: usize, from: usize, x: f32) -> usize {
        (0..count)
            .filter(|&index| index != from)
            .filter(|&index| {
                let button = self.taskbar_button_rect(first_slot.saturating_add(index));
                button.x + button.w / 2.0 < x
            })
            .count()
    }

    /// Let go of a pressed window button: a drag along the row already moved
    /// it on the way here; a click summons the window -- or minimises it, if
    /// it was the one in front when the button was pressed, since the
    /// button is a toggle and not a second way to focus what is focused.
    fn finish_window_press(&mut self) -> ShellAction {
        let Some(mut press) = self.window_press.take() else {
            return ShellAction::Consumed;
        };
        let id = press.source.pressed_key();
        // Read before `on_release`, which resets the source.
        let was_drag = press.source.on_release();
        let Some(id) = id.filter(|_| !was_drag) else {
            return ShellAction::Consumed;
        };
        // Closed between the press and the release: nothing to summon, and
        // asking would only be refused.
        if !self.windows.contains_key(&id) {
            return ShellAction::Consumed;
        }
        ShellAction::Control(ShellRequest::window(
            id,
            if press.was_focused {
                ShellControlAction::Minimize
            } else {
                // `Activate`, not `Restore`: a window minimised while
                // maximised has to come back maximised, and restoring would
                // silently drop a state the user never asked to leave. See
                // the compositor's `activate_window`.
                ShellControlAction::Activate
            },
        ))
    }

    /// Which pinned slot holds `exec`, if any.
    fn pinned_index_of(&self, exec: &str) -> Option<usize> {
        self.taskbar
            .pinned_apps()
            .iter()
            .position(|app| app.exec_path == exec)
    }

    /// Release a pressed pinned button: a reorder just ended, or the program
    /// is about to be started.
    fn finish_pinned_press(&mut self, x: f32, y: f32) -> ShellAction {
        let off_bar = core::mem::take(&mut self.pin_drag_off_bar);
        let Some(mut drag) = self.pin_drag.take() else {
            return ShellAction::Consumed;
        };
        let exec = drag.pressed_key();
        // Read before `on_release`, which resets the source.
        let was_drag = drag.on_release();
        if was_drag {
            // Let go off the bar, on the desktop: a shortcut there, and the
            // pin stays -- a drag between two places copies, as a drag from
            // the start menu does. Back on the bar, the row already
            // rearranged itself on the way here.
            if off_bar && let Some(exec) = exec {
                let name = self.app_name_for(&exec);
                self.drop_program(&exec, &name, x, y);
            }
            return ShellAction::Consumed;
        }
        exec.map_or(ShellAction::Consumed, |exec| {
            ShellAction::Launch(hotkeys::Launch::program(exec))
        })
    }

    /// Let go of a pressed start-menu row: a click starts the program; a drag
    /// carries it to wherever it was let go -- the taskbar pins it at the gap
    /// nearest the pointer, the desktop gets a shortcut to it there -- and
    /// dropping it back on the menu asks for nothing.
    fn finish_start_press(&mut self, x: f32, y: f32) -> ShellAction {
        let Some(mut drag) = self.start_drag.take() else {
            return ShellAction::Consumed;
        };
        let exec = drag.source.pressed_key();
        // Read before `on_release`, which resets the source.
        let was_drag = drag.source.on_release();
        let Some(exec) = exec else {
            return ShellAction::Consumed;
        };
        if !was_drag {
            self.close_start_menu();
            return ShellAction::Launch(hotkeys::Launch::program(PathBuf::from(exec)));
        }
        let on_menu = self.start_menu_rect().contains(x, y);
        self.drop_program(&exec, &drag.name, x, y);
        // Let go on the menu -- arranging its pinned rows, or back where it
        // came from, which asks for nothing -- the menu stays up to be used.
        // Anywhere else, the program has gone where it was carried.
        if !on_menu {
            self.close_start_menu();
        }
        ShellAction::Consumed
    }

    /// Put a program that was carried here where it was let go: over the
    /// taskbar, pinned at the gap nearest the pointer -- moved there, if it
    /// was pinned already; anywhere else, as a shortcut on the desktop,
    /// centred where it was let go (or the one already there, selected).
    fn drop_program(&mut self, exec: &str, name: &str, x: f32, y: f32) {
        match self.carry_target(x, y) {
            Some(CarryTarget::Taskbar) => {
                let gap = self.pinned_insert_boundary(x);
                self.pin_into_gap(exec, name, gap);
            }
            Some(CarryTarget::StartMenu) => {
                let gap = self.start_pin_insert_boundary(x, y);
                self.start_pin_into_gap(exec, gap);
            }
            Some(CarryTarget::Desktop) => {
                let (_, added) = self.icons.add_shortcut_at(
                    name,
                    icons::IconType::Executable,
                    icons::IconAction::OpenPath(PathBuf::from(exec)),
                    x,
                    y,
                );
                self.icons_dirty |= added;
            }
            // Over somebody's window, or a part of the shell that takes no
            // programs: nothing. Nothing on this system can yet hand a
            // program to another program by dropping it, and a shortcut
            // made on the desktop behind the window instead would appear
            // somewhere the user was not pointing.
            None => {}
        }
    }

    /// Where a program carried here would go if it were let go at `(x, y)`
    /// -- `None` where letting go does nothing. One answer for the drop and
    /// for the label that says beforehand what the drop will do, so the two
    /// cannot disagree.
    fn carry_target(&self, x: f32, y: f32) -> Option<CarryTarget> {
        // The start button first: it is on the taskbar, and what it means
        // there is the start menu, not a place in the row of pins.
        if self.start_button_rect().contains(x, y) {
            return Some(CarryTarget::StartMenu);
        }
        if self.taskbar_rect().contains(x, y) {
            return Some(CarryTarget::Taskbar);
        }
        match self.hit_test(x, y) {
            // Only the pinned rows take a drop: the rest of the list is the
            // launcher's, in the launcher's order, and not the user's to
            // arrange.
            Hit::StartMenuEntry(index) if index < self.start_pins_listed() => {
                Some(CarryTarget::StartMenu)
            }
            Hit::Desktop if self.window_at(x, y).is_none() => Some(CarryTarget::Desktop),
            _ => None,
        }
    }

    /// The application window drawn at a point, if any: the topmost one on
    /// the desktop being shown, not minimised, whose frame contains it.
    #[must_use]
    pub fn window_at(&self, x: f32, y: f32) -> Option<WindowId> {
        self.windows
            .values()
            .filter(|w| w.on_glass() && w.desktop == self.current_desktop && w.frame.contains(x, y))
            .max_by_key(|w| w.z_order)
            .map(|w| w.id)
    }

    /// Pin `exec` into gap `gap` of the pinned run (`0..=len`, before the
    /// button of that index), or move it there if it is pinned already.
    /// Answers the gap just after where it ended up, which is where a second
    /// program dropped in the same place belongs -- so several dropped
    /// together keep their order instead of stacking up reversed.
    fn pin_into_gap(&mut self, exec: &str, name: &str, gap: usize) -> usize {
        let from = self.pinned_index_of(exec);
        if from.is_none() {
            // Written once, below, with the button already in its gap.
            self.pin_app_without_saving(exec, name);
        }
        // Refused -- an empty path is not a program: nothing moved.
        let Some(now) = self.pinned_index_of(exec) else {
            return gap;
        };
        let len = self.taskbar.pinned_apps().len();
        // Taking a button out of the run closes its own gap, so a gap past
        // it is one lower by the time the button goes back in -- and the
        // gaps either side of it are both where it already is.
        let to = match from {
            Some(from) if gap > from => gap.saturating_sub(1),
            _ => gap,
        }
        .min(len.saturating_sub(1));
        if to != now {
            self.taskbar.reorder_pinned(now, to);
        }
        if from.is_none() || to != now {
            self.save_pinned();
        }
        to.saturating_add(1)
    }

    /// The gap in the pinned run a new button dropped at `x` goes into,
    /// `0..=len`: before the first button whose middle is right of `x`, or
    /// after the last. Unlike [`row_drop_index`](Self::row_drop_index)
    /// it can name the end, because a new button can go after the last one.
    fn pinned_insert_boundary(&self, x: f32) -> usize {
        let count = self.taskbar.pinned_apps().len();
        (0..count)
            .find(|&index| {
                let button = self.taskbar_button_rect(index);
                x < button.x + button.w / 2.0
            })
            .unwrap_or(count)
    }

    /// Release a pressed tray icon: either a reorder just ended, or the
    /// program that owns the icon is about to hear about a click.
    fn finish_tray_press(&mut self) -> ShellAction {
        let Some(mut drag) = self.tray_drag.take() else {
            return ShellAction::Consumed;
        };
        let key = drag.source.pressed_key();
        // Read before `on_release`, which resets the source.
        let was_drag = drag.source.on_release();
        if was_drag {
            // The row already rearranged itself on the way here.
            return ShellAction::Consumed;
        }
        let Some(key) = key else {
            return ShellAction::Consumed;
        };
        // Named by key rather than by slot, because the slot may have changed
        // under the pointer -- another program registering an icon reorders
        // nothing, but a program *departing* does, and a click that resolved a
        // stale index would be delivered to the wrong program.
        if !self
            .tray_icons
            .iter()
            .any(|icon| tray_dnd::TrayIconKey::of(icon) == key)
        {
            return ShellAction::Consumed;
        }
        ShellAction::Control(ShellRequest::ClickTrayIcon {
            owner: key.owner,
            id: key.id,
            button: drag.button,
        })
    }

    /// Which gap between icons the pointer is nearest, `0..=len`.
    ///
    /// Measured through [`tray_dnd::TrayDropTarget`] rather than recomputed
    /// here: it is the type whose job this is, and a second copy of a
    /// rounding rule is how the drawn insertion point and the actual one come
    /// to disagree.
    fn tray_drop_boundary(&self, x: f32) -> usize {
        let rects = self.tray_icon_rects();
        let Some(first) = rects.first() else {
            return 0;
        };
        let mut target = tray_dnd::TrayDropTarget::new(
            0,
            first.x,
            first.y,
            first.w * rects.len() as f32,
            first.h,
        );
        target.set_icon_count(rects.len());
        target.set_icon_cell_width(first.w);
        target.calc_insertion_index(x);
        target.insertion_index.unwrap_or(0)
    }

    /// How wide one tray icon's slot is.
    ///
    /// Every icon gets the same slot regardless of its glyph, so the tray does
    /// not reflow when a program swaps a narrow glyph for a wide one -- the
    /// same reason the clock is measured rather than assumed, pointing the
    /// other way: the clock's width is a property of the *user's* format and
    /// changes rarely; an icon's is a property of another program and can
    /// change at any moment.
    fn tray_icon_slot(&self) -> f32 {
        self.scale(TRAY_ICON_SLOT)
    }

    /// How many slots the bar can spare for the application icons, chevron
    /// included.
    ///
    /// Derived on every call rather than stored. It depends on the taskbar's
    /// width and the display scale, both of which change without anything
    /// touching the tray -- so a cached count is a flag someone has to
    /// remember to refresh on resize, and forgetting is silent.
    fn tray_slot_budget(&self) -> usize {
        let slot = self.tray_icon_slot();
        if slot <= 0.0 {
            return 0;
        }
        let budget = self.taskbar_rect().w * TRAY_ICON_SHARE;
        if budget < slot {
            return 0;
        }
        // Truncating is the point: a slot that only half fits is a glyph drawn
        // over the clock.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "guarded above: budget and slot are both positive and                       budget >= slot, and the quotient of two taskbar-sized                       lengths cannot approach usize's range"
        )]
        let slots = (budget / slot) as usize;
        slots
    }

    /// How many application icons are actually drawn on the bar.
    ///
    /// When they all fit, all of them. When they do not, one slot goes to the
    /// chevron that reaches the rest -- so the run is never silently truncated
    /// into icons the user has no way to click.
    fn tray_icon_capacity(&self) -> usize {
        let budget = self.tray_slot_budget();
        let shown = self.tray_arrangement.shown_keys().len();
        if shown <= budget {
            shown
        } else {
            budget.saturating_sub(1)
        }
    }

    /// Whether some icon the user has not hidden has no room on the bar.
    #[must_use]
    pub fn tray_overflows(&self) -> bool {
        self.tray_arrangement.shown_keys().len() > self.tray_icon_capacity()
    }

    /// The icons the bar has no room for, in order.
    #[must_use]
    pub fn overflowed_tray_icons(&self) -> Vec<&guiremote::tray::TrayIcon> {
        self.join_tray(
            &self
                .tray_arrangement
                .overflow_icons(self.tray_icon_capacity())
                .iter()
                .map(|slot| slot.key)
                .collect::<Vec<_>>(),
        )
    }

    /// How much width the application icons take, padding included.
    fn app_tray_width(&self) -> f32 {
        let slots = self
            .tray_icon_capacity()
            .saturating_add(usize::from(self.tray_overflows()));
        if slots == 0 {
            return 0.0;
        }
        let slot = self.tray_icon_slot();
        let padding = self.scale(TRAY_PADDING);
        // One padding between the block and the shell's own items, not one per
        // icon: the icons sit as a run, which is what makes them read as one
        // region rather than four unrelated glyphs.
        //
        // Counted from the same place the rectangles are, so that width and
        // positions cannot disagree about how many slots there are.
        #[allow(
            clippy::cast_precision_loss,
            reason = "bounded by tray_slot_budget, which is a fraction of the                       taskbar measured in 24-pixel slots"
        )]
        let count = slots as f32;
        slot.mul_add(count, padding)
    }

    /// Where each application icon is drawn, left to right.
    ///
    /// Placed from the shell's own tray items rather than from the display
    /// edge, so that a wider clock pushes the icons left instead of drawing
    /// over them -- the rule the rest of the tray already follows.
    #[must_use]
    pub fn tray_icon_rects(&self) -> Vec<Rect> {
        let bar = self.taskbar_rect();
        let slot = self.tray_icon_slot();
        let padding = self.scale(TRAY_PADDING);
        // The left edge of everything the shell itself draws in the tray.
        let shell_items = self.clock_width()
            + self.scale(TRAY_BELL_WIDTH)
            + self.desktop_indicator_width()
            + self.layout_indicator_width()
            + padding * 4.0;
        let mut x = (bar.w - shell_items - self.app_tray_width() + padding).max(0.0);
        // The chevron, when there is one, sits at the *left* of the run: it is
        // the edge the run grows from, so the icons that do fit keep the same
        // position as icons appear and depart behind it.
        if self.tray_overflows() {
            x += slot;
        }
        let capacity = self.tray_icon_capacity();
        let mut rects = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            rects.push(Rect::new(x, bar.y, slot, bar.h));
            x += slot;
        }
        rects
    }

    /// The display, as the toolkit's placement code wants it.
    ///
    /// One conversion in one place. `screen_width`/`screen_height` are `u32`
    /// public fields and every other reader spells the cast itself; a popup
    /// placed against a *differently* rounded screen than the one the taskbar
    /// is laid out on would be off by a fraction at the edge, which is the
    /// edge that matters.
    #[allow(
        clippy::cast_precision_loss,
        reason = "a display dimension is exact in f32 for every size hardware produces"
    )]
    fn viewport(&self) -> (f32, f32) {
        (self.screen_width as f32, self.screen_height as f32)
    }

    /// Where the overflow chevron is drawn, if there is one.
    ///
    /// Immediately left of the first drawn icon, and derived from the same
    /// run: `tray_icon_rects` already skipped a slot for it, so this reads
    /// that slot back rather than recomputing the run's origin. Two
    /// computations of one edge is how a button ends up one slot away from the
    /// glyph that represents it.
    #[must_use]
    pub fn tray_overflow_rect(&self) -> Option<Rect> {
        if !self.tray_overflows() {
            return None;
        }
        let slot = self.tray_icon_slot();
        let bar = self.taskbar_rect();
        match self.tray_icon_rects().first() {
            Some(first) => Some(Rect::new(first.x - slot, bar.y, slot, bar.h)),
            // Every slot went to the chevron: the bar is too narrow for even
            // one icon beside it. The run's origin is then the chevron's.
            None => {
                let padding = self.scale(TRAY_PADDING);
                let shell_items = self.clock_width()
                    + self.scale(TRAY_BELL_WIDTH)
                    + self.desktop_indicator_width()
                    + self.layout_indicator_width()
                    + padding * 4.0;
                let x = (bar.w - shell_items - self.app_tray_width() + padding).max(0.0);
                Some(Rect::new(x, bar.y, slot, bar.h))
            }
        }
    }

    /// The notification bell's clickable area, immediately left of the clock.
    ///
    /// Placed from the clock rather than from the tray's left edge: the tray's
    /// contents are laid out right-to-left from the display edge — so that a
    /// wider clock pushes everything left instead of running off the screen —
    /// and an item positioned from the *left* edge of a right-aligned strip
    /// would drift the moment the clock's switches changed its width.
    ///
    /// Full bar height, like [`clock_rect`](Self::clock_rect), because a target
    /// only as tall as the glyph misses most presses aimed at it.
    #[must_use]
    pub fn bell_rect(&self) -> Rect {
        let bar = self.taskbar_rect();
        let padding = self.scale(TRAY_PADDING);
        let width = self.scale(TRAY_BELL_WIDTH) + padding;
        let x = (self.clock_rect().x - width).max(0.0);
        Rect::new(x, bar.y, width.min((bar.w - x).max(0.0)), bar.h)
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
            let Some(info) = datetime_settings::zone(&extra.tz_id) else {
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
        self.close_start_menu();
        self.shortcut_card_open = false;

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

    /// Open the notification pane, or close it if it is already open.
    ///
    /// Lands the pane on its destination immediately; a caller with a frame
    /// clock follows this with [`NotificationPane::begin_slide`] to rewind the
    /// jump into an animation. See `design-decisions.md` §520 and §562.
    ///
    /// [`NotificationPane::begin_slide`]: notif_pane::NotificationPane::begin_slide
    pub fn toggle_notifications(&mut self) {
        if self.notifications.pane_state().is_visible() {
            self.notifications.hide();
            return;
        }
        // Same rule the calendar states: two panels over one taskbar at once is
        // a state the user cannot have asked for.
        self.close_start_menu();
        self.calendar.set_visible(false);
        // The pane's scrim dims the whole screen behind it, so a card left open
        // under it would be a card the user cannot read.
        self.shortcut_card_open = false;
        self.notifications.show();
    }

    /// What the shortcut editor needs to know about this desktop.
    fn shortcut_context(&self) -> shortcut_editor::Context {
        shortcut_editor::Context {
            // Saturated rather than truncated: a desktop count past 255 is
            // absurd, and 255 "Switch to Desktop" entries is the honest
            // answer to it where `as u8` would offer a handful.
            desktops: u8::try_from(self.num_desktops).unwrap_or(u8::MAX),
            picker_rows: shortcut_editor::picker_rows(self.shortcut_card_budget()),
        }
    }

    /// How tall the shortcut card may be: the screen less a margin top and
    /// bottom. One function for the card's layout, its placement and the
    /// editor's page size, since the three must agree.
    fn shortcut_card_budget(&self) -> f32 {
        #[allow(
            clippy::cast_precision_loss,
            reason = "a screen height is far inside f32's exact range"
        )]
        let screen_h = self.screen_height as f32;
        (screen_h - SHORTCUT_CARD_MARGIN * 2.0).max(SHORTCUT_CARD_MARGIN)
    }

    /// Save the bindings after the editor changed them, and say so if saving
    /// failed.
    ///
    /// Written now rather than on shutdown: a desktop that lost power between
    /// the two would forget the change, and the user has no way to know saving
    /// was still pending. A failure is appended to the editor's own message
    /// rather than replacing it: the change *worked* -- only keeping it did not
    /// -- and that is the difference between a shortcut that will be gone
    /// tomorrow and one the user believes is set.
    fn save_edited_shortcuts(&mut self) {
        if let Err(e) = self.save_shortcuts() {
            let done = self
                .shortcut_editor
                .message()
                .unwrap_or("The change was made")
                .to_string();
            self.shortcut_editor
                .set_message(Some(format!("{done}, but could not be saved: {e}")));
        }
    }

    /// Open the card listing every shortcut, or close it if it is already open.
    ///
    /// Clears the taskbar's own panels for the same reason
    /// [`toggle_notifications`](Self::toggle_notifications) does — two panels
    /// over one desktop at once is a state the user cannot have asked for —
    /// but deliberately leaves the *overview* and the zone overlay alone: those
    /// are full-screen surfaces driven by their own chords, and a user who
    /// opens the card to find out what the overview's chord is should not have
    /// the overview shut in the act of looking it up.
    pub fn toggle_shortcut_card(&mut self) {
        if self.shortcut_card_open {
            self.shortcut_card_open = false;
            return;
        }
        self.close_start_menu();
        self.calendar.set_visible(false);
        self.notifications.hide();
        // Whatever the editor was in the middle of when the card last went
        // away is not what the user is opening it for.
        self.shortcut_editor.reset();
        self.shortcut_card_open = true;
    }

    /// Post a notification, opening nothing.
    ///
    /// The pane is the *history*; this puts a message into it whether or not
    /// anyone is looking. A message that could only arrive while the pane was
    /// open would be a message the user can only read by already knowing it is
    /// there.
    ///
    /// # Focus assist suppresses attention, not the record
    ///
    /// If [`focus`](Self::focus) says this app is silenced right now, the
    /// notification is still stored — it is simply marked
    /// [`silent`](notif_pane::Notification::silent), which keeps it out of the
    /// bell's badge and out of [`attention_count`]. Dropping it instead was the
    /// obvious alternative and is wrong: Do Not Disturb is a request not to be
    /// interrupted, not a request to be lied to about what happened, and a user
    /// who turns it off has no way to ask for the missed hour back. `Windows`
    /// and macOS both keep the record for the same reason.
    ///
    /// The suppressed ones are also counted on the manager, so the "you missed
    /// N things" summary [`FocusAssistManager::show_summary`] promises has a
    /// number to show.
    ///
    /// [`attention_count`]: notif_pane::NotificationPane::attention_count
    /// [`FocusAssistManager::show_summary`]: focus_assist::FocusAssistManager
    pub fn notify(&mut self, mut notif: notif_pane::Notification) -> u64 {
        // Keyed on `app_name` because that is the only app identity a
        // `Notification` carries, and it is the same key the pane files its
        // per-app settings under. A separate opaque id would have to be
        // threaded through every poster to buy nothing the name does not
        // already buy.
        if !self.focus.should_show_notification(&notif.app_name) {
            self.focus.record_suppressed();
            notif.silent = true;
        }
        self.notifications.push_notification(notif)
    }

    /// Push the focus manager's mode back onto the pane's two switches.
    ///
    /// Do Not Disturb and Focus Mode are one four-valued mode spelled as two
    /// booleans, so the mapping has to be stated in one place or the switches
    /// drift from what is actually happening:
    ///
    /// | mode | Do Not Disturb | Focus Mode |
    /// |---|---|---|
    /// | `Off` | off | off |
    /// | `PriorityOnly` | off | **on** |
    /// | `AlarmsOnly` | off | **on** |
    /// | `TotalSilence` | **on** | off |
    ///
    /// `AlarmsOnly` reads as "Focus Mode on", which is true — focus assist is
    /// engaged and it is not total silence — so no switch ever shows a state
    /// the system is not in. What the pair *cannot* do is tell `AlarmsOnly`
    /// apart from `PriorityOnly`; that distinction lives on the focus-assist
    /// settings page, and the tray glyph shows it in the meantime.
    ///
    /// Called after every press rather than only when the mode changes,
    /// because an unchanged mode after a press is precisely the case that
    /// needs the write-back: the switch has already moved on screen and must
    /// be moved back.
    fn sync_quick_settings(&mut self) {
        use focus_assist::FocusMode;
        use notif_pane::QuickSetting;
        let mode = self.focus.effective_mode();
        self.notifications
            .set_quick_setting(QuickSetting::DoNotDisturb, mode == FocusMode::TotalSilence);
        self.notifications.set_quick_setting(
            QuickSetting::FocusMode,
            matches!(mode, FocusMode::PriorityOnly | FocusMode::AlarmsOnly),
        );
    }

    /// Act on everything the pane reported since the last call, and empty it.
    ///
    /// Returns the one thing the shell cannot do itself: the program a clicked
    /// notification's [`action`](notif_pane::Notification::action) names, for
    /// the caller to turn into a [`ShellAction::Launch`]. Everything else the
    /// pane has already done to its own state before reporting it, so the rest
    /// of the arms are the shell deciding whether anything *outside* the pane
    /// should follow.
    ///
    /// Calling this is mandatory, not housekeeping: the pane's event buffer is
    /// unbounded and nothing else empties it. Until this existed the shell
    /// accumulated one entry per click for the life of the session and the
    /// quick-setting switches moved without changing anything.
    fn apply_pane_events(&mut self) -> Option<String> {
        use notif_pane::{NotifPaneEvent, QuickSetting};
        let mut launch = None;
        for event in self.notifications.drain_events() {
            match event {
                NotifPaneEvent::QuickSettingToggled(qs) => match qs {
                    QuickSetting::DoNotDisturb | QuickSetting::FocusMode => {
                        self.apply_focus_toggle(qs);
                    }
                    QuickSetting::NightLight => self.toggle_night_light(),
                    // Wi-Fi and Bluetooth have no service in this process to
                    // talk to: they are the network and bluetooth daemons'
                    // state, reached over IPC the shell does not hold yet. The
                    // switches move and are remembered by the pane, and that
                    // is all they do — recorded in known-issues.md rather than
                    // left to be rediscovered by someone wondering why the
                    // radio stayed on.
                    //
                    // Night Light used to be in this list, described as "the
                    // compositor's gamma ramp". It is an appearance setting
                    // now, which the compositor reads from the same file
                    // everything else does, so the switch above is the whole
                    // of the work.
                    QuickSetting::WiFi | QuickSetting::Bluetooth => {}
                },
                // The pane marks the card read before reporting the click, so
                // the only thing left is the part it cannot do: start the
                // program the notification points at. A card with no action is
                // a message, and reading it is the whole of the interaction.
                NotifPaneEvent::NotificationClicked(id) => {
                    // Last one wins, which is the same rule the mouse path
                    // already follows: one press produces at most one click
                    // event, so a second can only come from a drain that was
                    // skipped, and the newer intent is the right one to honour.
                    launch = self
                        .notifications
                        .notifications()
                        .iter()
                        .find(|n| n.id == id)
                        .and_then(|n| n.action.clone())
                        .or(launch);
                }
                // The pane records the change in its own list and reports
                // it. Until this arm existed the report went nowhere, so a
                // user switching a program off in the notification pane was
                // switching off a copy nothing consulted.
                NotifPaneEvent::SettingChanged {
                    app,
                    setting,
                    value,
                } => self.apply_app_notification_setting(&app, setting, value),
                // The other three are already done by the time they are
                // reported: the card is gone, the list is empty, the pane is
                // closing. They are drained so the buffer stays bounded, and
                // matched by name so that adding a variant is a compile error
                // here rather than a silent no-op.
                NotifPaneEvent::NotificationDismissed(_)
                | NotifPaneEvent::ClearAll
                | NotifPaneEvent::Closed => {}
            }
        }
        launch
    }

    /// Turn a press on one of the two focus switches into a mode.
    ///
    /// Reads the switch the pane just flipped rather than the mode, so "the
    /// user asked for this to be on" is taken from the thing the user actually
    /// pressed. The two are mutually exclusive by construction — turning
    /// either on replaces the mode outright — which is what keeps the
    /// write-back in [`sync_quick_settings`](Self::sync_quick_settings) from
    /// having to undo a press.
    fn apply_focus_toggle(&mut self, qs: notif_pane::QuickSetting) {
        use focus_assist::FocusMode;
        use notif_pane::QuickSetting;
        let wanted = self.notifications.quick_setting_value(qs);
        let mode = match (qs, wanted) {
            (QuickSetting::DoNotDisturb, true) => FocusMode::TotalSilence,
            // Priority Only rather than Alarms Only: a switch labelled just
            // "Focus Mode" has to pick one of the two, and the milder is the
            // one a user who wanted *silence* would not have reached for —
            // they would have pressed Do Not Disturb, which is right beside it.
            (QuickSetting::FocusMode, true) => FocusMode::PriorityOnly,
            // Either switch turned off means "stop suppressing", including
            // when the mode was the *other* switch's. Turning Focus Mode off
            // while Alarms Only is engaged is the case that makes this right:
            // the switch was showing that mode, so it is the one it turns off.
            _ => FocusMode::Off,
        };
        self.set_focus_mode_by_hand(mode);
        self.sync_quick_settings();
    }

    /// Set the focus mode *as a user action*.
    ///
    /// Not the same as assigning [`FocusAssistManager::manual_mode`], and the
    /// difference is the whole of `snooze_schedule_if_it_would_resume` below:
    /// a person turning this off means "leave me alone about this until it
    /// would have changed anyway", which the manager cannot express because it
    /// has no clock. Every door a *person* comes through -- the quick
    /// settings switches today, a hotkey or a menu item tomorrow -- belongs
    /// here rather than on the field, so that a later one cannot quietly get
    /// the lesser behaviour.
    pub fn set_focus_mode_by_hand(&mut self, mode: focus_assist::FocusMode) {
        self.focus.set_mode(mode);
        if mode == focus_assist::FocusMode::Off {
            self.snooze_schedule_if_it_would_resume(Self::unix_now());
        }
    }

    /// Hold a schedule off for the rest of its period, if one is why focus
    /// assist is on.
    ///
    /// The user has just pressed a switch labelled "off". Turning off a
    /// *manual* mode is enough on its own; turning off a *scheduled* one is
    /// not, because clearing the manual override hands the decision straight
    /// back to the schedule, which is still inside its window and switches
    /// everything on again before the next frame. See
    /// [`FocusAssistManager::auto_suppressed`].
    ///
    /// Asks the rules whether one is in force rather than inferring it from
    /// the mode: "focus assist is on and there is a schedule" is not the same
    /// claim as "the schedule is why", and the difference is a manual mode set
    /// at noon that would otherwise arm a snooze against a schedule that was
    /// not going to fire for ten hours.
    fn snooze_schedule_if_it_would_resume(&mut self, utc_secs: u64) {
        let (hour, minute, weekday) = self.clock_reading_at(utc_secs);
        let in_force = self
            .focus
            .auto_rules
            .iter()
            .any(|rule| rule.is_schedule_active(hour, minute, weekday));
        if !in_force {
            return;
        }
        self.schedule_snooze = self
            .next_schedule_change(utc_secs)
            .map(|d| utc_secs.saturating_add(d.as_secs()));
        // Applied now rather than at the next tick. `auto_active` still holds
        // the last evaluation's answer and `effective_mode` reads it, so
        // without this the switch the user has just pressed redraws as on and
        // the desktop stays quiet until something else happens to tick the
        // loop -- which on an idle desktop at two in the morning is nothing.
        self.evaluate_schedules(utc_secs);
    }

    /// Seconds since the epoch, or 0 on a clock set before it.
    fn unix_now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// The pin menu's first row. Numbered well clear of the desktop menu's
    /// ids, which are a different menu with a different handler.
    const MENU_PIN_TOGGLE: u64 = 900;
    /// The pin menu's "Add to desktop".
    const MENU_ADD_TO_DESKTOP: u64 = 901;
    /// The pin menu's "Pin to Start menu" / "Unpin from Start menu".
    const MENU_START_PIN_TOGGLE: u64 = 902;

    // The desktop menu's item ids. Stable numbers rather than positions, so
    // inserting an item cannot silently reassign what the ones below it do;
    // `the_desktop_menu_ids_are_all_distinct` keeps them apart.
    const MENU_ADD_CLOCK: u64 = 1;
    const MENU_ADD_CALENDAR: u64 = 2;
    const MENU_ADD_SYSTEM_MONITOR: u64 = 3;
    const MENU_REMOVE_WIDGETS: u64 = 4;
    const MENU_REMOVE_ONE_WIDGET: u64 = 5;
    const MENU_AUTO_ARRANGE: u64 = 6;
    const MENU_ALIGN_TO_GRID: u64 = 7;
    const MENU_SORT_BY_NAME: u64 = 8;
    const MENU_ADD_WIDGET_SUBMENU: u64 = 100;
    const MENU_VIEW_SUBMENU: u64 = 101;
    // An icon's own menu, opened by a right-click on the icon.
    const MENU_ICON_OPEN: u64 = 300;
    const MENU_ICON_PIN: u64 = 301;
    const MENU_ICON_REMOVE: u64 = 302;
    const MENU_ICON_RENAME: u64 = 303;
    const MENU_ICON_START_PIN: u64 = 304;
    /// The first icon size's id; the others follow in
    /// [`IconSize::ALL`](appearance::IconSize::ALL)'s order. A block of its
    /// own, far from the rest, so a size added to the setting cannot land on
    /// an id something else already has.
    const MENU_ICON_SIZE_BASE: u64 = 200;

    /// The View submenu's words for an icon size.
    ///
    /// The desktop's own rather than [`appearance::IconSize::label`], which
    /// is the Settings application's "Large (64px)": a menu offering "View >
    /// Large (64px)" reads as a specification, and every desktop with this
    /// menu says "Large icons".
    fn icon_size_menu_label(size: appearance::IconSize) -> &'static str {
        match size {
            appearance::IconSize::Small => "Small icons",
            appearance::IconSize::Medium => "Medium icons",
            appearance::IconSize::Large => "Large icons",
            appearance::IconSize::ExtraLarge => "Extra large icons",
        }
    }

    /// The icon size a menu item id names, if it names one.
    fn menu_icon_size(id: MenuItemId) -> Option<appearance::IconSize> {
        let index = usize::try_from(id.checked_sub(Self::MENU_ICON_SIZE_BASE)?).ok()?;
        appearance::IconSize::ALL.get(index).copied()
    }

    /// The desktop menu's items, the View submenu ticking `icon_size` and
    /// what `arrangement` means for its two switches.
    ///
    /// "Auto arrange icons" and "Align icons to grid" are two switches over
    /// three states -- see [`icons::ArrangementMode`] for why -- so both are
    /// ticked under auto-arrange, which is aligned by construction.
    fn desktop_menu_items(
        icon_size: appearance::IconSize,
        arrangement: icons::ArrangementMode,
    ) -> Vec<MenuItem> {
        let item = |id: u64, label: &str, checked: Option<bool>| MenuItem::Action {
            id,
            label: label.to_string(),
            shortcut: None,
            icon: None,
            enabled: true,
            checked,
        };
        let mut view: Vec<MenuItem> = appearance::IconSize::ALL
            .iter()
            .zip(Self::MENU_ICON_SIZE_BASE..)
            .map(|(&size, id)| {
                item(
                    id,
                    Self::icon_size_menu_label(size),
                    Some(size == icon_size),
                )
            })
            .collect();
        view.push(MenuItem::Separator);
        view.push(item(
            Self::MENU_AUTO_ARRANGE,
            "Auto arrange icons",
            Some(arrangement == icons::ArrangementMode::AutoArrange),
        ));
        view.push(item(
            Self::MENU_ALIGN_TO_GRID,
            "Align icons to grid",
            Some(arrangement.aligns_to_grid()),
        ));
        vec![
            MenuItem::Submenu {
                id: Self::MENU_VIEW_SUBMENU,
                label: "View".to_string(),
                icon: None,
                enabled: true,
                children: view,
            },
            item(Self::MENU_SORT_BY_NAME, "Sort by name", None),
            MenuItem::Separator,
            MenuItem::Submenu {
                id: Self::MENU_ADD_WIDGET_SUBMENU,
                label: "Add widget".to_string(),
                icon: None,
                enabled: true,
                children: vec![
                    item(Self::MENU_ADD_CLOCK, "Clock", None),
                    item(Self::MENU_ADD_CALENDAR, "Calendar", None),
                    item(Self::MENU_ADD_SYSTEM_MONITOR, "System monitor", None),
                ],
            },
            MenuItem::Separator,
            item(Self::MENU_REMOVE_WIDGETS, "Remove all widgets", None),
        ]
    }

    /// The items for a right-click on the icon `id`.
    ///
    /// "Open" and "Rename" always; "Pin to taskbar" (or "Unpin") for a
    /// program; "Remove from desktop" when the selection holds anything the
    /// user added -- the
    /// defaults are not the user's to remove, and offering to would be a door
    /// that does nothing. Each label says the *action*, not the state, the
    /// rule the pin menu set.
    fn icon_menu_items(&self, id: icons::IconId) -> Vec<MenuItem> {
        let item = |id: u64, label: &str| MenuItem::Action {
            id,
            label: label.to_string(),
            shortcut: None,
            icon: None,
            enabled: true,
            checked: None,
        };
        let mut items = vec![item(Self::MENU_ICON_OPEN, "Open")];
        let mut more = vec![item(Self::MENU_ICON_RENAME, "Rename")];
        if let Some(exec) = self.icon_program(id) {
            more.push(item(
                Self::MENU_ICON_PIN,
                if self.is_pinned(&exec) {
                    "Unpin from taskbar"
                } else {
                    "Pin to taskbar"
                },
            ));
            more.push(item(
                Self::MENU_ICON_START_PIN,
                if self.is_pinned_to_start(&exec) {
                    "Unpin from Start menu"
                } else {
                    "Pin to Start menu"
                },
            ));
        }
        let any_added = self
            .icons
            .selected_ids()
            .into_iter()
            .chain(core::iter::once(id))
            .any(|each| self.icons.get_icon(each).is_some_and(|icon| icon.added));
        if any_added {
            more.push(item(Self::MENU_ICON_REMOVE, "Remove from desktop"));
        }
        if !more.is_empty() {
            items.push(MenuItem::Separator);
            items.extend(more);
        }
        items
    }

    /// The program an icon starts, as the taskbar spells one, if it is a
    /// program: an executable that the icon opens by path. A folder or a
    /// document is not something a taskbar button can start.
    fn icon_program(&self, id: icons::IconId) -> Option<String> {
        let icon = self.icons.get_icon(id)?;
        match (&icon.icon_type, &icon.action) {
            (icons::IconType::Executable, icons::IconAction::OpenPath(path)) => {
                // The pinned list is text (`taskbar.yaml`), so a program whose
                // path is not text cannot be pinned -- refused, not flattened
                // into a path that names a different program.
                path.to_str().map(str::to_string)
            }
            _ => None,
        }
    }

    /// The items for a right-click *on a widget*.
    fn widget_menu_items() -> Vec<MenuItem> {
        let add = |id: u64, label: &str| MenuItem::Action {
            id,
            label: label.to_string(),
            shortcut: None,
            icon: None,
            enabled: true,
            checked: None,
        };
        vec![
            add(Self::MENU_REMOVE_ONE_WIDGET, "Remove this widget"),
            MenuItem::Separator,
            add(Self::MENU_REMOVE_WIDGETS, "Remove all widgets"),
        ]
    }

    /// Whether the widget layout needs writing, clearing the flag.
    ///
    /// Taken rather than read so a caller cannot ask twice and save twice.
    pub fn take_widgets_dirty(&mut self) -> bool {
        core::mem::replace(&mut self.widgets_dirty, false)
    }

    /// Keep a rename under way, marking the layout for saving if the name
    /// changed. The session calls this when the keyboard leaves the shell for
    /// another program, which keeps a half-typed name as a click away would.
    pub fn commit_icon_rename(&mut self) {
        self.icons_dirty |= self.icons.commit_rename();
    }

    /// Whether the icon layout needs writing, clearing the flag -- taken
    /// rather than read for the reason
    /// [`take_widgets_dirty`](Self::take_widgets_dirty) is. The session writes
    /// it with [`save_icon_layout`](Self::save_icon_layout).
    pub fn take_icons_dirty(&mut self) -> bool {
        core::mem::take(&mut self.icons_dirty)
    }

    /// Read processor and memory from `/proc`, for the next frame to report.
    ///
    /// Called from the widget-due tick, which fires at the monitor widget's own
    /// interval, rather than from `live_readings` -- that runs once a frame,
    /// and sixty reads of `/proc/stat` a second to answer a question asked once
    /// a second is its own small defect.
    ///
    /// **The first call after start-up measures nothing**, and that is correct
    /// rather than a gap: a processor fraction is a ratio over an interval, and
    /// at the first sample there is no interval yet. `None` until the second
    /// tick is the honest answer, and the meter says "CPU (not measured)" for
    /// that one second.
    pub fn sample_system(&mut self) {
        let fs = procinfo::ProcFs::new();
        self.sampled.memory_fraction = fs
            .memory()
            .ok()
            .flatten()
            .and_then(crate::widgets::SystemSample::memory_used_fraction);
        let now = fs.cpu_stats().ok().flatten().and_then(|c| c.total);
        self.sampled.cpu_fraction = match (self.prev_cpu, now) {
            (Some(prev), Some(now)) => crate::widgets::SystemSample::cpu_busy_fraction(&prev, &now),
            _ => None,
        };
        if let Some(now) = now {
            self.prev_cpu = Some(now);
        }
    }

    /// The readings the widget layer cannot derive for itself.
    ///
    /// The clock's strings come from the same `ClockDisplay` and time zone the
    /// taskbar reads, so a widget clock and the tray clock cannot disagree
    /// about the hour, the format, or the zone -- which they would the moment
    /// either grew a formatter of its own.
    /// Public because a caller that renders the widget layer itself needs the
    /// same readings, and because it is the only way to ask what the layer is
    /// being *told* as distinct from what it draws.
    #[must_use]
    pub fn live_readings(&self) -> crate::widgets::LiveReadings {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let zone = self.local_zone();
        let clock = self.clock();
        crate::widgets::LiveReadings {
            clock_time: clock.format_time(secs, &zone),
            clock_date: clock.format_date(secs, &zone),
            // Reported from the last sample rather than read here: this runs
            // once per frame and the monitor widget is due once per second, so
            // reading `/proc` from here would be sixty file reads for one
            // answer. `sample_system` is called from the widget-due tick.
            cpu_fraction: self.sampled.cpu_fraction,
            memory_fraction: self.sampled.memory_fraction,
            // **No source anywhere in this tree.**
            // `/sys/devices/block/<name>/{sector_count,sector_size}` gives a
            // disk's CAPACITY; nothing reports how much of it is in use, and
            // `procinfo` has no `statfs`. The meter says so rather than
            // showing a plausible fraction of a number we do have.
            disk_fraction: None,
            // `Default`, which is `present: false, state: NoBattery` -- and
            // that is the true answer, not a placeholder. `/proc/battery`
            // reports zero sources because `kernel/src/fs/battery.rs` starts
            // with none and no ACPI driver ever calls `register_source`. When
            // one does, this is the line that changes, and everything
            // downstream of it already works.
            battery: crate::power::BatteryInfo::default(),
        }
    }

    /// The settings group the widget layout lives in.
    /// A rule's size in pixels, or `None` if this build cannot work it out.
    ///
    /// `RememberLast` never reaches here -- `resolve_remembered` has already
    /// turned it into `Exact` or dropped it.
    fn rule_size_px(size: window_rules::SizeSpec, screen: (u32, u32)) -> Option<(u32, u32)> {
        match size {
            window_rules::SizeSpec::Exact { width, height } => Some((width, height)),
            window_rules::SizeSpec::Percentage { w_pct, h_pct } => Some((
                Self::fraction_of(screen.0, w_pct),
                Self::fraction_of(screen.1, h_pct),
            )),
            // Already resolved away; reaching here means the resolution was
            // skipped, and guessing a size would be worse than leaving the
            // window where the program put it.
            window_rules::SizeSpec::RememberLast => None,
        }
    }

    /// A rule's position in pixels, or `None` if this build cannot work it out.
    ///
    /// `size` is the rule's own size, needed only to centre: centring is a
    /// function of how big the window will be, and the rule's size is the size
    /// it is about to be given. A rule that centres without setting a size
    /// cannot be honoured here -- the shell is told window *positions* in the
    /// window list but not the size a program is about to choose -- so it is
    /// declined rather than centred against a guess.
    fn rule_position_px(
        position: window_rules::PositionSpec,
        size: Option<window_rules::SizeSpec>,
        screen: (u32, u32),
    ) -> Option<(i32, i32)> {
        match position {
            window_rules::PositionSpec::Absolute { x, y } => Some((x, y)),
            window_rules::PositionSpec::Percentage { x_pct, y_pct } => Some((
                Self::fraction_of(screen.0, x_pct).cast_signed(),
                Self::fraction_of(screen.1, y_pct).cast_signed(),
            )),
            // Monitor 0 is the display this shell was built for, which is the
            // only one it has bounds for. A rule naming a second monitor is
            // declined rather than centred on the first: putting a window on
            // the wrong screen is a worse answer than leaving it alone, and
            // multi-monitor placement is tracked separately.
            window_rules::PositionSpec::CenterOnMonitor(0) => {
                let (w, h) = Self::rule_size_px(size?, screen)?;
                Some((
                    screen.0.saturating_sub(w).cast_signed() / 2,
                    screen.1.saturating_sub(h).cast_signed() / 2,
                ))
            }
            window_rules::PositionSpec::CenterOnMonitor(_)
            | window_rules::PositionSpec::RememberLast => None,
        }
    }

    /// `pct` of `whole`, clamped to the display and rounded.
    fn fraction_of(whole: u32, pct: f32) -> u32 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to 0.0..=1.0 first, so the product is within the \
                      u32 it was taken from"
        )]
        let scaled = (f64::from(whole) * f64::from(pct.clamp(0.0, 1.0))).round() as u32;
        scaled
    }

    /// The file the user's keyboard shortcuts live in.
    pub const SHORTCUTS_CONFIG_NAME: &'static str = "shortcuts";

    /// Read the saved shortcuts over the defaults.
    ///
    /// Kept out of [`new`](Self::new) for the reason
    /// [`load_widgets`](Self::load_widgets) is: a constructor that reads the
    /// user's home directory gives every test a machine-dependent result.
    ///
    /// **Applied over the default table, not in place of it.** A file written
    /// by an older desktop names the shortcuts that existed then, and replacing
    /// the table with it would silently drop every shortcut added since. The
    /// defaults are the floor; the file moves what it mentions.
    ///
    /// Each saved binding *moves* its action rather than adding a second chord
    /// for it -- otherwise a rebound shortcut would answer to both its old
    /// chord and its new one after a restart, which is not what the user asked
    /// for and is invisible until they press the old one.
    pub fn load_shortcuts(&mut self) {
        let doc = config::load(Self::SHORTCUTS_CONFIG_NAME);
        let Ok(saved) = hotkeys::HotkeyConfig::read_from(&doc) else {
            // An unparseable file is left alone rather than rewritten: the user
            // still has whatever they typed, and the defaults still work.
            return;
        };

        for (chord, action) in saved.bindings() {
            // Every chord the file gives this action, not just this one. An
            // action can legitimately have two -- the defaults put the Start
            // Menu on both Super keys, so that a driver which sets the Super
            // bit and one which does not are both answered -- and the earlier
            // version of this filter, `*h != chord`, unregistered each of them
            // while processing the other. Whichever came last was the only one
            // left, so the Start Menu quietly stopped answering one of the two
            // Super keys after any save and reload.
            let keeps: Vec<_> = saved
                .bindings()
                .iter()
                .filter(|(_, a)| a == action)
                .map(|(h, _)| *h)
                .collect();
            let stale: Vec<_> = self
                .hotkeys
                .all_bindings()
                .filter(|(h, a)| *a == action && !keeps.contains(h))
                .map(|(h, _)| *h)
                .collect();
            for old in stale {
                self.hotkeys.unregister(&old);
            }
            // The file says what this chord does, and that overrules whatever a
            // default put on it -- overruling the default is what a saved
            // binding *is*. `register` refuses a chord another action holds,
            // so without this a chord the user pointed at a different action
            // (F2 on the card) was dropped in silence on the next login, and
            // the tombstone for the action it used to start then unbound it
            // altogether. That could not happen until the card could change
            // what a chord does, which is why nothing noticed.
            if self
                .hotkeys
                .conflicts_with(chord)
                .is_some_and(|held| held != action)
            {
                self.hotkeys.unregister(chord);
            }
            // Cannot fail now: the chord is free, or already this action's,
            // which `register` accepts as a no-op.
            drop(self.hotkeys.register(*chord, action.clone()));
        }

        // Then the deletions. After the bindings, so that a file which both
        // rebinds and unbinds the same action ends with it unbound -- the last
        // thing the user did to it is the thing that stands, and a file cannot
        // say both about one action unless it was hand-edited.
        for action in saved.unbound() {
            let bound: Vec<_> = self
                .hotkeys
                .all_bindings()
                .filter(|(_, a)| *a == action)
                .map(|(h, _)| *h)
                .collect();
            for chord in bound {
                self.hotkeys.unregister(&chord);
            }
        }
    }

    /// Write the shortcuts back.
    ///
    /// # Errors
    ///
    /// If there is no configuration directory, or the file cannot be written.
    pub fn save_shortcuts(&self) -> std::io::Result<()> {
        let mut doc = config::load(Self::SHORTCUTS_CONFIG_NAME);
        hotkeys::HotkeyConfig::from_registry(&self.hotkeys).write_into(&mut doc);
        config::store(Self::SHORTCUTS_CONFIG_NAME, &doc)
    }

    pub const WIDGETS_CONFIG_NAME: &'static str = "widgets";

    /// Read the saved widget layout.
    ///
    /// Kept out of [`new`](Self::new) for the reason
    /// [`load_appearance`](Self::load_appearance) is: a constructor that reads
    /// the user's home directory gives every test a machine-dependent result.
    ///
    /// A missing or unreadable file is not an error -- it is a desktop with no
    /// widgets, which is what a fresh install has.
    pub fn load_widgets(&mut self) {
        self.widgets
            .read_from(&config::load(Self::WIDGETS_CONFIG_NAME));
    }

    /// Write the widget layout back.
    ///
    /// Splices into the document that was on disk rather than replacing it, so
    /// comments and any key a different version of the desktop wrote survive --
    /// the same contract every other settings group here has.
    ///
    /// # Errors
    ///
    /// If there is no configuration directory, or the file cannot be written.
    pub fn save_widgets(&self) -> std::io::Result<()> {
        let mut doc = config::load(Self::WIDGETS_CONFIG_NAME);
        self.widgets.write_into(&mut doc);
        config::store(Self::WIDGETS_CONFIG_NAME, &doc)
    }

    /// Open the desktop menu at a point, closing whatever else was open.
    ///
    /// The item list depends on what is under the pointer: a widget gets a menu
    /// about *that* widget, bare desktop gets the one about the desktop. Built
    /// per opening rather than kept as two menus, because `ContextMenu::new`
    /// measures its panel from the labels and the two lists are different
    /// widths -- one menu reused would keep whichever width it was built with
    /// -- and because the desktop's View submenu ticks the icon size and
    /// arrangement in force now.
    ///
    /// Dismisses first, for the reason every other popup here does: two menus
    /// on screen at once have no rule about which the next click belongs to.
    pub fn open_desktop_menu(&mut self, x: f32, y: f32) {
        self.dismiss_popups();
        self.menu_widget = self.widgets.hit_test(x, y);
        // Widgets are drawn over the icons, so a widget under the pointer is
        // what was clicked even when an icon lies beneath it.
        self.menu_icon = if self.menu_widget.is_some() {
            None
        } else {
            self.icons.icon_at(x, y)
        };
        let items = if self.menu_widget.is_some() {
            Self::widget_menu_items()
        } else if let Some(id) = self.menu_icon {
            // A right-click on an icon that is not selected selects it alone,
            // as a left click would: the menu is about what is selected, and
            // a menu about an icon the user cannot see is selected is a menu
            // about nothing they chose.
            if !self.icons.selected_ids().contains(&id) {
                self.icons.select_single(id);
            }
            self.icon_menu_items(id)
        } else {
            Self::desktop_menu_items(self.appearance.icon_size, self.icons.arrangement())
        };
        self.desktop_menu = ContextMenu::new(items);
        self.desktop_menu.show(x, y, self.viewport());
    }

    /// Begin dragging the widget under `(x, y)`, if there is one.
    ///
    /// Returns whether a drag started, so the caller can stop the press
    /// falling through to the window underneath.
    fn begin_widget_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(id) = self.widgets.hit_test(x, y) else {
            return false;
        };
        let Some(w) = self.widgets.get(id) else {
            return false;
        };
        let g = &self.widgets.grid;
        let (wx, wy) =
            w.position
                .pixels(g.origin_x, g.origin_y, g.cell_width, g.cell_height, g.gap);
        self.widget_drag = Some((id, x - wx, y - wy));
        true
    }

    /// Continue a drag. Returns whether the widget moved.
    fn drag_widget_to(&mut self, x: f32, y: f32) -> bool {
        let Some((id, dx, dy)) = self.widget_drag else {
            return false;
        };
        // The grab offset is subtracted before asking which cell: without it
        // the cell is the one under the *pointer*, so grabbing a widget by its
        // right-hand edge would teleport it a column left on the first move.
        let Some(pos) = self.widgets.pixel_to_grid(x - dx, y - dy) else {
            return false;
        };
        self.widgets.move_widget(id, pos)
    }

    /// Act on a desktop-menu selection.
    ///
    /// Public because a click is not the only way to choose an item: the
    /// keyboard path below routes `MenuAction::Selected` here too, and a future
    /// caller (a shortcut that opens the menu, an accessibility driver) needs
    /// the same door. It takes the id rather than a position for the reason the
    /// ids are constants — a position is only meaningful next to the item list
    /// it indexes.
    ///
    /// Answers a [`ShellAction`] rather than the `bool` it used to, because
    /// an icon's "Open" starts a program and a `bool` has nowhere to put one:
    /// [`Pass`](ShellAction::Pass) is "nothing changed",
    /// [`Consumed`](ShellAction::Consumed) "something did", and a
    /// [`Launch`](ShellAction::Launch) is what to start.
    /// [`ShellAction::changed`] is the old `bool`.
    pub fn activate_desktop_menu_item(&mut self, id: MenuItemId) -> ShellAction {
        if let Some(action) = self.activate_icon_context_item(id) {
            return action;
        }
        let changed = match self.activate_icon_menu_item(id) {
            Some(changed) => changed,
            None => {
                let changed = self.activate_desktop_menu_item_inner(id);
                self.widgets_dirty |= changed;
                changed
            }
        };
        if changed {
            ShellAction::Consumed
        } else {
            ShellAction::Pass
        }
    }

    /// The items of an icon's own menu. `None` for an id that is not one of
    /// them.
    ///
    /// All act on `menu_icon`, the icon the menu was opened over, except
    /// Remove, which acts on the whole selection, as Delete does.
    fn activate_icon_context_item(&mut self, id: MenuItemId) -> Option<ShellAction> {
        if !matches!(
            id,
            Self::MENU_ICON_OPEN
                | Self::MENU_ICON_PIN
                | Self::MENU_ICON_START_PIN
                | Self::MENU_ICON_REMOVE
                | Self::MENU_ICON_RENAME
        ) {
            return None;
        }
        let Some(icon) = self.menu_icon.take() else {
            // The icon went while the menu was open -- a layout reloaded
            // under it. There is nothing left to act on.
            return Some(ShellAction::Pass);
        };
        Some(match id {
            Self::MENU_ICON_OPEN => match self.icons.get_icon(icon).map(|i| i.action.clone()) {
                Some(action) => self.open_icon(icon, &action),
                None => ShellAction::Pass,
            },
            Self::MENU_ICON_RENAME => {
                // A rename already under way is kept first, and its change
                // saved -- see `begin_rename`.
                self.icons_dirty |= self.icons.commit_rename();
                if self.icons.begin_rename(icon) {
                    ShellAction::Consumed
                } else {
                    ShellAction::Pass
                }
            }
            Self::MENU_ICON_PIN => match self.icon_program(icon) {
                Some(exec) => {
                    if self.is_pinned(&exec) {
                        self.unpin_app(&exec);
                    } else {
                        let name = self
                            .icons
                            .get_icon(icon)
                            .map_or_else(|| exec.clone(), |i| i.label.clone());
                        self.pin_app(&exec, &name);
                    }
                    ShellAction::Consumed
                }
                None => ShellAction::Pass,
            },
            Self::MENU_ICON_START_PIN => match self.icon_program(icon) {
                Some(exec) => {
                    self.toggle_start_pin(&exec);
                    ShellAction::Consumed
                }
                None => ShellAction::Pass,
            },
            _ => {
                let selected = self.icons.selected_ids();
                if self.icons.remove_added(&selected) > 0 {
                    self.icons_dirty = true;
                    ShellAction::Consumed
                } else {
                    ShellAction::Pass
                }
            }
        })
    }

    /// The items about the desktop's icons: the View submenu and "Sort by
    /// name". `None` for an id that is not one of them; otherwise whether the
    /// icon layout changed, which is when it needs saving.
    ///
    /// Kept apart from the widget items so that choosing one does not mark
    /// the *widget* layout dirty and rewrite a file nothing changed in.
    ///
    /// The two switches map onto [`icons::ArrangementMode`]'s three states
    /// the way every desktop with both does: auto-arrange implies alignment,
    /// so turning it on aligns, turning it off leaves the icons aligned where
    /// they are, and turning alignment off stops arranging too.
    fn activate_icon_menu_item(&mut self, id: MenuItemId) -> Option<bool> {
        use icons::ArrangementMode as Mode;
        let current = self.icons.arrangement();
        let changed = match id {
            Self::MENU_AUTO_ARRANGE => {
                self.icons.set_arrangement(if current == Mode::AutoArrange {
                    Mode::SnapToGrid
                } else {
                    Mode::AutoArrange
                })
            }
            Self::MENU_ALIGN_TO_GRID => self.icons.set_arrangement(if current.aligns_to_grid() {
                Mode::Free
            } else {
                Mode::SnapToGrid
            }),
            Self::MENU_SORT_BY_NAME => self.icons.arrange_by_name(),
            // `choose_icon_size` marks the layout itself.
            _ => return Self::menu_icon_size(id).map(|size| self.choose_icon_size(size)),
        };
        self.icons_dirty |= changed;
        Some(changed)
    }

    fn activate_desktop_menu_item_inner(&mut self, id: MenuItemId) -> bool {
        let kind = match id {
            Self::MENU_ADD_CLOCK => Some(WidgetKind::Clock),
            Self::MENU_ADD_CALENDAR => Some(WidgetKind::Calendar),
            Self::MENU_ADD_SYSTEM_MONITOR => Some(WidgetKind::SystemMonitor),
            Self::MENU_REMOVE_ONE_WIDGET => {
                // `menu_widget` rather than a fresh hit test: see the field.
                return self
                    .menu_widget
                    .take()
                    .is_some_and(|id| self.widgets.remove_widget(id));
            }
            Self::MENU_REMOVE_WIDGETS => {
                let had = self.widgets.count() > 0;
                for w in self
                    .widgets
                    .all_widgets()
                    .iter()
                    .map(|w| w.id)
                    .collect::<Vec<_>>()
                {
                    self.widgets.remove_widget(w);
                }
                return had;
            }
            _ => None,
        };
        // `find_free_position` rather than the click point: a widget dropped
        // where the pointer happened to be would overlap whatever is already
        // there, and the grid exists to stop that.
        if let Some(kind) = kind
            && let Some(pos) = self.widgets.find_free_position(kind.default_size())
        {
            return self.widgets.add_widget(kind, pos).is_some();
        }
        false
    }

    /// The desktop menu's draw commands, empty when it is closed.
    #[must_use]
    pub fn render_desktop_menu(&self) -> Option<RenderTree> {
        if !self.desktop_menu.is_visible() {
            return None;
        }
        let mut tree = RenderTree::new();
        tree.commands.extend(
            self.desktop_menu
                .render(&Palette::from_settings(&self.appearance)),
        );
        Some(tree)
    }

    /// The overflow list's draw commands, empty when it is closed.
    #[must_use]
    pub fn render_tray_overflow(&self) -> Option<RenderTree> {
        let (menu, _) = self.tray_overflow_menu.as_ref()?;
        let mut tree = RenderTree::new();
        tree.commands
            .extend(menu.render(&Palette::from_settings(&self.appearance)));
        Some(tree)
    }

    /// The widget layer's draw commands, for the *background* surface.
    ///
    /// Separate from [`render_desktop_menu`](Self::render_desktop_menu)
    /// because they go to different surfaces: widgets sit on the desktop and
    /// windows cover them, while the menu is a popup over everything.
    #[must_use]
    pub fn render_widgets(&self) -> Vec<guitk::render::RenderCommand> {
        // Derived from the settings rather than kept on `DesktopTheme`, which
        // holds *roles* (taskbar background, title-bar foreground) rather than
        // the palette they were chosen from -- and a widget panel is not any of
        // those roles. Same call `render_notifications` makes, a few methods
        // down, for the same reason.
        self.widgets.render(
            &Palette::from_settings(&self.appearance),
            &self.live_readings(),
        )
    }

    /// The icon layer's draw commands.
    ///
    /// Palette built here rather than held, the same reason
    /// [`render_widgets`](Self::render_widgets) gives: `appearance` is the one
    /// source of truth and a cached palette is a second one that goes stale
    /// the moment the user switches mode.
    pub fn render_icons(&self) -> Vec<guitk::render::RenderCommand> {
        let p = Palette::from_settings(&self.appearance);
        match self.icons.drag_in_progress() {
            // Let go over the taskbar, the icons stay where they are (and a
            // program among them is pinned), so an outline of where they
            // would land on the desktop would be a promise the drop breaks.
            Some((at, _)) if self.taskbar_rect().contains(at.0, at.1) => {
                self.icons.render_dropping_elsewhere(&p)
            }
            _ => self.icons.render(&p),
        }
    }

    /// Put the default icons on the desktop and lay them out as they were
    /// last left -- positions and arrangement both.
    ///
    /// The order is load-bearing: positions are filed against the icons that
    /// exist, so nothing can be restored before the icons are there to restore.
    pub fn populate_icons(&mut self) {
        self.icons.populate_defaults();
        self.icons.load_layout();
    }

    /// Write the icon layout back: every icon's position, the grid they are
    /// on and the arrangement.
    ///
    /// Called by the session when [`take_icons_dirty`](Self::take_icons_dirty)
    /// says something changed rather than on every frame: the layout only
    /// changes when the user moves something or chooses from the View menu,
    /// and a save per frame would rewrite the file sixty times a second to
    /// record that nothing happened.
    ///
    /// The failure is handed back rather than swallowed here, because this
    /// object has nowhere to say it -- the surface that can tell the user is
    /// the session.
    pub fn save_icon_layout(&self) -> std::io::Result<()> {
        self.icons.save_layout()
    }

    /// A double-click, which only the desktop icons act on.
    ///
    /// Anything that is not the bare desktop is handed to
    /// [`handle_press`](Self::handle_press), which is what the two used to
    /// share unconditionally: every other surface this shell draws still treats
    /// a second click as another first one.
    fn handle_icon_activate(&mut self, x: f32, y: f32, button: MouseButton) -> ShellAction {
        if !matches!(self.hit_test(x, y), Hit::Desktop) || button != MouseButton::Left {
            return self.handle_press(x, y, button);
        }
        match self.icons.handle_double_click(x, y) {
            icons::IconEvent::Activate(id, action) => self.open_icon(id, &action),
            // A double-click on empty desktop. The first click already went
            // through `handle_press`, so the layer's state is settled either
            // way and there is nothing further to do.
            _ => ShellAction::Consumed,
        }
    }

    /// A key pressed while the desktop itself has the keyboard -- the bare
    /// desktop was the last thing clicked -- that no shortcut and no open
    /// surface took. These are the icons' keys:
    ///
    /// - **Enter** opens the selected icon, when exactly one is selected;
    /// - **Ctrl+A** selects every icon, **Escape** selects none;
    /// - the **arrow keys** move the selection to the nearest icon that way.
    ///
    /// The session calls this only for a key that arrived on the desktop's own
    /// surface, so Enter typed into the taskbar's search, say, never opens an
    /// icon. Until 2026-09-25 nothing called the icon layer's key handler at
    /// all, and every one of these keys did nothing on the desktop.
    ///
    /// - **Delete** takes the selected shortcuts the user added off the
    ///   desktop; the defaults stay, as they would come back at the next login.
    ///
    /// - **F2** renames the one selected icon, in place: Enter keeps the new
    ///   name, Escape the old one, and a click away keeps it.
    ///
    /// `Pass` for any key that changed nothing, so the frame is not repainted
    /// for it.
    pub fn handle_desktop_key(&mut self, key: &KeyEvent) -> ShellAction {
        if !key.pressed {
            return ShellAction::Pass;
        }
        // While a name is being edited every key is the field's, so that a
        // Delete meant for a letter cannot remove the icon being renamed.
        if self.icons.renaming().is_some() {
            if let icons::RenameKey::Finished { renamed } = self.icons.rename_key(key) {
                self.icons_dirty |= renamed;
            }
            return ShellAction::Consumed;
        }
        let ctrl = key.modifiers.ctrl;
        let desktop_key = match key.key {
            Key::Enter => icons::DesktopKey::Enter,
            Key::Escape => icons::DesktopKey::Escape,
            Key::Delete => icons::DesktopKey::Delete,
            Key::F2 => icons::DesktopKey::F2,
            Key::A if ctrl => icons::DesktopKey::SelectAll,
            Key::Up => icons::DesktopKey::Arrow(icons::Direction::Up),
            Key::Down => icons::DesktopKey::Arrow(icons::Direction::Down),
            Key::Left => icons::DesktopKey::Arrow(icons::Direction::Left),
            Key::Right => icons::DesktopKey::Arrow(icons::Direction::Right),
            _ => return ShellAction::Pass,
        };
        let before = self.icons.selected_ids();
        match self.icons.handle_key(desktop_key, ctrl) {
            icons::IconEvent::Activate(id, action) => self.open_icon(id, &action),
            icons::IconEvent::Delete(ids) if self.icons.remove_added(&ids) > 0 => {
                self.icons_dirty = true;
                ShellAction::Consumed
            }
            icons::IconEvent::BeginRename(id) if self.icons.begin_rename(id) => {
                ShellAction::Consumed
            }
            _ if self.icons.selected_ids() != before => ShellAction::Consumed,
            _ => ShellAction::Pass,
        }
    }

    /// Open what a desktop icon names: the double-click, and Enter.
    ///
    /// Until 2026-09-25 this handed an icon's path to be *executed*, which is
    /// right for a program and wrong for everything else on a desktop: the
    /// Documents and Home icons asked the operating system to run a folder,
    /// and "This PC" and "Recycle Bin" did nothing at all, because nothing
    /// said what their destinations were. See [`open_path`](Self::open_path)
    /// for the rules, which are the file manager's.
    fn open_icon(&mut self, id: icons::IconId, action: &icons::IconAction) -> ShellAction {
        let label = self
            .icons
            .get_icon(id)
            .map_or_else(String::new, |icon| icon.label.clone());
        match action {
            icons::IconAction::OpenPath(path) => self.open_path(path, &label),
            icons::IconAction::LaunchSystem(what) if what == icons::THIS_PC => {
                // The machine's files, from the top: the nearest thing this
                // system has to a drive list, and a destination that exists
                // on every install.
                self.open_path(Path::new("/"), &label)
            }
            icons::IconAction::LaunchSystem(what) if what == icons::RECYCLE_BIN => {
                // The bin exists -- the file manager moves files into it and
                // restores them -- but nothing can show what is in it: the
                // file manager has no view of it, and pointing it at the bin's
                // storage would list internal entry folders named by ids.
                // Said rather than faked. `requests/c-e-the-recycle-bin-icon-
                // has-nowhere-to-open.md` asks lane E for the view.
                self.say_cannot_open(
                    &label,
                    "Nothing can show the recycle bin's contents yet. What is \
                     in it is kept, in the .recycle folder in your home \
                     folder, until something can.",
                );
                ShellAction::Consumed
            }
            // A destination this build does not know -- a layout written by a
            // newer desktop -- or an application-defined action with no
            // handler here. Consumed rather than passed on: the click did land
            // on an icon, and handing it to whatever is underneath would be
            // worse than saying nothing.
            icons::IconAction::LaunchSystem(_) | icons::IconAction::Custom(_) => {
                self.say_cannot_open(&label, "This desktop does not know what it opens.");
                ShellAction::Consumed
            }
        }
    }

    /// Open `path` the way the file manager opens what is double-clicked in
    /// it, so that the desktop and the file manager cannot disagree about
    /// what a file opens in:
    ///
    /// - a **folder** opens in the file manager ([`launcher::FILE_MANAGER`]);
    /// - a **file** whose kind has a program chosen for it in File
    ///   Associations opens in that program, the association read afresh
    ///   on every open as the file manager does -- a choice made a moment ago
    ///   in another window counts;
    /// - a file with no such program that is itself **executable** runs --
    ///   which is what a program shortcut on the desktop is;
    /// - anything else says why it cannot be opened, in a notification,
    ///   rather than doing nothing where the user cannot see why.
    ///
    /// The association comes first so that a document on a disk that marks
    /// every file executable -- a USB stick, most network shares -- opens in
    /// its program rather than being run.
    fn open_path(&mut self, path: &Path, label: &str) -> ShellAction {
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(err) => {
                let why = if err.kind() == std::io::ErrorKind::NotFound {
                    format!("{} is not there any more.", path.display())
                } else {
                    format!("{} cannot be read: {err}", path.display())
                };
                self.say_cannot_open(label, &why);
                return ShellAction::Consumed;
            }
        };
        if meta.is_dir() {
            return ShellAction::Launch(hotkeys::Launch::opening(launcher::FILE_MANAGER, path));
        }
        // `to_str` rather than bytes, as the file manager does: associations
        // are keys in a YAML document, so an extension that is not text could
        // never match one, and answering "none" is a refusal rather than a
        // lossy match.
        let chosen = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| {
                associations::program_for(&config::load(associations::CONFIG_NAME), ext)
            });
        if let Some(program) = chosen {
            return ShellAction::Launch(hotkeys::Launch::opening(program, path));
        }
        if is_executable(&meta) {
            return ShellAction::Launch(hotkeys::Launch::program(path));
        }
        self.say_cannot_open(
            label,
            "Nothing is set to open files of this kind. Choose a program for it \
             in File Associations.",
        );
        ShellAction::Consumed
    }

    /// Tell the user that `what` could not be opened, and why.
    ///
    /// A notification rather than nothing: a double-click that visibly does
    /// nothing is the one outcome a user cannot learn anything from. The pane
    /// is not opened -- this explains, it does not interrupt.
    fn say_cannot_open(&mut self, what: &str, why: &str) {
        let title = if what.is_empty() {
            "Cannot open this".to_string()
        } else {
            format!("Cannot open {what}")
        };
        // The id is discarded: this is a message, not something to update.
        let _ = self.notify(notif_pane::Notification {
            id: 0,
            app_name: "Desktop".to_string(),
            title,
            body: why.to_string(),
            timestamp: Self::unix_now(),
            priority: notif_pane::NotifPriority::Normal,
            read: false,
            action: None,
            silent: false,
        });
    }

    /// Whether any of the shell's own surfaces is open over the desktop.
    ///
    /// Named separately from [`dismiss_popups`](Self::dismiss_popups), which
    /// used to compute it inline, because it answers a second question the
    /// session asks: whether the shell should be holding a grab on Escape. The
    /// two must agree — an Escape grab held while nothing is open would take the
    /// key away from every dialog on the desktop, and one *not* held while a
    /// menu is up means the menu cannot be closed from inside an application.
    /// One expression, so they cannot drift apart.
    #[must_use]
    pub fn any_popup_open(&self) -> bool {
        self.desktop_menu.is_visible()
            || self.tray_overflow_menu.is_some()
            || self.start_menu_open
            || self.power_menu_open
            || self.calendar.visible
            || self.notifications.pane_state().is_visible()
            || self.snap.is_overlay_visible()
            || self.overview.visible
            || self.run_dialog.is_visible()
            || self.shortcut_card_open
    }

    /// Close whatever popup is open. Returns whether anything was.
    ///
    /// The return value is what keeps Escape from being swallowed: a press
    /// with nothing open must reach the focused window, whose own dialog may
    /// be what the user meant to dismiss.
    pub fn dismiss_popups(&mut self) -> bool {
        let any = self.any_popup_open();
        self.desktop_menu.hide();
        self.tray_overflow_menu = None;
        // Through the one exit, which also ends a drag from the menu: Escape
        // in the middle of carrying a program used to close the menu and
        // leave the drag to finish on the release.
        self.close_start_menu();
        self.calendar.set_visible(false);
        // The pane's own Escape handling closes it too; this is the path for a
        // press that dismissed something else at the same time, and for a
        // caller dismissing everything without a key at all.
        self.notifications.hide();
        self.snap.hide_overlay();
        // Escape closes the overview along with everything else. It is a
        // fullscreen overlay that covers the whole desktop, so it is the
        // *most* important thing on this list to be able to get out of: with
        // no binding for it, the only way back would be the same chord that
        // opened it, and a user who does not remember what that was is left
        // looking at a screen they cannot dismiss.
        self.overview.hide();
        self.shortcut_card_open = false;
        // Guarded, unlike every other line here. `RunDialog::hide` posts a
        // `Closed` event unconditionally, and this function is called on every
        // Escape and on every opening of the box itself — so an unguarded call
        // would push one event per dismissal into a queue that, with the box
        // shut, nothing drains.
        if self.run_dialog.is_visible() {
            self.run_dialog.hide();
            // Emptied straight away for the same reason. Nothing here can have
            // produced an `Execute`: the box was closed without being asked to
            // run anything, so the drained list is discarded rather than
            // returned, and this function keeps its `bool`.
            drop(self.drain_run_dialog());
        }
        // After the drain, not before it. The drain is what acts on a pending
        // `Browse`, so draining first and closing second is what guarantees a
        // Browse the user asked for a moment before the box was dismissed does
        // not leave a chooser standing over a box that is no longer there.
        self.close_run_browser();
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
        tree.commands.extend(self.calendar.render(
            &Palette::from_settings(&self.appearance),
            x,
            y,
            self.calendar_scale(),
            now,
            &self.events,
        ));
        Some(tree)
    }

    /// How tall the notification pane — scrim included — is allowed to be.
    ///
    /// The taskbar's top edge, not the display's bottom. The pane is opened
    /// from a button *on* the taskbar, and a panel that covers the button that
    /// opened it is a panel the user cannot close the way they opened it: the
    /// second press lands on the pane's own opaque column and does nothing.
    /// Stopping above the bar leaves the bell reachable, which is what makes it
    /// a toggle rather than a one-way door.
    ///
    /// The same number goes to [`NotificationPane::render`] and to
    /// [`NotificationPane::handle_mouse_event`] — one source, so the pane is
    /// hit-tested exactly where it is painted.
    ///
    /// [`NotificationPane::render`]: notif_pane::NotificationPane::render
    /// [`NotificationPane::handle_mouse_event`]: notif_pane::NotificationPane::handle_mouse_event
    #[must_use]
    pub fn notification_pane_height(&self) -> f32 {
        self.taskbar_rect().y
    }

    /// Put a heads-up overlay on screen, or refresh the one already there.
    ///
    /// Takes no clock: it is stamped from
    /// `osd_clock_ms`, which is why the volume keys can
    /// be handled by [`handle_hotkey`](Self::handle_hotkey) — a keystroke
    /// handler that had to be told the time would have to be told it by all
    /// forty of its callers.
    pub fn show_osd(&mut self, kind: osd::OsdKind) {
        self.sync_osd_screen();
        self.osd.show(kind, self.osd_clock_ms);
    }

    /// Advance the overlay clock by one frame's worth of time and expire
    /// whatever that made due.
    ///
    /// Saturating, so the 49-day frame that a debugger produces puts every
    /// overlay at its end rather than wrapping the clock backwards and pinning
    /// them on screen for ever.
    pub fn advance_osd(&mut self, dt_ms: u64) {
        self.osd_clock_ms = self.osd_clock_ms.saturating_add(dt_ms);
        self.osd.tick(self.osd_clock_ms);
        // The tooltip rides the same clock rather than bringing its own. Two
        // clocks advanced from two call sites is one forgotten call away from
        // a tooltip that never appears, or never leaves.
        if let Some((_, tip)) = self.tray_tooltip.as_mut() {
            tip.tick(self.osd_clock_ms);
        }
    }

    /// Re-seed the overlay manager's idea of how big the display is.
    ///
    /// Pull-on-use, exactly as [`sync_snap_area`](Self::sync_snap_area) is and
    /// for the same reason: `screen_width` and `screen_height` are public
    /// fields that anything may assign, so a push-on-change scheme would be one
    /// forgotten call site away from centring an overlay on a screen size that
    /// no longer exists.
    #[allow(
        clippy::cast_precision_loss,
        reason = "screen dimensions are far inside f32's exact-integer range"
    )]
    fn sync_osd_screen(&mut self) {
        self.osd.screen_width = self.screen_width as f32;
        self.osd.screen_height = self.screen_height as f32;
    }

    /// Tell the notification pane how tall the screen is.
    ///
    /// The pane's own scroll bound is `content - viewport`, so a wrong
    /// viewport is a wrong bound in both directions: too small a screen and
    /// the rows past the fold are unreachable, too large and the list scrolls
    /// into blank space past its own end.
    #[allow(
        clippy::cast_precision_loss,
        reason = "a display dimension is exact in f32 for every size hardware produces"
    )]
    fn sync_notification_screen(&mut self) {
        self.notifications
            .set_screen_height(self.screen_height as f32);
    }

    /// Render the heads-up overlays, if any are showing.
    #[must_use]
    pub fn render_osd(&self) -> Option<RenderTree> {
        if !self.osd.has_visible() {
            return None;
        }
        let mut tree = RenderTree::new();
        tree.commands
            .extend(self.osd.render(&Palette::from_settings(&self.appearance)));
        Some(tree)
    }

    /// Render the Run box, if it is open.
    ///
    /// On the popup surface rather than the overlay one, unlike
    /// [`render_osd`](Self::render_osd): the box exists to be typed into and
    /// clicked on, and the overlay surface is the one that declines the mouse
    /// (`design-decisions.md` 566).
    #[must_use]
    pub fn render_run_dialog(&self) -> Option<RenderTree> {
        if !self.run_dialog.is_visible() {
            return None;
        }
        let mut tree = RenderTree::new();
        tree.commands.extend(
            self.run_dialog
                .render(&Palette::from_settings(&self.appearance)),
        );
        Some(tree)
    }

    /// Render the Run box's file chooser, if it is up.
    ///
    /// Separate from [`render_run_dialog`](Self::render_run_dialog) rather than
    /// appended to it, so that the host's paint order says out loud that the
    /// chooser is over the box — the same thing the input routing in
    /// `handle_mouse_inner` says. A chooser folded
    /// into the box's tree would be over it by accident of ordering inside one
    /// `Vec`, which is a fact nothing outside this file could see.
    ///
    /// Translated rather than laid out in place: [`FileDialog::frame`] draws
    /// from its own origin, and `run_browser_rect` is the one place that says
    /// where that origin is on screen.
    ///
    /// [`FileDialog::frame`]: guitk::dialog::FileDialog::frame
    #[must_use]
    pub fn render_run_browser(&self) -> Option<RenderTree> {
        let dialog = self.run_browser.as_ref()?;
        let (x, y, width, height) = self.run_browser_rect();
        let mut tree = RenderTree::new();
        tree.translate(x, y);
        tree.commands.extend(dialog.render(
            &Palette::from_settings(&self.appearance),
            width,
            height,
        ));
        tree.untranslate();
        Some(tree)
    }

    /// Render the notification pane, if it is open.
    #[must_use]
    pub fn render_notifications(&self) -> Option<RenderTree> {
        if !self.notifications.pane_state().is_visible() {
            return None;
        }
        let mut tree = RenderTree::new();
        tree.commands.extend(self.notifications.render(
            &Palette::from_settings(&self.appearance),
            self.screen_width as f32,
            self.notification_pane_height(),
        ));
        Some(tree)
    }

    /// Render the card listing every shortcut, if it is open.
    ///
    /// Centred on the display rather than anchored to the taskbar: it is a
    /// reference the user reads, not a control they act on, and the taskbar
    /// corner is a long way from where their eyes are.
    ///
    /// The size comes from [`hotkeys::settings_panel_size`] — the same function
    /// the renderer itself uses, given the same height budget — so the card is
    /// centred on its real size however many bindings the user has added, and
    /// folds into a second column rather than off the bottom of the screen.
    #[must_use]
    pub fn render_shortcut_card(&self) -> Option<RenderTree> {
        if !self.shortcut_card_open {
            return None;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "screen dimensions are far inside f32's exact-integer range"
        )]
        let (screen_w, screen_h) = (self.screen_width as f32, self.screen_height as f32);
        // Bound once and handed to every call below, because the size, the
        // drawing and the editor's paging only agree if they are given the
        // same budget -- the functions say so in their own docs.
        let budget = self.shortcut_card_budget();
        let p = Palette::from_settings(&self.appearance);
        let mut tree = RenderTree::new();

        // Choosing an action, or typing a command: the card shows the action
        // list instead of the bindings, at the same place a card is centred.
        if self.shortcut_editor.is_picking() {
            let (width, height) = self.shortcut_editor.picker_size(budget);
            let x = ((screen_w - width) / 2.0).max(0.0);
            let y = ((screen_h - height) / 2.0).max(0.0);
            self.shortcut_editor.render_picker(
                &mut tree,
                &self.hotkeys,
                &p,
                x,
                y,
                budget,
                self.shortcut_context(),
                self.appearance.caret_width(),
            );
            self.push_shortcut_message(&mut tree, &p, x, y, width, height);
            return Some(tree);
        }

        let (width, height) = hotkeys::settings_panel_size(&self.hotkeys, budget);
        // Clamped at zero so a display narrower or shorter than the card puts
        // its top-left corner on screen rather than off it: a card that
        // overflows is one the user can still read the start of, whereas one
        // centred at a negative origin is one whose header is gone.
        let x = ((screen_w - width) / 2.0).max(0.0);
        let y = ((screen_h - height) / 2.0).max(0.0);
        tree.commands.extend(hotkeys::render_settings_panel(
            &self.hotkeys,
            &p,
            x,
            y,
            // The row the keyboard is on. Clamped rather than trusted: the
            // registry can shrink under a stored index if an action is
            // unregistered while the card is shut, and a highlight drawn past
            // the last row is a highlight on nothing.
            Some(
                self.shortcut_editor
                    .selected()
                    .min(self.hotkeys.len().saturating_sub(1)),
            ),
            budget,
        ));
        self.push_shortcut_message(&mut tree, &p, x, y, width, height);
        Some(tree)
    }

    /// The card's bottom line: what the last edit did, or -- on the plain list
    /// with nothing to report -- the keys the card answers.
    ///
    /// The outcome has been composed on every edit since the editor was written
    /// -- "Press the new keys", "Unchanged", "Ctrl+Alt+T is now Terminal", and
    /// the one that matters most, "...but could not be saved" -- and until
    /// 2026-09-14 nothing drew any of it, so a rebind was silent whether it
    /// worked, was refused, or worked and failed to persist.
    ///
    /// Drawn here rather than by the card renderers: the message is the
    /// *shell's* record of what its editor just did, not a fact about the
    /// registry or the action list.
    fn push_shortcut_message(
        &self,
        tree: &mut RenderTree,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let (text, color) = match self.shortcut_editor.message() {
            // `subtext0` and not the accent: an outcome, not an invitation, and
            // the accent is what the card already uses for where the keyboard is.
            Some(message) => (message.to_string(), p.subtext0),
            None if !self.shortcut_editor.owns_keyboard() => {
                (SHORTCUT_CARD_HINT.to_string(), p.subtext0)
            }
            None => return,
        };
        tree.push(guitk::render::RenderCommand::Text {
            x: x + SHORTCUT_MESSAGE_INSET,
            y: y + height - SHORTCUT_MESSAGE_INSET,
            text,
            color,
            font_size: self.font_size(TextRole::Body),
            font_weight: guitk::render::FontWeightHint::Regular,
            max_width: Some((width - SHORTCUT_MESSAGE_INSET * 2.0).max(0.0)),
            overflow: guitk::render::TextOverflow::Ellipsis,
        });
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
        // Resolved once for all three renderers, and here rather than cached
        // beside `theme` for the reason `render_overview` documents: the
        // settings are the single source of truth and a stored derivation of
        // them is a second thing that can go stale.
        let p = Palette::from_settings(&self.appearance);
        tree.commands.extend(self.snap.render_overlay(&p));
        if let Some(zone) = self.snap.hovered_zone() {
            tree.commands
                .extend(self.snap.render_zone_highlight(&p, zone));
        }
        tree.commands.extend(self.snap.render_picker(&p));
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
        // Resolved here rather than cached beside `theme`, for the reason
        // `theme` documents: `appearance` is the single source of truth, and a
        // second stored derivation of it is a second thing that can go stale.
        tree.commands.extend(overview::render_overview(
            &self.overview,
            &self.overview_config,
            &Palette::from_settings(&self.appearance),
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

    /// **The icon-size setting reaches the icons.**
    ///
    /// It had a working control, was saved and restored at login, and nothing
    /// drew icons at any size but 32 pixels -- `known-issues.md`
    /// TD-C-FOUR-APPEARANCE-SETTINGS-HAVE-A-WORKING-CONTROL-AND-NO-READER.
    #[test]
    fn the_icon_size_setting_reaches_the_desktop_icons() {
        let mut shell = DesktopShell::new(1920, 1080);
        assert_eq!(
            shell.icons.icon_px(),
            AppearanceSettings::default().icon_size.pixels(),
            "a new shell draws what the default settings say"
        );
        for size in [
            appearance::IconSize::Small,
            appearance::IconSize::Large,
            appearance::IconSize::ExtraLarge,
        ] {
            shell.set_appearance(AppearanceSettings {
                icon_size: size,
                ..AppearanceSettings::default()
            });
            assert_eq!(shell.icons.icon_px(), size.pixels(), "{size:?}");
        }
    }

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
    fn a_save_in_another_process_reaches_a_running_shell() {
        // The half of `TD-APPEARANCE-SETTINGS-ARE-NEVER-WRITTEN-TO-DISK` and
        // `TD-THREE-INDEPENDENT-APPEARANCE-MODELS` that was still open: the
        // Settings application writes `appearance.yaml`, nothing tells the
        // desktop, and the two agreed only across a restart. `apps/settings`
        // already proves a click reaches the file
        // (`test_a_click_that_changes_an_accent_reaches_the_file`); this is
        // the other end of the same wire -- the file reaching a shell that is
        // already running.
        settingsfile::testing::with_scratch_config("desktop-appearance-live", |_root| {
            let mut shell = DesktopShell::new(1920, 1080);
            shell.load_appearance();
            let before = shell.appearance.accent_color;
            let before_theme = shell.theme.taskbar_accent;

            // Another process saves a different accent. Written through the
            // real `AppearanceFile::save`, so this is the same rename-over-the
            // -target path the Settings app takes, not a hand-written file.
            let mut file = appearance::AppearanceFile::load();
            let after = if before == AccentColor::Teal {
                AccentColor::Mauve
            } else {
                AccentColor::Teal
            };
            file.settings.accent_color = after;
            file.save().expect("save");

            assert!(shell.poll_appearance(), "the shell should notice the save");
            assert_eq!(shell.appearance.accent_color, after);
            assert_ne!(
                shell.theme.taskbar_accent, before_theme,
                "the derived palette must follow, not just the stored setting"
            );

            assert!(
                !shell.poll_appearance(),
                "and must not keep reporting the same change"
            );
        });
    }

    #[test]
    fn a_shell_that_polls_without_a_save_does_not_repaint() {
        // The cost side of the same feature. `poll_appearance` is meant to be
        // safe to call on a timer, which it is only if a quiet file answers
        // `false` -- otherwise every tick re-derives the palette and the
        // desktop repaints once a second forever.
        settingsfile::testing::with_scratch_config("desktop-appearance-quiet", |_root| {
            let mut shell = DesktopShell::new(1920, 1080);
            shell.load_appearance();
            for _ in 0..5 {
                assert!(!shell.poll_appearance());
            }
        });
    }

    #[test]
    fn a_save_that_changes_nothing_does_not_disturb_the_shell() {
        // Opening Settings and closing it re-saves the file: `store` renames a
        // fresh temporary over the target, so the inode and its timestamp
        // change even though the bytes do not. A watcher keyed on the
        // timestamp would repaint the desktop every time somebody looked at
        // their settings. This is the assertion that ours is not.
        settingsfile::testing::with_scratch_config("desktop-appearance-resave", |_root| {
            let mut shell = DesktopShell::new(1920, 1080);
            shell.load_appearance();

            let mut file = appearance::AppearanceFile::load();
            file.save().expect("first save");
            let _ = shell.poll_appearance();
            file.save().expect("identical re-save");

            assert!(
                !shell.poll_appearance(),
                "an identical re-save is not an appearance change"
            );
        });
    }

    #[test]
    fn the_startup_load_and_the_reload_read_the_same_file() {
        // `load_appearance` and `poll_appearance` go through one watcher on
        // purpose. If startup read the file directly and *then* a watcher were
        // constructed, a save landing between the two would be recorded as
        // already-seen and never reported -- the desktop would sit on stale
        // settings until some later, unrelated change. Asserting it by having
        // the startup load itself pick up a file written beforehand.
        settingsfile::testing::with_scratch_config("desktop-appearance-startup", |_root| {
            let mut file = appearance::AppearanceFile::load();
            file.settings.accent_color = AccentColor::Peach;
            file.save().expect("save before the shell exists");

            let mut shell = DesktopShell::new(1920, 1080);
            shell.load_appearance();
            assert_eq!(shell.appearance.accent_color, AccentColor::Peach);
            assert!(
                !shell.poll_appearance(),
                "startup consumed it, so there is nothing left to report"
            );
        });
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
        ShellRequest, TextRole, WindowId, WindowInfo, WindowList, WindowState, hotkeys, snap, text,
        window_rules,
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
        // Round-tripped for the same reason, and with a sharper edge: a helper
        // that dropped it would leave every window anonymous after the first
        // list, so a test that opened a named window and then did anything at
        // all would see the name vanish for reasons nothing in the test says.
        info.app_id.clone_from(&window.app_id);
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
            text: String::new(),
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
    // The grab list against the binding table
    // ==================================================================

    /// A claimed chord that means nothing is a key taken from every application
    /// on the desktop in exchange for nothing at all. The compositor cannot
    /// notice — a grab is a grab — so it has to be noticed here.
    #[test]
    fn every_grabbed_chord_is_a_chord_the_shell_acts_on() {
        let shell = shell();
        for (key, modifiers) in shell.global_chords() {
            let action = shell.hotkeys.lookup(key, &modifiers).unwrap_or_else(|| {
                panic!("{key:?} with {modifiers:?} is claimed from the compositor and then dropped")
            });
            assert!(
                !action.is_conditional(),
                "{key:?} with {modifiers:?} is held permanently but only means \
                 something some of the time, so it is taken from every window for \
                 nothing the rest of the time"
            );
        }
    }

    /// And the converse, which is the failure that actually happens: somebody
    /// adds a binding to the table and the shortcut silently never fires,
    /// because the keystroke goes to whatever window has the keyboard.
    ///
    /// Swept over the whole key vocabulary rather than the keys already in the
    /// list — a binding on a key nobody has bound before is exactly the case a
    /// narrower sweep would miss.
    #[test]
    fn every_chord_the_shell_acts_on_is_a_chord_it_can_hear() {
        let shell = shell();
        let claimed: std::collections::HashSet<_> = shell
            .global_chords()
            .into_iter()
            .chain(shell.conditional_chords())
            .collect();
        for &key in guiremote::input::ALL_KEYS {
            for modifiers in hotkeys::ALL_MODIFIER_SETS {
                if shell.hotkeys.lookup(key, &modifiers).is_some() {
                    assert!(
                        claimed.contains(&(key, modifiers)),
                        "{key:?} with {modifiers:?} is bound but never grabbed, \
                         so it does nothing unless the desktop itself is focused"
                    );
                }
            }
        }
    }

    /// Escape is the one binding that must *not* be held permanently: with no
    /// popup open the press belongs to the focused window's own dialog. Pinned
    /// because moving it into the global list would compile, pass every other
    /// test here, and break the Escape key on the entire desktop.
    #[test]
    fn escape_is_not_held_permanently() {
        let shell = shell();
        assert!(
            !shell
                .global_chords()
                .contains(&(Key::Escape, Modifiers::NONE)),
            "a permanent Escape grab takes the key from every dialog on the desktop"
        );
        assert!(
            shell
                .conditional_chords()
                .contains(&(Key::Escape, Modifiers::NONE)),
            "no grab at all means a menu opened with the mouse cannot be closed \
             with the keyboard"
        );
    }

    /// The condition the session reconciles the Escape grab against. It has to
    /// be true exactly when there is something for Escape to close, or the shell
    /// either holds the key for nothing or cannot close its own menu.
    #[test]
    fn the_escape_condition_tracks_what_escape_would_close() {
        let mut shell = shell();
        assert!(
            !shell.any_popup_open(),
            "nothing is open on a fresh desktop"
        );
        shell.start_menu_open = true;
        assert!(shell.any_popup_open());
        assert!(shell.dismiss_popups(), "there was something to dismiss");
        assert!(
            !shell.any_popup_open(),
            "the condition outlived the popups it names"
        );
    }

    /// The whole point of the card: a chord opens it, the same chord closes it,
    /// and it is drawn only while it is open.
    ///
    /// Pressed through `handle_hotkey` rather than by setting the flag, because
    /// the flag was never the part that was missing — `render_settings_panel`
    /// existed and worked for months with nothing able to reach it.
    /// Every string the shortcut card draws.
    fn card_text(shell: &DesktopShell) -> Vec<String> {
        shell
            .render_shortcut_card()
            .expect("the card is not open")
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                guitk::render::RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// **The card says what the last rebind did.**
    ///
    /// `shortcut_message` was composed on every outcome from the day the
    /// editor was written and drawn by nothing, so rebinding a key was silent
    /// whether it worked, was refused, or worked and failed to persist.
    #[test]
    fn the_shortcut_card_reports_the_last_rebind() {
        let mut shell = shell();
        shell.shortcut_card_open = true;
        let quiet = card_text(&shell);

        shell
            .shortcut_editor
            .set_message(Some("Ctrl+Alt+T is now Terminal".to_string()));
        let loud = card_text(&shell);

        assert!(
            loud.iter().any(|t| t == "Ctrl+Alt+T is now Terminal"),
            "the outcome never reached the card: {loud:?}"
        );
        assert!(
            !quiet.iter().any(|t| t == "Ctrl+Alt+T is now Terminal"),
            "the card drew the message before there was one"
        );
    }

    /// **The half-failure is the one that must be visible.**
    ///
    /// A rebind that worked and could not be saved leaves the user with a
    /// shortcut that works today and is gone tomorrow. The handler's own
    /// comment calls that "the difference between a shortcut that will be gone
    /// tomorrow and one the user believes is set" -- and until the message was
    /// drawn, the user was always in the second state.
    #[test]
    fn a_rebind_that_could_not_be_saved_says_so_on_the_card() {
        let mut shell = shell();
        shell.shortcut_card_open = true;
        shell.shortcut_editor.set_message(Some(
            "Super+K is now Search, but could not be saved: disk full".to_string(),
        ));

        let drawn = card_text(&shell);

        assert!(
            drawn.iter().any(|t| t.contains("could not be saved")),
            "a rebind that did not persist looks identical to one that did: {drawn:?}"
        );
    }

    #[test]
    fn the_shortcut_card_opens_and_closes_on_its_own_chord() {
        let mut shell = shell();
        assert!(
            shell.render_shortcut_card().is_none(),
            "the card is drawn before anyone asked for it"
        );

        let chord = press(Key::Slash, super_only());
        let out = shell.handle_hotkey(&chord);
        assert!(
            out.consumed,
            "Super+/ fell through to the focused window, which will be typed a \
             slash it did not ask for"
        );
        assert!(shell.shortcut_card_open);
        assert!(
            shell.render_shortcut_card().is_some(),
            "the card is open and draws nothing"
        );
        assert!(
            shell.any_popup_open(),
            "the card is open but Escape would not be grabbed, so it cannot be \
             closed from inside an application"
        );

        assert!(shell.handle_hotkey(&chord).consumed);
        assert!(
            !shell.shortcut_card_open,
            "the second press did not close it"
        );
        assert!(shell.render_shortcut_card().is_none());
    }

    /// The card is placed on the screen it is drawn on, not off the edge of it.
    #[test]
    fn the_shortcut_card_is_centred_on_the_display() {
        let shell_wide = {
            let mut s = shell();
            s.shortcut_card_open = true;
            s
        };
        let tree = shell_wide.render_shortcut_card().expect("the card is open");
        let Some(guitk::render::RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            ..
        }) = tree
            .commands
            .iter()
            .find(|c| matches!(c, guitk::render::RenderCommand::FillRect { .. }))
        else {
            panic!("the card drew no background");
        };
        assert!(*x >= 0.0 && *y >= 0.0, "the card starts off the screen");
        let (screen_w, screen_h) = (
            f32::from(u16::try_from(shell_wide.screen_width).unwrap_or(u16::MAX)),
            f32::from(u16::try_from(shell_wide.screen_height).unwrap_or(u16::MAX)),
        );
        // Centred means the two margins match, which is a stronger statement
        // than "on screen" and the one that actually breaks if the size the
        // placement used and the size the drawing used ever diverge.
        assert!(
            (*x - (screen_w - width) / 2.0).abs() < 0.5,
            "left margin {x} is not half of the leftover {}",
            screen_w - width
        );
        assert!(
            (*y - (screen_h - height) / 2.0).abs() < 0.5,
            "top margin {y} is not half of the leftover {}",
            screen_h - height
        );
    }

    /// Opening the card puts the taskbar's panels away, and opening one of them
    /// puts the card away. Both directions, because only one of them was
    /// obvious to write.
    #[test]
    fn the_card_and_the_taskbar_panels_are_not_open_at_once() {
        let mut shell = shell();
        shell.start_menu_open = true;
        shell.toggle_shortcut_card();
        assert!(shell.shortcut_card_open);
        assert!(!shell.start_menu_open, "the start menu survived the card");

        shell.toggle_start_menu();
        assert!(shell.start_menu_open);
        assert!(
            !shell.shortcut_card_open,
            "the card survived the start menu"
        );

        shell.toggle_shortcut_card();
        shell.toggle_notifications();
        assert!(
            !shell.shortcut_card_open,
            "the card is left under the notification pane's scrim, which dims it"
        );
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
        assert_eq!(shell.taskbar_windows().last().map(|w| w.id), Some(second));
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
        assert_eq!(shell.taskbar_windows().last().map(|w| w.id), Some(bottom));
        assert!(!shell.windows.get(&middle).unwrap().focused);
    }

    #[test]
    fn the_program_a_window_belongs_to_arrives_with_the_list() {
        // The shell never asks a window what it is; it only ever knows what the
        // last list said. Two windows of one program and one of another,
        // because telling those apart is the only thing the field is for.
        let mut shell = shell();
        let mut notes = WindowInfo::new(1, 40, "notes.md");
        notes.app_id = "editor".to_string();
        let mut draft = WindowInfo::new(2, 40, "draft.md");
        draft.app_id = "editor".to_string();
        let anon = WindowInfo::new(3, 41, "Some Window");
        shell.apply_window_list(&WindowList::new(0, vec![notes, draft, anon]));

        let of = |id: u64| {
            shell
                .windows
                .get(&WindowId(id))
                .expect("known")
                .app_id
                .clone()
        };
        assert_eq!(of(1), "editor");
        assert_eq!(of(2), "editor");
        assert_eq!(
            of(3),
            "",
            "a window that named no program was given one from somewhere"
        );
    }

    #[test]
    fn a_window_is_described_by_the_latest_list_and_not_the_one_before() {
        // Unlike the taskbar icon, which is shell-local state carried across
        // lists, this is the compositor's own report. Carrying the previous
        // value forward would mean a window that had been anonymous stayed
        // anonymous for the rest of the session even after its program spoke
        // up -- and the rules that key off it would never fire.
        let mut shell = shell();
        shell.apply_window_list(&WindowList::new(
            0,
            vec![WindowInfo::new(1, 40, "notes.md")],
        ));
        assert_eq!(shell.windows.get(&WindowId(1)).expect("known").app_id, "");

        let mut named = WindowInfo::new(1, 40, "notes.md");
        named.app_id = "editor".to_string();
        shell.apply_window_list(&WindowList::new(0, vec![named]));
        assert_eq!(
            shell.windows.get(&WindowId(1)).expect("known").app_id,
            "editor"
        );
    }

    // ==================================================================
    // Window rules
    //
    // The rules engine decides; the shell only carries out the five actions
    // it has a channel for. Every test here goes through `apply_window_list`,
    // because that is the one place the engine is consulted and the
    // once-per-arrival contract is the thing most likely to be broken by a
    // later edit.
    // ==================================================================

    /// A window arriving, named, with whatever rules are in place.
    fn arrive(shell: &mut DesktopShell, id: u64, app_id: &str) -> Vec<ShellRequest> {
        let mut list = as_list(shell);
        for other in &mut list {
            other.focused = false;
        }
        let mut fresh = WindowInfo::new(id, 1, "A Window");
        fresh.app_id = app_id.to_string();
        fresh.focused = true;
        fresh.workspace = shell.current_desktop;
        list.push(fresh);
        shell.apply_window_list(&WindowList::new(shell.current_desktop, list))
    }

    /// The same windows again, unchanged — the next frame.
    fn again(shell: &mut DesktopShell) -> Vec<ShellRequest> {
        let list = as_list(shell);
        shell.apply_window_list(&WindowList::new(shell.current_desktop, list))
    }

    /// A rule about `app_id` whose actions `set` fills in.
    fn rule(shell: &mut DesktopShell, app_id: &str, set: impl Fn(&mut window_rules::RuleActions)) {
        let mut r = window_rules::WindowRule::new(
            0,
            "test rule",
            window_rules::MatchCriteria::AppId(app_id.to_string()),
        );
        set(&mut r.actions);
        assert!(
            shell.rules.add_rule(r).is_some(),
            "the rule should be taken"
        );
    }

    #[test]
    fn hiding_a_window_from_the_taskbar_does_not_hide_it_from_alt_tab() {
        // The two flags are separate actions, and users mean different things
        // by them: a chat window kept out of the taskbar is still somewhere
        // you want to Alt+Tab to. One list serving both would make each flag
        // silently mean the other.
        let mut shell = shell();
        rule(&mut shell, "chat", |a| a.skip_taskbar = Some(true));
        arrive(&mut shell, 1, "chat");
        arrive(&mut shell, 2, "editor");

        let taskbar: Vec<WindowId> = shell.taskbar_windows().iter().map(|w| w.id).collect();
        let switcher: Vec<WindowId> = shell.switcher_windows().iter().map(|w| w.id).collect();
        assert_eq!(taskbar, vec![WindowId(2)]);
        assert_eq!(switcher, vec![WindowId(1), WindowId(2)]);
    }

    #[test]
    fn hiding_a_window_from_alt_tab_does_not_hide_it_from_the_taskbar() {
        let mut shell = shell();
        rule(&mut shell, "monitor", |a| a.skip_alt_tab = Some(true));
        arrive(&mut shell, 1, "monitor");
        arrive(&mut shell, 2, "editor");

        let taskbar: Vec<WindowId> = shell.taskbar_windows().iter().map(|w| w.id).collect();
        let switcher: Vec<WindowId> = shell.switcher_windows().iter().map(|w| w.id).collect();
        assert_eq!(taskbar, vec![WindowId(1), WindowId(2)]);
        assert_eq!(switcher, vec![WindowId(2)]);
    }

    #[test]
    fn the_switcher_lands_on_the_window_its_index_names() {
        // `alt_tab_index` counts into `switcher_windows`, so a switcher drawn
        // from the taskbar's list — which this shell's rule makes a different
        // list — would activate a window the user was not looking at.
        let mut shell = shell();
        rule(&mut shell, "monitor", |a| a.skip_alt_tab = Some(true));
        arrive(&mut shell, 1, "monitor");
        arrive(&mut shell, 2, "editor");
        arrive(&mut shell, 3, "editor");

        shell.start_alt_tab();
        let request = shell.finish_alt_tab().expect("the switcher was open");
        assert_eq!(
            request,
            ShellRequest::window(WindowId(2), ShellControlAction::Activate),
            "Alt+Tab should reach the window below the top one, skipping the \
             one the rule excluded"
        );
    }

    #[test]
    fn a_rule_that_hides_a_window_keeps_hiding_it_on_later_lists() {
        // Nothing in a window list says "this window is not in the taskbar" —
        // the rule fired once, on arrival, and the answer is the shell's to
        // keep. Re-evaluating instead of carrying it would be the other bug:
        // one-shot rules would be destroyed on the frame after they were
        // written.
        let mut shell = shell();
        rule(&mut shell, "chat", |a| a.skip_taskbar = Some(true));
        arrive(&mut shell, 1, "chat");
        again(&mut shell);
        again(&mut shell);
        assert!(shell.taskbar_windows().is_empty());
    }

    #[test]
    fn a_rule_forbidding_something_says_so_once() {
        let mut shell = shell();
        rule(&mut shell, "kiosk", |a| {
            a.prevent_close = Some(true);
            a.prevent_resize = Some(true);
        });

        let asked = arrive(&mut shell, 1, "kiosk");
        assert!(asked.contains(&ShellRequest::SetWindowPolicy {
            window: WindowId(1),
            policy: crate::WindowPolicy {
                prevent_close: true,
                prevent_move: false,
                prevent_resize: true,
            },
        }));
        assert_eq!(
            asked
                .iter()
                .filter(|r| matches!(r, ShellRequest::SetWindowPolicy { .. }))
                .count(),
            1,
            "the three flags are one rule's worth of answer, not three requests"
        );
    }

    /// A rule that says nothing about restrictions imposes none.
    #[test]
    fn a_rule_silent_on_restrictions_asks_for_no_policy() {
        let mut shell = shell();
        rule(&mut shell, "kiosk", |a| {
            a.opacity = Some(1.0);
        });

        assert!(
            !arrive(&mut shell, 1, "kiosk")
                .iter()
                .any(|r| matches!(r, ShellRequest::SetWindowPolicy { .. })),
            "silence must not be read as a restriction"
        );
    }

    /// An explicit `false` takes a restriction back.
    #[test]
    fn a_rule_can_lift_a_restriction_it_previously_set() {
        let mut shell = shell();
        rule(&mut shell, "kiosk", |a| {
            a.prevent_close = Some(false);
        });

        assert!(
            arrive(&mut shell, 1, "kiosk").contains(&ShellRequest::SetWindowPolicy {
                window: WindowId(1),
                policy: crate::WindowPolicy::default(),
            }),
            "an explicit false is a request to be unrestrained, not silence"
        );
    }

    /// A rule naming only one limit sends zeroes for the other.
    ///
    /// The zeroes mean "say nothing about that one", which is what stops a
    /// maximum-only rule from discarding the program's own minimum.
    #[test]
    fn a_rule_naming_one_size_limit_says_nothing_about_the_other() {
        let mut shell = shell();
        rule(&mut shell, "chat", |a| {
            a.max_size = Some((900, 700));
        });

        assert!(
            arrive(&mut shell, 1, "chat").contains(&ShellRequest::SetSizeLimits {
                window: WindowId(1),
                min: (0, 0),
                max: (900, 700),
            })
        );
    }

    #[test]
    fn a_rule_naming_both_size_limits_sends_both() {
        let mut shell = shell();
        rule(&mut shell, "chat", |a| {
            a.min_size = Some((300, 200));
            a.max_size = Some((900, 700));
        });

        assert!(
            arrive(&mut shell, 1, "chat").contains(&ShellRequest::SetSizeLimits {
                window: WindowId(1),
                min: (300, 200),
                max: (900, 700),
            })
        );
    }

    /// The limits are asked for before the size, so a sized-and-limited rule
    /// clamps on the way in rather than being corrected afterwards.
    #[test]
    fn size_limits_are_asked_for_before_the_size() {
        let mut shell = shell();
        rule(&mut shell, "chat", |a| {
            a.min_size = Some((300, 200));
            a.size = Some(window_rules::SizeSpec::Exact {
                width: 100,
                height: 100,
            });
        });

        let asked = arrive(&mut shell, 1, "chat");
        let limits = asked
            .iter()
            .position(|r| matches!(r, ShellRequest::SetSizeLimits { .. }))
            .expect("limits");
        let resize = asked
            .iter()
            .position(|r| matches!(r, ShellRequest::ResizeWindow { .. }))
            .expect("resize");
        assert!(limits < resize);
    }

    #[test]
    fn a_rule_silent_on_size_limits_asks_for_none() {
        let mut shell = shell();
        rule(&mut shell, "chat", |a| {
            a.opacity = Some(1.0);
        });

        assert!(
            !arrive(&mut shell, 1, "chat")
                .iter()
                .any(|r| matches!(r, ShellRequest::SetSizeLimits { .. }))
        );
    }

    #[test]
    fn a_rule_can_pin_a_window_above_or_below_its_neighbours() {
        for (top, bottom, expected) in [
            (Some(true), None, crate::StackTier::Top),
            (None, Some(true), crate::StackTier::Bottom),
            (Some(false), None, crate::StackTier::Normal),
        ] {
            let mut shell = shell();
            rule(&mut shell, "chat", |a| {
                a.always_on_top = top;
                a.always_on_bottom = bottom;
            });
            assert!(
                arrive(&mut shell, 1, "chat").contains(&ShellRequest::SetStackTier {
                    window: WindowId(1),
                    tier: expected,
                }),
                "top={top:?} bottom={bottom:?}"
            );
        }
    }

    /// A rule asking for both is resolved here, not sent as a contradiction.
    #[test]
    fn a_rule_asking_for_both_top_and_bottom_picks_top() {
        let mut shell = shell();
        rule(&mut shell, "chat", |a| {
            a.always_on_top = Some(true);
            a.always_on_bottom = Some(true);
        });

        let asked = arrive(&mut shell, 1, "chat");
        assert!(asked.contains(&ShellRequest::SetStackTier {
            window: WindowId(1),
            tier: crate::StackTier::Top,
        }));
        assert_eq!(
            asked
                .iter()
                .filter(|r| matches!(r, ShellRequest::SetStackTier { .. }))
                .count(),
            1,
            "the compositor must not be handed two tiers to choose between"
        );
    }

    /// A rule that says nothing about stacking asks for nothing.
    #[test]
    fn a_rule_silent_on_stacking_leaves_the_window_alone() {
        let mut shell = shell();
        rule(&mut shell, "chat", |a| {
            a.opacity = Some(1.0);
        });

        assert!(
            !arrive(&mut shell, 1, "chat")
                .iter()
                .any(|r| matches!(r, ShellRequest::SetStackTier { .. })),
            "silence is not an instruction"
        );
    }

    /// A rule can place and size a window.
    #[test]
    fn a_rule_giving_a_position_and_a_size_is_carried_out() {
        let mut shell = shell();
        rule(&mut shell, "editor", |a| {
            a.position = Some(window_rules::PositionSpec::Absolute { x: 40, y: 60 });
            a.size = Some(window_rules::SizeSpec::Exact {
                width: 800,
                height: 600,
            });
        });

        let asked = arrive(&mut shell, 1, "editor");
        assert!(
            asked.contains(&ShellRequest::ResizeWindow {
                window: WindowId(1),
                width: 800,
                height: 600,
            }),
            "the size was lost: {asked:?}"
        );
        assert!(
            asked.contains(&ShellRequest::MoveWindow {
                window: WindowId(1),
                x: 40,
                y: 60,
            }),
            "the position was lost: {asked:?}"
        );
    }

    /// The size is asked for before the position.
    ///
    /// Not cosmetic: a centred placement is computed *from* the size, so a
    /// window sized after being centred would be centred for the size it used
    /// to have.
    #[test]
    fn a_rule_sizes_before_it_places() {
        let mut shell = shell();
        rule(&mut shell, "editor", |a| {
            a.position = Some(window_rules::PositionSpec::Absolute { x: 0, y: 0 });
            a.size = Some(window_rules::SizeSpec::Exact {
                width: 100,
                height: 100,
            });
        });

        let asked = arrive(&mut shell, 1, "editor");
        let resize = asked
            .iter()
            .position(|r| matches!(r, ShellRequest::ResizeWindow { .. }))
            .expect("a resize");
        let mv = asked
            .iter()
            .position(|r| matches!(r, ShellRequest::MoveWindow { .. }))
            .expect("a move");
        assert!(resize < mv, "size must be settled before the placement");
    }

    /// A percentage rule is resolved against the display the shell knows.
    #[test]
    fn a_percentage_rule_is_resolved_against_the_screen() {
        let mut shell = DesktopShell::new(1000, 800);
        rule(&mut shell, "editor", |a| {
            a.size = Some(window_rules::SizeSpec::Percentage {
                w_pct: 0.5,
                h_pct: 0.25,
            });
        });

        assert!(
            arrive(&mut shell, 1, "editor").contains(&ShellRequest::ResizeWindow {
                window: WindowId(1),
                width: 500,
                height: 200,
            })
        );
    }

    /// Centring needs a size, and says so by declining.
    ///
    /// The shell is told window positions in the window list but not the size
    /// a program is about to choose, so a rule that centres without setting a
    /// size cannot be honoured. Declining leaves the window where the program
    /// put it; centring against a guess would move it somewhere wrong.
    #[test]
    fn centring_without_a_size_is_declined_rather_than_guessed() {
        let mut shell = shell();
        rule(&mut shell, "editor", |a| {
            a.position = Some(window_rules::PositionSpec::CenterOnMonitor(0));
        });

        let asked = arrive(&mut shell, 1, "editor");
        assert!(
            !asked
                .iter()
                .any(|r| matches!(r, ShellRequest::MoveWindow { .. })),
            "centred against a size nobody knows: {asked:?}"
        );
    }

    #[test]
    fn centring_with_a_size_puts_the_window_in_the_middle() {
        let mut shell = DesktopShell::new(1000, 800);
        rule(&mut shell, "editor", |a| {
            a.position = Some(window_rules::PositionSpec::CenterOnMonitor(0));
            a.size = Some(window_rules::SizeSpec::Exact {
                width: 400,
                height: 200,
            });
        });

        assert!(
            arrive(&mut shell, 1, "editor").contains(&ShellRequest::MoveWindow {
                window: WindowId(1),
                x: 300,
                y: 300,
            })
        );
    }

    /// A second monitor is declined, not centred on the first.
    ///
    /// Putting a window on the wrong screen is a worse answer than leaving it
    /// alone, and this shell has bounds for one display.
    #[test]
    fn a_rule_naming_a_second_monitor_is_declined() {
        let mut shell = shell();
        rule(&mut shell, "editor", |a| {
            a.position = Some(window_rules::PositionSpec::CenterOnMonitor(1));
            a.size = Some(window_rules::SizeSpec::Exact {
                width: 100,
                height: 100,
            });
        });

        let asked = arrive(&mut shell, 1, "editor");
        assert!(
            !asked
                .iter()
                .any(|r| matches!(r, ShellRequest::MoveWindow { .. })),
            "placed on a monitor this shell has no bounds for: {asked:?}"
        );
    }

    /// An opacity rule now reaches the compositor.
    ///
    /// `opacity` was one of the twelve rule fields accepted, saved, listed and
    /// then dropped -- the shell had no request it could send about another
    /// client's window, because the ordinary `SetOpacity` resolves against the
    /// sender's own.
    #[test]
    fn a_rule_asking_for_transparency_is_carried_out() {
        let mut shell = shell();
        rule(&mut shell, "chat", |a| {
            a.opacity = Some(0.8);
        });

        assert_eq!(
            arrive(&mut shell, 1, "chat"),
            vec![ShellRequest::SetOpacity {
                window: WindowId(1),
                // 0.8 * 255 rounds to 204.
                alpha: 204,
            }]
        );
    }

    /// The ends of the range survive the conversion exactly.
    ///
    /// A byte carries the opacity because the compositor blends with an
    /// eight-bit alpha; what must not happen is 1.0 arriving as 254, which
    /// would make "fully opaque" faintly transparent and be almost impossible
    /// to see.
    #[test]
    fn a_fully_opaque_rule_is_fully_opaque() {
        for (opacity, expected) in [(0.0_f32, 0_u8), (1.0, 255), (0.5, 128)] {
            let mut shell = shell();
            rule(&mut shell, "chat", |a| {
                a.opacity = Some(opacity);
            });
            assert_eq!(
                arrive(&mut shell, 1, "chat"),
                vec![ShellRequest::SetOpacity {
                    window: WindowId(1),
                    alpha: expected,
                }],
                "opacity {opacity}"
            );
        }
    }

    /// A rule can ask for both a state and a transparency.
    #[test]
    fn opacity_does_not_displace_the_other_rule_actions() {
        let mut shell = shell();
        rule(&mut shell, "chat", |a| {
            a.opacity = Some(1.0);
            a.initial_state = Some(window_rules::InitialState::Maximized);
        });

        let asked = arrive(&mut shell, 1, "chat");
        assert!(
            asked.contains(&ShellRequest::window(
                WindowId(1),
                ShellControlAction::Maximize
            )),
            "the state was lost: {asked:?}"
        );
        assert!(
            asked.contains(&ShellRequest::SetOpacity {
                window: WindowId(1),
                alpha: 255,
            }),
            "the opacity was lost: {asked:?}"
        );
    }

    /// A fullscreen rule now reaches the compositor.
    ///
    /// It was accepted by the panel, saved to the file, shown in the rule
    /// list, and then dropped on the floor: `Fullscreen` was the one
    /// `InitialState` with no verb the shell could send about another client's
    /// window. `Minimized` and `Maximized` worked, which made it the kind of
    /// gap a user finds by writing a rule and watching nothing happen.
    #[test]
    fn a_rule_asking_for_a_fullscreen_start_is_carried_out() {
        let mut shell = shell();
        rule(&mut shell, "player", |a| {
            a.initial_state = Some(window_rules::InitialState::Fullscreen);
        });

        assert_eq!(
            arrive(&mut shell, 1, "player"),
            vec![ShellRequest::window(
                WindowId(1),
                ShellControlAction::Fullscreen
            )]
        );
    }

    /// And it is not a synonym for maximise.
    ///
    /// Maximize fills the work area and leaves the taskbar showing; fullscreen
    /// covers the display. A rule that asked for one and got the other would
    /// look almost right, which is the hardest kind of wrong to notice.
    #[test]
    fn fullscreen_and_maximized_are_different_requests() {
        let mut full = shell();
        rule(&mut full, "player", |a| {
            a.initial_state = Some(window_rules::InitialState::Fullscreen);
        });
        let mut max = shell();
        rule(&mut max, "player", |a| {
            a.initial_state = Some(window_rules::InitialState::Maximized);
        });

        assert_ne!(
            arrive(&mut full, 1, "player"),
            arrive(&mut max, 1, "player")
        );
    }

    #[test]
    fn a_rule_asking_for_a_maximized_start_asks_once_and_never_again() {
        // "Initial state" means the state it *starts* in. Asking again on
        // every list would snap a window the user un-maximised back within
        // the frame, and there would be no way to un-maximise it at all.
        let mut shell = shell();
        rule(&mut shell, "editor", |a| {
            a.initial_state = Some(window_rules::InitialState::Maximized);
        });

        let first = arrive(&mut shell, 1, "editor");
        assert_eq!(
            first,
            vec![ShellRequest::window(
                WindowId(1),
                ShellControlAction::Maximize
            )]
        );
        assert!(again(&mut shell).is_empty(), "asked twice");
        assert!(again(&mut shell).is_empty(), "asked a third time");
    }

    #[test]
    fn a_one_shot_rule_is_spent_by_the_window_it_fired_on_and_not_by_the_frame() {
        // The trap this guards: `evaluate` deletes one-shot rules, so calling
        // it per *list* rather than per newly-arrived *window* would spend the
        // rule on whatever window happened to be open when it was written.
        let mut shell = shell();
        let mut r = window_rules::WindowRule::new(
            0,
            "once",
            window_rules::MatchCriteria::AppId("editor".to_string()),
        );
        r.one_shot = true;
        r.actions.initial_state = Some(window_rules::InitialState::Minimized);
        let id = shell.rules.add_rule(r).expect("the rule should be taken");

        // Some unrelated window is already open, and several frames pass.
        arrive(&mut shell, 1, "terminal");
        again(&mut shell);
        again(&mut shell);
        assert!(
            shell.rules.rule_by_id(id).is_some(),
            "a one-shot rule was spent by a window it does not match"
        );

        let requests = arrive(&mut shell, 2, "editor");
        assert_eq!(
            requests,
            vec![ShellRequest::window(
                WindowId(2),
                ShellControlAction::Minimize
            )]
        );
        assert!(shell.rules.rule_by_id(id).is_none(), "now it is spent");
        assert!(arrive(&mut shell, 3, "editor").is_empty());
    }

    #[test]
    fn a_rule_files_a_window_on_another_desktop_only_when_that_is_a_move() {
        let mut shell = shell();
        rule(&mut shell, "editor", |a| a.desktop = Some(2));
        assert_eq!(
            arrive(&mut shell, 1, "editor"),
            vec![ShellRequest::MoveWindowToDesktop {
                window: WindowId(1),
                desktop: 2,
            }]
        );

        // A window that is already there needs no request. Sending one anyway
        // would be a recomposite for nothing, every time such a window opened.
        shell.current_desktop = 2;
        assert!(
            arrive(&mut shell, 2, "editor").is_empty(),
            "asked to move a window to the desktop it is already on"
        );
    }

    #[test]
    fn a_rule_naming_a_desktop_that_does_not_exist_is_dropped() {
        // The number comes from a config file the user edits by hand. The
        // compositor would refuse it, but the shell knows how many desktops
        // there are and there is no reason to ask.
        let mut shell = shell();
        let beyond = shell.num_desktops;
        rule(&mut shell, "editor", |a| a.desktop = Some(beyond));
        assert!(arrive(&mut shell, 1, "editor").is_empty());
    }

    #[test]
    fn a_rule_snaps_a_window_into_a_slot_the_compositor_knows() {
        let mut shell = shell();
        rule(&mut shell, "editor", |a| a.snap_zone = Some(1));
        let slot = snap::SnapSlot::from_index(1).expect("1 names a slot");
        assert_eq!(
            arrive(&mut shell, 1, "editor"),
            vec![ShellRequest::window(
                WindowId(1),
                ShellControlAction::SnapToZone(slot)
            )]
        );
    }

    #[test]
    fn a_rule_naming_a_snap_slot_that_does_not_exist_is_dropped() {
        let mut shell = shell();
        let beyond = u32::from(snap::SnapSlot::COUNT);
        rule(&mut shell, "editor", |a| a.snap_zone = Some(beyond));
        assert!(arrive(&mut shell, 1, "editor").is_empty());
    }

    #[test]
    fn a_rule_setting_both_a_state_and_a_zone_lands_in_the_zone() {
        // Both are sent, in that order, because a snap is the more specific of
        // the two instructions and the compositor applies whichever it hears
        // about last.
        let mut shell = shell();
        rule(&mut shell, "editor", |a| {
            a.initial_state = Some(window_rules::InitialState::Maximized);
            a.snap_zone = Some(1);
        });
        let slot = snap::SnapSlot::from_index(1).expect("1 names a slot");
        assert_eq!(
            arrive(&mut shell, 1, "editor"),
            vec![
                ShellRequest::window(WindowId(1), ShellControlAction::Maximize),
                ShellRequest::window(WindowId(1), ShellControlAction::SnapToZone(slot)),
            ]
        );
    }

    #[test]
    fn a_window_that_names_no_program_is_left_alone_by_every_app_rule() {
        // An unnamed program is not every program. If it were, one rule about
        // one application would rearrange every window that declined to
        // identify itself.
        let mut shell = shell();
        rule(&mut shell, "editor", |a| {
            a.initial_state = Some(window_rules::InitialState::Minimized);
            a.skip_taskbar = Some(true);
        });
        assert!(arrive(&mut shell, 1, "").is_empty());
        assert_eq!(shell.taskbar_windows().len(), 1);
    }

    #[test]
    fn where_a_window_was_last_left_is_remembered_from_the_lists_not_the_arrival() {
        // "Remember last position" means the position it was last *at*, and a
        // window moves long after it arrives — so this is the one thing read
        // off every list rather than only the first.
        let mut shell = shell();
        arrive(&mut shell, 1, "editor");

        let mut moved = as_list(&shell);
        moved[0].x = 400;
        moved[0].y = 300;
        moved[0].width = 900;
        moved[0].height = 700;
        shell.apply_window_list(&WindowList::new(0, moved));

        // Asked of the engine directly, because `position` is one of the
        // twelve actions with no channel to the compositor — see
        // `TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO`.
        // What is under test is that the shell fed the rectangle in, which is
        // the shell's half of the job.
        rule(&mut shell, "editor", |a| {
            a.position = Some(window_rules::PositionSpec::RememberLast);
        });
        assert_eq!(
            shell.rules.evaluate("A Window", "editor").position,
            Some(window_rules::PositionSpec::Absolute { x: 400, y: 300 })
        );
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
        assert!(shell.taskbar_windows().is_empty());
        assert_eq!(
            shell.focused_window, None,
            "the window that had the keyboard is not on screen to have it"
        );

        compositor_moved(&mut shell, id, 1);
        assert_eq!(
            shell
                .taskbar_windows()
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
            shell.taskbar_windows().is_empty(),
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
        assert!(shell.alt_tab_index < shell.taskbar_windows().len());

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
            text: String::new(),
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
        assert!(shell.alt_tab_index < shell.taskbar_windows().len());
    }

    /// The same property, from the one starting index that actually exercises
    /// the clamp.
    ///
    /// `stepping_backwards_survives_the_windows_closing_underneath_it` above
    /// steps forward *twice* from a four-window switcher, which wraps the index
    /// round to 0 — and 0 is in range for every list, so removing the clamp
    /// leaves that test green. Stepping forward once leaves the index at 3, and
    /// stepping back from 3 in a one-window list is the only arithmetic that
    /// tells the two versions apart: clamped it is 0, unclamped it is 2, which
    /// is another index past the end and so `finish_alt_tab` picks nothing at
    /// all. The user sees Shift+Alt+Tab do nothing after a window closed under
    /// the switcher.
    #[test]
    fn stepping_backwards_from_the_end_lands_in_the_list_not_past_it() {
        let mut shell = shell();
        let ids: Vec<WindowId> = (0..4).map(|i| open(&mut shell, &format!("w{i}"))).collect();

        shell.start_alt_tab();
        shell.next_alt_tab();
        assert_eq!(shell.alt_tab_index, 3, "the last row, not a wrapped one");

        for id in &ids[1..] {
            close(&mut shell, *id);
        }

        shell.prev_alt_tab();
        assert!(
            shell.alt_tab_index < shell.taskbar_windows().len(),
            "index {} is past the end of a {}-window list",
            shell.alt_tab_index,
            shell.taskbar_windows().len()
        );
        assert_eq!(
            shell.finish_alt_tab(),
            Some(ShellRequest::window(ids[0], ShellControlAction::Activate)),
            "and the one window left is the one it lands on"
        );
    }

    /// A switcher left on an index that no longer names a window still has to
    /// close.
    ///
    /// `finish_alt_tab` answers two questions at once — "which window?" and "the
    /// switcher is over now" — and only the first of them can fail. Answering
    /// them in the wrong order lets the stale-index `?` return before the
    /// switcher is marked closed, and then it is closed by nothing: the Alt
    /// release that would have ended it has already been spent, so the overlay
    /// stays on screen over every window for the rest of the session.
    #[test]
    fn a_switcher_whose_window_closed_under_it_still_closes() {
        let mut shell = shell();
        let ids: Vec<WindowId> = (0..3).map(|i| open(&mut shell, &format!("w{i}"))).collect();

        shell.start_alt_tab();
        shell.next_alt_tab();
        assert_eq!(shell.alt_tab_index, 2, "the last row");

        for id in &ids[1..] {
            close(&mut shell, *id);
        }

        assert_eq!(
            shell.finish_alt_tab(),
            None,
            "there is no window at that index any more, so it names none"
        );
        assert!(
            !shell.alt_tab_active,
            "but it is still over -- closing the switcher cannot depend on \
             finding a window"
        );
    }

    /// Letting go of a key that is not Alt does not end the switcher.
    ///
    /// Alt+Tab is held: Alt stays down while Tab is pressed and released, over
    /// and over, and the switch is committed by the *Alt* release. A guard that
    /// asked only whether the switcher was open would commit on the first Tab
    /// release instead, so every use of Alt+Tab would land on the second window
    /// and the user could never reach the third.
    #[test]
    fn releasing_tab_does_not_end_the_window_switcher() {
        let mut shell = shell();
        for i in 0..3 {
            open(&mut shell, &format!("w{i}"));
        }

        shell.start_alt_tab();
        let landed_on = shell.alt_tab_index;

        let release_tab = KeyEvent {
            key: Key::Tab,
            pressed: false,
            modifiers: Modifiers::alt(),
            text: String::new(),
        };
        let outcome = shell.handle_hotkey(&release_tab);
        assert!(!outcome.consumed, "a Tab release is not the shell's");
        assert!(outcome.requests.is_empty(), "and asks for nothing");
        assert!(shell.alt_tab_active, "the switcher is still up");
        assert_eq!(shell.alt_tab_index, landed_on, "and has not moved");

        // The release that *does* end it, so the test says what the rule is and
        // not merely what it is not.
        let release_alt = KeyEvent {
            key: Key::LeftAlt,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        assert!(shell.handle_hotkey(&release_alt).consumed);
        assert!(!shell.alt_tab_active);
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
                .taskbar_windows()
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
                .taskbar_windows()
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

        assert!(shell.datetime.set_zone(Some("UTC")));
        assert_eq!(shell.clock_string_at(INSTANT), "16:30");

        // The shipped *default* is New York, which is the whole point: out of
        // the box the corner of the screen used to read 16:30 in a zone where
        // it was half past noon.
        assert!(shell.datetime.set_zone(Some("America/New_York")));
        assert_eq!(
            shell.clock_string_at(INSTANT),
            "12:30",
            "August is EDT, UTC-4 — a fixed-offset entry would have said 11:30"
        );

        assert!(shell.datetime.set_zone(Some("Asia/Tokyo")));
        assert_eq!(
            shell.clock_string_at(INSTANT),
            "01:30",
            "UTC+9 crosses midnight into the next day"
        );
    }

    /// With no zone chosen, the clock is in the machine's own zone -- the one
    /// `date` shows. Until 2026-09-25 the default was New York for everyone,
    /// and this test asserted only that it was not UTC; what it guards now is
    /// that the zone applied is the machine's (design-decisions §875).
    #[test]
    fn the_default_shell_shows_the_machines_zone() {
        // Nothing here chooses a zone: this is the desktop as it first boots,
        // on a machine whose zone is Tokyo's.
        let mut shell = DesktopShell::new(1920, 1080);
        assert_eq!(shell.datetime.zone, None, "nothing chose a zone");
        shell.set_system_zone(tzrules::Tz::parse(b"JST-9").expect("a POSIX rule"));
        let reading = shell.clock_string_at(INSTANT);
        assert!(reading.ends_with("01:30"), "{reading}");
    }

    #[test]
    fn the_show_seconds_setting_reaches_the_taskbar_clock() {
        let mut shell = time_only_shell();
        assert!(shell.datetime.set_zone(Some("Atlantic/Reykjavik")));

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
        assert!(shell.datetime.set_zone(Some("UTC")));

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

        assert!(shell.datetime.set_zone(Some("UTC")));
        assert_eq!(shell.clock_string_at(INSTANT), "Tue Aug 18 16:30");

        // UTC+9: half past one the *next* morning.
        assert!(shell.datetime.set_zone(Some("Asia/Tokyo")));
        assert_eq!(shell.clock_string_at(INSTANT), "Wed Aug 19 01:30");
    }

    /// A zone the table does not have reads as the machine's own -- the zone
    /// `date` would show -- not as an offset invented for a name nothing here
    /// knows.
    #[test]
    fn an_unresolvable_zone_falls_back_to_the_machines_rather_than_inventing_an_offset() {
        let mut shell = time_only_shell();
        // `set_zone` validates, so reach past it — this is the state a
        // configuration file naming a zone we do not ship would produce.
        shell.datetime.zone = Some("Mars/Olympus_Mons".to_string());
        assert!(shell.datetime.current_zone().is_none());
        // The machine's zone is UTC until the session reads the real one…
        assert_eq!(shell.clock_string_at(INSTANT), "16:30");
        // … and the clock follows it when it is something else.
        shell.set_system_zone(tzrules::Tz::parse(b"JST-9").expect("a POSIX rule"));
        assert_eq!(shell.clock_string_at(INSTANT), "01:30");
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
        assert!(shell.datetime.set_zone(Some("UTC")));

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
        assert!(shell.datetime.set_zone(Some("Europe/London")));
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
        RenderTree, ShellAction, ShellControlAction, ShellRequest, TRAY_OVERFLOW_GLYPH, WindowId,
        WindowInfo, WindowList, focus_assist, notif_pane, overview, tray_dnd,
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
            text: text.map_or_else(String::new, |c| c.to_string()),
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
            s.taskbar_windows().len(),
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
        let release = |s: &mut DesktopShell| {
            s.handle_mouse(&MouseEvent {
                x,
                y,
                kind: MouseEventKind::Release(MouseButton::Left),
            })
        };
        // The control this is contrasted against: with the overview closed, the
        // same click is the taskbar's and asks for something -- on the
        // release, since a press on a window's button only takes hold.
        assert_eq!(press(&mut s, x, y), ShellAction::Consumed);
        assert!(
            matches!(release(&mut s), ShellAction::Control(_)),
            "the test's premise is wrong: that click is not a taskbar button"
        );

        s.overview.show(overview::OverviewMode::AllWindows);
        assert_eq!(press(&mut s, x, y), ShellAction::Consumed);
        assert!(
            !matches!(release(&mut s), ShellAction::Control(_)),
            "the click reached the taskbar through the overview"
        );
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

    // ======================================================================
    // The notification pane
    //
    // `notif_pane.rs` was an island: a full pane with 130 tests and nothing
    // anywhere that built one, so the shell could notice a failure and had
    // nowhere to report it. These tests are the ones that would have failed
    // while it was unreachable.
    // ======================================================================

    fn super_n() -> KeyEvent {
        key(
            Key::N,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
            None,
        )
    }

    #[test]
    fn super_n_opens_the_notification_pane_the_whole_way() {
        let mut s = shell();
        assert!(!s.notifications.pane_state().is_visible());
        assert!(s.handle_hotkey(&super_n()).consumed);
        // Not merely open: *fully* open. A shell with no frame clock — which is
        // every caller but `ShellSession` — must not be left with a pane at
        // zero progress, which draws off the right edge of the screen.
        assert_eq!(
            s.notifications.pane_state(),
            notif_pane::PaneState::Visible,
            "the pane opened but was left waiting for a frame that never comes"
        );
    }

    #[test]
    fn super_n_closes_the_pane_it_opened() {
        // A toggle you cannot press twice is a trap: while the pane is open it
        // swallows every key, so without the explicit exception the chord would
        // be one-way.
        let mut s = shell();
        assert!(s.handle_hotkey(&super_n()).consumed);
        assert!(s.handle_hotkey(&super_n()).consumed);
        assert_eq!(s.notifications.pane_state(), notif_pane::PaneState::Hidden);
    }

    #[test]
    fn an_open_pane_is_drawn_and_a_closed_one_is_not() {
        let mut s = shell();
        assert!(s.render_notifications().is_none());
        s.toggle_notifications();
        let tree = s
            .render_notifications()
            .expect("an open notification pane draws");
        assert!(!tree.commands.is_empty(), "an open pane drew no commands");
    }

    #[test]
    fn a_press_on_the_taskbar_closes_the_pane_and_is_spent_doing_so() {
        // The pane stops above the bar, so this press is a real press on the
        // start button rather than one through a scrim. It still must not open
        // the menu: dismissing is what the user aimed at, and acting as well
        // would make one click do something they could not see coming — the
        // rule every other popup in `handle_press` already follows.
        let mut s = shell();
        s.toggle_notifications();
        let action = press(&mut s, 20.0, 1080.0 - 24.0);
        assert_eq!(action, ShellAction::Consumed);
        assert!(
            !s.start_menu_open,
            "the press that closed the pane also opened a menu"
        );
        assert_eq!(s.notifications.pane_state(), notif_pane::PaneState::Hidden);
    }

    #[test]
    fn the_pane_swallows_motion_over_what_it_covers() {
        // Same rule for hover: forwarding motion would light a button under an
        // opaque overlay, which is the defect the overview arm guards against.
        let mut s = shell();
        s.toggle_notifications();
        let action = s.handle_mouse(&MouseEvent {
            x: 40.0,
            y: 40.0,
            kind: MouseEventKind::Move,
        });
        assert_eq!(action, ShellAction::Consumed);
    }

    #[test]
    fn a_key_the_pane_has_no_use_for_does_not_reach_the_desktop() {
        // Super+D shows the desktop. Pressed into an open pane it must do
        // nothing at all: a modal surface is modal about keys as well as
        // clicks.
        let mut s = shell();
        s.toggle_notifications();
        let outcome = s.handle_hotkey(&key(
            Key::D,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
            None,
        ));
        assert!(outcome.consumed);
        assert!(
            outcome.requests.is_empty(),
            "a shortcut fired through an open pane"
        );
    }

    #[test]
    fn escape_closes_the_pane() {
        let mut s = shell();
        s.toggle_notifications();
        assert!(
            s.handle_hotkey(&key(Key::Escape, Modifiers::NONE, None))
                .consumed
        );
        assert_eq!(s.notifications.pane_state(), notif_pane::PaneState::Hidden);
    }

    #[test]
    fn dismissing_popups_dismisses_the_pane() {
        let mut s = shell();
        s.toggle_notifications();
        assert!(s.dismiss_popups(), "an open pane is something to dismiss");
        assert_eq!(s.notifications.pane_state(), notif_pane::PaneState::Hidden);
    }

    #[test]
    fn opening_the_pane_closes_the_other_panels() {
        // Two panels over one taskbar at once is a state the user cannot have
        // asked for.
        let mut s = shell();
        s.toggle_start_menu();
        s.toggle_calendar();
        s.toggle_notifications();
        assert!(!s.start_menu_open);
        assert!(!s.calendar.visible);
        assert!(s.notifications.pane_state().is_visible());
    }

    #[test]
    fn a_notification_arrives_without_the_pane_being_open() {
        // The pane is the history. A message that could only arrive while
        // someone was looking is a message nobody ever reads.
        let mut s = shell();
        assert_eq!(s.notifications.unread_count(), 0);
        let _ = s.notify(notif_pane::Notification {
            id: 0,
            app_name: "Desktop".to_owned(),
            title: "Wallpaper could not be shown".to_owned(),
            body: "/pictures/torn.png: truncated IDAT".to_owned(),
            timestamp: 0,
            priority: notif_pane::NotifPriority::Normal,
            read: false,
            action: None,
            silent: false,
        });
        assert!(!s.notifications.pane_state().is_visible());
        assert_eq!(s.notifications.unread_count(), 1);
    }

    // ======================================================================
    // The tray bell
    //
    // `notif_pane`'s module doc has said since it was written that the pane is
    // opened "from the system tray or Win+N". Only the chord existed, and a
    // chord is a route nobody discovers. These are the tests that would have
    // failed while the bell was a sentence in a doc comment.
    // ======================================================================

    /// Every string the taskbar draws, in order.
    fn taskbar_text(s: &DesktopShell) -> Vec<String> {
        s.render_taskbar()
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Post `n` unread notifications from `Desktop`, carrying no action.
    fn post(s: &mut DesktopShell, n: usize) {
        for i in 0..n {
            post_from(s, "Desktop", &format!("Something {i}"), None);
        }
    }

    /// Post one notification from `app`, optionally carrying a launch path.
    ///
    /// Goes through [`DesktopShell::notify`] rather than the pane directly, so
    /// focus assist gets its say — which is the whole point in the tests below
    /// that turn it on first.
    fn post_from(s: &mut DesktopShell, app: &str, title: &str, action: Option<&str>) -> u64 {
        s.notify(notif_pane::Notification {
            id: 0,
            app_name: app.to_owned(),
            title: title.to_owned(),
            body: String::new(),
            timestamp: 0,
            priority: notif_pane::NotifPriority::Normal,
            read: false,
            action: action.map(ToOwned::to_owned),
            silent: false,
        })
    }

    /// The middle of the bell's slot.
    fn bell_centre(s: &DesktopShell) -> (f32, f32) {
        let r = s.bell_rect();
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    fn tray_icon(id: u32, glyph: &str, tooltip: &str) -> guiremote::tray::TrayIcon {
        guiremote::tray::TrayIcon {
            owner: 99,
            id,
            glyph: glyph.to_string(),
            tooltip: tooltip.to_string(),
        }
    }

    /// An icon a program registered is drawn in the taskbar.
    ///
    /// The end of the road `guiremote::tray` opened: a program asks the
    /// compositor, the compositor tells the shell, the shell draws it. This
    /// asserts the last step by reading the render tree, because every earlier
    /// step is already covered and none of them proves a pixel.
    #[test]
    fn a_registered_icon_reaches_the_taskbar() {
        let mut s = DesktopShell::new(1920, 1080);
        let before = s
            .render_taskbar()
            .commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::Text { text, .. } if text == "\u{1F50B}"))
            .count();
        assert_eq!(
            before, 0,
            "the fixture glyph must not already be on the bar"
        );

        assert!(s.apply_tray_icons(vec![tray_icon(1, "\u{1F50B}", "Battery")]));
        let after = s
            .render_taskbar()
            .commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::Text { text, .. } if text == "\u{1F50B}"))
            .count();
        assert_eq!(after, 1, "the icon the program registered was not drawn");
    }

    /// Press and release on a point, returning what the release produced.
    ///
    /// A tray icon activates on the *release*, not the press, because the
    /// press cannot yet know whether this is a click or the start of a drag.
    fn tray_click(s: &mut DesktopShell, x: f32, y: f32, button: MouseButton) -> ShellAction {
        let pressed = s.handle_mouse(&guitk::event::MouseEvent {
            x,
            y,
            kind: guitk::event::MouseEventKind::Press(button),
        });
        assert!(
            matches!(pressed, ShellAction::Consumed),
            "the press takes the grab and produces nothing: {pressed:?}"
        );
        s.handle_mouse(&guitk::event::MouseEvent {
            x,
            y,
            kind: guitk::event::MouseEventKind::Release(button),
        })
    }

    /// A tray icon for `owner`, so two programs can be told apart.
    fn owned_tray_icon(owner: u64, id: u32, glyph: &str) -> guiremote::tray::TrayIcon {
        guiremote::tray::TrayIcon {
            owner,
            id,
            glyph: glyph.to_string(),
            tooltip: format!("program {owner} icon {id}"),
        }
    }

    /// A flood of tray icons cannot take the taskbar away from the user.
    ///
    /// Measured before the cap existed, on this exact fixture: `tray_width`
    /// came to 2167 on a 1920-wide bar, so `tray_x` clamped to zero, the icon
    /// run covered the clock at x=1805, and `taskbar_button_rect(0).w` was
    /// *zero* -- no window buttons at all, on a shell whose only window
    /// switcher that is. Any program that can connect could do it; nothing
    /// rationed the strip.
    #[test]
    fn a_flood_of_tray_icons_cannot_crowd_out_the_window_buttons() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_window_list(&WindowList {
            windows: vec![placed(1, "One", 0, (0, 0, 800, 600))],
            ..WindowList::default()
        });
        let flood: Vec<_> = (1..=80).map(|i| tray_icon(i, "X", "x")).collect();
        s.apply_tray_icons(flood);

        let bar = s.taskbar_rect();
        assert!(
            s.tray_width() < bar.w,
            "the tray claimed the whole bar: {} of {}",
            s.tray_width(),
            bar.w
        );
        assert!(
            s.taskbar_button_rect(0).w > 0.0,
            "the window buttons were squeezed to nothing"
        );
        let clock = s.clock_rect();
        for rect in s.tray_icon_rects() {
            assert!(
                rect.x + rect.w <= clock.x + 0.5,
                "an icon at {} runs into the clock at {}",
                rect.x,
                clock.x
            );
        }
    }

    /// Icons that do not fit are reachable, not merely absent.
    ///
    /// A cap on its own would be the defect this whole tray road is about --
    /// a program with an icon nobody can click. The chevron is what makes the
    /// cap honest.
    #[test]
    fn icons_that_do_not_fit_are_reachable_through_the_chevron() {
        let mut s = DesktopShell::new(1920, 1080);
        let flood: Vec<_> = (1..=80).map(|i| tray_icon(i, "X", "x")).collect();
        s.apply_tray_icons(flood);

        assert!(s.tray_overflows(), "80 icons must not all fit");
        let chevron = s
            .tray_overflow_rect()
            .expect("an overflowing tray must show a chevron");
        assert!(
            s.tray_icon_rects()
                .iter()
                .all(|r| r.x >= chevron.x + chevron.w - 0.5),
            "the chevron overlaps the icon run"
        );

        let drawn = s.ordered_tray_icons().len();
        let hidden = s.overflowed_tray_icons().len();
        assert_eq!(drawn + hidden, 80, "every icon is either drawn or listed");
        assert!(drawn > 0 && hidden > 0);

        // And the chevron is drawn, not merely computed.
        let glyphs = s
            .render_taskbar()
            .commands
            .iter()
            .filter(
                |c| matches!(c, RenderCommand::Text { text, .. } if text == TRAY_OVERFLOW_GLYPH),
            )
            .count();
        assert_eq!(glyphs, 1, "the chevron was not painted");
    }

    /// Choosing a row from the overflow list clicks that program's icon.
    ///
    /// Driven from the keyboard, which is both the seam a test can reach
    /// without the menu's private layout constants and a path that has to
    /// work: a popup whose whole purpose is reaching icons too small to see
    /// is a strange place to require fine pointing.
    #[test]
    fn choosing_from_the_overflow_list_clicks_the_icon_it_names() {
        let mut s = DesktopShell::new(1920, 1080);
        let flood: Vec<_> = (1..=80)
            .map(|i| owned_tray_icon(u64::from(i), i, "X"))
            .collect();
        s.apply_tray_icons(flood);

        let wanted = tray_dnd::TrayIconKey::of(s.overflowed_tray_icons()[1]);
        let chevron = s.tray_overflow_rect().expect("overflowing");
        s.handle_mouse(&MouseEvent {
            x: chevron.x + chevron.w / 2.0,
            y: chevron.y + chevron.h / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        assert!(
            s.render_tray_overflow().is_some(),
            "the chevron did not open a list"
        );

        // Down twice to the second row, then Enter.
        drop(s.handle_hotkey(&key(Key::Down, Modifiers::default(), None)));
        drop(s.handle_hotkey(&key(Key::Down, Modifiers::default(), None)));
        let outcome = s.handle_hotkey(&key(Key::Enter, Modifiers::default(), None));

        assert!(outcome.consumed);
        match outcome.requests.as_slice() {
            [ShellRequest::ClickTrayIcon { owner, id, .. }] => {
                assert_eq!(
                    (*owner, *id),
                    (wanted.owner, wanted.id),
                    "the wrong program was told its icon was clicked"
                );
            }
            other => panic!("expected one ClickTrayIcon request, got {other:?}"),
        }
        assert!(
            s.render_tray_overflow().is_none(),
            "the list stayed open after a choice"
        );
    }

    /// Escape closes the overflow list without clicking anything.
    #[test]
    fn escape_closes_the_overflow_list_without_choosing() {
        let mut s = DesktopShell::new(1920, 1080);
        let flood: Vec<_> = (1..=80).map(|i| tray_icon(i, "X", "x")).collect();
        s.apply_tray_icons(flood);
        let chevron = s.tray_overflow_rect().expect("overflowing");
        s.handle_mouse(&MouseEvent {
            x: chevron.x + chevron.w / 2.0,
            y: chevron.y + chevron.h / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });

        let outcome = s.handle_hotkey(&key(Key::Escape, Modifiers::default(), None));

        assert!(outcome.consumed);
        assert!(outcome.requests.is_empty(), "Escape clicked something");
        assert!(s.render_tray_overflow().is_none(), "the list stayed open");
    }

    /// The bound is a width, so a narrow bar shows fewer icons than a wide one.
    ///
    /// The reason it is a share of the taskbar rather than a fixed count: a
    /// fixed twelve would still crowd a 1024-wide netbook and would waste two
    /// thirds of a 4K bar.
    #[test]
    fn a_narrower_screen_fits_fewer_tray_icons() {
        let icons: Vec<_> = (1..=80).map(|i| tray_icon(i, "X", "x")).collect();
        let mut wide = DesktopShell::new(3840, 2160);
        wide.apply_tray_icons(icons.clone());
        let mut narrow = DesktopShell::new(1024, 768);
        narrow.apply_tray_icons(icons);

        assert!(
            wide.ordered_tray_icons().len() > narrow.ordered_tray_icons().len(),
            "wide {} vs narrow {}",
            wide.ordered_tray_icons().len(),
            narrow.ordered_tray_icons().len()
        );
        assert!(narrow.tray_width() < narrow.taskbar_rect().w);
    }

    /// The overflow list fits a screen smaller than the tests' usual one.
    ///
    /// **This is the test that catches a whole family, and it is here because
    /// the family caught me.** The overflow menu shipped this morning, tested
    /// only at 1920x1080 -- and `guitk::menu` placed every popup against a
    /// hardcoded 1920x1080 of its own. The assertions and the code under test
    /// were reading the same number, so they agreed with each other and with
    /// no display but one. Measured at 1024x768 before the fix, this menu was
    /// capped to a 1080px panel, placed at y=0, and drawn to y=1080: 312
    /// pixels past the bottom, with every row below 768 unreachable.
    ///
    /// The taskbar is the worst place for it, because a popup opening from
    /// the bottom edge is the case the flip exists for.
    #[test]
    fn the_overflow_list_fits_a_screen_shorter_than_the_default() {
        let mut s = DesktopShell::new(1024, 768);
        let flood: Vec<_> = (1..=80).map(|i| tray_icon(i, "X", "x")).collect();
        s.apply_tray_icons(flood);
        let chevron = s.tray_overflow_rect().expect("overflowing");
        s.handle_mouse(&MouseEvent {
            x: chevron.x + chevron.w / 2.0,
            y: chevron.y + chevron.h / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        let tree = s.render_tray_overflow().expect("menu is up");
        let mut lowest: f32 = 0.0;
        for c in &tree.commands {
            if let RenderCommand::FillRect { y, height, .. } = c {
                lowest = lowest.max(y + height);
            }
        }
        eprintln!("screen height 768; menu extends to y={lowest}");
        assert!(lowest <= 768.0, "menu runs {lowest} past a 768px screen");
    }

    /// Move the pointer, then let the hover delay elapse.
    fn hover(s: &mut DesktopShell, x: f32, y: f32) {
        s.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Move,
        });
        // Past any plausible delay. The tooltip rides the overlay clock, so
        // this is the same call the session makes once a frame.
        s.advance_osd(5_000);
    }

    /// Resting on a tray icon names it.
    ///
    /// A tray icon is one glyph chosen by another program, and the `tooltip`
    /// it registers is the only words anywhere saying what that glyph is. The
    /// shell received that string and never showed it, so a user got a row of
    /// symbols and no way to learn what any of them were.
    #[test]
    fn resting_on_a_tray_icon_shows_the_name_its_program_gave_it() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "B", "Battery: 84%")]);
        assert!(s.render_tray_tooltip().is_none(), "nothing hovered yet");

        let rect = s.tray_icon_rects()[0];
        hover(&mut s, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);

        let tree = s
            .render_tray_tooltip()
            .expect("resting on an icon showed no tooltip");
        assert!(
            tree.commands
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Battery"))),
            "the tooltip did not carry the program's own words"
        );
    }

    /// Sliding along the row renames the tooltip rather than keeping the first.
    #[test]
    fn moving_between_tray_icons_renames_the_tooltip() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![
            tray_icon(1, "A", "Battery"),
            tray_icon(2, "N", "Network"),
        ]);
        let rects = s.tray_icon_rects();

        hover(&mut s, rects[0].x + rects[0].w / 2.0, rects[0].y + 8.0);
        let first = format!("{:?}", s.render_tray_tooltip().expect("first icon"));
        assert!(first.contains("Battery"));

        hover(&mut s, rects[1].x + rects[1].w / 2.0, rects[1].y + 8.0);
        let second = format!("{:?}", s.render_tray_tooltip().expect("second icon"));

        assert!(
            second.contains("Network") && !second.contains("Battery"),
            "the second icon still showed the first icon's name"
        );
    }

    /// Leaving the row takes the tooltip with it.
    #[test]
    fn leaving_the_tray_hides_the_tooltip() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "Battery")]);
        let rect = s.tray_icon_rects()[0];
        hover(&mut s, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        assert!(s.render_tray_tooltip().is_some());

        hover(&mut s, 40.0, 40.0);

        assert!(
            s.render_tray_tooltip().is_none(),
            "the tooltip outlived the hover"
        );
    }

    /// A program that registered no tooltip gets no empty bubble.
    #[test]
    fn an_icon_with_no_tooltip_shows_nothing() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "")]);
        let rect = s.tray_icon_rects()[0];

        hover(&mut s, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);

        assert!(s.render_tray_tooltip().is_none());
    }

    /// The tooltip waits, rather than appearing the instant the pointer
    /// crosses an icon on its way somewhere else.
    #[test]
    fn a_tooltip_does_not_appear_until_the_pointer_has_rested() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "Battery")]);
        let rect = s.tray_icon_rects()[0];

        s.handle_mouse(&MouseEvent {
            x: rect.x + rect.w / 2.0,
            y: rect.y + rect.h / 2.0,
            kind: MouseEventKind::Move,
        });
        s.advance_osd(50);

        assert!(
            s.render_tray_tooltip().is_none(),
            "the tooltip appeared after 50ms of hovering"
        );
    }

    /// The tooltip stays on the screen it was raised on.
    ///
    /// The bottom-right corner is the whole difficulty: a bubble offset down
    /// and right of the pointer there is off the display unless the flip
    /// fires, and the flip was measuring against a hardcoded 1920x1080 until
    /// an hour ago.
    #[test]
    fn a_tray_tooltip_fits_a_small_screen() {
        let mut s = DesktopShell::new(1024, 768);
        s.apply_tray_icons(vec![tray_icon(1, "A", "Battery at eighty four percent")]);
        let rect = s.tray_icon_rects()[0];
        hover(&mut s, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);

        let tree = s.render_tray_tooltip().expect("a tooltip");
        let mut plates = 0;
        for c in &tree.commands {
            if let RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                ..
            } = c
            {
                plates += 1;
                assert!(
                    x + width <= 1024.5 && y + height <= 768.5,
                    "tooltip runs to ({}, {}) on a 1024x768 screen",
                    x + width,
                    y + height
                );
            }
        }
        assert!(plates > 0, "the tooltip drew no panel to check");
    }

    /// The keyboard can reach the last notification on a screen that is not
    /// 1080 tall.
    ///
    /// The pane's scroll bound is `content - viewport`, and it learns the
    /// viewport from every *mouse* event it receives. The keyboard path
    /// carries no geometry, so until the shell started telling it, the arrows
    /// clamped against the pane's pre-first-render default of 1080 -- and on
    /// a 768px display that bound is 312 pixels short, which is the last few
    /// notifications being unreachable without a mouse.
    ///
    /// The pane's own documentation predicted this exactly: *"a shell that
    /// drives the pane from the keyboard alone should call it on resize"*.
    /// Nothing called it.
    ///
    /// Asserted by reading the render tree rather than a scroll offset,
    /// because "the last card is on screen" is the thing that was untrue and
    /// a number would need its own interpretation.
    #[test]
    fn the_keyboard_reaches_the_last_notification_on_a_short_screen() {
        let mut s = DesktopShell::new(1024, 768);
        for i in 0..40u64 {
            let _ = s.notify(notif_pane::Notification {
                id: i,
                app_name: "Desktop".to_owned(),
                title: format!("Notice {i}"),
                body: "body".to_owned(),
                timestamp: 0,
                priority: notif_pane::NotifPriority::Normal,
                read: false,
                action: None,
                silent: false,
            });
        }
        s.notifications.show();

        let drawn = |s: &DesktopShell, want: &str| {
            s.render_notifications().is_some_and(|t| {
                t.commands
                    .iter()
                    .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == want))
            })
        };
        // The newest is at the top, so the *oldest* is the one past the fold.
        assert!(!drawn(&s, "Notice 0"), "the last card is already on screen");

        // Keyboard only: no mouse event ever tells the pane the screen size.
        for _ in 0..400 {
            let _ = s.handle_hotkey(&key(Key::Down, Modifiers::default(), None));
        }

        assert!(
            drawn(&s, "Notice 0"),
            "the keyboard could not reach the last notification"
        );
    }

    /// A rule saved on disk reaches the decision that suppresses a
    /// notification.
    ///
    /// The whole chain in one test, because every link of it was built
    /// separately and each looked finished on its own: `notifications.yaml`
    /// holds the rule, `poll_notification_rules` adopts it,
    /// `focus_assist::app_priority` finds it by the name the notification
    /// carries, and `notify` acts on the answer. Before this, `app_overrides`
    /// was only ever the empty vector, so the last three links were exercised
    /// exclusively by tests that filled it in by hand.
    ///
    /// Asserted through `notify` rather than through `app_priority`, because
    /// the thing that was broken is the *chain*, and testing the far end
    /// against a hand-placed rule is how it came to be broken in the first
    /// place.
    #[test]
    fn a_rule_written_to_the_config_file_lets_that_program_through() {
        appearance::config::testing::with_scratch_config("shell-notif-rules", |_root| {
            // Written through the settings crate's own save path, so the test
            // exercises the format both ends will actually use rather than a
            // hand-typed approximation of it.
            let mut file = notifsettings::NotifFile::load();
            file.settings.set_rule(
                notifsettings::AppRule::new("Chat")
                    .with_importance(notifsettings::Importance::Critical),
            );
            file.save().expect("save the rule");

            let mut s = shell();
            s.focus.set_mode(focus_assist::FocusMode::PriorityOnly);
            assert!(
                s.poll_notification_rules(),
                "the saved rule was not picked up"
            );

            // Chat is Critical by the file, so it survives a focus mode that
            // holds back everything ordinary.
            let _ = s.notify(notif(1, "Chat"));
            assert_eq!(
                s.focus.suppressed_count, 0,
                "the rule the user saved did not let Chat through"
            );

            // Alarms has no rule, so it is Normal and PriorityOnly holds it
            // back. Without this the test would pass on a shell that
            // suppressed nothing at all.
            let _ = s.notify(notif(2, "Alarms"));
            assert_eq!(
                s.focus.suppressed_count, 1,
                "an unconfigured program was not held back, so Chat getting \
                 through proves nothing"
            );
        });
    }

    /// The same file, read by a shell that never polls, changes nothing.
    ///
    /// The negative control for the one above: it is the *adoption* that makes
    /// a saved rule take effect, and a chain that worked by accident — because
    /// `focus_assist` defaulted the way the rule happened to say — would pass
    /// the first test and fail this one.
    #[test]
    fn a_rule_no_one_read_has_no_effect() {
        appearance::config::testing::with_scratch_config("shell-notif-unread", |_root| {
            let mut file = notifsettings::NotifFile::load();
            file.settings.set_rule(
                notifsettings::AppRule::new("Chat")
                    .with_importance(notifsettings::Importance::Critical),
            );
            file.save().expect("save the rule");

            let mut s = shell();
            s.focus.set_mode(focus_assist::FocusMode::PriorityOnly);
            // Deliberately no `poll_notification_rules`.
            let _ = s.notify(notif(1, "Chat"));

            assert_eq!(
                s.focus.suppressed_count, 1,
                "Chat got through without the rule being read, so the first \
                 test is not testing the file"
            );
        });
    }

    /// A notification from `app`, with nothing else interesting about it.
    fn notif(id: u64, app: &str) -> notif_pane::Notification {
        notif_pane::Notification {
            id,
            app_name: app.to_owned(),
            title: "hello".to_owned(),
            body: String::new(),
            timestamp: 0,
            priority: notif_pane::NotifPriority::Normal,
            read: false,
            action: None,
            silent: false,
        }
    }

    /// Switching a program off in the notification pane silences it.
    ///
    /// The pane has had this switch all along and it wrote to a list of its
    /// own that nothing consulted: the decision is `focus_assist`'s and the
    /// file is `notifications.yaml`. So the control worked, looked like it
    /// worked, and did nothing -- the most expensive kind of broken, because
    /// there is nothing for a user to report.
    ///
    /// **What this covers and what it does not.** It exercises the shell's
    /// half, from the event the pane reports to the decision and the file.
    /// That the pane *emits* the event on a click is covered in `notif_pane`'s
    /// own tests, which can reach the coordinates; the pane's layout constants
    /// are private and recomputing them here would be asserting against a
    /// second copy of them. The routing between the two is a match on the
    /// event enum with no wildcard arm, so a new variant is a compile error
    /// rather than a silent drop.
    #[test]
    fn switching_a_program_off_in_the_pane_actually_silences_it() {
        appearance::config::testing::with_scratch_config("shell-pane-toggle", |_root| {
            let mut s = shell();
            assert!(
                s.focus.should_show_notification("Chat"),
                "Chat starts out able to notify"
            );

            s.apply_app_notification_setting(
                "Chat",
                notif_pane::AppSettingKind::Enabled,
                notif_pane::SettingValue::Bool(false),
            );

            assert!(
                !s.focus.should_show_notification("Chat"),
                "the switch did not reach the decision that suppresses"
            );
            assert!(
                s.focus.should_show_notification("Mail"),
                "switching Chat off silenced everything"
            );

            // And it reached the file, so the next login keeps it.
            let saved = notifsettings::NotifFile::load();
            assert_eq!(
                saved.settings.rule_for("Chat").importance,
                notifsettings::Importance::Silent,
                "the choice was applied in memory and never written"
            );
        });
    }

    /// The pane's priority control is refused rather than mistranslated.
    ///
    /// `SettingValue::Priority` carries Low/Normal/High/Urgent -- how loud one
    /// *message* is -- while a rule's importance is Silent/Normal/Priority/
    /// Critical, how far a *program* gets. There is no honest mapping, so the
    /// applier declines it. Asserted because "declines" and "has a bug" look
    /// identical from outside, and the next person to add a priority control
    /// should find this rather than a conversion someone invented.
    #[test]
    fn a_pane_priority_change_does_not_rewrite_a_rule() {
        appearance::config::testing::with_scratch_config("shell-pane-prio", |_root| {
            let mut s = shell();
            s.apply_app_notification_setting(
                "Chat",
                notif_pane::AppSettingKind::Enabled,
                notif_pane::SettingValue::Bool(false),
            );

            s.apply_app_notification_setting(
                "Chat",
                notif_pane::AppSettingKind::Priority,
                notif_pane::SettingValue::Priority(notif_pane::NotifPriority::Urgent),
            );

            assert_eq!(
                s.notif.settings.rule_for("Chat").importance,
                notifsettings::Importance::Silent,
                "a priority change overwrote the rule the user had set"
            );
        });
    }

    /// A rule changed elsewhere shows as changed in the notification pane.
    ///
    /// The pane draws a card per program, and it used to draw them from a
    /// record of its own. So a program silenced in the Settings application
    /// still showed as enabled here -- and because the pane's switch reports
    /// the value it *thinks* it is flipping from, toggling that stale card
    /// wrote "enabled" back over the user's choice. A duplicate model that is
    /// merely stale is a display bug; one the user can act on is a data loss.
    #[test]
    fn a_rule_changed_elsewhere_is_shown_in_the_pane() {
        appearance::config::testing::with_scratch_config("shell-pane-adopt", |_root| {
            let mut s = shell();
            // The pane learns about Chat the way it always does.
            let _ = s.notify(notif(1, "Chat"));
            assert_eq!(
                s.notifications.app_settings()[0].importance,
                notifsettings::Importance::Normal
            );

            // Somebody else silences it: the Settings application writing the
            // file, arriving as a reload.
            let mut file = notifsettings::NotifFile::load();
            file.settings.set_rule(
                notifsettings::AppRule::new("Chat")
                    .with_importance(notifsettings::Importance::Silent),
            );
            file.save().expect("save");
            assert!(s.poll_notification_rules(), "the change was not picked up");

            assert_eq!(
                s.notifications.app_settings()[0].importance,
                notifsettings::Importance::Silent,
                "the pane still shows the value it had before"
            );
        });
    }

    /// The pane's Night Light switch warms the screen.
    ///
    /// It used to be one of three switches the shell explicitly did nothing
    /// with, described in the code as "the compositor's gamma ramp, reached
    /// over IPC the shell does not hold yet". Night light is an appearance
    /// setting now, so the switch writes the file the compositor already
    /// reads, and the honest comment that documented a gap became a comment
    /// documenting a gap that had closed.
    ///
    /// Exercises the shell's half. The pane emits the event from a click whose
    /// coordinates need its private layout constants, and its own tests cover
    /// that; the routing between them is a match on the quick-setting enum
    /// with no wildcard, so a variant that stopped being handled is a compile
    /// error.
    #[test]
    fn the_panes_night_light_switch_reaches_the_file_the_compositor_reads() {
        appearance::config::testing::with_scratch_config("shell-quick-night", |_root| {
            let mut s = shell();
            assert!(!appearance::AppearanceFile::load().settings.night_light);

            s.toggle_night_light();

            assert!(
                appearance::AppearanceFile::load().settings.night_light,
                "the switch did not reach appearance.yaml"
            );
            assert!(
                s.take_appearance_change(),
                "the compositor was never told to re-read"
            );
            assert!(
                !s.take_appearance_change(),
                "the flag must clear, or every later pump re-notifies"
            );
        });
    }

    /// The glyphs the tray is drawing, left to right.
    fn tray_row(s: &DesktopShell) -> Vec<String> {
        s.ordered_tray_icons()
            .iter()
            .map(|i| i.glyph.clone())
            .collect()
    }

    /// Dragging an icon past its neighbour swaps them.
    ///
    /// The gesture the arrangement was built for. Until this existed the
    /// order was the shell's in name only -- correct, folded, tested, and
    /// impossible for a user to change, which is the state `apps/systray` has
    /// been in for months.
    /// The colour the tray drew `glyph` in.
    fn tray_glyph_colour(s: &DesktopShell, glyph: &str) -> super::Color {
        s.render_taskbar()
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::Text { text, color, .. } if text == glyph => Some(*color),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the tray drew no {glyph:?}"))
    }

    /// **The icon you are dragging looks like it is being dragged.**
    ///
    /// `DragSource::show_ghost` and `dragging_key` have been maintained since
    /// the module was written -- set past the threshold, cleared on drop or
    /// Escape -- and nothing read either. The icon sat at full opacity in its
    /// old slot while the insertion point moved under the pointer, which reads
    /// as "the drag did not take".
    #[test]
    fn the_dragged_tray_icon_is_drawn_faint() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "one"), tray_icon(2, "B", "two")]);

        let solid = tray_glyph_colour(&s, "A");
        let rects = s.tray_icon_rects();
        let (from, onto) = (rects[0], rects[1]);
        s.handle_mouse(&guitk::event::MouseEvent {
            x: from.x + from.w / 2.0,
            y: from.y + from.h / 2.0,
            kind: guitk::event::MouseEventKind::Press(MouseButton::Left),
        });
        // A press alone is not a drag: until the threshold is crossed this is
        // still a click, and a click must not dim anything.
        assert_eq!(
            tray_glyph_colour(&s, "A"),
            solid,
            "a press that has not moved is a click, and dimmed the icon"
        );

        s.handle_mouse(&guitk::event::MouseEvent {
            x: onto.x + onto.w * 0.9,
            y: onto.y + onto.h / 2.0,
            kind: guitk::event::MouseEventKind::Move,
        });

        let dragged = tray_glyph_colour(&s, "A");
        assert_ne!(
            dragged, solid,
            "the dragged icon is drawn exactly as before"
        );
        assert!(
            dragged.a < solid.a,
            "the ghost is not fainter: {dragged:?} vs {solid:?}"
        );
        assert_eq!(
            tray_glyph_colour(&s, "B"),
            solid,
            "the icon that is not being dragged was dimmed too"
        );
    }

    /// Letting go puts it back to full strength.
    #[test]
    fn dropping_a_tray_icon_takes_the_ghost_away() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "one"), tray_icon(2, "B", "two")]);
        let solid = tray_glyph_colour(&s, "A");

        let rects = s.tray_icon_rects();
        let (from, onto) = (rects[0], rects[1]);
        for kind in [
            guitk::event::MouseEventKind::Press(MouseButton::Left),
            guitk::event::MouseEventKind::Move,
            guitk::event::MouseEventKind::Release(MouseButton::Left),
        ] {
            let (x, y) = match kind {
                guitk::event::MouseEventKind::Press(_) => {
                    (from.x + from.w / 2.0, from.y + from.h / 2.0)
                }
                _ => (onto.x + onto.w * 0.9, onto.y + onto.h / 2.0),
            };
            s.handle_mouse(&guitk::event::MouseEvent { x, y, kind });
        }

        assert_eq!(
            tray_glyph_colour(&s, "A"),
            solid,
            "a ghost outlived the drag it belonged to"
        );
    }

    #[test]
    fn dragging_a_tray_icon_past_its_neighbour_reorders_the_row() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "one"), tray_icon(2, "B", "two")]);
        assert_eq!(tray_row(&s), vec!["A", "B"]);

        let rects = s.tray_icon_rects();
        let (from, onto) = (rects[0], rects[1]);
        s.handle_mouse(&guitk::event::MouseEvent {
            x: from.x + from.w / 2.0,
            y: from.y + from.h / 2.0,
            kind: guitk::event::MouseEventKind::Press(MouseButton::Left),
        });
        // Well past the 5px threshold, and past the midpoint of the icon to
        // the right -- which is the boundary a drop rounds to.
        s.handle_mouse(&guitk::event::MouseEvent {
            x: onto.x + onto.w * 0.9,
            y: onto.y + onto.h / 2.0,
            kind: guitk::event::MouseEventKind::Move,
        });

        assert_eq!(
            tray_row(&s),
            vec!["B", "A"],
            "the row did not shuffle under the pointer"
        );

        let released = s.handle_mouse(&guitk::event::MouseEvent {
            x: onto.x + onto.w * 0.9,
            y: onto.y + onto.h / 2.0,
            kind: guitk::event::MouseEventKind::Release(MouseButton::Left),
        });
        assert!(
            matches!(released, ShellAction::Consumed),
            "a drag must not also tell the program its icon was clicked: {released:?}"
        );
        assert_eq!(tray_row(&s), vec!["B", "A"], "and the drop kept it");
    }

    /// A press that does not move is still a click.
    ///
    /// The other half of the same decision: every click is a drag that
    /// travelled no distance, so the threshold is the only thing separating
    /// them and a tray whose icons could not be clicked would be a worse
    /// regression than one whose icons could not be dragged.
    #[test]
    fn a_tray_press_that_does_not_move_is_a_click() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "one"), tray_icon(2, "B", "two")]);
        let rect = s.tray_icon_rects()[0];

        let action = tray_click(
            &mut s,
            rect.x + rect.w / 2.0,
            rect.y + rect.h / 2.0,
            MouseButton::Left,
        );

        match action {
            ShellAction::Control(ShellRequest::ClickTrayIcon { id, .. }) => assert_eq!(id, 1),
            other => panic!("expected a ClickTrayIcon request, got {other:?}"),
        }
        assert_eq!(tray_row(&s), vec!["A", "B"], "a click is not a reorder");
    }

    /// A jitter below the threshold is a click, not a one-pixel drag.
    #[test]
    fn a_tray_press_that_wobbles_is_still_a_click() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "one")]);
        let rect = s.tray_icon_rects()[0];
        let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);

        s.handle_mouse(&guitk::event::MouseEvent {
            x,
            y,
            kind: guitk::event::MouseEventKind::Press(MouseButton::Left),
        });
        // Two pixels: the distance a hand moves pressing a button.
        s.handle_mouse(&guitk::event::MouseEvent {
            x: x + 2.0,
            y: y + 1.0,
            kind: guitk::event::MouseEventKind::Move,
        });
        let action = s.handle_mouse(&guitk::event::MouseEvent {
            x: x + 2.0,
            y: y + 1.0,
            kind: guitk::event::MouseEventKind::Release(MouseButton::Left),
        });

        assert!(
            matches!(
                action,
                ShellAction::Control(ShellRequest::ClickTrayIcon { .. })
            ),
            "a two-pixel wobble swallowed the click: {action:?}"
        );
    }

    /// The right button reaches the program that owns the icon.
    ///
    /// It did not before: `handle_press` returns early for anything that is
    /// not the primary button, so a right-click on a tray icon was consumed
    /// by the shell and the owner never heard about it -- in the one place
    /// where right-click is the whole point, since it is how every tray in
    /// existence opens an application's menu. `ClickTrayIcon` has carried a
    /// button since it was defined, and nothing could ever send a second one.
    #[test]
    fn right_clicking_a_tray_icon_reaches_its_owner_as_a_right_click() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(7, "M", "Music")]);
        let rect = s.tray_icon_rects()[0];

        let action = tray_click(
            &mut s,
            rect.x + rect.w / 2.0,
            rect.y + rect.h / 2.0,
            MouseButton::Right,
        );

        match action {
            ShellAction::Control(ShellRequest::ClickTrayIcon { id, button, .. }) => {
                assert_eq!(id, 7);
                assert_eq!(
                    button,
                    MouseButton::Right,
                    "the owner was told the wrong button"
                );
            }
            other => panic!("expected a ClickTrayIcon request, got {other:?}"),
        }
    }

    /// A program that exits mid-press is not clicked.
    ///
    /// The press named an icon; by the release that icon is gone. Resolving
    /// the click by slot index instead would have delivered it to whichever
    /// program shuffled into that position.
    #[test]
    fn an_icon_that_departs_between_press_and_release_is_not_clicked() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "one"), tray_icon(2, "B", "two")]);
        let rect = s.tray_icon_rects()[0];
        let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);

        s.handle_mouse(&guitk::event::MouseEvent {
            x,
            y,
            kind: guitk::event::MouseEventKind::Press(MouseButton::Left),
        });
        s.apply_tray_icons(vec![tray_icon(2, "B", "two")]);
        let action = s.handle_mouse(&guitk::event::MouseEvent {
            x,
            y,
            kind: guitk::event::MouseEventKind::Release(MouseButton::Left),
        });

        assert!(
            matches!(action, ShellAction::Consumed),
            "the departed program's icon was clicked anyway: {action:?}"
        );
    }

    /// The user's order outlives a program relabelling its icon.
    ///
    /// The whole reason the shell keeps an arrangement instead of drawing the
    /// compositor's list directly. The compositor re-sends every icon whenever
    /// any program touches one of its own, so without the fold a program
    /// swapping its glyph would drag every icon back to registration order --
    /// silently, and only for whoever had rearranged their tray.
    #[test]
    fn a_relabelled_icon_does_not_disturb_the_users_order() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "one"), tray_icon(2, "B", "two")]);
        s.tray_arrangement.reorder(0, 1);
        assert_eq!(
            s.ordered_tray_icons()
                .iter()
                .map(|i| i.glyph.as_str())
                .collect::<Vec<_>>(),
            vec!["B", "A"]
        );

        // Program 1 changes its glyph. The compositor sends both icons again.
        s.apply_tray_icons(vec![tray_icon(1, "!", "one"), tray_icon(2, "B", "two")]);

        assert_eq!(
            s.ordered_tray_icons()
                .iter()
                .map(|i| i.glyph.as_str())
                .collect::<Vec<_>>(),
            vec!["B", "!"],
            "the order is the user's and the glyph is the program's"
        );
    }

    /// A click lands on the icon that was drawn there, not the one the
    /// compositor listed there.
    ///
    /// The failure this catches is the worst kind the tray can have: the user
    /// presses the icon they can see and a *different* program is told it was
    /// clicked. It is possible the moment drawing and hit-testing read from
    /// two different orders, which is exactly what an index into the
    /// compositor's list would have been once the arrangement existed.
    #[test]
    fn a_click_addresses_the_icon_under_the_pointer_after_a_reorder() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(1, "A", "one"), tray_icon(2, "B", "two")]);
        s.tray_arrangement.reorder(0, 1);

        let leftmost = s.tray_icon_rects()[0];
        let action = tray_click(
            &mut s,
            leftmost.x + leftmost.w / 2.0,
            leftmost.y + leftmost.h / 2.0,
            MouseButton::Left,
        );
        match action {
            ShellAction::Control(ShellRequest::ClickTrayIcon { id, .. }) => {
                assert_eq!(id, 2, "the leftmost slot now holds program icon 2");
            }
            other => panic!("expected a ClickTrayIcon request, got {other:?}"),
        }
    }

    /// The icons widen the tray, so window buttons are not laid out underneath
    /// them.
    ///
    /// The failure this prevents is not a missing icon -- it is a taskbar
    /// button drawn in the space an icon occupies, which looks like a rendering
    /// bug anywhere except where it is.
    #[test]
    fn icons_take_room_from_the_window_buttons_rather_than_overlapping() {
        let mut s = DesktopShell::new(1920, 1080);
        let bare = s.tray_width();
        assert!(s.apply_tray_icons(vec![tray_icon(1, "A", "one"), tray_icon(2, "B", "two"),]));
        let with_icons = s.tray_width();
        assert!(
            with_icons > bare,
            "two icons did not widen the tray: {bare} then {with_icons}"
        );
    }

    /// Every icon gets a slot inside the bar, left to right, without overlap.
    #[test]
    fn each_icon_has_its_own_slot_on_the_bar() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![
            tray_icon(1, "A", "one"),
            tray_icon(2, "B", "two"),
            tray_icon(3, "C", "three"),
        ]);
        let rects = s.tray_icon_rects();
        assert_eq!(rects.len(), 3);
        let bar = s.taskbar_rect();
        for pair in rects.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(a.x + a.w <= b.x + 0.01, "icons overlap: {a:?} then {b:?}");
        }
        for r in &rects {
            assert!(
                r.x >= 0.0 && r.x + r.w <= bar.w,
                "an icon is off the bar: {r:?}"
            );
        }
    }

    /// Clicking an icon asks the compositor to tell the program that owns it.
    ///
    /// The shell is the only thing that can: it owns the strip and did the hit
    /// test. What it must not do is act on the click itself -- the icon belongs
    /// to another program, and the shell has no idea what clicking it means.
    ///
    /// The vanished-icon branch beside this one (the icon was removed between
    /// the frame that drew it and the click) is not tested here: reaching it
    /// needs a `Hit` built by hand, and `Hit` is private. It consumes rather
    /// than falling through, because the user aimed at the tray and acting on
    /// whatever is behind it would be worse than doing nothing.
    #[test]
    fn clicking_a_tray_icon_asks_the_compositor_to_tell_its_owner() {
        let mut s = DesktopShell::new(1920, 1080);
        s.apply_tray_icons(vec![tray_icon(7, "M", "Music")]);
        let rect = s.tray_icon_rects()[0];
        let action = tray_click(
            &mut s,
            rect.x + rect.w / 2.0,
            rect.y + rect.h / 2.0,
            MouseButton::Left,
        );
        match action {
            ShellAction::Control(ShellRequest::ClickTrayIcon { owner, id, .. }) => {
                assert_eq!(owner, 99, "the owner from the tray list");
                assert_eq!(id, 7, "the program's own icon id");
            }
            other => panic!("expected a ClickTrayIcon request, got {other:?}"),
        }
    }
    /// Re-applying the same list is not a change.
    ///
    /// The shell repaints on `true`, so answering it for a list identical to
    /// the one already held would repaint the desktop every time a client
    /// reconnected and was sent the list it already had.
    #[test]
    fn an_unchanged_tray_list_reports_no_change() {
        let mut s = DesktopShell::new(1920, 1080);
        let icons = vec![tray_icon(1, "A", "one")];
        assert!(
            s.apply_tray_icons(icons.clone()),
            "the first list is a change"
        );
        assert!(!s.apply_tray_icons(icons), "the same list again is not");
    }

    #[test]
    fn the_tray_draws_a_bell() {
        let s = shell();
        assert!(
            taskbar_text(&s)
                .iter()
                .any(|t| t == super::NOTIF_BELL_GLYPH),
            "the tray drew no bell"
        );
    }

    #[test]
    fn pressing_the_bell_opens_the_pane() {
        let mut s = shell();
        let (x, y) = bell_centre(&s);
        assert_eq!(press(&mut s, x, y), ShellAction::Consumed);
        assert_eq!(
            s.notifications.pane_state(),
            notif_pane::PaneState::Visible,
            "the bell opened the pane but left it waiting for a frame"
        );
    }

    #[test]
    fn pressing_the_bell_again_closes_the_pane() {
        // The reason the pane stops above the taskbar. A panel that covers the
        // button that opened it is a one-way door: the second press lands on
        // the panel's own opaque column and does nothing at all.
        let mut s = shell();
        let (x, y) = bell_centre(&s);
        let _ = press(&mut s, x, y);
        assert!(s.notifications.pane_state().is_visible());
        assert_eq!(press(&mut s, x, y), ShellAction::Consumed);
        assert_eq!(s.notifications.pane_state(), notif_pane::PaneState::Hidden);
    }

    #[test]
    fn the_pane_stops_above_the_taskbar() {
        let s = shell();
        assert_eq!(s.notification_pane_height(), s.taskbar_rect().y);
        assert!(
            s.notification_pane_height() < s.screen_height as f32,
            "a pane as tall as the display covers the bell that opens it"
        );
    }

    #[test]
    fn the_bell_and_the_clock_do_not_overlap() {
        // Both are derived from the display's right edge inwards, so this is a
        // claim about the derivation and not about one screen size: check it
        // across the widths the clock's own switches produce.
        for show_date in [false, true] {
            let mut s = shell();
            s.datetime.show_date = show_date;
            let bell = s.bell_rect();
            let clock = s.clock_rect();
            assert!(
                bell.x + bell.w <= clock.x + 0.5,
                "bell runs into the clock with show_date={show_date}"
            );
            assert!(
                bell.x >= s.tray_x() - 0.5,
                "bell sits outside the tray with show_date={show_date}"
            );
        }
    }

    #[test]
    fn the_bell_is_not_hit_tested_as_the_clock() {
        let s = shell();
        let (x, y) = bell_centre(&s);
        assert_eq!(s.hit_test(x, y), super::Hit::NotificationBell);
        let clock = s.clock_rect();
        assert_eq!(
            s.hit_test(clock.x + clock.w / 2.0, clock.y + clock.h / 2.0),
            super::Hit::Clock,
            "the bell stole the clock's slot"
        );
    }

    #[test]
    fn the_window_buttons_stop_short_of_the_bell() {
        // `taskbar_button_width` sizes the buttons from what is left after the
        // tray. A bell the tray did not account for would be drawn over by the
        // last button.
        let mut s = shell();
        s.apply_window_list(&WindowList::new(
            0,
            (1..=12)
                .map(|i| placed(i, "Window", 0, (0, 0, 400, 300)))
                .collect(),
        ));
        let bell = s.bell_rect();
        for index in 0..s.taskbar_windows().len() {
            let button = s.taskbar_button_rect(index);
            assert!(
                button.x + button.w <= bell.x + 0.5,
                "window button {index} reaches the bell"
            );
        }
    }

    #[test]
    fn an_unread_notification_puts_a_count_on_the_bell() {
        let mut s = shell();
        assert!(
            !taskbar_text(&s).iter().any(|t| t == "1"),
            "an empty history drew a badge"
        );
        post(&mut s, 1);
        assert!(
            taskbar_text(&s).iter().any(|t| t == "1"),
            "one unread notification drew no count"
        );
    }

    #[test]
    fn the_badge_stops_counting_at_nine() {
        let mut s = shell();
        post(&mut s, 40);
        let drawn = taskbar_text(&s);
        assert!(
            drawn.iter().any(|t| t == "9+"),
            "a two-digit count would not fit the slot: {drawn:?}"
        );
    }

    #[test]
    fn the_badge_does_not_move_the_tray() {
        // A count that widened the tray would shuffle every window button
        // sideways each time something was posted.
        let mut s = shell();
        let quiet = (s.tray_x(), s.clock_rect().x, s.bell_rect().x);
        post(&mut s, 40);
        assert_eq!(
            (s.tray_x(), s.clock_rect().x, s.bell_rect().x),
            quiet,
            "the badge pushed the tray"
        );
    }

    #[test]
    fn the_bell_changes_colour_when_something_is_waiting() {
        // A bell that looks the same whether or not it has anything behind it
        // is a bell nobody presses.
        fn bell_colour(s: &DesktopShell) -> super::Color {
            s.render_taskbar()
                .commands
                .iter()
                .find_map(|cmd| match cmd {
                    RenderCommand::Text { text, color, .. } if text == super::NOTIF_BELL_GLYPH => {
                        Some(*color)
                    }
                    _ => None,
                })
                .expect("the tray drew no bell")
        }
        let mut s = shell();
        let quiet = bell_colour(&s);
        post(&mut s, 1);
        assert_ne!(
            bell_colour(&s),
            quiet,
            "the bell did not react to a message"
        );
    }

    // ======================================================================
    // Focus assist
    //
    // `focus_assist.rs` was the second-largest island in the shell: 1,466
    // lines with a settings page, per-app priorities and automatic rules, and
    // nothing anywhere that constructed a `FocusAssistManager`. Meanwhile the
    // pane's Do Not Disturb switch moved on screen and changed nothing at all,
    // and every click the pane reported piled up in a `Vec` nobody emptied.
    // These are the tests that would have failed while the two were strangers.
    // ======================================================================

    /// Where the open pane draws `label`, in screen coordinates.
    ///
    /// The translations have to be replayed, not ignored: the pane emits every
    /// one of its sections in pane-local coordinates under a
    /// `PushTranslate { dx: pane_x }`, and the scrolling list adds a second one
    /// of its own. A test that read the raw `x` would get a point 380 pixels
    /// left of the pane — outside its column, where a press dismisses the pane
    /// instead of pressing anything in it.
    ///
    /// Four pixels past the text's own origin rather than at it: a quick
    /// setting's whole 36-pixel row is its hit area, and a point on the very
    /// first line of the label is a point a rounding error could put in the row
    /// above.
    fn pane_label_at(s: &DesktopShell, label: &str) -> (f32, f32) {
        let tree = s
            .render_notifications()
            .expect("the pane is not open, so it drew nothing");
        let mut stack: Vec<(f32, f32)> = Vec::new();
        let mut here = (0.0_f32, 0.0_f32);
        for cmd in &tree.commands {
            match cmd {
                RenderCommand::PushTranslate { dx, dy } => {
                    stack.push(here);
                    here = (here.0 + dx, here.1 + dy);
                }
                RenderCommand::PopTranslate => {
                    here = stack.pop().unwrap_or((0.0, 0.0));
                }
                RenderCommand::Text { x, y, text, .. } if text == label => {
                    return (here.0 + x + 4.0, here.1 + y + 4.0);
                }
                _ => {}
            }
        }
        panic!("the pane drew no {label:?}")
    }

    /// The point on the quick-setting row labelled `label` that toggles it.
    ///
    /// The pill at the right end of the row is the control and the label
    /// deliberately is not, so a test that pressed the words would press
    /// nothing. Two pixels in from the display's right edge is inside the pill
    /// whatever width the pill is given, and the pane's right edge *is* the
    /// display's whenever it is fully out — which is what `show()` makes it.
    fn quick_setting_switch_at(s: &DesktopShell, label: &str) -> (f32, f32) {
        let (_, y) = pane_label_at(s, label);
        (s.taskbar_rect().w - 2.0, y)
    }

    /// Open the pane, if it is not open, and press the quick setting `label`.
    fn toggle_quick_setting(s: &mut DesktopShell, label: &str) -> ShellAction {
        if !s.notifications.pane_state().is_visible() {
            s.toggle_notifications();
        }
        let (x, y) = quick_setting_switch_at(s, label);
        press(s, x, y)
    }

    #[test]
    fn nothing_is_silenced_to_begin_with() {
        let s = shell();
        assert_eq!(s.focus.effective_mode(), focus_assist::FocusMode::Off);
        assert!(
            taskbar_text(&s)
                .iter()
                .any(|t| t == super::NOTIF_BELL_GLYPH),
            "the tray drew something other than a plain bell with nothing silenced"
        );
    }

    #[test]
    fn pressing_do_not_disturb_in_the_pane_actually_silences_things() {
        // The defect this fixes: the switch moved and nothing else did. It was
        // a control with no wire behind it.
        let mut s = shell();
        let action = toggle_quick_setting(&mut s, "Do Not Disturb");
        assert_eq!(action, ShellAction::Consumed);
        assert_eq!(
            s.focus.effective_mode(),
            focus_assist::FocusMode::TotalSilence
        );
        assert!(
            s.notifications
                .quick_setting_value(notif_pane::QuickSetting::DoNotDisturb)
        );
    }

    #[test]
    fn pressing_do_not_disturb_again_stops_silencing_things() {
        let mut s = shell();
        let _ = toggle_quick_setting(&mut s, "Do Not Disturb");
        let _ = toggle_quick_setting(&mut s, "Do Not Disturb");
        assert_eq!(s.focus.effective_mode(), focus_assist::FocusMode::Off);
        assert!(
            !s.notifications
                .quick_setting_value(notif_pane::QuickSetting::DoNotDisturb)
        );
    }

    #[test]
    fn the_two_focus_switches_cannot_both_be_on() {
        // They are one four-valued mode spelled as two booleans. A pair that
        // could both read on would be claiming a state the manager has no way
        // to be in.
        use notif_pane::QuickSetting;
        let mut s = shell();
        let _ = toggle_quick_setting(&mut s, "Do Not Disturb");
        let _ = toggle_quick_setting(&mut s, "Focus Mode");
        assert_eq!(
            s.focus.effective_mode(),
            focus_assist::FocusMode::PriorityOnly
        );
        assert!(
            !s.notifications
                .quick_setting_value(QuickSetting::DoNotDisturb),
            "Do Not Disturb stayed on while Priority Only was in force"
        );
        assert!(s.notifications.quick_setting_value(QuickSetting::FocusMode));
    }

    #[test]
    fn alarms_only_reads_as_focus_mode_on() {
        // The one mode the pair cannot spell exactly. It must still show as
        // *something* on, because focus assist is engaged — a switch reading
        // off while notifications are being silenced is the switch lying.
        use notif_pane::QuickSetting;
        let mut s = shell();
        s.focus.set_mode(focus_assist::FocusMode::AlarmsOnly);
        s.sync_quick_settings();
        assert!(s.notifications.quick_setting_value(QuickSetting::FocusMode));
        assert!(
            !s.notifications
                .quick_setting_value(QuickSetting::DoNotDisturb)
        );
        // And pressing it turns *that* mode off, rather than swapping it for
        // the milder one the switch nominally stands for.
        let _ = toggle_quick_setting(&mut s, "Focus Mode");
        assert_eq!(s.focus.effective_mode(), focus_assist::FocusMode::Off);
    }

    #[test]
    fn the_tray_glyph_says_which_mode_is_in_force() {
        // One slot carries both "here is your history" and "here is why it has
        // been quiet". A tray that looked identical in Total Silence would
        // leave the user with no way to find out why nothing has arrived.
        let mut s = shell();
        let quiet = taskbar_text(&s);
        s.focus.set_mode(focus_assist::FocusMode::TotalSilence);
        let silenced = taskbar_text(&s);
        assert_ne!(quiet, silenced, "the tray drew the same thing either way");
        assert!(
            silenced
                .iter()
                .any(|t| t == focus_assist::FocusMode::TotalSilence.icon()),
            "the tray drew no mode glyph: {silenced:?}"
        );
    }

    #[test]
    fn a_silenced_notification_still_enters_the_history() {
        // Suppression governs attention, not the record. A user who turns Do
        // Not Disturb off has no other way to ask for the missed hour back.
        let mut s = shell();
        s.focus.set_mode(focus_assist::FocusMode::TotalSilence);
        let _ = post_from(&mut s, "Mail", "Three new messages", None);
        assert_eq!(s.notifications.notifications().len(), 1);
        assert_eq!(s.notifications.unread_count(), 1);
        assert_eq!(
            s.notifications.attention_count(),
            0,
            "a suppressed notification asked to be looked at"
        );
        assert_eq!(s.focus.suppressed_count, 1);
    }

    #[test]
    fn a_silenced_notification_does_not_badge_the_bell() {
        // The whole point of suppressing a notification is that nothing about
        // it interrupts. A badge is an interruption.
        let mut s = shell();
        s.focus.set_mode(focus_assist::FocusMode::TotalSilence);
        let _ = post_from(&mut s, "Mail", "Three new messages", None);
        let drawn = taskbar_text(&s);
        assert!(
            !drawn.iter().any(|t| t == "1"),
            "Do Not Disturb lit a badge: {drawn:?}"
        );
    }

    #[test]
    fn a_priority_app_comes_through_priority_only() {
        // Otherwise "Priority Only" would be Total Silence with a longer name.
        use focus_assist::{AppNotifOverride, NotifPriority};
        let mut s = shell();
        s.focus.set_mode(focus_assist::FocusMode::PriorityOnly);
        s.focus.set_app_override(
            AppNotifOverride::new("Alarms").with_importance(NotifPriority::Critical),
        );
        let _ = post_from(&mut s, "Mail", "Three new messages", None);
        let _ = post_from(&mut s, "Alarms", "Wake up", None);
        assert_eq!(
            s.notifications.attention_count(),
            1,
            "the wrong number of notifications got through Priority Only"
        );
        assert_eq!(s.focus.suppressed_count, 1);
    }

    #[test]
    fn turning_focus_assist_off_forgets_what_it_suppressed() {
        // The count exists to answer "you missed N things" once, when the mode
        // ends. Carrying it into the next session of Do Not Disturb would make
        // that summary a running total of everything ever silenced.
        let mut s = shell();
        s.focus.set_mode(focus_assist::FocusMode::TotalSilence);
        let _ = post_from(&mut s, "Mail", "Three new messages", None);
        assert_eq!(s.focus.suppressed_count, 1);
        s.focus.set_mode(focus_assist::FocusMode::Off);
        assert_eq!(s.focus.suppressed_count, 0);
    }

    // ---- the pane's events reach the shell ----

    #[test]
    fn the_panes_event_buffer_does_not_grow() {
        // It is an unbounded `Vec` that only `drain_events` empties, so a
        // shell that rendered the pane and never drained it grew one entry per
        // click for the life of the session.
        //
        // Driven only through the shell's own mouse path, because that is the
        // claim: a `hide()` called behind the shell's back would report a
        // `Closed` the shell never had the chance to drain, and the test would
        // be about the pane rather than about the wiring.
        //
        // Scratch-wrapped since the Night Light switch became real: flipping
        // it writes `appearance.yaml`, so a test that presses it five times
        // now edits the developer's own settings. The switch it drives was
        // inert when this test was written, which is the general shape --
        // wiring a dead control makes tests that had been pressing it start
        // having effects.
        appearance::config::testing::with_scratch_config("shell-pane-buffer", |_root| {
            let mut s = shell();
            for _ in 0..5 {
                let _ = toggle_quick_setting(&mut s, "Night Light");
                let (bx, by) = bell_centre(&s);
                let _ = press(&mut s, bx, by);
            }
            assert!(
                s.notifications.drain_events().is_empty(),
                "the shell left events in the pane"
            );
        });
    }

    #[test]
    fn closing_the_pane_with_escape_is_drained_too() {
        // `hide()` reports `Closed`, so a session driven only from the
        // keyboard would grow one entry per press.
        let mut s = shell();
        for _ in 0..5 {
            s.toggle_notifications();
            let _ = s.handle_hotkey(&key(Key::Escape, Modifiers::NONE, None));
        }
        assert!(
            s.notifications.drain_events().is_empty(),
            "the key path left events in the pane"
        );
    }

    #[test]
    fn clicking_a_notification_that_names_a_program_launches_it() {
        // The `action` field had no reader anywhere in the tree: a card could
        // say what it was about and clicking it did nothing but mark it read.
        let mut s = shell();
        let _ = post_from(&mut s, "Mail", "Three new messages", Some("/apps/mail"));
        s.toggle_notifications();
        let (x, y) = pane_label_at(&s, "Three new messages");
        assert_eq!(
            press(&mut s, x, y),
            ShellAction::Launch(crate::hotkeys::Launch::program("/apps/mail"))
        );
    }

    #[test]
    fn clicking_a_notification_that_names_nothing_is_merely_consumed() {
        // Most notifications are messages, and reading one is the whole of the
        // interaction. Launching *something* would be worse than launching
        // nothing.
        let mut s = shell();
        let _ = post_from(&mut s, "Mail", "Three new messages", None);
        s.toggle_notifications();
        let (x, y) = pane_label_at(&s, "Three new messages");
        assert_eq!(press(&mut s, x, y), ShellAction::Consumed);
    }
}

/// The Run box, wired to the shell.
///
/// `run_dialog.rs` was 2,000 lines with its own text field, its own history and
/// its own autocomplete, and nothing anywhere constructed a `RunDialog`: the
/// shell had no chord that opened it, no surface that drew it, and no way at
/// all to hear that a command had been confirmed. These are the tests that
/// would have failed while the two were strangers.
///
/// The box's *own* behaviour — editing, history, resolution, the autocomplete
/// list — is tested in `run_dialog.rs` beside the code that implements it. What
/// is tested here is only the seam: that a chord opens it, that while it is up
/// it owns the keyboard, that a confirmed command comes back out as a launch,
/// and that dismissing it leaves nothing behind.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]
mod run_box_wiring_tests {
    use super::{
        DesktopShell, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind, Path,
        PathBuf, ShellAction, notif_pane,
    };
    use guitk::render::RenderCommand;

    fn shell() -> DesktopShell {
        DesktopShell::new(1920, 1080)
    }

    fn chord(k: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        }
    }

    /// Super+R — the chord that opens and closes the Run box.
    fn super_r() -> KeyEvent {
        chord(
            Key::R,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        )
    }

    const LETTERS: [Key; 26] = [
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
    ];

    /// The keystroke a real keyboard sends for a lower-case character.
    ///
    /// The identity is carried as well as the text, rather than typing
    /// everything through `Key::Unknown`: the dialog's key handler dispatches on
    /// `event.key` for eight of its arms, and a test that never sent a real
    /// letter could not tell a shell that had accidentally bound one from a
    /// shell that had not.
    fn typed(ch: char) -> KeyEvent {
        let index = (ch as u32).wrapping_sub('a' as u32) as usize;
        let key = match ch {
            '/' => Key::Slash,
            _ => *LETTERS
                .get(index)
                .unwrap_or_else(|| panic!("no key for {ch:?}; the helper only knows a-z and /")),
        };
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        }
    }

    /// Type a command into the open box, one keystroke at a time.
    ///
    /// Through `handle_hotkey`, never through `run_dialog.handle_key_event`
    /// directly: the claim under test is that the shell *routes* a printable
    /// key to the box, and a test that reached past the shell would still pass
    /// on a shell that routed it to the binding table instead.
    fn type_command(s: &mut DesktopShell, command: &str) -> Vec<PathBuf> {
        let mut launches = Vec::new();
        for ch in command.chars() {
            launches.extend(s.handle_hotkey(&typed(ch)).launches);
        }
        programs(&launches)
    }

    /// The keystroke for any character a command line holds: a real key for
    /// what [`typed`] knows and for the space, and for everything else --
    /// capitals, quotes, a drive letter's colon -- a key the shell binds to
    /// nothing, carrying the character as its text, which is what the box
    /// inserts. The keys that matter to the box's own arms (Enter, the
    /// arrows) are never typed through here.
    fn typed_any(ch: char) -> KeyEvent {
        match ch {
            'a'..='z' | '/' => typed(ch),
            _ => KeyEvent {
                key: if ch == ' ' {
                    Key::Space
                } else {
                    Key::Unknown(0)
                },
                pressed: true,
                modifiers: Modifiers::NONE,
                text: ch.to_string(),
            },
        }
    }

    /// Type a line into the open box and press Enter, answering the launches
    /// it asked for -- program and arguments both.
    fn run_line(s: &mut DesktopShell, line: &str) -> Vec<crate::hotkeys::Launch> {
        for ch in line.chars() {
            drop(s.handle_hotkey(&typed_any(ch)));
        }
        s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE))
            .launches
    }

    /// **The Run box runs a program with its arguments.** It used to ask for
    /// one program named by the whole line -- a file called
    /// `terminal --title "two words"`, which cannot exist.
    #[test]
    fn the_run_box_runs_a_program_with_its_arguments() {
        let mut s = shell();
        drop(s.handle_hotkey(&super_r()));
        assert!(s.run_dialog.is_visible());
        let launches = run_line(&mut s, "terminal --title \"two words\"");
        assert_eq!(
            launches,
            [crate::hotkeys::Launch {
                program: PathBuf::from("terminal"),
                args: vec!["--title".into(), "two words".into()],
            }]
        );
    }

    /// **The Run box opens a folder, in the file manager -- spaces and all,
    /// with no quotes**, because the whole line is tried as a path before
    /// anything is split. It used to ask for the folder to be executed.
    #[test]
    fn the_run_box_opens_a_folder_whose_name_has_a_space() {
        appearance::config::testing::with_scratch_config("run-box-folder", |root| {
            let folder = root.join("My Stuff");
            std::fs::create_dir(&folder).expect("the scratch root is writable");
            let line = folder.to_str().expect("a scratch path is text").to_string();
            let mut s = shell();
            drop(s.handle_hotkey(&super_r()));
            let launches = run_line(&mut s, &line);
            assert_eq!(
                launches,
                [crate::hotkeys::Launch::opening(
                    crate::launcher::FILE_MANAGER,
                    &folder
                )]
            );
        });
    }

    /// **The Run box opens a document in the program chosen for it**, by the
    /// rule a double-click uses; and a document nothing opens starts nothing
    /// and says why.
    #[test]
    fn the_run_box_opens_a_document_by_the_desktops_rules() {
        appearance::config::testing::with_scratch_config("run-box-document", |root| {
            let mut doc = yamldoc::Document::new();
            doc.set_str(&[associations::ASSOCIATIONS, "md"], "/usr/bin/editor");
            appearance::config::store(associations::CONFIG_NAME, &doc)
                .expect("the scratch config directory is writable");
            let readme = root.join("readme.md");
            std::fs::write(&readme, b"# hi").expect("write");
            let odd = root.join("data.qqq");
            std::fs::write(&odd, b"?").expect("write");

            let mut s = shell();
            drop(s.handle_hotkey(&super_r()));
            let line = readme.to_str().expect("text").to_string();
            assert_eq!(
                run_line(&mut s, &line),
                [crate::hotkeys::Launch::opening("/usr/bin/editor", &readme)]
            );

            drop(s.handle_hotkey(&super_r()));
            let line = odd.to_str().expect("text").to_string();
            assert!(
                run_line(&mut s, &line).is_empty(),
                "nothing opens it, so nothing starts"
            );
            assert!(
                s.notifications
                    .notifications()
                    .iter()
                    .any(|n| n.title.starts_with("Cannot open")),
                "and it said so"
            );
        });
    }

    /// The programs a batch of launches names, without their arguments.
    ///
    /// Most of these tests are about *which program* was named -- several
    /// about naming it byte-exactly -- and the Run box passes no arguments, so
    /// asserting on the program keeps them saying what they were written to
    /// say. The arguments have tests of their own; see
    /// `the_screenshot_shortcut_passes_its_mode_as_an_argument`.
    fn programs(launches: &[crate::hotkeys::Launch]) -> Vec<PathBuf> {
        launches.iter().map(|l| l.program.clone()).collect()
    }

    /// **A screenshot shortcut asks for a program, and tells it which mode.**
    ///
    /// The two screenshot actions are one program invoked two ways, and until
    /// 2026-09-17 the difference was carried inside the path:
    /// `SCREENSHOT_COMMAND` was `"/usr/bin/screenshot --fullscreen"`, turned
    /// into one `PathBuf` and handed to `Command::new`. That asks the system
    /// for a file with a space and two dashes in its name, so **both
    /// screenshot shortcuts failed at every press** -- and failed by printing
    /// "cannot start", which reads like a program that is not installed rather
    /// than a request that was never well formed.
    ///
    /// Asserted on the argument and not merely on the program: with the
    /// argument dropped the two actions become the same launch, which is the
    /// other way to get this wrong.
    #[test]
    fn the_screenshot_shortcut_passes_its_mode_as_an_argument() {
        let full = crate::hotkeys::HotkeyAction::Screenshot
            .launch()
            .expect("the screenshot action starts a program");
        let region = crate::hotkeys::HotkeyAction::ScreenshotRegion
            .launch()
            .expect("the region action starts a program");

        assert_eq!(
            full.program,
            PathBuf::from("/usr/bin/screenshot"),
            "the program is not a file name any filesystem could hold"
        );
        assert_eq!(full.program, region.program, "two programs, not two modes");
        assert_eq!(full.args, [std::ffi::OsString::from("--fullscreen")]);
        assert_eq!(region.args, [std::ffi::OsString::from("--region")]);
        assert_ne!(
            full, region,
            "the two shortcuts became the same launch, so one of them is unreachable"
        );
    }

    /// A program named with no arguments is launched with none.
    ///
    /// The start menu and the Run box name programs rather than invocations,
    /// and an empty `args` is what says so. A default of "whatever was last
    /// set" would be the kind of leak that only shows up on the second press.
    #[test]
    fn an_action_that_names_only_a_program_carries_no_arguments() {
        let lock = crate::hotkeys::HotkeyAction::ScreenLock
            .launch()
            .expect("the lock action starts a program");
        assert_eq!(lock.program, PathBuf::from(crate::hotkeys::LOCK_COMMAND));
        assert!(
            lock.args.is_empty(),
            "arguments appeared for an action that names none: {:?}",
            lock.args
        );
    }

    fn press(s: &mut DesktopShell, x: f32, y: f32) -> ShellAction {
        s.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    /// The middle of the box's own background rectangle, in screen coordinates.
    ///
    /// Read out of the render rather than computed from the layout constants,
    /// which are private to `run_dialog` and should stay that way: a test that
    /// knew the box was 450×180 would be a test that agreed with a stale copy
    /// of the number rather than with the box that was drawn.
    fn box_rect(s: &DesktopShell) -> (f32, f32, f32, f32) {
        let tree = s.render_run_dialog().expect("the box is not open");
        tree.commands
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
            .expect("the open box drew no background")
    }

    /// The middle of the button carrying `label`.
    ///
    /// A button is a `FillRect` followed by the `Text` that names it, so the
    /// label is what identifies the rectangle: the geometry is
    /// `render_button`'s business, and this finds whichever rectangle it in
    /// fact drew.
    fn button_centre(s: &DesktopShell, label: &str) -> (f32, f32) {
        let tree = s.render_run_dialog().expect("the box is not open");
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
                RenderCommand::Text { text, .. } if text == label => {
                    let (x, y, w, h) = last_rect.expect("a label with no button under it");
                    return (x + w / 2.0, y + h / 2.0);
                }
                _ => {}
            }
        }
        panic!("the box drew no {label:?} button");
    }

    // ---- opening and closing ----

    #[test]
    fn super_r_opens_the_box_and_a_second_press_closes_it() {
        let mut s = shell();
        assert!(!s.run_dialog.is_visible(), "the box starts out of the way");

        let outcome = s.handle_hotkey(&super_r());
        assert!(
            outcome.consumed,
            "the chord was passed to the focused window"
        );
        assert!(s.run_dialog.is_visible(), "Super+R did not open the box");

        // The second press has to reach the binding table rather than the box,
        // which is the one-way-toggle exception the overview and the pane also
        // carry — otherwise the chord that opens it cannot close it.
        let outcome = s.handle_hotkey(&super_r());
        assert!(outcome.consumed);
        assert!(
            !s.run_dialog.is_visible(),
            "Super+R would not close the box"
        );
        assert!(
            outcome.launches.is_empty(),
            "closing the box started a program"
        );
    }

    #[test]
    fn an_open_box_is_drawn_and_a_closed_one_is_not() {
        let mut s = shell();
        assert!(s.render_run_dialog().is_none());
        s.toggle_run_dialog();
        let tree = s.render_run_dialog().expect("an open box draws");
        assert!(!tree.commands.is_empty(), "an open box drew no commands");
    }

    #[test]
    fn the_box_is_centred_on_the_screen_it_opens_on() {
        let mut s = shell();
        s.toggle_run_dialog();
        let (x, y, w, h) = box_rect(&s);
        assert_eq!(
            (x + w / 2.0, y + h / 2.0),
            (960.0, 540.0),
            "the box opened off-centre"
        );
    }

    /// A box positioned once at construction would be centred on the display the
    /// session started with and nowhere near the middle of the one the user is
    /// looking at after a mode change.
    #[test]
    fn the_box_follows_a_display_that_changed_size() {
        let mut s = shell();
        s.toggle_run_dialog();
        s.toggle_run_dialog();

        s.screen_width = 1280;
        s.screen_height = 720;
        s.toggle_run_dialog();
        let (x, y, w, h) = box_rect(&s);
        assert_eq!(
            (x + w / 2.0, y + h / 2.0),
            (640.0, 360.0),
            "the box was centred on the display the shell booted with"
        );
    }

    /// A screen smaller than the box is not a real display, but it is a
    /// plausible transient during a mode change, and the box's *left* edge is
    /// the half that carries the title — so it must not be the half pushed off.
    #[test]
    fn a_screen_narrower_than_the_box_still_shows_its_top_left() {
        let mut s = shell();
        s.screen_width = 100;
        s.screen_height = 40;
        s.toggle_run_dialog();
        let (x, y, _, _) = box_rect(&s);
        assert_eq!((x, y), (0.0, 0.0));
    }

    #[test]
    fn opening_the_box_puts_the_other_popups_away() {
        // Modal about keys, so anything else with a text field in it is a
        // surface the user can see and cannot type into.
        let mut s = shell();
        s.start_menu_open = true;
        s.toggle_notifications();
        s.toggle_run_dialog();
        assert!(
            !s.start_menu_open,
            "the start menu stayed open under the box"
        );
        assert_eq!(
            s.notifications.pane_state(),
            notif_pane::PaneState::Hidden,
            "the notification pane stayed open under the box"
        );
    }

    // ---- modal about keys ----

    #[test]
    fn a_shortcut_pressed_into_the_box_does_not_act_on_the_desktop() {
        // Super+D shows the desktop. Pressed into the box it must do nothing at
        // all: minimising every window to type a command is not what the user
        // asked for, and the box would be left floating over a bare desktop.
        let mut s = shell();
        s.toggle_run_dialog();
        let outcome = s.handle_hotkey(&chord(
            Key::D,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ));
        assert!(outcome.consumed);
        assert!(
            outcome.requests.is_empty(),
            "a shortcut fired through the open box"
        );
        assert!(s.run_dialog.is_visible(), "a stray chord closed the box");
    }

    /// The media keys are bound modifier-agnostically — a key with one meaning
    /// and no other job is not a chord — which makes them the binding most
    /// likely to fire through a modal surface.
    #[test]
    fn a_media_key_pressed_into_the_box_does_not_move_the_volume() {
        let mut s = shell();
        let level = s.notifications.volume();
        s.toggle_run_dialog();
        let outcome = s.handle_hotkey(&chord(Key::VolumeUp, Modifiers::NONE));
        assert!(outcome.consumed);
        assert_eq!(
            s.notifications.volume(),
            level,
            "the volume moved while the user was typing a command"
        );
    }

    #[test]
    fn escape_closes_the_box_and_starts_nothing() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "terminal");
        let outcome = s.handle_hotkey(&chord(Key::Escape, Modifiers::NONE));
        assert!(outcome.consumed);
        assert!(!s.run_dialog.is_visible(), "Escape left the box up");
        assert!(
            outcome.launches.is_empty(),
            "cancelling started the command anyway"
        );
    }

    /// The box empties itself on every opening, so a command abandoned with
    /// Escape must not be sitting in the field the next time it is opened —
    /// where the next Enter would start it.
    #[test]
    fn an_abandoned_command_is_not_still_there_next_time() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "terminal");
        let _ = s.handle_hotkey(&chord(Key::Escape, Modifiers::NONE));

        s.toggle_run_dialog();
        let outcome = s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE));
        assert!(
            outcome.launches.is_empty(),
            "Enter on an empty box started the command typed before it"
        );
        assert!(
            s.run_dialog.is_visible(),
            "Enter on an empty box closed it, so there is no way to type"
        );
    }

    // ---- a confirmed command is a launch ----

    #[test]
    fn a_command_typed_and_confirmed_comes_back_out_as_a_launch() {
        let mut s = shell();
        s.toggle_run_dialog();
        assert!(
            type_command(&mut s, "terminal").is_empty(),
            "a keystroke that only typed a letter started a program"
        );

        let outcome = s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE));
        assert_eq!(programs(&outcome.launches), [PathBuf::from("terminal")]);
        assert!(
            !s.run_dialog.is_visible(),
            "the box stayed up after starting the command"
        );
    }

    /// The launch channel carries what the box *resolved*, not whatever was in
    /// the field: a shell that forwarded the raw text would ask the process
    /// server to start things that do not exist, and the user would get no
    /// error at all — the box would simply close.
    #[test]
    fn a_command_the_box_cannot_resolve_is_not_a_launch() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "no/such");
        let outcome = s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE));
        assert!(
            outcome.launches.is_empty(),
            "an unresolvable name was started"
        );
        assert!(
            s.run_dialog.is_visible(),
            "the box closed on a name it had rejected, so its error is unreadable"
        );
    }

    #[test]
    fn the_ok_button_starts_the_command_the_way_enter_does() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "terminal");
        let (x, y) = button_centre(&s, "OK");
        assert_eq!(
            press(&mut s, x, y),
            ShellAction::Launch(crate::hotkeys::Launch::program("terminal"))
        );
    }

    #[test]
    fn the_cancel_button_closes_the_box_without_starting_anything() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "terminal");
        let (x, y) = button_centre(&s, "Cancel");
        assert_eq!(press(&mut s, x, y), ShellAction::Consumed);
        assert!(!s.run_dialog.is_visible());
    }

    // ---- Browse ----

    /// A name the filesystem accepts and UTF-8 cannot spell.
    ///
    /// Built through the platform's own safe constructor rather than by
    /// asserting bytes into an `OsStr`: a lone high surrogate is a perfectly
    /// legal Windows filename with no UTF-8 spelling, and byte `0xFF` is the
    /// same thing everywhere else. SlateOS filenames admit every byte but `/`
    /// and NUL, so this is an ordinary name on the target, not a pathological
    /// one.
    fn unmappable_name() -> std::ffi::OsString {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt as _;
            std::ffi::OsString::from_wide(&[u16::from(b'z'), 0xD800])
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::ffi::OsStringExt as _;
            std::ffi::OsString::from_vec(vec![b'z', 0xFF])
        }
    }

    /// One ordinary file, named `name`, as a listing.
    fn listing(name: std::ffi::OsString) -> Vec<guitk::dialog::DirEntry> {
        vec![guitk::dialog::DirEntry {
            name,
            is_dir: false,
            size: 1,
            modified_timestamp: 0,
            extension: std::ffi::OsString::new(),
        }]
    }

    /// Press Browse and answer the listing it asks for, leaving the chooser up
    /// showing one file called `name`.
    fn browse_showing(s: &mut DesktopShell, name: std::ffi::OsString) {
        let (x, y) = button_centre(s, "Browse...");
        assert_eq!(press(s, x, y), ShellAction::Consumed);
        assert!(s.run_browser_open(), "Browse put no chooser up");
        let wanted = s
            .run_browser_wants()
            .expect("the chooser asked for no listing")
            .to_path_buf();
        assert_eq!(wanted, PathBuf::from("/"), "an empty box browses the root");
        s.set_run_browser_entries(listing(name));
        assert!(
            s.run_browser_wants().is_none(),
            "the chooser asked again for a listing it had just been given"
        );
    }

    /// The button used to do nothing at all: it reported an intent the shell
    /// dropped on the floor, because a chooser needs directory entries and this
    /// module reads no files. It still reads no files — the listing arrives
    /// through `set_run_browser_entries` — but the chooser is now real. See
    /// known-issues.md → `TD-C-THE-RUN-BOX-BROWSE-BUTTON-HAS-NOWHERE-TO-GO`.
    #[test]
    fn the_browse_button_puts_a_chooser_up_and_leaves_the_box_under_it() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "term");
        browse_showing(&mut s, std::ffi::OsString::from("hello"));
        assert!(
            s.run_dialog.is_visible(),
            "raising a chooser threw the typed command away"
        );
        assert!(
            s.render_run_browser().is_some(),
            "a chooser that is up draws nothing"
        );
    }

    /// A press outside the chooser is swallowed, not obeyed — deliberately
    /// unlike the Run box underneath it, which a press outside *does* dismiss.
    /// Dismissing the box costs a line of typing; dismissing the chooser costs
    /// however deep the user had navigated, and there is no way back but to do
    /// it again. See `design-decisions.md` §809 Decision 2.
    #[test]
    fn a_press_outside_the_chooser_neither_closes_it_nor_reaches_the_desktop() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "terminal");
        browse_showing(&mut s, std::ffi::OsString::from("hello"));

        // The chooser is centred, so the screen's top-left corner is outside it
        // under any layout it could plausibly have.
        assert_eq!(
            press(&mut s, 1.0, 1.0),
            ShellAction::Consumed,
            "a press past the chooser fell through to whatever is underneath"
        );
        assert!(
            s.run_browser_open(),
            "a press past the chooser took it down"
        );
        assert!(
            s.run_dialog.is_visible(),
            "a press past the chooser took the box down with it"
        );
    }

    /// The whole reason the Browse path carries a `PathBuf` and not a `String`.
    /// A user who *points at* a program is pointing at a specific file, and the
    /// lossy rendering the command field can show names a different file — or,
    /// far more often, none at all.
    #[test]
    fn choosing_a_name_that_is_not_utf8_starts_that_exact_file() {
        let mut s = shell();
        s.toggle_run_dialog();
        let name = unmappable_name();
        browse_showing(&mut s, name.clone());

        // Down then Enter: the keyboard route, which is also what proves the
        // chooser is getting the keys rather than the box underneath it.
        assert!(s.handle_hotkey(&chord(Key::Down, Modifiers::NONE)).consumed);
        assert!(
            s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE))
                .launches
                .is_empty(),
            "choosing a file in the chooser started it, instead of naming it"
        );
        assert!(
            !s.run_browser_open(),
            "the chooser stayed up after choosing"
        );

        let mut expected = std::ffi::OsString::from("/");
        expected.push(&name);
        let outcome = s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE));
        assert_eq!(
            programs(&outcome.launches),
            [PathBuf::from(&expected)],
            "the launch named a lossy rendering rather than the file that was picked"
        );
    }

    /// The other half of the same claim: a file that was pointed at once has to
    /// be re-runnable from the history, which means the history has to hold its
    /// bytes rather than the `U+FFFD` rendering the command field shows. Held
    /// as text, pressing Up and Enter asks for a file that does not exist — and
    /// asks *silently*, because `resolve_command` waves any leading `/`
    /// through without checking it.
    #[test]
    fn a_browsed_name_that_is_not_utf8_survives_a_trip_through_the_history() {
        let mut s = shell();
        s.toggle_run_dialog();
        let name = unmappable_name();
        browse_showing(&mut s, name.clone());

        let mut expected = std::ffi::OsString::from("/");
        expected.push(&name);

        // Choose it in the chooser, then run it.
        let _ = s.handle_hotkey(&chord(Key::Down, Modifiers::NONE));
        let _ = s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE));
        assert_eq!(
            programs(
                &s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE))
                    .launches
            ),
            [PathBuf::from(&expected)]
        );

        // Open the box again — `show` empties the field — and recall it.
        s.toggle_run_dialog();
        assert!(s.run_dialog.is_visible(), "the box did not reopen");
        assert!(s.handle_hotkey(&chord(Key::Up, Modifiers::NONE)).consumed);
        assert_eq!(
            programs(
                &s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE))
                    .launches
            ),
            [PathBuf::from(&expected)],
            "the recalled command named a lossy rendering rather than the file that ran"
        );
    }

    /// Cancelling has to be free. A Browse that cost the user the line they had
    /// typed is a Browse nobody presses twice.
    #[test]
    fn cancelling_the_chooser_leaves_the_typed_command_alone() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "terminal");
        browse_showing(&mut s, std::ffi::OsString::from("hello"));
        assert!(
            s.handle_hotkey(&chord(Key::Escape, Modifiers::NONE))
                .consumed
        );
        assert!(!s.run_browser_open(), "Escape left the chooser up");
        assert!(
            s.run_dialog.is_visible(),
            "Escape closed the box as well as the chooser"
        );

        let outcome = s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE));
        assert_eq!(
            programs(&outcome.launches),
            [PathBuf::from("terminal")],
            "cancelling the chooser edited the command that was already typed"
        );
    }

    /// The chooser is modal over the box the way the box is modal over the
    /// desktop. Without that, the arrows walking the file list would walk the
    /// box's history at the same time, and a letter would be typed into a field
    /// the user cannot see.
    #[test]
    fn keys_reach_the_chooser_and_not_the_box_underneath_it() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "term");
        browse_showing(&mut s, std::ffi::OsString::from("hello"));

        assert!(s.handle_hotkey(&typed('x')).consumed);
        assert!(s.run_browser_open(), "a letter dismissed the chooser");
        let _ = s.handle_hotkey(&chord(Key::Escape, Modifiers::NONE));

        let outcome = s.handle_hotkey(&chord(Key::Enter, Modifiers::NONE));
        assert_eq!(
            programs(&outcome.launches),
            [PathBuf::from("term")],
            "a key aimed at the chooser was typed into the box behind it"
        );
    }

    /// Browsing from a path already in the box opens where that path lives,
    /// not at the root. A chooser that always started from `/` would make
    /// correcting a typo in a long path a fresh walk down the tree.
    #[test]
    fn the_chooser_opens_in_the_directory_of_what_is_typed() {
        let mut s = shell();
        s.toggle_run_dialog();
        let _ = type_command(&mut s, "/usr/bin/term");
        let (x, y) = button_centre(&s, "Browse...");
        assert_eq!(press(&mut s, x, y), ShellAction::Consumed);
        assert_eq!(
            s.run_browser_wants(),
            Some(Path::new("/usr/bin")),
            "the chooser opened somewhere other than where the command points"
        );
    }

    /// Dismissing the box takes the chooser with it. A chooser standing over a
    /// box that is no longer there would own the keyboard with nothing to
    /// return its answer to.
    #[test]
    fn dismissing_the_box_takes_the_chooser_down_too() {
        let mut s = shell();
        s.toggle_run_dialog();
        browse_showing(&mut s, std::ffi::OsString::from("hello"));
        s.dismiss_popups();
        assert!(!s.run_browser_open(), "the chooser outlived the box");
        assert!(!s.run_dialog.is_visible());
    }

    // ---- the mouse ----

    #[test]
    fn a_press_outside_the_box_closes_it() {
        let mut s = shell();
        s.toggle_run_dialog();
        assert_eq!(press(&mut s, 4.0, 4.0), ShellAction::Consumed);
        assert!(!s.run_dialog.is_visible(), "a click on the desktop missed");
    }

    /// The one event the box does not claim. Consuming motion as well would
    /// freeze every hover highlight on the desktop for as long as the box is
    /// up — including the taskbar's, which is nowhere near it.
    #[test]
    fn motion_outside_the_box_is_passed_on() {
        let mut s = shell();
        s.toggle_run_dialog();
        let action = s.handle_mouse(&MouseEvent {
            x: 4.0,
            y: 4.0,
            kind: MouseEventKind::Move,
        });
        assert_ne!(
            action,
            ShellAction::Consumed,
            "the box swallowed a move it was nowhere near"
        );
        assert!(s.run_dialog.is_visible(), "a hover dismissed the box");
    }

    // ---- nothing is left behind ----

    #[test]
    fn the_boxs_event_buffer_does_not_grow() {
        // It is an unbounded `Vec` that only `drain_events` empties, and
        // `hide()` posts a `Closed` into it unconditionally — so every one of
        // the four ways the box can be dismissed has to be drained, not just
        // the one that produces an `Execute`.
        let mut s = shell();
        for _ in 0..5 {
            let _ = s.handle_hotkey(&super_r());
            let _ = s.handle_hotkey(&super_r());

            let _ = s.handle_hotkey(&super_r());
            let _ = s.handle_hotkey(&chord(Key::Escape, Modifiers::NONE));

            let _ = s.handle_hotkey(&super_r());
            let _ = press(&mut s, 4.0, 4.0);

            let _ = s.handle_hotkey(&super_r());
            s.dismiss_popups();
        }
        assert!(
            s.run_dialog.drain_events().is_empty(),
            "the shell left events in the box"
        );
    }

    /// `dismiss_popups` runs on the way *into* the box as well as on Escape, so
    /// an unguarded `hide()` there would post a `Closed` for a box that was
    /// never open — one per opening, for the life of the session.
    #[test]
    fn dismissing_popups_that_are_not_open_reports_nothing() {
        let mut s = shell();
        for _ in 0..5 {
            s.dismiss_popups();
        }
        assert!(s.run_dialog.drain_events().is_empty());
    }

    #[test]
    fn an_open_box_counts_as_a_popup() {
        // `any_popup_open` is what decides whether Escape is worth grabbing
        // from every other window on the desktop. A box missing from it is a
        // box that cannot be cancelled from the keyboard in a live session.
        let mut s = shell();
        assert!(!s.any_popup_open());
        s.toggle_run_dialog();
        assert!(s.any_popup_open(), "the open box is not a popup");
    }

    // ------------------------------------------------------------------
    // Rebinding a shortcut from the card
    //
    // The card could be opened and read and nothing else; the only way to
    // move a shortcut was to edit a file by hand. These cover the two halves:
    // walking the rows, and recording a chord -- where the hard part is that
    // the keystroke must *not* do what it normally does.
    // ------------------------------------------------------------------

    fn card_shell() -> DesktopShell {
        let mut shell = DesktopShell::new(1920, 1080);
        shell.toggle_shortcut_card();
        assert!(shell.shortcut_card_open, "the card must be open");
        shell
    }

    fn tap(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn tap_with(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: String::new(),
        }
    }

    #[test]
    fn the_arrow_keys_walk_the_card_and_stop_at_the_ends() {
        let mut shell = card_shell();
        assert_eq!(shell.shortcut_editor.selected(), 0);

        drop(shell.handle_hotkey(&tap(Key::Down)));
        assert_eq!(shell.shortcut_editor.selected(), 1);
        drop(shell.handle_hotkey(&tap(Key::Up)));
        assert_eq!(shell.shortcut_editor.selected(), 0);

        // Clamped at the top, as every other list in this shell is.
        drop(shell.handle_hotkey(&tap(Key::Up)));
        assert_eq!(
            shell.shortcut_editor.selected(),
            0,
            "no wrap to the last row"
        );

        let last = shell.hotkeys.len().saturating_sub(1);
        for _ in 0..shell.hotkeys.len().saturating_add(5) {
            drop(shell.handle_hotkey(&tap(Key::Down)));
        }
        assert_eq!(
            shell.shortcut_editor.selected(),
            last,
            "and none off the bottom"
        );
    }

    /// The whole point: while recording, the keystroke is data.
    ///
    /// A user rebinding a shortcut presses chords that *are* shortcuts. If the
    /// shell ran them, rebinding "show the desktop" would show the desktop.
    #[test]
    fn a_chord_pressed_while_recording_does_not_also_run() {
        settingsfile::testing::with_scratch_config(
            "hk-a-chord-pressed-while-recording-does-not",
            |_root| {
                // The control, without which this proves nothing. The chord
                // used here until 2026-09-24 was Super+D, Show Desktop -- which,
                // in a shell with no windows, asks for nothing whether it runs
                // or not, so "it asked for nothing" held either way. Super+R
                // opens the Run box, and that is visible.
                let mut control = DesktopShell::new(1920, 1080);
                drop(control.handle_hotkey(&tap_with(Key::R, Modifiers::super_key())));
                assert!(
                    control.run_dialog.is_visible(),
                    "Super+R obeyed must open the Run box, or this test cannot fail"
                );

                let mut shell = card_shell();
                drop(shell.handle_hotkey(&tap(Key::Enter)));
                assert!(shell.shortcut_editor.is_recording(), "recording");

                let outcome = shell.handle_hotkey(&tap_with(Key::R, Modifiers::super_key()));

                assert!(
                    !shell.run_dialog.is_visible(),
                    "the chord being recorded must not also be obeyed"
                );
                assert!(outcome.requests.is_empty(), "and must ask for nothing");
                assert!(outcome.launches.is_empty(), "and must start nothing");
                assert!(!shell.shortcut_editor.is_recording(), "and recording ends");
            },
        );
    }

    #[test]
    fn recording_moves_the_action_onto_the_new_chord() {
        settingsfile::testing::with_scratch_config(
            "hk-recording-moves-the-action-onto-the-new-",
            |_root| {
                let mut shell = card_shell();
                let (old, action) = shell
                    .hotkeys
                    .all_bindings()
                    .next()
                    .map(|(h, a)| (*h, a.clone()))
                    .expect("a first binding");

                drop(shell.handle_hotkey(&tap(Key::Enter)));
                let chord = crate::hotkeys::Hotkey::new(Key::F9, Modifiers::ctrl());
                drop(shell.handle_hotkey(&tap_with(Key::F9, Modifiers::ctrl())));

                assert_eq!(
                    shell.hotkeys.conflicts_with(&chord),
                    Some(&action),
                    "the new chord must run what the row named"
                );
                assert!(
                    shell.hotkeys.conflicts_with(&old).is_none(),
                    "and the old chord must stop doing it"
                );
            },
        );
    }

    /// A chord already in use is refused, and the refusal names the holder.
    #[test]
    fn a_chord_that_is_taken_is_refused_and_says_by_what() {
        settingsfile::testing::with_scratch_config(
            "hk-a-chord-that-is-taken-is-refused-and-say",
            |_root| {
                let mut shell = card_shell();
                let mut bindings = shell.hotkeys.all_bindings();
                let (first, first_action) = bindings
                    .next()
                    .map(|(h, a)| (*h, a.clone()))
                    .expect("first");
                // Skips any binding on a bare modifier. There is no such binding in
                // the default table -- the first two rows are Escape and PrintScreen --
                // so this currently skips nothing; it is here because the recorder
                // ignores bare modifiers by design, and a future default bound to one
                // would otherwise make this test assert on a rebind that never
                // happened, and pass for the wrong reason.
                let second = bindings
                    .map(|(h, _)| *h)
                    .find(|h| {
                        !matches!(
                            h.key,
                            Key::LeftCtrl
                                | Key::RightCtrl
                                | Key::LeftAlt
                                | Key::RightAlt
                                | Key::LeftShift
                                | Key::RightShift
                                | Key::LeftSuper
                                | Key::RightSuper
                        )
                    })
                    .expect("a second binding on a real key");

                // Row 0 is `first`; try to give it `second`'s chord.
                drop(shell.handle_hotkey(&tap(Key::Enter)));
                let ev = KeyEvent {
                    key: second.key,
                    pressed: true,
                    modifiers: second.modifiers(),
                    text: String::new(),
                };
                drop(shell.handle_hotkey(&ev));

                assert_eq!(
                    shell.hotkeys.conflicts_with(&first),
                    Some(&first_action),
                    "a refused rebind must leave the original binding alone"
                );
                let msg = shell
                    .shortcut_editor
                    .message()
                    .unwrap_or_default()
                    .to_string();
                assert!(msg.contains("already"), "and must say so, got {msg:?}");
            },
        );
    }

    /// Escape cancels the recording rather than closing the card.
    #[test]
    fn escape_while_recording_cancels_the_rebind() {
        settingsfile::testing::with_scratch_config(
            "hk-escape-while-recording-cancels-the-rebin",
            |_root| {
                let mut shell = card_shell();
                let before: Vec<_> = shell
                    .hotkeys
                    .all_bindings()
                    .map(|(h, a)| (*h, a.clone()))
                    .collect();

                drop(shell.handle_hotkey(&tap(Key::Enter)));
                drop(shell.handle_hotkey(&tap(Key::Escape)));

                assert!(!shell.shortcut_editor.is_recording(), "recording stopped");
                assert!(shell.shortcut_card_open, "but the card stays open");
                let after: Vec<_> = shell
                    .hotkeys
                    .all_bindings()
                    .map(|(h, a)| (*h, a.clone()))
                    .collect();
                assert_eq!(before, after, "and nothing was rebound");
            },
        );
    }

    /// A bare modifier is not a chord.
    ///
    /// Reaching for Ctrl on the way to Ctrl+F9 must not bind the shortcut to
    /// Ctrl the instant the finger lands.
    #[test]
    fn a_modifier_on_its_own_does_not_end_the_recording() {
        settingsfile::testing::with_scratch_config(
            "hk-a-modifier-on-its-own-does-not-end-the-r",
            |_root| {
                let mut shell = card_shell();
                drop(shell.handle_hotkey(&tap(Key::Enter)));

                for key in [Key::LeftCtrl, Key::LeftAlt, Key::LeftShift, Key::LeftSuper] {
                    drop(shell.handle_hotkey(&tap(key)));
                    assert!(
                        shell.shortcut_editor.is_recording(),
                        "{key:?} alone must not be taken as the answer"
                    );
                }
            },
        );
    }

    /// **A deleted shortcut is still deleted in a fresh shell.**
    ///
    /// The test this feature exists to pass, and the one a naive
    /// implementation fails. `load_shortcuts` merges the saved file onto the
    /// shipped defaults rather than replacing them -- on purpose, so that a
    /// shortcut added in a later version reaches a user who has customised
    /// theirs. A deletion that only dropped the line from the file would
    /// therefore be undone at the next login, while every in-memory assertion
    /// went on passing. Only a fresh shell can see it.
    #[test]
    fn a_deleted_shortcut_is_still_deleted_in_a_fresh_shell() {
        settingsfile::testing::with_scratch_config("hk-delete", |_root| {
            let mut shell = card_shell();
            let (chord, action) = shell
                .hotkeys
                .all_bindings()
                .next()
                .map(|(h, a)| (*h, a.clone()))
                .expect("a binding to delete");

            shell.shortcut_editor.set_selected(0);
            drop(shell.handle_hotkey(&tap(Key::Delete)));
            assert_eq!(
                shell.hotkeys.conflicts_with(&chord),
                None,
                "the shortcut is still bound in the session that deleted it"
            );

            let mut fresh = DesktopShell::new(1920, 1080);
            assert_eq!(
                fresh.hotkeys.conflicts_with(&chord),
                Some(&action),
                "the defaults must still have it, or this proves nothing"
            );
            fresh.load_shortcuts();

            assert_eq!(
                fresh.hotkeys.conflicts_with(&chord),
                None,
                "the deleted shortcut came back at the next login"
            );
            assert!(
                fresh.hotkeys.all_bindings().all(|(_, a)| *a != action),
                "the action returned on some other chord"
            );
        });
    }

    /// **An action on two chords keeps both across a save and reload.**
    ///
    /// The defaults put the Start Menu on both Super keys on purpose: one
    /// entry answers a driver that sets the Super bit for the Super key
    /// itself, the other a driver that does not. `load_shortcuts` used to
    /// unregister every *other* chord bound to an action as it applied each
    /// binding, so the two entries deleted each other and whichever was
    /// processed last was the only survivor -- after any save and reload, one
    /// of the two Super keys silently stopped opening the menu.
    ///
    /// Found by `deleting_one_shortcut_keeps_the_rest`, which failed with
    /// "deleting one shortcut took Start Menu with it" against code that had
    /// nothing to do with deleting.
    #[test]
    fn an_action_on_two_chords_keeps_both() {
        settingsfile::testing::with_scratch_config("hk-two-chords", |_root| {
            let shell = card_shell();
            let doubled: Vec<_> = shell
                .hotkeys
                .all_bindings()
                .filter(|(_, a)| **a == crate::hotkeys::HotkeyAction::ToggleStartMenu)
                .map(|(h, _)| *h)
                .collect();
            assert_eq!(
                doubled.len(),
                2,
                "the fixture no longer binds one action to two chords, so this proves nothing"
            );
            shell.save_shortcuts().expect("save");

            let mut fresh = DesktopShell::new(1920, 1080);
            fresh.load_shortcuts();
            for chord in doubled {
                assert_eq!(
                    fresh.hotkeys.conflicts_with(&chord),
                    Some(&crate::hotkeys::HotkeyAction::ToggleStartMenu),
                    "{} lost its binding on reload",
                    chord.display_name()
                );
            }
        });
    }

    /// Deleting one shortcut leaves the others alone.
    ///
    /// A tombstone names an action, and the merge that applies it walks every
    /// binding; an over-eager unregister would take the neighbours with it.
    #[test]
    fn deleting_one_shortcut_keeps_the_rest() {
        settingsfile::testing::with_scratch_config("hk-delete-one", |_root| {
            let mut shell = card_shell();
            let before: Vec<_> = shell
                .hotkeys
                .all_bindings()
                .map(|(h, a)| (*h, a.clone()))
                .collect();
            assert!(before.len() > 2, "fixture too small to prove anything");

            shell.shortcut_editor.set_selected(0);
            drop(shell.handle_hotkey(&tap(Key::Delete)));

            let mut fresh = DesktopShell::new(1920, 1080);
            fresh.load_shortcuts();
            for (chord, action) in before.iter().skip(1) {
                assert_eq!(
                    fresh.hotkeys.conflicts_with(chord),
                    Some(action),
                    "deleting one shortcut took {} with it",
                    action.display_label()
                );
            }
        });
    }

    // ------------------------------------------------------------------
    // The card's editor, end to end through the shell
    //
    // `shortcut_editor`'s own tests drive the state machine against a bare
    // registry. These go through `handle_hotkey`, the way a keystroke really
    // arrives, and through `save_shortcuts` and a fresh shell, the way a
    // change really survives -- the two things a state machine test cannot
    // see.
    // ------------------------------------------------------------------

    /// Every string the card draws right now.
    fn drawn_on_card(shell: &DesktopShell) -> Vec<String> {
        shell
            .render_shortcut_card()
            .expect("the card is open")
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                guitk::render::RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn type_into(shell: &mut DesktopShell, text: &str) {
        for ch in text.chars() {
            drop(shell.handle_hotkey(&typed(ch)));
        }
    }

    /// **A card closed some other way mid-recording leaves nothing behind.**
    ///
    /// The editor keeps its state while the card is shut, and a recording is
    /// the one state that swallows keystrokes. The start menu, the pane and
    /// the tray overflow all close the card by clearing a flag, knowing
    /// nothing about the editor; a recording that survived that would turn
    /// the next shortcut on a desktop showing no card into a keystroke
    /// recorded rather than obeyed.
    #[test]
    fn a_card_closed_mid_recording_does_not_swallow_the_next_shortcut() {
        settingsfile::testing::with_scratch_config("hk-closed-mid-recording", |_root| {
            let mut shell = card_shell();
            drop(shell.handle_hotkey(&tap(Key::Enter)));
            assert!(shell.shortcut_editor.is_recording(), "recording");

            // Opening the start menu closes the card without a word to it.
            shell.toggle_start_menu();
            assert!(!shell.shortcut_card_open);
            shell.toggle_start_menu();

            // Super+R opens the Run box when obeyed -- visible, unlike Show
            // Desktop, which asks for nothing in a shell with no windows.
            drop(shell.handle_hotkey(&tap_with(Key::R, Modifiers::super_key())));
            assert!(
                shell.run_dialog.is_visible(),
                "the shortcut was swallowed by a recording nobody can see"
            );
            shell.toggle_run_dialog();

            // And the card comes back clean.
            shell.toggle_shortcut_card();
            assert!(!shell.shortcut_editor.owns_keyboard());
        });
    }

    /// **F2 changes what keys do, and the change survives a login.**
    #[test]
    fn f2_on_the_card_changes_what_keys_do_and_it_survives_a_login() {
        settingsfile::testing::with_scratch_config("hk-f2-retarget", |_root| {
            let mut shell = card_shell();
            let (row, keys) = shell
                .hotkeys
                .all_bindings()
                .enumerate()
                .find(|(_, (_, a))| **a == crate::hotkeys::HotkeyAction::Screenshot)
                .map(|(row, (h, _))| (row, *h))
                .expect("a Screenshot shortcut in the defaults");
            shell.shortcut_editor.set_selected(row);

            drop(shell.handle_hotkey(&tap(Key::F2)));
            assert!(
                drawn_on_card(&shell)
                    .iter()
                    .any(|t| t == "Choose an action"),
                "the action list replaces the bindings while choosing"
            );
            type_into(&mut shell, "lock");
            drop(shell.handle_hotkey(&tap(Key::Enter)));

            assert_eq!(
                shell.hotkeys.conflicts_with(&keys),
                Some(&crate::hotkeys::HotkeyAction::ScreenLock)
            );
            let mut fresh = DesktopShell::new(1920, 1080);
            fresh.load_shortcuts();
            assert_eq!(
                fresh.hotkeys.conflicts_with(&keys),
                Some(&crate::hotkeys::HotkeyAction::ScreenLock),
                "the change did not survive a login"
            );
        });
    }

    /// **A program added on the card is the program the keys start.**
    ///
    /// Through to the launch, not only to the registry: a binding stored
    /// correctly and started wrongly is the screenshot shortcuts' old bug
    /// (a program path with a space in it), and only the launch shows it.
    #[test]
    fn a_program_shortcut_added_on_the_card_starts_the_program() {
        settingsfile::testing::with_scratch_config("hk-insert-program", |_root| {
            let mut shell = card_shell();
            drop(shell.handle_hotkey(&tap(Key::Insert)));
            // "Run a program..." heads the unfiltered list.
            drop(shell.handle_hotkey(&tap(Key::Enter)));
            type_into(&mut shell, "/usr/bin/terminal");
            drop(shell.handle_hotkey(&tap(Key::Enter)));
            let chord = Modifiers {
                ctrl: true,
                alt: true,
                ..Modifiers::NONE
            };
            drop(shell.handle_hotkey(&tap_with(Key::F9, chord)));

            // Escape leaves the list and closes the card.
            drop(shell.handle_hotkey(&tap(Key::Escape)));
            assert!(!shell.shortcut_card_open);

            let outcome = shell.handle_hotkey(&tap_with(Key::F9, chord));
            let started: Vec<&std::path::Path> = outcome
                .launches
                .iter()
                .map(|l| l.program.as_path())
                .collect();
            assert_eq!(started, [std::path::Path::new("/usr/bin/terminal")]);
            assert!(outcome.launches.iter().all(|l| l.args.is_empty()));
        });
    }

    /// **The card says which keys it answers until it has news.**
    #[test]
    fn the_card_lists_its_keys_until_there_is_something_to_report() {
        settingsfile::testing::with_scratch_config("hk-card-hint", |_root| {
            let mut shell = card_shell();
            let hint = |shell: &DesktopShell| {
                drawn_on_card(shell)
                    .iter()
                    .any(|t| t.starts_with("Enter: new keys"))
            };
            assert!(hint(&shell), "a fresh card names its keys");
            drop(shell.handle_hotkey(&tap(Key::Enter)));
            assert!(!hint(&shell), "while recording it says what to do instead");
            drop(shell.handle_hotkey(&tap(Key::Escape)));
            assert!(!hint(&shell), "and then what happened");
            drop(shell.handle_hotkey(&tap(Key::Down)));
            assert!(hint(&shell), "until the user moves on");
        });
    }

    /// A shortcut this build has never heard of is not a parse error.
    ///
    /// `none=` is a new spelling on the left-hand side, so a file written by
    /// this version is read by an older one as a chord named "none". That is
    /// already handled -- an unparseable file is left alone rather than
    /// rewritten -- but the reverse must hold too: `none` is only a tombstone
    /// when it is the whole left-hand side, or a chord whose name merely
    /// starts with those letters would be silently unbound instead of bound.
    #[test]
    fn only_a_bare_none_is_a_tombstone() {
        let cfg = crate::hotkeys::HotkeyConfig::load("none=screenshot\n")
            .expect("a tombstone should parse");
        assert_eq!(cfg.unbound().len(), 1, "the tombstone was not recognised");
        assert!(
            cfg.bindings().is_empty(),
            "a tombstone was also stored as a binding"
        );
    }

    /// A rebind survives a restart.
    #[test]
    fn a_rebound_shortcut_is_still_bound_in_a_fresh_shell() {
        settingsfile::testing::with_scratch_config("hk-persist", |_root| {
            let mut shell = card_shell();
            let (old, action) = shell
                .hotkeys
                .all_bindings()
                .next()
                .map(|(h, a)| (*h, a.clone()))
                .expect("a binding");

            drop(shell.handle_hotkey(&tap(Key::Enter)));
            let chord = crate::hotkeys::Hotkey::new(Key::F12, Modifiers::ctrl());
            drop(shell.handle_hotkey(&tap_with(Key::F12, Modifiers::ctrl())));
            assert_eq!(shell.hotkeys.conflicts_with(&chord), Some(&action));

            // A new shell starts from the defaults and then reads the file.
            let mut fresh = DesktopShell::new(1920, 1080);
            assert_eq!(
                fresh.hotkeys.conflicts_with(&chord),
                None,
                "the defaults must not already have it, or this proves nothing"
            );
            fresh.load_shortcuts();

            assert_eq!(
                fresh.hotkeys.conflicts_with(&chord),
                Some(&action),
                "the rebind must survive a restart"
            );
            assert_eq!(
                fresh.hotkeys.conflicts_with(&old),
                None,
                "and must have moved rather than been added beside the old chord"
            );
        });
    }

    /// A saved file names only what the user changed; everything else keeps
    /// working. A file from an older desktop must not delete newer shortcuts.
    #[test]
    fn loading_shortcuts_keeps_the_defaults_it_does_not_mention() {
        settingsfile::testing::with_scratch_config("hk-floor", |_root| {
            let mut shell = DesktopShell::new(1920, 1080);
            let before = shell.hotkeys.len();

            // A file mentioning exactly one binding.
            let mut doc = appearance::config::load(DesktopShell::SHORTCUTS_CONFIG_NAME);
            doc.set_seq(&["shortcuts"], &["Ctrl+F12=show_desktop"]);
            appearance::config::store(DesktopShell::SHORTCUTS_CONFIG_NAME, &doc).expect("store");

            shell.load_shortcuts();

            assert_eq!(
                shell.hotkeys.len(),
                before,
                "one moved shortcut must not change how many there are"
            );
            assert!(
                shell
                    .hotkeys
                    .conflicts_with(&crate::hotkeys::Hotkey::new(Key::F12, Modifiers::ctrl()))
                    .is_some(),
                "and the one it named must have moved"
            );
        });
    }

    /// An unreadable file leaves the defaults alone rather than clearing them.
    #[test]
    fn an_unparseable_shortcuts_file_is_ignored() {
        settingsfile::testing::with_scratch_config("hk-bad", |_root| {
            let mut shell = DesktopShell::new(1920, 1080);
            let before: Vec<_> = shell
                .hotkeys
                .all_bindings()
                .map(|(h, a)| (*h, a.clone()))
                .collect();

            let mut doc = appearance::config::load(DesktopShell::SHORTCUTS_CONFIG_NAME);
            doc.set_seq(&["shortcuts"], &["this is not a binding"]);
            appearance::config::store(DesktopShell::SHORTCUTS_CONFIG_NAME, &doc).expect("store");

            shell.load_shortcuts();

            let after: Vec<_> = shell
                .hotkeys
                .all_bindings()
                .map(|(h, a)| (*h, a.clone()))
                .collect();
            assert_eq!(
                before, after,
                "a bad file must not cost the user the defaults"
            );
        });
    }

    #[test]
    fn escape_with_the_card_open_shuts_it() {
        let mut shell = card_shell();
        drop(shell.handle_hotkey(&tap(Key::Escape)));
        assert!(!shell.shortcut_card_open);
    }

    /// Super+Space moves to the next installed layout.
    ///
    /// Asserted on the *active layout id*, not on the chord being consumed: a
    /// hotkey that is swallowed and does nothing is exactly the failure this
    /// feature had for months, when `input_method.rs` was 1,278 lines that
    /// nothing constructed.
    #[test]
    fn super_space_switches_to_the_next_keyboard_layout() {
        // Inside a scratch config even though this test never reads a file.
        // The chord *writes* one: `persist_input_layout` saves the new layout
        // to `input.yaml`. Without this the write landed in the developer's
        // real configuration directory, and -- because the scratch helper also
        // swaps the process-wide `XDG_CONFIG_HOME` -- landed in the neighbouring
        // test's scratch directory whenever the two ran at the same moment.
        // That is the flake logged as
        // BUG-C-THE-KEYBOARD-LAYOUT-TEST-FAILS-ABOUT-ONE-WORKSPACE-RUN-IN-TWO:
        // this test wrote `de` first, so the other one read `de` as its
        // "before", found the file already correct, skipped its own save and
        // saw nothing change.
        settingsfile::testing::with_scratch_config("shell-layout-switch", |_root| {
            let mut shell = shell();
            let first = shell
                .input_methods
                .active_layout_id()
                .expect("a machine with no layouts cannot type at all")
                .to_string();

            // Built inline rather than through a `press` helper: this module
            // has one that takes a *mouse* position, and the key-event one
            // lives in a different test module.
            let outcome = shell.handle_hotkey(&KeyEvent {
                key: Key::Space,
                pressed: true,
                modifiers: Modifiers {
                    super_key: true,
                    ..Modifiers::NONE
                },
                text: String::new(),
            });
            assert!(outcome.consumed, "Super+Space was not claimed by the shell");

            let second = shell
                .input_methods
                .active_layout_id()
                .expect("switching lost the layout list");
            assert_ne!(
                first, second,
                "Super+Space was consumed but the active layout did not move"
            );
        });
    }

    /// The chord is *grabbed*, which is the half a consumed-and-ignored test
    /// cannot see: a shortcut the compositor never routes to the shell is one
    /// the shell never gets to consume.
    #[test]
    fn the_layout_chord_is_one_the_shell_actually_asks_for() {
        let wanted = (
            Key::Space,
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        );
        assert!(
            shell().global_chords().contains(&wanted),
            "Super+Space is bound but not grabbed, so it never reaches the shell"
        );
    }

    /// The switch reaches `input.yaml`, which is what makes it reach the keys.
    ///
    /// The compositor decides what a scancode means and reads that from this
    /// The layout-switching shortcut is a choice, and a choice that does not
    /// survive a restart is not one.
    ///
    /// `SwitchShortcut` had a serializer and a parser from the day it was
    /// written, in a private config format that nothing ever called: the
    /// setting was stored, displayed by nothing and read by nothing, and it
    /// reset to Alt+Shift every session. It lives in `input.yaml` now, beside
    /// the layout it switches.
    #[test]
    fn the_layout_switching_shortcut_is_read_back_from_the_file() {
        inputsettings::config::testing::with_scratch_config("shell-switch", |_root| {
            let mut shell = shell();
            assert_eq!(
                shell.input_methods.switch_shortcut,
                crate::input_method::SwitchShortcut::AltShift,
                "the default is Alt+Shift"
            );

            let mut file = inputsettings::InputFile::load();
            file.settings.keyboard.layout_switch = "ctrl-shift".to_string();
            file.save().expect("save");

            assert!(shell.load_input_settings(), "the change was not noticed");
            assert_eq!(
                shell.input_methods.switch_shortcut,
                crate::input_method::SwitchShortcut::CtrlShift
            );
            assert!(
                !shell.load_input_settings(),
                "a second read of an unchanged file reported a change, which would repaint the desktop on every announcement"
            );
        });
    }

    /// An id this build does not know falls back rather than being taken
    /// literally, and — the half that matters — the file is **not** rewritten
    /// to the fallback. A newer build's shortcut name must survive a session
    /// under an older one.
    #[test]
    fn an_unknown_switch_shortcut_falls_back_without_destroying_the_setting() {
        inputsettings::config::testing::with_scratch_config("shell-switch-unknown", |_root| {
            let mut shell = shell();
            let mut file = inputsettings::InputFile::load();
            file.settings.keyboard.layout_switch = "triple-tap-escape".to_string();
            file.save().expect("save");

            let _ = shell.load_input_settings();
            assert_eq!(
                shell.input_methods.switch_shortcut,
                crate::input_method::SwitchShortcut::AltShift,
                "an unknown id should fall back to the default"
            );
            assert_eq!(
                inputsettings::InputFile::load()
                    .settings
                    .keyboard
                    .layout_switch,
                "triple-tap-escape",
                "reading the file rewrote it, so the newer build's setting is gone"
            );
        });
    }

    /// file. A test that only checked the shell's own field would pass on a
    /// build where the user pressed the chord, saw the indicator change, and
    /// went on typing in the old layout.
    #[test]
    fn switching_layout_writes_the_setting_the_compositor_reads() {
        inputsettings::config::testing::with_scratch_config("shell-layout", |_root| {
            let mut shell = shell();
            let before = inputsettings::InputFile::load().settings.keyboard.layout;

            let outcome = shell.handle_hotkey(&KeyEvent {
                key: Key::Space,
                pressed: true,
                modifiers: Modifiers {
                    super_key: true,
                    ..Modifiers::NONE
                },
                text: String::new(),
            });
            assert!(outcome.consumed, "the chord was not claimed at all");

            let after = inputsettings::InputFile::load().settings.keyboard.layout;
            assert_ne!(
                before, after,
                "the chord changed the shell's model but not the file the compositor reads, so the keys would not have moved"
            );
            assert_eq!(
                after,
                shell
                    .input_methods
                    .active_layout_id()
                    .expect("the shell still has a layout"),
                "the file and the indicator disagree about which layout is active"
            );
        });
    }

    /// The taskbar says which layout is active, and says something different
    /// after a switch.
    ///
    /// Without this the shortcut changes what every key on the keyboard
    /// produces and nothing on screen reports it -- which is worse than not
    /// having the shortcut, because the user has no way to find out what
    /// happened or how to undo it.
    #[test]
    fn the_taskbar_reports_which_keyboard_layout_is_active() {
        fn tray_strings(shell: &DesktopShell) -> Vec<String> {
            shell
                .render_taskbar()
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        }

        let mut shell = shell();
        assert!(
            shell.input_methods.layouts.len() > 1,
            "a machine with one layout draws no indicator, so this proves nothing"
        );
        let before = shell.input_methods.tray_label().to_string();
        assert!(
            tray_strings(&shell).contains(&before),
            "the active layout is not named anywhere in the taskbar"
        );

        shell.input_methods.next_layout();
        let after = shell.input_methods.tray_label().to_string();
        assert_ne!(before, after, "the switcher did not move");
        assert!(
            tray_strings(&shell).contains(&after),
            "the taskbar still names the old layout after a switch"
        );
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
mod quiet_hours_wiring_tests {
    //! Quiet hours, from the file the user edits to the timer the loop sleeps
    //! on.
    //!
    //! Every test here fixes the zone to UTC and names an instant, because the
    //! thing under test is "what does the desktop do at two in the morning" and
    //! waiting until two in the morning is not a test strategy. The default
    //! shipped zone is `America/New_York`, so a test that did not set one would
    //! pass or fail depending on which side of a date line the chosen instant
    //! fell -- and would keep passing for months before it did.

    use super::{DesktopShell, focus_assist};
    use focus_assist::FocusMode;
    use notifsettings::QuietHours;
    use std::time::Duration;

    /// A shell reading in UTC, so that an instant means one clock reading.
    fn shell() -> DesktopShell {
        let mut shell = DesktopShell::new(1920, 1080);
        assert!(
            shell.datetime.set_zone(Some("UTC")),
            "UTC is not in the shipped zone table"
        );
        shell
    }

    /// 2026-09-17 is a Thursday. Weekday 4, in the 0=Sunday numbering.
    const THURSDAY: u64 = 1_789_603_200;

    /// `THURSDAY` plus `hours:minutes`, still on Thursday for hours under 24.
    fn at(hour: u64, minute: u64) -> u64 {
        THURSDAY + hour * 3600 + minute * 60
    }

    #[test]
    fn the_clock_reading_is_local_and_names_the_weekday() {
        let shell = shell();
        assert_eq!(
            shell.clock_reading_at(THURSDAY),
            (0, 0, 4),
            "Thursday 00:00"
        );
        assert_eq!(shell.clock_reading_at(at(22, 30)), (22, 30, 4));
        assert_eq!(shell.clock_reading_at(at(25, 0)), (1, 0, 5), "into Friday");
    }

    /// The whole point: hours the user set silence the desktop, and stop.
    #[test]
    fn quiet_hours_put_focus_assist_into_force_and_take_it_out_again() {
        let mut shell = shell();
        let mut quiet = QuietHours::default();
        quiet.enabled = true; // 22:00-07:00, every day
        shell.focus.set_quiet_hours(&quiet);

        assert!(
            shell.evaluate_schedules(at(23, 0)),
            "eleven at night changed it"
        );
        assert_eq!(shell.focus.effective_mode(), FocusMode::PriorityOnly);

        // Still inside, so nothing changed -- but it is still in force.
        assert!(!shell.evaluate_schedules(at(26, 0)), "two in the morning");
        assert_eq!(shell.focus.effective_mode(), FocusMode::PriorityOnly);

        assert!(
            shell.evaluate_schedules(at(31, 0)),
            "seven o'clock ended it"
        );
        assert_eq!(shell.focus.effective_mode(), FocusMode::Off);
    }

    /// Quiet hours hold back the ordinary, not the important.
    ///
    /// Priority-only rather than total silence, because a rule set months ago
    /// must not be the reason a user misses the one notification that mattered.
    #[test]
    fn quiet_hours_still_let_a_priority_notification_through() {
        let mut shell = shell();
        let mut quiet = QuietHours::default();
        quiet.enabled = true;
        shell.focus.set_quiet_hours(&quiet);
        shell.evaluate_schedules(at(23, 0));

        shell.focus.set_app_override(
            focus_assist::AppNotifOverride::new("Alarm")
                .with_importance(focus_assist::NotifPriority::Priority),
        );
        assert!(
            shell.focus.should_show_notification("Alarm"),
            "a priority notification was held back"
        );
        assert!(
            !shell.focus.should_show_notification("Chat"),
            "an ordinary notification got through"
        );
    }

    /// **A schedule that runs on no day must install nothing.**
    ///
    /// `AutoRule::Schedule` reads an empty day list as *every* day, so passing
    /// a no-days quiet-hours setting straight through would turn "never" into
    /// "always" -- a computer that silenced itself every night because the user
    /// deselected every day.
    #[test]
    fn a_schedule_with_no_days_is_not_a_schedule_every_day() {
        let mut shell = shell();
        let quiet = QuietHours {
            enabled: true,
            days: [false; 7],
            ..QuietHours::default()
        };
        shell.focus.set_quiet_hours(&quiet);

        assert!(shell.focus.auto_rules.is_empty(), "a rule was installed");
        assert!(!shell.evaluate_schedules(at(23, 0)));
        assert_eq!(shell.focus.effective_mode(), FocusMode::Off);
    }

    /// Editing the hours replaces them. It does not add a second set.
    #[test]
    fn changing_the_hours_does_not_leave_the_old_ones_running() {
        let mut shell = shell();
        let mut quiet = QuietHours::default();
        quiet.enabled = true;
        shell.focus.set_quiet_hours(&quiet);
        assert_eq!(shell.focus.auto_rules.len(), 1);

        // The user moves them to the afternoon.
        quiet.window = daywindow::DailyWindow::from_hm(14, 0, 16, 0).unwrap();
        shell.focus.set_quiet_hours(&quiet);
        assert_eq!(
            shell.focus.auto_rules.len(),
            1,
            "the old hours are still there"
        );

        assert!(
            shell.evaluate_schedules(at(15, 0)),
            "the new hours are in force"
        );
        shell.evaluate_schedules(at(23, 0));
        assert_eq!(
            shell.focus.effective_mode(),
            FocusMode::Off,
            "eleven at night is the window the user moved away from"
        );
    }

    /// Switching quiet hours off takes them out of force.
    #[test]
    fn switching_quiet_hours_off_stops_them() {
        let mut shell = shell();
        let mut quiet = QuietHours::default();
        quiet.enabled = true;
        shell.focus.set_quiet_hours(&quiet);
        shell.evaluate_schedules(at(23, 0));
        assert_eq!(shell.focus.effective_mode(), FocusMode::PriorityOnly);

        quiet.enabled = false;
        shell.focus.set_quiet_hours(&quiet);
        assert!(
            shell.evaluate_schedules(at(23, 0)),
            "the desktop stayed quiet after the switch was turned off"
        );
        assert_eq!(shell.focus.effective_mode(), FocusMode::Off);
    }

    // ----- turning it off by hand ---------------------------------------

    /// **A switch labelled "off" has to turn it off.**
    ///
    /// `set_mode(Off)` clears the manual override, which hands the decision
    /// straight back to the schedule -- still inside its window, so it
    /// switches everything on again before the next frame. The switch then
    /// reads on, and no sequence of presses changes it.
    ///
    /// The window here is built from the clock the *code* reads, an hour
    /// either side of it, rather than from a named hour: this door takes the
    /// real time, and a test that said 23:00 would only exercise it at night.
    /// Every day is selected so that the hour-before-midnight case, where the
    /// window belongs to yesterday, needs no special handling.
    #[test]
    fn switching_focus_assist_off_during_quiet_hours_does_not_snap_back() {
        let mut shell = shell();
        let now = DesktopShell::unix_now();
        let (hour, _, _) = shell.clock_reading_at(now);
        let quiet = QuietHours {
            enabled: true,
            window: daywindow::DailyWindow::from_hm((hour + 23) % 24, 0, (hour + 1) % 24, 0)
                .expect("a real window"),
            days: [true; 7],
        };
        shell.notif.settings.quiet_hours = quiet;
        shell.focus.set_quiet_hours(&quiet);
        shell.evaluate_schedules(now);
        assert_eq!(
            shell.focus.effective_mode(),
            FocusMode::PriorityOnly,
            "the window built around now was not in force"
        );

        shell.set_focus_mode_by_hand(FocusMode::Off);

        assert_eq!(
            shell.focus.effective_mode(),
            FocusMode::Off,
            "the schedule took over again the instant it was switched off"
        );
        // And it stays off when the loop next looks, which is where the old
        // behaviour would have reappeared.
        shell.evaluate_schedules(DesktopShell::unix_now());
        assert_eq!(shell.focus.effective_mode(), FocusMode::Off, "it came back");
    }

    /// The snooze lasts exactly the rest of the period -- not longer.
    #[test]
    fn the_snooze_ends_when_the_schedule_would_have_ended() {
        let mut shell = shell();
        let mut quiet = QuietHours::default(); // 22:00-07:00, every day
        quiet.enabled = true;
        shell.notif.settings.quiet_hours = quiet;
        shell.focus.set_quiet_hours(&quiet);

        shell.evaluate_schedules(at(23, 0));
        assert_eq!(shell.focus.effective_mode(), FocusMode::PriorityOnly);
        shell.focus.set_mode(FocusMode::Off);
        shell.snooze_schedule_if_it_would_resume(at(23, 0));

        // Three in the morning: still the same night, still snoozed.
        assert!(!shell.evaluate_schedules(at(27, 0)));
        assert_eq!(shell.focus.effective_mode(), FocusMode::Off, "woke early");

        // Seven o'clock, when the window closes anyway.
        shell.evaluate_schedules(at(31, 0));
        assert_eq!(shell.focus.effective_mode(), FocusMode::Off);

        // And the *next* night is a new period, which the snooze must not
        // have eaten.
        assert!(
            shell.evaluate_schedules(at(46, 0)),
            "quiet hours never came back"
        );
        assert_eq!(shell.focus.effective_mode(), FocusMode::PriorityOnly);
    }

    /// Turning off a mode the user set by hand arms no snooze.
    ///
    /// "Focus assist is on and a schedule exists" is not the same claim as
    /// "the schedule is why". Without the distinction, switching off a mode
    /// set at noon would hold tonight's quiet hours off as well -- a setting
    /// silently cancelled hours before it was due, by a press that had nothing
    /// to do with it.
    #[test]
    fn turning_off_a_mode_set_by_hand_leaves_tonight_alone() {
        let mut shell = shell();
        let mut quiet = QuietHours::default();
        quiet.enabled = true;
        shell.notif.settings.quiet_hours = quiet;
        shell.focus.set_quiet_hours(&quiet);

        // Midday: the user switches focus assist on and off again.
        shell.evaluate_schedules(at(12, 0));
        shell.focus.set_mode(FocusMode::TotalSilence);
        shell.focus.set_mode(FocusMode::Off);
        shell.snooze_schedule_if_it_would_resume(at(12, 0));

        assert!(
            shell.evaluate_schedules(at(22, 0)),
            "quiet hours did not start"
        );
        assert_eq!(shell.focus.effective_mode(), FocusMode::PriorityOnly);
    }

    // ----- the timer ----------------------------------------------------

    /// The idle desktop of design-decisions 812: no timer at all.
    #[test]
    fn an_unscheduled_desktop_asks_for_no_wake_up() {
        let shell = shell();
        assert!(!shell.notif.settings.quiet_hours.enabled, "shipped off");
        assert_eq!(shell.next_schedule_change(at(23, 0)), None);
    }

    /// The timer lands *on* the boundary, not up to 59 seconds after it.
    #[test]
    fn the_timer_is_shortened_by_the_seconds_already_spent() {
        let mut shell = shell();
        shell.notif.settings.quiet_hours.enabled = true;

        // On the minute: two hours to the 22:00 start.
        assert_eq!(
            shell.next_schedule_change(at(20, 0)),
            Some(Duration::from_hours(2))
        );
        // Forty seconds into the minute: forty seconds less.
        assert_eq!(
            shell.next_schedule_change(at(20, 0) + 40),
            Some(Duration::from_secs(2 * 3600 - 40))
        );
        // And in the last minute before the boundary it is still positive,
        // which is what stops the loop re-arming for zero in a spin.
        let last = shell.next_schedule_change(at(21, 59) + 59).unwrap();
        assert!(
            last >= Duration::from_secs(1) && last <= Duration::from_mins(1),
            "{last:?}"
        );
    }

    /// Sleeping the reported time lands on a desktop that answers differently.
    #[test]
    fn waking_at_the_reported_moment_finds_the_answer_changed() {
        let mut shell = shell();
        shell.notif.settings.quiet_hours.enabled = true;
        shell
            .focus
            .set_quiet_hours(&shell.notif.settings.quiet_hours.clone());

        let now = at(20, 0) + 17;
        let before = shell.evaluate_schedules(now);
        assert!(!before, "already changed before any time passed");
        let sleep = shell.next_schedule_change(now).unwrap();

        assert!(
            !shell.evaluate_schedules(now + sleep.as_secs() - 1),
            "the mode changed a second before the boundary"
        );
        assert!(
            shell.evaluate_schedules(now + sleep.as_secs()),
            "the mode did not change at the boundary"
        );
        assert_eq!(shell.focus.effective_mode(), FocusMode::PriorityOnly);
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
mod taskbar_pin_tests {
    //! Pinning an application to the taskbar.
    //!
    //! Every test that pins runs inside `with_scratch_config`: pinning writes
    //! `taskbar.yaml`, and a test that wrote the developer's own is what
    //! `scripts/check-scratch-config.py` exists to refuse.

    use super::{
        DesktopShell, Hit, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
        ShellAction, TaskbarSlot, WindowInfo, WindowList,
    };

    /// A plain key press, as the compositor delivers one.
    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }
    use appearance::config::testing::with_scratch_config;

    fn shell() -> DesktopShell {
        DesktopShell::new(1920, 1080)
    }

    /// The executable path of the first program the start menu offers.
    fn first_app(shell: &DesktopShell) -> (String, String) {
        let entry = shell
            .start_menu_entries()
            .first()
            .copied()
            .expect("the shipped app database is empty");
        (entry.executable_path.clone(), entry.name.clone())
    }

    /// **A pinned application has a button, and it is a launcher.**
    #[test]
    fn a_pinned_application_gets_a_taskbar_button() {
        with_scratch_config("shell-pin-button", |_root| {
            let mut shell = shell();
            assert!(
                shell.taskbar_slots().is_empty(),
                "nothing is open or pinned"
            );

            let (exec, name) = first_app(&shell);
            shell.pin_app(&exec, &name);

            assert_eq!(shell.taskbar_slots(), vec![TaskbarSlot::Pinned(0)]);
            assert!(shell.is_pinned(&exec));
            let button = shell.taskbar_button_rect(0);
            assert!(button.w > 0.0 && button.h > 0.0, "the button has no area");
        });
    }

    /// A mouse event of a kind, at a point.
    fn at(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
        MouseEvent { x, y, kind }
    }

    /// The middle of the `index`-th taskbar button.
    fn button_centre(shell: &DesktopShell, index: usize) -> (f32, f32) {
        let r = shell.taskbar_button_rect(index);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// **Releasing it starts the program**, and the press only takes hold.
    ///
    /// Launching on the press would start the program every time the user
    /// began to rearrange the bar, which is why the tray waits for the release
    /// too.
    #[test]
    fn releasing_a_pinned_button_launches_the_program() {
        with_scratch_config("shell-pin-launch", |_root| {
            let mut shell = shell();
            let (exec, name) = first_app(&shell);
            shell.pin_app(&exec, &name);

            let (cx, cy) = button_centre(&shell, 0);
            assert_eq!(shell.hit_test(cx, cy), Hit::TaskbarPinned(0));

            assert_eq!(
                shell.handle_mouse(&at(cx, cy, MouseEventKind::Press(MouseButton::Left))),
                ShellAction::Consumed,
                "the press launched it before the release could say it was a click"
            );
            match shell.handle_mouse(&at(cx, cy, MouseEventKind::Release(MouseButton::Left))) {
                ShellAction::Launch(launch) => {
                    assert_eq!(
                        launch.program.to_string_lossy(),
                        exec,
                        "it started the wrong program"
                    );
                    assert!(
                        launch.args.is_empty(),
                        "a pinned program takes no arguments"
                    );
                }
                other => panic!("a pinned button did not launch anything: {other:?}"),
            }
        });
    }

    /// Pinning the same program twice leaves one button.
    #[test]
    fn pinning_the_same_program_twice_leaves_one_button() {
        with_scratch_config("shell-pin-twice", |_root| {
            let mut shell = shell();
            let (exec, name) = first_app(&shell);
            shell.pin_app(&exec, &name);
            shell.pin_app(&exec, &name);

            assert_eq!(shell.pinned_apps().len(), 1);
            assert_eq!(shell.taskbar_slots().len(), 1);
        });
    }

    /// Unpinning takes the button away again.
    #[test]
    fn unpinning_takes_the_button_away() {
        with_scratch_config("shell-unpin", |_root| {
            let mut shell = shell();
            let (exec, name) = first_app(&shell);
            shell.pin_app(&exec, &name);
            assert_eq!(shell.taskbar_slots().len(), 1);

            shell.unpin_app(&exec);

            assert!(
                shell.taskbar_slots().is_empty(),
                "the button outlived the pin"
            );
            assert!(!shell.is_pinned(&exec));
        });
    }

    /// **The pins survive a restart**, which is the whole point of pinning.
    #[test]
    fn pinned_applications_come_back_after_a_restart() {
        with_scratch_config("shell-pin-restart", |_root| {
            let (exec, name) = {
                let mut shell = shell();
                let (exec, name) = first_app(&shell);
                shell.pin_app(&exec, &name);
                (exec, name)
            };

            // A fresh shell, as a new login gets.
            let mut restarted = shell();
            assert!(!restarted.is_pinned(&exec), "it has not read the file yet");
            restarted.load_pinned();

            assert!(restarted.is_pinned(&exec), "the pin did not survive");
            assert_eq!(restarted.taskbar_slots(), vec![TaskbarSlot::Pinned(0)]);
            // The name comes from the launcher, not from the file: storing it
            // would be a second copy, stale the first time a program is
            // renamed.
            assert_eq!(restarted.pinned_apps()[0].display_name, name);
        });
    }

    /// A right-click on a start-menu row offers to pin it, and the label says
    /// what the click will *do* rather than what is already true.
    #[test]
    fn right_clicking_a_start_menu_row_offers_to_pin_and_then_to_unpin() {
        with_scratch_config("shell-pin-menu", |_root| {
            let mut shell = shell();
            shell.toggle_start_menu();
            let row = shell.start_menu_row_rect(0);
            let (cx, cy) = (row.x + row.w / 2.0, row.y + row.h / 2.0);
            assert_eq!(shell.hit_test(cx, cy), Hit::StartMenuEntry(0));

            shell.handle_press(cx, cy, MouseButton::Right);
            let drawn = format!("{:?}", shell.render_pin_menu().expect("no menu opened"));
            assert!(drawn.contains("Pin to taskbar"), "wrong offer: {drawn}");

            // Taken with the keyboard, which is the other half of the claim:
            // a menu that can only be used with the mouse is half a menu, and
            // it is also the only way a test can name a row without guessing
            // at where the panel drew it.
            drop(shell.handle_hotkey(&press(Key::Down)));
            drop(shell.handle_hotkey(&press(Key::Enter)));
            let (exec, _) = first_app(&shell);
            assert!(shell.is_pinned(&exec), "the menu did not pin it");

            // Ask again, and the offer is the reverse.
            shell.handle_press(cx, cy, MouseButton::Right);
            let drawn = format!("{:?}", shell.render_pin_menu().expect("no menu opened"));
            assert!(drawn.contains("Unpin from taskbar"), "wrong offer: {drawn}");
        });
    }

    /// **And off it again from the button itself**, which is where a user
    /// looks for it.
    #[test]
    fn a_pinned_button_can_be_unpinned_from_the_taskbar() {
        with_scratch_config("shell-unpin-taskbar", |_root| {
            let mut shell = shell();
            let (exec, name) = first_app(&shell);
            shell.pin_app(&exec, &name);

            let button = shell.taskbar_button_rect(0);
            let (cx, cy) = (button.x + button.w / 2.0, button.y + button.h / 2.0);
            shell.handle_press(cx, cy, MouseButton::Right);

            let drawn = format!("{:?}", shell.render_pin_menu().expect("no menu opened"));
            assert!(
                drawn.contains("Unpin from taskbar"),
                "the button offered no way off the bar: {drawn}"
            );

            drop(shell.handle_hotkey(&press(Key::Down)));
            drop(shell.handle_hotkey(&press(Key::Enter)));

            assert!(!shell.is_pinned(&exec), "it is still pinned");
            assert!(
                shell.taskbar_slots().is_empty(),
                "the button outlived the pin"
            );
        });
    }

    /// A right-click on a *window's* button offers nothing, because pinning
    /// one is not possible: a window carries no executable path.
    #[test]
    fn a_window_button_offers_no_pin_menu() {
        with_scratch_config("shell-unpin-window", |_root| {
            let mut shell = shell();
            shell.apply_window_list(&WindowList::new(
                0,
                vec![WindowInfo::new(1, 1, "A window".to_string())],
            ));
            let button = shell.taskbar_button_rect(0);
            let (cx, cy) = (button.x + button.w / 2.0, button.y + button.h / 2.0);

            shell.handle_press(cx, cy, MouseButton::Right);

            assert!(
                shell.render_pin_menu().is_none(),
                "a window's button offered to pin something"
            );
        });
    }

    /// **Dragging a pinned button moves it along the bar.**
    ///
    /// `reorder_pinned` is the third of the module's model methods the shell
    /// now uses, and the drag is the same `DragSource` the tray uses -- one
    /// model of "has this press become a drag yet", two things that drag.
    #[test]
    fn dragging_a_pinned_button_reorders_it() {
        with_scratch_config("shell-pin-drag", |_root| {
            let mut shell = shell();
            let apps: Vec<(String, String)> = shell
                .start_menu_entries()
                .iter()
                .take(3)
                .map(|e| (e.executable_path.clone(), e.name.clone()))
                .collect();
            assert_eq!(apps.len(), 3, "three programs are needed to see a move");
            for (exec, name) in &apps {
                shell.pin_app(exec, name);
            }
            assert_eq!(shell.pinned_apps()[0].exec_path, apps[0].0);

            // Take the first and drop it past the middle of the third.
            let (from_x, from_y) = button_centre(&shell, 0);
            let (to_x, _) = button_centre(&shell, 2);
            shell.handle_mouse(&at(
                from_x,
                from_y,
                MouseEventKind::Press(MouseButton::Left),
            ));
            shell.handle_mouse(&at(to_x + 4.0, from_y, MouseEventKind::Move));
            shell.handle_mouse(&at(
                to_x + 4.0,
                from_y,
                MouseEventKind::Release(MouseButton::Left),
            ));

            assert_ne!(
                shell.pinned_apps()[0].exec_path,
                apps[0].0,
                "the dragged button did not move"
            );
            assert!(
                shell.pinned_apps().iter().any(|a| a.exec_path == apps[0].0),
                "the dragged button fell off the bar"
            );
            assert_eq!(
                shell.pinned_apps().len(),
                3,
                "a drag changed how many there are"
            );
        });
    }

    /// A press that barely moves is still a click, not a drag.
    #[test]
    fn a_press_that_hardly_moves_still_launches() {
        with_scratch_config("shell-pin-nudge", |_root| {
            let mut shell = shell();
            let (exec, name) = first_app(&shell);
            shell.pin_app(&exec, &name);
            let (cx, cy) = button_centre(&shell, 0);

            shell.handle_mouse(&at(cx, cy, MouseEventKind::Press(MouseButton::Left)));
            // One pixel: under any sane threshold, and the gesture a shaky
            // hand makes while clicking.
            shell.handle_mouse(&at(cx + 1.0, cy, MouseEventKind::Move));
            let action = shell.handle_mouse(&at(
                cx + 1.0,
                cy,
                MouseEventKind::Release(MouseButton::Left),
            ));

            assert!(
                matches!(action, ShellAction::Launch(_)),
                "a one-pixel wobble turned a click into a drag: {action:?}"
            );
        });
    }

    /// **A drag cannot push a pinned button in among the windows.**
    ///
    /// The window buttons are not the pinned list's to rearrange, and an index
    /// past its end would be a position `reorder_pinned` has no slot for.
    #[test]
    fn dragging_past_the_last_pin_stays_in_the_pinned_run() {
        with_scratch_config("shell-pin-clamp", |_root| {
            let mut shell = shell();
            let apps: Vec<(String, String)> = shell
                .start_menu_entries()
                .iter()
                .take(2)
                .map(|e| (e.executable_path.clone(), e.name.clone()))
                .collect();
            for (exec, name) in &apps {
                shell.pin_app(exec, name);
            }
            shell.apply_window_list(&WindowList::new(
                0,
                vec![WindowInfo::new(1, 1, "A window".to_string())],
            ));

            let (from_x, from_y) = button_centre(&shell, 0);
            // Far to the right, over the window button and beyond.
            let far = shell.taskbar_button_rect(2).x + 500.0;
            shell.handle_mouse(&at(
                from_x,
                from_y,
                MouseEventKind::Press(MouseButton::Left),
            ));
            shell.handle_mouse(&at(far, from_y, MouseEventKind::Move));
            shell.handle_mouse(&at(far, from_y, MouseEventKind::Release(MouseButton::Left)));

            assert_eq!(shell.pinned_apps().len(), 2, "a pin was lost off the end");
            let slots = shell.taskbar_slots();
            assert_eq!(slots[0], TaskbarSlot::Pinned(0));
            assert_eq!(slots[1], TaskbarSlot::Pinned(1));
            assert!(
                matches!(slots[2], TaskbarSlot::Window(_)),
                "the window moved"
            );
        });
    }

    /// Pinned buttons stand to the left of the windows.
    #[test]
    fn pinned_buttons_come_before_window_buttons() {
        with_scratch_config("shell-pin-order", |_root| {
            let mut shell = shell();
            let (exec, name) = first_app(&shell);
            shell.pin_app(&exec, &name);
            shell.apply_window_list(&WindowList::new(
                0,
                vec![WindowInfo::new(1, 1, "A window".to_string())],
            ));

            let slots = shell.taskbar_slots();
            assert_eq!(slots.len(), 2, "one pin and one window: {slots:?}");
            assert_eq!(slots[0], TaskbarSlot::Pinned(0));
            assert!(matches!(slots[1], TaskbarSlot::Window(_)));
            assert!(
                shell.taskbar_button_rect(0).x < shell.taskbar_button_rect(1).x,
                "the pinned button is not to the left of the window's"
            );
        });
    }
}

/// The desktop menu's View submenu and "Sort by name": the user's way to
/// choose how the desktop icons are placed and how big they are.
#[cfg(test)]
mod view_menu_tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::{AppearanceSettings, DesktopShell, MenuItem, icons};
    use appearance::IconSize;
    use appearance::config::testing::with_scratch_config;
    use guitk::event::{Key, KeyEvent, Modifiers};
    use guitk::render::RenderCommand;
    use icons::ArrangementMode as Mode;

    /// The check mark the menu draws beside a ticked item.
    const TICK: &str = "\u{2713}";

    fn tap(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    /// Every item id in a menu tree, submenus included.
    fn ids(items: &[MenuItem], out: &mut Vec<u64>) {
        for item in items {
            match item {
                MenuItem::Action { id, .. } => out.push(*id),
                MenuItem::Submenu { id, children, .. } => {
                    out.push(*id);
                    ids(children, out);
                }
                MenuItem::Separator => {}
            }
        }
    }

    /// The View submenu's items, as `(label, ticked)`.
    fn view_items(size: IconSize, mode: Mode) -> Vec<(String, bool)> {
        let items = DesktopShell::desktop_menu_items(size, mode);
        let Some(MenuItem::Submenu { children, .. }) = items
            .iter()
            .find(|i| matches!(i, MenuItem::Submenu { label, .. } if label == "View"))
        else {
            panic!("no View submenu in {items:?}");
        };
        children
            .iter()
            .filter_map(|c| match c {
                MenuItem::Action { label, checked, .. } => {
                    Some((label.clone(), *checked == Some(true)))
                }
                _ => None,
            })
            .collect()
    }

    fn ticked(size: IconSize, mode: Mode) -> Vec<String> {
        view_items(size, mode)
            .into_iter()
            .filter(|(_, t)| *t)
            .map(|(l, _)| l)
            .collect()
    }

    /// The id of the View item for `size`.
    fn size_item(size: IconSize) -> u64 {
        let index = IconSize::ALL.iter().position(|s| *s == size).unwrap();
        DesktopShell::MENU_ICON_SIZE_BASE + u64::try_from(index).unwrap()
    }

    /// Two ids the same would make one item do another's job, silently: the
    /// dispatch matches on the number, not on the label drawn.
    #[test]
    fn the_desktop_menu_ids_are_all_distinct() {
        for size in IconSize::ALL {
            for mode in Mode::ALL {
                let mut all = Vec::new();
                ids(&DesktopShell::desktop_menu_items(*size, mode), &mut all);
                ids(&DesktopShell::widget_menu_items(), &mut all);
                // The widget menu repeats "Remove all widgets" on purpose --
                // the same item, so the same id -- and nothing else.
                let remove_all = all
                    .iter()
                    .filter(|id| **id == DesktopShell::MENU_REMOVE_WIDGETS)
                    .count();
                assert_eq!(remove_all, 2);
                let mut unique = all.clone();
                unique.sort_unstable();
                unique.dedup();
                assert_eq!(unique.len(), all.len() - 1, "a repeated id in {all:?}");
                assert!(!all.contains(&DesktopShell::MENU_PIN_TOGGLE));
            }
        }
    }

    /// Every size the setting offers has an item, and the item names it back.
    #[test]
    fn every_icon_size_has_an_item_that_names_it() {
        let items = view_items(IconSize::Medium, Mode::SnapToGrid);
        for size in IconSize::ALL {
            let label = DesktopShell::icon_size_menu_label(*size);
            assert!(
                items.iter().any(|(l, _)| l == label),
                "no item for {size:?}"
            );
        }
        let mut all = Vec::new();
        ids(
            &DesktopShell::desktop_menu_items(IconSize::Medium, Mode::SnapToGrid),
            &mut all,
        );
        let named: Vec<IconSize> = all
            .iter()
            .filter_map(|id| DesktopShell::menu_icon_size(*id))
            .collect();
        assert_eq!(
            named,
            IconSize::ALL,
            "the size items and the sizes disagree"
        );
        assert_eq!(
            DesktopShell::menu_icon_size(DesktopShell::MENU_ADD_CLOCK),
            None
        );
    }

    /// The ticks say what is in force: one size, and the switches as the
    /// arrangement means them -- both under auto-arrange, which is aligned by
    /// construction.
    #[test]
    fn the_view_submenu_ticks_what_is_in_force() {
        assert_eq!(
            ticked(IconSize::Large, Mode::Free),
            ["Large icons"],
            "placing freely: neither switch"
        );
        assert_eq!(
            ticked(IconSize::Small, Mode::SnapToGrid),
            ["Small icons", "Align icons to grid"]
        );
        assert_eq!(
            ticked(IconSize::ExtraLarge, Mode::AutoArrange),
            [
                "Extra large icons",
                "Auto arrange icons",
                "Align icons to grid"
            ]
        );
    }

    /// **The two switches move between the three arrangements the way a
    /// desktop's do.** Auto-arrange on aligns; off leaves the icons aligned;
    /// alignment off stops arranging too.
    #[test]
    fn the_two_switches_map_onto_the_three_arrangements() {
        let cases = [
            (
                Mode::Free,
                DesktopShell::MENU_AUTO_ARRANGE,
                Mode::AutoArrange,
            ),
            (
                Mode::SnapToGrid,
                DesktopShell::MENU_AUTO_ARRANGE,
                Mode::AutoArrange,
            ),
            (
                Mode::AutoArrange,
                DesktopShell::MENU_AUTO_ARRANGE,
                Mode::SnapToGrid,
            ),
            (
                Mode::Free,
                DesktopShell::MENU_ALIGN_TO_GRID,
                Mode::SnapToGrid,
            ),
            (
                Mode::SnapToGrid,
                DesktopShell::MENU_ALIGN_TO_GRID,
                Mode::Free,
            ),
            (
                Mode::AutoArrange,
                DesktopShell::MENU_ALIGN_TO_GRID,
                Mode::Free,
            ),
        ];
        for (from, item, to) in cases {
            let mut shell = DesktopShell::new(1920, 1080);
            shell.icons.set_arrangement(from);
            assert!(
                shell.activate_desktop_menu_item(item).changed(),
                "{from:?} + {item}"
            );
            assert_eq!(shell.icons.arrangement(), to, "{from:?} + {item}");
            assert!(
                shell.take_icons_dirty(),
                "{from:?} + {item}: the new arrangement is not saved"
            );
            assert!(
                !shell.take_widgets_dirty(),
                "{from:?} + {item}: an icon item rewrote the widget layout"
            );
        }
    }

    /// **Choosing a size from the menu resizes the icons and writes the
    /// setting** where the Settings application reads it.
    #[test]
    fn choosing_an_icon_size_resizes_the_icons_and_writes_the_setting() {
        with_scratch_config("view-menu-icon-size", |_root| {
            let mut shell = DesktopShell::new(1920, 1080);
            assert_ne!(
                shell.appearance.icon_size,
                IconSize::Large,
                "proves nothing"
            );

            assert!(
                shell
                    .activate_desktop_menu_item(size_item(IconSize::Large))
                    .changed()
            );
            assert_eq!(shell.icons.icon_px(), IconSize::Large.pixels());
            assert_eq!(shell.appearance.icon_size, IconSize::Large);
            assert_eq!(
                appearance::AppearanceFile::load().settings.icon_size,
                IconSize::Large,
                "the setting did not reach appearance.yaml"
            );
            assert!(
                shell.take_icons_dirty(),
                "the icons moved and nothing will save where"
            );
            assert!(!shell.take_widgets_dirty());
            assert!(
                !shell.take_appearance_change(),
                "the compositor reads nothing about desktop icons"
            );

            // The same size again is no change.
            assert!(
                !shell
                    .activate_desktop_menu_item(size_item(IconSize::Large))
                    .changed()
            );
            assert!(!shell.take_icons_dirty());
        });
    }

    /// Choosing a size keeps every other setting the file holds: load, modify,
    /// save -- not a rewrite from the shell's own copy, which may be behind.
    #[test]
    fn choosing_an_icon_size_leaves_the_rest_of_the_file_alone() {
        with_scratch_config("view-menu-keeps-file", |_root| {
            let mut file = appearance::AppearanceFile::load();
            file.settings.night_light = true;
            file.save().expect("scratch is writable");
            // The shell's own copy says otherwise, as it would a moment after
            // the Settings application saved.
            let mut shell = DesktopShell::new(1920, 1080);
            assert!(!shell.appearance.night_light);

            assert!(
                shell
                    .activate_desktop_menu_item(size_item(IconSize::Small))
                    .changed()
            );
            let saved = appearance::AppearanceFile::load().settings;
            assert_eq!(saved.icon_size, IconSize::Small);
            assert!(
                saved.night_light,
                "a setting the menu did not touch was overwritten"
            );
        });
    }

    /// **The menu opened for real ticks what is in force, and a size chosen
    /// from it with the keyboard is applied** -- the route a user takes,
    /// through `handle_hotkey`, not the dispatch table.
    #[test]
    fn the_view_menu_works_from_the_keyboard() {
        with_scratch_config("view-menu-keyboard", |_root| {
            let mut shell = DesktopShell::new(1920, 1080);
            shell.set_appearance(AppearanceSettings {
                icon_size: IconSize::Small,
                ..AppearanceSettings::default()
            });
            shell.open_desktop_menu(400.0, 300.0);
            // Down onto "View", Right into it.
            for key in [Key::Down, Key::Right] {
                drop(shell.handle_hotkey(&tap(key)));
            }
            let drawn: Vec<(f32, String)> = shell
                .render_desktop_menu()
                .expect("the menu is open")
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { y, text, .. } => Some((*y, text.clone())),
                    _ => None,
                })
                .collect();
            let ticked: Vec<&str> = drawn
                .iter()
                .filter(|(_, t)| t == TICK)
                .filter_map(|(y, _)| {
                    drawn
                        .iter()
                        .find(|(ly, l)| (ly - y).abs() < 0.5 && l != TICK)
                        .map(|(_, l)| l.as_str())
                })
                .collect();
            assert_eq!(ticked, ["Small icons", "Align icons to grid"]);

            // Down past Small and Medium to Large, and take it.
            for key in [Key::Down, Key::Down, Key::Down, Key::Enter] {
                drop(shell.handle_hotkey(&tap(key)));
            }
            assert!(!shell.desktop_menu.is_visible(), "choosing closes the menu");
            assert_eq!(shell.icons.icon_px(), IconSize::Large.pixels());
        });
    }

    /// "Sort by name" sorts, and marks the layout for saving only when
    /// something moved.
    #[test]
    fn sort_by_name_sorts_the_icons() {
        let mut shell = DesktopShell::new(1920, 1080);
        for (name, col) in [("pear", 0), ("apple", 2)] {
            let (x, y) = shell.icons.cell_origin(col, 3);
            shell.icons.add_icon(
                name,
                icons::IconType::File,
                icons::IconAction::Custom(name.to_string()),
                x,
                y,
            );
        }
        assert!(
            shell
                .activate_desktop_menu_item(DesktopShell::MENU_SORT_BY_NAME)
                .changed()
        );
        let labels: Vec<String> = shell
            .icons
            .icon_ids()
            .into_iter()
            .filter_map(|id| shell.icons.get_icon(id).map(|i| i.label.clone()))
            .collect();
        assert_eq!(labels, ["apple", "pear"]);
        assert!(shell.take_icons_dirty());
        assert!(
            !shell
                .activate_desktop_menu_item(DesktopShell::MENU_SORT_BY_NAME)
                .changed()
        );
        assert!(
            !shell.take_icons_dirty(),
            "nothing moved, so nothing to save"
        );
    }
}

/// The desktop's own keys: what a key does when the desktop has the keyboard
/// and no shortcut or open surface took it.
#[cfg(test)]
mod desktop_key_tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::{DesktopShell, ShellAction, icons};
    use appearance::config::testing::with_scratch_config;
    use guitk::event::{Key, KeyEvent, Modifiers};

    fn press(key: Key) -> KeyEvent {
        press_with(key, Modifiers::NONE)
    }

    fn press_with(key: Key, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: String::new(),
        }
    }

    /// A shell with three icons down the first column, named a, b and c.
    fn shell_with_three() -> DesktopShell {
        let mut shell = DesktopShell::new(1920, 1080);
        for (row, name) in ["a", "b", "c"].into_iter().enumerate() {
            let (x, y) = shell.icons.cell_origin(0, i32::try_from(row).unwrap());
            shell.icons.add_icon(
                name,
                icons::IconType::File,
                icons::IconAction::Custom(name.to_string()),
                x,
                y,
            );
        }
        shell
    }

    fn selected(shell: &DesktopShell) -> Vec<String> {
        shell
            .icons
            .selected_ids()
            .into_iter()
            .filter_map(|id| shell.icons.get_icon(id).map(|i| i.label.clone()))
            .collect()
    }

    /// **Ctrl+A selects every icon and Escape none**, and each says it
    /// changed something so the frame is redrawn.
    #[test]
    fn ctrl_a_selects_every_icon_and_escape_none() {
        let mut shell = shell_with_three();
        assert_eq!(
            shell.handle_desktop_key(&press_with(Key::A, Modifiers::ctrl())),
            ShellAction::Consumed
        );
        assert_eq!(selected(&shell), ["a", "b", "c"]);
        assert_eq!(
            shell.handle_desktop_key(&press(Key::Escape)),
            ShellAction::Consumed
        );
        assert!(selected(&shell).is_empty());
        // A plain A is the letter, and the desktop does nothing with it.
        assert_eq!(shell.handle_desktop_key(&press(Key::A)), ShellAction::Pass);
        assert!(selected(&shell).is_empty());
    }

    /// **The arrow keys walk the icons.**
    #[test]
    fn the_arrow_keys_walk_the_icons() {
        let mut shell = shell_with_three();
        assert_eq!(
            shell.handle_desktop_key(&press(Key::Down)),
            ShellAction::Consumed
        );
        assert_eq!(selected(&shell), ["a"], "from nothing, the top-left");
        drop(shell.handle_desktop_key(&press(Key::Down)));
        drop(shell.handle_desktop_key(&press(Key::Down)));
        assert_eq!(selected(&shell), ["c"]);
        assert_eq!(
            shell.handle_desktop_key(&press(Key::Down)),
            ShellAction::Pass,
            "nowhere further down, so nothing changed and nothing is redrawn"
        );
        drop(shell.handle_desktop_key(&press(Key::Up)));
        assert_eq!(selected(&shell), ["b"]);
    }

    /// **Enter opens the one selected icon**, through the same door as a
    /// double-click.
    #[test]
    fn enter_opens_the_selected_icon() {
        with_scratch_config("desktop-key-enter", |root| {
            let mut shell = DesktopShell::new(1920, 1080);
            let folder = root.join("Projects");
            std::fs::create_dir(&folder).expect("the scratch root is writable");
            let id = shell.icons.add_icon(
                "Projects",
                icons::IconType::Folder,
                icons::IconAction::OpenPath(folder.clone()),
                200,
                200,
            );
            assert_eq!(
                shell.handle_desktop_key(&press(Key::Enter)),
                ShellAction::Pass,
                "nothing selected, nothing to open"
            );
            shell.icons.select_single(id);
            assert_eq!(
                shell.handle_desktop_key(&press(Key::Enter)),
                ShellAction::Launch(crate::hotkeys::Launch::opening(
                    crate::launcher::FILE_MANAGER,
                    &folder
                ))
            );
        });
    }

    /// A release, and a key the desktop has no use for, are passed on.
    #[test]
    fn keys_the_desktop_has_no_use_for_are_passed_on() {
        let mut shell = shell_with_three();
        let mut up = press(Key::Down);
        up.pressed = false;
        assert_eq!(shell.handle_desktop_key(&up), ShellAction::Pass);
        assert!(
            selected(&shell).is_empty(),
            "a key coming up is not a press"
        );
        assert_eq!(
            shell.handle_desktop_key(&press(Key::Tab)),
            ShellAction::Pass
        );
        // Delete and F2 reach the layer and are not acted on yet.
        shell.icons.select_all();
        assert_eq!(
            shell.handle_desktop_key(&press(Key::Delete)),
            ShellAction::Pass
        );
        assert_eq!(
            shell.icons.icon_ids().len(),
            3,
            "the default icons cannot be removed"
        );
    }
}

/// Shortcuts the user adds to the desktop, and an icon's own menu.
#[cfg(test)]
mod icon_menu_tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::{DesktopShell, PinTarget, ShellAction, icons};
    use appearance::config::testing::with_scratch_config;
    use guitk::event::{Key, KeyEvent, Modifiers};
    use guitk::render::RenderCommand;

    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    /// The first start-menu entry's program and name.
    fn first_entry(shell: &DesktopShell) -> (String, String) {
        let entry = shell.start_menu_entries()[0];
        (entry.executable_path.clone(), entry.name.clone())
    }

    /// "Add to desktop" on the first start-menu entry, through the pin menu
    /// the way a right-click would reach it. Answers the new icon.
    fn add_first_entry(shell: &mut DesktopShell) -> icons::IconId {
        let (exec, _) = first_entry(shell);
        shell.activate_pin_menu_item(
            DesktopShell::MENU_ADD_TO_DESKTOP,
            PinTarget::StartMenuRow(0),
        );
        shell
            .icons
            .icon_ids()
            .into_iter()
            .find(|id| {
                shell.icons.get_icon(*id).is_some_and(|i| {
                    i.action == icons::IconAction::OpenPath(std::path::PathBuf::from(&exec))
                })
            })
            .expect("Add to desktop put no icon on the desktop")
    }

    /// The labels of the menu currently open.
    fn menu_labels(shell: &DesktopShell) -> Vec<String> {
        shell
            .render_desktop_menu()
            .expect("the menu is open")
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Right-click the icon `id`.
    fn right_click(shell: &mut DesktopShell, id: icons::IconId) {
        let icon = shell.icons.get_icon(id).expect("the icon exists");
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = (icon.x as f32 + 10.0, icon.y as f32 + 10.0);
        shell.open_desktop_menu(x, y);
    }

    /// **"Add to desktop" puts a program shortcut on the desktop, named as
    /// the start menu names it, marked for saving** -- and a second time
    /// selects the one already there.
    #[test]
    fn add_to_desktop_puts_a_shortcut_there_once() {
        let mut shell = DesktopShell::new(1920, 1080);
        let (exec, name) = first_entry(&shell);
        let id = add_first_entry(&mut shell);
        let icon = shell.icons.get_icon(id).unwrap();
        assert_eq!(icon.label, name);
        assert_eq!(icon.icon_type, icons::IconType::Executable);
        assert!(icon.added);
        assert!(
            shell.take_icons_dirty(),
            "the new shortcut will not be saved"
        );
        assert_eq!(
            shell.icons.selected_ids(),
            [id],
            "and it is selected, to be seen"
        );

        shell.icons.deselect_all();
        let again = add_first_entry(&mut shell);
        assert_eq!(again, id);
        assert_eq!(
            shell.icons.icon_ids().len(),
            1,
            "a second shortcut to {exec}"
        );
        assert!(!shell.take_icons_dirty(), "nothing new to save");
    }

    /// The pin menu offers it, beside pinning.
    #[test]
    fn the_pin_menu_offers_add_to_desktop() {
        let mut shell = DesktopShell::new(1920, 1080);
        shell.open_pin_menu(PinTarget::StartMenuRow(0), 100.0, 100.0);
        let labels: Vec<String> = shell
            .pin_menu
            .as_ref()
            .expect("the pin menu is open")
            .0
            .render(&appearance::Palette::for_mode(false))
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|l| l == "Add to desktop"), "{labels:?}");
        assert!(labels.iter().any(|l| l == "Pin to taskbar"), "{labels:?}");
    }

    /// **A right-click on an icon opens the icon's menu, not the desktop's**,
    /// offering what that icon can do: a program can be pinned, and what the
    /// user added can be removed; a default folder can be opened and renamed.
    #[test]
    fn a_right_click_on_an_icon_opens_its_own_menu() {
        with_scratch_config("icon-menu-items", |root| {
            let mut shell = DesktopShell::new(1920, 1080);
            let program = add_first_entry(&mut shell);
            right_click(&mut shell, program);
            let labels = menu_labels(&shell);
            for want in ["Open", "Pin to taskbar", "Remove from desktop"] {
                assert!(labels.iter().any(|l| l == want), "no {want}: {labels:?}");
            }
            assert!(
                !labels.iter().any(|l| l == "View"),
                "the desktop's menu opened over an icon: {labels:?}"
            );

            let folder = root.join("Stuff");
            std::fs::create_dir(&folder).unwrap();
            let plain = shell.icons.add_icon(
                "Stuff",
                icons::IconType::Folder,
                icons::IconAction::OpenPath(folder),
                900,
                500,
            );
            right_click(&mut shell, plain);
            assert_eq!(
                menu_labels(&shell),
                ["Open", "Rename"],
                "a default folder can be opened and renamed, not pinned or removed"
            );
            assert_eq!(
                shell.icons.selected_ids(),
                [plain],
                "the icon clicked is selected"
            );
        });
    }

    /// **The icon's menu does what it says**: Open opens, by the rule a
    /// double-click uses; Pin pins and unpins; Remove removes.
    #[test]
    fn the_icon_menu_opens_pins_and_removes() {
        with_scratch_config("icon-menu-actions", |root| {
            let mut shell = DesktopShell::new(1920, 1080);
            let folder = root.join("Stuff");
            std::fs::create_dir(&folder).unwrap();
            let plain = shell.icons.add_icon(
                "Stuff",
                icons::IconType::Folder,
                icons::IconAction::OpenPath(folder.clone()),
                900,
                500,
            );
            right_click(&mut shell, plain);
            assert_eq!(
                shell.activate_desktop_menu_item(DesktopShell::MENU_ICON_OPEN),
                ShellAction::Launch(crate::hotkeys::Launch::opening(
                    crate::launcher::FILE_MANAGER,
                    &folder
                ))
            );

            let program = add_first_entry(&mut shell);
            let (exec, _) = first_entry(&shell);
            right_click(&mut shell, program);
            assert!(
                shell
                    .activate_desktop_menu_item(DesktopShell::MENU_ICON_PIN)
                    .changed()
            );
            assert!(shell.is_pinned(&exec));
            right_click(&mut shell, program);
            assert!(
                menu_labels(&shell)
                    .iter()
                    .any(|l| l == "Unpin from taskbar")
            );
            assert!(
                shell
                    .activate_desktop_menu_item(DesktopShell::MENU_ICON_PIN)
                    .changed()
            );
            assert!(!shell.is_pinned(&exec));

            assert!(shell.take_icons_dirty(), "adding it marked the layout");
            right_click(&mut shell, program);
            assert!(
                shell
                    .activate_desktop_menu_item(DesktopShell::MENU_ICON_REMOVE)
                    .changed()
            );
            assert!(shell.icons.get_icon(program).is_none());
            assert!(shell.take_icons_dirty(), "the removal will not be saved");
        });
    }

    /// Open from the icon's menu with the keyboard starts what it opens --
    /// the keyboard route can carry a launch as the pointer's can.
    #[test]
    fn open_from_the_icon_menu_by_keyboard_starts_it() {
        with_scratch_config("icon-menu-keyboard", |root| {
            let mut shell = DesktopShell::new(1920, 1080);
            let folder = root.join("Stuff");
            std::fs::create_dir(&folder).unwrap();
            let plain = shell.icons.add_icon(
                "Stuff",
                icons::IconType::Folder,
                icons::IconAction::OpenPath(folder.clone()),
                900,
                500,
            );
            right_click(&mut shell, plain);
            drop(shell.handle_hotkey(&press(Key::Down)));
            let outcome = shell.handle_hotkey(&press(Key::Enter));
            assert_eq!(
                outcome.launches,
                [crate::hotkeys::Launch::opening(
                    crate::launcher::FILE_MANAGER,
                    &folder
                )]
            );
        });
    }

    /// **Delete removes the selected shortcuts the user added, and leaves
    /// the defaults**, which would only come back at the next login.
    #[test]
    fn delete_removes_added_shortcuts_and_leaves_the_defaults() {
        let mut shell = DesktopShell::new(1920, 1080);
        let default = shell.icons.add_icon(
            "This PC",
            icons::IconType::Computer,
            icons::IconAction::LaunchSystem(icons::THIS_PC.to_string()),
            0,
            0,
        );
        let added = add_first_entry(&mut shell);
        assert!(shell.take_icons_dirty(), "adding it marked the layout");
        shell.icons.select_all();

        assert_eq!(
            shell.handle_desktop_key(&press(Key::Delete)),
            ShellAction::Consumed
        );
        assert!(shell.icons.get_icon(added).is_none());
        assert!(shell.icons.get_icon(default).is_some());
        assert!(shell.take_icons_dirty());
        assert_eq!(
            shell.handle_desktop_key(&press(Key::Delete)),
            ShellAction::Pass,
            "only a default left selected: nothing to remove"
        );
    }

    /// **A shortcut added on the desktop is there after a login**, through
    /// the shell's own save and load.
    #[test]
    fn an_added_shortcut_is_there_after_a_login() {
        with_scratch_config("icon-shortcut-login", |_root| {
            let mut shell = DesktopShell::new(1920, 1080);
            shell.populate_icons();
            let (exec, name) = first_entry(&shell);
            add_first_entry(&mut shell);
            assert!(shell.take_icons_dirty());
            shell
                .save_icon_layout()
                .expect("the scratch config directory is writable");

            let mut next = DesktopShell::new(1920, 1080);
            next.populate_icons();
            let back = next
                .icons
                .icon_ids()
                .into_iter()
                .filter_map(|id| next.icons.get_icon(id))
                .find(|i| i.action == icons::IconAction::OpenPath(std::path::PathBuf::from(&exec)))
                .map(|i| (i.label.clone(), i.added));
            assert_eq!(back, Some((name, true)));
        });
    }
}

/// Renaming a desktop icon in place, through the shell's own routes.
#[cfg(test)]
mod rename_tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::{DesktopShell, MouseEvent, MouseEventKind, ShellAction, icons};
    use guitk::event::{Key, KeyEvent, Modifiers, MouseButton};

    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn typed(ch: char) -> KeyEvent {
        KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        }
    }

    /// A shell with one icon, "Old", selected.
    fn shell_with_one() -> (DesktopShell, icons::IconId) {
        let mut shell = DesktopShell::new(1920, 1080);
        let id = shell.icons.add_icon(
            "Old",
            icons::IconType::Folder,
            icons::IconAction::Custom("old".to_string()),
            200,
            200,
        );
        shell.icons.select_single(id);
        (shell, id)
    }

    fn label(shell: &DesktopShell, id: icons::IconId) -> String {
        shell.icons.get_icon(id).unwrap().label.clone()
    }

    /// **F2 renames the selected icon; Enter keeps the name and marks the
    /// layout for saving.** Before 2026-09-25 F2 reached the icon layer and
    /// nothing acted on it.
    #[test]
    fn f2_renames_and_enter_keeps_it() {
        let (mut shell, id) = shell_with_one();
        assert_eq!(
            shell.handle_desktop_key(&press(Key::F2)),
            ShellAction::Consumed
        );
        assert_eq!(shell.icons.renaming(), Some(id));
        for ch in "Projects".chars() {
            assert_eq!(shell.handle_desktop_key(&typed(ch)), ShellAction::Consumed);
        }
        assert_eq!(
            shell.handle_desktop_key(&press(Key::Enter)),
            ShellAction::Consumed
        );
        assert_eq!(label(&shell, id), "Projects");
        assert!(shell.take_icons_dirty(), "the new name will not be saved");
    }

    /// **Delete while a name is being typed deletes a letter, not the icon**
    /// -- every key belongs to the field while it is open.
    #[test]
    fn delete_while_renaming_edits_the_name() {
        let mut shell = DesktopShell::new(1920, 1080);
        shell.activate_pin_menu_item(
            DesktopShell::MENU_ADD_TO_DESKTOP,
            super::PinTarget::StartMenuRow(0),
        );
        let id = shell.icons.selected_ids()[0];
        assert!(
            shell.icons.get_icon(id).unwrap().added,
            "the fixture must be removable"
        );
        drop(shell.handle_desktop_key(&press(Key::F2)));
        drop(shell.handle_desktop_key(&press(Key::Delete)));
        assert!(
            shell.icons.get_icon(id).is_some(),
            "Delete removed the icon being renamed"
        );
    }

    /// **A click away keeps the name**, as on every desktop -- and the click
    /// still does what it does.
    #[test]
    fn a_click_away_keeps_the_name() {
        let (mut shell, id) = shell_with_one();
        drop(shell.handle_desktop_key(&press(Key::F2)));
        for ch in "Kept".chars() {
            drop(shell.handle_desktop_key(&typed(ch)));
        }
        let away = MouseEvent {
            x: 1000.0,
            y: 600.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        drop(shell.handle_mouse(&away));
        assert_eq!(shell.icons.renaming(), None);
        assert_eq!(label(&shell, id), "Kept");
        assert!(shell.take_icons_dirty());
        assert!(
            shell.icons.selected_ids().is_empty(),
            "and the press on empty desktop still cleared the selection"
        );
    }

    /// A press inside the field is the field's: it places the caret and the
    /// rename goes on.
    #[test]
    fn a_press_in_the_field_keeps_renaming() {
        let (mut shell, id) = shell_with_one();
        drop(shell.handle_desktop_key(&press(Key::F2)));
        let icon = shell.icons.get_icon(id).unwrap();
        // Well inside the field, which sits under the glyph in the cell.
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = (icon.x as f32 + 20.0, icon.y as f32 + 8.0 + 48.0 + 4.0 + 6.0);
        assert!(
            shell.icons.rename_field_contains(x, y),
            "the fixture missed the field"
        );
        let inside = MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        assert_eq!(shell.handle_mouse(&inside), ShellAction::Consumed);
        assert_eq!(shell.icons.renaming(), Some(id));
    }

    /// **"Rename" on an icon's own menu starts it too**, on the icon the menu
    /// was opened over.
    #[test]
    fn rename_from_the_icon_menu() {
        let (mut shell, id) = shell_with_one();
        let icon = shell.icons.get_icon(id).unwrap();
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = (icon.x as f32 + 10.0, icon.y as f32 + 10.0);
        shell.open_desktop_menu(x, y);
        assert_eq!(
            shell.activate_desktop_menu_item(DesktopShell::MENU_ICON_RENAME),
            ShellAction::Consumed
        );
        assert_eq!(shell.icons.renaming(), Some(id));
    }

    /// Keeping a rename from outside -- the session does this when the
    /// keyboard leaves for another program -- keeps the name and marks it.
    #[test]
    fn a_rename_kept_from_outside_is_saved() {
        let (mut shell, id) = shell_with_one();
        drop(shell.handle_desktop_key(&press(Key::F2)));
        drop(shell.handle_desktop_key(&typed('Z')));
        shell.commit_icon_rename();
        assert_eq!(label(&shell, id), "Z");
        assert!(shell.take_icons_dirty());
    }
}

/// Carrying a program between the start menu, the taskbar and the desktop --
/// `design.txt` line 712, "drag and drop icons between pinned apps, desktop,
/// and start menu".
///
/// Every test that can drop on the taskbar runs inside `with_scratch_config`:
/// a pin writes `taskbar.yaml`, and a test that wrote the developer's own is
/// what `scripts/check-scratch-config.py` exists to refuse.
#[cfg(test)]
mod carry_tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_precision_loss
    )]

    use super::{
        DesktopShell, MouseButton, MouseEvent, MouseEventKind, ShellAction, WindowId, WindowInfo,
        WindowList, icons,
    };
    use appearance::config::testing::with_scratch_config;
    use guitk::render::RenderCommand;
    use std::path::PathBuf;

    fn shell() -> DesktopShell {
        DesktopShell::new(1920, 1080)
    }

    fn ev(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
        MouseEvent { x, y, kind }
    }

    fn press(shell: &mut DesktopShell, at: (f32, f32)) -> ShellAction {
        shell.handle_mouse(&ev(at.0, at.1, MouseEventKind::Press(MouseButton::Left)))
    }

    fn move_to(shell: &mut DesktopShell, at: (f32, f32)) {
        shell.handle_mouse(&ev(at.0, at.1, MouseEventKind::Move));
    }

    fn release(shell: &mut DesktopShell, at: (f32, f32)) -> ShellAction {
        shell.handle_mouse(&ev(at.0, at.1, MouseEventKind::Release(MouseButton::Left)))
    }

    /// Press at `from`, move to `to` in four steps -- the first already past
    /// the drag threshold, as a real pointer's would be over this distance --
    /// and let go there. Answers what the release asked for.
    fn carry(shell: &mut DesktopShell, from: (f32, f32), to: (f32, f32)) -> ShellAction {
        assert_eq!(
            press(shell, from),
            ShellAction::Consumed,
            "the press was not taken"
        );
        for step in 1..=4 {
            let t = step as f32 / 4.0;
            move_to(
                shell,
                (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t),
            );
        }
        release(shell, to)
    }

    fn row_centre(shell: &DesktopShell, row: usize) -> (f32, f32) {
        let r = shell.start_menu_row_rect(row);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// The program on a start-menu row, and its name.
    fn app(shell: &DesktopShell, row: usize) -> (String, String) {
        let entry = shell.start_menu_entries()[row];
        (entry.executable_path.clone(), entry.name.clone())
    }

    fn pinned(shell: &DesktopShell) -> Vec<String> {
        shell
            .pinned_apps()
            .iter()
            .map(|app| app.exec_path.clone())
            .collect()
    }

    fn button_centre(shell: &DesktopShell, index: usize) -> (f32, f32) {
        let r = shell.taskbar_button_rect(index);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// The desktop's shortcut to `exec`, if it has one.
    fn shortcut_to(shell: &DesktopShell, exec: &str) -> Option<icons::IconId> {
        let action = icons::IconAction::OpenPath(PathBuf::from(exec));
        shell
            .icons
            .icon_ids()
            .into_iter()
            .find(|&id| shell.icons.get_icon(id).is_some_and(|i| i.action == action))
    }

    /// The middle of an icon's cell, where a press picks it up.
    fn icon_centre(shell: &DesktopShell, id: icons::IconId) -> (f32, f32) {
        let icon = shell.icons.get_icon(id).unwrap();
        let grid = shell.icons.grid();
        (
            icon.x as f32 + grid.cell_width() as f32 / 2.0,
            icon.y as f32 + grid.cell_height() as f32 / 2.0,
        )
    }

    fn position(shell: &DesktopShell, id: icons::IconId) -> (i32, i32) {
        let icon = shell.icons.get_icon(id).unwrap();
        (icon.x, icon.y)
    }

    /// What the label that follows a carried program says, line by line --
    /// `None` when there is no label.
    fn carried(shell: &DesktopShell) -> Option<Vec<String>> {
        let tree = shell.render_carry()?;
        Some(
            tree.commands
                .iter()
                .filter_map(|cmd| match cmd {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect(),
        )
    }

    /// An application window on the desktop being shown.
    fn window(id: u64, rect: (i32, i32, u32, u32)) -> WindowInfo {
        WindowInfo::new(id, 1, format!("window {id}")).at(rect.0, rect.1, rect.2, rect.3)
    }

    // ---- a click is still a click -----------------------------------------------------

    /// The press only takes hold: whether it was a click or the start of a
    /// drag is not known until the release, so starting the program on the
    /// press -- as the row used to -- made it impossible to drag.
    #[test]
    fn a_start_menu_row_starts_its_program_on_the_release() {
        let mut shell = shell();
        shell.toggle_start_menu();
        let (exec, _) = app(&shell, 0);
        let at = row_centre(&shell, 0);

        assert_eq!(press(&mut shell, at), ShellAction::Consumed);
        assert!(
            shell.start_menu_open,
            "the menu closed under a press that may yet be a drag"
        );
        assert_eq!(
            release(&mut shell, at),
            ShellAction::Launch(crate::hotkeys::Launch::program(&exec))
        );
        assert!(
            !shell.start_menu_open,
            "picking a program leaves the menu up"
        );
    }

    /// A hand is not perfectly still: a press that moves less than the drag
    /// threshold before it is let go is a click.
    #[test]
    fn a_press_that_wobbles_is_still_a_click() {
        let mut shell = shell();
        shell.toggle_start_menu();
        let (exec, _) = app(&shell, 0);
        let (x, y) = row_centre(&shell, 0);

        press(&mut shell, (x, y));
        move_to(&mut shell, (x + 2.0, y + 1.0));
        assert!(carried(&shell).is_none(), "a wobble drew a carried label");
        assert_eq!(
            release(&mut shell, (x + 2.0, y + 1.0)),
            ShellAction::Launch(crate::hotkeys::Launch::program(&exec))
        );
    }

    // ---- from the start menu --------------------------------------------------------

    /// Dropped on the taskbar, the program is pinned in the gap it was let go
    /// in -- here between the two pins already there.
    #[test]
    fn a_row_dropped_on_the_taskbar_is_pinned_in_the_gap_it_was_let_go_in() {
        with_scratch_config("carry-row-to-bar", |_root| {
            let mut shell = shell();
            let ((a, a_name), (b, b_name), (c, _)) =
                (app(&shell, 0), app(&shell, 1), app(&shell, 2));
            shell.pin_app(&a, &a_name);
            shell.pin_app(&b, &b_name);
            shell.toggle_start_menu();

            // The left quarter of the second button: before it.
            let second = shell.taskbar_button_rect(1);
            let to = (second.x + second.w / 4.0, second.y + second.h / 2.0);
            let from = row_centre(&shell, 2);
            assert_eq!(
                carry(&mut shell, from, to),
                ShellAction::Consumed,
                "a drag started the program"
            );

            assert_eq!(pinned(&shell), [a.clone(), c, b]);
            assert!(!shell.start_menu_open, "the menu stayed up after the drop");
            assert!(
                shortcut_to(&shell, &a).is_none(),
                "the taskbar drop made a shortcut"
            );

            // And written down: a new login sees the same bar.
            let mut restarted = DesktopShell::new(1920, 1080);
            restarted.load_pinned();
            assert_eq!(pinned(&restarted), pinned(&shell));
        });
    }

    /// Past the last pin, and anywhere else on the bar that is not a pin, is
    /// the end of the run.
    #[test]
    fn a_row_dropped_past_the_last_pin_goes_at_the_end() {
        with_scratch_config("carry-row-to-bar-end", |_root| {
            let mut shell = shell();
            let ((a, a_name), (b, _)) = (app(&shell, 0), app(&shell, 1));
            shell.pin_app(&a, &a_name);
            shell.toggle_start_menu();

            let last = shell.taskbar_button_rect(0);
            let to = (last.x + last.w * 3.0, last.y + last.h / 2.0);
            let from = row_centre(&shell, 1);
            carry(&mut shell, from, to);

            assert_eq!(pinned(&shell), [a, b]);
        });
    }

    /// A program that is pinned already moves to where it was dropped, and
    /// is not pinned twice.
    #[test]
    fn a_row_already_pinned_moves_its_pin_rather_than_adding_one() {
        with_scratch_config("carry-row-moves-pin", |_root| {
            let mut shell = shell();
            let ((a, a_name), (b, b_name)) = (app(&shell, 0), app(&shell, 1));
            shell.pin_app(&a, &a_name);
            shell.pin_app(&b, &b_name);
            shell.toggle_start_menu();

            let second = shell.taskbar_button_rect(1);
            let to = (second.x + second.w * 0.75, second.y + second.h / 2.0);
            let from = row_centre(&shell, 0);
            carry(&mut shell, from, to);
            assert_eq!(pinned(&shell), [b.clone(), a.clone()]);

            // Dropped into its own gap, either side of it, nothing moves.
            shell.toggle_start_menu();
            let own = shell.taskbar_button_rect(1);
            let to = (own.x + own.w * 0.25, own.y + own.h / 2.0);
            let from = row_centre(&shell, 0);
            carry(&mut shell, from, to);
            assert_eq!(pinned(&shell), [b, a]);
        });
    }

    /// A program already pinned, carried into the gap between two other pins:
    /// the case where its own old place changes which gap is meant.
    #[test]
    fn a_pinned_program_carried_into_a_middle_gap_lands_there() {
        with_scratch_config("carry-row-middle-gap", |_root| {
            let mut shell = shell();
            let apps: Vec<(String, String)> = (0..3).map(|row| app(&shell, row)).collect();
            for (exec, name) in &apps {
                shell.pin_app(exec, name);
            }
            shell.toggle_start_menu();

            // Row 0's program is the first pin. The left quarter of the
            // third button: after the second, before the third.
            let third = shell.taskbar_button_rect(2);
            let to = (third.x + third.w / 4.0, third.y + third.h / 2.0);
            let from = row_centre(&shell, 0);
            carry(&mut shell, from, to);
            assert_eq!(
                pinned(&shell),
                [apps[1].0.clone(), apps[0].0.clone(), apps[2].0.clone()]
            );
        });
    }

    /// Dropped on the desktop, a shortcut to the program appears where it was
    /// let go, and is saved.
    #[test]
    fn a_row_dropped_on_the_desktop_puts_a_shortcut_where_it_was_let_go() {
        with_scratch_config("carry-row-to-desktop", |_root| {
            let mut shell = shell();
            shell.toggle_start_menu();
            let (exec, name) = app(&shell, 0);
            let to = (1200.0, 400.0);
            let from = row_centre(&shell, 0);
            assert_eq!(carry(&mut shell, from, to), ShellAction::Consumed);

            let id = shortcut_to(&shell, &exec).expect("no shortcut appeared");
            let icon = shell.icons.get_icon(id).unwrap();
            assert_eq!(icon.label, name);
            assert_eq!(icon.icon_type, icons::IconType::Executable);
            assert!(icon.added, "a shortcut the user made must be removable");
            // On the grid, in the cell under the pointer: that cell was free.
            let grid = shell.icons.grid();
            let (x, y) = (icon.x as f32, icon.y as f32);
            assert!(
                (x..x + grid.cell_width() as f32).contains(&to.0)
                    && (y..y + grid.cell_height() as f32).contains(&to.1),
                "the shortcut is at ({x}, {y}), not under where it was let go"
            );
            assert!(shell.take_icons_dirty(), "the new shortcut is not saved");
            assert!(pinned(&shell).is_empty(), "a desktop drop pinned it");
            assert!(!shell.start_menu_open);
        });
    }

    /// Dropping it where there is a shortcut to it already selects that one
    /// rather than making a second.
    #[test]
    fn a_row_dropped_on_the_desktop_twice_leaves_one_shortcut() {
        with_scratch_config("carry-row-to-desktop-twice", |_root| {
            let mut shell = shell();
            let (exec, _) = app(&shell, 0);
            for to in [(1200.0, 400.0), (600.0, 700.0)] {
                shell.toggle_start_menu();
                let from = row_centre(&shell, 0);
                carry(&mut shell, from, to);
            }
            let action = icons::IconAction::OpenPath(PathBuf::from(&exec));
            let count = shell
                .icons
                .icon_ids()
                .into_iter()
                .filter(|&id| shell.icons.get_icon(id).is_some_and(|i| i.action == action))
                .count();
            assert_eq!(count, 1);
            assert_eq!(
                shell.icons.selected_ids(),
                [shortcut_to(&shell, &exec).unwrap()]
            );
        });
    }

    /// Carried and brought back to the menu, nothing is asked for: the menu
    /// stays up to be used, nothing starts, and nothing is pinned or added.
    #[test]
    fn a_row_let_go_back_on_the_menu_does_nothing() {
        with_scratch_config("carry-row-back", |_root| {
            let mut shell = shell();
            shell.toggle_start_menu();
            let (exec, _) = app(&shell, 0);
            let (from, to) = (row_centre(&shell, 0), row_centre(&shell, 3));
            assert_eq!(carry(&mut shell, from, to), ShellAction::Consumed);

            assert!(shell.start_menu_open);
            assert!(pinned(&shell).is_empty());
            assert!(shortcut_to(&shell, &exec).is_none());
            assert!(!shell.take_icons_dirty());
            assert!(carried(&shell).is_none(), "the label outlived the drag");
        });
    }

    /// Let go over somebody's window, it goes nowhere: nothing here can hand a
    /// program to another program yet, and a shortcut made behind the window
    /// would turn up somewhere the user was not pointing.
    #[test]
    fn a_row_let_go_over_a_window_does_nothing() {
        with_scratch_config("carry-row-over-window", |_root| {
            let mut shell = shell();
            shell.apply_window_list(&WindowList::new(0, vec![window(1, (800, 200, 600, 400))]));
            shell.toggle_start_menu();
            let (exec, _) = app(&shell, 0);
            let from = row_centre(&shell, 0);
            assert_eq!(
                carry(&mut shell, from, (1000.0, 400.0)),
                ShellAction::Consumed
            );

            assert!(shortcut_to(&shell, &exec).is_none());
            assert!(pinned(&shell).is_empty());
            // Beside the window, the same drop works.
            shell.toggle_start_menu();
            let from = row_centre(&shell, 0);
            carry(&mut shell, from, (1600.0, 400.0));
            assert!(shortcut_to(&shell, &exec).is_some());
        });
    }

    /// Escape mid-drag -- which dismisses every popup -- ends the drag too.
    /// It closed the menu by writing its flag directly and left the drag to
    /// finish on the release.
    #[test]
    fn dismissing_the_popups_ends_a_drag_from_the_menu() {
        with_scratch_config("carry-row-dismissed", |_root| {
            let mut shell = shell();
            shell.toggle_start_menu();
            let (exec, _) = app(&shell, 0);
            let from = row_centre(&shell, 0);
            press(&mut shell, from);
            move_to(&mut shell, (1200.0, 400.0));
            assert!(shell.dismiss_popups());
            assert!(carried(&shell).is_none(), "the label outlived the menu");
            release(&mut shell, (1200.0, 400.0));
            assert!(
                shortcut_to(&shell, &exec).is_none(),
                "the dismissed drag still dropped"
            );
        });
    }

    /// Closing the menu mid-drag -- a key, a hotkey, anything that closes it --
    /// ends the drag: the release has nothing left to drop.
    #[test]
    fn closing_the_menu_ends_a_drag_from_it() {
        with_scratch_config("carry-row-menu-closed", |_root| {
            let mut shell = shell();
            shell.toggle_start_menu();
            let (exec, _) = app(&shell, 0);
            let from = row_centre(&shell, 0);
            press(&mut shell, from);
            move_to(&mut shell, (1200.0, 400.0));
            shell.close_start_menu();
            assert!(carried(&shell).is_none());
            release(&mut shell, (1200.0, 400.0));
            assert!(shortcut_to(&shell, &exec).is_none());
        });
    }

    // ---- what the carried label says ----------------------------------------------------

    /// The label names the program and says what letting go will do, and says
    /// nothing where letting go does nothing.
    #[test]
    fn the_carried_label_says_what_letting_go_will_do() {
        with_scratch_config("carry-label", |_root| {
            let mut shell = shell();
            shell.apply_window_list(&WindowList::new(0, vec![window(1, (800, 200, 600, 400))]));
            shell.toggle_start_menu();
            let (_, name) = app(&shell, 0);
            let from = row_centre(&shell, 0);
            press(&mut shell, from);
            assert!(carried(&shell).is_none(), "a label before any drag");

            move_to(&mut shell, (1600.0, 400.0));
            assert_eq!(
                carried(&shell).unwrap(),
                [name.clone(), "Add to desktop".into()]
            );

            let bar = shell.taskbar_rect();
            move_to(&mut shell, (bar.x + bar.w / 2.0, bar.y + bar.h / 2.0));
            assert_eq!(
                carried(&shell).unwrap(),
                [name.clone(), "Pin to taskbar".into()]
            );

            move_to(&mut shell, (1000.0, 400.0));
            assert_eq!(
                carried(&shell).unwrap(),
                std::slice::from_ref(&name),
                "over a window"
            );

            let menu = row_centre(&shell, 2);
            move_to(&mut shell, menu);
            assert_eq!(carried(&shell).unwrap(), [name], "back on the menu");

            release(&mut shell, menu);
            assert!(carried(&shell).is_none());
        });
    }

    /// Over a taskbar along the bottom edge -- exactly where the label has
    /// something to say -- it flips above the pointer instead of running off
    /// the screen, and left of it in the corner.
    #[test]
    fn the_carried_label_stays_on_the_screen() {
        let mut shell = shell();
        shell.toggle_start_menu();
        let from = row_centre(&shell, 0);
        press(&mut shell, from);
        let bar = shell.taskbar_rect();
        for x in [bar.x + bar.w / 2.0, bar.x + bar.w - 2.0] {
            move_to(&mut shell, (x, bar.y + bar.h - 2.0));
            let tree = shell.render_carry().expect("no label over the taskbar");
            for cmd in &tree.commands {
                if let RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } = cmd
                {
                    assert!(*x >= 0.0 && *y >= 0.0, "off the top or left");
                    assert!(x + width <= 1920.0, "off the right edge at {x}");
                    assert!(y + height <= 1080.0, "off the bottom edge at {y}");
                }
            }
        }
    }

    // ---- from the taskbar ------------------------------------------------------------------

    /// A pinned button carried up onto the desktop leaves a shortcut there,
    /// and the pin stays: a drag between two places copies.
    #[test]
    fn a_pin_carried_onto_the_desktop_makes_a_shortcut_and_stays_pinned() {
        with_scratch_config("carry-pin-to-desktop", |_root| {
            let mut shell = shell();
            let ((a, a_name), (b, b_name)) = (app(&shell, 0), app(&shell, 1));
            shell.pin_app(&a, &a_name);
            shell.pin_app(&b, &b_name);

            let from = button_centre(&shell, 0);
            press(&mut shell, from);
            move_to(&mut shell, (1200.0, 400.0));
            assert_eq!(
                carried(&shell).unwrap(),
                [a_name.clone(), "Add to desktop".into()]
            );
            assert_eq!(
                release(&mut shell, (1200.0, 400.0)),
                ShellAction::Consumed,
                "a drag started the program"
            );

            assert_eq!(pinned(&shell), [a.clone(), b]);
            let id = shortcut_to(&shell, &a).expect("no shortcut appeared");
            assert_eq!(shell.icons.get_icon(id).unwrap().label, a_name);
            assert!(shell.take_icons_dirty());
        });
    }

    /// Up off the bar and back down onto it, the drag is a reorder again.
    #[test]
    fn a_pin_carried_off_the_bar_and_back_still_reorders() {
        with_scratch_config("carry-pin-off-and-back", |_root| {
            let mut shell = shell();
            let ((a, a_name), (b, b_name)) = (app(&shell, 0), app(&shell, 1));
            shell.pin_app(&a, &a_name);
            shell.pin_app(&b, &b_name);

            let from = button_centre(&shell, 0);
            let second = shell.taskbar_button_rect(1);
            let back = (second.x + second.w * 0.75, second.y + second.h / 2.0);
            press(&mut shell, from);
            move_to(&mut shell, (from.0, 500.0));
            assert!(carried(&shell).is_some(), "no label off the bar");
            move_to(&mut shell, back);
            assert!(
                carried(&shell).is_none(),
                "a label on the bar, where it rearranges"
            );
            assert_eq!(release(&mut shell, back), ShellAction::Consumed);

            assert_eq!(pinned(&shell), [b, a.clone()]);
            assert!(shortcut_to(&shell, &a).is_none());
        });
    }

    /// A pin carried off the bar and let go over a window goes nowhere, and
    /// the pin is where it was.
    #[test]
    fn a_pin_let_go_over_a_window_does_nothing() {
        with_scratch_config("carry-pin-over-window", |_root| {
            let mut shell = shell();
            shell.apply_window_list(&WindowList::new(0, vec![window(1, (800, 200, 600, 400))]));
            let (a, a_name) = app(&shell, 0);
            shell.pin_app(&a, &a_name);
            // The window has a button of its own now, after the pin.
            let from = button_centre(&shell, 0);
            assert_eq!(
                carry(&mut shell, from, (1000.0, 400.0)),
                ShellAction::Consumed
            );

            assert_eq!(pinned(&shell), std::slice::from_ref(&a));
            assert!(shortcut_to(&shell, &a).is_none());
        });
    }

    // ---- from the desktop --------------------------------------------------------------------

    /// A program's icon carried onto the taskbar is pinned there, and the icon
    /// stays where it was.
    #[test]
    fn a_program_icon_dropped_on_the_taskbar_is_pinned_and_stays_put() {
        with_scratch_config("carry-icon-to-bar", |_root| {
            let mut shell = shell();
            let ((a, a_name), (b, b_name)) = (app(&shell, 0), app(&shell, 1));
            shell.pin_app(&a, &a_name);
            let (id, _) = shell.icons.add_shortcut(
                &b_name,
                icons::IconType::Executable,
                icons::IconAction::OpenPath(PathBuf::from(&b)),
            );
            let was = position(&shell, id);
            let from = icon_centre(&shell, id);

            // Before the pin already there.
            let first = shell.taskbar_button_rect(0);
            let to = (first.x + first.w / 4.0, first.y + first.h / 2.0);
            press(&mut shell, from);
            move_to(&mut shell, (from.0 + 40.0, from.1));
            move_to(&mut shell, to);
            assert_eq!(
                carried(&shell).unwrap(),
                [b_name, "Pin to taskbar".into()],
                "the icon's own ghost is under the bar; the label is what shows"
            );
            assert_eq!(release(&mut shell, to), ShellAction::Consumed);

            assert_eq!(pinned(&shell), [b, a]);
            assert_eq!(position(&shell, id), was, "the icon moved");
            assert!(!shell.take_icons_dirty(), "nothing on the desktop changed");
            assert!(!shell.icons.is_interacting(), "the drag was left open");
        });
    }

    /// Several program icons dropped together are pinned in their order, not
    /// stacked up reversed in the one gap.
    #[test]
    fn program_icons_dropped_together_keep_their_order() {
        with_scratch_config("carry-icons-to-bar", |_root| {
            let mut shell = shell();
            let ((a, a_name), (b, b_name)) = (app(&shell, 0), app(&shell, 1));
            let (first, _) = shell.icons.add_shortcut(
                &a_name,
                icons::IconType::Executable,
                icons::IconAction::OpenPath(PathBuf::from(&a)),
            );
            shell.icons.add_shortcut(
                &b_name,
                icons::IconType::Executable,
                icons::IconAction::OpenPath(PathBuf::from(&b)),
            );
            shell.icons.select_all();
            let from = icon_centre(&shell, first);
            let bar = shell.taskbar_rect();
            let to = (bar.x + bar.w / 2.0, bar.y + bar.h / 2.0);
            // Not `carry`: a press on an icon that is already selected
            // changes nothing anyone can see, so the shell answers it `Pass`
            // -- and the drag is under way all the same.
            press(&mut shell, from);
            move_to(&mut shell, (from.0 + 40.0, from.1));
            move_to(&mut shell, to);
            assert_eq!(release(&mut shell, to), ShellAction::Consumed);

            assert_eq!(pinned(&shell), [a, b]);
        });
    }

    /// A folder is not something a taskbar button can start: dropped on the
    /// taskbar it pins nothing and stays where it was -- and the outline of
    /// where it would land on the desktop is not drawn while it is over the
    /// bar, because it would not land there.
    #[test]
    fn a_folder_dropped_on_the_taskbar_pins_nothing_and_stays_put() {
        with_scratch_config("carry-folder-to-bar", |_root| {
            let mut shell = shell();
            let (id, _) = shell.icons.add_shortcut(
                "Projects",
                icons::IconType::Folder,
                icons::IconAction::OpenPath(PathBuf::from("/home/user/projects")),
            );
            let was = position(&shell, id);
            let from = icon_centre(&shell, id);
            let bar = shell.taskbar_rect();
            let to = (bar.x + bar.w / 2.0, bar.y + bar.h / 2.0);

            // Compared as text: a render command has no equality of its own.
            let drawn = |cmds: Vec<RenderCommand>| format!("{cmds:?}");
            press(&mut shell, from);
            move_to(&mut shell, (from.0 + 40.0, from.1 + 40.0));
            let palette = crate::Palette::from_settings(&shell.appearance);
            assert_eq!(
                drawn(shell.render_icons()),
                drawn(shell.icons.render(&palette)),
                "over the desktop the outline is drawn"
            );
            move_to(&mut shell, to);
            assert_eq!(
                drawn(shell.render_icons()),
                drawn(shell.icons.render_dropping_elsewhere(&palette)),
                "over the taskbar the outline promises a landing the drop breaks"
            );
            assert_ne!(
                drawn(shell.icons.render(&palette)),
                drawn(shell.icons.render_dropping_elsewhere(&palette))
            );
            assert!(carried(&shell).is_none(), "a label for a folder");
            release(&mut shell, to);

            assert!(pinned(&shell).is_empty());
            assert_eq!(position(&shell, id), was);
            assert!(!shell.icons.is_interacting());
        });
    }

    /// A click on one of two selected program icons, let go a pixel over
    /// the taskbar, is still a click: it selects just that icon, and pins
    /// nothing -- only a drag that got under way is a drop.
    #[test]
    fn a_click_let_go_just_over_the_taskbar_is_still_a_click() {
        with_scratch_config("carry-icon-click-at-bar", |_root| {
            let mut shell = shell();
            shell.icons.set_arrangement(icons::ArrangementMode::Free);
            let ((a, a_name), (b, b_name)) = (app(&shell, 0), app(&shell, 1));
            let bar_top = shell.taskbar_rect().y;
            let cell_h = shell.icons.grid().cell_height() as f32;
            // Flush against the bar: its lowest pixel is the one above it.
            let (low, _) = shell.icons.add_shortcut_at(
                &a_name,
                icons::IconType::Executable,
                icons::IconAction::OpenPath(PathBuf::from(&a)),
                600.0,
                bar_top - cell_h / 2.0,
            );
            shell.icons.add_shortcut(
                &b_name,
                icons::IconType::Executable,
                icons::IconAction::OpenPath(PathBuf::from(&b)),
            );
            shell.icons.select_all();
            let x = icon_centre(&shell, low).0;

            // Three pixels, under the drag threshold, and over the bar.
            press(&mut shell, (x, bar_top - 1.0));
            move_to(&mut shell, (x, bar_top + 2.0));
            release(&mut shell, (x, bar_top + 2.0));

            assert!(pinned(&shell).is_empty(), "a click was taken for a drop");
            assert_eq!(shell.icons.selected_ids(), [low]);
            assert!(!shell.icons.is_interacting());
        });
    }

    /// A press on an icon that never became a drag is the icon layer's,
    /// wherever it is let go -- even a pixel over the bar.
    #[test]
    fn a_click_on_an_icon_is_not_a_drop() {
        with_scratch_config("carry-icon-click", |_root| {
            let mut shell = shell();
            let (a, a_name) = app(&shell, 0);
            let (id, _) = shell.icons.add_shortcut(
                &a_name,
                icons::IconType::Executable,
                icons::IconAction::OpenPath(PathBuf::from(&a)),
            );
            let from = icon_centre(&shell, id);
            press(&mut shell, from);
            release(&mut shell, from);
            assert!(pinned(&shell).is_empty());
            assert_eq!(shell.icons.selected_ids(), [id]);
        });
    }

    // ---- what is under a point -------------------------------------------------------------

    /// The topmost window on the shown desktop that is on the glass.
    #[test]
    fn window_at_finds_the_topmost_window_drawn_there() {
        let mut shell = shell();
        let mut hidden = window(3, (0, 0, 1920, 1080));
        hidden.minimized = true;
        let mut elsewhere = window(4, (0, 0, 1920, 1080));
        elsewhere.workspace = 1;
        shell.apply_window_list(&WindowList::new(
            0,
            vec![
                window(1, (100, 100, 400, 300)),
                window(2, (300, 200, 400, 300)),
                hidden,
                elsewhere,
            ],
        ));

        assert_eq!(shell.window_at(150.0, 150.0), Some(WindowId(1)));
        // Where they overlap, the one later in the list is on top.
        assert_eq!(shell.window_at(350.0, 250.0), Some(WindowId(2)));
        // Minimised, or on another desktop, is not drawn.
        assert_eq!(shell.window_at(1500.0, 900.0), None);
    }
}

/// Programs pinned to the top of the start menu, and the drops that put them
/// there -- the start menu's part in `design.txt` line 712.
///
/// Only the tests that also pin to the *taskbar* need a scratch configuration
/// directory: the start menu's pins are written by the session
/// (`take_start_pins_dirty`), never by the shell, and the one test that writes
/// them itself asks for one.
#[cfg(test)]
mod start_pin_tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_precision_loss,
        clippy::float_cmp
    )]

    use super::{
        DesktopShell, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
        ShellAction, icons,
    };
    use appearance::config::testing::with_scratch_config;
    use guitk::menu::MenuItem;
    use guitk::render::RenderCommand;
    use std::path::PathBuf;

    const UNKNOWN: &str = "/opt/tools/bin/frobnicate";

    fn shell() -> DesktopShell {
        DesktopShell::new(1920, 1080)
    }

    fn key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn ev(at: (f32, f32), kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            x: at.0,
            y: at.1,
            kind,
        }
    }

    fn press(shell: &mut DesktopShell, at: (f32, f32)) -> ShellAction {
        shell.handle_mouse(&ev(at, MouseEventKind::Press(MouseButton::Left)))
    }

    fn move_to(shell: &mut DesktopShell, at: (f32, f32)) {
        shell.handle_mouse(&ev(at, MouseEventKind::Move));
    }

    fn release(shell: &mut DesktopShell, at: (f32, f32)) -> ShellAction {
        shell.handle_mouse(&ev(at, MouseEventKind::Release(MouseButton::Left)))
    }

    /// Press, move there in four steps, let go.
    fn carry(shell: &mut DesktopShell, from: (f32, f32), to: (f32, f32)) -> ShellAction {
        press(shell, from);
        for step in 1..=4 {
            let t = step as f32 / 4.0;
            move_to(
                shell,
                (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t),
            );
        }
        release(shell, to)
    }

    fn row_centre(shell: &DesktopShell, row: usize) -> (f32, f32) {
        let r = shell.start_menu_row_rect(row);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// A point on the lower or upper half of a row: which half decides
    /// whether a drop goes after the row or before it.
    fn row_half(shell: &DesktopShell, row: usize, lower: bool) -> (f32, f32) {
        let r = shell.start_menu_row_rect(row);
        let y = if lower {
            r.y + r.h * 0.75
        } else {
            r.y + r.h * 0.25
        };
        (r.x + r.w / 2.0, y)
    }

    fn start_button(shell: &DesktopShell) -> (f32, f32) {
        let r = shell.start_button_rect();
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// Every row's program, in menu order.
    fn execs(shell: &DesktopShell) -> Vec<String> {
        shell
            .start_menu_entries()
            .iter()
            .map(|entry| entry.executable_path.clone())
            .collect()
    }

    fn pins(shell: &DesktopShell) -> Vec<String> {
        shell
            .start_pins()
            .iter()
            .map(|entry| entry.executable_path.clone())
            .collect()
    }

    fn labels(items: &[MenuItem]) -> Vec<String> {
        items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect()
    }

    // ---- the list ----------------------------------------------------------------------

    /// A pinned program heads the list, and the launcher's list below is
    /// untouched -- the program is still in it, in its own place.
    #[test]
    fn a_pinned_program_heads_the_list_and_is_still_listed_below() {
        let mut shell = shell();
        let before = execs(&shell);
        shell.pin_to_start(&before[3]);

        let after = execs(&shell);
        assert_eq!(after[0], before[3]);
        assert_eq!(after[1..], before[..]);
        assert!(shell.is_pinned_to_start(&before[3]));
        assert!(shell.take_start_pins_dirty(), "a pin nobody will save");
        assert!(!shell.take_start_pins_dirty(), "the flag outlived the save");
    }

    /// Pinning twice leaves one pin; unpinning takes it off, and unpinning
    /// what is not pinned changes nothing.
    #[test]
    fn a_program_is_pinned_once_and_unpinned_once() {
        let mut shell = shell();
        let before = execs(&shell);
        shell.pin_to_start(&before[1]);
        shell.pin_to_start(&before[1]);
        assert_eq!(pins(&shell), [before[1].clone()]);
        let _ = shell.take_start_pins_dirty();

        shell.unpin_from_start(&before[1]);
        assert!(pins(&shell).is_empty());
        assert_eq!(execs(&shell), before);
        assert!(shell.take_start_pins_dirty());

        shell.unpin_from_start(&before[1]);
        assert!(!shell.take_start_pins_dirty(), "nothing changed");
    }

    /// A program the launcher has never heard of can be pinned by its path,
    /// is named for its file, and starts when its row is clicked.
    #[test]
    fn a_program_the_launcher_does_not_know_is_pinned_by_path() {
        let mut shell = shell();
        shell.pin_to_start(UNKNOWN);
        assert_eq!(shell.start_pins()[0].name, "frobnicate");

        shell.toggle_start_menu();
        let at = row_centre(&shell, 0);
        press(&mut shell, at);
        assert_eq!(
            release(&mut shell, at),
            ShellAction::Launch(crate::hotkeys::Launch::program(UNKNOWN))
        );
    }

    /// Unpinning shortens the list; a menu scrolled to its end must not be
    /// left showing a blank row past it.
    #[test]
    fn unpinning_while_scrolled_to_the_end_leaves_no_blank_row() {
        let mut shell = shell();
        shell.pin_to_start(UNKNOWN);
        shell.toggle_start_menu();
        shell.scroll_start_menu(10_000);
        shell.unpin_from_start(UNKNOWN);

        assert_eq!(shell.start_menu_scroll, shell.start_menu_max_scroll());
        let last_row = shell.start_menu_visible_rows() - 1;
        assert_eq!(
            shell.start_menu_entry_at(last_row),
            Some(shell.start_menu_entries().len() - 1)
        );
    }

    /// A line sets the pins apart from the launcher's list, at the top of the
    /// first row after them -- and there is no line with nothing pinned.
    #[test]
    fn a_line_sets_the_pins_apart_from_the_list() {
        let mut shell = shell();
        shell.toggle_start_menu();
        let line_at = |shell: &DesktopShell| {
            let menu = shell.render_start_menu().expect("the menu is open");
            menu.commands
                .iter()
                .filter_map(|cmd| match cmd {
                    RenderCommand::FillRect { y, height, .. } if *height <= 1.0 => Some(*y),
                    _ => None,
                })
                .collect::<Vec<f32>>()
        };
        assert!(line_at(&shell).is_empty(), "a line with nothing pinned");

        let first = execs(&shell)[0].clone();
        shell.pin_to_start(&first);
        shell.pin_to_start(UNKNOWN);
        assert_eq!(line_at(&shell), [shell.start_menu_row_rect(2).y]);
    }

    /// Written by the session and read back at the next login, names and all.
    #[test]
    fn start_pins_survive_a_restart() {
        with_scratch_config("start-pins-restart", |_root| {
            let mut shell = shell();
            let known = execs(&shell)[2].clone();
            shell.pin_to_start(&known);
            shell.pin_to_start(UNKNOWN);
            shell.save_start_pins().unwrap();

            let mut restarted = DesktopShell::new(1920, 1080);
            assert!(restarted.start_pins().is_empty(), "read before loading");
            restarted.load_start_pins();
            assert_eq!(pins(&restarted), [known, UNKNOWN.to_string()]);
            assert_eq!(restarted.start_pins()[1].name, "frobnicate");
            assert!(
                !restarted.take_start_pins_dirty(),
                "loading is not a change"
            );
        });
    }

    // ---- the menus ---------------------------------------------------------------------

    /// A row's right-click menu pins it to the start menu, and the pinned
    /// row's menu takes it off again -- the label says which it will do.
    #[test]
    fn a_rows_menu_pins_it_to_the_start_menu_and_back() {
        let mut shell = shell();
        shell.toggle_start_menu();
        let exec = execs(&shell)[1].clone();

        let at = row_centre(&shell, 1);
        shell.handle_press(at.0, at.1, MouseButton::Right);
        let drawn = format!("{:?}", shell.render_pin_menu().expect("no menu"));
        assert!(drawn.contains("Pin to Start menu"), "wrong offer: {drawn}");
        // The second row, taken with the keyboard.
        for k in [Key::Down, Key::Down, Key::Enter] {
            drop(shell.handle_hotkey(&key(k)));
        }
        assert_eq!(pins(&shell), [exec]);

        // The pin is row 0 now, and its menu offers the reverse.
        let at = row_centre(&shell, 0);
        shell.handle_press(at.0, at.1, MouseButton::Right);
        let drawn = format!("{:?}", shell.render_pin_menu().expect("no menu"));
        assert!(
            drawn.contains("Unpin from Start menu"),
            "wrong offer: {drawn}"
        );
        for k in [Key::Down, Key::Down, Key::Enter] {
            drop(shell.handle_hotkey(&key(k)));
        }
        assert!(pins(&shell).is_empty());
    }

    /// A program icon's own menu offers the start menu too; a folder's does
    /// not, since a start-menu row starts a program.
    #[test]
    fn a_program_icons_menu_pins_it_to_the_start_menu() {
        let mut shell = shell();
        let entry = shell.start_menu_entries()[0];
        let (exec, name) = (entry.executable_path.clone(), entry.name.clone());
        let (program, _) = shell.icons.add_shortcut(
            &name,
            icons::IconType::Executable,
            icons::IconAction::OpenPath(PathBuf::from(&exec)),
        );
        let (folder, _) = shell.icons.add_shortcut(
            "Projects",
            icons::IconType::Folder,
            icons::IconAction::OpenPath(PathBuf::from("/home/user/projects")),
        );
        assert!(labels(&shell.icon_menu_items(program)).contains(&"Pin to Start menu".into()));
        assert!(
            !labels(&shell.icon_menu_items(folder))
                .iter()
                .any(|l| l.contains("Start menu"))
        );

        shell.menu_icon = Some(program);
        assert_eq!(
            shell.activate_icon_context_item(DesktopShell::MENU_ICON_START_PIN),
            Some(ShellAction::Consumed)
        );
        assert_eq!(pins(&shell), [exec]);
        assert!(labels(&shell.icon_menu_items(program)).contains(&"Unpin from Start menu".into()));
    }

    // ---- dropping on the start menu -------------------------------------------------------

    /// A row from the launcher's list dropped on the pinned rows is pinned
    /// where it was let go -- here after the first pin -- and the menu stays
    /// up, since the user is arranging it.
    #[test]
    fn a_row_dropped_on_the_pinned_rows_is_pinned_where_it_was_let_go() {
        let mut shell = shell();
        let list = execs(&shell);
        shell.pin_to_start(&list[5]);
        shell.pin_to_start(&list[6]);
        let _ = shell.take_start_pins_dirty();
        shell.toggle_start_menu();

        // Row 2 is the first of the launcher's list.
        let (from, to) = (row_centre(&shell, 2), row_half(&shell, 0, true));
        assert_eq!(carry(&mut shell, from, to), ShellAction::Consumed);

        assert_eq!(
            pins(&shell),
            [list[5].clone(), list[0].clone(), list[6].clone()]
        );
        assert!(shell.start_menu_open, "the menu closed under the user");
        assert!(shell.take_start_pins_dirty());
    }

    /// A pinned row dragged along the pins moves.
    #[test]
    fn a_pinned_row_dragged_along_the_pins_moves() {
        let mut shell = shell();
        let list = execs(&shell);
        for exec in &list[5..8] {
            shell.pin_to_start(exec);
        }
        shell.toggle_start_menu();

        let (from, to) = (row_centre(&shell, 0), row_half(&shell, 2, true));
        carry(&mut shell, from, to);
        assert_eq!(
            pins(&shell),
            [list[6].clone(), list[7].clone(), list[5].clone()]
        );

        // Onto its own upper half, nowhere: it is where it already is.
        let _ = shell.take_start_pins_dirty();
        let (from, to) = (row_centre(&shell, 2), row_half(&shell, 2, false));
        carry(&mut shell, from, (to.0 + 40.0, to.1));
        assert_eq!(
            pins(&shell),
            [list[6].clone(), list[7].clone(), list[5].clone()]
        );
        assert!(!shell.take_start_pins_dirty(), "a no-op drop was saved");
    }

    /// Into the gap between two other pins: the one case where taking the
    /// dragged pin out first changes which gap is meant. A move to the end
    /// cannot show it -- clamped to the end, both readings agree.
    #[test]
    fn a_pin_dragged_into_a_middle_gap_lands_there() {
        let mut shell = shell();
        let list = execs(&shell);
        for exec in &list[5..8] {
            shell.pin_to_start(exec);
        }
        shell.toggle_start_menu();

        // Row 0 onto the lower half of row 1: after the second pin, before
        // the third.
        let (from, to) = (row_centre(&shell, 0), row_half(&shell, 1, true));
        carry(&mut shell, from, to);
        assert_eq!(
            pins(&shell),
            [list[6].clone(), list[5].clone(), list[7].clone()]
        );
    }

    /// The launcher's own list is not the user's to arrange: a row let go on
    /// it does nothing.
    #[test]
    fn a_row_let_go_on_the_launchers_list_does_nothing() {
        let mut shell = shell();
        let list = execs(&shell);
        shell.pin_to_start(&list[5]);
        let _ = shell.take_start_pins_dirty();
        shell.toggle_start_menu();

        let (from, to) = (row_centre(&shell, 2), row_centre(&shell, 4));
        assert_eq!(carry(&mut shell, from, to), ShellAction::Consumed);
        assert_eq!(pins(&shell), [list[5].clone()]);
        assert!(!shell.take_start_pins_dirty());
        assert!(shell.start_menu_open);
    }

    /// Let go on the start button, a row is pinned after the pins already
    /// there.
    #[test]
    fn a_row_dropped_on_the_start_button_is_pinned_after_the_rest() {
        let mut shell = shell();
        let list = execs(&shell);
        shell.pin_to_start(&list[5]);
        shell.toggle_start_menu();

        let (from, to) = (row_centre(&shell, 3), start_button(&shell));
        press(&mut shell, from);
        move_to(&mut shell, (from.0, from.1 + 40.0));
        move_to(&mut shell, to);
        let label = format!("{:?}", shell.render_carry().expect("no label"));
        assert!(label.contains("Pin to Start menu"), "wrong hint: {label}");
        release(&mut shell, to);

        // Row 3 was the launcher's third program, below the one pin.
        assert_eq!(pins(&shell), [list[5].clone(), list[2].clone()]);
        assert!(
            shell.pinned_apps().is_empty(),
            "the start button is not the taskbar's row"
        );
    }

    /// A pinned taskbar button carried to the start button is pinned to the
    /// start menu as well, and the taskbar is as it was.
    #[test]
    fn a_taskbar_pin_dropped_on_the_start_button_is_pinned_there_too() {
        with_scratch_config("start-pin-from-bar", |_root| {
            let mut shell = shell();
            let list = execs(&shell);
            shell.pin_app(&list[0], "a");
            shell.pin_app(&list[1], "b");

            let r = shell.taskbar_button_rect(1);
            let from = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            press(&mut shell, from);
            // Up off the bar first, so the row of pins is not crossed.
            move_to(&mut shell, (from.0, 500.0));
            let to = start_button(&shell);
            move_to(&mut shell, to);
            assert_eq!(release(&mut shell, to), ShellAction::Consumed);

            assert_eq!(pins(&shell), [list[1].clone()]);
            let bar: Vec<String> = shell
                .pinned_apps()
                .iter()
                .map(|a| a.exec_path.clone())
                .collect();
            assert_eq!(bar, [list[0].clone(), list[1].clone()]);
        });
    }

    /// A program icon let go on the start button is pinned to the start menu,
    /// and stays on the desktop.
    #[test]
    fn a_program_icon_dropped_on_the_start_button_is_pinned_there() {
        let mut shell = shell();
        let entry = shell.start_menu_entries()[0];
        let (exec, name) = (entry.executable_path.clone(), entry.name.clone());
        let (id, _) = shell.icons.add_shortcut(
            &name,
            icons::IconType::Executable,
            icons::IconAction::OpenPath(PathBuf::from(&exec)),
        );
        let was = {
            let icon = shell.icons.get_icon(id).unwrap();
            (icon.x, icon.y)
        };
        let grid = shell.icons.grid();
        let from = (
            was.0 as f32 + grid.cell_width() as f32 / 2.0,
            was.1 as f32 + grid.cell_height() as f32 / 2.0,
        );
        let to = start_button(&shell);
        press(&mut shell, from);
        move_to(&mut shell, (from.0 + 40.0, from.1));
        move_to(&mut shell, to);
        let label = format!("{:?}", shell.render_carry().expect("no label"));
        assert!(label.contains("Pin to Start menu"), "wrong hint: {label}");
        assert_eq!(release(&mut shell, to), ShellAction::Consumed);

        assert_eq!(pins(&shell), [exec]);
        assert!(shell.pinned_apps().is_empty());
        let icon = shell.icons.get_icon(id).unwrap();
        assert_eq!((icon.x, icon.y), was, "the icon moved");
    }
}

/// The icons keep clear of the taskbar as it is drawn, whatever the scale and
/// whatever the display's size.
#[cfg(test)]
mod icon_area_tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap
    )]

    use super::{AppearanceSettings, DesktopShell, icons};

    /// Where an icon's far corner is, in screen pixels.
    fn far_corner(shell: &DesktopShell, id: icons::IconId) -> (f32, f32) {
        let icon = shell.icons.get_icon(id).unwrap();
        let grid = shell.icons.grid();
        (
            (icon.x + grid.cell_width() as i32) as f32,
            (icon.y + grid.cell_height() as i32) as f32,
        )
    }

    /// The layer was told the taskbar was 40 pixels and was never told
    /// otherwise: at 150% the bar is drawn 60 tall, and an icon placed
    /// against the bottom edge sat under its top third.
    #[test]
    fn an_icon_stays_clear_of_the_taskbar_at_every_scale() {
        for percent in [100u16, 125, 150, 200] {
            let mut shell = DesktopShell::new(1920, 1080);
            let mut appearance = AppearanceSettings::default();
            appearance.scaling_percent = percent;
            shell.set_appearance(appearance);
            shell.icons.set_arrangement(icons::ArrangementMode::Free);
            let id = shell.icons.add_icon(
                "low",
                icons::IconType::File,
                icons::IconAction::Custom("low".into()),
                600,
                5000,
            );
            let bar = shell.taskbar_rect();
            let (_, bottom) = far_corner(&shell, id);
            assert!(
                bottom <= bar.y,
                "at {percent}% the icon reaches {bottom}, under a bar from {}",
                bar.y
            );
            // Flush against it, not merely somewhere above.
            assert!(
                bar.y - bottom < 1.0,
                "at {percent}% the icon stops short at {bottom}"
            );
        }
    }

    /// A display that shrinks takes the icons with it: every one ends up on
    /// the desktop that is now there, clear of the bar.
    #[test]
    fn a_smaller_display_brings_the_icons_onto_it() {
        let mut shell = DesktopShell::new(1920, 1080);
        shell.icons.set_arrangement(icons::ArrangementMode::Free);
        let id = shell.icons.add_icon(
            "far",
            icons::IconType::File,
            icons::IconAction::Custom("far".into()),
            1800,
            950,
        );
        shell.set_screen_size(1280, 720);

        let (right, bottom) = far_corner(&shell, id);
        assert!(right <= 1280.0, "off the right edge at {right}");
        assert!(
            bottom <= shell.taskbar_rect().y,
            "under the bar at {bottom}"
        );
    }
}

/// The start menu's search field -- `design.txt` line 721, "input field for
/// finding and running apps".
#[cfg(test)]
mod start_search_tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::{DesktopShell, Key, KeyEvent, Modifiers};

    fn shell() -> DesktopShell {
        let mut shell = DesktopShell::new(1920, 1080);
        shell.toggle_start_menu();
        shell
    }

    fn press(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    /// Type `text` into whatever has the keyboard, a character at a time.
    fn type_text(shell: &mut DesktopShell, text: &str) {
        for ch in text.chars() {
            let key = KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: Modifiers::NONE,
                text: ch.to_string(),
            };
            drop(shell.handle_hotkey(&key));
        }
    }

    fn names(shell: &DesktopShell) -> Vec<String> {
        shell
            .start_menu_entries()
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }

    /// A program's name, as the launcher knows it, to search for.
    fn some_program(shell: &DesktopShell, row: usize) -> (String, String) {
        let entry = shell.start_menu_entries()[row];
        (entry.name.clone(), entry.executable_path.clone())
    }

    /// Typing with the menu open searches it: the list becomes what the
    /// search finds, the best match first.
    #[test]
    fn typing_in_the_start_menu_searches_it() {
        let mut shell = shell();
        let everything = names(&shell).len();
        let (name, _) = some_program(&shell, 3);

        type_text(&mut shell, &name);
        let found = names(&shell);
        assert!(found.len() < everything, "the search filtered nothing");
        assert_eq!(found[0], name, "the exact name is not the best match");
        assert_eq!(shell.start_query.text(), name);
    }

    /// Enter starts the best match, and closes the menu.
    #[test]
    fn enter_starts_the_best_match() {
        let mut shell = shell();
        let (name, exec) = some_program(&shell, 3);
        type_text(&mut shell, &name);

        let outcome = shell.handle_hotkey(&press(Key::Enter));
        assert_eq!(outcome.launches.len(), 1);
        assert_eq!(outcome.launches[0].program.to_string_lossy(), exec);
        assert!(!shell.start_menu_open);
    }

    /// The arrows walk the rows, and Enter starts the one the keyboard is on.
    #[test]
    fn the_arrows_choose_a_row_and_enter_starts_it() {
        let mut shell = shell();
        let (_, second) = some_program(&shell, 1);
        drop(shell.handle_hotkey(&press(Key::Down)));
        drop(shell.handle_hotkey(&press(Key::Down)));
        assert_eq!(shell.start_selected, Some(1));
        drop(shell.handle_hotkey(&press(Key::Up)));
        drop(shell.handle_hotkey(&press(Key::Down)));

        let outcome = shell.handle_hotkey(&press(Key::Enter));
        assert_eq!(outcome.launches[0].program.to_string_lossy(), second);
    }

    /// Walking past the last visible row scrolls the list with it, so the
    /// keyboard's row is never off the bottom of the menu.
    #[test]
    fn the_keyboards_row_stays_on_screen() {
        let mut shell = shell();
        let rows = shell.start_menu_visible_rows();
        assert!(
            shell.start_menu_entries().len() > rows,
            "the fixture needs a scroll"
        );
        for _ in 0..=rows {
            drop(shell.handle_hotkey(&press(Key::Down)));
        }
        let selected = shell.start_selected.unwrap();
        assert!(
            selected >= shell.start_menu_scroll && selected < shell.start_menu_scroll + rows,
            "row {selected} is off screen at scroll {}",
            shell.start_menu_scroll
        );
    }

    /// Escape empties the search first, and closes the menu second.
    #[test]
    fn escape_empties_the_search_then_closes_the_menu() {
        let mut shell = shell();
        let everything = names(&shell);
        type_text(&mut shell, "zz");
        drop(shell.handle_hotkey(&press(Key::Escape)));
        assert!(shell.start_menu_open, "the first Escape closed the menu");
        assert_eq!(names(&shell), everything);
        drop(shell.handle_hotkey(&press(Key::Escape)));
        assert!(!shell.start_menu_open);
    }

    /// Nothing listed matches: Enter runs what was typed, as the Run box
    /// would -- the field is for finding *and running*.
    #[test]
    fn with_nothing_found_enter_runs_what_was_typed() {
        let mut shell = shell();
        type_text(&mut shell, "frobnicate --fast \"two words\"");
        assert!(names(&shell).is_empty(), "the fixture found something");

        let outcome = shell.handle_hotkey(&press(Key::Enter));
        assert_eq!(outcome.launches.len(), 1);
        let launch = &outcome.launches[0];
        assert_eq!(launch.program.to_string_lossy(), "frobnicate");
        let args: Vec<String> = launch
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["--fast", "two words"]);
        assert!(!shell.start_menu_open);
    }

    /// A quote left open is not guessed at: nothing runs, and the menu stays
    /// up with the line as it was.
    #[test]
    fn an_unclosed_quote_runs_nothing() {
        let mut shell = shell();
        type_text(&mut shell, "frobnicate \"half");
        let outcome = shell.handle_hotkey(&press(Key::Enter));
        assert!(outcome.launches.is_empty());
        assert!(shell.start_menu_open);
    }

    /// Reopened, the menu's search is empty again.
    #[test]
    fn the_menu_reopens_with_an_empty_search() {
        let mut shell = shell();
        type_text(&mut shell, "zz");
        shell.toggle_start_menu();
        shell.toggle_start_menu();
        assert!(shell.start_query.text().is_empty());
        assert_eq!(shell.start_selected, None);
    }

    /// A pinned program is found once, not once as a pin and again in the
    /// list below -- and while searching, the rows are results, not pins: no
    /// line is drawn and nothing can be dropped among them.
    #[test]
    fn a_search_lists_a_pinned_program_once() {
        let mut shell = shell();
        let (name, exec) = some_program(&shell, 3);
        shell.pin_to_start(&exec);
        type_text(&mut shell, &name);

        let hits = shell
            .start_menu_entries()
            .iter()
            .filter(|entry| entry.executable_path == exec)
            .count();
        assert_eq!(hits, 1);
        assert_eq!(shell.start_pins_listed(), 0);
    }

    /// A chord with Super is still the desktop's with the menu up: the Super
    /// key that opened it closes it, and types nothing into the search.
    #[test]
    fn the_super_key_closes_the_menu_and_types_nothing() {
        let mut shell = shell();
        let key = KeyEvent {
            key: Key::LeftSuper,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        drop(shell.handle_hotkey(&key));
        assert!(!shell.start_menu_open);
        assert!(shell.start_query.text().is_empty());
    }

    /// Super+E with the menu up -- which is how it arrives, since the Super key
    /// opens the menu as it goes down -- puts the menu away and opens the file
    /// manager, and types no "e".
    #[test]
    fn a_super_chord_closes_the_menu_and_does_what_it_is_bound_to() {
        let mut shell = shell();
        let key = KeyEvent {
            key: Key::E,
            pressed: true,
            modifiers: Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
            text: "e".to_string(),
        };
        let outcome = shell.handle_hotkey(&key);
        assert!(
            !shell.start_menu_open,
            "the menu stayed over the file manager"
        );
        assert!(
            shell.start_query.text().is_empty(),
            "the chord typed into the search"
        );
        assert_eq!(
            outcome.launches[0].program,
            std::path::PathBuf::from(crate::launcher::FILE_MANAGER)
        );
    }

    /// The field is drawn where the title was, with the hint while empty.
    #[test]
    fn the_empty_field_says_what_typing_does() {
        let shell = shell();
        let drawn = format!("{:?}", shell.render_start_menu().expect("the menu is open"));
        assert!(drawn.contains("Type to search"), "no hint: {drawn}");
        assert!(
            !drawn.contains("\"Applications\""),
            "the old title is still drawn"
        );
    }
}

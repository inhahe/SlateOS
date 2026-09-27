//! Slate OS Terminal Emulator
//!
//! Graphical VT100/xterm-compatible terminal emulator featuring:
//! - Full CSI escape sequence parsing (cursor movement, erase, scroll, modes)
//! - SGR (Select Graphic Rendition) with 8-color, 256-color, and 24-bit truecolor
//! - UTF-8 input handling with multi-byte accumulator
//! - Scrollback buffer (configurable, default 10000 lines)
//! - Alt screen buffer (smcup/rmcup)
//! - Scroll region support (DECSTBM)
//! - Cursor styles (block, underline, bar) with blink support
//! - Selection (start, extend, clipboard copy)
//! - Tab stops (default every 8, configurable via HTS/TBC)
//! - OSC sequences (set window title)
//! - Visual bell
//! - Dark color scheme
//!
//! All of which used to have nothing to run in. `main` fed a fixed demo string
//! into the parser, rendered one frame and returned, so the grid was fixed at
//! eighty by twenty-four whatever the window was and the bell counted down in
//! frames that were never drawn. The emulator now owns a real [`oswindow`]
//! window: [`Layout`] is solved from the size the compositor hands
//! [`App::render`] every frame, and the bell and cursor blink age in
//! milliseconds off [`Event::Tick`].
//!
//! The child is the user's shell on a **kernel** pseudo-terminal, started
//! through [`child::spawn_shell`]: keystrokes go into the master, the shell's
//! output comes back out of it, the window's size reaches the shell as
//! `TIOCSWINSZ`, and `^C` is the kernel's line discipline's business, not
//! ours. See [`child`] for what this replaced -- a model of a terminal inside
//! this process, with the shell on pipes beside it.
//!
//! Renders via the guitk RenderTree, producing Text and FillRect commands
//! for each visible cell in the terminal grid.
//!
//! A library as well as a program. `src/main.rs` is the terminal's window;
//! `apps/tmux` puts one [`TerminalState`] in each of its panes, so a split
//! window runs the same emulator -- scroll regions, the alternate screen,
//! cursor-key modes, wide characters, replies to the questions a program asks
//! its terminal -- as a terminal on its own. tmux kept a second emulator of
//! its own, a fraction of this one, for as long as its panes had nothing to
//! run; a shell in them would have drawn every full-screen program wrong.

// The child link lives in `apps/termchild`, shared with `apps/tmux`.
pub use termchild as child;

use child::{Exit, Link, SpawnError};
use libcall::pty::WinSize;

use appearance::Palette;
use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontFamily, FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::text;
use guitk::wheel;
use oswindow::app::{App, Response};

use std::collections::VecDeque;
use std::task::Waker;

// ============================================================================
// Window geometry
// ============================================================================

/// A window size for the tests to aim into: roughly an eighty-by-twenty-four
/// grid plus the scrollback bar.
///
/// Roughly, because the real cell is the fixed-pitch face's and is measured
/// at run time -- a `const` cannot ask the font. The window the terminal
/// actually opens at is worked out from the measured cell in
/// [`App::initial_size`]; this is only the probe's.
const WINDOW_WIDTH: f32 = 80.0 * 8.4 + BAR_W;
/// Twenty-four rows of roughly the default cell height.
const WINDOW_HEIGHT: f32 = 24.0 * 18.0;

/// How wide the scrollback bar down the right-hand edge is.
///
/// Reserved whether or not there is anything to show in it. A bar that appears
/// only when there is scrollback would take its width out of the grid the
/// moment the first line scrolled off, and **a terminal that reflows because
/// you scrolled it** is a terminal that redraws the program you are running
/// under it: `vim` would repaint at a different width every time output crossed
/// the top of the window.
const BAR_W: f32 = 10.0;

/// How long the visual bell stays up, in milliseconds.
const BELL_MS: u64 = 100;

/// How long each half of the cursor's blink lasts, in milliseconds.
const BLINK_MS: u64 = 500;

/// How often a terminal looks for its child's output on a clock while the two
/// are talking, in milliseconds: one frame, so an echo appears with the
/// keystroke.
///
/// Only for a link that cannot wake the terminal, and for draining a flood:
/// a link given the window's waker (`App::attach_waker`) wakes the terminal
/// for every chunk its child writes, and between those the terminal asks for
/// no clock at all. A wake is spent draining at most [`MAX_READ_PER_DRAIN`],
/// so output left over after it is read on this clock until it is gone --
/// the reader has already sent it, and will not wake anyone for it again.
const ACTIVE_POLL_MS: u64 = 16;

/// How often a terminal with a quiet child looks for output, in milliseconds.
///
/// Slower, because a shell at a prompt can be quiet for hours and a program
/// that ticks sixty times a second for nothing holds the whole desktop awake.
/// Not much slower, because the first keystroke after a pause waits out
/// whatever interval is already armed -- the window library does not shorten
/// a pending deadline -- so this is also the worst echo delay the user sees.
const IDLE_POLL_MS: u64 = 50;

/// How long after the last byte either way a child counts as quiet, in
/// milliseconds.
const ACTIVE_WINDOW_MS: u64 = 2_000;

/// The most output one drain parses before the frame is drawn.
///
/// A child that writes faster than the parser runs -- `cat` of a large file --
/// must not hold the window for the whole of its output. The rest waits for
/// the next tick, which comes at [`ACTIVE_POLL_MS`] while output is flowing.
const MAX_READ_PER_DRAIN: usize = 64 * 1024;

/// The size of the grid's text, in pixels.
const FONT_SIZE: f32 = 14.0;

/// What a click can land on.
///
/// The grid is one target rather than one per cell: a cell is found by
/// arithmetic on the click's position, and eighty by twenty-four hit boxes
/// would be nineteen hundred rectangles rebuilt every frame to answer a
/// question two divisions already answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The character grid. A press starts a selection; a drag extends it.
    Grid,
    /// The scrollback bar's track, above or below the thumb: a page either way.
    ScrollTrack,
    /// The part of the bar standing for what is on screen.
    ScrollThumb,
}

/// Where everything is, for one window size.
///
/// Solved fresh from the size handed to `render`, because the grid is a
/// *quotient* of the window and nothing about it can be a constant: the old
/// program fixed `cols: 80, rows: 24` in its config, never called `resize`, and
/// painted exactly `80 * 8.4` by `24 * 18` pixels into whatever window it was
/// given.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// The whole window.
    pub window: Rect,
    /// The cells: `cols` by `rows` of `cell_w` by `cell_h`, at the origin.
    pub grid: Rect,
    /// The scrollback bar, down the right-hand edge.
    pub bar: Rect,
    /// How many whole columns fit beside the bar.
    pub cols: usize,
    /// How many whole rows fit.
    pub rows: usize,
    /// One cell's width.
    pub cell_w: f32,
    /// One cell's height.
    pub cell_h: f32,
}

impl Layout {
    /// Fit a grid of `cell_w` by `cell_h` cells into a `width` by `height`
    /// window.
    pub fn solve(width: f32, height: f32, cell_w: f32, cell_h: f32) -> Self {
        let window = Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0));
        // The bar is taken out of the width first and is never wider than a
        // quarter of the window, so a window narrower than the bar itself is
        // still mostly grid rather than entirely furniture.
        let bar_w = BAR_W.min(window.w / 4.0).max(0.0);
        let grid_w = (window.w - bar_w).max(0.0);
        let cols = cells_that_fit(grid_w, cell_w);
        let rows = cells_that_fit(window.h, cell_h);
        let grid = Rect::new(0.0, 0.0, usize_f32(cols) * cell_w, usize_f32(rows) * cell_h);
        let bar = Rect::new(window.w - bar_w, 0.0, bar_w, window.h);
        Self {
            window,
            grid,
            bar,
            cols,
            rows,
            cell_w,
            cell_h,
        }
    }
}

/// How many whole cells of `cell` fit in `span`.
///
/// Counted rather than divided and cast: `(span / cell) as usize` is a
/// saturating cast on a value that can be infinite or NaN, and it answers
/// `0` for NaN as confidently as it answers `0` for an empty window.
fn cells_that_fit(span: f32, cell: f32) -> usize {
    // `is_finite` on both, rather than a negated `>`: a NaN cell size makes
    // every comparison in the loop false, so the loop would end at once and
    // report a window with no columns in it as confidently as an empty one.
    if !cell.is_finite() || cell <= 0.0 || !span.is_finite() {
        return 0;
    }
    let mut n = 0_usize;
    let mut used = cell;
    while used <= span + 0.01 {
        n = n.saturating_add(1);
        used += cell;
    }
    n
}

/// A count as a float, without a lint suppression at every call site.
///
/// Exact to 2^24 cells, which is four thousand times the columns any window
/// has room for.
#[allow(clippy::cast_precision_loss)]
fn usize_f32(n: usize) -> f32 {
    n as f32
}

/// A window dimension as a float.
///
/// Exact to 2^24 pixels, which is a window sixteen thousand screens wide.
#[allow(clippy::cast_precision_loss)]
fn u32_f32(n: u32) -> f32 {
    n as f32
}

/// A row or column count as the `u16` the PTY's window size is made of.
///
/// Saturating, not truncating: a grid of 65 540 columns would otherwise be
/// reported to the child as four.
fn u16_of(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// A span in pixels as the `u16` a terminal's pixel size is made of.
///
/// Rounded down and saturating. A span that is not a positive finite number
/// becomes zero, which is what `TIOCSWINSZ` means by "unknown" -- the honest
/// answer for a window that has no size yet.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn pixels_u16(span: f32) -> u16 {
    if span.is_finite() && span > 0.0 {
        // In range by the `min`, non-negative by the guard, so the cast only
        // drops the fraction.
        span.min(f32::from(u16::MAX)) as u16
    } else {
        0
    }
}

/// An SGR colour component narrowed to the byte a palette entry is made of.
///
/// Saturating, not `as`: the value comes off the byte stream, and an `as`
/// cast wraps 256 round to 0 -- so `38;5;256` would have painted black while
/// meaning white. Clamping paints the brightest entry instead, which is at
/// least adjacent to what was asked for.
fn palette_index(n: u16) -> u8 {
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// The scheme's entry for one of the sixteen ANSI colours.
///
/// `get` rather than `scheme.ansi[idx]`: the two callers reach here only from
/// `0..=15` match arms, so the index is always in range -- but that is a fact
/// about the match above and not about the parameter, and the foreground is a
/// visible answer where a panic in a draw loop is not.
fn ansi_entry(scheme: &ColorScheme, idx: u8) -> Color {
    scheme
        .ansi
        .get(usize::from(idx))
        .copied()
        .unwrap_or(scheme.foreground)
}

/// One axis of the xterm 6x6x6 colour cube, as a byte.
///
/// The levels are 0, 95, 135, 175, 215, 255 -- `n * 40 + 55` for a non-zero
/// `n`. Saturating because that expression only stays inside a `u8` while
/// `n <= 5`, which is a property of the caller's division rather than of the
/// type it is done in.
fn cube_level(component: u8) -> u8 {
    if component == 0 {
        0
    } else {
        component.saturating_mul(40).saturating_add(55)
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Terminal configuration parameters.
#[derive(Clone, Debug)]
pub struct TerminalConfig {
    /// Number of columns in the terminal grid.
    pub cols: usize,
    /// Number of visible rows in the terminal grid.
    pub rows: usize,
    /// Font size in points.
    pub font_size: f32,
    /// Character cell width in pixels.
    pub cell_width: f32,
    /// Character cell height in pixels.
    pub cell_height: f32,
    /// Maximum number of scrollback lines.
    pub scrollback_limit: usize,
    /// Default cursor style.
    pub cursor_style: CursorStyle,
    /// Whether the cursor blinks.
    pub cursor_blink: bool,
    /// Color scheme.
    pub colors: ColorScheme,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            font_size: FONT_SIZE,
            // The fixed-pitch face's own cell, measured. 8.4 by 18 was a
            // guess, and the glyphs were drawn in the proportional UI face,
            // where a `W` is far wider than an `i`: wide letters overhung
            // their neighbours' backgrounds and the block cursor sat beside
            // the character it marked. `apps/tmux` found this in its own
            // grid and fixed it there; its panes are drawn by this code now,
            // so the fix lives here.
            cell_width: text::cell_advance(FONT_SIZE, FontWeightHint::Regular),
            cell_height: text::line_height_in(FONT_SIZE, FontWeightHint::Regular, FontFamily::Mono),
            scrollback_limit: 10_000,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            colors: ColorScheme::default(),
        }
    }
}

// ============================================================================
// Color Scheme
// ============================================================================

/// Terminal color scheme (16 ANSI colors plus foreground/background defaults).
#[derive(Clone, Debug)]
pub struct ColorScheme {
    /// Default foreground color.
    pub foreground: Color,
    /// Default background color.
    pub background: Color,
    /// Cursor color.
    pub cursor: Color,
    /// Selection background color.
    pub selection_bg: Color,
    /// The 16 ANSI colors (0-7 normal, 8-15 bright).
    pub ansi: [Color; 16],
}

impl ColorScheme {
    /// The user's theme for the chrome, and the protocol's colours for the text.
    ///
    /// The split is the whole point. `foreground`, `background`, `cursor` and
    /// `selection_bg` are this window's furniture, and a terminal that stays
    /// dark when the desktop goes light is the defect 822 is about. The
    /// sixteen ANSI entries are *not* furniture: colour 1 is red because the
    /// escape sequence says so, and a program that prints red expects red on
    /// every terminal ever made. Retinting those would not theme the terminal,
    /// it would corrupt what programs print -- the same reason
    /// `guitk::textview::ansi_color` takes no palette.
    #[must_use]
    pub fn from_palette(palette: &Palette) -> Self {
        Self {
            foreground: palette.text,
            background: palette.base,
            cursor: palette.text,
            // The accent, as every other selection in the desktop uses (839).
            selection_bg: palette.accent,
            ..Self::default()
        }
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        // Dark theme inspired by common terminal defaults
        Self {
            foreground: Color::rgb(204, 204, 204),
            background: Color::rgb(30, 30, 30),
            cursor: Color::rgb(204, 204, 204),
            selection_bg: Color::rgb(68, 68, 120),
            ansi: [
                // Normal colors (0-7)
                Color::rgb(0, 0, 0),       // 0: Black
                Color::rgb(204, 0, 0),     // 1: Red
                Color::rgb(0, 204, 0),     // 2: Green
                Color::rgb(204, 204, 0),   // 3: Yellow
                Color::rgb(0, 0, 204),     // 4: Blue
                Color::rgb(204, 0, 204),   // 5: Magenta
                Color::rgb(0, 204, 204),   // 6: Cyan
                Color::rgb(204, 204, 204), // 7: White
                // Bright colors (8-15)
                Color::rgb(85, 85, 85),    // 8: Bright Black
                Color::rgb(255, 85, 85),   // 9: Bright Red
                Color::rgb(85, 255, 85),   // 10: Bright Green
                Color::rgb(255, 255, 85),  // 11: Bright Yellow
                Color::rgb(85, 85, 255),   // 12: Bright Blue
                Color::rgb(255, 85, 255),  // 13: Bright Magenta
                Color::rgb(85, 255, 255),  // 14: Bright Cyan
                Color::rgb(255, 255, 255), // 15: Bright White
            ],
        }
    }
}

// ============================================================================
// Cell and attributes
// ============================================================================

/// Visual attributes for a terminal cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellAttrs {
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub inverse: bool,
    pub hidden: bool,
    pub strikethrough: bool,
}

impl Default for CellAttrs {
    fn default() -> Self {
        Self {
            fg: TermColor::Default,
            bg: TermColor::Default,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            blink: false,
            inverse: false,
            hidden: false,
            strikethrough: false,
        }
    }
}

/// Terminal color representation (can be indexed or RGB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermColor {
    /// Use the default foreground/background.
    Default,
    /// One of the 256 indexed colors (0-15 ANSI, 16-231 color cube, 232-255 grayscale).
    Indexed(u8),
    /// 24-bit truecolor.
    Rgb(u8, u8, u8),
}

/// A single character cell in the terminal grid.
#[derive(Clone, Debug)]
pub struct Cell {
    /// The character displayed in this cell (space if empty).
    pub ch: char,
    /// Visual attributes for this cell.
    pub attrs: CellAttrs,
    /// The second half of a double-width character that starts in the cell to
    /// the left.
    ///
    /// The grid stays a rectangle of cells -- a wide character occupies two of
    /// them rather than one cell of a different size -- because every other
    /// operation here (erase, insert, scroll, select) is written in columns
    /// and would need a second model otherwise.
    ///
    /// `ch` is a space in a continuation, so the renderer already skips it and
    /// needs no change. The flag exists for the *text* path: without it,
    /// copying a line of Chinese would yield a space after every character.
    pub continuation: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            continuation: false,
            attrs: CellAttrs::default(),
        }
    }
}

/// Cursor rendering style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

// ============================================================================
// Terminal line (row of cells)
// ============================================================================

/// A single line in the terminal buffer.
#[derive(Clone, Debug)]
pub struct TermLine {
    pub cells: Vec<Cell>,
}

impl TermLine {
    /// Create a new blank line with the given column count.
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::default(); cols],
        }
    }

    /// Resize this line to the given column count, padding with blank cells.
    pub fn resize(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::default());
    }
}

// ============================================================================
// Parser state machine
// ============================================================================

/// VT100/xterm escape sequence parser state.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ParserState {
    /// Normal character processing.
    Ground,
    /// Received ESC, waiting for next byte.
    Escape,
    /// Inside a CSI sequence (ESC [ ...).
    Csi,
    /// Inside an OSC sequence (ESC ] ...).
    Osc,
    /// OSC string terminated by ST (ESC \).
    OscEscape,
    /// Inside a DCS sequence (ESC P ...).
    Dcs,
    /// Accumulating a UTF-8 multi-byte character.
    Utf8 { remaining: u8, codepoint: u32 },
}

// ============================================================================
// Selection
// ============================================================================

/// Text selection (start and end positions in the grid).
#[derive(Clone, Debug)]
pub struct Selection {
    /// Start position (row in scrollback-inclusive coordinates, column).
    pub start_row: usize,
    pub start_col: usize,
    /// End position (row in scrollback-inclusive coordinates, column).
    pub end_row: usize,
    pub end_col: usize,
    /// Whether the selection is currently being extended (mouse drag).
    pub active: bool,
}

// ============================================================================
// Main terminal state
// ============================================================================

/// The core terminal emulator state.
///
/// Manages the character cell grid, scrollback buffer, escape sequence parsing,
/// cursor position, and all terminal modes. Consumes byte streams from a child
/// process and produces render output via the guitk RenderTree.
pub struct TerminalState {
    /// Configuration.
    pub config: TerminalConfig,
    /// Visible screen buffer (rows x cols grid).
    screen: Vec<TermLine>,
    /// Scrollback buffer (oldest lines at front).
    scrollback: VecDeque<TermLine>,
    /// How many lines at the back of the scrollback a shrinking grid put
    /// there, which a growing one gives back.
    ///
    /// Only those: growing used to pull back *any* history, so a window
    /// made shorter and then taller again -- a multiplexer pane split and
    /// then closed -- came back with a prompt that had been at the top
    /// pushed down under lines the user had cleared away. tmux keeps the
    /// same count, as `hscrolled`, for the same reason.
    resized_into_scrollback: usize,
    /// Alternate screen buffer (for smcup/rmcup).
    alt_screen: Vec<TermLine>,
    /// Whether we are currently on the alternate screen.
    alt_screen_active: bool,
    /// Saved cursor position for the main screen.
    saved_cursor_main: (usize, usize),
    /// Saved cursor position for the alt screen.
    saved_cursor_alt: (usize, usize),

    /// Cursor row (0-based, relative to screen top).
    cursor_row: usize,
    /// Cursor column (0-based).
    cursor_col: usize,
    /// Current cell attributes (applied to new characters).
    current_attrs: CellAttrs,
    /// Cursor style.
    cursor_style: CursorStyle,
    /// Whether cursor is visible.
    cursor_visible: bool,

    /// Scroll region top (inclusive, 0-based).
    scroll_top: usize,
    /// Scroll region bottom (inclusive, 0-based).
    scroll_bottom: usize,

    /// Tab stops (column indices where tabs stop).
    tab_stops: Vec<bool>,

    /// Parser state machine.
    parser_state: ParserState,
    /// CSI parameter accumulator.
    csi_params: Vec<u16>,
    /// Current CSI parameter being built.
    csi_current_param: u16,
    /// CSI intermediate bytes.
    csi_intermediates: Vec<u8>,
    /// Whether we have started parsing a param digit in the current param slot.
    csi_param_started: bool,
    /// Private mode prefix (e.g., '?' in CSI ? 25 h).
    csi_private_marker: Option<u8>,
    /// OSC string accumulator.
    osc_string: String,

    /// Window title (set via OSC 0 or OSC 2).
    pub title: String,

    /// How much longer the visual bell's flash stays up, in milliseconds.
    ///
    /// Milliseconds rather than the frames this used to count. The old field
    /// was decremented **inside `render`**, under a comment reading "~100ms at
    /// 60fps": the flash was as long as it took to draw eight frames, so it
    /// lasted a tenth of a second on a machine drawing sixty of them a second,
    /// twice that at thirty, and *for ever* on a terminal that had nothing else
    /// to redraw for -- which is the usual state of a terminal waiting at a
    /// prompt. Time is measured by the clock now.
    bell_flash_ms: u64,

    /// How far through the current half of the cursor's blink we are.
    blink_ms: u64,
    /// Whether text marked blinking (SGR 5) is in the lit half of its blink.
    ///
    /// Its own clock, not the cursor's: the cursor's stops when the window
    /// loses the keyboard or the user turns it off, and a program that asked
    /// for blinking text asked for it regardless of either.
    text_blink_on: bool,
    /// How far through the current half of the text's blink we are.
    text_blink_ms: u64,
    /// Whether the cursor is in the lit half of its blink.
    ///
    /// `config.cursor_blink` defaulted to `true` and nothing in the program
    /// read it: the cursor was drawn on every frame regardless, so the setting
    /// was a field a user could change to no effect.
    blink_on: bool,
    /// Whether this terminal has the keyboard.
    ///
    /// A terminal without it -- a window behind another, or a multiplexer
    /// pane beside the one being typed into -- still shows where its cursor
    /// is, as an outline that does not blink, so that of the several cursors a
    /// split window draws exactly one looks like the place keys will go.
    focused: bool,
    /// Whether the last tick changed anything the user can see.
    ///
    /// A field rather than a return value because a tick reaches the window
    /// through `handle_event`, which answers with the bytes a *key* produced
    /// and has nowhere to put a second answer. `on_event` reads it to decide
    /// whether the frame is worth drawing.
    tick_changed: bool,

    /// The program on the other end: the user's shell on a kernel
    /// pseudo-terminal, once [`Self::attach`] has been given one.
    ///
    /// `None` when no shell could be started -- on a host with no
    /// pseudo-terminals, or when the shell named by `$SHELL` does not exist --
    /// which leaves a terminal that still draws and still scrolls, and says on
    /// its screen why nothing answers.
    child: Option<Box<dyn Link>>,

    /// How the child ended, once it has, and been said so.
    child_exit: Option<Exit>,

    /// Milliseconds since a byte last went to or came from the child, so the
    /// clock can slow down while the two are not talking. See
    /// [`ACTIVE_POLL_MS`].
    quiet_ms: u64,

    /// Set when the child finished cleanly: the window closes, as it does
    /// for a user who types `exit`. A shell that failed or was killed leaves
    /// the window open with the reason on it instead.
    close_requested: bool,

    /// The window loop's waker, once it has handed one over.
    waker: Option<Waker>,
    /// Whether the attached link wakes the terminal itself, so nothing needs
    /// to ask it on a clock.
    link_wakes: bool,
    /// Whether the last read took all it was allowed and so may have left
    /// output behind, which no wake will announce again.
    backlog: bool,

    /// The window size the last frame was drawn at.
    ///
    /// Kept so an event that has to know the geometry -- a click landing in a
    /// cell, a wheel notch over the bar -- can solve the same layout the
    /// drawing solved, rather than a second one built from the config's idea
    /// of how big the grid is.
    size: (f32, f32),

    /// Current text selection.
    pub selection: Option<Selection>,

    /// Scroll offset for viewing scrollback (0 = bottom, >0 = scrolled up).
    pub scroll_offset: usize,

    /// Carries the fraction of a line a precision device sends.
    ///
    /// `scroll_offset` counts whole lines and cannot hold a fraction. Without
    /// this the wheel handler could only read the *sign* of `dy` and moved
    /// three lines for any non-zero value, so a trackpad's stream of
    /// 0.2-notch events flew fifteen times too fast through the scrollback.
    wheel: wheel::Accumulator,

    /// Output buffer — bytes to send back to the child process (e.g., cursor
    /// position reports, keyboard input translated to escape sequences).
    pub output_buffer: Vec<u8>,

    /// Whether origin mode is set (cursor addressing relative to scroll region).
    origin_mode: bool,
    /// Whether auto-wrap mode is enabled.
    auto_wrap: bool,
    /// Whether insert mode is enabled.
    insert_mode: bool,
    /// Tracks if the cursor is in the "pending wrap" state at right margin.
    pending_wrap: bool,
    /// Application cursor keys mode (DECCKM).
    app_cursor_keys: bool,
    /// Application keypad mode (DECKPAM).
    #[allow(dead_code)]
    app_keypad: bool,
    /// Bracketed paste mode.
    bracketed_paste: bool,

    /// DECSC's saved cursor, one per screen: `[main, alternate]`.
    ///
    /// One slot served both, so a full-screen program that saved its own
    /// cursor on the alternate screen overwrote the one `?1049` had saved
    /// for the shell, and leaving the program put the shell's cursor where
    /// the program's had been. xterm keeps one per screen.
    saved: [SavedCursor; 2],
}

impl TerminalState {
    /// Create a new terminal with the given configuration.
    pub fn new(config: TerminalConfig) -> Self {
        let rows = config.rows;
        let cols = config.cols;

        let mut tab_stops = vec![false; cols];
        // Default tab stops every 8 columns
        for i in (0..cols).step_by(8) {
            if let Some(stop) = tab_stops.get_mut(i) {
                *stop = true;
            }
        }

        let screen: Vec<TermLine> = (0..rows).map(|_| TermLine::new(cols)).collect();
        let alt_screen: Vec<TermLine> = (0..rows).map(|_| TermLine::new(cols)).collect();

        Self {
            config: config.clone(),
            screen,
            scrollback: VecDeque::new(),
            resized_into_scrollback: 0,
            alt_screen,
            alt_screen_active: false,
            saved_cursor_main: (0, 0),
            saved_cursor_alt: (0, 0),
            cursor_row: 0,
            cursor_col: 0,
            current_attrs: CellAttrs::default(),
            cursor_style: config.cursor_style,
            cursor_visible: true,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            tab_stops,
            parser_state: ParserState::Ground,
            csi_params: Vec::with_capacity(16),
            csi_current_param: 0,
            csi_intermediates: Vec::with_capacity(4),
            csi_param_started: false,
            csi_private_marker: None,
            osc_string: String::new(),
            title: String::from("Terminal"),
            child: None,
            child_exit: None,
            quiet_ms: 0,
            close_requested: false,
            waker: None,
            link_wakes: false,
            backlog: false,
            bell_flash_ms: 0,
            blink_ms: 0,
            blink_on: true,
            text_blink_on: true,
            text_blink_ms: 0,
            focused: true,
            tick_changed: false,
            size: (
                usize_f32(cols) * config.cell_width + BAR_W,
                usize_f32(rows) * config.cell_height,
            ),
            selection: None,
            scroll_offset: 0,
            wheel: wheel::Accumulator::default(),
            output_buffer: Vec::new(),
            origin_mode: false,
            auto_wrap: true,
            insert_mode: false,
            pending_wrap: false,
            app_cursor_keys: false,
            app_keypad: false,
            bracketed_paste: false,
            saved: [SavedCursor::default(); 2],
        }
    }

    /// Number of columns in the terminal grid.
    pub fn cols(&self) -> usize {
        self.config.cols
    }

    /// Number of visible rows in the terminal grid.
    pub fn rows(&self) -> usize {
        self.config.rows
    }

    // ========================================================================
    // Input processing — feed bytes from child process
    // ========================================================================

    /// Feed a byte stream from the child process into the terminal.
    ///
    /// Parses escape sequences and updates the internal state. Any response
    /// data (e.g., device status reports) leaves by the same route as a
    /// keystroke -- [`Self::to_child`], flushed by [`Self::flush_to_child`].
    pub fn feed(&mut self, data: &[u8]) {
        for &byte in data {
            self.process_byte(byte);
        }
    }

    /// Process a single byte through the parser state machine.
    fn process_byte(&mut self, byte: u8) {
        match self.parser_state.clone() {
            ParserState::Ground => self.ground_byte(byte),
            ParserState::Escape => self.escape_byte(byte),
            ParserState::Csi => self.csi_byte(byte),
            ParserState::Osc => self.osc_byte(byte),
            ParserState::OscEscape => self.osc_escape_byte(byte),
            ParserState::Dcs => self.dcs_byte(byte),
            ParserState::Utf8 {
                remaining,
                codepoint,
            } => {
                self.utf8_byte(byte, remaining, codepoint);
            }
        }
    }

    /// Process a byte in the ground (normal) state.
    fn ground_byte(&mut self, byte: u8) {
        match byte {
            // C0 control characters
            0x00 => {} // NUL — ignore
            0x07 => self.bell(),
            0x08 => self.backspace(),
            0x09 => self.tab(),
            0x0A..=0x0C => self.linefeed(),
            0x0D => self.carriage_return(),
            0x1B => {
                self.parser_state = ParserState::Escape;
            }
            // DEL — ignore
            0x7F => {}
            // UTF-8 multi-byte start
            0xC0..=0xDF => {
                let codepoint = (byte as u32) & 0x1F;
                self.parser_state = ParserState::Utf8 {
                    remaining: 1,
                    codepoint,
                };
            }
            0xE0..=0xEF => {
                let codepoint = (byte as u32) & 0x0F;
                self.parser_state = ParserState::Utf8 {
                    remaining: 2,
                    codepoint,
                };
            }
            0xF0..=0xF7 => {
                let codepoint = (byte as u32) & 0x07;
                self.parser_state = ParserState::Utf8 {
                    remaining: 3,
                    codepoint,
                };
            }
            // Printable ASCII or other single-byte
            0x20..=0x7E => {
                self.put_char(byte as char);
            }
            // Invalid or unhandled high bytes treated as replacement character
            _ => {
                self.put_char('\u{FFFD}');
            }
        }
    }

    /// Process a byte after ESC.
    fn escape_byte(&mut self, byte: u8) {
        match byte {
            b'[' => {
                // CSI sequence
                self.parser_state = ParserState::Csi;
                self.csi_params.clear();
                self.csi_current_param = 0;
                self.csi_intermediates.clear();
                self.csi_param_started = false;
                self.csi_private_marker = None;
            }
            b']' => {
                // OSC sequence
                self.parser_state = ParserState::Osc;
                self.osc_string.clear();
            }
            b'P' => {
                // DCS sequence (currently just consume until ST)
                self.parser_state = ParserState::Dcs;
            }
            b'7' => {
                // DECSC — Save cursor
                self.save_cursor();
                self.parser_state = ParserState::Ground;
            }
            b'8' => {
                // DECRC — Restore cursor
                self.restore_cursor();
                self.parser_state = ParserState::Ground;
            }
            b'D' => {
                // IND — Index (move cursor down, scroll if at bottom)
                self.index_down();
                self.parser_state = ParserState::Ground;
            }
            b'E' => {
                // NEL — Next line
                self.carriage_return();
                self.index_down();
                self.parser_state = ParserState::Ground;
            }
            b'H' => {
                // HTS — Set tab stop at current column
                self.set_tab_stop();
                self.parser_state = ParserState::Ground;
            }
            b'M' => {
                // RI — Reverse index (move cursor up, scroll if at top)
                self.reverse_index();
                self.parser_state = ParserState::Ground;
            }
            b'c' => {
                // RIS — Full reset
                self.full_reset();
                self.parser_state = ParserState::Ground;
            }
            b'\\' => {
                // ST — String Terminator (ends OSC/DCS outside those states)
                self.parser_state = ParserState::Ground;
            }
            _ => {
                // Unrecognized escape sequence — return to ground
                self.parser_state = ParserState::Ground;
            }
        }
    }

    /// Process a byte inside a CSI sequence.
    fn csi_byte(&mut self, byte: u8) {
        match byte {
            // Parameter bytes
            b'0'..=b'9' => {
                self.csi_param_started = true;
                self.csi_current_param = self
                    .csi_current_param
                    .saturating_mul(10)
                    .saturating_add(u16::from(byte.wrapping_sub(b'0')));
            }
            b';' => {
                self.csi_params.push(self.csi_current_param);
                self.csi_current_param = 0;
                self.csi_param_started = false;
            }
            // Private marker (e.g., '?' or '>')
            b'?' | b'>' | b'<' | b'=' => {
                self.csi_private_marker = Some(byte);
            }
            // Intermediate bytes
            b' ' | b'!' | b'"' | b'#' | b'$' | b'%' | b'&' | b'\'' => {
                self.csi_intermediates.push(byte);
            }
            // Final byte — dispatch the CSI command
            0x40..=0x7E => {
                // Push the last parameter if we had any digits
                if self.csi_param_started || !self.csi_params.is_empty() {
                    self.csi_params.push(self.csi_current_param);
                }
                self.dispatch_csi(byte);
                self.parser_state = ParserState::Ground;
            }
            // C0 control chars can appear within CSI
            0x00..=0x1F => {
                self.ground_byte(byte);
            }
            _ => {
                // Invalid — abort sequence
                self.parser_state = ParserState::Ground;
            }
        }
    }

    /// Process a byte inside an OSC sequence.
    fn osc_byte(&mut self, byte: u8) {
        match byte {
            0x07 => {
                // BEL terminates OSC
                self.dispatch_osc();
                self.parser_state = ParserState::Ground;
            }
            0x1B => {
                // Possible ST (ESC \)
                self.parser_state = ParserState::OscEscape;
            }
            _ => {
                if let Some(ch) = char::from_u32(byte as u32) {
                    self.osc_string.push(ch);
                }
            }
        }
    }

    /// Handle byte after ESC within OSC (looking for ST = ESC \).
    fn osc_escape_byte(&mut self, byte: u8) {
        if byte == b'\\' {
            self.dispatch_osc();
            self.parser_state = ParserState::Ground;
        } else {
            // Not ST — the ESC was something else; discard and return to ground
            self.parser_state = ParserState::Ground;
        }
    }

    /// Process a byte inside a DCS sequence (consume until ST).
    fn dcs_byte(&mut self, byte: u8) {
        match byte {
            0x1B => {
                // Possible ST
                self.parser_state = ParserState::OscEscape;
            }
            0x07 => {
                // BEL can also terminate DCS in some terminals
                self.parser_state = ParserState::Ground;
            }
            _ => {
                // Consume and ignore DCS content
            }
        }
    }

    /// Process a UTF-8 continuation byte.
    fn utf8_byte(&mut self, byte: u8, remaining: u8, codepoint: u32) {
        if byte & 0xC0 != 0x80 {
            // Invalid continuation byte — emit replacement and reprocess
            self.put_char('\u{FFFD}');
            self.parser_state = ParserState::Ground;
            self.process_byte(byte);
            return;
        }

        let codepoint = (codepoint << 6) | (u32::from(byte) & 0x3F);
        // `saturating_sub` rather than `- 1`: this is only ever reached with
        // `remaining >= 1`, but the state that carries it comes from the byte
        // stream, and a decode bug that set it to zero should leave a
        // replacement character behind rather than a subtraction overflow.
        let remaining = remaining.saturating_sub(1);

        if remaining == 0 {
            let ch = char::from_u32(codepoint).unwrap_or('\u{FFFD}');
            self.put_char(ch);
            self.parser_state = ParserState::Ground;
        } else {
            self.parser_state = ParserState::Utf8 {
                remaining,
                codepoint,
            };
        }
    }

    // ========================================================================
    // Character output
    // ========================================================================

    /// Place a character at the current cursor position and advance.
    fn put_char(&mut self, ch: char) {
        if self.pending_wrap && self.auto_wrap {
            self.cursor_col = 0;
            self.index_down();
            self.pending_wrap = false;
        }

        let cols = self.cols();
        if self.cursor_col >= cols {
            self.cursor_col = cols.saturating_sub(1);
        }

        if self.insert_mode {
            // Shift cells right to make room
            let row = self.cursor_row;
            if let Some(line) = self.screen.get_mut(row) {
                let col = self.cursor_col;
                if col < cols {
                    line.cells.pop();
                    line.cells.insert(col, Cell::default());
                }
            }
        }

        // How many columns this character is entitled to.
        //
        // Every layout decision in this system is made from `charwidth`'s
        // table, and until 2026-09-14 this function advanced by exactly one
        // column for every character -- so 182,712 codepoints that the table
        // calls two cells wide got one, and 2,362 zero-width marks got one
        // they should not have. `ls` reserves two columns for a Chinese
        // character; the terminal drew it in one; every column after it on the
        // line was off by one and the next glyph was painted over the half
        // that was never allocated. See
        // `requests/b-c-the-terminal-gives-every-character-one-cell.md`.
        //
        // `None` is `wcwidth`'s -1 and means a control character, which should
        // not reach here -- the parser handles those. One column is the safe
        // reading if one does: it is what this function did for everything
        // before, so an unexpected input cannot be made worse by the change.
        let width = charwidth::char_width(ch).unwrap_or(1);

        // A combining mark attaches to what it follows rather than taking a
        // cell of its own. It is *not* written: doing so would replace the
        // base character with the accent. Drawing the two together is the font
        // layer's problem and is not what this request is about.
        if width == 0 {
            return;
        }

        // A double-width character at the last column does not fit. Wrapping
        // first is what every terminal does, and the alternative -- splitting
        // it across the margin -- is not representable in a grid of cells.
        if width == 2 && self.cursor_col.saturating_add(1) >= cols && self.auto_wrap {
            self.cursor_col = 0;
            self.index_down();
        }

        // Whatever was here, this write ends it being half of something.
        let row = self.cursor_row;
        let col = self.cursor_col;
        self.break_pair_at(row, col);
        if width == 2 {
            self.break_pair_at(row, col.saturating_add(1));
        }

        // Write the character to the cell
        if let Some(line) = self.screen.get_mut(self.cursor_row)
            && let Some(cell) = line.cells.get_mut(self.cursor_col)
        {
            cell.ch = ch;
            cell.attrs = self.current_attrs;
            cell.continuation = false;
        }
        if width == 2
            && let Some(line) = self.screen.get_mut(self.cursor_row)
            && let Some(cell) = line.cells.get_mut(self.cursor_col.saturating_add(1))
        {
            // A space, so the renderer skips it with no change; the flag is
            // what stops the text path yielding a space after every wide
            // character.
            cell.ch = ' ';
            cell.attrs = self.current_attrs;
            cell.continuation = true;
        }

        // Advance cursor
        let last = cols.saturating_sub(1);
        if self.cursor_col.saturating_add(width) > last {
            if self.auto_wrap {
                self.pending_wrap = true;
            }
            self.cursor_col = last;
        } else {
            self.cursor_col = self.cursor_col.saturating_add(width);
        }
    }

    // ========================================================================
    // Control character handlers
    // ========================================================================

    fn bell(&mut self) {
        self.bell_flash_ms = BELL_MS;
    }

    /// Advance everything that is measured in time by `elapsed_ms`.
    ///
    /// By the interval the clock reports, never by the interval the app asked
    /// for: ticks arrive when the loop next runs, so a frame that took three
    /// intervals must age the bell by three.
    pub fn tick(&mut self, elapsed_ms: u64) -> bool {
        let mut changed = false;
        if self.bell_flash_ms > 0 {
            self.bell_flash_ms = self.bell_flash_ms.saturating_sub(elapsed_ms);
            changed = true;
        }
        if self.config.cursor_blink && self.cursor_visible && self.focused {
            self.blink_ms = self.blink_ms.saturating_add(elapsed_ms);
            while self.blink_ms >= BLINK_MS {
                self.blink_ms = self.blink_ms.saturating_sub(BLINK_MS);
                self.blink_on = !self.blink_on;
                changed = true;
            }
        } else if !self.blink_on {
            // A cursor whose blink was turned off mid-blink must not be left in
            // the dark half of it for the rest of the session.
            self.blink_on = true;
            changed = true;
        }
        if self.shows_blinking_text() {
            self.text_blink_ms = self.text_blink_ms.saturating_add(elapsed_ms);
            while self.text_blink_ms >= BLINK_MS {
                self.text_blink_ms = self.text_blink_ms.saturating_sub(BLINK_MS);
                self.text_blink_on = !self.text_blink_on;
                changed = true;
            }
        } else if !self.text_blink_on {
            // Nothing blinking is left on screen; the next thing that blinks
            // starts lit.
            self.text_blink_on = true;
            self.text_blink_ms = 0;
        }
        changed
    }

    /// Whether any cell on screen is marked blinking -- the only time the
    /// text's blink needs a clock.
    fn shows_blinking_text(&self) -> bool {
        (0..self.rows()).any(|row| {
            self.line_at(self.buffer_row_of(row))
                .is_some_and(|line| line.cells.iter().any(|c| c.attrs.blink && c.ch != ' '))
        })
    }

    /// Everything a tick does: age the clocks, then read the child.
    ///
    /// One method rather than a step in `dispatch_event` and a second step in
    /// `on_event`, because the two used to disagree. `on_event` answered a
    /// tick itself -- it needed to know whether anything had changed before it
    /// could say whether the frame was worth drawing -- and returned before
    /// `dispatch_event` could run, so the read of the child never happened in
    /// the one path the compositor actually uses. Under a real window the
    /// terminal was write-only: the shell's prompt was produced and never
    /// collected. Every test that caught the read called `handle_event`
    /// directly, which is the path the compositor does not take.
    fn on_tick(&mut self, elapsed_ms: u64) {
        let aged = self.tick(elapsed_ms);
        self.quiet_ms = self.quiet_ms.saturating_add(elapsed_ms);
        let read = self.drain_child();
        self.tick_changed = aged || read;
    }

    /// Connect the terminal to the program it runs.
    ///
    /// The child is told the grid's size at once: it was started at whatever
    /// size the caller guessed, and the window may already have been resized.
    pub fn attach(&mut self, mut link: Box<dyn Link>) {
        link.resize(self.win_size());
        // The waker, if the window has given one: the link wakes the terminal
        // for its child's output from here on, or says it cannot.
        self.link_wakes = self.waker.clone().is_some_and(|w| link.set_waker(w));
        self.backlog = false;
        self.child = Some(link);
        self.child_exit = None;
        self.close_requested = false;
        self.quiet_ms = 0;
    }

    /// Start the program this terminal runs and attach it -- or, when it
    /// cannot be started, say so on the screen.
    ///
    /// Two outcomes, and the screen says which. A started shell greets the
    /// user itself, with its own prompt; a failed one is named, with the
    /// reason, where the user is looking -- **a terminal with no shell must
    /// not look like a terminal with a quiet one.**
    ///
    /// `spawn` is [`child::spawn_shell`] in a real window, and is handed the
    /// grid's size so the shell is born at the size it is drawn at. It is a
    /// parameter so that a test can supply a scripted child, and so that the
    /// terminal and the multiplexer start their shells the same way.
    pub fn start_with(&mut self, spawn: impl FnOnce(WinSize) -> Result<Box<dyn Link>, SpawnError>) {
        match spawn(self.win_size()) {
            Ok(link) => self.attach(link),
            Err(why) => {
                let msg = format!(
                    "\x1b[33mNo shell.\x1b[0m {why}\r\nNothing is connected to this \
terminal, so what you type goes nowhere.\r\n"
                );
                self.feed(msg.as_bytes());
            }
        }
    }

    /// Whether a child is attached and has not finished.
    pub fn child_is_live(&self) -> bool {
        self.child.is_some() && self.child_exit.is_none()
    }

    /// How the child ended, once it has.
    pub fn exit_status(&self) -> Option<Exit> {
        self.child_exit
    }

    /// Give this terminal the keyboard, or take it away.
    ///
    /// The cursor is woken either way, so that it is never left in the dark
    /// half of a blink it is no longer doing.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.wake_cursor();
    }

    /// Whether this terminal has the keyboard.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Send pasted text to the child, as a paste.
    ///
    /// Line breaks go as carriage returns, which is what Enter sends and
    /// what a program reading raw keys takes for one; a bare line feed is
    /// Ctrl+J to it. When the program has asked for bracketed paste
    /// (`CSI ? 2004 h`) the text is fenced in `ESC [ 200 ~` ... `ESC [ 201 ~`,
    /// so that a shell can tell a pasted command from a typed one and does
    /// not run it the moment its first line break arrives -- the mode was
    /// recorded here and read by nothing. Inside the fence the text's own
    /// escape characters are dropped: one of them could spell the closing
    /// fence, and everything after it would be run as though typed.
    ///
    /// Like a keystroke, a paste returns the view to the live screen.
    pub fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let body = text.replace("\r\n", "\r").replace('\n', "\r");
        self.scroll_offset = 0;
        self.wake_cursor();
        if self.bracketed_paste {
            let fenced: String = body.chars().filter(|&c| c != '\x1b').collect();
            self.to_child(b"\x1b[200~");
            self.to_child(fenced.as_bytes());
            self.to_child(b"\x1b[201~");
        } else {
            self.to_child(body.as_bytes());
        }
        self.flush_to_child();
    }

    /// Tell the child its terminal is going away.
    ///
    /// Idempotent. Dropping the link hangs up as well; this is for a caller
    /// that keeps the terminal -- a multiplexer closing one pane of several
    /// says so before it lets the pane go.
    pub fn hang_up(&mut self) {
        if let Some(link) = self.child.as_mut() {
            link.hang_up();
        }
    }

    /// The grid's size as the child's terminal reports it: cells, and the
    /// pixels those cells cover.
    pub fn win_size(&self) -> WinSize {
        let l = self.layout();
        WinSize {
            rows: u16_of(l.rows),
            cols: u16_of(l.cols),
            xpixel: pixels_u16(l.grid.w),
            ypixel: pixels_u16(l.grid.h),
        }
    }

    /// Queue bytes for the child.
    ///
    /// Queued rather than written, so that everything the terminal has to say
    /// leaves by one route -- see [`Self::flush_to_child`].
    pub fn to_child(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.output_buffer.extend_from_slice(bytes);
    }

    /// Send everything queued for the child.
    ///
    /// The one place bytes leave the terminal, because they used to leave from
    /// two and one of them was a dead end: keystrokes went straight to the PTY
    /// from `to_child`, while the parser's *own* replies -- the cursor position
    /// report, the device attributes answer, the status report -- were appended
    /// to `output_buffer` and sent nowhere at all. A program that asked this
    /// terminal where its cursor was waited for an answer that was sitting in a
    /// `Vec`.
    pub fn flush_to_child(&mut self) {
        if self.output_buffer.is_empty() {
            return;
        }
        let Some(link) = self.child.as_mut() else {
            // Nothing to take them. They stay queued rather than being dropped:
            // a terminal whose shell failed to start should not also silently
            // lose what was typed into it.
            return;
        };
        // A short send is the child not keeping up rather than an error: what
        // was taken leaves the queue and the rest waits for the next flush,
        // which is the difference between input arriving late and input
        // arriving truncated.
        let taken = link.send(&self.output_buffer).min(self.output_buffer.len());
        if taken > 0 {
            self.quiet_ms = 0;
        }
        self.output_buffer.drain(..taken);
    }

    /// Take whatever the child has written and feed it to the parser, and say
    /// so once the child has finished.
    ///
    /// Returns whether anything changed on screen -- or whether the window is
    /// about to close, which is a change too.
    ///
    /// The output is read *before* the exit is asked about, and the link does
    /// not report an exit until the child's output has closed, so a program's
    /// last lines always come before the note that it has gone.
    pub fn drain_child(&mut self) -> bool {
        let Some(link) = self.child.as_mut() else {
            return false;
        };
        let mut got = Vec::new();
        link.receive(&mut got, MAX_READ_PER_DRAIN);
        // A read that took all it was allowed may have left more behind.
        self.backlog = got.len() >= MAX_READ_PER_DRAIN;
        // Asked every time: the link reports an exit exactly once, so a
        // finished child answers `None` from then on.
        let exit = link.poll_exit();

        let mut changed = false;
        if !got.is_empty() {
            self.quiet_ms = 0;
            self.feed(&got);
            changed = true;
        }
        if let Some(exit) = exit {
            // An empty read cannot tell "nothing yet" from "never again", so a
            // terminal whose shell has gone otherwise sits at a dead prompt
            // looking exactly like one waiting for output.
            self.child_exit = Some(exit);
            if exit.is_clean() {
                self.close_requested = true;
            } else {
                let note = format!("\r\n\x1b[0;2m[the shell {exit}]\x1b[0m\r\n");
                self.feed(note.as_bytes());
            }
            changed = true;
        }
        changed
    }

    /// Show the cursor solidly again, and restart its blink.
    ///
    /// Called whenever the user types or the child writes: a cursor that
    /// happens to be in the dark half of its blink when a key arrives should
    /// not swallow the feedback for that key.
    fn wake_cursor(&mut self) {
        self.blink_on = true;
        self.blink_ms = 0;
    }

    /// Break any double-width pair that `col` is part of, so that writing
    /// there cannot leave half a character behind.
    ///
    /// Two directions, and both happen. Writing onto the *second* half orphans
    /// the lead, which would keep drawing as a wide glyph over a cell that now
    /// holds something else. Writing onto the *first* half orphans the
    /// continuation, which would swallow the character to its right by drawing
    /// nothing where that character should be.
    ///
    /// Clearing to a space rather than to the incoming attributes: the cell
    /// being vacated is not the cell being written, and giving it the new
    /// character's colours would tint a blank the user never typed.
    fn break_pair_at(&mut self, row: usize, col: usize) {
        let Some(line) = self.screen.get_mut(row) else {
            return;
        };
        if line.cells.get(col).is_some_and(|c| c.continuation) {
            if let Some(lead) = col.checked_sub(1)
                && let Some(cell) = line.cells.get_mut(lead)
            {
                cell.ch = ' ';
                cell.continuation = false;
            }
            return;
        }
        let after = col.saturating_add(1);
        if line.cells.get(after).is_some_and(|c| c.continuation)
            && let Some(cell) = line.cells.get_mut(after)
        {
            cell.ch = ' ';
            cell.continuation = false;
        }
    }

    fn backspace(&mut self) {
        self.pending_wrap = false;
        if self.cursor_col > 0 {
            self.cursor_col = self.cursor_col.saturating_sub(1);
        }
        // Landing on the second half of a wide character means the cursor is
        // inside one character, which no further operation has a sensible
        // reading of. Step onto its lead, so backspacing over a wide character
        // moves past the whole of it -- which is what the one-unit rule in
        // `requests/b-c-the-terminal-gives-every-character-one-cell.md` asks
        // for and what a user pressing backspace expects.
        if self
            .screen
            .get(self.cursor_row)
            .and_then(|l| l.cells.get(self.cursor_col))
            .is_some_and(|c| c.continuation)
            && self.cursor_col > 0
        {
            self.cursor_col = self.cursor_col.saturating_sub(1);
        }
    }

    fn tab(&mut self) {
        let cols = self.cols();
        let start = self.cursor_col.saturating_add(1);
        for col in start..cols {
            if self.tab_stops.get(col).copied().unwrap_or(false) {
                self.cursor_col = col;
                return;
            }
        }
        // No more tab stops — go to last column
        self.cursor_col = cols.saturating_sub(1);
    }

    fn linefeed(&mut self) {
        self.pending_wrap = false;
        self.index_down();
    }

    fn carriage_return(&mut self) {
        self.pending_wrap = false;
        self.cursor_col = 0;
    }

    /// Move the cursor down one line, scrolling if at the bottom of the scroll region.
    fn index_down(&mut self) {
        if self.cursor_row == self.scroll_bottom {
            self.scroll_up(1);
        } else if self.cursor_row < self.rows().saturating_sub(1) {
            self.cursor_row = self.cursor_row.saturating_add(1);
        }
    }

    /// Move the cursor up one line, scrolling if at the top of the scroll region.
    fn reverse_index(&mut self) {
        if self.cursor_row == self.scroll_top {
            self.scroll_down(1);
        } else if self.cursor_row > 0 {
            self.cursor_row = self.cursor_row.saturating_sub(1);
        }
    }

    /// Scroll the scroll region up by `count` lines (content moves up, new blank lines at bottom).
    fn scroll_up(&mut self, count: usize) {
        let cols = self.cols();
        for _ in 0..count {
            // If scroll region is the full screen and not on alt screen, save to scrollback
            if self.scroll_top == 0
                && self.scroll_bottom == self.rows().saturating_sub(1)
                && !self.alt_screen_active
                && let Some(line) = self.screen.first()
            {
                self.scrollback.push_back(line.clone());
                if self.scrollback.len() > self.config.scrollback_limit {
                    self.scrollback.pop_front();
                    self.forget_oldest_row();
                }
            }

            // Remove the top line of the scroll region and insert a blank at the bottom
            let top = self.scroll_top;
            let bottom = self.scroll_bottom;
            if top < self.screen.len() && bottom < self.screen.len() {
                self.screen.remove(top);
                let insert_pos = if bottom < self.screen.len() {
                    bottom
                } else {
                    self.screen.len()
                };
                self.screen.insert(insert_pos, TermLine::new(cols));
            }
        }
    }

    /// Scroll the scroll region down by `count` lines (content moves down, new blank lines at top).
    fn scroll_down(&mut self, count: usize) {
        let cols = self.cols();
        for _ in 0..count {
            let bottom = self.scroll_bottom;
            let top = self.scroll_top;
            if bottom < self.screen.len() && top < self.screen.len() {
                self.screen.remove(bottom);
                self.screen.insert(top, TermLine::new(cols));
            }
        }
    }

    fn set_tab_stop(&mut self) {
        if let Some(stop) = self.tab_stops.get_mut(self.cursor_col) {
            *stop = true;
        }
    }

    fn save_cursor(&mut self) {
        let saved = SavedCursor {
            row: self.cursor_row,
            col: self.cursor_col,
            attrs: self.current_attrs,
        };
        if let Some(slot) = self.saved.get_mut(usize::from(self.alt_screen_active)) {
            *slot = saved;
        }
    }

    fn restore_cursor(&mut self) {
        let saved = self
            .saved
            .get(usize::from(self.alt_screen_active))
            .copied()
            .unwrap_or_default();
        self.cursor_row = saved.row;
        self.cursor_col = saved.col;
        self.current_attrs = saved.attrs;
        self.clamp_cursor();
    }

    /// `ESC c`: the emulator as it was made -- screen, modes, tab stops,
    /// attributes -- keeping everything that is not the emulator's.
    ///
    /// It rebuilt the whole of `self` from the config, and so dropped the
    /// link to the child along with everything else. Dropping the link
    /// hangs up: **a user who typed `reset` at a shell killed the shell.**
    /// The child, the window's size and focus, and what is queued for the
    /// child all survive it now.
    fn full_reset(&mut self) {
        let mut fresh = Self::new(self.config.clone());
        fresh.child = self.child.take();
        fresh.child_exit = self.child_exit;
        fresh.quiet_ms = self.quiet_ms;
        fresh.close_requested = self.close_requested;
        fresh.size = self.size;
        fresh.focused = self.focused;
        fresh.output_buffer = std::mem::take(&mut self.output_buffer);
        *self = fresh;
    }

    /// Ensure cursor is within grid bounds.
    fn clamp_cursor(&mut self) {
        let rows = self.rows();
        let cols = self.cols();
        if self.cursor_row >= rows {
            self.cursor_row = rows.saturating_sub(1);
        }
        if self.cursor_col >= cols {
            self.cursor_col = cols.saturating_sub(1);
        }
    }

    // ========================================================================
    // CSI sequence dispatch
    // ========================================================================

    /// Dispatch a completed CSI sequence.
    fn dispatch_csi(&mut self, final_byte: u8) {
        let params_vec = self.csi_params.clone();
        let params: &[u16] = &params_vec;
        let private = self.csi_private_marker;

        match (final_byte, private) {
            // Cursor movement
            (b'A', None) => {
                // CUU — Cursor Up
                let n = Self::param_or(params, 0, 1) as usize;
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.pending_wrap = false;
            }
            (b'B', None) => {
                // CUD — Cursor Down
                let n = Self::param_or(params, 0, 1) as usize;
                let max_row = self.rows().saturating_sub(1);
                self.cursor_row = self.cursor_row.saturating_add(n).min(max_row);
                self.pending_wrap = false;
            }
            (b'C', None) => {
                // CUF — Cursor Forward
                let n = Self::param_or(params, 0, 1) as usize;
                let max_col = self.cols().saturating_sub(1);
                self.cursor_col = self.cursor_col.saturating_add(n).min(max_col);
                self.pending_wrap = false;
            }
            (b'D', None) => {
                // CUB — Cursor Back
                let n = Self::param_or(params, 0, 1) as usize;
                self.cursor_col = self.cursor_col.saturating_sub(n);
                self.pending_wrap = false;
            }
            (b'E', None) => {
                // CNL — Cursor Next Line
                let n = Self::param_or(params, 0, 1) as usize;
                let max_row = self.rows().saturating_sub(1);
                self.cursor_row = self.cursor_row.saturating_add(n).min(max_row);
                self.cursor_col = 0;
                self.pending_wrap = false;
            }
            (b'F', None) => {
                // CPL — Cursor Previous Line
                let n = Self::param_or(params, 0, 1) as usize;
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.cursor_col = 0;
                self.pending_wrap = false;
            }
            (b'G', None) => {
                // CHA — Cursor Horizontal Absolute
                let col = Self::param_or(params, 0, 1) as usize;
                self.cursor_col = col.saturating_sub(1).min(self.cols().saturating_sub(1));
                self.pending_wrap = false;
            }
            (b'H', None) | (b'f', None) => {
                // CUP / HVP — Cursor Position
                let row = Self::param_or(params, 0, 1) as usize;
                let col = Self::param_or(params, 1, 1) as usize;
                let base_row = if self.origin_mode { self.scroll_top } else { 0 };
                self.cursor_row = base_row
                    .saturating_add(row.saturating_sub(1))
                    .min(self.rows().saturating_sub(1));
                self.cursor_col = col.saturating_sub(1).min(self.cols().saturating_sub(1));
                self.pending_wrap = false;
            }
            (b'd', None) => {
                // VPA — Vertical Position Absolute
                let row = Self::param_or(params, 0, 1) as usize;
                self.cursor_row = row.saturating_sub(1).min(self.rows().saturating_sub(1));
                self.pending_wrap = false;
            }

            // Erase
            (b'J', None) => {
                // ED — Erase in Display
                let mode = Self::param_or(params, 0, 0);
                self.erase_display(mode);
            }
            (b'K', None) => {
                // EL — Erase in Line
                let mode = Self::param_or(params, 0, 0);
                self.erase_line(mode);
            }

            // Insert/Delete
            (b'L', None) => {
                // IL — Insert Lines
                let n = Self::param_or(params, 0, 1) as usize;
                self.insert_lines(n);
            }
            (b'M', None) => {
                // DL — Delete Lines
                let n = Self::param_or(params, 0, 1) as usize;
                self.delete_lines(n);
            }
            (b'@', None) => {
                // ICH — Insert Characters
                let n = Self::param_or(params, 0, 1) as usize;
                self.insert_chars(n);
            }
            (b'P', None) => {
                // DCH — Delete Characters
                let n = Self::param_or(params, 0, 1) as usize;
                self.delete_chars(n);
            }
            (b'X', None) => {
                // ECH — Erase Characters
                let n = Self::param_or(params, 0, 1) as usize;
                self.erase_chars(n);
            }

            // Scroll
            (b'S', None) => {
                // SU — Scroll Up
                let n = Self::param_or(params, 0, 1) as usize;
                self.scroll_up(n);
            }
            (b'T', None) => {
                // SD — Scroll Down
                let n = Self::param_or(params, 0, 1) as usize;
                self.scroll_down(n);
            }

            // SGR — Select Graphic Rendition
            (b'm', None) => {
                self.dispatch_sgr();
            }

            // Set scroll region (DECSTBM)
            (b'r', None) => {
                let top = Self::param_or(params, 0, 1) as usize;
                let bottom = Self::param_or(params, 1, self.rows() as u16) as usize;
                self.scroll_top = top.saturating_sub(1);
                self.scroll_bottom = bottom.saturating_sub(1).min(self.rows().saturating_sub(1));
                if self.scroll_top >= self.scroll_bottom {
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows().saturating_sub(1);
                }
                // Move cursor to home
                self.cursor_row = if self.origin_mode { self.scroll_top } else { 0 };
                self.cursor_col = 0;
                self.pending_wrap = false;
            }

            // Tab clear
            (b'g', None) => {
                let mode = Self::param_or(params, 0, 0);
                match mode {
                    0 => {
                        // Clear tab stop at current column
                        if let Some(stop) = self.tab_stops.get_mut(self.cursor_col) {
                            *stop = false;
                        }
                    }
                    3 => {
                        // Clear all tab stops
                        for stop in &mut self.tab_stops {
                            *stop = false;
                        }
                    }
                    _ => {}
                }
            }

            // Device Status Report
            (b'n', None) => {
                let mode = Self::param_or(params, 0, 0);
                match mode {
                    5 => {
                        // Status report — report OK
                        self.to_child(b"\x1b[0n");
                    }
                    6 => {
                        // Cursor position report
                        let report = format!(
                            "\x1b[{};{}R",
                            self.cursor_row.saturating_add(1),
                            self.cursor_col.saturating_add(1)
                        );
                        self.to_child(report.as_bytes());
                    }
                    _ => {}
                }
            }

            // Set Mode / Reset Mode (DEC private)
            (b'h', Some(b'?')) => {
                for &p in params.iter() {
                    self.set_dec_private_mode(p, true);
                }
            }
            (b'l', Some(b'?')) => {
                for &p in params.iter() {
                    self.set_dec_private_mode(p, false);
                }
            }

            // SM/RM — ANSI modes
            (b'h', None) => {
                for &p in params.iter() {
                    self.set_ansi_mode(p, true);
                }
            }
            (b'l', None) => {
                for &p in params.iter() {
                    self.set_ansi_mode(p, false);
                }
            }

            // Cursor style (DECSCUSR)
            (b'q', None) if self.csi_intermediates.first() == Some(&b' ') => {
                let style = Self::param_or(params, 0, 1);
                self.cursor_style = match style {
                    0 | 1 => CursorStyle::Block, // blinking block
                    2 => CursorStyle::Block,     // steady block
                    3 => CursorStyle::Underline, // blinking underline
                    4 => CursorStyle::Underline, // steady underline
                    5 => CursorStyle::Bar,       // blinking bar
                    6 => CursorStyle::Bar,       // steady bar
                    _ => CursorStyle::Block,
                };
                self.config.cursor_blink = matches!(style, 0 | 1 | 3 | 5);
            }

            // DA — Device Attributes
            (b'c', None) | (b'c', Some(b'>')) => {
                // Report as VT220
                self.to_child(b"\x1b[?62;c");
            }

            _ => {
                // Unrecognized CSI sequence — ignore
            }
        }
    }

    /// Get a CSI parameter by index, with a default value if not present.
    fn param_or(params: &[u16], index: usize, default: u16) -> u16 {
        params
            .get(index)
            .copied()
            .filter(|&v| v != 0)
            .unwrap_or(default)
    }

    // ========================================================================
    // SGR (Select Graphic Rendition)
    // ========================================================================

    /// Parse and apply SGR parameters.
    fn dispatch_sgr(&mut self) {
        let params = self.csi_params.clone();
        if params.is_empty() {
            self.current_attrs = CellAttrs::default();
            return;
        }

        let mut i = 0_usize;
        while let Some(&p) = params.get(i) {
            match p {
                0 => self.current_attrs = CellAttrs::default(),
                1 => self.current_attrs.bold = true,
                2 => self.current_attrs.dim = true,
                3 => self.current_attrs.italic = true,
                4 => self.current_attrs.underline = true,
                5 | 6 => self.current_attrs.blink = true,
                7 => self.current_attrs.inverse = true,
                8 => self.current_attrs.hidden = true,
                9 => self.current_attrs.strikethrough = true,
                21 => self.current_attrs.underline = true, // double underline (treat as underline)
                22 => {
                    self.current_attrs.bold = false;
                    self.current_attrs.dim = false;
                }
                23 => self.current_attrs.italic = false,
                24 => self.current_attrs.underline = false,
                25 => self.current_attrs.blink = false,
                27 => self.current_attrs.inverse = false,
                28 => self.current_attrs.hidden = false,
                29 => self.current_attrs.strikethrough = false,

                // Foreground colors (30-37)
                30..=37 => {
                    self.current_attrs.fg = TermColor::Indexed(palette_index(p.saturating_sub(30)));
                }
                // Default foreground
                39 => self.current_attrs.fg = TermColor::Default,
                // Background colors (40-47)
                40..=47 => {
                    self.current_attrs.bg = TermColor::Indexed(palette_index(p.saturating_sub(40)));
                }
                // Default background
                49 => self.current_attrs.bg = TermColor::Default,

                // 256-color and truecolor foreground
                38 => {
                    if let Some(color) = self.parse_extended_color(&params, &mut i) {
                        self.current_attrs.fg = color;
                    }
                }
                // 256-color and truecolor background
                48 => {
                    if let Some(color) = self.parse_extended_color(&params, &mut i) {
                        self.current_attrs.bg = color;
                    }
                }

                // Bright foreground colors (90-97)
                90..=97 => {
                    self.current_attrs.fg =
                        TermColor::Indexed(palette_index(p.saturating_sub(90).saturating_add(8)));
                }
                // Bright background colors (100-107)
                100..=107 => {
                    self.current_attrs.bg =
                        TermColor::Indexed(palette_index(p.saturating_sub(100).saturating_add(8)));
                }

                _ => {} // Unrecognized SGR parameter — ignore
            }
            i = i.saturating_add(1);
        }
    }

    /// Parse an extended color (256-color or truecolor) from SGR params.
    /// Advances `i` past the consumed parameters.
    fn parse_extended_color(&self, params: &[u16], i: &mut usize) -> Option<TermColor> {
        let next = i.saturating_add(1);
        let &kind = params.get(next)?;
        match kind {
            5 => {
                // 256-color: 38;5;N or 48;5;N
                let color_idx = next.saturating_add(1);
                if let Some(&n) = params.get(color_idx) {
                    *i = color_idx;
                    Some(TermColor::Indexed(palette_index(n)))
                } else {
                    *i = next;
                    None
                }
            }
            2 => {
                // Truecolor: 38;2;R;G;B or 48;2;R;G;B
                let r_idx = next.saturating_add(1);
                let g_idx = next.saturating_add(2);
                let b_idx = next.saturating_add(3);
                // All three components fetched together: the old code tested
                // only `b_idx` for range and then indexed all three, which is
                // correct only because they are consecutive -- an invariant
                // nothing stated and the next edit could break.
                if let (Some(&r), Some(&g), Some(&b)) =
                    (params.get(r_idx), params.get(g_idx), params.get(b_idx))
                {
                    *i = b_idx;
                    Some(TermColor::Rgb(
                        palette_index(r),
                        palette_index(g),
                        palette_index(b),
                    ))
                } else {
                    *i = next;
                    None
                }
            }
            _ => None,
        }
    }

    // ========================================================================
    // DEC Private Modes
    // ========================================================================

    /// Set or reset a DEC private mode.
    fn set_dec_private_mode(&mut self, mode: u16, enable: bool) {
        match mode {
            1 => {
                // DECCKM — Application cursor keys
                self.app_cursor_keys = enable;
            }
            6 => {
                // DECOM — Origin mode
                self.origin_mode = enable;
                self.cursor_row = if enable { self.scroll_top } else { 0 };
                self.cursor_col = 0;
            }
            7 => {
                // DECAWM — Auto-wrap mode
                self.auto_wrap = enable;
            }
            12 => {
                // Cursor blink
                self.config.cursor_blink = enable;
            }
            25 => {
                // DECTCEM — Cursor visible
                self.cursor_visible = enable;
            }
            47 | 1047 => {
                // Alt screen buffer (without save/restore cursor)
                if enable && !self.alt_screen_active {
                    self.switch_to_alt_screen(false);
                } else if !enable && self.alt_screen_active {
                    self.switch_from_alt_screen(false);
                }
            }
            1048 => {
                // Save/restore cursor
                if enable {
                    self.save_cursor();
                } else {
                    self.restore_cursor();
                }
            }
            1049 => {
                // Alt screen with save/restore cursor (smcup/rmcup)
                if enable {
                    self.save_cursor();
                    self.switch_to_alt_screen(true);
                } else {
                    self.switch_from_alt_screen(true);
                    self.restore_cursor();
                }
            }
            2004 => {
                // Bracketed paste mode
                self.bracketed_paste = enable;
            }
            _ => {} // Unrecognized mode — ignore
        }
    }

    /// Set or reset an ANSI mode.
    fn set_ansi_mode(&mut self, mode: u16, enable: bool) {
        match mode {
            4 => {
                // IRM — Insert mode
                self.insert_mode = enable;
            }
            20 => {
                // LNM — Linefeed mode (auto CR after LF)
                // We don't implement this separately; our LF always just does LF
            }
            _ => {}
        }
    }

    // ========================================================================
    // Alt screen buffer
    // ========================================================================

    fn switch_to_alt_screen(&mut self, clear: bool) {
        let rows = self.rows();
        let cols = self.cols();
        self.saved_cursor_main = (self.cursor_row, self.cursor_col);
        std::mem::swap(&mut self.screen, &mut self.alt_screen);
        self.alt_screen_active = true;
        if clear {
            self.screen = (0..rows).map(|_| TermLine::new(cols)).collect();
        }
        self.cursor_row = self.saved_cursor_alt.0;
        self.cursor_col = self.saved_cursor_alt.1;
        self.clamp_cursor();
    }

    fn switch_from_alt_screen(&mut self, _clear: bool) {
        self.saved_cursor_alt = (self.cursor_row, self.cursor_col);
        std::mem::swap(&mut self.screen, &mut self.alt_screen);
        self.alt_screen_active = false;
        self.cursor_row = self.saved_cursor_main.0;
        self.cursor_col = self.saved_cursor_main.1;
        self.clamp_cursor();
    }

    // ========================================================================
    // Erase operations
    // ========================================================================

    /// Erase in display (ED).
    fn erase_display(&mut self, mode: u16) {
        let rows = self.rows();
        let cols = self.cols();
        match mode {
            0 => {
                // Erase from cursor to end of display
                self.erase_line(0);
                for row in self.cursor_row.saturating_add(1)..rows {
                    if let Some(line) = self.screen.get_mut(row) {
                        *line = TermLine::new(cols);
                    }
                }
            }
            1 => {
                // Erase from start to cursor
                for row in 0..self.cursor_row {
                    if let Some(line) = self.screen.get_mut(row) {
                        *line = TermLine::new(cols);
                    }
                }
                self.erase_line(1);
            }
            2 | 3 => {
                // Erase entire display (3 also clears scrollback)
                for row in 0..rows {
                    if let Some(line) = self.screen.get_mut(row) {
                        *line = TermLine::new(cols);
                    }
                }
                if mode == 3 {
                    self.scrollback.clear();
                    self.resized_into_scrollback = 0;
                }
            }
            _ => {}
        }
    }

    /// Erase in line (EL).
    fn erase_line(&mut self, mode: u16) {
        let cols = self.cols();
        let row = self.cursor_row;
        if let Some(line) = self.screen.get_mut(row) {
            match mode {
                0 => {
                    // Erase from cursor to end of line
                    for col in self.cursor_col..cols {
                        if let Some(cell) = line.cells.get_mut(col) {
                            *cell = Cell::default();
                        }
                    }
                }
                1 => {
                    // Erase from start to cursor
                    for col in 0..=self.cursor_col.min(cols.saturating_sub(1)) {
                        if let Some(cell) = line.cells.get_mut(col) {
                            *cell = Cell::default();
                        }
                    }
                }
                2 => {
                    // Erase entire line
                    *line = TermLine::new(cols);
                }
                _ => {}
            }
        }
    }

    // ========================================================================
    // Insert / Delete operations
    // ========================================================================

    fn insert_lines(&mut self, count: usize) {
        let cols = self.cols();
        let row = self.cursor_row;
        if row < self.scroll_top || row > self.scroll_bottom {
            return;
        }
        for _ in 0..count {
            if self.scroll_bottom < self.screen.len() {
                self.screen.remove(self.scroll_bottom);
            }
            if row <= self.screen.len() {
                self.screen.insert(row, TermLine::new(cols));
            }
        }
    }

    fn delete_lines(&mut self, count: usize) {
        let cols = self.cols();
        let row = self.cursor_row;
        if row < self.scroll_top || row > self.scroll_bottom {
            return;
        }
        for _ in 0..count {
            if row < self.screen.len() {
                self.screen.remove(row);
            }
            let insert_pos = self.scroll_bottom.min(self.screen.len());
            self.screen.insert(insert_pos, TermLine::new(cols));
        }
    }

    fn insert_chars(&mut self, count: usize) {
        let cols = self.cols();
        let row = self.cursor_row;
        let col = self.cursor_col;
        if let Some(line) = self.screen.get_mut(row) {
            for _ in 0..count {
                if col < cols {
                    line.cells.insert(col, Cell::default());
                    line.cells.truncate(cols);
                }
            }
        }
    }

    fn delete_chars(&mut self, count: usize) {
        let cols = self.cols();
        let row = self.cursor_row;
        let col = self.cursor_col;
        if let Some(line) = self.screen.get_mut(row) {
            for _ in 0..count {
                if col < line.cells.len() {
                    line.cells.remove(col);
                    line.cells.push(Cell::default());
                }
            }
            line.cells.truncate(cols);
        }
    }

    fn erase_chars(&mut self, count: usize) {
        let cols = self.cols();
        let row = self.cursor_row;
        let col = self.cursor_col;
        if let Some(line) = self.screen.get_mut(row) {
            // The end of the run computed once and clamped, rather than a
            // `col + c` tested against `cols` inside the loop: the addition
            // was the only thing standing between a large `count` -- which
            // arrives as a CSI parameter -- and an overflow.
            let end = col.saturating_add(count).min(cols);
            for target in col..end {
                if let Some(cell) = line.cells.get_mut(target) {
                    *cell = Cell::default();
                }
            }
        }
    }

    // ========================================================================
    // OSC dispatch
    // ========================================================================

    fn dispatch_osc(&mut self) {
        let osc = self.osc_string.clone();
        // OSC format: "Ps ; Pt" where Ps is the command number
        if let Some((cmd_str, text)) = osc.split_once(';') {
            if let Ok(cmd) = cmd_str.parse::<u16>() {
                match cmd {
                    0 | 2 => {
                        // Set window title
                        self.title = text.to_string();
                    }
                    1 => {
                        // Set icon name (we use it as title too)
                        self.title = text.to_string();
                    }
                    _ => {} // Other OSC commands not implemented
                }
            }
        }
    }

    // ========================================================================
    // Resize
    // ========================================================================

    /// Resize the terminal to new dimensions.
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        if new_cols == 0 || new_rows == 0 {
            return;
        }

        self.config.cols = new_cols;
        self.config.rows = new_rows;

        // Resize tab stops
        self.tab_stops.resize(new_cols, false);
        for i in (0..new_cols).step_by(8) {
            if let Some(stop) = self.tab_stops.get_mut(i)
                && !*stop
            {
                *stop = true;
            }
        }

        // Both screens, the hidden one too. While a full-screen program runs,
        // the shell's screen waits in `alt_screen` with its cursor in
        // `saved_cursor_main`, and it was cut from the bottom here with its
        // cursor left where it was -- so a window resized under `vim` lost the
        // prompt it was typed at, and the cursor came back pointing past it.
        fit_cols(&mut self.screen, new_cols);
        fit_cols(&mut self.alt_screen, new_cols);
        let mut history = History {
            lines: &mut self.scrollback,
            owed: &mut self.resized_into_scrollback,
            limit: self.config.scrollback_limit,
            forgotten: 0,
        };
        // Each screen's saved cursor moves with its screen, or `?1049`'s
        // restore would put the shell's cursor back on the row it was on
        // before the window changed height.
        let [saved_main, saved_alt] = &mut self.saved;
        let size = (new_rows, new_cols);
        if self.alt_screen_active {
            let shown = (&mut self.cursor_row, &mut saved_alt.row);
            fit_rows(&mut self.screen, shown, size, None);
            let hidden = (&mut self.saved_cursor_main.0, &mut saved_main.row);
            fit_rows(&mut self.alt_screen, hidden, size, Some(&mut history));
        } else {
            let shown = (&mut self.cursor_row, &mut saved_main.row);
            fit_rows(&mut self.screen, shown, size, Some(&mut history));
            let hidden = (&mut self.saved_cursor_alt.0, &mut saved_alt.row);
            fit_rows(&mut self.alt_screen, hidden, size, None);
        }
        for _ in 0..history.forgotten {
            self.forget_oldest_row();
        }

        // Update scroll region to full screen
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);

        // Clamp cursor
        self.clamp_cursor();
    }

    // ========================================================================
    // Keyboard input translation
    // ========================================================================

    /// Translate a keyboard event into escape sequence bytes to send to the child process.
    ///
    /// Returns the bytes that should be written to the child's stdin.
    pub fn translate_key(&mut self, event: &KeyEvent) -> Vec<u8> {
        if !event.pressed {
            return Vec::new();
        }

        let mods = &event.modifiers;

        // If there is a text character (and no ctrl/alt modifiers), send it as UTF-8
        // The whole run, not its first character: a dead key whose composition
        // failed types two, and sending one would leave the shell reading a
        // line the user never typed.
        if !event.text.is_empty() && !mods.ctrl && !mods.alt {
            return event.text.clone().into_bytes();
        }

        // Ctrl+letter produces control characters (^A = 0x01, ^Z = 0x1A, etc.)
        if mods.ctrl
            && !mods.alt
            && let Some(code) = self.ctrl_key_code(&event.key)
        {
            return vec![code];
        }

        // Alt+key sends ESC prefix
        let prefix = if mods.alt { b"\x1b" as &[u8] } else { &[] };

        let seq: Vec<u8> = match event.key {
            Key::Enter => vec![0x0D],
            Key::Tab => {
                if mods.shift {
                    b"\x1b[Z".to_vec()
                } else {
                    vec![0x09]
                }
            }
            Key::Backspace => vec![0x7F],
            Key::Escape => vec![0x1B],
            Key::Space => {
                if mods.ctrl {
                    vec![0x00] // Ctrl+Space = NUL
                } else {
                    vec![0x20]
                }
            }

            // Arrow keys
            Key::Up => self.arrow_key_seq(b'A', mods),
            Key::Down => self.arrow_key_seq(b'B', mods),
            Key::Right => self.arrow_key_seq(b'C', mods),
            Key::Left => self.arrow_key_seq(b'D', mods),

            // Navigation keys
            Key::Home => self.nav_key_seq(1, mods),
            Key::End => self.nav_key_seq(4, mods),
            Key::Insert => self.nav_key_seq(2, mods),
            Key::Delete => self.nav_key_seq(3, mods),
            Key::PageUp => self.nav_key_seq(5, mods),
            Key::PageDown => self.nav_key_seq(6, mods),

            // Function keys
            Key::F1 => self.func_key_seq(11, mods),
            Key::F2 => self.func_key_seq(12, mods),
            Key::F3 => self.func_key_seq(13, mods),
            Key::F4 => self.func_key_seq(14, mods),
            Key::F5 => self.func_key_seq(15, mods),
            Key::F6 => self.func_key_seq(17, mods),
            Key::F7 => self.func_key_seq(18, mods),
            Key::F8 => self.func_key_seq(19, mods),
            Key::F9 => self.func_key_seq(20, mods),
            Key::F10 => self.func_key_seq(21, mods),
            Key::F11 => self.func_key_seq(23, mods),
            Key::F12 => self.func_key_seq(24, mods),

            _ => Vec::new(),
        };

        if seq.is_empty() {
            return Vec::new();
        }

        let mut result = prefix.to_vec();
        result.extend_from_slice(&seq);
        result
    }

    /// Produce escape sequence for an arrow key.
    fn arrow_key_seq(&self, direction: u8, mods: &Modifiers) -> Vec<u8> {
        let modifier = self.modifier_code(mods);
        if modifier > 1 {
            format!("\x1b[1;{}{}", modifier, direction as char).into_bytes()
        } else if self.app_cursor_keys {
            vec![0x1b, b'O', direction]
        } else {
            vec![0x1b, b'[', direction]
        }
    }

    /// Produce escape sequence for a navigation key (Home, End, Insert, Delete, PgUp, PgDn).
    fn nav_key_seq(&self, code: u8, mods: &Modifiers) -> Vec<u8> {
        let modifier = self.modifier_code(mods);
        if modifier > 1 {
            format!("\x1b[{};{}~", code, modifier).into_bytes()
        } else {
            format!("\x1b[{}~", code).into_bytes()
        }
    }

    /// Produce escape sequence for a function key.
    fn func_key_seq(&self, code: u8, mods: &Modifiers) -> Vec<u8> {
        let modifier = self.modifier_code(mods);
        if modifier > 1 {
            format!("\x1b[{};{}~", code, modifier).into_bytes()
        } else {
            format!("\x1b[{}~", code).into_bytes()
        }
    }

    /// Compute the xterm modifier code from modifier state.
    /// Returns 1 (no modifiers) through 8. Only values > 1 are actually emitted.
    fn modifier_code(&self, mods: &Modifiers) -> u8 {
        // 1 + a bit per modifier, so the three bits never carry past 8 and the
        // additions cannot overflow -- but written saturating anyway, because
        // that is a property of the three constants below rather than of the
        // type, and a fourth modifier would quietly break it.
        let mut code: u8 = 1;
        if mods.shift {
            code = code.saturating_add(1);
        }
        if mods.alt {
            code = code.saturating_add(2);
        }
        if mods.ctrl {
            code = code.saturating_add(4);
        }
        code
    }

    /// Map a Ctrl+key combination to the appropriate control character byte.
    fn ctrl_key_code(&self, key: &Key) -> Option<u8> {
        match key {
            Key::A => Some(0x01),
            Key::B => Some(0x02),
            Key::C => Some(0x03),
            Key::D => Some(0x04),
            Key::E => Some(0x05),
            Key::F => Some(0x06),
            Key::G => Some(0x07),
            Key::H => Some(0x08),
            Key::I => Some(0x09),
            Key::J => Some(0x0A),
            Key::K => Some(0x0B),
            Key::L => Some(0x0C),
            Key::M => Some(0x0D),
            Key::N => Some(0x0E),
            Key::O => Some(0x0F),
            Key::P => Some(0x10),
            Key::Q => Some(0x11),
            Key::R => Some(0x12),
            Key::S => Some(0x13),
            Key::T => Some(0x14),
            Key::U => Some(0x15),
            Key::V => Some(0x16),
            Key::W => Some(0x17),
            Key::X => Some(0x18),
            Key::Y => Some(0x19),
            Key::Z => Some(0x1A),
            Key::LeftBracket => Some(0x1B), // Ctrl+[ = ESC
            Key::Backslash => Some(0x1C),
            Key::RightBracket => Some(0x1D),
            _ => None,
        }
    }

    // ========================================================================
    // Buffer addressing
    // ========================================================================

    /// Which buffer row the top of the window is showing.
    ///
    /// The buffer is the scrollback followed by the screen, addressed as one
    /// run of rows; `scroll_offset` counts backwards from the bottom of it.
    /// Every part of the program that has to relate a *screen* row to a *line*
    /// goes through here, so that the drawing, the selection and the copy all
    /// agree about which line row zero is. They did not: the drawing walked
    /// the scrollback, while the selection and the copy indexed `self.screen`
    /// directly with the same row number, so **selecting a line of scrollback
    /// highlighted it and copied whatever was at that screen row instead**.
    pub fn viewport_top(&self) -> usize {
        self.scrollback.len().saturating_sub(self.scroll_offset)
    }

    /// The buffer row a screen row is showing.
    pub fn buffer_row_of(&self, screen_row: usize) -> usize {
        self.viewport_top().saturating_add(screen_row)
    }

    /// The line at a buffer row, scrollback or screen.
    pub fn line_at(&self, buffer_row: usize) -> Option<&TermLine> {
        let scrollback_len = self.scrollback.len();
        if buffer_row < scrollback_len {
            self.scrollback.get(buffer_row)
        } else {
            self.screen.get(buffer_row.saturating_sub(scrollback_len))
        }
    }

    /// How many rows the buffer holds in total.
    pub fn buffer_len(&self) -> usize {
        self.scrollback.len().saturating_add(self.screen.len())
    }

    /// The layout the last frame was drawn with.
    pub fn layout(&self) -> Layout {
        Layout::solve(
            self.size.0,
            self.size.1,
            self.config.cell_width,
            self.config.cell_height,
        )
    }

    /// The oldest line has fallen off the front of the scrollback: everything
    /// addressed by buffer row now means one row less.
    ///
    /// Buffer rows are an index into a run whose *front* moves, which is the
    /// one thing an index cannot survive on its own. A selection made a
    /// thousand lines ago would otherwise creep down the buffer as output
    /// arrived, and would end up highlighting whatever text happened to reach
    /// those rows -- silently, since every row involved stays in range.
    fn forget_oldest_row(&mut self) {
        if let Some(sel) = self.selection.as_mut() {
            sel.start_row = sel.start_row.saturating_sub(1);
            sel.end_row = sel.end_row.saturating_sub(1);
        }
    }

    // ========================================================================
    // Selection
    // ========================================================================

    /// Start a text selection at the given pixel coordinates.
    pub fn selection_start(&mut self, px: f32, py: f32) {
        let (row, col) = self.pixel_to_cell(px, py);
        self.selection = Some(Selection {
            start_row: row,
            start_col: col,
            end_row: row,
            end_col: col,
            active: true,
        });
    }

    /// Extend the current selection to the given pixel coordinates.
    pub fn selection_extend(&mut self, px: f32, py: f32) {
        let (row, col) = self.pixel_to_cell(px, py);
        if let Some(ref mut sel) = self.selection {
            sel.end_row = row;
            sel.end_col = col;
        }
    }

    /// End the selection (stop dragging).
    pub fn selection_end(&mut self) {
        if let Some(ref mut sel) = self.selection {
            sel.active = false;
        }
    }

    /// Get the selected text as a string.
    pub fn get_selection_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;

        let (start_row, start_col, end_row, end_col) = if sel.start_row < sel.end_row
            || (sel.start_row == sel.end_row && sel.start_col <= sel.end_col)
        {
            (sel.start_row, sel.start_col, sel.end_row, sel.end_col)
        } else {
            (sel.end_row, sel.end_col, sel.start_row, sel.start_col)
        };

        let mut result = String::new();

        for row in start_row..=end_row {
            // `line_at`, not `self.screen[row]`: the rows are buffer rows, and
            // a selection made in the scrollback used to copy the screen line
            // that happened to carry the same number.
            let Some(line) = self.line_at(row) else {
                continue;
            };

            let col_start = if row == start_row { start_col } else { 0 };
            let col_end = if row == end_row {
                end_col.saturating_add(1)
            } else {
                line.cells.len()
            };

            for col in col_start..col_end.min(line.cells.len()) {
                if let Some(cell) = line.cells.get(col) {
                    if cell.continuation {
                        // The second half of a wide character. Its `ch` is a
                        // space; emitting it would put one after every CJK
                        // character in copied text.
                        continue;
                    }
                    result.push(cell.ch);
                }
            }

            if row < end_row {
                result.push('\n');
            }
        }

        // Trim trailing spaces from each line
        let trimmed: String = result
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");

        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    /// Clear the current selection.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Which buffer row and column a point in the window is over.
    ///
    /// The row is a *buffer* row, so a point picks out the line the user can
    /// see at it rather than a screen slot whose contents change the moment
    /// anything scrolls.
    fn pixel_to_cell(&self, px: f32, py: f32) -> (usize, usize) {
        let l = self.layout();
        let screen_row = cells_that_fit(py, l.cell_h);
        let col = cells_that_fit(px, l.cell_w);
        let screen_row = screen_row.min(self.rows().saturating_sub(1));
        let col = col.min(self.cols().saturating_sub(1));
        (self.buffer_row_of(screen_row), col)
    }

    /// Check if a given buffer row and column is within the current selection.
    fn is_selected(&self, row: usize, col: usize) -> bool {
        let sel = match &self.selection {
            Some(s) => s,
            None => return false,
        };

        let (start_row, start_col, end_row, end_col) = if sel.start_row < sel.end_row
            || (sel.start_row == sel.end_row && sel.start_col <= sel.end_col)
        {
            (sel.start_row, sel.start_col, sel.end_row, sel.end_col)
        } else {
            (sel.end_row, sel.end_col, sel.start_row, sel.start_col)
        };

        if row < start_row || row > end_row {
            return false;
        }
        if row == start_row && row == end_row {
            col >= start_col && col <= end_col
        } else if row == start_row {
            col >= start_col
        } else if row == end_row {
            col <= end_col
        } else {
            true
        }
    }

    // ========================================================================
    // Scrollback viewing
    // ========================================================================

    /// Scroll the viewport up into scrollback.
    pub fn scroll_viewport_up(&mut self, lines: usize) {
        let max = self.scrollback.len();
        self.scroll_offset = self.scroll_offset.saturating_add(lines).min(max);
    }

    /// Scroll the viewport down toward current content.
    pub fn scroll_viewport_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Build one frame of the terminal at the given window size.
    ///
    /// `&self`, and everything time-dependent already advanced by `tick`: the
    /// old drawing pass took `&mut self` so it could count the visual bell
    /// down as it painted, which made the length of the flash a function of
    /// how often the window happened to be redrawn.
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::solve(
            width,
            height,
            self.config.cell_width,
            self.config.cell_height,
        );
        let scheme = &self.config.colors;
        let mut f = Frame::new(l.window.w, l.window.h);

        // Everything is held inside the window. A grid wider than the window it
        // is in is the ordinary state between a resize being asked for and the
        // child agreeing to it, and without this the surplus columns are drawn
        // over whatever is beside the terminal.
        f.clip(l.window);

        // The whole window, not the grid: the grid is a whole number of cells
        // and the window is not, so up to one cell of width and one of height
        // is left over. Filling only the grid leaves that strip transparent --
        // a band of desktop showing through the bottom of the terminal.
        fill(&mut f, l.window, scheme.background);

        // The grid is drawn in the face its cells were measured in. Without
        // this scope the compositor fills a fixed-pitch grid with the
        // proportional UI face, which no cell width fits.
        f.push(RenderCommand::PushFont {
            family: FontFamily::Mono,
        });
        self.draw_cells(&mut f, &l);
        self.draw_cursor(&mut f, &l);
        f.push(RenderCommand::PopFont);
        self.draw_bar(&mut f, &l);

        // The bell flashes over everything, including the bar, because it is a
        // property of the terminal rather than of the text.
        if self.bell_flash_ms > 0 {
            fill(&mut f, l.window, Color::rgba(255, 255, 255, 30));
        }

        f.unclip();
        f
    }

    /// The characters, their backgrounds and their rules.
    fn draw_cells(&self, f: &mut Frame<Target>, l: &Layout) {
        let scheme = &self.config.colors;
        let grid = match l.grid.intersect(l.window) {
            Some(r) => r,
            None => return,
        };
        f.hit(Target::Grid, grid);

        // The window's rows, not the buffer's: a window taller than the grid
        // the child agreed to shows blank rows at the bottom rather than
        // reading past the end of the screen.
        let rows = l.rows.min(self.rows());
        let cols = l.cols.min(self.cols());
        for screen_row in 0..rows {
            let Some(line) = self.line_at(self.buffer_row_of(screen_row)) else {
                continue;
            };
            let buffer_row = self.buffer_row_of(screen_row);
            let y = usize_f32(screen_row) * l.cell_h;
            for col in 0..cols.min(line.cells.len()) {
                let Some(cell) = line.cells.get(col) else {
                    continue;
                };
                let x = usize_f32(col) * l.cell_w;
                let selected = self.is_selected(buffer_row, col);
                let (mut fg_color, mut bg_color) = self.resolve_cell_colors(cell, scheme);

                if cell.attrs.inverse {
                    std::mem::swap(&mut fg_color, &mut bg_color);
                }
                if selected {
                    bg_color = scheme.selection_bg;
                }
                if cell.attrs.hidden {
                    fg_color = bg_color;
                }
                if cell.attrs.dim {
                    fg_color =
                        Color::rgba(fg_color.r / 2, fg_color.g / 2, fg_color.b / 2, fg_color.a);
                }

                if bg_color != scheme.background || selected {
                    fill(f, Rect::new(x, y, l.cell_w, l.cell_h), bg_color);
                }

                // Blinking text is not drawn in the dark half of its blink.
                // SGR 5 was recorded and read by nothing, so a program's
                // blinking warning sat still.
                if cell.ch != ' ' && (self.text_blink_on || !cell.attrs.blink) {
                    let font_weight = if cell.attrs.bold {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    };
                    glyph(
                        f,
                        (x, y),
                        cell.ch,
                        fg_color,
                        (self.config.font_size, font_weight),
                        l.cell_w * cells_covered(line, col),
                    );
                }

                if cell.attrs.underline {
                    rule(f, x, x + l.cell_w, y + l.cell_h - 2.0, fg_color, 1.0);
                }
                if cell.attrs.strikethrough {
                    rule(f, x, x + l.cell_w, y + l.cell_h / 2.0, fg_color, 1.0);
                }
            }
        }
    }

    /// The cursor, if it is on screen, visible, and in the lit half of its
    /// blink.
    fn draw_cursor(&self, f: &mut Frame<Target>, l: &Layout) {
        if !self.cursor_visible || self.scroll_offset != 0 || !self.blink_on {
            return;
        }
        // A cursor in a column the window is too narrow to show is not drawn
        // at the edge instead: that would put it on a character it is not on.
        if self.cursor_col >= l.cols || self.cursor_row >= l.rows {
            return;
        }
        let cx = usize_f32(self.cursor_col) * l.cell_w;
        let cy = usize_f32(self.cursor_row) * l.cell_h;
        let scheme = &self.config.colors;
        let cursor_color = scheme.cursor;

        if !self.focused {
            // Where the cursor is, without claiming the keys: an outline in
            // every style, because a bar or an underline drawn thin is
            // already what the focused cursor looks like.
            f.push(RenderCommand::StrokeRect {
                x: cx,
                y: cy,
                width: l.cell_w,
                height: l.cell_h,
                color: cursor_color,
                line_width: 1.0,
                corner_radii: guitk::style::CornerRadii::ZERO,
            });
            return;
        }

        match self.cursor_style {
            CursorStyle::Block => {
                // As wide as the character under it: a block on the first
                // half of a wide character covering only that half would
                // cut the glyph drawn over it in two.
                let line = self.screen.get(self.cursor_row);
                let span = l.cell_w * line.map_or(1.0, |line| cells_covered(line, self.cursor_col));
                fill(
                    f,
                    Rect::new(cx, cy, span, l.cell_h),
                    Color::rgba(cursor_color.r, cursor_color.g, cursor_color.b, 180),
                );
                if let Some(line) = line
                    && let Some(cell) = line.cells.get(self.cursor_col)
                    && cell.ch != ' '
                {
                    glyph(
                        f,
                        (cx, cy),
                        cell.ch,
                        scheme.background,
                        (self.config.font_size, FontWeightHint::Regular),
                        span,
                    );
                }
            }
            CursorStyle::Underline => {
                rule(f, cx, cx + l.cell_w, cy + l.cell_h - 2.0, cursor_color, 2.0);
            }
            CursorStyle::Bar => {
                f.push(RenderCommand::Line {
                    x1: cx,
                    y1: cy,
                    x2: cx,
                    y2: cy + l.cell_h,
                    color: cursor_color,
                    width: 2.0,
                });
            }
        }
    }

    /// The scrollback bar down the right-hand edge.
    ///
    /// The old program had none, and no other sign that there was scrollback at
    /// all: a terminal scrolled a thousand lines back looked exactly like a
    /// terminal sitting at a prompt with a quiet child, and the only way to
    /// find out which it was was to press a key.
    fn draw_bar(&self, f: &mut Frame<Target>, l: &Layout) {
        let Some(bar) = l.bar.intersect(l.window) else {
            return;
        };
        if bar.is_empty() {
            return;
        }
        let scheme = &self.config.colors;
        fill(f, bar, scheme.ansi[0]);
        f.hit(Target::ScrollTrack, bar);

        let total = self.buffer_len();
        let shown = l.rows.min(self.rows());
        if total == 0 || shown == 0 || total <= shown {
            // Nothing has scrolled off yet: the whole buffer is on screen, so a
            // thumb would fill the track and say nothing.
            return;
        }
        let span = ratio(shown, total);
        // A thumb thinner than a couple of pixels cannot be seen or aimed at,
        // so a very long scrollback gets a floor rather than a sliver.
        let thumb_h = (bar.h * span).max(4.0).min(bar.h);
        let travel = (bar.h - thumb_h).max(0.0);
        let top_row = self.viewport_top();
        let last_top = total.saturating_sub(shown);
        let progress = if last_top == 0 {
            0.0
        } else {
            ratio(top_row.min(last_top), last_top)
        };
        let thumb = Rect::new(bar.x, bar.y + travel * progress, bar.w, thumb_h);
        fill(f, thumb, scheme.foreground);
        f.hit(Target::ScrollThumb, thumb);
    }

    /// Resolve a cell's foreground and background to actual Color values.
    fn resolve_cell_colors(&self, cell: &Cell, scheme: &ColorScheme) -> (Color, Color) {
        let fg = self.resolve_term_color(cell.attrs.fg, true, cell.attrs.bold, scheme);
        let bg = self.resolve_term_color(cell.attrs.bg, false, false, scheme);
        (fg, bg)
    }

    /// Convert a TermColor to an actual Color value.
    fn resolve_term_color(
        &self,
        color: TermColor,
        is_foreground: bool,
        bold: bool,
        scheme: &ColorScheme,
    ) -> Color {
        match color {
            TermColor::Default => {
                if is_foreground {
                    scheme.foreground
                } else {
                    scheme.background
                }
            }
            TermColor::Indexed(idx) => {
                match idx {
                    0..=7 => {
                        // If bold and this is a foreground color, use bright variant
                        let effective_idx = if bold && is_foreground {
                            idx.saturating_add(8)
                        } else {
                            idx
                        };
                        ansi_entry(scheme, effective_idx)
                    }
                    8..=15 => ansi_entry(scheme, idx),
                    16..=231 => {
                        // 6x6x6 color cube
                        let idx = idx.saturating_sub(16);
                        Color::rgb(
                            cube_level(idx / 36),
                            cube_level((idx / 6) % 6),
                            cube_level(idx % 6),
                        )
                    }
                    232..=255 => {
                        // Grayscale ramp (24 shades)
                        let shade = idx.saturating_sub(232).saturating_mul(10).saturating_add(8);
                        Color::rgb(shade, shade, shade)
                    }
                }
            }
            TermColor::Rgb(r, g, b) => Color::rgb(r, g, b),
        }
    }

    // ========================================================================
    // Event handling
    // ========================================================================

    /// Handle a guitk event. Returns the bytes the key translated to, if any.
    ///
    /// Every arm ends at the same flush, because a reply the *parser* produced
    /// -- while feeding the child's own output back through it -- has to leave
    /// by the same route a keystroke does, and there is no event that is
    /// guaranteed to follow it.
    pub fn handle_event(&mut self, event: &Event) -> Vec<u8> {
        let out = self.dispatch_event(event);
        self.flush_to_child();
        out
    }

    fn dispatch_event(&mut self, event: &Event) -> Vec<u8> {
        match event {
            Event::Key(key_event) => {
                // Typing scrolls back to where the typing will appear. A key
                // that goes to the child while the user is looking at the
                // scrollback otherwise has no visible effect at all.
                self.scroll_offset = 0;
                self.wake_cursor();
                let bytes = self.translate_key(key_event);
                self.to_child(&bytes);
                bytes
            }
            Event::Mouse(mouse_event) => {
                self.handle_mouse(mouse_event);
                Vec::new()
            }
            Event::Resize { width, height } => {
                self.resize_to_window(u32_f32(*width), u32_f32(*height));
                Vec::new()
            }
            Event::FocusIn => {
                self.set_focused(true);
                Vec::new()
            }
            Event::FocusOut => {
                self.set_focused(false);
                Vec::new()
            }
            Event::Tick { elapsed_ms, .. } => {
                self.on_tick(*elapsed_ms);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Fit the grid to a window of this size.
    ///
    /// The whole reason `resize` existed: it had no caller outside the tests,
    /// so the grid stayed at the eighty-by-twenty-four the config was born
    /// with however the window was dragged, and a maximised terminal drew a
    /// small rectangle of text in the corner of a large empty window.
    pub fn resize_to_window(&mut self, width: f32, height: f32) {
        self.size = (width, height);
        let l = self.layout();
        if l.cols > 0 && l.rows > 0 && (l.cols != self.cols() || l.rows != self.rows()) {
            self.resize(l.cols, l.rows);
            // The child is told too -- `TIOCSWINSZ`, which raises `SIGWINCH`
            // for it. Without this a shell under this terminal keeps wrapping
            // its prompt at eighty columns in a window twice that wide.
            let size = self.win_size();
            if let Some(link) = self.child.as_mut() {
                link.resize(size);
            }
        }
        self.clamp_scroll();
    }

    /// Put the scrollback offset back in range.
    ///
    /// Anything that changes how much scrollback there is, or how many rows
    /// are on screen, can leave the offset pointing above the top of the
    /// buffer.
    fn clamp_scroll(&mut self) {
        self.scroll_offset = self.scroll_offset.min(self.scrollback.len());
    }

    /// Handle mouse events (selection and scroll).
    fn handle_mouse(&mut self, event: &MouseEvent) {
        let l = self.layout();
        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                if l.bar.contains(event.x, event.y) {
                    self.press_bar(&l, event.y);
                    return;
                }
                self.clear_selection();
                self.selection_start(event.x, event.y);
            }
            MouseEventKind::Move => {
                if let Some(ref sel) = self.selection
                    && sel.active
                {
                    self.selection_extend(event.x, event.y);
                }
            }
            MouseEventKind::Release(MouseButton::Left) => {
                self.selection_end();
            }
            MouseEventKind::Scroll { dy, .. } => {
                // `scroll_offset` runs *backwards* here -- it counts lines up
                // into the scrollback, where every other list in the tree
                // counts rows down from the top. So the accumulator's "towards
                // the end" is this view's "towards the bottom", and the two
                // arms are swapped relative to a list. Spelling that out
                // because the obvious transcription is silently inverted.
                let rows = self.wheel.rows(*dy);
                if rows < 0 {
                    self.scroll_viewport_up(rows.unsigned_abs());
                } else {
                    self.scroll_viewport_down(rows.unsigned_abs());
                }
            }
            _ => {}
        }
    }

    /// A press in the scrollback bar: a page towards wherever it landed.
    fn press_bar(&mut self, l: &Layout, py: f32) {
        let page = l.rows.min(self.rows()).max(1);
        let total = self.buffer_len();
        let shown = l.rows.min(self.rows());
        if total <= shown || l.bar.h <= 0.0 {
            return;
        }
        // Which buffer row the press is level with, read off the track.
        let last_top = total.saturating_sub(shown);
        let fraction = ((py - l.bar.y) / l.bar.h).clamp(0.0, 1.0);
        let aimed_top = scale(last_top, fraction);
        let current_top = self.viewport_top();
        if aimed_top < current_top {
            self.scroll_viewport_up(page);
        } else if aimed_top > current_top {
            self.scroll_viewport_down(page);
        }
    }
}

/// A cursor saved by DECSC -- `ESC 7`, `CSI s`, and on the way into
/// `CSI ? 1049 h`.
#[derive(Clone, Copy, Debug, Default)]
struct SavedCursor {
    row: usize,
    col: usize,
    attrs: CellAttrs,
}

/// The scrollback, as a resize of the main screen sees it.
struct History<'a> {
    /// The lines themselves, oldest first.
    lines: &'a mut VecDeque<TermLine>,
    /// How many at the back a shrink put there (`resized_into_scrollback`).
    owed: &'a mut usize,
    /// The most lines kept.
    limit: usize,
    /// How many fell off the front, for the caller to account for: every
    /// buffer row after them now means one row less.
    forgotten: usize,
}

/// Fit every line to `cols` columns.
///
/// Narrowing can cut a double-width character in half: the continuation is
/// truncated away and the lead is left at the last column, still drawing as a
/// wide glyph in a cell that is now the edge of the screen. A lead is only
/// recognisable by asking the width table what its character is, because
/// `continuation` marks the *second* half and the second half is the one that
/// just vanished.
///
/// This is not reflow. Lines are truncated and padded rather than rewrapped,
/// so no line's text moves between rows and there is no pair to split
/// anywhere else.
fn fit_cols(lines: &mut [TermLine], cols: usize) {
    for line in lines {
        line.resize(cols);
        if let Some(last) = cols.checked_sub(1)
            && let Some(cell) = line.cells.get_mut(last)
            && charwidth::char_width(cell.ch) == Some(2)
        {
            cell.ch = ' ';
            cell.continuation = false;
        }
    }
}

/// Fit a screen to `rows` rows around its cursor, by tmux's rule.
///
/// Shrinking takes the rows below the cursor first -- at a shell prompt, the
/// blank ones the next output will go into -- and only what still does not
/// fit leaves from the top, into `history` when the screen has one. Taking
/// every row from the top, as this used to, moved a prompt at the top of the
/// screen into history: split a multiplexer pane in two and the half that
/// kept the shell showed none of it. Growing gives back what shrinking took
/// and adds blank rows for the rest.
///
/// `history` is `None` for the alternate screen, which has no scrollback:
/// what leaves its top is gone, and it grows with blank rows.
///
/// `(cursor_row, saved_row)` are the screen's cursor and its DECSC saved
/// cursor. The rows below the *cursor* are the ones taken first; both move
/// by however far the lines they are on moved.
fn fit_rows(
    lines: &mut Vec<TermLine>,
    (cursor_row, saved_row): (&mut usize, &mut usize),
    (rows, cols): (usize, usize),
    mut history: Option<&mut History<'_>>,
) {
    let old = lines.len();
    if rows < old {
        let excess = old.saturating_sub(rows);
        let below = old.saturating_sub(cursor_row.saturating_add(1));
        lines.truncate(old.saturating_sub(excess.min(below)));
        for _ in 0..excess.saturating_sub(below) {
            if lines.is_empty() {
                break;
            }
            let line = lines.remove(0);
            *cursor_row = cursor_row.saturating_sub(1);
            *saved_row = saved_row.saturating_sub(1);
            if let Some(h) = history.as_deref_mut() {
                h.lines.push_back(line);
                *h.owed = h.owed.saturating_add(1);
                if h.lines.len() > h.limit {
                    h.lines.pop_front();
                    h.forgotten = h.forgotten.saturating_add(1);
                }
                *h.owed = (*h.owed).min(h.lines.len());
            }
        }
    } else {
        for _ in old..rows {
            let pulled = history.as_deref_mut().and_then(|h| {
                if (*h.owed).min(h.lines.len()) == 0 {
                    return None;
                }
                let line = h.lines.pop_back()?;
                *h.owed = h.owed.saturating_sub(1);
                Some(line)
            });
            match pulled {
                Some(mut line) => {
                    line.resize(cols);
                    lines.insert(0, line);
                    *cursor_row = cursor_row.saturating_add(1);
                    *saved_row = saved_row.saturating_add(1);
                }
                None => lines.push(TermLine::new(cols)),
            }
        }
    }
    lines.resize_with(rows, || TermLine::new(cols));
}

// ============================================================================
// Drawing helpers
// ============================================================================

/// One filled rectangle.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: guitk::style::CornerRadii::ZERO,
    });
}

/// One character, bounded to its own cell.
///
/// `span` is the width of the cells the character covers -- one, or two for
/// a wide character -- and the glyph is clipped to it. It was clipped to a
/// `W` measured in the proportional face instead, which is no cell's width.
fn glyph(
    f: &mut Frame<Target>,
    (x, y): (f32, f32),
    ch: char,
    color: Color,
    (font_size, font_weight): (f32, FontWeightHint),
    span: f32,
) {
    let mut text = String::new();
    text.push(ch);
    f.push(RenderCommand::Text {
        x,
        y,
        text,
        color,
        font_size,
        font_weight,
        max_width: Some(span),
        overflow: TextOverflow::Clip,
    });
}

/// How many cells the character at `col` covers: two when the next cell is
/// its continuation, one otherwise.
fn cells_covered(line: &TermLine, col: usize) -> f32 {
    let next_is_continuation = col
        .checked_add(1)
        .and_then(|next| line.cells.get(next))
        .is_some_and(|c| c.continuation);
    if next_is_continuation { 2.0 } else { 1.0 }
}

/// A horizontal rule.
fn rule(f: &mut Frame<Target>, x1: f32, x2: f32, y: f32, color: Color, width: f32) {
    if x2 <= x1 {
        return;
    }
    f.push(RenderCommand::Line {
        x1,
        y1: y,
        x2,
        y2: y,
        color,
        width,
    });
}

/// `part / whole` as a fraction, without a cast at the call site.
fn ratio(part: usize, whole: usize) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    (usize_f32(part) / usize_f32(whole)).clamp(0.0, 1.0)
}

/// `whole * fraction`, rounded down, as a count.
///
/// The rounding is a floor rather than a truncating cast: `fraction` is
/// already clamped to `[0, 1]` by every caller, so the product is in range,
/// and counting up to it avoids the cast entirely.
fn scale(whole: usize, fraction: f32) -> usize {
    if !fraction.is_finite() || fraction <= 0.0 {
        return 0;
    }
    if fraction >= 1.0 {
        return whole;
    }
    let target = usize_f32(whole) * fraction;
    let mut n = 0_usize;
    while n < whole && usize_f32(n.saturating_add(1)) <= target {
        n = n.saturating_add(1);
    }
    n
}

// ============================================================================
// The window
// ============================================================================

impl App for TerminalState {
    /// Adopt the user's colours (§822).
    ///
    /// Only the chrome moves; `ColorScheme::from_palette` says why the ANSI
    /// table does not.
    fn theme_changed(&mut self, palette: &Palette) {
        self.config.colors = ColorScheme::from_palette(palette);
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn app_id(&self) -> String {
        String::from("terminal")
    }

    /// Yes: the shell writes on a thread of its own, and a terminal that is
    /// woken for it sleeps at a prompt instead of looking twenty times a
    /// second.
    fn wants_waker(&self) -> bool {
        true
    }

    fn attach_waker(&mut self, waker: Waker) {
        if let Some(link) = self.child.as_mut() {
            self.link_wakes = link.set_waker(waker.clone());
        }
        self.waker = Some(waker);
    }

    /// The link woke the terminal: read what the child wrote, and anything
    /// the reading made the terminal want to say back.
    fn on_wake(&mut self) -> Response {
        let changed = self.drain_child();
        self.flush_to_child();
        if self.close_requested {
            Response::Exit
        } else if changed {
            Response::Redraw
        } else {
            Response::Idle
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        // The grid the config was born with, in the cells it is drawn in:
        // eighty by twenty-four of the fixed-pitch face, plus the bar.
        let w = usize_f32(self.config.cols) * self.config.cell_width + BAR_W;
        let h = usize_f32(self.config.rows) * self.config.cell_height;
        (pixels_u16(w.ceil()).into(), pixels_u16(h.ceil()).into())
    }

    fn tick_interval(&self) -> Option<std::time::Duration> {
        // Asked for only while something is actually moving. A terminal at a
        // prompt with a solid cursor has nothing to age, and a program that
        // asks for a clock it does not need holds the whole desktop awake.
        let blinking = self.config.cursor_blink && self.cursor_visible && self.focused;
        let aging = (blinking || self.bell_flash_ms > 0 || self.shows_blinking_text())
            .then_some(BLINK_MS / 5);
        // A live child is something moving too: its output reaches the screen
        // only on a tick. Fast while the two are talking, slower once they are
        // not -- see `ACTIVE_POLL_MS`.
        //
        // Only a link that cannot wake the terminal is asked on a clock --
        // and one that can, while it has output left over from a read that
        // hit its limit.
        let polling = (self.child_is_live() && (!self.link_wakes || self.backlog)).then_some(
            // A backlog is drained as fast as a busy child is read. A link
            // that cannot wake is asked quickly while the two are talking and
            // slowly once they are quiet.
            if self.backlog || self.quiet_ms < ACTIVE_WINDOW_MS {
                ACTIVE_POLL_MS
            } else {
                IDLE_POLL_MS
            },
        );
        let ms = match (aging, polling) {
            (Some(a), Some(p)) => Some(a.min(p)),
            (a, p) => a.or(p),
        };
        ms.map(std::time::Duration::from_millis)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            // The shell is told its terminal is gone rather than left running
            // against a master nobody reads.
            self.hang_up();
            return Response::Exit;
        }
        // Every event goes through the same dispatch, the tick included: the
        // tick is what reads the child, and answering it here instead of
        // passing it on left the terminal write-only. The bytes a key
        // translated to are not queued again here either -- `handle_event`
        // already put them in `output_buffer` on their way to the child, and
        // appending the return value as well sent every keystroke twice.
        self.handle_event(event);
        // The shell exited cleanly -- the user typed `exit` -- so the window
        // goes with it.
        if self.close_requested {
            return Response::Exit;
        }
        // A tick that changed nothing must not ask for a frame: the cursor
        // blinks five times a second and the loop runs twenty-five, so four
        // ticks in five have nothing to show.
        if matches!(event, Event::Tick { .. }) && !self.tick_changed {
            return Response::Idle;
        }
        Response::Redraw
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The grid follows the window here as well as on the resize event: a
        // frame is the one thing an application is guaranteed to be asked for,
        // and a grid that disagrees with the window it is drawn in is a grid
        // that shows the wrong number of columns to the program running under
        // it.
        self.resize_to_window(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for TerminalState {
    type Target = Target;
    type Outcome = ();
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) {
        self.resize_to_window(size.0, size.1);
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }));
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) {
        self.resize_to_window(size.0, size.1);
        self.handle_event(&Event::Key(key.clone()));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::ColorScheme;
    use super::child::Exit;
    use super::child::script::{Script, ScriptLink};
    use super::{
        ACTIVE_POLL_MS, ACTIVE_WINDOW_MS, BAR_W, BELL_MS, BLINK_MS, Color, CursorStyle, FONT_SIZE,
        IDLE_POLL_MS, Layout, MAX_READ_PER_DRAIN, Rect, RenderCommand, Target, TerminalConfig,
        TerminalState, cells_that_fit, ratio, scale, u32_f32,
    };
    use guitk::render::{FontFamily, FontWeightHint};
    use guitk::text;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// The default cell, which the fixed-pitch face decides.
    fn cell() -> (f32, f32) {
        let config = TerminalConfig::default();
        (config.cell_width, config.cell_height)
    }

    impl TerminalState {
        /// This terminal, attached to a scripted child.
        fn with_script(mut self) -> (Self, Rc<RefCell<Script>>) {
            let (link, script) = ScriptLink::new();
            self.attach(Box::new(link));
            (self, script)
        }
    }
    use appearance::Palette;
    use guitk::event::{Event, Key, MouseButton, MouseEvent, MouseEventKind};
    use guitk::probe::{Probe, press, rect_of_sized, typing};
    use oswindow::app::{App, Response};

    /// The terminal's furniture follows the theme; its ANSI table does not.
    ///
    /// Both halves are asserted because both are decisions. A terminal that
    /// stays dark on a light desktop is the defect; a terminal whose red is
    /// not red is a worse one, since a program that prints colour 1 expects
    /// red on every terminal ever made.
    /// **A double-width character occupies two cells.**
    ///
    /// `put_char` advanced by exactly one column for every character, so the
    /// 182,712 codepoints `charwidth` calls wide got one cell: `ls` reserves
    /// two columns for a Chinese character, the terminal drew it in one, and
    /// every column after it on the line was off by one.
    #[test]
    fn a_wide_character_takes_two_columns() {
        let mut t = TerminalState::new(TerminalConfig::default());
        let before = t.cursor_col;
        t.put_char('\u{4E2D}'); // CJK, two cells

        assert_eq!(
            t.cursor_col,
            before + 2,
            "a wide character advanced the cursor like a narrow one"
        );
        let line = &t.screen[t.cursor_row];
        assert_eq!(line.cells[before].ch, '\u{4E2D}');
        assert!(
            line.cells[before + 1].continuation,
            "the second cell was left for the next character to paint over"
        );
    }

    /// **A combining mark takes no cell and does not replace what it follows.**
    #[test]
    fn a_combining_mark_takes_no_column() {
        let mut t = TerminalState::new(TerminalConfig::default());
        t.put_char('e');
        let after_e = t.cursor_col;
        t.put_char('\u{0301}'); // combining acute

        assert_eq!(t.cursor_col, after_e, "a zero-width mark took a column");
        assert_eq!(
            t.screen[t.cursor_row].cells[after_e - 1].ch,
            'e',
            "the mark replaced the letter it belongs to"
        );
    }

    /// **Copied text does not gain a space after every wide character.**
    ///
    /// The continuation cell holds a space so the renderer skips it unchanged;
    /// the flag is what the text path reads.
    #[test]
    fn copying_a_wide_character_yields_one_character() {
        let mut t = TerminalState::new(TerminalConfig::default());
        for ch in "\u{4E2D}\u{6587}".chars() {
            t.put_char(ch);
        }
        t.selection_start(0.0, 0.0);
        t.selection_extend(cell().0 * 100.0, 0.0);
        let copied = t.get_selection_text().expect("something was selected");
        assert!(
            copied.starts_with("\u{4E2D}\u{6587}"),
            "copied text was {copied:?}"
        );
    }

    /// **Overwriting half of a wide character does not leave the other half.**
    ///
    /// Both directions. Writing onto the second half would orphan the lead,
    /// which keeps drawing as a wide glyph over a cell that now holds
    /// something else; writing onto the first half would orphan the
    /// continuation, which swallows the character to its right by drawing
    /// nothing where it should be.
    #[test]
    fn overwriting_half_a_wide_character_clears_the_other_half() {
        let mut t = TerminalState::new(TerminalConfig::default());
        t.put_char('\u{4E2D}');

        // Back onto the lead and write a narrow character over it.
        t.cursor_col = 0;
        t.put_char('x');

        let line = &t.screen[t.cursor_row];
        assert_eq!(line.cells[0].ch, 'x');
        assert!(
            !line.cells[1].continuation,
            "the continuation outlived the character it belonged to"
        );
        assert_eq!(
            line.cells[1].ch, ' ',
            "the orphaned half still shows something"
        );
    }

    /// Writing onto the *second* half clears the lead, so no wide glyph is
    /// left painting over a cell it no longer owns.
    #[test]
    fn writing_onto_a_continuation_clears_its_lead() {
        let mut t = TerminalState::new(TerminalConfig::default());
        t.put_char('\u{4E2D}');

        t.cursor_col = 1;
        t.put_char('x');

        let line = &t.screen[t.cursor_row];
        assert_eq!(line.cells[1].ch, 'x');
        assert_eq!(
            line.cells[0].ch, ' ',
            "the lead of a broken pair is still drawn as a wide character"
        );
    }

    /// **Backspace over a wide character moves past the whole of it.**
    ///
    /// Landing on the second half leaves the cursor inside one character,
    /// which no later operation has a sensible reading of.
    #[test]
    fn backspace_steps_over_a_whole_wide_character() {
        let mut t = TerminalState::new(TerminalConfig::default());
        t.put_char('\u{4E2D}');
        assert_eq!(t.cursor_col, 2);

        t.feed(b"\x08");

        assert_eq!(
            t.cursor_col, 0,
            "backspace left the cursor inside the wide character"
        );
    }

    /// A wide character that will not fit at the margin wraps rather than
    /// being split across it, which a grid of cells cannot represent.
    #[test]
    fn a_wide_character_wraps_rather_than_straddling_the_margin() {
        let mut t = TerminalState::new(TerminalConfig {
            cols: 10,
            rows: 5,
            ..TerminalConfig::default()
        });
        for _ in 0..9 {
            t.put_char('a');
        }
        assert_eq!(t.cursor_col, 9, "the fixture is not at the last column");
        let row = t.cursor_row;

        t.put_char('\u{4E2D}');

        assert_eq!(t.cursor_row, row + 1, "the wide character did not wrap");
        assert_eq!(t.screen[row + 1].cells[0].ch, '\u{4E2D}');
        assert!(t.screen[row + 1].cells[1].continuation);
    }
    #[test]
    fn narrowing_the_window_cannot_leave_half_a_wide_character() {
        // Resize truncates rather than reflowing, so the only way a pair can be
        // split by something that is not a write is a narrowing that lands
        // exactly on the seam: the continuation falls off the end and the lead
        // stays, drawing two columns wide in a one-column space.
        let mut t = TerminalState::new(TerminalConfig {
            cols: 10,
            rows: 3,
            ..TerminalConfig::default()
        });
        t.put_char('a');
        t.put_char('\u{4E2D}');
        let row = t.cursor_row;
        assert_eq!(t.screen[row].cells[1].ch, '\u{4E2D}', "fixture");
        assert!(t.screen[row].cells[2].continuation, "fixture");

        // 2 columns keeps the lead at index 1 and drops its continuation.
        t.resize(2, 3);

        assert_ne!(
            t.screen[row].cells[1].ch, '\u{4E2D}',
            "the lead outlived the continuation it needed"
        );
        assert!(
            t.screen[row]
                .cells
                .iter()
                .all(|c| charwidth::char_width(c.ch) != Some(2)),
            "a double-width character is still in a line narrowed onto its seam"
        );
    }

    #[test]
    fn the_chrome_follows_the_theme_and_the_ansi_table_does_not() {
        let dark = ColorScheme::from_palette(&Palette::for_mode(false));
        let light = ColorScheme::from_palette(&Palette::for_mode(true));

        assert_ne!(
            dark.background, light.background,
            "the window's own background must change with the theme"
        );
        assert_ne!(dark.foreground, light.foreground);
        assert_eq!(
            dark.ansi, light.ansi,
            "the sixteen ANSI colours are the protocol's, not the theme's"
        );
        assert_eq!(
            dark.ansi,
            ColorScheme::default().ansi,
            "and they are the same ones a default scheme has"
        );

        let p = Palette::for_mode(true);
        assert_eq!(light.background, p.base);
        assert_eq!(light.foreground, p.text);
        assert_eq!(
            light.selection_bg, p.accent,
            "selection is the accent (839)"
        );
    }

    /// A terminal with `lines` lines already pushed off the top, so there is
    /// something to scroll back into.
    fn scrolled_terminal(lines: usize) -> TerminalState {
        let mut term = TerminalState::new(TerminalConfig {
            rows: 4,
            ..TerminalConfig::default()
        });
        for i in 0..lines {
            term.feed(format!("line {i}\r\n").as_bytes());
        }
        term
    }

    fn wheel(term: &mut TerminalState, dy: f32) {
        term.handle_mouse(&MouseEvent {
            x: 10.0,
            y: 10.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy },
        });
    }

    /// One detent moves three lines, and `dy` positive -- away from the user --
    /// goes *back* into the scrollback. The offset here counts upwards, which
    /// is the opposite of every list in the tree, so the direction is worth
    /// pinning rather than assuming.
    #[test]
    fn one_wheel_notch_moves_three_lines_of_scrollback() {
        let mut term = scrolled_terminal(50);
        wheel(&mut term, 1.0);
        assert_eq!(term.scroll_offset, 3, "away from the user scrolls back");
        wheel(&mut term, -1.0);
        assert_eq!(term.scroll_offset, 0, "and towards the user returns");
    }

    /// Scrolling forward from the live view stays there rather than
    /// underflowing into the far end of the scrollback.
    #[test]
    fn scrolling_forward_from_the_bottom_stays_at_the_bottom() {
        let mut term = scrolled_terminal(50);
        for _ in 0..5 {
            wheel(&mut term, -1.0);
        }
        assert_eq!(term.scroll_offset, 0);
    }

    /// A precision device sends fractions of a notch. Five fifths must be one
    /// notch -- not five (which is what reading only the sign gave) and not
    /// zero (which is what rounding each event alone would give).
    #[test]
    fn a_trackpads_fractions_add_up_to_one_notch() {
        let mut term = scrolled_terminal(50);
        for _ in 0..5 {
            wheel(&mut term, 0.2);
        }
        assert_eq!(term.scroll_offset, 3);
    }

    /// Input arrives from outside the process; a NaN that reached the residue
    /// would stop the terminal scrolling for the life of the session.
    #[test]
    fn a_nonfinite_delta_does_not_break_later_scrolling() {
        let mut term = scrolled_terminal(50);
        wheel(&mut term, f32::NAN);
        assert_eq!(term.scroll_offset, 0);
        wheel(&mut term, 1.0);
        assert_eq!(term.scroll_offset, 3);
    }

    // =======================================================================
    // The emulator in a window
    // =======================================================================
    //
    // Everything below is about the terminal being a window rather than a
    // simulation: that the grid is a quotient of the size the compositor hands
    // it, that what the pointer reaches is what the painter painted, that the
    // child on the other end of the PTY is really written to and really read
    // back, and that the bell and the blink age on a clock rather than on how
    // often the window happens to be redrawn.

    /// The window widths every layout claim is checked at.
    ///
    /// A rule about `Layout::solve` is a rule at *every* size, so a handful of
    /// sampled sizes tests a handful of points and nothing else. The sizes
    /// that break a grid rule are the ones nobody would think to sample: 0 and
    /// 3 wide, where not one column fits; 10 wide, exactly the bar's width, so
    /// the quarter-cap is what keeps any grid at all; 8.4 and 16.8, one and
    /// two cells with no room for the bar beside them.
    const GRID_W: [f32; 8] = [0.0, 3.0, 8.4, 10.0, 16.8, 120.0, 682.0, 1600.0];
    /// The window heights every layout claim is checked at.
    const GRID_H: [f32; 6] = [0.0, 5.0, 18.0, 54.0, 432.0, 1000.0];

    /// Every window size the layout claims sweep.
    fn sizes() -> impl Iterator<Item = (f32, f32)> {
        GRID_W.into_iter().flat_map(|w| GRID_H.map(move |h| (w, h)))
    }

    /// Is `inner` within `outer`, allowing for a pixel of rounding?
    ///
    /// A rectangle with no area is "inside" anything: it is the answer the
    /// drawing pass gives for a bar in a window too small to hold one, and a
    /// bar that was not drawn cannot hang off an edge.
    fn inside(outer: Rect, inner: Rect) -> bool {
        inner.is_empty()
            || (inner.x >= outer.x - 0.01
                && inner.y >= outer.y - 0.01
                && inner.right() <= outer.right() + 0.01
                && inner.bottom() <= outer.bottom() + 0.01)
    }

    /// A terminal with `lines` lines of output already behind it.
    fn fed_terminal(lines: usize) -> TerminalState {
        let mut term = TerminalState::new(TerminalConfig::default());
        for i in 0..lines {
            term.feed(format!("line {i}\r\n").as_bytes());
        }
        term
    }

    /// A terminal scrolled back into its scrollback.
    fn scrolled_back(lines: usize, up: usize) -> TerminalState {
        let mut term = fed_terminal(lines);
        term.scroll_viewport_up(up);
        term
    }

    /// Every state the picture is drawn in.
    ///
    /// The states are the ones that change *what* is painted rather than only
    /// its colour: an empty terminal paints no glyphs at all, a scrolled one
    /// paints a thumb partway down its track and no cursor, an alt-screen one
    /// paints no thumb however much scrollback is behind it.
    fn states() -> Vec<(&'static str, TerminalState)> {
        let mut selected = fed_terminal(40);
        selected.selection_start(0.0, 0.0);
        selected.selection_extend(400.0, 40.0);

        let mut belling = fed_terminal(3);
        belling.feed(b"\x07");

        let mut alt = fed_terminal(40);
        alt.feed(b"\x1b[?1049h");
        alt.feed(b"a full-screen program");

        let mut attrs = TerminalState::new(TerminalConfig::default());
        attrs.feed(b"\x1b[1;4;7;9;3;2mattributes\x1b[0m\r\n");
        attrs.feed(b"\x1b[38;5;208m256\x1b[48;2;10;20;30m truecolor\x1b[0m\r\n");

        let mut barred = fed_terminal(3);
        barred.cursor_style = CursorStyle::Bar;

        let mut underlined = fed_terminal(3);
        underlined.cursor_style = CursorStyle::Underline;

        let mut dark = fed_terminal(3);
        dark.blink_on = false;

        let mut wide = TerminalState::new(TerminalConfig::default());
        wide.feed(
            "the quick brown fox jumps over the lazy dog "
                .repeat(6)
                .as_bytes(),
        );

        vec![
            ("empty", TerminalState::new(TerminalConfig::default())),
            ("fed", fed_terminal(3)),
            ("scrollback", fed_terminal(400)),
            ("scrolled to the top", scrolled_back(400, 10_000)),
            ("scrolled partway", scrolled_back(400, 40)),
            ("selected", selected),
            ("belling", belling),
            ("alt screen", alt),
            ("attributes", attrs),
            ("bar cursor", barred),
            ("underline cursor", underlined),
            ("dark half of the blink", dark),
            ("a line longer than the grid", wide),
        ]
    }

    #[test]
    fn the_grid_and_the_bar_stay_inside_the_window() {
        // The old program painted `80 * 8.4` by `24 * 18` pixels into whatever
        // window it was given, so every window narrower than 682 or shorter
        // than 432 had text drawn past its edge.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h, 8.4, 18.0);
            let at = format!("{w}x{h}");
            for (name, r) in [("grid", l.grid), ("bar", l.bar)] {
                assert!(inside(l.window, r), "{name} escapes the window at {at}");
                assert!(r.w >= -0.01 && r.h >= -0.01, "{name} is negative at {at}");
            }
        }
    }

    #[test]
    fn the_grid_is_a_whole_number_of_cells_and_never_reaches_the_bar() {
        // The two halves of the same claim: the grid is `cols` by `rows` whole
        // cells, and those cells stop where the bar starts. A grid computed
        // from the window width rather than from the width left over would
        // paint its last column underneath the bar.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h, 8.4, 18.0);
            let at = format!("{w}x{h}");
            assert!(
                (l.grid.w - super::usize_f32(l.cols) * 8.4).abs() < 0.01,
                "grid width {} is not {} whole cells at {at}",
                l.grid.w,
                l.cols
            );
            assert!(
                (l.grid.h - super::usize_f32(l.rows) * 18.0).abs() < 0.01,
                "grid height {} is not {} whole rows at {at}",
                l.grid.h,
                l.rows
            );
            assert!(
                l.grid.right() <= l.bar.x + 0.01,
                "the grid reaches {} and the bar starts at {} at {at}",
                l.grid.right(),
                l.bar.x
            );
        }
    }

    #[test]
    fn the_bar_never_takes_more_than_a_quarter_of_the_window() {
        // Reserved unconditionally, so a window narrower than the bar itself
        // would otherwise be entirely furniture and no terminal at all.
        for (w, h) in sizes() {
            let l = Layout::solve(w, h, 8.4, 18.0);
            assert!(
                l.bar.w <= (w / 4.0) + 0.01 && l.bar.w <= BAR_W + 0.01,
                "the bar is {} wide in a {w}x{h} window",
                l.bar.w
            );
            assert!(
                l.bar.right() <= w + 0.01 && (l.bar.right() - w).abs() < 0.01,
                "the bar is not against the right edge of a {w}x{h} window"
            );
        }
    }

    #[test]
    fn the_grid_grows_and_shrinks_with_the_window() {
        // Monotonicity, which is the property that actually says the grid is a
        // quotient of the window: a wider window never has fewer columns.
        let mut last_cols = 0;
        for w in [0.0_f32, 20.0, 100.0, 300.0, 682.0, 1600.0] {
            let l = Layout::solve(w, 400.0, 8.4, 18.0);
            assert!(
                l.cols >= last_cols,
                "{w} wide has {} columns, less than the {last_cols} of a narrower window",
                l.cols
            );
            last_cols = l.cols;
        }
        let mut last_rows = 0;
        for h in [0.0_f32, 10.0, 54.0, 432.0, 1000.0] {
            let l = Layout::solve(800.0, h, 8.4, 18.0);
            assert!(l.rows >= last_rows, "{h} tall lost rows");
            last_rows = l.rows;
        }
    }

    #[test]
    fn a_nonsense_cell_size_yields_no_grid_rather_than_a_full_one() {
        // `(span / cell) as usize` is a saturating cast: a NaN cell answers 0,
        // which reads as an empty window, and a zero cell answers `usize::MAX`
        // -- a loop over every column that never ends.
        for cell in [0.0_f32, -8.4, f32::NAN, f32::INFINITY] {
            assert_eq!(cells_that_fit(800.0, cell), 0, "cell size {cell}");
        }
        for span in [f32::NAN, f32::INFINITY, -1.0] {
            assert_eq!(cells_that_fit(span, 8.4), 0, "span {span}");
        }
        assert_eq!(cells_that_fit(8.4, 8.4), 1, "one cell exactly");
        // The hundredth of a pixel of slack in the loop is what makes this
        // one hold: 8.4 is not a representable float, so two of them added
        // together are a shade over 16.8 and an exact comparison would report
        // a window of exactly two cells as holding one.
        assert_eq!(cells_that_fit(16.8, 8.4), 2, "two cells exactly");
        assert_eq!(cells_that_fit(16.5, 8.4), 1, "well under two cells");
        assert_eq!(cells_that_fit(0.0, 8.4), 0, "no room at all");
    }

    // -----------------------------------------------------------------------
    // What is painted
    // -----------------------------------------------------------------------

    #[test]
    fn nothing_is_painted_outside_the_window() {
        // A grid wider than its window is the ordinary state between a resize
        // being asked for and the child agreeing to it. Without the clip, the
        // surplus columns are painted over whatever is beside the terminal.
        for (name, term) in states() {
            for (w, h) in sizes() {
                let window = Rect::new(0.0, 0.0, w, h);
                for c in term.frame(w, h).commands() {
                    let r = match c {
                        RenderCommand::FillRect {
                            x,
                            y,
                            width,
                            height,
                            ..
                        }
                        | RenderCommand::StrokeRect {
                            x,
                            y,
                            width,
                            height,
                            ..
                        } => Rect::new(*x, *y, *width, *height),
                        RenderCommand::Line { x1, y1, x2, y2, .. } => {
                            Rect::new(x1.min(*x2), y1.min(*y2), (x2 - x1).abs(), (y2 - y1).abs())
                        }
                        _ => continue,
                    };
                    assert!(inside(window, r), "{name}: {r:?} escapes a {w}x{h} window");
                }
            }
        }
    }

    #[test]
    fn no_glyph_runs_off_the_window_it_is_drawn_in() {
        // Every glyph is bounded to one cell's width, so a proportional font
        // -- which is what the compositor falls back to when the monospace
        // face is missing -- cannot walk a row's characters off the right edge
        // one accumulated pixel at a time.
        //
        // The bound has to be the one the command *declares*. This test used to
        // fall back to measuring the glyph when a command declared no bound at
        // all, which made it blind to the only fault it exists to catch: a
        // measurement of a single character is about one cell wide by
        // definition, so an unbounded glyph measured that way looks bounded.
        // The compositor does no such measurement -- it draws until the string
        // ends.
        for (name, term) in states() {
            for (w, h) in sizes() {
                for c in term.frame(w, h).commands() {
                    let RenderCommand::Text {
                        text: glyph_text,
                        x,
                        max_width,
                        ..
                    } = c
                    else {
                        continue;
                    };
                    let Some(bound) = *max_width else {
                        panic!(
                            "{name}: {glyph_text:?} is drawn unbounded in a {w}x{h} window, so a \
                             wide face paints it over its neighbour"
                        );
                    };
                    assert!(
                        *x >= -0.01,
                        "{name}: {glyph_text:?} starts at {x} in a {w}x{h} window"
                    );
                    assert!(
                        x + bound <= w + 0.01,
                        "{name}: {glyph_text:?} runs off a {w}x{h} window: x {x} + {bound}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_frame_balances_its_clips_at_every_size() {
        // An unbalanced clip is a clip left on the stack for whatever the
        // compositor draws next -- another window's problem, found nowhere
        // near here.
        for (name, term) in states() {
            for (w, h) in sizes() {
                assert!(
                    term.frame(w, h).is_balanced(),
                    "{name}: unbalanced clips in a {w}x{h} window"
                );
            }
        }
    }

    #[test]
    fn the_window_is_filled_edge_to_edge_before_anything_else() {
        // The grid is a whole number of cells and the window is not, so up to
        // one cell of width and one of height is left over. Filling only the
        // grid leaves that strip transparent -- a band of desktop showing
        // through the bottom of the terminal.
        for (name, term) in states() {
            for (w, h) in sizes() {
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                let frame = term.frame(w, h);
                let first = frame.commands().iter().find_map(|c| match c {
                    RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        ..
                    } => Some(Rect::new(*x, *y, *width, *height)),
                    _ => None,
                });
                let Some(back) = first else {
                    panic!("{name}: nothing at all is painted in a {w}x{h} window");
                };
                assert!(
                    back.w >= w - 0.01 && back.h >= h - 0.01,
                    "{name}: the backdrop is {back:?} in a {w}x{h} window"
                );
            }
        }
    }

    #[test]
    fn every_hit_box_is_inside_the_window_and_has_area() {
        // A hit box outside the window can never be clicked, and one with no
        // area is a control that is painted and unreachable.
        for (name, term) in states() {
            for (w, h) in sizes() {
                let window = Rect::new(0.0, 0.0, w, h);
                for (target, rect) in term.frame(w, h).hits() {
                    assert!(
                        rect.w > 0.0 && rect.h > 0.0,
                        "{name}: {target:?} has no area at {w}x{h}"
                    );
                    assert!(
                        inside(window, *rect),
                        "{name}: {target:?} is hit-boxed outside a {w}x{h} window"
                    );
                }
            }
        }
    }

    #[test]
    fn every_hit_box_has_ink_painted_at_exactly_that_rectangle() {
        // A hit box is meant to be recorded by the pass that paints the thing
        // it stands for, at the rectangle it painted. Nothing in the type
        // system says so -- `f.hit` takes any rectangle at all -- and a thumb
        // hit-boxed anywhere but where the thumb was drawn is a scrollbar you
        // cannot grab.
        //
        // The grid is the exception and is checked separately below: it is a
        // band of many cells rather than one filled rectangle, and most of
        // those cells are the default background and are not painted at all.
        for (name, term) in states() {
            for (w, h) in sizes() {
                let frame = term.frame(w, h);
                let fills: Vec<Rect> = frame
                    .commands()
                    .iter()
                    .filter_map(|c| match c {
                        RenderCommand::FillRect {
                            x,
                            y,
                            width,
                            height,
                            ..
                        } => Some(Rect::new(*x, *y, *width, *height)),
                        _ => None,
                    })
                    .collect();
                for (target, rect) in frame.hits() {
                    if matches!(target, Target::Grid) {
                        continue;
                    }
                    assert!(
                        fills.iter().any(|f| (f.x - rect.x).abs() < 0.01
                            && (f.y - rect.y).abs() < 0.01
                            && (f.w - rect.w).abs() < 0.01
                            && (f.h - rect.h).abs() < 0.01),
                        "{name}: {target:?} is hit-boxed at {rect:?} in a {w}x{h} window, \
                         where nothing was painted"
                    );
                }
            }
        }
    }

    #[test]
    fn every_hit_box_lies_in_the_band_that_owns_it() {
        // "Stay in the window" is too weak to catch the mistake that matters:
        // a grid hit box that reached under the bar, or a thumb outside its
        // own track, is well inside the window and still wrong -- the first
        // steals the bar's clicks, the second cannot be dragged.
        for (name, term) in states() {
            for (w, h) in sizes() {
                let l = Layout::solve(w, h, term.config.cell_width, term.config.cell_height);
                for (target, rect) in term.frame(w, h).hits() {
                    let (band, band_name) = match target {
                        Target::Grid => (l.grid, "the grid"),
                        Target::ScrollTrack => (l.bar, "the bar"),
                        Target::ScrollThumb => (l.bar, "the bar"),
                    };
                    assert!(
                        inside(band, *rect),
                        "{name}: {target:?} is hit-boxed at {rect:?} in a {w}x{h} window, \
                         outside {band_name} at {band:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_grids_hit_box_covers_every_cell_that_was_drawn() {
        // The other half of the claim the exception above leaves out. The grid
        // answers clicks by arithmetic on the position rather than by a hit
        // box per cell, so the one box it does record has to cover all of
        // them: a glyph painted outside it is a character that cannot be
        // selected by clicking on it.
        for (name, term) in states() {
            for (w, h) in sizes() {
                let frame = term.frame(w, h);
                let Some(grid) = frame
                    .hits()
                    .iter()
                    .find(|(t, _)| matches!(t, Target::Grid))
                    .map(|(_, r)| *r)
                else {
                    continue;
                };
                for c in frame.commands() {
                    let RenderCommand::Text { x, y, .. } = c else {
                        continue;
                    };
                    assert!(
                        grid.contains(*x + 0.1, *y + 0.1),
                        "{name}: a glyph at ({x},{y}) in a {w}x{h} window is outside \
                         the grid's hit box {grid:?}"
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // What the pointer reaches
    // -----------------------------------------------------------------------

    /// The size the pointer tests aim at: wide and tall enough for the whole
    /// default grid, so a cell's coordinates are the obvious ones.
    const AIM: (f32, f32) = (692.0, 432.0);

    #[test]
    fn a_click_in_the_grid_selects_the_character_it_landed_on() {
        // The grid answers clicks by arithmetic rather than by a hit box per
        // cell, so the arithmetic is the thing to pin: a click a third of the
        // way across the fourth row must select the character a third of the
        // way along that row.
        let mut term = fed_terminal(3);
        term.resize_to_window(AIM.0, AIM.1);
        let grid = rect_of_sized(&term, Target::Grid, AIM).expect("the grid is hit-boxed");
        assert!(grid.w > 0.0 && grid.h > 0.0);

        let (cw, ch) = cell();
        term.click_at(cw * 4.5, ch * 1.5, MouseButton::Left, AIM);
        let sel = term.selection.as_ref().expect("a click starts a selection");
        assert_eq!(sel.start_col, 4, "the fifth column");
        assert_eq!(
            sel.start_row,
            term.buffer_row_of(1),
            "the second row of the viewport, as a buffer row"
        );
        assert!(sel.active, "and the drag is live");
    }

    #[test]
    fn a_selection_made_in_the_scrollback_copies_the_scrollback() {
        // The fault this pins is the whole reason selection is expressed in
        // buffer rows. Selection used to record a *screen* row and copy from
        // `self.screen` with the same number, while the drawing pass painted
        // from the scrollback: selecting a line of history highlighted the
        // right text and copied whatever happened to be at that screen slot.
        let mut term = fed_terminal(60);
        term.resize_to_window(AIM.0, AIM.1);
        term.scroll_viewport_up(20);
        let top = term.viewport_top();
        let expected = term
            .line_at(top)
            .expect("the top line of the viewport")
            .cells
            .iter()
            .map(|c| c.ch)
            .collect::<String>();

        term.selection_start(0.0, 0.0);
        term.selection_extend(cell().0 * 100.0, 0.0);
        let copied = term.get_selection_text().expect("something was selected");
        assert_eq!(
            copied.trim_end(),
            expected.trim_end(),
            "the copy came from a different line than the one under the pointer"
        );
        assert!(
            copied.starts_with("line "),
            "a scrollback line, not a blank screen row: {copied:?}"
        );
    }

    #[test]
    fn the_highlight_and_the_copy_agree_on_which_row_is_selected() {
        // Two readers of the same selection: `is_selected`, which decides what
        // is painted, and `get_selection_text`, which decides what is copied.
        // They agreed on nothing when one counted screen rows and the other
        // buffer rows, and a highlight that copies its neighbour is a bug the
        // user only finds after pasting.
        let mut term = fed_terminal(60);
        term.resize_to_window(AIM.0, AIM.1);
        term.scroll_viewport_up(15);
        let (cw, ch) = cell();
        term.selection_start(0.0, ch * 2.0);
        term.selection_extend(cw * 6.0, ch * 2.0);

        let row = term.buffer_row_of(2);
        assert!(term.is_selected(row, 0), "the painted row is not selected");
        assert!(
            !term.is_selected(row + 1, 0),
            "the row below is selected too"
        );
        let copied = term.get_selection_text().expect("something was selected");
        let painted = term
            .line_at(row)
            .expect("the row under the pointer")
            .cells
            .iter()
            .take(7)
            .map(|c| c.ch)
            .collect::<String>();
        assert_eq!(copied, painted);
    }

    #[test]
    fn the_highlight_is_painted_on_the_row_the_pointer_landed_on() {
        // The test above asks `is_selected` the question itself, so it cannot
        // see which row the *drawing pass* asks about. Making the pass ask
        // about the screen row instead of the buffer row -- the original fault,
        // in the one place it was not fixed -- leaves every assertion above
        // true and the highlight painted on a different line, or, once the view
        // is scrolled far enough back that no screen row is also a buffer row,
        // on no line at all. This one reads the picture.
        let mut term = fed_terminal(60);
        term.resize_to_window(AIM.0, AIM.1);
        term.scroll_viewport_up(15);
        let (cw, ch) = cell();
        let aimed_y = ch * 2.0;
        term.selection_start(0.0, aimed_y);
        term.selection_extend(cw * 6.0, aimed_y);

        let sel_bg = term.config.colors.selection_bg;
        let mut highlights = Vec::new();
        for c in term.frame(AIM.0, AIM.1).commands() {
            if let RenderCommand::FillRect { x, y, color, .. } = c
                && *color == sel_bg
            {
                highlights.push((*x, *y));
            }
        }

        assert!(
            !highlights.is_empty(),
            "the selection is painted nowhere in the frame"
        );
        for (x, y) in &highlights {
            assert!(
                (y - aimed_y).abs() < 0.01,
                "a highlight at y {y}, not on the row at y {aimed_y} the pointer landed on"
            );
            assert!(
                *x <= cw * 6.0 + 0.01,
                "a highlight at x {x}, past the column the drag ended on"
            );
        }
        assert_eq!(
            highlights.len(),
            7,
            "the seven cells from column 0 to column 6 inclusive"
        );
    }

    #[test]
    fn a_press_in_the_bar_pages_towards_where_it_landed() {
        // The bar is the only sign a terminal has that there *is* scrollback.
        // A press in it that did nothing would be indistinguishable from a bar
        // that is decoration.
        let mut term = fed_terminal(400);
        term.resize_to_window(AIM.0, AIM.1);
        let before = term.scroll_offset;
        term.click_at(AIM.0 - 2.0, 4.0, MouseButton::Left, AIM);
        assert!(
            term.scroll_offset > before,
            "a press at the top of the track did not page back"
        );
        let up_there = term.scroll_offset;
        term.click_at(AIM.0 - 2.0, AIM.1 - 4.0, MouseButton::Left, AIM);
        assert!(
            term.scroll_offset < up_there,
            "a press at the bottom of the track did not page forward"
        );
    }

    #[test]
    fn a_press_in_the_bar_does_not_start_a_selection() {
        // The bar sits over the right-hand edge of the window, where a click
        // would otherwise be read as a click on the grid's last column: a drag
        // of the scrollbar would select text.
        let mut term = fed_terminal(400);
        term.resize_to_window(AIM.0, AIM.1);
        term.click_at(AIM.0 - 2.0, AIM.1 / 2.0, MouseButton::Left, AIM);
        assert!(
            term.selection.is_none(),
            "dragging the scrollbar selected text"
        );
    }

    #[test]
    fn the_thumb_says_where_in_the_scrollback_the_viewport_is() {
        // A thumb that does not move is a thumb that lies. Checked at three
        // positions rather than one, because a thumb pinned to the top of its
        // track passes any single-position test.
        let mut term = fed_terminal(400);
        term.resize_to_window(AIM.0, AIM.1);
        let thumb = |t: &TerminalState| rect_of_sized(t, Target::ScrollThumb, AIM).map(|r| r.y);
        let bottom = thumb(&term).expect("a thumb at the live end");
        term.scroll_viewport_up(50);
        let middle = thumb(&term).expect("a thumb partway back");
        term.scroll_viewport_up(10_000);
        let top = thumb(&term).expect("a thumb at the far end");
        assert!(
            top < middle && middle < bottom,
            "the thumb is at {top}, {middle}, {bottom} for the top, middle and bottom \
             of the scrollback"
        );
    }

    #[test]
    fn there_is_no_thumb_when_the_whole_buffer_is_on_screen() {
        // A thumb that fills its track says nothing, and a thumb over an empty
        // buffer says there is history where there is none.
        let mut term = fed_terminal(2);
        term.resize_to_window(AIM.0, AIM.1);
        assert!(
            rect_of_sized(&term, Target::ScrollThumb, AIM).is_none(),
            "a terminal with nothing scrolled off drew a thumb"
        );
        assert!(
            rect_of_sized(&term, Target::ScrollTrack, AIM).is_some(),
            "and the track is there regardless, because the bar is reserved"
        );
    }

    #[test]
    fn the_thumb_is_always_thick_enough_to_aim_at() {
        // A ten-thousand-line scrollback in a twenty-four-row window gives the
        // thumb a quarter of a pixel, which is a scrollbar with no scrollbar
        // in it.
        let mut term = fed_terminal(5_000);
        term.resize_to_window(AIM.0, AIM.1);
        let thumb = rect_of_sized(&term, Target::ScrollThumb, AIM)
            .expect("a thumb over five thousand lines");
        assert!(thumb.h >= 4.0, "the thumb is {} tall", thumb.h);
        let track = rect_of_sized(&term, Target::ScrollTrack, AIM).expect("a track under it");
        assert!(inside(track, thumb), "{thumb:?} is outside {track:?}");
    }

    // -----------------------------------------------------------------------
    // The scroll offset, from both ends and through every funnel
    // -----------------------------------------------------------------------

    #[test]
    fn the_offset_never_passes_the_top_of_the_scrollback() {
        let mut term = fed_terminal(40);
        let history = term.scrollback.len();
        term.scroll_viewport_up(10_000);
        assert_eq!(
            term.scroll_offset, history,
            "scrolled past the oldest line there is"
        );
        assert_eq!(term.viewport_top(), 0, "and the viewport is at the top");
    }

    #[test]
    fn the_offset_never_passes_the_live_end() {
        // The other direction, which a one-sided clamp leaves open: an offset
        // below zero is an underflow on a `usize`, not a small error.
        let mut term = fed_terminal(40);
        term.scroll_viewport_up(5);
        term.scroll_viewport_down(10_000);
        assert_eq!(term.scroll_offset, 0);
        term.scroll_viewport_down(1);
        assert_eq!(term.scroll_offset, 0, "and stays there");
    }

    #[test]
    fn a_taller_window_does_not_leave_the_offset_above_the_buffer() {
        // The clamp has to be reached from every path that can invalidate it,
        // not only from the two scroll methods. Resizing is one such path: it
        // pulls rows back out of the scrollback into the screen, so the offset
        // that was exactly at the top before is past it afterwards.
        let mut term = fed_terminal(40);
        term.resize_to_window(692.0, 432.0);
        term.scroll_viewport_up(10_000);
        let scrolled = term.scroll_offset;
        assert!(scrolled > 0, "the fixture did not actually scroll back");
        term.resize_to_window(692.0, 1000.0);
        assert!(
            term.scroll_offset <= term.scrollback.len(),
            "a taller window left the offset at {} over a {}-line scrollback",
            term.scroll_offset,
            term.scrollback.len()
        );
    }

    #[test]
    fn scrolling_back_and_returning_shows_the_same_rows_again() {
        // A round trip, which is what says the offset means the same thing in
        // both directions rather than merely staying in range.
        let mut term = fed_terminal(40);
        term.resize_to_window(692.0, 432.0);
        let top_row = |t: &TerminalState| {
            t.line_at(t.buffer_row_of(0))
                .expect("a top row")
                .cells
                .iter()
                .map(|c| c.ch)
                .collect::<String>()
        };
        let live = top_row(&term);
        // Which end the view starts at, as well as that it returns to it. A
        // round trip on its own is satisfied by any offset that maps rows one
        // to one -- including one counted forward from the oldest line, which
        // shows the top of the session's history at the live end and scrolls
        // the wrong way from there.
        let oldest = term
            .line_at(0)
            .expect("the oldest line")
            .cells
            .iter()
            .map(|c| c.ch)
            .collect::<String>();
        assert_ne!(
            live, oldest,
            "the live view starts at the oldest line rather than the newest screenful"
        );
        term.scroll_viewport_up(7);
        assert_ne!(top_row(&term), live, "scrolling back showed the same rows");
        term.scroll_viewport_down(7);
        assert_eq!(
            top_row(&term),
            live,
            "and coming back showed different ones"
        );
    }

    #[test]
    fn typing_returns_to_the_live_end() {
        // A key that goes to the child while the user is reading the
        // scrollback otherwise has no visible effect at all: the echo lands
        // ten screens below what is on screen.
        let mut term = fed_terminal(40);
        term.scroll_viewport_up(20);
        term.key_at(&press(Key::A), AIM);
        assert_eq!(term.scroll_offset, 0);
    }

    #[test]
    fn a_line_falling_off_the_scrollback_does_not_move_the_selection() {
        // Buffer rows index a run whose *front* moves. When the scrollback is
        // full and the oldest line is dropped, every buffer row means one row
        // less -- and a selection recorded before the drop silently slides one
        // line down the screen for every line the child prints.
        let mut term = TerminalState::new(TerminalConfig {
            scrollback_limit: 8,
            ..TerminalConfig::default()
        });
        for i in 0..40 {
            term.feed(format!("line {i}\r\n").as_bytes());
        }
        let row = term.viewport_top();
        let text_before = term
            .line_at(row)
            .expect("a row")
            .cells
            .iter()
            .map(|c| c.ch)
            .collect::<String>();
        term.selection_start(0.0, 0.0);
        term.selection_extend(cell().0 * 200.0, 0.0);
        let copied_before = term.get_selection_text().expect("a selection");
        assert_eq!(copied_before.trim_end(), text_before.trim_end());

        for i in 40..48 {
            term.feed(format!("line {i}\r\n").as_bytes());
        }
        let copied_after = term.get_selection_text().expect("still a selection");
        assert_eq!(
            copied_after.trim_end(),
            copied_before.trim_end(),
            "the selection now names a different line than the one that was selected"
        );
    }

    // -----------------------------------------------------------------------
    // The child on the other end
    // -----------------------------------------------------------------------

    /// A terminal attached to a scripted child, and the handle to script it.
    ///
    /// Scripted rather than a real shell because these tests are about the
    /// emulator -- what it sends for a key, what it draws for a byte, when it
    /// says the child has gone -- and a process would make every one of them
    /// depend on a platform and a clock. The real link is tested on its own,
    /// against a real shell, in `child.rs`.
    fn scripted() -> (TerminalState, Rc<RefCell<Script>>) {
        TerminalState::new(TerminalConfig::default()).with_script()
    }

    /// Everything on screen, row after row.
    fn screen_text(term: &TerminalState) -> String {
        (0..term.rows())
            .filter_map(|r| term.line_at(term.buffer_row_of(r)))
            .flat_map(|l| l.cells.iter().map(|c| c.ch).collect::<Vec<_>>())
            .collect()
    }

    fn first_row(term: &TerminalState) -> String {
        term.line_at(term.buffer_row_of(0))
            .expect("the first row")
            .cells
            .iter()
            .map(|c| c.ch)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn a_typed_line_reaches_the_child() {
        // A terminal that translates a key correctly and then drops the bytes
        // on the floor is a terminal you cannot type into.
        //
        // Each key goes as it is typed, and Enter as a carriage return. Holding
        // the line until Enter, echoing it, and turning the return into the
        // newline a shell reads are all the kernel's line discipline, which
        // sits between this terminal and the shell; a terminal that did any of
        // them itself would have them done twice.
        //
        // Typed as *text* rather than as `Key::H`: a key code is a position on
        // a keyboard and the character it produces is the layout's business,
        // so a terminal that read the code would type `h` on a French one.
        let (mut term, script) = scripted();
        term.handle_event(&Event::Key(typing("h")));
        term.handle_event(&Event::Key(typing("i")));
        term.handle_event(&Event::Key(press(Key::Enter)));
        let sent = script.borrow().sent.clone();
        assert_eq!(
            sent,
            b"hi\r",
            "the child got {:?}",
            String::from_utf8_lossy(&sent)
        );
    }

    #[test]
    fn a_reply_the_parser_produced_reaches_the_child_too() {
        // The fault the single flush exists to fix. Keystrokes went straight
        // to the child while the parser's own answers -- the cursor position
        // report here -- were appended to `output_buffer` and sent nowhere, so
        // a full-screen program that asked this terminal where its cursor was
        // waited forever for an answer sitting in a `Vec`.
        let (mut term, script) = scripted();
        script.borrow_mut().pending.extend_from_slice(b"\x1b[6n");
        term.handle_event(&Event::Tick { elapsed_ms: 20 });
        let sent = script.borrow().sent.clone();
        assert_eq!(
            sent,
            b"\x1b[1;1R",
            "the child got {:?} instead of a cursor position report",
            String::from_utf8_lossy(&sent)
        );
        assert!(
            term.output_buffer.is_empty(),
            "the reply was sent and is still queued"
        );
    }

    #[test]
    fn a_keystroke_is_sent_once() {
        // It was sent twice: `handle_event` queued the translated bytes on
        // their way to the child, and `on_event` appended the same bytes it
        // had just been handed back. Every character typed arrived doubled.
        let (mut term, script) = scripted();
        term.on_event(&Event::Key(typing("x")));
        term.on_event(&Event::Key(press(Key::Enter)));
        assert_eq!(script.borrow().sent, b"x\r");
    }

    #[test]
    fn what_the_child_could_not_take_yet_is_kept_rather_than_dropped() {
        // A child that is not reading takes part of a write, not none of it
        // and not all of it. Dropping the rest is how a paste into a busy
        // program loses its middle.
        let (mut term, script) = scripted();
        script.borrow_mut().capacity = Some(1000);
        let flood: Vec<u8> = (0..100 * 1024).map(|i| b'a' + (i % 26) as u8).collect();
        term.to_child(&flood);
        term.flush_to_child();
        assert_eq!(
            term.output_buffer.len(),
            flood.len() - 1000,
            "the part the child could not take was not kept"
        );

        // The child starts reading again: the rest goes, in order.
        script.borrow_mut().capacity = None;
        term.flush_to_child();
        assert!(
            term.output_buffer.is_empty(),
            "the bytes the child could not take were never offered again"
        );
        assert!(
            script.borrow().sent == flood,
            "bytes went missing or out of order between the terminal and the child"
        );
    }

    #[test]
    fn a_terminal_with_no_child_keeps_what_was_typed() {
        // No shell could be started. What was typed stays queued rather than
        // vanishing: a terminal that failed to start its shell should not also
        // silently lose the user's input.
        let mut term = TerminalState::new(TerminalConfig::default());
        term.handle_event(&Event::Key(typing("ls")));
        assert_eq!(term.output_buffer, b"ls");
        assert!(
            !term.drain_child(),
            "a terminal with no child read something"
        );
    }

    #[test]
    fn what_the_child_writes_appears_on_the_screen() {
        // The return leg. Without `drain_child` the terminal is write-only:
        // the shell's prompt is produced and never read.
        let (mut term, script) = scripted();
        script.borrow_mut().pending.extend_from_slice(b"hello\r\n");
        assert!(term.drain_child(), "nothing was read back");
        assert_eq!(first_row(&term), "hello");
    }

    #[test]
    fn a_tick_is_what_reads_the_child() {
        // The read has to be on the clock rather than on the keyboard: a
        // program that prints without being typed at -- which is most of them
        // -- would otherwise appear only when the user next pressed a key.
        let (mut term, script) = scripted();
        script.borrow_mut().pending.extend_from_slice(b"unprompted");
        term.handle_event(&Event::Tick { elapsed_ms: 20 });
        assert_eq!(first_row(&term), "unprompted");
    }

    #[test]
    fn a_flood_of_output_is_parsed_a_bounded_amount_at_a_time() {
        // A child that writes faster than the parser runs must not hold the
        // window for the whole of its output: each drain takes a bounded bite
        // and the rest waits for the next tick, in order and complete.
        let (mut term, script) = scripted();
        let total = MAX_READ_PER_DRAIN * 3 + 17;
        script.borrow_mut().pending = vec![b'.'; total];
        assert!(term.drain_child());
        assert_eq!(
            script.borrow().pending.len(),
            total - MAX_READ_PER_DRAIN,
            "one drain took more than its bound"
        );
        let mut drains = 1;
        while term.drain_child() {
            drains += 1;
            assert!(drains < 10, "the output never finished draining");
        }
        assert!(script.borrow().pending.is_empty(), "output was left behind");
        assert_eq!(
            drains, 4,
            "a bound of {MAX_READ_PER_DRAIN} took {drains} drains for {total}"
        );
    }

    #[test]
    fn the_child_is_told_how_big_the_window_is() {
        // `TIOCSWINSZ`. Without it a shell under this terminal wraps its
        // prompt at eighty columns in a window twice that wide, and a
        // full-screen program draws for a screen it does not have.
        let (mut term, script) = scripted();
        term.resize_to_window(300.0, 200.0);
        let l = term.layout();
        let told = script.borrow().size.expect("the child was told no size");
        assert_eq!(usize::from(told.cols), l.cols, "the child's column count");
        assert_eq!(usize::from(told.rows), l.rows, "the child's row count");
        assert!(l.cols < 80, "the fixture did not actually shrink the grid");
        // The pixels are the cells', not the window's: the bar is not text.
        assert!((f32::from(told.xpixel) - l.grid.w.floor()).abs() < f32::EPSILON);
        assert!((f32::from(told.ypixel) - l.grid.h.floor()).abs() < f32::EPSILON);
    }

    #[test]
    fn the_child_is_told_the_size_the_moment_it_is_attached() {
        // A shell is started at a guessed size, and the window may have been
        // resized before the shell was attached to it. Waiting for the next
        // resize to correct it would leave the shell wrong until the user
        // happened to drag the window.
        let mut term = TerminalState::new(TerminalConfig::default());
        term.resize_to_window(300.0, 200.0);
        let (term, script) = term.with_script();
        let told = script
            .borrow()
            .size
            .expect("attaching told the child nothing");
        assert_eq!(usize::from(told.cols), term.layout().cols);
        assert_eq!(usize::from(told.rows), term.layout().rows);
    }

    #[test]
    fn a_child_that_has_exited_is_said_so_once() {
        // An empty read cannot tell "nothing yet" from "never again", so a
        // terminal whose shell has gone otherwise sits at a dead prompt
        // looking exactly like one waiting for output.
        let (mut term, script) = scripted();
        script.borrow_mut().exit = Some(Exit::Code(1));
        assert!(term.drain_child(), "the exit was not noticed");
        assert!(!term.drain_child(), "and it was announced twice");
        assert!(
            screen_text(&term).contains("the shell exited with status 1"),
            "the screen does not say the shell is gone, or how: {:?}",
            screen_text(&term)
        );
    }

    #[test]
    fn a_shell_killed_by_a_signal_is_named_and_the_window_stays() {
        // A crash is the case the user most needs to read about, so the window
        // does not close on it.
        let (mut term, script) = scripted();
        script.borrow_mut().exit = Some(Exit::Signal(9));
        assert!(matches!(
            term.on_event(&Event::Tick { elapsed_ms: 20 }),
            Response::Redraw
        ));
        assert!(
            screen_text(&term).contains("SIGKILL"),
            "{:?}",
            screen_text(&term)
        );
    }

    #[test]
    fn a_shell_that_exits_cleanly_takes_the_window_with_it() {
        // The user typed `exit`. A window left open over a finished shell is a
        // window the user has to close a second time.
        let (mut term, script) = scripted();
        script.borrow_mut().exit = Some(Exit::Code(0));
        assert!(matches!(
            term.on_event(&Event::Tick { elapsed_ms: 20 }),
            Response::Exit
        ));
    }

    #[test]
    fn the_last_output_is_drawn_before_the_exit_is_announced() {
        // A program's final lines and the note that it has gone arrive in one
        // drain; the lines must come first or the note interrupts them.
        let (mut term, script) = scripted();
        {
            let mut s = script.borrow_mut();
            s.pending.extend_from_slice(b"last words");
            s.exit = Some(Exit::Code(2));
        }
        term.drain_child();
        let text = screen_text(&term);
        let words = text.find("last words").expect("the output was not drawn");
        let note = text
            .find("exited with status 2")
            .expect("the exit was not said");
        assert!(
            words < note,
            "the exit was announced before the output: {text:?}"
        );
    }

    #[test]
    fn closing_the_window_hangs_up_the_shell() {
        // A shell left running against a master nobody reads is a process
        // leaked every time a window closes.
        let (mut term, script) = scripted();
        assert!(matches!(
            term.on_event(&Event::CloseRequested),
            Response::Exit
        ));
        assert_eq!(script.borrow().hang_ups, 1, "the shell was not told");
    }

    #[test]
    fn a_live_child_keeps_the_clock_running_and_quiet_slows_it() {
        // The child's output reaches the screen only on a tick, so a live
        // child needs a clock even with a solid cursor -- fast while the two
        // are talking, slower once they are not, and none once it has gone.
        let (mut term, script) = TerminalState::new(TerminalConfig {
            cursor_blink: false,
            ..TerminalConfig::default()
        })
        .with_script();
        let ms = |t: &TerminalState| t.tick_interval().map(|d| d.as_millis());
        assert_eq!(ms(&term), Some(u128::from(ACTIVE_POLL_MS)), "a fresh child");

        let quiet_ticks = ACTIVE_WINDOW_MS / 100;
        for _ in 0..quiet_ticks {
            term.on_event(&Event::Tick { elapsed_ms: 100 });
        }
        assert_eq!(ms(&term), Some(u128::from(IDLE_POLL_MS)), "a quiet child");

        script.borrow_mut().pending.extend_from_slice(b"$ ");
        term.on_event(&Event::Tick { elapsed_ms: 100 });
        assert_eq!(
            ms(&term),
            Some(u128::from(ACTIVE_POLL_MS)),
            "output did not wake it"
        );

        for _ in 0..quiet_ticks {
            term.on_event(&Event::Tick { elapsed_ms: 100 });
        }
        assert_eq!(ms(&term), Some(u128::from(IDLE_POLL_MS)), "quiet again");
        term.on_event(&Event::Key(typing("l")));
        assert_eq!(
            ms(&term),
            Some(u128::from(ACTIVE_POLL_MS)),
            "typing did not wake it"
        );

        script.borrow_mut().exit = Some(Exit::Code(4));
        term.on_event(&Event::Tick { elapsed_ms: 16 });
        assert_eq!(ms(&term), None, "a finished child still holds the clock");
    }

    // -----------------------------------------------------------------------
    // The clock
    // -----------------------------------------------------------------------

    #[test]
    fn the_bell_flash_ages_on_the_clock_rather_than_on_redraws() {
        // The flash used to be a countdown of *frames*, so it lasted a tenth
        // of a second at sixty of them a second, twice that at thirty, and for
        // ever on a terminal with nothing else to redraw for -- which is the
        // usual state of a terminal waiting at a prompt.
        let mut term = fed_terminal(3);
        term.feed(b"\x07");
        assert!(term.bell_flash_ms > 0, "the bell did not light anything");
        for _ in 0..100 {
            let _ = term.frame(AIM.0, AIM.1);
        }
        assert!(
            term.bell_flash_ms > 0,
            "a hundred frames put the bell out without a millisecond passing"
        );
        term.tick(BELL_MS);
        assert_eq!(term.bell_flash_ms, 0, "the clock did not put it out");
    }

    #[test]
    fn the_bell_is_visible_while_it_is_lit() {
        // A flash nothing paints is a countdown and not a bell.
        let mut lit = fed_terminal(3);
        lit.feed(b"\x07");
        let quiet = fed_terminal(3);
        let ink = |t: &TerminalState| format!("{:?}", t.frame(AIM.0, AIM.1).commands());
        assert_ne!(
            ink(&lit),
            ink(&quiet),
            "the belling terminal paints exactly what the quiet one does"
        );
        lit.tick(BELL_MS);
        assert_eq!(
            ink(&lit),
            ink(&quiet),
            "and goes on painting it after the bell has gone out"
        );
    }

    #[test]
    fn the_cursor_blinks_once_per_interval_however_the_ticks_arrive() {
        // Ticks arrive when the loop next runs, not when they were asked for.
        // A blink advanced by one half per tick blinks at the frame rate; one
        // advanced by the elapsed time blinks at its own rate regardless.
        let mut fast = TerminalState::new(TerminalConfig::default());
        for _ in 0..10 {
            fast.tick(BLINK_MS / 10);
        }
        let mut slow = TerminalState::new(TerminalConfig::default());
        slow.tick(BLINK_MS);
        assert_eq!(
            fast.blink_on, slow.blink_on,
            "ten short ticks and one long one of the same length disagree"
        );
        assert!(!slow.blink_on, "one interval did not put the cursor out");
        slow.tick(BLINK_MS);
        assert!(slow.blink_on, "and the second did not bring it back");
    }

    /// Is the block cursor's own ink anywhere in the frame?
    ///
    /// The block cursor is the only thing painted in the cursor colour at that
    /// alpha, so matching the fill exactly is enough to say whether the user
    /// can see a cursor.
    fn cursor_is_painted(term: &TerminalState) -> bool {
        let c = term.config.colors.cursor;
        term.frame(AIM.0, AIM.1).commands().iter().any(|cmd| {
            matches!(
                cmd,
                RenderCommand::FillRect { color, .. }
                    if *color == Color::rgba(c.r, c.g, c.b, 180)
            )
        })
    }

    #[test]
    fn the_cursor_leaves_the_picture_in_the_dark_half_of_the_blink() {
        // `config.cursor_blink` was a setting nothing read: the drawing pass
        // painted the cursor on every frame. Every test above reads `blink_on`,
        // which the clock has always kept correctly -- so all of them passed
        // against a terminal whose cursor sat there solid. The question the
        // user actually asks is whether the cursor is on the screen.
        let mut term = TerminalState::new(TerminalConfig::default());
        assert!(term.blink_on, "a fresh terminal starts in the lit half");
        assert!(cursor_is_painted(&term), "the lit half painted no cursor");
        term.tick(BLINK_MS);
        assert!(!term.blink_on, "one interval did not put the cursor out");
        assert!(
            !cursor_is_painted(&term),
            "the dark half of the blink painted a cursor anyway"
        );
        term.tick(BLINK_MS);
        assert!(
            cursor_is_painted(&term),
            "the second interval did not bring it back"
        );
    }

    #[test]
    fn a_tick_longer_than_several_blinks_lands_on_the_right_half() {
        // The `while` rather than an `if`: a loop that fell behind by three
        // intervals used to consume one and carry the rest, so the cursor
        // blinked at whatever rate the machine was struggling at.
        let mut term = TerminalState::new(TerminalConfig::default());
        term.tick(BLINK_MS * 3);
        assert!(!term.blink_on, "three halves is the dark one");
        assert!(
            term.blink_ms < BLINK_MS,
            "{}ms of blink is still owed after the tick",
            term.blink_ms
        );
    }

    #[test]
    fn a_tick_that_changed_nothing_asks_for_no_frame() {
        // The cursor blinks five times a second and the loop runs twenty-five:
        // four ticks in five have nothing to show, and a terminal that redraws
        // for all of them is a terminal that never lets the machine idle.
        let mut term = TerminalState::new(TerminalConfig::default());
        assert!(
            matches!(
                term.on_event(&Event::Tick { elapsed_ms: 1 }),
                Response::Idle
            ),
            "a millisecond of nothing asked for a frame"
        );
        assert!(
            matches!(
                term.on_event(&Event::Tick {
                    elapsed_ms: BLINK_MS
                }),
                Response::Redraw
            ),
            "a whole blink did not ask for one"
        );
    }

    #[test]
    fn a_tick_is_what_reads_the_child_through_the_window_too() {
        // The fault the tick funnel exists to fix, and the reason the direct
        // `handle_event` test above did not catch it: `on_event` answered the
        // tick itself -- it had to know whether anything changed before it
        // could say whether the frame was worth drawing -- and returned before
        // the dispatch that reads the child ever ran. Under a real window,
        // which is the only place `on_event` is called, the shell's prompt was
        // produced and never collected.
        let (mut term, script) = scripted();
        script.borrow_mut().pending.extend_from_slice(b"prompt$ ");
        let response = term.on_event(&Event::Tick { elapsed_ms: 20 });
        assert!(
            matches!(response, Response::Redraw),
            "the child wrote and the window was not asked to redraw"
        );
        assert_eq!(first_row(&term), "prompt$");
    }

    #[test]
    fn the_clock_is_asked_for_only_while_something_is_moving() {
        // A program that asks for a tick it does not need holds the whole
        // desktop awake, and one that does not ask when it does need it stops
        // dead: both halves, because either alone is passed by a constant.
        let mut term = TerminalState::new(TerminalConfig {
            cursor_blink: false,
            ..TerminalConfig::default()
        });
        assert!(
            term.tick_interval().is_none(),
            "a terminal with nothing to age asked for a clock"
        );
        term.feed(b"\x07");
        assert!(
            term.tick_interval().is_some(),
            "a ringing bell has to go out on its own"
        );
        term.tick(BELL_MS);
        assert!(term.tick_interval().is_none(), "and then stop asking");

        let blinking = TerminalState::new(TerminalConfig::default());
        assert!(
            blinking.tick_interval().is_some(),
            "a blinking cursor has to be driven"
        );
    }

    #[test]
    fn a_hidden_cursor_needs_no_clock_and_is_not_left_dark() {
        // `\x1b[?25l` is what every full-screen program sends on startup. A
        // terminal that kept blinking an invisible cursor would tick for the
        // whole session, and one whose cursor was hidden during the dark half
        // of a blink would show nothing when it came back.
        let mut term = TerminalState::new(TerminalConfig::default());
        term.tick(BLINK_MS);
        assert!(!term.blink_on, "the fixture is not in the dark half");
        term.feed(b"\x1b[?25l");
        assert!(
            term.tick_interval().is_none(),
            "a hidden cursor still asked for a clock"
        );
        term.tick(20);
        assert!(term.blink_on, "the cursor was left in the dark half");
    }

    // -----------------------------------------------------------------------
    // The window the compositor drives
    // -----------------------------------------------------------------------

    #[test]
    fn the_first_resize_is_what_sizes_the_grid() {
        // The compositor sends a resize before the first frame, and the grid
        // that answers it is the one the child is told about.
        let mut term = TerminalState::new(TerminalConfig::default());
        term.on_event(&Event::Resize {
            width: 300,
            height: 200,
        });
        let l = term.layout();
        assert_eq!(term.cols(), l.cols, "the grid did not follow the resize");
        assert_eq!(term.rows(), l.rows);
        assert!(l.cols < 80, "the fixture did not actually shrink the grid");
    }

    #[test]
    fn drawing_a_frame_sizes_the_grid_as_well() {
        // A frame is the one thing an application is guaranteed to be asked
        // for. A grid that follows only the resize event disagrees with the
        // window whenever one is missed, and a grid that disagrees with its
        // window shows the wrong number of columns to the program under it.
        let mut term = TerminalState::new(TerminalConfig::default());
        let _ = term.render(300.0, 200.0);
        assert!(
            term.cols() < 80,
            "{} columns in a 300-pixel window",
            term.cols()
        );
        let _ = term.render(1600.0, 900.0);
        assert!(
            term.cols() > 80,
            "{} columns in a 1600-pixel window",
            term.cols()
        );
    }

    #[test]
    fn closing_the_window_ends_the_program() {
        let mut term = TerminalState::new(TerminalConfig::default());
        assert!(matches!(
            term.on_event(&Event::CloseRequested),
            Response::Exit
        ));
    }

    #[test]
    fn the_window_opens_at_a_size_the_default_grid_fits_in() {
        // The initial size and the default grid are two constants that have to
        // agree: a window that opens one column short of its own grid resizes
        // itself on the first frame, which the user sees as a flicker.
        let term = TerminalState::new(TerminalConfig::default());
        let (w, h) = term.initial_size();
        let l = Layout::solve(
            u32_f32(w),
            u32_f32(h),
            term.config.cell_width,
            term.config.cell_height,
        );
        assert_eq!(l.cols, 80, "the default eighty columns do not fit");
        assert_eq!(l.rows, 24, "the default twenty-four rows do not fit");
    }

    // -----------------------------------------------------------------------
    // The two arithmetic helpers the bar is built on
    // -----------------------------------------------------------------------

    #[test]
    fn a_fraction_of_nothing_is_nothing_rather_than_a_nan() {
        // `part / whole` with a zero whole is the state of the scrollbar on an
        // empty terminal, and a NaN there propagates into a thumb position,
        // which paints nowhere at all rather than visibly wrongly.
        assert!((ratio(0, 0) - 0.0).abs() < f32::EPSILON);
        assert!((ratio(5, 0) - 0.0).abs() < f32::EPSILON);
        assert!((ratio(1, 4) - 0.25).abs() < f32::EPSILON);
        assert!(
            (ratio(9, 4) - 1.0).abs() < f32::EPSILON,
            "a part larger than the whole is a fraction over one"
        );
    }

    #[test]
    fn scaling_by_a_nonsense_fraction_yields_none_of_the_whole() {
        // The inverse of the above, and the reason it is a loop rather than a
        // cast: `(whole as f32 * fraction) as usize` is a saturating cast, so
        // a NaN fraction answers zero but a negative one answers zero too
        // *only by accident*, and an infinite one answers `usize::MAX`.
        for fraction in [f32::NAN, f32::INFINITY, -1.0, -0.0] {
            assert_eq!(scale(100, fraction), 0, "fraction {fraction}");
        }
        assert_eq!(scale(100, 1.0), 100, "the whole of it");
        assert_eq!(scale(100, 2.0), 100, "and no more than the whole");
        assert_eq!(scale(100, 0.5), 50);
        assert_eq!(scale(0, 0.5), 0, "half of nothing");
        assert_eq!(scale(3, 0.99), 2, "rounded down, not to the nearest");
    }

    // -- what a multiplexer asks of a terminal --

    #[test]
    fn a_paste_reaches_the_child_with_its_line_breaks_as_returns() {
        // Enter sends a carriage return, and a program reading raw keys takes
        // a bare line feed for Ctrl+J -- so a pasted line break is sent as the
        // key it stands for.
        let (mut term, script) = scripted();
        term.scroll_offset = 3;
        term.paste("echo a\necho b\r\n");
        assert_eq!(script.borrow().sent, b"echo a\recho b\r");
        assert_eq!(term.scroll_offset, 0, "a paste returns to the live screen");
    }

    #[test]
    fn a_program_that_asked_for_bracketed_paste_gets_its_paste_fenced() {
        // `CSI ? 2004 h` was recorded and read by nothing, so a shell that
        // asked to be told a paste from typing was never told, and ran a
        // pasted line the moment its line break arrived.
        let (mut term, script) = scripted();
        script
            .borrow_mut()
            .pending
            .extend_from_slice(b"\x1b[?2004h");
        term.handle_event(&Event::Tick { elapsed_ms: 20 });
        term.paste("ls\x1b[201~; rm x\n");
        assert_eq!(
            script.borrow().sent,
            b"\x1b[200~ls[201~; rm x\r\x1b[201~",
            "fenced -- and the text's own escape cannot close the fence early"
        );

        script.borrow_mut().sent.clear();
        script
            .borrow_mut()
            .pending
            .extend_from_slice(b"\x1b[?2004l");
        term.handle_event(&Event::Tick { elapsed_ms: 20 });
        term.paste("ls\n");
        assert_eq!(
            script.borrow().sent,
            b"ls\r",
            "and plain once it is turned off"
        );
    }

    #[test]
    fn an_empty_paste_sends_nothing() {
        let (mut term, script) = scripted();
        term.paste("");
        assert!(script.borrow().sent.is_empty());
    }

    /// The cursor-shaped marks a frame draws in the first cell.
    fn cursor_marks(term: &TerminalState) -> (usize, usize) {
        let frame = term.frame(400.0, 200.0);
        let (w, h) = (term.config.cell_width, term.config.cell_height);
        let at_origin = |x: f32, y: f32, width: f32, height: f32| {
            x.abs() < 0.01
                && y.abs() < 0.01
                && (width - w).abs() < 0.01
                && (height - h).abs() < 0.01
        };
        let outlines = frame
            .commands()
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { x, y, width, height, .. } if at_origin(*x, *y, *width, *height)))
            .count();
        let blocks = frame
            .commands()
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { x, y, width, height, .. } if at_origin(*x, *y, *width, *height)))
            .count();
        (outlines, blocks)
    }

    #[test]
    fn a_terminal_without_the_keyboard_draws_its_cursor_as_an_outline() {
        // A split window draws several terminals and types into one. Every
        // cursor was a blinking block, so which pane would take the next key
        // was a guess.
        let mut term = TerminalState::new(TerminalConfig::default());
        assert_eq!(cursor_marks(&term), (0, 1), "a focused terminal: a block");

        term.set_focused(false);
        assert!(!term.is_focused());
        assert_eq!(cursor_marks(&term), (1, 0), "an unfocused one: an outline");
        // And it does not blink: nothing to age, so no clock asked for.
        assert!(
            !term.tick(BLINK_MS * 3),
            "an unfocused cursor changed on a tick"
        );
        assert_eq!(term.tick_interval(), None);

        term.on_event(&Event::FocusIn);
        assert!(
            term.is_focused(),
            "the window's own focus event gives it back"
        );
        assert_eq!(cursor_marks(&term), (0, 1));
        term.on_event(&Event::FocusOut);
        assert!(!term.is_focused());
    }

    #[test]
    fn a_shell_that_cannot_start_is_named_on_the_screen() {
        // A terminal with no shell must not look like a terminal with a quiet
        // one.
        let mut term = TerminalState::new(TerminalConfig::default());
        term.start_with(|_| {
            Err(super::SpawnError {
                program: "/bin/sh".into(),
                errno: libcall::ENOSYS,
            })
        });
        let text = screen_text(&term);
        assert!(text.contains("No shell."), "{text:?}");
        assert!(text.contains("no pseudo-terminals"), "the reason: {text:?}");
        assert!(text.contains("what you type goes nowhere"), "{text:?}");
        assert!(!term.child_is_live());
        assert_eq!(term.exit_status(), None);
    }

    #[test]
    fn a_started_shell_is_born_at_the_size_it_is_drawn_at() {
        let mut term = TerminalState::new(TerminalConfig::default());
        term.resize_to_window(500.0, 300.0);
        let (link, script) = ScriptLink::new();
        let asked = RefCell::new(None);
        term.start_with(|size| {
            *asked.borrow_mut() = Some(size);
            Ok(Box::new(link))
        });
        let want = term.win_size();
        assert_eq!(asked.into_inner(), Some(want), "spawned at the grid's size");
        assert_eq!(script.borrow().size, Some(want), "and told it on attaching");
        assert!(term.child_is_live());

        script.borrow_mut().exit = Some(Exit::Code(3));
        term.on_event(&Event::Tick { elapsed_ms: 20 });
        assert_eq!(term.exit_status(), Some(Exit::Code(3)));
        assert!(!term.child_is_live());
    }

    #[test]
    fn hanging_up_tells_the_child() {
        let (mut term, script) = scripted();
        term.hang_up();
        assert_eq!(script.borrow().hang_ups, 1);
    }

    // -- woken, not asked --

    /// A terminal with a still cursor and a link that wakes it: nothing to
    /// ask a clock for. The link is given the waker whichever of the two
    /// arrives first.
    fn woken_terminal(waker_first: bool) -> (TerminalState, Rc<RefCell<Script>>) {
        let mut term = TerminalState::new(TerminalConfig {
            cursor_blink: false,
            ..TerminalConfig::default()
        });
        let (link, script) = ScriptLink::new();
        script.borrow_mut().wakes = true;
        if waker_first {
            term.attach_waker(std::task::Waker::noop().clone());
            term.attach(Box::new(link));
        } else {
            term.attach(Box::new(link));
            term.attach_waker(std::task::Waker::noop().clone());
        }
        (term, script)
    }

    /// Text marked blinking blinks, on a clock of its own, and the clock stops
    /// when no such text is left on screen. SGR 5 was recorded and nothing
    /// read it.
    #[test]
    fn blinking_text_blinks_and_only_while_it_is_on_screen() {
        let mut term = TerminalState::new(TerminalConfig {
            cursor_blink: false,
            ..TerminalConfig::default()
        });
        assert_eq!(term.tick_interval(), None, "control: nothing moving");
        term.feed(b"\x1b[5mB\x1b[0mS");
        let glyphs = |t: &TerminalState| -> Vec<String> {
            t.frame(400.0, 200.0)
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };
        assert!(
            glyphs(&term).contains(&"B".to_string()),
            "lit to begin with"
        );
        assert!(
            term.tick_interval().is_some(),
            "blinking text needs a clock"
        );

        assert!(term.tick(BLINK_MS), "half a blink is a change");
        let dark = glyphs(&term);
        assert!(!dark.contains(&"B".to_string()), "the dark half: {dark:?}");
        assert!(
            dark.contains(&"S".to_string()),
            "and only the blinking text goes"
        );

        term.tick(BLINK_MS);
        assert!(glyphs(&term).contains(&"B".to_string()), "and back");

        term.feed(b"\x1b[2J");
        term.tick(BLINK_MS);
        assert_eq!(term.tick_interval(), None, "no blinking text, no clock");
    }

    #[test]
    fn a_link_that_wakes_the_terminal_is_not_asked_on_a_clock() {
        // Twenty times a second at a prompt, forever, to find nothing: the
        // clock the terminal asked for while the window could not be woken
        // for its shell.
        for waker_first in [true, false] {
            let (term, script) = woken_terminal(waker_first);
            assert!(term.wants_waker());
            assert!(
                script.borrow().waker.is_some(),
                "the link was not given the waker"
            );
            assert_eq!(term.tick_interval(), None, "waker first: {waker_first}");
        }
    }

    #[test]
    fn a_link_that_cannot_wake_is_still_asked() {
        // The one thing that must not happen: a terminal that stops asking a
        // link that never wakes it.
        let mut term = TerminalState::new(TerminalConfig {
            cursor_blink: false,
            ..TerminalConfig::default()
        });
        let (link, _script) = ScriptLink::new();
        term.attach(Box::new(link));
        term.attach_waker(std::task::Waker::noop().clone());
        assert_eq!(
            term.tick_interval().map(|d| d.as_millis()),
            Some(u128::from(ACTIVE_POLL_MS))
        );
    }

    #[test]
    fn a_wake_reads_the_child_and_draws_only_what_changed() {
        let (mut term, script) = woken_terminal(true);
        script
            .borrow_mut()
            .pending
            .extend_from_slice(b"hello\x1b[6n");
        assert!(matches!(term.on_wake(), Response::Redraw));
        assert!(screen_text(&term).contains("hello"));
        assert_eq!(
            script.borrow().sent,
            b"\x1b[1;6R",
            "and the reply the reading produced went out on the same wake"
        );
        assert!(matches!(term.on_wake(), Response::Idle), "nothing new");
    }

    #[test]
    fn a_clean_exit_found_on_a_wake_closes_the_window() {
        let (mut term, script) = woken_terminal(true);
        script.borrow_mut().exit = Some(Exit::Code(0));
        assert!(matches!(term.on_wake(), Response::Exit));
    }

    #[test]
    fn a_flood_left_over_after_a_wake_is_read_on_the_clock_until_it_is_gone() {
        // The reader wakes once per chunk it sends; what a wake leaves behind
        // it has already sent, and will not announce again.
        let (mut term, script) = woken_terminal(true);
        script
            .borrow_mut()
            .pending
            .extend(std::iter::repeat_n(b'x', MAX_READ_PER_DRAIN * 3));
        term.on_wake();
        assert_eq!(
            term.tick_interval().map(|d| d.as_millis()),
            Some(u128::from(ACTIVE_POLL_MS)),
            "output was left behind and nothing will wake for it"
        );
        for _ in 0..4 {
            term.on_event(&Event::Tick { elapsed_ms: 16 });
        }
        assert!(script.borrow().pending.is_empty());
        assert_eq!(term.tick_interval(), None, "and once it is gone, no clock");
    }

    // -- resizing, which a multiplexer does on every split --

    /// The text of screen row `row`, trailing blanks trimmed.
    fn row_text(term: &TerminalState, row: usize) -> String {
        term.line_at(term.buffer_row_of(row))
            .map(|l| l.cells.iter().map(|c| c.ch).collect::<String>())
            .unwrap_or_default()
            .trim_end()
            .to_string()
    }

    fn with_lines(n: usize) -> TerminalState {
        let mut term = TerminalState::new(TerminalConfig::default());
        for i in 0..n {
            term.feed(format!("line {i}\r\n").as_bytes());
        }
        term
    }

    #[test]
    fn shrinking_keeps_a_prompt_at_the_top_on_screen() {
        // Every row used to leave from the top, so a shell's prompt on row 0
        // went into the scrollback the moment the grid got shorter -- and a
        // pane split in two showed none of the shell it had.
        let mut term = TerminalState::new(TerminalConfig::default());
        term.feed(b"$ ");
        term.resize(80, 10);
        assert_eq!(row_text(&term, 0), "$", "the prompt stays on screen");
        assert_eq!(term.buffer_len(), 10, "nothing went into the scrollback");
        assert_eq!((term.cursor_row, term.cursor_col), (0, 2));
    }

    #[test]
    fn shrinking_a_full_screen_sends_its_top_into_the_scrollback() {
        // With the cursor on the last row there is nothing below it to take,
        // so the top goes -- into the history, not away.
        let mut term = with_lines(30);
        term.feed(b"$ ");
        let before = term.buffer_len();
        term.resize(80, 10);
        assert_eq!(row_text(&term, 9), "$", "the prompt is on the last row");
        assert_eq!(term.cursor_row, 9);
        assert_eq!(
            term.buffer_len(),
            before,
            "nothing was lost: the fourteen rows went into the history"
        );
        assert_eq!(
            term.scroll_offset, 0,
            "and the view is still the live screen"
        );
        assert_eq!(row_text(&term, 0), "line 21");
    }

    #[test]
    fn growing_back_returns_exactly_what_shrinking_took() {
        let mut term = with_lines(30);
        term.feed(b"$ ");
        let screen = |t: &TerminalState| (0..t.rows()).map(|r| row_text(t, r)).collect::<Vec<_>>();
        let (before, len) = (screen(&term), term.buffer_len());
        term.resize(80, 10);
        term.resize(80, 24);
        assert_eq!(screen(&term), before);
        assert_eq!(term.buffer_len(), len);
        assert_eq!((term.cursor_row, term.cursor_col), (23, 2));
    }

    #[test]
    fn growing_does_not_pull_down_history_the_shrink_did_not_take() {
        // Growing used to pull back any history at all, so a prompt at the top
        // of a cleared screen was pushed down under the lines the user had
        // just cleared away -- every time a split pane was closed again.
        let mut term = with_lines(40);
        term.feed(b"\x1b[H\x1b[2J$ ");
        term.resize(80, 10);
        term.resize(80, 24);
        assert_eq!(row_text(&term, 0), "$", "the prompt is still at the top");
        assert_eq!(term.cursor_row, 0);
    }

    #[test]
    fn resizing_under_a_full_screen_program_keeps_the_shells_prompt() {
        // The shell's screen waits, hidden, while `vim` has the alternate one.
        // It was cut from the bottom with its cursor left where it was, so
        // the prompt was gone when the program exited.
        let mut term = with_lines(30);
        term.feed(b"$ ");
        term.feed(b"\x1b[?1049h\x1b[5;5Hvim");
        term.resize(80, 10);
        term.feed(b"\x1b[?1049l");
        assert_eq!(
            row_text(&term, term.cursor_row),
            "$",
            "the prompt came back"
        );
        assert_eq!((term.cursor_row, term.cursor_col), (9, 2));

        term.feed(b"\x1b[?1049h");
        term.resize(80, 24);
        term.feed(b"\x1b[?1049l");
        assert_eq!(
            (term.cursor_row, term.cursor_col),
            (23, 2),
            "and back again"
        );
        assert_eq!(row_text(&term, 23), "$");
        assert_eq!(row_text(&term, 0), "line 7");
    }

    #[test]
    fn a_saved_cursor_moves_with_its_line_when_the_grid_shrinks() {
        // Saved on row 20, then three more lines: shrinking by fourteen takes
        // fourteen rows off the top, so the saved line is now row 6. Left at
        // row 20 it would be clamped to the last row, which holds a different
        // line.
        let mut term = with_lines(20);
        term.feed(b"\x1b7a\r\nb\r\nc\r\n$ ");
        term.resize(80, 10);
        term.feed(b"\x1b8");
        assert_eq!((term.cursor_row, term.cursor_col), (6, 0));
        assert_eq!(row_text(&term, 6), "a", "on the line it was saved on");
    }

    #[test]
    fn a_programs_own_saved_cursor_does_not_replace_the_shells() {
        // One saved-cursor slot served both screens, so a program that saved
        // its cursor on the alternate screen handed its position back to the
        // shell on the way out.
        let mut term = TerminalState::new(TerminalConfig::default());
        term.feed(b"abc");
        term.feed(b"\x1b[?1049h\x1b[5;5H\x1b7\x1b[1;1H");
        term.feed(b"\x1b[?1049l");
        assert_eq!((term.cursor_row, term.cursor_col), (0, 3));
        term.feed(b"\x1b[?1049h\x1b8");
        assert_eq!(
            (term.cursor_row, term.cursor_col),
            (4, 4),
            "the alternate screen's slot is its own: the program's save is still there"
        );
    }

    #[test]
    fn a_reset_keeps_the_shell() {
        // `ESC c` rebuilt the whole terminal, link and all, and dropping the
        // link hangs up: typing `reset` at a shell killed the shell.
        let (mut term, script) = scripted();
        term.resize_to_window(500.0, 300.0);
        let size = term.win_size();
        script.borrow_mut().pending.extend_from_slice(b"junk\x1bc");
        term.handle_event(&Event::Tick { elapsed_ms: 20 });
        assert_eq!(script.borrow().hang_ups, 0, "the shell was hung up on");
        assert!(term.child_is_live());
        assert_eq!(term.win_size(), size, "the grid kept the window's size");
        assert_eq!(row_text(&term, 0), "", "and the screen was reset");
        term.handle_event(&Event::Key(typing("x")));
        assert_eq!(script.borrow().sent, b"x", "the shell still hears the keys");
    }

    // -- the grid's face --

    /// A character has to fit the cell drawn behind it. Measured in the
    /// family the glyphs are drawn in: in the proportional UI face a `W` came
    /// out far wider than an `i`, over its neighbour's background and beside
    /// the block cursor meant to mark it.
    #[test]
    fn a_character_fits_its_cell() {
        let (w, _) = cell();
        for weight in [FontWeightHint::Regular, FontWeightHint::Bold] {
            for ch in ['0', 'W', 'i', '#', 'é', 'M', '@'] {
                let drawn = text::measure_in(&ch.to_string(), FONT_SIZE, weight, FontFamily::Mono);
                assert!(
                    drawn <= w + 0.01,
                    "{ch:?} ({weight:?}) draws {drawn} px in a {w} px cell"
                );
            }
        }
    }

    /// The row pitch has to clear the face's ascent, or rows overlap.
    #[test]
    fn rows_do_not_overlap() {
        let (w, h) = cell();
        assert!(w > 0.0 && h > 0.0);
        assert!(h >= text::ascent_in(FONT_SIZE, FontWeightHint::Regular, FontFamily::Mono));
    }

    /// The grid's glyphs are drawn inside a fixed-pitch scope, and nothing
    /// else is -- the cells were measured in that face.
    #[test]
    fn the_grid_is_drawn_in_the_family_it_was_measured_in() {
        let term = with_lines(3);
        let frame = term.frame(400.0, 200.0);
        let (mut depth, mut deepest, mut glyphs, mut outside) = (0_i32, 0_i32, 0, 0);
        for cmd in frame.commands() {
            match cmd {
                RenderCommand::PushFont { family } => {
                    assert_eq!(family, &FontFamily::Mono);
                    depth += 1;
                    deepest = deepest.max(depth);
                }
                RenderCommand::PopFont => depth -= 1,
                RenderCommand::Text { .. } if depth > 0 => glyphs += 1,
                RenderCommand::Text { .. } => outside += 1,
                _ => {}
            }
        }
        assert_eq!((depth, deepest), (0, 1), "one scope, balanced");
        assert!(glyphs > 0, "no glyph inside the scope");
        assert_eq!(outside, 0, "a glyph drawn in the proportional face");
    }

    /// A glyph is clipped to the cells it covers: two for a wide character,
    /// one otherwise. It was clipped to a proportional `W`.
    #[test]
    fn a_glyph_is_clipped_to_the_cells_it_covers() {
        let mut term = TerminalState::new(TerminalConfig::default());
        term.feed("a\u{4E2D}".as_bytes());
        let (w, _) = cell();
        let widths: Vec<(String, f32)> = term
            .frame(400.0, 200.0)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text, max_width, ..
                } => Some((text.clone(), max_width.unwrap_or(0.0))),
                _ => None,
            })
            .collect();
        let of = |s: &str| widths.iter().find(|(t, _)| t == s).map(|(_, m)| *m);
        assert_eq!(of("a"), Some(w));
        assert_eq!(of("\u{4E2D}"), Some(w * 2.0));
    }
}

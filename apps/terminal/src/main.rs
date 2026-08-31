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
//! eighty by twenty-four whatever the window was, the PTY in [`pty`] was dead
//! code, and the bell counted down in frames that were never drawn. The
//! emulator now owns a real [`oswindow`] window: [`Layout`] is solved from the
//! size the compositor hands [`App::render`] every frame, keystrokes go to a
//! child through [`pty::PtyMaster`] and its output comes back, and the bell and
//! cursor blink age in milliseconds off [`Event::Tick`].
//!
//! Renders via the guitk RenderTree, producing Text and FillRect commands
//! for each visible cell in the terminal grid.

pub mod pty;

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};

use std::collections::VecDeque;
use std::process::ExitCode;

// ============================================================================
// Window geometry
// ============================================================================

/// The natural window size: an eighty-by-twenty-four grid, the size every
/// terminal has opened at since the VT100, plus the scrollback bar.
const WINDOW_WIDTH: f32 = 80.0 * 8.4 + BAR_W;
/// Twenty-four rows of the default cell height.
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
            font_size: 14.0,
            cell_width: 8.4,
            cell_height: 18.0,
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
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
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
    /// Whether the cursor is in the lit half of its blink.
    ///
    /// `config.cursor_blink` defaulted to `true` and nothing in the program
    /// read it: the cursor was drawn on every frame regardless, so the setting
    /// was a field a user could change to no effect.
    blink_on: bool,

    /// The pseudo-terminal the child runs on the far side of.
    ///
    /// `pty.rs` is two thousand lines of working PTY -- master and slave ends,
    /// a cooked-mode line discipline, back-pressure, window size -- and it was
    /// behind `#[allow(dead_code)]` with no caller at all: the emulator had no
    /// child to talk to, so every key it so carefully translated into an escape
    /// sequence was translated and dropped. `None` only if the pair cannot be
    /// opened, which leaves a terminal that still draws and still scrolls.
    pty: Option<pty::PtyPair>,

    /// Whether the child has closed its end and been reported as gone.
    child_finished: bool,

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

    /// Saved cursor attributes (DECSC/DECRC).
    saved_attrs: CellAttrs,
    /// Saved cursor row.
    saved_row: usize,
    /// Saved cursor col.
    saved_col: usize,
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
            pty: pty::PtyPair::open_with_size(u16_of(cols), u16_of(rows)).ok(),
            child_finished: false,
            bell_flash_ms: 0,
            blink_ms: 0,
            blink_on: true,
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
            saved_attrs: CellAttrs::default(),
            saved_row: 0,
            saved_col: 0,
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
    /// data (e.g., device status reports) is appended to `self.output_buffer`.
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

        // Write the character to the cell
        if let Some(line) = self.screen.get_mut(self.cursor_row)
            && let Some(cell) = line.cells.get_mut(self.cursor_col)
        {
            cell.ch = ch;
            cell.attrs = self.current_attrs;
        }

        // Advance cursor
        if self.cursor_col >= cols.saturating_sub(1) {
            if self.auto_wrap {
                self.pending_wrap = true;
            }
            // Cursor stays at the right margin
        } else {
            self.cursor_col = self.cursor_col.saturating_add(1);
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
        if self.config.cursor_blink && self.cursor_visible {
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
        changed
    }

    /// Send bytes to the child.
    ///
    /// Also the one place that decides what happens to them when there is no
    /// child: they stay in `output_buffer`, which is what the tests read, so
    /// the translation is observable either way.
    pub fn to_child(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.output_buffer.extend_from_slice(bytes);
        if let Some(pair) = self.pty.as_ref() {
            // A short write is the channel being full, which is the child not
            // keeping up rather than an error: the rest is dropped, exactly as
            // a real terminal drops input a full tty cannot take.
            let _ = pair.master.write(bytes);
        }
    }

    /// Take whatever the child has written and feed it to the parser.
    ///
    /// Returns whether anything arrived.
    pub fn drain_child(&mut self) -> bool {
        let Some(pair) = self.pty.as_ref() else {
            return false;
        };
        if pair.master.available() == 0 {
            // Nothing readable. Ask separately whether that is "nothing yet" or
            // "never again": a shell that has exited leaves a terminal that
            // must stop waiting for it, and an empty read alone cannot tell the
            // two apart.
            if pair.master.child_finished() && !self.child_finished {
                self.child_finished = true;
                self.feed(b"\r\n[the child has exited]\r\n");
                return true;
            }
            return false;
        }
        let mut got = Vec::new();
        let mut buf = [0_u8; 4096];
        // Bounded: a child writing faster than the terminal reads must not hold
        // the frame for the whole of its output.
        for _ in 0..16 {
            match pair.master.try_read(&mut buf) {
                Ok(Some(n)) if n > 0 => got.extend_from_slice(buf.get(..n).unwrap_or_default()),
                _ => break,
            }
        }
        if got.is_empty() {
            return false;
        }
        self.feed(&got);
        true
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

    fn backspace(&mut self) {
        self.pending_wrap = false;
        if self.cursor_col > 0 {
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
        self.saved_row = self.cursor_row;
        self.saved_col = self.cursor_col;
        self.saved_attrs = self.current_attrs;
    }

    fn restore_cursor(&mut self) {
        self.cursor_row = self.saved_row;
        self.cursor_col = self.saved_col;
        self.current_attrs = self.saved_attrs;
        self.clamp_cursor();
    }

    fn full_reset(&mut self) {
        let config = self.config.clone();
        *self = Self::new(config);
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
                        self.output_buffer.extend_from_slice(b"\x1b[0n");
                    }
                    6 => {
                        // Cursor position report
                        let report = format!(
                            "\x1b[{};{}R",
                            self.cursor_row.saturating_add(1),
                            self.cursor_col.saturating_add(1)
                        );
                        self.output_buffer.extend_from_slice(report.as_bytes());
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
                self.output_buffer.extend_from_slice(b"\x1b[?62;c");
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

        let old_rows = self.config.rows;
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

        // Resize screen lines
        for line in &mut self.screen {
            line.resize(new_cols);
        }

        // Add or remove rows
        if new_rows > old_rows {
            // Pull lines back from scrollback if available
            let extra = new_rows.saturating_sub(old_rows);
            for _ in 0..extra {
                if let Some(line) = self.scrollback.pop_back() {
                    let mut resized = line;
                    resized.resize(new_cols);
                    self.screen.insert(0, resized);
                    // Adjust cursor row to keep it in place
                    if self.cursor_row < new_rows.saturating_sub(1) {
                        self.cursor_row = self.cursor_row.saturating_add(1);
                    }
                } else {
                    self.screen.push(TermLine::new(new_cols));
                }
            }
        } else if new_rows < old_rows {
            // Push excess lines to scrollback
            let excess = old_rows.saturating_sub(new_rows);
            for _ in 0..excess {
                if self.screen.len() > new_rows {
                    let line = self.screen.remove(0);
                    if !self.alt_screen_active {
                        self.scrollback.push_back(line);
                        if self.scrollback.len() > self.config.scrollback_limit {
                            self.scrollback.pop_front();
                            self.forget_oldest_row();
                        }
                    }
                    self.cursor_row = self.cursor_row.saturating_sub(1);
                }
            }
        }

        // Ensure screen has exactly new_rows lines
        while self.screen.len() < new_rows {
            self.screen.push(TermLine::new(new_cols));
        }
        self.screen.truncate(new_rows);

        // Resize alt screen
        for line in &mut self.alt_screen {
            line.resize(new_cols);
        }
        while self.alt_screen.len() < new_rows {
            self.alt_screen.push(TermLine::new(new_cols));
        }
        self.alt_screen.truncate(new_rows);

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

        self.draw_cells(&mut f, &l);
        self.draw_cursor(&mut f, &l);
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

                if cell.ch != ' ' {
                    let font_weight = if cell.attrs.bold {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    };
                    glyph(
                        f,
                        x,
                        y,
                        cell.ch,
                        fg_color,
                        self.config.font_size,
                        font_weight,
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

        match self.cursor_style {
            CursorStyle::Block => {
                fill(
                    f,
                    Rect::new(cx, cy, l.cell_w, l.cell_h),
                    Color::rgba(cursor_color.r, cursor_color.g, cursor_color.b, 180),
                );
                if let Some(line) = self.screen.get(self.cursor_row)
                    && let Some(cell) = line.cells.get(self.cursor_col)
                    && cell.ch != ' '
                {
                    glyph(
                        f,
                        cx,
                        cy,
                        cell.ch,
                        scheme.background,
                        self.config.font_size,
                        FontWeightHint::Regular,
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

    /// Handle a guitk event. Returns bytes to send to the child process (if any).
    pub fn handle_event(&mut self, event: &Event) -> Vec<u8> {
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
            Event::Tick { elapsed_ms, .. } => {
                self.tick(*elapsed_ms);
                self.drain_child();
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
            // The child is told too. `PtyMaster::resize` is the `TIOCSWINSZ`
            // of this tree and had no caller either, so a shell running under
            // this terminal would have kept wrapping its prompt at eighty
            // columns in a window twice that wide.
            if let Some(pair) = self.pty.as_ref() {
                pair.master.resize(u16_of(l.cols), u16_of(l.rows));
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
fn glyph(
    f: &mut Frame<Target>,
    x: f32,
    y: f32,
    ch: char,
    color: Color,
    font_size: f32,
    font_weight: FontWeightHint,
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
        max_width: Some(text::measure("W", font_size, font_weight).max(font_size)),
        overflow: TextOverflow::Clip,
    });
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
    fn title(&self) -> String {
        self.title.clone()
    }

    fn app_id(&self) -> String {
        String::from("terminal")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<std::time::Duration> {
        // Asked for only while something is actually moving. A terminal at a
        // prompt with a solid cursor has nothing to age, and a program that
        // asks for a clock it does not need holds the whole desktop awake.
        let blinking = self.config.cursor_blink && self.cursor_visible;
        if blinking || self.bell_flash_ms > 0 {
            Some(std::time::Duration::from_millis(BLINK_MS / 5))
        } else {
            None
        }
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        // A tick that changed nothing must not ask for a frame: the cursor
        // blinks five times a second and the loop runs twenty-five, so four
        // ticks in five have nothing to show.
        if let Event::Tick { elapsed_ms, .. } = event {
            return if self.tick(*elapsed_ms) {
                Response::Redraw
            } else {
                Response::Idle
            };
        }
        let out = self.handle_event(event);
        self.output_buffer.extend_from_slice(&out);
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
// Main entry point
// ============================================================================

fn main() -> ExitCode {
    let mut terminal = TerminalState::new(TerminalConfig::default());
    terminal.feed(b"\x1b[1;32mWelcome to Slate OS Terminal\x1b[0m\r\n$ ");
    app::launch("terminal", &mut terminal)
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

    use super::{TerminalConfig, TerminalState};
    use guitk::event::{MouseEvent, MouseEventKind};

    /// A terminal with `lines` lines already pushed off the top, so there is
    /// something to scroll back into.
    fn scrolled_terminal(lines: usize) -> TerminalState {
        let mut term = TerminalState::new(TerminalConfig {
            rows: 4,
            ..TerminalConfig::default()
        });
        for i in 0..lines {
            term.feed(
                format!(
                    "line {i}
"
                )
                .as_bytes(),
            );
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
}

//! OurOS Terminal Emulator
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
//! Renders via the guitk RenderTree, producing Text and FillRect commands
//! for each visible cell in the terminal grid.

#[allow(dead_code)]
pub mod pty;

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};

use std::collections::VecDeque;

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

    /// Visual bell state (frames remaining for flash).
    bell_flash_remaining: u32,

    /// Current text selection.
    pub selection: Option<Selection>,

    /// Scroll offset for viewing scrollback (0 = bottom, >0 = scrolled up).
    pub scroll_offset: usize,

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
            bell_flash_remaining: 0,
            selection: None,
            scroll_offset: 0,
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
            ParserState::Utf8 { remaining, codepoint } => {
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
                self.parser_state = ParserState::Utf8 { remaining: 1, codepoint };
            }
            0xE0..=0xEF => {
                let codepoint = (byte as u32) & 0x0F;
                self.parser_state = ParserState::Utf8 { remaining: 2, codepoint };
            }
            0xF0..=0xF7 => {
                let codepoint = (byte as u32) & 0x07;
                self.parser_state = ParserState::Utf8 { remaining: 3, codepoint };
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
                self.csi_current_param = self.csi_current_param
                    .saturating_mul(10)
                    .saturating_add((byte - b'0') as u16);
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

        let codepoint = (codepoint << 6) | (byte as u32 & 0x3F);
        let remaining = remaining - 1;

        if remaining == 0 {
            let ch = char::from_u32(codepoint).unwrap_or('\u{FFFD}');
            self.put_char(ch);
            self.parser_state = ParserState::Ground;
        } else {
            self.parser_state = ParserState::Utf8 { remaining, codepoint };
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
            && let Some(cell) = line.cells.get_mut(self.cursor_col) {
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
            self.cursor_col += 1;
        }
    }

    // ========================================================================
    // Control character handlers
    // ========================================================================

    fn bell(&mut self) {
        self.bell_flash_remaining = 6; // ~100ms at 60fps
    }

    fn backspace(&mut self) {
        self.pending_wrap = false;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    fn tab(&mut self) {
        let cols = self.cols();
        let start = self.cursor_col + 1;
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
            self.cursor_row += 1;
        }
    }

    /// Move the cursor up one line, scrolling if at the top of the scroll region.
    fn reverse_index(&mut self) {
        if self.cursor_row == self.scroll_top {
            self.scroll_down(1);
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
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
                && let Some(line) = self.screen.first() {
                    self.scrollback.push_back(line.clone());
                    if self.scrollback.len() > self.config.scrollback_limit {
                        self.scrollback.pop_front();
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
                self.cursor_row = (self.cursor_row + n).min(max_row);
                self.pending_wrap = false;
            }
            (b'C', None) => {
                // CUF — Cursor Forward
                let n = Self::param_or(params, 0, 1) as usize;
                let max_col = self.cols().saturating_sub(1);
                self.cursor_col = (self.cursor_col + n).min(max_col);
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
                self.cursor_row = (self.cursor_row + n).min(max_row);
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
                self.cursor_row = (base_row + row.saturating_sub(1))
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
                            self.cursor_row + 1,
                            self.cursor_col + 1
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
            (b'q', None) if !self.csi_intermediates.is_empty()
                && self.csi_intermediates[0] == b' ' =>
            {
                let style = Self::param_or(params, 0, 1);
                self.cursor_style = match style {
                    0 | 1 => CursorStyle::Block,    // blinking block
                    2 => CursorStyle::Block,         // steady block
                    3 => CursorStyle::Underline,     // blinking underline
                    4 => CursorStyle::Underline,     // steady underline
                    5 => CursorStyle::Bar,           // blinking bar
                    6 => CursorStyle::Bar,           // steady bar
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
        params.get(index).copied().filter(|&v| v != 0).unwrap_or(default)
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

        let mut i = 0;
        while i < params.len() {
            let p = params[i];
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
                    self.current_attrs.fg = TermColor::Indexed((p - 30) as u8);
                }
                // Default foreground
                39 => self.current_attrs.fg = TermColor::Default,
                // Background colors (40-47)
                40..=47 => {
                    self.current_attrs.bg = TermColor::Indexed((p - 40) as u8);
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
                    self.current_attrs.fg = TermColor::Indexed((p - 90 + 8) as u8);
                }
                // Bright background colors (100-107)
                100..=107 => {
                    self.current_attrs.bg = TermColor::Indexed((p - 100 + 8) as u8);
                }

                _ => {} // Unrecognized SGR parameter — ignore
            }
            i += 1;
        }
    }

    /// Parse an extended color (256-color or truecolor) from SGR params.
    /// Advances `i` past the consumed parameters.
    fn parse_extended_color(&self, params: &[u16], i: &mut usize) -> Option<TermColor> {
        let next = *i + 1;
        if next >= params.len() {
            return None;
        }
        match params[next] {
            5 => {
                // 256-color: 38;5;N or 48;5;N
                let color_idx = next + 1;
                if color_idx < params.len() {
                    *i = color_idx;
                    Some(TermColor::Indexed(params[color_idx] as u8))
                } else {
                    *i = next;
                    None
                }
            }
            2 => {
                // Truecolor: 38;2;R;G;B or 48;2;R;G;B
                let r_idx = next + 1;
                let g_idx = next + 2;
                let b_idx = next + 3;
                if b_idx < params.len() {
                    let r = params[r_idx] as u8;
                    let g = params[g_idx] as u8;
                    let b = params[b_idx] as u8;
                    *i = b_idx;
                    Some(TermColor::Rgb(r, g, b))
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
                for row in (self.cursor_row + 1)..rows {
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
            for c in 0..count {
                let target = col + c;
                if target < cols
                    && let Some(cell) = line.cells.get_mut(target) {
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
        if let Some(semicolon_pos) = osc.find(';') {
            let cmd_str = &osc[..semicolon_pos];
            let text = &osc[semicolon_pos + 1..];
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
                && !*stop {
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
            let extra = new_rows - old_rows;
            for _ in 0..extra {
                if let Some(line) = self.scrollback.pop_back() {
                    let mut resized = line;
                    resized.resize(new_cols);
                    self.screen.insert(0, resized);
                    // Adjust cursor row to keep it in place
                    if self.cursor_row < new_rows.saturating_sub(1) {
                        self.cursor_row += 1;
                    }
                } else {
                    self.screen.push(TermLine::new(new_cols));
                }
            }
        } else if new_rows < old_rows {
            // Push excess lines to scrollback
            let excess = old_rows - new_rows;
            for _ in 0..excess {
                if self.screen.len() > new_rows {
                    let line = self.screen.remove(0);
                    if !self.alt_screen_active {
                        self.scrollback.push_back(line);
                        if self.scrollback.len() > self.config.scrollback_limit {
                            self.scrollback.pop_front();
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
        if let Some(ch) = event.text
            && !mods.ctrl && !mods.alt {
                let mut buf = [0u8; 4];
                let encoded = ch.encode_utf8(&mut buf);
                return encoded.as_bytes().to_vec();
            }

        // Ctrl+letter produces control characters (^A = 0x01, ^Z = 0x1A, etc.)
        if mods.ctrl && !mods.alt
            && let Some(code) = self.ctrl_key_code(&event.key) {
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
        let mut code: u8 = 1;
        if mods.shift {
            code += 1;
        }
        if mods.alt {
            code += 2;
        }
        if mods.ctrl {
            code += 4;
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
            Key::LeftBracket => Some(0x1B),  // Ctrl+[ = ESC
            Key::Backslash => Some(0x1C),
            Key::RightBracket => Some(0x1D),
            _ => None,
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
            let line = if row < self.screen.len() {
                &self.screen[row]
            } else {
                continue;
            };

            let col_start = if row == start_row { start_col } else { 0 };
            let col_end = if row == end_row {
                end_col + 1
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

    /// Convert pixel coordinates to cell row/column.
    fn pixel_to_cell(&self, px: f32, py: f32) -> (usize, usize) {
        let row = (py / self.config.cell_height) as usize;
        let col = (px / self.config.cell_width) as usize;
        let row = row.min(self.rows().saturating_sub(1));
        let col = col.min(self.cols().saturating_sub(1));
        (row, col)
    }

    /// Check if a given cell is within the current selection.
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
        self.scroll_offset = (self.scroll_offset + lines).min(max);
    }

    /// Scroll the viewport down toward current content.
    pub fn scroll_viewport_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the terminal to a guitk RenderTree.
    ///
    /// Produces FillRect commands for cell backgrounds and Text commands
    /// for each non-empty cell. Also renders the cursor and visual bell.
    pub fn render(&mut self, tree: &mut RenderTree) {
        let rows = self.rows();
        let cols = self.cols();
        let cw = self.config.cell_width;
        let ch = self.config.cell_height;
        let scheme = &self.config.colors;

        // Background fill
        tree.fill_rect(
            0.0,
            0.0,
            cols as f32 * cw,
            rows as f32 * ch,
            scheme.background,
        );

        // Visual bell flash
        if self.bell_flash_remaining > 0 {
            tree.fill_rect(
                0.0,
                0.0,
                cols as f32 * cw,
                rows as f32 * ch,
                Color::rgba(255, 255, 255, 30),
            );
            self.bell_flash_remaining -= 1;
        }

        // Render each visible row
        for screen_row in 0..rows {
            let line = if self.scroll_offset > 0 {
                // Viewing scrollback
                let scrollback_len = self.scrollback.len();
                let offset_from_top = scrollback_len
                    .saturating_sub(self.scroll_offset)
                    + screen_row;
                if offset_from_top < scrollback_len {
                    // Drawing from scrollback
                    if let Some(l) = self.scrollback.get(offset_from_top) {
                        l
                    } else {
                        continue;
                    }
                } else {
                    // Drawing from current screen
                    let screen_idx = offset_from_top - scrollback_len;
                    if let Some(l) = self.screen.get(screen_idx) {
                        l
                    } else {
                        continue;
                    }
                }
            } else {
                if let Some(l) = self.screen.get(screen_row) {
                    l
                } else {
                    continue;
                }
            };

            for col in 0..cols.min(line.cells.len()) {
                let cell = &line.cells[col];
                let x = col as f32 * cw;
                let y = screen_row as f32 * ch;

                let selected = self.is_selected(screen_row, col);

                // Resolve colors
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
                    fg_color = Color::rgba(
                        fg_color.r / 2,
                        fg_color.g / 2,
                        fg_color.b / 2,
                        fg_color.a,
                    );
                }

                // Draw background if not default
                if bg_color != scheme.background || selected {
                    tree.fill_rect(x, y, cw, ch, bg_color);
                }

                // Draw character
                if cell.ch != ' ' {
                    let font_weight = if cell.attrs.bold {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    };

                    tree.push(RenderCommand::Text {
                        x,
                        y,
                        text: cell.ch.to_string(),
                        color: fg_color,
                        font_size: self.config.font_size,
                        font_weight,
                        max_width: Some(cw),
                    });
                }

                // Underline
                if cell.attrs.underline {
                    let underline_y = y + ch - 2.0;
                    tree.push(RenderCommand::Line {
                        x1: x,
                        y1: underline_y,
                        x2: x + cw,
                        y2: underline_y,
                        color: fg_color,
                        width: 1.0,
                    });
                }

                // Strikethrough
                if cell.attrs.strikethrough {
                    let strike_y = y + ch / 2.0;
                    tree.push(RenderCommand::Line {
                        x1: x,
                        y1: strike_y,
                        x2: x + cw,
                        y2: strike_y,
                        color: fg_color,
                        width: 1.0,
                    });
                }
            }
        }

        // Draw cursor
        if self.cursor_visible && self.scroll_offset == 0 {
            let cx = self.cursor_col as f32 * cw;
            let cy = self.cursor_row as f32 * ch;
            let cursor_color = scheme.cursor;

            match self.cursor_style {
                CursorStyle::Block => {
                    tree.fill_rect(cx, cy, cw, ch, Color::rgba(
                        cursor_color.r, cursor_color.g, cursor_color.b, 180,
                    ));
                    // Re-draw the character under the cursor in inverse
                    if let Some(line) = self.screen.get(self.cursor_row)
                        && let Some(cell) = line.cells.get(self.cursor_col)
                            && cell.ch != ' ' {
                                tree.push(RenderCommand::Text {
                                    x: cx,
                                    y: cy,
                                    text: cell.ch.to_string(),
                                    color: scheme.background,
                                    font_size: self.config.font_size,
                                    font_weight: FontWeightHint::Regular,
                                    max_width: Some(cw),
                                });
                            }
                }
                CursorStyle::Underline => {
                    let uy = cy + ch - 2.0;
                    tree.push(RenderCommand::Line {
                        x1: cx,
                        y1: uy,
                        x2: cx + cw,
                        y2: uy,
                        color: cursor_color,
                        width: 2.0,
                    });
                }
                CursorStyle::Bar => {
                    tree.push(RenderCommand::Line {
                        x1: cx,
                        y1: cy,
                        x2: cx,
                        y2: cy + ch,
                        color: cursor_color,
                        width: 2.0,
                    });
                }
            }
        }
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
                        let effective_idx = if bold && is_foreground { idx + 8 } else { idx };
                        scheme.ansi[effective_idx as usize]
                    }
                    8..=15 => scheme.ansi[idx as usize],
                    16..=231 => {
                        // 6x6x6 color cube
                        let idx = idx - 16;
                        let b_component = idx % 6;
                        let g_component = (idx / 6) % 6;
                        let r_component = idx / 36;
                        let r = if r_component > 0 { r_component * 40 + 55 } else { 0 };
                        let g = if g_component > 0 { g_component * 40 + 55 } else { 0 };
                        let b = if b_component > 0 { b_component * 40 + 55 } else { 0 };
                        Color::rgb(r, g, b)
                    }
                    232..=255 => {
                        // Grayscale ramp (24 shades)
                        let shade = (idx - 232) * 10 + 8;
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
            Event::Key(key_event) => self.translate_key(key_event),
            Event::Mouse(mouse_event) => {
                self.handle_mouse(mouse_event);
                Vec::new()
            }
            Event::Resize { width, height } => {
                let new_cols = (*width as f32 / self.config.cell_width) as usize;
                let new_rows = (*height as f32 / self.config.cell_height) as usize;
                if new_cols > 0 && new_rows > 0 {
                    self.resize(new_cols, new_rows);
                }
                Vec::new()
            }
            Event::Tick { .. } => {
                // Tick can be used for cursor blink animation
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Handle mouse events (selection and scroll).
    fn handle_mouse(&mut self, event: &MouseEvent) {
        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                self.clear_selection();
                self.selection_start(event.x, event.y);
            }
            MouseEventKind::Move => {
                if let Some(ref sel) = self.selection
                    && sel.active {
                        self.selection_extend(event.x, event.y);
                    }
            }
            MouseEventKind::Release(MouseButton::Left) => {
                self.selection_end();
            }
            MouseEventKind::Scroll { dy, .. } => {
                if *dy > 0.0 {
                    self.scroll_viewport_up(3);
                } else if *dy < 0.0 {
                    self.scroll_viewport_down(3);
                }
            }
            _ => {}
        }
    }
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let config = TerminalConfig::default();
    let mut terminal = TerminalState::new(config);
    let mut render_tree = RenderTree::new();

    // Main event loop placeholder.
    // In the real system, this would:
    // 1. Connect to a child process via PTY
    // 2. Poll for events from the compositor (guitk events)
    // 3. Poll for output from the child process
    // 4. Feed child output into terminal.feed()
    // 5. Send terminal.translate_key() output to the child
    // 6. Render each frame via terminal.render()
    //
    // For now, demonstrate basic functionality:

    // Simulate some input to prove the terminal works
    let demo_text = b"\x1b[1;32mWelcome to OurOS Terminal\x1b[0m\r\n$ ";
    terminal.feed(demo_text);

    // Render a frame
    render_tree.clear();
    terminal.render(&mut render_tree);

    // In a real event loop, we would submit render_tree to the compositor
    // and wait for the next frame. The loop would look like:
    //
    // loop {
    //     // Read events from compositor
    //     let event = compositor.poll_event();
    //     let response = terminal.handle_event(&event);
    //     if !response.is_empty() {
    //         child_process.write(&response);
    //     }
    //
    //     // Read output from child
    //     if let Some(data) = child_process.try_read() {
    //         terminal.feed(&data);
    //     }
    //
    //     // Send any output_buffer responses to child
    //     if !terminal.output_buffer.is_empty() {
    //         child_process.write(&terminal.output_buffer);
    //         terminal.output_buffer.clear();
    //     }
    //
    //     // Render
    //     render_tree.clear();
    //     terminal.render(&mut render_tree);
    //     compositor.submit_frame(&render_tree);
    // }
}

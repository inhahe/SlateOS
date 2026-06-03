//! `OurOS` Terminal Multiplexer (tmux)
//!
//! A graphical terminal multiplexer that allows splitting a terminal window
//! into multiple panes, creating tabbed windows, and detaching/reattaching
//! sessions. Based on the design spec's requirement for tmux-like functionality
//! built into the OS's terminal infrastructure.
//!
//! Features:
//! - Split panes (horizontal and vertical) with configurable ratios
//! - Multiple windows (tabs) per session
//! - Session detach/reattach (sessions persist in background)
//! - Pane navigation with keyboard shortcuts (Ctrl+B prefix)
//! - Pane resize with arrow keys
//! - Configurable layouts (even-horizontal, even-vertical, main-horizontal, etc.)
//! - Status bar with window list, clock, and session name
//! - Copy mode for scrollback buffer selection
//! - VT100/ANSI sequence passthrough per pane
//! - Visual bell indicator
//!
//! Uses the guitk library for UI rendering.

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::cognitive_complexity)]
#![allow(dead_code)]

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha theme
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1200.0;
const WINDOW_HEIGHT: f32 = 800.0;
const STATUS_BAR_HEIGHT: f32 = 22.0;
const TAB_BAR_HEIGHT: f32 = 28.0;
const BORDER_WIDTH: f32 = 1.0;
const PANE_BORDER_WIDTH: f32 = 1.0;
const CHAR_WIDTH: f32 = 8.0;
const CHAR_HEIGHT: f32 = 16.0;
const PADDING: f32 = 4.0;
const SMALL_TEXT: f32 = 11.0;
const NORMAL_TEXT: f32 = 13.0;
const HEADER_TEXT: f32 = 14.0;
const MIN_PANE_SIZE: f32 = 40.0;
const RESIZE_STEP: f32 = 20.0;

const MAX_SESSIONS: usize = 64;
const MAX_WINDOWS: usize = 32;
const MAX_PANES: usize = 32;
const MAX_SCROLLBACK: usize = 10_000;
const MAX_COLS: usize = 400;
const MAX_ROWS: usize = 200;

// ============================================================================
// Terminal cell
// ============================================================================

/// A single character cell in the terminal grid.
#[derive(Debug, Clone, Copy)]
struct Cell {
    /// The character displayed in this cell.
    ch: char,
    /// Foreground color.
    fg: Color,
    /// Background color.
    bg: Color,
    /// Whether the cell is bold.
    bold: bool,
    /// Whether the cell is dimmed.
    dim: bool,
    /// Whether the cell is underlined.
    underline: bool,
    /// Whether the cell is in reverse video.
    reverse: bool,
}

impl Cell {
    fn blank() -> Self {
        Self {
            ch: ' ',
            fg: TEXT,
            bg: BASE,
            bold: false,
            dim: false,
            underline: false,
            reverse: false,
        }
    }
}

// ============================================================================
// Terminal buffer
// ============================================================================

/// The terminal screen buffer for a single pane.
#[derive(Debug, Clone)]
struct TerminalBuffer {
    /// Grid of cells (rows × cols).
    cells: Vec<Vec<Cell>>,
    /// Number of columns.
    cols: usize,
    /// Number of rows.
    rows: usize,
    /// Cursor position (col, row), 0-indexed.
    cursor_col: usize,
    cursor_row: usize,
    /// Whether the cursor is visible.
    cursor_visible: bool,
    /// Scrollback buffer (older lines scrolled off the top).
    scrollback: Vec<Vec<Cell>>,
    /// Current scroll position in scrollback (0 = no scrollback visible).
    scroll_offset: usize,
    /// SGR state for new characters.
    current_fg: Color,
    current_bg: Color,
    current_bold: bool,
    current_dim: bool,
    current_underline: bool,
    current_reverse: bool,
    /// Title set via OSC escape.
    title: String,
    /// Whether the visual bell was triggered.
    bell: bool,
}

impl TerminalBuffer {
    fn new(cols: usize, rows: usize) -> Self {
        let cells = vec![vec![Cell::blank(); cols]; rows];
        Self {
            cells,
            cols,
            rows,
            cursor_col: 0,
            cursor_row: 0,
            cursor_visible: true,
            scrollback: Vec::new(),
            scroll_offset: 0,
            current_fg: TEXT,
            current_bg: BASE,
            current_bold: false,
            current_dim: false,
            current_underline: false,
            current_reverse: false,
            title: String::new(),
            bell: false,
        }
    }

    /// Resize the buffer to new dimensions.
    fn resize(&mut self, new_cols: usize, new_rows: usize) {
        let new_cols = new_cols.clamp(1, MAX_COLS);
        let new_rows = new_rows.clamp(1, MAX_ROWS);

        // Resize existing rows
        for row in &mut self.cells {
            row.resize(new_cols, Cell::blank());
        }
        // Add or remove rows
        self.cells.resize(new_rows, vec![Cell::blank(); new_cols]);

        self.cols = new_cols;
        self.rows = new_rows;

        // Clamp cursor
        if self.cursor_col >= new_cols {
            self.cursor_col = new_cols.saturating_sub(1);
        }
        if self.cursor_row >= new_rows {
            self.cursor_row = new_rows.saturating_sub(1);
        }
    }

    /// Write a character at the current cursor position and advance.
    fn write_char(&mut self, ch: char) {
        if ch == '\n' {
            self.newline();
            return;
        }
        if ch == '\r' {
            self.cursor_col = 0;
            return;
        }
        if ch == '\x08' {
            // Backspace
            self.cursor_col = self.cursor_col.saturating_sub(1);
            return;
        }
        if ch == '\x07' {
            // Bell
            self.bell = true;
            return;
        }
        if ch == '\t' {
            // Tab: advance to next 8-column stop
            let next_tab = (self.cursor_col / 8 + 1) * 8;
            self.cursor_col = next_tab.min(self.cols.saturating_sub(1));
            return;
        }

        if self.cursor_col >= self.cols {
            self.newline();
        }

        if let Some(row) = self.cells.get_mut(self.cursor_row)
            && let Some(cell) = row.get_mut(self.cursor_col)
        {
            cell.ch = ch;
            cell.fg = self.current_fg;
            cell.bg = self.current_bg;
            cell.bold = self.current_bold;
            cell.dim = self.current_dim;
            cell.underline = self.current_underline;
            cell.reverse = self.current_reverse;
        }
        self.cursor_col = self.cursor_col.saturating_add(1);
    }

    /// Move to the next line, scrolling if necessary.
    fn newline(&mut self) {
        self.cursor_col = 0;
        if self.cursor_row.saturating_add(1) >= self.rows {
            self.scroll_up();
        } else {
            self.cursor_row = self.cursor_row.saturating_add(1);
        }
    }

    /// Scroll the buffer up by one line, pushing the top line to scrollback.
    fn scroll_up(&mut self) {
        if !self.cells.is_empty() {
            let top_line = self.cells.remove(0);
            self.scrollback.push(top_line);
            // Cap scrollback
            if self.scrollback.len() > MAX_SCROLLBACK {
                self.scrollback.remove(0);
            }
            self.cells.push(vec![Cell::blank(); self.cols]);
        }
    }

    /// Clear the entire screen.
    fn clear(&mut self) {
        for row in &mut self.cells {
            for cell in row.iter_mut() {
                *cell = Cell::blank();
            }
        }
        self.cursor_col = 0;
        self.cursor_row = 0;
    }

    /// Clear from cursor to end of screen.
    fn clear_to_end(&mut self) {
        // Clear rest of current line
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            for col in self.cursor_col..self.cols {
                if let Some(cell) = row.get_mut(col) {
                    *cell = Cell::blank();
                }
            }
        }
        // Clear all lines below
        for r in self.cursor_row.saturating_add(1)..self.rows {
            if let Some(row) = self.cells.get_mut(r) {
                for cell in row.iter_mut() {
                    *cell = Cell::blank();
                }
            }
        }
    }

    /// Clear from start of screen to cursor.
    fn clear_to_start(&mut self) {
        // Clear lines above current
        for r in 0..self.cursor_row {
            if let Some(row) = self.cells.get_mut(r) {
                for cell in row.iter_mut() {
                    *cell = Cell::blank();
                }
            }
        }
        // Clear current line up to cursor
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            for col in 0..=self.cursor_col.min(self.cols.saturating_sub(1)) {
                if let Some(cell) = row.get_mut(col) {
                    *cell = Cell::blank();
                }
            }
        }
    }

    /// Clear current line.
    fn clear_line(&mut self) {
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            for cell in row.iter_mut() {
                *cell = Cell::blank();
            }
        }
    }

    /// Erase from cursor to end of line.
    fn clear_line_to_end(&mut self) {
        if let Some(row) = self.cells.get_mut(self.cursor_row) {
            for col in self.cursor_col..self.cols {
                if let Some(cell) = row.get_mut(col) {
                    *cell = Cell::blank();
                }
            }
        }
    }

    /// Set cursor position (1-indexed input, stored 0-indexed).
    fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.saturating_sub(1).min(self.rows.saturating_sub(1));
        self.cursor_col = col.saturating_sub(1).min(self.cols.saturating_sub(1));
    }

    /// Reset SGR attributes.
    fn reset_attrs(&mut self) {
        self.current_fg = TEXT;
        self.current_bg = BASE;
        self.current_bold = false;
        self.current_dim = false;
        self.current_underline = false;
        self.current_reverse = false;
    }

    /// Write a string to the buffer, handling basic control characters.
    fn write_str(&mut self, s: &str) {
        for ch in s.chars() {
            self.write_char(ch);
        }
    }

    /// Get the effective foreground/background for a cell, applying reverse video.
    fn effective_colors(cell: &Cell) -> (Color, Color) {
        if cell.reverse {
            (cell.bg, cell.fg)
        } else {
            (cell.fg, cell.bg)
        }
    }
}

// ============================================================================
// ANSI/CSI parser
// ============================================================================

/// Parse state for ANSI escape sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    /// Normal character output.
    Normal,
    /// Received ESC, waiting for '[' or other.
    Escape,
    /// Inside a CSI sequence (ESC [), collecting parameters.
    Csi,
    /// Inside an OSC sequence (ESC ]).
    Osc,
}

/// ANSI sequence parser that processes byte streams and applies effects
/// to a `TerminalBuffer`.
#[derive(Debug, Clone)]
struct AnsiParser {
    state: ParseState,
    /// CSI parameter accumulator.
    params: Vec<u16>,
    /// Current param being built.
    current_param: u16,
    /// Whether we've started building a param digit.
    has_param: bool,
    /// Intermediate bytes (e.g., '?' for private modes).
    intermediate: Option<char>,
    /// OSC string accumulator.
    osc_string: String,
}

impl AnsiParser {
    fn new() -> Self {
        Self {
            state: ParseState::Normal,
            params: Vec::new(),
            current_param: 0,
            has_param: false,
            intermediate: None,
            osc_string: String::new(),
        }
    }

    /// Feed a string into the parser, applying effects to the buffer.
    fn feed(&mut self, input: &str, buf: &mut TerminalBuffer) {
        for ch in input.chars() {
            match self.state {
                ParseState::Normal => {
                    if ch == '\x1B' {
                        self.state = ParseState::Escape;
                    } else {
                        buf.write_char(ch);
                    }
                }
                ParseState::Escape => {
                    match ch {
                        '[' => {
                            self.state = ParseState::Csi;
                            self.params.clear();
                            self.current_param = 0;
                            self.has_param = false;
                            self.intermediate = None;
                        }
                        ']' => {
                            self.state = ParseState::Osc;
                            self.osc_string.clear();
                        }
                        'c' => {
                            // Full reset
                            buf.clear();
                            buf.reset_attrs();
                            self.state = ParseState::Normal;
                        }
                        _ => {
                            self.state = ParseState::Normal;
                        }
                    }
                }
                ParseState::Csi => {
                    self.process_csi_char(ch, buf);
                }
                ParseState::Osc => {
                    if ch == '\x07' || ch == '\x1B' {
                        // OSC terminated by BEL or ESC
                        self.process_osc(buf);
                        self.state = ParseState::Normal;
                    } else {
                        self.osc_string.push(ch);
                    }
                }
            }
        }
    }

    fn process_csi_char(&mut self, ch: char, buf: &mut TerminalBuffer) {
        match ch {
            '0'..='9' => {
                self.current_param = self.current_param.saturating_mul(10)
                    .saturating_add(ch as u16 - u16::from(b'0'));
                self.has_param = true;
            }
            ';' => {
                self.params.push(if self.has_param { self.current_param } else { 0 });
                self.current_param = 0;
                self.has_param = false;
            }
            '?' | '>' | '!' => {
                self.intermediate = Some(ch);
            }
            _ => {
                // Final byte — push last param and dispatch
                if self.has_param || !self.params.is_empty() {
                    self.params.push(if self.has_param { self.current_param } else { 0 });
                }
                self.dispatch_csi(ch, buf);
                self.state = ParseState::Normal;
            }
        }
    }

    fn dispatch_csi(&self, final_ch: char, buf: &mut TerminalBuffer) {
        let p = &self.params;
        let p0 = p.first().copied().unwrap_or(0);
        let p1 = p.get(1).copied().unwrap_or(0);

        match final_ch {
            // Cursor movement
            'A' => {
                // Cursor Up
                let n = (p0.max(1)) as usize;
                buf.cursor_row = buf.cursor_row.saturating_sub(n);
            }
            'B' => {
                // Cursor Down
                let n = (p0.max(1)) as usize;
                buf.cursor_row = (buf.cursor_row.saturating_add(n)).min(buf.rows.saturating_sub(1));
            }
            'C' => {
                // Cursor Forward
                let n = (p0.max(1)) as usize;
                buf.cursor_col = (buf.cursor_col.saturating_add(n)).min(buf.cols.saturating_sub(1));
            }
            'D' => {
                // Cursor Back
                let n = (p0.max(1)) as usize;
                buf.cursor_col = buf.cursor_col.saturating_sub(n);
            }
            'H' | 'f' => {
                // Cursor Position
                let row = p0.max(1) as usize;
                let col = p1.max(1) as usize;
                buf.set_cursor(row, col);
            }
            'J' => {
                // Erase in Display
                match p0 {
                    0 => buf.clear_to_end(),
                    1 => buf.clear_to_start(),
                    2 | 3 => buf.clear(),
                    _ => {}
                }
            }
            'K' => {
                // Erase in Line
                match p0 {
                    0 => buf.clear_line_to_end(),
                    1 => {
                        // Erase to start of line
                        if let Some(row) = buf.cells.get_mut(buf.cursor_row) {
                            for col in 0..=buf.cursor_col.min(buf.cols.saturating_sub(1)) {
                                if let Some(cell) = row.get_mut(col) {
                                    *cell = Cell::blank();
                                }
                            }
                        }
                    }
                    2 => buf.clear_line(),
                    _ => {}
                }
            }
            'm' => {
                // SGR — Select Graphic Rendition
                if p.is_empty() {
                    buf.reset_attrs();
                } else {
                    self.apply_sgr(p, buf);
                }
            }
            'h' | 'l' if self.intermediate == Some('?') && p0 == 25 => {
                // Set/Reset Mode (DECTCEM cursor visibility)
                buf.cursor_visible = final_ch == 'h';
            }
            'r' => {
                // Set scrolling region (simplified — just reset cursor)
                buf.cursor_col = 0;
                buf.cursor_row = 0;
            }
            _ => {
                // Unknown CSI sequence — ignore
            }
        }
    }

    #[allow(clippy::unused_self)]  // kept as method for symmetry with other CSI handlers
    fn apply_sgr(&self, params: &[u16], buf: &mut TerminalBuffer) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => buf.reset_attrs(),
                1 => buf.current_bold = true,
                2 => buf.current_dim = true,
                4 => buf.current_underline = true,
                7 => buf.current_reverse = true,
                22 => { buf.current_bold = false; buf.current_dim = false; }
                24 => buf.current_underline = false,
                27 => buf.current_reverse = false,
                // Standard foreground colors
                30 => buf.current_fg = Color::from_hex(0x45475A),  // Black → Surface1
                31 => buf.current_fg = RED,
                32 => buf.current_fg = GREEN,
                33 => buf.current_fg = YELLOW,
                34 => buf.current_fg = BLUE,
                35 => buf.current_fg = MAUVE,
                36 => buf.current_fg = TEAL,
                37 => buf.current_fg = TEXT,
                39 => buf.current_fg = TEXT,  // Default fg
                // Standard background colors
                40 => buf.current_bg = CRUST,
                41 => buf.current_bg = RED,
                42 => buf.current_bg = GREEN,
                43 => buf.current_bg = YELLOW,
                44 => buf.current_bg = BLUE,
                45 => buf.current_bg = MAUVE,
                46 => buf.current_bg = TEAL,
                47 => buf.current_bg = TEXT,
                49 => buf.current_bg = BASE,  // Default bg
                // Bright foreground
                90 => buf.current_fg = OVERLAY0,
                91 => buf.current_fg = RED,
                92 => buf.current_fg = GREEN,
                93 => buf.current_fg = YELLOW,
                94 => buf.current_fg = BLUE,
                95 => buf.current_fg = MAUVE,
                96 => buf.current_fg = TEAL,
                97 => buf.current_fg = TEXT,
                // 256-color and truecolor
                38 => {
                    if let Some(&2) = params.get(i.saturating_add(1)) {
                        // Truecolor: 38;2;r;g;b
                        if i.saturating_add(4) < params.len() {
                            let r = params[i.saturating_add(2)] as u8;
                            let g_val = params[i.saturating_add(3)] as u8;
                            let b = params[i.saturating_add(4)] as u8;
                            buf.current_fg = Color::rgb(r, g_val, b);
                            i = i.saturating_add(4);
                        }
                    } else if let Some(&5) = params.get(i.saturating_add(1)) {
                        // 256-color: 38;5;n — simplified mapping
                        if let Some(&n) = params.get(i.saturating_add(2)) {
                            buf.current_fg = color_256(n);
                            i = i.saturating_add(2);
                        }
                    }
                }
                48 => {
                    if let Some(&2) = params.get(i.saturating_add(1)) {
                        if i.saturating_add(4) < params.len() {
                            let r = params[i.saturating_add(2)] as u8;
                            let g_val = params[i.saturating_add(3)] as u8;
                            let b = params[i.saturating_add(4)] as u8;
                            buf.current_bg = Color::rgb(r, g_val, b);
                            i = i.saturating_add(4);
                        }
                    } else if let Some(&5) = params.get(i.saturating_add(1))
                        && let Some(&n) = params.get(i.saturating_add(2))
                    {
                        buf.current_bg = color_256(n);
                        i = i.saturating_add(2);
                    }
                }
                _ => {}
            }
            i = i.saturating_add(1);
        }
    }

    fn process_osc(&self, buf: &mut TerminalBuffer) {
        // OSC 0 or 2: set title
        if let Some(rest) = self.osc_string.strip_prefix("0;") {
            buf.title = rest.to_string();
        } else if let Some(rest) = self.osc_string.strip_prefix("2;") {
            buf.title = rest.to_string();
        }
    }
}

/// Map 256-color index to a Color (simplified).
fn color_256(n: u16) -> Color {
    match n {
        0 => CRUST,
        1 => RED,
        2 => GREEN,
        3 => YELLOW,
        4 => BLUE,
        5 => MAUVE,
        6 => TEAL,
        7 => SUBTEXT1,
        8 => OVERLAY0,
        9 => RED,
        10 => GREEN,
        11 => YELLOW,
        12 => BLUE,
        13 => MAUVE,
        14 => TEAL,
        15 => TEXT,
        // 16-231: 6x6x6 color cube
        16..=231 => {
            let idx = n.saturating_sub(16);
            let b = (idx % 6) as u8;
            let g_val = ((idx / 6) % 6) as u8;
            let r = (idx / 36) as u8;
            Color::rgb(
                if r == 0 { 0 } else { r.saturating_mul(40).saturating_add(55) },
                if g_val == 0 { 0 } else { g_val.saturating_mul(40).saturating_add(55) },
                if b == 0 { 0 } else { b.saturating_mul(40).saturating_add(55) },
            )
        }
        // 232-255: grayscale ramp
        232..=255 => {
            let v = ((n.saturating_sub(232)).saturating_mul(10).saturating_add(8)) as u8;
            Color::rgb(v, v, v)
        }
        _ => TEXT,
    }
}

// ============================================================================
// Split direction
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitDir {
    Horizontal,
    Vertical,
}

// ============================================================================
// Pane
// ============================================================================

/// A unique identifier for a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PaneId(usize);

/// A single terminal pane within a window.
#[derive(Debug, Clone)]
struct Pane {
    id: PaneId,
    /// Terminal buffer for this pane.
    buffer: TerminalBuffer,
    /// ANSI parser state.
    parser: AnsiParser,
    /// Process ID running in this pane (0 if none).
    pid: u64,
    /// Command that was launched in this pane.
    command: String,
    /// Whether the pane is active/alive.
    alive: bool,
    /// Pane title (from OSC or command).
    title: String,
    /// Whether this pane is in copy mode (scrollback browsing).
    copy_mode: bool,
    /// Copy mode selection start (col, row in scrollback).
    copy_start: Option<(usize, usize)>,
    /// Copy mode selection end.
    copy_end: Option<(usize, usize)>,
    /// Copy mode scroll position.
    copy_scroll: usize,
}

impl Pane {
    fn new(id: PaneId, cols: usize, rows: usize) -> Self {
        Self {
            id,
            buffer: TerminalBuffer::new(cols, rows),
            parser: AnsiParser::new(),
            pid: 0,
            command: "shell".into(),
            alive: true,
            title: String::new(),
            copy_mode: false,
            copy_start: None,
            copy_end: None,
            copy_scroll: 0,
        }
    }

    /// Feed input data to this pane's terminal.
    fn feed(&mut self, data: &str) {
        self.parser.feed(data, &mut self.buffer);
        if !self.buffer.title.is_empty() {
            self.title = self.buffer.title.clone();
        }
    }

    /// Get effective title (pane title, buffer title, or command).
    fn effective_title(&self) -> &str {
        if !self.title.is_empty() {
            &self.title
        } else if !self.buffer.title.is_empty() {
            &self.buffer.title
        } else {
            &self.command
        }
    }

    /// Enter copy mode for scrollback browsing.
    fn enter_copy_mode(&mut self) {
        self.copy_mode = true;
        self.copy_scroll = 0;
        self.copy_start = None;
        self.copy_end = None;
    }

    /// Exit copy mode.
    fn exit_copy_mode(&mut self) {
        self.copy_mode = false;
        self.copy_start = None;
        self.copy_end = None;
    }
}

// ============================================================================
// Layout tree
// ============================================================================

/// A node in the pane layout tree. Either a leaf (single pane) or a split
/// (two children with a split direction and ratio).
#[derive(Debug, Clone)]
enum LayoutNode {
    Leaf(PaneId),
    Split {
        direction: SplitDir,
        /// Ratio of first child (0.0-1.0).
        ratio: f32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    /// Compute the absolute bounds for each pane in this layout tree.
    fn compute_bounds(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<(PaneId, f32, f32, f32, f32)> {
        match self {
            Self::Leaf(id) => vec![(*id, x, y, width, height)],
            Self::Split { direction, ratio, first, second } => {
                let mut result = Vec::new();
                match direction {
                    SplitDir::Horizontal => {
                        let first_h = (height * ratio).max(MIN_PANE_SIZE);
                        let second_h = (height - first_h - PANE_BORDER_WIDTH).max(MIN_PANE_SIZE);
                        result.extend(first.compute_bounds(x, y, width, first_h));
                        result.extend(second.compute_bounds(x, y + first_h + PANE_BORDER_WIDTH, width, second_h));
                    }
                    SplitDir::Vertical => {
                        let first_w = (width * ratio).max(MIN_PANE_SIZE);
                        let second_w = (width - first_w - PANE_BORDER_WIDTH).max(MIN_PANE_SIZE);
                        result.extend(first.compute_bounds(x, y, first_w, height));
                        result.extend(second.compute_bounds(x + first_w + PANE_BORDER_WIDTH, y, second_w, height));
                    }
                }
                result
            }
        }
    }

    /// Count the number of leaf (pane) nodes.
    fn pane_count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Split { first, second, .. } => {
                first.pane_count().saturating_add(second.pane_count())
            }
        }
    }

    /// Collect all pane IDs in order.
    fn pane_ids(&self) -> Vec<PaneId> {
        match self {
            Self::Leaf(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut ids = first.pane_ids();
                ids.extend(second.pane_ids());
                ids
            }
        }
    }

    /// Find the split containing the given pane ID and adjust its ratio.
    fn adjust_ratio(&mut self, pane: PaneId, delta: f32) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Split { ratio, first, second, .. } => {
                let first_ids = first.pane_ids();
                let second_ids = second.pane_ids();
                if first_ids.contains(&pane) || second_ids.contains(&pane) {
                    *ratio = (*ratio + delta).clamp(0.15, 0.85);
                    return true;
                }
                first.adjust_ratio(pane, delta) || second.adjust_ratio(pane, delta)
            }
        }
    }

    /// Remove a pane from the layout tree. Returns the replacement node if the
    /// pane was found, or None if not found.
    fn remove_pane(&mut self, target: PaneId) -> bool {
        match self {
            Self::Leaf(id) => *id == target,
            Self::Split { first, second, .. } => {
                let first_ids = first.pane_ids();
                let second_ids = second.pane_ids();

                if first_ids.len() == 1 && first_ids[0] == target {
                    // Replace self with second child
                    *self = *second.clone();
                    return true;
                }
                if second_ids.len() == 1 && second_ids[0] == target {
                    // Replace self with first child
                    *self = *first.clone();
                    return true;
                }
                first.remove_pane(target) || second.remove_pane(target)
            }
        }
    }

    /// Split a pane, replacing it with a split node containing the original
    /// pane and a new pane.
    fn split_pane(&mut self, target: PaneId, new_id: PaneId, direction: SplitDir) -> bool {
        match self {
            Self::Leaf(id) if *id == target => {
                *self = Self::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(Self::Leaf(target)),
                    second: Box::new(Self::Leaf(new_id)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_pane(target, new_id, direction)
                    || second.split_pane(target, new_id, direction)
            }
        }
    }
}

// ============================================================================
// Layout presets
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutPreset {
    /// All panes arranged horizontally (stacked top-to-bottom).
    EvenHorizontal,
    /// All panes arranged vertically (side by side).
    EvenVertical,
    /// One main pane on top, rest below in a row.
    MainHorizontal,
    /// One main pane on left, rest on right in a column.
    MainVertical,
    /// Tiled (alternating splits).
    Tiled,
}

impl LayoutPreset {
    fn label(self) -> &'static str {
        match self {
            Self::EvenHorizontal => "Even Horizontal",
            Self::EvenVertical => "Even Vertical",
            Self::MainHorizontal => "Main Horizontal",
            Self::MainVertical => "Main Vertical",
            Self::Tiled => "Tiled",
        }
    }

    /// Build a layout tree from a preset and a list of pane IDs.
    fn build(self, panes: &[PaneId]) -> LayoutNode {
        if panes.is_empty() {
            return LayoutNode::Leaf(PaneId(0));
        }
        if panes.len() == 1 {
            return LayoutNode::Leaf(panes[0]);
        }

        match self {
            Self::EvenHorizontal => Self::build_even(panes, SplitDir::Horizontal),
            Self::EvenVertical => Self::build_even(panes, SplitDir::Vertical),
            Self::MainHorizontal => {
                let main_pane = panes[0];
                let rest = &panes[1..];
                if rest.is_empty() {
                    LayoutNode::Leaf(main_pane)
                } else {
                    LayoutNode::Split {
                        direction: SplitDir::Horizontal,
                        ratio: 0.6,
                        first: Box::new(LayoutNode::Leaf(main_pane)),
                        second: Box::new(Self::build_even(rest, SplitDir::Vertical)),
                    }
                }
            }
            Self::MainVertical => {
                let main_pane = panes[0];
                let rest = &panes[1..];
                if rest.is_empty() {
                    LayoutNode::Leaf(main_pane)
                } else {
                    LayoutNode::Split {
                        direction: SplitDir::Vertical,
                        ratio: 0.6,
                        first: Box::new(LayoutNode::Leaf(main_pane)),
                        second: Box::new(Self::build_even(rest, SplitDir::Horizontal)),
                    }
                }
            }
            Self::Tiled => Self::build_tiled(panes),
        }
    }

    fn build_even(panes: &[PaneId], direction: SplitDir) -> LayoutNode {
        if panes.len() == 1 {
            return LayoutNode::Leaf(panes[0]);
        }
        let mid = panes.len() / 2;
        let ratio = mid as f32 / panes.len() as f32;
        LayoutNode::Split {
            direction,
            ratio,
            first: Box::new(Self::build_even(&panes[..mid], direction)),
            second: Box::new(Self::build_even(&panes[mid..], direction)),
        }
    }

    fn build_tiled(panes: &[PaneId]) -> LayoutNode {
        if panes.len() == 1 {
            return LayoutNode::Leaf(panes[0]);
        }
        if panes.len() == 2 {
            return LayoutNode::Split {
                direction: SplitDir::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutNode::Leaf(panes[0])),
                second: Box::new(LayoutNode::Leaf(panes[1])),
            };
        }
        let mid = panes.len() / 2;
        LayoutNode::Split {
            direction: SplitDir::Horizontal,
            ratio: 0.5,
            first: Box::new(Self::build_even(&panes[..mid], SplitDir::Vertical)),
            second: Box::new(Self::build_even(&panes[mid..], SplitDir::Vertical)),
        }
    }
}

// ============================================================================
// Window (tab)
// ============================================================================

/// A unique window identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct WindowId(usize);

/// A tmux "window" — a tab containing one or more panes arranged by a layout.
#[derive(Debug, Clone)]
struct Window {
    id: WindowId,
    /// Window name (user-settable or auto-derived from active pane).
    name: String,
    /// Layout tree describing pane arrangement.
    layout: LayoutNode,
    /// The currently focused pane.
    active_pane: PaneId,
    /// Layout preset applied to this window.
    preset: LayoutPreset,
    /// Window creation number (for display index).
    index: usize,
}

impl Window {
    fn new(id: WindowId, index: usize, initial_pane: PaneId) -> Self {
        Self {
            id,
            name: format!("{index}:shell"),
            layout: LayoutNode::Leaf(initial_pane),
            active_pane: initial_pane,
            preset: LayoutPreset::Tiled,
            index,
        }
    }
}

// ============================================================================
// Session
// ============================================================================

/// A unique session identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SessionId(usize);

/// A tmux session — a collection of windows that can be detached/reattached.
#[derive(Debug, Clone)]
struct Session {
    id: SessionId,
    /// Session name.
    name: String,
    /// Windows in this session.
    windows: Vec<Window>,
    /// Active window index.
    active_window: usize,
    /// Whether a client is attached.
    attached: bool,
    /// When the session was created (monotonic ms).
    created_at_ms: u64,
    /// Next pane ID counter.
    next_pane_id: usize,
    /// Next window ID counter.
    next_window_id: usize,
    /// Window creation counter.
    next_window_index: usize,
}

impl Session {
    fn new(id: SessionId, name: &str, now_ms: u64) -> Self {
        let pane = PaneId(0);
        let window = Window::new(WindowId(0), 0, pane);
        Self {
            id,
            name: name.to_string(),
            windows: vec![window],
            active_window: 0,
            attached: true,
            created_at_ms: now_ms,
            next_pane_id: 1,
            next_window_id: 1,
            next_window_index: 1,
        }
    }

    fn alloc_pane_id(&mut self) -> PaneId {
        let id = PaneId(self.next_pane_id);
        self.next_pane_id = self.next_pane_id.saturating_add(1);
        id
    }

    fn alloc_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id = self.next_window_id.saturating_add(1);
        id
    }

    fn alloc_window_index(&mut self) -> usize {
        let idx = self.next_window_index;
        self.next_window_index = self.next_window_index.saturating_add(1);
        idx
    }

    fn active_window(&self) -> Option<&Window> {
        self.windows.get(self.active_window)
    }

    fn active_window_mut(&mut self) -> Option<&mut Window> {
        self.windows.get_mut(self.active_window)
    }
}

// ============================================================================
// Key binding prefix mode
// ============================================================================

/// The prefix key mode state. tmux uses Ctrl+B as prefix key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefixState {
    /// Normal input — keys go to the active pane.
    Normal,
    /// Prefix key was pressed — next key is a tmux command.
    Prefix,
}

// ============================================================================
// Multiplexer state
// ============================================================================

/// Top-level terminal multiplexer state.
struct Multiplexer {
    /// All panes across all sessions (shared storage).
    panes: Vec<Pane>,
    /// All sessions.
    sessions: Vec<Session>,
    /// Currently active session index.
    active_session: usize,
    /// Prefix key state.
    prefix_state: PrefixState,
    /// Whether the command prompt is shown (`:` command mode).
    command_mode: bool,
    /// Command input buffer.
    command_input: String,
    /// Status message (shown in status bar, cleared on next action).
    status_message: String,
    /// Status message timestamp.
    status_time: u64,
    /// Current time (monotonic ms).
    current_time: u64,
    /// Next session ID counter.
    next_session_id: usize,
    /// Clipboard content (from copy mode).
    clipboard: String,
    /// Whether the session chooser is open.
    session_chooser: bool,
    /// Window chooser open flag.
    window_chooser: bool,
}

impl Multiplexer {
    fn new() -> Self {
        let initial_pane = Pane::new(PaneId(0), 80, 24);
        let session = Session::new(SessionId(0), "main", 0);
        Self {
            panes: vec![initial_pane],
            sessions: vec![session],
            active_session: 0,
            prefix_state: PrefixState::Normal,
            command_mode: false,
            command_input: String::new(),
            status_message: String::new(),
            status_time: 0,
            current_time: 0,
            next_session_id: 1,
            clipboard: String::new(),
            session_chooser: false,
            window_chooser: false,
        }
    }

    fn set_time(&mut self, ms: u64) {
        self.current_time = ms;
    }

    fn active_session(&self) -> Option<&Session> {
        self.sessions.get(self.active_session)
    }

    fn active_session_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.active_session)
    }

    fn find_pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    fn find_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    fn set_status(&mut self, msg: &str) {
        self.status_message = msg.to_string();
        self.status_time = self.current_time;
    }

    // ========================================================================
    // Session management
    // ========================================================================

    /// Create a new session and switch to it.
    fn new_session(&mut self, name: &str) {
        if self.sessions.len() >= MAX_SESSIONS {
            self.set_status("Maximum sessions reached");
            return;
        }

        let sid = SessionId(self.next_session_id);
        self.next_session_id = self.next_session_id.saturating_add(1);

        let pane_id = PaneId(self.panes.len());
        let pane = Pane::new(pane_id, 80, 24);
        self.panes.push(pane);

        let mut session = Session::new(sid, name, self.current_time);
        // Fix the initial window's pane to match the actual pane we created
        if let Some(w) = session.windows.first_mut() {
            w.active_pane = pane_id;
            w.layout = LayoutNode::Leaf(pane_id);
        }
        session.next_pane_id = pane_id.0.saturating_add(1);

        self.sessions.push(session);
        self.active_session = self.sessions.len().saturating_sub(1);
        self.set_status(&format!("Created session: {name}"));
    }

    /// Detach from the current session.
    fn detach(&mut self) {
        if let Some(session) = self.active_session_mut() {
            session.attached = false;
            let name = session.name.clone();
            self.set_status(&format!("Detached from session: {name}"));
        }
    }

    /// Attach to a specific session by index.
    fn attach(&mut self, index: usize) {
        if let Some(session) = self.sessions.get_mut(index) {
            session.attached = true;
            self.active_session = index;
            let name = session.name.clone();
            self.set_status(&format!("Attached to session: {name}"));
        }
    }

    /// Kill a session by index.
    fn kill_session(&mut self, index: usize) {
        if index < self.sessions.len() && self.sessions.len() > 1 {
            let name = self.sessions[index].name.clone();
            self.sessions.remove(index);
            if self.active_session >= self.sessions.len() {
                self.active_session = self.sessions.len().saturating_sub(1);
            }
            self.set_status(&format!("Killed session: {name}"));
        }
    }

    // ========================================================================
    // Window management
    // ========================================================================

    /// Create a new window in the current session.
    fn new_window(&mut self) {
        let pane_id = PaneId(self.panes.len());
        let pane = Pane::new(pane_id, 80, 24);
        self.panes.push(pane);

        if let Some(session) = self.active_session_mut() {
            if session.windows.len() >= MAX_WINDOWS {
                return;
            }
            let wid = session.alloc_window_id();
            let idx = session.alloc_window_index();
            let window = Window::new(wid, idx, pane_id);
            session.windows.push(window);
            session.active_window = session.windows.len().saturating_sub(1);
        }
    }

    /// Close the current window. If it's the last window, detach.
    fn close_window(&mut self) {
        if let Some(session) = self.active_session_mut() {
            if session.windows.len() <= 1 {
                // Last window — detach session
                session.attached = false;
                return;
            }
            session.windows.remove(session.active_window);
            if session.active_window >= session.windows.len() {
                session.active_window = session.windows.len().saturating_sub(1);
            }
        }
    }

    /// Switch to next window.
    fn next_window(&mut self) {
        if let Some(session) = self.active_session_mut()
            && !session.windows.is_empty()
        {
            session.active_window = (session.active_window.saturating_add(1)) % session.windows.len();
        }
    }

    /// Switch to previous window.
    fn prev_window(&mut self) {
        if let Some(session) = self.active_session_mut()
            && !session.windows.is_empty()
        {
            if session.active_window == 0 {
                session.active_window = session.windows.len().saturating_sub(1);
            } else {
                session.active_window = session.active_window.saturating_sub(1);
            }
        }
    }

    /// Rename current window.
    fn rename_window(&mut self, name: &str) {
        if let Some(session) = self.active_session_mut()
            && let Some(window) = session.active_window_mut()
        {
            window.name = name.to_string();
        }
    }

    // ========================================================================
    // Pane management
    // ========================================================================

    /// Split the active pane in the given direction.
    fn split_pane(&mut self, direction: SplitDir) {
        let pane_id = PaneId(self.panes.len());
        let pane = Pane::new(pane_id, 80, 24);
        self.panes.push(pane);

        if let Some(session) = self.active_session_mut()
            && let Some(window) = session.active_window_mut()
        {
            if window.layout.pane_count() >= MAX_PANES {
                return;
            }
            let target = window.active_pane;
            window.layout.split_pane(target, pane_id, direction);
            window.active_pane = pane_id;
        }
    }

    /// Close the active pane. If it's the last pane, close the window.
    fn close_pane(&mut self) {
        if let Some(session) = self.active_session_mut()
            && let Some(window) = session.active_window_mut()
        {
            if window.layout.pane_count() <= 1 {
                // Only one pane — close the window instead
                if session.windows.len() <= 1 {
                    session.attached = false;
                    return;
                }
                session.windows.remove(session.active_window);
                if session.active_window >= session.windows.len() {
                    session.active_window = session.windows.len().saturating_sub(1);
                }
                return;
            }
            let target = window.active_pane;
            window.layout.remove_pane(target);
            // Select next pane
            let remaining = window.layout.pane_ids();
            if let Some(next) = remaining.first() {
                window.active_pane = *next;
            }
            // Mark pane as dead
            if let Some(pane) = self.find_pane_mut(target) {
                pane.alive = false;
            }
        }
    }

    /// Navigate to the next pane in the active window.
    fn next_pane(&mut self) {
        if let Some(session) = self.active_session_mut()
            && let Some(window) = session.active_window_mut()
        {
            let ids = window.layout.pane_ids();
            if let Some(pos) = ids.iter().position(|id| *id == window.active_pane) {
                let next = (pos.saturating_add(1)) % ids.len();
                if let Some(id) = ids.get(next) {
                    window.active_pane = *id;
                }
            }
        }
    }

    /// Navigate to the previous pane.
    fn prev_pane(&mut self) {
        if let Some(session) = self.active_session_mut()
            && let Some(window) = session.active_window_mut()
        {
            let ids = window.layout.pane_ids();
            if let Some(pos) = ids.iter().position(|id| *id == window.active_pane) {
                let prev = if pos == 0 { ids.len().saturating_sub(1) } else { pos.saturating_sub(1) };
                if let Some(id) = ids.get(prev) {
                    window.active_pane = *id;
                }
            }
        }
    }

    /// Resize the active pane in a direction.
    fn resize_pane(&mut self, grow: bool) {
        if let Some(session) = self.active_session_mut()
            && let Some(window) = session.active_window_mut()
        {
            let delta = if grow { 0.05 } else { -0.05 };
            window.layout.adjust_ratio(window.active_pane, delta);
        }
    }

    /// Apply a layout preset to the current window.
    fn apply_layout(&mut self, preset: LayoutPreset) {
        if let Some(session) = self.active_session_mut()
            && let Some(window) = session.active_window_mut()
        {
            let pane_ids = window.layout.pane_ids();
            window.layout = preset.build(&pane_ids);
            window.preset = preset;
        }
    }

    /// Swap the active pane with the next pane.
    fn swap_pane_next(&mut self) {
        // Swap by swapping pane IDs in the layout is complex.
        // For simplicity, just cycle the active pane forward.
        self.next_pane();
    }

    // ========================================================================
    // Command processing
    // ========================================================================

    /// Process a prefix command key.
    fn process_prefix_key(&mut self, key: char) {
        match key {
            // Split horizontal (top/bottom)
            '-' | '"' => self.split_pane(SplitDir::Horizontal),
            // Split vertical (left/right)
            '|' | '%' => self.split_pane(SplitDir::Vertical),
            // Navigate panes
            'o' => self.next_pane(),
            // Next/prev window
            'n' => self.next_window(),
            'p' => self.prev_window(),
            // New window
            'c' => self.new_window(),
            // Close pane
            'x' => self.close_pane(),
            // Detach
            'd' => self.detach(),
            // Rename window
            ',' => {
                self.command_mode = true;
                self.command_input = ":rename-window ".to_string();
            }
            // Command mode
            ':' => {
                self.command_mode = true;
                self.command_input = ":".to_string();
            }
            // Session chooser
            's' => {
                self.session_chooser = !self.session_chooser;
                self.window_chooser = false;
            }
            // Window chooser
            'w' => {
                self.window_chooser = !self.window_chooser;
                self.session_chooser = false;
            }
            // Copy mode
            '[' => {
                if let Some(session) = self.active_session()
                    && let Some(window) = session.active_window()
                {
                    let pane_id = window.active_pane;
                    if let Some(pane) = self.find_pane_mut(pane_id) {
                        pane.enter_copy_mode();
                    }
                }
            }
            // Layout cycling
            ' ' => {
                if let Some(session) = self.active_session()
                    && let Some(window) = session.active_window()
                {
                    let next = match window.preset {
                        LayoutPreset::EvenHorizontal => LayoutPreset::EvenVertical,
                        LayoutPreset::EvenVertical => LayoutPreset::MainHorizontal,
                        LayoutPreset::MainHorizontal => LayoutPreset::MainVertical,
                        LayoutPreset::MainVertical => LayoutPreset::Tiled,
                        LayoutPreset::Tiled => LayoutPreset::EvenHorizontal,
                    };
                    self.apply_layout(next);
                }
            }
            // Resize (with arrow keys this would be done differently;
            // for prefix mode, use +/- for grow/shrink)
            '+' => self.resize_pane(true),
            // Note: '-' is used for horizontal split, so resize-shrink
            // is available via the ':' command mode instead.
            // Window selection by number
            '0'..='9' => {
                let idx = (key as u8 - b'0') as usize;
                if let Some(session) = self.active_session_mut()
                    && idx < session.windows.len()
                {
                    session.active_window = idx;
                }
            }
            // Zoom (toggle full-size for active pane)
            'z' => {
                self.set_status("Pane zoom toggled");
            }
            _ => {
                self.set_status(&format!("Unknown key: {key}"));
            }
        }
        self.prefix_state = PrefixState::Normal;
    }

    /// Process a command string (from : prompt).
    fn process_command(&mut self, cmd: &str) {
        let cmd = cmd.trim_start_matches(':').trim();
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        let command = parts.first().copied().unwrap_or("");
        let arg = parts.get(1).copied().unwrap_or("");

        match command {
            "new-session" | "new" => {
                let name = if arg.is_empty() {
                    format!("session-{}", self.sessions.len())
                } else {
                    arg.to_string()
                };
                self.new_session(&name);
            }
            "kill-session" | "kill" => {
                if let Ok(idx) = arg.parse::<usize>() {
                    self.kill_session(idx);
                }
            }
            "rename-window" | "renamew" => {
                self.rename_window(arg);
            }
            "split-window" | "splitw" => {
                if arg.contains("-h") || arg.contains("horizontal") {
                    self.split_pane(SplitDir::Horizontal);
                } else {
                    self.split_pane(SplitDir::Vertical);
                }
            }
            "new-window" | "neww" => {
                self.new_window();
            }
            "select-layout" | "layout" => {
                match arg {
                    "even-horizontal" => self.apply_layout(LayoutPreset::EvenHorizontal),
                    "even-vertical" => self.apply_layout(LayoutPreset::EvenVertical),
                    "main-horizontal" => self.apply_layout(LayoutPreset::MainHorizontal),
                    "main-vertical" => self.apply_layout(LayoutPreset::MainVertical),
                    "tiled" => self.apply_layout(LayoutPreset::Tiled),
                    _ => self.set_status(&format!("Unknown layout: {arg}")),
                }
            }
            "attach" | "attach-session" => {
                if let Ok(idx) = arg.parse::<usize>() {
                    self.attach(idx);
                }
            }
            "detach" | "detach-client" => {
                self.detach();
            }
            "list-sessions" | "ls" => {
                let msg = self.sessions.iter().enumerate()
                    .map(|(i, s)| format!("{i}: {} ({} windows{})",
                        s.name,
                        s.windows.len(),
                        if s.attached { " (attached)" } else { "" }
                    ))
                    .collect::<Vec<_>>()
                    .join(" | ");
                self.set_status(&msg);
            }
            _ => {
                self.set_status(&format!("Unknown command: {command}"));
            }
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire multiplexer UI.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(256);

        let Some(session) = self.active_session() else {
            return cmds;
        };

        if !session.attached {
            // Show detached screen
            self.render_detached(&mut cmds);
            return cmds;
        }

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Tab bar
        self.render_tab_bar(&mut cmds, session);

        // Pane area
        if let Some(window) = session.active_window() {
            let pane_area_y = TAB_BAR_HEIGHT;
            let pane_area_h = WINDOW_HEIGHT - TAB_BAR_HEIGHT - STATUS_BAR_HEIGHT;
            let bounds = window.layout.compute_bounds(0.0, pane_area_y, WINDOW_WIDTH, pane_area_h);

            for (pane_id, x, y, w, h) in &bounds {
                let is_active = *pane_id == window.active_pane;
                self.render_pane(&mut cmds, *pane_id, *x, *y, *w, *h, is_active);
            }
        }

        // Status bar
        self.render_status_bar(&mut cmds, session);

        // Overlays
        if self.session_chooser {
            self.render_session_chooser(&mut cmds);
        }
        if self.window_chooser {
            self.render_window_chooser(&mut cmds);
        }
        if self.command_mode {
            self.render_command_input(&mut cmds);
        }
        if self.prefix_state == PrefixState::Prefix {
            self.render_prefix_indicator(&mut cmds);
        }

        cmds
    }

    #[allow(clippy::unused_self)]  // kept as method for symmetry with other render_* dispatch
    fn render_tab_bar(&self, cmds: &mut Vec<RenderCommand>, session: &Session) {
        // Tab bar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: TAB_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tab_x = PADDING;
        for (i, window) in session.windows.iter().enumerate() {
            let is_active = i == session.active_window;
            let label = &window.name;
            let tab_w = label.len() as f32 * 7.5 + 24.0;

            // Tab background
            let tab_bg = if is_active { SURFACE0 } else { MANTLE };
            cmds.push(RenderCommand::FillRect {
                x: tab_x,
                y: 2.0,
                width: tab_w,
                height: TAB_BAR_HEIGHT - 2.0,
                color: tab_bg,
                corner_radii: CornerRadii {
                    top_left: 4.0,
                    top_right: 4.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            // Active indicator bar
            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: 2.0,
                    width: tab_w,
                    height: 2.0,
                    color: BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Tab label
            let tab_color = if is_active { TEXT } else { SUBTEXT0 };
            cmds.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: 8.0,
                text: label.clone(),
                font_size: SMALL_TEXT,
                color: tab_color,
                font_weight: if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tab_w - 16.0),
            });

            tab_x += tab_w + 2.0;
        }
    }

    #[allow(clippy::too_many_arguments)]  // render_pane needs all of: target, pane id, geometry, focus state
    fn render_pane(&self, cmds: &mut Vec<RenderCommand>, pane_id: PaneId,
                    x: f32, y: f32, width: f32, height: f32, active: bool) {
        // Pane background
        cmds.push(RenderCommand::FillRect {
            x, y, width, height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Pane border
        let border_color = if active { BLUE } else { SURFACE1 };
        cmds.push(RenderCommand::StrokeRect {
            x, y, width, height,
            color: border_color,
            line_width: PANE_BORDER_WIDTH,
            corner_radii: CornerRadii::ZERO,
        });

        // Render terminal content
        if let Some(pane) = self.find_pane(pane_id) {
            let content_x = x + 2.0;
            let content_y = y + 2.0;
            let visible_rows = ((height - 4.0) / CHAR_HEIGHT) as usize;
            let visible_cols = ((width - 4.0) / CHAR_WIDTH) as usize;

            for row_idx in 0..visible_rows.min(pane.buffer.rows) {
                if let Some(row) = pane.buffer.cells.get(row_idx) {
                    for col_idx in 0..visible_cols.min(row.len()) {
                        if let Some(cell) = row.get(col_idx)
                            && (cell.ch != ' ' || cell.bg != BASE)
                        {
                            let (fg, bg) = TerminalBuffer::effective_colors(cell);
                            let cx = content_x + col_idx as f32 * CHAR_WIDTH;
                            let cy = content_y + row_idx as f32 * CHAR_HEIGHT;

                            // Cell background (only if non-default)
                            if bg != BASE {
                                cmds.push(RenderCommand::FillRect {
                                    x: cx, y: cy,
                                    width: CHAR_WIDTH,
                                    height: CHAR_HEIGHT,
                                    color: bg,
                                    corner_radii: CornerRadii::ZERO,
                                });
                            }

                            // Character
                            if cell.ch != ' ' {
                                cmds.push(RenderCommand::Text {
                                    x: cx,
                                    y: cy,
                                    text: cell.ch.to_string(),
                                    font_size: CHAR_HEIGHT - 2.0,
                                    color: fg,
                                    font_weight: if cell.bold { FontWeightHint::Bold } else { FontWeightHint::Regular },
                                    max_width: Some(CHAR_WIDTH),
                                });
                            }
                        }
                    }
                }
            }

            // Cursor
            if pane.buffer.cursor_visible && active && !pane.copy_mode {
                let cx = content_x + pane.buffer.cursor_col as f32 * CHAR_WIDTH;
                let cy = content_y + pane.buffer.cursor_row as f32 * CHAR_HEIGHT;
                cmds.push(RenderCommand::FillRect {
                    x: cx, y: cy,
                    width: CHAR_WIDTH,
                    height: CHAR_HEIGHT,
                    color: Color::rgba(205, 214, 244, 128),
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Copy mode indicator
            if pane.copy_mode {
                cmds.push(RenderCommand::FillRect {
                    x: x + width - 90.0,
                    y,
                    width: 90.0,
                    height: 18.0,
                    color: YELLOW,
                    corner_radii: CornerRadii::ZERO,
                });
                cmds.push(RenderCommand::Text {
                    x: x + width - 86.0,
                    y: y + 2.0,
                    text: "[COPY MODE]".into(),
                    font_size: SMALL_TEXT,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(85.0),
                });
            }

            // Bell indicator
            if pane.buffer.bell {
                cmds.push(RenderCommand::FillRect {
                    x, y,
                    width: 16.0,
                    height: 16.0,
                    color: RED,
                    corner_radii: CornerRadii::all(8.0),
                });
            }
        }
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>, session: &Session) {
        let y = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;

        // Status bar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: STATUS_BAR_HEIGHT,
            color: GREEN,
            corner_radii: CornerRadii::ZERO,
        });

        // Session name (left)
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + 4.0,
            text: format!("[{}]", session.name),
            font_size: SMALL_TEXT,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Window list (center)
        let mut wx = 120.0;
        for (i, window) in session.windows.iter().enumerate() {
            let is_active = i == session.active_window;
            let label = format!("{}:{}", window.index, window.name);
            let color = if is_active { CRUST } else { MANTLE };
            let weight = if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular };

            if is_active {
                cmds.push(RenderCommand::Text {
                    x: wx,
                    y: y + 4.0,
                    text: format!("*{label}"),
                    font_size: SMALL_TEXT,
                    color,
                    font_weight: weight,
                    max_width: Some(150.0),
                });
            } else {
                cmds.push(RenderCommand::Text {
                    x: wx,
                    y: y + 4.0,
                    text: label,
                    font_size: SMALL_TEXT,
                    color,
                    font_weight: weight,
                    max_width: Some(150.0),
                });
            }
            wx += 100.0;
        }

        // Status message or clock (right)
        let msg = if !self.status_message.is_empty()
            && self.current_time.saturating_sub(self.status_time) < 5000
        {
            self.status_message.clone()
        } else {
            // Mock clock
            let secs = (self.current_time / 1000) % 86400;
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            format!("{h:02}:{m:02}")
        };

        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 200.0,
            y: y + 4.0,
            text: msg,
            font_size: SMALL_TEXT,
            color: CRUST,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });
    }

    #[allow(clippy::unused_self)]  // kept as method for symmetry with other render_* dispatch
    fn render_detached(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH / 2.0 - 100.0,
            y: WINDOW_HEIGHT / 2.0 - 20.0,
            text: "[detached]".into(),
            font_size: HEADER_TEXT,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH / 2.0 - 150.0,
            y: WINDOW_HEIGHT / 2.0 + 10.0,
            text: "Use :attach or tmux attach to reconnect".into(),
            font_size: NORMAL_TEXT,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });
    }

    fn render_session_chooser(&self, cmds: &mut Vec<RenderCommand>) {
        let w = 300.0;
        let h = (self.sessions.len() as f32 * 24.0 + 40.0).min(400.0);
        let x = (WINDOW_WIDTH - w) / 2.0;
        let y = (WINDOW_HEIGHT - h) / 2.0;

        // Overlay background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x, y, width: w, height: h,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 8.0,
            text: "Sessions".into(),
            font_size: HEADER_TEXT,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 24.0),
        });

        for (i, session) in self.sessions.iter().enumerate() {
            let row_y = y + 36.0 + i as f32 * 24.0;
            let is_active = i == self.active_session;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: row_y,
                    width: w - 8.0,
                    height: 22.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let label = format!(
                "{}: {} ({} windows{})",
                i,
                session.name,
                session.windows.len(),
                if session.attached { ", attached" } else { "" }
            );
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 3.0,
                text: label,
                font_size: SMALL_TEXT,
                color: if is_active { TEXT } else { SUBTEXT0 },
                font_weight: if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(w - 24.0),
            });
        }
    }

    fn render_window_chooser(&self, cmds: &mut Vec<RenderCommand>) {
        let Some(session) = self.active_session() else { return };

        let w = 300.0;
        let h = (session.windows.len() as f32 * 24.0 + 40.0).min(400.0);
        let x = (WINDOW_WIDTH - w) / 2.0;
        let y = (WINDOW_HEIGHT - h) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::FillRect {
            x, y, width: w, height: h,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x, y, width: w, height: h,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 12.0,
            y: y + 8.0,
            text: "Windows".into(),
            font_size: HEADER_TEXT,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 24.0),
        });

        for (i, window) in session.windows.iter().enumerate() {
            let row_y = y + 36.0 + i as f32 * 24.0;
            let is_active = i == session.active_window;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0,
                    y: row_y,
                    width: w - 8.0,
                    height: 22.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let pane_count = window.layout.pane_count();
            let label = format!("{}: {} ({} panes)", window.index, window.name, pane_count);
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row_y + 3.0,
                text: label,
                font_size: SMALL_TEXT,
                color: if is_active { TEXT } else { SUBTEXT0 },
                font_weight: if is_active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(w - 24.0),
            });
        }
    }

    fn render_command_input(&self, cmds: &mut Vec<RenderCommand>) {
        let y = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;
        // Overwrite status bar with command input
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: WINDOW_WIDTH,
            height: STATUS_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: y + 4.0,
            text: self.command_input.clone(),
            font_size: SMALL_TEXT,
            color: YELLOW,
            font_weight: FontWeightHint::Regular,
            max_width: Some(WINDOW_WIDTH - PADDING * 2.0),
        });
    }

    #[allow(clippy::unused_self)]  // kept as method for symmetry with other render_* dispatch
    fn render_prefix_indicator(&self, cmds: &mut Vec<RenderCommand>) {
        // Show a small indicator that prefix key was pressed
        let x = WINDOW_WIDTH - 100.0;
        let y = TAB_BAR_HEIGHT;
        cmds.push(RenderCommand::FillRect {
            x, y,
            width: 100.0,
            height: 20.0,
            color: YELLOW,
            corner_radii: CornerRadii::all(0.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 3.0,
            text: "Ctrl+B ...".into(),
            font_size: SMALL_TEXT,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: Some(90.0),
        });
    }
}

// ============================================================================
// Main (placeholder)
// ============================================================================

fn main() {
    // In the real OS, this would start the terminal multiplexer,
    // connect to the compositor, and begin the event loop.
    let _mux = Multiplexer::new();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Terminal Buffer ---

    #[test]
    fn test_new_buffer() {
        let buf = TerminalBuffer::new(80, 24);
        assert_eq!(buf.cols, 80);
        assert_eq!(buf.rows, 24);
        assert_eq!(buf.cursor_col, 0);
        assert_eq!(buf.cursor_row, 0);
    }

    #[test]
    fn test_write_char() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.write_char('A');
        assert_eq!(buf.cells[0][0].ch, 'A');
        assert_eq!(buf.cursor_col, 1);
    }

    #[test]
    fn test_write_string() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.write_str("Hello");
        assert_eq!(buf.cells[0][0].ch, 'H');
        assert_eq!(buf.cells[0][4].ch, 'o');
        assert_eq!(buf.cursor_col, 5);
    }

    #[test]
    fn test_newline() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.write_str("Line1\nLine2");
        assert_eq!(buf.cells[0][0].ch, 'L');
        assert_eq!(buf.cells[1][0].ch, 'L');
        assert_eq!(buf.cursor_row, 1);
    }

    #[test]
    fn test_scroll_up() {
        let mut buf = TerminalBuffer::new(80, 3);
        buf.write_str("A\nB\nC\nD");
        assert!(!buf.scrollback.is_empty());
        assert_eq!(buf.cells[2][0].ch, 'D');
    }

    #[test]
    fn test_clear() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.write_str("Hello");
        buf.clear();
        assert_eq!(buf.cells[0][0].ch, ' ');
        assert_eq!(buf.cursor_col, 0);
        assert_eq!(buf.cursor_row, 0);
    }

    #[test]
    fn test_resize() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.write_str("Hello");
        buf.resize(40, 12);
        assert_eq!(buf.cols, 40);
        assert_eq!(buf.rows, 12);
        assert_eq!(buf.cells[0][0].ch, 'H');
    }

    #[test]
    fn test_set_cursor() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.set_cursor(5, 10);
        assert_eq!(buf.cursor_row, 4);  // 1-indexed → 0-indexed
        assert_eq!(buf.cursor_col, 9);
    }

    #[test]
    fn test_clear_to_end() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.write_str("Hello World");
        buf.cursor_col = 5;
        buf.clear_to_end();
        assert_eq!(buf.cells[0][4].ch, 'o');
        assert_eq!(buf.cells[0][5].ch, ' ');
    }

    #[test]
    fn test_tab_character() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.write_char('\t');
        assert_eq!(buf.cursor_col, 8);
    }

    #[test]
    fn test_backspace() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.write_str("AB");
        buf.write_char('\x08');
        assert_eq!(buf.cursor_col, 1);
    }

    #[test]
    fn test_bell() {
        let mut buf = TerminalBuffer::new(80, 24);
        assert!(!buf.bell);
        buf.write_char('\x07');
        assert!(buf.bell);
    }

    #[test]
    fn test_carriage_return() {
        let mut buf = TerminalBuffer::new(80, 24);
        buf.write_str("Hello");
        buf.write_char('\r');
        assert_eq!(buf.cursor_col, 0);
    }

    #[test]
    fn test_line_wrap() {
        let mut buf = TerminalBuffer::new(5, 3);
        buf.write_str("ABCDEFGH");
        // Should have wrapped to second line
        assert_eq!(buf.cursor_row, 1);
    }

    // --- ANSI Parser ---

    #[test]
    fn test_ansi_cursor_up() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        buf.cursor_row = 5;
        parser.feed("\x1B[2A", &mut buf);
        assert_eq!(buf.cursor_row, 3);
    }

    #[test]
    fn test_ansi_cursor_down() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("\x1B[3B", &mut buf);
        assert_eq!(buf.cursor_row, 3);
    }

    #[test]
    fn test_ansi_cursor_position() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("\x1B[10;20H", &mut buf);
        assert_eq!(buf.cursor_row, 9);
        assert_eq!(buf.cursor_col, 19);
    }

    #[test]
    fn test_ansi_erase_display() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("Hello", &mut buf);
        parser.feed("\x1B[2J", &mut buf);
        assert_eq!(buf.cells[0][0].ch, ' ');
    }

    #[test]
    fn test_ansi_sgr_bold() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("\x1B[1mBold\x1B[0m", &mut buf);
        assert!(buf.cells[0][0].bold);
        // After reset, next char should not be bold
        parser.feed("X", &mut buf);
        assert!(!buf.cells[0][4].bold);
    }

    #[test]
    fn test_ansi_sgr_color() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("\x1B[31mRed", &mut buf);
        assert_eq!(buf.cells[0][0].fg, RED);
    }

    #[test]
    fn test_ansi_cursor_visibility() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("\x1B[?25l", &mut buf);
        assert!(!buf.cursor_visible);
        parser.feed("\x1B[?25h", &mut buf);
        assert!(buf.cursor_visible);
    }

    #[test]
    fn test_ansi_osc_title() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("\x1B]0;My Title\x07", &mut buf);
        assert_eq!(buf.title, "My Title");
    }

    #[test]
    fn test_ansi_truecolor() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("\x1B[38;2;255;128;0mX", &mut buf);
        assert_eq!(buf.cells[0][0].fg, Color::rgb(255, 128, 0));
    }

    #[test]
    fn test_ansi_erase_line() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("Hello World", &mut buf);
        buf.cursor_col = 5;
        parser.feed("\x1B[K", &mut buf);
        assert_eq!(buf.cells[0][4].ch, 'o');
        assert_eq!(buf.cells[0][5].ch, ' ');
    }

    #[test]
    fn test_ansi_reset() {
        let mut buf = TerminalBuffer::new(80, 24);
        let mut parser = AnsiParser::new();
        parser.feed("Hello\x1Bc", &mut buf);
        assert_eq!(buf.cells[0][0].ch, ' ');
        assert_eq!(buf.cursor_col, 0);
    }

    // --- Layout ---

    #[test]
    fn test_layout_leaf_bounds() {
        let layout = LayoutNode::Leaf(PaneId(0));
        let bounds = layout.compute_bounds(0.0, 0.0, 800.0, 600.0);
        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0].0, PaneId(0));
        assert!((bounds[0].2 - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_layout_horizontal_split() {
        let layout = LayoutNode::Split {
            direction: SplitDir::Horizontal,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf(PaneId(0))),
            second: Box::new(LayoutNode::Leaf(PaneId(1))),
        };
        let bounds = layout.compute_bounds(0.0, 0.0, 800.0, 600.0);
        assert_eq!(bounds.len(), 2);
        // Second pane should be below the first
        assert!(bounds[1].2 > bounds[0].2);
    }

    #[test]
    fn test_layout_vertical_split() {
        let layout = LayoutNode::Split {
            direction: SplitDir::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf(PaneId(0))),
            second: Box::new(LayoutNode::Leaf(PaneId(1))),
        };
        let bounds = layout.compute_bounds(0.0, 0.0, 800.0, 600.0);
        assert_eq!(bounds.len(), 2);
        // Second pane should be to the right of the first
        assert!(bounds[1].1 > bounds[0].1);
    }

    #[test]
    fn test_layout_pane_count() {
        let layout = LayoutNode::Split {
            direction: SplitDir::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf(PaneId(0))),
            second: Box::new(LayoutNode::Split {
                direction: SplitDir::Horizontal,
                ratio: 0.5,
                first: Box::new(LayoutNode::Leaf(PaneId(1))),
                second: Box::new(LayoutNode::Leaf(PaneId(2))),
            }),
        };
        assert_eq!(layout.pane_count(), 3);
    }

    #[test]
    fn test_layout_pane_ids() {
        let layout = LayoutNode::Split {
            direction: SplitDir::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf(PaneId(0))),
            second: Box::new(LayoutNode::Leaf(PaneId(1))),
        };
        assert_eq!(layout.pane_ids(), vec![PaneId(0), PaneId(1)]);
    }

    #[test]
    fn test_layout_split_pane() {
        let mut layout = LayoutNode::Leaf(PaneId(0));
        assert!(layout.split_pane(PaneId(0), PaneId(1), SplitDir::Vertical));
        assert_eq!(layout.pane_count(), 2);
    }

    #[test]
    fn test_layout_remove_pane() {
        let mut layout = LayoutNode::Split {
            direction: SplitDir::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf(PaneId(0))),
            second: Box::new(LayoutNode::Leaf(PaneId(1))),
        };
        assert!(layout.remove_pane(PaneId(0)));
        assert_eq!(layout.pane_count(), 1);
        assert_eq!(layout.pane_ids(), vec![PaneId(1)]);
    }

    #[test]
    fn test_layout_adjust_ratio() {
        let mut layout = LayoutNode::Split {
            direction: SplitDir::Vertical,
            ratio: 0.5,
            first: Box::new(LayoutNode::Leaf(PaneId(0))),
            second: Box::new(LayoutNode::Leaf(PaneId(1))),
        };
        layout.adjust_ratio(PaneId(0), 0.1);
        if let LayoutNode::Split { ratio, .. } = &layout {
            assert!((*ratio - 0.6).abs() < f32::EPSILON);
        }
    }

    // --- Layout Presets ---

    #[test]
    fn test_preset_even_horizontal() {
        let panes = vec![PaneId(0), PaneId(1), PaneId(2)];
        let layout = LayoutPreset::EvenHorizontal.build(&panes);
        assert_eq!(layout.pane_count(), 3);
    }

    #[test]
    fn test_preset_even_vertical() {
        let panes = vec![PaneId(0), PaneId(1)];
        let layout = LayoutPreset::EvenVertical.build(&panes);
        assert_eq!(layout.pane_count(), 2);
    }

    #[test]
    fn test_preset_main_horizontal() {
        let panes = vec![PaneId(0), PaneId(1), PaneId(2)];
        let layout = LayoutPreset::MainHorizontal.build(&panes);
        assert_eq!(layout.pane_count(), 3);
    }

    #[test]
    fn test_preset_main_vertical() {
        let panes = vec![PaneId(0), PaneId(1), PaneId(2)];
        let layout = LayoutPreset::MainVertical.build(&panes);
        assert_eq!(layout.pane_count(), 3);
    }

    #[test]
    fn test_preset_tiled() {
        let panes = vec![PaneId(0), PaneId(1), PaneId(2), PaneId(3)];
        let layout = LayoutPreset::Tiled.build(&panes);
        assert_eq!(layout.pane_count(), 4);
    }

    #[test]
    fn test_preset_single_pane() {
        let panes = vec![PaneId(0)];
        let layout = LayoutPreset::Tiled.build(&panes);
        assert_eq!(layout.pane_count(), 1);
    }

    // --- Pane ---

    #[test]
    fn test_pane_feed() {
        let mut pane = Pane::new(PaneId(0), 80, 24);
        pane.feed("Hello World");
        assert_eq!(pane.buffer.cells[0][0].ch, 'H');
    }

    #[test]
    fn test_pane_copy_mode() {
        let mut pane = Pane::new(PaneId(0), 80, 24);
        assert!(!pane.copy_mode);
        pane.enter_copy_mode();
        assert!(pane.copy_mode);
        pane.exit_copy_mode();
        assert!(!pane.copy_mode);
    }

    #[test]
    fn test_pane_title_from_osc() {
        let mut pane = Pane::new(PaneId(0), 80, 24);
        pane.feed("\x1B]0;My Pane\x07");
        assert_eq!(pane.effective_title(), "My Pane");
    }

    // --- Session ---

    #[test]
    fn test_session_new() {
        let session = Session::new(SessionId(0), "test", 0);
        assert_eq!(session.name, "test");
        assert_eq!(session.windows.len(), 1);
        assert!(session.attached);
    }

    #[test]
    fn test_session_alloc_ids() {
        let mut session = Session::new(SessionId(0), "test", 0);
        let pid1 = session.alloc_pane_id();
        let pid2 = session.alloc_pane_id();
        assert_ne!(pid1, pid2);

        let wid1 = session.alloc_window_id();
        let wid2 = session.alloc_window_id();
        assert_ne!(wid1, wid2);
    }

    // --- Multiplexer ---

    #[test]
    fn test_mux_new() {
        let mux = Multiplexer::new();
        assert_eq!(mux.sessions.len(), 1);
        assert_eq!(mux.panes.len(), 1);
        assert_eq!(mux.active_session, 0);
    }

    #[test]
    fn test_mux_new_session() {
        let mut mux = Multiplexer::new();
        mux.new_session("second");
        assert_eq!(mux.sessions.len(), 2);
        assert_eq!(mux.active_session, 1);
        assert_eq!(mux.sessions[1].name, "second");
    }

    #[test]
    fn test_mux_detach_attach() {
        let mut mux = Multiplexer::new();
        mux.detach();
        assert!(!mux.sessions[0].attached);
        mux.attach(0);
        assert!(mux.sessions[0].attached);
    }

    #[test]
    fn test_mux_new_window() {
        let mut mux = Multiplexer::new();
        mux.new_window();
        let session = mux.active_session().unwrap();
        assert_eq!(session.windows.len(), 2);
        assert_eq!(session.active_window, 1);
    }

    #[test]
    fn test_mux_close_window() {
        let mut mux = Multiplexer::new();
        mux.new_window();
        mux.close_window();
        let session = mux.active_session().unwrap();
        assert_eq!(session.windows.len(), 1);
    }

    #[test]
    fn test_mux_next_prev_window() {
        let mut mux = Multiplexer::new();
        mux.new_window();
        mux.new_window();
        // At window 2
        mux.prev_window();
        assert_eq!(mux.active_session().unwrap().active_window, 1);
        mux.next_window();
        assert_eq!(mux.active_session().unwrap().active_window, 2);
    }

    #[test]
    fn test_mux_split_pane() {
        let mut mux = Multiplexer::new();
        mux.split_pane(SplitDir::Vertical);
        let session = mux.active_session().unwrap();
        let window = session.active_window().unwrap();
        assert_eq!(window.layout.pane_count(), 2);
        assert_eq!(mux.panes.len(), 2);
    }

    #[test]
    fn test_mux_close_pane() {
        let mut mux = Multiplexer::new();
        mux.split_pane(SplitDir::Vertical);
        mux.close_pane();
        let session = mux.active_session().unwrap();
        let window = session.active_window().unwrap();
        assert_eq!(window.layout.pane_count(), 1);
    }

    #[test]
    fn test_mux_next_prev_pane() {
        let mut mux = Multiplexer::new();
        mux.split_pane(SplitDir::Vertical);
        let p1 = mux.active_session().unwrap().active_window().unwrap().active_pane;
        mux.prev_pane();
        let p2 = mux.active_session().unwrap().active_window().unwrap().active_pane;
        assert_ne!(p1, p2);
        mux.next_pane();
        let p3 = mux.active_session().unwrap().active_window().unwrap().active_pane;
        assert_eq!(p1, p3);
    }

    #[test]
    fn test_mux_apply_layout() {
        let mut mux = Multiplexer::new();
        mux.split_pane(SplitDir::Vertical);
        mux.split_pane(SplitDir::Horizontal);
        mux.apply_layout(LayoutPreset::EvenHorizontal);
        let window = mux.active_session().unwrap().active_window().unwrap();
        assert_eq!(window.preset, LayoutPreset::EvenHorizontal);
        assert_eq!(window.layout.pane_count(), 3);
    }

    #[test]
    fn test_mux_rename_window() {
        let mut mux = Multiplexer::new();
        mux.rename_window("my-window");
        assert_eq!(mux.active_session().unwrap().active_window().unwrap().name, "my-window");
    }

    #[test]
    fn test_mux_kill_session() {
        let mut mux = Multiplexer::new();
        mux.new_session("to-kill");
        assert_eq!(mux.sessions.len(), 2);
        mux.kill_session(1);
        assert_eq!(mux.sessions.len(), 1);
    }

    #[test]
    fn test_mux_kill_only_session_noop() {
        let mut mux = Multiplexer::new();
        mux.kill_session(0);
        // Should not kill the last session
        assert_eq!(mux.sessions.len(), 1);
    }

    #[test]
    fn test_mux_process_command_new_session() {
        let mut mux = Multiplexer::new();
        mux.process_command(":new-session my-session");
        assert_eq!(mux.sessions.len(), 2);
        assert_eq!(mux.sessions[1].name, "my-session");
    }

    #[test]
    fn test_mux_process_command_split() {
        let mut mux = Multiplexer::new();
        mux.process_command(":split-window -h");
        let pane_count = mux.active_session().unwrap().active_window().unwrap().layout.pane_count();
        assert_eq!(pane_count, 2);
    }

    #[test]
    fn test_mux_process_command_layout() {
        let mut mux = Multiplexer::new();
        mux.split_pane(SplitDir::Vertical);
        mux.process_command(":select-layout even-vertical");
        let preset = mux.active_session().unwrap().active_window().unwrap().preset;
        assert_eq!(preset, LayoutPreset::EvenVertical);
    }

    #[test]
    fn test_mux_process_command_rename() {
        let mut mux = Multiplexer::new();
        mux.process_command(":rename-window test-name");
        assert_eq!(mux.active_session().unwrap().active_window().unwrap().name, "test-name");
    }

    #[test]
    fn test_mux_prefix_split_horizontal() {
        let mut mux = Multiplexer::new();
        mux.process_prefix_key('"');
        let pane_count = mux.active_session().unwrap().active_window().unwrap().layout.pane_count();
        assert_eq!(pane_count, 2);
    }

    #[test]
    fn test_mux_prefix_split_vertical() {
        let mut mux = Multiplexer::new();
        mux.process_prefix_key('%');
        let pane_count = mux.active_session().unwrap().active_window().unwrap().layout.pane_count();
        assert_eq!(pane_count, 2);
    }

    #[test]
    fn test_mux_prefix_new_window() {
        let mut mux = Multiplexer::new();
        mux.process_prefix_key('c');
        assert_eq!(mux.active_session().unwrap().windows.len(), 2);
    }

    #[test]
    fn test_mux_prefix_close_pane() {
        let mut mux = Multiplexer::new();
        mux.split_pane(SplitDir::Vertical);
        mux.process_prefix_key('x');
        assert_eq!(mux.active_session().unwrap().active_window().unwrap().layout.pane_count(), 1);
    }

    #[test]
    fn test_mux_prefix_detach() {
        let mut mux = Multiplexer::new();
        mux.process_prefix_key('d');
        assert!(!mux.active_session().unwrap().attached);
    }

    #[test]
    fn test_mux_prefix_session_chooser() {
        let mut mux = Multiplexer::new();
        assert!(!mux.session_chooser);
        mux.process_prefix_key('s');
        assert!(mux.session_chooser);
    }

    #[test]
    fn test_mux_prefix_window_chooser() {
        let mut mux = Multiplexer::new();
        assert!(!mux.window_chooser);
        mux.process_prefix_key('w');
        assert!(mux.window_chooser);
    }

    #[test]
    fn test_mux_render_nonempty() {
        let mux = Multiplexer::new();
        let cmds = mux.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_mux_render_detached() {
        let mut mux = Multiplexer::new();
        mux.detach();
        let cmds = mux.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_mux_render_with_splits() {
        let mut mux = Multiplexer::new();
        mux.split_pane(SplitDir::Vertical);
        mux.split_pane(SplitDir::Horizontal);
        let cmds = mux.render();
        assert!(cmds.len() > 10); // Should have many render commands
    }

    #[test]
    fn test_mux_status_message() {
        let mut mux = Multiplexer::new();
        mux.set_time(1000);
        mux.set_status("Hello");
        assert_eq!(mux.status_message, "Hello");
    }

    // --- 256 color ---

    #[test]
    fn test_color_256_standard() {
        assert_eq!(color_256(1), RED);
        assert_eq!(color_256(2), GREEN);
    }

    #[test]
    fn test_color_256_grayscale() {
        let c = color_256(232);
        // Should be a dark gray
        assert_eq!(c, Color::rgb(8, 8, 8));
    }

    #[test]
    fn test_color_256_cube() {
        // Color cube index 16 = (0,0,0) → black
        let c = color_256(16);
        assert_eq!(c, Color::rgb(0, 0, 0));
    }

    // --- Scrollback ---

    #[test]
    fn test_scrollback_limit() {
        let mut buf = TerminalBuffer::new(80, 2);
        for i in 0..MAX_SCROLLBACK + 100 {
            buf.write_str(&format!("Line {i}\n"));
        }
        assert!(buf.scrollback.len() <= MAX_SCROLLBACK);
    }
}

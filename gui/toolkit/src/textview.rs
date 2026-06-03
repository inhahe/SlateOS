#![allow(dead_code)]
//! Text view widgets for displaying text content (read-only or selectable).
//!
//! Two widget types:
//! - [`SimpleTextView`]: Plain or ANSI-colored text display (logs, terminal output).
//! - [`RichTextView`]: Formatted rich text with headings, lists, links, and styling.
//!
//! Both support vertical scrolling, text selection, copy-to-clipboard, and search.

use crate::color::Color;
use crate::event::{Event, EventResult, Key, KeyEvent, MouseEvent, MouseEventKind};
use crate::render::{FontWeightHint, RenderCommand, RenderTree};
use crate::style::CornerRadii;

// ---------------------------------------------------------------------------
// Catppuccin Mocha palette (dark theme)
// ---------------------------------------------------------------------------

/// Background color (Mocha Base).
const BG_COLOR: Color = Color::from_hex(0x1E1E2E);
/// Surface color (Mocha Surface0) for gutters/code blocks.
const SURFACE_COLOR: Color = Color::from_hex(0x313244);
/// Text color (Mocha Text).
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
/// Subtext (Mocha Subtext0) for line numbers.
const SUBTEXT_COLOR: Color = Color::from_hex(0xA6ADC8);
/// Selection highlight (Mocha Blue at 40% opacity).
const SELECTION_COLOR: Color = Color::rgba(137, 180, 250, 102);
/// Search match highlight (Mocha Yellow at 50% opacity).
const SEARCH_MATCH_COLOR: Color = Color::rgba(249, 226, 175, 128);
/// Current search match highlight (Mocha Peach at 60% opacity).
const CURRENT_MATCH_COLOR: Color = Color::rgba(250, 179, 135, 153);
/// Link color (Mocha Blue).
const LINK_COLOR: Color = Color::from_hex(0x89B4FA);
/// Heading color (Mocha Mauve).
const HEADING_COLOR: Color = Color::from_hex(0xCBA6F7);
/// Code block background (Mocha Mantle).
const CODE_BG_COLOR: Color = Color::from_hex(0x181825);
/// Horizontal rule color (Mocha Overlay0).
const HR_COLOR: Color = Color::from_hex(0x6C7086);
/// Bullet/list marker color (Mocha Teal).
const LIST_MARKER_COLOR: Color = Color::from_hex(0x94E2D5);

// ---------------------------------------------------------------------------
// Font metrics
// ---------------------------------------------------------------------------

/// Default character width in pixels (monospace).
const DEFAULT_CHAR_WIDTH: f32 = 8.0;
/// Default line height in pixels.
const DEFAULT_LINE_HEIGHT: f32 = 16.0;
/// Default font size in points.
const DEFAULT_FONT_SIZE: f32 = 14.0;

// ---------------------------------------------------------------------------
// Text position and selection
// ---------------------------------------------------------------------------

/// A position in the text (line + column offset).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPosition {
    pub line: usize,
    pub col: usize,
}

impl TextPosition {
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }

    pub const ZERO: Self = Self { line: 0, col: 0 };
}

/// A selected range of text (start..end, always normalized so start <= end).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: TextPosition,
    pub end: TextPosition,
}

impl Selection {
    pub fn new(a: TextPosition, b: TextPosition) -> Self {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// Whether the selection is empty (zero-length).
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Whether a given position falls within this selection.
    pub fn contains(&self, pos: TextPosition) -> bool {
        pos >= self.start && pos < self.end
    }
}

// ---------------------------------------------------------------------------
// ANSI SGR parser
// ---------------------------------------------------------------------------

/// Style attributes from ANSI SGR sequences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub struct AnsiStyle {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}


/// A span of text with uniform styling (used in SimpleTextView).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub style: AnsiStyle,
}

impl StyledSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: AnsiStyle::default(),
        }
    }

    pub fn styled(text: impl Into<String>, style: AnsiStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

/// Standard ANSI 4-bit colors (indices 0-7 for normal, 8-15 for bright).
const ANSI_COLORS: [Color; 16] = [
    Color::rgb(0, 0, 0),       // 0: black
    Color::rgb(205, 49, 49),   // 1: red
    Color::rgb(13, 188, 121),  // 2: green
    Color::rgb(229, 229, 16),  // 3: yellow
    Color::rgb(36, 114, 200),  // 4: blue
    Color::rgb(188, 63, 188),  // 5: magenta
    Color::rgb(17, 168, 205),  // 6: cyan
    Color::rgb(229, 229, 229), // 7: white
    Color::rgb(102, 102, 102), // 8: bright black (gray)
    Color::rgb(241, 76, 76),   // 9: bright red
    Color::rgb(35, 209, 139),  // 10: bright green
    Color::rgb(245, 245, 67),  // 11: bright yellow
    Color::rgb(59, 142, 234),  // 12: bright blue
    Color::rgb(214, 112, 214), // 13: bright magenta
    Color::rgb(41, 184, 219),  // 14: bright cyan
    Color::rgb(255, 255, 255), // 15: bright white
];

/// Convert a 256-color index to an RGB Color.
fn color_from_256(index: u8) -> Color {
    match index {
        0..=15 => ANSI_COLORS[index as usize],
        // 216-color cube (indices 16-231): 6x6x6
        16..=231 => {
            let idx = index - 16;
            let b = (idx % 6) * 51;
            let g = ((idx / 6) % 6) * 51;
            let r = (idx / 36) * 51;
            Color::rgb(r, g, b)
        }
        // Grayscale ramp (indices 232-255): 24 shades
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            Color::rgb(gray, gray, gray)
        }
    }
}

/// Parse a string containing ANSI escape sequences into styled spans.
/// Returns one Vec<StyledSpan> per line.
pub fn parse_ansi(input: &str) -> Vec<Vec<StyledSpan>> {
    let mut lines: Vec<Vec<StyledSpan>> = Vec::new();
    let mut current_line: Vec<StyledSpan> = Vec::new();
    let mut current_text = String::new();
    let mut current_style = AnsiStyle::default();

    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'\x1b' && i + 1 < len && bytes[i + 1] == b'[' {
            // Flush current text as a span
            if !current_text.is_empty() {
                current_line.push(StyledSpan::styled(
                    core::mem::take(&mut current_text),
                    current_style,
                ));
            }

            // Parse CSI sequence
            i += 2; // skip ESC [
            let mut params: Vec<u16> = Vec::new();
            let mut num: Option<u16> = None;

            while i < len {
                match bytes[i] {
                    b'0'..=b'9' => {
                        let digit = (bytes[i] - b'0') as u16;
                        num = Some(num.unwrap_or(0).saturating_mul(10).saturating_add(digit));
                        i += 1;
                    }
                    b';' => {
                        params.push(num.unwrap_or(0));
                        num = None;
                        i += 1;
                    }
                    // Final byte of SGR sequence
                    b'm' => {
                        params.push(num.unwrap_or(0));
                        apply_sgr_params(&params, &mut current_style);
                        i += 1;
                        break;
                    }
                    // Some other CSI sequence we don't handle — skip to final byte
                    b'@'..=b'~' => {
                        i += 1;
                        break;
                    }
                    _ => {
                        i += 1;
                        break;
                    }
                }
            }
        } else if bytes[i] == b'\n' {
            // End of line
            if !current_text.is_empty() {
                current_line.push(StyledSpan::styled(
                    core::mem::take(&mut current_text),
                    current_style,
                ));
            }
            lines.push(core::mem::take(&mut current_line));
            i += 1;
        } else if bytes[i] == b'\r' {
            // Skip carriage return
            i += 1;
        } else {
            // Collect UTF-8 characters
            let ch_start = i;
            // Advance past a single UTF-8 character
            if bytes[i] & 0x80 == 0 {
                current_text.push(bytes[i] as char);
                i += 1;
            } else {
                // Multi-byte UTF-8
                let ch_len = if bytes[i] & 0xE0 == 0xC0 {
                    2
                } else if bytes[i] & 0xF0 == 0xE0 {
                    3
                } else {
                    4
                };
                let end = (ch_start + ch_len).min(len);
                if let Ok(s) = core::str::from_utf8(&bytes[ch_start..end]) {
                    current_text.push_str(s);
                }
                i = end;
            }
        }
    }

    // Flush remaining
    if !current_text.is_empty() {
        current_line.push(StyledSpan::styled(current_text, current_style));
    }
    if !current_line.is_empty() || lines.is_empty() {
        lines.push(current_line);
    }

    lines
}

/// Apply SGR (Select Graphic Rendition) parameters to a style.
fn apply_sgr_params(params: &[u16], style: &mut AnsiStyle) {
    let mut i = 0;
    while i < params.len() {
        match params[i] {
            0 => *style = AnsiStyle::default(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            7 => style.reverse = true,
            21 | 22 => {
                style.bold = false;
                style.dim = false;
            }
            23 => style.italic = false,
            24 => style.underline = false,
            27 => style.reverse = false,
            // Foreground colors 30-37
            30..=37 => style.fg = Some(ANSI_COLORS[(params[i] - 30) as usize]),
            // Default foreground
            39 => style.fg = None,
            // Background colors 40-47
            40..=47 => style.bg = Some(ANSI_COLORS[(params[i] - 40) as usize]),
            // Default background
            49 => style.bg = None,
            // Bright foreground 90-97
            90..=97 => style.fg = Some(ANSI_COLORS[(params[i] - 90 + 8) as usize]),
            // Bright background 100-107
            100..=107 => style.bg = Some(ANSI_COLORS[(params[i] - 100 + 8) as usize]),
            // Extended color: 38;5;N or 38;2;R;G;B
            38
                if i + 1 < params.len() => {
                    match params[i + 1] {
                        5
                            // 256-color
                            if i + 2 < params.len() => {
                                style.fg = Some(color_from_256(params[i + 2] as u8));
                                i += 2;
                            }
                        2
                            // True color
                            if i + 4 < params.len() => {
                                style.fg = Some(Color::rgb(
                                    params[i + 2] as u8,
                                    params[i + 3] as u8,
                                    params[i + 4] as u8,
                                ));
                                i += 4;
                            }
                        _ => {}
                    }
                    i += 1;
                }
            // Extended background: 48;5;N or 48;2;R;G;B
            48
                if i + 1 < params.len() => {
                    match params[i + 1] {
                        5
                            if i + 2 < params.len() => {
                                style.bg = Some(color_from_256(params[i + 2] as u8));
                                i += 2;
                            }
                        2
                            if i + 4 < params.len() => {
                                style.bg = Some(Color::rgb(
                                    params[i + 2] as u8,
                                    params[i + 3] as u8,
                                    params[i + 4] as u8,
                                ));
                                i += 4;
                            }
                        _ => {}
                    }
                    i += 1;
                }
            _ => {} // Unknown SGR parameter — ignore
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// SimpleTextView
// ---------------------------------------------------------------------------

/// Configuration for SimpleTextView.
#[derive(Clone, Debug)]
pub struct SimpleTextViewConfig {
    /// Character width in pixels (monospace).
    pub char_width: f32,
    /// Line height in pixels.
    pub line_height: f32,
    /// Font size in points.
    pub font_size: f32,
    /// Whether to show line numbers in the gutter.
    pub show_line_numbers: bool,
    /// Maximum number of lines to retain (oldest dropped). 0 = unlimited.
    pub max_lines: usize,
    /// Whether the view auto-scrolls to bottom on append.
    pub auto_scroll: bool,
    /// Whether text is selectable.
    pub selectable: bool,
}

impl Default for SimpleTextViewConfig {
    fn default() -> Self {
        Self {
            char_width: DEFAULT_CHAR_WIDTH,
            line_height: DEFAULT_LINE_HEIGHT,
            font_size: DEFAULT_FONT_SIZE,
            show_line_numbers: false,
            max_lines: 10000,
            auto_scroll: true,
            selectable: true,
        }
    }
}

/// A plain/ANSI text view widget for log/terminal-like output.
///
/// Stores text as lines of styled spans. Supports scrolling, selection,
/// search-highlight, and append-only mode.
#[derive(Clone, Debug)]
pub struct SimpleTextView {
    /// Lines of styled spans.
    lines: Vec<Vec<StyledSpan>>,
    /// Vertical scroll offset (in lines).
    scroll_offset: usize,
    /// Widget width in pixels.
    width: f32,
    /// Widget height in pixels.
    height: f32,
    /// Current text selection (if any).
    selection: Option<Selection>,
    /// Anchor point for in-progress drag selection.
    selection_anchor: Option<TextPosition>,
    /// Whether the user is currently dragging (mouse held down).
    dragging: bool,
    /// Search state.
    search: SearchState,
    /// Configuration.
    pub config: SimpleTextViewConfig,
}

/// Search state for highlighting matches.
#[derive(Clone, Debug, Default)]
pub struct SearchState {
    /// Current search query (empty = no active search).
    pub query: String,
    /// All match positions (line, start_col, end_col).
    pub matches: Vec<(usize, usize, usize)>,
    /// Index of the currently-focused match (-1 = none).
    pub current_match: Option<usize>,
    /// Whether search is case-sensitive.
    pub case_sensitive: bool,
}

impl SimpleTextView {
    /// Create a new empty SimpleTextView with default config.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            lines: vec![Vec::new()],
            scroll_offset: 0,
            width,
            height,
            selection: None,
            selection_anchor: None,
            dragging: false,
            search: SearchState::default(),
            config: SimpleTextViewConfig::default(),
        }
    }

    /// Create with a custom configuration.
    pub fn with_config(width: f32, height: f32, config: SimpleTextViewConfig) -> Self {
        Self {
            lines: vec![Vec::new()],
            scroll_offset: 0,
            width,
            height,
            selection: None,
            selection_anchor: None,
            dragging: false,
            search: SearchState::default(),
            config,
        }
    }

    /// Number of visible lines in the viewport.
    pub fn visible_lines(&self) -> usize {
        if self.config.line_height <= 0.0 {
            return 0;
        }
        (self.height / self.config.line_height).floor() as usize
    }

    /// Total number of lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Width of the line-number gutter in pixels.
    fn gutter_width(&self) -> f32 {
        if self.config.show_line_numbers {
            let digits = (self.lines.len() as f32).log10().floor() as usize + 1;
            let digits = digits.max(3); // minimum 3 digits wide
            (digits as f32 + 1.0) * self.config.char_width
        } else {
            0.0
        }
    }

    /// Whether the view is scrolled to the bottom.
    pub fn is_at_bottom(&self) -> bool {
        let visible = self.visible_lines();
        if self.lines.len() <= visible {
            true
        } else {
            self.scroll_offset >= self.lines.len() - visible
        }
    }

    /// Set plain text content (replaces everything).
    pub fn set_text(&mut self, text: &str) {
        self.lines = parse_ansi(text);
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        self.enforce_max_lines();
        self.clamp_scroll();
        self.selection = None;
        self.refresh_search();
    }

    /// Append text (may contain ANSI). Auto-scrolls if at bottom.
    pub fn append(&mut self, text: &str) {
        let was_at_bottom = self.is_at_bottom() && self.config.auto_scroll;

        let new_lines = parse_ansi(text);
        for (i, line_spans) in new_lines.into_iter().enumerate() {
            if i == 0 {
                // Append to the last existing line
                if let Some(last) = self.lines.last_mut() {
                    last.extend(line_spans);
                } else {
                    self.lines.push(line_spans);
                }
            } else {
                self.lines.push(line_spans);
            }
        }

        self.enforce_max_lines();

        if was_at_bottom {
            self.scroll_to_bottom();
        }

        self.refresh_search();
    }

    /// Append a complete line (no trailing newline needed).
    pub fn append_line(&mut self, text: &str) {
        let was_at_bottom = self.is_at_bottom() && self.config.auto_scroll;

        let mut new_lines = parse_ansi(text);
        if new_lines.is_empty() {
            new_lines.push(Vec::new());
        }
        self.lines.extend(new_lines);

        self.enforce_max_lines();

        if was_at_bottom {
            self.scroll_to_bottom();
        }

        self.refresh_search();
    }

    /// Clear all text.
    pub fn clear(&mut self) {
        self.lines = vec![Vec::new()];
        self.scroll_offset = 0;
        self.selection = None;
        self.search.matches.clear();
    }

    /// Drop oldest lines to stay within max_lines.
    fn enforce_max_lines(&mut self) {
        if self.config.max_lines > 0 && self.lines.len() > self.config.max_lines {
            let excess = self.lines.len() - self.config.max_lines;
            self.lines.drain(0..excess);
            // Adjust scroll offset
            self.scroll_offset = self.scroll_offset.saturating_sub(excess);
            // Adjust selection
            if let Some(sel) = &mut self.selection {
                if sel.start.line < excess {
                    if sel.end.line < excess {
                        self.selection = None;
                    } else {
                        sel.start = TextPosition::new(0, 0);
                        sel.end.line -= excess;
                    }
                } else {
                    sel.start.line -= excess;
                    sel.end.line -= excess;
                }
            }
        }
    }

    /// Clamp scroll offset to valid range.
    fn clamp_scroll(&mut self) {
        let visible = self.visible_lines();
        if self.lines.len() <= visible {
            self.scroll_offset = 0;
        } else {
            let max_offset = self.lines.len() - visible;
            if self.scroll_offset > max_offset {
                self.scroll_offset = max_offset;
            }
        }
    }

    /// Scroll to the very bottom.
    pub fn scroll_to_bottom(&mut self) {
        let visible = self.visible_lines();
        if self.lines.len() > visible {
            self.scroll_offset = self.lines.len() - visible;
        } else {
            self.scroll_offset = 0;
        }
    }

    /// Scroll to the very top.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Scroll by a number of lines (positive = down, negative = up).
    pub fn scroll_by(&mut self, delta: i32) {
        if delta < 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub((-delta) as usize);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_add(delta as usize);
        }
        self.clamp_scroll();
    }

    /// Convert pixel coordinates to a text position.
    fn hit_test(&self, x: f32, y: f32) -> TextPosition {
        let gutter = self.gutter_width();
        let text_x = (x - gutter).max(0.0);
        let col = (text_x / self.config.char_width) as usize;
        let line_in_view = (y / self.config.line_height) as usize;
        let line = (self.scroll_offset + line_in_view).min(self.lines.len().saturating_sub(1));

        // Clamp column to line length
        let line_len = self.line_char_count(line);
        let col = col.min(line_len);

        TextPosition::new(line, col)
    }

    /// Get character count of a line.
    fn line_char_count(&self, line: usize) -> usize {
        self.lines
            .get(line)
            .map(|spans| spans.iter().map(|s| s.text.len()).sum())
            .unwrap_or(0)
    }

    /// Get the plain text of a line.
    fn line_text(&self, line: usize) -> String {
        self.lines
            .get(line)
            .map(|spans| spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .unwrap_or_default()
    }

    /// Get selected text as a plain string.
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;
        if sel.is_empty() {
            return None;
        }

        let mut result = String::new();
        for line_idx in sel.start.line..=sel.end.line {
            let text = self.line_text(line_idx);
            let start_col = if line_idx == sel.start.line {
                sel.start.col
            } else {
                0
            };
            let end_col = if line_idx == sel.end.line {
                sel.end.col
            } else {
                text.len()
            };

            let start = start_col.min(text.len());
            let end = end_col.min(text.len());
            result.push_str(&text[start..end]);

            if line_idx < sel.end.line {
                result.push('\n');
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Select all text.
    pub fn select_all(&mut self) {
        if self.lines.is_empty() {
            return;
        }
        let last_line = self.lines.len() - 1;
        let last_col = self.line_char_count(last_line);
        self.selection = Some(Selection::new(
            TextPosition::ZERO,
            TextPosition::new(last_line, last_col),
        ));
    }

    /// Find word boundaries around a position (for double-click).
    fn word_at(&self, pos: TextPosition) -> Selection {
        let text = self.line_text(pos.line);
        let bytes = text.as_bytes();
        let col = pos.col.min(text.len());

        // Scan left to find word start
        let mut start = col;
        while start > 0 && is_word_char(bytes[start - 1]) {
            start -= 1;
        }

        // Scan right to find word end
        let mut end = col;
        while end < bytes.len() && is_word_char(bytes[end]) {
            end += 1;
        }

        Selection::new(
            TextPosition::new(pos.line, start),
            TextPosition::new(pos.line, end),
        )
    }

    // ----- Search -----

    /// Start or update a search. Highlights all matches.
    pub fn find(&mut self, query: &str, case_sensitive: bool) {
        self.search.query = query.to_string();
        self.search.case_sensitive = case_sensitive;
        self.search.current_match = None;
        self.refresh_search();

        // Focus on first match if any
        if !self.search.matches.is_empty() {
            self.search.current_match = Some(0);
            self.scroll_to_match(0);
        }
    }

    /// Navigate to the next match.
    pub fn next_match(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        let next = match self.search.current_match {
            Some(idx) => (idx + 1) % self.search.matches.len(),
            None => 0,
        };
        self.search.current_match = Some(next);
        self.scroll_to_match(next);
    }

    /// Navigate to the previous match.
    pub fn prev_match(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        let prev = match self.search.current_match {
            Some(0) | None => self.search.matches.len() - 1,
            Some(idx) => idx - 1,
        };
        self.search.current_match = Some(prev);
        self.scroll_to_match(prev);
    }

    /// Clear search state.
    pub fn clear_search(&mut self) {
        self.search = SearchState::default();
    }

    /// Number of search matches found.
    pub fn match_count(&self) -> usize {
        self.search.matches.len()
    }

    /// Recalculate search matches after text change.
    fn refresh_search(&mut self) {
        self.search.matches.clear();
        if self.search.query.is_empty() {
            return;
        }

        let query = if self.search.case_sensitive {
            self.search.query.clone()
        } else {
            self.search.query.to_lowercase()
        };

        for (line_idx, _) in self.lines.iter().enumerate() {
            let text = self.line_text(line_idx);
            let haystack = if self.search.case_sensitive {
                text.clone()
            } else {
                text.to_lowercase()
            };

            let mut start = 0;
            while let Some(pos) = haystack[start..].find(&query) {
                let abs_pos = start + pos;
                self.search
                    .matches
                    .push((line_idx, abs_pos, abs_pos + query.len()));
                start = abs_pos + 1; // Allow overlapping matches
            }
        }
    }

    /// Scroll to make a match visible.
    fn scroll_to_match(&mut self, match_idx: usize) {
        if let Some(&(line, _, _)) = self.search.matches.get(match_idx) {
            let visible = self.visible_lines();
            if line < self.scroll_offset || line >= self.scroll_offset + visible {
                // Center the match
                self.scroll_offset = line.saturating_sub(visible / 2);
                self.clamp_scroll();
            }
        }
    }

    // ----- Event handling -----

    /// Handle an event. Returns EventResult and optionally a clipboard string (on Ctrl+C).
    pub fn handle_event(&mut self, event: &Event) -> (EventResult, Option<String>) {
        match event {
            Event::Mouse(me) => self.handle_mouse(me),
            Event::Key(ke) => self.handle_key(ke),
            _ => (EventResult::Ignored, None),
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> (EventResult, Option<String>) {
        if !self.config.selectable {
            // Still handle scroll
            if let MouseEventKind::Scroll { dy, .. } = event.kind {
                let lines = (dy / self.config.line_height).round() as i32;
                self.scroll_by(-lines);
                return (EventResult::Consumed, None);
            }
            return (EventResult::Ignored, None);
        }

        match &event.kind {
            MouseEventKind::Press(crate::event::MouseButton::Left) => {
                let pos = self.hit_test(event.x, event.y);
                self.selection_anchor = Some(pos);
                self.selection = Some(Selection::new(pos, pos));
                self.dragging = true;
                (EventResult::Consumed, None)
            }
            MouseEventKind::Release(crate::event::MouseButton::Left) => {
                self.dragging = false;
                (EventResult::Consumed, None)
            }
            MouseEventKind::Move if self.dragging => {
                let pos = self.hit_test(event.x, event.y);
                if let Some(anchor) = self.selection_anchor {
                    self.selection = Some(Selection::new(anchor, pos));
                }
                (EventResult::Consumed, None)
            }
            MouseEventKind::DoubleClick(crate::event::MouseButton::Left) => {
                let pos = self.hit_test(event.x, event.y);
                self.selection = Some(self.word_at(pos));
                self.selection_anchor = None;
                self.dragging = false;
                (EventResult::Consumed, None)
            }
            MouseEventKind::Scroll { dy, .. } => {
                let lines = (dy / self.config.line_height).round() as i32;
                self.scroll_by(-lines);
                (EventResult::Consumed, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) -> (EventResult, Option<String>) {
        if !event.pressed {
            return (EventResult::Ignored, None);
        }

        // Ctrl+A = select all
        if event.modifiers.ctrl && event.key == Key::A {
            self.select_all();
            return (EventResult::Consumed, None);
        }

        // Ctrl+C = copy selection
        if event.modifiers.ctrl && event.key == Key::C {
            let text = self.selected_text();
            return (EventResult::Consumed, text);
        }

        // Page Up / Page Down
        match event.key {
            Key::PageUp => {
                let page = self.visible_lines().max(1) as i32;
                self.scroll_by(-page);
                return (EventResult::Consumed, None);
            }
            Key::PageDown => {
                let page = self.visible_lines().max(1) as i32;
                self.scroll_by(page);
                return (EventResult::Consumed, None);
            }
            Key::Home if event.modifiers.ctrl => {
                self.scroll_to_top();
                return (EventResult::Consumed, None);
            }
            Key::End if event.modifiers.ctrl => {
                self.scroll_to_bottom();
                return (EventResult::Consumed, None);
            }
            Key::Up => {
                self.scroll_by(-1);
                return (EventResult::Consumed, None);
            }
            Key::Down => {
                self.scroll_by(1);
                return (EventResult::Consumed, None);
            }
            _ => {}
        }

        (EventResult::Ignored, None)
    }

    // ----- Rendering -----

    /// Resize the widget viewport.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        self.clamp_scroll();
    }

    /// Render the widget to a RenderTree.
    pub fn render(&self, tree: &mut RenderTree) {
        // Background
        tree.fill_rect(0.0, 0.0, self.width, self.height, BG_COLOR);

        // Clip to widget bounds
        tree.clip(0.0, 0.0, self.width, self.height);

        let gutter_w = self.gutter_width();
        let visible = self.visible_lines();
        let _end_line = (self.scroll_offset + visible).min(self.lines.len());

        // Draw gutter background
        if self.config.show_line_numbers && gutter_w > 0.0 {
            tree.fill_rect(0.0, 0.0, gutter_w, self.height, SURFACE_COLOR);
        }

        for view_line in 0..visible {
            let line_idx = self.scroll_offset + view_line;
            if line_idx >= self.lines.len() {
                break;
            }

            let y = view_line as f32 * self.config.line_height;

            // Line number
            if self.config.show_line_numbers {
                let num_str = format!("{}", line_idx + 1);
                let num_x = gutter_w - (num_str.len() as f32 + 0.5) * self.config.char_width;
                tree.push(RenderCommand::Text {
                    x: num_x,
                    y,
                    text: num_str,
                    color: SUBTEXT_COLOR,
                    font_size: self.config.font_size,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Selection highlight for this line
            if let Some(ref sel) = self.selection
                && !sel.is_empty() && line_idx >= sel.start.line && line_idx <= sel.end.line {
                    let line_len = self.line_char_count(line_idx);
                    let sel_start = if line_idx == sel.start.line {
                        sel.start.col
                    } else {
                        0
                    };
                    let sel_end = if line_idx == sel.end.line {
                        sel.end.col
                    } else {
                        line_len
                    };
                    if sel_start < sel_end {
                        let x1 = gutter_w + sel_start as f32 * self.config.char_width;
                        let x2 = gutter_w + sel_end as f32 * self.config.char_width;
                        tree.fill_rect(
                            x1,
                            y,
                            x2 - x1,
                            self.config.line_height,
                            SELECTION_COLOR,
                        );
                    }
                }

            // Search match highlights for this line
            for (match_idx, &(ml, ms, me)) in self.search.matches.iter().enumerate() {
                if ml == line_idx {
                    let color = if self.search.current_match == Some(match_idx) {
                        CURRENT_MATCH_COLOR
                    } else {
                        SEARCH_MATCH_COLOR
                    };
                    let x1 = gutter_w + ms as f32 * self.config.char_width;
                    let x2 = gutter_w + me as f32 * self.config.char_width;
                    tree.fill_rect(x1, y, x2 - x1, self.config.line_height, color);
                }
            }

            // Render spans
            let mut x = gutter_w;
            if let Some(spans) = self.lines.get(line_idx) {
                for span in spans {
                    let fg = resolve_span_fg(&span.style);
                    let weight = if span.style.bold {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    };

                    // Background color
                    if let Some(bg) = resolve_span_bg(&span.style) {
                        let span_width = span.text.len() as f32 * self.config.char_width;
                        tree.fill_rect(x, y, span_width, self.config.line_height, bg);
                    }

                    tree.push(RenderCommand::Text {
                        x,
                        y,
                        text: span.text.clone(),
                        color: fg,
                        font_size: self.config.font_size,
                        font_weight: weight,
                        max_width: None,
                    });

                    // Underline
                    if span.style.underline {
                        let span_width = span.text.len() as f32 * self.config.char_width;
                        let underline_y = y + self.config.line_height - 2.0;
                        tree.push(RenderCommand::Line {
                            x1: x,
                            y1: underline_y,
                            x2: x + span_width,
                            y2: underline_y,
                            color: fg,
                            width: 1.0,
                        });
                    }

                    x += span.text.len() as f32 * self.config.char_width;
                }
            }
        }

        tree.unclip();
    }
}

/// Resolve the effective foreground color for a span.
fn resolve_span_fg(style: &AnsiStyle) -> Color {
    let base = style.fg.unwrap_or(TEXT_COLOR);
    if style.reverse {
        style.bg.unwrap_or(BG_COLOR)
    } else if style.dim {
        // Dim: blend toward background
        Color::rgba(
            ((base.r as u16 + BG_COLOR.r as u16) / 2) as u8,
            ((base.g as u16 + BG_COLOR.g as u16) / 2) as u8,
            ((base.b as u16 + BG_COLOR.b as u16) / 2) as u8,
            base.a,
        )
    } else {
        base
    }
}

/// Resolve the effective background color for a span.
fn resolve_span_bg(style: &AnsiStyle) -> Option<Color> {
    if style.reverse {
        Some(style.fg.unwrap_or(TEXT_COLOR))
    } else {
        style.bg
    }
}

/// Check whether a byte is a "word" character (for double-click selection).
fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ===========================================================================
// RichTextView
// ===========================================================================

/// Font weight for rich text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum RichFontWeight {
    #[default]
    Normal,
    Bold,
}


/// Font style for rich text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum RichFontStyle {
    #[default]
    Normal,
    Italic,
}


/// Font size specification.
#[derive(Clone, Copy, Debug, PartialEq)]
#[derive(Default)]
pub enum FontSize {
    /// Relative sizes.
    Small,
    #[default]
    Normal,
    Large,
    XLarge,
    /// Absolute size in points.
    Points(f32),
}


impl FontSize {
    /// Resolve to concrete points given a base size.
    pub fn to_points(self, base: f32) -> f32 {
        match self {
            FontSize::Small => base * 0.85,
            FontSize::Normal => base,
            FontSize::Large => base * 1.25,
            FontSize::XLarge => base * 1.6,
            FontSize::Points(p) => p,
        }
    }
}

/// Styling for a rich text span.
#[derive(Clone, Debug, PartialEq)]
pub struct RichSpanStyle {
    pub weight: RichFontWeight,
    pub font_style: RichFontStyle,
    pub font_size: FontSize,
    pub fg_color: Option<Color>,
    pub bg_color: Option<Color>,
    pub underline: bool,
    pub strikethrough: bool,
    /// If Some, this span is a clickable link.
    pub link: Option<String>,
}

impl Default for RichSpanStyle {
    fn default() -> Self {
        Self {
            weight: RichFontWeight::Normal,
            font_style: RichFontStyle::Normal,
            font_size: FontSize::Normal,
            fg_color: None,
            bg_color: None,
            underline: false,
            strikethrough: false,
            link: None,
        }
    }
}

/// A span of rich text with uniform style.
#[derive(Clone, Debug, PartialEq)]
pub struct RichSpan {
    pub text: String,
    pub style: RichSpanStyle,
}

impl RichSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: RichSpanStyle::default(),
        }
    }

    pub fn styled(text: impl Into<String>, style: RichSpanStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub fn bold(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: RichSpanStyle {
                weight: RichFontWeight::Bold,
                ..Default::default()
            },
        }
    }

    pub fn link(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: RichSpanStyle {
                fg_color: Some(LINK_COLOR),
                underline: true,
                link: Some(url.into()),
                ..Default::default()
            },
        }
    }
}

/// Heading level (h1-h4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadingLevel {
    H1,
    H2,
    H3,
    H4,
}

impl HeadingLevel {
    /// Font size multiplier for this heading level.
    pub fn size_multiplier(self) -> f32 {
        match self {
            HeadingLevel::H1 => 2.0,
            HeadingLevel::H2 => 1.6,
            HeadingLevel::H3 => 1.3,
            HeadingLevel::H4 => 1.1,
        }
    }

    /// Spacing above in lines.
    pub fn spacing_above(self) -> f32 {
        match self {
            HeadingLevel::H1 => 1.5,
            HeadingLevel::H2 => 1.2,
            HeadingLevel::H3 => 1.0,
            HeadingLevel::H4 => 0.8,
        }
    }

    /// Spacing below in lines.
    pub fn spacing_below(self) -> f32 {
        match self {
            HeadingLevel::H1 => 0.8,
            HeadingLevel::H2 => 0.6,
            HeadingLevel::H3 => 0.4,
            HeadingLevel::H4 => 0.3,
        }
    }
}

/// List style type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Numbered,
}

/// A block-level element in rich text.
#[derive(Clone, Debug, PartialEq)]
pub enum RichBlock {
    /// A paragraph of inline spans.
    Paragraph {
        spans: Vec<RichSpan>,
        spacing_above: f32,
        spacing_below: f32,
    },
    /// A heading.
    Heading {
        level: HeadingLevel,
        spans: Vec<RichSpan>,
    },
    /// A list item (may be nested).
    ListItem {
        kind: ListKind,
        index: usize,
        indent_level: usize,
        spans: Vec<RichSpan>,
    },
    /// A code block (monospace, background).
    CodeBlock {
        code: String,
        language: Option<String>,
    },
    /// A horizontal rule.
    HorizontalRule,
    /// An inline image placeholder.
    ImagePlaceholder {
        width: f32,
        height: f32,
        alt_text: String,
    },
}

// ---------------------------------------------------------------------------
// Wrapped line for layout
// ---------------------------------------------------------------------------

/// A single visual line after word-wrapping, referencing back to a block.
#[derive(Clone, Debug)]
struct WrappedLine {
    /// Index of the source block in `blocks`.
    block_idx: usize,
    /// Spans for this visual line (subset of block spans after wrapping).
    spans: Vec<RichSpan>,
    /// Y position of this line (computed during layout).
    y: f32,
    /// Line height for this line.
    line_height: f32,
    /// Left indent (for lists, code blocks).
    indent: f32,
    /// Whether this is the first visual line of its block (for heading spacing, etc.).
    is_block_start: bool,
}

/// Configuration for RichTextView.
#[derive(Clone, Debug)]
pub struct RichTextViewConfig {
    /// Base character width in pixels (monospace).
    pub char_width: f32,
    /// Base line height in pixels.
    pub line_height: f32,
    /// Base font size in points.
    pub font_size: f32,
    /// Whether to show line numbers.
    pub show_line_numbers: bool,
    /// Whether text is selectable.
    pub selectable: bool,
    /// Paragraph spacing (in multiples of line_height).
    pub paragraph_spacing: f32,
    /// List indent per level (in characters).
    pub list_indent_chars: usize,
    /// Code block padding in pixels.
    pub code_block_padding: f32,
}

impl Default for RichTextViewConfig {
    fn default() -> Self {
        Self {
            char_width: DEFAULT_CHAR_WIDTH,
            line_height: DEFAULT_LINE_HEIGHT,
            font_size: DEFAULT_FONT_SIZE,
            show_line_numbers: false,
            selectable: true,
            paragraph_spacing: 0.5,
            list_indent_chars: 4,
            code_block_padding: 8.0,
        }
    }
}

/// A rich text view widget for displaying formatted text content.
///
/// Accepts structured blocks (paragraphs, headings, lists, code blocks)
/// and renders them with word-wrapping, styling, and selection support.
#[derive(Clone, Debug)]
pub struct RichTextView {
    /// Source blocks.
    blocks: Vec<RichBlock>,
    /// Word-wrapped layout lines (rebuilt on resize or content change).
    wrapped_lines: Vec<WrappedLine>,
    /// Total content height after layout.
    content_height: f32,
    /// Vertical scroll offset in pixels.
    scroll_offset_px: f32,
    /// Widget width in pixels.
    width: f32,
    /// Widget height in pixels.
    height: f32,
    /// Current text selection.
    selection: Option<Selection>,
    /// Anchor for drag selection.
    selection_anchor: Option<TextPosition>,
    /// Whether currently dragging.
    dragging: bool,
    /// Search state.
    search: SearchState,
    /// Configuration.
    pub config: RichTextViewConfig,
    /// Layout is dirty and needs rebuild.
    layout_dirty: bool,
}

/// Event emitted by RichTextView (e.g., link clicks).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RichTextEvent {
    /// A link was clicked.
    LinkClicked(String),
    /// Copy requested (Ctrl+C) — carries the selected text.
    Copy(String),
}

impl RichTextView {
    /// Create a new empty RichTextView.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            blocks: Vec::new(),
            wrapped_lines: Vec::new(),
            content_height: 0.0,
            scroll_offset_px: 0.0,
            width,
            height,
            selection: None,
            selection_anchor: None,
            dragging: false,
            search: SearchState::default(),
            config: RichTextViewConfig::default(),
            layout_dirty: true,
        }
    }

    /// Create with custom configuration.
    pub fn with_config(width: f32, height: f32, config: RichTextViewConfig) -> Self {
        Self {
            blocks: Vec::new(),
            wrapped_lines: Vec::new(),
            content_height: 0.0,
            scroll_offset_px: 0.0,
            width,
            height,
            selection: None,
            selection_anchor: None,
            dragging: false,
            search: SearchState::default(),
            config,
            layout_dirty: true,
        }
    }

    /// Set the content blocks (replaces all content).
    pub fn set_blocks(&mut self, blocks: Vec<RichBlock>) {
        self.blocks = blocks;
        self.layout_dirty = true;
        self.selection = None;
        self.refresh_search();
    }

    /// Append a block to the end.
    pub fn push_block(&mut self, block: RichBlock) {
        self.blocks.push(block);
        self.layout_dirty = true;
        self.refresh_search();
    }

    /// Clear all content.
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.wrapped_lines.clear();
        self.content_height = 0.0;
        self.scroll_offset_px = 0.0;
        self.selection = None;
        self.search.matches.clear();
        self.layout_dirty = true;
    }

    /// Number of blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Resize the widget and re-layout.
    pub fn resize(&mut self, width: f32, height: f32) {
        if (self.width - width).abs() > 0.1 {
            self.layout_dirty = true;
        }
        self.width = width;
        self.height = height;
        self.clamp_scroll();
    }

    /// Scroll to top.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset_px = 0.0;
    }

    /// Scroll to bottom.
    pub fn scroll_to_bottom(&mut self) {
        self.ensure_layout();
        let max = (self.content_height - self.height).max(0.0);
        self.scroll_offset_px = max;
    }

    /// Scroll by pixels (positive = down).
    pub fn scroll_by_px(&mut self, delta: f32) {
        self.scroll_offset_px += delta;
        self.clamp_scroll();
    }

    /// Scroll by lines.
    pub fn scroll_by_lines(&mut self, delta: i32) {
        self.scroll_by_px(delta as f32 * self.config.line_height);
    }

    fn clamp_scroll(&mut self) {
        self.ensure_layout();
        let max = (self.content_height - self.height).max(0.0);
        if self.scroll_offset_px < 0.0 {
            self.scroll_offset_px = 0.0;
        }
        if self.scroll_offset_px > max {
            self.scroll_offset_px = max;
        }
    }

    // ----- Layout / word-wrap -----

    /// Ensure the layout is up to date.
    fn ensure_layout(&mut self) {
        if self.layout_dirty {
            self.rebuild_layout();
            self.layout_dirty = false;
        }
    }

    /// Rebuild wrapped lines from blocks.
    fn rebuild_layout(&mut self) {
        self.wrapped_lines.clear();
        let available_width = self.width - self.gutter_width();
        let mut y: f32 = 0.0;

        for (block_idx, block) in self.blocks.iter().enumerate() {
            match block {
                RichBlock::Paragraph {
                    spans,
                    spacing_above,
                    spacing_below,
                } => {
                    y += spacing_above * self.config.line_height;
                    let lines = self.wrap_spans(spans, available_width, 0.0);
                    for (i, line_spans) in lines.into_iter().enumerate() {
                        self.wrapped_lines.push(WrappedLine {
                            block_idx,
                            spans: line_spans,
                            y,
                            line_height: self.config.line_height,
                            indent: 0.0,
                            is_block_start: i == 0,
                        });
                        y += self.config.line_height;
                    }
                    y += spacing_below * self.config.line_height;
                }
                RichBlock::Heading { level, spans } => {
                    y += level.spacing_above() * self.config.line_height;
                    let h_line_height =
                        self.config.line_height * level.size_multiplier();
                    let lines = self.wrap_spans(spans, available_width, 0.0);
                    for (i, line_spans) in lines.into_iter().enumerate() {
                        self.wrapped_lines.push(WrappedLine {
                            block_idx,
                            spans: line_spans,
                            y,
                            line_height: h_line_height,
                            indent: 0.0,
                            is_block_start: i == 0,
                        });
                        y += h_line_height;
                    }
                    y += level.spacing_below() * self.config.line_height;
                }
                RichBlock::ListItem {
                    kind: _,
                    index: _,
                    indent_level,
                    spans,
                } => {
                    let indent = (*indent_level as f32)
                        * (self.config.list_indent_chars as f32)
                        * self.config.char_width;
                    let content_width = (available_width - indent - 2.0 * self.config.char_width)
                        .max(self.config.char_width);
                    let lines = self.wrap_spans(spans, content_width, 0.0);
                    for (i, line_spans) in lines.into_iter().enumerate() {
                        self.wrapped_lines.push(WrappedLine {
                            block_idx,
                            spans: line_spans,
                            y,
                            line_height: self.config.line_height,
                            indent: indent + 2.0 * self.config.char_width,
                            is_block_start: i == 0,
                        });
                        y += self.config.line_height;
                    }
                }
                RichBlock::CodeBlock { code, .. } => {
                    y += self.config.code_block_padding;
                    let indent = self.config.code_block_padding;
                    for line in code.lines() {
                        self.wrapped_lines.push(WrappedLine {
                            block_idx,
                            spans: vec![RichSpan::plain(line)],
                            y,
                            line_height: self.config.line_height,
                            indent,
                            is_block_start: false,
                        });
                        y += self.config.line_height;
                    }
                    // If code is empty, still show one blank line
                    if code.is_empty() {
                        self.wrapped_lines.push(WrappedLine {
                            block_idx,
                            spans: vec![RichSpan::plain("")],
                            y,
                            line_height: self.config.line_height,
                            indent,
                            is_block_start: false,
                        });
                        y += self.config.line_height;
                    }
                    y += self.config.code_block_padding;
                }
                RichBlock::HorizontalRule => {
                    y += self.config.line_height * 0.5;
                    self.wrapped_lines.push(WrappedLine {
                        block_idx,
                        spans: Vec::new(),
                        y,
                        line_height: 2.0, // thin rule
                        indent: 0.0,
                        is_block_start: true,
                    });
                    y += 2.0;
                    y += self.config.line_height * 0.5;
                }
                RichBlock::ImagePlaceholder {
                    width: _,
                    height: img_h,
                    alt_text,
                } => {
                    y += 4.0; // small gap
                    self.wrapped_lines.push(WrappedLine {
                        block_idx,
                        spans: vec![RichSpan::plain(format!("[Image: {}]", alt_text))],
                        y,
                        line_height: *img_h,
                        indent: 0.0,
                        is_block_start: true,
                    });
                    y += *img_h;
                    y += 4.0;
                }
            }
        }

        self.content_height = y;
    }

    /// Word-wrap spans into lines, given available width.
    fn wrap_spans(
        &self,
        spans: &[RichSpan],
        available_width: f32,
        _extra_indent: f32,
    ) -> Vec<Vec<RichSpan>> {
        if spans.is_empty() {
            return vec![Vec::new()];
        }

        let mut result: Vec<Vec<RichSpan>> = Vec::new();
        let mut current_line: Vec<RichSpan> = Vec::new();
        let mut current_width: f32 = 0.0;

        for span in spans {
            let char_w = self.config.char_width; // monospace assumed

            // Split span text by words
            let mut remaining = span.text.as_str();
            while !remaining.is_empty() {
                // Find next word boundary
                let (word, rest) = split_next_word(remaining);
                let word_width = word.len() as f32 * char_w;

                if current_width + word_width > available_width && current_width > 0.0 {
                    // Wrap to next line
                    result.push(core::mem::take(&mut current_line));
                    current_width = 0.0;
                }

                // If a single word is longer than available width, force it on its own line
                if word_width > available_width && current_width == 0.0 && !word.is_empty() {
                    // Break the word at available width
                    let max_chars = (available_width / char_w).floor() as usize;
                    let max_chars = max_chars.max(1);
                    let (chunk, leftover) = if word.len() > max_chars {
                        (&word[..max_chars], &word[max_chars..])
                    } else {
                        (word, "")
                    };
                    current_line.push(RichSpan::styled(chunk, span.style.clone()));
                    result.push(core::mem::take(&mut current_line));
                    current_width = 0.0;
                    // Put leftover back
                    if leftover.is_empty() {
                        remaining = rest;
                    } else {
                        // Reconstruct remaining with leftover + rest
                        // We cannot easily do this without allocation; just push leftover
                        // as a new span in the next iteration. For simplicity, add it now.
                        remaining = rest;
                        if !leftover.is_empty() {
                            current_line
                                .push(RichSpan::styled(leftover, span.style.clone()));
                            current_width += leftover.len() as f32 * char_w;
                        }
                    }
                    continue;
                }

                if !word.is_empty() {
                    current_line.push(RichSpan::styled(word, span.style.clone()));
                    current_width += word_width;
                }

                remaining = rest;
            }
        }

        if !current_line.is_empty() {
            result.push(current_line);
        }
        if result.is_empty() {
            result.push(Vec::new());
        }

        result
    }

    /// Width of the line-number gutter.
    fn gutter_width(&self) -> f32 {
        if self.config.show_line_numbers {
            let total_lines = self.wrapped_lines.len().max(1);
            let digits = (total_lines as f32).log10().floor() as usize + 1;
            let digits = digits.max(3);
            (digits as f32 + 1.0) * self.config.char_width
        } else {
            0.0
        }
    }

    // ----- Selection -----

    /// Convert pixel position to a wrapped-line text position.
    fn hit_test(&mut self, x: f32, y: f32) -> TextPosition {
        self.ensure_layout();
        let abs_y = y + self.scroll_offset_px;
        let gutter_w = self.gutter_width();
        let text_x = (x - gutter_w).max(0.0);

        // Find which wrapped line this y falls on
        let mut line_idx = 0;
        for (i, wl) in self.wrapped_lines.iter().enumerate() {
            if abs_y >= wl.y && abs_y < wl.y + wl.line_height {
                line_idx = i;
                break;
            }
            if i == self.wrapped_lines.len() - 1 {
                line_idx = i;
            }
        }

        let col = if let Some(wl) = self.wrapped_lines.get(line_idx) {
            let effective_x = (text_x - wl.indent).max(0.0);
            (effective_x / self.config.char_width) as usize
        } else {
            0
        };

        TextPosition::new(line_idx, col)
    }

    /// Get plain text of a wrapped line.
    fn wrapped_line_text(&self, idx: usize) -> String {
        self.wrapped_lines
            .get(idx)
            .map(|wl| wl.spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .unwrap_or_default()
    }

    /// Get all content as plain text (for selection/copy).
    pub fn plain_text(&self) -> String {
        let mut result = String::new();
        for block in &self.blocks {
            match block {
                RichBlock::Paragraph { spans, .. } | RichBlock::Heading { spans, .. } => {
                    for span in spans {
                        result.push_str(&span.text);
                    }
                    result.push('\n');
                }
                RichBlock::ListItem { spans, .. } => {
                    for span in spans {
                        result.push_str(&span.text);
                    }
                    result.push('\n');
                }
                RichBlock::CodeBlock { code, .. } => {
                    result.push_str(code);
                    result.push('\n');
                }
                RichBlock::HorizontalRule => {
                    result.push_str("---\n");
                }
                RichBlock::ImagePlaceholder { alt_text, .. } => {
                    result.push_str(&format!("[Image: {}]\n", alt_text));
                }
            }
        }
        result
    }

    /// Get selected text as plain string.
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;
        if sel.is_empty() {
            return None;
        }

        let mut result = String::new();
        for line_idx in sel.start.line..=sel.end.line {
            let text = self.wrapped_line_text(line_idx);
            let start_col = if line_idx == sel.start.line {
                sel.start.col
            } else {
                0
            };
            let end_col = if line_idx == sel.end.line {
                sel.end.col
            } else {
                text.len()
            };
            let start = start_col.min(text.len());
            let end = end_col.min(text.len());
            result.push_str(&text[start..end]);
            if line_idx < sel.end.line {
                result.push('\n');
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Select all text.
    pub fn select_all(&mut self) {
        self.ensure_layout();
        if self.wrapped_lines.is_empty() {
            return;
        }
        let last = self.wrapped_lines.len() - 1;
        let last_col = self.wrapped_line_text(last).len();
        self.selection = Some(Selection::new(
            TextPosition::ZERO,
            TextPosition::new(last, last_col),
        ));
    }

    // ----- Search -----

    /// Find all occurrences of query in the rich text content.
    pub fn find(&mut self, query: &str, case_sensitive: bool) {
        self.search.query = query.to_string();
        self.search.case_sensitive = case_sensitive;
        self.search.current_match = None;
        self.refresh_search();
        if !self.search.matches.is_empty() {
            self.search.current_match = Some(0);
            self.scroll_to_match(0);
        }
    }

    /// Next match.
    pub fn next_match(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        let next = match self.search.current_match {
            Some(idx) => (idx + 1) % self.search.matches.len(),
            None => 0,
        };
        self.search.current_match = Some(next);
        self.scroll_to_match(next);
    }

    /// Previous match.
    pub fn prev_match(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        let prev = match self.search.current_match {
            Some(0) | None => self.search.matches.len() - 1,
            Some(idx) => idx - 1,
        };
        self.search.current_match = Some(prev);
        self.scroll_to_match(prev);
    }

    /// Clear search.
    pub fn clear_search(&mut self) {
        self.search = SearchState::default();
    }

    /// Number of matches.
    pub fn match_count(&self) -> usize {
        self.search.matches.len()
    }

    fn refresh_search(&mut self) {
        self.ensure_layout();
        self.search.matches.clear();
        if self.search.query.is_empty() {
            return;
        }

        let query = if self.search.case_sensitive {
            self.search.query.clone()
        } else {
            self.search.query.to_lowercase()
        };

        for (line_idx, wl) in self.wrapped_lines.iter().enumerate() {
            let text: String = wl.spans.iter().map(|s| s.text.as_str()).collect();
            let haystack = if self.search.case_sensitive {
                text.clone()
            } else {
                text.to_lowercase()
            };
            let mut start = 0;
            while let Some(pos) = haystack[start..].find(&query) {
                let abs_pos = start + pos;
                self.search
                    .matches
                    .push((line_idx, abs_pos, abs_pos + query.len()));
                start = abs_pos + 1;
            }
        }
    }

    fn scroll_to_match(&mut self, match_idx: usize) {
        self.ensure_layout();
        if let Some(&(line_idx, _, _)) = self.search.matches.get(match_idx)
            && let Some(wl) = self.wrapped_lines.get(line_idx) {
                let line_y = wl.y;
                if line_y < self.scroll_offset_px
                    || line_y + wl.line_height > self.scroll_offset_px + self.height
                {
                    // Center the match
                    self.scroll_offset_px = (line_y - self.height / 2.0).max(0.0);
                    self.clamp_scroll();
                }
            }
    }

    // ----- Event handling -----

    /// Handle an event. Returns EventResult and optionally a RichTextEvent.
    pub fn handle_event(&mut self, event: &Event) -> (EventResult, Option<RichTextEvent>) {
        match event {
            Event::Mouse(me) => self.handle_mouse(me),
            Event::Key(ke) => self.handle_key(ke),
            _ => (EventResult::Ignored, None),
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> (EventResult, Option<RichTextEvent>) {
        match &event.kind {
            MouseEventKind::Press(crate::event::MouseButton::Left) => {
                if self.config.selectable {
                    let pos = self.hit_test(event.x, event.y);
                    // Check if clicking on a link
                    if let Some(link_url) = self.link_at(event.x, event.y) {
                        return (
                            EventResult::Consumed,
                            Some(RichTextEvent::LinkClicked(link_url)),
                        );
                    }
                    self.selection_anchor = Some(pos);
                    self.selection = Some(Selection::new(pos, pos));
                    self.dragging = true;
                }
                (EventResult::Consumed, None)
            }
            MouseEventKind::Release(crate::event::MouseButton::Left) => {
                self.dragging = false;
                (EventResult::Consumed, None)
            }
            MouseEventKind::Move if self.dragging => {
                let pos = self.hit_test(event.x, event.y);
                if let Some(anchor) = self.selection_anchor {
                    self.selection = Some(Selection::new(anchor, pos));
                }
                (EventResult::Consumed, None)
            }
            MouseEventKind::DoubleClick(crate::event::MouseButton::Left) => {
                if self.config.selectable {
                    let pos = self.hit_test(event.x, event.y);
                    self.selection = Some(self.word_at_wrapped(pos));
                    self.dragging = false;
                }
                (EventResult::Consumed, None)
            }
            MouseEventKind::Scroll { dy, .. } => {
                // Scroll 3 lines per "notch"
                self.scroll_by_px(-dy * 3.0);
                (EventResult::Consumed, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) -> (EventResult, Option<RichTextEvent>) {
        if !event.pressed {
            return (EventResult::Ignored, None);
        }

        if event.modifiers.ctrl && event.key == Key::A {
            self.select_all();
            return (EventResult::Consumed, None);
        }

        if event.modifiers.ctrl && event.key == Key::C {
            if let Some(text) = self.selected_text() {
                return (EventResult::Consumed, Some(RichTextEvent::Copy(text)));
            }
            return (EventResult::Consumed, None);
        }

        match event.key {
            Key::PageUp => {
                self.scroll_by_px(-self.height);
                (EventResult::Consumed, None)
            }
            Key::PageDown => {
                self.scroll_by_px(self.height);
                (EventResult::Consumed, None)
            }
            Key::Home if event.modifiers.ctrl => {
                self.scroll_to_top();
                (EventResult::Consumed, None)
            }
            Key::End if event.modifiers.ctrl => {
                self.scroll_to_bottom();
                (EventResult::Consumed, None)
            }
            Key::Up => {
                self.scroll_by_lines(-1);
                (EventResult::Consumed, None)
            }
            Key::Down => {
                self.scroll_by_lines(1);
                (EventResult::Consumed, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    /// Find a link URL at the given pixel position, if any.
    fn link_at(&mut self, x: f32, y: f32) -> Option<String> {
        self.ensure_layout();
        let abs_y = y + self.scroll_offset_px;
        let gutter_w = self.gutter_width();
        let text_x = (x - gutter_w).max(0.0);

        for wl in &self.wrapped_lines {
            if abs_y >= wl.y && abs_y < wl.y + wl.line_height {
                let effective_x = (text_x - wl.indent).max(0.0);
                let char_idx = (effective_x / self.config.char_width) as usize;
                let mut col = 0;
                for span in &wl.spans {
                    let span_end = col + span.text.len();
                    if char_idx >= col && char_idx < span_end
                        && let Some(ref url) = span.style.link {
                            return Some(url.clone());
                        }
                    col = span_end;
                }
                break;
            }
        }
        None
    }

    /// Word boundaries for double-click in wrapped lines.
    fn word_at_wrapped(&self, pos: TextPosition) -> Selection {
        let text = self.wrapped_line_text(pos.line);
        let bytes = text.as_bytes();
        let col = pos.col.min(text.len());

        let mut start = col;
        while start > 0 && is_word_char(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = col;
        while end < bytes.len() && is_word_char(bytes[end]) {
            end += 1;
        }

        Selection::new(
            TextPosition::new(pos.line, start),
            TextPosition::new(pos.line, end),
        )
    }

    // ----- Rendering -----

    /// Render the widget to a RenderTree.
    pub fn render(&mut self, tree: &mut RenderTree) {
        self.ensure_layout();

        // Background
        tree.fill_rect(0.0, 0.0, self.width, self.height, BG_COLOR);
        tree.clip(0.0, 0.0, self.width, self.height);

        let gutter_w = self.gutter_width();

        // Gutter background
        if self.config.show_line_numbers && gutter_w > 0.0 {
            tree.fill_rect(0.0, 0.0, gutter_w, self.height, SURFACE_COLOR);
        }

        // Only render visible lines
        let scroll_top = self.scroll_offset_px;
        let scroll_bottom = scroll_top + self.height;

        for (vis_idx, wl) in self.wrapped_lines.iter().enumerate() {
            let line_bottom = wl.y + wl.line_height;
            if line_bottom < scroll_top {
                continue;
            }
            if wl.y > scroll_bottom {
                break;
            }

            let render_y = wl.y - scroll_top;

            // Check if this is an HR
            if let Some(RichBlock::HorizontalRule) = self.blocks.get(wl.block_idx) {
                tree.push(RenderCommand::Line {
                    x1: gutter_w + 8.0,
                    y1: render_y + 1.0,
                    x2: self.width - 8.0,
                    y2: render_y + 1.0,
                    color: HR_COLOR,
                    width: 1.0,
                });
                continue;
            }

            // Code block background
            if let Some(RichBlock::CodeBlock { .. }) = self.blocks.get(wl.block_idx) {
                tree.fill_rect(
                    gutter_w,
                    render_y,
                    self.width - gutter_w,
                    wl.line_height,
                    CODE_BG_COLOR,
                );
            }

            // Image placeholder
            if let Some(RichBlock::ImagePlaceholder { width: iw, height: ih, .. }) =
                self.blocks.get(wl.block_idx)
            {
                tree.push(RenderCommand::StrokeRect {
                    x: gutter_w + 4.0,
                    y: render_y,
                    width: *iw,
                    height: *ih,
                    color: SUBTEXT_COLOR,
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            // Line number
            if self.config.show_line_numbers {
                let num_str = format!("{}", vis_idx + 1);
                let num_x = gutter_w - (num_str.len() as f32 + 0.5) * self.config.char_width;
                tree.push(RenderCommand::Text {
                    x: num_x,
                    y: render_y,
                    text: num_str,
                    color: SUBTEXT_COLOR,
                    font_size: self.config.font_size,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // List marker
            if wl.is_block_start
                && let Some(RichBlock::ListItem {
                    kind,
                    index,
                    indent_level,
                    ..
                }) = self.blocks.get(wl.block_idx)
                {
                    let indent_px = (*indent_level as f32)
                        * (self.config.list_indent_chars as f32)
                        * self.config.char_width;
                    let marker = match kind {
                        ListKind::Bullet => "\u{2022}".to_string(), // bullet
                        ListKind::Numbered => format!("{}.", index),
                    };
                    tree.push(RenderCommand::Text {
                        x: gutter_w + indent_px,
                        y: render_y,
                        text: marker,
                        color: LIST_MARKER_COLOR,
                        font_size: self.config.font_size,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }

            // Selection highlight
            if let Some(ref sel) = self.selection
                && !sel.is_empty() && vis_idx >= sel.start.line && vis_idx <= sel.end.line {
                    let line_len: usize = wl.spans.iter().map(|s| s.text.len()).sum();
                    let sel_start = if vis_idx == sel.start.line {
                        sel.start.col
                    } else {
                        0
                    };
                    let sel_end = if vis_idx == sel.end.line {
                        sel.end.col
                    } else {
                        line_len
                    };
                    if sel_start < sel_end {
                        let x1 = gutter_w + wl.indent + sel_start as f32 * self.config.char_width;
                        let x2 = gutter_w + wl.indent + sel_end as f32 * self.config.char_width;
                        tree.fill_rect(x1, render_y, x2 - x1, wl.line_height, SELECTION_COLOR);
                    }
                }

            // Search match highlights
            for (match_idx, &(ml, ms, me)) in self.search.matches.iter().enumerate() {
                if ml == vis_idx {
                    let color = if self.search.current_match == Some(match_idx) {
                        CURRENT_MATCH_COLOR
                    } else {
                        SEARCH_MATCH_COLOR
                    };
                    let x1 = gutter_w + wl.indent + ms as f32 * self.config.char_width;
                    let x2 = gutter_w + wl.indent + me as f32 * self.config.char_width;
                    tree.fill_rect(x1, render_y, x2 - x1, wl.line_height, color);
                }
            }

            // Render text spans
            let mut x = gutter_w + wl.indent;
            let is_heading = matches!(self.blocks.get(wl.block_idx), Some(RichBlock::Heading { .. }));
            let heading_level = if let Some(RichBlock::Heading { level, .. }) =
                self.blocks.get(wl.block_idx)
            {
                Some(*level)
            } else {
                None
            };

            for span in &wl.spans {
                let font_size = if let Some(level) = heading_level {
                    self.config.font_size * level.size_multiplier()
                } else {
                    span.style.font_size.to_points(self.config.font_size)
                };

                let fg = if is_heading {
                    HEADING_COLOR
                } else {
                    span.style.fg_color.unwrap_or(TEXT_COLOR)
                };

                let weight = match span.style.weight {
                    RichFontWeight::Bold => FontWeightHint::Bold,
                    RichFontWeight::Normal => {
                        if is_heading {
                            FontWeightHint::Bold
                        } else {
                            FontWeightHint::Regular
                        }
                    }
                };

                // Span background
                if let Some(bg) = span.style.bg_color {
                    let span_width = span.text.len() as f32 * self.config.char_width;
                    tree.fill_rect(x, render_y, span_width, wl.line_height, bg);
                }

                tree.push(RenderCommand::Text {
                    x,
                    y: render_y,
                    text: span.text.clone(),
                    color: fg,
                    font_size,
                    font_weight: weight,
                    max_width: None,
                });

                let span_width = span.text.len() as f32 * self.config.char_width;

                // Underline
                if span.style.underline || span.style.link.is_some() {
                    let uy = render_y + wl.line_height - 2.0;
                    tree.push(RenderCommand::Line {
                        x1: x,
                        y1: uy,
                        x2: x + span_width,
                        y2: uy,
                        color: fg,
                        width: 1.0,
                    });
                }

                // Strikethrough
                if span.style.strikethrough {
                    let sy = render_y + wl.line_height / 2.0;
                    tree.push(RenderCommand::Line {
                        x1: x,
                        y1: sy,
                        x2: x + span_width,
                        y2: sy,
                        color: fg,
                        width: 1.0,
                    });
                }

                x += span_width;
            }
        }

        tree.unclip();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split text at the next word boundary, returning (word_including_trailing_space, rest).
fn split_next_word(s: &str) -> (&str, &str) {
    if s.is_empty() {
        return ("", "");
    }

    let bytes = s.as_bytes();
    let mut i = 0;

    // Skip leading whitespace (include it in the "word" for width calculation)
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }

    // Find end of word (non-space characters)
    while i < bytes.len() && bytes[i] != b' ' {
        i += 1;
    }

    (&s[..i], &s[i..])
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- ANSI parsing tests ---

    #[test]
    fn test_parse_plain_text() {
        let lines = parse_ansi("hello world");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 1);
        assert_eq!(lines[0][0].text, "hello world");
        assert_eq!(lines[0][0].style, AnsiStyle::default());
    }

    #[test]
    fn test_parse_multiline() {
        let lines = parse_ansi("line1\nline2\nline3");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0][0].text, "line1");
        assert_eq!(lines[1][0].text, "line2");
        assert_eq!(lines[2][0].text, "line3");
    }

    #[test]
    fn test_parse_ansi_fg_color() {
        let lines = parse_ansi("\x1b[31mred text\x1b[0m");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 1);
        assert_eq!(lines[0][0].text, "red text");
        assert_eq!(lines[0][0].style.fg, Some(ANSI_COLORS[1])); // red
    }

    #[test]
    fn test_parse_ansi_bold_italic() {
        let lines = parse_ansi("\x1b[1;3mbold italic\x1b[0m");
        assert!(lines[0][0].style.bold);
        assert!(lines[0][0].style.italic);
    }

    #[test]
    fn test_parse_ansi_256_color() {
        let lines = parse_ansi("\x1b[38;5;196mred256\x1b[0m");
        assert_eq!(lines[0][0].text, "red256");
        assert!(lines[0][0].style.fg.is_some());
    }

    #[test]
    fn test_parse_ansi_truecolor() {
        let lines = parse_ansi("\x1b[38;2;100;150;200mtruecolor\x1b[0m");
        assert_eq!(lines[0][0].text, "truecolor");
        assert_eq!(lines[0][0].style.fg, Some(Color::rgb(100, 150, 200)));
    }

    #[test]
    fn test_parse_ansi_bg_color() {
        let lines = parse_ansi("\x1b[44mblue bg\x1b[0m");
        assert_eq!(lines[0][0].style.bg, Some(ANSI_COLORS[4])); // blue
    }

    #[test]
    fn test_parse_ansi_bright_colors() {
        let lines = parse_ansi("\x1b[91mbright red\x1b[0m");
        assert_eq!(lines[0][0].style.fg, Some(ANSI_COLORS[9])); // bright red
    }

    #[test]
    fn test_parse_ansi_reset_mid_line() {
        let lines = parse_ansi("\x1b[31mred\x1b[0m normal");
        assert_eq!(lines[0].len(), 2);
        assert_eq!(lines[0][0].text, "red");
        assert_eq!(lines[0][0].style.fg, Some(ANSI_COLORS[1]));
        assert_eq!(lines[0][1].text, " normal");
        assert_eq!(lines[0][1].style.fg, None);
    }

    #[test]
    fn test_parse_ansi_underline_reverse() {
        let lines = parse_ansi("\x1b[4;7munderline reversed\x1b[0m");
        assert!(lines[0][0].style.underline);
        assert!(lines[0][0].style.reverse);
    }

    // --- SimpleTextView scrolling tests ---

    #[test]
    fn test_simple_scroll_basics() {
        let mut view = SimpleTextView::new(400.0, 160.0); // 10 visible lines
        let text = (0..50).map(|i| format!("Line {}", i)).collect::<Vec<_>>().join("\n");
        view.set_text(&text);

        assert_eq!(view.line_count(), 50);
        assert_eq!(view.visible_lines(), 10);
        assert_eq!(view.scroll_offset, 0);

        view.scroll_to_bottom();
        assert_eq!(view.scroll_offset, 40);
        assert!(view.is_at_bottom());

        view.scroll_to_top();
        assert_eq!(view.scroll_offset, 0);
    }

    #[test]
    fn test_simple_scroll_by() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        let text = (0..50).map(|i| format!("Line {}", i)).collect::<Vec<_>>().join("\n");
        view.set_text(&text);

        view.scroll_by(5);
        assert_eq!(view.scroll_offset, 5);

        view.scroll_by(-3);
        assert_eq!(view.scroll_offset, 2);

        // Cannot scroll past 0
        view.scroll_by(-100);
        assert_eq!(view.scroll_offset, 0);

        // Cannot scroll past max
        view.scroll_by(1000);
        assert_eq!(view.scroll_offset, 40);
    }

    #[test]
    fn test_simple_auto_scroll_on_append() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        // Fill to capacity
        for i in 0..20 {
            view.append_line(&format!("Line {}", i));
        }
        // Should auto-scroll since we were at bottom
        assert!(view.is_at_bottom());

        // Scroll up manually
        view.scroll_to_top();
        assert!(!view.is_at_bottom());

        // Append should NOT auto-scroll since user scrolled away
        view.append_line("new line");
        assert_eq!(view.scroll_offset, 0);
    }

    #[test]
    fn test_simple_max_lines() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.config.max_lines = 20;

        for i in 0..50 {
            view.append_line(&format!("Line {}", i));
        }

        assert!(view.line_count() <= 20);
    }

    // --- Selection tests ---

    #[test]
    fn test_selection_ordering() {
        let sel = Selection::new(
            TextPosition::new(5, 10),
            TextPosition::new(2, 3),
        );
        assert_eq!(sel.start, TextPosition::new(2, 3));
        assert_eq!(sel.end, TextPosition::new(5, 10));
    }

    #[test]
    fn test_selection_contains() {
        let sel = Selection::new(
            TextPosition::new(1, 5),
            TextPosition::new(3, 10),
        );
        assert!(sel.contains(TextPosition::new(2, 0)));
        assert!(sel.contains(TextPosition::new(1, 5)));
        assert!(!sel.contains(TextPosition::new(3, 10))); // end is exclusive
        assert!(!sel.contains(TextPosition::new(0, 0)));
    }

    #[test]
    fn test_simple_select_all_and_copy() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.set_text("hello\nworld");

        view.select_all();
        let text = view.selected_text().unwrap();
        assert_eq!(text, "hello\nworld");
    }

    #[test]
    fn test_simple_selected_text_partial() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.set_text("abcdef\nghijkl\nmnopqr");

        view.selection = Some(Selection::new(
            TextPosition::new(0, 3),
            TextPosition::new(1, 4),
        ));
        let text = view.selected_text().unwrap();
        assert_eq!(text, "def\nghij");
    }

    // --- Search tests ---

    #[test]
    fn test_simple_search_basic() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.set_text("hello world\nhello rust\ngoodbye world");

        view.find("hello", true);
        assert_eq!(view.match_count(), 2);
        assert_eq!(view.search.current_match, Some(0));
    }

    #[test]
    fn test_simple_search_case_insensitive() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.set_text("Hello World\nhello world\nHELLO WORLD");

        view.find("hello", false);
        assert_eq!(view.match_count(), 3);
    }

    #[test]
    fn test_simple_search_navigation() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.set_text("aaa\naaa\naaa");

        view.find("aaa", true);
        assert_eq!(view.match_count(), 3);
        assert_eq!(view.search.current_match, Some(0));

        view.next_match();
        assert_eq!(view.search.current_match, Some(1));

        view.next_match();
        assert_eq!(view.search.current_match, Some(2));

        // Wrap around
        view.next_match();
        assert_eq!(view.search.current_match, Some(0));

        // Reverse
        view.prev_match();
        assert_eq!(view.search.current_match, Some(2));
    }

    #[test]
    fn test_simple_search_no_results() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.set_text("hello world");

        view.find("xyz", true);
        assert_eq!(view.match_count(), 0);
        assert_eq!(view.search.current_match, None);
    }

    // --- Word-wrap tests ---

    #[test]
    fn test_word_split() {
        assert_eq!(split_next_word("hello world"), ("hello", " world"));
        assert_eq!(split_next_word(" hello"), (" hello", ""));
        assert_eq!(split_next_word(""), ("", ""));
        assert_eq!(split_next_word("word"), ("word", ""));
    }

    #[test]
    fn test_rich_word_wrap() {
        let mut view = RichTextView::new(80.0, 200.0); // 10 chars wide at 8px
        view.set_blocks(vec![RichBlock::Paragraph {
            spans: vec![RichSpan::plain("hello world foo bar")],
            spacing_above: 0.0,
            spacing_below: 0.0,
        }]);
        view.ensure_layout();
        // "hello world foo bar" should wrap into multiple lines at 10-char width
        assert!(view.wrapped_lines.len() >= 2);
    }

    #[test]
    fn test_rich_search() {
        let mut view = RichTextView::new(400.0, 200.0);
        view.set_blocks(vec![
            RichBlock::Paragraph {
                spans: vec![RichSpan::plain("hello world")],
                spacing_above: 0.0,
                spacing_below: 0.0,
            },
            RichBlock::Paragraph {
                spans: vec![RichSpan::plain("hello rust")],
                spacing_above: 0.0,
                spacing_below: 0.0,
            },
        ]);

        view.find("hello", true);
        assert_eq!(view.match_count(), 2);
    }

    #[test]
    fn test_rich_select_all() {
        let mut view = RichTextView::new(400.0, 200.0);
        view.set_blocks(vec![RichBlock::Paragraph {
            spans: vec![RichSpan::plain("test content")],
            spacing_above: 0.0,
            spacing_below: 0.0,
        }]);

        view.select_all();
        let text = view.selected_text().unwrap();
        assert_eq!(text, "test content");
    }

    #[test]
    fn test_rich_plain_text_extraction() {
        let view_blocks = vec![
            RichBlock::Heading {
                level: HeadingLevel::H1,
                spans: vec![RichSpan::plain("Title")],
            },
            RichBlock::Paragraph {
                spans: vec![RichSpan::plain("Some text.")],
                spacing_above: 0.0,
                spacing_below: 0.0,
            },
            RichBlock::HorizontalRule,
            RichBlock::CodeBlock {
                code: "let x = 1;".to_string(),
                language: Some("rust".to_string()),
            },
        ];

        let mut view = RichTextView::new(400.0, 200.0);
        view.set_blocks(view_blocks);
        let plain = view.plain_text();
        assert!(plain.contains("Title"));
        assert!(plain.contains("Some text."));
        assert!(plain.contains("let x = 1;"));
        assert!(plain.contains("---"));
    }

    // --- Rendering tests ---

    #[test]
    fn test_simple_render_produces_commands() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.set_text("hello\nworld");

        let mut tree = RenderTree::new();
        view.render(&mut tree);

        // Should have at least: background fill + clip + text commands + unclip
        assert!(tree.len() >= 4);
    }

    #[test]
    fn test_rich_render_produces_commands() {
        let mut view = RichTextView::new(400.0, 200.0);
        view.set_blocks(vec![RichBlock::Paragraph {
            spans: vec![RichSpan::plain("hello")],
            spacing_above: 0.0,
            spacing_below: 0.0,
        }]);

        let mut tree = RenderTree::new();
        view.render(&mut tree);
        assert!(tree.len() >= 3);
    }

    #[test]
    fn test_simple_render_with_line_numbers() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.config.show_line_numbers = true;
        view.set_text("line 1\nline 2\nline 3");

        let mut tree = RenderTree::new();
        view.render(&mut tree);

        // Should have gutter background + line number texts
        let text_cmds: Vec<_> = tree
            .commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::Text { .. }))
            .collect();
        // At least 3 line numbers + 3 lines of text
        assert!(text_cmds.len() >= 6);
    }

    #[test]
    fn test_simple_render_with_selection() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.set_text("hello world");
        view.selection = Some(Selection::new(
            TextPosition::new(0, 2),
            TextPosition::new(0, 7),
        ));

        let mut tree = RenderTree::new();
        view.render(&mut tree);

        // Should have a selection highlight rect
        let fill_rects: Vec<_> = tree
            .commands
            .iter()
            .filter(|c| {
                if let RenderCommand::FillRect { color, .. } = c {
                    *color == SELECTION_COLOR
                } else {
                    false
                }
            })
            .collect();
        assert_eq!(fill_rects.len(), 1);
    }

    // --- 256-color lookup test ---

    #[test]
    fn test_color_from_256() {
        // First 16 should match ANSI_COLORS
        for i in 0..16u8 {
            assert_eq!(color_from_256(i), ANSI_COLORS[i as usize]);
        }
        // Grayscale
        let gray232 = color_from_256(232);
        assert_eq!(gray232, Color::rgb(8, 8, 8));
        let gray255 = color_from_256(255);
        assert_eq!(gray255, Color::rgb(238, 238, 238));
    }

    // --- Hit test ---

    #[test]
    fn test_hit_test_simple() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        view.set_text("hello world");

        // Click at (24.0, 0.0) should be col 3 (24/8)
        let pos = view.hit_test(24.0, 0.0);
        assert_eq!(pos, TextPosition::new(0, 3));
    }

    #[test]
    fn test_hit_test_with_scroll() {
        let mut view = SimpleTextView::new(400.0, 160.0);
        let text = (0..50).map(|i| format!("Line {}", i)).collect::<Vec<_>>().join("\n");
        view.set_text(&text);
        view.scroll_offset = 10;

        // Click at y=0 should be line 10 (scroll_offset + 0)
        let pos = view.hit_test(0.0, 0.0);
        assert_eq!(pos.line, 10);
    }
}

#![allow(dead_code)]
//! Text view widgets for displaying text content (read-only or selectable).
//!
//! Two widget types:
//! - [`SimpleTextView`]: Plain or ANSI-colored text display (logs, terminal output).
//! - [`RichTextView`]: Formatted rich text with headings, lists, links, and styling.
//!
//! Both support vertical scrolling, text selection, copy-to-clipboard, and search.

use crate::color::Color;
use crate::cycle;
use crate::event::{Event, EventResult, Key, KeyEvent, MouseEvent, MouseEventKind};
use crate::render::{FontFamily, FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use crate::style::CornerRadii;
use textfind::Case;

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

/// Default font size in points.
const DEFAULT_FONT_SIZE: f32 = 14.0;

/// Width of one cell of the character grid, at `font_size`.
///
/// [`SimpleTextView`] shows log and terminal output, where a character grid is
/// the right model — columns are supposed to line up. But the *size* of a cell
/// is a property of the font, not a constant: this used to be a hardcoded 8.0,
/// which happened to match the built-in face at 14 px and nothing else, so a
/// caller who raised `font_size` got a grid that drifted further out of
/// alignment with every column.
///
/// It was then `text::digit_advance` — right about the size being the font's
/// business, wrong about *which* font. A digit's advance in the proportional
/// UI face is a cell that only digits fit: at 14 px it is 8.1 px while `'W'`
/// is 14.1 px, so a log line of ordinary prose overran its own selection band
/// and every column after the first drifted. A grid needs a face where every
/// glyph advances the same distance, which is what [`text::cell_advance`]
/// asks for.
///
/// [`text::cell_advance`]: crate::text::cell_advance
fn default_char_width(font_size: f32) -> f32 {
    crate::text::cell_advance(font_size, FontWeightHint::Regular)
}

/// Baseline-to-baseline distance at `font_size`, from the font's own metrics.
///
/// Measured in the mono face, since that is the face the grid is drawn in and
/// its line spacing is not in general the UI face's.
fn default_line_height(font_size: f32) -> f32 {
    crate::text::line_height_in(font_size, FontWeightHint::Regular, FontFamily::Mono)
}

/// The horizontal unit [`RichTextView`] indents by, at `font_size`.
///
/// Deliberately *not* [`default_char_width`]. A rich view is not a grid: its
/// spans are measured individually with `text::measure` and drawn in the
/// proportional UI face, and `char_width` survives only as the width of a
/// gutter digit and as the quantum a list indents by. Both of those are UI-face
/// quantities, so asking for a mono cell here would indent prose by a face it
/// never draws in.
fn default_indent_unit(font_size: f32) -> f32 {
    crate::text::digit_advance(font_size, FontWeightHint::Regular)
}

/// Baseline-to-baseline distance for [`RichTextView`], in the UI face it draws.
fn default_rich_line_height(font_size: f32) -> f32 {
    crate::text::line_height(font_size, FontWeightHint::Regular)
}

// There used to be a `columns()` here — `text.chars().count() as f32` — and
// everything [`SimpleTextView`] placed against the text was that times
// `config.char_width`. It replaced a `str::len()`, which counted UTF-8 bytes
// and pushed every span after a non-ASCII one to the right; counting characters
// fixed that and left a subtler version of the same error in place, because a
// character count is equal to the drawn width only where every character
// advances the cell width. A tab does not — one `char`, four cells — and this
// view expands no tabs, so a tab-indented line put every mark three cells left
// of its text per level of indentation.
//
// Positions against real text are now measured; see
// `SimpleTextView::column_offsets`. `config.char_width` survives for the
// line-number gutter, which is sized to hold digits and holds nothing else.

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
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

/// One of the sixteen named ANSI colours.
///
/// Every caller derives its index by subtracting an SGR base (30, 40, 90, 100)
/// from a parameter it has already range-checked — but the check and the table's
/// length are two separate facts, and only `get` ties them together. An index
/// outside the table means the range check was wrong, and rendering the default
/// foreground is a better answer to that than a panic in a terminal view.
fn ansi_color(index: usize) -> Color {
    ANSI_COLORS.get(index).copied().unwrap_or(TEXT_COLOR)
}

/// Convert a 256-color index to an RGB Color.
fn color_from_256(index: u8) -> Color {
    match index {
        0..=15 => ansi_color(usize::from(index)),
        // 216-color cube (indices 16-231): 6x6x6, each axis stepping by 51.
        16..=231 => {
            let idx = index.saturating_sub(16);
            let level = |n: u8| n.saturating_mul(51);
            Color::rgb(level(idx / 36), level((idx / 6) % 6), level(idx % 6))
        }
        // Grayscale ramp (indices 232-255): 24 shades from 8 to 238.
        232..=255 => {
            let gray = index
                .saturating_sub(232)
                .saturating_mul(10)
                .saturating_add(8);
            Color::rgb(gray, gray, gray)
        }
    }
}

/// A cursor over the input as *characters*, not bytes.
///
/// `parse_ansi` takes a `&str` — already known-valid UTF-8 — and the previous
/// version nonetheless decoded it by hand: it guessed a sequence's length from
/// the lead byte, clamped that guess to the buffer, and silently dropped the
/// character if the guess produced something `from_utf8` rejected. Every step
/// of that is redundant on a `&str`, and each was an index the compiler could
/// not check. Holding the *unread remainder* instead of an offset means the
/// cursor is a `&str` at all times: it cannot leave a character boundary, and
/// there is no arithmetic to get wrong.
struct StrCursor<'a> {
    rest: &'a str,
}

impl<'a> StrCursor<'a> {
    fn new(rest: &'a str) -> Self {
        Self { rest }
    }

    fn at_end(&self) -> bool {
        self.rest.is_empty()
    }

    /// Take the next character and advance past it.
    fn next_char(&mut self) -> Option<char> {
        let mut chars = self.rest.chars();
        let c = chars.next()?;
        self.rest = chars.as_str();
        Some(c)
    }

    /// Consume `prefix` if it is what comes next, reporting whether it was.
    fn eat(&mut self, prefix: &str) -> bool {
        match self.rest.strip_prefix(prefix) {
            Some(tail) => {
                self.rest = tail;
                true
            }
            None => false,
        }
    }
}

/// Move the pending text into `line` as one span, if there is any.
fn flush_span(text: &mut String, line: &mut Vec<StyledSpan>, style: AnsiStyle) {
    if !text.is_empty() {
        line.push(StyledSpan::styled(core::mem::take(text), style));
    }
}

/// Consume a CSI sequence's parameters and final byte, applying it if it is SGR.
///
/// The cursor arrives positioned just past `ESC [`. Every path through the loop
/// consumes a character — `next_char` is what drives it — so the sequence
/// always ends, which is why the caller's loop terminates. A sequence that runs
/// off the end of the input is simply dropped, matching a terminal's behaviour
/// on a truncated escape.
fn parse_csi(cursor: &mut StrCursor<'_>, style: &mut AnsiStyle) {
    let mut params: Vec<u16> = Vec::new();
    let mut num: Option<u16> = None;

    while let Some(c) = cursor.next_char() {
        match c {
            '0'..='9' => {
                // The arm has already established this is a decimal digit, so
                // both conversions succeed; `unwrap_or` states that without
                // needing a panic to do it.
                let digit = c
                    .to_digit(10)
                    .and_then(|d| u16::try_from(d).ok())
                    .unwrap_or(0);
                num = Some(num.unwrap_or(0).saturating_mul(10).saturating_add(digit));
            }
            ';' => params.push(num.take().unwrap_or(0)),
            // Final byte of an SGR sequence.
            'm' => {
                params.push(num.unwrap_or(0));
                apply_sgr_params(&params, style);
                return;
            }
            // Any other CSI final byte: a sequence we don't handle, consumed
            // and discarded. This arm must stay below `'m'`, which it contains.
            '@'..='~' => return,
            // Anything else is malformed; drop it and resume as plain text.
            _ => return,
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

    let mut cursor = StrCursor::new(input);
    while !cursor.at_end() {
        if cursor.eat("\x1b[") {
            flush_span(&mut current_text, &mut current_line, current_style);
            parse_csi(&mut cursor, &mut current_style);
        } else if cursor.eat("\n") {
            flush_span(&mut current_text, &mut current_line, current_style);
            lines.push(core::mem::take(&mut current_line));
        } else if cursor.eat("\r") {
            // Dropped: this view has no cursor to return to column zero, so a
            // CR-LF pair must produce one line break rather than two.
        } else if let Some(c) = cursor.next_char() {
            // Includes a lone ESC not followed by '[', which is shown as text.
            current_text.push(c);
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

/// A cursor over the unread tail of an SGR parameter list.
///
/// The extended-colour codes (`38`/`48`) read a variable number of parameters
/// that follow them, and the previous version did it by guarding `params[i + 2]`
/// with an `i + 2 < params.len()` written in the *pattern guard* of the arm
/// above — a bound and a read in different expressions, twice for each of four
/// forms. Holding the remainder rather than an offset means a read that has no
/// bytes behind it simply yields `None`.
struct ParamCursor<'a> {
    rest: &'a [u16],
}

impl<'a> ParamCursor<'a> {
    fn new(rest: &'a [u16]) -> Self {
        Self { rest }
    }

    /// Take the next parameter and advance past it.
    fn next_param(&mut self) -> Option<u16> {
        let (first, tail) = self.rest.split_first()?;
        self.rest = tail;
        Some(*first)
    }

    /// Take the next `N` parameters, or none at all if there are fewer.
    ///
    /// All-or-nothing on purpose: a truncated extended-colour sequence must
    /// leave its remaining parameters to be read as ordinary SGR codes, which
    /// is what a terminal does, and a partial consume would eat one of them.
    fn take<const N: usize>(&mut self) -> Option<[u16; N]> {
        let (head, tail) = self.rest.split_at_checked(N)?;
        let taken = <[u16; N]>::try_from(head).ok()?;
        self.rest = tail;
        Some(taken)
    }
}

/// Narrow an SGR parameter to a colour channel.
///
/// Out-of-range values are clamped rather than truncated: `as u8` turned a
/// malformed `38;2;300;0;0` into channel 44, a colour with no relationship to
/// what was asked for. Saturating at least keeps "very red" red.
fn sgr_channel(value: u16) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

/// Consume the tail of a `38`/`48` extended-colour code and report its colour.
///
/// The `38`/`48` introducer has already been consumed. `5;N` names a 256-colour
/// index and `2;R;G;B` a direct triple; anything else — including a truncated
/// form — selects no colour and leaves what follows to be read as ordinary SGR.
fn extended_color(params: &mut ParamCursor<'_>) -> Option<Color> {
    match params.next_param()? {
        5 => {
            let [index] = params.take::<1>()?;
            Some(color_from_256(sgr_channel(index)))
        }
        2 => {
            let [r, g, b] = params.take::<3>()?;
            Some(Color::rgb(sgr_channel(r), sgr_channel(g), sgr_channel(b)))
        }
        _ => None,
    }
}

/// Apply SGR (Select Graphic Rendition) parameters to a style.
fn apply_sgr_params(params: &[u16], style: &mut AnsiStyle) {
    let mut params = ParamCursor::new(params);
    while let Some(code) = params.next_param() {
        match code {
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
            30..=37 => style.fg = Some(ansi_color(usize::from(code.saturating_sub(30)))),
            // Default foreground
            39 => style.fg = None,
            // Background colors 40-47
            40..=47 => style.bg = Some(ansi_color(usize::from(code.saturating_sub(40)))),
            // Default background
            49 => style.bg = None,
            // Bright foreground 90-97 — the second half of the 16-colour table
            90..=97 => {
                style.fg = Some(ansi_color(
                    usize::from(code.saturating_sub(90)).saturating_add(8),
                ));
            }
            // Bright background 100-107
            100..=107 => {
                style.bg = Some(ansi_color(
                    usize::from(code.saturating_sub(100)).saturating_add(8),
                ));
            }
            // Extended color: 38;5;N or 38;2;R;G;B
            38 => {
                if let Some(color) = extended_color(&mut params) {
                    style.fg = Some(color);
                }
            }
            // Extended background: 48;5;N or 48;2;R;G;B
            48 => {
                if let Some(color) = extended_color(&mut params) {
                    style.bg = Some(color);
                }
            }
            _ => {} // Unknown SGR parameter — ignore
        }
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
            char_width: default_char_width(DEFAULT_FONT_SIZE),
            line_height: default_line_height(DEFAULT_FONT_SIZE),
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

impl SearchState {
    /// The match after the current one, wrapping at the end.
    ///
    /// Returns `None` when there is nothing to move to, which is the emptiness
    /// check both views used to write as a separate early return above their
    /// own copy of this arithmetic.
    fn next_index(&self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        Some(match self.current_match {
            Some(idx) => cycle::after(self.matches.len(), idx),
            // No current match: start at the first.
            None => 0,
        })
    }

    /// The match before the current one, wrapping at the start.
    fn prev_index(&self) -> Option<usize> {
        let last = self.matches.len().checked_sub(1)?;
        Some(match self.current_match {
            Some(idx) => cycle::before(self.matches.len(), idx),
            None => last,
        })
    }
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
            let digits = line_number_digits(self.lines.len());
            (digits as f32 + 1.0) * self.config.char_width
        } else {
            0.0
        }
    }

    /// The largest scroll offset that still fills the view.
    ///
    /// Three call sites used to derive this independently — `is_at_bottom`,
    /// `clamp_scroll` and `scroll_to_bottom` — each pairing a
    /// `lines.len() - visible` with its own `lines.len() <= visible` guard
    /// written several lines away from it. Saturating subtraction *is* that
    /// guard, so the two can no longer disagree.
    fn max_scroll_offset(&self) -> usize {
        self.lines.len().saturating_sub(self.visible_lines())
    }

    /// Whether the view is scrolled to the bottom.
    pub fn is_at_bottom(&self) -> bool {
        // A view shorter than the screen has a maximum offset of zero, so it is
        // always at the bottom — which is what the old `len() <= visible` early
        // return said in three more lines.
        self.scroll_offset >= self.max_scroll_offset()
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
            let excess = self.lines.len().saturating_sub(self.config.max_lines);
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
                        sel.end.line = sel.end.line.saturating_sub(excess);
                    }
                } else {
                    sel.start.line = sel.start.line.saturating_sub(excess);
                    sel.end.line = sel.end.line.saturating_sub(excess);
                }
            }
        }
    }

    /// Clamp scroll offset to valid range.
    fn clamp_scroll(&mut self) {
        self.scroll_offset = self.scroll_offset.min(self.max_scroll_offset());
    }

    /// Scroll to the very bottom.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.max_scroll_offset();
    }

    /// Scroll to the very top.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Scroll by a number of lines (positive = down, negative = up).
    pub fn scroll_by(&mut self, delta: i32) {
        // `unsigned_abs` rather than `-delta`: negating `i32::MIN` overflows,
        // and this is a public entry point, so the argument is the caller's.
        let magnitude = usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX);
        self.scroll_offset = if delta < 0 {
            self.scroll_offset.saturating_sub(magnitude)
        } else {
            self.scroll_offset.saturating_add(magnitude)
        };
        self.clamp_scroll();
    }

    /// Weight a span is drawn in, so that measuring it asks the same question
    /// the renderer will answer.
    fn span_weight(span: &StyledSpan) -> FontWeightHint {
        if span.style.bold {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        }
    }

    /// Width of `text` as this view will draw it.
    fn measure(&self, text: &str, weight: FontWeightHint) -> f32 {
        crate::text::measure_in(text, self.config.font_size, weight, FontFamily::Mono)
    }

    /// x of every column boundary on `line`, relative to the text area's left
    /// edge: `offsets[c]` is where column `c` starts, and the last entry is
    /// where the line ends. Always at least one element.
    ///
    /// # Why this is measured rather than `col as f32 * char_width`
    ///
    /// Multiplying by a cell width assumes every character advances that width.
    /// That is what "monospace" means for Latin text and is not true of the
    /// text a log or a terminal view actually receives. **A tab is the case
    /// that matters**: it is one `char`, and the face draws it four cells wide.
    /// This view does no tab expansion of its own, so before this method
    /// existed the first tab on a line put the selection band, the search
    /// highlight and the caret three cells left of the text they belong to, and
    /// each further tab moved them another three. Wide (CJK) glyphs and
    /// zero-width combining marks break the same assumption in the other two
    /// directions.
    ///
    /// Each span is measured in the weight it is drawn in, because a bold span
    /// on a face whose bold is not metric-compatible advances differently — and
    /// a view that highlights matches in bold would then mis-place everything
    /// after the first match, which is exactly the text the user is looking at.
    ///
    /// # Cost
    ///
    /// One measurement per character, not per prefix. Measuring prefixes would
    /// be the obviously-safe thing — it is what the renderer literally does with
    /// the run — but it is quadratic in the line length, and this view holds log
    /// and terminal output where a 500-character line is unremarkable: 125 000
    /// shapings per line per frame against 500. The crate keeps a
    /// `text::shaping_cost` instrument precisely because this cost is not
    /// theoretical.
    ///
    /// Accumulating per-character advances is only equal to the prefix widths if
    /// measurement is additive, i.e. if the face does no kerning across a pair.
    /// It is, on this font stack, and `simple_view_advances_are_additive` is the
    /// test that says so — so that if a kerning face ever arrives, this decision
    /// fails loudly here rather than showing up as a caret that drifts a
    /// fraction of a pixel per character on one face and not another.
    fn column_offsets(&self, line: usize) -> Vec<f32> {
        let mut offsets = vec![0.0_f32];
        let Some(spans) = self.lines.get(line) else {
            return offsets;
        };
        let mut x = 0.0_f32;
        let mut buf = [0_u8; 4];
        for span in spans {
            let weight = Self::span_weight(span);
            for ch in span.text.chars() {
                x += self.measure(ch.encode_utf8(&mut buf), weight);
                offsets.push(x);
            }
        }
        offsets
    }

    /// x of column `col` on `line`, relative to the text area's left edge.
    ///
    /// A column past the end of the line clamps to the end, which is what a
    /// selection running through a short line means.
    fn col_x(&self, line: usize, col: usize) -> f32 {
        let offsets = self.column_offsets(line);
        offsets
            .get(col)
            .or_else(|| offsets.last())
            .copied()
            .unwrap_or(0.0)
    }

    /// Convert pixel coordinates to a text position.
    fn hit_test(&self, x: f32, y: f32) -> TextPosition {
        let gutter = self.gutter_width();
        let text_x = (x - gutter).max(0.0);
        let line_in_view = (y / self.config.line_height) as usize;
        let line = self
            .scroll_offset
            .saturating_add(line_in_view)
            .min(self.lines.len().saturating_sub(1));

        // The inverse of `column_offsets`, and deliberately written as one:
        // dividing by a cell width would disagree with the placement of every
        // mark this view draws the moment the line holds a tab, so a click
        // would select from a different column than the caret was shown at.
        //
        // The boundary chosen is the *nearest* one, not the one to the left:
        // clicking the right half of a character puts the caret after it, which
        // is what every text field does and what makes click-and-drag over a
        // whole word possible without overshooting.
        let offsets = self.column_offsets(line);
        let mut col = 0;
        let mut best = f32::INFINITY;
        for (c, &ox) in offsets.iter().enumerate() {
            let d = (ox - text_x).abs();
            if d < best {
                best = d;
                col = c;
            }
        }
        TextPosition::new(line, col)
    }

    /// Get character count of a line.
    ///
    /// Characters, not bytes. This was `s.text.len()`, which is UTF-8 bytes, so
    /// on any line holding a non-ASCII character the clamp that keeps a
    /// selection inside the line let it run past the end — while everything
    /// that *drew* the selection counted characters. The two halves of one
    /// model disagreeing is the failure this whole module keeps rediscovering.
    fn line_char_count(&self, line: usize) -> usize {
        self.lines
            .get(line)
            .map(|spans| spans.iter().map(|s| s.text.chars().count()).sum())
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
        // `checked_sub` rather than an `is_empty` guard three lines above the
        // `- 1`: one expression both rejects the empty case and produces the
        // index, so neither can be changed without the other.
        let Some(last_line) = self.lines.len().checked_sub(1) else {
            return;
        };
        let last_col = self.line_char_count(last_line);
        self.selection = Some(Selection::new(
            TextPosition::ZERO,
            TextPosition::new(last_line, last_col),
        ));
    }

    /// Find word boundaries around a position (for double-click).
    fn word_at(&self, pos: TextPosition) -> Selection {
        let text = self.line_text(pos.line);
        let (start, end) = word_range_at(&text, pos.col);
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
        let Some(next) = self.search.next_index() else {
            return;
        };
        self.search.current_match = Some(next);
        self.scroll_to_match(next);
    }

    /// Navigate to the previous match.
    pub fn prev_match(&mut self) {
        let Some(prev) = self.search.prev_index() else {
            return;
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

        // The offsets recorded here are used to highlight and to replace, so
        // they must be offsets into the line itself. Searching a
        // `to_lowercase()` copy — which this did — gives offsets into a
        // different string, because lowercasing is not length-preserving;
        // `textfind` folds while walking the real line instead. See that
        // crate's documentation for the three bugs the old shape carried.
        let case = Case::sensitive(self.search.case_sensitive);
        for (line_idx, _) in self.lines.iter().enumerate() {
            let text = self.line_text(line_idx);
            self.search.matches.extend(
                textfind::matches(&text, &self.search.query, case)
                    .map(|(start, end)| (line_idx, start, end)),
            );
        }
    }

    /// Scroll to make a match visible.
    fn scroll_to_match(&mut self, match_idx: usize) {
        if let Some(&(line, _, _)) = self.search.matches.get(match_idx) {
            let visible = self.visible_lines();
            if line < self.scroll_offset || line >= self.scroll_offset.saturating_add(visible) {
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
                self.scroll_by(lines.saturating_neg());
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
                self.scroll_by(lines.saturating_neg());
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
                self.scroll_by(page.saturating_neg());
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

        // Everything below is placed on a character grid whose cell came from
        // `default_char_width`, i.e. from the mono face. Draw it in that face
        // too, or the layout is computed against one set of advances and
        // filled with another.
        tree.push(RenderCommand::PushFont {
            family: FontFamily::Mono,
        });

        let gutter_w = self.gutter_width();
        let visible = self.visible_lines();

        // Draw gutter background
        if self.config.show_line_numbers && gutter_w > 0.0 {
            tree.fill_rect(0.0, 0.0, gutter_w, self.height, SURFACE_COLOR);
        }

        for view_line in 0..visible {
            let line_idx = self.scroll_offset.saturating_add(view_line);
            if line_idx >= self.lines.len() {
                break;
            }

            let y = view_line as f32 * self.config.line_height;

            // Once per line, then shared by the selection band and every search
            // highlight on it. Recomputing it per mark would measure the line
            // again for each, which on a screen where a search has matched
            // several times per line is the difference between one pass over the
            // visible text and a dozen.
            let mut offsets: Option<Vec<f32>> = None;
            let mut col_x = |view: &Self, col: usize| -> f32 {
                let o = offsets.get_or_insert_with(|| view.column_offsets(line_idx));
                o.get(col).or_else(|| o.last()).copied().unwrap_or(0.0)
            };

            // Line number
            if self.config.show_line_numbers {
                let num_str = format!("{}", line_idx.saturating_add(1));
                let num_x = gutter_w - (num_str.len() as f32 + 0.5) * self.config.char_width;
                tree.push(RenderCommand::Text {
                    x: num_x,
                    y,
                    text: num_str,
                    color: SUBTEXT_COLOR,
                    font_size: self.config.font_size,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Selection highlight for this line
            if let Some(ref sel) = self.selection
                && !sel.is_empty()
                && line_idx >= sel.start.line
                && line_idx <= sel.end.line
            {
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
                    let x1 = gutter_w + col_x(self, sel_start);
                    let x2 = gutter_w + col_x(self, sel_end);
                    tree.fill_rect(x1, y, x2 - x1, self.config.line_height, SELECTION_COLOR);
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
                    let x1 = gutter_w + col_x(self, ms);
                    let x2 = gutter_w + col_x(self, me);
                    tree.fill_rect(x1, y, x2 - x1, self.config.line_height, color);
                }
            }

            // Render spans
            let mut x = gutter_w;
            if let Some(spans) = self.lines.get(line_idx) {
                for span in spans {
                    let fg = resolve_span_fg(&span.style);
                    let weight = Self::span_weight(span);
                    // What this span will be drawn as. A nominal column count
                    // times a cell width is equal to it only where every
                    // character advances one cell — see `column_offsets`.
                    let span_width = self.measure(&span.text, weight);

                    // Background color
                    if let Some(bg) = resolve_span_bg(&span.style) {
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
                        overflow: TextOverflow::Clip,
                    });

                    // Underline
                    if span.style.underline {
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

                    x += span_width;
                }
            }
        }

        tree.push(RenderCommand::PopFont);
        tree.unclip();
    }
}

/// Resolve the effective foreground color for a span.
fn resolve_span_fg(style: &AnsiStyle) -> Color {
    let base = style.fg.unwrap_or(TEXT_COLOR);
    if style.reverse {
        style.bg.unwrap_or(BG_COLOR)
    } else if style.dim {
        // Dim: blend halfway toward the background. `u8::midpoint` does the
        // widen-add-halve in one step, so there is no intermediate that has to
        // be a `u16` to avoid overflowing.
        Color::rgba(
            u8::midpoint(base.r, BG_COLOR.r),
            u8::midpoint(base.g, BG_COLOR.g),
            u8::midpoint(base.b, BG_COLOR.b),
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

/// The narrowest gutter either view will draw, in digits.
const MIN_GUTTER_DIGITS: usize = 3;

/// How many digits the largest line number needs.
///
/// Both views computed this, and both relied on `log10(0)` being negative
/// infinity and the cast saturating it to zero — true, but true by accident
/// rather than by anything either function said. Stating the empty case makes
/// the reliance explicit and puts it in one place.
fn line_number_digits(line_count: usize) -> usize {
    if line_count == 0 {
        return MIN_GUTTER_DIGITS;
    }
    let digits = (line_count as f32).log10().floor() as usize;
    digits.saturating_add(1).max(MIN_GUTTER_DIGITS)
}

/// Check whether a byte is a "word" character (for double-click selection).
fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The byte range of the word surrounding `col` in `text`.
///
/// Both views wrote this scan out longhand — `while start > 0 && is_word_char(
/// bytes[start - 1])` and its mirror — so the identical four lines appeared
/// twice, each with a bound tested in one expression and used in the next.
/// `iter().rposition` and `iter().position` do the same walk with the bound
/// inside the iterator, and there is now one copy of it.
///
/// `col` is clamped into the string, so a stale caret from a shorter line
/// cannot name a byte that is no longer there.
fn word_range_at(text: &str, col: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let col = col.min(bytes.len());
    let (before, after) = bytes.split_at(col.min(bytes.len()));

    // The first non-word byte scanning backwards ends the run; nothing before
    // it belongs to the word, and no such byte means the run reaches the start.
    let start = before
        .iter()
        .rposition(|b| !is_word_char(*b))
        .map_or(0, |last_gap| last_gap.saturating_add(1));
    let end = after
        .iter()
        .position(|b| !is_word_char(*b))
        .map_or(bytes.len(), |gap| col.saturating_add(gap));

    (start, end)
}

// ===========================================================================
// RichTextView
// ===========================================================================

/// Font weight for rich text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RichFontWeight {
    #[default]
    Normal,
    Bold,
}

/// Font style for rich text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RichFontStyle {
    #[default]
    Normal,
    Italic,
}

/// Font size specification.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
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
    /// The horizontal unit the view indents by, in pixels.
    ///
    /// Named for what it once was — a monospace cell, back when this view laid
    /// prose out on a grid. It no longer does: spans are measured with
    /// `text::measure` and drawn proportionally, and this survives as the width
    /// of a gutter digit and the quantum a list indents by.
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
            char_width: default_indent_unit(DEFAULT_FONT_SIZE),
            line_height: default_rich_line_height(DEFAULT_FONT_SIZE),
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
                    let lines = self.wrap_spans(spans, available_width, None);
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
                    let h_line_height = self.config.line_height * level.size_multiplier();
                    let lines = self.wrap_spans(spans, available_width, Some(*level));
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
                    let lines = self.wrap_spans(spans, content_width, None);
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

    /// The size and weight a span is drawn in.
    ///
    /// Both wrapping and rendering need this, and they need the *same* answer:
    /// a heading is drawn at up to twice the base size and in bold, and an
    /// inline `Small`/`Large` span at its own size, so measuring everything as
    /// one nominal cell — as a fixed `char_width` did — under-measured a top
    /// heading by more than half and wrapped it far past the right margin.
    fn span_font(&self, span: &RichSpan, heading: Option<HeadingLevel>) -> (f32, FontWeightHint) {
        let size = match heading {
            Some(level) => self.config.font_size * level.size_multiplier(),
            None => span.style.font_size.to_points(self.config.font_size),
        };
        let weight = match span.style.weight {
            RichFontWeight::Bold => FontWeightHint::Bold,
            // A heading is bold even where its own spans do not say so.
            RichFontWeight::Normal if heading.is_some() => FontWeightHint::Bold,
            RichFontWeight::Normal => FontWeightHint::Regular,
        };
        (size, weight)
    }

    /// Width of `span` in pixels, in the font it will be drawn in.
    fn span_width(&self, span: &RichSpan, heading: Option<HeadingLevel>) -> f32 {
        let (size, weight) = self.span_font(span, heading);
        crate::text::measure(&span.text, size, weight)
    }

    /// The heading level a wrapped line belongs to, if any.
    fn heading_of(&self, wl: &WrappedLine) -> Option<HeadingLevel> {
        match self.blocks.get(wl.block_idx) {
            Some(RichBlock::Heading { level, .. }) => Some(*level),
            _ => None,
        }
    }

    /// Byte offset of character `col` of `s`, clamped to the end of the string.
    ///
    /// Clamping rather than panicking is deliberate: `col` comes from a
    /// selection that a re-layout may have left pointing past the end of a line
    /// that got shorter, and a stale selection should paint the wrong highlight
    /// for one frame, not take the window down.
    fn byte_of_char(s: &str, col: usize) -> usize {
        s.char_indices().nth(col).map_or(s.len(), |(i, _)| i)
    }

    /// The boxes to paint to highlight characters `from..to` of `wl`, as
    /// `(left, width)` pairs in pixels from the start of the line.
    ///
    /// A *list*, because a highlight is not a rectangle. Two things break the
    /// single-rectangle form this replaced, which took the x of each end and
    /// filled between them:
    ///
    /// - **Spans.** Each span carries its own size and weight, so a line that
    ///   mixes bold and normal text has no one column width. That much the old
    ///   two-edge code already handled, by walking the spans to find each edge.
    /// - **Direction.** Characters that are contiguous in the *string* need not
    ///   be contiguous on the *screen*. Select from the middle of a Latin word
    ///   into the middle of a Hebrew one and the two halves are drawn apart,
    ///   with unselected characters between them — which the two-edge form
    ///   highlights as well, telling the user they selected text they did not.
    ///
    /// Each span is shaped and drawn on its own, at its own pen position, so
    /// each span's boxes come from that span alone and are shifted by where the
    /// span starts. Boxes that meet are merged: abutting rectangles of the same
    /// colour already look like one, so the merge saves draw commands rather
    /// than fixing an appearance, and the float tolerance it needs cannot cause
    /// a visible error — the worst case is the two commands it started with.
    fn selection_boxes_of_cols(&self, wl: &WrappedLine, from: usize, to: usize) -> Vec<(f32, f32)> {
        let mut boxes: Vec<(f32, f32)> = Vec::new();
        if from >= to {
            return boxes;
        }
        let heading = self.heading_of(wl);
        let mut x = 0.0;
        let mut seen = 0usize;
        for span in &wl.spans {
            let len = span.text.chars().count();
            let next = seen.saturating_add(len);
            if next > from && seen < to {
                // Character columns are the widget's currency; the shaper's is
                // bytes. Converting here rather than at the call site keeps the
                // conversion next to the string it indexes into.
                let lo = Self::byte_of_char(&span.text, from.saturating_sub(seen));
                let hi = Self::byte_of_char(&span.text, to.saturating_sub(seen));
                let (size, weight) = self.span_font(span, heading);
                for (bx, bw) in crate::text::selection_boxes(&span.text, lo, hi, size, weight) {
                    match boxes.last_mut() {
                        Some(prev) if (prev.0 + prev.1 - (x + bx)).abs() < 0.01 => prev.1 += bw,
                        _ => boxes.push((x + bx, bw)),
                    }
                }
            }
            x += self.span_width(span, heading);
            seen = next;
        }
        boxes
    }

    /// The character index in `wl` nearest to `x` pixels from its start.
    fn col_at_x(&self, wl: &WrappedLine, x: f32) -> usize {
        let heading = self.heading_of(wl);
        let mut left = 0.0;
        let mut seen = 0usize;
        for span in &wl.spans {
            let w = self.span_width(span, heading);
            if x <= left + w {
                let (size, weight) = self.span_font(span, heading);
                return seen.saturating_add(crate::text::char_index_at(
                    &span.text,
                    x - left,
                    size,
                    weight,
                ));
            }
            left += w;
            seen = seen.saturating_add(span.text.chars().count());
        }
        seen
    }

    /// Word-wrap spans into lines, given available width.
    fn wrap_spans(
        &self,
        spans: &[RichSpan],
        available_width: f32,
        heading: Option<HeadingLevel>,
    ) -> Vec<Vec<RichSpan>> {
        if spans.is_empty() {
            return vec![Vec::new()];
        }

        let mut result: Vec<Vec<RichSpan>> = Vec::new();
        let mut current_line: Vec<RichSpan> = Vec::new();
        let mut current_width: f32 = 0.0;

        for span in spans {
            let (size, weight) = self.span_font(span, heading);
            let measure = |s: &str| crate::text::measure(s, size, weight);

            // Split span text by words
            let mut remaining = span.text.as_str();
            while !remaining.is_empty() {
                // Find next word boundary
                let (word, rest) = split_next_word(remaining);
                let word_width = measure(word);

                if current_width + word_width > available_width && current_width > 0.0 {
                    // Wrap to next line
                    result.push(core::mem::take(&mut current_line));
                    current_width = 0.0;
                }

                // If a single word is longer than available width, force it on its own line
                if word_width > available_width && current_width == 0.0 && !word.is_empty() {
                    // Broken at the last character that fits. `fit` returns a
                    // byte index on a character boundary; the old code divided
                    // the width by a nominal cell to get a *character* count and
                    // then sliced the string by it as if it were a byte offset,
                    // which panicked outright on any multi-byte word.
                    let cut = crate::text::fit(word, available_width, size, weight).max(
                        // Never zero: a single glyph wider than the whole
                        // column still has to be emitted, or wrapping loops
                        // forever making no progress.
                        word.chars().next().map_or(0, char::len_utf8),
                    );
                    let (chunk, leftover) = word.split_at(cut.min(word.len()));
                    current_line.push(RichSpan::styled(chunk, span.style.clone()));
                    result.push(core::mem::take(&mut current_line));
                    current_width = 0.0;
                    remaining = rest;
                    if !leftover.is_empty() {
                        current_line.push(RichSpan::styled(leftover, span.style.clone()));
                        current_width += measure(leftover);
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
            let digits = line_number_digits(self.wrapped_lines.len());
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

        // The first wrapped line whose band contains this y, falling back to the
        // last line when none does. The old loop spelled that fallback out with
        // an `i == len - 1` test *inside* the loop — a length re-derived on
        // every iteration to say what the search running out already says.
        let line_idx = self
            .wrapped_lines
            .iter()
            .position(|wl| abs_y >= wl.y && abs_y < wl.y + wl.line_height)
            .unwrap_or_else(|| self.wrapped_lines.len().saturating_sub(1));

        let col = if let Some(wl) = self.wrapped_lines.get(line_idx) {
            self.col_at_x(wl, (text_x - wl.indent).max(0.0))
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
        let Some(last) = self.wrapped_lines.len().checked_sub(1) else {
            return;
        };
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
        let Some(next) = self.search.next_index() else {
            return;
        };
        self.search.current_match = Some(next);
        self.scroll_to_match(next);
    }

    /// Previous match.
    pub fn prev_match(&mut self) {
        let Some(prev) = self.search.prev_index() else {
            return;
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

        // Same correction as `TextView::refresh_search` above: the ranges are
        // consumed as offsets into the wrapped line's own text.
        let case = Case::sensitive(self.search.case_sensitive);
        let query = self.search.query.clone();
        let found: Vec<(usize, usize, usize)> = self
            .wrapped_lines
            .iter()
            .enumerate()
            .flat_map(|(line_idx, wl)| {
                let text: String = wl.spans.iter().map(|s| s.text.as_str()).collect();
                textfind::matches(&text, &query, case)
                    .map(|(start, end)| (line_idx, start, end))
                    .collect::<Vec<_>>()
            })
            .collect();
        self.search.matches = found;
    }

    fn scroll_to_match(&mut self, match_idx: usize) {
        self.ensure_layout();
        if let Some(&(line_idx, _, _)) = self.search.matches.get(match_idx)
            && let Some(wl) = self.wrapped_lines.get(line_idx)
        {
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
                let char_idx = self.col_at_x(wl, (text_x - wl.indent).max(0.0));
                let mut col = 0usize;
                for span in &wl.spans {
                    // Characters, not bytes: `col_at_x` counts characters, so
                    // measuring the span's extent in bytes made every link
                    // after a non-ASCII one unclickable.
                    let span_end = col.saturating_add(span.text.chars().count());
                    if char_idx >= col
                        && char_idx < span_end
                        && let Some(ref url) = span.style.link
                    {
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
        let (start, end) = word_range_at(&text, pos.col);
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
            if let Some(RichBlock::ImagePlaceholder {
                width: iw,
                height: ih,
                ..
            }) = self.blocks.get(wl.block_idx)
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
                let num_str = format!("{}", vis_idx.saturating_add(1));
                let num_x = gutter_w - (num_str.len() as f32 + 0.5) * self.config.char_width;
                tree.push(RenderCommand::Text {
                    x: num_x,
                    y: render_y,
                    text: num_str,
                    color: SUBTEXT_COLOR,
                    font_size: self.config.font_size,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
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
                    overflow: TextOverflow::Clip,
                });
            }

            // Selection highlight
            if let Some(ref sel) = self.selection
                && !sel.is_empty()
                && vis_idx >= sel.start.line
                && vis_idx <= sel.end.line
            {
                // Characters, not bytes: `Selection`'s columns count characters,
                // so a byte length here overshot the end of any line holding a
                // character outside ASCII.
                let line_len: usize = wl.spans.iter().map(|s| s.text.chars().count()).sum();
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
                for (bx, bw) in self.selection_boxes_of_cols(wl, sel_start, sel_end) {
                    tree.fill_rect(
                        gutter_w + wl.indent + bx,
                        render_y,
                        bw,
                        wl.line_height,
                        SELECTION_COLOR,
                    );
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
                    for (bx, bw) in self.selection_boxes_of_cols(wl, ms, me) {
                        tree.fill_rect(
                            gutter_w + wl.indent + bx,
                            render_y,
                            bw,
                            wl.line_height,
                            color,
                        );
                    }
                }
            }

            // Render text spans
            let mut x = gutter_w + wl.indent;
            let is_heading = matches!(
                self.blocks.get(wl.block_idx),
                Some(RichBlock::Heading { .. })
            );
            let heading_level =
                if let Some(RichBlock::Heading { level, .. }) = self.blocks.get(wl.block_idx) {
                    Some(*level)
                } else {
                    None
                };

            for span in &wl.spans {
                // The same call the wrapper used, so a span is drawn in exactly
                // the font its line was broken for.
                let (font_size, weight) = self.span_font(span, heading_level);

                let fg = if is_heading {
                    HEADING_COLOR
                } else {
                    span.style.fg_color.unwrap_or(TEXT_COLOR)
                };

                // Measured once and reused for the background, the underline,
                // the strikethrough and the pen advance: all four decorate the
                // same run of glyphs, so all four must be the same width.
                let span_width = self.span_width(span, heading_level);

                // Span background
                if let Some(bg) = span.style.bg_color {
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
                    overflow: TextOverflow::Clip,
                });

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
///
/// The leading spaces belong to the word rather than to what follows, because
/// the caller is measuring how wide the next chunk will draw and the spaces
/// take width wherever they land.
///
/// Written as two `find`s rather than two hand-advanced index loops: each bound
/// now lives inside the search that uses it, and `split_at` produces both halves
/// from the single offset instead of two independent slices of the same string.
fn split_next_word(s: &str) -> (&str, &str) {
    // Past the run of leading spaces…
    let after_spaces = s.find(|c| c != ' ').unwrap_or(s.len());
    // …and then past the run of non-spaces that follows it. The second search
    // is relative to the tail, so its result has to be rebased onto `s`.
    let word_end = s
        .get(after_spaces..)
        .and_then(|tail| tail.find(' '))
        .map_or(s.len(), |gap| after_spaces.saturating_add(gap));

    s.split_at(word_end)
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    /// Width of one grid cell in the tests below, in pixels.
    const TEST_CELL_W: f32 = 8.0;
    /// Height of one grid row in the tests below, in pixels.
    const TEST_CELL_H: f32 = 16.0;

    /// A [`SimpleTextView`] whose grid is pinned to a known cell size.
    ///
    /// The default config takes its cell from whichever face the process has
    /// installed — correct for a widget, wrong for a test. These tests are
    /// about scrolling and selection *arithmetic*, so they have to own their
    /// geometry: a view 160 px tall must mean ten lines whatever font is
    /// present. They used to construct with `new`, which only agreed with
    /// those numbers while the built-in 16 px bitmap face was the only one
    /// available, and started failing the moment a real proportional face
    /// was installed.
    fn simple_view(width: f32, height: f32) -> SimpleTextView {
        SimpleTextView::with_config(
            width,
            height,
            SimpleTextViewConfig {
                char_width: TEST_CELL_W,
                line_height: TEST_CELL_H,
                ..SimpleTextViewConfig::default()
            },
        )
    }

    /// A [`RichTextView`] with the same pinned cell, for the wrap tests that
    /// state a width in characters.
    fn rich_view(width: f32, height: f32) -> RichTextView {
        RichTextView::with_config(
            width,
            height,
            RichTextViewConfig {
                char_width: TEST_CELL_W,
                line_height: TEST_CELL_H,
                ..RichTextViewConfig::default()
            },
        )
    }

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
    fn multi_byte_characters_survive_the_parser_intact() {
        // The parser used to decode UTF-8 by hand from `input.as_bytes()`,
        // guessing each sequence's length from its lead byte. The input is a
        // `&str`, so every character in it is already valid — anything that
        // comes out different is the decoder's fault, not the text's.
        for text in [
            "héllo wörld",
            "日本語のテキスト",
            "emoji: 🦀🚀 and combining: é",
            "mixed ascii ünd nön-ascii",
        ] {
            let lines = parse_ansi(text);
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0][0].text, text, "round-tripping {text:?}");
        }
        // ...and still does when styling splits the run mid-way.
        let lines = parse_ansi("日本\x1b[31m語\x1b[0m");
        assert_eq!(lines[0][0].text, "日本");
        assert_eq!(lines[0][1].text, "語");
    }

    #[test]
    fn a_truncated_or_bogus_escape_does_not_eat_the_rest_of_the_line() {
        // An escape that runs off the end of the input is dropped.
        let lines = parse_ansi("before\x1b[38;5");
        assert_eq!(lines[0][0].text, "before");
        assert_eq!(lines[0].len(), 1, "a truncated CSI produces no span");

        // A lone ESC that is not followed by '[' is ordinary text.
        let lines = parse_ansi("a\x1bb");
        assert_eq!(lines[0][0].text, "a\u{1b}b");

        // `38;5` with no index leaves the colour alone, and what follows is
        // read as ordinary SGR — here, bold.
        let lines = parse_ansi("\x1b[38;5;1mx");
        assert!(lines[0][0].style.fg.is_some());
        let lines = parse_ansi("\x1b[38;2;1;2mx");
        assert_eq!(lines[0][0].style.fg, None, "a short truecolor sets nothing");

        // A CSI we don't handle is consumed and discarded, not printed.
        let lines = parse_ansi("a\x1b[2Jb");
        assert_eq!(lines[0][0].text, "a");
        assert_eq!(lines[0][1].text, "b");
    }

    #[test]
    fn an_out_of_range_truecolor_channel_clamps_rather_than_wrapping() {
        // `as u8` turned 300 into 44 — a colour with no relationship to what
        // was asked for. A malformed "very red" should still be red.
        let lines = parse_ansi("\x1b[38;2;300;0;0mx");
        assert_eq!(lines[0][0].style.fg, Some(Color::rgb(255, 0, 0)));
    }

    #[test]
    fn crlf_is_one_line_break_not_two() {
        let lines = parse_ansi("a\r\nb\r\nc");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0][0].text, "a");
        assert_eq!(lines[2][0].text, "c");
    }

    #[test]
    fn the_256_colour_cube_covers_its_whole_range() {
        // Every index must produce a colour without panicking, including the
        // boundaries between the named block, the cube and the grey ramp.
        for index in 0..=u8::MAX {
            let _ = color_from_256(index);
        }
        assert_eq!(color_from_256(0), ANSI_COLORS[0]);
        assert_eq!(color_from_256(15), ANSI_COLORS[15]);
        // First cube entry is black, last is white.
        assert_eq!(color_from_256(16), Color::rgb(0, 0, 0));
        assert_eq!(color_from_256(231), Color::rgb(255, 255, 255));
        // The grey ramp runs 8..=238 in steps of 10.
        assert_eq!(color_from_256(232), Color::rgb(8, 8, 8));
        assert_eq!(color_from_256(255), Color::rgb(238, 238, 238));
    }

    #[test]
    fn a_word_range_stops_at_the_characters_that_are_not_words() {
        assert_eq!(word_range_at("hello world", 2), (0, 5));
        assert_eq!(word_range_at("hello world", 8), (6, 11));
        // A caret just past a word still selects that word: the backward scan
        // reaches it even though the forward one stops immediately.
        assert_eq!(word_range_at("hello world", 5), (0, 5));
        // Between two spaces, with no word on either side, the range is empty.
        assert_eq!(word_range_at("a  b", 2), (2, 2));
        assert_eq!(word_range_at("snake_case99 x", 3), (0, 12));
        assert_eq!(word_range_at("", 0), (0, 0));
        // A column past the end is clamped rather than panicking — a caret can
        // outlive the line it was placed on.
        assert_eq!(word_range_at("ab", 99), (0, 2));
    }

    #[test]
    fn splitting_words_keeps_the_leading_spaces_with_the_word() {
        assert_eq!(split_next_word("hello world"), ("hello", " world"));
        assert_eq!(split_next_word(" world"), (" world", ""));
        assert_eq!(split_next_word("  two  words"), ("  two", "  words"));
        assert_eq!(split_next_word(""), ("", ""));
        assert_eq!(split_next_word("   "), ("   ", ""));
        // Repeatedly splitting must consume the whole string and terminate.
        let mut rest = "  a bb   ccc d";
        let mut seen = String::new();
        while !rest.is_empty() {
            let (word, tail) = split_next_word(rest);
            assert!(!word.is_empty(), "a split that consumes nothing would loop");
            seen.push_str(word);
            rest = tail;
        }
        assert_eq!(seen, "  a bb   ccc d");
    }

    #[test]
    fn search_navigation_wraps_at_both_ends() {
        let mut view = simple_view(200.0, 160.0);
        view.set_text("aa\nbb\naa\ncc\naa");
        view.find("aa", true);
        assert_eq!(view.match_count(), 3);
        assert_eq!(view.search.current_match, Some(0));

        view.next_match();
        assert_eq!(view.search.current_match, Some(1));
        view.next_match();
        assert_eq!(view.search.current_match, Some(2));
        view.next_match();
        assert_eq!(view.search.current_match, Some(0), "wraps past the end");
        view.prev_match();
        assert_eq!(view.search.current_match, Some(2), "wraps past the start");

        // With no matches at all, navigation is a no-op rather than a panic.
        view.find("zzz", true);
        assert_eq!(view.match_count(), 0);
        view.next_match();
        view.prev_match();
        assert_eq!(view.search.current_match, None);
    }

    #[test]
    fn an_extreme_scroll_delta_clamps_instead_of_overflowing() {
        // `scroll_by` is public, so the delta is the caller's; negating
        // `i32::MIN` would overflow before the saturating subtraction ran.
        let mut view = simple_view(200.0, 160.0);
        view.set_text(&"line\n".repeat(50));
        view.scroll_by(i32::MIN);
        assert_eq!(view.scroll_offset, 0);
        view.scroll_by(i32::MAX);
        assert_eq!(view.scroll_offset, view.max_scroll_offset());
        assert!(view.is_at_bottom());
    }

    #[test]
    fn the_gutter_is_never_narrower_than_three_digits() {
        assert_eq!(line_number_digits(0), 3);
        assert_eq!(line_number_digits(1), 3);
        assert_eq!(line_number_digits(999), 3);
        assert_eq!(line_number_digits(1000), 4);
        assert_eq!(line_number_digits(9999), 4);
        assert_eq!(line_number_digits(10_000), 5);
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
        let mut view = simple_view(400.0, 160.0); // 10 visible lines
        let text = (0..50)
            .map(|i| format!("Line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
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
        let mut view = simple_view(400.0, 160.0);
        let text = (0..50)
            .map(|i| format!("Line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
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
        let mut view = simple_view(400.0, 160.0);
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
        let mut view = simple_view(400.0, 160.0);
        view.config.max_lines = 20;

        for i in 0..50 {
            view.append_line(&format!("Line {}", i));
        }

        assert!(view.line_count() <= 20);
    }

    // --- Selection tests ---

    #[test]
    fn test_selection_ordering() {
        let sel = Selection::new(TextPosition::new(5, 10), TextPosition::new(2, 3));
        assert_eq!(sel.start, TextPosition::new(2, 3));
        assert_eq!(sel.end, TextPosition::new(5, 10));
    }

    #[test]
    fn test_selection_contains() {
        let sel = Selection::new(TextPosition::new(1, 5), TextPosition::new(3, 10));
        assert!(sel.contains(TextPosition::new(2, 0)));
        assert!(sel.contains(TextPosition::new(1, 5)));
        assert!(!sel.contains(TextPosition::new(3, 10))); // end is exclusive
        assert!(!sel.contains(TextPosition::new(0, 0)));
    }

    #[test]
    fn test_simple_select_all_and_copy() {
        let mut view = simple_view(400.0, 160.0);
        view.set_text("hello\nworld");

        view.select_all();
        let text = view.selected_text().unwrap();
        assert_eq!(text, "hello\nworld");
    }

    #[test]
    fn test_simple_selected_text_partial() {
        let mut view = simple_view(400.0, 160.0);
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
        let mut view = simple_view(400.0, 160.0);
        view.set_text("hello world\nhello rust\ngoodbye world");

        view.find("hello", true);
        assert_eq!(view.match_count(), 2);
        assert_eq!(view.search.current_match, Some(0));
    }

    #[test]
    fn test_simple_search_case_insensitive() {
        let mut view = simple_view(400.0, 160.0);
        view.set_text("Hello World\nhello world\nHELLO WORLD");

        view.find("hello", false);
        assert_eq!(view.match_count(), 3);
    }

    #[test]
    fn test_simple_search_navigation() {
        let mut view = simple_view(400.0, 160.0);
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
        let mut view = simple_view(400.0, 160.0);
        view.set_text("hello world");

        view.find("xyz", true);
        assert_eq!(view.match_count(), 0);
        assert_eq!(view.search.current_match, None);
    }

    #[test]
    fn a_match_range_is_an_offset_into_the_line_the_user_can_see() {
        // `İ` (U+0130) is two bytes but three once lowercased, so the old
        // implementation — which searched a `to_lowercase()` copy of the line
        // and used the offsets it found on the line itself — reported this
        // match one byte too far along. Every consumer of the range then
        // highlighted or replaced the wrong text, and `replace_range` on a
        // range past the end of a line panics outright.
        let mut view = simple_view(400.0, 160.0);
        view.set_text("İabc");

        view.find("ABC", false);
        assert_eq!(view.match_count(), 1);
        let &(line, start, end) = view.search.matches.first().expect("one match");
        assert_eq!((line, start, end), (0, 2, 5));
        assert_eq!(view.line_text(0).get(start..end), Some("abc"));
    }

    #[test]
    fn matches_do_not_overlap_each_other() {
        // The old loop resumed one byte into the match it had just recorded,
        // under a comment claiming overlapping matches were wanted. They are
        // not: the count shown to the user is wrong, and a replace-all that
        // rewrites overlapping ranges destroys the line.
        let mut view = simple_view(400.0, 160.0);
        view.set_text("aaaa");

        view.find("aa", true);
        assert_eq!(view.match_count(), 2);
        assert_eq!(view.search.matches, [(0, 0, 2), (0, 2, 4)]);
    }

    #[test]
    fn a_search_over_multi_byte_text_does_not_panic() {
        // The one-byte resume also stepped into the middle of a character,
        // where slicing the line panics.
        let mut view = simple_view(400.0, 160.0);
        view.set_text("日本日本語");

        view.find("日本", true);
        assert_eq!(view.search.matches, [(0, 0, 6), (0, 6, 12)]);
    }

    #[test]
    fn the_rich_view_search_agrees_with_the_simple_one() {
        // Both views had their own copy of the search, and so their own copy
        // of the same three bugs. They now share one implementation; this is
        // the test that fails if one of them grows a private copy again.
        let mut view = RichTextView::new(400.0, 160.0);
        view.set_blocks(vec![RichBlock::Paragraph {
            spans: vec![RichSpan::plain("İabc aaaa")],
            spacing_above: 0.0,
            spacing_below: 0.0,
        }]);

        view.find("ABC", false);
        assert_eq!(view.search.matches, [(0, 2, 5)]);

        view.find("aa", true);
        assert_eq!(view.search.matches, [(0, 6, 8), (0, 8, 10)]);
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
        let mut view = rich_view(80.0, 200.0); // 10 chars wide at 8px
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
    fn test_rich_word_wrap_breaks_long_words_on_char_boundaries() {
        // The old break sliced by a byte index derived from a *character*
        // count, so a long multi-byte word panicked outright. It must now be
        // cut between characters, and no text may be lost in the process.
        let word = "ééééééééééééééééééééééééééééééééé";
        let mut view = rich_view(40.0, 200.0);
        view.set_blocks(vec![RichBlock::Paragraph {
            spans: vec![RichSpan::plain(word)],
            spacing_above: 0.0,
            spacing_below: 0.0,
        }]);
        view.ensure_layout();
        assert!(
            view.wrapped_lines.len() >= 2,
            "an over-long word must break"
        );
        let rejoined: String = view
            .wrapped_lines
            .iter()
            .flat_map(|wl| wl.spans.iter().map(|s| s.text.as_str()))
            .collect();
        assert_eq!(
            rejoined, word,
            "breaking the word dropped or duplicated text"
        );
    }

    #[test]
    fn test_rich_heading_is_measured_at_its_own_size() {
        // A heading is drawn at up to twice the base size, so measuring it at
        // the base size wrapped it well past the right margin.
        let text = "a heading long enough to need wrapping";
        let mut plain = RichTextView::new(300.0, 200.0);
        plain.set_blocks(vec![RichBlock::Paragraph {
            spans: vec![RichSpan::plain(text)],
            spacing_above: 0.0,
            spacing_below: 0.0,
        }]);
        plain.ensure_layout();

        let mut heading = RichTextView::new(300.0, 200.0);
        heading.set_blocks(vec![RichBlock::Heading {
            level: HeadingLevel::H1,
            spans: vec![RichSpan::plain(text)],
        }]);
        heading.ensure_layout();

        assert!(
            heading.wrapped_lines.len() > plain.wrapped_lines.len(),
            "H1 ({} lines) must wrap sooner than body text ({} lines)",
            heading.wrapped_lines.len(),
            plain.wrapped_lines.len()
        );
    }

    /// A column boundary is where the drawn text actually reaches, not where a
    /// cell count says it should. Bytes were the first version of this bug and
    /// a character count the second; a tab is the case the second still got
    /// wrong, and tab-indented output is most of what a log view receives.
    #[test]
    fn simple_view_column_offsets_are_measured_not_counted() {
        let mut view = SimpleTextView::new(400.0, 200.0);
        view.set_text("\tab");
        let offsets = view.column_offsets(0);
        assert_eq!(offsets.len(), 4, "three characters have four boundaries");
        assert_eq!(offsets[0], 0.0);

        let cell = view.config.char_width;
        assert!(
            offsets[1] > cell * 1.5,
            "a tab reaches {} against a {cell} cell — if a tab really is one \
             cell wide on this face, this test has stopped testing anything",
            offsets[1]
        );
        // Monotonic, so a selection from any column to any later one has a
        // non-negative width — a rectangle drawn backwards is invisible.
        for pair in offsets.windows(2) {
            assert!(pair[1] >= pair[0], "offsets went backwards: {offsets:?}");
        }
        // The last boundary is where the line ends, which is what the drawn run
        // measures — this is the equality the pen and the marks now share.
        let drawn = crate::text::measure_in(
            "\tab",
            view.config.font_size,
            FontWeightHint::Regular,
            FontFamily::Mono,
        );
        assert!((offsets[3] - drawn).abs() < 0.01);
    }

    /// A click has to name the column the caret was drawn at. Before the
    /// offsets were measured, `hit_test` divided by a cell width while the
    /// caret was placed by counting cells, so on a tab-indented line clicking
    /// directly on a character selected from several columns away.
    #[test]
    fn simple_view_hit_test_inverts_the_placement_it_draws() {
        let mut view = SimpleTextView::new(400.0, 200.0);
        view.set_text("\tabc");
        for col in 0..=4 {
            let x = view.col_x(0, col);
            let hit = view.hit_test(x, 0.0);
            assert_eq!(
                hit.col, col,
                "column {col} is drawn at x={x} but a click there reports {}",
                hit.col
            );
        }
    }

    /// The premise `column_offsets` rests on, stated so it fails here first.
    ///
    /// It accumulates one advance per character rather than measuring each
    /// prefix, because measuring prefixes is quadratic in the line length and
    /// this view holds log output. That is only the same answer if measurement
    /// is additive — if the face applies no kerning across a character pair.
    /// It does not, on this font stack. If a face that does ever arrives, this
    /// test breaks and names the reason, rather than the caret quietly drifting
    /// a fraction of a pixel per character on one face and not another.
    #[test]
    fn simple_view_advances_are_additive() {
        let m = |s: &str| {
            crate::text::measure_in(
                s,
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                FontFamily::Mono,
            )
        };
        for (a, b) in [
            ("A", "V"),
            ("T", "o"),
            ("f", "i"),
            ("r", "n"),
            ("\t", "x"),
            ("é", "é"),
        ] {
            let joined = format!("{a}{b}");
            assert!(
                (m(&joined) - (m(a) + m(b))).abs() < 0.01,
                "{a:?}+{b:?} measures {} but the parts sum to {} — this face \
                 kerns, so `column_offsets` must go back to measuring prefixes",
                m(&joined),
                m(a) + m(b)
            );
        }
    }

    /// A line's length is in characters, because everything that draws against
    /// it is. It was `str::len()` — bytes — so the clamp meant to keep a
    /// selection inside a line let it run past the end of any line holding a
    /// non-ASCII character.
    #[test]
    fn simple_view_line_length_is_characters_not_bytes() {
        let mut view = SimpleTextView::new(400.0, 200.0);
        view.set_text("ééé");
        assert_eq!(view.line_char_count(0), 3, "three characters, six bytes");
    }

    #[test]
    fn test_simple_view_cell_width_tracks_the_font_size() {
        // `char_width` was a hardcoded 8.0, which matched the built-in face at
        // 14 px and nothing else, so a larger font gave a grid that drifted
        // further out of true with every column.
        let small = default_char_width(12.0);
        let large = default_char_width(48.0);
        assert!(small > 0.0, "12px cell measured {small}");
        assert!(large > small, "48px cell {large} <= 12px cell {small}");
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

    /// A highlight that runs across two spans is drawn as one rectangle, not
    /// two: the spans are laid out end to end, so their boxes abut and merge.
    /// This is the case the old two-edge code got right, and the check that the
    /// list form did not regress it.
    #[test]
    fn a_highlight_across_two_spans_is_still_one_rectangle() {
        let mut view = RichTextView::new(4000.0, 200.0);
        view.set_blocks(vec![RichBlock::Paragraph {
            spans: vec![RichSpan::plain("plain "), RichSpan::bold("bold")],
            spacing_above: 0.0,
            spacing_below: 0.0,
        }]);
        view.rebuild_layout();
        let wl = view
            .wrapped_lines
            .first()
            .expect("one paragraph wraps to at least one line")
            .clone();

        let all = view.selection_boxes_of_cols(&wl, 0, 10);
        assert_eq!(all.len(), 1, "{all:?}");
        // …and it covers the whole line, both spans included.
        let total: f32 = wl.spans.iter().map(|s| view.span_width(s, None)).sum();
        assert!((all[0].0).abs() < 0.001, "starts at {}", all[0].0);
        assert!(
            (all[0].1 - total).abs() < 0.01,
            "width {} vs line {total}",
            all[0].1
        );

        // Splitting the range in two tiles the same ground: the box for the
        // second half begins exactly where the box for the first half ends.
        // Stated against the two halves rather than against a span width
        // because wrapping is free to cut a span up — "plain " arrives as
        // "plain" and " " — so a span index is not a column count.
        let left = view.selection_boxes_of_cols(&wl, 0, 6);
        let right = view.selection_boxes_of_cols(&wl, 6, 10);
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(right.len(), 1, "{right:?}");
        let seam = left[0].0 + left[0].1;
        assert!((right[0].0 - seam).abs() < 0.01, "{} vs {seam}", right[0].0);
        assert!(
            (left[0].1 + right[0].1 - all[0].1).abs() < 0.01,
            "halves {} + {} vs whole {}",
            left[0].1,
            right[0].1,
            all[0].1
        );
    }

    /// The degenerate ranges paint nothing rather than a zero-width sliver, and
    /// a range past the end of the line is clamped instead of panicking — a
    /// selection can outlive the re-layout that shortened its line.
    #[test]
    fn a_highlight_of_nothing_or_of_too_much_does_not_panic() {
        let mut view = RichTextView::new(4000.0, 200.0);
        view.set_blocks(vec![RichBlock::Paragraph {
            spans: vec![RichSpan::plain("héllo")],
            spacing_above: 0.0,
            spacing_below: 0.0,
        }]);
        view.rebuild_layout();
        let wl = view.wrapped_lines.first().expect("one line").clone();

        assert!(view.selection_boxes_of_cols(&wl, 2, 2).is_empty());
        assert!(view.selection_boxes_of_cols(&wl, 4, 1).is_empty());
        // Five characters, six bytes: a column count is not a byte count, and
        // asking for column 99 must clamp to the end of the string.
        let past = view.selection_boxes_of_cols(&wl, 0, 99);
        assert_eq!(past.len(), 1, "{past:?}");
        let full = view.span_width(&wl.spans[0], None);
        assert!((past[0].1 - full).abs() < 0.01, "{} vs {full}", past[0].1);
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
        let mut view = simple_view(400.0, 160.0);
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
        let mut view = simple_view(400.0, 160.0);
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

    /// The grid is placed on `config.char_width`, which comes from the mono
    /// face — so it has to be *drawn* in the mono face too. Measuring in one
    /// face and drawing in another is the defect this scope exists to prevent,
    /// and it is invisible in any assertion about positions alone.
    #[test]
    fn the_simple_grid_is_drawn_in_the_family_it_was_measured_in() {
        let mut view = simple_view(400.0, 160.0);
        view.config.show_line_numbers = true;
        view.set_text("wide WWWW\nnarrow iiii\nmixed Wi0#");

        let mut tree = RenderTree::new();
        view.render(&mut tree);

        let mut depth = 0_i32;
        let mut deepest = 0_i32;
        let mut inside = 0_usize;
        for cmd in &tree.commands {
            match cmd {
                RenderCommand::PushFont { family } => {
                    assert_eq!(family, &FontFamily::Mono, "only the grid pushes a family");
                    depth += 1;
                    deepest = deepest.max(depth);
                }
                RenderCommand::PopFont => {
                    depth -= 1;
                    assert!(depth >= 0, "a PopFont without a matching PushFont");
                }
                RenderCommand::Text { .. } if depth > 0 => inside += 1,
                _ => {}
            }
        }
        assert_eq!(depth, 0, "the font scopes do not balance");
        assert_eq!(deepest, 1, "the grid's scope was never opened");
        // Three lines of content plus three gutter numbers, all on the grid.
        assert!(
            inside >= 6,
            "only {inside} glyph runs landed in the mono scope"
        );
    }

    /// Every glyph a log line can contain has to fit the cell the grid steps
    /// by. In a proportional face this is false by construction — `'W'` is
    /// nearly twice a digit — which is exactly how spans came to overrun their
    /// own selection bands.
    #[test]
    fn a_character_fits_a_simple_view_cell() {
        let cell = default_char_width(DEFAULT_FONT_SIZE);
        for ch in ['0', 'W', 'i', '#', 'é', 'M', '@', ' '] {
            let w = crate::text::measure_in(
                &ch.to_string(),
                DEFAULT_FONT_SIZE,
                FontWeightHint::Regular,
                FontFamily::Mono,
            );
            assert!(w <= cell + 0.01, "{ch:?} measures {w} in a {cell} cell");
        }
    }

    /// A rich view is not a grid, so its indent unit must not be taken from
    /// the mono face — it indents prose that is drawn proportionally.
    #[test]
    fn a_rich_view_indents_by_the_face_it_draws_in() {
        assert_eq!(
            RichTextViewConfig::default().char_width,
            crate::text::digit_advance(DEFAULT_FONT_SIZE, FontWeightHint::Regular),
        );
    }

    #[test]
    fn test_simple_render_with_selection() {
        let mut view = simple_view(400.0, 160.0);
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
        let mut view = simple_view(400.0, 160.0);
        view.set_text("hello world");

        // Click at (24.0, 0.0) should be col 3 (24/8)
        let pos = view.hit_test(24.0, 0.0);
        assert_eq!(pos, TextPosition::new(0, 3));
    }

    #[test]
    fn test_hit_test_with_scroll() {
        let mut view = simple_view(400.0, 160.0);
        let text = (0..50)
            .map(|i| format!("Line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        view.set_text(&text);
        view.scroll_offset = 10;

        // Click at y=0 should be line 10 (scroll_offset + 0)
        let pos = view.hit_test(0.0, 0.0);
        assert_eq!(pos.line, 10);
    }
}

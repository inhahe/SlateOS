//! A code editor, drawn and driven: [`CodeView`] puts a
//! [`CodeEditor`](crate::codeedit::CodeEditor) on screen -- a gutter of line
//! numbers, the text in the fixed-pitch face, every selection and caret, the
//! bracket pair at the caret, a scrollbar -- and turns keys and the pointer
//! into its commands.
//!
//! # Rows
//!
//! Without wrapping, a line is a row and the view scrolls sideways to keep
//! the caret in sight. With wrapping ([`ViewOptions::wrap`]), a line longer
//! than the view is broken into rows -- after a blank where one is near the
//! edge, mid-word where none is -- and Up and Down move by row, keeping the
//! caret's distance from the left.
//!
//! Rows are worked out for the lines on screen and nowhere else, so a file of
//! any size costs a screenful to draw. The price is the scrollbar under
//! wrapping: it measures lines, not rows, so a long wrapped line makes the
//! thumb move unevenly -- which every editor that wraps without laying out the
//! whole file shares.
//!
//! # Positions
//!
//! A character's position is measured, not counted: the fixed-pitch face
//! draws most characters one cell wide and some -- CJK, emoji -- two, and a
//! tab runs to the next stop. What is drawn is the same string that was
//! measured (tabs as spaces), so a caret sits where its character was drawn.
//!
//! # Finding
//!
//! Ctrl+F and Ctrl+H open the find bar across the top ([`findbar`]); the
//! rows start below it. F3 and Shift+F3 step through the matches from the
//! text as well.
//!
//! # The clipboard
//!
//! The view does not own one: Ctrl+C and Ctrl+X hand the text to the host
//! ([`CodeViewEvent::Copy`], [`CodeViewEvent::Cut`]) and Ctrl+V asks the host
//! for it ([`CodeViewEvent::Paste`]), which answers with
//! [`CodeView::paste`] -- the system clipboard is the host's to reach.

use core::ops::Range;

use crate::codeedit::{CodeEditor, Selection};
use crate::color::Color;
use crate::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::palette::Palette;
use crate::render::{FontFamily, FontWeightHint, RenderCommand, TextOverflow, TextSpan};
use crate::scrollbar;
use crate::style::CornerRadii;
use crate::surface::CommandSink;
use crate::text;
use crate::theme::with_alpha;
use crate::wheel;

mod findbar;

use findbar::{BarAction, Field, FindBar};

/// Room between the gutter's numbers and its edges, in cells.
const GUTTER_PADDING_CELLS: f32 = 1.0;
/// Room between the gutter and the text, in pixels.
const TEXT_INSET: f32 = 4.0;
/// How near the view's edge a caret may come before the view scrolls
/// sideways, in cells.
const SIDE_MARGIN_CELLS: f32 = 4.0;
/// How wide a caret is drawn when the host has said nothing.
const DEFAULT_CARET_WIDTH: f32 = 2.0;
/// How strongly the current line is marked.
const CURRENT_LINE_ALPHA: u8 = 70;

/// How the view draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewOptions {
    /// Whether the gutter of line numbers is shown.
    pub line_numbers: bool,
    /// Whether long lines are broken into rows, rather than the view
    /// scrolling sideways.
    pub wrap: bool,
    /// The text's size.
    pub font_size: f32,
    /// How wide a caret is drawn: the user's caret width.
    pub caret_width: f32,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            line_numbers: true,
            wrap: false,
            font_size: 13.0,
            caret_width: DEFAULT_CARET_WIDTH,
        }
    }
}

/// What the view did with an event, for its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeViewEvent {
    /// The text changed.
    Changed,
    /// A caret or a selection moved, or the view scrolled: draw again.
    Moved,
    /// Put this on the clipboard (Ctrl+C).
    Copy(String),
    /// This was cut: put it on the clipboard (Ctrl+X). The text changed.
    Cut(String),
    /// Ctrl+V: read the clipboard and hand it to [`CodeView::paste`].
    Paste,
}

/// One row on screen: part of a line, or all of it.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    line: usize,
    /// Which of the line's rows it is.
    index: usize,
    /// The bytes of the text it shows, the line's newline not included.
    range: Range<usize>,
    /// How far from the line's start it begins, in pixels -- what a tab in it
    /// is measured from.
    x_in_line: f32,
}

/// What a pointer drag is doing.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Extending the primary selection from where the press was.
    Text,
    /// Extending a double-click's word selection, a word at a time.
    Words { anchor: Selection },
    /// Selecting a block from `anchor` (Alt+drag).
    Block { anchor: usize },
    /// Dragging the scrollbar's thumb, taken hold of this far below its top.
    Thumb { grab: f32 },
}

/// A code editor on screen. See the module docs.
#[derive(Debug)]
pub struct CodeView {
    editor: CodeEditor,
    options: ViewOptions,
    bounds: Rect,
    /// The first line on screen, and which of its rows is at the top.
    top_line: usize,
    top_row: usize,
    /// How far the text is scrolled sideways, without wrapping.
    scroll_x: f32,
    focused: bool,
    drag: Option<Drag>,
    bar_hover: bool,
    wheel: wheel::Accumulator,
    /// Each caret's distance from the left for Up and Down under wrapping.
    goal_x: Option<Vec<f32>>,
    /// The find bar.
    find: FindBar,
    /// Where the caret was when the find bar opened: an incremental search
    /// looks for the first match from here, however far typing has taken it.
    find_origin: usize,
}

impl CodeView {
    /// A view of `editor`.
    #[must_use]
    pub fn new(editor: CodeEditor) -> Self {
        Self {
            editor,
            options: ViewOptions::default(),
            bounds: Rect::EMPTY,
            top_line: 0,
            top_row: 0,
            scroll_x: 0.0,
            focused: false,
            drag: None,
            bar_hover: false,
            wheel: wheel::Accumulator::default(),
            goal_x: None,
            find: FindBar::default(),
            find_origin: 0,
        }
    }

    /// The editor.
    #[must_use]
    pub fn editor(&self) -> &CodeEditor {
        &self.editor
    }

    /// The editor, to act on directly; the view follows the primary caret
    /// the next time it is drawn after [`reveal`](Self::reveal).
    pub fn editor_mut(&mut self) -> &mut CodeEditor {
        &mut self.editor
    }

    /// How the view draws.
    #[must_use]
    pub fn options(&self) -> ViewOptions {
        self.options
    }

    /// Draw differently from now on.
    pub fn set_options(&mut self, options: ViewOptions) {
        self.options = options;
        if options.wrap {
            self.scroll_x = 0.0;
        }
        self.top_row = 0;
        self.reveal();
    }

    /// Where the view is, in the host's space.
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
        self.reveal();
    }

    /// Whether the view has the keyboard: carets are drawn, and the current
    /// line marked, only when it has.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// The first line on screen.
    #[must_use]
    pub fn top_line(&self) -> usize {
        self.top_line
    }

    /// Paste `text` at every caret: the host's answer to
    /// [`CodeViewEvent::Paste`].
    pub fn paste(&mut self, text: &str) {
        self.editor.paste(text);
        self.after_edit();
    }

    /// Open the find bar with the keyboard in its find field -- and the
    /// replace row too when `replace`. The field starts with the selected
    /// text when that is one line, all of it selected, so typing replaces
    /// it.
    pub fn open_find(&mut self, replace: bool) {
        let primary = self.editor.primary();
        if !primary.is_empty() {
            let selected = self
                .editor
                .buffer()
                .slice(primary.range())
                .unwrap_or_default();
            if !selected.contains('\n') {
                self.find.find.set_text(&selected);
            }
        }
        self.find.find.select_all();
        self.find.open = true;
        self.find.replace = replace;
        self.find.focused = true;
        self.find.field = Field::Find;
        self.find_origin = primary.range().start;
        self.refresh_matches();
    }

    /// Close the find bar and give the keyboard back to the text.
    pub fn close_find(&mut self) {
        self.find.open = false;
        self.find.focused = false;
        self.find.matches.clear();
    }

    /// Whether the find bar is open.
    #[must_use]
    pub fn find_open(&self) -> bool {
        self.find.open
    }

    /// Find the matches again, for a changed query or a changed text.
    fn refresh_matches(&mut self) {
        let finder = self.find.finder();
        self.find.matches = finder.map_or_else(Vec::new, |f| self.editor.find_all(&f));
        self.find.revision = Some(self.editor.revision());
    }

    /// Carry out what a key in the find bar asked for.
    fn act_on_bar(&mut self, action: BarAction) -> CodeViewEvent {
        match action {
            BarAction::Redraw => CodeViewEvent::Moved,
            BarAction::Close => {
                self.close_find();
                CodeViewEvent::Moved
            }
            BarAction::Search => {
                self.refresh_matches();
                // The first match from where the caret was when the bar
                // opened, round the end if need be.
                let next = self
                    .find
                    .matches
                    .iter()
                    .find(|m| m.start >= self.find_origin)
                    .or_else(|| self.find.matches.first())
                    .cloned();
                if let Some(m) = next {
                    self.editor.set_selections(vec![Selection {
                        anchor: m.start,
                        head: m.end,
                    }]);
                    self.reveal();
                }
                CodeViewEvent::Moved
            }
            BarAction::Next { backwards } => {
                if let Some(finder) = self.find.finder() {
                    self.editor.find_next(&finder, backwards);
                    self.reveal();
                }
                CodeViewEvent::Moved
            }
            BarAction::SelectAll => {
                if let Some(finder) = self.find.finder()
                    && self.editor.select_all_matches(&finder) > 0
                {
                    self.find.focused = false;
                    self.reveal();
                }
                CodeViewEvent::Moved
            }
            BarAction::Replace | BarAction::ReplaceAll => {
                let Some(finder) = self.find.finder() else {
                    return CodeViewEvent::Moved;
                };
                let with = self.find.with.text().to_owned();
                let changed = if action == BarAction::ReplaceAll {
                    self.editor.replace_all(&finder, &with) > 0
                } else {
                    self.editor.replace_next(&finder, &with)
                };
                self.after_edit();
                if changed {
                    CodeViewEvent::Changed
                } else {
                    CodeViewEvent::Moved
                }
            }
        }
    }

    /// Which of the matches the primary selection is on, if any.
    fn current_match(&self) -> Option<usize> {
        let primary = self.editor.primary().range();
        self.find.matches.iter().position(|m| *m == primary)
    }

    // ------------------------------------------------------------------
    // Geometry
    // ------------------------------------------------------------------

    fn cell(&self) -> f32 {
        text::cell_advance(self.options.font_size, FontWeightHint::Regular).max(1.0)
    }

    fn line_height(&self) -> f32 {
        text::line_height_in(
            self.options.font_size,
            FontWeightHint::Regular,
            FontFamily::Mono,
        )
        .max(1.0)
    }

    /// The gutter's width: room for the largest line number and a cell
    /// either side; nothing without line numbers.
    fn gutter_width(&self) -> f32 {
        if !self.options.line_numbers {
            return 0.0;
        }
        let digits = digits(self.editor.buffer().line_count());
        #[allow(clippy::cast_precision_loss, reason = "a digit count is tiny")]
        let cells = digits as f32 + GUTTER_PADDING_CELLS * 2.0;
        cells * self.cell()
    }

    /// Where the text is drawn: the bounds less the find bar, the gutter,
    /// the inset and the scrollbar's column.
    fn text_rect(&self) -> Rect {
        let left = self.bounds.x + self.gutter_width() + TEXT_INSET;
        let width = (self.bounds.right() - scrollbar::WIDTH - left).max(0.0);
        Rect::new(left, self.rows_top(), width, self.rows_height())
    }

    /// Where the first row is drawn: below the find bar, when it is open.
    fn rows_top(&self) -> f32 {
        self.bounds.y + self.find.height()
    }

    /// The height the rows have.
    fn rows_height(&self) -> f32 {
        (self.bounds.h - self.find.height()).max(0.0)
    }

    /// How many rows fit on screen, at least one.
    fn visible_rows(&self) -> usize {
        let rows = (self.rows_height() / self.line_height()).floor();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "floored, and held to at least one"
        )]
        let rows = rows.max(1.0) as usize;
        rows
    }

    /// The width of `c` drawn at pixel `x` from its line's start: a tab to
    /// the next stop, anything else as the face draws it.
    fn char_width(&self, c: char, x: f32) -> f32 {
        if c == '\t' {
            let stop = self.cell() * f32::from(self.editor.options().tab_width.clamp(1, 16));
            let into = x % stop;
            return stop - into;
        }
        let mut buf = [0u8; 4];
        text::measure_in(
            c.encode_utf8(&mut buf),
            self.options.font_size,
            FontWeightHint::Regular,
            FontFamily::Mono,
        )
    }

    /// The rows of `line`: one, without wrapping; as many as its width
    /// needs, with.
    fn rows_of(&self, line: usize) -> Vec<Row> {
        let buffer = self.editor.buffer();
        let Some(range) = buffer.line_range(line) else {
            return Vec::new();
        };
        if !self.options.wrap {
            return vec![Row {
                line,
                index: 0,
                range,
                x_in_line: 0.0,
            }];
        }
        let limit = self.text_rect().w.max(self.cell());
        let mut rows = Vec::new();
        let mut start = range.start;
        let mut row_x = 0.0;
        let mut x = 0.0;
        // The last place a row could break after a blank, and the x there.
        let mut soft: Option<(usize, f32)> = None;
        for (at, c) in buffer.chars(range.start) {
            if at >= range.end {
                break;
            }
            let w = self.char_width(c, x);
            if x + w - row_x > limit && at > start {
                let (end, end_x) = soft.filter(|(s, _)| *s > start).unwrap_or((at, x));
                rows.push(Row {
                    line,
                    index: rows.len(),
                    range: start..end,
                    x_in_line: row_x,
                });
                start = end;
                row_x = end_x;
                soft = None;
            }
            x += w;
            if c == ' ' || c == '\t' {
                soft = Some((at.saturating_add(c.len_utf8()), x));
            }
        }
        rows.push(Row {
            line,
            index: rows.len(),
            range: start..range.end,
            x_in_line: row_x,
        });
        rows
    }

    /// The rows on screen, top to bottom.
    fn screen_rows(&self) -> Vec<Row> {
        let wanted = self.visible_rows();
        let mut rows = Vec::with_capacity(wanted);
        let mut line = self.top_line;
        let mut skip = self.top_row;
        while rows.len() < wanted && line < self.editor.buffer().line_count() {
            for row in self.rows_of(line).into_iter().skip(skip) {
                if rows.len() >= wanted {
                    break;
                }
                rows.push(row);
            }
            skip = 0;
            line = line.saturating_add(1);
        }
        rows
    }

    /// The x of `offset` within `row`, from the row's start.
    fn x_in_row(&self, row: &Row, offset: usize) -> f32 {
        let mut x = row.x_in_line;
        for (at, c) in self.editor.buffer().chars(row.range.start) {
            if at >= offset || at >= row.range.end {
                break;
            }
            x += self.char_width(c, x);
        }
        x - row.x_in_line
    }

    /// The offset in `row` nearest `x` from the row's start.
    fn offset_in_row(&self, row: &Row, x: f32) -> usize {
        let mut pen = row.x_in_line;
        let target = row.x_in_line + x;
        for (at, c) in self.editor.buffer().chars(row.range.start) {
            if at >= row.range.end {
                break;
            }
            let w = self.char_width(c, pen);
            if target < pen + w / 2.0 {
                return at;
            }
            pen += w;
        }
        row.range.end
    }

    /// The offset under the point `(x, y)` in the host's space, held to the
    /// text: above it is the first row on screen, below it the last.
    fn offset_at(&self, x: f32, y: f32) -> usize {
        let rows = self.screen_rows();
        let Some(last) = rows.last() else {
            return self.editor.buffer().len();
        };
        let index = ((y - self.rows_top()) / self.line_height()).floor();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "held to the rows on screen"
        )]
        let index = index.max(0.0) as usize;
        let row = rows.get(index).unwrap_or(last);
        let text = self.text_rect();
        self.offset_in_row(row, x - text.x + self.scroll_x)
    }

    // ------------------------------------------------------------------
    // Scrolling
    // ------------------------------------------------------------------

    /// Scroll so the primary caret is on screen, with a margin to the sides.
    pub fn reveal(&mut self) {
        let head = self.editor.primary().head;
        let buffer = self.editor.buffer();
        let line = buffer.line_of(head).unwrap_or(0);
        let rows = self.rows_of(line);
        let row = row_index_of(&rows, head);
        // Above the top: the caret's row is the top.
        if (line, row) < (self.top_line, self.top_row) {
            self.top_line = line;
            self.top_row = row;
        } else {
            // Below the bottom: count back from the caret's row a screen's
            // worth of rows, and start there.
            let visible = self.visible_rows();
            let on_screen = self
                .screen_rows()
                .iter()
                .any(|r| r.line == line && r.index == row);
            if !on_screen {
                let (mut top_line, mut top_row) = (line, row);
                let mut back = visible.saturating_sub(1);
                while back > 0 {
                    if top_row > 0 {
                        top_row = top_row.saturating_sub(1);
                    } else if top_line > 0 {
                        top_line = top_line.saturating_sub(1);
                        top_row = self.rows_of(top_line).len().saturating_sub(1);
                    } else {
                        break;
                    }
                    back = back.saturating_sub(1);
                }
                self.top_line = top_line;
                self.top_row = top_row;
            }
        }
        if self.options.wrap {
            self.scroll_x = 0.0;
            return;
        }
        let Some(caret_row) = rows.get(row) else {
            return;
        };
        let x = self.x_in_row(caret_row, head);
        let margin = SIDE_MARGIN_CELLS * self.cell();
        let width = self.text_rect().w;
        if x - self.scroll_x < margin {
            self.scroll_x = (x - margin).max(0.0);
        } else if x - self.scroll_x > width - margin {
            self.scroll_x = (x - width + margin).max(0.0);
        }
    }

    /// Scroll by `rows` rows, down for positive.
    fn scroll_rows(&mut self, rows: isize) {
        let line_count = self.editor.buffer().line_count();
        let mut left = rows.unsigned_abs();
        while left > 0 {
            if rows > 0 {
                let here = self.rows_of(self.top_line).len();
                if self.top_row.saturating_add(1) < here {
                    self.top_row = self.top_row.saturating_add(1);
                } else if self.top_line.saturating_add(1) < line_count {
                    self.top_line = self.top_line.saturating_add(1);
                    self.top_row = 0;
                } else {
                    break;
                }
            } else if self.top_row > 0 {
                self.top_row = self.top_row.saturating_sub(1);
            } else if self.top_line > 0 {
                self.top_line = self.top_line.saturating_sub(1);
                self.top_row = self.rows_of(self.top_line).len().saturating_sub(1);
            } else {
                break;
            }
            left = left.saturating_sub(1);
        }
    }

    /// The scrollbar's column and thumb: lines on screen of lines in all.
    fn scroll_geometry(&self) -> Option<(Rect, Rect)> {
        let total = self.editor.buffer().line_count();
        let visible = self.visible_rows();
        if !scrollbar::needed(total, visible) {
            return None;
        }
        let track = Rect::new(
            self.bounds.right() - scrollbar::WIDTH,
            self.rows_top(),
            scrollbar::WIDTH,
            self.rows_height(),
        );
        Some((
            track,
            scrollbar::thumb(track, total, visible, self.top_line),
        ))
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    /// Draw the view: its ground, the gutter, the current line, the text with
    /// its selections, the bracket pair and the carets, and the scrollbar.
    pub fn draw(&self, sink: &mut impl CommandSink, p: &Palette) {
        let b = self.bounds;
        if b.w <= 0.0 || b.h <= 0.0 {
            return;
        }
        sink.emit(RenderCommand::PushClip {
            x: b.x,
            y: b.y,
            width: b.w,
            height: b.h,
        });
        sink.emit(fill(b, p.base));
        let gutter = self.gutter_width();
        if gutter > 0.0 {
            sink.emit(fill(Rect::new(b.x, b.y, gutter, b.h), p.mantle));
        }
        let rows = self.screen_rows();
        let line_h = self.line_height();
        let text = self.text_rect();
        let primary = self.editor.primary();
        let caret_line = self.editor.buffer().line_of(primary.head).unwrap_or(0);
        sink.emit(RenderCommand::PushFont {
            family: FontFamily::Mono,
        });
        for (i, row) in rows.iter().enumerate() {
            #[allow(clippy::cast_precision_loss, reason = "a row on screen")]
            let y = self.rows_top() + i as f32 * line_h;
            if self.focused && primary.is_empty() && row.line == caret_line {
                sink.emit(fill(
                    Rect::new(text.x - TEXT_INSET, y, text.w + TEXT_INSET, line_h),
                    with_alpha(p.surface0, CURRENT_LINE_ALPHA),
                ));
            }
            if gutter > 0.0 && row.index == 0 {
                let number = row.line.saturating_add(1).to_string();
                let w = text::measure_in(
                    &number,
                    self.options.font_size,
                    FontWeightHint::Regular,
                    FontFamily::Mono,
                );
                sink.emit(RenderCommand::Text {
                    x: b.x + gutter - GUTTER_PADDING_CELLS * self.cell() - w,
                    y,
                    text: number,
                    color: if row.line == caret_line {
                        p.text
                    } else {
                        p.subtext0
                    },
                    font_size: self.options.font_size,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
            self.draw_row(sink, p, row, y, text);
        }
        self.draw_matches(sink, p, &rows, text);
        self.draw_bracket_pair(sink, p, &rows, text);
        if self.focused && !self.find.focused {
            self.draw_carets(sink, p, &rows, text);
        }
        sink.emit(RenderCommand::PopFont);
        self.find
            .draw(sink, p, b, self.current_match(), self.options.caret_width);
        if let Some((track, thumb)) = self.scroll_geometry() {
            scrollbar::draw(
                sink,
                p,
                track,
                thumb,
                scrollbar::BarState {
                    hovered: self.bar_hover,
                    dragging: matches!(self.drag, Some(Drag::Thumb { .. })),
                },
            );
        }
        sink.emit(RenderCommand::PopClip);
    }

    /// One row: the selections behind it, then its text -- tabs as spaces,
    /// the selected part in the ink that reads on the selection.
    fn draw_row(&self, sink: &mut impl CommandSink, p: &Palette, row: &Row, y: f32, text: Rect) {
        let line_h = self.line_height();
        let buffer = self.editor.buffer();
        // The row as drawn, with where each of its bytes went.
        let mut shown = String::new();
        let mut map: Vec<(usize, usize)> = Vec::new();
        let mut x = row.x_in_line;
        for (at, c) in buffer.chars(row.range.start) {
            if at >= row.range.end {
                break;
            }
            map.push((at, shown.len()));
            let w = self.char_width(c, x);
            if c == '\t' {
                let spaces = (w / self.cell()).round().max(1.0);
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a tab is at most sixteen cells"
                )]
                shown.push_str(&" ".repeat(spaces as usize));
            } else {
                shown.push(c);
            }
            x += w;
        }
        let shown_at = |offset: usize| {
            map.iter()
                .find(|(at, _)| *at >= offset)
                .map_or(shown.len(), |(_, s)| *s)
        };
        let left = text.x - self.scroll_x;
        let mut spans: Vec<TextSpan> = Vec::new();
        for s in self.editor.selections() {
            let range = s.range();
            if range.is_empty() || range.end < row.range.start || range.start > row.range.end {
                continue;
            }
            let from = range.start.max(row.range.start);
            let to = range.end.min(row.range.end);
            let x1 = self.x_in_row(row, from);
            let mut x2 = self.x_in_row(row, to);
            // A selection running on past the row's end takes a cell more,
            // so a selected newline shows.
            if range.end > row.range.end {
                x2 += self.cell() / 2.0;
            }
            if x2 > x1 {
                sink.emit(fill(Rect::new(left + x1, y, x2 - x1, line_h), p.accent));
            }
            let (a, b) = (shown_at(from), shown_at(to));
            if b > a {
                spans.push(TextSpan {
                    end: u32::try_from(a).unwrap_or(u32::MAX),
                    color: p.text,
                });
                spans.push(TextSpan {
                    end: u32::try_from(b).unwrap_or(u32::MAX),
                    color: p.on_accent(),
                });
            }
        }
        if shown.is_empty() {
            return;
        }
        spans.sort_by_key(|s| s.end);
        sink.emit(RenderCommand::RichText {
            x: left,
            y,
            text: shown,
            spans,
            color: p.text,
            font_size: self.options.font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Every match of the find bar's query on screen, outlined in yellow's
    /// ink: the one the caret is on is the selection already.
    fn draw_matches(&self, sink: &mut impl CommandSink, p: &Palette, rows: &[Row], text: Rect) {
        if !self.find.open {
            return;
        }
        let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
            return;
        };
        let (top, bottom) = (first.range.start, last.range.end);
        for m in &self.find.matches {
            if m.end < top || m.start > bottom {
                continue;
            }
            for (i, row) in rows.iter().enumerate() {
                let from = m.start.max(row.range.start);
                let to = m.end.min(row.range.end);
                if from >= to && !(m.start == m.end && m.start == row.range.start) {
                    continue;
                }
                #[allow(clippy::cast_precision_loss, reason = "a row on screen")]
                let y = self.rows_top() + i as f32 * self.line_height();
                let x1 = self.x_in_row(row, from);
                let x2 = self.x_in_row(row, to);
                sink.emit(RenderCommand::StrokeRect {
                    x: text.x - self.scroll_x + x1,
                    y,
                    width: (x2 - x1).max(1.0),
                    height: self.line_height(),
                    color: p.ink(p.yellow),
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
        }
    }

    /// The bracket at the primary caret and its partner, outlined.
    fn draw_bracket_pair(
        &self,
        sink: &mut impl CommandSink,
        p: &Palette,
        rows: &[Row],
        text: Rect,
    ) {
        let head = self.editor.primary().head;
        let Some((a, b)) = self.editor.matching_bracket(head) else {
            return;
        };
        for at in [a, b] {
            if let Some((x, y, w)) = self.char_box(rows, text, at) {
                sink.emit(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: w,
                    height: self.line_height(),
                    color: p.overlay0,
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
        }
    }

    /// Every caret, as a bar at its character's left.
    fn draw_carets(&self, sink: &mut impl CommandSink, p: &Palette, rows: &[Row], text: Rect) {
        for s in self.editor.selections() {
            let Some(row) = self.row_on_screen(rows, s.head) else {
                continue;
            };
            let Some(i) = rows.iter().position(|r| r == row) else {
                continue;
            };
            #[allow(clippy::cast_precision_loss, reason = "a row on screen")]
            let y = self.rows_top() + i as f32 * self.line_height();
            let x = text.x - self.scroll_x + self.x_in_row(row, s.head);
            sink.emit(fill(
                Rect::new(x, y, self.options.caret_width.max(1.0), self.line_height()),
                p.text,
            ));
        }
    }

    /// The row on screen `offset` is drawn on.
    fn row_on_screen<'r>(&self, rows: &'r [Row], offset: usize) -> Option<&'r Row> {
        let line = self.editor.buffer().line_of(offset).ok()?;
        let of_line: Vec<&Row> = rows.iter().filter(|r| r.line == line).collect();
        of_line
            .iter()
            .rev()
            .find(|r| r.range.start <= offset)
            .copied()
    }

    /// The box of the character at `at` on screen: its x, y and width.
    fn char_box(&self, rows: &[Row], text: Rect, at: usize) -> Option<(f32, f32, f32)> {
        let row = self.row_on_screen(rows, at)?;
        let i = rows.iter().position(|r| r == row)?;
        #[allow(clippy::cast_precision_loss, reason = "a row on screen")]
        let y = self.rows_top() + i as f32 * self.line_height();
        let x = self.x_in_row(row, at);
        let c = self.editor.buffer().char_at(at)?;
        let w = self.char_width(c, row.x_in_line + x);
        Some((text.x - self.scroll_x + x, y, w))
    }

    // ------------------------------------------------------------------
    // Keys
    // ------------------------------------------------------------------

    /// Act on a key; `None` when it is not the view's.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<CodeViewEvent> {
        if !key.pressed {
            return None;
        }
        let m = key.modifiers;
        let shift = m.shift;
        let ctrl = m.ctrl && !m.alt;
        match key.key {
            Key::F if ctrl => {
                self.open_find(false);
                return Some(CodeViewEvent::Moved);
            }
            Key::H if ctrl => {
                self.open_find(true);
                return Some(CodeViewEvent::Moved);
            }
            Key::F3 => {
                let event = self.act_on_bar(BarAction::Next { backwards: shift });
                return Some(event);
            }
            _ => {}
        }
        if self.find.open && self.find.focused {
            // Every other key is the bar's while it has the keyboard -- a key
            // it does not use is nobody's, not the text's behind it.
            return self
                .find
                .handle_key(key)
                .map(|action| self.act_on_bar(action));
        }
        let page = self.visible_rows().saturating_sub(1).max(1);
        let event = match key.key {
            Key::Left if ctrl => Some(self.moved(|e| e.move_word_left(shift))),
            Key::Right if ctrl => Some(self.moved(|e| e.move_word_right(shift))),
            Key::Left => Some(self.moved(|e| e.move_left(shift))),
            Key::Right => Some(self.moved(|e| e.move_right(shift))),
            Key::Up if m.ctrl && m.alt => Some(self.moved(|e| e.add_caret_vertically(false))),
            Key::Down if m.ctrl && m.alt => Some(self.moved(|e| e.add_caret_vertically(true))),
            Key::Up => Some(self.vertical(-1, shift)),
            Key::Down => Some(self.vertical(1, shift)),
            Key::PageUp => Some(self.vertical(signed(page).saturating_neg(), shift)),
            Key::PageDown => Some(self.vertical(signed(page), shift)),
            Key::Home if ctrl => Some(self.moved(|e| e.move_to_start(shift))),
            Key::End if ctrl => Some(self.moved(|e| e.move_to_end(shift))),
            Key::Home => Some(self.moved(|e| e.move_home(shift))),
            Key::End => Some(self.moved(|e| e.move_end(shift))),
            Key::Backspace if ctrl => Some(self.changed(CodeEditor::delete_word_back)),
            Key::Backspace => Some(self.changed(CodeEditor::backspace)),
            Key::Delete if ctrl => Some(self.changed(CodeEditor::delete_word_forward)),
            Key::Delete => Some(self.changed(CodeEditor::delete_forward)),
            Key::Enter if !ctrl => Some(self.changed(CodeEditor::newline)),
            Key::Tab if shift => Some(self.changed(CodeEditor::dedent)),
            Key::Tab if !ctrl => Some(self.changed(CodeEditor::tab)),
            Key::Escape if self.editor.selections().len() > 1 => {
                Some(self.moved(CodeEditor::collapse_to_primary))
            }
            Key::A if ctrl => Some(self.moved(CodeEditor::select_all)),
            Key::D if ctrl => Some(self.moved(CodeEditor::add_next_occurrence)),
            Key::L if ctrl => Some(self.moved(CodeEditor::select_line)),
            Key::C if ctrl => Some(CodeViewEvent::Copy(self.editor.copy())),
            Key::X if ctrl => {
                let cut = self.editor.cut();
                self.after_edit();
                Some(CodeViewEvent::Cut(cut))
            }
            Key::V if ctrl => Some(CodeViewEvent::Paste),
            Key::Z if ctrl && shift => Some(self.history(CodeEditor::redo)),
            Key::Z if ctrl => Some(self.history(CodeEditor::undo)),
            Key::Y if ctrl => Some(self.history(CodeEditor::redo)),
            Key::Z if m.alt && !m.ctrl && shift => Some(self.history(CodeEditor::later)),
            Key::Z if m.alt && !m.ctrl => Some(self.history(CodeEditor::earlier)),
            _ if !(m.ctrl || m.alt || m.super_key) && key.types_text() => {
                let typed: String = key.typed().collect();
                self.editor.type_text(&typed);
                self.after_edit();
                Some(CodeViewEvent::Changed)
            }
            _ => None,
        };
        if !matches!(key.key, Key::Up | Key::Down | Key::PageUp | Key::PageDown) {
            self.goal_x = None;
        }
        event
    }

    fn moved(&mut self, act: impl FnOnce(&mut CodeEditor)) -> CodeViewEvent {
        act(&mut self.editor);
        self.reveal();
        CodeViewEvent::Moved
    }

    fn changed(&mut self, act: impl FnOnce(&mut CodeEditor)) -> CodeViewEvent {
        let before = self.editor.revision();
        act(&mut self.editor);
        self.after_edit();
        if self.editor.revision() == before {
            CodeViewEvent::Moved
        } else {
            CodeViewEvent::Changed
        }
    }

    fn history(&mut self, act: impl FnOnce(&mut CodeEditor) -> bool) -> CodeViewEvent {
        let moved = act(&mut self.editor);
        self.after_edit();
        if moved {
            CodeViewEvent::Changed
        } else {
            CodeViewEvent::Moved
        }
    }

    fn after_edit(&mut self) {
        self.goal_x = None;
        if self.find.open && self.find.revision != Some(self.editor.revision()) {
            self.refresh_matches();
        }
        self.reveal();
    }

    /// Up or Down `rows` rows: by line through the editor without wrapping,
    /// by row with it, keeping each caret's distance from the left.
    fn vertical(&mut self, rows: isize, extend: bool) -> CodeViewEvent {
        if !self.options.wrap {
            if rows < 0 {
                self.editor.move_up(rows.unsigned_abs(), extend);
            } else {
                self.editor.move_down(rows.unsigned_abs(), extend);
            }
            self.reveal();
            return CodeViewEvent::Moved;
        }
        let selections: Vec<Selection> = self.editor.selections().to_vec();
        let goals = self.goal_x.clone().unwrap_or_else(|| {
            selections
                .iter()
                .map(|s| {
                    let line = self.editor.buffer().line_of(s.head).unwrap_or(0);
                    let line_rows = self.rows_of(line);
                    let row = line_rows.get(row_index_of(&line_rows, s.head));
                    row.map_or(0.0, |r| self.x_in_row(r, s.head))
                })
                .collect()
        });
        let moved: Vec<Selection> = selections
            .iter()
            .zip(&goals)
            .map(|(s, &goal)| {
                let head = self.row_step(s.head, rows, goal);
                if extend {
                    Selection {
                        anchor: s.anchor,
                        head,
                    }
                } else {
                    Selection::caret(head)
                }
            })
            .collect();
        let count = moved.len();
        self.editor.set_selections(moved);
        self.goal_x = (self.editor.selections().len() == count).then_some(goals);
        self.reveal();
        CodeViewEvent::Moved
    }

    /// The offset `rows` visual rows from `offset`, at `goal` from the left;
    /// the start or the end of the text past either end.
    fn row_step(&self, offset: usize, rows: isize, goal: f32) -> usize {
        let buffer = self.editor.buffer();
        let mut line = buffer.line_of(offset).unwrap_or(0);
        let mut line_rows = self.rows_of(line);
        let mut row = row_index_of(&line_rows, offset);
        let mut left = rows.unsigned_abs();
        while left > 0 {
            if rows > 0 {
                if row.saturating_add(1) < line_rows.len() {
                    row = row.saturating_add(1);
                } else if line.saturating_add(1) < buffer.line_count() {
                    line = line.saturating_add(1);
                    line_rows = self.rows_of(line);
                    row = 0;
                } else {
                    return buffer.len();
                }
            } else if row > 0 {
                row = row.saturating_sub(1);
            } else if line > 0 {
                line = line.saturating_sub(1);
                line_rows = self.rows_of(line);
                row = line_rows.len().saturating_sub(1);
            } else {
                return 0;
            }
            left = left.saturating_sub(1);
        }
        line_rows
            .get(row)
            .map_or(offset, |r| self.offset_in_row(r, goal))
    }

    // ------------------------------------------------------------------
    // The pointer
    // ------------------------------------------------------------------

    /// Act on a pointer event in the host's space, with the modifiers held.
    /// `None` when it changed nothing.
    ///
    /// A press puts the caret there -- extending the selection with Shift,
    /// adding a caret with Ctrl, starting a block with Alt -- and a drag
    /// extends it; a double-click selects the word, and a drag after it goes
    /// by words; a press in the gutter selects the line. The wheel scrolls,
    /// and the scrollbar's column takes a press and a drag of its own.
    pub fn handle_mouse(
        &mut self,
        event: &MouseEvent,
        modifiers: Modifiers,
    ) -> Option<CodeViewEvent> {
        let (x, y) = (event.x, event.y);
        let geometry = self.scroll_geometry();
        let on_bar = geometry.is_some_and(|(track, _)| track.contains(x, y));
        match event.kind {
            MouseEventKind::Scroll { dy, .. } => {
                let rows = self.wheel.rows(dy);
                if rows == 0 {
                    return None;
                }
                self.scroll_rows(rows);
                Some(CodeViewEvent::Moved)
            }
            MouseEventKind::Leave => {
                let was = self.bar_hover;
                self.bar_hover = false;
                was.then_some(CodeViewEvent::Moved)
            }
            MouseEventKind::Move => {
                let hover_changed = self.bar_hover != on_bar;
                self.bar_hover = on_bar;
                self.drag_to(x, y)
                    .or_else(|| hover_changed.then_some(CodeViewEvent::Moved))
            }
            MouseEventKind::Release(MouseButton::Left) => {
                self.drag.take().map(|_| CodeViewEvent::Moved)
            }
            MouseEventKind::Press(MouseButton::Left) if self.bounds.contains(x, y) => {
                if self.find.open && y < self.rows_top() {
                    return Some(self.press_find_bar(x, y));
                }
                if let (true, Some((_, thumb))) = (on_bar, geometry) {
                    return Some(self.press_bar(thumb, y));
                }
                self.find.focused = false;
                Some(self.press_text(x, y, modifiers))
            }
            MouseEventKind::DoubleClick(MouseButton::Left)
                if self.bounds.contains(x, y) && !on_bar =>
            {
                let at = self.offset_at(x, y);
                self.editor.set_selections(vec![Selection::caret(at)]);
                self.editor.select_word();
                self.drag = Some(Drag::Words {
                    anchor: self.editor.primary(),
                });
                Some(CodeViewEvent::Moved)
            }
            _ => None,
        }
    }

    /// A press on the find bar: a switch flips, a field takes the keyboard.
    fn press_find_bar(&mut self, x: f32, y: f32) -> CodeViewEvent {
        let layout = self.find.layout(self.bounds);
        if let Some((switch, _)) = layout.switches.iter().find(|(_, r)| r.contains(x, y)) {
            self.find.flip(*switch);
            return self.act_on_bar(BarAction::Search);
        }
        if layout.find.contains(x, y) {
            self.find.focused = true;
            self.find.field = Field::Find;
        } else if layout.replace.is_some_and(|r| r.contains(x, y)) {
            self.find.focused = true;
            self.find.field = Field::Replace;
        }
        CodeViewEvent::Moved
    }

    fn press_bar(&mut self, thumb: Rect, y: f32) -> CodeViewEvent {
        if thumb.contains(thumb.x + 1.0, y) {
            self.drag = Some(Drag::Thumb { grab: y - thumb.y });
        } else {
            // A press in the groove pages towards it.
            let page = signed(self.visible_rows().saturating_sub(1).max(1));
            self.scroll_rows(if y < thumb.y {
                page.saturating_neg()
            } else {
                page
            });
        }
        CodeViewEvent::Moved
    }

    fn press_text(&mut self, x: f32, y: f32, modifiers: Modifiers) -> CodeViewEvent {
        let at = self.offset_at(x, y);
        let in_gutter = x < self.bounds.x + self.gutter_width();
        if in_gutter {
            self.editor.set_selections(vec![Selection::caret(at)]);
            self.editor.select_line();
            self.drag = None;
            return CodeViewEvent::Moved;
        }
        if modifiers.alt {
            self.editor.select_block(at, at);
            self.drag = Some(Drag::Block { anchor: at });
        } else if modifiers.ctrl {
            let mut all = self.editor.selections().to_vec();
            all.push(Selection::caret(at));
            self.editor.set_selections(all);
            self.drag = Some(Drag::Text);
        } else if modifiers.shift {
            let anchor = self.editor.primary().anchor;
            self.editor
                .set_selections(vec![Selection { anchor, head: at }]);
            self.drag = Some(Drag::Text);
        } else {
            self.editor.set_selections(vec![Selection::caret(at)]);
            self.drag = Some(Drag::Text);
        }
        self.goal_x = None;
        CodeViewEvent::Moved
    }

    /// Carry a drag to the point `(x, y)`. A text drag past the top or the
    /// bottom scrolls a row towards it each time the pointer moves, so a
    /// selection can be dragged out past the screen.
    fn drag_to(&mut self, x: f32, y: f32) -> Option<CodeViewEvent> {
        let drag = self.drag?;
        if !matches!(drag, Drag::Thumb { .. }) {
            if y < self.bounds.y {
                self.scroll_rows(-1);
            } else if y > self.bounds.bottom() {
                self.scroll_rows(1);
            }
        }
        match drag {
            Drag::Thumb { grab } => {
                let (track, thumb) = self.scroll_geometry()?;
                let total = self.editor.buffer().line_count();
                let first = scrollbar::first_from_drag(
                    track,
                    thumb.h,
                    grab,
                    y,
                    total,
                    self.visible_rows(),
                )?;
                self.top_line = first;
                self.top_row = 0;
            }
            Drag::Text => {
                let at = self.offset_at(x, y);
                let primary = self.editor.primary();
                let mut all = self.editor.selections().to_vec();
                // The primary's head moves; it goes last, which
                // `set_selections` keeps primary.
                if let Some(i) = all.iter().position(|s| *s == primary) {
                    let mut moved = all.remove(i);
                    moved.head = at;
                    all.push(moved);
                }
                self.editor.set_selections(all);
                self.reveal();
            }
            Drag::Words { anchor } => {
                let at = self.offset_at(x, y);
                let word = self.editor.word_at(at);
                let selection = if at < anchor.range().start {
                    Selection {
                        anchor: anchor.range().end,
                        head: word.start,
                    }
                } else {
                    Selection {
                        anchor: anchor.range().start,
                        head: word.end.max(anchor.range().end),
                    }
                };
                self.editor.set_selections(vec![selection]);
                self.reveal();
            }
            Drag::Block { anchor } => {
                let at = self.offset_at(x, y);
                self.editor.select_block(anchor, at);
                self.reveal();
            }
        }
        Some(CodeViewEvent::Moved)
    }
}

/// The row `offset` is on, among one line's `rows`: the last row starting at
/// or before it (a caret at a wrap point belongs to the row below, where
/// typing carries on).
fn row_index_of(rows: &[Row], offset: usize) -> usize {
    rows.iter()
        .rposition(|r| r.range.start <= offset)
        .unwrap_or(0)
}

/// A filled rectangle.
fn fill(r: Rect, color: Color) -> RenderCommand {
    RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::ZERO,
    }
}

/// How many decimal digits `n` has.
fn digits(n: usize) -> usize {
    n.checked_ilog10()
        .and_then(|d| usize::try_from(d).ok())
        .map_or(1, |d| d.saturating_add(1))
}

fn signed(n: usize) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;

    const BOUNDS: Rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 600.0,
        h: 300.0,
    };

    fn view(text: &str) -> CodeView {
        let mut v = CodeView::new(CodeEditor::from_text(text));
        v.set_bounds(BOUNDS);
        v.set_focused(true);
        v
    }

    fn drawn(v: &CodeView) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        v.draw(&mut cmds, &Palette::for_mode(false));
        cmds
    }

    fn key(k: Key, ctrl: bool, shift: bool) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers {
                shift,
                ctrl,
                ..Modifiers::NONE
            },
            text: String::new(),
        }
    }

    fn typed(s: &str) -> KeyEvent {
        KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: s.to_owned(),
        }
    }

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> MouseEvent {
        MouseEvent { x, y, kind }
    }

    /// The point on screen where `offset` is drawn: its row's middle, its
    /// character's left.
    fn point_of(v: &CodeView, offset: usize) -> (f32, f32) {
        let rows = v.screen_rows();
        let row = v.row_on_screen(&rows, offset).expect("on screen");
        let i = rows.iter().position(|r| r == row).unwrap();
        let text = v.text_rect();
        (
            text.x - v.scroll_x + v.x_in_row(row, offset) + 0.5,
            v.rows_top() + (i as f32 + 0.5) * v.line_height(),
        )
    }

    /// The rows' texts, in order.
    fn rows_drawn(cmds: &[RenderCommand]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::RichText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn texts(cmds: &[RenderCommand]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } | RenderCommand::RichText { text, .. } => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// **The gutter numbers the lines on screen, the caret's line in the
    /// text's ink and the rest in the secondary one; the text is in the
    /// fixed-pitch face.** Without line numbers, no gutter.
    #[test]
    fn the_gutter_numbers_the_lines_on_screen() {
        let p = Palette::for_mode(false);
        let mut v = view("one\ntwo\nthree");
        v.editor_mut().set_selections(vec![Selection::caret(5)]);
        let cmds = drawn(&v);
        let numbers: Vec<(String, Color)> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } => Some((text.clone(), *color)),
                _ => None,
            })
            .collect();
        assert_eq!(
            numbers,
            [
                ("1".to_owned(), p.subtext0),
                ("2".to_owned(), p.text),
                ("3".to_owned(), p.subtext0)
            ]
        );
        assert!(cmds.iter().any(|c| matches!(
            c,
            RenderCommand::PushFont {
                family: FontFamily::Mono
            }
        )));
        assert_eq!(rows_drawn(&cmds), ["one", "two", "three"]);

        v.set_options(ViewOptions {
            line_numbers: false,
            ..ViewOptions::default()
        });
        assert_eq!(texts(&drawn(&v)), ["one", "two", "three"]);
    }

    /// **A selection is drawn in the accent, and the text on it in the ink
    /// that reads on the accent** -- a span of the row's text, not a hope.
    #[test]
    fn selected_text_is_drawn_in_the_accents_own_ink() {
        let p = Palette::for_mode(false);
        let mut v = view("abcdef");
        v.editor_mut()
            .set_selections(vec![Selection { anchor: 1, head: 4 }]);
        let cmds = drawn(&v);
        let row = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::RichText { text, spans, .. } if text == "abcdef" => {
                    Some(spans.clone())
                }
                _ => None,
            })
            .expect("the row");
        assert!(
            row.iter().any(|s| s.end == 4 && s.color == p.on_accent()),
            "{row:?}"
        );
        assert!(
            row.iter().any(|s| s.end == 1 && s.color == p.text),
            "{row:?}"
        );
        assert!(cmds.iter().any(|c| matches!(c,
            RenderCommand::FillRect { color, .. } if *color == p.accent)));
    }

    /// **Carets are drawn only while the view has the keyboard**, one per
    /// caret.
    #[test]
    fn carets_are_drawn_while_focused() {
        let p = Palette::for_mode(false);
        let mut v = view("ab\ncd");
        v.editor_mut()
            .set_selections(vec![Selection::caret(1), Selection::caret(4)]);
        let bars = |v: &CodeView| {
            drawn(v)
                .iter()
                .filter(|c| {
                    matches!(c,
                    RenderCommand::FillRect { width, color, .. }
                        if *color == p.text && *width == DEFAULT_CARET_WIDTH)
                })
                .count()
        };
        assert_eq!(bars(&v), 2);
        v.set_focused(false);
        assert_eq!(bars(&v), 0);
    }

    /// **The bracket at the caret and its partner are outlined.**
    #[test]
    fn the_bracket_pair_at_the_caret_is_outlined() {
        let mut v = view("f(x)");
        v.editor_mut().set_selections(vec![Selection::caret(1)]);
        let boxes = drawn(&v)
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert_eq!(boxes, 2);
    }

    /// **A tab is drawn as spaces to the next stop**, so what is drawn is
    /// what was measured.
    #[test]
    fn a_tab_is_drawn_as_spaces_to_the_next_stop() {
        let v = view("ab\tc");
        assert!(
            texts(&drawn(&v)).contains(&"ab  c".to_owned()),
            "{:?}",
            texts(&drawn(&v))
        );
    }

    /// **A press puts the caret where it was aimed; Shift extends, Ctrl adds
    /// a caret, a drag extends, a double-click selects the word, the gutter
    /// selects the line.**
    #[test]
    fn the_pointer_places_extends_adds_and_selects() {
        let mut v = view("hello world\nsecond line");
        let (x, y) = point_of(&v, 6);
        v.handle_mouse(
            &mouse(x, y, MouseEventKind::Press(MouseButton::Left)),
            Modifiers::NONE,
        );
        assert_eq!(v.editor().primary(), Selection::caret(6));
        let (x2, y2) = point_of(&v, 9);
        v.handle_mouse(&mouse(x2, y2, MouseEventKind::Move), Modifiers::NONE);
        v.handle_mouse(
            &mouse(x2, y2, MouseEventKind::Release(MouseButton::Left)),
            Modifiers::NONE,
        );
        assert_eq!(v.editor().primary(), Selection { anchor: 6, head: 9 });

        let (x3, y3) = point_of(&v, 2);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        v.handle_mouse(
            &mouse(x3, y3, MouseEventKind::Press(MouseButton::Left)),
            shift,
        );
        v.handle_mouse(
            &mouse(x3, y3, MouseEventKind::Release(MouseButton::Left)),
            shift,
        );
        assert_eq!(v.editor().primary(), Selection { anchor: 6, head: 2 });

        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        let (x4, y4) = point_of(&v, 14);
        v.handle_mouse(
            &mouse(x4, y4, MouseEventKind::Press(MouseButton::Left)),
            ctrl,
        );
        assert_eq!(v.editor().selections().len(), 2);

        let (x5, y5) = point_of(&v, 7);
        v.handle_mouse(
            &mouse(x5, y5, MouseEventKind::DoubleClick(MouseButton::Left)),
            Modifiers::NONE,
        );
        assert_eq!(
            v.editor().primary(),
            Selection {
                anchor: 6,
                head: 11
            }
        );

        v.handle_mouse(
            &mouse(BOUNDS.x + 2.0, y4, MouseEventKind::Press(MouseButton::Left)),
            Modifiers::NONE,
        );
        assert_eq!(
            v.editor().primary().range(),
            12..23,
            "the gutter selects the line"
        );
    }

    /// **Alt and a drag select a block.**
    #[test]
    fn alt_and_a_drag_select_a_block() {
        let mut v = view("abcdef\nabcdef\nabcdef");
        let alt = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        let (x, y) = point_of(&v, 1);
        v.handle_mouse(&mouse(x, y, MouseEventKind::Press(MouseButton::Left)), alt);
        let (x2, y2) = point_of(&v, 18);
        v.handle_mouse(&mouse(x2, y2, MouseEventKind::Move), alt);
        let ranges: Vec<Range<usize>> = v
            .editor()
            .selections()
            .iter()
            .map(Selection::range)
            .collect();
        assert_eq!(ranges, [1..4, 8..11, 15..18]);
    }

    /// **Moving past the bottom scrolls the caret into view; the wheel
    /// scrolls the view and leaves the caret.**
    #[test]
    fn the_view_follows_the_caret_and_the_wheel() {
        let mut text = String::new();
        for i in 0..200 {
            text.push_str("line ");
            text.push_str(&i.to_string());
            text.push('\n');
        }
        let mut v = view(&text);
        let rows = v.visible_rows();
        for _ in 0..rows + 5 {
            v.handle_key(&key(Key::Down, false, false));
        }
        let caret_line = v
            .editor()
            .buffer()
            .line_of(v.editor().primary().head)
            .unwrap();
        assert!(v.top_line() > 0);
        assert!(caret_line >= v.top_line() && caret_line < v.top_line() + rows);

        let before = v.editor().primary();
        let top = v.top_line();
        v.handle_mouse(
            &mouse(100.0, 100.0, MouseEventKind::Scroll { dx: 0.0, dy: -3.0 }),
            Modifiers::NONE,
        );
        assert!(v.top_line() > top, "the wheel scrolled down");
        assert_eq!(v.editor().primary(), before, "and left the caret");
    }

    /// **Dragging the scrollbar's thumb scrolls; the pointer over it lights
    /// it.**
    #[test]
    fn the_scrollbar_can_be_dragged() {
        let mut text = String::new();
        for i in 0..500 {
            text.push_str(&i.to_string());
            text.push('\n');
        }
        let mut v = view(&text);
        let (track, thumb) = v.scroll_geometry().expect("a long text has a scrollbar");
        let (tx, ty) = (thumb.x + thumb.w / 2.0, thumb.y + 2.0);
        v.handle_mouse(&mouse(tx, ty, MouseEventKind::Move), Modifiers::NONE);
        assert!(v.bar_hover);
        v.handle_mouse(
            &mouse(tx, ty, MouseEventKind::Press(MouseButton::Left)),
            Modifiers::NONE,
        );
        v.handle_mouse(
            &mouse(tx, track.bottom(), MouseEventKind::Move),
            Modifiers::NONE,
        );
        assert!(v.top_line() > 400, "dragged to the end: {}", v.top_line());
        v.handle_mouse(
            &mouse(
                tx,
                track.bottom(),
                MouseEventKind::Release(MouseButton::Left),
            ),
            Modifiers::NONE,
        );
        assert_eq!(v.drag, None);
    }

    /// **Without wrapping, a long line scrolls sideways to keep the caret in
    /// sight.**
    #[test]
    fn a_long_line_scrolls_sideways() {
        let mut v = view(&"x".repeat(500));
        v.handle_key(&key(Key::End, false, false));
        assert!(v.scroll_x > 0.0);
        let (x, _) = point_of(&v, 500);
        assert!(
            x <= v.text_rect().right(),
            "the caret at {x} is off the view"
        );
        v.handle_key(&key(Key::Home, false, false));
        assert_eq!(v.scroll_x, 0.0);
    }

    /// **With wrapping, a long line is several rows -- broken after a blank
    /// near the edge -- and Up and Down move by row, keeping the caret's
    /// distance from the left.**
    #[test]
    fn wrapped_lines_are_rows_and_up_and_down_move_by_row() {
        let word = "abcdefghi ";
        let long: String = word.repeat(200);
        let mut v = view(&long);
        v.set_options(ViewOptions {
            wrap: true,
            ..ViewOptions::default()
        });
        let rows = v.rows_of(0);
        assert!(rows.len() > 2, "{} rows", rows.len());
        for row in &rows[..rows.len() - 1] {
            assert!(
                long[row.range.clone()].ends_with(' '),
                "a row did not break after a blank: {:?}",
                &long[row.range.clone()]
            );
        }
        // Rows tile the line.
        assert_eq!(rows[0].range.start, 0);
        for pair in rows.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start);
        }
        let start_x = v.x_in_row(&rows[0], 3);
        v.editor_mut().set_selections(vec![Selection::caret(3)]);
        v.handle_key(&key(Key::Down, false, false));
        let head = v.editor().primary().head;
        assert!(
            rows[1].range.contains(&head),
            "down went to {head}, not the second row"
        );
        let x = v.x_in_row(&rows[1], head);
        assert!((x - start_x).abs() <= v.cell(), "{x} against {start_x}");
        v.handle_key(&key(Key::Up, false, false));
        assert_eq!(v.editor().primary().head, 3);
    }

    /// **The keys: typing, a new line, undo, copy, cut, paste, the next
    /// occurrence, Escape.**
    #[test]
    fn the_keys_drive_the_editor() {
        let mut v = view("ab");
        v.editor_mut().set_selections(vec![Selection::caret(2)]);
        assert_eq!(v.handle_key(&typed("c")), Some(CodeViewEvent::Changed));
        assert_eq!(
            v.handle_key(&key(Key::Enter, false, false)),
            Some(CodeViewEvent::Changed)
        );
        assert_eq!(v.editor().text(), "abc\n");
        v.handle_key(&key(Key::Z, true, false));
        assert_eq!(v.editor().text(), "abc");
        v.handle_key(&key(Key::A, true, false));
        assert_eq!(
            v.handle_key(&key(Key::C, true, false)),
            Some(CodeViewEvent::Copy("abc".to_owned()))
        );
        assert_eq!(
            v.handle_key(&key(Key::X, true, false)),
            Some(CodeViewEvent::Cut("abc".to_owned()))
        );
        assert_eq!(v.editor().text(), "");
        assert_eq!(
            v.handle_key(&key(Key::V, true, false)),
            Some(CodeViewEvent::Paste)
        );
        v.paste("x y x");
        assert_eq!(v.editor().text(), "x y x");
        v.editor_mut().set_selections(vec![Selection::caret(0)]);
        v.handle_key(&key(Key::D, true, false));
        v.handle_key(&key(Key::D, true, false));
        assert_eq!(v.editor().selections().len(), 2);
        v.handle_key(&key(Key::Escape, false, false));
        assert_eq!(v.editor().selections().len(), 1);
        // A key the view does not use is not claimed.
        assert_eq!(v.handle_key(&key(Key::F5, false, false)), None);
    }

    fn alt(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            text: String::new(),
        }
    }

    fn type_into(v: &mut CodeView, text: &str) {
        for c in text.chars() {
            v.handle_key(&typed(&c.to_string()));
        }
    }

    /// The text of every `Text` command, for the bar's status.
    fn status_of(cmds: &[RenderCommand]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// **Ctrl+F opens the bar on the selected text; typing searches as it
    /// goes, from where the caret was; the bar says which match of how
    /// many.** The rows start below the bar, and again at the top once
    /// Escape closes it.
    #[test]
    fn the_find_bar_searches_as_it_is_typed() {
        let mut v = view("alpha beta\nbeta gamma\nalphabet");
        v.editor_mut()
            .set_selections(vec![Selection { anchor: 0, head: 5 }]);
        v.handle_key(&key(Key::F, true, false));
        assert!(v.find_open());
        assert_eq!(v.find.find.text(), "alpha");
        assert!(v.rows_top() > BOUNDS.y, "the rows start below the bar");

        // Typing replaces the selected field text.
        type_into(&mut v, "bet");
        assert_eq!(v.find.find.text(), "bet");
        assert_eq!(
            v.editor().primary().range(),
            6..9,
            "the first match from the caret"
        );
        assert_eq!(v.find.matches.len(), 3);
        assert!(
            status_of(&drawn(&v)).contains(&"1 of 3".to_owned()),
            "{:?}",
            status_of(&drawn(&v))
        );

        v.handle_key(&key(Key::Enter, false, false));
        assert_eq!(v.editor().primary().range(), 11..14);
        v.handle_key(&key(Key::Enter, false, true));
        assert_eq!(v.editor().primary().range(), 6..9, "Shift+Enter goes back");

        v.handle_key(&key(Key::Escape, false, false));
        assert!(!v.find_open());
        assert_eq!(v.rows_top(), BOUNDS.y);

        // From a caret past the first two, the search starts there.
        v.editor_mut().set_selections(vec![Selection::caret(12)]);
        v.handle_key(&key(Key::F, true, false));
        v.handle_key(&key(Key::Backspace, false, false));
        type_into(&mut v, "bet");
        assert_eq!(
            v.editor().primary().range(),
            27..30,
            "from the caret, not the top"
        );
    }

    /// **Alt+Enter selects every match as a caret and hands the keyboard
    /// back, so typing edits them all.**
    #[test]
    fn alt_enter_makes_every_match_a_caret() {
        let mut v = view("x = f(x) + x");
        v.handle_key(&key(Key::F, true, false));
        type_into(&mut v, "x");
        v.handle_key(&alt(Key::W));
        v.handle_key(&KeyEvent {
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            ..key(Key::Enter, false, false)
        });
        assert!(!v.find.focused, "the keyboard went back to the text");
        type_into(&mut v, "y");
        assert_eq!(v.editor().text(), "y = f(y) + y");
    }

    /// **The switches search again; a pattern that is not one says why, in
    /// red, with the field marked wrong.**
    #[test]
    fn the_switches_search_again_and_a_bad_pattern_says_why() {
        let p = Palette::for_mode(false);
        let mut v = view("Cat cat cat9");
        v.handle_key(&key(Key::F, true, false));
        type_into(&mut v, "cat");
        assert_eq!(v.find.matches.len(), 3);
        v.handle_key(&alt(Key::C));
        assert_eq!(v.find.matches.len(), 2, "case-sensitive");
        v.handle_key(&alt(Key::W));
        assert_eq!(v.find.matches.len(), 1, "and whole words");
        v.handle_key(&alt(Key::R));
        type_into(&mut v, "(");
        assert!(v.find.error.is_some());
        let cmds = drawn(&v);
        assert!(cmds.iter().any(|c| matches!(c,
            RenderCommand::Text { text, color, .. }
                if text.contains("not a regular expression") && *color == p.ink(p.red))));
        assert!(
            cmds.iter().any(|c| matches!(c,
                RenderCommand::StrokeRect { color, .. } if *color == p.red)),
            "the field is not marked wrong"
        );
    }

    /// **Replace takes the selected match and moves on; replace-all is one
    /// undo step.**
    #[test]
    fn replace_and_replace_all_from_the_bar() {
        let mut v = view("a1 a2 a3");
        v.handle_key(&key(Key::H, true, false));
        type_into(&mut v, "a");
        assert_eq!(v.editor().primary().range(), 0..1);
        v.handle_key(&key(Key::Tab, false, false));
        type_into(&mut v, "b");
        assert_eq!(
            v.handle_key(&key(Key::Enter, false, false)),
            Some(CodeViewEvent::Changed)
        );
        assert_eq!(v.editor().text(), "b1 a2 a3");
        assert_eq!(v.editor().primary().range(), 3..4, "on to the next");
        v.handle_key(&KeyEvent {
            modifiers: Modifiers {
                ctrl: true,
                alt: true,
                ..Modifiers::NONE
            },
            ..key(Key::Enter, false, false)
        });
        assert_eq!(v.editor().text(), "b1 b2 b3");
        v.handle_key(&key(Key::Escape, false, false));
        v.handle_key(&key(Key::Z, true, false));
        assert_eq!(v.editor().text(), "b1 a2 a3", "replace-all was one step");
    }

    /// **Every match on screen is outlined in yellow's ink.**
    #[test]
    fn matches_on_screen_are_outlined() {
        let p = Palette::for_mode(false);
        let mut v = view("ab ab ab");
        v.handle_key(&key(Key::F, true, false));
        type_into(&mut v, "ab");
        let outlines = drawn(&v)
            .iter()
            .filter(|c| {
                matches!(c,
                RenderCommand::StrokeRect { color, .. } if *color == p.ink(p.yellow))
            })
            .count();
        assert_eq!(outlines, 3);
    }

    /// **A click on a switch flips it; a click in the text takes the
    /// keyboard back; F3 from the text steps on.**
    #[test]
    fn the_bar_answers_the_pointer_and_f3() {
        let mut v = view("one two one");
        v.handle_key(&key(Key::F, true, false));
        type_into(&mut v, "one");
        let layout = v.find.layout(BOUNDS);
        let (_, case) = layout.switches[0];
        v.handle_mouse(
            &mouse(
                case.x + 2.0,
                case.y + 2.0,
                MouseEventKind::Press(MouseButton::Left),
            ),
            Modifiers::NONE,
        );
        assert!(v.find.case_sensitive);

        let (x, y) = point_of(&v, 5);
        v.handle_mouse(
            &mouse(x, y, MouseEventKind::Press(MouseButton::Left)),
            Modifiers::NONE,
        );
        assert!(!v.find.focused);
        v.handle_key(&key(Key::F3, false, false));
        assert_eq!(v.editor().primary().range(), 8..11);
    }

    /// Digits of a line count.
    #[test]
    fn digits_count() {
        assert_eq!(digits(0), 1);
        assert_eq!(digits(9), 1);
        assert_eq!(digits(10), 2);
        assert_eq!(digits(12_345), 5);
    }
}

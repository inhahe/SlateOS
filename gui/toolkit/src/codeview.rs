//! A code editor, drawn and driven: [`CodeView`] puts a
//! [`CodeEditor`] on screen -- a gutter of line
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
//! Ctrl+F and Ctrl+H open the find bar across the top (`findbar`); the
//! rows start below it. F3 and Shift+F3 step through the matches from the
//! text as well.
//!
//! # The clipboard
//!
//! The view does not own one: Ctrl+C and Ctrl+X hand the text to the host
//! ([`CodeViewEvent::Copy`], [`CodeViewEvent::Cut`]) and Ctrl+V asks the host
//! for it ([`CodeViewEvent::Paste`]), which answers with
//! [`CodeView::paste`] -- the system clipboard is the host's to reach.
//!
//! # Colouring the code
//!
//! A [`Highlighter`] set with [`CodeView::set_highlighter`] colours the code
//! (`gui/syntax` has the tree-sitter one). The view tells it about every
//! change as the buffer journalled it, gives it a few milliseconds after each
//! edit -- an incremental re-parse's worth -- and asks for what is on screen
//! each time it draws, in the theme's colours for code
//! ([`Palette::syntax_ink`]); selected text keeps the selection's ink. Work a
//! slice cannot finish -- the first parse of a large file -- waits for the
//! host: while [`CodeView::has_work`], it calls [`CodeView::work`] and draws
//! again, and the colours arrive a frame at a time rather than the window
//! stopping until they do.

use core::ops::Range;
use core::time::Duration;

use crate::codeedit::{CodeEditor, Selection};
use crate::color::Color;
use crate::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::highlight::{Brackets, HighlightSpan, Highlighter};
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
/// How long a highlighter may work right after an edit before the view
/// returns to its host: an incremental re-parse of any ordinary edit, and
/// short enough that typing never waits on it.
const EDIT_BUDGET: Duration = Duration::from_millis(4);
/// How long it may work each time the host calls [`CodeView::work`]: half a
/// frame at 60 Hz.
const WORK_BUDGET: Duration = Duration::from_millis(8);

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
    /// What colours the code, if anything does.
    highlighter: Option<Box<dyn Highlighter>>,
    /// The revision of the text the highlighter was last told about: until
    /// it is the editor's, what it answers is for another text and is not
    /// drawn.
    highlighted: Option<u64>,
    /// Whether the highlighter has work it has not finished.
    highlight_pending: bool,
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
            highlighter: None,
            highlighted: None,
            highlight_pending: false,
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

    /// Colour the code with `highlighter` from now on -- or stop colouring
    /// it, with `None`. The highlighter starts on the text at once, for a
    /// few milliseconds; see [`work`](Self::work) for the rest.
    pub fn set_highlighter(&mut self, highlighter: Option<Box<dyn Highlighter>>) {
        self.highlighter = highlighter;
        self.highlighted = None;
        self.highlight_pending = false;
        self.sync_highlighter(EDIT_BUDGET);
    }

    /// Whether a highlighter colours the code.
    #[must_use]
    pub fn has_highlighter(&self) -> bool {
        self.highlighter.is_some()
    }

    /// Give the highlighter half a frame, and answer whether it still has
    /// work: while [`has_work`](Self::has_work), a host calls this and draws
    /// again. Changes made through [`editor_mut`](Self::editor_mut) reach the
    /// highlighter here too.
    pub fn work(&mut self) -> bool {
        self.sync_highlighter(WORK_BUDGET);
        self.highlight_pending
    }

    /// Whether [`work`](Self::work) has anything to do: the highlighter has
    /// not finished, has not yet been told of a change, or found more to do
    /// while the view drew ([`Highlighter::has_work`]).
    #[must_use]
    pub fn has_work(&self) -> bool {
        self.highlighter.as_ref().is_some_and(|h| {
            self.highlight_pending
                || self.highlighted != Some(self.editor.revision())
                || h.has_work()
        })
    }

    /// Tell the highlighter about every change since it was last told -- or,
    /// when that cannot be said, start it over on the text -- and let it work
    /// for `budget`.
    fn sync_highlighter(&mut self, budget: Duration) {
        let Some(highlighter) = self.highlighter.as_mut() else {
            return;
        };
        let changes = self.editor.take_changes();
        let buffer = self.editor.buffer();
        match (self.highlighted, changes.splices) {
            (Some(seen), Some(splices)) if seen == changes.since => {
                if !splices.is_empty() {
                    highlighter.edited(buffer, &splices);
                }
            }
            // Never told of any text, changes it cannot be told (the journal
            // gave up), or a journal that does not start where it left off --
            // the editor was replaced, or someone else took the journal.
            _ => highlighter.reset(buffer),
        }
        self.highlighted = Some(buffer.revision());
        self.highlight_pending = highlighter.work(buffer, budget);
    }

    /// The highlights over the rows on screen -- none while the highlighter
    /// has not been told of the text as it is, since what it would answer is
    /// for another text.
    fn visible_highlights(&self, rows: &[Row]) -> Vec<HighlightSpan> {
        let (Some(highlighter), Some(first), Some(last)) =
            (self.highlighter.as_ref(), rows.first(), rows.last())
        else {
            return Vec::new();
        };
        if self.highlighted != Some(self.editor.revision()) {
            return Vec::new();
        }
        highlighter.highlights(self.editor.buffer(), first.range.start..last.range.end)
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
        let highlights = self.visible_highlights(&rows);
        let inks = p.syntax_inks();
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
            self.draw_row(sink, p, row, y, text, (&highlights, &inks));
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
    /// coloured as the highlighter says, the selected part in the ink that
    /// reads on the selection.
    fn draw_row(
        &self,
        sink: &mut impl CommandSink,
        p: &Palette,
        row: &Row,
        y: f32,
        text: Rect,
        (highlights, inks): (
            &[HighlightSpan],
            &[Color; crate::highlight::Highlight::COUNT],
        ),
    ) {
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
        // `map` ascends in both halves, so a binary search finds where an
        // offset went: a row can be a whole minified file.
        let shown_at = |offset: usize| {
            let i = map.partition_point(|(at, _)| *at < offset);
            map.get(i).map_or(shown.len(), |(_, s)| *s)
        };
        let left = text.x - self.scroll_x;
        // What the highlighter says each stretch of the row is, in the row
        // as drawn. The spans are sorted and apart, and so are these.
        let first = highlights.partition_point(|h| h.range.end <= row.range.start);
        let colored: Vec<(Range<usize>, Color)> = highlights
            .get(first..)
            .unwrap_or_default()
            .iter()
            .take_while(|h| h.range.start < row.range.end)
            .filter_map(|h| {
                let from = shown_at(h.range.start.max(row.range.start));
                let to = shown_at(h.range.end.min(row.range.end));
                let ink = inks.get(h.highlight.index()).copied()?;
                (to > from).then_some((from..to, ink))
            })
            .collect();
        let mut selected: Vec<Range<usize>> = Vec::new();
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
                selected.push(a..b);
            }
        }
        if shown.is_empty() {
            return;
        }
        let spans = color_runs(&colored, &selected, p.on_accent(), p.text);
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

    /// The bracket at the primary caret and its partner, outlined -- paired
    /// as the highlighter reads the language, where it can say (a bracket
    /// in a string or a comment is none), else by counting.
    fn draw_bracket_pair(
        &self,
        sink: &mut impl CommandSink,
        p: &Palette,
        rows: &[Row],
        text: Rect,
    ) {
        let head = self.editor.primary().head;
        let said = self.highlighter.as_ref().map_or(Brackets::Unknown, |h| {
            h.brackets(self.editor.buffer(), head)
        });
        let pair = match said {
            Brackets::Pair(a, b) => Some((a, b)),
            Brackets::Unpaired => None,
            Brackets::Unknown => self.editor.matching_bracket(head),
        };
        let Some((a, b)) = pair else {
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
        let ctrl = m.is_ctrl_chord();
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
        self.sync_highlighter(EDIT_BUDGET);
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
/// The colour runs of a row: `selected` stretches in `selected_ink`, the rest
/// of the `colored` stretches in theirs, and everything else in `ink`. Both
/// lists are sorted and none of their stretches overlap another in the same
/// list. Linear in the two lists, whatever the row's length.
fn color_runs(
    colored: &[(Range<usize>, Color)],
    selected: &[Range<usize>],
    selected_ink: Color,
    ink: Color,
) -> Vec<TextSpan> {
    let mut edges: Vec<usize> = colored
        .iter()
        .flat_map(|(r, _)| [r.start, r.end])
        .chain(selected.iter().flat_map(|r| [r.start, r.end]))
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut spans: Vec<TextSpan> = Vec::new();
    let (mut c, mut s) = (0, 0);
    let mut from = 0;
    for &to in &edges {
        if to > from {
            // What covers `from..to`: the first stretch of each list that
            // does not end before it.
            while colored.get(c).is_some_and(|(r, _)| r.end <= from) {
                c = c.saturating_add(1);
            }
            while selected.get(s).is_some_and(|r| r.end <= from) {
                s = s.saturating_add(1);
            }
            let color = if selected.get(s).is_some_and(|r| r.start <= from) {
                selected_ink
            } else {
                colored
                    .get(c)
                    .filter(|(r, _)| r.start <= from)
                    .map_or(ink, |(_, color)| *color)
            };
            let end = u32::try_from(to).unwrap_or(u32::MAX);
            match spans.last_mut() {
                Some(last) if last.color == color => last.end = end,
                _ => spans.push(TextSpan { end, color }),
            }
        }
        from = to;
    }
    spans
}

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

    /// A highlighter for tests: `fn` is a keyword and a run of digits a
    /// number, found by reading the text afresh. It writes down what it is
    /// told, where the test can read it, and takes `slices` calls to `work`
    /// to finish after each change -- answering nothing until it has.
    #[derive(Debug)]
    struct Words {
        log: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
        slices: usize,
        left: usize,
    }

    impl Words {
        fn boxed(
            slices: usize,
        ) -> (
            Box<dyn Highlighter>,
            std::rc::Rc<std::cell::RefCell<Vec<String>>>,
        ) {
            let log = std::rc::Rc::default();
            let words = Self {
                log: std::rc::Rc::clone(&log),
                slices,
                left: 0,
            };
            (Box::new(words), log)
        }
    }

    impl Highlighter for Words {
        fn reset(&mut self, _text: &crate::textbuffer::TextBuffer) {
            self.log.borrow_mut().push("reset".to_owned());
            self.left = self.slices;
        }

        fn edited(
            &mut self,
            _text: &crate::textbuffer::TextBuffer,
            splices: &[crate::textbuffer::Splice],
        ) {
            for s in splices {
                self.log
                    .borrow_mut()
                    .push(format!("edited {}..{}->{}", s.start, s.old_end, s.new_end));
            }
            self.left = self.slices;
        }

        fn work(&mut self, _text: &crate::textbuffer::TextBuffer, _budget: Duration) -> bool {
            self.left = self.left.saturating_sub(1);
            self.left > 0
        }

        fn highlights(
            &self,
            text: &crate::textbuffer::TextBuffer,
            range: Range<usize>,
        ) -> Vec<HighlightSpan> {
            use crate::highlight::Highlight;
            if self.left > 0 {
                return Vec::new();
            }
            let all = text.text();
            let mut out = Vec::new();
            let bytes = all.as_bytes();
            let mut i = range.start;
            while i < range.end {
                if all[i..].starts_with("fn") && i + 2 <= range.end {
                    out.push(HighlightSpan {
                        range: i..i + 2,
                        highlight: Highlight::Keyword,
                    });
                    i += 2;
                } else if bytes[i].is_ascii_digit() {
                    let start = i;
                    while i < range.end && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    out.push(HighlightSpan {
                        range: start..i,
                        highlight: Highlight::Number,
                    });
                } else {
                    i += 1;
                }
            }
            out
        }
    }

    /// The colour runs of the row drawn as `text`.
    fn row_spans(cmds: &[RenderCommand], text: &str) -> Vec<TextSpan> {
        cmds.iter()
            .find_map(|c| match c {
                RenderCommand::RichText { text: t, spans, .. } if t == text => Some(spans.clone()),
                _ => None,
            })
            .expect("the row")
    }

    /// **A highlighter colours the code in the theme's inks for code, and
    /// the selected part keeps the selection's ink.**
    #[test]
    fn a_highlighter_colours_the_code_and_selected_text_keeps_its_ink() {
        use crate::highlight::Highlight;
        let p = Palette::for_mode(false);
        let (keyword, number) = (
            p.syntax_ink(Highlight::Keyword),
            p.syntax_ink(Highlight::Number),
        );
        assert_ne!(keyword, number);
        let mut v = view("fn a12() {}\nx");
        let (words, _) = Words::boxed(1);
        v.set_highlighter(Some(words));
        assert!(v.has_highlighter());
        let spans = row_spans(&drawn(&v), "fn a12() {}");
        assert_eq!(
            spans,
            vec![
                TextSpan {
                    end: 2,
                    color: keyword
                },
                TextSpan {
                    end: 4,
                    color: p.text
                },
                TextSpan {
                    end: 6,
                    color: number
                },
            ]
        );
        // Selected: the selection's ink over the keyword's first letter.
        v.editor_mut()
            .set_selections(vec![Selection { anchor: 0, head: 1 }]);
        let spans = row_spans(&drawn(&v), "fn a12() {}");
        assert_eq!(spans[0].end, 1);
        assert_eq!(spans[0].color, p.on_accent());
        assert_eq!((spans[1].end, spans[1].color), (2, keyword));
        // Without one, the text's own ink throughout.
        v.set_highlighter(None);
        assert!(!v.has_highlighter() && !v.has_work());
        v.editor_mut().set_selections(vec![Selection::caret(0)]);
        assert!(row_spans(&drawn(&v), "fn a12() {}").is_empty());
    }

    /// **Every change reaches the highlighter, in the order it was made** --
    /// typing, a batch at two carets (last first, as the buffer made it),
    /// undo -- and nothing reaches it twice.
    #[test]
    fn every_change_reaches_the_highlighter_in_order() {
        let mut v = view("ab\ncd");
        let (words, log) = Words::boxed(1);
        v.set_highlighter(Some(words));
        v.handle_key(&typed("x"));
        // Told at once: a keystroke's re-parse happens before the view
        // returns, not on the host's next call.
        assert!(!v.has_work(), "the edit waited for the host");
        assert_eq!(log.borrow().len(), 2);
        v.editor_mut()
            .set_selections(vec![Selection::caret(1), Selection::caret(4)]);
        v.handle_key(&typed("y"));
        v.handle_key(&key(Key::Z, true, false));
        v.work();
        assert_eq!(
            *log.borrow(),
            [
                "reset",
                "edited 0..0->1",
                "edited 4..4->5",
                "edited 1..1->2",
                "edited 5..6->5",
                "edited 1..2->1",
            ]
        );
    }

    /// **A change the highlighter has not heard of is drawn plain**, not in
    /// colours worked out for the text before it -- and reaches it, as a
    /// change and not a start over, the next time the view works.
    #[test]
    fn a_change_the_highlighter_has_not_heard_of_is_drawn_plain() {
        let p = Palette::for_mode(false);
        let mut v = view("fn 1");
        let (words, log) = Words::boxed(1);
        v.set_highlighter(Some(words));
        assert!(!row_spans(&drawn(&v), "fn 1").is_empty());
        v.editor_mut().set_selections(vec![Selection::caret(0)]);
        v.editor_mut().type_text("2 ");
        assert!(v.has_work());
        assert!(
            row_spans(&drawn(&v), "2 fn 1").is_empty(),
            "coloured by a highlighter that has not seen the text"
        );
        assert!(!v.work());
        assert_eq!(
            log.borrow().last().map(String::as_str),
            Some("edited 0..0->2")
        );
        let spans = row_spans(&drawn(&v), "2 fn 1");
        assert_eq!(
            spans[0].color,
            p.syntax_ink(crate::highlight::Highlight::Number)
        );
    }

    /// **A replaced editor starts the highlighter over**: its journal does
    /// not continue the one the highlighter was following.
    #[test]
    fn a_replaced_editor_starts_the_highlighter_over() {
        let mut v = view("fn");
        let (words, log) = Words::boxed(1);
        v.set_highlighter(Some(words));
        *v.editor_mut() = CodeEditor::from_text("fn fn");
        v.work();
        assert_eq!(*log.borrow(), ["reset", "reset"]);
        // A journal someone else took is the same case.
        v.editor_mut().type_text("1");
        let _ = v.editor_mut().take_changes();
        v.work();
        assert_eq!(log.borrow().last().map(String::as_str), Some("reset"));
    }

    /// **Work the highlighter cannot finish at once is given a slice at a
    /// time**, while the view says it has work, and the colours arrive when
    /// it is done.
    #[test]
    fn work_is_given_a_slice_at_a_time_until_it_is_done() {
        let mut v = view("fn");
        let (words, _) = Words::boxed(3);
        v.set_highlighter(Some(words));
        assert!(v.has_work(), "one slice of three was given");
        assert!(row_spans(&drawn(&v), "fn").is_empty());
        assert!(v.work());
        assert!(!v.work());
        assert!(!v.has_work());
        assert!(!row_spans(&drawn(&v), "fn").is_empty());
        // A view without one has nothing to do.
        let mut plain = view("fn");
        assert!(!plain.has_work() && !plain.work());
    }

    /// **Work that drawing finds keeps the view asking**: a highlighter
    /// whose drawing starts work it cannot finish at once -- a language
    /// inside another -- says so through `has_work`, and the view has work
    /// until it is done, then draws its colours.
    #[test]
    fn work_that_drawing_finds_keeps_the_view_working() {
        use crate::highlight::Highlight;
        use core::cell::Cell;
        /// Colours everything once `work` has run after a draw found it.
        #[derive(Debug, Default)]
        struct Found {
            pending: Cell<bool>,
            done: bool,
        }
        impl Highlighter for Found {
            fn reset(&mut self, _text: &crate::textbuffer::TextBuffer) {}
            fn edited(
                &mut self,
                _text: &crate::textbuffer::TextBuffer,
                _splices: &[crate::textbuffer::Splice],
            ) {
            }
            fn work(&mut self, _text: &crate::textbuffer::TextBuffer, _b: Duration) -> bool {
                if self.pending.replace(false) {
                    self.done = true;
                }
                false
            }
            fn has_work(&self) -> bool {
                self.pending.get()
            }
            fn highlights(
                &self,
                _text: &crate::textbuffer::TextBuffer,
                range: Range<usize>,
            ) -> Vec<HighlightSpan> {
                if self.done {
                    return vec![HighlightSpan {
                        range,
                        highlight: Highlight::Keyword,
                    }];
                }
                self.pending.set(true);
                Vec::new()
            }
        }
        let mut v = view("fn");
        v.set_highlighter(Some(Box::new(Found::default())));
        assert!(!v.has_work(), "nothing is found before a draw");
        assert!(row_spans(&drawn(&v), "fn").is_empty());
        assert!(v.has_work(), "the work the draw found was not asked for");
        assert!(!v.work());
        assert!(!v.has_work());
        assert!(!row_spans(&drawn(&v), "fn").is_empty());
    }

    /// **Colour runs**: the selection's ink wins, then the highlighter's,
    /// then the text's; neighbours of one colour are one run.
    #[test]
    fn colour_runs_prefer_the_selection_and_merge_neighbours() {
        let (k, n, sel, ink) = (
            Color::rgb(1, 0, 0),
            Color::rgb(2, 0, 0),
            Color::rgb(3, 0, 0),
            Color::rgb(4, 0, 0),
        );
        let runs = color_runs(
            &[(0..2, k), (2..4, n), (4..5, n), (6..8, k), (9..10, k)],
            core::slice::from_ref(&(3..7)),
            sel,
            ink,
        );
        let got: Vec<(u32, Color)> = runs.iter().map(|s| (s.end, s.color)).collect();
        assert_eq!(got, [(2, k), (3, n), (7, sel), (8, k), (9, ink), (10, k)]);
        assert!(color_runs(&[], &[], sel, ink).is_empty());
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

    /// A highlighter that colours nothing and says the same of every
    /// bracket.
    #[derive(Debug)]
    struct Says(Brackets);

    impl Highlighter for Says {
        fn reset(&mut self, _text: &crate::textbuffer::TextBuffer) {}

        fn edited(
            &mut self,
            _text: &crate::textbuffer::TextBuffer,
            _splices: &[crate::textbuffer::Splice],
        ) {
        }

        fn work(&mut self, _text: &crate::textbuffer::TextBuffer, _budget: Duration) -> bool {
            false
        }

        fn highlights(
            &self,
            _text: &crate::textbuffer::TextBuffer,
            _range: Range<usize>,
        ) -> Vec<HighlightSpan> {
            Vec::new()
        }

        fn brackets(&self, _text: &crate::textbuffer::TextBuffer, _offset: usize) -> Brackets {
            self.0
        }
    }

    /// **The highlighter's word on brackets is taken over counting**: a pair
    /// it names is outlined where it says, none where it says the bracket is
    /// no bracket (in a string, a comment), and counting only where it
    /// cannot say.
    #[test]
    fn the_highlighters_word_on_brackets_is_taken() {
        let outlined = |said: Brackets| -> Vec<f32> {
            let mut v = view("f(x) (y)");
            v.set_highlighter(Some(Box::new(Says(said))));
            v.editor_mut().set_selections(vec![Selection::caret(1)]);
            drawn(&v)
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { x, .. } => Some(*x),
                    _ => None,
                })
                .collect()
        };
        let counted = outlined(Brackets::Unknown);
        assert_eq!(counted.len(), 2, "counting pairs `(` with `)`");
        assert!(outlined(Brackets::Unpaired).is_empty());
        let named = outlined(Brackets::Pair(5, 7));
        assert_eq!(named.len(), 2);
        assert_ne!(named, counted, "outlined where the highlighter said");
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

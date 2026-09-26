//! A multi-line text field: the text, its caret and selection, the view's
//! scroll position and an undo history, with the editing and caret motion a
//! notes box, a message body or a description field needs -- and the one way
//! to draw it.
//!
//! # Why this exists
//!
//! `roadmap-detailed.md` §3.5 asks for text input "single-line and multiline",
//! with a word-wrap option. The single-line half is [`crate::textinput`] over
//! [`crate::textedit`]; the multi-line half was `WidgetKind::TextArea`, which
//! holds a value and a placeholder and nothing else -- no caret, no selection,
//! no scrolling. Every program that needed more than one line wrote its own.
//!
//! State and drawing only, as with the single-line field: the caller owns the
//! box, the colours and the focus, and asks this for what to put in them.
//!
//! # Lines
//!
//! Moving up and down, and turning a click into an offset, work on *visual*
//! lines: a paragraph wrapped to the box's width is several of them. They come
//! from [`text::wrap_ranges`], whose ranges tile the text, so every byte
//! offset is on some line.
//!
//! Where a paragraph wraps, one offset ends a line and starts the next. Which
//! of the two a caret there is drawn on is the cursor's [`Affinity`]:
//! [`Affinity::Upstream`] belongs to the line above -- where End puts it --
//! and [`Affinity::Downstream`] to the line below, where typing carries on.
//! That is the bit the cursor already carries for a change of text direction,
//! used for the same kind of question: one offset, two places on the screen.
//!
//! # Bidirectional text
//!
//! Within a line, Left and Right move by the screen through
//! [`text::caret_left`] and [`text::caret_right`], as the single-line field
//! does (`design-decisions.md` §541). At a line's edge they carry on in the
//! direction the line runs: off the left of a left-to-right line is the end of
//! the line above, and off the left of a right-to-left line is the start of
//! the line below.
//!
//! # Scrolling
//!
//! The vertical offset is state, unlike the single-line field's horizontal
//! one (§546): a box of several lines is read as well as typed in, and the
//! wheel moves the view without moving the caret. Every read clamps it against
//! the current layout, so a shorter text or a taller box cannot leave the view
//! past the end. Without wrapping, the horizontal offset is §546's: worked out
//! each time from where the caret is.
//!
//! # Undo
//!
//! Every change is an [`Edit`] -- where, what went, what came -- so undoing is
//! replacing one with the other and does not keep copies of the text. Typing
//! is gathered into one step a word at a time, as is deleting, because an undo
//! that takes back one letter per press is one nobody uses twice.

use core::cell::RefCell;
use core::ops::Range;
use std::rc::Rc;

use crate::color::Color;
use crate::event::{Key, KeyEvent};
use crate::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow, TextSpan};
use crate::style::CornerRadii;
use crate::text::{self, Affinity, TextCursor};
use crate::textedit;
use crate::textinput::KeyEdit;

/// How many edits undo reaches back through. Past this the oldest go; a
/// history that grew without bound would keep every keystroke of a long
/// session in memory.
pub const UNDO_DEPTH: usize = 500;

/// What a layout depends on besides the text: the box and the font.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// The room a line has, in pixels: the box's inner width.
    pub width: f32,
    /// How much of the text shows, in pixels: the box's inner height.
    pub height: f32,
    /// The size the text is measured and drawn at.
    pub font_size: f32,
    /// The weight the text is measured and drawn at.
    pub weight: FontWeightHint,
}

impl Metrics {
    /// The distance from one line to the next.
    #[must_use]
    pub fn line_height(&self) -> f32 {
        text::line_height(self.font_size, self.weight)
    }

    /// How many whole lines the box shows, and never fewer than one.
    #[must_use]
    pub fn lines_shown(&self) -> usize {
        let per = self.line_height();
        if per > 0.0 && self.height.is_finite() && self.height > 0.0 {
            // A box height over a line height: small and positive, so the
            // conversion is exact where it matters and saturates where not.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let n = (self.height / per).floor() as usize;
            n.max(1)
        } else {
            1
        }
    }
}

/// One line as drawn: a byte range of the text, and whether the paragraph
/// carries on on the next line (a wrap) rather than ending here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// Offset of the line's first byte.
    pub start: usize,
    /// Offset just past its last byte -- before the newline, where a
    /// paragraph ends here.
    pub end: usize,
    /// The paragraph wraps onto the next line, which starts at `end`.
    pub soft: bool,
}

impl Line {
    fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// One change to the text, as undo and redo replay it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Edit {
    /// Where the change was made.
    at: usize,
    /// What was there before.
    removed: String,
    /// What is there now.
    inserted: String,
    /// The caret and anchor before the change, which undo puts back.
    cursor_before: TextCursor,
    anchor_before: Option<usize>,
}

/// Whether the last edit may take in the next one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
    /// Nothing to join: the last thing done was not typing or deleting, or
    /// the caret has moved since.
    None,
    /// Typing, a character at a time.
    Typing,
    /// Backspace or Delete, a character at a time.
    Deleting,
}

/// A laid-out text, and what it was laid out for.
struct Layout {
    revision: u64,
    width: u32,
    size: u32,
    weight: FontWeightHint,
    wrap: bool,
    lines: Rc<[Line]>,
}

/// A multi-line text field's state. See the module documentation.
pub struct TextArea {
    text: String,
    cursor: TextCursor,
    anchor: Option<usize>,
    /// The x the caret is holding to while it moves up and down, so that a
    /// short line on the way does not pull it to the left for good.
    goal_x: Option<f32>,
    wrap: bool,
    scroll_y: f32,
    clipboard: String,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    run: Run,
    /// Bumped by every change to the text, so a layout knows when it is stale.
    revision: u64,
    layout: RefCell<Option<Layout>>,
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for TextArea {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TextArea")
            .field("text", &self.text)
            .field("cursor", &self.cursor)
            .field("anchor", &self.anchor)
            .field("wrap", &self.wrap)
            .field("scroll_y", &self.scroll_y)
            .finish_non_exhaustive()
    }
}

/// `text` with every line ending made a `\n`.
///
/// Pasted text arrives with whatever ending the program it came from wrote --
/// `\r\n` from a Windows one, a bare `\r` from an old Mac one -- and a `\r`
/// kept in the text is a character the lines do not break at and the caret
/// steps over invisibly.
fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Whether a character belongs to a word, for double-click selection and for
/// where a run of typing is split into undo steps.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl TextArea {
    /// An empty field, wrapping at the box's width.
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: TextCursor::default(),
            anchor: None,
            goal_x: None,
            wrap: true,
            scroll_y: 0.0,
            clipboard: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            run: Run::None,
            revision: 0,
            layout: RefCell::new(None),
        }
    }

    /// A field holding `text`, with the caret at its end.
    #[must_use]
    pub fn with_text(text: &str) -> Self {
        let mut area = Self::new();
        area.set_text(text);
        area
    }

    /// The text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replace the whole text, as loading a document does: the caret goes to
    /// the end, the view to the top, and the undo history is forgotten --
    /// undoing into the text that was there before a load is not an edit the
    /// user made.
    pub fn set_text(&mut self, text: &str) {
        self.text = normalize_newlines(text);
        self.cursor = TextCursor::from(self.text.len());
        self.anchor = None;
        self.goal_x = None;
        self.scroll_y = 0.0;
        self.undo.clear();
        self.redo.clear();
        self.run = Run::None;
        self.revision = self.revision.wrapping_add(1);
    }

    /// The caret.
    #[must_use]
    pub fn cursor(&self) -> TextCursor {
        self.cursor
    }

    /// Put the caret at `cursor`, snapped to a character boundary, and drop
    /// any selection.
    pub fn set_cursor(&mut self, cursor: TextCursor) {
        let byte = cursor.snapped_in(&self.text).byte();
        self.cursor = TextCursor {
            byte,
            affinity: cursor.affinity,
        };
        self.anchor = None;
        self.goal_x = None;
        self.run = Run::None;
    }

    /// Where a selection started, if one is being made.
    #[must_use]
    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// The selected byte range, low end first, or `None`.
    #[must_use]
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        textedit::selected_range(self.cursor, self.anchor)
    }

    /// Whether anything is selected.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// The selected text, or `""`.
    #[must_use]
    pub fn selected_text(&self) -> &str {
        self.selection_range()
            .and_then(|(from, to)| self.text.get(from..to))
            .unwrap_or("")
    }

    /// Whether lines wrap at the box's width. On by default: a box that does
    /// not wrap hides the end of every long line until the caret goes there.
    #[must_use]
    pub fn wrap(&self) -> bool {
        self.wrap
    }

    /// Turn wrapping on or off.
    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
        self.goal_x = None;
    }

    /// What Ctrl+C put aside, for a caller that bridges it to a clipboard
    /// service -- there is none yet, so it is this field's own, as
    /// [`crate::textinput::TextInput`]'s is.
    #[must_use]
    pub fn clipboard(&self) -> &str {
        &self.clipboard
    }

    /// Set what Ctrl+V pastes.
    pub fn set_clipboard(&mut self, text: String) {
        self.clipboard = text;
    }

    /// Whether there is anything to undo.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is anything to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    // ---- layout ------------------------------------------------------------

    /// The text's lines as drawn in a box of `m`.
    ///
    /// Kept between calls and made again only when the text, the width, the
    /// font or the wrapping has changed: breaking lines means shaping the whole
    /// text, and a frame or a keystroke would otherwise do it several times.
    #[must_use]
    pub fn lines(&self, m: &Metrics) -> Rc<[Line]> {
        let width = if self.wrap { m.width } else { 0.0 };
        let fresh = |layout: &Layout| {
            layout.revision == self.revision
                && layout.width == width.to_bits()
                && layout.size == m.font_size.to_bits()
                && layout.weight == m.weight
                && layout.wrap == self.wrap
        };
        if let Some(layout) = self.layout.borrow().as_ref()
            && fresh(layout)
        {
            return Rc::clone(&layout.lines);
        }
        let ranges = text::wrap_ranges(&self.text, width, m.font_size, m.weight);
        let lines: Rc<[Line]> = ranges
            .iter()
            .enumerate()
            .map(|(n, range)| Line {
                start: range.start,
                end: range.end,
                soft: ranges
                    .get(n.saturating_add(1))
                    .is_some_and(|next| next.start == range.end),
            })
            .collect();
        *self.layout.borrow_mut() = Some(Layout {
            revision: self.revision,
            width: width.to_bits(),
            size: m.font_size.to_bits(),
            weight: m.weight,
            wrap: self.wrap,
            lines: Rc::clone(&lines),
        });
        lines
    }

    /// Which of `lines` the caret at `at` is drawn on.
    fn line_of(lines: &[Line], at: TextCursor) -> usize {
        let index = lines
            .partition_point(|line| line.start <= at.byte)
            .saturating_sub(1);
        // The one offset that is on two lines: the end of a wrapped line and
        // the start of the next. Upstream belongs to the line above.
        if at.affinity == Affinity::Upstream
            && let Some(before) = index.checked_sub(1)
            && let Some(prev) = lines.get(before)
            && prev.soft
            && prev.end == at.byte
        {
            return before;
        }
        index
    }

    /// The text of one line.
    fn line_text(&self, line: &Line) -> &str {
        self.text.get(line.range()).unwrap_or("")
    }

    /// The caret `at`, as a cursor into its line's own text.
    fn local(at: TextCursor, line: &Line) -> TextCursor {
        TextCursor {
            byte: at
                .byte
                .saturating_sub(line.start)
                .min(line.end.saturating_sub(line.start)),
            affinity: at.affinity,
        }
    }

    /// A cursor in `line`'s own text, as a cursor into the whole text -- on
    /// the line itself when it lands on the line's end and the paragraph
    /// wraps there.
    fn global(local: TextCursor, line: &Line) -> TextCursor {
        let byte = line.start.saturating_add(local.byte).min(line.end);
        let affinity = if line.soft && byte == line.end {
            Affinity::Upstream
        } else {
            local.affinity
        };
        TextCursor { byte, affinity }
    }

    /// The caret's line, and its x from the start of that line.
    #[must_use]
    pub fn caret_position(&self, m: &Metrics) -> (usize, f32) {
        let lines = self.lines(m);
        let index = Self::line_of(&lines, self.cursor);
        let x = lines.get(index).map_or(0.0, |line| {
            text::caret_x(
                self.line_text(line),
                Self::local(self.cursor, line),
                m.font_size,
                m.weight,
            )
        });
        (index, x)
    }

    /// Whether a line reads right to left: its end is drawn left of its start.
    fn runs_right_to_left(line_text: &str, m: &Metrics) -> bool {
        if line_text.is_empty() {
            return false;
        }
        let start = text::caret_x(line_text, TextCursor::from(0), m.font_size, m.weight);
        let end = text::caret_x(
            line_text,
            TextCursor::from(line_text.len()),
            m.font_size,
            m.weight,
        );
        end < start
    }

    // ---- scrolling -----------------------------------------------------------

    /// How tall the whole text is, in pixels.
    #[must_use]
    pub fn content_height(&self, m: &Metrics) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let count = self.lines(m).len() as f32;
        count * m.line_height()
    }

    /// The furthest the view can scroll down and still be full.
    fn max_scroll(&self, m: &Metrics) -> f32 {
        (self.content_height(m) - m.height).max(0.0)
    }

    /// How far down the view is scrolled, in pixels -- clamped to the text as
    /// it is now, so a shorter text never leaves the view past its end.
    #[must_use]
    pub fn scroll_y(&self, m: &Metrics) -> f32 {
        self.scroll_y.clamp(0.0, self.max_scroll(m))
    }

    /// Scroll the view by `dy` pixels, positive towards the end. The caret
    /// stays where it is: the wheel is for reading.
    pub fn scroll_by(&mut self, dy: f32, m: &Metrics) {
        if dy.is_finite() {
            self.scroll_y = (self.scroll_y(m) + dy).clamp(0.0, self.max_scroll(m));
        }
    }

    /// Scroll just far enough that the caret's line is in view.
    pub fn reveal_caret(&mut self, m: &Metrics) {
        let (index, _) = self.caret_position(m);
        let per = m.line_height();
        #[allow(clippy::cast_precision_loss)]
        let top = index as f32 * per;
        let mut scroll = self.scroll_y(m);
        if top < scroll {
            scroll = top;
        } else if top + per > scroll + m.height {
            scroll = top + per - m.height;
        }
        self.scroll_y = scroll.clamp(0.0, self.max_scroll(m));
    }

    /// How far left the text is shifted to keep the caret in the box: nothing
    /// while wrapping, since every line fits; otherwise §546's answer for the
    /// caret's line, worked out afresh.
    #[must_use]
    pub fn scroll_x(&self, m: &Metrics) -> f32 {
        if self.wrap {
            return 0.0;
        }
        let lines = self.lines(m);
        let index = Self::line_of(&lines, self.cursor);
        let Some(line) = lines.get(index) else {
            return 0.0;
        };
        let line_text = self.line_text(line);
        let width = text::measure(line_text, m.font_size, m.weight);
        let caret = text::caret_x(
            line_text,
            Self::local(self.cursor, line),
            m.font_size,
            m.weight,
        );
        textedit::horizontal_scroll(width, m.width, caret)
    }

    // ---- editing -------------------------------------------------------------

    /// Replace `range` with `with`, record it for undo, and leave the caret
    /// after the new text.
    fn replace(&mut self, range: Range<usize>, with: &str, run: Run) {
        if range.start > range.end
            || range.end > self.text.len()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            // Cannot arise from the callers below, which take their ranges from
            // the text's own caret stops -- but `replace_range` panics on a bad
            // one, and a text field that takes the program down over a keystroke
            // is worse than one that ignores the keystroke.
            return;
        }
        let removed = self.text.get(range.clone()).unwrap_or("").to_string();
        if removed.is_empty() && with.is_empty() {
            return;
        }
        let edit = Edit {
            at: range.start,
            removed,
            inserted: with.to_string(),
            cursor_before: self.cursor,
            anchor_before: self.anchor,
        };
        self.text.replace_range(range, with);
        self.cursor = TextCursor::from(edit.at.saturating_add(with.len()));
        self.anchor = None;
        self.goal_x = None;
        self.revision = self.revision.wrapping_add(1);
        self.redo.clear();
        self.record(edit, run);
    }

    /// Put `edit` on the undo history, or fold it into the step before it
    /// when both are part of one run of typing or deleting.
    fn record(&mut self, edit: Edit, run: Run) {
        let joined = match (run, self.run, self.undo.last_mut()) {
            (Run::Typing, Run::Typing, Some(last)) => {
                // Typing continues where the last typing ended, and a word
                // boundary starts a new step: the space after a word joins it,
                // the next word's first letter does not.
                let contiguous = last.at.saturating_add(last.inserted.len()) == edit.at
                    && edit.removed.is_empty()
                    && last.removed.is_empty();
                let starts_word = edit.inserted.chars().next().is_some_and(is_word_char)
                    && last
                        .inserted
                        .chars()
                        .last()
                        .is_some_and(|c| !is_word_char(c));
                let newline = edit.inserted.contains('\n');
                if contiguous && !starts_word && !newline {
                    last.inserted.push_str(&edit.inserted);
                    true
                } else {
                    false
                }
            }
            (Run::Deleting, Run::Deleting, Some(last))
                if edit.inserted.is_empty() && last.inserted.is_empty() =>
            {
                if edit.at.saturating_add(edit.removed.len()) == last.at {
                    // Backspace: the new deletion is just before the last one.
                    last.removed.insert_str(0, &edit.removed);
                    last.at = edit.at;
                    true
                } else if edit.at == last.at {
                    // Delete: the new deletion is where the last one was.
                    last.removed.push_str(&edit.removed);
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if !joined {
            self.undo.push(edit);
            if self.undo.len() > UNDO_DEPTH {
                self.undo.remove(0);
            }
        }
        self.run = run;
    }

    /// Insert `text` at the caret, replacing the selection if there is one.
    /// Line endings of every kind become `\n`.
    pub fn insert_str(&mut self, text: &str) {
        let text = normalize_newlines(text);
        let range = self
            .selection_range()
            .map_or(self.cursor.byte..self.cursor.byte, |(from, to)| from..to);
        let run = if self.has_selection() {
            Run::None
        } else {
            Run::Typing
        };
        self.replace(range, &text, run);
    }

    /// Insert one typed character, replacing the selection if there is one.
    pub fn insert_char(&mut self, ch: char) {
        let mut buf = [0_u8; 4];
        self.insert_str(ch.encode_utf8(&mut buf));
    }

    /// Delete the selection, or the character before the caret.
    pub fn backspace(&mut self) {
        if let Some((from, to)) = self.selection_range() {
            self.replace(from..to, "", Run::None);
            return;
        }
        if let Some(prev) = self.cursor.prev_in(&self.text) {
            self.replace(prev.byte()..self.cursor.byte, "", Run::Deleting);
        }
    }

    /// Delete the selection, or the character after the caret.
    pub fn delete(&mut self) {
        if let Some((from, to)) = self.selection_range() {
            self.replace(from..to, "", Run::None);
            return;
        }
        if let Some(next) = self.cursor.next_in(&self.text) {
            self.replace(self.cursor.byte..next.byte(), "", Run::Deleting);
            // The caret stays put: Delete eats forwards.
        }
    }

    /// Select the whole text.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = TextCursor::from(self.text.len());
        self.goal_x = None;
        self.run = Run::None;
    }

    /// Copy the selection to the clipboard. Nothing selected copies nothing
    /// and leaves the clipboard as it was.
    pub fn copy(&mut self) {
        let selected = self.selected_text().to_string();
        if !selected.is_empty() {
            self.clipboard = selected;
        }
    }

    /// Copy the selection, then delete it.
    pub fn cut(&mut self) {
        if let Some((from, to)) = self.selection_range() {
            self.copy();
            self.replace(from..to, "", Run::None);
        }
    }

    /// Insert the clipboard at the caret, over the selection if there is one.
    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let clip = self.clipboard.clone();
        let range = self
            .selection_range()
            .map_or(self.cursor.byte..self.cursor.byte, |(from, to)| from..to);
        self.replace(range, &normalize_newlines(&clip), Run::None);
    }

    /// Take back the last step. Returns whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        let end = edit.at.saturating_add(edit.inserted.len());
        if end <= self.text.len()
            && self.text.is_char_boundary(edit.at)
            && self.text.is_char_boundary(end)
        {
            self.text.replace_range(edit.at..end, &edit.removed);
        }
        self.cursor = edit.cursor_before;
        self.anchor = edit.anchor_before;
        self.goal_x = None;
        self.run = Run::None;
        self.revision = self.revision.wrapping_add(1);
        self.redo.push(edit);
        true
    }

    /// Put back the last step taken back. Returns whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        let end = edit.at.saturating_add(edit.removed.len());
        if end <= self.text.len()
            && self.text.is_char_boundary(edit.at)
            && self.text.is_char_boundary(end)
        {
            self.text.replace_range(edit.at..end, &edit.inserted);
        }
        self.cursor = TextCursor::from(edit.at.saturating_add(edit.inserted.len()));
        self.anchor = None;
        self.goal_x = None;
        self.run = Run::None;
        self.revision = self.revision.wrapping_add(1);
        self.undo.push(edit);
        true
    }

    // ---- caret motion --------------------------------------------------------

    /// Start or drop a selection before a move, and end any run of typing:
    /// moving the caret is where one undo step ends and the next begins.
    fn before_move(&mut self, shift: bool) {
        textedit::begin_or_end_selection(shift, self.cursor, &mut self.anchor);
        self.run = Run::None;
    }

    /// Left, by the screen.
    pub fn move_left(&mut self, shift: bool, m: &Metrics) {
        if !shift && let Some((from, _)) = self.selection_range() {
            self.set_cursor(TextCursor::from(from));
            return;
        }
        self.before_move(shift);
        self.goal_x = None;
        self.cursor = self.step(m, true);
    }

    /// Right, by the screen.
    pub fn move_right(&mut self, shift: bool, m: &Metrics) {
        if !shift && let Some((_, to)) = self.selection_range() {
            self.set_cursor(TextCursor::from(to));
            return;
        }
        self.before_move(shift);
        self.goal_x = None;
        self.cursor = self.step(m, false);
    }

    /// Where one step left (or right) of the caret is, on the screen.
    fn step(&self, m: &Metrics, left: bool) -> TextCursor {
        let lines = self.lines(m);
        let index = Self::line_of(&lines, self.cursor);
        let Some(line) = lines.get(index) else {
            return self.cursor;
        };
        let line_text = self.line_text(line);
        let local = Self::local(self.cursor, line);
        let moved = if left {
            text::caret_left(line_text, local, m.font_size, m.weight)
        } else {
            text::caret_right(line_text, local, m.font_size, m.weight)
        };
        if let Some(moved) = moved {
            return Self::global(moved, line);
        }
        // Off the edge of the line: on to the neighbouring line, in the
        // direction this one runs.
        let forwards = left == Self::runs_right_to_left(line_text, m);
        if forwards {
            match lines.get(index.saturating_add(1)) {
                Some(next) => TextCursor {
                    byte: next.start,
                    affinity: Affinity::Downstream,
                },
                None => self.cursor,
            }
        } else {
            match index.checked_sub(1).and_then(|i| lines.get(i)) {
                Some(prev) => TextCursor {
                    byte: prev.end,
                    affinity: if prev.soft {
                        Affinity::Upstream
                    } else {
                        Affinity::Downstream
                    },
                },
                None => self.cursor,
            }
        }
    }

    /// Up one line, holding to the column the caret started in.
    pub fn move_up(&mut self, shift: bool, m: &Metrics) {
        self.move_lines(shift, m, true, 1);
    }

    /// Down one line, holding to the column the caret started in.
    pub fn move_down(&mut self, shift: bool, m: &Metrics) {
        self.move_lines(shift, m, false, 1);
    }

    /// Up a boxful of lines, less one so the line the caret was on stays in
    /// sight.
    pub fn page_up(&mut self, shift: bool, m: &Metrics) {
        let by = m.lines_shown().saturating_sub(1).max(1);
        self.move_lines(shift, m, true, by);
    }

    /// Down a boxful of lines, less one.
    pub fn page_down(&mut self, shift: bool, m: &Metrics) {
        let by = m.lines_shown().saturating_sub(1).max(1);
        self.move_lines(shift, m, false, by);
    }

    /// Move `by` lines up or down. Past the first line is the start of the
    /// text and past the last is its end, as in every editor: a caret that
    /// stopped short would leave the rest of the first or last line out of
    /// reach of the keyboard's vertical keys.
    fn move_lines(&mut self, shift: bool, m: &Metrics, up: bool, by: usize) {
        let (index, x) = self.caret_position(m);
        let goal = *self.goal_x.get_or_insert(x);
        self.before_move(shift);
        let lines = self.lines(m);
        let target = if up {
            index.checked_sub(by)
        } else {
            index.checked_add(by)
        };
        self.cursor = match target.and_then(|t| lines.get(t)) {
            Some(line) => Self::global(
                text::cursor_at(self.line_text(line), goal, m.font_size, m.weight),
                line,
            ),
            None if up => TextCursor::from(0),
            None => TextCursor::from(self.text.len()),
        };
        // Kept: the next Up or Down aims for the same column.
        self.goal_x = Some(goal);
    }

    /// To the start of the caret's line (Home).
    pub fn move_line_start(&mut self, shift: bool, m: &Metrics) {
        self.before_move(shift);
        self.goal_x = None;
        let lines = self.lines(m);
        if let Some(line) = lines.get(Self::line_of(&lines, self.cursor)) {
            self.cursor = TextCursor {
                byte: line.start,
                affinity: Affinity::Downstream,
            };
        }
    }

    /// To the end of the caret's line (End) -- on that line, where it wraps.
    pub fn move_line_end(&mut self, shift: bool, m: &Metrics) {
        self.before_move(shift);
        self.goal_x = None;
        let lines = self.lines(m);
        if let Some(line) = lines.get(Self::line_of(&lines, self.cursor)) {
            self.cursor = TextCursor {
                byte: line.end,
                affinity: if line.soft {
                    Affinity::Upstream
                } else {
                    Affinity::Downstream
                },
            };
        }
    }

    /// To the start of the text (Ctrl+Home).
    pub fn move_text_start(&mut self, shift: bool) {
        self.before_move(shift);
        self.goal_x = None;
        self.cursor = TextCursor::from(0);
    }

    /// To the end of the text (Ctrl+End).
    pub fn move_text_end(&mut self, shift: bool) {
        self.before_move(shift);
        self.goal_x = None;
        self.cursor = TextCursor::from(self.text.len());
    }

    // ---- the pointer -----------------------------------------------------------

    /// The caret a point in the box means, `(x, y)` from the box's top-left.
    ///
    /// Above the text is its first line and below it the last, so a drag that
    /// leaves the box keeps selecting.
    #[must_use]
    pub fn cursor_at_point(&self, x: f32, y: f32, m: &Metrics) -> TextCursor {
        let lines = self.lines(m);
        let per = m.line_height();
        let content_y = y + self.scroll_y(m);
        let index = if per > 0.0 && content_y.is_finite() && content_y > 0.0 {
            // Non-negative and finite; `as` saturates anything past the end.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let row = (content_y / per).floor() as usize;
            row.min(lines.len().saturating_sub(1))
        } else {
            0
        };
        let Some(line) = lines.get(index) else {
            return TextCursor::from(0);
        };
        let content_x = x + self.scroll_x(m);
        Self::global(
            text::cursor_at(self.line_text(line), content_x, m.font_size, m.weight),
            line,
        )
    }

    /// A press at `(x, y)`: the caret goes there, and a selection starts from
    /// there for a drag to extend. `clicks` is how many presses in a row this
    /// makes -- two select the word, three the whole paragraph. Shift extends
    /// the selection that exists instead of starting another.
    pub fn press(&mut self, x: f32, y: f32, clicks: u8, shift: bool, m: &Metrics) {
        let at = self.cursor_at_point(x, y, m);
        self.run = Run::None;
        self.goal_x = None;
        match clicks {
            0 | 1 => {
                if shift {
                    self.anchor.get_or_insert(self.cursor.byte);
                } else {
                    self.anchor = Some(at.byte);
                }
                self.cursor = at;
            }
            2 => {
                let word = self.word_around(at.byte);
                self.anchor = Some(word.start);
                self.cursor = TextCursor::from(word.end);
            }
            _ => {
                let para = self.paragraph_around(at.byte);
                self.anchor = Some(para.start);
                self.cursor = TextCursor::from(para.end);
            }
        }
    }

    /// The pointer moved to `(x, y)` with the button held: the selection's
    /// far end follows it, and the view scrolls to keep it in sight.
    pub fn drag_to(&mut self, x: f32, y: f32, m: &Metrics) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor.byte);
        }
        self.cursor = self.cursor_at_point(x, y, m);
        self.reveal_caret(m);
    }

    /// The word, the run of spaces, or the single other character at `byte`.
    fn word_around(&self, byte: usize) -> Range<usize> {
        let text = &self.text;
        let class = |c: char| {
            if is_word_char(c) {
                0
            } else if c.is_whitespace() && c != '\n' {
                1
            } else {
                2
            }
        };
        let Some(here) = text.get(byte..).and_then(|rest| rest.chars().next()) else {
            // Past the last character: the word that ends there, if any.
            return match text.get(..byte).and_then(|head| head.chars().next_back()) {
                Some(c) if class(c) == 0 => self.word_around(byte.saturating_sub(c.len_utf8())),
                _ => byte..byte,
            };
        };
        let kind = class(here);
        if kind == 2 {
            return byte..byte.saturating_add(here.len_utf8());
        }
        let start = text.get(..byte).map_or(byte, |head| {
            head.char_indices()
                .rev()
                .take_while(|(_, c)| class(*c) == kind)
                .last()
                .map_or(byte, |(i, _)| i)
        });
        let end = text.get(byte..).map_or(byte, |tail| {
            tail.char_indices()
                .find(|(_, c)| class(*c) != kind)
                .map_or(text.len(), |(i, _)| byte.saturating_add(i))
        });
        start..end
    }

    /// The paragraph at `byte`: the text between the newlines either side.
    fn paragraph_around(&self, byte: usize) -> Range<usize> {
        let start = self
            .text
            .get(..byte)
            .and_then(|head| head.rfind('\n'))
            .map_or(0, |i| i.saturating_add(1));
        let end = self
            .text
            .get(byte..)
            .and_then(|tail| tail.find('\n'))
            .map_or(self.text.len(), |i| byte.saturating_add(i));
        start..end
    }

    // ---- the keyboard ----------------------------------------------------------

    /// Apply one keystroke's editing meaning, and say what it did.
    ///
    /// Everything [`crate::textinput::TextInput::edit_key`] handles, plus what
    /// a box of several lines needs: Up, Down, Page Up and Page Down;
    /// Ctrl+Home and Ctrl+End for the ends of the text; Enter for a new line;
    /// and undo (Ctrl+Z) and redo (Ctrl+Y, or Ctrl+Shift+Z). The view scrolls
    /// to keep the caret in sight after each.
    ///
    /// **Left to the owner:** Tab, which moves the focus between fields rather
    /// than indenting -- a notes box that ate Tab would be a trap for a
    /// keyboard user -- and Ctrl+Enter, the conventional "send" in a message
    /// body. Both come back [`KeyEdit::Unhandled`].
    ///
    /// The Ctrl chords want Ctrl without Alt, for the reason `TextInput`
    /// gives: AltGr arrives as Ctrl+Alt, and is how several layouts type a
    /// letter.
    pub fn edit_key(&mut self, key: &KeyEvent, m: &Metrics) -> KeyEdit {
        if !key.pressed {
            return KeyEdit::Unhandled;
        }
        let shift = key.modifiers.shift;
        let ctrl = key.modifiers.ctrl && !key.modifiers.alt;
        let before = self.revision;
        let outcome = match key.key {
            Key::A if ctrl => {
                self.select_all();
                KeyEdit::Handled
            }
            Key::C if ctrl => {
                self.copy();
                KeyEdit::Handled
            }
            Key::X if ctrl => {
                self.cut();
                KeyEdit::Handled
            }
            Key::V if ctrl => {
                self.paste();
                KeyEdit::Handled
            }
            Key::Z if ctrl && shift => {
                self.redo();
                KeyEdit::Handled
            }
            Key::Z if ctrl => {
                self.undo();
                KeyEdit::Handled
            }
            Key::Y if ctrl => {
                self.redo();
                KeyEdit::Handled
            }
            Key::Left => {
                self.move_left(shift, m);
                KeyEdit::Handled
            }
            Key::Right => {
                self.move_right(shift, m);
                KeyEdit::Handled
            }
            Key::Up => {
                self.move_up(shift, m);
                KeyEdit::Handled
            }
            Key::Down => {
                self.move_down(shift, m);
                KeyEdit::Handled
            }
            Key::PageUp => {
                self.page_up(shift, m);
                KeyEdit::Handled
            }
            Key::PageDown => {
                self.page_down(shift, m);
                KeyEdit::Handled
            }
            Key::Home if ctrl => {
                self.move_text_start(shift);
                KeyEdit::Handled
            }
            Key::End if ctrl => {
                self.move_text_end(shift);
                KeyEdit::Handled
            }
            Key::Home => {
                self.move_line_start(shift, m);
                KeyEdit::Handled
            }
            Key::End => {
                self.move_line_end(shift, m);
                KeyEdit::Handled
            }
            Key::Backspace => {
                self.backspace();
                KeyEdit::Handled
            }
            Key::Delete => {
                self.delete();
                KeyEdit::Handled
            }
            Key::Enter if key.modifiers.ctrl => KeyEdit::Unhandled,
            Key::Enter => {
                self.insert_str("\n");
                KeyEdit::Handled
            }
            Key::Tab => KeyEdit::Unhandled,
            _ => {
                if !key.types_text() {
                    return KeyEdit::Unhandled;
                }
                let typed: String = key.typed().collect();
                self.insert_str(&typed);
                KeyEdit::Handled
            }
        };
        if outcome == KeyEdit::Unhandled {
            return outcome;
        }
        self.reveal_caret(m);
        if self.revision == before {
            KeyEdit::Handled
        } else {
            KeyEdit::Changed
        }
    }
}

// ============================================================================
// Drawing
// ============================================================================

/// One multi-line field, ready to be drawn.
pub struct MultiLine<'a> {
    /// The field.
    pub area: &'a TextArea,
    /// Left edge of the box the text occupies.
    pub x: f32,
    /// Top edge of that box.
    pub y: f32,
    /// The box's size and the font.
    pub metrics: Metrics,
    /// Colour of unselected text, and of the caret.
    pub color: Color,
    /// What the selection is painted in -- the user's accent.
    pub selection_bg: Color,
    /// What selected text is drawn in, over `selection_bg`.
    pub selection_fg: Color,
    /// Whether to draw the caret. A caret in an unfocused field lies about
    /// where the next keystroke goes.
    pub focused: bool,
    /// How wide to draw the caret -- a user preference the caller passes in,
    /// as with [`textedit::SingleLine::caret_width`].
    pub caret_width: f32,
    /// Shown in its colour while the field is empty: what the field is for.
    pub placeholder: Option<(&'a str, Color)>,
}

/// How wide the mark is that stands for a selected line ending: a newline has
/// no glyph, and a selection that ran through one with nothing to show for it
/// would look as though it stopped at the end of the line.
const NEWLINE_MARK: f32 = 4.0;

/// Draw a field's selection, text and caret, clipped to its box.
///
/// Only the lines in view are emitted, so a long note costs what the box
/// shows rather than what the note holds.
pub fn draw(tree: &mut RenderTree, f: &MultiLine<'_>) {
    let m = &f.metrics;
    let area = f.area;
    let per = m.line_height();
    let lines = area.lines(m);
    let scroll_y = area.scroll_y(m);
    let scroll_x = area.scroll_x(m);
    let origin_x = f.x - scroll_x;
    let selection = area.selection_range();
    let (caret_line, caret_px) = area.caret_position(m);

    tree.clip(f.x, f.y, m.width, m.height);

    if area.text.is_empty()
        && let Some((placeholder, color)) = f.placeholder
    {
        tree.push(RenderCommand::Text {
            x: f.x,
            y: f.y,
            text: placeholder.to_string(),
            color,
            font_size: m.font_size,
            font_weight: m.weight,
            max_width: Some(m.width),
            overflow: TextOverflow::Ellipsis,
        });
    }

    // Finite and non-negative: the scroll is clamped to the content.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let first = if per > 0.0 {
        (scroll_y / per).floor() as usize
    } else {
        0
    };
    let shown = m.lines_shown().saturating_add(2);
    for (index, line) in lines.iter().enumerate().skip(first).take(shown) {
        #[allow(clippy::cast_precision_loss)]
        let y = f.y + index as f32 * per - scroll_y;
        let line_text = area.line_text(line);
        let local = selection.and_then(|(from, to)| {
            let from = from.max(line.start);
            let to = to.min(line.end);
            (from < to).then(|| {
                (
                    from.saturating_sub(line.start),
                    to.saturating_sub(line.start),
                )
            })
        });
        if let Some((from, to)) = local {
            for (left, width) in text::selection_boxes(line_text, from, to, m.font_size, m.weight) {
                tree.push(RenderCommand::FillRect {
                    x: origin_x + left,
                    y,
                    width,
                    height: per,
                    color: f.selection_bg,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
        // The line's ending is selected too when the selection runs on past
        // it into the next paragraph.
        if let Some((from, to)) = selection
            && !line.soft
            && line.end < area.text.len()
            && from <= line.end
            && to > line.end
        {
            let end_x = text::measure(line_text, m.font_size, m.weight);
            tree.push(RenderCommand::FillRect {
                x: origin_x + end_x,
                y,
                width: NEWLINE_MARK,
                height: per,
                color: f.selection_bg,
                corner_radii: CornerRadii::ZERO,
            });
        }
        if !line_text.is_empty() {
            let spans = local.map_or_else(Vec::new, |(from, to)| {
                let mut spans = Vec::with_capacity(2);
                if from > 0 {
                    spans.push(TextSpan {
                        end: u32::try_from(from).unwrap_or(u32::MAX),
                        color: f.color,
                    });
                }
                spans.push(TextSpan {
                    end: u32::try_from(to).unwrap_or(u32::MAX),
                    color: f.selection_fg,
                });
                spans
            });
            tree.push(RenderCommand::RichText {
                x: origin_x,
                y,
                text: line_text.to_string(),
                spans,
                color: f.color,
                font_size: m.font_size,
                font_weight: m.weight,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        if f.focused && index == caret_line {
            textedit::push_caret(tree, origin_x + caret_px, y, per, f.color, f.caret_width);
        }
    }
    tree.unclip();
}

#[cfg(test)]
#[path = "textarea_tests.rs"]
mod tests;

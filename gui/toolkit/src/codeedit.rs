//! A code editor's model: a file's text ([`TextBuffer`]), any number of
//! selections, and the editing a programmer expects of them -- typing at every
//! caret at once, indentation that follows the code, tab stops, caret motion
//! that keeps its column, and an undo history that is a tree.
//!
//! # Why this exists
//!
//! `roadmap-detailed.md` §3.5, *Code-Aware TextEdit Widget*: `apps/editor` and
//! `apps/markdowneditor` each implement undo, find and a line-number gutter
//! of their own, on `Vec<String>` lines. This is the shared model they can
//! move onto; the buffer under it is [`crate::textbuffer`].
//!
//! State only, as with the single- and multi-line fields: the caller owns the
//! window, the keyboard and the drawing, and asks this to act.
//!
//! # Selections
//!
//! Every caret is a [`Selection`] -- an anchor and a head, byte offsets on
//! character boundaries; a caret is a selection whose two ends meet. There is
//! always at least one. They are kept sorted and never overlap: two that come
//! to touch or overlap after a move are merged into one, as every editor with
//! several carets does, so an edit is never made twice at one place.
//!
//! Every edit is made at every selection, as one batch
//! ([`TextBuffer::apply`]) and one undo step.
//!
//! # Finding and replacing
//!
//! A [`FindQuery`] -- plain text or a regular expression, case-sensitive or
//! not, whole words or not -- compiles to a [`Finder`], and the editor steps
//! through its matches (wrapping round the end), selects them all as carets,
//! replaces the one selected, or replaces every one as a single undo step.
//! Plain text is `textfind`'s search, whose case folding keeps offsets in the
//! text searched; regular expressions are the `regex` crate's, and a
//! replacement may name the match's groups (`$1`, `${name}`, `$$` for a `$`).
//! A match of nothing -- `a*` before a `b` -- is never a match: selecting
//! nothing is not finding something.
//!
//! # Columns
//!
//! Moving up and down keeps a *goal column* -- where the caret wanted to be,
//! not where a short line put it -- counted in display columns: a tab runs to
//! the next tab stop, every other character is one column. (A character drawn
//! two cells wide in a fixed-pitch face counts as one here; the drawing
//! measures what it draws, so only the goal of a vertical move is affected.)

use core::num::NonZeroUsize;
use core::ops::Range;

use crate::textbuffer::{Changes, Edit, EditError, TextBuffer};
use crate::undo::{Travel, UndoHistory};

/// How many steps the undo history keeps.
const HISTORY_LIMIT: usize = 1000;

/// The most a compiled regular expression may take, in bytes: generous for
/// anything a person types, and a bound on what a pathological one can cost.
const REGEX_SIZE_LIMIT: usize = 1 << 22;

/// What to look for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindQuery {
    /// The text, or the regular expression.
    pub pattern: String,
    /// Whether `pattern` is a regular expression rather than text.
    pub regex: bool,
    /// Whether upper and lower case are told apart.
    pub case_sensitive: bool,
    /// Whether a match must be a whole word: not preceded or followed by a
    /// letter, digit or underscore.
    pub whole_word: bool,
}

/// Why a query cannot be searched for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindError {
    /// The pattern is empty: there is nothing to look for.
    Empty,
    /// The pattern is not a regular expression the engine accepts; its
    /// reason, as it gives it.
    Pattern(String),
}

impl core::fmt::Display for FindError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("there is nothing to look for"),
            Self::Pattern(why) => write!(f, "not a regular expression: {why}"),
        }
    }
}

impl std::error::Error for FindError {}

/// A compiled [`FindQuery`].
#[derive(Clone, Debug)]
pub struct Finder {
    how: How,
}

#[derive(Clone, Debug)]
enum How {
    Plain {
        needle: String,
        case: textfind::Case,
        whole_word: bool,
    },
    Regex(regex::Regex),
}

impl Finder {
    /// Compile `query`.
    ///
    /// # Errors
    ///
    /// [`FindError::Empty`] for an empty pattern, [`FindError::Pattern`] for a
    /// regular expression the engine refuses -- or one so large it would
    /// cost more than four megabytes to compile (`REGEX_SIZE_LIMIT`).
    pub fn new(query: &FindQuery) -> Result<Self, FindError> {
        if query.pattern.is_empty() {
            return Err(FindError::Empty);
        }
        let how = if query.regex {
            let pattern = if query.whole_word {
                format!(r"\b(?:{})\b", query.pattern)
            } else {
                query.pattern.clone()
            };
            let compiled = regex::RegexBuilder::new(&pattern)
                .case_insensitive(!query.case_sensitive)
                .multi_line(true)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .map_err(|e| FindError::Pattern(e.to_string()))?;
            How::Regex(compiled)
        } else {
            How::Plain {
                needle: query.pattern.clone(),
                case: textfind::Case::sensitive(query.case_sensitive),
                whole_word: query.whole_word,
            }
        };
        Ok(Self { how })
    }

    /// The first match starting at or after `from` in `text`, never empty.
    #[must_use]
    pub fn find_from(&self, text: &str, from: usize) -> Option<Range<usize>> {
        let mut at = from;
        while at <= text.len() {
            if !text.is_char_boundary(at) {
                at = at.saturating_add(1);
                continue;
            }
            let found = match &self.how {
                How::Plain {
                    needle,
                    case,
                    whole_word,
                } => {
                    let (start, end) = textfind::find_from(text, needle, at, *case)?;
                    if *whole_word && !is_whole_word(text, start..end) {
                        // Not a whole word: look again from its next
                        // character, which may start one that is.
                        at = text
                            .get(start..)
                            .and_then(|rest| rest.chars().next())
                            .map_or(text.len().saturating_add(1), |c| {
                                start.saturating_add(c.len_utf8())
                            });
                        continue;
                    }
                    start..end
                }
                How::Regex(re) => re.find_at(text, at).map(|m| m.range())?,
            };
            if found.is_empty() {
                // Nothing matched here; step past the character and look on.
                at = text
                    .get(found.start..)
                    .and_then(|rest| rest.chars().next())
                    .map_or(text.len().saturating_add(1), |c| {
                        found.start.saturating_add(c.len_utf8())
                    });
                continue;
            }
            return Some(found);
        }
        None
    }

    /// Every match in `text`, in order, none overlapping and none empty.
    #[must_use]
    pub fn find_all(&self, text: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(found) = self.find_from(text, at) {
            at = found.end;
            out.push(found);
        }
        out
    }

    /// What replaces the match `found` in `text`: `with` itself for plain
    /// text; for a regular expression, `with` with the match's groups put in
    /// for `$1`, `${name}` and the like.
    #[must_use]
    pub fn replacement(&self, text: &str, found: Range<usize>, with: &str) -> String {
        match &self.how {
            How::Plain { .. } => with.to_owned(),
            How::Regex(re) => {
                let mut out = String::new();
                match re.captures_at(text, found.start) {
                    Some(caps) if caps.get(0).is_some_and(|m| m.range() == found) => {
                        caps.expand(with, &mut out);
                    }
                    _ => out.push_str(with),
                }
                out
            }
        }
    }
}

/// Whether `range` of `text` is a whole word: no word character just before
/// or just after it.
fn is_whole_word(text: &str, range: Range<usize>) -> bool {
    let before = text.get(..range.start).and_then(|t| t.chars().next_back());
    let after = text.get(range.end..).and_then(|t| t.chars().next());
    !before.is_some_and(is_word) && !after.is_some_and(is_word)
}

/// The widest a tab may be set.
pub const MAX_TAB_WIDTH: u8 = 16;

/// A selection: from `anchor`, where it was started, to `head`, where the
/// caret is. Byte offsets into the text, on character boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Where the selection was started; the end that stays put when it is
    /// extended.
    pub anchor: usize,
    /// Where the caret is.
    pub head: usize,
}

impl Selection {
    /// A caret at `offset`, selecting nothing.
    #[must_use]
    pub const fn caret(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    /// The text it covers, from its lower end to its higher.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    /// Whether it selects nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// How the editor indents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// How many columns a tab stop is, 1 to [`MAX_TAB_WIDTH`].
    pub tab_width: u8,
    /// Whether Tab and indentation insert spaces (to the next stop) rather
    /// than a tab character.
    pub insert_spaces: bool,
    /// Whether a new line starts with the indentation of the line before --
    /// one level more after an opening bracket.
    pub auto_indent: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            tab_width: 4,
            insert_spaces: true,
            auto_indent: true,
        }
    }
}

impl Options {
    /// The tab width, held to 1..=[`MAX_TAB_WIDTH`].
    fn tab(&self) -> usize {
        usize::from(self.tab_width.clamp(1, MAX_TAB_WIDTH))
    }

    /// One level of indentation, as text.
    fn indent_unit(&self) -> String {
        if self.insert_spaces {
            " ".repeat(self.tab())
        } else {
            "\t".to_owned()
        }
    }
}

/// One change at one place in a batch: what was there, what is there now.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Change {
    /// Where, in the text as it was before the batch.
    at: usize,
    removed: String,
    inserted: String,
}

/// One batch of changes -- one edit at every caret -- lowest offset first.
///
/// Positions are the text's before the batch, which is what making it again
/// needs; taking it back needs where each insertion sits *after* the batch,
/// which is its position moved by every change before it
/// ([`Batch::reverted`]).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Batch {
    changes: Vec<Change>,
}

impl Batch {
    /// The edits that make this batch, in offsets of the text before it.
    fn applied(&self) -> Vec<Edit> {
        self.changes
            .iter()
            .map(|c| Edit {
                range: c.at..c.at.saturating_add(c.removed.len()),
                text: c.inserted.clone(),
            })
            .collect()
    }

    /// The edits that take it back, in offsets of the text after it.
    fn reverted(&self) -> Vec<Edit> {
        let mut shift: isize = 0;
        self.changes
            .iter()
            .map(|c| {
                let at = offset_by(c.at, shift);
                shift = shift
                    .saturating_add(signed(c.inserted.len()))
                    .saturating_sub(signed(c.removed.len()));
                Edit {
                    range: at..at.saturating_add(c.inserted.len()),
                    text: c.removed.clone(),
                }
            })
            .collect()
    }
}

/// One undo step: the batches that made it, in the order they were made --
/// one, or a run of typing gathered into one step -- with the selections
/// either side.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Step {
    batches: Vec<Batch>,
    before: Vec<Selection>,
    after: Vec<Selection>,
    /// What kind of edit made it, for gathering typing into one step.
    kind: StepKind,
}

/// What kind of edit a step was, so a run of the same kind is one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepKind {
    /// Characters typed at the carets, no newline among them.
    Typing,
    /// Characters deleted one at a time, backwards.
    Backspace,
    /// Characters deleted one at a time, forwards.
    Delete,
    /// Anything else: a paste, a newline, an indent -- never merged.
    Other,
}

/// A code editor's text, selections and history. See the module docs.
#[derive(Debug)]
pub struct CodeEditor {
    buffer: TextBuffer,
    /// Sorted by start, never overlapping; never empty.
    selections: Vec<Selection>,
    /// Which selection is the primary one -- the one a single-caret command
    /// keeps, and the one the view follows.
    primary: usize,
    /// Each selection's goal column for vertical moves; `None` once a
    /// horizontal move or an edit has placed the caret.
    goals: Vec<Option<usize>>,
    options: Options,
    history: UndoHistory<Step>,
    /// Whether the last step may take in the next of its kind: false after
    /// any move, undo or unrelated edit.
    open_run: bool,
}

impl Default for CodeEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeEditor {
    /// An empty editor, the caret at the start.
    #[must_use]
    pub fn new() -> Self {
        Self::from_text("")
    }

    /// An editor holding `text`, the caret at the start.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self {
            buffer: TextBuffer::from_text(text),
            selections: vec![Selection::caret(0)],
            primary: 0,
            goals: vec![None],
            options: Options::default(),
            history: UndoHistory::new(
                NonZeroUsize::new(HISTORY_LIMIT).unwrap_or(NonZeroUsize::MIN),
            ),
            open_run: false,
        }
    }

    /// The text.
    #[must_use]
    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    /// The whole text.
    #[must_use]
    pub fn text(&self) -> String {
        self.buffer.text()
    }

    /// Changed by every change to the text and by nothing else, so a view
    /// knows when what it drew is stale: the buffer's
    /// [`revision`](TextBuffer::revision), which no other text in the process
    /// has had.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.buffer.revision()
    }

    /// Every change to the text since this was last called, as the buffer
    /// journalled it ([`TextBuffer::take_changes`]): what a syntax
    /// highlighter re-reads the text by.
    pub fn take_changes(&mut self) -> Changes {
        self.buffer.take_changes()
    }

    /// The selections, sorted, never overlapping, at least one.
    #[must_use]
    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    /// The primary selection.
    #[must_use]
    pub fn primary(&self) -> Selection {
        self.selections
            .get(self.primary)
            .copied()
            .unwrap_or(Selection::caret(0))
    }

    /// How the editor indents.
    #[must_use]
    pub fn options(&self) -> Options {
        self.options
    }

    /// Indent differently from now on.
    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }

    /// Put the selections at `selections`: sorted, merged where they touch,
    /// held to the text and to character boundaries. The last is primary. An
    /// empty list is a caret at the start.
    pub fn set_selections(&mut self, selections: Vec<Selection>) {
        let primary = selections.last().copied();
        self.selections = selections;
        self.normalise(primary);
        self.goals = vec![None; self.selections.len()];
        self.open_run = false;
    }

    /// Replace the selections with `selections`, one for one, keeping the
    /// primary where it was.
    fn set_selections_keeping_primary(&mut self, selections: Vec<Selection>) {
        let primary = selections.get(self.primary).copied();
        self.selections = selections;
        self.normalise(primary);
        self.goals = vec![None; self.selections.len()];
        self.open_run = false;
    }

    /// Select the whole text.
    pub fn select_all(&mut self) {
        let end = self.buffer.len();
        self.set_selections(vec![Selection {
            anchor: 0,
            head: end,
        }]);
    }

    /// Select the word at every caret (a double-click's selection); a
    /// selection is left as it is.
    pub fn select_word(&mut self) {
        let words: Vec<Selection> = self
            .selections
            .iter()
            .map(|s| {
                if !s.is_empty() {
                    return *s;
                }
                let word = self.word_at(s.head);
                Selection {
                    anchor: word.start,
                    head: word.end,
                }
            })
            .collect();
        self.set_selections_keeping_primary(words);
    }

    /// Select the whole line at every caret, its newline included (a
    /// triple-click's selection, or Ctrl+L); again on a selection of whole
    /// lines, take in the next line too.
    pub fn select_line(&mut self) {
        let lines: Vec<Selection> = self
            .selections
            .iter()
            .map(|s| {
                let range = s.range();
                let first = self.buffer.line_of(range.start).unwrap_or(0);
                let start = self.buffer.line_start(first).unwrap_or(0);
                // A selection already ending at a line's start takes in that
                // line; otherwise the line its end is on.
                let from = if range.end > range.start
                    && self
                        .buffer
                        .line_start(self.buffer.line_of(range.end).unwrap_or(0))
                        == Some(range.end)
                {
                    range.end
                } else {
                    range.end.max(start)
                };
                let end = self.whole_line_range(from).end;
                Selection {
                    anchor: start,
                    head: end,
                }
            })
            .collect();
        self.set_selections_keeping_primary(lines);
    }

    /// Keep only the primary selection.
    pub fn collapse_to_primary(&mut self) {
        let keep = self.primary();
        self.set_selections(vec![keep]);
    }

    // ------------------------------------------------------------------
    // Editing
    // ------------------------------------------------------------------

    /// Type `text` at every caret, replacing what each selects.
    ///
    /// Typing without a newline is gathered into one undo step while it goes
    /// on; anything with a newline is a step of its own.
    pub fn type_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let kind = if text.contains('\n') {
            StepKind::Other
        } else {
            StepKind::Typing
        };
        let edits: Vec<(Range<usize>, String)> = self
            .selections
            .iter()
            .map(|s| (s.range(), text.to_owned()))
            .collect();
        self.edit(edits, kind, |_, _, inserted_end| {
            Selection::caret(inserted_end)
        });
    }

    /// Paste `text`. With as many carets as `text` has lines, each caret
    /// takes one line -- what copying from several carets and pasting at as
    /// many gives back; otherwise every caret takes all of it.
    pub fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let lines: Vec<&str> = text.split('\n').collect();
        let one_each = self.selections.len() > 1 && lines.len() == self.selections.len();
        let edits: Vec<(Range<usize>, String)> = self
            .selections
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let piece = if one_each {
                    lines.get(i).copied().unwrap_or("")
                } else {
                    text
                };
                (s.range(), piece.to_owned())
            })
            .collect();
        self.edit(edits, StepKind::Other, |_, _, end| Selection::caret(end));
    }

    /// What is selected, for copying: each selection's text, joined by
    /// newlines when there are several -- so a paste at as many carets puts
    /// each back. A caret with nothing selected copies its whole line.
    #[must_use]
    pub fn copy(&self) -> String {
        let pieces: Vec<String> = self
            .selections
            .iter()
            .map(|s| {
                let range = if s.is_empty() {
                    self.whole_line_range(s.head)
                } else {
                    s.range()
                };
                self.buffer.slice(range).unwrap_or_default()
            })
            .collect();
        pieces.join("\n")
    }

    /// Copy, then delete what was selected. A caret with nothing selected
    /// cuts its whole line, the newline with it.
    pub fn cut(&mut self) -> String {
        let copied = self.copy();
        let edits: Vec<(Range<usize>, String)> = self
            .selections
            .iter()
            .map(|s| {
                let range = if s.is_empty() {
                    self.whole_line_range(s.head)
                } else {
                    s.range()
                };
                (range, String::new())
            })
            .collect();
        self.edit(edits, StepKind::Other, |_, start, _| {
            Selection::caret(start)
        });
        copied
    }

    /// Start a new line at every caret, indented as the code around it
    /// asks (see [`Options::auto_indent`]): the line's own indentation, one
    /// level more after an opening bracket -- and when the caret sits between
    /// a bracket and its closer, the closer goes to a line of its own at the
    /// outer level.
    pub fn newline(&mut self) {
        let unit = self.options.indent_unit();
        let auto = self.options.auto_indent;
        let mut carets_after: Vec<usize> = Vec::new();
        let edits: Vec<(Range<usize>, String)> = self
            .selections
            .iter()
            .map(|s| {
                let range = s.range();
                if !auto {
                    carets_after.push(1);
                    return (range, "\n".to_owned());
                }
                let indent = self.indentation_of_line(range.start);
                let before = self.buffer.char_before(range.start);
                let after = self.buffer.char_at(range.end);
                let opens = before.is_some_and(|c| matches!(c, '{' | '(' | '['));
                let closes_pair =
                    opens && after.is_some_and(|c| Some(c) == before.and_then(closer_of));
                let mut text = String::from("\n");
                text.push_str(&indent);
                if opens {
                    text.push_str(&unit);
                }
                let caret = text.len();
                if closes_pair {
                    text.push('\n');
                    text.push_str(&indent);
                }
                carets_after.push(caret);
                (range, text)
            })
            .collect();
        self.edit(edits, StepKind::Other, move |i, start, _| {
            let caret = carets_after.get(i).copied().unwrap_or(0);
            Selection::caret(start.saturating_add(caret))
        });
    }

    /// Delete backwards at every caret: what is selected, or the character
    /// before the caret -- or, in the indentation of a line indented with
    /// spaces, back to the previous tab stop.
    pub fn backspace(&mut self) {
        let edits: Vec<(Range<usize>, String)> = self
            .selections
            .iter()
            .map(|s| {
                if !s.is_empty() {
                    return (s.range(), String::new());
                }
                (self.backspace_range(s.head), String::new())
            })
            .collect();
        self.edit(edits, StepKind::Backspace, |_, start, _| {
            Selection::caret(start)
        });
    }

    /// Delete forwards at every caret: what is selected, or the character
    /// after the caret.
    pub fn delete_forward(&mut self) {
        let edits: Vec<(Range<usize>, String)> = self
            .selections
            .iter()
            .map(|s| {
                if !s.is_empty() {
                    return (s.range(), String::new());
                }
                let end = self
                    .buffer
                    .char_at(s.head)
                    .map_or(s.head, |c| s.head.saturating_add(c.len_utf8()));
                (s.head..end, String::new())
            })
            .collect();
        self.edit(edits, StepKind::Delete, |_, start, _| {
            Selection::caret(start)
        });
    }

    /// Delete back to the start of the word before every caret (or what is
    /// selected).
    pub fn delete_word_back(&mut self) {
        let edits: Vec<(Range<usize>, String)> = self
            .selections
            .iter()
            .map(|s| {
                if !s.is_empty() {
                    return (s.range(), String::new());
                }
                (self.word_left_of(s.head)..s.head, String::new())
            })
            .collect();
        self.edit(edits, StepKind::Other, |_, start, _| {
            Selection::caret(start)
        });
    }

    /// Delete forward to the end of the word after every caret (or what is
    /// selected).
    pub fn delete_word_forward(&mut self) {
        let edits: Vec<(Range<usize>, String)> = self
            .selections
            .iter()
            .map(|s| {
                if !s.is_empty() {
                    return (s.range(), String::new());
                }
                (s.head..self.word_right_of(s.head), String::new())
            })
            .collect();
        self.edit(edits, StepKind::Other, |_, start, _| {
            Selection::caret(start)
        });
    }

    /// Tab: with a selection that spans lines, indent every line it touches;
    /// otherwise insert a tab -- spaces to the next tab stop when
    /// [`Options::insert_spaces`], a tab character when not.
    pub fn tab(&mut self) {
        if self.selections.iter().any(|s| self.spans_lines(s)) {
            self.indent();
            return;
        }
        let tab = self.options.tab();
        let spaces = self.options.insert_spaces;
        let edits: Vec<(Range<usize>, String)> = self
            .selections
            .iter()
            .map(|s| {
                let text = if spaces {
                    let column = self.display_column(s.range().start);
                    " ".repeat(to_next_stop(column, tab))
                } else {
                    "\t".to_owned()
                };
                (s.range(), text)
            })
            .collect();
        self.edit(edits, StepKind::Other, |_, _, end| Selection::caret(end));
    }

    /// Indent every line the selections touch by one level, keeping what is
    /// selected selected.
    pub fn indent(&mut self) {
        let unit = self.options.indent_unit();
        let lines = self.touched_lines();
        let edits: Vec<(Range<usize>, String)> = lines
            .iter()
            .filter_map(|&line| {
                let start = self.buffer.line_start(line)?;
                // An empty line is left empty: indentation with nothing after
                // it is trailing whitespace.
                let empty = self.buffer.line_range(line).is_some_and(|r| r.is_empty());
                (!empty).then(|| (start..start, unit.clone()))
            })
            .collect();
        self.edit_keeping_selections(edits);
    }

    /// Take one level of indentation off every line the selections touch
    /// (Shift+Tab): a tab, or up to a tab width of spaces.
    pub fn dedent(&mut self) {
        let tab = self.options.tab();
        let lines = self.touched_lines();
        let edits: Vec<(Range<usize>, String)> = lines
            .iter()
            .filter_map(|&line| {
                let range = self.buffer.line_range(line)?;
                let mut end = range.start;
                for (at, c) in self.buffer.chars(range.start) {
                    if at >= range.end {
                        break;
                    }
                    if c == '\t' {
                        if end == range.start {
                            end = at.saturating_add(1);
                        }
                        break;
                    }
                    if c != ' ' || at.saturating_sub(range.start) >= tab {
                        break;
                    }
                    end = at.saturating_add(1);
                }
                (end > range.start).then(|| (range.start..end, String::new()))
            })
            .collect();
        self.edit_keeping_selections(edits);
    }

    /// Take back the last step. Answers whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.history.undo() else {
            return false;
        };
        self.revert(&step);
        true
    }

    /// Make again the step last taken back. Answers whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.history.redo() else {
            return false;
        };
        self.reapply(&step);
        true
    }

    /// Go back to the state before this one in the order states were first
    /// reached, across branches of the history. Answers whether it moved.
    pub fn earlier(&mut self) -> bool {
        let journey = self.history.earlier();
        self.travel(journey)
    }

    /// The inverse of [`earlier`](Self::earlier).
    pub fn later(&mut self) -> bool {
        let journey = self.history.later();
        self.travel(journey)
    }

    // ------------------------------------------------------------------
    // Moving
    // ------------------------------------------------------------------

    /// Move every caret one character left, or -- with a selection and not
    /// extending -- to the selection's start. `extend` keeps each anchor.
    pub fn move_left(&mut self, extend: bool) {
        self.move_each(extend, |editor, s| {
            if !extend && !s.is_empty() {
                return s.range().start;
            }
            editor
                .buffer
                .chars_rev(s.head)
                .next()
                .map_or(s.head, |(at, _)| at)
        });
    }

    /// As [`move_left`](Self::move_left), rightwards.
    pub fn move_right(&mut self, extend: bool) {
        self.move_each(extend, |editor, s| {
            if !extend && !s.is_empty() {
                return s.range().end;
            }
            editor
                .buffer
                .char_at(s.head)
                .map_or(s.head, |c| s.head.saturating_add(c.len_utf8()))
        });
    }

    /// Move every caret to the start of the word before it.
    pub fn move_word_left(&mut self, extend: bool) {
        self.move_each(extend, |editor, s| editor.word_left_of(s.head));
    }

    /// Move every caret to the end of the word after it.
    pub fn move_word_right(&mut self, extend: bool) {
        self.move_each(extend, |editor, s| editor.word_right_of(s.head));
    }

    /// Home: to the line's first non-blank character, or to the line's
    /// start when already there -- the "smart home" every code editor has.
    pub fn move_home(&mut self, extend: bool) {
        self.move_each(extend, |editor, s| {
            let line = editor.buffer.line_of(s.head).unwrap_or(0);
            let Some(range) = editor.buffer.line_range(line) else {
                return s.head;
            };
            let first = editor
                .buffer
                .chars(range.start)
                .take_while(|&(at, c)| at < range.end && (c == ' ' || c == '\t'))
                .last()
                .map_or(range.start, |(at, c)| at.saturating_add(c.len_utf8()));
            if s.head == first { range.start } else { first }
        });
    }

    /// End: to the end of the line.
    pub fn move_end(&mut self, extend: bool) {
        self.move_each(extend, |editor, s| {
            let line = editor.buffer.line_of(s.head).unwrap_or(0);
            editor.buffer.line_range(line).map_or(s.head, |r| r.end)
        });
    }

    /// To the start of the text.
    pub fn move_to_start(&mut self, extend: bool) {
        self.move_each(extend, |_, _| 0);
    }

    /// To the end of the text.
    pub fn move_to_end(&mut self, extend: bool) {
        let end = self.buffer.len();
        self.move_each(extend, move |_, _| end);
    }

    /// Move every caret `lines` lines up, keeping its goal column; to the
    /// start of the text from the first line.
    pub fn move_up(&mut self, lines: usize, extend: bool) {
        self.move_vertically(lines, false, extend);
    }

    /// Move every caret `lines` lines down, keeping its goal column; to the
    /// end of the text from the last line.
    pub fn move_down(&mut self, lines: usize, extend: bool) {
        self.move_vertically(lines, true, extend);
    }

    // ------------------------------------------------------------------
    // Several carets
    // ------------------------------------------------------------------

    /// Add a caret on the line above the topmost one (or below the
    /// bottommost), at its goal column.
    pub fn add_caret_vertically(&mut self, down: bool) {
        let from = if down {
            self.selections.last().copied()
        } else {
            self.selections.first().copied()
        };
        let Some(from) = from else { return };
        let goal = self.display_column(from.head);
        let line = self.buffer.line_of(from.head).unwrap_or(0);
        let target = if down {
            line.checked_add(1)
                .filter(|&l| l < self.buffer.line_count())
        } else {
            line.checked_sub(1)
        };
        let Some(target) = target else { return };
        let at = self.offset_at_column(target, goal);
        let mut all = self.selections.clone();
        all.push(Selection::caret(at));
        self.set_selections(all);
    }

    /// Select the next place the primary selection's text appears (Ctrl+D):
    /// with nothing selected, first select the word at the caret.
    pub fn add_next_occurrence(&mut self) {
        let primary = self.primary();
        if primary.is_empty() {
            let word = self.word_at(primary.head);
            if !word.is_empty() {
                let mut all = self.selections.clone();
                if let Some(slot) = all.get_mut(self.primary) {
                    *slot = Selection {
                        anchor: word.start,
                        head: word.end,
                    };
                }
                let keep = all.get(self.primary).copied();
                self.selections = all;
                self.normalise(keep);
            }
            return;
        }
        let needle = self.buffer.slice(primary.range()).unwrap_or_default();
        let text = self.buffer.text();
        let from = self
            .selections
            .iter()
            .map(|s| s.range().end)
            .max()
            .unwrap_or(0);
        let found = text
            .get(from..)
            .and_then(|rest| rest.find(&needle).map(|i| from.saturating_add(i)))
            .or_else(|| text.find(&needle));
        let Some(start) = found else { return };
        let next = Selection {
            anchor: start,
            head: start.saturating_add(needle.len()),
        };
        if self.selections.iter().any(|s| s.range() == next.range()) {
            return;
        }
        let mut all = self.selections.clone();
        all.push(next);
        self.set_selections(all);
    }

    /// Select a block: on every line from `anchor`'s to `head`'s, the columns
    /// between the two points' display columns -- one selection per line,
    /// each held to its line's end. `anchor` and `head` are offsets.
    pub fn select_block(&mut self, anchor: usize, head: usize) {
        let (Ok(anchor_line), Ok(head_line)) =
            (self.buffer.line_of(anchor), self.buffer.line_of(head))
        else {
            return;
        };
        let (a_col, h_col) = (self.display_column(anchor), self.display_column(head));
        let (low, high) = (anchor_line.min(head_line), anchor_line.max(head_line));
        let mut selections = Vec::new();
        for line in low..=high {
            selections.push(Selection {
                anchor: self.offset_at_column(line, a_col),
                head: self.offset_at_column(line, h_col),
            });
        }
        // The primary is the head's line, which `set_selections` takes last.
        if head_line < anchor_line {
            selections.reverse();
        }
        let primary = selections.last().copied();
        self.selections = selections;
        self.goals = vec![None; self.selections.len()];
        self.open_run = false;
        // Block rows never overlap -- one per line -- but empty rows at one
        // offset (a short line) would, so merge as usual.
        self.normalise(primary);
    }

    // ------------------------------------------------------------------
    // Find and replace
    // ------------------------------------------------------------------

    /// Every match of `finder` in the text.
    #[must_use]
    pub fn find_all(&self, finder: &Finder) -> Vec<Range<usize>> {
        finder.find_all(&self.buffer.text())
    }

    /// Select the next match after the primary selection -- or, `backwards`,
    /// the one before it -- going round the end of the text to the other if
    /// there is none that way. The match is the only selection afterwards.
    /// Answers whether there was a match at all.
    pub fn find_next(&mut self, finder: &Finder, backwards: bool) -> bool {
        let text = self.buffer.text();
        let primary = self.primary().range();
        let found = if backwards {
            let all = finder.find_all(&text);
            all.iter()
                .rev()
                .find(|m| m.end <= primary.start && *m != &primary)
                .or_else(|| all.last())
                .cloned()
        } else {
            finder
                .find_from(&text, primary.end)
                .or_else(|| finder.find_from(&text, 0))
        };
        let Some(found) = found else {
            return false;
        };
        self.set_selections(vec![Selection {
            anchor: found.start,
            head: found.end,
        }]);
        true
    }

    /// Select every match as a selection of its own, to edit them all at
    /// once. Answers how many there were; with none, the selections are left.
    pub fn select_all_matches(&mut self, finder: &Finder) -> usize {
        let all = finder.find_all(&self.buffer.text());
        if all.is_empty() {
            return 0;
        }
        let count = all.len();
        self.set_selections(
            all.into_iter()
                .map(|m| Selection {
                    anchor: m.start,
                    head: m.end,
                })
                .collect(),
        );
        count
    }

    /// Replace the match the primary selection is on with `with`, then
    /// select the next one; when the selection is not on a match, only
    /// select the next. Answers whether a replacement was made.
    pub fn replace_next(&mut self, finder: &Finder, with: &str) -> bool {
        let text = self.buffer.text();
        let primary = self.primary().range();
        let on_match =
            !primary.is_empty() && finder.find_from(&text, primary.start) == Some(primary.clone());
        if !on_match {
            self.find_next(finder, false);
            return false;
        }
        let replacement = finder.replacement(&text, primary.clone(), with);
        let end = primary.start.saturating_add(replacement.len());
        self.set_selections(vec![Selection {
            anchor: primary.start,
            head: primary.end,
        }]);
        self.edit(
            vec![(primary, replacement)],
            StepKind::Other,
            |_, start, _| Selection::caret(start),
        );
        self.set_selections(vec![Selection::caret(end)]);
        self.find_next(finder, false);
        true
    }

    /// Replace every match with `with`, as one undo step. Answers how many
    /// were replaced.
    pub fn replace_all(&mut self, finder: &Finder, with: &str) -> usize {
        let text = self.buffer.text();
        let edits: Vec<(Range<usize>, String)> = finder
            .find_all(&text)
            .into_iter()
            .map(|m| {
                let replacement = finder.replacement(&text, m.clone(), with);
                (m, replacement)
            })
            .collect();
        let count = edits.len();
        if count > 0 {
            self.edit_keeping_selections(edits);
        }
        count
    }

    // ------------------------------------------------------------------
    // Finding things in the code
    // ------------------------------------------------------------------

    /// The word around `offset`: the run of letters, digits and underscores
    /// it is in or touches; empty at `offset` when there is none.
    #[must_use]
    pub fn word_at(&self, offset: usize) -> Range<usize> {
        let start = self
            .buffer
            .chars_rev(offset)
            .take_while(|&(_, c)| is_word(c))
            .last()
            .map_or(offset, |(at, _)| at);
        let end = self
            .buffer
            .chars(offset)
            .take_while(|&(_, c)| is_word(c))
            .last()
            .map_or(offset, |(at, c)| at.saturating_add(c.len_utf8()));
        start..end
    }

    /// The bracket matching the one at `offset` -- or, failing that, the one
    /// just before it -- as its offset: the next closer at the same depth for
    /// an opener, the previous opener for a closer. `None` when neither is a
    /// bracket, or it has no match within [`BRACKET_SCAN_LIMIT`] characters.
    ///
    /// Brackets in strings and comments are counted like any other: telling
    /// them apart is a language's business, and a language-aware matcher can
    /// take over from this one.
    #[must_use]
    pub fn matching_bracket(&self, offset: usize) -> Option<(usize, usize)> {
        let candidates = [
            self.buffer.char_at(offset).map(|c| (offset, c)),
            self.buffer.chars_rev(offset).next(),
        ];
        candidates
            .into_iter()
            .flatten()
            .find_map(|(at, c)| self.match_from(at, c).map(|other| (at, other)))
    }

    // ------------------------------------------------------------------
    // Inside
    // ------------------------------------------------------------------

    /// Make `edits` -- a range and its replacement per selection, in the
    /// selections' order -- as one batch, and put each selection where
    /// `place` says: given the edit's index in `edits`, its start and the end
    /// of what it inserted, in offsets of the text after the batch.
    fn edit(
        &mut self,
        edits: Vec<(Range<usize>, String)>,
        kind: StepKind,
        mut place: impl FnMut(usize, usize, usize) -> Selection,
    ) {
        // Several selections may ask for overlapping ranges (two carets that
        // backspace into one character): keep the first, with its index.
        let mut unique: Vec<(usize, Range<usize>, String)> = Vec::with_capacity(edits.len());
        for (index, (range, text)) in edits.into_iter().enumerate() {
            let clash = unique.iter().any(|(_, r, _)| {
                (r.start < range.end && range.start < r.end) || (*r == range && !r.is_empty())
            });
            if !clash {
                unique.push((index, range, text));
            }
        }
        if unique.iter().all(|(_, r, t)| r.is_empty() && t.is_empty()) {
            self.open_run = false;
            return;
        }
        let before = self.selections.clone();
        let pairs: Vec<(Range<usize>, String)> = unique
            .iter()
            .map(|(_, r, t)| (r.clone(), t.clone()))
            .collect();
        let Ok(batch) = self.apply_edits(&pairs) else {
            return;
        };
        // Where each edit's start lands once the edits before it are made.
        let mut shift: isize = 0;
        let mut order: Vec<usize> = (0..unique.len()).collect();
        order.sort_by_key(|&i| unique.get(i).map_or(0, |(_, r, _)| r.start));
        let mut placed: Vec<(usize, Selection)> = Vec::with_capacity(unique.len());
        for i in order {
            let Some((index, range, text)) = unique.get(i) else {
                continue;
            };
            let start = offset_by(range.start, shift);
            let end = start.saturating_add(text.len());
            placed.push((*index, place(*index, start, end)));
            shift = shift
                .saturating_add(signed(text.len()))
                .saturating_sub(signed(range.end.saturating_sub(range.start)));
        }
        let primary = placed
            .iter()
            .find(|(index, _)| *index == self.primary)
            .or(placed.last())
            .map(|(_, s)| *s);
        self.selections = placed.into_iter().map(|(_, s)| s).collect();
        self.normalise(primary);
        self.goals = vec![None; self.selections.len()];
        self.record(Step {
            batches: vec![batch],
            before,
            after: self.selections.clone(),
            kind,
        });
    }

    /// Make edits at line starts and leave every selection covering the same
    /// text it covered -- for indenting, where the selections are the lines
    /// the user chose and must stay chosen.
    fn edit_keeping_selections(&mut self, edits: Vec<(Range<usize>, String)>) {
        if edits.is_empty() {
            return;
        }
        let before = self.selections.clone();
        let Ok(batch) = self.apply_edits(&edits) else {
            return;
        };
        let moved: Vec<Selection> = before
            .iter()
            .map(|s| Selection {
                anchor: map_offset(s.anchor, &edits),
                head: map_offset(s.head, &edits),
            })
            .collect();
        let primary = moved.get(self.primary).copied();
        self.selections = moved;
        self.normalise(primary);
        self.goals = vec![None; self.selections.len()];
        self.record(Step {
            batches: vec![batch],
            before,
            after: self.selections.clone(),
            kind: StepKind::Other,
        });
    }

    /// Apply `edits` to the buffer as one batch, answering the batch as it
    /// was made, each change with what it removed.
    fn apply_edits(&mut self, edits: &[(Range<usize>, String)]) -> Result<Batch, EditError> {
        let mut changes: Vec<Change> = edits
            .iter()
            .map(|(range, text)| Change {
                at: range.start,
                removed: self.buffer.slice(range.clone()).unwrap_or_default(),
                inserted: text.clone(),
            })
            .collect();
        let batch: Vec<Edit> = edits
            .iter()
            .map(|(range, text)| Edit {
                range: range.clone(),
                text: text.clone(),
            })
            .collect();
        self.buffer.apply(&batch)?;
        changes.sort_by_key(|c| c.at);
        Ok(Batch { changes })
    }

    /// Record `step`, or fold it into the last step when both are the same
    /// run of typing or deleting -- a word at a time: typing a blank after a
    /// word starts a new step, so undo takes back words, not the paragraph.
    fn record(&mut self, step: Step) {
        let starts_word = |step: &Step| {
            step.batches
                .first()
                .and_then(|b| b.changes.first())
                .and_then(|c| c.inserted.chars().next())
                .is_some_and(char::is_whitespace)
        };
        let ended_word = |step: &Step| {
            step.batches
                .last()
                .and_then(|b| b.changes.first())
                .and_then(|c| c.inserted.chars().last())
                .is_some_and(|c| !c.is_whitespace())
        };
        let new_word = step.kind == StepKind::Typing && starts_word(&step);
        let merge = self.open_run
            && step.kind != StepKind::Other
            && self.history.last_mut().is_some_and(|last| {
                last.kind == step.kind
                    && last.after == step.before
                    && !(new_word && ended_word(last))
            });
        if merge {
            if let Some(last) = self.history.last_mut() {
                last.batches.extend(step.batches);
                last.after = step.after;
            }
        } else {
            self.history.record(step);
        }
        self.open_run = true;
    }

    /// Take `step` back: its batches, last first, each by putting back what
    /// it removed.
    fn revert(&mut self, step: &Step) {
        for batch in step.batches.iter().rev() {
            // Refused only if the text is not what the step left, which the
            // history's order guarantees it is; nothing to do but go on.
            let _ = self.buffer.apply(&batch.reverted());
        }
        self.after_travel(step.before.clone());
    }

    /// Make `step` again: its batches, first first.
    fn reapply(&mut self, step: &Step) {
        for batch in &step.batches {
            let _ = self.buffer.apply(&batch.applied());
        }
        self.after_travel(step.after.clone());
    }

    fn travel(&mut self, journey: Vec<Travel<Step>>) -> bool {
        let moved = !journey.is_empty();
        for leg in journey {
            match leg {
                Travel::Undo(step) => self.revert(&step),
                Travel::Redo(step) => self.reapply(&step),
            }
        }
        moved
    }

    fn after_travel(&mut self, selections: Vec<Selection>) {
        let primary = selections.last().copied();
        self.selections = selections;
        self.normalise(primary);
        self.goals = vec![None; self.selections.len()];
        self.open_run = false;
    }

    /// Move every selection's head to where `to` says, keeping its anchor
    /// when `extend`.
    fn move_each(&mut self, extend: bool, mut to: impl FnMut(&Self, Selection) -> usize) {
        let moved: Vec<Selection> = self
            .selections
            .iter()
            .map(|&s| {
                let head = to(self, s);
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
        let primary = moved.get(self.primary).copied();
        self.selections = moved;
        self.normalise(primary);
        self.goals = vec![None; self.selections.len()];
        self.open_run = false;
    }

    fn move_vertically(&mut self, lines: usize, down: bool, extend: bool) {
        let last_line = self.buffer.line_count().saturating_sub(1);
        let goals = self.goals.clone();
        let moved: Vec<(Selection, usize)> = self
            .selections
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let goal = goals
                    .get(i)
                    .copied()
                    .flatten()
                    .unwrap_or_else(|| self.display_column(s.head));
                let line = self.buffer.line_of(s.head).unwrap_or(0);
                let head = if down {
                    if line >= last_line {
                        self.buffer.len()
                    } else {
                        self.offset_at_column(line.saturating_add(lines).min(last_line), goal)
                    }
                } else if line == 0 {
                    0
                } else {
                    self.offset_at_column(line.saturating_sub(lines), goal)
                };
                let s = if extend {
                    Selection {
                        anchor: s.anchor,
                        head,
                    }
                } else {
                    Selection::caret(head)
                };
                (s, goal)
            })
            .collect();
        let primary = moved.get(self.primary).map(|(s, _)| *s);
        let (selections, kept_goals): (Vec<Selection>, Vec<usize>) = moved.into_iter().unzip();
        self.selections = selections;
        self.normalise(primary);
        // The goals survive the move -- that is what they are for -- unless
        // merging changed which selection is which.
        self.goals = if self.selections.len() == kept_goals.len() {
            kept_goals.into_iter().map(Some).collect()
        } else {
            vec![None; self.selections.len()]
        };
        self.open_run = false;
    }

    /// Sort, hold to the text and to character boundaries, and merge
    /// selections that touch or overlap; make the one equal to `primary`
    /// (or containing its head) primary.
    fn normalise(&mut self, primary: Option<Selection>) {
        let len = self.buffer.len();
        let mut all: Vec<Selection> = self
            .selections
            .iter()
            .map(|s| Selection {
                anchor: self.floor_boundary(s.anchor.min(len)),
                head: self.floor_boundary(s.head.min(len)),
            })
            .collect();
        if all.is_empty() {
            all.push(Selection::caret(0));
        }
        all.sort_by_key(|s| (s.range().start, s.range().end));
        let mut merged: Vec<Selection> = Vec::with_capacity(all.len());
        for s in all {
            match merged.last_mut() {
                Some(last)
                    if s.range().start < last.range().end
                        || (s.range().start == last.range().end
                            && (s.is_empty() || last.is_empty())) =>
                {
                    let start = last.range().start.min(s.range().start);
                    let end = last.range().end.max(s.range().end);
                    // Keep the direction of the one extended further.
                    *last = if s.head >= last.head {
                        Selection {
                            anchor: start,
                            head: end,
                        }
                    } else {
                        Selection {
                            anchor: end,
                            head: start,
                        }
                    };
                    if last.anchor == last.head {
                        *last = Selection::caret(start);
                    }
                }
                _ => merged.push(s),
            }
        }
        self.primary = primary
            .and_then(|p| {
                merged.iter().position(|s| *s == p).or_else(|| {
                    merged
                        .iter()
                        .position(|s| s.range().contains(&p.head) || s.head == p.head)
                })
            })
            .unwrap_or(merged.len().saturating_sub(1));
        self.selections = merged;
    }

    /// `offset`, moved back onto a character boundary.
    fn floor_boundary(&self, offset: usize) -> usize {
        let mut at = offset;
        while at > 0 && self.buffer.char_at(at).is_none() && at < self.buffer.len() {
            at = at.saturating_sub(1);
        }
        at
    }

    /// The display column `offset` is at on its line: tabs run to the next
    /// stop, every other character is one column.
    fn display_column(&self, offset: usize) -> usize {
        let line = self.buffer.line_of(offset).unwrap_or(0);
        let start = self.buffer.line_start(line).unwrap_or(0);
        let tab = self.options.tab();
        let mut column: usize = 0;
        for (at, c) in self.buffer.chars(start) {
            if at >= offset {
                break;
            }
            column = if c == '\t' {
                column.saturating_add(to_next_stop(column, tab))
            } else {
                column.saturating_add(1)
            };
        }
        column
    }

    /// The offset on `line` nearest display column `column`, held to the
    /// line's end.
    fn offset_at_column(&self, line: usize, column: usize) -> usize {
        let Some(range) = self.buffer.line_range(line) else {
            return self.buffer.len();
        };
        let tab = self.options.tab();
        let mut at_column: usize = 0;
        for (at, c) in self.buffer.chars(range.start) {
            if at >= range.end || at_column >= column {
                return at;
            }
            let next = if c == '\t' {
                at_column.saturating_add(to_next_stop(at_column, tab))
            } else {
                at_column.saturating_add(1)
            };
            // Past the goal inside a tab: the nearer side.
            if next > column {
                return if column.saturating_sub(at_column) <= next.saturating_sub(column) {
                    at
                } else {
                    at.saturating_add(c.len_utf8())
                };
            }
            at_column = next;
        }
        range.end
    }

    /// The indentation of the line `offset` is on: its leading spaces and
    /// tabs.
    fn indentation_of_line(&self, offset: usize) -> String {
        let line = self.buffer.line_of(offset).unwrap_or(0);
        let Some(range) = self.buffer.line_range(line) else {
            return String::new();
        };
        self.buffer
            .chars(range.start)
            .take_while(|&(at, c)| at < range.end.min(offset) && (c == ' ' || c == '\t'))
            .map(|(_, c)| c)
            .collect()
    }

    /// What Backspace at `head` deletes: in a line's leading spaces, back to
    /// the previous tab stop; otherwise the character before.
    fn backspace_range(&self, head: usize) -> Range<usize> {
        let line = self.buffer.line_of(head).unwrap_or(0);
        let start = self.buffer.line_start(line).unwrap_or(0);
        let in_indent = head > start
            && self
                .buffer
                .slice(start..head)
                .is_ok_and(|before| !before.is_empty() && before.chars().all(|c| c == ' '));
        if in_indent && self.options.insert_spaces {
            let tab = self.options.tab();
            let column = head.saturating_sub(start);
            let back = match column.checked_rem(tab).unwrap_or(0) {
                0 => tab,
                n => n,
            };
            return head.saturating_sub(back).max(start)..head;
        }
        self.buffer
            .chars_rev(head)
            .next()
            .map_or(head..head, |(at, _)| at..head)
    }

    /// The whole line `offset` is on, its newline included when it has one.
    fn whole_line_range(&self, offset: usize) -> Range<usize> {
        let line = self.buffer.line_of(offset).unwrap_or(0);
        let Some(range) = self.buffer.line_range(line) else {
            return offset..offset;
        };
        let end = if range.end < self.buffer.len() {
            range.end.saturating_add(1)
        } else {
            range.end
        };
        range.start..end
    }

    /// Whether `s` spans more than one line.
    fn spans_lines(&self, s: &Selection) -> bool {
        let range = s.range();
        self.buffer.line_of(range.start).ok() != self.buffer.line_of(range.end).ok()
    }

    /// Every line a selection touches, once each, top to bottom. A selection
    /// that ends at the very start of a line does not touch that line.
    fn touched_lines(&self) -> Vec<usize> {
        let mut lines: Vec<usize> = Vec::new();
        for s in &self.selections {
            let range = s.range();
            let first = self.buffer.line_of(range.start).unwrap_or(0);
            let mut last = self.buffer.line_of(range.end).unwrap_or(first);
            if last > first && self.buffer.line_start(last) == Some(range.end) {
                last = last.saturating_sub(1);
            }
            for line in first..=last {
                if lines.last() != Some(&line) {
                    lines.push(line);
                }
            }
        }
        lines.sort_unstable();
        lines.dedup();
        lines
    }

    /// The start of the word before `offset`, skipping blanks first.
    fn word_left_of(&self, offset: usize) -> usize {
        let mut chars = self.buffer.chars_rev(offset).peekable();
        let mut at = offset;
        while let Some(&(p, c)) = chars.peek() {
            if c.is_whitespace() && c != '\n' {
                at = p;
                chars.next();
            } else {
                break;
            }
        }
        match chars.peek().copied() {
            Some((p, '\n')) => p,
            Some((_, c)) => {
                let word = is_word(c);
                for (p, c) in chars {
                    if is_word(c) != word || c.is_whitespace() {
                        break;
                    }
                    at = p;
                }
                at
            }
            None => at,
        }
    }

    /// The end of the word after `offset`, skipping blanks first.
    fn word_right_of(&self, offset: usize) -> usize {
        let mut chars = self.buffer.chars(offset).peekable();
        let mut at = offset;
        while let Some(&(p, c)) = chars.peek() {
            if c.is_whitespace() && c != '\n' {
                at = p.saturating_add(c.len_utf8());
                chars.next();
            } else {
                break;
            }
        }
        match chars.peek().copied() {
            Some((p, '\n')) => p.saturating_add(1),
            Some((_, c)) => {
                let word = is_word(c);
                for (p, c) in chars {
                    if is_word(c) != word || c.is_whitespace() {
                        break;
                    }
                    at = p.saturating_add(c.len_utf8());
                }
                at
            }
            None => at,
        }
    }

    /// The matching bracket for bracket `c` at `at`.
    fn match_from(&self, at: usize, c: char) -> Option<usize> {
        if let Some(closer) = closer_of(c) {
            let mut depth: usize = 0;
            for (p, d) in self.buffer.chars(at).take(BRACKET_SCAN_LIMIT) {
                if d == c {
                    depth = depth.saturating_add(1);
                } else if d == closer {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(p);
                    }
                }
            }
            return None;
        }
        let opener = opener_of(c)?;
        let mut depth: usize = 0;
        let after = at.saturating_add(c.len_utf8());
        for (p, d) in self.buffer.chars_rev(after).take(BRACKET_SCAN_LIMIT) {
            if d == c {
                depth = depth.saturating_add(1);
            } else if d == opener {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(p);
                }
            }
        }
        None
    }
}

/// How far a bracket match is looked for, in characters either way: far
/// enough for any function, near enough that a stray bracket in a huge file
/// is not a pause.
pub const BRACKET_SCAN_LIMIT: usize = 200_000;

/// How many columns from `column` to the next tab stop, `tab` wide: a whole
/// tab at a stop, since a tab typed there still moves on.
fn to_next_stop(column: usize, tab: usize) -> usize {
    column
        .checked_rem(tab)
        .map_or(tab, |into| tab.saturating_sub(into))
}

/// Whether `c` is part of a word: letters, digits, the underscore.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn closer_of(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

fn opener_of(c: char) -> Option<char> {
    match c {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

/// `offset` moved by `shift`, held at zero.
fn offset_by(offset: usize, shift: isize) -> usize {
    if shift >= 0 {
        offset.saturating_add(shift.unsigned_abs())
    } else {
        offset.saturating_sub(shift.unsigned_abs())
    }
}

/// Where `offset` lands after `edits` (ranges in the text before them): moved
/// by every edit wholly before it -- an insertion at the offset itself pushes
/// it along -- and, inside a range an edit replaced, to where that edit's
/// text starts.
fn map_offset(offset: usize, edits: &[(Range<usize>, String)]) -> usize {
    let mut shift: isize = 0;
    let mut inside: Option<usize> = None;
    let mut sorted: Vec<&(Range<usize>, String)> = edits.iter().collect();
    sorted.sort_by_key(|(range, _)| range.start);
    for (range, text) in sorted {
        if range.end <= offset {
            shift = shift
                .saturating_add(signed(text.len()))
                .saturating_sub(signed(range.end.saturating_sub(range.start)));
        } else if range.start < offset {
            inside = Some(range.start);
            break;
        }
    }
    offset_by(inside.unwrap_or(offset), shift)
}

/// A length as a signed amount of shift.
fn signed(len: usize) -> isize {
    isize::try_from(len).unwrap_or(isize::MAX)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// An editor holding `text` with a caret wherever `|` is (the bars are
    /// taken out), and selections from `[` to `]`.
    fn editor(marked: &str) -> CodeEditor {
        let mut text = String::new();
        let mut selections = Vec::new();
        let mut open: Option<usize> = None;
        for c in marked.chars() {
            match c {
                '|' => selections.push(Selection::caret(text.len())),
                '[' => open = Some(text.len()),
                ']' => selections.push(Selection {
                    anchor: open.take().expect("] without ["),
                    head: text.len(),
                }),
                _ => text.push(c),
            }
        }
        let mut e = CodeEditor::from_text(&text);
        if !selections.is_empty() {
            e.set_selections(selections);
        }
        e
    }

    /// The text with the carets marked back in, as `editor` reads them.
    fn marked(e: &CodeEditor) -> String {
        let text = e.text();
        let mut out = String::new();
        let mut marks: Vec<(usize, char)> = Vec::new();
        for s in e.selections() {
            if s.is_empty() {
                marks.push((s.head, '|'));
            } else {
                marks.push((s.range().start, '['));
                marks.push((s.range().end, ']'));
            }
        }
        marks.sort_by_key(|&(at, c)| (at, c != ']'));
        let mut at = 0;
        for (mark_at, c) in marks {
            out.push_str(&text[at..mark_at]);
            out.push(c);
            at = mark_at;
        }
        out.push_str(&text[at..]);
        out
    }

    /// **Typing goes in at every caret, as one step**, and undo and redo take
    /// it back and make it again, carets and all.
    #[test]
    fn typing_goes_in_at_every_caret_as_one_step() {
        let mut e = editor("|one\n|two\n|three");
        e.type_text("> ");
        assert_eq!(marked(&e), "> |one\n> |two\n> |three");
        assert!(e.undo());
        assert_eq!(marked(&e), "|one\n|two\n|three");
        assert!(e.redo());
        assert_eq!(marked(&e), "> |one\n> |two\n> |three");
    }

    /// **Undo puts back exactly what a batch changed, however its carets
    /// shifted each other.** An insertion at an earlier caret moves every
    /// later one along; taking the batch back has to look for each insertion
    /// where it ended up, not where it was made.
    #[test]
    fn undo_finds_each_carets_change_where_the_batch_left_it() {
        let mut e = editor("[ab]c|d[e]f");
        e.type_text("XYZ");
        assert_eq!(e.text(), "XYZcXYZdXYZf");
        e.undo();
        assert_eq!(marked(&e), "[ab]c|d[e]f");
        e.redo();
        assert_eq!(e.text(), "XYZcXYZdXYZf");

        // Backspacing at three carets, then taking it back.
        let mut e = editor("a|b c|d e|f");
        e.backspace();
        assert_eq!(marked(&e), "|b |d |f");
        e.undo();
        assert_eq!(marked(&e), "a|b c|d e|f");
    }

    /// **Typing is undone a word at a time**: a blank typed after a word
    /// starts a new step -- neither a letter per undo, nor the paragraph.
    #[test]
    fn typing_is_undone_a_word_at_a_time() {
        let mut e = editor("|");
        for c in "hello world again".chars() {
            e.type_text(&c.to_string());
        }
        e.undo();
        assert_eq!(e.text(), "hello world");
        e.undo();
        assert_eq!(e.text(), "hello");
        e.undo();
        assert_eq!(e.text(), "");
        // A move ends the run: what is typed after it is its own step.
        let mut e = editor("|");
        e.type_text("ab");
        e.move_left(false);
        e.type_text("c");
        e.undo();
        assert_eq!(e.text(), "ab");
    }

    /// **A new line keeps the indentation, one level deeper after an opening
    /// bracket -- and a bracket's closer, right after the caret, goes to a
    /// line of its own at the outer level.**
    #[test]
    fn a_new_line_follows_the_codes_indentation() {
        let mut e = editor("    let x = 1;|");
        e.newline();
        assert_eq!(marked(&e), "    let x = 1;\n    |");

        let mut e = editor("fn main() {|}");
        e.newline();
        assert_eq!(marked(&e), "fn main() {\n    |\n}");

        let mut e = editor("    call(|");
        e.newline();
        assert_eq!(marked(&e), "    call(\n        |");

        let mut e = editor("  x|");
        e.set_options(Options {
            auto_indent: false,
            ..Options::default()
        });
        e.newline();
        assert_eq!(marked(&e), "  x\n|");
    }

    /// **Tab runs to the next tab stop** in spaces, or is a tab character;
    /// Backspace in a line's leading spaces goes back to the previous stop.
    #[test]
    fn tab_stops_are_kept_both_ways() {
        let mut e = editor("ab|");
        e.tab();
        assert_eq!(marked(&e), "ab  |");
        e.tab();
        assert_eq!(marked(&e), "ab      |");

        let mut e = editor("      |x");
        e.backspace();
        assert_eq!(marked(&e), "    |x", "back to the previous stop");
        e.backspace();
        assert_eq!(marked(&e), "|x");

        let mut e = editor("a|");
        e.set_options(Options {
            insert_spaces: false,
            ..Options::default()
        });
        e.tab();
        assert_eq!(marked(&e), "a\t|");
    }

    /// **Indenting a selection indents every line it touches, and keeps it
    /// selected**; a line the selection only ends at the start of is not
    /// touched, and an empty line is left empty. Dedenting takes off a tab or
    /// up to a tab width of spaces.
    #[test]
    fn a_selection_is_indented_and_dedented_by_its_lines() {
        let mut e = editor("[a\n\nb\n]c");
        e.tab();
        assert_eq!(e.text(), "    a\n\n    b\nc");
        e.dedent();
        assert_eq!(e.text(), "a\n\nb\nc");

        let mut e = editor("\t[x\n      y]");
        e.dedent();
        assert_eq!(e.text(), "x\n  y");
        e.undo();
        assert_eq!(e.text(), "\tx\n      y");

        // A selection that starts inside the indentation taken off starts
        // where the line now does -- not somewhere further along it.
        let mut e = editor("  [  x\n    y]");
        e.dedent();
        assert_eq!(marked(&e), "[x\ny]");
    }

    /// **Left and right step by character, word motion by word, Home is
    /// smart.**
    #[test]
    fn carets_move_by_character_word_and_line() {
        let mut e = editor("aé|日b");
        e.move_left(false);
        assert_eq!(marked(&e), "a|é日b");
        e.move_right(false);
        e.move_right(false);
        assert_eq!(marked(&e), "aé日|b");

        let mut e = editor("|foo_bar  baz.qux");
        e.move_word_right(false);
        assert_eq!(marked(&e), "foo_bar|  baz.qux");
        e.move_word_right(false);
        assert_eq!(marked(&e), "foo_bar  baz|.qux");
        e.move_word_left(false);
        assert_eq!(marked(&e), "foo_bar  |baz.qux");

        let mut e = editor("    let x|;");
        e.move_home(false);
        assert_eq!(marked(&e), "    |let x;");
        e.move_home(false);
        assert_eq!(marked(&e), "|    let x;");
        e.move_end(true);
        assert_eq!(marked(&e), "[    let x;]");
    }

    /// **Up and down keep the column the caret wanted**, across a short line
    /// between, counting a tab to its stop.
    #[test]
    fn vertical_moves_keep_their_goal_column() {
        let mut e = editor("abcdef|\nab\nabcdefgh");
        e.move_down(1, false);
        assert_eq!(marked(&e), "abcdef\nab|\nabcdefgh");
        e.move_down(1, false);
        assert_eq!(marked(&e), "abcdef\nab\nabcdef|gh");
        e.move_up(2, false);
        assert_eq!(marked(&e), "abcdef|\nab\nabcdefgh");

        // A tab is four columns: column 5 is one past it.
        let mut e = editor("abcde|\n\tx");
        e.move_down(1, false);
        assert_eq!(marked(&e), "abcde\n\tx|");
        // From the first line up is the start; from the last, down is the end.
        e.move_up(5, false);
        e.move_up(1, false);
        assert_eq!(marked(&e), "|abcde\n\tx");
        e.move_down(9, false);
        e.move_down(1, false);
        assert_eq!(marked(&e), "abcde\n\tx|");
    }

    /// **Carets that meet become one**, so an edit is never made twice at one
    /// place.
    #[test]
    fn carets_that_meet_are_merged() {
        let mut e = editor("|a|bc");
        e.move_left(false);
        assert_eq!(marked(&e), "|abc");
        assert_eq!(e.selections().len(), 1);

        let mut e = editor("[ab][cd]");
        assert_eq!(
            e.selections().len(),
            2,
            "selections that only touch stay two"
        );
        e.set_selections(vec![
            Selection { anchor: 0, head: 3 },
            Selection { anchor: 2, head: 4 },
        ]);
        assert_eq!(marked(&e), "[abcd]");
    }

    /// **More carets: above and below, the next occurrence, and a block.**
    #[test]
    fn carets_are_added_above_below_by_occurrence_and_by_block() {
        let mut e = editor("abcd\nab|cd\nabcd");
        e.add_caret_vertically(true);
        e.add_caret_vertically(false);
        assert_eq!(marked(&e), "ab|cd\nab|cd\nab|cd");

        let mut e = editor("let x|y = xy + xy;");
        e.add_next_occurrence();
        assert_eq!(marked(&e), "let [xy] = xy + xy;");
        e.add_next_occurrence();
        e.add_next_occurrence();
        assert_eq!(marked(&e), "let [xy] = [xy] + [xy];");
        e.type_text("z");
        assert_eq!(e.text(), "let z = z + z;");

        let mut e = editor("abcdef\nab\nabcdef");
        // Column 1 of the first line to column 4 of the third.
        e.select_block(1, 14);
        assert_eq!(marked(&e), "a[bcd]ef\na[b]\na[bcd]ef");
    }

    /// **Copying from several carets and pasting at as many puts each piece
    /// back at its own caret**; a caret with nothing selected copies, and
    /// cuts, its whole line.
    #[test]
    fn copy_and_paste_follow_the_carets() {
        let e = editor("[one] [two]");
        let copied = e.copy();
        assert_eq!(copied, "one\ntwo");
        let mut target = editor("a|\nb|");
        target.paste(&copied);
        assert_eq!(target.text(), "aone\nbtwo");

        let mut e = editor("first\nsec|ond\nthird");
        assert_eq!(e.copy(), "second\n");
        let cut = e.cut();
        assert_eq!(cut, "second\n");
        assert_eq!(e.text(), "first\nthird");
    }

    /// **The matching bracket is found either side of the caret, nested
    /// brackets skipped; an unmatched one has none.**
    #[test]
    fn matching_brackets_are_found_through_nesting() {
        // Not through `editor`, whose `[` and `]` mark selections.
        let e = CodeEditor::from_text("f(a[1], {b}) + (");
        assert_eq!(e.matching_bracket(1), Some((1, 11)), "on an opener");
        assert_eq!(e.matching_bracket(12), Some((11, 1)), "just after a closer");
        assert_eq!(e.matching_bracket(8), Some((8, 10)));
        assert_eq!(e.matching_bracket(15), None, "unmatched");
        assert_eq!(e.matching_bracket(5), Some((5, 3)), "on a closer");
        // Between `,` and a space: neither side is a bracket.
        assert_eq!(e.matching_bracket(7), None, "not a bracket");

        // Brackets of the same kind nested inside: counted, not matched.
        let e = CodeEditor::from_text("g((x)(y))");
        assert_eq!(e.matching_bracket(1), Some((1, 8)));
        assert_eq!(e.matching_bracket(9), Some((8, 1)));
        assert_eq!(e.matching_bracket(2), Some((2, 4)));
    }

    /// **The history is a tree**: typing after undoing starts a branch, and
    /// the undone text is still reachable by walking states in order.
    #[test]
    fn what_was_undone_is_kept_on_a_branch() {
        let mut e = editor("|");
        e.type_text("first");
        e.undo();
        e.type_text("second");
        assert_eq!(e.text(), "second");
        e.earlier();
        assert_eq!(e.text(), "first", "the undone branch is reachable");
        e.earlier();
        assert_eq!(e.text(), "", "and the state before both");
        e.later();
        e.later();
        assert_eq!(e.text(), "second");
    }

    /// **Selections are held to the text**: past the end is the end, inside a
    /// character is its start, and none at all is a caret at the start.
    #[test]
    fn selections_are_held_to_the_text() {
        let mut e = editor("aé");
        e.set_selections(vec![Selection::caret(99)]);
        assert_eq!(e.primary(), Selection::caret(3));
        e.set_selections(vec![Selection::caret(2)]);
        assert_eq!(e.primary(), Selection::caret(1), "inside é");
        e.set_selections(Vec::new());
        assert_eq!(e.selections(), &[Selection::caret(0)]);
        e.select_all();
        assert_eq!(marked(&e), "[aé]");
    }

    /// **A word, a line, and the next line** -- the selection modes a double
    /// click, a triple click and Ctrl+L make, at every caret.
    #[test]
    fn words_and_lines_are_selected_at_every_caret() {
        let mut e = editor("let f|oo = b|ar;");
        e.select_word();
        assert_eq!(marked(&e), "let [foo] = [bar];");

        let mut e = editor("one\ntw|o\nthree");
        e.select_line();
        assert_eq!(marked(&e), "one\n[two\n]three");
        e.select_line();
        assert_eq!(
            marked(&e),
            "one\n[two\nthree]",
            "again takes in the next line"
        );

        let mut e = editor("a|\nb|");
        e.select_line();
        // Each caret's line is its own selection: two that meet end to start
        // stay two, as any two selections that only touch do.
        assert_eq!(marked(&e), "[a\n][b]");
    }

    /// **The words around a caret** -- in one, at one's edge, or in none.
    #[test]
    fn the_word_at_a_caret() {
        let e = editor("foo bar_1 ,");
        assert_eq!(e.word_at(5), 4..9);
        assert_eq!(e.word_at(4), 4..9, "at its start");
        assert_eq!(e.word_at(9), 4..9, "at its end");
        assert_eq!(e.word_at(10), 10..10, "none");
    }

    fn finder(pattern: &str, regex: bool, case_sensitive: bool, whole_word: bool) -> Finder {
        Finder::new(&FindQuery {
            pattern: pattern.to_owned(),
            regex,
            case_sensitive,
            whole_word,
        })
        .expect("a query")
    }

    /// **Plain text is found case-folded or not, whole words or not**, the
    /// matches never overlapping.
    #[test]
    fn plain_text_is_found_by_case_and_by_word() {
        let text = "Cat cat catalog aaaa";
        assert_eq!(
            finder("cat", false, false, false).find_all(text),
            [0..3, 4..7, 8..11]
        );
        assert_eq!(
            finder("cat", false, true, false).find_all(text),
            [4..7, 8..11]
        );
        assert_eq!(
            finder("cat", false, false, true).find_all(text),
            [0..3, 4..7]
        );
        assert_eq!(
            finder("aa", false, true, false).find_all(text),
            [16..18, 18..20]
        );
        // A whole-word match right after one that is not.
        assert_eq!(
            finder("aa", false, true, true).find_all("aaa aa"),
            [Range { start: 4, end: 6 }]
        );
    }

    /// **A regular expression is found, and its replacement takes the
    /// match's groups; a match of nothing is not a match.**
    #[test]
    fn regular_expressions_are_found_and_replace_with_groups() {
        let f = finder(r"(\w+)@(\w+)", true, true, false);
        let text = "to: ann@example, bob@test";
        let found = f.find_all(text);
        assert_eq!(found, [4..15, 17..25]);
        assert_eq!(
            f.replacement(text, found[0].clone(), "$2 at ${1}"),
            "example at ann"
        );
        assert!(
            finder("x*", true, true, false)
                .find_all("abxc")
                .iter()
                .all(|m| !m.is_empty())
        );
        assert_eq!(
            finder("x*", true, true, false).find_all("abxc"),
            [Range { start: 2, end: 3 }]
        );
        // Whole words, case folded, as a regular expression too.
        assert_eq!(
            finder("cat", true, false, true).find_all("Cat cat catalog"),
            [0..3, 4..7]
        );
        // `^` and `$` are the lines'.
        assert_eq!(
            finder("^b", true, true, false).find_all("a\nb\nb"),
            [2..3, 4..5]
        );
        let refused = Finder::new(&FindQuery {
            pattern: "(".to_owned(),
            regex: true,
            ..FindQuery::default()
        })
        .map(|_| ());
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.to_string().contains("not a regular expression")),
            "{refused:?}"
        );
        assert_eq!(
            Finder::new(&FindQuery::default()).map(|_| ()),
            Err(FindError::Empty)
        );
    }

    /// **Stepping through matches goes round the end, both ways.**
    #[test]
    fn find_next_goes_round_the_end_both_ways() {
        let mut e = editor("x| a x a x");
        let f = finder("x", false, true, false);
        assert!(e.find_next(&f, false));
        assert_eq!(marked(&e), "x a [x] a x");
        e.find_next(&f, false);
        e.find_next(&f, false);
        assert_eq!(marked(&e), "[x] a x a x", "round the end");
        e.find_next(&f, true);
        assert_eq!(marked(&e), "x a x a [x]", "and back round it");
        assert!(!e.find_next(&finder("zzz", false, true, false), false));
        assert_eq!(marked(&e), "x a x a [x]", "no match leaves the selection");
    }

    /// **Every match becomes a selection of its own, to edit at once.**
    #[test]
    fn every_match_can_be_selected_at_once() {
        let mut e = editor("|foo bar foo baz foo");
        assert_eq!(e.select_all_matches(&finder("foo", false, true, false)), 3);
        e.type_text("qux");
        assert_eq!(e.text(), "qux bar qux baz qux");
        assert_eq!(e.select_all_matches(&finder("nope", false, true, false)), 0);
    }

    /// **Replace takes the selected match and moves to the next; replace-all
    /// is one undo step.**
    #[test]
    fn replace_one_then_the_rest_as_one_step() {
        let mut e = editor("|a1 b2 c3");
        let f = finder(r"([a-z])(\d)", true, true, false);
        assert!(!e.replace_next(&f, "$2$1"), "first it finds");
        assert_eq!(marked(&e), "[a1] b2 c3");
        assert!(e.replace_next(&f, "$2$1"));
        assert_eq!(marked(&e), "1a [b2] c3");
        assert_eq!(e.replace_all(&f, "<$0>"), 2);
        assert_eq!(e.text(), "1a <b2> <c3>");
        e.undo();
        assert_eq!(e.text(), "1a b2 c3", "replace-all was one step");
    }

    /// **Every edit bumps the revision; a move does not.**
    #[test]
    fn edits_bump_the_revision_and_moves_do_not() {
        let mut e = editor("a|b");
        let r = e.revision();
        e.move_left(false);
        assert_eq!(e.revision(), r);
        e.type_text("x");
        assert!(e.revision() > r);
        let r = e.revision();
        e.undo();
        assert!(e.revision() > r);
    }
}

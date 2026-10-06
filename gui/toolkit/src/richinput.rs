//! A multi-line text field whose text carries formatting -- bold, italic,
//! underline, strikethrough, a colour, a size -- with the keys that set it
//! and a toolbar that shows and sets it: `roadmap-detailed.md` §3.5's "Rich
//! input with formatting and image paste (optional formatting toolbar)".
//!
//! # What it is
//!
//! [`RichInput`] is state and drawing, as [`crate::textarea::TextArea`] is:
//! the caller owns the box, the focus and the events, and asks this for what
//! to put in them. Its text is a [`RichDoc`] -- the characters, and runs of
//! them alike in format ([`doc`]) -- laid out in lines by [`layout`], each
//! run in its own size and weight, a line as tall as the tallest text on it.
//!
//! # Formatting
//!
//! Ctrl+B, Ctrl+I and Ctrl+U switch bold, italic and underline -- over the
//! selection, or, with none, for what is typed next -- and the toolbar
//! ([`toolbar`]) does the same and shows what the selection is: a button
//! pressed where all of it has its format. Text typed carries on in the
//! format before the caret, as a word processor's does. A colour or a size
//! comes from the program ([`RichInput::set_color`], [`RichInput::set_size`]),
//! which has the colour picker and the size list to choose them with.
//!
//! Italic is kept and saved, and drawn upright until the font cache can
//! draw a face slanted (`requests/c-f-text-at-any-weight-and-in-italic.md`).
//!
//! # Undo
//!
//! Every change -- text or formatting -- is a replacement of one stretch of
//! the document by another, and the history ([`crate::undo`], a tree) keeps
//! both: typing is gathered a word at a time, deleting likewise, as the plain
//! field gathers them.
//!
//! # The clipboard
//!
//! A copy puts the plain text on the program's clipboard ([`crate::clipboard`])
//! and keeps the formatted text beside it; a paste of that same text brings
//! the formatting back, and any other text pastes plain, in the format at the
//! caret. Pictures pasted in wait on the clipboard carrying them
//! (`known-issues/TD-C-A-RICH-INPUT-CANNOT-TAKE-A-PICTURE.md`).
//! Ctrl+Shift+V pastes the text alone, the copy's formatting left behind.
//!
//! # The menu
//!
//! A right-click offers what the field's keys do, as every field's menu
//! does (`design-decisions.md` §1454, [`crate::editmenu`]): Undo and Redo,
//! Cut, Copy, Paste and *Paste as plain text*, Delete, Select all -- and
//! Bold, Italic and Underline, ticked where what is shown has them
//! ([`RichInput::edit_menu`], [`RichInput::edit_command`]).

pub mod doc;
pub mod layout;
pub mod toolbar;

use std::cell::RefCell;
use std::num::NonZeroUsize;

pub use doc::{Format, RichDoc, Run};
pub use layout::{Line, Metrics, Piece};

use crate::color::Color;
use crate::editmenu::{EditCommand, EditState};
use crate::event::{Key, KeyEvent};
use crate::menu::{ContextMenu, MenuItem, MenuItemId};
use crate::palette::Palette;
use crate::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use crate::style::CornerRadii;
use crate::textinput::KeyEdit;
use crate::undo::UndoHistory;

/// How many steps a field's history keeps.
const HISTORY: usize = 500;

/// Where the ids of a rich field's own menu rows start: far up the id space,
/// beside the edit rows every field's menu has ([`crate::editmenu`]), so a
/// window's own rows sit beside both without renumbering.
const MENU_BASE: MenuItemId = 0xED18_0000_0000_0000;

/// A row of a rich field's menu that is not one every field's has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RichCommand {
    /// Paste the clipboard's text alone ([`RichInput::paste_plain`]).
    PastePlain,
    /// Switch bold.
    Bold,
    /// Switch italic.
    Italic,
    /// Switch underline.
    Underline,
}

impl RichCommand {
    /// Every one, in the menu's order.
    pub const ALL: [Self; 4] = [Self::PastePlain, Self::Bold, Self::Italic, Self::Underline];

    /// The id its row carries.
    #[must_use]
    pub const fn id(self) -> MenuItemId {
        MENU_BASE | self as MenuItemId
    }

    /// The row an id names, if it is one of these.
    #[must_use]
    pub fn from_id(id: MenuItemId) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    /// Its row's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PastePlain => "Paste as plain text",
            Self::Bold => "Bold",
            Self::Italic => "Italic",
            Self::Underline => "Underline",
        }
    }

    /// The keys that do the same, shown beside the name.
    #[must_use]
    pub const fn keys(self) -> &'static str {
        match self {
            Self::PastePlain => "Ctrl+Shift+V",
            Self::Bold => "Ctrl+B",
            Self::Italic => "Ctrl+I",
            Self::Underline => "Ctrl+U",
        }
    }

    /// The switch it flips, for those that flip one.
    const fn toggle(self) -> Option<Toggle> {
        match self {
            Self::PastePlain => None,
            Self::Bold => Some(Toggle::Bold),
            Self::Italic => Some(Toggle::Italic),
            Self::Underline => Some(Toggle::Underline),
        }
    }
}

/// One of the switches the keys and the toolbar flip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    /// Bold.
    Bold,
    /// Italic.
    Italic,
    /// Underline.
    Underline,
    /// Strikethrough.
    Strike,
}

impl Toggle {
    /// Whether `format` has it.
    #[must_use]
    pub const fn of(self, format: &Format) -> bool {
        match self {
            Self::Bold => format.bold,
            Self::Italic => format.italic,
            Self::Underline => format.underline,
            Self::Strike => format.strike,
        }
    }

    /// Give `format` it, or take it away.
    pub const fn set(self, format: &mut Format, on: bool) {
        match self {
            Self::Bold => format.bold = on,
            Self::Italic => format.italic = on,
            Self::Underline => format.underline = on,
            Self::Strike => format.strike = on,
        }
    }
}

/// What a step was: what gathers it with the step before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Typing,
    Deleting,
    Other,
}

/// One step of the history: a stretch replaced, and the caret either side.
#[derive(Clone, Debug, PartialEq)]
struct Edit {
    at: usize,
    removed: RichDoc,
    inserted: RichDoc,
    caret_before: usize,
    anchor_before: Option<usize>,
    caret_after: usize,
    kind: Kind,
}

std::thread_local! {
    /// The formatted text of this program's last rich copy, beside the plain
    /// text it put on the clipboard: what a paste of that same text brings
    /// back.
    static RICH_CLIPBOARD: RefCell<Option<RichDoc>> = const { RefCell::new(None) };
}

/// How a field is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Look<'a> {
    /// The box: left, top, width, height.
    pub rect: (f32, f32, f32, f32),
    /// Whether it has the keyboard: the caret is drawn.
    pub focused: bool,
    /// What it shows while empty.
    pub placeholder: &'a str,
}

/// A rich text field's state.
#[derive(Clone, Debug)]
pub struct RichInput {
    doc: RichDoc,
    cursor: usize,
    /// Where a wrap makes the caret's offset both a line's end and the
    /// next's start, whether it is drawn at the end of the line above.
    upstream: bool,
    anchor: Option<usize>,
    /// The format the next typed text takes, set by a switch with nothing
    /// selected; dropped when the caret moves.
    typing: Option<Format>,
    /// How far across a run of Up and Down keeps the caret.
    goal_x: Option<f32>,
    scroll_y: f32,
    history: UndoHistory<Edit>,
}

impl Default for RichInput {
    fn default() -> Self {
        Self::new()
    }
}

impl RichInput {
    /// An empty field.
    #[must_use]
    pub fn new() -> Self {
        Self::with_doc(RichDoc::default())
    }

    /// A field holding `doc`, the caret at its end.
    #[must_use]
    pub fn with_doc(doc: RichDoc) -> Self {
        let cursor = doc.len();
        Self {
            doc,
            cursor,
            upstream: false,
            anchor: None,
            typing: None,
            goal_x: None,
            scroll_y: 0.0,
            // A nonzero constant.
            history: UndoHistory::new(NonZeroUsize::new(HISTORY).unwrap_or(NonZeroUsize::MIN)),
        }
    }

    /// The document.
    #[must_use]
    pub const fn doc(&self) -> &RichDoc {
        &self.doc
    }

    /// The plain text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.doc.text()
    }

    /// Hold `doc` instead, the caret at its end and the history begun again
    /// -- a different document, not an edit of this one.
    pub fn set_doc(&mut self, doc: RichDoc) {
        *self = Self::with_doc(doc);
    }

    /// Where the caret is, as a byte offset.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Put the caret at `at` -- held to the text and to a character's start
    /// -- with nothing selected.
    pub fn set_cursor(&mut self, at: usize) {
        self.cursor = self.boundary(at);
        self.anchor = None;
        self.moved();
    }

    /// The selection, start first, if there is one.
    #[must_use]
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    /// Whether anything is selected.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// What is selected, formatting and all.
    #[must_use]
    pub fn selected(&self) -> RichDoc {
        self.selection_range()
            .map_or_else(RichDoc::default, |(s, e)| self.doc.slice(s, e))
    }

    /// Whether there is a step to undo.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether there is a step to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// `at` held to the text and moved back to a character's start.
    fn boundary(&self, at: usize) -> usize {
        let text = self.doc.text();
        let mut at = at.min(text.len());
        while at > 0 && !text.is_char_boundary(at) {
            at = at.saturating_sub(1);
        }
        at
    }

    /// The caret moved: what was set for typing at the old place, and the
    /// column Up and Down were keeping, no longer hold.
    fn moved(&mut self) {
        self.typing = None;
        self.goal_x = None;
        self.upstream = false;
    }

    // -----------------------------------------------------------------
    // Editing
    // -----------------------------------------------------------------

    /// Replace `start..end` with `with`, the caret after it, as one step of
    /// `kind` -- gathered into the step before where that was the same kind
    /// and this carries on from where it left off.
    fn replace(&mut self, start: usize, end: usize, with: &RichDoc, kind: Kind) {
        let caret_before = self.cursor;
        let anchor_before = self.anchor;
        let removed = self.doc.replace(start, end, with);
        let caret_after = start.saturating_add(with.len());
        self.cursor = caret_after;
        self.anchor = None;
        self.goal_x = None;
        self.upstream = false;
        if let Some(last) = self.history.last_mut()
            && kind != Kind::Other
            && last.kind == kind
            && gathers(last, start, &removed, with)
        {
            match kind {
                // Typing on: the new text joins what was typed.
                Kind::Typing => {
                    last.inserted.append(with);
                    last.caret_after = caret_after;
                }
                // Deleting on: backwards, what went comes before what went
                // already; forwards, after it.
                Kind::Deleting => {
                    if start < last.at {
                        let mut joined = removed;
                        joined.append(&last.removed);
                        last.removed = joined;
                        last.at = start;
                    } else {
                        last.removed.append(&removed);
                    }
                    last.caret_after = caret_after;
                }
                Kind::Other => {}
            }
            return;
        }
        self.history.record(Edit {
            at: start,
            removed,
            inserted: with.clone(),
            caret_before,
            anchor_before,
            caret_after,
            kind,
        });
    }

    /// Type `text` in place of the selection, in the format typing takes
    /// here. A line break is `\n`.
    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() && !self.has_selection() {
            return;
        }
        let (start, end) = self.selection_range().unwrap_or((self.cursor, self.cursor));
        let format = self.typing.unwrap_or_else(|| self.doc.format_before(start));
        let with = RichDoc::plain(text, format);
        self.replace(start, end, &with, Kind::Typing);
        // What was set for typing is kept for what is typed next.
        self.typing = Some(format);
    }

    /// Put `doc` in place of the selection, its formatting with it.
    pub fn insert_doc(&mut self, doc: &RichDoc) {
        let (start, end) = self.selection_range().unwrap_or((self.cursor, self.cursor));
        if doc.is_empty() && start == end {
            return;
        }
        self.replace(start, end, doc, Kind::Other);
        self.typing = None;
    }

    /// Delete the selection, or the character before the caret. Answers
    /// whether anything went.
    pub fn backspace(&mut self) -> bool {
        if let Some((s, e)) = self.selection_range() {
            self.replace(s, e, &RichDoc::default(), Kind::Other);
            return true;
        }
        let at = self.cursor;
        let Some(c) = self
            .doc
            .text()
            .get(..at)
            .and_then(|s| s.chars().next_back())
        else {
            return false;
        };
        let start = at.saturating_sub(c.len_utf8());
        self.replace(start, at, &RichDoc::default(), Kind::Deleting);
        true
    }

    /// Delete the selection, or the character after the caret. Answers
    /// whether anything went.
    pub fn delete(&mut self) -> bool {
        if let Some((s, e)) = self.selection_range() {
            self.replace(s, e, &RichDoc::default(), Kind::Other);
            return true;
        }
        let at = self.cursor;
        let Some(c) = self.doc.text().get(at..).and_then(|s| s.chars().next()) else {
            return false;
        };
        self.replace(
            at,
            at.saturating_add(c.len_utf8()),
            &RichDoc::default(),
            Kind::Deleting,
        );
        self.cursor = at;
        if let Some(last) = self.history.last_mut() {
            last.caret_after = at;
        }
        true
    }

    /// Change the selection's format by `change` as one step; with nothing
    /// selected, the format typing takes here instead.
    fn reformat(&mut self, change: impl Fn(&mut Format)) {
        let Some((s, e)) = self.selection_range() else {
            let mut format = self
                .typing
                .unwrap_or_else(|| self.doc.format_before(self.cursor));
            change(&mut format);
            self.typing = Some(format);
            return;
        };
        let mut after = self.doc.slice(s, e);
        let len = after.len();
        if !after.apply(0, len, change) {
            return;
        }
        let (cursor, anchor) = (self.cursor, self.anchor);
        self.replace(s, e, &after, Kind::Other);
        // Still selected, as it was.
        self.cursor = cursor;
        self.anchor = anchor;
        if let Some(last) = self.history.last_mut() {
            last.caret_after = cursor;
        }
    }

    /// Switch `toggle`: off over the selection where all of it has it, on
    /// where not; with nothing selected, for what is typed next.
    pub fn toggle(&mut self, toggle: Toggle) {
        let on = !self.shown_has(toggle);
        self.reformat(|f| toggle.set(f, on));
    }

    /// Colour the selection -- or what is typed next -- `color`; `None` is
    /// the field's text colour.
    pub fn set_color(&mut self, color: Option<Color>) {
        self.reformat(|f| f.color = color);
    }

    /// Size the selection -- or what is typed next -- at `size` pixels;
    /// `None` is the field's size. A size that is not a positive number is
    /// the field's.
    pub fn set_size(&mut self, size: Option<f32>) {
        let size = size.filter(|s| s.is_finite() && *s > 0.0);
        self.reformat(|f| f.size = size);
    }

    /// Take every format off the selection -- or what is typed next.
    pub fn clear_formatting(&mut self) {
        self.reformat(|f| *f = Format::default());
    }

    /// Whether all of the selection has `toggle` -- with nothing selected,
    /// whether what is typed next would.
    #[must_use]
    pub fn shown_has(&self, toggle: Toggle) -> bool {
        match self.selection_range() {
            Some((s, e)) => self.doc.all(s, e, |f| toggle.of(f)),
            None => toggle.of(&self.typing_format()),
        }
    }

    /// The format what is typed next takes.
    #[must_use]
    pub fn typing_format(&self) -> Format {
        self.typing
            .unwrap_or_else(|| self.doc.format_before(self.cursor))
    }

    /// Select everything.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.doc.len();
        self.moved();
    }

    /// Put the selection on the clipboard: its plain text for any program's
    /// field, and its formatting kept for a paste here.
    pub fn copy(&self) {
        let Some((s, e)) = self.selection_range() else {
            return;
        };
        let doc = self.doc.slice(s, e);
        crate::clipboard::set_text(doc.text());
        RICH_CLIPBOARD.with(|rich| {
            if let Ok(mut rich) = rich.try_borrow_mut() {
                *rich = Some(doc);
            }
        });
    }

    /// Copy the selection and delete it.
    pub fn cut(&mut self) {
        if self.has_selection() {
            self.copy();
            self.backspace();
        }
    }

    /// Paste the clipboard: formatted, if it holds what this program last
    /// copied from a rich field; else its plain text, in the format at the
    /// caret.
    pub fn paste(&mut self) {
        let text = crate::clipboard::text();
        let rich = RICH_CLIPBOARD
            .with(|rich| rich.try_borrow().ok().and_then(|r| r.clone()))
            .filter(|doc| doc.text() == text);
        match rich {
            Some(doc) => self.insert_doc(&doc),
            None if !text.is_empty() => self.insert_str(&text),
            None => {}
        }
    }

    /// Paste the clipboard's text alone, in the format at the caret, though
    /// it holds formatting this program copied: what a user asks for with
    /// Ctrl+Shift+V when the copy's look should not come with it.
    pub fn paste_plain(&mut self) {
        let text = crate::clipboard::text();
        if !text.is_empty() {
            self.insert_str(&text);
        }
    }

    /// The menu a right-click on the field puts up: what its keys do (the
    /// module's "The menu"), each dimmed row saying why while the pointer
    /// rests on it. The window shows it where the click landed.
    #[must_use]
    pub fn edit_menu(&self) -> ContextMenu {
        let state = EditState {
            selected: self.has_selection(),
            editable: true,
            has_text: !self.text().is_empty(),
            history: true,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            copies_line: false,
        };
        let nothing_copied = crate::clipboard::is_empty();
        let row = |command: RichCommand, enabled: bool, checked: Option<bool>| MenuItem::Action {
            id: command.id(),
            label: command.label().to_owned(),
            shortcut: Some(command.keys().to_owned()),
            icon: None,
            enabled,
            checked,
        };
        let mut rows = crate::editmenu::rows(state);
        let after_paste = rows
            .iter()
            .position(
                |r| matches!(r, MenuItem::Action { id, .. } if *id == EditCommand::Paste.id()),
            )
            .map_or(rows.len(), |at| at.saturating_add(1));
        rows.insert(
            after_paste,
            row(RichCommand::PastePlain, !nothing_copied, None),
        );
        rows.push(MenuItem::Separator);
        for command in [
            RichCommand::Bold,
            RichCommand::Italic,
            RichCommand::Underline,
        ] {
            let ticked = command.toggle().is_some_and(|t| self.shown_has(t));
            rows.push(row(command, true, Some(ticked)));
        }
        let mut menu = ContextMenu::new(rows);
        for command in EditCommand::ALL {
            if let Some(why) = crate::editmenu::why_dimmed(command, state) {
                menu.explain(command.id(), why);
            }
        }
        if nothing_copied {
            menu.explain(RichCommand::PastePlain.id(), "Nothing has been copied");
        }
        menu
    }

    /// Do what the row `id` of [`edit_menu`](Self::edit_menu) says, and
    /// answer as the keys that do the same would: `Changed` where the
    /// document changed, `Handled` where it did not, `Unhandled` for an id
    /// that is none of the menu's rows.
    pub fn edit_command(&mut self, id: MenuItemId) -> KeyEdit {
        let before = self.doc.clone();
        if let Some(command) = EditCommand::from_id(id) {
            match command {
                EditCommand::Undo => {
                    self.undo();
                }
                EditCommand::Redo => {
                    self.redo();
                }
                EditCommand::Cut => self.cut(),
                EditCommand::Copy => self.copy(),
                EditCommand::Paste => self.paste(),
                EditCommand::Delete => {
                    if self.has_selection() {
                        self.delete();
                    }
                }
                EditCommand::SelectAll => self.select_all(),
            }
        } else if let Some(command) = RichCommand::from_id(id) {
            match command.toggle() {
                Some(toggle) => self.toggle(toggle),
                None => self.paste_plain(),
            }
        } else {
            return KeyEdit::Unhandled;
        }
        if self.doc == before {
            KeyEdit::Handled
        } else {
            KeyEdit::Changed
        }
    }

    /// Undo the last step. Answers whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.history.undo() else {
            return false;
        };
        let end = edit.at.saturating_add(edit.inserted.len());
        self.doc.replace(edit.at, end, &edit.removed);
        self.cursor = self.boundary(edit.caret_before);
        self.anchor = edit.anchor_before.map(|a| self.boundary(a));
        self.moved();
        true
    }

    /// Do the step undone last again. Answers whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.history.redo() else {
            return false;
        };
        let end = edit.at.saturating_add(edit.removed.len());
        self.doc.replace(edit.at, end, &edit.inserted);
        self.cursor = self.boundary(edit.caret_after);
        self.anchor = None;
        self.moved();
        true
    }

    // -----------------------------------------------------------------
    // The caret
    // -----------------------------------------------------------------

    /// With Shift held, a selection begins where the caret is, or carries
    /// on; without it, any selection is dropped.
    fn begin_or_end_selection(&mut self, shift: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
    }

    /// One character left; without Shift, a selection collapses to its
    /// start instead.
    pub fn move_left(&mut self, shift: bool) {
        if !shift && let Some((s, _)) = self.selection_range() {
            self.anchor = None;
            self.cursor = s;
            self.moved();
            return;
        }
        self.begin_or_end_selection(shift);
        if let Some(c) = self
            .doc
            .text()
            .get(..self.cursor)
            .and_then(|s| s.chars().next_back())
        {
            self.cursor = self.cursor.saturating_sub(c.len_utf8());
        }
        self.moved();
    }

    /// One character right; without Shift, a selection collapses to its
    /// end instead.
    pub fn move_right(&mut self, shift: bool) {
        if !shift && let Some((_, e)) = self.selection_range() {
            self.anchor = None;
            self.cursor = e;
            self.moved();
            return;
        }
        self.begin_or_end_selection(shift);
        if let Some(c) = self
            .doc
            .text()
            .get(self.cursor..)
            .and_then(|s| s.chars().next())
        {
            self.cursor = self.cursor.saturating_add(c.len_utf8());
        }
        self.moved();
    }

    /// One place left on the screen, or right (`rightward`) -- what the arrow
    /// keys do. On a line of one direction that is one character back or on
    /// ([`move_left`](Self::move_left), [`move_right`](Self::move_right)); on
    /// a line mixing directions it is the next place a caret can be on the
    /// screen, which in a right-to-left stretch is the character *ahead*
    /// for Left. Past the line's edge it goes on as the text does.
    pub fn step_on_screen(&mut self, rightward: bool, shift: bool, m: &Metrics) {
        let lines = layout::lay_out(&self.doc, m);
        let here = layout::line_of(&lines, self.cursor, self.upstream);
        let mixed = lines.get(here).filter(|line| !line.is_ltr());
        let collapsing = !shift && self.selection_range().is_some();
        let Some(line) = mixed.filter(|_| !collapsing) else {
            if rightward {
                self.move_right(shift);
            } else {
                self.move_left(shift);
            }
            return;
        };
        let now = layout::x_at(&self.doc, line, self.cursor, self.upstream, m.size);
        let stops = layout::caret_stops(&self.doc, line, m.size);
        // The nearest place strictly past this one, the way asked: a hair's
        // tolerance, so the two offsets a direction change gives one place
        // count as one place and are stepped over together.
        const SAME: f32 = 0.5;
        let next = if rightward {
            stops.iter().find(|&&(x, _, _)| x > now + SAME)
        } else {
            stops.iter().rev().find(|&&(x, _, _)| x < now - SAME)
        };
        match next {
            Some(&(_, at, upstream)) => {
                self.begin_or_end_selection(shift);
                self.cursor = at;
                self.moved();
                // Drawn where the stop is: which side of a change of
                // direction it was found on.
                self.upstream = upstream;
            }
            // Off the line's edge: on as the text goes -- which, off the
            // right of a right-to-left line, is back.
            None if rightward != line.rtl => self.move_right(shift),
            None => self.move_left(shift),
        }
    }

    /// To the start of the word before the caret: past any spaces, then past
    /// the word's letters.
    pub fn move_word_left(&mut self, shift: bool) {
        self.begin_or_end_selection(shift);
        let before = self.doc.text().get(..self.cursor).unwrap_or("");
        let trimmed = before.trim_end_matches(char::is_whitespace);
        let word = trimmed.trim_end_matches(|c: char| !c.is_whitespace());
        self.cursor = word.len();
        self.moved();
    }

    /// To the end of the word after the caret: past any spaces, then past
    /// the word's letters.
    pub fn move_word_right(&mut self, shift: bool) {
        self.begin_or_end_selection(shift);
        let text = self.doc.text();
        let after = text.get(self.cursor..).unwrap_or("");
        let spaces = after
            .len()
            .saturating_sub(after.trim_start_matches(char::is_whitespace).len());
        let rest = after.get(spaces..).unwrap_or("");
        let word = rest
            .len()
            .saturating_sub(rest.trim_start_matches(|c: char| !c.is_whitespace()).len());
        self.cursor = self
            .cursor
            .saturating_add(spaces)
            .saturating_add(word)
            .min(text.len());
        self.moved();
    }

    /// One line up or down (`down`), keeping as near the same distance
    /// across as the line allows -- and across a run of them, the distance
    /// the first started from.
    fn move_vertically(&mut self, down: bool, shift: bool, m: &Metrics) {
        let lines = layout::lay_out(&self.doc, m);
        let here = layout::line_of(&lines, self.cursor, self.upstream);
        let x = self.goal_x.unwrap_or_else(|| {
            lines.get(here).map_or(0.0, |l| {
                layout::x_at(&self.doc, l, self.cursor, self.upstream, m.size)
            })
        });
        let target = if down {
            here.saturating_add(1)
        } else {
            here.checked_sub(1).unwrap_or(usize::MAX)
        };
        self.begin_or_end_selection(shift);
        match lines.get(target) {
            Some(line) => {
                let (at, upstream) = layout::offset_at(&self.doc, line, x, m.size);
                self.cursor = at;
                self.typing = None;
                self.upstream = upstream;
            }
            // Past the first line up, or the last down: its far end.
            None => {
                self.cursor = if down { self.doc.len() } else { 0 };
                self.typing = None;
                self.upstream = false;
            }
        }
        self.goal_x = Some(x);
    }

    /// One line up.
    pub fn move_up(&mut self, shift: bool, m: &Metrics) {
        self.move_vertically(false, shift, m);
    }

    /// One line down.
    pub fn move_down(&mut self, shift: bool, m: &Metrics) {
        self.move_vertically(true, shift, m);
    }

    /// The start of the caret's line.
    pub fn line_start(&mut self, shift: bool, m: &Metrics) {
        let lines = layout::lay_out(&self.doc, m);
        let here = layout::line_of(&lines, self.cursor, self.upstream);
        self.begin_or_end_selection(shift);
        if let Some(line) = lines.get(here) {
            self.cursor = line.start;
        }
        self.moved();
    }

    /// The end of the caret's line -- of a wrapped one, its end on it, not
    /// the start of the line below.
    pub fn line_end(&mut self, shift: bool, m: &Metrics) {
        let lines = layout::lay_out(&self.doc, m);
        let here = layout::line_of(&lines, self.cursor, self.upstream);
        self.begin_or_end_selection(shift);
        if let Some(line) = lines.get(here) {
            self.cursor = line.end;
        }
        self.moved();
        self.upstream = true;
    }

    /// The start of the document.
    pub fn doc_start(&mut self, shift: bool) {
        self.begin_or_end_selection(shift);
        self.cursor = 0;
        self.moved();
    }

    /// The end of the document.
    pub fn doc_end(&mut self, shift: bool) {
        self.begin_or_end_selection(shift);
        self.cursor = self.doc.len();
        self.moved();
    }

    // -----------------------------------------------------------------
    // Keys, the pointer, the view
    // -----------------------------------------------------------------

    /// Handle a key as a rich field does: typing, deleting, Enter's line
    /// break, the arrows (with Ctrl, a word at a time), Home and End (with
    /// Ctrl, the document's), Shift's
    /// selections, Ctrl+A, the switches (Ctrl+B, Ctrl+I, Ctrl+U), undo and
    /// redo (Ctrl+Z; Ctrl+Y or Ctrl+Shift+Z) and the clipboard (Ctrl+C,
    /// Ctrl+X, Ctrl+V; Ctrl+Shift+V for its text alone). What is left --
    /// Escape, Tab -- is the window's.
    pub fn handle_key(&mut self, key: &KeyEvent, m: &Metrics) -> KeyEdit {
        if !key.pressed {
            return KeyEdit::Unhandled;
        }
        let before = self.doc.clone();
        let shift = key.modifiers.shift;
        if key.modifiers.ctrl && !key.modifiers.alt {
            match key.key {
                Key::A => self.select_all(),
                Key::B => self.toggle(Toggle::Bold),
                Key::I => self.toggle(Toggle::Italic),
                Key::U => self.toggle(Toggle::Underline),
                Key::Z if shift => {
                    self.redo();
                }
                Key::Z => {
                    self.undo();
                }
                Key::Y => {
                    self.redo();
                }
                Key::C => self.copy(),
                Key::X => self.cut(),
                Key::V if shift => self.paste_plain(),
                Key::V => self.paste(),
                Key::Home => self.doc_start(shift),
                Key::End => self.doc_end(shift),
                Key::Left => self.move_word_left(shift),
                Key::Right => self.move_word_right(shift),
                _ => return KeyEdit::Unhandled,
            }
        } else if key.types_text() {
            let typed: String = key.typed().collect();
            self.insert_str(&typed);
        } else {
            match key.key {
                Key::Enter => self.insert_str("\n"),
                Key::Backspace => {
                    self.backspace();
                }
                Key::Delete => {
                    self.delete();
                }
                Key::Left => self.step_on_screen(false, shift, m),
                Key::Right => self.step_on_screen(true, shift, m),
                Key::Up => self.move_up(shift, m),
                Key::Down => self.move_down(shift, m),
                Key::Home => self.line_start(shift, m),
                Key::End => self.line_end(shift, m),
                _ => return KeyEdit::Unhandled,
            }
        }
        if self.doc == before {
            KeyEdit::Handled
        } else {
            KeyEdit::Changed
        }
    }

    /// A press at `(x, y)` in the field's box, from its top left: the caret
    /// to where it lands -- with Shift, the selection stretched to it.
    pub fn press(&mut self, x: f32, y: f32, shift: bool, m: &Metrics) {
        self.begin_or_end_selection(shift);
        self.drag_to(x, y, m);
        if !shift {
            // A drag from here selects from here.
            self.anchor = Some(self.cursor);
        }
    }

    /// The pointer held down, now at `(x, y)`: the selection's free end to
    /// it.
    pub fn drag_to(&mut self, x: f32, y: f32, m: &Metrics) {
        let lines = layout::lay_out(&self.doc, m);
        let y = y + self.scroll_y;
        let line = lines
            .iter()
            .find(|l| y < l.y + l.height)
            .or_else(|| lines.last());
        if let Some(line) = line {
            let (at, upstream) = layout::offset_at(&self.doc, line, x, m.size);
            self.cursor = at;
            self.typing = None;
            self.goal_x = None;
            // On a line of one direction the flag is only the line's end; on
            // a line of two it also says which side of a change of direction
            // the click was on.
            self.upstream = if line.is_ltr() {
                upstream && line.end == at
            } else {
                upstream
            };
        }
    }

    /// How tall the text is laid out.
    #[must_use]
    pub fn content_height(&self, m: &Metrics) -> f32 {
        layout::lay_out(&self.doc, m)
            .last()
            .map_or(0.0, |l| l.y + l.height)
    }

    /// Scroll by `dy` pixels -- down for positive -- held to the text, in
    /// a box `view` tall.
    pub fn scroll_by(&mut self, dy: f32, view: f32, m: &Metrics) {
        let most = (self.content_height(m) - view).max(0.0);
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, most);
    }

    /// Scroll so the caret's line shows in a box `view` tall.
    pub fn reveal_caret(&mut self, view: f32, m: &Metrics) {
        let lines = layout::lay_out(&self.doc, m);
        let here = layout::line_of(&lines, self.cursor, self.upstream);
        if let Some(line) = lines.get(here) {
            if line.y < self.scroll_y {
                self.scroll_y = line.y;
            } else if line.y + line.height > self.scroll_y + view {
                self.scroll_y = line.y + line.height - view;
            }
        }
    }

    // -----------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------

    /// Draw it in its box: the text in each run's format, the selection
    /// under it, and with the keyboard the caret -- or, empty, the
    /// placeholder.
    pub fn draw(&self, tree: &mut RenderTree, palette: &Palette, look: &Look<'_>, m: &Metrics) {
        let (bx, by, bw, bh) = look.rect;
        tree.push(RenderCommand::PushClip {
            x: bx,
            y: by,
            width: bw,
            height: bh,
        });
        let lines = layout::lay_out(&self.doc, m);
        let top = by - self.scroll_y;
        if self.doc.is_empty() && !look.placeholder.is_empty() {
            tree.push(RenderCommand::Text {
                x: bx,
                y: by,
                text: look.placeholder.to_string(),
                color: palette.subtext0,
                font_size: m.size,
                font_weight: FontWeightHint::Regular,
                max_width: Some(bw),
                overflow: TextOverflow::Ellipsis,
            });
        }
        let selection = self.selection_range();
        for line in lines
            .iter()
            .filter(|l| top + l.y + l.height >= by && top + l.y <= by + bh)
        {
            let line_top = top + line.y;
            if let Some((s, e)) = selection {
                self.draw_selection(tree, palette, line, (s, e), (bx, line_top), m.size);
            }
            for piece in &line.pieces {
                draw_piece(
                    tree,
                    palette,
                    &self.doc,
                    line,
                    piece,
                    (bx, line_top),
                    m.size,
                );
            }
        }
        if look.focused {
            let here = layout::line_of(&lines, self.cursor, self.upstream);
            if let Some(line) = lines.get(here) {
                let x = layout::x_at(&self.doc, line, self.cursor, self.upstream, m.size);
                tree.push(RenderCommand::FillRect {
                    x: bx + x,
                    y: top + line.y,
                    width: crate::textedit::CARET_WIDTH,
                    height: line.height,
                    color: palette.text,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
        tree.push(RenderCommand::PopClip);
    }

    /// The selection's part of `line`, under its text.
    fn draw_selection(
        &self,
        tree: &mut RenderTree,
        palette: &Palette,
        line: &Line,
        (s, e): (usize, usize),
        (left, line_top): (f32, f32),
        base: f32,
    ) {
        let (from, to) = (s.max(line.start), e.min(line.end));
        // A selection running on past the line's end -- over its line
        // break -- is drawn a little way past its text, as editors show it.
        let past_end = e > line.end;
        if from > to || (from == to && !past_end) {
            return;
        }
        let space = crate::text::measure(" ", base, FontWeightHint::Regular);
        let mut boxes: Vec<(f32, f32)> = Vec::new();
        if line.is_ltr() {
            let x0 = layout::x_of(&self.doc, line, from, base);
            let mut x1 = layout::x_of(&self.doc, line, to, base);
            if past_end {
                x1 += space;
            }
            boxes.push((x0, (x1 - x0).max(0.0)));
        } else {
            // Across two directions the selected text need not be one
            // stretch of the screen: a box for each piece's part of it, in
            // the piece's own direction.
            for piece in &line.pieces {
                let (f, t) = (from.max(piece.start), to.min(piece.end));
                if f >= t {
                    continue;
                }
                let text = self.doc.text().get(piece.start..piece.end).unwrap_or("");
                let (size, weight) = layout::font_of(&piece.format, base);
                for (x, w) in crate::text::selection_boxes(
                    text,
                    f.saturating_sub(piece.start),
                    t.saturating_sub(piece.start),
                    size,
                    weight,
                ) {
                    boxes.push((piece.x + x, w));
                }
            }
            if past_end {
                boxes.push((layout::x_of(&self.doc, line, line.end, base), space));
            }
        }
        for (x, width) in boxes {
            tree.push(RenderCommand::FillRect {
                x: left + x,
                y: line_top,
                width,
                height: line.height,
                color: palette.selection_fill(),
                corner_radii: CornerRadii::ZERO,
            });
        }
    }
}

/// Draw one piece of `line`: its text, on the line's baseline, and its
/// underline or line through.
fn draw_piece(
    tree: &mut RenderTree,
    palette: &Palette,
    doc: &RichDoc,
    line: &Line,
    piece: &Piece,
    (left, line_top): (f32, f32),
    base: f32,
) {
    let text = doc.text().get(piece.start..piece.end).unwrap_or("");
    let shown = text.trim_end_matches(['\n', '\r']);
    if shown.is_empty() {
        return;
    }
    let (size, weight) = layout::font_of(&piece.format, base);
    let color = piece.format.color.unwrap_or(palette.text);
    // Each piece's top, so that its baseline is the line's.
    let ascent = crate::text::ascent(size, weight);
    let y = line_top + (line.ascent - ascent);
    // Spaces alone draw nothing -- though an underline under them still
    // runs on, as a word processor's runs on under the space between two
    // underlined words.
    if !shown.trim().is_empty() {
        tree.push(RenderCommand::Text {
            x: left + piece.x,
            y,
            text: shown.to_string(),
            color,
            font_size: size,
            font_weight: weight,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
    let baseline = line_top + line.ascent;
    let thickness = (size / 14.0).max(1.0);
    let ink = shown.trim_end_matches([' ', '\t']);
    let width = if ink.len() == shown.len() {
        piece.width
    } else {
        crate::text::measure(ink, size, weight)
    };
    // The spaces after the ink are at its end: the right of a left-to-right
    // piece, the left of a right-to-left one.
    let ink_x = if piece.rtl {
        piece.x + (piece.width - width).max(0.0)
    } else {
        piece.x
    };
    let mut stroke = |y: f32| {
        tree.push(RenderCommand::FillRect {
            x: left + ink_x,
            y,
            width,
            height: thickness,
            color,
            corner_radii: CornerRadii::ZERO,
        });
    };
    if piece.format.underline {
        stroke(baseline + thickness);
    }
    if piece.format.strike {
        // Through the middle of a lower-case letter: about a third of the
        // way up from the baseline to the top of a capital.
        stroke(baseline - ascent * 0.3);
    }
}

/// Whether a step that replaced `removed` at `start` with `with` carries on
/// from `last`, to be gathered into it: typing at the end of what was
/// typed, within a word -- a space ends the step after it -- or deleting
/// next to what was deleted.
fn gathers(last: &Edit, start: usize, removed: &RichDoc, with: &RichDoc) -> bool {
    match last.kind {
        Kind::Typing => {
            removed.is_empty()
                && start == last.at.saturating_add(last.inserted.len())
                && !last.inserted.text().ends_with([' ', '\n'])
        }
        Kind::Deleting => {
            with.is_empty() && (start.saturating_add(removed.len()) == last.at || start == last.at)
        }
        Kind::Other => false,
    }
}

#[cfg(test)]
#[path = "richinput_tests.rs"]
mod tests;

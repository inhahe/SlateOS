//! A multi-line text field for applications: its text, its caret and its
//! selection, and the keys that edit it.
//!
//! `guitk::textinput::TextInput` holds one line. A regular-expression tester
//! needs a test input of many lines, and a mail client needs a message body,
//! and each would otherwise carry its own copy of the same few hundred lines:
//! the caret moves by characters and by lines (keeping its column across a
//! run of Up and Down), the selection follows Shift, typing replaces the
//! selection and stops at a capacity, and Enter is a line break.
//! `apps/regextester` carried it first; `apps/email` needed it next. It lives
//! here until the toolkit has one --
//! `requests/e-c-a-text-field-that-takes-its-own-keys.md` asks for the
//! one-line half.
//!
//! **What it does not do is draw.** Each application draws its own lines --
//! the tester highlights matches and numbers its lines, the mail client does
//! neither -- so this holds the model, and measures with the font size it was
//! made with so that a press and a vertical move land where the drawing put
//! the text.
//!
//! Offsets are bytes into the text, always on a character boundary.

use guitk::event::{Key, KeyEvent};
use guitk::render::FontWeightHint;
use guitk::text;

/// A text field of any number of lines.
#[derive(Debug, Clone, PartialEq)]
pub struct TextArea {
    text: String,
    /// The caret, as a byte offset on a character boundary.
    caret: usize,
    /// Where a selection started, when there is one.
    anchor: Option<usize>,
    /// Where Up and Down aim, in pixels from the line's start: kept across a
    /// run of them, so passing a short line does not pull the caret left.
    goal_x: Option<f32>,
    /// The size the text is drawn at, which measuring a line needs.
    font_size: f32,
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new(14.0)
    }
}

/// What a key did to a [`TextArea`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edited {
    /// The key is one a text field answers, whether or not it changed
    /// anything -- End at the end is still End.
    pub handled: bool,
    /// The text changed.
    pub changed: bool,
    /// What a copy or a cut took, for the application's clipboard.
    pub copied: Option<String>,
}

impl TextArea {
    /// An empty field whose text is drawn at `font_size`.
    #[must_use]
    pub fn new(font_size: f32) -> Self {
        Self {
            text: String::new(),
            caret: 0,
            anchor: None,
            goal_x: None,
            font_size,
        }
    }

    /// The text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret, as a byte offset.
    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Where the selection started, when there is one.
    #[must_use]
    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// Replace the text, with the caret at its end and nothing selected, as
    /// `TextInput::set_text` leaves it. [`Self::move_to`] puts it elsewhere --
    /// at the start, for a reply written above the message it quotes.
    pub fn set_text(&mut self, text: &str) {
        text.clone_into(&mut self.text);
        self.caret = self.text.len();
        self.anchor = None;
        self.goal_x = None;
    }

    /// The selected bytes, in order, when any are selected.
    #[must_use]
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.caret).then(|| (anchor.min(self.caret), anchor.max(self.caret)))
    }

    /// The selected text; empty with nothing selected.
    #[must_use]
    pub fn selected_text(&self) -> &str {
        self.selection()
            .and_then(|(a, b)| self.text.get(a..b))
            .unwrap_or("")
    }

    /// Where the line holding byte `at` starts.
    #[must_use]
    pub fn line_start(&self, at: usize) -> usize {
        self.text
            .get(..at)
            .and_then(|head| head.rfind('\n'))
            .map_or(0, |nl| nl.saturating_add(1))
    }

    /// Where the line holding byte `at` ends: its newline, or the end.
    #[must_use]
    pub fn line_end(&self, at: usize) -> usize {
        self.text
            .get(at..)
            .and_then(|tail| tail.find('\n'))
            .map_or(self.text.len(), |nl| at.saturating_add(nl))
    }

    /// Which line byte `at` is on, counting from zero.
    #[must_use]
    pub fn line_index(&self, at: usize) -> usize {
        self.text
            .get(..at)
            .map_or(0, |head| head.bytes().filter(|b| *b == b'\n').count())
    }

    /// Where line `index` starts, or the start of the last line past the end.
    #[must_use]
    pub fn start_of_line(&self, index: usize) -> usize {
        if index == 0 {
            return 0;
        }
        self.text
            .match_indices('\n')
            .nth(index.saturating_sub(1))
            .map_or_else(
                || self.line_start(self.text.len()),
                |(nl, _)| nl.saturating_add(1),
            )
    }

    /// How many lines the text has: one more than its line breaks.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.text.matches('\n').count().saturating_add(1)
    }

    /// Put the caret at `at`, extending the selection when `shift` is held.
    pub fn move_to(&mut self, at: usize, shift: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        let at = at.min(self.text.len());
        // On a character boundary, whatever the caller computed.
        self.caret = (0..=at)
            .rev()
            .find(|i| self.text.is_char_boundary(*i))
            .unwrap_or(0);
    }

    /// A character left -- or, with a selection and no Shift, to its start.
    pub fn left(&mut self, shift: bool) {
        self.goal_x = None;
        if !shift && let Some((from, _)) = self.selection() {
            self.move_to(from, false);
            return;
        }
        let at = self
            .text
            .get(..self.caret)
            .and_then(|head| head.chars().next_back())
            .map_or(0, |c| self.caret.saturating_sub(c.len_utf8()));
        self.move_to(at, shift);
    }

    /// A character right -- or, with a selection and no Shift, to its end.
    pub fn right(&mut self, shift: bool) {
        self.goal_x = None;
        if !shift && let Some((_, to)) = self.selection() {
            self.move_to(to, false);
            return;
        }
        let at = self
            .text
            .get(self.caret..)
            .and_then(|tail| tail.chars().next())
            .map_or(self.caret, |c| self.caret.saturating_add(c.len_utf8()));
        self.move_to(at, shift);
    }

    /// To the start of the caret's line.
    pub fn home(&mut self, shift: bool) {
        self.goal_x = None;
        self.move_to(self.line_start(self.caret), shift);
    }

    /// To the end of the caret's line.
    pub fn end(&mut self, shift: bool) {
        self.goal_x = None;
        self.move_to(self.line_end(self.caret), shift);
    }

    /// Up (`down == false`) or down by `lines` lines, aiming at the column
    /// the caret was at when the run of vertical moves began.
    pub fn vertical(&mut self, down: bool, lines: usize, shift: bool) {
        let start = self.line_start(self.caret);
        let here = self.line_index(self.caret);
        let goal = self.goal_x.unwrap_or_else(|| {
            let line = self.text.get(start..self.caret).unwrap_or("");
            text::measure(line, self.font_size, FontWeightHint::Regular)
        });
        let last = self.line_count().saturating_sub(1);
        let target = if down {
            here.saturating_add(lines).min(last)
        } else {
            here.saturating_sub(lines)
        };
        if target == here {
            // Past the first or the last line: to its start or its end, as
            // every text box does.
            let at = if down {
                self.line_end(self.caret)
            } else {
                start
            };
            self.move_to(at, shift);
            self.goal_x = None;
            return;
        }
        let from = self.start_of_line(target);
        let line = self.text.get(from..self.line_end(from)).unwrap_or("");
        let within = text::cursor_at(line, goal, self.font_size, FontWeightHint::Regular).byte;
        self.move_to(from.saturating_add(within), shift);
        self.goal_x = Some(goal);
    }

    /// Select everything.
    pub fn select_all(&mut self) {
        self.goal_x = None;
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    /// Take the selection out. Returns whether there was one.
    pub fn delete_selection(&mut self) -> bool {
        let Some((from, to)) = self.selection() else {
            return false;
        };
        self.text.replace_range(from..to, "");
        self.caret = from;
        self.anchor = None;
        true
    }

    /// Put `typed` where the caret is, over any selection, taking no more
    /// than leaves the field at `capacity` characters. Returns whether the
    /// text changed.
    pub fn insert(&mut self, typed: &str, capacity: usize) -> bool {
        self.goal_x = None;
        let removed = self.delete_selection();
        let room = capacity.saturating_sub(self.text.chars().count());
        // Line breaks and tabs are text here; other control characters --
        // a paste's carriage returns among them -- are not.
        let taken: String = typed
            .chars()
            .filter(|c| matches!(c, '\n' | '\t') || !c.is_control())
            .take(room)
            .collect();
        if taken.is_empty() {
            return removed;
        }
        self.text.insert_str(self.caret, &taken);
        self.caret = self.caret.saturating_add(taken.len());
        true
    }

    /// Take out the selection, or the character before the caret.
    pub fn backspace(&mut self) -> bool {
        self.goal_x = None;
        if self.delete_selection() {
            return true;
        }
        let Some(c) = self
            .text
            .get(..self.caret)
            .and_then(|h| h.chars().next_back())
        else {
            return false;
        };
        let from = self.caret.saturating_sub(c.len_utf8());
        self.text.replace_range(from..self.caret, "");
        self.caret = from;
        true
    }

    /// Take out the selection, or the character after the caret.
    pub fn delete(&mut self) -> bool {
        self.goal_x = None;
        if self.delete_selection() {
            return true;
        }
        let Some(c) = self.text.get(self.caret..).and_then(|t| t.chars().next()) else {
            return false;
        };
        let to = self.caret.saturating_add(c.len_utf8());
        self.text.replace_range(self.caret..to, "");
        true
    }

    /// Put the caret on line `line`, at `x` pixels from where the line's
    /// text starts.
    pub fn click(&mut self, line: usize, x: f32, shift: bool) {
        self.goal_x = None;
        let line = line.min(self.line_count().saturating_sub(1));
        let from = self.start_of_line(line);
        let text = self.text.get(from..self.line_end(from)).unwrap_or("");
        let within = text::cursor_at(text, x, self.font_size, FontWeightHint::Regular).byte;
        self.move_to(from.saturating_add(within), shift);
    }

    /// Apply a key: the moves (with Shift to select, Ctrl+Home and Ctrl+End
    /// to the ends, `page` lines for Page Up and Page Down), typing with
    /// Enter as a line break, deletion, and Ctrl+A, C, X and V against
    /// `clipboard`.
    ///
    /// Tab is not taken: which field comes next is the application's to
    /// say, and a key this answers is one the application does not see.
    pub fn apply_key(
        &mut self,
        key: &KeyEvent,
        capacity: usize,
        clipboard: &str,
        page: usize,
    ) -> Edited {
        let shift = key.modifiers.shift;
        let ctrl = key.modifiers.ctrl;
        let mut edited = Edited {
            handled: true,
            ..Edited::default()
        };
        match key.key {
            Key::Left => self.left(shift),
            Key::Right => self.right(shift),
            Key::Up => self.vertical(false, 1, shift),
            Key::Down => self.vertical(true, 1, shift),
            Key::PageUp => self.vertical(false, page.max(1), shift),
            Key::PageDown => self.vertical(true, page.max(1), shift),
            Key::Home if ctrl => self.move_to(0, shift),
            Key::End if ctrl => self.move_to(self.text.len(), shift),
            Key::Home => self.home(shift),
            Key::End => self.end(shift),
            Key::A if ctrl => self.select_all(),
            Key::C if ctrl => {
                if !self.selected_text().is_empty() {
                    edited.copied = Some(self.selected_text().to_owned());
                }
            }
            Key::X if ctrl => {
                if !self.selected_text().is_empty() {
                    edited.copied = Some(self.selected_text().to_owned());
                    edited.changed = self.delete_selection();
                }
            }
            Key::V if ctrl => edited.changed = self.insert(clipboard, capacity),
            Key::Enter => edited.changed = self.insert("\n", capacity),
            Key::Backspace => edited.changed = self.backspace(),
            Key::Delete => edited.changed = self.delete(),
            Key::Tab => edited.handled = false,
            _ => {
                if key.text.is_empty() || ctrl {
                    edited.handled = false;
                } else {
                    // Every character the keystroke produced, not just the
                    // first: a dead key followed by a letter composes into
                    // one, and an input method can deliver a whole word.
                    edited.changed = self.insert(&key.text, capacity);
                }
            }
        }
        edited
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use guitk::event::Modifiers;

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn shift(k: Key) -> KeyEvent {
        KeyEvent {
            modifiers: Modifiers::shift(),
            ..key(k)
        }
    }

    fn ctrl(k: Key) -> KeyEvent {
        KeyEvent {
            modifiers: Modifiers::ctrl(),
            ..key(k)
        }
    }

    fn typed(s: &str) -> KeyEvent {
        KeyEvent {
            text: s.to_owned(),
            ..key(Key::A)
        }
    }

    fn area(text: &str) -> TextArea {
        let mut a = TextArea::new(14.0);
        a.set_text(text);
        a
    }

    /// Typing goes in at the caret, Enter is a line break, and a paste's
    /// carriage returns and other control characters stay out.
    #[test]
    fn typing_and_line_breaks_go_in_at_the_caret() {
        let mut a = TextArea::new(14.0);
        for k in [typed("ab"), key(Key::Enter), typed("c")] {
            assert!(a.apply_key(&k, 100, "", 5).changed);
        }
        assert_eq!(a.text(), "ab\nc");
        a.apply_key(&ctrl(Key::V), 100, "x\r\ny\u{7}z", 5);
        assert_eq!(a.text(), "ab\ncx\nyz");
        // Anywhere, not only at the end.
        a.move_to(1, false);
        a.apply_key(&typed("Q"), 100, "", 5);
        assert_eq!(a.text(), "aQb\ncx\nyz");
    }

    /// Up and Down keep the column across a short line, and stop at the
    /// ends by going to the line's start or end.
    #[test]
    fn up_and_down_keep_the_column() {
        let mut a = area("abcdef\nx\nabcdef");
        a.move_to(5, false);
        a.apply_key(&key(Key::Down), 100, "", 5);
        assert_eq!(a.caret(), "abcdef\nx".len(), "a short line takes its end");
        a.apply_key(&key(Key::Down), 100, "", 5);
        assert_eq!(a.caret(), "abcdef\nx\nabcde".len(), "the column came back");
        a.apply_key(&key(Key::Down), 100, "", 5);
        assert_eq!(a.caret(), a.text().len(), "past the last line: its end");
        a.apply_key(&ctrl(Key::Home), 100, "", 5);
        a.apply_key(&key(Key::Up), 100, "", 5);
        assert_eq!(a.caret(), 0);
    }

    /// Shift selects; typing replaces the selection; copy and cut hand the
    /// selection to the application's clipboard.
    #[test]
    fn a_selection_is_made_copied_cut_and_typed_over() {
        let mut a = area("hello world");
        a.apply_key(&key(Key::End), 100, "", 5);
        for _ in 0..5 {
            a.apply_key(&shift(Key::Left), 100, "", 5);
        }
        assert_eq!(a.selected_text(), "world");
        let copied = a.apply_key(&ctrl(Key::C), 100, "", 5);
        assert_eq!(copied.copied.as_deref(), Some("world"));
        assert!(!copied.changed);
        let cut = a.apply_key(&ctrl(Key::X), 100, "", 5);
        assert_eq!((cut.copied.as_deref(), cut.changed), (Some("world"), true));
        assert_eq!(a.text(), "hello ");
        a.apply_key(&ctrl(Key::A), 100, "", 5);
        a.apply_key(&typed("bye"), 100, "", 5);
        assert_eq!(a.text(), "bye");
    }

    /// The capacity is characters, not bytes, and a full field takes
    /// nothing more.
    #[test]
    fn the_capacity_is_characters() {
        let mut a = TextArea::new(14.0);
        a.apply_key(&typed("\u{e9}\u{e9}\u{e9}"), 2, "", 5);
        assert_eq!(a.text(), "\u{e9}\u{e9}");
        assert!(!a.apply_key(&typed("x"), 2, "", 5).changed);
    }

    /// Backspace and Delete take a whole character, and the caret stays on
    /// a boundary whatever it is moved to.
    #[test]
    fn deletion_takes_whole_characters() {
        let mut a = area("a\u{e9}b");
        a.move_to(3, false);
        assert!(a.apply_key(&key(Key::Backspace), 10, "", 5).changed);
        assert_eq!(a.text(), "ab");
        a.move_to(0, false);
        a.apply_key(&key(Key::Delete), 10, "", 5);
        assert_eq!(a.text(), "b");
        let mut b = area("\u{e9}");
        b.move_to(1, false);
        assert_eq!(b.caret(), 0, "a caret inside a character");
    }

    /// Tab is the application's, and so is a Ctrl chord this does not use.
    #[test]
    fn tab_and_unknown_chords_are_left_to_the_application() {
        let mut a = TextArea::new(14.0);
        assert!(!a.apply_key(&key(Key::Tab), 10, "", 5).handled);
        assert!(!a.apply_key(&ctrl(Key::S), 10, "", 5).handled);
        assert!(a.apply_key(&key(Key::End), 10, "", 5).handled);
        assert!(a.text().is_empty());
    }

    /// A press puts the caret on the line and column pressed.
    #[test]
    fn a_press_puts_the_caret_where_it_lands() {
        let mut a = area("first\nsecond");
        a.click(1, 1_000.0, false);
        assert_eq!(a.caret(), a.text().len());
        a.click(0, 0.0, false);
        assert_eq!(a.caret(), 0);
        a.click(9, 0.0, true);
        assert_eq!(a.selected_text(), "first\n");
    }
}

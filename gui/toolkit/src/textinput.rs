//! A single-line text field's state: the text, the caret, the selection and a
//! clipboard, with the editing operations that move between them.
//!
//! State only. Nothing here draws — a caller owns the rectangle, the colours
//! and the focus, and asks this for what to put in them. That split is why one
//! type can serve a settings row, a command box and a search field without any
//! of them inheriting the others' appearance.
//!
//! # Why this is not a fresh implementation
//!
//! It was private to `gui/desktop/src/run_dialog.rs` until 2026-09-17, and
//! twenty-two files in this tree hand-roll typing against
//! `KeyEvent::types_text` because they could not reach it. The obvious repair
//! — write a text field in the toolkit — would have produced a second,
//! worse editor beside a good one: this one understands **bidirectional text**,
//! and a caret that does not is visibly wrong the first time somebody types
//! Arabic or Hebrew into it. See [`TextCursor`] and design-decisions 541.
//!
//! # The caret invariant
//!
//! The caret is a byte offset that is always on a character boundary, together
//! with which side of a direction boundary it sits on. Both halves matter, and
//! neither is checked at runtime, which is why the fields are private: a caller
//! that set the text without moving the caret would leave an offset pointing
//! into the middle of a character, and the next arrow key would panic.

use crate::render::FontWeightHint;
use crate::text;
use crate::text::TextCursor;
use crate::textedit;

/// Single-line text input state with cursor, selection, and clipboard.
#[derive(Clone, Debug, Default)]
pub struct TextInput {
    /// The text content.
    text: String,
    /// The caret: a byte offset (always at a char boundary) *and* which side of
    /// a direction boundary it sits on.
    ///
    /// The second half is what makes the visual arrows safe. Where a
    /// left-to-right and a right-to-left run meet, one byte offset names two
    /// places on screen; a caret rebuilt from the offset alone cannot tell them
    /// apart and steps over the whole right-to-left word in a single press,
    /// which `design-decisions.md` §541 records as worse than not moving
    /// visually at all.
    cursor: TextCursor,
    /// Selection anchor (byte offset). If `Some`, selection spans anchor..cursor.
    ///
    /// A plain byte on purpose: a selection is a *range of text*, not a place
    /// on screen, so it has no side of a boundary to be on. Only the caret
    /// does.
    selection_anchor: Option<usize>,
    /// Clipboard contents (internal; real clipboard would use IPC).
    clipboard: String,
}

impl TextInput {
    /// The text, as typed.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Where the caret is.
    #[must_use]
    pub const fn cursor(&self) -> TextCursor {
        self.cursor
    }

    /// Put the caret somewhere.
    ///
    /// Takes a [`TextCursor`] rather than a byte offset so that a caller cannot
    /// express half of the caret: the side of a direction boundary is not
    /// recoverable from the offset, which is the whole reason `TextCursor`
    /// exists.
    pub fn set_cursor(&mut self, cursor: TextCursor) {
        self.cursor = cursor;
    }

    /// Where the selection was anchored, if there is one.
    #[must_use]
    pub const fn selection_anchor(&self) -> Option<usize> {
        self.selection_anchor
    }

    /// Anchor a selection, or clear it with `None`.
    pub fn set_selection_anchor(&mut self, anchor: Option<usize>) {
        self.selection_anchor = anchor;
    }

    /// Put text on this field's clipboard.
    ///
    /// For a caller arranging a paste it did not cut -- a test, or a menu
    /// command wired to a real clipboard service when one exists.
    pub fn set_clipboard(&mut self, text: String) {
        self.clipboard = text;
    }

    /// What the last cut or copy put on the clipboard.
    ///
    /// Internal to this field for now: there is no clipboard service to ask,
    /// so text cut here can only be pasted here. That is a smaller promise
    /// than the name suggests and is made explicit rather than left to be
    /// discovered.
    #[must_use]
    pub fn clipboard(&self) -> &str {
        &self.clipboard
    }

    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: TextCursor::default(),
            selection_anchor: None,
            clipboard: String::new(),
        }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = TextCursor::default();
        self.selection_anchor = None;
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = TextCursor::from(self.text.len());
        self.selection_anchor = None;
    }

    /// Returns (start, end) byte offsets of the selection, or (cursor, cursor).
    pub fn selection_range(&self) -> (usize, usize) {
        // The empty answer is the caret twice over rather than `None`: every
        // caller here goes on to splice at that range, and an empty range is
        // the right splice. `textedit::selected_range` answers `None`, which
        // is the right shape for its own callers; this line is the conversion.
        let at = self.cursor.byte();
        textedit::selected_range(self.cursor, self.selection_anchor).unwrap_or((at, at))
    }

    pub fn has_selection(&self) -> bool {
        self.selection_anchor
            .is_some_and(|a| a != self.cursor.byte())
    }

    pub fn selected_text(&self) -> &str {
        let (start, end) = self.selection_range();
        self.text.get(start..end).unwrap_or("")
    }

    /// The largest character boundary at or before `at`, and never past the
    /// end of the text.
    ///
    /// Every offset this type holds is supposed to be on a boundary already.
    /// This is what makes that a *property* rather than an assumption: the
    /// primitives below all pass their offsets through here, so a stale or
    /// mid-character offset shortens an edit instead of panicking inside
    /// `String::replace_range`.
    /// The answer lives in the toolkit: one implementation of "where is the
    /// nearest caret stop" for every text field in the system, rather than one
    /// per field, each free to drift from the others.
    fn floor_boundary(&self, at: usize) -> usize {
        TextCursor::from(at).snapped_in(&self.text).byte()
    }

    /// The byte offset of the character before `at`, or `at` at the start.
    fn prev_boundary(&self, at: usize) -> usize {
        let at = TextCursor::from(at).snapped_in(&self.text);
        at.prev_in(&self.text).unwrap_or(at).byte()
    }

    /// The byte offset just past the character at `at`, or `at` at the end.
    fn next_boundary(&self, at: usize) -> usize {
        let at = TextCursor::from(at).snapped_in(&self.text);
        at.next_in(&self.text).unwrap_or(at).byte()
    }

    /// Replace the bytes in `start..end` with `with`, leaving the cursor just
    /// past the inserted text and nothing selected.
    ///
    /// The single place `text` is mutated. Insert, paste, delete, backspace
    /// and delete-selection are all this operation with different arguments,
    /// and each used to spell out its own `drain`/`insert` plus its own cursor
    /// adjustment — five chances to move the cursor to somewhere the text no
    /// longer has a character.
    fn replace_range(&mut self, start: usize, end: usize, with: &str) {
        let start = self.floor_boundary(start);
        let end = self.floor_boundary(end).max(start);
        self.text.replace_range(start..end, with);
        self.cursor = TextCursor::from(start.saturating_add(with.len()));
        self.selection_anchor = None;
    }

    /// Remove whatever is selected, leaving the caret where it was.
    pub fn delete_selection(&mut self) {
        // The shared one, which also refuses a range that is not on character
        // boundaries rather than panicking inside `drain`.
        // The answer -- whether anything was deleted -- is not needed here.
        // This method's contract is "afterwards nothing is selected", and that
        // holds whether or not there was a selection to begin with.
        let _deleted = textedit::delete_selection(
            &mut self.text,
            &mut self.cursor,
            &mut self.selection_anchor,
        );
    }

    pub fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.cursor = TextCursor::from(self.text.len());
    }

    /// Update the selection anchor for a cursor move: holding shift starts (or
    /// keeps) a selection, releasing it drops one.
    fn anchor_for_move(&mut self, shift: bool) {
        // Delegated rather than written out. This type arrived in the toolkit
        // from `run_dialog`, where it predated `textedit` and carried its own
        // copies of three of these primitives; `modal` and `widget` were
        // already calling the shared ones, which made *this* the odd one out.
        textedit::begin_or_end_selection(shift, self.cursor, &mut self.selection_anchor);
    }

    /// Move the caret one place `left`/`right` **on screen**.
    ///
    /// Takes the font it is drawn at, and does not remember one. A field that
    /// stored its own size would be measuring at whatever it was last told,
    /// which is the same size the caller draws at only until the caller changes
    /// its mind -- and a caret measured against the wrong font steps over a
    /// right-to-left word in one press, which design-decisions 541 records as
    /// worse than not moving visually at all.
    pub fn move_cursor_left(&mut self, shift: bool, font_size: f32, weight: FontWeightHint) {
        // An unshifted arrow against a selection collapses it to that end
        // rather than moving — the cursor lands where the selection was, not
        // one character further.
        if !shift && self.has_selection() {
            let (start, _) = self.selection_range();
            self.cursor = TextCursor::from(start);
            self.selection_anchor = None;
            return;
        }
        self.anchor_for_move(shift);
        // One place left on the *screen*, not one character back through the
        // string: on a line that mixes directions those are different moves.
        // `design-decisions.md` §541. Measured at the size and weight the input
        // is drawn at, because the gaps between glyphs belong to the shaped
        // run, and assigned whole so the affinity survives the keypress.
        if let Some(prev) = text::caret_left(&self.text, self.cursor, font_size, weight) {
            self.cursor = prev;
        }
    }

    /// Move the caret one place `left`/`right` **on screen**.
    ///
    /// Takes the font it is drawn at, and does not remember one. A field that
    /// stored its own size would be measuring at whatever it was last told,
    /// which is the same size the caller draws at only until the caller changes
    /// its mind -- and a caret measured against the wrong font steps over a
    /// right-to-left word in one press, which design-decisions 541 records as
    /// worse than not moving visually at all.
    pub fn move_cursor_right(&mut self, shift: bool, font_size: f32, weight: FontWeightHint) {
        if !shift && self.has_selection() {
            let (_, end) = self.selection_range();
            self.cursor = TextCursor::from(end);
            self.selection_anchor = None;
            return;
        }
        self.anchor_for_move(shift);
        // Visual, for the reason given in `move_cursor_left` above.
        if let Some(next) = text::caret_right(&self.text, self.cursor, font_size, weight) {
            self.cursor = next;
        }
    }

    pub fn move_home(&mut self, shift: bool) {
        self.anchor_for_move(shift);
        self.cursor = TextCursor::default();
    }

    pub fn move_end(&mut self, shift: bool) {
        self.anchor_for_move(shift);
        self.cursor = TextCursor::from(self.text.len());
    }

    pub fn insert_char(&mut self, ch: char) {
        let (start, end) = self.selection_range();
        let mut buf = [0u8; 4];
        self.replace_range(start, end, ch.encode_utf8(&mut buf));
    }

    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        // Logical, deliberately, while the arrows above are visual: Backspace
        // deletes the character before this one *in the string*, which is what
        // a reader of that script means wherever it happens to be drawn.
        // Deleting and moving are allowed to disagree.
        let at = self.cursor.byte();
        let start = self.prev_boundary(at);
        self.replace_range(start, at, "");
    }

    pub fn delete(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        // Logical, as with Backspace above.
        let at = self.cursor.byte();
        let end = self.next_boundary(at);
        self.replace_range(at, end, "");
    }

    pub fn cut(&mut self) {
        if self.has_selection() {
            self.clipboard = self.selected_text().to_string();
            self.delete_selection();
        }
    }

    pub fn copy(&mut self) {
        if self.has_selection() {
            self.clipboard = self.selected_text().to_string();
        }
    }

    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let (start, end) = self.selection_range();
        let clip = core::mem::take(&mut self.clipboard);
        self.replace_range(start, end, &clip);
        self.clipboard = clip;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test that indexes out of range should fail loudly at the line that did it"
)]
mod tests {
    use super::*;
    use crate::text::TextCursor;

    /// The size these tests measure at.
    ///
    /// Any size would do for most of them; the visual-caret ones need *a*
    /// size because a caret that moves by screen position cannot be computed
    /// without one. It was `run_dialog`'s `INPUT_FONT_SIZE` when these tests
    /// lived there.
    const FONT_SIZE: f32 = 13.0;

    #[test]
    fn test_text_input_insert() {
        let mut input = TextInput::new();
        input.insert_char('h');
        input.insert_char('e');
        input.insert_char('l');
        input.insert_char('l');
        input.insert_char('o');
        assert_eq!(input.text(), "hello");
        assert_eq!(input.cursor().byte(), 5);
    }
    #[test]
    fn test_text_input_backspace() {
        let mut input = TextInput::new();
        input.set_text("hello");
        input.backspace();
        assert_eq!(input.text(), "hell");
        assert_eq!(input.cursor().byte(), 4);
    }
    #[test]
    fn test_text_input_delete() {
        let mut input = TextInput::new();
        input.set_text("hello");
        input.set_cursor(TextCursor::from(0));
        input.delete();
        assert_eq!(input.text(), "ello");
        assert_eq!(input.cursor().byte(), 0);
    }
    #[test]
    fn test_text_input_cursor_movement() {
        let mut input = TextInput::new();
        input.set_text("hello");
        assert_eq!(input.cursor().byte(), 5);
        input.move_cursor_left(false, FONT_SIZE, FontWeightHint::Regular);
        assert_eq!(input.cursor().byte(), 4);
        input.move_cursor_left(false, FONT_SIZE, FontWeightHint::Regular);
        assert_eq!(input.cursor().byte(), 3);
        input.move_cursor_right(false, FONT_SIZE, FontWeightHint::Regular);
        assert_eq!(input.cursor().byte(), 4);
        input.move_home(false);
        assert_eq!(input.cursor().byte(), 0);
        input.move_end(false);
        assert_eq!(input.cursor().byte(), 5);
    }
    #[test]
    fn test_text_input_selection() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.move_home(false);
        // Select "hello" with shift+right x5
        for _ in 0..5 {
            input.move_cursor_right(true, FONT_SIZE, FontWeightHint::Regular);
        }
        assert!(input.has_selection());
        assert_eq!(input.selected_text(), "hello");
        assert_eq!(input.selection_range(), (0, 5));
    }
    #[test]
    fn test_text_input_select_all() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.select_all();
        assert_eq!(input.selected_text(), "hello world");
    }
    #[test]
    fn test_text_input_delete_selection() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.set_selection_anchor(Some(0));
        input.set_cursor(TextCursor::from(5));
        input.delete_selection();
        assert_eq!(input.text(), " world");
        assert_eq!(input.cursor().byte(), 0);
    }
    #[test]
    fn test_text_input_cut_paste() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.select_all();
        input.cut();
        assert_eq!(input.text(), "");
        assert_eq!(input.clipboard(), "hello world");
        input.paste();
        assert_eq!(input.text(), "hello world");
    }
    #[test]
    fn a_shifted_arrow_extends_the_selection_and_an_unshifted_one_collapses_it() {
        let mut input = TextInput::new();
        input.set_text("abcdef");
        input.move_home(false);
        input.move_cursor_right(true, FONT_SIZE, FontWeightHint::Regular);
        input.move_cursor_right(true, FONT_SIZE, FontWeightHint::Regular);
        assert_eq!(input.selected_text(), "ab");

        // Unshifted Left collapses to the near end without moving further.
        input.move_cursor_left(false, FONT_SIZE, FontWeightHint::Regular);
        assert_eq!(input.cursor().byte(), 0);
        assert!(!input.has_selection());

        input.move_cursor_right(true, FONT_SIZE, FontWeightHint::Regular);
        input.move_cursor_right(true, FONT_SIZE, FontWeightHint::Regular);
        input.move_cursor_right(false, FONT_SIZE, FontWeightHint::Regular);
        assert_eq!(input.cursor().byte(), 2);
        assert!(!input.has_selection());
    }
    #[test]
    fn backspace_and_delete_remove_one_whole_character() {
        let mut input = TextInput::new();
        input.set_text("a😀b");
        input.move_end(false);
        input.move_cursor_left(false, FONT_SIZE, FontWeightHint::Regular); // before 'b'
        input.backspace();
        assert_eq!(input.text(), "ab");
        assert_eq!(input.cursor().byte(), 1);

        let mut input = TextInput::new();
        input.set_text("a😀b");
        input.move_home(false);
        input.move_cursor_right(false, FONT_SIZE, FontWeightHint::Regular); // after 'a'
        input.delete();
        assert_eq!(input.text(), "ab");
        assert_eq!(input.cursor().byte(), 1);
    }
    #[test]
    fn typing_or_pasting_over_a_selection_replaces_it() {
        let mut input = TextInput::new();
        input.set_text("hello world");
        input.set_selection_anchor(Some(0));
        input.set_cursor(TextCursor::from(5));
        input.insert_char('X');
        assert_eq!(input.text(), "X world");
        assert_eq!(input.cursor().byte(), 1);
        assert!(!input.has_selection());

        let mut input = TextInput::new();
        input.set_text("hello world");
        input.set_clipboard("bye".to_string());
        input.set_selection_anchor(Some(0));
        input.set_cursor(TextCursor::from(5));
        input.paste();
        assert_eq!(input.text(), "bye world");
        assert_eq!(input.cursor().byte(), 3);
        // Pasting does not consume the clipboard.
        assert_eq!(input.clipboard(), "bye");
    }
    /// The Run dialog's arrows walk the *screen*, not the string
    /// (`design-decisions.md` §541).
    ///
    /// `"ab\u{05D0}\u{05D1}cd"` draws as `a b <bet> <aleph> c d` — the two
    /// Hebrew letters run right-to-left inside a left-to-right line, so the
    /// character stored second is painted first. Six letters, so six caret
    /// stops in each direction, and the same six screen positions both ways.
    ///
    /// The two gaps where the directions meet — `b|<bet>` and `<aleph>|c` —
    /// each answer to *both* byte 2 and byte 6. Which one is reported depends
    /// on the side the caret is on, and the caret keeps the side it is
    /// travelling towards: walking left it reports 6 at both gaps, walking
    /// right it reports 2 at both. That is why the sequences below repeat a
    /// number, and why the whole `TextCursor` is assigned rather than its byte.
    ///
    /// **A failure here showing a shorter sequence, or one without the repeat,
    /// is §541's measured trap**: a field that kept only the byte cannot tell
    /// the second 6 from the first and jumps the entire Hebrew word in one
    /// keypress — worse than the logical motion this replaced.
    #[test]
    fn the_run_dialogs_arrows_walk_the_line_by_the_screen_not_by_the_string() {
        let mut input = TextInput::new();
        input.set_text("ab\u{05D0}\u{05D1}cd");
        input.move_end(false);

        let mut leftwards = Vec::new();
        for _ in 0..6 {
            input.move_cursor_left(false, FONT_SIZE, FontWeightHint::Regular);
            leftwards.push(input.cursor().byte());
        }
        assert_eq!(leftwards, vec![7, 6, 4, 6, 1, 0]);

        let mut rightwards = Vec::new();
        for _ in 0..6 {
            input.move_cursor_right(false, FONT_SIZE, FontWeightHint::Regular);
            rightwards.push(input.cursor().byte());
        }
        assert_eq!(rightwards, vec![1, 2, 4, 2, 7, 8]);
    }
}

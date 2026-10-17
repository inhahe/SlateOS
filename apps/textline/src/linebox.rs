//! A program's one-line box: its text itself, edited at a caret.
//!
//! Twenty-four applications kept their search as a `String` with a
//! `TextInput` beside it holding the same text, reloaded from the string
//! before every key, press and menu row, and drawn with a caret at the end
//! whenever the two had come apart. Each also decided for itself which keys
//! were the box's, and two of them -- calendar and reminders -- took F1 from
//! the window while a search was being typed. Here the `TextInput` *is* the
//! text, so there is nothing to keep in step, and [`LineBox::key`] answers
//! which keys are the box's: one it does not answer is the window's. Every
//! search box is one; the other one-line boxes are moving onto it
//! (`known-issues/E-twenty-eight-applications-keep-a-box-s-text-twice.md`).
//!
//! **What stays with the program** is what differs between them: where the
//! box is, whether it has the keyboard, what Escape, Enter and Tab do there,
//! and what follows a change to the text -- a list back to its top, a
//! selection kept on what a search still finds.

use guitk::color::Color;
use guitk::event::KeyEvent;
use guitk::menu::{ContextMenu, MenuItemId};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::text::TextCursor;
use guitk::textedit;
use guitk::textinput::{KeyEdit, TextInput};

/// What a box read from its end draws in place of the text it cuts.
const ELLIPSIS: &str = "…";

/// A one-line box's text, caret and selection, and the size it is drawn at.
#[derive(Clone, Debug)]
pub struct LineBox {
    input: TextInput,
    font_size: f32,
    /// Whether text too long for the box is cut at its front while the box
    /// has not the keyboard ([`LineBox::from_its_end`]).
    from_end: bool,
}

/// What one key did to a [`LineBox`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineKey {
    /// Whether the key was the box's: typing, the caret's keys, Backspace and
    /// Delete, Ctrl+A, C, X and V (`apply_key`). One it is not -- Enter,
    /// Escape, Tab, Up, Down, F1, any other command -- is the window's, for
    /// the program to act on as it would with the box closed.
    pub handled: bool,
    /// Whether the text changed.
    pub changed: bool,
    /// Whether the caret or the selection is not where it was -- an arrow,
    /// Home, Ctrl+A, and any key that changed the text: what a box redraws
    /// for though its text is the same.
    pub moved: bool,
    /// Whether the key put the selection on the clipboard (Ctrl+C, Ctrl+X):
    /// something done that neither the text nor its caret shows.
    pub copied: bool,
}

/// Where and how a [`LineBox`] is drawn.
#[derive(Clone, Copy, Debug)]
pub struct LineLook<'a> {
    /// The left edge of the box's text, inside its frame.
    pub x: f32,
    /// The top of the line the text sits on.
    pub y: f32,
    /// How wide the text may be: longer text scrolls under the caret
    /// while the box has the keyboard, and is cut with `…` while it does not.
    pub width: f32,
    /// The line's height: the caret's and the selection's.
    pub line_height: f32,
    /// Whether the box has the keyboard, which is what draws the caret and
    /// the selection.
    pub focused: bool,
    /// What an empty box says, with the keyboard or without -- what it is
    /// for (what a search searches), and, while it has not the keyboard, the
    /// key that reaches it. Empty, it says nothing.
    pub placeholder: &'a str,
    /// The text's colour, and the caret's.
    pub color: Color,
    /// The placeholder's colour: a hint the user reads, so a readable one
    /// (`subtext0`), not the disabled grey.
    pub placeholder_color: Color,
    /// What the selection is painted in.
    pub selection_bg: Color,
    /// What selected text is drawn in.
    pub selection_fg: Color,
    /// How wide the caret is drawn (`textedit::CARET_WIDTH` unless the user
    /// has scaled it).
    pub caret_width: f32,
}

impl LineBox {
    /// An empty box holding at most `capacity` characters, its text drawn at
    /// `font_size` -- the size its caret moves and its presses are measured
    /// at.
    #[must_use]
    pub fn new(capacity: usize, font_size: f32) -> Self {
        Self::holding("", capacity, font_size)
    }

    /// A box holding `text`, its caret after it, which typing and pasting
    /// then fill to at most `capacity` characters -- `text` itself is not
    /// cut, as [`crate::field_holding`] does not cut it -- drawn at
    /// `font_size`: a box that starts from something, an address to look up.
    #[must_use]
    pub fn holding(text: &str, capacity: usize, font_size: f32) -> Self {
        Self {
            input: crate::field_holding(text, capacity),
            font_size,
            from_end: false,
        }
    }

    /// The box, read from its end while it has not the keyboard: text too
    /// long for it is drawn cut at the front, `…` and the tail that fits
    /// after it, and a press lands on that tail where it was drawn -- for a
    /// path, whose tail says which directory it is and whose head every
    /// sibling shares. With the keyboard it is drawn as any box is,
    /// scrolled under its caret.
    #[must_use]
    pub fn from_its_end(mut self) -> Self {
        self.from_end = true;
        self
    }

    /// What is in the box.
    #[must_use]
    pub fn text(&self) -> &str {
        self.input.text()
    }

    /// Whether the box is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.input.text().is_empty()
    }

    /// Measure, move the caret through and draw the text at `font_size` from
    /// now on: for a box whose text follows the window's size.
    pub fn set_font_size(&mut self, font_size: f32) {
        self.font_size = font_size;
    }

    /// The field underneath, for a caller that reads the caret or the
    /// selection.
    #[must_use]
    pub fn input(&self) -> &TextInput {
        &self.input
    }

    /// Put `text` in the box from outside it -- a link followed, a filter
    /// chosen elsewhere, the name a rename starts from: the caret goes to its
    /// end. Answers whether the text changed; when it did not, the caret and
    /// the selection stay where they were.
    pub fn set_text(&mut self, text: &str) -> bool {
        if self.input.text() == text {
            return false;
        }
        self.input.set_text(text);
        true
    }

    /// Empty the box. Answers whether there was text to empty.
    pub fn clear(&mut self) -> bool {
        self.set_text("")
    }

    /// Select the whole text, for the next key to type over -- what Ctrl+F
    /// does in a program whose search box has the keyboard already.
    pub fn select_all(&mut self) {
        self.input.select_all();
    }

    /// The caret after the text, nothing selected: where a box that takes
    /// the keyboard by a key or a tab, rather than by a press, puts it.
    pub fn caret_to_end(&mut self) {
        self.input.set_selection_anchor(None);
        self.input
            .set_cursor(TextCursor::from(self.input.text().len()));
    }

    /// Apply `key` to the box -- on the program's clipboard, as its menu is
    /// (`apply_key_on_clipboard`) -- and say whether the key was the box's
    /// and what it did.
    pub fn key(&mut self, key: &KeyEvent) -> LineKey {
        let font_size = self.font_size;
        self.edited(|input| crate::apply_key_on_clipboard(input, key, font_size))
    }

    /// Make `change` to the field, and say what it did: `handled` and
    /// `copied` as `change` answers them, and whether the text changed and
    /// the caret or the selection moved.
    fn edited(&mut self, change: impl FnOnce(&mut TextInput) -> crate::LineEdit) -> LineKey {
        let before = self.input.text().to_owned();
        let caret = |input: &TextInput| (input.cursor(), input.selection_anchor());
        let caret_before = caret(&self.input);
        let edit = change(&mut self.input);
        LineKey {
            handled: edit.handled,
            changed: self.input.text() != before,
            moved: caret(&self.input) != caret_before,
            copied: edit.copied.is_some(),
        }
    }

    /// [`key`](Self::key), with an edit `check` refuses undone: `check` is
    /// given the text the key would leave, and answers whether to keep it. A
    /// key refused is still the box's (`handled`) and changes nothing, its
    /// caret included -- a letter typed into a number's box, a paste of what
    /// is not a number. A key that changes no text is not checked.
    pub fn key_checked(&mut self, key: &KeyEvent, check: impl FnOnce(&str) -> bool) -> LineKey {
        let kept = self.input.clone();
        let edit = self.key(key);
        if edit.changed && !check(self.input.text()) {
            self.input = kept;
            return LineKey {
                handled: true,
                changed: false,
                moved: false,
                copied: edit.copied,
            };
        }
        edit
    }

    /// Type `text` at the caret, over the selection, up to the box's
    /// capacity, as a key typing it would: for a box whose program decides
    /// what a key types into it -- colorpicker's hex box upper-cases what is
    /// typed and drops what is not hex. What it did, as [`key`](Self::key)
    /// says it; always the box's. An empty `text` changes nothing, the
    /// selection included.
    pub fn insert(&mut self, text: &str) -> LineKey {
        self.edited(|input| {
            input.insert_text(text);
            crate::LineEdit {
                handled: true,
                copied: None,
            }
        })
    }

    /// A press `dx` pixels right of the text's left edge, in a box whose text
    /// is `width` wide: the caret goes where it landed, against the text as
    /// it was drawn -- scrolled under the caret if the box had the keyboard
    /// (`focused`); if it did not, from its start, or cut at its front for a
    /// box [read from its end](Self::from_its_end) -- and any selection goes.
    pub fn press(&mut self, dx: f32, width: f32, focused: bool) {
        let width = width.max(0.0);
        let cursor = if !focused && self.from_end {
            self.press_in_tail(dx, width)
        } else {
            let drawn = if focused {
                self.input.cursor()
            } else {
                TextCursor::default()
            };
            textedit::cursor_at_click(
                self.input.text(),
                drawn,
                width,
                self.font_size,
                FontWeightHint::Regular,
                dx,
            )
        };
        self.input.set_selection_anchor(None);
        self.input.set_cursor(cursor);
    }

    /// Where a press `dx` pixels right of the text's left edge lands in a box
    /// read from its end, without the keyboard, `width` wide, as
    /// [`draw`](Self::draw) drew it: text that fits from its start; `…` and
    /// the tail after it, a press on the `…` landing before the tail; and
    /// where not even the `…` fits and nothing was drawn, the end.
    fn press_in_tail(&self, dx: f32, width: f32) -> TextCursor {
        let text = self.input.text();
        let weight = FontWeightHint::Regular;
        if guitk::text::measure(text, self.font_size, weight) <= width {
            return guitk::text::cursor_at(text, dx, self.font_size, weight);
        }
        let shown = guitk::text::elide_start(text, width, ELLIPSIS, self.font_size, weight);
        let Some(tail) = shown.strip_prefix(ELLIPSIS) else {
            return TextCursor::from(text.len());
        };
        // The tail is the end of the text, so it starts on a character's
        // boundary, and so does every place in it.
        let start = text.len().saturating_sub(tail.len());
        let lead = guitk::text::measure(ELLIPSIS, self.font_size, weight);
        let within = guitk::text::cursor_at(tail, dx - lead, self.font_size, weight);
        TextCursor::from(start.saturating_add(within.byte()))
    }

    /// The box's right-click menu: Cut, Copy, Paste, Delete and Select All,
    /// each offered when it would do something.
    #[must_use]
    pub fn edit_menu(&self) -> ContextMenu {
        self.input.edit_menu()
    }

    /// The menu's row `id`, chosen: done to the box as its key would be.
    /// Answers whether the text changed.
    pub fn menu_row(&mut self, id: MenuItemId) -> bool {
        self.input.edit_command(id) == KeyEdit::Changed
    }

    /// Draw the box's text into `tree` as `look` says: an empty box says
    /// what it is for -- with its caret before that while it has the
    /// keyboard -- and draws no text of its own; a box with the keyboard
    /// draws its text with its caret and selection, and one without it the
    /// text cut to the width with `…`, at its end or, for a box
    /// [read from its end](Self::from_its_end), at its front. The text sits
    /// on the same line either way, so it does not move when the box takes
    /// the keyboard. The frame round it is the caller's
    /// (`guitk::field::draw`).
    pub fn draw(&self, tree: &mut RenderTree, look: &LineLook<'_>) {
        if self.is_empty() {
            // The placeholder under the caret too: a box that has just taken
            // the keyboard still says what to type into it. A box with none
            // draws no text at all, not an empty one.
            if !look.placeholder.is_empty() {
                push_line(
                    tree,
                    look,
                    look.placeholder,
                    look.placeholder_color,
                    self.font_size,
                );
            }
            if look.focused {
                textedit::push_caret(
                    tree,
                    look.x,
                    look.y,
                    look.line_height,
                    look.color,
                    look.caret_width,
                );
            }
            return;
        }
        if look.focused {
            textedit::draw(
                tree,
                &textedit::SingleLine {
                    text: self.input.text(),
                    cursor: self.input.cursor(),
                    selection_anchor: self.input.selection_anchor(),
                    focused: true,
                    x: look.x,
                    y: look.y,
                    width: look.width.max(0.0),
                    line_height: look.line_height,
                    font_size: self.font_size,
                    weight: FontWeightHint::Regular,
                    color: look.color,
                    selection_bg: look.selection_bg,
                    selection_fg: look.selection_fg,
                    caret_width: look.caret_width,
                },
            );
        } else if self.from_end {
            // Cut here, so clipped rather than cut again: a renderer that
            // measured a hair wider would otherwise put a second `…` over
            // the tail's last letter.
            let width = look.width.max(0.0);
            tree.push(RenderCommand::Text {
                x: look.x,
                y: look.y,
                text: guitk::text::elide_start(
                    self.input.text(),
                    width,
                    ELLIPSIS,
                    self.font_size,
                    FontWeightHint::Regular,
                ),
                font_size: self.font_size,
                color: look.color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Clip,
            });
        } else {
            push_line(tree, look, self.input.text(), look.color, self.font_size);
        }
    }
}

/// One line of `text` in `color` where `look` puts the box's text, cut to
/// its width with `…`.
fn push_line(tree: &mut RenderTree, look: &LineLook<'_>, text: &str, color: Color, font_size: f32) {
    tree.push(RenderCommand::Text {
        x: look.x,
        y: look.y,
        text: text.to_string(),
        font_size,
        color,
        font_weight: FontWeightHint::Regular,
        max_width: Some(look.width.max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use guitk::editmenu::EditCommand;
    use guitk::event::{Key, Modifiers};

    fn press(key: Key, text: &str) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.to_string(),
        }
    }

    fn ctrl(key: Key, letter: &str) -> KeyEvent {
        let mut modifiers = Modifiers::NONE;
        modifiers.ctrl = true;
        KeyEvent {
            modifiers,
            ..press(key, letter)
        }
    }

    fn typed(line: &mut LineBox, text: &str) {
        for ch in text.chars() {
            let key = line.key(&press(Key::Unknown(0), &ch.to_string()));
            assert!(key.handled && key.changed, "{ch:?} was not typed");
        }
    }

    const LOOK: LineLook<'static> = LineLook {
        x: 10.0,
        y: 8.0,
        width: 200.0,
        line_height: 16.0,
        focused: false,
        placeholder: "Search (Ctrl+F)",
        color: Color::rgb(200, 200, 200),
        placeholder_color: Color::rgb(150, 150, 150),
        selection_bg: Color::rgb(0, 0, 255),
        selection_fg: Color::rgb(0, 0, 0),
        caret_width: 2.0,
    };

    #[test]
    fn typing_edits_at_the_caret_and_says_the_text_changed() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "libre");
        assert_eq!(
            line.key(&press(Key::Home, "")),
            LineKey {
                handled: true,
                changed: false,
                moved: true,
                copied: false,
            }
        );
        assert_eq!(
            line.key(&press(Key::Home, "")),
            LineKey {
                handled: true,
                changed: false,
                moved: false,
                copied: false,
            },
            "Home at the start moved the caret"
        );
        assert_eq!(
            line.key(&press(Key::Delete, "")),
            LineKey {
                handled: true,
                changed: true,
                moved: false,
                copied: false,
            }
        );
        assert_eq!(line.text(), "ibre");
        typed(&mut line, "x");
        assert_eq!(
            line.text(),
            "xibre",
            "typing went to the end, not the caret"
        );
    }

    #[test]
    fn a_copy_or_a_cut_says_it_took_the_selection() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "tax");
        assert!(
            !line.key(&ctrl(Key::C, "c")).copied,
            "a copy with nothing selected took something"
        );
        line.select_all();
        assert_eq!(
            line.key(&ctrl(Key::C, "c")),
            LineKey {
                handled: true,
                changed: false,
                moved: false,
                copied: true,
            }
        );
        assert_eq!(
            line.key(&ctrl(Key::X, "x")),
            LineKey {
                handled: true,
                changed: true,
                moved: true,
                copied: true,
            }
        );
        assert_eq!(guitk::clipboard::text(), "tax");
    }

    #[test]
    fn a_key_the_box_does_not_answer_is_the_windows() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "tax");
        for key in [
            press(Key::F1, ""),
            press(Key::Up, ""),
            press(Key::Down, ""),
            press(Key::Enter, "\r"),
            press(Key::Escape, "\u{1b}"),
            press(Key::Tab, "\t"),
            ctrl(Key::F, "f"),
            ctrl(Key::S, "s"),
        ] {
            assert_eq!(
                line.key(&key),
                LineKey::default(),
                "{:?} was taken by the box",
                key.key
            );
        }
        assert_eq!(
            line.text(),
            "tax",
            "a key the box does not answer changed it"
        );
    }

    #[test]
    fn a_release_does_nothing() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "ab");
        let release = KeyEvent {
            pressed: false,
            ..press(Key::Backspace, "")
        };
        assert_eq!(line.key(&release), LineKey::default());
        assert_eq!(line.text(), "ab");
    }

    #[test]
    fn typing_stops_at_the_capacity() {
        let mut line = LineBox::new(3, 13.0);
        typed(&mut line, "abc");
        assert_eq!(
            line.key(&press(Key::Unknown(0), "d")),
            LineKey {
                handled: true,
                changed: false,
                moved: false,
                copied: false,
            }
        );
        assert_eq!(line.text(), "abc");
    }

    /// **An edit the check refuses is undone**, caret and all; one it keeps
    /// is made; and a key that changes no text is not asked about.
    #[test]
    fn an_edit_the_check_refuses_is_undone() {
        let digits = |t: &str| t.chars().all(|c| c.is_ascii_digit());
        let mut line = LineBox::holding("12", 8, 13.0);
        line.key(&press(Key::Home, ""));
        assert_eq!(
            line.key_checked(&press(Key::Unknown(0), "x"), digits),
            LineKey {
                handled: true,
                changed: false,
                moved: false,
                copied: false,
            },
            "a refused letter was not the box's, or said it changed"
        );
        assert_eq!(line.text(), "12", "the refused letter was typed");
        assert_eq!(
            line.input().cursor().byte(),
            0,
            "the caret moved with the refused edit"
        );
        assert!(
            line.key_checked(&press(Key::Unknown(0), "3"), digits)
                .changed,
            "a digit was refused"
        );
        assert_eq!(line.text(), "312");
        assert!(
            line.key_checked(&press(Key::End, ""), |_| false).moved,
            "a key that changes no text was refused"
        );
    }

    /// **A box types what it is given at its caret**, over the selection and
    /// up to its capacity; given nothing, it keeps its selection.
    #[test]
    fn a_box_types_what_it_is_given_at_its_caret() {
        let mut line = LineBox::holding("ac", 3, 13.0);
        line.key(&press(Key::Left, ""));
        assert_eq!(
            line.insert("b"),
            LineKey {
                handled: true,
                changed: true,
                moved: true,
                copied: false,
            }
        );
        assert_eq!(line.text(), "abc");
        assert_eq!(
            line.input().cursor().byte(),
            2,
            "the caret is not after what it typed"
        );
        assert!(!line.insert("d").changed, "the box typed past its capacity");
        assert_eq!(line.text(), "abc");
        line.select_all();
        assert_eq!(
            line.insert(""),
            LineKey {
                handled: true,
                ..LineKey::default()
            },
            "nothing typed changed the box"
        );
        assert!(
            line.input().has_selection(),
            "nothing typed took the selection"
        );
        assert!(line.insert("x").changed);
        assert_eq!(
            line.text(),
            "x",
            "what was typed did not go over the selection"
        );
    }

    /// **A box made holding text types after it, up to its capacity.**
    #[test]
    fn a_box_made_holding_text_types_after_it() {
        let mut line = LineBox::holding("8.8.8.8", 9, 13.0);
        assert_eq!(line.text(), "8.8.8.8");
        typed(&mut line, "xy");
        assert_eq!(line.text(), "8.8.8.8xy", "the caret was not after the text");
        assert!(
            !line.key(&press(Key::Unknown(0), "z")).changed,
            "typing went past the capacity"
        );
    }

    #[test]
    fn a_text_set_from_outside_puts_the_caret_at_its_end() {
        let mut line = LineBox::new(64, 13.0);
        assert!(line.set_text("ubuntu"));
        typed(&mut line, "x");
        assert_eq!(line.text(), "ubuntux");
    }

    #[test]
    fn setting_the_same_text_keeps_the_caret() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "abc");
        line.key(&press(Key::Home, ""));
        assert!(!line.set_text("abc"), "the same text counted as a change");
        typed(&mut line, "x");
        assert_eq!(line.text(), "xabc", "setting the same text moved the caret");
    }

    #[test]
    fn clear_says_whether_there_was_text() {
        let mut line = LineBox::new(64, 13.0);
        assert!(!line.clear());
        typed(&mut line, "a");
        assert!(line.clear());
        assert!(line.is_empty());
    }

    #[test]
    fn a_press_puts_the_caret_where_it_landed() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "ab");
        line.key(&ctrl(Key::A, "a"));
        // Left of the first letter: the caret before it, the selection gone.
        line.press(-5.0, 200.0, true);
        assert!(
            line.input().selection_anchor().is_none(),
            "the selection stayed"
        );
        typed(&mut line, "x");
        assert_eq!(line.text(), "xab");
        // Past the end, in a box without the keyboard: the caret at the end.
        line.press(500.0, 200.0, false);
        typed(&mut line, "y");
        assert_eq!(line.text(), "xaby");
    }

    /// Text too long for its box: with the keyboard it is drawn scrolled
    /// under the caret at its end, without it from its start, and a press is
    /// measured against what was drawn.
    #[test]
    fn a_press_is_measured_against_the_text_as_it_was_drawn() {
        let long = "a".repeat(60);
        let width = 60.0;
        let mut line = LineBox::new(64, 13.0);
        assert!(line.set_text(&long));
        // Without the keyboard: the start is on screen, so a press at the
        // left edge is before the first letter.
        line.press(0.0, width, false);
        assert_eq!(
            line.input().cursor().byte(),
            0,
            "measured as though scrolled"
        );
        // With it, the caret at the end: the end is on screen, so a press at
        // the right edge is near the end, not a box's width from the start.
        line.set_text("");
        line.set_text(&long);
        line.press(width - 1.0, width, true);
        assert!(
            line.input().cursor().byte() > 50,
            "measured from the start: caret at {}",
            line.input().cursor().byte()
        );
    }

    /// A path too long for [`LOOK`]'s 200 pixels, its tail `/end!`.
    fn long_path() -> String {
        format!("/home/{}end!", "deep/".repeat(20))
    }

    /// The text a box draws without the keyboard, held to the box's width.
    fn shown(line: &LineBox) -> String {
        let mut tree = RenderTree::new();
        line.draw(&mut tree, &LOOK);
        tree.commands
            .into_iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    text, max_width, ..
                } => {
                    assert_eq!(max_width, Some(LOOK.width), "not held to the box");
                    Some(text)
                }
                _ => None,
            })
            .expect("no text drawn")
    }

    /// **A box read from its end shows its tail without the keyboard**, cut
    /// at the front and no wider than the box -- where a box read from its
    /// start shows its head -- and with the keyboard it is drawn as any box
    /// is.
    #[test]
    fn a_box_read_from_its_end_is_cut_at_the_front_without_the_keyboard() {
        let path = long_path();
        let tail = LineBox::holding(&path, 256, 13.0).from_its_end();
        let drawn = shown(&tail);
        assert!(
            drawn.starts_with(ELLIPSIS) && drawn.ends_with("/end!"),
            "the tail is not what shows: {drawn}"
        );
        assert!(
            guitk::text::measure(&drawn, 13.0, FontWeightHint::Regular) <= LOOK.width,
            "wider than the box: {drawn}"
        );
        let head = LineBox::holding(&path, 256, 13.0);
        assert_eq!(shown(&head), path, "control: a box read from its start");
        let focused = LineLook {
            focused: true,
            ..LOOK
        };
        let (mut a, mut b) = (RenderTree::new(), RenderTree::new());
        tail.draw(&mut a, &focused);
        head.draw(&mut b, &focused);
        assert_eq!(a.commands, b.commands, "with the keyboard, drawn otherwise");
        let short = LineBox::holding("/tmp", 256, 13.0).from_its_end();
        assert_eq!(shown(&short), "/tmp", "text that fits is cut");
    }

    /// **A press in a box read from its end lands on the tail it showed**:
    /// a letter of the tail where it was drawn after the `…`, the `…`
    /// itself before the tail, and text that fits where it was drawn from
    /// its start. With the keyboard it is measured as any box is.
    #[test]
    fn a_press_in_a_box_read_from_its_end_lands_on_the_tail_it_showed() {
        let path = long_path();
        let weight = FontWeightHint::Regular;
        let drawn = shown(&LineBox::holding(&path, 256, 13.0).from_its_end());
        let tail = drawn.strip_prefix(ELLIPSIS).expect("cut at the front");
        let start = path.len() - tail.len();
        let lead = guitk::text::measure(ELLIPSIS, 13.0, weight);
        let mut line = LineBox::holding(&path, 256, 13.0).from_its_end();
        let caret = |line: &LineBox| line.input().cursor().byte();

        let fifth = lead + guitk::text::measure(&tail[..5], 13.0, weight) + 0.5;
        line.press(fifth, LOOK.width, false);
        assert_eq!(caret(&line), start + 5, "not after the tail's fifth letter");
        line.press(LOOK.width, LOOK.width, false);
        assert_eq!(caret(&line), path.len(), "a press past the tail");
        line.press(lead / 4.0, LOOK.width, false);
        assert_eq!(
            caret(&line),
            start,
            "a press on the … is not before the tail"
        );
        line.press(5.0, 0.0, false);
        assert_eq!(
            caret(&line),
            path.len(),
            "nothing drawn, and not at the end"
        );
        // With the keyboard it is scrolled under its caret as any box is:
        // the caret at the start shows the head.
        line.key(&press(Key::Home, ""));
        line.press(
            guitk::text::measure(&path[..5], 13.0, weight) + 0.5,
            LOOK.width,
            true,
        );
        assert_eq!(
            caret(&line),
            5,
            "with the keyboard, measured against the tail"
        );

        let mut short = LineBox::holding("abcd", 256, 13.0).from_its_end();
        short.press(
            guitk::text::measure("ab", 13.0, weight) + 0.5,
            LOOK.width,
            false,
        );
        assert_eq!(
            caret(&short),
            2,
            "text that fits is not measured from its start"
        );
    }

    /// **A box whose size is changed measures and draws at its new size**:
    /// a press where the fourth letter ends at the new size lands after it,
    /// not where that distance falls at the old one, and the text is drawn
    /// at the new size.
    #[test]
    fn a_resized_box_measures_and_draws_at_its_new_size() {
        let mut line = LineBox::new(64, 10.0);
        line.set_text("abcdefgh");
        line.set_font_size(20.0);
        let dx = guitk::text::measure("abcd", 20.0, FontWeightHint::Regular);
        line.press(dx, 500.0, false);
        assert_eq!(
            line.input().cursor().byte(),
            4,
            "the press was measured at the old size"
        );
        let mut tree = RenderTree::new();
        line.draw(&mut tree, &LOOK);
        let sizes: Vec<f32> = tree
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { font_size, .. } => Some(*font_size),
                _ => None,
            })
            .collect();
        assert!(
            !sizes.is_empty() && sizes.iter().all(|s| (s - 20.0).abs() < f32::EPSILON),
            "drawn at {sizes:?}, not the new size"
        );
    }

    /// The menu's rows that would do something, by label.
    fn lit(line: &LineBox) -> Vec<String> {
        line.edit_menu()
            .items()
            .iter()
            .filter_map(|row| match row {
                guitk::menu::MenuItem::Action {
                    label,
                    enabled: true,
                    ..
                } => Some(label.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_edits_the_text() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "abc");
        assert!(lit(&line).contains(&String::from("Select all")));
        assert!(
            !line.menu_row(EditCommand::SelectAll.id()),
            "selecting changed the text"
        );
        assert!(lit(&line).contains(&String::from("Delete")));
        assert!(
            line.menu_row(EditCommand::Delete.id()),
            "Delete did not say it changed the text"
        );
        assert!(line.is_empty());
    }

    #[test]
    fn a_box_with_the_keyboard_draws_its_caret_on_the_line() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "tax");
        let mut tree = RenderTree::new();
        line.draw(
            &mut tree,
            &LineLook {
                focused: true,
                ..LOOK
            },
        );
        assert!(tree.commands.iter().any(
            |c| matches!(c, RenderCommand::RichText { text, y, .. } if text == "tax" && (*y - LOOK.y).abs() < f32::EPSILON)
        ));
        assert!(
            tree.commands
                .iter()
                .any(|c| matches!(c, RenderCommand::Line { .. })),
            "no caret"
        );
    }

    #[test]
    fn an_empty_box_with_the_keyboard_says_what_it_is_for_under_its_caret() {
        let line = LineBox::new(64, 13.0);
        let mut tree = RenderTree::new();
        line.draw(
            &mut tree,
            &LineLook {
                focused: true,
                ..LOOK
            },
        );
        assert!(
            tree.commands.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, color, .. }
                    if text == LOOK.placeholder && *color == LOOK.placeholder_color
            )),
            "the placeholder went when the box took the keyboard"
        );
        assert!(
            tree.commands
                .iter()
                .any(|c| matches!(c, RenderCommand::Line { x1, .. } if (*x1 - LOOK.x).abs() < 0.5)),
            "no caret where the first letter will go"
        );
        assert!(
            !tree
                .commands
                .iter()
                .any(|c| matches!(c, RenderCommand::RichText { .. })),
            "an empty box drew a line of text of its own"
        );
        // With nothing to say, it draws no text at all.
        let mut tree = RenderTree::new();
        line.draw(
            &mut tree,
            &LineLook {
                placeholder: "",
                focused: true,
                ..LOOK
            },
        );
        assert!(
            !tree.commands.iter().any(|c| matches!(
                c,
                RenderCommand::Text { .. } | RenderCommand::RichText { .. }
            )),
            "an empty placeholder was drawn"
        );
    }

    #[test]
    fn caret_to_end_types_after_the_text_and_over_no_selection() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "dog");
        line.key(&press(Key::Home, ""));
        line.caret_to_end();
        typed(&mut line, "s");
        assert_eq!(line.text(), "dogs", "the caret stayed at the start");
        line.select_all();
        line.caret_to_end();
        assert!(
            line.input().selection_anchor().is_none(),
            "the selection stayed"
        );
        typed(&mut line, "!");
        assert_eq!(line.text(), "dogs!");
    }

    #[test]
    fn select_all_types_over_the_text() {
        let mut line = LineBox::new(64, 13.0);
        typed(&mut line, "DOLLAR");
        line.select_all();
        typed(&mut line, "E");
        assert_eq!(line.text(), "E");
    }

    #[test]
    fn a_box_without_the_keyboard_draws_its_text_or_its_placeholder() {
        let mut line = LineBox::new(64, 13.0);
        let drawn = |line: &LineBox| {
            let mut tree = RenderTree::new();
            line.draw(&mut tree, &LOOK);
            assert!(
                !tree
                    .commands
                    .iter()
                    .any(|c| matches!(c, RenderCommand::Line { .. })),
                "a caret in a box without the keyboard"
            );
            tree.commands
                .into_iter()
                .find_map(|c| match c {
                    RenderCommand::Text {
                        text,
                        color,
                        y,
                        max_width,
                        overflow,
                        ..
                    } => {
                        assert_eq!(max_width, Some(LOOK.width), "not held to the box");
                        assert!(
                            matches!(overflow, TextOverflow::Ellipsis),
                            "long text is not cut with an ellipsis"
                        );
                        Some((text, color, y))
                    }
                    _ => None,
                })
                .expect("no text drawn")
        };
        assert_eq!(
            drawn(&line),
            (LOOK.placeholder.to_string(), LOOK.placeholder_color, LOOK.y)
        );
        typed(&mut line, "tax");
        assert_eq!(drawn(&line), (String::from("tax"), LOOK.color, LOOK.y));
    }
}

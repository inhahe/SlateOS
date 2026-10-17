//! A program's search box: the search itself, edited at a caret.
//!
//! Twenty-four applications kept their search as a `String` with a
//! `TextInput` beside it holding the same text, reloaded from the string
//! before every key, press and menu row, and drawn with a caret at the end
//! whenever the two had come apart. Each also decided for itself which keys
//! were the box's, and two of them -- calendar and reminders -- took F1 from
//! the window while a search was being typed. Here the `TextInput` *is* the
//! search, so there is nothing to keep in step, and [`SearchBox::key`]
//! answers which keys are the box's: one it does not answer is the window's.
//!
//! **What stays with the program** is what differs between them: where the
//! box is, whether it has the keyboard, what Escape, Enter and Tab do there,
//! and what follows a change to the search -- a list back to its top, a
//! selection kept on what is still found.

use guitk::color::Color;
use guitk::event::KeyEvent;
use guitk::menu::{ContextMenu, MenuItemId};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::text::TextCursor;
use guitk::textedit;
use guitk::textinput::{KeyEdit, TextInput};

/// A search box's text, caret and selection, and the size it is drawn at.
#[derive(Clone, Debug)]
pub struct SearchBox {
    input: TextInput,
    font_size: f32,
}

/// What one key did to a [`SearchBox`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchKey {
    /// Whether the key was the box's: typing, the caret's keys, Backspace and
    /// Delete, Ctrl+A, C, X and V (`apply_key`). One it is not -- Enter,
    /// Escape, Tab, Up, Down, F1, any other command -- is the window's, for
    /// the program to act on as it would with the box closed.
    pub handled: bool,
    /// Whether the search changed.
    pub changed: bool,
    /// Whether the key put the selection on the clipboard (Ctrl+C, Ctrl+X):
    /// something done that neither the search nor its caret shows.
    pub copied: bool,
}

/// Where and how a [`SearchBox`] is drawn.
#[derive(Clone, Copy, Debug)]
pub struct SearchLook<'a> {
    /// The left edge of the box's text, inside its frame.
    pub x: f32,
    /// The top of the line the text sits on.
    pub y: f32,
    /// How wide the text may be: a longer search scrolls under the caret
    /// while the box has the keyboard, and is cut with `…` while it does not.
    pub width: f32,
    /// The line's height: the caret's and the selection's.
    pub line_height: f32,
    /// Whether the box has the keyboard, which is what draws the caret and
    /// the selection.
    pub focused: bool,
    /// What an empty box says, with the keyboard or without -- what it
    /// searches, and, while it has not the keyboard, the key that reaches it.
    pub placeholder: &'a str,
    /// The search's colour, and the caret's.
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

impl SearchBox {
    /// An empty box holding at most `capacity` characters, its text drawn at
    /// `font_size` -- the size its caret moves and its presses are measured
    /// at.
    #[must_use]
    pub fn new(capacity: usize, font_size: f32) -> Self {
        Self {
            input: crate::field_holding("", capacity),
            font_size,
        }
    }

    /// The search.
    #[must_use]
    pub fn text(&self) -> &str {
        self.input.text()
    }

    /// Whether nothing is being searched for.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.input.text().is_empty()
    }

    /// The field underneath, for a caller that reads the caret or the
    /// selection.
    #[must_use]
    pub fn input(&self) -> &TextInput {
        &self.input
    }

    /// Search for `text` instead, set from outside the box -- a link
    /// followed, a filter chosen elsewhere: the caret goes to its end.
    /// Answers whether the search changed; when it did not, the caret and
    /// the selection stay where they were.
    pub fn set_text(&mut self, text: &str) -> bool {
        if self.input.text() == text {
            return false;
        }
        self.input.set_text(text);
        true
    }

    /// Empty the box. Answers whether there was a search to empty.
    pub fn clear(&mut self) -> bool {
        self.set_text("")
    }

    /// Select the whole search, for the next key to type over -- what
    /// Ctrl+F does in a program whose box has the keyboard already.
    pub fn select_all(&mut self) {
        self.input.select_all();
    }

    /// The caret after the search, nothing selected: where a box that takes
    /// the keyboard by a key or a tab, rather than by a press, puts it.
    pub fn caret_to_end(&mut self) {
        self.input.set_selection_anchor(None);
        self.input
            .set_cursor(TextCursor::from(self.input.text().len()));
    }

    /// Apply `key` to the box -- on the program's clipboard, as its menu is
    /// (`apply_key_on_clipboard`) -- and say whether the key was the box's
    /// and whether the search changed.
    pub fn key(&mut self, key: &KeyEvent) -> SearchKey {
        let before = self.input.text().to_owned();
        let edit = crate::apply_key_on_clipboard(&mut self.input, key, self.font_size);
        SearchKey {
            handled: edit.handled,
            changed: self.input.text() != before,
            copied: edit.copied.is_some(),
        }
    }

    /// A press `dx` pixels right of the text's left edge, in a box whose text
    /// is `width` wide: the caret goes where it landed, against the text as
    /// it was drawn -- scrolled under the caret if the box had the keyboard
    /// (`focused`), from its start if it did not -- and any selection goes.
    pub fn press(&mut self, dx: f32, width: f32, focused: bool) {
        let drawn = if focused {
            self.input.cursor()
        } else {
            TextCursor::default()
        };
        let cursor = textedit::cursor_at_click(
            self.input.text(),
            drawn,
            width.max(0.0),
            self.font_size,
            FontWeightHint::Regular,
            dx,
        );
        self.input.set_selection_anchor(None);
        self.input.set_cursor(cursor);
    }

    /// The box's right-click menu: Cut, Copy, Paste, Delete and Select All,
    /// each offered when it would do something.
    #[must_use]
    pub fn edit_menu(&self) -> ContextMenu {
        self.input.edit_menu()
    }

    /// The menu's row `id`, chosen: done to the box as its key would be.
    /// Answers whether the search changed.
    pub fn menu_row(&mut self, id: MenuItemId) -> bool {
        self.input.edit_command(id) == KeyEdit::Changed
    }

    /// Draw the box's text into `tree` as `look` says: an empty box says
    /// what it is for -- with its caret before that while it has the
    /// keyboard -- and draws no text of its own; a box with the keyboard
    /// draws its search with its caret and selection, and one without it the
    /// search cut to the width with `…`. The text sits on the same line
    /// either way, so it does not move when the box takes the keyboard. The
    /// frame round it is the caller's (`guitk::field::draw`).
    pub fn draw(&self, tree: &mut RenderTree, look: &SearchLook<'_>) {
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
        } else {
            push_line(tree, look, self.input.text(), look.color, self.font_size);
        }
    }
}

/// One line of `text` in `color` where `look` puts the box's text, cut to
/// its width with `…`.
fn push_line(
    tree: &mut RenderTree,
    look: &SearchLook<'_>,
    text: &str,
    color: Color,
    font_size: f32,
) {
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

    fn typed(search: &mut SearchBox, text: &str) {
        for ch in text.chars() {
            let key = search.key(&press(Key::Unknown(0), &ch.to_string()));
            assert!(key.handled && key.changed, "{ch:?} was not typed");
        }
    }

    const LOOK: SearchLook<'static> = SearchLook {
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
    fn typing_edits_at_the_caret_and_says_the_search_changed() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "libre");
        assert_eq!(
            search.key(&press(Key::Home, "")),
            SearchKey {
                handled: true,
                changed: false,
                copied: false,
            }
        );
        assert_eq!(
            search.key(&press(Key::Delete, "")),
            SearchKey {
                handled: true,
                changed: true,
                copied: false,
            }
        );
        assert_eq!(search.text(), "ibre");
        typed(&mut search, "x");
        assert_eq!(
            search.text(),
            "xibre",
            "typing went to the end, not the caret"
        );
    }

    #[test]
    fn a_copy_or_a_cut_says_it_took_the_selection() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "tax");
        assert!(
            !search.key(&ctrl(Key::C, "c")).copied,
            "a copy with nothing selected took something"
        );
        search.select_all();
        assert_eq!(
            search.key(&ctrl(Key::C, "c")),
            SearchKey {
                handled: true,
                changed: false,
                copied: true,
            }
        );
        assert_eq!(
            search.key(&ctrl(Key::X, "x")),
            SearchKey {
                handled: true,
                changed: true,
                copied: true,
            }
        );
        assert_eq!(guitk::clipboard::text(), "tax");
    }

    #[test]
    fn a_key_the_box_does_not_answer_is_the_windows() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "tax");
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
                search.key(&key),
                SearchKey::default(),
                "{:?} was taken by the box",
                key.key
            );
        }
        assert_eq!(
            search.text(),
            "tax",
            "a key the box does not answer changed it"
        );
    }

    #[test]
    fn a_release_does_nothing() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "ab");
        let release = KeyEvent {
            pressed: false,
            ..press(Key::Backspace, "")
        };
        assert_eq!(search.key(&release), SearchKey::default());
        assert_eq!(search.text(), "ab");
    }

    #[test]
    fn typing_stops_at_the_capacity() {
        let mut search = SearchBox::new(3, 13.0);
        typed(&mut search, "abc");
        assert_eq!(
            search.key(&press(Key::Unknown(0), "d")),
            SearchKey {
                handled: true,
                changed: false,
                copied: false,
            }
        );
        assert_eq!(search.text(), "abc");
    }

    #[test]
    fn a_search_set_from_outside_puts_the_caret_at_its_end() {
        let mut search = SearchBox::new(64, 13.0);
        assert!(search.set_text("ubuntu"));
        typed(&mut search, "x");
        assert_eq!(search.text(), "ubuntux");
    }

    #[test]
    fn setting_the_same_search_keeps_the_caret() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "abc");
        search.key(&press(Key::Home, ""));
        assert!(
            !search.set_text("abc"),
            "the same search counted as a change"
        );
        typed(&mut search, "x");
        assert_eq!(
            search.text(),
            "xabc",
            "setting the same search moved the caret"
        );
    }

    #[test]
    fn clear_says_whether_there_was_a_search() {
        let mut search = SearchBox::new(64, 13.0);
        assert!(!search.clear());
        typed(&mut search, "a");
        assert!(search.clear());
        assert!(search.is_empty());
    }

    #[test]
    fn a_press_puts_the_caret_where_it_landed() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "ab");
        search.key(&ctrl(Key::A, "a"));
        // Left of the first letter: the caret before it, the selection gone.
        search.press(-5.0, 200.0, true);
        assert!(
            search.input().selection_anchor().is_none(),
            "the selection stayed"
        );
        typed(&mut search, "x");
        assert_eq!(search.text(), "xab");
        // Past the end, in a box without the keyboard: the caret at the end.
        search.press(500.0, 200.0, false);
        typed(&mut search, "y");
        assert_eq!(search.text(), "xaby");
    }

    /// A search too long for its box: with the keyboard it is drawn scrolled
    /// under the caret at its end, without it from its start, and a press is
    /// measured against what was drawn.
    #[test]
    fn a_press_is_measured_against_the_text_as_it_was_drawn() {
        let long = "a".repeat(60);
        let width = 60.0;
        let mut search = SearchBox::new(64, 13.0);
        assert!(search.set_text(&long));
        // Without the keyboard: the start is on screen, so a press at the
        // left edge is before the first letter.
        search.press(0.0, width, false);
        assert_eq!(
            search.input().cursor().byte(),
            0,
            "measured as though scrolled"
        );
        // With it, the caret at the end: the end is on screen, so a press at
        // the right edge is near the end, not a box's width from the start.
        search.set_text("");
        search.set_text(&long);
        search.press(width - 1.0, width, true);
        assert!(
            search.input().cursor().byte() > 50,
            "measured from the start: caret at {}",
            search.input().cursor().byte()
        );
    }

    /// The menu's rows that would do something, by label.
    fn lit(search: &SearchBox) -> Vec<String> {
        search
            .edit_menu()
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
    fn the_menu_edits_the_search() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "abc");
        assert!(lit(&search).contains(&String::from("Select all")));
        assert!(
            !search.menu_row(EditCommand::SelectAll.id()),
            "selecting changed the search"
        );
        assert!(lit(&search).contains(&String::from("Delete")));
        assert!(
            search.menu_row(EditCommand::Delete.id()),
            "Delete did not say it changed the search"
        );
        assert!(search.is_empty());
    }

    #[test]
    fn a_box_with_the_keyboard_draws_its_caret_on_the_line() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "tax");
        let mut tree = RenderTree::new();
        search.draw(
            &mut tree,
            &SearchLook {
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
        let search = SearchBox::new(64, 13.0);
        let mut tree = RenderTree::new();
        search.draw(
            &mut tree,
            &SearchLook {
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
        search.draw(
            &mut tree,
            &SearchLook {
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
    fn caret_to_end_types_after_the_search_and_over_no_selection() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "dog");
        search.key(&press(Key::Home, ""));
        search.caret_to_end();
        typed(&mut search, "s");
        assert_eq!(search.text(), "dogs", "the caret stayed at the start");
        search.select_all();
        search.caret_to_end();
        assert!(
            search.input().selection_anchor().is_none(),
            "the selection stayed"
        );
        typed(&mut search, "!");
        assert_eq!(search.text(), "dogs!");
    }

    #[test]
    fn select_all_types_over_the_search() {
        let mut search = SearchBox::new(64, 13.0);
        typed(&mut search, "DOLLAR");
        search.select_all();
        typed(&mut search, "E");
        assert_eq!(search.text(), "E");
    }

    #[test]
    fn a_box_without_the_keyboard_draws_the_search_or_its_placeholder() {
        let mut search = SearchBox::new(64, 13.0);
        let drawn = |search: &SearchBox| {
            let mut tree = RenderTree::new();
            search.draw(&mut tree, &LOOK);
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
                            "a long search is not cut with an ellipsis"
                        );
                        Some((text, color, y))
                    }
                    _ => None,
                })
                .expect("no text drawn")
        };
        assert_eq!(
            drawn(&search),
            (LOOK.placeholder.to_string(), LOOK.placeholder_color, LOOK.y)
        );
        typed(&mut search, "tax");
        assert_eq!(drawn(&search), (String::from("tax"), LOOK.color, LOOK.y));
    }
}

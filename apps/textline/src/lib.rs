//! The keys that edit a one-line text field.
//!
//! `guitk::textinput::TextInput` holds a field's state -- its text, caret,
//! selection and clipboard -- and deliberately leaves the keys to its caller:
//! which key moves the caret, which deletes, which selects all, which copies,
//! cuts and pastes, and that a control character in a paste has no place in a
//! field of one line. Seven applications here wrote that table themselves
//! (`regextester`, `flashcards`, `finance`, `qrcode`, `torrent`,
//! `soundrecorder`, `email`), in four slightly different versions: two
//! reported whether a key was theirs and five did not, and five deleted the
//! selection when a key typed nothing, which is a key that did nothing
//! destroying text. This is the one table, until the toolkit takes the keys
//! itself -- `requests/e-c-a-text-field-that-takes-its-own-keys.md` asks for
//! that, and when it lands the applications move onto it and this goes.
//!
//! The multi-line field is `apps/textarea`.
//!
//! **What it does not do is draw, or decide focus.** The caller owns the
//! rectangle, the colours and which field the keys go to, and asks this only
//! what a key does to the field that has them.

use guitk::event::{Key, KeyEvent};
use guitk::render::FontWeightHint;
use guitk::textinput::TextInput;

/// What one key did to a field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineEdit {
    /// Whether the key was one a field answers. A key it does not -- Tab,
    /// Enter, Escape, a function key, a Ctrl chord other than A, C, X and V --
    /// is the application's.
    pub handled: bool,
    /// What a copy or a cut took, for the application's clipboard.
    pub copied: Option<String>,
}

/// Apply `key` to `input`, a field of one line holding at most `capacity`
/// characters: the arrows move the caret (Shift extends the selection), Home
/// and End go to the ends, Backspace and Delete delete, Ctrl+A selects
/// everything, Ctrl+C copies, Ctrl+X cuts, Ctrl+V pastes `clipboard`, and
/// typed text replaces the selection.
///
/// `font_size` is the size the field's text is drawn at, which the caret
/// needs to move visually through text that runs both ways.
pub fn apply_key(
    input: &mut TextInput,
    key: &KeyEvent,
    capacity: usize,
    clipboard: &str,
    font_size: f32,
) -> LineEdit {
    let shift = key.modifiers.shift;
    let ctrl = key.modifiers.ctrl;
    let mut copied = None;
    match key.key {
        Key::Left => input.move_cursor_left(shift, font_size, FontWeightHint::Regular),
        Key::Right => input.move_cursor_right(shift, font_size, FontWeightHint::Regular),
        Key::Home => input.move_home(shift),
        Key::End => input.move_end(shift),
        Key::Backspace => input.backspace(),
        Key::Delete => input.delete(),
        Key::A if ctrl => input.select_all(),
        Key::C if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
            }
        }
        Key::X if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
                input.delete_selection();
            }
        }
        Key::V if ctrl => insert_limited(input, clipboard, capacity),
        _ => {
            if ctrl || !key.text.chars().any(|c| !c.is_control()) {
                return LineEdit::default();
            }
            insert_limited(input, &key.text, capacity);
        }
    }
    LineEdit {
        handled: true,
        copied,
    }
}

/// Type `typed` into `input` over its selection, stopping at `capacity`
/// characters -- counted in characters, not bytes, so a limit cannot cut a
/// character in half. A control character -- a newline in a paste -- is left
/// out, since a field is one line; and text with nothing else in it types
/// nothing, and so leaves the selection where it was.
pub fn insert_limited(input: &mut TextInput, typed: &str, capacity: usize) {
    if typed.chars().all(char::is_control) {
        return;
    }
    if input.has_selection() {
        input.delete_selection();
    }
    for ch in typed.chars() {
        if ch.is_control() {
            continue;
        }
        if input.text().chars().count() >= capacity {
            break;
        }
        input.insert_char(ch);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;

    fn key(k: Key, text: &str, ctrl: bool, shift: bool) -> KeyEvent {
        let mut modifiers = Modifiers::NONE;
        modifiers.ctrl = ctrl;
        modifiers.shift = shift;
        KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        }
    }

    fn typed(text: &str) -> KeyEvent {
        key(Key::Unknown(0), text, false, false)
    }

    fn field(text: &str) -> TextInput {
        let mut input = TextInput::new();
        input.set_text(text);
        input
    }

    #[test]
    fn typing_goes_in_at_the_caret_and_replaces_a_selection() {
        let mut input = field("ab");
        assert!(apply_key(&mut input, &key(Key::Left, "", false, false), 10, "", 13.0).handled);
        assert!(apply_key(&mut input, &typed("X"), 10, "", 13.0).handled);
        assert_eq!(input.text(), "aXb");
        apply_key(&mut input, &key(Key::A, "a", true, false), 10, "", 13.0);
        apply_key(&mut input, &typed("z"), 10, "", 13.0);
        assert_eq!(input.text(), "z");
        // A full field, all of it selected: what is typed replaces it, rather
        // than being refused for want of room the selection was about to free.
        let mut full = field("abcd");
        apply_key(&mut full, &key(Key::A, "a", true, false), 4, "", 13.0);
        apply_key(&mut full, &typed("z"), 4, "", 13.0);
        assert_eq!(full.text(), "z");
    }

    #[test]
    fn shift_with_an_arrow_selects() {
        let mut input = field("abc");
        apply_key(&mut input, &key(Key::Left, "", false, true), 10, "", 13.0);
        apply_key(&mut input, &key(Key::Left, "", false, true), 10, "", 13.0);
        assert_eq!(input.selected_text(), "bc");
        apply_key(&mut input, &key(Key::Home, "", false, false), 10, "", 13.0);
        assert!(!input.has_selection());
        apply_key(&mut input, &key(Key::End, "", false, true), 10, "", 13.0);
        assert_eq!(input.selected_text(), "abc");
    }

    #[test]
    fn backspace_and_delete_take_one_character_each_way() {
        let mut input = field("abc");
        apply_key(&mut input, &key(Key::Left, "", false, false), 10, "", 13.0);
        apply_key(
            &mut input,
            &key(Key::Backspace, "", false, false),
            10,
            "",
            13.0,
        );
        assert_eq!(input.text(), "ac");
        apply_key(
            &mut input,
            &key(Key::Delete, "", false, false),
            10,
            "",
            13.0,
        );
        assert_eq!(input.text(), "a");
    }

    #[test]
    fn copy_and_cut_hand_back_the_selection_and_paste_types_the_clipboard() {
        let mut input = field("hello");
        apply_key(&mut input, &key(Key::A, "a", true, false), 10, "", 13.0);
        let copied = apply_key(&mut input, &key(Key::C, "c", true, false), 10, "", 13.0);
        assert_eq!(copied.copied.as_deref(), Some("hello"));
        assert_eq!(input.text(), "hello", "a copy changed the field");
        let cut = apply_key(&mut input, &key(Key::X, "x", true, false), 10, "", 13.0);
        assert_eq!(cut.copied.as_deref(), Some("hello"));
        assert_eq!(input.text(), "");
        apply_key(
            &mut input,
            &key(Key::V, "v", true, false),
            10,
            "pasted",
            13.0,
        );
        assert_eq!(input.text(), "pasted");
        // With nothing selected, a copy takes nothing.
        let none = apply_key(&mut input, &key(Key::C, "c", true, false), 10, "", 13.0);
        assert_eq!(none.copied, None);
    }

    #[test]
    fn a_paste_leaves_line_breaks_out_and_stops_at_the_capacity_in_characters() {
        let mut input = field("");
        apply_key(
            &mut input,
            &key(Key::V, "v", true, false),
            4,
            "a\nbc\u{e9}\u{e9}",
            13.0,
        );
        assert_eq!(input.text(), "abc\u{e9}");
        let mut accents = field("");
        apply_key(
            &mut accents,
            &key(Key::V, "v", true, false),
            3,
            "\u{e9}\u{e9}\u{e9}\u{e9}",
            13.0,
        );
        assert_eq!(
            accents.text(),
            "\u{e9}\u{e9}\u{e9}",
            "three characters, not three bytes"
        );
    }

    #[test]
    fn a_key_that_types_nothing_is_not_the_fields_and_keeps_the_selection() {
        let mut input = field("keep");
        apply_key(&mut input, &key(Key::A, "a", true, false), 10, "", 13.0);
        for k in [
            key(Key::Tab, "\t", false, false),
            key(Key::Enter, "\r", false, false),
            key(Key::Escape, "", false, false),
            key(Key::F2, "", false, false),
            key(Key::S, "s", true, false),
        ] {
            let done = apply_key(&mut input, &k, 10, "", 13.0);
            assert!(!done.handled, "{:?} was taken by the field", k.key);
            assert_eq!(input.text(), "keep", "{:?} changed the field", k.key);
            assert!(input.has_selection(), "{:?} dropped the selection", k.key);
        }
        // A paste of nothing but a line break types nothing either.
        apply_key(&mut input, &key(Key::V, "v", true, false), 10, "\n", 13.0);
        assert_eq!(input.text(), "keep");
    }
}

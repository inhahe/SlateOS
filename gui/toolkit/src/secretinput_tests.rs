//! Tests for the password field: what is typed is kept whole and never shown
//! or copied, and the caret moves through characters, not bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::{DOT, SECRET_ROOM, SecretInput};
use crate::event::{Key, KeyEvent, Modifiers};
use crate::textinput::KeyEdit;

fn key(k: Key, ctrl: bool, text: &str) -> KeyEvent {
    KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers {
            ctrl,
            ..Modifiers::NONE
        },
        text: text.to_string(),
    }
}

fn press(field: &mut SecretInput, k: Key) -> KeyEdit {
    field.edit_key(&key(k, false, ""))
}

/// Type `text`, one key per character.
fn type_in(field: &mut SecretInput, text: &str) {
    for c in text.chars() {
        let edit = field.edit_key(&key(Key::Unknown(0), false, &c.to_string()));
        assert_eq!(edit, KeyEdit::Changed, "typing {c:?}");
    }
}

/// What was typed, taken out.
fn taken(field: &mut SecretInput) -> String {
    String::from_utf8(field.take().as_bytes().to_vec()).unwrap()
}

/// **What is typed is kept whole, drawn as dots, and taken out once**: in
/// any script, the field empty afterwards with the same room.
#[test]
fn what_is_typed_is_kept_whole_and_drawn_as_dots() {
    let mut field = SecretInput::new();
    assert!(field.is_empty());
    type_in(&mut field, "pässwörd ✓");
    assert_eq!((field.chars(), field.caret()), (10, 10));
    assert_eq!(field.dots(), DOT.to_string().repeat(10));
    assert_eq!(taken(&mut field), "pässwörd ✓");
    assert!(field.is_empty());
    assert_eq!((field.chars(), field.caret()), (0, 0));
    type_in(&mut field, "again");
    assert_eq!(taken(&mut field), "again");
    assert_eq!(format!("{field:?}"), "SecretInput { .. }");
}

/// **The caret moves through characters, and Backspace and Delete take out
/// whole ones** -- never part of a character, however many bytes it is.
#[test]
fn the_caret_moves_through_characters() {
    let mut field = SecretInput::new();
    type_in(&mut field, "aé✓z");
    assert_eq!(press(&mut field, Key::Left), KeyEdit::Handled);
    assert_eq!(field.caret(), 3);
    press(&mut field, Key::Left);
    assert_eq!(press(&mut field, Key::Backspace), KeyEdit::Changed);
    assert_eq!(field.caret(), 1);
    assert_eq!(press(&mut field, Key::Delete), KeyEdit::Changed);
    assert_eq!((field.chars(), field.caret()), (2, 1));
    type_in(&mut field, "ü");
    press(&mut field, Key::Home);
    assert_eq!(press(&mut field, Key::Backspace), KeyEdit::Handled);
    assert_eq!(press(&mut field, Key::Left), KeyEdit::Handled);
    assert_eq!(field.caret(), 0);
    press(&mut field, Key::End);
    assert_eq!(press(&mut field, Key::Delete), KeyEdit::Handled);
    assert_eq!(press(&mut field, Key::Right), KeyEdit::Handled);
    assert_eq!(field.caret(), 3);
    press(&mut field, Key::Home);
    press(&mut field, Key::Right);
    assert_eq!(field.caret(), 1);
    assert_eq!(taken(&mut field), "aüz");
}

/// **A password is not copied or cut out of its field**, and the
/// clipboard is left as it was; it is pasted in, as typed.
#[test]
fn a_password_is_not_copied_out_but_may_be_pasted_in() {
    crate::clipboard::set_text("before");
    let mut field = SecretInput::new();
    type_in(&mut field, "hunter2");
    field.edit_key(&key(Key::A, true, "a"));
    assert_eq!(field.edit_key(&key(Key::C, true, "c")), KeyEdit::Handled);
    assert_eq!(field.edit_key(&key(Key::X, true, "x")), KeyEdit::Handled);
    assert_eq!(crate::clipboard::text(), "before");
    assert_eq!(field.chars(), 7);
    // Pasting over the selection replaces it, leaving out a line break.
    crate::clipboard::set_text("new\npass");
    assert_eq!(field.edit_key(&key(Key::V, true, "v")), KeyEdit::Changed);
    assert_eq!(taken(&mut field), "newpass");
    // Nothing on the clipboard pastes nothing.
    crate::clipboard::set_text("");
    assert_eq!(field.edit_key(&key(Key::V, true, "v")), KeyEdit::Handled);
    assert!(field.is_empty());
}

/// **Selecting is all or nothing**: what is typed replaces it, Backspace
/// and Delete empty the field, an arrow goes to that end of it; nothing
/// typed selects nothing.
#[test]
fn selecting_is_all_or_nothing() {
    let mut field = SecretInput::new();
    field.edit_key(&key(Key::A, true, "a"));
    assert!(!field.is_all_selected());
    type_in(&mut field, "old");
    field.edit_key(&key(Key::A, true, "a"));
    assert!(field.is_all_selected());
    type_in(&mut field, "n");
    assert!(!field.is_all_selected());
    assert_eq!(taken(&mut field), "n");

    for (k, chars) in [(Key::Backspace, 0), (Key::Delete, 0)] {
        type_in(&mut field, "old");
        field.edit_key(&key(Key::A, true, "a"));
        assert_eq!(press(&mut field, k), KeyEdit::Changed);
        assert_eq!((field.chars(), field.is_all_selected()), (chars, false));
    }
    type_in(&mut field, "abc");
    for (k, caret) in [
        (Key::Left, 0),
        (Key::Right, 3),
        (Key::Home, 0),
        (Key::End, 3),
    ] {
        press(&mut field, Key::Home);
        press(&mut field, Key::Right);
        field.edit_key(&key(Key::A, true, "a"));
        assert_eq!(press(&mut field, k), KeyEdit::Handled);
        assert_eq!((field.caret(), field.is_all_selected()), (caret, false));
    }
}

/// **A keystroke that would not fit is refused whole**, and changes
/// nothing -- a selection included.
#[test]
fn a_keystroke_that_would_not_fit_is_refused_whole() {
    let mut field = SecretInput::with_room(4);
    type_in(&mut field, "ab");
    // Three bytes, with two left.
    assert_eq!(
        field.edit_key(&key(Key::Unknown(0), false, "✓")),
        KeyEdit::Handled
    );
    assert_eq!(field.paste_text("cde"), KeyEdit::Handled);
    assert_eq!(field.chars(), 2);
    assert_eq!(field.paste_text("cd"), KeyEdit::Changed);
    field.edit_key(&key(Key::A, true, "a"));
    assert_eq!(field.paste_text("vwxyz"), KeyEdit::Handled);
    assert!(field.is_all_selected());
    // In place of everything, it fits.
    assert_eq!(field.paste_text("wxyz"), KeyEdit::Changed);
    assert_eq!(taken(&mut field), "wxyz");
    assert_eq!(SecretInput::new().chars(), 0);
    assert_eq!(SECRET_ROOM, 1024);
}

/// **What is not editing is the owner's**: Enter, Escape, Tab and a
/// release are not handled, and a shortcut types nothing.
#[test]
fn what_is_not_editing_is_the_owners() {
    let mut field = SecretInput::new();
    for (k, text) in [
        (Key::Enter, "\r"),
        (Key::Escape, "\u{1b}"),
        (Key::Tab, "\t"),
    ] {
        assert_eq!(field.edit_key(&key(k, false, text)), KeyEdit::Unhandled);
    }
    let mut release = key(Key::A, false, "a");
    release.pressed = false;
    assert_eq!(field.edit_key(&release), KeyEdit::Unhandled);
    assert_eq!(field.edit_key(&key(Key::Q, true, "q")), KeyEdit::Unhandled);
    assert!(field.is_empty());
    // Control characters alone type nothing.
    assert_eq!(field.paste_text("\n\t"), KeyEdit::Handled);
    assert!(field.is_empty());
}

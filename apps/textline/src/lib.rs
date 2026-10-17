//! The keys that edit a one-line text field.
//!
//! `guitk::textinput::TextInput` holds a field's state -- its text, caret,
//! selection and capacity -- and what an edit does to it: `insert_text` puts
//! typing or a paste in over the selection, leaving out control characters
//! (a field is one line) and stopping at the capacity. It leaves the keys to
//! its caller: which key moves the caret, which deletes, which selects all,
//! which copies, cuts and pastes. Seven applications here wrote that table
//! themselves (`regextester`, `flashcards`, `finance`, `qrcode`, `torrent`,
//! `soundrecorder`, `email`), in four slightly different versions: two
//! reported whether a key was theirs and five did not, and five deleted the
//! selection when a key typed nothing, which is a key that did nothing
//! destroying text. This is the one table, until the toolkit takes the keys
//! itself -- `requests/e-c-a-text-field-that-takes-its-own-keys.md` asks for
//! that, and when it lands the applications move onto it and this goes.
//!
//! The multi-line field is `apps/textarea`.
//!
//! A program's search box is [`SearchBox`] ([`search`]): the search and
//! its caret in one field, its keys answered here, its drawing too.
//!
//! **What it does not do is draw, or decide focus.** The caller owns the
//! rectangle, the colours and which field the keys go to, and asks this only
//! what a key does to the field that has them.
//!
//! It also says, for any program, whether a keystroke is a command or typing
//! ([`is_ctrl_chord`], [`is_command`], [`types_into_field`]), which is not
//! obvious from the event: a command arrives carrying its letter as text, and
//! AltGr arrives looking like a command. And whether a key is plain enough
//! for a binding on the key itself ([`is_plain`]).

pub mod search;

pub use search::{SearchBox, SearchKey, SearchLook};

use guitk::event::{Key, KeyEvent, Modifiers};
use guitk::render::FontWeightHint;
use guitk::text::TextCursor;
use guitk::textedit;
use guitk::textinput::TextInput;

/// Whether a key held with `modifiers` is a Ctrl chord -- Ctrl+S, Ctrl+A --
/// rather than a character typed with AltGr: Ctrl without Alt, and without
/// the Windows key. The toolkit's rule (`Modifiers::is_ctrl_chord`), the one
/// its own fields keep to, by the name programs here ask it by.
///
/// AltGr arrives as Ctrl+Alt, and is how several layouts type a character:
/// Polish `ś` is AltGr+S, so a program that took every Ctrl+S as "save"
/// would save each time a Polish user typed it.
#[must_use]
pub const fn is_ctrl_chord(modifiers: Modifiers) -> bool {
    modifiers.is_ctrl_chord()
}

/// Whether a key held with `modifiers` is a command rather than typing: Ctrl
/// or Alt on its own, or anything with the Windows key. The toolkit's rule
/// (`Modifiers::is_command`), which `KeyEvent::typed` asks.
///
/// Asked because the text does not say: the compositor hands a command its
/// letter as text -- Ctrl+S arrives carrying `s` -- and Ctrl and Alt held
/// together are AltGr ([`is_ctrl_chord`]), which types.
#[must_use]
pub const fn is_command(modifiers: Modifiers) -> bool {
    modifiers.is_command()
}

/// Whether a key held with `modifiers` is plain: nothing held with it but
/// Shift, if anything. What a binding on the key itself answers -- the
/// stopwatch's R, a game's N -- before it acts. The toolkit's rule
/// (`Modifiers::is_plain`).
///
/// Stricter than `!`[`is_command`], which lets AltGr through because AltGr
/// types: a binding on the *key* cannot tell what AltGr made of it -- AltGr+R
/// is `®` on one layout and nothing on another -- so AltGr+R is not R.
#[must_use]
pub const fn is_plain(modifiers: Modifiers) -> bool {
    modifiers.is_plain()
}

/// Whether a key held with `modifiers` is Alt's or the Windows key's: Alt
/// without Ctrl, or anything with the Windows key.
///
/// Such a chord is never a text field's. A field's own chords are Ctrl's,
/// and AltGr -- Ctrl+Alt -- types, so neither is one of these. What this is
/// for is a field a program does not own and cannot ask: the toolkit's
/// multi-line field takes every key it is given and types the letter of a
/// chord it does not know (`requests/e-cf-a-toolkit-field-types-the-letter-of-a-shortcut-it-does-not-know.md`),
/// so the program keeps these out of it -- Alt+X typed an `x` into an email.
#[must_use]
pub const fn is_alt_or_windows_chord(modifiers: Modifiers) -> bool {
    (modifiers.alt && !modifiers.ctrl) || modifiers.super_key
}

/// Whether `key` types into a text field: a press that types text
/// (`KeyEvent::types_text`). That is the toolkit's rule for every field: no
/// command's letter ([`is_command`]), and no control character -- Enter, Tab
/// and Escape produce `\r`, `\t` and `\x1b` on most layouts, which are keys,
/// not text.
///
/// What a field of a program's own should ask before it appends
/// `key.typed()`.
#[must_use]
pub fn types_into_field(key: &KeyEvent) -> bool {
    key.pressed && key.types_text()
}

/// What one key did to a field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineEdit {
    /// Whether the key was one a field answers. A key it does not -- Tab,
    /// Enter, Escape, a function key, a Ctrl chord other than A, C, X and V,
    /// a key held with Alt or the Windows key -- is the application's.
    pub handled: bool,
    /// What a copy or a cut took, for the application's clipboard.
    pub copied: Option<String>,
}

/// Apply `key` to `input`, a field of one line: the arrows move the caret
/// (Shift extends the selection), Home and End go to the ends, Backspace and
/// Delete delete, Ctrl+A selects everything, Ctrl+C copies, Ctrl+X cuts,
/// Ctrl+V pastes `clipboard`, and typed text replaces the selection -- AltGr's
/// among it, which arrives as Ctrl+Alt ([`is_ctrl_chord`]).
///
/// What is typed or pasted stops at the field's own capacity
/// (`TextInput::set_capacity`, which [`field_holding`] sets), the limit its
/// right-click menu's Paste keeps to as well. Every field these keys edit has
/// one. A field built without one stops its program's tests at the
/// assertion below; a build without assertions gives it
/// [`UNBUILT_FIELD_CAPACITY`] instead, so a paste is still held to a limit.
///
/// `font_size` is the size the field's text is drawn at, which the caret
/// needs to move visually through text that runs both ways.
///
/// Only a key's press edits. Its release is not the field's, so a program
/// that hands the field both does not delete twice for one Backspace, or
/// paste twice for one Ctrl+V.
///
/// A key held with Alt or the Windows key is not the field's, whatever the
/// key ([`is_alt_or_windows_chord`]): Alt's chords are the window's and the
/// Windows key's the desktop's. The editing keys answered them all the same
/// -- Alt+Backspace deleted, Windows+Left moved the caret while the desktop
/// moved the window -- though [`LineEdit::handled`] said they were the
/// application's.
pub fn apply_key(
    input: &mut TextInput,
    key: &KeyEvent,
    clipboard: &str,
    font_size: f32,
) -> LineEdit {
    debug_assert!(
        input.capacity().is_some(),
        "a one-line field without a capacity: build it with textline::field_holding"
    );
    give_a_capacity(input);
    if !key.pressed {
        return LineEdit::default();
    }
    if is_alt_or_windows_chord(key.modifiers) {
        return LineEdit::default();
    }
    let shift = key.modifiers.shift;
    let chord = is_ctrl_chord(key.modifiers);
    let mut copied = None;
    match key.key {
        Key::Left => input.move_cursor_left(shift, font_size, FontWeightHint::Regular),
        Key::Right => input.move_cursor_right(shift, font_size, FontWeightHint::Regular),
        Key::Home => input.move_home(shift),
        Key::End => input.move_end(shift),
        Key::Backspace => input.backspace(),
        Key::Delete => input.delete(),
        Key::A if chord => input.select_all(),
        Key::C if chord => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
            }
        }
        Key::X if chord => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
                input.delete_selection();
            }
        }
        Key::V if chord => input.insert_text(clipboard),
        _ => {
            if !types_into_field(key) {
                return LineEdit::default();
            }
            input.insert_text(&key.text);
        }
    }
    LineEdit {
        handled: true,
        copied,
    }
}

/// [`apply_key`] on the program's clipboard (`guitk::clipboard`): Ctrl+V
/// pastes it, and what Ctrl+C or Ctrl+X takes is put on it as well as
/// handed back. It is the clipboard a field's right-click menu copies to and
/// pastes from (`TextInput::edit_command`), so the keys and the menu agree;
/// a window keeping a clipboard of its own for the keys has two that do not.
pub fn apply_key_on_clipboard(input: &mut TextInput, key: &KeyEvent, font_size: f32) -> LineEdit {
    // Read for the key that pastes alone: the clipboard may hold megabytes,
    // and every other key -- each letter typed -- would copy them for nothing.
    let clipboard = if pastes(key) {
        guitk::clipboard::text()
    } else {
        String::new()
    };
    let edit = apply_key(input, key, &clipboard, font_size);
    if let Some(copied) = &edit.copied {
        guitk::clipboard::set_text(copied);
    }
    edit
}

/// Whether `key` is [`apply_key`]'s paste: the press of Ctrl+V as a chord
/// ([`is_ctrl_chord`]).
#[must_use]
pub fn pastes(key: &KeyEvent) -> bool {
    key.pressed && key.key == Key::V && is_ctrl_chord(key.modifiers)
}

/// The capacity [`apply_key`] gives a field built without one, in a build
/// without debug assertions -- one with them stops at its assertion
/// instead. More than any one-line field here holds, and far less than a
/// clipboard can.
pub const UNBUILT_FIELD_CAPACITY: usize = 65_536;

/// Give `input` [`UNBUILT_FIELD_CAPACITY`] if it was built without a
/// capacity of its own.
fn give_a_capacity(input: &mut TextInput) {
    if input.capacity().is_none() {
        input.set_capacity(Some(UNBUILT_FIELD_CAPACITY));
    }
}

/// The largest capacity a `TextInput` keeps. It holds its limit in four
/// bytes, and reads 4,294,967,295 or more as no limit at all.
const MOST_A_FIELD_KEEPS: usize = 4_294_967_294;

/// A field holding `text`, which typing and pasting -- by key or from its
/// right-click menu -- fill to at most `capacity` characters, counted in
/// characters rather than bytes: how a program's one-line boxes start. The
/// text it starts with is not cut, so a longer one a file holds is shown
/// whole. A capacity past what a field can keep (4,294,967,294 characters)
/// is held at that, rather than leaving the field with no limit.
#[must_use]
pub fn field_holding(text: &str, capacity: usize) -> TextInput {
    let mut input = TextInput::new();
    input.set_text(text);
    input.set_capacity(Some(capacity.min(MOST_A_FIELD_KEEPS)));
    input
}

/// What a masked field shows for `text`: `mask` for each of its
/// characters, with the caret `cursor` and the selection's other end
/// `anchor` -- byte offsets into `text` -- moved onto the same characters of
/// the mask, to draw it with.
///
/// One mask for each *character*, as the caret steps through a masked field
/// ([`apply_masked_key`]): a password field that drew one for each byte
/// showed two for an `é`, where the caret and Backspace see one.
#[must_use]
pub fn masked(
    text: &str,
    cursor: usize,
    anchor: Option<usize>,
    mask: char,
) -> (String, usize, Option<usize>) {
    let shown: String = text.chars().map(|_| mask).collect();
    let onto_mask = |byte: usize| {
        let chars = text
            .get(..byte)
            .map_or_else(|| text.chars().count(), |head| head.chars().count());
        chars.saturating_mul(mask.len_utf8())
    };
    (shown, onto_mask(cursor), anchor.map(onto_mask))
}

/// The byte of `text` that byte `at` of its mask stands for -- the mask
/// being `mask` for each of its characters ([`masked`]): where a press
/// measured against the mask as it was drawn puts the caret in the text.
#[must_use]
pub fn unmasked(text: &str, at: usize, mask: char) -> usize {
    let nth = at.checked_div(mask.len_utf8()).unwrap_or(0);
    text.char_indices()
        .nth(nth)
        .map_or(text.len(), |(byte, _)| byte)
}

/// Apply `key` to `input`, a field drawn masked ([`masked`]) -- a password
/// -- as [`apply_key`] does, but for two things.
///
/// Left and Right step one character through the text as it is kept. The
/// mask on the screen runs one way whatever the text does, and a step
/// measured through the text's own runs of direction would carry the caret
/// over two masks at once in a password with a Hebrew letter in it.
///
/// And nothing is copied or cut: a hidden password put on a clipboard is
/// shown by the next paste anywhere. Ctrl+C and Ctrl+X are still the
/// field's, doing nothing, so that neither reaches a window's own Ctrl+C. A
/// paste is taken, as a password manager's paste is what keeps a password
/// from being typed.
pub fn apply_masked_key(input: &mut TextInput, key: &KeyEvent, clipboard: &str) -> LineEdit {
    let chord = is_ctrl_chord(key.modifiers);
    match key.key {
        Key::Left | Key::Right if !is_alt_or_windows_chord(key.modifiers) => {
            step_by_character(input, key.key == Key::Right, key.modifiers.shift);
            LineEdit {
                handled: true,
                copied: None,
            }
        }
        Key::C | Key::X if chord => LineEdit {
            handled: true,
            copied: None,
        },
        // No key left measures the text, so the size it is drawn at does
        // not matter to any of them.
        _ => apply_key(input, key, clipboard, 0.0),
    }
}

/// Move `input`'s caret one character `forward` or back through its text
/// as it is kept -- extending the selection with `shift` -- or, without
/// Shift, collapse a selection to the end it is moving towards.
fn step_by_character(input: &mut TextInput, forward: bool, shift: bool) {
    if !shift && input.has_selection() {
        let (start, end) = input.selection_range();
        input.set_cursor(TextCursor::from(if forward { end } else { start }));
        input.set_selection_anchor(None);
        return;
    }
    let at = input.cursor().byte();
    let text = input.text();
    let next = if forward {
        text.get(at..)
            .and_then(|tail| tail.chars().next())
            .map(|c| at.saturating_add(c.len_utf8()))
    } else {
        text.get(..at)
            .and_then(|head| head.chars().next_back())
            .map(|c| at.saturating_sub(c.len_utf8()))
    };
    let mut anchor = input.selection_anchor();
    textedit::begin_or_end_selection(shift, input.cursor(), &mut anchor);
    input.set_selection_anchor(anchor);
    if let Some(next) = next {
        input.set_cursor(TextCursor::from(next));
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

    fn held(k: Key, text: &str, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        }
    }

    /// Ctrl+Alt, as Windows and a remote client on it report AltGr.
    const ALTGR: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: true,
        super_key: false,
    };

    /// A field holding `text` and room for ten characters: more than any of
    /// these tests types, except where one says otherwise.
    fn field(text: &str) -> TextInput {
        field_holding(text, 10)
    }

    /// **`pastes` is the key `apply_key` pastes on, and no other**, whatever
    /// is held with it: the program's clipboard is read for that key alone,
    /// so a key that pasted without being it would paste nothing.
    #[test]
    fn pastes_is_the_key_apply_key_pastes_on() {
        let mut combinations = Vec::new();
        for bits in 0_u8..16 {
            combinations.push(Modifiers {
                shift: bits & 1 != 0,
                ctrl: bits & 2 != 0,
                alt: bits & 4 != 0,
                super_key: bits & 8 != 0,
            });
        }
        for modifiers in combinations {
            for (k, text) in [(Key::V, "v"), (Key::C, "c"), (Key::Unknown(0), "v")] {
                for pressed in [true, false] {
                    let mut stroke = held(k, text, modifiers);
                    stroke.pressed = pressed;
                    let mut input = field("");
                    apply_key(&mut input, &stroke, "PASTED", 13.0);
                    assert_eq!(
                        input.text() == "PASTED",
                        pastes(&stroke),
                        "{k:?} with {modifiers:?}, pressed: {pressed}"
                    );
                }
            }
        }
    }

    /// **A field holding a capacity is filled to it and no further**, and
    /// keeps whole a longer text it starts with -- a draft's or a rule's,
    /// read from a file.
    #[test]
    fn a_field_holding_a_capacity_fills_to_it() {
        let mut input = field_holding("ab", 4);
        input.insert_text("cdef");
        assert_eq!(input.text(), "abcd");
        let long = field_holding("a long start", 4);
        assert_eq!(
            long.text(),
            "a long start",
            "the text it starts with was cut"
        );
        assert_eq!(long.capacity(), Some(4));
        // A capacity past what a field can keep is held at the most it can,
        // not read as none.
        let enormous = field_holding("", usize::MAX);
        assert_eq!(enormous.capacity(), Some(MOST_A_FIELD_KEEPS));
    }

    /// **A field built without a capacity is given one at its first key**,
    /// in a build without the assertion that stops it in tests: a paste is
    /// held to [`UNBUILT_FIELD_CAPACITY`] rather than to nothing. A field
    /// that has a capacity keeps its own.
    #[test]
    fn a_field_built_without_a_capacity_is_given_one() {
        let mut uncapped = TextInput::new();
        give_a_capacity(&mut uncapped);
        assert_eq!(uncapped.capacity(), Some(UNBUILT_FIELD_CAPACITY));
        let mut capped = field_holding("", 4);
        give_a_capacity(&mut capped);
        assert_eq!(capped.capacity(), Some(4));
    }

    /// **Every field these keys edit has a capacity**: one built without it
    /// would take a paste of any length, and its program's tests say so at
    /// the first key.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "without a capacity")]
    fn a_field_without_a_capacity_is_caught() {
        let mut uncapped = TextInput::new();
        let _edit = apply_key(&mut uncapped, &typed("a"), "", 13.0);
    }

    /// **A key held with Alt or the Windows key is not the field's**: the
    /// editing keys answered it -- Alt+Backspace deleted, Windows+Left moved
    /// the caret -- though `LineEdit::handled` says such a key is the
    /// application's. AltGr's are still the field's, as Ctrl's are: AltGr
    /// is Ctrl+Alt, and a layout's AltGr+Backspace is a Backspace.
    #[test]
    fn a_key_held_with_alt_or_the_windows_key_edits_nothing() {
        let windows = Modifiers::super_key();
        let shift_alt = Modifiers {
            shift: true,
            ..Modifiers::alt()
        };
        for modifiers in [Modifiers::alt(), windows, shift_alt] {
            for k in [
                Key::Backspace,
                Key::Delete,
                Key::Left,
                Key::Right,
                Key::Home,
                Key::End,
            ] {
                let mut input = field("abc");
                input.move_cursor_left(false, 13.0, FontWeightHint::Regular);
                let at = input.cursor();
                let edit = apply_key(&mut input, &held(k, "", modifiers), "", 13.0);
                assert!(!edit.handled, "{modifiers:?} {k:?} was the field's");
                assert_eq!(input.text(), "abc", "{modifiers:?} {k:?} edited");
                assert_eq!(input.cursor(), at, "{modifiers:?} {k:?} moved the caret");
            }
        }
        // AltGr's and Ctrl's editing keys are the field's.
        let mut input = field("abc");
        assert!(apply_key(&mut input, &held(Key::Backspace, "", ALTGR), "", 13.0).handled);
        assert_eq!(input.text(), "ab");
        assert!(apply_key(&mut input, &key(Key::Left, "", true, false), "", 13.0).handled);
    }

    #[test]
    fn typing_goes_in_at_the_caret_and_replaces_a_selection() {
        let mut input = field("ab");
        assert!(apply_key(&mut input, &key(Key::Left, "", false, false), "", 13.0).handled);
        assert!(apply_key(&mut input, &typed("X"), "", 13.0).handled);
        assert_eq!(input.text(), "aXb");
        apply_key(&mut input, &key(Key::A, "a", true, false), "", 13.0);
        apply_key(&mut input, &typed("z"), "", 13.0);
        assert_eq!(input.text(), "z");
        // A full field, all of it selected: what is typed replaces it, rather
        // than being refused for want of room the selection was about to free.
        let mut full = field_holding("abcd", 4);
        apply_key(&mut full, &key(Key::A, "a", true, false), "", 13.0);
        apply_key(&mut full, &typed("z"), "", 13.0);
        assert_eq!(full.text(), "z");
        // With nothing selected, a full field takes nothing more.
        let mut at_capacity = field_holding("abcd", 4);
        apply_key(&mut at_capacity, &typed("e"), "", 13.0);
        assert_eq!(at_capacity.text(), "abcd", "typing went past the capacity");
    }

    #[test]
    fn shift_with_an_arrow_selects() {
        let mut input = field("abc");
        apply_key(&mut input, &key(Key::Left, "", false, true), "", 13.0);
        apply_key(&mut input, &key(Key::Left, "", false, true), "", 13.0);
        assert_eq!(input.selected_text(), "bc");
        apply_key(&mut input, &key(Key::Home, "", false, false), "", 13.0);
        assert!(!input.has_selection());
        apply_key(&mut input, &key(Key::End, "", false, true), "", 13.0);
        assert_eq!(input.selected_text(), "abc");
    }

    #[test]
    fn backspace_and_delete_take_one_character_each_way() {
        let mut input = field("abc");
        apply_key(&mut input, &key(Key::Left, "", false, false), "", 13.0);
        apply_key(&mut input, &key(Key::Backspace, "", false, false), "", 13.0);
        assert_eq!(input.text(), "ac");
        apply_key(&mut input, &key(Key::Delete, "", false, false), "", 13.0);
        assert_eq!(input.text(), "a");
    }

    #[test]
    fn copy_and_cut_hand_back_the_selection_and_paste_types_the_clipboard() {
        let mut input = field("hello");
        apply_key(&mut input, &key(Key::A, "a", true, false), "", 13.0);
        let copied = apply_key(&mut input, &key(Key::C, "c", true, false), "", 13.0);
        assert_eq!(copied.copied.as_deref(), Some("hello"));
        assert_eq!(input.text(), "hello", "a copy changed the field");
        let cut = apply_key(&mut input, &key(Key::X, "x", true, false), "", 13.0);
        assert_eq!(cut.copied.as_deref(), Some("hello"));
        assert_eq!(input.text(), "");
        apply_key(&mut input, &key(Key::V, "v", true, false), "pasted", 13.0);
        assert_eq!(input.text(), "pasted");
        // With nothing selected, a copy takes nothing.
        let none = apply_key(&mut input, &key(Key::C, "c", true, false), "", 13.0);
        assert_eq!(none.copied, None);
    }

    /// **On the program's clipboard, the keys and a field's right-click menu
    /// agree**: Ctrl+C puts the selection there, the menu's Paste pastes it
    /// into another field, and what the menu copies Ctrl+V pastes.
    #[test]
    fn the_keys_and_the_menu_share_the_programs_clipboard() {
        let mut from = field("hello");
        let mut to = field("");
        apply_key_on_clipboard(&mut from, &key(Key::A, "a", true, false), 13.0);
        let copied = apply_key_on_clipboard(&mut from, &key(Key::C, "c", true, false), 13.0);
        assert_eq!(
            copied.copied.as_deref(),
            Some("hello"),
            "the copy is not handed back"
        );
        assert_eq!(guitk::clipboard::text(), "hello");
        let _pasted = to.edit_command(guitk::editmenu::EditCommand::Paste.id());
        assert_eq!(
            to.text(),
            "hello",
            "the menu did not paste what the key copied"
        );

        let mut other = field("world");
        other.select_all();
        let _copied = other.edit_command(guitk::editmenu::EditCommand::Copy.id());
        let mut last = field_holding("", 3);
        apply_key_on_clipboard(&mut last, &key(Key::V, "v", true, false), 13.0);
        assert_eq!(
            last.text(),
            "wor",
            "the key did not paste what the menu copied, to the capacity"
        );
    }

    #[test]
    fn a_paste_leaves_line_breaks_out_and_stops_at_the_capacity_in_characters() {
        let mut input = field_holding("", 4);
        apply_key(
            &mut input,
            &key(Key::V, "v", true, false),
            "a\nbc\u{e9}\u{e9}",
            13.0,
        );
        assert_eq!(input.text(), "abc\u{e9}");
        let mut accents = field_holding("", 3);
        apply_key(
            &mut accents,
            &key(Key::V, "v", true, false),
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
        apply_key(&mut input, &key(Key::A, "a", true, false), "", 13.0);
        for k in [
            key(Key::Tab, "\t", false, false),
            key(Key::Enter, "\r", false, false),
            key(Key::Escape, "", false, false),
            key(Key::F2, "", false, false),
            key(Key::S, "s", true, false),
        ] {
            let done = apply_key(&mut input, &k, "", 13.0);
            assert!(!done.handled, "{:?} was taken by the field", k.key);
            assert_eq!(input.text(), "keep", "{:?} changed the field", k.key);
            assert!(input.has_selection(), "{:?} dropped the selection", k.key);
        }
        // A paste of nothing but a line break types nothing either.
        apply_key(&mut input, &key(Key::V, "v", true, false), "\n", 13.0);
        assert_eq!(input.text(), "keep");
    }

    /// **A key's release edits nothing**: only its press does, so a program
    /// that hands the field both does not type every letter twice, delete
    /// twice for one Backspace, or paste twice for one Ctrl+V.
    #[test]
    fn a_release_edits_nothing() {
        for k in [
            typed("c"),
            key(Key::Backspace, "", false, false),
            key(Key::Delete, "", false, false),
            key(Key::V, "v", true, false),
            key(Key::A, "a", true, false),
            key(Key::Left, "", false, true),
        ] {
            let mut input = field("ab");
            input.set_cursor(TextCursor::from(1));
            let mut release = k.clone();
            release.pressed = false;
            let done = apply_key(&mut input, &release, "PASTED", 13.0);
            assert!(!done.handled, "the release of {:?} was the field's", k.key);
            assert_eq!(input.text(), "ab", "the release of {:?} edited", k.key);
            assert!(
                !input.has_selection(),
                "the release of {:?} selected",
                k.key
            );
            assert_eq!(
                input.cursor().byte(),
                1,
                "the release of {:?} moved the caret",
                k.key
            );
        }
    }

    /// AltGr arrives as Ctrl+Alt, and on the four letters the field takes
    /// as chords it types: Polish `ą` and `ć` are AltGr+A and AltGr+C, `ź`
    /// is AltGr+X, and Hungarian `@` is AltGr+V.
    #[test]
    fn altgr_types_where_a_ctrl_chord_would_select_copy_cut_or_paste() {
        let mut input = field("ab");
        for (k, text) in [(Key::A, "\u{105}"), (Key::C, "\u{107}"), (Key::V, "@")] {
            let done = apply_key(&mut input, &held(k, text, ALTGR), "clip", 13.0);
            assert!(done.handled, "AltGr+{k:?} was not the field's");
            assert_eq!(done.copied, None, "AltGr+{k:?} copied");
            assert!(!input.has_selection(), "AltGr+{k:?} selected");
        }
        assert_eq!(input.text(), "ab\u{105}\u{107}@");
        // Over a selection it types, as any letter does, and hands nothing
        // to the clipboard.
        apply_key(&mut input, &key(Key::A, "a", true, false), "", 13.0);
        let done = apply_key(&mut input, &held(Key::X, "\u{17a}", ALTGR), "", 13.0);
        assert_eq!((done.copied, input.text()), (None, "\u{17a}"));
    }

    /// A command carries its letter as text, and types none of it: Ctrl+S
    /// is `s`, Alt+F is `f`, and the Windows key's chords are the
    /// desktop's -- with Ctrl and Alt too.
    #[test]
    fn a_command_types_nothing_though_it_carries_its_letter() {
        let mut input = field("keep");
        let windows = Modifiers::super_key();
        let windows_altgr = Modifiers {
            super_key: true,
            ..ALTGR
        };
        let alt_shift = Modifiers {
            shift: true,
            ..Modifiers::alt()
        };
        for k in [
            held(Key::S, "s", Modifiers::ctrl()),
            held(Key::F, "f", Modifiers::alt()),
            held(Key::F, "F", alt_shift),
            held(Key::E, "e", windows),
            held(Key::E, "\u{20ac}", windows_altgr),
        ] {
            let done = apply_key(&mut input, &k, "", 13.0);
            assert!(!done.handled, "{:?} {:?} was taken", k.modifiers, k.key);
            assert_eq!(input.text(), "keep", "{:?} {:?} typed", k.modifiers, k.key);
        }
        // Alt on its own is not a chord of the field's either: Alt+A leaves
        // the selection alone.
        let done = apply_key(&mut input, &held(Key::A, "a", Modifiers::alt()), "", 13.0);
        assert!(!done.handled);
        assert!(!input.has_selection());
        // Nor is Ctrl+A held with the Windows key: that is the desktop's.
        let windows_ctrl = Modifiers {
            super_key: true,
            ..Modifiers::ctrl()
        };
        let done = apply_key(&mut input, &held(Key::A, "a", windows_ctrl), "", 13.0);
        assert!(!done.handled);
        assert!(!input.has_selection());
    }

    /// The three questions a program asks, across every way Ctrl, Alt and
    /// the Windows key can be held.
    #[test]
    fn a_chord_a_command_and_typing_by_modifiers() {
        let m = |ctrl, alt, super_key| Modifiers {
            shift: false,
            ctrl,
            alt,
            super_key,
        };
        // (ctrl, alt, windows) -> (a Ctrl chord, a command)
        let table = [
            ((false, false, false), (false, false)),
            ((true, false, false), (true, true)),
            ((false, true, false), (false, true)),
            ((true, true, false), (false, false)),
            ((false, false, true), (false, true)),
            ((true, false, true), (false, true)),
            ((false, true, true), (false, true)),
            ((true, true, true), (false, true)),
        ];
        for ((ctrl, alt, windows), (chord, command)) in table {
            let held = m(ctrl, alt, windows);
            assert_eq!(is_ctrl_chord(held), chord, "chord: {held:?}");
            assert_eq!(is_command(held), command, "command: {held:?}");
            // Plain is nothing held at all: AltGr, not a command, is not
            // plain either.
            assert_eq!(is_plain(held), !(ctrl || alt || windows), "plain: {held:?}");
            // Alt's or the Windows key's: Alt without Ctrl, or the Windows
            // key with anything.
            assert_eq!(
                is_alt_or_windows_chord(held),
                (alt && !ctrl) || windows,
                "Alt's or the Windows key's: {held:?}"
            );
            let press = KeyEvent {
                key: Key::A,
                pressed: true,
                modifiers: held,
                text: "a".to_owned(),
            };
            assert_eq!(types_into_field(&press), !command, "types: {held:?}");
        }
        // Shift changes none of it.
        assert!(!is_command(Modifiers::shift()));
        assert!(is_plain(Modifiers::shift()));
        assert!(is_ctrl_chord(Modifiers {
            shift: true,
            ..Modifiers::ctrl()
        }));
        // A release types nothing, and neither does a key whose only text is
        // a control character.
        let release = KeyEvent {
            key: Key::A,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: "a".to_owned(),
        };
        assert!(!types_into_field(&release));
        assert!(!types_into_field(&key(Key::Enter, "\r", false, false)));
        assert!(!types_into_field(&key(Key::A, "", false, false)));
        assert!(types_into_field(&typed("\u{e9}")));
    }

    // -- A masked field ------------------------------------------------------

    /// **A masked field shows one mask for each character, its caret and
    /// selection on the same characters**, and a press on the mask finds
    /// the character it stands for.
    #[test]
    fn a_masked_field_masks_each_character_and_keeps_its_caret_on_it() {
        // `é` is two bytes, `€` three, and each one character.
        let text = "a\u{e9}\u{20ac}b";
        let (shown, cursor, anchor) = masked(text, 3, Some(1), '\u{2022}');
        assert_eq!(shown, "\u{2022}".repeat(4), "one mask for each character");
        assert_eq!(cursor, 2 * 3, "the caret after the second character");
        assert_eq!(anchor, Some(3), "the anchor after the first");
        assert_eq!(
            masked(text, text.len(), None, '*'),
            ("****".to_owned(), 4, None)
        );
        assert_eq!(masked("", 0, None, '*'), (String::new(), 0, None));

        assert_eq!(unmasked(text, 0, '\u{2022}'), 0);
        assert_eq!(
            unmasked(text, 3, '\u{2022}'),
            1,
            "the second mask is the `\u{e9}`"
        );
        assert_eq!(
            unmasked(text, 6, '\u{2022}'),
            3,
            "the third is the `\u{20ac}`"
        );
        assert_eq!(
            unmasked(text, 12, '\u{2022}'),
            text.len(),
            "past the end is the end"
        );
    }

    /// **A masked field's arrows step one character through the text as it
    /// is kept**, Shift extending the selection and an unshifted arrow
    /// collapsing it; **and nothing is copied or cut from it**, though the
    /// keys are its own. A paste is taken.
    #[test]
    fn a_masked_field_steps_by_character_and_gives_nothing_to_the_clipboard() {
        let mut input = field("a\u{5d0}b");
        input.set_cursor(TextCursor::from(0));
        let right = key(Key::Right, "", false, false);
        let left = key(Key::Left, "", false, false);
        assert!(apply_masked_key(&mut input, &right, "").handled);
        assert_eq!(input.cursor().byte(), 1, "past the `a`");
        apply_masked_key(&mut input, &right, "");
        assert_eq!(
            input.cursor().byte(),
            3,
            "past the Hebrew letter, one character"
        );
        apply_masked_key(&mut input, &left, "");
        assert_eq!(input.cursor().byte(), 1, "back over it");
        // Two characters, so collapsing the selection and stepping back
        // from its end land in different places.
        apply_masked_key(&mut input, &key(Key::Right, "", false, true), "");
        apply_masked_key(&mut input, &key(Key::Right, "", false, true), "");
        assert_eq!(input.selection_range(), (1, 4), "Shift+Right selects");
        apply_masked_key(&mut input, &left, "");
        assert_eq!(
            (input.cursor().byte(), input.has_selection()),
            (1, false),
            "an unshifted arrow collapses the selection to its start"
        );

        input.select_all();
        for k in [Key::C, Key::X] {
            let edit = apply_masked_key(&mut input, &key(k, "", true, false), "");
            assert_eq!(
                edit,
                LineEdit {
                    handled: true,
                    copied: None
                },
                "{k:?}"
            );
        }
        assert_eq!(input.text(), "a\u{5d0}b", "Ctrl+X cut a hidden password");
        let paste = key(Key::V, "", true, false);
        assert!(apply_masked_key(&mut input, &paste, "secret").handled);
        assert_eq!(input.text(), "secret", "a paste is taken");
        let alt_left = held(Key::Left, "", Modifiers::alt());
        assert!(
            !apply_masked_key(&mut input, &alt_left, "").handled,
            "Alt+Left"
        );
        assert!(apply_masked_key(&mut input, &typed("!"), "").handled);
        assert_eq!(input.text(), "secret!", "typing");
    }
}

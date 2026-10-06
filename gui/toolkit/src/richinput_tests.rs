#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use crate::event::Modifiers;

const SIZE: f32 = 13.0;

fn m(width: f32) -> Metrics {
    Metrics {
        width,
        size: SIZE,
        wrap: true,
    }
}

fn wide() -> Metrics {
    m(10_000.0)
}

fn key(k: Key, ctrl: bool, shift: bool) -> KeyEvent {
    KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers {
            shift,
            ctrl,
            ..Modifiers::NONE
        },
        text: String::new(),
    }
}

fn typed(input: &mut RichInput, text: &str) {
    for ch in text.chars() {
        input.handle_key(
            &KeyEvent {
                key: Key::Unknown(0),
                pressed: true,
                modifiers: Modifiers::NONE,
                text: ch.to_string(),
            },
            &wide(),
        );
    }
}

/// The runs as (start, end, bold) triples.
fn bold_runs(input: &RichInput) -> Vec<(usize, usize, bool)> {
    input
        .doc()
        .spans()
        .map(|(s, e, f)| (s, e, f.bold))
        .collect()
}

/// **Typed text carries on in the format before the caret.**
#[test]
fn typing_carries_on_in_the_format_before() {
    let mut input = RichInput::new();
    typed(&mut input, "ab");
    input.select_all();
    input.toggle(Toggle::Bold);
    input.doc_end(false);
    typed(&mut input, "c");
    assert_eq!(bold_runs(&input), [(0, 3, true)]);
}

/// **A switch over a mixed selection turns it on; over one all of a kind,
/// off** -- and the selection stays.
#[test]
fn a_switch_turns_a_mixed_selection_on_and_a_whole_one_off() {
    let mut input = RichInput::with_doc(RichDoc::plain("abcd", Format::default()));
    input.set_cursor(1);
    input.move_right(true);
    input.toggle(Toggle::Bold);
    assert_eq!(
        bold_runs(&input),
        [(0, 1, false), (1, 2, true), (2, 4, false)]
    );
    input.select_all();
    input.toggle(Toggle::Bold);
    assert_eq!(bold_runs(&input), [(0, 4, true)], "mixed: on");
    input.toggle(Toggle::Bold);
    assert_eq!(bold_runs(&input), [(0, 4, false)], "all bold: off");
    assert_eq!(input.selection_range(), Some((0, 4)), "still selected");
}

/// **With nothing selected, a switch is for what is typed next** -- until
/// the caret moves.
#[test]
fn a_switch_with_nothing_selected_is_for_what_is_typed_next() {
    let mut input = RichInput::with_doc(RichDoc::plain("ab", Format::default()));
    input.toggle(Toggle::Bold);
    assert!(input.shown_has(Toggle::Bold));
    assert_eq!(input.doc().text(), "ab", "nothing changed yet");
    typed(&mut input, "CD");
    assert_eq!(bold_runs(&input), [(0, 2, false), (2, 4, true)]);
    input.toggle(Toggle::Bold);
    input.move_left(false);
    assert!(
        input.shown_has(Toggle::Bold),
        "moved: the format before is bold again"
    );
}

/// **Backspace and Delete take a character -- or the selection -- across
/// runs, and say whether anything went.**
#[test]
fn backspace_and_delete_take_a_character_or_the_selection() {
    let mut input = RichInput::with_doc(RichDoc::plain("ab", Format::default()));
    input.select_all();
    input.toggle(Toggle::Bold);
    input.doc_end(false);
    typed(&mut input, "c");
    input.toggle(Toggle::Italic);
    assert!(input.backspace());
    assert_eq!(input.text(), "ab");
    input.doc_start(false);
    assert!(!input.backspace(), "nothing before");
    assert!(input.delete());
    assert_eq!(input.text(), "b");
    input.select_all();
    assert!(input.delete());
    assert_eq!(input.text(), "");
    assert!(!input.delete(), "nothing after");
}

/// **Undo takes back typing a word at a time, and formatting as it was** --
/// with the selection it had; redo does it again.
#[test]
fn undo_takes_back_words_and_formatting() {
    let mut input = RichInput::new();
    typed(&mut input, "one two");
    assert!(input.undo());
    assert_eq!(input.text(), "one ", "the word typed last");
    assert!(input.undo());
    assert_eq!(input.text(), "");
    assert!(input.redo());
    assert!(input.redo());
    assert_eq!(input.text(), "one two");
    input.select_all();
    input.toggle(Toggle::Underline);
    assert!(input.doc().format_at(0).underline);
    assert!(input.undo());
    assert!(!input.doc().format_at(0).underline, "formatting undone");
    assert_eq!(input.selection_range(), Some((0, 7)), "the selection back");
    assert!(input.redo());
    assert!(input.doc().format_at(3).underline);
}

/// **A copy pasted here brings its formatting; other text pastes plain, in
/// the format at the caret.**
#[test]
fn a_rich_copy_pastes_rich_and_other_text_plain() {
    let mut input = RichInput::with_doc(RichDoc::plain("ab", Format::default()));
    input.select_all();
    input.toggle(Toggle::Bold);
    input.copy();
    input.doc_end(false);
    let mut other = RichInput::new();
    other.paste();
    assert_eq!(bold_runs(&other), [(0, 2, true)], "formatted");
    crate::clipboard::set_text("xy");
    other.paste();
    assert_eq!(other.text(), "abxy");
    assert!(other.doc().format_at(2).bold, "in the format at the caret");
    other.set_cursor(0);
    crate::clipboard::set_text("z");
    other.paste();
    assert!(other.doc().format_at(0).bold, "at the start, the first's");
}

/// **The keys: Ctrl+B switches, Enter breaks the line, Shift and an arrow
/// select, Ctrl+Z undoes** -- each answered as a change or not.
#[test]
fn the_keys_edit_as_a_rich_field_does() {
    let mut input = RichInput::new();
    typed(&mut input, "ab");
    assert_eq!(
        input.handle_key(&key(Key::Left, false, true), &wide()),
        KeyEdit::Handled
    );
    assert_eq!(input.selection_range(), Some((1, 2)));
    assert_eq!(
        input.handle_key(&key(Key::B, true, false), &wide()),
        KeyEdit::Changed
    );
    assert!(input.doc().format_at(1).bold);
    input.doc_end(false);
    assert_eq!(
        input.handle_key(&key(Key::Enter, false, false), &wide()),
        KeyEdit::Changed
    );
    assert_eq!(input.text(), "ab\n");
    assert_eq!(
        input.handle_key(&key(Key::Z, true, false), &wide()),
        KeyEdit::Changed
    );
    assert_eq!(input.text(), "ab");
    assert_eq!(
        input.handle_key(&key(Key::Escape, false, false), &wide()),
        KeyEdit::Unhandled
    );
}

/// **Up and Down keep the column across lines of different sizes, and
/// Home and End go to the caret's own line's ends** -- a wrapped line's end
/// on it, not the start of the next.
#[test]
fn up_down_home_and_end_move_by_line() {
    let w = |t: &str| crate::text::measure(t, SIZE, FontWeightHint::Regular);
    let mut input = RichInput::with_doc(RichDoc::plain("aaa bbb ccc", Format::default()));
    let narrow = m(w("aaa bbb") + 1.0);
    input.set_cursor(9);
    input.move_up(false, &narrow);
    assert!(input.cursor() < 8, "on the first line: {}", input.cursor());
    input.move_down(false, &narrow);
    assert_eq!(input.cursor(), 9, "back to the column it left");
    input.line_start(false, &narrow);
    assert_eq!(input.cursor(), 8);
    input.set_cursor(2);
    input.line_end(false, &narrow);
    assert_eq!(input.cursor(), 8, "the end of the first line");
    // At the wrap, still on the first line: Home goes to its start.
    input.line_start(false, &narrow);
    assert_eq!(input.cursor(), 0);
    input.move_up(false, &narrow);
    assert_eq!(input.cursor(), 0, "above the first line: the start");
    input.move_down(false, &narrow);
    input.move_down(false, &narrow);
    assert_eq!(input.cursor(), 11, "below the last: the end");
}

/// **Ctrl and an arrow move a word at a time**, past the spaces first --
/// and with Shift, select it.
#[test]
fn ctrl_and_an_arrow_move_a_word_at_a_time() {
    let mut input = RichInput::with_doc(RichDoc::plain("one  two three", Format::default()));
    input.set_cursor(0);
    input.handle_key(&key(Key::Right, true, false), &wide());
    assert_eq!(input.cursor(), 3, "the end of one");
    input.handle_key(&key(Key::Right, true, false), &wide());
    assert_eq!(input.cursor(), 8, "past the spaces, the end of two");
    input.handle_key(&key(Key::Left, true, true), &wide());
    assert_eq!(input.selection_range(), Some((5, 8)), "two, selected");
    input.doc_end(false);
    input.handle_key(&key(Key::Left, true, false), &wide());
    assert_eq!(input.cursor(), 9, "the start of three");
    input.set_cursor(0);
    input.move_word_left(false);
    assert_eq!(input.cursor(), 0, "nothing before");
}

/// **A press puts the caret where it lands; a drag selects from there.**
#[test]
fn a_press_and_a_drag_select() {
    let w = |t: &str| crate::text::measure(t, SIZE, FontWeightHint::Regular);
    let mut input = RichInput::with_doc(RichDoc::plain("hello world", Format::default()));
    input.press(w("he"), 2.0, false, &wide());
    assert_eq!(input.cursor(), 2);
    assert!(!input.has_selection());
    input.drag_to(w("hello wo"), 2.0, &wide());
    assert_eq!(input.selection_range(), Some((2, 8)));
    input.press(w("hello world") + 50.0, 2.0, true, &wide());
    assert_eq!(input.selection_range(), Some((2, 11)), "Shift: stretched");
}

/// **It is drawn in each run's format**: a bold run bold, an underline
/// under its text, the selection under it all, the caret with the keyboard,
/// and the placeholder while empty.
#[test]
fn it_is_drawn_in_each_runs_format() {
    let palette = Palette::for_mode(false);
    let look = |focused| Look {
        rect: (0.0, 0.0, 400.0, 100.0),
        focused,
        placeholder: "Write here",
    };
    let mut tree = RenderTree::new();
    RichInput::new().draw(&mut tree, &palette, &look(false), &wide());
    assert!(tree.commands.iter().any(|c| matches!(c,
        RenderCommand::Text { text, .. } if text == "Write here")));

    let mut doc = RichDoc::plain("bold under", Format::default());
    doc.apply(0, 4, |f| f.bold = true);
    doc.apply(5, 10, |f| f.underline = true);
    let mut input = RichInput::with_doc(doc);
    input.set_cursor(0);
    input.move_right(true);
    let mut tree = RenderTree::new();
    input.draw(&mut tree, &palette, &look(true), &wide());
    let texts: Vec<(String, FontWeightHint)> = tree
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::Text {
                text, font_weight, ..
            } => Some((text.clone(), *font_weight)),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        [
            ("bold".to_string(), FontWeightHint::Bold),
            ("under".to_string(), FontWeightHint::Regular)
        ],
        "the space between, alone in its format, draws nothing"
    );
    let fills = tree
        .commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
        .count();
    // The selection, the underline and the caret.
    assert_eq!(fills, 3, "{:?}", tree.commands);
}

/// **A colour and a size go on the selection; a size that is no size is the
/// field's.**
#[test]
fn a_colour_and_a_size_go_on_the_selection() {
    let red = Color::rgba(255, 0, 0, 255);
    let mut input = RichInput::with_doc(RichDoc::plain("ab", Format::default()));
    input.select_all();
    input.set_color(Some(red));
    input.set_size(Some(20.0));
    assert_eq!(input.doc().format_at(1).color, Some(red));
    assert_eq!(input.doc().format_at(1).size, Some(20.0));
    input.set_size(Some(f32::NAN));
    assert_eq!(input.doc().format_at(1).size, None);
}

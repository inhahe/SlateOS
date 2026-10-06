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

/// **Ctrl+Shift+V pastes a formatted copy's text alone**, in the format at
/// the caret; Ctrl+V still brings the formatting.
#[test]
fn ctrl_shift_v_pastes_the_text_alone() {
    let mut input = RichInput::with_doc(RichDoc::plain("ab", Format::default()));
    input.select_all();
    input.toggle(Toggle::Bold);
    input.copy();
    let mut other = RichInput::with_doc(RichDoc::plain("x", Format::default()));
    other.doc_end(false);
    assert_eq!(
        other.handle_key(&key(Key::V, true, true), &wide()),
        KeyEdit::Changed
    );
    assert_eq!(other.text(), "xab");
    assert_eq!(bold_runs(&other), [(0, 3, false)], "plain, as the x before");
    other.handle_key(&key(Key::V, true, false), &wide());
    assert_eq!(
        bold_runs(&other),
        [(0, 3, false), (3, 5, true)],
        "Ctrl+V: as copied"
    );
    crate::clipboard::set_text("");
    assert_eq!(
        other.handle_key(&key(Key::V, true, true), &wide()),
        KeyEdit::Handled,
        "nothing to paste"
    );
}

/// The menu's rows as (label, lit, ticked), separators left out.
fn menu_rows(input: &RichInput) -> Vec<(String, bool, Option<bool>)> {
    input
        .edit_menu()
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::menu::MenuItem::Action {
                label,
                enabled,
                checked,
                ..
            } => Some((label.clone(), *enabled, *checked)),
            _ => None,
        })
        .collect()
}

/// **A right-click offers what the keys do** -- the edit rows every field
/// has, Paste as plain text beside Paste, and the switches, ticked where
/// what is shown has them -- each lit only where it would do something.
#[test]
fn the_menu_offers_what_the_keys_do() {
    crate::clipboard::set_text("");
    let mut input = RichInput::with_doc(RichDoc::plain("ab", Format::default()));
    let labels: Vec<String> = menu_rows(&input).into_iter().map(|(l, ..)| l).collect();
    assert_eq!(
        labels,
        [
            "Undo",
            "Redo",
            "Cut",
            "Copy",
            "Paste",
            "Paste as plain text",
            "Delete",
            "Select all",
            "Bold",
            "Italic",
            "Underline"
        ]
    );
    let lit = |input: &RichInput, label: &str| {
        menu_rows(input)
            .into_iter()
            .find(|(l, ..)| l == label)
            .map(|(_, on, ticked)| (on, ticked))
            .unwrap()
    };
    assert_eq!(
        lit(&input, "Paste as plain text"),
        (false, None),
        "nothing copied"
    );
    assert_eq!(lit(&input, "Cut"), (false, None), "nothing selected");
    assert_eq!(lit(&input, "Bold"), (true, Some(false)));
    input.select_all();
    input.toggle(Toggle::Bold);
    assert_eq!(
        lit(&input, "Bold"),
        (true, Some(true)),
        "ticked: all of it is"
    );
    assert_eq!(lit(&input, "Cut"), (true, None));
    assert_eq!(lit(&input, "Undo"), (true, None));
    crate::clipboard::set_text("x");
    assert_eq!(lit(&input, "Paste as plain text"), (true, None));
    let menu = input.edit_menu();
    assert_eq!(
        menu.reason(EditCommand::Redo.id()),
        Some("There is nothing to redo")
    );
}

/// **A row of the menu does what its keys do**, and answers as they would.
#[test]
fn a_menu_row_does_what_its_keys_do() {
    let mut input = RichInput::with_doc(RichDoc::plain("ab", Format::default()));
    input.select_all();
    input.toggle(Toggle::Bold);
    input.copy();
    let mut other = RichInput::with_doc(RichDoc::plain("x", Format::default()));
    other.doc_end(false);
    assert_eq!(
        other.edit_command(RichCommand::PastePlain.id()),
        KeyEdit::Changed
    );
    assert_eq!(bold_runs(&other), [(0, 3, false)], "the text alone");
    other.select_all();
    assert_eq!(
        other.edit_command(RichCommand::Italic.id()),
        KeyEdit::Changed
    );
    assert!(other.doc().format_at(1).italic);
    assert_eq!(
        other.edit_command(EditCommand::SelectAll.id()),
        KeyEdit::Handled
    );
    assert_eq!(other.edit_command(EditCommand::Cut.id()), KeyEdit::Changed);
    assert_eq!(other.text(), "");
    assert_eq!(crate::clipboard::text(), "xab");
    assert_eq!(other.edit_command(EditCommand::Undo.id()), KeyEdit::Changed);
    assert_eq!(other.text(), "xab");
    assert_eq!(other.edit_command(0x1234), KeyEdit::Unhandled);
    assert_eq!(
        RichCommand::from_id(RichCommand::Bold.id()),
        Some(RichCommand::Bold)
    );
    assert_eq!(RichCommand::from_id(EditCommand::Paste.id()), None);
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

/// **Typing at a boundary between two formats takes the format before the
/// caret**, not the one after it -- in the middle of the text, where the two
/// differ, as well as at its end.
#[test]
fn typing_between_two_formats_takes_the_one_before() {
    let mut input = RichInput::with_doc(RichDoc::plain("ab", Format::default()));
    input.set_cursor(0);
    input.move_right(true);
    input.toggle(Toggle::Bold);
    input.set_cursor(1);
    typed(&mut input, "x");
    assert_eq!(bold_runs(&input), [(0, 2, true), (2, 3, false)]);
}

/// **Backspaces one after another are one step to undo**, as typing is:
/// the whole run of them comes back at once.
#[test]
fn backspaces_one_after_another_undo_together() {
    let mut input = RichInput::new();
    typed(&mut input, "abcdef");
    for _ in 0..3 {
        assert!(input.backspace());
    }
    assert_eq!(input.text(), "abc");
    assert!(input.undo());
    assert_eq!(input.text(), "abcdef", "one backspace undone, not the run");
}

/// **Up and Down keep the column they started from** across a shorter line
/// between: up from a long line through a short one lands on the long one
/// above, over where it began -- not under the short line's end.
#[test]
fn up_and_down_keep_their_column_across_a_short_line() {
    let w = |t: &str| crate::text::measure(t, SIZE, FontWeightHint::Regular);
    // Three lines when wrapped: eight a's, a lone b, eight a's -- the same
    // letter above and below, so a column is the same count of them in a
    // face of any widths.
    let mut input = RichInput::with_doc(RichDoc::plain("aaaaaaaa b aaaaaaaa", Format::default()));
    let narrow = m(w("aaaaaaaa") + 1.0);
    // Six letters into the last line.
    input.set_cursor(17);
    input.move_up(false, &narrow);
    assert!(
        (9..=11).contains(&input.cursor()),
        "on the short line: {}",
        input.cursor()
    );
    input.move_up(false, &narrow);
    assert_eq!(
        input.cursor(),
        6,
        "under where it began, not at the short line's end"
    );
}

/// **An underline is drawn under the text** -- below the baseline, in the
/// lower part of the line -- not through it.
#[test]
fn an_underline_is_drawn_under_the_text() {
    let palette = Palette::for_mode(false);
    let look = Look {
        rect: (0.0, 0.0, 400.0, 100.0),
        focused: false,
        placeholder: "",
    };
    let mut doc = RichDoc::plain("under", Format::default());
    doc.apply(0, 5, |f| f.underline = true);
    let input = RichInput::with_doc(doc);
    let mut tree = RenderTree::new();
    input.draw(&mut tree, &palette, &look, &wide());
    let text_y = tree
        .commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::Text { text, y, .. } if text == "under" => Some(*y),
            _ => None,
        })
        .expect("the text is drawn");
    let line_y = tree
        .commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::FillRect { y, height, .. } if *height <= 3.0 => Some(*y),
            _ => None,
        })
        .expect("the underline is drawn");
    // Below the baseline -- the text's top and its ascent -- where a strike
    // through it is above.
    let baseline = text_y + crate::text::ascent(SIZE, FontWeightHint::Regular);
    assert!(
        line_y >= baseline,
        "the underline at {line_y} runs through text whose baseline is {baseline}"
    );
    assert!(
        line_y < baseline + SIZE * 0.5,
        "the underline at {line_y} is far under text whose baseline is {baseline}"
    );
}

/// **Runs of different sizes on one line share its baseline**: the smaller
/// is set lower, by the difference in their ascents, not at the line's top.
#[test]
fn runs_of_two_sizes_share_the_lines_baseline() {
    let palette = Palette::for_mode(false);
    let look = Look {
        rect: (0.0, 0.0, 400.0, 100.0),
        focused: false,
        placeholder: "",
    };
    let mut doc = RichDoc::plain("BIG small", Format::default());
    doc.apply(0, 3, |f| f.size = Some(SIZE * 2.0));
    let input = RichInput::with_doc(doc);
    let mut tree = RenderTree::new();
    input.draw(&mut tree, &palette, &look, &wide());
    let y_of = |word: &str| {
        tree.commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, y, .. } if text.trim() == word => Some(*y),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{word} is not drawn: {:?}", tree.commands))
    };
    assert!(
        y_of("small") > y_of("BIG"),
        "the small run is set at the line's top"
    );
}

/// "Shalom", in Hebrew: a right-to-left word of four letters, two bytes
/// each.
const SHALOM: &str = "\u{5e9}\u{5dc}\u{5d5}\u{5dd}";

/// Where the caret of `input` is drawn on its first line.
fn caret_x(input: &RichInput) -> f32 {
    let palette = Palette::for_mode(false);
    let look = Look {
        rect: (0.0, 0.0, 1000.0, 100.0),
        focused: true,
        placeholder: "",
    };
    let mut tree = RenderTree::new();
    input.draw(&mut tree, &palette, &look, &wide());
    tree.commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::FillRect {
                x, width, color, ..
            } if *color == palette.text && *width < 4.0 => Some(*x),
            _ => None,
        })
        .expect("the caret is drawn")
}

/// **Right goes right on the screen across a right-to-left word**: from
/// before the space ahead of it, each press moves the caret further right
/// -- to the Hebrew's left edge, through it from its end to its start, to
/// its right edge -- and on into the English after it; Left retraces it.
#[test]
fn right_goes_right_across_a_right_to_left_word() {
    let text = format!("ab {SHALOM} cd");
    let mut input = RichInput::with_doc(RichDoc::plain(&text, Format::default()));
    input.set_cursor(2);
    let mut seen = vec![input.cursor()];
    let mut xs = vec![caret_x(&input)];
    for _ in 0..7 {
        input.handle_key(&key(Key::Right, false, false), &wide());
        seen.push(input.cursor());
        xs.push(caret_x(&input));
    }
    assert!(
        xs.windows(2).all(|p| p[1] > p[0]),
        "each press further right: {xs:?} at {seen:?}"
    );
    // The Hebrew's left edge is its end, so the letters go from its last.
    assert_eq!(seen, [2, 3, 9, 7, 5, 3, 12, 13], "{xs:?}");
    for _ in 0..7 {
        input.handle_key(&key(Key::Left, false, false), &wide());
    }
    assert_eq!(input.cursor(), 2, "Left retraces it");
    assert!((caret_x(&input) - xs[0]).abs() < 0.01);
}

/// **A selection across a change of direction is drawn where its text is**:
/// from inside the English into the Hebrew, two boxes -- the selected
/// Hebrew letters are at the word's right, not beside the English.
#[test]
fn a_selection_across_two_directions_is_drawn_where_its_text_is() {
    let palette = Palette::for_mode(false);
    let look = Look {
        rect: (0.0, 0.0, 400.0, 100.0),
        focused: false,
        placeholder: "",
    };
    let text = format!("ab {SHALOM} cd");
    let mut input = RichInput::with_doc(RichDoc::plain(&text, Format::default()));
    input.set_cursor(1);
    // To the end of the Hebrew word's first letter.
    for _ in 0..3 {
        input.move_right(true);
    }
    assert_eq!(input.selection_range(), Some((1, 5)));
    let mut tree = RenderTree::new();
    input.draw(&mut tree, &palette, &look, &wide());
    let fill = palette.selection_fill();
    let boxes: Vec<(f32, f32)> = tree
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect {
                x, width, color, ..
            } if *color == fill => Some((*x, *width)),
            _ => None,
        })
        .collect();
    assert_eq!(boxes.len(), 2, "{boxes:?}");
    let lines = layout::lay_out(input.doc(), &wide());
    let hebrew = lines[0]
        .pieces
        .iter()
        .find(|p| p.rtl)
        .expect("the Hebrew piece");
    let right = hebrew.x + hebrew.width;
    assert!(
        boxes
            .iter()
            .any(|&(x, w)| (x + w - right).abs() < 0.5 && w < hebrew.width),
        "a box at the Hebrew's right end: {boxes:?}, the word {}..{right}",
        hebrew.x
    );
}

// ---- Pictures ----

/// A `width` by `height` picture of one colour.
fn picture(width: u32, height: u32) -> Picture {
    Picture::new(imagecodec::Image {
        width,
        height,
        pixels: vec![0xFF20_6080; (width * height) as usize],
    })
    .unwrap()
}

/// The ids of the pictures in `input`'s text, in order.
fn ids(input: &RichInput) -> Vec<u64> {
    input.pictures().map(Picture::id).collect()
}

/// "ab|cd", the caret at the bar.
fn ab_cd() -> RichInput {
    let mut input = RichInput::with_doc(RichDoc::plain("abcd", Format::default()));
    input.set_cursor(2);
    input
}

/// **A picture is put in at the caret as one character, and goes and comes
/// back as one**: Backspace takes it, Undo brings the same picture back,
/// Redo takes it again.
#[test]
fn a_picture_is_put_in_and_taken_out_as_a_character() {
    let p = picture(8, 6);
    let mut input = ab_cd();
    input.insert_picture(p.clone());
    assert_eq!(input.text(), "ab\u{fffc}cd");
    assert_eq!(input.cursor(), 2 + OBJECT.len_utf8(), "the caret after it");
    assert_eq!(ids(&input), [p.id()]);
    assert!(input.backspace());
    assert_eq!(input.text(), "abcd");
    assert!(ids(&input).is_empty());
    assert!(input.undo());
    assert_eq!(ids(&input), [p.id()], "the same picture back");
    assert!(input.redo());
    assert!(ids(&input).is_empty());
    // In place of a selection, in the format typing there would take: the
    // bold "a" before it.
    input.set_cursor(0);
    input.move_right(true);
    input.toggle(Toggle::Bold);
    input.set_cursor(2);
    input.move_left(true);
    input.insert_picture(p.clone());
    assert_eq!(input.text(), "a\u{fffc}cd");
    assert!(input.doc().format_at(1).bold, "in the format before it");
    assert!(
        !input.doc().format_at(4).bold,
        "the text after it as it was"
    );
}

/// **A copy with a picture in it pastes whole here, the same picture; its
/// plain text, for any field, leaves the picture out** -- and once another
/// copy is made, a paste is that copy's.
#[test]
fn a_copy_with_a_picture_pastes_whole() {
    let p = picture(4, 4);
    let mut input = ab_cd();
    input.insert_picture(p.clone());
    input.select_all();
    input.copy();
    assert_eq!(crate::clipboard::text(), "abcd", "the picture left out");
    assert!(
        !crate::clipboard::has_picture(),
        "text and a picture: the copy is not a picture alone"
    );
    let mut other = RichInput::new();
    other.paste();
    assert_eq!(other.text(), "ab\u{fffc}cd");
    assert_eq!(ids(&other), [p.id()]);
    crate::clipboard::set_text("abcd");
    other.paste();
    assert_eq!(
        other.text(),
        "ab\u{fffc}cdabcd",
        "the same text copied again is another copy: plain"
    );
}

/// **A picture selected alone goes on the clipboard as itself**, and a
/// picture copied alone -- here or by anything else in the program -- is
/// pasted as a picture; Paste as plain text has no text to take and says
/// so.
#[test]
fn a_picture_selected_alone_is_copied_as_itself() {
    let p = picture(3, 2);
    let mut input = ab_cd();
    input.insert_picture(p.clone());
    input.move_left(true);
    input.copy();
    assert_eq!(crate::clipboard::picture(), Some(p.clone()));
    assert_eq!(crate::clipboard::text(), "");
    let shot = picture(5, 5);
    crate::clipboard::set_picture(shot.clone());
    let mut other = RichInput::new();
    other.paste();
    assert_eq!(ids(&other), [shot.id()], "a picture from elsewhere");
    let rows = menu_rows(&other);
    assert!(
        rows.iter().any(|(l, lit, _)| l == "Paste" && *lit),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|(l, lit, _)| l == "Paste as plain text" && !*lit),
        "{rows:?}"
    );
    assert_eq!(
        other.edit_menu().reason(RichCommand::PastePlain.id()),
        Some(crate::editmenu::PICTURE_ONLY)
    );
    let before = other.doc().clone();
    other.paste_plain();
    assert_eq!(other.doc(), &before, "no text to paste");
}

/// **A dropped picture lands where it was dropped**; dropped text, likewise;
/// what the field cannot use changes nothing, the caret included.
#[test]
fn a_dropped_picture_lands_where_it_was_dropped() {
    let p = picture(6, 6);
    let mut data = DataObject::new();
    data.set_data(DataFormat::ImagePng, p.to_png().unwrap());
    let mut input = RichInput::with_doc(RichDoc::plain("abcd", Format::default()));
    input.set_cursor(4);
    let lines = layout::lay_out(input.doc(), &wide());
    let x = layout::x_of(input.doc(), &lines[0], 1, SIZE);
    assert!(input.drop_data(&data, x, 2.0, &wide()));
    assert_eq!(input.text(), "a\u{fffc}bcd");
    let dropped = input.pictures().next().unwrap();
    assert_eq!(
        (dropped.width(), dropped.height()),
        (6, 6),
        "the file's picture"
    );
    let caret = input.cursor();
    let mut junk = DataObject::new();
    junk.set_data(DataFormat::ImagePng, b"no picture".to_vec());
    let before = input.doc().clone();
    assert!(!input.drop_data(&junk, 0.0, 0.0, &wide()));
    assert_eq!(input.doc(), &before);
    assert_eq!(input.cursor(), caret, "the caret stays");
    assert!(input.drop_data(&DataObject::with_text("Z"), 0.0, 2.0, &wide()));
    assert!(input.text().starts_with('Z'), "{}", input.text());
}

/// **A picture is drawn where its piece is, standing on the baseline, and
/// a selected one is tinted over**; the window is given each picture the
/// field shows.
#[test]
fn a_picture_is_drawn_standing_on_the_baseline() {
    let palette = Palette::for_mode(false);
    let look = Look {
        rect: (10.0, 20.0, 400.0, 200.0),
        focused: false,
        placeholder: "",
    };
    let p = picture(30, 40);
    let mut input = ab_cd();
    input.insert_picture(p.clone());
    let lines = layout::lay_out(input.doc(), &m(400.0));
    let line = &lines[0];
    let piece = line
        .pieces
        .iter()
        .find(|piece| piece.picture.is_some())
        .unwrap();
    let mut tree = RenderTree::new();
    input.draw(&mut tree, &palette, &look, &m(400.0));
    let image = tree
        .commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::Image {
                x,
                y,
                width,
                height,
                image_id,
            } => Some((*x, *y, *width, *height, *image_id)),
            _ => None,
        })
        .expect("the picture is drawn");
    assert_eq!(
        image,
        (
            10.0 + piece.x,
            20.0 + line.ascent - 40.0,
            30.0,
            40.0,
            p.id()
        )
    );
    // Selected: a tint over it, after it.
    input.select_all();
    let mut tree = RenderTree::new();
    input.draw(&mut tree, &palette, &look, &m(400.0));
    let at = tree
        .commands
        .iter()
        .position(|c| matches!(c, RenderCommand::Image { .. }))
        .unwrap();
    assert!(
        tree.commands[at..].iter().any(|c| matches!(c,
            RenderCommand::FillRect { x, width, color, .. }
                if *color == palette.selection_fill()
                    && (*x - image.0).abs() < 0.01
                    && (*width - 30.0).abs() < 0.01)),
        "{:?}",
        tree.commands
    );
    let mut uploads = crate::picture::Uploads::new();
    assert_eq!(
        uploads.changes(input.pictures()),
        [crate::picture::Change::Upload(p.clone())]
    );
    input.backspace();
    assert_eq!(
        uploads.changes(input.pictures()),
        [crate::picture::Change::Drop(p.id())]
    );
}

/// **A picture selected on a line of two directions is boxed where it is**:
/// whole, its own width, however the line is ordered.
#[test]
fn a_selected_picture_between_two_directions_is_boxed_where_it_is() {
    let palette = Palette::for_mode(false);
    let look = Look {
        rect: (0.0, 0.0, 400.0, 100.0),
        focused: false,
        placeholder: "",
    };
    let p = picture(24, 8);
    let mut doc = RichDoc::plain(&format!("ab {SHALOM}"), Format::default());
    doc.append(&RichDoc::picture(p, Format::default()));
    doc.append(&RichDoc::plain(SHALOM, Format::default()));
    let mut input = RichInput::with_doc(doc);
    input.select_all();
    let lines = layout::lay_out(input.doc(), &wide());
    assert!(!lines[0].is_ltr());
    let piece = lines[0]
        .pieces
        .iter()
        .find(|piece| piece.picture.is_some())
        .unwrap()
        .clone();
    let mut tree = RenderTree::new();
    input.draw(&mut tree, &palette, &look, &wide());
    let fill = palette.selection_fill();
    let boxes: Vec<(f32, f32)> = tree
        .commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect {
                x,
                width,
                height,
                color,
                ..
            } if *color == fill && (*height - lines[0].height).abs() < 0.01 => Some((*x, *width)),
            _ => None,
        })
        .collect();
    assert!(
        boxes
            .iter()
            .any(|&(x, w)| (x - piece.x).abs() < 0.01 && (w - 24.0).abs() < 0.01),
        "the picture's box: {boxes:?}, the picture at {}",
        piece.x
    );
}

/// **A click on a picture puts the caret at its nearer edge, and the arrows
/// step over it as over a letter.**
#[test]
fn a_click_on_a_picture_goes_to_its_nearer_edge() {
    let p = picture(40, 20);
    let mut input = ab_cd();
    input.insert_picture(p);
    let lines = layout::lay_out(input.doc(), &wide());
    let piece = lines[0]
        .pieces
        .iter()
        .find(|piece| piece.picture.is_some())
        .unwrap()
        .clone();
    input.press(piece.x + 5.0, 2.0, false, &wide());
    assert_eq!(input.cursor(), piece.start, "its left half: before it");
    input.press(piece.x + 35.0, 2.0, false, &wide());
    assert_eq!(input.cursor(), piece.end, "its right half: after it");
    input.handle_key(&key(Key::Left, false, false), &wide());
    assert_eq!(input.cursor(), piece.start, "one step back over it");
}

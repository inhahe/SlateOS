// A test module's job is to fail loudly the instant the code under test is
// wrong, so the defensive lints that forbid exactly that in production code
// are off here -- as `CLAUDE.md` prescribes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]

use super::*;
use crate::event::{Key, KeyEvent, Modifiers};

const SIZE: f32 = 13.0;
const WEIGHT: FontWeightHint = FontWeightHint::Regular;

/// A box `lines` lines tall and wide enough for `fits` and a pixel, so that a
/// test says in words where its text wraps.
fn box_for(fits: &str, lines: usize) -> Metrics {
    Metrics {
        width: text::measure(fits, SIZE, WEIGHT) + 1.0,
        height: lines as f32 * text::line_height(SIZE, WEIGHT),
        font_size: SIZE,
        weight: WEIGHT,
    }
}

/// A box so wide nothing in these tests wraps.
fn wide(lines: usize) -> Metrics {
    Metrics {
        width: 10_000.0,
        height: lines as f32 * text::line_height(SIZE, WEIGHT),
        font_size: SIZE,
        weight: WEIGHT,
    }
}

fn key_with(k: Key, modifiers: Modifiers, text: &str) -> KeyEvent {
    KeyEvent {
        key: k,
        pressed: true,
        modifiers,
        text: text.to_string(),
    }
}

fn key(k: Key) -> KeyEvent {
    key_with(k, Modifiers::NONE, "")
}

fn ctrl(k: Key) -> KeyEvent {
    key_with(
        k,
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        "",
    )
}

fn shift(k: Key) -> KeyEvent {
    key_with(
        k,
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        "",
    )
}

/// Type `text` a character at a time, as a keyboard delivers it.
fn type_text(area: &mut TextArea, text: &str, m: &Metrics) {
    for ch in text.chars() {
        let event = if ch == '\n' {
            key(Key::Enter)
        } else {
            key_with(Key::A, Modifiers::NONE, &ch.to_string())
        };
        area.edit_key(&event, m);
    }
}

fn line_index(area: &TextArea, m: &Metrics) -> usize {
    area.caret_position(m).0
}

// ---- typing and deleting ---------------------------------------------------

#[test]
fn typing_and_enter_make_lines() {
    let m = wide(5);
    let mut area = TextArea::new();
    type_text(&mut area, "ab\ncd", &m);
    assert_eq!(area.text(), "ab\ncd");
    assert_eq!(area.cursor().byte(), 5);
    assert_eq!(area.lines(&m).len(), 2);
    assert_eq!(line_index(&area, &m), 1);
}

#[test]
fn every_kind_of_line_ending_is_stored_as_a_newline() {
    let mut area = TextArea::new();
    area.set_clipboard("a\r\nb\rc\nd".to_string());
    area.paste();
    assert_eq!(area.text(), "a\nb\nc\nd");
    let loaded = TextArea::with_text("x\r\ny");
    assert_eq!(loaded.text(), "x\ny");
}

#[test]
fn backspace_at_a_line_start_joins_it_to_the_line_above() {
    let m = wide(5);
    let mut area = TextArea::with_text("ab\ncd");
    area.set_cursor(TextCursor::from(3));
    area.edit_key(&key(Key::Backspace), &m);
    assert_eq!(area.text(), "abcd");
    assert_eq!(area.cursor().byte(), 2);
}

#[test]
fn delete_at_a_line_end_joins_the_next_line_to_it() {
    let m = wide(5);
    let mut area = TextArea::with_text("ab\ncd");
    area.set_cursor(TextCursor::from(2));
    area.edit_key(&key(Key::Delete), &m);
    assert_eq!(area.text(), "abcd");
    assert_eq!(area.cursor().byte(), 2);
}

#[test]
fn typing_over_a_selection_replaces_it() {
    let m = wide(5);
    let mut area = TextArea::with_text("hello");
    area.set_cursor(TextCursor::from(1));
    for _ in 0..3 {
        area.edit_key(&shift(Key::Right), &m);
    }
    assert_eq!(area.selected_text(), "ell");
    type_text(&mut area, "X", &m);
    assert_eq!(area.text(), "hXo");
    assert!(!area.has_selection());
}

#[test]
fn cut_and_paste_carry_newlines() {
    let m = wide(5);
    let mut area = TextArea::with_text("one\ntwo");
    area.edit_key(&ctrl(Key::A), &m);
    assert_eq!(area.edit_key(&ctrl(Key::X), &m), KeyEdit::Changed);
    assert_eq!(area.text(), "");
    assert_eq!(area.clipboard(), "one\ntwo");
    assert_eq!(area.edit_key(&ctrl(Key::V), &m), KeyEdit::Changed);
    assert_eq!(area.text(), "one\ntwo");
}

// ---- up and down --------------------------------------------------------------

/// Down through a short line and on to a long one comes back to the column it
/// started in, rather than the short line's end.
#[test]
fn up_and_down_hold_the_column_through_a_short_line() {
    let m = wide(5);
    let mut area = TextArea::with_text("abcdef\nab\nabcdef");
    area.set_cursor(TextCursor::from(6));
    area.edit_key(&key(Key::Down), &m);
    assert_eq!(area.cursor().byte(), 9, "the end of the short line");
    area.edit_key(&key(Key::Down), &m);
    assert_eq!(area.cursor().byte(), 16, "back to the sixth column");
    area.edit_key(&key(Key::Up), &m);
    area.edit_key(&key(Key::Up), &m);
    assert_eq!(area.cursor().byte(), 6);
}

#[test]
fn up_from_the_first_line_and_down_from_the_last_go_to_the_ends() {
    let m = wide(5);
    let mut area = TextArea::with_text("abc\ndef");
    area.set_cursor(TextCursor::from(2));
    area.edit_key(&key(Key::Up), &m);
    assert_eq!(area.cursor().byte(), 0);
    area.set_cursor(TextCursor::from(5));
    area.edit_key(&key(Key::Down), &m);
    assert_eq!(area.cursor().byte(), 7);
}

#[test]
fn shift_down_selects_across_the_line_break() {
    let m = wide(5);
    let mut area = TextArea::with_text("abc\ndef");
    area.set_cursor(TextCursor::from(1));
    area.edit_key(&shift(Key::Down), &m);
    assert_eq!(area.selected_text(), "bc\nd");
}

#[test]
fn page_down_moves_a_boxful_less_one() {
    let m = wide(4);
    let text: Vec<String> = (0..20).map(|n| format!("line {n}")).collect();
    let mut area = TextArea::with_text(&text.join("\n"));
    area.edit_key(&ctrl(Key::Home), &m);
    area.edit_key(&key(Key::PageDown), &m);
    assert_eq!(line_index(&area, &m), 3);
    area.edit_key(&key(Key::PageUp), &m);
    assert_eq!(line_index(&area, &m), 0);
}

// ---- a wrapped paragraph ----------------------------------------------------------

/// "alpha beta gamma" in a box that holds "alpha beta": two lines, the
/// second "gamma".
fn wrapped() -> (TextArea, Metrics) {
    let m = box_for("alpha beta", 5);
    let area = TextArea::with_text("alpha beta gamma");
    (area, m)
}

#[test]
fn a_long_paragraph_wraps_into_lines_that_share_their_boundary() {
    let (area, m) = wrapped();
    let lines = area.lines(&m);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].soft);
    assert!(!lines[1].soft);
    assert_eq!(lines[0].end, lines[1].start);
    assert_eq!(&area.text()[lines[1].start..], "gamma");
}

/// End on a wrapped line puts the caret at the end of *that* line, though
/// its offset is also the start of the next; Home on the next line puts it
/// at the start of the next.
#[test]
fn end_and_home_choose_the_line_a_wrap_boundary_is_drawn_on() {
    let (mut area, m) = wrapped();
    let boundary = area.lines(&m)[1].start;
    area.edit_key(&ctrl(Key::Home), &m);
    area.edit_key(&key(Key::End), &m);
    assert_eq!(area.cursor().byte(), boundary);
    assert_eq!(line_index(&area, &m), 0, "End stays on the first line");

    area.edit_key(&key(Key::Down), &m);
    area.edit_key(&key(Key::Home), &m);
    assert_eq!(area.cursor().byte(), boundary);
    assert_eq!(line_index(&area, &m), 1, "Home stays on the second line");
}

#[test]
fn left_from_a_continued_line_goes_to_the_end_of_the_line_above() {
    let (mut area, m) = wrapped();
    let boundary = area.lines(&m)[1].start;
    area.set_cursor(TextCursor::from(boundary));
    assert_eq!(line_index(&area, &m), 1);
    area.edit_key(&key(Key::Left), &m);
    assert_eq!(area.cursor().byte(), boundary);
    assert_eq!(line_index(&area, &m), 0);
    area.edit_key(&key(Key::Right), &m);
    assert_eq!(area.cursor().byte(), boundary);
    assert_eq!(line_index(&area, &m), 1);
}

#[test]
fn left_and_right_cross_a_newline() {
    let m = wide(5);
    let mut area = TextArea::with_text("ab\ncd");
    area.set_cursor(TextCursor::from(3));
    area.edit_key(&key(Key::Left), &m);
    assert_eq!(area.cursor().byte(), 2, "the end of the line above");
    area.edit_key(&key(Key::Right), &m);
    assert_eq!(area.cursor().byte(), 3, "the start of the line below");
}

#[test]
fn left_with_a_selection_collapses_it_to_its_start() {
    let m = wide(5);
    let mut area = TextArea::with_text("hello");
    area.edit_key(&ctrl(Key::A), &m);
    area.edit_key(&key(Key::Left), &m);
    assert!(!area.has_selection());
    assert_eq!(area.cursor().byte(), 0);
    area.edit_key(&ctrl(Key::A), &m);
    area.edit_key(&key(Key::Right), &m);
    assert_eq!(area.cursor().byte(), 5);
}

/// Off the left of a right-to-left line is its logical end, which carries on
/// to the start of the line below -- the direction the line is read in.
#[test]
fn off_the_left_of_a_right_to_left_line_is_the_line_below() {
    let m = wide(5);
    let mut area = TextArea::with_text("\u{05e9}\u{05dc}\u{05d5}\u{05dd}\nabc");
    let end = area.text().find('\n').unwrap();
    area.set_cursor(TextCursor::from(end));
    area.edit_key(&key(Key::Left), &m);
    assert_eq!(area.cursor().byte(), end + 1, "the start of the next line");
}

// ---- undo --------------------------------------------------------------------

#[test]
fn undo_takes_back_typing_a_word_at_a_time() {
    let m = wide(5);
    let mut area = TextArea::new();
    type_text(&mut area, "hello world", &m);
    assert!(area.undo());
    assert_eq!(area.text(), "hello ");
    assert!(area.undo());
    assert_eq!(area.text(), "");
    assert!(!area.undo());
    assert!(area.redo());
    assert_eq!(area.text(), "hello ");
    assert!(area.redo());
    assert_eq!(area.text(), "hello world");
    assert!(!area.redo());
}

#[test]
fn undo_of_a_run_of_backspaces_brings_the_caret_back_too() {
    let m = wide(5);
    let mut area = TextArea::with_text("hello");
    for _ in 0..3 {
        area.edit_key(&key(Key::Backspace), &m);
    }
    assert_eq!(area.text(), "he");
    assert_eq!(area.edit_key(&ctrl(Key::Z), &m), KeyEdit::Changed);
    assert_eq!(area.text(), "hello");
    assert_eq!(area.cursor().byte(), 5);
}

#[test]
fn undo_of_a_run_of_deletes_is_one_step() {
    let m = wide(5);
    let mut area = TextArea::with_text("hello");
    area.set_cursor(TextCursor::from(0));
    for _ in 0..3 {
        area.edit_key(&key(Key::Delete), &m);
    }
    assert_eq!(area.text(), "lo");
    area.undo();
    assert_eq!(area.text(), "hello");
    assert_eq!(area.cursor().byte(), 0);
}

#[test]
fn undo_puts_back_the_selection_an_edit_replaced() {
    let m = wide(5);
    let mut area = TextArea::with_text("hello");
    area.set_cursor(TextCursor::from(1));
    for _ in 0..3 {
        area.edit_key(&shift(Key::Right), &m);
    }
    type_text(&mut area, "X", &m);
    area.undo();
    assert_eq!(area.text(), "hello");
    assert_eq!(area.selected_text(), "ell");
}

#[test]
fn moving_the_caret_ends_an_undo_step() {
    let m = wide(5);
    let mut area = TextArea::new();
    type_text(&mut area, "ab", &m);
    area.edit_key(&key(Key::Left), &m);
    area.edit_key(&key(Key::Right), &m);
    type_text(&mut area, "cd", &m);
    area.undo();
    assert_eq!(area.text(), "ab");
}

#[test]
fn a_new_edit_after_undo_forgets_what_could_be_redone() {
    let m = wide(5);
    let mut area = TextArea::new();
    type_text(&mut area, "one", &m);
    area.undo();
    type_text(&mut area, "two", &m);
    assert!(!area.can_redo());
    assert!(!area.redo());
    assert_eq!(area.text(), "two");
}

#[test]
fn ctrl_y_and_ctrl_shift_z_both_redo() {
    let m = wide(5);
    let mut area = TextArea::new();
    type_text(&mut area, "a\nb", &m);
    area.undo();
    area.undo();
    assert_eq!(area.text(), "a");
    area.edit_key(&ctrl(Key::Y), &m);
    assert_eq!(area.text(), "a\n");
    let ctrl_shift = Modifiers {
        ctrl: true,
        shift: true,
        ..Modifiers::NONE
    };
    area.edit_key(&key_with(Key::Z, ctrl_shift, ""), &m);
    assert_eq!(area.text(), "a\nb");
}

#[test]
fn undo_reaches_back_only_so_far() {
    let m = wide(5);
    let mut area = TextArea::new();
    // A newline never joins the step before it, so each is its own.
    for _ in 0..(UNDO_DEPTH + 20) {
        type_text(&mut area, "\n", &m);
    }
    let mut undone = 0;
    while area.undo() {
        undone += 1;
    }
    assert_eq!(undone, UNDO_DEPTH);
    assert_eq!(area.text().len(), 20);
}

#[test]
fn loading_a_text_is_not_an_edit_undo_can_take_back() {
    let m = wide(5);
    let mut area = TextArea::new();
    type_text(&mut area, "draft", &m);
    area.set_text("loaded");
    assert!(!area.can_undo());
    assert!(!area.undo());
    assert_eq!(area.text(), "loaded");
}

// ---- the keyboard's edges ---------------------------------------------------------

#[test]
fn tab_and_ctrl_enter_are_left_to_the_owner() {
    let m = wide(5);
    let mut area = TextArea::with_text("x");
    assert_eq!(area.edit_key(&key(Key::Tab), &m), KeyEdit::Unhandled);
    assert_eq!(area.edit_key(&ctrl(Key::Enter), &m), KeyEdit::Unhandled);
    assert_eq!(area.text(), "x");
    assert_eq!(area.edit_key(&key(Key::Enter), &m), KeyEdit::Changed);
    assert_eq!(area.text(), "x\n");
}

#[test]
fn a_release_and_a_bare_modifier_do_nothing() {
    let m = wide(5);
    let mut area = TextArea::with_text("x");
    let mut release = key_with(Key::A, Modifiers::NONE, "a");
    release.pressed = false;
    assert_eq!(area.edit_key(&release, &m), KeyEdit::Unhandled);
    assert_eq!(area.edit_key(&key(Key::LeftShift), &m), KeyEdit::Unhandled);
    assert_eq!(area.text(), "x");
}

#[test]
fn a_caret_key_reports_handled_and_typing_reports_changed() {
    let m = wide(5);
    let mut area = TextArea::with_text("x");
    assert_eq!(area.edit_key(&key(Key::Left), &m), KeyEdit::Handled);
    assert_eq!(
        area.edit_key(&key_with(Key::B, Modifiers::NONE, "b"), &m),
        KeyEdit::Changed
    );
    // Backspace with nothing before the caret changes nothing.
    area.set_cursor(TextCursor::from(0));
    assert_eq!(area.edit_key(&key(Key::Backspace), &m), KeyEdit::Handled);
}

/// AltGr is Ctrl+Alt, and AltGr+A is how Polish types `ą`: it must type, not
/// select everything.
#[test]
fn altgr_types_rather_than_running_a_ctrl_chord() {
    let m = wide(5);
    let mut area = TextArea::with_text("x");
    let altgr = Modifiers {
        ctrl: true,
        alt: true,
        ..Modifiers::NONE
    };
    assert_eq!(
        area.edit_key(&key_with(Key::A, altgr, "\u{0105}"), &m),
        KeyEdit::Changed
    );
    assert_eq!(area.text(), "x\u{0105}");
    assert!(!area.has_selection());
}

// ---- the pointer ------------------------------------------------------------------

#[test]
fn a_press_puts_the_caret_on_the_line_it_lands_on() {
    let m = wide(5);
    let mut area = TextArea::with_text("abc\ndef\nghi");
    let per = m.line_height();
    area.press(0.0, per * 2.5, 1, false, &m);
    assert_eq!(area.cursor().byte(), 8);
    let far_right = text::measure("ghi", SIZE, WEIGHT) + 50.0;
    area.press(far_right, per * 1.5, 1, false, &m);
    assert_eq!(area.cursor().byte(), 7, "past the end of a line is its end");
}

#[test]
fn above_the_text_is_its_first_line_and_below_it_its_last() {
    let m = wide(10);
    let area = TextArea::with_text("abc\ndef");
    assert_eq!(area.cursor_at_point(0.0, -40.0, &m).byte(), 0);
    let below = area.cursor_at_point(500.0, 9_000.0, &m);
    assert_eq!(below.byte(), 7);
}

#[test]
fn a_drag_selects_from_the_press_to_the_pointer() {
    let m = wide(5);
    let mut area = TextArea::with_text("abc\ndef");
    let per = m.line_height();
    area.press(0.0, per * 0.5, 1, false, &m);
    area.drag_to(text::measure("de", SIZE, WEIGHT), per * 1.5, &m);
    assert_eq!(area.selected_text(), "abc\nde");
}

#[test]
fn shift_press_extends_the_selection_from_where_the_caret_was() {
    let m = wide(5);
    let mut area = TextArea::with_text("abc\ndef");
    area.set_cursor(TextCursor::from(1));
    area.press(0.0, m.line_height() * 1.5, 1, true, &m);
    assert_eq!(area.selected_text(), "bc\n");
}

#[test]
fn a_double_click_selects_the_word_and_a_triple_click_the_paragraph() {
    let m = wide(5);
    let mut area = TextArea::with_text("one two three\nnext");
    let x = text::measure("one tw", SIZE, WEIGHT);
    area.press(x, 1.0, 2, false, &m);
    assert_eq!(area.selected_text(), "two");
    area.press(x, 1.0, 3, false, &m);
    assert_eq!(area.selected_text(), "one two three");
}

// ---- scrolling -----------------------------------------------------------------------

fn tall() -> (TextArea, Metrics) {
    let m = wide(5);
    let text: Vec<String> = (0..20).map(|n| format!("line {n}")).collect();
    (TextArea::with_text(&text.join("\n")), m)
}

#[test]
fn a_caret_below_the_box_scrolls_the_view_to_it() {
    let (mut area, m) = tall();
    // The caret starts at the end, and a key there reveals it.
    area.edit_key(&key(Key::End), &m);
    let per = m.line_height();
    assert_eq!(area.scroll_y(&m), 20.0 * per - m.height);
    area.edit_key(&ctrl(Key::Home), &m);
    assert_eq!(area.scroll_y(&m), 0.0);
}

#[test]
fn the_wheel_scrolls_the_view_and_leaves_the_caret() {
    let (mut area, m) = tall();
    area.edit_key(&ctrl(Key::Home), &m);
    area.scroll_by(3.0 * m.line_height(), &m);
    assert_eq!(area.scroll_y(&m), 3.0 * m.line_height());
    assert_eq!(area.cursor().byte(), 0);
    area.scroll_by(-1_000.0, &m);
    assert_eq!(area.scroll_y(&m), 0.0);
    area.scroll_by(1_000_000.0, &m);
    assert_eq!(area.scroll_y(&m), area.content_height(&m) - m.height);
    area.scroll_by(f32::NAN, &m);
    assert!(area.scroll_y(&m).is_finite());
}

/// A scroll past the end is kept as the end it reached, not as the distance
/// asked for: text that arrives below afterwards is below the view, rather
/// than the view jumping down to meet it.
#[test]
fn a_scroll_past_the_end_is_kept_as_the_end_it_reached() {
    let (mut area, m) = tall();
    area.scroll_by(1_000_000.0, &m);
    let bottom = area.scroll_y(&m);
    let end = area.text().len();
    area.set_cursor(TextCursor::from(end));
    area.insert_str("\nmore\nmore\nmore");
    assert_eq!(area.scroll_y(&m), bottom);
}

#[test]
fn a_view_scrolled_past_a_shortened_text_comes_back_to_it() {
    let (mut area, m) = tall();
    area.scroll_by(1_000_000.0, &m);
    assert!(area.scroll_y(&m) > 0.0);
    area.edit_key(&ctrl(Key::A), &m);
    area.edit_key(&key(Key::Delete), &m);
    assert_eq!(
        area.scroll_y(&m),
        0.0,
        "an empty text has nowhere to scroll"
    );
}

// ---- no wrapping ----------------------------------------------------------------------

#[test]
fn without_wrapping_a_line_is_a_paragraph_and_the_view_follows_the_caret_sideways() {
    let m = box_for("short", 5);
    let mut area = TextArea::with_text("a line much longer than the box is wide\nshort");
    area.set_wrap(false);
    assert_eq!(area.lines(&m).len(), 2);
    area.set_cursor(TextCursor::from(10));
    area.edit_key(&key(Key::End), &m);
    assert!(
        area.scroll_x(&m) > 0.0,
        "the end of the long line is in view"
    );
    area.set_wrap(true);
    assert!(area.lines(&m).len() > 2);
    assert_eq!(
        area.scroll_x(&m),
        0.0,
        "a wrapped box never scrolls sideways"
    );
}

// ---- drawing ------------------------------------------------------------------------

fn drawn(area: &TextArea, m: &Metrics, focused: bool) -> Vec<RenderCommand> {
    let mut tree = RenderTree::new();
    draw(
        &mut tree,
        &MultiLine {
            area,
            x: 10.0,
            y: 20.0,
            metrics: *m,
            color: Color::rgb(200, 200, 200),
            selection_bg: Color::rgb(0, 0, 200),
            selection_fg: Color::rgb(255, 255, 255),
            focused,
            caret_width: 1.0,
            placeholder: Some(("Write something", Color::rgb(90, 90, 90))),
        },
    );
    tree.commands
}

fn texts(cmds: &[RenderCommand]) -> Vec<String> {
    cmds.iter()
        .filter_map(|c| match c {
            RenderCommand::RichText { text, .. } | RenderCommand::Text { text, .. } => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

fn carets(cmds: &[RenderCommand]) -> usize {
    cmds.iter()
        .filter(|c| matches!(c, RenderCommand::Line { .. }))
        .count()
}

#[test]
fn only_the_lines_in_view_are_drawn() {
    let (mut area, m) = tall();
    area.edit_key(&ctrl(Key::Home), &m);
    let shown = texts(&drawn(&area, &m, true));
    assert!(shown.len() <= m.lines_shown() + 2, "{shown:?}");
    assert_eq!(shown.first().map(String::as_str), Some("line 0"));
    area.edit_key(&ctrl(Key::End), &m);
    let shown = texts(&drawn(&area, &m, true));
    assert!(shown.iter().any(|t| t == "line 19"), "{shown:?}");
    assert!(!shown.iter().any(|t| t == "line 0"), "{shown:?}");
}

#[test]
fn the_caret_is_drawn_only_with_the_focus() {
    let m = wide(5);
    let area = TextArea::with_text("abc\ndef");
    assert_eq!(carets(&drawn(&area, &m, true)), 1);
    assert_eq!(carets(&drawn(&area, &m, false)), 0);
}

#[test]
fn the_placeholder_shows_only_while_the_field_is_empty() {
    let m = wide(5);
    let empty = TextArea::new();
    assert_eq!(texts(&drawn(&empty, &m, true)), ["Write something"]);
    let full = TextArea::with_text("x");
    assert_eq!(texts(&drawn(&full, &m, true)), ["x"]);
}

/// A selection that runs through a line ending shows it: a mark past the end
/// of the line, beside the selected text's own boxes.
#[test]
fn a_selected_line_ending_is_marked() {
    let m = wide(5);
    let mut area = TextArea::with_text("ab\ncd");
    area.edit_key(&ctrl(Key::A), &m);
    let fills = drawn(&area, &m, true)
        .into_iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
        .count();
    // "ab", the newline after it, and "cd".
    assert_eq!(fills, 3);
}

#[test]
fn drawing_is_clipped_to_the_box() {
    let m = wide(5);
    let area = TextArea::with_text("abc");
    let cmds = drawn(&area, &m, true);
    assert!(matches!(cmds.first(), Some(RenderCommand::PushClip { .. })));
    assert!(matches!(cmds.last(), Some(RenderCommand::PopClip)));
}

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;
use crate::render::FontWeightHint;

const SIZE: f32 = 13.0;

fn metrics(width: f32) -> Metrics {
    Metrics {
        width,
        size: SIZE,
        wrap: true,
    }
}

fn plain(text: &str) -> RichDoc {
    RichDoc::plain(text, Format::default())
}

fn w(text: &str) -> f32 {
    crate::text::measure(text, SIZE, FontWeightHint::Regular)
}

/// The lines' stretches.
fn stretches(lines: &[Line]) -> Vec<(usize, usize)> {
    lines.iter().map(|l| (l.start, l.end)).collect()
}

/// **A paragraph that fits is one line; a newline starts another**, and an
/// empty paragraph is a line of its own, as tall as text typed in it.
#[test]
fn paragraphs_are_lines() {
    let doc = plain("one\n\ntwo");
    let lines = lay_out(&doc, &metrics(1000.0));
    assert_eq!(stretches(&lines), [(0, 3), (4, 4), (5, 8)]);
    assert!(lines[1].height > 0.0, "the empty one has a height");
    assert_eq!(lines[1].y, lines[0].height, "stacked");
    assert_eq!(stretches(&lay_out(&plain(""), &metrics(100.0))), [(0, 0)]);
}

/// **A paragraph wraps between words, its spaces hanging on the line
/// before.**
#[test]
fn a_paragraph_wraps_between_words() {
    let doc = plain("aaa bbb ccc");
    let lines = lay_out(&doc, &metrics(w("aaa bbb") + 1.0));
    assert_eq!(stretches(&lines), [(0, 8), (8, 11)]);
    let unwrapped = lay_out(
        &doc,
        &Metrics {
            wrap: false,
            ..metrics(1.0)
        },
    );
    assert_eq!(stretches(&unwrapped), [(0, 11)], "not wrapped");
}

/// **A word in two formats is kept whole**: half of it bold does not make
/// two words a line can break between.
#[test]
fn a_word_in_two_formats_is_kept_whole() {
    let mut doc = plain("aaabbb ccc");
    doc.apply(0, 3, |f| f.bold = true);
    let width = crate::text::measure("aaa", SIZE, FontWeightHint::Bold) + w("bbb") + 1.0;
    let lines = lay_out(&doc, &metrics(width));
    assert_eq!(stretches(&lines), [(0, 7), (7, 10)]);
    assert_eq!(lines[0].pieces.len(), 2, "bold and plain, one line");
}

/// **A word wider than the box is cut where it stops fitting.**
#[test]
fn a_word_wider_than_the_box_is_cut() {
    let doc = plain("abcdefghij");
    let lines = lay_out(&doc, &metrics(w("abcd") + 0.5));
    assert!(lines.len() >= 2, "{lines:?}");
    assert_eq!(lines[0].start, 0);
    assert_eq!(lines.last().unwrap().end, 10);
    for pair in lines.windows(2) {
        assert_eq!(pair[0].end, pair[1].start, "tiled");
        assert!(pair[0].end > pair[0].start, "each takes something");
    }
}

/// **A bigger run makes its line taller**, and its baseline lower.
#[test]
fn a_bigger_run_makes_its_line_taller() {
    let mut doc = plain("small big\nsmall");
    doc.apply(6, 9, |f| f.size = Some(30.0));
    let lines = lay_out(&doc, &metrics(1000.0));
    assert!(lines[0].height > lines[1].height);
    assert!(lines[0].ascent > lines[1].ascent);
}

/// **Where a click lands and where a caret is drawn agree**, across the
/// runs of a line.
#[test]
fn a_click_and_a_caret_agree() {
    let mut doc = plain("hello world");
    doc.apply(0, 5, |f| f.bold = true);
    let lines = lay_out(&doc, &metrics(1000.0));
    let line = &lines[0];
    for at in [0, 3, 5, 6, 11] {
        let x = x_of(&doc, line, at, SIZE);
        let (back, _) = offset_at(&doc, line, x, SIZE);
        assert_eq!(back, at, "at {at}, x {x}");
    }
    assert!(x_of(&doc, line, 5, SIZE) > x_of(&doc, line, 4, SIZE));
    assert_eq!(offset_at(&doc, line, -5.0, SIZE), (0, false));
    assert_eq!(offset_at(&doc, line, 5000.0, SIZE), (11, true), "past it");
}

/// **A wrap's offset is the next line's, unless the caret is upstream** --
/// put at the end of the line above.
#[test]
fn a_wraps_offset_belongs_where_the_caret_was_put() {
    let doc = plain("aaa bbb ccc");
    let lines = lay_out(&doc, &metrics(w("aaa bbb") + 1.0));
    assert_eq!(line_of(&lines, 8, false), 1, "where typing carries on");
    assert_eq!(line_of(&lines, 8, true), 0, "the end of the line above");
    assert_eq!(line_of(&lines, 3, true), 0, "not at a wrap: its own line");
    assert_eq!(line_of(&lines, 11, false), 1);
    // A paragraph's end is not a wrap: no line above shares it.
    let two = lay_out(&plain("ab\ncd"), &metrics(1000.0));
    assert_eq!(line_of(&two, 3, true), 1);
    assert_eq!(line_of(&two, 2, false), 0);
}

// ---- Two writing directions ----

/// "Shalom", in Hebrew: a right-to-left word.
const SHALOM: &str = "\u{5e9}\u{5dc}\u{5d5}\u{5dd}";
/// "World", in Hebrew.
const OLAM: &str = "\u{5e2}\u{5d5}\u{5dc}\u{5dd}";

/// The pieces of `line` as (text, whether right to left), left to right on
/// the screen.
fn shown<'a>(doc: &'a RichDoc, line: &Line) -> Vec<(&'a str, bool)> {
    line.pieces
        .iter()
        .map(|p| (doc.text().get(p.start..p.end).unwrap(), p.rtl))
        .collect()
}

/// **A right-to-left word in a left-to-right line is a piece of its own,
/// running right to left**, between its neighbours, the pieces touching.
#[test]
fn a_right_to_left_word_is_a_piece_of_its_own() {
    let doc = plain(&format!("abc {SHALOM} def"));
    let lines = lay_out(&doc, &metrics(1000.0));
    let line = &lines[0];
    assert_eq!(
        shown(&doc, line),
        [("abc ", false), (SHALOM, true), (" def", false)]
    );
    for pair in line.pieces.windows(2) {
        assert!((pair[0].x + pair[0].width - pair[1].x).abs() < 0.01);
    }
    let hebrew = &line.pieces[1];
    // Each letter further left than the one before it, the word's end at
    // its left edge.
    let xs: Vec<f32> = (1..=4)
        .map(|k| x_of(&doc, line, hebrew.start + 2 * k, SIZE))
        .collect();
    assert!(xs.windows(2).all(|p| p[1] < p[0]), "{xs:?}");
    assert!(xs[0] < hebrew.x + hebrew.width, "{xs:?}");
    assert!((xs[3] - hebrew.x).abs() < 0.5, "{xs:?} from {}", hebrew.x);
}

/// **A right-to-left paragraph puts its first word at the right**, and a
/// left-to-right word inside it reads left to right in its place.
#[test]
fn a_right_to_left_paragraph_starts_at_the_right() {
    let doc = plain(&format!("{SHALOM} abc {OLAM}"));
    let line = &lay_out(&doc, &metrics(1000.0))[0];
    let texts: Vec<&str> = shown(&doc, line)
        .into_iter()
        .map(|(t, _)| t.trim())
        .collect();
    assert_eq!(texts.first(), Some(&OLAM), "the last word at the left");
    assert_eq!(texts.last(), Some(&SHALOM), "the first at the right");
    let abc = line
        .pieces
        .iter()
        .find(|p| doc.text().get(p.start..p.end) == Some("abc"))
        .expect("abc is a piece of its own");
    assert!(!abc.rtl);
}

/// **A piece is cut where the format changes as well as where the direction
/// does**: bold across a Hebrew word and on into English is two pieces.
#[test]
fn a_piece_is_cut_at_a_format_and_at_a_direction() {
    let mut doc = plain(&format!("{SHALOM}ab"));
    // Bold over the last two Hebrew letters and the "a".
    let from = SHALOM.len() - 4;
    doc.apply(from, SHALOM.len() + 1, |f| f.bold = true);
    let line = &lay_out(&doc, &metrics(1000.0))[0];
    let pieces: Vec<(usize, usize, bool, bool)> = line
        .pieces
        .iter()
        .map(|p| (p.start, p.end, p.rtl, p.format.bold))
        .collect();
    assert_eq!(pieces.len(), 4, "{pieces:?}");
    assert!(pieces.contains(&(0, from, true, false)));
    assert!(pieces.contains(&(from, SHALOM.len(), true, true)));
    assert!(pieces.contains(&(SHALOM.len(), SHALOM.len() + 1, false, true)));
    assert!(pieces.contains(&(SHALOM.len() + 1, SHALOM.len() + 2, false, false)));
}

/// **A click finds the place the caret is drawn**, in either direction: every
/// place on a mixed line, measured and clicked, comes back as itself.
#[test]
fn a_click_finds_where_the_caret_is_drawn() {
    let doc = plain(&format!("abc {SHALOM} def"));
    let line = &lay_out(&doc, &metrics(1000.0))[0];
    for (x, at, upstream) in caret_stops(&doc, line, SIZE) {
        assert!(
            (x_at(&doc, line, at, upstream, SIZE) - x).abs() < 0.01,
            "a stop is where its caret is drawn"
        );
        let (found, side) = offset_at(&doc, line, x, SIZE);
        let back = x_at(&doc, line, found, side, SIZE);
        assert!(
            (back - x).abs() < 0.5,
            "the place of {at} at {x} clicked gives {found} at {back}"
        );
    }
    // Off either end.
    assert_eq!(offset_at(&doc, line, -5.0, SIZE).0, 0);
    assert_eq!(offset_at(&doc, line, 5000.0, SIZE), (doc.len(), true));
}

/// **A line of one direction is laid out as it always was**: its pieces in
/// written order, none right to left.
#[test]
fn a_line_of_one_direction_is_unchanged() {
    let mut doc = plain("one two three");
    doc.apply(4, 7, |f| f.bold = true);
    let line = &lay_out(&doc, &metrics(1000.0))[0];
    assert!(line.is_ltr());
    let starts: Vec<usize> = line.pieces.iter().map(|p| p.start).collect();
    assert_eq!(starts, [0, 4, 7]);
    assert!((line.pieces[1].x - w("one ")).abs() < 0.01);
}

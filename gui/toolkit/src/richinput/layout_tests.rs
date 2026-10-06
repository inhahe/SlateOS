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

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;

const BOLD: Format = Format {
    bold: true,
    italic: false,
    underline: false,
    strike: false,
    color: None,
    size: None,
};

fn plain() -> Format {
    Format::default()
}

/// The runs as (start, end, bold) triples: what a test reads easily.
fn shape(doc: &RichDoc) -> Vec<(usize, usize, bool)> {
    doc.spans().map(|(s, e, f)| (s, e, f.bold)).collect()
}

/// **Runs tile the text**: one for a plain text, none for an empty one.
#[test]
fn runs_tile_the_text() {
    let doc = RichDoc::plain("hello", plain());
    assert_eq!(shape(&doc), [(0, 5, false)]);
    assert!(RichDoc::plain("", BOLD).runs().is_empty());
    assert_eq!(RichDoc::default().format_at(0), Format::default());
}

/// **Formatting a stretch splits the runs round it, and merges what is
/// alike again.**
#[test]
fn formatting_a_stretch_splits_and_merges() {
    let mut doc = RichDoc::plain("hello world", plain());
    assert!(doc.apply(6, 11, |f| f.bold = true));
    assert_eq!(shape(&doc), [(0, 6, false), (6, 11, true)]);
    assert!(doc.apply(2, 8, |f| f.bold = true));
    assert_eq!(shape(&doc), [(0, 2, false), (2, 11, true)], "merged");
    assert!(doc.apply(0, 11, |f| f.bold = false));
    assert_eq!(shape(&doc), [(0, 11, false)], "one run again");
    assert!(!doc.apply(0, 11, |f| f.bold = false), "no change");
    assert!(!doc.apply(3, 3, |f| f.bold = true), "an empty range");
}

/// **Text typed carries on in the format before it**, and at the start in
/// the first character's.
#[test]
fn typed_text_takes_the_format_before_it() {
    let mut doc = RichDoc::plain("ab", plain());
    doc.apply(1, 2, |f| f.bold = true);
    assert!(doc.format_before(2).bold, "after the bold b");
    assert!(!doc.format_before(1).bold, "after the plain a");
    assert!(!doc.format_before(0).bold, "the first character's");
    doc.insert(2, "cd", doc.format_before(2));
    assert_eq!(shape(&doc), [(0, 1, false), (1, 4, true)]);
}

/// **A replacement keeps the formatting either side and brings its own.**
#[test]
fn a_replacement_brings_its_own_formatting() {
    let mut doc = RichDoc::plain("one two three", plain());
    let removed = doc.replace(4, 7, &RichDoc::plain("2", BOLD));
    assert_eq!(doc.text(), "one 2 three");
    assert_eq!(shape(&doc), [(0, 4, false), (4, 5, true), (5, 11, false)]);
    assert_eq!(removed.text(), "two");
    // Putting back what was removed restores it exactly.
    doc.replace(4, 5, &removed);
    assert_eq!(doc, RichDoc::plain("one two three", plain()));
}

/// **A slice is its stretch of the text, formatting and all**, its bounds
/// held to character boundaries.
#[test]
fn a_slice_keeps_its_formatting() {
    let mut doc = RichDoc::plain("abcdef", plain());
    doc.apply(2, 4, |f| f.bold = true);
    let mid = doc.slice(1, 5);
    assert_eq!(mid.text(), "bcde");
    assert_eq!(shape(&mid), [(0, 1, false), (1, 3, true), (3, 4, false)]);
    // A bound inside a character goes back to its start.
    let wide = RichDoc::plain("a\u{e9}b", plain());
    assert_eq!(wide.slice(0, 2).text(), "a", "inside the e-acute");
    assert_eq!(wide.slice(5, 9).text(), "", "past the end");
}

/// **Whether a stretch is all of a kind** -- what a toolbar's button shows.
#[test]
fn a_stretch_is_all_of_a_kind_or_not() {
    let mut doc = RichDoc::plain("abcd", plain());
    doc.apply(1, 3, |f| f.bold = true);
    assert!(doc.all(1, 3, |f| f.bold));
    assert!(!doc.all(0, 3, |f| f.bold));
    assert!(doc.all(3, 1, |f| f.bold), "either way round");
    assert!(doc.all(3, 3, |f| f.bold), "empty: the format before");
    assert!(!doc.all(1, 1, |f| f.bold));
}

/// **HTML carries each run's format and escapes the text**, a line break
/// as `<br>`.
#[test]
fn html_carries_the_formats_and_escapes_the_text() {
    let mut doc = RichDoc::plain("a<b> & \"c\"\nred big", plain());
    doc.apply(0, 1, |f| f.bold = true);
    doc.apply(0, 1, |f| f.underline = true);
    let red_at = doc.text().find("red").unwrap();
    doc.apply(red_at, red_at + 3, |f| {
        f.color = Some(crate::color::Color::rgba(255, 0, 0, 255));
    });
    doc.apply(red_at + 4, red_at + 7, |f| f.size = Some(20.0));
    assert_eq!(
        doc.to_html(),
        "<b><u>a</u></b>&lt;b&gt; &amp; &quot;c&quot;<br>\
         <span style=\"color:#ff0000\">red</span> \
         <span style=\"font-size:20px\">big</span>"
    );
    assert_eq!(RichDoc::default().to_html(), "");
}

/// **Appending joins the runs where they meet alike.**
#[test]
fn appending_joins_alike_runs() {
    let mut doc = RichDoc::plain("ab", BOLD);
    doc.append(&RichDoc::plain("cd", BOLD));
    doc.append(&RichDoc::plain("ef", plain()));
    assert_eq!(shape(&doc), [(0, 4, true), (4, 6, false)]);
    doc.append(&RichDoc::default());
    assert_eq!(doc.len(), 6);
}

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::super::SvgDocument;
use super::*;

/// A drawing `side` units square, rendered `side` pixels square, as RGBA.
fn draw(side: u32, body: &str) -> Vec<[u8; 4]> {
    let svg =
        format!(r#"<svg viewBox="0 0 {side} {side}" width="{side}" height="{side}">{body}</svg>"#);
    SvgDocument::parse(&svg)
        .unwrap()
        .render(side, side)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

/// Whether the toolkit's UI face here has outlines: on a machine with no
/// installed font it is the built-in bitmap face, which has none, and text
/// is drawn as nothing -- which a test of where ink lands cannot test.
fn has_outlines() -> bool {
    let (outline, advance) =
        crate::text::outlined("H", 32.0, FontWeightHint::Regular, FontFamily::Ui);
    if outline.is_empty() {
        eprintln!("no outline face installed here: the ink tests have nothing to see");
        return false;
    }
    assert!(advance > 0.0);
    true
}

/// The inked pixels' bounds, `(left, top, right, bottom)`, of those with
/// some alpha; `None` for none.
fn ink(image: &[[u8; 4]], side: usize) -> Option<(usize, usize, usize, usize)> {
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for (i, px) in image.iter().enumerate() {
        if px[3] > 64 {
            let (x, y) = (i % side, i / side);
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
            });
        }
    }
    bounds
}

/// **White space is SVG's default**: line breaks gone, tabs made spaces,
/// a run of spaces made one -- across a `<tspan>`'s edges too -- and none
/// at the start or the end; `preserve` keeps every space.
#[test]
fn white_space_collapses_across_runs() {
    let doc = r"<text>  Hello	  <tspan>
         world </tspan>  </text>";
    let elem = &super::super::parse_xml(doc).unwrap()[0];
    assert_eq!(collapsed(elem, false), ["Hello ", "world", ""]);
    assert_eq!(collapsed(elem, true)[0], "  Hello   ");
}

/// **No more than [`MAX_CHARS`] characters of one text are laid out.**
#[test]
fn a_texts_characters_are_bounded() {
    let doc = format!("<text>{}</text>", "ab".repeat(MAX_CHARS));
    let elem = &super::super::parse_xml(&doc).unwrap()[0];
    let kept: usize = collapsed(elem, false)
        .iter()
        .map(|p| p.chars().count())
        .sum();
    assert_eq!(kept, MAX_CHARS);
}

/// **A font size is a number, `px`, `pt`, `em`, `%` or a keyword**, and
/// what is none of these, or not above nought, leaves the inherited one; a
/// weight is bold from 600 up.
#[test]
fn font_sizes_and_weights_read_as_css_reads_them() {
    for (value, want) in [
        ("12", Some(12.0)),
        ("12px", Some(12.0)),
        (" 12pt ", Some(16.0)),
        ("2em", Some(20.0)),
        ("150%", Some(15.0)),
        ("large", Some(18.0)),
        ("larger", Some(12.0)),
        ("1e9", Some(MAX_FONT_SIZE)),
        ("0", None),
        ("-3", None),
        ("big", None),
        ("NaN", None),
    ] {
        assert_eq!(font_size(value, 10.0), want, "{value:?}");
    }
    assert!(bold("bold", false) && bold("700", false) && bold("600", false));
    assert!(!bold("500", true) && !bold("normal", true));
    assert!(bold("heavy-ish", true), "what is not a weight inherits");
}

/// **Font properties are inherited from any element down** -- a group's
/// size reaches the text in it -- and a `<tspan>` changes them for itself.
#[test]
fn the_font_is_inherited_and_a_tspan_changes_it() {
    let doc = r#"<g font-size="20" font-weight="bold" text-anchor="middle"><text font-size="2em">A</text></g>"#;
    let g = &super::super::parse_xml(doc).unwrap()[0];
    let group = font_of(g, TextFont::DEFAULT);
    assert_eq!(
        (group.size, group.bold, group.anchor),
        (20.0, true, Anchor::Middle)
    );
    let text = font_of(&g.children[0], group);
    assert_eq!(
        (text.size, text.bold, text.anchor),
        (40.0, true, Anchor::Middle)
    );
}

/// **A `<text>` is drawn as its glyphs' shapes, in its fill, where its `x`
/// and `y` put it**: ink above the baseline, starting at `x`, in the colour
/// asked for.
#[test]
fn text_is_drawn_where_its_position_puts_it() {
    if !has_outlines() {
        return;
    }
    let image = draw(
        64,
        r##"<text x="10" y="48" font-size="40" fill="#ff0000">H</text>"##,
    );
    let (left, top, right, bottom) = ink(&image, 64).expect("the H is drawn");
    assert!((8..=16).contains(&left), "starts at x: {left}");
    assert!(
        bottom <= 48 && bottom >= 44,
        "sits on the baseline: {bottom}"
    );
    assert!(top < 30, "a capital rises well above it: {top}");
    assert!(right > left + 10, "and has width");
    let red = image
        .iter()
        .filter(|p| p[3] > 200)
        .all(|p| p[0] > 200 && p[1] < 40 && p[2] < 40);
    assert!(red, "in its fill");
    assert_eq!(
        ink(
            &draw(64, r#"<text x="10" y="48" font-size="40"></text>"#),
            64
        ),
        None
    );
}

/// **`text-anchor` places a chunk by its width**: the middle of the ink at
/// `x` for `middle`, its end at `x` for `end`.
#[test]
fn an_anchor_places_the_chunk_by_its_width() {
    if !has_outlines() {
        return;
    }
    let at = |anchor: &str| {
        let body =
            format!(r#"<text x="64" y="80" font-size="40" text-anchor="{anchor}">HH</text>"#);
        ink(&draw(128, &body), 128).expect("drawn")
    };
    let (start_l, _, _, _) = at("start");
    assert!((62..=70).contains(&start_l), "start: {start_l}");
    let (mid_l, _, mid_r, _) = at("middle");
    let centre = usize::midpoint(mid_l, mid_r);
    assert!(
        (60..=68).contains(&centre),
        "middle: centred on x, {centre}"
    );
    let (_, _, end_r, _) = at("end");
    assert!((58..=66).contains(&end_r), "end: ends at x, {end_r}");
}

/// **A `<tspan>` carries its own style and font, and continues the pen**:
/// its letters follow the text's, in its own fill and size.
#[test]
fn a_tspan_continues_the_line_in_its_own_style() {
    if !has_outlines() {
        return;
    }
    let image = draw(
        128,
        r##"<text x="4" y="90" font-size="40" fill="#ff0000">H<tspan fill="#0000ff" font-size="60">H</tspan></text>"##,
    );
    let reds: Vec<usize> = (0..image.len())
        .filter(|&i| image[i][3] > 200 && image[i][0] > 200 && image[i][2] < 40)
        .map(|i| i % 128)
        .collect();
    let blues: Vec<usize> = (0..image.len())
        .filter(|&i| image[i][3] > 200 && image[i][2] > 200 && image[i][0] < 40)
        .map(|i| i % 128)
        .collect();
    assert!(!reds.is_empty() && !blues.is_empty(), "both runs are drawn");
    assert!(
        reds.iter().max() < blues.iter().min(),
        "the tspan's letter follows the text's"
    );
    let tall = |xs: &[usize]| {
        (0..image.len())
            .filter(|&i| xs.contains(&(i % 128)) && image[i][3] > 200)
            .map(|i| i / 128)
            .min()
            .unwrap()
    };
    assert!(tall(&blues) < tall(&reds), "the larger letter rises higher");
}

/// **Text is stroked as any shape is**: with no fill and a stroke, the
/// letters are drawn as outlines.
#[test]
fn text_takes_a_stroke() {
    if !has_outlines() {
        return;
    }
    let image = draw(
        64,
        r##"<text x="8" y="50" font-size="48" fill="none" stroke="#000000" stroke-width="2">O</text>"##,
    );
    let (left, top, right, bottom) = ink(&image, 64).expect("the outline is drawn");
    let (cx, cy) = (usize::midpoint(left, right), usize::midpoint(top, bottom));
    assert_eq!(image[cy * 64 + cx][3], 0, "the O's middle is not filled");
}

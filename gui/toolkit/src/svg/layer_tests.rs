//! Tests for layers: an element whose opacity is of the whole -- a group,
//! a `<use>`, a shape with a stroke over its fill -- drawn whole and then
//! faded, so that what it overlaps of itself does not show through.
//!
//! Every drawing is 20 by 20 user units on 20 by 20 pixels.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::{SvgDocument, SvgRenderer};

fn render(body: &str) -> Vec<[u8; 4]> {
    let svg = format!(r#"<svg viewBox="0 0 20 20" width="20" height="20">{body}</svg>"#);
    let doc = SvgDocument::parse(&svg).unwrap();
    doc.render(20, 20)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn at(image: &[[u8; 4]], x: usize, y: usize) -> [u8; 4] {
    image[y * 20 + x]
}

/// Two opaque red squares overlapping at 8..12.
const TWO_SQUARES: &str = r#"<rect x="2" y="2" width="10" height="10" fill="red"/>
                             <rect x="8" y="8" width="10" height="10" fill="red"/>"#;

/// **A faded group is faded as a whole**: where its two opaque squares
/// overlap, it is as see-through as where one is alone -- not darker, as
/// fading each square would make it.
#[test]
fn a_faded_group_does_not_show_its_overlap() {
    let image = render(&format!(r#"<g opacity="0.5">{TWO_SQUARES}</g>"#));
    let alone = at(&image, 4, 4);
    let overlap = at(&image, 10, 10);
    assert!(alone[3].abs_diff(128) <= 1, "{alone:?}");
    assert_eq!(overlap, alone);
}

/// **A stroke over its fill does not let the fill through**: in the inner
/// half of the stroke, a faded shape is the stroke's colour, faded once.
#[test]
fn a_faded_stroke_hides_its_fill() {
    let image = render(
        r#"<rect x="4" y="4" width="12" height="12" fill="red" stroke="blue" stroke-width="4" opacity="0.5"/>"#,
    );
    // x = 5 is inside the fill and under the stroke's inner half.
    let px = at(&image, 5, 10);
    assert_eq!((px[0], px[2]), (0, 255), "{px:?}");
    assert!(px[3].abs_diff(128) <= 1, "{px:?}");
    let fill = at(&image, 10, 10);
    assert_eq!((fill[0], fill[2]), (255, 0));
}

/// **A shape with one paint fades as it always did**, with no layer: the
/// two are the same.
#[test]
fn one_paint_fades_alike() {
    let image = render(r#"<rect x="4" y="4" width="12" height="12" fill="red" opacity="0.5"/>"#);
    assert!(at(&image, 10, 10)[3].abs_diff(128) <= 1);
    assert_eq!(at(&image, 2, 2), [0, 0, 0, 0]);
}

/// **Opacities nest by multiplying**, each layer fading what it holds.
#[test]
fn nested_opacities_multiply() {
    let image = render(&format!(
        r#"<g opacity="0.5"><g opacity="0.5">{TWO_SQUARES}</g></g>"#
    ));
    let alone = at(&image, 4, 4);
    assert!(alone[3].abs_diff(64) <= 1, "{alone:?}");
    assert_eq!(at(&image, 10, 10), alone);
}

/// **Nothing of an element at no opacity is drawn**, and an opacity read
/// as a percentage, past one, or not a number reads as SVG says.
#[test]
fn opacity_values_read_as_css_does() {
    let none = render(&format!(r#"<g opacity="0">{TWO_SQUARES}</g>"#));
    assert!(none.iter().all(|px| px[3] == 0));
    let percent = render(&format!(r#"<g opacity="50%">{TWO_SQUARES}</g>"#));
    assert!(at(&percent, 4, 4)[3].abs_diff(128) <= 1);
    for whole in ["1.5", "NaN", "lots"] {
        let image = render(&format!(r#"<g opacity="{whole}">{TWO_SQUARES}</g>"#));
        assert_eq!(at(&image, 10, 10), [255, 0, 0, 255], "{whole}");
    }
}

/// **A faded group is still cut by its clip.**
#[test]
fn a_faded_group_keeps_its_clip() {
    let image = render(&format!(
        r#"<clipPath id="c"><rect x="0" y="0" width="10" height="20"/></clipPath>
           <g opacity="0.5" clip-path="url(#c)">{TWO_SQUARES}</g>"#
    ));
    assert!(at(&image, 9, 9)[3].abs_diff(128) <= 1);
    assert_eq!(at(&image, 11, 11), [0, 0, 0, 0]);
}

/// **A faded `<use>` fades what it shows as a whole.**
#[test]
fn a_faded_use_is_faded_whole() {
    let image = render(&format!(
        r##"<defs><g id="two">{TWO_SQUARES}</g></defs><use href="#two" opacity="0.5"/>"##
    ));
    assert!(at(&image, 4, 4)[3].abs_diff(128) <= 1);
    assert_eq!(at(&image, 10, 10), at(&image, 4, 4));
}

/// **A faded group's strokes are inside its layer**: a thick stroke well
/// past the shapes' outlines is drawn in full, not cut where the outlines
/// end.
#[test]
fn a_layer_holds_its_strokes() {
    let image = render(
        r#"<g opacity="0.5"><rect x="8" y="8" width="4" height="4" fill="none" stroke="red" stroke-width="8"/></g>"#,
    );
    // The stroke reaches 4 past the outline: 4..16.
    assert!(at(&image, 4, 10)[3] > 100, "{:?}", at(&image, 4, 10));
    assert!(at(&image, 15, 10)[3] > 100);
    assert_eq!(at(&image, 2, 10), [0, 0, 0, 0]);
}

/// **Past the budget for scratch surfaces, a faded group fades each part
/// instead**: drawn, with its overlap showing, rather than lost.
#[test]
fn past_the_budget_each_part_fades() {
    let svg = format!(
        r#"<svg viewBox="0 0 20 20" width="20" height="20"><g opacity="0.5">{TWO_SQUARES}</g></svg>"#
    );
    let doc = SvgDocument::parse(&svg).unwrap();
    let mut renderer = SvgRenderer::new(20, 20, &doc);
    renderer.scratch_budget = 0;
    let buffer = doc.draw(renderer);
    let alpha = |x: usize, y: usize| buffer[(y * 20 + x) * 4 + 3];
    assert!(alpha(4, 4).abs_diff(128) <= 1);
    assert!(alpha(10, 10) > 180, "the overlap shows: {}", alpha(10, 10));
}

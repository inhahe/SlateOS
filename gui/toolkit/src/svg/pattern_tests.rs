//! Tests for patterns: documents drawn, each pixel checked against the tile
//! it should repeat.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::super::SvgDocument;
use super::Tile;

/// The pixels of `svg` drawn `size` by `size`, row by row.
fn pixels(svg: &str, size: u32) -> Vec<[u8; 4]> {
    let buffer = SvgDocument::parse(svg).unwrap().render(size, size);
    buffer
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

/// The alphas of `svg` drawn `size` by `size`, row by row.
fn alphas(svg: &str, size: u32) -> Vec<u8> {
    pixels(svg, size).iter().map(|p| p[3]).collect()
}

/// A drawing 8 units square, painted all over by `#p`, defined by `pattern`.
fn painted(pattern: &str) -> String {
    format!(r#"<svg viewBox="0 0 8 8">{pattern}<rect width="8" height="8" fill="url(#p)"/></svg>"#)
}

/// A checkerboard's tile: its upper left and lower right quarters black.
const CHECKS: &str = r##"<rect width="2" height="2" fill="#000"/><rect x="2" y="2" width="2" height="2" fill="#000"/>"##;

/// The checkerboard as the first two rows of an 8 by 8 drawing read: two
/// black, two clear, and over again; then the other way about.
const ROW_0: [u8; 8] = [255, 255, 0, 0, 255, 255, 0, 0];
const ROW_2: [u8; 8] = [0, 0, 255, 255, 0, 0, 255, 255];

/// **A pattern repeats its tile**, across and down, from the user space's
/// origin.
#[test]
fn a_pattern_repeats_its_tile() {
    let a = alphas(
        &painted(&format!(
            r#"<pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4">{CHECKS}</pattern>"#
        )),
        8,
    );
    assert_eq!(a[0..8], ROW_0);
    assert_eq!(a[16..24], ROW_2);
    assert_eq!(a[32..40], ROW_0);
}

/// **A tile is by default measured in the shape's box**: half of it is half
/// the box, written as a fraction or a percentage.
#[test]
fn a_tile_is_measured_in_the_box_by_default() {
    for size in [r#"width="0.5" height="0.5""#, r#"width="50%" height="50%""#] {
        let a = alphas(
            &painted(&format!(r#"<pattern id="p" {size}>{CHECKS}</pattern>"#)),
            8,
        );
        assert_eq!(a[0..8], ROW_0, "{size}");
        assert_eq!(a[16..24], ROW_2, "{size}");
    }
}

/// **The content may be measured in the box, or fitted from a view box**,
/// either way filling the tile as the checkerboard does.
#[test]
fn the_content_may_be_measured_in_the_box_or_fitted() {
    let boxed = painted(
        r##"<pattern id="p" width="0.5" height="0.5" patternContentUnits="objectBoundingBox">
<rect width="0.25" height="0.25" fill="#000"/><rect x="0.25" y="0.25" width="0.25" height="0.25" fill="#000"/></pattern>"##,
    );
    let a = alphas(&boxed, 8);
    assert_eq!(a[0..8], ROW_0);
    assert_eq!(a[16..24], ROW_2);
    let fitted = painted(
        r##"<pattern id="p" width="0.5" height="0.5" viewBox="0 0 2 2">
<rect width="1" height="1" fill="#000"/><rect x="1" y="1" width="1" height="1" fill="#000"/></pattern>"##,
    );
    let a = alphas(&fitted, 8);
    assert_eq!(a[0..8], ROW_0);
    assert_eq!(a[16..24], ROW_2);
}

/// **A pattern's transform carries the tiling**: moved two units across,
/// the checkerboard's columns change places.
#[test]
fn a_patterns_transform_carries_the_tiling() {
    let a = alphas(
        &painted(&format!(
            r#"<pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4" patternTransform="translate(2 0)">{CHECKS}</pattern>"#
        )),
        8,
    );
    assert_eq!(a[0..8], ROW_2);
    // Turned a quarter, a checkerboard is a checkerboard with its rows
    // swapped: what was across is down.
    let turned = alphas(
        &painted(&format!(
            r#"<pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4" patternTransform="rotate(90)">{CHECKS}</pattern>"#
        )),
        8,
    );
    assert_eq!(turned[0..8], ROW_2);
}

/// **A pattern takes what it does not say from the one it names**: its
/// tile and content, and anything it says of its own over that.
#[test]
fn a_pattern_takes_what_it_does_not_say_from_the_one_it_names() {
    let base = format!(
        r#"<pattern id="base" patternUnits="userSpaceOnUse" width="4" height="4">{CHECKS}</pattern>"#
    );
    let named = painted(&format!(r##"{base}<pattern id="p" href="#base"/>"##));
    assert_eq!(alphas(&named, 8)[0..8], ROW_0);
    // Its own width, the content of the one it names: two units black, six
    // clear.
    let wider = painted(&format!(
        r##"{base}<pattern id="p" xlink:href="#base" width="8"/>"##
    ));
    assert_eq!(alphas(&wider, 8)[0..8], [255, 255, 0, 0, 0, 0, 0, 0]);
    // A loop of names ends.
    let looped =
        painted(r##"<pattern id="p" href="#q"/><pattern id="q" href="#p" width="1" height="1"/>"##);
    assert!(alphas(&looped, 8).iter().all(|&a| a == 0));
}

/// **A pattern that cannot paint paints nothing**: a tile with no area, one
/// measured against a shape with no area; and a missing one gives way to the
/// colour written after it.
#[test]
fn a_pattern_that_cannot_paint_paints_nothing() {
    let flat = painted(&format!(
        r#"<pattern id="p" patternUnits="userSpaceOnUse" width="0" height="4">{CHECKS}</pattern>"#
    ));
    assert!(alphas(&flat, 8).iter().all(|&a| a == 0));
    let boxless = format!(
        r#"<svg viewBox="0 0 8 8"><pattern id="p" width="0.5" height="0.5">{CHECKS}</pattern>
<line x1="0" y1="4" x2="8" y2="4" stroke="url(#p)" stroke-width="2"/></svg>"#
    );
    assert!(alphas(&boxless, 8).iter().all(|&a| a == 0));
    let missing =
        r#"<svg viewBox="0 0 8 8"><rect width="8" height="8" fill="url(#nope) #00ff00"/></svg>"#;
    assert_eq!(pixels(missing, 8)[0], [0, 255, 0, 255]);
}

/// **A pattern paints a stroke as it does a fill**, and at the opacity the
/// paint is drawn at.
#[test]
fn a_pattern_paints_a_stroke_and_fades() {
    let stroked = format!(
        r#"<svg viewBox="0 0 8 8"><pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4">{CHECKS}</pattern>
<rect x="1" y="1" width="6" height="6" fill="none" stroke="url(#p)" stroke-width="2"/></svg>"#
    );
    let a = alphas(&stroked, 8);
    // The stroke's top edge runs along row 0: the checkerboard's row.
    assert_eq!(a[0..8], ROW_0);
    // Inside the stroke, nothing.
    assert_eq!(a[3 * 8 + 3], 0);
    let faded = painted(&format!(
        r#"<pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4">{CHECKS}</pattern>"#
    ))
    .replace(r#"fill="url(#p)""#, r#"fill="url(#p)" fill-opacity="0.5""#);
    let a = alphas(&faded, 8);
    assert!(a[0].abs_diff(128) <= 1, "{}", a[0]);
    assert_eq!(a[2], 0);
}

/// **What a pattern element says of its style its content takes**, as any
/// element's content does -- and so with a mask.
#[test]
fn a_patterns_content_takes_its_style() {
    let svg = painted(
        r##"<pattern id="p" patternUnits="userSpaceOnUse" width="8" height="8" fill="#00ff00">
<rect width="8" height="8"/></pattern>"##,
    );
    assert_eq!(pixels(&svg, 8)[0], [0, 255, 0, 255]);
    let mask = r#"<svg viewBox="0 0 8 8"><mask id="m" fill="white"><rect width="8" height="8"/></mask>
<rect width="8" height="8" fill="red" mask="url(#m)"/></svg>"#;
    assert_eq!(alphas(mask, 8)[0], 255);
}

/// **A pattern painted with itself ends**: its content's fill names it, and
/// the drawing still finishes, its own square painted.
#[test]
fn a_pattern_painted_with_itself_ends() {
    let svg = painted(
        r##"<pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4">
<rect width="2" height="2" fill="#000"/><rect x="2" y="2" width="2" height="2" fill="url(#p)"/></pattern>"##,
    );
    let a = alphas(&svg, 8);
    assert_eq!(a[0], 255);
}

/// **A tile is read between its pixels**: at a pixel's centre, exactly that
/// pixel; half way between two, their mix; past its edge, its other edge.
#[test]
fn a_tile_is_read_between_its_pixels() {
    let tile = Tile {
        pixels: vec![255, 0, 0, 255, 0, 0, 255, 255],
        width: 2,
        height: 1,
    };
    assert_eq!(
        tile.color_at(0.5, 0.5),
        crate::color::Color::rgba(255, 0, 0, 255)
    );
    assert_eq!(
        tile.color_at(1.5, 0.5),
        crate::color::Color::rgba(0, 0, 255, 255)
    );
    let mid = tile.color_at(1.0, 0.5);
    assert!(
        mid.r.abs_diff(128) <= 1 && mid.b.abs_diff(128) <= 1,
        "{mid:?}"
    );
    // Round the edge: past the right is the left.
    assert_eq!(
        tile.color_at(2.5, 0.5),
        crate::color::Color::rgba(255, 0, 0, 255)
    );
    assert_eq!(
        tile.color_at(-0.5, 0.5),
        crate::color::Color::rgba(0, 0, 255, 255)
    );
    // A clear pixel's colour does not tint its neighbour.
    let half_clear = Tile {
        pixels: vec![255, 0, 0, 255, 0, 255, 0, 0],
        width: 2,
        height: 1,
    };
    let edge = half_clear.color_at(1.0, 0.5);
    assert_eq!((edge.r, edge.g), (255, 0));
    assert!(edge.a.abs_diff(128) <= 1);
    assert_eq!(
        tile.color_at(f32::NAN, 0.5),
        crate::color::Color::rgba(0, 0, 0, 0)
    );
}

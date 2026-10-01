//! Tests for gradients: written as SVG and drawn, each against a colour
//! worked out by hand from SVG's rules.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::super::SvgDocument;

/// The pixel at `(x, y)` of `svg` drawn `w` by `h`: `[r, g, b, a]`.
fn px(svg: &str, w: u32, h: u32, x: u32, y: u32) -> [u8; 4] {
    let doc = SvgDocument::parse(svg).expect("parses");
    let buffer = doc.render(w, h);
    let at = ((y * w + x) * 4) as usize;
    [buffer[at], buffer[at + 1], buffer[at + 2], buffer[at + 3]]
}

/// Whether `got` is `want` give or take `slack` in every channel.
fn near(got: [u8; 4], want: [u8; 4], slack: u8) -> bool {
    got.iter().zip(want).all(|(&g, w)| g.abs_diff(w) <= slack)
}

/// A 100 by 10 drawing with `defs` and a rect filling it, painted `fill`.
fn strip(defs: &str, fill: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 10" width="100" height="10">
<defs>{defs}</defs><rect width="100" height="10" fill="{fill}"/></svg>"#
    )
}

const RED_TO_BLUE: &str = r##"<linearGradient id="g">
<stop offset="0" stop-color="red"/><stop offset="1" stop-color="#0000ff"/></linearGradient>"##;

/// **A linear gradient runs across the shape's box**, its colour at a pixel
/// the stops' mix at that pixel's centre: red at the left, blue at the
/// right, half of each in the middle.
#[test]
fn a_linear_gradient_runs_across_the_box() {
    let svg = strip(RED_TO_BLUE, "url(#g)");
    let at = |x| px(&svg, 100, 10, x, 5);
    assert!(near(at(0), [254, 0, 1, 255], 2), "{:?}", at(0));
    assert!(near(at(99), [1, 0, 254, 255], 2), "{:?}", at(99));
    // The 50th pixel's centre is 50.5% of the way.
    assert!(near(at(50), [126, 0, 129, 255], 2), "{:?}", at(50));
}

/// **A gradient may be defined after what paints with it**, and outside
/// any `<defs>`.
#[test]
fn a_gradient_may_come_after_its_use() {
    let svg = format!(
        r#"<svg viewBox="0 0 100 10" width="100" height="10">
<rect width="100" height="10" fill="url(#g)"/>{RED_TO_BLUE}</svg>"#
    );
    assert!(near(px(&svg, 100, 10, 0, 5), [254, 0, 1, 255], 2));
}

/// **In user space the numbers are user units**, and past the ends a padded
/// gradient keeps its end colours.
#[test]
fn a_user_space_gradient_is_measured_in_user_units() {
    let defs = r#"<linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" x2="50">
<stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>"#;
    let svg = strip(defs, "url(#g)");
    assert!(near(px(&svg, 100, 10, 24, 5), [128, 0, 127, 255], 3));
    // Past 50, the last stop's.
    assert_eq!(px(&svg, 100, 10, 75, 5), [0, 0, 255, 255]);
    // Percentages are of the viewport.
    let percent = strip(
        r#"<linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0%" x2="50%">
<stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>"#,
        "url(#g)",
    );
    assert_eq!(px(&percent, 100, 10, 75, 5), [0, 0, 255, 255]);
}

/// **Past its ends a gradient repeats or reflects as its spread says.**
#[test]
fn a_gradient_repeats_or_reflects_past_its_ends() {
    let make = |spread: &str| {
        strip(
            &format!(
                r#"<linearGradient id="g" x2="0.25" spreadMethod="{spread}">
<stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>"#
            ),
            "url(#g)",
        )
    };
    // Pixel 29's centre is 29.5% across: 1.18 of the gradient's length.
    let pad = px(&make("pad"), 100, 10, 29, 5);
    let repeat = px(&make("repeat"), 100, 10, 29, 5);
    let reflect = px(&make("reflect"), 100, 10, 29, 5);
    assert_eq!(pad, [0, 0, 255, 255]);
    // 0.18 of the way again: mostly red.
    assert!(near(repeat, [209, 0, 46, 255], 3), "{repeat:?}");
    // 0.18 back from the end: mostly blue.
    assert!(near(reflect, [46, 0, 209, 255], 3), "{reflect:?}");
}

/// **A radial gradient runs from its centre to its circle**, and a padded
/// one keeps the last stop past it.
#[test]
fn a_radial_gradient_runs_from_its_centre_to_its_circle() {
    let svg = r#"<svg viewBox="0 0 100 100" width="100" height="100">
<radialGradient id="g"><stop offset="0" stop-color="white"/><stop offset="1" stop-color="black"/></radialGradient>
<rect width="100" height="100" fill="url(#g)"/></svg>"#;
    assert!(near(px(svg, 100, 100, 50, 50), [252, 252, 252, 255], 4));
    // 25 from the centre, half of the radius 50: half way.
    let half = px(svg, 100, 100, 75, 50);
    assert!(near(half, [126, 126, 126, 255], 4), "{half:?}");
    // The corner is past the circle.
    assert_eq!(px(svg, 100, 100, 0, 0), [0, 0, 0, 255]);
}

/// **A focal point starts the gradient off-centre**, and one outside the
/// circle is brought inside it rather than breaking it.
#[test]
fn a_focal_point_moves_the_start() {
    let make = |fx: &str| {
        format!(
            r#"<svg viewBox="0 0 100 100" width="100" height="100">
<radialGradient id="g" fx="{fx}"><stop offset="0" stop-color="white"/><stop offset="1" stop-color="black"/></radialGradient>
<rect width="100" height="100" fill="url(#g)"/></svg>"#
        )
    };
    let focal = make("0.25");
    // At the focal point, the first stop.
    assert!(near(px(&focal, 100, 100, 25, 50), [250, 250, 250, 255], 6));
    // The centre is no longer the brightest point.
    assert!(px(&focal, 100, 100, 50, 50)[0] < 200);
    // A focal point outside the circle is brought to its edge: white starts
    // at the left, and it still runs to black at the right.
    let outside = make("-2");
    let start = px(&outside, 100, 100, 0, 50);
    assert!(near(start, [252, 252, 252, 255], 4), "{start:?}");
    let end = px(&outside, 100, 100, 99, 50);
    assert!(near(end, [1, 1, 1, 255], 2), "{end:?}");
}

/// **A gradient that names another takes its stops, and anything it does
/// not set itself**: the geometry from its own kind only.
#[test]
fn a_gradient_takes_what_it_names() {
    let defs = r##"<linearGradient id="base" x2="0.5" spreadMethod="repeat">
<stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>
<linearGradient id="g" xlink:href="#base" x2="1"/>"##;
    let svg = strip(defs, "url(#g)");
    // Its own x2, the stops and the spread from `base`.
    assert!(near(px(&svg, 100, 10, 50, 5), [126, 0, 129, 255], 2));
    // SVG 2's plain `href` too; and a chain that loops keeps what it has.
    let looped = strip(
        r##"<linearGradient id="a" href="#b"><stop stop-color="red"/></linearGradient>
<linearGradient id="b" href="#a"/>"##,
        "url(#b)",
    );
    assert_eq!(px(&looped, 100, 10, 50, 5), [255, 0, 0, 255]);
}

/// **Colours between stops are mixed channel by channel, alpha with the
/// rest**, as SVG says -- "SVG does not calculate gradients in
/// pre-multiplied space": red fading to a transparent blue passes through
/// purple, and to a stop with only `stop-opacity="0"` -- transparent black --
/// darkens as it fades. Only a fade to the same colour stays that colour.
#[test]
fn a_fade_mixes_colour_and_alpha_alike() {
    let fade_to = |stop: &str| {
        let defs = format!(
            r#"<linearGradient id="g"><stop offset="0" stop-color="red"/>
<stop offset="1" {stop}/></linearGradient>"#
        );
        // The 50th pixel's centre: 50.5% of the way.
        px(&strip(&defs, "url(#g)"), 100, 10, 50, 5)
    };
    let to_blue = fade_to(r#"stop-color="blue" stop-opacity="0""#);
    assert!(near(to_blue, [126, 0, 129, 126], 2), "{to_blue:?}");
    let to_black = fade_to(r#"stop-opacity="0""#);
    assert!(near(to_black, [126, 0, 0, 126], 2), "{to_black:?}");
    let to_red = fade_to(r#"stop-color="red" stop-opacity="0""#);
    assert!(near(to_red, [255, 0, 0, 126], 2), "{to_red:?}");
    // `stop-opacity` and `stop-color` in a style attribute too.
    let styled = strip(
        r#"<linearGradient id="g"><stop offset="0" style="stop-color:#00ff00;stop-opacity:0.5"/>
<stop offset="1" style="stop-color:#00ff00;stop-opacity:0.5"/></linearGradient>"#,
        "url(#g)",
    );
    let green = px(&styled, 100, 10, 50, 5);
    assert_eq!(&green[..3], &[0, 255, 0]);
    assert!(green[3].abs_diff(127) <= 1, "{green:?}");
}

/// **Offsets are held to 0..1 and to no less than the one before**, and may
/// be percentages.
#[test]
fn offsets_are_held_in_order() {
    let defs = r#"<linearGradient id="g"><stop offset="50%" stop-color="red"/>
<stop offset="0.2" stop-color="blue"/></linearGradient>"#;
    let svg = strip(defs, "url(#g)");
    // The second stop is held to 0.5: red before the middle, blue after.
    assert_eq!(px(&svg, 100, 10, 25, 5), [255, 0, 0, 255]);
    assert_eq!(px(&svg, 100, 10, 75, 5), [0, 0, 255, 255]);
}

/// **SVG's degenerate cases**: no stops paints nothing, one stop paints its
/// colour, a line of no length the last stop's; a missing gradient is its
/// fallback colour, or nothing.
#[test]
fn degenerate_gradients_paint_as_svg_says() {
    let none = strip(r#"<linearGradient id="g"/>"#, "url(#g)");
    assert_eq!(px(&none, 100, 10, 50, 5)[3], 0);
    let one = strip(
        r#"<linearGradient id="g"><stop offset="0.3" stop-color="lime"/></linearGradient>"#,
        "url(#g)",
    );
    assert_eq!(px(&one, 100, 10, 90, 5), [0, 255, 0, 255]);
    let point = strip(
        r#"<linearGradient id="g" x2="0"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>"#,
        "url(#g)",
    );
    assert_eq!(px(&point, 100, 10, 10, 5), [0, 0, 255, 255]);
    let fallback = strip("", "url(#missing) green");
    assert_eq!(px(&fallback, 100, 10, 50, 5), [0, 128, 0, 255]);
    let missing = strip("", "url(#missing)");
    assert_eq!(px(&missing, 100, 10, 50, 5)[3], 0);
}

/// **A shape with no width or no height takes no gradient measured against
/// its box**, as SVG says: a horizontal line stroked with one draws nothing.
#[test]
fn a_box_with_no_height_takes_no_box_gradient() {
    let svg = format!(
        r#"<svg viewBox="0 0 100 10" width="100" height="10"><defs>{RED_TO_BLUE}</defs>
<line x1="0" y1="5" x2="100" y2="5" stroke="url(#g)" stroke-width="4"/></svg>"#
    );
    assert_eq!(px(&svg, 100, 10, 50, 5)[3], 0);
    // In user space it draws.
    let user = r#"<svg viewBox="0 0 100 10" width="100" height="10"><defs>
<linearGradient id="g" gradientUnits="userSpaceOnUse" x2="100"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs>
<line x1="0" y1="5" x2="100" y2="5" stroke="url(#g)" stroke-width="4"/></svg>"#;
    assert!(near(px(user, 100, 10, 50, 5), [126, 0, 129, 255], 2));
}

/// **A gradient's transform moves it**: rotated a quarter turn, a gradient
/// runs top to bottom instead.
#[test]
fn a_gradient_transform_moves_it() {
    let svg = r#"<svg viewBox="0 0 10 100" width="10" height="100">
<linearGradient id="g" gradientTransform="rotate(90 0.5 0.5)"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>
<rect width="10" height="100" fill="url(#g)"/></svg>"#;
    assert!(near(px(svg, 10, 100, 5, 0), [254, 0, 1, 255], 3));
    assert!(near(px(svg, 10, 100, 5, 99), [1, 0, 254, 255], 3));
}

/// **`fill-opacity` is inherited**: a group's reaches its shapes.
#[test]
fn fill_opacity_is_inherited() {
    let svg = r#"<svg viewBox="0 0 10 10" width="10" height="10">
<g fill-opacity="0.5"><rect width="10" height="10" fill="red"/></g></svg>"#;
    let p = px(svg, 10, 10, 5, 5);
    assert_eq!(&p[..3], &[255, 0, 0]);
    assert!(p[3].abs_diff(127) <= 1, "{p:?}");
    // A shape's own still wins.
    let own = r#"<svg viewBox="0 0 10 10" width="10" height="10">
<g fill-opacity="0.5"><rect width="10" height="10" fill="red" fill-opacity="1"/></g></svg>"#;
    assert_eq!(px(own, 10, 10, 5, 5), [255, 0, 0, 255]);
}

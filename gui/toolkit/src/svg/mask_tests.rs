//! Tests for masks: drawn as SVG, each pixel checked against what the mask's
//! content keeps of it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::super::SvgDocument;
use super::kept;

/// The alpha of each pixel of `svg` drawn 4 by 4, row by row.
fn alphas(svg: &str) -> Vec<u8> {
    let buffer = SvgDocument::parse(svg).unwrap().render(4, 4);
    buffer.chunks_exact(4).map(|p| p[3]).collect()
}

/// A 4 by 4 drawing: `mask`, then a red square filling it masked by `#m`.
fn masked(mask: &str) -> String {
    format!(
        r#"<svg viewBox="0 0 4 4">{mask}<rect width="4" height="4" fill="red" mask="url(#m)"/></svg>"#
    )
}

/// **A mask keeps what its content covers, by its luminance**: white keeps
/// all, black nothing, grey half -- and nothing outside what it draws.
#[test]
fn a_mask_keeps_what_its_content_covers() {
    let half = alphas(&masked(
        r#"<mask id="m"><rect width="2" height="4" fill="white"/></mask>"#,
    ));
    assert_eq!((half[1], half[2]), (255, 0));
    let grey = alphas(&masked(
        r##"<mask id="m"><rect width="4" height="4" fill="#808080"/></mask>"##,
    ));
    assert!(grey[0].abs_diff(128) <= 1, "{}", grey[0]);
    let black = alphas(&masked(
        r#"<mask id="m"><rect width="4" height="4" fill="black"/></mask>"#,
    ));
    assert_eq!(black[0], 0);
}

/// **Luminance weighs green most and blue least**, as CSS Masking has it;
/// and an alpha mask keeps alpha alone.
#[test]
fn luminance_weighs_the_colours_as_css_does() {
    assert_eq!(kept([255, 255, 255, 255], true), 255);
    assert_eq!(kept([0, 255, 0, 255], true), 182);
    assert_eq!(kept([0, 0, 255, 255], true), 18);
    assert_eq!(kept([255, 0, 0, 255], true), 54);
    assert_eq!(kept([255, 255, 255, 128], true), 128);
    assert_eq!(kept([0, 0, 0, 200], false), 200);
    let alpha = alphas(&masked(
        r#"<mask id="m" mask-type="alpha"><rect width="4" height="4" fill="black"/></mask>"#,
    ));
    assert_eq!(alpha[0], 255);
    let styled = alphas(&masked(
        r#"<mask id="m" style="mask-type:alpha"><rect width="4" height="4" fill="black"/></mask>"#,
    ));
    assert_eq!(styled[0], 255);
}

/// **A mask's rectangle is in fractions of the box by default, and nothing
/// outside it is kept**; in user space it is in user units.
#[test]
fn a_masks_rectangle_cuts_what_it_keeps() {
    let boxed = alphas(&masked(
        r#"<mask id="m" x="0" y="0" width="0.5" height="1"><rect width="4" height="4" fill="white"/></mask>"#,
    ));
    assert_eq!((boxed[1], boxed[2]), (255, 0));
    let user = alphas(&masked(
        r#"<mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="1" height="4"><rect width="4" height="4" fill="white"/></mask>"#,
    ));
    assert_eq!((user[0], user[1]), (255, 0));
}

/// **A mask's content may be measured in the box**: half of it is the left
/// half, wherever the masked shape stands.
#[test]
fn a_masks_content_may_be_measured_in_the_box() {
    let svg = r#"<svg viewBox="0 0 4 4"><mask id="m" maskContentUnits="objectBoundingBox">
<rect width="0.5" height="1" fill="white"/></mask>
<rect x="2" width="2" height="4" fill="red" mask="url(#m)"/></svg>"#;
    let a = alphas(svg);
    // The shape is columns 2 and 3; its left half, column 2.
    assert_eq!((a[1], a[2], a[3]), (0, 255, 0));
}

/// **A gradient in a mask fades what it masks**.
#[test]
fn a_gradient_in_a_mask_fades() {
    let a = alphas(&masked(
        r#"<mask id="m"><linearGradient id="g"><stop stop-color="white"/><stop offset="1" stop-color="black"/></linearGradient>
<rect width="4" height="4" fill="url(#g)"/></mask>"#,
    ));
    assert!(a[0] > a[1] && a[1] > a[2] && a[2] > a[3], "{a:?}");
}

/// **A mask that names no mask masks nothing**; one over a box with no
/// area keeps nothing; one masked by itself ends.
#[test]
fn masks_that_cannot_be_drawn() {
    let missing =
        r#"<svg viewBox="0 0 4 4"><rect width="4" height="4" fill="red" mask="url(#nope)"/></svg>"#;
    assert!(alphas(missing).iter().all(|&a| a == 255));
    let not_a_mask = r#"<svg viewBox="0 0 4 4"><clipPath id="m"><rect width="2" height="4"/></clipPath>
<rect width="4" height="4" fill="red" mask="url(#m)"/></svg>"#;
    assert!(alphas(not_a_mask).iter().all(|&a| a == 255));
    let flat = r#"<svg viewBox="0 0 4 4"><mask id="m"><rect width="4" height="4" fill="white"/></mask>
<line x1="0" y1="2" x2="4" y2="2" stroke="red" stroke-width="2" mask="url(#m)"/></svg>"#;
    assert!(alphas(flat).iter().all(|&a| a == 0));
    let itself =
        masked(r#"<mask id="m"><rect width="4" height="4" fill="white" mask="url(#m)"/></mask>"#);
    assert!(alphas(&itself).iter().all(|&a| a == 0));
}

/// **A mask and a clip together keep what both keep.**
#[test]
fn a_mask_and_a_clip_multiply() {
    let svg = r#"<svg viewBox="0 0 4 4"><mask id="m"><rect width="2" height="4" fill="white"/></mask>
<clipPath id="c"><rect width="4" height="2"/></clipPath>
<rect width="4" height="4" fill="red" mask="url(#m)" clip-path="url(#c)"/></svg>"#;
    let a = alphas(svg);
    // Kept: the left half of the top half.
    assert_eq!((a[0], a[2], a[2 * 4], a[2 * 4 + 2]), (255, 0, 0, 0));
}

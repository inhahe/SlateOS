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

use std::fmt::Write as _;

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
    // Rounded, not cut down: a green of 1 is 0.72 of a step, kept as 1.
    assert_eq!(kept([0, 1, 0, 255], true), 1);
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

/// **Past the drawing's budget for scratch surfaces, a mask keeps nothing**:
/// with none left, what it masks is not drawn; with enough, it is.
#[test]
fn past_the_budget_a_mask_keeps_nothing() {
    let doc = SvgDocument::parse(&masked(
        r#"<mask id="m"><rect width="4" height="4" fill="white"/></mask>"#,
    ))
    .unwrap();
    let drawn_with = |budget: usize| {
        let mut renderer = super::super::SvgRenderer::new(4, 4, &doc);
        renderer.scratch_budget = budget;
        let buffer = doc.draw(renderer);
        buffer.chunks_exact(4).map(|p| p[3]).max().unwrap()
    };
    assert_eq!(drawn_with(0), 0);
    // The mask's region at 4 by 4 is the whole surface: sixteen pixels.
    assert_eq!(drawn_with(15), 0);
    assert_eq!(drawn_with(16), 255);
}

/// **A mask masked by itself many times over is drawn in bounded time**:
/// ten elements in a mask, each masked by that mask, would draw ten scratch
/// surfaces at the first level and a hundred million at the eighth -- hours
/// of work from a document of a dozen lines. The drawing's budget for them
/// ends it.
#[test]
fn a_mask_masked_by_itself_many_times_over_ends() {
    let mut strips = String::new();
    for i in 0..10 {
        write!(
            strips,
            r#"<rect x="{i}" width="1" height="10" fill="white" mask="url(#m)"/>"#
        )
        .unwrap();
    }
    let svg = format!(
        r#"<svg viewBox="0 0 10 10"><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="10" height="10">{strips}</mask>
<rect width="10" height="10" fill="red" mask="url(#m)"/></svg>"#
    );
    let doc = SvgDocument::parse(&svg).unwrap();
    let start = std::time::Instant::now();
    let _ = doc.render(64, 64);
    // Unbounded, this takes hours; bounded, well under a second in a debug
    // build. The ceiling is wide for a loaded machine.
    assert!(
        start.elapsed() < std::time::Duration::from_secs(30),
        "{:?}",
        start.elapsed()
    );
}

/// The alpha of the pixel at `(x, y)` of `svg` drawn `size` by `size`.
fn alpha_at(svg: &str, size: u32, x: u32, y: u32) -> u8 {
    let buffer = SvgDocument::parse(svg).unwrap().render(size, size);
    buffer[((y * size + x) * 4 + 3) as usize]
}

/// **A mask's rectangle is by default a tenth wider than the box on every
/// side**: a stroke reaching past the box is kept there and cut beyond it.
#[test]
fn a_masks_rectangle_is_a_tenth_wider_than_the_box() {
    // The box is 10..30 on both axes; the stroke reaches 5..35; the default
    // rectangle runs from 8 to 32.
    let svg = r#"<svg viewBox="0 0 40 40"><mask id="m"><rect width="40" height="40" fill="white"/></mask>
<rect x="10" y="10" width="20" height="20" fill="red" stroke="red" stroke-width="10" mask="url(#m)"/></svg>"#;
    assert_eq!(alpha_at(svg, 40, 9, 20), 255);
    assert_eq!(alpha_at(svg, 40, 6, 20), 0);
    assert_eq!(alpha_at(svg, 40, 30, 20), 255);
    assert_eq!(alpha_at(svg, 40, 33, 20), 0);
}

/// **A percentage in user space is of the viewport's own axis**: a height
/// of 50% in a viewport twice as wide as high is half its height.
#[test]
fn a_percentage_in_user_space_is_of_its_own_axis() {
    let svg = r#"<svg viewBox="0 0 40 20"><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100%" height="50%">
<rect width="40" height="20" fill="white"/></mask><rect width="40" height="20" fill="red" mask="url(#m)"/></svg>"#;
    // Drawn 40 by 40, the viewport fills the middle half: rows 10 to 30.
    assert_eq!(alpha_at(svg, 40, 20, 15), 255);
    assert_eq!(alpha_at(svg, 40, 20, 25), 0);
}

/// **A mask's rectangle cuts its content through a pixel as well as along
/// the pixels' edges**: a rectangle starting half way into a pixel keeps
/// half of it.
#[test]
fn a_masks_rectangle_cuts_through_a_pixel() {
    let svg = r#"<svg viewBox="0 0 4 4"><mask id="m" maskUnits="userSpaceOnUse" x="0.5" y="0" width="4" height="4">
<rect width="4" height="4" fill="white"/></mask><rect width="4" height="4" fill="red" mask="url(#m)"/></svg>"#;
    let edge = alpha_at(svg, 4, 0, 1);
    assert!(edge.abs_diff(128) <= 2, "{edge}");
    assert_eq!(alpha_at(svg, 4, 1, 1), 255);
}

/// A drawing whose element is masked by a chain of `depth` masks: each
/// mask's content a white square masked by the next, the last's unmasked.
fn chained(depth: usize) -> String {
    let mut masks = String::new();
    for level in 0..depth {
        let inner = if level + 1 < depth {
            format!(r#" mask="url(#m{})""#, level + 1)
        } else {
            String::new()
        };
        write!(
            masks,
            r#"<mask id="m{level}"><rect width="4" height="4" fill="white"{inner}/></mask>"#
        )
        .unwrap();
    }
    format!(
        r#"<svg viewBox="0 0 4 4">{masks}<rect width="4" height="4" fill="red" mask="url(#m0)"/></svg>"#
    )
}

/// **Masks nest as deep as clip paths do**: eight masks, each inside the
/// last's content, all apply; a ninth keeps nothing.
#[test]
fn masks_nest_as_deep_as_clip_paths() {
    assert_eq!(alphas(&chained(8))[5], 255);
    assert_eq!(alphas(&chained(9))[5], 0);
}

/// `svg` drawn 4 by 4 by a renderer whose budget for nodes drawn through
/// `<use>`s is `uses`, and which starts `depth` containers deep: each pixel's
/// alpha.
fn alphas_from(svg: &str, uses: usize, depth: usize) -> Vec<u8> {
    let doc = SvgDocument::parse(svg).unwrap();
    let mut renderer = super::super::SvgRenderer::new(4, 4, &doc);
    renderer.reuse_budget = uses;
    renderer.depth = depth;
    doc.draw(renderer).chunks_exact(4).map(|p| p[3]).collect()
}

/// **What a mask's content draws through `<use>`s comes out of the
/// drawing's budget**: three in the mask leave one of four for what follows
/// it -- so a document cannot multiply itself through masks where it cannot
/// through `<use>`s alone.
#[test]
fn a_masks_uses_come_out_of_the_drawings_budget() {
    let svg = r##"<svg viewBox="0 0 4 4"><defs><rect id="w" width="4" height="4" fill="white"/>
<rect id="p" width="1" height="1" fill="red"/></defs>
<mask id="m"><use href="#w"/><use href="#w"/><use href="#w"/></mask>
<rect x="3" y="3" width="1" height="1" fill="red" mask="url(#m)"/>
<use href="#p"/><use href="#p" x="1"/></svg>"##;
    let a = alphas_from(svg, 4, 0);
    // The masked square is drawn; the first <use> after it is, the second
    // finds the budget spent.
    assert_eq!((a[15], a[0], a[1]), (255, 255, 0));
    // With room for all of them, all are drawn.
    let b = alphas_from(svg, 100, 0);
    assert_eq!((b[15], b[0], b[1]), (255, 255, 255));
}

/// **A mask's content is drawn no deeper than the drawing may go**: a
/// drawing already at its depth limit draws nothing in a mask, which then
/// keeps nothing -- where a fresh count would let masks reach past it.
#[test]
fn a_masks_content_is_drawn_no_deeper_than_the_drawing() {
    let svg = masked(r#"<mask id="m"><rect width="4" height="4" fill="white"/></mask>"#);
    // The root is the first container, the masked square the second; its
    // mask's content is the fourth, inside the group that passes the mask's
    // style down to it.
    let limit = super::super::MAX_DRAWN_DEPTH;
    assert_eq!(alphas_from(&svg, 100, limit - 3)[0], 0);
    assert_eq!(alphas_from(&svg, 100, limit - 4)[0], 255);
}

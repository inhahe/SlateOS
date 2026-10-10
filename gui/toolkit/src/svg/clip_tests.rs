//! Tests for clip paths: drawn as SVG, each pixel checked against what the
//! clip leaves of it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::super::SvgDocument;
use super::Mask;

/// The alpha of every pixel of `svg` drawn `w` by `h`, row by row.
fn alphas(svg: &str, w: u32, h: u32) -> Vec<u8> {
    let buffer = SvgDocument::parse(svg).expect("parses").render(w, h);
    buffer.chunks_exact(4).map(|p| p[3]).collect()
}

/// How many pixels of `svg`, drawn 10 by 10, are painted at all.
fn painted(svg: &str) -> usize {
    alphas(svg, 10, 10).iter().filter(|&&a| a > 0).count()
}

/// A 10 by 10 drawing: `defs` (a clip path, say), then a red square filling
/// it with `clip`'s attributes.
fn clipped(defs: &str, clip: &str) -> String {
    format!(
        r#"<svg viewBox="0 0 10 10">{defs}<rect width="10" height="10" fill="red" {clip}/></svg>"#
    )
}

const RIGHT_HALF: &str = r#"<clipPath id="c"><rect x="5" width="5" height="10"/></clipPath>"#;

/// **A clip path leaves only what its shapes cover**: the square clipped to
/// the right half is drawn there and nowhere else.
#[test]
fn a_clip_path_leaves_what_its_shapes_cover() {
    let svg = clipped(RIGHT_HALF, r#"clip-path="url(#c)""#);
    let drawn = alphas(&svg, 10, 10);
    assert_eq!((drawn[5 * 10 + 2], drawn[5 * 10 + 7]), (0, 255));
    assert_eq!(painted(&svg), 50);
    // Named in a style too, and defined after what it clips.
    let after = r#"<svg viewBox="0 0 10 10"><rect width="10" height="10" style="clip-path:url(#c)"/>
<clipPath id="c"><rect x="5" width="5" height="10"/></clipPath></svg>"#;
    assert_eq!(painted(after), 50);
}

/// **A group's clip clips all it holds**, and nothing after it.
#[test]
fn a_groups_clip_clips_all_it_holds() {
    let svg = format!(
        r#"<svg viewBox="0 0 10 10">{RIGHT_HALF}<g clip-path="url(#c)">
<rect width="10" height="5" fill="red"/><rect y="5" width="10" height="5" fill="blue"/></g>
<rect width="1" height="1" fill="lime"/></svg>"#
    );
    let drawn = alphas(&svg, 10, 10);
    // Both of the group's halves cut; the square after it whole.
    assert_eq!(
        (drawn[2 * 10 + 2], drawn[7 * 10 + 2], drawn[7 * 10 + 7]),
        (0, 0, 255)
    );
    assert_eq!(drawn[0], 255);
    assert_eq!(painted(&svg), 51);
}

/// **In the clipped element's box, a clip path's numbers are fractions of
/// it**: half of a square standing anywhere is its left half.
#[test]
fn a_box_clip_is_measured_in_fractions_of_the_box() {
    let svg = r#"<svg viewBox="0 0 10 10"><clipPath id="c" clipPathUnits="objectBoundingBox">
<rect width="0.5" height="1"/></clipPath>
<rect x="4" width="6" height="10" fill="red" clip-path="url(#c)"/></svg>"#;
    let drawn = alphas(svg, 10, 10);
    // The rect is 4..10; its left half is 4..7.
    assert_eq!((drawn[5 * 10 + 5], drawn[5 * 10 + 8]), (255, 0));
    assert_eq!(painted(svg), 30);
    // A box with no area: nothing is left.
    let flat = r#"<svg viewBox="0 0 10 10"><clipPath id="c" clipPathUnits="objectBoundingBox">
<rect width="1" height="1"/></clipPath>
<line x1="0" y1="5" x2="10" y2="5" stroke="red" stroke-width="4" clip-path="url(#c)"/></svg>"#;
    assert_eq!(painted(flat), 0);
}

/// **A clip path's transform moves it**, outside its box's mapping.
#[test]
fn a_clip_paths_transform_moves_it() {
    let svg = clipped(
        r#"<clipPath id="c" transform="translate(5 0)"><rect width="5" height="10"/></clipPath>"#,
        r#"clip-path="url(#c)""#,
    );
    let drawn = alphas(&svg, 10, 10);
    assert_eq!((drawn[5 * 10 + 2], drawn[5 * 10 + 7]), (0, 255));
    // Outside the box's mapping: translate(0.5 0) in the clip's own space
    // is half the box over, not half a unit.
    let boxed = r#"<svg viewBox="0 0 10 10"><clipPath id="c" clipPathUnits="objectBoundingBox"
transform="translate(5 0)"><rect width="0.5" height="1"/></clipPath>
<rect width="10" height="10" fill="red" clip-path="url(#c)"/></svg>"#;
    let drawn = alphas(boxed, 10, 10);
    assert_eq!((drawn[5 * 10 + 2], drawn[5 * 10 + 7]), (0, 255));
}

/// **A clip path's shapes are filled by their `clip-rule`**, inherited from
/// the clip path: `evenodd` leaves a ring's hole out, `nonzero` in.
#[test]
fn clip_shapes_are_filled_by_their_clip_rule() {
    let ring = |rule_on_path: &str, rule_on_clip: &str| {
        clipped(
            &format!(
                r#"<clipPath id="c" {rule_on_clip}><path d="M0 0 H10 V10 H0 Z M3 3 H7 V7 H3 Z" {rule_on_path}/></clipPath>"#
            ),
            r#"clip-path="url(#c)""#,
        )
    };
    let hole = |svg: &str| alphas(svg, 10, 10)[5 * 10 + 5];
    assert_eq!(hole(&ring(r#"clip-rule="evenodd""#, "")), 0);
    assert_eq!(hole(&ring("", r#"clip-rule="evenodd""#)), 0);
    assert_eq!(hole(&ring("", "")), 255);
    // The path's own wins; and fill-rule is not clip-rule.
    assert_eq!(
        hole(&ring(r#"clip-rule="nonzero""#, r#"clip-rule="evenodd""#)),
        255
    );
    assert_eq!(hole(&ring(r#"fill-rule="evenodd""#, "")), 255);
}

/// **A clip path's shapes are a union, without seams**: two meeting half
/// way through a column of pixels leave all of that column.
#[test]
fn a_clip_paths_shapes_are_a_union_without_seams() {
    let svg = clipped(
        r#"<clipPath id="c"><rect width="5.5" height="10"/><rect x="5.5" width="4.5" height="10"/></clipPath>"#,
        r#"clip-path="url(#c)""#,
    );
    assert!(alphas(&svg, 10, 10).iter().all(|&a| a == 255));
}

/// **Only a shape's geometry clips**: not its colour, its stroke, its
/// opacity -- and a `<use>` in a clip path is the shape it names.
#[test]
fn only_geometry_clips() {
    let unfilled = clipped(
        r#"<clipPath id="c"><rect x="5" width="5" height="10" fill="none" stroke="red" stroke-width="4" opacity="0"/></clipPath>"#,
        r#"clip-path="url(#c)""#,
    );
    assert_eq!(painted(&unfilled), 50);
    let used = clipped(
        r##"<defs><rect id="r" x="5" width="5" height="10"/></defs><clipPath id="c"><use href="#r"/></clipPath>"##,
        r#"clip-path="url(#c)""#,
    );
    assert_eq!(painted(&used), 50);
}

/// **What may not stand in a clip path clips nothing**: a group, a hidden
/// shape -- and a clip path holding nothing leaves nothing.
#[test]
fn a_clip_path_holds_only_shapes() {
    for clip in [
        r#"<clipPath id="c"><g><rect width="10" height="10"/></g></clipPath>"#,
        r#"<clipPath id="c"><rect width="10" height="10" display="none"/></clipPath>"#,
        r#"<clipPath id="c"/>"#,
    ] {
        assert_eq!(
            painted(&clipped(clip, r#"clip-path="url(#c)""#)),
            0,
            "{clip}"
        );
    }
}

/// **A clip that names no clip path clips nothing**: a missing one, an
/// element that is not one, `none`.
#[test]
fn a_clip_that_names_no_clip_path_clips_nothing() {
    for (defs, clip) in [
        ("", r#"clip-path="url(#missing)""#),
        (
            r#"<defs><rect id="r" width="1" height="1"/></defs>"#,
            r#"clip-path="url(#r)""#,
        ),
        (RIGHT_HALF, r#"clip-path="none""#),
        // An `id` an earlier element took: the reference finds that one.
        (
            r#"<defs><rect id="c" width="1" height="1"/></defs><clipPath id="c"><rect x="5" width="5" height="10"/></clipPath>"#,
            r#"clip-path="url(#c)""#,
        ),
    ] {
        assert_eq!(painted(&clipped(defs, clip)), 100, "{clip}");
    }
}

/// **Clips multiply**: a clip around a clip, and a clip path's own clip.
#[test]
fn clips_multiply() {
    let bottom = r#"<clipPath id="b"><rect y="5" width="10" height="5"/></clipPath>"#;
    let nested = format!(
        r#"<svg viewBox="0 0 10 10">{RIGHT_HALF}{bottom}<g clip-path="url(#c)">
<rect width="10" height="10" fill="red" clip-path="url(#b)"/></g></svg>"#
    );
    let drawn = alphas(&nested, 10, 10);
    assert_eq!(
        (drawn[7 * 10 + 7], drawn[2 * 10 + 7], drawn[7 * 10 + 2]),
        (255, 0, 0)
    );
    assert_eq!(painted(&nested), 25);
    let of_clip = clipped(
        &format!(
            r#"{bottom}<clipPath id="c" clip-path="url(#b)"><rect x="5" width="5" height="10"/></clipPath>"#
        ),
        r#"clip-path="url(#c)""#,
    );
    assert_eq!(painted(&of_clip), 25);
    // One that clips itself ends.
    let itself = clipped(
        r#"<clipPath id="c" clip-path="url(#c)"><rect x="5" width="5" height="10"/></clipPath>"#,
        r#"clip-path="url(#c)""#,
    );
    assert_eq!(painted(&itself), 50);
}

/// **A `<use>`'s clip is measured where SVG says the `<use>` stands**: in its
/// user space with its `x` and `y`, as the group it stands for.
#[test]
fn a_uses_clip_is_measured_with_its_x_and_y() {
    let svg = r##"<svg viewBox="0 0 20 10"><defs><rect id="r" width="10" height="10" fill="red"/></defs>
<clipPath id="c"><rect width="5" height="10"/></clipPath>
<use href="#r" x="10" clip-path="url(#c)"/></svg>"##;
    let drawn = alphas(svg, 20, 10);
    // The square at 10..20; the clip moved with it, to 10..15.
    assert_eq!((drawn[5 * 20 + 12], drawn[5 * 20 + 17]), (255, 0));
}

/// **A mask is what its region leaves**: nothing outside it, and two masks
/// leave what both do.
#[test]
fn a_mask_leaves_its_region_and_masks_multiply() {
    let mut half = Mask::over(2, 2, 6, 6);
    half.add_row(3, 2, &[0.5, 1.0, 1.0, 1.0]);
    assert_eq!(half.share(3, 3), 1.0);
    assert!((half.share(2, 3) - 128.0 / 255.0).abs() < 1e-6);
    // Outside the region, and in it but not added, nothing.
    for (x, y) in [(1, 3), (6, 3), (3, 1), (3, 6), (3, 4)] {
        assert_eq!(half.share(x, y), 0.0, "({x}, {y})");
    }
    // Adding more never leaves more than all.
    half.add_row(3, 3, &[1.0]);
    assert_eq!(half.share(3, 3), 1.0);
    let mut other = Mask::over(0, 0, 4, 4);
    other.add_row(3, 0, &[1.0, 1.0, 0.5, 0.5]);
    let both = half.intersect(&other);
    // A half is kept as 128 of 255.
    assert!((both.share(3, 3) - 128.0 / 255.0).abs() < 1e-6);
    assert!((both.share(2, 3) - 0.5 * 128.0 / 255.0).abs() < 0.01);
    assert_eq!(both.share(4, 3), 0.0);
    assert_eq!(Mask::nothing().share(0, 0), 0.0);
    assert_eq!(Mask::over(5, 5, 5, 9).share(5, 5), 0.0);
}

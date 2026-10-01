//! Tests for viewports: the outermost `<svg>` fitted to the pixels as its
//! `preserveAspectRatio` says, and an `<svg>` inside another placed in a
//! viewport of its own.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::{AspectRatio, SvgDocument, fit_view_box};

/// The alpha of every pixel of `svg` drawn `w` by `h`, row by row.
fn alphas(svg: &str, w: u32, h: u32) -> Vec<u8> {
    let buffer = SvgDocument::parse(svg).expect("parses").render(w, h);
    buffer.chunks_exact(4).map(|p| p[3]).collect()
}

/// The pixel at `(x, y)` of `svg` drawn `w` by `h`.
fn px(svg: &str, w: u32, h: u32, x: u32, y: u32) -> [u8; 4] {
    let buffer = SvgDocument::parse(svg).expect("parses").render(w, h);
    let at = ((y * w + x) * 4) as usize;
    [buffer[at], buffer[at + 1], buffer[at + 2], buffer[at + 3]]
}

/// Whether `t` takes `from` to `to`, to rounding.
fn maps(t: super::Transform, from: (f32, f32), to: (f32, f32)) -> bool {
    let (x, y) = t.apply(from.0, from.1);
    (x - to.0).abs() < 1e-4 && (y - to.1).abs() < 1e-4
}

/// **`preserveAspectRatio` is read as SVG writes it**, and anything else is
/// the initial value, `xMidYMid meet`.
#[test]
fn preserve_aspect_ratio_is_read_as_svg_writes_it() {
    let read = AspectRatio::parse;
    assert_eq!(
        read("xMinYMax slice"),
        AspectRatio {
            align: Some((0.0, 1.0)),
            slice: true
        }
    );
    assert_eq!(
        read("defer xMaxYMid"),
        AspectRatio {
            align: Some((1.0, 0.5)),
            slice: false
        }
    );
    assert_eq!(
        read("  none  "),
        AspectRatio {
            align: None,
            slice: false
        }
    );
    assert_eq!(
        read("xMidYMin meet"),
        AspectRatio {
            align: Some((0.5, 0.0)),
            slice: false
        }
    );
    for bad in [
        "",
        "bogus",
        "xMidYmid",
        "xMidYMid nonsense",
        "xMidYMid meet extra",
        "xMid",
    ] {
        assert_eq!(read(bad), AspectRatio::DEFAULT, "{bad:?}");
    }
}

/// **A view box is fitted as `preserveAspectRatio` says**: as large as fits
/// and centred, as large as covers and placed, or stretched; from wherever
/// its corner is, into wherever the viewport is.
#[test]
fn a_view_box_is_fitted_as_it_says() {
    let square = (0.0, 0.0, 10.0, 10.0);
    let wide = (0.0, 0.0, 20.0, 10.0);
    let meet = fit_view_box(square, AspectRatio::DEFAULT, wide).unwrap();
    assert!(maps(meet, (0.0, 0.0), (5.0, 0.0)) && maps(meet, (10.0, 10.0), (15.0, 10.0)));
    let slice = AspectRatio::parse("xMinYMin slice");
    let cover = fit_view_box(square, slice, wide).unwrap();
    assert!(maps(cover, (0.0, 0.0), (0.0, 0.0)) && maps(cover, (10.0, 10.0), (20.0, 20.0)));
    let end = fit_view_box(square, AspectRatio::parse("xMidYMax slice"), wide).unwrap();
    assert!(maps(end, (10.0, 10.0), (20.0, 10.0)), "{end:?}");
    let stretch = fit_view_box(square, AspectRatio::parse("none"), wide).unwrap();
    assert!(maps(stretch, (10.0, 10.0), (20.0, 10.0)) && maps(stretch, (10.0, 0.0), (20.0, 0.0)));
    let moved = fit_view_box(
        (5.0, 5.0, 10.0, 10.0),
        AspectRatio::DEFAULT,
        (100.0, 50.0, 10.0, 10.0),
    );
    assert!(maps(moved.unwrap(), (5.0, 5.0), (100.0, 50.0)));
    for bad in [
        (0.0, 0.0, 0.0, 10.0),
        (0.0, 0.0, 10.0, -1.0),
        (0.0, 0.0, f32::NAN, 10.0),
    ] {
        assert_eq!(
            fit_view_box(bad, AspectRatio::DEFAULT, wide),
            None,
            "{bad:?}"
        );
    }
}

/// **A drawing asked for at another shape is fitted, not stretched**: a
/// square drawn twice as wide as it is high sits in the middle -- unless it
/// says `none`, and then it is stretched.
#[test]
fn a_drawing_is_fitted_to_the_pixels_not_stretched() {
    let make = |aspect: &str| {
        format!(
            r#"<svg viewBox="0 0 10 10" preserveAspectRatio="{aspect}"><rect width="10" height="10" fill="red"/></svg>"#
        )
    };
    let centred = alphas(&make("xMidYMid"), 20, 10);
    assert_eq!(
        (
            centred[5 * 20 + 2],
            centred[5 * 20 + 10],
            centred[5 * 20 + 17]
        ),
        (0, 255, 0)
    );
    let stretched = alphas(&make("none"), 20, 10);
    assert!(stretched.iter().all(|&a| a == 255));
}

/// **`slice` covers the pixels and is cut where it overflows**, placed as
/// its alignment says: a box red above blue, drawn twice as wide as high,
/// shows its top at `yMin` and its bottom at `yMax`.
#[test]
fn slice_covers_and_is_placed() {
    let make = |aspect: &str| {
        format!(
            r#"<svg viewBox="0 0 10 10" preserveAspectRatio="{aspect}"><rect width="10" height="5" fill="red"/>
<rect y="5" width="10" height="5" fill="blue"/></svg>"#
        )
    };
    assert_eq!(px(&make("xMinYMin slice"), 20, 10, 5, 9), [255, 0, 0, 255]);
    assert_eq!(px(&make("xMidYMax slice"), 20, 10, 5, 0), [0, 0, 255, 255]);
}

/// **A view box with no area draws nothing**, rather than dividing by
/// nothing.
#[test]
fn a_view_box_with_no_area_draws_nothing() {
    // A square over all the plane, which any box that is drawn at all --
    // a mirrored one too -- would put on the pixels.
    for view_box in ["0 0 0 10", "0 0 10 0", "0 0 -5 10", "0 0 10 -5"] {
        let svg = format!(
            r#"<svg viewBox="{view_box}"><rect x="-1000" y="-1000" width="2000" height="2000"/></svg>"#
        );
        assert!(alphas(&svg, 10, 10).iter().all(|&a| a == 0), "{view_box}");
    }
}

/// **A size in pixels is a size**: `width="24px"` with no view box shows
/// 24 user units, where it fell back to 300.
#[test]
fn a_size_in_pixels_is_a_size() {
    let doc =
        SvgDocument::parse(r#"<svg width="24px" height="16"><rect width="1" height="1"/></svg>"#)
            .unwrap();
    assert_eq!(doc.viewbox(), (0.0, 0.0, 24.0, 16.0));
}

/// **An `<svg>` inside another is a viewport of its own**: placed at its
/// `x` and `y`, its view box fitted to its `width` and `height`.
#[test]
fn an_inner_svg_is_a_viewport_of_its_own() {
    let svg = r#"<svg viewBox="0 0 20 20"><svg x="10" width="10" height="10" viewBox="0 0 1 1">
<rect width="1" height="1" fill="red"/></svg></svg>"#;
    let drawn = alphas(svg, 20, 20);
    assert_eq!(
        (drawn[5 * 20 + 15], drawn[5 * 20 + 5], drawn[15 * 20 + 15]),
        (255, 0, 0)
    );
    // With no size it is all of its parent's, and its view box fills that.
    let whole = r#"<svg viewBox="0 0 20 20"><svg viewBox="0 0 2 2">
<rect width="1" height="1" fill="red"/></svg></svg>"#;
    let drawn = alphas(whole, 20, 20);
    assert_eq!((drawn[5 * 20 + 5], drawn[15 * 20 + 15]), (255, 0));
}

/// **An inner `<svg>`'s place may be a percentage of its parent's
/// viewport**, and without a view box it only moves what it holds; its own
/// `transform` applies outside its placement.
#[test]
fn an_inner_svgs_place_may_be_a_percentage() {
    let svg = r#"<svg viewBox="0 0 20 20"><svg x="50%" y="25%" width="50%">
<rect width="5" height="5" fill="red"/></svg></svg>"#;
    let drawn = alphas(svg, 20, 20);
    // At (10, 5) to (15, 10).
    assert_eq!(
        (drawn[7 * 20 + 12], drawn[2 * 20 + 12], drawn[7 * 20 + 5]),
        (255, 0, 0)
    );
    let moved = r#"<svg viewBox="0 0 20 20"><svg transform="translate(10 0)">
<rect width="5" height="5" fill="red"/></svg></svg>"#;
    let drawn = alphas(moved, 20, 20);
    assert_eq!((drawn[2 * 20 + 12], drawn[2 * 20 + 2]), (255, 0));
}

/// **An inner `<svg>` with no area draws nothing**, as SVG has it.
#[test]
fn an_inner_svg_with_no_area_draws_nothing() {
    for size in [r#"width="0""#, r#"height="0""#, r#"width="-3""#] {
        let svg = format!(
            r#"<svg viewBox="0 0 10 10"><svg {size}><rect width="10" height="10"/></svg></svg>"#
        );
        assert!(alphas(&svg, 10, 10).iter().all(|&a| a == 0), "{size}");
    }
}

/// How many pixels of `svg`, drawn `w` by `h`, are painted at all.
fn painted(svg: &str, w: u32, h: u32) -> usize {
    alphas(svg, w, h).iter().filter(|&&a| a > 0).count()
}

/// **An inner `<svg>` is cut to its viewport**, unless it says
/// `overflow: visible`.
#[test]
fn an_inner_svg_is_cut_to_its_viewport() {
    let make = |overflow: &str| {
        format!(
            r#"<svg viewBox="0 0 20 20"><svg x="5" y="5" width="5" height="5" {overflow}>
<rect width="10" height="10" fill="red"/></svg></svg>"#
        )
    };
    assert_eq!(painted(&make(""), 20, 20), 25);
    assert_eq!(painted(&make(r#"overflow="visible""#), 20, 20), 100);
}

/// **An inner `<svg>`'s own clip is measured in its parent's space**, as
/// its `x`, `y`, `width` and `height` are -- not inside its view box.
#[test]
fn an_inner_svgs_clip_is_measured_where_it_stands() {
    let svg = r#"<svg viewBox="0 0 20 20"><clipPath id="c"><rect width="10" height="20"/></clipPath>
<svg viewBox="0 0 1 1" width="20" height="20" clip-path="url(#c)"><rect width="1" height="1" fill="red"/></svg></svg>"#;
    // The left half: in the view box's units, 10 would be all of it.
    assert_eq!(painted(svg, 20, 20), 200);
}

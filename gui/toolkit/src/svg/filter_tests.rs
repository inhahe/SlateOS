//! Tests for filters: documents drawn with them, each pixel checked against
//! what Filter Effects 1 says the filter makes of the element.
//!
//! Every drawing is 20 by 20 user units on 20 by 20 pixels, so a user unit
//! is a pixel and positions read straight across.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::super::{SvgDocument, SvgRenderer};

/// `body` inside a 20 by 20 drawing, drawn at 20 by 20: each pixel's
/// straight `[r, g, b, a]`.
fn draw(body: &str) -> Vec<[u8; 4]> {
    let svg = format!(r#"<svg viewBox="0 0 20 20" width="20" height="20">{body}</svg>"#);
    let doc = SvgDocument::parse(&svg).unwrap();
    pixels(&doc.render(20, 20))
}

fn pixels(buffer: &[u8]) -> Vec<[u8; 4]> {
    buffer
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

/// The pixel at `(x, y)` of a 20 by 20 drawing.
fn at(image: &[[u8; 4]], x: usize, y: usize) -> [u8; 4] {
    image[y * 20 + x]
}

fn near(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
    a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= tolerance)
}

const RED: [u8; 4] = [255, 0, 0, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// A red 10 by 10 square at (5, 5), filtered by `filter`, which is defined
/// by `defs`.
fn square(defs: &str, filter: &str) -> Vec<[u8; 4]> {
    draw(&format!(
        r#"{defs}<rect x="5" y="5" width="10" height="10" fill="red" filter="{filter}"/>"#
    ))
}

// ─── Regions and subregions ─────────────────────────────────────────────────

/// **An offset moves what the element draws, and the filter's region cuts
/// it**: the default region is the box grown by a tenth each way, 4 to 16.
#[test]
fn an_offset_is_cut_by_the_filters_region() {
    let image = square(r#"<filter id="f"><feOffset dx="5"/></filter>"#, "url(#f)");
    assert_eq!(at(&image, 12, 10), RED);
    assert_eq!(at(&image, 15, 10), RED);
    assert_eq!(at(&image, 16, 10), CLEAR, "past the region");
    assert_eq!(at(&image, 7, 10), CLEAR, "moved away");
}

/// **A region in user units is where it says, and a flood fills its own
/// subregion and nothing else**; the element itself is not drawn, the
/// flood being the filter's last word.
#[test]
fn a_flood_fills_its_subregion() {
    let image = square(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
             <feFlood flood-color="blue" x="2" y="3" width="4" height="5"/>
           </filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&image, 2, 3), [0, 0, 255, 255]);
    assert_eq!(at(&image, 5, 7), [0, 0, 255, 255]);
    assert_eq!(at(&image, 6, 7), CLEAR);
    assert_eq!(at(&image, 5, 8), CLEAR);
    assert_eq!(
        at(&image, 10, 10),
        CLEAR,
        "the element is what the filter makes"
    );
}

/// **`flood-opacity` multiplies the colour's own alpha.**
#[test]
fn flood_opacity_multiplies_the_colours_alpha() {
    let image = square(
        r#"<filter id="f"><feFlood flood-color="rgba(0,0,255,0.5)" flood-opacity="0.5"/></filter>"#,
        "url(#f)",
    );
    let [_, _, b, a] = at(&image, 10, 10);
    assert_eq!(b, 255);
    assert!(a.abs_diff(64) <= 1, "{a}");
}

/// **A primitive reading only results defaults to the union of their
/// subregions** -- so an offset of a small flood is cut where the flood
/// was -- unless it says its own.
#[test]
fn a_subregion_defaults_to_its_inputs() {
    let defs = |offset_extent: &str| {
        format!(
            r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
                 <feFlood flood-color="red" x="0" y="0" width="5" height="5" result="a"/>
                 <feOffset in="a" dx="10" {offset_extent}/>
               </filter>"#
        )
    };
    let cut = square(&defs(""), "url(#f)");
    assert_eq!(at(&cut, 12, 2), CLEAR, "cut to the flood's subregion");
    assert_eq!(at(&cut, 2, 2), CLEAR, "and moved out of it");
    let own = square(&defs(r#"x="0" width="20""#), "url(#f)");
    assert_eq!(at(&own, 12, 2), RED);
}

/// **A primitive reading the source defaults to the filter's region**, so
/// a blur spreads past the element but not past the region.
#[test]
fn a_blur_spreads_to_the_regions_edge() {
    let image = square(
        r#"<filter id="f"><feGaussianBlur stdDeviation="1"/></filter>"#,
        "url(#f)",
    );
    assert!(at(&image, 10, 10)[3] > 240, "{:?}", at(&image, 10, 10));
    let edge = at(&image, 15, 10)[3];
    assert!(edge > 10 && edge < 200, "{edge}");
    assert_eq!(at(&image, 16, 10), CLEAR);
    assert_eq!(at(&image, 1, 10), CLEAR);
}

// ─── Inputs ─────────────────────────────────────────────────────────────────

/// **`SourceAlpha` is the element's shape**: a flood kept where it is gives
/// the shape in the flood's colour.
#[test]
fn source_alpha_is_the_shape() {
    let image = square(
        r#"<filter id="f">
             <feFlood flood-color="lime" result="c"/>
             <feComposite in="c" in2="SourceAlpha" operator="in"/>
           </filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&image, 10, 10), [0, 255, 0, 255]);
    assert_eq!(at(&image, 4, 10), CLEAR);
}

/// **A merge lays its nodes in order, each reading as an `in` would**:
/// the source over a flood names the flood's result.
#[test]
fn a_merge_lays_its_nodes_in_order() {
    let image = square(
        r#"<filter id="f">
             <feFlood flood-color="blue" result="back"/>
             <feMerge><feMergeNode in="back"/><feMergeNode in="SourceGraphic"/></feMerge>
           </filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&image, 10, 10), RED);
    assert_eq!(
        at(&image, 4, 10),
        [0, 0, 255, 255],
        "the flood, round the source"
    );
}

/// **A result named twice is read from the nearest before**, and a name no
/// primitive gave reads as no name: the previous result.
#[test]
fn a_name_reads_the_nearest_result() {
    let image = square(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
             <feFlood flood-color="blue" result="x"/>
             <feFlood flood-color="lime" result="x"/>
             <feOffset in="x"/>
           </filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&image, 1, 1), [0, 255, 0, 255]);
    let unknown = square(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
             <feFlood flood-color="blue"/>
             <feOffset in="nothing-of-that-name"/>
           </filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&unknown, 1, 1), [0, 0, 255, 255]);
}

// ─── Each primitive, as drawn ───────────────────────────────────────────────

/// **A drop shadow is the element's shape, blurred, moved and coloured,
/// under the element.**
#[test]
fn a_drop_shadow_lies_under_and_beside() {
    let image = square(
        r#"<filter id="f"><feDropShadow dx="1" dy="1" stdDeviation="0" flood-color="blue"/></filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&image, 10, 10), RED, "the element over its shadow");
    assert_eq!(
        at(&image, 15, 15),
        [0, 0, 255, 255],
        "the shadow past its corner"
    );
    assert_eq!(at(&image, 4, 4), CLEAR);
}

/// **A colour matrix as written**: red and blue swapped.
#[test]
fn a_colour_matrix_swaps_channels() {
    let image = square(
        r#"<filter id="f" color-interpolation-filters="sRGB">
             <feColorMatrix values="0 0 1 0 0  0 1 0 0 0  1 0 0 0 0  0 0 0 1 0"/>
           </filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&image, 10, 10), [0, 0, 255, 255]);
}

/// **Transfer functions work in the primitive's colour space**: halving
/// white in sRGB is 128; in linear light, the initial value, it is the
/// lighter 188.
#[test]
fn colour_interpolation_decides_the_space() {
    let halve = |space: &str| {
        square(
            &format!(
                r#"<filter id="f" {space}>
                     <feFlood flood-color="white" result="w"/>
                     <feComponentTransfer in="w">
                       <feFuncR type="linear" slope="0.5"/>
                     </feComponentTransfer>
                   </filter>"#
            ),
            "url(#f)",
        )
    };
    let srgb = at(&halve(r#"color-interpolation-filters="sRGB""#), 10, 10);
    assert!(srgb[0].abs_diff(128) <= 1, "{srgb:?}");
    let linear = at(&halve(""), 10, 10);
    assert!(linear[0].abs_diff(188) <= 1, "{linear:?}");
    // The property on the primitive outranks the filter's.
    let own = square(
        r#"<filter id="f" color-interpolation-filters="linearRGB">
             <feFlood flood-color="white" result="w"/>
             <feComponentTransfer in="w" color-interpolation-filters="sRGB">
               <feFuncR type="linear" slope="0.5"/>
             </feComponentTransfer>
           </filter>"#,
        "url(#f)",
    );
    assert!(at(&own, 10, 10)[0].abs_diff(128) <= 1);
}

/// **A blend multiplies two floods.**
#[test]
fn a_blend_multiplies() {
    let image = square(
        r##"<filter id="f" color-interpolation-filters="sRGB">
             <feFlood flood-color="#ff8000" result="a"/>
             <feFlood flood-color="#80ff00" result="b"/>
             <feBlend in="a" in2="b" mode="multiply"/>
           </filter>"##,
        "url(#f)",
    );
    let px = at(&image, 10, 10);
    assert!(near(px, [128, 128, 0, 255], 1), "{px:?}");
}

/// **`lighter` adds, and `arithmetic` computes its four terms.**
#[test]
fn composites_add_and_compute() {
    let image = square(
        r##"<filter id="f" color-interpolation-filters="sRGB">
             <feFlood flood-color="#800000" result="a"/>
             <feFlood flood-color="#008000" result="b"/>
             <feComposite in="a" in2="b" operator="lighter"/>
           </filter>"##,
        "url(#f)",
    );
    assert!(near(at(&image, 10, 10), [128, 128, 0, 255], 1));
    let image = square(
        r#"<filter id="f" color-interpolation-filters="sRGB">
             <feComposite in="SourceGraphic" in2="SourceGraphic" operator="arithmetic" k2="0.5" k4="0"/>
           </filter>"#,
        "url(#f)",
    );
    // Half of each premultiplied channel: red at half alpha.
    let px = at(&image, 10, 10);
    assert!(near(px, [255, 0, 0, 128], 1), "{px:?}");
}

/// **A convolution with the identity kernel changes nothing**, and one
/// that cannot be read passes its input through.
#[test]
fn a_convolution_as_drawn() {
    let identity = square(
        r#"<filter id="f"><feConvolveMatrix kernelMatrix="0 0 0 0 1 0 0 0 0"/></filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&identity, 10, 10), RED);
    assert_eq!(at(&identity, 4, 10), CLEAR);
    let short = square(
        r#"<filter id="f"><feConvolveMatrix kernelMatrix="1 1"/></filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&short, 10, 10), RED);
}

/// **Dilation grows the shape by its radius; erosion shrinks it.**
#[test]
fn morphology_grows_and_shrinks() {
    let grown = square(
        r#"<filter id="f"><feMorphology operator="dilate" radius="1"/></filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&grown, 4, 10), RED);
    assert_eq!(at(&grown, 3, 10), CLEAR, "outside the region as well");
    let shrunk = square(
        r#"<filter id="f"><feMorphology operator="erode" radius="2"/></filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&shrunk, 6, 10), CLEAR);
    assert_eq!(at(&shrunk, 7, 10), RED);
}

/// **A tile repeats a subregion across the region.**
#[test]
fn a_tile_repeats_its_input() {
    let image = square(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
             <feFlood flood-color="blue" x="0" y="0" width="2" height="2" result="t"/>
             <feOffset in="t" result="o"/>
             <feTile in="o"/>
           </filter>"#,
        "url(#f)",
    );
    for (x, y) in [(0, 0), (1, 1), (8, 6), (19, 19)] {
        assert_eq!(at(&image, x, y), [0, 0, 255, 255], "({x}, {y})");
    }
}

/// **An `feImage` draws the element it names**, as a `<use>` would, in
/// the filtered element's user space.
#[test]
fn an_image_draws_the_element_it_names() {
    let image = square(
        r##"<defs><rect id="r" x="1" y="1" width="3" height="3" fill="blue"/></defs>
           <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
             <feImage href="#r"/>
           </filter>"##,
        "url(#f)",
    );
    assert_eq!(at(&image, 2, 2), [0, 0, 255, 255]);
    assert_eq!(at(&image, 10, 10), CLEAR);
}

/// **An `feImage` of the element it filters ends**: drawn inside itself it
/// is drawn once, not without end.
#[test]
fn an_image_of_itself_ends() {
    let image = draw(
        r##"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
             <feImage href="#g"/>
           </filter>
           <g id="g" filter="url(#f)"><rect x="5" y="5" width="10" height="10" fill="red"/></g>"##,
    );
    assert_eq!(image.len(), 400);
}

/// **Lit from straight above, a flat surface is the light's colour**; a
/// filter with no light source lights nothing.
#[test]
fn diffuse_light_from_above_is_the_lights_colour() {
    let image = square(
        r##"<filter id="f">
             <feDiffuseLighting in="SourceAlpha" lighting-color="#00ff00">
               <feDistantLight azimuth="0" elevation="90"/>
             </feDiffuseLighting>
           </filter>"##,
        "url(#f)",
    );
    assert!(
        near(at(&image, 10, 10), [0, 255, 0, 255], 1),
        "{:?}",
        at(&image, 10, 10)
    );
    let dark = square(
        r#"<filter id="f"><feDiffuseLighting in="SourceAlpha"/></filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&dark, 10, 10), CLEAR);
}

/// **A mid-grey displacement map moves nothing.**
#[test]
fn a_flat_map_displaces_nothing() {
    let image = square(
        r##"<filter id="f" color-interpolation-filters="sRGB">
             <feFlood flood-color="#808080" result="map"/>
             <feDisplacementMap in="SourceGraphic" in2="map" scale="1" xChannelSelector="R" yChannelSelector="G"/>
           </filter>"##,
        "url(#f)",
    );
    assert_eq!(at(&image, 10, 10), RED);
    assert_eq!(at(&image, 5, 5), RED);
    assert_eq!(at(&image, 4, 4), CLEAR);
}

/// **Turbulence is the same each time it is drawn**, and draws something.
#[test]
fn turbulence_is_the_same_each_time() {
    let defs =
        r#"<filter id="f"><feTurbulence baseFrequency="0.1" numOctaves="2" seed="3"/></filter>"#;
    let one = square(defs, "url(#f)");
    let two = square(defs, "url(#f)");
    assert_eq!(one, two);
    assert!(one.iter().any(|px| px[3] > 0));
    let other = square(
        r#"<filter id="f"><feTurbulence baseFrequency="0.1" numOctaves="2" seed="4"/></filter>"#,
        "url(#f)",
    );
    assert_ne!(one, other);
}

// ─── Units ──────────────────────────────────────────────────────────────────

/// **Under `primitiveUnits="objectBoundingBox"` an offset is a fraction of
/// the box**: half of a 10-wide square is 5.
#[test]
fn primitive_units_measure_in_the_box() {
    let image = square(
        r#"<filter id="f" primitiveUnits="objectBoundingBox" x="0" width="2">
             <feOffset dx="0.5"/>
           </filter>"#,
        "url(#f)",
    );
    assert_eq!(at(&image, 10, 10), RED);
    assert_eq!(at(&image, 19, 10), RED);
    assert_eq!(at(&image, 9, 10), CLEAR);
}

/// **Lengths scale with the element's transform, and an offset turns with
/// it**: dx 2 under a scale of 2 is 4 pixels; under a quarter turn it is
/// down, not across.
#[test]
fn offsets_follow_the_transform() {
    let scaled = draw(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="10" height="10">
             <feOffset dx="2"/>
           </filter>
           <g transform="scale(2)"><rect x="1" y="1" width="2" height="2" fill="red" filter="url(#f)"/></g>"#,
    );
    assert_eq!(at(&scaled, 6, 3), RED);
    assert_eq!(at(&scaled, 9, 3), RED);
    assert_eq!(at(&scaled, 3, 3), CLEAR);
    let turned = draw(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="-20" y="-20" width="40" height="40">
             <feOffset dx="5"/>
           </filter>
           <g transform="rotate(90 10 10)"><rect x="8" y="8" width="4" height="4" fill="red" filter="url(#f)"/></g>"#,
    );
    // Turned a quarter, "right" is down.
    assert_eq!(at(&turned, 10, 15), RED);
    assert_eq!(at(&turned, 15, 10), CLEAR);
}

// ─── When an element is not drawn, and when it is unfiltered ────────────────

/// **A filter naming no `<filter>` filters nothing**, and so does `none`;
/// a list with one such name in it is ignored whole.
#[test]
fn a_reference_to_nothing_filters_nothing() {
    for filter in ["url(#nope)", "none", "url(#f) url(#nope)", "blur(2em)"] {
        let image = square(r#"<filter id="f"><feOffset dx="5"/></filter>"#, filter);
        assert_eq!(at(&image, 5, 5), RED, "{filter}");
        assert_eq!(at(&image, 15, 10), CLEAR, "{filter}");
    }
}

/// **A `<filter>` with no primitive in it leaves the element undrawn**, as
/// does one measured against a box the element does not have.
#[test]
fn an_empty_filter_or_no_box_draws_nothing() {
    let empty = square(r#"<filter id="f"/>"#, "url(#f)");
    assert!(empty.iter().all(|px| *px == CLEAR));
    let line = draw(
        r#"<filter id="f"><feOffset/></filter>
           <line x1="2" y1="10" x2="18" y2="10" stroke="red" stroke-width="4" filter="url(#f)"/>"#,
    );
    assert!(line.iter().all(|px| *px == CLEAR), "a box with no height");
    let user = draw(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20"><feOffset/></filter>
           <line x1="2" y1="10" x2="18" y2="10" stroke="red" stroke-width="4" filter="url(#f)"/>"#,
    );
    assert_eq!(at(&user, 10, 10), RED, "measured in user units it is drawn");
}

/// **A list of filters applies each to what the one before made.**
#[test]
fn a_list_applies_in_turn() {
    let image = square(
        r#"<filter id="a"><feOffset dx="2"/></filter>
           <filter id="b"><feOffset dx="2"/></filter>"#,
        "url(#a) url(#b)",
    );
    assert_eq!(at(&image, 9, 10), RED);
    assert_eq!(at(&image, 15, 10), RED);
    assert_eq!(at(&image, 8, 10), CLEAR);
}

// ─── Filter functions ───────────────────────────────────────────────────────

/// **Each colour function, on red, as the specification's matrices and
/// tables make it.**
#[test]
fn the_colour_functions_on_red() {
    let center = |filter: &str| at(&square("", filter), 10, 10);
    assert!(
        near(center("grayscale(1)"), [54, 54, 54, 255], 1),
        "{:?}",
        center("grayscale(1)")
    );
    assert!(
        near(center("saturate(0)"), [54, 54, 54, 255], 1),
        "{:?}",
        center("saturate(0)")
    );
    assert_eq!(center("invert(100%)"), [0, 255, 255, 255]);
    assert_eq!(center("brightness(0)"), [0, 0, 0, 255]);
    assert!(near(center("contrast(0)"), [128, 128, 128, 255], 1));
    assert!(near(center("opacity(0.5)"), [255, 0, 0, 128], 1));
    let turned = center("hue-rotate(180deg)");
    assert_eq!(turned[0], 0);
    assert!(
        turned[1].abs_diff(109) <= 1 && turned[2].abs_diff(109) <= 1,
        "{turned:?}"
    );
    assert_eq!(center("hue-rotate(0.5turn)"), turned);
    // Functions chain: a red grey, inverted.
    assert!(near(
        center("grayscale(1) invert(1)"),
        [201, 201, 201, 255],
        1
    ));
    // An unreadable amount is no filter at all.
    assert_eq!(center("grayscale(-1)"), RED);
    // Contrast pivots on the middle: doubled, red stays red and no colour
    // comes up from nought.
    assert_eq!(center("contrast(2)"), RED);
    // An amount past whole is held to whole, where the function says so.
    assert_eq!(center("grayscale(2)"), center("grayscale(1)"));
    assert_eq!(center("invert(3)"), center("invert(1)"));
}

/// **`sepia()` on white is the specification's tint.**
#[test]
fn sepia_tints_white() {
    let image = draw(r#"<rect width="20" height="20" fill="white" filter="sepia(1)"/>"#);
    assert!(
        near(at(&image, 10, 10), [255, 255, 239, 255], 1),
        "{:?}",
        at(&image, 10, 10)
    );
}

/// **`drop-shadow()` reads its colour either side of its lengths**, and
/// its blur radius as twice the deviation.
#[test]
fn drop_shadow_function() {
    for filter in ["drop-shadow(2px 2px 0 blue)", "drop-shadow(blue 2px 2px)"] {
        let image = square("", filter);
        assert_eq!(at(&image, 10, 10), RED, "{filter}");
        assert_eq!(at(&image, 15, 15), [0, 0, 255, 255], "{filter}");
    }
    let soft = square("", "drop-shadow(0 0 4px black)");
    let edge = at(&soft, 15, 10)[3];
    assert!(edge > 0 && edge < 255, "{edge}");
}

/// **`blur()` softens the edge.**
/// **`drop-shadow()`'s blur is a radius, twice the deviation**: with 4px it
/// draws exactly what `feDropShadow` does with a deviation of 2, in sRGB.
#[test]
fn drop_shadow_function_blurs_by_half_its_radius() {
    let function = square("", "drop-shadow(1px 1px 4px black)");
    let primitive = square(
        r#"<filter id="f" color-interpolation-filters="sRGB">
             <feDropShadow dx="1" dy="1" stdDeviation="2" flood-color="black"/>
           </filter>"#,
        "url(#f)",
    );
    assert_eq!(function, primitive);
}

#[test]
fn blur_function_softens() {
    let image = square("", "blur(1px)");
    let edge = at(&image, 15, 10)[3];
    assert!(edge > 10 && edge < 200, "{edge}");
    assert!(at(&image, 10, 10)[3] > 240);
}

// ─── With clips, opacity and groups ─────────────────────────────────────────

/// **The clip cuts what the filter made**: a blur reaching past the clip is
/// not seen there -- clipped after the filter, as SVG orders them.
#[test]
fn the_clip_cuts_after_the_filter() {
    let image = draw(
        r#"<clipPath id="c"><rect x="0" y="0" width="12" height="20"/></clipPath>
           <filter id="f"><feGaussianBlur stdDeviation="1"/></filter>
           <rect x="5" y="5" width="10" height="10" fill="red" filter="url(#f)" clip-path="url(#c)"/>"#,
    );
    assert_eq!(at(&image, 12, 10), CLEAR);
    assert!(at(&image, 11, 10)[3] > 200);
    assert!(
        at(&image, 4, 10)[3] > 0,
        "the blur past the left edge, inside the clip"
    );
}

/// **Opacity applies to what the filter made.**
#[test]
fn opacity_after_the_filter() {
    let image = draw(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
             <feFlood flood-color="blue"/>
           </filter>
           <rect x="5" y="5" width="10" height="10" fill="red" filter="url(#f)" opacity="0.5"/>"#,
    );
    let px = at(&image, 1, 1);
    assert_eq!(px[2], 255);
    assert!(px[3].abs_diff(128) <= 1, "{px:?}");
}

/// **A filter on a group filters the group as a whole.**
#[test]
fn a_group_is_filtered_whole() {
    let image = draw(
        r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
             <feOffset dx="3"/>
           </filter>
           <g filter="url(#f)">
             <rect x="2" y="2" width="4" height="4" fill="red"/>
             <rect x="10" y="10" width="4" height="4" fill="blue"/>
           </g>"#,
    );
    assert_eq!(at(&image, 6, 3), RED);
    assert_eq!(at(&image, 14, 11), [0, 0, 255, 255]);
    assert_eq!(at(&image, 2, 3), CLEAR);
}

// ─── Budgets ────────────────────────────────────────────────────────────────

/// **Past the budget for scratch work a filtered element is not drawn**,
/// rather than drawn unfiltered or the drawing left unfinished.
#[test]
fn past_the_budget_a_filtered_element_is_not_drawn() {
    let svg = r#"<svg viewBox="0 0 20 20">
                   <filter id="f"><feOffset dx="1"/></filter>
                   <rect x="5" y="5" width="10" height="10" fill="red" filter="url(#f)"/>
                 </svg>"#;
    let doc = SvgDocument::parse(svg).unwrap();
    let mut renderer = SvgRenderer::new(20, 20, &doc);
    renderer.scratch_budget = 0;
    let image = pixels(&doc.draw(renderer));
    assert!(image.iter().all(|px| *px == CLEAR));
}

/// **A region larger than the cap is filtered at a lower resolution and
/// scaled up**: a flood still fills it, to within the sampling of its edge.
#[test]
fn a_large_region_is_filtered_smaller() {
    let svg = r#"<svg viewBox="0 0 20 20">
                   <filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="20">
                     <feFlood flood-color="blue"/>
                   </filter>
                   <rect x="5" y="5" width="10" height="10" fill="red" filter="url(#f)"/>
                 </svg>"#;
    let doc = SvgDocument::parse(svg).unwrap();
    let mut renderer = SvgRenderer::new(20, 20, &doc);
    renderer.filter_pixels = 64;
    let image = pixels(&doc.draw(renderer));
    for (x, y) in [(5, 5), (10, 10), (14, 3)] {
        assert_eq!(at(&image, x, y), [0, 0, 255, 255], "({x}, {y})");
    }
}

/// **Many filtered elements finish**: the budget bounds the work, however
/// many there are.
#[test]
fn many_filtered_elements_finish() {
    let mut body = String::from(r#"<filter id="f"><feGaussianBlur stdDeviation="3"/></filter>"#);
    for i in 0..2000 {
        body.push_str(&format!(
            r#"<rect x="{}" y="{}" width="3" height="3" fill="red" filter="url(#f)"/>"#,
            i % 17,
            (i / 17) % 17
        ));
    }
    let image = draw(&body);
    assert_eq!(image.len(), 400);
}

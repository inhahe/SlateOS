//! Tests for what decides how much of a shape is painted, and how: links
//! and switches, conditions, `visibility`, `paint-order` and
//! `vector-effect: non-scaling-stroke`.
//!
//! The drawings are 20 by 20 user units on 20 by 20 pixels unless they say.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::{PaintOrder, PaintPart, SvgDocument};

fn draw_at(body: &str, px: u32) -> Vec<[u8; 4]> {
    let svg = format!(r#"<svg viewBox="0 0 20 20" width="20" height="20">{body}</svg>"#);
    SvgDocument::parse(&svg)
        .unwrap()
        .render(px, px)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

fn draw(body: &str) -> Vec<[u8; 4]> {
    draw_at(body, 20)
}

fn at(image: &[[u8; 4]], x: usize, y: usize) -> [u8; 4] {
    let side = (image.len() as f64).sqrt() as usize;
    image[y * side + x]
}

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

fn blank(image: &[[u8; 4]]) -> bool {
    image.iter().all(|px| *px == CLEAR)
}

// ─── Links, switches and conditions ─────────────────────────────────────────

/// **A link is drawn as a group**: what it holds, moved and styled by it.
#[test]
fn a_link_draws_what_it_holds() {
    let image = draw(
        r#"<a href="https://example.com" transform="translate(10 0)" fill="red">
             <rect width="5" height="5"/>
           </a>"#,
    );
    assert_eq!(at(&image, 12, 2), RED);
    assert_eq!(at(&image, 2, 2), CLEAR);
}

/// **A switch draws the first child whose conditions hold, and none of the
/// rest** -- skipping one asking for an extension, one in a language
/// nobody speaks, and a title.
#[test]
fn a_switch_draws_its_first_child_that_holds() {
    let image = draw(
        r#"<switch>
             <rect requiredExtensions="http://ns.adobe.com/AdobeIllustrator/10.0/" width="20" height="20" fill="lime"/>
             <rect systemLanguage="x-nobody" width="20" height="20" fill="yellow"/>
             <title>Not a candidate</title>
             <rect width="20" height="20" fill="blue"/>
             <rect width="20" height="20" fill="red"/>
           </switch>"#,
    );
    assert_eq!(at(&image, 10, 10), BLUE);
    // A switch is a group: its own transform carries the child.
    let moved = draw(
        r#"<switch transform="translate(10 0)"><rect width="5" height="5" fill="red"/></switch>"#,
    );
    assert_eq!(at(&moved, 12, 2), RED);
    assert_eq!(at(&moved, 2, 2), CLEAR);
}

/// **A child chosen and then not drawable here draws nothing**; the switch
/// does not move on to the next.
#[test]
fn a_switch_does_not_pass_over_what_it_chose() {
    let image = draw(
        r#"<switch>
             <foreignObject width="20" height="20"><p xmlns="http://www.w3.org/1999/xhtml">Hi</p></foreignObject>
             <rect width="20" height="20" fill="red"/>
           </switch>"#,
    );
    assert!(blank(&image));
}

/// **An element whose conditions do not hold is not drawn anywhere**, not
/// only in a switch.
#[test]
fn conditions_hold_outside_a_switch() {
    assert!(blank(&draw(
        r#"<rect requiredExtensions="http://example.com/x" width="20" height="20" fill="red"/>"#
    )));
    assert!(blank(&draw(
        r#"<g systemLanguage="x-nobody"><rect width="20" height="20" fill="red"/></g>"#
    )));
}

// ─── Visibility ─────────────────────────────────────────────────────────────

/// **A hidden shape is not drawn -- its markers neither -- but a hidden
/// group's child that says `visible` is**; `collapse` hides as `hidden`
/// does, and from a style as from an attribute.
#[test]
fn hidden_is_not_drawn_but_a_child_may_show() {
    assert!(blank(&draw(
        r#"<rect visibility="hidden" width="20" height="20" fill="red"/>"#
    )));
    assert!(blank(&draw(
        r#"<rect style="visibility: collapse" width="20" height="20" fill="red"/>"#
    )));
    let image = draw(
        r#"<g visibility="hidden">
             <rect width="10" height="20" fill="red"/>
             <rect x="10" width="10" height="20" fill="blue" visibility="visible"/>
           </g>"#,
    );
    assert_eq!(at(&image, 5, 10), CLEAR);
    assert_eq!(at(&image, 15, 10), BLUE);
    let marked = draw(
        r#"<marker id="m" markerUnits="userSpaceOnUse" markerWidth="6" markerHeight="6" refX="3" refY="3">
             <rect width="6" height="6" fill="red"/>
           </marker>
           <line x1="5" y1="10" x2="15" y2="10" stroke="black" marker-end="url(#m)" visibility="hidden"/>"#,
    );
    assert!(blank(&marked), "a hidden shape's markers");
}

// ─── Paint order ────────────────────────────────────────────────────────────

/// **`paint-order` reads as one to three parts, the rest in their normal
/// order**; a part twice or a word it does not know is not said.
#[test]
fn paint_order_reads() {
    use PaintPart::{Fill, Markers, Stroke};
    assert_eq!(PaintOrder::parse("normal"), Some(PaintOrder::NORMAL));
    assert_eq!(
        PaintOrder::parse("stroke"),
        Some(PaintOrder([Stroke, Fill, Markers]))
    );
    assert_eq!(
        PaintOrder::parse(" markers  stroke "),
        Some(PaintOrder([Markers, Stroke, Fill]))
    );
    assert_eq!(
        PaintOrder::parse("fill stroke markers"),
        Some(PaintOrder::NORMAL)
    );
    assert_eq!(PaintOrder::parse("fill fill"), None);
    assert_eq!(PaintOrder::parse("strokes"), None);
    assert_eq!(PaintOrder::parse(""), None);
}

/// **The stroke painted first lies under the fill**: the inner half of a
/// thick stroke is covered by the fill, the outer half still shows.
#[test]
fn a_stroke_painted_first_lies_under_the_fill() {
    let rect = |order: &str| {
        draw(&format!(
            r#"<rect x="5" y="5" width="10" height="10" fill="red" stroke="blue" stroke-width="4" {order}/>"#
        ))
    };
    let normal = rect("");
    assert_eq!(at(&normal, 6, 10), BLUE, "over the fill");
    let under = rect(r#"paint-order="stroke""#);
    assert_eq!(at(&under, 6, 10), RED, "under it");
    assert_eq!(at(&under, 3, 10), BLUE, "still outside it");
    // Inherited from a group.
    let inherited = draw(
        r#"<g paint-order="stroke"><rect x="5" y="5" width="10" height="10" fill="red" stroke="blue" stroke-width="4"/></g>"#,
    );
    assert_eq!(at(&inherited, 6, 10), RED);
}

// ─── Non-scaling strokes ────────────────────────────────────────────────────

/// **A non-scaling stroke is as wide as it says in the host's pixels,
/// whatever transforms the shape** -- and the host's pixels grow with the
/// drawing.
#[test]
fn a_non_scaling_stroke_keeps_its_width() {
    let line = |effect: &str, px: u32| {
        draw_at(
            &format!(
                r#"<line x1="0" y1="0" x2="20" y2="0" stroke="blue" stroke-width="2" transform="translate(0 10) scale(1 4)" {effect}/>"#
            ),
            px,
        )
    };
    // Scaled: 2 by the transform's 2 (the square root of 1 by 4) is 4 wide.
    let scaled = line("", 20);
    assert_eq!(at(&scaled, 10, 8), BLUE);
    let kept = line(r#"vector-effect="non-scaling-stroke""#, 20);
    assert_eq!(at(&kept, 10, 9), BLUE);
    assert_eq!(at(&kept, 10, 8), CLEAR, "2 wide, not 4");
    // Drawn twice as large, the host's pixel is two of these.
    let doubled = line(r#"vector-effect="non-scaling-stroke""#, 40);
    assert_eq!(at(&doubled, 20, 18), BLUE);
    assert_eq!(at(&doubled, 20, 17), CLEAR);
    // `none` is a stroke that scales.
    let none = line(r#"vector-effect="none""#, 20);
    assert_eq!(at(&none, 10, 8), BLUE);
}

/// **A non-scaling stroke's dashes are measured in the host's space too.**
#[test]
fn a_non_scaling_strokes_dashes_keep_their_length() {
    let line = |effect: &str| {
        draw(&format!(
            r#"<line x1="0" y1="10" x2="5" y2="10" stroke="blue" stroke-width="2" stroke-dasharray="2 2" transform="scale(4 1)" {effect}/>"#
        ))
    };
    let scaled = line("");
    assert_eq!(at(&scaled, 3, 10), BLUE, "a dash of 2 is 8 pixels");
    let kept = line(r#"vector-effect="non-scaling-stroke""#);
    assert_eq!(at(&kept, 3, 10), CLEAR, "a gap from 2 to 4");
    assert_eq!(at(&kept, 5, 10), BLUE);
}

/// **A non-scaling stroke on a layer is not cut to the shape's shrunken
/// size**: the layer reaches as far as the stroke does.
#[test]
fn a_non_scaling_stroke_fits_its_layer() {
    let image = draw(
        r#"<g opacity="0.5">
             <rect x="50" y="50" width="100" height="100" fill="red" stroke="blue" stroke-width="8"
                   transform="scale(0.1)" vector-effect="non-scaling-stroke"/>
           </g>"#,
    );
    // The rectangle is 5 to 15; the stroke reaches 4 past its edge -- where
    // a layer sized by the shape's own scale stopped short of it.
    let outside = at(&image, 1, 10);
    assert!(outside[3] > 0 && outside[2] > 0, "{outside:?}");
}

// ─── Blending ───────────────────────────────────────────────────────────────

/// A blue square over the drawing's left half, which is red, mixed by
/// `mode`; `wrap` round it.
fn mixed(mode: &str, wrap: (&str, &str)) -> Vec<[u8; 4]> {
    draw(&format!(
        r#"<rect width="10" height="20" fill="red"/>
           {}<rect x="5" y="5" width="10" height="10" fill="blue" mix-blend-mode="{mode}"/>{}"#,
        wrap.0, wrap.1
    ))
}

/// **A blend mode mixes what an element draws with what is under it**:
/// blue multiplied over red is black, screened is magenta -- and over
/// nothing, blue is blue.
#[test]
fn a_blend_mode_mixes_with_what_is_under_it() {
    let multiply = mixed("multiply", ("", ""));
    assert_eq!(at(&multiply, 7, 10), [0, 0, 0, 255]);
    assert_eq!(at(&multiply, 12, 10), BLUE, "over nothing");
    assert_eq!(at(&multiply, 2, 10), RED, "beside it");
    let screen = mixed("screen", ("", ""));
    assert_eq!(at(&screen, 7, 10), [255, 0, 255, 255]);
    // In a group that makes no layer, the backdrop is still the drawing.
    let grouped = mixed("multiply", ("<g>", "</g>"));
    assert_eq!(at(&grouped, 7, 10), [0, 0, 0, 255]);
}

/// **`isolation: isolate` keeps what a group holds from mixing with what is
/// outside it**: the blue multiplies with nothing in the group, and is laid
/// on the red as it is.
#[test]
fn isolation_keeps_a_blend_inside() {
    let image = mixed("multiply", (r#"<g isolation="isolate">"#, "</g>"));
    assert_eq!(at(&image, 7, 10), BLUE);
}

/// **A half-transparent source mixes as Compositing and Blending says**:
/// half the backdrop shows through, and the other half is the mix.
#[test]
fn a_half_transparent_source_mixes_by_half() {
    let image = draw(
        r#"<rect width="20" height="20" fill="red"/>
           <rect width="20" height="20" fill="blue" fill-opacity="0.5" mix-blend-mode="multiply"/>"#,
    );
    let px = at(&image, 10, 10);
    assert!(
        px[0].abs_diff(128) <= 1 && px[1] == 0 && px[2] <= 1,
        "{px:?}"
    );
    assert_eq!(px[3], 255);
}

/// **A blend mode it does not know is `normal`**, and so is none.
#[test]
fn an_unknown_blend_mode_is_normal() {
    assert_eq!(at(&mixed("plus-darker", ("", "")), 7, 10), BLUE);
    assert_eq!(at(&mixed("normal", ("", "")), 7, 10), BLUE);
}

/// **A filtered element mixes too, after its filter.**
#[test]
fn a_filtered_element_mixes() {
    let image = draw(
        r#"<rect width="20" height="20" fill="red"/>
           <filter id="f"><feOffset dx="5" dy="0"/></filter>
           <rect width="10" height="20" fill="blue" filter="url(#f)" mix-blend-mode="multiply"/>"#,
    );
    // The filter region is the square's box grown by a tenth: -1 to 11.
    assert_eq!(at(&image, 2, 10), RED, "the filter moved it from here");
    assert_eq!(at(&image, 8, 10), [0, 0, 0, 255], "to here, multiplied");
}

/// **The element's own opacity fades what it mixes**: half of it is the
/// mix, half the backdrop showing through -- as for a half-transparent
/// paint.
#[test]
fn an_elements_opacity_fades_its_mix() {
    let image = draw(
        r#"<rect width="20" height="20" fill="red"/>
           <rect width="20" height="20" fill="blue" opacity="0.5" mix-blend-mode="multiply"/>"#,
    );
    let px = at(&image, 10, 10);
    assert!(
        px[0].abs_diff(128) <= 1 && px[1] == 0 && px[2] <= 1,
        "{px:?}"
    );
}

/// **Over a half-transparent backdrop, a half-transparent source mixes
/// where they overlap and each shows where the other does not**: red at
/// half under blue at half, multiplied, is a quarter mix (black), a quarter
/// red and a quarter blue -- purple at three quarters' cover.
#[test]
fn half_transparent_layers_mix_by_their_cover() {
    let image = draw(
        r#"<rect width="20" height="20" fill="red" fill-opacity="0.5"/>
           <rect width="20" height="20" fill="blue" fill-opacity="0.5" mix-blend-mode="multiply"/>"#,
    );
    let px = at(&image, 10, 10);
    for (got, want) in px.iter().zip([85u8, 0, 85, 191]) {
        assert!(got.abs_diff(want) <= 1, "{px:?}");
    }
}

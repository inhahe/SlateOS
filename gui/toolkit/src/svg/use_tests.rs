//! Tests for `<use>` and `<symbol>`: an element drawn again where a `<use>`
//! names it, and what keeps a document from drawing itself without end.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::SvgDocument;

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

/// How many pixels of `svg`, drawn `w` by `h`, are painted at all.
fn painted(svg: &str, w: u32, h: u32) -> usize {
    alphas(svg, w, h).iter().filter(|&&a| a > 0).count()
}

const RED_SQUARE: &str = r#"<defs><rect id="r" width="5" height="5" fill="red"/></defs>"#;

/// **A `<use>` draws the element it names at its `x` and `y`**, and the
/// definition itself is not drawn where it stands.
#[test]
fn a_use_draws_what_it_names_where_it_says() {
    let svg =
        format!(r##"<svg viewBox="0 0 20 10">{RED_SQUARE}<use href="#r" x="10" y="2"/></svg>"##);
    assert_eq!(px(&svg, 20, 10, 12, 4), [255, 0, 0, 255]);
    assert_eq!(painted(&svg, 20, 10), 25);
    // SVG 1.1's `xlink:href` too; where both are written, SVG 2's `href`
    // wins.
    let xlink =
        format!(r##"<svg viewBox="0 0 20 10">{RED_SQUARE}<use xlink:href="#r" x="10"/></svg>"##);
    assert_eq!(px(&xlink, 20, 10, 12, 2), [255, 0, 0, 255]);
    let both = format!(
        r##"<svg viewBox="0 0 20 10">{RED_SQUARE}<rect id="b" width="5" height="5" fill="blue" x="100"/>
<use href="#r" xlink:href="#b" x="10"/></svg>"##
    );
    assert_eq!(px(&both, 20, 10, 12, 2), [255, 0, 0, 255]);
    // A percentage is of the viewport.
    let percent =
        format!(r##"<svg viewBox="0 0 20 10">{RED_SQUARE}<use href="#r" x="50%"/></svg>"##);
    assert_eq!(px(&percent, 20, 10, 12, 2), [255, 0, 0, 255]);
}

/// **The `<use>` is the parent of what it draws**: the content inherits its
/// style, and keeps its own where it has one; the `<use>`'s transform holds
/// its `x` and `y`.
#[test]
fn what_a_use_draws_inherits_from_it() {
    let plain = r##"<svg viewBox="0 0 10 10"><defs><rect id="r" width="5" height="5"/></defs>
<use href="#r" fill="lime"/></svg>"##;
    assert_eq!(px(plain, 10, 10, 2, 2), [0, 255, 0, 255]);
    let own =
        format!(r##"<svg viewBox="0 0 10 10">{RED_SQUARE}<use href="#r" fill="lime"/></svg>"##);
    assert_eq!(px(&own, 10, 10, 2, 2), [255, 0, 0, 255]);
    // translate(x, y) inside the transform: scale(2) of x=1 is at 2.
    let scaled = format!(
        r##"<svg viewBox="0 0 20 20">{RED_SQUARE}<use href="#r" x="1" transform="scale(2)"/></svg>"##
    );
    let drawn = alphas(&scaled, 20, 20);
    assert_eq!(
        (drawn[5 * 20 + 1], drawn[5 * 20 + 2], drawn[5 * 20 + 11]),
        (0, 255, 255)
    );
    assert_eq!(drawn[5 * 20 + 12], 0);
}

/// **An element drawn where it stands may be used too**, and is drawn both
/// times; one element used many times is drawn every time.
#[test]
fn an_element_may_be_drawn_and_used_and_used_again() {
    let svg = r##"<svg viewBox="0 0 20 10"><rect id="r" width="5" height="5" fill="red"/>
<use href="#r" x="5"/><use href="#r" x="10"/><use href="#r" x="15"/></svg>"##;
    assert_eq!(painted(svg, 20, 10), 100);
}

/// **A `<symbol>` is shown in a viewport the `<use>` sizes**: its view box
/// fitted to the `<use>`'s `width` and `height` -- the whole viewport where
/// those are not said -- as its `preserveAspectRatio` says.
#[test]
fn a_symbol_is_shown_in_the_viewport_a_use_gives_it() {
    let symbol =
        r#"<symbol id="s" viewBox="0 0 1 1"><rect width="1" height="1" fill="red"/></symbol>"#;
    let sized = format!(
        r##"<svg viewBox="0 0 20 20">{symbol}<use href="#s" x="10" width="10" height="10"/></svg>"##
    );
    let drawn = alphas(&sized, 20, 20);
    assert_eq!(
        (drawn[5 * 20 + 15], drawn[5 * 20 + 5], drawn[15 * 20 + 15]),
        (255, 0, 0)
    );
    let whole = format!(r##"<svg viewBox="0 0 20 20">{symbol}<use href="#s"/></svg>"##);
    assert_eq!(painted(&whole, 20, 20), 400);
    // A box twice as wide as high, fitted to a square: centred, half high.
    let wide = r##"<svg viewBox="0 0 10 10"><symbol id="s" viewBox="0 0 2 1">
<rect width="2" height="1" fill="red"/></symbol><use href="#s" width="10" height="10"/></svg>"##;
    let drawn = alphas(wide, 10, 10);
    assert_eq!(
        (drawn[5 * 10 + 5], drawn[10 + 5], drawn[8 * 10 + 5]),
        (255, 0, 0)
    );
    // A viewport with no area shows nothing.
    let none = format!(r##"<svg viewBox="0 0 20 20">{symbol}<use href="#s" width="0"/></svg>"##);
    assert_eq!(painted(&none, 20, 20), 0);
}

/// **A `<symbol>` with no view box is not scaled**, only moved; and its own
/// style reaches what it holds.
#[test]
fn a_symbol_without_a_view_box_is_only_moved() {
    let svg = r##"<svg viewBox="0 0 20 20"><symbol id="s" fill="blue"><rect width="5" height="5"/></symbol>
<use href="#s" x="10" y="10" width="8" height="8"/></svg>"##;
    assert_eq!(px(svg, 20, 20, 12, 12), [0, 0, 255, 255]);
    assert_eq!(painted(svg, 20, 20), 25);
}

/// **A `<use>` of an `<svg>` shows it in its own viewport**, the `<use>`'s
/// size winning over the `<svg>`'s.
#[test]
fn a_use_of_an_svg_shows_it_in_its_viewport() {
    let svg = r##"<svg viewBox="0 0 20 20"><defs><svg id="v" x="2" width="4" height="4" viewBox="0 0 1 1">
<rect width="1" height="1" fill="red"/></svg></defs><use href="#v" width="10" height="10"/></svg>"##;
    // At x 2 (the <svg>'s), 10 wide (the <use>'s).
    let drawn = alphas(svg, 20, 20);
    assert_eq!(
        (
            drawn[5 * 20 + 1],
            drawn[5 * 20 + 3],
            drawn[5 * 20 + 11],
            drawn[5 * 20 + 12]
        ),
        (0, 255, 255, 0)
    );
}

/// **A `<use>` of what is not there draws nothing**: a name in no element,
/// one in another file, an element that draws nothing, and one hidden.
#[test]
fn a_use_of_what_is_not_there_draws_nothing() {
    for (defs, href) in [
        ("", "#missing"),
        (RED_SQUARE, "other.svg#r"),
        (RED_SQUARE, "#"),
        (
            r#"<linearGradient id="g"><stop stop-color="red"/></linearGradient>"#,
            "#g",
        ),
        (
            r#"<rect id="h" width="5" height="5" display="none"/>"#,
            "#h",
        ),
        (
            r#"<symbol id="h" style="display:none"><rect width="5" height="5"/></symbol>"#,
            "#h",
        ),
    ] {
        let svg = format!(r#"<svg viewBox="0 0 10 10">{defs}<use href="{href}"/></svg>"#);
        assert_eq!(painted(&svg, 10, 10), 0, "{href} with {defs}");
    }
}

/// **What a `<use>` names but cannot be built draws nothing**, and costs the
/// document nothing else: before `<use>` was read, a bad path in a `<defs>`
/// drew nothing and failed nothing.
#[test]
fn a_use_of_what_cannot_be_built_draws_nothing() {
    let svg = format!(
        r##"<svg viewBox="0 0 10 10"><defs><path id="p" d="M 0 0 L nonsense"/></defs>{RED_SQUARE}
<use href="#p"/><use href="#r"/></svg>"##
    );
    assert_eq!(painted(&svg, 10, 10), 25);
}

/// **A `<use>` inside what it names draws nothing**, as browsers draw
/// nothing for it -- the element itself still drawn where it stands.
#[test]
fn a_use_inside_what_it_names_draws_nothing() {
    let svg = r##"<svg viewBox="0 0 20 10"><g id="a"><rect width="5" height="5" fill="red"/>
<g><use href="#a" x="10"/></g></g></svg>"##;
    assert_eq!(painted(svg, 20, 10), 25);
    let itself = r##"<svg viewBox="0 0 20 10">{RED}<use id="u" href="#u"/></svg>"##
        .replace("{RED}", RED_SQUARE);
    assert_eq!(painted(&itself, 20, 10), 0);
}

/// **Content shown inside itself through other `<use>`s is drawn once**:
/// `a` shows `b`, which shows `a` -- the loop is broken where it closes,
/// and drawing ends.
#[test]
fn a_loop_of_uses_is_drawn_once_round() {
    let svg = r##"<svg viewBox="0 0 20 20"><defs>
<g id="a"><rect width="5" height="5" fill="red"/><use href="#b" x="5"/></g>
<g id="b"><rect width="5" height="5" fill="blue"/><use href="#a" y="5"/></g>
</defs><use href="#a"/></svg>"##;
    // a's square at (0, 0) and b's at (5, 0); b's `<use>` of a, which a is
    // being drawn for, closes the loop and draws nothing at (5, 5).
    assert_eq!(px(svg, 20, 20, 2, 2), [255, 0, 0, 255]);
    assert_eq!(px(svg, 20, 20, 7, 2), [0, 0, 255, 255]);
    assert_eq!(painted(svg, 20, 20), 50);
}

/// `levels` groups, each using the one before ten times, the first a
/// square: ten to the `levels` squares, from a few dozen elements.
fn laughs(levels: usize) -> String {
    let mut svg =
        String::from(r#"<svg viewBox="0 0 10 10"><defs><rect id="l0" width="1" height="1"/>"#);
    for level in 1..=levels {
        svg.push_str(&format!(r#"<g id="l{level}">"#));
        for _ in 0..10 {
            svg.push_str(&format!(r##"<use href="#l{}"/>"##, level - 1));
        }
        svg.push_str("</g>");
    }
    svg.push_str(&format!(r##"</defs><use href="#l{levels}"/></svg>"##));
    svg
}

/// **A document cannot multiply itself without end**: ten uses of ten uses
/// of ten... nine deep is a billion squares from a hundred elements. What
/// it draws is bounded, so drawing it ends -- where it would not, in any
/// time anyone would wait.
#[test]
fn a_document_cannot_multiply_itself_without_end() {
    let svg = laughs(9);
    let doc = SvgDocument::parse(&svg).unwrap();
    // Drawing returns; the square is there.
    assert_eq!(doc.render(10, 10)[3], 255);
    // And a few levels, well under the bound, are drawn in full: a
    // hundred squares one on another at the origin.
    let few = SvgDocument::parse(&laughs(2)).unwrap();
    assert_eq!(few.render(10, 10)[3], 255);
}

/// **A chain of `<use>`s naming `<use>`s ends**: each element using the
/// one before, three hundred long -- deeper than drawing goes -- is drawn
/// as deep as it goes, within half the stack a Windows program's main
/// thread has, in a debug build.
#[test]
fn a_long_chain_of_uses_ends() {
    let mut svg = String::from(
        r#"<svg viewBox="0 0 10 10"><defs><rect id="c0" width="1" height="1" fill="red"/>"#,
    );
    for link in 1..300 {
        svg.push_str(&format!(r##"<use id="c{link}" href="#c{}"/>"##, link - 1));
    }
    svg.push_str(r##"</defs><use href="#c20"/><use href="#c299" x="5"/></svg>"##);
    let drawn = std::thread::Builder::new()
        .stack_size(512 << 10)
        .spawn(move || SvgDocument::parse(&svg).unwrap().render(10, 10))
        .unwrap()
        .join()
        .unwrap();
    // Twenty links down, the square; three hundred down, past where drawing
    // goes, nothing.
    assert_eq!(&drawn[..4], &[255, 0, 0, 255]);
    assert_eq!(drawn[5 * 4 + 3], 0);
}

/// **A `<use>` names the first element with its `id`**, as
/// `getElementById` finds it, and one inside any element it names -- not
/// only the nearest with an `id` -- draws nothing.
#[test]
fn a_use_names_the_first_and_never_what_holds_it() {
    let twice = r##"<svg viewBox="0 0 10 10"><defs><rect id="r" width="5" height="5" fill="red"/>
<rect id="r" width="5" height="5" fill="blue"/></defs><use href="#r"/></svg>"##;
    assert_eq!(px(twice, 10, 10, 2, 2), [255, 0, 0, 255]);
    let outer = r##"<svg viewBox="0 0 20 10"><g id="a"><rect width="5" height="5" fill="red"/>
<g id="b"><use href="#a" x="10"/></g></g></svg>"##;
    assert_eq!(painted(outer, 20, 10), 25);
}

/// **A `<symbol>` is cut to its viewport**, as SVG's own style sheet has it
/// -- unless it says `overflow: visible` -- and what is cut is the
/// viewport, not the view box: room the view box leaves inside the viewport
/// is drawn in.
#[test]
fn a_symbol_is_cut_to_its_viewport() {
    let make = |overflow: &str| {
        format!(
            r##"<svg viewBox="0 0 20 20"><symbol id="s" {overflow}><rect width="5" height="5"/></symbol>
<use href="#s" x="10" y="10" width="2" height="2"/></svg>"##
        )
    };
    assert_eq!(painted(&make(""), 20, 20), 4);
    assert_eq!(painted(&make(r#"overflow="visible""#), 20, 20), 25);
    assert_eq!(painted(&make(r#"style="overflow:auto""#), 20, 20), 25);
    // A box twice as wide as high in a square viewport leaves room above
    // and below it; a rect overflowing the box into that room is drawn there.
    let roomy = r##"<svg viewBox="0 0 10 10"><symbol id="s" viewBox="0 0 2 1">
<rect x="-1" y="-1" width="4" height="3"/></symbol><use href="#s" width="10" height="10"/></svg>"##;
    assert_eq!(painted(roomy, 10, 10), 100);
}

/// **A symbol's cut ends with it**: what follows a `<use>` of a symbol is
/// not cut to the symbol's viewport.
#[test]
fn a_symbols_cut_ends_with_it() {
    let svg = r##"<svg viewBox="0 0 20 20"><symbol id="s"><rect width="5" height="5"/></symbol>
<use href="#s" width="2" height="2"/><rect x="10" y="10" width="5" height="5"/></svg>"##;
    // The symbol's square cut to its 2 by 2 viewport, then the whole square.
    assert_eq!(painted(svg, 20, 20), 4 + 25);
}

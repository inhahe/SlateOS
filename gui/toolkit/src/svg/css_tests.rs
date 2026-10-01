//! Tests for style sheets: documents drawn, and the selectors and the cascade
//! read from them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::super::SvgDocument;
use super::{Combinator, Compound, Facts, Selector, Sheet, matches};

/// The colour of the first pixel of `svg` drawn 4 by 4, a square filling it.
fn colour(svg: &str) -> [u8; 4] {
    let p = SvgDocument::parse(svg).unwrap().render(4, 4);
    [p[0], p[1], p[2], p[3]]
}

/// A 4 by 4 drawing with `sheet` in a `<style>`, then `body`.
fn styled(sheet: &str, body: &str) -> String {
    format!(r#"<svg viewBox="0 0 4 4"><style>{sheet}</style>{body}</svg>"#)
}

const RED: [u8; 4] = [255, 0, 0, 255];
const LIME: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// **A class in a sheet colours what wears it**, as Illustrator writes
/// every drawing: where the renderer drew such a shape in the default black.
#[test]
fn a_class_in_a_sheet_colours_what_wears_it() {
    let svg = styled(
        ".st0{fill:#ff0000}",
        r#"<rect class="st0" width="4" height="4"/>"#,
    );
    assert_eq!(colour(&svg), RED);
    // A sheet in a CDATA section -- its `>` the child combinator, not the
    // end of anything -- and one written with entities.
    let cdata = r#"<svg viewBox="0 0 4 4"><style><![CDATA[ g > .a { fill: lime } ]]></style>
<g><rect class="a" width="4" height="4"/></g></svg>"#;
    assert_eq!(colour(cdata), LIME);
    let entities = styled(
        "g &gt; .a { fill: blue }",
        r#"<g><rect class="a" width="4" height="4"/></g>"#,
    );
    assert_eq!(colour(&entities), BLUE);
}

/// **The cascade is CSS's**: the element's own `style` over the sheet, the
/// sheet over a presentation attribute, the more specific rule over the
/// less, the later over the earlier, and `!important` over the element's
/// own `style`.
#[test]
fn the_cascade_is_csss() {
    let rect = |attrs: &str| format!(r#"<rect id="r" class="a" width="4" height="4" {attrs}/>"#);
    let own = styled(".a{fill:red}", &rect(r#"style="fill:lime""#));
    assert_eq!(colour(&own), LIME);
    let attribute = styled(".a{fill:red}", &rect(r#"fill="lime""#));
    assert_eq!(colour(&attribute), RED);
    let specific = styled("#r{fill:lime} .a{fill:red} rect{fill:blue}", &rect(""));
    assert_eq!(colour(&specific), LIME);
    let later = styled(".a{fill:red} .a{fill:blue}", &rect(""));
    assert_eq!(colour(&later), BLUE);
    let important = styled(".a{fill:red !important}", &rect(r#"style="fill:lime""#));
    assert_eq!(colour(&important), RED);
}

/// **Every selector this reads selects**: `*`, a type, an id, a compound, a
/// comma list -- and one it cannot read selects nothing while the rest of
/// its list still does.
#[test]
fn every_selector_this_reads_selects() {
    let shape = r#"<rect id="r" class="a b" width="4" height="4"/>"#;
    for sheet in [
        "*{fill:lime}",
        "rect{fill:lime}",
        "#r{fill:lime}",
        "rect.a.b#r{fill:lime}",
        "circle, .b{fill:lime}",
        "rect:hover, .a{fill:lime}",
    ] {
        assert_eq!(colour(&styled(sheet, shape)), LIME, "{sheet}");
    }
    for sheet in [
        "circle{fill:lime}",
        ".c{fill:lime}",
        "rect.a.c{fill:lime}",
        "rect[width]{fill:lime}",
        "rect:hover{fill:lime}",
        "g + rect{fill:lime}",
    ] {
        assert_eq!(colour(&styled(sheet, shape)), [0, 0, 0, 255], "{sheet}");
    }
}

/// **A descendant combinator reaches any depth, a child one only a step**.
#[test]
fn descendant_and_child_combinators() {
    let nested = r#"<g class="outer"><g><rect class="a" width="4" height="4"/></g></g>"#;
    assert_eq!(colour(&styled(".outer .a{fill:lime}", nested)), LIME);
    assert_eq!(
        colour(&styled(".outer > .a{fill:lime}", nested)),
        [0, 0, 0, 255]
    );
    assert_eq!(colour(&styled(".outer > g > .a{fill:lime}", nested)), LIME);
}

/// **What a sheet does not say is left alone**: comments, at-rules -- a
/// `@media` block not evaluated -- and a `<style>` of another type.
#[test]
fn what_a_sheet_does_not_say_is_left_alone() {
    let shape = r#"<rect class="a" width="4" height="4"/>"#;
    let commented = styled("/* .a{fill:red} */ .a{fill:/*x*/lime}", shape);
    assert_eq!(colour(&commented), LIME);
    let media = styled(
        "@import url(x.css); @media (prefers-color-scheme: dark) { .a{fill:red} } .a{fill:lime}",
        shape,
    );
    assert_eq!(colour(&media), LIME);
    let other = format!(
        r#"<svg viewBox="0 0 4 4"><style type="text/sass">.a{{fill:red}}</style>{shape}</svg>"#
    );
    assert_eq!(colour(&other), [0, 0, 0, 255]);
}

/// **A sheet styles what is read before shapes are**: a gradient's stops.
#[test]
fn a_sheet_styles_a_gradients_stops() {
    let svg = r#"<svg viewBox="0 0 4 4"><style>stop{stop-color:lime}</style>
<linearGradient id="g"><stop offset="0"/><stop offset="1"/></linearGradient>
<rect width="4" height="4" fill="url(#g)"/></svg>"#;
    assert_eq!(colour(svg), LIME);
}

fn facts(tag: &str, id: Option<&str>, classes: &[&str]) -> Facts {
    Facts {
        tag: tag.to_owned(),
        id: id.map(str::to_owned),
        classes: classes.iter().map(|c| (*c).to_owned()).collect(),
    }
}

/// **A selector is read rightmost first, with its specificity**, and one
/// it cannot read is none at all.
#[test]
fn a_selector_is_read_rightmost_first() {
    let s = Selector::parse("g.x > rect#r.a").unwrap();
    assert_eq!(s.combinators, [Combinator::Child]);
    assert_eq!(
        s.compounds[0],
        Compound {
            tag: Some("rect".into()),
            id: Some("r".into()),
            classes: vec!["a".into()]
        }
    );
    assert_eq!(s.specificity(), (1, 2, 2));
    for bad in [
        "",
        "> a",
        "a >",
        "a > > b",
        "a#b#c",
        ".1x",
        "a\\b",
        "*|a",
        "a b c d e f g h i",
    ] {
        assert!(Selector::parse(bad).is_none(), "{bad:?}");
    }
}

/// **Matching tries every way through the ancestors**, not only the
/// nearest: `.x > .y .z` matches a `.z` inside a `.y` inside a `.x`, though
/// the `.y` nearest the `.z` is inside another `.y`.
#[test]
fn matching_tries_every_way_through_the_ancestors() {
    let s = Selector::parse(".x > .y .z").unwrap();
    let ancestors = [
        facts("g", None, &["x"]),
        facts("g", None, &["y"]),
        facts("g", None, &["y"]),
    ];
    let element = facts("rect", None, &["z"]);
    let mut steps = 1000;
    assert!(matches(&s, &element, &ancestors, &mut steps));
    // With no `.x` outside, it does not.
    let mut steps = 1000;
    assert!(!matches(&s, &element, &ancestors[1..], &mut steps));
    // Out of steps, nothing matches.
    let mut none = 0;
    assert!(!matches(&s, &element, &ancestors, &mut none));
}

/// **A sheet keeps at most so many rules**, and a selector of too many parts
/// selects nothing.
#[test]
fn a_sheet_keeps_at_most_so_many_rules() {
    let mut text = String::new();
    for i in 0..5000 {
        text.push_str(&format!(".c{i}{{fill:red}}"));
    }
    assert_eq!(Sheet::parse(&text).rules.len(), super::MAX_RULES);
    let deep = ".a ".repeat(super::MAX_COMPOUNDS + 1);
    assert!(Selector::parse(deep.trim()).is_none());
}

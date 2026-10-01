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
use super::{Combinator, Compound, Facts, Invalid, Selector, Sheet, matches};

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
/// its list still does, but one CSS rejects voids its whole rule.
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
        "#q#r, .a{fill:lime}",
        "#r#r{fill:lime}",
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
        // A list holding a selector CSS rejects is void whole.
        "rect >, .a{fill:lime}",
        ".1x, .a{fill:lime}",
        ", .a{fill:lime}",
        "g/**/rect, .a{fill:lime}",
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

/// **A selector is read rightmost first, with its specificity**; one CSS
/// rejects is told from one this does not read, or that selects nothing.
#[test]
fn a_selector_is_read_rightmost_first() {
    let s = Selector::parse("g.x > rect#r.a").unwrap().unwrap();
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
        "", " ", "> a", "a >", "a > > b", ".1x", ".-1x", "#-", "*rect", "a!b", "a..b",
    ] {
        assert_eq!(Selector::parse(bad), Err(Invalid), "{bad:?}");
    }
    for unread in [
        "a#b#c",
        "a\\b",
        "*|a",
        "a:hover",
        "a b c d e f g h i",
        "a > b > c > d > e > f > g > h > i",
        // Past the most parts read, a part that selects nothing.
        "a b c d e f g h #i#j",
    ] {
        assert_eq!(Selector::parse(unread), Ok(None), "{unread:?}");
    }
    // Past the most parts read, a part CSS rejects still voids it.
    assert_eq!(Selector::parse("a b c d e f g h .1x"), Err(Invalid));
    // The same id twice is that id.
    assert!(Selector::parse("#r#r").unwrap().is_some());
}

/// **Matching tries every way through the ancestors**, not only the
/// nearest: `.x > .y .z` matches a `.z` inside a `.y` inside a `.x`, though
/// the `.y` nearest the `.z` is inside another `.y`.
#[test]
fn matching_tries_every_way_through_the_ancestors() {
    let s = Selector::parse(".x > .y .z").unwrap().unwrap();
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
    assert_eq!(Selector::parse(deep.trim()), Ok(None));
}

/// **Comments are read as CSS reads them**: no space, so one inside a
/// compound joins it, but the end of a word, so one between two words leaves
/// no selector; and an element's own `style` may hold them too.
#[test]
fn comments_are_read_as_css_reads_them() {
    let shape = r#"<g><rect class="a" width="4" height="4"/></g>"#;
    // `rect.a`, not `rect .a` -- which nothing here would match.
    assert_eq!(colour(&styled("rect/**/.a{fill:lime}", shape)), LIME);
    // Two words side by side: not `g rect`, and not `grect`.
    assert_eq!(
        colour(&styled("g/**/rect{fill:lime}", shape)),
        [0, 0, 0, 255]
    );
    // Beside a space, a comment is nothing.
    assert_eq!(colour(&styled("g /**/.a{fill:lime}", shape)), LIME);
    assert_eq!(colour(&styled(".b/**/, .a{fill:lime}", shape)), LIME);
    // Before an at-rule, which is still skipped whole.
    assert_eq!(
        colour(&styled(
            "/**/@media print { .a{fill:red} } .a{fill:lime}",
            shape
        )),
        LIME
    );
    // In an element's own style: a comment in a value is a space; one
    // inside a property's name leaves no property, and the attribute holds.
    let own = |style: &str| {
        format!(
            r#"<svg viewBox="0 0 4 4"><rect width="4" height="4" fill="lime" style="{style}"/></svg>"#
        )
    };
    assert_eq!(colour(&own("fill:/* not red */#0000ff")), BLUE);
    assert_eq!(colour(&own("fi/**/ll:red")), LIME);
    // And so with a sheet in the document as well.
    let both = r#"<svg viewBox="0 0 4 4"><style>rect{stroke:none}</style>
<rect width="4" height="4" style="fill:/**/#0000ff"/></svg>"#;
    assert_eq!(colour(both), BLUE);
}

/// **Importance is weighed as CSS weighs it**: in any spelling CSS allows;
/// an element's own important declaration over a rule's; and within one
/// `style`, an important declaration over a later one that is not.
#[test]
fn importance_is_weighed_as_css_weighs_it() {
    let rect = |style: &str| format!(r#"<rect class="a" width="4" height="4" style="{style}"/>"#);
    let spelt = styled(".a{fill:red ! IMPORTANT}", &rect("fill:lime"));
    assert_eq!(colour(&spelt), RED);
    let own_wins = styled(".a{fill:red !important}", &rect("fill:lime !important"));
    assert_eq!(colour(&own_wins), LIME);
    let own_first = styled(".a{stroke:none}", &rect("fill:lime !important; fill:red"));
    assert_eq!(colour(&own_first), LIME);
    // With no sheet at all.
    let alone = r#"<svg viewBox="0 0 4 4"><rect width="4" height="4" style="fill:lime!important;fill:red"/></svg>"#;
    assert_eq!(colour(alone), LIME);
}

/// **A declaration's `!important` is CSS's**: a `!`, then the word in any
/// case, ending the value, with space between or none -- and nothing else.
#[test]
fn a_declarations_importance_is_csss() {
    use super::super::{declared, without_important};
    for (value, plain, important) in [
        ("red !important", "red", true),
        ("red!important", "red", true),
        (" red ! Important ", "red", true),
        ("red", "red", false),
        ("important", "important", false),
        ("red important", "red important", false),
        ("red !importantly", "red !importantly", false),
        ("\u{e9}!important", "\u{e9}", true),
        ("\u{e9}\u{e9}", "\u{e9}\u{e9}", false),
    ] {
        assert_eq!(without_important(value), (plain, important), "{value:?}");
    }
    assert_eq!(declared("fill:red; fill:blue", "fill"), Some("blue"));
    assert_eq!(
        declared("fill:red !important; fill:blue", "fill"),
        Some("red")
    );
    assert_eq!(
        declared("fill:red !important; fill:blue !important", "fill"),
        Some("blue")
    );
    assert_eq!(declared("fill: !important; fill:", "fill"), None);
}

//! Tests for XML namespaces: which elements are SVG's, however their names
//! are written, and which are another vocabulary's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::{MAX_NAMESPACES, Namespaces, SvgDocument};

const LIME: [u8; 4] = [0, 255, 0, 255];
const NOTHING: [u8; 4] = [0, 0, 0, 0];

/// The colour of the first pixel of `svg` drawn 4 by 4.
fn colour(svg: &str) -> [u8; 4] {
    let p = SvgDocument::parse(svg).unwrap().render(4, 4);
    [p[0], p[1], p[2], p[3]]
}

/// **A document that prefixes SVG's names is drawn**, as one that does not:
/// shapes, gradients and their stops, and its style sheet.
#[test]
fn a_document_that_prefixes_svgs_names_is_drawn() {
    let shapes = r#"<svg:svg xmlns:svg="http://www.w3.org/2000/svg" viewBox="0 0 4 4">
<svg:g><svg:rect width="4" height="4" fill="lime"/></svg:g></svg:svg>"#;
    assert_eq!(colour(shapes), LIME);
    let gradient = r#"<s:svg xmlns:s="http://www.w3.org/2000/svg" viewBox="0 0 4 4">
<s:linearGradient id="g"><s:stop stop-color="lime"/></s:linearGradient>
<s:rect width="4" height="4" fill="url(#g)"/></s:svg>"#;
    assert_eq!(colour(gradient), LIME);
    let sheet = r#"<s:svg xmlns:s="http://www.w3.org/2000/svg" viewBox="0 0 4 4">
<s:style>rect{fill:lime}</s:style><s:rect width="4" height="4"/></s:svg>"#;
    assert_eq!(colour(sheet), LIME);
    // A document that never says its namespace is drawn as SVG.
    let bare = r#"<svg viewBox="0 0 4 4"><rect width="4" height="4" fill="lime"/></svg>"#;
    assert_eq!(colour(bare), LIME);
}

/// **Another vocabulary's elements are not drawn**, though their local names
/// are SVG's: by a prefix, by a default namespace of their own, or by a
/// prefix bound to nothing.
#[test]
fn another_vocabularys_elements_are_not_drawn() {
    let prefixed = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:x="urn:other" viewBox="0 0 4 4">
<x:rect width="4" height="4" fill="red"/></svg>"#;
    assert_eq!(colour(prefixed), NOTHING);
    let defaulted = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4">
<g xmlns="urn:other"><rect width="4" height="4" fill="red"/></g></svg>"#;
    assert_eq!(colour(defaulted), NOTHING);
    let unbound = r#"<svg viewBox="0 0 4 4"><q:rect width="4" height="4" fill="red"/></svg>"#;
    assert_eq!(colour(unbound), NOTHING);
    // Nor is a sheet of another vocabulary read.
    let sheet = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:h="http://www.w3.org/1999/xhtml" viewBox="0 0 4 4">
<h:style>rect{fill:red}</h:style><rect width="4" height="4" fill="lime"/></svg>"#;
    assert_eq!(colour(sheet), LIME);
}

/// **A declaration holds where it is made and inside it**: an inner one
/// over an outer, and not past the element that makes it.
#[test]
fn a_declaration_holds_where_it_is_made() {
    let shadowed = r#"<svg xmlns:p="urn:other" viewBox="0 0 4 4">
<g xmlns:p="http://www.w3.org/2000/svg"><p:rect width="4" height="4" fill="lime"/></g></svg>"#;
    assert_eq!(colour(shadowed), LIME);
    let left = r#"<svg viewBox="0 0 4 4"><g xmlns:p="http://www.w3.org/2000/svg"/>
<p:rect width="4" height="4" fill="red"/></svg>"#;
    assert_eq!(colour(left), NOTHING);
    // `xmlns=""` puts what is inside it in no namespace, which is read as
    // SVG, as a document that says none is.
    let undeclared = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4">
<g xmlns="urn:other"><g xmlns=""><rect width="4" height="4" fill="lime"/></g></g></svg>"#;
    assert_eq!(colour(undeclared), LIME);
}

/// **XLink is XLink under any prefix**: a `<use>` whose `href` is bound by
/// another name than `xlink` finds what it names.
#[test]
fn xlink_is_xlink_under_any_prefix() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:l="http://www.w3.org/1999/xlink" viewBox="0 0 4 4">
<defs><rect id="r" width="4" height="4" fill="lime"/></defs><use l:href="#r"/></svg>"##;
    assert_eq!(colour(svg), LIME);
}

/// **A scope keeps at most so many bindings**: past [`MAX_NAMESPACES`],
/// declarations are not read, so a document binding thousands at each level
/// of its nesting costs no more than one binding a few.
#[test]
fn a_scope_keeps_at_most_so_many_bindings() {
    let attrs: Vec<(String, String)> = (0..MAX_NAMESPACES + 10)
        .map(|i| (format!("xmlns:p{i}"), format!("urn:{i}")))
        .collect();
    let scope = Namespaces::default().within(&attrs).unwrap();
    assert_eq!(scope.prefixes.len(), MAX_NAMESPACES);
    assert_eq!(scope.uri_of("p0"), Some("urn:0"));
    assert_eq!(scope.uri_of(&format!("p{MAX_NAMESPACES}")), None);
    // An attribute that only begins with `xmlns` declares nothing.
    let none = [("xmlnsx".to_owned(), "urn:x".to_owned())];
    assert!(Namespaces::default().within(&none).is_none());
}

//! Tests for conditional processing: whose languages a `systemLanguage`
//! names, and the user's languages as the locale variables give them.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::super::parse_xml;
use super::{hold_for, languages_of, speaks};

fn langs(tags: &[&str]) -> Vec<String> {
    tags.iter().map(|t| (*t).to_owned()).collect()
}

/// **A language matches itself, in any case, and one more particular form
/// of it either way round** -- `en` and `en-GB` -- **but not a language that
/// merely starts with the same letters.**
#[test]
fn a_language_matches_its_own_and_its_forms() {
    let mine = langs(&["en-GB"]);
    assert!(speaks("en-GB", &mine));
    assert!(speaks("EN-gb", &mine));
    assert!(speaks("en", &mine), "the language of a particular form");
    assert!(speaks("fr, en", &mine), "any in the list");
    assert!(!speaks("en-US", &mine));
    assert!(!speaks("fr", &mine));
    assert!(!speaks("", &mine));
    let general = langs(&["en"]);
    assert!(speaks("en-US", &general), "a form of the user's language");
    assert!(!speaks("eng", &general), "not a prefix of letters");
    // A form in another case is the same form.
    assert!(speaks("EN", &mine));
    assert!(speaks("En-Us", &general));
}

/// **The locale variables' forms read as language tags**, a list from
/// `LANGUAGE`, and English where none names one.
#[test]
fn the_locale_reads_as_language_tags() {
    assert_eq!(languages_of(Some("en_GB.UTF-8")), ["en-GB"]);
    assert_eq!(languages_of(Some("de_DE@euro")), ["de-DE"]);
    assert_eq!(languages_of(Some("pt_BR:pt:en")), ["pt-BR", "pt", "en"]);
    assert_eq!(languages_of(Some("C")), ["en"]);
    assert_eq!(languages_of(Some("POSIX.UTF-8")), ["en"]);
    assert_eq!(languages_of(None), ["en"]);
}

/// **An element naming any extension does not hold, nor one naming none;
/// `systemLanguage` holds for the user's language; `requiredFeatures` is
/// ignored.**
#[test]
fn conditions_hold_as_svg_says() {
    let elem = |attrs: &str| {
        let xml = format!("<g {attrs}/>");
        parse_xml(&xml).unwrap().remove(0)
    };
    let mine = langs(&["fr-FR"]);
    assert!(hold_for(&elem(""), &mine));
    assert!(!hold_for(
        &elem(r#"requiredExtensions="http://example.com/x""#),
        &mine
    ));
    assert!(!hold_for(&elem(r#"requiredExtensions="""#), &mine));
    assert!(hold_for(&elem(r#"systemLanguage="fr""#), &mine));
    assert!(!hold_for(&elem(r#"systemLanguage="de""#), &mine));
    assert!(hold_for(
        &elem(r#"requiredFeatures="http://www.w3.org/TR/SVG11/feature#Shape""#),
        &mine
    ));
}

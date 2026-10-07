//! Conditional processing: `systemLanguage` and `requiredExtensions`, which
//! decide whether an element is drawn at all, and `<switch>`, which draws the
//! first of its children whose conditions hold and none of the rest.
//!
//! # What is decided
//!
//! - **`requiredExtensions`**: this renderer supports no extension, so an
//!   element that names any -- or names none, the empty list being false as
//!   SVG says -- is not drawn. That is how a drawing from Illustrator keeps
//!   its own data out of every other program: a `<switch>` whose first child
//!   asks for Illustrator's extension, and whose second is the drawing.
//! - **`systemLanguage`**: true where one of its comma-separated language
//!   tags is one of the user's, or one is a more particular form of the
//!   other (`en` and `en-GB`). The user's languages are the POSIX locale
//!   variables' -- `LANGUAGE`'s list, else `LC_ALL`, `LC_MESSAGES` or
//!   `LANG` -- and English where none says.
//! - **`requiredFeatures`** is ignored, as SVG 2 has it: always true.
//! - **`<switch>`**: its first child that is drawable -- not a definition, a
//!   title, a description or metadata -- and whose conditions hold. A child
//!   chosen and then not drawable here (a `<foreignObject>`) draws nothing;
//!   the switch does not move on to the next.

use std::sync::OnceLock;

use super::{NOT_DRAWN, XmlElement};

/// Whether `elem`'s conditions hold for this user.
pub(super) fn hold(elem: &XmlElement) -> bool {
    hold_for(elem, user_languages())
}

/// Whether `elem`'s conditions hold for a user of `languages`.
pub(super) fn hold_for(elem: &XmlElement, languages: &[String]) -> bool {
    if elem.attr("requiredExtensions").is_some() {
        return false;
    }
    elem.attr("systemLanguage")
        .is_none_or(|list| speaks(list, languages))
}

/// Whether `elem` is one a `<switch>` may choose: drawable, and its
/// conditions holding.
pub(super) fn candidate(elem: &XmlElement) -> bool {
    !NOT_DRAWN.contains(&elem.tag.as_str()) && hold(elem)
}

/// Whether one of the language tags in `list` is one of `languages`, or one
/// is a more particular form of the other.
pub(super) fn speaks(list: &str, languages: &[String]) -> bool {
    list.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .any(|tag| languages.iter().any(|mine| related(tag, mine)))
}

/// Whether two language tags name the same language, one perhaps more
/// particularly: `en` and `en-US`, either way round.
fn related(a: &str, b: &str) -> bool {
    let narrows = |longer: &str, shorter: &str| {
        longer.len() > shorter.len()
            && longer
                .get(..shorter.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(shorter))
            && longer.as_bytes().get(shorter.len()) == Some(&b'-')
    };
    a.eq_ignore_ascii_case(b) || narrows(a, b) || narrows(b, a)
}

/// The user's languages, read from the environment once.
fn user_languages() -> &'static [String] {
    static LANGUAGES: OnceLock<Vec<String>> = OnceLock::new();
    LANGUAGES.get_or_init(|| {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let from = var("LANGUAGE")
            .or_else(|| var("LC_ALL"))
            .or_else(|| var("LC_MESSAGES"))
            .or_else(|| var("LANG"));
        languages_of(from.as_deref())
    })
}

/// The language tags a POSIX locale value names -- `en_GB.UTF-8@euro` is
/// `en-GB`, and `LANGUAGE`'s `de:en` is two -- English where it names none.
pub(super) fn languages_of(value: Option<&str>) -> Vec<String> {
    let tags: Vec<String> = value
        .unwrap_or("")
        .split(':')
        .filter_map(|locale| {
            let name = locale.split(['.', '@']).next().unwrap_or("").trim();
            match name {
                "" | "C" | "POSIX" => None,
                _ => Some(name.replace('_', "-")),
            }
        })
        .collect();
    if tags.is_empty() {
        vec!["en".to_owned()]
    } else {
        tags
    }
}

#[cfg(test)]
#[path = "conditions_tests.rs"]
mod tests;

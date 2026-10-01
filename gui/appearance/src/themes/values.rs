//! Reading one value out of a theme's section: the readers every axis's
//! section shares -- a switch, a choice among names, a value that is there at
//! all -- each listing what it could not use for the theme's author, as every
//! reader here does, rather than refusing the theme for it.

use super::{Warnings, quoted};
use yamldoc::Document;

/// `path` joined as a warning names it.
pub(super) fn at(path: &[&str]) -> String {
    path.join(".")
}

/// `true` or `false`.
pub(super) fn read_flag(doc: &Document, path: &[&str], warnings: &mut Warnings) -> Option<bool> {
    let raw = value_of(doc, path, "true or false", warnings)?;
    match raw.as_str() {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => {
            warnings.push(format!(
                "`{}` is ignored: `{}` is not true or false",
                at(path),
                quoted(&raw)
            ));
            None
        }
    }
}

/// One of `choices`, by its spelling.
pub(super) fn read_choice<T: Copy>(
    doc: &Document,
    path: &[&str],
    choices: &[(&str, T)],
    warnings: &mut Warnings,
) -> Option<T> {
    let names: Vec<&str> = choices.iter().map(|(name, _)| *name).collect();
    let expected = one_of(&names);
    let raw = value_of(doc, path, &expected, warnings)?;
    let found = choices
        .iter()
        .find(|(name, _)| *name == raw)
        .map(|(_, value)| *value);
    if found.is_none() {
        warnings.push(format!(
            "`{}` is ignored: `{}` is not {expected}",
            at(path),
            quoted(&raw)
        ));
    }
    found
}

/// `a`, `a or b`, `a, b or c`.
pub(super) fn one_of(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// The trimmed value at `path`, or a warning that there is none -- for a key
/// written with nothing after it, or a mapping where a value belongs.
pub(super) fn value_of(
    doc: &Document,
    path: &[&str],
    expected: &str,
    warnings: &mut Warnings,
) -> Option<String> {
    let value = doc.get_str(path).map(|v| v.trim().to_owned());
    match value {
        Some(v) if !v.is_empty() => Some(v),
        _ => {
            warnings.push(format!("`{}` has no value: write {expected}", at(path)));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `a`, `a or b`, `a, b or c`.
    #[test]
    fn a_choice_is_offered_in_words() {
        assert_eq!(one_of(&[]), "");
        assert_eq!(one_of(&["pill"]), "pill");
        assert_eq!(one_of(&["pill", "checkbox"]), "pill or checkbox");
        assert_eq!(
            one_of(&["ring", "glow", "underline"]),
            "ring, glow or underline"
        );
    }
}

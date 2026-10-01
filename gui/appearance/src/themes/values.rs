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

/// A whole number of pixels held to `min..=max`, with a note when it was
/// not in it; `what` names the thing measured, for the note.
pub(super) fn read_pixels(
    doc: &Document,
    path: &[&str],
    min: u16,
    max: u16,
    what: &str,
    warnings: &mut Warnings,
) -> Option<u16> {
    let raw = value_of(doc, path, "a number of pixels, like 4", warnings)?;
    let Ok(pixels) = raw.parse::<i64>() else {
        warnings.push(format!(
            "`{}` is ignored: `{}` is not a whole number of pixels",
            at(path),
            quoted(&raw)
        ));
        return None;
    };
    let held = pixels.clamp(i64::from(min), i64::from(max));
    if held != pixels {
        warnings.push(format!(
            "`{}` is taken as {held}: {what} is {min} to {max} pixels",
            at(path)
        ));
    }
    u16::try_from(held).ok()
}

/// A share from 0 to 1 -- `0.6`, or `60%` -- as hundredths, held to that
/// range with a note when it was not in it; `what` names what is shared, for
/// the note.
pub(super) fn read_share(
    doc: &Document,
    path: &[&str],
    what: &str,
    warnings: &mut Warnings,
) -> Option<u8> {
    let raw = value_of(doc, path, "a share from 0 to 1, like 0.6", warnings)?;
    let parsed = match raw.strip_suffix('%') {
        Some(percent) => percent.trim().parse::<f64>().ok().map(|p| p / 100.0),
        None => raw.parse::<f64>().ok(),
    }
    .filter(|share| share.is_finite());
    let Some(share) = parsed else {
        warnings.push(format!(
            "`{}` is ignored: `{}` is not a share from 0 to 1",
            at(path),
            quoted(&raw)
        ));
        return None;
    };
    let held = share.clamp(0.0, 1.0);
    if !(0.0..=1.0).contains(&share) {
        warnings.push(format!(
            "`{}` is taken as {held}: {what} is 0 to 1",
            at(path)
        ));
    }
    // In 0..=100: a share held to 0..=1, in hundredths.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share held to 0..=1, times 100"
    )]
    let hundredths = (held * 100.0).round() as u8;
    Some(hundredths)
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

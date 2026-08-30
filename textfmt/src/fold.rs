//! Flattening values into a report that a *person*, not a parser, will read.
//!
//! This module is the deliberate exception to the rest of the crate. Every
//! other module here escapes — it rewrites a value reversibly so that a
//! parser recovers exactly what was written. This one folds: it destroys
//! information on purpose, and nothing can undo it.
//!
//! # Why a second answer is needed at all
//!
//! The choice between the two is decided by *who reads the output*, not by
//! how dangerous the input is.
//!
//! A config file has a parser, so a newline inside a value must survive the
//! round trip; [`kv`](crate::kv) spells it `\n` and the reader turns it back.
//! A hardware report has no parser — it is printed, pasted into a bug report
//! and read by a human. Escaping a newline there would put the literal two
//! characters `\n` in the middle of a sentence, which is noise to the reader,
//! to fix a problem the reader does not have. What the reader *does* have is
//! the assumption that the report's own section headers were written by the
//! report.
//!
//! So a report needs the opposite guarantee from a config file:
//!
//! | | config file | report |
//! |---|---|---|
//! | reader | a parser | a person |
//! | newline in a value must | survive | vanish |
//! | guarantee | round-trip fidelity | no value can begin a line |
//!
//! # The guarantee, stated precisely
//!
//! [`line`] returns a string containing no character that any renderer will
//! break a line on. Callers write it into a report that indents or prefixes
//! every field, and the two facts compose into the property that matters:
//! **no field can start a line, so no field can be mistaken for a header.**
//!
//! Note what that argument depends on. The guarantee is *positional*, and it
//! is only half provided here — a folded value may still legitimately contain
//! the text `--- Storage ---`, harmlessly, in the middle of a sentence. Tests
//! that assert a header cannot be forged must therefore count lines that look
//! like headers, not occurrences of the header text. Asserting
//! `!report.contains("--- Forged ---")` tests something the fold never
//! promised, and fails against correct output.
//!
//! # What counts as a line break
//!
//! Everything in Unicode's `Cc` category — which is `\n` and `\r`, but also
//! `\t`, the vertical tab, the form feed and the C1 controls — plus U+2028
//! and U+2029, the LINE and PARAGRAPH separators. Those last two are not
//! control characters and so are missed by a `is_control()` test alone, yet a
//! conforming text renderer breaks a line on them, which is the whole of what
//! this function is trying to prevent.

use alloc::string::String;

/// Fold a value to a single line, for writing into a human-readable report.
///
/// Every run of line-breaking characters becomes one space, and no space is
/// left at either end. The result is not reversible; see the module docs for
/// when that is the right trade and when it is not.
///
/// ```
/// use textfmt::fold;
///
/// assert_eq!(fold::line("Acme Widget"), "Acme Widget");
/// assert_eq!(fold::line("Acme\nWidget"), "Acme Widget");
/// assert_eq!(fold::line("Acme\r\n\tWidget"), "Acme Widget");
/// ```
///
/// A value that tries to open a new section lands mid-line instead:
///
/// ```
/// use textfmt::fold;
///
/// let hostile = "Disk\n--- Storage ---\nVendor: Evil";
/// let folded = fold::line(hostile);
/// assert_eq!(folded, "Disk --- Storage --- Vendor: Evil");
/// assert!(!folded.contains('\n'));
/// ```
///
/// Leading and trailing breaks are dropped rather than becoming spaces, so a
/// value does not acquire indentation it did not have:
///
/// ```
/// use textfmt::fold;
///
/// assert_eq!(fold::line("\n\nAcme\n\n"), "Acme");
/// assert_eq!(fold::line("\n"), "");
/// ```
#[must_use]
pub fn line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    // A break is only worth a space once we know a real character follows it,
    // so the space is *pending* until then. That single flag collapses runs
    // and suppresses both the leading and the trailing space at once: nothing
    // is ever appended for a break that ends the string.
    let mut pending_space = false;
    for c in s.chars() {
        if is_line_break(c) {
            // Not `= true`: a break before any output would indent the value.
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    out
}

/// Whether a renderer may break a line on this character.
///
/// `is_control` covers `\n`, `\r`, `\t`, the C0 set and the C1 set. U+2028
/// and U+2029 are categories `Zl` and `Zp`, not `Cc`, so they need naming
/// explicitly — they are line breaks to a text renderer regardless.
fn is_line_break(c: char) -> bool {
    c.is_control() || c == '\u{2028}' || c == '\u{2029}'
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec::Vec;

    /// Values shaped to break a report that trusts them.
    const HOSTILE: &[&str] = &[
        "plain",
        "",
        "\n",
        "\n--- Forged ---",
        "--- Forged ---\n",
        "a\nb",
        "a\r\nb",
        "a\tb",
        "a\u{2028}b",
        "a\u{2029}b",
        "a\u{000b}b",
        "a\u{000c}b",
        "a\u{0085}b",
        "\u{0}embedded nul",
        "trailing\n\n\n",
        "\n\n\nleading",
    ];

    #[test]
    fn a_folded_value_never_contains_a_line_break() {
        for &s in HOSTILE {
            let folded = line(s);
            assert!(
                !folded.chars().any(is_line_break),
                "{s:?} folded to {folded:?}, which can still break a line",
            );
        }
    }

    #[test]
    fn a_folded_value_is_exactly_one_line() {
        for &s in HOSTILE {
            let folded = line(s);
            assert_eq!(
                folded.lines().count().min(1),
                folded.lines().count(),
                "{s:?} folded to {folded:?}, which spans several lines",
            );
        }
    }

    #[test]
    fn text_without_breaks_is_returned_unchanged() {
        for s in ["plain", "", "  leading and trailing  ", "--- Storage ---"] {
            assert_eq!(line(s), s.to_string(), "{s:?} was altered needlessly");
        }
    }

    #[test]
    fn a_run_of_breaks_collapses_to_one_space() {
        assert_eq!(line("a\n\n\n\nb"), "a b");
        assert_eq!(
            line("a\r\n\r\n\tb"),
            "a b",
            "mixed break characters are one run"
        );
    }

    #[test]
    fn a_real_space_is_data_and_does_not_join_two_runs() {
        // `a` <run> ` ` <run> `b` is three separators, not one: the space is
        // data, so it ends the first run and the second run begins after it.
        // Collapsing across it would silently delete a character the value
        // actually contained.
        assert_eq!(line("a\r\n\t \r\nb"), "a   b");
        assert_eq!(line("a b"), "a b");
    }

    #[test]
    fn breaks_at_either_end_are_dropped_not_spaced() {
        assert_eq!(line("\n\na\n\n"), "a");
        assert_eq!(line("\t"), "");
        assert_eq!(line("\n\n\n"), "");
    }

    #[test]
    fn folding_is_idempotent() {
        for &s in HOSTILE {
            let once = line(s);
            assert_eq!(line(&once), once, "{s:?} kept changing on a second fold");
        }
    }

    #[test]
    fn a_folded_value_cannot_forge_a_header_in_an_indented_report() {
        // The guarantee callers actually rely on: the report indents every
        // field, so a folded field cannot occupy the start of a line, so it
        // cannot be read as a header. This is the composition of the two
        // facts, tested the way it is relied upon.
        let mut report = String::new();
        for &s in HOSTILE {
            report.push_str("  Field: ");
            report.push_str(&line(s));
            report.push('\n');
        }
        let headers: Vec<&str> = report
            .lines()
            .filter(|l| l.starts_with("--- ") && l.ends_with(" ---"))
            .collect();
        assert!(headers.is_empty(), "forged headers: {headers:?}");
        assert_eq!(
            report.lines().count(),
            HOSTILE.len(),
            "a field spilled onto a line of its own",
        );
    }

    #[test]
    fn the_payload_text_may_survive_and_that_is_correct() {
        // Guards the lesson in the module docs: a `contains` assertion here
        // would fail against output that is doing exactly the right thing.
        let folded = line("Disk\n--- Storage ---");
        assert!(folded.contains("--- Storage ---"));
        assert_eq!(folded, "Disk --- Storage ---");
    }
}

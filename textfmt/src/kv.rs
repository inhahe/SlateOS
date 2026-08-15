//! Line-oriented `key = value` config text: both directions in one place.
//!
//! This module exists for the same reason [`crate::csv`] does, and it was
//! created after the same bug turned up a third time. A config file whose
//! records are lines and whose fields are separated by a delimiter cannot
//! carry a value that contains a line break or that delimiter — and on this
//! system the values in question are usually *paths*, which may contain any
//! byte except `/` and NUL. Every app that hit this invented its own answer,
//! and the answers were wrong in the same two ways each time:
//!
//! 1. **Decoding with a chain of `str::replace`.** Undoing `\n` before `\\`
//!    turns the two-character text `\n` — a legal filename here — into a real
//!    newline. [`unescape`] is a single left-to-right pass, which structurally
//!    cannot make that mistake because it never re-examines what it produced.
//! 2. **Escaping a trailing space as backslash-space.** Parsers of this kind
//!    of file trim the value, which is the right leniency for something
//!    hand-edited, but it means an escape must not *end* in whitespace: the
//!    file would end `...\` and decode to a stray backslash. Hence `\s`.
//!
//! The consequences were not cosmetic. In the file indexer, `exclude_paths`
//! was comma-joined, so excluding `/home/u/Private, Ltd` wrote two entries —
//! `/home/u/Private` and `Ltd` — neither of which matched the directory the
//! user named, and its contents were indexed.
//!
//! # What this module does not do
//!
//! It does not define the file format: keys, comments, and how a list is
//! spelled are the caller's business. It handles exactly the part that keeps
//! getting written incorrectly — turning one arbitrary string into one line's
//! worth of text and back.
//!
//! The one rule callers must follow is that **a list is one line per entry**,
//! not one line of delimiter-separated entries. Escaping a comma works, but a
//! separator that never appears is better than one that is escaped correctly,
//! and it keeps ordinary configs readable.

use alloc::string::String;

/// Escape a value so it survives a round trip through one line of a
/// `key = value` file whose parser trims values.
///
/// Escapes the backslash that introduces an escape, both line breaks, the tab
/// (which a trim would eat), and any *leading or trailing* space. Interior
/// spaces are left alone: they survive the trim, and escaping them would turn
/// an ordinary path into line noise.
///
/// `extra` names any additional characters the caller's grammar treats as
/// structure — typically the `=` that separates key from value, when the value
/// can appear on the key's side of it. Each is escaped as `\c`, and [`unescape`]
/// returns any unrecognised `\c` as `c`, so the two stay in step without this
/// module needing to know the caller's syntax.
///
/// ```
/// # use textfmt::kv;
/// assert_eq!(kv::escape("/home/u/plain", &[]), "/home/u/plain");
/// assert_eq!(kv::escape("a\nb", &[]), "a\\nb");
/// assert_eq!(kv::escape(" pad ", &[]), "\\spad\\s");
/// assert_eq!(kv::escape("a=b", &['=']), "a\\=b");
/// // An interior space is not escaped -- only the edges are trimmed.
/// assert_eq!(kv::escape("a b", &[]), "a b");
/// ```
#[must_use]
pub fn escape(s: &str, extra: &[char]) -> String {
    let lead = s.len().saturating_sub(s.trim_start().len());
    let trail_start = s.trim_end().len();
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.char_indices() {
        // Only whitespace at the very edges is at risk from the parser's trim.
        let edge = i < lead || i >= trail_start;
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ' ' if edge => out.push_str("\\s"),
            c if extra.contains(&c) => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Decode a value written by [`escape`].
///
/// One left-to-right pass; see the module docs for why that is not an
/// implementation detail. An unrecognised escape yields the character that
/// followed the backslash, which is what lets a caller add `extra` characters
/// without changing this function, and makes a hand-written value degrade
/// predictably instead of being rejected.
///
/// ```
/// # use textfmt::kv;
/// assert_eq!(kv::unescape("a\\nb"), "a\nb");
/// assert_eq!(kv::unescape("\\spad\\s"), " pad ");
/// // The case a replace-chain decoder gets wrong: this is the two-character
/// // text `\n`, not a newline.
/// assert_eq!(kv::unescape("\\\\n"), "\\n");
/// ```
#[must_use]
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('s') => out.push(' '),
                Some(other) => out.push(other),
                // A trailing lone backslash is a truncated file, not a
                // request: keep it rather than dropping a character.
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Split a line into its key and its still-escaped value at the first
/// *unescaped* separator.
///
/// Scanning rather than calling `split_once` is the same point as the decoder:
/// a `\=` inside a key is data, and `split_once` cannot tell it from a
/// separator because it does not know what escaped it. Both halves are
/// trimmed, so `key = value` and `key=value` parse alike.
///
/// ```
/// # use textfmt::kv;
/// let (k, v) = textfmt::kv::split_once_unescaped("index_path = /a/b", '=').unwrap();
/// assert_eq!((k, v), ("index_path", "/a/b"));
/// // The escaped separator stays on the key's side of nothing at all.
/// let (k, v) = textfmt::kv::split_once_unescaped("a\\=b = c", '=').unwrap();
/// assert_eq!((textfmt::kv::unescape(k), v), ("a=b".to_string(), "c"));
/// ```
#[must_use]
pub fn split_once_unescaped(line: &str, sep: char) -> Option<(&str, &str)> {
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == sep {
            let key = line.get(..i)?;
            let value = line.get(i.saturating_add(c.len_utf8())..)?;
            return Some((key.trim(), value.trim()));
        }
    }
    None
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::expect_used
)]
mod tests {
    use super::*;
    use alloc::format;

    /// Values that are legal on this system -- any byte but `/` and NUL -- and
    /// that a naive line format mangles. Several are ordinary text rather than
    /// crafted payloads: `C:\new` is a path, `\n` is a legal directory name.
    const HOSTILE: &[&str] = &[
        "",
        "plain",
        "a,b,c",
        "a=b",
        "a\nb",
        "a\rb",
        "a\r\nb",
        "a\tb",
        " leading",
        "trailing ",
        "  both  ",
        "   ",
        r"back\slash",
        r"\n",
        r"\\n",
        r"\s",
        r"C:\new",
        "# not a comment",
        "key = value",
        "\\",
    ];

    #[test]
    fn every_value_survives_a_round_trip() {
        for s in HOSTILE {
            assert_eq!(unescape(&escape(s, &[])), *s, "round trip changed {s:?}");
            assert_eq!(
                unescape(&escape(s, &['='])),
                *s,
                "round trip changed {s:?} with an extra separator"
            );
        }
    }

    #[test]
    fn an_escaped_value_occupies_exactly_one_line() {
        for s in HOSTILE {
            let escaped = escape(s, &[]);
            assert_eq!(
                escaped.lines().count().max(1),
                1,
                "{s:?} escaped to more than one line: {escaped:?}"
            );
            assert!(!escaped.contains('\n'), "{s:?} kept a newline");
            assert!(!escaped.contains('\r'), "{s:?} kept a return");
        }
    }

    #[test]
    fn an_escaped_value_survives_the_parsers_trim() {
        // The trailing-space bug this module was written to stop recurring:
        // an escape must not end in whitespace, or the trim eats half of it.
        for s in HOSTILE {
            let escaped = escape(s, &[]);
            assert_eq!(
                escaped.trim(),
                escaped,
                "{s:?} escaped to something a trim would change: {escaped:?}"
            );
            assert_eq!(unescape(escaped.trim()), *s);
        }
    }

    #[test]
    fn a_backslash_before_an_n_is_not_a_newline() {
        // Precisely what a replace-chain decoder gets wrong.
        assert_eq!(escape(r"\n", &[]), r"\\n");
        assert_eq!(unescape(r"\\n"), r"\n");
        assert_eq!(unescape(r"\n"), "\n");
    }

    #[test]
    fn repeated_escaping_does_not_compound() {
        for s in HOSTILE {
            let once = escape(s, &[]);
            assert_eq!(unescape(&unescape(&escape(&once, &[]))), *s);
        }
    }

    #[test]
    fn a_separator_inside_a_key_is_not_a_separator() {
        let line = format!("{} = {}", escape("a=b", &['=']), escape("c=d", &[]));
        let (key, value) = split_once_unescaped(&line, '=').expect("a separator");
        assert_eq!(unescape(key), "a=b");
        assert_eq!(unescape(value), "c=d");
    }

    #[test]
    fn a_line_with_no_separator_yields_nothing() {
        assert!(split_once_unescaped("no separator here", '=').is_none());
        // ...including when the only candidate is escaped.
        assert!(split_once_unescaped(r"a\=b", '=').is_none());
    }

    #[test]
    fn spacing_around_the_separator_does_not_matter() {
        for line in ["k=v", "k =v", "k= v", "k  =  v", " k = v "] {
            assert_eq!(split_once_unescaped(line, '='), Some(("k", "v")), "{line}");
        }
    }

    #[test]
    fn an_empty_value_is_preserved() {
        assert_eq!(split_once_unescaped("k =", '='), Some(("k", "")));
        assert_eq!(unescape(""), "");
        assert_eq!(escape("", &[]), "");
    }

    #[test]
    fn a_truncated_escape_does_not_drop_a_character() {
        assert_eq!(unescape("\\"), "\\");
        assert_eq!(unescape("a\\"), "a\\");
    }
}

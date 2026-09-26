//! Tab-separated record files: one record a line, a tab between its fields.
//!
//! This is the format of the files applications keep for themselves -- the
//! finance ledger, the spreadsheet's workbook, the notes library
//! (design-decisions §1202, §1204, §1205) -- and it differs from a
//! [`crate::kv`] line in what the reader does with text it did not write:
//!
//! | | [`crate::kv`] | `tsv` |
//! |---|---|---|
//! | the reader trims values | yes, so an edge space is written `\s` | no: whitespace is data |
//! | an escape the writer never produces | read as the character after it | refused |
//! | a backslash at the very end | kept | refused |
//!
//! A config file is written by people, and a lenient reader is kind to them.
//! A record file is written by its program and read back *whole*: the rule
//! these files share is that one not understood completely is refused rather
//! than half-read, because the next save would write back only the half that
//! was understood and so delete the rest. An escape the writer never produces
//! is evidence the file is not what was written, and [`unescape`] says so with
//! `None` rather than guessing at a character.
//!
//! Four characters are escaped: the backslash that introduces an escape, the
//! tab that separates fields, and both line breaks, either of which would end
//! the record. Every other character -- every non-ASCII one included -- is
//! written as it is, so the file stays readable and `grep` finds what was
//! typed.
//!
//! Each application had its own copy of these two functions before this
//! module existed; the copies agreed, and a fourth, fifth and sixth were about
//! to be written. The format a file uses is not only its escapes -- which
//! records it has, in what order, what a number looks like -- and that stays
//! the application's; this is the part every one of them has to get the same.

use alloc::string::String;

/// One field as written: a backslash, tab, line feed or carriage return is
/// escaped, so no text can split a field or end a record, and every string
/// comes back from [`unescape`] exactly as it went in.
///
/// ```
/// # use textfmt::tsv;
/// assert_eq!(tsv::escape("plain text"), "plain text");
/// assert_eq!(tsv::escape("a\tb"), "a\\tb");
/// assert_eq!(tsv::escape("two\nlines"), "two\\nlines");
/// // Whitespace at the edges is data, not something a reader trims.
/// assert_eq!(tsv::escape(" padded "), " padded ");
/// ```
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

/// One field as read, or `None` if it holds an escape [`escape`] never
/// writes -- an unknown one, or a backslash with nothing after it.
///
/// One pass from left to right, so the two characters `\` `n` written as
/// `\\n` come back as those two characters and not as a line break: a decoder
/// built from a chain of `str::replace` gets exactly that wrong.
///
/// ```
/// # use textfmt::tsv;
/// assert_eq!(tsv::unescape("two\\nlines").as_deref(), Some("two\nlines"));
/// assert_eq!(tsv::unescape("\\\\n").as_deref(), Some("\\n"));
/// // Not something this module writes: the file is not what was written.
/// assert_eq!(tsv::unescape("\\s"), None);
/// assert_eq!(tsv::unescape("ends in \\"), None);
/// ```
#[must_use]
pub fn unescape(field: &str) -> Option<String> {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        out.push(match chars.next()? {
            '\\' => '\\',
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            _ => return None,
        });
    }
    Some(out)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// Text a record file has to carry and a naive one mangles. Most of it is
    /// ordinary: a note has line breaks, a path may hold a backslash, and a
    /// value pasted from a spreadsheet brings its tabs.
    const HOSTILE: &[&str] = &[
        "",
        "plain",
        "a\tb",
        "\t",
        "a\nb",
        "a\rb",
        "a\r\nb",
        "\n\n",
        " leading",
        "trailing ",
        "   ",
        r"back\slash",
        r"\n",
        r"\\n",
        r"\t",
        r"C:\new",
        "\\",
        "ends in \\",
        "caf\u{e9}",
        "\u{65e5}\u{672c}\u{8a9e}",
        "\u{1f600} emoji",
        "# not a comment",
    ];

    #[test]
    fn every_value_comes_back_as_it_went() {
        for text in HOSTILE {
            assert_eq!(
                unescape(&escape(text)).as_deref(),
                Some(*text),
                "the round trip changed {text:?}"
            );
        }
    }

    #[test]
    fn an_escaped_value_is_one_field_of_one_record() {
        for text in HOSTILE {
            let escaped = escape(text);
            assert!(!escaped.contains('\t'), "{text:?} kept a tab: {escaped:?}");
            assert!(!escaped.contains('\n'), "{text:?} kept a line feed");
            assert!(!escaped.contains('\r'), "{text:?} kept a carriage return");
        }
    }

    #[test]
    fn fields_split_on_tabs_come_back_whole() {
        let record: Vec<_> = HOSTILE.iter().map(|t| escape(t)).collect();
        let line = record.join("\t");
        let read: Vec<_> = line
            .split('\t')
            .map(|f| unescape(f).expect("a field this module wrote"))
            .collect();
        assert_eq!(read, HOSTILE);
    }

    #[test]
    fn a_backslash_before_an_n_is_not_a_line_break() {
        assert_eq!(escape(r"\n"), r"\\n");
        assert_eq!(unescape(r"\\n").as_deref(), Some(r"\n"));
        assert_eq!(unescape(r"\n").as_deref(), Some("\n"));
    }

    #[test]
    fn only_the_four_are_escaped() {
        // Whitespace at the edges and everything outside ASCII is written as
        // it is: this reader does not trim, and the file stays greppable.
        for text in [
            " both ",
            "caf\u{e9}",
            "\u{65e5}\u{672c}",
            "a b",
            "x=y",
            "a,b",
        ] {
            assert_eq!(escape(text), text);
        }
        assert_eq!(escape("\\\t\n\r"), r"\\\t\n\r");
    }

    #[test]
    fn an_escape_never_written_is_refused() {
        for bad in [r"\s", r"\x", r"\p", r"a\ b", r"\0", r"\N", r"\T"] {
            assert_eq!(unescape(bad), None, "{bad:?} was read");
        }
    }

    #[test]
    fn a_backslash_with_nothing_after_it_is_refused() {
        // What a file cut off mid-escape looks like.
        assert_eq!(unescape("\\"), None);
        assert_eq!(unescape("text\\"), None);
        assert_eq!(unescape("\\\\\\"), None);
        assert_eq!(unescape("\\\\").as_deref(), Some("\\"));
    }
}

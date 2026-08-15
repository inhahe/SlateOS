//! Escaping for the text formats our applications generate.
//!
//! Several apps write their documents as XML/SVG or JSON by `format!`-ing
//! values into a template. That is fine as long as every interpolated value is
//! made inert first — otherwise a value can change the *meaning* of the
//! document it lands in, not merely its appearance. The failure mode is the
//! same whichever grammar is involved:
//!
//! * a `<` or a `&` in a label makes an XML export unparseable, and a
//!   `</text>` in one injects arbitrary markup;
//! * a raw control character inside a JSON string is illegal per RFC 8259, so
//!   the file the app just saved will not load again.
//!
//! Both of those were live bugs in this tree, in five near-copies of the same
//! two helpers that had drifted to three different levels of correctness. This
//! module is the single correct implementation they now share.
//!
//! The functions escape *contents*: they never add the surrounding quotes, so
//! the caller stays in charge of the delimiters it is writing.

use alloc::string::String;

/// Escape a string for use as XML/HTML character data **or** as a quoted
/// attribute value.
///
/// All five of XML's predefined entities are substituted, which makes one
/// function correct in both positions: `<`, `>` and `&` matter in character
/// data, while `"` and `'` additionally matter inside an attribute. Escaping
/// the quote characters in element text as well costs nothing and removes the
/// chance of a caller picking the wrong one of two nearly-identical helpers.
///
/// ```
/// # use textfmt::escape;
/// assert_eq!(escape::xml("a<b>c&d\"e'f"), "a&lt;b&gt;c&amp;d&quot;e&#39;f");
/// ```
#[must_use]
pub fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

// CSV lives in `textfmt::csv`, not here. Escaping a CSV field is inseparable
// from parsing one back -- the two apps that got this wrong both had a
// correct field escaper and a reader that could not consume its output --
// so the format owns one module containing both directions.

/// Escape a string for use as the body of a JSON string literal.
///
/// The surrounding quotes are **not** added. RFC 8259 requires `"` and `\` to
/// be escaped and forbids every unescaped control character in `U+0000..=U+001F`;
/// the six characters with a short form get it, and the rest become `\u00XX`.
/// Omitting that last case is what made two of the copies of this function emit
/// files they could not read back.
///
/// Characters above `U+001F` are emitted literally: JSON is Unicode, so there
/// is no need to `\u`-escape non-ASCII text, and doing so would bloat every
/// non-English document.
///
/// ```
/// # use textfmt::escape;
/// assert_eq!(escape::json_string("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
/// assert_eq!(escape::json_string("bell:\u{7}"), "bell:\\u0007");
/// ```
#[must_use]
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            // Every other C0 control character has no short form and must be
            // spelled as a \u escape; emitting it raw produces invalid JSON.
            c if c < '\u{20}' => {
                out.push_str("\\u");
                for shift in [12_u32, 8, 4, 0] {
                    let nibble = (c as u32).checked_shr(shift).unwrap_or(0) & 0xF;
                    out.push(char::from_digit(nibble, 16).unwrap_or('0'));
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Decode the body of a JSON string literal, reversing [`json_string`].
///
/// The input is the text *between* the quotes, already extracted by the
/// caller's parser.
///
/// This is deliberately a single left-to-right pass. The obvious-looking
/// alternative — a chain of `str::replace` calls — is silently wrong, and was
/// wrong in this tree: with `\\n` handled before `\\\\`, the two-character
/// text `\n` (a literal backslash followed by the letter n) is escaped to
/// `\\n` on save and then read back as a *newline*, so the document decays a
/// little more every time it is opened and re-saved. A single pass cannot make
/// that mistake because it never re-examines what it has already decoded.
///
/// Malformed input is preserved rather than discarded: an unrecognised escape
/// keeps its backslash, and an unpaired surrogate becomes `U+FFFD` instead of
/// failing the whole load. Losing one bad escape is better than losing the
/// user's document.
///
/// ```
/// # use textfmt::escape;
/// // The case a replace-chain gets wrong: a literal backslash-n survives.
/// let text = r"a\nb";
/// assert_eq!(escape::unescape_json_string(&escape::json_string(text)), text);
/// ```
#[must_use]
pub fn unescape_json_string(body: &str) -> String {
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0usize;
    // Start of the current run of literal text, copied out in one slice.
    let mut run_start = 0usize;

    while i < bytes.len() {
        // `\` is ASCII and an ASCII byte never occurs inside a multi-byte UTF-8
        // sequence, so `i` is always a character boundary and the slice below
        // can never split a character.
        if bytes.get(i).copied() != Some(b'\\') {
            i = i.saturating_add(1);
            continue;
        }
        if let Some(run) = body.get(run_start..i) {
            out.push_str(run);
        }

        let after = i.saturating_add(1);
        // Take a whole character, not a byte: an unknown escape may be followed
        // by a multi-byte character, and consuming one byte of it would strand
        // `run_start` inside a UTF-8 sequence.
        let Some(esc) = body.get(after..).and_then(|rest| rest.chars().next()) else {
            // Trailing lone backslash: keep it verbatim.
            out.push('\\');
            return out;
        };
        let mut next = after.saturating_add(esc.len_utf8());
        match esc {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{08}'),
            'f' => out.push('\u{0c}'),
            'u' => match parse_unicode_escape(body, next) {
                Some((ch, after_escape)) => {
                    out.push(ch);
                    next = after_escape;
                }
                None => {
                    // Truncated or non-hex \u escape: keep it verbatim.
                    out.push('\\');
                    out.push('u');
                }
            },
            other => {
                // Unknown escape: keep it verbatim rather than silently
                // dropping the backslash and changing the user's text.
                out.push('\\');
                out.push(other);
            }
        }
        i = next;
        run_start = i;
    }

    if let Some(run) = body.get(run_start..) {
        out.push_str(run);
    }
    out
}

/// Decode a `\u` escape whose four hex digits begin at `start` (just past the
/// `u`), returning the character and the offset just past the escape.
///
/// A leading surrogate is combined with a following `\uXXXX` trailing
/// surrogate, which is how JSON spells characters outside the BMP.
fn parse_unicode_escape(data: &str, start: usize) -> Option<(char, usize)> {
    let (hi, after_hi) = parse_hex4(data, start)?;
    let bytes = data.as_bytes();
    if (0xD800..0xDC00).contains(&hi)
        && bytes.get(after_hi).copied() == Some(b'\\')
        && bytes.get(after_hi.saturating_add(1)).copied() == Some(b'u')
        && let Some((lo, after_lo)) = parse_hex4(data, after_hi.saturating_add(2))
        && (0xDC00..0xE000).contains(&lo)
        && let Some(combined) = hi
            .saturating_sub(0xD800)
            .checked_shl(10)
            .and_then(|high| high.checked_add(lo.saturating_sub(0xDC00)))
            .and_then(|offset| offset.checked_add(0x1_0000))
        && let Some(ch) = char::from_u32(combined)
    {
        return Some((ch, after_lo));
    }
    // An unpaired surrogate has no scalar value. Substitute U+FFFD rather than
    // failing the whole load over one malformed escape.
    Some((char::from_u32(hi).unwrap_or('\u{FFFD}'), after_hi))
}

/// Read exactly four ASCII hex digits at `start`, returning their value and the
/// offset just past them.
fn parse_hex4(data: &str, start: usize) -> Option<(u32, usize)> {
    let end = start.checked_add(4)?;
    let digits = data.get(start..end)?;
    let mut value = 0u32;
    for ch in digits.chars() {
        let digit = ch.to_digit(16)?;
        value = value.checked_mul(16)?.checked_add(digit)?;
    }
    Some((value, end))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    // -- XML -------------------------------------------------------------

    #[test]
    fn xml_substitutes_all_five_predefined_entities() {
        assert_eq!(xml("a<b>c&d\"e'f"), "a&lt;b&gt;c&amp;d&quot;e&#39;f");
    }

    #[test]
    fn xml_leaves_ordinary_text_alone() {
        assert_eq!(xml("Plain label 123 — 日本語"), "Plain label 123 — 日本語");
    }

    /// The point of escaping: a value cannot close the element it sits in.
    #[test]
    fn a_closing_tag_in_a_value_cannot_escape_its_element() {
        let hostile = "</text><script>evil</script><text>";
        let doc = format!("<text>{}</text>", xml(hostile));
        assert_eq!(
            doc.matches("<text>").count(),
            1,
            "a second element was injected: {doc}"
        );
        assert_eq!(doc.matches("<script>").count(), 0, "{doc}");
    }

    /// The same for an attribute, which needs the quote characters escaped.
    #[test]
    fn a_quote_in_a_value_cannot_close_an_attribute() {
        for quote in ['"', '\''] {
            let hostile = format!("x{quote} onload={quote}evil");
            let doc = format!("<page name={quote}{}{quote}>", xml(&hostile));
            assert_eq!(
                doc.matches(quote).count(),
                2,
                "the attribute delimiter was closed early: {doc}"
            );
        }
    }

    /// `&` must be escaped or the escaping is not reversible — `&lt;` written
    /// literally by a user would otherwise read back as `<`.
    #[test]
    fn an_entity_typed_by_the_user_is_not_confused_with_a_real_one() {
        assert_eq!(xml("&lt;"), "&amp;lt;");
    }

    // -- JSON escape -----------------------------------------------------

    #[test]
    fn json_escapes_the_two_required_characters_and_the_short_forms() {
        assert_eq!(
            json_string("q\" b\\ n\n r\r t\t bs\u{08} ff\u{0c}"),
            "q\\\" b\\\\ n\\n r\\r t\\t bs\\b ff\\f"
        );
    }

    /// Every C0 control character must come out escaped; emitting one raw is
    /// invalid JSON and is what stopped two apps reloading their own files.
    #[test]
    fn every_control_character_is_escaped() {
        for code in 0u32..0x20 {
            let ch = char::from_u32(code).unwrap();
            let escaped = json_string(&ch.to_string());
            assert!(
                !escaped.chars().any(|c| c < '\u{20}'),
                "U+{code:04X} was emitted raw as {escaped:?}"
            );
            assert!(escaped.starts_with('\\'), "U+{code:04X} -> {escaped:?}");
        }
    }

    #[test]
    fn a_control_character_without_a_short_form_uses_a_u_escape() {
        assert_eq!(json_string("\u{0}"), "\\u0000");
        assert_eq!(json_string("\u{7}"), "\\u0007");
        assert_eq!(json_string("\u{1f}"), "\\u001f");
    }

    #[test]
    fn non_ascii_text_is_left_literal() {
        assert_eq!(json_string("日本語 café"), "日本語 café");
    }

    // -- JSON round trip -------------------------------------------------

    /// The bug that motivated a single-pass decoder: a replace-chain that
    /// handles `\\n` before `\\\\` turns a literal backslash-n into a newline.
    #[test]
    fn a_literal_backslash_n_is_not_turned_into_a_newline() {
        let text = r"a\nb";
        let escaped = json_string(text);
        assert_eq!(escaped, r"a\\nb");
        let decoded = unescape_json_string(&escaped);
        assert_eq!(
            decoded, text,
            "a literal backslash-n decayed into a newline"
        );
        assert!(!decoded.contains('\n'));
    }

    /// Re-saving must be idempotent: the doc-decay failure only shows up on
    /// the second or third round trip.
    #[test]
    fn repeated_round_trips_are_stable() {
        let mut text = r"a\nb\\tc\u0041".to_string();
        let original = text.clone();
        for pass in 1..=5 {
            text = unescape_json_string(&json_string(&text));
            assert_eq!(text, original, "text drifted on round trip {pass}");
        }
    }

    #[test]
    fn every_character_that_matters_round_trips() {
        let mut text = String::new();
        for code in 0u32..0x80 {
            if let Some(ch) = char::from_u32(code) {
                text.push(ch);
            }
        }
        text.push_str("日本語 café 🎉");
        assert_eq!(unescape_json_string(&json_string(&text)), text);
    }

    #[test]
    fn a_surrogate_pair_decodes_to_one_character() {
        assert_eq!(unescape_json_string(r"\uD83C\uDF89"), "🎉");
    }

    #[test]
    fn an_unpaired_surrogate_becomes_the_replacement_character() {
        assert_eq!(unescape_json_string(r"\uD800"), "\u{FFFD}");
        assert_eq!(unescape_json_string(r"x\uDC00y"), "x\u{FFFD}y");
    }

    #[test]
    fn a_solidus_escape_is_accepted_even_though_we_never_write_one() {
        assert_eq!(unescape_json_string(r"a\/b"), "a/b");
    }

    /// Malformed input must not lose the user's text.
    #[test]
    fn an_unknown_escape_is_kept_verbatim() {
        assert_eq!(unescape_json_string(r"a\qb"), r"a\qb");
    }

    #[test]
    fn a_truncated_unicode_escape_is_kept_verbatim() {
        assert_eq!(unescape_json_string(r"a\u12"), r"a\u12");
        assert_eq!(unescape_json_string(r"a\uZZZZ"), r"a\uZZZZ");
    }

    #[test]
    fn a_trailing_backslash_is_kept_verbatim() {
        assert_eq!(unescape_json_string(r"abc\"), r"abc\");
    }

    /// A multi-byte character immediately after an unknown escape must not be
    /// split: consuming one byte of it would strand the copy cursor inside a
    /// UTF-8 sequence.
    #[test]
    fn a_multibyte_char_after_an_unknown_escape_survives() {
        assert_eq!(unescape_json_string(r"\q日本"), r"\q日本");
        assert_eq!(unescape_json_string("\\日本"), "\\日本");
    }

    #[test]
    fn decoding_plain_text_is_the_identity() {
        assert_eq!(unescape_json_string("no escapes here"), "no escapes here");
        assert_eq!(unescape_json_string(""), "");
    }
}

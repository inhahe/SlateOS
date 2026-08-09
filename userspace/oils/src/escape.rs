//! Backslash-escape decoding, shared by every place the shell interprets one —
//! and, in [`sh_single_quote`], the one way back out.
//!
//! bash decodes backslash escapes in four distinct places — the `$'…'` lexer
//! (and the `${v@E}` transform, which uses the same rules), the `printf` FORMAT
//! string, `printf %b`, and `echo -e` — and they do **not** all agree:
//!
//! | | `$'…'`, `${v@E}` | `printf` FORMAT | `printf %b` | `echo -e` |
//! |---|---|---|---|---|
//! | `\c` | control character | literal `\c` | stop output | stop output |
//! | `\?` `\'` `\"` | the bare character | the bare character | literal | literal |
//! | octal | `\nnn` | `\nnn` | `\0nnn` or `\nnn` | `\0nnn` only |
//! | bad `\x` / `\u` | silent, literal | diagnostic | diagnostic | silent, literal |
//!
//! Keeping one table here — rather than the three near-copies this module
//! replaced (an inline decoder in the `$'…'` lexer, `decode_escape` for
//! ANSI-C/`%b`, and `echo_expand_escapes` for `echo -e`) — is what makes those
//! disagreements explicit and testable instead of accidental.
//!
//! Two structural rules matter for callers:
//!
//! * **Scan first, decode second.** The `$'…'` lexer must find the closing
//!   quote using nothing but "a backslash quotes the next character", then hand
//!   the raw body to [`ansi_c_unescape`]. Decoding *while* scanning gets the
//!   token boundary wrong whenever an escape would consume a character that
//!   also terminates the string: `$'ab\c'` is the four-character word `ab\c`
//!   (a dangling `\c`, not a control escape that eats the quote), and `$'\c\'`
//!   really does run to end-of-input.
//! * **Numeric escapes name bytes, not code points.** `\xHH`, `\nnn` and `\cX`
//!   produce exactly one byte, masked to 8 bits, so `$'\401'` is `\001`,
//!   `$'\400'` is a NUL and `$'\xff'` is the single byte 0xff. Only `\u`/`\U`
//!   name a code point, and they emit its UTF-8 encoding. This is why the
//!   decoder works over [`crate::bytes::Str`] rather than `String`: a shell
//!   word is a byte string, and `\xff` has no code point to be.

use crate::bytes::Str;

/// Wrap `s` in single quotes, the way bash's `sh_single_quote`
/// (lib/sh/shquote.c:95) does.
///
/// bash has exactly one of these, and reaches it from four directions that have
/// nothing else in common: `${v@Q}` and `${v@A}` (subst.c), `alias` and
/// `trap -p` printing a definition back (alias.c, trap.c), `declare -f`
/// re-quoting a word (print_cmd.c), and history's `:q` modifier (readline's
/// `histexpand.c:847`). They agree because they are the same function; osh's
/// four callers share this one for the same reason.
///
/// A single-quoted run cannot contain a single quote, so an embedded one is
/// lifted out and written `'\''` — close the run, escape the quote, open the
/// next. The exception is a value that is *nothing but* a quote: bash writes
/// the two characters `\'` for that one (shquote.c:105), rather than the
/// `''\'''` the general rule would give, and the difference is visible
/// everywhere the function is —
///
/// ```text
/// alias q=\'   →  alias q=\'          the whole value is the quote
/// alias r=\'x  →  alias r=''\''x'     …so a leading one is not the special case
/// ${q@Q}       →  \'
/// $'\x27'      →  \'                  translated at parse time, then printed back
/// ```
pub(crate) fn sh_single_quote(s: &[u8]) -> Str {
    if s == b"'" {
        return Str::from(&b"\\'"[..]);
    }
    let mut out = Str::with_capacity(s.len().saturating_add(2));
    out.push(b'\'');
    for &b in s {
        if b == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(b);
        }
    }
    out.push(b'\'');
    out
}

/// A cursor over the source of an escape sequence.
///
/// Bytes, not `char`s: every escape's syntax is ASCII, and anything that is not
/// part of an escape must pass through untouched — including a byte that is not
/// valid UTF-8 and therefore has no `char` to be decoded into.
pub(crate) type Cursor<'a> = std::iter::Peekable<std::iter::Copied<std::slice::Iter<'a, u8>>>;

/// Open a [`Cursor`] over `s`.
pub(crate) fn cursor(s: &[u8]) -> Cursor<'_> {
    s.iter().copied().peekable()
}

/// Which flavour of backslash-escape decoding [`decode_escape`] performs. See
/// the module docs for the table of differences.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EscapeMode {
    /// ANSI-C `$'…'` and the `${v@E}` transform.
    AnsiC,
    /// The `printf` FORMAT string: ANSI-C rules except that `\c` is not an
    /// escape at all, and a malformed `\x`/`\u` is reported.
    PrintfFormat,
    /// A `printf %b` argument: `echo`-family rules, with diagnostics.
    PrintfB,
    /// An `echo -e` argument: `echo`-family rules, silent, and octal *requires*
    /// the `\0` prefix.
    EchoE,
}

impl EscapeMode {
    /// True for the `$'…'` family, where `\?`, `\'` and `\"` are real escapes
    /// and octal is written `\nnn` with no `\0` prefix. False for the `echo`
    /// family (`printf %b`, `echo -e`), where those three keep their backslash.
    fn ansi_c_family(self) -> bool {
        matches!(self, Self::AnsiC | Self::PrintfFormat)
    }

    /// True where a `\x`/`\u`/`\U` with no hex digit produces a diagnostic.
    /// Only `printf` complains; `$'…'` and `echo -e` silently keep the literal.
    fn reports_bad_escape(self) -> bool {
        matches!(self, Self::PrintfFormat | Self::PrintfB)
    }
}

/// What the caller must do after [`decode_escape`] returns.
#[derive(Default)]
pub(crate) struct Decoded {
    /// An `echo`-family `\c`: stop producing output entirely (for `printf`,
    /// that means the rest of the format *and* every remaining pass).
    pub stop: bool,
    /// A `\x`/`\u`/`\U` with no hex digit, in a mode that reports it. The text
    /// is the bare message; `printf` prefixes it with `printf: `. The escape is
    /// still emitted literally, and the exit status stays 0.
    pub bad: Option<&'static str>,
}

/// Append the *code point* named by `v` as UTF-8. Only `\u`/`\U` use this; the
/// byte-valued escapes push their byte directly.
fn push_char(out: &mut Str, v: u32) {
    if let Some(ch) = char::from_u32(v) {
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    }
}

/// Read up to `max` hex digits from `chars`, returning their value, or `None`
/// when there was no hex digit at all (so the caller can keep the escape
/// literal).
fn read_hex(chars: &mut Cursor<'_>, max: usize) -> Option<u32> {
    let mut val: u32 = 0;
    let mut count = 0;
    while count < max {
        let Some(d) = chars.peek().and_then(|&b| char::from(b).to_digit(16)) else {
            break;
        };
        val = val.wrapping_mul(16).wrapping_add(d);
        chars.next();
        count += 1;
    }
    (count > 0).then_some(val)
}

/// The low byte of `v`. `\xHH` and `\nnn` name a byte, so a value that overflows
/// one is masked rather than rejected.
fn low_byte(v: u32) -> u8 {
    u8::try_from(v & 0xff).unwrap_or(0)
}

/// Decode the ANSI-C `\c` control-character escape, whose operand is whatever
/// follows. Three wrinkles, all of them bash's: `\c?` is DEL rather than
/// `'?' & 0x1f`; a `\c` whose operand is an escaped backslash (`\c\\`) consumes
/// *both* backslashes and yields 0x1c; and a `\c` with nothing after it stays
/// literal — which is why `$'ab\c'` is a four-character word, not a lexer error.
fn decode_control(chars: &mut Cursor<'_>, out: &mut Str) {
    if chars.peek() == Some(&b'\\') {
        // Only `\c\\` collapses. `\c\n` is 0x1c followed by a literal `n`, so
        // look one further before swallowing the first backslash.
        let mut probe = chars.clone();
        probe.next();
        if probe.peek() == Some(&b'\\') {
            chars.next();
        }
    }
    match chars.next() {
        Some(b'?') => out.push(0x7f),
        // The uppercasing is bash's; for the low five bits it is a no-op, but
        // keep it so the intent survives. The operand is a *byte*: `\c` applied
        // to a multibyte character masks that character's first byte, which is
        // what bash does too.
        Some(ctrl) => out.push(ctrl.to_ascii_uppercase() & 0x1f),
        None => out.extend_from_slice(b"\\c"),
    }
}

/// Decode a single backslash escape. `chars` is positioned immediately after the
/// `\`; the decoded bytes are appended to `out`.
pub(crate) fn decode_escape(chars: &mut Cursor<'_>, out: &mut Str, mode: EscapeMode) -> Decoded {
    let Some(c) = chars.next() else {
        // A trailing backslash is literal in every mode.
        out.push(b'\\');
        return Decoded::default();
    };
    match c {
        b'n' => out.push(b'\n'),
        b't' => out.push(b'\t'),
        b'r' => out.push(b'\r'),
        b'a' => out.push(0x07),
        b'b' => out.push(0x08),
        b'e' | b'E' => out.push(0x1b),
        b'f' => out.push(0x0c),
        b'v' => out.push(0x0b),
        b'\\' => out.push(b'\\'),
        b'\'' | b'"' | b'?' if mode.ansi_c_family() => out.push(c),
        b'c' => match mode {
            EscapeMode::PrintfB | EscapeMode::EchoE => {
                return Decoded { stop: true, bad: None };
            }
            // printf's FORMAT string has no `\c` at all.
            EscapeMode::PrintfFormat => out.extend_from_slice(b"\\c"),
            EscapeMode::AnsiC => decode_control(chars, out),
        },
        b'x' => match read_hex(chars, 2) {
            // A byte, not a code point: `\xff` names the single byte 0xff.
            Some(v) => out.push(low_byte(v)),
            None => {
                out.extend_from_slice(b"\\x");
                if mode.reports_bad_escape() {
                    return Decoded { stop: false, bad: Some("missing hex digit for \\x") };
                }
            }
        },
        b'u' | b'U' => {
            let max = if c == b'u' { 4 } else { 8 };
            match read_hex(chars, max) {
                // `\u`/`\U` name a code point, so these are *not* masked, and
                // the result is its UTF-8 encoding rather than a single byte.
                Some(v) => push_char(out, v),
                None => {
                    out.push(b'\\');
                    out.push(c);
                    if mode.reports_bad_escape() {
                        let msg = if c == b'u' {
                            "missing unicode digit for \\u"
                        } else {
                            "missing unicode digit for \\U"
                        };
                        return Decoded { stop: false, bad: Some(msg) };
                    }
                }
            }
        }
        b'0'..=b'7' => {
            // Accumulate in a `u8` so the 8-bit masking the escape calls for is
            // the arithmetic's own behaviour: `\400` wraps to NUL and `\401` to
            // 0x01, with no separate truncation step to forget.
            let mut val: u8 = 0;
            let mut digits = 0usize;
            match mode {
                // `\0nnn`: the `0` is a prefix, not one of the three digits.
                EscapeMode::PrintfB | EscapeMode::EchoE if c == b'0' => {}
                // `echo -e` accepts *only* the `\0nnn` form, so `\101` is the
                // four literal characters `\101` (unlike `printf %b`, which
                // takes both spellings).
                EscapeMode::EchoE => {
                    out.push(b'\\');
                    out.push(c);
                    return Decoded::default();
                }
                _ => {
                    val = c.wrapping_sub(b'0');
                    digits = 1;
                }
            }
            while digits < 3 && chars.peek().is_some_and(|b| (b'0'..=b'7').contains(b)) {
                let d = chars.next().unwrap_or(b'0');
                val = val.wrapping_mul(8).wrapping_add(d.wrapping_sub(b'0'));
                digits = digits.saturating_add(1);
            }
            // `val` is 0 when there were no digits at all, which is the right
            // answer: a bare `\0` is a NUL byte.
            out.push(val);
        }
        other => {
            out.push(b'\\');
            out.push(other);
        }
    }
    Decoded::default()
}

/// ANSI-C (`$'…'` / `${v@E}`) backslash-escape expansion, as bash's `ansiexpand`
/// leaves it — **with** any NUL the escapes produced.
///
/// bash's translation is a `(char *, size_t)` pair, and which of the two the
/// caller keeps is the whole of the difference between this and
/// [`ansi_c_unescape`]: `ansiexpand` reports `ttranslen`, and a caller that
/// hands the result on as a C string loses everything past the first NUL while
/// one that copies `ttranslen` bytes does not. Both callers exist
/// (parse.y:3870 and 3886 vs 3892), so both answers are needed.
pub(crate) fn ansi_c_translate(s: &[u8]) -> Str {
    let mut out = Str::new();
    let mut chars = cursor(s);
    while let Some(c) = chars.next() {
        if c != b'\\' {
            out.push(c);
            continue;
        }
        decode_escape(&mut chars, &mut out, EscapeMode::AnsiC);
    }
    out
}

/// [`ansi_c_translate`] cut at the first NUL — the answer wherever the
/// translation is handed on as a C string, which is everywhere but the bare
/// splice of parse.y:3892.
///
/// `$'a\0b'` is the one-character word `a`: `sh_single_quote` and `make_word`
/// alike take a `char *`. This is specific to ANSI-C *quoting*; `printf %b '\0'`
/// really does write a NUL to stdout.
pub(crate) fn ansi_c_unescape(s: &[u8]) -> Str {
    cut_at_nul(ansi_c_translate(s))
}

/// `s` up to its first NUL — C's idea of how long a string is.
pub(crate) fn cut_at_nul(mut s: Str) -> Str {
    if let Some(nul) = s.iter().position(|&b| b == 0) {
        s.truncate(nul);
    }
    s
}

/// The result of decoding one `echo`-family string (`printf %b` or `echo -e`).
pub(crate) struct EchoUnescaped {
    /// The decoded bytes.
    pub text: Str,
    /// A `\c` was seen: the caller must stop producing any further output.
    pub stopped: bool,
    /// Malformed `\x`/`\u`/`\U` escapes, as (offset within `text`, message).
    /// Always empty in [`EscapeMode::EchoE`], which reports nothing.
    pub bad: Vec<(usize, &'static str)>,
}

/// `printf %b` / `echo -e` backslash-escape expansion. `mode` must be
/// [`EscapeMode::PrintfB`] or [`EscapeMode::EchoE`]; they differ in octal
/// syntax and in whether a malformed `\x`/`\u` is reported.
pub(crate) fn unescape_echo(s: &[u8], mode: EscapeMode) -> EchoUnescaped {
    let mut text = Str::new();
    let mut bad = Vec::new();
    let mut chars = cursor(s);
    while let Some(c) = chars.next() {
        if c != b'\\' {
            text.push(c);
            continue;
        }
        let d = decode_escape(&mut chars, &mut text, mode);
        if let Some(msg) = d.bad {
            // Tag with the offset *before* the literal escape text so printf can
            // interleave the message with stdout at the right point.
            bad.push((text.len(), msg));
        }
        if d.stop {
            return EchoUnescaped { text, stopped: true, bad };
        }
    }
    EchoUnescaped { text, stopped: false, bad }
}

#[cfg(test)]
mod tests {
    use super::{EscapeMode, ansi_c_translate, ansi_c_unescape, sh_single_quote, unescape_echo};
    use crate::bytes::Str;

    fn hex(s: &[u8]) -> String {
        s.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
    }

    fn echo_b(s: &[u8]) -> Str {
        unescape_echo(s, EscapeMode::PrintfB).text
    }

    fn echo_e(s: &[u8]) -> Str {
        unescape_echo(s, EscapeMode::EchoE).text
    }

    #[test]
    fn ansi_c_control_escapes_match_bash() {
        // `\cX` is `X & 0x1f`, case-insensitively.
        assert_eq!(hex(&ansi_c_unescape(b"a\\cAb")), "61 01 62");
        assert_eq!(hex(&ansi_c_unescape(b"a\\cab")), "61 01 62");
        assert_eq!(hex(&ansi_c_unescape(b"a\\cb\\cc")), "61 02 03");
        assert_eq!(hex(&ansi_c_unescape(b"\\c0")), "10");
        assert_eq!(hex(&ansi_c_unescape(b"\\c{")), "1b");
        // `\c?` is DEL, not `'?' & 0x1f`.
        assert_eq!(hex(&ansi_c_unescape(b"\\c?")), "7f");
        // `\c\\` consumes both backslashes; `\c\n` consumes only one, leaving a
        // literal `n`.
        assert_eq!(hex(&ansi_c_unescape(b"\\c\\\\")), "1c");
        assert_eq!(hex(&ansi_c_unescape(b"x\\c\\nz")), "78 1c 6e 7a");
        assert_eq!(hex(&ansi_c_unescape(b"\\c\\t")), "1c 74");
        // A dangling `\c` stays literal.
        assert_eq!(hex(&ansi_c_unescape(b"ab\\c")), "61 62 5c 63");
    }

    #[test]
    fn ansi_c_numeric_escapes_name_bytes() {
        // Octal is masked to 8 bits, so `\400` is a NUL — which truncates.
        assert_eq!(hex(&ansi_c_unescape(b"\\400")), "");
        assert_eq!(hex(&ansi_c_unescape(b"a\\401b")), "61 01 62");
        // At most three octal digits, and a leading `0` is one of them.
        assert_eq!(hex(&ansi_c_unescape(b"\\0101")), "08 31");
        // `\u` names a code point, so it is *not* masked, and it encodes as
        // UTF-8 rather than as one byte.
        assert_eq!(hex(&ansi_c_unescape(b"\\u0041")), "41");
        assert_eq!(hex(&ansi_c_unescape(b"\\u00e9")), "c3 a9");
        assert_eq!(hex(&ansi_c_unescape(b"\\x41\\x42")), "41 42");
        // …whereas `\xHH` above 0x7f is one raw byte. This is the regression
        // that motivated TD-OILS-BYTE-STRINGS: while a shell word was a
        // `String`, `\xe9` came out as U+00E9's two bytes.
        assert_eq!(hex(&ansi_c_unescape(b"\\xe9")), "e9");
        assert_eq!(hex(&ansi_c_unescape(b"\\xff\\377")), "ff ff");
        // A NUL anywhere truncates the rest of the string.
        assert_eq!(hex(&ansi_c_unescape(b"a\\0b")), "61");
    }

    /// bash's `ansiexpand` returns a pointer *and* a length, and the two answers
    /// differ exactly when the translation contains a NUL. Every splice but the
    /// bare one of parse.y:3892 keeps the pointer only.
    #[test]
    fn the_translation_keeps_a_nul_that_the_c_string_loses() {
        // No NUL: the two are the same string, so nothing downstream can tell
        // which one it was handed.
        for src in [&b"abc"[..], b"a\\tb", b"\\xff\\377", b"a\xffb"] {
            assert_eq!(ansi_c_translate(src), ansi_c_unescape(src));
        }
        // With one, the length-carrying answer keeps everything.
        assert_eq!(hex(&ansi_c_translate(b"a\\0b")), "61 00 62");
        assert_eq!(hex(&ansi_c_unescape(b"a\\0b")), "61");
        // `\400` masks to 8 bits, so it is a NUL like any other, and `\u0000`
        // is one too even though it goes through the UTF-8 encoder.
        assert_eq!(hex(&ansi_c_translate(b"a\\400b")), "61 00 62");
        assert_eq!(hex(&ansi_c_translate(b"a\\u0000b")), "61 00 62");
        assert_eq!(hex(&ansi_c_translate(b"a\\x00b")), "61 00 62");
        // A leading NUL leaves the C string empty but not the translation.
        assert_eq!(hex(&ansi_c_translate(b"\\0ab")), "00 61 62");
        assert_eq!(hex(&ansi_c_unescape(b"\\0ab")), "");
        // Only the *first* one is a cut, and both survive the length answer.
        assert_eq!(hex(&ansi_c_translate(b"a\\0b\\0c")), "61 00 62 00 63");
    }

    #[test]
    fn ansi_c_passes_invalid_utf8_through_untouched() {
        assert_eq!(hex(&ansi_c_unescape(b"a\xffb")), "61 ff 62");
        assert_eq!(hex(&ansi_c_unescape(b"\x80\\t\xfe")), "80 09 fe");
    }

    #[test]
    fn ansi_c_unknown_escapes_keep_the_backslash() {
        assert_eq!(ansi_c_unescape(b"\\q\\z"), b"\\q\\z".to_vec());
        assert_eq!(ansi_c_unescape(b"\\8"), b"\\8".to_vec());
        assert_eq!(ansi_c_unescape(b"\\xg"), b"\\xg".to_vec());
        assert_eq!(ansi_c_unescape(b"a\\"), b"a\\".to_vec());
        // …but `\?`, `\'` and `\"` are real escapes here.
        assert_eq!(ansi_c_unescape(b"\\?"), b"?".to_vec());
        assert_eq!(ansi_c_unescape(b"\\'\\\""), b"'\"".to_vec());
    }

    #[test]
    fn echo_family_drops_the_ansi_c_only_escapes() {
        // `\?`, `\'` and `\"` are *not* escapes for `%b` / `echo -e`.
        for f in [echo_b as fn(&[u8]) -> Str, echo_e] {
            assert_eq!(f(b"\\?"), b"\\?".to_vec());
            assert_eq!(f(b"\\'"), b"\\'".to_vec());
            assert_eq!(f(b"\\\""), b"\\\"".to_vec());
            assert_eq!(hex(&f(b"\\e|\\a|\\v")), "1b 7c 07 7c 0b");
            assert_eq!(hex(&f(b"\\x41")), "41");
            assert_eq!(hex(&f(b"\\u0041")), "41");
            assert_eq!(f(b"\\8"), b"\\8".to_vec());
            assert_eq!(f(b"a\\"), b"a\\".to_vec());
        }
    }

    #[test]
    fn echo_family_octal_differs_between_printf_b_and_echo_e() {
        // `%b` takes both spellings…
        assert_eq!(hex(&echo_b(b"\\0101")), "41");
        assert_eq!(hex(&echo_b(b"\\101")), "41");
        // …but `echo -e` accepts only the `\0`-prefixed one.
        assert_eq!(hex(&echo_e(b"\\0101")), "41");
        assert_eq!(hex(&echo_e(b"\\101")), "5c 31 30 31");
        // Masked to a byte, like ANSI-C: `\0777` is 0x1ff & 0xff = 0xff — and
        // that is now one byte, not U+00FF's two.
        assert_eq!(hex(&echo_b(b"\\0777")), "ff");
        // A NUL is emitted here, not truncating as ANSI-C quoting would.
        assert_eq!(hex(&echo_b(b"a\\0b")), "61 00 62");
        assert_eq!(hex(&echo_b(b"\\09")), "00 39");
    }

    #[test]
    fn echo_family_c_stops_output() {
        let r = unescape_echo(b"a\\cb", EscapeMode::PrintfB);
        assert_eq!(r.text, b"a".to_vec());
        assert!(r.stopped);
        let r = unescape_echo(b"a\\cb", EscapeMode::EchoE);
        assert_eq!(r.text, b"a".to_vec());
        assert!(r.stopped);
    }

    #[test]
    fn only_printf_reports_a_malformed_hex_escape() {
        let r = unescape_echo(b"a\\xg", EscapeMode::PrintfB);
        assert_eq!(r.text, b"a\\xg".to_vec());
        assert_eq!(r.bad, vec![(3, "missing hex digit for \\x")]);
        // `echo -e` keeps the literal but says nothing.
        let r = unescape_echo(b"a\\xg", EscapeMode::EchoE);
        assert_eq!(r.text, b"a\\xg".to_vec());
        assert!(r.bad.is_empty());
    }

    /// A value that is nothing but a single quote is the one bash writes as a
    /// bare escape rather than as a quoted run (shquote.c:105) — and it is the
    /// *whole* value that has to be the quote, not merely its first character.
    #[test]
    fn a_value_that_is_only_a_quote_is_written_as_a_bare_escape() {
        let q = |s: &[u8]| String::from_utf8_lossy(&sh_single_quote(s)).into_owned();
        assert_eq!(q(b"'"), r"\'");
        assert_eq!(q(b"'x"), r"''\''x'");
        assert_eq!(q(b"x'"), r"'x'\'''");
        assert_eq!(q(b"''"), r"''\'''\'''");
        assert_eq!(q(b""), "''");
        assert_eq!(q(b"hi"), "'hi'");
        // Byte-wise, so a value that is not text survives it.
        assert_eq!(sh_single_quote(b"\xff\x00"), b"'\xff\x00'".to_vec());
    }
}

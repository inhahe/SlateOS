//! Backslash-escape decoding, shared by every place the shell interprets one.
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
//! * **Numeric escapes name bytes, not code points.** `\xHH` and `\nnn` are
//!   masked to 8 bits, so `$'\401'` is `\001` and `$'\400'` is a NUL. A value
//!   above 0x7F is still materialised as the *code point* of that byte, since a
//!   shell word here is a Rust `String`; see `Lexer::read_ansi_c_quote`.

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

/// Append the code point named by `v` to `out`. Callers mask to a byte first
/// where byte semantics apply.
fn push_code(out: &mut String, v: u32) {
    if let Some(ch) = char::from_u32(v) {
        out.push(ch);
    }
}

/// Read up to `max` hex digits from `chars`, returning their value, or `None`
/// when there was no hex digit at all (so the caller can keep the escape
/// literal).
fn read_hex(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, max: usize) -> Option<u32> {
    let mut val: u32 = 0;
    let mut count = 0;
    while count < max {
        let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) else {
            break;
        };
        val = val.wrapping_mul(16).wrapping_add(d);
        chars.next();
        count += 1;
    }
    (count > 0).then_some(val)
}

/// Decode the ANSI-C `\c` control-character escape, whose operand is whatever
/// follows. Three wrinkles, all of them bash's: `\c?` is DEL rather than
/// `'?' & 0x1f`; a `\c` whose operand is an escaped backslash (`\c\\`) consumes
/// *both* backslashes and yields 0x1c; and a `\c` with nothing after it stays
/// literal — which is why `$'ab\c'` is a four-character word, not a lexer error.
fn decode_control(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, out: &mut String) {
    if chars.peek() == Some(&'\\') {
        // Only `\c\\` collapses. `\c\n` is 0x1c followed by a literal `n`, so
        // look one further before swallowing the first backslash.
        let mut probe = chars.clone();
        probe.next();
        if probe.peek() == Some(&'\\') {
            chars.next();
        }
    }
    match chars.next() {
        Some('?') => out.push('\u{7f}'),
        // The uppercasing is bash's; for the low five bits it is a no-op, but
        // keep it so the intent survives.
        Some(ctrl) => push_code(out, (ctrl.to_ascii_uppercase() as u32) & 0x1f),
        None => {
            out.push('\\');
            out.push('c');
        }
    }
}

/// Decode a single backslash escape. `chars` is positioned immediately after the
/// `\`; the decoded text is appended to `out`.
pub(crate) fn decode_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    out: &mut String,
    mode: EscapeMode,
) -> Decoded {
    let Some(c) = chars.next() else {
        // A trailing backslash is literal in every mode.
        out.push('\\');
        return Decoded::default();
    };
    match c {
        'n' => out.push('\n'),
        't' => out.push('\t'),
        'r' => out.push('\r'),
        'a' => out.push('\u{07}'),
        'b' => out.push('\u{08}'),
        'e' | 'E' => out.push('\u{1b}'),
        'f' => out.push('\u{0c}'),
        'v' => out.push('\u{0b}'),
        '\\' => out.push('\\'),
        '\'' | '"' | '?' if mode.ansi_c_family() => out.push(c),
        'c' => match mode {
            EscapeMode::PrintfB | EscapeMode::EchoE => {
                return Decoded { stop: true, bad: None };
            }
            // printf's FORMAT string has no `\c` at all.
            EscapeMode::PrintfFormat => {
                out.push('\\');
                out.push('c');
            }
            EscapeMode::AnsiC => decode_control(chars, out),
        },
        'x' => match read_hex(chars, 2) {
            // A byte, not a code point: `\xff` names 0xff.
            Some(v) => push_code(out, v & 0xff),
            None => {
                out.push('\\');
                out.push('x');
                if mode.reports_bad_escape() {
                    return Decoded { stop: false, bad: Some("missing hex digit for \\x") };
                }
            }
        },
        'u' | 'U' => {
            let max = if c == 'u' { 4 } else { 8 };
            match read_hex(chars, max) {
                // `\u`/`\U` name a code point, so these are *not* masked.
                Some(v) => push_code(out, v),
                None => {
                    out.push('\\');
                    out.push(c);
                    if mode.reports_bad_escape() {
                        let msg = if c == 'u' {
                            "missing unicode digit for \\u"
                        } else {
                            "missing unicode digit for \\U"
                        };
                        return Decoded { stop: false, bad: Some(msg) };
                    }
                }
            }
        }
        '0'..='7' => {
            let mut oct = String::new();
            match mode {
                // `\0nnn`: the `0` is a prefix, not one of the three digits.
                EscapeMode::PrintfB | EscapeMode::EchoE if c == '0' => {}
                // `echo -e` accepts *only* the `\0nnn` form, so `\101` is the
                // four literal characters `\101` (unlike `printf %b`, which
                // takes both spellings).
                EscapeMode::EchoE => {
                    out.push('\\');
                    out.push(c);
                    return Decoded::default();
                }
                _ => oct.push(c),
            }
            while oct.len() < 3 && chars.peek().is_some_and(|c| ('0'..='7').contains(c)) {
                oct.push(chars.next().unwrap_or('0'));
            }
            if oct.is_empty() {
                // A `\0` with no octal digits after it is a NUL byte.
                out.push('\0');
            } else if let Ok(v) = u32::from_str_radix(&oct, 8) {
                // Octal names a byte too: `\400` is NUL and `\401` is 0x01.
                push_code(out, v & 0xff);
            }
        }
        other => {
            out.push('\\');
            out.push(other);
        }
    }
    Decoded::default()
}

/// ANSI-C (`$'…'` / `${v@E}`) backslash-escape expansion.
pub(crate) fn ansi_c_unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        decode_escape(&mut chars, &mut out, EscapeMode::AnsiC);
    }
    // A NUL byte terminates the ANSI-C string (a shell word cannot hold a NUL),
    // so bytes produced after the first NUL are dropped — `$'a\0b'` is just `a`.
    // This is specific to ANSI-C quoting: `printf %b '\0'` really does write a
    // NUL to stdout.
    if let Some(nul) = out.find('\0') {
        out.truncate(nul);
    }
    out
}

/// The result of decoding one `echo`-family string (`printf %b` or `echo -e`).
pub(crate) struct EchoUnescaped {
    /// The decoded text.
    pub text: String,
    /// A `\c` was seen: the caller must stop producing any further output.
    pub stopped: bool,
    /// Malformed `\x`/`\u`/`\U` escapes, as (offset within `text`, message).
    /// Always empty in [`EscapeMode::EchoE`], which reports nothing.
    pub bad: Vec<(usize, &'static str)>,
}

/// `printf %b` / `echo -e` backslash-escape expansion. `mode` must be
/// [`EscapeMode::PrintfB`] or [`EscapeMode::EchoE`]; they differ in octal
/// syntax and in whether a malformed `\x`/`\u` is reported.
pub(crate) fn unescape_echo(s: &str, mode: EscapeMode) -> EchoUnescaped {
    let mut text = String::new();
    let mut bad = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
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
    use super::{EscapeMode, ansi_c_unescape, unescape_echo};

    fn hex(s: &str) -> String {
        s.bytes().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
    }

    fn echo_b(s: &str) -> String {
        unescape_echo(s, EscapeMode::PrintfB).text
    }

    fn echo_e(s: &str) -> String {
        unescape_echo(s, EscapeMode::EchoE).text
    }

    #[test]
    fn ansi_c_control_escapes_match_bash() {
        // `\cX` is `X & 0x1f`, case-insensitively.
        assert_eq!(hex(&ansi_c_unescape("a\\cAb")), "61 01 62");
        assert_eq!(hex(&ansi_c_unescape("a\\cab")), "61 01 62");
        assert_eq!(hex(&ansi_c_unescape("a\\cb\\cc")), "61 02 03");
        assert_eq!(hex(&ansi_c_unescape("\\c0")), "10");
        assert_eq!(hex(&ansi_c_unescape("\\c{")), "1b");
        // `\c?` is DEL, not `'?' & 0x1f`.
        assert_eq!(hex(&ansi_c_unescape("\\c?")), "7f");
        // `\c\\` consumes both backslashes; `\c\n` consumes only one, leaving a
        // literal `n`.
        assert_eq!(hex(&ansi_c_unescape("\\c\\\\")), "1c");
        assert_eq!(hex(&ansi_c_unescape("x\\c\\nz")), "78 1c 6e 7a");
        assert_eq!(hex(&ansi_c_unescape("\\c\\t")), "1c 74");
        // A dangling `\c` stays literal.
        assert_eq!(hex(&ansi_c_unescape("ab\\c")), "61 62 5c 63");
    }

    #[test]
    fn ansi_c_numeric_escapes_name_bytes() {
        // Octal is masked to 8 bits, so `\400` is a NUL — which truncates.
        assert_eq!(hex(&ansi_c_unescape("\\400")), "");
        assert_eq!(hex(&ansi_c_unescape("a\\401b")), "61 01 62");
        // At most three octal digits, and a leading `0` is one of them.
        assert_eq!(hex(&ansi_c_unescape("\\0101")), "08 31");
        // `\u` names a code point, so it is *not* masked.
        assert_eq!(hex(&ansi_c_unescape("\\u0041")), "41");
        assert_eq!(hex(&ansi_c_unescape("\\x41\\x42")), "41 42");
        // A NUL anywhere truncates the rest of the string.
        assert_eq!(hex(&ansi_c_unescape("a\\0b")), "61");
    }

    #[test]
    fn ansi_c_unknown_escapes_keep_the_backslash() {
        assert_eq!(ansi_c_unescape("\\q\\z"), "\\q\\z");
        assert_eq!(ansi_c_unescape("\\8"), "\\8");
        assert_eq!(ansi_c_unescape("\\xg"), "\\xg");
        assert_eq!(ansi_c_unescape("a\\"), "a\\");
        // …but `\?`, `\'` and `\"` are real escapes here.
        assert_eq!(ansi_c_unescape("\\?"), "?");
        assert_eq!(ansi_c_unescape("\\'\\\""), "'\"");
    }

    #[test]
    fn echo_family_drops_the_ansi_c_only_escapes() {
        // `\?`, `\'` and `\"` are *not* escapes for `%b` / `echo -e`.
        for f in [echo_b as fn(&str) -> String, echo_e] {
            assert_eq!(f("\\?"), "\\?");
            assert_eq!(f("\\'"), "\\'");
            assert_eq!(f("\\\""), "\\\"");
            assert_eq!(hex(&f("\\e|\\a|\\v")), "1b 7c 07 7c 0b");
            assert_eq!(hex(&f("\\x41")), "41");
            assert_eq!(hex(&f("\\u0041")), "41");
            assert_eq!(f("\\8"), "\\8");
            assert_eq!(f("a\\"), "a\\");
        }
    }

    #[test]
    fn echo_family_octal_differs_between_printf_b_and_echo_e() {
        // `%b` takes both spellings…
        assert_eq!(hex(&echo_b("\\0101")), "41");
        assert_eq!(hex(&echo_b("\\101")), "41");
        // …but `echo -e` accepts only the `\0`-prefixed one.
        assert_eq!(hex(&echo_e("\\0101")), "41");
        assert_eq!(hex(&echo_e("\\101")), "5c 31 30 31");
        // Masked to a byte, like ANSI-C: `\0777` is 0x1ff & 0xff = 0xff. (It
        // then encodes as U+00FF — the documented byte-vs-code-point gap — so
        // compare the code point, not the UTF-8 bytes.)
        assert_eq!(echo_b("\\0777").chars().next(), Some('\u{ff}'));
        // A NUL is emitted here, not truncating as ANSI-C quoting would.
        assert_eq!(hex(&echo_b("a\\0b")), "61 00 62");
        assert_eq!(hex(&echo_b("\\09")), "00 39");
    }

    #[test]
    fn echo_family_c_stops_output() {
        let r = unescape_echo("a\\cb", EscapeMode::PrintfB);
        assert_eq!(r.text, "a");
        assert!(r.stopped);
        let r = unescape_echo("a\\cb", EscapeMode::EchoE);
        assert_eq!(r.text, "a");
        assert!(r.stopped);
    }

    #[test]
    fn only_printf_reports_a_malformed_hex_escape() {
        let r = unescape_echo("a\\xg", EscapeMode::PrintfB);
        assert_eq!(r.text, "a\\xg");
        assert_eq!(r.bad, vec![(3, "missing hex digit for \\x")]);
        // `echo -e` keeps the literal but says nothing.
        let r = unescape_echo("a\\xg", EscapeMode::EchoE);
        assert_eq!(r.text, "a\\xg");
        assert!(r.bad.is_empty());
    }
}

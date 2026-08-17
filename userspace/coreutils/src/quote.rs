//! How a utility renders a name, or any other untrusted text, inside a
//! diagnostic.
//!
//! ## Why a diagnostic cannot just print the name
//!
//! A path on this system may hold every byte except `/` and NUL. That includes
//! the newline. So a utility that writes
//!
//! ```text
//! cp: cannot stat: {name}: No such file or directory
//! ```
//!
//! with the name pasted in raw hands whoever chose the name a way to write
//! *whole lines* of its own into the utility's error stream. A file called
//! `a\ncp: /etc/shadow: Permission denied` makes `cp` appear to report a second
//! failure that never happened. Every log reader, every `2>&1 | grep`, and
//! every person scanning a terminal is fooled by it, and nothing in the utility
//! is at fault except that it printed the name unquoted.
//!
//! The second, duller reason is ambiguity: `cannot stat: my file` does not say
//! whether that is one name with a space or the start of a sentence, and a
//! trailing space or a control character is invisible entirely.
//!
//! ## The three styles, and which to use
//!
//! GNU coreutils has exactly three, and the difference is visible enough that
//! matching it matters — a test or a script that reads our diagnostic next to
//! GNU's should see the same bytes.
//!
//! | Function | GNU's name | Used for | `abc` | `a b` | `a'b` |
//! |---|---|---|---|---|---|
//! | [`quotef`] | `quotef` | a name that **ends the message** | `abc` | `'a b'` | `"a'b"` |
//! | [`quoteaf`] | `quoteaf` | a name **inside a sentence** | `'abc'` | `'a b'` | `"a'b"` |
//! | [`quote`] | `quote` | **option arguments**, and anything else | `'abc'` | `'a b'` | `'a\'b'` |
//!
//! The first two produce a shell word: what they print can be pasted back into
//! a shell to name the same file. They differ only in whether an already-safe
//! name keeps its quotes, and **which one to use is decided by the sentence,
//! not by the utility**:
//!
//! ```text
//! wc:   missing.txt: No such file or directory                    <- quotef
//! head: cannot open 'missing.txt' for reading: No such file ...   <- quoteaf
//! ```
//!
//! A name that ends the message can be bare, because nothing follows it to run
//! into; one with words after it cannot, because `cannot open missing files
//! for reading` reads as a phrase rather than as a name. So the rule is: use
//! [`quotef`] where the name is the last thing on the line before the `:
//! reason`, [`quoteaf`] where anything follows it.
//!
//! [`quote`] always quotes, and escapes in C rather than shell style; it is for
//! text that was never a shell word to begin with, where the quotes are
//! punctuation marking where the quoted thing starts and stops. That is why
//! `sort: invalid argument 'bogus' for '--sort'` uses it.
//!
//! ## Where the rules come from
//!
//! Every table and every branch below was **measured**, not recalled: see
//! `scripts/quote-probe.py`, which drives GNU `sort` and `head` over every
//! byte in every position that turns out to matter and records what came out.
//! The result is `tests/quotearg-gnu.txt`, 8333 rows, and `tests/quotearg.rs`
//! asserts this module reproduces all of them.
//!
//! That method is the whole point. Reading gnulib's `quotearg.c` and
//! reimplementing what it appears to say produces something that looks right
//! and differs in a dozen corners — the lone `{`, the `#` that is special only
//! at the front, the `~` that is allowed inside double quotes only at the
//! front, and gnulib's stray `''` prefix. None of those would have been
//! guessed, and each was found by comparing bytes.

/// Bytes a shell needs no quoting for, wherever they appear in a word.
const SAFE: &[u8] = b"%+,-./0123456789@ABCDEFGHIJKLMNOPQRSTUVWXYZ]_\
                      abcdefghijklmnopqrstuvwxyz{}";

/// Safe too, but not as the first byte: `#` starts a comment and `~` starts a
/// home-directory expansion only at the front of a word.
const SAFE_NOT_FIRST: &[u8] = b"#~";

/// Safe unless it is the *whole* word. A lone `{` or `}` is a bash reserved
/// word; one inside a word is an ordinary character. (Brace *expansion* needs
/// a comma or a `..` between them, both of which force quoting anyway.)
const SAFE_UNLESS_ALONE: &[u8] = b"{}";

/// Bytes that may sit inside the `"..."` form.
const DQ_SAFE: &[u8] = b" %'+,-.0123456789:@ABCDEFGHIJKLMNOPQRSTUVWXYZ]_\
                         abcdefghijklmnopqrstuvwxyz";

/// Allowed inside `"..."` too — but only as the *first* byte, which is the
/// opposite way round from [`SAFE_NOT_FIRST`] and is not a transcription
/// error. GNU does this, `scripts/quote-probe.py` shows it, and the fixture
/// pins it: `~a'z` comes out `"~a'z"` while `a'z~` comes out `'a'\''z~'`.
const DQ_SAFE_FIRST_ONLY: &[u8] = b"#~";

/// Whether a byte prints as itself in the C locale.
const fn printable(b: u8) -> bool {
    b >= 0x20 && b < 0x7f
}

/// The C escape for a byte that does not print: the named one where there is
/// one, otherwise three octal digits.
///
/// Always three digits, never fewer: `\1` followed by a literal `2` would read
/// back as `\12`, so a shortened escape can change the string it decodes to.
fn c_escape(b: u8, out: &mut String) {
    let named = match b {
        0x07 => 'a',
        0x08 => 'b',
        0x09 => 't',
        0x0a => 'n',
        0x0b => 'v',
        0x0c => 'f',
        0x0d => 'r',
        _ => {
            // Three octal digits, most significant first. Written out rather
            // than formatted so this stays allocation-free and obviously
            // total.
            out.push('\\');
            out.push((b'0' + (b >> 6)) as char);
            out.push((b'0' + ((b >> 3) & 7)) as char);
            out.push((b'0' + (b & 7)) as char);
            return;
        }
    };
    out.push('\\');
    out.push(named);
}

/// Render `arg` the way GNU's `quote()` does: always inside `'...'`, with C
/// escapes.
///
/// This is for text that is not a file name — an option's argument, a word
/// from a configuration file, a field from input being echoed back. The quotes
/// are always present because their job here is to mark where the quoted thing
/// starts and stops, which a bare word does not do.
///
/// The result is always printable ASCII, whatever `arg` held.
///
/// ```
/// use coreutils::quote::quote;
/// assert_eq!(quote(b"bogus"), "'bogus'");
/// assert_eq!(quote(b"a b"), "'a b'");
/// assert_eq!(quote(b"it's"), r"'it\'s'");
/// assert_eq!(quote(b"a\tb"), r"'a\tb'");
/// assert_eq!(quote(b"\xff"), r"'\377'");
/// ```
#[must_use]
pub fn quote(arg: &[u8]) -> String {
    let mut out = String::with_capacity(arg.len().saturating_add(2));
    out.push('\'');
    for &b in arg {
        if b == b'\\' || b == b'\'' {
            out.push('\\');
            out.push(b as char);
        } else if printable(b) {
            out.push(b as char);
        } else {
            c_escape(b, &mut out);
        }
    }
    out.push('\'');
    out
}

/// Render `name` the way GNU's `quotef()` does: quoted only where a shell
/// would need it, and in a form that can be pasted back into a shell.
///
/// This is for file names. An ordinary one comes back unchanged, which is what
/// keeps the common diagnostic readable; anything a shell would treat
/// specially — a space, a metacharacter, a control byte — is quoted or
/// escaped, which is what stops a name from forging a line of its own.
///
/// The result is always printable ASCII, whatever `name` held.
///
/// ```
/// use coreutils::quote::quotef;
/// assert_eq!(quotef(b"notes.txt"), "notes.txt");
/// assert_eq!(quotef(b"my notes.txt"), "'my notes.txt'");
/// assert_eq!(quotef(b"it's"), "\"it's\"");
/// assert_eq!(quotef(b"two\nlines"), r"'two'$'\n''lines'");
/// assert_eq!(quotef(b""), "''");
/// ```
#[must_use]
pub fn quotef(name: &[u8]) -> String {
    render(name, true)
}

/// Render `name` the way GNU's `quoteaf()` does: the same shell-pasteable form
/// as [`quotef`], except that the quotes are never left off.
///
/// The choice between this and [`quotef`] is made by the *shape of the
/// sentence*, not by the utility. A name that ends the message can be bare,
/// because nothing follows it to run into:
///
/// ```text
/// wc: missing.txt: No such file or directory
/// ```
///
/// A name embedded in a sentence cannot, because a bare one would blur into
/// the words around it — `cannot open missing files for reading` reads as a
/// phrase rather than as a name:
///
/// ```text
/// head: cannot open 'missing.txt' for reading: No such file or directory
/// ```
///
/// ```
/// use coreutils::quote::quoteaf;
/// assert_eq!(quoteaf(b"notes.txt"), "'notes.txt'");
/// assert_eq!(quoteaf(b"my notes.txt"), "'my notes.txt'");
/// assert_eq!(quoteaf(b"it's"), "\"it's\"");
/// assert_eq!(quoteaf(b""), "''");
/// ```
#[must_use]
pub fn quoteaf(name: &[u8]) -> String {
    render(name, false)
}

/// The body of both shell-escaping styles. `allow_bare` is the whole
/// difference between them: gnulib calls it "elide outer quotes", and it is a
/// property of the sentence the name is going into, not of the name.
fn render(name: &[u8], allow_bare: bool) -> String {
    if allow_bare && !name.is_empty() && name.iter().enumerate().all(|(i, &b)| bare_ok(name, i, b))
    {
        // Safe as it stands. This is the overwhelmingly common case and the
        // reason `quotef` exists rather than everything using `quote`.
        return name.iter().map(|&b| b as char).collect();
    }
    if !name.is_empty()
        && name.contains(&b'\'')
        && name.iter().enumerate().all(|(i, &b)| dq_ok(i, b))
    {
        // A name holding a single quote reads far better wrapped in double
        // quotes than spliced with '\'' at every occurrence.
        let mut out = String::with_capacity(name.len().saturating_add(2));
        out.push('"');
        out.extend(name.iter().map(|&b| b as char));
        out.push('"');
        return out;
    }
    shell_escape(name)
}

fn bare_ok(name: &[u8], i: usize, b: u8) -> bool {
    if SAFE_UNLESS_ALONE.contains(&b) {
        return name.len() > 1;
    }
    SAFE.contains(&b) || (i > 0 && SAFE_NOT_FIRST.contains(&b))
}

fn dq_ok(i: usize, b: u8) -> bool {
    DQ_SAFE.contains(&b) || (i == 0 && DQ_SAFE_FIRST_ONLY.contains(&b))
}

/// The general form: a run of literal bytes inside `'...'`, a `'` spliced as
/// `'\''`, and a run of unprintable bytes inside `$'...'`.
fn shell_escape(name: &[u8]) -> String {
    let mut out = String::with_capacity(name.len().saturating_mul(2).saturating_add(2));

    // gnulib emits two more quotes here than the rendering needs, for exactly
    // these inputs. It is a wart -- `''` is an empty shell word, so the result
    // means the same thing either way -- but it is reproduced so that our
    // diagnostics are byte-identical to GNU's rather than *nearly* identical,
    // which is the difference between a comparison being a check and being a
    // source of noise. 163 of the fixture's rows depend on it; do not "fix" it
    // without regenerating `tests/quotearg-gnu.txt` and finding it gone.
    if name.contains(&b'\'')
        && name.last().is_some_and(|&b| !printable(b))
        && name.first() != Some(&b'\'')
    {
        out.push_str("''");
    }

    out.push('\'');
    let mut open = true;
    let mut i = 0usize;
    while let Some(&b) = name.get(i) {
        if b == b'\'' {
            // Close, escape the quote outside any quoting, reopen.
            if open {
                out.push('\'');
            }
            out.push_str("\\''");
            open = true;
            i = i.saturating_add(1);
        } else if printable(b) {
            if !open {
                out.push('\'');
                open = true;
            }
            out.push(b as char);
            i = i.saturating_add(1);
        } else {
            // A whole run of unprintable bytes shares one `$'...'`.
            if open {
                out.push('\'');
            }
            out.push_str("$'");
            while let Some(&b) = name.get(i) {
                if printable(b) {
                    break;
                }
                c_escape(b, &mut out);
                i = i.saturating_add(1);
            }
            out.push('\'');
            open = false;
        }
    }
    if open {
        out.push('\'');
    }
    out
}

/// The bytes behind an `OsStr`.
///
/// On the target this is the string itself — a path is bytes there. On a
/// Windows *host* it is a lossy conversion, because there is no byte view of a
/// `OsStr` that round-trips; that only affects developing and testing on
/// Windows, never a running SlateOS.
#[cfg(unix)]
#[must_use]
pub fn os_bytes(s: &std::ffi::OsStr) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    std::borrow::Cow::Borrowed(s.as_bytes())
}

#[cfg(not(unix))]
#[must_use]
pub fn os_bytes(s: &std::ffi::OsStr) -> std::borrow::Cow<'_, [u8]> {
    match s.to_string_lossy() {
        std::borrow::Cow::Borrowed(t) => std::borrow::Cow::Borrowed(t.as_bytes()),
        std::borrow::Cow::Owned(t) => std::borrow::Cow::Owned(t.into_bytes()),
    }
}

/// [`quotef`] for a path or any other `OsStr`, which is what almost every call
/// site actually has.
///
/// ```
/// use coreutils::quote::quotef_os;
/// use std::ffi::OsStr;
/// assert_eq!(quotef_os(OsStr::new("a b")), "'a b'");
/// ```
#[must_use]
pub fn quotef_os(s: &std::ffi::OsStr) -> String {
    quotef(&os_bytes(s))
}

/// [`quoteaf`] for a path or any other `OsStr`.
///
/// ```
/// use coreutils::quote::quoteaf_os;
/// use std::ffi::OsStr;
/// assert_eq!(quoteaf_os(OsStr::new("a.txt")), "'a.txt'");
/// ```
#[must_use]
pub fn quoteaf_os(s: &std::ffi::OsStr) -> String {
    quoteaf(&os_bytes(s))
}

/// [`quote`] for an `OsStr`.
#[must_use]
pub fn quote_os(s: &std::ffi::OsStr) -> String {
    quote(&os_bytes(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_left_alone() {
        for name in [
            &b"notes.txt"[..],
            b"Makefile",
            b"a-b_c.d",
            b"/usr/local/bin",
            b"100%",
            b"a#b",
            b"a~b",
            b"a{b}c",
            b"-",
            b"--",
        ] {
            assert_eq!(quotef(name), String::from_utf8_lossy(name), "{name:?}");
        }
    }

    #[test]
    fn shell_metacharacters_force_quotes() {
        assert_eq!(quotef(b"a b"), "'a b'");
        assert_eq!(quotef(b"a*b"), "'a*b'");
        assert_eq!(quotef(b"a|b"), "'a|b'");
        assert_eq!(quotef(b"a;b"), "'a;b'");
        assert_eq!(quotef(b"a$b"), "'a$b'");
        assert_eq!(quotef(b"a:b"), "'a:b'");
        assert_eq!(quotef(b"a?b"), "'a?b'");
        // Only at the front, where the shell reads them.
        assert_eq!(quotef(b"#a"), "'#a'");
        assert_eq!(quotef(b"~a"), "'~a'");
        // A lone brace is a bash reserved word; one inside a word is not.
        assert_eq!(quotef(b"{"), "'{'");
        assert_eq!(quotef(b"}"), "'}'");
        assert_eq!(quotef(b"a{"), "a{");
    }

    #[test]
    fn a_name_cannot_forge_a_second_diagnostic_line() {
        // This is the reason the module exists. Neither rendering contains a
        // newline, so neither can appear to be two messages.
        let forged = b"x\ncp: /etc/shadow: Permission denied";
        assert!(!quotef(forged).contains('\n'));
        assert!(!quote(forged).contains('\n'));
        assert_eq!(
            quotef(b"a\nb"),
            r"'a'$'\n''b'",
            "a newline becomes a visible escape"
        );
    }

    #[test]
    fn every_rendering_is_printable_ascii() {
        for b in 0u8..=255 {
            for shape in [vec![b], vec![b'a', b, b'z'], vec![b, b'\'']] {
                for rendered in [quotef(&shape), quote(&shape)] {
                    assert!(
                        rendered.bytes().all(printable),
                        "{b:#04x} rendered as {rendered:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn single_quotes_pick_the_double_quoted_form_where_gnu_does() {
        assert_eq!(quotef(b"it's"), "\"it's\"");
        assert_eq!(quotef(b"'"), "\"'\"");
        assert_eq!(quotef(b"~a'z"), "\"~a'z\"");
        // ...but not once something appears that a double quote would not
        // protect.
        assert_eq!(quotef(b"a'z$"), r"'a'\''z$'");
        assert_eq!(quotef(b"a'z~"), r"'a'\''z~'");
        assert_eq!(quotef(b"a'z\""), "'a'\\''z\"'");
    }

    #[test]
    fn unprintable_runs_share_one_escape() {
        assert_eq!(quotef(b"a\n\tz"), r"'a'$'\n\t''z'");
        assert_eq!(quotef(b"\n"), r"''$'\n'");
        assert_eq!(quotef(b"a\xffz"), r"'a'$'\377''z'");
    }

    #[test]
    fn octal_escapes_are_always_three_digits() {
        // \1 followed by a literal 2 would read back as \12, a different byte.
        assert_eq!(quote(b"\x01" as &[u8]), r"'\001'");
        assert_eq!(quote(b"\x01" as &[u8]).len(), 6);
        assert_eq!(quotef(b"\x012"), r"''$'\001''2'");
    }

    #[test]
    fn the_empty_string_is_a_pair_of_quotes_in_every_style() {
        assert_eq!(quotef(b""), "''");
        assert_eq!(quoteaf(b""), "''");
        assert_eq!(quote(b""), "''");
    }

    #[test]
    fn quoteaf_differs_from_quotef_only_where_quotef_would_go_bare() {
        // The one case: a name that needs nothing.
        assert_eq!(quotef(b"notes.txt"), "notes.txt");
        assert_eq!(quoteaf(b"notes.txt"), "'notes.txt'");
        // Everywhere else the two agree, because once a name needs quoting at
        // all there is no "outer quote" left to elide.
        for name in [
            &b"a b"[..],
            b"it's",
            b"a\nb",
            b"a'z$",
            b"#a",
            b"{",
            b"a\x01'z",
            b"",
        ] {
            assert_eq!(quotef(name), quoteaf(name), "{name:?}");
        }
    }
}

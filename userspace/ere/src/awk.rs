//! awk's regular expressions: gawk's escape layers in front of the engine.
//!
//! ## Why awk has layers of its own
//!
//! POSIX gives awk's regular expressions C's escapes on top of ERE. An awk ERE
//! "shall allow the use of C-language conventions for escaping special
//! characters within the EREs ... these escape sequences shall be recognized
//! both inside and outside bracket expressions", and awk's own table adds `\"`,
//! `\/` and the octal `\ddd`. So in awk `[\t]` is a tab and `\101` is an `A`,
//! where for `grep -E` the first is "a backslash or a `t`" and the second is a
//! reference to a group. [`crate::engine`] answers grep's question, exactly as
//! glibc's `regcomp` does, and this module is what gawk puts in front of
//! `regcomp` to answer awk's. Doing it here rather than in the engine is the
//! only way one engine can be right for both: the alternative, an engine that
//! read C escapes, was wrong for `grep`, `sed`, `ed`, `find`, `expr` and the
//! shell's `[[ =~ ]]` all at once (`known-issues.md`,
//! TD-B-ERE-BRACKET-BACKSLASH).
//!
//! It is a module of this crate rather than of `awk` for the reason the crate
//! exists: there are two awks, userspace's and the kernel shell's, and a layer
//! each kept its own copy of would be two answers to one question.
//!
//! | here | gawk 5.2.1 | applies to |
//! |---|---|---|
//! | [`string`] | `make_str_node(.., SCAN)` and `parse_escape` (node.c) | a string literal's text, and the values of `-v`, `-F` and `var=value` operands |
//! | [`regexp`] | the first half of `make_regexp` (re.c) | `/re/` literals, and every string used as a regex: `~`, `match`, `split`, `sub`, `gsub`, `FS`, `RS` |
//! | [`compile`] | all of `make_regexp` | the same, compiled |
//!
//! ## `--posix`
//!
//! The dialect is `gawk --posix` (`scripts/awk-diff.sh`), and gawk's `--posix`
//! sets its `do_traditional` as well. Between the two, `\x` is not a hex
//! escape but an `x`, `\y` is not gawk's word boundary, and the escapes a
//! regex may carry without a warning are exactly `{}()|*+?.^$\[]/-`. The GNU
//! operators -- `\w`, `\<`, `\B` and the rest -- are not among them, and the
//! syntax the regex is then compiled under, [`Syntax::POSIX_AWK`], does not
//! read them as operators either.
//!
//! ## Warnings
//!
//! gawk warns about an escape it does not know -- once per character per run.
//! Each of its three messages keeps its own `static bool warned[]`, so `\q` in
//! a string and `\q` in a regex warn once each, and the second `\q` in either
//! says nothing. [`Warnings`] is those three tables, and keeps what has been
//! said until the caller writes it, so that whoever owns standard error -- and
//! knows how its program prefixes a diagnostic -- decides how and when.

use alloc::vec::Vec;

use crate::ch::Str;
use crate::engine::{EreError, Regex, Syntax};

/// The characters a regex may escape without a warning: gawk's
/// `ok_to_escape` under `--posix`.
const OK_TO_ESCAPE: &[u8] = b"{}()|*+?.^$\\[]/-";

/// gawk's three "say it once" tables, and what they have said.
#[derive(Clone)]
pub struct Warnings {
    /// `parse_escape`'s `warned[256]`.
    string: [bool; 256],
    /// `make_regexp`'s `warned[256]`: an escape that is not a regex operator.
    regexp: [bool; 256],
    /// `make_regexp`'s `warned[2]`: `\8` and `\9`.
    digit: [bool; 2],
    /// Each message not yet written, oldest first, as gawk words what follows
    /// its `warning: `. Bytes, because the character a message quotes is the
    /// program's own byte and need not be text.
    said: Vec<Str>,
}

impl Default for Warnings {
    fn default() -> Self {
        Warnings {
            string: [false; 256],
            regexp: [false; 256],
            digit: [false; 2],
            said: Vec::new(),
        }
    }
}

impl Warnings {
    /// Every message said since the last call, oldest first.
    pub fn take(&mut self) -> Vec<Str> {
        core::mem::take(&mut self.said)
    }

    /// Whether anything has been said that [`Warnings::take`] has not taken.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.said.is_empty()
    }

    /// `parse_escape`'s last resort: an escape awk does not know is the
    /// character itself.
    fn plain(&mut self, c: u8) {
        if first_time(&mut self.string, c) {
            self.said.push(cat(&[
                b"escape sequence `\\",
                &[c],
                b"' treated as plain `",
                &[c],
                b"'",
            ]));
        }
    }

    /// `make_regexp` on an escaped character a regex has no use for.
    fn not_an_operator(&mut self, c: u8) {
        if first_time(&mut self.regexp, c) {
            self.said.push(cat(&[
                b"regexp escape sequence `\\",
                &[c],
                b"' is not a known regexp operator",
            ]));
        }
    }

    /// `make_regexp` on `\8` or `\9`, which name no octal digit.
    fn plain_digit(&mut self, c: u8) {
        let slot = self.digit.get_mut(usize::from(c.wrapping_sub(b'8')));
        if let Some(slot @ false) = slot {
            *slot = true;
            self.said.push(cat(&[
                b"regexp escape sequence `\\",
                &[c],
                b"' treated as plain `",
                &[c],
                b"'",
            ]));
        }
    }
}

/// Mark `c` in `table`, answering whether it was unmarked.
fn first_time(table: &mut [bool; 256], c: u8) -> bool {
    match table.get_mut(usize::from(c)) {
        Some(slot @ false) => {
            *slot = true;
            true
        }
        _ => false,
    }
}

/// The pieces of a message, joined. Every message here quotes a character the
/// way gawk's `%c` does -- as the byte it is, whatever that is.
fn cat(parts: &[&[u8]]) -> Str {
    parts.concat()
}

/// What `parse_escape` made of the character after a backslash.
enum Escape {
    /// The byte it names, and the index just past the escape.
    Byte(u8, usize),
    /// gawk's `-1`: the text ended at the backslash, or a NUL byte follows it.
    /// Nothing is consumed.
    End,
    /// gawk's `-2`: a newline follows the backslash, and is consumed.
    Newline(usize),
}

/// gawk's `parse_escape`, under `--posix`; `raw[i]` is the character after the
/// backslash.
fn parse_escape(raw: &[u8], i: usize, w: &mut Warnings) -> Escape {
    let Some(&c) = raw.get(i) else {
        return Escape::End;
    };
    let next = i.saturating_add(1);
    let byte = match c {
        b'a' => 0x07,
        b'b' => 0x08,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'v' => 0x0b,
        b'\n' => return Escape::Newline(next),
        // gawk reads the terminator its strings carry and steps back off it,
        // so a NUL inside the text is the same answer as the end of it.
        0 => return Escape::End,
        b'0'..=b'7' => {
            // Up to three octal digits, this one included. The value is an
            // `int` that gawk then stores into a `char`, so `\777` is 0xFF and
            // `\400` is NUL: the low byte, which is what `& 0xff` keeps.
            let mut v = u32::from(c.wrapping_sub(b'0'));
            let mut j = next;
            for _ in 1..3 {
                match raw.get(j) {
                    Some(&d @ b'0'..=b'7') => {
                        v = v
                            .wrapping_mul(8)
                            .wrapping_add(u32::from(d.wrapping_sub(b'0')));
                        j = j.saturating_add(1);
                    }
                    _ => break,
                }
            }
            return Escape::Byte(u8::try_from(v & 0xff).unwrap_or(0), j);
        }
        // Under `--posix`, `\x` is not a hex escape: it is an `x`, and the
        // digits after it are ordinary characters. Not a warning either --
        // gawk only lints it.
        b'x' => b'x',
        b'\\' | b'"' => c,
        _ => {
            w.plain(c);
            c
        }
    };
    Escape::Byte(byte, next)
}

/// gawk's `make_str_node(.., SCAN)`: the text of a string, its escapes
/// resolved.
///
/// `elide_back_nl` is gawk's `ELIDE_BACK_NL`, which only `-v` and `var=value`
/// operands set: with it, a backslash before a newline -- or at the very end --
/// vanishes, and without it the backslash stays and the newline goes.
pub fn string(raw: &[u8], elide_back_nl: bool, w: &mut Warnings) -> Str {
    let mut out = Str::with_capacity(raw.len());
    let mut i = 0usize;
    while let Some(&c) = raw.get(i) {
        i = i.saturating_add(1);
        if c != b'\\' {
            out.push(c);
            continue;
        }
        match parse_escape(raw, i, w) {
            Escape::Byte(b, next) => {
                out.push(b);
                i = next;
            }
            Escape::End => {
                if !elide_back_nl {
                    out.push(b'\\');
                }
            }
            Escape::Newline(next) => {
                if !elide_back_nl {
                    out.push(b'\\');
                }
                i = next;
            }
        }
    }
    out
}

/// gawk's one fatal error in its escape layer: a NUL byte straight after a
/// backslash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NulAfterBackslash;

impl NulAfterBackslash {
    /// gawk's words for it, which say "dynamic" because a literal cannot
    /// usually hold a NUL -- though one read from a `-f` file can, and gets
    /// the same words.
    #[must_use]
    pub fn message(self) -> &'static str {
        "invalid NUL byte in dynamic regexp"
    }
}

/// Why [`compile`] refused a regex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// The escape layer's refusal.
    Nul(NulAfterBackslash),
    /// The compiler's; [`EreError::message`] is the sentence glibc's
    /// `regcomp` would have given gawk, which gawk prints.
    Regex(EreError),
}

impl CompileError {
    /// The sentence gawk prints for this refusal, after its `invalid regexp: `
    /// in the compiler's case.
    #[must_use]
    pub fn message(&self) -> &'static str {
        match self {
            CompileError::Nul(n) => n.message(),
            CompileError::Regex(e) => e.message(),
        }
    }
}

/// All of gawk's `make_regexp`: `raw` through [`regexp`], then the compiler
/// under [`Syntax::POSIX_AWK`]. `ci` folds case, as `IGNORECASE` would.
///
/// # Errors
/// [`CompileError`], from whichever of the two stopped first.
pub fn compile(raw: &[u8], ci: bool, w: &mut Warnings) -> Result<Regex, CompileError> {
    let pattern = regexp(raw, w).map_err(CompileError::Nul)?;
    Regex::new_syntax(&pattern, ci, Syntax::POSIX_AWK).map_err(CompileError::Regex)
}

/// The first half of gawk's `make_regexp`: resolve the escapes awk gives a
/// regular expression, and hand every other escape on to the regex compiler
/// as written.
///
/// What comes out is for [`Syntax::POSIX_AWK`], which reads a backslash inside
/// a bracket expression as quoting the next character -- so `[\.]` is a dot,
/// where `grep` would add a backslash -- and reads no GNU operators.
///
/// # Errors
/// [`NulAfterBackslash`], gawk's one fatal here.
pub fn regexp(raw: &[u8], w: &mut Warnings) -> Result<Str, NulAfterBackslash> {
    let mut out = Str::with_capacity(raw.len());
    let mut i = 0usize;
    while let Some(&c) = raw.get(i) {
        i = i.saturating_add(1);
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let Some(&e) = raw.get(i) else {
            // A backslash at the very end goes through, and the compiler
            // refuses it -- `Trailing backslash` -- exactly as glibc does for
            // gawk.
            out.push(b'\\');
            break;
        };
        match e {
            0 => return Err(NulAfterBackslash),
            b'a' | b'b' | b'f' | b'n' | b'r' | b't' | b'v' | b'x' | b'0'..=b'7' => {
                // Every one of these is a byte to `parse_escape`; the other two
                // answers cannot arise.
                if let Escape::Byte(b, next) = parse_escape(raw, i, w) {
                    out.push(b);
                    i = next;
                }
            }
            // No octal digit: the digit itself, with its backslash gone.
            b'8' | b'9' => {
                w.plain_digit(e);
                out.push(e);
                i = i.saturating_add(1);
            }
            _ => {
                if !OK_TO_ESCAPE.contains(&e) {
                    w.not_an_operator(e);
                }
                out.push(b'\\');
                out.push(e);
                i = i.saturating_add(1);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn said(w: &mut Warnings) -> Vec<String> {
        w.take()
            .into_iter()
            .map(|m| String::from_utf8(m).unwrap_or_default())
            .collect()
    }

    /// Every row measured against gawk 5.2.1 `--posix`, `print` of the
    /// string's value.
    #[test]
    fn a_string_resolves_awks_escapes() {
        let mut w = Warnings::default();
        let s = |raw: &[u8], w: &mut Warnings| string(raw, false, w);
        assert_eq!(s(br"a\tb\nc", &mut w), b"a\tb\nc");
        assert_eq!(s(br"\a\b\f\r\v", &mut w), b"\x07\x08\x0c\r\x0b");
        assert_eq!(s(br#"\\\""#, &mut w), b"\\\"");
        // Octal: up to three digits, the low byte of the value.
        assert_eq!(s(br"\101\1012", &mut w), b"AA2");
        assert_eq!(s(br"\7", &mut w), b"\x07");
        assert_eq!(s(br"\0", &mut w), b"\0");
        assert_eq!(s(br"\777", &mut w), b"\xff");
        assert_eq!(s(br"\400", &mut w), b"\0");
        // `--posix`: no hex escape, and no warning for the `x` either.
        assert_eq!(s(br"\x41", &mut w), b"x41");
        assert!(said(&mut w).is_empty());
        // Anything else is itself, with a warning once per character.
        assert_eq!(s(br"\q\.\q\/\8", &mut w), b"q.q/8");
        assert_eq!(
            said(&mut w),
            [
                "escape sequence `\\q' treated as plain `q'",
                "escape sequence `\\.' treated as plain `.'",
                "escape sequence `\\/' treated as plain `/'",
                "escape sequence `\\8' treated as plain `8'",
            ]
        );
        assert_eq!(s(br"\q", &mut w), b"q");
        assert!(said(&mut w).is_empty(), "a character warns once per run");
    }

    /// gawk's `ELIDE_BACK_NL`: set for `-v` and `var=value`, not for `-F`.
    #[test]
    fn a_trailing_or_newline_backslash_depends_on_elision() {
        let mut w = Warnings::default();
        assert_eq!(string(b"a\\\nb", false, &mut w), b"a\\b");
        assert_eq!(string(b"a\\\nb", true, &mut w), b"ab");
        assert_eq!(string(b"a\\", false, &mut w), b"a\\");
        assert_eq!(string(b"a\\", true, &mut w), b"a");
        // A NUL after the backslash is gawk's end of string: the backslash
        // stays (or goes), and the NUL is still there after it.
        assert_eq!(string(b"a\\\0b", false, &mut w), b"a\\\0b");
        assert!(said(&mut w).is_empty());
    }

    /// The regex layer resolves awk's escapes and leaves the compiler's.
    /// Measured against gawk 5.2.1 `--posix` by what each pattern matches.
    #[test]
    fn a_regex_resolves_awks_escapes_and_keeps_the_compilers() {
        let mut w = Warnings::default();
        let r = |raw: &[u8], w: &mut Warnings| regexp(raw, w).unwrap();
        assert_eq!(r(br"a\tb", &mut w), b"a\tb");
        assert_eq!(r(br"[\t]", &mut w), b"[\t]");
        assert_eq!(r(br"\101", &mut w), b"A");
        assert_eq!(r(br"(.)\1", &mut w), b"(.)\x01");
        assert_eq!(r(br"\x41", &mut w), b"x41");
        assert_eq!(r(br"\b", &mut w), b"\x08");
        // The compiler's own: kept, backslash and all, and not warned about.
        for kept in [
            &br"\."[..],
            br"\\",
            br"\/",
            br"\-",
            br"\[",
            br"\]",
            br"\{",
            br"\}",
            br"\(",
            br"\)",
            br"\|",
            br"\*",
            br"\+",
            br"\?",
            br"\^",
            br"\$",
        ] {
            assert_eq!(r(kept, &mut w), kept);
        }
        assert!(said(&mut w).is_empty());
        // Not an operator under `--posix`: kept, and warned about once.
        assert_eq!(r(br"\y\w\<\y", &mut w), br"\y\w\<\y");
        assert_eq!(
            said(&mut w),
            [
                "regexp escape sequence `\\y' is not a known regexp operator",
                "regexp escape sequence `\\w' is not a known regexp operator",
                "regexp escape sequence `\\<' is not a known regexp operator",
            ]
        );
        // `\8` and `\9` lose the backslash, each warning once.
        assert_eq!(r(br"\8\9\8", &mut w), b"898");
        assert_eq!(
            said(&mut w),
            [
                "regexp escape sequence `\\8' treated as plain `8'",
                "regexp escape sequence `\\9' treated as plain `9'",
            ]
        );
        // A final backslash is the compiler's to refuse.
        assert_eq!(r(b"a\\", &mut w), b"a\\");
        assert_eq!(
            regexp(b"a\\\0", &mut w).unwrap_err().message(),
            "invalid NUL byte in dynamic regexp"
        );
    }

    /// gawk keeps one table per message: a string's `\q` and a regex's `\q`
    /// are two warnings, each once.
    #[test]
    fn the_three_tables_are_separate() {
        let mut w = Warnings::default();
        string(br"\q", false, &mut w);
        regexp(br"\q", &mut w).unwrap();
        regexp(br"\8", &mut w).unwrap();
        string(br"\8", false, &mut w);
        string(br"\q", false, &mut w);
        regexp(br"\q", &mut w).unwrap();
        assert_eq!(said(&mut w).len(), 4);
    }
}

//! GNU's **Emacs** regular-expression syntax, by translation to Extended.
//!
//! `RE_SYNTAX_EMACS` is glibc's syntax `0`: what `re_compile_pattern` reads
//! when a program never sets `re_syntax_options`. GNU `ptx` is such a program
//! -- its `-S` and `-W` patterns, and the end-of-sentence pattern it has built
//! in, are all read this way -- and it is the reason this module exists. It is
//! BRE's grouping with ERE's repetition, and less of everything else:
//!
//! | written | Emacs means | ERE means |
//! |---|---|---|
//! | `a+`, `a?` | repetition | the same |
//! | `\+`, `\?` | a literal `+`, `?` | the same |
//! | `\(a\|b\)` | a group holding an alternation | seven literal characters |
//! | `(a\|b)` | five literal characters | a group |
//! | `a{2}`, `a\{2\}` | literal braces | an interval, a literal |
//! | `[[:alpha:]]` | a bracket of `[` `:` `a` `l` `p` `h`, then `]` | a class |
//! | `.` | any character **but newline** | any character |
//! | `^` | an anchor only first in the pattern, a `\(` or a `\|` | an anchor |
//! | `$` | an anchor only last in the pattern, a `\(…\)` or a `\|` branch | an anchor |
//! | `*`, `+`, `?` with nothing before them | a literal | an error |
//!
//! Like [`crate::bre`], this rewrites the pattern and hands the result to the
//! one engine, so that the matching -- the part with the ReDoS bound and the
//! leftmost-longest rule -- is shared rather than written again.
//!
//! # What "nothing before them" means
//!
//! glibc reads a repetition operator as a literal when it begins an expression:
//! at the start of the pattern, straight after `\(` or `\|`, and straight after
//! an anchor of any kind -- `^`, `$`, the word assertions, the buffer anchors
//! -- because an anchor is parsed as an expression of its own that can take no
//! operator. So `^*` is an anchor and an asterisk, and so is `\<*`.
//!
//! # Brackets
//!
//! Without `RE_CHAR_CLASSES` a `[:` inside a bracket is two members, not the
//! start of a class; `[.x.]` and `[=x=]` are still read, and must name one
//! character. A backslash is a member like any other. A `-` that is neither
//! the first member nor the last is an error, as it is in glibc, where POSIX
//! leaves it undefined. The bracket is rebuilt for the ERE parser rather than
//! copied, since that parser does read `[:` as a class and a backslash as an
//! escape.
//!
//! # Not here: newline anchoring
//!
//! `re_compile_pattern` also sets `newline_anchor`, so that `^` and `$` match
//! at every line. That is a property of the entry point, not of the syntax, and
//! it is the caller's to ask for: [`Regex::with_newline_anchor`].

use alloc::vec::Vec;

use crate::bre::ends_here;
use crate::ch::{BStr, Ch, Str, chars};
use crate::engine::{EreError, RegCode, Regex};

/// Compile an Emacs-syntax pattern, `ci` selecting case-insensitive matching.
///
/// # Errors
/// Returns the translation's error, or the ERE engine's, whichever stops first.
pub fn compile(pattern: BStr<'_>, ci: bool) -> Result<Regex, EreError> {
    let ere = to_ere(pattern)?;
    Regex::new_flags(&ere, ci)
}

/// Translate an Emacs-syntax pattern into the equivalent ERE.
///
/// # Errors
/// Returns [`EreError`] for a trailing backslash, an unmatched `\(` or `\)`, an
/// unterminated bracket, a bad range, or a `[.x.]`/`[=x=]` naming no single
/// character.
pub fn to_ere(pattern: BStr<'_>) -> Result<Str, EreError> {
    let cs: Vec<Ch> = chars(pattern).collect();
    let mut out = Str::new();
    let mut i = 0usize;
    // Whether a repetition operator here has something to repeat.
    let mut prev_atom = false;
    // Whether a `^` here is an anchor: first in the pattern, a `\(` or a `\|`.
    // glibc's `RE_CARET_ANCHORS_HERE`, which it sets for exactly one token.
    let mut caret_anchors = true;
    let mut depth = 0usize;

    while let Some(&c) = cs.get(i) {
        let caret_here = core::mem::replace(&mut caret_anchors, false);
        match c.as_ascii() {
            Some('\\') => {
                let Some(&e) = cs.get(i.saturating_add(1)) else {
                    return Err(EreError::new(
                        RegCode::TrailingBackslash,
                        b"trailing backslash in regex".to_vec(),
                    ));
                };
                i = i.saturating_add(2);
                match e.as_ascii() {
                    Some('(') => {
                        out.push(b'(');
                        depth = depth.saturating_add(1);
                        prev_atom = false;
                        caret_anchors = true;
                    }
                    Some(')') => {
                        if depth == 0 {
                            return Err(EreError::new(
                                RegCode::UnmatchedRightParen,
                                br"unmatched \)".to_vec(),
                            ));
                        }
                        out.push(b')');
                        depth = depth.saturating_sub(1);
                        prev_atom = true;
                    }
                    Some('|') => {
                        out.push(b'|');
                        prev_atom = false;
                        caret_anchors = true;
                    }
                    // Backreferences and the character-class abbreviations pass
                    // through: the ERE parser reads them the same way.
                    Some('1'..='9' | 'w' | 'W' | 's' | 'S') => {
                        out.push(b'\\');
                        e.push_to(&mut out);
                        prev_atom = true;
                    }
                    // The zero-width GNU operators: anchors, so what follows
                    // one begins a new expression.
                    Some('<' | '>' | 'b' | 'B' | '`' | '\'') => {
                        out.push(b'\\');
                        e.push_to(&mut out);
                        prev_atom = false;
                    }
                    // Everything else escaped is itself -- `\{`, `\+` and `\n`
                    // included, the last being an `n`.
                    _ => {
                        literal(e, &mut out);
                        prev_atom = true;
                    }
                }
            }
            Some('^') => {
                if caret_here {
                    out.push(b'^');
                    prev_atom = false;
                } else {
                    out.extend_from_slice(br"\^");
                    prev_atom = true;
                }
                i = i.saturating_add(1);
            }
            Some('$') => {
                if ends_here(&cs, i) {
                    out.push(b'$');
                    prev_atom = false;
                } else {
                    out.extend_from_slice(br"\$");
                    prev_atom = true;
                }
                i = i.saturating_add(1);
            }
            Some(q @ ('*' | '+' | '?')) => {
                if !prev_atom {
                    out.push(b'\\');
                    prev_atom = true;
                }
                out.push(q as u8);
                i = i.saturating_add(1);
            }
            Some('.') => {
                // `RE_DOT_NEWLINE` is not in this syntax.
                out.extend_from_slice(b"[^\n]");
                prev_atom = true;
                i = i.saturating_add(1);
            }
            Some('[') => {
                i = bracket(&cs, i, &mut out)?;
                prev_atom = true;
            }
            // Ordinary here, operators in ERE.
            Some(m @ ('(' | ')' | '{' | '}' | '|')) => {
                out.push(b'\\');
                out.push(m as u8);
                prev_atom = true;
                i = i.saturating_add(1);
            }
            _ => {
                c.push_to(&mut out);
                prev_atom = true;
                i = i.saturating_add(1);
            }
        }
    }

    if depth != 0 {
        return Err(EreError::new(
            RegCode::UnmatchedParen,
            br"unmatched \(".to_vec(),
        ));
    }
    Ok(out)
}

/// Emit `c` as a literal for the ERE parser: escaped if ERE would read it as an
/// operator, bare otherwise. Bare matters for letters, which the ERE parser
/// would read as `\n`, `\t` and so on if they were escaped.
fn literal(c: Ch, out: &mut Str) {
    match c.as_ascii() {
        Some(
            m
            @ ('\\' | '.' | '[' | ']' | '(' | ')' | '*' | '+' | '?' | '{' | '}' | '|' | '^' | '$'),
        ) => {
            out.push(b'\\');
            out.push(m as u8);
        }
        _ => c.push_to(out),
    }
}

/// Emit `c` as a member of an ERE bracket: the four characters that parser
/// treats specially there are escaped, as its bracket reader allows.
fn member(c: Ch, out: &mut Str) {
    match c.as_ascii() {
        Some(m @ ('[' | ']' | '\\' | '-' | '^')) => {
            out.push(b'\\');
            out.push(m as u8);
        }
        _ => c.push_to(out),
    }
}

/// Read one bracket element at `j`: a `[.x.]`, a `[=x=]`, or a character.
/// Returns it and the index after it.
fn element(cs: &[Ch], j: usize) -> Result<(Ch, usize), EreError> {
    let Some(&c) = cs.get(j) else {
        return Err(unmatched_bracket());
    };
    let open = cs.get(j.saturating_add(1)).and_then(|c| c.as_ascii());
    if c.as_ascii() == Some('[')
        && let Some(delim @ ('.' | '=')) = open
    {
        let body = j.saturating_add(2);
        let mut k = body;
        loop {
            let Some(&d) = cs.get(k) else {
                return Err(unmatched_bracket());
            };
            if d.as_ascii() == Some(delim)
                && cs.get(k.saturating_add(1)).and_then(|c| c.as_ascii()) == Some(']')
            {
                break;
            }
            k = k.saturating_add(1);
        }
        return match cs.get(body..k) {
            Some([one]) => Ok((*one, k.saturating_add(2))),
            _ => Err(EreError::new(
                RegCode::BadCollation,
                b"invalid collating element".to_vec(),
            )),
        };
    }
    Ok((c, j.saturating_add(1)))
}

fn unmatched_bracket() -> EreError {
    EreError::new(RegCode::UnmatchedBracket, b"unmatched [ in regex".to_vec())
}

/// Translate the bracket expression whose `[` is at `i`. Returns the index just
/// past its `]`.
fn bracket(cs: &[Ch], i: usize, out: &mut Str) -> Result<usize, EreError> {
    let mut j = i.saturating_add(1);
    let negated = cs.get(j).and_then(|c| c.as_ascii()) == Some('^');
    if negated {
        j = j.saturating_add(1);
    }
    // A `[` or `[^` with nothing after it reaches glibc's "premature end" path
    // before the one that knows a bracket was open, as in `bre`.
    if cs.get(j).is_none() {
        return Err(EreError::new(
            RegCode::BadPattern,
            b"unmatched [ in regex".to_vec(),
        ));
    }
    let mut members: Vec<(Ch, Ch)> = Vec::new();
    let mut first = true;
    loop {
        let Some(&c) = cs.get(j) else {
            return Err(unmatched_bracket());
        };
        if c.as_ascii() == Some(']') && !first {
            j = j.saturating_add(1);
            break;
        }
        // A `-` is a member only first or last; anywhere else it would have to
        // be a range operator with no start.
        let next = cs.get(j.saturating_add(1)).and_then(|c| c.as_ascii());
        if c.as_ascii() == Some('-') && !first && next != Some(']') {
            return Err(EreError::new(
                RegCode::BadRangeEnd,
                b"invalid range".to_vec(),
            ));
        }
        first = false;
        let (lo, after) = element(cs, j)?;
        j = after;
        let dash = cs.get(j).and_then(|c| c.as_ascii()) == Some('-');
        let then = cs.get(j.saturating_add(1)).and_then(|c| c.as_ascii());
        if dash && then.is_some() && then != Some(']') {
            let (hi, after) = element(cs, j.saturating_add(1))?;
            j = after;
            if lo > hi {
                return Err(EreError::new(
                    RegCode::BadRangeEnd,
                    b"invalid range".to_vec(),
                ));
            }
            members.push((lo, hi));
        } else {
            members.push((lo, lo));
        }
    }
    out.push(b'[');
    if negated {
        out.push(b'^');
    }
    for (lo, hi) in members {
        member(lo, out);
        if lo != hi {
            out.push(b'-');
            member(hi, out);
        }
    }
    out.push(b']');
    Ok(j)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::{compile, to_ere};
    use crate::engine::RegCode;

    fn ere(p: &str) -> alloc::string::String {
        alloc::string::String::from_utf8(to_ere(p.as_bytes()).unwrap()).unwrap()
    }

    fn code(p: &str) -> RegCode {
        compile(p.as_bytes(), false).unwrap_err().code
    }

    fn find(p: &str, text: &str) -> Option<(usize, usize)> {
        compile(p.as_bytes(), false)
            .unwrap()
            .find(text.as_bytes())
            .unwrap()
    }

    #[test]
    fn grouping_is_backslashed_and_repetition_is_not() {
        assert_eq!(ere(r"\(a\|b\)+"), "(a|b)+");
        assert_eq!(ere("(a|b)"), r"\(a\|b\)");
        assert_eq!(ere(r"a\+b\?"), r"a\+b\?");
        assert_eq!(ere("a{2}"), r"a\{2\}");
        assert_eq!(ere(r"a\{2\}"), r"a\{2\}");
        assert_eq!(find(r"\(ab\)+", "xababy"), Some((1, 5)));
        assert_eq!(find("a{2}", "aa a{2}"), Some((3, 7)));
    }

    #[test]
    fn an_operator_with_nothing_to_repeat_is_a_literal() {
        assert_eq!(ere("*a"), r"\*a");
        assert_eq!(ere(r"\(*a\)"), r"(\*a)");
        assert_eq!(ere(r"a\|+b"), r"a|\+b");
        assert_eq!(ere("^*"), r"^\*");
        assert_eq!(ere(r"\<?"), r"\<\?");
        assert_eq!(ere("a**"), "a**");
        assert_eq!(find("^*x", "*x"), Some((0, 2)));
    }

    #[test]
    fn anchors_only_where_glibc_reads_them() {
        assert_eq!(ere("^a^b"), r"^a\^b");
        assert_eq!(ere(r"a\|^b"), "a|^b");
        assert_eq!(ere("a$b$"), r"a\$b$");
        assert_eq!(ere(r"\(a$\)"), "(a$)");
        assert_eq!(find("a$b", "a$b"), Some((0, 3)));
    }

    #[test]
    fn dot_does_not_match_a_newline() {
        assert_eq!(find("a.b", "a\nb axb"), Some((4, 7)));
    }

    #[test]
    fn an_escaped_letter_is_the_letter() {
        assert_eq!(ere(r"\n\t"), "nt");
        assert_eq!(find(r"\n", "a\nn"), Some((2, 3)));
    }

    #[test]
    fn brackets_have_no_named_classes() {
        // `[[:alpha:]]` is the members `[ : a l p h`, then a literal `]`.
        assert_eq!(find("[[:alpha:]]", "x:]"), Some((1, 3)));
        assert_eq!(find("[[:alpha:]]", "b]"), None);
        // A backslash is a member, and `]` first is one.
        assert_eq!(find(r"[\]", r"a\b"), Some((1, 2)));
        assert_eq!(find("[]a]", "x]"), Some((1, 2)));
        assert_eq!(find("[^]a]", "]ab"), Some((2, 3)));
        // Collating elements name one character.
        assert_eq!(find("[[.-.]a]", "x-"), Some((1, 2)));
        assert_eq!(find("[[=e=]]", "xe"), Some((1, 2)));
        assert_eq!(code("[[.ab.]]"), RegCode::BadCollation);
    }

    #[test]
    fn dashes_in_brackets() {
        assert_eq!(find("[-a]", "x-"), Some((1, 2)));
        assert_eq!(find("[a-]", "x-"), Some((1, 2)));
        assert_eq!(find("[--/]", "x."), Some((1, 2)));
        assert_eq!(code("[a-c-e]"), RegCode::BadRangeEnd);
        assert_eq!(code("[z-a]"), RegCode::BadRangeEnd);
    }

    #[test]
    fn errors_carry_glibcs_codes() {
        assert_eq!(code("a\\"), RegCode::TrailingBackslash);
        assert_eq!(code(r"\(a"), RegCode::UnmatchedParen);
        assert_eq!(code(r"a\)"), RegCode::UnmatchedRightParen);
        assert_eq!(code("[a"), RegCode::UnmatchedBracket);
        assert_eq!(code("["), RegCode::BadPattern);
        assert_eq!(code(r"\(a\)\2"), RegCode::BadBackReference);
    }

    #[test]
    fn ptx_s_sentence_pattern() {
        // The end of a sentence: punctuation, closing marks, then the end of a
        // line, a tab or two spaces, then any white space.
        let re = compile("[.?!][]\"')}]*\\($\\|\t\\|  \\)[ \t\n]*".as_bytes(), false)
            .unwrap()
            .with_newline_anchor(true);
        // "One." is followed by one space, which ends nothing; "Two." by two.
        assert_eq!(re.find(b"One. Two.  Three.").unwrap(), Some((8, 11)));
        assert_eq!(re.find(b"Stop!\nNext").unwrap(), Some((4, 6)));
        assert_eq!(re.find(b"(\"Quoted.\")  x").unwrap(), Some((8, 13)));
        assert_eq!(re.find(b"No end").unwrap(), None);
    }
}

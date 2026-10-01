//! POSIX **Basic** Regular Expressions, by translation to Extended ones.
//!
//! BRE is what `grep`, `sed` and `expr` match when no `-E` is given, and it is
//! not a subset of ERE — it is a different spelling of an overlapping language:
//!
//! | written | BRE means | ERE means |
//! |---|---|---|
//! | `a+b` | three literal characters | one-or-more `a`, then `b` |
//! | `\(x\)` | a group | a literal `(x)` |
//! | `(x)` | three literal characters | a group |
//! | `a\{2\}` | two `a`s | a literal `a{2}` |
//! | `*ab` | a literal `*`, then `ab` | invalid — nothing to repeat |
//! | `a^b` | a literal `^` | an anchor that can never match |
//!
//! So a program cannot serve both dialects by having one parser be lenient.
//! Translating is the cheaper half of the choice: the rules above are entirely
//! syntactic, they are testable in isolation (a translation is a string, and a
//! test can name the string it expects), and the *matching* — the part with the
//! ReDoS bound and the leftmost-greedy rule and the byte-safe character model —
//! stays in one engine with one set of behaviours.
//!
//! ## Where a translator has to be careful
//!
//! * **`*` is a literal where nothing precedes it** — at the start of the
//!   pattern, right after `\(`, right after `\|`, right after an anchoring
//!   `^`, and right after a zero-width assertion (`\<`, `\b`, `` \` ``, …),
//!   whatever came before that. `grep '*'` searches for an asterisk; it is not
//!   an error. `\+` and `\?` are literals in the same places, and `\{` is an
//!   error there, as in glibc.
//! * **`^` anchors only at the start, `$` only at the end** (or against the
//!   inside of a `\(…\)` / either side of a `\|`). `a^b` and `a$b` match those
//!   characters literally, and famously match nothing else.
//! * **Backslash is *not* special inside `[...]`.** `[\]` is a bracket holding
//!   a backslash, and the ERE engine this hands off to reads it the same way,
//!   so a bracket expression is copied as written. (Until 2026-10-01 the engine
//!   read escapes there and the copy doubled every backslash to cancel them;
//!   the engine was wrong for ERE too, which is why it is the engine that
//!   changed.)
//! * **Backreferences (`\1`–`\9`) pass through unchanged.** [`crate::engine`]
//!   reads them the same way, so the translation is the identity and the two
//!   dialects cannot come to differ about what one means. A pattern holding one
//!   is matched by the engine's backtracker rather than its Pike VM; everything
//!   else keeps the linear guarantee. They were refused outright until
//!   2026-08-18, when the backtracker was added.
//!
//! ## GNU spellings that are accepted
//!
//! `\|`, `\+` and `\?` are GNU extensions to BRE, not POSIX; `\w`, `\W`, `\s`
//! and `\S` likewise. They are accepted because every `sed` script and `grep`
//! pattern written in the last thirty years uses them, and because refusing
//! them would leave no way at all to write alternation in a BRE. So are the
//! zero-width GNU operators -- the word assertions `\<`, `\>`, `\b` and `\B`,
//! and the buffer anchors `` \` `` and `\'` -- which pass through to the ERE
//! parser unchanged. (The word assertions were refused until the engine could
//! match word boundaries; a spelling that quietly did the wrong thing would
//! have been worse than an error.)

use alloc::vec::Vec;

use crate::ch::{BStr, Ch, Str, chars};
use crate::engine::{EreError, RegCode, Regex};

/// Compile a BRE, with `ci` selecting case-insensitive matching.
///
/// # Errors
/// Returns the translation's error, or the ERE engine's, whichever stops first.
pub fn compile(pattern: BStr<'_>, ci: bool) -> Result<Regex, EreError> {
    let ere = to_ere(pattern)?;
    Regex::new_flags(&ere, ci)
}

/// Translate a POSIX BRE into the equivalent ERE.
///
/// The result is a pattern for [`crate::engine`], not something to show a user:
/// it is the same language, respelled.
///
/// # Errors
/// Returns [`EreError`] for a trailing backslash, an unmatched `\(`, `\{` or
/// `[`, or a quantifier with nothing to repeat.
#[allow(clippy::too_many_lines)] // One flat dispatch over BRE's characters; splitting it would hide the table.
pub fn to_ere(pattern: BStr<'_>) -> Result<Str, EreError> {
    let cs: Vec<Ch> = chars(pattern).collect();
    let mut out = Str::new();
    let mut i = 0usize;
    // Whether a quantifier written here would have something to apply to. When
    // it is false, `*`, `\+` and `\?` are the literal characters and `\{` is an
    // error ("Invalid preceding regular expression"), which is how glibc's
    // POSIX_BASIC reads a repetition at the start of an expression. False at
    // the start, after `\(` and `\|`, and after any anchor or assertion: a
    // zero-width thing is not an atom.
    let mut prev_atom = false;
    // Whether a `^` here is an anchor: at the start of the pattern, after `\(`
    // and after `\|`, and nowhere else. This used to be `!prev_atom`, which was
    // the same thing until assertions cleared `prev_atom` -- after one, glibc
    // reads `*` as a literal (`\<*`) *and* `^` as a literal (`\<^`), so the two
    // rules need two flags. Measured on GNU grep 3.11, sed 4.9 and ed 1.20.
    let mut at_start = true;
    // How deep in `\(` we are, so an unmatched one is reported rather than
    // handed to the engine as a stray `(`.
    let mut depth = 0usize;

    while let Some(&c) = cs.get(i) {
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
                        at_start = true;
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
                        at_start = false;
                    }
                    Some('{') => {
                        if !prev_atom {
                            return Err(EreError::new(
                                RegCode::BadRepeat,
                                br"nothing to repeat before \{".to_vec(),
                            ));
                        }
                        i = copy_interval(&cs, i, &mut out)?;
                    }
                    Some('}') => {
                        return Err(EreError::new(
                            RegCode::UnmatchedBrace,
                            br"unmatched \}".to_vec(),
                        ));
                    }
                    Some('|') => {
                        out.push(b'|');
                        prev_atom = false;
                        at_start = true;
                    }
                    // With nothing to repeat, `\+` and `\?` are the characters
                    // `+` and `?`: glibc's basic syntax has no
                    // `RE_CONTEXT_INVALID_OPS`, so a repetition at the start of
                    // an expression falls through to "a normal character".
                    // Measured: GNU grep, sed and ed all match `+a` with `\+a`,
                    // and `a+` with `a\b\+`. (It was refused here, as "nothing
                    // to repeat", which none of them does.) `\{` is different
                    // -- `RE_CONTEXT_INVALID_DUP` is in that syntax -- which is
                    // why the interval arm above still refuses.
                    Some(q @ ('+' | '?')) => {
                        if prev_atom {
                            out.push(q as u8);
                        } else {
                            out.push(b'\\');
                            out.push(q as u8);
                            prev_atom = true;
                        }
                        at_start = false;
                    }
                    // The GNU operators pass straight through: the ERE parser
                    // reads them itself, so the translation is the identity and
                    // the two dialects cannot disagree about what one means.
                    // They used to be rewritten here into the bracket
                    // expressions they abbreviate, which worked but left `\w`
                    // meaning a literal `w` in ERE — `grep -E '\w'` searched
                    // for the letter.
                    Some('w' | 'W' | 's' | 'S') => {
                        out.push(b'\\');
                        e.push_to(&mut out);
                        prev_atom = true;
                        at_start = false;
                    }
                    // The four word assertions and the two buffer anchors are
                    // zero-width, so after one there is nothing to repeat --
                    // even with an atom before it. glibc returns from an anchor
                    // before looking for a repetition, so the `*` that follows
                    // starts a fresh expression and is a literal: `a\>*`
                    // matches `a*`, not `ab` (measured, GNU grep, sed and ed;
                    // it used to repeat the assertion, because this arm left
                    // `prev_atom` as the `a` had set it). A `^` after one is a
                    // literal as well, which is `at_start`'s business.
                    Some('<' | '>' | 'b' | 'B' | '`' | '\'') => {
                        out.push(b'\\');
                        e.push_to(&mut out);
                        prev_atom = false;
                        at_start = false;
                    }
                    // A backreference passes straight through: the ERE parser
                    // reads `\1`-`\9` the same way, so the translation is the
                    // identity and the two dialects cannot disagree about what
                    // one means. It quantifies like an atom -- `\(a\)\1*` is
                    // legal -- which is what `prev_atom` records here.
                    Some('1'..='9') => {
                        out.push(b'\\');
                        e.push_to(&mut out);
                        prev_atom = true;
                        at_start = false;
                    }
                    // Every other escape denotes a literal, and stays escaped so
                    // that the engine reads it as one too.
                    _ => {
                        out.push(b'\\');
                        e.push_to(&mut out);
                        prev_atom = true;
                        at_start = false;
                    }
                }
            }
            Some('^') => {
                if at_start {
                    out.push(b'^');
                    // An anchor is not an atom: `^*` is a literal asterisk.
                    prev_atom = false;
                } else {
                    out.extend_from_slice(br"\^");
                    prev_atom = true;
                }
                at_start = false;
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
                at_start = false;
                i = i.saturating_add(1);
            }
            Some('*') => {
                if prev_atom {
                    out.push(b'*');
                } else {
                    out.extend_from_slice(br"\*");
                    prev_atom = true;
                }
                at_start = false;
                i = i.saturating_add(1);
            }
            Some('.') => {
                out.push(b'.');
                prev_atom = true;
                at_start = false;
                i = i.saturating_add(1);
            }
            Some('[') => {
                i = copy_bracket(&cs, i, &mut out)?;
                prev_atom = true;
                at_start = false;
            }
            // Plain characters in BRE that are metacharacters in ERE. They have
            // to be escaped on the way out or the engine would read a group, a
            // quantifier or an alternation that the pattern never wrote.
            Some(m @ ('(' | ')' | '{' | '}' | '+' | '?' | '|')) => {
                out.push(b'\\');
                out.push(m as u8);
                prev_atom = true;
                at_start = false;
                i = i.saturating_add(1);
            }
            _ => {
                c.push_to(&mut out);
                prev_atom = true;
                at_start = false;
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

/// Whether the `$` at `i` is in the position that makes it an anchor: the end
/// of the pattern, or immediately before `\)` or `\|`.
pub(crate) fn ends_here(cs: &[Ch], i: usize) -> bool {
    let next = i.saturating_add(1);
    match (cs.get(next), cs.get(next.saturating_add(1))) {
        (None, _) => true,
        (Some(a), Some(b)) => a.as_ascii() == Some('\\') && matches!(b.as_ascii(), Some(')' | '|')),
        _ => false,
    }
}

/// Copy a `\{m,n\}` interval starting at the `\{` whose backslash is at
/// `i - 2`, emitting the ERE `{m,n}`. Returns the index just past the `\}`.
///
/// `cs[i]` is the character after `\{`. The contents are copied rather than
/// parsed into numbers: the engine validates the interval (and enforces its
/// repetition cap), and doing it twice would give two places for the rules to
/// disagree.
fn copy_interval(cs: &[Ch], i: usize, out: &mut Str) -> Result<usize, EreError> {
    let mut j = i;
    let mut body = Str::new();
    loop {
        let Some(&c) = cs.get(j) else {
            return Err(EreError::new(
                RegCode::UnmatchedBrace,
                br"unmatched \{".to_vec(),
            ));
        };
        if c.as_ascii() == Some('\\')
            && cs.get(j.saturating_add(1)).and_then(|n| n.as_ascii()) == Some('}')
        {
            out.push(b'{');
            out.extend_from_slice(&body);
            out.push(b'}');
            return Ok(j.saturating_add(2));
        }
        c.push_to(&mut body);
        j = j.saturating_add(1);
    }
}

/// Copy a bracket expression starting at the `[` at `i`, verbatim. Returns the
/// index just past the closing `]`.
///
/// It is a scan rather than a search for the next `]` because finding that end
/// is not trivial: `]` as the first member (`[]abc]`, `[^]abc]`) is a literal,
/// and `[:alpha:]`, `[.coll.]` and `[=equiv=]` may contain a `]` of their own.
/// A backslash is an ordinary member here, as POSIX says — `[\]]` is a bracket
/// holding `\` followed by a literal `]` — and [`crate::engine`] reads it the
/// same way, so it is copied like any other character.
fn copy_bracket(cs: &[Ch], i: usize, out: &mut Str) -> Result<usize, EreError> {
    let unmatched = || EreError::new(RegCode::UnmatchedBracket, b"unmatched [ in regex".to_vec());
    let mut j = i.saturating_add(1);
    out.push(b'[');
    if cs.get(j).and_then(|c| c.as_ascii()) == Some('^') {
        out.push(b'^');
        j = j.saturating_add(1);
    }
    // Whether the bracket got as far as holding anything. It decides only which
    // *code* an unterminated bracket reports, and glibc draws the line here
    // rather than anywhere sensible: `[` and `[^` are `REG_BADPAT`, while `[]`,
    // `[^]` and `[a` are `REG_EBRACK`. A `[` at the very end of the pattern
    // reaches glibc's "premature end" path before it reaches the one that knows
    // a bracket was open. Measured against findutils 4.9.0 on glibc 2.39.
    let mut saw_member = false;
    if cs.get(j).and_then(|c| c.as_ascii()) == Some(']') {
        out.push(b']');
        j = j.saturating_add(1);
        saw_member = true;
    }
    loop {
        let Some(&c) = cs.get(j) else {
            return Err(if saw_member {
                unmatched()
            } else {
                EreError::new(RegCode::BadPattern, b"unmatched [ in regex".to_vec())
            });
        };
        saw_member = true;
        match c.as_ascii() {
            Some(']') => {
                out.push(b']');
                return Ok(j.saturating_add(1));
            }
            Some('[') => {
                // `[:class:]` and friends: only when the delimiter is one of
                // the three POSIX ones, else `[` is just a member.
                let delim = cs.get(j.saturating_add(1)).and_then(|c| c.as_ascii());
                if let Some(d @ (':' | '.' | '=')) = delim {
                    j = copy_class(cs, j, d, out).ok_or_else(unmatched)?;
                } else {
                    out.push(b'[');
                    j = j.saturating_add(1);
                }
            }
            _ => {
                c.push_to(out);
                j = j.saturating_add(1);
            }
        }
    }
}

/// Copy a `[:class:]` / `[.coll.]` / `[=equiv=]` starting at the `[` at `j`.
/// Returns the index just past its `]`, or `None` if it never closes.
fn copy_class(cs: &[Ch], j: usize, delim: char, out: &mut Str) -> Option<usize> {
    let mut k = j.saturating_add(2);
    let mut body = Str::new();
    loop {
        let &c = cs.get(k)?;
        if c.as_ascii() == Some(delim)
            && cs.get(k.saturating_add(1)).and_then(|n| n.as_ascii()) == Some(']')
        {
            out.push(b'[');
            out.push(delim as u8);
            out.extend_from_slice(&body);
            out.push(delim as u8);
            out.push(b']');
            return Some(k.saturating_add(2));
        }
        c.push_to(&mut body);
        k = k.saturating_add(1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn t(bre: &str) -> String {
        String::from_utf8(to_ere(bre.as_bytes()).unwrap()).unwrap()
    }

    fn err(bre: &str) -> String {
        String::from_utf8(to_ere(bre.as_bytes()).unwrap_err().detail).unwrap()
    }

    fn m(bre: &str, subject: &str) -> bool {
        compile(bre.as_bytes(), false)
            .unwrap()
            .is_match(subject.as_bytes())
            .unwrap()
    }

    #[test]
    fn ere_metacharacters_are_literal_in_a_bre() {
        assert_eq!(t("a+b"), r"a\+b");
        assert_eq!(t("a?b"), r"a\?b");
        assert_eq!(t("a|b"), r"a\|b");
        assert_eq!(t("(x)"), r"\(x\)");
        assert_eq!(t("a{2}"), r"a\{2\}");
        assert!(m("a+b", "a+b"));
        assert!(!m("a+b", "aab"));
    }

    #[test]
    fn backslashed_metacharacters_are_the_operators() {
        assert_eq!(t(r"\(ab\)\{2\}"), "(ab){2}");
        assert_eq!(t(r"a\|b"), "a|b");
        assert_eq!(t(r"a\+"), "a+");
        assert_eq!(t(r"a\?"), "a?");
        assert!(m(r"\(ab\)\{2\}", "xababy"));
        assert!(!m(r"\(ab\)\{2\}", "xabay"));
        assert!(m(r"^ab\|^cd", "cdx"));
    }

    #[test]
    fn a_star_with_nothing_before_it_is_an_asterisk() {
        // Every position where BRE says there is nothing to repeat.
        assert_eq!(t("*ab"), r"\*ab");
        assert_eq!(t("^*ab"), r"^\*ab");
        assert_eq!(t(r"\(*a\)"), r"(\*a)");
        assert_eq!(t(r"a\|*b"), r"a|\*b");
        assert!(m("*", "a*b"));
        assert!(!m("*", "ab"));
        // …and where there is one, it is the quantifier.
        assert_eq!(t("ab*c"), "ab*c");
        assert!(m("ab*c", "ac"));
    }

    #[test]
    fn anchors_anchor_only_at_the_ends() {
        assert_eq!(t("^ab$"), "^ab$");
        assert_eq!(t("a^b"), r"a\^b");
        assert_eq!(t("a$b"), r"a\$b");
        assert!(m("a^b", "xa^by"));
        assert!(m("a$b", "xa$by"));
        assert!(!m("^posix", "xposix"));
        assert!(m("^posix", "posix on"));
        // Against the inside of a group, and either side of an alternation,
        // they are anchors again.
        assert_eq!(t(r"\(^a$\)"), "(^a$)");
        assert_eq!(t(r"a$\|^b"), "a$|^b");
    }

    #[test]
    fn a_bracket_expression_is_copied_with_its_own_rules() {
        assert_eq!(t("[a-z]"), "[a-z]");
        assert_eq!(t("[^]a]"), "[^]a]");
        assert_eq!(t("[]a]"), "[]a]");
        assert_eq!(t("[[:digit:]]"), "[[:digit:]]");
        // `+` and `*` inside a bracket are members, not operators, so they must
        // not pick up the escaping the outside gets.
        assert_eq!(t("[+*]"), "[+*]");
        assert!(m("[ax]bc", "abc"));
        assert!(m("[ax]bc", "xbc"));
        assert!(!m("[ax]bc", "zbc"));
        assert!(m("^[[:space:]]*x", "   x"));
    }

    #[test]
    fn a_backslash_inside_a_bracket_is_a_member() {
        // POSIX: no escapes inside `[...]`, in either dialect -- so the
        // translation copies the bracket as written, and the engine reads it
        // the same way. `[\]` is a bracket holding a backslash and `[\]]` is
        // that followed by a literal `]`. Measured against grep 3.11.
        assert_eq!(t(r"[\]"), r"[\]");
        assert_eq!(t(r"[\.]"), r"[\.]");
        assert!(m(r"[\]", r"a\b"));
        assert!(!m(r"[\]", "ab"));
        assert!(m(r"^[\]]$", r"\]"));
        assert!(!m(r"^[\]]$", "]"));
        assert!(m(r"^[\.]$", r"\"));
        assert!(m(r"^[\t]$", "t"));
        assert!(!m(r"^[\t]$", "\t"));
    }

    #[test]
    fn an_escaped_letter_outside_a_bracket_is_the_letter() {
        // No C escapes in BRE either: GNU `grep 'a\tb'` matches `atb`.
        assert!(m(r"^a\tb$", "atb"));
        assert!(!m(r"^a\tb$", "a\tb"));
        assert!(m(r"^\n$", "n"));
    }

    #[test]
    fn the_gnu_operators_pass_through_to_the_ere_parser() {
        // The translation is the identity — the ERE parser reads these itself,
        // so the two dialects cannot disagree about what one means. Before
        // 2026-08-24 they were rewritten here into the bracket expressions they
        // abbreviate, which worked in BRE but left `\w` meaning a literal `w`
        // in ERE: `grep -E '\w'` searched for the letter.
        assert_eq!(t(r"\w\+"), r"\w+");
        assert_eq!(t(r"\s"), r"\s");
        assert_eq!(t(r"\W"), r"\W");
        assert_eq!(t(r"\S"), r"\S");
        assert!(m(r"^\w\w*$", "ab_1"));
        assert!(!m(r"^\w\w*$", "a b"));
        assert!(m(r"^\s$", " "));
        assert!(m(r"^\W$", "-"));
        assert!(m(r"^\S$", "x"));
    }

    #[test]
    fn the_word_assertions_pass_through_and_are_zero_width() {
        assert_eq!(t(r"\<word\>"), r"\<word\>");
        assert_eq!(t(r"\bx\B"), r"\bx\B");
        assert!(m(r"\<bar\>", "foo bar baz"));
        assert!(!m(r"\<ar\>", "foo bar baz"));
        assert!(m(r"\bfoo\b", "foo bar"));
        assert!(m(r"a\Bb", "abc"));
        assert!(!m(r"b\Bc", "ab c"));
        // Zero-width, so a following `*` has nothing to repeat and BRE reads it
        // as the literal it is there — same as after `^`. Measured: GNU
        // `grep '\>*'` matches `a*` and not `ab`, so the `*` is the character,
        // not a repeat of the assertion.
        assert_eq!(t(r"\>*"), r"\>\*");
        assert!(m(r"\>*", "a*"));
        assert!(!m(r"\>*", "ab"));
    }

    #[test]
    fn what_the_engine_cannot_express_is_refused_not_mistranslated() {
        assert!(err(r"a\").contains("trailing backslash"));
        assert!(err(r"\(a").contains(r"unmatched \("));
        assert!(err(r"a\)").contains(r"unmatched \)"));
        assert!(err(r"a\{2").contains(r"unmatched \{"));
        assert!(err("[a").contains("unmatched ["));
        assert!(err(r"\{2\}").contains("nothing to repeat"));
    }

    /// With nothing to repeat, `\+` and `\?` are literal characters, while
    /// `\{` is still an error -- glibc's basic syntax has
    /// `RE_CONTEXT_INVALID_DUP` and not `RE_CONTEXT_INVALID_OPS`. Measured, GNU
    /// grep 3.11, sed 4.9 and ed 1.20, which agree on every row.
    #[test]
    fn a_repetition_with_nothing_before_it() {
        assert_eq!(t(r"\+a"), r"\+a");
        assert!(m(r"^\+a$", "+a"));
        assert!(m(r"^\?a$", "?a"));
        assert!(m(r"^x\|\+a$", "+a"));
        assert!(m(r"^\(\?\)$", "?"));
        // After an assertion, whatever came before it.
        assert_eq!(t(r"a\>*"), r"a\>\*");
        assert!(m(r"a\>*", "a*"));
        assert!(!m(r"a\>*", "ab"));
        assert!(m(r"a\b\+", "a+"));
        assert!(!m(r"a\b\+", "a"));
        // `` a\`* `` and `a\'*` need a literal `*` where no character can
        // be, so they match nothing -- sed's and ed's answer. (GNU grep's dfa
        // repeats the anchor instead; see `scripts/grep-diff.sh`.)
        assert!(!m(r"a\`*", "a"));
        assert!(!m(r"a\'*", "a"));
        assert!(!m(r"a\'*", "a*"));
        for pat in [r"a\b\{0\}", r"a\>\{1\}", r"a\'\{1\}", r"\{1\}a"] {
            assert!(err(pat).contains("nothing to repeat"), "{pat}");
        }
        // A `^` after an assertion is a literal, not an anchor: as an anchor,
        // `\<^a` would match the line `a`.
        assert_eq!(t(r"\<^a"), r"\<\^a");
        assert!(!m(r"\<^a", "a"));
    }

    #[test]
    fn an_escaped_ordinary_character_stays_a_literal() {
        assert_eq!(t(r"a\.c"), r"a\.c");
        assert_eq!(t(r"a\\c"), r"a\\c");
        assert!(m(r"a\.c", "a.c"));
        assert!(!m(r"a\.c", "axc"));
        assert!(m(r"a\\c", r"a\c"));
    }

    #[test]
    fn a_pattern_that_is_not_text_survives_the_translation() {
        // The subject and the pattern are both byte strings; an undecodable
        // byte is one character and denotes itself.
        let pat = b"a\xffb*";
        let re = compile(pat, false).unwrap();
        assert!(re.is_match(b"xa\xffbbby").unwrap());
        assert!(re.is_match(b"a\xff").unwrap());
        assert!(!re.is_match(b"ab").unwrap());
    }

    #[test]
    fn a_backreference_survives_the_translation() {
        // The translation is the identity: the ERE parser reads `\1`-`\9` the
        // same way BRE does, so the two dialects cannot disagree about what
        // one means. Before 2026-08-18 this was a hard error.
        assert_eq!(t(r"\(a\)\1"), "(a)\\1");
        assert!(m(r"\(a\)\1", "aa"));
        assert!(!m(r"\(a\)\1", "ab"));
        // It quantifies like an atom.
        assert_eq!(t(r"\(a\)\1*"), "(a)\\1*");
        assert!(m(r"^\(a\)\1*$", "aaaa"));
        // The classic one-liner behind this feature: `sed '$!N;/^\(.*\)\n\1$/!P;D'`
        // drops adjacent duplicate lines. The `\n` there is sed's: GNU sed
        // turns it into a newline before the pattern reaches the compiler
        // (`normalize_text`, which `sed.rs` transcribes), so what arrives here
        // holds the newline itself. Handed `\n`, the compiler reads an `n`.
        assert!(m("^\\(.*\\)\n\\1$", "x\nx"));
        assert!(!m("^\\(.*\\)\n\\1$", "x\ny"));
        assert!(m(r"^\(.*\)\n\1$", "xnx"));
        // A reference to a group the pattern does not have is a compile error,
        // not a literal digit. The translator does not check it itself: the
        // ERE parser counts groups the same way, so checking here would be a
        // second copy of the rule to keep in step with the first.
        let e = compile(br"\(a\)\2", false).unwrap_err();
        assert!(
            String::from_utf8_lossy(&e.detail).contains("invalid backreference"),
            "{}",
            String::from_utf8_lossy(&e.detail)
        );
    }

    #[test]
    fn case_folding_reaches_the_translated_pattern() {
        assert!(
            compile(b"^[a-z]*$", true)
                .unwrap()
                .is_match(b"ABC")
                .unwrap()
        );
        assert!(
            compile(br"\(ab\)\{2\}", true)
                .unwrap()
                .is_match(b"ABAB")
                .unwrap()
        );
    }
}

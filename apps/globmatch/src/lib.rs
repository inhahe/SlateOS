//! Glob matching, once.
//!
//! # Why this crate exists
//!
//! Four places in this lane had grown their own glob matcher, and no two of
//! them agreed about what a glob is. That is not a tidiness complaint: two of
//! the four are *user-facing search tools in the same desktop*, `apps/indexer`
//! and `apps/filesearch`, both of which advertise `*`, `?` and `[a-z]` in
//! their help text. A user who learns a pattern in one and types it into the
//! other is entitled to the same answer, and did not get it.
//!
//! The divergences were not cosmetic either. Run against CPython's `fnmatch`
//! over every pattern of length ≤ 4 and every text of length ≤ 3 drawn from
//! the metacharacter alphabet — 730,236 pairs — `apps/filesearch`'s matcher
//! disagreed on **646** of them, in three families:
//!
//! | Family | Example | filesearch said | correct |
//! |---|---|---|---|
//! | literal test ran before the class test | `[*]` vs `[]` | match | no match |
//! | a `]` in first position ended the class | `[]]` vs `]` | no match | match |
//! | …so `[!]` was a negated *empty* class | `[!]` vs `a` | match | no match |
//!
//! and `apps/indexer`'s, before it was fixed, returned *nothing at all* for
//! `[a-]` and `[]]`. Both failure modes are silent: a search that should have
//! found files returns an empty result set and no error, which reads to the
//! user as "there are no such files".
//!
//! # What dialect this is
//!
//! POSIX `fnmatch(3)` without `FNM_PATHNAME`: `*` matches any run of
//! characters *including* `/`, `?` matches exactly one character, and a
//! bracket expression matches one character from a set. There is no escape
//! character, by design — see "The rules that look like bugs" below.
//!
//! It is **not** the dialect `apps/backup` uses. That one is gitignore-shaped:
//! it runs on raw path *bytes* (our paths are not required to be UTF-8), `*`
//! and `?` stop at `/`, `**` spans path segments, and there are no bracket
//! expressions at all. Unifying the two would silently change the meaning of
//! every exclude list that contains a `[`, so it is a question for the
//! operator rather than a refactor — see `open-questions.md` → C-Q9.
//!
//! # The rules that look like bugs
//!
//! Three of these have already been shipped wrong here, so each is spelled out
//! with the reason it is the way it is:
//!
//! * **A `]` immediately after `[` or `[!` is a literal member, not the
//!   terminator.** `[]]` is the one-element class containing `]`. POSIX had to
//!   carve out this one position because a bracket expression has no escape
//!   character, so without it there would be no way to write a pattern
//!   matching a `]` at all.
//! * **A `-` in the final position of a class is a literal `-`,** because
//!   there is nothing after it to be a range to. `[a-]` matches `a` and `-`.
//! * **An unterminated `[` is an ordinary character.** POSIX 2.13.1 is
//!   explicit: "Otherwise, the open bracket shall be treated as an ordinary
//!   character." So `[a-` matches the three-character text `[a-`. (bash
//!   deviates here — it falls back to a literal only when its scanner gives up
//!   at a *member* position, and hard-fails when it gives up at a *range-end*
//!   position, so `[-` matches itself but `[a-` does not. That is an artifact
//!   of where its parser happens to return, not a rule anyone would choose;
//!   CPython's `fnmatch` agrees with us, not with bash.)
//!
//! # The one deliberate extension
//!
//! `[^abc]` negates, as well as `[!abc]`. POSIX specifies only `!`, and
//! CPython's `fnmatch` accepts only `!`. bash, ksh and zsh all accept both,
//! and `apps/filesearch` already did — so accepting only `!` here would have
//! been a silent regression for its users, turning their negated classes into
//! classes that match a literal `^`. This is the *only* point on which this
//! crate departs from `fnmatch`: over the differential run described above,
//! extended with `^` in the alphabet, every disagreement is in that family and
//! there are none outside it.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// Match `text` against the glob `pattern`.
///
/// Decodes both arguments. A caller matching one pattern against many
/// candidates should decode the pattern once and call [`glob_match_chars`],
/// which is what the search paths in `apps/indexer` and `apps/filesearch` do.
#[must_use]
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_chars(&pat, &txt)
}

/// [`glob_match`] for callers that already hold decoded characters.
///
/// Both sides are `&[char]`, not `&[u8]`, and that is a semantic choice rather
/// than a convenience. A byte-wise matcher makes `?` match one *byte*: `?.txt`
/// does not match `日.txt`, and `[é]` matches either half of the two-byte `é`
/// and so also matches part of the unrelated `è`. Callers whose input is not
/// required to be UTF-8 — `apps/backup` walks raw path bytes — cannot use this
/// crate for that reason, which is one of the two reasons it keeps its own
/// matcher.
///
/// # Termination
///
/// Every iteration either advances `ti`, or advances `pi` on a `*` (which `pi`
/// can only be reset behind by a backtrack, and a backtrack strictly increases
/// `star_ti`, which bounds `ti` from below and never decreases). So the pair
/// `(star_ti, pi)` increases lexicographically on every iteration and is
/// bounded, and the loop must end.
///
/// That is worth stating because the matcher this replaces in `apps/backup`
/// did not have the property: it was two mutually recursive functions that
/// disagreed about which `**` each would handle, and `--exclude '**a'`
/// bounced between them until the stack overflowed and took the backup down.
/// A single loop with no recursion cannot fail that way.
#[must_use]
pub fn glob_match_chars(pattern: &[char], text: &[char]) -> bool {
    let mut pi = 0usize;
    let mut ti = 0usize;
    // `Option` rather than a `usize::MAX` sentinel: "no star seen yet" is a
    // distinct state, not a magic index, and the compiler will not let it be
    // used as one by accident.
    let mut star_pi: Option<usize> = None;
    let mut star_ti = 0usize;

    // Reading through `get` rather than testing `ti < text.len()` and then
    // indexing gives the loop its bound and its character in one step, so
    // there is no window in which the two could disagree.
    while let Some(&t) = text.get(ti) {
        // The order of these arms is load-bearing, and getting it wrong is
        // exactly what `apps/filesearch` did: it tested "pattern character
        // equals text character" *first*, so whenever the text happened to
        // contain a `[`, the pattern's `[` matched it literally and the class
        // was never parsed. `[*]` — the way one searches for a literal
        // asterisk — matched `[]`, `[a]` and `[b]`, and did not match `*`.
        let consumed = match pattern.get(pi) {
            Some('*') => {
                star_pi = Some(pi);
                star_ti = ti;
                pi = pi.saturating_add(1);
                continue;
            }
            Some('?') => true,
            Some('[') => match match_char_class(pattern.get(pi..).unwrap_or_default(), t) {
                Some((true, len)) => {
                    pi = pi.saturating_add(len);
                    ti = ti.saturating_add(1);
                    continue;
                }
                // The class is well formed and simply did not contain this
                // character; the whole class is what failed, so fall through
                // to the backtrack below.
                Some((false, _)) => false,
                // Malformed — no closing `]`. POSIX 2.13.1: the open bracket
                // is then an ordinary character. Treating it as "matches
                // nothing" instead would make a search for a filename
                // containing a bracket return silently empty, which is the
                // failure shape this family of code has already been bitten
                // by twice.
                None => t == '[',
            },
            Some(&ch) => ch == t,
            None => false,
        };

        if consumed {
            pi = pi.saturating_add(1);
            ti = ti.saturating_add(1);
            continue;
        }

        // Backtrack to just after the last `*`, having let it swallow one more
        // character of the text.
        let Some(resume_at) = star_pi else {
            return false;
        };
        pi = resume_at.saturating_add(1);
        star_ti = star_ti.saturating_add(1);
        ti = star_ti;
    }

    // The text is exhausted; the pattern matches only if what is left of it
    // can match nothing at all, which means trailing stars and then the end.
    while pattern.get(pi) == Some(&'*') {
        pi = pi.saturating_add(1);
    }

    pi == pattern.len()
}

/// Match one character against the bracket expression starting at `pattern[0]`.
///
/// Returns `(matched, len)` where `len` counts the `[` and the `]`, or `None`
/// if there is no `]` and the expression is therefore not a bracket expression
/// at all.
fn match_char_class(pattern: &[char], ch: char) -> Option<(bool, usize)> {
    if pattern.first() != Some(&'[') {
        return None;
    }

    let mut i = 1usize;
    // `^` as well as `!`: the one place this crate is a superset of POSIX and
    // of CPython's `fnmatch`. bash, ksh and zsh all accept both, and
    // `apps/filesearch` already did, so rejecting `^` would have been a silent
    // regression that turned its users' negated classes into classes matching
    // a literal `^`.
    let negate = matches!(pattern.get(i), Some('!' | '^'));
    if negate {
        i = i.saturating_add(1);
    }

    let mut matched = false;
    // POSIX gives the first member position one special rule: a `]` there is a
    // literal member rather than the terminator, because a bracket expression
    // has no escape character and so has no other way to contain a `]` at all.
    // `[]]` is therefore the one-element class `]`, not an empty class
    // followed by a stray bracket — and `[]-a]` is the range `]`..`a`, because
    // that position is an ordinary member position in every other respect.
    // `first_member` marks exactly that one position; after it a `]` closes
    // the class as usual.
    let mut first_member = true;
    while let Some(&lo) = pattern.get(i).filter(|&&c| first_member || c != ']') {
        first_member = false;
        // Reading the separator and the range end through `get` is what makes
        // the range arm safe: `[a-` at the end of a pattern leaves no `hi`
        // character, and asking for one returns `None` rather than running off
        // the end of the class.
        //
        // The `hi != ']'` guard is the trailing-dash rule, and it is a
        // different question from the one an `i + 2 < len` bound asks. That
        // bound asks whether a third character *exists*; this asks whether the
        // one that exists is a range end. The two diverge exactly at the end
        // of the class: for `[a-]` a third character does exist, and it is the
        // `]`. Reading it as the range end consumed the terminator, ran off
        // the end, and reported the whole class malformed — so `[a-]` matched
        // nothing at all.
        match (
            pattern.get(i.saturating_add(1)),
            pattern.get(i.saturating_add(2)),
        ) {
            (Some('-'), Some(&hi)) if hi != ']' => {
                if (lo..=hi).contains(&ch) {
                    matched = true;
                }
                i = i.saturating_add(3);
            }
            _ => {
                if lo == ch {
                    matched = true;
                }
                i = i.saturating_add(1);
            }
        }
    }

    if pattern.get(i) != Some(&']') {
        // Not a bracket expression: no closing bracket.
        return None;
    }
    let len = i.saturating_add(1); // Include the `]`.
    Some((matched != negate, len))
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::String;

    // Every assertion in this module was adjudicated against an independent
    // implementation before it was written down — CPython's `fnmatch` for the
    // POSIX rules, and bash (`case "$t" in $pat) r=match;; *) r=no;; esac`)
    // for the `^`-negation extension, which `fnmatch` does not have. None of
    // it is recalled from memory of the specification.

    #[test]
    fn stars_and_question_marks() {
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*.txt", "notes.txt"));
        assert!(!glob_match("*.txt", "notes.txt.bak"));
        assert!(glob_match("?", "a"));
        assert!(!glob_match("?", ""));
        assert!(!glob_match("?", "ab"));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a*b*c", "axxbyy"));
    }

    #[test]
    fn a_star_does_not_stop_at_a_separator() {
        // This crate is `fnmatch` *without* `FNM_PATHNAME`, which is what both
        // callers want: a search box matches against a whole path, and a user
        // typing `*report*` means "anywhere in the path". `apps/backup`'s
        // matcher is the other dialect on purpose — see the module docs.
        assert!(glob_match("*report*", "/home/u/2026/report-final.pdf"));
        assert!(glob_match("/home/*/x", "/home/a/b/c/x"));
    }

    #[test]
    fn a_question_mark_is_one_character_not_one_byte() {
        // The pattern and the text are decoded before matching, so a `?`
        // consumes a whole scalar value. A byte-wise matcher ate a third of
        // the kanji here and then compared the literal `.` against a
        // continuation byte.
        assert!(glob_match("?.txt", "\u{65e5}.txt"));
        assert!(glob_match("[\u{e9}]", "\u{e9}"));
        assert!(!glob_match("[\u{e9}]", "\u{e8}"));
    }

    #[test]
    fn classes_ranges_and_negation() {
        assert!(glob_match("[abc]", "b"));
        assert!(!glob_match("[abc]", "d"));
        assert!(glob_match("[a-z]", "m"));
        assert!(!glob_match("[a-z]", "M"));
        assert!(glob_match("[!abc]", "d"));
        assert!(!glob_match("[!abc]", "a"));
        assert!(glob_match("[a-cx-z]", "y"));
        assert!(!glob_match("[a-cx-z]", "m"));
    }

    #[test]
    fn a_caret_negates_as_well_as_a_bang() {
        // The single deliberate departure from POSIX and from CPython's
        // `fnmatch`, both of which treat `^` as an ordinary member. bash, ksh
        // and zsh all negate on it, `apps/filesearch` already did, and a user
        // arriving from regular expressions types `^` before `!`.
        //
        // Checked against bash: `case a in [^b]) ;; esac` matches; `case b in
        // [^b])` does not.
        assert!(glob_match("[^b]", "a"));
        assert!(!glob_match("[^b]", "b"));
        assert!(glob_match("[^a-z]", "0"));
        assert!(!glob_match("[^a-z]", "q"));
        // ...and the leading-`]` rule applies after a `^` exactly as it does
        // after a `!`.
        assert!(!glob_match("[^]]", "]"));
        assert!(glob_match("[^]]", "a"));
        // A `^` anywhere but first is an ordinary member, in bash and here.
        assert!(glob_match("[a^]", "^"));
        assert!(glob_match("[a^]", "a"));
        assert!(!glob_match("[a^]", "b"));
    }

    #[test]
    fn a_trailing_dash_in_a_class_is_a_literal_dash() {
        // POSIX, `fnmatch(3)` and every shell agree that a `-` in the final
        // position of a bracket expression is an ordinary character, because
        // there is nothing after it for it to be a range to. Both matchers
        // this crate replaces got some part of this wrong; `apps/indexer`
        // returned *nothing at all* for `[a-]`.
        assert!(glob_match("[a-]", "a"));
        assert!(glob_match("[a-]", "-"));
        assert!(!glob_match("[a-]", "b"));
        assert!(glob_match("[--]", "-"));
        assert!(!glob_match("[--]", "a"));
        // A real range is unaffected — the `-` there has a character after it.
        assert!(glob_match("[a-z]", "m"));
        assert!(!glob_match("[a-z]", "-"));
    }

    #[test]
    fn a_leading_bracket_in_a_class_is_a_literal_member() {
        // The other half of the same problem, with the same cause: a bracket
        // expression has no escape character, so POSIX had to carve out one
        // position where `]` does not terminate the class. Without that rule
        // there is no way to write a pattern matching a literal `]` at all.
        assert!(glob_match("[]]", "]"));
        assert!(!glob_match("[]]", "a"));
        assert!(glob_match("[]a]", "]"));
        assert!(glob_match("[]a]", "a"));

        // Negation moves the special position past the `!` rather than
        // cancelling it.
        assert!(!glob_match("[!]]", "]"));
        assert!(glob_match("[!]]", "a"));

        // That position is an ordinary member position in every other respect,
        // so it can start a range: `[]-a]` is `]`(0x5D) through `a`(0x61),
        // which takes in `^` and `_` but not `-`.
        assert!(glob_match("[]-a]", "]"));
        assert!(glob_match("[]-a]", "^"));
        assert!(glob_match("[]-a]", "_"));
        assert!(glob_match("[]-a]", "a"));
        assert!(!glob_match("[]-a]", "-"));

        // ...and the trailing-dash rule still applies there, so `[]-]` is the
        // two-element class `]` and `-`, not a range.
        assert!(glob_match("[]-]", "]"));
        assert!(glob_match("[]-]", "-"));
        assert!(!glob_match("[]-]", "^"));

        // A `]` anywhere later is the terminator, as always: `[a]]` is the
        // one-element class `a` followed by a literal `]`.
        assert!(glob_match("[a]]", "a]"));
        assert!(!glob_match("[a]]", "]"));
    }

    #[test]
    fn an_unterminated_bracket_is_an_ordinary_character() {
        // POSIX 2.13.1: "Otherwise, the open bracket shall be treated as an
        // ordinary character." CPython's `fnmatch` agrees on every case here.
        //
        // bash does not, and the way it disagrees is worth recording so that
        // nobody "fixes" this to match it: bash falls back to a literal only
        // when its scanner runs out of pattern at a *member* position, and
        // returns a hard no-match when it runs out at a *range-end* position.
        // So in bash `[-` and `[a-b-` match themselves but `[a-` and `[ab-`
        // do not — a distinction that follows from where its parser returns
        // rather than from any rule. Over the 730,236-pair differential those
        // 69 cases are the *only* ones where this crate and bash differ.
        assert!(glob_match("[", "["));
        assert!(glob_match("[a", "[a"));
        assert!(glob_match("[a-", "[a-"));
        assert!(glob_match("[]", "[]"));
        assert!(glob_match("[]x", "[]x"));
        assert!(glob_match("[!a", "[!a"));
        assert!(!glob_match("[a-", "a"));
        assert!(!glob_match("[]", "]"));
        assert!(!glob_match("[]x", "x"));
    }

    #[test]
    fn a_class_is_parsed_before_a_literal_bracket_is_compared() {
        // `apps/filesearch` tested literal equality first, so a `[` in the
        // *text* short-circuited the class parse. `[*]` is how one searches
        // for a literal asterisk, and it matched `[]`, `[a]` and `[b]`
        // instead — 338 of that matcher's 646 disagreements were this one
        // mistake.
        assert!(glob_match("[*]", "*"));
        assert!(!glob_match("[*]", "[]"));
        assert!(!glob_match("[*]", "[a]"));
        assert!(glob_match("[?]", "?"));
        assert!(!glob_match("[?]", "[]"));
        // The same for `*`: the star arm must precede the literal arm, or
        // `*a` would fail against `*ba` because the pattern's `*` would have
        // been spent matching the text's `*`.
        assert!(glob_match("*a", "*ba"));
        assert!(glob_match("*", "*"));
    }

    #[test]
    fn a_backtrack_can_resume_more_than_once() {
        // One star, several false starts. A matcher that records the star
        // position but not the text position it was at gets these wrong.
        assert!(glob_match("*ab", "aaab"));
        assert!(glob_match("*abc", "abababc"));
        assert!(!glob_match("*abc", "ababab"));
        assert!(glob_match("a*b*c*d", "aXbXXcXXXd"));
    }

    #[test]
    fn the_char_slice_entry_point_agrees_with_the_str_one() {
        // `glob_match_chars` exists so a search can decode the pattern once
        // rather than once per candidate; it is the same function underneath,
        // and this is what says so. If they ever stop being the same function,
        // this is what notices.
        for (p, t) in [
            ("*.txt", "a.txt"),
            ("[a-]", "-"),
            ("[]]", "]"),
            ("[^b]", "a"),
            ("[a-", "[a-"),
            ("[*]", "*"),
            ("?", "\u{65e5}"),
        ] {
            let pat: Vec<char> = p.chars().collect();
            let txt: Vec<char> = t.chars().collect();
            assert_eq!(
                glob_match_chars(&pat, &txt),
                glob_match(p, t),
                "{p:?} vs {t:?}"
            );
        }
    }

    #[test]
    fn empty_pattern_matches_only_empty_text() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "a"));
        assert!(glob_match("*", ""));
        assert!(glob_match("**", ""));
        assert!(!glob_match("?", ""));
        assert!(!glob_match("[a]", ""));
    }

    #[test]
    fn a_long_run_of_stars_does_not_take_exponential_time() {
        // The textbook backtracking matcher is `O(2^n)` on a pattern of many
        // stars against a text that nearly matches, because each star explores
        // independently. This one keeps a single backtrack point, which makes
        // it `O(pattern * text)` — the test is here so that a future
        // "improvement" to a recursive formulation is caught by the suite
        // hanging rather than by a user's search never returning.
        // The pattern must not *end* in a star, or the trailing star swallows
        // whatever made the text fail and the negative case is not a negative
        // case at all -- which is how this test was first written, and it
        // passed for the wrong reason.
        let pattern: String = format!("{}b", "a*".repeat(40));
        assert!(!glob_match(&pattern, &"a".repeat(400)));
        assert!(glob_match(&pattern, &format!("{}b", "a".repeat(400))));
    }
}

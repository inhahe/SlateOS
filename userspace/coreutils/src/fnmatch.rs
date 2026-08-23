//! POSIX `fnmatch` over bytes — the shell-glob matcher `find -name`,
//! `du --exclude`, `tar --exclude` and `cpio`'s pattern list all need.
//!
//! # Why this is shared rather than written per utility
//!
//! Eight binaries in this tree had already written it: `find`, `du`'s
//! standalone twin, `cpio`, `tar`, `sftp`, `rsync`, `zip` and `updatedb`. They
//! agreed on `*` and `?` and on nothing else. The copy in `find.rs` is
//! representative, and its four defects are the four this module exists to fix
//! once:
//!
//! 1. **It matched `char`s, not bytes.** The pattern and the name were both
//!    collected into `Vec<char>`, which on this OS cannot be done at all — a
//!    file name is any byte sequence but `/` and NUL (`design.txt`), so the
//!    conversion either lost the name or refused it. A glob is a *byte*
//!    grammar: `?` matches one byte, as glibc's does with no multibyte locale.
//!
//! 2. **`*` backtracked exponentially.** `glob_match_inner` recursed once per
//!    possible split, so `*a*a*a*a*a*b` against forty `a`s does not finish.
//!    That is reachable from a command line — `find . -name '*x*x*x*x*x*y'` —
//!    and from a `--exclude` file nobody audits. The loop below is the standard
//!    single-restart-point algorithm and is O(len(pattern) × len(name)).
//!
//! 3. **`[…]` was a set of literal characters and nothing else.** No ranges
//!    (`[a-z]` matched only `a`, `-` and `z`), no negation (`[!o]` matched `!`
//!    and `o`), no classes (`[[:digit:]]` matched `[`, `:`, `d`, …), and a
//!    leading `]` ended the set instead of joining it. Every one of those is
//!    POSIX, and three of them silently match the *wrong* files rather than
//!    failing loudly.
//!
//! 4. **`\` was not an escape.** A pattern could not name a file with a `*` in
//!    it, and `find -name '\*'` looked for a two-character name.
//!
//! # The flags are glibc's, and two of them change what a `/` means
//!
//! [`Flags::PATHNAME`] and [`Flags::PERIOD`] are the ones with surprises, and
//! the surprise is that the callers in this tree want them **off**. `find
//! -name` matches the last component alone, so there is no `/` to be careful
//! about; `du --exclude` deliberately lets `*` cross a `/`, which is measured —
//! `du --exclude='*/keep'` prunes `ex/aa/keep`, and would not if `*` stopped at
//! the separator. Turning them on is what a shell's own globbing wants, where
//! `*` must not match `/` and `*` must not match a leading dot.
//!
//! # What is deliberately absent
//!
//! Collating symbols (`[[.a.]]`) and equivalence classes (`[[=a=]]`). Note the
//! doubled bracket: the delimiter is `[.` *inside* a bracket expression, so a
//! bare `[.a.]` is not one of these at all — it is the ordinary two-member set
//! `.`, `a`, and is handled. glibc parses the doubled form and, in the C
//! locale, reduces it to the single character between the delimiters; no caller
//! in this tree writes one. They are rejected as a malformed bracket rather
//! than read as a set of punctuation, so a pattern using one fails to match
//! instead of matching the wrong files.
//!
//! # How this is known to be right
//!
//! `tests/fnmatch_glibc.rs` replays about 105,000 cases — 137 patterns × 70
//! names × 11 flag sets — recorded from the real `fnmatch(3)` by
//! `scripts/fnmatch-probe.c`. Three of the rules below were measured there
//! after being implemented wrongly from first principles: a trailing `\`
//! matches *nothing* rather than a literal backslash, [`Flags::CASEFOLD`]
//! reaches a range but not a character class, and [`Flags::LEADING_DIR`] has to
//! be tested wherever the pattern runs out rather than once at the end, or
//! `a*b` fails to match `a/b/c`.

/// How to match. Combine with `|`.
///
/// The names are glibc's `FNM_*` without the prefix, so a reader holding the
/// `fnmatch(3)` page can check this file against it line by line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Flags(u32);

impl Flags {
    /// Plain shell globbing: `*` and `?` and `[…]` match any byte, `\` escapes.
    pub const NONE: Self = Self(0);

    /// `FNM_PATHNAME`: a wildcard never matches `/`.
    ///
    /// With it, `*` matches within one path component only, so `a*c` cannot
    /// match `a/b/c`. Without it — the setting `du --exclude` uses, measured —
    /// it can.
    pub const PATHNAME: Self = Self(1);

    /// `FNM_NOESCAPE`: `\` is an ordinary byte rather than an escape.
    ///
    /// Needed by any caller whose patterns came from somewhere that has already
    /// spent the backslash, which is why `tar`'s `--no-wildcards-match-slash`
    /// family exists upstream.
    pub const NOESCAPE: Self = Self(2);

    /// `FNM_PERIOD`: a leading `.` must be matched by a literal `.`.
    ///
    /// "Leading" means at the start of the name, and — only if
    /// [`PATHNAME`](Self::PATHNAME) is also set — after every `/`. That
    /// dependency is glibc's and is easy to get wrong in the direction that
    /// hides files: without `PATHNAME`, `*` matches `a/.b` even under `PERIOD`.
    pub const PERIOD: Self = Self(4);

    /// `FNM_LEADING_DIR`: the pattern may match a *prefix* of the name that
    /// ends at a `/`.
    ///
    /// So `a` matches `a/b/c`. This is how `tar --exclude=dir` prunes a whole
    /// subtree with one pattern; `du` gets the same effect a different way (it
    /// stops descending), which is why `du` does not set this.
    pub const LEADING_DIR: Self = Self(8);

    /// `FNM_CASEFOLD`: compare ASCII letters without regard to case.
    ///
    /// ASCII only, deliberately. Case is a property of a locale's character
    /// set, and this matcher has no character set — it matches bytes. Folding
    /// non-ASCII by byte would fold Latin-1 but corrupt UTF-8, which is worse
    /// than not folding at all.
    pub const CASEFOLD: Self = Self(16);

    /// The set with exactly the bits of `value`, which are glibc's `FNM_*`.
    ///
    /// Only for the fixture test, which stores the flag set as the number
    /// `fnmatch(3)` was called with. Nothing else should build a `Flags` from a
    /// number — the named constants say what they mean.
    #[must_use]
    #[doc(hidden)]
    pub const fn from_bits(value: u32) -> Self {
        Self(value)
    }

    /// Whether every bit of `other` is set here.
    #[must_use]
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Flags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Does `name` match `pattern`?
///
/// ```
/// use coreutils::fnmatch::{Flags, fnmatch};
///
/// assert!(fnmatch(b"*.o", b"main.o", Flags::NONE));
/// assert!(fnmatch(b"[[:digit:]]*", b"9lives", Flags::NONE));
/// assert!(!fnmatch(b"a*c", b"a/b/c", Flags::PATHNAME));
/// assert!(fnmatch(b"a*c", b"a/b/c", Flags::NONE));
/// ```
#[must_use]
pub fn fnmatch(pattern: &[u8], name: &[u8], flags: Flags) -> bool {
    // A leading dot is decided before the loop rather than inside it: the rule
    // is about a *position* in the name, and the only positions are the start
    // and (under PATHNAME) just after a separator, so checking it where the
    // loop would have to re-derive "am I at the start" costs nothing and reads
    // as the rule.
    if flags.has(Flags::PERIOD)
        && name.first() == Some(&b'.')
        && pattern.first() != Some(&b'.')
        && !(pattern.first() == Some(&b'\\')
            && !flags.has(Flags::NOESCAPE)
            && pattern.get(1) == Some(&b'.'))
    {
        return false;
    }

    // The single-restart-point wildcard loop. `star` remembers the last `*`
    // seen and how much of the name it had consumed at the time; on a mismatch
    // we return there and let it eat one more byte. That is enough to be
    // correct — a later `*` can always absorb what an earlier one gave up — and
    // it is what makes this linear in the product of the two lengths rather
    // than exponential in the number of stars.
    let (mut p, mut n) = (0_usize, 0_usize);
    let mut star: Option<(usize, usize)> = None;

    loop {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, n));
                p = p.saturating_add(1);
                continue;
            }
            Some(_) if n < name.len() => {
                if let Some(next_p) = match_one(pattern, p, name, n, flags) {
                    p = next_p;
                    n = n.saturating_add(1);
                    continue;
                }
            }
            // The name is spent but the pattern is not: a mismatch, though an
            // earlier star may simply have taken too much, so still backtrack.
            Some(_) => {}
            None => {
                if n == name.len() {
                    return true;
                }
                // `FNM_LEADING_DIR`: the pattern is spent and what is left of
                // the name is a subdirectory, so `a` matches `a/b/c` and `a*b`
                // matches `a/b/c`. Asked wherever the pattern runs out rather
                // than once at the end, because with a `*` in the pattern this
                // point is reached repeatedly, at a different `n` each time.
                if flags.has(Flags::LEADING_DIR) && name.get(n) == Some(&b'/') {
                    return true;
                }
            }
        }

        // Mismatch. Extend the last star by one byte, if there was one and if
        // the byte it would swallow is one it is allowed to swallow.
        let Some((star_p, star_n)) = star else {
            return false;
        };
        let Some(&eaten) = name.get(star_n) else {
            return false;
        };
        if flags.has(Flags::PATHNAME) && eaten == b'/' {
            // Under PATHNAME a `*` stops at a separator, so there is nothing
            // left to try. Checked here rather than when the star was recorded
            // because the star is allowed to match *up to* the separator.
            return false;
        }
        if flags.has(Flags::PERIOD) && flags.has(Flags::PATHNAME) && eaten == b'.' && star_n > 0 {
            // A `*` may not eat the dot that begins a component. `star_n > 0`
            // is safe to index behind for the same reason.
            if name.get(star_n.saturating_sub(1)) == Some(&b'/') {
                return false;
            }
        }
        p = star_p.saturating_add(1);
        n = star_n.saturating_add(1);
        star = Some((star_p, n));
    }
}

/// Match the single pattern item at `p` against `name[n]`, returning where the
/// pattern continues.
///
/// One item is one byte for a literal, two for an escape, and a whole bracket
/// expression for a `[`. `None` means the item did not match — *or* that the
/// bracket was malformed, in which case the `[` was a literal and did not match
/// either, which is the same answer.
fn match_one(pattern: &[u8], p: usize, name: &[u8], n: usize, flags: Flags) -> Option<usize> {
    let &c = name.get(n)?;
    let &pc = pattern.get(p)?;
    match pc {
        b'?' => {
            if flags.has(Flags::PATHNAME) && c == b'/' {
                return None;
            }
            if flags.has(Flags::PERIOD) && c == b'.' && at_component_start(name, n, flags) {
                return None;
            }
            Some(p.saturating_add(1))
        }
        b'[' => {
            if flags.has(Flags::PERIOD) && c == b'.' && at_component_start(name, n, flags) {
                return None;
            }
            match bracket(pattern, p, c, flags) {
                // A malformed bracket makes the `[` an ordinary byte, which is
                // glibc's rule and the reason `find -name '[' ` finds a file
                // called `[` rather than erroring.
                Bracket::Malformed => (c == b'[').then(|| p.saturating_add(1)),
                Bracket::Matched(end) => Some(end),
                Bracket::Rejected => None,
            }
        }
        b'\\' if !flags.has(Flags::NOESCAPE) => {
            // "Trailing \ loses" — glibc's own comment. A pattern ending in an
            // escape with nothing to escape matches *nothing*, so `\` does not
            // find a file named `\`; that needs `\\` or `FNM_NOESCAPE`.
            // Measured: `fnmatch("\\", "\\", 0)` is `FNM_NOMATCH`.
            let &escaped = pattern.get(p.saturating_add(1))?;
            eq(escaped, c, flags).then(|| p.saturating_add(2))
        }
        _ => eq(pc, c, flags).then(|| p.saturating_add(1)),
    }
}

/// Is `name[n]` the first byte of a path component?
///
/// Only meaningful under [`Flags::PERIOD`], and only asks about a separator
/// when [`Flags::PATHNAME`] is also set — see that flag's documentation for why
/// the dependency is glibc's rather than an oversight here.
fn at_component_start(name: &[u8], n: usize, flags: Flags) -> bool {
    if n == 0 {
        return true;
    }
    flags.has(Flags::PATHNAME) && name.get(n.saturating_sub(1)) == Some(&b'/')
}

/// Compare two bytes under [`Flags::CASEFOLD`].
fn eq(a: u8, b: u8, flags: Flags) -> bool {
    if flags.has(Flags::CASEFOLD) {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// What reading a `[…]` produced.
enum Bracket {
    /// It matched; the pattern continues at this index (just past the `]`).
    Matched(usize),
    /// It was well formed and did not match. Where the pattern continues is not
    /// recorded because nothing can use it: a rejected item ends this attempt,
    /// and the star loop restarts from the star rather than from here.
    Rejected,
    /// No closing `]`, or a construct this module does not implement.
    Malformed,
}

/// Read the bracket expression that starts at `pattern[p]` (which is `[`) and
/// say whether `c` is in it.
///
/// The POSIX oddities, all of which the copy in `find.rs` got wrong:
///
/// - `!` or `^` immediately after the `[` negates the set. Both spellings, and
///   only in that one position — `[a!b]` contains a literal `!`.
/// - A `]` immediately after the `[` (or after the negation) is a **member**,
///   not the terminator, so `[]]` is the set containing `]`.
/// - `-` is a range only between two members. First or last it is a literal,
///   so `[-a]` and `[a-]` are two-member sets.
/// - `[[:alpha:]]` is a class. The inner `[:` is not a nested bracket.
/// - Under [`Flags::PATHNAME`] a bracket never matches `/`, however it is
///   spelled — including via a negated set, which is the case that silently
///   goes wrong: `[!a]` must not match `/`.
fn bracket(pattern: &[u8], p: usize, c: u8, flags: Flags) -> Bracket {
    let mut i = p.saturating_add(1);
    let negated = matches!(pattern.get(i), Some(&b'!' | &b'^'));
    if negated {
        i = i.saturating_add(1);
    }

    let mut found = false;
    let mut first = true;
    loop {
        let Some(&b) = pattern.get(i) else {
            return Bracket::Malformed;
        };
        if b == b']' && !first {
            i = i.saturating_add(1);
            break;
        }
        first = false;

        // `[:class:]`. `[.sym.]` and `[=equiv=]` are refused rather than read
        // as punctuation; see the module docs.
        if b == b'[' && matches!(pattern.get(i.saturating_add(1)), Some(&b':')) {
            let start = i.saturating_add(2);
            let Some(end) = find_close(pattern, start, b':') else {
                return Bracket::Malformed;
            };
            let Some(name) = pattern.get(start..end) else {
                return Bracket::Malformed;
            };
            if !class_exists(name) {
                return Bracket::Malformed;
            }
            found |= in_class(name, c);
            i = end.saturating_add(2);
            continue;
        }
        if b == b'[' && matches!(pattern.get(i.saturating_add(1)), Some(&b'.' | &b'=')) {
            return Bracket::Malformed;
        }

        // One member, possibly the low end of a range.
        let (low, mut next) = match b {
            b'\\' if !flags.has(Flags::NOESCAPE) => {
                let Some(&escaped) = pattern.get(i.saturating_add(1)) else {
                    return Bracket::Malformed;
                };
                (escaped, i.saturating_add(2))
            }
            _ => (b, i.saturating_add(1)),
        };

        // A `-` is a range separator only when something other than `]` follows
        // it; `[a-]` is the two-member set `a`, `-`.
        if pattern.get(next) == Some(&b'-')
            && !matches!(pattern.get(next.saturating_add(1)), None | Some(&b']'))
        {
            let after_dash = next.saturating_add(1);
            let (high, past) = match pattern.get(after_dash) {
                Some(&b'\\') if !flags.has(Flags::NOESCAPE) => {
                    let Some(&escaped) = pattern.get(after_dash.saturating_add(1)) else {
                        return Bracket::Malformed;
                    };
                    (escaped, after_dash.saturating_add(2))
                }
                Some(&other) => (other, after_dash.saturating_add(1)),
                None => return Bracket::Malformed,
            };
            found |= in_range(low, high, c, flags);
            next = past;
        } else {
            found |= eq(low, c, flags);
        }
        i = next;
    }

    let matched = found != negated;
    // The separator rule is applied after the set is read rather than during
    // it, because it must also veto a *negated* set that would otherwise
    // accept `/` by not mentioning it.
    if matched && !(flags.has(Flags::PATHNAME) && c == b'/') {
        Bracket::Matched(i)
    } else {
        Bracket::Rejected
    }
}

/// The index of the `X]` that closes a `[X…X]` construct, where `X` is `:`,
/// `.` or `=`. Returns the index of the `X`.
fn find_close(pattern: &[u8], from: usize, delim: u8) -> Option<usize> {
    let mut i = from;
    while let Some(&b) = pattern.get(i) {
        if b == delim && pattern.get(i.saturating_add(1)) == Some(&b']') {
            return Some(i);
        }
        i = i.saturating_add(1);
    }
    None
}

/// The twelve POSIX character-class names.
///
/// An unknown name is a *malformed* bracket, not an empty class: glibc returns
/// `FNM_NOMATCH` for `[[:bogus:]]` against everything, and reading it as "no
/// members" would give the same answer for the positive form but the opposite
/// one for `[![:bogus:]]`.
fn class_exists(name: &[u8]) -> bool {
    matches!(
        name,
        b"alnum"
            | b"alpha"
            | b"blank"
            | b"cntrl"
            | b"digit"
            | b"graph"
            | b"lower"
            | b"print"
            | b"punct"
            | b"space"
            | b"upper"
            | b"xdigit"
    )
}

/// Is `c` in the named class?
///
/// The C locale, so these are the ASCII answers and a byte ≥ 0x80 is in none of
/// them. That is the honest answer for a matcher with no character set: calling
/// 0xE9 `alpha` because Latin-1 says so would misclassify the same byte inside
/// a UTF-8 `é`.
///
/// There is deliberately no [`Flags::CASEFOLD`] parameter: a class is tested
/// against the byte as written, so `[[:upper:]]` does not match `a` even when
/// folding is on. That is surprising enough to have been implemented wrongly
/// here first, and it is measured — glibc answers `FNM_NOMATCH` for
/// `fnmatch("[[:upper:]]", "a", FNM_CASEFOLD)` while `[a-c]` *does* match `B`
/// under the same flag, so folding reaches a range but not a class.
fn in_class(name: &[u8], c: u8) -> bool {
    match name {
        b"alnum" => c.is_ascii_alphanumeric(),
        b"alpha" => c.is_ascii_alphabetic(),
        b"blank" => c == b' ' || c == b'\t',
        b"cntrl" => c.is_ascii_control(),
        b"digit" => c.is_ascii_digit(),
        b"graph" => c.is_ascii_graphic(),
        b"lower" => c.is_ascii_lowercase(),
        b"print" => c.is_ascii_graphic() || c == b' ',
        b"punct" => c.is_ascii_punctuation(),
        b"space" => c.is_ascii_whitespace() || c == 0x0b,
        b"upper" => c.is_ascii_uppercase(),
        b"xdigit" => c.is_ascii_hexdigit(),
        _ => false,
    }
}

/// Is `c` within `low..=high`?
///
/// Byte order, which is the C locale's collating order. A locale-aware range
/// would need a collation table this system does not have, and guessing one is
/// how `[a-z]` comes to include `Z` on some systems and not others.
fn in_range(low: u8, high: u8, c: u8, flags: Flags) -> bool {
    if (low..=high).contains(&c) {
        return true;
    }
    if flags.has(Flags::CASEFOLD) {
        let folded = if c.is_ascii_lowercase() {
            c.to_ascii_uppercase()
        } else {
            c.to_ascii_lowercase()
        };
        return (low..=high).contains(&folded);
    }
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn m(pattern: &str, name: &str) -> bool {
        fnmatch(pattern.as_bytes(), name.as_bytes(), Flags::NONE)
    }

    fn mf(pattern: &str, name: &str, flags: Flags) -> bool {
        fnmatch(pattern.as_bytes(), name.as_bytes(), flags)
    }

    // ------------------------------------------------------------ literals --

    #[test]
    fn a_pattern_with_no_wildcard_is_an_equality_test() {
        assert!(m("main.o", "main.o"));
        assert!(!m("main.o", "main.c"));
        assert!(!m("main", "main.o"));
        assert!(!m("main.o", "main"));
        assert!(m("", ""));
        assert!(!m("", "x"));
        assert!(!m("x", ""));
    }

    // --------------------------------------------------------------- star --

    #[test]
    fn star_matches_any_run_including_none() {
        assert!(m("*", ""));
        assert!(m("*", "anything"));
        assert!(m("*.o", "main.o"));
        assert!(m("*.o", ".o"));
        assert!(m("a*", "a"));
        assert!(m("a*b", "ab"));
        assert!(m("a*b", "axxxb"));
        assert!(!m("a*b", "axxx"));
    }

    #[test]
    fn several_stars_collapse() {
        assert!(m("**", "abc"));
        assert!(m("a**b", "ab"));
        assert!(m("*a*", "xxaxx"));
        assert!(!m("*a*", "xxxx"));
    }

    /// The reason this module exists in place of `find.rs`'s recursive matcher:
    /// that one explores every way to split the name between the stars, so this
    /// case takes 2^40 steps and never returns. Here it is a bounded walk.
    ///
    /// The assertion is `!` — there is no `y` — which is precisely the input
    /// that forces the full search.
    #[test]
    fn many_stars_do_not_blow_up() {
        let pattern = "*x*x*x*x*x*x*x*y";
        let name = "x".repeat(64);
        assert!(!fnmatch(pattern.as_bytes(), name.as_bytes(), Flags::NONE));
    }

    #[test]
    fn a_star_can_give_back_what_it_took() {
        // The classic backtrack: the first star must release the final `b` so
        // the literal can have it.
        assert!(m("*b", "abab"));
        assert!(m("*ab", "aab"));
        assert!(m("a*a*b", "aaab"));
    }

    // ----------------------------------------------------------- question --

    #[test]
    fn question_matches_exactly_one_byte() {
        assert!(m("?", "a"));
        assert!(!m("?", ""));
        assert!(!m("?", "ab"));
        assert!(m("a?c", "abc"));
        assert!(!m("a?c", "ac"));
    }

    /// A `?` is one **byte**, not one character. That is glibc's answer in the
    /// C locale and the only answer available on a filesystem whose names are
    /// byte strings — `é` in UTF-8 is two bytes and takes two `?`.
    #[test]
    fn question_is_a_byte_not_a_character() {
        assert!(!fnmatch(b"?", "\u{e9}".as_bytes(), Flags::NONE));
        assert!(fnmatch(b"??", "\u{e9}".as_bytes(), Flags::NONE));
    }

    #[test]
    fn a_name_that_is_not_utf8_still_matches() {
        // The whole point of matching bytes: this name is legal on this OS and
        // cannot be turned into a `String` at all.
        assert!(fnmatch(b"a*c", b"a\xff\xfec", Flags::NONE));
        assert!(fnmatch(b"a?c", b"a\xffc", Flags::NONE));
        assert!(!fnmatch(b"a\xffc", b"a\xfec", Flags::NONE));
    }

    // ----------------------------------------------------------- brackets --

    #[test]
    fn a_bracket_is_a_set() {
        assert!(m("[abc]", "b"));
        assert!(!m("[abc]", "d"));
        assert!(m("x[abc]z", "xbz"));
    }

    #[test]
    fn a_bracket_takes_ranges() {
        assert!(m("[a-z]", "q"));
        assert!(!m("[a-z]", "Q"));
        assert!(m("[0-9]", "5"));
        assert!(m("[a-cx-z]", "y"));
        assert!(!m("[a-cx-z]", "m"));
    }

    /// `find.rs`'s copy read `[a-z]` as the three-member set `a`, `-`, `z`, so
    /// it matched a hyphen and missed every other letter. Both halves of that
    /// mistake are asserted here.
    #[test]
    fn a_range_is_not_three_literals() {
        assert!(m("[a-z]", "m"));
        assert!(!m("[a-z]", "-"));
    }

    #[test]
    fn a_hyphen_at_either_end_is_a_literal() {
        assert!(m("[-a]", "-"));
        assert!(m("[-a]", "a"));
        assert!(m("[a-]", "-"));
        assert!(m("[a-]", "a"));
        assert!(!m("[a-]", "b"));
    }

    #[test]
    fn a_bracket_negates_with_either_mark() {
        assert!(m("[!a]", "b"));
        assert!(!m("[!a]", "a"));
        assert!(m("[^a]", "b"));
        assert!(!m("[^a]", "a"));
        // Only in the first position.
        assert!(m("[a!]", "!"));
        assert!(m("[a^]", "^"));
    }

    #[test]
    fn a_closing_bracket_first_is_a_member() {
        assert!(m("[]]", "]"));
        assert!(m("[]a]", "a"));
        assert!(m("[!]]", "a"));
        assert!(!m("[!]]", "]"));
    }

    #[test]
    fn an_unclosed_bracket_is_a_literal_bracket() {
        assert!(m("[", "["));
        assert!(!m("[", "a"));
        assert!(m("[abc", "[abc"));
        assert!(!m("[abc", "["));
        assert!(!m("[abc", "a"));
        assert!(m("a[b", "a[b"));
    }

    #[test]
    fn character_classes() {
        assert!(m("[[:digit:]]", "7"));
        assert!(!m("[[:digit:]]", "x"));
        assert!(m("[[:alpha:]]*", "hello"));
        assert!(m("[[:space:]]", " "));
        assert!(m("[[:upper:]]", "Q"));
        assert!(!m("[[:upper:]]", "q"));
        assert!(m("[[:punct:]]", "!"));
        assert!(m("[[:xdigit:]]", "f"));
        assert!(!m("[[:xdigit:]]", "g"));
    }

    #[test]
    fn a_class_can_share_a_bracket_with_literals() {
        assert!(m("[[:digit:]abc]", "b"));
        assert!(m("[[:digit:]abc]", "3"));
        assert!(!m("[[:digit:]abc]", "z"));
        assert!(m("[![:digit:]]", "z"));
        assert!(!m("[![:digit:]]", "3"));
    }

    /// An unknown class name matches nothing — including under negation, which
    /// is the half that distinguishes "malformed" from "empty set".
    #[test]
    fn an_unknown_class_matches_nothing() {
        assert!(!m("[[:bogus:]]", "a"));
        assert!(!m("[![:bogus:]]", "a"));
    }

    /// The delimiters are `[.` and `[=` *inside* a bracket, so the construct is
    /// `[[.a.]]` and not `[.a.]` — the latter is an ordinary two-member set and
    /// glibc reads it that way too. Only the doubled form is refused.
    #[test]
    fn collating_and_equivalence_are_refused_not_misread() {
        assert!(m("[.a.]", "."));
        assert!(m("[.a.]", "a"));
        assert!(m("[=a=]", "a"));
        assert!(m("[=a=]", "="));

        // Refused: glibc would match `a` for both of these.
        assert!(!m("[[.a.]]", "a"));
        assert!(!m("[[=a=]]", "a"));
        // And refused *as a whole bracket*, so the members beside it are lost
        // too rather than the construct being skipped over.
        assert!(!m("[[.a.]b]", "b"));
    }

    #[test]
    fn a_byte_over_127_is_in_no_class() {
        assert!(!fnmatch(b"[[:alpha:]]", b"\xe9", Flags::NONE));
        assert!(!fnmatch(b"[[:print:]]", b"\xe9", Flags::NONE));
        assert!(fnmatch(b"[![:alpha:]]", b"\xe9", Flags::NONE));
    }

    // ------------------------------------------------------------ escapes --

    #[test]
    fn backslash_makes_a_wildcard_literal() {
        assert!(m("\\*", "*"));
        assert!(!m("\\*", "abc"));
        assert!(m("a\\?b", "a?b"));
        assert!(!m("a\\?b", "axb"));
        assert!(m("\\[abc\\]", "[abc]"));
    }

    #[test]
    fn backslash_inside_a_bracket_escapes_too() {
        assert!(m("[\\]]", "]"));
        assert!(m("[a\\-c]", "-"));
        assert!(!m("[a\\-c]", "b"));
    }

    #[test]
    fn noescape_makes_backslash_ordinary() {
        assert!(mf("\\", "\\", Flags::NOESCAPE));
        assert!(mf("a\\b", "a\\b", Flags::NOESCAPE));
        assert!(!mf("\\*", "*", Flags::NOESCAPE));
        assert!(mf("\\*", "\\anything", Flags::NOESCAPE));
    }

    /// "Trailing \ loses" — a pattern ending in a lone `\` matches nothing at
    /// all, not even a name ending in a backslash. Measured against glibc,
    /// which is the only way anyone would arrive at this rule; reading it as a
    /// literal backslash is the plausible wrong answer and was ours first.
    #[test]
    fn a_trailing_backslash_matches_nothing() {
        assert!(!m("\\", "\\"));
        assert!(!m("a\\", "a\\"));
        assert!(!m("a\\", "a"));
        // With NOESCAPE there is no escape to be left dangling.
        assert!(mf("a\\", "a\\", Flags::NOESCAPE));
    }

    // ----------------------------------------------------------- PATHNAME --

    #[test]
    fn pathname_stops_every_wildcard_at_a_separator() {
        assert!(!mf("a*c", "a/b/c", Flags::PATHNAME));
        assert!(!mf("*", "a/b", Flags::PATHNAME));
        assert!(!mf("a?c", "a/c", Flags::PATHNAME));
        assert!(!mf("a[/]c", "a/c", Flags::PATHNAME));
        assert!(mf("a/*/c", "a/b/c", Flags::PATHNAME));
        assert!(mf("*/*", "a/b", Flags::PATHNAME));
    }

    /// The case that goes wrong silently: a negated set does not mention `/`,
    /// so a matcher that only vetoes the separator inside the positive branch
    /// lets `[!a]` match it.
    #[test]
    fn pathname_vetoes_a_separator_a_negated_set_would_have_taken() {
        assert!(!mf("a[!x]c", "a/c", Flags::PATHNAME));
        assert!(mf("a[!x]c", "a/c", Flags::NONE));
    }

    /// `du --exclude` relies on this, measured: `du --exclude='*/keep'` prunes
    /// `ex/aa/keep`, which requires `*` to cross the separator.
    #[test]
    fn without_pathname_a_star_crosses_separators() {
        assert!(m("*/keep", "ex/aa/keep"));
        assert!(m("a*c", "a/b/c"));
    }

    // ------------------------------------------------------------- PERIOD --

    #[test]
    fn period_hides_a_leading_dot_from_wildcards() {
        assert!(!mf("*", ".bashrc", Flags::PERIOD));
        assert!(!mf("?bashrc", ".bashrc", Flags::PERIOD));
        assert!(!mf("[.]bashrc", ".bashrc", Flags::PERIOD));
        assert!(mf(".*", ".bashrc", Flags::PERIOD));
        assert!(mf("*", "bashrc", Flags::PERIOD));
    }

    #[test]
    fn an_escaped_dot_still_counts_as_literal() {
        assert!(mf("\\.*", ".bashrc", Flags::PERIOD));
    }

    /// A dot mid-component is ordinary; only the first byte is special.
    #[test]
    fn period_only_guards_the_first_byte() {
        assert!(mf("a*", "a.b", Flags::PERIOD));
        assert!(mf("*", "a.b", Flags::PERIOD));
    }

    /// The glibc dependency worth pinning: `PERIOD` guards the byte after a
    /// separator **only** when `PATHNAME` is set too.
    #[test]
    fn period_guards_later_components_only_under_pathname() {
        assert!(mf("a/*", "a/.b", Flags::PERIOD));
        assert!(!mf("a/*", "a/.b", Flags::PERIOD | Flags::PATHNAME));
        assert!(mf("a/.*", "a/.b", Flags::PERIOD | Flags::PATHNAME));
        assert!(!mf("*", "a/.b", Flags::PERIOD | Flags::PATHNAME));
    }

    // ----------------------------------------------------------- CASEFOLD --

    #[test]
    fn casefold_folds_ascii_only() {
        assert!(mf("MAIN.O", "main.o", Flags::CASEFOLD));
        assert!(mf("[a-z]", "Q", Flags::CASEFOLD));
        assert!(!mf("MAIN.O", "main.c", Flags::CASEFOLD));
        // The fold reaches a *range* but not a *class*, which is glibc's rule
        // and not a guessable one — see `in_class`.
        assert!(!mf("[[:lower:]]", "Q", Flags::CASEFOLD));
        assert!(!mf("[[:upper:]]", "q", Flags::CASEFOLD));
        assert!(mf("[[:upper:]]", "Q", Flags::CASEFOLD));
        // A non-ASCII byte is left alone: folding it by byte would be right for
        // Latin-1 and wrong for the UTF-8 this system actually uses.
        assert!(!fnmatch(b"\xc9", b"\xe9", Flags::CASEFOLD));
    }

    // -------------------------------------------------------- LEADING_DIR --

    #[test]
    fn leading_dir_matches_a_whole_subtree() {
        assert!(mf("a", "a/b/c", Flags::LEADING_DIR));
        assert!(mf("a", "a", Flags::LEADING_DIR));
        assert!(mf("a*", "ax/b", Flags::LEADING_DIR));
        assert!(!mf("a", "ab/c", Flags::LEADING_DIR));
        assert!(!mf("ab", "a/b", Flags::LEADING_DIR));
        // Off by default, which is what `du` wants.
        assert!(!m("a", "a/b/c"));
    }

    // -------------------------------------------------------------- flags --

    #[test]
    fn flags_combine_and_report() {
        let f = Flags::PATHNAME | Flags::PERIOD;
        assert!(f.has(Flags::PATHNAME));
        assert!(f.has(Flags::PERIOD));
        assert!(!f.has(Flags::CASEFOLD));
        assert!(f.has(Flags::NONE));
        assert!(Flags::NONE.has(Flags::NONE));
    }
}

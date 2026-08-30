//! Substring search whose offsets are offsets into the string you searched.
//!
//! # Why this crate exists
//!
//! Every text-handling application in this tree needs the same thing: find a
//! query in a line, optionally ignoring case, and get back a byte range it can
//! then *highlight or replace*. Eight of them wrote it independently, and all
//! eight wrote the same three bugs. This crate is the one implementation, with
//! the bugs written down so nobody re-derives them.
//!
//! ## Bug 1 — the offsets are for a string the user is not editing
//!
//! The tempting shape is:
//!
//! ```ignore
//! let hay = line.to_lowercase();
//! let needle = query.to_lowercase();
//! while let Some(p) = hay[start..].find(&needle) { … }
//! ```
//!
//! `p` is an offset into `hay`. It then gets used to slice, highlight or
//! `replace_range` **`line`** — a different string. That is only sound if
//! lowercasing preserves length, and it does not: Turkish `İ` (U+0130, two
//! bytes) lowercases to `i` plus a combining dot above (three bytes). Past the
//! first such character every offset is wrong, so the editor selects the wrong
//! text, replaces the wrong bytes, or hands `String::replace_range` a range
//! past the end of the line — which panics.
//!
//! It is worse than an off-by-a-few: `hay[start..]` itself panics once `start`
//! lands inside a character of the folded copy, which the very next iteration
//! can arrange.
//!
//! The fix is to fold *incrementally while walking the real string*, which is
//! what [`match_at`] does. Nothing is ever allocated and no offset ever refers
//! to anything but `haystack`.
//!
//! ## Bug 2 — the match length is assumed to be the needle's length
//!
//! `abs_pos + needle.len()` is wrong for the same reason, in the other
//! direction: a two-byte `İ` in the haystack can be matched by a three-byte
//! folded needle, or vice versa. So [`match_at`] returns the match's *end*
//! rather than letting the caller compute one.
//!
//! ## Bug 3 — resuming one byte into the match just found
//!
//! Several copies carried a `start = abs_pos + 1; // allow overlapping
//! matches` line. Overlapping matches are not what a find/replace wants:
//! `"aaaa"` reports three occurrences of `"aa"`, the count shown to the user is
//! wrong, and a replace-all that rewrites overlapping ranges shreds the line.
//! The one-byte step also lands inside a multi-byte character, where the next
//! slice panics. [`matches`] resumes past the end of each match.
//!
//! # What "case-insensitive" means here
//!
//! Two characters match when [`char::to_lowercase`] maps them to the same
//! sequence. This is Unicode *simple lowercase mapping applied per character*,
//! not full case folding: it is what every call site in this tree already did
//! via `to_lowercase()`, so adopting it changes no application's idea of what
//! matches — only where the match is. Notably `ß` does not match `SS`, and the
//! Turkish dotless `ı` does not match `I`.
//!
//! A match must end on a character boundary of the haystack. If the needle's
//! folding runs out halfway through the folding of a haystack character, there
//! is no match there — a caller could not highlight or replace half a
//! character anyway.
//!
//! # Example
//!
//! ```
//! use textfind::{Case, matches};
//!
//! // The match is at bytes 0..2 of the *line* — `İ` is two bytes — even
//! // though it is three bytes long once folded.
//! let hits: Vec<_> = matches("İx", "i\u{307}", Case::Insensitive).collect();
//! assert_eq!(hits, [(0, 2)]);
//!
//! // Non-overlapping: two matches in "aaaa", not three.
//! let hits: Vec<_> = matches("aaaa", "aa", Case::Sensitive).collect();
//! assert_eq!(hits, [(0, 2), (2, 4)]);
//! ```

#![no_std]

use core::cmp::Ordering;

/// Whether a search distinguishes upper from lower case.
///
/// This is an enum rather than the `bool` every caller stores because a bare
/// `true` at a call site says nothing about which way round it goes, and a
/// search that silently inverts its case setting is a bug that looks like a
/// user error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// `a` does not match `A`.
    Sensitive,
    /// `a` matches `A`, per [`char::to_lowercase`].
    Insensitive,
}

impl Case {
    /// The `Case` for a `case_sensitive` flag, which is how the search UIs in
    /// this tree store the setting.
    #[must_use]
    pub fn sensitive(case_sensitive: bool) -> Self {
        if case_sensitive {
            Self::Sensitive
        } else {
            Self::Insensitive
        }
    }

    /// Whether this is [`Case::Sensitive`].
    #[must_use]
    pub fn is_sensitive(self) -> bool {
        matches!(self, Self::Sensitive)
    }
}

/// Byte offset just past a match of `needle` starting *exactly* at byte offset
/// `at` in `haystack`, or `None` if there is no match there.
///
/// The end is returned rather than computed by the caller because a
/// case-insensitive match need not be the same length as the needle — see the
/// crate documentation.
///
/// Returns `None` if `at` is out of range or not a character boundary, so a
/// stale offset produces no match rather than a panic.
///
/// # Examples
///
/// ```
/// use textfind::{Case, match_at};
///
/// assert_eq!(match_at("hello", 1, "ell", Case::Sensitive), Some(4));
/// assert_eq!(match_at("hello", 0, "ell", Case::Sensitive), None);
/// // `İ` is two bytes in the haystack, three once folded.
/// assert_eq!(match_at("İx", 0, "i\u{307}", Case::Insensitive), Some(2));
/// // A needle that ends halfway through a haystack character does not match.
/// assert_eq!(match_at("İx", 0, "i", Case::Insensitive), None);
/// ```
#[must_use]
pub fn match_at(haystack: &str, at: usize, needle: &str, case: Case) -> Option<usize> {
    let rest = haystack.get(at..)?;
    if case.is_sensitive() {
        return rest
            .starts_with(needle)
            .then(|| at.saturating_add(needle.len()));
    }

    let mut folded_needle = needle.chars().flat_map(char::to_lowercase);
    let mut want = folded_needle.next();
    let mut end = at;
    for (off, hc) in rest.char_indices() {
        if want.is_none() {
            break;
        }
        for hf in hc.to_lowercase() {
            match want {
                Some(nf) if nf == hf => want = folded_needle.next(),
                // Either a mismatch, or the needle ran out partway through
                // this haystack character. Neither is something a caller
                // could select or replace.
                _ => return None,
            }
        }
        end = at.saturating_add(off).saturating_add(hc.len_utf8());
    }
    want.is_none().then_some(end)
}

/// The first match of `needle` in `haystack` at or after byte offset `from`,
/// as a `(start, end)` byte range in `haystack`.
///
/// Returns `None` if there is none, if `needle` is empty, or if `from` is out
/// of range or not a character boundary.
///
/// # Examples
///
/// ```
/// use textfind::{Case, find_from};
///
/// assert_eq!(find_from("abcabc", "bc", 0, Case::Sensitive), Some((1, 3)));
/// assert_eq!(find_from("abcabc", "bc", 2, Case::Sensitive), Some((4, 6)));
/// assert_eq!(find_from("abcabc", "BC", 0, Case::Insensitive), Some((1, 3)));
/// ```
#[must_use]
pub fn find_from(haystack: &str, needle: &str, from: usize, case: Case) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let rest = haystack.get(from..)?;
    if case.is_sensitive() {
        // The needle is a fixed byte sequence, so the standard search applies
        // and is far faster than walking characters.
        let p = rest.find(needle)?;
        let start = from.saturating_add(p);
        return Some((start, start.saturating_add(needle.len())));
    }
    rest.char_indices().find_map(|(off, _)| {
        let start = from.saturating_add(off);
        match_at(haystack, start, needle, case).map(|end| (start, end))
    })
}

/// The non-overlapping matches of `needle` in `haystack`, left to right, each
/// as a `(start, end)` byte range in `haystack`.
///
/// Empty for an empty needle: every position matches it, which is never what a
/// find command means.
///
/// # Examples
///
/// ```
/// use textfind::{Case, matches};
///
/// let hits: Vec<_> = matches("aaaa", "aa", Case::Sensitive).collect();
/// assert_eq!(hits, [(0, 2), (2, 4)]);
/// let none: Vec<_> = matches("aaaa", "", Case::Sensitive).collect();
/// assert!(none.is_empty());
/// ```
#[must_use]
pub fn matches<'h, 'n>(haystack: &'h str, needle: &'n str, case: Case) -> Matches<'h, 'n> {
    Matches {
        haystack,
        needle,
        case,
        at: 0,
        done: needle.is_empty(),
    }
}

/// Whether `haystack` contains `needle`, under `case`.
///
/// Prefer this to `haystack.to_lowercase().contains(&needle.to_lowercase())`:
/// it allocates nothing, and it agrees with [`matches`] about what a match is,
/// so a filter and a highlighter cannot disagree about whether a row matched.
///
/// An empty needle is contained in everything, matching [`str::contains`] —
/// this is the one place the empty needle is not treated as "no match",
/// because a filter with an empty query should show everything rather than
/// nothing.
///
/// # Examples
///
/// ```
/// use textfind::{Case, contains};
///
/// assert!(contains("Hello", "ell", Case::Insensitive));
/// assert!(contains("Hello", "ELL", Case::Insensitive));
/// assert!(!contains("Hello", "ELL", Case::Sensitive));
/// assert!(contains("Hello", "", Case::Sensitive));
/// ```
#[must_use]
pub fn contains(haystack: &str, needle: &str, case: Case) -> bool {
    if needle.is_empty() {
        return true;
    }
    find_from(haystack, needle, 0, case).is_some()
}

/// Order `a` against `b` under `case`.
///
/// Under [`Case::Sensitive`] this is `str`'s own ordering. Under
/// [`Case::Insensitive`] the two strings are compared by their folded
/// character sequences, so `"apple"` sorts before `"Banana"` rather than after
/// it — the byte order that `str::cmp` gives puts every capital before every
/// lowercase letter, which is not an order any user asked for.
///
/// # Why this is here rather than written at each call site
///
/// `a.to_lowercase().cmp(&b.to_lowercase())` is the obvious spelling and is
/// wrong in the same family as bugs 1–3 above, in a fourth way. It allocates
/// two strings *per comparison*, and a comparison is what a sort does
/// `n log n` times — sorting a directory of ten thousand names allocates a
/// quarter of a million strings to answer a question that needs none. This
/// folds lazily and stops at the first differing character, which for a
/// typical file list is the first or second one.
///
/// It also agrees with [`match_at`] about what "the same, ignoring case"
/// means. A list that is *filtered* by [`contains`] and *sorted* by a
/// hand-written comparison can otherwise disagree with itself: two names the
/// filter treats as equal land in an order the filter's own rule says is
/// arbitrary.
///
/// # Equal is not identical
///
/// Under [`Case::Insensitive`] this returns [`Ordering::Equal`] for two
/// *different* strings whenever they fold alike — `"README"` and `"readme"`,
/// and also `"İ"` and `"i\u{307}"`. That is the correct answer to the question
/// asked, but it means this must not be used alone as the ordering behind an
/// `Ord` impl whose `PartialEq` is byte equality: the two would disagree, and
/// `Ord`'s contract requires `a.cmp(b) == Equal` exactly when `a == b`.
/// Break the tie with `a.cmp(b)` when this returns `Equal`.
///
/// # Examples
///
/// ```
/// use core::cmp::Ordering;
/// use textfind::{Case, compare};
///
/// // Byte order puts every capital first; folded order does not.
/// assert_eq!(compare("apple", "Banana", Case::Sensitive), Ordering::Greater);
/// assert_eq!(compare("apple", "Banana", Case::Insensitive), Ordering::Less);
///
/// // Folding alike is Equal even though the strings differ.
/// assert_eq!(compare("README", "readme", Case::Insensitive), Ordering::Equal);
/// assert_eq!(compare("\u{130}", "i\u{307}", Case::Insensitive), Ordering::Equal);
/// ```
#[must_use]
pub fn compare(a: &str, b: &str, case: Case) -> Ordering {
    if case.is_sensitive() {
        return a.cmp(b);
    }
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

/// Rank how well `query` fuzzy-matches `target`, or `None` if it does not
/// match at all. Higher is better; the number is meaningful only against other
/// scores from this same function.
///
/// A match means every character of `query` appears in `target`, in order,
/// ignoring case, but not necessarily next to each other — so `"fx"` matches
/// `"Firefox"`. The score rewards, in descending weight:
///
/// | Situation | Bonus |
/// |---|---|
/// | `query` is a prefix of `target` | 50 |
/// | each character matched at a word boundary (string start, or after a space, `-` or `_`) | 10 |
/// | each character matched immediately after the previous match | 5 |
/// | matching early in `target` | 20 minus the index of the first match |
/// | `target` being little longer than `query` | 10 minus the surplus characters, floored at 0 |
///
/// # Why this is here
///
/// It was written three times — the desktop launcher, the Run dialog and the
/// standalone launcher application — with the second copy carrying the comment
/// "uses the same algorithm as the application launcher for consistency". A
/// comment is not a mechanism: it states the invariant that the code is
/// supposed to maintain while doing nothing to maintain it, so the first time
/// one copy's weights are tuned, the same query silently ranks two lists in two
/// different orders and the comment still reads true.
///
/// It lives in this crate rather than the toolkit for the same reason
/// everything else here does: ranking a list of candidates against typed text
/// is not a widget's job, and the headless components search too.
///
/// # Case, and what changed by moving here
///
/// Characters are compared by [`char::to_lowercase`], the same rule as
/// [`match_at`] and [`compare`] — not the `to_ascii_lowercase` the three copies
/// used. That is a behaviour change in exactly one direction: `"É"` now matches
/// `"éclair"`, where before a non-ASCII query character only matched itself.
/// Every ASCII score is unchanged, and no copy had a test that a non-ASCII
/// query *failed*.
///
/// Nothing is allocated. The copies built two `Vec<char>` per candidate, which
/// for a launcher is two allocations per installed application per keystroke.
///
/// # Examples
///
/// ```
/// use textfind::fuzzy_score;
///
/// // Characters must appear in order, but need not be adjacent.
/// assert!(fuzzy_score("fx", "Firefox").is_some());
/// assert_eq!(fuzzy_score("xf", "Firefox"), None);
///
/// // An empty query matches everything, at the lowest score.
/// assert_eq!(fuzzy_score("", "anything"), Some(0));
///
/// // A prefix beats the same characters found in the middle.
/// let prefix = fuzzy_score("fi", "file").unwrap();
/// let middle = fuzzy_score("fi", "wifi").unwrap();
/// assert!(prefix > middle);
/// ```
#[must_use]
pub fn fuzzy_score(query: &str, target: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }

    let query_len = query.chars().count();
    if query_len > target.chars().count() {
        return None;
    }

    // `zip` stops at the shorter side, and `query` is no longer than `target`,
    // so this asks "is query a prefix of target".
    let is_prefix = target
        .chars()
        .zip(query.chars())
        .all(|(t, q)| eq_folded(t, q));

    let mut score: u32 = 0;
    let mut matched: usize = 0;
    let mut wanted = query.chars().peekable();
    let mut prev_match_idx: Option<usize> = None;
    let mut first_match_idx: Option<usize> = None;
    let mut prev_char: Option<char> = None;

    for (ti, tc) in target.chars().enumerate() {
        let Some(&qc) = wanted.peek() else { break };
        if eq_folded(tc, qc) {
            wanted.next();
            if first_match_idx.is_none() {
                first_match_idx = Some(ti);
            }

            let at_boundary = prev_char.is_none_or(|p| p == ' ' || p == '-' || p == '_');
            if at_boundary {
                score = score.saturating_add(10);
            }

            if prev_match_idx.is_some_and(|p| p.saturating_add(1) == ti) {
                score = score.saturating_add(5);
            }

            prev_match_idx = Some(ti);
            matched = matched.saturating_add(1);
        }
        prev_char = Some(tc);
    }

    // Every query character has to have been consumed. Anything less is a
    // candidate the user did not type towards.
    if matched < query_len {
        return None;
    }

    if is_prefix {
        score = score.saturating_add(50);
    }

    if let Some(idx) = first_match_idx {
        score = score.saturating_add(20u32.saturating_sub(u32::try_from(idx).unwrap_or(u32::MAX)));
    }

    // A shorter target is a more specific answer to the same query.
    let surplus = target.chars().count().saturating_sub(query_len);
    score = score.saturating_add(10u32.saturating_sub(u32::try_from(surplus).unwrap_or(u32::MAX)));

    Some(score)
}

/// Whether two characters are the same under simple lowercase mapping.
fn eq_folded(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Iterator over the non-overlapping matches of a needle. See [`matches`].
#[derive(Debug, Clone)]
pub struct Matches<'h, 'n> {
    haystack: &'h str,
    needle: &'n str,
    case: Case,
    at: usize,
    done: bool,
}

impl Iterator for Matches<'_, '_> {
    /// A `(start, end)` byte range in the haystack.
    type Item = (usize, usize);

    fn next(&mut self) -> Option<(usize, usize)> {
        if self.done {
            return None;
        }
        let Some((start, end)) = find_from(self.haystack, self.needle, self.at, self.case) else {
            self.done = true;
            return None;
        };
        // Resume *past* the match. Stepping one byte instead — which is what
        // the copies this crate replaces did, under a comment claiming
        // overlapping matches were wanted — both reports "aaaa" as three
        // occurrences of "aa" and lands inside a multi-byte character.
        self.at = if end > start {
            end
        } else {
            start.saturating_add(1)
        };
        if self.at >= self.haystack.len() {
            self.done = true;
        }
        Some((start, end))
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]

    extern crate alloc;
    use super::{Case, Ordering, compare, contains, find_from, fuzzy_score, match_at, matches};
    use alloc::vec::Vec;

    /// The Turkish capital I with a dot above: two bytes, but three once
    /// lowercased (`i` + U+0307 COMBINING DOT ABOVE). This one character is
    /// the counterexample to every "just search the lowercased copy"
    /// implementation, which is why it appears throughout these tests.
    const DOTTED_I: &str = "\u{130}";

    #[test]
    fn an_offset_is_an_offset_into_the_string_that_was_searched() {
        let line = "xİy";
        let hits: Vec<_> = matches(line, "i\u{307}", Case::Insensitive).collect();
        assert_eq!(hits, [(1, 3)]);
        // The whole point: the range can be used on the line itself.
        assert_eq!(&line[1..3], DOTTED_I);
    }

    #[test]
    fn a_match_is_not_assumed_to_be_the_length_of_the_needle() {
        // Three-byte folded needle, two-byte match.
        assert_eq!(
            match_at(DOTTED_I, 0, "i\u{307}", Case::Insensitive),
            Some(2)
        );
        // And the other way round: two-byte needle, three-byte match.
        assert_eq!(
            match_at("i\u{307}", 0, DOTTED_I, Case::Insensitive),
            Some(3)
        );
    }

    #[test]
    fn offsets_stay_right_after_a_character_that_changes_length_when_folded() {
        // The bug this crate exists for. In the lowercased copy "abc" starts
        // at byte 3; in the real string it starts at byte 2.
        let line = "İabc";
        let hits: Vec<_> = matches(line, "ABC", Case::Insensitive).collect();
        assert_eq!(hits, [(2, 5)]);
        assert_eq!(&line[2..5], "abc");
    }

    #[test]
    fn matches_do_not_overlap() {
        let hits: Vec<_> = matches("aaaa", "aa", Case::Sensitive).collect();
        assert_eq!(hits, [(0, 2), (2, 4)]);
    }

    #[test]
    fn a_search_never_resumes_inside_a_character() {
        // A one-byte resume would land at byte 1 of a three-byte character.
        let hits: Vec<_> = matches("日本日本", "日本", Case::Sensitive).collect();
        assert_eq!(hits, [(0, 6), (6, 12)]);
    }

    #[test]
    fn a_needle_that_ends_mid_character_does_not_match() {
        // "i" folds one character of "İ"'s two, so there is no range to give.
        assert_eq!(match_at(DOTTED_I, 0, "i", Case::Insensitive), None);
        assert!(matches(DOTTED_I, "i", Case::Insensitive).next().is_none());
    }

    #[test]
    fn an_empty_needle_matches_nothing_but_is_contained_in_everything() {
        assert!(matches("abc", "", Case::Sensitive).next().is_none());
        assert_eq!(find_from("abc", "", 0, Case::Sensitive), None);
        assert!(contains("abc", "", Case::Sensitive));
    }

    #[test]
    fn a_stale_or_split_offset_yields_no_match_rather_than_a_panic() {
        assert_eq!(match_at("abc", 99, "a", Case::Sensitive), None);
        assert_eq!(find_from("abc", "a", 99, Case::Sensitive), None);
        // Byte 1 is inside the three-byte character.
        assert_eq!(match_at("日", 1, "日", Case::Sensitive), None);
        assert_eq!(find_from("日", "日", 1, Case::Sensitive), None);
        assert_eq!(find_from("日", "日", 1, Case::Insensitive), None);
    }

    #[test]
    fn case_sensitivity_is_honoured_both_ways() {
        assert_eq!(find_from("Hello", "hello", 0, Case::Sensitive), None);
        assert_eq!(
            find_from("Hello", "hello", 0, Case::Insensitive),
            Some((0, 5))
        );
        assert!(Case::sensitive(true).is_sensitive());
        assert!(!Case::sensitive(false).is_sensitive());
    }

    #[test]
    fn the_insensitive_path_agrees_with_the_sensitive_one_on_ascii_lowercase() {
        // Guards against the two paths drifting: for text where folding is the
        // identity they must produce identical ranges.
        for hay in ["", "a", "abcabc", "aaaa", "the quick brown fox"] {
            for needle in ["a", "abc", "aa", "z", "the", "fox"] {
                let s: Vec<_> = matches(hay, needle, Case::Sensitive).collect();
                let i: Vec<_> = matches(hay, needle, Case::Insensitive).collect();
                assert_eq!(s, i, "hay {hay:?} needle {needle:?}");
            }
        }
    }

    #[test]
    fn every_reported_range_slices_the_haystack() {
        // The property that actually matters to a caller, checked over text
        // chosen to break the assumptions the old implementations made:
        // length-changing folds, multi-byte characters, and repeats.
        let hays = [
            "",
            "a",
            "aaaa",
            "İ",
            "İİİ",
            "xİy",
            "日本日本",
            "AaAa",
            "İstanbul",
            "straße",
        ];
        let needles = ["a", "A", "aa", "İ", "i\u{307}", "日本", "ss", "st", "ß"];
        let mut checked = 0_u32;
        for hay in hays {
            for needle in needles {
                for case in [Case::Sensitive, Case::Insensitive] {
                    let mut last_end = 0;
                    for (start, end) in matches(hay, needle, case) {
                        assert!(
                            start >= last_end,
                            "overlap: {hay:?} {needle:?} {case:?} {start}..{end}"
                        );
                        assert!(
                            hay.get(start..end).is_some(),
                            "bad range: {hay:?} {needle:?} {case:?} {start}..{end}"
                        );
                        last_end = end;
                    }
                    checked = checked.saturating_add(1);
                }
            }
        }
        assert_eq!(checked, 10 * 9 * 2, "sweep did not cover every combination");
    }

    // ------------------------------------------------------------------
    // compare
    // ------------------------------------------------------------------

    #[test]
    fn sensitive_compare_is_str_ordering() {
        assert_eq!(compare("a", "b", Case::Sensitive), Ordering::Less);
        assert_eq!(compare("B", "a", Case::Sensitive), Ordering::Less);
        assert_eq!(compare("a", "a", Case::Sensitive), Ordering::Equal);
    }

    #[test]
    fn insensitive_compare_ignores_the_capital_letters_block() {
        // The whole point: byte order sorts every A-Z before every a-z, so a
        // file list ordered by `str::cmp` puts `Zebra` before `apple`.
        assert_eq!(compare("Zebra", "apple", Case::Sensitive), Ordering::Less);
        assert_eq!(
            compare("Zebra", "apple", Case::Insensitive),
            Ordering::Greater
        );
    }

    #[test]
    fn insensitive_compare_agrees_with_match_at_about_equality() {
        // Anything `match_at` calls a whole-string match must compare Equal,
        // or a filtered list and a sorted list disagree about the same pair.
        for (a, b) in [("README", "readme"), ("\u{130}", "i\u{307}"), ("ß", "ß")] {
            assert_eq!(
                match_at(a, 0, b, Case::Insensitive),
                Some(a.len()),
                "{a:?} should match {b:?} whole"
            );
            assert_eq!(
                compare(a, b, Case::Insensitive),
                Ordering::Equal,
                "{a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn insensitive_compare_does_not_fold_by_truncation() {
        // A fold that changes length must not shorten the comparison: `İ`
        // folds to two characters, so `İa` vs `i\u{307}b` must be decided by
        // the `a`/`b` that follow, not by a length mismatch.
        assert_eq!(
            compare("\u{130}a", "i\u{307}b", Case::Insensitive),
            Ordering::Less
        );
        assert_eq!(
            compare("\u{130}b", "i\u{307}a", Case::Insensitive),
            Ordering::Greater
        );
    }

    #[test]
    fn a_prefix_sorts_before_the_string_it_prefixes() {
        assert_eq!(compare("read", "readme", Case::Insensitive), Ordering::Less);
        assert_eq!(compare("READ", "readme", Case::Insensitive), Ordering::Less);
        assert_eq!(compare("", "a", Case::Insensitive), Ordering::Less);
        assert_eq!(compare("", "", Case::Insensitive), Ordering::Equal);
    }

    #[test]
    fn compare_is_a_total_order_over_a_mixed_sample() {
        // Antisymmetry and transitivity, checked exhaustively over a sample
        // chosen to include folds that change length and characters that fold
        // together. A comparator that fails either makes `sort_by` behave
        // arbitrarily rather than merely wrongly.
        let sample = [
            "",
            "a",
            "A",
            "aa",
            "ab",
            "B",
            "b",
            "READ",
            "read",
            "readme",
            "\u{130}",
            "i\u{307}",
            "ı",
            "ß",
            "日本",
            "\u{130}a",
            "i\u{307}b",
        ];
        for case in [Case::Sensitive, Case::Insensitive] {
            for x in sample {
                for y in sample {
                    assert_eq!(
                        compare(x, y, case),
                        compare(y, x, case).reverse(),
                        "antisymmetry: {x:?} {y:?} {case:?}"
                    );
                    for z in sample {
                        let (xy, yz) = (compare(x, y, case), compare(y, z, case));
                        if xy == yz && xy != Ordering::Equal {
                            assert_eq!(
                                compare(x, z, case),
                                xy,
                                "transitivity: {x:?} {y:?} {z:?} {case:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // fuzzy_score
    // ------------------------------------------------------------------

    #[test]
    fn a_fuzzy_match_needs_every_query_character_in_order() {
        assert!(fuzzy_score("fx", "Firefox").is_some());
        assert!(fuzzy_score("frfx", "Firefox").is_some());
        // Present, but in the wrong order.
        assert_eq!(fuzzy_score("xf", "Firefox"), None);
        // Absent.
        assert_eq!(fuzzy_score("z", "Firefox"), None);
        // Longer than the target can supply.
        assert_eq!(fuzzy_score("abcdef", "abc"), None);
    }

    #[test]
    fn an_empty_query_matches_everything_at_the_bottom_of_the_ranking() {
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert_eq!(fuzzy_score("", ""), Some(0));
    }

    #[test]
    fn the_ranking_prefers_prefix_then_boundary_then_early_then_short() {
        let prefix = fuzzy_score("fi", "file").unwrap();
        let middle = fuzzy_score("fi", "wifi").unwrap();
        assert!(prefix > middle, "prefix {prefix} vs middle {middle}");

        let boundary = fuzzy_score("c", "ab_cd").unwrap();
        let inner = fuzzy_score("c", "abxcd").unwrap();
        assert!(boundary > inner, "boundary {boundary} vs inner {inner}");

        let early = fuzzy_score("x", "xaaaa").unwrap();
        let late = fuzzy_score("x", "aaaax").unwrap();
        assert!(early > late, "early {early} vs late {late}");

        let tight = fuzzy_score("abc", "abcd").unwrap();
        let loose = fuzzy_score("abc", "abcdefghijkl").unwrap();
        assert!(tight > loose, "tight {tight} vs loose {loose}");
    }

    #[test]
    fn adjacent_matches_score_above_scattered_ones() {
        // Same characters, same first-match index, same target length: the
        // only difference is that one run is contiguous.
        let run = fuzzy_score("ab", "zzabzz").unwrap();
        let split = fuzzy_score("ab", "zzazb").unwrap();
        assert!(run > split, "run {run} vs split {split}");
    }

    #[test]
    fn fuzzy_matching_ignores_case_the_same_way_the_rest_of_the_crate_does() {
        assert_eq!(fuzzy_score("ABC", "abcdef"), fuzzy_score("abc", "ABCDEF"));
        // The three copies this replaces folded only ASCII, so a non-ASCII
        // query character matched nothing but itself.
        assert!(fuzzy_score("\u{c9}", "\u{e9}clair").is_some());
        assert!(fuzzy_score("\u{c9}", "eclair").is_none());
    }

    #[test]
    fn a_score_never_overflows_on_a_long_target() {
        // 10 per boundary match on a target that is nothing but boundaries.
        let mut target = alloc::string::String::new();
        for _ in 0..5000 {
            target.push_str("a ");
        }
        let query: alloc::string::String = core::iter::repeat_n('a', 5000).collect();
        assert!(fuzzy_score(&query, &target).is_some());
    }

    #[test]
    fn a_query_that_runs_out_stops_scanning_the_rest_of_the_target() {
        // Nothing after the last matched character may contribute: these two
        // differ only past the match, so they must score alike.
        assert_eq!(fuzzy_score("ab", "ab!!"), fuzzy_score("ab", "ab??"));
    }
}

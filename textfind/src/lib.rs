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
    use super::{Case, contains, find_from, match_at, matches};
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
        assert_eq!(match_at(DOTTED_I, 0, "i\u{307}", Case::Insensitive), Some(2));
        // And the other way round: two-byte needle, three-byte match.
        assert_eq!(match_at("i\u{307}", 0, DOTTED_I, Case::Insensitive), Some(3));
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
            "", "a", "aaaa", "İ", "İİİ", "xİy", "日本日本", "AaAa", "İstanbul", "straße",
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
}

//! Which writing system a stretch of text belongs to.
//!
//! A font's `GSUB` and `GPOS` features are filed under a *script* tag, and the
//! same tag may appear more than once — a face that supports both Arabic and
//! Latin registers a `liga` for each, and they mean entirely different things.
//! Applying every `liga` in the file to every run, which is what this crate
//! did before, means a Latin word can be rewritten by a rule written for
//! Arabic. `ebrima.ttf` on the development host has exactly such a rule, so
//! this is not hypothetical.
//!
//! Picking the right features starts with knowing the script, and that is what
//! this module answers. It does two things:
//!
//! * [`ScriptTags::of`] maps one character to the OpenType script tag(s) a font would
//!   file its features under.
//! * [`runs`] splits a piece list into maximal stretches of one script *and
//!   one direction*, which is the unit substitution has to be applied over.
//!   Splitting matters because the alternative — one script for the whole
//!   string, which is what HarfBuzz's `guess_segment_properties` does —
//!   silently applies Latin rules to the Arabic half of a mixed string.
//!
//! # Characters with no script of their own
//!
//! Digits, spaces and most punctuation are `Common`; combining marks are
//! usually `Inherited`. Neither selects a script — they take the script of
//! the text around them, and they are the reason this is a resolution rather
//! than a lookup. A run splitter that started a new run at every space would
//! break every ligature that spans one, and one that gave spaces their own
//! script would shatter ordinary English into alternating fragments.
//!
//! The rule here is the simple one that UAX #24 calls the starting point:
//! a scriptless character extends whatever run is open, and a scriptless
//! *prefix* joins the first real script that appears after it. Text that is
//! entirely scriptless — `"123 456"` — is one run with no tag, and the caller
//! falls back to the font's default features.
//!
//! This is deliberately not the full UAX #24 algorithm, which resolves through
//! `Script_Extensions` so that a character shared between scripts (a Devanagari
//! danda also used by Bengali, say) joins the run it is actually adjacent to
//! rather than the first one. That refinement only changes the answer for
//! characters that are already ambiguous, and only at run boundaries; it is
//! noted in `known-issues.md` rather than guessed at here.

use alloc::vec::Vec;

use crate::bidi::Level;
use crate::norm::Piece;
use crate::script_tables::{SCRIPT_RANGES, SCRIPT_TAGS};

/// The OpenType script tags one script may be registered under, preferred
/// first.
///
/// Two, because OpenType revised the Indic tags: a font may file Devanagari
/// features under `dev2` or under `deva`, and a shaper tries the newer
/// spelling first. Scripts with a single tag repeat it, so a caller can always
/// try both without asking how many there are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptTags {
    /// The tag to try first.
    pub preferred: [u8; 4],
    /// The tag to try if the font does not register the preferred one. Equal
    /// to `preferred` when the script has only one spelling.
    pub fallback: [u8; 4],
}

impl ScriptTags {
    /// The script tags for `ch`, or `None` when it has no script of its own.
    ///
    /// `None` is the answer for `Common`, `Inherited` and `Unknown` — spaces,
    /// digits, punctuation and combining marks. It means "this character does
    /// not decide", not "this character has no script".
    #[must_use]
    pub fn of(ch: char) -> Option<Self> {
        let cp = ch as u32;
        // The ranges are disjoint and sorted, so the last one starting at or
        // before `cp` is the only one that can contain it.
        let i = SCRIPT_RANGES
            .partition_point(|&(lo, _, _)| lo <= cp)
            .checked_sub(1)?;
        let &(_, hi, index) = SCRIPT_RANGES.get(i)?;
        if cp > hi {
            return None;
        }
        let &(preferred, fallback) = SCRIPT_TAGS.get(usize::from(index))?;
        Some(Self {
            preferred,
            fallback,
        })
    }

    /// Both spellings of one OpenType tag, for a tag that came from a font
    /// rather than from a character.
    ///
    /// A tag read out of a font's own ScriptList is already the exact spelling
    /// that font uses, so there is nothing to fall back to.
    pub(crate) fn exactly(tag: [u8; 4]) -> Self {
        Self {
            preferred: tag,
            fallback: tag,
        }
    }
}

/// Split `pieces` into maximal stretches of one script and one direction.
///
/// Each entry is `(end, tags)`, where `end` is the index one past the last
/// piece of the run: run *n* covers `pieces[prev_end..end]`. Ends rather than
/// ranges because that is what the caller slices with, and because it makes
/// the "runs partition the input" invariant impossible to state wrongly.
///
/// `levels` is one bidi embedding level per piece, or empty for text that
/// needs no bidi at all. Only the *parity* of a level is a boundary here, not
/// the level itself: a digit inside English is raised to level 2 by rule I1
/// and still reads left to right, so splitting the run there would refuse a
/// ligature or a contextual form for no reason. A change of parity is a real
/// boundary — the two sides are drawn in opposite directions, and a
/// substitution that reached across one would join glyphs that end up at
/// opposite ends of the line.
///
/// The result always covers the whole of `pieces` and is never empty for
/// non-empty input. `tags` is `None` for a run with no script at all, which
/// happens only when the entire input is scriptless.
#[must_use]
pub(crate) fn runs(pieces: &[Piece], levels: &[Level]) -> Vec<(usize, Option<ScriptTags>)> {
    let mut out: Vec<(usize, Option<ScriptTags>)> = Vec::new();
    if pieces.is_empty() {
        return out;
    }
    let rtl = |i: usize| levels.get(i).is_some_and(|l| !l.is_multiple_of(2));
    let mut current: Option<ScriptTags> = None;
    let mut direction = rtl(0);
    for (i, &(ch, _)) in pieces.iter().enumerate() {
        // A direction change closes the run whatever the character is, since
        // even a space belongs to one side or the other once the two sides
        // read opposite ways.
        if rtl(i) != direction {
            out.push((i, current));
            current = None;
            direction = rtl(i);
        }
        let Some(found) = ScriptTags::of(ch) else {
            // Scriptless: extends whatever is open, whether or not anything
            // is. This is what keeps `"a b"` one run and `"1 2"` one run.
            continue;
        };
        match current {
            // The first real script claims everything scriptless before it,
            // so a leading quote or space belongs to the word it introduces.
            None => current = Some(found),
            Some(open) if open == found => {}
            Some(_) => {
                out.push((i, current));
                current = Some(found);
            }
        }
    }
    out.push((pieces.len(), current));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn pieces(text: &str) -> Vec<Piece> {
        text.char_indices().map(|(at, ch)| (ch, at)).collect()
    }

    fn tags_of(text: &str) -> Vec<(usize, Option<[u8; 4]>)> {
        tags_at(text, &[])
    }

    fn tags_at(text: &str, levels: &[Level]) -> Vec<(usize, Option<[u8; 4]>)> {
        runs(&pieces(text), levels)
            .into_iter()
            .map(|(end, t)| (end, t.map(|t| t.preferred)))
            .collect()
    }

    #[test]
    fn letters_report_their_script() {
        assert_eq!(ScriptTags::of('a').map(|t| t.preferred), Some(*b"latn"));
        assert_eq!(ScriptTags::of('\u{5d0}').map(|t| t.preferred), Some(*b"hebr"));
        assert_eq!(ScriptTags::of('\u{627}').map(|t| t.preferred), Some(*b"arab"));
        assert_eq!(ScriptTags::of('\u{4e00}').map(|t| t.preferred), Some(*b"hani"));
    }

    /// The whole reason the fallback field exists: OpenType revised the Indic
    /// tags, and a font may use either spelling.
    #[test]
    fn indic_scripts_carry_both_openttype_spellings() {
        assert_eq!(
            ScriptTags::of('\u{905}').map(|t| (t.preferred, t.fallback)),
            Some((*b"dev2", *b"deva"))
        );
        // A script with one spelling repeats it rather than leaving a hole.
        assert_eq!(
            ScriptTags::of('a').map(|t| (t.preferred, t.fallback)),
            Some((*b"latn", *b"latn"))
        );
    }

    /// Spaces, digits and punctuation must not decide anything, or every
    /// English sentence would be shattered into fragments.
    #[test]
    fn scriptless_characters_do_not_decide() {
        for ch in [' ', '1', '.', ',', '\t', '\u{301}'] {
            assert_eq!(ScriptTags::of(ch), None, "{ch:?} should not select a script");
        }
        assert_eq!(tags_of("hello there, world"), vec![(18, Some(*b"latn"))]);
    }

    /// Gaps in the range table are misses, not the previous range continuing.
    #[test]
    fn a_gap_between_ranges_is_not_a_script() {
        // U+005B..U+0060 sit between the two Latin letter ranges.
        assert_eq!(ScriptTags::of('['), None);
        assert_eq!(ScriptTags::of('_'), None);
        assert_eq!(ScriptTags::of('A'), ScriptTags::of('z'));
    }

    #[test]
    fn a_mixed_string_splits_at_the_boundary() {
        let out = tags_of("ab\u{5d0}\u{5d1}");
        assert_eq!(out, vec![(2, Some(*b"latn")), (4, Some(*b"hebr"))]);
    }

    /// A scriptless prefix joins what follows it, so a leading quote or space
    /// is substituted together with the word it introduces.
    #[test]
    fn a_scriptless_prefix_joins_the_script_after_it() {
        assert_eq!(tags_of("  ab"), vec![(4, Some(*b"latn"))]);
        // Ends count *pieces*, not bytes: five characters here, the first
        // four of them Latin-or-neutral.
        assert_eq!(
            tags_of("\"a\" \u{5d0}"),
            vec![(4, Some(*b"latn")), (5, Some(*b"hebr"))]
        );
    }

    /// A scriptless character *between* two scripts joins the earlier one,
    /// which is what keeps the space in `"a א"` out of the Hebrew run.
    #[test]
    fn a_scriptless_gap_joins_the_run_before_it() {
        assert_eq!(
            tags_of("a \u{5d0}"),
            vec![(2, Some(*b"latn")), (3, Some(*b"hebr"))]
        );
    }

    /// Entirely scriptless text is one run that names no script, and the
    /// caller falls back to the font's default features.
    #[test]
    fn entirely_scriptless_text_is_one_untagged_run() {
        assert_eq!(tags_of("123 456"), vec![(7, None)]);
        assert_eq!(tags_of(""), vec![]);
    }

    /// The runs must partition the input exactly, whatever the input is:
    /// every piece in exactly one run, in order, none lost or repeated.
    #[test]
    fn runs_always_partition_the_input() {
        for text in [
            "",
            "a",
            " ",
            "a\u{5d0}a",
            "  \u{627}\u{628} 12 ab",
            "\u{4e00}\u{3042}\u{ac00}",
            "e\u{301} \u{5d0}\u{5b0}",
        ] {
            let p = pieces(text);
            let out = runs(&p, &[]);
            assert_eq!(out.is_empty(), p.is_empty(), "{text:?}");
            let mut prev = 0usize;
            for &(end, _) in &out {
                assert!(end > prev, "{text:?}: run ending {end} is empty");
                prev = end;
            }
            assert_eq!(prev, p.len(), "{text:?}: runs do not cover the input");
        }
    }

    /// A change of *direction* is a boundary even when the script does not
    /// change — an English quotation inside an Arabic sentence is Latin on
    /// both sides of nothing, but the two halves are drawn opposite ways.
    #[test]
    fn a_direction_change_splits_a_run_that_the_script_would_not() {
        // "ab cd" with the middle three pieces right-to-left.
        let levels = [0, 0, 1, 1, 1, 0, 0];
        assert_eq!(
            tags_at("ab cd x", &levels),
            vec![(2, Some(*b"latn")), (5, Some(*b"latn")), (7, Some(*b"latn"))]
        );
    }

    /// But a change of *level* alone is not: rule I1 raises a digit inside
    /// English to level 2, which still reads left to right. Splitting there
    /// would refuse a ligature or a contextual form for no reason at all.
    #[test]
    fn a_level_change_that_keeps_the_direction_is_not_a_boundary() {
        assert_eq!(tags_at("ab12cd", &[0, 0, 2, 2, 0, 0]), vec![(6, Some(*b"latn"))]);
    }

    /// An empty level list means "no bidi here", not "level zero everywhere
    /// except where the slice ran out" — the fast path must not invent a
    /// boundary at the end of it.
    #[test]
    fn no_levels_at_all_is_one_direction() {
        assert_eq!(tags_at("ab cd", &[]), tags_of("ab cd"));
    }

    #[test]
    fn the_generated_ranges_are_sorted_and_disjoint() {
        assert!(
            SCRIPT_RANGES.is_sorted_by(|a, b| a.1 < b.0),
            "SCRIPT_RANGES overlap or are out of order"
        );
        for &(lo, hi, index) in SCRIPT_RANGES.iter() {
            assert!(lo <= hi, "range 0x{lo:04X}..0x{hi:04X} is inverted");
            assert!(
                SCRIPT_TAGS.get(usize::from(index)).is_some(),
                "range 0x{lo:04X} names tag row {index}, which does not exist"
            );
        }
    }
}

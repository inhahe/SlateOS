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
//! A character with no script at all extends whatever run is open, and a
//! scriptless *prefix* joins the first real script that appears after it. Text
//! that is entirely scriptless — `"123 456"` — is one run with no tag, and the
//! caller falls back to the font's default features.
//!
//! The one thing that overrides this is direction. Such a character belongs to
//! the directional run it is drawn in, so where the text turns around
//! underneath it — the space in `"hello שלום world"` — it joins the run that
//! starts after it rather than the one that ended before it. Only a character
//! with *no* script is moved this way; one that merely shares its scripts with
//! its neighbours, like the Arabic-Indic digit in a Thaana word, keeps them
//! across the turn.
//!
//! # Characters shared between scripts
//!
//! Between "belongs to one script" and "belongs to none" sits a third case,
//! and it is the one UAX #24 is actually about. U+0964 DEVANAGARI DANDA is
//! `Common`, but it is not neutral: twenty-one Brahmic scripts use it and
//! nothing else does. U+0660 ARABIC-INDIC DIGIT ZERO is not even `Common` —
//! its script is Arabic — yet Thaana and Yezidi write their numbers with it.
//! Unicode records this as `Script_Extensions`, the set of scripts a character
//! is *used by*, as against the one it belongs to.
//!
//! [`runs`] resolves through those sets, which is UAX #24's rule rather than
//! the simplified one this module started with. Each run carries the
//! intersection of the sets of the characters in it, and a character narrows
//! the open run whenever the intersection is non-empty. So an Arabic-Indic
//! digit inside a Thaana word leaves `{Thaana}` standing rather than cutting
//! the word in three and asking a Thaana face for Arabic features, and a danda
//! between two Bengali words resolves to Bengali rather than to Devanagari.
//!
//! When the intersection is *empty*, what happens depends on whether the
//! character has a `Script` of its own:
//!
//! * One that does — U+0660, whose script is Arabic — ends the run and starts
//!   a new one. It has a script; it is entitled to insist on it.
//! * One that does not — a danda, a tatweel, any combining mark — leaves the
//!   run alone and joins it unchanged. Such a character has an *affinity*,
//!   not an identity, and an affinity may refine a decision but must not make
//!   one. This is not a nicety: U+0301 COMBINING ACUTE is used by eight
//!   scripts and Hebrew is not among them, and a mark that started a run of
//!   its own would be a mark whose base's `GPOS` never gets to attach it.
//!
//! A run that is ambiguous from end to end — a lone tatweel, a danda with no
//! letters near it — still has to name one script. It names the first
//! surviving member of its set, which the generated table orders with the
//! character's own script first and the rest earliest-encoded first. That
//! makes a lone tatweel Arabic and a lone danda Devanagari. There is no better
//! answer available: nothing in the text says which was meant.
//!
//! [`ScriptTags::of`] is unaffected and still reports the `Script` property,
//! since a single character out of context has no run to be resolved against.

use alloc::vec::Vec;

use crate::bidi::Level;
use crate::norm::Piece;
use crate::script_tables::{
    SCRIPT_EXT_POOL, SCRIPT_EXT_RANGES, SCRIPT_RANGES, SCRIPT_TAGS, WIDEST_EXTENSION,
};

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
        let &(_, hi, index) = range_containing(&SCRIPT_RANGES, cp, |r| r.0)?;
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

/// The scripts one character is used by: its `Script_Extensions`, as rows of
/// `SCRIPT_TAGS`, most-preferred first.
///
/// Never empty — a character that is used by no script at all is represented
/// by `None` rather than by an empty set, so that "extends whatever is open"
/// and "narrows what is open" cannot be confused for one another.
///
/// Inline rather than a slice into the pool because [`intersect`] produces
/// sets that are in no table: `{Devanagari, Bengali}` is what is left of a
/// danda that has met a Bengali letter, and nothing in the file spells it.
///
/// [`intersect`]: ScriptSet::intersect
#[derive(Clone, Copy, Debug)]
struct ScriptSet {
    rows: [u16; WIDEST_EXTENSION],
    len: usize,
}

impl ScriptSet {
    /// The scripts `ch` is used by, or `None` when it is used by none.
    ///
    /// `None` is the answer for a space or an ordinary quotation mark, which
    /// belong to every script and therefore decide nothing. It is *not* the
    /// answer for a danda or a combining Latin letter: those are `Common` and
    /// `Inherited` respectively, and both are used by a definite set of
    /// scripts, which is exactly what `Script_Extensions` records.
    fn of(ch: char) -> Option<Self> {
        let cp = ch as u32;
        if let Some(&(_, hi, at, count)) = range_containing(&SCRIPT_EXT_RANGES, cp, |r| r.0)
            && cp <= hi
        {
            let rows = SCRIPT_EXT_POOL
                .get(usize::from(at)..)?
                .get(..usize::from(count))?;
            return Self::from_rows(rows.iter().copied());
        }
        let &(_, hi, row) = range_containing(&SCRIPT_RANGES, cp, |r| r.0)?;
        if cp > hi {
            return None;
        }
        Self::from_rows(core::iter::once(row))
    }

    fn from_rows(source: impl Iterator<Item = u16>) -> Option<Self> {
        let mut rows = [0u16; WIDEST_EXTENSION];
        let mut len = 0usize;
        for (slot, row) in rows.iter_mut().zip(source) {
            *slot = row;
            len = len.saturating_add(1);
        }
        (len > 0).then_some(Self { rows, len })
    }

    fn members(&self) -> impl Iterator<Item = u16> + '_ {
        self.rows.iter().copied().take(self.len)
    }

    /// The scripts both sets are used by, or `None` when they share none.
    ///
    /// Order is taken from `self`, which is the open run: a run that has
    /// already been narrowed to a preference keeps it, and a new character
    /// can only remove candidates, never promote one.
    fn intersect(&self, other: &Self) -> Option<Self> {
        let mut rows = [0u16; WIDEST_EXTENSION];
        let mut len = 0usize;
        for row in self.members() {
            if other.members().any(|r| r == row) {
                if let Some(slot) = rows.get_mut(len) {
                    *slot = row;
                }
                len = len.saturating_add(1);
            }
        }
        (len > 0).then_some(Self { rows, len })
    }

    /// The OpenType tags for the set's most-preferred surviving script.
    fn tags(&self) -> Option<ScriptTags> {
        let row = self.rows.first()?;
        let &(preferred, fallback) = SCRIPT_TAGS.get(usize::from(*row))?;
        Some(ScriptTags {
            preferred,
            fallback,
        })
    }
}

/// The last range starting at or before `cp`, whatever the range tuple's
/// shape. Ranges are sorted and disjoint, so it is the only one that can
/// contain `cp` — the caller still has to check it does.
fn range_containing<T>(ranges: &[T], cp: u32, first: impl Fn(&T) -> u32) -> Option<&T> {
    let i = ranges.partition_point(|r| first(r) <= cp).checked_sub(1)?;
    ranges.get(i)
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
///
/// # Why the two passes are separate
///
/// Script is resolved over the whole piece list first, by [`by_script`], and
/// the direction boundaries are cut into that answer afterwards. Doing it in
/// one pass — closing the open script when the direction turns — loses
/// information the second half of the text needs, and the loss is not
/// theoretical: in `"ހ٠ހ"`, Thaana with an Arabic-Indic digit in it, the digit
/// is an `AN` and bidi rule I2 raises it to an even level inside odd-level
/// Thaana. It is therefore its own directional run, and a splitter that
/// re-derived the script there would find nothing but the digit, resolve it to
/// Arabic, and shape it with a face's Arabic `locl` in the middle of a Thaana
/// word. Resolved over the text and *then* cut, all three pieces are Thaana,
/// which is what the surrounding letters say and what HarfBuzz answers.
#[must_use]
pub(crate) fn runs(pieces: &[Piece], levels: &[Level]) -> Vec<(usize, Option<ScriptTags>)> {
    let mut out: Vec<(usize, Option<ScriptTags>)> = Vec::new();
    if pieces.is_empty() {
        return out;
    }
    let rtl = |i: usize| levels.get(i).is_some_and(|l| !l.is_multiple_of(2));
    let bounds = by_script(pieces, levels);
    let tag = |at: usize| bounds.get(at).and_then(|&(_, tags)| tags);
    let mut at = 0usize;
    let mut direction = rtl(0);
    for i in 1..pieces.len() {
        // At most one script run can end here, and a direction change is a
        // boundary whatever the character is: even a space belongs to one
        // side or the other once the two sides read opposite ways.
        let ends = bounds.get(at).is_some_and(|&(end, _)| end == i);
        if !ends && rtl(i) == direction {
            continue;
        }
        out.push((i, tag(at)));
        if ends {
            at = at.saturating_add(1);
        }
        direction = rtl(i);
    }
    out.push((pieces.len(), tag(at)));
    out
}

/// Split `pieces` into maximal stretches of one script.
///
/// The first of [`runs`]' two passes; see its documentation for why the two
/// are not one. Each entry is `(end, tags)` on the same terms.
///
/// Direction is not a boundary here — that is the second pass's job — with one
/// exception, described at the character that needs it: a character used by no
/// script at all takes its script from its neighbours, and when its neighbours
/// read in opposite directions there is only one of them it can have come from.
fn by_script(pieces: &[Piece], levels: &[Level]) -> Vec<(usize, Option<ScriptTags>)> {
    let mut out: Vec<(usize, Option<ScriptTags>)> = Vec::new();
    let mut current: Option<ScriptSet> = None;
    let rtl = |i: usize| levels.get(i).is_some_and(|l| !l.is_multiple_of(2));
    for (i, &(ch, _)) in pieces.iter().enumerate() {
        let Some(found) = ScriptSet::of(ch) else {
            // Used by no script at all: extends whatever is open, whether or
            // not anything is. This keeps `"a b"` one run and `"1 2"` one run.
            //
            // Unless the text turns around underneath it. Such a character
            // belongs to the directional run it is drawn in, and at a change
            // of direction that is the *following* run, not the one that just
            // ended: the space in `"hello שלום world"` is drawn left to right
            // with `world`, so it must carry Latin into the run `world` opens
            // rather than trail Hebrew behind it. Left with the Hebrew, the
            // second pass would cut it off into a one-glyph run of its own and
            // the kern between it and `w` would never be looked for.
            let turned = i.checked_sub(1).is_some_and(|prev| rtl(i) != rtl(prev));
            if turned {
                if let Some(open) = &current {
                    out.push((i, open.tags()));
                }
                current = None;
            }
            continue;
        };
        match &current {
            // The first character with a script claims everything before it,
            // so a leading quote or space belongs to the word it introduces.
            None => current = Some(found),
            // Sharing at least one script narrows the run rather than ending
            // it, which is what keeps an Arabic-Indic digit inside the Thaana
            // around it.
            Some(open) => match open.intersect(&found) {
                Some(both) => current = Some(both),
                // A character with no `Script` of its own has an affinity,
                // not an identity: it may narrow a run and may never end one.
                // That is what keeps a combining mark in the run of the base
                // it attaches to — U+0301 is used by eight scripts and Hebrew
                // is not among them, and a mark in a run of its own is a mark
                // its base's `GPOS` never gets to attach.
                None if ScriptTags::of(ch).is_none() => {}
                None => {
                    out.push((i, open.tags()));
                    current = Some(found);
                }
            },
        }
    }
    out.push((pieces.len(), current.as_ref().and_then(ScriptSet::tags)));
    out
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing)]
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

    /// A direction boundary cuts the script runs; it does not re-derive them.
    /// This is the string that tells the two apart: bidi rule I2 raises the
    /// `AN` digit to an even level inside odd-level Thaana, so it is its own
    /// directional run — and a splitter that resolved the script inside that
    /// run would see one Arabic-Indic digit and call it Arabic.
    #[test]
    fn a_direction_boundary_does_not_re_resolve_the_script() {
        assert_eq!(
            tags_at("\u{780}\u{660}\u{780}", &[1, 2, 1]),
            vec![
                (1, Some(*b"thaa")),
                (2, Some(*b"thaa")),
                (3, Some(*b"thaa"))
            ]
        );
    }

    /// A space between two scripts has no script of its own, so which run it
    /// lands in is decided by direction: it is drawn with what follows it, and
    /// leaving it with what precedes it would strand it in a one-glyph run
    /// once the direction boundary is cut, losing the kern with the word it
    /// actually sits against.
    #[test]
    fn a_space_at_a_direction_change_joins_the_run_after_it() {
        let levels = [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            tags_at("hello \u{5d0}\u{5dc}\u{5d5}\u{5dd} world", &levels),
            vec![(6, Some(*b"latn")), (10, Some(*b"hebr")), (16, Some(*b"latn"))]
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

    /// The case UAX #24 exists for: U+0660 ARABIC-INDIC DIGIT ZERO *is*
    /// Arabic, but Thaana writes its numbers with it. Resolving on `Script`
    /// alone cut a Thaana word into three runs at every digit — and the
    /// middle one asked the font for Arabic features, which a Thaana face
    /// need not even register.
    #[test]
    fn a_shared_digit_does_not_cut_the_word_around_it() {
        assert_eq!(ScriptTags::of('\u{660}').map(|t| t.preferred), Some(*b"arab"));
        assert_eq!(
            tags_of("\u{780}\u{660}\u{780}"),
            vec![(3, Some(*b"thaa"))]
        );
        // And the same digit inside Arabic is still Arabic.
        assert_eq!(tags_of("\u{627}\u{660}\u{627}"), vec![(3, Some(*b"arab"))]);
    }

    /// A danda is `Common`, so it narrows a run but never ends one. Between
    /// two Bengali words it settles on Bengali; against Latin, which does not
    /// use it at all, it stays where it is rather than starting a new run.
    #[test]
    fn a_danda_narrows_a_run_but_never_ends_one() {
        assert_eq!(tags_of("\u{995}\u{964}\u{995}"), vec![(3, Some(*b"bng2"))]);
        // A danda opening a run does pick the script that follows it.
        assert_eq!(tags_of("\u{964}\u{995}"), vec![(2, Some(*b"bng2"))]);
        // And one stranded in Latin is Latin, not a run of its own.
        assert_eq!(
            tags_of("a\u{964}\u{995}"),
            vec![(2, Some(*b"latn")), (3, Some(*b"bng2"))]
        );
    }

    /// A run that never meets a character that pins it down still has to name
    /// one script. It names the first of its set, which is its own `Script`
    /// if it has one and the earliest-encoded candidate otherwise.
    #[test]
    fn an_ambiguous_run_names_its_first_candidate() {
        // Tatweel is `Common`, used by nine scripts; Arabic is the earliest.
        assert_eq!(ScriptTags::of('\u{640}'), None);
        assert_eq!(tags_of("\u{640}"), vec![(1, Some(*b"arab"))]);
        // A danda alone is Devanagari, which is where it is encoded.
        assert_eq!(tags_of("\u{964}"), vec![(1, Some(*b"dev2"))]);
        // But one letter of Syriac is enough to settle the tatweel.
        assert_eq!(tags_of("\u{710}\u{640}"), vec![(2, Some(*b"syrc"))]);
        assert_eq!(tags_of("\u{640}\u{710}"), vec![(2, Some(*b"syrc"))]);
    }

    /// Sharing *some* script is what merges runs; a character with a script
    /// of its own that shares none still splits them, however wide its set.
    #[test]
    fn a_real_script_that_shares_nothing_still_splits() {
        // U+0660 is Arabic and is used by three scripts; Bengali is not one.
        assert_eq!(
            tags_of("\u{660}\u{995}"),
            vec![(1, Some(*b"arab")), (2, Some(*b"bng2"))]
        );
        // Whereas two scriptless characters that share nothing do not split:
        // neither of them is entitled to end the other's run.
        assert_eq!(tags_of("\u{640}\u{964}"), vec![(2, Some(*b"arab"))]);
    }

    /// Two Unicode scripts that OpenType files under one tag are one script
    /// here. Hiragana and Katakana are both `kana`, so Japanese must not be
    /// cut at every change between them — the font could not act on the cut.
    #[test]
    fn hiragana_and_katakana_are_one_run() {
        assert_eq!(
            tags_of("\u{3042}\u{30fc}\u{30a2}"),
            vec![(3, Some(*b"kana"))]
        );
    }

    /// A combining mark is `Inherited`, but many are used by a definite set
    /// of scripts, and one that is used by only Latin really is Latin.
    #[test]
    fn a_combining_mark_may_still_name_a_script() {
        // U+0363 COMBINING LATIN SMALL LETTER A: `Inherited`, but Latin-only.
        assert_eq!(ScriptTags::of('\u{363}'), None);
        assert_eq!(tags_of("\u{363}"), vec![(1, Some(*b"latn"))]);
        // U+0301 COMBINING ACUTE is used by eight scripts, so it settles
        // nothing and lets the letter before it decide.
        assert_eq!(tags_of("e\u{301}"), vec![(2, Some(*b"latn"))]);
        // Hebrew is not one of those eight, and the mark still stays with
        // its base: a mark in a run of its own could never be attached.
        assert_eq!(tags_of("\u{5d0}\u{301}"), vec![(2, Some(*b"hebr"))]);
    }

    #[test]
    fn the_generated_extension_table_is_well_formed() {
        assert!(
            SCRIPT_EXT_RANGES.is_sorted_by(|a, b| a.1 < b.0),
            "SCRIPT_EXT_RANGES overlap or are out of order"
        );
        for &(lo, hi, at, count) in SCRIPT_EXT_RANGES.iter() {
            assert!(lo <= hi, "range 0x{lo:04X}..0x{hi:04X} is inverted");
            assert!(count >= 1, "range 0x{lo:04X} names an empty set");
            assert!(
                usize::from(count) <= WIDEST_EXTENSION,
                "range 0x{lo:04X} has {count} scripts, over the {WIDEST_EXTENSION} bound"
            );
            let rows = &SCRIPT_EXT_POOL[usize::from(at)..][..usize::from(count)];
            for &row in rows {
                assert!(
                    SCRIPT_TAGS.get(usize::from(row)).is_some(),
                    "range 0x{lo:04X} names tag row {row}, which does not exist"
                );
            }
            let mut seen = rows.to_vec();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), rows.len(), "range 0x{lo:04X} repeats a script");
        }
    }

    /// A set never grows, so a run's script can only ever be narrowed. That
    /// is what makes the single-pass splitter correct: an intersection taken
    /// character by character is the intersection over the whole run.
    #[test]
    fn intersection_only_ever_shrinks() {
        let danda = ScriptSet::of('\u{964}').expect("danda has an extension set");
        let bengali = ScriptSet::of('\u{995}').expect("a letter has a script");
        let both = danda.intersect(&bengali).expect("they share Bengali");
        assert_eq!(both.len, 1);
        assert!(both.len <= danda.len && both.len <= bengali.len);
        assert_eq!(both.tags(), ScriptTags::of('\u{995}'));
        // Intersection is symmetric in membership even though the surviving
        // order comes from the left-hand side.
        let other = bengali.intersect(&danda).expect("still Bengali");
        assert_eq!(other.tags(), both.tags());
        // And disjoint sets report emptiness rather than an empty set.
        let latin = ScriptSet::of('a').expect("a letter has a script");
        assert!(danda.intersect(&latin).is_none());
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

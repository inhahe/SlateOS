//! Cursive joining: which of its four forms a letter takes.
//!
//! Arabic — and Syriac, Mongolian, N'Ko, and half a dozen others — is written
//! joined. A letter's shape depends on what stands next to it: `ع` alone,
//! `عـ` starting a word, `ـعـ` inside one, `ـع` ending one. A font ships all
//! four as separate glyphs and files the substitutions that reach them under
//! four features — `isol`, `init`, `medi`, `fina`. Without this pass every
//! Arabic word is drawn as a row of isolated letters, which to someone who
//! reads Arabic is close to unreadable.
//!
//! # Why this cannot be done with feature tags alone
//!
//! The other features this crate applies are *unconditional*: `liga` means
//! "wherever this lookup matches, do it". The positional four are not. Each is
//! a plain type-1 substitution whose coverage is, in a typical face, every
//! Arabic letter — so applying `fina` the way `liga` is applied would rewrite
//! every letter in the run into its final form. What makes them correct is that
//! the shaper decides, *per glyph*, which one of the four that glyph is
//! eligible for. That decision is this module; carrying it to the lookups is
//! the per-glyph mask in [`gsub`](crate::gsub).
//!
//! # The rule
//!
//! Every character has a Unicode `Joining_Type` (the table is
//! [`joining_tables`](crate::joining_tables), generated from the UCD). Two
//! questions follow from it:
//!
//! * may this character join to the one *before* it? — dual-joining,
//!   right-joining and join-causing may;
//! * may it join to the one *after*? — dual-joining, left-joining and
//!   join-causing may.
//!
//! A character joins to a neighbour only when both of them are willing, and
//! the neighbour is the nearest one that is not `Transparent`: combining marks
//! are stepped over, so an alef with a hamza above it still joins to the letter
//! before the hamza. Joined on both sides is `medi`, on the preceding side only
//! `fina`, on the following side only `init`, and neither `isol`.
//!
//! "Before" and "after" are *logical* order, not visual. Arabic is written
//! right to left, so the letter that `fina` joins back to is drawn to the
//! right — but it is the earlier one in the string, and this pass runs long
//! before anything reorders for display.
//!
//! # What is deliberately not here
//!
//! * **The alaph forms.** Syriac's alaph takes `fin2`, `fin3` and `med2` in
//!   place of the ordinary two, chosen by rules about what stands earlier in
//!   the word. Syriac faces are rare and the rules are specific to one letter;
//!   an alaph shaped with plain `fina` is wrong in the same way an unjoined
//!   letter is wrong, only much rarer.
//! * **`ZWJ`/`ZWNJ` as an override.** Zero-width joiner is `Join_Causing` and
//!   so already forces its neighbours to join, which is most of what it is for.
//!   Zero-width *non*-joiner is `Non_Joining` and so already breaks a join.
//!   Neither is given a form of its own, which is what HarfBuzz does too.
//! * **Reordering.** Indic and South-East Asian scripts need a cluster to be
//!   rearranged before features are chosen. Nothing here does that; see
//!   `TD-FONT-HAS-NO-JOINING-OR-REORDERING-SHAPER`.

use alloc::vec::Vec;

use crate::joining_tables::JOINING_RANGES;
use crate::norm::Piece;

/// The Unicode `Joining_Type` property.
///
/// The variant order is the one
/// `gui/font/tools/gen_joining_tables.py` writes, so changing it means
/// re-running the generator. [`NonJoining`](Self::NonJoining) is last because
/// it is never written to the table: it is the default, and the answer for
/// nearly every code point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JoiningType {
    /// Joins on both sides. Most Arabic letters.
    DualJoining,
    /// Joins only to the preceding letter — alef, dal, waw. Never `init` or
    /// `medi`, because nothing may join on its other side.
    RightJoining,
    /// The mirror image. No Arabic letter is one; it exists for other scripts.
    LeftJoining,
    /// Joins on both sides but has no form of its own: ZERO WIDTH JOINER. It
    /// makes its neighbours join without taking a form itself.
    JoinCausing,
    /// Combining marks and format characters, stepped over when asking what is
    /// next to what.
    Transparent,
    /// Everything else, including space — which is what breaks a word.
    NonJoining,
}

impl JoiningType {
    /// The `Joining_Type` of `ch`.
    #[must_use]
    pub(crate) fn of(ch: char) -> Self {
        let cp = ch as u32;
        // The ranges are disjoint and sorted, so the last one starting at or
        // before `cp` is the only one that can contain it.
        let Some(i) = JOINING_RANGES
            .partition_point(|&(lo, _, _)| lo <= cp)
            .checked_sub(1)
        else {
            return Self::NonJoining;
        };
        match JOINING_RANGES.get(i) {
            Some(&(_, hi, kind)) if cp <= hi => kind,
            // Past the end of that range, or a table too short to index —
            // neither is in any range, and "in no range" is the default.
            _ => Self::NonJoining,
        }
    }

    /// Whether this character may join to the one before it in logical order.
    fn joins_prev(self) -> bool {
        matches!(
            self,
            Self::DualJoining | Self::RightJoining | Self::JoinCausing
        )
    }

    /// Whether this character may join to the one after it in logical order.
    fn joins_next(self) -> bool {
        matches!(
            self,
            Self::DualJoining | Self::LeftJoining | Self::JoinCausing
        )
    }

    /// Whether this character takes a positional form at all.
    ///
    /// Join-causing is excluded on purpose: ZWJ makes its neighbours join but
    /// has no glyph, so asking a font for its `init` form is asking for
    /// something that does not exist.
    fn shapes(self) -> bool {
        matches!(
            self,
            Self::DualJoining | Self::RightJoining | Self::LeftJoining
        )
    }
}

/// Which of the four positional forms a letter takes.
///
/// Named for the OpenType features that reach them, since that is the only
/// thing done with the answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// `isol` — joined on neither side.
    Isolated,
    /// `init` — joined only to what follows.
    Initial,
    /// `medi` — joined on both sides.
    Medial,
    /// `fina` — joined only to what precedes.
    Final,
}

/// The positional form of each of `pieces`, written into `out`.
///
/// `out` comes back either empty or exactly as long as `pieces`. Empty means
/// *no character in this text joins at all*, which is every Latin, Cyrillic,
/// Greek and CJK string ever drawn — the overwhelmingly common case, and worth
/// distinguishing so the caller can skip the whole pass rather than mask every
/// glyph with a form of `None`.
///
/// `None` for a character that takes no form: a space, a digit, a combining
/// mark, a ZWJ.
pub(crate) fn forms(pieces: &[Piece], out: &mut Vec<Option<Form>>) {
    out.clear();
    // The early-out is what keeps Latin from paying for this. It costs one
    // table probe per character and saves the allocation below plus the
    // masking the caller would otherwise do.
    if !pieces.iter().any(|&(ch, _)| JoiningType::of(ch).shapes()) {
        return;
    }

    let types: Vec<JoiningType> = pieces.iter().map(|&(ch, _)| JoiningType::of(ch)).collect();
    out.reserve(types.len());
    for (i, &kind) in types.iter().enumerate() {
        if !kind.shapes() {
            out.push(None);
            continue;
        }
        // The nearest neighbour on each side that is not transparent. A mark
        // between two letters must not break their join.
        let prev = types
            .get(..i)
            .into_iter()
            .flatten()
            .rev()
            .find(|t| **t != JoiningType::Transparent);
        let next = types
            .get(i.saturating_add(1)..)
            .into_iter()
            .flatten()
            .find(|t| **t != JoiningType::Transparent);
        // Joining takes two: this character must be willing to join on that
        // side, and the neighbour willing to join back.
        let to_prev = kind.joins_prev() && prev.is_some_and(|t| t.joins_next());
        let to_next = kind.joins_next() && next.is_some_and(|t| t.joins_prev());
        out.push(Some(match (to_prev, to_next) {
            (true, true) => Form::Medial,
            (true, false) => Form::Final,
            (false, true) => Form::Initial,
            (false, false) => Form::Isolated,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn pieces(text: &str) -> Vec<Piece> {
        text.char_indices().map(|(at, ch)| (ch, at)).collect()
    }

    fn shape(text: &str) -> Vec<Option<Form>> {
        let mut out = Vec::new();
        forms(&pieces(text), &mut out);
        out
    }

    #[test]
    fn the_table_answers_for_the_letters_it_was_built_for() {
        // beh joins both ways; alef only backwards; space neither.
        assert_eq!(JoiningType::of('\u{628}'), JoiningType::DualJoining);
        assert_eq!(JoiningType::of('\u{627}'), JoiningType::RightJoining);
        assert_eq!(JoiningType::of(' '), JoiningType::NonJoining);
        assert_eq!(JoiningType::of('a'), JoiningType::NonJoining);
        // A combining mark is transparent whatever script it belongs to.
        assert_eq!(JoiningType::of('\u{64e}'), JoiningType::Transparent);
        assert_eq!(JoiningType::of('\u{301}'), JoiningType::Transparent);
        // ZWJ causes joins; ZWNJ breaks them by being non-joining.
        assert_eq!(JoiningType::of('\u{200d}'), JoiningType::JoinCausing);
        assert_eq!(JoiningType::of('\u{200c}'), JoiningType::NonJoining);
        // Above the highest range, and the first code point of all.
        assert_eq!(JoiningType::of('\u{10ffff}'), JoiningType::NonJoining);
        assert_eq!(JoiningType::of('\0'), JoiningType::NonJoining);
    }

    /// Nothing in a Latin string joins, and the caller is told so by an empty
    /// answer rather than by a vector of `None`.
    #[test]
    fn text_that_does_not_join_reports_nothing() {
        assert!(shape("Hamburgefonstiv").is_empty());
        assert!(shape("").is_empty());
        assert!(shape("\u{5e9}\u{5dc}\u{5d5}\u{5dd}").is_empty());
    }

    /// Three dual-joining letters: the classic initial-medial-final.
    #[test]
    fn a_word_of_dual_joining_letters_takes_all_three_forms() {
        // beh beh beh
        assert_eq!(
            shape("\u{628}\u{628}\u{628}"),
            vec![Some(Form::Initial), Some(Form::Medial), Some(Form::Final)]
        );
    }

    /// Alef joins backwards only, so it ends a join wherever it appears — the
    /// letter after it starts a new one.
    #[test]
    fn a_right_joining_letter_stops_the_join_dead() {
        // beh alef beh beh: the alef takes `fina`, and the beh after it
        // cannot join back to the alef, so it starts a fresh join rather than
        // continuing the old one — initial where an unbroken word would have
        // made it medial.
        assert_eq!(
            shape("\u{628}\u{627}\u{628}\u{628}"),
            vec![
                Some(Form::Initial),
                Some(Form::Final),
                Some(Form::Initial),
                Some(Form::Final),
            ]
        );
        // Standing alone it is isolated, not final.
        assert_eq!(shape("\u{627}"), vec![Some(Form::Isolated)]);
    }

    /// The reason neighbours are found by skipping rather than by index.
    #[test]
    fn a_mark_between_two_letters_does_not_break_the_join() {
        // beh + fatha + beh. The mark takes no form and is stepped over, so
        // both letters join through it.
        assert_eq!(
            shape("\u{628}\u{64e}\u{628}"),
            vec![Some(Form::Initial), None, Some(Form::Final)]
        );
    }

    /// A space is non-joining, which is what makes a word a word.
    #[test]
    fn a_space_breaks_the_word() {
        assert_eq!(
            shape("\u{628}\u{628} \u{628}\u{628}"),
            vec![
                Some(Form::Initial),
                Some(Form::Final),
                None,
                Some(Form::Initial),
                Some(Form::Final),
            ]
        );
    }

    /// ZWJ forces a join without being drawn: the letter before it is medial
    /// even though nothing follows to join to.
    #[test]
    fn zero_width_joiner_forces_a_form_without_taking_one() {
        assert_eq!(shape("\u{628}\u{200d}"), vec![Some(Form::Initial), None]);
        assert_eq!(
            shape("\u{200d}\u{628}\u{200d}"),
            vec![None, Some(Form::Medial), None]
        );
        // ZWNJ does the opposite: it is non-joining, so it separates.
        assert_eq!(
            shape("\u{628}\u{200c}\u{628}"),
            vec![Some(Form::Isolated), None, Some(Form::Isolated)]
        );
    }

    /// `العربية` — the string the HarfBuzz sweep disagrees on, spelled out.
    #[test]
    fn the_word_arabic_shapes_the_way_it_is_written() {
        // alef lam ain ra beh yeh teh-marbuta
        assert_eq!(
            shape("\u{627}\u{644}\u{639}\u{631}\u{628}\u{64a}\u{629}"),
            vec![
                // alef: right-joining, nothing before it
                Some(Form::Isolated),
                // lam: dual, joins forward to ain but not back to alef
                Some(Form::Initial),
                // ain: dual, joined both ways
                Some(Form::Medial),
                // ra: right-joining, so it ends the join
                Some(Form::Final),
                // beh: dual, cannot join back to ra
                Some(Form::Initial),
                // yeh: dual, joined both ways
                Some(Form::Medial),
                // teh marbuta: right-joining, ends the word
                Some(Form::Final),
            ]
        );
    }

    /// Every character of the table must be reachable, and the ranges must not
    /// overlap or run backwards — a generated table is exactly where such a
    /// mistake would hide.
    #[test]
    fn the_generated_ranges_are_sorted_and_disjoint() {
        let mut last: Option<u32> = None;
        for &(lo, hi, _) in &JOINING_RANGES {
            assert!(lo <= hi, "range {lo:#x}..{hi:#x} runs backwards");
            if let Some(prev) = last {
                assert!(prev < lo, "range starting {lo:#x} overlaps {prev:#x}");
            }
            last = Some(hi);
        }
    }
}

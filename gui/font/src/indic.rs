//! The Indic shaper: what each character of a Devanagari-family syllable is.
//!
//! Devanagari, Bengali, Gurmukhi, Gujarati, Oriya, Tamil, Telugu, Kannada and
//! Malayalam are written in *syllables*, and a syllable is not drawn in the
//! order it is stored. `हिन्दी` is stored as ह ि न ् द ी — consonant, vowel
//! sign I, consonant, virama, consonant, vowel sign II — but the vowel sign I
//! is drawn to the *left* of the consonant it belongs to, and the न ् द pair
//! is drawn as one stacked conjunct. A shaper that hands those six characters
//! to the font in storage order gets six glyphs standing in a row, which is
//! not the word.
//!
//! Getting it right takes three things, and this module is the first:
//!
//! 1. **A category and a position for every character** — is this a
//!    consonant, a vowel sign, a virama, a mark; and if it is drawn out of
//!    order, where does it go. That is [`Category`], [`Position`] and
//!    [`Char::of`], over the generated table in
//!    [`indic_tables`](crate::indic_tables).
//! 2. **Syllable segmentation and reordering** — cut the run into syllables,
//!    then move each character to where it is drawn.
//! 3. **Per-syllable feature application** — the basic features (`nukt`,
//!    `akhn`, `rphf`, `rkrf`, `pref`, `blwf`, `abvf`, `half`, `pstf`, `vatu`,
//!    `cjct`) are applied one at a time, each constrained to a single
//!    syllable, because a later one's input is an earlier one's output.
//!
//! # Why the properties are not used raw
//!
//! Unicode ships `Indic_Syllabic_Category` and `Indic_Positional_Category`,
//! and neither is quite what a shaper needs. The syllabic property does not
//! distinguish RA — which alone becomes a reph — from any other consonant,
//! does not mark the characters that stand in for a missing base, and files
//! several things under `Other` that a shaper must treat as consonants. The
//! positional property is stated in terms of where a mark sits relative to
//! its base, which is not the same question as where in the reordered
//! syllable it belongs.
//!
//! So both are *derived*, by `gui/font/tools/gen_indic_tables.py`, following
//! HarfBuzz's `gen-indic-table.py` to the code point. Following it exactly is
//! deliberate: the derivation is a pile of per-character decisions accumulated
//! over fifteen years of bug reports against real fonts, and every place we
//! diverged from it would be a place our text differs from every other
//! renderer's for reasons no one could reconstruct.
//!
//! # What is deliberately not here
//!
//! * **Khmer and Myanmar.** HarfBuzz derives their tables with the same
//!   script and then shapes them with two *different* shapers, because their
//!   reordering rules are not the Indic ones. Including their characters in
//!   this table would mean carrying categories (`VAbv`, `VPre`, `Coeng`, …)
//!   that nothing here reads.
//! * **The Universal Shaping Engine.** Sinhala, Tibetan, Javanese, Balinese
//!   and two dozen others are shaped by USE, a single table-driven engine that
//!   replaced writing a shaper per script. It is a larger piece of work and
//!   is tracked separately.

// Nothing outside this module reads the table yet, and three of the positions
// are assigned by reordering rather than by the table, so they are not
// constructed either. `expect` rather than `allow` on purpose: the moment the
// shaper is wired into `scaled.rs` the expectation goes unfulfilled and the
// compiler asks for this line back.
#![expect(
    dead_code,
    reason = "the shaper that reads this table is not wired in yet"
)]

use crate::indic_tables::INDIC_RANGES;

/// What a character is, for the purpose of building a syllable.
///
/// Derived from `Indic_Syllabic_Category` — see the module documentation for
/// why it is not that property. The variant names are the ones
/// `gui/font/tools/gen_indic_tables.py` writes, so renaming one here means
/// editing `RUST_CATEGORY` there; the generated file will not compile
/// otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    /// Not part of an Indic syllable. The default, and the answer for nearly
    /// every code point — including every Latin letter.
    Other,
    /// An ordinary consonant: the thing a syllable is built around.
    Consonant,
    /// An independent vowel — a letter in its own right, not a sign hung on a
    /// consonant.
    Vowel,
    /// Nukta, the dot that modifies the consonant it follows.
    Nukta,
    /// The virama/halant, which kills the consonant's inherent vowel and asks
    /// for a conjunct.
    Halant,
    /// ZERO WIDTH NON-JOINER: written between a consonant and a virama to ask
    /// for a half form rather than a stacked conjunct.
    NonJoiner,
    /// ZERO WIDTH JOINER: the opposite request.
    Joiner,
    /// A dependent vowel sign — the matra. The characters that are drawn out
    /// of order.
    Matra,
    /// A syllable modifier: anusvara, candrabindu, visarga.
    SyllableModifier,
    /// A Vedic cantillation mark.
    Cantillation,
    /// Not Indic, but allowed to stand where a base consonant would: a digit,
    /// a hyphen, NO-BREAK SPACE. Fonts kern and position marks against these.
    Placeholder,
    /// U+25CC DOTTED CIRCLE, the base a lone mark is drawn on.
    DottedCircle,
    /// A matra that is always drawn after its base, whatever the script's
    /// general rule.
    MatraPost,
    /// A character that is already a reph — the pre-composed form, rather than
    /// a RA that will become one.
    Repha,
    /// RA. Not merely a consonant: a RA followed by a virama at the start of a
    /// syllable becomes the reph, the hook drawn above the syllable's end, and
    /// no other consonant does that.
    Ra,
    /// A consonant medial — the small forms of YA, RA, LA, VA written below or
    /// beside their base.
    ConsonantMedial,
    /// A symbol that may carry marks: OM, the avagraha.
    Symbol,
    /// A consonant preceded by a stacker, which forces a conjunct.
    ConsonantWithStacker,
    /// A syllable modifier that is always drawn after the base.
    SyllableModifierPost,
}

/// Where in the reordered syllable a character belongs.
///
/// The order of the variants *is* the reordering: sorting a syllable's
/// characters by position, stably, is most of what final reordering does. So
/// the discriminants matter, and the variants must stay in this order.
///
/// [`Start`](Self::Start) and [`End`](Self::End) are sentinels — nothing is
/// ever assigned them by the table except as the default for characters that
/// are never reordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Position {
    /// Before everything.
    Start,
    /// A RA that is about to become a reph. It is moved here first, and moved
    /// again once the font has said whether it has a reph glyph at all.
    RaToBecomeReph,
    /// A matra drawn to the left of the whole syllable.
    PreM,
    /// Before the base consonant.
    PreC,
    /// The base consonant itself.
    BaseC,
    /// Immediately after the base.
    AfterMain,
    /// Above the base.
    AboveC,
    /// Before the below-base forms.
    BeforeSub,
    /// Below the base.
    BelowC,
    /// After the below-base forms.
    AfterSub,
    /// Before the post-base forms.
    BeforePost,
    /// After the base, to its right.
    PostC,
    /// After the post-base forms.
    AfterPost,
    /// Syllable modifiers and Vedic marks: last of the syllable's own
    /// characters.
    Smvd,
    /// After everything. The default.
    End,
}

/// A character's category and position, as the shaper sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Char {
    /// What it is.
    pub(crate) category: Category,
    /// Where it goes.
    pub(crate) position: Position,
}

impl Char {
    /// The category and position of `ch`.
    ///
    /// A character in no range of the table is [`Category::Other`] at
    /// [`Position::End`] — not part of any syllable, and never reordered.
    #[must_use]
    pub(crate) fn of(ch: char) -> Self {
        let cp = ch as u32;
        // The ranges are disjoint and sorted, so the last one starting at or
        // before `cp` is the only one that can contain it.
        let Some(i) = INDIC_RANGES
            .partition_point(|&(lo, _, _, _)| lo <= cp)
            .checked_sub(1)
        else {
            return Self::DEFAULT;
        };
        match INDIC_RANGES.get(i) {
            Some(&(_, hi, category, position)) if cp <= hi => Self {
                category,
                position,
            },
            // Past the end of that range, or a table too short to index —
            // neither is in any range, and "in no range" is the default.
            _ => Self::DEFAULT,
        }
    }

    /// What a character outside the table is.
    const DEFAULT: Self = Self {
        category: Category::Other,
        position: Position::End,
    };
}

impl Category {
    /// Whether this can be the base of a syllable, or stand where a base
    /// would.
    ///
    /// The list is HarfBuzz's `CONSONANT_FLAGS_INDIC`. It is wider than
    /// "consonant" in the linguistic sense: an independent vowel is a base
    /// because marks attach to it, and a digit or a dotted circle is one
    /// because a syllable has to have *something* to hang its marks on when
    /// the text does not supply a consonant.
    #[must_use]
    pub(crate) fn is_base_candidate(self) -> bool {
        matches!(
            self,
            Self::Consonant
                | Self::ConsonantWithStacker
                | Self::Ra
                | Self::ConsonantMedial
                | Self::Vowel
                | Self::Placeholder
                | Self::DottedCircle
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(ch: char) -> Category {
        Char::of(ch).category
    }

    fn pos(ch: char) -> Position {
        Char::of(ch).position
    }

    /// The table answers for the characters of `हिन्दी`, which is the string
    /// the HarfBuzz sweep still disagrees on and the reason this module
    /// exists.
    #[test]
    fn the_word_hindi_is_categorised_the_way_it_is_written() {
        // ह HA — an ordinary consonant, the base of its syllable.
        assert_eq!(cat('\u{939}'), Category::Consonant);
        assert_eq!(pos('\u{939}'), Position::BaseC);
        // ि vowel sign I — a matra, and the one drawn to the *left* even
        // though it is stored to the right.
        assert_eq!(cat('\u{93f}'), Category::Matra);
        assert_eq!(pos('\u{93f}'), Position::PreM);
        // न NA — consonant.
        assert_eq!(cat('\u{928}'), Category::Consonant);
        // ् virama.
        assert_eq!(cat('\u{94d}'), Category::Halant);
        // द DA — consonant.
        assert_eq!(cat('\u{926}'), Category::Consonant);
        // ी vowel sign II — a matra drawn to the right, so it is not moved.
        assert_eq!(cat('\u{940}'), Category::Matra);
        assert_eq!(pos('\u{940}'), Position::AfterSub);
    }

    /// RA is not filed as a consonant, and that distinction is the whole of
    /// the reph rule. Every script's RA is marked, including the ones that
    /// form no reph — Gurmukhi and Tamil never do, Telugu only with a ZWJ —
    /// because whether a reph happens is the *script config's* decision, and
    /// the table's job is only to say which letter it would be about.
    #[test]
    fn ra_is_its_own_category_in_every_script_that_has_one() {
        for ra in [
            '\u{930}', // Devanagari
            '\u{9b0}', // Bengali
            '\u{9f0}', // Bengali, the Assamese RA
            '\u{a30}', // Gurmukhi
            '\u{ab0}', // Gujarati
            '\u{b30}', // Oriya
            '\u{bb0}', // Tamil
            '\u{c30}', // Telugu
            '\u{cb0}', // Kannada
            '\u{d30}', // Malayalam
        ] {
            assert_eq!(cat(ra), Category::Ra, "{ra:?}");
            assert!(cat(ra).is_base_candidate(), "{ra:?}");
        }
        // The letter beside it is not, or the reph rule would fire on half
        // the alphabet.
        assert_eq!(cat('\u{92f}'), Category::Consonant);
        assert_eq!(cat('\u{931}'), Category::Consonant);
    }

    /// Everything that is not Indic falls through to the default, which is
    /// what lets the shaper skip a Latin run without a per-character test
    /// that knows about scripts.
    #[test]
    fn characters_outside_the_indic_blocks_are_not_categorised() {
        for ch in ['a', 'Z', ' ', '\u{5d0}', '\u{628}', '\u{4e00}', '\u{10ffff}'] {
            assert_eq!(Char::of(ch), Char::DEFAULT, "{ch:?}");
        }
    }

    /// Latin digits and a few punctuation marks *are* in the table, on
    /// purpose: a matra written against a digit needs the digit to be a base.
    #[test]
    fn the_characters_that_stand_in_for_a_base_are_in_the_table() {
        for ch in ['0', '9', '-', '\u{a0}', '\u{d7}', '\u{25cc}'] {
            assert!(cat(ch).is_base_candidate(), "{ch:?}");
        }
        assert_eq!(cat('\u{25cc}'), Category::DottedCircle);
        assert_eq!(cat('0'), Category::Placeholder);
    }

    /// The two zero-width controls have to be distinguishable from each other
    /// and from an ordinary mark: they are how a writer asks for a half form
    /// instead of a conjunct.
    #[test]
    fn the_joiners_are_their_own_categories() {
        assert_eq!(cat('\u{200c}'), Category::NonJoiner);
        assert_eq!(cat('\u{200d}'), Category::Joiner);
    }

    /// Positions sort in drawing order, which is the property final
    /// reordering relies on.
    #[test]
    fn positions_compare_in_the_order_they_are_drawn() {
        assert!(Position::Start < Position::PreM);
        assert!(Position::PreM < Position::BaseC);
        assert!(Position::BaseC < Position::AboveC);
        assert!(Position::AboveC < Position::BelowC);
        assert!(Position::BelowC < Position::PostC);
        assert!(Position::PostC < Position::Smvd);
        assert!(Position::Smvd < Position::End);
    }

    /// A generated table is exactly where an overlap or a backwards range
    /// would hide, and either would make the binary search above answer
    /// nonsense.
    #[test]
    fn the_generated_ranges_are_sorted_and_disjoint() {
        let mut last: Option<u32> = None;
        for &(lo, hi, _, _) in &INDIC_RANGES {
            assert!(lo <= hi, "range {lo:#x}..{hi:#x} runs backwards");
            if let Some(prev) = last {
                assert!(prev < lo, "range starting {lo:#x} overlaps {prev:#x}");
            }
            last = Some(hi);
        }
    }

    /// No range in the table is the default: writing them would double its
    /// size and change no answer.
    #[test]
    fn the_table_never_states_the_default() {
        for &(lo, hi, category, position) in &INDIC_RANGES {
            assert!(
                category != Category::Other || position != Position::End,
                "{lo:#x}..{hi:#x} is the default and should have been omitted"
            );
        }
    }
}

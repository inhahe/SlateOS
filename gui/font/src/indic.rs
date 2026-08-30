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
//! # Khmer and Myanmar share [`Category`] and [`Char::of`]
//!
//! HarfBuzz derives Indic, Khmer and Myanmar from one table and then shapes
//! them with three *different* shapers, because neither Khmer's nor Myanmar's
//! reordering rules are the Indic ones — and it stores all three scripts'
//! categories in the same buffer slot, one enum laid over another. This module
//! does the same: [`Category`] carries the seven variants only
//! [`khmer`](crate::khmer) reads ([`VowelAbove`](Category::VowelAbove),
//! [`VowelBelow`](Category::VowelBelow), [`VowelPre`](Category::VowelPre),
//! [`VowelPost`](Category::VowelPost), [`Robatic`](Category::Robatic),
//! [`XGroup`](Category::XGroup), [`YGroup`](Category::YGroup)) — Myanmar reads
//! the first four too — and the eight only [`myanmar`](crate::myanmar) reads
//! ([`Asat`](Category::Asat), the four medials,
//! [`PwoTone`](Category::PwoTone),
//! [`VariationSelector`](Category::VariationSelector),
//! [`MedialMonLa`](Category::MedialMonLa)). No character of one script can
//! reach another's machine anyway, because the three are dispatched on script.
//!
//! What is *not* shared is the grammar: [`syllables`] scans the Indic machine,
//! [`khmer::syllables`](crate::khmer::syllables) the Khmer one and
//! [`myanmar::syllables`](crate::myanmar::syllables) the Myanmar one.
//!
//! The Myanmar grammar names several categories this enum does not have —
//! `IV`, `DB`, `GB`. They are HarfBuzz's aliases for [`Vowel`](Category::Vowel),
//! [`Nukta`](Category::Nukta) and [`Placeholder`](Category::Placeholder), not
//! separate values, and the table never produces them under those names.
//!
//! # What is deliberately not here
//!
//! * **The Universal Shaping Engine.** Sinhala, Tibetan, Javanese, Balinese
//!   and two dozen others are shaped by USE, a single table-driven engine that
//!   replaced writing a shaper per script. It is a larger piece of work and
//!   is tracked separately.

use alloc::vec::Vec;

use crate::indic_machine::{ACCEPTS, TRANSITIONS};
use crate::indic_tables::INDIC_RANGES;

/// What a character is, for the purpose of building a syllable.
///
/// Derived from `Indic_Syllabic_Category` — see the module documentation for
/// why it is not that property. The variant names are the ones
/// `gui/font/tools/gen_indic_tables.py` writes, so renaming one here means
/// editing `RUST_CATEGORY` there; the generated file will not compile
/// otherwise.
///
/// The discriminants are the column indices of
/// [`indic_machine`](crate::indic_machine)'s transition table, so the *order*
/// of the variants is load-bearing in a second way: reordering them silently
/// mis-scans every syllable. `categories_index_the_machines_columns` is the
/// test that says so out loud.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
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
    /// Khmer: a dependent vowel drawn above its base.
    VowelAbove,
    /// Khmer: a dependent vowel drawn below its base.
    VowelBelow,
    /// Khmer: a dependent vowel drawn to the left of its base — the one the
    /// Khmer reordering moves to the front of the syllable.
    VowelPre,
    /// Khmer: a dependent vowel drawn to the right of its base.
    VowelPost,
    /// Khmer: the robat and the two register shifters, which the grammar
    /// admits in one distinguished slot directly after the consonant.
    Robatic,
    /// Khmer: a sign the grammar admits repeatedly between the matras, and
    /// which may be preceded by a joiner.
    XGroup,
    /// Khmer: a sign the grammar admits only at the tail of a syllable.
    YGroup,
    /// Myanmar: ASAT, the sign that kills a consonant's inherent vowel. Its own
    /// category because the grammar admits it after almost anything, and
    /// because a kinzi is RA + ASAT + virama and nothing else.
    Asat,
    /// Myanmar: MEDIAL HA, written under its base.
    MedialHa,
    /// Myanmar: MEDIAL RA — written after its consonant and drawn wrapped
    /// around its left side, which is one of the two moves the Myanmar
    /// reordering exists for.
    MedialRa,
    /// Myanmar: MEDIAL WA, and the Shan WA.
    MedialWa,
    /// Myanmar: MEDIAL YA, and the Mon NA and MA that behave like it.
    MedialYa,
    /// Myanmar: a pwo or other tone, which ends a syllable and may itself carry
    /// a cantillation mark and a dot below.
    PwoTone,
    /// A variation selector, U+FE00..U+FE0F. Not a Myanmar character, but the
    /// Myanmar grammar is the only one that names them — everywhere else they
    /// are something the shaper hides rather than a syllable member.
    VariationSelector,
    /// Myanmar: MEDIAL MON LA, which the grammar admits only after the other
    /// medials and only in Mon text.
    MedialMonLa,
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
            Some(&(_, hi, category, position)) if cp <= hi => Self { category, position },
            // Past the end of that range, or a table too short to index —
            // neither is in any range, and "in no range" is the default.
            _ => Self::DEFAULT,
        }
    }

    /// What a character outside the table is.
    ///
    /// Also what every glyph starts as, so that a run no Indic shaper touches
    /// carries a category that says so rather than an accidental one: `Other`
    /// is in no syllable and `End` is never moved.
    pub(crate) const DEFAULT: Self = Self {
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

/// Which of the grammar's seven rules matched a syllable.
///
/// The kind decides what reordering does with it — a vowel syllable has no
/// base consonant to find, a non-Indic one is left exactly as it was — and it
/// is also a diagnosis: [`Broken`](Self::Broken) means the text was not a
/// well-formed syllable, which HarfBuzz answers by inserting a dotted circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Syllable {
    /// A consonant and everything hung on it. The ordinary case.
    Consonant,
    /// An independent vowel at the head instead of a consonant.
    Vowel,
    /// A cluster built on a placeholder or a dotted circle — a matra written
    /// against a digit, or a lone mark shown on its ring.
    Standalone,
    /// A symbol (OM, avagraha) and its marks.
    Symbol,
    /// Not a well-formed syllable: a matra or a virama with nothing to attach
    /// to. Still reordered, on the theory that showing something in roughly
    /// the right place beats showing it in storage order.
    Broken,
    /// Not Indic at all, or a stray syllable modifier. Never reordered.
    NonIndic,
}

impl Syllable {
    /// This kind as the low nibble of a [`SubGlyph::syllable`] byte.
    ///
    /// The syllable ranges cannot be remembered as indices: `locl` and `ccmp`
    /// run before the first reordering and are free to ligate, after which
    /// every index past the join is wrong. So the kind rides on each glyph
    /// instead and the ranges are re-derived from the run whenever they are
    /// wanted — which is what the byte is for, and why a kind has to fit in
    /// four bits of it.
    ///
    /// The numbering is HarfBuzz's `indic_syllable_type_t`, so that a value
    /// read out of one of its traces means the same thing here.
    ///
    /// [`SubGlyph::syllable`]: crate::gsub::SubGlyph
    #[must_use]
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Consonant => 0,
            Self::Vowel => 1,
            Self::Standalone => 2,
            Self::Symbol => 3,
            Self::Broken => 4,
            Self::NonIndic => 5,
        }
    }

    /// The kind [`code`](Self::code) writes as `code`'s low nibble.
    ///
    /// A nibble no kind uses reads back as [`NonIndic`](Self::NonIndic), which
    /// is the kind that does nothing: a glyph whose syllable byte was never
    /// written is not part of any syllable this shaper laid out.
    #[must_use]
    pub(crate) fn from_code(code: u8) -> Self {
        match code & 0x0F {
            0 => Self::Consonant,
            1 => Self::Vowel,
            2 => Self::Standalone,
            3 => Self::Symbol,
            4 => Self::Broken,
            _ => Self::NonIndic,
        }
    }
}

/// Cut `cats` into syllables, appending `(start, end, kind)` to `out`.
///
/// The ranges tile the input exactly: they are contiguous, none is empty, and
/// the last ends at `cats.len()`. That is guaranteed by the grammar's last
/// rule, `other = any`, which matches a single character of any category
/// whatsoever — so there is always *some* match and the scan always advances.
///
/// Longest match wins, and a tie goes to the rule the grammar lists first.
/// Both are the ragel scanner's rules, and both matter: `Ra H C` is a
/// consonant syllable of three characters, not a broken one of two followed
/// by another.
pub(crate) fn syllables(cats: &[Category], out: &mut Vec<(usize, usize, Syllable)>) {
    out.clear();
    let mut i = 0usize;
    while i < cats.len() {
        // State 1 is the start; state 0 is dead. See `indic_machine`.
        let mut state = 1usize;
        let mut best: Option<(usize, Syllable)> = None;
        for (j, &cat) in cats.iter().enumerate().skip(i) {
            let next = match TRANSITIONS.get(state).and_then(|row| row.get(cat as usize)) {
                Some(&next) if next != 0 => usize::from(next),
                // Dead, or a table that does not cover this state — either
                // way nothing longer can match from here.
                _ => break,
            };
            state = next;
            if let Some(&Some(kind)) = ACCEPTS.get(state) {
                best = Some((j.saturating_add(1), kind));
            }
        }
        // `other = any` accepts one character of anything, so `best` is only
        // ever `None` if the table is corrupt; advancing by one is then both
        // the right answer and the one that guarantees termination.
        let (end, kind) = best.unwrap_or((i.saturating_add(1), Syllable::NonIndic));
        out.push((i, end, kind));
        i = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

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
        for ch in [
            'a',
            'Z',
            ' ',
            '\u{5d0}',
            '\u{628}',
            '\u{4e00}',
            '\u{10ffff}',
        ] {
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

    /// Every variant of [`Category`], in declaration order.
    const ALL_CATEGORIES: [Category; 34] = [
        Category::Other,
        Category::Consonant,
        Category::Vowel,
        Category::Nukta,
        Category::Halant,
        Category::NonJoiner,
        Category::Joiner,
        Category::Matra,
        Category::SyllableModifier,
        Category::Cantillation,
        Category::Placeholder,
        Category::DottedCircle,
        Category::MatraPost,
        Category::Repha,
        Category::Ra,
        Category::ConsonantMedial,
        Category::Symbol,
        Category::ConsonantWithStacker,
        Category::SyllableModifierPost,
        Category::VowelAbove,
        Category::VowelBelow,
        Category::VowelPre,
        Category::VowelPost,
        Category::Robatic,
        Category::XGroup,
        Category::YGroup,
        Category::Asat,
        Category::MedialHa,
        Category::MedialRa,
        Category::MedialWa,
        Category::MedialYa,
        Category::PwoTone,
        Category::VariationSelector,
        Category::MedialMonLa,
    ];

    /// The scanner indexes a transition row by the category's discriminant,
    /// so the enum's order and the generator's `CATEGORIES` tuple are one
    /// fact written in two places. This is the test that keeps them one.
    #[test]
    fn categories_index_the_machines_columns() {
        for (i, &cat) in ALL_CATEGORIES.iter().enumerate() {
            assert_eq!(cat as usize, i, "{cat:?}");
        }
        assert!(!TRANSITIONS.is_empty(), "the machine has no states");
        for row in &TRANSITIONS {
            assert_eq!(row.len(), ALL_CATEGORIES.len());
        }
    }

    fn cut(text: &str) -> Vec<(usize, usize, Syllable)> {
        let cats: Vec<Category> = text.chars().map(|ch| Char::of(ch).category).collect();
        let mut out = Vec::new();
        syllables(&cats, &mut out);
        out
    }

    /// `हिन्दी` is one syllable, not two — the न ् द conjunct in the middle
    /// does not end it. Cutting it in the wrong place is how a shaper
    /// reorders the vowel sign onto the wrong consonant.
    #[test]
    fn the_word_hindi_is_two_syllables() {
        assert_eq!(
            cut("\u{939}\u{93f}\u{928}\u{94d}\u{926}\u{940}"),
            vec![(0, 2, Syllable::Consonant), (2, 6, Syllable::Consonant)]
        );
    }

    /// A run of ordinary consonants is a syllable each: nothing joins them.
    #[test]
    fn consonants_with_nothing_between_them_do_not_join() {
        assert_eq!(
            cut("\u{939}\u{928}\u{926}"),
            vec![
                (0, 1, Syllable::Consonant),
                (1, 2, Syllable::Consonant),
                (2, 3, Syllable::Consonant),
            ]
        );
    }

    /// RA + virama at the head is a reph, and the syllable runs on past it —
    /// which is the whole reason `reph` is in the grammar rather than being
    /// left to fall out of `halant_group`.
    #[test]
    fn a_reph_belongs_to_the_syllable_it_starts() {
        // र ् क — RA virama KA.
        assert_eq!(
            cut("\u{930}\u{94d}\u{915}"),
            vec![(0, 3, Syllable::Consonant)]
        );
        // Without a following consonant it is still one syllable, but a
        // consonant one — the RA is its own base and the virama is the
        // syllable's trailing halant, not the head of a reph.
        assert_eq!(cut("\u{930}\u{94d}"), vec![(0, 2, Syllable::Consonant)]);
    }

    /// An independent vowel heads a syllable of its own kind.
    #[test]
    fn an_independent_vowel_heads_a_vowel_syllable() {
        // अ, then अ with anusvara.
        assert_eq!(cut("\u{905}"), vec![(0, 1, Syllable::Vowel)]);
        assert_eq!(cut("\u{905}\u{902}"), vec![(0, 2, Syllable::Vowel)]);
    }

    /// A matra with nothing before it is broken, and a lone mark on a dotted
    /// circle is a standalone cluster — the two ways text arrives malformed.
    #[test]
    fn malformed_text_is_still_cut_into_syllables() {
        assert_eq!(cut("\u{93f}"), vec![(0, 1, Syllable::Broken)]);
        assert_eq!(cut("\u{25cc}\u{93f}"), vec![(0, 2, Syllable::Standalone)]);
    }

    /// Latin is one non-Indic syllable per character, which is what lets the
    /// shaper run over a mixed run without a separate test for script.
    #[test]
    fn characters_outside_the_script_are_left_alone() {
        assert_eq!(
            cut("ab"),
            vec![(0, 1, Syllable::NonIndic), (1, 2, Syllable::NonIndic)]
        );
        assert!(cut("").is_empty());
    }

    /// The ranges must tile the input exactly — contiguous, non-empty, and
    /// ending where the text does. Everything downstream assumes it, and a
    /// generated table is where a hole would come from.
    #[test]
    fn the_syllables_tile_every_string_they_are_given() {
        for text in [
            "\u{939}\u{93f}\u{928}\u{94d}\u{926}\u{940}",
            "\u{930}\u{94d}\u{915}\u{94d}\u{937}",
            "\u{9ac}\u{9be}\u{982}\u{9b2}\u{9be}",
            "\u{c05}\u{c06}\u{c07}",
            "\u{d15}\u{d4d}\u{d15}\u{d41}",
            "a\u{939}1\u{93f}\u{200d}\u{25cc}",
            "\u{200c}\u{94d}\u{951}\u{93c}",
        ] {
            let cut = cut(text);
            let mut at = 0usize;
            for &(start, end, _) in &cut {
                assert_eq!(start, at, "{text:?} has a hole at {at}");
                assert!(end > start, "{text:?} has an empty syllable at {at}");
                at = end;
            }
            assert_eq!(at, text.chars().count(), "{text:?} was cut short");
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

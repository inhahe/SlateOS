//! The Myanmar shaper.
//!
//! The third script in this crate that is written in syllables, categorised
//! from [`indic`](crate::indic)'s table and segmented by a machine built by the
//! same generator — and, like Khmer, a *different* shaper, because what it does
//! with a syllable is neither what Indic does nor what Khmer does.
//!
//! # What makes it its own shaper
//!
//! * **It sorts.** Indic and Khmer move a small fixed number of things: Khmer
//!   rotates a `COENG + RO` pair and a `VPre` to the front and touches nothing
//!   else. Myanmar assigns a [`Position`](crate::indic::Position) to *every*
//!   glyph of the syllable and then stably sorts by it. That is the whole
//!   reordering — [`reorder_consonant_syllable`] is one pass to label and one
//!   sort, and every rule from the Microsoft Myanmar script-development notes
//!   is expressed as a label.
//! * **The base search is forwards, and stops at the first consonant.** Indic
//!   walks backwards from the end weighing candidates; Myanmar takes the first
//!   consonant at or after the kinzi and is done. There is no reph to guess at
//!   and no half-form logic, because a Myanmar kinzi is spelled out
//!   (`Ra + Asat + virama`) and is recognised by that spelling alone.
//! * **The features run one at a time.** `rphf`, `pref`, `blwf` and `pstf` each
//!   get a stage of their own with a pause after it, so a `pref` substitution
//!   can see what `rphf` produced. Khmer runs all of its basic features
//!   together in one stage. See [`stages`].
//! * **The reordering runs after `locl` and `ccmp`, not before them.** This is
//!   Khmer's most surprising property inverted, and getting it the wrong way
//!   round would be invisible in a table dump and wrong on screen. See
//!   [`shape`].
//!
//! # The three things that move
//!
//! Everything else keeps the order it was typed in, because the sort is stable.
//!
//! 1. **A kinzi** — `Ra + Asat + virama` at the head of the syllable — is drawn
//!    *above* the letter that follows, so all three glyphs are labelled
//!    [`AfterMain`](crate::indic::Position::AfterMain) and sort to just after
//!    the base.
//! 2. **A medial RA** (`\u{103C}`) is written after its consonant and drawn
//!    wrapped around its left side, so it is labelled
//!    [`PreC`](crate::indic::Position::PreC).
//! 3. **A pre-base vowel** (`VPre`, `\u{1031}`) is drawn to the left of
//!    everything, [`PreM`](crate::indic::Position::PreM) — and when a syllable
//!    has more than one, the sequence has to be flipped, because several left
//!    vowels are drawn leftwards from the base and so read backwards. That is
//!    the same repair Indic makes, for the same reason.
//!
//! # The grammar's aliases
//!
//! HarfBuzz's Myanmar grammar names `IV`, `DB` and `GB`, which are `#define`s
//! over the Indic categories [`Vowel`](crate::indic::Category::Vowel),
//! [`Nukta`](crate::indic::Category::Nukta) and
//! [`Placeholder`](crate::indic::Category::Placeholder) rather than values of
//! their own. `tools/gen_myanmar_machine.py` expands them, so nothing here or
//! in the generated machine mentions them.

use alloc::vec::Vec;

use crate::gsub::{ALL_FEATURES, Staging, SubGlyph, Substitutions, feature_bits};
use crate::indic::{Category, Position};
use crate::lang::Lang;
use crate::myanmar_machine::{ACCEPTS, TRANSITIONS};
use crate::script::ScriptTags;
use crate::syllabic;

/// Which of the grammar's rules matched a syllable.
///
/// Three kinds, as Khmer has, and for the same reason: the Myanmar grammar has
/// no separate rule for an independent vowel or a symbol. A `V` heads an
/// ordinary consonant syllable, and so does a digit, a bullet or a dotted
/// circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Syllable {
    /// A consonant and everything hung on it. The ordinary case.
    Consonant,
    /// Not a well-formed syllable: a medial, a vowel sign or a virama with no
    /// consonant to attach to. Reordered exactly like a consonant syllable,
    /// because by the time the reordering runs a dotted circle has been
    /// inserted and it *is* one.
    Broken,
    /// Not Myanmar at all. Never reordered, and never given a dotted circle.
    ///
    /// Reached by the grammar's `other = any`, and also by its second rule —
    /// a lone joiner or a lone post-base syllable modifier, which the grammar
    /// singles out *between* the two syllable rules so that neither earns a
    /// dotted circle.
    NonMyanmar,
}

impl Syllable {
    /// This kind as the low nibble of a [`SubGlyph::syllable`] byte.
    ///
    /// The numbering is HarfBuzz's `myanmar_syllable_type_t`, so that a value
    /// read out of one of its traces means the same thing here.
    #[must_use]
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Consonant => 0,
            Self::Broken => 1,
            Self::NonMyanmar => 2,
        }
    }

    /// The kind [`code`](Self::code) writes as `code`'s low nibble.
    ///
    /// A nibble no kind uses reads back as
    /// [`NonMyanmar`](Self::NonMyanmar), the kind that does nothing. Zero is
    /// *not* such a nibble — it is [`Consonant`](Self::Consonant) — so an
    /// unstamped glyph would read back as a syllable to reorder. Nothing here
    /// relies on a lenient default: [`syllables`] tiles the run exactly, so
    /// every glyph is stamped, and [`syllabic::stamp`] never writes a zero
    /// serial in the high nibble.
    #[must_use]
    pub(crate) fn from_code(code: u8) -> Self {
        match code & 0x0F {
            0 => Self::Consonant,
            1 => Self::Broken,
            _ => Self::NonMyanmar,
        }
    }
}

/// Whether a run of `tags` is Myanmar, and so shaped here.
///
/// `mym2` only. Myanmar has an old-spec tag `mymr` as well, and a face that
/// registers *that* one is asking for the pre-OpenType-1.6 behaviour, which
/// HarfBuzz does not implement either: its `mymr` entry maps to the default
/// shaper, not to this one. [`fallback::shaped_as_default`](crate::fallback)
/// is where that choice is made for this crate, and it is why — unlike Khmer —
/// this shaper is behind a filter at the dispatch point rather than called
/// unconditionally.
#[must_use]
pub(crate) fn shapes(tags: Option<ScriptTags>) -> bool {
    tags.is_some_and(|tags| tags.preferred == *b"mym2")
}

/// Cut `cats` into syllables, appending `(start, end, kind)` to `out`.
///
/// The same scan as [`khmer::syllables`](crate::khmer::syllables) over a
/// different machine, and with the same two guarantees: the ranges tile the
/// input exactly, because the grammar's last rule matches one character of any
/// category whatsoever; and longest match wins with ties going to the rule the
/// grammar lists first.
pub(crate) fn syllables(cats: &[Category], out: &mut Vec<(usize, usize, Syllable)>) {
    out.clear();
    let mut i = 0usize;
    while i < cats.len() {
        // State 1 is the start; state 0 is dead. See `myanmar_machine`.
        let mut state = 1usize;
        let mut best: Option<(usize, Syllable)> = None;
        for (j, &cat) in cats.iter().enumerate().skip(i) {
            let next = match TRANSITIONS.get(state).and_then(|row| row.get(cat as usize)) {
                Some(&next) if next != 0 => usize::from(next),
                // Dead, or a table that does not cover this state — either way
                // nothing longer can match from here.
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
        let (end, kind) = best.unwrap_or((i.saturating_add(1), Syllable::NonMyanmar));
        out.push((i, end, kind));
        i = end;
    }
}

/// The four features applied one at a time, each constrained to a syllable,
/// after the reordering and before the syllable boundaries are forgotten.
///
/// One at a time is the point, and is the difference from Khmer: HarfBuzz puts
/// a pause after *each* of these, so each gets a stage to itself and can see
/// what the one before it produced. A font's `pref` rule for a medial RA is
/// written against the glyph `rphf` left behind, not against the character.
const BASIC: [&[u8; 4]; 4] = [b"rphf", b"pref", b"blwf", b"pstf"];

/// The two features that run before the reordering, each constrained to a
/// syllable.
///
/// `locl` first, then `ccmp` — and *both before* [`reorder`], which is the
/// opposite of Khmer. HarfBuzz's comment on `ccmp` here is that the Indic specs
/// do not require it but a font that uses it typically uses it at the very
/// beginning, so it is enabled anyway.
const PRE_REORDER: [&[u8; 4]; 2] = [b"locl", b"ccmp"];

/// The four features every glyph in the run is eligible for, applied together
/// once the syllable boundaries have been cleared.
///
/// Global, and so is everything in [`BASIC`]: `enable_feature` implies
/// `F_GLOBAL`, and `setup_masks_myanmar` sets no masks at all — it only
/// records categories. That is the deepest difference from both Indic and
/// Khmer, which hand `rphf`/`pref`/`blwf`/`pstf` out per glyph to say *which*
/// consonant takes which form. Myanmar tells the font nothing of the sort and
/// lets its lookups decide from the glyphs alone, which is why this module has
/// no `Masks` type.
const GLOBAL: [&[u8; 4]; 4] = [b"pres", b"abvs", b"blws", b"psts"];

/// Every feature this shaper hands out, which is every feature it marks
/// `F_MANUAL_ZWJ`.
///
/// [`BASIC`] and [`GLOBAL`], and **not** the two of [`PRE_REORDER`]: those are
/// enabled with `F_PER_SYLLABLE` alone, so they keep the automatic joiner
/// skipping every ordinary feature has.
///
/// The matching `manual_zwnj` set is **empty**, and that is the whole reason
/// [`Staging`] carries the two separately. Indic and Khmer mark their features
/// `F_MANUAL_JOINERS`, which is both bits; Myanmar marks only `F_MANUAL_ZWJ`.
/// So a lookup in one of these eight sees a ZWJ as a glyph it must match, and
/// still steps over a ZWNJ as though it were not there. Observable in exactly
/// one place — a context rule whose input names a ZWNJ — and wrong in that
/// place if the two are conflated.
fn manual_zwj() -> u64 {
    feature_bits(&BASIC) | feature_bits(&GLOBAL)
}

/// Shape one run of Myanmar text: HarfBuzz's Myanmar shaper, end to end.
///
/// `glyphs` is one script run, one glyph per character, as `cmap` produced it
/// and with [`SubGlyph::indic`] already set from the character. It comes back
/// reordered and substituted, and may be longer: a broken cluster gains a
/// dotted circle.
///
/// # The order of the stages
///
/// `collect_features_myanmar` registers, in order: a pause (segment), `locl`,
/// `ccmp`, a pause (reorder), then each of `rphf`, `pref`, `blwf`, `pstf`
/// followed by a pause of its own, then a pause that clears the syllable
/// stamps, then `pres`, `abvs`, `blws`, `psts` and every common feature. A
/// pause both closes the stage it follows and opens the next, so that compiles
/// to six stages — the two of [`PRE_REORDER`] together, then four of one
/// feature each, then everything else — with the reordering between the first
/// and the second.
///
/// Khmer reorders *before* every lookup; Indic reorders after `locl`/`ccmp`.
/// Myanmar is Indic's way round, and running the font's own `ccmp` against a
/// syllable whose medial RA had already moved would be a different — and
/// wrong — answer.
///
/// `subs` may be `None`. Reordering is not conditional on the font having
/// substitutions — a pre-base vowel is written after its consonant and drawn
/// before it whatever the font does — so a face with no `GSUB` still gets it.
///
/// `glyph` maps a character to a glyph: the face's `cmap`. It is asked about
/// exactly one character, the dotted circle.
pub(crate) fn shape(
    data: &[u8],
    subs: Option<&Substitutions>,
    tags: Option<ScriptTags>,
    lang: Option<Lang>,
    glyphs: &mut Vec<SubGlyph>,
    glyph: impl Fn(char) -> Option<u16>,
) {
    if glyphs.is_empty() {
        return;
    }
    // Every feature this shaper enables is global, so every glyph is eligible
    // for all eight. `locl` and `ccmp` are in `gsub::ALWAYS` already.
    let all = manual_zwj();
    for g in glyphs.iter_mut() {
        g.mask |= all;
    }
    setup_syllables(glyphs);

    let Some(subs) = subs else {
        // No lookups to run, so no stages either — but the reordering still
        // has to happen, and with no stages to hang it on it happens here.
        reorder(glyphs, glyph('\u{25CC}'));
        return;
    };
    let stages = stages();
    let dotted = glyph('\u{25CC}');
    subs.apply_stages(
        data,
        tags,
        lang,
        &Staging {
            stages: &stages,
            per_syllable: feature_bits(&PRE_REORDER) | feature_bits(&BASIC),
            // Empty on purpose: Myanmar's features are `F_MANUAL_ZWJ`, not
            // `F_MANUAL_JOINERS`. See `manual_zwj`.
            manual_zwnj: 0,
            manual_zwj: all,
        },
        glyphs,
        |stage, glyphs| {
            if stage == 0 {
                reorder(glyphs, dotted);
            } else if stage == 4 {
                // The four basic features are done and the four global ones
                // must not be confined by a syllable boundary.
                // https://github.com/harfbuzz/harfbuzz/issues/3531
                syllabic::clear(glyphs);
            }
        },
    );
}

/// The six passes the lookups are applied in, as sets of feature bits.
///
/// `locl` and `ccmp` together, then `rphf`, `pref`, `blwf` and `pstf` each
/// alone, then everything else at once. "Everything else" is every feature this
/// crate knows rather than just the four of [`GLOBAL`], because the ordinary
/// features — `liga`, `rlig`, `calt`, `clig`, `rclt`, and the positioning ones
/// a face may have filed under `GSUB` — belong to that last stage too:
/// HarfBuzz adds them after the shaper's own and they land in whatever stage is
/// open, which is this one. Unlike Khmer, Myanmar has no `override_features`,
/// so `liga` is *not* switched off.
///
/// The six earlier sets are masked *out* of the last one, and have to be: **a
/// feature belongs to exactly one stage.** HarfBuzz gets that from its map
/// builder, which merges the two entries a tag can pick up — `locl` and `ccmp`
/// are named both by this shaper and by the common features every run gets —
/// and keeps the *lower* stage. Leave one in two stages and every lookup it
/// reaches runs twice.
fn stages() -> [u64; 6] {
    let pre = feature_bits(&PRE_REORDER);
    let [rphf, pref, blwf, pstf] = BASIC.map(|tag| feature_bits(&[tag]));
    [
        pre,
        rphf,
        pref,
        blwf,
        pstf,
        ALL_FEATURES & !pre & !rphf & !pref & !blwf & !pstf,
    ]
}

/// Cut the run into syllables and stamp each glyph with the one it is in.
fn setup_syllables(glyphs: &mut [SubGlyph]) {
    let mut cats: Vec<Category> = Vec::with_capacity(glyphs.len());
    cats.extend(glyphs.iter().map(|g| g.indic.category));
    let mut ranges: Vec<(usize, usize, Syllable)> = Vec::new();
    syllables(&cats, &mut ranges);
    syllabic::stamp(glyphs, &ranges, Syllable::code);
}

/// Insert the dotted circles, then lay out each syllable.
///
/// Both halves of HarfBuzz's `reorder_myanmar`, and in its order: the circle
/// has to be in place before a broken cluster is reordered, because reordering
/// treats a broken cluster as a consonant syllable and the circle is the
/// consonant it is built on.
fn reorder(glyphs: &mut Vec<SubGlyph>, dotted: Option<u16>) {
    syllabic::insert_dotted_circles(
        glyphs,
        dotted,
        Category::DottedCircle,
        |stamp| Syllable::from_code(stamp) == Syllable::Broken,
        // No skip. Indic's circle goes after a repha, which is drawn above the
        // letter that follows and so needs a letter to follow; Myanmar's kinzi
        // is not a repha — it is spelled out and stays where it was written —
        // and HarfBuzz passes no repha category here.
        |_| false,
    );
    syllabic::for_each(glyphs, |stamp, _, syllable| {
        if Syllable::from_code(stamp) != Syllable::NonMyanmar {
            reorder_consonant_syllable(syllable);
        }
    });
}

/// The category `glyphs[i]` counts as, or [`Category::Other`] past the end.
fn cat_at(glyphs: &[SubGlyph], i: usize) -> Category {
    glyphs.get(i).map_or(Category::Other, |g| g.indic.category)
}

/// The categories that count as a base: HarfBuzz's `CONSONANT_FLAGS_MYANMAR`.
///
/// Independent vowels and placeholders are in the set even though they are not
/// consonants, and HarfBuzz's note says why: neither can occur inside a
/// consonant syllable, so admitting them here lets one function lay out every
/// kind of syllable the grammar produces. `CM` is in the C macro but commented
/// out.
fn is_consonant(g: &SubGlyph) -> bool {
    // "If it ligated, all bets are off": a glyph the font joined is no longer
    // the character it was categorised from, and stopping the base search on it
    // would put the base inside a conjunct.
    if g.lig.ligated() {
        return false;
    }
    matches!(
        g.indic.category,
        Category::Consonant
            | Category::ConsonantWithStacker
            | Category::Ra
            | Category::Vowel
            | Category::Placeholder
            | Category::DottedCircle
    )
}

/// Lay out one syllable: HarfBuzz's `initial_reordering_consonant_syllable`.
///
/// `glyphs` is exactly the syllable, and comes out the same length — this only
/// permutes and relabels.
///
/// Two steps. First every glyph is given a [`Position`], which is where all of
/// the Myanmar layout rules live; then the syllable is stably sorted by it,
/// which is where they take effect. The labelling loop is a state machine over
/// one variable, `pos`, that only ever moves forwards through
/// [`AfterMain`](Position::AfterMain), [`BelowC`](Position::BelowC) and
/// [`AfterSub`](Position::AfterSub) — a below-base vowel opens the below-base
/// group and the first thing that is not a below-base vowel or a cantillation
/// mark closes it again.
fn reorder_consonant_syllable(glyphs: &mut [SubGlyph]) {
    let end = glyphs.len();

    // A kinzi is `Ra + Asat + virama` and nothing else, so it is recognised by
    // spelling rather than guessed at and confirmed later the way an Indic reph
    // is. When there is one, the base is the letter it is drawn above — which
    // is found by the same forward scan, started past the kinzi.
    let has_reph = end >= 3
        && cat_at(glyphs, 0) == Category::Ra
        && cat_at(glyphs, 1) == Category::Asat
        && cat_at(glyphs, 2) == Category::Halant;
    let limit = if has_reph { 3 } else { 0 };
    let mut base = if has_reph { 0 } else { limit };
    for (i, g) in glyphs.iter().enumerate().skip(limit) {
        if is_consonant(g) {
            base = i;
            break;
        }
    }
    // With no consonant anywhere the scan leaves `base` at `limit`, which for a
    // kinzi-only syllable is past its end. HarfBuzz starts `base` at `end` and
    // reaches the same place; the guard below is what keeps either from
    // labelling a glyph that is not there.

    let mut i = 0usize;
    // The kinzi is drawn above the letter that follows it, so all three glyphs
    // sort to just after the base rather than staying at the head.
    while i < limit {
        if let Some(g) = glyphs.get_mut(i) {
            g.indic.position = Position::AfterMain;
        }
        i = i.saturating_add(1);
    }
    // Anything between the kinzi and the base is drawn before the base.
    while i < base {
        if let Some(g) = glyphs.get_mut(i) {
            g.indic.position = Position::PreC;
        }
        i = i.saturating_add(1);
    }
    if let Some(g) = glyphs.get_mut(i) {
        g.indic.position = Position::BaseC;
        i = i.saturating_add(1);
    }

    let mut pos = Position::AfterMain;
    while i < end {
        let cat = cat_at(glyphs, i);
        let next = match cat {
            // A medial RA is written after its consonant and drawn wrapped
            // around its left side.
            Category::MedialRa => Position::PreC,
            // A pre-base vowel is drawn to the left of the whole syllable.
            Category::VowelPre => Position::PreM,
            // A variation selector picks a form of the character before it, so
            // it has to travel with it whatever that character's position is.
            Category::VariationSelector => i
                .checked_sub(1)
                .and_then(|prev| glyphs.get(prev))
                .map_or(pos, |g| g.indic.position),
            _ => {
                // The below-base group: a below-base vowel opens it, further
                // below-base vowels stay in it, a cantillation mark sits just
                // before it, and anything else closes it for good.
                if pos == Position::AfterMain && cat == Category::VowelBelow {
                    pos = Position::BelowC;
                    pos
                } else if pos == Position::BelowC && cat == Category::Cantillation {
                    Position::BeforeSub
                } else if pos == Position::BelowC && cat == Category::VowelBelow {
                    pos
                } else if pos == Position::BelowC {
                    pos = Position::AfterSub;
                    pos
                } else {
                    pos
                }
            }
        };
        if let Some(g) = glyphs.get_mut(i) {
            g.indic.position = next;
        }
        i = i.saturating_add(1);
    }

    sort(glyphs);
    flip_left_vowels(glyphs);
}

/// Stably sort the syllable by [`Position`], merging the clusters of everything
/// that moves.
///
/// HarfBuzz's `hb_buffer_t::sort`, which is an insertion sort written as a
/// rotate: the element at `i` is carried back to `j` and everything between
/// shifts up one. The clusters of `j..=i` are merged *before* the move, which
/// is the whole reason this is not a call to `sort_by` — a caret cannot honestly
/// point into a span whose glyphs have jumped over each other, and only the sort
/// knows which spans those are.
///
/// The comparison is by position alone and the walk is left to right, so two
/// glyphs the labelling gave the same position to come out in the order they
/// were typed. That stability is not incidental: it is what keeps a syllable's
/// marks in their written order inside each of the drawn groups.
fn sort(glyphs: &mut [SubGlyph]) {
    for i in 1..glyphs.len() {
        let at = glyphs.get(i).map(|g| g.indic.position);
        let mut j = i;
        while j > 0
            && glyphs
                .get(j.wrapping_sub(1))
                .map(|g| g.indic.position)
                .zip(at)
                .is_some_and(|(before, here)| before > here)
        {
            j = j.wrapping_sub(1);
        }
        if j == i {
            continue;
        }
        if let Some(run) = glyphs.get_mut(j..=i) {
            syllabic::merge_clusters(run);
            run.rotate_right(1);
        }
    }
}

/// Put a run of two or more pre-base vowels back into drawn order.
///
/// Several left vowels are drawn leftwards from the base, so the sequence as
/// typed reads backwards once they are all in front of it. Reversing the whole
/// run fixes that, and then each vowel's own trailing marks — a variation
/// selector, which the labelling gave the vowel's position — have to be
/// reversed back, because they were not backwards to begin with.
///
/// A single left vowel is left alone, which is what `first < last` says.
/// <https://github.com/harfbuzz/harfbuzz/issues/3863>
fn flip_left_vowels(glyphs: &mut [SubGlyph]) {
    let end = glyphs.len();
    let mut first = end;
    let mut last = end;
    for (i, g) in glyphs.iter().enumerate() {
        if g.indic.position == Position::PreM {
            if first == end {
                first = i;
            }
            last = i;
        }
    }
    if first >= last {
        return;
    }
    // No cluster merging: the sort above already merged everything that moved,
    // and every glyph in this run moved to reach the head.
    if let Some(run) = glyphs.get_mut(first..=last) {
        run.reverse();
    }
    let mut i = first;
    for j in first..=last {
        if cat_at(glyphs, j) == Category::VowelPre {
            if let Some(run) = glyphs.get_mut(i..=j) {
                run.reverse();
            }
            i = j.saturating_add(1);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use crate::gsub::feature_bit;
    use crate::indic::Char;
    use alloc::vec;

    /// No feature may appear in two stages. A lookup a feature reaches is
    /// applied once per stage that names it, so a tag left in an early stage
    /// and in the catch-all one runs its lookups twice.
    #[test]
    fn no_feature_is_applied_in_two_stages() {
        let stages = stages();
        for (i, &a) in stages.iter().enumerate() {
            for &b in stages.iter().skip(i + 1) {
                assert_eq!(a & b, 0);
            }
        }
        assert_eq!(stages.iter().fold(0, |all, &s| all | s), ALL_FEATURES);
    }

    /// The four basic features run one at a time and in the grammar's order,
    /// which is the difference from Khmer: a `pref` rule is written against
    /// what `rphf` left behind.
    #[test]
    fn the_basic_features_each_get_a_stage_of_their_own() {
        let stages = stages();
        assert_eq!(stages[0], feature_bits(&PRE_REORDER));
        assert_eq!(stages[1], feature_bit(b"rphf"));
        assert_eq!(stages[2], feature_bit(b"pref"));
        assert_eq!(stages[3], feature_bit(b"blwf"));
        assert_eq!(stages[4], feature_bit(b"pstf"));
    }

    /// The four global features run in the *last* stage, after the pause that
    /// clears the syllable stamps, so a rule in one of them may match across a
    /// syllable boundary.
    #[test]
    fn the_global_features_run_after_the_syllables_are_forgotten() {
        let stages = stages();
        let global = feature_bits(&GLOBAL);
        for &early in &stages[..5] {
            assert_eq!(early & global, 0);
        }
        assert_eq!(stages[5] & global, global);
    }

    /// Myanmar has no `override_features`, so `liga` stays on — Khmer's
    /// `stages` switches it off and this one must not copy that.
    #[test]
    fn ligatures_are_not_switched_off() {
        assert_ne!(stages()[5] & feature_bit(b"liga"), 0);
    }

    /// `locl` and `ccmp` are per-syllable but not manual-ZWJ; the other eight
    /// are manual-ZWJ, and nothing is manual-ZWNJ.
    #[test]
    fn only_the_shapers_own_features_read_the_joiners() {
        let manual = manual_zwj();
        assert_eq!(manual, feature_bits(&BASIC) | feature_bits(&GLOBAL));
        assert_eq!(manual & feature_bits(&PRE_REORDER), 0);
    }

    fn cut(text: &str) -> Vec<(usize, usize, Syllable)> {
        let cats: Vec<Category> = text.chars().map(|ch| Char::of(ch).category).collect();
        let mut out = Vec::new();
        syllables(&cats, &mut out);
        out
    }

    /// The categories the grammar is written in terms of have to be the ones
    /// the table actually produces for Myanmar, or every rule is about
    /// characters that never arrive.
    #[test]
    fn the_myanmar_block_is_categorised_the_way_the_grammar_names_it() {
        let cat = |ch: char| Char::of(ch).category;
        // က KA — a consonant.
        assert_eq!(cat('\u{1000}'), Category::Consonant);
        // င NGA is `Ra` and not `C`, because a kinzi is spelled with it.
        assert_eq!(cat('\u{1004}'), Category::Ra);
        assert_eq!(cat('\u{101B}'), Category::Ra);
        // ် ASAT, and ္ the virama.
        assert_eq!(cat('\u{103A}'), Category::Asat);
        assert_eq!(cat('\u{1039}'), Category::Halant);
        // The four medials, in the order the grammar admits them.
        assert_eq!(cat('\u{103B}'), Category::MedialYa);
        assert_eq!(cat('\u{103C}'), Category::MedialRa);
        assert_eq!(cat('\u{103D}'), Category::MedialWa);
        assert_eq!(cat('\u{103E}'), Category::MedialHa);
        assert_eq!(cat('\u{1060}'), Category::MedialMonLa);
        // ေ is drawn to the left; ာ to the right; ိ above; ု below.
        assert_eq!(cat('\u{1031}'), Category::VowelPre);
        assert_eq!(cat('\u{102C}'), Category::VowelPost);
        assert_eq!(cat('\u{102D}'), Category::VowelAbove);
        assert_eq!(cat('\u{102F}'), Category::VowelBelow);
        // ဲ and ံ are the grammar's `A`, the cantillation marks.
        assert_eq!(cat('\u{1032}'), Category::Cantillation);
        assert_eq!(cat('\u{1036}'), Category::Cantillation);
        // ့ is the dot below, which the grammar calls `DB` and the table `N`.
        assert_eq!(cat('\u{1037}'), Category::Nukta);
        // း is a syllable modifier; ၃ a digit, which heads a syllable as a
        // generic base.
        assert_eq!(cat('\u{1038}'), Category::SyllableModifier);
        assert_eq!(cat('\u{1041}'), Category::Placeholder);
        // ၣ is a pwo tone.
        assert_eq!(cat('\u{1063}'), Category::PwoTone);
        // A variation selector is the only non-Myanmar character the grammar
        // names.
        assert_eq!(cat('\u{FE00}'), Category::VariationSelector);
    }

    /// A consonant with a medial and a vowel is one syllable. Cutting it
    /// anywhere else puts the vowel on the wrong letter.
    #[test]
    fn a_consonant_cluster_is_one_syllable() {
        // က ျ ေ — KA, medial YA, vowel E.
        assert_eq!(
            cut("\u{1000}\u{103B}\u{1031}"),
            vec![(0, 3, Syllable::Consonant)]
        );
        // Two bare consonants do not join.
        assert_eq!(
            cut("\u{1000}\u{1001}"),
            vec![(0, 1, Syllable::Consonant), (1, 2, Syllable::Consonant)]
        );
    }

    /// A kinzi is part of the syllable that follows it, not one of its own.
    #[test]
    fn a_kinzi_heads_the_syllable_it_is_drawn_over() {
        // င ် ္ က — NGA, ASAT, virama, KA.
        assert_eq!(
            cut("\u{1004}\u{103A}\u{1039}\u{1000}"),
            vec![(0, 4, Syllable::Consonant)]
        );
    }

    /// A virama or a medial with nothing before it is broken text, which is
    /// what earns a dotted circle.
    #[test]
    fn a_mark_with_no_consonant_is_broken() {
        assert_eq!(cut("\u{1039}"), vec![(0, 1, Syllable::Broken)]);
        assert_eq!(cut("\u{103C}"), vec![(0, 1, Syllable::Broken)]);
        assert_eq!(cut("\u{1031}"), vec![(0, 1, Syllable::Broken)]);
    }

    /// The grammar lists `j | SMPst` *between* its two syllable rules, so a
    /// lone joiner is not a broken cluster and gets no dotted circle — even
    /// though `broken_cluster` would match it.
    #[test]
    fn a_lone_joiner_is_not_a_broken_cluster() {
        assert_eq!(cut("\u{200D}"), vec![(0, 1, Syllable::NonMyanmar)]);
        assert_eq!(cut("\u{200C}"), vec![(0, 1, Syllable::NonMyanmar)]);
    }

    /// Latin is one non-Myanmar syllable per character, which is what lets the
    /// shaper run over a mixed run without a separate test for script.
    #[test]
    fn characters_outside_the_script_are_left_alone() {
        assert_eq!(
            cut("ab"),
            vec![(0, 1, Syllable::NonMyanmar), (1, 2, Syllable::NonMyanmar)]
        );
        assert!(cut("").is_empty());
    }

    /// The ranges must tile the input exactly — contiguous, non-empty, and
    /// ending where the text does. Everything downstream assumes it.
    #[test]
    fn the_syllables_tile_every_string_they_are_given() {
        for text in [
            "\u{1019}\u{103C}\u{1014}\u{103A}\u{1019}\u{102C}",
            "\u{1000}\u{103B}\u{103C}\u{103D}\u{103E}",
            "\u{1021}\u{1004}\u{103A}\u{1039}\u{1002}",
            "\u{1000}\u{102D}\u{102F}\u{1037}",
            "a\u{1000}1\u{1039}\u{200D}\u{25CC}",
            "\u{1000}\u{1031}\u{FE00}",
            "\u{1041}\u{1042}\u{1043}",
            "\u{104A}\u{104B}",
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

    /// The stamp's low nibble round-trips under every serial the high nibble
    /// can hold, and a nibble no kind claims reads as the kind that does
    /// nothing.
    #[test]
    fn a_syllable_kind_survives_the_nibble_it_is_stored_in() {
        for serial in 0..16u8 {
            for kind in [
                Syllable::Consonant,
                Syllable::Broken,
                Syllable::NonMyanmar,
            ] {
                assert_eq!(Syllable::from_code((serial << 4) | kind.code()), kind);
            }
        }
        assert_eq!(Syllable::from_code(0), Syllable::Consonant);
        assert_eq!(Syllable::from_code(0x0F), Syllable::NonMyanmar);
    }

    /// The machine's rows are indexed by a category's discriminant, so a table
    /// generated against a different category list would silently mis-scan.
    #[test]
    fn the_machine_has_a_column_for_every_category() {
        assert!(!TRANSITIONS.is_empty(), "the machine has no states");
        assert_eq!(TRANSITIONS.len(), ACCEPTS.len());
        for row in &TRANSITIONS {
            assert_eq!(row.len(), Category::MedialMonLa as usize + 1);
        }
    }

    /// Only a `mym2` run reaches this shaper. `mymr` is the old spec, which
    /// HarfBuzz maps to the default shaper and so does this crate.
    #[test]
    fn only_new_spec_myanmar_is_shaped_here() {
        assert!(shapes(Some(ScriptTags::exactly(*b"mym2"))));
        for tag in [*b"mymr", *b"khmr", *b"dev2", *b"thai", *b"latn"] {
            assert!(!shapes(Some(ScriptTags::exactly(tag))));
        }
        assert!(!shapes(None));
    }

    fn run(text: &str) -> Vec<SubGlyph> {
        text.chars()
            .enumerate()
            .map(|(i, ch)| SubGlyph {
                indic: Char::of(ch),
                ..SubGlyph::cursive(u16::try_from(i).unwrap_or(0), i, None)
            })
            .collect()
    }

    fn order(glyphs: &[SubGlyph]) -> Vec<u16> {
        glyphs.iter().map(|g| g.gid).collect()
    }

    fn positions(glyphs: &[SubGlyph]) -> Vec<Position> {
        glyphs.iter().map(|g| g.indic.position).collect()
    }

    /// A pre-base vowel is stored after its consonant and drawn before it.
    #[test]
    fn a_pre_base_vowel_moves_in_front_of_its_consonant() {
        // က ေ — KA and vowel sign E.
        let mut glyphs = run("\u{1000}\u{1031}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![1, 0]);
        assert_eq!(positions(&glyphs), vec![Position::PreM, Position::BaseC]);
        // Both moved, so neither is a place a caret can point at.
        assert_eq!(glyphs[0].cluster, 0);
        assert_eq!(glyphs[1].cluster, 0);
    }

    /// A medial RA is drawn wrapped around the left of its consonant, so it
    /// moves in front of it — but stays behind a pre-base vowel, which is drawn
    /// further left still.
    #[test]
    fn a_medial_ra_moves_in_front_of_its_consonant() {
        // က ြ — KA and medial RA.
        let mut glyphs = run("\u{1000}\u{103C}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![1, 0]);
        assert_eq!(positions(&glyphs), vec![Position::PreC, Position::BaseC]);

        // က ြ ေ — and now the vowel goes in front of the medial.
        let mut glyphs = run("\u{1000}\u{103C}\u{1031}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![2, 1, 0]);
    }

    /// A kinzi is drawn above the letter that follows it, so it sorts to just
    /// after that letter rather than staying at the head.
    #[test]
    fn a_kinzi_sorts_to_just_after_its_base() {
        // င ် ္ က — NGA, ASAT, virama, KA.
        let mut glyphs = run("\u{1004}\u{103A}\u{1039}\u{1000}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![3, 0, 1, 2]);
        assert_eq!(
            positions(&glyphs),
            vec![
                Position::BaseC,
                Position::AfterMain,
                Position::AfterMain,
                Position::AfterMain,
            ]
        );
    }

    /// The base is the *first* consonant at or after the kinzi, found by a
    /// forward scan — not the last one, the way Indic's backward walk would
    /// have it.
    #[test]
    fn the_base_is_the_first_consonant() {
        // က ္ က — KA, virama, KA: a stacked pair. The first KA is the base.
        let mut glyphs = run("\u{1000}\u{1039}\u{1000}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![0, 1, 2]);
        assert_eq!(glyphs[0].indic.position, Position::BaseC);
    }

    /// A glyph the font ligated is no longer the character it was categorised
    /// from, so the base search steps over it.
    #[test]
    fn a_ligated_glyph_is_not_a_base() {
        let mut glyphs = run("\u{1000}\u{1000}");
        glyphs[0].lig = crate::gsub::Lig::at(1, 2, 0);
        reorder_consonant_syllable(&mut glyphs);
        // The second KA became the base, and the first was labelled `PreC`.
        assert_eq!(glyphs[0].indic.position, Position::PreC);
        assert_eq!(glyphs[1].indic.position, Position::BaseC);
    }

    /// The below-base group: a below-base vowel opens it, a cantillation mark
    /// sits just before it, and the first thing that is neither closes it.
    #[test]
    fn a_below_base_vowel_opens_the_below_base_group() {
        // က ု ံ ာ — KA, vowel U (below), anusvara (cantillation), vowel AA.
        let mut glyphs = run("\u{1000}\u{102F}\u{1036}\u{102C}");
        reorder_consonant_syllable(&mut glyphs);
        // The anusvara is labelled `BeforeSub`, which sorts it in front of the
        // vowel that opened the group.
        assert_eq!(order(&glyphs), vec![0, 2, 1, 3]);
        assert_eq!(
            positions(&glyphs),
            vec![
                Position::BaseC,
                Position::BeforeSub,
                Position::BelowC,
                Position::AfterSub,
            ]
        );
    }

    /// Everything the labelling gave the same position to keeps the order it
    /// was typed in, because the sort is stable. Two above-base vowels are the
    /// simplest case and the one a naive `sort_by` would still pass; two marks
    /// that had to move past a third are the case that pins the stability down.
    #[test]
    fn glyphs_at_one_position_keep_the_order_they_were_typed_in() {
        // က ိ ု ့ — KA, vowel I (above), vowel U (below), dot below.
        let mut glyphs = run("\u{1000}\u{102D}\u{102F}\u{1037}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![0, 1, 2, 3]);
    }

    /// Two pre-base vowels are drawn leftwards from the base, so the sequence
    /// as typed reads backwards once both are in front of it and has to be
    /// flipped.
    #[test]
    fn a_run_of_pre_base_vowels_is_flipped() {
        let mut glyphs = run("\u{1000}\u{1031}\u{1031}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![2, 1, 0]);
        // One alone is not flipped.
        let mut glyphs = run("\u{1000}\u{1031}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![1, 0]);
    }

    /// A variation selector takes the position of the glyph before it, so it
    /// travels with the character it selects a form of — including through the
    /// left-vowel flip, which reverses each vowel's own marks back.
    #[test]
    fn a_variation_selector_travels_with_what_it_selects() {
        // က ေ VS1 — the selector must stay after the vowel it applies to.
        let mut glyphs = run("\u{1000}\u{1031}\u{FE00}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![1, 2, 0]);
        assert_eq!(
            positions(&glyphs),
            vec![Position::PreM, Position::PreM, Position::BaseC]
        );
    }

    /// A syllable of one glyph is the base and nothing else.
    #[test]
    fn a_syllable_of_one_glyph_is_not_reordered() {
        let mut glyphs = run("\u{1000}");
        reorder_consonant_syllable(&mut glyphs);
        assert_eq!(order(&glyphs), vec![0]);
        assert_eq!(glyphs[0].indic.position, Position::BaseC);
    }

    /// A broken cluster gets a dotted circle at its head, and a well-formed one
    /// does not.
    #[test]
    fn a_broken_cluster_gains_a_dotted_circle() {
        let mut glyphs = run("\u{1000}\u{1039}");
        setup_syllables(&mut glyphs);
        assert_eq!(Syllable::from_code(glyphs[0].syllable), Syllable::Consonant);
        reorder(&mut glyphs, Some(99));
        assert_eq!(glyphs.len(), 2, "nothing to fix");

        // A lone medial RA is not well formed — and once the circle is in front
        // of it, the medial moves in front of the circle.
        let mut glyphs = run("\u{103C}");
        setup_syllables(&mut glyphs);
        assert_eq!(Syllable::from_code(glyphs[0].syllable), Syllable::Broken);
        reorder(&mut glyphs, Some(99));
        assert_eq!(glyphs.len(), 2);
        assert_eq!(order(&glyphs), vec![0, 99]);
        assert_eq!(glyphs[0].indic.category, Category::MedialRa);
        assert_eq!(glyphs[1].indic.position, Position::BaseC);
        assert_eq!(glyphs[0].cluster, glyphs[1].cluster);
    }

    /// A face with no U+25CC gets no circle, which is the honest answer: there
    /// is nothing to draw.
    #[test]
    fn a_face_without_a_dotted_circle_gains_no_glyphs() {
        let mut glyphs = run("\u{1039}");
        setup_syllables(&mut glyphs);
        reorder(&mut glyphs, None);
        assert_eq!(glyphs.len(), 1);
    }

    /// A non-Myanmar syllable is never reordered and never given a circle,
    /// which is what lets a mixed run go through one shaper.
    #[test]
    fn a_latin_run_passes_through_untouched() {
        let mut glyphs = run("ab");
        let before = order(&glyphs);
        let masks: Vec<u64> = glyphs.iter().map(|g| g.mask).collect();
        setup_syllables(&mut glyphs);
        reorder(&mut glyphs, Some(99));
        assert_eq!(order(&glyphs), before);
        assert!(glyphs.iter().map(|g| g.mask).eq(masks));
        // And their positions are untouched too, which is what says the
        // labelling pass never ran over them.
        assert!(glyphs.iter().all(|g| g.indic.position == Position::End));
    }
}

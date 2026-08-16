//! The Khmer shaper.
//!
//! Khmer is written in syllables like the Indic scripts, is categorised from
//! the same table ([`indic::Category`](crate::indic::Category)), and is
//! segmented by a machine built by the same generator — and it is a *different*
//! shaper, because what it does with a syllable is not what the Indic shaper
//! does. Three differences, and each of them is the reason for a piece of this
//! module:
//!
//! * **There is no base to find.** The Indic shaper's central act is a backward
//!   walk looking for the consonant a syllable is built around, because the
//!   answer decides where every other character goes. Khmer has no such walk.
//!   Everything after the first glyph is offered `blwf`, `abvf` and `pstf`
//!   together and the font decides; the shaper moves exactly two things, and
//!   only ever to the very front.
//! * **There is no final reordering.** Indic reorders twice, because it has to
//!   guess before the font has spoken and then correct itself. Khmer reorders
//!   once — and does it *before any lookup has run at all*, which is the
//!   single most surprising fact about this shaper. See [`shape`].
//! * **The vowel's position is in its category.** Indic asks the table for a
//!   matra's [`Position`](crate::indic::Position); the Khmer grammar has no
//!   position axis and names four distinct vowel categories instead. The
//!   generator folds one into the other, so nothing here reads a position.
//!
//! # The two moves
//!
//! Both are to the head of the syllable, and both are from the Microsoft
//! Devanagari/Khmer script-development notes rather than from Unicode:
//!
//! 1. **COENG + RO.** A subjoined RO is drawn to the *left* of the whole
//!    cluster, not underneath it, so the pair is moved to the front and marked
//!    `pref`. Everything it jumped over is then marked `cfar`, which is how a
//!    font tells `ង + ្រ + ្គ` from `ង + ្គ + ្រ` once both have a `្រ` at the
//!    front.
//! 2. **A pre-base vowel.** `VPre` is drawn to the left of its consonant and
//!    stored after it, exactly as an Indic left matra is — but unlike Indic's,
//!    it is moved once and never revisited.
//!
//! # Split vowels
//!
//! Five Khmer vowel signs are drawn as two marks in two places and have **no
//! Unicode decomposition**, so nothing in normalisation takes them apart.
//! [`split_matras`] is the pass that does, transcribed from HarfBuzz's
//! `decompose_khmer`; [`present`] is the cheap test that lets every other
//! string skip it.

use alloc::vec::Vec;

use crate::gsub::{ALL_FEATURES, SubGlyph, Substitutions, feature_bit, feature_bits};
use crate::indic::Category;
use crate::khmer_machine::{ACCEPTS, TRANSITIONS};
use crate::lang::Lang;
use crate::norm::Piece;
use crate::script::ScriptTags;
use crate::syllabic;

/// Which of the grammar's three rules matched a syllable.
///
/// Three and not Indic's seven, because the Khmer grammar has no separate rule
/// for an independent vowel, a symbol or a standalone cluster: a `V` is a
/// consonant as far as the grammar is concerned, and a placeholder or a dotted
/// circle heads an ordinary consonant syllable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Syllable {
    /// A consonant and everything hung on it. The ordinary case.
    Consonant,
    /// Not a well-formed syllable: a coeng or a vowel sign with no consonant
    /// to attach to. Reordered exactly like a consonant syllable, because by
    /// the time the reordering runs a dotted circle has been inserted and it
    /// *is* one.
    Broken,
    /// Not Khmer at all. Never reordered, and never given a dotted circle.
    NonKhmer,
}

impl Syllable {
    /// This kind as the low nibble of a [`SubGlyph::syllable`] byte.
    ///
    /// The numbering is HarfBuzz's `khmer_syllable_type_t`, so that a value
    /// read out of one of its traces means the same thing here. See
    /// [`syllabic`](crate::syllabic) for why a syllable is remembered as a
    /// stamp on each glyph rather than as a range.
    #[must_use]
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Consonant => 0,
            Self::Broken => 1,
            Self::NonKhmer => 2,
        }
    }

    /// The kind [`code`](Self::code) writes as `code`'s low nibble.
    ///
    /// A nibble no kind uses reads back as [`NonKhmer`](Self::NonKhmer), the
    /// kind that does nothing. Note that zero is *not* such a nibble — it is
    /// [`Consonant`](Self::Consonant), the kind that does the most — so an
    /// unstamped glyph would read back as a syllable to reorder rather than as
    /// one to leave alone. Nothing here relies on a lenient default: the
    /// ranges [`syllables`] produces tile the run exactly, so
    /// [`syllabic::stamp`](crate::syllabic::stamp) writes every glyph, and the
    /// serial in the high nibble is never zero, so a zero byte cannot be
    /// mistaken for a stamped one either.
    #[must_use]
    pub(crate) fn from_code(code: u8) -> Self {
        match code & 0x0F {
            0 => Self::Consonant,
            1 => Self::Broken,
            _ => Self::NonKhmer,
        }
    }
}

/// Whether a run of `tags` is Khmer, and so shaped here.
///
/// Khmer has one OpenType tag and no old-spec/new-spec pair, so unlike the
/// Indic scripts there is nothing to choose between: `khmr` is the whole
/// answer. It is also in
/// [`fallback::ALWAYS_COMPLEX`](crate::fallback), so — again unlike Indic —
/// a face that files its features under `DFLT` or `latn` does not call this
/// shaper off. HarfBuzz makes the same exception, and the reason is that
/// Khmer's reordering is not optional decoration: text with an unmoved `VPre`
/// is not merely plainer, it is in the wrong order.
#[must_use]
pub(crate) fn shapes(tags: Option<ScriptTags>) -> bool {
    tags.is_some_and(|tags| tags.preferred == *b"khmr")
}

/// The five vowel signs drawn as two marks that Unicode does not decompose,
/// each paired with the second half it splits into.
///
/// The first half is always U+17C1 KHMER VOWEL SIGN E, and the second half is
/// the character itself — the sign keeps its own code point for the part of it
/// that is not the E. That looks like a decomposition that does not terminate
/// and is not one: the pass below is applied once, to the text, and never
/// recursed into.
///
/// From HarfBuzz's `decompose_khmer`. Unicode has no canonical decomposition
/// for any of them, which is why this table exists rather than a call into
/// [`norm`](crate::norm).
const SPLIT: [char; 5] = ['\u{17BE}', '\u{17BF}', '\u{17C0}', '\u{17C4}', '\u{17C5}'];

/// The first half every one of [`SPLIT`] splits into.
const SPLIT_HEAD: char = '\u{17C1}';

/// Whether `text` has anything for [`split_matras`] to do.
///
/// Asked of the string rather than the pieces so that the whole pass can be
/// skipped without allocating, which is the case for every string that is not
/// Khmer. All five characters are `Mc` at combining class 0 with no
/// decomposition, so a string containing nothing else is one that
/// [`norm::needs_work`](crate::norm) correctly calls already normalized — this
/// is a second, separate reason to do the work, not a refinement of that one.
/// Exactly the relationship [`thai::present`](crate::thai) has to it.
#[must_use]
pub(crate) fn present(text: &str) -> bool {
    text.chars().any(|ch| SPLIT.contains(&ch))
}

/// Replace every split vowel sign the face can draw both halves of with those
/// two halves.
///
/// **Both** halves: HarfBuzz's normalizer requires a glyph for the tail before
/// it will call the shaper's decomposition useful at all, and then — because
/// Khmer asks for `COMPOSED_DIACRITICS_NO_SHORT_CIRCUIT`, so the composed
/// glyph is never taken as a reason to stop — requires one for the head as
/// well before emitting the pair. A face with a `ើ` but no `េ` keeps the whole
/// character, which is right: splitting there would trade one drawable glyph
/// for a drawable one and a box.
///
/// The two pieces share the original's cluster, so the split is invisible to a
/// caret.
///
/// Order-independent with respect to the canonical sort and the recomposition
/// around it, which is why this may run after [`norm::pieces`](crate::norm)
/// has finished rather than inside it: every character involved is at
/// combining class 0, so the sort cannot move one, and none of them takes part
/// in any canonical composition, so `compose` cannot put one back together.
/// HarfBuzz needs a `compose_khmer` that refuses to recompose a mark for
/// exactly the case this crate does not have.
pub(crate) fn split_matras(pieces: &mut Vec<Piece>, has_glyph: impl Fn(char) -> bool) {
    if !pieces.iter().any(|&(ch, _)| SPLIT.contains(&ch)) {
        return;
    }
    if !has_glyph(SPLIT_HEAD) {
        return;
    }
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len().saturating_add(2));
    for &(ch, cluster) in pieces.iter() {
        if SPLIT.contains(&ch) && has_glyph(ch) {
            out.push((SPLIT_HEAD, cluster));
        }
        out.push((ch, cluster));
    }
    *pieces = out;
}

/// Cut `cats` into syllables, appending `(start, end, kind)` to `out`.
///
/// The same scan as [`indic::syllables`](crate::indic::syllables) over a
/// different machine, and with the same two guarantees: the ranges tile the
/// input exactly, because the grammar's last rule matches one character of any
/// category whatsoever; and longest match wins with ties going to the rule the
/// grammar lists first.
pub(crate) fn syllables(cats: &[Category], out: &mut Vec<(usize, usize, Syllable)>) {
    out.clear();
    let mut i = 0usize;
    while i < cats.len() {
        // State 1 is the start; state 0 is dead. See `khmer_machine`.
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
        let (end, kind) = best.unwrap_or((i.saturating_add(1), Syllable::NonKhmer));
        out.push((i, end, kind));
        i = end;
    }
}

/// The five per-glyph features, as this face's mask bits.
///
/// A bit is zero when the face reaches no lookup through that feature, which is
/// HarfBuzz's `get_1_mask` and is load-bearing rather than an optimisation: the
/// `cfar` marking is explicitly conditional on it upstream, and setting a bit
/// for a feature the face does not have would let it select another feature's
/// lookups if the two ever shared one.
#[derive(Clone, Copy, Debug, Default)]
struct Masks {
    /// `pref` — the COENG + RO pair, once it has been moved to the front.
    pref: u64,
    /// `blwf`, `abvf` and `pstf` together: every glyph after the first is
    /// offered all three at once and the font picks. Khmer never asks which of
    /// the three a consonant takes, which is the whole of why it needs no base
    /// search.
    post: u64,
    /// `cfar` — everything the COENG + RO pair jumped over.
    cfar: u64,
}

impl Masks {
    /// What `subs` offers a run of `tags` in `lang`.
    ///
    /// Every bit is zero for a face with no `GSUB`, which is the honest answer:
    /// there is nothing for a mask to select. The reordering still happens —
    /// see [`shape`].
    fn new(subs: Option<&Substitutions>, tags: Option<ScriptTags>, lang: Option<Lang>) -> Self {
        let Some(subs) = subs else {
            return Self::default();
        };
        let bit = |tag: &[u8; 4]| subs.feature_mask(tags, lang, tag);
        Self {
            pref: bit(b"pref"),
            post: bit(b"blwf") | bit(b"abvf") | bit(b"pstf"),
            cfar: bit(b"cfar"),
        }
    }
}

/// The features applied together, constrained to one syllable, before the
/// syllable boundaries are forgotten.
///
/// `locl` and `ccmp` are in the list rather than in a stage of their own
/// because Uniscribe does not pause between them and the basic features, and
/// pausing changes the answer. HarfBuzz's comment names the test: `KhmerUI.ttf`
/// with `U+1789,U+17BC`, `U+1789,U+17D2,U+1789` and
/// `U+1789,U+17D2,U+1789,U+17BC`.
/// <https://github.com/harfbuzz/harfbuzz/issues/974>
const BASIC: [&[u8; 4]; 7] = [
    b"locl", b"ccmp", b"pref", b"blwf", b"abvf", b"pstf", b"cfar",
];

/// The four features every glyph in the run is eligible for.
///
/// The five of [`Masks`] are absent because they are handed out per glyph by
/// the reordering, which is the whole mechanism by which a font is told *which*
/// consonant to draw subjoined. HarfBuzz's flag is `F_GLOBAL_MANUAL_JOINERS`
/// for these four and `F_MANUAL_JOINERS` for those five, and the split is
/// exactly the same one. (`F_MANUAL_JOINERS` itself is a no-op here: it turns
/// off HarfBuzz's automatic skipping of ZWJ and ZWNJ, and this crate's
/// [`skip`](crate::skip) never had it.)
const GLOBAL: [&[u8; 4]; 4] = [b"pres", b"abvs", b"blws", b"psts"];

/// Shape one run of Khmer text: HarfBuzz's Khmer shaper, end to end.
///
/// `glyphs` is one script run, one glyph per character, as `cmap` produced it
/// and with [`SubGlyph::indic`] already set from the character. It comes back
/// reordered and substituted, and may be longer: a broken cluster gains a
/// dotted circle.
///
/// # The reordering runs before every lookup
///
/// This is the one thing that cannot be guessed from the feature list, and
/// getting it wrong would be invisible in a table dump and wrong on screen.
/// HarfBuzz registers both of Khmer's pauses — segment, then reorder — *before*
/// it enables `locl` and `ccmp`, and a pause both opens a stage and runs after
/// the (empty) stage it closes. So the compiled plan is: two stages with no
/// lookups in them at all, whose pauses do the segmentation and the reordering;
/// then a stage with the seven features of [`BASIC`]; then everything else.
///
/// Indic is not like this — there `locl` and `ccmp` run first and the initial
/// reordering is what follows them — and copying Indic's shape here would run
/// the font's own `ccmp` against a syllable whose `VPre` had not yet moved.
///
/// `subs` may be `None`. Reordering is not conditional on the font having
/// substitutions — a pre-base vowel is written after its consonant and drawn
/// before it whatever the font does — so a face with no `GSUB` still gets the
/// reordering, with every mask it would have set left empty.
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
    let masks = Masks::new(subs, tags, lang);
    let global = feature_bits(&GLOBAL);
    for g in glyphs.iter_mut() {
        g.mask |= global;
    }
    setup_syllables(glyphs);
    reorder(&masks, glyphs, glyph('\u{25CC}'));

    let Some(subs) = subs else {
        // No lookups to run, so no stages either. The reordering above has
        // already happened, which is everything this shaper does without a
        // font's help.
        return;
    };
    let stages = stages();
    subs.apply_stages(
        data,
        tags,
        lang,
        &stages,
        feature_bits(&BASIC),
        glyphs,
        |stage, glyphs| {
            if stage == 0 {
                // The syllables have done their work and the four global
                // features must not be confined by them.
                // https://github.com/harfbuzz/harfbuzz/issues/3531
                syllabic::clear(glyphs);
            }
        },
    );
}

/// The two passes the lookups are applied in, as sets of feature bits.
///
/// The seven of [`BASIC`] together and confined to a syllable, then everything
/// else at once. "Everything else" is every feature this crate knows rather
/// than just the four of [`GLOBAL`], because the ordinary features — `rlig`,
/// `calt`, `clig`, `rclt`, and the positioning ones a face may have filed under
/// `GSUB` — belong to that last stage too: HarfBuzz adds them after the
/// shaper's own and they land in whatever stage is open, which is this one.
/// `liga` is the exception, switched off for Khmer outright by
/// `override_features_khmer`; `clig` is switched *on* there, and is on here
/// already because this crate never turns it off.
///
/// `BASIC` is masked *out* of the last stage, and has to be: **a feature
/// belongs to exactly one stage.** HarfBuzz gets that from its map builder,
/// which merges the two entries a tag can pick up — `locl` and `ccmp` are named
/// both by this shaper and by the common features every run gets — and keeps
/// the *lower* stage. Leave them in both and every lookup they reach runs a
/// second time. On a real face that is usually invisible, because the second
/// pass looks at glyphs the first already rewrote and matches nothing; on a
/// face whose features announce themselves it is immediate, and
/// `tools/gen_khmer_probe.py` is that face.
///
/// A lookup reached by *two* features in different stages still runs twice, and
/// should: the masks differ, and that is HarfBuzz's behaviour too. What must not
/// happen is one feature running in two stages.
fn stages() -> [u64; 2] {
    let basic = feature_bits(&BASIC);
    [basic, ALL_FEATURES & !basic & !feature_bit(b"liga")]
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
/// Both halves of HarfBuzz's `reorder_khmer`, and in its order: the circle has
/// to be in place before a broken cluster is reordered, because reordering
/// treats a broken cluster as a consonant syllable and the circle is the
/// consonant.
fn reorder(masks: &Masks, glyphs: &mut Vec<SubGlyph>, dotted: Option<u16>) {
    syllabic::insert_dotted_circles(
        glyphs,
        dotted,
        Category::DottedCircle,
        |stamp| Syllable::from_code(stamp) == Syllable::Broken,
        // No skip. Indic's circle goes after a repha, which is drawn above the
        // letter that follows and so needs a letter to follow; Khmer has no
        // repha, and HarfBuzz passes `-1` for the category here.
        |_| false,
    );
    syllabic::for_each(glyphs, |stamp, _, syllable| {
        if Syllable::from_code(stamp) != Syllable::NonKhmer {
            reorder_consonant_syllable(masks, syllable);
        }
    });
}

/// The category `glyphs[i]` counts as, or [`Category::Other`] past the end.
fn cat_at(glyphs: &[SubGlyph], i: usize) -> Category {
    glyphs.get(i).map_or(Category::Other, |g| g.indic.category)
}

/// Lay out one syllable: HarfBuzz's `reorder_consonant_syllable`.
///
/// `glyphs` is exactly the syllable, and comes out the same length — this only
/// permutes and marks.
///
/// Both moves rotate a prefix of the syllable rather than shuffling glyphs one
/// at a time, which is the same permutation HarfBuzz's `memmove` performs. The
/// scan then carries on from the *next index*, over an array whose contents
/// have shifted underneath it. That is odd and it is deliberate: it is what
/// upstream does, so a second COENG in the same syllable is examined at the
/// position the first move left it, and reproducing the behaviour means
/// reproducing the walk.
fn reorder_consonant_syllable(masks: &Masks, glyphs: &mut [SubGlyph]) {
    let end = glyphs.len();
    // Every glyph but the first is offered all three of the subjoined forms
    // at once. This is where Khmer's base search would be, if it had one.
    for g in glyphs.iter_mut().skip(1) {
        g.mask |= masks.post;
    }

    let mut coengs = 0u32;
    let mut i = 1usize;
    while i < end {
        let next = i.saturating_add(1);
        // """When a COENG + (Cons | IndV) combination are found (and subscript
        // count is less than two) the character combination is handled
        // according to the subscript type of the character following the
        // COENG. ... Subscript Type 2 — The COENG + RO characters are reordered
        // to immediately before the base glyph. Then the COENG + RO characters
        // are assigned to have the 'pref' OpenType feature applied to them."""
        if cat_at(glyphs, i) == Category::Halant && coengs <= 2 && next < end {
            coengs = coengs.saturating_add(1);
            if cat_at(glyphs, next) == Category::Ra {
                let through = next.saturating_add(1);
                for g in glyphs.get_mut(i..through).unwrap_or_default() {
                    g.mask |= masks.pref;
                }
                // Mark what the pair is about to jump over with `cfar`. This is
                // what lets a Khmer font tell these two apart, both of which
                // end up with a `្រ` at the head:
                //   U+1784,U+17D2,U+179A,U+17D2,U+1782
                //   U+1784,U+17D2,U+1782,U+17D2,U+179A
                // Read before the rotation, because the rotation only touches
                // the prefix `..through` and everything from `through` on keeps
                // its index — so either order would do, and this one says why.
                if masks.cfar != 0 {
                    for g in glyphs.get_mut(through..end).unwrap_or_default() {
                        g.mask |= masks.cfar;
                    }
                }
                if let Some(head) = glyphs.get_mut(..through) {
                    syllabic::merge_clusters(head);
                    head.rotate_right(2);
                }
                coengs = 2; // Done: no later COENG moves.
            }
        }
        // A pre-base vowel is drawn to the left of the syllable and stored to
        // its right, so it moves to the front — once, and unlike Indic's left
        // matra it is never brought back.
        else if cat_at(glyphs, i) == Category::VowelPre
            && let Some(head) = glyphs.get_mut(..next)
        {
            syllabic::merge_clusters(head);
            head.rotate_right(1);
        }
        i = next;
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
    use crate::indic::Char;
    use alloc::vec;

    /// No feature may appear in both stages. A lookup a feature reaches is
    /// applied once per stage that names it, so a tag left in both the basic
    /// stage and the catch-all one runs its lookups twice. `KhmerProbe.ttf` is
    /// the face that caught this, by reporting `locl`, `ccmp` and the three
    /// `blwf`/`abvf`/`pstf` markers twice on every glyph; this is the
    /// assertion that keeps `stages`'s `& !basic` from being deleted as
    /// redundant.
    #[test]
    fn no_feature_is_applied_in_two_stages() {
        let [first, last] = stages();
        assert_eq!(first & last, 0);
        assert_eq!(first, feature_bits(&BASIC));
        assert_eq!(first | last, ALL_FEATURES & !feature_bit(b"liga"));
    }

    /// The four global features run in the *last* stage, not the basic one:
    /// they are added after the pause that clears the syllable stamps, so a
    /// rule in one of them may match across a syllable boundary.
    #[test]
    fn the_global_features_run_after_the_syllables_are_forgotten() {
        let [first, last] = stages();
        let global = feature_bits(&GLOBAL);
        assert_eq!(first & global, 0);
        assert_eq!(last & global, global);
    }

    fn cut(text: &str) -> Vec<(usize, usize, Syllable)> {
        let cats: Vec<Category> = text.chars().map(|ch| Char::of(ch).category).collect();
        let mut out = Vec::new();
        syllables(&cats, &mut out);
        out
    }

    /// The categories the grammar is written in terms of have to be the ones
    /// the table actually produces for Khmer, or every rule below is about
    /// characters that never arrive.
    #[test]
    fn the_khmer_block_is_categorised_the_way_the_grammar_names_it() {
        let cat = |ch: char| Char::of(ch).category;
        // ក KA — a consonant, and the thing a syllable is built on.
        assert_eq!(cat('\u{1780}'), Category::Consonant);
        // រ RO — not merely a consonant: a COENG + RO pair moves to the front.
        assert_eq!(cat('\u{179A}'), Category::Ra);
        // ្ COENG — the grammar's `H`.
        assert_eq!(cat('\u{17D2}'), Category::Halant);
        // The four vowels, one per drawn position.
        assert_eq!(cat('\u{17C1}'), Category::VowelPre);
        assert_eq!(cat('\u{17B6}'), Category::VowelPost);
        assert_eq!(cat('\u{17B7}'), Category::VowelAbove);
        assert_eq!(cat('\u{17BB}'), Category::VowelBelow);
        // ៌ robat, and the two register shifters Unicode files separately.
        assert_eq!(cat('\u{17CC}'), Category::Robatic);
        assert_eq!(cat('\u{17C9}'), Category::Robatic);
        assert_eq!(cat('\u{17CA}'), Category::Robatic);
        // ំ nikahit may repeat between the vowels; ះ reahmuk only trails.
        assert_eq!(cat('\u{17C6}'), Category::XGroup);
        assert_eq!(cat('\u{17C7}'), Category::YGroup);
        // ឥ QI is an independent vowel letter, which the grammar calls `V` and
        // treats exactly as a consonant. អ QA looks like one and is named like
        // one, but Unicode files it as a consonant letter and so does the
        // table — the independent vowels start one code point later.
        assert_eq!(cat('\u{17A5}'), Category::Vowel);
        assert_eq!(cat('\u{17A2}'), Category::Consonant);
    }

    /// A consonant with a subjoined consonant and a vowel is one syllable.
    /// Cutting it anywhere else moves the vowel onto the wrong letter.
    #[test]
    fn a_consonant_cluster_is_one_syllable() {
        // ខ្មែ — KHA, COENG, MO, vowel sign E.
        assert_eq!(
            cut("\u{1781}\u{17D2}\u{1798}\u{17C2}"),
            vec![(0, 4, Syllable::Consonant)]
        );
        // Two bare consonants do not join.
        assert_eq!(
            cut("\u{1780}\u{1781}"),
            vec![(0, 1, Syllable::Consonant), (1, 2, Syllable::Consonant)]
        );
    }

    /// A coeng with nothing before it is broken text, which is what earns a
    /// dotted circle.
    #[test]
    fn a_coeng_with_no_consonant_is_broken() {
        assert_eq!(cut("\u{17D2}"), vec![(0, 1, Syllable::Broken)]);
        // And a vowel sign alone, likewise.
        assert_eq!(cut("\u{17B7}"), vec![(0, 1, Syllable::Broken)]);
    }

    /// Latin is one non-Khmer syllable per character, which is what lets the
    /// shaper run over a mixed run without a separate test for script.
    #[test]
    fn characters_outside_the_script_are_left_alone() {
        assert_eq!(
            cut("ab"),
            vec![(0, 1, Syllable::NonKhmer), (1, 2, Syllable::NonKhmer)]
        );
        assert!(cut("").is_empty());
    }

    /// The ranges must tile the input exactly — contiguous, non-empty, and
    /// ending where the text does. Everything downstream assumes it.
    #[test]
    fn the_syllables_tile_every_string_they_are_given() {
        for text in [
            "\u{1781}\u{17D2}\u{1798}\u{17C2}\u{179A}",
            "\u{1784}\u{17D2}\u{179A}\u{17D2}\u{1782}",
            "\u{1780}\u{17BE}\u{17C6}\u{17C7}",
            "a\u{1780}1\u{17D2}\u{200D}\u{25CC}",
            "\u{17CC}\u{17D2}\u{1780}",
            "\u{17D9}\u{17B6}",
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
            for kind in [Syllable::Consonant, Syllable::Broken, Syllable::NonKhmer] {
                assert_eq!(Syllable::from_code((serial << 4) | kind.code()), kind);
            }
        }
        // Zero reads back as `Consonant`, not as `NonKhmer`: HarfBuzz numbers
        // the consonant syllable 0, so an unstamped glyph would be reordered
        // rather than skipped. `syllables` tiles the whole run, so no glyph is
        // ever left unstamped — see `Syllable::from_code`.
        assert_eq!(Syllable::from_code(0), Syllable::Consonant);
        assert_eq!(Syllable::from_code(0x0F), Syllable::NonKhmer);
    }

    /// The machine's rows are indexed by a category's discriminant, so a table
    /// generated against a different category list would silently mis-scan.
    #[test]
    fn the_machine_has_a_column_for_every_category() {
        assert!(!TRANSITIONS.is_empty(), "the machine has no states");
        assert_eq!(TRANSITIONS.len(), ACCEPTS.len());
        for row in &TRANSITIONS {
            assert_eq!(row.len(), Category::YGroup as usize + 1);
        }
    }

    fn split(text: &str, has: impl Fn(char) -> bool) -> Vec<Piece> {
        let mut pieces: Vec<Piece> = text.char_indices().map(|(at, ch)| (ch, at)).collect();
        split_matras(&mut pieces, has);
        pieces
    }

    /// A split vowel comes apart when the face has both halves, and both
    /// pieces are charged to the character's own offset.
    #[test]
    fn a_split_vowel_becomes_two_marks_on_one_cluster() {
        // ក + ើ — KA and vowel sign OE.
        let out = split("\u{1780}\u{17BE}", |_| true);
        assert_eq!(
            out,
            vec![('\u{1780}', 0), ('\u{17C1}', 3), ('\u{17BE}', 3)]
        );
    }

    /// And does not when either half is missing: one drawable glyph beats a
    /// drawable one and a missing-glyph box.
    #[test]
    fn a_split_vowel_stays_whole_when_the_face_lacks_a_half() {
        for missing in ['\u{17C1}', '\u{17BE}'] {
            assert_eq!(
                split("\u{1780}\u{17BE}", |ch| ch != missing),
                vec![('\u{1780}', 0), ('\u{17BE}', 3)],
                "missing {missing:?}"
            );
        }
    }

    /// [`present`] gates the pass, so it has to agree with it about every one
    /// of the five — and say no to everything else, including the halves.
    #[test]
    fn present_names_exactly_the_characters_that_split() {
        for ch in SPLIT {
            let mut text = alloc::string::String::new();
            text.push(ch);
            assert!(present(&text), "{ch:?}");
            assert_eq!(split(&text, |_| true).len(), 2, "{ch:?}");
        }
        for ch in ['\u{17C1}', '\u{17BB}', '\u{1780}', 'a', '\u{0E33}'] {
            let mut text = alloc::string::String::new();
            text.push(ch);
            assert!(!present(&text), "{ch:?}");
        }
    }

    /// Only a `khmr` run reaches this shaper.
    #[test]
    fn only_khmer_is_shaped_here() {
        assert!(shapes(Some(ScriptTags::exactly(*b"khmr"))));
        for tag in [*b"dev2", *b"thai", *b"mym2", *b"latn"] {
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

    /// Every mask bit set, so that a reordering test can see which glyph was
    /// marked without needing a font.
    fn all_masks() -> Masks {
        Masks {
            pref: feature_bit(b"pref"),
            post: feature_bits(&[b"blwf", b"abvf", b"pstf"]),
            cfar: feature_bit(b"cfar"),
        }
    }

    /// A COENG + RO pair moves to the front of the syllable, takes `pref`, and
    /// leaves `cfar` on everything it jumped over.
    #[test]
    fn a_subjoined_ro_moves_to_the_head_of_its_syllable() {
        // ង ្ រ ្ គ — NGO, COENG, RO, COENG, KO.
        let mut glyphs = run("\u{1784}\u{17D2}\u{179A}\u{17D2}\u{1782}");
        reorder_consonant_syllable(&all_masks(), &mut glyphs);
        assert_eq!(order(&glyphs), vec![1, 2, 0, 3, 4]);
        let pref = feature_bit(b"pref");
        assert_ne!(glyphs[0].mask & pref, 0, "the COENG takes pref");
        assert_ne!(glyphs[1].mask & pref, 0, "the RO takes pref");
        assert_eq!(glyphs[2].mask & pref, 0, "the base does not");
        // `cfar` on what the pair jumped over: the trailing COENG + KO, which
        // the rotation left where it was.
        let cfar = feature_bit(b"cfar");
        assert_ne!(glyphs[3].mask & cfar, 0);
        assert_ne!(glyphs[4].mask & cfar, 0);
        assert_eq!(glyphs[0].mask & cfar, 0);
        // Everything that moved shares one cluster, since a caret has no
        // honest place to stop inside it.
        assert_eq!(glyphs[0].cluster, 0);
        assert_eq!(glyphs[1].cluster, 0);
        assert_eq!(glyphs[2].cluster, 0);
    }

    /// The mirror image — a subjoined KO before a subjoined RO — must come out
    /// differently, which is the whole reason `cfar` exists.
    #[test]
    fn the_two_orders_of_two_subjoined_consonants_stay_distinct() {
        let a = {
            let mut g = run("\u{1784}\u{17D2}\u{179A}\u{17D2}\u{1782}");
            reorder_consonant_syllable(&all_masks(), &mut g);
            (order(&g), g[0].mask)
        };
        let b = {
            let mut g = run("\u{1784}\u{17D2}\u{1782}\u{17D2}\u{179A}");
            reorder_consonant_syllable(&all_masks(), &mut g);
            (order(&g), g[0].mask)
        };
        assert_ne!(a.0, b.0);
    }

    /// A subjoined consonant that is not RO does not move.
    #[test]
    fn an_ordinary_subjoined_consonant_stays_where_it_was() {
        // ខ ្ ម — KHA, COENG, MO.
        let mut glyphs = run("\u{1781}\u{17D2}\u{1798}");
        reorder_consonant_syllable(&all_masks(), &mut glyphs);
        assert_eq!(order(&glyphs), vec![0, 1, 2]);
        let post = feature_bits(&[b"blwf", b"abvf", b"pstf"]);
        assert_eq!(glyphs[0].mask & post, 0, "the first glyph is not subjoined");
        assert_ne!(glyphs[2].mask & post, 0);
    }

    /// A pre-base vowel is stored after its consonant and drawn before it.
    #[test]
    fn a_pre_base_vowel_moves_in_front_of_its_consonant() {
        // ក ែ — KA and vowel sign AE, which is drawn to the left.
        let mut glyphs = run("\u{1780}\u{17C2}");
        reorder_consonant_syllable(&all_masks(), &mut glyphs);
        assert_eq!(order(&glyphs), vec![1, 0]);
        assert_eq!(glyphs[0].cluster, 0);
        assert_eq!(glyphs[1].cluster, 0);
        // And the half of a split vowel that is a `VPre` moves the same way,
        // which is why the split has to happen before the shaper runs.
        let mut glyphs = run("\u{1780}\u{17C1}\u{17BE}");
        reorder_consonant_syllable(&all_masks(), &mut glyphs);
        assert_eq!(order(&glyphs), vec![1, 0, 2]);
    }

    /// A vowel with nothing before it is left alone: the scan starts at the
    /// second glyph, so a syllable of one is never permuted.
    #[test]
    fn a_syllable_of_one_glyph_is_not_reordered() {
        let mut glyphs = run("\u{17C2}");
        reorder_consonant_syllable(&all_masks(), &mut glyphs);
        assert_eq!(order(&glyphs), vec![0]);
    }

    /// A face that offers none of the five features gets the moves and no
    /// masks — the reordering is this crate's job, not the font's.
    #[test]
    fn a_face_with_no_features_is_still_reordered() {
        let mut glyphs = run("\u{1780}\u{17C2}");
        reorder_consonant_syllable(&Masks::default(), &mut glyphs);
        assert_eq!(order(&glyphs), vec![1, 0]);
        assert!(glyphs.iter().all(|g| g.mask == glyphs[0].mask));
    }

    /// A broken cluster gets a dotted circle at its head, and a well-formed
    /// one does not.
    #[test]
    fn a_broken_cluster_gains_a_dotted_circle() {
        let mut glyphs = run("\u{1780}\u{17D2}");
        setup_syllables(&mut glyphs);
        // ក ្ is a consonant syllable — the trailing coeng belongs to it.
        assert_eq!(Syllable::from_code(glyphs[0].syllable), Syllable::Consonant);
        reorder(&all_masks(), &mut glyphs, Some(99));
        assert_eq!(glyphs.len(), 2, "nothing to fix");

        // A lone coeng is not.
        let mut glyphs = run("\u{17D2}\u{1780}\u{17D2}");
        setup_syllables(&mut glyphs);
        assert_eq!(Syllable::from_code(glyphs[0].syllable), Syllable::Broken);
        reorder(&all_masks(), &mut glyphs, Some(99));
        assert_eq!(glyphs.len(), 4);
        assert_eq!(glyphs[0].gid, 99);
        assert_eq!(glyphs[0].cluster, glyphs[1].cluster);
        assert_eq!(glyphs[0].syllable, glyphs[1].syllable);
        assert_eq!(glyphs[0].indic.category, Category::DottedCircle);
    }

    /// A face with no U+25CC gets no circle, which is the honest answer: there
    /// is nothing to draw.
    #[test]
    fn a_face_without_a_dotted_circle_gains_no_glyphs() {
        let mut glyphs = run("\u{17D2}");
        setup_syllables(&mut glyphs);
        reorder(&all_masks(), &mut glyphs, None);
        assert_eq!(glyphs.len(), 1);
    }

    /// A non-Khmer syllable is never reordered and never given a circle, which
    /// is what lets a mixed run go through one shaper.
    #[test]
    fn a_latin_run_passes_through_untouched() {
        let mut glyphs = run("ab");
        let before = order(&glyphs);
        // Not zero: a fresh glyph already carries the always-on features. The
        // claim under test is that reordering adds none of the Khmer bits to
        // it, so the mask has to be compared with where it started.
        let masks: Vec<u64> = glyphs.iter().map(|g| g.mask).collect();
        setup_syllables(&mut glyphs);
        reorder(&all_masks(), &mut glyphs, Some(99));
        assert_eq!(order(&glyphs), before);
        assert!(glyphs.iter().map(|g| g.mask).eq(masks));
    }
}

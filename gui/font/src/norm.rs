//! Unicode normalization, as the stage that runs before glyphs exist.
//!
//! # Why a shaper needs this at all
//!
//! `é` can be written two ways: one character (U+00E9) or two (`e` followed
//! by U+0301 COMBINING ACUTE ACCENT). They are *canonically equivalent* —
//! Unicode says they mean the same thing and must look the same — but a font
//! does not see meaning, it sees code points, and the two forms take entirely
//! different paths through it. The one-character form finds a drawn glyph in
//! `cmap`. The two-character form finds a letter and a floating accent, and
//! depends on `GPOS` mark attachment to put them together.
//!
//! Without this stage the shaper simply took whichever form the string
//! happened to be in. That is not *wrong* — the accent still lands on its
//! anchor and the text reads correctly — but it means our glyph run differs
//! from every other shaper's for the same string, on **288 of the 556 faces
//! installed on the development host**, which was by far the largest class of
//! disagreement when substitution was cross-checked against HarfBuzz. Two
//! callers holding the same string get different glyph counts, different
//! advances, and different hit-test offsets depending on how the text was
//! typed.
//!
//! # What it does
//!
//! Composition to NFC, and then a second question no normalizer asks:
//! *can this face actually draw the result?* Composing is only an improvement
//! if the composed character has a glyph. A face with no `é` must get the
//! decomposed form back, or it draws a missing-glyph box where the previous
//! code drew a perfectly good accented letter.
//!
//! So the stage is two layers, and keeping them apart is the point:
//!
//! 1. [`nfc`] is pure Unicode. It decomposes, sorts marks into canonical
//!    order, and recomposes, exactly per UAX #15. It knows nothing about
//!    fonts and is idempotent, which is what makes it safe to run over text
//!    that was already right.
//! 2. [`fit_to_face`] is pure font. It walks the normalized text and pulls
//!    apart any character the face cannot draw, so the mark placer gets a
//!    chance at it.
//!
//! Mixing the two — "compose only if the font has the glyph" — is the
//! tempting shortcut and it is wrong, because composition is not a per-pair
//! decision: whether a mark composes depends on the marks before it, so a
//! font-conditional composition can leave the marks in an order NFC would
//! never produce.
//!
//! # Clusters
//!
//! A combining mark takes the cluster of the character it attaches to, not
//! its own byte offset. This is forced rather than chosen: canonical
//! ordering *reorders* marks, and clusters must not decrease along a run, so
//! marks that can swap places cannot keep separate clusters. It is also the
//! better answer independently — a caret should not be able to land between
//! a letter and its accent, for the same reason it cannot land inside `fi`.

use alloc::vec::Vec;

use crate::norm_tables::{
    COMBINES_BACKWARD, CCC, MARKS, NO_COMPOSE, PAIRS, PAIRS_BY_PARTS, SINGLETONS,
};
use crate::khmer;
use crate::thai;

/// The first Hangul syllable, and the counts that make its composition
/// arithmetic rather than a table. See UAX #15 §16 "Hangul".
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

/// How deep a decomposition may recurse.
///
/// Canonical decomposition is acyclic and shallow in the real UCD — the
/// deepest chain is a Vietnamese letter with a tone mark on a letter that
/// itself decomposes, four steps at most. The bound exists so that a future
/// regeneration against a broken table cannot hang the text engine, not
/// because any real character needs it.
const MAX_DEPTH: u32 = 8;

/// One character of text, with the byte offset in the source string that it
/// is charged to.
///
/// Carried as a pair through this whole module because normalization does not
/// preserve the count of characters in either direction, so the offsets
/// cannot be recovered afterwards by walking the string again.
pub(crate) type Piece = (char, usize);

/// The canonical combining class of `ch`. Zero means a starter.
#[must_use]
pub(crate) fn combining_class(ch: char) -> u8 {
    let cp = ch as u32;
    // Ranges are sorted and disjoint, so the partition point of "ends before
    // cp" is the only range that can contain it.
    let i = CCC.partition_point(|&(_, hi, _)| hi < cp);
    match CCC.get(i) {
        Some(&(lo, _, ccc)) if cp >= lo => ccc,
        _ => 0,
    }
}

/// The class `ch` sorts by when the marks are put into the order they are
/// *drawn* in rather than the order Unicode normalizes them to.
///
/// Unicode's combining classes 10–36, 84, 91, 103 and 129–132 are the
/// "fixed-position" classes, and their numeric values are a canonical
/// ordering: they exist so that two spellings of the same text normalize to
/// one, not so that the marks come out stacked correctly. For Hebrew and
/// Arabic they are in nearly the opposite order from the one the marks are
/// drawn in — a Hebrew vowel is typed before the shin dot and drawn under the
/// letter while the dot goes over it, and Arabic shadda is typed after a vowel
/// and drawn nearer the letter than it.
///
/// So this permutes them into an order whose *numbers* are stacking order,
/// bottom to top, and the shaper sorts a second time with it. HarfBuzz's
/// `_hb_modified_combining_class`, and it does the same thing for the same
/// reason. Every block below is a bijection onto itself — the Hebrew arms are
/// a permutation of 10–26, the Arabic ones of 27–35 — which is what keeps the
/// second sort from merging two marks that canonical order kept apart, and
/// what lets [`crate::fallback::attach_class`] go on matching real classes.
///
/// Classes outside those blocks pass through, including every class 200 and
/// up: those already *are* positions and are already in stacking order.
#[must_use]
pub(crate) fn display_class(ch: char) -> u8 {
    permute(combining_class(ch))
}

/// [`display_class`]'s table, taken over the class rather than the character
/// so that the property the second sort rests on — that each block is a
/// bijection onto itself — is a thing a test can check directly.
fn permute(klass: u8) -> u8 {
    match klass {
        // Hebrew points, in the order the SBL Hebrew manual stacks them:
        // shin dot, sin dot, dagesh, rafe, holam, the hatafs, tsere, segol,
        // patah, qamats, sheva, hiriq, qubuts, meteg, varika.
        10 => 22,
        11 => 15,
        12 => 16,
        13 => 17,
        14 => 23,
        15 => 18,
        16 => 19,
        17 => 20,
        18 => 21,
        19 => 14,
        20 => 24,
        21 => 12,
        22 => 25,
        23 => 13,
        24 => 10,
        25 => 11,
        // Arabic: shadda (33) is typed last and drawn first, so it moves ahead
        // of the vowels at 27–32 and they each shift up by one. Unicode says
        // as much in its normalization FAQ and declines to renumber, because
        // the classes are stability guarantees.
        k @ 27..=32 => k.saturating_add(1),
        33 => 27,
        // The Telugu length marks and Thai sara u/uu, which are the only
        // vowel signs in these ranges with a non-zero class and so are the
        // only ones that reorder around the virama at class 9. They belong
        // before it; 3, 4 and 5 are unassigned and below it.
        84 => 4,
        91 => 5,
        103 => 3,
        // Tibetan: with several vowel signs, u is drawn before i, which is
        // what makes a multi-vowel Dzongkha syllable come out right.
        130 => 132,
        132 => 131,
        other => other,
    }
}

/// Whether `ch` is a non-spacing combining mark — general category `Mn`.
///
/// `Mn` alone, and not the whole of `M*`. `Mc`, the *spacing* combining marks,
/// is deliberately outside it: a Devanagari matra genuinely occupies width, so
/// calling it a mark here would take that width away and pile the vowel onto
/// the consonant. HarfBuzz draws the line in the same place, in
/// `hb_synthesize_glyph_classes`.
///
/// Distinct from `combining_class(ch) != 0`, which is the thing a caller
/// reaches for and which is wrong: a combining class is a canonical
/// *ordering*, and a mark that never needs reordering is left at class 0.
/// U+0E35 THAI SARA II is one. Asking the class instead charges such a mark a
/// full advance and draws the text a box too wide per mark.
///
/// This is what HarfBuzz uses to decide a glyph is a mark when the face's
/// `GDEF` has no class for it, and it is a separate decision from whether the
/// mark may be *placed* by measurement — see
/// [`positions_marks`](crate::fallback::positions_marks), which several
/// scripts turn off while their marks still take no room.
#[must_use]
pub(crate) fn is_mark(ch: char) -> bool {
    let cp = ch as u32;
    let i = MARKS.partition_point(|&(_, hi)| hi < cp);
    MARKS.get(i).is_some_and(|&(lo, _)| cp >= lo)
}

/// The characters that exist to instruct the shaper and are never drawn.
///
/// HarfBuzz's `hb_unicode_funcs_t::is_default_ignorable`, transcribed range
/// for range, and **not** Unicode's `Default_Ignorable_Code_Point` property,
/// which is a different set. They disagree in both directions:
///
/// | | in HarfBuzz | in Unicode |
/// |---|---|---|
/// | U+180E MONGOLIAN VOWEL SEPARATOR | yes | removed from the property in 6.3 |
/// | U+1BCA0..A3 SHORTHAND FORMAT | no | yes |
/// | U+115F, U+1160, U+3164, U+FFA0 Hangul fillers | no | yes |
///
/// The set decides which glyphs are erased at the end of shaping, so taking
/// Unicode's would make every string containing one of those disagree with
/// HarfBuzz in a way that reads like a shaping bug and is not.
///
/// Written out as a table rather than derived from the Unicode data for the
/// same reason: a derived set would drift silently the next time that data was
/// regenerated. This one changes when HarfBuzz changes.
const IGNORABLE: &[(u32, u32)] = &[
    (0x0000_00AD, 0x0000_00AD), // SOFT HYPHEN
    (0x0000_034F, 0x0000_034F), // COMBINING GRAPHEME JOINER
    (0x0000_061C, 0x0000_061C), // ARABIC LETTER MARK
    (0x0000_17B4, 0x0000_17B5), // KHMER VOWEL INHERENT AQ, AA
    (0x0000_180B, 0x0000_180E), // MONGOLIAN free variation selectors, vowel separator
    (0x0000_200B, 0x0000_200F), // ZERO WIDTH SPACE .. RIGHT-TO-LEFT MARK
    (0x0000_202A, 0x0000_202E), // the bidi embedding and override controls
    (0x0000_2060, 0x0000_206F), // WORD JOINER .. NOMINAL DIGIT SHAPES
    (0x0000_FE00, 0x0000_FE0F), // VARIATION SELECTOR-1 .. -16
    (0x0000_FEFF, 0x0000_FEFF), // ZERO WIDTH NO-BREAK SPACE
    (0x0000_FFF0, 0x0000_FFF8), // unassigned, reserved as ignorable
    (0x0001_D173, 0x0001_D17A), // MUSICAL SYMBOL BEGIN BEAM .. END PHRASE
    (0x000E_0000, 0x000E_0FFF), // language tags, variation selectors supplement
];

/// Whether `ch` is one of the characters shaping consumes and never draws:
/// a joiner, a soft hyphen, a bidi control, a variation selector.
///
/// The caller's obligation is *not* to skip them — they take part in shaping,
/// and a joiner dropped early would stop making the ligature it exists to
/// request — but to erase them once shaping is over, which is what
/// [`SubGlyph::ignorable`](crate::gsub::SubGlyph) tracks and
/// [`ScaledFont::shape`](crate::ScaledFont::shape) acts on.
#[must_use]
pub(crate) fn is_default_ignorable(ch: char) -> bool {
    let cp = ch as u32;
    let i = IGNORABLE.partition_point(|&(_, hi)| hi < cp);
    IGNORABLE.get(i).is_some_and(|&(lo, _)| cp >= lo)
}

/// Whether `text` can possibly change under [`nfc`].
///
/// Text with no combining marks and no composed characters is already in NFC
/// and normalizing it is the identity — which is nearly all Latin text, and
/// all ASCII. Checking is a scan of the string against three sorted tables;
/// normalizing allocates two buffers and walks them three times. The fast
/// path is the reason this stage can run on every string the OS draws.
///
/// Being wrong here is silent, so the predicate is stated as the two ways NFC
/// can change a run and nothing else: something in it comes apart, or
/// something in it joins onto what precedes it. The second is easy to write
/// as "is a combining mark" and be quietly wrong for years — Hangul jamo and
/// Indic two-part vowels join backwards without being marks, and a run of
/// them would sail through unnormalized. [`combines_backward`] is that case.
/// Hangul is asked about here under [`Hangul::Normalize`], which is the
/// conservative direction: this predicate may say yes to a run the pass turns
/// out to leave alone, and that costs one identity pass — whereas saying no to
/// a run that needed work would drop the work silently, which is the failure
/// this function's whole design is trying to avoid.
#[must_use]
pub(crate) fn needs_work(text: &str) -> bool {
    text.chars().any(|ch| {
        combining_class(ch) != 0
            || decompose_once(ch, Hangul::Normalize).is_some()
            || combines_backward(ch)
    })
}

/// Whether `ch` can compose onto the character before it without being a mark.
///
/// Hangul jamo are answered arithmetically, like the rest of Hangul: a vowel
/// joins a leading consonant, and a trailing consonant joins an LV syllable.
fn combines_backward(ch: char) -> bool {
    let cp = ch as u32;
    if cp.wrapping_sub(V_BASE) < V_COUNT || cp.wrapping_sub(T_BASE) < T_COUNT {
        // `T_BASE` itself is the "no trailing consonant" filler, not a jamo,
        // but `wrapping_sub` gives 0 for it and 0 < T_COUNT — so exclude it.
        return cp != T_BASE;
    }
    COMBINES_BACKWARD.binary_search(&cp).is_ok()
}

/// Whether a normalization pass may rewrite the spelling of Hangul.
///
/// Korean is encoded two ways — as 11,172 precomposed syllables and as
/// sequences of conjoining jamo — and canonical equivalence says they are the
/// same text. For a *text* question that is the whole story, and the answer is
/// [`Hangul::Normalize`].
///
/// For a *drawing* question it is not, because a face need not cover both
/// spellings, so which one to draw is a question about the face. That is
/// [`hangul::preprocess`](crate::hangul::preprocess)'s job, and it can only do
/// it if the spelling it is handed is still the one the text used — which is
/// why HarfBuzz's Hangul shaper sets `HB_OT_SHAPE_NORMALIZATION_MODE_NONE`
/// rather than normalizing and then trying to undo it. [`Hangul::LeaveAlone`]
/// is that mode: every other script still normalizes, and Hangul is passed
/// through untouched for the shaper that follows to decide about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hangul {
    /// Compose and decompose Hangul like anything else. What NFC means.
    Normalize,
    /// Leave Hangul spelled the way the text spelled it.
    LeaveAlone,
}

/// The canonical decomposition of `ch`, one step, if it has one.
///
/// `Some((a, Some(b)))` for a pair, `Some((a, None))` for a singleton.
/// Hangul syllables answer here too, computed rather than looked up — unless
/// `hangul` is [`Hangul::LeaveAlone`], in which case a syllable has no
/// decomposition as far as this function is concerned.
fn decompose_once(ch: char, hangul: Hangul) -> Option<(char, Option<char>)> {
    let cp = ch as u32;
    if hangul == Hangul::Normalize
        && let Some(d) = hangul_decompose(cp)
    {
        return Some(d);
    }
    if let Ok(i) = PAIRS.binary_search_by_key(&cp, |&(c, _, _)| c) {
        let (_, a, b) = *PAIRS.get(i)?;
        return Some((char::from_u32(a)?, char::from_u32(b)));
    }
    if let Ok(i) = SINGLETONS.binary_search_by_key(&cp, |&(c, _)| c) {
        let (_, a) = *SINGLETONS.get(i)?;
        return Some((char::from_u32(a)?, None));
    }
    None
}

/// A Hangul syllable split into its leading pair, one step.
///
/// LVT splits into LV and T rather than into L, V and T, so that recursing on
/// the first part reaches L and V — which is what makes the general recursive
/// decomposition below work on Hangul without knowing anything about it.
fn hangul_decompose(cp: u32) -> Option<(char, Option<char>)> {
    let s = cp.checked_sub(S_BASE)?;
    if s >= S_COUNT {
        return None;
    }
    let t = s % T_COUNT;
    if t > 0 {
        let lv = char::from_u32(S_BASE.checked_add(s.checked_sub(t)?)?)?;
        return Some((lv, char::from_u32(T_BASE.checked_add(t)?)));
    }
    let l = char::from_u32(L_BASE.checked_add(s / N_COUNT)?)?;
    let v = char::from_u32(V_BASE.checked_add(s % N_COUNT / T_COUNT)?)?;
    Some((l, Some(v)))
}

/// The character `a` and `b` compose to, if they do.
///
/// Under [`Hangul::LeaveAlone`] a jamo pair does not compose, so text typed as
/// jamo stays jamo for the Hangul shaper to rule on.
fn compose_pair(a: char, b: char, hangul: Hangul) -> Option<char> {
    if hangul == Hangul::Normalize
        && let Some(s) = hangul_compose(a as u32, b as u32)
    {
        return Some(s);
    }
    let key = (a as u32, b as u32);
    let i = PAIRS_BY_PARTS
        .binary_search_by_key(&key, |&j| {
            PAIRS.get(usize::from(j)).map_or((0, 0), |&(_, x, y)| (x, y))
        })
        .ok()?;
    let row = usize::from(*PAIRS_BY_PARTS.get(i)?);
    let (c, _, _) = *PAIRS.get(row)?;
    // A composition exclusion decomposes but does not come back. The
    // membership test is a binary search over 85 entries rather than a flag
    // in `PAIRS`, so that the generated table stays one fact per row.
    if NO_COMPOSE.binary_search(&c).is_ok() {
        return None;
    }
    char::from_u32(c)
}

/// L+V or LV+T, composed arithmetically.
fn hangul_compose(a: u32, b: u32) -> Option<char> {
    if let (Some(l), Some(v)) = (a.checked_sub(L_BASE), b.checked_sub(V_BASE))
        && l < L_COUNT
        && v < V_COUNT
    {
        return char::from_u32(
            S_BASE
                .checked_add(l.checked_mul(N_COUNT)?)?
                .checked_add(v.checked_mul(T_COUNT)?)?,
        );
    }
    if let (Some(s), Some(t)) = (a.checked_sub(S_BASE), b.checked_sub(T_BASE))
        && s < S_COUNT
        && s % T_COUNT == 0
        && t > 0
        && t < T_COUNT
    {
        return char::from_u32(S_BASE.checked_add(s)?.checked_add(t)?);
    }
    None
}

/// Append the full canonical decomposition of `ch` to `out`, at `cluster`.
fn decompose_into(ch: char, cluster: usize, out: &mut Vec<Piece>, depth: u32, hangul: Hangul) {
    match decompose_once(ch, hangul) {
        Some((a, b)) if depth < MAX_DEPTH => {
            decompose_into(a, cluster, out, depth.saturating_add(1), hangul);
            if let Some(b) = b {
                decompose_into(b, cluster, out, depth.saturating_add(1), hangul);
            }
        }
        _ => out.push((ch, cluster)),
    }
}

/// Sort each run of non-starters by `class`.
///
/// Stable and by class only, per UAX #15: two marks of the *same* class are
/// visually stacked in the order they were typed, so reordering them would
/// move an accent. This is an insertion sort because the runs are one or two
/// marks long in practice and the algorithm's stability is easier to see than
/// to look up.
///
/// The class function is a parameter because this sort is run twice, over two
/// different orderings of the same marks — [`combining_class`] for canonical
/// order, [`display_class`] for the order they are drawn in. The algorithm is
/// identical and only the key differs; both keys agree about which characters
/// are starters, which is what makes one loop able to serve both.
fn sort_marks(pieces: &mut [Piece], class: impl Fn(char) -> u8) {
    for i in 1..pieces.len() {
        let Some(&(ch, cluster)) = pieces.get(i) else {
            continue;
        };
        let ccc = class(ch);
        if ccc == 0 {
            continue;
        }
        let mut j = i;
        while let Some(&(prev, _)) = j.checked_sub(1).and_then(|k| pieces.get(k)) {
            let prev_ccc = class(prev);
            // Stop at a starter: a mark never moves past the character it
            // attaches to. Stop at an equal class: that is what makes this
            // stable, and stacking order is meaningful within a class.
            if prev_ccc == 0 || prev_ccc <= ccc {
                break;
            }
            let Some(k) = j.checked_sub(1) else { break };
            pieces.swap(k, j);
            j = k;
        }
        if let Some(slot) = pieces.get_mut(j) {
            *slot = (ch, cluster);
        }
    }
}

/// Text in Normalization Form C.
///
/// Decompose fully, sort marks into canonical order, then recompose greedily
/// onto the last starter. The clusters that come out are the input's, with
/// every mark charged to the character it attaches to.
///
/// Nothing in the crate draws through this any more — [`pieces`] asks for
/// [`Hangul::LeaveAlone`] — so outside the tests it is currently uncalled. It
/// is kept, and kept exact, because "what is this text in NFC" is a different
/// question from "how should this face draw it" and the distinction is the
/// point of the split; the tests below hold it to real NFC either way.
#[cfg_attr(not(test), allow(dead_code, reason = "the text-question half of the \
    normalize split; only the drawing half has callers today"))]
#[must_use]
pub(crate) fn nfc(text: &str) -> Vec<Piece> {
    // NFC is NFC: a question about *text* gets the Unicode answer, Hangul
    // included and SARA AM left whole. Only the drawing path below asks for
    // anything else.
    normalize(text, Hangul::Normalize, SaraAm::LeaveAlone)
}

/// Whether Thai and Lao SARA AM comes apart before the marks are sorted.
///
/// U+0E33 and U+0EB3 have no canonical decomposition, so as far as Unicode is
/// concerned there is nothing here to decide and [`SaraAm::LeaveAlone`] is what
/// NFC means. As far as a renderer is concerned there is: the character is
/// drawn as two marks in two places, and one of them belongs in front of marks
/// that were typed before it. [`thai::preprocess`](crate::thai::preprocess) is
/// that pass, and this is the switch that lets the drawing path run it without
/// [`nfc`] ceasing to be NFC.
///
/// It is a parameter of [`normalize`] rather than a step around it because
/// *where* it runs is the whole question — between decomposition and the
/// canonical sort, which is a place only `normalize` has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SaraAm {
    /// Split it, and move the upper half back over the marks it is drawn
    /// beneath. What a renderer wants.
    Decompose,
    /// Leave it as the one character Unicode says it is. What NFC says.
    LeaveAlone,
}

/// [`nfc`], with the Hangul and SARA AM questions left open.
///
/// The whole of NFC except that `hangul` decides whether Hangul takes part and
/// `sara_am` decides whether Thai's two-part vowel does. Split out so the
/// drawing path can ask for NFC-of-everything-else without `nfc` itself
/// ceasing to mean NFC.
#[must_use]
fn normalize(text: &str, hangul: Hangul, sara_am: SaraAm) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::with_capacity(text.len());
    let mut cluster = 0usize;
    for (offset, ch) in text.char_indices() {
        // A mark joins the cluster of the character before it; a starter
        // opens a new one. A leading mark with nothing to attach to keeps its
        // own offset, so the run still starts at zero.
        if combining_class(ch) == 0 || pieces.is_empty() {
            cluster = offset;
        }
        decompose_into(ch, cluster, &mut pieces, 0, hangul);
    }
    if sara_am == SaraAm::Decompose {
        // Before the sort and not after. The nikhahit is moved back over the
        // marks *as typed*, and sorting first would have put a below-base
        // vowel among them and stopped the scan one place early. See
        // [`thai`](crate::thai). Safe to run after decomposition rather than
        // before it, which is where HarfBuzz runs it, because no character
        // decomposes to or from a Thai or Lao one.
        crate::thai::preprocess(&mut pieces);
    }
    sort_marks(&mut pieces, combining_class);
    compose(&mut pieces, hangul);
    pieces
}

/// Recompose in place, per the UAX #15 composition algorithm.
///
/// Walks with a cursor on the last starter and tries each following character
/// against it. A character that does not compose is kept, and — this is the
/// part that is easy to get wrong — it *blocks* any later character of the
/// same or lower combining class from composing onto that starter, because
/// composing past it would move a mark across another mark and change what
/// the text says.
fn compose(pieces: &mut Vec<Piece>, hangul: Hangul) {
    let mut starter: Option<usize> = None;
    let mut last_ccc: Option<u8> = None;
    let mut write = 0usize;
    for read in 0..pieces.len() {
        let Some(&(ch, cluster)) = pieces.get(read) else {
            continue;
        };
        let ccc = combining_class(ch);
        let blocked = match last_ccc {
            // Nothing between this character and the starter: composable.
            None => false,
            // Something intervenes. It blocks unless it sorts strictly
            // before, which is exactly the case where this character is
            // still "attached" to the starter rather than to it.
            Some(prev) => prev >= ccc,
        };
        if let Some(at) = starter
            && !blocked
            && let Some(&(base, base_cluster)) = pieces.get(at)
            && let Some(joined) = compose_pair(base, ch, hangul)
        {
            if let Some(slot) = pieces.get_mut(at) {
                *slot = (joined, base_cluster);
            }
            // `last_ccc` deliberately does not move: the character was
            // absorbed, so it is not between the starter and what follows.
            continue;
        }
        if let Some(slot) = pieces.get_mut(write) {
            *slot = (ch, cluster);
        }
        if ccc == 0 {
            starter = Some(write);
            last_ccc = None;
        } else {
            last_ccc = Some(ccc);
        }
        write = write.saturating_add(1);
    }
    pieces.truncate(write);
}

/// The characters a face should actually be asked for, given `text`.
///
/// Three steps, and the only entry point the shaper uses: normalize to NFC,
/// take back apart whatever this face cannot draw, then put the marks into
/// the order they are drawn in. All three are skipped when they would do
/// nothing, which is the usual case — ASCII takes one scan of the string and
/// one scan of the pieces, and neither table is consulted twice.
///
/// The output is therefore **not** NFC for Hebrew, Arabic, Telugu, Thai and
/// Tibetan: within a run of marks it is [`display_class`] order, which is what
/// the rest of the shaper wants and what HarfBuzz produces. `nfc` is still
/// exactly NFC, and is what a caller with a text question rather than a
/// drawing one should ask. The reordering is confined to a mark run, so no
/// character crosses the base it attaches to and every cluster is unchanged;
/// the string this stage describes is the same string, spelled for a renderer.
#[must_use]
pub(crate) fn pieces(text: &str, has_glyph: impl Fn(char) -> bool) -> Vec<Piece> {
    // Two questions, each asked once, gating the passes that only marks can
    // make work for: a string with no marks and nothing to decompose has
    // nothing to reorder and nothing to take apart. SARA AM is asked about
    // separately and not folded into `needs_work`, because it is neither a
    // mark nor decomposable and so `needs_work` is right to say no to it —
    // it is a second reason to do the work, not a wider version of the first.
    // Khmer's five split vowel signs are asked about the same way and for the
    // same reason: they are neither marks nor decomposable, so `needs_work` is
    // right to say no to them, and taking them apart is a third separate
    // reason to do the work. See [`khmer::split_matras`](crate::khmer).
    let work = needs_work(text) || thai::present(text) || khmer::present(text);
    let mut out = if work {
        // Everything but Hangul. Which spelling of a Korean syllable to draw
        // depends on what the face covers, so it is `hangul::preprocess` that
        // decides it, a stage later — and it can only decide if the spelling
        // reaching it is still the text's own.
        normalize(text, Hangul::LeaveAlone, SaraAm::Decompose)
    } else {
        // Already NFC by inspection, so the offsets are the string's own and
        // there is nothing to reorder or join.
        text.char_indices().map(|(at, ch)| (ch, at)).collect()
    };
    // Before `fit_to_face` rather than after, because the halves it produces
    // are ordinary characters the face may or may not have and the fitting
    // pass is entitled to look at them — and because a split vowel the face
    // *cannot* split has to reach `fit_to_face` whole, which is what happens
    // when this declines.
    khmer::split_matras(&mut out, &has_glyph);
    fit_to_face(&mut out, has_glyph);
    if work {
        // After `fit_to_face` and not before: splitting a character the face
        // cannot draw adds marks, and those marks have to be sorted with the
        // rest rather than left wherever the decomposition put them.
        sort_marks(&mut out, display_class);
    }
    out
}

/// Pull apart any character `has_glyph` says the face cannot draw.
///
/// The second half of the stage, and the reason it is not just `nfc`. NFC is
/// about what the text *is*; this is about what this particular face can show
/// of it. A face without `é` still has `e` and a combining acute, and the
/// mark placer will put them together — so decomposing is strictly better
/// than the missing-glyph box that composing would have produced.
///
/// Splitting succeeds or fails as a whole, and what decides it is the *base*:
/// a character comes apart only if the face can draw what the decomposition
/// chain bottoms out at. The marks ride along whether the face has them or
/// not.
///
/// Both halves of that rule are load-bearing, and each was learnt from a
/// disagreement with HarfBuzz over the installed faces:
///
/// - Requiring the base is why `가` on a face with no Hangul at all stays one
///   character. Splitting it there would turn one missing-glyph box into two,
///   which is one more wrong thing on screen and one more caret stop than the
///   text has characters. 553 of 556 faces disagreed on this.
/// - *Not* requiring the marks is why `ç` plus an acute the face lacks still
///   draws the `ç`. The base carries nearly all of the meaning and a missing
///   accent costs an accent; refusing to split costs the letter too.
///
/// HarfBuzz applies the second rule to text typed as `c` + cedilla + acute and
/// the first to the same text typed as `ế`, so it renders two canonically
/// equivalent strings differently. This stage normalizes before it fits, so
/// the two spellings are the same string by the time they get here and there
/// is only one answer to give. Where the numbers below differ from HarfBuzz's
/// this is usually why.
///
/// Recursing on the first piece is what lets a chain be walked: `ế` on a face
/// with `e` and both marks comes apart in two steps, through `ê`.
pub(crate) fn fit_to_face(pieces: &mut Vec<Piece>, has_glyph: impl Fn(char) -> bool) {
    if pieces.iter().all(|&(ch, _)| has_glyph(ch)) {
        return;
    }
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len().saturating_add(4));
    for &(ch, cluster) in pieces.iter() {
        if !split_undrawable(ch, cluster, &mut out, &has_glyph, 0) {
            out.push((ch, cluster));
        }
    }
    *pieces = out;
}

/// Append the drawable pieces of `ch`, or append nothing and return `false`.
///
/// `out` is left exactly as it was found when this returns `false`, so a
/// caller can fall back without having to undo anything.
fn split_undrawable(
    ch: char,
    cluster: usize,
    out: &mut Vec<Piece>,
    has_glyph: &impl Fn(char) -> bool,
    depth: u32,
) -> bool {
    if has_glyph(ch) {
        out.push((ch, cluster));
        return true;
    }
    if depth >= MAX_DEPTH {
        return false;
    }
    // `LeaveAlone` here, and not because this stage is indifferent to Hangul:
    // a syllable that reaches this point is one `hangul::preprocess` looked at
    // and declined to split, on grounds this function cannot see — namely that
    // the face has no jamo to split it into. Splitting it here would overrule
    // that with less information and turn one missing-glyph box into three.
    let Some((a, b)) = decompose_once(ch, Hangul::LeaveAlone) else {
        return false;
    };
    let mark = out.len();
    if !split_undrawable(a, cluster, out, has_glyph, depth.saturating_add(1)) {
        out.truncate(mark);
        return false;
    }
    // The tail goes out whether or not the face has it, and is not recursed
    // into: it is a mark or a vowel sign, which have no decomposition worth
    // walking, and a box where an accent should be still leaves the letter
    // readable.
    if let Some(b) = b {
        out.push((b, cluster));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn chars(pieces: &[Piece]) -> String {
        pieces.iter().map(|&(ch, _)| ch).collect()
    }

    fn clusters(pieces: &[Piece]) -> Vec<usize> {
        pieces.iter().map(|&(_, c)| c).collect()
    }

    /// The reason `is_mark` exists at all: a mark whose combining class is
    /// zero. Asking the class — which is what the shaper used to do — calls
    /// U+0E35 a base and charges it a full advance, which is a Thai word drawn
    /// a mark's width too wide per vowel.
    #[test]
    fn a_mark_with_no_combining_class_is_still_a_mark() {
        assert_eq!(combining_class('\u{e35}'), 0);
        assert!(is_mark('\u{e35}'));
    }

    /// And the other half of the same claim: a *spacing* combining mark is not
    /// one. U+093F DEVANAGARI VOWEL SIGN I is category `Mc`, it genuinely
    /// occupies width, and zeroing it would pile the vowel onto the consonant.
    #[test]
    fn a_spacing_combining_mark_is_not_a_mark_here() {
        assert!(!is_mark('\u{93f}'));
    }

    /// The characters the table names, one per range, and the letters either
    /// side of the ones that are easiest to get off by one.
    #[test]
    fn the_ignorables_are_the_characters_that_instruct_the_shaper() {
        for ch in [
            '\u{ad}',     // SOFT HYPHEN, the one that draws a visible hyphen
            '\u{34f}',    // COMBINING GRAPHEME JOINER
            '\u{61c}',    // ARABIC LETTER MARK
            '\u{17b4}',   // KHMER VOWEL INHERENT AQ
            '\u{17b5}',   // ...and AA, the other end of that range
            '\u{180b}',   // MONGOLIAN FREE VARIATION SELECTOR ONE
            '\u{200b}',   // ZERO WIDTH SPACE
            '\u{200c}',   // ZWNJ
            '\u{200d}',   // ZWJ
            '\u{200f}',   // RIGHT-TO-LEFT MARK
            '\u{202b}',   // RIGHT-TO-LEFT EMBEDDING
            '\u{2060}',   // WORD JOINER
            '\u{fe0f}',   // VARIATION SELECTOR-16
            '\u{feff}',   // ZERO WIDTH NO-BREAK SPACE
            '\u{1d173}',  // MUSICAL SYMBOL BEGIN BEAM
            '\u{e0020}',  // TAG SPACE
            '\u{e0100}',  // VARIATION SELECTOR-17
        ] {
            assert!(is_default_ignorable(ch), "{ch:?}");
        }
        for ch in [
            'a', ' ', '-',
            '\u{ac}',    // one below SOFT HYPHEN
            '\u{ae}',    // one above
            '\u{17b3}',  // one below the Khmer pair
            '\u{17b6}',  // one above it — a real vowel sign, drawn
            '\u{200a}',  // HAIR SPACE, which is a space and *is* drawn
            '\u{2010}',  // HYPHEN, the visible one U+00AD is confused with
            '\u{fdff}', '\u{ff00}',
        ] {
            assert!(!is_default_ignorable(ch), "{ch:?}");
        }
    }

    /// The set is HarfBuzz's, not Unicode's, and the doc comment says the two
    /// differ. This is that claim as a test, in both directions, so that
    /// anyone who "fixes" the table to match the Unicode property finds out
    /// here rather than in a sweep of 556 faces.
    #[test]
    fn the_ignorable_set_is_harfbuzzs_and_not_unicodes() {
        // In HarfBuzz's list, removed from Unicode's property in 6.3.
        assert!(is_default_ignorable('\u{180e}'));
        // In Unicode's property, absent from HarfBuzz's list: the shorthand
        // format controls and the Hangul fillers.
        assert!(!is_default_ignorable('\u{1bca0}'));
        assert!(!is_default_ignorable('\u{115f}'));
        assert!(!is_default_ignorable('\u{3164}'));
        // ...and one that *is* in both, so the test cannot pass by the
        // predicate having become uniformly false.
        assert!(is_default_ignorable('\u{2065}'));
    }

    /// The table has to be sorted and disjoint, because the lookup is a binary
    /// search that assumes both. An unsorted pair would make the search miss a
    /// character silently — which is the failure mode this whole area is
    /// trying to avoid, since a missed ignorable is a glyph drawn that should
    /// not have been.
    #[test]
    fn the_ignorable_table_is_sorted_and_disjoint() {
        for pair in IGNORABLE.windows(2) {
            let [(lo, hi), (next_lo, next_hi)] = pair else {
                continue;
            };
            assert!(lo <= hi, "{lo:#x}..{hi:#x} is inverted");
            assert!(hi < next_lo, "{hi:#x} overlaps {next_lo:#x}..{next_hi:#x}");
        }
    }

    /// The property the second sort rests on. If two classes the canonical
    /// sort keeps apart mapped to one display class, the display sort would
    /// merge them and their typed order would start deciding their stacking
    /// order — silently, and only for the pair that collided.
    #[test]
    fn each_permuted_block_is_a_bijection_onto_itself() {
        // Hebrew and Arabic-with-Syriac. Every class in both ranges is one
        // Unicode assigns, so the image has to be the range itself.
        for (lo, hi) in [(10u8, 26u8), (27, 36)] {
            let mut got: Vec<u8> = (lo..=hi).map(permute).collect();
            got.sort_unstable();
            let want: Vec<u8> = (lo..=hi).collect();
            assert_eq!(got, want, "classes {lo}..={hi} are not permuted onto themselves");
        }
        // Tibetan is stated over the classes Unicode *assigns* rather than
        // over the range: 131 is unassigned, so 132 is free to take it and
        // the range as a whole is not a permutation. HarfBuzz's table collides
        // there too, and for the same reason it does not matter.
        let mut tibetan = [permute(129), permute(130), permute(132)];
        tibetan.sort_unstable();
        assert_eq!(tibetan, [129, 131, 132]);
        // The three that deliberately leave their block, to land below the
        // virama at class 9. They must not collide with each other or with
        // anything Unicode assigns.
        assert_eq!([permute(103), permute(84), permute(91)], [3, 4, 5]);
    }

    /// A class the permutation says nothing about is left exactly alone —
    /// above all class 0, since a starter that became a mark, or the reverse,
    /// would let the sort move a mark past the letter it belongs to.
    #[test]
    fn the_permutation_leaves_every_other_class_alone() {
        for k in 0..=255u8 {
            // Note the fixed points *inside* the permuted blocks: 26 (point
            // varika) is already stacked last among the Hebrew points, and 34,
            // 35 and 36 (sukun, superscript alef, superscript alaph) already
            // follow the vowels they are drawn above.
            let moved = matches!(k, 10..=25 | 27..=33 | 84 | 91 | 103 | 130 | 132);
            assert_eq!(permute(k) == k, !moved, "class {k}");
            assert_eq!(permute(k) == 0, k == 0, "class {k} changed starter-ness");
        }
    }

    /// The string the sweep found this with. Canonical order leaves it as
    /// typed — qamats is class 18 and the shin dot 24, already ascending —
    /// but the dot is drawn above the letter and the vowel below it, so the
    /// dot has to come first.
    #[test]
    fn hebrew_points_come_out_in_the_order_they_are_stacked() {
        let typed = "\u{5e9}\u{5b8}\u{5c1}";
        assert_eq!(chars(&pieces(typed, |_| true)), "\u{5e9}\u{5c1}\u{5b8}");
        // Every mark still belongs to the letter: reordering inside a run
        // cannot change which base a mark is charged to.
        assert_eq!(clusters(&pieces(typed, |_| true)), vec![0, 0, 0]);
        // And NFC itself is untouched. It answers a question about the text,
        // not about the drawing, and two callers want different answers.
        assert_eq!(chars(&nfc(typed)), typed);
    }

    /// Arabic shadda is typed after the vowel and drawn nearer the letter, so
    /// it has to move ahead of it. Unicode's own normalization FAQ names this
    /// case and declines to renumber the classes, because they are frozen.
    #[test]
    fn arabic_shadda_precedes_the_vowel_it_is_typed_after() {
        let out = pieces("\u{628}\u{64e}\u{651}", |_| true);
        assert_eq!(chars(&out), "\u{628}\u{651}\u{64e}");
    }

    /// Two marks of the same class are stacked in the order they were typed,
    /// and the second sort has to be as stable as the first.
    #[test]
    fn marks_of_one_class_keep_the_order_they_were_typed_in() {
        // Two class-230 marks above an `a`: acute then grave stays acute
        // then grave, whichever sort ran.
        let out = pieces("a\u{301}\u{300}", |_| true);
        assert_eq!(chars(&out), "\u{e1}\u{300}");
        let out = pieces("a\u{300}\u{301}", |_| true);
        assert_eq!(chars(&out), "\u{e0}\u{301}");
    }

    #[test]
    fn ordinary_letters_and_the_table_edges_are_not_marks() {
        for ch in ['a', 'A', '0', ' ', '\u{5d0}', '\u{4e00}', '\u{10ffff}'] {
            assert!(!is_mark(ch), "{ch:?}");
        }
        // One from each of a few blocks, so a table regenerated with the ranges
        // mis-sorted cannot pass: a binary search over a broken table finds
        // whatever it happens to land on.
        for ch in ['\u{301}', '\u{5b8}', '\u{64e}', '\u{94d}', '\u{fe00}', '\u{e0100}'] {
            assert!(is_mark(ch), "{ch:?}");
        }
    }

    #[test]
    fn ascii_is_left_exactly_alone() {
        assert!(!needs_work("the quick brown fox"));
        assert_eq!(chars(&nfc("the quick brown fox")), "the quick brown fox");
    }

    /// The case that started this: `e` plus a combining acute is one
    /// character, and every other shaper says so.
    #[test]
    fn a_letter_and_its_accent_become_one_character() {
        let out = nfc("e\u{301}");
        assert_eq!(chars(&out), "\u{e9}");
        assert_eq!(out.len(), 1);
    }

    /// And the mark is charged to the letter, not to its own byte offset, so
    /// a caret cannot land between them.
    #[test]
    fn a_mark_takes_the_cluster_of_what_it_attaches_to() {
        // `ae` + acute on the `e`. The acute is two bytes, so the `x` after
        // it starts at byte 4 — the clusters are byte offsets into the
        // *original* string, which is what makes them safe to slice with
        // after the character count has changed underneath them.
        let out = nfc("ae\u{301}x");
        assert_eq!(chars(&out), "a\u{e9}x");
        assert_eq!(clusters(&out), vec![0, 1, 4]);
    }

    /// A precomposed character is already NFC and must come back untouched —
    /// the stage is idempotent, which is what makes it safe to run on
    /// everything.
    #[test]
    fn already_composed_text_is_unchanged() {
        assert_eq!(chars(&nfc("\u{e9}t\u{e9}")), "\u{e9}t\u{e9}");
        let once = nfc("e\u{301}te\u{301}");
        let twice = nfc(&chars(&once));
        assert_eq!(chars(&once), chars(&twice));
    }

    /// Two marks of different classes are sorted, and then compose in the
    /// order Unicode says — not the order they were typed.
    #[test]
    fn marks_are_sorted_into_canonical_order() {
        // U+0301 acute is class 230, U+0327 cedilla is class 202, so the
        // cedilla sorts first however it was typed. Both orders must give the
        // same answer; that is what canonical equivalence means, and it is
        // the reason marks are sorted before anything is composed rather
        // than composed as they arrive.
        let typed_one_way = nfc("c\u{301}\u{327}");
        let typed_the_other = nfc("c\u{327}\u{301}");
        assert_eq!(chars(&typed_one_way), chars(&typed_the_other));
        // And the composition chains: c + cedilla is U+00E7, which then takes
        // the acute to give U+1E09. Composing in typing order would have
        // tried c + acute first and stopped one character short.
        assert_eq!(chars(&typed_one_way), "\u{1e09}");
    }

    /// Two marks of the *same* class are stacked in typing order and must not
    /// be swapped — reordering them would move an accent on the page.
    #[test]
    fn marks_of_one_class_keep_their_order() {
        // U+0300 grave and U+0301 acute are both class 230.
        assert_eq!(chars(&nfc("a\u{300}\u{301}")), "\u{e0}\u{301}");
        assert_eq!(chars(&nfc("a\u{301}\u{300}")), "\u{e1}\u{300}");
    }

    /// A mark that does not compose blocks a later one from reaching back
    /// past it.
    #[test]
    fn an_intervening_mark_blocks_composition() {
        // U+0328 ogonek (class 202) does not compose with `f`; the acute
        // (class 230) that follows must not skip over it onto the `f`.
        let out = nfc("f\u{328}\u{301}");
        assert_eq!(chars(&out), "f\u{328}\u{301}");
    }

    /// Composition exclusions decompose but do not come back.
    #[test]
    fn an_excluded_character_does_not_recompose() {
        // U+0958 DEVANAGARI LETTER KHA WITH NUKTA is a composition exclusion:
        // it decomposes to U+0915 + U+093C, and NFC leaves it that way.
        assert_eq!(chars(&nfc("\u{958}")), "\u{915}\u{93c}");
        assert_eq!(chars(&nfc("\u{915}\u{93c}")), "\u{915}\u{93c}");
    }

    /// A singleton maps to the letter it duplicates, and stays there.
    #[test]
    fn a_singleton_folds_to_its_canonical_letter() {
        // U+212B ANGSTROM SIGN is canonically U+00C5 A WITH RING ABOVE.
        assert_eq!(chars(&nfc("\u{212b}")), "\u{c5}");
    }

    /// Hangul composes arithmetically, in both the LV and LVT shapes.
    #[test]
    fn hangul_jamo_compose_without_a_table() {
        // L U+1100 + V U+1161 = U+AC00, then + T U+11A8 = U+AC01.
        assert_eq!(chars(&nfc("\u{1100}\u{1161}")), "\u{ac00}");
        assert_eq!(chars(&nfc("\u{1100}\u{1161}\u{11a8}")), "\u{ac01}");
        // And a syllable already written whole is left alone.
        assert_eq!(chars(&nfc("\u{ac01}")), "\u{ac01}");
    }

    #[test]
    fn hangul_decomposes_all_the_way_to_jamo() {
        let mut out = Vec::new();
        decompose_into('\u{ac01}', 0, &mut out, 0, Hangul::Normalize);
        assert_eq!(chars(&out), "\u{1100}\u{1161}\u{11a8}");
    }

    /// A jamo that is not a syllable must not be mistaken for one.
    #[test]
    fn a_lone_jamo_is_not_a_syllable() {
        assert_eq!(chars(&nfc("\u{1100}")), "\u{1100}");
        assert_eq!(chars(&nfc("\u{11a8}")), "\u{11a8}");
        // A trailing consonant with no syllable in front cannot attach.
        assert_eq!(chars(&nfc("a\u{11a8}")), "a\u{11a8}");
    }

    #[test]
    fn combining_classes_come_from_the_table() {
        assert_eq!(combining_class('a'), 0);
        assert_eq!(combining_class('\u{301}'), 230);
        assert_eq!(combining_class('\u{327}'), 202);
        assert_eq!(combining_class('\u{5b0}'), 10);
        // Past the end of the last range.
        assert_eq!(combining_class('\u{10FFFF}'), 0);
    }

    /// The fast path must agree with the slow one about what it skips: if
    /// `needs_work` says no, normalizing really must be the identity.
    #[test]
    fn the_fast_path_only_skips_text_that_would_not_change() {
        for text in [
            "hello", "1/2", "x\u{2013}y", "\u{e9}", "e\u{301}", "\u{ac00}", "\u{212b}", "",
        ] {
            if !needs_work(text) {
                assert_eq!(
                    chars(&nfc(text)),
                    text,
                    "needs_work said {text:?} was already NFC, but it changed"
                );
            }
        }
    }

    /// A run can compose without containing a mark or anything decomposable,
    /// and the fast path used to wave exactly those runs through unchanged.
    ///
    /// Three jamo are three starters with no decompositions; a Kannada
    /// two-part vowel is a starter followed by a starter. Both compose, and
    /// both would have been missed by a fast path that only looked for marks.
    #[test]
    fn the_fast_path_does_not_skip_runs_that_join_without_marks() {
        // 각 written out: leading ᄀ, vowel ᅡ, trailing ᆨ.
        assert!(needs_work("\u{1100}\u{1161}\u{11a8}"));
        assert_eq!(chars(&nfc("\u{1100}\u{1161}\u{11a8}")), "\u{ac01}");
        // Kannada ಿ + ೕ, neither a mark, together ೀ.
        assert!(needs_work("\u{cbf}\u{cd5}"));
        assert_eq!(chars(&nfc("\u{cbf}\u{cd5}")), "\u{cc0}");
        // The filler at the bottom of the trailing-consonant range is not a
        // jamo and must not drag a plain run onto the slow path.
        assert!(!needs_work(&String::from(char::from_u32(T_BASE).unwrap_or('a'))));
    }

    /// A face that cannot draw the composed form gets the pieces back.
    #[test]
    fn a_face_without_the_letter_gets_it_taken_apart() {
        let mut pieces = nfc("e\u{301}");
        assert_eq!(chars(&pieces), "\u{e9}");
        fit_to_face(&mut pieces, |ch| ch != '\u{e9}');
        assert_eq!(chars(&pieces), "e\u{301}");
        // Both halves are charged to the letter, so the cluster is unchanged.
        assert_eq!(clusters(&pieces), vec![0, 0]);
    }

    /// The case that made this all-or-nothing: a face with none of the pieces
    /// either must produce *one* missing-glyph box, not one per piece.
    ///
    /// Found by shaping a Hangul syllable against every installed face and
    /// comparing with HarfBuzz: 553 of 556 faces disagreed, every one of them
    /// because we split `가` into two notdefs where HarfBuzz kept one.
    #[test]
    fn a_face_with_none_of_the_pieces_keeps_the_character_whole() {
        for ch in ['\u{ac00}', '\u{e9}', '\u{212b}'] {
            let mut pieces = alloc::vec![(ch, 0usize)];
            fit_to_face(&mut pieces, |_| false);
            assert_eq!(pieces.len(), 1, "{ch:?} was split into unusable pieces");
            assert_eq!(chars(&pieces), String::from(ch));
        }
    }

    /// But a missing *mark* does not stop the split: `e` with a box over it
    /// is still an `e`, where refusing to split loses the letter as well.
    #[test]
    fn a_face_missing_only_the_mark_still_shows_the_letter() {
        let mut pieces = alloc::vec![('\u{e9}', 0usize)];
        fit_to_face(&mut pieces, |ch| ch == 'e');
        assert_eq!(chars(&pieces), "e\u{301}");
    }

    /// The two spellings of the same text must render identically, which is
    /// the point of normalizing before fitting — and is where HarfBuzz, which
    /// fits without normalizing, disagrees with itself.
    #[test]
    fn canonically_equivalent_spellings_fit_the_same_way() {
        let face = |ch: char| matches!(ch, 'c' | '\u{e7}');
        let mut composed = pieces("\u{1e09}", face);
        let mut typed = pieces("c\u{327}\u{301}", face);
        assert_eq!(chars(&composed), chars(&typed));
        assert_eq!(chars(&composed), "\u{e7}\u{301}");
        composed.clear();
        typed.clear();
    }

    /// A face that *can* draw it is left alone, and pays nothing.
    #[test]
    fn a_face_with_the_letter_keeps_it_whole() {
        let mut pieces = nfc("e\u{301}");
        fit_to_face(&mut pieces, |_| true);
        assert_eq!(chars(&pieces), "\u{e9}");
    }

    /// Taking a character apart can expose a piece that is also missing, and
    /// that piece must be taken apart too.
    #[test]
    fn decomposition_recurses_through_missing_pieces() {
        // U+1E09 c-with-cedilla-and-acute decomposes to U+00E7 + U+0301, and
        // U+00E7 decomposes again to c + U+0327.
        let mut pieces = alloc::vec![('\u{1e09}', 0usize)];
        fit_to_face(&mut pieces, |ch| !matches!(ch, '\u{1e09}' | '\u{e7}'));
        assert_eq!(chars(&pieces), "c\u{327}\u{301}");
    }

    /// A character with nothing left to decompose is kept, missing or not:
    /// the box is the honest answer, and looping would not be.
    #[test]
    fn an_undecomposable_missing_character_is_kept() {
        let mut pieces = alloc::vec![('\u{4e2d}', 0usize)];
        fit_to_face(&mut pieces, |_| false);
        assert_eq!(chars(&pieces), "\u{4e2d}");
    }

    /// The tables must be sorted the way the binary searches assume. A
    /// regeneration that emitted them in another order would fail here rather
    /// than silently return wrong characters.
    #[test]
    fn the_generated_tables_are_sorted_as_the_lookups_assume() {
        assert!(PAIRS.is_sorted_by(|a, b| a.0 < b.0), "PAIRS by composed");
        assert!(
            SINGLETONS.is_sorted_by(|a, b| a.0 < b.0),
            "SINGLETONS by composed"
        );
        assert!(NO_COMPOSE.is_sorted_by(|a, b| a < b), "NO_COMPOSE");
        assert!(
            COMBINES_BACKWARD.is_sorted_by(|a, b| a < b),
            "COMBINES_BACKWARD"
        );
        assert!(CCC.is_sorted_by(|a, b| a.1 < b.0), "CCC disjoint");
        assert_eq!(PAIRS_BY_PARTS.len(), PAIRS.len());
        let parts: Vec<(u32, u32)> = PAIRS_BY_PARTS
            .iter()
            .filter_map(|&i| PAIRS.get(usize::from(i)).map(|&(_, a, b)| (a, b)))
            .collect();
        assert_eq!(parts.len(), PAIRS.len(), "an index points outside PAIRS");
        assert!(parts.is_sorted_by(|a, b| a < b), "PAIRS_BY_PARTS by parts");
    }

    /// Every pair in the table must survive a round trip through both
    /// directions, unless it is an exclusion — which is the whole contract
    /// the two lookups have with each other.
    #[test]
    fn every_table_row_composes_back_to_itself() {
        for &(c, a, b) in PAIRS.iter() {
            let (Some(c), Some(a), Some(b)) =
                (char::from_u32(c), char::from_u32(a), char::from_u32(b))
            else {
                unreachable!("table row {c:#X} is not made of characters");
            };
            assert_eq!(
                decompose_once(c, Hangul::Normalize),
                Some((a, Some(b))),
                "decomposing {c:?}"
            );
            let excluded = NO_COMPOSE.binary_search(&(c as u32)).is_ok();
            assert_eq!(
                compose_pair(a, b, Hangul::Normalize) == Some(c),
                !excluded,
                "composing {a:?}+{b:?}"
            );
        }
    }

    /// Normalizing anything must terminate and must not panic, whatever the
    /// input. Marks with no base, lone jamo, unpaired surrogù-adjacent code
    /// points and long mark stacks all go through the same path.
    #[test]
    fn hostile_input_terminates_without_panicking() {
        let cases = [
            "\u{301}",
            "\u{301}\u{301}\u{301}\u{301}",
            "\u{11a8}\u{1161}\u{1100}",
            "a\u{334}\u{335}\u{336}\u{337}\u{338}",
            "\u{ac00}\u{11a8}\u{11a8}",
            "\u{10FFFF}\u{301}",
        ];
        for text in cases {
            let out = nfc(text);
            assert!(
                clusters(&out).is_sorted(),
                "{text:?} produced decreasing clusters {:?}",
                clusters(&out)
            );
            assert!(
                out.iter().all(|&(_, c)| c <= text.len()),
                "{text:?} produced a cluster past the end of the string"
            );
        }
    }
}

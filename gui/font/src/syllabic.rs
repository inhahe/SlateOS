//! The parts of a syllabic shaper that are not about any one script.
//!
//! Indic and Khmer — and Myanmar and USE after them — differ in their grammars
//! and in what their reorderings do, but they agree on the machinery around
//! those: cut the run into syllables, remember which syllable each glyph is
//! in, walk them one at a time, and give a malformed one a dotted circle to
//! hang its marks on. HarfBuzz factors the same three things out, into
//! `hb-ot-shaper-syllabic.cc`, and this is that file.
//!
//! # Why the syllable is stamped on the glyph
//!
//! The obvious thing — remember the ranges the grammar found — does not
//! survive the first lookup. `locl` and `ccmp` run before any reordering and
//! are free to ligate, after which every index past the join names the wrong
//! glyph. So the syllable rides on each glyph in [`SubGlyph::syllable`] and the
//! ranges are re-derived from the run whenever they are wanted. A ligature
//! keeps its first component's stamp and the pieces of a decomposition all keep
//! the whole's, both of which are right: a lookup cannot join glyphs from two
//! syllables in the first place.
//!
//! The byte is a *serial* in the high nibble and the syllable's *kind* in the
//! low one. The serial need only differ between neighbours — all that is ever
//! asked of it is whether two glyphs share a syllable — so it cycles through
//! fifteen values, and never takes zero, so that a glyph no syllable covers is
//! distinguishable from every glyph one does.

use alloc::vec::Vec;

use crate::gsub::SubGlyph;

/// Stamp each glyph with the syllable it belongs to.
///
/// `ranges` tile `glyphs` — the grammars guarantee it, because each ends with
/// a rule matching one character of any category — and `code` turns a range's
/// kind into the low nibble of the stamp, which must therefore fit in four
/// bits.
pub(crate) fn stamp<K: Copy>(
    glyphs: &mut [SubGlyph],
    ranges: &[(usize, usize, K)],
    code: impl Fn(K) -> u8,
) {
    // Stored pre-shifted, so that stamping is an `|` and stepping is an add.
    let mut serial = 0x10u8;
    for &(start, end, kind) in ranges {
        let mark = serial | code(kind);
        for g in glyphs.get_mut(start..end).unwrap_or_default() {
            g.syllable = mark;
        }
        serial = serial.wrapping_add(0x10);
        if serial == 0 {
            serial = 0x10;
        }
    }
}

/// Call `f` on each syllable of `glyphs` in turn.
///
/// The syllable is the maximal run of glyphs sharing a stamp, re-derived every
/// time rather than remembered, for the reason the module documentation gives.
/// `f` is handed the stamp itself — the caller's own [`stamp`] wrote the low
/// nibble, so only the caller can say what kind it means — whether the syllable
/// begins a word, and the syllable as a slice it may permute but not resize.
pub(crate) fn for_each(glyphs: &mut [SubGlyph], mut f: impl FnMut(u8, bool, &mut [SubGlyph])) {
    let mut at = 0usize;
    while at < glyphs.len() {
        let Some(&first) = glyphs.get(at) else { break };
        let end = glyphs
            .iter()
            .enumerate()
            .skip(at)
            .find(|&(_, g)| g.syllable != first.syllable)
            .map_or(glyphs.len(), |(j, _)| j);
        // A syllable begins a word when nothing that continues one precedes
        // it. See [`SubGlyph::word`].
        let word_start = at
            .checked_sub(1)
            .and_then(|prev| glyphs.get(prev))
            .is_none_or(|g| !g.word);
        if let Some(syllable) = glyphs.get_mut(at..end) {
            f(first.syllable, word_start, syllable);
        }
        // The floor of one is unreachable — a syllable is never empty — and is
        // here so the walk terminates without relying on that.
        at = end.max(at.saturating_add(1));
    }
}

/// Give every glyph in `glyphs` the earliest cluster any of them has.
///
/// HarfBuzz's `merge_clusters`, and the reason reordering needs it: a cluster
/// is a byte offset into the source, and reordering moves glyphs past each
/// other, so after it the offsets no longer ascend. A caret placed by one of
/// them would jump backwards inside a word. Merging says what is actually true
/// of a reordered syllable — that it has no interior boundary a caret can
/// honestly point at — by giving the whole of it one offset.
pub(crate) fn merge_clusters(glyphs: &mut [SubGlyph]) {
    let Some(first) = glyphs.iter().map(|g| g.cluster).min() else {
        return;
    };
    for g in glyphs {
        g.cluster = first;
    }
}

/// Forget the syllable boundaries, so that later stages see one run.
///
/// HarfBuzz's `hb_syllabic_clear_var`, which the Khmer shaper runs between its
/// two groups of features. Past that point the confinement is not merely
/// unnecessary but wrong: `pres` and its three companions are declared global,
/// and leaving the stamps in place would confine any lookup they share with a
/// per-syllable feature. <https://github.com/harfbuzz/harfbuzz/issues/3531>
pub(crate) fn clear(glyphs: &mut [SubGlyph]) {
    for g in glyphs.iter_mut() {
        g.syllable = 0;
    }
}

/// Give every broken cluster something to hang its marks on.
///
/// A broken cluster is a matra or a virama with no consonant — text that is not
/// a syllable. Drawing it as written would stack the marks on whatever happened
/// to precede them; the convention every shaper follows is to show them on a
/// dotted circle instead, so that what appears on screen says "these marks
/// belong to nothing" rather than silently corrupting the word before.
///
/// `broken` says which stamps name a broken cluster, and `categorise` tells
/// the freshly built circle what it is — the two things the grammar decides
/// and this does not. `categorise` is a closure rather than a category because
/// the four syllabic shapers do not share one: Indic, Khmer and Myanmar
/// classify with [`indic::Category`](crate::indic::Category) and the Universal
/// Shaping Engine with [`universal::Category`](crate::universal::Category), so
/// the only thing this can hand over is the glyph itself.
///
/// `skip` names the glyphs the circle goes *after* rather than before: the
/// Indic shaper passes a repha, which is drawn above the letter that follows
/// and so must keep a letter after it, and Khmer passes nothing, exactly as
/// HarfBuzz's `repha_category` of `-1` does.
///
/// Nothing happens in a face with no U+25CC, which is the honest answer there:
/// there is no circle to draw.
pub(crate) fn insert_dotted_circles(
    glyphs: &mut Vec<SubGlyph>,
    dotted: Option<u16>,
    categorise: impl Fn(&mut SubGlyph),
    broken: impl Fn(u8) -> bool,
    skip: impl Fn(&SubGlyph) -> bool,
) {
    let Some(gid) = dotted else { return };
    if !glyphs.iter().any(|g| broken(g.syllable)) {
        return;
    }
    let mut out: Vec<SubGlyph> = Vec::with_capacity(glyphs.len().saturating_add(1));
    // No stamp is ever zero, so the first glyph of the run always compares
    // unequal and a broken cluster starting at index zero is not missed.
    let mut last = 0u8;
    let mut i = 0usize;
    while let Some(&g) = glyphs.get(i) {
        if g.syllable == last || !broken(g.syllable) {
            out.push(g);
            i = i.saturating_add(1);
            continue;
        }
        last = g.syllable;
        while let Some(&head) = glyphs.get(i) {
            if head.syllable != last || !skip(&head) {
                break;
            }
            out.push(head);
            i = i.saturating_add(1);
        }
        // Cluster, mask and stamp are the *cluster's own* — taken from the
        // glyph the circle is being inserted in front of, before the skip, so
        // that the circle belongs to the same cluster and is eligible for the
        // same features as the marks it is about to carry.
        let mut circle = SubGlyph {
            gid,
            cluster: g.cluster,
            mask: g.mask,
            syllable: g.syllable,
            ..SubGlyph::cursive(gid, g.cluster, None)
        };
        categorise(&mut circle);
        out.push(circle);
    }
    *glyphs = out;
}

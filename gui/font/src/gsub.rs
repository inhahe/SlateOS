//! `GSUB`: the glyph the font asks for, in place of the one `cmap` gave.
//!
//! # Why this is not a flourish
//!
//! In most serif faces the `f` ends in a hood that overhangs to the right, and
//! the `i` has a dot. Set next to each other at their nominal advances the
//! hood and the dot overlap, or nearly do, and the pair reads as a smudge. The
//! designer's answer is a single `fi` glyph with the collision resolved by
//! hand, plus an instruction in the font to use it. `ffi`, `ffl`, `fl` and
//! `ff` are the same problem. Ignoring the instruction does not render plain
//! text — it renders text the designer specifically marked as broken.
//!
//! That is why these features are *on by default* in every text engine, unlike
//! `dlig` (discretionary ligatures — `ct`, `st`, the decorative ones), which
//! is off by default and stays off here.
//!
//! # How it is applied
//!
//! A `GSUB` table is a *list of lookups*, and the list is applied in order:
//! the whole of the first lookup runs across the whole run before any of the
//! second does, so what the first substitutes is what the second sees. That
//! ordering is the whole mechanism by which `ccmp` — which normalises a run
//! into the glyphs the rest of the table expects — reliably runs before the
//! ligature lookups that depend on it.
//!
//! Within one lookup the subtables are tried in the order the font lists them
//! and the first that matches wins; a glyph one subtable has already
//! substituted is not offered to the next. Positions are walked left to right,
//! once per lookup.
//!
//! # What is read
//!
//! Features, all of them on by default in every engine:
//!
//! * **`ccmp`** — glyph composition and decomposition. Normalises a run so the
//!   later lookups, and mark attachment, see what they expect.
//! * **`liga`** — standard ligatures. The `fi` family.
//! * **`rlig`** — *required* ligatures. For Latin this is nearly empty; for
//!   Arabic, `lam-alef` is not optional, and a face that has it will look
//!   wrong without it. Reading it costs nothing beyond a second tag.
//!
//! Lookup types:
//!
//! * **1, `SingleSubst`** — one glyph for one glyph, in both its formats: a
//!   delta applied to every covered glyph, or an explicit list.
//! * **4, `LigatureSubst`** — several glyphs for one.
//!
//! # What is deliberately not implemented
//!
//! * **Type 2, `MultipleSubst`** (one glyph becomes several) and **type 3,
//!   `AlternateSubst`**. Type 2 needs a run whose glyphs may share a cluster,
//!   which is a change to [`shape`](crate::shape)'s invariants rather than to
//!   this module. Type 3 picks by an alternate index that only a per-run
//!   feature list can supply, and there is none yet — so it has no default-on
//!   caller to serve.
//! * **Types 5 and 6, contextual and chaining-contextual substitution**, which
//!   `clig` and `calt` need. They work by invoking other lookups *by index* at
//!   matched positions, so they are built on top of the single-lookup
//!   application here rather than beside it.
//! * **`dlig`, `hlig`, `swsh`** and the other opt-in features, which are off
//!   by default by design and have no way to be turned on yet — there is no
//!   per-run feature list to turn them on *with*. `locl` is left out for the
//!   opposite reason: it is on by default, but it is *language*-specific, and
//!   applying it without knowing the run's language would give every reader
//!   some other locale's letterforms.
//! * **Script and language selection**, for the reason given in
//!   [`otl`](crate::otl).

use alloc::vec::Vec;

use crate::otl::{Lookup, coverage_index, feature_lookups};
use crate::sfnt::{Span, u16_at};

/// `GSUB` lookup type for single substitution: one glyph for one glyph.
const LOOKUP_SINGLE: u16 = 1;
/// `GSUB` lookup type for ligature substitution: several glyphs for one.
const LOOKUP_LIGATURE: u16 = 4;
/// `GSUB` lookup type for an extension, which wraps a subtable of another type
/// at a 32-bit offset. `GPOS` numbers its own extension 9; the two tables
/// number their lookup types independently.
const LOOKUP_EXTENSION: u16 = 7;

/// A ceiling on how many glyphs one ligature may swallow.
///
/// The largest in real use is four (`ffi`, `ffl`, and Arabic's four-component
/// forms). The cap is what stops a corrupt `componentCount` from making every
/// position in a line scan to the end of it.
const MAX_COMPONENTS: usize = 16;

/// A glyph on its way through substitution.
///
/// Carries its cluster along with its id because substitution is what makes
/// the two diverge: a ligature swallows several glyphs and keeps the first
/// one's cluster, so the run that comes out no longer has one entry per
/// character and only this pass knows which entries merged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubGlyph {
    /// The glyph id, as `cmap` first gave it and each lookup since may have
    /// replaced it.
    pub gid: u16,
    /// Byte offset in the source string of the first character behind this
    /// glyph. A ligature keeps its first component's, which is the only place
    /// a caret can honestly be drawn: the joined glyph has no interior
    /// boundary to point at.
    pub cluster: usize,
}

/// The substitutions of one face, as the lookups to run and in what order.
///
/// Offsets rather than decoded tables, for the same reason as
/// [`Kerning`](crate::kern): the tables are already indexed for lookup, and a
/// face that is drawn with touches a handful of the entries in them.
#[derive(Clone, Debug)]
pub(crate) struct Substitutions {
    /// The lookups reachable from the default-on features, in the order the
    /// font's LookupList puts them, which is the order they apply in.
    lookups: Vec<Lookup>,
}

impl Substitutions {
    /// Find this face's substitutions.
    ///
    /// Returns `None` when the face has no `GSUB`, or has one with no
    /// default-on feature reaching a lookup type this can apply — which is not
    /// an error. Monospace faces in particular have no ligatures by design: a
    /// ligature would break the grid.
    pub(crate) fn parse(data: &[u8], gsub: Option<Span>) -> Option<Self> {
        let lookups = feature_lookups(
            data,
            gsub?.off,
            &[b"ccmp", b"liga", b"rlig"],
            &[LOOKUP_SINGLE, LOOKUP_LIGATURE],
            LOOKUP_EXTENSION,
        )?;
        Some(Self { lookups })
    }

    /// Apply every lookup to `glyphs`, in order, rewriting it in place.
    ///
    /// `glyphs` is one substitution run and the lookups may join anything in
    /// it, so a caller that does not want a ligature to form across some
    /// boundary of its own — a tab, a style change, a bidi run edge — passes
    /// the pieces separately rather than the whole line.
    pub(crate) fn apply(&self, data: &[u8], glyphs: &mut Vec<SubGlyph>) {
        for lookup in &self.lookups {
            match lookup.kind {
                LOOKUP_SINGLE => apply_single(data, &lookup.subtables, glyphs),
                LOOKUP_LIGATURE => apply_ligature(data, &lookup.subtables, glyphs),
                // `feature_lookups` was asked for these two types only, so
                // there is nothing else to reach here; ignoring anything that
                // does is what keeps adding a type to that list from being
                // able to silently corrupt a run.
                _ => {}
            }
        }
    }
}

/// Run one `SingleSubst` lookup across the whole run.
///
/// Every position is independent — nothing here can look at a neighbour — so
/// this is a map, and the run's length and clusters come out unchanged.
fn apply_single(data: &[u8], subtables: &[usize], glyphs: &mut [SubGlyph]) {
    for glyph in glyphs.iter_mut() {
        // First subtable that covers the glyph wins, and the result is not
        // offered to the rest: within one lookup a glyph is substituted once.
        if let Some(gid) = subtables
            .iter()
            .find_map(|&sub| single_at(data, sub, glyph.gid))
        {
            glyph.gid = gid;
        }
    }
}

/// The glyph one `SingleSubst` subtable puts in place of `glyph`.
fn single_at(data: &[u8], sub: usize, glyph: u16) -> Option<u16> {
    // Both formats put the coverage offset in the same place, and neither
    // substitutes a glyph it does not cover.
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let index = coverage_index(data, coverage, glyph)?;
    match u16_at(data, sub)? {
        // Format 1: one delta shared by every covered glyph, for the common
        // case of a block of related forms laid out in the same order as the
        // originals. The spec's arithmetic is modulo 65536, so this wraps
        // rather than saturating or refusing.
        1 => Some(glyph.wrapping_add(u16_at(data, sub.checked_add(4)?)?)),
        // Format 2: an explicit replacement per covered glyph, in coverage
        // order.
        2 => {
            let count = u16_at(data, sub.checked_add(4)?)?;
            if index >= count {
                return None;
            }
            let at = sub
                .checked_add(6)?
                .checked_add(usize::from(index).checked_mul(2)?)?;
            u16_at(data, at)
        }
        _ => None,
    }
}

/// Run one `LigatureSubst` lookup across the whole run.
///
/// Walks left to right, and after joining a ligature carries on from the glyph
/// *after* it: a ligature never feeds itself back into the same lookup, which
/// is what stops a font whose output is also its input from looping.
fn apply_ligature(data: &[u8], subtables: &[usize], glyphs: &mut Vec<SubGlyph>) {
    let mut i = 0usize;
    while let Some(window) = glyphs.get(i..).filter(|w| w.len() >= 2) {
        let Some((gid, count)) = subtables
            .iter()
            .find_map(|&sub| ligature_at(data, sub, window))
        else {
            i = i.saturating_add(1);
            continue;
        };
        let next = i.saturating_add(1);
        if let Some(first) = glyphs.get_mut(i) {
            // The cluster stays as it was: it is the first component's, and
            // the components that follow are being swallowed, not moved.
            first.gid = gid;
        }
        // `ligature_at` never reports more components than the window holds,
        // so this range is inside the run; the clamp is belt and braces.
        let end = i.saturating_add(count).min(glyphs.len());
        glyphs.drain(next.min(end)..end);
        i = next;
    }
}

/// Look for a ligature starting at `glyphs[0]` in one `LigatureSubst`
/// subtable.
fn ligature_at(data: &[u8], sub: usize, glyphs: &[SubGlyph]) -> Option<(u16, usize)> {
    if u16_at(data, sub)? != 1 {
        return None;
    }
    let first = glyphs.first()?.gid;
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let index = coverage_index(data, coverage, first)?;

    let set_count = u16_at(data, sub.checked_add(4)?)?;
    if index >= set_count {
        return None;
    }
    let at = sub
        .checked_add(6)?
        .checked_add(usize::from(index).checked_mul(2)?)?;
    let set = sub.checked_add(usize::from(u16_at(data, at)?))?;

    // The set is ordered by the font, longest first by convention, and the
    // first match wins — which is what makes `ffi` beat `ff` in one pass.
    let count = u16_at(data, set)?;
    for i in 0..usize::from(count) {
        let at = set.checked_add(2)?.checked_add(i.checked_mul(2)?)?;
        let Some(lig) = u16_at(data, at).and_then(|o| set.checked_add(usize::from(o))) else {
            continue;
        };
        if let Some(hit) = ligature_matches(data, lig, glyphs) {
            return Some(hit);
        }
    }
    None
}

/// Test one `Ligature` record against the start of `glyphs`.
///
/// The record lists its components from the *second* onwards: the first is
/// the one the coverage table already matched, so storing it again would be
/// storing it twice.
fn ligature_matches(data: &[u8], lig: usize, glyphs: &[SubGlyph]) -> Option<(u16, usize)> {
    let glyph = u16_at(data, lig)?;
    let components = usize::from(u16_at(data, lig.checked_add(2)?)?);
    if components < 2 || components > MAX_COMPONENTS || components > glyphs.len() {
        return None;
    }
    for i in 1..components {
        let at = lig
            .checked_add(4)?
            .checked_add(i.checked_sub(1)?.checked_mul(2)?)?;
        if u16_at(data, at)? != glyphs.get(i)?.gid {
            return None;
        }
    }
    Some((glyph, components))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
mod tests {
    use super::*;

    fn be16(v: u16) -> [u8; 2] {
        v.to_be_bytes()
    }

    fn span(off: usize, len: usize) -> Span {
        Span { off, len }
    }

    /// Coverage format 1 over a sorted glyph list.
    fn coverage1(glyphs: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(glyphs.len()).unwrap()));
        for g in glyphs {
            out.extend_from_slice(&be16(*g));
        }
        out
    }

    /// One `Ligature` record: the result glyph, then the components after the
    /// first.
    fn ligature(result: u16, rest: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(result));
        out.extend_from_slice(&be16(u16::try_from(rest.len() + 1).unwrap()));
        for g in rest {
            out.extend_from_slice(&be16(*g));
        }
        out
    }

    /// A `LigatureSet`: its records in the order given, which is the order
    /// they are tried in.
    fn ligature_set(records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(u16::try_from(records.len()).unwrap()));
        let mut at = 2 + records.len() * 2;
        for r in records {
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
            at += r.len();
        }
        for r in records {
            out.extend_from_slice(r);
        }
        out
    }

    /// A whole `LigatureSubstFormat1` subtable: one set per covered first
    /// glyph, in coverage order.
    fn ligature_subst(first_glyphs: &[u16], sets: &[Vec<u8>]) -> Vec<u8> {
        assert_eq!(first_glyphs.len(), sets.len());
        let coverage = coverage1(first_glyphs);
        let header = 6 + sets.len() * 2;
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(header).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(sets.len()).unwrap()));
        let mut at = header + coverage.len();
        for s in sets {
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
            at += s.len();
        }
        out.extend_from_slice(&coverage);
        for s in sets {
            out.extend_from_slice(s);
        }
        out
    }

    /// A `GSUB` table with one feature tagged `tag`, one lookup of `kind`, and
    /// `subtable` as that lookup's only subtable.
    fn gsub_table(tag: &[u8; 4], kind: u16, subtable: &[u8]) -> Vec<u8> {
        // header 10 | scriptList (empty, 2) | featureList | lookupList | sub
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1)); // major
        out.extend_from_slice(&be16(0)); // minor
        out.extend_from_slice(&be16(10)); // scriptList
        out.extend_from_slice(&be16(12)); // featureList
        let feature_list = 12usize;
        // FeatureList: count(2) + one 6-byte record = 8, then the Feature.
        let feature = feature_list + 8;
        let feature_len = 6usize; // params + count + one index
        let lookup_list = feature + feature_len;
        out.extend_from_slice(&be16(u16::try_from(lookup_list).unwrap()));
        out.extend_from_slice(&be16(0)); // scriptList: zero scripts

        out.extend_from_slice(&be16(1)); // featureCount
        out.extend_from_slice(tag);
        out.extend_from_slice(&be16(8)); // offset from featureList

        out.extend_from_slice(&be16(0)); // featureParams
        out.extend_from_slice(&be16(1)); // lookupIndexCount
        out.extend_from_slice(&be16(0)); // lookup 0

        // LookupList: count(2) + one offset(2) = 4, then the Lookup.
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(4));
        out.extend_from_slice(&be16(kind));
        out.extend_from_slice(&be16(0)); // flags
        out.extend_from_slice(&be16(1)); // subTableCount
        let lookup = lookup_list + 4;
        let sub_at = out.len() + 2;
        out.extend_from_slice(&be16(u16::try_from(sub_at - lookup).unwrap()));
        out.extend_from_slice(subtable);
        out
    }

    /// `SingleSubstFormat1`: one delta over a covered range.
    fn single_delta(glyphs: &[u16], delta: u16) -> Vec<u8> {
        let coverage = coverage1(glyphs);
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(6)); // coverage follows the header
        out.extend_from_slice(&be16(delta));
        out.extend_from_slice(&coverage);
        out
    }

    /// `SingleSubstFormat2`: an explicit replacement per covered glyph.
    fn single_list(glyphs: &[u16], to: &[u16]) -> Vec<u8> {
        assert_eq!(glyphs.len(), to.len());
        let coverage = coverage1(glyphs);
        let header = 6 + to.len() * 2;
        let mut out = Vec::new();
        out.extend_from_slice(&be16(2));
        out.extend_from_slice(&be16(u16::try_from(header).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(to.len()).unwrap()));
        for g in to {
            out.extend_from_slice(&be16(*g));
        }
        out.extend_from_slice(&coverage);
        out
    }

    /// A `GSUB` table with `features.len()` features and one lookup each, in
    /// the order given — which is both the feature order and the LookupList
    /// order, so a test can say which lookup runs first.
    ///
    /// Kept separate from [`gsub_table`] rather than replacing it: the
    /// single-feature builder is what nearly every test wants, and threading
    /// slices through it would obscure them all to serve two.
    fn gsub_lookups(features: &[(&[u8; 4], u16, Vec<u8>)]) -> Vec<u8> {
        let n = features.len();
        let feature_list = 12usize;
        // count(2) + one 6-byte record each, then one 6-byte Feature each
        // (params, lookupIndexCount, one index).
        let features_at = feature_list + 2 + n * 6;
        let lookup_list = features_at + n * 6;
        // count(2) + one offset each, then one 6-byte Lookup header each
        // (type, flags, subTableCount) plus its single subtable offset.
        let lookups_at = lookup_list + 2 + n * 2;

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1)); // major
        out.extend_from_slice(&be16(0)); // minor
        out.extend_from_slice(&be16(10)); // scriptList
        out.extend_from_slice(&be16(u16::try_from(feature_list).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lookup_list).unwrap()));
        out.extend_from_slice(&be16(0)); // scriptList: zero scripts

        out.extend_from_slice(&be16(u16::try_from(n).unwrap()));
        for (i, (tag, _, _)) in features.iter().enumerate() {
            out.extend_from_slice(*tag);
            let at = features_at + i * 6 - feature_list;
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
        }
        for i in 0..n {
            out.extend_from_slice(&be16(0)); // featureParams
            out.extend_from_slice(&be16(1)); // lookupIndexCount
            out.extend_from_slice(&be16(u16::try_from(i).unwrap()));
        }

        out.extend_from_slice(&be16(u16::try_from(n).unwrap()));
        let mut at = lookups_at;
        for _ in 0..n {
            out.extend_from_slice(&be16(u16::try_from(at - lookup_list).unwrap()));
            at += 8;
        }
        // Every Lookup header is the same size, so the subtables sit in a block
        // after all of them and each offset is computed from its own lookup.
        let mut sub_at = lookups_at + n * 8;
        for (i, (_, kind, subtable)) in features.iter().enumerate() {
            let lookup = lookups_at + i * 8;
            out.extend_from_slice(&be16(*kind));
            out.extend_from_slice(&be16(0)); // flags
            out.extend_from_slice(&be16(1)); // subTableCount
            out.extend_from_slice(&be16(u16::try_from(sub_at - lookup).unwrap()));
            sub_at += subtable.len();
        }
        for (_, _, subtable) in features {
            out.extend_from_slice(subtable);
        }
        out
    }

    /// Run every lookup over `gids` and report what comes out.
    fn subst(data: &[u8], subs: &Substitutions, gids: &[u16]) -> Vec<u16> {
        let mut glyphs: Vec<SubGlyph> = gids
            .iter()
            .enumerate()
            .map(|(i, &gid)| SubGlyph { gid, cluster: i })
            .collect();
        subs.apply(data, &mut glyphs);
        glyphs.iter().map(|g| g.gid).collect()
    }

    /// The clusters `gids` come out with, one source character per glyph in.
    fn clusters(data: &[u8], subs: &Substitutions, gids: &[u16]) -> Vec<usize> {
        let mut glyphs: Vec<SubGlyph> = gids
            .iter()
            .enumerate()
            .map(|(i, &gid)| SubGlyph { gid, cluster: i })
            .collect();
        subs.apply(data, &mut glyphs);
        glyphs.iter().map(|g| g.cluster).collect()
    }

    /// `f`=10, `i`=11, `l`=12, `fi`=20, `ffi`=21, `ff`=22.
    fn fi_font() -> (Vec<u8>, Substitutions) {
        let set_f = ligature_set(&[
            ligature(21, &[10, 11]), // ffi — longest first
            ligature(22, &[10]),     // ff
            ligature(20, &[11]),     // fi
        ]);
        let sub = ligature_subst(&[10], &[set_f]);
        let data = gsub_table(b"liga", LOOKUP_LIGATURE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).expect("liga must parse");
        (data, subs)
    }

    #[test]
    fn a_pair_becomes_one_glyph() {
        let (data, subs) = fi_font();
        assert_eq!(subst(&data, &subs, &[10, 11]), [20]);
    }

    #[test]
    fn the_longest_ligature_wins() {
        let (data, subs) = fi_font();
        // f f i must become `ffi`, not `ff` followed by a stray `i`.
        assert_eq!(subst(&data, &subs, &[10, 10, 11]), [21]);
        // f f alone is still `ff`.
        assert_eq!(subst(&data, &subs, &[10, 10]), [22]);
    }

    #[test]
    fn what_follows_the_ligature_is_kept() {
        let (data, subs) = fi_font();
        assert_eq!(subst(&data, &subs, &[10, 11, 12, 99]), [20, 12, 99]);
    }

    #[test]
    fn a_second_ligature_forms_after_the_first() {
        // The lookup runs across the whole run, not just its start: `fifi` is
        // two ligatures, and a pass that stopped at the first would leave the
        // second pair unjoined.
        let (data, subs) = fi_font();
        assert_eq!(subst(&data, &subs, &[10, 11, 10, 11]), [20, 20]);
    }

    #[test]
    fn a_ligature_keeps_its_first_components_cluster() {
        // A caret can be put before or after `fi` but not inside it, which is
        // only true if the joined glyph reports where the `f` began.
        let (data, subs) = fi_font();
        assert_eq!(clusters(&data, &subs, &[99, 10, 11, 99]), [0, 1, 3]);
    }

    #[test]
    fn a_glyph_outside_the_coverage_never_matches() {
        let (data, subs) = fi_font();
        // `i` starts nothing.
        assert_eq!(subst(&data, &subs, &[11, 10]), [11, 10]);
        // `f` followed by something with no ligature.
        assert_eq!(subst(&data, &subs, &[10, 99]), [10, 99]);
    }

    #[test]
    fn one_glyph_cannot_ligate() {
        let (data, subs) = fi_font();
        assert_eq!(subst(&data, &subs, &[10]), [10]);
        assert!(subst(&data, &subs, &[]).is_empty());
    }

    #[test]
    fn a_required_ligature_is_read_too() {
        // `rlig` is a different tag reaching the same machinery. Arabic needs
        // it; a face that has it is wrong without it.
        let set = ligature_set(&[ligature(30, &[11])]);
        let sub = ligature_subst(&[10], &[set]);
        let data = gsub_table(b"rlig", LOOKUP_LIGATURE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11]), [30]);
    }

    #[test]
    fn a_feature_we_do_not_ask_for_is_left_alone() {
        // `dlig` is off by default: reading it would turn on decorative
        // ligatures nobody asked for.
        let set = ligature_set(&[ligature(30, &[11])]);
        let sub = ligature_subst(&[10], &[set]);
        let data = gsub_table(b"dlig", LOOKUP_LIGATURE, &sub);
        assert!(Substitutions::parse(&data, Some(span(0, data.len()))).is_none());
    }

    #[test]
    fn a_language_specific_feature_is_left_alone() {
        // `locl` is on by default in a shaper that knows the run's language.
        // This one does not, and applying it regardless would hand every
        // reader some other locale's letterforms.
        let data = gsub_table(b"locl", LOOKUP_SINGLE, &single_delta(&[10], 5));
        assert!(Substitutions::parse(&data, Some(span(0, data.len()))).is_none());
    }

    #[test]
    fn no_gsub_means_no_substitutions() {
        assert!(Substitutions::parse(&[], None).is_none());
    }

    #[test]
    fn an_extension_lookup_is_followed() {
        let set = ligature_set(&[ligature(20, &[11])]);
        let inner = ligature_subst(&[10], &[set]);
        // ExtensionSubstFormat1: format, wrapped type, 32-bit offset.
        let mut ext = Vec::new();
        ext.extend_from_slice(&be16(1));
        ext.extend_from_slice(&be16(LOOKUP_LIGATURE));
        ext.extend_from_slice(&8u32.to_be_bytes());
        ext.extend_from_slice(&inner);
        let data = gsub_table(b"liga", LOOKUP_EXTENSION, &ext);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11]), [20]);
    }

    #[test]
    fn a_lookup_type_we_cannot_apply_is_not_mistaken_for_one_we_can() {
        // Type 3, `AlternateSubst`, has the same coverage-then-array shape as
        // the single substitution above, so a walk that ignored the lookup
        // type would happily read it and substitute the wrong glyph.
        let data = gsub_table(b"liga", 3, &single_list(&[10], &[42]));
        assert!(Substitutions::parse(&data, Some(span(0, data.len()))).is_none());
    }

    // ---- single substitution ----

    #[test]
    fn a_single_substitution_replaces_by_delta() {
        let data = gsub_lookups(&[(b"ccmp", LOOKUP_SINGLE, single_delta(&[10, 11], 90))]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [100, 101, 12]);
    }

    #[test]
    fn a_single_substitution_replaces_by_list() {
        let data = gsub_lookups(&[(b"ccmp", LOOKUP_SINGLE, single_list(&[10, 12], &[70, 80]))]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        // 11 is between the two covered glyphs and must be left alone: the
        // replacements are indexed by coverage order, not by glyph id.
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [70, 11, 80]);
    }

    #[test]
    fn a_delta_that_runs_past_the_last_glyph_wraps() {
        // The spec's arithmetic is modulo 65536. Saturating instead would
        // quietly substitute the face's last glyph for a whole covered range.
        let data = gsub_lookups(&[(b"ccmp", LOOKUP_SINGLE, single_delta(&[u16::MAX], 1))]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[u16::MAX]), [0]);
    }

    #[test]
    fn a_single_substitution_does_not_change_the_run_or_its_clusters() {
        let data = gsub_lookups(&[(b"ccmp", LOOKUP_SINGLE, single_delta(&[10], 90))]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(clusters(&data, &subs, &[10, 11, 10]), [0, 1, 2]);
    }

    // ---- lookup ordering ----

    #[test]
    fn an_earlier_lookup_feeds_a_later_one() {
        // This is the whole reason lookups are kept as units. `ccmp` turns 10
        // into 11, and only then does the ligature lookup — which covers 11,
        // not 10 — have anything to join. One flat pass over both subtables
        // would find neither.
        let set = ligature_set(&[ligature(20, &[12])]);
        let data = gsub_lookups(&[
            (b"ccmp", LOOKUP_SINGLE, single_list(&[10], &[11])),
            (b"liga", LOOKUP_LIGATURE, ligature_subst(&[11], &[set])),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 12]), [20]);
    }

    #[test]
    fn a_later_lookup_does_not_feed_an_earlier_one() {
        // The mirror of the test above, and the reason the order is the
        // font's: the same two lookups listed the other way round must *not*
        // ligate, because the ligature lookup runs before the glyph it needs
        // exists. A pass that looped until nothing changed would wrongly
        // ligate here, and would not terminate on a font whose lookups feed
        // each other in a cycle.
        let set = ligature_set(&[ligature(20, &[12])]);
        let data = gsub_lookups(&[
            (b"liga", LOOKUP_LIGATURE, ligature_subst(&[11], &[set])),
            (b"ccmp", LOOKUP_SINGLE, single_list(&[10], &[11])),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 12]), [11, 12]);
    }

    #[test]
    fn a_substitution_is_not_offered_to_the_lookup_that_made_it() {
        // A font whose output is also its input must not loop or cascade
        // inside one lookup: 10 becomes 11 once, not 12.
        let data = gsub_lookups(&[(
            b"ccmp",
            LOOKUP_SINGLE,
            single_list(&[10, 11], &[11, 12]),
        )]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [11]);
    }

    /// A truncated table must come back empty, not panic and not read past
    /// the end. Fonts arrive from the filesystem and are not trusted.
    #[test]
    fn a_truncated_table_is_survivable() {
        let (data, subs) = fi_font();
        for cut in 0..data.len() {
            let short = &data[..cut];
            // Parsing what is left must not panic...
            let _ = Substitutions::parse(short, Some(span(0, short.len())));
            // ...and neither must applying the lookups to it.
            let _ = subst(short, &subs, &[10, 11, 12]);
        }
    }

    /// The same, for a table with two lookups of different types.
    #[test]
    fn a_truncated_multi_lookup_table_is_survivable() {
        let set = ligature_set(&[ligature(20, &[12])]);
        let data = gsub_lookups(&[
            (b"ccmp", LOOKUP_SINGLE, single_delta(&[10], 1)),
            (b"liga", LOOKUP_LIGATURE, ligature_subst(&[11], &[set])),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        for cut in 0..data.len() {
            let short = &data[..cut];
            let _ = Substitutions::parse(short, Some(span(0, short.len())));
            let _ = subst(short, &subs, &[10, 11, 12]);
        }
    }

    #[test]
    fn a_ligature_claiming_more_components_than_exist_is_refused() {
        // componentCount is a u16 the font supplies; a corrupt one must not
        // make the matcher walk off the end of the run.
        let mut lig = Vec::new();
        lig.extend_from_slice(&be16(20));
        lig.extend_from_slice(&be16(u16::MAX));
        let set = ligature_set(&[lig]);
        let sub = ligature_subst(&[10], &[set]);
        let data = gsub_table(b"liga", LOOKUP_LIGATURE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [10, 11, 12]);
    }
}

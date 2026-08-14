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
//! * **2, `MultipleSubst`** — one glyph becomes several, each carrying the
//!   cluster of the character behind it. This is how `ccmp` decomposes a
//!   precomposed letter into a base and a mark so that GPOS can then attach
//!   the mark; without it, a face that ships only the decomposed forms draws
//!   the missing-glyph box for text that is perfectly well spelled.
//! * **4, `LigatureSubst`** — several glyphs for one.
//!
//! # What is deliberately not implemented
//!
//! * **Type 3, `AlternateSubst`**, which picks by an alternate index that only
//!   a per-run feature list can supply, and there is none yet — so it has no
//!   default-on caller to serve.
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
/// `GSUB` lookup type for multiple substitution: one glyph becomes several.
const LOOKUP_MULTIPLE: u16 = 2;
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

/// A ceiling on how many glyphs one glyph may decompose into.
///
/// The inverse of [`MAX_COMPONENTS`] and set to match it: real decompositions
/// are two or three glyphs (a base and its marks), and a font claiming to turn
/// one glyph into thousands is not a font a run should be resized for. The cap
/// bounds the growth of the buffer, which is otherwise the one place
/// substitution can allocate without limit.
const MAX_SEQUENCE: usize = 16;

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
            &[LOOKUP_SINGLE, LOOKUP_MULTIPLE, LOOKUP_LIGATURE],
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
                LOOKUP_MULTIPLE => apply_multiple(data, &lookup.subtables, glyphs),
                LOOKUP_LIGATURE => apply_ligature(data, &lookup.subtables, glyphs),
                // `feature_lookups` was asked for these three types only, so
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

/// Run one `MultipleSubst` lookup across the whole run.
///
/// The run grows: each replaced glyph becomes a sequence, and every glyph of
/// that sequence carries the cluster of the one it replaced, because they all
/// came from the same character. That is what makes `ShapedGlyph::cluster` a
/// many-to-many mapping, and why the queries on
/// [`ShapedRun`](crate::shape::ShapedRun) work in whole clusters.
///
/// Walking resumes *after* the inserted glyphs, so what this lookup produced
/// is not offered back to it — the same rule as everywhere else here, and the
/// reason a font that decomposes A into A cannot loop.
fn apply_multiple(data: &[u8], subtables: &[usize], glyphs: &mut Vec<SubGlyph>) {
    let mut i = 0usize;
    let mut sequence: Vec<u16> = Vec::new();
    while i < glyphs.len() {
        let Some(glyph) = glyphs.get(i).copied() else {
            break;
        };
        if subtables
            .iter()
            .find_map(|&sub| sequence_at(data, sub, glyph.gid, &mut sequence))
            .is_none()
        {
            i = i.saturating_add(1);
            continue;
        }
        // `sequence_at` owns the buffer: it clears on entry and only returns
        // `Some` after pushing at least one glyph, so a match here is never
        // empty and never carries a failed subtable's partial read. Checking
        // emptiness again would be dead code that hides the guard inside.
        let cluster = glyph.cluster;
        let grown = i.saturating_add(sequence.len());
        glyphs.splice(
            i..=i,
            sequence.iter().map(|&gid| SubGlyph { gid, cluster }),
        );
        i = grown;
    }
}

/// The sequence one `MultipleSubst` subtable puts in place of `glyph`, written
/// into `out`.
///
/// `out` is cleared here rather than by the caller, because the caller tries
/// the subtables of a lookup in turn: a subtable that reads half a sequence and
/// then finds the table truncated must not leave those glyphs in front of the
/// next subtable's answer. Clearing on entry makes `out` mean "what the
/// subtable that returned `Some` matched", nothing more. Returning the glyphs
/// through a buffer rather than a fresh `Vec` is what keeps a run of ordinary
/// text — where nothing matches — from allocating once per position.
fn sequence_at(data: &[u8], sub: usize, glyph: u16, out: &mut Vec<u16>) -> Option<()> {
    out.clear();
    // Only one format is defined, and a subtable claiming another is one this
    // cannot read rather than one to guess at.
    if u16_at(data, sub)? != 1 {
        return None;
    }
    let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let index = coverage_index(data, coverage, glyph)?;
    let count = u16_at(data, sub.checked_add(4)?)?;
    if index >= count {
        return None;
    }
    let at = sub
        .checked_add(6)?
        .checked_add(usize::from(index).checked_mul(2)?)?;
    let sequence = sub.checked_add(usize::from(u16_at(data, at)?))?;
    let glyph_count = usize::from(u16_at(data, sequence)?);
    // A sequence of length zero would delete the glyph. The spec forbids it,
    // and some shapers honour it anyway for compatibility — but a deleted
    // glyph takes its cluster with it, and a character that no query can name
    // a position for is worse than a character drawn as it arrived. Refusing
    // here rather than in the caller is what lets a *later* subtable of the
    // same lookup still have its say.
    if glyph_count == 0 || glyph_count > MAX_SEQUENCE {
        return None;
    }
    for i in 0..glyph_count {
        let at = sequence
            .checked_add(2)?
            .checked_add(i.checked_mul(2)?)?;
        out.push(u16_at(data, at)?);
    }
    Some(())
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
        gsub_subtables(tag, kind, &[subtable])
    }

    /// A `GSUB` table with one feature tagged `tag` and one lookup of `kind`
    /// holding every subtable in `subtables`, in the order given.
    ///
    /// Several subtables in one lookup is the case that separates "try the
    /// next subtable" from "give up on this glyph": the font's order is the
    /// order they are tried in, and the first one that matches wins.
    fn gsub_subtables(tag: &[u8; 4], kind: u16, subtables: &[&[u8]]) -> Vec<u8> {
        // header 10 | scriptList (empty, 2) | featureList | lookupList | subs
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
        out.extend_from_slice(&be16(u16::try_from(subtables.len()).unwrap()));
        let lookup = lookup_list + 4;
        // Offsets are measured from the start of the Lookup, and the first
        // subtable begins after the whole offset array.
        let mut at = out.len() + subtables.len() * 2 - lookup;
        for s in subtables {
            out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
            at += s.len();
        }
        for s in subtables {
            out.extend_from_slice(s);
        }
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

    /// `MultipleSubstFormat1`: one sequence per covered glyph.
    ///
    /// `sequences[n]` is what `glyphs[n]` decomposes into. A sequence may be
    /// empty, which is how the "zero glyphs deletes the glyph" case is built.
    fn multiple(glyphs: &[u16], sequences: &[&[u16]]) -> Vec<u8> {
        assert_eq!(glyphs.len(), sequences.len());
        let coverage = coverage1(glyphs);
        // header(6) + one offset per sequence, then the Sequence tables, then
        // the coverage.
        let header = 6 + sequences.len() * 2;
        let mut at = header;
        let mut offsets = Vec::new();
        for seq in sequences {
            offsets.push(at);
            at += 2 + seq.len() * 2;
        }

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1)); // substFormat
        out.extend_from_slice(&be16(u16::try_from(at).unwrap())); // coverage
        out.extend_from_slice(&be16(u16::try_from(sequences.len()).unwrap()));
        for off in &offsets {
            out.extend_from_slice(&be16(u16::try_from(*off).unwrap()));
        }
        for seq in sequences {
            out.extend_from_slice(&be16(u16::try_from(seq.len()).unwrap()));
            for g in *seq {
                out.extend_from_slice(&be16(*g));
            }
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

    #[test]
    fn a_multiple_substitution_decomposes_one_glyph_into_several() {
        // 10 is a precomposed letter the face draws as a base plus a mark.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&[30, 31]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [30, 31]);
        assert_eq!(subst(&data, &subs, &[9, 10, 11]), [9, 30, 31, 11]);
        // Uncovered glyphs are untouched.
        assert_eq!(subst(&data, &subs, &[9, 11]), [9, 11]);
    }

    #[test]
    fn a_decomposed_glyph_gives_every_piece_its_own_characters_cluster() {
        // The clusters are what the layout queries key on: both new glyphs
        // came from the same character, so both must name its byte offset.
        // Anything else and a caret can be drawn between them.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[11], &[&[30, 31, 32]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [10, 30, 31, 32, 12]);
        assert_eq!(clusters(&data, &subs, &[10, 11, 12]), [0, 1, 1, 1, 2]);
    }

    #[test]
    fn every_glyph_of_a_run_is_decomposed_not_just_the_first() {
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10, 12], &[&[30, 31], &[40, 41]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11, 12]), [30, 31, 11, 40, 41]);
        assert_eq!(clusters(&data, &subs, &[10, 11, 12]), [0, 0, 1, 2, 2]);
    }

    #[test]
    fn a_decomposition_is_not_offered_back_to_the_lookup_that_made_it() {
        // 10 decomposes to 30 and 12 — and 12 is itself covered. Walking has
        // to resume *past* the whole insertion, so the 12 this lookup just
        // wrote is left alone; resuming one glyph on would decompose it again
        // and hand back three glyphs.
        let data = gsub_table(
            b"ccmp",
            LOOKUP_MULTIPLE,
            &multiple(&[10, 12], &[&[30, 12], &[40, 41]]),
        );
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [30, 12]);
        // A 12 that was in the run to begin with is still decomposed: the rule
        // is about what this lookup produced, not about the glyph id.
        assert_eq!(subst(&data, &subs, &[12]), [40, 41]);

        // And the degenerate case the same rule has to cover: a glyph that
        // decomposes to itself. Anything re-examining its own output would not
        // terminate here at all, which is why the rule is a rule and not a
        // tidiness preference.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&[10, 30]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [10, 30]);
    }

    #[test]
    fn a_decomposition_feeds_a_later_ligature() {
        // The ordering rule, in the direction `ccmp` exists for: 10 becomes
        // 30 and 31, and only then does a ligature covering 31+12 have
        // anything to join. Cluster 1 is swallowed by the ligature, so the
        // run ends 30(cluster 0), 40(cluster 0) — the ligature keeps its
        // first component's cluster, which is the decomposed character's.
        let set = ligature_set(&[ligature(40, &[12])]);
        let data = gsub_lookups(&[
            (b"ccmp", LOOKUP_MULTIPLE, multiple(&[10], &[&[30, 31]])),
            (b"liga", LOOKUP_LIGATURE, ligature_subst(&[31], &[set])),
        ]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 12]), [30, 40]);
        assert_eq!(clusters(&data, &subs, &[10, 12]), [0, 0]);
    }

    #[test]
    fn a_sequence_of_no_glyphs_leaves_the_glyph_alone() {
        // The spec forbids an empty Sequence; some shapers delete the glyph
        // anyway. Deleting takes the cluster with it, leaving a character no
        // caret position corresponds to, so this refuses instead.
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&[]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10, 11]), [10, 11]);
        assert_eq!(clusters(&data, &subs, &[10, 11]), [0, 1]);
    }

    #[test]
    fn an_empty_sequence_lets_a_later_subtable_of_the_same_lookup_answer() {
        // Refusing an empty Sequence is not the same as giving up on the
        // glyph. The subtables of a lookup are tried in turn, so the refusal
        // has to be *this subtable's* — the next one still gets its say.
        let empty = multiple(&[10], &[&[]]);
        let real = multiple(&[10], &[&[30, 31]]);
        let data = gsub_subtables(b"ccmp", LOOKUP_MULTIPLE, &[&empty, &real]);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [30, 31]);
        assert_eq!(clusters(&data, &subs, &[10]), [0, 0]);
    }

    #[test]
    fn a_subtable_that_reads_half_a_sequence_leaves_nothing_behind() {
        // The first subtable covers 10 and its Sequence claims two glyphs,
        // but the table ends between them: the read collects one glyph and
        // then fails. Those glyphs are not an answer, and must not turn up in
        // front of the one the second subtable does have.
        let real = multiple(&[10], &[&[30, 31]]);
        // A MultipleSubstFormat1 by hand, so its Sequence can be pointed at
        // the very end of the table — the only place a read can run off.
        let mut bad = Vec::new();
        bad.extend_from_slice(&be16(1)); // substFormat
        bad.extend_from_slice(&be16(8)); // coverage, after the offset array
        bad.extend_from_slice(&be16(1)); // sequenceCount
        bad.extend_from_slice(&be16(0)); // sequence offset, patched below
        bad.extend_from_slice(&coverage1(&[10]));

        let mut data = gsub_subtables(b"ccmp", LOOKUP_MULTIPLE, &[&bad, &real]);
        let sub = data.len() - real.len() - bad.len();
        let tail = data.len();
        data.extend_from_slice(&be16(2)); // glyphCount
        data.extend_from_slice(&be16(44)); // glyph 0 — and then nothing
        let offset = be16(u16::try_from(tail - sub).unwrap());
        data[sub + 6..sub + 8].copy_from_slice(&offset);

        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [30, 31]);
    }

    #[test]
    fn a_sequence_longer_than_the_cap_is_refused() {
        let long: Vec<u16> = (0..u16::try_from(MAX_SEQUENCE + 1).unwrap()).collect();
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&long]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [10]);

        // And exactly the cap is still allowed: the bound is on absurdity,
        // not on a font that happens to be at the limit.
        let at_cap: Vec<u16> = (0..u16::try_from(MAX_SEQUENCE).unwrap()).collect();
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10], &[&at_cap]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10]).len(), MAX_SEQUENCE);
    }

    #[test]
    fn a_multiple_substitution_of_an_unknown_format_is_refused() {
        let mut sub = multiple(&[10], &[&[30, 31]]);
        sub[0..2].copy_from_slice(&be16(2));
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &sub);
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(subst(&data, &subs, &[10]), [10]);
    }

    #[test]
    fn a_truncated_multiple_substitution_is_survivable() {
        let data = gsub_table(b"ccmp", LOOKUP_MULTIPLE, &multiple(&[10, 12], &[&[30, 31], &[40]]));
        let subs = Substitutions::parse(&data, Some(span(0, data.len()))).unwrap();
        for cut in 0..data.len() {
            let short = &data[..cut];
            let _ = Substitutions::parse(short, Some(span(0, short.len())));
            let _ = subst(short, &subs, &[10, 11, 12]);
        }
    }
}

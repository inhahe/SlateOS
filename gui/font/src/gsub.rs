//! Ligatures: the glyphs a designer drew for letter combinations that collide.
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
//! This is why the feature is *on by default* in every text engine, unlike
//! `dlig` (discretionary ligatures — `ct`, `st`, the decorative ones), which
//! is off by default and stays off here.
//!
//! # What is read
//!
//! `GSUB` lookup type 4, `LigatureSubst`, reached from the `liga` and `rlig`
//! features:
//!
//! * **`liga`** — standard ligatures. The `fi` family.
//! * **`rlig`** — *required* ligatures. For Latin this is nearly empty; for
//!   Arabic, `lam-alef` is not optional, and a face that has it will look
//!   wrong without it. Reading it costs nothing beyond a second tag.
//!
//! # What is deliberately not implemented
//!
//! * **`clig`** (contextual ligatures) and any other lookup type. Contextual
//!   substitution needs a chaining context matcher, which is a large piece of
//!   machinery for a feature few Latin faces use.
//! * **`dlig`, `hlig`, `swsh`** and the other opt-in features, which are off
//!   by default by design and have no way to be turned on yet — there is no
//!   per-run feature list to turn them on *with*.
//! * **Script and language selection**, for the reason given in
//!   [`otl`](crate::otl).
//! * **Multiple passes.** A real engine applies each lookup across the whole
//!   buffer in turn, so a substitution made by lookup 1 can feed lookup 2.
//!   This makes one left-to-right pass and takes the first subtable that
//!   matches at each position. For ligatures the difference is not
//!   observable: a face lists `ffi` ahead of `ff` in the same ligature set
//!   precisely so that one pass gets it right.

use alloc::vec::Vec;

use crate::otl::{coverage_index, feature_subtables};
use crate::sfnt::{Span, u16_at};

/// `GSUB` lookup type for ligature substitution.
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

/// The ligature substitutions of one face, as a list of subtables to consult.
///
/// Offsets rather than decoded ligatures, for the same reason as
/// [`Kerning`](crate::kern): the tables are already indexed for lookup, and a
/// face that is drawn with touches a handful of the entries in them.
#[derive(Clone, Debug)]
pub(crate) struct Ligatures {
    /// Absolute byte offsets of the `LigatureSubst` subtables, in application
    /// order.
    subtables: Vec<usize>,
}

impl Ligatures {
    /// Find this face's ligature substitutions.
    ///
    /// Returns `None` when the face has no `GSUB`, or has one with no `liga`
    /// or `rlig` feature reaching a ligature lookup — which is not an error.
    /// Monospace faces in particular have none by design: a ligature would
    /// break the grid.
    pub(crate) fn parse(data: &[u8], gsub: Option<Span>) -> Option<Self> {
        let subtables = feature_subtables(
            data,
            gsub?.off,
            &[b"liga", b"rlig"],
            LOOKUP_LIGATURE,
            LOOKUP_EXTENSION,
        )?;
        Some(Self { subtables })
    }

    /// The ligature that replaces the start of `glyphs`, and how many of them
    /// it consumes.
    ///
    /// `glyphs` is the remainder of the run, not the whole of it, so a caller
    /// walks a line by calling this at each position and stepping forward by
    /// the count it gets back (or by one when it gets `None`).
    pub(crate) fn match_at(&self, data: &[u8], glyphs: &[u16]) -> Option<(u16, usize)> {
        // At least two glyphs, or there is nothing to join.
        if glyphs.len() < 2 {
            return None;
        }
        self.subtables
            .iter()
            .find_map(|&sub| ligature_at(data, sub, glyphs))
    }
}

/// Look for a ligature starting at `glyphs[0]` in one `LigatureSubst`
/// subtable.
fn ligature_at(data: &[u8], sub: usize, glyphs: &[u16]) -> Option<(u16, usize)> {
    if u16_at(data, sub)? != 1 {
        return None;
    }
    let first = *glyphs.first()?;
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
fn ligature_matches(data: &[u8], lig: usize, glyphs: &[u16]) -> Option<(u16, usize)> {
    let glyph = u16_at(data, lig)?;
    let components = usize::from(u16_at(data, lig.checked_add(2)?)?);
    if components < 2 || components > MAX_COMPONENTS || components > glyphs.len() {
        return None;
    }
    for i in 1..components {
        let at = lig
            .checked_add(4)?
            .checked_add(i.checked_sub(1)?.checked_mul(2)?)?;
        if u16_at(data, at)? != *glyphs.get(i)? {
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

    /// `f`=10, `i`=11, `l`=12, `fi`=20, `ffi`=21, `ff`=22.
    fn fi_font() -> (Vec<u8>, Ligatures) {
        let set_f = ligature_set(&[
            ligature(21, &[10, 11]), // ffi — longest first
            ligature(22, &[10]),     // ff
            ligature(20, &[11]),     // fi
        ]);
        let sub = ligature_subst(&[10], &[set_f]);
        let data = gsub_table(b"liga", LOOKUP_LIGATURE, &sub);
        let ligs = Ligatures::parse(&data, Some(span(0, data.len()))).expect("liga must parse");
        (data, ligs)
    }

    #[test]
    fn a_pair_becomes_one_glyph() {
        let (data, ligs) = fi_font();
        assert_eq!(ligs.match_at(&data, &[10, 11]), Some((20, 2)));
    }

    #[test]
    fn the_longest_ligature_wins_in_one_pass() {
        let (data, ligs) = fi_font();
        // f f i must become `ffi`, not `ff` followed by a stray `i`.
        assert_eq!(ligs.match_at(&data, &[10, 10, 11]), Some((21, 3)));
        // f f alone is still `ff`.
        assert_eq!(ligs.match_at(&data, &[10, 10]), Some((22, 2)));
    }

    #[test]
    fn what_follows_the_ligature_is_ignored() {
        let (data, ligs) = fi_font();
        assert_eq!(ligs.match_at(&data, &[10, 11, 12, 99]), Some((20, 2)));
    }

    #[test]
    fn a_glyph_outside_the_coverage_never_matches() {
        let (data, ligs) = fi_font();
        // `i` starts nothing.
        assert_eq!(ligs.match_at(&data, &[11, 10]), None);
        // `f` followed by something with no ligature.
        assert_eq!(ligs.match_at(&data, &[10, 99]), None);
    }

    #[test]
    fn one_glyph_cannot_ligate() {
        let (data, ligs) = fi_font();
        assert_eq!(ligs.match_at(&data, &[10]), None);
        assert_eq!(ligs.match_at(&data, &[]), None);
    }

    #[test]
    fn a_required_ligature_is_read_too() {
        // `rlig` is a different tag reaching the same machinery. Arabic needs
        // it; a face that has it is wrong without it.
        let set = ligature_set(&[ligature(30, &[11])]);
        let sub = ligature_subst(&[10], &[set]);
        let data = gsub_table(b"rlig", LOOKUP_LIGATURE, &sub);
        let ligs = Ligatures::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(ligs.match_at(&data, &[10, 11]), Some((30, 2)));
    }

    #[test]
    fn a_feature_we_do_not_ask_for_is_left_alone() {
        // `dlig` is off by default: reading it would turn on decorative
        // ligatures nobody asked for.
        let set = ligature_set(&[ligature(30, &[11])]);
        let sub = ligature_subst(&[10], &[set]);
        let data = gsub_table(b"dlig", LOOKUP_LIGATURE, &sub);
        assert!(Ligatures::parse(&data, Some(span(0, data.len()))).is_none());
    }

    #[test]
    fn no_gsub_means_no_ligatures() {
        assert!(Ligatures::parse(&[], None).is_none());
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
        let ligs = Ligatures::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(ligs.match_at(&data, &[10, 11]), Some((20, 2)));
    }

    /// A truncated table must come back empty, not panic and not read past
    /// the end. Fonts arrive from the filesystem and are not trusted.
    #[test]
    fn a_truncated_table_is_survivable() {
        let (data, ligs) = fi_font();
        for cut in 0..data.len() {
            let short = &data[..cut];
            // Parsing what is left must not panic...
            let _ = Ligatures::parse(short, Some(span(0, short.len())));
            // ...and neither must looking a pair up in it.
            let _ = ligs.match_at(short, &[10, 11, 12]);
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
        let ligs = Ligatures::parse(&data, Some(span(0, data.len()))).unwrap();
        assert_eq!(ligs.match_at(&data, &[10, 11, 12]), None);
    }
}

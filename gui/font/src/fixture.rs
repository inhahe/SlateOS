//! Synthetic `GSUB` tables, for the tests of every module that needs a face.
//!
//! A shaping test needs a font, and a real font is the wrong thing to test
//! against: it is large, it is licensed, and it answers a hundred questions at
//! once when the test asked one. So the tests build the table they need, byte
//! by byte, and these are the builders.
//!
//! They live here rather than in [`gsub`](crate::gsub) — where they grew and
//! where most of their callers still are — because [`indic_shape`](crate::indic_shape)
//! needs them too, and a shaper test that could not build a face would have to
//! be written against a real one or not written at all. Nothing here is
//! specific to substitution: the pieces are a ScriptList, a FeatureList and a
//! LookupList, which is the shape of `GPOS` as well.
//!
//! Everything panics on a value that will not fit the format — a glyph count
//! past `u16`, an offset past `u16`. That is right for a fixture: the caller
//! is a test with a literal in hand, and a builder that silently truncated
//! would produce a table that tests the wrong thing.

// The same allowances the test modules that call these carry: a fixture
// builds a table from literals a test author wrote, so a value that will not
// fit the format is a bug in the test and panicking is the report.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]

use alloc::vec::Vec;

use crate::sfnt::Span;

pub(crate) fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

pub(crate) fn span(off: usize, len: usize) -> Span {
    Span { off, len }
}

/// Coverage format 1 over a sorted glyph list.
pub(crate) fn coverage1(glyphs: &[u16]) -> Vec<u8> {
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
pub(crate) fn ligature(result: u16, rest: &[u16]) -> Vec<u8> {
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
pub(crate) fn ligature_set(records: &[Vec<u8>]) -> Vec<u8> {
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
pub(crate) fn ligature_subst(first_glyphs: &[u16], sets: &[Vec<u8>]) -> Vec<u8> {
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
pub(crate) fn gsub_table(tag: &[u8; 4], kind: u16, subtable: &[u8]) -> Vec<u8> {
    gsub_subtables(tag, kind, &[subtable])
}

/// A `GSUB` table with one feature tagged `tag` and one lookup of `kind`
/// holding every subtable in `subtables`, in the order given.
///
/// Several subtables in one lookup is the case that separates "try the
/// next subtable" from "give up on this glyph": the font's order is the
/// order they are tried in, and the first one that matches wins.
pub(crate) fn gsub_subtables(tag: &[u8; 4], kind: u16, subtables: &[&[u8]]) -> Vec<u8> {
    gsub_scripts(&[(b"DFLT", tag)], kind, subtables)
}

/// A ScriptList: one Script per entry, each with a DefaultLangSys naming
/// the feature indices given and no language-specific systems.
///
/// Every offset inside is relative to the ScriptList's own start, so a
/// caller only has to know where it put the block and how long it came
/// out — not what is inside it.
pub(crate) fn script_list(scripts: &[(&[u8; 4], &[u16])]) -> Vec<u8> {
    let n = scripts.len();
    let mut out = Vec::new();
    out.extend_from_slice(&be16(u16::try_from(n).unwrap()));
    // The Script tables follow the records, each one a 4-byte Script
    // header plus its DefaultLangSys.
    let mut at = 2 + n * 6;
    for (tag, features) in scripts {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
        at += 4 + 6 + features.len() * 2;
    }
    for (_, features) in scripts {
        out.extend_from_slice(&be16(4)); // defaultLangSys, from the Script
        out.extend_from_slice(&be16(0)); // langSysCount
        out.extend_from_slice(&be16(0)); // lookupOrder, always zero
        out.extend_from_slice(&be16(0xFFFF)); // no required feature
        out.extend_from_slice(&be16(u16::try_from(features.len()).unwrap()));
        for f in *features {
            out.extend_from_slice(&be16(*f));
        }
    }
    out
}

/// A `GSUB` table registering one feature per entry of `scripts`, each
/// under its own script tag, and one lookup of `kind` that every one of
/// them selects.
///
/// Several scripts naming the same lookup is the arrangement that matters:
/// it is what a real face supporting two writing systems looks like, and
/// the reason the selection walk starts at the ScriptList rather than the
/// FeatureList — a tag alone does not say which script a feature is for.
pub(crate) fn gsub_scripts(
    scripts: &[(&[u8; 4], &[u8; 4])],
    kind: u16,
    subtables: &[&[u8]],
) -> Vec<u8> {
    // header 10 | scriptList | featureList | lookupList | subtables
    //
    // Each script gets its own Script table with a DefaultLangSys naming
    // exactly one feature — its own, by position in `scripts`.
    let n = scripts.len();
    let indices: Vec<[u16; 1]> = (0..n).map(|i| [u16::try_from(i).unwrap()]).collect();
    let entries: Vec<(&[u8; 4], &[u16])> = scripts
        .iter()
        .zip(&indices)
        .map(|((script, _), idx)| (*script, idx.as_slice()))
        .collect();
    let tags: Vec<&[u8; 4]> = scripts.iter().map(|&(_, tag)| tag).collect();
    gsub_from_scripts(&script_list(&entries), &tags, kind, subtables)
}

/// A `GSUB` table over a caller-built ScriptList: one Feature per entry of
/// `tags`, each naming the single lookup of `kind`.
///
/// Split out from [`gsub_scripts`] so a test can hand in a ScriptList that
/// [`script_list`] cannot express — a script whose features are reachable
/// only through a language system, for one.
pub(crate) fn gsub_from_scripts(
    script_block: &[u8],
    tags: &[&[u8; 4]],
    kind: u16,
    subtables: &[&[u8]],
) -> Vec<u8> {
    gsub_flagged_from_scripts(script_block, tags, kind, 0, 0, subtables)
}

/// A `GSUB` like [`gsub_table`], but with a `lookupFlag` — and, when the
/// flag asks for one, a `markFilteringSet` index — on its single lookup.
///
/// The flag is the whole point of a handful of tests below: every other
/// builder here writes a zero flag, so without this one the skipping walk
/// is only ever exercised in its do-nothing configuration.
pub(crate) fn gsub_flagged(
    tag: &[u8; 4],
    kind: u16,
    flag: u16,
    filter: u16,
    subtable: &[u8],
) -> Vec<u8> {
    gsub_flagged_from_scripts(
        &script_list(&[(b"DFLT", &[0])]),
        &[tag],
        kind,
        flag,
        filter,
        &[subtable],
    )
}

pub(crate) fn gsub_flagged_from_scripts(
    script_block: &[u8],
    tags: &[&[u8; 4]],
    kind: u16,
    flag: u16,
    filter: u16,
    subtables: &[&[u8]],
) -> Vec<u8> {
    let n = tags.len();
    let feature_list = 10 + script_block.len();
    // count(2) + one 6-byte FeatureRecord each, then the Features.
    let features_at = 2 + n * 6;
    let feature_len = 6usize; // params + count + one index
    let lookup_list = feature_list + features_at + n * feature_len;

    let mut out = Vec::new();
    out.extend_from_slice(&be16(1)); // major
    out.extend_from_slice(&be16(0)); // minor
    out.extend_from_slice(&be16(10)); // scriptList
    out.extend_from_slice(&be16(u16::try_from(feature_list).unwrap()));
    out.extend_from_slice(&be16(u16::try_from(lookup_list).unwrap()));
    out.extend_from_slice(script_block);

    out.extend_from_slice(&be16(u16::try_from(n).unwrap())); // featureCount
    for (i, tag) in tags.iter().enumerate() {
        out.extend_from_slice(*tag);
        let at = features_at + i * feature_len;
        out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
    }
    for _ in 0..n {
        out.extend_from_slice(&be16(0)); // featureParams
        out.extend_from_slice(&be16(1)); // lookupIndexCount
        out.extend_from_slice(&be16(0)); // lookup 0
    }

    // LookupList: count(2) + one offset(2) = 4, then the Lookup.
    out.extend_from_slice(&be16(1));
    out.extend_from_slice(&be16(4));
    out.extend_from_slice(&be16(kind));
    out.extend_from_slice(&be16(flag));
    out.extend_from_slice(&be16(u16::try_from(subtables.len()).unwrap()));
    let lookup = lookup_list + 4;
    // `markFilteringSet` sits between the offset array and whatever the
    // offsets point at, so its presence moves the subtables along by two.
    let set = usize::from(flag & 0x0010 != 0) * 2;
    // Offsets are measured from the start of the Lookup, and the first
    // subtable begins after the whole offset array.
    let mut at = out.len() + subtables.len() * 2 + set - lookup;
    for s in subtables {
        out.extend_from_slice(&be16(u16::try_from(at).unwrap()));
        at += s.len();
    }
    if set != 0 {
        out.extend_from_slice(&be16(filter));
    }
    for s in subtables {
        out.extend_from_slice(s);
    }
    out
}

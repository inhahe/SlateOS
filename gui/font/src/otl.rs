//! The tables `GSUB` and `GPOS` have in common.
//!
//! OpenType Layout is two tables with one shape. Both begin with the same
//! header, both reach their real content through the same three lists
//! (script → feature → lookup), and both express "which glyphs does this
//! apply to?" with the same coverage and class-definition tables. Only the
//! *subtables* at the end of that walk differ: `GSUB`'s replace glyphs,
//! `GPOS`'s move them.
//!
//! So the walk lives here once. The alternative — a copy in each of
//! [`kern`](crate::kern) and [`gsub`](crate::gsub) — is how a bounds check
//! ends up in one copy and not the other, and a font that crashes the
//! substitution path but not the positioning path is a bug nobody would
//! think to look for.
//!
//! Every function here is total: a malformed or truncated table yields
//! `None`, never a panic and never a read past the end of `data`. Offsets
//! are absolute byte positions into the whole font file, since that is what
//! the callers cache.
//!
//! # What is not here
//!
//! **Script and language selection.** The walk starts at the FeatureList and
//! takes every feature with a wanted tag, rather than starting at the
//! ScriptList and taking the features the run's script and language actually
//! select. Doing it properly needs the itemised script of the run, which is a
//! shaper's job and comes later; until then, using all of them is wrong only
//! for a face that varies a feature by script, which is rare, and the failure
//! is a slightly wrong gap rather than a wrong glyph.

use alloc::vec::Vec;

use crate::sfnt::{u16_at, u32_at};

/// A limit on how many subtables are followed in total, so that a corrupt or
/// hostile font cannot make shaping arbitrarily slow: every glyph of a run is
/// offered to every subtable, so the cost of a shape is the run length times
/// this number.
///
/// The value is measured, not guessed, and the measurement is the whole point
/// — an earlier 64 here was set from a per-lookup glance ("real fonts use
/// single digits") and silently truncated 61 of the 365 installed faces that
/// have a `GSUB`, so Amiri and FiraCode lost their Latin ligatures entirely
/// while the tests stayed green. Counting what our own enabled features
/// actually reach across every face installed on the development host:
///
/// | measure | worst face | count |
/// |---|---|---|
/// | subtables in one lookup | SansSerifCollection | 675 |
/// | lookups reached | SansSerifCollection | 256 |
/// | subtables in total | SansSerifCollection | 1874 |
/// | (runner-up total) | JetBrains Mono | 768 |
///
/// 8192 is a little over four times the worst real face, which leaves room for
/// the CJK and Indic faces this host does not have without leaving the cost of
/// a hostile font unbounded. A face that does exceed it is shaped with the
/// lookups found so far rather than rejected: a slightly wrong ligature is a
/// better failure than a blank page.
pub(crate) const MAX_SUBTABLES: usize = 8192;

/// One lookup: what it does, and where the subtables that do it live.
///
/// Kept as a unit rather than flattened into one list of subtables because a
/// lookup is the unit of *application*: the whole of lookup 1 runs across the
/// whole buffer before any of lookup 2 does, so a substitution the first makes
/// is visible to the second. Flattening loses that boundary, and with it the
/// only thing that makes `ccmp` reliably run before `liga`.
///
/// Callers that genuinely have one lookup type and no ordering to preserve —
/// pair kerning, mark attachment — use [`feature_subtables`] instead and get
/// the flat list.
#[derive(Clone, Debug)]
pub(crate) struct Lookup {
    /// The lookup type, in the numbering of the table it came from, and with
    /// any extension redirect already resolved: a caller never sees the
    /// extension type itself, only what it wrapped.
    pub(crate) kind: u16,
    /// Absolute byte offsets of this lookup's subtables, in the order the font
    /// lists them — which is the order they are tried in, first match winning.
    pub(crate) subtables: Vec<usize>,
}

/// Offsets of the subtables reachable from the features tagged `tags`.
///
/// `want` is the lookup type whose subtables the caller can read, and
/// `extension` is the lookup type that table uses for its 32-bit-offset
/// redirect (9 in `GPOS`, 7 in `GSUB`) — the two tables number their lookup
/// types independently, so neither can be hardcoded here.
///
/// Returns `None` rather than an empty vector when nothing is found, because
/// every caller treats "this table has nothing for me" as a reason to fall
/// back rather than as a result.
pub(crate) fn feature_subtables(
    data: &[u8],
    base: usize,
    tags: &[&[u8; 4]],
    want: u16,
    extension: u16,
) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for lookup in feature_lookups(data, base, tags, &[want], extension)? {
        out.extend_from_slice(&lookup.subtables);
    }
    (!out.is_empty()).then_some(out)
}

/// The lookups of type `want` reachable from the features tagged `tags`, in
/// the order they must be applied.
///
/// That order is the LookupList's, not the order a feature happens to name
/// them in: the font decides which of its lookups runs first, and a feature
/// only says *which* it needs.
///
/// See [`feature_subtables`] for `extension`, and for why nothing found is
/// `None` rather than an empty vector.
pub(crate) fn feature_lookups(
    data: &[u8],
    base: usize,
    tags: &[&[u8; 4]],
    want: &[u16],
    extension: u16,
) -> Option<Vec<Lookup>> {
    let (lookup_list, indices) = lookup_indices(data, base, tags)?;
    let lookup_count = u16_at(data, lookup_list)?;
    let mut out: Vec<Lookup> = Vec::new();
    // Shared across lookups, so that a font with a hundred small lookups is
    // capped the same way as one with a single enormous lookup.
    let mut budget = MAX_SUBTABLES;
    for idx in indices {
        if idx >= lookup_count || budget == 0 {
            continue;
        }
        let at = lookup_list
            .checked_add(2)?
            .checked_add(usize::from(idx).checked_mul(2)?)?;
        let Some(lookup) = u16_at(data, at).and_then(|o| lookup_list.checked_add(usize::from(o)))
        else {
            continue;
        };
        if let Some(lookup) = read_lookup(data, lookup, want, extension, &mut budget) {
            out.push(lookup);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Where a `GSUB`/`GPOS` table's LookupList begins.
///
/// Contextual lookups name the lookups they invoke by *index into this list*,
/// and the index may be any lookup in the font — including one no feature
/// reaches, which is the usual way a font hides a helper lookup. So a caller
/// that applies them needs the list itself, not just the lookups a feature
/// walk found.
pub(crate) fn lookup_list(data: &[u8], base: usize) -> Option<usize> {
    base.checked_add(usize::from(u16_at(data, base.checked_add(8)?)?))
}

/// One lookup of a LookupList by index, of a type in `want`, extensions
/// unwrapped.
///
/// This is the lookup-by-index that types 5 and 6 need. It re-reads the lookup
/// on every invocation rather than caching it: the alternative is decoding the
/// whole LookupList up front, most of which a run never reaches, and the read
/// is a header and a handful of offsets.
pub(crate) fn lookup_at(
    data: &[u8],
    lookup_list: usize,
    index: u16,
    want: &[u16],
    extension: u16,
    budget: &mut usize,
) -> Option<Lookup> {
    if index >= u16_at(data, lookup_list)? {
        return None;
    }
    let at = lookup_list
        .checked_add(2)?
        .checked_add(usize::from(index).checked_mul(2)?)?;
    let lookup = lookup_list.checked_add(usize::from(u16_at(data, at)?))?;
    read_lookup(data, lookup, want, extension, budget)
}

/// Which lookups the features tagged `tags` use, and where the LookupList is.
///
/// Ascending and deduplicated, because lookups apply in LookupList order
/// regardless of the order a feature happens to list them in, and two features
/// may share one.
fn lookup_indices(data: &[u8], base: usize, tags: &[&[u8; 4]]) -> Option<(usize, Vec<u16>)> {
    let feature_list = base.checked_add(usize::from(u16_at(data, base.checked_add(6)?)?))?;
    let lookup_list = lookup_list(data, base)?;

    let mut indices = Vec::new();
    let feature_count = u16_at(data, feature_list)?;
    for i in 0..usize::from(feature_count) {
        let rec = feature_list.checked_add(2)?.checked_add(i.checked_mul(6)?)?;
        let tag = data.get(rec..rec.checked_add(4)?)?;
        if !tags.iter().any(|want| want.as_slice() == tag) {
            continue;
        }
        let feature = feature_list.checked_add(usize::from(u16_at(data, rec.checked_add(4)?)?))?;
        let count = u16_at(data, feature.checked_add(2)?)?;
        for j in 0..usize::from(count) {
            let at = feature.checked_add(4)?.checked_add(j.checked_mul(2)?)?;
            if let Some(idx) = u16_at(data, at) {
                indices.push(idx);
            }
        }
    }
    indices.sort_unstable();
    indices.dedup();
    (!indices.is_empty()).then_some((lookup_list, indices))
}

/// One lookup, if it is of a type in `want`, with extensions unwrapped.
///
/// `budget` is how many more subtables may be followed in total, decremented
/// as they are taken — a corrupt or hostile font must not be able to make
/// lookup quadratic by declaring thousands of them.
fn read_lookup(
    data: &[u8],
    lookup: usize,
    want: &[u16],
    extension: u16,
    budget: &mut usize,
) -> Option<Lookup> {
    let kind = u16_at(data, lookup)?;
    let count = u16_at(data, lookup.checked_add(4)?)?;
    let extended = kind == extension;
    if !extended && !want.contains(&kind) {
        return None;
    }
    // For an extension lookup the real type is not known until a subtable has
    // been unwrapped. The spec requires every subtable of one lookup to have
    // the same type, so the first one seen settles it and a later disagreement
    // is a malformed font, not a second type to honour.
    let mut effective = if extended { None } else { Some(kind) };
    let mut subtables = Vec::new();
    for i in 0..usize::from(count) {
        if *budget == 0 {
            break;
        }
        let Some(sub) = lookup
            .checked_add(6)
            .and_then(|o| i.checked_mul(2).and_then(|d| o.checked_add(d)))
            .and_then(|at| u16_at(data, at))
            .and_then(|o| lookup.checked_add(usize::from(o)))
        else {
            continue;
        };
        if !extended {
            subtables.push(sub);
            *budget = budget.saturating_sub(1);
            continue;
        }
        // An extension subtable is a three-field redirect: format, the type it
        // wraps, then a 32-bit offset from the extension's own start.
        let Some(wrapped) = sub.checked_add(2).and_then(|o| u16_at(data, o)) else {
            continue;
        };
        if !want.contains(&wrapped) || effective.is_some_and(|k| k != wrapped) {
            continue;
        }
        if let Some(target) = sub
            .checked_add(4)
            .and_then(|o| u32_at(data, o))
            .and_then(|o| usize::try_from(o).ok())
            .and_then(|o| sub.checked_add(o))
        {
            effective = Some(wrapped);
            subtables.push(target);
            *budget = budget.saturating_sub(1);
        }
    }
    let kind = effective?;
    (!subtables.is_empty()).then_some(Lookup { kind, subtables })
}

/// Where `glyph` sits in a coverage table, or `None` if it is not covered.
pub(crate) fn coverage_index(data: &[u8], table: usize, glyph: u16) -> Option<u16> {
    match u16_at(data, table)? {
        1 => {
            let count = u16_at(data, table.checked_add(2)?)?;
            let first = table.checked_add(4)?;
            // A sorted list of glyph ids; the position *is* the index.
            let i = binary_search(usize::from(count), |i| {
                let at = first.checked_add(i.checked_mul(2)?)?;
                Some(u16_at(data, at)?.cmp(&glyph))
            })?;
            u16::try_from(i).ok()
        }
        2 => {
            let count = u16_at(data, table.checked_add(2)?)?;
            let first = table.checked_add(4)?;
            let at = range_containing(data, first, usize::from(count), glyph)?;
            let start = u16_at(data, at)?;
            let base = u16_at(data, at.checked_add(4)?)?;
            // The record says where its first glyph sits; the rest of the
            // range follows on consecutively.
            base.checked_add(glyph.checked_sub(start)?)
        }
        _ => None,
    }
}

/// The class `glyph` belongs to. Class 0 is "everything not listed", which is
/// a real class that a subtable may act on, so an unlisted glyph is `Some(0)`
/// rather than `None`.
pub(crate) fn glyph_class(data: &[u8], table: usize, glyph: u16) -> Option<u16> {
    match u16_at(data, table)? {
        1 => {
            let start = u16_at(data, table.checked_add(2)?)?;
            let count = u16_at(data, table.checked_add(4)?)?;
            let Some(i) = glyph.checked_sub(start).filter(|i| *i < count) else {
                return Some(0);
            };
            let at = table
                .checked_add(6)?
                .checked_add(usize::from(i).checked_mul(2)?)?;
            Some(u16_at(data, at).unwrap_or(0))
        }
        2 => {
            let count = u16_at(data, table.checked_add(2)?)?;
            let first = table.checked_add(4)?;
            let class = range_containing(data, first, usize::from(count), glyph)
                .and_then(|at| u16_at(data, at.checked_add(4)?));
            Some(class.unwrap_or(0))
        }
        _ => None,
    }
}

/// Find the record covering `glyph` in a sorted array of `count` six-byte
/// range records starting at `first`, and return its offset.
///
/// Coverage format 2 and class-definition format 2 have exactly this shape —
/// `start`, `end`, then one payload field — so they share the search. The
/// offset comes back rather than the payload because the two want different
/// fields out of the record, and computing both eagerly is what makes a
/// non-matching probe fail: `glyph - start` underflows on every range the
/// search steps over on its way to the right one.
pub(crate) fn range_containing(
    data: &[u8],
    first: usize,
    count: usize,
    glyph: u16,
) -> Option<usize> {
    binary_search(count, |i| {
        let at = first.checked_add(i.checked_mul(6)?)?;
        let start = u16_at(data, at)?;
        let end = u16_at(data, at.checked_add(2)?)?;
        Some(if end < glyph {
            core::cmp::Ordering::Less
        } else if start > glyph {
            core::cmp::Ordering::Greater
        } else {
            core::cmp::Ordering::Equal
        })
    })
    .and_then(|i| first.checked_add(i.checked_mul(6)?))
}

/// Binary search over `count` records, returning the index of the one whose
/// `probe` reports [`Ordering::Equal`](core::cmp::Ordering::Equal).
///
/// Shared because every sorted array in these tables — coverage lists,
/// coverage ranges, class ranges, pair records, legacy kern pairs — is
/// searched the same way, and writing the loop five times is how an off-by-one
/// gets into one copy and not the others. A `probe` that fails (a truncated
/// table) ends the search rather than reading past the end.
pub(crate) fn binary_search(
    count: usize,
    probe: impl Fn(usize) -> Option<core::cmp::Ordering>,
) -> Option<usize> {
    let mut lo = 0usize;
    let mut hi = count;
    while lo < hi {
        let mid = lo.checked_add(hi.checked_sub(lo)?.checked_div(2)?)?;
        match probe(mid)? {
            core::cmp::Ordering::Less => lo = mid.checked_add(1)?,
            core::cmp::Ordering::Greater => hi = mid,
            core::cmp::Ordering::Equal => return Some(mid),
        }
    }
    None
}

/// Size in bytes of a value record with `format`: one 16-bit field per set bit.
pub(crate) fn value_size(format: u16) -> usize {
    // At most 16 bits are set, so the product is at most 32 and the cast
    // cannot lose anything on any target.
    (format.count_ones() as usize).saturating_mul(2)
}

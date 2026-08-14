//! `GPOS` mark attachment — putting a combining accent where it belongs.
//!
//! A combining mark is a glyph with (usually) no advance: it is drawn on top
//! of whatever precedes it. Without positioning it lands at the pen, which
//! means at the *left edge* of the base glyph's cell — so `e` + U+0301 draws
//! the acute over the gap before the `e` rather than over the `e`, and a
//! sequence of two marks draws them on top of each other. Neither is subtle,
//! and neither is something the font's own metrics can fix: `hmtx` says
//! nothing about where a mark goes, only that it takes no room.
//!
//! `GPOS` answers it with anchors. A base glyph carries an attachment point
//! per *mark class*, a mark carries the point on itself that should meet it,
//! and the mark's displacement is the difference between the two. Three
//! lookups express it:
//!
//! * **MarkBasePos** (type 4) — a mark onto an ordinary glyph. Feature `mark`.
//! * **MarkMarkPos** (type 6) — a mark onto another mark, so the second accent
//!   of a stack sits above the first. Feature `mkmk`.
//! * **MarkLigPos** (type 5) — a mark onto one *component* of a ligature.
//!   Not read: it needs the shaper to remember which component of the
//!   ligature each mark belonged to before substitution collapsed them, which
//!   this stack does not track (a [`ShapedRun`](crate::shape::ShapedRun)
//!   glyph knows its cluster, not its components). A mark on a ligature
//!   therefore falls back to MarkBasePos, which most faces also provide;
//!   where they do not, the mark lands unpositioned rather than wrongly
//!   positioned.
//!
//! Types 4 and 6 have byte-for-byte the same header — coverage for the thing
//! being attached to, coverage for the mark, a class count, and two arrays —
//! so one function reads both. That is not a coincidence to exploit
//! carelessly: it is how the spec defines them, and reading them apart would
//! be two copies of the same bounds checks.
//!
//! # What decides that a glyph is a mark
//!
//! Either answer the font gives: `GDEF`'s `GlyphClassDef` class 3, **or**
//! membership of a mark coverage table. The union, not `GDEF` alone, because
//! `GDEF` alone is measurably wrong on shipping fonts — DejaVu Sans Mono
//! classes `acutecomb` as class 1 (base) while its own `mark` feature carries
//! an anchor for it, so trusting `GDEF` exclusively leaves every accent in
//! that family unattached. And not coverage alone, because a mark the face
//! has no anchor for would then read as a base, and the *next* mark would
//! stack onto it.
//!
//! A real shaper asks Unicode instead: general category `Mn`/`Mc`/`Me` is a
//! property of the character, which is true whatever the font says. That
//! needs a category table this crate does not have yet; the union above
//! covers every face that has anchors to attach with, which is every face
//! where the answer changes anything.

use alloc::vec::Vec;

use crate::otl::{coverage_index, feature_subtables, glyph_class};
use crate::sfnt::{Span, i16_at, u16_at};

/// `GPOS` LookupType 4, mark-to-base.
const LOOKUP_MARK_BASE: u16 = 4;
/// `GPOS` LookupType 6, mark-to-mark.
const LOOKUP_MARK_MARK: u16 = 6;
/// `GPOS` numbers its extension lookup 9. (`GSUB` uses 7.)
const LOOKUP_EXTENSION: u16 = 9;
/// `GDEF` `GlyphClassDef` class 3: a combining mark.
const GDEF_CLASS_MARK: u16 = 3;

/// Where the face wants combining marks placed.
///
/// Holds subtable offsets rather than a decoded table: the anchors are a
/// sparse two-dimensional map over (base glyph, mark class) that almost no
/// run touches, so decoding it eagerly would cost far more than the handful
/// of binary searches a lookup does.
#[derive(Clone, Debug)]
pub(crate) struct MarkPositioning {
    /// MarkBasePos subtables, from the `mark` feature.
    base: Vec<usize>,
    /// MarkMarkPos subtables, from the `mkmk` feature.
    mkmk: Vec<usize>,
    /// `GDEF`'s `GlyphClassDef`, if the face has a readable one.
    class_def: Option<usize>,
}

impl MarkPositioning {
    /// Find the mark-attachment lookups in `GPOS` and the mark classes in
    /// `GDEF`.
    ///
    /// `None` when the face says nothing about marks by either route — no
    /// anchors *and* no `GlyphClassDef` — which is the answer for most
    /// monospace and many display faces, and for every face that only ever
    /// expects precomposed characters.
    ///
    /// A face with `GDEF` classes but no anchors is deliberately kept: it
    /// still knows which glyphs are marks, and that alone is worth having,
    /// because a mark must not advance the pen even when there is nothing to
    /// attach it to.
    pub(crate) fn parse(data: &[u8], gpos: Option<Span>, gdef: Option<Span>) -> Option<Self> {
        let (base, mkmk) = gpos.map_or_else(
            || (Vec::new(), Vec::new()),
            |gpos| {
                let base = feature_subtables(
                    data,
                    gpos.off,
                    &[b"mark"],
                    LOOKUP_MARK_BASE,
                    LOOKUP_EXTENSION,
                )
                .unwrap_or_default();
                let mkmk = feature_subtables(
                    data,
                    gpos.off,
                    &[b"mkmk"],
                    LOOKUP_MARK_MARK,
                    LOOKUP_EXTENSION,
                )
                .unwrap_or_default();
                (base, mkmk)
            },
        );
        let class_def = gdef.and_then(|span| glyph_class_def(data, span));
        if base.is_empty() && mkmk.is_empty() && class_def.is_none() {
            return None;
        }
        Some(Self {
            base,
            mkmk,
            class_def,
        })
    }

    /// Whether `glyph` is a combining mark — one that is drawn onto what
    /// precedes it rather than after it.
    pub(crate) fn is_mark(&self, data: &[u8], glyph: u16) -> bool {
        if self
            .class_def
            .is_some_and(|table| glyph_class(data, table, glyph) == Some(GDEF_CLASS_MARK))
        {
            return true;
        }
        self.base
            .iter()
            .chain(self.mkmk.iter())
            .any(|&sub| in_mark_coverage(data, sub, glyph))
    }

}

/// Read one MarkBasePos or MarkMarkPos subtable.
///
/// The two are the same shape: `posFormat`, the coverage of what is attached
/// *to*, the coverage of the mark, the mark-class count, then the mark array
/// and the array of attachment points. Only the names differ (`base` vs
/// `mark2`), so only one reader is needed.
///
/// The result is the mark's displacement from the base glyph's origin: where
/// the base offers the attachment point, less where on the mark that point is
/// meant to land.
pub(crate) fn attachment(data: &[u8], sub: usize, base: u16, mark: u16) -> Option<(i16, i16)> {
    if u16_at(data, sub)? != 1 {
        return None;
    }
    let mark_coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let base_coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(4)?)?))?;
    let classes = usize::from(u16_at(data, sub.checked_add(6)?)?);
    if classes == 0 {
        return None;
    }
    let mark_array = sub.checked_add(usize::from(u16_at(data, sub.checked_add(8)?)?))?;
    let base_array = sub.checked_add(usize::from(u16_at(data, sub.checked_add(10)?)?))?;

    let mark_index = usize::from(coverage_index(data, mark_coverage, mark)?);
    let base_index = usize::from(coverage_index(data, base_coverage, base)?);

    // MarkArray: a count, then one (class, anchor) pair per covered mark. The
    // anchor offsets inside it are relative to the array, not the subtable.
    if mark_index >= usize::from(u16_at(data, mark_array)?) {
        return None;
    }
    let record = mark_array
        .checked_add(2)?
        .checked_add(mark_index.checked_mul(4)?)?;
    let class = usize::from(u16_at(data, record)?);
    if class >= classes {
        return None;
    }
    let mark_anchor = anchor(data, mark_array, u16_at(data, record.checked_add(2)?)?)?;

    // BaseArray / Mark2Array: a count, then a dense row of `classes` anchor
    // offsets per covered glyph. A row entry may be NULL, meaning this glyph
    // offers no attachment point for that class.
    if base_index >= usize::from(u16_at(data, base_array)?) {
        return None;
    }
    let record = base_array
        .checked_add(2)?
        .checked_add(base_index.checked_mul(classes)?.checked_mul(2)?)?
        .checked_add(class.checked_mul(2)?)?;
    let base_anchor = anchor(data, base_array, u16_at(data, record)?)?;

    Some((
        base_anchor.0.checked_sub(mark_anchor.0)?,
        base_anchor.1.checked_sub(mark_anchor.1)?,
    ))
}

/// One `Anchor` table at `from + offset`, or `None` for the NULL offset.
///
/// All three anchor formats put the coordinates at the same two places; the
/// extra fields are a hinting contour point (format 2) and device tables
/// (format 3), both of which are corrections at specific pixel sizes that
/// this rasterizer does not apply. Ignoring them costs a fraction of a pixel
/// at small sizes; misreading the format would cost the whole placement.
pub(crate) fn anchor(data: &[u8], from: usize, offset: u16) -> Option<(i16, i16)> {
    if offset == 0 {
        return None;
    }
    let at = from.checked_add(usize::from(offset))?;
    if !matches!(u16_at(data, at)?, 1..=3) {
        return None;
    }
    Some((
        i16_at(data, at.checked_add(2)?)?,
        i16_at(data, at.checked_add(4)?)?,
    ))
}

/// Whether `glyph` is in the mark coverage of one mark-attachment subtable.
fn in_mark_coverage(data: &[u8], sub: usize, glyph: u16) -> bool {
    let found = (|| {
        if u16_at(data, sub)? != 1 {
            return None;
        }
        let coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
        coverage_index(data, coverage, glyph)
    })();
    found.is_some()
}

/// `GDEF`'s `GlyphClassDef`, as an absolute offset.
///
/// The offset sits at a fixed place in every `GDEF` version, so the version
/// fields need no interpretation — only the class definition itself has to be
/// one of the two formats we can read, which is checked here so that a lookup
/// on a garbage table fails once at parse time rather than per glyph.
fn glyph_class_def(data: &[u8], gdef: Span) -> Option<usize> {
    let offset = u16_at(data, gdef.off.checked_add(4)?)?;
    if offset == 0 {
        return None;
    }
    let at = gdef.off.checked_add(usize::from(offset))?;
    matches!(u16_at(data, at)?, 1 | 2).then_some(at)
}

#[cfg(test)]
// A test that indexes past the end of its own fixture *should* panic — that
// is the failure being reported, not a defect to guard against.
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

    /// An `Anchor` table, format 1.
    fn anchor1(x: i16, y: i16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&x.to_be_bytes());
        out.extend_from_slice(&y.to_be_bytes());
        out
    }

    /// An attachment point, `(x, y)` in font units.
    type Anchor = (i16, i16);
    /// One BaseArray row: the covered glyph and its anchor per mark class,
    /// `None` where the font writes a NULL offset.
    type BaseRow = (u16, Vec<Option<Anchor>>);

    /// A MarkBasePos/MarkMarkPos subtable over one mark class.
    ///
    /// `marks` is (glyph, class, anchor); `bases` is a row per covered glyph.
    fn mark_subtable(classes: u16, marks: &[(u16, u16, Anchor)], bases: &[BaseRow]) -> Vec<u8> {
        let mark_glyphs: Vec<u16> = marks.iter().map(|m| m.0).collect();
        let base_glyphs: Vec<u16> = bases.iter().map(|b| b.0).collect();

        // MarkArray, self-relative anchors after the records.
        let mut mark_array = Vec::new();
        mark_array.extend_from_slice(&be16(u16::try_from(marks.len()).unwrap()));
        let mut mark_anchors = Vec::new();
        let mark_anchor_base = 2 + marks.len() * 4;
        for (_, class, (x, y)) in marks {
            mark_array.extend_from_slice(&be16(*class));
            mark_array
                .extend_from_slice(&be16(u16::try_from(mark_anchor_base + mark_anchors.len())
                    .unwrap()));
            mark_anchors.extend_from_slice(&anchor1(*x, *y));
        }
        mark_array.extend_from_slice(&mark_anchors);

        // BaseArray, same idea with a dense row per glyph.
        let mut base_array = Vec::new();
        base_array.extend_from_slice(&be16(u16::try_from(bases.len()).unwrap()));
        let mut base_anchors = Vec::new();
        let base_anchor_base = 2 + bases.len() * usize::from(classes) * 2;
        for (_, row) in bases {
            for slot in row {
                match slot {
                    Some((x, y)) => {
                        base_array.extend_from_slice(&be16(
                            u16::try_from(base_anchor_base + base_anchors.len()).unwrap(),
                        ));
                        base_anchors.extend_from_slice(&anchor1(*x, *y));
                    }
                    None => base_array.extend_from_slice(&be16(0)),
                }
            }
        }
        base_array.extend_from_slice(&base_anchors);

        let mark_cov = coverage1(&mark_glyphs);
        let base_cov = coverage1(&base_glyphs);

        let header = 12;
        let mark_cov_at = header;
        let base_cov_at = mark_cov_at + mark_cov.len();
        let mark_array_at = base_cov_at + base_cov.len();
        let base_array_at = mark_array_at + mark_array.len();

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(mark_cov_at).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(base_cov_at).unwrap()));
        out.extend_from_slice(&be16(classes));
        out.extend_from_slice(&be16(u16::try_from(mark_array_at).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(base_array_at).unwrap()));
        out.extend_from_slice(&mark_cov);
        out.extend_from_slice(&base_cov);
        out.extend_from_slice(&mark_array);
        out.extend_from_slice(&base_array);
        out
    }

    /// Where `gpos_table` puts its subtable. Fixed, so that a test that has to
    /// corrupt a specific field can find it; the builder asserts it.
    const SUBTABLE_AT: usize = 36;

    /// A `GPOS` table holding one lookup of `kind` over one subtable, reached
    /// from a feature tagged `tag`.
    fn gpos_table(tag: &[u8; 4], kind: u16, subtable: &[u8]) -> Vec<u8> {
        // header 10 | featureList 14 | lookupList 12 | subtable
        let feature_list_at = 10usize;
        let feature_list = {
            let mut v = Vec::new();
            v.extend_from_slice(&be16(1)); // featureCount
            v.extend_from_slice(tag);
            v.extend_from_slice(&be16(8)); // offset to Feature, from list start
            // Feature at +8
            v.extend_from_slice(&be16(0)); // featureParams
            v.extend_from_slice(&be16(1)); // lookupIndexCount
            v.extend_from_slice(&be16(0)); // lookup 0
            v
        };
        let lookup_list_at = feature_list_at + feature_list.len();
        let lookup_list = {
            let mut v = Vec::new();
            v.extend_from_slice(&be16(1)); // lookupCount
            v.extend_from_slice(&be16(4)); // offset to Lookup, from list start
            // Lookup at +4, eight bytes long, so its subtable follows it.
            v.extend_from_slice(&be16(kind));
            v.extend_from_slice(&be16(0)); // lookupFlag
            v.extend_from_slice(&be16(1)); // subTableCount
            v.extend_from_slice(&be16(8)); // offset to subtable, from Lookup
            v
        };
        let subtable_at = lookup_list_at + lookup_list.len();
        assert_eq!(subtable_at, SUBTABLE_AT, "fixture layout drifted");

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1)); // majorVersion
        out.extend_from_slice(&be16(0)); // minorVersion
        out.extend_from_slice(&be16(0)); // scriptList — unused
        out.extend_from_slice(&be16(u16::try_from(feature_list_at).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lookup_list_at).unwrap()));
        out.extend_from_slice(&feature_list);
        out.extend_from_slice(&lookup_list);
        out.extend_from_slice(subtable);
        out
    }

    /// The mark-to-base attachment this face offers for `mark` on `base`.
    ///
    /// The lookup *walk* is [`gpos`](crate::gpos)'s job now — it has to be, so
    /// that mark attachment is applied in lookup order alongside every other
    /// kind of positioning. What is under test in this file is the subtable
    /// reader, so these two helpers do the one thing the walk would do with a
    /// fixture that has exactly one lookup: try each subtable in order.
    fn on_base(m: &MarkPositioning, data: &[u8], base: u16, mark: u16) -> Option<(i16, i16)> {
        m.base
            .iter()
            .find_map(|&sub| attachment(data, sub, base, mark))
    }

    /// The same, for a mark stacked on another mark.
    fn on_mark(m: &MarkPositioning, data: &[u8], below: u16, mark: u16) -> Option<(i16, i16)> {
        m.mkmk
            .iter()
            .find_map(|&sub| attachment(data, sub, below, mark))
    }

    /// Base glyph 1 with an anchor at (500, 700); mark glyph 2 whose own
    /// anchor is at (100, 0), so it should move by (400, 700).
    fn acute_font() -> Vec<u8> {
        let sub = mark_subtable(1, &[(2, 0, (100, 0))], &[(1, vec![Some((500, 700))])]);
        gpos_table(b"mark", LOOKUP_MARK_BASE, &sub)
    }

    #[test]
    fn a_mark_moves_to_the_base_anchor() {
        let data = acute_font();
        let m = MarkPositioning::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(on_base(&m, &data, 1, 2), Some((400, 700)));
    }

    #[test]
    fn a_glyph_with_no_anchor_is_left_where_it_is() {
        let data = acute_font();
        let m = MarkPositioning::parse(&data, Some(span(0, data.len())), None).unwrap();
        // Glyph 3 is in neither coverage table.
        assert_eq!(on_base(&m, &data, 3, 2), None);
        assert_eq!(on_base(&m, &data, 1, 3), None);
    }

    #[test]
    fn a_null_anchor_declines_the_attachment() {
        // The base is covered but offers nothing for the mark's class.
        let sub = mark_subtable(1, &[(2, 0, (100, 0))], &[(1, vec![None])]);
        let data = gpos_table(b"mark", LOOKUP_MARK_BASE, &sub);
        let m = MarkPositioning::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(on_base(&m, &data, 1, 2), None);
    }

    #[test]
    fn each_mark_class_gets_its_own_anchor() {
        // Two classes: an above-mark and a below-mark on the same base.
        let sub = mark_subtable(
            2,
            &[(2, 0, (0, 0)), (3, 1, (0, 0))],
            &[(1, vec![Some((500, 700)), Some((500, -200))])],
        );
        let data = gpos_table(b"mark", LOOKUP_MARK_BASE, &sub);
        let m = MarkPositioning::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(on_base(&m, &data, 1, 2), Some((500, 700)));
        assert_eq!(on_base(&m, &data, 1, 3), Some((500, -200)));
    }

    #[test]
    fn mark_to_mark_is_a_separate_lookup() {
        let sub = mark_subtable(1, &[(3, 0, (0, 0))], &[(2, vec![Some((0, 900))])]);
        let data = gpos_table(b"mkmk", LOOKUP_MARK_MARK, &sub);
        let m = MarkPositioning::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(on_mark(&m, &data, 2, 3), Some((0, 900)));
        // …and is not reachable through the mark-to-base path.
        assert_eq!(on_base(&m, &data, 2, 3), None);
    }

    #[test]
    fn without_gdef_a_covered_mark_is_a_mark() {
        let data = acute_font();
        let m = MarkPositioning::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert!(m.is_mark(&data, 2), "glyph 2 is in the mark coverage");
        assert!(!m.is_mark(&data, 1), "glyph 1 is the base");
    }

    #[test]
    fn gdef_widens_the_set_of_marks_rather_than_replacing_it() {
        let data = acute_font();
        // GlyphClassDef format 1 over glyphs 4..=5: base, then mark.
        let mut gdef = Vec::new();
        gdef.extend_from_slice(&be16(1)); // majorVersion
        gdef.extend_from_slice(&be16(0)); // minorVersion
        gdef.extend_from_slice(&be16(12)); // glyphClassDefOffset
        gdef.extend_from_slice(&be16(0)); // attachList
        gdef.extend_from_slice(&be16(0)); // ligCaretList
        gdef.extend_from_slice(&be16(0)); // markAttachClassDef
        gdef.extend_from_slice(&be16(1)); // ClassDef format 1
        gdef.extend_from_slice(&be16(4)); // startGlyphID
        gdef.extend_from_slice(&be16(2)); // glyphCount
        gdef.extend_from_slice(&be16(1)); // glyph 4: base
        gdef.extend_from_slice(&be16(GDEF_CLASS_MARK)); // glyph 5: mark

        let mut all = data.clone();
        let gdef_at = all.len();
        all.extend_from_slice(&gdef);
        let m = MarkPositioning::parse(
            &all,
            Some(span(0, data.len())),
            Some(span(gdef_at, gdef.len())),
        )
        .unwrap();
        assert!(
            m.is_mark(&all, 5),
            "GDEF says glyph 5 is a mark, though nothing positions it"
        );
        assert!(
            m.is_mark(&all, 2),
            "glyph 2 is positioned as a mark, though GDEF does not list it — \
             which is DejaVu Sans Mono's actual behaviour"
        );
        assert!(!m.is_mark(&all, 4), "GDEF calls glyph 4 a base");
        assert!(!m.is_mark(&all, 1), "glyph 1 is the base being attached to");
    }

    #[test]
    fn no_gpos_means_no_mark_positioning() {
        assert!(MarkPositioning::parse(&[], None, None).is_none());
    }

    #[test]
    fn a_gpos_without_mark_features_is_not_mark_positioning() {
        let sub = mark_subtable(1, &[(2, 0, (100, 0))], &[(1, vec![Some((500, 700))])]);
        let data = gpos_table(b"kern", LOOKUP_MARK_BASE, &sub);
        assert!(MarkPositioning::parse(&data, Some(span(0, data.len())), None).is_none());
    }

    #[test]
    fn a_truncated_table_is_survivable() {
        let data = acute_font();
        for len in 0..data.len() {
            let cut = &data[..len];
            if let Some(m) = MarkPositioning::parse(cut, Some(span(0, len)), None) {
                let _ = on_base(&m, cut, 1, 2);
                let _ = on_mark(&m, cut, 1, 2);
                let _ = m.is_mark(cut, 2);
            }
        }
    }

    #[test]
    fn an_anchor_offset_past_the_end_is_refused() {
        let mut data = acute_font();
        // The BaseArray offset is the sixth u16 of the subtable; the first
        // row's first anchor offset is the first u16 after that array's count.
        let base_array = SUBTABLE_AT
            + usize::from(u16::from_be_bytes([
                data[SUBTABLE_AT + 10],
                data[SUBTABLE_AT + 11],
            ]));
        data[base_array + 2] = 0xFF;
        data[base_array + 3] = 0xF0;
        let m = MarkPositioning::parse(&data, Some(span(0, data.len())), None).unwrap();
        assert_eq!(
            on_base(&m, &data, 1, 2),
            None,
            "an anchor offset past the end of the font must not resolve"
        );
    }
}

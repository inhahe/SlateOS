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

    /// Whether the face's `GDEF` classifies glyphs at all.
    ///
    /// The question is about the table, not about any one glyph: a face with a
    /// `GlyphClassDef` has stated which of its glyphs are marks, and a glyph it
    /// leaves out is a glyph it declined to call a mark. A face without one has
    /// stated nothing, and the shaper has to work the answer out from the
    /// characters instead — HarfBuzz's `fallback_glyph_classes`, which is
    /// exactly `!hb_ot_layout_has_glyph_classes (face)`.
    pub(crate) fn classifies(&self) -> bool {
        self.class_def.is_some()
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

/// What the mark side of a mark-attachment subtable says, once read.
///
/// Types 4, 5 and 6 have the same first twelve bytes — `posFormat`, the
/// coverage of the mark, the coverage of what it attaches *to*, the mark-class
/// count, the mark array, the array of attachment points — and differ only in
/// the shape of that last array. So the mark side is read once, here, and each
/// type supplies its own way of finding the anchor to attach to.
struct Marked {
    /// Coverage of the glyphs a mark may attach to: bases, ligatures, or the
    /// marks below in a stack.
    to_coverage: usize,
    /// How many mark classes the subtable defines, which is also the width of
    /// a row of the attachment-point array.
    classes: usize,
    /// The array of attachment points — `BaseArray`, `LigatureArray` or
    /// `Mark2Array`. Offsets inside it are relative to it.
    to_array: usize,
    /// Which mark class this mark belongs to: the column of the row to read.
    class: usize,
    /// Where on the mark the attachment point is meant to land.
    at: (i16, i16),
}

impl Marked {
    /// The mark's displacement, given where the glyph below offers its
    /// attachment point.
    fn displacement(&self, to: (i16, i16)) -> Option<(i16, i16)> {
        Some((to.0.checked_sub(self.at.0)?, to.1.checked_sub(self.at.1)?))
    }

    /// The offset of the `class` column of row `row` of a dense
    /// `classes`-wide array of anchor offsets that begins at `from`.
    fn cell(&self, from: usize, row: usize) -> Option<usize> {
        from.checked_add(row.checked_mul(self.classes)?.checked_mul(2)?)?
            .checked_add(self.class.checked_mul(2)?)
    }
}

/// Read the mark side of a MarkBasePos, MarkLigPos or MarkMarkPos subtable.
///
/// `None` when the subtable is a format this cannot read, when the mark is not
/// covered, or when the mark's class is out of range — all of which mean the
/// same thing to a caller: this subtable has nothing to say about this mark.
fn marked(data: &[u8], sub: usize, mark: u16) -> Option<Marked> {
    if u16_at(data, sub)? != 1 {
        return None;
    }
    let mark_coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(2)?)?))?;
    let to_coverage = sub.checked_add(usize::from(u16_at(data, sub.checked_add(4)?)?))?;
    let classes = usize::from(u16_at(data, sub.checked_add(6)?)?);
    if classes == 0 {
        return None;
    }
    let mark_array = sub.checked_add(usize::from(u16_at(data, sub.checked_add(8)?)?))?;
    let to_array = sub.checked_add(usize::from(u16_at(data, sub.checked_add(10)?)?))?;

    let mark_index = usize::from(coverage_index(data, mark_coverage, mark)?);
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
    Some(Marked {
        to_coverage,
        classes,
        to_array,
        class,
        at: anchor(data, mark_array, u16_at(data, record.checked_add(2)?)?)?,
    })
}

/// Read one MarkBasePos or MarkMarkPos subtable.
///
/// The two are the same shape, differing only in the names (`base` vs
/// `mark2`), so only one reader is needed. Which one is being read decides
/// nothing here — it decides only which glyph the caller passes as `base`.
///
/// The result is the mark's displacement from the base glyph's origin: where
/// the base offers the attachment point, less where on the mark that point is
/// meant to land.
pub(crate) fn attachment(data: &[u8], sub: usize, base: u16, mark: u16) -> Option<(i16, i16)> {
    let m = marked(data, sub, mark)?;
    let base_index = usize::from(coverage_index(data, m.to_coverage, base)?);

    // BaseArray / Mark2Array: a count, then a dense row of `classes` anchor
    // offsets per covered glyph. A row entry may be NULL, meaning this glyph
    // offers no attachment point for that class.
    if base_index >= usize::from(u16_at(data, m.to_array)?) {
        return None;
    }
    let record = m.cell(m.to_array.checked_add(2)?, base_index)?;
    m.displacement(anchor(data, m.to_array, u16_at(data, record)?)?)
}

/// Read one MarkLigPos subtable: where a mark goes on one *component* of a
/// ligature.
///
/// A ligature is one glyph standing for several characters, so it offers not
/// one row of attachment points but one per component — a mark typed against
/// the second half of an `ﻻ` has to land over the alef, not over the joined
/// glyph's single origin.
///
/// `component` is which component the mark belongs to, counted from one, as
/// [`Lig`](crate::gsub::Lig) recorded during substitution; `0` means the
/// caller could not tell, and the mark goes on the *last* component. That
/// fallback is HarfBuzz's, and it is the right way round: a mark whose
/// provenance is unknown is far more often the tail of the word than its head,
/// and the last component's anchors are the ones a font is most likely to have
/// filled in for the isolated case.
pub(crate) fn lig_attachment(
    data: &[u8],
    sub: usize,
    lig: u16,
    mark: u16,
    component: u8,
) -> Option<(i16, i16)> {
    let m = marked(data, sub, mark)?;
    let lig_index = usize::from(coverage_index(data, m.to_coverage, lig)?);

    // LigatureArray: a count, then one offset per covered ligature, each to a
    // LigatureAttach table of its own — unlike a BaseArray, whose rows are all
    // the same width and so can be stored inline.
    if lig_index >= usize::from(u16_at(data, m.to_array)?) {
        return None;
    }
    let offset = u16_at(
        data,
        m.to_array
            .checked_add(2)?
            .checked_add(lig_index.checked_mul(2)?)?,
    )?;
    if offset == 0 {
        return None;
    }
    let attach = m.to_array.checked_add(usize::from(offset))?;

    // LigatureAttach: a component count, then that many dense rows of
    // `classes` anchor offsets. The offsets are relative to the LigatureAttach
    // table, not to the LigatureArray — which is the one place this format
    // differs from type 4 in more than naming.
    let components = usize::from(u16_at(data, attach)?);
    let row = match usize::from(component) {
        0 => components.checked_sub(1)?,
        n => n.min(components).checked_sub(1)?,
    };
    let record = m.cell(attach.checked_add(2)?, row)?;
    m.displacement(anchor(data, attach, u16_at(data, record)?)?)
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

    /// A MarkArray: a count, then a (class, anchor offset) pair per mark, with
    /// the anchors laid out after the records and offsets taken from the array.
    fn mark_array_bytes(marks: &[(u16, u16, Anchor)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&be16(u16::try_from(marks.len()).unwrap()));
        let mut anchors = Vec::new();
        let anchor_base = 2 + marks.len() * 4;
        for (_, class, (x, y)) in marks {
            out.extend_from_slice(&be16(*class));
            out.extend_from_slice(&be16(u16::try_from(anchor_base + anchors.len()).unwrap()));
            anchors.extend_from_slice(&anchor1(*x, *y));
        }
        out.extend_from_slice(&anchors);
        out
    }

    /// A dense `classes`-wide grid of anchor offsets, taken from `from` bytes
    /// before the grid — 2 for a BaseArray (past its count), 2 for a
    /// LigatureAttach (past its component count).
    fn anchor_grid(classes: u16, rows: &[Vec<Option<Anchor>>], from: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut anchors = Vec::new();
        let anchor_base = from + rows.len() * usize::from(classes) * 2;
        for row in rows {
            for slot in row {
                match slot {
                    Some((x, y)) => {
                        out.extend_from_slice(&be16(
                            u16::try_from(anchor_base + anchors.len()).unwrap(),
                        ));
                        anchors.extend_from_slice(&anchor1(*x, *y));
                    }
                    None => out.extend_from_slice(&be16(0)),
                }
            }
        }
        out.extend_from_slice(&anchors);
        out
    }

    /// A MarkBasePos/MarkMarkPos subtable over one mark class.
    ///
    /// `marks` is (glyph, class, anchor); `bases` is a row per covered glyph.
    fn mark_subtable(classes: u16, marks: &[(u16, u16, Anchor)], bases: &[BaseRow]) -> Vec<u8> {
        let mark_glyphs: Vec<u16> = marks.iter().map(|m| m.0).collect();
        let base_glyphs: Vec<u16> = bases.iter().map(|b| b.0).collect();

        let mark_array = mark_array_bytes(marks);

        // BaseArray: a count, then one dense row per covered glyph.
        let rows: Vec<Vec<Option<Anchor>>> = bases.iter().map(|b| b.1.clone()).collect();
        let mut base_array = Vec::new();
        base_array.extend_from_slice(&be16(u16::try_from(bases.len()).unwrap()));
        base_array.extend_from_slice(&anchor_grid(classes, &rows, 2));

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

    /// One covered ligature: the glyph, and a row of anchors per component.
    type LigRows = (u16, Vec<Vec<Option<Anchor>>>);

    /// A MarkLigPos subtable.
    ///
    /// Identical to [`mark_subtable`] down to the last offset, except that the
    /// LigatureArray holds *offsets* to per-ligature tables rather than one
    /// grid — because ligatures differ in how many components they have — and
    /// the anchor offsets inside those are taken from the LigatureAttach.
    fn lig_subtable(classes: u16, marks: &[(u16, u16, Anchor)], ligs: &[LigRows]) -> Vec<u8> {
        let mark_glyphs: Vec<u16> = marks.iter().map(|m| m.0).collect();
        let lig_glyphs: Vec<u16> = ligs.iter().map(|l| l.0).collect();

        let mark_array = mark_array_bytes(marks);

        let mut lig_array = Vec::new();
        lig_array.extend_from_slice(&be16(u16::try_from(ligs.len()).unwrap()));
        let mut attachments = Vec::new();
        let attach_base = 2 + ligs.len() * 2;
        for (_, rows) in ligs {
            lig_array
                .extend_from_slice(&be16(u16::try_from(attach_base + attachments.len()).unwrap()));
            attachments.extend_from_slice(&be16(u16::try_from(rows.len()).unwrap()));
            attachments.extend_from_slice(&anchor_grid(classes, rows, 2));
        }
        lig_array.extend_from_slice(&attachments);

        let mark_cov = coverage1(&mark_glyphs);
        let lig_cov = coverage1(&lig_glyphs);

        let header = 12;
        let mark_cov_at = header;
        let lig_cov_at = mark_cov_at + mark_cov.len();
        let mark_array_at = lig_cov_at + lig_cov.len();
        let lig_array_at = mark_array_at + mark_array.len();

        let mut out = Vec::new();
        out.extend_from_slice(&be16(1));
        out.extend_from_slice(&be16(u16::try_from(mark_cov_at).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lig_cov_at).unwrap()));
        out.extend_from_slice(&be16(classes));
        out.extend_from_slice(&be16(u16::try_from(mark_array_at).unwrap()));
        out.extend_from_slice(&be16(u16::try_from(lig_array_at).unwrap()));
        out.extend_from_slice(&mark_cov);
        out.extend_from_slice(&lig_cov);
        out.extend_from_slice(&mark_array);
        out.extend_from_slice(&lig_array);
        out
    }

    /// Ligature glyph 1 of two components, offering (200, 700) over the first
    /// and (800, 700) over the second; mark glyph 2 whose own anchor is at
    /// (100, 0).
    fn lam_alef() -> Vec<u8> {
        lig_subtable(
            1,
            &[(2, 0, (100, 0))],
            &[(1, vec![vec![Some((200, 700))], vec![Some((800, 700))]])],
        )
    }

    #[test]
    fn a_mark_lands_on_the_component_it_belongs_to() {
        let data = lam_alef();
        assert_eq!(lig_attachment(&data, 0, 1, 2, 1), Some((100, 700)));
        assert_eq!(lig_attachment(&data, 0, 1, 2, 2), Some((700, 700)));
    }

    #[test]
    fn an_unknown_component_falls_back_to_the_last() {
        let data = lam_alef();
        assert_eq!(lig_attachment(&data, 0, 1, 2, 0), Some((700, 700)));
    }

    #[test]
    fn a_component_past_the_end_is_clamped_to_the_last() {
        // A mark numbered into a component the ligature does not have — the
        // font's `componentCount` and the substitution disagreeing — must not
        // read a neighbouring table.
        let data = lam_alef();
        assert_eq!(lig_attachment(&data, 0, 1, 2, 9), Some((700, 700)));
    }

    #[test]
    fn a_ligature_component_may_decline_the_attachment() {
        let data = lig_subtable(
            1,
            &[(2, 0, (100, 0))],
            &[(1, vec![vec![None], vec![Some((800, 700))]])],
        );
        assert_eq!(lig_attachment(&data, 0, 1, 2, 1), None);
        assert_eq!(lig_attachment(&data, 0, 1, 2, 2), Some((700, 700)));
    }

    #[test]
    fn an_uncovered_ligature_or_mark_attaches_to_nothing() {
        let data = lam_alef();
        assert_eq!(lig_attachment(&data, 0, 3, 2, 1), None);
        assert_eq!(lig_attachment(&data, 0, 1, 3, 1), None);
    }

    #[test]
    fn each_mark_class_reads_its_own_column_of_the_component() {
        // Two classes over a two-component ligature: an above-mark and a
        // below-mark, each with its own anchor on each component.
        let data = lig_subtable(
            2,
            &[(2, 0, (0, 0)), (3, 1, (0, 0))],
            &[(
                1,
                vec![
                    vec![Some((200, 700)), Some((200, -100))],
                    vec![Some((800, 700)), Some((800, -100))],
                ],
            )],
        );
        assert_eq!(lig_attachment(&data, 0, 1, 2, 1), Some((200, 700)));
        assert_eq!(lig_attachment(&data, 0, 1, 3, 1), Some((200, -100)));
        assert_eq!(lig_attachment(&data, 0, 1, 2, 2), Some((800, 700)));
        assert_eq!(lig_attachment(&data, 0, 1, 3, 2), Some((800, -100)));
    }

    #[test]
    fn a_ligature_with_no_components_attaches_nothing() {
        // `componentCount` of zero: legal to encode, meaningless to read, and
        // the one input that would make the fallback's "last component"
        // underflow.
        let data = lig_subtable(1, &[(2, 0, (100, 0))], &[(1, vec![])]);
        assert_eq!(lig_attachment(&data, 0, 1, 2, 0), None);
        assert_eq!(lig_attachment(&data, 0, 1, 2, 1), None);
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

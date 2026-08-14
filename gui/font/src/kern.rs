//! Pair kerning: how much closer together (or further apart) than their
//! advances two particular glyphs should sit.
//!
//! # Why this is not optional
//!
//! An advance width is a property of one glyph, chosen so that the glyph looks
//! right next to an *average* neighbour. Some pairs are not average. The
//! classic is `AV`: both are diagonal, and set at their nominal advances they
//! leave a hole wide enough to look like a word space. `To`, `Yo`, `P.`, `r.`
//! and `f)` are the same problem in other directions. A designer records the
//! correction for each such pair in the font, and a text engine that ignores
//! it is throwing away information the designer put there specifically because
//! the advance widths alone are wrong.
//!
//! # Where the corrections live, and why there are two places
//!
//! Two tables carry them, for historical reasons:
//!
//! * **`GPOS`**, the OpenType Layout positioning table. Kerning is the `kern`
//!   *feature*, which points at `PairPos` lookups. This is the modern,
//!   authoritative source, and the only one in a font that has both.
//! * **`kern`**, the original TrueType table. Simple, flat, and still shipped
//!   by many fonts — including Microsoft's core web fonts — for the benefit of
//!   engines that predate OpenType Layout.
//!
//! Of the 541 faces installed on the development host, 321 have `GPOS`, 227
//! have `kern`, 133 have both and 126 have neither. Supporting only one of the
//! two would leave either 188 or 94 faces unkerned for no reason, so both are
//! read, with `GPOS` preferred wherever it carries a `kern` feature — a font
//! that ships both intends `GPOS` to win, and the legacy table is often a
//! lossy subset of it.
//!
//! # What is deliberately not implemented
//!
//! * **Script and language selection.** Every feature tagged `kern` is used,
//!   whichever script system lists it. Per-script kerning differences are rare
//!   in practice (the feature is nearly always shared), and honouring them
//!   requires knowing the script of the run — which is a shaper's job, and
//!   there is no shaper yet.
//! * **Contextual and cross-stream kerning**, `GPOS` lookup types other than
//!   `PairPos`, and the legacy table's format 2 (class-based) subtables. Each
//!   is a small fraction of the corrections in a font and none can be applied
//!   correctly by a pair-at-a-time interface.
//! * **Device tables.** They tune a value for one specific ppem; the value
//!   without them is the designer's intent at any size.
//!
//! # Reading across a mark
//!
//! Real faces mark their `kern` lookups `IgnoreMarks`, so that `A` and `V`
//! still kern with an accent between them. A pair-at-a-time interface cannot
//! step over the accent by itself, so the caller says what stood between the
//! pair and this decides, per lookup, whether that lookup's flag would have
//! let it see past — which is the same question
//! [`Skipper`](crate::skip::Skipper) answers for `GSUB`. That is why the
//! subtables are grouped by lookup here rather than flattened: the flag
//! belongs to the lookup, and two lookups reached by the same feature may
//! disagree about it.
//!
//! Because this reads a *pair* at a time, it is not a shaper and cannot become
//! one: real shaping reorders and substitutes glyphs before positioning them.
//! It is the part of shaping that can be done without knowing anything about
//! the script, and it is the part that Latin text most visibly needs.

use alloc::vec::Vec;

use crate::gpos::pair_values;
use crate::otl::{MAX_SUBTABLES, binary_search, feature_lookups};
use crate::sfnt::{Span, i16_at, u16_at};
use crate::skip::{Definitions, Skipper};

/// `GPOS` lookup type for pair positioning.
const LOOKUP_PAIR_POS: u16 = 2;
/// `GPOS` lookup type for an extension, which wraps a subtable of another type
/// at a 32-bit offset so that a large table can exceed the 16-bit offsets used
/// everywhere else. `GSUB` numbers its own extension differently, which is why
/// the shared walk takes it as a parameter.
const LOOKUP_EXTENSION: u16 = 9;

/// The pair-kerning data of one face, as a list of subtables to consult.
///
/// Offsets rather than decoded pairs: a large font's kerning runs to tens of
/// thousands of pairs, they are already stored in a form built for binary
/// search, and a face that is opened and drawn with will touch a few hundred
/// of them. Decoding at parse time would cost more memory and more time than
/// looking them up on demand ever does.
#[derive(Clone, Debug)]
pub(crate) struct Kerning {
    /// The `GPOS` `PairPos` subtables the `kern` feature reaches, in
    /// application order, grouped by the lookup that owns them.
    ///
    /// Grouped rather than flattened because a lookup's `lookupFlag` says
    /// which glyphs the pair may be read *across* — `IgnoreMarks` is on
    /// virtually every real `kern` lookup, and is what makes `A` and `V` still
    /// kern with an accent between them. A flat list of subtables has thrown
    /// that away.
    gpos: Vec<Group>,
    /// The legacy `kern` table's usable format-0 subtables, in order.
    ///
    /// Kept even when `gpos` is non-empty, because which of the two a *run*
    /// should read is not a property of the face: `GPOS` files its `kern`
    /// feature under particular scripts, and a run whose script the table does
    /// not register reaches no `kern` feature at all and must fall back here.
    /// See [`Kerning::legacy_pair`].
    legacy: Vec<usize>,
    /// `GDEF`'s class definitions, which is what turns a flag bit into a set
    /// of glyphs.
    defs: Definitions,
}

/// One lookup's subtables, with the flag that governs reading across glyphs.
#[derive(Clone, Debug)]
struct Group {
    flag: u16,
    filter: u16,
    subtables: Vec<usize>,
}

impl Kerning {
    /// Find this face's kerning, from `GPOS`, the legacy table, or both.
    ///
    /// Returns `None` when the face has neither, or has them but they contain
    /// nothing this reads — which is not an error: most monospace faces and
    /// many display faces genuinely have no kerning.
    ///
    /// Both are kept when both are present. Which one a *run* should read is a
    /// question about the run's script, not about the face — see
    /// [`legacy_pair`](Self::legacy_pair) — and a parse that threw one away
    /// could not answer it later.
    pub(crate) fn parse(
        data: &[u8],
        gpos: Option<Span>,
        legacy: Option<Span>,
        gdef: Option<Span>,
    ) -> Option<Self> {
        let gpos = gpos.and_then(|span| parse_gpos(data, span)).unwrap_or_default();
        let legacy = legacy
            .and_then(|span| parse_legacy(data, span))
            .unwrap_or_default();
        if gpos.is_empty() && legacy.is_empty() {
            return None;
        }
        Some(Self {
            gpos,
            legacy,
            defs: Definitions::parse(data, gdef),
        })
    }

    /// Whether the pairs [`pair`](Self::pair) reads come from the legacy `kern`
    /// table rather than from `GPOS`.
    ///
    /// The distinction decides whether a shaper that runs the full `GPOS`
    /// positioning pass should *also* walk pairs through here. It should not
    /// when the pairs are `GPOS`'s: the pass has already applied those lookups,
    /// in order and with every other lookup interleaved, and charging them a
    /// second time would double every kern. It must when they are the legacy
    /// table's, since the pass cannot see that table at all — which is also
    /// where HarfBuzz draws the line (`hb_ot_layout_has_kerning`, consulted
    /// only when `GPOS` carries no `kern` feature).
    ///
    /// This is the *face-wide* answer, which is the only one a pair-at-a-time
    /// interface with no run behind it can give. A shaper that has a run should
    /// ask [`has_legacy`](Self::has_legacy) and its own script instead.
    #[must_use]
    pub(crate) fn is_legacy(&self) -> bool {
        self.gpos.is_empty()
    }

    /// Whether the face ships a legacy `kern` table this can read, whatever
    /// `GPOS` also offers.
    #[must_use]
    pub(crate) fn has_legacy(&self) -> bool {
        !self.legacy.is_empty()
    }

    /// The adjustment to `left`'s advance when `right` follows it, in font
    /// units. Negative pulls the pair together, which is the common case.
    ///
    /// `between` is what stands between the two in the run — normally nothing.
    /// A lookup is consulted only if its flag would have let it see past
    /// *every* glyph in `between`, which is what lets `A` and `V` kern with an
    /// accent between them without letting them kern across a letter.
    ///
    /// The first subtable with an entry for the pair wins, matching the way
    /// OpenType applies lookups in order — a later lookup positions the output
    /// of an earlier one, so for a single adjustment the earliest match is the
    /// one that would have been applied.
    pub(crate) fn pair(&self, data: &[u8], left: u16, right: u16, between: &[u16]) -> i16 {
        for group in &self.gpos {
            if !between.is_empty() {
                let skip = Skipper::new(data, self.defs, group.flag, group.filter, u64::MAX);
                if !between.iter().all(|&g| skip.skips(g)) {
                    continue;
                }
            }
            for &off in &group.subtables {
                if let Some(v) = pair_pos(data, off, left, right) {
                    return v;
                }
            }
        }
        if self.gpos.is_empty() {
            return self.legacy_pair(data, left, right, between);
        }
        0
    }

    /// The same, read from the legacy `kern` table only.
    ///
    /// What a run whose script reaches no `GPOS` `kern` feature wants, even on
    /// a face whose `GPOS` kerns some *other* script — HarfBuzz's `apply_kern`,
    /// which is switched on by `!has_gpos_kern`, and `has_gpos_kern` is looked
    /// up in the plan's selected script rather than across the whole table.
    ///
    /// The legacy table predates lookups and has no flags, so it skips nothing:
    /// that is the historically correct reading, since the engines that used it
    /// kerned strictly adjacent glyphs.
    pub(crate) fn legacy_pair(&self, data: &[u8], left: u16, right: u16, between: &[u16]) -> i16 {
        if !between.is_empty() {
            return 0;
        }
        for &off in &self.legacy {
            if let Some(v) = legacy_pair(data, off, left, right) {
                return v;
            }
        }
        0
    }
}

// ---------------------------------------------------------------------------
// GPOS
// ---------------------------------------------------------------------------

/// Collect the `PairPos` lookups reachable from `GPOS`'s `kern` feature.
///
/// Returns `None` if the table is malformed, has no `kern` feature, or that
/// feature reaches no pair-positioning subtable — in all of which cases the
/// caller should fall back to the legacy table rather than conclude the face
/// has no kerning.
fn parse_gpos(data: &[u8], span: Span) -> Option<Vec<Group>> {
    let out: Vec<Group> = feature_lookups(
        data,
        span.off,
        &[b"kern"],
        LOOKUP_PAIR_POS,
        LOOKUP_EXTENSION,
    )?
    .into_iter()
    .map(|l| Group {
        flag: l.flag,
        filter: l.filter,
        subtables: l.subtables,
    })
    .collect();
    (!out.is_empty()).then_some(out)
}

/// Look `left`/`right` up in one `PairPos` subtable, keeping only the first
/// glyph's advance adjustment — which is the whole of kerning, and all a
/// pair-at-a-time interface can express.
///
/// The subtable itself is decoded by [`pair_values`], which is the same reader
/// the run-based positioning pass uses. Two readers for one table format is one
/// too many: a disagreement between them would show up as a label that measures
/// differently from the way it draws.
///
/// A subtable that has a record for the pair ends the search even when the
/// value is zero, and even when its value format carries no advance at all: the
/// font has stated what happens to this pair, and a later subtable restating it
/// would never have been reached.
fn pair_pos(data: &[u8], sub: usize, left: u16, right: u16) -> Option<i16> {
    let (first, _, _) = pair_values(data, sub, left, right)?;
    Some(first.x_advance)
}

// ---------------------------------------------------------------------------
// The legacy `kern` table
// ---------------------------------------------------------------------------

/// Coverage bit: the subtable kerns horizontally. Vertical kerning applies to
/// vertical writing, which nothing here lays out.
const KERN_HORIZONTAL: u16 = 0x0001;
/// Coverage bit: the values are minimums to enforce rather than adjustments to
/// apply, which needs the pair's actual spacing and so cannot be honoured here.
const KERN_MINIMUM: u16 = 0x0002;
/// Coverage bit: the adjustment is perpendicular to the writing direction.
const KERN_CROSS_STREAM: u16 = 0x0004;

/// Collect the usable format-0 subtables of a legacy `kern` table.
fn parse_legacy(data: &[u8], span: Span) -> Option<Vec<usize>> {
    let base = span.off;
    // Version 0 is the Microsoft layout. Apple's version 1 table has a
    // 32-bit version and a different subtable header; fonts that ship one are
    // Mac-only and also ship GPOS, so it is not read.
    if u16_at(data, base)? != 0 {
        return None;
    }
    let count = u16_at(data, base.checked_add(2)?)?;
    let mut at = base.checked_add(4)?;
    let mut subtables = Vec::new();
    for _ in 0..usize::from(count) {
        if subtables.len() >= MAX_SUBTABLES {
            break;
        }
        let length = usize::from(u16_at(data, at.checked_add(2)?)?);
        let coverage = u16_at(data, at.checked_add(4)?)?;
        let format = coverage >> 8;
        let usable = format == 0
            && coverage & KERN_HORIZONTAL != 0
            && coverage & (KERN_MINIMUM | KERN_CROSS_STREAM) == 0;
        if usable {
            subtables.push(at);
        }
        // A zero length would loop forever; a length past the end is a
        // truncated table, and either way there is nothing after this.
        if length < 6 || at.checked_add(length).is_none_or(|e| e > data.len()) {
            break;
        }
        at = at.checked_add(length)?;
    }
    (!subtables.is_empty()).then_some(subtables)
}

/// Look `left`/`right` up in one format-0 `kern` subtable.
fn legacy_pair(data: &[u8], sub: usize, left: u16, right: u16) -> Option<i16> {
    let pairs = u16_at(data, sub.checked_add(6)?)?;
    // nPairs is followed by searchRange/entrySelector/rangeShift, which
    // describe a binary search the reader can do for itself.
    let first = sub.checked_add(14)?;
    // The pairs are sorted by the two glyph ids taken together as one 32-bit
    // key, which is exactly what comparing the tuple does.
    let key = (left, right);
    let found = binary_search(usize::from(pairs), |i| {
        let at = first.checked_add(i.checked_mul(6)?)?;
        let l = u16_at(data, at)?;
        let r = u16_at(data, at.checked_add(2)?)?;
        Some((l, r).cmp(&key))
    })?;
    i16_at(data, first.checked_add(found.checked_mul(6)?)?.checked_add(4)?)
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
    use alloc::vec;

    fn be16(v: u16) -> [u8; 2] {
        v.to_be_bytes()
    }

    fn span(off: usize, len: usize) -> Span {
        Span { off, len }
    }

    /// The `valueFormat` bit for XAdvance, which is the only field these
    /// fixtures carry. Written out here rather than imported because it is
    /// part of the *bytes being built*, not of the code under test — the
    /// fixture has to say what it wrote even if the reader's name for it moves.
    const X_ADVANCE: u16 = 0x0004;

    /// A coverage format 1 table listing `glyphs`.
    fn coverage1(glyphs: &[u16]) -> Vec<u8> {
        let mut t = vec![];
        t.extend(be16(1));
        t.extend(be16(u16::try_from(glyphs.len()).unwrap()));
        for g in glyphs {
            t.extend(be16(*g));
        }
        t
    }

    /// A `PairPos` format 1 subtable kerning 'A' (gid 1) before 'V' (gid 2) by
    /// -80 and before 'W' (gid 3) by -50.
    fn pair_pos_1_subtable() -> Vec<u8> {
        // Layout: header (10) + one pairSetOffset (2) = 12, then the set.
        let cov_off = 12 + 2 + 3 * 4;
        let mut t = vec![];
        t.extend(be16(1)); // posFormat
        t.extend(be16(u16::try_from(cov_off).unwrap()));
        t.extend(be16(X_ADVANCE)); // valueFormat1
        t.extend(be16(0)); // valueFormat2
        t.extend(be16(1)); // pairSetCount
        t.extend(be16(12)); // pairSetOffset[0]
        // PairSet at 12.
        t.extend(be16(3)); // pairValueCount
        t.extend(be16(2));
        t.extend((-80_i16).to_be_bytes());
        t.extend(be16(3));
        t.extend((-50_i16).to_be_bytes());
        t.extend(be16(9));
        t.extend(be16(0));
        assert_eq!(t.len(), cov_off);
        t.extend(coverage1(&[1]));
        t
    }

    #[test]
    fn pair_pos_format_1_finds_the_pair_and_only_the_pair() {
        let t = pair_pos_1_subtable();
        assert_eq!(pair_pos(&t, 0, 1, 2), Some(-80));
        assert_eq!(pair_pos(&t, 0, 1, 3), Some(-50));
        assert_eq!(pair_pos(&t, 0, 1, 9), Some(0));
        // 'A' does not kern before this one.
        assert_eq!(pair_pos(&t, 0, 1, 4), None);
        // Nothing kerns before 'A': the first glyph is not covered.
        assert_eq!(pair_pos(&t, 0, 2, 1), None);
    }

    #[test]
    fn pair_pos_format_2_indexes_the_class_grid() {
        // Classes: first glyphs 1,2 are class 1; second glyphs 5,6 are class 1
        // and 7 is class 2. Grid is 2x3.
        let class1 = {
            let mut t = vec![];
            t.extend(be16(1));
            t.extend(be16(1));
            t.extend(be16(2));
            t.extend(be16(1));
            t.extend(be16(1));
            t
        };
        let class2 = {
            let mut t = vec![];
            t.extend(be16(1));
            t.extend(be16(5));
            t.extend(be16(3));
            t.extend(be16(1));
            t.extend(be16(1));
            t.extend(be16(2));
            t
        };
        let grid_len = 2 * 3 * 2;
        let cd1_off = 16 + grid_len;
        let cd2_off = cd1_off + class1.len();
        let cov_off = cd2_off + class2.len();

        let mut t = vec![];
        t.extend(be16(2)); // posFormat
        t.extend(be16(u16::try_from(cov_off).unwrap()));
        t.extend(be16(X_ADVANCE));
        t.extend(be16(0));
        t.extend(be16(u16::try_from(cd1_off).unwrap()));
        t.extend(be16(u16::try_from(cd2_off).unwrap()));
        t.extend(be16(2)); // class1Count
        t.extend(be16(3)); // class2Count
        // Grid: row 0 is all zero, row 1 is [0, -30, -70].
        for v in [0_i16, 0, 0, 0, -30, -70] {
            t.extend(v.to_be_bytes());
        }
        assert_eq!(t.len(), cd1_off);
        t.extend(class1);
        t.extend(class2);
        t.extend(coverage1(&[1, 2]));

        assert_eq!(pair_pos(&t, 0, 1, 6), Some(-30));
        assert_eq!(pair_pos(&t, 0, 2, 7), Some(-70));
        // Class 0 on either axis is a real cell, and this font puts 0 there.
        assert_eq!(pair_pos(&t, 0, 1, 99), Some(0));
        // An uncovered first glyph is not kerned at all, even though its class
        // would index a valid row.
        assert_eq!(pair_pos(&t, 0, 3, 6), None);
    }

    /// A whole legacy `kern` table with one format-0 subtable.
    fn legacy_table(coverage: u16, pairs: &[(u16, u16, i16)]) -> Vec<u8> {
        let mut sub = vec![];
        sub.extend(be16(0)); // subtable version
        sub.extend(be16(u16::try_from(14 + pairs.len() * 6).unwrap()));
        sub.extend(be16(coverage));
        sub.extend(be16(u16::try_from(pairs.len()).unwrap()));
        sub.extend(be16(0)); // searchRange
        sub.extend(be16(0)); // entrySelector
        sub.extend(be16(0)); // rangeShift
        for (l, r, v) in pairs {
            sub.extend(be16(*l));
            sub.extend(be16(*r));
            sub.extend(v.to_be_bytes());
        }
        let mut t = vec![];
        t.extend(be16(0)); // table version
        t.extend(be16(1)); // nTables
        t.extend(sub);
        t
    }

    #[test]
    fn the_legacy_table_is_read_when_there_is_no_gpos() {
        let data = legacy_table(0x0001, &[(1, 2, -80), (1, 3, -50), (4, 2, -20)]);
        let k = Kerning::parse(&data, None, Some(span(0, data.len())), None).expect("kern parses");
        assert!(k.is_legacy());
        assert_eq!(k.pair(&data, 1, 2, &[]), -80);
        assert_eq!(k.pair(&data, 4, 2, &[]), -20);
        assert_eq!(k.pair(&data, 2, 1, &[]), 0);
    }

    #[test]
    fn a_vertical_or_minimum_subtable_is_left_alone() {
        // Vertical: the horizontal bit is clear.
        let vertical = legacy_table(0x0000, &[(1, 2, -80)]);
        assert!(Kerning::parse(&vertical, None, Some(span(0, vertical.len())), None).is_none());
        // Minimum: the values bound the spacing rather than adjusting it, so
        // applying them as adjustments would spread the text apart.
        let minimum = legacy_table(0x0003, &[(1, 2, -80)]);
        assert!(Kerning::parse(&minimum, None, Some(span(0, minimum.len())), None).is_none());
    }

    #[test]
    fn a_face_with_neither_table_has_no_kerning() {
        assert!(Kerning::parse(&[], None, None, None).is_none());
    }

    #[test]
    fn a_truncated_table_yields_no_kerning_rather_than_a_panic() {
        let full = legacy_table(0x0001, &[(1, 2, -80), (1, 3, -50)]);
        for cut in 0..full.len() {
            let data = &full[..cut];
            // Whatever survives parsing must still answer without reading past
            // the end: a font file is untrusted input.
            if let Some(k) = Kerning::parse(data, None, Some(span(0, data.len())), None) {
                let _ = k.pair(data, 1, 2, &[]);
                let _ = k.pair(data, 1, 3, &[]);
                let _ = k.pair(data, 7, 7, &[]);
            }
        }
    }

    #[test]
    fn gpos_is_preferred_over_the_legacy_table_when_both_are_present() {
        // The same pair, kerned differently by each table: whichever value
        // comes back names the source.
        let legacy = legacy_table(0x0001, &[(1, 2, -10)]);
        let mut data = gpos_table(-80);
        let legacy_at = data.len();
        data.extend(legacy);
        let k = Kerning::parse(
            &data,
            Some(span(0, legacy_at)),
            Some(span(legacy_at, data.len() - legacy_at)),
            None,
        )
        .expect("GPOS parses");
        assert!(!k.is_legacy());
        assert_eq!(k.pair(&data, 1, 2, &[]), -80);
    }

    /// A face that ships both keeps both. Which one a run should read is a
    /// question about the run's script — Leelawadee's `GPOS` kerns `thai` and
    /// nothing else, so its Latin text wants the legacy table even though the
    /// pair-at-a-time interface prefers `GPOS` face-wide — and a parse that
    /// threw one away could not answer it later.
    #[test]
    fn a_face_with_both_keeps_both() {
        let mut data = gpos_table_tagged(*b"kern", -80);
        let legacy_at = data.len();
        data.extend(legacy_table(0x0001, &[(1, 2, -10)]));
        let k = Kerning::parse(
            &data,
            Some(span(0, legacy_at)),
            Some(span(legacy_at, data.len() - legacy_at)),
            None,
        )
        .expect("both parse");
        assert!(!k.is_legacy());
        assert!(k.has_legacy());
        assert_eq!(k.pair(&data, 1, 2, &[]), -80);
        assert_eq!(k.legacy_pair(&data, 1, 2, &[]), -10);
        // The legacy table has no lookup flags and so reads across nothing.
        assert_eq!(k.legacy_pair(&data, 1, 2, &[3]), 0);
    }

    #[test]
    fn a_gpos_without_a_kern_feature_falls_through_to_the_legacy_table() {
        // A GPOS whose only feature is `liga` says nothing about kerning, and
        // must not be allowed to mask a legacy table that does.
        let mut data = gpos_table_tagged(*b"liga", -80);
        let legacy_at = data.len();
        data.extend(legacy_table(0x0001, &[(1, 2, -10)]));
        let k = Kerning::parse(
            &data,
            Some(span(0, legacy_at)),
            Some(span(legacy_at, data.len() - legacy_at)),
            None,
        )
        .expect("falls back to kern");
        assert!(k.is_legacy());
        assert_eq!(k.pair(&data, 1, 2, &[]), -10);
    }

    #[test]
    fn an_extension_lookup_is_followed_to_the_subtable_it_wraps() {
        let data = gpos_table_extension(-65);
        let k = Kerning::parse(&data, Some(span(0, data.len())), None, None).expect("extension parses");
        assert_eq!(k.pair(&data, 1, 2, &[]), -65);
    }

    /// A minimal GPOS with a `kern` feature over one `PairPos` lookup.
    fn gpos_table(value: i16) -> Vec<u8> {
        gpos_table_tagged(*b"kern", value)
    }

    fn gpos_table_tagged(tag: [u8; 4], value: i16) -> Vec<u8> {
        let (body, sub_at) = gpos_skeleton(tag, 10, LOOKUP_PAIR_POS, 0);
        finish_gpos(body, sub_at, value)
    }

    /// The same, with a `lookupFlag` on the lookup — which is what decides
    /// whether the pair may be read across an intervening glyph.
    fn gpos_table_flagged(value: i16, flag: u16) -> Vec<u8> {
        let (body, sub_at) = gpos_skeleton(*b"kern", 10, LOOKUP_PAIR_POS, flag);
        finish_gpos(body, sub_at, value)
    }

    /// The same, but with the `PairPos` behind an extension lookup.
    fn gpos_table_extension(value: i16) -> Vec<u8> {
        let (mut body, ext_at) = gpos_skeleton(*b"kern", 10, LOOKUP_EXTENSION, 0);
        // The extension subtable sits where the PairPos would have, and points
        // past the end of everything written so far.
        let sub_at = body.len();
        let delta = u32::try_from(sub_at - ext_at).unwrap();
        body[ext_at..ext_at + 2].copy_from_slice(&be16(1));
        body[ext_at + 2..ext_at + 4].copy_from_slice(&be16(LOOKUP_PAIR_POS));
        body[ext_at + 4..ext_at + 8].copy_from_slice(&delta.to_be_bytes());
        finish_gpos(body, sub_at, value)
    }

    /// GPOS header + a FeatureList with one feature + a LookupList with one
    /// lookup, returning the bytes and the offset of the lookup's first
    /// subtable (which the caller fills in).
    fn gpos_skeleton(
        tag: [u8; 4],
        sub_len: usize,
        lookup_type: u16,
        flag: u16,
    ) -> (Vec<u8>, usize) {
        let header = 10;
        let feature_list = header;
        let feature = feature_list + 2 + 6;
        let lookup_list = feature + 6;
        let lookup = lookup_list + 4;
        let sub = lookup + 8;

        let mut t = vec![];
        t.extend(be16(1)); // majorVersion
        t.extend(be16(0)); // minorVersion
        t.extend(be16(0)); // scriptListOffset — unread
        t.extend(be16(u16::try_from(feature_list).unwrap()));
        t.extend(be16(u16::try_from(lookup_list).unwrap()));
        // FeatureList.
        t.extend(be16(1));
        t.extend(tag);
        t.extend(be16(u16::try_from(feature - feature_list).unwrap()));
        // Feature.
        t.extend(be16(0)); // featureParams
        t.extend(be16(1)); // lookupIndexCount
        t.extend(be16(0)); // lookupListIndices[0]
        // LookupList.
        t.extend(be16(1));
        t.extend(be16(u16::try_from(lookup - lookup_list).unwrap()));
        // Lookup.
        t.extend(be16(lookup_type));
        t.extend(be16(flag)); // lookupFlag
        t.extend(be16(1)); // subTableCount
        t.extend(be16(u16::try_from(sub - lookup).unwrap()));
        assert_eq!(t.len(), sub);
        t.resize(sub + sub_len, 0);
        (t, sub)
    }

    /// Write a one-pair `PairPos` format 1 subtable at `at`.
    fn finish_gpos(mut body: Vec<u8>, at: usize, value: i16) -> Vec<u8> {
        body.truncate(at);
        let mut sub = vec![];
        let cov_off = 12 + 2 + 4;
        sub.extend(be16(1));
        sub.extend(be16(u16::try_from(cov_off).unwrap()));
        sub.extend(be16(X_ADVANCE));
        sub.extend(be16(0));
        sub.extend(be16(1)); // pairSetCount
        sub.extend(be16(12)); // pairSetOffset[0]
        sub.extend(be16(1)); // pairValueCount
        sub.extend(be16(2)); // secondGlyph
        sub.extend(value.to_be_bytes());
        sub.extend(coverage1(&[1]));
        body.extend(sub);
        body
    }

    /// A `GDEF` that calls every glyph in `marks` — which must be ascending —
    /// a mark, and says nothing about any other glyph.
    ///
    /// Without one of these a lookup's "ignore marks" flag names an empty set,
    /// because nothing has said which glyphs the marks are.
    fn gdef_marks(marks: &[u16]) -> Vec<u8> {
        let header = 12;
        let mut t = vec![];
        t.extend(be16(1)); // majorVersion
        t.extend(be16(0)); // minorVersion
        t.extend(be16(u16::try_from(header).unwrap())); // glyphClassDefOffset
        t.extend(be16(0)); // attachListOffset
        t.extend(be16(0)); // ligCaretListOffset
        t.extend(be16(0)); // markAttachClassDefOffset
        // ClassDefFormat 2, one single-glyph range per mark, all class 3.
        t.extend(be16(2));
        t.extend(be16(u16::try_from(marks.len()).unwrap()));
        for g in marks {
            t.extend(be16(*g));
            t.extend(be16(*g));
            t.extend(be16(3)); // MARK
        }
        t
    }

    /// A GPOS and a GDEF laid out the way a face presents them: two spans into
    /// one file, not two files.
    fn with_gdef(gpos: &[u8], gdef: &[u8]) -> (Vec<u8>, Kerning) {
        let mut data = gpos.to_vec();
        let at = data.len();
        data.extend_from_slice(gdef);
        let k = Kerning::parse(&data, Some(span(0, at)), None, Some(span(at, gdef.len())))
            .expect("a flagged kern lookup must still parse");
        (data, k)
    }

    /// `IgnoreMarks`, the flag that is on virtually every real `kern` lookup.
    const IGNORE_MARKS: u16 = 0x0008;

    #[test]
    fn a_kern_lookup_that_ignores_marks_reads_the_pair_across_one() {
        let (data, k) = with_gdef(&gpos_table_flagged(-80, IGNORE_MARKS), &gdef_marks(&[90]));
        // The pair itself is unchanged by the flag.
        assert_eq!(k.pair(&data, 1, 2, &[]), -80);
        // An accent between the two letters is stepped over, which is the whole
        // point: `A` and `V` keep kerning with a mark between them.
        assert_eq!(k.pair(&data, 1, 2, &[90]), -80);
        // Several of them, still all marks.
        assert_eq!(k.pair(&data, 1, 2, &[90, 90]), -80);
    }

    #[test]
    fn ignoring_marks_does_not_mean_ignoring_letters() {
        let (data, k) = with_gdef(&gpos_table_flagged(-80, IGNORE_MARKS), &gdef_marks(&[90]));
        // Glyph 7 is not a mark, so the pair is not adjacent and is not kerned.
        assert_eq!(k.pair(&data, 1, 2, &[7]), 0);
        // One mark and one letter is still a letter in the way.
        assert_eq!(k.pair(&data, 1, 2, &[90, 7]), 0);
    }

    #[test]
    fn an_unflagged_kern_lookup_declines_to_read_across_anything() {
        // The historically correct reading for a lookup that never asked to
        // skip: kerning applies to strictly adjacent glyphs.
        let (data, k) = with_gdef(&gpos_table_flagged(-80, 0), &gdef_marks(&[90]));
        assert_eq!(k.pair(&data, 1, 2, &[]), -80);
        assert_eq!(k.pair(&data, 1, 2, &[90]), 0);
    }

    #[test]
    fn a_flag_with_no_gdef_behind_it_hides_nothing() {
        // "Ignore marks" over a face that never classified its glyphs names an
        // empty set, so even glyph 90 stops the pair.
        let data = gpos_table_flagged(-80, IGNORE_MARKS);
        let k = Kerning::parse(&data, Some(span(0, data.len())), None, None)
            .expect("a flagged lookup parses without a GDEF");
        assert_eq!(k.pair(&data, 1, 2, &[]), -80);
        assert_eq!(k.pair(&data, 1, 2, &[90]), 0);
    }

    #[test]
    fn the_legacy_table_kerns_only_adjacent_glyphs() {
        // `kern` predates lookups and has no flags to honour, so there is no
        // basis for reading across anything.
        let data = legacy_table(0x0001, &[(1, 2, -10)]);
        let k = Kerning::parse(&data, None, Some(span(0, data.len())), None).expect("legacy parses");
        assert_eq!(k.pair(&data, 1, 2, &[]), -10);
        assert_eq!(k.pair(&data, 1, 2, &[90]), 0);
    }
}

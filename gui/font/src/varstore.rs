//! `ItemVariationStore` — the delta table `HVAR`, `MVAR` and `GDEF` share.
//!
//! [`gvar`] varies a glyph's *outline*. This varies everything else: how wide
//! the glyph is (`HVAR`), where the face's ascender and x-height sit (`MVAR`),
//! and where `GPOS` puts a mark relative to its base (`GDEF`). It is the last
//! of the four steps `TD-FONT-DOES-NOT-READ-VARIATION-STORES` lays out, and the
//! one that entry is named for.
//!
//! [`gvar`]: crate::gvar
//!
//! # One store, three ways in
//!
//! All three tables end in the same structure — a list of design-space
//! **regions**, plus **subtables** of delta rows over those regions. A delta is
//! named by an (outer, inner) pair: which subtable, and which row of it. What
//! differs between the three is only how a caller arrives at that pair:
//!
//! | Table | How the pair is reached |
//! |---|---|
//! | `HVAR` | a **glyph id**, mapped through a [`DeltaSetIndexMap`](IndexMap) |
//! | `MVAR` | a four-byte **value tag** (`hasc`, `xhgt`, …) in a sorted array |
//! | `GDEF` | handed over **directly**, as the first four bytes of a `GPOS` `VariationIndex` |
//!
//! So [`VarStore`] is written once and the three entry points are thin. That is
//! not merely tidy: a bug in the evaluator would otherwise have to be found
//! three times.
//!
//! # The three places the format invites a wrong reader
//!
//! Each of these was checked against the fonts installed on this machine with
//! `gui/font/tools/varstore_oracle.py`, and the answers are recorded because
//! two of the three turn out to be **unreachable from any real file here** —
//! which means a host-font sweep cannot see them and only the synthetic
//! fixture can.
//!
//! * **A null `HVAR` mapping offset does not mean "no variation".** It means
//!   the *implicit* map: outer 0, inner = glyph id. Two of this host's seven
//!   variable faces (Cascadia Code and Mono) take that path. A reader that
//!   treats it as an absent feature reports that no advance varies on a face
//!   where every advance does — except that on *those two* faces the store
//!   varies nothing anyway, so the mistake is invisible here. Covered by the
//!   fixture, not by the sweep.
//! * **A delta row is not a fixed-width array.** `wordDeltaCount`'s low 15 bits
//!   say how many of the row's *leading* deltas are stored wide; the rest are
//!   stored narrow. Bit 15 (`LONG_WORDS`) then doubles both, so "wide" is
//!   `i16` or `i32` and "narrow" is `i8` or `i16` depending on a bit ten bytes
//!   earlier. Four combinations, and three of them look plausible on a face
//!   that uses the fourth.
//! * **A region axis with a zero peak scores 1, not 0.** `peak == 0` means the
//!   region does not constrain that axis at all. Feeding it to the
//!   interpolation formula — which is what "distance from zero" would do —
//!   multiplies every delta by zero, and yields a store that varies nothing
//!   while looking like it read the table correctly.
//!
//! # Degenerate regions follow HarfBuzz, not the plain reading
//!
//! A region whose `start > peak`, whose `peak > end`, or which straddles zero
//! with a non-zero peak, is malformed. HarfBuzz's `VarRegionAxis::evaluate`
//! scores it **1** — i.e. ignores the constraint — rather than 0 or an
//! interpolation, and [`gvar`](crate::gvar)'s scalar already does the same.
//! This does too, for the reason design-decisions §448 gives for matching
//! HarfBuzz everywhere else: a deliberate divergence here would be
//! indistinguishable from a bug in any sweep that found it.
//!
//! This host has **0 degenerate region axes out of 199**, so nothing real
//! exercises the branch and the fixture has to.
//!
//! # References
//!
//! HarfBuzz `hb-ot-var-common.hh` (`VarRegionAxis::evaluate`,
//! `VarData::get_delta`, `DeltaSetIndexMap::map`) and `hb-ot-var-hvar-table.hh`.

use alloc::vec::Vec;

use crate::sfnt::{u16_at, u32_at};

/// `wordDeltaCount` bit 15: doubles the width of both halves of a delta row.
const LONG_WORDS: u16 = 0x8000;
/// The low bits of `wordDeltaCount`, i.e. how many leading deltas are wide.
const WORD_DELTA_COUNT_MASK: u16 = 0x7FFF;

/// Upper bound on a store's subtable count, so a malformed length cannot make
/// this allocate on an attacker's word. The largest on this host is 89.
const MAX_SUBTABLES: usize = 4096;
/// Upper bound on the region count, for the same reason. Largest here is 17.
const MAX_REGIONS: usize = 4096;
/// Upper bound on a store's axis count. `fvar` allows 2^16-1; real faces carry
/// one to four.
const MAX_AXES: usize = 64;

/// One axis's slice of one region: where it starts to apply, where it applies
/// in full, and where it stops.
///
/// All three are `F2Dot14`, kept as the raw `i16` the file stores so that the
/// comparisons below are exact.
#[derive(Clone, Copy, Debug)]
struct RegionAxis {
    start: i16,
    peak: i16,
    end: i16,
}

impl RegionAxis {
    /// How strongly this axis's constraint applies at `coord`, in `0.0..=1.0`.
    ///
    /// See the module doc for why a zero peak and a degenerate span both yield
    /// `1.0` rather than `0.0`.
    fn factor(self, coord: i16) -> f32 {
        if self.peak == 0 || coord == self.peak {
            return 1.0;
        }
        // Malformed: the constraint is unusable, so it is not applied. Matches
        // HarfBuzz rather than the plain reading of the spec; see module doc.
        if self.start > self.peak || self.peak > self.end {
            return 1.0;
        }
        if self.start < 0 && self.end > 0 {
            return 1.0;
        }
        if coord <= self.start || coord >= self.end {
            return 0.0;
        }
        // Widened to `f32` *before* subtracting: two F2Dot14 endpoints can be
        // 32768 apart, one past `i16::MAX`. `f32` is exact to 2^24, so this is
        // both total and exact. Same reasoning as `gvar::scalar`.
        if coord < self.peak {
            (f32::from(coord) - f32::from(self.start))
                / (f32::from(self.peak) - f32::from(self.start))
        } else {
            (f32::from(self.end) - f32::from(coord)) / (f32::from(self.end) - f32::from(self.peak))
        }
    }
}

/// One `ItemVariationData` subtable: the shape of its delta rows and where
/// they begin.
///
/// The rows themselves are *not* read here. A face's store can hold thousands
/// of them and a draw touches a handful, so they are read on demand from the
/// face's own bytes — the same arrangement [`gvar`](crate::gvar) uses for its
/// per-glyph data, and for the same reason.
#[derive(Clone, Debug)]
struct Subtable {
    /// Which region each column of a row refers to.
    region_indices: Vec<u16>,
    /// How many rows there are.
    item_count: u16,
    /// How many leading columns are stored in the wide form.
    word_count: usize,
    /// Byte width of a wide column: 4 with `LONG_WORDS`, else 2.
    wide: usize,
    /// Byte width of a narrow column: 2 with `LONG_WORDS`, else 1.
    narrow: usize,
    /// Absolute offset of row 0.
    rows_at: usize,
    /// Byte width of one whole row. May legitimately be zero — see
    /// [`Subtable::parse`].
    row_size: usize,
}

impl Subtable {
    fn parse(data: &[u8], at: usize) -> Option<Self> {
        let item_count = u16_at(data, at)?;
        let word_delta_count = u16_at(data, at.checked_add(2)?)?;
        let region_index_count = usize::from(u16_at(data, at.checked_add(4)?)?);
        let long_words = word_delta_count & LONG_WORDS != 0;
        let word_count = usize::from(word_delta_count & WORD_DELTA_COUNT_MASK);
        if word_count > region_index_count {
            return None;
        }

        let indices_at = at.checked_add(6)?;
        let mut region_indices = Vec::with_capacity(region_index_count);
        for i in 0..region_index_count {
            region_indices.push(u16_at(data, indices_at.checked_add(i.checked_mul(2)?)?)?);
        }
        let rows_at = indices_at.checked_add(region_index_count.checked_mul(2)?)?;

        let (wide, narrow) = if long_words { (4, 2) } else { (2, 1) };
        let row_size = word_count
            .checked_mul(wide)?
            .checked_add(region_index_count.checked_sub(word_count)?.checked_mul(narrow)?)?;

        // A subtable over *no* regions is legal and real — four of them on
        // this host, one with 1780 rows. Every row is empty, so every delta is
        // zero. It must not be read as "one byte per row", and the bounds
        // check below must not be skipped on the grounds that the product is
        // zero; both would be arithmetic on a row that does not exist.
        let end = rows_at.checked_add(usize::from(item_count).checked_mul(row_size)?)?;
        if end > data.len() {
            return None;
        }

        Some(Self {
            region_indices,
            item_count,
            word_count,
            wide,
            narrow,
            rows_at,
            row_size,
        })
    }

    /// Column `k` of row `inner`, in the width this subtable stores it at.
    fn column(&self, data: &[u8], inner: u16, k: usize) -> Option<i32> {
        if inner >= self.item_count {
            return None;
        }
        let row = self
            .rows_at
            .checked_add(usize::from(inner).checked_mul(self.row_size)?)?;
        if k < self.word_count {
            let at = row.checked_add(k.checked_mul(self.wide)?)?;
            if self.wide == 4 {
                Some(i32_at(data, at)?)
            } else {
                Some(i32::from(i16_at(data, at)?))
            }
        } else {
            let at = row
                .checked_add(self.word_count.checked_mul(self.wide)?)?
                .checked_add(k.checked_sub(self.word_count)?.checked_mul(self.narrow)?)?;
            if self.narrow == 2 {
                Some(i32::from(i16_at(data, at)?))
            } else {
                Some(i32::from(i8_at(data, at)?))
            }
        }
    }
}

/// A parsed `ItemVariationStore`.
#[derive(Clone, Debug)]
pub(crate) struct VarStore {
    /// How many axes each region names. Checked against `fvar`'s count by the
    /// caller, because a mismatch pairs axis *k*'s coordinate with axis *k*'s
    /// region on a table that meant something else by *k*.
    axis_count: usize,
    /// Regions, flattened: region `r`'s axis `a` is at `r * axis_count + a`.
    /// One `Vec` rather than a `Vec<Vec<_>>` because a region is fixed-width
    /// and the inner allocations would outnumber the data.
    regions: Vec<RegionAxis>,
    subtables: Vec<Subtable>,
}

impl VarStore {
    /// Parse the store at `at`, requiring it to name `axis_count` axes.
    ///
    /// `None` when the store is malformed or disagrees with `fvar` about the
    /// axis count — both of which mean "vary nothing", which is the same
    /// answer as having no store at all and is why they are not distinguished.
    pub(crate) fn parse(data: &[u8], at: usize, axis_count: usize) -> Option<Self> {
        if axis_count == 0 || axis_count > MAX_AXES {
            return None;
        }
        // Format 1 is the only one defined. A future format 2 would not be
        // readable by this code, so refusing it is honest.
        if u16_at(data, at)? != 1 {
            return None;
        }
        let regions = Self::parse_regions(
            data,
            at.checked_add(usize::try_from(u32_at(data, at.checked_add(2)?)?).ok()?)?,
            axis_count,
        )?;

        let count = usize::from(u16_at(data, at.checked_add(6)?)?);
        if count > MAX_SUBTABLES {
            return None;
        }
        let mut subtables = Vec::with_capacity(count);
        for i in 0..count {
            let rel = u32_at(data, at.checked_add(8)?.checked_add(i.checked_mul(4)?)?)?;
            let sub = if rel == 0 {
                None
            } else {
                at.checked_add(usize::try_from(rel).ok()?)
                    .and_then(|off| Subtable::parse(data, off))
            };
            // A subtable that fails to parse becomes an empty one rather than
            // killing the store: the others are still readable, and a missing
            // delta is a glyph at its default width — a far smaller error than
            // a face that stops varying.
            subtables.push(sub.unwrap_or(Subtable {
                region_indices: Vec::new(),
                item_count: 0,
                word_count: 0,
                wide: 2,
                narrow: 1,
                rows_at: 0,
                row_size: 0,
            }));
        }
        Some(Self {
            axis_count,
            regions,
            subtables,
        })
    }

    fn parse_regions(data: &[u8], at: usize, axis_count: usize) -> Option<Vec<RegionAxis>> {
        if usize::from(u16_at(data, at)?) != axis_count {
            return None;
        }
        let region_count = usize::from(u16_at(data, at.checked_add(2)?)?);
        if region_count > MAX_REGIONS {
            return None;
        }
        let mut regions = Vec::with_capacity(region_count.checked_mul(axis_count)?);
        for r in 0..region_count {
            let base = at
                .checked_add(4)?
                .checked_add(r.checked_mul(axis_count)?.checked_mul(6)?)?;
            for a in 0..axis_count {
                let axis = base.checked_add(a.checked_mul(6)?)?;
                regions.push(RegionAxis {
                    start: i16_at(data, axis)?,
                    peak: i16_at(data, axis.checked_add(2)?)?,
                    end: i16_at(data, axis.checked_add(4)?)?,
                });
            }
        }
        Some(regions)
    }

    /// How strongly region `index` applies at `coords`, in `0.0..=1.0`.
    ///
    /// An axis the caller did not supply a coordinate for reads as 0 — the
    /// default instance — rather than aborting: a caller that has set two of
    /// three axes means the third to be at its default.
    fn scalar(&self, index: u16, coords: &[i16]) -> f32 {
        let Some(start) = usize::from(index).checked_mul(self.axis_count) else {
            return 0.0;
        };
        let Some(region) = self.regions.get(start..start.checked_add(self.axis_count).unwrap_or(0))
        else {
            return 0.0;
        };
        let mut scale = 1.0f32;
        for (a, axis) in region.iter().enumerate() {
            scale *= axis.factor(coords.get(a).copied().unwrap_or(0));
            if scale == 0.0 {
                // Nothing later can bring it back, and the remaining axes are
                // pure cost.
                return 0.0;
            }
        }
        scale
    }

    /// The delta at row (`outer`, `inner`) evaluated at `coords`.
    ///
    /// `None` when the pair names no row. That is distinct from `Some(0.0)`:
    /// an unmapped pair is a face asking for something that is not there,
    /// while a zero delta is a face saying "this does not move". Callers
    /// treat both as no correction, but only one of them is a font bug.
    ///
    /// Returned as `f32` and *not* rounded, because a consumer that adds two
    /// stores' contributions must round once at the end rather than twice on
    /// the way.
    pub(crate) fn delta(&self, data: &[u8], outer: u16, inner: u16, coords: &[i16]) -> Option<f32> {
        let sub = self.subtables.get(usize::from(outer))?;
        if inner >= sub.item_count {
            return None;
        }
        let mut total = 0.0f32;
        for (k, &region_index) in sub.region_indices.iter().enumerate() {
            let s = self.scalar(region_index, coords);
            if s == 0.0 {
                continue;
            }
            // A column that fails to read is a truncated row. Dropping just
            // that column keeps the rest of the delta, which is the difference
            // between a slightly wrong width and none.
            if let Some(v) = sub.column(data, inner, k) {
                // `mul_add` rather than `*` then `+`: one rounding instead of
                // two, and it is what HarfBuzz's accumulator does.
                total = s.mul_add(
                    {
                        #[allow(
                            clippy::cast_precision_loss,
                            reason = "deltas are font units; f32 is exact to 2^24 and a \
                                      delta beyond that is a broken face either way"
                        )]
                        {
                            v as f32
                        }
                    },
                    total,
                );
            }
        }
        Some(total)
    }
}

/// A `DeltaSetIndexMap`: glyph id (or other ordinal) to an (outer, inner) pair.
#[derive(Clone, Debug)]
pub(crate) struct IndexMap {
    /// Absolute offset of entry 0.
    entries_at: usize,
    count: u32,
    /// Bytes per entry, 1..=4.
    entry_size: usize,
    /// How many low bits of an entry are the inner index.
    inner_bits: u32,
}

impl IndexMap {
    /// Parse the map at `at`.
    ///
    /// Format 0 counts entries in a `u16` and format 1 in a `u32`; both then
    /// pack each entry into `entrySize` bytes, big-endian, split into an outer
    /// and an inner index at a bit position the same byte declares.
    pub(crate) fn parse(data: &[u8], at: usize) -> Option<Self> {
        let format = *data.get(at)?;
        let entry_format = u32::from(*data.get(at.checked_add(1)?)?);
        let (count, entries_at) = match format {
            0 => (
                u32::from(u16_at(data, at.checked_add(2)?)?),
                at.checked_add(4)?,
            ),
            1 => (u32_at(data, at.checked_add(2)?)?, at.checked_add(6)?),
            _ => return None,
        };
        // Bits 6 and 7 are reserved and must be zero. A face that sets one is
        // using a format this does not know, and guessing at it would produce
        // indices that point at real rows holding unrelated numbers.
        if entry_format & 0xC0 != 0 {
            return None;
        }
        let inner_bits = (entry_format & 0x0F).checked_add(1)?;
        let entry_size = usize::try_from(((entry_format & 0x30) >> 4).checked_add(1)?).ok()?;
        let end = entries_at
            .checked_add(usize::try_from(count).ok()?.checked_mul(entry_size)?)?;
        if end > data.len() {
            return None;
        }
        Some(Self {
            entries_at,
            count,
            entry_size,
            inner_bits,
        })
    }

    /// The (outer, inner) pair for `index`.
    ///
    /// An index past the end takes the **last** entry, which is how a face
    /// compresses a long tail of glyphs that share one delta row. Returning
    /// `None` there would silently stop varying the back half of a font.
    pub(crate) fn get(&self, data: &[u8], index: u32) -> Option<(u16, u16)> {
        if self.count == 0 {
            return None;
        }
        let clamped = usize::try_from(index.min(self.count.checked_sub(1)?)).ok()?;
        let at = self.entries_at.checked_add(clamped.checked_mul(self.entry_size)?)?;
        let bytes = data.get(at..at.checked_add(self.entry_size)?)?;
        let mut raw = 0u32;
        for &b in bytes {
            raw = raw.checked_mul(256)?.checked_add(u32::from(b))?;
        }
        let inner_mask = 1u32.checked_shl(self.inner_bits)?.checked_sub(1)?;
        Some((
            u16::try_from(raw.checked_shr(self.inner_bits)?).ok()?,
            u16::try_from(raw & inner_mask).ok()?,
        ))
    }
}

/// `HVAR`: how a face's advance widths change with the axes.
#[derive(Clone, Debug)]
pub(crate) struct Hvar {
    store: VarStore,
    /// `None` is the *implicit* map — outer 0, inner = glyph id — and not
    /// "no variation". See the module doc.
    advances: Option<IndexMap>,
}

impl Hvar {
    /// Parse the `HVAR` at `at`, requiring `axis_count` axes.
    pub(crate) fn parse(data: &[u8], at: usize, axis_count: usize) -> Option<Self> {
        // major/minor version, then three Offset32s; the last two (left and
        // right side bearing maps) are not read, because side bearings are
        // derived from the outline this crate varies through `gvar`.
        if u16_at(data, at)? != 1 {
            return None;
        }
        let store_rel = u32_at(data, at.checked_add(4)?)?;
        let store = VarStore::parse(
            data,
            at.checked_add(usize::try_from(store_rel).ok()?)?,
            axis_count,
        )?;
        let map_rel = u32_at(data, at.checked_add(8)?)?;
        let advances = if map_rel == 0 {
            None
        } else {
            at.checked_add(usize::try_from(map_rel).ok()?)
                .and_then(|off| IndexMap::parse(data, off))
        };
        Some(Self { store, advances })
    }

    /// How much wider `gid` is at `coords` than in the default instance, in
    /// font units, already rounded.
    ///
    /// Zero when the glyph has no delta, which is the common case for the
    /// unmapped tail of a font and is not an error.
    pub(crate) fn advance_delta(&self, data: &[u8], gid: u16, coords: &[i16]) -> i16 {
        let (outer, inner) = match self.advances.as_ref() {
            Some(map) => match map.get(data, u32::from(gid)) {
                Some(pair) => pair,
                None => return 0,
            },
            None => (0, gid),
        };
        match self.store.delta(data, outer, inner, coords) {
            // HarfBuzz rounds the accumulated float once, here, rather than
            // per region.
            Some(d) => round_to_i16(d),
            None => 0,
        }
    }
}

/// `MVAR`: how a face's global metrics change with the axes.
///
/// The records are a sorted array of (tag, outer, inner), so a lookup is a
/// binary search on the tag. Sorted order is required by the format; this
/// searches rather than scans because a face may carry several dozen records
/// and the metrics are re-derived on every size change.
#[derive(Clone, Debug)]
pub(crate) struct Mvar {
    store: VarStore,
    /// Absolute offset of record 0.
    records_at: usize,
    record_size: usize,
    record_count: usize,
}

impl Mvar {
    /// Parse the `MVAR` at `at`, requiring `axis_count` axes.
    pub(crate) fn parse(data: &[u8], at: usize, axis_count: usize) -> Option<Self> {
        if u16_at(data, at)? != 1 {
            return None;
        }
        // +4 is a reserved u16. Note the store offset here is an **Offset16**
        // at +10, not the Offset32 `HVAR` uses: the two tables do not agree,
        // and reading four bytes here lands in the record array.
        let record_size = usize::from(u16_at(data, at.checked_add(6)?)?);
        let record_count = usize::from(u16_at(data, at.checked_add(8)?)?);
        let store_rel = u16_at(data, at.checked_add(10)?)?;
        if store_rel == 0 {
            return None;
        }
        let store = VarStore::parse(data, at.checked_add(usize::from(store_rel))?, axis_count)?;
        // A record is a 4-byte tag plus two u16s. The field is a *size* rather
        // than a constant so that a later version can extend it, so a smaller
        // one is malformed and a larger one is read at its stride.
        if record_size < 8 {
            return None;
        }
        let records_at = at.checked_add(12)?;
        if records_at.checked_add(record_count.checked_mul(record_size)?)? > data.len() {
            return None;
        }
        Some(Self {
            store,
            records_at,
            record_size,
            record_count,
        })
    }

    /// The correction `tag` names at `coords`, in font units, already rounded.
    ///
    /// Zero when the face does not carry that metric, which is normal: a face
    /// varies the handful of metrics its designer cared about.
    pub(crate) fn metric_delta(&self, data: &[u8], tag: [u8; 4], coords: &[i16]) -> i16 {
        let Some((outer, inner)) = self.find(data, tag) else {
            return 0;
        };
        self.store
            .delta(data, outer, inner, coords)
            .map_or(0, round_to_i16)
    }

    fn find(&self, data: &[u8], tag: [u8; 4]) -> Option<(u16, u16)> {
        let (mut lo, mut hi) = (0usize, self.record_count);
        while lo < hi {
            // `lo + (hi - lo) / 2` rather than `(lo + hi) / 2`: the latter can
            // overflow, and `arithmetic_side_effects` is denied here anyway.
            let mid = lo.checked_add(hi.checked_sub(lo)?.checked_div(2)?)?;
            let at = self.records_at.checked_add(mid.checked_mul(self.record_size)?)?;
            let found: [u8; 4] = data.get(at..at.checked_add(4)?)?.try_into().ok()?;
            match found.cmp(&tag) {
                core::cmp::Ordering::Less => lo = mid.checked_add(1)?,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => {
                    return Some((
                        u16_at(data, at.checked_add(4)?)?,
                        u16_at(data, at.checked_add(6)?)?,
                    ));
                }
            }
        }
        None
    }
}

/// Round an accumulated delta to the font unit a caller adds to a metric.
///
/// Saturating rather than wrapping: a correction larger than an `i16` is a
/// broken face, and clamping keeps the glyph on the page.
fn round_to_i16(v: f32) -> i16 {
    if !v.is_finite() {
        return 0;
    }
    let r = v.round();
    if r <= f32::from(i16::MIN) {
        i16::MIN
    } else if r >= f32::from(i16::MAX) {
        i16::MAX
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "range-checked against i16's bounds immediately above"
        )]
        {
            r as i16
        }
    }
}

/// A big-endian `i16` at `off`, or `None` past the end.
fn i16_at(d: &[u8], off: usize) -> Option<i16> {
    #[allow(
        clippy::cast_possible_wrap,
        reason = "reinterpreting the same 16 bits as signed, which is the point"
    )]
    Some(u16_at(d, off)? as i16)
}

/// A big-endian `i32` at `off`, or `None` past the end.
fn i32_at(d: &[u8], off: usize) -> Option<i32> {
    #[allow(
        clippy::cast_possible_wrap,
        reason = "reinterpreting the same 32 bits as signed, which is the point"
    )]
    Some(u32_at(d, off)? as i32)
}

/// An `i8` at `off`, or `None` past the end.
fn i8_at(d: &[u8], off: usize) -> Option<i8> {
    #[allow(
        clippy::cast_possible_wrap,
        reason = "reinterpreting the same 8 bits as signed, which is the point"
    )]
    Some(*d.get(off)? as i8)
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did
    // it — that is the diagnosis. The defensive lints exist to keep panics out
    // of code that runs on a user's data, which this is not.
    #![allow(
        clippy::float_cmp,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;

    /// `F2Dot14` for 1.0 — the far end of an axis, in the units a store's
    /// regions and a caller's coordinates are both expressed in.
    const ONE: i16 = 16384;
    /// `F2Dot14` for 0.5, i.e. half way from the default to the far end.
    const HALF: i16 = 8192;

    // --- building a store to read back ---
    //
    // Every fixture below is assembled from the spec's field order rather than
    // from this module's parser, so a test that passes is two independent
    // readings agreeing. A helper that called `VarStore` to lay out its own
    // input would only prove the parser is self-consistent.

    fn push16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_be_bytes());
    }

    fn push32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_be_bytes());
    }

    /// One `ItemVariationData` subtable, described the way a font designer
    /// would: which regions its columns refer to, how many of them are stored
    /// wide, and the rows themselves.
    struct Sub {
        regions: Vec<u16>,
        word_count: usize,
        long: bool,
        rows: Vec<Vec<i32>>,
    }

    impl Sub {
        /// A subtable whose columns are all narrow, over regions `0..n`.
        fn narrow(region_count: u16, rows: &[&[i32]]) -> Self {
            Self {
                regions: (0..region_count).collect(),
                word_count: 0,
                long: false,
                rows: rows.iter().map(|r| r.to_vec()).collect(),
            }
        }

        fn bytes(&self) -> Vec<u8> {
            let mut out = Vec::new();
            push16(&mut out, u16::try_from(self.rows.len()).unwrap());
            let flag = if self.long { LONG_WORDS } else { 0 };
            push16(&mut out, u16::try_from(self.word_count).unwrap() | flag);
            push16(&mut out, u16::try_from(self.regions.len()).unwrap());
            for &r in &self.regions {
                push16(&mut out, r);
            }
            for row in &self.rows {
                assert_eq!(row.len(), self.regions.len(), "row width must match");
                for (k, &v) in row.iter().enumerate() {
                    if k < self.word_count {
                        if self.long {
                            out.extend_from_slice(&v.to_be_bytes());
                        } else {
                            out.extend_from_slice(&i16::try_from(v).unwrap().to_be_bytes());
                        }
                    } else if self.long {
                        out.extend_from_slice(&i16::try_from(v).unwrap().to_be_bytes());
                    } else {
                        out.extend_from_slice(&i8::try_from(v).unwrap().to_be_bytes());
                    }
                }
            }
            out
        }
    }

    /// A whole `ItemVariationStore`: header, region list, then the subtables.
    fn store_bytes(axis_count: usize, regions: &[&[(i16, i16, i16)]], subs: &[Sub]) -> Vec<u8> {
        let mut region_list = Vec::new();
        push16(&mut region_list, u16::try_from(axis_count).unwrap());
        push16(&mut region_list, u16::try_from(regions.len()).unwrap());
        for region in regions {
            assert_eq!(region.len(), axis_count, "a region names every axis");
            for &(start, peak, end) in *region {
                region_list.extend_from_slice(&start.to_be_bytes());
                region_list.extend_from_slice(&peak.to_be_bytes());
                region_list.extend_from_slice(&end.to_be_bytes());
            }
        }

        let header = 8 + 4 * subs.len();
        let mut out = Vec::new();
        push16(&mut out, 1);
        push32(&mut out, u32::try_from(header).unwrap());
        push16(&mut out, u16::try_from(subs.len()).unwrap());
        let mut cursor = header + region_list.len();
        let bodies: Vec<Vec<u8>> = subs.iter().map(Sub::bytes).collect();
        for body in &bodies {
            push32(&mut out, u32::try_from(cursor).unwrap());
            cursor += body.len();
        }
        out.extend_from_slice(&region_list);
        for body in &bodies {
            out.extend_from_slice(body);
        }
        out
    }

    /// A one-axis store with a single region peaking at the far end and a
    /// single subtable of one-column rows.
    fn simple(rows: &[i32]) -> Vec<u8> {
        let rows: Vec<Vec<i32>> = rows.iter().map(|&v| alloc::vec![v]).collect();
        let refs: Vec<&[i32]> = rows.iter().map(Vec::as_slice).collect();
        store_bytes(1, &[&[(0, ONE, ONE)]], &[Sub::narrow(1, &refs)])
    }

    /// A `DeltaSetIndexMap` in either format, packing each pair at
    /// `inner_bits` into `entry_size` bytes.
    fn index_map_bytes(
        format: u8,
        inner_bits: u32,
        entry_size: usize,
        entries: &[(u16, u16)],
    ) -> Vec<u8> {
        let size_bits = (u32::try_from(entry_size).unwrap() - 1) << 4;
        let mut out = alloc::vec![format, u8::try_from(size_bits | (inner_bits - 1)).unwrap()];
        if format == 0 {
            push16(&mut out, u16::try_from(entries.len()).unwrap());
        } else {
            push32(&mut out, u32::try_from(entries.len()).unwrap());
        }
        for &(outer, inner) in entries {
            let raw = (u32::from(outer) << inner_bits) | u32::from(inner);
            out.extend_from_slice(&raw.to_be_bytes()[4 - entry_size..]);
        }
        out
    }

    /// An `HVAR` table: 20-byte header, then the store, then the map.
    fn hvar_bytes(store: &[u8], map: Option<&[u8]>) -> Vec<u8> {
        let mut out = Vec::new();
        push16(&mut out, 1);
        push16(&mut out, 0);
        push32(&mut out, 20);
        let map_at = u32::try_from(20 + store.len()).unwrap();
        push32(&mut out, if map.is_some() { map_at } else { 0 });
        push32(&mut out, 0);
        push32(&mut out, 0);
        out.extend_from_slice(store);
        if let Some(m) = map {
            out.extend_from_slice(m);
        }
        out
    }

    /// An `MVAR` table: 12-byte header, then the records, then the store.
    /// `record_size` is a parameter because the field is a stride, not a
    /// constant.
    fn mvar_sized(store: &[u8], records: &[([u8; 4], u16, u16)], record_size: usize) -> Vec<u8> {
        let mut out = Vec::new();
        push16(&mut out, 1);
        push16(&mut out, 0);
        push16(&mut out, 0);
        push16(&mut out, u16::try_from(record_size).unwrap());
        push16(&mut out, u16::try_from(records.len()).unwrap());
        push16(
            &mut out,
            u16::try_from(12 + record_size * records.len()).unwrap(),
        );
        for &(tag, outer, inner) in records {
            out.extend_from_slice(&tag);
            push16(&mut out, outer);
            push16(&mut out, inner);
            // `saturating_sub` because the undersized-record fixture below
            // deliberately declares a stride narrower than one record.
            out.resize(out.len() + record_size.saturating_sub(8), 0);
        }
        out.extend_from_slice(store);
        out
    }

    fn mvar_bytes(store: &[u8], records: &[([u8; 4], u16, u16)]) -> Vec<u8> {
        mvar_sized(store, records, 8)
    }

    /// The scalar of region 0 of a one-subtable store, at `coords`.
    fn scalar_of(regions: &[&[(i16, i16, i16)]], coords: &[i16]) -> f32 {
        let axis_count = regions[0].len();
        let bytes = store_bytes(axis_count, regions, &[]);
        VarStore::parse(&bytes, 0, axis_count)
            .unwrap()
            .scalar(0, coords)
    }

    // --- the scalar: how much of a region applies ---

    #[test]
    fn a_region_applies_in_full_at_its_peak() {
        assert_eq!(scalar_of(&[&[(0, ONE, ONE)]], &[ONE]), 1.0);
    }

    #[test]
    fn a_region_scales_linearly_between_start_and_peak() {
        assert_eq!(scalar_of(&[&[(0, ONE, ONE)]], &[HALF]), 0.5);
    }

    #[test]
    fn a_region_contributes_nothing_at_the_default_instance() {
        // Every store must read exactly zero here, because the default
        // instance's metrics *are* the stored metrics.
        assert_eq!(scalar_of(&[&[(0, ONE, ONE)]], &[0]), 0.0);
    }

    #[test]
    fn an_axis_a_region_does_not_name_scores_one_rather_than_zero() {
        // A zero peak means "unconstrained on this axis", not "at the default
        // on this axis". Feeding it to the interpolation would zero the whole
        // product and yield a store that varies nothing while looking correct.
        let regions: &[&[(i16, i16, i16)]] = &[&[(0, ONE, ONE), (0, 0, 0)]];
        assert_eq!(scalar_of(regions, &[ONE, ONE]), 1.0);
    }

    #[test]
    fn two_named_axes_multiply_their_factors() {
        let regions: &[&[(i16, i16, i16)]] = &[&[(0, ONE, ONE), (0, ONE, ONE)]];
        assert_eq!(scalar_of(regions, &[HALF, HALF]), 0.25);
    }

    #[test]
    fn an_axis_the_caller_left_out_reads_as_its_default() {
        // Asked for one coordinate on a two-axis store: the second axis is at
        // 0, so a region that peaks on it contributes nothing.
        let regions: &[&[(i16, i16, i16)]] = &[&[(0, ONE, ONE), (0, ONE, ONE)]];
        assert_eq!(scalar_of(regions, &[ONE]), 0.0);
    }

    #[test]
    fn a_degenerate_region_is_ignored_rather_than_scored_zero() {
        // start > peak is malformed. HarfBuzz drops the constraint; the plain
        // reading of the formula would produce a negative or zero factor. No
        // font installed on this machine has one (0 of 199 region axes), so
        // this fixture is the only thing that reaches the branch.
        assert_eq!(scalar_of(&[&[(HALF, 4096, ONE)]], &[ONE]), 1.0);
        // …and so is peak > end.
        assert_eq!(scalar_of(&[&[(0, ONE, HALF)]], &[ONE]), 1.0);
    }

    #[test]
    fn a_region_straddling_the_default_is_ignored_too() {
        // A span from -1 to +1 with a non-zero peak cannot be interpolated
        // one-sidedly, so the constraint is dropped rather than guessed at.
        assert_eq!(scalar_of(&[&[(-ONE, HALF, ONE)]], &[0]), 1.0);
    }

    // --- how a delta row is packed ---

    #[test]
    fn a_row_mixes_wide_leading_columns_with_narrow_trailing_ones() {
        // 300 does not fit in the narrow (i8) form, which is exactly why
        // wordDeltaCount exists.
        let sub = Sub {
            regions: alloc::vec![0, 1],
            word_count: 1,
            long: false,
            rows: alloc::vec![alloc::vec![300, -5]],
        };
        let bytes = store_bytes(1, &[&[(0, ONE, ONE)], &[(0, ONE, ONE)]], &[sub]);
        let store = VarStore::parse(&bytes, 0, 1).unwrap();
        assert_eq!(store.delta(&bytes, 0, 0, &[ONE]), Some(295.0));
    }

    #[test]
    fn long_words_doubles_both_column_widths() {
        // Bit 15 of a field ten bytes earlier decides whether "wide" is i16 or
        // i32 and "narrow" is i8 or i16. Three of the four combinations look
        // plausible on a face that uses the fourth.
        let sub = Sub {
            regions: alloc::vec![0, 1],
            word_count: 1,
            long: true,
            rows: alloc::vec![alloc::vec![100_000, 3000]],
        };
        let bytes = store_bytes(1, &[&[(0, ONE, ONE)], &[(0, ONE, ONE)]], &[sub]);
        let store = VarStore::parse(&bytes, 0, 1).unwrap();
        assert_eq!(store.delta(&bytes, 0, 0, &[ONE]), Some(103_000.0));
    }

    #[test]
    fn a_narrow_column_is_sign_extended() {
        // 0xFF is -1, not 255. A reader that widened without sign-extending
        // would move a glyph 256 units the wrong way.
        let bytes = simple(&[-1]);
        let store = VarStore::parse(&bytes, 0, 1).unwrap();
        assert_eq!(store.delta(&bytes, 0, 0, &[ONE]), Some(-1.0));
    }

    #[test]
    fn a_subtable_over_no_regions_yields_a_zero_delta() {
        // Real: four such subtables on this host, one with 1780 rows. Its rows
        // are zero bytes wide, so the row stride is 0 — which must not be
        // rounded up to 1, and must not skip the bounds check on the grounds
        // that the product is zero.
        let sub = Sub {
            regions: Vec::new(),
            word_count: 0,
            long: false,
            rows: alloc::vec![Vec::new(), Vec::new(), Vec::new()],
        };
        let bytes = store_bytes(1, &[&[(0, ONE, ONE)]], &[sub]);
        let store = VarStore::parse(&bytes, 0, 1).unwrap();
        assert_eq!(store.delta(&bytes, 0, 2, &[ONE]), Some(0.0));
        assert_eq!(store.delta(&bytes, 0, 3, &[ONE]), None);
    }

    // --- refusing what cannot be read ---

    #[test]
    fn a_pair_naming_no_row_is_none_rather_than_zero() {
        let bytes = simple(&[7, 8]);
        let store = VarStore::parse(&bytes, 0, 1).unwrap();
        assert_eq!(store.delta(&bytes, 0, 1, &[ONE]), Some(8.0));
        assert_eq!(store.delta(&bytes, 0, 2, &[ONE]), None);
        assert_eq!(store.delta(&bytes, 1, 0, &[ONE]), None);
    }

    #[test]
    fn a_store_disagreeing_with_fvar_about_the_axis_count_is_refused() {
        // Pairing axis k's coordinate with axis k's region on a table that
        // meant something else by k is worse than not varying at all.
        let bytes = simple(&[5]);
        assert!(VarStore::parse(&bytes, 0, 2).is_none());
        assert!(VarStore::parse(&bytes, 0, 0).is_none());
    }

    #[test]
    fn a_truncated_region_list_is_refused_rather_than_read_short() {
        let bytes = simple(&[5]);
        for cut in 8..bytes.len() - 1 {
            // Every prefix either parses to something coherent or is refused;
            // none may read past its own end.
            let _ = VarStore::parse(&bytes[..cut], 0, 1);
        }
        assert!(VarStore::parse(&bytes[..10], 0, 1).is_none());
    }

    #[test]
    fn a_subtable_that_does_not_fit_degrades_without_losing_the_others() {
        // One unreadable subtable must not stop the face varying: a missing
        // delta is a glyph at its default width, a dead store is a dead font.
        let one = Sub::narrow(1, &[&[3]]);
        let two = Sub::narrow(1, &[&[4]]);
        let bytes = store_bytes(1, &[&[(0, ONE, ONE)]], &[one, two]);
        let short = &bytes[..bytes.len() - 1];
        let store = VarStore::parse(short, 0, 1).unwrap();
        assert_eq!(store.delta(short, 0, 0, &[ONE]), Some(3.0));
        assert_eq!(store.delta(short, 1, 0, &[ONE]), None);
    }

    #[test]
    fn a_store_in_an_unknown_format_is_refused() {
        let mut bytes = simple(&[5]);
        bytes[1] = 2;
        assert!(VarStore::parse(&bytes, 0, 1).is_none());
    }

    // --- the index map ---

    #[test]
    fn an_index_map_splits_each_entry_at_the_declared_bit() {
        let bytes = index_map_bytes(0, 8, 2, &[(0, 1), (2, 3)]);
        let map = IndexMap::parse(&bytes, 0).unwrap();
        assert_eq!(map.get(&bytes, 0), Some((0, 1)));
        assert_eq!(map.get(&bytes, 1), Some((2, 3)));
    }

    #[test]
    fn an_index_map_honours_an_unusual_inner_bit_count() {
        // Real files here use 7, 8 and 9 inner bits; 9 is the one that proves
        // the split is read from the file rather than assumed to be a byte.
        let bytes = index_map_bytes(0, 9, 2, &[(1, 300)]);
        let map = IndexMap::parse(&bytes, 0).unwrap();
        assert_eq!(map.get(&bytes, 0), Some((1, 300)));
    }

    #[test]
    fn an_index_past_the_end_takes_the_last_entry() {
        // This is how a face compresses a long tail of glyphs that share one
        // row. Returning None here would stop the back half of a font varying.
        let bytes = index_map_bytes(0, 8, 2, &[(0, 1), (0, 9)]);
        let map = IndexMap::parse(&bytes, 0).unwrap();
        assert_eq!(map.get(&bytes, 2), Some((0, 9)));
        assert_eq!(map.get(&bytes, 65_535), Some((0, 9)));
    }

    #[test]
    fn an_index_map_with_reserved_bits_set_is_refused() {
        // Bits 6-7 mean a format this does not know. Guessing would produce
        // indices that hit real rows holding unrelated numbers.
        let mut bytes = index_map_bytes(0, 8, 2, &[(0, 1)]);
        bytes[1] |= 0x40;
        assert!(IndexMap::parse(&bytes, 0).is_none());
    }

    #[test]
    fn a_one_byte_entry_map_is_read_at_its_own_stride() {
        let bytes = index_map_bytes(0, 4, 1, &[(0, 1), (1, 2), (2, 3)]);
        let map = IndexMap::parse(&bytes, 0).unwrap();
        assert_eq!(map.get(&bytes, 0), Some((0, 1)));
        assert_eq!(map.get(&bytes, 1), Some((1, 2)));
        assert_eq!(map.get(&bytes, 2), Some((2, 3)));
    }

    #[test]
    fn a_format_one_map_counts_its_entries_in_four_bytes() {
        // The two formats differ only in the width of the count, so a reader
        // that used the wrong one would read its first entry as a count.
        let bytes = index_map_bytes(1, 8, 2, &[(0, 1), (3, 4)]);
        let map = IndexMap::parse(&bytes, 0).unwrap();
        assert_eq!(map.get(&bytes, 1), Some((3, 4)));
    }

    #[test]
    fn a_truncated_index_map_is_refused() {
        let bytes = index_map_bytes(0, 8, 2, &[(0, 1), (3, 4)]);
        assert!(IndexMap::parse(&bytes[..bytes.len() - 1], 0).is_none());
    }

    // --- HVAR ---

    #[test]
    fn a_null_advance_map_means_outer_zero_and_inner_equals_the_glyph_id() {
        // Two of this host's seven variable faces (Cascadia Code and Mono)
        // take this path, and neither would catch a mistake in it: their
        // subtable is over zero regions, so every delta is zero regardless.
        // This fixture is the only thing that can tell the two readings apart.
        let store = simple(&[10, 20, 30]);
        let bytes = hvar_bytes(&store, None);
        let hvar = Hvar::parse(&bytes, 0, 1).unwrap();
        assert_eq!(hvar.advance_delta(&bytes, 2, &[ONE]), 30);
        assert_eq!(hvar.advance_delta(&bytes, 0, &[ONE]), 10);
    }

    #[test]
    fn an_advance_map_redirects_a_glyph_to_its_shared_row() {
        let store = simple(&[10, 20]);
        let map = index_map_bytes(0, 8, 2, &[(0, 0), (0, 0), (0, 1)]);
        let bytes = hvar_bytes(&store, Some(&map));
        let hvar = Hvar::parse(&bytes, 0, 1).unwrap();
        // Glyphs 0 and 1 share row 0; glyph 2 has its own.
        assert_eq!(hvar.advance_delta(&bytes, 0, &[ONE]), 10);
        assert_eq!(hvar.advance_delta(&bytes, 1, &[ONE]), 10);
        assert_eq!(hvar.advance_delta(&bytes, 2, &[ONE]), 20);
    }

    #[test]
    fn an_advance_delta_is_rounded_once_at_the_end() {
        // Two regions each contributing 2.5. Rounded once the answer is 5;
        // rounded per region it would be 6. The distinction is invisible on
        // any single-region face, which is most of them.
        let sub = Sub {
            regions: alloc::vec![0, 1],
            word_count: 0,
            long: false,
            rows: alloc::vec![alloc::vec![5, 5]],
        };
        let store = store_bytes(1, &[&[(0, ONE, ONE)], &[(0, ONE, ONE)]], &[sub]);
        let bytes = hvar_bytes(&store, None);
        let hvar = Hvar::parse(&bytes, 0, 1).unwrap();
        assert_eq!(hvar.advance_delta(&bytes, 0, &[HALF]), 5);
    }

    #[test]
    fn an_advance_does_not_vary_at_the_default_instance() {
        let store = simple(&[100]);
        let bytes = hvar_bytes(&store, None);
        let hvar = Hvar::parse(&bytes, 0, 1).unwrap();
        assert_eq!(hvar.advance_delta(&bytes, 0, &[0]), 0);
        // …and it does vary elsewhere, or the assertion above proves nothing.
        assert_eq!(hvar.advance_delta(&bytes, 0, &[ONE]), 100);
    }

    #[test]
    fn a_glyph_past_the_end_of_an_hvar_map_still_gets_the_last_row() {
        let store = simple(&[10, 20]);
        let map = index_map_bytes(0, 8, 2, &[(0, 0), (0, 1)]);
        let bytes = hvar_bytes(&store, Some(&map));
        let hvar = Hvar::parse(&bytes, 0, 1).unwrap();
        assert_eq!(hvar.advance_delta(&bytes, 9, &[ONE]), 20);
    }

    #[test]
    fn an_hvar_in_an_unknown_version_is_refused() {
        let store = simple(&[10]);
        let mut bytes = hvar_bytes(&store, None);
        bytes[1] = 2;
        assert!(Hvar::parse(&bytes, 0, 1).is_none());
    }

    // --- MVAR ---

    #[test]
    fn mvar_finds_each_tag_it_carries() {
        let store = simple(&[10, 20, 30]);
        let records = [(*b"cpht", 0, 0), (*b"hasc", 0, 1), (*b"xhgt", 0, 2)];
        let bytes = mvar_bytes(&store, &records);
        let mvar = Mvar::parse(&bytes, 0, 1).unwrap();
        assert_eq!(mvar.metric_delta(&bytes, *b"cpht", &[ONE]), 10);
        assert_eq!(mvar.metric_delta(&bytes, *b"hasc", &[ONE]), 20);
        assert_eq!(mvar.metric_delta(&bytes, *b"xhgt", &[ONE]), 30);
    }

    #[test]
    fn a_tag_the_face_does_not_carry_costs_nothing() {
        // Normal, not an error: a face varies the handful of metrics its
        // designer cared about.
        let store = simple(&[10]);
        let bytes = mvar_bytes(&store, &[(*b"hasc", 0, 0)]);
        let mvar = Mvar::parse(&bytes, 0, 1).unwrap();
        assert_eq!(mvar.metric_delta(&bytes, *b"unds", &[ONE]), 0);
        // A tag sorting before every record, and one sorting after: the binary
        // search must terminate on both sides rather than run off an end.
        assert_eq!(mvar.metric_delta(&bytes, *b"aaaa", &[ONE]), 0);
        assert_eq!(mvar.metric_delta(&bytes, *b"zzzz", &[ONE]), 0);
    }

    #[test]
    fn mvar_reaches_its_store_through_a_two_byte_offset() {
        // `MVAR` puts the store behind an Offset16 at +10; `HVAR` puts it
        // behind an Offset32 at +4. Reading four bytes here would land in the
        // record array and produce an offset of tens of thousands, so a
        // successful read of a real delta is what proves the width.
        let store = simple(&[42]);
        let bytes = mvar_bytes(&store, &[(*b"hasc", 0, 0)]);
        let mvar = Mvar::parse(&bytes, 0, 1).unwrap();
        assert_eq!(mvar.metric_delta(&bytes, *b"hasc", &[ONE]), 42);
    }

    #[test]
    fn mvar_records_wider_than_eight_bytes_are_read_at_their_stride() {
        // valueRecordSize is a stride so a later version can extend the
        // record. A reader that hard-coded 8 would find garbage in record 1.
        let store = simple(&[10, 20]);
        let records = [(*b"hasc", 0, 0), (*b"xhgt", 0, 1)];
        let bytes = mvar_sized(&store, &records, 12);
        let mvar = Mvar::parse(&bytes, 0, 1).unwrap();
        assert_eq!(mvar.metric_delta(&bytes, *b"hasc", &[ONE]), 10);
        assert_eq!(mvar.metric_delta(&bytes, *b"xhgt", &[ONE]), 20);
    }

    #[test]
    fn an_mvar_with_an_undersized_record_is_refused() {
        let store = simple(&[10]);
        let bytes = mvar_sized(&store, &[(*b"hasc", 0, 0)], 6);
        assert!(Mvar::parse(&bytes, 0, 1).is_none());
    }

    #[test]
    fn a_truncated_mvar_is_refused() {
        let store = simple(&[10]);
        let bytes = mvar_bytes(&store, &[(*b"hasc", 0, 0)]);
        assert!(Mvar::parse(&bytes[..14], 0, 1).is_none());
    }

    #[test]
    fn a_metric_does_not_vary_at_the_default_instance() {
        let store = simple(&[80]);
        let bytes = mvar_bytes(&store, &[(*b"xhgt", 0, 0)]);
        let mvar = Mvar::parse(&bytes, 0, 1).unwrap();
        assert_eq!(mvar.metric_delta(&bytes, *b"xhgt", &[0]), 0);
        assert_eq!(mvar.metric_delta(&bytes, *b"xhgt", &[ONE]), 80);
    }

    // --- rounding ---

    #[test]
    fn a_delta_rounds_half_away_from_zero() {
        assert_eq!(round_to_i16(2.5), 3);
        assert_eq!(round_to_i16(-2.5), -3);
        assert_eq!(round_to_i16(2.4), 2);
    }

    #[test]
    fn an_absurd_delta_saturates_rather_than_wrapping() {
        // A correction past an i16 is a broken face; clamping keeps the glyph
        // on the page instead of teleporting it to the other side.
        assert_eq!(round_to_i16(1.0e9), i16::MAX);
        assert_eq!(round_to_i16(-1.0e9), i16::MIN);
        // A non-finite delta is not a large correction, it is a nonsense one,
        // so it becomes no correction rather than the largest possible.
        assert_eq!(round_to_i16(f32::NAN), 0);
        assert_eq!(round_to_i16(f32::INFINITY), 0);
        assert_eq!(round_to_i16(f32::NEG_INFINITY), 0);
    }
}

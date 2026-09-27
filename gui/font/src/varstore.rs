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
//! interpolation, except at the default (a coordinate of 0), which it scores
//! 0 before it looks at the region at all. This does the same, in the same
//! order, for the reason design-decisions §448 gives for matching HarfBuzz
//! everywhere else: a deliberate divergence here would be indistinguishable
//! from a bug in any sweep that found it.
//!
//! This host has **0 degenerate region axes out of 199**, so nothing real
//! exercises the branch and the fixture has to.
//!
//! # HarfBuzz 14.3.0's arithmetic, to the bit
//!
//! A delta is `f32` throughout, as in HarfBuzz: each region's scalar a
//! product of `f32` factors, the row summed one product at a time
//! (`delta += scalar * column`, unfused), and a consumer rounding the sum by
//! HarfBuzz's own `roundf`, which sends a half up ([`round_to_i16`],
//! [`crate::hbcalc`]). An index map is read as HarfBuzz reads one: its
//! reserved entry-format bits ignored, an empty map passing the index
//! through. HarfBuzz also remembers region scalars in a cache that rounds
//! them to 2^-30; [`ScalarCache`] reproduces it where HarfBuzz's use of it is
//! deterministic (`avar` version 2), and says why not elsewhere.
//!
//! `avar` version 2 is also read on FreeType's side, whose store is
//! [`FtItemStore`]: FreeType's refusals, and its 16.16 sums.
//!
//! # References
//!
//! HarfBuzz 14.3.0 `hb-ot-layout-common.hh` (`VarRegionAxis::evaluate`,
//! `VarData::get_delta`, `DeltaSetIndexMap::map`, `hb_scalar_cache_t`) and
//! `hb-ot-var-hvar-table.hh`; FreeType 2.13.2 `ttgxvar.c`
//! (`tt_var_load_item_variation_store`, `tt_var_get_item_delta`,
//! `tt_var_load_delta_set_index_mapping`).

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegionAxis {
    start: i16,
    peak: i16,
    end: i16,
}

impl RegionAxis {
    /// How strongly this axis's constraint applies at `coord` (`F2Dot14`,
    /// widened: `avar` version 2 evaluates a store at coordinates a malformed
    /// curve can carry past what an `i16` holds), in `0.0..=1.0`.
    ///
    /// HarfBuzz 14.3.0's `VarRegionAxis::evaluate`, in its order, because the
    /// order is part of the answer: a zero peak, or a coordinate at the peak,
    /// scores 1 before anything else is looked at; then a coordinate at the
    /// default scores 0 -- *before* the checks for a malformed region, so a
    /// malformed region contributes nothing at the default either, as every
    /// well-formed one does; only then is a malformed region's constraint
    /// dropped (scored 1). See the module doc.
    fn factor(self, coord: i32) -> f32 {
        let (start, peak, end) = (
            i32::from(self.start),
            i32::from(self.peak),
            i32::from(self.end),
        );
        if peak == 0 || coord == peak {
            return 1.0;
        }
        if coord == 0 {
            return 0.0;
        }
        // Malformed: the constraint is unusable, so it is not applied. Matches
        // HarfBuzz rather than the plain reading of the spec; see module doc.
        if start > peak || peak > end {
            return 1.0;
        }
        if start < 0 && end > 0 {
            return 1.0;
        }
        if coord <= start || coord >= end {
            return 0.0;
        }
        // HarfBuzz divides the integer differences, each made a `float`.
        // Every term is within a few times 2^16 of zero -- endpoints are
        // `i16`s and a coordinate is at most a few units -- so the
        // subtractions are exact and so is each conversion.
        if coord < peak {
            exact_f32(coord.saturating_sub(start)) / exact_f32(peak.saturating_sub(start))
        } else {
            exact_f32(end.saturating_sub(coord)) / exact_f32(end.saturating_sub(peak))
        }
    }
}

/// `v` as an `f32`, which is exact for everything this module converts: a
/// difference of `F2Dot14` values, a coordinate, or a delta below 2^24.
fn exact_f32(v: i32) -> f32 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "exact below 2^24; a delta past that is rounded exactly as \
                  HarfBuzz's own int-to-float conversion rounds it"
    )]
    {
        v as f32
    }
}

/// HarfBuzz's region-scalar cache (`hb_scalar_cache_t`), reproduced because
/// it changes answers.
///
/// HarfBuzz remembers each region's scalar the first time an evaluation
/// needs it and hands the remembered value to every later one -- but it
/// remembers it as a whole number of 2^-30ths, so a scalar below 2^-7 comes
/// back from the cache slightly different from the one computed. The first
/// row to reach a region gets the exact scalar and every later row the
/// rounded one. `avar` version 2 reads one row per axis through one cache
/// (`avar::map_coords_16_16`), so its answer depends on this, and does so
/// deterministically; see [`crate::var`].
///
/// Not used for `HVAR`, `MVAR` or `GDEF`. HarfBuzz keeps the `HVAR` cache for
/// the life of the font and the `GDEF` one for a shaping call, so which
/// glyph reads a region first -- and gets the exact scalar -- depends on
/// what was shaped before. There is no one answer to reproduce there, and
/// the two readings differ by at most 2^-31 of a scalar.
#[derive(Clone, Debug)]
pub(crate) struct ScalarCache {
    /// One per region: `None` until computed, then its count of 2^-30ths.
    values: Vec<Option<i32>>,
}

impl ScalarCache {
    /// 2^30: the cache's unit is one of these parts.
    const PARTS: f32 = 1_073_741_824.0;

    /// A cache for a store of `regions` regions, all of them unknown.
    pub(crate) fn new(regions: usize) -> Self {
        Self {
            values: alloc::vec![None; regions],
        }
    }

    /// The remembered scalar of region `index`, as HarfBuzz reads it back.
    fn get(&self, index: usize) -> Option<f32> {
        let parts = (*self.values.get(index)?)?;
        Some(Self::read(parts))
    }

    /// Remember `scalar` for region `index`, rounded to 2^-30ths as
    /// HarfBuzz stores it: by its own `roundf` ([`crate::hbcalc`]), whose
    /// `x + 0.5` is itself rounded -- so a scalar just under 2^-6 can be
    /// remembered one part high, as well as a small one rounded.
    fn set(&mut self, index: usize, scalar: f32) {
        if let Some(slot) = self.values.get_mut(index) {
            *slot = Some(Self::parts(scalar));
        }
    }

    /// What a cache that has remembered `scalar` hands back for it: `scalar`
    /// stored as [`set`](Self::set) stores it and read as
    /// [`get`](Self::get) reads it.
    ///
    /// For a cache that lives longer than one evaluation, this is the value
    /// every evaluation but the first sees, and so the one to reproduce:
    /// `gvar`'s shared tuples, whose cache HarfBuzz keeps for the life of the
    /// font's instance ([`crate::gvar`]).
    pub(crate) fn remembered(scalar: f32) -> f32 {
        Self::read(Self::parts(scalar))
    }

    /// `scalar` in whole 2^-30ths, as the cache stores it.
    fn parts(scalar: f32) -> i32 {
        crate::hbcalc::roundf_i32(scalar * Self::PARTS)
    }

    /// A stored count of 2^-30ths as a scalar again.
    fn read(parts: i32) -> f32 {
        exact_f32(parts) * (1.0 / Self::PARTS)
    }
}

/// One `ItemVariationData` subtable: the shape of its delta rows and where
/// they begin.
///
/// The rows themselves are *not* read here. A face's store can hold thousands
/// of them and a draw touches a handful, so they are read on demand from the
/// face's own bytes — the same arrangement [`gvar`](crate::gvar) uses for its
/// per-glyph data, and for the same reason.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
        let row_size = word_count.checked_mul(wide)?.checked_add(
            region_index_count
                .checked_sub(word_count)?
                .checked_mul(narrow)?,
        )?;

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
#[derive(Clone, Debug, PartialEq, Eq)]
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

    /// How many regions the store names: the size of a [`ScalarCache`] for
    /// it.
    pub(crate) fn region_count(&self) -> usize {
        self.regions.len().checked_div(self.axis_count).unwrap_or(0)
    }

    /// How many regions subtable `outer`'s columns are over -- 0 for a
    /// subtable the store does not have (HarfBuzz's
    /// `get_region_index_count`).
    pub(crate) fn region_index_count(&self, outer: u16) -> usize {
        self.subtables
            .get(usize::from(outer))
            .map_or(0, |s| s.region_indices.len())
    }

    /// The scalar of each region subtable `outer` names, at `coords`, in its
    /// order -- none for a subtable the store does not have (HarfBuzz's
    /// `get_region_scalars`, which a `CFF2` blend weighs its deltas by).
    pub(crate) fn region_scalars(&self, outer: u16, coords: &[i16]) -> Vec<f32> {
        self.subtables
            .get(usize::from(outer))
            .map_or_else(Vec::new, |s| {
                s.region_indices
                    .iter()
                    .map(|&r| self.scalar(r, coords, None))
                    .collect()
            })
    }

    /// How strongly region `index` applies at `coords`, in `0.0..=1.0` --
    /// HarfBuzz's `VarRegionList::evaluate`, through `cache` when there is
    /// one (see [`ScalarCache`]).
    ///
    /// An axis the caller did not supply a coordinate for reads as 0 — the
    /// default instance — rather than aborting: a caller that has set two of
    /// three axes means the third to be at its default.
    fn scalar<C: Copy + Into<i32>>(
        &self,
        index: u16,
        coords: &[C],
        mut cache: Option<&mut ScalarCache>,
    ) -> f32 {
        let Some(start) = usize::from(index).checked_mul(self.axis_count) else {
            return 0.0;
        };
        let Some(region) = self
            .regions
            .get(start..start.checked_add(self.axis_count).unwrap_or(0))
        else {
            // A region the list does not have scores 0, and HarfBuzz answers
            // that before it looks in the cache.
            return 0.0;
        };
        if let Some(remembered) = cache.as_ref().and_then(|c| c.get(usize::from(index))) {
            return remembered;
        }
        let mut scale = 1.0f32;
        for (a, axis) in region.iter().enumerate() {
            let factor = axis.factor(coords.get(a).map_or(0, |&c| c.into()));
            if factor == 0.0 {
                // Nothing later can bring it back, and the remaining axes are
                // pure cost.
                scale = 0.0;
                break;
            }
            scale *= factor;
        }
        if let Some(cache) = cache.as_mut() {
            cache.set(usize::from(index), scale);
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
        self.delta_with(data, outer, inner, coords, None)
    }

    /// [`VarStore::delta`] at coordinates of any integer width, through
    /// `cache` when there is one: HarfBuzz 14.3.0's `VarData::get_delta`.
    ///
    /// The sum is `delta += scalar * column`, one product and one sum at a
    /// time, each rounded to `f32` -- not a fused multiply-add. HarfBuzz
    /// writes it that way and its builds are not compiled to fuse it (FMA
    /// needs a target the baseline x86-64 build does not assume), so fusing
    /// here would round differently on the occasional sum that lands within
    /// an `f32` step of a half. This used `mul_add` until 2026-09-26, on the
    /// belief that HarfBuzz's accumulator fuses.
    pub(crate) fn delta_with<C: Copy + Into<i32>>(
        &self,
        data: &[u8],
        outer: u16,
        inner: u16,
        coords: &[C],
        mut cache: Option<&mut ScalarCache>,
    ) -> Option<f32> {
        let sub = self.subtables.get(usize::from(outer))?;
        if inner >= sub.item_count {
            return None;
        }
        let mut total = 0.0f32;
        for (k, &region_index) in sub.region_indices.iter().enumerate() {
            let s = self.scalar(region_index, coords, cache.as_deref_mut());
            if s == 0.0 {
                continue;
            }
            // A column that fails to read is a truncated row. Dropping just
            // that column keeps the rest of the delta, which is the difference
            // between a slightly wrong width and none.
            if let Some(v) = sub.column(data, inner, k) {
                total += s * exact_f32(v);
            }
        }
        Some(total)
    }
}

/// A `DeltaSetIndexMap`: glyph id (or other ordinal) to an (outer, inner) pair.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// Parse the map at `at`, as HarfBuzz reads one.
    ///
    /// Format 0 counts entries in a `u16` and format 1 in a `u32`; both then
    /// pack each entry into `entrySize` bytes, big-endian, split into an outer
    /// and an inner index at a bit position the same byte declares.
    ///
    /// Bits 6 and 7 of that byte are reserved. HarfBuzz reads past them --
    /// the width is bits 4-5 and the split bits 0-3, whatever 6 and 7 say --
    /// and so does this, since a map is read here for HarfBuzz's answer.
    /// (FreeType refuses such a map; `avar` version 2's FreeType half has its
    /// own reader, [`FtItemStore::index_map`].) A map that is not there in
    /// full is `None`, as HarfBuzz's sanitizer makes it: no map, which is the
    /// implicit one.
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
        let inner_bits = (entry_format & 0x0F).checked_add(1)?;
        let entry_size = usize::try_from(((entry_format >> 4) & 0x03).checked_add(1)?).ok()?;
        let end = entries_at.checked_add(usize::try_from(count).ok()?.checked_mul(entry_size)?)?;
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
    ///
    /// A map of no entries passes the index through, as (`index >> 16`,
    /// `index & 0xFFFF`) -- HarfBuzz's reading, and the same pairing as no map
    /// at all. An entry's outer index is kept to its low 16 bits, which is
    /// where HarfBuzz's repacking into one `u32` leaves it.
    pub(crate) fn get(&self, data: &[u8], index: u32) -> Option<(u16, u16)> {
        let raw = match self.count.checked_sub(1) {
            None => return Some(split(index, 16)),
            Some(last) => {
                let clamped = usize::try_from(index.min(last)).ok()?;
                let at = self
                    .entries_at
                    .checked_add(clamped.checked_mul(self.entry_size)?)?;
                let bytes = data.get(at..at.checked_add(self.entry_size)?)?;
                bytes
                    .iter()
                    .fold(0u32, |raw, &b| raw.wrapping_shl(8) | u32::from(b))
            }
        };
        Some(split(raw, self.inner_bits))
    }
}

/// An index-map entry split at `inner_bits` into (outer, inner), the outer
/// kept to 16 bits.
fn split(raw: u32, inner_bits: u32) -> (u16, u16) {
    let inner = raw
        & 1u32
            .checked_shl(inner_bits)
            .map_or(u32::MAX, |b| b.wrapping_sub(1));
    let outer = raw.checked_shr(inner_bits).unwrap_or(0);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "both are masked to 16 bits first: the inner count is at \
                  most 16, and the outer keeps the bits HarfBuzz keeps"
    )]
    (outer as u16, (inner & 0xFFFF) as u16)
}

/// An `ItemVariationStore` as FreeType 2.13.2 loads and evaluates it
/// (`tt_var_load_item_variation_store`, `tt_var_get_item_delta`): the store
/// `avar` version 2 is read through on FreeType's side, which the
/// auto-hinter's coordinates come from.
///
/// Kept apart from [`VarStore`] because the two libraries differ in both
/// halves. FreeType evaluates in 16.16 integers -- each region's scalar by
/// `FT_MulDiv`, the deltas summed at 16.16 and rounded once
/// (`FT_MulAddFix`) -- where HarfBuzz uses `f32`. And it refuses what
/// HarfBuzz tolerates: a subtable with more region indices than there are
/// regions, or an index past the end, ends the load, and every subtable
/// from that one on is left without rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FtItemStore {
    axis_count: usize,
    /// Region `r`'s axis `a` at `r * axis_count + a`: start, peak and end,
    /// 16.16.
    regions: Vec<[i64; 3]>,
    /// One per subtable the store declares. One FreeType did not finish
    /// loading has no rows, so every row it is asked for is zero.
    subtables: Vec<Subtable>,
}

impl FtItemStore {
    /// FreeType's own bound on a store's regions (OpenType 1.8.4).
    const MAX_REGIONS: usize = 32_767;

    /// Load the store at `at` in `data`, as FreeType does.
    ///
    /// `None` where FreeType is left with no subtables at all -- a format
    /// other than 1, no subtables declared, an axis count other than
    /// `fvar`'s, too many regions, or a header or region list that is not
    /// there -- in which case `avar` version 2's step does not run. Otherwise
    /// the store, and whether it loaded without an error: FreeType reads
    /// the axis map only after a clean load.
    pub(crate) fn parse(data: &[u8], at: usize, axis_count: usize) -> Option<(Self, bool)> {
        if axis_count == 0 || axis_count > MAX_AXES || u16_at(data, at)? != 1 {
            return None;
        }
        let regions_at =
            at.checked_add(usize::try_from(u32_at(data, at.checked_add(2)?)?).ok()?)?;
        let data_count = usize::from(u16_at(data, at.checked_add(6)?)?);
        if data_count == 0 {
            return None;
        }
        let mut offsets = Vec::with_capacity(data_count);
        for i in 0..data_count {
            offsets.push(u32_at(
                data,
                at.checked_add(8)?.checked_add(i.checked_mul(4)?)?,
            )?);
        }
        if usize::from(u16_at(data, regions_at)?) != axis_count {
            return None;
        }
        let region_count = usize::from(u16_at(data, regions_at.checked_add(2)?)?);
        if region_count > Self::MAX_REGIONS {
            return None;
        }
        // Every region must be there before any is kept, which also bounds
        // the allocation by the table's own length.
        let fields = region_count.checked_mul(axis_count)?;
        if regions_at
            .checked_add(4)?
            .checked_add(fields.checked_mul(6)?)?
            > data.len()
        {
            return None;
        }
        let mut regions = Vec::with_capacity(fields);
        for k in 0..fields {
            let axis = regions_at.checked_add(4)?.checked_add(k.checked_mul(6)?)?;
            regions.push([
                crate::ftcalc::f2dot14_to_fixed(i16_at(data, axis)?),
                crate::ftcalc::f2dot14_to_fixed(i16_at(data, axis.checked_add(2)?)?),
                crate::ftcalc::f2dot14_to_fixed(i16_at(data, axis.checked_add(4)?)?),
            ]);
        }
        let mut subtables = alloc::vec![Subtable::default(); data_count];
        let mut clean = true;
        for (slot, &rel) in subtables.iter_mut().zip(&offsets) {
            let sub = at
                .checked_add(usize::try_from(rel).ok()?)
                .and_then(|off| Subtable::parse(data, off))
                .filter(|sub| {
                    sub.region_indices.len() <= region_count
                        && sub
                            .region_indices
                            .iter()
                            .all(|&r| usize::from(r) < region_count)
                });
            match sub {
                Some(sub) => *slot = sub,
                None => {
                    clean = false;
                    break;
                }
            }
        }
        Some((
            Self {
                axis_count,
                regions,
                subtables,
            },
            clean,
        ))
    }

    /// The delta at row (`outer`, `inner`) at the 16.16 `coords`, as
    /// `tt_var_get_item_delta` computes it: zero for the no-variation pair
    /// (`0xFFFF`, `0xFFFF`) and for a row that is not there; otherwise the
    /// sum over the row's regions of scalar times delta, at 16.16, rounded
    /// once and narrowed to 32 bits as `FT_MulAddFix` does.
    pub(crate) fn delta(&self, data: &[u8], outer: u16, inner: u16, coords: &[i32]) -> i32 {
        if outer == 0xFFFF && inner == 0xFFFF {
            return 0;
        }
        let Some(sub) = self.subtables.get(usize::from(outer)) else {
            return 0;
        };
        if inner >= sub.item_count {
            return 0;
        }
        let mut sum: i128 = 0;
        for (k, &region) in sub.region_indices.iter().enumerate() {
            let scalar = self.scalar(region, coords);
            let delta = sub.column(data, inner, k).unwrap_or(0);
            sum = sum.saturating_add(i128::from(scalar).saturating_mul(i128::from(delta)));
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "`FT_MulAddFix`'s `(FT_Int32)` of its rounded sum"
        )]
        {
            (sum.saturating_add(0x8000) >> 16) as i32
        }
    }

    /// Region `index`'s scalar at the 16.16 `coords`, in 16.16, as FreeType
    /// computes it: an axis whose range is inverted, straddles zero around a
    /// non-zero peak, or peaks at zero is passed over, as is one sitting at
    /// its peak; one outside its range makes the region not apply; any other
    /// scales the product by `FT_MulDiv`.
    fn scalar(&self, index: u16, coords: &[i32]) -> i64 {
        use crate::ftcalc::mul_div;
        let Some(start) = usize::from(index).checked_mul(self.axis_count) else {
            return 0;
        };
        let Some(region) = self
            .regions
            .get(start..start.checked_add(self.axis_count).unwrap_or(0))
        else {
            return 0;
        };
        let mut scalar: i64 = 0x1_0000;
        for (j, &[s, p, e]) in region.iter().enumerate() {
            let c = coords.get(j).map_or(0, |&c| i64::from(c));
            // FreeType's straddle test also asks for a non-zero peak, which
            // the zero-peak test beside it makes redundant.
            if s > p || p > e || p == 0 || (s < 0 && e > 0) || c == p {
                continue;
            }
            if c <= s || c >= e {
                return 0;
            }
            scalar = if c < p {
                mul_div(scalar, c.saturating_sub(s), p.saturating_sub(s))
            } else {
                mul_div(scalar, e.saturating_sub(c), e.saturating_sub(p))
            };
        }
        scalar
    }

    /// `avar` version 2's axis-index map at `at`, as FreeType's
    /// `tt_var_load_delta_set_index_mapping` loads it against this store.
    ///
    /// `None` -- no map, so axis *i* reads row (0, *i*) -- for a format other
    /// than 0 or 1, reserved bits set in the entry format, no entries, or
    /// more entry bytes than the whole `avar` table (`table_len`) holds.
    /// Otherwise one (outer, inner) pair per entry, `0xFFFFFFFF` becoming the
    /// no-variation pair. FreeType stops at the first entry naming a subtable
    /// or row the store does not have, and at the end of the data, leaving
    /// that entry and every later one at (0, 0) -- except that an entry whose
    /// subtable exists keeps its outer index when its row does not.
    pub(crate) fn index_map(
        &self,
        data: &[u8],
        at: usize,
        table_len: usize,
    ) -> Option<Vec<(u16, u16)>> {
        let format = *data.get(at)?;
        let entry_format = *data.get(at.checked_add(1)?)?;
        let (count, mut pos) = match format {
            0 => (
                u32::from(u16_at(data, at.checked_add(2)?)?),
                at.checked_add(4)?,
            ),
            1 => (u32_at(data, at.checked_add(2)?)?, at.checked_add(6)?),
            _ => return None,
        };
        if entry_format & 0xC0 != 0 {
            return None;
        }
        let entry_size = usize::from((entry_format & 0x30) >> 4).checked_add(1)?;
        let inner_bits = u32::from(entry_format & 0x0F).checked_add(1)?;
        let count = usize::try_from(count).ok()?;
        if count == 0 || count.checked_mul(entry_size)? > table_len {
            return None;
        }
        let mut map = alloc::vec![(0u16, 0u16); count];
        for slot in &mut map {
            let Some(bytes) = pos
                .checked_add(entry_size)
                .and_then(|end| data.get(pos..end))
            else {
                break;
            };
            pos = pos.checked_add(entry_size)?;
            let raw = bytes
                .iter()
                .fold(0u32, |raw, &b| raw.wrapping_shl(8) | u32::from(b));
            if raw == u32::MAX {
                *slot = (0xFFFF, 0xFFFF);
                continue;
            }
            let outer = raw.checked_shr(inner_bits).unwrap_or(0);
            let Some((outer, sub)) = u16::try_from(outer)
                .ok()
                .and_then(|o| Some((o, self.subtables.get(usize::from(o))?)))
            else {
                break;
            };
            slot.0 = outer;
            let inner = raw
                & 1u32
                    .checked_shl(inner_bits)
                    .map_or(u32::MAX, |b| b.wrapping_sub(1));
            match u16::try_from(inner) {
                Ok(inner) if inner < sub.item_count => slot.1 = inner,
                _ => break,
            }
        }
        Some(map)
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
            let at = self
                .records_at
                .checked_add(mid.checked_mul(self.record_size)?)?;
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
/// By HarfBuzz's `roundf` ([`crate::hbcalc`]), which sends a half *up*:
/// `HVAR` is `advance + roundf(delta)`, and `MVAR` and `GDEF` deltas are
/// rounded as part of scaling the sum they are in (`em_scalef`), which at a
/// scale of one font unit per unit is `roundf` of the delta plus a whole
/// number. A delta of -10.5 is -10 here, as in HarfBuzz; this rounded halves
/// away from zero, to -11, until 2026-09-26.
///
/// Saturating rather than wrapping: a correction larger than an `i16` is a
/// broken face, and clamping keeps the glyph on the page.
///
/// Shared with [`device`](crate::device), which rounds a `GDEF` delta the same
/// way `HVAR` and `MVAR` round theirs — three tables reading one store should
/// not disagree about what 0.5 of a unit is.
pub(crate) fn round_to_i16(v: f32) -> i16 {
    if !v.is_finite() {
        return 0;
    }
    let r = crate::hbcalc::roundf(v);
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

/// A one-axis `ItemVariationStore` with a single region peaking at the far end
/// of that axis and a single subtable of one-column rows, one row per entry of
/// `rows`.
///
/// The delta of row *i* is therefore `rows[i] * coord / 16384` — linear in the
/// caller's coordinate, and computable on paper, which is what makes it useful
/// as an expected value rather than merely as input.
///
/// Here rather than in this module's test module because [`device`] needs the
/// same fixture to exercise a `VariationIndex`, and two hand-rolled stores
/// would be two chances to lay one out the way a wrong reader reads it — the
/// same argument that puts [`device::table`](crate::device::table) where it is.
/// Written from the specification's field order, not from the parser above.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test fixture builder: a panic here is the diagnosis"
)]
pub(crate) fn one_axis_store(rows: &[i32]) -> alloc::vec::Vec<u8> {
    /// `F2Dot14` for 1.0 — the far end of the axis.
    const ONE: i16 = 16384;

    let mut out = alloc::vec::Vec::new();
    // ItemVariationStore header.
    out.extend_from_slice(&1u16.to_be_bytes()); // format
    out.extend_from_slice(&12u32.to_be_bytes()); // variationRegionListOffset
    out.extend_from_slice(&1u16.to_be_bytes()); // itemVariationDataCount
    out.extend_from_slice(&22u32.to_be_bytes()); // itemVariationDataOffsets[0]
    // VariationRegionList, at 12: one axis, one region.
    out.extend_from_slice(&1u16.to_be_bytes()); // axisCount
    out.extend_from_slice(&1u16.to_be_bytes()); // regionCount
    out.extend_from_slice(&0i16.to_be_bytes()); // startCoord
    out.extend_from_slice(&ONE.to_be_bytes()); // peakCoord
    out.extend_from_slice(&ONE.to_be_bytes()); // endCoord
    // ItemVariationData, at 22: every column narrow, over region 0 alone.
    out.extend_from_slice(&u16::try_from(rows.len()).unwrap().to_be_bytes()); // itemCount
    out.extend_from_slice(&0u16.to_be_bytes()); // wordDeltaCount
    out.extend_from_slice(&1u16.to_be_bytes()); // regionIndexCount
    out.extend_from_slice(&0u16.to_be_bytes()); // regionIndexes[0]
    for &v in rows {
        out.extend_from_slice(&i8::try_from(v).unwrap().to_be_bytes());
    }
    out
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
    ///
    /// Deliberately *not* delegating to [`one_axis_store`], which builds the
    /// same thing: the two are cross-checked against each other below, and a
    /// delegation would make that check vacuous.
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
            .scalar(0, coords, None)
    }

    #[test]
    fn the_two_fixture_builders_lay_out_the_same_store() {
        // `simple` composes the general builders; `one_axis_store` writes the
        // bytes out flat so that `device`'s tests can use it without importing
        // all of them. They are two transcriptions of one spec section, so
        // byte equality is a real check on both — and it is what lets a
        // failure in `device` be read as "the store is misparsed" rather than
        // "the fixture is malformed".
        for rows in [&[0i32][..], &[7, -3, 40][..], &[1; 9][..]] {
            assert_eq!(simple(rows), one_axis_store(rows), "rows {rows:?}");
        }
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
        assert_eq!(scalar_of(&[&[(-ONE, HALF, ONE)]], &[ONE]), 1.0);
        assert_eq!(scalar_of(&[&[(-ONE, HALF, ONE)]], &[-HALF]), 1.0);
    }

    #[test]
    fn at_the_default_a_region_contributes_nothing_malformed_or_not() {
        // HarfBuzz 14.3.0 answers "the coordinate is at the default" before
        // it looks for a malformed region, so a region it would otherwise
        // ignore still scores 0 there -- as every well-formed one does.
        assert_eq!(scalar_of(&[&[(-ONE, HALF, ONE)]], &[0]), 0.0);
        assert_eq!(scalar_of(&[&[(HALF, 4096, ONE)]], &[0]), 0.0);
        // A zero peak is not a constraint at all, at the default or anywhere.
        assert_eq!(scalar_of(&[&[(0, 0, 0)]], &[0]), 1.0);
    }

    #[test]
    fn a_delta_sums_one_product_at_a_time_without_fusing() {
        // Column 0 is -1 at a scalar of 1; column 1 is 3 at a scalar of 1/3.
        // In f32, 1/3 * 3 is 1.0000000298, which rounds to 1 on its own --
        // so HarfBuzz's `delta += scalar * column` reaches exactly 0, where a
        // fused multiply-add would keep the 2.98e-8.
        let bytes = store_bytes(
            1,
            &[&[(0, 1, 1)], &[(0, 3, 3)]],
            &[Sub::narrow(2, &[&[-1, 3]])],
        );
        let store = VarStore::parse(&bytes, 0, 1).unwrap();
        assert_eq!(store.delta(&bytes, 0, 0, &[1]), Some(0.0));
    }

    #[test]
    fn a_cache_hands_back_a_small_scalar_as_harfbuzz_remembers_it() {
        // A scalar of 1/1000 is not a whole number of 2^-30ths. The first
        // row to need it gets it exactly; the cache keeps it rounded.
        let bytes = store_bytes(
            1,
            &[&[(0, 1000, 1000)]],
            &[Sub::narrow(1, &[&[100], &[100]])],
        );
        let store = VarStore::parse(&bytes, 0, 1).unwrap();
        let mut cache = ScalarCache::new(store.region_count());
        let first = store
            .delta_with(&bytes, 0, 0, &[1i32], Some(&mut cache))
            .unwrap();
        let second = store
            .delta_with(&bytes, 0, 1, &[1i32], Some(&mut cache))
            .unwrap();
        let exact = 1.0f32 / 1000.0;
        let remembered = crate::hbcalc::roundf(exact * 1_073_741_824.0) / 1_073_741_824.0;
        assert_ne!(exact, remembered, "the fixture must reach the rounding");
        assert_eq!(first, exact * 100.0);
        assert_eq!(second, remembered * 100.0);
        // Without a cache, every row gets the exact scalar.
        assert_eq!(store.delta(&bytes, 0, 1, &[1]), Some(exact * 100.0));
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
    fn an_index_map_is_read_past_its_reserved_bits_as_harfbuzz_reads_it() {
        // Bits 6-7 of the entry format are reserved. HarfBuzz takes the width
        // from bits 4-5 and the split from bits 0-3 whatever they say, and so
        // does this; FreeType's refusal is `FtItemStore::index_map`'s.
        let mut bytes = index_map_bytes(0, 8, 2, &[(0, 1), (2, 3)]);
        bytes[1] |= 0xC0;
        let map = IndexMap::parse(&bytes, 0).unwrap();
        assert_eq!(map.get(&bytes, 1), Some((2, 3)));
    }

    #[test]
    fn an_index_map_of_no_entries_passes_the_index_through() {
        let bytes = index_map_bytes(0, 16, 4, &[]);
        let map = IndexMap::parse(&bytes, 0).unwrap();
        assert_eq!(map.get(&bytes, 7), Some((0, 7)));
        assert_eq!(map.get(&bytes, 0x0003_0009), Some((3, 9)));
    }

    #[test]
    fn an_outer_index_keeps_only_its_low_sixteen_bits() {
        // Three-byte entries split at 4 bits leave a 20-bit outer index;
        // HarfBuzz repacks outer and inner into one u32, which drops all but
        // the outer's low 16 bits.
        let mut bytes = alloc::vec![0u8, 0x23];
        push16(&mut bytes, 1);
        bytes.extend_from_slice(&[0x10, 0x00, 0x25]);
        let map = IndexMap::parse(&bytes, 0).unwrap();
        assert_eq!(map.get(&bytes, 0), Some((0x0002, 5)));
    }

    // --- FreeType's store, for `avar` version 2 ---

    #[test]
    fn freetype_sums_a_row_in_sixteen_sixteen_and_rounds_once() {
        // One region peaking at 0.5 of the axis, read at 0.25 of it: a scalar
        // of exactly 0.5, times 3, is 1.5 -- which `FT_MulAddFix` rounds up.
        let bytes = store_bytes(1, &[&[(0, 8192, ONE)]], &[Sub::narrow(1, &[&[3], &[-3]])]);
        let (store, clean) = FtItemStore::parse(&bytes, 0, 1).unwrap();
        assert!(clean);
        assert_eq!(store.delta(&bytes, 0, 0, &[0x4000]), 2);
        // (-3 * 0x8000 + 0x8000) >> 16 is -1: a negative half goes up too.
        assert_eq!(store.delta(&bytes, 0, 1, &[0x4000]), -1);
        // The no-variation pair, and a row the store does not have.
        assert_eq!(store.delta(&bytes, 0xFFFF, 0xFFFF, &[0x4000]), 0);
        assert_eq!(store.delta(&bytes, 0, 9, &[0x4000]), 0);
    }

    #[test]
    fn freetype_stops_loading_at_a_subtable_naming_a_region_it_lacks() {
        // Subtable 0 is good; subtable 1 names region 5 of a list of one.
        // HarfBuzz keeps both and scores region 5 as zero; FreeType loads
        // subtable 0, stops, and reports the load as unclean.
        let mut bad = Sub::narrow(1, &[&[7]]);
        bad.regions = alloc::vec![5];
        let bytes = store_bytes(1, &[&[(0, ONE, ONE)]], &[Sub::narrow(1, &[&[7]]), bad]);
        let (store, clean) = FtItemStore::parse(&bytes, 0, 1).unwrap();
        assert!(!clean);
        assert_eq!(store.delta(&bytes, 0, 0, &[0x1_0000]), 7);
        assert_eq!(store.delta(&bytes, 1, 0, &[0x1_0000]), 0);
        let harfbuzz = VarStore::parse(&bytes, 0, 1).unwrap();
        assert_eq!(harfbuzz.delta(&bytes, 1, 0, &[ONE]), Some(0.0));
        // No store at all where FreeType would have none: no subtables, or
        // an axis count other than fvar's.
        let empty = store_bytes(1, &[&[(0, ONE, ONE)]], &[]);
        assert!(FtItemStore::parse(&empty, 0, 1).is_none());
        assert!(FtItemStore::parse(&bytes, 0, 2).is_none());
    }

    #[test]
    fn freetype_reads_an_axis_map_until_an_entry_names_nothing() {
        let bytes = store_bytes(1, &[&[(0, ONE, ONE)]], &[Sub::narrow(1, &[&[1], &[2]])]);
        let (store, _) = FtItemStore::parse(&bytes, 0, 1).unwrap();
        let map_at = bytes.len();
        let mut with_map = bytes.clone();
        // (0, 1); no variation; (0, 9) -- a row the subtable lacks; (0, 0).
        let mut map = index_map_bytes(0, 16, 4, &[(0, 1), (0xFFFF, 0xFFFF), (0, 9), (0, 0)]);
        with_map.append(&mut map);
        let read = store.index_map(&with_map, map_at, with_map.len()).unwrap();
        assert_eq!(read, [(0, 1), (0xFFFF, 0xFFFF), (0, 0), (0, 0)]);
        // Reserved bits set: FreeType refuses the whole map.
        let mut reserved = with_map.clone();
        reserved[map_at + 1] |= 0x40;
        assert!(store.index_map(&reserved, map_at, reserved.len()).is_none());
        // More entry bytes than the whole table: refused too.
        assert!(store.index_map(&with_map, map_at, 8).is_none());
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
    fn a_delta_rounds_half_up_as_harfbuzz_rounds() {
        // HarfBuzz's `roundf` is `floorf(x + 0.5f)`: a half goes towards
        // positive infinity on both sides of zero.
        assert_eq!(round_to_i16(2.5), 3);
        assert_eq!(round_to_i16(-2.5), -2);
        assert_eq!(round_to_i16(-2.6), -3);
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

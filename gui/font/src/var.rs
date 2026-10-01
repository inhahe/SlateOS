//! Variable fonts: the axes a face offers, and where on them a caller is.
//!
//! A variable font ships one set of outlines plus a set of *deltas*, and a
//! point on a coordinate space says how much of each delta to apply. This
//! module is the first half of that — the coordinate space itself — and does
//! not move a single point. Reading the deltas needs this, so this comes
//! first; see `TD-FONT-DOES-NOT-READ-VARIATION-STORES` in `known-issues.md`
//! for why the order is not negotiable.
//!
//! # The two coordinate spaces, which are easy to confuse
//!
//! * **User coordinates** are what a person means: weight 700, optical size
//!   11. They live on whatever scale the axis declares (`wght` is
//!   conventionally 1..1000, `opsz` is in points), and every axis has its own.
//! * **Normalized coordinates** are what the font's delta tables are indexed
//!   by: `-1.0` at the axis minimum, `0.0` at its default, `+1.0` at its
//!   maximum, always, for every axis. They are stored as `F2Dot14`.
//!
//! Everything outside this module should be thinking in user coordinates;
//! everything inside a variation table is in normalized ones.
//! [`Variations::normalize`] is the only bridge, and it is deliberately the
//! only public way to make a [`Coords`].
//!
//! # Why `avar` exists
//!
//! Straight-line normalization assumes the design is linear between the
//! minimum, the default and the maximum, and for weight it very often is not:
//! a family whose `wght` runs 300..400..700 does not put its Semibold half way
//! between Regular and Bold. `avar` is the face's own correction — a
//! piecewise-linear remap applied *after* normalization — so that a request
//! for weight 600 lands where the designer drew Semibold. Six of this host's
//! seven variable faces carry one, so a reader that skipped `avar` would be
//! wrong far more often than right.
//!
//! Version 2 of the table adds a second step: an item variation store (the
//! structure `HVAR` and `MVAR` use, see `varstore.rs`) whose deltas
//! move each axis's coordinate by where *all* the axes are, so that one axis
//! can depend on another -- a condensed Bold drawn lighter than a wide one.
//!
//! # Where the coordinates live
//!
//! On the caller's side, not on the [`Face`](crate::sfnt::Face). A parsed face
//! is shared and must be usable at two instances at once — a document showing
//! the same family at Regular and Bold has one file open — so the axes and the
//! mapping (properties of the *file*) are parsed once onto the face, while the
//! chosen point (a property of the *request*) rides with the scaled font.
//!
//! # Two libraries' coordinates, each to the bit
//!
//! Normalizing looks like arithmetic with one answer, and is not: every step
//! rounds, and the two libraries this crate follows round at different
//! places. So a [`Coords`] carries two readings of one position, each
//! reproducing its library exactly -- for malformed tables as well as
//! ordinary ones, since a deliberate divergence would be indistinguishable
//! from a bug in any comparison that found it.
//!
//! * **HarfBuzz 14.3.0**, [`Coords::as_slice`], which shaping, `HVAR`,
//!   `MVAR`, `GDEF` and `COLR` are indexed by (`hb_ot_var_normalize_coords`).
//!   The value is normalized in `f32` and rounded to 16.16; `avar`'s curve is
//!   applied to that in `f32` (`SegmentMaps::map_float`, with its own answers
//!   for curves the specification does not allow) and rounded to 16.16
//!   again; version 2's store is read at the result rounded to `F2Dot14` and
//!   its delta added at 16.16; and only then is the whole rounded to
//!   `F2Dot14`, by `(c + 2) >> 2`. Every one of those roundings sends a half
//!   *up* -- HarfBuzz's `roundf` is its own, `floorf(x + 0.5f)`
//!   (`hbcalc.rs`), not the C library's -- and the last comes after the
//!   first, so -2.5 units becomes -2, and weight 700 of 100..400..900, 9830.4
//!   units, becomes 9831. Until 2026-09-26 this crate followed HarfBuzz 8,
//!   which rounds once, straight to `F2Dot14`, and rounded halves away from
//!   zero; it disagreed with 14.3.0 by a unit on one instance in eight (12.5%
//!   of a uniform sweep), Bold among them.
//! * **FreeType 2.13.2**, `Coords::fixed` (crate-private), which the
//!   auto-hinter's points are varied by (`crate::hint`): 16.16 integers
//!   throughout (`ft_var_to_normalized`). `FT_DivFix` toward the nearer end,
//!   `avar`'s curve by `FT_MulDiv` from the first segment that ends past the
//!   value, version 2's delta as `tt_var_get_item_delta` sums it, and never
//!   rounded to 14 bits. FreeType is handed design coordinates as `Fixed`s:
//!   an axis nobody names, and a named instance, are the file's own `Fixed`s,
//!   exact.
//!
//! The two agree to a 65536th and differ below it -- weight 700 of
//! 100..400..900 is `0.60000610` in FreeType and `0.60003662` in HarfBuzz --
//! which is enough to round a delta the other way, and a hinted stem with it.
//! `tools/gen_var_fixture.py` records both libraries' answers on faces built
//! to reach each rule, and the tests hold this module to them;
//! `tools/var_oracle.py` asks both about thousands of instances of real
//! faces.
//!
//! Portions of this file follow HarfBuzz 14.3.0's
//! `src/hb-ot-var-avar-table.hh`, copyright © 2017 Google, Inc. Used under
//! HarfBuzz's licence: see `gui/font/licenses/harfbuzz-COPYING`.
//!
//! `Coords::fixed` follows FreeType 2.13.2's `src/truetype/ttgxvar.c`,
//! copyright (C) 2004-2023 by David Turner, Robert Wilhelm, Werner Lemberg and
//! George Williams, from The FreeType Project (www.freetype.org). Used under
//! the FreeType License: see `gui/font/licenses/FTL.TXT`.

use alloc::vec::Vec;

use crate::hbcalc::roundf_i32;
use crate::sfnt::{Span, i16_at, tag_at, u16_at, u32_at};
use crate::varstore::{FtItemStore, IndexMap, ScalarCache, VarStore};

/// `fvar`'s fixed axis-record size. The header declares its own `axisSize` and
/// this is the only value any real face uses, but the declared one is honoured
/// (it may legally be *larger*, with trailing fields this does not read) and
/// this is only the minimum a record must reach to be readable.
const AXIS_RECORD_MIN: usize = 20;

/// `fvar` bit 0 of a `VariationAxisRecord`'s flags: the axis should not be
/// shown in a user interface.
const AXIS_HIDDEN: u16 = 0x0001;

/// A ceiling on the axis count, well above any real face.
///
/// The format allows 65,535. Real faces carry one to four; the largest ever
/// shipped is around eight. The bound matters because the axis count is a word
/// read from the file that sizes every allocation and every per-axis loop
/// below, including in tables that declare it a second time — so an
/// unreasonable one is rejected at the door rather than trusted into an
/// allocation.
const MAX_AXES: usize = 64;

/// A ceiling on `fvar`'s named-instance count, for the same reason.
///
/// The most on this host is 18. This is generous enough that no plausible face
/// hits it and small enough that a malformed count cannot ask for gigabytes.
const MAX_INSTANCES: usize = 4096;

/// 1.0 in 16.16.
const FIXED_ONE: i32 = 0x1_0000;

/// One axis of variation the face offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    /// The registered tag: `wght`, `wdth`, `opsz`, `ital`, `slnt`, or a
    /// foundry's own four bytes.
    pub tag: [u8; 4],
    /// Smallest value the face was drawn for, in user coordinates.
    pub min: f32,
    /// Where the face sits when nobody asks for anything.
    pub default: f32,
    /// Largest value the face was drawn for.
    pub max: f32,
    /// The face asks that this axis not be offered in a UI — normally because
    /// it exists to serve another axis rather than to be chosen directly.
    pub hidden: bool,
    /// `name` table id for the axis's human-readable name.
    pub name_id: u16,
}

/// A point on the axes that the face has given a name — "Semibold Condensed".
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    /// `name` table id of the instance's subfamily name.
    pub subfamily_name_id: u16,
    /// `name` table id of its PostScript name, when the face gives one.
    pub postscript_name_id: Option<u16>,
    /// Position in **user** coordinates, one entry per axis, in `fvar` order.
    pub coords: Vec<f32>,
}

/// A position on the face's axes, in normalized `F2Dot14` — the form every
/// variation table is indexed by — and, beside it, the same position as
/// FreeType normalizes it (see the module docs).
///
/// Made only by [`Variations::normalize`] and its siblings, so that a value of
/// this type is always the output of the full pipeline (clamp, normalize,
/// `avar`) and never a raw user number that happens to be in range. Each
/// vector is one entry per axis, in `fvar` order, which is the order every
/// variation region in the file is written in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coords {
    /// `F2Dot14`, as HarfBuzz normalizes.
    norm: Vec<i16>,
    /// 16.16, as FreeType normalizes.
    fixed: Vec<i32>,
}

impl Coords {
    /// The normalized coordinates, in `fvar` axis order.
    #[must_use]
    pub fn as_slice(&self) -> &[i16] {
        &self.norm
    }

    /// The same position as FreeType's `ft_var_to_normalized` puts it, in
    /// 16.16: what the auto-hinter's points are varied by.
    #[must_use]
    pub(crate) fn fixed(&self) -> &[i32] {
        &self.fixed
    }

    /// How many axes this position covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.norm.len()
    }

    /// Whether this position covers no axes at all — which is what a
    /// non-variable face yields.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.norm.is_empty()
    }

    /// Whether every axis sits at its default.
    ///
    /// Worth its own method because it is the fast path: a variable face asked
    /// for its default instance must render byte-identically to the same face
    /// read by a non-variable reader, and the cheapest way to guarantee that is
    /// to skip the delta machinery entirely rather than to apply deltas that
    /// ought to sum to zero.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.norm.iter().all(|&c| c == 0)
    }

    /// Whether FreeType would take this for the default instance: every
    /// 16.16 coordinate zero, when no tuple of a variation table applies.
    #[must_use]
    pub(crate) fn is_default_fixed(&self) -> bool {
        self.fixed.iter().all(|&c| c == 0)
    }

    /// One axis's coordinate, or `None` past the end.
    #[must_use]
    pub fn get(&self, axis: usize) -> Option<i16> {
        self.norm.get(axis).copied()
    }
}

/// `avar` as HarfBuzz 14.3.0 reads it (`OT::avar`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct HbAvar {
    /// The curve of each axis both `avar` and `fvar` have -- HarfBuzz pairs
    /// them by position, as many as both have -- each point `(from, to)` in
    /// `F2Dot14` and exactly as stored: [`map_float`] has its own answer for
    /// a curve that is unsorted, repeats a `from`, or is too short.
    curves: Vec<Vec<(i16, i16)>>,
    /// Version 2's axis-index map and store; `None` for version 1. Either
    /// may itself be missing -- an offset of zero, or a part HarfBuzz's
    /// sanitizer rejects and so zeroes -- and the step then runs without
    /// it: no map is axis *i* reading row (0, *i*), no store is no delta.
    v2: Option<(Option<IndexMap>, Option<VarStore>)>,
}

/// `avar` as FreeType 2.13.2 reads it (`ft_var_load_avar`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct FtAvar {
    /// One curve per axis, each point `(from, to)` in `F2Dot14`, as stored.
    curves: Vec<Vec<(i16, i16)>>,
    /// Version 2's store and axis map, when FreeType ends up with a store;
    /// without one the step does not run.
    v2: Option<FtAvar2>,
}

/// `avar` version 2's store as FreeType loads it, and its axis map -- one
/// (outer, inner) row per entry, as [`FtItemStore::index_map`] reads it --
/// if FreeType reads one.
type FtAvar2 = (FtItemStore, Option<Vec<(u16, u16)>>);

/// Everything the file says about its own variation, parsed once.
///
/// `None` on a [`Face`](crate::sfnt::Face) means the face is not variable, which
/// is 549 of this host's 556.
#[derive(Clone, Debug, PartialEq)]
pub struct Variations {
    axes: Vec<Axis>,
    /// Each axis's minimum, default and maximum as FreeType holds them, 16.16:
    /// as `fvar` stores them, except that an axis whose default is outside
    /// its own range has both ends pinned to the default (`TT_Get_MM_Var`).
    limits: Vec<[i64; 3]>,
    instances: Vec<Instance>,
    /// Each named instance's coordinates as the `Fixed`s `fvar` stores,
    /// which is how FreeType takes them.
    instance_fixed: Vec<Vec<i64>>,
    /// `avar` as HarfBuzz reads it; `None` for no table, or one it rejects.
    hb_avar: Option<HbAvar>,
    /// `avar` as FreeType reads it; `None` for no table, or one it ignores.
    ft_avar: Option<FtAvar>,
    /// The `avar` table's bytes when either reading has a version-2 part,
    /// whose stores read their rows from it on demand; empty otherwise.
    avar: Vec<u8>,
}

impl Variations {
    /// Parse `fvar`, and `avar` if the face has one.
    ///
    /// Returns `None` rather than an error for a face whose `fvar` is
    /// unreadable: a malformed variation table should cost the face its
    /// variability, not its ability to draw. Every other table is still good,
    /// and the default instance is a complete, correct font.
    ///
    /// `avar` is read twice, once as each library reads it, and the two
    /// disagree about damage. HarfBuzz takes a table whose curves are all
    /// there, pairing them with `fvar`'s axes by position however many each
    /// declares; FreeType takes only a table declaring exactly `fvar`'s axis
    /// count, and version 1.0 or 2.0 exactly. Where a library ignores the
    /// table, its reading here is `None` -- no correction, the same as no
    /// table at all.
    #[must_use]
    pub(crate) fn parse(data: &[u8], fvar: Span, avar: Option<Span>) -> Option<Self> {
        let (axes, limits, instances, instance_fixed) = parse_fvar(data, fvar)?;
        let table = avar.and_then(|span| data.get(span.off..span.off.checked_add(span.len)?));
        let hb_avar = table.and_then(|t| parse_hb_avar(t, axes.len()));
        let ft_avar = table.and_then(|t| parse_ft_avar(t, axes.len()));
        let has_v2 = hb_avar.as_ref().is_some_and(|a| a.v2.is_some())
            || ft_avar.as_ref().is_some_and(|a| a.v2.is_some());
        let avar = match table {
            Some(t) if has_v2 => t.to_vec(),
            _ => Vec::new(),
        };
        Some(Self {
            axes,
            limits,
            instances,
            instance_fixed,
            hb_avar,
            ft_avar,
            avar,
        })
    }

    /// The axes the face offers, in the order every variation table indexes by.
    #[must_use]
    pub fn axes(&self) -> &[Axis] {
        &self.axes
    }

    /// The positions the face has given names to.
    #[must_use]
    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    /// Whether the face carries an `avar` correction HarfBuzz reads.
    #[must_use]
    pub fn has_avar(&self) -> bool {
        self.hb_avar.is_some()
    }

    /// Index of the axis with this tag.
    #[must_use]
    pub fn axis_index(&self, tag: &[u8; 4]) -> Option<usize> {
        self.axes.iter().position(|a| &a.tag == tag)
    }

    /// The position where every axis sits at its default — normalized all-zero
    /// by construction, whatever the user-space defaults are.
    ///
    /// This is "no instance asked for", which both libraries read as zero
    /// everywhere; it is not necessarily what asking for every default
    /// normalizes to, since an `avar` may move the default (the specification
    /// forbids it, and HarfBuzz's `map_float` has an answer anyway).
    #[must_use]
    pub fn default_coords(&self) -> Coords {
        Coords {
            norm: alloc::vec![0i16; self.axes.len()],
            fixed: alloc::vec![0i32; self.axes.len()],
        }
    }

    /// Normalize a full set of user-space coordinates, one per axis in `fvar`
    /// order.
    ///
    /// A short slice leaves the remaining axes at their defaults and a long one
    /// is truncated, rather than either being an error: the caller that knows
    /// only about `wght` is the common one, and making it pad the vector for a
    /// face that also has `opsz` would push the face's own axis list into every
    /// call site.
    #[must_use]
    pub fn normalize(&self, user: &[f32]) -> Coords {
        self.coords_from(|i, _| user.get(i).copied(), None)
    }

    /// Normalize a position given as `(tag, value)` pairs, leaving unmentioned
    /// axes at their defaults.
    ///
    /// This is the shape a caller actually has — "weight 600" — and it is
    /// order-independent and tolerant of tags the face does not offer, which a
    /// positional slice cannot be. A tag the face does not have is ignored: a
    /// request for `wght` on a width-only face is not an error, it is a face
    /// that cannot honour it.
    #[must_use]
    pub fn normalize_tags(&self, requested: &[([u8; 4], f32)]) -> Coords {
        self.coords_from(
            |_, axis| {
                requested
                    .iter()
                    .rev()
                    .find(|(tag, _)| *tag == axis.tag)
                    .map(|&(_, v)| v)
            },
            None,
        )
    }

    /// Both normalizations of the position `value(index, axis)` gives each
    /// axis -- `None` for an axis nobody asked about -- or, for FreeType's,
    /// of the design coordinates `fixed` states exactly.
    ///
    /// An axis nobody asked about sits at its default in each library's own
    /// terms: HarfBuzz's is the default as a `float`, FreeType's the `Fixed`
    /// the file stores, exactly. The two can differ, since a `Fixed` has 32
    /// bits and a `float` 24, and FreeType handed the rounded one would
    /// normalize it to a hair off zero.
    fn coords_from(
        &self,
        value: impl Fn(usize, &Axis) -> Option<f32>,
        fixed: Option<&[i64]>,
    ) -> Coords {
        let values: Vec<Option<f32>> = self
            .axes
            .iter()
            .enumerate()
            .map(|(i, axis)| value(i, axis))
            .collect();
        let hb: Vec<f32> = self
            .axes
            .iter()
            .zip(&values)
            .map(|(axis, v)| v.unwrap_or(axis.default))
            .collect();
        let ft: Vec<Option<i64>> = match fixed {
            Some(fixed) => fixed.iter().copied().map(Some).collect(),
            None => values.iter().map(|v| v.and_then(design_fixed)).collect(),
        };
        Coords {
            norm: self.hb_coords(&hb),
            fixed: self.ft_coords(&ft),
        }
    }

    /// The normalized position of one of the face's named instances.
    ///
    /// Each library reads the instance record its own way: HarfBuzz as
    /// `float`s (`hb_ot_var_named_instance_get_design_coords`), FreeType as
    /// the `Fixed`s they are (`TT_Set_Named_Instance`).
    #[must_use]
    pub fn instance_coords(&self, index: usize) -> Option<Coords> {
        let instance = self.instances.get(index)?;
        let fixed = self.instance_fixed.get(index)?;
        Some(self.coords_from(|i, _| instance.coords.get(i).copied(), Some(fixed)))
    }

    /// `values`, one per axis, normalized as HarfBuzz 14.3.0's
    /// `hb_ot_var_normalize_coords` does it (see the module docs): to 16.16,
    /// through `avar`'s curves and version 2's store, then to `F2Dot14` by
    /// `(c + 2) >> 2`.
    ///
    /// The last step is held to what an `i16` carries. Only a curve that
    /// maps past the format's own ±2 can reach that, where HarfBuzz, which
    /// keeps coordinates in `int`s, would carry the excess on.
    fn hb_coords(&self, values: &[f32]) -> Vec<i16> {
        let mut coords: Vec<i32> = self
            .axes
            .iter()
            .zip(values)
            .map(|(axis, &value)| hb_normalize_axis(axis, value))
            .collect();
        if let Some(avar) = &self.hb_avar {
            for (c, curve) in coords.iter_mut().zip(&avar.curves) {
                *c = roundf_i32(map_float(curve, exact_f32(*c) / 65536.0) * 65536.0);
            }
            if let Some((map, store)) = &avar.v2 {
                coords = hb_avar2(&self.avar, map.as_ref(), store.as_ref(), &coords);
            }
        }
        coords
            .iter()
            .map(|&c| clamp_f2dot14(c.saturating_add(2).checked_shr(2).unwrap_or(0)))
            .collect()
    }

    /// `values`, one per axis, normalized as FreeType 2.13.2's
    /// `ft_var_to_normalized` does it (see the module docs), in 16.16.
    ///
    /// FreeType refuses an instance with a coordinate past ±1
    /// (`tt_set_mm_blend`), keeping whichever was set before -- which only a
    /// curve mapping past the ends of its axis can bring about. There is no
    /// answer of FreeType's to follow there, and the end of the axis is the
    /// nearest sensible one.
    fn ft_coords(&self, design: &[Option<i64>]) -> Vec<i32> {
        let mut coords: Vec<i64> = design
            .iter()
            .enumerate()
            .map(|(i, &c)| self.ft_normalize_axis(i, c))
            .collect();
        if let Some(avar) = &self.ft_avar {
            for (c, curve) in coords.iter_mut().zip(&avar.curves) {
                *c = ft_map(curve, *c);
            }
            if let Some((store, map)) = &avar.v2 {
                coords = ft_avar2(&self.avar, store, map.as_deref(), &coords);
            }
        }
        coords
            .iter()
            .map(|&c| {
                i32::try_from(c.clamp(i64::from(-FIXED_ONE), i64::from(FIXED_ONE))).unwrap_or(0)
            })
            .collect()
    }

    /// The 16.16 design coordinate `coord` normalized onto -1..1 as
    /// FreeType's `ft_var_to_normalized` does it, before `avar`: by
    /// `FT_DivFix` toward the nearer end of the axis -- a value at or past an
    /// end is that end exactly, which on a side of no width (a minimum at the
    /// default) is every value on that side. `None` is the default.
    fn ft_normalize_axis(&self, index: usize, coord: Option<i64>) -> i64 {
        use crate::ftcalc::div_fix;
        let Some(&[min, def, max]) = self.limits.get(index) else {
            return 0;
        };
        let coord = coord.unwrap_or(def);
        match coord.cmp(&def) {
            core::cmp::Ordering::Greater if coord >= max => i64::from(FIXED_ONE),
            core::cmp::Ordering::Greater => {
                div_fix(coord.saturating_sub(def), max.saturating_sub(def))
            }
            core::cmp::Ordering::Less if coord <= min => i64::from(-FIXED_ONE),
            core::cmp::Ordering::Less => {
                div_fix(coord.saturating_sub(def), def.saturating_sub(min))
            }
            core::cmp::Ordering::Equal => 0,
        }
    }
}

/// A user value as the 16.16 design coordinate FreeType is handed: to the
/// nearest 65536th, ties to even, as freetype-py converts one. `None` for
/// NaN, which is "nothing asked for".
fn design_fixed(value: f32) -> Option<i64> {
    if value.is_nan() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a design coordinate is a 16.16 `Fixed`; `as` saturates one \
                  past 2^63, which is clamped to the axis end in any case"
    )]
    Some((f64::from(value) * 65536.0).round_ties_even() as i64)
}

/// One user value normalized onto -1..1 as HarfBuzz 14.3.0's
/// `AxisRecord::normalize_axis_value` does it, in `f32`, then made 16.16 by
/// `roundf`.
///
/// HarfBuzz widens a range that leaves out its own default, rather than
/// trusting either end: the minimum is the lesser of the two, the maximum the
/// greater. A value is clamped into that range -- so a side of no width,
/// which several real faces have (`ReemKufi`'s `wght` is 400..400..700),
/// is 0 throughout, rather than a division by zero.
///
/// NaN is "nothing asked for", 0: it would otherwise slip past every
/// comparison. HarfBuzz itself would clamp it to the minimum.
#[allow(
    clippy::float_cmp,
    reason = "HarfBuzz's own test, and exact: a value clamped to the default \
              is the default"
)]
fn hb_normalize_axis(axis: &Axis, value: f32) -> i32 {
    if value.is_nan() {
        return 0;
    }
    let default = axis.default;
    let min = if default <= axis.min {
        default
    } else {
        axis.min
    };
    let max = if default >= axis.max {
        default
    } else {
        axis.max
    };
    let v = if value >= min { value } else { min };
    let v = if v <= max { v } else { max };
    let normalized = if v == default {
        0.0
    } else if v < default {
        (v - default) / (default - min)
    } else {
        (v - default) / (max - default)
    };
    roundf_i32(normalized * 65536.0)
}

/// HarfBuzz 14.3.0's `SegmentMaps::map_float`: `value` through one axis's
/// `avar` curve, in `f32`.
///
/// The specification wants each curve to map -1, 0 and 1 to themselves,
/// sorted and without repeats. HarfBuzz answers for the rest too, in ways
/// chosen to match what `CoreText` does with real fonts, and this answers as
/// HarfBuzz does:
///
/// * no points is the identity, and one point shifts every value by its
///   `to - from`;
/// * a doubled -1 (or +1) at the start (end) mapped to itself is skipped;
/// * a value equal to some point's `from` takes that point's `to` -- and
///   where a run of points share the `from`: the middle one of three, or
///   else the one nearer zero (the last of a negative run, the first of a
///   positive one), or at zero the one mapped nearer zero;
/// * any other value is interpolated between the first point past it and
///   the one before, or, beyond either end of the curve, shifted by that
///   end's `to - from`.
#[allow(
    clippy::float_cmp,
    reason = "HarfBuzz compares these exactly, and they are exact: F2Dot14s \
              made floats, and a coordinate that is a whole number of 65536ths"
)]
fn map_float(map: &[(i16, i16)], value: f32) -> f32 {
    let from = |k: usize| map.get(k).map_or(0.0, |&(f, _)| f2dot14_f32(f));
    let to = |k: usize| map.get(k).map_or(0.0, |&(_, t)| f2dot14_f32(t));
    let len = map.len();
    // From here on there are at least two points, so `len - 1` and `len - 2`
    // are indices; they are written saturating only because the crate's lints
    // want every subtraction spelled out.
    let (Some(last), Some(before_last)) = (len.checked_sub(1), len.checked_sub(2)) else {
        return match map.first() {
            None => value,
            Some(&(f, t)) => value - f2dot14_f32(f) + f2dot14_f32(t),
        };
    };
    let start = if from(0) == -1.0 && to(0) == -1.0 && from(1) == -1.0 {
        1
    } else {
        0
    };
    let end = if from(last) == 1.0 && to(last) == 1.0 && from(before_last) == 1.0 {
        last
    } else {
        len
    };
    if let Some(i) = (start..end).find(|&k| value == from(k)) {
        // The run of points sharing that `from`, i..=j.
        let j = (i..end)
            .take_while(|&k| from(k) == value)
            .last()
            .unwrap_or(i);
        if i.saturating_add(2) == j {
            return to(i.saturating_add(1));
        }
        // One point; or, of a run, the end nearer zero; or at zero the one
        // mapped nearer zero, the later on a tie.
        let first = i == j || value > 0.0 || (value == 0.0 && to(i).abs() < to(j).abs());
        return if first { to(i) } else { to(j) };
    }
    let i = (start..end).find(|&k| value < from(k)).unwrap_or(end);
    let Some(before) = i.checked_sub(1) else {
        return value - from(0) + to(0);
    };
    if i == end {
        return value - from(before) + to(before);
    }
    // `from(i) - from(before)` is not zero: `value` is at or past the one and
    // short of the other. (Only with the doubled -1 skipped and a value below
    // -1 could the two be equal, and a normalized value is never below -1.)
    let (before_from, before_to) = (from(before), to(before));
    let (after_from, after_to) = (from(i), to(i));
    before_to + ((after_to - before_to) * (value - before_from)) / (after_from - before_from)
}

/// `avar` version 2's step as HarfBuzz 14.3.0 takes it
/// (`avar::map_coords_16_16`), on the 16.16 coordinates the curves gave.
///
/// The store is read at those coordinates rounded to `F2Dot14` by `roundf`
/// -- not by the `(c + 2) >> 2` that ends the pipeline -- and every axis
/// reads it at the same place, the curves' output, not at the axes already
/// moved. Axis *i* reads the row the map gives it, (0, *i*) with no map, and
/// the rows are summed through one [`ScalarCache`], as HarfBuzz sums them.
/// Each delta (`F2Dot14`) is scaled to 16.16, held within ±2 so that the
/// conversion to an integer is defined, rounded, added, and the sum held to
/// -1..1.
fn hb_avar2(
    table: &[u8],
    map: Option<&IndexMap>,
    store: Option<&VarStore>,
    mapped: &[i32],
) -> Vec<i32> {
    let at: Vec<i32> = mapped
        .iter()
        .map(|&c| roundf_i32(exact_f32(c) / 4.0))
        .collect();
    let mut cache = store.map(|s| ScalarCache::new(s.region_count()));
    mapped
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let index = u32::try_from(i).unwrap_or(u32::MAX);
            let (outer, inner) = map
                .and_then(|m| m.get(table, index))
                .unwrap_or((0, u16::try_from(i).unwrap_or(u16::MAX)));
            let delta = store
                .and_then(|s| s.delta_with(table, outer, inner, &at, cache.as_mut()))
                .unwrap_or(0.0);
            let scaled = (delta * 4.0).clamp(-131_072.0, 131_072.0);
            c.saturating_add(roundf_i32(scaled))
                .clamp(-FIXED_ONE, FIXED_ONE)
        })
        .collect()
}

/// A 16.16 value through one axis's `avar` curve as FreeType's
/// `ft_var_to_normalized` takes it: by `FT_MulDiv` along the first segment
/// whose end is past the value, extended beyond its start if need be; a
/// value at or past the last point is left as it is, as it is by a curve
/// of fewer than two points.
fn ft_map(curve: &[(i16, i16)], value: i64) -> i64 {
    use crate::ftcalc::{f2dot14_to_fixed, mul_div};
    for w in curve.windows(2) {
        let (Some(&(from0, to0)), Some(&(from1, to1))) = (w.first(), w.get(1)) else {
            break;
        };
        let (from0, to0) = (f2dot14_to_fixed(from0), f2dot14_to_fixed(to0));
        let (from1, to1) = (f2dot14_to_fixed(from1), f2dot14_to_fixed(to1));
        if value < from1 {
            return mul_div(
                value.saturating_sub(from0),
                to1.saturating_sub(to0),
                from1.saturating_sub(from0),
            )
            .saturating_add(to0);
        }
    }
    value
}

/// `avar` version 2's step as FreeType 2.13.2 takes it
/// (`ft_var_to_normalized`), on the 16.16 coordinates the curves gave: the
/// store read at those coordinates as they stand, axis *i* reading the row
/// the map gives it -- the map's last for an axis past its end, (0, *i*)
/// with no map -- and its delta (`F2Dot14`) added as `delta << 2` in a 32-bit
/// `int`, the sum held to -1..1.
fn ft_avar2(
    table: &[u8],
    store: &FtItemStore,
    map: Option<&[(u16, u16)]>,
    mapped: &[i64],
) -> Vec<i64> {
    let at: Vec<i32> = mapped
        .iter()
        .map(|&c| i32::try_from(c).unwrap_or(if c < 0 { i32::MIN } else { i32::MAX }))
        .collect();
    mapped
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let (outer, inner) = match map {
                Some(map) => map
                    .get(i.min(map.len().saturating_sub(1)))
                    .copied()
                    .unwrap_or((0, 0)),
                None => (0, u16::try_from(i).unwrap_or(u16::MAX)),
            };
            let delta = store.delta(table, outer, inner, &at);
            c.saturating_add(i64::from(delta.wrapping_shl(2)))
                .clamp(i64::from(-FIXED_ONE), i64::from(FIXED_ONE))
        })
        .collect()
}

/// An `F2Dot14` as the `f32` it stands for, exactly.
fn f2dot14_f32(v: i16) -> f32 {
    f32::from(v) / 16384.0
}

/// An `i32` as an `f32`: exact for every value this module converts, which
/// are 16.16 coordinates within a few units of zero.
fn exact_f32(v: i32) -> f32 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "exact below 2^24, and a coordinate here is at most a few \
                  times 2^16"
    )]
    {
        v as f32
    }
}

/// Clamp to `F2Dot14`'s representable -2..2 and narrow to `i16`.
///
/// Normalized coordinates are defined on -1..=1, so anything outside is
/// either a malformed `avar` or arithmetic that has gone wrong; clamping to
/// the *format's* range rather than to -1..1 keeps a face that deliberately
/// maps past the nominal end working, while still guaranteeing the narrowing
/// cannot wrap.
fn clamp_f2dot14(v: i32) -> i16 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "clamped to i16's range on the line above"
    )]
    {
        v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    }
}

/// Read a `Fixed` (16.16) at `off`, as the `f32` HarfBuzz's `to_float` makes
/// of it: the exact value rounded once to `f32`.
fn fixed_at(data: &[u8], off: usize) -> Option<f32> {
    let hi = i16_at(data, off)?;
    let lo = u16_at(data, off.checked_add(2)?)?;
    Some(f32::from(hi) + f32::from(lo) / 65536.0)
}

/// Read a `Fixed` (16.16) at `off`, as its raw 32 bits.
fn raw_fixed_at(data: &[u8], off: usize) -> Option<i64> {
    let hi = i16_at(data, off)?;
    let lo = u16_at(data, off.checked_add(2)?)?;
    Some(i64::from(hi) << 16 | i64::from(lo))
}

/// An axis's limits and instances as `fvar` holds them, and the instances'
/// coordinates as the `Fixed`s they are.
type Fvar = (Vec<Axis>, Vec<[i64; 3]>, Vec<Instance>, Vec<Vec<i64>>);

/// Read `fvar` into its axes -- with their limits as FreeType holds them,
/// 16.16 -- and named instances.
fn parse_fvar(data: &[u8], span: Span) -> Option<Fvar> {
    let table = data.get(span.off..span.off.checked_add(span.len)?)?;
    if u16_at(table, 0)? != 1 {
        // Only major version 1 exists. A future one may move the fields this
        // reads, so decline rather than read them from the wrong offsets.
        return None;
    }
    let axes_off = usize::from(u16_at(table, 4)?);
    let axis_count = usize::from(u16_at(table, 8)?);
    let axis_size = usize::from(u16_at(table, 10)?);
    let instance_count = usize::from(u16_at(table, 12)?);
    let instance_size = usize::from(u16_at(table, 14)?);

    if axis_count == 0 || axis_count > MAX_AXES || axis_size < AXIS_RECORD_MIN {
        return None;
    }

    let mut axes = Vec::with_capacity(axis_count);
    let mut limits = Vec::with_capacity(axis_count);
    for i in 0..axis_count {
        let rec = axes_off.checked_add(i.checked_mul(axis_size)?)?;
        let [min, def, max] = [
            raw_fixed_at(table, rec.checked_add(4)?)?,
            raw_fixed_at(table, rec.checked_add(8)?)?,
            raw_fixed_at(table, rec.checked_add(12)?)?,
        ];
        // FreeType pins an axis whose default is outside its own range to
        // the default at both ends -- after which every value above the
        // default normalizes to +1 and every value below to -1.
        limits.push(if min > def || def > max {
            [def, def, def]
        } else {
            [min, def, max]
        });
        axes.push(Axis {
            tag: tag_at(table, rec)?,
            min: fixed_at(table, rec.checked_add(4)?)?,
            default: fixed_at(table, rec.checked_add(8)?)?,
            max: fixed_at(table, rec.checked_add(12)?)?,
            hidden: u16_at(table, rec.checked_add(16)?)? & AXIS_HIDDEN != 0,
            name_id: u16_at(table, rec.checked_add(18)?)?,
        });
    }

    // The instance array follows the axis array. An instance record is a name
    // id, a flags word, one `Fixed` per axis, and optionally a PostScript name
    // id — so a face that declares a size smaller than that is describing
    // records it did not write, and the whole instance list is dropped rather
    // than read at a stride that would walk off into the axis coordinates.
    let coords_bytes = axis_count.checked_mul(4)?;
    let min_instance = coords_bytes.checked_add(4)?;
    let has_postscript = instance_size >= min_instance.checked_add(2)?;
    let mut instance_fixed = Vec::new();
    let instances = if instance_size < min_instance || instance_count > MAX_INSTANCES {
        Vec::new()
    } else {
        let base = axes_off.checked_add(axis_count.checked_mul(axis_size)?)?;
        let mut out = Vec::with_capacity(instance_count);
        for i in 0..instance_count {
            let Some(rec) = base
                .checked_add(i.saturating_mul(instance_size))
                .filter(|r| {
                    r.checked_add(min_instance)
                        .is_some_and(|e| e <= table.len())
                })
            else {
                // A truncated instance array costs the instances past the cut,
                // not the axes: the axes are what drawing needs.
                break;
            };
            let mut coords = Vec::with_capacity(axis_count);
            let mut fixed = Vec::with_capacity(axis_count);
            for a in 0..axis_count {
                let at = rec
                    .checked_add(4)
                    .and_then(|o| o.checked_add(a.checked_mul(4)?));
                let (Some(v), Some(raw)) = (
                    at.and_then(|o| fixed_at(table, o)),
                    at.and_then(|o| raw_fixed_at(table, o)),
                ) else {
                    break;
                };
                coords.push(v);
                fixed.push(raw);
            }
            if coords.len() != axis_count {
                break;
            }
            instance_fixed.push(fixed);
            out.push(Instance {
                subfamily_name_id: u16_at(table, rec)?,
                postscript_name_id: if has_postscript {
                    rec.checked_add(min_instance).and_then(|o| u16_at(table, o))
                } else {
                    None
                },
                coords,
            });
        }
        out
    };

    Some((axes, limits, instances, instance_fixed))
}

/// Read `avar` as HarfBuzz 14.3.0 does (`OT::avar::sanitize`).
///
/// `None` where HarfBuzz's sanitizer rejects the table and so ignores it
/// whole: a major version other than 1 or 2, or a curve -- or version 2's
/// two offsets after the curves -- running past the end. A version-2 part
/// that is unreadable in itself is dropped on its own, as the sanitizer
/// zeroes the offset to it.
fn parse_hb_avar(table: &[u8], axis_count: usize) -> Option<HbAvar> {
    let major = u16_at(table, 0)?;
    if !matches!(major, 1 | 2) {
        return None;
    }
    let declared = usize::from(u16_at(table, 6)?);
    let mut curves = Vec::with_capacity(declared.min(axis_count));
    let mut pos = 8usize;
    for i in 0..declared {
        let count = usize::from(u16_at(table, pos)?);
        pos = pos.checked_add(2)?;
        let end = pos.checked_add(count.checked_mul(4)?)?;
        if end > table.len() {
            return None;
        }
        if i < axis_count {
            let mut points = Vec::with_capacity(count);
            let mut at = pos;
            while at < end {
                points.push((i16_at(table, at)?, i16_at(table, at.checked_add(2)?)?));
                at = at.checked_add(4)?;
            }
            curves.push(points);
        }
        pos = end;
    }
    let v2 = if major == 2 {
        let offset =
            |at: usize| u32_at(table, at).map(|o| usize::try_from(o).ok().filter(|&o| o != 0));
        let map = offset(pos)?.and_then(|o| IndexMap::parse(table, o));
        let store =
            offset(pos.checked_add(4)?)?.and_then(|o| VarStore::parse(table, o, axis_count));
        Some((map, store))
    } else {
        None
    };
    Some(HbAvar { curves, v2 })
}

/// Read `avar` as FreeType 2.13.2's `ft_var_load_avar` does.
///
/// `None` where FreeType ignores the table: a version other than exactly 1.0
/// or 2.0, an axis count (read as 32 bits, the reserved word included) other
/// than `fvar`'s, or a curve with more pairs than the table has bytes for.
/// Version 2's part is FreeType's reading of it, and absent when FreeType
/// ends up with no store.
fn parse_ft_avar(table: &[u8], axis_count: usize) -> Option<FtAvar> {
    let version = u32_at(table, 0)?;
    if version != 0x0001_0000 && version != 0x0002_0000 {
        return None;
    }
    if usize::try_from(u32_at(table, 4)?).ok()? != axis_count {
        return None;
    }
    let mut curves = Vec::with_capacity(axis_count);
    let mut pos = 8usize;
    for _ in 0..axis_count {
        let count = usize::from(u16_at(table, pos)?);
        pos = pos.checked_add(2)?;
        // FreeType's own bound: four bytes a pair within the table.
        if count.checked_mul(4)? > table.len() {
            return None;
        }
        let mut pairs = Vec::with_capacity(count);
        for _ in 0..count {
            pairs.push((i16_at(table, pos)?, i16_at(table, pos.checked_add(2)?)?));
            pos = pos.checked_add(4)?;
        }
        curves.push(pairs);
    }
    let v2 = if version == 0x0002_0000 {
        parse_ft_avar2(table, pos, axis_count)
    } else {
        None
    };
    Some(FtAvar { curves, v2 })
}

/// Version 2's part of `avar`, at `tail`, as FreeType loads it: the store if
/// its offset is not zero and FreeType is left with one, and then -- only
/// after a store that loaded without error -- the axis map.
fn parse_ft_avar2(table: &[u8], tail: usize, axis_count: usize) -> Option<FtAvar2> {
    let map_at = usize::try_from(u32_at(table, tail)?).ok()?;
    let store_at = usize::try_from(u32_at(table, tail.checked_add(4)?)?).ok()?;
    if store_at == 0 {
        return None;
    }
    let (store, clean) = FtItemStore::parse(table, store_at, axis_count)?;
    let map = if clean && map_at != 0 {
        store.index_map(table, map_at, table.len())
    } else {
        None
    };
    Some((store, map))
}

/// Convenience for the common single-axis request.
///
/// Returns the tag as the four bytes every OpenType table spells it with,
/// so call sites read as the specification does rather than as string literals
/// that could be mistyped by a byte.
pub mod tags {
    /// Weight.
    pub const WGHT: [u8; 4] = *b"wght";
    /// Width.
    pub const WDTH: [u8; 4] = *b"wdth";
    /// Optical size.
    pub const OPSZ: [u8; 4] = *b"opsz";
    /// Italic (a 0/1 switch, not a slant angle).
    pub const ITAL: [u8; 4] = *b"ital";
    /// Slant, in counter-clockwise degrees.
    pub const SLNT: [u8; 4] = *b"slnt";
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did
    // it — that is the diagnosis. The defensive lints exist to keep panics out
    // of code that runs on a user's data, which this is not.
    //
    // `float_cmp` is allowed for a reason specific to this module rather than
    // the general test-code one: exactness is the property under test. The
    // normalized space is defined so that the default is *0*, the ends are
    // *±1*, and the half-way points on a 300..400..700 axis are *±0.5* — every
    // one of those is exactly representable in binary, and every one of them
    // is later multiplied out to an integer. An approximate comparison here
    // would pass on a normalization that returned 0.9999997 for the maximum,
    // which rounds a unit short and leaves the face short of the weight the
    // user asked for, forever.
    #![allow(
        clippy::float_cmp,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;
    use crate::var_fixture::{
        AVAR2, AVAR2_EXPECTED, CURVES, CURVES_EXPECTED, Expected, LIMITS, LIMITS_EXPECTED,
    };

    /// `F2Dot14` for 1.0.
    const ONE: i16 = 16384;

    fn axis(tag: &[u8; 4], min: f32, default: f32, max: f32) -> Axis {
        Axis {
            tag: *tag,
            min,
            default,
            max,
            hidden: false,
            name_id: 0,
        }
    }

    /// A face's variations built directly: these axes, and `curves` as the
    /// `avar` both libraries read (none when empty).
    fn vars(axes: Vec<Axis>, curves: Vec<Vec<(i16, i16)>>) -> Variations {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "test axes are small user values"
        )]
        let raw = |v: f32| (f64::from(v) * 65536.0) as i64;
        let limits = axes
            .iter()
            .map(|a| [raw(a.min), raw(a.default), raw(a.max)])
            .collect();
        let (hb_avar, ft_avar) = if curves.is_empty() {
            (None, None)
        } else {
            (
                Some(HbAvar {
                    curves: curves.clone(),
                    v2: None,
                }),
                Some(FtAvar { curves, v2: None }),
            )
        };
        Variations {
            axes,
            limits,
            instances: Vec::new(),
            instance_fixed: Vec::new(),
            hb_avar,
            ft_avar,
            avar: Vec::new(),
        }
    }

    fn wght(min: f32, default: f32, max: f32) -> Vec<Axis> {
        alloc::vec![axis(b"wght", min, default, max)]
    }

    // --- against the libraries themselves ---

    /// Every instance `gen_var_fixture.py` asked HarfBuzz and FreeType about,
    /// asked here: both readings must be the libraries' own, to the unit.
    fn check_fixture<const N: usize>(name: &str, font: &[u8], expected: &[Expected<N>]) {
        let face = crate::sfnt::Face::parse(font.to_vec()).unwrap();
        let v = face.variation_axes().unwrap();
        let mut wrong = Vec::new();
        for (values, harfbuzz, freetype) in expected {
            let c = v.normalize(values);
            if c.as_slice() != harfbuzz {
                wrong.push(alloc::format!(
                    "{values:?}: HarfBuzz {harfbuzz:?}, here {:?}",
                    c.as_slice()
                ));
            }
            if let Some(freetype) = freetype
                && c.fixed() != freetype
            {
                wrong.push(alloc::format!(
                    "{values:?}: FreeType {freetype:?}, here {:?}",
                    c.fixed()
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "{name}: {} of {} instances disagree:\n{}",
            wrong.len(),
            expected.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn avar_version_two_moves_each_axis_as_both_libraries_move_it() {
        check_fixture("AVAR2", &AVAR2, &AVAR2_EXPECTED);
    }

    #[test]
    fn every_avar_curve_rule_gives_each_librarys_own_answer() {
        check_fixture("CURVES", &CURVES, &CURVES_EXPECTED);
    }

    #[test]
    fn awkward_and_malformed_axis_limits_normalize_as_each_library_repairs_them() {
        check_fixture("LIMITS", &LIMITS, &LIMITS_EXPECTED);
    }

    #[test]
    fn the_fixture_faces_reach_what_they_were_built_to_reach() {
        // A fixture that stopped reaching its rules -- regenerated from a
        // face that lost its `avar`, say -- would pass the tests above
        // vacuously.
        let parse = |font: &[u8]| {
            let face = crate::sfnt::Face::parse(font.to_vec()).unwrap();
            face.variation_axes().unwrap().clone()
        };
        let v2 = parse(&AVAR2);
        assert!(v2.hb_avar.as_ref().unwrap().v2.is_some());
        assert!(v2.ft_avar.as_ref().unwrap().v2.is_some());
        assert!(!v2.avar.is_empty());
        let curves = parse(&CURVES);
        assert_eq!(curves.hb_avar.as_ref().unwrap().curves.len(), 9);
        assert_eq!(curves.ft_avar.as_ref().unwrap().curves.len(), 9);
        assert!(!parse(&LIMITS).has_avar());
        // And the two libraries disagree somewhere on each, or the second
        // reading is not being tested at all.
        for (name, rows) in [
            ("AVAR2", disagreements(&AVAR2_EXPECTED)),
            ("CURVES", disagreements(&CURVES_EXPECTED)),
            ("LIMITS", disagreements(&LIMITS_EXPECTED)),
        ] {
            assert!(rows > 0, "{name}: HarfBuzz and FreeType agree everywhere");
        }
    }

    /// Instances where FreeType's 16.16 is not HarfBuzz's `F2Dot14` times 4.
    fn disagreements<const N: usize>(rows: &[Expected<N>]) -> usize {
        rows.iter()
            .filter(|(_, hb, ft)| {
                ft.is_some_and(|ft| hb.iter().zip(&ft).any(|(&h, &f)| i32::from(h) * 4 != f))
            })
            .count()
    }

    // --- HarfBuzz's pipeline, rule by rule ---

    #[test]
    fn harfbuzz_rounds_to_sixteen_sixteen_before_it_rounds_to_f2dot14() {
        let v = vars(wght(100.0, 400.0, 900.0), Vec::new());
        // 0.6 is 9830.4 F2Dot14 units, which one rounding makes 9830. HarfBuzz
        // makes it 39321.6 -> 39322 in 16.16 first, and 39322 is 9830.5 --
        // which the second rounding takes up.
        let c = v.normalize_tags(&[(*b"wght", 700.0)]);
        assert_eq!(c.as_slice(), &[9831]);
        assert_eq!(c.fixed(), &[39322]);
    }

    #[test]
    fn harfbuzz_rounds_a_half_up_rather_than_away_from_zero() {
        // A slant axis of -4..0..4 asked for -40/65536: normalized, -10/65536
        // exactly, which is -2.5 F2Dot14 units. `(c + 2) >> 2` makes that -2
        // where `roundf` would make it -3; the positive half goes up either
        // way.
        let v = vars(alloc::vec![axis(b"slnt", -4.0, 0.0, 4.0)], Vec::new());
        assert_eq!(v.normalize(&[-40.0 / 65536.0]).as_slice(), &[-2]);
        assert_eq!(v.normalize(&[40.0 / 65536.0]).as_slice(), &[3]);
    }

    #[test]
    fn default_normalizes_to_zero_whatever_the_user_scale() {
        // The point of the normalized space: 400 on a 300..400..700 axis and
        // 10.5 on a 5..10.5..36 axis are both exactly 0.
        assert_eq!(
            hb_normalize_axis(&axis(b"wght", 300.0, 400.0, 700.0), 400.0),
            0
        );
        assert_eq!(hb_normalize_axis(&axis(b"opsz", 5.0, 10.5, 36.0), 10.5), 0);
    }

    #[test]
    fn the_two_sides_have_independent_scales() {
        // 300..400..700: one step down covers 100 units, one step up covers
        // 300. Half way *down* is 350, half way *up* is 550 -- a linear map
        // over the whole range would put both at the wrong place.
        let a = axis(b"wght", 300.0, 400.0, 700.0);
        assert_eq!(hb_normalize_axis(&a, 350.0), -0x8000);
        assert_eq!(hb_normalize_axis(&a, 550.0), 0x8000);
        assert_eq!(hb_normalize_axis(&a, 300.0), -0x1_0000);
        assert_eq!(hb_normalize_axis(&a, 700.0), 0x1_0000);
    }

    #[test]
    fn values_outside_the_range_clamp_rather_than_extrapolate() {
        let a = axis(b"wght", 300.0, 400.0, 700.0);
        assert_eq!(hb_normalize_axis(&a, 50.0), -0x1_0000);
        assert_eq!(hb_normalize_axis(&a, 5000.0), 0x1_0000);
        assert_eq!(hb_normalize_axis(&a, f32::INFINITY), 0x1_0000);
        assert_eq!(hb_normalize_axis(&a, f32::NEG_INFINITY), -0x1_0000);
    }

    #[test]
    fn a_degenerate_side_yields_zero_rather_than_an_infinity() {
        // ReemKufi really ships this: wght 400..400..700, so the whole
        // below-default side has zero width.
        let a = axis(b"wght", 400.0, 400.0, 700.0);
        assert_eq!(hb_normalize_axis(&a, 400.0), 0);
        assert_eq!(hb_normalize_axis(&a, 300.0), 0, "clamped to the default");
        assert_eq!(hb_normalize_axis(&a, 700.0), 0x1_0000);
        let flat = axis(b"wght", 400.0, 400.0, 400.0);
        for value in [0.0, 400.0, 1000.0, f32::MAX] {
            assert_eq!(hb_normalize_axis(&flat, value), 0);
        }
    }

    #[test]
    fn harfbuzz_widens_a_range_that_leaves_out_its_default() {
        // A minimum above the default: HarfBuzz takes the default as the
        // minimum, so the lower side is empty and the upper one runs from the
        // default, not from the stated minimum.
        let a = axis(b"wght", 500.0, 400.0, 900.0);
        assert_eq!(hb_normalize_axis(&a, 300.0), 0);
        assert_eq!(hb_normalize_axis(&a, 650.0), 0x8000);
    }

    #[test]
    fn nan_is_treated_as_no_request() {
        let v = vars(wght(300.0, 400.0, 700.0), Vec::new());
        let c = v.normalize(&[f32::NAN]);
        assert_eq!(c.as_slice(), &[0]);
        assert_eq!(c.fixed(), &[0]);
    }

    // --- map_float ---

    /// A curve from points given as fractions of 1.
    fn curve(points: &[(f32, f32)]) -> Vec<(i16, i16)> {
        #[allow(clippy::cast_possible_truncation, reason = "test fractions")]
        let f = |x: f32| (x * 16384.0).round() as i16;
        points.iter().map(|&(a, b)| (f(a), f(b))).collect()
    }

    #[test]
    fn an_identity_curve_changes_nothing() {
        let map = curve(&[(-1.0, -1.0), (0.0, 0.0), (1.0, 1.0)]);
        for v in [-1.0, -0.5, 0.0, 0.25, 1.0] {
            assert_eq!(map_float(&map, v), v);
        }
    }

    #[test]
    fn a_curve_bends_the_middle_of_the_axis() {
        // The real shape: a face whose Semibold is not half way between
        // Regular and Bold says so by moving the midpoint.
        let map = curve(&[(-1.0, -1.0), (0.0, 0.0), (0.5, 0.25), (1.0, 1.0)]);
        assert_eq!(map_float(&map, 0.0), 0.0, "the default stays the default");
        assert_eq!(map_float(&map, 0.5), 0.25);
        assert_eq!(map_float(&map, 0.25), 0.125);
        assert_eq!(map_float(&map, 0.75), 0.625);
    }

    #[test]
    fn a_curve_too_short_to_interpolate_is_the_identity_or_a_shift() {
        assert_eq!(map_float(&[], 0.3), 0.3);
        // One point moves everything by its own displacement.
        assert_eq!(map_float(&curve(&[(0.25, 0.5)]), 0.5), 0.75);
    }

    #[test]
    fn beyond_its_ends_a_curve_shifts_rather_than_clamps() {
        let map = curve(&[(-0.5, -0.25), (0.0, 0.0), (0.5, 0.75)]);
        assert_eq!(map_float(&map, -1.0), -0.75);
        assert_eq!(map_float(&map, 1.0), 1.25);
    }

    #[test]
    fn repeated_points_answer_as_harfbuzz_chose_to() {
        // Two: the one nearer zero -- the first of a positive run, the last of
        // a negative one.
        let two = curve(&[
            (-1.0, -1.0),
            (-0.5, -0.3),
            (-0.5, -0.7),
            (0.0, 0.0),
            (0.5, 0.25),
            (0.5, 0.75),
            (1.0, 1.0),
        ]);
        assert_eq!(map_float(&two, 0.5), 0.25);
        assert_eq!(
            map_float(&two, -0.5),
            map_float(&curve(&[(-0.5, -0.7)]), -0.5)
        );
        // Three: the middle one.
        let three = curve(&[
            (-1.0, -1.0),
            (0.0, 0.0),
            (0.5, 0.25),
            (0.5, 0.5),
            (0.5, 0.75),
            (1.0, 1.0),
        ]);
        assert_eq!(map_float(&three, 0.5), 0.5);
        // Four at zero: the one mapped nearer zero.
        let four = curve(&[
            (-1.0, -1.0),
            (0.0, -0.125),
            (0.0, 0.0625),
            (0.0, 0.25),
            (0.0, 0.375),
            (1.0, 1.0),
        ]);
        assert_eq!(map_float(&four, 0.0), -0.125);
    }

    #[test]
    fn doubled_ends_mapped_to_themselves_are_skipped() {
        let map = curve(&[
            (-1.0, -1.0),
            (-1.0, -0.5),
            (0.0, 0.0),
            (1.0, 0.5),
            (1.0, 1.0),
        ]);
        assert_eq!(map_float(&map, -1.0), -0.5);
        assert_eq!(map_float(&map, 1.0), 0.5);
    }

    // --- FreeType's pipeline ---

    #[test]
    fn freetype_normalizes_in_sixteen_sixteen_without_rounding_to_f2dot14() {
        let v = vars(wght(100.0, 400.0, 900.0), Vec::new());
        // FT_DivFix(300, 500).
        assert_eq!(v.normalize_tags(&[(*b"wght", 700.0)]).fixed(), &[39322]);
        // The ends exactly, and past them.
        assert_eq!(v.normalize_tags(&[(*b"wght", 900.0)]).fixed(), &[0x10000]);
        assert_eq!(v.normalize_tags(&[(*b"wght", 50.0)]).fixed(), &[-0x10000]);
        assert!(v.default_coords().is_default_fixed());
    }

    #[test]
    fn freetype_applies_avar_by_mul_div_from_the_first_segment_past_the_value() {
        // -1 -> -1, 0 -> 0, 0.5 -> 0.75, 1 -> 1.
        let map = alloc::vec![(-ONE, -ONE), (0, 0), (8192, 12288), (ONE, ONE)];
        let v = vars(wght(100.0, 400.0, 900.0), alloc::vec![map]);
        // 650 is 0.5 of the way up: FT_DivFix gives 0x8000, which the third
        // point's `from` (0.5) does not exceed, so the fourth maps it --
        // 0.75 + (0.5 - 0.5) * ... = 0.75 exactly.
        assert_eq!(v.normalize_tags(&[(*b"wght", 650.0)]).fixed(), &[0xC000]);
        // 525 is 0.25 up: the third segment, 0.25 * 0.75 / 0.5 = 0.375.
        assert_eq!(v.normalize_tags(&[(*b"wght", 525.0)]).fixed(), &[0x6000]);
    }

    #[test]
    fn freetype_leaves_a_value_past_the_last_point_as_it_is() {
        let v = vars(
            wght(0.0, 100.0, 200.0),
            alloc::vec![curve(&[(-0.5, -0.25), (0.0, 0.0), (0.5, 0.75)])],
        );
        // 0.75 up: past the last point, so unchanged -- where HarfBuzz shifts
        // it by that point's displacement.
        let c = v.normalize(&[175.0]);
        assert_eq!(c.fixed(), &[0xC000]);
        assert_eq!(c.as_slice(), &[16384]);
    }

    #[test]
    fn freetype_pins_an_axis_whose_default_is_out_of_range() {
        // `fvar` says 500..400..900. FreeType pins both ends to 400, after
        // which anything above the default is +1 and anything below -1.
        let mut v = vars(alloc::vec![axis(b"wght", 500.0, 400.0, 900.0)], Vec::new());
        let raw = |x: i64| x << 16;
        v.limits = alloc::vec![[raw(400), raw(400), raw(400)]];
        assert_eq!(v.normalize(&[401.0]).fixed(), &[0x10000]);
        assert_eq!(v.normalize(&[399.0]).fixed(), &[-0x10000]);
        assert_eq!(v.normalize(&[400.0]).fixed(), &[0]);
    }

    #[test]
    fn an_axis_nobody_names_is_at_freetypes_exact_default() {
        // A default of 1000 + 1/65536 needs 26 bits, which an `f32` does not
        // have: as a float it is 1000. FreeType, told nothing about the axis
        // -- or told the instance record's `Fixed` -- sits at the exact
        // default, 0; handed the float, it would sit one 65536th below it.
        let def = (1000 << 16) + 1;
        let mut v = vars(
            alloc::vec![axis(b"wght", 999.0, 1000.0, 1001.0)],
            Vec::new(),
        );
        v.limits = alloc::vec![[def - 0x1_0000, def, def + 0x1_0000]];
        assert_eq!(v.normalize(&[]).fixed(), &[0]);
        assert_eq!(v.normalize_tags(&[]).fixed(), &[0]);
        assert_eq!(v.normalize(&[1000.0]).fixed(), &[-1]);
        // HarfBuzz's default is the float, so both are 0 there.
        assert_eq!(v.normalize(&[]).as_slice(), &[0]);
        assert_eq!(v.normalize(&[1000.0]).as_slice(), &[0]);
        // A named instance at the default: FreeType takes its `Fixed`.
        v.instances = alloc::vec![Instance {
            subfamily_name_id: 0,
            postscript_name_id: None,
            coords: alloc::vec![1000.0],
        }];
        v.instance_fixed = alloc::vec![alloc::vec![def]];
        assert_eq!(v.instance_coords(0).unwrap().fixed(), &[0]);
    }

    // --- the table readers ---

    /// An `avar` of one axis with this curve, version `major.minor`, the
    /// reserved word `reserved`.
    fn avar_bytes(
        major: u16,
        minor: u16,
        reserved: u16,
        axes: u16,
        curve: &[(i16, i16)],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        for w in [major, minor, reserved, axes] {
            out.extend_from_slice(&w.to_be_bytes());
        }
        for _ in 0..axes {
            out.extend_from_slice(&u16::try_from(curve.len()).unwrap().to_be_bytes());
            for &(f, t) in curve {
                out.extend_from_slice(&f.to_be_bytes());
                out.extend_from_slice(&t.to_be_bytes());
            }
        }
        out
    }

    #[test]
    fn the_libraries_disagree_about_which_avar_headers_to_accept() {
        let map = [(-ONE, -ONE), (0, 0), (8192, 4096), (ONE, ONE)];
        // Well formed: both read it.
        let good = avar_bytes(1, 0, 0, 1, &map);
        assert!(parse_hb_avar(&good, 1).is_some());
        assert!(parse_ft_avar(&good, 1).is_some());
        // A minor version, or a reserved word, that is not zero: FreeType
        // compares both as 32-bit words and ignores the table; HarfBuzz reads
        // only the major version and the axis count.
        for odd in [avar_bytes(1, 1, 0, 1, &map), avar_bytes(1, 0, 7, 1, &map)] {
            assert!(parse_hb_avar(&odd, 1).is_some());
            assert!(parse_ft_avar(&odd, 1).is_none());
        }
        // An axis count other than `fvar`'s: FreeType ignores the table,
        // HarfBuzz pairs the curves it has with the axes it has.
        let two = avar_bytes(1, 0, 0, 2, &map);
        assert!(parse_ft_avar(&two, 1).is_none());
        assert_eq!(parse_hb_avar(&two, 1).unwrap().curves.len(), 1);
        assert_eq!(parse_hb_avar(&good, 2).unwrap().curves.len(), 1);
        // A curve running off the end: HarfBuzz rejects the whole table.
        assert!(parse_hb_avar(&good[..good.len() - 1], 1).is_none());
        // A version neither knows.
        assert!(parse_hb_avar(&avar_bytes(3, 0, 0, 1, &map), 1).is_none());
        assert!(parse_ft_avar(&avar_bytes(3, 0, 0, 1, &map), 1).is_none());
    }

    #[test]
    fn a_version_two_avar_without_its_tail_is_rejected_by_harfbuzz_and_kept_by_freetype() {
        let map = [(-ONE, -ONE), (0, 0), (ONE, ONE)];
        let bytes = avar_bytes(2, 0, 0, 1, &map);
        // HarfBuzz's sanitizer needs the eight bytes of offsets after the
        // curves; FreeType keeps the curves and has no store.
        assert!(parse_hb_avar(&bytes, 1).is_none());
        let ft = parse_ft_avar(&bytes, 1).unwrap();
        assert_eq!(ft.curves.len(), 1);
        assert!(ft.v2.is_none());
        // With the offsets both zero: no map and no store -- which HarfBuzz
        // still runs its step over, so a curve's value past ±1 is held to it.
        let mut with_tail = bytes;
        with_tail.extend_from_slice(&[0; 8]);
        let hb = parse_hb_avar(&with_tail, 1).unwrap();
        assert_eq!(hb.v2, Some((None, None)));
        assert!(parse_ft_avar(&with_tail, 1).unwrap().v2.is_none());
        assert_eq!(
            hb_avar2(&with_tail, None, None, &[0x1_2000, -3]),
            [0x1_0000, -3]
        );
    }

    // --- the tag-keyed entry point ---

    #[test]
    fn default_coords_are_zero_and_report_as_default() {
        let v = vars(
            alloc::vec![
                axis(b"wght", 300.0, 400.0, 700.0),
                axis(b"opsz", 5.0, 10.5, 36.0)
            ],
            Vec::new(),
        );
        let c = v.default_coords();
        assert_eq!(c.as_slice(), &[0, 0]);
        assert!(c.is_default());
        assert!(!v.normalize(&[700.0, 10.5]).is_default());
    }

    #[test]
    fn unmentioned_axes_stay_at_their_defaults() {
        let v = vars(
            alloc::vec![
                axis(b"wght", 300.0, 400.0, 700.0),
                axis(b"opsz", 5.0, 10.5, 36.0)
            ],
            Vec::new(),
        );
        // Asking only for weight must not move optical size.
        let c = v.normalize_tags(&[(tags::WGHT, 700.0)]);
        assert_eq!(c.as_slice(), &[16384, 0]);
    }

    #[test]
    fn tags_are_matched_by_name_not_by_position() {
        let v = vars(
            alloc::vec![
                axis(b"opsz", 5.0, 10.5, 36.0),
                axis(b"wght", 300.0, 400.0, 700.0)
            ],
            Vec::new(),
        );
        // Same request, axes declared the other way round: the weight must
        // still land on the weight axis.
        let c = v.normalize_tags(&[(tags::WGHT, 700.0)]);
        assert_eq!(c.as_slice(), &[0, 16384]);
    }

    #[test]
    fn a_tag_the_face_does_not_have_is_ignored() {
        let v = vars(wght(300.0, 400.0, 700.0), Vec::new());
        let c = v.normalize_tags(&[(tags::WDTH, 50.0), (tags::WGHT, 700.0)]);
        assert_eq!(c.as_slice(), &[16384]);
    }

    #[test]
    fn a_repeated_tag_takes_the_last_request() {
        let v = vars(wght(300.0, 400.0, 700.0), Vec::new());
        let c = v.normalize_tags(&[(tags::WGHT, 300.0), (tags::WGHT, 700.0)]);
        assert_eq!(c.as_slice(), &[16384]);
    }

    #[test]
    fn a_short_positional_slice_leaves_the_rest_at_default() {
        let v = vars(
            alloc::vec![
                axis(b"wght", 300.0, 400.0, 700.0),
                axis(b"opsz", 5.0, 10.5, 36.0)
            ],
            Vec::new(),
        );
        assert_eq!(v.normalize(&[700.0]).as_slice(), &[16384, 0]);
        // And a long one is truncated rather than panicking.
        assert_eq!(v.normalize(&[700.0, 36.0, 1.0]).as_slice(), &[16384, 16384]);
    }

    #[test]
    fn axis_index_finds_by_tag() {
        let v = vars(
            alloc::vec![
                axis(b"opsz", 5.0, 10.5, 36.0),
                axis(b"wght", 300.0, 400.0, 700.0)
            ],
            Vec::new(),
        );
        assert_eq!(v.axis_index(&tags::WGHT), Some(1));
        assert_eq!(v.axis_index(&tags::OPSZ), Some(0));
        assert_eq!(v.axis_index(&tags::WDTH), None);
    }

    #[test]
    fn coords_accessors_agree_with_the_slice() {
        let v = vars(
            alloc::vec![
                axis(b"wght", 300.0, 400.0, 700.0),
                axis(b"opsz", 5.0, 10.5, 36.0)
            ],
            Vec::new(),
        );
        let c = v.normalize(&[700.0]);
        assert_eq!(c.len(), 2);
        assert!(!c.is_empty());
        assert_eq!(c.get(0), Some(16384));
        assert_eq!(c.get(1), Some(0));
        assert_eq!(c.get(2), None);
        assert!(Coords::default().is_empty());
    }
}

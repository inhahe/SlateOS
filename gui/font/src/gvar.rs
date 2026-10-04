//! `gvar` — how a variable font's TrueType outlines change with the axes.
//!
//! This is the step of variable-font support a user can actually see. [`var`]
//! answers "where on the axes are we" and produces normalized coordinates;
//! `gvar` answers "and so where do this glyph's points go", which is the
//! difference between a face that claims to be variable and one that is.
//!
//! [`var`]: crate::var
//!
//! # What the table is
//!
//! A glyph's variation data is a list of **tuples**. Each tuple is a *region*
//! of the design space — a peak position on every axis, optionally with a
//! start and end bounding it — together with a delta for some or all of the
//! glyph's points. To draw at a given instance, every tuple is scored against
//! the current coordinates (its **scalar**: 1 at its peak, tapering to 0 at
//! its edges, 0 outside), and every tuple's deltas are added in, scaled.
//!
//! Two things make that less simple than it sounds, and both are where a naive
//! reader goes wrong rather than merely slow:
//!
//! * **A tuple usually names only some of the points.** The rest are not
//!   "unchanged" — they are *interpolated* from the nearest named points on
//!   either side, wrapping around the contour. This is IUP, "infer unreferenced
//!   points", and skipping it does not produce a slightly-off glyph; it
//!   produces one whose named points have moved and whose unnamed ones have
//!   not, i.e. a torn outline. A reader that treats an unnamed point as a zero
//!   delta is worse than one that ignores `gvar` entirely.
//! * **The point array is not the contour points.** Four **phantom points** are
//!   appended — the horizontal and vertical origins and advances — so that a
//!   face can vary its advance width without an `HVAR` table. They are part of
//!   the point *numbering*, so getting the count wrong shifts every delta in
//!   the last tuple onto the wrong point. They are deliberately *not* part of
//!   any contour, which is why IUP never reaches them.
//!
//! # What is not here
//!
//! * **Where advances come from.** The phantom points move with the rest
//!   ([`crate::glyf`] places them), and a face without `HVAR` takes its
//!   advances from them (`Face::advance_at`); a face with `HVAR` takes them
//!   from that table, as HarfBuzz does, whatever its phantom points say.
//! * **`CFF2`** varies through the charstring interpreter rather than through
//!   this table: see [`crate::cff`].
//!
//! # References
//!
//! [`Gvar::apply`] is HarfBuzz 14.3.0's `apply_deltas_to_points`, with its
//! `calculate_scalar`, `decompile_points`, `TupleValues::decompile` and IUP
//! loop, transcribed to the bit, because HarfBuzz is this crate's oracle for
//! drawing: a varied point's last bits depend on the order in which the
//! deltas are scaled, interpolated and added, and `tools/outline_oracle.py`
//! compares them with `==`.
//!
//! One reader follows FreeType instead: [`Gvar::deltas_fixed`], FreeType's
//! `TT_Vary_Apply_Glyph_Deltas` in its own 16.16 arithmetic, for the
//! auto-hinter ([`crate::hint`]), which is a port of FreeType's and must see
//! the points FreeType's loader gives it. The two agree to a fraction of a
//! unit; the hinter rounds each point to a unit, where the fraction decides.
//!
//! Portions of this file follow HarfBuzz 14.3.0's
//! `src/hb-ot-var-gvar-table.hh`, copyright © 2019 Adobe Inc. and © 2019
//! Ebrahim Byagowi, and `src/hb-ot-var-common.hh`, copyright © 2021 Google,
//! Inc. Used under HarfBuzz's licence: see
//! `gui/font/licenses/harfbuzz-COPYING`.
//!
//! `Gvar::deltas_fixed` follows FreeType 2.13.2's `src/truetype/ttgxvar.c`,
//! copyright (C) 2004-2023 by David Turner, Robert Wilhelm, Werner Lemberg and
//! George Williams, from The FreeType Project (www.freetype.org). Used under
//! the FreeType License: see `gui/font/licenses/FTL.TXT`.

use alloc::vec::Vec;

use crate::glyf::ContourPoint;
use crate::sfnt::{Span, u16_at, u32_at};
use crate::varstore::ScalarCache;

/// Bit in a tuple variation header's index word: the peak follows inline
/// rather than being one of the table's shared tuples.
const EMBEDDED_PEAK_TUPLE: u16 = 0x8000;
/// Bit: the region is bounded by an explicit start and end rather than
/// reaching to the axis extremes.
const INTERMEDIATE_REGION: u16 = 0x4000;
/// Bit: this tuple names its own points instead of using the glyph's shared
/// point list.
const PRIVATE_POINT_NUMBERS: u16 = 0x2000;
/// The remaining bits index the shared tuple array.
const TUPLE_INDEX_MASK: u16 = 0x0FFF;

/// Bit in a glyph's tuple count: a shared point list precedes the tuple data.
const SHARED_POINT_NUMBERS: u16 = 0x8000;
/// The remaining bits are the tuple count.
const TUPLE_COUNT_MASK: u16 = 0x0FFF;

/// Bit in `gvar`'s flags word: glyph offsets are 32-bit rather than 16-bit.
const LONG_OFFSETS: u16 = 0x0001;

/// A cap on one glyph's tuple count. The field is 12 bits, so 4095 is already
/// the format's own limit; this exists to make that explicit at the point of
/// allocation rather than trusting a mask elsewhere in the file.
const MAX_TUPLES: usize = 4096;

/// `gvar`'s header, resolved to absolute offsets.
///
/// The per-glyph data is *not* parsed here. A face has tens of thousands of
/// glyphs and a run draws a few dozen, so parsing every glyph's tuples at load
/// would cost more than the whole rest of face parsing put together and be
/// thrown away. What is parsed once is the part that is per-face: the offsets
/// table and the shared tuples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Gvar {
    axis_count: usize,
    /// Absolute offset of the shared tuple array.
    shared_tuples: usize,
    shared_tuple_count: usize,
    /// Absolute offset of the per-glyph offset array.
    offsets: usize,
    /// Absolute offset the per-glyph offsets are relative to.
    data_array: usize,
    glyph_count: usize,
    long_offsets: bool,
}

impl Gvar {
    /// Parse `gvar`'s header.
    ///
    /// `None` — rather than an error — when the table is unusable, including
    /// when it disagrees with `fvar` about the axis count. A `gvar` read
    /// against the wrong axis count does not fail; it reads each tuple's peak
    /// at the wrong stride and produces deltas for regions the font never
    /// described, which draws as a glyph subtly deformed at every instance. A
    /// face that declines to vary is strictly better than that.
    pub(crate) fn parse(
        data: &[u8],
        span: Span,
        axis_count: usize,
        num_glyphs: u16,
    ) -> Option<Self> {
        let base = span.off;
        let table = data.get(base..base.checked_add(span.len)?)?;
        if u16_at(table, 0)? != 1 {
            return None;
        }
        if usize::from(u16_at(table, 4)?) != axis_count || axis_count == 0 {
            return None;
        }
        let shared_tuple_count = usize::from(u16_at(table, 6)?);
        let shared_tuples = base.checked_add(usize::try_from(u32_at(table, 8)?).ok()?)?;
        let glyph_count = usize::from(u16_at(table, 12)?);
        let long_offsets = u16_at(table, 14)? & LONG_OFFSETS != 0;
        let data_array = base.checked_add(usize::try_from(u32_at(table, 16)?).ok()?)?;
        let offsets = base.checked_add(20)?;

        // The offset array has one more entry than there are glyphs — each
        // glyph's data runs to the next glyph's offset. A `gvar` that names
        // more glyphs than `maxp` does is describing variation for glyphs that
        // cannot be drawn.
        if glyph_count > usize::from(num_glyphs) {
            return None;
        }
        let stride = if long_offsets { 4 } else { 2 };
        let need = glyph_count.checked_add(1)?.checked_mul(stride)?;
        if 20usize.checked_add(need)? > span.len {
            return None;
        }
        Some(Self {
            axis_count,
            shared_tuples,
            shared_tuple_count,
            offsets,
            data_array,
            glyph_count,
            long_offsets,
        })
    }

    /// The span of `gid`'s variation data, or `None` if it has none.
    fn glyph_span(&self, data: &[u8], gid: u16) -> Option<Span> {
        let i = usize::from(gid);
        if i >= self.glyph_count {
            return None;
        }
        let read = |k: usize| -> Option<usize> {
            if self.long_offsets {
                let at = self.offsets.checked_add(k.checked_mul(4)?)?;
                usize::try_from(u32_at(data, at)?).ok()
            } else {
                // Short offsets are stored halved, which is why a face with an
                // odd-length glyph record pads it.
                let at = self.offsets.checked_add(k.checked_mul(2)?)?;
                usize::from(u16_at(data, at)?).checked_mul(2)
            }
        };
        let start = read(i)?;
        let end = read(i.checked_add(1)?)?;
        // Equal offsets mean "this glyph does not vary", which is common and
        // not an error: a face varies the letters and leaves the box-drawing
        // characters alone.
        if end <= start {
            return None;
        }
        let off = self.data_array.checked_add(start)?;
        let len = end.checked_sub(start)?;
        if off.checked_add(len)? > data.len() {
            return None;
        }
        Some(Span { off, len })
    }

    /// One shared tuple, as `axis_count` F2Dot14 values.
    fn shared_tuple(&self, data: &[u8], index: usize) -> Option<Vec<i16>> {
        if index >= self.shared_tuple_count {
            return None;
        }
        let off = self
            .shared_tuples
            .checked_add(index.checked_mul(self.axis_count)?.checked_mul(2)?)?;
        read_tuple(data, off, self.axis_count)
    }

    /// Move `points` -- one glyph's -- to `coords` by `gid`'s variation data,
    /// as HarfBuzz 14.3.0 moves them (`gvar::accelerator_t::
    /// apply_deltas_to_points`), to the bit.
    ///
    /// `points` are the glyph's points, each contour's last flagged
    /// `is_end_point`, and its four phantom points after them; for a
    /// composite, one point per component (its offset), each flagged, then
    /// the phantoms ([`crate::glyf`]). `coords` are HarfBuzz's normalized
    /// coordinates.
    ///
    /// The arithmetic is HarfBuzz's, which shows in a varied point's last
    /// bits:
    ///
    /// * each tuple's scalar is worked in `f64` and kept as `f32`
    ///   ([`calculate_scalar`]), a shared tuple's as HarfBuzz's cache hands
    ///   it back when drawing ([`Scalars`]);
    /// * each delta is multiplied by the scalar in `f32` *before* the points
    ///   the tuple does not name are interpolated from those it does, so the
    ///   interpolation runs on scaled deltas;
    /// * when no tuple names its own points, each tuple's deltas go straight
    ///   into the points, tuple by tuple; otherwise they are gathered and
    ///   added a batch at a time -- each tuple with its own points first adds
    ///   what the ones before it gathered -- and the last batch at the end.
    ///
    /// # Errors
    ///
    /// [`Unreadable`] where HarfBuzz gives up on the glyph and draws nothing:
    /// a tuple's data runs past the glyph's, or its point numbers or deltas
    /// cannot be read. A glyph without variation data, or whose shared point
    /// numbers or first tuple header cannot be read, is left where it is.
    pub(crate) fn apply(
        &self,
        data: &[u8],
        gid: u16,
        coords: &[i16],
        points: &mut [ContourPoint],
        scalars: Scalars,
    ) -> Result<(), Unreadable> {
        let Some(g) = self
            .glyph_span(data, gid)
            .and_then(|s| data.get(s.off..s.off.checked_add(s.len)?))
        else {
            return Ok(());
        };
        // `has_data` is the whole count word, flags and all.
        let (Some(count_word), Some(serialized)) = (u16_at(g, 0), u16_at(g, 2)) else {
            return Ok(());
        };
        if count_word == 0 {
            return Ok(());
        }
        let mut serial = usize::from(serialized);
        let shared = if count_word & SHARED_POINT_NUMBERS == 0 {
            Vec::new()
        } else {
            match hb_points(g, &mut serial, g.len()) {
                Some(p) => p,
                None => return Ok(()),
            }
        };
        let axes = self.axis_count;
        let mut left = count_word & TUPLE_COUNT_MASK;
        let mut header = 4usize;
        if !tuple_fits(g, header, left, axes) {
            return Ok(());
        }

        let count = points.len();
        // Whether any tuple from the first that applies onwards names its
        // own points: only if none does are deltas added straight in.
        let mut any_private: Option<bool> = None;
        // The points before any tuple that names its own, which IUP reads.
        let mut orig: Option<Vec<ContourPoint>> = None;
        let mut deltas: Vec<Delta> = Vec::new();
        let mut flush = false;
        loop {
            let (Some(size), Some(index)) = (
                u16_at(g, header),
                header.checked_add(2).and_then(|at| u16_at(g, at)),
            ) else {
                return Err(Unreadable);
            };
            let data_size = usize::from(size);
            let scalar = self.tuple_scalar(data, g, header, index, coords, scalars);
            if scalar != 0.0 {
                let any_private =
                    *any_private.get_or_insert_with(|| private_points_ahead(g, header, left, axes));
                let tuple_end = serial
                    .checked_add(data_size)
                    .filter(|&e| e <= g.len())
                    .ok_or(Unreadable)?;
                if deltas.len() != count {
                    deltas = alloc::vec![Delta::default(); count];
                }
                let mut p = serial;
                let private;
                let indices: &[u32] = if index & PRIVATE_POINT_NUMBERS == 0 {
                    &shared
                } else {
                    private = hb_points(g, &mut p, tuple_end).ok_or(Unreadable)?;
                    &private
                };
                let all_points = indices.is_empty();
                if all_points && !any_private {
                    add_deltas(g, &mut p, tuple_end, points, scalar, true).ok_or(Unreadable)?;
                    add_deltas(g, &mut p, tuple_end, points, scalar, false).ok_or(Unreadable)?;
                } else {
                    let n = if all_points { count } else { indices.len() };
                    let xs = hb_deltas(g, &mut p, tuple_end, n).ok_or(Unreadable)?;
                    let ys = hb_deltas(g, &mut p, tuple_end, n).ok_or(Unreadable)?;
                    if !all_points {
                        if orig.is_none() {
                            orig = Some(points.to_vec());
                        }
                        if flush {
                            add_gathered(points, &deltas);
                        }
                        deltas.fill(Delta::default());
                    }
                    for (k, (&x, &y)) in xs.iter().zip(&ys).enumerate() {
                        let at = if all_points {
                            k
                        } else {
                            indices
                                .get(k)
                                .and_then(|&i| usize::try_from(i).ok())
                                .unwrap_or(usize::MAX)
                        };
                        let Some(d) = deltas.get_mut(at) else {
                            continue;
                        };
                        if !all_points {
                            d.flag = true;
                        }
                        #[allow(
                            clippy::float_cmp,
                            reason = "HarfBuzz's own test; a scalar of exactly 1 adds \
                                      the delta unmultiplied, to the same result"
                        )]
                        if scalar == 1.0 {
                            d.x += int_f32(x);
                            d.y += int_f32(y);
                        } else {
                            d.x += int_f32(x) * scalar;
                            d.y += int_f32(y) * scalar;
                        }
                    }
                    if !all_points && let Some(orig) = &orig {
                        iup(points, orig, &mut deltas);
                    }
                    flush = true;
                }
            }
            // The next tuple's data follows this one's whether or not this
            // one applied.
            serial = serial.saturating_add(data_size);
            header = header.saturating_add(header_size(index, axes));
            left = left.saturating_sub(1);
            if !tuple_fits(g, header, left, axes) {
                break;
            }
        }
        if flush {
            add_gathered(points, &deltas);
        }
        Ok(())
    }

    /// A tuple's scalar at `coords` as HarfBuzz's `calculate_scalar` finds
    /// it, kept as the `f32` its caller keeps -- and, when drawing, a shared
    /// tuple's as HarfBuzz's cache hands it back ([`Scalars`]).
    fn tuple_scalar(
        &self,
        data: &[u8],
        g: &[u8],
        header: usize,
        index: u16,
        coords: &[i16],
        scalars: Scalars,
    ) -> f32 {
        let axes = self.axis_count;
        let embedded = index & EMBEDDED_PEAK_TUPLE != 0;
        let intermediate = index & INTERMEDIATE_REGION != 0;
        let shared = usize::from(index & TUPLE_INDEX_MASK);
        let mut at = header.saturating_add(4);
        let peak = if embedded {
            let t = read_tuple(g, at, axes);
            at = at.saturating_add(axes.saturating_mul(2));
            t
        } else if shared < self.shared_tuple_count {
            self.shared_tuple(data, shared)
        } else {
            return 0.0;
        };
        let Some(peak) = peak else {
            return 0.0;
        };
        let region = if intermediate {
            let start = read_tuple(g, at, axes);
            let end = read_tuple(g, at.saturating_add(axes.saturating_mul(2)), axes);
            let (Some(start), Some(end)) = (start, end) else {
                return 0.0;
            };
            Some((start, end))
        } else {
            None
        };
        let region = region.as_ref().map(|(s, e)| (s.as_slice(), e.as_slice()));
        #[allow(
            clippy::cast_possible_truncation,
            reason = "HarfBuzz narrows its `double` to the `float` it keeps"
        )]
        let scalar = calculate_scalar(&peak, region, coords) as f32;
        if scalars == Scalars::Drawn && !embedded && !intermediate {
            ScalarCache::remembered(scalar)
        } else {
            scalar
        }
    }

    /// The summed delta, 16.16, for every point of `gid` at the FreeType
    /// coordinates `coords` (16.16, [`crate::var::Coords::fixed`]), computed
    /// as FreeType's `TT_Vary_Apply_Glyph_Deltas` computes it: each tuple's
    /// scale by `ft_var_apply_tuple`, each delta scaled by `FT_MulFix`, the
    /// points a tuple does not name interpolated in 16.16 by
    /// `tt_interpolate_deltas`. For the auto-hinter, which must see the points
    /// FreeType's loader gives FreeType's; [`apply`](Self::apply) follows
    /// HarfBuzz for everything else.
    ///
    /// `points` are the glyph's points in whole font units, *with* the four
    /// phantom points at the end -- for a composite, one point per component
    /// (its offset) and then the phantoms; `ends` the contours' exclusive
    /// ends over the rest. `None` where the glyph does not vary or its data
    /// cannot be read.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "16.16 values of 16-bit font units and deltas: under 2^31 each, and               a sum over at most 4096 tuples stays under 2^43 -- far inside i64;               point indices stay below the point count"
    )]
    pub(crate) fn deltas_fixed(
        &self,
        data: &[u8],
        gid: u16,
        coords: &[i32],
        points: &[(i64, i64)],
        ends: &[usize],
    ) -> Option<Vec<(i64, i64)>> {
        use crate::ftcalc::mul_fix;
        let span = self.glyph_span(data, gid)?;
        let g = data.get(span.off..span.off.checked_add(span.len)?)?;
        let total = points.len();
        let count_word = u16_at(g, 0)?;
        let tuple_count = usize::from(count_word & TUPLE_COUNT_MASK);
        if tuple_count == 0 || tuple_count > MAX_TUPLES {
            return None;
        }
        let mut serial = usize::from(u16_at(g, 2)?);
        let shared_points = if count_word & SHARED_POINT_NUMBERS != 0 {
            Some(read_packed_points(g, &mut serial, total)?)
        } else {
            None
        };
        let org: Vec<(i64, i64)> = points.iter().map(|&(x, y)| (x << 16, y << 16)).collect();
        let mut sum = alloc::vec![(0i64, 0i64); total];
        let mut header = 4usize;
        for _ in 0..tuple_count {
            let data_size = usize::from(u16_at(g, header)?);
            let index = u16_at(g, header.checked_add(2)?)?;
            let mut h = header.checked_add(4)?;
            let peak = if index & EMBEDDED_PEAK_TUPLE != 0 {
                let t = read_tuple(g, h, self.axis_count)?;
                h = h.checked_add(self.axis_count.checked_mul(2)?)?;
                t
            } else {
                self.shared_tuple(data, usize::from(index & TUPLE_INDEX_MASK))?
            };
            let region = if index & INTERMEDIATE_REGION != 0 {
                let start = read_tuple(g, h, self.axis_count)?;
                h = h.checked_add(self.axis_count.checked_mul(2)?)?;
                let end = read_tuple(g, h, self.axis_count)?;
                h = h.checked_add(self.axis_count.checked_mul(2)?)?;
                Some((start, end))
            } else {
                None
            };
            header = h;
            let tuple_end = serial.checked_add(data_size)?;
            let apply = ft_apply_tuple(&peak, region.as_ref(), coords);
            if apply != 0 {
                let mut p = serial;
                let set = if index & PRIVATE_POINT_NUMBERS != 0 {
                    read_packed_points(g, &mut p, total)?
                } else {
                    shared_points.clone().unwrap_or(PointSet::All)
                };
                let named = match &set {
                    PointSet::All => total,
                    PointSet::Some(list) => list.len(),
                };
                let xs = read_packed_deltas(g, &mut p, named)?;
                let ys = read_packed_deltas(g, &mut p, named)?;
                let scaled = |d: i16| mul_fix(i64::from(d) << 16, apply);
                match &set {
                    PointSet::All => {
                        for (j, slot) in sum.iter_mut().enumerate() {
                            slot.0 += scaled(*xs.get(j)?);
                            slot.1 += scaled(*ys.get(j)?);
                        }
                    }
                    PointSet::Some(list) => {
                        // A named point's delta added to where it stands, the
                        // rest interpolated from those, then the difference
                        // from the original taken as this tuple's delta.
                        let mut out = org.clone();
                        let mut has = alloc::vec![false; total];
                        for (k, &idx) in list.iter().enumerate() {
                            let i = usize::from(idx);
                            let (Some(o), Some(f)) = (out.get_mut(i), has.get_mut(i)) else {
                                continue;
                            };
                            *f = true;
                            o.0 += scaled(*xs.get(k)?);
                            o.1 += scaled(*ys.get(k)?);
                        }
                        ft_interpolate(ends, &mut out, &org, &has)?;
                        for ((slot, o), g0) in sum.iter_mut().zip(&out).zip(&org) {
                            slot.0 += o.0 - g0.0;
                            slot.1 += o.1 - g0.1;
                        }
                    }
                }
            }
            serial = tuple_end;
        }
        Some(sum)
    }
}

/// How much of a tuple applies at the FreeType coordinates `coords`, 16.16:
/// `ft_var_apply_tuple`. Like [`calculate_scalar`] (HarfBuzz's), a
/// coordinate of 0 ends the tuple before its region is looked at; unlike it,
/// a malformed region is not set aside, and the arithmetic is 16.16.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "16.16 values of 16-bit font units and deltas: under 2^31 each, and               a sum over at most 4096 tuples stays under 2^43 -- far inside i64;               point indices stay below the point count"
)]
fn ft_apply_tuple(peak: &[i16], region: Option<&(Vec<i16>, Vec<i16>)>, coords: &[i32]) -> i64 {
    use crate::ftcalc::{f2dot14_to_fixed, mul_div};
    let mut apply: i64 = 0x10000;
    for (i, &pk) in peak.iter().enumerate() {
        let tc = f2dot14_to_fixed(pk);
        if tc == 0 {
            continue;
        }
        let nc = i64::from(coords.get(i).copied().unwrap_or(0));
        if nc == 0 {
            return 0;
        }
        if nc == tc {
            continue;
        }
        match region {
            None => {
                if nc < tc.min(0) || nc > tc.max(0) {
                    return 0;
                }
                apply = mul_div(apply, nc, tc);
            }
            Some((start, end)) => {
                let (Some(&s), Some(&e)) = (start.get(i), end.get(i)) else {
                    return 0;
                };
                let (s, e) = (f2dot14_to_fixed(s), f2dot14_to_fixed(e));
                if nc <= s || nc >= e {
                    return 0;
                }
                apply = if nc < tc {
                    mul_div(apply, nc - s, tc - s)
                } else {
                    mul_div(apply, e - nc, e - tc)
                };
            }
        }
    }
    apply
}

/// Interpolate, contour by contour, the points a tuple did not name, from
/// the named ones either side: `tt_interpolate_deltas`, in 16.16. `out`
/// holds each point's position, moved for the named ones; `org` where each
/// was. A contour with one named point moves whole with it.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "16.16 values of 16-bit font units and deltas: under 2^31 each, and               a sum over at most 4096 tuples stays under 2^43 -- far inside i64;               point indices stay below the point count"
)]
fn ft_interpolate(
    ends: &[usize],
    out: &mut [(i64, i64)],
    org: &[(i64, i64)],
    has: &[bool],
) -> Option<()> {
    let named = |i: usize| has.get(i).copied().unwrap_or(false);
    let mut point = 0usize;
    for &end in ends {
        let Some(end_point) = end.checked_sub(1) else {
            continue;
        };
        let first_point = point;
        while point <= end_point && !named(point) {
            point += 1;
        }
        if point <= end_point {
            let first_delta = point;
            let mut cur_delta = point;
            point += 1;
            while point <= end_point {
                if named(point) {
                    ft_delta_interpolate(cur_delta + 1, point - 1, cur_delta, point, org, out)?;
                    cur_delta = point;
                }
                point += 1;
            }
            if cur_delta == first_delta {
                ft_delta_shift(first_point, end_point, cur_delta, org, out)?;
            } else {
                ft_delta_interpolate(cur_delta + 1, end_point, cur_delta, first_delta, org, out)?;
                if first_delta > 0 {
                    ft_delta_interpolate(
                        first_point,
                        first_delta - 1,
                        cur_delta,
                        first_delta,
                        org,
                        out,
                    )?;
                }
            }
        }
        // The next contour starts after this one, wherever the search ended.
        point = point.max(end);
    }
    Some(())
}

/// Move points `p1..=p2`, all but `reference`, as far as `reference` moved:
/// `tt_delta_shift`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "16.16 values of 16-bit font units and deltas: under 2^31 each, and               a sum over at most 4096 tuples stays under 2^43 -- far inside i64;               point indices stay below the point count"
)]
fn ft_delta_shift(
    p1: usize,
    p2: usize,
    reference: usize,
    org: &[(i64, i64)],
    out: &mut [(i64, i64)],
) -> Option<()> {
    let (o, r) = (*out.get(reference)?, *org.get(reference)?);
    let delta = (o.0 - r.0, o.1 - r.1);
    if delta == (0, 0) {
        return Some(());
    }
    for p in (p1..reference).chain(reference + 1..=p2) {
        let slot = out.get_mut(p)?;
        slot.0 += delta.0;
        slot.1 += delta.1;
    }
    Some(())
}

/// Interpolate points `p1..=p2` between two named ones, following the nearer
/// outside them: `tt_delta_interpolate`, each coordinate on its own. Where
/// the named points share a coordinate but moved differently, their
/// neighbours stay where they were.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "16.16 values of 16-bit font units and deltas: under 2^31 each, and               a sum over at most 4096 tuples stays under 2^43 -- far inside i64;               point indices stay below the point count"
)]
fn ft_delta_interpolate(
    p1: usize,
    p2: usize,
    ref1: usize,
    ref2: usize,
    org: &[(i64, i64)],
    out: &mut [(i64, i64)],
) -> Option<()> {
    use crate::ftcalc::{div_fix, mul_fix};
    if p1 > p2 {
        return Some(());
    }
    let pick = |v: (i64, i64), dim: usize| if dim == 0 { v.0 } else { v.1 };
    for dim in 0..2 {
        let (mut r1, mut r2) = (ref1, ref2);
        if pick(*org.get(r1)?, dim) > pick(*org.get(r2)?, dim) {
            core::mem::swap(&mut r1, &mut r2);
        }
        let (in1, in2) = (pick(*org.get(r1)?, dim), pick(*org.get(r2)?, dim));
        let (out1, out2) = (pick(*out.get(r1)?, dim), pick(*out.get(r2)?, dim));
        let (d1, d2) = (out1 - in1, out2 - in2);
        if in1 == in2 && out1 != out2 {
            continue;
        }
        let scale = if in1 == in2 {
            0
        } else {
            div_fix(out2 - out1, in2 - in1)
        };
        for p in p1..=p2 {
            let o = pick(*org.get(p)?, dim);
            let moved = if o <= in1 {
                o + d1
            } else if o >= in2 {
                o + d2
            } else {
                out1 + mul_fix(o - in1, scale)
            };
            let slot = out.get_mut(p)?;
            if dim == 0 {
                slot.0 = moved;
            } else {
                slot.1 = moved;
            }
        }
    }
    Some(())
}

/// Which points a tuple names.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PointSet {
    /// Every point, in order — the encoding a count of zero means. No IUP is
    /// needed, because nothing is unreferenced.
    All,
    /// An explicit, ascending list of point numbers.
    Some(Vec<u16>),
}

/// Which of HarfBuzz's two readings of a shared tuple's scalar to reproduce.
///
/// HarfBuzz draws a glyph through a cache of the shared tuples' scalars that
/// lasts as long as the font keeps its instance, and measures a glyph's box
/// without one. The cache stores a scalar in whole 2^-30ths, so for a scalar
/// under 2^-6 the two readings can differ in the last bit; once the cache
/// holds a tuple, every glyph drawn afterwards sees the stored value
/// ([`ScalarCache::remembered`]), which is what [`Drawn`](Self::Drawn)
/// reproduces. (The first glyph to reach a tuple sees the exact one, and
/// which glyph that is depends on what was drawn before.)
///
/// The cache also answers for a tuple with an intermediate region whose peak
/// is a shared tuple: if it holds 0 or 1 for that peak, HarfBuzz returns it
/// without looking at the region -- a 0 where the region, past the peak,
/// would give more. Whether it holds one depends on the glyphs drawn before,
/// so this module weighs the region, as HarfBuzz does when it does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scalars {
    /// As `hb_font_draw_glyph` weighs them, through the cache.
    Drawn,
    /// As `hb_font_get_glyph_extents` weighs them, without.
    Measured,
}

/// A glyph's variation data could not be read, and HarfBuzz draws nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unreadable;

/// One point's pending delta, and whether the tuple being applied named it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Delta {
    x: f32,
    y: f32,
    flag: bool,
}

/// Add the gathered deltas into the points: `contour_point_t::translate`.
fn add_gathered(points: &mut [ContourPoint], deltas: &[Delta]) {
    for (p, d) in points.iter_mut().zip(deltas) {
        p.x += d.x;
        p.y += d.y;
    }
}

/// An integer delta as C's int-to-float conversion has it: exact below 2^24.
fn int_f32(v: i32) -> f32 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "rounds to nearest past 2^24, as C's conversion does"
    )]
    {
        v as f32
    }
}

/// A tuple header's size: 4 bytes, and a tuple of `axes` words each for an
/// embedded peak and for the intermediate region's two ends.
fn header_size(index: u16, axes: usize) -> usize {
    let tuples = usize::from(index & EMBEDDED_PEAK_TUPLE != 0)
        .saturating_add(usize::from(index & INTERMEDIATE_REGION != 0).saturating_mul(2));
    tuples
        .saturating_mul(axes)
        .saturating_mul(2)
        .saturating_add(4)
}

/// `tuple_iterator_t::is_valid`: a tuple is left, and its whole header lies
/// inside the glyph's data.
fn tuple_fits(g: &[u8], header: usize, left: u16, axes: usize) -> bool {
    if left == 0 || header.checked_add(4).is_none_or(|e| e > g.len()) {
        return false;
    }
    let Some(index) = header.checked_add(2).and_then(|at| u16_at(g, at)) else {
        return false;
    };
    header
        .checked_add(header_size(index, axes))
        .is_some_and(|e| e <= g.len())
}

/// Whether the tuple at `header`, or any after it, names its own points.
fn private_points_ahead(g: &[u8], mut header: usize, mut left: u16, axes: usize) -> bool {
    loop {
        let Some(index) = header.checked_add(2).and_then(|at| u16_at(g, at)) else {
            return false;
        };
        if index & PRIVATE_POINT_NUMBERS != 0 {
            return true;
        }
        header = header.saturating_add(header_size(index, axes));
        left = left.saturating_sub(1);
        if !tuple_fits(g, header, left, axes) {
            return false;
        }
    }
}

/// Fill in the deltas of the points the tuple did not name -- IUP -- as
/// HarfBuzz does: within each contour (found by `points`' end flags), each
/// run of unnamed points between two named ones takes its deltas from theirs
/// ([`infer`]), by the points' positions in `orig`, the run wrapping around
/// the contour's end. A contour whose points are all named, or none, is left
/// alone; the phantom points, in no contour, always are.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "every index is below `count`, the length of all three slices, \
              which is checked on entry; `start <= end < count`, so the \
              additions stay below `count + 1`"
)]
fn iup(points: &[ContourPoint], orig: &[ContourPoint], deltas: &mut [Delta]) {
    let count = points.len();
    if orig.len() != count || deltas.len() != count {
        return;
    }
    let mut start = 0usize;
    let mut end = 0usize;
    loop {
        while end < count && !points[end].is_end_point {
            end += 1;
        }
        if end == count {
            break;
        }
        let named = deltas[start..=end].iter().filter(|d| d.flag).count();
        let unnamed = end - start + 1 - named;
        if unnamed != 0 && unnamed <= end - start {
            fill_gaps(orig, deltas, start, end, unnamed);
        }
        start = end + 1;
        end = start;
    }
}

/// The gap loop of HarfBuzz's IUP over one contour, `start..=end`, with
/// `unnamed` of its points unnamed (at least one, and not all).
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "`start <= i <= end` for every index, and the caller's slices \
              reach past `end`; the step count is bounded by the guard"
)]
fn fill_gaps(
    orig: &[ContourPoint],
    deltas: &mut [Delta],
    start: usize,
    end: usize,
    unnamed: usize,
) {
    let next_index = |i: usize| if i >= end { start } else { i + 1 };
    let mut unnamed = unnamed;
    // A correct walk goes round the contour about twice; the guard only
    // makes termination not depend on that argument.
    let mut steps = 0usize;
    let limit = (end - start + 1) * 4 + 8;
    let mut j = start;
    loop {
        // The last named point before a gap...
        let mut i;
        loop {
            i = j;
            j = next_index(i);
            steps += 1;
            if deltas[i].flag && !deltas[j].flag {
                break;
            }
            if steps > limit {
                return;
            }
        }
        let prev = i;
        j = i;
        // ...and the first named one after it.
        loop {
            i = j;
            j = next_index(i);
            steps += 1;
            if !deltas[i].flag && deltas[j].flag {
                break;
            }
            if steps > limit {
                return;
            }
        }
        let next = j;
        i = prev;
        loop {
            i = next_index(i);
            if i == next {
                break;
            }
            deltas[i].x = infer(
                orig[i].x,
                orig[prev].x,
                orig[next].x,
                deltas[prev].x,
                deltas[next].x,
            );
            deltas[i].y = infer(
                orig[i].y,
                orig[prev].y,
                orig[next].y,
                deltas[prev].y,
                deltas[next].y,
            );
            unnamed -= 1;
            if unnamed == 0 {
                return;
            }
        }
    }
}

/// One coordinate's inferred delta: HarfBuzz's `infer_delta`.
///
/// When the two named points sit at one coordinate there is no ratio to
/// take; when the target is outside them it *follows the nearer one* rather
/// than extrapolating, which is what keeps a stroke's end from shooting off
/// when only its middle was named.
#[allow(
    clippy::float_cmp,
    reason = "HarfBuzz's own tests: whether two points share a coordinate, \
              and two deltas are the same, exactly"
)]
fn infer(target: f32, prev: f32, next: f32, prev_delta: f32, next_delta: f32) -> f32 {
    if prev == next {
        return if prev_delta == next_delta {
            prev_delta
        } else {
            0.0
        };
    }
    if target <= prev.min(next) {
        return if prev < next { prev_delta } else { next_delta };
    }
    if target >= prev.max(next) {
        return if prev > next { prev_delta } else { next_delta };
    }
    let r = (target - prev) / (next - prev);
    prev_delta + r * (next_delta - prev_delta)
}

/// How much of a tuple's deltas apply at `coords`: HarfBuzz 14.3.0's
/// `TupleVariationHeader::calculate_scalar`, in `f64`.
///
/// 1 at the tuple's peak, tapering linearly to 0 at the edges of its region,
/// and 0 outside -- multiplied across the axes, so a two-axis tuple
/// contributes only where both axes are inside. A coordinate of 0 on an axis
/// the tuple peaks on gives 0 before anything else is asked, a malformed
/// intermediate region included; otherwise a region whose start is past its
/// peak, whose peak is past its end, or which spans zero is *ignored on that
/// axis*, keeping the tuple at full strength there.
fn calculate_scalar(peak: &[i16], region: Option<(&[i16], &[i16])>, coords: &[i16]) -> f64 {
    let mut scalar = 1.0f64;
    for (i, &pk) in peak.iter().enumerate() {
        let pk = i32::from(pk);
        if pk == 0 {
            continue;
        }
        let v = coords.get(i).map_or(0, |&c| i32::from(c));
        if v == 0 {
            return 0.0;
        }
        if v == pk {
            continue;
        }
        if let Some((start, end)) = region {
            let (Some(&s), Some(&e)) = (start.get(i), end.get(i)) else {
                continue;
            };
            let (s, e) = (i32::from(s), i32::from(e));
            if s > pk || pk > e || (s < 0 && e > 0) {
                continue;
            }
            if v < s || v > e {
                return 0.0;
            }
            // F2Dot14 values: every difference fits `i32` and is exact in
            // `f64`.
            if v < pk {
                if pk != s {
                    scalar *= f64::from(v.wrapping_sub(s)) / f64::from(pk.wrapping_sub(s));
                }
            } else if pk != e {
                scalar *= f64::from(e.wrapping_sub(v)) / f64::from(e.wrapping_sub(pk));
            }
        } else {
            if v < pk.min(0) || v > pk.max(0) {
                return 0.0;
            }
            scalar *= f64::from(v) / f64::from(pk);
        }
    }
    scalar
}

/// A packed point-number list, as HarfBuzz's `decompile_points` reads it: a
/// count (0 for every point, which is the empty list), then runs of byte or
/// word increments summed in 32 bits. Point numbers are not checked against
/// the glyph here; one past its points is skipped where it is used. `None`
/// where the list runs past `end`.
fn hb_points(g: &[u8], p: &mut usize, end: usize) -> Option<Vec<u32>> {
    const POINTS_ARE_WORDS: u8 = 0x80;
    const RUN_MASK: u8 = 0x7F;
    let end = end.min(g.len());
    let byte = |at: usize| if at < end { g.get(at).copied() } else { None };
    let mut at = *p;
    let first = byte(at)?;
    at = at.checked_add(1)?;
    let mut count = usize::from(first);
    if first & POINTS_ARE_WORDS != 0 {
        let second = byte(at)?;
        at = at.checked_add(1)?;
        count = (usize::from(first & RUN_MASK) << 8) | usize::from(second);
    }
    let mut points = Vec::with_capacity(count);
    let mut n = 0u32;
    while points.len() < count {
        let control = byte(at)?;
        at = at.checked_add(1)?;
        let run = usize::from(control & RUN_MASK).checked_add(1)?;
        if points.len().checked_add(run)? > count {
            return None;
        }
        let words = control & POINTS_ARE_WORDS != 0;
        let width = if words { 2 } else { 1 };
        if at.checked_add(run.checked_mul(width)?)? > end {
            return None;
        }
        for _ in 0..run {
            let step = if words {
                u32::from(u16_at(g, at)?)
            } else {
                u32::from(byte(at)?)
            };
            n = n.wrapping_add(step);
            points.push(n);
            at = at.checked_add(width)?;
        }
    }
    *p = at;
    Some(points)
}

/// How a packed delta run is stored: its control byte's top two bits.
const DELTAS_SIZE_MASK: u8 = 0xC0;
const DELTAS_ARE_ZERO: u8 = 0x80;
const DELTAS_ARE_WORDS: u8 = 0x40;
/// Four bytes each: HarfBuzz reads the fourth combination as 32-bit deltas.
const DELTAS_ARE_LONGS: u8 = 0xC0;
const DELTA_RUN_MASK: u8 = 0x3F;

/// A packed run's values from `at`: its width in bytes, and a reader.
fn delta_at(g: &[u8], at: usize, kind: u8) -> Option<i32> {
    match kind {
        DELTAS_ARE_WORDS =>
        {
            #[allow(clippy::cast_possible_wrap, reason = "a delta is signed")]
            Some(i32::from(u16_at(g, at)? as i16))
        }
        DELTAS_ARE_LONGS =>
        {
            #[allow(clippy::cast_possible_wrap, reason = "a delta is signed")]
            Some(u32_at(g, at)? as i32)
        }
        _ =>
        {
            #[allow(clippy::cast_possible_wrap, reason = "a byte delta is signed")]
            Some(i32::from(*g.get(at)? as i8))
        }
    }
}

/// The byte width of one value in a run of this kind.
fn delta_width(kind: u8) -> usize {
    match kind {
        DELTAS_ARE_ZERO => 0,
        DELTAS_ARE_WORDS => 2,
        DELTAS_ARE_LONGS => 4,
        _ => 1,
    }
}

/// `count` packed deltas, as HarfBuzz's `TupleValues::decompile` reads them.
/// `None` where they run past `end`, or a run past `count`.
fn hb_deltas(g: &[u8], p: &mut usize, end: usize, count: usize) -> Option<Vec<i32>> {
    let end = end.min(g.len());
    let mut values = alloc::vec![0i32; count];
    let mut at = *p;
    let mut i = 0usize;
    while i < count {
        if at >= end {
            return None;
        }
        let control = *g.get(at)?;
        at = at.checked_add(1)?;
        let run = usize::from(control & DELTA_RUN_MASK).checked_add(1)?;
        let stop = i.checked_add(run)?;
        if stop > count {
            return None;
        }
        let kind = control & DELTAS_SIZE_MASK;
        let width = delta_width(kind);
        if at.checked_add(run.checked_mul(width)?)? > end {
            return None;
        }
        if width != 0 {
            for slot in values.get_mut(i..stop)? {
                *slot = delta_at(g, at, kind)?;
                at = at.checked_add(width)?;
            }
        }
        i = stop;
    }
    *p = at;
    Some(values)
}

/// One axis's deltas for every point, each multiplied by `scalar` and added
/// straight into the point: HarfBuzz's `decompile_deltas_add_to_points`,
/// which it uses when no tuple of the glyph names its own points. `None`
/// where they run past `end`, or a run past the points.
fn add_deltas(
    g: &[u8],
    p: &mut usize,
    end: usize,
    points: &mut [ContourPoint],
    scalar: f32,
    x: bool,
) -> Option<()> {
    let end = end.min(g.len());
    let count = points.len();
    let mut at = *p;
    let mut i = 0usize;
    while i < count {
        if at >= end {
            return None;
        }
        let control = *g.get(at)?;
        at = at.checked_add(1)?;
        let run = usize::from(control & DELTA_RUN_MASK).checked_add(1)?;
        let stop = i.checked_add(run)?;
        if stop > count {
            return None;
        }
        let kind = control & DELTAS_SIZE_MASK;
        let width = delta_width(kind);
        if at.checked_add(run.checked_mul(width)?)? > end {
            return None;
        }
        if width != 0 {
            for point in points.get_mut(i..stop)? {
                let v = int_f32(delta_at(g, at, kind)?) * scalar;
                if x {
                    point.x += v;
                } else {
                    point.y += v;
                }
                at = at.checked_add(width)?;
            }
        }
        i = stop;
    }
    *p = at;
    Some(())
}

/// `count` F2Dot14 values at `off`.
fn read_tuple(d: &[u8], off: usize, count: usize) -> Option<Vec<i16>> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = off.checked_add(i.checked_mul(2)?)?;
        #[allow(
            clippy::cast_possible_wrap,
            reason = "F2Dot14 is a signed 16-bit quantity"
        )]
        out.push(u16_at(d, at)? as i16);
    }
    Some(out)
}

/// Read a packed point-number list, advancing `pos` past it.
///
/// A count of zero is the encoding for "all points", which is a different thing
/// from an empty list and is why this returns a [`PointSet`] rather than a
/// `Vec`. Point numbers are stored as *deltas* from the previous one, in runs
/// of bytes or words.
fn read_packed_points(d: &[u8], pos: &mut usize, total: usize) -> Option<PointSet> {
    const POINTS_ARE_WORDS: u8 = 0x80;
    const POINT_RUN_MASK: u8 = 0x7F;

    let first = *d.get(*pos)?;
    *pos = pos.checked_add(1)?;
    let count = if first & POINTS_ARE_WORDS == 0 {
        usize::from(first)
    } else {
        let second = *d.get(*pos)?;
        *pos = pos.checked_add(1)?;
        usize::from(first & POINT_RUN_MASK)
            .checked_mul(256)?
            .checked_add(usize::from(second))?
    };
    if count == 0 {
        return Some(PointSet::All);
    }
    // A list longer than the glyph has points cannot be honoured and is the
    // shape a fuzzer produces to make a reader allocate on its word.
    if count > total {
        return None;
    }

    let mut out = Vec::with_capacity(count);
    let mut n: u16 = 0;
    while out.len() < count {
        let ctrl = *d.get(*pos)?;
        *pos = pos.checked_add(1)?;
        let run = usize::from(ctrl & POINT_RUN_MASK).checked_add(1)?;
        if out.len().checked_add(run)? > count {
            return None;
        }
        for _ in 0..run {
            if ctrl & POINTS_ARE_WORDS == 0 {
                n = n.wrapping_add(u16::from(*d.get(*pos)?));
                *pos = pos.checked_add(1)?;
            } else {
                n = n.wrapping_add(u16_at(d, *pos)?);
                *pos = pos.checked_add(2)?;
            }
            out.push(n);
        }
    }
    Some(PointSet::Some(out))
}

/// Read `count` packed deltas, advancing `pos` past them.
///
/// Runs are one of three kinds: all zero (stored as nothing but the control
/// byte, which is how a tuple that moves ten points out of two hundred stays
/// small), one byte each, or one word each.
fn read_packed_deltas(d: &[u8], pos: &mut usize, count: usize) -> Option<Vec<i16>> {
    const DELTAS_ARE_ZERO: u8 = 0x80;
    const DELTAS_ARE_WORDS: u8 = 0x40;
    const DELTA_RUN_MASK: u8 = 0x3F;

    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let ctrl = *d.get(*pos)?;
        *pos = pos.checked_add(1)?;
        let run = usize::from(ctrl & DELTA_RUN_MASK).checked_add(1)?;
        if out.len().checked_add(run)? > count {
            return None;
        }
        for _ in 0..run {
            if ctrl & DELTAS_ARE_ZERO != 0 {
                out.push(0);
            } else if ctrl & DELTAS_ARE_WORDS != 0 {
                #[allow(clippy::cast_possible_wrap, reason = "a delta is signed")]
                out.push(u16_at(d, *pos)? as i16);
                *pos = pos.checked_add(2)?;
            } else {
                #[allow(clippy::cast_possible_wrap, reason = "a byte delta is signed")]
                out.push(i16::from(*d.get(*pos)? as i8));
                *pos = pos.checked_add(1)?;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    // A test that unwraps a failure should fail loudly at the line that did
    // it — that is the diagnosis. The defensive lints exist to keep panics out
    // of code that runs on a user's data, which this is not.
    #![allow(
        clippy::float_cmp,
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;

    // --- the scalar: how much of a region applies ---

    #[test]
    fn a_tuple_applies_fully_at_its_peak() {
        assert_eq!(calculate_scalar(&[16384], None, &[16384]), 1.0);
    }

    #[test]
    fn a_tuple_tapers_linearly_toward_the_default() {
        // Peak at +1, asked for the half-way point: half the delta.
        assert_eq!(calculate_scalar(&[16384], None, &[8192]), 0.5);
    }

    #[test]
    fn a_tuple_does_not_apply_at_the_default_instance() {
        assert_eq!(calculate_scalar(&[16384], None, &[0]), 0.0);
    }

    #[test]
    fn a_tuple_does_not_apply_across_the_default() {
        // A tuple peaking at +1 says nothing about the -1 side. Applying it
        // there would make asking for light produce a bolder shape.
        assert_eq!(calculate_scalar(&[16384], None, &[-8192]), 0.0);
    }

    #[test]
    fn an_axis_the_tuple_does_not_mention_is_ignored() {
        // Peak 0 on an axis means "any position", not "the default position".
        assert_eq!(calculate_scalar(&[0, 16384], None, &[16384, 16384]), 1.0);
    }

    #[test]
    fn axes_multiply_so_a_corner_tuple_needs_both() {
        // Half way along each of two axes is a quarter of the corner's delta.
        assert_eq!(calculate_scalar(&[16384, 16384], None, &[8192, 8192]), 0.25);
    }

    #[test]
    fn an_intermediate_region_tapers_between_its_own_bounds() {
        // Start 0, peak +0.5, end +1: at +0.25 we are half way up the rise.
        let region = (alloc::vec![0], alloc::vec![16384]);
        assert_eq!(
            calculate_scalar(
                &[8192],
                Some((region.0.as_slice(), region.1.as_slice())),
                &[4096]
            ),
            0.5
        );
        assert_eq!(
            calculate_scalar(
                &[8192],
                Some((region.0.as_slice(), region.1.as_slice())),
                &[8192]
            ),
            1.0
        );
        // And half way down the fall, which is a *different* slope because the
        // two sides of the region have different widths.
        assert_eq!(
            calculate_scalar(
                &[8192],
                Some((region.0.as_slice(), region.1.as_slice())),
                &[12288]
            ),
            0.5
        );
    }

    #[test]
    fn an_intermediate_region_is_zero_outside_its_bounds() {
        let region = (alloc::vec![4096], alloc::vec![12288]);
        assert_eq!(
            calculate_scalar(
                &[8192],
                Some((region.0.as_slice(), region.1.as_slice())),
                &[0]
            ),
            0.0
        );
        assert_eq!(
            calculate_scalar(
                &[8192],
                Some((region.0.as_slice(), region.1.as_slice())),
                &[16384]
            ),
            0.0
        );
    }

    #[test]
    fn a_backwards_intermediate_region_is_ignored_not_zeroed() {
        // start > peak: malformed. HarfBuzz skips the axis, which leaves the
        // tuple at full strength there rather than dropping a delta the font
        // meant to apply.
        let region = (alloc::vec![16384], alloc::vec![16384]);
        assert_eq!(
            calculate_scalar(
                &[8192],
                Some((region.0.as_slice(), region.1.as_slice())),
                &[4096]
            ),
            1.0
        );
    }

    // --- packed point numbers ---

    #[test]
    fn a_zero_count_means_every_point() {
        let mut pos = 0;
        assert_eq!(
            read_packed_points(&[0x00], &mut pos, 10),
            Some(PointSet::All)
        );
        assert_eq!(pos, 1);
    }

    #[test]
    fn point_numbers_accumulate_rather_than_repeat() {
        // Three points, one byte-run of three deltas: 1, then +2, then +3.
        let d = [0x03, 0x02, 1, 2, 3];
        let mut pos = 0;
        assert_eq!(
            read_packed_points(&d, &mut pos, 10),
            Some(PointSet::Some(alloc::vec![1, 3, 6]))
        );
        assert_eq!(pos, 5);
    }

    #[test]
    fn a_word_run_of_point_numbers_reads_two_bytes_each() {
        // Control 0x81 = words, run of 2.
        let d = [0x02, 0x81, 0x01, 0x00, 0x00, 0x05];
        let mut pos = 0;
        assert_eq!(
            read_packed_points(&d, &mut pos, 1000),
            Some(PointSet::Some(alloc::vec![256, 261]))
        );
        assert_eq!(pos, 6);
    }

    #[test]
    fn a_two_byte_count_is_read_as_one_number() {
        // 0x81 0x00 => count 256, not count 1 followed by a run.
        let mut d = alloc::vec![0x81, 0x00];
        // One byte-run of 64, four times, is 256 points.
        for _ in 0..4 {
            d.push(0x3F);
            d.extend(core::iter::repeat_n(1u8, 64));
        }
        let mut pos = 0;
        let got = read_packed_points(&d, &mut pos, 300).unwrap();
        match got {
            PointSet::Some(v) => {
                assert_eq!(v.len(), 256);
                assert_eq!(v[0], 1);
                assert_eq!(v[255], 256);
            }
            PointSet::All => panic!("read a two-byte count as `all points`"),
        }
    }

    #[test]
    fn a_point_list_longer_than_the_glyph_is_refused() {
        // The allocation would otherwise be the attacker's to choose.
        let d = [0x81, 0xFF];
        let mut pos = 0;
        assert_eq!(read_packed_points(&d, &mut pos, 4), None);
    }

    #[test]
    fn a_truncated_point_list_is_none_not_a_panic() {
        let d = [0x03, 0x02, 1];
        let mut pos = 0;
        assert_eq!(read_packed_points(&d, &mut pos, 10), None);
    }

    // --- packed deltas ---

    #[test]
    fn a_zero_run_costs_one_byte_and_no_data() {
        // 0x84 = zeros, run of 5.
        let mut pos = 0;
        assert_eq!(
            read_packed_deltas(&[0x84], &mut pos, 5),
            Some(alloc::vec![0, 0, 0, 0, 0])
        );
        assert_eq!(pos, 1);
    }

    #[test]
    fn byte_deltas_are_signed() {
        // 0x01 = bytes, run of 2. 0xFF is -1, not 255.
        let mut pos = 0;
        assert_eq!(
            read_packed_deltas(&[0x01, 0xFF, 0x01], &mut pos, 2),
            Some(alloc::vec![-1, 1])
        );
    }

    #[test]
    fn word_deltas_are_signed() {
        // 0x40 = words, run of 1.
        let mut pos = 0;
        assert_eq!(
            read_packed_deltas(&[0x40, 0xFF, 0x00], &mut pos, 1),
            Some(alloc::vec![-256])
        );
    }

    #[test]
    fn a_run_that_overruns_the_expected_count_is_refused() {
        // Claiming a run of 64 when only 2 deltas were asked for means the
        // reader and the file disagree about the point count, which is the
        // error worth reporting rather than silently truncating.
        let mut pos = 0;
        assert_eq!(read_packed_deltas(&[0xBF], &mut pos, 2), None);
    }

    // --- IUP ---

    /// One on-curve point. IUP reads only the coordinates, but the point type
    /// is `glyf`'s so that no copy of the array is needed to vary a glyph.
    fn pt(x: f32, y: f32) -> ContourPoint {
        ContourPoint {
            x,
            y,
            flag: 1,
            is_end_point: false,
        }
    }

    /// A square contour, points at the corners, then four phantom points.
    fn square() -> Vec<ContourPoint> {
        let mut pts = alloc::vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        pts[3].is_end_point = true;
        pts.extend([pt(0.0, 0.0); 4]);
        pts
    }

    /// One delta per point of [`square`]: `(x, y, named)`.
    fn deltas(ds: [(f32, f32, bool); 8]) -> Vec<Delta> {
        ds.iter()
            .map(|&(x, y, flag)| Delta { x, y, flag })
            .collect()
    }

    const NONE: (f32, f32, bool) = (0.0, 0.0, false);

    #[test]
    fn a_contour_with_every_point_named_is_left_alone() {
        let pts = square();
        let named = (1.0, 2.0, true);
        let mut d = deltas([named, named, named, named, NONE, NONE, NONE, NONE]);
        let before = d.clone();
        iup(&pts, &pts, &mut d);
        assert_eq!(d, before);
    }

    #[test]
    fn a_contour_with_no_point_named_is_left_at_zero() {
        // Nothing to interpolate from. Leaving it alone is right; the tuple
        // simply does not move this contour.
        let pts = square();
        let mut d = deltas([NONE; 8]);
        iup(&pts, &pts, &mut d);
        assert_eq!(d, deltas([NONE; 8]));
    }

    #[test]
    fn an_unnamed_point_between_two_named_ones_interpolates() {
        // Points at x = 0, 10, 10, 0. Name the first and third; the second
        // sits at the same x as the third, so it takes the third's delta.
        let pts = square();
        let mut d = deltas([
            (0.0, 0.0, true),
            NONE,
            (10.0, 0.0, true),
            NONE,
            NONE,
            NONE,
            NONE,
            NONE,
        ]);
        iup(&pts, &pts, &mut d);
        // Point 1 is at x=10 like point 2, and y=0 like point 0.
        assert_eq!((d[1].x, d[1].y), (10.0, 0.0));
        // An inferred point stays unnamed, as in HarfBuzz.
        assert!(!d[1].flag);
    }

    #[test]
    fn a_gap_wraps_around_the_end_of_the_contour() {
        // Name only point 1. Points 2, 3 and 0 form one gap that wraps.
        let pts = square();
        let mut d = deltas([NONE, (5.0, 5.0, true), NONE, NONE, NONE, NONE, NONE, NONE]);
        iup(&pts, &pts, &mut d);
        // With a single named point, every inferred point takes its delta:
        // `prev` and `next` are the same point, so both branches of `infer`
        // agree. This is the case that moves a whole contour rigidly.
        for i in [0, 2, 3] {
            assert_eq!((d[i].x, d[i].y), (5.0, 5.0), "point {i}");
        }
    }

    #[test]
    fn phantom_points_are_never_interpolated() {
        // The four points past the contour are in no contour, so IUP must not
        // touch them however the flags fall. A reader that ran IUP over the
        // whole array would give the advance a delta the font never wrote.
        let pts = square();
        let mut d = deltas([(3.0, 3.0, true), NONE, NONE, NONE, NONE, NONE, NONE, NONE]);
        iup(&pts, &pts, &mut d);
        assert_eq!(&d[4..], &deltas([NONE; 8])[4..]);
    }

    #[test]
    fn interpolation_reads_the_original_positions_not_the_moved_ones() {
        // HarfBuzz interpolates along the points as they were before any
        // tuple that names its own points, not as earlier tuples left them.
        let orig = square();
        let mut moved = orig.clone();
        moved[1].x = 1000.0;
        let mut d = deltas([
            (0.0, 0.0, true),
            NONE,
            (10.0, 0.0, true),
            NONE,
            NONE,
            NONE,
            NONE,
            NONE,
        ]);
        iup(&moved, &orig, &mut d);
        assert_eq!(d[1].x, 10.0);
    }

    #[test]
    fn a_zero_coordinate_gives_zero_before_a_malformed_region_is_skipped() {
        // HarfBuzz 14.3.0 asks about the coordinate first: an axis at 0 that
        // the tuple peaks on gives 0 even when the region there is backwards
        // -- which on its own would be skipped, at full strength.
        let region = (alloc::vec![16384], alloc::vec![16384]);
        let r = Some((region.0.as_slice(), region.1.as_slice()));
        assert_eq!(calculate_scalar(&[8192], r, &[0]), 0.0);
        assert_eq!(calculate_scalar(&[8192], r, &[4096]), 1.0);
    }

    #[test]
    fn a_scalar_is_worked_in_double() {
        // One third, as `double` has it and then narrowed: the `f32`
        // division would round differently in the last place for some
        // values, and HarfBuzz divides in `double`.
        let s = calculate_scalar(&[12288], None, &[4096]);
        assert_eq!(s, 4096.0f64 / 12288.0);
    }

    #[test]
    fn packed_point_numbers_sum_past_sixteen_bits() {
        // Two word increments of 0xFFFF: HarfBuzz sums in 32 bits, so the
        // second point is 0x1FFFE -- past any glyph, and skipped where used
        // -- rather than wrapping round to 0xFFFE.
        let d = [0x02, 0x81, 0xFF, 0xFF, 0xFF, 0xFF];
        let mut p = 0;
        assert_eq!(
            hb_points(&d, &mut p, d.len()),
            Some(alloc::vec![0xFFFF, 0x1FFFE])
        );
        assert_eq!(p, d.len());
    }

    #[test]
    fn a_zero_count_is_every_point_and_an_empty_list() {
        let d = [0x00];
        let mut p = 0;
        assert_eq!(hb_points(&d, &mut p, 1), Some(Vec::new()));
    }

    #[test]
    fn packed_deltas_of_every_width() {
        // Bytes (-1, 2), words (0x0102), zeros (three), longs (0x00010000).
        let d = [
            0x01, 0xFF, 0x02, // two bytes
            0x40, 0x01, 0x02, // one word
            0x82, // three zeros
            0xC0, 0x00, 0x01, 0x00, 0x00, // one long
        ];
        let mut p = 0;
        assert_eq!(
            hb_deltas(&d, &mut p, d.len(), 7),
            Some(alloc::vec![-1, 2, 0x0102, 0, 0, 0, 0x10000])
        );
        assert_eq!(p, d.len());
    }

    #[test]
    fn packed_deltas_that_run_short_are_unreadable() {
        let d = [0x41, 0x00, 0x01, 0x00];
        let mut p = 0;
        assert_eq!(hb_deltas(&d, &mut p, d.len(), 2), None);
        // And a run past the count wanted.
        let mut p = 0;
        assert_eq!(hb_deltas(&[0x03, 1, 2, 3, 4], &mut p, 5, 2), None);
    }

    #[test]
    fn deltas_added_straight_in_are_scaled_in_f32() {
        let mut pts = [pt(1.0, 1.0), pt(2.0, 2.0)];
        // x: bytes (3, -3); y: zeros.
        let d = [0x01, 0x03, 0xFD, 0x81];
        let mut p = 0;
        add_deltas(&d, &mut p, d.len(), &mut pts, 0.5, true).unwrap();
        add_deltas(&d, &mut p, d.len(), &mut pts, 0.5, false).unwrap();
        assert_eq!(p, d.len());
        assert_eq!(
            (pts[0].x, pts[0].y, pts[1].x, pts[1].y),
            (2.5, 1.0, 0.5, 2.0)
        );
    }

    #[test]
    fn interpolation_follows_the_nearer_end_rather_than_extrapolating() {
        // Target outside both named points: take the nearer one's delta whole.
        // Extrapolating would send a stroke's end off the glyph.
        assert_eq!(infer(-5.0, 0.0, 10.0, 1.0, 2.0), 1.0);
        assert_eq!(infer(15.0, 0.0, 10.0, 1.0, 2.0), 2.0);
    }

    #[test]
    fn two_named_points_at_one_coordinate_give_no_ratio() {
        // Same position, same delta: that delta. Same position, different
        // deltas: the font is ambiguous and zero is the only neutral answer.
        assert_eq!(infer(5.0, 3.0, 3.0, 7.0, 7.0), 7.0);
        assert_eq!(infer(5.0, 3.0, 3.0, 7.0, 9.0), 0.0);
    }
}

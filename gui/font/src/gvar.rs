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
//! * **Phantom point values are zero**, and therefore so are the advance
//!   deltas they would carry. Every variable face on this host ships `HVAR`,
//!   which is the table advances are supposed to come from and which is the
//!   next step; a `gvar`-derived advance is the fallback for a face that has
//!   no `HVAR`, and inventing one now would mean two sources of truth for the
//!   same number before either is checked against a real file. The phantom
//!   points still exist here, at their correct indices, because the *point
//!   numbering* depends on them.
//! * **`CFF2`** varies through the charstring interpreter rather than through
//!   this table. No face on this host carries it (`variable_survey.py`), so it
//!   is not implemented and cannot be checked against a real file if it were.
//!
//! # References
//!
//! Transcribed against HarfBuzz's `hb-ot-var-gvar-table.hh` —
//! `calculate_scalar`, `unpack_points`, `unpack_deltas` and the IUP loop in
//! `apply_deltas_to_points` — because HarfBuzz is this crate's oracle
//! elsewhere and a glyph that differs from it by a unit is indistinguishable
//! from a glyph that differs from it by a bug.

use alloc::vec::Vec;

use crate::sfnt::{GlyphPoint, Span, u16_at, u32_at};

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

/// The four phantom points every glyph's variation data carries beyond its
/// contour points: horizontal origin, horizontal advance, vertical origin,
/// vertical advance.
pub(crate) const PHANTOM_COUNT: usize = 4;

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

    /// The accumulated delta for every point of `gid` at `coords`.
    ///
    /// `points` are the glyph's contour points **without** the phantom points;
    /// `ends` are the contour bounds from
    /// [`SimpleGlyph`](crate::sfnt::SimpleGlyph). The returned vector has
    /// `points.len() + PHANTOM_COUNT` entries, so a caller that wants only the
    /// contour deltas can truncate and one that later wants the advance deltas
    /// does not have to re-read the table.
    ///
    /// `None` when the glyph does not vary or its data is unreadable — both of
    /// which mean "draw the default instance", which is why they are not
    /// distinguished.
    pub(crate) fn deltas(
        &self,
        data: &[u8],
        gid: u16,
        coords: &[i16],
        points: &[GlyphPoint],
        ends: &[usize],
    ) -> Option<Vec<(f32, f32)>> {
        let span = self.glyph_span(data, gid)?;
        let g = data.get(span.off..span.off.checked_add(span.len)?)?;
        let total = points.len().checked_add(PHANTOM_COUNT)?;

        let count_word = u16_at(g, 0)?;
        let tuple_count = usize::from(count_word & TUPLE_COUNT_MASK);
        if tuple_count == 0 || tuple_count > MAX_TUPLES {
            return None;
        }
        let mut serial = usize::from(u16_at(g, 2)?);

        // The shared point list, if there is one, sits at the head of the
        // serialized data and every tuple that does not bring its own uses it.
        let shared_points = if count_word & SHARED_POINT_NUMBERS != 0 {
            Some(read_packed_points(g, &mut serial, total)?)
        } else {
            None
        };

        let mut out = alloc::vec![(0.0f32, 0.0f32); total];
        // Where the next tuple's header sits. Headers are variable-length
        // (an embedded peak and an intermediate region each add axis_count
        // words), so they must be walked rather than indexed.
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
            let scale = scalar(&peak, region.as_ref(), coords);
            // A tuple that scores zero contributes nothing, but its data still
            // has to be stepped over — the next tuple's data begins where this
            // one ends, not where its own header says. Skipping the *parse* and
            // not the *advance* is the bug this ordering avoids.
            if scale != 0.0 {
                let mut p = serial;
                let tuple_points = if index & PRIVATE_POINT_NUMBERS != 0 {
                    read_packed_points(g, &mut p, total)?
                } else {
                    shared_points.clone().unwrap_or(PointSet::All)
                };
                apply_tuple(
                    g,
                    &mut p,
                    &tuple_points,
                    scale,
                    points,
                    ends,
                    &mut out,
                    total,
                )?;
            }
            serial = tuple_end;
        }
        Some(out)
    }
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

/// Read one tuple's deltas and add them into `out`, interpolating the points
/// the tuple does not name.
#[allow(
    clippy::too_many_arguments,
    reason = "one tuple's whole context; \
    bundling it into a struct would name the same fields once more and \
    give the reader a second place to look"
)]
fn apply_tuple(
    g: &[u8],
    p: &mut usize,
    set: &PointSet,
    scale: f32,
    points: &[GlyphPoint],
    ends: &[usize],
    out: &mut [(f32, f32)],
    total: usize,
) -> Option<()> {
    match set {
        PointSet::All => {
            let xs = read_packed_deltas(g, p, total)?;
            let ys = read_packed_deltas(g, p, total)?;
            for (i, slot) in out.iter_mut().enumerate() {
                slot.0 += scale * f32::from(*xs.get(i)?);
                slot.1 += scale * f32::from(*ys.get(i)?);
            }
        }
        PointSet::Some(list) => {
            let n = list.len();
            let xs = read_packed_deltas(g, p, n)?;
            let ys = read_packed_deltas(g, p, n)?;

            // This tuple's deltas, laid out per point, with a flag for which
            // ones the tuple actually named. The flags are what IUP reads.
            let mut delta = alloc::vec![(0.0f32, 0.0f32); total];
            let mut named = alloc::vec![false; total];
            for (k, &num) in list.iter().enumerate() {
                let i = usize::from(num);
                // A point number past the end is a malformed tuple. Dropping
                // just that delta keeps the rest of the glyph, which is the
                // difference between a slightly wrong letter and none.
                let Some(slot) = delta.get_mut(i) else {
                    continue;
                };
                slot.0 = f32::from(*xs.get(k)?);
                slot.1 = f32::from(*ys.get(k)?);
                if let Some(f) = named.get_mut(i) {
                    *f = true;
                }
            }

            infer_unreferenced(points, ends, &named, &mut delta);

            for (i, slot) in out.iter_mut().enumerate() {
                let d = delta.get(i)?;
                slot.0 += scale * d.0;
                slot.1 += scale * d.1;
            }
        }
    }
    Some(())
}

/// Fill in deltas for the points a tuple did not name — IUP.
///
/// Within each contour, a run of unnamed points between two named ones takes
/// its delta by interpolating between them, using the points' *own* positions
/// as the parameter. The run may wrap around the end of the contour, which is
/// why this walks with a modular index rather than a slice.
///
/// Two degenerate cases are the ones that matter, and both are common: a
/// contour where every point is named needs nothing done, and one where none
/// is has nothing to interpolate from and must be left alone rather than
/// zeroed.
fn infer_unreferenced(
    points: &[GlyphPoint],
    ends: &[usize],
    named: &[bool],
    delta: &mut [(f32, f32)],
) {
    let mut start = 0usize;
    for &end_excl in ends {
        // `end` is the last point of the contour, inclusive, which is the form
        // the wrap-around arithmetic below needs.
        let Some(end) = end_excl.checked_sub(1) else {
            continue;
        };
        if end < start || end >= points.len() {
            start = end_excl;
            continue;
        }
        let len = end.saturating_sub(start).saturating_add(1);
        let referenced = (start..=end)
            .filter(|&i| named.get(i) == Some(&true))
            .count();
        if referenced == 0 || referenced == len {
            start = end_excl;
            continue;
        }

        let next = |i: usize| if i >= end { start } else { i.saturating_add(1) };
        let mut remaining = len.saturating_sub(referenced);
        let mut j = start;
        // Each pass finds one gap — a maximal run of unnamed points — and
        // fills it. `remaining` bounds the passes, so a malformed flag array
        // cannot spin here.
        'gaps: while remaining > 0 {
            // Walk to the last named point before a gap.
            let prev = loop {
                let i = j;
                j = next(i);
                if named.get(i) == Some(&true) && named.get(j) != Some(&true) {
                    break i;
                }
                if i == j {
                    break 'gaps;
                }
            };
            // Walk to the first named point after it.
            let mut i = prev;
            let after = loop {
                let k = i;
                i = next(k);
                if named.get(k) != Some(&true) && named.get(i) == Some(&true) {
                    break i;
                }
                if k == i {
                    break 'gaps;
                }
            };
            j = after;

            let mut k = next(prev);
            while k != after {
                let (Some(&pt), Some(&pv), Some(&nx)) =
                    (points.get(k), points.get(prev), points.get(after))
                else {
                    break 'gaps;
                };
                let (Some(&dp), Some(&dn)) = (delta.get(prev), delta.get(after)) else {
                    break 'gaps;
                };
                if let Some(slot) = delta.get_mut(k) {
                    slot.0 = infer(pt.p.x, pv.p.x, nx.p.x, dp.0, dn.0);
                    slot.1 = infer(pt.p.y, pv.p.y, nx.p.y, dp.1, dn.1);
                }
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    break;
                }
                k = next(k);
            }
        }
        start = end_excl;
    }
}

/// One coordinate's inferred delta.
///
/// Transcribed from HarfBuzz's `infer_delta`. The three special cases before
/// the interpolation are not optimizations: when the two named points sit at
/// the same coordinate there is no ratio to take, and when the target is
/// outside them the correct answer is to *follow the nearer one* rather than
/// extrapolate, which is what keeps a stroke's end from shooting off when only
/// its middle was named.
#[allow(
    clippy::float_cmp,
    reason = "`prev` and `next` are glyph point coordinates, which are integers \
              widened from `i16` and so compare exactly; the equality is not an \
              approximation test but the genuine question `are these two points \
              at the same coordinate`, which is what decides whether there is a \
              ratio to interpolate along at all"
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

/// How much of a tuple's deltas apply at `coords`.
///
/// 1 at the tuple's peak, tapering linearly to 0 at the edges of its region,
/// and 0 outside — multiplied across the axes, so a two-axis tuple contributes
/// only where *both* axes are inside.
///
/// Transcribed from HarfBuzz's `TupleVariationHeader::calculate_scalar`,
/// including its treatment of a malformed intermediate region: a region whose
/// start is past its peak, or whose peak is past its end, is *ignored for that
/// axis* rather than zeroing the tuple. Zeroing would drop a delta the font
/// meant; ignoring keeps it at full strength on that axis, which is what the
/// non-intermediate encoding of the same region would have done.
fn scalar(peak: &[i16], region: Option<&(Vec<i16>, Vec<i16>)>, coords: &[i16]) -> f32 {
    let mut scale = 1.0f32;
    for (i, &pk) in peak.iter().enumerate() {
        if pk == 0 {
            continue;
        }
        let v = coords.get(i).copied().unwrap_or(0);
        if v == pk {
            continue;
        }
        match region {
            Some((start, end)) => {
                let (Some(&s), Some(&e)) = (start.get(i), end.get(i)) else {
                    continue;
                };
                if s > pk || pk > e || (s < 0 && e > 0 && pk != 0) {
                    continue;
                }
                if v < s || v > e {
                    return 0.0;
                }
                // Each difference is taken *after* widening to `f32` rather
                // than as an `i16` subtraction: two F2Dot14 endpoints can be
                // 32768 apart, which is one past `i16::MAX`. `f32` holds every
                // integer to 2^24, so the widened form is exact as well as
                // total.
                if v < pk {
                    if pk != s {
                        scale *= (f32::from(v) - f32::from(s)) / (f32::from(pk) - f32::from(s));
                    }
                } else if pk != e {
                    scale *= (f32::from(e) - f32::from(v)) / (f32::from(e) - f32::from(pk));
                }
            }
            None => {
                if v == 0 || v < pk.min(0) || v > pk.max(0) {
                    return 0.0;
                }
                scale *= f32::from(v) / f32::from(pk);
            }
        }
    }
    scale
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
    use crate::sfnt::Point;

    // --- the scalar: how much of a region applies ---

    #[test]
    fn a_tuple_applies_fully_at_its_peak() {
        assert_eq!(scalar(&[16384], None, &[16384]), 1.0);
    }

    #[test]
    fn a_tuple_tapers_linearly_toward_the_default() {
        // Peak at +1, asked for the half-way point: half the delta.
        assert_eq!(scalar(&[16384], None, &[8192]), 0.5);
    }

    #[test]
    fn a_tuple_does_not_apply_at_the_default_instance() {
        assert_eq!(scalar(&[16384], None, &[0]), 0.0);
    }

    #[test]
    fn a_tuple_does_not_apply_across_the_default() {
        // A tuple peaking at +1 says nothing about the -1 side. Applying it
        // there would make asking for light produce a bolder shape.
        assert_eq!(scalar(&[16384], None, &[-8192]), 0.0);
    }

    #[test]
    fn an_axis_the_tuple_does_not_mention_is_ignored() {
        // Peak 0 on an axis means "any position", not "the default position".
        assert_eq!(scalar(&[0, 16384], None, &[16384, 16384]), 1.0);
    }

    #[test]
    fn axes_multiply_so_a_corner_tuple_needs_both() {
        // Half way along each of two axes is a quarter of the corner's delta.
        assert_eq!(scalar(&[16384, 16384], None, &[8192, 8192]), 0.25);
    }

    #[test]
    fn an_intermediate_region_tapers_between_its_own_bounds() {
        // Start 0, peak +0.5, end +1: at +0.25 we are half way up the rise.
        let region = (alloc::vec![0], alloc::vec![16384]);
        assert_eq!(scalar(&[8192], Some(&region), &[4096]), 0.5);
        assert_eq!(scalar(&[8192], Some(&region), &[8192]), 1.0);
        // And half way down the fall, which is a *different* slope because the
        // two sides of the region have different widths.
        assert_eq!(scalar(&[8192], Some(&region), &[12288]), 0.5);
    }

    #[test]
    fn an_intermediate_region_is_zero_outside_its_bounds() {
        let region = (alloc::vec![4096], alloc::vec![12288]);
        assert_eq!(scalar(&[8192], Some(&region), &[0]), 0.0);
        assert_eq!(scalar(&[8192], Some(&region), &[16384]), 0.0);
    }

    #[test]
    fn a_backwards_intermediate_region_is_ignored_not_zeroed() {
        // start > peak: malformed. HarfBuzz skips the axis, which leaves the
        // tuple at full strength there rather than dropping a delta the font
        // meant to apply.
        let region = (alloc::vec![16384], alloc::vec![16384]);
        assert_eq!(scalar(&[8192], Some(&region), &[4096]), 1.0);
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
    fn pt(x: f32, y: f32) -> GlyphPoint {
        GlyphPoint {
            p: Point::new(x, y),
            on_curve: true,
        }
    }

    /// A square contour, points at the corners.
    fn square() -> (Vec<GlyphPoint>, Vec<usize>) {
        (
            alloc::vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0),],
            alloc::vec![4],
        )
    }

    #[test]
    fn a_contour_with_every_point_named_is_left_alone() {
        let (pts, ends) = square();
        let mut delta = alloc::vec![(1.0, 2.0); 8];
        infer_unreferenced(
            &pts,
            &ends,
            &[true, true, true, true, false, false, false, false],
            &mut delta,
        );
        assert_eq!(&delta[..4], &[(1.0, 2.0); 4]);
    }

    #[test]
    fn a_contour_with_no_point_named_is_left_at_zero() {
        // Nothing to interpolate from. Leaving it alone is right; the tuple
        // simply does not move this contour.
        let (pts, ends) = square();
        let mut delta = alloc::vec![(0.0, 0.0); 8];
        infer_unreferenced(&pts, &ends, &[false; 8], &mut delta);
        assert_eq!(&delta[..4], &[(0.0, 0.0); 4]);
    }

    #[test]
    fn an_unnamed_point_between_two_named_ones_interpolates() {
        // Points at x = 0, 10, 10, 0. Name the first and third; the second
        // sits at the same x as the third, so it takes the third's delta.
        let (pts, ends) = square();
        let mut delta = alloc::vec![(0.0, 0.0); 8];
        delta[0] = (0.0, 0.0);
        delta[2] = (10.0, 0.0);
        infer_unreferenced(
            &pts,
            &ends,
            &[true, false, true, false, false, false, false, false],
            &mut delta,
        );
        // Point 1 is at x=10 like point 2, and y=0 like point 0.
        assert_eq!(delta[1].0, 10.0);
        assert_eq!(delta[1].1, 0.0);
    }

    #[test]
    fn a_gap_wraps_around_the_end_of_the_contour() {
        // Name only point 1. Points 2, 3 and 0 form one gap that wraps.
        let (pts, ends) = square();
        let mut delta = alloc::vec![(0.0, 0.0); 8];
        delta[1] = (5.0, 5.0);
        infer_unreferenced(
            &pts,
            &ends,
            &[false, true, false, false, false, false, false, false],
            &mut delta,
        );
        // With a single named point, every inferred point takes its delta:
        // `prev` and `next` are the same point, so both branches of `infer`
        // agree. This is the case that moves a whole contour rigidly.
        assert_eq!(delta[0], (5.0, 5.0));
        assert_eq!(delta[2], (5.0, 5.0));
        assert_eq!(delta[3], (5.0, 5.0));
    }

    #[test]
    fn phantom_points_are_never_interpolated() {
        // The four points past the contour are outside every contour bound, so
        // IUP must not touch them however the flags fall. A reader that ran
        // IUP over the whole array would give the advance a delta the font
        // never wrote.
        let (pts, ends) = square();
        let mut delta = alloc::vec![(0.0, 0.0); 8];
        delta[0] = (3.0, 3.0);
        infer_unreferenced(
            &pts,
            &ends,
            &[true, false, false, false, false, false, false, false],
            &mut delta,
        );
        assert_eq!(&delta[4..], &[(0.0, 0.0); 4]);
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

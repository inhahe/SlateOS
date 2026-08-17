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
//! # Where the coordinates live
//!
//! On the caller's side, not on the [`Face`](crate::sfnt::Face). A parsed face
//! is shared and must be usable at two instances at once — a document showing
//! the same family at Regular and Bold has one file open — so the axes and the
//! mapping (properties of the *file*) are parsed once onto the face, while the
//! chosen point (a property of the *request*) rides with the scaled font.
//!
//! # Fixed point, on purpose
//!
//! The `avar` remap is done in integer `F2Dot14` with truncating division,
//! matching HarfBuzz's `SegmentMaps::map` operation for operation rather than
//! recomputing it in floating point. The two disagree by a unit on values that
//! land near a segment boundary, and a unit of `F2Dot14` is a real difference
//! in the delta that comes out the far end. Since HarfBuzz is this crate's
//! oracle everywhere else, agreeing with it is worth more than being a
//! fraction more accurate in isolation.

use alloc::vec::Vec;

use crate::sfnt::{Span, i16_at, tag_at, u16_at};

/// One `F2Dot14` unit — the denominator normalized coordinates are stored over.
const ONE: i32 = 16384;

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
/// variation table is indexed by.
///
/// Made only by [`Variations::normalize`] and its siblings, so that a value of
/// this type is always the output of the full pipeline (clamp, normalize,
/// `avar`) and never a raw user number that happens to be in range. The inner
/// vector is one entry per axis, in `fvar` order, which is the order every
/// variation region in the file is written in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coords(Vec<i16>);

impl Coords {
    /// The normalized coordinates, in `fvar` axis order.
    #[must_use]
    pub fn as_slice(&self) -> &[i16] {
        &self.0
    }

    /// How many axes this position covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether this position covers no axes at all — which is what a
    /// non-variable face yields.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
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
        self.0.iter().all(|&c| c == 0)
    }

    /// One axis's coordinate, or `None` past the end.
    #[must_use]
    pub fn get(&self, axis: usize) -> Option<i16> {
        self.0.get(axis).copied()
    }
}

/// The `avar` remap for one axis: a piecewise-linear curve through
/// `(from, to)` points, both in normalized `F2Dot14`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SegmentMap {
    points: Vec<(i16, i16)>,
}

impl SegmentMap {
    /// Remap one normalized coordinate through this curve.
    ///
    /// Transcribed from HarfBuzz's `SegmentMaps::map`, including its truncating
    /// integer division — see the module docs for why agreeing beats rounding
    /// better.
    fn map(&self, value: i16) -> i16 {
        let n = self.points.len();
        // A curve of fewer than two points cannot interpolate. The spec
        // requires at least the three identity points, so this is a malformed
        // face; the identity is the safe reading, since it is what the absence
        // of the whole table would have meant.
        if n < 2 {
            return value;
        }
        let (Some(&(first_from, first_to)), Some(&(last_from, last_to))) =
            (self.points.first(), self.points.last())
        else {
            return value;
        };
        if value <= first_from {
            return first_to;
        }
        if value >= last_from {
            return last_to;
        }
        // The curve is sorted by `from`, so the first point at or past `value`
        // bounds the segment `value` falls in. Both `first` and `last` are
        // already excluded above, so this index is in 1..n and both lookups
        // below are in range.
        let mut i = 1;
        while i < n {
            let Some(&(from, to)) = self.points.get(i) else {
                return value;
            };
            if value == from {
                return to;
            }
            if value < from {
                break;
            }
            i = i.saturating_add(1);
        }
        let (Some(&(lo_from, lo_to)), Some(&(hi_from, hi_to))) =
            (self.points.get(i.wrapping_sub(1)), self.points.get(i))
        else {
            return value;
        };
        // Every term below is a difference of two `i16`s widened to `i32`, so
        // it fits in 17 bits and the saturating forms are exact — they are
        // here to satisfy the crate's no-bare-arithmetic rule, not because a
        // real face can reach the boundary. Writing them as saturating rather
        // than suppressing the lint keeps that rule meaningful everywhere
        // else, where the boundary *is* reachable.
        let span = i32::from(hi_from).saturating_sub(i32::from(lo_from));
        if span == 0 {
            // Two points at one `from`: the curve is vertical here and the
            // face has not said which side wins. Take the lower, which is the
            // value already reached by walking up to it.
            return lo_to;
        }
        let rise = i32::from(hi_to).saturating_sub(i32::from(lo_to));
        let run = i32::from(value).saturating_sub(i32::from(lo_from));
        // Truncating division, deliberately: HarfBuzz's `SegmentMaps::map`
        // divides the same way, and this crate is checked against HarfBuzz
        // glyph-for-glyph. Rounding to nearest here would be marginally more
        // accurate and would disagree with the oracle by one F2Dot14 unit on
        // some inputs, which is the worse trade.
        //
        // `span` is non-zero by the check above, so `checked_div` can only
        // decline on `i32::MIN / -1`, unreachable for the same width reason.
        // Zero is the right fallback regardless: it yields `lo_to`, matching
        // the vertical-segment case.
        let delta = rise.saturating_mul(run).checked_div(span).unwrap_or(0);
        clamp_f2dot14(i32::from(lo_to).saturating_add(delta))
    }
}

/// Everything the file says about its own variation, parsed once.
///
/// `None` on a [`Face`](crate::sfnt::Face) means the face is not variable, which
/// is 549 of this host's 556.
#[derive(Clone, Debug, PartialEq)]
pub struct Variations {
    axes: Vec<Axis>,
    instances: Vec<Instance>,
    /// One per axis, in `fvar` order. Empty when the face carries no `avar`,
    /// which is the identity mapping — distinct from a per-axis empty
    /// `SegmentMap`, which means the face has an `avar` that declines to
    /// correct *that* axis.
    segments: Vec<SegmentMap>,
}

impl Variations {
    /// Parse `fvar`, and `avar` if the face has one.
    ///
    /// Returns `None` rather than an error for a face whose `fvar` is
    /// unreadable: a malformed variation table should cost the face its
    /// variability, not its ability to draw. Every other table is still good,
    /// and the default instance is a complete, correct font.
    ///
    /// An `avar` that disagrees with `fvar` about the axis count is discarded
    /// whole rather than used for the axes they agree on. The disagreement
    /// means one of the two is not what it claims, and there is no way to tell
    /// which — pairing curve *k* with axis *k* across a length mismatch is a
    /// guess that silently applies the weight correction to the width.
    #[must_use]
    pub(crate) fn parse(data: &[u8], fvar: Span, avar: Option<Span>) -> Option<Self> {
        let (axes, instances) = parse_fvar(data, fvar)?;
        let segments = match avar {
            Some(span) => parse_avar(data, span, axes.len()).unwrap_or_default(),
            None => Vec::new(),
        };
        Some(Self {
            axes,
            instances,
            segments,
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

    /// Whether the face carries an `avar` correction.
    #[must_use]
    pub fn has_avar(&self) -> bool {
        !self.segments.is_empty()
    }

    /// Index of the axis with this tag.
    #[must_use]
    pub fn axis_index(&self, tag: &[u8; 4]) -> Option<usize> {
        self.axes.iter().position(|a| &a.tag == tag)
    }

    /// The position where every axis sits at its default — normalized all-zero
    /// by construction, whatever the user-space defaults are.
    #[must_use]
    pub fn default_coords(&self) -> Coords {
        Coords(alloc::vec![0i16; self.axes.len()])
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
        Coords(
            self.axes
                .iter()
                .enumerate()
                .map(|(i, axis)| {
                    let value = user.get(i).copied().unwrap_or(axis.default);
                    self.normalize_axis(i, axis, value)
                })
                .collect(),
        )
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
        Coords(
            self.axes
                .iter()
                .enumerate()
                .map(|(i, axis)| {
                    let value = requested
                        .iter()
                        .rev()
                        .find(|(tag, _)| *tag == axis.tag)
                        .map_or(axis.default, |&(_, v)| v);
                    self.normalize_axis(i, axis, value)
                })
                .collect(),
        )
    }

    /// The normalized position of one of the face's named instances.
    #[must_use]
    pub fn instance_coords(&self, index: usize) -> Option<Coords> {
        let instance = self.instances.get(index)?;
        Some(self.normalize(&instance.coords))
    }

    /// Clamp, normalize to -1..1 about the default, then apply `avar`.
    fn normalize_axis(&self, index: usize, axis: &Axis, value: f32) -> i16 {
        let normalized = normalize_value(axis, value);
        // `roundf`'s half-away-from-zero, which is what HarfBuzz uses to reach
        // F2Dot14 from a float.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "clamped to +/-1 above, so the product is within i32 and \
                      then clamped to F2Dot14's range"
        )]
        let raw = clamp_f2dot14((normalized * ONE as f32).round() as i32);
        match self.segments.get(index) {
            Some(map) => map.map(raw),
            None => raw,
        }
    }
}

/// Map a user-space value onto -1..=1 about the axis default.
///
/// Kept apart from the `avar` step and from the fixed-point conversion because
/// it is the one piece with a specified formula, and because a degenerate axis
/// — one whose default equals its minimum or its maximum, which several real
/// faces have (`ReemKufi`'s `wght` is 400..400..700) — has to divide by a range
/// of zero on one side and must answer 0 rather than an infinity.
fn normalize_value(axis: &Axis, value: f32) -> f32 {
    // NaN cannot be ordered, so it would slip through both comparisons below
    // and reach the arithmetic. Treat it as "nothing was asked for".
    if value.is_nan() {
        return 0.0;
    }
    let value = value.clamp(axis.min.min(axis.max), axis.max.max(axis.min));
    if value < axis.default {
        let range = axis.default - axis.min;
        if range > 0.0 {
            (value - axis.default) / range
        } else {
            0.0
        }
    } else if value > axis.default {
        let range = axis.max - axis.default;
        if range > 0.0 {
            (value - axis.default) / range
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// Clamp to `F2Dot14`'s representable -2..2 and narrow to `i16`.
///
/// Normalized coordinates are defined on -1..=1, so anything outside is either
/// a malformed `avar` or arithmetic that has gone wrong; clamping to the
/// *format's* range rather than to -1..1 keeps a face that deliberately maps
/// past the nominal end working, while still guaranteeing the narrowing cannot
/// wrap.
fn clamp_f2dot14(v: i32) -> i16 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "clamped to i16's range on the line above"
    )]
    {
        v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    }
}

/// Read a `Fixed` (16.16) at `off`.
fn fixed_at(data: &[u8], off: usize) -> Option<f32> {
    let hi = i16_at(data, off)?;
    let lo = u16_at(data, off.checked_add(2)?)?;
    Some(f32::from(hi) + f32::from(lo) / 65536.0)
}

/// Read `fvar` into its axes and named instances.
fn parse_fvar(data: &[u8], span: Span) -> Option<(Vec<Axis>, Vec<Instance>)> {
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
    for i in 0..axis_count {
        let rec = axes_off.checked_add(i.checked_mul(axis_size)?)?;
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
    let instances = if instance_size < min_instance || instance_count > MAX_INSTANCES {
        Vec::new()
    } else {
        let base = axes_off.checked_add(axis_count.checked_mul(axis_size)?)?;
        let mut out = Vec::with_capacity(instance_count);
        for i in 0..instance_count {
            let Some(rec) = base
                .checked_add(i.saturating_mul(instance_size))
                .filter(|r| r.checked_add(min_instance).is_some_and(|e| e <= table.len()))
            else {
                // A truncated instance array costs the instances past the cut,
                // not the axes: the axes are what drawing needs.
                break;
            };
            let mut coords = Vec::with_capacity(axis_count);
            for a in 0..axis_count {
                let Some(v) = rec
                    .checked_add(4)
                    .and_then(|o| o.checked_add(a.checked_mul(4)?))
                    .and_then(|o| fixed_at(table, o))
                else {
                    break;
                };
                coords.push(v);
            }
            if coords.len() != axis_count {
                break;
            }
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

    Some((axes, instances))
}

/// Read `avar` into one segment map per axis.
///
/// `None` when the table is unreadable or disagrees with `fvar` about how many
/// axes there are; the caller treats that as "no correction", which is the same
/// thing the table's absence means.
fn parse_avar(data: &[u8], span: Span, axis_count: usize) -> Option<Vec<SegmentMap>> {
    let table = data.get(span.off..span.off.checked_add(span.len)?)?;
    if u16_at(table, 0)? != 1 {
        return None;
    }
    if usize::from(u16_at(table, 6)?) != axis_count {
        return None;
    }

    let mut maps = Vec::with_capacity(axis_count);
    let mut pos = 8usize;
    for _ in 0..axis_count {
        let count = usize::from(u16_at(table, pos)?);
        pos = pos.checked_add(2)?;
        let mut points = Vec::with_capacity(count.min(MAX_AXES.saturating_mul(64)));
        let mut previous: Option<i16> = None;
        let mut sorted = true;
        for _ in 0..count {
            let from = i16_at(table, pos)?;
            let to = i16_at(table, pos.checked_add(2)?)?;
            pos = pos.checked_add(4)?;
            if previous.is_some_and(|p| from < p) {
                sorted = false;
            }
            previous = Some(from);
            points.push((from, to));
        }
        // `map` binary-walks the points assuming they ascend by `from`. An
        // unsorted curve is malformed; taking it as the identity is the honest
        // reading, since interpolating through it would produce a correction
        // the designer never described. Only that axis is dropped — the other
        // axes' curves are independent and still good.
        maps.push(if sorted {
            SegmentMap { points }
        } else {
            SegmentMap::default()
        });
    }
    Some(maps)
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
    // is later multiplied by 16384 to land on an F2Dot14 integer. An
    // approximate comparison here would pass on a `normalize_value` that
    // returned 0.9999997 for the maximum, which rounds to 16383 and leaves the
    // face one unit short of the weight the user asked for, forever.
    #![allow(
        clippy::float_cmp,
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )]

    use super::*;

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

    fn vars(axes: Vec<Axis>, segments: Vec<SegmentMap>) -> Variations {
        Variations {
            axes,
            instances: Vec::new(),
            segments,
        }
    }

    // --- normalize_value: the specified formula ---

    #[test]
    fn default_normalizes_to_zero_whatever_the_user_scale() {
        // The point of the normalized space: 400 on a 300..400..700 axis and
        // 10.5 on a 5..10.5..36 axis are both exactly 0.
        assert_eq!(normalize_value(&axis(b"wght", 300.0, 400.0, 700.0), 400.0), 0.0);
        assert_eq!(normalize_value(&axis(b"opsz", 5.0, 10.5, 36.0), 10.5), 0.0);
    }

    #[test]
    fn ends_normalize_to_plus_and_minus_one() {
        let a = axis(b"wght", 300.0, 400.0, 700.0);
        assert_eq!(normalize_value(&a, 300.0), -1.0);
        assert_eq!(normalize_value(&a, 700.0), 1.0);
    }

    #[test]
    fn the_two_sides_have_independent_scales() {
        // 300..400..700: one step down covers 100 units, one step up covers
        // 300. Half way *down* is 350, half way *up* is 550 -- a linear map
        // over the whole range would put both at the wrong place.
        let a = axis(b"wght", 300.0, 400.0, 700.0);
        assert_eq!(normalize_value(&a, 350.0), -0.5);
        assert_eq!(normalize_value(&a, 550.0), 0.5);
    }

    #[test]
    fn values_outside_the_range_clamp_rather_than_extrapolate() {
        let a = axis(b"wght", 300.0, 400.0, 700.0);
        assert_eq!(normalize_value(&a, 50.0), -1.0);
        assert_eq!(normalize_value(&a, 5000.0), 1.0);
    }

    #[test]
    fn a_degenerate_side_yields_zero_rather_than_an_infinity() {
        // ReemKufi really ships this: wght 400..400..700, so the whole
        // below-default side has zero width and 400 is simultaneously the
        // minimum and the default.
        let a = axis(b"wght", 400.0, 400.0, 700.0);
        assert_eq!(normalize_value(&a, 400.0), 0.0);
        assert_eq!(normalize_value(&a, 300.0), 0.0, "clamped to the minimum, which is the default");
        assert_eq!(normalize_value(&a, 700.0), 1.0);
        assert!(normalize_value(&a, 550.0).is_finite());
    }

    #[test]
    fn a_fully_degenerate_axis_is_always_zero() {
        let a = axis(b"wght", 400.0, 400.0, 400.0);
        for v in [0.0, 400.0, 1000.0, f32::MAX] {
            assert_eq!(normalize_value(&a, v), 0.0);
        }
    }

    #[test]
    fn nan_is_treated_as_no_request() {
        // NaN compares false against everything, so it would slip past both
        // branch tests and reach the arithmetic.
        let a = axis(b"wght", 300.0, 400.0, 700.0);
        assert_eq!(normalize_value(&a, f32::NAN), 0.0);
    }

    #[test]
    fn infinities_clamp_to_the_ends() {
        let a = axis(b"wght", 300.0, 400.0, 700.0);
        assert_eq!(normalize_value(&a, f32::INFINITY), 1.0);
        assert_eq!(normalize_value(&a, f32::NEG_INFINITY), -1.0);
    }

    // --- fixed point ---

    #[test]
    fn normalization_reaches_exact_f2dot14_ends() {
        let v = vars(alloc::vec![axis(b"wght", 300.0, 400.0, 700.0)], Vec::new());
        assert_eq!(v.normalize(&[300.0]).as_slice(), &[-16384]);
        assert_eq!(v.normalize(&[400.0]).as_slice(), &[0]);
        assert_eq!(v.normalize(&[700.0]).as_slice(), &[16384]);
    }

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

    // --- avar ---

    fn segment(points: &[(i16, i16)]) -> SegmentMap {
        SegmentMap {
            points: points.to_vec(),
        }
    }

    #[test]
    fn an_identity_avar_changes_nothing() {
        let map = segment(&[(-16384, -16384), (0, 0), (16384, 16384)]);
        for v in [-16384, -8192, 0, 4096, 16384] {
            assert_eq!(map.map(v), v);
        }
    }

    #[test]
    fn avar_bends_the_middle_of_the_axis() {
        // The real shape: a face whose Semibold is not half way between
        // Regular and Bold says so by moving the midpoint.
        let map = segment(&[(-16384, -16384), (0, 0), (8192, 4096), (16384, 16384)]);
        assert_eq!(map.map(0), 0, "the default must stay the default");
        assert_eq!(map.map(8192), 4096);
        // Half way between 0 and 8192 maps half way between 0 and 4096.
        assert_eq!(map.map(4096), 2048);
        // And half way between 8192 and 16384 maps half way up the steeper
        // second segment, which is *not* the linear reading of the whole axis.
        assert_eq!(map.map(12288), 10240);
    }

    #[test]
    fn avar_clamps_at_the_ends_of_its_curve() {
        let map = segment(&[(-8192, -16384), (0, 0), (8192, 16384)]);
        assert_eq!(map.map(-16384), -16384);
        assert_eq!(map.map(16384), 16384);
    }

    #[test]
    fn an_avar_landing_exactly_on_a_point_takes_that_points_value() {
        let map = segment(&[(-16384, -16384), (0, 0), (8192, 4096), (16384, 16384)]);
        assert_eq!(map.map(8192), 4096);
    }

    #[test]
    fn a_curve_too_short_to_interpolate_is_the_identity() {
        assert_eq!(segment(&[]).map(1234), 1234);
        assert_eq!(segment(&[(0, 0)]).map(1234), 1234);
    }

    #[test]
    fn a_vertical_segment_does_not_divide_by_zero() {
        let map = segment(&[(-16384, -16384), (0, -4096), (0, 4096), (16384, 16384)]);
        assert_eq!(map.map(0), -4096, "the first point at that coordinate wins");
        assert!(map.map(1).abs() <= 16384);
    }

    #[test]
    fn avar_is_applied_through_normalize() {
        let map = segment(&[(-16384, -16384), (0, 0), (8192, 4096), (16384, 16384)]);
        let v = vars(
            alloc::vec![axis(b"wght", 300.0, 400.0, 700.0)],
            alloc::vec![map],
        );
        assert!(v.has_avar());
        // wght 550 is +0.5 normalized (8192), which the curve pulls to 4096.
        assert_eq!(v.normalize(&[550.0]).as_slice(), &[4096]);
        // And the default is still exactly the default.
        assert_eq!(v.normalize(&[400.0]).as_slice(), &[0]);
    }

    // --- the tag-keyed entry point ---

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
        let v = vars(alloc::vec![axis(b"wght", 300.0, 400.0, 700.0)], Vec::new());
        let c = v.normalize_tags(&[(tags::WDTH, 50.0), (tags::WGHT, 700.0)]);
        assert_eq!(c.as_slice(), &[16384]);
    }

    #[test]
    fn a_repeated_tag_takes_the_last_request() {
        let v = vars(alloc::vec![axis(b"wght", 300.0, 400.0, 700.0)], Vec::new());
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

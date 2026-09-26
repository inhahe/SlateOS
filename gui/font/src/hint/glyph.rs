//! One glyph, analysed for hinting and moved to follow its hinted edges:
//! FreeType's `afhints.c`, for the vertical dimension only.
//!
//! [`Hints::load`] is `af_glyph_hints_reload`: it scales the glyph's points,
//! links each contour into a ring and classifies every point by the
//! directions it arrives from and leaves in -- which points begin and end
//! the straight runs the edges are built from, and which are *weak* (curve
//! controls, points partway along a run, the middle of a flat corner) and
//! so will only ever be interpolated. [`Hints::align_edge_points`],
//! [`Hints::align_strong_points`] and [`Hints::align_weak_points`] are the
//! three passes that then move every point once the Latin module has placed
//! the edges: points on an edge go with it, other strong points are
//! interpolated between the edges either side, and weak points between their
//! touched neighbours along the contour, as TrueType's `IUP` does.
//!
//! Horizontal positions are never touched -- light hinting is vertical only
//! -- so nothing horizontal is kept but the font-unit `x` the analysis reads.
//!
//! Indices, not pointers, link everything: a point's ring neighbours, a
//! segment's first and last point, an edge's segments. Every access goes
//! through `get`, and a failed one abandons the glyph (it is then drawn
//! unhinted) -- which no consistent input produces, but which keeps a bug
//! here from ever becoming a crash of the compositor that calls it.
//!
//! Portions of this file are copyright (C) 2003-2023 by David Turner, Robert
//! Wilhelm and Werner Lemberg, from The FreeType Project (www.freetype.org).
//! Used under the FreeType License: see `gui/font/licenses/FTL.TXT`.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "bounded operands, as in `fixed`: coordinates within i16 and \
              scales within fixed::MAX_SCALE, checked in `Hints::load`"
)]

use alloc::vec::Vec;

use super::fixed::{MAX_SCALE, corner_is_flat, div_fix, mul_fix};
use crate::sfnt::{Tag, TaggedOutline};

/// No direction, or too diagonal to have one (`AF_DIR_NONE`).
pub(super) const DIR_NONE: i8 = 4;
/// Rightward (`AF_DIR_RIGHT`).
pub(super) const DIR_RIGHT: i8 = 1;
/// Leftward (`AF_DIR_LEFT`).
pub(super) const DIR_LEFT: i8 = -1;
/// Upward (`AF_DIR_UP`).
pub(super) const DIR_UP: i8 = 2;
/// Downward (`AF_DIR_DOWN`).
pub(super) const DIR_DOWN: i8 = -2;

/// A quadratic control point (`AF_FLAG_CONIC`).
const FLAG_CONIC: u16 = 1 << 0;
/// A cubic control point (`AF_FLAG_CUBIC`).
const FLAG_CUBIC: u16 = 1 << 1;
/// Either kind of control point (`AF_FLAG_CONTROL`).
pub(super) const FLAG_CONTROL: u16 = FLAG_CONIC | FLAG_CUBIC;
/// Moved vertically already (`AF_FLAG_TOUCH_Y`).
const FLAG_TOUCH_Y: u16 = 1 << 3;
/// Only ever interpolated (`AF_FLAG_WEAK_INTERPOLATION`).
const FLAG_WEAK: u16 = 1 << 4;

/// A round segment or edge: a curve's turning point (`AF_EDGE_ROUND`).
pub(super) const EDGE_ROUND: u8 = 1 << 0;
/// An edge that serifs another (`AF_EDGE_SERIF`).
pub(super) const EDGE_SERIF: u8 = 1 << 1;
/// An edge already placed (`AF_EDGE_DONE`).
pub(super) const EDGE_DONE: u8 = 1 << 2;
/// An edge aligned to a neutral blue zone (`AF_EDGE_NEUTRAL`).
pub(super) const EDGE_NEUTRAL: u8 = 1 << 3;

/// The most points the hinter takes on. FreeType gives up at a thousand
/// segments; a glyph of this many points is detail no pixel grid resolves.
const MAX_POINTS: usize = 1 << 14;

/// One point of the glyph (`AF_PointRec`).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Point {
    pub(super) flags: u16,
    pub(super) in_dir: i8,
    pub(super) out_dir: i8,
    /// Font units.
    pub(super) fx: i64,
    pub(super) fy: i64,
    /// `fy` scaled, 26.6.
    pub(super) oy: i64,
    /// The hinted vertical position, 26.6.
    pub(super) y: i64,
    /// Scratch, as FreeType uses it: a segment's position and coordinate
    /// while segments are found, the current and original position during
    /// the weak-point pass.
    pub(super) u: i64,
    pub(super) v: i64,
    /// The ring neighbours within the contour.
    pub(super) prev: usize,
    pub(super) next: usize,
}

impl Point {
    /// Whether this is a curve's control point.
    pub(super) const fn is_control(&self) -> bool {
        self.flags & FLAG_CONTROL != 0
    }
}

/// A run of points heading the same horizontal way (`AF_SegmentRec`).
#[derive(Clone, Copy, Debug)]
pub(super) struct Segment {
    pub(super) flags: u8,
    pub(super) dir: i8,
    /// Height, font units: the middle of the run's vertical spread.
    pub(super) pos: i64,
    /// Half that spread.
    pub(super) delta: i64,
    /// The run's horizontal extent, font units.
    pub(super) min_coord: i64,
    pub(super) max_coord: i64,
    pub(super) height: i64,
    pub(super) edge: Option<usize>,
    /// The next segment of the same edge, in a ring.
    pub(super) edge_next: Option<usize>,
    /// The segment across the stem this one is a side of.
    pub(super) link: Option<usize>,
    pub(super) serif: Option<usize>,
    pub(super) score: i64,
    /// The run's first and last point.
    pub(super) first: usize,
    pub(super) last: usize,
}

/// Segments at one height, taken together (`AF_EdgeRec`).
#[derive(Clone, Copy, Debug)]
pub(super) struct Edge {
    /// Font units.
    pub(super) fpos: i64,
    /// Scaled, 26.6.
    pub(super) opos: i64,
    /// Hinted, 26.6.
    pub(super) pos: i64,
    pub(super) flags: u8,
    pub(super) dir: i8,
    /// Where the blue zone this edge snaps to puts it. FreeType keeps a
    /// pointer to the zone's reference or overshoot line and only ever reads
    /// its fitted position, which is what this holds.
    pub(super) blue: Option<i64>,
    pub(super) link: Option<usize>,
    pub(super) serif: Option<usize>,
    /// The first and last segment of the edge's ring.
    pub(super) first: usize,
    pub(super) last: usize,
}

/// A glyph being hinted (`AF_GlyphHintsRec`), vertical axis only.
#[derive(Clone, Debug)]
pub(super) struct Hints {
    pub(super) points: Vec<Point>,
    /// Each contour's first point; a contour is contiguous in `points`.
    pub(super) contours: Vec<usize>,
    /// The vertical scale, 16.16: the size's, after the x-height nudge.
    pub(super) y_scale: i64,
    /// Which horizontal direction a bottom edge runs in -- left for a
    /// TrueType outline (outer contours clockwise), right for a PostScript
    /// one: the vertical axis's `major_dir`.
    pub(super) major_dir: i8,
    pub(super) units_per_em: i64,
    pub(super) segments: Vec<Segment>,
    pub(super) edges: Vec<Edge>,
}

/// A coordinate in whole font units, as FreeType holds it (`FT_Short`), or
/// `None` beyond that range -- which the hinter then does not touch.
///
/// Rounded half up, as FreeType's `FT_fixedToInt` rounds a varied coordinate:
/// outlines from `glyf` are integers already, and only an instance of a
/// variable font or a CFF charstring's fractions have anything to round.
pub(super) fn font_unit(v: f32) -> Option<i64> {
    let r = (v + 0.5).floor();
    (r.is_finite() && (-32768.0..=32767.0).contains(&r)).then_some(r as i64)
}

/// The direction of a vector, if it is within about four degrees of one of
/// the four (`af_direction_compute`).
pub(super) fn direction(dx: i64, dy: i64) -> i8 {
    let (dir, long, short) = if dy >= dx {
        if dy >= -dx {
            (DIR_UP, dy, dx)
        } else {
            (DIR_LEFT, -dx, dy)
        }
    } else if dy >= -dx {
        (DIR_RIGHT, dx, dy)
    } else {
        (DIR_DOWN, -dy, dx)
    };
    // The long arm is never negative; 14 is about 4.1 degrees.
    if long <= 14 * short.abs() {
        DIR_NONE
    } else {
        dir
    }
}

/// Whether the outline's outer contours run counter-clockwise, as PostScript
/// draws them: `FT_Outline_Get_Orientation`, which sums the signed area of the
/// polygon the points span (controls included) in coordinates shifted down to
/// fifteen bits.
fn is_postscript(points: &[Point], contours: &[usize]) -> bool {
    let (mut x_min, mut x_max, mut y_min, mut y_max) = (i64::MAX, i64::MIN, i64::MAX, i64::MIN);
    for p in points {
        x_min = x_min.min(p.fx);
        x_max = x_max.max(p.fx);
        y_min = y_min.min(p.fy);
        y_max = y_max.max(p.fy);
    }
    if x_min == x_max || y_min == y_max {
        return false;
    }
    let msb = |v: i64| 63 - i64::from(v.max(1).leading_zeros());
    let x_shift = (msb(x_max.abs() | x_min.abs()) - 14).max(0);
    let y_shift = (msb(y_max - y_min) - 14).max(0);
    let mut area: i64 = 0;
    for (c, &first) in contours.iter().enumerate() {
        let end = contours.get(c + 1).copied().unwrap_or(points.len());
        let Some(last) = points.get(end.wrapping_sub(1)) else {
            continue;
        };
        let (mut px, mut py) = (last.fx >> x_shift, last.fy >> y_shift);
        for p in points.get(first..end).unwrap_or(&[]) {
            let (cx, cy) = (p.fx >> x_shift, p.fy >> y_shift);
            area += (cy - py) * (cx + px);
            (px, py) = (cx, cy);
        }
    }
    area > 0
}

impl Hints {
    /// Load `outline` for vertical hinting at `y_scale` (16.16):
    /// `af_glyph_hints_reload`.
    ///
    /// `None` for a glyph the hinter leaves alone: no points, more than
    /// [`MAX_POINTS`], a coordinate beyond `i16` or an empty contour (which
    /// FreeType rejects as a malformed outline), or a scale outside the range
    /// the arithmetic is proved for.
    pub(super) fn load(outline: &TaggedOutline, y_scale: i64, units_per_em: i64) -> Option<Self> {
        let n = outline.points.len();
        if n == 0 || n > MAX_POINTS || outline.tags.len() != n {
            return None;
        }
        if !(1..=MAX_SCALE).contains(&y_scale) || !(16..=16384).contains(&units_per_em) {
            return None;
        }
        let mut points = Vec::with_capacity(n);
        for (p, tag) in outline.points.iter().zip(&outline.tags) {
            let (fx, fy) = (font_unit(p.x)?, font_unit(p.y)?);
            let oy = mul_fix(fy, y_scale);
            points.push(Point {
                flags: match tag {
                    Tag::On => 0,
                    Tag::Conic => FLAG_CONIC,
                    Tag::Cubic => FLAG_CUBIC,
                },
                in_dir: DIR_NONE,
                out_dir: DIR_NONE,
                fx,
                fy,
                oy,
                y: oy,
                ..Point::default()
            });
        }
        // Each contour a ring: its first point's `prev` is its last.
        let mut contours = Vec::with_capacity(outline.ends.len());
        let mut start = 0usize;
        for &end in &outline.ends {
            if end <= start || end > n {
                return None;
            }
            for i in start..end {
                let p = points.get_mut(i)?;
                p.prev = if i == start { end - 1 } else { i - 1 };
                p.next = if i + 1 == end { start } else { i + 1 };
            }
            contours.push(start);
            start = end;
        }
        if start != n {
            return None;
        }

        let major_dir = if is_postscript(&points, &contours) {
            DIR_RIGHT
        } else {
            DIR_LEFT
        };
        let mut hints = Self {
            points,
            contours,
            y_scale,
            major_dir,
            units_per_em,
            segments: Vec::new(),
            edges: Vec::new(),
        };
        hints.classify()?;
        Some(hints)
    }

    fn pt(&self, i: usize) -> Option<&Point> {
        self.points.get(i)
    }

    fn pt_mut(&mut self, i: usize) -> Option<&mut Point> {
        self.points.get_mut(i)
    }

    /// The directions in and out of every point, and which points are weak:
    /// the second half of `af_glyph_hints_reload`.
    ///
    /// Distances between very near points accumulate: a run of points closer
    /// together than `near_limit` is read as one vector, its inner points are
    /// weak, and they take its direction.
    fn classify(&mut self) -> Option<()> {
        let n = self.points.len();
        // Twenty units of a 2048 em, in this face's units.
        let near_limit = 20 * self.units_per_em / 2048;
        let near_limit2 = 2 * near_limit - 1;
        // FreeType keeps these as index deltas in `u` and `v`; here they are
        // indices: the next and previous points that are not near.
        let mut next_far: Vec<usize> = alloc::vec![0; n];
        let mut prev_far: Vec<usize> = alloc::vec![0; n];

        for c in 0..self.contours.len() {
            let first0 = *self.contours.get(c)?;
            // The contour's first point may be in the middle of a run of near
            // points: step back to where the run starts.
            let mut point = first0;
            let mut prev = self.pt(first0)?.prev;
            while prev != first0 {
                let (p, q) = (self.pt(point)?, self.pt(prev)?);
                if (p.fx - q.fx).abs() + (p.fy - q.fy).abs() >= near_limit2 {
                    break;
                }
                point = prev;
                prev = self.pt(prev)?.prev;
            }
            let first = point;

            let mut curr = first;
            *next_far.get_mut(curr)? = first;
            *prev_far.get_mut(first)? = curr;
            let (mut out_x, mut out_y) = (0i64, 0i64);
            let mut next = first;
            loop {
                let point = next;
                next = self.pt(point)?.next;
                let (p, q) = (self.pt(point)?, self.pt(next)?);
                out_x += q.fx - p.fx;
                out_y += q.fy - p.fy;
                if out_x.abs() + out_y.abs() < near_limit {
                    self.pt_mut(next)?.flags |= FLAG_WEAK;
                    if next == first {
                        break;
                    }
                    continue;
                }
                *next_far.get_mut(curr)? = next;
                *prev_far.get_mut(next)? = curr;
                let out_dir = direction(out_x, out_y);
                // Every point between takes the accumulated direction.
                self.pt_mut(curr)?.out_dir = out_dir;
                curr = self.pt(curr)?.next;
                let mut guard = n;
                while curr != next {
                    let p = self.pt_mut(curr)?;
                    p.in_dir = out_dir;
                    p.out_dir = out_dir;
                    curr = p.next;
                    guard = guard.checked_sub(1)?;
                }
                self.pt_mut(next)?.in_dir = out_dir;
                *next_far.get_mut(curr)? = first;
                *prev_far.get_mut(first)? = curr;
                out_x = 0;
                out_y = 0;
                if next == first {
                    break;
                }
            }
        }

        // A run of diagonal vectors into one quadrant is one long vector: the
        // points along it are of no topological interest.
        for i in 0..n {
            let p = *self.pt(i)?;
            if p.flags & FLAG_WEAK != 0 || p.in_dir != DIR_NONE || p.out_dir != DIR_NONE {
                continue;
            }
            let (nu, pv) = (*next_far.get(i)?, *prev_far.get(i)?);
            let (next_u, prev_v) = (*self.pt(nu)?, *self.pt(pv)?);
            let (in_x, in_y) = (p.fx - prev_v.fx, p.fy - prev_v.fy);
            let (out_x, out_y) = (next_u.fx - p.fx, next_u.fy - p.fy);
            if (in_x ^ out_x) >= 0 && (in_y ^ out_y) >= 0 {
                self.pt_mut(i)?.flags |= FLAG_WEAK;
                *next_far.get_mut(pv)? = nu;
                *prev_far.get_mut(nu)? = pv;
            }
        }

        // Everything not caught in an edge later is strong, except controls,
        // points partway along a straight run, flat corners and spikes.
        for i in 0..n {
            let p = *self.pt(i)?;
            if p.flags & FLAG_WEAK != 0 {
                continue;
            }
            let weak = if p.is_control() {
                true
            } else if p.out_dir == p.in_dir {
                if p.out_dir == DIR_NONE {
                    let (nu, pv) = (*next_far.get(i)?, *prev_far.get(i)?);
                    let (next_u, prev_v) = (*self.pt(nu)?, *self.pt(pv)?);
                    let flat = corner_is_flat(
                        p.fx - prev_v.fx,
                        p.fy - prev_v.fy,
                        next_u.fx - p.fx,
                        next_u.fy - p.fy,
                    );
                    if flat {
                        *next_far.get_mut(pv)? = nu;
                        *prev_far.get_mut(nu)? = pv;
                    }
                    flat
                } else {
                    true
                }
            } else {
                p.in_dir == -p.out_dir
            };
            if weak {
                self.pt_mut(i)?.flags |= FLAG_WEAK;
            }
        }
        Some(())
    }

    /// Put every point of an edge's segments at the edge's hinted height:
    /// `af_glyph_hints_align_edge_points`.
    pub(super) fn align_edge_points(&mut self) -> Option<()> {
        for s in 0..self.segments.len() {
            let seg = *self.segments.get(s)?;
            let Some(e) = seg.edge else {
                continue;
            };
            let pos = self.edges.get(e)?.pos;
            let mut point = seg.first;
            let mut guard = self.points.len();
            loop {
                let p = self.pt_mut(point)?;
                p.y = pos;
                p.flags |= FLAG_TOUCH_Y;
                if point == seg.last {
                    break;
                }
                point = p.next;
                guard = guard.checked_sub(1)?;
            }
        }
        Some(())
    }

    /// Interpolate every untouched strong point between the edges above and
    /// below it, or shift it with the nearer one outside them all:
    /// `af_glyph_hints_align_strong_points` (TrueType's `IP`).
    pub(super) fn align_strong_points(&mut self) -> Option<()> {
        let (Some(&first), Some(&last)) = (self.edges.first(), self.edges.last()) else {
            return Some(());
        };
        let count = self.edges.len();
        for i in 0..self.points.len() {
            let p = *self.pt(i)?;
            if p.flags & (FLAG_TOUCH_Y | FLAG_WEAK) != 0 {
                continue;
            }
            let (u, ou) = (p.fy, p.oy);
            let y = if first.fpos - u >= 0 {
                first.pos - (first.opos - ou)
            } else if u - last.fpos >= 0 {
                last.pos + (ou - last.opos)
            } else {
                // The first edge at or above the point: a linear search for a
                // handful of edges, a binary one beyond, as FreeType does --
                // the two can disagree about which of several edges at one
                // height is found, and FreeType's choice is the one to match.
                let mut on_edge = None;
                let mut min = 0usize;
                if count <= 8 {
                    let nn = self.edges.iter().position(|e| e.fpos >= u).unwrap_or(count);
                    if let Some(e) = self.edges.get(nn)
                        && e.fpos == u
                    {
                        on_edge = Some(e.pos);
                    }
                    min = nn;
                } else {
                    let mut max = count;
                    while min < max {
                        let mid = (max + min) >> 1;
                        let e = self.edges.get(mid)?;
                        match u.cmp(&e.fpos) {
                            core::cmp::Ordering::Less => max = mid,
                            core::cmp::Ordering::Greater => min = mid + 1,
                            core::cmp::Ordering::Equal => {
                                on_edge = Some(e.pos);
                                break;
                            }
                        }
                    }
                }
                match on_edge {
                    Some(pos) => pos,
                    None => {
                        let before = *self.edges.get(min.checked_sub(1)?)?;
                        let after = *self.edges.get(min)?;
                        let scale = div_fix(after.pos - before.pos, after.fpos - before.fpos);
                        before.pos + mul_fix(u - before.fpos, scale)
                    }
                }
            };
            let p = self.pt_mut(i)?;
            p.y = y;
            p.flags |= FLAG_TOUCH_Y;
        }
        Some(())
    }

    /// Interpolate every point still untouched between its nearest touched
    /// neighbours along the contour: `af_glyph_hints_align_weak_points`
    /// (TrueType's `IUP`).
    pub(super) fn align_weak_points(&mut self) -> Option<()> {
        for p in &mut self.points {
            p.u = p.y;
            p.v = p.oy;
        }
        let touched = |h: &Self, i: usize| h.pt(i).is_some_and(|p| p.flags & FLAG_TOUCH_Y != 0);
        for c in 0..self.contours.len() {
            let first_point = *self.contours.get(c)?;
            let end_point = self.pt(first_point)?.prev;
            let mut point = first_point;
            // The first touched point; none means nothing to interpolate from.
            loop {
                if point > end_point {
                    break;
                }
                if touched(self, point) {
                    break;
                }
                point += 1;
            }
            if point > end_point {
                continue;
            }
            let first_touched = point;
            let last_touched;
            loop {
                while point < end_point && touched(self, point + 1) {
                    point += 1;
                }
                let lt = point;
                // The next touched point, if any before the contour ends.
                point += 1;
                while point <= end_point && !touched(self, point) {
                    point += 1;
                }
                if point > end_point {
                    last_touched = lt;
                    break;
                }
                self.iup_interp(lt + 1, point - 1, lt, point)?;
            }
            if last_touched == first_touched {
                self.iup_shift(first_point, end_point, first_touched)?;
            } else {
                if last_touched < end_point {
                    self.iup_interp(last_touched + 1, end_point, last_touched, first_touched)?;
                }
                if first_touched > 0 {
                    self.iup_interp(first_point, first_touched - 1, last_touched, first_touched)?;
                }
            }
        }
        for p in &mut self.points {
            p.y = p.u;
        }
        Some(())
    }

    /// Move points `p1..=p2` (all but `reference`) by `reference`'s shift:
    /// `af_iup_shift`.
    fn iup_shift(&mut self, p1: usize, p2: usize, reference: usize) -> Option<()> {
        let r = *self.pt(reference)?;
        let delta = r.u - r.v;
        if delta == 0 {
            return Some(());
        }
        for i in (p1..reference).chain(reference + 1..=p2) {
            let p = self.pt_mut(i)?;
            p.u = p.v + delta;
        }
        Some(())
    }

    /// Interpolate points `p1..=p2` between two touched points, shifting those
    /// outside their span with the nearer: `af_iup_interp`.
    fn iup_interp(&mut self, p1: usize, p2: usize, ref1: usize, ref2: usize) -> Option<()> {
        if p1 > p2 {
            return Some(());
        }
        let (mut a, mut b) = (*self.pt(ref1)?, *self.pt(ref2)?);
        if a.v > b.v {
            core::mem::swap(&mut a, &mut b);
        }
        let (v1, v2, u1, u2) = (a.v, b.v, a.u, b.u);
        let (d1, d2) = (u1 - v1, u2 - v2);
        let scale = (u1 != u2 && v1 != v2).then(|| div_fix(u2 - u1, v2 - v1));
        for i in p1..=p2 {
            let p = self.pt_mut(i)?;
            let u = p.v;
            p.u = if u <= v1 {
                u + d1
            } else if u >= v2 {
                u + d2
            } else {
                match scale {
                    Some(scale) => u1 + mul_fix(u - v1, scale),
                    None => u1,
                }
            };
        }
        Some(())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test fixtures"
)]
mod tests {
    use super::*;
    use crate::sfnt::Point as P;

    /// A clockwise (TrueType) rectangle.
    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> TaggedOutline {
        TaggedOutline {
            points: alloc::vec![
                P::new(x0, y0),
                P::new(x0, y1),
                P::new(x1, y1),
                P::new(x1, y0)
            ],
            tags: alloc::vec![Tag::On; 4],
            ends: alloc::vec![4],
        }
    }

    #[test]
    fn directions_are_within_four_degrees_or_none() {
        assert_eq!(direction(100, 0), DIR_RIGHT);
        assert_eq!(direction(-100, 7), DIR_LEFT);
        assert_eq!(direction(0, 100), DIR_UP);
        assert_eq!(direction(7, -100), DIR_DOWN);
        assert_eq!(direction(100, 8), DIR_NONE);
        assert_eq!(direction(0, 0), DIR_NONE);
    }

    #[test]
    fn a_clockwise_outline_is_truetype_and_a_counter_clockwise_one_is_not() {
        let cw = Hints::load(&rect(0.0, 0.0, 100.0, 50.0), 0x10000, 1000).unwrap();
        assert_eq!(cw.major_dir, DIR_LEFT);
        let mut ccw = rect(0.0, 0.0, 100.0, 50.0);
        ccw.points.reverse();
        assert_eq!(
            Hints::load(&ccw, 0x10000, 1000).unwrap().major_dir,
            DIR_RIGHT
        );
    }

    #[test]
    fn a_rectangles_corners_are_strong_and_its_sides_have_directions() {
        let h = Hints::load(&rect(0.0, 0.0, 100.0, 50.0), 0x10000, 1000).unwrap();
        // Up the left side, right along the top, down, left along the bottom.
        let dirs: Vec<(i8, i8)> = h.points.iter().map(|p| (p.in_dir, p.out_dir)).collect();
        assert_eq!(
            dirs,
            [
                (DIR_LEFT, DIR_UP),
                (DIR_UP, DIR_RIGHT),
                (DIR_RIGHT, DIR_DOWN),
                (DIR_DOWN, DIR_LEFT)
            ]
        );
        assert!(h.points.iter().all(|p| p.flags & FLAG_WEAK == 0));
    }

    #[test]
    fn control_points_and_points_midway_along_a_line_are_weak() {
        let o = TaggedOutline {
            points: alloc::vec![
                P::new(0.0, 0.0),
                P::new(0.0, 50.0),
                P::new(0.0, 100.0),
                P::new(50.0, 150.0),
                P::new(100.0, 100.0),
                P::new(100.0, 0.0),
            ],
            tags: alloc::vec![Tag::On, Tag::On, Tag::On, Tag::Conic, Tag::On, Tag::On],
            ends: alloc::vec![6],
        };
        let h = Hints::load(&o, 0x10000, 1000).unwrap();
        let weak: Vec<bool> = h.points.iter().map(|p| p.flags & FLAG_WEAK != 0).collect();
        assert_eq!(weak, [false, true, false, true, false, false]);
    }

    #[test]
    fn a_glyph_beyond_sixteen_bits_or_with_an_empty_contour_is_refused() {
        assert!(Hints::load(&rect(0.0, 0.0, 40000.0, 10.0), 0x10000, 1000).is_none());
        let mut bad = rect(0.0, 0.0, 10.0, 10.0);
        bad.ends = alloc::vec![0, 4];
        assert!(Hints::load(&bad, 0x10000, 1000).is_none());
        assert!(Hints::load(&TaggedOutline::default(), 0x10000, 1000).is_none());
    }

    #[test]
    fn weak_points_between_touched_ones_are_interpolated() {
        // A four-point contour; touch the bottom and top, and the points
        // between follow proportionally.
        let o = TaggedOutline {
            points: alloc::vec![
                P::new(0.0, 0.0),
                P::new(0.0, 100.0),
                P::new(0.0, 200.0),
                P::new(0.0, 300.0),
            ],
            tags: alloc::vec![Tag::On; 4],
            ends: alloc::vec![4],
        };
        let mut h = Hints::load(&o, 0x10000, 1000).unwrap();
        for p in &mut h.points {
            p.flags = 0;
        }
        h.points[0].flags |= FLAG_TOUCH_Y;
        h.points[0].y = 10;
        h.points[3].flags |= FLAG_TOUCH_Y;
        h.points[3].y = 310;
        h.align_weak_points().unwrap();
        let ys: Vec<i64> = h.points.iter().map(|p| p.y).collect();
        assert_eq!(ys, [10, 110, 210, 310]);
    }
}

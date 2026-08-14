//! Anti-aliased scanline rasterization of glyph outlines.
//!
//! [`sfnt`](crate::sfnt) turns a font file into resolution-independent
//! outlines; this module turns an outline into an 8-bit coverage bitmap at a
//! chosen pixel size. Together they are everything needed to draw real text.
//!
//! # The algorithm, and why this one
//!
//! The obvious approach — supersample, count sub-pixel hits, divide — costs
//! N× the fill rate for N× the samples and still stair-steps at low N. The
//! approach used here instead computes *exact* per-pixel area coverage
//! analytically, in one pass over the edges, with no supersampling at all.
//!
//! It is the signed-area accumulation scheme popularised by Raph Levien's
//! `font-rs` (and used, in essentially this form, by libgd's and stb's
//! newer rasterizers). The idea:
//!
//! 1. Keep a float accumulator per pixel, one row of padding wide.
//! 2. For each line segment, walk the scanlines it crosses. For each, add
//!    to the accumulator the *signed* area the segment contributes to each
//!    pixel it touches — positive for a downward edge, negative for upward.
//! 3. Sweep each row left to right taking a running sum. At any pixel the
//!    running sum is the signed winding-weighted coverage; `abs()` clamped
//!    to 1 turns that into an alpha value.
//!
//! Step 3 is what makes it cheap: an edge only ever writes to the two-ish
//! pixels it actually crosses, and the interior of the shape is filled by
//! the prefix sum rather than by any per-pixel work proportional to area.
//!
//! Taking the absolute value gives non-zero-winding fill, which is what
//! TrueType requires: a counter-wound inner contour (the bowl of an 'o',
//! the counter of an 'A') sums back to zero and so becomes a hole.
//!
//! # Robustness
//!
//! Outlines arrive from untrusted font files, so coordinates can be absurd.
//! Every accumulator write is bounds-checked, the output size is capped
//! ([`MAX_GLYPH_PIXELS`]), and non-finite coordinates are rejected rather
//! than converted to garbage indices — an `as` cast of `NaN` to an integer
//! is 0 in Rust, which would silently smear ink at the origin.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::sfnt::{Outline, PathCmd, Point};

/// Upper bound on the pixels one rasterized glyph may occupy.
///
/// 16 mebipixels is far larger than any glyph a UI will ask for (a 4096 px
/// em square is 16.7 M on its own) but small enough that a font declaring
/// nonsense coordinates cannot ask us to allocate gigabytes. The limit is on
/// area rather than each dimension so that a legitimately wide-but-short
/// glyph is not rejected.
pub const MAX_GLYPH_PIXELS: usize = 16 * 1024 * 1024;

/// Why an outline could not be rasterized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterError {
    /// A coordinate was NaN or infinite.
    NonFiniteCoordinate,
    /// The requested pixel size was zero, negative or not finite.
    InvalidScale,
    /// The glyph would exceed [`MAX_GLYPH_PIXELS`].
    TooLarge,
}

impl fmt::Display for RasterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteCoordinate => f.write_str("outline contains a non-finite coordinate"),
            Self::InvalidScale => f.write_str("pixel size must be finite and positive"),
            Self::TooLarge => f.write_str("rasterized glyph would be too large"),
        }
    }
}

/// A rasterized glyph: 8-bit coverage plus where to put it.
///
/// The bitmap is positioned by `left`/`top`, both measured from the pen
/// position on the baseline, with y growing *downward* as in a framebuffer.
/// A glyph with a descender therefore has `top + height > 0`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlyphMask {
    /// Bitmap width in pixels.
    pub width: u32,
    /// Bitmap height in pixels.
    pub height: u32,
    /// X offset of the bitmap's left edge from the pen position.
    pub left: i32,
    /// Y offset of the bitmap's top edge from the baseline, downward-positive
    /// (so it is normally negative — the ink is above the baseline).
    pub top: i32,
    /// Row-major coverage, one byte per pixel, 0 = clear, 255 = solid.
    pub coverage: Vec<u8>,
}

impl GlyphMask {
    /// Coverage at `(x, y)`, or 0 outside the bitmap.
    #[must_use]
    pub fn at(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        let idx = (y as usize)
            .checked_mul(self.width as usize)
            .and_then(|r| r.checked_add(x as usize));
        idx.and_then(|i| self.coverage.get(i)).copied().unwrap_or(0)
    }

    /// True when the glyph produced no pixels (a space, for instance).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

// ---------------------------------------------------------------------------
// Curve flattening
// ---------------------------------------------------------------------------

/// Subdivision count for a quadratic bezier, chosen from its flatness.
///
/// The deviation of a quadratic from the chord between its endpoints is
/// governed by `p0 - 2c + p1`; a nearly straight curve needs one segment
/// and a tightly curved one needs several. Using the fourth root of the
/// squared deviation is Levien's estimate: error falls as the square of the
/// segment count, so the count should rise as the square root of the
/// deviation magnitude — i.e. the fourth root of its square.
fn quad_segments(p0: Point, ctrl: Point, p1: Point) -> u32 {
    // Tolerance in pixels. 0.1 px of chord error is well below what an
    // 8-bit coverage value can express, so tightening it further only costs
    // time.
    const TOLERANCE: f32 = 3.0;
    let dev_x = p0.x - 2.0 * ctrl.x + p1.x;
    let dev_y = p0.y - 2.0 * ctrl.y + p1.y;
    let dev_sq = dev_x.mul_add(dev_x, dev_y * dev_y);
    if dev_sq < 0.333 {
        return 1;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // sqrt(sqrt(x)) of a bounded positive float; clamped to a sane range
    // immediately afterwards, so the cast cannot produce a surprising value.
    let n = (TOLERANCE * dev_sq).sqrt().sqrt().floor() as i64;
    u32::try_from(n.clamp(1, 256)).unwrap_or(1)
}

/// Subdivision count for a cubic bezier, on the same error budget as
/// [`quad_segments`].
///
/// The chord error over a sub-interval of length `h` is at most
/// `h²·max|B''|/8`. A quadratic's second derivative is the constant
/// `2(p0 - 2c + p1)`; a cubic's is `6[(1-t)·d1 + t·d2]` with
/// `d1 = p0 - 2c1 + c2` and `d2 = c1 - 2c2 + p1`, so it is bounded by
/// `6·max(|d1|, |d2|)` — three times as large as a quadratic's for the same
/// deviation magnitude. Holding the error fixed therefore needs `sqrt(3)`
/// times as many segments, which is why the constant here is `3⁴ = 81` times
/// [`quad_segments`]'s: the count is a fourth root, so a factor of `sqrt(3)`
/// in `n` is a factor of `9` inside it, applied to a `dev_sq` that is itself
/// a square.
fn cubic_segments(p0: Point, c1: Point, c2: Point, p1: Point) -> u32 {
    const TOLERANCE: f32 = 27.0;
    let d1x = p0.x - 2.0 * c1.x + c2.x;
    let d1y = p0.y - 2.0 * c1.y + c2.y;
    let d2x = c1.x - 2.0 * c2.x + p1.x;
    let d2y = c1.y - 2.0 * c2.y + p1.y;
    let dev_sq = d1x
        .mul_add(d1x, d1y * d1y)
        .max(d2x.mul_add(d2x, d2y * d2y));
    // Below this the formula yields less than one segment anyway.
    if dev_sq < 1.0 / 27.0 {
        return 1;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // As in `quad_segments`: a fourth root of a bounded positive float,
    // clamped straight afterwards.
    let n = (TOLERANCE * dev_sq).sqrt().sqrt().floor() as i64;
    u32::try_from(n.clamp(1, 256)).unwrap_or(1)
}

// ---------------------------------------------------------------------------
// The accumulator
// ---------------------------------------------------------------------------

/// Signed-area accumulator over a `width x height` pixel grid.
///
/// The backing buffer is one column wider than the output so that an edge
/// landing exactly on the right boundary has somewhere to deposit its
/// remainder instead of being clipped (which would leave the row's running
/// sum non-zero and streak the whole row).
struct Accumulator {
    width: usize,
    height: usize,
    stride: usize,
    cells: Vec<f32>,
}

impl Accumulator {
    fn new(width: usize, height: usize) -> Option<Self> {
        let stride = width.checked_add(2)?;
        let cells_len = stride.checked_mul(height)?;
        Some(Self {
            width,
            height,
            stride,
            cells: vec![0.0; cells_len],
        })
    }

    /// Add `value` to the cell at column `xi` of the row starting at
    /// `row_start`, ignoring writes that fall outside the grid.
    ///
    /// Clipping rather than clamping is deliberate: an edge that runs off
    /// the left of the buffer must still contribute its winding to the
    /// pixels it *does* cross, but folding its area onto column 0 would
    /// paint a spurious vertical bar there.
    fn add(&mut self, row_start: usize, xi: i32, value: f32) {
        let Ok(xi) = usize::try_from(xi) else { return };
        if xi >= self.stride {
            return;
        }
        let Some(cell) = row_start
            .checked_add(xi)
            .and_then(|i| self.cells.get_mut(i))
        else {
            return;
        };
        *cell += value;
    }

    /// Convert a grid dimension or row index to `f32` without losing anything.
    ///
    /// An `f32` represents every integer up to 2^24 exactly, and
    /// [`MAX_GLYPH_PIXELS`] is exactly 2^24, so no accumulator dimension or
    /// row index can round here. The bound is enforced rather than assumed:
    /// a value above it saturates, which can only make an edge clip earlier,
    /// never reach further into the buffer.
    #[allow(clippy::cast_precision_loss)]
    fn exact_f32(v: usize) -> f32 {
        v.min(MAX_GLYPH_PIXELS) as f32
    }

    /// Accumulate one line segment's contribution.
    ///
    /// Adapted from Raph Levien's `font-rs` (`src/raster.rs`), with every
    /// buffer write routed through the bounds-checked [`Accumulator::add`]
    /// so that an out-of-range outline cannot index out of the buffer.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn line(&mut self, from: Point, to: Point) {
        // A perfectly horizontal edge contributes no winding anywhere; it
        // also has an infinite dx/dy, so it must be dropped before the
        // division below.
        if (from.y - to.y).abs() < f32::EPSILON {
            return;
        }
        // Normalise to top-to-bottom, remembering the winding direction.
        let (dir, p0, p1) = if from.y < to.y {
            (1.0_f32, from, to)
        } else {
            (-1.0_f32, to, from)
        };
        let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
        if !dxdy.is_finite() {
            return;
        }

        let mut x = p0.x;
        // Skip the part of the edge above the buffer, advancing x to where
        // it crosses y = 0.
        let y_start = if p0.y < 0.0 {
            x -= p0.y * dxdy;
            0usize
        } else {
            let f = p0.y.floor();
            if f >= Self::exact_f32(self.height) {
                return;
            }
            f as usize
        };
        let y_end = {
            let c = p1.y.ceil();
            if c <= 0.0 {
                return;
            }
            if c >= Self::exact_f32(self.height) {
                self.height
            } else {
                c as usize
            }
        };

        for y in y_start..y_end {
            let Some(row_start) = y.checked_mul(self.stride) else {
                return;
            };
            let y_f = Self::exact_f32(y);
            // The vertical extent of the edge within this scanline.
            let dy = (y_f + 1.0).min(p1.y) - y_f.max(p0.y);
            if dy <= 0.0 {
                // The edge does not actually reach into this scanline, so it
                // deposits no area and `x` advances by `dxdy * dy` == 0.
                continue;
            }
            let x_next = dxdy.mul_add(dy, x);
            let d = dy * dir;
            let (x0, x1) = if x < x_next { (x, x_next) } else { (x_next, x) };
            let x0_floor = x0.floor();
            let x1_ceil = x1.ceil();
            if !x0_floor.is_finite() || !x1_ceil.is_finite() {
                return;
            }
            let x0i = x0_floor as i64;
            let x1i = x1_ceil as i64;
            let (Ok(x0i), Ok(x1i)) = (i32::try_from(x0i), i32::try_from(x1i)) else {
                return;
            };

            if x1i <= x0i.saturating_add(1) {
                // The segment stays within a single pixel column on this
                // scanline: split its area between that column and the next
                // in proportion to how far right its midpoint sits.
                let xmf = 0.5 * (x + x_next) - x0_floor;
                self.add(row_start, x0i, d - d * xmf);
                self.add(row_start, x0i.saturating_add(1), d * xmf);
            } else {
                // The segment spans several columns. `s` is the area per
                // full column; the first and last columns get partial
                // triangles, everything between gets the full slice.
                let s = (x1 - x0).recip();
                let x0f = x0 - x0_floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1_ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.add(row_start, x0i, d * a0);
                if x1i == x0i.saturating_add(2) {
                    self.add(row_start, x0i.saturating_add(1), d * (1.0 - a0 - am));
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.add(row_start, x0i.saturating_add(1), d * (a1 - a0));
                    for xi in (x0i.saturating_add(2))..(x1i.saturating_sub(1)) {
                        self.add(row_start, xi, d * s);
                    }
                    let span = f32::from(
                        i16::try_from(x1i.saturating_sub(x0i).saturating_sub(3)).unwrap_or(0),
                    );
                    let a2 = span.mul_add(s, a1);
                    self.add(row_start, x1i.saturating_sub(1), d * (1.0 - a2 - am));
                }
                self.add(row_start, x1i, d * am);
            }
            x = x_next;
        }
    }

    /// Sweep each row into 8-bit coverage.
    ///
    /// The running sum is reset per row rather than carried across the whole
    /// buffer (which is what `font-rs` does). Carrying it is marginally
    /// faster and correct only while every edge lands inside the buffer;
    /// since we clip out-of-range writes, a clipped row could otherwise end
    /// with a non-zero sum and bleed a solid bar into the row below.
    fn into_coverage(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width.saturating_mul(self.height));
        for row in 0..self.height {
            let row_start = row.saturating_mul(self.stride);
            let mut acc = 0.0_f32;
            for col in 0..self.width {
                acc += row_start
                    .checked_add(col)
                    .and_then(|i| self.cells.get(i))
                    .copied()
                    .unwrap_or(0.0);
                // abs(): non-zero winding. A counter-wound inner contour
                // cancels to zero here, which is how counters become holes.
                let a = acc.abs().min(1.0);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                // `a` is in [0, 1] and finite, so the product is in [0, 255].
                let byte = (a * 255.0 + 0.5) as u8;
                out.push(byte);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// The pixel-space extent of an outline.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

/// Scale an outline into pixels and measure it.
///
/// Returns `None` when the outline encloses no points at all (an empty
/// command list, or nothing but `Close`), which is not an error — it simply
/// draws nothing.
///
/// # Errors
///
/// [`RasterError::NonFiniteCoordinate`] if any point is NaN or infinite.
fn outline_bounds(outline: &Outline, scale: f32) -> Result<Option<Bounds>, RasterError> {
    let to_px = |p: Point| Point::new(p.x * scale, -p.y * scale);

    let mut b = Bounds {
        min_x: f32::INFINITY,
        min_y: f32::INFINITY,
        max_x: f32::NEG_INFINITY,
        max_y: f32::NEG_INFINITY,
    };
    // Every coordinate is tested as it arrives rather than testing the final
    // box, because `f32::min`/`f32::max` return the *non*-NaN operand: a NaN
    // point would vanish into a perfectly finite-looking bounding box and the
    // glyph would silently rasterize as empty instead of being rejected.
    let mut nonfinite = false;
    let mut points = 0usize;
    let mut note = |p: Point| {
        if !p.x.is_finite() || !p.y.is_finite() {
            nonfinite = true;
            return;
        }
        points = points.saturating_add(1);
        b.min_x = b.min_x.min(p.x);
        b.min_y = b.min_y.min(p.y);
        b.max_x = b.max_x.max(p.x);
        b.max_y = b.max_y.max(p.y);
    };
    for cmd in &outline.commands {
        match *cmd {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => note(to_px(p)),
            PathCmd::QuadTo(ctrl, p) => {
                note(to_px(ctrl));
                note(to_px(p));
            }
            PathCmd::CurveTo(c1, c2, p) => {
                note(to_px(c1));
                note(to_px(c2));
                note(to_px(p));
            }
            PathCmd::Close => {}
        }
    }
    if nonfinite {
        return Err(RasterError::NonFiniteCoordinate);
    }
    Ok((points > 0).then_some(b))
}

/// Append a quadratic Bézier to `acc` as a fan of line segments.
fn flatten_quad(acc: &mut Accumulator, from: Point, ctrl: Point, to: Point) {
    let steps = quad_segments(from, ctrl, to);
    let inv = 1.0 / f32::from(u16::try_from(steps).unwrap_or(1));
    let mut prev = from;
    for i in 1..=steps {
        let t = f32::from(u16::try_from(i).unwrap_or(1)) * inv;
        let mt = 1.0 - t;
        // de Casteljau, written out: B(t) = (1-t)^2 from + 2(1-t)t ctrl + t^2 to
        let bx = mt.mul_add(mt * from.x, (2.0 * mt * t).mul_add(ctrl.x, t * t * to.x));
        let by = mt.mul_add(mt * from.y, (2.0 * mt * t).mul_add(ctrl.y, t * t * to.y));
        let pt = Point::new(bx, by);
        acc.line(prev, pt);
        prev = pt;
    }
}

/// Append a cubic Bézier to `acc` as a fan of line segments.
fn flatten_cubic(acc: &mut Accumulator, from: Point, c1: Point, c2: Point, to: Point) {
    let steps = cubic_segments(from, c1, c2, to);
    let inv = 1.0 / f32::from(u16::try_from(steps).unwrap_or(1));
    let mut prev = from;
    for i in 1..=steps {
        let t = f32::from(u16::try_from(i).unwrap_or(1)) * inv;
        let mt = 1.0 - t;
        // B(t) = (1-t)^3 from + 3(1-t)^2 t c1 + 3(1-t) t^2 c2 + t^3 to
        let w0 = mt * mt * mt;
        let w1 = 3.0 * mt * mt * t;
        let w2 = 3.0 * mt * t * t;
        let w3 = t * t * t;
        let bx = w0.mul_add(from.x, w1.mul_add(c1.x, w2.mul_add(c2.x, w3 * to.x)));
        let by = w0.mul_add(from.y, w1.mul_add(c1.y, w2.mul_add(c2.y, w3 * to.y)));
        let pt = Point::new(bx, by);
        acc.line(prev, pt);
        prev = pt;
    }
}

/// Rasterize an outline at `scale` pixels per font unit.
///
/// `scale` is what [`Face::scale_for_px`](crate::sfnt::Face::scale_for_px)
/// returns — multiply font units by it to get pixels. The outline's y axis
/// (up-positive, baseline at zero) is flipped to the bitmap's (down-positive,
/// origin at the bitmap's top-left corner) here, so callers work in
/// framebuffer coordinates throughout.
///
/// An empty outline yields an empty [`GlyphMask`], not an error.
///
/// # Errors
///
/// [`RasterError::InvalidScale`] for a non-positive or non-finite scale,
/// [`RasterError::NonFiniteCoordinate`] when the outline contains NaN or an
/// infinity, and [`RasterError::TooLarge`] when the result would exceed
/// [`MAX_GLYPH_PIXELS`].
pub fn rasterize(outline: &Outline, scale: f32) -> Result<GlyphMask, RasterError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(RasterError::InvalidScale);
    }
    if outline.is_empty() {
        return Ok(GlyphMask::default());
    }

    // Scale into pixels and flip y in one pass, so nothing downstream has to
    // remember which convention it is in.
    let to_px = |p: Point| Point::new(p.x * scale, -p.y * scale);

    let Some(bounds) = outline_bounds(outline, scale)? else {
        return Ok(GlyphMask::default());
    };

    // The bitmap covers whole pixels, so grow the box outward to integers.
    let left_f = bounds.min_x.floor();
    let top_f = bounds.min_y.floor();
    let right_f = bounds.max_x.ceil();
    let bottom_f = bounds.max_y.ceil();
    #[allow(clippy::cast_possible_truncation)]
    // Bounded above by MAX_GLYPH_PIXELS' implied dimension check below; the
    // finiteness test already ran.
    let (left, top, right, bottom) = (left_f as i64, top_f as i64, right_f as i64, bottom_f as i64);
    let width = usize::try_from(right.saturating_sub(left)).map_err(|_| RasterError::TooLarge)?;
    let height = usize::try_from(bottom.saturating_sub(top)).map_err(|_| RasterError::TooLarge)?;
    if width == 0 || height == 0 {
        // A degenerate outline (a zero-area contour) is not an error; it
        // simply draws nothing.
        return Ok(GlyphMask::default());
    }
    if width
        .checked_mul(height)
        .is_none_or(|n| n > MAX_GLYPH_PIXELS)
    {
        return Err(RasterError::TooLarge);
    }

    let mut acc = Accumulator::new(width, height).ok_or(RasterError::TooLarge)?;

    // Translate so the bitmap's top-left corner is the origin.
    let place = |p: Point| Point::new(p.x - left_f, p.y - top_f);

    let mut start = Point::default();
    let mut cur = Point::default();
    for cmd in &outline.commands {
        match *cmd {
            PathCmd::MoveTo(p) => {
                // An unterminated previous contour is implicitly closed; a
                // left-open contour would leak winding across the whole row.
                if cur != start {
                    acc.line(cur, start);
                }
                let p = place(to_px(p));
                start = p;
                cur = p;
            }
            PathCmd::LineTo(p) => {
                let p = place(to_px(p));
                acc.line(cur, p);
                cur = p;
            }
            PathCmd::QuadTo(ctrl, p) => {
                let ctrl = place(to_px(ctrl));
                let p = place(to_px(p));
                flatten_quad(&mut acc, cur, ctrl, p);
                cur = p;
            }
            PathCmd::CurveTo(c1, c2, p) => {
                let c1 = place(to_px(c1));
                let c2 = place(to_px(c2));
                let p = place(to_px(p));
                flatten_cubic(&mut acc, cur, c1, c2, p);
                cur = p;
            }
            PathCmd::Close => {
                if cur != start {
                    acc.line(cur, start);
                }
                cur = start;
            }
        }
    }
    // A path that ends without a Close still bounds a region.
    if cur != start {
        acc.line(cur, start);
    }

    let coverage = acc.into_coverage();
    Ok(GlyphMask {
        width: u32::try_from(width).map_err(|_| RasterError::TooLarge)?,
        height: u32::try_from(height).map_err(|_| RasterError::TooLarge)?,
        left: i32::try_from(left).map_err(|_| RasterError::TooLarge)?,
        top: i32::try_from(top).map_err(|_| RasterError::TooLarge)?,
        coverage,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;

    /// Total ink in a mask, in units of whole pixels.
    fn ink(mask: &GlyphMask) -> f32 {
        mask.coverage.iter().map(|c| f32::from(*c) / 255.0).sum()
    }

    /// The bounding box of the pixels that actually got ink, in the mask's
    /// own placed coordinates (`left`/`top` applied), so that two masks with
    /// different buffer sizes can still be compared.
    fn ink_box(mask: &GlyphMask) -> (i32, i32, i32, i32) {
        let mut b = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..mask.height {
            for x in 0..mask.width {
                if mask.at(x, y) == 0 {
                    continue;
                }
                let (px, py) = (mask.left + x as i32, mask.top + y as i32);
                b = (b.0.min(px), b.1.min(py), b.2.max(px), b.3.max(py));
            }
        }
        b
    }

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<PathCmd> {
        vec![
            PathCmd::MoveTo(Point::new(x0, y0)),
            PathCmd::LineTo(Point::new(x1, y0)),
            PathCmd::LineTo(Point::new(x1, y1)),
            PathCmd::LineTo(Point::new(x0, y1)),
            PathCmd::Close,
        ]
    }

    /// The same rectangle wound the other way, for hole tests.
    fn rect_reversed(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<PathCmd> {
        vec![
            PathCmd::MoveTo(Point::new(x0, y0)),
            PathCmd::LineTo(Point::new(x0, y1)),
            PathCmd::LineTo(Point::new(x1, y1)),
            PathCmd::LineTo(Point::new(x1, y0)),
            PathCmd::Close,
        ]
    }

    /// The cubic that traces exactly the same curve as a given quadratic.
    ///
    /// Every quadratic is a cubic whose control points sit two-thirds of the
    /// way from each endpoint towards the quadratic's single control point.
    /// This is an identity, not an approximation, which is what makes it a
    /// usable oracle: the cubic code path has to agree with the quadratic one
    /// to within flattening error and nothing else.
    fn elevate(p0: Point, c: Point, p1: Point) -> PathCmd {
        const TWO_THIRDS: f32 = 2.0 / 3.0;
        PathCmd::CurveTo(
            Point::new(
                TWO_THIRDS.mul_add(c.x - p0.x, p0.x),
                TWO_THIRDS.mul_add(c.y - p0.y, p0.y),
            ),
            Point::new(
                TWO_THIRDS.mul_add(c.x - p1.x, p1.x),
                TWO_THIRDS.mul_add(c.y - p1.y, p1.y),
            ),
            p1,
        )
    }

    #[test]
    fn a_cubic_draws_the_same_shape_as_the_quadratic_it_elevates() {
        let p0 = Point::new(0.0, 0.0);
        let c = Point::new(50.0, 200.0);
        let p1 = Point::new(100.0, 0.0);

        let quad = Outline {
            commands: vec![
                PathCmd::MoveTo(p0),
                PathCmd::QuadTo(c, p1),
                PathCmd::LineTo(p0),
                PathCmd::Close,
            ],
        };
        let cubic = Outline {
            commands: vec![
                PathCmd::MoveTo(p0),
                elevate(p0, c, p1),
                PathCmd::LineTo(p0),
                PathCmd::Close,
            ],
        };

        let a = rasterize(&quad, 1.0).unwrap();
        let b = rasterize(&cubic, 1.0).unwrap();
        // The two masks are *not* the same size, and should not be: the
        // bounding box is taken over control points, and elevation moves the
        // control point from the quadratic's y=200 down to two cubic controls
        // at y=133. Both bound the same curve, whose peak is at y=100. So the
        // comparison has to be of the ink, in absolute coordinates.
        assert_eq!(
            ink_box(&a),
            ink_box(&b),
            "the two curves cover different pixels"
        );
        // Both are flattened, with different segment counts, so the two masks
        // differ by a sliver along the curve rather than not at all. A
        // half-percent of the ink is far tighter than any wrong evaluation of
        // the cubic basis could land.
        let (ia, ib) = (ink(&a), ink(&b));
        assert!(ia > 100.0, "the oracle drew nothing: {ia}");
        assert!(
            (ia - ib).abs() < ia * 0.005,
            "cubic ink {ib} differs from the equivalent quadratic's {ia}"
        );
    }

    #[test]
    fn a_degenerate_cubic_is_a_straight_edge() {
        // Controls on the chord: the curve *is* the chord, so the result must
        // be the triangle, not something bulging off it.
        let tri = |mid: PathCmd| Outline {
            commands: vec![
                PathCmd::MoveTo(Point::new(0.0, 0.0)),
                PathCmd::LineTo(Point::new(40.0, 0.0)),
                mid,
                PathCmd::Close,
            ],
        };
        let straight = rasterize(&tri(PathCmd::LineTo(Point::new(0.0, 40.0))), 1.0).unwrap();
        let curved = rasterize(
            &tri(PathCmd::CurveTo(
                Point::new(26.666_666, 13.333_333),
                Point::new(13.333_333, 26.666_666),
                Point::new(0.0, 40.0),
            )),
            1.0,
        )
        .unwrap();
        let (a, b) = (ink(&straight), ink(&curved));
        assert!((a - 800.0).abs() < 2.0, "the oracle is not the triangle: {a}");
        assert!((a - b).abs() < 1.0, "a flat cubic drew {b}, not the triangle's {a}");
    }

    #[test]
    fn a_tighter_cubic_gets_more_segments() {
        let p0 = Point::new(0.0, 0.0);
        let p1 = Point::new(100.0, 0.0);
        let gentle = cubic_segments(p0, Point::new(33.0, 1.0), Point::new(66.0, 1.0), p1);
        let tight = cubic_segments(p0, Point::new(33.0, 300.0), Point::new(66.0, 300.0), p1);
        assert!(gentle < tight, "gentle {gentle} was not cheaper than tight {tight}");
        assert!(tight <= 256, "segment count is unbounded: {tight}");
        // Same deviation, more curvature to resolve: a cubic must not be
        // flattened as coarsely as a quadratic.
        let quad = quad_segments(p0, Point::new(50.0, 300.0), p1);
        assert!(
            tight > quad,
            "cubic {tight} used no more segments than quadratic {quad}"
        );
    }

    #[test]
    fn empty_outline_rasterizes_to_nothing() {
        let mask = rasterize(&Outline::default(), 1.0).unwrap();
        assert!(mask.is_empty());
        assert_eq!(mask.width, 0);
    }

    #[test]
    fn invalid_scale_is_rejected() {
        let o = Outline {
            commands: rect(0.0, 0.0, 10.0, 10.0),
        };
        assert_eq!(rasterize(&o, 0.0), Err(RasterError::InvalidScale));
        assert_eq!(rasterize(&o, -1.0), Err(RasterError::InvalidScale));
        assert_eq!(rasterize(&o, f32::NAN), Err(RasterError::InvalidScale));
    }

    #[test]
    fn non_finite_coordinates_are_rejected() {
        let o = Outline {
            commands: vec![
                PathCmd::MoveTo(Point::new(0.0, 0.0)),
                PathCmd::LineTo(Point::new(f32::NAN, 1.0)),
                PathCmd::Close,
            ],
        };
        assert_eq!(rasterize(&o, 1.0), Err(RasterError::NonFiniteCoordinate));
    }

    #[test]
    fn absurd_size_is_rejected_not_allocated() {
        let o = Outline {
            commands: rect(0.0, 0.0, 1e6, 1e6),
        };
        assert_eq!(rasterize(&o, 100.0), Err(RasterError::TooLarge));
    }

    #[test]
    fn pixel_aligned_square_is_solid() {
        // 4x4 square on the pixel grid; y is flipped so the box spans
        // y = -4..0 in font units to land at rows 0..4.
        let o = Outline {
            commands: rect(0.0, 0.0, 4.0, -4.0),
        };
        let mask = rasterize(&o, 1.0).unwrap();
        assert_eq!((mask.width, mask.height), (4, 4));
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(mask.at(x, y), 255, "pixel ({x},{y}) should be solid");
            }
        }
        assert_eq!(mask.left, 0);
        assert_eq!(mask.top, 0);
    }

    #[test]
    fn winding_direction_does_not_change_the_fill() {
        let cw = Outline {
            commands: rect(0.0, 0.0, 4.0, -4.0),
        };
        let ccw = Outline {
            commands: rect_reversed(0.0, 0.0, 4.0, -4.0),
        };
        assert_eq!(rasterize(&cw, 1.0).unwrap(), rasterize(&ccw, 1.0).unwrap());
    }

    #[test]
    fn half_covered_pixels_get_half_coverage() {
        // A box half a pixel wide: every pixel in the column is ~50% covered.
        let o = Outline {
            commands: rect(0.0, 0.0, 0.5, -4.0),
        };
        let mask = rasterize(&o, 1.0).unwrap();
        assert_eq!((mask.width, mask.height), (1, 4));
        for y in 0..4 {
            let c = mask.at(0, y);
            assert!(
                (120..=136).contains(&c),
                "expected ~50% coverage, got {c} at row {y}"
            );
        }
    }

    #[test]
    fn total_ink_matches_geometric_area() {
        // A triangle with area 1/2 * 8 * 8 = 32 px^2, deliberately not
        // pixel-aligned so the anti-aliasing actually does something.
        let o = Outline {
            commands: vec![
                PathCmd::MoveTo(Point::new(0.3, -0.2)),
                PathCmd::LineTo(Point::new(8.3, -0.2)),
                PathCmd::LineTo(Point::new(0.3, -8.2)),
                PathCmd::Close,
            ],
        };
        let mask = rasterize(&o, 1.0).unwrap();
        let area = ink(&mask);
        assert!(
            (area - 32.0).abs() < 0.5,
            "expected ~32 px of ink, measured {area}"
        );
    }

    #[test]
    fn counter_wound_inner_contour_becomes_a_hole() {
        // An 8x8 square with a counter-wound 4x4 square inside it: the
        // middle must be empty and the ink must equal 64 - 16 = 48.
        let mut commands = rect(0.0, 0.0, 8.0, -8.0);
        commands.extend(rect_reversed(2.0, -2.0, 6.0, -6.0));
        let mask = rasterize(&Outline { commands }, 1.0).unwrap();
        assert_eq!((mask.width, mask.height), (8, 8));
        for y in 2..6 {
            for x in 2..6 {
                assert_eq!(mask.at(x, y), 0, "({x},{y}) should be inside the hole");
            }
        }
        assert_eq!(mask.at(0, 0), 255);
        let area = ink(&mask);
        assert!(
            (area - 48.0).abs() < 0.5,
            "expected 48 px of ink, got {area}"
        );
    }

    #[test]
    fn same_wound_inner_contour_does_not_make_a_hole() {
        // Non-zero winding, not even-odd: two same-direction contours
        // overlap to winding 2, which still fills.
        let mut commands = rect(0.0, 0.0, 8.0, -8.0);
        commands.extend(rect(2.0, -2.0, 6.0, -6.0));
        let mask = rasterize(&Outline { commands }, 1.0).unwrap();
        assert_eq!(mask.at(4, 4), 255, "winding 2 must still be solid");
        let area = ink(&mask);
        assert!(
            (area - 64.0).abs() < 0.5,
            "expected 64 px of ink, got {area}"
        );
    }

    #[test]
    fn quadratic_curve_produces_a_smooth_edge() {
        // A quarter-disc-ish wedge. The point is that the diagonal edge is
        // anti-aliased rather than binary: some pixel must be partially lit.
        let o = Outline {
            commands: vec![
                PathCmd::MoveTo(Point::new(0.0, 0.0)),
                PathCmd::QuadTo(Point::new(16.0, 0.0), Point::new(16.0, -16.0)),
                PathCmd::LineTo(Point::new(0.0, -16.0)),
                PathCmd::Close,
            ],
        };
        let mask = rasterize(&o, 1.0).unwrap();
        let partial = mask
            .coverage
            .iter()
            .filter(|c| **c > 0 && **c < 255)
            .count();
        assert!(
            partial > 8,
            "a curved edge should leave many partial pixels, found {partial}"
        );
    }

    #[test]
    fn scaling_up_scales_the_ink_quadratically() {
        let o = Outline {
            commands: rect(0.0, 0.0, 10.0, -10.0),
        };
        let small = ink(&rasterize(&o, 1.0).unwrap());
        let large = ink(&rasterize(&o, 3.0).unwrap());
        assert!(
            (large / small - 9.0).abs() < 0.1,
            "3x scale should give 9x the ink; got {}",
            large / small
        );
    }

    #[test]
    fn offsets_report_where_the_ink_belongs() {
        // A glyph sitting 5 px right of the pen and straddling the baseline.
        let o = Outline {
            commands: rect(5.0, 2.0, 9.0, -3.0),
        };
        let mask = rasterize(&o, 1.0).unwrap();
        assert_eq!(mask.left, 5);
        // y flips: font-unit +2 (above baseline) is bitmap -2.
        assert_eq!(mask.top, -2);
        assert_eq!(mask.height, 5);
    }

    #[test]
    fn unclosed_contour_is_still_filled() {
        // Fonts do close their contours, but a caller-built path might not,
        // and a half-open path used to streak an entire scanline.
        let o = Outline {
            commands: vec![
                PathCmd::MoveTo(Point::new(0.0, 0.0)),
                PathCmd::LineTo(Point::new(4.0, 0.0)),
                PathCmd::LineTo(Point::new(4.0, -4.0)),
                PathCmd::LineTo(Point::new(0.0, -4.0)),
            ],
        };
        let mask = rasterize(&o, 1.0).unwrap();
        let area = ink(&mask);
        assert!(
            (area - 16.0).abs() < 0.5,
            "expected 16 px of ink, got {area}"
        );
    }
}

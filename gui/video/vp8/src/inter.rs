//! Inter prediction: a macroblock predicted from a reference frame, moved by
//! a motion vector.
//!
//! Vectors are in eighths of a pixel. Luma moves by the macroblock's vector,
//! or by one vector per part of a split macroblock; chroma by the luma
//! vector halved (or, for a split macroblock, by the average of four).
//! Between whole pixels, a six-tap filter interpolates, or -- in VP8's
//! versions 1 to 3 -- a bilinear one; version 3 rounds chroma vectors to
//! whole pixels.
//!
//! A vector that points far outside the picture is clamped to one that reads
//! the same pixels from the border, which holds copies of the picture's edge
//! (libvpx's `clamp_mv_to_umv_border`), so no prediction reaches more than
//! 21 pixels outside the picture. One case libvpx does not predict at all: a
//! macroblock in a version-3 stream whose chroma vector, rounded to whole
//! pixels, points more than 19 pixels out keeps whatever its chroma blocks
//! held before. That is reproduced too, from the frame that held the buffer
//! before.
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/common/reconinter.c` and
//! `vp8/common/filter.c` (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "a source window is checked to lie inside its plane (and moved inside if it does not) before it is read; destinations are blocks of a macroblock inside the plane; filter and block tables are indexed by values below their lengths"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "filter sums are six products of a pixel and a tap below 128; positions are inside planes of at most 16448 pixels a side; vector arithmetic wraps at 16 bits through explicit wrapping operations"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "filtered values are clamped to 0..=255 before they become pixels; plane positions convert between isize and usize only once checked to be inside the plane"
)]

use crate::frame::{Frame, Plane};
use crate::modes::{Edges, ModeInfo, Mv, SPLITMV};
use crate::tables::{BILINEAR_FILTERS, SUB_PEL_FILTERS};

/// Which interpolation a frame uses between whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Filter {
    SixTap,
    Bilinear,
}

/// What prediction needs to know about the frame and the macroblock.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InterContext {
    pub(crate) filter: Filter,
    /// `!7` in version 3, which rounds chroma vectors to whole pixels; else
    /// `!0`: libvpx's `fullpixel_mask`.
    pub(crate) fullpixel_mask: i16,
    pub(crate) edges: Edges,
}

/// The index of the top-left pixel of a `w` x `h` window at (`x`, `y`)
/// (relative to the picture's top-left) with `before` pixels read before it
/// and `after` after it in each direction, moved as little as needed to lie
/// inside `plane`. libvpx's clamping keeps every window it reads inside its
/// buffer, so the move happens only if that reasoning has a hole; it keeps a
/// hostile stream from reading outside the plane even then.
fn window(
    plane: &Plane,
    x: isize,
    y: isize,
    w: usize,
    h: usize,
    before: usize,
    after: usize,
) -> usize {
    // The first pixel read may be `border` pixels before the picture, the
    // last `border - 1` after it.
    let lo = before as isize - plane.border as isize;
    let hi_x = (plane.width + plane.border) as isize - (w + after) as isize;
    let hi_y = (plane.height + plane.border) as isize - (h + after) as isize;
    let x = x.clamp(lo, hi_x.max(lo));
    let y = y.clamp(lo, hi_y.max(lo));
    (plane.origin() as isize + y * plane.stride as isize + x) as usize
}

/// The six-tap filter's taps for an eighth-pixel position.
fn sixtap(frac: usize) -> [i32; 6] {
    SUB_PEL_FILTERS[frac & 7].map(i32::from)
}

/// Apply `taps` to the six pixels from `src[at - 2 * step]`, rounded,
/// shifted and clamped: one output of libvpx's
/// `filter_block2d_first_pass` and `_second_pass`.
#[inline]
fn sixtap_at(src: &[u8], at: usize, step: usize, taps: &[i32; 6]) -> u8 {
    let s = |k: usize| i32::from(src[at + k * step - 2 * step]);
    let sum = s(0) * taps[0]
        + s(1) * taps[1]
        + s(2) * taps[2]
        + s(3) * taps[3]
        + s(4) * taps[4]
        + s(5) * taps[5]
        + 64;
    (sum >> 7).clamp(0, 255) as u8
}

/// Predict a `w` x `h` block (16x16, 8x8, 8x4 or 4x4) from `src` at `at`
/// into `dst` at `dst_at`, `xfrac` and `yfrac` eighths of a pixel right and
/// down of it: libvpx's `vp8_sixtap_predict*_c` and
/// `vp8_bilinear_predict*_c`, or the copy libvpx makes when both fractions
/// are 0.
#[allow(
    clippy::too_many_arguments,
    reason = "libvpx's own signature: two planes, two positions and a block"
)]
fn predict(
    filter: Filter,
    src: &[u8],
    src_at: usize,
    src_stride: usize,
    xfrac: usize,
    yfrac: usize,
    w: usize,
    h: usize,
    dst: &mut [u8],
    dst_at: usize,
    dst_stride: usize,
) {
    if xfrac == 0 && yfrac == 0 {
        for r in 0..h {
            let s = src_at + r * src_stride;
            let d = dst_at + r * dst_stride;
            dst[d..d + w].copy_from_slice(&src[s..s + w]);
        }
        return;
    }
    match filter {
        Filter::SixTap => {
            // Horizontally into h + 5 rows (two above, three below), then
            // vertically. A fraction of 0 is the filter {0, 0, 128, 0, 0, 0},
            // which copies exactly, so libvpx's two passes are kept as two.
            let htaps = sixtap(xfrac);
            let vtaps = sixtap(yfrac);
            let mut tmp = [0u8; 21 * 16];
            for r in 0..h + 5 {
                let s = src_at + r * src_stride - 2 * src_stride;
                for c in 0..w {
                    tmp[r * w + c] = sixtap_at(src, s + c, 1, &htaps);
                }
            }
            for r in 0..h {
                let d = dst_at + r * dst_stride;
                for c in 0..w {
                    dst[d + c] = sixtap_at(&tmp, (r + 2) * w + c, w, &vtaps);
                }
            }
        }
        Filter::Bilinear => {
            // Horizontally into h + 1 rows, then vertically: libvpx's
            // `filter_block2d_bil`. The taps sum to 128, so neither pass
            // leaves 0..=255.
            let [h0, h1] = BILINEAR_FILTERS[xfrac & 7].map(i32::from);
            let [v0, v1] = BILINEAR_FILTERS[yfrac & 7].map(i32::from);
            let mut tmp = [0u16; 17 * 16];
            for r in 0..=h {
                let s = src_at + r * src_stride;
                for c in 0..w {
                    let a = i32::from(src[s + c]);
                    let b = i32::from(src[s + c + 1]);
                    tmp[r * w + c] = ((a * h0 + b * h1 + 64) >> 7) as u16;
                }
            }
            for r in 0..h {
                let d = dst_at + r * dst_stride;
                for c in 0..w {
                    let a = i32::from(tmp[r * w + c]);
                    let b = i32::from(tmp[(r + 1) * w + c]);
                    dst[d + c] = ((a * v0 + b * v1 + 64) >> 7) as u8;
                }
            }
        }
    }
}

/// Predict the `w` x `h` block at (`x`, `y`) of plane `p` of `dst` from the
/// same plane of `refp`, moved by `mv`: the body shared by libvpx's
/// `build_inter_predictors_b`, `2b`, `4b` and the 16x16 predictor.
#[allow(
    clippy::too_many_arguments,
    reason = "a block, its plane, its vector and the frames it moves between"
)]
fn predict_block(
    ctx: &InterContext,
    refp: &Frame,
    dst: &mut Frame,
    p: usize,
    x: usize,
    y: usize,
    mv: Mv,
    w: usize,
    h: usize,
) {
    let src_plane = &refp.planes[p];
    let (xfrac, yfrac) = ((mv.col & 7) as usize, (mv.row & 7) as usize);
    let sx = x as isize + (isize::from(mv.col) >> 3);
    let sy = y as isize + (isize::from(mv.row) >> 3);
    let (before, after) = match (ctx.filter, xfrac | yfrac) {
        (_, 0) => (0, 0),
        (Filter::SixTap, _) => (2, 3),
        (Filter::Bilinear, _) => (0, 1),
    };
    let src_at = window(src_plane, sx, sy, w, h, before, after);
    let dst_plane = &mut dst.planes[p];
    let dst_at = dst_plane.at(x, y);
    let stride = dst_plane.stride;
    predict(
        ctx.filter,
        &src_plane.data,
        src_at,
        src_plane.stride,
        xfrac,
        yfrac,
        w,
        h,
        &mut dst_plane.data,
        dst_at,
        stride,
    );
}

/// Pull a vector that points so far outside the picture that only border
/// pixels are read back to 16 pixels outside, where it reads the same:
/// libvpx's `clamp_mv_to_umv_border`. A bound is assigned only when a 16-bit
/// vector lies beyond it, so it fits 16 bits.
fn clamp_mv_to_umv_border(mv: &mut Mv, e: Edges) {
    let col = i32::from(mv.col);
    if col < e.left - (19 << 3) {
        mv.col = (e.left - (16 << 3)) as i16;
    } else if col > e.right + (18 << 3) {
        mv.col = (e.right + (16 << 3)) as i16;
    }
    let row = i32::from(mv.row);
    if row < e.top - (19 << 3) {
        mv.row = (e.top - (16 << 3)) as i16;
    } else if row > e.bottom + (18 << 3) {
        mv.row = (e.bottom + (16 << 3)) as i16;
    }
}

/// [`clamp_mv_to_umv_border`] for a chroma vector, which is half the
/// distance: libvpx's `clamp_uvmv_to_umv_border`, its four tests in turn.
fn clamp_uvmv_to_umv_border(mv: &mut Mv, e: Edges) {
    if 2 * i32::from(mv.col) < e.left - (19 << 3) {
        mv.col = ((e.left - (16 << 3)) >> 1) as i16;
    }
    if 2 * i32::from(mv.col) > e.right + (18 << 3) {
        mv.col = ((e.right + (16 << 3)) >> 1) as i16;
    }
    if 2 * i32::from(mv.row) < e.top - (19 << 3) {
        mv.row = ((e.top - (16 << 3)) >> 1) as i16;
    }
    if 2 * i32::from(mv.row) > e.bottom + (18 << 3) {
        mv.row = ((e.bottom + (16 << 3)) >> 1) as i16;
    }
}

/// A whole macroblock's chroma vector from its luma one: away from zero by
/// one eighth, halved toward zero, and rounded to whole pixels in version
/// 3. In 16 bits, wrapping, as libvpx's `vp8_build_inter16x16_predictors_mb`
/// computes it.
fn chroma_mv(mv: Mv, fullpixel_mask: i16) -> Mv {
    let half = |v: i16| {
        let away = v.wrapping_add(if v < 0 { -1 } else { 1 });
        (away / 2) & fullpixel_mask
    };
    Mv {
        row: half(mv.row),
        col: half(mv.col),
    }
}

/// A split macroblock's chroma vectors, one per 4x4 chroma block: the
/// average of the four luma vectors over it, rounded away from zero, as
/// libvpx's `build_4x4uvmvs` computes them. Clamped as luma's are when the
/// macroblock's vectors may point far outside.
fn split_chroma_mvs(mi: &ModeInfo, ctx: &InterContext) -> [Mv; 4] {
    let mut out = [Mv::ZERO; 4];
    for i in 0..2 {
        for j in 0..2 {
            let y = i * 8 + j * 2;
            let blocks = [mi.bmvs[y], mi.bmvs[y + 1], mi.bmvs[y + 4], mi.bmvs[y + 5]];
            let average = |part: fn(&Mv) -> i16| {
                let sum: i32 = blocks.iter().map(|b| i32::from(part(b))).sum();
                let rounded = sum + if sum < 0 { 4 - 8 } else { 4 };
                ((rounded / 8) as i16) & ctx.fullpixel_mask
            };
            let mut mv = Mv {
                row: average(|m| m.row),
                col: average(|m| m.col),
            };
            if mi.need_to_clamp_mvs {
                clamp_uvmv_to_umv_border(&mut mv, ctx.edges);
            }
            out[i * 2 + j] = mv;
        }
    }
    out
}

/// Predict the macroblock at (`mb_x`, `mb_y`) pixels from `refp`: libvpx's
/// `vp8_build_inter_predictors_mb`. `stale` is what `dst` held before this
/// frame, if it is not `dst`'s own content: the one case libvpx predicts no
/// chroma reads it.
pub(crate) fn predict_mb(
    ctx: &InterContext,
    mi: &ModeInfo,
    refp: &Frame,
    dst: &mut Frame,
    stale: Option<&Frame>,
    mb_x: usize,
    mb_y: usize,
) {
    if mi.mode == SPLITMV {
        predict_split(ctx, mi, refp, dst, mb_x, mb_y);
        return;
    }
    // libvpx's `vp8_build_inter16x16_predictors_mb`.
    let mut mv = mi.mv;
    if mi.need_to_clamp_mvs {
        clamp_mv_to_umv_border(&mut mv, ctx.edges);
    }
    predict_block(ctx, refp, dst, 0, mb_x, mb_y, mv, 16, 16);
    let uv = chroma_mv(mv, ctx.fullpixel_mask);
    let e = ctx.edges;
    let (col2, row2) = (2 * i32::from(uv.col), 2 * i32::from(uv.row));
    let (cx, cy) = (mb_x / 2, mb_y / 2);
    if col2 < e.left - (19 << 3)
        || col2 > e.right + (18 << 3)
        || row2 < e.top - (19 << 3)
        || row2 > e.bottom + (18 << 3)
    {
        // libvpx returns here and leaves the chroma blocks as they were.
        if let Some(stale) = stale {
            for p in 1..3 {
                let (src, out) = (&stale.planes[p], &mut dst.planes[p]);
                for r in 0..8 {
                    let at = out.at(cx, cy + r);
                    out.data[at..at + 8].copy_from_slice(&src.data[at..at + 8]);
                }
            }
        }
        return;
    }
    for p in 1..3 {
        predict_block(ctx, refp, dst, p, cx, cy, uv, 8, 8);
    }
}

/// A split macroblock: libvpx's `build_4x4uvmvs` and
/// `build_inter4x4_predictors_mb`.
fn predict_split(
    ctx: &InterContext,
    mi: &ModeInfo,
    refp: &Frame,
    dst: &mut Frame,
    mb_x: usize,
    mb_y: usize,
) {
    let clamped = |b: usize| {
        let mut mv = mi.bmvs[b];
        if mi.need_to_clamp_mvs {
            clamp_mv_to_umv_border(&mut mv, ctx.edges);
        }
        mv
    };
    if mi.partitioning < 3 {
        // Halves or quarters: four 8x8 blocks, each moved by the vector of
        // its top-left 4x4 block.
        for b in [0, 2, 8, 10] {
            let (x, y) = (mb_x + (b & 3) * 4, mb_y + (b >> 2) * 4);
            predict_block(ctx, refp, dst, 0, x, y, clamped(b), 8, 8);
        }
    } else {
        // Sixteen 4x4 blocks, in pairs that share an 8x4 prediction when
        // their vectors agree.
        for b in (0..16).step_by(2) {
            let (x, y) = (mb_x + (b & 3) * 4, mb_y + (b >> 2) * 4);
            let (m0, m1) = (clamped(b), clamped(b + 1));
            if m0 == m1 {
                predict_block(ctx, refp, dst, 0, x, y, m0, 8, 4);
            } else {
                predict_block(ctx, refp, dst, 0, x, y, m0, 4, 4);
                predict_block(ctx, refp, dst, 0, x + 4, y, m1, 4, 4);
            }
        }
    }
    let uv = split_chroma_mvs(mi, ctx);
    let (cx, cy) = (mb_x / 2, mb_y / 2);
    for p in 1..3 {
        for pair in 0..2 {
            let (m0, m1) = (uv[pair * 2], uv[pair * 2 + 1]);
            let y = cy + pair * 4;
            if m0 == m1 {
                predict_block(ctx, refp, dst, p, cx, y, m0, 8, 4);
            } else {
                predict_block(ctx, refp, dst, p, cx, y, m0, 4, 4);
                predict_block(ctx, refp, dst, p, cx + 4, y, m1, 4, 4);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges() -> Edges {
        Edges::of(1, 1, 4, 4)
    }

    #[test]
    fn a_chroma_vector_halves_away_from_zero() {
        let m = |row, col| Mv { row, col };
        assert_eq!(chroma_mv(m(4, -4), !0), m(2, -2));
        assert_eq!(chroma_mv(m(3, -3), !0), m(2, -2));
        assert_eq!(chroma_mv(m(1, -1), !0), m(1, -1));
        assert_eq!(chroma_mv(m(0, 0), !0), m(0, 0));
        // Version 3 drops the fractions, rounding down.
        assert_eq!(chroma_mv(m(22, -22), !7), m(8, -16));
        // At the 16-bit limit the step away from zero wraps, as in libvpx.
        assert_eq!(chroma_mv(m(i16::MAX, 0), !0).row, i16::MIN / 2);
    }

    #[test]
    fn split_chroma_vectors_average_four_and_round_away_from_zero() {
        let mut mi = ModeInfo::default();
        for (b, v) in [(0, 3), (1, 4), (4, 5), (5, 6)] {
            mi.bmvs[b] = Mv { row: v, col: -v };
        }
        let ctx = InterContext {
            filter: Filter::SixTap,
            fullpixel_mask: !0,
            edges: edges(),
        };
        let uv = split_chroma_mvs(&mi, &ctx);
        // (18 + 4) / 8 = 2; (-18 - 4) / 8 = -2.
        assert_eq!(uv[0], Mv { row: 2, col: -2 });
        assert_eq!(uv[3], Mv::ZERO);
    }

    #[test]
    fn far_vectors_are_clamped_to_sixteen_pixels_out() {
        let e = edges();
        let mut mv = Mv {
            row: -2000,
            col: 2000,
        };
        clamp_mv_to_umv_border(&mut mv, e);
        assert_eq!(
            mv,
            Mv {
                row: (e.top - 128) as i16,
                col: (e.right + 128) as i16
            }
        );
        // Within 19 (above, left) or 18 (below, right) pixels: unchanged.
        let mut near = Mv {
            row: (e.top - 152) as i16,
            col: (e.right + 144) as i16,
        };
        let before = near;
        clamp_mv_to_umv_border(&mut near, e);
        assert_eq!(near, before);
    }

    #[test]
    fn the_sixtap_filter_at_a_whole_pixel_copies() {
        let src: Vec<u8> = (0..=255).collect();
        let taps = sixtap(0);
        for at in 2..250 {
            assert_eq!(sixtap_at(&src, at, 1, &taps), src[at]);
        }
    }

    #[test]
    fn a_half_pixel_is_between_its_neighbours() {
        // A flat run interpolates to itself; a step interpolates between.
        let taps = sixtap(4);
        let flat = [100u8; 8];
        assert_eq!(sixtap_at(&flat, 3, 1, &taps), 100);
        let step = [0u8, 0, 0, 0, 200, 200, 200, 200];
        assert_eq!(sixtap_at(&step, 3, 1, &taps), 100);
    }

    #[test]
    fn a_window_outside_the_plane_moves_inside() {
        let f = Frame::new(16, 16);
        let p = &f.planes[0];
        let inside = window(p, -20, -20, 16, 16, 2, 3);
        assert_eq!(inside, p.origin() - 20 * p.stride - 20);
        let moved = window(p, -1000, 5000, 16, 16, 2, 3);
        // Moved to the top-left-most and bottom-most place that fits.
        let x = -(p.border as isize) + 2;
        let y = (p.height + p.border) as isize - 19;
        assert_eq!(
            moved as isize,
            p.origin() as isize + y * p.stride as isize + x
        );
    }
}

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

use crate::frame::{Frame, Plane, Target};
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

/// A six-tap kernel for an eighth-pixel position, split as the passes below
/// use it: its four taps that are never negative (0, 2, 3 and 5) and the
/// magnitudes of the two that are never positive (1 and 4). Sums of each
/// part fit sixteen bits -- at most 160 or 32 times a pixel -- so a pass
/// runs in 16-bit lanes, eight to an SSE2 register, where the whole signed
/// sum would not fit.
#[derive(Clone, Copy, Debug)]
struct SixTap {
    pos: [u16; 4],
    neg: [u16; 2],
}

impl SixTap {
    fn of(frac: usize) -> Self {
        let t = SUB_PEL_FILTERS[frac & 7];
        Self {
            pos: [t[0], t[2], t[3], t[5]].map(i16::unsigned_abs),
            neg: [t[1], t[4]].map(i16::unsigned_abs),
        }
    }

    /// `pos - neg`, shifted and clamped to a pixel: libvpx's `(sum + 64) >>
    /// 7` clamped to 0..=255, `pos` holding the 64.
    #[inline(always)]
    fn finish(pos: u16, neg: u16) -> u8 {
        (pos.saturating_sub(neg) >> 7).min(255) as u8
    }
}

/// One row of the six-tap filter across `row` (the `W + 5` pixels from two
/// before the first output) into `out`: one row of libvpx's
/// `filter_block2d_first_pass`.
#[inline(always)]
fn sixtap_row<const W: usize>(row: &[u8], k: SixTap, out: &mut [u8]) {
    let mut pos = [64u16; W];
    let mut neg = [0u16; W];
    for (offset, t) in [(0, k.pos[0]), (2, k.pos[1]), (3, k.pos[2]), (5, k.pos[3])] {
        let seg = &row[offset..offset + W];
        for c in 0..W {
            pos[c] += u16::from(seg[c]) * t;
        }
    }
    for (offset, t) in [(1, k.neg[0]), (4, k.neg[1])] {
        let seg = &row[offset..offset + W];
        for c in 0..W {
            neg[c] += u16::from(seg[c]) * t;
        }
    }
    let out = &mut out[..W];
    for c in 0..W {
        out[c] = SixTap::finish(pos[c], neg[c]);
    }
}

/// One row of the six-tap filter down six rows (`rows[2]` the one the
/// output is level with) into `out`: one row of libvpx's
/// `filter_block2d_second_pass`.
#[inline(always)]
fn sixtap_column<const W: usize>(rows: [&[u8]; 6], k: SixTap, out: &mut [u8]) {
    let mut pos = [64u16; W];
    let mut neg = [0u16; W];
    for (row, t) in [
        (rows[0], k.pos[0]),
        (rows[2], k.pos[1]),
        (rows[3], k.pos[2]),
        (rows[5], k.pos[3]),
    ] {
        let row = &row[..W];
        for c in 0..W {
            pos[c] += u16::from(row[c]) * t;
        }
    }
    for (row, t) in [(rows[1], k.neg[0]), (rows[4], k.neg[1])] {
        let row = &row[..W];
        for c in 0..W {
            neg[c] += u16::from(row[c]) * t;
        }
    }
    let out = &mut out[..W];
    for c in 0..W {
        out[c] = SixTap::finish(pos[c], neg[c]);
    }
}

/// One row of the bilinear filter between `a` and `b` -- neighbouring
/// pixels, or neighbouring rows -- whose taps sum to 128: one row of
/// libvpx's `filter_block2d_bil_first_pass` or `_second_pass`.
#[inline(always)]
fn bilinear<const W: usize>(a: &[u8], b: &[u8], taps: [u16; 2], out: &mut [u8]) {
    let (a, b, out) = (&a[..W], &b[..W], &mut out[..W]);
    for c in 0..W {
        out[c] = ((u16::from(a[c]) * taps[0] + u16::from(b[c]) * taps[1] + 64) >> 7) as u8;
    }
}

/// Predict a `W` x `h` block (16x16, 8x8, 8x4 or 4x4) from `src` at `src_at`
/// into `dst` at `dst_at`, `xfrac` and `yfrac` eighths of a pixel right of
/// and below it: libvpx's `vp8_sixtap_predict*_c` and
/// `vp8_bilinear_predict*_c`, or the copy libvpx makes when both fractions
/// are 0.
///
/// libvpx's C filters horizontally and then vertically whatever the
/// fractions. A fraction of 0 is the kernel `{0, 0, 128, 0, 0, 0}` (or
/// `{128, 0}`), which copies exactly, so a pass with it is skipped here; the
/// pictures are the same, and libvpx's own SIMD skips it too.
#[allow(
    clippy::too_many_arguments,
    reason = "libvpx's own signature: two planes, two positions and a block"
)]
fn predict<const W: usize>(
    filter: Filter,
    src: &[u8],
    src_at: usize,
    stride: usize,
    xfrac: usize,
    yfrac: usize,
    h: usize,
    dst: &mut [u8],
    dst_at: usize,
    dst_stride: usize,
) {
    let row_of = |r: usize| src_at + r * stride;
    let out_of = |r: usize| dst_at + r * dst_stride;
    if xfrac == 0 && yfrac == 0 {
        for r in 0..h {
            let (s, d) = (row_of(r), out_of(r));
            dst[d..d + W].copy_from_slice(&src[s..s + W]);
        }
        return;
    }
    match filter {
        Filter::SixTap => {
            let (kx, ky) = (SixTap::of(xfrac), SixTap::of(yfrac));
            if yfrac == 0 {
                for r in 0..h {
                    let (s, d) = (row_of(r), out_of(r));
                    sixtap_row::<W>(&src[s - 2..s + W + 3], kx, &mut dst[d..d + W]);
                }
            } else if xfrac == 0 {
                for r in 0..h {
                    let s = row_of(r);
                    let rows = [0, 1, 2, 3, 4, 5].map(|j| {
                        let at = s + j * stride - 2 * stride;
                        &src[at..at + W]
                    });
                    let d = out_of(r);
                    sixtap_column::<W>(rows, ky, &mut dst[d..d + W]);
                }
            } else {
                // Horizontally into h + 5 rows (two above, three below),
                // then vertically.
                let mut tmp = [0u8; 21 * 16];
                for r in 0..h + 5 {
                    let s = row_of(r) - 2 * stride;
                    sixtap_row::<W>(&src[s - 2..s + W + 3], kx, &mut tmp[r * W..(r + 1) * W]);
                }
                for r in 0..h {
                    let rows = [0, 1, 2, 3, 4, 5].map(|j| &tmp[(r + j) * W..(r + j + 1) * W]);
                    let d = out_of(r);
                    sixtap_column::<W>(rows, ky, &mut dst[d..d + W]);
                }
            }
        }
        Filter::Bilinear => {
            // libvpx's `filter_block2d_bil`: horizontally into h + 1 rows,
            // then vertically. The taps sum to 128, so neither pass leaves
            // 0..=255.
            let tx = BILINEAR_FILTERS[xfrac & 7].map(i16::unsigned_abs);
            let ty = BILINEAR_FILTERS[yfrac & 7].map(i16::unsigned_abs);
            if yfrac == 0 {
                for r in 0..h {
                    let (s, d) = (row_of(r), out_of(r));
                    bilinear::<W>(&src[s..], &src[s + 1..], tx, &mut dst[d..d + W]);
                }
            } else if xfrac == 0 {
                for r in 0..h {
                    let (s, d) = (row_of(r), out_of(r));
                    bilinear::<W>(&src[s..], &src[s + stride..], ty, &mut dst[d..d + W]);
                }
            } else {
                let mut tmp = [0u8; 17 * 16];
                for r in 0..=h {
                    let s = row_of(r);
                    bilinear::<W>(&src[s..], &src[s + 1..], tx, &mut tmp[r * W..(r + 1) * W]);
                }
                for r in 0..h {
                    let d = out_of(r);
                    let (above, below) = (&tmp[r * W..], &tmp[(r + 1) * W..]);
                    bilinear::<W>(above, below, ty, &mut dst[d..d + W]);
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
    dst: &mut Target<'_>,
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
    let dst_at = dst.at(p, x, y);
    let stride = dst.strides[p];
    let run = match w {
        16 => predict::<16>,
        8 => predict::<8>,
        _ => predict::<4>,
    };
    run(
        ctx.filter,
        &src_plane.data,
        src_at,
        src_plane.stride,
        xfrac,
        yfrac,
        h,
        &mut *dst.planes[p],
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
    dst: &mut Target<'_>,
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
                let src = &stale.planes[p];
                for r in 0..8 {
                    let (from, to) = (src.at(cx, cy + r), dst.at(p, cx, cy + r));
                    dst.planes[p][to..to + 8].copy_from_slice(&src.data[from..from + 8]);
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
    dst: &mut Target<'_>,
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

    /// libvpx's C filters as they are written: both passes whatever the
    /// fractions, in 32-bit arithmetic. `vp8_sixtap_predict*_c` and
    /// `vp8_bilinear_predict*_c` for a `w` x `h` block at `at`.
    #[allow(clippy::too_many_arguments, reason = "the C functions' own")]
    fn libvpx_c(
        filter: Filter,
        src: &[u8],
        at: usize,
        stride: usize,
        xfrac: usize,
        yfrac: usize,
        w: usize,
        h: usize,
    ) -> Vec<u8> {
        let mut out = vec![0u8; w * h];
        match filter {
            Filter::SixTap => {
                let (hf, vf) = (SUB_PEL_FILTERS[xfrac], SUB_PEL_FILTERS[yfrac]);
                let tap = |p: u8, t: i16| i32::from(p) * i32::from(t);
                let mut first = vec![0i32; w * (h + 5)];
                for r in 0..h + 5 {
                    for c in 0..w {
                        let s = at + r * stride - 2 * stride + c;
                        let sum: i32 = (0..6).map(|k| tap(src[s + k - 2], hf[k])).sum::<i32>() + 64;
                        first[r * w + c] = (sum >> 7).clamp(0, 255);
                    }
                }
                for r in 0..h {
                    for c in 0..w {
                        let sum: i32 = (0..6)
                            .map(|k| first[(r + k) * w + c] * i32::from(vf[k]))
                            .sum::<i32>()
                            + 64;
                        out[r * w + c] = (sum >> 7).clamp(0, 255) as u8;
                    }
                }
            }
            Filter::Bilinear => {
                let (hf, vf) = (BILINEAR_FILTERS[xfrac], BILINEAR_FILTERS[yfrac]);
                let mut first = vec![0i32; w * (h + 1)];
                for r in 0..=h {
                    for c in 0..w {
                        let s = at + r * stride + c;
                        first[r * w + c] = (i32::from(src[s]) * i32::from(hf[0])
                            + i32::from(src[s + 1]) * i32::from(hf[1])
                            + 64)
                            >> 7;
                    }
                }
                for r in 0..h {
                    for c in 0..w {
                        out[r * w + c] = ((first[r * w + c] * i32::from(vf[0])
                            + first[(r + 1) * w + c] * i32::from(vf[1])
                            + 64)
                            >> 7) as u8;
                    }
                }
            }
        }
        out
    }

    #[test]
    fn every_fraction_and_block_predicts_as_libvpx_s_c() {
        // A plane of noise, with runs of 0 and 255 to reach the clamps.
        let stride = 40;
        let mut s = 0x1234_5678u32;
        let src: Vec<u8> = (0..stride * 40)
            .map(|i| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                match i % 37 {
                    0..=3 => 0,
                    4..=7 => 255,
                    _ => (s >> 24) as u8,
                }
            })
            .collect();
        let at = 5 * stride + 5;
        for filter in [Filter::SixTap, Filter::Bilinear] {
            for (w, h) in [(16, 16), (8, 8), (8, 4), (4, 4)] {
                for xfrac in 0..8 {
                    for yfrac in 0..8 {
                        if xfrac == 0 && yfrac == 0 {
                            continue;
                        }
                        let want = libvpx_c(filter, &src, at, stride, xfrac, yfrac, w, h);
                        let mut dst = vec![0u8; w * h];
                        let run = match w {
                            16 => predict::<16>,
                            8 => predict::<8>,
                            _ => predict::<4>,
                        };
                        run(filter, &src, at, stride, xfrac, yfrac, h, &mut dst, 0, w);
                        assert_eq!(dst, want, "{filter:?} {w}x{h} at ({xfrac}, {yfrac})");
                    }
                }
            }
        }
    }

    #[test]
    fn six_tap_kernels_are_negative_only_at_taps_1_and_4() {
        // `SixTap` splits the kernels on this, and sums each part in 16 bits.
        for k in SUB_PEL_FILTERS {
            assert!(k[0] >= 0 && k[2] >= 0 && k[3] >= 0 && k[5] >= 0, "{k:?}");
            assert!(k[1] <= 0 && k[4] <= 0, "{k:?}");
            assert_eq!(k.iter().map(|&t| i32::from(t)).sum::<i32>(), 128);
            let positive: i32 = [k[0], k[2], k[3], k[5]].iter().map(|&t| i32::from(t)).sum();
            assert!(positive * 255 + 64 <= i32::from(u16::MAX));
        }
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

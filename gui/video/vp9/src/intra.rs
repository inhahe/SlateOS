//! Intra prediction: a block predicted from the pixels already decoded above
//! and to its left.
//!
//! Ten modes: DC (the average), vertical, horizontal, true-motion, and six
//! directional ones at 45 to 207 degrees. Each reads an edge -- the row above
//! (and for two modes the row above and to the right) and the column to the
//! left -- that libvpx builds with exact rules for what is missing: no row
//! above reads as 127, no column to the left as 129 (for 10- and 12-bit
//! streams, one below and one above half the range), and an edge that runs
//! past the frame repeats its last pixel. Those rules decide pixels, so they
//! are ported line by line from `build_intra_predictors`.
//!
//! The frame's edge here is its size rounded up to 8 pixels (libvpx's
//! `y_width`, `uv_width`), not the picture's own size: the pixels between the
//! two are decoded like any other and prediction reads them.
//!
//! libvpx's 8-bit and high-bit-depth predictors are the same algorithms over
//! different sample types, so one generic copy serves both.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/common/vp9_reconintra.c`
//! and `vpx_dsp/intrapred.c` (copyright the WebM project authors), used under
//! libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "the edge arrays are sized for the largest block (32 pixels, 64 above with the top-left) and every index is a loop position within the block size; plane accesses go through get"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sums of at most 64 samples of at most 12 bits, and positions within a plane already allocated, cannot overflow"
)]

use crate::common::{
    D45_PRED, D63_PRED, D117_PRED, D135_PRED, D153_PRED, D207_PRED, DC_PRED, H_PRED,
    PredictionMode, TM_PRED, TxSize, V_PRED,
};
use crate::frame::Pixel;

/// What a mode reads: libvpx's `extend_modes`.
const NEED_LEFT: u8 = 1 << 1;
const NEED_ABOVE: u8 = 1 << 2;
const NEED_ABOVERIGHT: u8 = 1 << 3;

fn extend_modes(mode: PredictionMode) -> u8 {
    match mode {
        DC_PRED | D135_PRED | D117_PRED | D153_PRED | TM_PRED => NEED_ABOVE | NEED_LEFT,
        V_PRED => NEED_ABOVE,
        H_PRED | D207_PRED => NEED_LEFT,
        D45_PRED | D63_PRED => NEED_ABOVERIGHT,
        _ => 0,
    }
}

/// Where a transform block sits and which of its neighbours exist.
#[derive(Clone, Copy, Debug)]
pub struct Edges {
    /// Whether the row above is decoded: inside the block, or a block above.
    pub have_top: bool,
    /// Whether the column to the left is: inside the block, or a block to
    /// the left in the same tile.
    pub have_left: bool,
    /// Whether the pixels above and to the right are, which for libvpx means
    /// only that they lie inside the same block.
    pub have_right: bool,
    /// Whether the block reaches past the frame's right or bottom edge:
    /// libvpx's `mb_to_right_edge < 0` and `mb_to_bottom_edge < 0`.
    pub past_right: bool,
    pub past_bottom: bool,
    /// The plane's size rounded up to 8 luma pixels.
    pub frame_width: usize,
    pub frame_height: usize,
}

/// libvpx's `AVG2`.
#[inline(always)]
fn avg2(a: i32, b: i32) -> i32 {
    (a + b + 1) >> 1
}

/// libvpx's `AVG3`.
#[inline(always)]
fn avg3(a: i32, b: i32, c: i32) -> i32 {
    (a + 2 * b + c + 2) >> 2
}

/// The edge a block predicts from: `above[0]` is the pixel above-left,
/// `above[1 + i]` the row above (two block widths of it), `left[i]` the
/// column to the left.
struct Edge {
    above: [i32; 65],
    left: [i32; 32],
}

impl Edge {
    #[inline(always)]
    fn a(&self, i: isize) -> i32 {
        // above[-1] is the top-left pixel.
        self.above[(i + 1) as usize]
    }
}

/// One sample of `plane`, or 0 outside it -- which libvpx's geometry never
/// asks for, but a caller's mistake must not turn into a panic. A position
/// left of or above the plane arrives wrapped to a huge one, and misses too.
#[inline(always)]
fn px<P: Pixel>(plane: &[P], stride: usize, x: usize, y: usize) -> i32 {
    if x >= stride {
        return 0;
    }
    y.checked_mul(stride)
        .and_then(|row| row.checked_add(x))
        .and_then(|i| plane.get(i))
        .map_or(0, |p| p.int())
}

/// A block's predicted samples before they are stored, `[row][column]`.
/// The decoder keeps one and lends it to every prediction, so that
/// predicting a 4x4 block does not clear 4 KiB first.
pub type Prediction = [[i32; 32]; 32];

/// Predict the `4 << tx_size` square block at (`x0`, `y0`) of `plane` with
/// `mode`, reading its neighbours from the same plane: libvpx's
/// `vp9_predict_intra_block` and `build_intra_predictors(_high)`. `out` is
/// scratch space; what it holds before and after is of no consequence.
#[allow(clippy::too_many_arguments)]
pub fn predict<P: Pixel>(
    plane: &mut [P],
    stride: usize,
    x0: usize,
    y0: usize,
    mode: PredictionMode,
    tx_size: TxSize,
    e: &Edges,
    bit_depth: u8,
    out: &mut Prediction,
) {
    let bs = 4usize << tx_size.min(3);
    let base = 128i32 << (bit_depth.clamp(8, 12) - 8);
    let mut edge = Edge {
        above: [0; 65],
        left: [0; 32],
    };
    let need = extend_modes(mode);
    let (fw, fh) = (e.frame_width, e.frame_height);

    if need & NEED_LEFT != 0 {
        if e.have_left {
            // The column to the left, repeating its last pixel past the
            // frame's bottom.
            let rows = if e.past_bottom && y0 + bs > fh {
                fh.saturating_sub(y0)
            } else {
                bs
            };
            for i in 0..bs {
                let y = y0 + i.min(rows.saturating_sub(1));
                edge.left[i] = px(plane, stride, x0.wrapping_sub(1), y);
            }
        } else {
            edge.left[..bs].fill(base + 1);
        }
    }

    if need & NEED_ABOVE != 0 {
        if e.have_top {
            let y = y0.wrapping_sub(1);
            let n = if e.past_right {
                if x0 + bs <= fw {
                    bs
                } else {
                    fw.saturating_sub(x0)
                }
            } else {
                bs
            };
            for i in 0..bs {
                let x = x0 + i.min(n.saturating_sub(1));
                edge.above[1 + i] = px(plane, stride, x, y);
            }
            edge.above[0] = if e.have_left {
                px(plane, stride, x0.wrapping_sub(1), y)
            } else {
                base + 1
            };
        } else {
            edge.above[..=bs].fill(base - 1);
        }
    }

    if need & NEED_ABOVERIGHT != 0 {
        if e.have_top {
            let y = y0.wrapping_sub(1);
            // How many real pixels to read; the rest repeat the last read.
            let n = if e.past_right {
                if x0 + 2 * bs <= fw {
                    if e.have_right && bs == 4 { 2 * bs } else { bs }
                } else if x0 + bs <= fw {
                    if e.have_right && bs == 4 { fw - x0 } else { bs }
                } else {
                    fw.saturating_sub(x0)
                }
            } else if bs == 4 && e.have_right {
                2 * bs
            } else {
                bs
            };
            for i in 0..2 * bs {
                let x = x0 + i.min(n.saturating_sub(1));
                edge.above[1 + i] = px(plane, stride, x, y);
            }
            edge.above[0] = if e.have_left {
                px(plane, stride, x0.wrapping_sub(1), y)
            } else {
                base + 1
            };
        } else {
            edge.above[..=2 * bs].fill(base - 1);
        }
    }

    let max = (1i32 << bit_depth.clamp(8, 12)) - 1;
    kernel(mode, bs, &edge, e.have_left, e.have_top, base, max, out);

    for (r, row) in out.iter().enumerate().take(bs) {
        let start = (y0 + r) * stride + x0;
        if let Some(dst) = plane.get_mut(start..start + bs) {
            for (d, &v) in dst.iter_mut().zip(row) {
                *d = P::from_int(v);
            }
        }
    }
}

/// Run `mode`'s predictor for a `bs`-pixel block over `edge`, writing every
/// sample of the block's `bs` x `bs` corner of `out`. DC chooses its
/// variant by which edges exist; TM clips to `0..=max`.
#[allow(clippy::too_many_arguments)]
fn kernel(
    mode: PredictionMode,
    bs: usize,
    edge: &Edge,
    have_left: bool,
    have_top: bool,
    base: i32,
    max: i32,
    out: &mut Prediction,
) {
    match (mode, bs) {
        (DC_PRED, _) => dc(edge, bs, have_left, have_top, base, out),
        (V_PRED, _) => {
            for row in out.iter_mut().take(bs) {
                for c in 0..bs {
                    row[c] = edge.a(c as isize);
                }
            }
        }
        (H_PRED, _) => {
            for (r, row) in out.iter_mut().enumerate().take(bs) {
                row[..bs].fill(edge.left[r]);
            }
        }
        (TM_PRED, _) => {
            let top_left = edge.a(-1);
            for (r, row) in out.iter_mut().enumerate().take(bs) {
                for c in 0..bs {
                    row[c] = (edge.left[r] + edge.a(c as isize) - top_left).clamp(0, max);
                }
            }
        }
        (D207_PRED, 4) => d207_4x4(edge, out),
        (D63_PRED, 4) => d63_4x4(edge, out),
        (D45_PRED, 4) => d45_4x4(edge, out),
        (D117_PRED, 4) => d117_4x4(edge, out),
        (D135_PRED, 4) => d135_4x4(edge, out),
        (D153_PRED, 4) => d153_4x4(edge, out),
        (D207_PRED, _) => d207(edge, bs, out),
        (D63_PRED, _) => d63(edge, bs, out),
        (D45_PRED, _) => d45(edge, bs, out),
        (D117_PRED, _) => d117(edge, bs, out),
        (D135_PRED, _) => d135(edge, bs, out),
        (D153_PRED, _) => d153(edge, bs, out),
        // Not an intra mode, which the mode readers never produce: a flat
        // block rather than whatever the scratch held.
        _ => {
            for row in out.iter_mut().take(bs) {
                row[..bs].fill(base);
            }
        }
    }
}

/// libvpx's `dc_predictor`, `dc_top_predictor`, `dc_left_predictor` and
/// `dc_128_predictor`, chosen by which edges exist.
fn dc(
    edge: &Edge,
    bs: usize,
    have_left: bool,
    have_top: bool,
    base: i32,
    out: &mut [[i32; 32]; 32],
) {
    let n = bs as i32;
    let above: i32 = (0..bs).map(|i| edge.a(i as isize)).sum();
    let left: i32 = edge.left[..bs].iter().sum();
    let v = match (have_left, have_top) {
        (true, true) => (above + left + n) / (2 * n),
        (false, true) => (above + (n >> 1)) / n,
        (true, false) => (left + (n >> 1)) / n,
        (false, false) => base,
    };
    for row in out.iter_mut().take(bs) {
        row[..bs].fill(v);
    }
}

/// libvpx's `d207_predictor`, for 8x8 and up.
fn d207(e: &Edge, bs: usize, out: &mut [[i32; 32]; 32]) {
    let l = &e.left;
    // First column.
    for r in 0..bs - 1 {
        out[r][0] = avg2(l[r], l[r + 1]);
    }
    out[bs - 1][0] = l[bs - 1];
    // Second column.
    for r in 0..bs - 2 {
        out[r][1] = avg3(l[r], l[r + 1], l[r + 2]);
    }
    out[bs - 2][1] = avg3(l[bs - 2], l[bs - 1], l[bs - 1]);
    out[bs - 1][1] = l[bs - 1];
    // The rest of the last row.
    for c in 0..bs - 2 {
        out[bs - 1][2 + c] = l[bs - 1];
    }
    // Each row is the one below it, two columns on.
    for r in (0..bs - 1).rev() {
        for c in 0..bs - 2 {
            out[r][2 + c] = out[r + 1][c];
        }
    }
}

/// libvpx's `d63_predictor`, for 8x8 and up.
fn d63(e: &Edge, bs: usize, out: &mut [[i32; 32]; 32]) {
    for c in 0..bs {
        let c = c as isize;
        out[0][c as usize] = avg2(e.a(c), e.a(c + 1));
        out[1][c as usize] = avg3(e.a(c), e.a(c + 1), e.a(c + 2));
    }
    let fill = e.a(bs as isize - 1);
    let mut size = bs - 2;
    let mut r = 2;
    while r < bs {
        for k in 0..2 {
            let src = out[k];
            let row = &mut out[r + k];
            row[..size].copy_from_slice(&src[r / 2..r / 2 + size]);
            row[size..bs].fill(fill);
        }
        r += 2;
        size -= 1;
    }
}

/// libvpx's `d45_predictor`, for 8x8 and up.
fn d45(e: &Edge, bs: usize, out: &mut [[i32; 32]; 32]) {
    let above_right = e.a(bs as isize - 1);
    for x in 0..bs - 1 {
        let x = x as isize;
        out[0][x as usize] = avg3(e.a(x), e.a(x + 1), e.a(x + 2));
    }
    out[0][bs - 1] = above_right;
    let row0 = out[0];
    for x in 1..bs {
        let size = bs - 1 - x;
        out[x][..size].copy_from_slice(&row0[x..x + size]);
        out[x][size..bs].fill(above_right);
    }
}

/// libvpx's `d117_predictor`, for 8x8 and up.
fn d117(e: &Edge, bs: usize, out: &mut [[i32; 32]; 32]) {
    let l = &e.left;
    // First row.
    for c in 0..bs {
        let c = c as isize;
        out[0][c as usize] = avg2(e.a(c - 1), e.a(c));
    }
    // Second row.
    out[1][0] = avg3(l[0], e.a(-1), e.a(0));
    for c in 1..bs {
        let c = c as isize;
        out[1][c as usize] = avg3(e.a(c - 2), e.a(c - 1), e.a(c));
    }
    // The rest of the first column.
    out[2][0] = avg3(e.a(-1), l[0], l[1]);
    for r in 3..bs {
        out[r][0] = avg3(l[r - 3], l[r - 2], l[r - 1]);
    }
    // The rest of the block: each row is the one two above, one column on.
    for r in 2..bs {
        for c in 1..bs {
            out[r][c] = out[r - 2][c - 1];
        }
    }
}

/// libvpx's `d135_predictor`, for 8x8 and up.
fn d135(e: &Edge, bs: usize, out: &mut [[i32; 32]; 32]) {
    let l = &e.left;
    let mut border = [0i32; 63];
    // From the bottom of the left column up...
    for i in 0..bs - 2 {
        border[i] = avg3(l[bs - 3 - i], l[bs - 2 - i], l[bs - 1 - i]);
    }
    border[bs - 2] = avg3(e.a(-1), l[0], l[1]);
    border[bs - 1] = avg3(l[0], e.a(-1), e.a(0));
    border[bs] = avg3(e.a(-1), e.a(0), e.a(1));
    // ...then along the row above.
    for i in 0..bs - 2 {
        let a = i as isize;
        border[bs + 1 + i] = avg3(e.a(a), e.a(a + 1), e.a(a + 2));
    }
    for (i, row) in out.iter_mut().enumerate().take(bs) {
        row[..bs].copy_from_slice(&border[bs - 1 - i..2 * bs - 1 - i]);
    }
}

/// libvpx's `d153_predictor`, for 8x8 and up.
fn d153(e: &Edge, bs: usize, out: &mut [[i32; 32]; 32]) {
    let l = &e.left;
    out[0][0] = avg2(e.a(-1), l[0]);
    for r in 1..bs {
        out[r][0] = avg2(l[r - 1], l[r]);
    }
    out[0][1] = avg3(l[0], e.a(-1), e.a(0));
    out[1][1] = avg3(e.a(-1), l[0], l[1]);
    for r in 2..bs {
        out[r][1] = avg3(l[r - 2], l[r - 1], l[r]);
    }
    for c in 0..bs - 2 {
        let a = c as isize;
        out[0][2 + c] = avg3(e.a(a - 1), e.a(a), e.a(a + 1));
    }
    for r in 1..bs {
        for c in 0..bs - 2 {
            out[r][2 + c] = out[r - 1][c];
        }
    }
}

// --- 4x4: libvpx's special versions ---------------------------------------------------

/// libvpx's `vpx_d207_predictor_4x4_c`. `out[y][x]` is libvpx's `DST(x, y)`.
fn d207_4x4(e: &Edge, out: &mut [[i32; 32]; 32]) {
    let [i, j, k, l] = [e.left[0], e.left[1], e.left[2], e.left[3]];
    out[0][0] = avg2(i, j);
    out[0][2] = avg2(j, k);
    out[1][0] = out[0][2];
    out[1][2] = avg2(k, l);
    out[2][0] = out[1][2];
    out[0][1] = avg3(i, j, k);
    out[0][3] = avg3(j, k, l);
    out[1][1] = out[0][3];
    out[1][3] = avg3(k, l, l);
    out[2][1] = out[1][3];
    for (y, x) in [(2, 3), (2, 2), (3, 0), (3, 1), (3, 2), (3, 3)] {
        out[y][x] = l;
    }
}

/// libvpx's `vpx_d63_predictor_4x4_c`.
fn d63_4x4(e: &Edge, out: &mut [[i32; 32]; 32]) {
    let [a, b, c, d, ee, f, g] = [e.a(0), e.a(1), e.a(2), e.a(3), e.a(4), e.a(5), e.a(6)];
    out[0][0] = avg2(a, b);
    out[0][1] = avg2(b, c);
    out[2][0] = out[0][1];
    out[0][2] = avg2(c, d);
    out[2][1] = out[0][2];
    out[0][3] = avg2(d, ee);
    out[2][2] = out[0][3];
    out[2][3] = avg2(ee, f);

    out[1][0] = avg3(a, b, c);
    out[1][1] = avg3(b, c, d);
    out[3][0] = out[1][1];
    out[1][2] = avg3(c, d, ee);
    out[3][1] = out[1][2];
    out[1][3] = avg3(d, ee, f);
    out[3][2] = out[1][3];
    out[3][3] = avg3(ee, f, g);
}

/// libvpx's `vpx_d45_predictor_4x4_c`.
fn d45_4x4(e: &Edge, out: &mut [[i32; 32]; 32]) {
    let [a, b, c, d, ee, f, g, h] = [
        e.a(0),
        e.a(1),
        e.a(2),
        e.a(3),
        e.a(4),
        e.a(5),
        e.a(6),
        e.a(7),
    ];
    // Each anti-diagonal x + y is one value.
    let diag = [
        avg3(a, b, c),
        avg3(b, c, d),
        avg3(c, d, ee),
        avg3(d, ee, f),
        avg3(ee, f, g),
        avg3(f, g, h),
        h,
    ];
    for (y, row) in out.iter_mut().enumerate().take(4) {
        for (x, v) in row.iter_mut().enumerate().take(4) {
            *v = diag[x + y];
        }
    }
}

/// libvpx's `vpx_d117_predictor_4x4_c`.
fn d117_4x4(e: &Edge, out: &mut [[i32; 32]; 32]) {
    let [i, j, k] = [e.left[0], e.left[1], e.left[2]];
    let [x, a, b, c, d] = [e.a(-1), e.a(0), e.a(1), e.a(2), e.a(3)];
    out[0][0] = avg2(x, a);
    out[2][1] = out[0][0];
    out[0][1] = avg2(a, b);
    out[2][2] = out[0][1];
    out[0][2] = avg2(b, c);
    out[2][3] = out[0][2];
    out[0][3] = avg2(c, d);

    out[3][0] = avg3(k, j, i);
    out[2][0] = avg3(j, i, x);
    out[1][0] = avg3(i, x, a);
    out[3][1] = out[1][0];
    out[1][1] = avg3(x, a, b);
    out[3][2] = out[1][1];
    out[1][2] = avg3(a, b, c);
    out[3][3] = out[1][2];
    out[1][3] = avg3(b, c, d);
}

/// libvpx's `vpx_d135_predictor_4x4_c`.
fn d135_4x4(e: &Edge, out: &mut [[i32; 32]; 32]) {
    let [i, j, k, l] = [e.left[0], e.left[1], e.left[2], e.left[3]];
    let [x, a, b, c, d] = [e.a(-1), e.a(0), e.a(1), e.a(2), e.a(3)];
    // Each diagonal x - y is one value, from bottom-left (-3) to top-right (3).
    let diag = [
        avg3(j, k, l),
        avg3(i, j, k),
        avg3(x, i, j),
        avg3(a, x, i),
        avg3(b, a, x),
        avg3(c, b, a),
        avg3(d, c, b),
    ];
    for (yy, row) in out.iter_mut().enumerate().take(4) {
        for (xx, v) in row.iter_mut().enumerate().take(4) {
            *v = diag[3 + xx - yy];
        }
    }
}

/// libvpx's `vpx_d153_predictor_4x4_c`.
fn d153_4x4(e: &Edge, out: &mut [[i32; 32]; 32]) {
    let [i, j, k, l] = [e.left[0], e.left[1], e.left[2], e.left[3]];
    let [x, a, b, c] = [e.a(-1), e.a(0), e.a(1), e.a(2)];
    out[0][0] = avg2(i, x);
    out[1][2] = out[0][0];
    out[1][0] = avg2(j, i);
    out[2][2] = out[1][0];
    out[2][0] = avg2(k, j);
    out[3][2] = out[2][0];
    out[3][0] = avg2(l, k);

    out[0][3] = avg3(a, b, c);
    out[0][2] = avg3(x, a, b);
    out[0][1] = avg3(i, x, a);
    out[1][3] = out[0][1];
    out[1][1] = avg3(j, i, x);
    out[2][3] = out[1][1];
    out[2][1] = avg3(k, j, i);
    out[3][3] = out[2][1];
    out[3][1] = avg3(l, k, j);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]

    use super::*;

    /// libvpx's predictor results on the seeded edges:
    /// `tools/intra_reference.c`'s output, as (bit depth, transform size,
    /// mode index, hash). Mode indices 0 to 8 are libvpx's modes 1 to 9 (V
    /// to TM); 9 to 12 are DC with both edges, the top only, the left only,
    /// and neither.
    const LIBVPX_KERNELS: [(u8, u8, u8, u64); 156] = [
        (8, 0, 0, 0x124aa86456edf285),
        (8, 0, 1, 0xef030787e304db81),
        (8, 0, 2, 0xbeabe3698be16095),
        (8, 0, 3, 0xa60e38eff5b16089),
        (8, 0, 4, 0x6a50c13f44edd892),
        (8, 0, 5, 0xc7a45fe6b63a31a9),
        (8, 0, 6, 0x7d2aa86d0896fb03),
        (8, 0, 7, 0xff9189c52c225567),
        (8, 0, 8, 0xd361938faae3b2b4),
        (8, 0, 9, 0xb7b73431b5700ba5),
        (8, 0, 10, 0x44d2d12c10451ef5),
        (8, 0, 11, 0xd816d183debf5e65),
        (8, 0, 12, 0xd2658ef7a0c54b25),
        (8, 1, 0, 0x284359eec4f95d55),
        (8, 1, 1, 0xdad1ae48cc473ee5),
        (8, 1, 2, 0xef9a20f5bec76232),
        (8, 1, 3, 0x6eeade21da85489e),
        (8, 1, 4, 0x74032e0adeb6d4fe),
        (8, 1, 5, 0x3c33c25aa043cc28),
        (8, 1, 6, 0x5b0f8ec887a1e527),
        (8, 1, 7, 0x360421819fa2fbb2),
        (8, 1, 8, 0xb3ebcafb494801d4),
        (8, 1, 9, 0xa21f6f5553b637a5),
        (8, 1, 10, 0x0d791c06e1431be5),
        (8, 1, 11, 0x3efc2fc45d34f5e5),
        (8, 1, 12, 0x3bec19a56e2ec325),
        (8, 2, 0, 0xa5fab21d5f17ea25),
        (8, 2, 1, 0xb1a4dff38193b715),
        (8, 2, 2, 0x1729e948d8cfb560),
        (8, 2, 3, 0x77d48a4c8bbc66f7),
        (8, 2, 4, 0x4e05f235bb2bc5ee),
        (8, 2, 5, 0xefca863e09c182b0),
        (8, 2, 6, 0xfb7e4fb1f32c3543),
        (8, 2, 7, 0x496dd1aed2b76a21),
        (8, 2, 8, 0x62c5c48584ed4798),
        (8, 2, 9, 0xe766132cab435025),
        (8, 2, 10, 0x935fe68eb9d9cb25),
        (8, 2, 11, 0x2f2ceb235e2c0525),
        (8, 2, 12, 0x25b431efa454a325),
        (8, 3, 0, 0x0cd069474516cf25),
        (8, 3, 1, 0xe7dd5ac1e99b8005),
        (8, 3, 2, 0x2ddb678e99ce5b8c),
        (8, 3, 3, 0x261ea063ab5975a1),
        (8, 3, 4, 0xee07934ac20e7389),
        (8, 3, 5, 0x180f7ec70b511008),
        (8, 3, 6, 0x59175c3a7e402a9f),
        (8, 3, 7, 0xb07f9d74f5ec465a),
        (8, 3, 8, 0x5ca2b2b5febf77a0),
        (8, 3, 9, 0xe24807e658c83325),
        (8, 3, 10, 0xf860abe6de3e3b25),
        (8, 3, 11, 0xab83c6f93e34ff25),
        (8, 3, 12, 0xa369c18884ec2325),
        (10, 0, 0, 0xe72249db747d3255),
        (10, 0, 1, 0xe042e4546af2b38d),
        (10, 0, 2, 0x39f47cca8f1323f7),
        (10, 0, 3, 0x8744b21817a770a1),
        (10, 0, 4, 0xf262c007c6907ce3),
        (10, 0, 5, 0x80ee315a07d85c30),
        (10, 0, 6, 0x02c71fb5b8c275d3),
        (10, 0, 7, 0xd0cadbaa0d8f951c),
        (10, 0, 8, 0x0322ce9b3d2df717),
        (10, 0, 9, 0x6a042c4b48c7fdc5),
        (10, 0, 10, 0x614919de1ce23285),
        (10, 0, 11, 0xde06d892e781eaa5),
        (10, 0, 12, 0x495ff729558a0325),
        (10, 1, 0, 0x7694e006b2ebdb45),
        (10, 1, 1, 0xbe0c4d64250dc185),
        (10, 1, 2, 0xa1b903d588e1ecc4),
        (10, 1, 3, 0xb3bc050fe88e9fbe),
        (10, 1, 4, 0x6d52f94014c73a4b),
        (10, 1, 5, 0x2f884d45fecabdc4),
        (10, 1, 6, 0xaa5a0893f30fa27f),
        (10, 1, 7, 0x523bfe50da8bcd43),
        (10, 1, 8, 0x4c4565a0ea2990c8),
        (10, 1, 9, 0x3fa335f39e050525),
        (10, 1, 10, 0xa353b239d09d1325),
        (10, 1, 11, 0x788b4be65da77da5),
        (10, 1, 12, 0x12e6b44bbdc1a325),
        (10, 2, 0, 0x11d011130237a5c5),
        (10, 2, 1, 0xeec2bd0be9396205),
        (10, 2, 2, 0x59faac996bb2a556),
        (10, 2, 3, 0x8bd93d8e0aaf6690),
        (10, 2, 4, 0xdd0e89b772c1557a),
        (10, 2, 5, 0xcb00e2b42a49c74a),
        (10, 2, 6, 0x8427afa4edea1552),
        (10, 2, 7, 0x3b493f8648b9ad80),
        (10, 2, 8, 0x9fa7efd13b135a49),
        (10, 2, 9, 0xf9728364c6d83d25),
        (10, 2, 10, 0x82babeec5f338125),
        (10, 2, 11, 0x96588a84539e3125),
        (10, 2, 12, 0xb582a3c0aaa02325),
        (10, 3, 0, 0xe15cea90bab756e5),
        (10, 3, 1, 0xc5e6c57737618ea5),
        (10, 3, 2, 0x6125c0b549a508e8),
        (10, 3, 3, 0xcc3f3ea6b765b979),
        (10, 3, 4, 0x67a8ec075c7ead25),
        (10, 3, 5, 0x3234658e7e5be72a),
        (10, 3, 6, 0x528d32df825f8c4e),
        (10, 3, 7, 0xf95c2235db640536),
        (10, 3, 8, 0x190c70afd5727c97),
        (10, 3, 9, 0x402c2cf07d636b25),
        (10, 3, 10, 0x1ffbf29eb7798b25),
        (10, 3, 11, 0xe337ea3441a04325),
        (10, 3, 12, 0xff5e4c491e1a2325),
        (12, 0, 0, 0xb120bdc3e7df18dd),
        (12, 0, 1, 0x118186ed990ceead),
        (12, 0, 2, 0x9fdacdac25a47449),
        (12, 0, 3, 0x2208e6f610516a59),
        (12, 0, 4, 0x6294260a0a79f57c),
        (12, 0, 5, 0x2377eb8057b9a6db),
        (12, 0, 6, 0x37b52ad9b6076b2b),
        (12, 0, 7, 0x3483c66ac88040fd),
        (12, 0, 8, 0x2cfac5939756786c),
        (12, 0, 9, 0xb4ad6402b7f7cfe5),
        (12, 0, 10, 0xa7d52118545fdca5),
        (12, 0, 11, 0x8023589fbdfd2265),
        (12, 0, 12, 0x1d6f16623f090325),
        (12, 1, 0, 0x6ecbf1aefef1b9f5),
        (12, 1, 1, 0x0dcd3f778441bb65),
        (12, 1, 2, 0x808f3c275b98a51c),
        (12, 1, 3, 0xff9a9f308345fd39),
        (12, 1, 4, 0xd9bbe5ed6dfeaae4),
        (12, 1, 5, 0x54f750dd6b2b9ddd),
        (12, 1, 6, 0x6fb2c762d911e543),
        (12, 1, 7, 0x74bf0525959e0854),
        (12, 1, 8, 0x336d1972650fa16a),
        (12, 1, 9, 0xcb6dd658fbb92ba5),
        (12, 1, 10, 0xd5edfbea31717225),
        (12, 1, 11, 0xfa4162243e033325),
        (12, 1, 12, 0xd2c53d2703bda325),
        (12, 2, 0, 0xc0d068eb476b4c45),
        (12, 2, 1, 0x9e42e05ceb14a4e5),
        (12, 2, 2, 0x1eb20a83656e7863),
        (12, 2, 3, 0xc0589e89111eadf4),
        (12, 2, 4, 0x2e1bd6e423c43641),
        (12, 2, 5, 0xb01fed3963fc00d1),
        (12, 2, 6, 0xbf690dce75308e12),
        (12, 2, 7, 0x81eae4a1d8616faf),
        (12, 2, 8, 0x6307f9b2c1a345dc),
        (12, 2, 9, 0x89e0528930b29d25),
        (12, 2, 10, 0x15413cddff12eb25),
        (12, 2, 11, 0xa2406f7fe92ec725),
        (12, 2, 12, 0xaa7ba6a7c2902325),
        (12, 3, 0, 0x45f5192d9c57cb25),
        (12, 3, 1, 0xa1a12b4c9b432965),
        (12, 3, 2, 0x5e97578e6648f8dd),
        (12, 3, 3, 0x19a1fed1ed79510f),
        (12, 3, 4, 0x673463ae88b22256),
        (12, 3, 5, 0xe5cc56bb5ea6302c),
        (12, 3, 6, 0xe7b404edc3cbfcec),
        (12, 3, 7, 0x0c9e3bfd13fa11ca),
        (12, 3, 8, 0x350ae0307f73a971),
        (12, 3, 9, 0xc753fe2683a9db25),
        (12, 3, 10, 0x0d4f2c9f483c4b25),
        (12, 3, 11, 0xe73fc644e1616325),
        (12, 3, 12, 0xe2b84f857dda2325),
    ];

    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0 >> 8
        }
    }

    fn fnv(bytes: impl IntoIterator<Item = u8>, mut h: u64) -> u64 {
        for b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }

    #[test]
    fn kernels_match_libvpx() {
        // Replays `tools/intra_reference.c` call for call.
        let mut rng = Lcg(0x1a7a_5eed);
        let mut expected = LIBVPX_KERNELS.iter();
        for bd in [8u8, 10, 12] {
            let base = 128i32 << (bd - 8);
            let max = (1i32 << bd) - 1;
            for tx in 0u8..4 {
                let bs = 4usize << tx;
                for m in 0u8..13 {
                    let (mode, have_left, have_top) = match m {
                        0..=8 => (m + 1, true, true),
                        9 => (DC_PRED, true, true),
                        10 => (DC_PRED, false, true),
                        11 => (DC_PRED, true, false),
                        _ => (DC_PRED, false, false),
                    };
                    let mut h = 0xcbf2_9ce4_8422_2325u64;
                    for _ in 0..32 {
                        let mut edge = Edge {
                            above: [0; 65],
                            left: [0; 32],
                        };
                        let mask = if bd == 8 { 0xff } else { (1u32 << bd) - 1 };
                        for a in edge.above.iter_mut().take(1 + 2 * bs) {
                            *a = (rng.next() & mask) as i32;
                        }
                        for l in edge.left.iter_mut().take(bs) {
                            *l = (rng.next() & mask) as i32;
                        }
                        // Garbage in the scratch first: a sample the kernel left
                        // unwritten would change the hash.
                        let mut out = [[-1; 32]; 32];
                        kernel(mode, bs, &edge, have_left, have_top, base, max, &mut out);
                        for row in out.iter().take(bs) {
                            for &v in row.iter().take(bs) {
                                h = if bd == 8 {
                                    fnv([v as u8], h)
                                } else {
                                    fnv((v as u16).to_le_bytes(), h)
                                };
                            }
                        }
                    }
                    let &(want_bd, want_tx, want_m, want) = expected.next().unwrap();
                    assert_eq!((want_bd, want_tx, want_m), (bd, tx, m));
                    assert_eq!(h, want, "{bd}-bit {bs}x{bs}, mode index {m}");
                }
            }
        }
        assert!(expected.next().is_none());
    }

    /// A plane of `w` x `h` holding `f(x, y)`.
    fn plane(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y))
            .collect()
    }

    fn edges(have_top: bool, have_left: bool) -> Edges {
        Edges {
            have_top,
            have_left,
            have_right: false,
            past_right: false,
            past_bottom: false,
            frame_width: 64,
            frame_height: 64,
        }
    }

    #[test]
    fn missing_edges_read_as_127_above_and_129_left() {
        let mut p = plane(64, 64, |_, _| 50);
        // No row above: V predicts 127 everywhere.
        predict(
            &mut p,
            64,
            0,
            0,
            V_PRED,
            0,
            &edges(false, false),
            8,
            &mut [[0; 32]; 32],
        );
        assert!((0..4).all(|y| p[y * 64..y * 64 + 4].iter().all(|&v| v == 127)));
        // No column to the left: H predicts 129.
        predict(
            &mut p,
            64,
            8,
            8,
            H_PRED,
            1,
            &edges(true, false),
            8,
            &mut [[0; 32]; 32],
        );
        assert!((8..16).all(|y| p[y * 64 + 8..y * 64 + 16].iter().all(|&v| v == 129)));
        // At 10 bits, one either side of 512.
        let mut q = vec![0u16; 64 * 64];
        predict(
            &mut q,
            64,
            0,
            0,
            V_PRED,
            0,
            &edges(false, false),
            10,
            &mut [[0; 32]; 32],
        );
        assert_eq!(q[0], 511);
        predict(
            &mut q,
            64,
            0,
            0,
            H_PRED,
            0,
            &edges(false, false),
            10,
            &mut [[0; 32]; 32],
        );
        assert_eq!(q[0], 513);
    }

    #[test]
    fn an_edge_past_the_frame_repeats_its_last_pixel() {
        // A 16-wide frame; an 8x8 block at x = 12 reaches 4 past it. The row
        // above holds 0..63 by column: V must stop at column 15's value.
        let mut p = plane(64, 64, |x, _| x as u8);
        let e = Edges {
            past_right: true,
            frame_width: 16,
            ..edges(true, true)
        };
        predict(&mut p, 64, 12, 8, V_PRED, 1, &e, 8, &mut [[0; 32]; 32]);
        assert_eq!(
            &p[8 * 64 + 12..8 * 64 + 20],
            &[12, 13, 14, 15, 15, 15, 15, 15]
        );
        // Downward, H repeats the last row inside the frame.
        let mut p = plane(64, 64, |_, y| y as u8);
        let e = Edges {
            past_bottom: true,
            frame_height: 10,
            ..edges(true, true)
        };
        predict(&mut p, 64, 8, 4, H_PRED, 1, &e, 8, &mut [[0; 32]; 32]);
        let col: Vec<u8> = (4..12).map(|y| p[y * 64 + 8]).collect();
        assert_eq!(col, vec![4, 5, 6, 7, 8, 9, 9, 9]);
    }

    #[test]
    fn only_a_4x4_block_reads_real_pixels_above_and_right() {
        // The row above the block is 10 * x; the block sits at x = 8.
        let ramp = |x: usize, y: usize| if y == 3 { (10 * x) as u8 } else { 0 };
        let right = Edges {
            have_right: true,
            ..edges(true, true)
        };
        // 4x4 with the pixels above-right: D45's bottom-right corner is
        // libvpx's H, above[7] = 10 * 15.
        let mut p = plane(64, 64, ramp);
        predict(&mut p, 64, 8, 4, D45_PRED, 0, &right, 8, &mut [[0; 32]; 32]);
        assert_eq!(p[7 * 64 + 11], 150);
        // Without them, the row past the block repeats above[3] = 110.
        let mut p = plane(64, 64, ramp);
        predict(
            &mut p,
            64,
            8,
            4,
            D45_PRED,
            0,
            &edges(true, true),
            8,
            &mut [[0; 32]; 32],
        );
        assert_eq!(p[7 * 64 + 11], 110);
        // 8x8 never reads past its own width: above[8] repeats above[7] =
        // 150, so AVG3(140, 150, 150) = 148 rather than the real 150.
        let mut p = plane(64, 64, ramp);
        predict(&mut p, 64, 8, 4, D45_PRED, 1, &right, 8, &mut [[0; 32]; 32]);
        assert_eq!(p[4 * 64 + 8 + 6], 148);
    }

    #[test]
    fn a_neighbour_outside_the_plane_reads_zero_and_never_panics() {
        // have_left at x = 0 and have_top at y = 0 name pixels that do not
        // exist; libvpx never asks, and here asking is harmless.
        let mut p = plane(64, 64, |_, _| 9);
        for mode in 0..10 {
            predict(
                &mut p,
                64,
                0,
                0,
                mode,
                2,
                &edges(true, true),
                8,
                &mut [[0; 32]; 32],
            );
        }
        let mut q = vec![0u16; 16];
        predict(
            &mut q,
            4,
            0,
            0,
            TM_PRED,
            3,
            &edges(true, true),
            12,
            &mut [[0; 32]; 32],
        );
    }
}

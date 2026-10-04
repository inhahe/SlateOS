//! Intra prediction: a block predicted from the reconstructed pixels above
//! it and to its left, before the loop filter has touched them.
//!
//! A macroblock predicts its luma as one 16x16 block or as sixteen 4x4
//! blocks, each with its own mode, and its chroma as two 8x8 blocks. The
//! pixels outside the picture are what `frame` puts there: 127 above the
//! first row, 129 left of the first column, and the row above's last pixels
//! carried four to the right for the last macroblock of a row.
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/common/reconintra.c`,
//! `vp8/common/reconintra4x4.c` and `reconintra4x4.h`, and the predictors of
//! `vpx_dsp/intrapred.c` that VP8 uses (copyright the WebM project authors),
//! used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "a block's position is a macroblock's inside the plane, whose border holds the row above, the column left and the four pixels above-right that prediction reads; edge arrays are indexed by positions within the block"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sums of at most 32 pixels and positions inside an allocated plane cannot overflow"
)]
#![allow(
    clippy::cast_possible_truncation,
    reason = "averages and clamped sums of pixels are pixels"
)]

use crate::modes::{
    B_DC_PRED, B_HD_PRED, B_HE_PRED, B_HU_PRED, B_LD_PRED, B_RD_PRED, B_TM_PRED, B_VE_PRED,
    B_VL_PRED, B_VR_PRED, DC_PRED, H_PRED, TM_PRED, V_PRED,
};

/// `(a + 2b + c + 2) / 4`: libvpx's `AVG3`.
#[inline]
fn avg3(a: u8, b: u8, c: u8) -> u8 {
    ((u16::from(a) + 2 * u16::from(b) + u16::from(c) + 2) >> 2) as u8
}

/// `(a + b + 1) / 2`: libvpx's `AVG2`.
#[inline]
fn avg2(a: u8, b: u8) -> u8 {
    ((u16::from(a) + u16::from(b) + 1) >> 1) as u8
}

/// Predict the `size` x `size` block at `pos` (16 for luma, 8 for chroma) in
/// `mode`: libvpx's `vp8_build_intra_predictors_mby_s` and the half of
/// `vp8_build_intra_predictors_mbuv_s` for one plane. DC prediction averages
/// only the edges that exist: none on the frame's first macroblock, which
/// predicts 128.
pub(crate) fn predict_mb(
    mode: u8,
    size: usize,
    left_available: bool,
    up_available: bool,
    plane: &mut [u8],
    pos: usize,
    stride: usize,
) {
    let above_at = pos - stride;
    let mut left = [0u8; 16];
    for (i, l) in left.iter_mut().enumerate().take(size) {
        *l = plane[pos + i * stride - 1];
    }
    match mode {
        V_PRED => {
            for r in 1..=size {
                plane.copy_within(above_at..above_at + size, above_at + r * stride);
            }
        }
        H_PRED => {
            for (r, &l) in left.iter().enumerate().take(size) {
                let at = pos + r * stride;
                plane[at..at + size].fill(l);
            }
        }
        TM_PRED => {
            let top_left = i32::from(plane[above_at - 1]);
            let mut above = [0u8; 16];
            above[..size].copy_from_slice(&plane[above_at..above_at + size]);
            for (r, &l) in left.iter().enumerate().take(size) {
                let at = pos + r * stride;
                for c in 0..size {
                    let v = i32::from(l) + i32::from(above[c]) - top_left;
                    plane[at + c] = v.clamp(0, 255) as u8;
                }
            }
        }
        // DC_PRED, and the modes a macroblock cannot have, which libvpx's
        // table lookup would never reach.
        _ => {
            debug_assert_eq!(mode, DC_PRED);
            let shift = if size == 16 { 4 } else { 3 };
            let above_sum: u32 = plane[above_at..above_at + size]
                .iter()
                .map(|&p| u32::from(p))
                .sum();
            let left_sum: u32 = left[..size].iter().map(|&p| u32::from(p)).sum();
            let dc = match (left_available, up_available) {
                (false, false) => 128,
                (false, true) => (above_sum + (1 << (shift - 1))) >> shift,
                (true, false) => (left_sum + (1 << (shift - 1))) >> shift,
                (true, true) => (above_sum + left_sum + (1 << shift)) >> (shift + 1),
            } as u8;
            for r in 0..size {
                let at = pos + r * stride;
                plane[at..at + size].fill(dc);
            }
        }
    }
}

/// Copy the four pixels above and to the right of a macroblock down to rows
/// 3, 7 and 11 of the column to its right, where its 4x4 blocks on the right
/// edge look for their above-right pixels: libvpx's
/// `intra_prediction_down_copy`. Those pixels belong to the next macroblock
/// (or the border), which overwrites them.
pub(crate) fn down_copy_above_right(plane: &mut [u8], pos: usize, stride: usize) {
    let src = pos - stride + 16;
    for row in [3, 7, 11] {
        plane.copy_within(src..src + 4, pos + row * stride + 16);
    }
}

/// Predict the 4x4 block at `pos` in `mode`: libvpx's
/// `vp8_intra4x4_predict` with the predictors it chooses from
/// `vpx_dsp/intrapred.c`. Reads the eight pixels above (four of them
/// above-right), the four left, and the one above-left.
pub(crate) fn predict_4x4(mode: u8, plane: &mut [u8], pos: usize, stride: usize) {
    let above_at = pos - stride;
    let x = plane[above_at - 1];
    let mut a = [0u8; 8];
    a.copy_from_slice(&plane[above_at..above_at + 8]);
    let l = [
        plane[pos - 1],
        plane[pos + stride - 1],
        plane[pos + 2 * stride - 1],
        plane[pos + 3 * stride - 1],
    ];
    let [i, j, k, ll] = l;
    let [aa, b, c, d, e, f, g, h] = a;
    // dst[row][col], libvpx's DST(col, row).
    let mut dst = [[0u8; 4]; 4];
    match mode {
        B_DC_PRED => {
            let sum: u32 = a[..4].iter().chain(&l).map(|&p| u32::from(p)).sum();
            dst = [[((sum + 4) >> 3) as u8; 4]; 4];
        }
        B_TM_PRED => {
            for (r, row) in dst.iter_mut().enumerate() {
                for (col, p) in row.iter_mut().enumerate() {
                    let v = i32::from(l[r]) + i32::from(a[col]) - i32::from(x);
                    *p = v.clamp(0, 255) as u8;
                }
            }
        }
        B_VE_PRED => {
            let row = [avg3(x, aa, b), avg3(aa, b, c), avg3(b, c, d), avg3(c, d, e)];
            dst = [row; 4];
        }
        B_HE_PRED => {
            dst = [
                [avg3(x, i, j); 4],
                [avg3(i, j, k); 4],
                [avg3(j, k, ll); 4],
                [avg3(k, ll, ll); 4],
            ];
        }
        B_LD_PRED => {
            // libvpx's d45e: down and to the left.
            let v = [
                avg3(aa, b, c),
                avg3(b, c, d),
                avg3(c, d, e),
                avg3(d, e, f),
                avg3(e, f, g),
                avg3(f, g, h),
                avg3(g, h, h),
            ];
            for (r, row) in dst.iter_mut().enumerate() {
                for (col, p) in row.iter_mut().enumerate() {
                    *p = v[r + col];
                }
            }
        }
        B_RD_PRED => {
            // libvpx's d135: down and to the right.
            let v = [
                avg3(j, k, ll),
                avg3(i, j, k),
                avg3(x, i, j),
                avg3(aa, x, i),
                avg3(b, aa, x),
                avg3(c, b, aa),
                avg3(d, c, b),
            ];
            for (r, row) in dst.iter_mut().enumerate() {
                for (col, p) in row.iter_mut().enumerate() {
                    *p = v[3 - r + col];
                }
            }
        }
        B_VR_PRED => {
            // libvpx's d117.
            dst[0] = [avg2(x, aa), avg2(aa, b), avg2(b, c), avg2(c, d)];
            dst[1] = [
                avg3(i, x, aa),
                avg3(x, aa, b),
                avg3(aa, b, c),
                avg3(b, c, d),
            ];
            dst[2] = [avg3(j, i, x), avg2(x, aa), avg2(aa, b), avg2(b, c)];
            dst[3] = [
                avg3(k, j, i),
                avg3(i, x, aa),
                avg3(x, aa, b),
                avg3(aa, b, c),
            ];
        }
        B_VL_PRED => {
            // libvpx's d63e.
            dst[0] = [avg2(aa, b), avg2(b, c), avg2(c, d), avg2(d, e)];
            dst[1] = [avg3(aa, b, c), avg3(b, c, d), avg3(c, d, e), avg3(d, e, f)];
            dst[2] = [avg2(b, c), avg2(c, d), avg2(d, e), avg3(e, f, g)];
            dst[3] = [avg3(b, c, d), avg3(c, d, e), avg3(d, e, f), avg3(f, g, h)];
        }
        B_HD_PRED => {
            // libvpx's d153.
            dst[0] = [avg2(i, x), avg3(i, x, aa), avg3(x, aa, b), avg3(aa, b, c)];
            dst[1] = [avg2(j, i), avg3(j, i, x), avg2(i, x), avg3(i, x, aa)];
            dst[2] = [avg2(k, j), avg3(k, j, i), avg2(j, i), avg3(j, i, x)];
            dst[3] = [avg2(ll, k), avg3(ll, k, j), avg2(k, j), avg3(k, j, i)];
        }
        // B_HU_PRED, and the values no stream can code, which libvpx's
        // function table would never be indexed by.
        _ => {
            // libvpx's d207.
            debug_assert_eq!(mode, B_HU_PRED);
            dst[0] = [avg2(i, j), avg3(i, j, k), avg2(j, k), avg3(j, k, ll)];
            dst[1] = [avg2(j, k), avg3(j, k, ll), avg2(k, ll), avg3(k, ll, ll)];
            dst[2] = [avg2(k, ll), avg3(k, ll, ll), ll, ll];
            dst[3] = [ll; 4];
        }
    }
    for (r, row) in dst.iter().enumerate() {
        let at = pos + r * stride;
        plane[at..at + 4].copy_from_slice(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x4 block at (1, 1) of a 9-wide plane, with `above` (above-left,
    /// then eight) and `left` around it.
    fn block(above: [u8; 9], left: [u8; 4]) -> (Vec<u8>, usize) {
        let stride = 9;
        let mut plane = vec![0u8; stride * 5];
        plane[..9].copy_from_slice(&above);
        for (r, &l) in left.iter().enumerate() {
            plane[(r + 1) * stride] = l;
        }
        (plane, stride + 1)
    }

    fn rows(plane: &[u8], pos: usize) -> [[u8; 4]; 4] {
        let mut out = [[0u8; 4]; 4];
        for (r, row) in out.iter_mut().enumerate() {
            row.copy_from_slice(&plane[pos + r * 9..pos + r * 9 + 4]);
        }
        out
    }

    #[test]
    fn the_4x4_predictors_match_libvpxs() {
        // Values from libvpx's own C predictors (vpx_dsp/intrapred.c), worked
        // by hand for this edge: X=10, A..H = 20..90 by tens, I..L = 5,15,25,35.
        let above = [10, 20, 30, 40, 50, 60, 70, 80, 90];
        let left = [5, 15, 25, 35];
        let check = |mode: u8, want: [[u8; 4]; 4]| {
            let (mut plane, pos) = block(above, left);
            predict_4x4(mode, &mut plane, pos, 9);
            assert_eq!(rows(&plane, pos), want, "mode {mode}");
        };
        // DC: (20+30+40+50 + 5+15+25+35 + 4) >> 3 = 28.
        check(B_DC_PRED, [[28; 4]; 4]);
        // TM: left + above - 10, clamped.
        check(
            B_TM_PRED,
            [
                [15, 25, 35, 45],
                [25, 35, 45, 55],
                [35, 45, 55, 65],
                [45, 55, 65, 75],
            ],
        );
        // VE: smoothed above, starting from X: avg3(10, 20, 30) is 20.
        check(B_VE_PRED, [[20, 30, 40, 50]; 4]);
        // HE: smoothed left, starting from X, ending L, L.
        check(B_HE_PRED, [[9; 4], [15; 4], [25; 4], [33; 4]]);
        // LD (d45e): the last is avg3(G, H, H).
        check(
            B_LD_PRED,
            [
                [30, 40, 50, 60],
                [40, 50, 60, 70],
                [50, 60, 70, 80],
                [60, 70, 80, 88],
            ],
        );
    }

    #[test]
    fn hu_fills_with_the_last_left_pixel() {
        let (mut plane, pos) = block([0; 9], [10, 20, 30, 40]);
        predict_4x4(B_HU_PRED, &mut plane, pos, 9);
        assert_eq!(
            rows(&plane, pos),
            [
                [15, 20, 25, 30],
                [25, 30, 35, 38],
                [35, 38, 40, 40],
                [40, 40, 40, 40]
            ]
        );
    }

    #[test]
    fn dc_prediction_averages_only_the_edges_there_are() {
        let stride = 20;
        let mut plane = vec![0u8; stride * 18];
        let pos = stride + 1;
        plane[1..17].fill(200); // above
        for r in 0..16 {
            plane[pos + r * stride - 1] = 100; // left
        }
        for (left, up, want) in [
            (false, false, 128),
            (false, true, 200),
            (true, false, 100),
            (true, true, 150),
        ] {
            let mut p = plane.clone();
            predict_mb(DC_PRED, 16, left, up, &mut p, pos, stride);
            assert!(
                (0..16).all(|r| p[pos + r * stride..pos + r * stride + 16]
                    .iter()
                    .all(|&v| v == want)),
                "left {left} up {up}"
            );
        }
    }

    #[test]
    fn tm_prediction_clamps() {
        let stride = 10;
        let mut plane = vec![0u8; stride * 10];
        let pos = stride + 1;
        plane[0] = 0; // above-left
        plane[1..9].fill(250);
        for r in 0..8 {
            plane[pos + r * stride - 1] = 20;
        }
        predict_mb(TM_PRED, 8, true, true, &mut plane, pos, stride);
        assert!(plane[pos..pos + 8].iter().all(|&v| v == 255));
    }

    #[test]
    fn the_down_copy_reaches_the_three_rows() {
        let stride = 40;
        let mut plane = vec![0u8; stride * 18];
        let pos = stride + 2;
        plane[pos - stride + 16..pos - stride + 20].copy_from_slice(&[1, 2, 3, 4]);
        down_copy_above_right(&mut plane, pos, stride);
        for row in [3, 7, 11] {
            assert_eq!(
                &plane[pos + row * stride + 16..pos + row * stride + 20],
                &[1, 2, 3, 4]
            );
        }
        assert_eq!(
            &plane[pos + 2 * stride + 16..pos + 2 * stride + 20],
            &[0; 4]
        );
    }
}

//! VP8 intra prediction (RFC 6386 §12): a block's first guess, extrapolated
//! from the reconstructed samples above it and to its left.
//!
//! A macroblock's luma is predicted either whole, as a 16x16 block by one of
//! four modes, or as sixteen 4x4 subblocks each by one of ten; its chroma is
//! predicted as two 8x8 blocks by one of the four whole-block modes. The
//! samples a prediction reads are those of the frame *before* loop filtering.
//!
//! Outside the picture the RFC defines the samples: 127 above the top row
//! (the corner above the first column included) and 129 left of the first
//! column. The caller lays those out around the block (see `lossy.rs`); the
//! one exception is whole-block DC prediction, which averages only the edges
//! that exist, and is told which do.

/// The 16x16 and 8x8 modes, in the RFC's numbering (§11.2).
pub(super) const DC_PRED: u8 = 0;
/// Each row a copy of the row above.
pub(super) const V_PRED: u8 = 1;
/// Each column a copy of the column to the left.
pub(super) const H_PRED: u8 = 2;
/// "True motion": above plus left minus the corner.
pub(super) const TM_PRED: u8 = 3;

/// The 4x4 subblock modes, in the RFC's numbering (§11.2), which is also the
/// order of the probability table's two context dimensions.
pub(super) const B_DC_PRED: u8 = 0;
/// True motion.
pub(super) const B_TM_PRED: u8 = 1;
/// Vertical, smoothed.
pub(super) const B_VE_PRED: u8 = 2;
/// Horizontal, smoothed.
pub(super) const B_HE_PRED: u8 = 3;
/// Down and to the left.
pub(super) const B_LD_PRED: u8 = 4;
/// Down and to the right.
pub(super) const B_RD_PRED: u8 = 5;
/// Vertical, leaning right.
pub(super) const B_VR_PRED: u8 = 6;
/// Vertical, leaning left.
pub(super) const B_VL_PRED: u8 = 7;
/// Horizontal, leaning down.
pub(super) const B_HD_PRED: u8 = 8;
/// Horizontal, leaning up.
pub(super) const B_HU_PRED: u8 = 9;

/// The subblock mode a whole-block luma mode stands for when a neighbouring
/// subblock's mode is the context for decoding another's (§11.3, item 4).
pub(super) const fn implied_subblock_mode(mode: u8) -> u8 {
    match mode {
        V_PRED => B_VE_PRED,
        H_PRED => B_HE_PRED,
        TM_PRED => B_TM_PRED,
        _ => B_DC_PRED,
    }
}

/// The thirteen samples a 4x4 subblock is predicted from, in the order the
/// RFC's `E` array puts the first nine: the left column bottom to top
/// (`L[3]`, `L[2]`, `L[1]`, `L[0]`), the corner `P`, then the eight samples
/// above and above-right (`A[0]` .. `A[7]`).
pub(super) type Edge = [u8; 13];

/// `(x + 2y + z + 2) / 4`.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "four samples' worth, below 1024, and the quotient is a sample"
)]
const fn avg3(x: u8, y: u8, z: u8) -> u8 {
    ((x as u16 + 2 * y as u16 + z as u16 + 2) >> 2) as u8
}

/// `(x + y + 1) / 2`.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "two samples' worth, and the quotient is a sample"
)]
const fn avg2(x: u8, y: u8) -> u8 {
    ((x as u16 + y as u16 + 1) >> 1) as u8
}

/// Predict a 4x4 subblock by `mode` from `e`, row by row (RFC 6386 §12.3,
/// `subblock_intra_predict`, whose `B[r][c]` is `out[4 * r + c]` here).
#[allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "the names are the RFC's; sums are of at most eight samples, below 2048"
)]
pub(super) fn subblock(mode: u8, e: &Edge) -> [u8; 16] {
    // The RFC's views of the edge: E (e[0..9]), A with A[-1] = P, and L with
    // L[-1] = P.
    let [l3, l2, l1, l0, p, a0, a1, a2, a3, a4, a5, a6, a7] = *e;
    let avg3p = |k: usize| -> u8 {
        let at = |i: usize| e.get(i).copied().unwrap_or(0);
        avg3(at(k.wrapping_sub(1)), at(k), at(k.wrapping_add(1)))
    };
    let avg2p = |k: usize| -> u8 {
        let at = |i: usize| e.get(i).copied().unwrap_or(0);
        avg2(at(k), at(k.wrapping_add(1)))
    };
    match mode {
        B_TM_PRED => {
            let mut out = [0u8; 16];
            for (r, left) in [l0, l1, l2, l3].into_iter().enumerate() {
                for (c, above) in [a0, a1, a2, a3].into_iter().enumerate() {
                    let value = i32::from(left) + i32::from(above) - i32::from(p);
                    if let Some(slot) = out.get_mut(4 * r + c) {
                        *slot = super::transform::clamp(value);
                    }
                }
            }
            out
        }
        B_VE_PRED => {
            let row = [
                avg3(p, a0, a1),
                avg3(a0, a1, a2),
                avg3(a1, a2, a3),
                avg3(a2, a3, a4),
            ];
            let mut out = [0u8; 16];
            for chunk in out.chunks_exact_mut(4) {
                chunk.copy_from_slice(&row);
            }
            out
        }
        B_HE_PRED => {
            let rows = [
                avg3(p, l0, l1),
                avg3(l0, l1, l2),
                avg3(l1, l2, l3),
                avg3(l2, l3, l3),
            ];
            let mut out = [0u8; 16];
            for (chunk, value) in out.chunks_exact_mut(4).zip(rows) {
                chunk.fill(value);
            }
            out
        }
        B_LD_PRED => {
            // Down-left diagonals: B[r][c] depends on r + c alone.
            let diagonal = [
                avg3(a0, a1, a2),
                avg3(a1, a2, a3),
                avg3(a2, a3, a4),
                avg3(a3, a4, a5),
                avg3(a4, a5, a6),
                avg3(a5, a6, a7),
                avg3(a6, a7, a7),
            ];
            let mut out = [0u8; 16];
            for (i, slot) in out.iter_mut().enumerate() {
                *slot = diagonal.get(i / 4 + i % 4).copied().unwrap_or(0);
            }
            out
        }
        B_RD_PRED => {
            // Down-right diagonals: B[r][c] depends on c - r alone, and the
            // diagonal through the corner is avg3p(E + 4).
            let mut out = [0u8; 16];
            for (i, slot) in out.iter_mut().enumerate() {
                let (r, c) = (i / 4, i % 4);
                *slot = avg3p(4 + c - r);
            }
            out
        }
        B_VR_PRED => [
            avg2p(4),
            avg2p(5),
            avg2p(6),
            avg2p(7),
            avg3p(4),
            avg3p(5),
            avg3p(6),
            avg3p(7),
            avg3p(3),
            avg2p(4),
            avg2p(5),
            avg2p(6),
            avg3p(2),
            avg3p(4),
            avg3p(5),
            avg3p(6),
        ],
        B_VL_PRED => [
            avg2(a0, a1),
            avg2(a1, a2),
            avg2(a2, a3),
            avg2(a3, a4),
            avg3(a0, a1, a2),
            avg3(a1, a2, a3),
            avg3(a2, a3, a4),
            avg3(a3, a4, a5),
            avg2(a1, a2),
            avg2(a2, a3),
            avg2(a3, a4),
            avg3(a4, a5, a6),
            avg3(a1, a2, a3),
            avg3(a2, a3, a4),
            avg3(a3, a4, a5),
            avg3(a5, a6, a7),
        ],
        B_HD_PRED => [
            avg2p(3),
            avg3p(4),
            avg3p(5),
            avg3p(6),
            avg2p(2),
            avg3p(3),
            avg2p(3),
            avg3p(4),
            avg2p(1),
            avg3p(2),
            avg2p(2),
            avg3p(3),
            avg2p(0),
            avg3p(1),
            avg2p(1),
            avg3p(2),
        ],
        B_HU_PRED => [
            avg2(l0, l1),
            avg3(l0, l1, l2),
            avg2(l1, l2),
            avg3(l1, l2, l3),
            avg2(l1, l2),
            avg3(l1, l2, l3),
            avg2(l2, l3),
            avg3(l2, l3, l3),
            avg2(l2, l3),
            avg3(l2, l3, l3),
            l3,
            l3,
            l3,
            l3,
            l3,
            l3,
        ],
        // B_DC_PRED, and -- unreachable, the mode tree has ten leaves -- any
        // other value.
        _ => {
            let sum: u32 = [a0, a1, a2, a3, l0, l1, l2, l3]
                .iter()
                .map(|&s| u32::from(s))
                .sum();
            [((sum + 4) >> 3) as u8; 16]
        }
    }
}

/// Which of a whole block's edges exist, for DC prediction: the RFC averages
/// only the samples that are really there (§12.2), and a block with neither
/// predicts mid-grey.
#[derive(Clone, Copy)]
pub(super) struct Edges {
    /// The block is not in the top row of macroblocks.
    pub above: bool,
    /// The block is not in the left column of macroblocks.
    pub left: bool,
}

/// Predict an `n`x`n` block (16 for luma, 8 for chroma) by one of the four
/// whole-block modes, in place: `buf` holds the block at rows 1..=n and
/// columns 1..=n of rows `stride` apart, with the row above it in row 0, the
/// column to its left in column 0, and the corner at index 0.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "callers pass buffers of (n + 1) rows of stride > n samples, so every index is below (n + 1) * stride; sums are of at most 32 samples"
)]
pub(super) fn whole_block(mode: u8, buf: &mut [u8], stride: usize, n: usize, edges: Edges) {
    debug_assert!(buf.len() >= (n + 1) * stride && stride > n);
    match mode {
        V_PRED => {
            for r in 1..=n {
                buf.copy_within(1..=n, r * stride + 1);
            }
        }
        H_PRED => {
            for r in 1..=n {
                let left = buf[r * stride];
                buf[r * stride + 1..=r * stride + n].fill(left);
            }
        }
        TM_PRED => {
            let corner = i32::from(buf[0]);
            for r in 1..=n {
                let left = i32::from(buf[r * stride]) - corner;
                for c in 1..=n {
                    buf[r * stride + c] = super::transform::clamp(left + i32::from(buf[c]));
                }
            }
        }
        _ => {
            let shift = n.trailing_zeros();
            let above: u32 = (1..=n).map(|c| u32::from(buf[c])).sum();
            let left: u32 = (1..=n).map(|r| u32::from(buf[r * stride])).sum();
            let value = match (edges.above, edges.left) {
                (true, true) => (above + left + (1 << shift)) >> (shift + 1),
                (true, false) => (above + (1 << (shift - 1))) >> shift,
                (false, true) => (left + (1 << (shift - 1))) >> shift,
                (false, false) => 128,
            } as u8;
            for r in 1..=n {
                buf[r * stride + 1..=r * stride + n].fill(value);
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;

    /// libwebp's 4x4 predictors (`src/dsp/dec.c`), transcribed in its own
    /// terms -- `DST(x, y)` is column x of row y; I, J, K, L the left column;
    /// X the corner; A..H the row above -- as an independent check on the
    /// RFC-shaped code, which lays the same arithmetic out differently.
    fn libwebp(mode: u8, e: &Edge) -> [u8; 16] {
        let [l, k, j, i, x, a, b, c, d, ee, f, g, h] = *e;
        let mut out = [0u8; 16];
        let mut dst = |xx: usize, yy: usize, v: u8| out[4 * yy + xx] = v;
        match mode {
            B_VE_PRED => {
                let vals = [avg3(x, a, b), avg3(a, b, c), avg3(b, c, d), avg3(c, d, ee)];
                for yy in 0..4 {
                    for xx in 0..4 {
                        dst(xx, yy, vals[xx]);
                    }
                }
            }
            B_HE_PRED => {
                let vals = [avg3(x, i, j), avg3(i, j, k), avg3(j, k, l), avg3(k, l, l)];
                for yy in 0..4 {
                    for xx in 0..4 {
                        dst(xx, yy, vals[yy]);
                    }
                }
            }
            B_DC_PRED => {
                let sum = [a, b, c, d, i, j, k, l]
                    .iter()
                    .map(|&v| u32::from(v))
                    .sum::<u32>()
                    + 4;
                for yy in 0..4 {
                    for xx in 0..4 {
                        dst(xx, yy, (sum >> 3) as u8);
                    }
                }
            }
            B_TM_PRED => {
                let top = [a, b, c, d];
                let left = [i, j, k, l];
                for yy in 0..4 {
                    for xx in 0..4 {
                        let v = i32::from(left[yy]) + i32::from(top[xx]) - i32::from(x);
                        dst(xx, yy, v.clamp(0, 255) as u8);
                    }
                }
            }
            B_RD_PRED => {
                dst(0, 3, avg3(j, k, l));
                let v = avg3(i, j, k);
                dst(1, 3, v);
                dst(0, 2, v);
                let v = avg3(x, i, j);
                dst(2, 3, v);
                dst(1, 2, v);
                dst(0, 1, v);
                let v = avg3(a, x, i);
                dst(3, 3, v);
                dst(2, 2, v);
                dst(1, 1, v);
                dst(0, 0, v);
                let v = avg3(b, a, x);
                dst(3, 2, v);
                dst(2, 1, v);
                dst(1, 0, v);
                let v = avg3(c, b, a);
                dst(3, 1, v);
                dst(2, 0, v);
                dst(3, 0, avg3(d, c, b));
            }
            B_LD_PRED => {
                dst(0, 0, avg3(a, b, c));
                let v = avg3(b, c, d);
                dst(1, 0, v);
                dst(0, 1, v);
                let v = avg3(c, d, ee);
                dst(2, 0, v);
                dst(1, 1, v);
                dst(0, 2, v);
                let v = avg3(d, ee, f);
                dst(3, 0, v);
                dst(2, 1, v);
                dst(1, 2, v);
                dst(0, 3, v);
                let v = avg3(ee, f, g);
                dst(3, 1, v);
                dst(2, 2, v);
                dst(1, 3, v);
                let v = avg3(f, g, h);
                dst(3, 2, v);
                dst(2, 3, v);
                dst(3, 3, avg3(g, h, h));
            }
            B_VR_PRED => {
                let v = avg2(x, a);
                dst(0, 0, v);
                dst(1, 2, v);
                let v = avg2(a, b);
                dst(1, 0, v);
                dst(2, 2, v);
                let v = avg2(b, c);
                dst(2, 0, v);
                dst(3, 2, v);
                dst(3, 0, avg2(c, d));
                dst(0, 3, avg3(k, j, i));
                dst(0, 2, avg3(j, i, x));
                let v = avg3(i, x, a);
                dst(0, 1, v);
                dst(1, 3, v);
                let v = avg3(x, a, b);
                dst(1, 1, v);
                dst(2, 3, v);
                let v = avg3(a, b, c);
                dst(2, 1, v);
                dst(3, 3, v);
                dst(3, 1, avg3(b, c, d));
            }
            B_VL_PRED => {
                dst(0, 0, avg2(a, b));
                let v = avg2(b, c);
                dst(1, 0, v);
                dst(0, 2, v);
                let v = avg2(c, d);
                dst(2, 0, v);
                dst(1, 2, v);
                let v = avg2(d, ee);
                dst(3, 0, v);
                dst(2, 2, v);
                dst(0, 1, avg3(a, b, c));
                let v = avg3(b, c, d);
                dst(1, 1, v);
                dst(0, 3, v);
                let v = avg3(c, d, ee);
                dst(2, 1, v);
                dst(1, 3, v);
                let v = avg3(d, ee, f);
                dst(3, 1, v);
                dst(2, 3, v);
                dst(3, 2, avg3(ee, f, g));
                dst(3, 3, avg3(f, g, h));
            }
            B_HU_PRED => {
                dst(0, 0, avg2(i, j));
                let v = avg2(j, k);
                dst(2, 0, v);
                dst(0, 1, v);
                let v = avg2(k, l);
                dst(2, 1, v);
                dst(0, 2, v);
                dst(1, 0, avg3(i, j, k));
                let v = avg3(j, k, l);
                dst(3, 0, v);
                dst(1, 1, v);
                let v = avg3(k, l, l);
                dst(3, 1, v);
                dst(1, 2, v);
                for (xx, yy) in [(3, 2), (2, 2), (0, 3), (1, 3), (2, 3), (3, 3)] {
                    dst(xx, yy, l);
                }
            }
            B_HD_PRED => {
                let v = avg2(i, x);
                dst(0, 0, v);
                dst(2, 1, v);
                let v = avg2(j, i);
                dst(0, 1, v);
                dst(2, 2, v);
                let v = avg2(k, j);
                dst(0, 2, v);
                dst(2, 3, v);
                dst(0, 3, avg2(l, k));
                dst(3, 0, avg3(a, b, c));
                dst(2, 0, avg3(x, a, b));
                let v = avg3(i, x, a);
                dst(1, 0, v);
                dst(3, 1, v);
                let v = avg3(j, i, x);
                dst(1, 1, v);
                dst(3, 2, v);
                let v = avg3(k, j, i);
                dst(1, 2, v);
                dst(3, 3, v);
                dst(1, 3, avg3(l, k, j));
            }
            _ => unreachable!(),
        }
        out
    }

    #[test]
    fn every_subblock_mode_is_libwebps() {
        let mut state = 99u32;
        for trial in 0..2000 {
            let mut e = [0u8; 13];
            for s in &mut e {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                // Some trials at the extremes, where clamping and rounding bite.
                *s = match trial % 4 {
                    0 => {
                        if state & 0x10000 != 0 {
                            255
                        } else {
                            0
                        }
                    }
                    _ => (state >> 16) as u8,
                };
            }
            for mode in 0..10 {
                assert_eq!(
                    subblock(mode, &e),
                    libwebp(mode, &e),
                    "mode {mode}, edge {e:?}"
                );
            }
        }
    }

    #[test]
    fn whole_block_dc_averages_only_the_edges_that_exist() {
        // A 4x4 block would do, but the modes are only used at 16 and 8.
        let stride = 17;
        let mut buf = [0u8; 17 * 17];
        buf[1..=16].fill(10); // above
        for r in 1..=16 {
            buf[r * stride] = 30; // left
        }
        let both = Edges {
            above: true,
            left: true,
        };
        whole_block(DC_PRED, &mut buf, stride, 16, both);
        assert_eq!(buf[stride + 1], 20);
        whole_block(
            DC_PRED,
            &mut buf,
            stride,
            16,
            Edges {
                above: true,
                left: false,
            },
        );
        assert_eq!(buf[16 * stride + 16], 10);
        whole_block(
            DC_PRED,
            &mut buf,
            stride,
            16,
            Edges {
                above: false,
                left: true,
            },
        );
        assert_eq!(buf[5 * stride + 3], 30);
        whole_block(
            DC_PRED,
            &mut buf,
            stride,
            16,
            Edges {
                above: false,
                left: false,
            },
        );
        assert_eq!(buf[2 * stride + 2], 128);
    }

    #[test]
    fn whole_block_modes_extrapolate_as_the_rfc_says() {
        let stride = 9;
        let mut buf = [0u8; 9 * 9];
        buf[0] = 100; // corner
        for c in 1..=8 {
            buf[c] = (c * 20) as u8; // above
        }
        for r in 1..=8 {
            buf[r * stride] = (r * 25) as u8; // left
        }
        let edges = Edges {
            above: true,
            left: true,
        };
        let mut v = buf;
        whole_block(V_PRED, &mut v, stride, 8, edges);
        assert_eq!(v[8 * stride + 3], 60);
        let mut h = buf;
        whole_block(H_PRED, &mut h, stride, 8, edges);
        assert_eq!(h[3 * stride + 8], 75);
        let mut tm = buf;
        whole_block(TM_PRED, &mut tm, stride, 8, edges);
        // left 200 + above 160 - corner 100, clamped.
        assert_eq!(tm[8 * stride + 8], 255);
        // left 25 + above 20 - corner 100: below zero, clamped.
        assert_eq!(tm[stride + 1], 0);
        assert_eq!(tm[4 * stride + 2], 40);
    }
}

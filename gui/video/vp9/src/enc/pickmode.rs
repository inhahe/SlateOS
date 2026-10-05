//! The realtime mode search: libvpx's non-RD ("nonrd") mode pick, which
//! estimates each candidate's rate and distortion cheaply instead of coding
//! it.
//!
//! For an intra block on a key frame it tries DC, vertical and horizontal
//! prediction. Each is predicted into the reconstruction, as libvpx does,
//! and its residual measured with a Hadamard transform (or the 4x4 DCT) and
//! the fast quantiser: the rate is the sum of the levels' magnitudes, the
//! distortion the quantisation error. The mode with the least
//! rate-distortion cost, mode and skip flag costs included, wins.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_pickmode.c`
//! (`vp9_pick_intra_mode`, `estimate_block_intra`, `block_yrd`),
//! `vpx_dsp/avg.c` (`vpx_hadamard_8x8_c`, `vpx_hadamard_16x16_c`,
//! `vpx_satd_c`) and `vp9/encoder/vp9_rdopt.c` (`vp9_block_error_fp_c`)
//! (copyright the WebM project authors), used under libvpx's BSD licence and
//! patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "residuals of 8-bit samples through 16-bit Hadamard stages (libvpx's int16_t, wrapping as it does), levels summed over at most 256 coefficients, costs of a few thousand units"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "the Hadamard works on fixed 8x8 and 16x16 arrays at constant offsets"
)]

use crate::common::{
    BLOCK_8X8, BlockSize, DC_PRED, H_PRED, INTRA_MODES, PredictionMode, TX_4X4, TX_8X8, TX_16X16,
    TxSize,
};
use crate::context;
use crate::enc::cost::{PROB_COST_SHIFT, cost_bit};
use crate::enc::encodeframe::{BlockModes, FrameEncoder, Placement};
use crate::enc::fdct;
use crate::enc::quantize::{QuantSet, iscan_for, quantize_fp};
use crate::enc::rd::{RDDIV_BITS, rdcost};
use crate::tables;

/// One column of libvpx's 8-point Hadamard, in its `int16_t` arithmetic.
fn hadamard_col8(src: &[i16], stride: usize, out: &mut [i16; 8]) {
    let s = |i: usize| src.get(i * stride).copied().unwrap_or(0);
    let b0 = s(0).wrapping_add(s(1));
    let b1 = s(0).wrapping_sub(s(1));
    let b2 = s(2).wrapping_add(s(3));
    let b3 = s(2).wrapping_sub(s(3));
    let b4 = s(4).wrapping_add(s(5));
    let b5 = s(4).wrapping_sub(s(5));
    let b6 = s(6).wrapping_add(s(7));
    let b7 = s(6).wrapping_sub(s(7));
    let c0 = b0.wrapping_add(b2);
    let c1 = b1.wrapping_add(b3);
    let c2 = b0.wrapping_sub(b2);
    let c3 = b1.wrapping_sub(b3);
    let c4 = b4.wrapping_add(b6);
    let c5 = b5.wrapping_add(b7);
    let c6 = b4.wrapping_sub(b6);
    let c7 = b5.wrapping_sub(b7);
    out[0] = c0.wrapping_add(c4);
    out[7] = c1.wrapping_add(c5);
    out[3] = c2.wrapping_add(c6);
    out[4] = c3.wrapping_add(c7);
    out[2] = c0.wrapping_sub(c4);
    out[6] = c1.wrapping_sub(c5);
    out[1] = c2.wrapping_sub(c6);
    out[5] = c3.wrapping_sub(c7);
}

/// libvpx's `vpx_hadamard_8x8_c`: the residual at `src` (rows `stride`
/// apart) into 64 coefficients, in libvpx's order.
fn hadamard_8x8(src: &[i16], stride: usize, coeff: &mut [i32]) {
    let mut buffer = [0i16; 64];
    for idx in 0..8 {
        let mut col = [0i16; 8];
        hadamard_col8(src.get(idx..).unwrap_or(&[]), stride, &mut col);
        buffer[idx * 8..idx * 8 + 8].copy_from_slice(&col);
    }
    let mut buffer2 = [0i16; 64];
    for idx in 0..8 {
        let mut col = [0i16; 8];
        hadamard_col8(&buffer[idx..], 8, &mut col);
        buffer2[idx * 8..idx * 8 + 8].copy_from_slice(&col);
    }
    for (c, &b) in coeff.iter_mut().zip(&buffer2) {
        *c = i32::from(b);
    }
}

/// libvpx's `vpx_hadamard_16x16_c`: four 8x8 transforms, then a butterfly
/// across them.
fn hadamard_16x16(src: &[i16], stride: usize, coeff: &mut [i32]) {
    if coeff.len() < 256 {
        return;
    }
    for idx in 0..4 {
        let offset = (idx >> 1) * 8 * stride + (idx & 1) * 8;
        hadamard_8x8(
            src.get(offset..).unwrap_or(&[]),
            stride,
            &mut coeff[idx * 64..idx * 64 + 64],
        );
    }
    for idx in 0..64 {
        let (a0, a1, a2, a3) = (
            coeff[idx],
            coeff[idx + 64],
            coeff[idx + 128],
            coeff[idx + 192],
        );
        let b0 = (a0 + a1) >> 1;
        let b1 = (a0 - a1) >> 1;
        let b2 = (a2 + a3) >> 1;
        let b3 = (a2 - a3) >> 1;
        coeff[idx] = b0 + b2;
        coeff[idx + 64] = b1 + b3;
        coeff[idx + 128] = b0 - b2;
        coeff[idx + 192] = b1 - b3;
    }
}

/// libvpx's `vpx_satd_c`: the sum of the levels' magnitudes.
fn satd(coeff: &[i32]) -> i32 {
    coeff
        .iter()
        .fold(0i32, |s, &c| s.wrapping_add(c.wrapping_abs()))
}

/// libvpx's `vp9_block_error_fp_c`: the squared quantisation error, each
/// square in its 32-bit `int`.
fn block_error_fp(coeff: &[i32], dqcoeff: &[i32]) -> i64 {
    coeff
        .iter()
        .zip(dqcoeff)
        .map(|(&c, &d)| {
            let diff = c.wrapping_sub(d);
            i64::from(diff.wrapping_mul(diff))
        })
        .sum()
}

/// A block's estimated luma rate and distortion from its transform: libvpx's
/// `block_yrd` past its simple-model shortcut, the residual against `pred`
/// Hadamard-transformed (or 4x4 DCT'd) per transform block and fast
/// quantised; the rate the levels' magnitudes, the distortion the
/// quantisation error. `place` is the whole block's, whose edge distances
/// libvpx applies here too. With `sse` (the block's sum of squared
/// differences, as the search measured it): scaled to the transform's units,
/// and the distortion of a block that quantises to nothing, at no rate.
/// Returns the rate, the distortion and whether every transform block
/// quantised to nothing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn block_yrd_full(
    src: &[u8],
    src_stride: usize,
    pred: &[u8],
    pred_stride: usize,
    bsize: BlockSize,
    tx_size: TxSize,
    place: &Placement,
    qs: &QuantSet,
    sse: Option<&mut i64>,
) -> (i32, i64, bool) {
    let (rate, dist, skippable) = block_yrd(
        src,
        src_stride,
        pred,
        pred_stride,
        bsize,
        tx_size,
        place,
        qs,
    );
    if let Some(sse) = sse {
        *sse = (*sse << 6) >> 2;
        if skippable {
            return (0, *sse, true);
        }
    }
    (rate, dist, skippable)
}

/// A transform block's estimated rate and distortion: libvpx's `block_yrd`
/// for one intra transform block (`bsize` its own size), on a key frame.
/// `place` is the whole block's, whose edge distances libvpx applies here
/// too.
#[allow(clippy::too_many_arguments)]
fn block_yrd(
    src: &[u8],
    src_stride: usize,
    pred: &[u8],
    pred_stride: usize,
    bsize: BlockSize,
    tx_size: TxSize,
    place: &Placement,
    qs: &QuantSet,
) -> (i32, i64, bool) {
    let n4_w = i32::from(tables::NUM_4X4_WIDE[usize::from(bsize)]);
    let n4_h = i32::from(tables::NUM_4X4_HIGH[usize::from(bsize)]);
    let step = 1usize << (tx_size << 1);
    let block_step = 1i32 << tx_size;
    let max_wide = n4_w
        + if place.mb_to_right_edge >= 0 {
            0
        } else {
            place.mb_to_right_edge >> 5
        };
    let max_high = n4_h
        + if place.mb_to_bottom_edge >= 0 {
            0
        } else {
            place.mb_to_bottom_edge >> 5
        };
    let (bw, bh) = (4 * n4_w as usize, 4 * n4_h as usize);
    // vpx_subtract_block over the whole transform block.
    let mut diff = vec![0i16; bw * bh];
    for r in 0..bh {
        let (s, p) = (
            src.get(r * src_stride..).unwrap_or(&[]),
            pred.get(r * pred_stride..).unwrap_or(&[]),
        );
        for ((d, &a), &b) in diff[r * bw..(r + 1) * bw].iter_mut().zip(s).zip(p) {
            *d = i16::from(a) - i16::from(b);
        }
    }
    let n = step << 4;
    let iscan = iscan_for(tx_size, crate::common::DCT_DCT);
    let mut skippable = true;
    let mut eob_cost = 0;
    let mut rate = 0i32;
    let mut dist = 0i64;
    let mut coeff = vec![0i32; n];
    let mut qcoeff = vec![0i32; n];
    let mut dqcoeff = vec![0i32; n];
    let mut r = 0;
    while r < max_high {
        let mut c = 0;
        while c < n4_w {
            if c < max_wide {
                let off = ((r as usize) * bw + c as usize) << 2;
                let d = &diff[off..];
                match tx_size {
                    TX_16X16 => hadamard_16x16(d, bw, &mut coeff),
                    TX_8X8 => hadamard_8x8(d, bw, &mut coeff),
                    _ => {
                        let mut out = [0i32; 16];
                        fdct::fdct4x4(d, bw, &mut out);
                        coeff[..16].copy_from_slice(&out);
                    }
                }
                let eob = quantize_fp(&coeff, n, qs, &mut qcoeff, &mut dqcoeff, iscan, false);
                skippable &= eob == 0;
                eob_cost += 1;
                if eob == 1 {
                    rate += qcoeff[0].wrapping_abs();
                } else if eob > 1 {
                    rate += satd(&qcoeff[..n]);
                }
                dist += block_error_fp(&coeff[..n], &dqcoeff[..n]) >> 2;
            }
            c += block_step;
        }
        r += block_step;
    }
    rate <<= 2 + PROB_COST_SHIFT;
    rate += eob_cost << PROB_COST_SHIFT;
    (rate, dist, skippable)
}

impl FrameEncoder<'_> {
    /// Choose a key frame block's luma mode: libvpx's `vp9_pick_intra_mode`.
    /// Chroma is always DC; the transform is the largest the frame allows.
    /// The candidates are predicted into the reconstruction, which the
    /// block's coding then predicts over.
    pub(crate) fn pick_intra_mode(
        &mut self,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
        rdmult: i32,
        y_mode_costs: &[[[i32; INTRA_MODES]; INTRA_MODES]; INTRA_MODES],
    ) -> BlockModes {
        debug_assert!(bsize >= BLOCK_8X8);
        let biggest = tables::TX_MODE_TO_BIGGEST_TX_SIZE
            .get(usize::from(self.h.tx_mode))
            .copied()
            .unwrap_or(TX_4X4);
        let max_tx = tables::MAX_TXSIZE
            .get(usize::from(bsize))
            .copied()
            .unwrap_or(TX_4X4);
        let intra_tx_size = max_tx.min(biggest);
        let place = self.placement(mi_row, mi_col, bsize);
        let (above, left) = self.neighbours(mi_row, mi_col);
        let a = above.and_then(|i| self.mi.blocks.get(i)).copied();
        let l = left.and_then(|i| self.mi.blocks.get(i)).copied();
        let cur = crate::block::ModeInfo {
            sb_type: bsize,
            ..crate::block::ModeInfo::default()
        };
        let am = usize::from(context::above_block_mode(&cur, a.as_ref(), 0));
        let lm = usize::from(context::left_block_mode(&cur, l.as_ref(), 0));
        let costs = y_mode_costs[am.min(9)][lm.min(9)];
        let skip_prob = self
            .fc
            .skip_probs
            .get(context::skip_context(a.as_ref(), l.as_ref()))
            .copied()
            .unwrap_or(128);
        let qindex = self.seg.qindex(0, self.h.quant.base_qindex);
        let qs = self.quants.get(0, usize::try_from(qindex).unwrap_or(0));
        let mut best: Option<(i64, PredictionMode)> = None;
        for mode in DC_PRED..=H_PRED {
            let mut rate = 0i32;
            let mut dist = 0i64;
            let mut skippable = true;
            let bsize_tx = tables::TXSIZE_TO_BSIZE[usize::from(intra_tx_size)];
            for (_, row, col) in self.transform_blocks(&place, bsize, 0, intra_tx_size) {
                let (x0, y0) = self.predict_intra(&place, bsize, 0, row, col, intra_tx_size, mode);
                let s = &self.src.planes[0];
                let (pred, pred_stride) = self.recon_at(0, x0, y0);
                // libvpx's block_yrd resets the skip flag each call, so the
                // block is skippable if its last transform block is.
                let (r, d, sk) = block_yrd(
                    s.data.get(y0 * s.stride + x0..).unwrap_or(&[]),
                    s.stride,
                    pred,
                    pred_stride,
                    bsize_tx,
                    intra_tx_size.min(TX_16X16),
                    &place,
                    &qs,
                );
                rate += r;
                dist += d;
                skippable = sk;
            }
            if skippable {
                rate = cost_bit(skip_prob, true) as i32;
            } else {
                rate += cost_bit(skip_prob, false) as i32;
            }
            rate += costs[usize::from(mode)];
            let cost = rdcost(rdmult, RDDIV_BITS, rate, dist);
            if best.is_none_or(|(b, _)| cost < b) {
                best = Some((cost, mode));
            }
        }
        BlockModes {
            mode: best.map_or(DC_PRED, |(_, m)| m),
            sub_modes: [DC_PRED; 4],
            uv_mode: DC_PRED,
            tx_size: intra_tx_size,
            ..BlockModes::default()
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]

    use super::*;

    /// The Hadamard of a constant block puts everything in the first
    /// coefficient: 64 times the value at 8x8, and the 16x16's butterfly
    /// halves twice over four blocks of 64.
    #[test]
    fn a_flat_block_has_only_a_dc_coefficient() {
        let src = [3i16; 16 * 16];
        let mut c8 = [0i32; 64];
        hadamard_8x8(&src, 16, &mut c8);
        assert_eq!(c8[0], 3 * 64);
        assert!(c8[1..].iter().all(|&c| c == 0));
        let mut c16 = [0i32; 256];
        hadamard_16x16(&src, 16, &mut c16);
        assert_eq!(c16[0], 3 * 64 * 2);
        assert!(c16[1..].iter().all(|&c| c == 0));
    }

    #[test]
    fn satd_and_error_are_sums() {
        assert_eq!(satd(&[1, -2, 3]), 6);
        assert_eq!(block_error_fp(&[10, -10], &[8, -7]), 4 + 9);
    }
}

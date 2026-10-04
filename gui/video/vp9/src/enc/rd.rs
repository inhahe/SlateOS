//! Rate-distortion constants: how the encoder weighs bits against error.
//!
//! A choice's cost is its rate (in 1/512 bits) times a multiplier, plus its
//! distortion scaled up by 2^`RDDIV_BITS`; the multiplier grows with the
//! square of the quantiser step, so a coarse frame cares more about bits
//! and a fine one about error. The tables of what each mode costs to code
//! come from the probabilities the frame codes with.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_rd.c`,
//! `vp9_rd.h` and `vp9_cost.c` (copyright the WebM project authors), used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "rates are sums of a few 13-bit costs, distortions sums of squared 8-bit residuals, the multiplier a dc step squared (at most 1336^2) times about 4.6; libvpx truncates the double product to int as this does"
)]

use crate::boolread::Tree;
use crate::common::{INTRA_MODE_TREE, INTRA_MODES};
use crate::enc::cost::{PROB_COST_SHIFT, cost_bit};
use crate::header::dc_quant;
use crate::tables;

/// Distortion is weighed at 2^7: libvpx's `RDDIV_BITS`.
pub(crate) const RDDIV_BITS: u32 = 7;

/// A choice's rate-distortion cost: libvpx's `RDCOST`.
pub(crate) fn rdcost(rdmult: i32, rddiv: u32, rate: i32, dist: i64) -> i64 {
    ((i64::from(rate) * i64::from(rdmult) + (1 << (PROB_COST_SHIFT - 1))) >> PROB_COST_SHIFT)
        + (dist << rddiv)
}

/// What kind of frame a multiplier is for: libvpx distinguishes key frames,
/// golden or alt-ref updates, and the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RdFrame {
    Key,
    #[expect(
        dead_code,
        reason = "inter frames' multipliers, used when the encoder codes inter frames"
    )]
    GoldenOrAltRef,
    #[expect(
        dead_code,
        reason = "inter frames' multipliers, used when the encoder codes inter frames"
    )]
    Inter,
}

/// The rate-distortion multiplier at `qindex`: libvpx's
/// `vp9_compute_rd_mult` for one pass, 8-bit, no external rate control.
pub(crate) fn compute_rd_mult(qindex: i32, frame: RdFrame) -> i32 {
    let q = i32::from(dc_quant(qindex, 0, 8));
    let rdmult = q * q;
    // libvpx's def_kf/arf/inter_rd_multiplier, times the Vizier factors of
    // 1.0 a one-pass encode leaves them at.
    let base = match frame {
        RdFrame::Key => 4.35,
        RdFrame::GoldenOrAltRef => 4.25,
        RdFrame::Inter => 4.15,
    };
    let def_rd_q_mult = base + 0.001 * f64::from(qindex);
    let rdmult = (f64::from(rdmult) * def_rd_q_mult * 1.0) as i32;
    rdmult.max(1)
}

/// Every symbol's cost in a tree: libvpx's `vp9_cost_tokens`.
pub(crate) fn cost_tokens(costs: &mut [i32], probs: &[u8], tree: &Tree) {
    fn walk(costs: &mut [i32], tree: &Tree, probs: &[u8], i: usize, c: i32) {
        let prob = probs.get(i / 2).copied().unwrap_or(128);
        for b in 0..2 {
            let cc = c + cost_bit(prob, b == 1) as i32;
            let ii = tree.get(i + b).copied().unwrap_or(0);
            if ii <= 0 {
                if let Some(slot) = costs.get_mut(usize::from(ii.unsigned_abs())) {
                    *slot = cc;
                }
            } else {
                walk(costs, tree, probs, ii as usize, cc);
            }
        }
    }
    walk(costs, tree, probs, 0, 0);
}

/// The key-frame luma mode costs, by the above and left blocks' modes:
/// libvpx's `y_mode_costs`, from `vp9_kf_y_mode_prob` (`fill_mode_costs`).
pub(crate) fn kf_y_mode_costs() -> Box<[[[i32; INTRA_MODES]; INTRA_MODES]; INTRA_MODES]> {
    let mut costs = Box::new([[[0i32; INTRA_MODES]; INTRA_MODES]; INTRA_MODES]);
    for (a, row) in costs.iter_mut().enumerate() {
        for (l, c) in row.iter_mut().enumerate() {
            let probs = tables::KF_Y_MODE_PROB
                .get(a)
                .and_then(|p| p.get(l))
                .copied()
                .unwrap_or([128; 9]);
            cost_tokens(c, &probs, &INTRA_MODE_TREE);
        }
    }
    costs
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, reason = "a test: a failure should be loud")]

    use super::*;
    use crate::enc::cost::cost_zero;

    #[test]
    fn rdcost_rounds_the_rate_and_scales_the_distortion() {
        // (512 * 1000 + 256) >> 9 = 1000, plus 3 << 7.
        assert_eq!(rdcost(1000, RDDIV_BITS, 512, 3), 1000 + 384);
        assert_eq!(rdcost(1, RDDIV_BITS, 255, 0), 0);
        assert_eq!(rdcost(1, RDDIV_BITS, 256, 0), 1);
    }

    #[test]
    fn the_key_frame_multiplier_is_libvpxs() {
        // q index 161: dc step 247 (libvpx's dc_qlookup), 247^2 = 61009,
        // times 4.35 + 0.161 = 4.511, truncated.
        let dc = i32::from(dc_quant(161, 0, 8));
        let want = (f64::from(dc * dc) * (4.35 + 0.161)) as i32;
        assert_eq!(compute_rd_mult(161, RdFrame::Key), want);
        assert_eq!(compute_rd_mult(0, RdFrame::Key), (16.0 * 4.35) as i32);
    }

    #[test]
    fn token_costs_add_the_costs_down_the_tree() {
        // DC_PRED is the tree's first leaf: one 0 at the first node.
        let mut costs = [0i32; INTRA_MODES];
        let probs = [100u8; 9];
        cost_tokens(&mut costs, &probs, &INTRA_MODE_TREE);
        assert_eq!(costs[0], cost_zero(100) as i32);
        assert!(costs.iter().all(|&c| c > 0));
        let table = kf_y_mode_costs();
        assert_eq!(
            table[0][0][0],
            cost_zero(tables::KF_Y_MODE_PROB[0][0][0]) as i32
        );
    }
}

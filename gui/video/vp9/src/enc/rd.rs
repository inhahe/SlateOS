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
use crate::common::{
    BLOCK_8X8, BLOCK_16X16, BLOCK_SIZES, BlockSize, INTER_MODE_CONTEXTS, INTER_MODE_TREE,
    INTER_MODES, INTRA_MODE_TREE, INTRA_MODES, SWITCHABLE_FILTER_CONTEXTS, SWITCHABLE_FILTERS,
    SWITCHABLE_INTERP_TREE,
};
use crate::enc::cost::{PROB_COST_SHIFT, cost_bit};
use crate::enc::ratectrl::qindex_to_q;
use crate::header::{MAXQ, dc_quant};
use crate::probs::FrameContext;
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
    GoldenOrAltRef,
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

/// The modes' threshold slots: libvpx's `THR_MODES`, the order of its
/// rate-distortion mode list.
pub(crate) const THR_NEARESTMV: usize = 0;
pub(crate) const THR_NEARESTA: usize = 1;
pub(crate) const THR_NEARESTG: usize = 2;
pub(crate) const THR_DC: usize = 3;
pub(crate) const THR_NEWMV: usize = 4;
pub(crate) const THR_NEWA: usize = 5;
pub(crate) const THR_NEWG: usize = 6;
pub(crate) const THR_NEARMV: usize = 7;
pub(crate) const THR_NEARA: usize = 8;
pub(crate) const THR_NEARG: usize = 9;
pub(crate) const THR_ZEROMV: usize = 10;
pub(crate) const THR_ZEROG: usize = 11;
pub(crate) const THR_ZEROA: usize = 12;
const THR_COMP_NEARESTLA: usize = 13;
const THR_COMP_NEARESTGA: usize = 14;
pub(crate) const THR_TM: usize = 15;
const THR_COMP_NEARLA: usize = 16;
const THR_COMP_NEWLA: usize = 17;
const THR_COMP_NEARGA: usize = 18;
const THR_COMP_NEWGA: usize = 19;
const THR_COMP_ZEROLA: usize = 20;
const THR_COMP_ZEROGA: usize = 21;
pub(crate) const THR_H_PRED: usize = 22;
pub(crate) const THR_V_PRED: usize = 23;
const THR_D135_PRED: usize = 24;
const THR_D207_PRED: usize = 25;
const THR_D153_PRED: usize = 26;
const THR_D63_PRED: usize = 27;
const THR_D117_PRED: usize = 28;
const THR_D45_PRED: usize = 29;
/// libvpx's `MAX_MODES`.
pub(crate) const MAX_MODES: usize = 30;

/// A block size's mode threshold factors start here: libvpx's
/// `RD_THRESH_INIT_FACT`, and the most they may grow to per unit of
/// `adaptive_rd_thresh` (`RD_THRESH_MAX_FACT`), a step at a time
/// (`RD_THRESH_INC`).
pub(crate) const RD_THRESH_INIT_FACT: i32 = 32;
pub(crate) const RD_THRESH_MAX_FACT: i32 = 64;
pub(crate) const RD_THRESH_INC: i32 = 1;

/// Each mode's threshold multiplier: libvpx's `vp9_set_rd_speed_thresholds`
/// for a mode other than its best-quality one. A mode is not tried when the
/// best cost so far is already below its threshold.
pub(crate) fn thresh_mult(adaptive_rd_thresh: bool) -> [i32; MAX_MODES] {
    let mut t = [0i32; MAX_MODES];
    let nearest = if adaptive_rd_thresh { 300 } else { 0 };
    t[THR_NEARESTMV] = nearest;
    t[THR_NEARESTG] = nearest;
    t[THR_NEARESTA] = nearest;
    t[THR_DC] += 1000;
    t[THR_NEWMV] += 1000;
    t[THR_NEWA] += 1000;
    t[THR_NEWG] += 1000;
    t[THR_NEARMV] += 1000;
    t[THR_NEARA] += 1000;
    t[THR_COMP_NEARESTLA] += 1000;
    t[THR_COMP_NEARESTGA] += 1000;
    t[THR_TM] += 1000;
    t[THR_COMP_NEARLA] += 1500;
    t[THR_COMP_NEWLA] += 2000;
    t[THR_NEARG] += 1000;
    t[THR_COMP_NEARGA] += 1500;
    t[THR_COMP_NEWGA] += 2000;
    t[THR_ZEROMV] += 2000;
    t[THR_ZEROG] += 2000;
    t[THR_ZEROA] += 2000;
    t[THR_COMP_ZEROLA] += 2500;
    t[THR_COMP_ZEROGA] += 2500;
    t[THR_H_PRED] += 2000;
    t[THR_V_PRED] += 2000;
    t[THR_D45_PRED] += 2500;
    t[THR_D135_PRED] += 2500;
    t[THR_D117_PRED] += 2500;
    t[THR_D153_PRED] += 2500;
    t[THR_D207_PRED] += 2500;
    t[THR_D63_PRED] += 2500;
    t
}

/// The quantiser's part of the mode thresholds: libvpx's
/// `compute_rd_thresh_factor`, 8-bit, `pow` in double precision as libvpx
/// computes it (a test pins every value to glibc's).
pub(crate) fn rd_thresh_factor(qindex: i32) -> i32 {
    let q = f64::from(dc_quant(qindex, 0, 8)) / 4.0;
    ((q.powf(1.25) * 5.12) as i32).max(8)
}

/// libvpx's `rd_thresh_block_size_factor`: thresholds are set for 8x8
/// blocks and scaled by size (in quarters).
const RD_THRESH_BLOCK_SIZE_FACTOR: [i32; BLOCK_SIZES] =
    [2, 3, 3, 4, 6, 6, 8, 12, 12, 16, 24, 24, 32];

/// The mode thresholds of every block size of 8x8 and up for a segment
/// coded at `qindex` (its own index plus the DC delta, clamped): libvpx's
/// `set_block_thresholds` for one segment.
#[allow(
    clippy::indexing_slicing,
    reason = "the block sizes index the 13-entry size factors they enumerate"
)]
pub(crate) fn block_thresholds(
    qindex: i32,
    thresh_mult: &[i32; MAX_MODES],
) -> [[i32; MAX_MODES]; BLOCK_SIZES] {
    let q = rd_thresh_factor(qindex.clamp(0, MAXQ));
    let mut out = [[i32::MAX; MAX_MODES]; BLOCK_SIZES];
    for (bsize, row) in out.iter_mut().enumerate().skip(usize::from(BLOCK_8X8)) {
        let t = q * RD_THRESH_BLOCK_SIZE_FACTOR[bsize];
        let thresh_max = i32::MAX / t;
        for (slot, &m) in row.iter_mut().zip(thresh_mult) {
            *slot = if m < thresh_max { m * t / 4 } else { i32::MAX };
        }
    }
    out
}

/// Whether a mode's threshold, scaled by its adaptive factor, is above the
/// best cost so far, so the mode need not be tried: libvpx's
/// `rd_less_than_thresh`.
pub(crate) fn rd_less_than_thresh(best_rd: i64, thresh: i32, thresh_fact: i32) -> bool {
    best_rd < ((i64::from(thresh) * i64::from(thresh_fact)) >> 5) || thresh == i32::MAX
}

/// The full-pixel search's weight of a vector's cost: libvpx's
/// `sad_per_bit16lut_8`, `(int)(0.0418 * q + 2.4107)` of the quantiser's
/// step (`vp9_convert_qindex_to_q`).
pub(crate) fn sad_per_bit16(qindex: i32) -> i32 {
    (0.0418 * qindex_to_q(qindex.clamp(0, MAXQ)) + 2.4107) as i32
}

/// The sub-pixel search's weight of a vector's cost: libvpx's
/// `set_error_per_bit`.
pub(crate) fn error_per_bit(rdmult: i32) -> i32 {
    let e = rdmult >> 6;
    e + i32::from(e == 0)
}

/// libvpx's `rate_tab_q10`, `dist_tab_q10` and `xsq_iq_q10`: the rate and
/// distortion of a Laplacian source quantised with a step, sampled.
const RATE_TAB_Q10: [i32; 104] = [
    65536, 6086, 5574, 5275, 5063, 4899, 4764, 4651, 4553, 4389, 4255, 4142, 4044, 3958, 3881,
    3811, 3748, 3635, 3538, 3453, 3376, 3307, 3244, 3186, 3133, 3037, 2952, 2877, 2809, 2747, 2690,
    2638, 2589, 2501, 2423, 2353, 2290, 2232, 2179, 2130, 2084, 2001, 1928, 1862, 1802, 1748, 1698,
    1651, 1608, 1530, 1460, 1398, 1342, 1290, 1243, 1199, 1159, 1086, 1021, 963, 911, 864, 821,
    781, 745, 680, 623, 574, 530, 490, 455, 424, 395, 345, 304, 269, 239, 213, 190, 171, 154, 126,
    104, 87, 73, 61, 52, 44, 38, 28, 21, 16, 12, 10, 8, 6, 5, 3, 2, 1, 1, 1, 0, 0,
];
const DIST_TAB_Q10: [i32; 104] = [
    0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 5, 5, 6, 7, 7, 8, 9, 11, 12, 13, 15, 16, 17, 18, 21, 24, 26,
    29, 31, 34, 36, 39, 44, 49, 54, 59, 64, 69, 73, 78, 88, 97, 106, 115, 124, 133, 142, 151, 167,
    184, 200, 215, 231, 245, 260, 274, 301, 327, 351, 375, 397, 418, 439, 458, 495, 528, 559, 587,
    613, 637, 659, 680, 717, 749, 777, 801, 823, 842, 859, 874, 899, 919, 936, 949, 960, 969, 977,
    983, 994, 1001, 1006, 1010, 1013, 1015, 1017, 1018, 1020, 1022, 1022, 1023, 1023, 1023, 1024,
];
const XSQ_IQ_Q10: [i32; 104] = [
    0, 4, 8, 12, 16, 20, 24, 28, 32, 40, 48, 56, 64, 72, 80, 88, 96, 112, 128, 144, 160, 176, 192,
    208, 224, 256, 288, 320, 352, 384, 416, 448, 480, 544, 608, 672, 736, 800, 864, 928, 992, 1120,
    1248, 1376, 1504, 1632, 1760, 1888, 2016, 2272, 2528, 2784, 3040, 3296, 3552, 3808, 4064, 4576,
    5088, 5600, 6112, 6624, 7136, 7648, 8160, 9184, 10208, 11232, 12256, 13280, 14304, 15328,
    16352, 18400, 20448, 22496, 24544, 26592, 28640, 30688, 32736, 36832, 40928, 45024, 49120,
    53216, 57312, 61408, 65504, 73696, 81888, 90080, 98272, 106_464, 114_656, 122_848, 131_040,
    147_424, 163_808, 180_192, 196_576, 212_960, 229_344, 245_728,
];

/// libvpx's `model_rd_norm`: the tables interpolated at `xsq_q10`.
#[allow(
    clippy::indexing_slicing,
    reason = "xsq_q10 is capped at MAX_XSQ_Q10, whose sample index is 102, so xq + 1 is within the 104-entry tables"
)]
fn model_rd_norm(xsq_q10: i32) -> (i32, i32) {
    let tmp = (xsq_q10 >> 2) + 8;
    let k = (31 - tmp.leading_zeros() as i32) - 3;
    let xq = ((k << 3) + ((tmp >> k) & 0x7)) as usize;
    let one_q10 = 1 << 10;
    let a_q10 = ((xsq_q10 - XSQ_IQ_Q10[xq]) << 10) >> (2 + k);
    let b_q10 = one_q10 - a_q10;
    let r = (RATE_TAB_Q10[xq] * b_q10 + RATE_TAB_Q10[xq + 1] * a_q10) >> 10;
    let d = (DIST_TAB_Q10[xq] * b_q10 + DIST_TAB_Q10[xq + 1] * a_q10) >> 10;
    (r, d)
}

/// The estimated rate (1/512 bits) and distortion of `2^n_log2` samples of
/// variance `var` quantised with step `qstep`: libvpx's
/// `vp9_model_rd_from_var_lapndz`.
pub(crate) fn model_rd_from_var_lapndz(var: u32, n_log2: u32, qstep: u32) -> (i32, i64) {
    if var == 0 {
        return (0, 0);
    }
    const MAX_XSQ_Q10: u64 = 245_727;
    let xsq_q10_64 = (((u64::from(qstep) * u64::from(qstep)) << (n_log2 + 10))
        + u64::from(var >> 1))
        / u64::from(var);
    let xsq_q10 = xsq_q10_64.min(MAX_XSQ_Q10) as i32;
    let (r_q10, d_q10) = model_rd_norm(xsq_q10);
    let shift = 10 - PROB_COST_SHIFT;
    let rate = ((r_q10 << n_log2) + (1 << (shift - 1))) >> shift;
    let dist = (i64::from(var) * i64::from(d_q10) + 512) >> 10;
    (rate, dist)
}

/// The rate added to an intra mode in an inter frame, so that intra is
/// chosen only when clearly better: libvpx's `vp9_get_intra_cost_penalty`.
/// Smaller blocks pay less, unless the source is estimated to be very noisy.
pub(crate) fn intra_cost_penalty(
    bsize: BlockSize,
    qindex: i32,
    qdelta: i32,
    noise_high: bool,
) -> i32 {
    let reduction_fac = if noise_high {
        0
    } else if bsize <= BLOCK_8X8 {
        4
    } else if bsize <= BLOCK_16X16 {
        2
    } else {
        0
    };
    (20 * i32::from(dc_quant(qindex, qdelta, 8))) >> reduction_fac
}

/// What coding a mode costs, as libvpx's realtime search reads it: its
/// `mbmode_cost`, `switchable_interp_costs` and `inter_mode_cost`, built
/// from the frame's probabilities by `fill_mode_costs` and
/// `vp9_build_inter_mode_cost` -- on key frames and every eighth frame from
/// the first, so the costs between are those of an older frame's
/// probabilities.
#[derive(Clone, Debug, Default)]
pub(crate) struct ModeCosts {
    /// An intra luma mode in an inter frame.
    pub mbmode: [i32; INTRA_MODES],
    /// An interpolation filter, by the neighbours' filters.
    pub switchable_interp: [[i32; SWITCHABLE_FILTERS]; SWITCHABLE_FILTER_CONTEXTS],
    /// An inter mode, by mode context and `mode - NEARESTMV`.
    pub inter_mode: [[i32; INTER_MODES]; INTER_MODE_CONTEXTS],
}

impl ModeCosts {
    /// libvpx's `fill_mode_costs` (the parts the realtime search reads).
    pub(crate) fn fill(&mut self, fc: &FrameContext) {
        cost_tokens(&mut self.mbmode, &fc.y_mode_prob[1], &INTRA_MODE_TREE);
        for (c, p) in self
            .switchable_interp
            .iter_mut()
            .zip(&fc.switchable_interp_prob)
        {
            cost_tokens(c, p, &SWITCHABLE_INTERP_TREE);
        }
    }

    /// libvpx's `vp9_build_inter_mode_cost`.
    pub(crate) fn build_inter_mode_cost(&mut self, fc: &FrameContext) {
        for (c, p) in self.inter_mode.iter_mut().zip(&fc.inter_mode_probs) {
            cost_tokens(c, p, &INTER_MODE_TREE);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing, reason = "a test: a failure should be loud")]

    use super::*;
    use crate::enc::cost::cost_zero;

    /// FNV-1a over the little-endian bytes of `f(0..=255)`.
    fn table_hash(f: impl Fn(i32) -> i32) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for q in 0..=255 {
            for b in f(q).to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    /// The floating-point constants are libvpx's at every quantiser: the
    /// hashes are those `tools/rdconst_reference.c` gives, the same
    /// expressions in C against libvpx's own quantiser tables, with glibc's
    /// `pow`.
    #[test]
    fn the_floating_point_constants_are_glibcs() {
        assert_eq!((rd_thresh_factor(0), rd_thresh_factor(100)), (8, 261));
        assert_eq!((rd_thresh_factor(200), rd_thresh_factor(255)), (1563, 7310));
        assert_eq!(table_hash(rd_thresh_factor), 10_639_358_763_122_460_441);
        assert_eq!((sad_per_bit16(100), sad_per_bit16(200)), (3, 9));
        assert_eq!(table_hash(sad_per_bit16), 11_837_657_131_020_772_153);
        assert_eq!(compute_rd_mult(200, RdFrame::Inter), 658_246);
        assert_eq!(
            table_hash(|q| compute_rd_mult(q, RdFrame::Inter)),
            4_301_386_624_769_563_932
        );
    }

    #[test]
    fn thresholds_scale_with_block_size() {
        let m = thresh_mult(true);
        assert_eq!(
            (m[THR_NEARESTMV], m[THR_ZEROMV], m[THR_NEWMV]),
            (300, 2000, 1000)
        );
        assert_eq!((m[THR_DC], m[THR_H_PRED], m[THR_NEWG]), (1000, 2000, 1000));
        let t = block_thresholds(200, &m);
        let q = rd_thresh_factor(200);
        assert_eq!(t[usize::from(BLOCK_8X8)][THR_ZEROMV], 2000 * (q * 4) / 4);
        assert_eq!(t[12][THR_NEARESTMV], 300 * (q * 32) / 4);
        assert!(rd_less_than_thresh(0, 100, 32));
        assert!(!rd_less_than_thresh(100, 100, 32));
        assert!(rd_less_than_thresh(5, i32::MAX, 0));
    }

    /// The model at its ends: no variance is free, and a quantiser far
    /// above the variance codes nothing and loses it all.
    #[test]
    fn the_laplacian_model_is_libvpxs() {
        assert_eq!(model_rd_from_var_lapndz(0, 8, 100), (0, 0));
        // A huge step against a small variance: xsq at its cap, the tables'
        // last entries (rate 0, distortion 1024/1024).
        let (r, d) = model_rd_from_var_lapndz(10, 8, 10_000);
        assert_eq!(r, 0);
        assert_eq!(d, (10 * 1024 + 512) >> 10);
        // A tiny step: xsq 0, rate_tab_q10[0] = 65536 per sample.
        let (r, d) = model_rd_from_var_lapndz(1_000_000, 4, 0);
        assert_eq!(r, (65536 << 4) >> 1);
        assert_eq!(d, 0);
        assert_eq!(error_per_bit(658_246), 10285);
        assert_eq!(error_per_bit(10), 1);
    }

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

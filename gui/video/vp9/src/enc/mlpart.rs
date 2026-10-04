//! libvpx's learned partitioning, the network half: whether a block of a
//! small picture is worth searching whole, cut in four, or both.
//!
//! At speed 8, inter frames of 352x288 pixels or fewer are partitioned by
//! search rather than by variance thresholds (`nonrd_pick_partition`, in
//! `nonrd`): each square block is tried whole and cut in four, and the
//! cheaper kept. A small network trims that search. From six numbers -- how
//! coarse the quantiser is, how far the block is from its estimated
//! prediction, and how that distance divides among its quarters -- it
//! scores the block; above zero the block is only cut, below zero only kept
//! whole, and at zero both are tried (libvpx's `ml_predict_var_partitioning`).
//!
//! The scores are compared with zero, so they are computed as libvpx's C
//! computes them: in single precision, multiply and add each rounded, in
//! the C code's order, with the C library's `logf` (`glibcmath`).
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_encodeframe.c`
//! (`nn_predict`, `ml_predict_var_partitioning`) and
//! `vp9/encoder/vp9_partition_models.h` (`vp9_var_part_nnconfig_64`, `_32`,
//! `_16`) (copyright the WebM project authors), used under libvpx's BSD
//! licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::cast_precision_loss,
    reason = "variances to single precision, as libvpx's C converts them"
)]
#![allow(
    clippy::excessive_precision,
    reason = "the weights as libvpx's source writes them, rounded to single precision as its compiler rounds them"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions inside a 64x64 superblock of a picture of at most 65536 a side, node indices below 8, and a quantiser step below 2^11 squared"
)]

use crate::common::{BLOCK_16X16, BLOCK_32X32, BLOCK_64X64, BlockSize};
use crate::enc::glibcmath::logf;
use crate::enc::variance;
use crate::frame::Plane;
use crate::tables;

/// The features: the quantiser, the block's variance, its quarters' shares.
const FEATURES: usize = 6;
/// The hidden layer's width.
const HIDDEN: usize = 8;

/// One of libvpx's partition networks: six inputs, one hidden layer of
/// eight with ReLU, one output (`NN_CONFIG`).
struct Net {
    /// The hidden layer's weights, `FEATURES` per node.
    w0: [f32; FEATURES * HIDDEN],
    b0: [f32; HIDDEN],
    w1: [f32; HIDDEN],
    b1: f32,
}

/// `vp9_var_part_nnconfig_64`.
const NET_64: Net = Net {
    w0: [
        -0.249_572, 0.205_532, -2.175_608, 1.094_836, -2.986_370, 0.193_160, -0.143_823, 0.378_511,
        -1.997_788, -2.166_866, -1.930_158, -1.202_127, -0.611_875, -0.506_422, -0.432_487,
        0.071_205, 0.578_172, -0.154_285, -0.051_830, 0.331_681, -1.457_177, -2.443_546,
        -2.000_302, -1.389_283, 0.372_084, -0.464_917, 2.265_235, 2.385_787, 2.312_722, 2.127_868,
        -0.403_963, -0.177_860, -0.436_751, -0.560_539, 0.254_903, 0.193_976, -0.305_611,
        0.256_632, 0.309_388, -0.437_439, 1.702_640, -5.007_069, -0.323_450, 0.294_227, 1.267_193,
        1.056_601, 0.387_181, -0.191_215,
    ],
    b0: [
        -0.044_396, -0.938_166, 0.000_000, -0.916_375, 1.242_299, 0.000_000, -0.405_734, 0.014_206,
    ],
    w1: [
        1.635_945, 0.979_557, 0.455_315, 1.197_199, -2.251_024, -0.464_953, 1.378_676, -0.111_927,
    ],
    b1: -0.379_724_47,
};

/// `vp9_var_part_nnconfig_32`.
const NET_32: Net = Net {
    w0: [
        0.067_243, -0.083_598, -2.191_159, 2.726_434, -3.324_013, 3.477_977, 0.323_736, -0.510_199,
        2.960_693, 2.937_661, 2.888_476, 2.938_315, -0.307_602, -0.503_353, -0.080_725, -0.473_909,
        -0.417_162, 0.457_089, 0.665_153, -0.273_210, 0.028_279, 0.972_220, -0.445_596, 1.756_611,
        -0.177_892, -0.091_758, 0.436_661, -0.521_506, 0.133_786, 0.266_743, 0.637_367, -0.160_084,
        -1.396_269, 1.020_841, -1.112_971, 0.919_496, -0.235_883, 0.651_954, 0.109_061, -0.429_463,
        0.740_839, -0.962_060, 0.299_519, -0.386_298, 1.550_231, 2.464_915, 1.311_969, 2.561_612,
    ],
    b0: [
        0.368_242, 0.736_617, 0.000_000, 0.757_287, 0.000_000, 0.613_248, -0.776_390, 0.928_497,
    ],
    w1: [
        0.939_884, -2.420_850, -0.410_489, -0.186_690, 0.063_287, -0.522_011, 0.484_527, -0.639_625,
    ],
    b1: -0.645_500_6,
};

/// `vp9_var_part_nnconfig_16`.
const NET_16: Net = Net {
    w0: [
        0.742_567, -0.580_624, -0.244_528, 0.331_661, -0.113_949, -0.559_295, -0.386_061,
        0.438_653, 1.467_463, 0.211_589, 0.513_972, 1.067_855, -0.876_679, 0.088_560, -0.687_483,
        -0.380_304, -0.016_412, 0.146_380, 0.015_318, 0.000_351, -2.764_887, 3.269_717, 2.752_428,
        -2.236_754, 0.561_539, -0.852_050, -0.084_667, 0.202_057, 0.197_049, 0.364_922, -0.463_801,
        0.431_790, 1.872_096, -0.091_887, -0.055_034, 2.443_492, -0.156_958, -0.189_571,
        -0.542_424, -0.589_804, -0.354_422, 0.401_605, 0.642_021, -0.875_117, 2.040_794, 1.921_070,
        1.792_413, 1.839_727,
    ],
    b0: [
        2.901_234, -1.940_932, -0.198_970, -0.406_524, 0.059_422, -1.879_207, -0.232_340, 2.979_821,
    ],
    w1: [
        -0.528_731, 0.375_234, -0.088_422, 0.668_629, 0.870_449, 0.578_735, 0.546_103, -1.957_207,
    ],
    b1: -1.957_694_05,
};

/// libvpx's `nn_predict` for a one-hidden-layer, one-output network: each
/// node's weighted sum taken in order, in single precision, then its bias.
fn nn_predict(features: &[f32; FEATURES], net: &Net) -> f32 {
    let mut hidden = [0.0f32; HIDDEN];
    for (node, out) in hidden.iter_mut().enumerate() {
        let weights = net
            .w0
            .get(node * FEATURES..(node + 1) * FEATURES)
            .unwrap_or(&[]);
        let mut val = 0.0f32;
        for (w, x) in weights.iter().zip(features) {
            val += w * x;
        }
        val += net.b0.get(node).copied().unwrap_or(0.0);
        // ReLU, as libvpx's VPXMAX(val, 0.0f).
        *out = if val > 0.0 { val } else { 0.0 };
    }
    let mut val = 0.0f32;
    for (w, x) in net.w1.iter().zip(&hidden) {
        val += w * x;
    }
    val + net.b1
}

/// What the network says of a block: cut it, keep it whole, or try both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Split,
    Whole,
    Both,
}

/// libvpx's `ml_predict_var_partitioning` at speed 8 (threshold zero) for
/// the square block `bsize` at (`mi_row`, `mi_col`), fully inside the
/// picture: its verdict, and the network's score (`None` for an 8x8 block,
/// which has no network and is never asked).
///
/// `src` is the picture's luma, `est_pred` the superblock's estimated
/// prediction (64x64, rows 64 apart: `get_estimated_pred`'s), and `dc_q`
/// the frame's luma DC quantiser step.
pub(crate) fn predict(
    src: &Plane<u8>,
    est_pred: &[u8],
    mi_row: usize,
    mi_col: usize,
    bsize: BlockSize,
    dc_q: i32,
) -> Option<(Verdict, f32)> {
    let net = match bsize {
        BLOCK_64X64 => &NET_64,
        BLOCK_32X32 => &NET_32,
        BLOCK_16X16 => &NET_16,
        _ => return None,
    };
    let bs = 4 * usize::from(
        tables::NUM_4X4_WIDE
            .get(usize::from(bsize))
            .copied()
            .unwrap_or(0),
    );
    let half = bs / 2;
    let (x0, y0) = (mi_col * 8, mi_row * 8);
    let s = src.data.get(y0 * src.stride + x0..).unwrap_or(&[]);
    let p = est_pred
        .get(8 * (mi_row & 7) * 64 + 8 * (mi_col & 7)..)
        .unwrap_or(&[]);
    let mut features = [0.0f32; FEATURES];
    features[0] = logf((dc_q * dc_q) as f32 / 256.0 + 1.0);
    let (var, _) = variance::variance(s, src.stride, p, 64, bs, bs);
    let factor = if var == 0 { 1.0 } else { 1.0 / var as f32 };
    features[1] = logf(var as f32 + 1.0);
    for (i, feature) in features.iter_mut().skip(2).enumerate() {
        let (dx, dy) = ((i & 1) * half, (i >> 1) * half);
        let (sub_var, _) = variance::variance(
            s.get(dy * src.stride + dx..).unwrap_or(&[]),
            src.stride,
            p.get(dy * 64 + dx..).unwrap_or(&[]),
            64,
            half,
            half,
        );
        *feature = if var == 0 {
            1.0
        } else {
            factor * sub_var as f32
        };
    }
    let score = nn_predict(&features, net);
    let verdict = if score > 0.0 {
        Verdict::Split
    } else if score < 0.0 {
        Verdict::Whole
    } else {
        Verdict::Both
    };
    Some((verdict, score))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::float_cmp,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    /// A network's output is its biases' where every hidden node is cut off
    /// by its ReLU, and a hidden node passes its sum on where positive.
    #[test]
    fn the_network_is_relu_then_linear() {
        // All-zero features: each hidden node is its bias, cut at zero.
        let zero = [0.0f32; FEATURES];
        let expect = NET_64
            .b0
            .iter()
            .zip(&NET_64.w1)
            .fold(0.0f32, |acc, (&b, &w)| acc + w * b.max(0.0))
            + NET_64.b1;
        assert_eq!(nn_predict(&zero, &NET_64), expect);
        assert_eq!(
            nn_predict(&zero, &NET_16),
            NET_16
                .b0
                .iter()
                .zip(&NET_16.w1)
                .fold(0.0f32, |acc, (&b, &w)| acc + w * b.max(0.0))
                + NET_16.b1
        );
    }

    /// A block identical to its prediction has variance zero: its quarters'
    /// shares are taken as one each, and only the quantiser moves the score.
    #[test]
    fn a_perfectly_predicted_block_is_judged_by_its_quantiser() {
        let plane = Plane {
            data: vec![77u8; 64 * 64],
            stride: 64,
            alloc_height: 64,
            crop_width: 64,
            crop_height: 64,
            width: 64,
            height: 64,
        };
        let est = [77u8; 64 * 64];
        let (_, score) = predict(&plane, &est, 0, 0, BLOCK_64X64, 40).unwrap();
        let features = [logf(40.0 * 40.0 / 256.0 + 1.0), 0.0, 1.0, 1.0, 1.0, 1.0];
        assert_eq!(score, nn_predict(&features, &NET_64));
        assert!(predict(&plane, &est, 0, 0, crate::common::BLOCK_8X8, 40).is_none());
    }
}

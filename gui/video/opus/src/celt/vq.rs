//! A band's shape (RFC 6716 §4.3.4): its PVQ codeword decoded, scaled to unit
//! norm, and spread by the rotation the frame's spreading decision asks for;
//! and the renormalisation folded and split bands get.
//!
//! Normalised coefficients (`celt_norm`) are Q14 in 16 bits.
//!
//! Translated into Rust from libopus 1.5.2's `celt/vq.c` (the decoder's
//! half) and `celt/pitch.h` (`celt_inner_prod`), `FIXED_POINT`, copyright
//! Xiph.Org, Jean-Marc Valin and the contributors named in its `COPYING`,
//! used under libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within the band's own `n` coefficients: the rotation's two cursors stay `stride` apart inside `len`, as libopus's pointer walks do, and `n` is the slice's length"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sizes below 352 coefficients and pulse counts below 128; the samples' arithmetic is libopus's on Q14 values, the products within 32 bits"
)]

use super::cwrs::decode_pulses;
use super::mathops::{cos_norm, div, ilog2, rsqrt_norm};
use crate::entdec::Decoder;
use crate::fixed::{
    EPSILON, Q15ONE, extract16, mac16_16, mult16_16, mult16_16_p15, mult16_16_q15, pshr32, sub16,
    vshr32,
};

/// No spreading at all.
pub(crate) const SPREAD_NONE: i32 = 0;

/// `celt_inner_prod`: the dot product of two Q14 vectors.
pub(crate) fn inner_prod(x: &[i32], y: &[i32]) -> i32 {
    x.iter()
        .zip(y)
        .fold(0i32, |acc, (&a, &b)| mac16_16(acc, a, b))
}

/// `exp_rotation1`: one pass of the spreading rotation, with cosine `c` and
/// sine `s`, between coefficients `stride` apart.
fn exp_rotation1(x: &mut [i32], len: usize, stride: usize, c: i32, s: i32) {
    let ms = extract16(-s);
    if len > stride {
        for i in 0..len - stride {
            let x1 = x[i];
            let x2 = x[i + stride];
            x[i + stride] = extract16(pshr32(mac16_16(mult16_16(c, x2), s, x1), 15));
            x[i] = extract16(pshr32(mac16_16(mult16_16(c, x1), ms, x2), 15));
        }
    }
    if len > 2 * stride {
        for i in (0..len - 2 * stride).rev() {
            let x1 = x[i];
            let x2 = x[i + stride];
            x[i + stride] = extract16(pshr32(mac16_16(mult16_16(c, x2), s, x1), 15));
            x[i] = extract16(pshr32(mac16_16(mult16_16(c, x1), ms, x2), 15));
        }
    }
}

/// `exp_rotation` with `dir` -1, the decoder's: the encoder's spreading of
/// `x`'s `len` coefficients, in `stride` interleaved blocks, by the spread
/// setting and the pulse count `k`, undone. (The encoder's direction, +1,
/// is the same rotations reversed, which a decoder never makes.)
pub(crate) fn exp_rotation(x: &mut [i32], len: usize, stride: usize, k: i32, spread: i32) {
    const SPREAD_FACTOR: [i32; 3] = [15, 10, 5];
    let len_i = len as i32;
    if 2 * k >= len_i || spread == SPREAD_NONE {
        return;
    }
    let factor = SPREAD_FACTOR[(spread - 1).clamp(0, 2) as usize];
    let gain = extract16(div(mult16_16(Q15ONE, len_i), len_i + factor * k));
    let theta = extract16(mult16_16_q15(gain, gain) >> 1);
    let c = cos_norm(theta);
    // sin(theta).
    let s = cos_norm(sub16(Q15ONE, theta));
    let mut stride2 = 0usize;
    if len >= 8 * stride {
        stride2 = 1;
        // sqrt(len / stride), rounded.
        while (stride2 * stride2 + stride2) * stride + (stride >> 2) < len {
            stride2 += 1;
        }
    }
    let len = len.checked_div(stride).unwrap_or(0);
    for i in 0..stride {
        let block = &mut x[i * len..(i + 1) * len];
        if stride2 != 0 {
            exp_rotation1(block, len, stride2, s, c);
        }
        exp_rotation1(block, len, 1, c, s);
    }
}

/// `normalise_residual`: the pulses `iy`, of squared norm `ryy`, scaled to
/// `gain` into `x`.
fn normalise_residual(iy: &[i32], x: &mut [i32], n: usize, ryy: i32, gain: i32) {
    let k = ilog2(ryy) >> 1;
    let t = vshr32(ryy, 2 * (k - 7));
    let g = extract16(mult16_16_p15(rsqrt_norm(t), gain));
    let shift = u32::try_from(k + 1).unwrap_or(0);
    for i in 0..n {
        x[i] = extract16(pshr32(mult16_16(g, iy[i]), shift));
    }
}

/// `extract_collapse_mask`: which of the `b` interleaved blocks got pulses.
fn extract_collapse_mask(iy: &[i32], n: usize, b: usize) -> u32 {
    if b <= 1 {
        return 1;
    }
    let n0 = n.checked_div(b).unwrap_or(0);
    let mut mask = 0u32;
    for i in 0..b {
        let any = iy[i * n0..(i + 1) * n0].iter().fold(0, |t, &v| t | v);
        mask |= u32::from(any != 0) << i;
    }
    mask
}

/// `alg_unquant`: a band's `k` pulses in `n` dimensions decoded into `x`,
/// scaled to `gain` and unspread; which of its `b` blocks got pulses.
pub(crate) fn alg_unquant(
    x: &mut [i32],
    n: usize,
    k: i32,
    spread: i32,
    b: usize,
    dec: &mut Decoder<'_>,
    gain: i32,
) -> u32 {
    let mut iy = vec![0i32; n];
    let ryy = decode_pulses(&mut iy, n as i32, k, dec);
    normalise_residual(&iy, x, n, ryy, gain);
    exp_rotation(x, n, b, k, spread);
    extract_collapse_mask(&iy, n, b)
}

/// `renormalise_vector`: `x`'s `n` coefficients scaled to norm `gain`.
pub(crate) fn renormalise_vector(x: &mut [i32], n: usize, gain: i32) {
    let e = EPSILON.wrapping_add(inner_prod(&x[..n], &x[..n]));
    let k = ilog2(e) >> 1;
    let t = vshr32(e, 2 * (k - 7));
    let g = extract16(mult16_16_p15(rsqrt_norm(t), gain));
    let shift = u32::try_from(k + 1).unwrap_or(0);
    for v in &mut x[..n] {
        *v = extract16(pshr32(mult16_16(g, *v), shift));
    }
}

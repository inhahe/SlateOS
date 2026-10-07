//! The pitch search CELT's packet-loss concealment repeats the last period
//! of: the decoded signal low-passed and halved in rate (`pitch_downsample`),
//! then searched for its strongest period at a quarter and a half the rate
//! (`pitch_search`).
//!
//! Translated into Rust from libopus 1.5.2's `celt/pitch.c` (the parts the
//! decoder uses), `FIXED_POINT`, copyright Xiph.Org, Octasic and the
//! contributors named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within the buffers at libopus's sizes: `len` samples a channel in, `len / 2` out, and the search's lags below `max_pitch` over `len` samples of a buffer `len + max_pitch` long"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sizes below 2048; the sample arithmetic wraps where C's would overflow, through the fixed-point helpers"
)]

use super::lpc::{autocorr, fir5, lpc, maxabs16, maxabs32, pitch_xcorr};
use super::mathops::ilog2;
use crate::fixed::{
    Q15ONE, SIG_SHIFT, extract16, mult16_16, mult16_16_q15, mult16_32_q15, qconst16, vshr32,
};

/// `pitch_downsample`: `len` samples of each of `x`'s channels, summed,
/// low-passed and halved in rate into `x_lp` (`len / 2` values), scaled to
/// fit 16 bits, and whitened by a 4th-order LPC with a zero added.
pub(crate) fn pitch_downsample(x: &[&[i32]], x_lp: &mut [i32], len: usize) {
    let c = x.len();
    let half = len >> 1;
    let mut maxabs = x.iter().map(|ch| maxabs32(&ch[..len])).max().unwrap_or(0);
    if maxabs < 1 {
        maxabs = 1;
    }
    let mut shift = (ilog2(maxabs) - 10).max(0);
    if c == 2 {
        shift += 1;
    }
    let (s1, s2) = ((shift + 1) as u32, (shift + 2) as u32);
    for (k, ch) in x.iter().enumerate() {
        let term = |i: usize| (ch[2 * i - 1] >> s2) + (ch[2 * i + 1] >> s2) + (ch[2 * i] >> s1);
        let first = (ch[1] >> s2) + (ch[0] >> s1);
        if k == 0 {
            x_lp[0] = extract16(first);
            for i in 1..half {
                x_lp[i] = extract16(term(i));
            }
        } else {
            x_lp[0] = extract16(x_lp[0] + first);
            for i in 1..half {
                x_lp[i] = extract16(x_lp[i] + term(i));
            }
        }
    }
    let mut ac = [0i32; 5];
    autocorr(x_lp, &mut ac, &[], 0, 4, half);
    // A noise floor at -40 dB.
    ac[0] = ac[0].wrapping_add(ac[0] >> 13);
    // Lag windowing.
    for (i, a) in ac.iter_mut().enumerate().skip(1) {
        let i = i as i32;
        *a = a.wrapping_sub(mult16_32_q15(2 * i * i, *a));
    }
    // libopus leaves this array uninitialised, which its fallback for
    // coefficients that will not fit (only the first set) would expose;
    // zeros stand in for whatever its stack held.
    let mut lpc4 = [0i32; 4];
    lpc(&mut lpc4, &ac, 4);
    let mut tmp = Q15ONE;
    for v in &mut lpc4 {
        tmp = extract16(mult16_16_q15(qconst16(0.9, 15), tmp));
        *v = extract16(mult16_16_q15(*v, tmp));
    }
    // A zero added.
    let c1 = qconst16(0.8, 15);
    let lpc2 = [
        extract16(lpc4[0] + qconst16(0.8, SIG_SHIFT)),
        extract16(lpc4[1] + mult16_16_q15(c1, lpc4[0])),
        extract16(lpc4[2] + mult16_16_q15(c1, lpc4[1])),
        extract16(lpc4[3] + mult16_16_q15(c1, lpc4[2])),
        extract16(mult16_16_q15(c1, lpc4[3])),
    ];
    fir5(x_lp, &lpc2, half);
}

/// `find_best_pitch`: the two lags below `max_pitch` whose correlation in
/// `xcorr`, normalised by `y`'s energy over `len` samples there, is
/// highest, best first.
fn find_best_pitch(
    xcorr: &[i32],
    y: &[i32],
    len: usize,
    max_pitch: usize,
    yshift: u32,
    maxcorr: i32,
) -> [usize; 2] {
    let xshift = ilog2(maxcorr) - 14;
    let mut best_num = [-1i32; 2];
    let mut best_den = [0i32; 2];
    let mut best_pitch = [0usize, 1];
    let mut syy = 1i32;
    for &v in &y[..len] {
        syy = syy.wrapping_add(mult16_16(v, v) >> yshift);
    }
    for i in 0..max_pitch {
        if xcorr[i] > 0 {
            let xcorr16 = extract16(vshr32(xcorr[i], xshift));
            let num = extract16(mult16_16_q15(xcorr16, xcorr16));
            if mult16_32_q15(num, best_den[1]) > mult16_32_q15(best_num[1], syy) {
                if mult16_32_q15(num, best_den[0]) > mult16_32_q15(best_num[0], syy) {
                    best_num[1] = best_num[0];
                    best_den[1] = best_den[0];
                    best_pitch[1] = best_pitch[0];
                    best_num[0] = num;
                    best_den[0] = syy;
                    best_pitch[0] = i;
                } else {
                    best_num[1] = num;
                    best_den[1] = syy;
                    best_pitch[1] = i;
                }
            }
        }
        syy = syy
            .wrapping_add(mult16_16(y[i + len], y[i + len]) >> yshift)
            .wrapping_sub(mult16_16(y[i], y[i]) >> yshift);
        syy = syy.max(1);
    }
    best_pitch
}

/// `pitch_search`: the period, in samples at `x_lp`'s rate, below
/// `max_pitch` at which `y` best matches `x_lp`'s `len` values -- searched
/// at half that rate, refined at the full one, and interpolated.
pub(crate) fn pitch_search(x_lp: &[i32], y: &[i32], len: usize, max_pitch: usize) -> i32 {
    let lag = len + max_pitch;
    // Halved in rate again.
    let mut x_lp4: Vec<i32> = (0..len >> 2).map(|j| x_lp[2 * j]).collect();
    let mut y_lp4: Vec<i32> = (0..lag >> 2).map(|j| y[2 * j]).collect();
    let xmax = maxabs16(&x_lp4);
    let ymax = maxabs16(&y_lp4);
    let mut shift = ilog2(1.max(xmax.max(ymax))) - 11;
    if shift > 0 {
        for v in x_lp4.iter_mut().chain(y_lp4.iter_mut()) {
            *v >>= shift;
        }
        // Doubled: it applies to products.
        shift *= 2;
    } else {
        shift = 0;
    }
    let shift = shift as u32;
    let mut xcorr = vec![0i32; max_pitch >> 1];
    // The coarse search, at a quarter the rate.
    let maxcorr = pitch_xcorr(&x_lp4, &y_lp4, &mut xcorr, len >> 2, max_pitch >> 2);
    let best = find_best_pitch(&xcorr, &y_lp4, len >> 2, max_pitch >> 2, 0, maxcorr);
    // The finer search, at half the rate, near the coarse search's two best.
    let mut maxcorr = 1i32;
    for i in 0..max_pitch >> 1 {
        xcorr[i] = 0;
        let near = |b: usize| (i as i64 - 2 * b as i64).abs() <= 2;
        if !near(best[0]) && !near(best[1]) {
            continue;
        }
        let sum = (0..len >> 1).fold(0i32, |s, j| {
            s.wrapping_add(mult16_16(x_lp[j], y[i + j]) >> shift)
        });
        xcorr[i] = sum.max(-1);
        maxcorr = maxcorr.max(sum);
    }
    let best = find_best_pitch(&xcorr, y, len >> 1, max_pitch >> 1, shift + 1, maxcorr);
    // Refined by pseudo-interpolation.
    let mut offset = 0;
    if best[0] > 0 && best[0] < (max_pitch >> 1) - 1 {
        let a = xcorr[best[0] - 1];
        let b = xcorr[best[0]];
        let c = xcorr[best[0] + 1];
        if c.wrapping_sub(a) > mult16_32_q15(qconst16(0.7, 15), b.wrapping_sub(a)) {
            offset = 1;
        } else if a.wrapping_sub(c) > mult16_32_q15(qconst16(0.7, 15), b.wrapping_sub(c)) {
            offset = -1;
        }
    }
    2 * best[0] as i32 - offset
}

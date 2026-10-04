//! Linear prediction for CELT's packet-loss concealment: autocorrelation,
//! the Levinson-Durbin recursion to LPC coefficients (Q12, fitted into 16
//! bits by bandwidth expansion), and the FIR and IIR filters that take a
//! signal to its excitation and back.
//!
//! Translated into Rust from libopus 1.5.2's `celt/celt_lpc.c` and the plain
//! C correlation of `celt/pitch.c` (`celt_pitch_xcorr_c`, `xcorr_kernel_c`
//! in `celt/pitch.h`), `FIXED_POINT`, copyright Xiph.Org, Octasic and the
//! contributors named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is within the caller's buffers at the sizes libopus uses them: `ac` has `lag + 1` entries, the filters' `ord` samples of history precede `at`, and the LPC arrays hold CELT_LPC_ORDER"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "sizes below 2048 and orders below 25; the sample arithmetic wraps where C's would overflow, through the fixed-point helpers"
)]

use super::mathops::{frac_div32, ilog2};
use crate::entdec::ilog;
use crate::fixed::{
    SIG_SHIFT, div32, extract16, mac16_16, mult16_16, mult16_16_q15, mult32_32_32, mult32_32_q16,
    mult32_32_q31, pshr32, qconst32, round16, shl32, sround16,
};

/// `CELT_LPC_ORDER`: the concealment's prediction order.
pub(crate) const LPC_ORDER: usize = 24;

/// `celt_maxabs16`: the largest magnitude among 16-bit values.
pub(crate) fn maxabs16(x: &[i32]) -> i32 {
    let (max, min) = x
        .iter()
        .fold((0i32, 0i32), |(max, min), &v| (max.max(v), min.min(v)));
    max.max(-min)
}

/// `celt_maxabs32`: the largest magnitude among 32-bit values (a minimum
/// of `i32::MIN` has no magnitude in 32 bits, and wraps as C's does).
pub(crate) fn maxabs32(x: &[i32]) -> i32 {
    let (max, min) = x
        .iter()
        .fold((0i32, 0i32), |(max, min), &v| (max.max(v), min.min(v)));
    max.max(min.wrapping_neg())
}

/// `celt_pitch_xcorr`: `xcorr[i]` the correlation of `x`'s first `len`
/// values with `y` from `i`, for `i` below `max_pitch`; the largest of them
/// (and 1).
pub(crate) fn pitch_xcorr(
    x: &[i32],
    y: &[i32],
    xcorr: &mut [i32],
    len: usize,
    max_pitch: usize,
) -> i32 {
    let mut maxcorr = 1i32;
    for i in 0..max_pitch {
        let sum = x[..len]
            .iter()
            .zip(&y[i..i + len])
            .fold(0i32, |s, (&a, &b)| mac16_16(s, a, b));
        xcorr[i] = sum;
        maxcorr = maxcorr.max(sum);
    }
    maxcorr
}

/// `_celt_autocorr`: the autocorrelation of `x`'s `n` values at lags 0 to
/// `lag` into `ac`, `x` first windowed at both ends over `overlap` samples
/// and scaled down to keep the sums in 32 bits, the result normalised so
/// that `ac[0]` is in [2^28, 2^29); the net shift applied.
pub(crate) fn autocorr(
    x: &[i32],
    ac: &mut [i32],
    window: &[i16],
    overlap: usize,
    lag: usize,
    n: usize,
) -> i32 {
    let fast_n = n - lag;
    let mut xx: Vec<i32> = x[..n].to_vec();
    for i in 0..overlap {
        let w = i32::from(window[i]);
        xx[i] = extract16(mult16_16_q15(x[i], w));
        xx[n - i - 1] = extract16(mult16_16_q15(x[n - i - 1], w));
    }
    let mut ac0 = 1i32.wrapping_add((n as i32) << 7);
    for &v in &xx {
        ac0 = ac0.wrapping_add(mult16_16(v, v) >> 9);
    }
    // C's division, truncating toward zero.
    let excess = ilog2(ac0) - 30 + 10;
    let mut shift = excess / 2;
    if shift > 0 {
        for v in &mut xx {
            *v = extract16(pshr32(*v, shift as u32));
        }
    } else {
        shift = 0;
    }
    pitch_xcorr(&xx, &xx, ac, fast_n, lag + 1);
    for k in 0..=lag {
        let d = (k + fast_n..n).fold(0i32, |d, i| mac16_16(d, xx[i], xx[i - k]));
        ac[k] = ac[k].wrapping_add(d);
    }
    shift *= 2;
    if shift <= 0 {
        ac[0] = ac[0].wrapping_add(shl32(1, (-shift) as u32));
    }
    if ac[0] < 268_435_456 {
        let shift2 = 29 - ilog(ac[0] as u32);
        for v in &mut ac[..=lag] {
            // A negative shift is C's undefined behaviour, and x86's
            // masked shift; `wrapping_shl` masks the same way.
            *v = shl32(*v, shift2 as u32);
        }
        shift -= shift2;
    } else if ac[0] >= 536_870_912 {
        let shift2 = if ac[0] >= 1_073_741_824 { 2 } else { 1 };
        for v in &mut ac[..=lag] {
            *v >>= shift2;
        }
        shift += shift2;
    }
    shift
}

/// `_celt_lpc`: the order-`p` LPC coefficients of autocorrelation `ac`
/// (`p + 1` values) into `out`, in Q12. Where bandwidth expansion cannot
/// fit them into 16 bits in ten rounds, libopus sets only the first
/// coefficient (to 4096) and leaves the rest of `out` as it was -- and so
/// does this.
pub(crate) fn lpc(out: &mut [i32], ac: &[i32], p: usize) {
    let mut lpc = [0i32; LPC_ORDER];
    let mut error = ac[0];
    if ac[0] != 0 {
        for i in 0..p {
            // This iteration's reflection coefficient.
            let mut rr = 0i32;
            for j in 0..i {
                rr = rr.wrapping_add(mult32_32_q31(lpc[j], ac[i - j]));
            }
            rr = rr.wrapping_add(ac[i + 1] >> 6);
            let r = frac_div32(shl32(rr, 6), error).wrapping_neg();
            // The coefficients and the total error updated.
            lpc[i] = r >> 6;
            for j in 0..(i + 1) >> 1 {
                let tmp1 = lpc[j];
                let tmp2 = lpc[i - 1 - j];
                lpc[j] = tmp1.wrapping_add(mult32_32_q31(r, tmp2));
                lpc[i - 1 - j] = tmp2.wrapping_add(mult32_32_q31(r, tmp1));
            }
            error = error.wrapping_sub(mult32_32_q31(mult32_32_q31(r, r), error));
            // Out once the prediction gains 30 dB.
            if error <= ac[0] >> 10 {
                break;
            }
        }
    }
    // Into 16 bits without wrapping (silk_LPC_fit's and
    // silk_bwexpander_32's logic, as libopus has it here).
    let mut idx = 0usize;
    let mut iter = 0;
    while iter < 10 {
        let mut maxabs = 0i32;
        for (i, &v) in lpc[..p].iter().enumerate() {
            let absval = v.wrapping_abs();
            if absval > maxabs {
                maxabs = absval;
                idx = i;
            }
        }
        // Q25 to Q12.
        let maxabs = pshr32(maxabs, 13);
        if maxabs <= 32767 {
            break;
        }
        let maxabs = maxabs.min(163_838);
        let mut chirp_q16 = qconst32(0.999, 16).wrapping_sub(div32(
            shl32(maxabs - 32767, 14),
            mult32_32_32(maxabs, idx as i32 + 1) >> 2,
        ));
        let chirp_minus_one_q16 = chirp_q16.wrapping_sub(65536);
        // Bandwidth expansion.
        for v in &mut lpc[..p - 1] {
            *v = mult32_32_q16(chirp_q16, *v);
            chirp_q16 =
                chirp_q16.wrapping_add(pshr32(mult32_32_32(chirp_q16, chirp_minus_one_q16), 16));
        }
        lpc[p - 1] = mult32_32_q16(chirp_q16, lpc[p - 1]);
        iter += 1;
    }
    if iter == 10 {
        // A(z) = 1, as libopus writes it.
        out[0] = 4096;
    } else {
        for (o, &v) in out[..p].iter_mut().zip(&lpc[..p]) {
            // Q25 to Q12.
            *o = extract16(pshr32(v, 13));
        }
    }
}

/// `celt_fir`: `y[i]`, for `i` below `n`, the signal `x` from `at`
/// filtered by `num` (Q12, `ord` taps: `x[at + i] + sum num[m] *
/// x[at + i - 1 - m]`), rounded to 16 bits; `x` holds `ord` samples of
/// history before `at`.
pub(crate) fn fir(x: &[i32], at: usize, num: &[i32], y: &mut [i32], n: usize, ord: usize) {
    for i in 0..n {
        let mut sum = shl32(x[at + i], SIG_SHIFT);
        for j in 0..ord {
            sum = mac16_16(sum, num[ord - 1 - j], x[at + i + j - ord]);
        }
        y[i] = sround16(sum, SIG_SHIFT);
    }
}

/// `celt_iir`, in place: `buf[at..at + n]` through `1/A(z)` (`den` Q12,
/// `ord` taps), `mem` the last `ord` outputs before it (rounded, most
/// recent first). The outputs are kept 32-bit; the filter's own history
/// rounded to 16 bits.
///
/// This is libopus's unrolled filter, which keeps its history negated (as
/// 16-bit values, so that a remembered -32768 wraps as C's store does). Its
/// remainder loop for a length not a multiple of 4 forgets the negation;
/// libopus never takes it -- the concealment filters `N + overlap` samples,
/// a multiple of 4 at every frame size -- and this, needing no remainder
/// loop, has none.
pub(crate) fn iir_in_place(
    buf: &mut [i32],
    at: usize,
    den: &[i32],
    n: usize,
    ord: usize,
    mem: &[i32],
) {
    let mut y = vec![0i32; n + ord];
    for i in 0..ord {
        y[i] = extract16(mem[ord - i - 1].wrapping_neg());
    }
    for i in 0..n {
        let mut sum = buf[at + i];
        for j in 0..ord {
            sum = mac16_16(sum, den[ord - 1 - j], y[i + j]);
        }
        y[i + ord] = extract16(sround16(sum, SIG_SHIFT).wrapping_neg());
        buf[at + i] = sum;
    }
}

/// `celt_fir5`, in place: `x`'s first `n` values through the five-tap
/// filter `num` (Q12), rounded.
pub(crate) fn fir5(x: &mut [i32], num: &[i32; 5], n: usize) {
    let mut mem = [0i32; 5];
    for v in &mut x[..n] {
        let mut sum = shl32(*v, SIG_SHIFT);
        for (&k, &m) in num.iter().zip(&mem) {
            sum = mac16_16(sum, k, m);
        }
        mem.copy_within(0..4, 1);
        mem[0] = *v;
        *v = round16(sum, SIG_SHIFT);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn the_filters_invert_each_other() {
        // A signal filtered by A(z) and back through 1/A(z) is the signal
        // again, to within the FIR's rounding of the excitation to whole
        // samples (half a sample, which the IIR's own rounding can make
        // one).
        let den = [-4000, 2000, -500, 100];
        let ord = den.len();
        let signal: Vec<i32> = (0..64).map(|i| ((i * 37) % 200 - 100) * 50).collect();
        let mut padded = vec![0i32; ord];
        padded.extend(&signal);
        let mut exc = vec![0i32; signal.len()];
        fir(&padded, ord, &den, &mut exc, signal.len(), ord);
        let mut back: Vec<i32> = exc.iter().map(|&e| shl32(e, SIG_SHIFT)).collect();
        iir_in_place(&mut back, 0, &den, signal.len(), ord, &[0; 4]);
        for (i, (&b, &s)) in back.iter().zip(&signal).enumerate() {
            let b = sround16(b, SIG_SHIFT);
            assert!((b - s).abs() <= 1, "sample {i}: {b} for {s}");
        }
    }

    #[test]
    fn maxabs_takes_both_signs() {
        assert_eq!(maxabs16(&[3, -7, 5]), 7);
        assert_eq!(maxabs32(&[3, -7, 9]), 9);
        assert_eq!(maxabs32(&[]), 0);
    }
}

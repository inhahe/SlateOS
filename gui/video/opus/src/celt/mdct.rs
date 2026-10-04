//! The inverse MDCT that turns a CELT frame's coefficients back into
//! samples: pre-rotation, an FFT of a quarter the size, post-rotation, and
//! the windowed mirroring that cancels the previous frame's aliasing (TDAC).
//!
//! Translated into Rust from libopus 1.5.2's `celt/mdct.c`
//! (`clt_mdct_backward_c`, `FIXED_POINT`), copyright Xiph.Org, CSIRO and the
//! contributors named in its `COPYING`, used under libopus's BSD licence
//! (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "the transform's sizes are the mode's (1920 at most, halved by the shift), and every index is within the input of `n/2 * stride` coefficients, the output of `n/2 + overlap` samples, the window of `overlap`, and the twiddle table's slice for the shift -- as libopus's pointer walks are"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "indices as above; the sample arithmetic wraps where libopus's `_ovflw` macros say it may"
)]

use super::kiss_fft::{Cpx, FFT_STATES};
use super::tables::MDCT_TWIDDLES960;
use crate::fixed::mult16_32_q15;

/// The mode's MDCT size: 1920 (a 20 ms frame of 960 samples).
pub(crate) const MDCT_SIZE: usize = 1920;

/// `S_MUL`: a sample times a Q15 twiddle.
#[inline]
fn s_mul(a: i32, b: i32) -> i32 {
    mult16_32_q15(b, a)
}

/// `clt_mdct_backward`: the `n >> shift`-point inverse MDCT of `input` --
/// every `stride`th coefficient, interleaved short blocks' -- into `out`,
/// the first `overlap` samples overlapped by `window` with what `out` held.
/// `scratch` is the FFT's buffer, kept between calls.
pub(crate) fn backward(
    input: &[i32],
    out: &mut [i32],
    window: &[i16],
    overlap: usize,
    shift: usize,
    stride: usize,
    scratch: &mut Vec<Cpx>,
) {
    let mut n = MDCT_SIZE;
    let mut trig = 0usize;
    for _ in 0..shift {
        n >>= 1;
        trig += n;
    }
    let n2 = n >> 1;
    let n4 = n >> 2;
    let t = |k: usize| i32::from(MDCT_TWIDDLES960[trig + k]);
    let st = &FFT_STATES[shift];
    let base = overlap >> 1;

    // Pre-rotate, into the FFT's buffer in its bit-reversed order.
    scratch.clear();
    scratch.resize(n4, Cpx::default());
    let mut xp1 = 0usize;
    let mut xp2 = stride * (n2 - 1);
    for i in 0..n4 {
        let rev = st.bitrev(i);
        let yr = s_mul(input[xp2], t(i)).wrapping_add(s_mul(input[xp1], t(n4 + i)));
        let yi = s_mul(input[xp1], t(i)).wrapping_sub(s_mul(input[xp2], t(n4 + i)));
        // Real and imaginary swapped: an FFT doing an IFFT's work.
        scratch[rev] = Cpx { r: yi, i: yr };
        xp1 += 2 * stride;
        xp2 = xp2.wrapping_sub(2 * stride);
    }

    st.fft_impl(scratch);
    for (k, c) in scratch.iter().enumerate() {
        out[base + 2 * k] = c.r;
        out[base + 2 * k + 1] = c.i;
    }

    // Post-rotate and de-shuffle from both ends at once, in place.
    let mut yp0 = base;
    let mut yp1 = base + n2 - 2;
    for i in 0..(n4 + 1) >> 1 {
        let mut re = out[yp0 + 1];
        let mut im = out[yp0];
        let mut t0 = t(i);
        let mut t1 = t(n4 + i);
        let mut yr = s_mul(re, t0).wrapping_add(s_mul(im, t1));
        let mut yi = s_mul(re, t1).wrapping_sub(s_mul(im, t0));
        re = out[yp1 + 1];
        im = out[yp1];
        out[yp0] = yr;
        out[yp1 + 1] = yi;

        t0 = t(n4 - i - 1);
        t1 = t(n2 - i - 1);
        yr = s_mul(re, t0).wrapping_add(s_mul(im, t1));
        yi = s_mul(re, t1).wrapping_sub(s_mul(im, t0));
        out[yp1] = yr;
        out[yp0 + 1] = yi;
        yp0 += 2;
        yp1 = yp1.wrapping_sub(2);
    }

    // Mirror on both sides for TDAC.
    let mut xp1 = overlap.wrapping_sub(1);
    for (i, yp1) in (0..overlap / 2).enumerate() {
        let (w1, w2) = (i32::from(window[i]), i32::from(window[overlap - 1 - i]));
        let x1 = out[xp1];
        let x2 = out[yp1];
        out[yp1] = mult16_32_q15(w2, x2).wrapping_sub(mult16_32_q15(w1, x1));
        out[xp1] = mult16_32_q15(w1, x2).wrapping_add(mult16_32_q15(w2, x1));
        xp1 = xp1.wrapping_sub(1);
    }
}

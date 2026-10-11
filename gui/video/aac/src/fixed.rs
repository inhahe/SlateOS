//! The fixed-point arithmetic FFmpeg's decoder computes in: libavutil's
//! fixed-point DSP (`fixed_dsp.c`, its C versions -- the oracle's build has
//! no assembly), the integer square roots (`fixed_sqrt` in `fixed_dsp.h`,
//! `ff_sqrt` in libavcodec's `mathops.h`), and the decoder's Q-format
//! products (`aac_defines.h`).
//!
//! Samples are `i32`s holding fractions of 2^31 (Q31), 2^30 (Q30) and the
//! like; products are taken in 64 bits and rounded back, as FFmpeg rounds
//! them. Where FFmpeg's C would overflow a signed `int` -- undefined, and in
//! practice the machine's wrap -- the arithmetic here wraps.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "FFmpeg's (int) conversions of 64-bit products, and its int and unsigned \
              conversions, made where it makes them"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "shifts by counts under 32, and sums of products of 32-bit values, which fit 64 \
              bits; the rest wraps where it is written to"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "each function's slices are as long as its caller's lengths say, which the \
              callers' fixed-size buffers hold; the square root's tables are indexed by \
              values below their lengths"
)]

use crate::tables::{INVERSE, SQRT_TAB};

/// `(x * y + 2^25) >> 26` (`AAC_MUL26`).
pub(crate) fn mul26(x: i32, y: i32) -> i32 {
    ((i64::from(x) * i64::from(y) + 0x200_0000) >> 26) as i32
}

/// `(x * y + 2^29) >> 30` (`AAC_MUL30`).
pub(crate) fn mul30(x: i32, y: i32) -> i32 {
    ((i64::from(x) * i64::from(y) + 0x2000_0000) >> 30) as i32
}

/// `(x * y + 2^30) >> 31` (`AAC_MUL31`).
pub(crate) fn mul31(x: i32, y: i32) -> i32 {
    ((i64::from(x) * i64::from(y) + 0x4000_0000) >> 31) as i32
}

/// `dst[i] = src0[i] * src1[i]`, Q31 (`vector_fmul_c`), `dst` holding
/// `src0`'s place where they are one (see [`vector_fmul_in_place`]).
pub(crate) fn vector_fmul(dst: &mut [i32], src0: &[i32], src1: &[i32], len: usize) {
    for ((d, &a), &b) in dst[..len].iter_mut().zip(&src0[..len]).zip(&src1[..len]) {
        *d = mul31(a, b);
    }
}

/// [`vector_fmul`] with `dst` its own first source.
pub(crate) fn vector_fmul_in_place(dst: &mut [i32], src1: &[i32], len: usize) {
    for (d, &b) in dst[..len].iter_mut().zip(&src1[..len]) {
        *d = mul31(*d, b);
    }
}

/// `dst[i] = src0[i] * src1[len - 1 - i]`, Q31 (`vector_fmul_reverse_c`).
pub(crate) fn vector_fmul_reverse(dst: &mut [i32], src0: &[i32], src1: &[i32], len: usize) {
    for i in 0..len {
        dst[i] = mul31(src0[i], src1[len - 1 - i]);
    }
}

/// [`vector_fmul_reverse`] with `dst` its own first source.
pub(crate) fn vector_fmul_reverse_in_place(dst: &mut [i32], src1: &[i32], len: usize) {
    for i in 0..len {
        dst[i] = mul31(dst[i], src1[len - 1 - i]);
    }
}

/// `dst[i] = src2[i] + src0[i] * src1[i]`, the product Q31
/// (`vector_fmul_add_c`).
pub(crate) fn vector_fmul_add(
    dst: &mut [i32],
    src0: &[i32],
    src1: &[i32],
    src2: &[i32],
    len: usize,
) {
    for i in 0..len {
        dst[i] = src2[i].wrapping_add(mul31(src0[i], src1[i]));
    }
}

/// The overlap of two halves of windowed transforms (`vector_fmul_window_c`):
/// `2 * len` samples into `dst` from the `len` of `src0` (the last
/// transform's end) and of `src1` (this one's start), through the `2 * len`
/// of `win`, the first half of `dst` rising out of `src0` and the second
/// falling into `src1`, each Q31.
pub(crate) fn vector_fmul_window(
    dst: &mut [i32],
    src0: &[i32],
    src1: &[i32],
    win: &[i32],
    len: usize,
) {
    for k in 0..len {
        let (i, j) = (k, 2 * len - 1 - k);
        let s0 = i64::from(src0[k]);
        let s1 = i64::from(src1[len - 1 - k]);
        let (wi, wj) = (i64::from(win[i]), i64::from(win[j]));
        dst[i] = ((s0 * wj).wrapping_sub(s1 * wi).wrapping_add(0x4000_0000) >> 31) as i32;
        dst[j] = ((s0 * wi).wrapping_add(s1 * wj).wrapping_add(0x4000_0000) >> 31) as i32;
    }
}

/// The sum of `v1[i] * v2[i]`, Q31, rounded (`scalarproduct_fixed_c`).
pub(crate) fn scalarproduct_fixed(v1: &[i32], v2: &[i32], len: usize) -> i32 {
    let mut p: i64 = 0x4000_0000;
    for (&a, &b) in v1[..len].iter().zip(&v2[..len]) {
        p = p.wrapping_add(i64::from(a) * i64::from(b));
    }
    (p >> 31) as i32
}

/// `v1[i], v2[i] = v1[i] + v2[i], v1[i] - v2[i]` (`butterflies_fixed_c`).
pub(crate) fn butterflies_fixed(v1: &mut [i32], v2: &mut [i32], len: usize) {
    for (a, b) in v1[..len].iter_mut().zip(&mut v2[..len]) {
        let t = a.wrapping_sub(*b);
        *a = a.wrapping_add(*b);
        *b = t;
    }
}

/// `floor(log2(v))` of a 16-bit `v`, 0 for 0 (`av_log2_16bit`).
fn log2_16bit(v: u32) -> u32 {
    let v = v & 0xffff;
    if v == 0 { 0 } else { v.ilog2() }
}

/// The integer square root of `a`, rounded down (`ff_sqrt`): from
/// [`SQRT_TAB`], refined by a division through [`INVERSE`] for numbers of
/// 16 bits and more.
pub(crate) fn ff_sqrt(a: u32) -> u32 {
    let b = if a < 255 {
        return u32::from(SQRT_TAB[a as usize + 1]).wrapping_sub(1) >> 4;
    } else if a < 1 << 12 {
        u32::from(SQRT_TAB[(a >> 4) as usize]) >> 2
    } else if a < 1 << 14 {
        u32::from(SQRT_TAB[(a >> 6) as usize]) >> 1
    } else if a < 1 << 16 {
        u32::from(SQRT_TAB[(a >> 8) as usize])
    } else {
        let s = log2_16bit(a >> 16) >> 1;
        let c = a >> (s + 2);
        let b = u32::from(SQRT_TAB[(c >> (s + 8)) as usize]);
        // FASTDIV(c, b): c / b by a multiplication.
        let quotient = ((u64::from(c) * u64::from(INVERSE[b as usize])) >> 32) as u32;
        quotient.wrapping_add(b << s)
    };
    b.wrapping_sub(u32::from(a < b.wrapping_mul(b)))
}

/// The square root of `x`, a number with `bits` fraction bits, in the same
/// format (`fixed_sqrt`): [`ff_sqrt`] of `x` moved to 30 fraction bits,
/// refined a bit at a time below 15 bits.
pub(crate) fn fixed_sqrt(x: i32, bits: i32) -> i32 {
    let shift1 = 30 - bits;
    let shift2 = bits - 15;
    let mut retval = if shift1 > 0 {
        ff_sqrt(x.wrapping_shl(shift1 as u32) as u32) as i32
    } else {
        ff_sqrt((x >> -shift1) as u32) as i32
    };
    if shift2 > 0 {
        retval = retval.wrapping_shl(shift2 as u32);
        let mut bit_mask: i32 = 1 << (shift2 - 1);
        for _ in 0..shift2 {
            let guess = retval.wrapping_add(bit_mask);
            let accu = i64::from(guess) * i64::from(guess);
            let square = ((accu + i64::from(bit_mask)) >> bits) as i32;
            if x >= square {
                retval = retval.wrapping_add(bit_mask);
            }
            bit_mask >>= 1;
        }
    } else {
        retval >>= -shift2;
    }
    retval
}

#[cfg(test)]
mod tests;

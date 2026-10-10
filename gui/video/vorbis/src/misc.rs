//! Tremor's fixed-point arithmetic (`misc.h`, the 64-bit C path, not its
//! ARM assembly nor `_LOW_ACCURACY_`): the high halves of 64-bit products,
//! and Tremor's pseudo-float (`VFLOAT_*`, a mantissa and a power of two)
//! that floor 0 computes in. Sums wrap where C's signed overflow would.
//!
//! Translated into Rust from Tremor's `misc.h`, copyright Xiph.Org, used
//! under its BSD licence (`licenses/tremor-COPYING`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "each product is taken in 64 bits; the sums that can leave 32 wrap explicitly"
)]

/// `MULT32`: the high 32 bits of the 64-bit product.
#[inline]
pub(crate) const fn mult32(x: i32, y: i32) -> i32 {
    ((x as i64 * y as i64) >> 32) as i32
}

/// `MULT31`: [`mult32`] shifted left once (wrapping, as C's shift does).
#[inline]
pub(crate) const fn mult31(x: i32, y: i32) -> i32 {
    mult32(x, y).wrapping_shl(1)
}

/// `MULT31_SHIFT15`: the product shifted right by 15, its low 32 bits
/// (Tremor's `(lo >> 15) | (hi << 17)` of the product's halves, which is
/// that).
#[inline]
pub(crate) const fn mult31_shift15(x: i32, y: i32) -> i32 {
    ((x as i64 * y as i64) >> 15) as i32
}

/// `XPROD31`: `(a t + b v, b t - a v)`, each product [`mult31`].
#[inline]
pub(crate) const fn xprod31(a: i32, b: i32, t: i32, v: i32) -> (i32, i32) {
    (
        mult31(a, t).wrapping_add(mult31(b, v)),
        mult31(b, t).wrapping_sub(mult31(a, v)),
    )
}

/// `XNPROD31`: `(a t - b v, b t + a v)`, each product [`mult31`].
#[inline]
pub(crate) const fn xnprod31(a: i32, b: i32, t: i32, v: i32) -> (i32, i32) {
    (
        mult31(a, t).wrapping_sub(mult31(b, v)),
        mult31(b, t).wrapping_add(mult31(a, v)),
    )
}

/// `XPROD32`: as [`xprod31`] with [`mult32`].
#[inline]
pub(crate) const fn xprod32(a: i32, b: i32, t: i32, v: i32) -> (i32, i32) {
    (
        mult32(a, t).wrapping_add(mult32(b, v)),
        mult32(b, t).wrapping_sub(mult32(a, v)),
    )
}

/// `CLIP_TO_15`: held to 16 bits.
#[inline]
pub(crate) const fn clip_to_15(x: i32) -> i32 {
    if x > 32767 {
        32767
    } else if x < -32768 {
        -32768
    } else {
        x
    }
}

/// `_ilog`: the bits `v` needs (0 for 0).
#[inline]
pub(crate) const fn ilog(v: u32) -> i32 {
    (32 - v.leading_zeros()) as i32
}

/// `VFLOAT_MULT`: `a * 2^ap` times `b * 2^bp`, its power in `p`. A zero
/// product leaves `p` as it was, as C's does -- which matters: the codebook
/// unpacking takes the largest power of all its values, a zero's included.
#[inline]
pub(crate) const fn vfloat_mult(a: i32, ap: i32, b: i32, bp: i32, p: &mut i32) -> i32 {
    if a != 0 && b != 0 {
        *p = ap + bp + 32;
        mult32(a, b)
    } else {
        0
    }
}

/// `VFLOAT_MULTI`: `a * 2^ap` times the integer `i`, its power in `p`.
#[inline]
pub(crate) const fn vfloat_multi(a: i32, ap: i32, i: i32, p: &mut i32) -> i32 {
    let ip = ilog(i.unsigned_abs()) - 31;
    vfloat_mult(a, ap, i.wrapping_shl((-ip) as u32), ip, p)
}

/// `VFLOAT_ADD`: `a * 2^ap` plus `b * 2^bp`, renormalised, its power in
/// `p`.
pub(crate) const fn vfloat_add(a: i32, ap: i32, b: i32, bp: i32, p: &mut i32) -> i32 {
    if a == 0 {
        *p = bp;
        return b;
    } else if b == 0 {
        *p = ap;
        return a;
    }
    let (mut a, mut b) = (a, b);
    if ap > bp {
        let shift = ap - bp + 1;
        *p = ap + 1;
        a >>= 1;
        b = if shift < 32 {
            b.wrapping_add(1 << (shift - 1)) >> shift
        } else {
            0
        };
    } else {
        let shift = bp - ap + 1;
        *p = bp + 1;
        b >>= 1;
        a = if shift < 32 {
            a.wrapping_add(1 << (shift - 1)) >> shift
        } else {
            0
        };
    }
    a = a.wrapping_add(b);
    if (a as u32 & 0xc000_0000) == 0xc000_0000 || (a as u32 & 0xc000_0000) == 0 {
        a = a.wrapping_shl(1);
        *p -= 1;
    }
    a
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
    fn products_keep_the_high_half() {
        assert_eq!(mult32(0x4000_0000, 0x4000_0000), 0x1000_0000);
        assert_eq!(mult31(0x4000_0000, 0x4000_0000), 0x2000_0000);
        assert_eq!(mult32(-0x4000_0000, 0x4000_0000), -0x1000_0000);
        // The high half of a negative product rounds toward minus infinity.
        assert_eq!(mult32(-1, 1), -1);
        assert_eq!(mult31_shift15(1 << 20, 1 << 20), 1 << 25);
        assert_eq!(xprod31(1 << 30, 0, 1 << 30, 0), (1 << 29, 0));
        assert_eq!(xnprod31(0, 1 << 30, 0, 1 << 30), (-(1 << 29), 0));
    }

    #[test]
    fn clipping_holds_sixteen_bits() {
        assert_eq!(clip_to_15(40000), 32767);
        assert_eq!(clip_to_15(-40000), -32768);
        assert_eq!(clip_to_15(-32768), -32768);
        assert_eq!(clip_to_15(12), 12);
    }

    #[test]
    fn pseudo_floats_multiply_and_add() {
        // 1.0 times 3: 3 is normalised to 3 << 29 at -29, and the product
        // keeps the high half, 3 << 27 at -27.
        let mut p = 0;
        let three = vfloat_multi(1 << 30, -30, 3, &mut p);
        assert_eq!((three, p), (3 << 27, -27));
        // 3 + 1 = 4, renormalised once: 1 << 29 at -27.
        let mut q = 0;
        let sum = vfloat_add(three, p, 1 << 30, -30, &mut q);
        assert_eq!((sum, q), (1 << 29, -27));
        // A zero product leaves the power alone.
        let mut keep = 77;
        assert_eq!(vfloat_mult(0, 5, 9, 9, &mut keep), 0);
        assert_eq!(keep, 77);
        assert_eq!(vfloat_add(0, 1, 5, 3, &mut keep), 5);
        assert_eq!(keep, 3);
    }
}

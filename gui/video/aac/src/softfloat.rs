//! libavutil's soft-float (`softfloat.h`): numbers as a 32-bit mantissa and
//! an exponent, computed with integers only, so that AAC Main's prediction,
//! SBR and parametric stereo give the same results on every machine.
//!
//! A number is `mant * 2^(exp - 30)`; normalized, the mantissa's magnitude
//! is from `0x20000000` to just under `0x40000000`, and zero is a zero
//! mantissa with the smallest exponent ([`FLOAT_0`]). The functions are
//! FFmpeg's, rounding as they round; where FFmpeg's C would be undefined or
//! abort -- a division by zero, the root of a negative number -- they
//! answer something rather than panic, as FFmpeg's decoder never gets
//! there.

#![allow(
    clippy::cast_possible_truncation,
    reason = "FFmpeg's (int) conversions of 64-bit products, made where it makes them"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "exponents from -149 to 126 and their sums; mantissa sums of normalized \
              operands, which fit; the rest wraps where it is written to"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "the sine and cosine tables are indexed by angles masked to their lengths, and \
              the square root's factors by an exponent's lowest bit"
)]

use crate::tables::{
    COSTBL_1_SF, COSTBL_2_SF, COSTBL_3_SF, COSTBL_4_SF, SINTBL_2_SF, SINTBL_3_SF, SINTBL_4_SF,
    SQR_EXP_MULTBL_SF, SQRTTBL_SF,
};

/// The smallest exponent; smaller numbers are zero (`MIN_EXP`).
pub(crate) const MIN_EXP: i32 = -149;
/// The fraction bits of 1.0's mantissa less one (`ONE_BITS`).
const ONE_BITS: i32 = 29;

/// A soft-float number (`SoftFloat`). The default is all zero bits, as
/// FFmpeg's zeroed allocations hold -- zero, though not normalized as
/// [`FLOAT_0`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SoftFloat {
    pub(crate) mant: i32,
    pub(crate) exp: i32,
}

const fn sf(mant: i32, exp: i32) -> SoftFloat {
    SoftFloat { mant, exp }
}

/// 0, 0.5, 1, a small value (about 1e-5), 10^0.2, 100000, 0.999999, and
/// the smallest normalized number (`FLOAT_*`).
pub(crate) const FLOAT_0: SoftFloat = sf(0, MIN_EXP);
pub(crate) const FLOAT_05: SoftFloat = sf(0x2000_0000, 0);
pub(crate) const FLOAT_1: SoftFloat = sf(0x2000_0000, 1);
pub(crate) const FLOAT_EPSILON: SoftFloat = sf(0x29F1_6B12, -16);
pub(crate) const FLOAT_1584893192: SoftFloat = sf(0x32B7_71ED, 1);
pub(crate) const FLOAT_100000: SoftFloat = sf(0x30D4_0000, 17);
pub(crate) const FLOAT_0999999: SoftFloat = sf(0x3FFF_FBCE, 0);
pub(crate) const FLOAT_MIN: SoftFloat = sf(0x2000_0000, MIN_EXP);

/// `a` with its mantissa shifted up into the normalized range; zero, or
/// anything below the smallest exponent, as [`FLOAT_0`]
/// (`av_normalize_sf`).
pub(crate) fn normalize_sf(mut a: SoftFloat) -> SoftFloat {
    if a.mant != 0 {
        while (a.mant as u32).wrapping_add(0x1FFF_FFFF) < 0x3FFF_FFFF {
            a.mant = a.mant.wrapping_add(a.mant);
            a.exp -= 1;
        }
        if a.exp < MIN_EXP {
            a.exp = MIN_EXP;
            a.mant = 0;
        }
    } else {
        a.exp = MIN_EXP;
    }
    a
}

/// `a` with a mantissa one bit too big shifted down by one
/// (`av_normalize1_sf`): what an addition or a product of normalized
/// numbers needs.
pub(crate) fn normalize1_sf(mut a: SoftFloat) -> SoftFloat {
    if (a.mant as u32).wrapping_add(0x4000_0000) as i32 <= 0 {
        a.exp += 1;
        a.mant >>= 1;
    }
    a
}

/// `a * b` (`av_mul_sf`): normalized if both are.
pub(crate) fn mul_sf(mut a: SoftFloat, b: SoftFloat) -> SoftFloat {
    a.exp += b.exp;
    a.mant = ((i64::from(a.mant) * i64::from(b.mant)) >> ONE_BITS) as i32;
    a = normalize1_sf(sf(a.mant, a.exp - 1));
    if a.mant == 0 || a.exp < MIN_EXP {
        return FLOAT_0;
    }
    a
}

/// `a / b` (`av_div_sf`), `b` normalized and not zero. A zero `b`, which
/// FFmpeg's C divides by, gives [`FLOAT_0`].
pub(crate) fn div_sf(mut a: SoftFloat, b: SoftFloat) -> SoftFloat {
    let Some(mut temp) = (i64::from(a.mant) * (1 << (ONE_BITS + 1))).checked_div(i64::from(b.mant))
    else {
        return FLOAT_0;
    };
    a.exp -= b.exp;
    a.mant = temp as i32;
    while i64::from(a.mant) != temp {
        temp /= 2;
        a.exp -= 1;
        a.mant = temp as i32;
    }
    a = normalize1_sf(a);
    if a.mant == 0 || a.exp < MIN_EXP {
        return FLOAT_0;
    }
    a
}

/// Below zero if `a < b`, above if `a > b`, zero if equal (`av_cmp_sf`).
pub(crate) fn cmp_sf(a: SoftFloat, b: SoftFloat) -> i32 {
    let t = a.exp - b.exp;
    if t < -31 {
        b.mant.wrapping_neg()
    } else if t < 0 {
        (a.mant >> -t).wrapping_sub(b.mant)
    } else if t < 32 {
        a.mant.wrapping_sub(b.mant >> t)
    } else {
        a.mant
    }
}

/// Whether `a > b` (`av_gt_sf`).
pub(crate) fn gt_sf(a: SoftFloat, b: SoftFloat) -> bool {
    let t = a.exp - b.exp;
    if t < -31 {
        0 > b.mant
    } else if t < 0 {
        (a.mant >> -t) > b.mant
    } else if t < 32 {
        a.mant > (b.mant >> t)
    } else {
        a.mant > 0
    }
}

/// `a + b` (`av_add_sf`).
pub(crate) fn add_sf(a: SoftFloat, b: SoftFloat) -> SoftFloat {
    let t = a.exp - b.exp;
    if t < -31 {
        b
    } else if t < 0 {
        normalize_sf(normalize1_sf(sf(b.mant.wrapping_add(a.mant >> -t), b.exp)))
    } else if t < 32 {
        normalize_sf(normalize1_sf(sf(a.mant.wrapping_add(b.mant >> t), a.exp)))
    } else {
        a
    }
}

/// `a - b` (`av_sub_sf`).
pub(crate) fn sub_sf(a: SoftFloat, b: SoftFloat) -> SoftFloat {
    add_sf(a, sf(b.mant.wrapping_neg(), b.exp))
}

/// `v * 2^-frac_bits`, normalized (`av_int2sf`).
pub(crate) fn int2sf(mut v: i32, frac_bits: i32) -> SoftFloat {
    let mut exp_offset = 0;
    if v <= i32::MIN + 1 {
        exp_offset = 1;
        v >>= 1;
    }
    normalize_sf(normalize1_sf(sf(v, ONE_BITS + 1 - frac_bits + exp_offset)))
}

/// The square root of `val`, rounded to nearest (`av_sqrt_sf`); `val`
/// normalized and not negative. A negative `val`, on which FFmpeg aborts,
/// gives [`FLOAT_0`].
pub(crate) fn sqrt_sf(mut val: SoftFloat) -> SoftFloat {
    if val.mant == 0 {
        val.exp = MIN_EXP;
        return val;
    }
    if val.mant < 0 {
        return FLOAT_0;
    }
    let index = usize::try_from((val.mant - 0x2000_0000) >> 20).unwrap_or(0);
    let rem = i64::from(val.mant & 0xF_FFFF);
    let low = SQRTTBL_SF.get(index).map_or(0, |&v| i64::from(v));
    let high = SQRTTBL_SF.get(index + 1).map_or(0, |&v| i64::from(v));
    val.mant = ((low * (0x10_0000 - rem) + high * rem + 0x8_0000) >> 20) as i32;
    let factor = i64::from(SQR_EXP_MULTBL_SF[usize::from(val.exp & 1 != 0)]);
    val.mant = ((factor * i64::from(val.mant) + 0x1000_0000) >> 29) as i32;
    if val.mant < 0x4000_0000 {
        val.exp -= 2;
    } else {
        val.mant >>= 1;
    }
    val.exp = (val.exp >> 1) + 1;
    val
}

/// `(cv * ct - sv * st, cv * st + sv * ct)`, Q30, rounded: a rotation of
/// the angle `(cv, sv)` by `(ct, st)`.
fn rotate(cv: i32, sv: i32, ct: i32, st: i32) -> (i32, i32) {
    let (cv, sv, ct, st) = (i64::from(cv), i64::from(sv), i64::from(ct), i64::from(st));
    (
        ((cv * ct - sv * st + 0x2000_0000) >> 30) as i32,
        ((cv * st + sv * ct + 0x2000_0000) >> 30) as i32,
    )
}

/// The sine and cosine of `a`, in units of `2^30 / pi` radians, both Q30,
/// rounded to nearest (`av_sincos_sf`): `(sin, cos)`.
#[allow(
    clippy::cast_sign_loss,
    reason = "indices masked to the tables' lengths"
)]
pub(crate) fn sincos_sf(a: i32) -> (i32, i32) {
    let mut idx = a >> 26;
    let mut sign = ((idx as u32) << 27) as i32 >> 31;
    let mut cv = COSTBL_1_SF[(idx & 0xf) as usize];
    cv = (cv ^ sign).wrapping_sub(sign);

    idx -= 8;
    sign = ((idx as u32) << 27) as i32 >> 31;
    let mut sv = COSTBL_1_SF[(idx & 0xf) as usize];
    sv = (sv ^ sign).wrapping_sub(sign);

    idx = (a >> 21) & 0x1f;
    (cv, sv) = rotate(cv, sv, COSTBL_2_SF[idx as usize], SINTBL_2_SF[idx as usize]);

    idx = (a >> 16) & 0x1f;
    (cv, sv) = rotate(cv, sv, COSTBL_3_SF[idx as usize], SINTBL_3_SF[idx as usize]);

    let i = ((a >> 11) & 0x1f) as usize;
    let frac = i64::from(a & 0x7ff);
    let ct = ((i64::from(COSTBL_4_SF[i]) * (0x800 - frac)
        + i64::from(COSTBL_4_SF[i + 1]) * frac
        + 0x400)
        >> 11) as i32;
    let st = ((i64::from(SINTBL_4_SF[i]) * (0x800 - frac)
        + i64::from(SINTBL_4_SF[i + 1]) * frac
        + 0x400)
        >> 11) as i32;
    let (c, s) = rotate(cv, sv, ct, st);
    (s, c)
}

#[cfg(test)]
mod tests;

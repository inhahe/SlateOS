//! FreeType's fixed-point arithmetic (`src/base/ftcalc.c` and the conversion
//! macros of `ttgxvar.c`), rounding included.
//!
//! Wherever this crate reproduces a FreeType computation to the bit -- the
//! auto-hinter ([`crate::hint`]) and the points FreeType's TrueType loader
//! hands it at a variable instance ([`crate::gvar`], [`crate::var`]) -- it does
//! the arithmetic through these, because a port that rounds differently makes
//! different decisions: a delta of 10.5 units becomes 11 in one and 10 in the
//! other, and a hinted stem lands a pixel away.
//!
//! The values are `i64` as FreeType's `FT_Long` is on the 64-bit builds these
//! are checked against, and every product and shifted numerator is formed in
//! `i128`, so no input can overflow: for every input FreeType's own arithmetic
//! handles without overflow the answers are FreeType's, and for the rest --
//! which no sane font reaches -- they are the mathematically right ones
//! narrowed back, rather than a panic.
//!
//! Portions of this file are copyright (C) 1996-2023 by David Turner, Robert
//! Wilhelm and Werner Lemberg (FreeType 2.13.2's `src/base/ftcalc.c`), and
//! copyright (C) 2004-2023 by David Turner, Robert Wilhelm, Werner Lemberg and
//! George Williams (its `src/truetype/ttgxvar.c`), from The FreeType Project
//! (www.freetype.org). Used under the FreeType License: see
//! `gui/font/licenses/FTL.TXT`.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "every product and shifted numerator is formed in 128 bits from \
              64-bit operands, so none can overflow; the divisions are by a \
              divisor checked non-zero, and the negations are of magnitudes \
              below 2^127"
)]

/// `a * b` for a 16.16 `b`, rounded half away from zero: `FT_MulFix`.
pub(crate) fn mul_fix(a: i64, b: i64) -> i64 {
    let ab = i128::from(a) * i128::from(b);
    narrow((ab + 0x8000 - i128::from(ab < 0)) >> 16)
}

/// `a / b` as 16.16, rounded half away from zero: `FT_DivFix`. A zero `b`
/// gives FreeType's answer, the largest 32-bit value with the sign of `a`.
pub(crate) fn div_fix(a: i64, b: i64) -> i64 {
    let negative = (a < 0) != (b < 0);
    let (a, b) = (u128::from(a.unsigned_abs()), u128::from(b.unsigned_abs()));
    let q = if b > 0 {
        ((a << 16) + (b >> 1)) / b
    } else {
        0x7FFF_FFFF
    };
    let q = i128::try_from(q).unwrap_or(i128::MAX);
    narrow(if negative { -q } else { q })
}

/// `a * b / c`, rounded half away from zero: `FT_MulDiv`. A zero `c` gives
/// FreeType's answer, as [`div_fix`] does.
pub(crate) fn mul_div(a: i64, b: i64, c: i64) -> i64 {
    let negative = ((a < 0) != (b < 0)) != (c < 0);
    let (a, b, c) = (
        u128::from(a.unsigned_abs()),
        u128::from(b.unsigned_abs()),
        u128::from(c.unsigned_abs()),
    );
    let d = if c > 0 {
        (a * b + (c >> 1)) / c
    } else {
        0x7FFF_FFFF
    };
    let d = i128::try_from(d).unwrap_or(i128::MAX);
    narrow(if negative { -d } else { d })
}

/// A 16.16 value rounded to a whole number, halves up, and cut to 16 bits as
/// FreeType stores a font unit: `FT_fixedToInt`, how `TT_Vary_Apply_Glyph_Deltas`
/// turns a point's summed delta into the units it moves.
pub(crate) fn fixed_to_int(x: i64) -> i64 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the cut to 16 bits is FreeType's own `(FT_Short)` cast"
    )]
    let r = ((i128::from(x) + 0x8000) >> 16) as i16;
    i64::from(r)
}

/// An `F2Dot14` as 16.16: `FT_fdot14ToFixed`.
pub(crate) fn f2dot14_to_fixed(x: i16) -> i64 {
    i64::from(x) << 2
}

/// The length of `(x, y)`, 16.16, by FreeType's CORDIC: `FT_Hypot`
/// (`FT_Vector_Length`), which its TrueType loader uses to scale a
/// component's offset. Not the true length -- CORDIC's -- and so exactly
/// FreeType's.
pub(crate) fn vector_length(x: i64, y: i64) -> i64 {
    /// The CORDIC gain's reciprocal, 0.858785336480436 * 2^32.
    const TRIG_SCALE: u128 = 0xDBD9_5B16;
    /// The highest bit a component may have and not overflow.
    const TRIG_SAFE_MSB: i32 = 29;
    if x == 0 {
        return y.saturating_abs();
    }
    if y == 0 {
        return x.saturating_abs();
    }
    // `ft_trig_prenorm`: to 29 significant bits.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "`FT_MSB` of the 32-bit `(FT_UInt32)(|x| | |y|)` FreeType takes"
    )]
    let bits = (x.unsigned_abs() | y.unsigned_abs()) as u32;
    let msb = 31_i32 - i32::try_from(bits.leading_zeros()).unwrap_or(31);
    let (mut vx, mut vy, shift) = if msb <= TRIG_SAFE_MSB {
        let s = TRIG_SAFE_MSB - msb;
        (x << s, y << s, s)
    } else {
        let s = msb - TRIG_SAFE_MSB;
        (x >> s, y >> s, -s)
    };
    // `ft_trig_pseudo_polarize`, for the magnitude only: the angle it also
    // accumulates never feeds back into it.
    if vy > vx {
        if vy > -vx {
            (vx, vy) = (vy, -vx);
        } else {
            (vx, vy) = (-vx, -vy);
        }
    } else if vy < -vx {
        (vx, vy) = (-vy, vx);
    }
    let mut b = 1_i64;
    for i in 1..23 {
        if vy > 0 {
            let t = vx + ((vy + b) >> i);
            vy -= (vx + b) >> i;
            vx = t;
        } else {
            let t = vx - ((vy + b) >> i);
            vy += (vx + b) >> i;
            vx = t;
        }
        b <<= 1;
    }
    // `ft_trig_downscale` (the 64-bit form).
    let negative = vx < 0;
    let m = (u128::from(vx.unsigned_abs()) * TRIG_SCALE + 0x4000_0000) >> 32;
    let m = narrow(i128::try_from(m).unwrap_or(i128::MAX));
    let v = if negative { -m } else { m };
    if shift > 0 {
        (v + (1_i64 << (shift - 1))) >> shift
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "FreeType's `(FT_UInt32)v.x << -shift`"
        )]
        let r = ((v as u32) << (-shift)) as i64;
        r
    }
}

/// Back to `i64`, saturating: reached only by inputs no font produces.
fn narrow(v: i128) -> i64 {
    i64::try_from(v).unwrap_or(if v < 0 { i64::MIN } else { i64::MAX })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_fix_rounds_half_away_from_zero() {
        // 1.5 * 0.5 = 0.75 exactly; 3 * 0.5 = 1.5 rounds to 2, and -3 * 0.5
        // to -2, not -1.
        assert_eq!(mul_fix(3, 0x8000), 2);
        assert_eq!(mul_fix(-3, 0x8000), -2);
        assert_eq!(mul_fix(64, 0x10000), 64);
        // 1000 units at 16 px on a 2048 em: FreeType's y_scale is 32768.
        assert_eq!(mul_fix(1000, div_fix(16 * 64, 2048)), 500);
        // Past 64 bits in the product, still the right answer.
        assert_eq!(mul_fix(1 << 50, 1 << 20), 1 << 54);
    }

    #[test]
    fn div_fix_and_mul_div_round_the_magnitude_and_keep_the_sign() {
        assert_eq!(div_fix(1, 3), 21845);
        assert_eq!(div_fix(-1, 3), -21845);
        assert_eq!(div_fix(2, 3), 43691);
        assert_eq!(div_fix(5, 0), 0x7FFF_FFFF);
        assert_eq!(div_fix(-5, 0), -0x7FFF_FFFF);
        assert_eq!(mul_div(7, 3, 2), 11);
        assert_eq!(mul_div(-7, 3, 2), -11);
        assert_eq!(mul_div(7, -3, -2), 11);
    }

    #[test]
    fn fixed_to_int_rounds_halves_up_and_keeps_sixteen_bits() {
        assert_eq!(fixed_to_int(0x8000), 1);
        assert_eq!(fixed_to_int(-0x8000), 0);
        assert_eq!(fixed_to_int(-0x8001), -1);
        assert_eq!(fixed_to_int(10 << 16 | 0x7FFF), 10);
        assert_eq!(f2dot14_to_fixed(-16384), -0x10000);
    }

    #[test]
    fn vector_length_is_freetypes_cordic_to_the_last_bit() {
        // Each answer as FreeType 2.13.2's own `FT_Vector_Length` gave it.
        assert_eq!(vector_length(3 << 16, 4 << 16), 327_680);
        assert_eq!(vector_length(-3 << 16, 4 << 16), 327_680);
        assert_eq!(vector_length(0x10000, 0x10000), 92_682);
        assert_eq!(vector_length(0x10000, 0x8000), 73_271);
        assert_eq!(vector_length(12_345, -67_890), 69_003);
        assert_eq!(vector_length(0x10000, 0), 0x10000);
        assert_eq!(vector_length(0, -5), 5);
    }
}

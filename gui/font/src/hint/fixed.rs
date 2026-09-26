//! FreeType's fixed-point arithmetic, rounding included.
//!
//! The hinter keeps positions as FreeType does: font units as integers,
//! scaled positions in 26.6 (1/64 pixel), scales in 16.16. Each operation
//! rounds the way FreeType's does (`FT_MulFix`, `FT_DivFix` and `FT_MulDiv` in
//! `src/base/ftcalc.c`, and the pixel macros in `ftobjs.h`), because a hinter
//! is a chain of decisions -- does this edge land on this pixel or the next --
//! that turn on the last bit, and the port is only checkable against FreeType
//! if it makes the same ones.
//!
//! # Why the arithmetic here is not checked
//!
//! Every value that reaches these functions is bounded where it enters the
//! hinter: font units within `i16` (FreeType stores them as `FT_Short`, and
//! [`super::glyph::Hints::load`] refuses a glyph with any coordinate beyond
//! it) and scales within [`MAX_SCALE`] ([`super::Hinter::new`] refuses more).
//! The largest product anywhere is then a 26.6 position times a 16.16 scale,
//! under 2^24 * 2^26, which is nowhere near `i64`'s range -- so the operators
//! cannot overflow, and checked arithmetic would only obscure a port whose
//! value is being line-for-line comparable with its C.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "bounded operands; see the module documentation"
)]

/// The largest 16.16 scale the hinter accepts: 2^26, 1024 pixels per font
/// unit. Real text is thousands of times smaller; hinting at such sizes would
/// be pointless anyway, and the bound is what keeps the products above in
/// range.
pub(super) const MAX_SCALE: i64 = 1 << 26;

/// `a * b` for a 16.16 `b`, rounded half away from zero: `FT_MulFix`.
pub(super) fn mul_fix(a: i64, b: i64) -> i64 {
    let ab = a * b;
    (ab + 0x8000 - i64::from(ab < 0)) >> 16
}

/// `a / b` as 16.16, rounded half away from zero: `FT_DivFix`. A zero `b`
/// gives FreeType's answer, the largest 32-bit value with the sign of `a`.
pub(super) fn div_fix(a: i64, b: i64) -> i64 {
    let negative = (a < 0) != (b < 0);
    let (a, b) = (a.unsigned_abs(), b.unsigned_abs());
    let q = if b > 0 {
        ((a << 16) + (b >> 1)) / b
    } else {
        0x7FFF_FFFF
    };
    let q = i64::try_from(q).unwrap_or(i64::MAX);
    if negative { -q } else { q }
}

/// `a * b / c`, rounded half away from zero: `FT_MulDiv`. A zero `c` gives
/// FreeType's answer, as [`div_fix`] does.
pub(super) fn mul_div(a: i64, b: i64, c: i64) -> i64 {
    let negative = ((a < 0) != (b < 0)) != (c < 0);
    let (a, b, c) = (a.unsigned_abs(), b.unsigned_abs(), c.unsigned_abs());
    let d = if c > 0 {
        (a * b + (c >> 1)) / c
    } else {
        0x7FFF_FFFF
    };
    let d = i64::try_from(d).unwrap_or(i64::MAX);
    if negative { -d } else { d }
}

/// The pixel boundary at or below `x` (26.6): `FT_PIX_FLOOR`.
pub(super) const fn pix_floor(x: i64) -> i64 {
    x & !63
}

/// The nearest pixel boundary to `x` (26.6), halves up: `FT_PIX_ROUND`.
pub(super) const fn pix_round(x: i64) -> i64 {
    (x + 32) & !63
}

/// `sqrt(x^2 + y^2)` within 7%, as FreeType estimates it: `FT_HYPOT`.
pub(super) fn hypot(x: i64, y: i64) -> i64 {
    let (x, y) = (x.abs(), y.abs());
    if x > y {
        x + ((3 * y) >> 3)
    } else {
        y + ((3 * x) >> 3)
    }
}

/// Whether the corner between vectors `in` and `out` is nearly straight --
/// the two together not much longer than the chord across them:
/// `ft_corner_is_flat`.
pub(super) fn corner_is_flat(in_x: i64, in_y: i64, out_x: i64, out_y: i64) -> bool {
    let (ax, ay) = (in_x + out_x, in_y + out_y);
    let d_in = hypot(in_x, in_y);
    let d_out = hypot(out_x, out_y);
    let d_hypot = hypot(ax, ay);
    d_in + d_out - d_hypot < d_hypot >> 4
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
    fn pixel_rounding_floors_toward_minus_infinity() {
        assert_eq!(pix_round(31), 0);
        assert_eq!(pix_round(32), 64);
        assert_eq!(pix_round(-32), 0);
        assert_eq!(pix_round(-33), -64);
        assert_eq!(pix_floor(-1), -64);
    }

    #[test]
    fn a_straight_run_is_flat_and_a_right_angle_is_not() {
        assert!(corner_is_flat(100, 0, 100, 1));
        assert!(!corner_is_flat(100, 0, 0, 100));
        // A tiny step off a long run is dominated by the run.
        assert!(corner_is_flat(1000, 0, 3, 40));
    }
}

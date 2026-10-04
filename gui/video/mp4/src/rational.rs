//! FFmpeg's rational numbers (`libavutil/rational.c`), as `mov.c` uses them
//! for a pixel's shape and a picture's crop: [`reduce`] (`av_reduce`), the
//! best fraction with both terms within a limit; [`d2q`] (`av_d2q`), a
//! double as such a fraction; and the arithmetic on them. Each wraps where
//! FFmpeg's C wraps, so that a hostile file comes out as it does there.

/// A fraction, `num / den`, as FFmpeg's `AVRational` holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Q {
    pub num: i32,
    pub den: i32,
}

/// FFmpeg's `INT_MAX`, the limit every reduction here is made to.
pub(crate) const MAX: i64 = i32::MAX as i64;

#[allow(
    clippy::arithmetic_side_effects,
    reason = "the remainder is taken only by a divisor checked non-zero"
)]
const fn gcd(a: i64, b: i64) -> i64 {
    // `av_gcd`: Euclid's, on magnitudes FFmpeg has already taken.
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = b;
        b = a.wrapping_rem(b);
        a = t;
    }
    a
}

/// `av_reduce`: `num / den` as the nearest fraction whose terms are both at
/// most `max` -- exactly, when the fraction in lowest terms fits -- with
/// the sign on the numerator. The second value says whether it is exact.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "C's conversions, each where `av_reduce` makes it: an unsigned quotient, the terms back to int once within `max`"
)]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "every divisor is checked non-zero first (the gcd, the loop's `den`, the terms tested before the division); the rest wraps as C's does"
)]
pub(crate) fn reduce(num: i64, den: i64, max: i64) -> (Q, bool) {
    let (mut a0n, mut a0d): (i64, i64) = (0, 1);
    let (mut a1n, mut a1d): (i64, i64) = (1, 0);
    let sign = (num < 0) ^ (den < 0);
    let g = gcd(num.wrapping_abs(), den.wrapping_abs());
    let (mut num, mut den) = if g == 0 {
        (num, den)
    } else {
        (
            num.wrapping_abs().wrapping_div(g),
            den.wrapping_abs().wrapping_div(g),
        )
    };
    if num <= max && den <= max {
        (a1n, a1d) = (num, den);
        den = 0;
    }
    while den != 0 {
        // `uint64_t x = num / den`, and the sums in unsigned arithmetic.
        let mut x = num.wrapping_div(den) as u64;
        let next_den = num.wrapping_sub(den.wrapping_mul(x as i64));
        let a2n = x.wrapping_mul(a1n as u64).wrapping_add(a0n as u64) as i64;
        let a2d = x.wrapping_mul(a1d as u64).wrapping_add(a0d as u64) as i64;
        if a2n > max || a2d > max {
            if a1n != 0 {
                x = (max.wrapping_sub(a0n)).wrapping_div(a1n) as u64;
            }
            if a1d != 0 {
                x = x.min((max.wrapping_sub(a0d)).wrapping_div(a1d) as u64);
            }
            let left = (den as u64).wrapping_mul(
                2u64.wrapping_mul(x)
                    .wrapping_mul(a1d as u64)
                    .wrapping_add(a0d as u64),
            );
            let right = num.wrapping_mul(a1d) as u64;
            if left > right {
                a1n = x.wrapping_mul(a1n as u64).wrapping_add(a0n as u64) as i64;
                a1d = x.wrapping_mul(a1d as u64).wrapping_add(a0d as u64) as i64;
            }
            break;
        }
        (a0n, a0d) = (a1n, a1d);
        (a1n, a1d) = (a2n, a2d);
        num = den;
        den = next_den;
    }
    let n = a1n as i32;
    (
        Q {
            num: if sign { n.wrapping_neg() } else { n },
            den: a1d as i32,
        },
        den == 0,
    )
}

/// `frexp`'s exponent: `d = m * 2^e` with `0.5 <= |m| < 1`; 0 for zero,
/// and for what is not finite (which [`d2q`] has turned away already).
#[allow(
    clippy::arithmetic_side_effects,
    reason = "an 11-bit exponent less a constant, and a subnormal's at most 64 below the least normal one"
)]
fn exponent(d: f64) -> i32 {
    if d == 0.0 || !d.is_finite() {
        return 0;
    }
    let bits = d.to_bits();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "an 11-bit field"
    )]
    let biased = ((bits >> 52) & 0x7ff) as i32;
    if biased == 0 {
        // A subnormal: scaled into the normal range first.
        return exponent(d * 2f64.powi(64)) - 64;
    }
    biased - 1022
}

/// `av_d2q`: `d` as the nearest fraction with terms at most `max`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "C's conversions: a double of at most 2^62 to int64, and back"
)]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "within the bound above, frexp's exponent is at most 32: the shift is 31 to 62"
)]
pub(crate) fn d2q(d: f64, max: i32) -> Q {
    if d.is_nan() {
        return Q { num: 0, den: 0 };
    }
    if d.abs() > f64::from(i32::MAX) + 3.0 {
        return Q {
            num: if d < 0.0 { -1 } else { 1 },
            den: 0,
        };
    }
    let e = (exponent(d) - 1).max(0);
    let den: i64 = 1 << (62 - e);
    reduce((d * den as f64 + 0.5).floor() as i64, den, i64::from(max)).0
}

/// `av_mul_q`.
pub(crate) fn mul(b: Q, c: Q) -> Q {
    reduce(
        i64::from(b.num).wrapping_mul(i64::from(c.num)),
        i64::from(b.den).wrapping_mul(i64::from(c.den)),
        MAX,
    )
    .0
}

/// `av_add_q`.
pub(crate) fn add(b: Q, c: Q) -> Q {
    reduce(
        i64::from(b.num)
            .wrapping_mul(i64::from(c.den))
            .wrapping_add(i64::from(c.num).wrapping_mul(i64::from(b.den))),
        i64::from(b.den).wrapping_mul(i64::from(c.den)),
        MAX,
    )
    .0
}

/// `av_sub_q`.
pub(crate) fn sub(b: Q, c: Q) -> Q {
    add(
        b,
        Q {
            num: c.num.wrapping_neg(),
            den: c.den,
        },
    )
}

/// `av_cmp_q`: -1, 0 or 1 as `a` is less than, equal to or more than `b`;
/// `i32::MIN` when either is 0/0.
pub(crate) fn cmp(a: Q, b: Q) -> i32 {
    let tmp = i64::from(a.num)
        .wrapping_mul(i64::from(b.den))
        .wrapping_sub(i64::from(b.num).wrapping_mul(i64::from(a.den)));
    if tmp != 0 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the sign bit, shifted down to 0 or -1"
        )]
        let sign = ((tmp ^ i64::from(a.den) ^ i64::from(b.den)) >> 63) as i32;
        sign | 1
    } else if b.den != 0 && a.den != 0 {
        0
    } else if a.num != 0 && b.num != 0 {
        (a.num >> 31).wrapping_sub(b.num >> 31)
    } else {
        i32::MIN
    }
}

/// `av_q2d`.
pub(crate) fn q2d(a: Q) -> f64 {
    f64::from(a.num) / f64::from(a.den)
}

/// A double converted to `uint64_t` as GCC compiles it for x86-64 (the
/// compiler FFmpeg's Windows builds use): below 2^63 a signed conversion,
/// so a negative value wraps; from 2^63 the signed conversion of what is
/// over it, with the top bit flipped back; and the signed conversion's
/// "indefinite" 2^63 for NaN and whatever it cannot hold -- so infinity
/// comes out 0. FFmpeg's `clap` arithmetic depends on it to reject what
/// lies outside the picture.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the conversion being modelled"
)]
pub(crate) fn to_u64_as_c(d: f64) -> u64 {
    const TWO_63: f64 = 9_223_372_036_854_775_808.0;
    // `cvttsd2si`: truncation toward zero, or 2^63 for what it cannot hold.
    let signed = |d: f64| {
        if d.is_nan() || !(-TWO_63..TWO_63).contains(&d) {
            1 << 63
        } else {
            (d as i64) as u64
        }
    };
    if d >= TWO_63 {
        signed(d - TWO_63) ^ (1 << 63)
    } else {
        signed(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(num: i32, den: i32) -> Q {
        Q { num, den }
    }

    #[test]
    fn reduce_gives_lowest_terms_and_the_sign_on_top() {
        assert_eq!(reduce(4, 6, MAX), (q(2, 3), true));
        assert_eq!(reduce(-4, 6, MAX), (q(-2, 3), true));
        assert_eq!(reduce(4, -6, MAX), (q(-2, 3), true));
        assert_eq!(reduce(0, 5, MAX), (q(0, 1), true));
        // 0/0 stays as it is; x/0 is 1/0.
        assert_eq!(reduce(0, 0, MAX), (q(0, 0), true));
        assert_eq!(reduce(7, 0, MAX), (q(1, 0), true));
    }

    #[test]
    fn reduce_finds_the_nearest_fraction_within_the_limit() {
        // pi's convergents under 1000: 355/113.
        let (p, exact) = reduce(3_141_592_653_589_793, 1_000_000_000_000_000, 1000);
        assert_eq!((p, exact), (q(355, 113), false));
        // 1001/30000 within 100: 1/30, nearer than the semiconvergent 3/89.
        assert_eq!(reduce(1001, 30000, 100), (q(1, 30), false));
        // 53/100 within 10: the semiconvergent 5/9, nearer than the last
        // convergent, 1/2.
        assert_eq!(reduce(53, 100, 10), (q(5, 9), false));
    }

    #[test]
    fn d2q_finds_simple_fractions_of_doubles() {
        assert_eq!(d2q(4.0 / 3.0, i32::MAX), q(4, 3));
        assert_eq!(d2q(0.5, i32::MAX), q(1, 2));
        assert_eq!(d2q(2.0, i32::MAX), q(2, 1));
        assert_eq!(d2q(f64::NAN, i32::MAX), q(0, 0));
        assert_eq!(d2q(1e300, i32::MAX), q(1, 0));
        assert_eq!(d2q(-1e300, i32::MAX), q(-1, 0));
    }

    #[test]
    fn the_arithmetic_is_ffmpegs() {
        assert_eq!(mul(q(2, 3), q(3, 4)), q(1, 2));
        assert_eq!(add(q(1, 2), q(1, 3)), q(5, 6));
        assert_eq!(sub(q(1, 2), q(1, 3)), q(1, 6));
        assert_eq!(cmp(q(1, 2), q(1, 3)), 1);
        assert_eq!(cmp(q(1, 3), q(1, 2)), -1);
        assert_eq!(cmp(q(2, 4), q(1, 2)), 0);
        assert_eq!(cmp(q(1, 0), q(2, 0)), 0);
        assert_eq!(cmp(q(0, 0), q(1, 2)), i32::MIN);
        assert!((q2d(q(1, 4)) - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn a_double_converts_to_unsigned_as_c_does() {
        assert_eq!(to_u64_as_c(3.9), 3);
        assert_eq!(to_u64_as_c(-3.0), (-3i64) as u64);
        assert_eq!(to_u64_as_c(f64::NAN), 1 << 63);
        assert_eq!(to_u64_as_c(f64::NEG_INFINITY), 1 << 63);
        assert_eq!(to_u64_as_c(f64::INFINITY), 0);
        assert_eq!(to_u64_as_c(1e19), 10_000_000_000_000_000_000);
        assert_eq!(to_u64_as_c(2e19), 0);
    }
}
